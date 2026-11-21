//! How a child gets its ports (Chromium's `MachPortRendezvous`): a child
//! can't inherit a Mach port, so the host checks a server in with
//! launchd's bootstrap under a name of its own pid; a child looks the
//! name of its parent's pid up and asks; the server answers each asker,
//! known by the kernel's audit token and not by anything it says, with
//! the rights registered for its pid, once.
//!
//! A child may ask before the host has registered it (the host learns
//! its pid only once it is spawned): the question waits for the
//! registration. A process nothing is registered for gets no answer, and
//! one that was answered is refused if it asks again.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::CString;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mach2::bootstrap::{bootstrap_check_in, bootstrap_look_up, bootstrap_port};
use mach2::kern_return::KERN_SUCCESS;
use mach2::notify::MACH_NOTIFY_SEND_ONCE;
use mach2::port::MACH_PORT_NULL;

use super::message::{self, Buffer, ReceiveError, To};
use super::port::{ReceiveRight, Right, SendOnceRight, SendRight};

/// `msgh_id` of a child's question and of the server's answer.
const REQUEST: i32 = 0x4d52_5131;
const REPLY: i32 = 0x4d52_5132;

/// How many questions wait for a registration at most: past it the
/// oldest is refused, so processes nothing will ever be registered for
/// can't make the server keep a right for each.
const MAX_WAITING: usize = 64;

/// How long the server gives an answer to be taken: an asker's own reply
/// port has room, unless the asker filled it.
const ANSWER_WAIT: Duration = Duration::from_secs(1);

/// The name the server of the host `host_pid` is checked in under.
pub fn name_for(host_pid: u32) -> String {
    format!("org.machopo.MachPortRendezvousServer.{host_pid}")
}

#[derive(Default)]
struct Registry {
    /// The rights each child gets, until it asks.
    registered: HashMap<i32, Vec<Right>>,
    /// Questions from processes nothing is registered for yet, oldest
    /// first, each with the right to answer it.
    waiting: VecDeque<(i32, SendOnceRight)>,
    /// The children answered: one that asks again is refused.
    answered: HashSet<i32>,
}

/// The host's side: one a process, serving on a thread of its own for as
/// long as the process lives.
pub struct Server {
    registry: Arc<Mutex<Registry>>,
}

fn lock(registry: &Mutex<Registry>) -> MutexGuard<'_, Registry> {
    // Nothing panics while holding it: answers are sent after it is let
    // go.
    registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Server {
    /// Checks the server in under this process's name and starts serving.
    /// `None` when the name can't be checked in (it already was: there is
    /// one server a process) or the thread can't be started.
    pub fn start() -> Option<Self> {
        let name = CString::new(name_for(std::process::id())).ok()?;
        let mut port = MACH_PORT_NULL;
        // SAFETY: a call with a C string and a place for one name, both
        // alive for it.
        let result = unsafe { bootstrap_check_in(bootstrap_port, name.as_ptr(), &raw mut port) };
        if result != KERN_SUCCESS {
            return None;
        }
        // SAFETY: the check-in succeeded: `port` is the service's receive
        // right, this task's alone.
        let port = unsafe { ReceiveRight::from_raw(port) };
        let registry = Arc::new(Mutex::new(Registry::default()));
        let serving = registry.clone();
        std::thread::Builder::new()
            .name("rendezvous".into())
            .spawn(move || serve(&port, &serving))
            .ok()?;
        Some(Self { registry })
    }

    /// The rights the child `pid` gets when it asks, in this order. It is
    /// answered now if it asked already. What was registered for the pid
    /// before, and not asked for, is let go.
    pub fn register(&self, pid: u32, rights: Vec<Right>) {
        let pid = pid.cast_signed();
        let mut registry = lock(&self.registry);
        registry.answered.remove(&pid);
        let asked = registry.waiting.iter().position(|(asker, _)| *asker == pid);
        match asked.and_then(|i| registry.waiting.remove(i)) {
            Some((_, reply)) => {
                registry.answered.insert(pid);
                drop(registry);
                answer(reply, rights);
            }
            None => {
                registry.registered.insert(pid, rights);
            }
        }
    }

    /// Lets go of what is registered for `pid` (a child that went before
    /// it asked).
    pub fn forget(&self, pid: u32) {
        lock(&self.registry).registered.remove(&pid.cast_signed());
    }
}

fn answer(reply: SendOnceRight, rights: Vec<Right>) {
    // The answer is as long as its rights; a failure lets them go, and
    // the asker is told by the kernel that no answer is coming.
    let mut buffer = Buffer::for_body(0);
    let _ = message::send(
        &mut buffer,
        To::Reply(reply),
        None,
        REPLY,
        &[],
        rights,
        Some(ANSWER_WAIT),
    );
}

fn serve(port: &ReceiveRight, registry: &Mutex<Registry>) {
    // A question has no body.
    let mut buffer = Buffer::for_body(0);
    loop {
        let request = match message::receive(&mut buffer, port.name(), None) {
            Ok(request) => request,
            Err(ReceiveError::TooLarge | ReceiveError::Malformed | ReceiveError::TimedOut) => {
                continue;
            }
            // The port itself is no more: nothing is left to serve.
            Err(ReceiveError::Kernel(_)) => return,
        };
        // A question, with nothing in it and a port for the answer:
        // anything else is let go, with whatever rights it brought.
        let empty = buffer.body(&request).is_empty() && request.rights.is_empty();
        let (REQUEST, true, Some(reply)) = (request.id, empty, request.reply) else {
            continue;
        };
        let pid = request.sender.pid();
        let mut registry = lock(registry);
        if let Some(rights) = registry.registered.remove(&pid) {
            registry.answered.insert(pid);
            drop(registry);
            answer(reply, rights);
        } else if registry.answered.contains(&pid) {
            // Dropping the right to answer tells the asker, through the
            // kernel, that it gets nothing.
            drop(registry);
            drop(reply);
        } else {
            // Not registered yet, or never to be: the question waits. A
            // second from the same process takes the first's place.
            let refused: Vec<_> = {
                let again = registry.waiting.iter().position(|(asker, _)| *asker == pid);
                let mut refused: Vec<_> = again
                    .and_then(|i| registry.waiting.remove(i))
                    .into_iter()
                    .collect();
                registry.waiting.push_back((pid, reply));
                if registry.waiting.len() > MAX_WAITING {
                    refused.extend(registry.waiting.pop_front());
                }
                refused
            };
            drop(registry);
            drop(refused);
        }
    }
}

/// Why a child got no rights.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchError {
    /// No server is checked in for that host.
    NoServer,
    /// The server refused: this process was answered already, or the
    /// server went.
    Refused,
    /// No answer came in the time given: nothing is registered for this
    /// process.
    TimedOut,
    /// A system call failed.
    Failed,
}

/// A child's side: the rights registered for this process with the
/// server of `host_pid`, in the order registered. Waits `wait` for the
/// answer.
pub fn fetch(host_pid: u32, wait: Duration) -> Result<Vec<Right>, FetchError> {
    let name = CString::new(name_for(host_pid)).map_err(|_| FetchError::Failed)?;
    let mut server = MACH_PORT_NULL;
    // SAFETY: a call with a C string and a place for one name, both alive
    // for it.
    let result = unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &raw mut server) };
    if result != KERN_SUCCESS {
        return Err(FetchError::NoServer);
    }
    // SAFETY: the look-up succeeded: `server` is a send right it gave
    // this task.
    let server = unsafe { SendRight::from_raw(server) };
    let answers = ReceiveRight::new().ok_or(FetchError::Failed)?;
    let mut buffer = Buffer::for_body(0);
    message::send(
        &mut buffer,
        To::Port(&server),
        Some(&answers),
        REQUEST,
        &[],
        Vec::new(),
        Some(wait),
    )
    .map_err(|_| FetchError::Failed)?;
    match message::receive(&mut buffer, answers.name(), Some(wait)) {
        // Only the server holds the right to answer; that the answer is
        // its own is the kernel's word.
        Ok(reply) if reply.id == REPLY && reply.sender.pid() == host_pid.cast_signed() => {
            Ok(reply.rights)
        }
        // The right to answer was let go unused.
        Ok(told) if told.id == MACH_NOTIFY_SEND_ONCE && told.sender.pid() == 0 => {
            Err(FetchError::Refused)
        }
        Err(ReceiveError::TimedOut) => Err(FetchError::TimedOut),
        _ => Err(FetchError::Failed),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::OnceLock;

    use super::*;

    /// The test process's one server.
    fn server() -> &'static Server {
        static SERVER: OnceLock<Server> = OnceLock::new();
        SERVER.get_or_init(|| Server::start().expect("the rendezvous server checks in"))
    }

    /// A send right to this process's own server.
    fn look_up() -> SendRight {
        let name = CString::new(name_for(std::process::id())).unwrap();
        let mut port = MACH_PORT_NULL;
        // SAFETY: a call with a C string and a place for one name.
        let result = unsafe { bootstrap_look_up(bootstrap_port, name.as_ptr(), &raw mut port) };
        assert_eq!(result, KERN_SUCCESS);
        // SAFETY: the send right the look-up gave this task.
        unsafe { SendRight::from_raw(port) }
    }

    #[test]
    fn only_a_question_is_answered() {
        let own = std::process::id();
        let inbox = ReceiveRight::new().unwrap();
        server().register(own, vec![Right::Send(inbox.make_send().unwrap())]);
        let to_server = look_up();
        let mut buffer = Buffer::for_body(64);
        let soon = Some(Duration::from_secs(5));
        // A message that isn't a question, though it names a port for the
        // answer: no answer, and the right to give one is let go.
        let answers = ReceiveRight::new().unwrap();
        message::send(
            &mut buffer,
            To::Port(&to_server),
            Some(&answers),
            REQUEST + 7,
            &[],
            vec![],
            soon,
        )
        .unwrap();
        let told = message::receive(&mut buffer, answers.name(), soon).unwrap();
        assert_eq!((told.id, told.sender.pid()), (MACH_NOTIFY_SEND_ONCE, 0));
        // A question with nowhere to answer: nothing happens.
        message::send(
            &mut buffer,
            To::Port(&to_server),
            None,
            REQUEST,
            &[],
            vec![],
            soon,
        )
        .unwrap();
        // A question with a body: longer than a question is.
        message::send(
            &mut buffer,
            To::Port(&to_server),
            Some(&answers),
            REQUEST,
            b"padding",
            vec![],
            soon,
        )
        .unwrap();
        let told = message::receive(&mut buffer, answers.name(), soon).unwrap();
        assert_eq!((told.id, told.sender.pid()), (MACH_NOTIFY_SEND_ONCE, 0));
        // None of it took what is registered: a question gets it.
        let rights = fetch(own, Duration::from_secs(5)).unwrap();
        assert!(matches!(&rights[..], [Right::Send(_)]), "{rights:?}");
        // And only once.
        assert_eq!(
            fetch(own, Duration::from_secs(5)).unwrap_err(),
            FetchError::Refused
        );
    }
}
