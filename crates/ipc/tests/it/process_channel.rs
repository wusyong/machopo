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
use ipc::endpoint::{
    self, Config, Event, Handler, IoLoop, Link, LoopHandle, Platform, Received, Waited, Waiter,
};
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
        peer_region_limit: 8 << 20,
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

/// A child of ours: answers what it is sent. Its channel over, it stops
/// its IO thread's loop and says why.
struct Echo {
    link: Link<Channel>,
    io: LoopHandle<Channel>,
    over: mpsc::Sender<Gone>,
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

    fn gone(&mut self, gone: Gone) {
        let _ = self.over.send(gone);
        self.io.stop();
    }
}

/// A child's one IO thread, here the thread itself: serves the child's
/// end until its channel is over, and says why it was.
fn serve_as_child(channel: Channel) -> Gone {
    let (link, endpoint) = endpoint::endpoint(channel, config(false));
    let (io, handle) = IoLoop::new().unwrap();
    let (over, was_over) = mpsc::channel();
    handle.serve(
        endpoint,
        Echo {
            link,
            io: handle.clone(),
            over,
        },
    );
    io.run();
    was_over.recv().unwrap()
}

/// What a platform's waiter says next of the one channel it waits on.
struct Alone<'a> {
    channel: &'a Channel,
    waiter: <Channel as Platform>::Waiter,
}

impl<'a> Alone<'a> {
    fn new(channel: &'a Channel) -> Self {
        let mut waiter = <Channel as Platform>::Waiter::new().unwrap();
        assert!(waiter.add(0, channel));
        Self { channel, waiter }
    }

    /// The next event, with the handle a read came with mapped to write
    /// if `map` says to: whether it could be.
    fn next(&mut self, map: Option<usize>) -> (Event, Option<bool>) {
        let channel = self.channel;
        self.waiter.wait(|waited| match waited {
            Waited::Interrupted => (Event::Woken, None),
            Waited::Channel { event, handles, .. } => {
                let mapped = handles
                    .pop()
                    .zip(map)
                    .map(|(handle, len)| channel.map(handle, len, true).is_some());
                (event, mapped)
            }
        })
    }
}

impl Alone<'_> {
    /// Waits for the host's end to go, letting go of what the host says
    /// meanwhile.
    fn until_the_host_goes(mut self) -> i32 {
        for _ in 0..10_000 {
            if self.next(None).0 == Event::PeerGone {
                return PEER_CLOSED;
            }
        }
        OTHER
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
    // A child nothing tells to end (its host failed and is gone) ends by
    // itself: one left behind keeps the test run's output open.
    thread::spawn(|| {
        thread::sleep(SOON * 3);
        std::process::exit(9);
    });
    let outcome = match role.as_str() {
        // Goes without taking its end.
        "never" => 8,
        role => platform::inherited().map_or(SETUP, |channel| match role {
            "echo" | "sleepy" => {
                if role == "sleepy" {
                    thread::sleep(Duration::from_millis(1500));
                }
                gone_code(serve_as_child(channel))
            }
            // Has its end and goes, without a word.
            "die" => 7,
            other => rogue(other, &channel),
        }),
    };
    std::process::exit(outcome);
}

/// A child that isn't ours: what it writes to the channel is made by
/// hand. Its end is waited on before it writes: on some systems an end
/// writes nothing until a thread serves it.
fn rogue(role: &str, channel: &Channel) -> i32 {
    match role {
        // Bytes that are no message of the core's.
        "garbage" => {
            let alone = Alone::new(channel);
            channel.write(&[0xff; 40], &mut Vec::new()).unwrap();
            alone.until_the_host_goes()
        }
        // A good message with a handle attached, which a child may not.
        "handle" => {
            let Some((_mapping, handle)) = channel.create(64 * KIB, false) else {
                return SETUP;
            };
            let alone = Alone::new(channel);
            let with_handle = frame(&mut Encoder::new(), 0, &text("with a handle"), 1);
            channel.write(&with_handle, &mut vec![handle]).unwrap();
            alone.until_the_host_goes()
        }
        // The host sends large bytes: the region they are in comes as a
        // handle. Whether this process can map to write what it should
        // only read.
        "map" => {
            let mut alone = Alone::new(channel);
            for _ in 0..100 {
                match alone.next(Some(256 * KIB)) {
                    (Event::Read, Some(true)) => return WRITABLE,
                    (Event::Read, Some(false)) => return 0,
                    (Event::PeerGone, _) => return OTHER,
                    _ => {}
                }
            }
            SETUP
        }
        _ => SETUP,
    }
}

/// Sends what a host's endpoint hands up to the test, and why its
/// channel was over, as the endpoint `name`.
struct Forward {
    name: usize,
    got: mpsc::Sender<(usize, u32, Msg)>,
    over: mpsc::Sender<(usize, Gone)>,
}

impl Handler<Channel> for Forward {
    fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
        let msg = message.decode::<Msg>()?;
        let _ = self.got.send((self.name, message.destination(), msg));
        Ok(())
    }

    fn gone(&mut self, gone: Gone) {
        let _ = self.over.send((self.name, gone));
    }
}

/// The host's end, its IO thread, and the child.
pub(crate) struct Host {
    pub(crate) link: Link<Channel>,
    pub(crate) got: mpsc::Receiver<(usize, u32, Msg)>,
    /// The host's IO thread: it ends, with the reason, once the channel
    /// is over.
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
        let io = thread::spawn(move || {
            let (io, handle) = IoLoop::new().unwrap();
            let (over, was_over) = mpsc::channel();
            handle.serve(
                endpoint,
                Forward {
                    name: 0,
                    got: forward,
                    over,
                },
            );
            // The loop, on this thread; another waits for the channel's
            // end to stop it.
            let stop = thread::spawn(move || {
                let gone = was_over.recv().map(|(_, gone)| gone);
                handle.stop();
                gone
            });
            io.run();
            stop.join().unwrap().unwrap()
        });
        Self {
            link,
            got,
            io,
            child,
        }
    }

    pub(crate) fn next(&self) -> (u32, Msg) {
        let (_, destination, msg) = self.got.recv_timeout(SOON).expect("the child's message");
        (destination, msg)
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
    // the message (the child has no region, and asks for one for the
    // next time).
    let body = pattern(300 * KIB, 1);
    host.link.send(1, &Msg::Body(body.clone())).unwrap();
    assert_eq!(host.next(), (1, Msg::Body(body)));
    // The child says it has read the region after it has answered: the
    // answer is here first.
    let read_by_the_child = || {
        let waited = Instant::now();
        while host.link.regions() != (512 * KIB, 512 * KIB) {
            assert!(
                waited.elapsed() < SOON,
                "made, read by the child, free again: {:?}",
                host.link.regions()
            );
            thread::sleep(Duration::from_millis(1));
        }
    };
    read_by_the_child();
    // Given a region, the child's go by it too. Each is sent once the
    // child said it read the one before: sooner, the host's region is
    // still out, and it makes another.
    host.link.grant_to_peer(512 * KIB);
    for salt in 2..6 {
        let body = pattern(400 * KIB, salt);
        host.link.send(0, &Msg::Body(body.clone())).unwrap();
        assert_eq!(host.next(), (0, Msg::Body(body)));
        read_by_the_child();
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
    // The child goes as soon as it has the region, which is told of
    // before the message that names it: where a write to a peer that
    // is gone is refused, the message may not be sent.
    let _ = host.link.send(0, &Msg::Body(pattern(200 * KIB, 3)));
    let (_, code) = host.end();
    assert_eq!(code, 0, "{WRITABLE}: mapped to write");
}

#[test]
fn one_io_thread_serves_several_children() {
    // One loop, as a host process has: four children's channels on it.
    let (io, handle) = IoLoop::<Channel>::new().unwrap();
    let (stopped, was_stopped) = mpsc::channel();
    thread::spawn(move || {
        io.run();
        let _ = stopped.send(());
    });
    let (got, came) = mpsc::channel();
    let (over, was_over) = mpsc::channel();
    let mut children = Vec::new();
    let mut links = Vec::new();
    for name in 0..4 {
        let (child, channel) =
            platform::spawn(&mut command("process_channel::child", "echo")).unwrap();
        let (link, endpoint) = endpoint::endpoint(channel, config(true));
        handle.serve(
            endpoint,
            Forward {
                name,
                got: got.clone(),
                over: over.clone(),
            },
        );
        children.push(child);
        links.push(link);
    }
    let next = || came.recv_timeout(SOON).expect("a child's message");
    // Each child answers its own, in order, with the others' in between;
    // large bytes go by each channel's own region.
    for round in 0..20 {
        for (name, link) in links.iter().enumerate() {
            link.send(1, &Msg::Text(format!("{name} {round}"))).unwrap();
        }
    }
    for (name, link) in links.iter().enumerate() {
        link.send(2, &Msg::Body(pattern(300 * KIB, name as u8)))
            .unwrap();
    }
    let mut answered = [0; 4];
    let mut bodies = [false; 4];
    while answered != [20; 4] || bodies != [true; 4] {
        match next() {
            (name, 1, Msg::Text(text)) => {
                assert_eq!(text, format!("echo {name} {}", answered[name]));
                answered[name] += 1;
            }
            (name, 2, Msg::Body(body)) => {
                assert!(body == pattern(300 * KIB, name as u8));
                bodies[name] = true;
            }
            other => panic!("{other:?}"),
        }
    }
    // One child killed is that channel's alone.
    children[1].kill().unwrap();
    assert_eq!(was_over.recv_timeout(SOON).unwrap(), (1, Gone::PeerClosed));
    for name in [0, 2, 3] {
        links[name].send(0, &text("still")).unwrap();
    }
    let mut still = [false; 4];
    for _ in 0..3 {
        let (name, _, msg) = next();
        assert_eq!(msg, text("echo still"));
        still[name] = true;
    }
    assert_eq!(still, [true, false, true, true]);
    // The loop stopped closes the rest: their children hear, and exit.
    handle.stop();
    was_stopped.recv_timeout(SOON).expect("the loop stops");
    let mut closed: Vec<_> = (0..3)
        .map(|_| was_over.recv_timeout(SOON).unwrap())
        .collect();
    closed.sort_by_key(|(name, _)| *name);
    assert_eq!(
        closed,
        [(0, Gone::Closed), (2, Gone::Closed), (3, Gone::Closed)]
    );
    for (name, mut child) in children.into_iter().enumerate() {
        let waited = Instant::now();
        let code = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break status.code();
            }
            assert!(waited.elapsed() < SOON, "child {name} never exited");
            thread::sleep(Duration::from_millis(10));
        };
        if name != 1 {
            assert_eq!(code, Some(PEER_CLOSED), "child {name}");
        }
    }
}
