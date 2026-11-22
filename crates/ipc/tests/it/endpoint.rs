//! `ipc::endpoint` over the platform within one process: two ends, a host
//! (which may attach handles and makes regions) and a child, turned by
//! hand so each test sees every step; then on two threads, as they run.
//! And a child's end that isn't ours: whatever it sends, the host closes
//! the channel and says why.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use bytes::Bytes;
use ipc::core::channel::{self, Gone, MAX_QUEUED_BYTES};
use ipc::core::control::{CONTROL_DESTINATION, Control};
use ipc::core::frame::BadMessage;
use ipc::core::region::HeapMemory;
use ipc::data_pipe::{self, Consumer, Read, TryWriteError};
use ipc::endpoint::{
    self, Config, Endpoint, Event, Handler, IoLoop, Link, LoopHandle, Received, SendError,
};
use ipc::local::{self, Local};
use serde::{Deserialize, Serialize};

const KIB: usize = 1 << 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Msg {
    Text(String),
    Body(Bytes),
    Stream { pipe: u32 },
}

/// A message with pipes in it, as values.
#[derive(Serialize, Deserialize)]
struct Carrying {
    name: String,
    pipes: Vec<Consumer>,
}

/// Keeps what came, and the pipes the messages named.
#[derive(Default)]
struct Recorder {
    got: Vec<(u32, Msg)>,
    pipes: Vec<Consumer>,
    /// What comes is a [`Carrying`]: its name, and its pipes with the
    /// others.
    carrying: bool,
    carried: Vec<String>,
}

impl Handler<Local> for Recorder {
    fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
        if self.carrying {
            let Carrying { name, pipes } = message.decode()?;
            self.carried.push(name);
            self.pipes.extend(pipes);
            return Ok(());
        }
        let msg = message.decode::<Msg>()?;
        if let Msg::Stream { pipe } = &msg {
            self.pipes
                .push(message.take_pipe(*pipe).ok_or(BadMessage::Rejected)?);
        }
        self.got.push((message.destination(), msg));
        Ok(())
    }
}

fn config(host: bool) -> Config {
    Config {
        destinations: 3,
        may_attach: host,
        peer_may_attach: !host,
        peer_may_open_pipes: !host,
        region_limit: 4 << 20,
        queue_limit: MAX_QUEUED_BYTES,
    }
}

struct End {
    link: Link<Local>,
    endpoint: Endpoint<Local>,
    recorder: Recorder,
    gone: Option<Gone>,
    /// The end's wake was called: what a loop would turn it for.
    woken: Arc<AtomicBool>,
}

impl End {
    fn new(platform: Local, config: Config) -> Self {
        let (link, endpoint) = endpoint::endpoint(platform, config);
        // The test is the loop: it turns the end by hand, for its wakes
        // too.
        let woken = Arc::new(AtomicBool::new(false));
        let flag = woken.clone();
        link.on_wake(Arc::new(move || flag.store(true, Ordering::SeqCst)));
        Self {
            link,
            endpoint,
            recorder: Recorder::default(),
            gone: None,
            woken,
        }
    }

    /// One turn, if the end was woken or the platform has something:
    /// whether it did.
    fn turn(&mut self) -> bool {
        let mut bytes = Vec::new();
        let mut handles = Vec::new();
        let event = match self.woken.swap(false, Ordering::SeqCst) {
            true => Event::Woken,
            false => match self.link.platform().poll(&mut bytes, &mut handles) {
                Some(event) => event,
                None => return false,
            },
        };
        if self.gone.is_some() {
            return false;
        }
        self.gone = self
            .endpoint
            .turn(event, &bytes, &mut handles, &mut self.recorder);
        true
    }
}

/// A host's end and a child's, with `room` bytes of room at each.
fn ends(room: Option<usize>) -> (End, End) {
    let (host, child) = local::pair(room);
    (End::new(host, config(true)), End::new(child, config(false)))
}

/// Turns both ends until neither has anything to do.
fn settle(host: &mut End, child: &mut End) {
    let mut turns = 0;
    while host.turn() | child.turn() {
        turns += 1;
        assert!(turns < 100_000, "the two ends never settle");
    }
}

fn pattern(len: usize, salt: u8) -> Bytes {
    (0..len)
        .map(|i| (i as u8).wrapping_mul(31).wrapping_add(salt))
        .collect()
}

fn text(s: &str) -> Msg {
    Msg::Text(s.into())
}

#[test]
fn messages_arrive_in_order_at_their_destinations() {
    let (mut host, mut child) = ends(None);
    host.link.send(0, &text("a")).unwrap();
    host.link.send(2, &text("b")).unwrap();
    child.link.send(1, &text("c")).unwrap();
    host.link.send(1, &Msg::Body(pattern(10, 0))).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(
        child.recorder.got,
        [
            (0, text("a")),
            (2, text("b")),
            (1, Msg::Body(pattern(10, 0)))
        ]
    );
    assert_eq!(host.recorder.got, [(1, text("c"))]);
    assert_eq!((host.gone, child.gone), (None, None));
}

#[test]
fn a_hosts_large_bytes_go_by_a_region_it_makes_and_uses_again() {
    let (mut host, mut child) = ends(None);
    assert_eq!(host.link.regions(), (0, 0));
    let first = Msg::Body(pattern(300 * KIB, 1));
    host.link.send(0, &first).unwrap();
    assert_eq!(
        host.link.regions(),
        (512 * KIB, 0),
        "a region of the class that holds it, out"
    );
    assert!(
        host.link.platform().written() < KIB,
        "the bytes aren't in the message"
    );
    settle(&mut host, &mut child);
    assert_eq!(child.recorder.got, [(0, first)]);
    assert_eq!(
        host.link.regions(),
        (512 * KIB, 512 * KIB),
        "the child read it: free again"
    );
    // The next one goes in the same region, and smaller ones too.
    for (salt, len) in [(2, 400 * KIB), (3, 70 * KIB), (4, 512 * KIB)] {
        let body = Msg::Body(pattern(len, salt));
        host.link.send(1, &body).unwrap();
        assert_eq!(host.link.regions(), (512 * KIB, 0));
        settle(&mut host, &mut child);
        assert_eq!(child.recorder.got.last(), Some(&(1, body)));
    }
    assert!(host.link.platform().written() < KIB);
    // Two at once: a second region; one too long for both: a third.
    host.link
        .send(0, &Msg::Body(pattern(100 * KIB, 5)))
        .unwrap();
    host.link
        .send(0, &Msg::Body(pattern(100 * KIB, 6)))
        .unwrap();
    assert_eq!(host.link.regions(), (512 * KIB + 128 * KIB, 0));
    host.link
        .send(0, &Msg::Body(pattern(600 * KIB, 7)))
        .unwrap();
    assert_eq!(host.link.regions().0, (512 + 128 + 1024) * KIB);
    settle(&mut host, &mut child);
    assert_eq!(child.recorder.got.len(), 7);
    assert_eq!(child.recorder.got[6], (0, Msg::Body(pattern(600 * KIB, 7))));
    let (total, free) = host.link.regions();
    assert_eq!(total, free);
    assert_eq!((host.gone, child.gone), (None, None));
}

#[test]
fn a_host_at_its_limit_of_regions_sends_large_bytes_in_the_message() {
    let (host_end, child_end) = local::pair(None);
    let mut host = End::new(
        host_end,
        Config {
            region_limit: 256 * KIB,
            ..config(true)
        },
    );
    let mut child = End::new(child_end, config(false));
    host.link
        .send(0, &Msg::Body(pattern(200 * KIB, 1)))
        .unwrap();
    assert_eq!(host.link.regions().0, 256 * KIB);
    let written = host.link.platform().written();
    // The one region is out, and a second would pass the limit.
    let second = Msg::Body(pattern(200 * KIB, 2));
    host.link.send(0, &second).unwrap();
    assert_eq!(host.link.regions().0, 256 * KIB);
    assert!(host.link.platform().written() > written + 200 * KIB);
    // Longer than the limit itself.
    let third = Msg::Body(pattern(300 * KIB, 3));
    host.link.send(0, &third).unwrap();
    assert_eq!(
        host.link.platform().created(),
        1,
        "no region is made to be thrown away"
    );
    settle(&mut host, &mut child);
    assert_eq!(child.recorder.got[1..], [(0, second), (0, third)]);
}

#[test]
fn a_child_makes_no_regions_and_uses_the_ones_it_is_granted() {
    let (mut host, mut child) = ends(None);
    let first = Msg::Body(pattern(200 * KIB, 1));
    child.link.send(0, &first).unwrap();
    assert_eq!(child.link.regions(), (0, 0));
    assert!(
        child.link.platform().written() > 200 * KIB,
        "in the message: it has no region"
    );
    settle(&mut host, &mut child);
    assert_eq!(host.recorder.got, [(0, first)]);

    host.link.grant_to_peer(200 * KIB);
    settle(&mut host, &mut child);
    assert_eq!(child.link.regions(), (256 * KIB, 256 * KIB));
    let written = child.link.platform().written();
    let second = Msg::Body(pattern(256 * KIB, 2));
    child.link.send(1, &second).unwrap();
    assert_eq!(child.link.regions(), (256 * KIB, 0));
    assert!(child.link.platform().written() < written + KIB);
    settle(&mut host, &mut child);
    assert_eq!(host.recorder.got[1], (1, second));
    assert_eq!(
        child.link.regions(),
        (256 * KIB, 256 * KIB),
        "the host read it: free again"
    );
    // A child that asks for a region for its peer gets nothing done.
    child.link.grant_to_peer(100 * KIB);
    settle(&mut host, &mut child);
    assert_eq!(host.link.regions(), (0, 0));
    assert_eq!(child.link.platform().created(), 0);
    // Each region the host grants is one more, with a number of its own.
    host.link.grant_to_peer(64 * KIB);
    host.link.grant_to_peer(100 * KIB);
    settle(&mut host, &mut child);
    assert_eq!(child.link.regions().0, (256 + 64 + 128) * KIB);
    let bodies = [
        pattern(256 * KIB, 5),
        pattern(64 * KIB, 6),
        pattern(128 * KIB, 7),
    ];
    for body in &bodies {
        child.link.send(2, &Msg::Body(body.clone())).unwrap();
    }
    assert_eq!(child.link.regions().1, 0, "one in each");
    settle(&mut host, &mut child);
    for (i, body) in bodies.into_iter().enumerate() {
        assert_eq!(host.recorder.got[2 + i], (2, Msg::Body(body)));
    }
    assert_eq!((host.gone, child.gone), (None, None));
}

/// Writes all of `bytes` into a pipe's source as the two ends turn.
fn feed(
    source: &mut data_pipe::Producer,
    bytes: &Bytes,
    host: &mut End,
    child: &mut End,
    read: impl Fn(&mut End),
) {
    let mut at = 0;
    let mut turns = 0;
    while at < bytes.len() {
        let end = (at + 50 * KIB).min(bytes.len());
        match source.try_write(bytes.slice(at..end)) {
            Ok(()) => at = end,
            Err(TryWriteError::Full) => {}
            Err(TryWriteError::Closed) => panic!("the source's reader is gone"),
        }
        settle(host, child);
        read(child);
        turns += 1;
        assert!(turns < 10_000, "stuck at {at}");
    }
}

#[test]
fn a_pipes_bytes_arrive_in_order_as_far_as_the_reader_has_room() {
    let (mut host, mut child) = ends(None);
    let (mut source, consumer) = data_pipe::create(1 << 20);
    let pipe = host.link.open_pipe(consumer, 128 * KIB as u32).unwrap();
    host.link.send(0, &Msg::Stream { pipe }).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(child.recorder.got, [(0, Msg::Stream { pipe })]);
    let mut reader = child.recorder.pipes.pop().unwrap();
    assert_eq!(reader.read(64), Read::Empty);

    // The reader reads nothing: only the pipe's capacity goes.
    let body = pattern(700 * KIB, 9);
    source.try_write(body.slice(..400 * KIB)).unwrap();
    settle(&mut host, &mut child);
    let written = host.link.platform().written();
    settle(&mut host, &mut child);
    assert_eq!(
        host.link.platform().written(),
        written,
        "nothing more goes while nothing is read"
    );
    let mut got = Vec::new();
    while let Read::Data(bytes) = reader.read(usize::MAX) {
        got.extend_from_slice(&bytes);
    }
    assert_eq!(got.len(), 128 * KIB);

    // As it reads, the rest comes.
    source.try_write(body.slice(400 * KIB..)).unwrap();
    drop(source);
    let mut turns = 0;
    loop {
        settle(&mut host, &mut child);
        match reader.read(30 * KIB) {
            Read::Data(bytes) => got.extend_from_slice(&bytes),
            Read::Empty => {}
            Read::Done => break,
        }
        turns += 1;
        assert!(turns < 10_000, "stuck with {} bytes", got.len());
    }
    assert!(got == body[..], "what was read isn't what was written");
    settle(&mut host, &mut child);
    // It read to the end: there is nothing to cancel when it goes.
    let written = child.link.platform().written();
    drop(reader);
    settle(&mut host, &mut child);
    assert_eq!(child.link.platform().written(), written);
    assert_eq!((host.gone, child.gone), (None, None));
}

#[test]
fn pipes_in_a_message_cross_with_it() {
    let (mut host, mut child) = ends(None);
    child.recorder.carrying = true;
    let (mut first, first_reader) = data_pipe::create(128 * KIB);
    let (mut second, second_reader) = data_pipe::create(64 * KIB);
    // Written before the message is sent, and after.
    let body = pattern(300 * KIB, 5);
    first.try_write(body.slice(..100 * KIB)).unwrap();
    let carrying = Carrying {
        name: "two pipes".into(),
        pipes: vec![first_reader, second_reader],
    };
    host.link.send(1, &carrying).unwrap();
    // The value gave its pipes up.
    let Carrying { mut pipes, .. } = carrying;
    assert_eq!(pipes[0].read(64), Read::Done);
    drop(pipes);
    settle(&mut host, &mut child);
    assert_eq!(child.recorder.carried, ["two pipes"]);
    assert_eq!(child.recorder.pipes.len(), 2);
    let mut second_at_child = child.recorder.pipes.pop().unwrap();
    let mut first_at_child = child.recorder.pipes.pop().unwrap();

    // Each pipe is its own, in the order the value had them.
    second.try_write(Bytes::from_static(b"the second")).unwrap();
    drop(second);
    let mut at = 100 * KIB;
    let mut got = Vec::new();
    let mut turns = 0;
    loop {
        if at < body.len() {
            let end = (at + 40 * KIB).min(body.len());
            if first.try_write(body.slice(at..end)).is_ok() {
                at = end;
            }
        } else if at == body.len() {
            // Its writer is done: the reader reads to the end.
            let (done, _) = data_pipe::create(1);
            drop(std::mem::replace(&mut first, done));
            at += 1;
        }
        settle(&mut host, &mut child);
        match first_at_child.read(30 * KIB) {
            Read::Data(bytes) => got.extend_from_slice(&bytes),
            Read::Empty => {}
            Read::Done => break,
        }
        turns += 1;
        assert!(turns < 10_000, "stuck with {} bytes", got.len());
    }
    assert!(got == body[..], "what was read isn't what was written");
    let mut small = Vec::new();
    loop {
        settle(&mut host, &mut child);
        match second_at_child.read(64) {
            Read::Data(bytes) => small.extend_from_slice(&bytes),
            Read::Empty => {}
            Read::Done => break,
        }
    }
    assert_eq!(small, b"the second");
    assert_eq!((host.gone, child.gone), (None, None));
}

#[test]
fn a_pipe_crosses_holding_no_more_than_its_own_capacity() {
    let (mut host, mut child) = ends(None);
    child.recorder.carrying = true;
    let (mut source, reader) = data_pipe::create(96 * KIB);
    host.link
        .send(
            0,
            &Carrying {
                name: "one".into(),
                pipes: vec![reader],
            },
        )
        .unwrap();
    settle(&mut host, &mut child);
    let mut reader = child.recorder.pipes.pop().unwrap();
    // The reader reads nothing: what crossed is the pipe's capacity, and
    // the rest waits in the writer's own pipe, which fills.
    let mut written = 0;
    loop {
        settle(&mut host, &mut child);
        match source.try_write(pattern(32 * KIB, 1)) {
            Ok(()) => written += 32 * KIB,
            Err(TryWriteError::Full) => break,
            Err(TryWriteError::Closed) => panic!("closed"),
        }
        assert!(written < 1 << 20, "nothing holds the writer back");
    }
    assert_eq!(reader.capacity(), 96 * KIB);
    let mut at_reader = 0;
    while let Read::Data(bytes) = reader.read(usize::MAX) {
        at_reader += bytes.len();
    }
    assert_eq!(at_reader, 96 * KIB);
    assert_eq!(written, 2 * 96 * KIB);
}

#[test]
fn a_message_that_isnt_sent_ends_the_pipes_it_had() {
    let (mut host, mut child) = ends(None);
    host.link.close();
    settle(&mut host, &mut child);
    let (mut source, reader) = data_pipe::create(64 * KIB);
    let carrying = Carrying {
        name: "too late".into(),
        pipes: vec![reader],
    };
    assert_eq!(
        host.link.send(0, &carrying),
        Err(SendError::Refused),
        "no pipe can be opened on a channel that is over"
    );
    assert_eq!(
        source.try_write(Bytes::from_static(b"x")),
        Err(TryWriteError::Closed),
        "the pipe went with the message"
    );
}

#[test]
fn a_message_the_channel_cant_take_ends_the_pipes_opened_for_it() {
    // A channel with little room and a short queue: the pipe is told of,
    // and then the message itself is more than may wait.
    let (host_end, child_end) = local::pair(Some(64));
    let mut host = End::new(
        host_end,
        Config {
            queue_limit: KIB,
            ..config(true)
        },
    );
    let mut child = End::new(child_end, config(false));
    child.recorder.carrying = true;
    let (mut source, reader) = data_pipe::create(64 * KIB);
    let sent = host.link.send(
        0,
        &Carrying {
            name: "x".repeat(4 * KIB),
            pipes: vec![reader],
        },
    );
    assert_eq!(sent, Err(SendError::Gone(Gone::QueueFull)));
    assert_eq!(
        source.try_write(Bytes::from_static(b"x")),
        Err(TryWriteError::Closed),
        "the pipe went with the message"
    );
    settle(&mut host, &mut child);
    assert!(child.recorder.carried.is_empty() && child.recorder.pipes.is_empty());
}

#[test]
fn a_child_sends_no_pipe_in_a_message_either() {
    // The host lets its child open no pipe: one in a message is opened
    // all the same, and is the end of the channel.
    let (mut host, mut child) = ends(None);
    let (_source, reader) = data_pipe::create(64 * KIB);
    let _ = child.link.send(
        0,
        &Carrying {
            name: "from a child".into(),
            pipes: vec![reader],
        },
    );
    settle(&mut host, &mut child);
    assert_eq!(host.gone, bad(BadMessage::Rejected));
    assert!(host.recorder.got.is_empty() && host.recorder.pipes.is_empty());
}

#[test]
fn a_reader_that_goes_stops_the_pipes_source() {
    let (mut host, mut child) = ends(None);
    let (mut source, consumer) = data_pipe::create(64 * KIB);
    let pipe = host.link.open_pipe(consumer, 64 * KIB as u32).unwrap();
    host.link.send(0, &Msg::Stream { pipe }).unwrap();
    source.try_write(pattern(10 * KIB, 0)).unwrap();
    settle(&mut host, &mut child);
    let reader = child.recorder.pipes.pop().unwrap();
    drop(reader);
    // Bytes on their way when it went are let go.
    source.try_write(pattern(10 * KIB, 1)).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(
        source.try_write(pattern(KIB, 2)),
        Err(TryWriteError::Closed)
    );
    assert_eq!((host.gone, child.gone), (None, None));
    // The channel still carries messages and other pipes.
    let (mut source, consumer) = data_pipe::create(64 * KIB);
    let second = host.link.open_pipe(consumer, 64 * KIB as u32).unwrap();
    assert_ne!(second, pipe);
    host.link.send(1, &Msg::Stream { pipe: second }).unwrap();
    let body = pattern(200 * KIB, 3);
    let got = std::cell::RefCell::new(Vec::new());
    feed(&mut source, &body, &mut host, &mut child, |child| {
        let reader = child.recorder.pipes.last_mut().unwrap();
        while let Read::Data(bytes) = reader.read(usize::MAX) {
            got.borrow_mut().extend_from_slice(&bytes);
        }
    });
    drop(source);
    settle(&mut host, &mut child);
    let reader = child.recorder.pipes.last_mut().unwrap();
    assert_eq!(reader.read(1), Read::Done);
    assert!(got.into_inner() == body[..]);
}

#[test]
fn a_transport_with_little_room_delays_nothing_for_good() {
    let (mut host, mut child) = ends(Some(100));
    for i in 0..50 {
        host.link
            .send(0, &text(&format!("to the child {i} {}", "x".repeat(i))))
            .unwrap();
        child
            .link
            .send(1, &text(&format!("to the host {i}")))
            .unwrap();
    }
    host.link
        .send(2, &Msg::Body(pattern(300 * KIB, 4)))
        .unwrap();
    settle(&mut host, &mut child);
    assert_eq!(child.recorder.got.len(), 51);
    assert_eq!(host.recorder.got.len(), 50);
    for i in 0..50 {
        assert_eq!(
            child.recorder.got[i],
            (0, text(&format!("to the child {i} {}", "x".repeat(i))))
        );
        assert_eq!(host.recorder.got[i], (1, text(&format!("to the host {i}"))));
    }
    assert_eq!(
        child.recorder.got[50],
        (2, Msg::Body(pattern(300 * KIB, 4)))
    );
}

#[test]
fn a_closed_channel_is_over_at_both_ends_and_sends_nothing() {
    let (mut host, mut child) = ends(None);
    host.link.send(0, &text("before")).unwrap();
    settle(&mut host, &mut child);
    host.link.close();
    settle(&mut host, &mut child);
    assert_eq!(host.gone, Some(Gone::Closed));
    assert_eq!(
        host.link.send(0, &text("after")),
        Err(SendError::Gone(Gone::Closed))
    );
    // Large bytes placed for a message that doesn't go are free again.
    let (mut host, mut child) = ends(None);
    host.link
        .send(0, &Msg::Body(pattern(100 * KIB, 0)))
        .unwrap();
    settle(&mut host, &mut child);
    assert_eq!(host.link.regions(), (128 * KIB, 128 * KIB));
    host.link.close();
    assert_eq!(
        host.link.send(0, &Msg::Body(pattern(100 * KIB, 1))),
        Err(SendError::Gone(Gone::Closed))
    );
    assert_eq!(host.link.regions(), (128 * KIB, 128 * KIB));
    // A pipe's source learns its reader is gone.
    let (mut host, mut child) = ends(None);
    let (mut source, consumer) = data_pipe::create(64 * KIB);
    host.link.open_pipe(consumer, 10).unwrap();
    source.try_write(pattern(40, 0)).unwrap();
    settle(&mut host, &mut child);
    assert_eq!(source.try_write(pattern(1, 0)), Ok(()));
    host.link.close();
    settle(&mut host, &mut child);
    assert_eq!(source.try_write(pattern(1, 0)), Err(TryWriteError::Closed));
    // The peer's end dropped: over at this one.
    let (mut host, child) = ends(None);
    drop(child);
    assert!(host.turn());
    assert_eq!(host.gone, Some(Gone::PeerClosed));
}

/// A child's end that is not ours: a bare channel over the same platform,
/// which sends whatever it is told to.
struct Rogue {
    sender: channel::Sender<Local>,
}

fn rogue() -> (End, Rogue) {
    let (host, child) = local::pair(None);
    let (sender, _) = channel::channel(
        child,
        channel::Config {
            destinations: 3,
            // What it may do is the host's to decide, not its own.
            may_attach: true,
            peer_may_attach: true,
            queue_limit: MAX_QUEUED_BYTES,
        },
    );
    (End::new(host, config(true)), Rogue { sender })
}

impl Rogue {
    fn control(&self, control: &Control<'_>, handles: Vec<local::Region>) {
        let mut payload = Vec::new();
        control.encode(&mut payload);
        self.sender
            .send(CONTROL_DESTINATION, &payload, handles)
            .unwrap();
    }

    fn message(&self, destination: u32, msg: &impl Serialize) {
        let payload = postcard::to_allocvec(msg).unwrap();
        self.sender.send(destination, &payload, Vec::new()).unwrap();
    }
}

/// What the host says of a child that sends `send`. What `send` returns
/// is kept until the host has said it (a pipe's source, so the pipe stays
/// open).
fn host_says<Kept>(send: impl FnOnce(&mut End, &Rogue) -> Kept) -> Option<Gone> {
    let (mut host, rogue) = rogue();
    let _kept = send(&mut host, &rogue);
    while host.turn() {}
    // Nothing a child sends makes the host hand anything up after.
    let got = host.recorder.got.len();
    // (To a channel the host closed the probe doesn't even go.)
    let probe = postcard::to_allocvec(&text("probe")).unwrap();
    let _ = rogue.sender.send(0, &probe, Vec::new());
    while host.turn() {}
    assert_eq!(host.recorder.got.len(), got);
    host.gone
}

fn bad(bad: BadMessage) -> Option<Gone> {
    Some(Gone::BadMessage(bad))
}

#[test]
fn whatever_a_child_sends_that_ours_wouldnt_closes_the_channel() {
    // First, what ours would: nothing wrong. (`host_says` checks nothing
    // is handed up after; here the probe would be, so look by hand.)
    let (mut host, child) = rogue();
    child.control(&Control::Released(vec![]), vec![]);
    child.message(0, &text("fine"));
    while host.turn() {}
    assert_eq!(host.gone, None);
    assert_eq!(host.recorder.got, [(0, text("fine"))]);

    // A destination it has none at; a value that isn't one.
    assert_eq!(
        host_says(|_, rogue| rogue.message(3, &0u8)),
        bad(BadMessage::UnknownDestination { destination: 3 })
    );
    assert_eq!(
        host_says(|_, rogue| rogue.message(0, &[9u8; 4])),
        bad(BadMessage::Rejected)
    );
    // Regions: giving back one the host never wrote, or twice; naming one
    // it was never granted.
    assert_eq!(
        host_says(|_, rogue| rogue.control(&Control::Released(vec![0]), vec![])),
        bad(BadMessage::UnknownRegion { number: 0 })
    );
    assert_eq!(
        host_says(|host, rogue| {
            host.link
                .send(0, &Msg::Body(pattern(100 * KIB, 0)))
                .unwrap();
            rogue.control(&Control::Released(vec![0, 0]), vec![]);
        }),
        bad(BadMessage::RegionNotOut { number: 0 })
    );
    #[derive(Serialize)]
    enum Forged {
        #[expect(dead_code, reason = "keeps `Body` the second variant, as `Msg` has it")]
        Text(String),
        Body(ForgedBytes),
    }
    #[derive(Serialize)]
    enum ForgedBytes {
        #[expect(
            dead_code,
            reason = "keeps `Region` the second variant, as the codec writes it"
        )]
        Inline(Bytes),
        Region(u32, u32),
    }
    assert_eq!(
        host_says(|_, rogue| rogue.message(0, &Forged::Body(ForgedBytes::Region(0, 1)))),
        bad(BadMessage::UnknownRegion { number: 0 })
    );
    assert_eq!(
        host_says(|host, rogue| {
            host.link.grant_to_peer(64 * KIB);
            while host.turn() {}
            rogue.message(
                0,
                &Forged::Body(ForgedBytes::Region(0, 64 * KIB as u32 + 1)),
            );
        }),
        bad(BadMessage::OutsideRegion {
            number: 0,
            len: 64 * KIB as u32 + 1
        })
    );
    // A region of its own making, or any handle.
    let region = || std::sync::Arc::new(HeapMemory::new(64 * KIB));
    assert_eq!(
        host_says(|_, rogue| {
            let granted = Control::Granted {
                number: 5,
                len: 64 * KIB as u32,
                writable: false,
            };
            rogue.control(&granted, vec![region()]);
        }),
        bad(BadMessage::HandlesNotAllowed)
    );
    assert_eq!(
        host_says(|_, rogue| {
            let payload = postcard::to_allocvec(&text("with a handle")).unwrap();
            rogue.sender.send(0, &payload, vec![region()]).unwrap();
        }),
        bad(BadMessage::HandlesNotAllowed)
    );
    assert_eq!(
        host_says(|_, rogue| {
            let granted = Control::Granted {
                number: 5,
                len: 64 * KIB as u32,
                writable: false,
            };
            rogue.control(&granted, vec![]);
        }),
        bad(BadMessage::Rejected),
        "a grant with no region"
    );
    // Pipes: opening one to the host; reports and cancels of ones the host
    // never opened; a report of more than it was sent; a message that
    // names one.
    assert_eq!(
        host_says(|_, rogue| rogue.control(
            &Control::PipeOpened {
                pipe: 0,
                capacity: 10
            },
            vec![]
        )),
        bad(BadMessage::Rejected)
    );
    assert_eq!(
        host_says(|_, rogue| rogue.control(&Control::PipeRead { pipe: 0, count: 0 }, vec![])),
        bad(BadMessage::UnknownPipe { pipe: 0 })
    );
    assert_eq!(
        host_says(|_, rogue| rogue.control(&Control::PipeCancelled { pipe: 0 }, vec![])),
        bad(BadMessage::UnknownPipe { pipe: 0 })
    );
    assert_eq!(
        host_says(|_, rogue| rogue.control(
            &Control::PipeData {
                pipe: 0,
                bytes: b"x"
            },
            vec![]
        )),
        bad(BadMessage::UnknownPipe { pipe: 0 })
    );
    assert_eq!(
        host_says(|_, rogue| rogue.control(&Control::PipeEnded { pipe: 0 }, vec![])),
        bad(BadMessage::UnknownPipe { pipe: 0 })
    );
    assert_eq!(
        host_says(|host, rogue| {
            let (mut source, consumer) = data_pipe::create(KIB);
            let pipe = host.link.open_pipe(consumer, 100).unwrap();
            source.try_write(pattern(60, 0)).unwrap();
            while host.turn() {}
            rogue.control(&Control::PipeRead { pipe, count: 61 }, vec![]);
            source
        }),
        bad(BadMessage::PipeOverRead { pipe: 0 })
    );
    assert_eq!(
        host_says(|_, rogue| rogue.message(0, &Msg::Stream { pipe: 0 })),
        bad(BadMessage::Rejected)
    );
    // Not a control message at all.
    assert_eq!(
        host_says(|_, rogue| rogue
            .sender
            .send(CONTROL_DESTINATION, &[0xee, 1, 2], vec![])
            .unwrap()),
        bad(BadMessage::Rejected)
    );
}

#[test]
fn only_the_cores_own_messages_come_with_handles() {
    // A peer that may attach handles (a host), to a child: a handle on a
    // message for the layer above is still a bad message.
    let (child_end, host_end) = local::pair(None);
    let (sender, _) = channel::channel(
        host_end,
        channel::Config {
            destinations: 3,
            may_attach: true,
            peer_may_attach: false,
            queue_limit: MAX_QUEUED_BYTES,
        },
    );
    let mut child = End::new(child_end, config(false));
    let payload = postcard::to_allocvec(&text("fine")).unwrap();
    sender.send(0, &payload, Vec::new()).unwrap();
    while child.turn() {}
    assert_eq!(child.gone, None);
    let region = std::sync::Arc::new(HeapMemory::new(64 * KIB));
    sender.send(0, &payload, vec![region]).unwrap();
    while child.turn() {}
    assert_eq!(child.gone, Some(Gone::BadMessage(BadMessage::Rejected)));
    assert_eq!(
        child.recorder.got,
        [(0, text("fine"))],
        "and it wasn't handed up"
    );
}

#[test]
fn a_child_keeps_to_what_its_host_may_send_too() {
    // The same checks at the child's end, for a host that isn't ours:
    // bytes past a pipe's capacity, a pipe's number used again, a region
    // that isn't one.
    let child_says = |send: &dyn Fn(&Rogue)| {
        let (child_end, host_end) = local::pair(None);
        let (sender, _) = channel::channel(
            host_end,
            channel::Config {
                destinations: 3,
                may_attach: true,
                peer_may_attach: false,
                queue_limit: MAX_QUEUED_BYTES,
            },
        );
        let mut child = End::new(child_end, config(false));
        send(&Rogue { sender });
        while child.turn() {}
        child.gone
    };
    assert_eq!(
        child_says(&|rogue| {
            rogue.control(
                &Control::PipeOpened {
                    pipe: 4,
                    capacity: 10,
                },
                vec![],
            );
            rogue.control(
                &Control::PipeData {
                    pipe: 4,
                    bytes: &[0; 10],
                },
                vec![],
            );
            rogue.control(
                &Control::PipeData {
                    pipe: 4,
                    bytes: &[0; 1],
                },
                vec![],
            );
        }),
        bad(BadMessage::PipeOverrun { pipe: 4 })
    );
    assert_eq!(
        child_says(&|rogue| {
            rogue.control(
                &Control::PipeOpened {
                    pipe: 4,
                    capacity: 10,
                },
                vec![],
            );
            rogue.control(
                &Control::PipeOpened {
                    pipe: 4,
                    capacity: 10,
                },
                vec![],
            );
        }),
        bad(BadMessage::PipeReopened { pipe: 4 })
    );
    let region = |len| std::sync::Arc::new(HeapMemory::new(len));
    let granted = |writable| Control::Granted {
        number: 1,
        len: 64 * KIB as u32,
        writable,
    };
    for writable in [false, true] {
        // Not the length it says; a number used twice; two handles.
        assert_eq!(
            child_says(&|rogue| rogue.control(&granted(writable), vec![region(4 * KIB)])),
            bad(BadMessage::Rejected)
        );
        assert_eq!(
            child_says(&|rogue| {
                rogue.control(&granted(writable), vec![region(64 * KIB)]);
                rogue.control(&granted(writable), vec![region(64 * KIB)]);
            }),
            bad(BadMessage::Rejected)
        );
        assert_eq!(
            child_says(&|rogue| rogue
                .control(&granted(writable), vec![region(64 * KIB), region(64 * KIB)])),
            bad(BadMessage::Rejected)
        );
        // A region that is one: nothing wrong, until the host's end goes.
        assert_eq!(
            child_says(&|rogue| rogue.control(&granted(writable), vec![region(64 * KIB)])),
            Some(Gone::PeerClosed)
        );
    }
}

/// What an endpoint served by a loop saw, told to the test.
#[derive(Debug, PartialEq)]
enum Seen {
    Text(String),
    Body(Bytes),
    /// A pipe a message named, read to its end.
    Pipe(Vec<u8>),
    Gone(Gone),
}

/// Tells the test of everything, as the endpoint `name`. Reads each pipe
/// a message names on a thread of its own; answers a text that begins
/// "ask" with "answer".
struct Teller {
    name: usize,
    link: Link<Local>,
    seen: mpsc::Sender<(usize, Seen)>,
}

impl Handler<Local> for Teller {
    fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
        let seen = match message.decode::<Msg>()? {
            Msg::Text(text) => {
                if let Some(rest) = text.strip_prefix("ask") {
                    self.link
                        .send(message.destination(), &Msg::Text(format!("answer{rest}")))
                        .unwrap();
                }
                Seen::Text(text)
            }
            Msg::Body(body) => Seen::Body(body),
            Msg::Stream { pipe } => {
                let mut reader = message.take_pipe(pipe).ok_or(BadMessage::Rejected)?;
                let (name, seen) = (self.name, self.seen.clone());
                thread::spawn(move || {
                    let mut got = Vec::new();
                    loop {
                        match reader.read(40 * KIB) {
                            Read::Data(bytes) => got.extend_from_slice(&bytes),
                            Read::Empty => thread::yield_now(),
                            Read::Done => break,
                        }
                    }
                    let _ = seen.send((name, Seen::Pipe(got)));
                });
                return Ok(());
            }
        };
        let _ = self.seen.send((self.name, seen));
        Ok(())
    }

    fn gone(&mut self, gone: Gone) {
        let _ = self.seen.send((self.name, Seen::Gone(gone)));
    }
}

/// A loop on a thread of its own.
fn io_thread() -> (LoopHandle<Local>, thread::JoinHandle<()>) {
    let (io, handle) = IoLoop::new().unwrap();
    (handle, thread::spawn(move || io.run()))
}

/// Whatever goes wrong with threads here is a wait that never ends: the
/// test says so itself, instead of hanging.
fn within(test: impl FnOnce() + Send + 'static) {
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        test();
        let _ = done.send(());
    });
    match finished.recv_timeout(Duration::from_secs(30)) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => {
            panic!("stuck: a message, a wake or the channel's end never came")
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => panic!("failed (above)"),
    }
}

#[test]
fn two_loops_on_two_threads_run_a_channel_until_one_end_closes() {
    within(|| {
        let (host_end, child_end) = local::pair(Some(64 * KIB));
        let (host, host_endpoint) = endpoint::endpoint(host_end, config(true));
        let (child, child_endpoint) = endpoint::endpoint(child_end, config(false));
        let (host_io, host_thread) = io_thread();
        let (child_io, child_thread) = io_thread();
        let (seen, saw) = mpsc::channel();
        // Sent before either end is served: nothing is lost.
        host.send(0, &text("early")).unwrap();
        let teller = |name, link: &Link<Local>| Teller {
            name,
            link: link.clone(),
            seen: seen.clone(),
        };
        host_io.serve(host_endpoint, teller(0, &host));
        child_io.serve(child_endpoint, teller(1, &child));

        host.grant_to_peer(256 * KIB);
        let body = pattern(900 * KIB, 7);
        let (mut source, consumer) = data_pipe::create(128 * KIB);
        let pipe = host.open_pipe(consumer, 128 * KIB as u32).unwrap();
        host.send(0, &Msg::Stream { pipe }).unwrap();
        let senders: Vec<_> = (0..3u8)
            .map(|n| {
                let (host, child) = (host.clone(), child.clone());
                thread::spawn(move || {
                    for i in 0..40u8 {
                        host.send(u32::from(n), &text("to the child")).unwrap();
                        child.send(u32::from(n), &text("to the host")).unwrap();
                        if i % 8 == 0 {
                            host.send(1, &Msg::Body(pattern(100 * KIB, n))).unwrap();
                            child.send(1, &Msg::Body(pattern(100 * KIB, n))).unwrap();
                        }
                    }
                })
            })
            .collect();
        let mut at = 0;
        while at < body.len() {
            let end = (at + 30 * KIB).min(body.len());
            match source.try_write(body.slice(at..end)) {
                Ok(()) => at = end,
                Err(TryWriteError::Full) => thread::yield_now(),
                Err(TryWriteError::Closed) => panic!("the pipe's source lost its reader"),
            }
        }
        drop(source);
        for sender in senders {
            sender.join().unwrap();
        }
        // Everything arrives at both ends: 120 texts and 15 bodies each
        // way, the early text and the pipe at the child.
        let mut texts = [0, 0];
        let mut bodies = [Vec::new(), Vec::new()];
        let mut piped = None;
        while texts != [120, 121]
            || bodies[0].len() != 15
            || bodies[1].len() != 15
            || piped.is_none()
        {
            match saw.recv().unwrap() {
                (name, Seen::Text(_)) => texts[name] += 1,
                (name, Seen::Body(body)) => bodies[name].push(body),
                (1, Seen::Pipe(got)) => piped = Some(got),
                other => panic!("{other:?}"),
            }
        }
        for got in &bodies {
            for n in 0..3u8 {
                assert_eq!(
                    got.iter()
                        .filter(|body| **body == pattern(100 * KIB, n))
                        .count(),
                    5
                );
            }
        }
        assert!(piped.unwrap() == body[..], "the pipe's bytes");
        // The host closes: each end is told why, once.
        host.close();
        let mut gone = [saw.recv().unwrap(), saw.recv().unwrap()];
        gone.sort_by_key(|(name, _)| *name);
        assert_eq!(
            gone,
            [
                (0, Seen::Gone(Gone::Closed)),
                (1, Seen::Gone(Gone::PeerClosed))
            ]
        );
        assert_eq!(
            child.send(0, &text("after")),
            Err(SendError::Gone(Gone::PeerClosed))
        );
        host_io.stop();
        child_io.stop();
        host_thread.join().unwrap();
        child_thread.join().unwrap();
        assert!(saw.try_recv().is_err(), "nothing more is told");
    });
}

#[test]
fn what_an_end_did_before_a_loop_served_it_is_done_when_one_does() {
    within(|| {
        let (io, io_thread) = io_thread();
        let (seen, saw) = mpsc::channel();
        let teller = |name, link: &Link<Local>| Teller {
            name,
            link: link.clone(),
            seen: seen.clone(),
        };
        // A pipe opened, fed and ended while nothing serves the end: its
        // wakes woke nobody.
        let (host_end, child_end) = local::pair(Some(64 * KIB));
        let (host, host_endpoint) = endpoint::endpoint(host_end, config(true));
        let (child, child_endpoint) = endpoint::endpoint(child_end, config(false));
        let body = pattern(50 * KIB, 3);
        let (mut source, consumer) = data_pipe::create(128 * KIB);
        let pipe = host.open_pipe(consumer, 128 * KIB as u32).unwrap();
        host.send(0, &Msg::Stream { pipe }).unwrap();
        source.try_write(body.clone()).unwrap();
        drop(source);
        // And an end closed before it was ever served.
        let (closed_end, other_end) = local::pair(Some(64 * KIB));
        let (closed, closed_endpoint) = endpoint::endpoint(closed_end, config(true));
        let (other, other_endpoint) = endpoint::endpoint(other_end, config(false));
        closed.close();

        // And a message that came to an end before it was served, with
        // nothing after it to have the loop look.
        let (early_end, late_end) = local::pair(Some(64 * KIB));
        let (early, _early_endpoint) = endpoint::endpoint(early_end, config(true));
        let (late, late_endpoint) = endpoint::endpoint(late_end, config(false));
        early.send(0, &text("early")).unwrap();
        io.serve(late_endpoint, teller(4, &late));
        assert_eq!(saw.recv().unwrap(), (4, Seen::Text("early".into())));

        io.serve(child_endpoint, teller(1, &child));
        io.serve(other_endpoint, teller(3, &other));
        assert!(saw.recv_timeout(Duration::from_millis(100)).is_err());
        io.serve(host_endpoint, teller(0, &host));
        io.serve(closed_endpoint, teller(2, &closed));
        let mut told = vec![
            saw.recv().unwrap(),
            saw.recv().unwrap(),
            saw.recv().unwrap(),
        ];
        told.sort_by_key(|(name, _)| *name);
        assert_eq!(
            told,
            [
                (1, Seen::Pipe(body.to_vec())),
                (2, Seen::Gone(Gone::Closed)),
                (3, Seen::Gone(Gone::PeerClosed))
            ]
        );
        io.stop();
        io_thread.join().unwrap();
    });
}

#[test]
fn one_loop_serves_many_channels() {
    within(|| {
        // Five channels: all the host ends on one thread, all the child
        // ends on another.
        let (host_io, host_thread) = io_thread();
        let (child_io, child_thread) = io_thread();
        let (seen, saw) = mpsc::channel();
        let mut hosts = Vec::new();
        let mut children = Vec::new();
        for name in 0..5 {
            let (host_end, child_end) = local::pair(Some(16 * KIB));
            let (host, host_endpoint) = endpoint::endpoint(host_end, config(true));
            let (child, child_endpoint) = endpoint::endpoint(child_end, config(false));
            host_io.serve(
                host_endpoint,
                Teller {
                    name,
                    link: host.clone(),
                    seen: seen.clone(),
                },
            );
            // The children's are told of as 100 and up.
            child_io.serve(
                child_endpoint,
                Teller {
                    name: 100 + name,
                    link: child.clone(),
                    seen: seen.clone(),
                },
            );
            hosts.push(host);
            children.push(child);
        }
        // Each channel carries its own, in order, whatever the others do:
        // texts answered by the child, and a large body by a region.
        for round in 0..30 {
            for (name, host) in hosts.iter().enumerate() {
                host.send(0, &Msg::Text(format!("ask {name} {round}")))
                    .unwrap();
            }
        }
        for (name, host) in hosts.iter().enumerate() {
            host.send(1, &Msg::Body(pattern(200 * KIB, name as u8)))
                .unwrap();
        }
        let mut asked = [0; 5];
        let mut answered = [0; 5];
        let mut bodies = 0;
        while answered != [30; 5] || bodies != 5 {
            match saw.recv().unwrap() {
                (name @ 100.., Seen::Text(text)) => {
                    let name = name - 100;
                    assert_eq!(text, format!("ask {name} {}", asked[name]));
                    asked[name] += 1;
                }
                (name, Seen::Text(text)) => {
                    assert_eq!(text, format!("answer {name} {}", answered[name]));
                    answered[name] += 1;
                }
                (name @ 100.., Seen::Body(body)) => {
                    assert!(body == pattern(200 * KIB, (name - 100) as u8));
                    bodies += 1;
                }
                other => panic!("{other:?}"),
            }
        }
        // One channel closed is that channel's alone.
        hosts[2].close();
        let mut gone = [saw.recv().unwrap(), saw.recv().unwrap()];
        gone.sort_by_key(|(name, _)| *name);
        assert_eq!(
            gone,
            [
                (2, Seen::Gone(Gone::Closed)),
                (102, Seen::Gone(Gone::PeerClosed))
            ]
        );
        assert_eq!(
            hosts[2].send(0, &text("after")),
            Err(SendError::Gone(Gone::Closed))
        );
        for name in [0, 1, 3, 4] {
            hosts[name]
                .send(0, &Msg::Text(format!("ask {name} 30")))
                .unwrap();
        }
        let mut again = 0;
        while again < 8 {
            match saw.recv().unwrap() {
                (name, Seen::Text(_)) => {
                    assert!(name % 100 != 2);
                    again += 1;
                }
                other => panic!("{other:?}"),
            }
        }
        // A loop stopped closes what it still served: the hosts are told
        // they closed, and their peers that the peer did.
        host_io.stop();
        host_thread.join().unwrap();
        let mut told: Vec<_> = (0..8).map(|_| saw.recv().unwrap()).collect();
        told.sort_by_key(|(name, _)| *name);
        let expected: Vec<_> = [0, 1, 3, 4]
            .into_iter()
            .map(|name| (name, Seen::Gone(Gone::Closed)))
            .chain(
                [0, 1, 3, 4]
                    .into_iter()
                    .map(|name| (100 + name, Seen::Gone(Gone::PeerClosed))),
            )
            .collect();
        assert_eq!(told, expected);
        child_io.stop();
        child_thread.join().unwrap();
        assert!(saw.try_recv().is_err(), "nothing more is told");
    });
}
