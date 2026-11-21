//! A channel between real processes, whatever the platform: the test is
//! the host, with a whole endpoint over the platform's channel, and
//! starts itself again as the child (this file's `child`, told what to be
//! by an environment variable), which takes its end as any child does.
//! Some children are endpoints that answer what they are sent; some
//! write, by hand, what a child of ours never would.
//!
//! Nothing here names a platform: a platform's layer passes these as they
//! are, and has tests of its own for what only it has.

use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use bytes::Bytes;
use ipc::core::channel::{Gone, MAX_QUEUED_BYTES, Transport};
use ipc::core::frame::{BadMessage, Encoder};
use ipc::data_pipe::{self, Read, TryWriteError};
use ipc::endpoint::{self, Config, Event, Handler, Link, Platform, Received};
use ipc::platform::{self, Channel};
use serde::{Deserialize, Serialize};

pub(crate) const ROLE: &str = "MACHOPO_IPC_CHANNEL_CHILD";
pub(crate) const KIB: usize = 1 << 10;
pub(crate) const SOON: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) enum Msg {
    Text(String),
    Body(Bytes),
    Stream { pipe: u32 },
}

pub(crate) fn text(s: &str) -> Msg {
    Msg::Text(s.into())
}

fn pattern(len: usize, salt: u8) -> Bytes {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
        .collect()
}

pub(crate) fn config(host: bool) -> Config {
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
pub(crate) const CLOSED: i32 = 20;
pub(crate) const PEER_CLOSED: i32 = 21;
const BAD_MESSAGE: i32 = 22;
pub(crate) const OTHER: i32 = 23;
pub(crate) const SETUP: i32 = 24;
const WRITABLE: i32 = 31;

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

/// One of the core's messages as its bytes.
pub(crate) fn frame(encoder: &mut Encoder, destination: u32, msg: &Msg, handles: usize) -> Vec<u8> {
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
        // Goes without taking its end.
        "never" => 8,
        role => platform::inherited().map_or(SETUP, |channel| match role {
            "echo" | "sleepy" => {
                if role == "sleepy" {
                    thread::sleep(Duration::from_millis(1500));
                }
                let (link, endpoint) = endpoint::endpoint(channel, config(false));
                gone_code(endpoint.run(Echo { link }))
            }
            // Has its end and goes, without a word.
            "die" => 7,
            other => rogue(other, &channel),
        }),
    };
    std::process::exit(outcome);
}

/// A child that isn't ours: what it writes to the channel is made by
/// hand.
fn rogue(role: &str, channel: &Channel) -> i32 {
    match role {
        // Bytes that are no message of the core's.
        "garbage" => {
            channel.write(&[0xff; 40], &mut Vec::new()).unwrap();
            wait_for_the_host_to_go(channel)
        }
        // A good message with a handle attached, which a child may not.
        "handle" => {
            let Some((_mapping, handle)) = channel.create(64 * KIB, false) else {
                return SETUP;
            };
            let with_handle = frame(&mut Encoder::new(), 0, &text("with a handle"), 1);
            channel.write(&with_handle, &mut vec![handle]).unwrap();
            wait_for_the_host_to_go(channel)
        }
        // The host sends large bytes: the region they are in comes as a
        // handle. Whether this process can map to write what it should
        // only read.
        "map" => {
            for _ in 0..100 {
                let handle =
                    channel.wait(|event, _, handles| (event == Event::Read).then(|| handles.pop()));
                match handle {
                    Some(Some(region)) => {
                        return match channel.map(region, 256 * KIB, true) {
                            Some(_) => WRITABLE,
                            None => 0,
                        };
                    }
                    Some(None) => {}
                    None => return OTHER,
                }
            }
            SETUP
        }
        _ => SETUP,
    }
}

/// Waits for the host's end to go, letting go of what the host says
/// meanwhile.
pub(crate) fn wait_for_the_host_to_go(channel: &Channel) -> i32 {
    for _ in 0..10_000 {
        if channel.wait(|event, _, _| event) == Event::PeerGone {
            return PEER_CLOSED;
        }
    }
    OTHER
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
pub(crate) struct Host {
    pub(crate) link: Link<Channel>,
    pub(crate) got: mpsc::Receiver<(u32, Msg)>,
    pub(crate) io: JoinHandle<Gone>,
    pub(crate) child: Child,
}

/// The test started again to run `entry`, as the child `role`.
pub(crate) fn command(entry: &str, role: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", entry, "--nocapture", "--test-threads=1"])
        .env(ROLE, role)
        .stdout(Stdio::null());
    command
}

impl Host {
    /// Starts a child as `role`, with a channel to it.
    fn start(role: &str) -> Self {
        let (child, channel) =
            platform::spawn(&mut command("process_channel::child", role)).unwrap();
        Self::over(channel, child)
    }

    /// A host's endpoint over its end of a channel to `child`.
    pub(crate) fn over(channel: Channel, child: Child) -> Self {
        let (link, endpoint) = endpoint::endpoint(channel, config(true));
        let (forward, got) = mpsc::channel();
        let io = thread::spawn(move || endpoint.run(Forward(forward)));
        Self {
            link,
            got,
            io,
            child,
        }
    }

    pub(crate) fn next(&self) -> (u32, Msg) {
        self.got.recv_timeout(SOON).expect("the child's message")
    }

    /// Why the host's channel was over, and what the child exited with.
    pub(crate) fn end(mut self) -> (Gone, i32) {
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
        let waited = Instant::now();
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
    let host = Host::start("echo");
    host.link.send(0, &text("one")).unwrap();
    host.link.send(2, &text("two")).unwrap();
    assert_eq!(host.next(), (0, text("echo one")));
    assert_eq!(host.next(), (2, text("echo two")));

    // Large bytes to the child go by a region the host makes; back, in
    // the message (the child has no region).
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

    // A long message that isn't bytes, both ways.
    let long = "long text ".repeat(300_000);
    host.link.send(1, &text(&long)).unwrap();
    assert_eq!(host.next(), (1, Msg::Text(format!("echo {long}"))));

    // A pipe.
    let stream = pattern(700 * KIB, 9);
    let (mut source, consumer) = data_pipe::create(128 * KIB);
    let pipe = host.link.open_pipe(consumer, 128 * KIB as u32).unwrap();
    host.link.send(0, &Msg::Stream { pipe }).unwrap();
    let mut at = 0;
    let started = Instant::now();
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
    let host = Host::start("echo");
    host.link.send(0, &text("hello")).unwrap();
    assert_eq!(host.next(), (0, text("echo hello")));
    host.link.close();
    assert_eq!(host.end(), (Gone::Closed, PEER_CLOSED));
}

#[test]
fn a_child_that_exits_or_is_killed_is_heard_by_its_host() {
    let host = Host::start("die");
    assert_eq!(host.end(), (Gone::PeerClosed, 7));

    let mut host = Host::start("echo");
    host.link.send(0, &text("alive")).unwrap();
    assert_eq!(host.next(), (0, text("echo alive")));
    host.child.kill().unwrap();
    let (gone, _) = host.end();
    assert_eq!(gone, Gone::PeerClosed);
}

#[test]
fn a_child_gone_before_it_took_its_end_is_heard_once_the_host_says_so() {
    let mut host = Host::start("never");
    assert_eq!(host.child.wait().unwrap().code(), Some(8));
    // Where the kernel doesn't say so by itself (the end the child was
    // to take is still held for it), the host does, having seen the
    // child exit.
    platform::child_gone(host.child.id());
    assert_eq!(host.end(), (Gone::PeerClosed, 8));
}

#[test]
fn a_child_that_isnt_reading_delays_nothing_for_good() {
    // The child sleeps before it reads: more is sent than the transport
    // holds, the rest waits, and goes when there is room.
    let host = Host::start("sleepy");
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
fn what_a_child_may_not_send_closes_its_channel() {
    let host = Host::start("handle");
    assert_eq!(
        host.end(),
        (Gone::BadMessage(BadMessage::HandlesNotAllowed), PEER_CLOSED)
    );

    let host = Host::start("garbage");
    let (gone, code) = host.end();
    assert!(
        matches!(gone, Gone::BadMessage(BadMessage::TooLong { .. })),
        "{gone:?}"
    );
    assert_eq!(code, PEER_CLOSED);
    // And nothing of either was handed up.
    let host = Host::start("garbage");
    let handed_up = host.got.recv_timeout(Duration::from_millis(300));
    assert!(handed_up.is_err(), "{handed_up:?}");
    host.end();
}

#[test]
fn a_child_cant_map_to_write_what_the_host_sends_it() {
    let host = Host::start("map");
    host.link
        .send(0, &Msg::Body(pattern(200 * KIB, 3)))
        .unwrap();
    let (_, code) = host.end();
    assert_eq!(code, 0, "{WRITABLE}: mapped to write");
}
