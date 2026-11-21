//! `ipc::endpoint` over the platform within one process: two ends, a host
//! (which may attach handles and makes regions) and a child, turned by
//! hand so each test sees every step; then on two threads, as they run.
//! And a child's end that isn't ours: whatever it sends, the host closes
//! the channel and says why.

use std::thread;

use bytes::Bytes;
use ipc::core::channel::{self, Gone, MAX_QUEUED_BYTES};
use ipc::core::control::{CONTROL_DESTINATION, Control};
use ipc::core::frame::BadMessage;
use ipc::core::region::HeapMemory;
use ipc::data_pipe::{self, Consumer, Read, TryWriteError};
use ipc::endpoint::{self, Config, Endpoint, Handler, Link, Received, SendError};
use ipc::local::{self, Local};
use serde::{Deserialize, Serialize};

const KIB: usize = 1 << 10;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
enum Msg {
    Text(String),
    Body(Bytes),
    Stream { pipe: u32 },
}

/// Keeps what came, and the pipes the messages named.
#[derive(Default)]
struct Recorder {
    got: Vec<(u32, Msg)>,
    pipes: Vec<Consumer>,
}

impl Handler<Local> for Recorder {
    fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
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
}

impl End {
    fn new(platform: Local, config: Config) -> Self {
        let (link, endpoint) = endpoint::endpoint(platform, config);
        Self {
            link,
            endpoint,
            recorder: Recorder::default(),
            gone: None,
        }
    }

    /// One turn, if the platform has something: whether it did.
    fn turn(&mut self) -> bool {
        let mut bytes = Vec::new();
        let mut handles = Vec::new();
        let Some(event) = self.link.platform().poll(&mut bytes, &mut handles) else {
            return false;
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

#[test]
fn two_ends_on_two_threads_run_until_one_closes() {
    // What can go wrong here is a wait that never ends: the test says so
    // itself, instead of hanging.
    let (done, finished) = std::sync::mpsc::channel();
    thread::spawn(move || {
        two_ends_on_two_threads();
        done.send(()).unwrap();
    });
    match finished.recv_timeout(std::time::Duration::from_secs(20)) {
        Ok(()) => {}
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("stuck: a message, a wake or the channel's end never came")
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => panic!("failed (above)"),
    }
}

fn two_ends_on_two_threads() {
    use std::sync::mpsc;

    let (host_end, child_end) = local::pair(Some(64 * KIB));
    let (host, host_endpoint) = endpoint::endpoint(host_end, config(true));
    let (child, child_endpoint) = endpoint::endpoint(child_end, config(false));

    /// Counts what comes (telling the test of each), and reads each pipe
    /// a message names to its end on a thread of its own, which then says
    /// so to the peer.
    struct Sink {
        link: Link<Local>,
        came: mpsc::Sender<()>,
        texts: usize,
        bodies: Vec<Bytes>,
        readers: Vec<thread::JoinHandle<Vec<u8>>>,
    }
    impl Handler<Local> for &mut Sink {
        fn message(&mut self, mut message: Received<'_, Local>) -> Result<(), BadMessage> {
            match message.decode::<Msg>()? {
                Msg::Text(_) => self.texts += 1,
                Msg::Body(body) => self.bodies.push(body),
                Msg::Stream { pipe } => {
                    let mut reader = message.take_pipe(pipe).ok_or(BadMessage::Rejected)?;
                    let link = self.link.clone();
                    self.readers.push(thread::spawn(move || {
                        let mut got = Vec::new();
                        loop {
                            match reader.read(40 * KIB) {
                                Read::Data(bytes) => got.extend_from_slice(&bytes),
                                Read::Empty => thread::yield_now(),
                                Read::Done => break,
                            }
                        }
                        link.send(0, &text("the pipe is read")).unwrap();
                        got
                    }));
                }
            }
            self.came.send(()).unwrap();
            Ok(())
        }
    }
    let run = |endpoint: Endpoint<Local>, link: Link<Local>| {
        let (came, count) = mpsc::channel();
        let thread = thread::spawn(move || {
            let mut sink = Sink {
                link,
                came,
                texts: 0,
                bodies: Vec::new(),
                readers: Vec::new(),
            };
            let gone = endpoint.run(&mut sink);
            (gone, sink)
        });
        (thread, count)
    };
    let (host_thread, host_came) = run(host_endpoint, host.clone());
    let (child_thread, child_came) = run(child_endpoint, child.clone());

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
    // Everything has arrived at both ends (the host's last is the child's
    // word that the pipe is read); then the host closes.
    for _ in 0..120 + 15 + 1 {
        child_came.recv().unwrap();
    }
    for _ in 0..120 + 15 + 1 {
        host_came.recv().unwrap();
    }
    host.close();
    let (host_gone, host_sink) = host_thread.join().unwrap();
    let (child_gone, mut child_sink) = child_thread.join().unwrap();
    assert_eq!(host_gone, Gone::Closed);
    assert_eq!(child_gone, Gone::PeerClosed);
    assert_eq!(host_sink.texts, 121);
    assert_eq!(child_sink.texts, 120);
    for sink in [&host_sink, &child_sink] {
        assert_eq!(sink.bodies.len(), 15);
        for n in 0..3u8 {
            assert_eq!(
                sink.bodies
                    .iter()
                    .filter(|got| **got == pattern(100 * KIB, n))
                    .count(),
                5
            );
        }
    }
    let got = child_sink.readers.pop().unwrap().join().unwrap();
    assert!(
        got == body[..],
        "the pipe's bytes: {} of {}",
        got.len(),
        body.len()
    );
    assert_eq!(
        child.send(0, &text("after")),
        Err(SendError::Gone(Gone::PeerClosed))
    );
}
