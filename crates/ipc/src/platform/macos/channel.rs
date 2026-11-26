//! A channel's end over Mach ports (Chromium's `ChannelMac`): this end's
//! receive port and a send right to the peer's. One of the core's
//! messages is one Mach message, whole or not at all: in the message
//! itself up to [`INLINE_MAX`] bytes, out of line past that. Handles are
//! send rights (a region's memory entry), copied into the message.
//!
//! A process's IO thread waits on one port set ([`PortWaiter`]): the
//! port of each channel's end, where its peer's messages and the
//! kernel's notifications about it come, and a port of the thread's own
//! that other threads wake it by. A send never waits: to a full queue it
//! is refused, and the kernel is asked to say when there is room.
//!
//! Who sent a message is the kernel's word, not the message's: only the
//! peer's are taken (by its pid at first, then by the whole audit token
//! its first message came with), and a notification only when the kernel
//! sent it. A message with a notification's id from anyone else is
//! nothing.

use std::fs::File;
use std::io;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use mach2::notify::{MACH_NOTIFY_DEAD_NAME, MACH_NOTIFY_NO_SENDERS, MACH_NOTIFY_SEND_POSSIBLE};

use super::memory::{self, Mapping};
use super::message::{self, Buffer, ReceiveError, SendError, Sender};
use super::port::{PortSet, ReceiveRight, Right, SendRight};
use super::rendezvous::{self, Server};
use crate::core::channel::{Transport, WriteError};
use crate::core::frame::{HEADER_LEN, MAX_PAYLOAD_LEN};
use crate::endpoint::{Event, Key, Platform, Waited, Waiter, Wake};

/// The longest message that goes in the Mach message itself: past it the
/// kernel's mapping of the sender's pages costs less than copying them in
/// and out.
pub const INLINE_MAX: usize = 64 << 10;

/// `msgh_id` of one of the core's messages, and of a waiter's wake.
pub const MESSAGE_ID: i32 = 0x4d43_4831;
const WAKE_ID: i32 = 0x4d43_4857;

/// The longest message the core makes: its header and the longest
/// payload.
fn longest_message() -> usize {
    usize::try_from(MAX_PAYLOAD_LEN)
        .unwrap_or(usize::MAX)
        .saturating_add(HEADER_LEN)
}

/// How long a child waits for its end: a host registers it as soon as the
/// child is started, so the wait is the host's being busy.
const INHERIT_WAIT: Duration = Duration::from_secs(30);

/// Starts `command` as a child that has the other end of a new channel,
/// which it takes with [`inherited`]. A child can't inherit a Mach port:
/// its end waits for it at this process's rendezvous server, under its
/// pid.
///
/// If the child goes without taking its end, nothing tells this end so
/// (the rights it was to take are still held for it): whoever sees the
/// child exit calls [`child_gone`].
pub fn spawn(command: &mut Command) -> io::Result<(Child, Channel)> {
    let server =
        Server::global().ok_or_else(|| io::Error::other("the rendezvous server couldn't start"))?;
    let mut child = command.spawn()?;
    let Some((channel, rights)) = Channel::pair(child.id()) else {
        // No ports: the child would wait for an end that isn't coming.
        let _ = child.kill();
        return Err(io::Error::other("a Mach port couldn't be made"));
    };
    server.register(child.id(), rights);
    Ok((child, channel))
}

/// In a process started by [`spawn`]: its end of the channel. `None`
/// when the parent has none for it.
pub fn inherited() -> Option<Channel> {
    let host = std::os::unix::process::parent_id();
    let rights = rendezvous::fetch(host, INHERIT_WAIT).ok()?;
    Channel::from_rights(rights, host)
}

/// The child `pid` is gone: the end it never took is let go, which the
/// host's own end then hears of as its peer gone. Nothing, for a child
/// that took its end.
pub fn child_gone(pid: u32) {
    if let Some(server) = Server::global() {
        server.forget(pid);
    }
}

/// Who the peer is known to be.
enum Peer {
    /// Its pid, until its first message says more.
    Pid(i32),
    /// The audit token its first message came with: every later one
    /// comes with the same.
    Known(Sender),
}

/// What an end is, to its own value and to the waiter of the IO thread
/// that serves it.
struct End {
    /// To the peer's port; `None` once the channel is shut. Locked before
    /// `ours` wherever both are.
    theirs: Mutex<Option<SendRight>>,
    /// This end's port; `None` once the channel is shut.
    ours: Mutex<Option<ReceiveRight>>,
    /// The name of `ours`: what a message received on a set says it came
    /// to.
    port: mach2::port::mach_port_t,
    peer: Mutex<Peer>,
    /// Where a message is put together: one writer at a time (the
    /// channel's own lock sees to that).
    sending: Mutex<Buffer>,
}

/// A channel's end.
// An `Arc`: the IO thread's waiter reaches the end too (who its peer is,
// the right a dead-name notification is about), from the thread it
// waits on while others write.
pub struct Channel(Arc<End>);

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // Nothing panics while holding any of them.
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Channel {
    /// Both ends of a new channel, made by the host: its own, and the
    /// rights that are the peer's end, for the peer `peer_pid` to be
    /// given by the rendezvous.
    pub fn pair(peer_pid: u32) -> Option<(Self, Vec<Right>)> {
        let ours = ReceiveRight::new()?;
        let peers = ReceiveRight::new()?;
        let to_peers = peers.make_send()?;
        let to_ours = ours.make_send()?;
        let host = Self::new(ours, to_peers, peer_pid)?;
        Some((host, vec![Right::Receive(peers), Right::Send(to_ours)]))
    }

    /// The end the rights of [`Channel::pair`] are, in the process they
    /// were given to: `peer_pid` is the host's.
    pub fn from_rights(rights: Vec<Right>, peer_pid: u32) -> Option<Self> {
        let mut rights = rights.into_iter();
        let (Some(Right::Receive(ours)), Some(Right::Send(theirs)), None) =
            (rights.next(), rights.next(), rights.next())
        else {
            return None;
        };
        Self::new(ours, theirs, peer_pid)
    }

    fn new(ours: ReceiveRight, theirs: SendRight, peer_pid: u32) -> Option<Self> {
        // A port holds five messages unless told otherwise. The peer's
        // port dying, and the last right to this one going, are each the
        // peer gone: both are told to this end's port.
        let ready = ours.raise_queue_limit()
            && theirs.tell_when_dead(&ours)
            && ours.tell_when_no_senders(1);
        ready.then(|| {
            Self(Arc::new(End {
                port: ours.name(),
                theirs: Mutex::new(Some(theirs)),
                ours: Mutex::new(Some(ours)),
                peer: Mutex::new(Peer::Pid(peer_pid.cast_signed())),
                sending: Mutex::new(Buffer::with_out_of_line(INLINE_MAX, longest_message())),
            }))
        })
    }
}

impl Transport for Channel {
    type Handle = SendRight;

    fn write(&self, bytes: &[u8], handles: &mut Vec<SendRight>) -> Result<usize, WriteError> {
        let theirs = lock(&self.0.theirs);
        let Some(to) = theirs.as_ref() else {
            return Err(WriteError::Closed);
        };
        let mut buffer = lock(&self.0.sending);
        match message::try_send(&mut buffer, to, MESSAGE_ID, bytes, handles) {
            Ok(()) => {
                // The message has copies: these were this end's.
                handles.clear();
                Ok(bytes.len())
            }
            Err(SendError::TimedOut) => {
                // The peer's queue is full: the kernel is asked to say
                // when it has room (at once, if room came meanwhile).
                let asked = lock(&self.0.ours)
                    .as_ref()
                    .is_some_and(|ours| to.tell_when_sendable(ours));
                Err(if asked {
                    WriteError::WouldBlock
                } else {
                    WriteError::Closed
                })
            }
            // The peer's port is dead, or the message is one the core
            // never makes.
            Err(SendError::Kernel(_) | SendError::TooLarge) => Err(WriteError::Closed),
        }
    }
}

impl Platform for Channel {
    type Memory = Mapping;
    type Waiter = PortWaiter;

    fn map(&self, handle: SendRight, len: usize, writable: bool) -> Option<Mapping> {
        memory::map(&handle, len, writable)
    }

    fn create(&self, len: usize, peer_writes: bool) -> Option<(Mapping, SendRight)> {
        memory::create(len, peer_writes)
    }

    // Files don't cross on this platform yet: a descriptor isn't a port.
    fn send_file(&self, _: File) -> Option<SendRight> {
        None
    }

    fn file(&self, _: SendRight) -> Option<File> {
        None
    }

    fn shut(&self) {
        // The right to the peer's port, then this end's port: the peer's
        // kernel tells it of each. The port leaves its waiter's set as
        // it dies.
        let theirs = lock(&self.0.theirs).take();
        let ours = lock(&self.0.ours).take();
        drop((theirs, ours));
    }
}

/// What other threads end a waiter's wait by.
struct WakePort {
    to: SendRight,
    buffer: Mutex<Buffer>,
    /// A wake was asked for: looked at before every wait, so one that
    /// finds the wake port full isn't lost. The message only ends the
    /// wait.
    woken: AtomicBool,
}

/// What a process's IO thread waits with: a port set of its channels'
/// ports and a port to be woken by.
pub struct PortWaiter {
    set: PortSet,
    /// Held, not read: the port `wake` has the right to, in the set as
    /// long as this waiter is.
    _wake_port: ReceiveRight,
    wake: Arc<WakePort>,
    /// For the one message a wait receives.
    buffer: Buffer,
    channels: Vec<(Key, Arc<End>)>,
}

impl Waiter<Channel> for PortWaiter {
    fn new() -> Option<Self> {
        let wake_port = ReceiveRight::new()?;
        let to = wake_port.make_send()?;
        let set = PortSet::new()?;
        set.add(&wake_port).then(|| Self {
            set,
            _wake_port: wake_port,
            wake: Arc::new(WakePort {
                to,
                buffer: Mutex::new(Buffer::for_body(0)),
                woken: AtomicBool::new(false),
            }),
            buffer: Buffer::with_out_of_line(INLINE_MAX, longest_message()),
            channels: Vec::new(),
        })
    }

    fn interrupter(&self) -> Wake {
        let wake = self.wake.clone();
        Arc::new(move || {
            wake.woken.store(true, Ordering::SeqCst);
            // To a wake port already full the flag is enough: the IO
            // thread isn't waiting.
            let _ = message::try_send(&mut lock(&wake.buffer), &wake.to, WAKE_ID, &[], &[]);
        })
    }

    fn add(&mut self, key: Key, channel: &Channel) -> bool {
        // What came to the port before now is still in it: it is
        // received once the port is in the set.
        let added = lock(&channel.0.ours)
            .as_ref()
            .is_some_and(|ours| self.set.add(ours));
        if added {
            self.channels.push((key, channel.0.clone()));
        }
        added
    }

    fn remove(&mut self, key: Key) {
        self.channels.retain(|(served, _)| *served != key);
    }

    fn wait<R>(&mut self, then: impl FnOnce(Waited<'_, SendRight>) -> R) -> R {
        let mut handles = Vec::new();
        let (key, event, received) = loop {
            if self.wake.woken.swap(false, Ordering::SeqCst) {
                return then(Waited::Interrupted);
            }
            let received = match message::receive(&mut self.buffer, self.set.name(), None) {
                Ok(received) => received,
                // What is laid out otherwise than a message only a
                // channel's peer could have sent to its port: a peer
                // that does is no peer.
                Err(ReceiveError::Malformed) => match self.served(self.buffer.refused_at()) {
                    Some((key, _)) => break (key, Event::PeerGone, None),
                    None => continue,
                },
                // A message longer than the buffer is gone with nothing
                // to say whose it was: its channel finds the messages
                // after it out of order, and is closed then.
                Err(ReceiveError::TooLarge | ReceiveError::TimedOut | ReceiveError::Kernel(_)) => {
                    continue;
                }
            };
            // A wake (the flag says whether it is still to be told), or
            // a message to a port no longer served.
            let Some((key, end)) = self.served(received.port) else {
                continue;
            };
            if received.sender.pid() == 0 {
                // The kernel's.
                match received.id {
                    MACH_NOTIFY_DEAD_NAME => {
                        if let Some(theirs) = lock(&end.theirs).as_ref() {
                            theirs.dead_name_told();
                        }
                        break (key, Event::PeerGone, None);
                    }
                    MACH_NOTIFY_NO_SENDERS => break (key, Event::PeerGone, None),
                    MACH_NOTIFY_SEND_POSSIBLE => break (key, Event::Writable, None),
                    _ => continue,
                }
            }
            let mut peer = lock(&end.peer);
            let is_peer = match &*peer {
                Peer::Pid(pid) => received.sender.pid() == *pid,
                Peer::Known(sender) => received.sender == *sender,
            };
            // Another process's, or not one of the core's messages (one
            // with a notification's id, say): nothing.
            if !is_peer || received.id != MESSAGE_ID {
                continue;
            }
            *peer = Peer::Known(received.sender);
            break (key, Event::Read, Some(received));
        };
        let Some(mut received) = received else {
            return then(Waited::Channel {
                key,
                event,
                bytes: &[],
                handles: &mut handles,
            });
        };
        for right in std::mem::take(&mut received.rights) {
            // A handle is a send right. A receive right is let go: the
            // message then has fewer handles than it says, which the
            // core refuses.
            if let Right::Send(right) = right {
                handles.push(right);
            }
        }
        // The bytes are lent where they are: in the buffer, or in the
        // memory the message brought, which is unmapped after.
        then(Waited::Channel {
            key,
            event,
            bytes: self.buffer.body(&received),
            handles: &mut handles,
        })
    }
}

impl PortWaiter {
    /// The channel whose end's port is `port`, among those served. (The
    /// wake port is none of them.)
    fn served(&self, port: mach2::port::mach_port_t) -> Option<(Key, &End)> {
        self.channels
            .iter()
            .find(|(_, end)| end.port == port)
            .map(|(key, end)| (*key, &**end))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs a test that waits on a channel, which waits as long as it
    /// takes: a wait that never ends is the test failing, said so.
    fn within(test: impl FnOnce() + Send + 'static) {
        let (done, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            test();
            let _ = done.send(());
        });
        match finished.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(()) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                panic!("stuck: a wait on the channel never ended")
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("failed (above)"),
        }
    }

    /// An end and the waiter of the thread that serves it, as key 7.
    struct Served {
        channel: Channel,
        waiter: PortWaiter,
    }

    const KEY: Key = 7;

    impl Served {
        fn new(channel: Channel) -> Self {
            let mut waiter = PortWaiter::new().unwrap();
            assert!(waiter.add(KEY, &channel));
            Self { channel, waiter }
        }

        /// What the next wait comes back with: `None` when it was
        /// interrupted.
        fn next(&mut self) -> Option<(Event, Vec<u8>, usize)> {
            self.waiter.wait(|waited| match waited {
                Waited::Interrupted => None,
                Waited::Channel {
                    key,
                    event,
                    bytes,
                    handles,
                } => {
                    assert_eq!(key, KEY);
                    Some((event, bytes.to_vec(), handles.len()))
                }
            })
        }

        fn write(&self, bytes: &[u8]) -> Result<usize, WriteError> {
            self.channel.write(bytes, &mut Vec::new())
        }
    }

    /// Both ends in this process: each takes the other to be this
    /// process.
    fn ends() -> (Served, Served) {
        let own = std::process::id();
        let (host, rights) = Channel::pair(own).unwrap();
        (
            Served::new(host),
            Served::new(Channel::from_rights(rights, own).unwrap()),
        )
    }

    fn read(bytes: &[u8], handles: usize) -> Option<(Event, Vec<u8>, usize)> {
        Some((Event::Read, bytes.to_vec(), handles))
    }

    #[test]
    fn what_one_end_writes_the_other_is_lent_whole() {
        within(|| {
            let (mut host, mut child) = ends();
            for len in [0usize, 1, INLINE_MAX, INLINE_MAX + 1, 1 << 20] {
                let bytes: Vec<u8> = (0..len).map(|i| (i as u8).wrapping_mul(3)).collect();
                assert_eq!(host.write(&bytes), Ok(len));
                assert!(child.next() == read(&bytes, 0), "{len} bytes to the child");
                assert_eq!(child.write(&bytes), Ok(len));
                assert!(host.next() == read(&bytes, 0), "{len} bytes to the host");
            }
        });
    }

    #[test]
    fn handles_go_with_their_message_and_leave_the_writers_hands() {
        within(|| {
            let (host, mut child) = ends();
            let (mapping, handle) = memory::create(INLINE_MAX, false).unwrap();
            let mut handles = vec![handle];
            assert_eq!(host.channel.write(b"with a region", &mut handles), Ok(13));
            assert!(handles.is_empty());
            let Served { channel, waiter } = &mut child;
            let mapped = waiter.wait(|waited| {
                let Waited::Channel {
                    event,
                    bytes,
                    handles,
                    ..
                } = waited
                else {
                    panic!("a wake");
                };
                assert_eq!(
                    (event, bytes, handles.len()),
                    (Event::Read, &b"with a region"[..], 1)
                );
                channel.map(handles.pop().unwrap(), INLINE_MAX, false)
            });
            // The region the handle is: what the maker writes is read
            // there.
            // SAFETY: inside both mappings; nothing else touches them.
            unsafe {
                crate::core::region::Memory::ptr(&mapping).write(0x42);
                assert_eq!(
                    crate::core::region::Memory::ptr(&mapped.unwrap()).read(),
                    0x42
                );
            }
        });
    }

    #[test]
    fn a_wait_is_interrupted_once_however_many_times_it_was_asked() {
        within(|| {
            let (mut host, child) = ends();
            // More than the wake port holds.
            let interrupt = host.waiter.interrupter();
            for _ in 0..20 {
                interrupt();
            }
            assert_eq!(host.next(), None);
            // The messages still in the wake port are of that one
            // telling: the next thing told is what comes next.
            assert_eq!(child.write(b"after"), Ok(5));
            assert_eq!(host.next(), read(b"after", 0));
            interrupt();
            assert_eq!(host.next(), None);
            // From another thread, with the wait under way.
            let from_elsewhere = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(50));
                interrupt();
            });
            assert_eq!(host.next(), None);
            from_elsewhere.join().unwrap();
        });
    }

    #[test]
    fn one_waiter_tells_its_channels_apart() {
        within(|| {
            let own = std::process::id();
            let mut waiter = PortWaiter::new().unwrap();
            let mut peers = Vec::new();
            for key in [3, 9, 4] {
                let (host, rights) = Channel::pair(own).unwrap();
                assert!(waiter.add(key, &host));
                peers.push((key, host, Channel::from_rights(rights, own).unwrap()));
            }
            let next = |waiter: &mut PortWaiter| {
                waiter.wait(|waited| match waited {
                    Waited::Interrupted => None,
                    Waited::Channel {
                        key, event, bytes, ..
                    } => Some((key, event, bytes.to_vec())),
                })
            };
            // Written before it was waited on, and after.
            let (_, _, late) = {
                let (host, rights) = Channel::pair(own).unwrap();
                let child = Channel::from_rights(rights, own).unwrap();
                child.write(b"early", &mut Vec::new()).unwrap();
                assert!(waiter.add(1, &host));
                (1, host, child)
            };
            assert_eq!(next(&mut waiter), Some((1, Event::Read, b"early".to_vec())));
            for (key, _, child) in &peers {
                child.write(&[*key as u8], &mut Vec::new()).unwrap();
                assert_eq!(
                    next(&mut waiter),
                    Some((*key, Event::Read, vec![*key as u8]))
                );
            }
            // One peer gone is that channel's alone.
            let (key, host, child) = peers.remove(1);
            drop(child);
            assert_eq!(next(&mut waiter), Some((key, Event::PeerGone, vec![])));
            // No longer served, what comes to its port is nothing.
            waiter.remove(key);
            drop(host);
            late.write(b"late", &mut Vec::new()).unwrap();
            assert_eq!(next(&mut waiter), Some((1, Event::Read, b"late".to_vec())));
            // An end shut can't join.
            let (host, _rights) = Channel::pair(own).unwrap();
            host.shut();
            assert!(!waiter.add(2, &host));
        });
    }

    #[test]
    fn an_end_shut_sends_nothing_and_its_peer_is_told() {
        within(|| {
            let (host, mut child) = ends();
            assert_eq!(host.write(b"before"), Ok(6));
            host.channel.shut();
            assert_eq!(host.write(b"after"), Err(WriteError::Closed));
            // What was sent before still comes; then the kernel's word.
            assert_eq!(child.next(), read(b"before", 0));
            assert_eq!(child.next().unwrap().0, Event::PeerGone);
            assert_eq!(child.write(b"to nobody"), Err(WriteError::Closed));
        });
    }

    #[test]
    fn an_end_dropped_is_an_end_gone() {
        within(|| {
            let (mut host, child) = ends();
            drop(child);
            assert_eq!(host.next().unwrap().0, Event::PeerGone);
        });
    }

    #[test]
    fn a_full_queue_refuses_and_says_when_it_has_room() {
        within(|| {
            let (mut host, mut child) = ends();
            let mut written = 0;
            let refused = loop {
                match host.write(b"fill") {
                    Ok(_) => written += 1,
                    Err(error) => break error,
                }
                assert!(written < 10_000);
            };
            assert_eq!(refused, WriteError::WouldBlock);
            assert_eq!(written, 1024, "a port made as long as one can be");
            // A handle refused with its message is still the writer's.
            let (_mapping, handle) = memory::create(INLINE_MAX, false).unwrap();
            let mut handles = vec![handle];
            assert_eq!(
                host.channel.write(b"refused", &mut handles),
                Err(WriteError::WouldBlock)
            );
            assert_eq!(handles.len(), 1);
            // The child reads one: the host is told there is room, and
            // the same write goes.
            assert_eq!(child.next().unwrap().0, Event::Read);
            assert_eq!(host.next().unwrap().0, Event::Writable);
            assert_eq!(host.channel.write(b"refused", &mut handles), Ok(7));
            assert!(handles.is_empty());
        });
    }

    /// An end whose peer is these rights in the test's hands: the peer's
    /// port, and a right to the end's own.
    fn end_by_hand() -> (Served, ReceiveRight, SendRight) {
        let ours = ReceiveRight::new().unwrap();
        let to_ours = ours.make_send().unwrap();
        let peers = ReceiveRight::new().unwrap();
        let to_peers = peers.make_send().unwrap();
        let end = Channel::new(ours, to_peers, std::process::id()).unwrap();
        (Served::new(end), peers, to_ours)
    }

    #[test]
    fn the_peers_port_dying_alone_is_the_peer_gone() {
        within(|| {
            let (mut end, peers, to_ours) = end_by_hand();
            let to_peers = lock(&end.channel.0.theirs).as_ref().unwrap().name();
            drop(peers);
            // A right to this end's port is still held: only the dead
            // name says so.
            assert_eq!(end.next().unwrap().0, Event::PeerGone);
            drop(to_ours);
            // The reference the notification came with was let go: once
            // the end lets go of its own, the name is no more.
            end.channel.shut();
            // SAFETY: a name the end owned and let go; if the call
            // succeeds the test fails.
            let gone = unsafe {
                mach2::mach_port::mach_port_deallocate(mach2::traps::mach_task_self(), to_peers)
                    != mach2::kern_return::KERN_SUCCESS
            };
            assert!(gone, "a reference to the dead name was kept");
        });
    }

    #[test]
    fn the_last_right_to_this_port_going_alone_is_the_peer_gone() {
        within(|| {
            let (mut end, peers, to_ours) = end_by_hand();
            drop(to_ours);
            // The peer's port is alive: only this port's having no
            // senders says so.
            assert_eq!(end.next().unwrap().0, Event::PeerGone);
            drop(peers);
        });
    }

    #[test]
    fn a_receive_right_isnt_a_handle() {
        within(|| {
            let (mut end, _peers, to_ours) = end_by_hand();
            let carried = ReceiveRight::new().unwrap();
            let region = memory::create(INLINE_MAX, false).unwrap().1;
            let mut buffer = Buffer::for_body(64);
            let rights = vec![Right::Send(region), Right::Receive(carried)];
            message::send(
                &mut buffer,
                message::To::Port(&to_ours),
                None,
                MESSAGE_ID,
                b"two rights",
                rights,
                None,
            )
            .unwrap();
            assert_eq!(end.next(), read(b"two rights", 1), "the send right alone");
        });
    }

    #[test]
    fn an_end_is_made_only_of_its_two_rights() {
        // Each with its host's end kept: an end can't be made for a host
        // that is gone.
        let pair = || Channel::pair(1).unwrap();
        let (_host, rights) = pair();
        assert!(Channel::from_rights(rights, 1).is_some());
        let (_host, mut rights) = pair();
        rights.pop();
        assert!(Channel::from_rights(rights, 1).is_none());
        assert!(Channel::from_rights(Vec::new(), 1).is_none());
        let (_host, mut rights) = pair();
        rights.reverse();
        assert!(Channel::from_rights(rights, 1).is_none());
        let (_host, mut rights) = pair();
        let (_other, more) = pair();
        rights.extend(more);
        assert!(Channel::from_rights(rights, 1).is_none());
        // The host gone before its child made its end.
        let (host, rights) = pair();
        drop(host);
        assert!(Channel::from_rights(rights, 1).is_none());
    }
}
