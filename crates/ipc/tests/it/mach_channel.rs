//! `ipc`'s macOS channel between real processes: the test is the host,
//! with a whole endpoint over a Mach channel, and starts itself again as
//! the child (this file's `child`, told what to be by an environment
//! variable), which gets its end by the rendezvous. Some children are
//! endpoints that answer what they are sent; some send, by hand, what a
//! child of ours never would.

use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use bytes::Bytes;
use ipc::core::channel::{Gone, MAX_QUEUED_BYTES};
use ipc::core::frame::{BadMessage, Encoder};
use ipc::data_pipe::{self, Read, TryWriteError};
use ipc::endpoint::{self, Config, Handler, Link, Received};
use ipc::platform::macos::channel::{Channel, MESSAGE_ID};
use ipc::platform::macos::memory;
use ipc::platform::macos::message::{self, Buffer};
use ipc::platform::macos::port::{ReceiveRight, Right, SendRight};
use ipc::platform::macos::rendezvous;
use serde::{Deserialize, Serialize};

use super::rendezvous::server;

const ROLE: &str = "MACHOPO_IPC_CHANNEL_CHILD";
const KIB: usize = 1 << 10;
const SOON: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Msg {
    Text(String),
    Body(Bytes),
    Stream { pipe: u32 },
}

fn text(s: &str) -> Msg {
    Msg::Text(s.into())
}

fn pattern(len: usize, salt: u8) -> Bytes {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
        .collect()
}

fn config(host: bool) -> Config {
    Config {
        destinations: 3,
        may_attach: host,
        peer_may_attach: !host,
        peer_may_open_pipes: !host,
        region_limit: 8 << 20,
        queue_limit: MAX_QUEUED_BYTES,
    }
}

// What a child exits with: why its channel was over, or what it found.
const CLOSED: i32 = 20;
const PEER_CLOSED: i32 = 21;
const BAD_MESSAGE: i32 = 22;
const OTHER: i32 = 23;
const SETUP: i32 = 24;
const WRITABLE: i32 = 31;
const UNREADABLE: i32 = 32;

fn gone_code(gone: Gone) -> i32 {
    match gone {
        Gone::Closed => CLOSED,
        Gone::PeerClosed => PEER_CLOSED,
        Gone::BadMessage(_) => BAD_MESSAGE,
        Gone::QueueFull => OTHER,
    }
}

/// A child of ours: answers what it is sent.
struct Echo {
    link: Link<Channel>,
}

impl Handler<Channel> for Echo {
    fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
        let destination = message.destination();
        let answer = match message.decode::<Msg>()? {
            Msg::Text(text) if text == "quit" => {
                self.link.close();
                return Ok(());
            }
            Msg::Text(text) => Msg::Text(format!("echo {text}")),
            Msg::Body(body) => Msg::Body(body),
            Msg::Stream { pipe } => {
                let mut reader = message.take_pipe(pipe).ok_or(BadMessage::Rejected)?;
                let link = self.link.clone();
                thread::spawn(move || {
                    let (mut len, mut sum) = (0usize, 0u64);
                    loop {
                        match reader.read(48 * KIB) {
                            Read::Data(bytes) => {
                                len += bytes.len();
                                sum += bytes.iter().map(|byte| u64::from(*byte)).sum::<u64>();
                            }
                            Read::Empty => thread::yield_now(),
                            Read::Done => break,
                        }
                    }
                    let _ = link.send(0, &Msg::Text(format!("pipe {len} {sum}")));
                });
                return Ok(());
            }
        };
        // To a channel that is over there is nobody to answer.
        let _ = self.link.send(destination, &answer);
        Ok(())
    }
}

/// The child's end of the channel, by the rendezvous.
fn connect_child() -> Option<Channel> {
    let host = std::os::unix::process::parent_id();
    let rights = rendezvous::fetch(host, SOON).ok()?;
    Channel::from_rights(rights, host)
}

/// The rights of the child's end, for a child that sends by hand.
fn raw_rights() -> Option<(ReceiveRight, SendRight)> {
    let host = std::os::unix::process::parent_id();
    let mut rights = rendezvous::fetch(host, SOON).ok()?.into_iter();
    match (rights.next(), rights.next()) {
        (Some(Right::Receive(mine)), Some(Right::Send(to_host))) => Some((mine, to_host)),
        _ => None,
    }
}

/// One of the core's messages as its bytes.
fn frame(encoder: &mut Encoder, destination: u32, msg: &Msg, handles: usize) -> Vec<u8> {
    let payload = postcard::to_allocvec(msg).unwrap();
    let mut bytes = Vec::new();
    encoder
        .encode(destination, &payload, handles, &mut bytes)
        .unwrap();
    bytes
}

/// The child's side: is what `ROLE` says and exits with what came of it.
/// Without `ROLE` (the test run itself) it does nothing.
#[test]
fn child() {
    let Ok(role) = std::env::var(ROLE) else {
        return;
    };
    let outcome = match role.as_str() {
        "echo" | "sleepy" => connect_child().map_or(SETUP, |channel| {
            if role == "sleepy" {
                thread::sleep(Duration::from_millis(1500));
            }
            let (link, endpoint) = endpoint::endpoint(channel, config(false));
            gone_code(endpoint.run(Echo { link }))
        }),
        // Has its end and goes, without a word.
        "die" => connect_child().map_or(SETUP, |_channel| 7),
        other => raw_rights().map_or(SETUP, |(mine, to_host)| rogue(other, &mine, &to_host)),
    };
    std::process::exit(outcome);
}

/// A child that isn't ours: what it sends is made by hand.
fn rogue(role: &str, mine: &ReceiveRight, to_host: &SendRight) -> i32 {
    let mut buffer = Buffer::with_out_of_line(64 * KIB, 256 << 20);
    let mut encoder = Encoder::new();
    let send = |buffer: &mut Buffer, id: i32, body: &[u8], rights: &[SendRight]| {
        message::try_send(buffer, to_host, id, body, rights).is_ok()
    };
    match role {
        // Messages with the ids of the kernel's notifications, and one of
        // no id the channel knows; then a good message.
        "forged" => {
            // Each with a body that would be a good message, were its id
            // the core's.
            let forged = frame(&mut Encoder::new(), 0, &text("forged"), 0);
            for id in [0o110, 0o106, 0o102, 0o107, 12345] {
                assert!(send(&mut buffer, id, &forged, &[]));
            }
            let good = frame(&mut encoder, 1, &text("still here"), 0);
            assert!(send(&mut buffer, MESSAGE_ID, &good, &[]));
            wait_for_the_host_to_go(mine)
        }
        // A good message with a right attached, which a child may not.
        "handle" => {
            let own = mine.make_send().unwrap();
            let with_handle = frame(&mut encoder, 0, &text("with a handle"), 1);
            assert!(send(&mut buffer, MESSAGE_ID, &with_handle, &[own]));
            wait_for_the_host_to_go(mine)
        }
        // Bytes that are no message of the core's.
        "garbage" => {
            assert!(send(&mut buffer, MESSAGE_ID, &[0xff; 40], &[]));
            wait_for_the_host_to_go(mine)
        }
        // More out of line than any message of the core's is long.
        "huge" => {
            let huge = vec![0u8; (128 << 20) + 64 * KIB];
            assert!(send(&mut buffer, MESSAGE_ID, &huge, &[]));
            wait_for_the_host_to_go(mine)
        }
        // The host sends large bytes: the region they are in comes as a
        // right. Whether this process can write what it should only read.
        "map" => loop {
            let Ok(received) = message::receive(&mut buffer, mine.name(), Some(SOON)) else {
                break SETUP;
            };
            if let Some(Right::Send(region)) = received.rights.first() {
                let len = 256 * KIB;
                if memory::map(region, len, true).is_some() {
                    break WRITABLE;
                }
                break match memory::map(region, len, false) {
                    Some(_) => 0,
                    None => UNREADABLE,
                };
            }
        },
        _ => SETUP,
    }
}

/// Waits for the host's end to go: the kernel says so on this port (the
/// last right to it is let go). What the host says meanwhile is let go.
fn wait_for_the_host_to_go(mine: &ReceiveRight) -> i32 {
    assert!(mine.tell_when_no_senders(1));
    let mut buffer = Buffer::with_out_of_line(64 * KIB, 256 << 20);
    loop {
        match message::receive(&mut buffer, mine.name(), Some(SOON)) {
            Ok(told) if told.sender.pid() == 0 && told.id == 0o106 => return PEER_CLOSED,
            Ok(_) => {}
            Err(_) => return OTHER,
        }
    }
}

/// Sends what the host's endpoint hands up to the test.
struct Forward(mpsc::Sender<(u32, Msg)>);

impl Handler<Channel> for Forward {
    fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
        let msg = message.decode::<Msg>()?;
        let _ = self.0.send((message.destination(), msg));
        Ok(())
    }
}

/// The host's end, its IO thread, and the child.
struct Host {
    link: Link<Channel>,
    got: mpsc::Receiver<(u32, Msg)>,
    io: JoinHandle<Gone>,
    child: Child,
}

/// Starts a child as `role` and connects to it. `known_as` is the pid the
/// host takes the child to have (its own, unless the test says).
fn start(role: &str, known_as: Option<u32>) -> Host {
    let child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "mach_channel::child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(ROLE, role)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let (channel, rights) = Channel::pair(known_as.unwrap_or(child.id())).unwrap();
    server().register(child.id(), rights);
    let (link, endpoint) = endpoint::endpoint(channel, config(true));
    let (forward, got) = mpsc::channel();
    let io = thread::spawn(move || endpoint.run(Forward(forward)));
    Host {
        link,
        got,
        io,
        child,
    }
}

impl Host {
    fn next(&self) -> (u32, Msg) {
        self.got.recv_timeout(SOON).expect("the child's message")
    }

    /// Why the host's channel was over, and what the child exited with.
    fn end(mut self) -> (Gone, i32) {
        let (done, finished) = mpsc::channel();
        let io = self.io;
        thread::spawn(move || done.send(io.join().unwrap()));
        let gone = match finished.recv_timeout(SOON) {
            Ok(gone) => gone,
            Err(_) => {
                let _ = self.child.kill();
                panic!("the host's channel was never over");
            }
        };
        // A child that doesn't go by itself is a failure too, not a
        // wait for ever.
        let waited = std::time::Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return (gone, status.code().unwrap_or(-1));
            }
            if waited.elapsed() > SOON {
                let _ = self.child.kill();
                panic!("the child never exited (the host's channel was over: {gone:?})");
            }
            thread::sleep(Duration::from_millis(10));
        }
    }
}

#[test]
fn a_host_and_a_child_talk() {
    let host = start("echo", None);
    host.link.send(0, &text("one")).unwrap();
    host.link.send(2, &text("two")).unwrap();
    assert_eq!(host.next(), (0, text("echo one")));
    assert_eq!(host.next(), (2, text("echo two")));

    // Large bytes to the child go by a region the host makes; back, in
    // the message (out of line: the child has no region).
    let body = pattern(300 * KIB, 1);
    host.link.send(1, &Msg::Body(body.clone())).unwrap();
    assert_eq!(host.next(), (1, Msg::Body(body)));
    assert_eq!(
        host.link.regions(),
        (512 * KIB, 512 * KIB),
        "made, read by the child, free again"
    );
    // Given a region, the child's go by it too.
    host.link.grant_to_peer(512 * KIB);
    for salt in 2..6 {
        let body = pattern(400 * KIB, salt);
        host.link.send(0, &Msg::Body(body.clone())).unwrap();
        assert_eq!(host.next(), (0, Msg::Body(body)));
    }
    assert_eq!(
        host.link.regions().0,
        512 * KIB,
        "the one region, used again"
    );

    // A long message that isn't bytes: out of line, both ways.
    let long = "long text ".repeat(300_000);
    host.link.send(1, &text(&long)).unwrap();
    assert_eq!(host.next(), (1, Msg::Text(format!("echo {long}"))));

    // A pipe.
    let stream = pattern(700 * KIB, 9);
    let (mut source, consumer) = data_pipe::create(128 * KIB);
    let pipe = host.link.open_pipe(consumer, 128 * KIB as u32).unwrap();
    host.link.send(0, &Msg::Stream { pipe }).unwrap();
    let mut at = 0;
    let started = std::time::Instant::now();
    while at < stream.len() {
        let end = (at + 40 * KIB).min(stream.len());
        assert!(
            started.elapsed() < SOON,
            "stuck: the pipe took {at} of {} bytes",
            stream.len()
        );
        match source.try_write(stream.slice(at..end)) {
            Ok(()) => at = end,
            Err(TryWriteError::Full) => thread::yield_now(),
            Err(TryWriteError::Closed) => panic!("the pipe's source lost its reader"),
        }
    }
    drop(source);
    let sum: u64 = stream.iter().map(|byte| u64::from(*byte)).sum();
    assert_eq!(
        host.next(),
        (0, Msg::Text(format!("pipe {} {sum}", stream.len())))
    );

    // The child closes its end: the host hears.
    host.link.send(0, &text("quit")).unwrap();
    assert_eq!(host.end(), (Gone::PeerClosed, CLOSED));
}

#[test]
fn a_host_that_closes_is_heard_by_its_child() {
    let host = start("echo", None);
    host.link.send(0, &text("hello")).unwrap();
    assert_eq!(host.next(), (0, text("echo hello")));
    host.link.close();
    assert_eq!(host.end(), (Gone::Closed, PEER_CLOSED));
}

#[test]
fn a_child_that_exits_or_is_killed_is_heard_by_its_host() {
    let host = start("die", None);
    assert_eq!(host.end(), (Gone::PeerClosed, 7));

    let mut host = start("echo", None);
    host.link.send(0, &text("alive")).unwrap();
    assert_eq!(host.next(), (0, text("echo alive")));
    host.child.kill().unwrap();
    let (gone, _) = host.end();
    assert_eq!(gone, Gone::PeerClosed);
}

#[test]
fn a_child_that_isnt_reading_delays_nothing_for_good() {
    // The child sleeps before it reads: more is sent than its port
    // holds, the rest waits, and goes when the kernel says there is room.
    let host = start("sleepy", None);
    for i in 0..3000 {
        host.link.send(i % 3, &Msg::Text(format!("{i}"))).unwrap();
    }
    for i in 0..3000 {
        assert_eq!(host.next(), (i % 3, Msg::Text(format!("echo {i}"))));
    }
    host.link.close();
    assert_eq!(host.end(), (Gone::Closed, PEER_CLOSED));
}

#[test]
fn a_notification_the_kernel_didnt_send_is_nothing() {
    let host = start("forged", None);
    assert_eq!(host.next(), (1, text("still here")));
    // The channel is as it was: a pause, and nothing ended it.
    assert_eq!(
        host.got.recv_timeout(Duration::from_millis(300)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!host.io.is_finished());
    host.link.close();
    assert_eq!(host.end(), (Gone::Closed, PEER_CLOSED));
}

#[test]
fn what_a_child_may_not_send_closes_its_channel() {
    let host = start("handle", None);
    assert_eq!(
        host.end(),
        (Gone::BadMessage(BadMessage::HandlesNotAllowed), PEER_CLOSED)
    );

    let host = start("garbage", None);
    let (gone, code) = host.end();
    assert!(
        matches!(gone, Gone::BadMessage(BadMessage::TooLong { .. })),
        "{gone:?}"
    );
    assert_eq!(code, PEER_CLOSED);

    // More than a message is long, out of line: not a message at all.
    let host = start("huge", None);
    assert_eq!(host.end(), (Gone::PeerClosed, PEER_CLOSED));
}

#[test]
fn a_child_cant_write_what_the_host_sends_it() {
    let host = start("map", None);
    host.link
        .send(0, &Msg::Body(pattern(200 * KIB, 3)))
        .unwrap();
    let (_, code) = host.end();
    assert_eq!(
        code, 0,
        "{WRITABLE}: mapped to write; {UNREADABLE}: not mapped to read"
    );
}

#[test]
fn messages_from_a_process_that_isnt_the_peer_are_nothing() {
    // The host takes its peer to be another process (launchd): what the
    // child sends comes from the wrong one.
    let mut host = start("echo", Some(1));
    host.link.send(0, &text("hello")).unwrap();
    assert_eq!(
        host.got.recv_timeout(Duration::from_millis(700)),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(!host.io.is_finished(), "nor do they end the channel");
    host.child.kill().unwrap();
    let (gone, _) = host.end();
    assert_eq!(gone, Gone::PeerClosed);
}
