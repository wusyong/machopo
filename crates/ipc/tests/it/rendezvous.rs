//! `ipc`'s macOS rendezvous, across processes: the test starts itself
//! again as a child (this file's `child`, told what to do by an
//! environment variable), which asks the test's server for its ports.
//! What the child got is seen by what it does with them: it writes to the
//! test's port, where the kernel says which process wrote.

use std::process::{Child, Command, Stdio};
use std::time::Duration;

use ipc::platform::macos::message::{self, Buffer, Received, To};
use ipc::platform::macos::port::{ReceiveRight, Right, SendRight};
use ipc::platform::macos::rendezvous::{self, FetchError, Server};

const ROLE: &str = "MACHOPO_IPC_RENDEZVOUS_CHILD";
const ID: i32 = 0x7e57;
const SOON: Option<Duration> = Some(Duration::from_secs(10));

/// The test process's one server.
fn server() -> &'static Server {
    Server::global().expect("the rendezvous server checks in")
}

fn spawn(role: &str) -> Child {
    Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "rendezvous::child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, role)
        .stdout(Stdio::null())
        .spawn()
        .unwrap()
}

fn exit_code(mut child: Child) -> i32 {
    child.wait().unwrap().code().unwrap_or(-1)
}

/// A port of the test's, and the right to it a child is given.
fn port() -> (ReceiveRight, SendRight) {
    let receive = ReceiveRight::new().unwrap();
    let send = receive.make_send().unwrap();
    (receive, send)
}

fn receive(buffer: &mut Buffer, on: &ReceiveRight) -> Received {
    message::receive(buffer, on.name(), SOON).expect("the child's message")
}

// What a child exits with.
const DONE: i32 = 0;
const NO_SERVER: i32 = 11;
const REFUSED: i32 = 12;
const TIMED_OUT: i32 = 13;
const FAILED: i32 = 14;
const WRONG: i32 = 15;

fn code(error: FetchError) -> i32 {
    match error {
        FetchError::NoServer => NO_SERVER,
        FetchError::Refused => REFUSED,
        FetchError::TimedOut => TIMED_OUT,
        FetchError::Failed => FAILED,
    }
}

/// The child's side: does what `ROLE` says and exits with what came of
/// it. Without `ROLE` (the test run itself) it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let host = std::os::unix::process::parent_id();
    let wait = Duration::from_secs(10);
    let mut buffer = Buffer::for_body(64);
    let outcome = match role.as_str() {
        // Writes on the send right it got, then answers what comes on
        // the receive right it got.
        "talk" => match rendezvous::fetch(host, wait).as_deref_mut() {
            Ok(
                [
                    Right::Send(to_host),
                    Right::Receive(mine),
                    Right::Send(second),
                ],
            ) => {
                message::send(
                    &mut buffer,
                    To::Port(to_host),
                    None,
                    ID,
                    b"hello",
                    vec![],
                    None,
                )
                .unwrap();
                let asked = message::receive(&mut buffer, mine.name(), SOON).unwrap();
                let from_host = asked.sender.pid() == host.cast_signed();
                let mut answer = buffer.body(&asked).to_vec();
                answer.extend_from_slice(if from_host {
                    b" from the host"
                } else {
                    b" from another"
                });
                message::send(
                    &mut buffer,
                    To::Port(second),
                    None,
                    ID,
                    &answer,
                    vec![],
                    None,
                )
                .unwrap();
                DONE
            }
            Ok(_) => WRONG,
            Err(error) => code(*error),
        },
        // Asks again once answered.
        "twice" => match rendezvous::fetch(host, wait) {
            Ok(_) => match rendezvous::fetch(host, wait) {
                Ok(_) => WRONG,
                Err(error) => code(error),
            },
            Err(error) => code(error),
        },
        // Nothing is registered for it: it doesn't wait long.
        "unregistered" => match rendezvous::fetch(host, Duration::from_millis(400)) {
            Ok(_) => WRONG,
            Err(error) => code(error),
        },
        // Asks only after a while: what the host did meanwhile is done.
        "slow" => {
            std::thread::sleep(Duration::from_millis(700));
            match rendezvous::fetch(host, Duration::from_millis(400)) {
                Ok(_) => WRONG,
                Err(error) => code(error),
            }
        }
        // A host that has no server (launchd).
        "no server" => match rendezvous::fetch(1, wait) {
            Ok(_) => WRONG,
            Err(error) => code(error),
        },
        _ => WRONG,
    };
    std::process::exit(outcome);
}

#[test]
fn a_child_gets_the_rights_registered_for_it_in_order() {
    let (inbox, to_inbox) = port();
    let (second_inbox, to_second) = port();
    let childs = ReceiveRight::new().unwrap();
    let to_child = childs.make_send().unwrap();
    let child = spawn("talk");
    let pid = child.id();
    server().register(
        pid,
        vec![
            Right::Send(to_inbox),
            Right::Receive(childs),
            Right::Send(to_second),
        ],
    );
    let mut buffer = Buffer::for_body(64);
    let hello = receive(&mut buffer, &inbox);
    assert_eq!(buffer.body(&hello), b"hello");
    assert_eq!(
        hello.sender.pid(),
        pid.cast_signed(),
        "the kernel says the child wrote it"
    );
    // The receive right is the child's now: what is sent to the port
    // comes out there, and the answer on the third right.
    message::send(
        &mut buffer,
        To::Port(&to_child),
        None,
        ID,
        b"asked",
        vec![],
        None,
    )
    .unwrap();
    let answer = receive(&mut buffer, &second_inbox);
    assert_eq!(buffer.body(&answer), b"asked from the host");
    assert_eq!(answer.sender.pid(), pid.cast_signed());
    assert_eq!(exit_code(child), DONE);
}

#[test]
fn a_child_that_asks_before_it_is_registered_is_answered_when_it_is() {
    let (inbox, to_inbox) = port();
    let (second_inbox, to_second) = port();
    let childs = ReceiveRight::new().unwrap();
    let to_child = childs.make_send().unwrap();
    let child = spawn("talk");
    // Long enough for the child to have started and asked.
    std::thread::sleep(Duration::from_millis(700));
    server().register(
        child.id(),
        vec![
            Right::Send(to_inbox),
            Right::Receive(childs),
            Right::Send(to_second),
        ],
    );
    let mut buffer = Buffer::for_body(64);
    let hello = receive(&mut buffer, &inbox);
    assert_eq!(buffer.body(&hello), b"hello");
    message::send(
        &mut buffer,
        To::Port(&to_child),
        None,
        ID,
        b"late",
        vec![],
        None,
    )
    .unwrap();
    let answer = receive(&mut buffer, &second_inbox);
    assert_eq!(buffer.body(&answer), b"late from the host");
    assert_eq!(exit_code(child), DONE);
}

#[test]
fn a_child_is_answered_once() {
    let (_inbox, to_inbox) = port();
    let child = spawn("twice");
    server().register(child.id(), vec![Right::Send(to_inbox)]);
    assert_eq!(exit_code(child), REFUSED);
}

#[test]
fn a_process_nothing_is_registered_for_gets_nothing() {
    // Another child's rights are registered meanwhile: they aren't this
    // one's.
    let (inbox, to_inbox) = port();
    let other = spawn("twice");
    let unregistered = spawn("unregistered");
    server().register(other.id(), vec![Right::Send(to_inbox)]);
    assert_eq!(exit_code(unregistered), TIMED_OUT);
    assert_eq!(exit_code(other), REFUSED);
    drop(inbox);
}

#[test]
fn what_is_registered_can_be_taken_back() {
    let (inbox, to_inbox) = port();
    let child = spawn("slow");
    server().register(child.id(), vec![Right::Send(to_inbox)]);
    server().forget(child.id());
    // The child asks after both: nothing is registered for it.
    assert_eq!(exit_code(child), TIMED_OUT);
    // And the right that was registered was let go, not kept.
    assert!(inbox.tell_when_no_senders(0));
    let mut buffer = Buffer::for_body(64);
    let told = receive(&mut buffer, &inbox);
    assert_eq!(told.id, NO_SENDERS);
    assert_eq!(told.sender.pid(), 0);
}

/// `MACH_NOTIFY_NO_SENDERS` (`<mach/notify.h>`).
const NO_SENDERS: i32 = 0o106;

#[test]
fn a_second_question_from_a_process_takes_its_firsts_place() {
    // The test process itself asks its own server, which has nothing for
    // it: the first question waits, until a second from the same process
    // makes the server let go of it.
    let _ = server();
    let host = std::process::id();
    let first = std::thread::spawn(move || rendezvous::fetch(host, Duration::from_secs(10)));
    std::thread::sleep(Duration::from_millis(300));
    let second = rendezvous::fetch(host, Duration::from_millis(300));
    assert_eq!(second.unwrap_err(), FetchError::TimedOut);
    assert_eq!(first.join().unwrap().unwrap_err(), FetchError::Refused);
}

#[test]
fn a_host_without_a_server_is_no_server() {
    assert_eq!(exit_code(spawn("no server")), NO_SERVER);
}

#[test]
fn a_process_has_one_server() {
    let _ = server();
    assert!(Server::start().is_none(), "the name is checked in already");
    assert_eq!(
        rendezvous::name_for(42),
        "org.machopo.MachPortRendezvousServer.42"
    );
}
