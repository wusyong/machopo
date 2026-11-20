//! A channel's two sides over whatever carries its bytes (Chromium's
//! `mojo::core::Channel`): any thread sends, and never waits; one thread,
//! the one that reads the transport, takes what came and hears once that
//! the channel is gone.
//!
//! A send that the transport can't take whole waits in a queue here, in
//! order, behind one lock with the numbering of the messages. A peer that
//! stops reading fills the queue to its limit and no further: the channel
//! is closed.

use std::collections::VecDeque;

use super::frame::{BadMessage, Decoder, EncodeError, Encoder, Message};
use super::sync::{Arc, Mutex, MutexGuard, lock};

/// The most bytes that wait to be written before the peer counts as not
/// reading: two of the longest messages.
pub const MAX_QUEUED_BYTES: usize = 256 << 20;

/// A send buffer kept between messages is no larger than this: a large
/// message's is let go.
const SCRATCH_KEPT: usize = 64 << 10;

/// Why a channel is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gone {
    /// The peer closed its end, or is no more.
    PeerClosed,
    /// The peer sent what a working one doesn't.
    BadMessage(BadMessage),
    /// The peer stopped reading: more than the queue's limit waited.
    QueueFull,
    /// This end closed it.
    Closed,
}

/// Why a write took nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteError {
    /// The transport has no room now.
    WouldBlock,
    /// The peer is gone.
    Closed,
}

/// What a platform does for a channel's sending side. Its reading side is
/// the platform's own loop, which feeds a [`Receiver`].
pub trait Transport {
    type Handle;

    /// Writes the start of `bytes`, which are one message's (what is left
    /// of it), without waiting, and says how much it took: all of it, some
    /// of it (a stream), or none (`WouldBlock`). `handles` go with the
    /// message's first byte: a write that takes any byte takes them all,
    /// out of the vector.
    fn write(&self, bytes: &[u8], handles: &mut Vec<Self::Handle>) -> Result<usize, WriteError>;

    /// Has the thread that reads the transport call [`Receiver::woken`],
    /// soon and from its loop: there is something to write when the
    /// transport has room, or the channel was closed from another thread.
    /// Called with no lock of the channel's held.
    fn wake(&self);
}

/// What a channel is made with.
#[derive(Debug, Clone, Copy)]
pub struct Config {
    /// How many destinations this end has: a message for one from this
    /// number up is a bad message.
    pub destinations: u32,
    /// This end may attach handles to what it sends.
    pub may_attach: bool,
    /// The peer's end may.
    pub peer_may_attach: bool,
    /// [`MAX_QUEUED_BYTES`], but for a test.
    pub queue_limit: usize,
}

/// Why a message wasn't sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError {
    /// The channel is over.
    Gone(Gone),
    /// This end's own mistake: nothing was sent, and the channel goes on.
    Encode(EncodeError),
    /// Handles from an end that may attach none: as `Encode`.
    HandlesNotAllowed,
}

/// A message, or what is left of it to write.
struct Pending<H> {
    bytes: Vec<u8>,
    /// How many of `bytes` are written.
    written: usize,
    /// Its handles, until its first byte is written.
    handles: Vec<H>,
}

/// What the sending side's lock guards: the numbering of the messages and
/// the order they are written in are one.
struct Outgoing<H> {
    encoder: Encoder,
    /// Where a message is put together, kept from one to the next.
    scratch: Vec<u8>,
    queue: VecDeque<Pending<H>>,
    /// The bytes in `queue` not yet written.
    queued: usize,
    /// Why the channel is over: the first reason there was.
    gone: Option<Gone>,
}

impl<H> Outgoing<H> {
    /// The channel is over, for `gone` if for nothing before: what waited
    /// is dropped, its handles with it.
    fn close(&mut self, gone: Gone) -> Gone {
        self.queue.clear();
        self.queued = 0;
        *self.gone.get_or_insert(gone)
    }
}

struct Shared<T: Transport> {
    transport: T,
    outgoing: Mutex<Outgoing<T::Handle>>,
    may_attach: bool,
    queue_limit: usize,
}

impl<T: Transport> Shared<T> {
    fn outgoing(&self) -> MutexGuard<'_, Outgoing<T::Handle>> {
        lock(&self.outgoing)
    }

    /// Closes the channel and has the reading thread hear of it.
    fn close(&self, mut outgoing: MutexGuard<'_, Outgoing<T::Handle>>, gone: Gone) -> Gone {
        let first = outgoing.close(gone);
        drop(outgoing);
        self.transport.wake();
        first
    }
}

/// A channel over `transport`: its sending side, for any thread, and its
/// receiving side, for the thread that reads the transport.
pub fn channel<T: Transport>(transport: T, config: Config) -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Shared {
        transport,
        outgoing: Mutex::new(Outgoing {
            encoder: Encoder::new(),
            scratch: Vec::new(),
            queue: VecDeque::new(),
            queued: 0,
            gone: None,
        }),
        may_attach: config.may_attach,
        queue_limit: config.queue_limit,
    });
    let receiver = Receiver {
        shared: shared.clone(),
        decoder: Decoder::new(config.peer_may_attach),
        destinations: config.destinations,
        reported: false,
    };
    (Sender(shared), receiver)
}

/// A channel's sending side.
pub struct Sender<T: Transport>(Arc<Shared<T>>);

impl<T: Transport> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

/// What [`Sender::flush`] left.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flushed {
    /// Nothing waits.
    Empty,
    /// The transport took what it could: more waits for room.
    Pending,
}

impl<T: Transport> Sender<T> {
    /// Sends a message to the peer's `destination`, without waiting: it is
    /// written now if the transport takes it, and waits in order if not.
    pub fn send(
        &self,
        destination: u32,
        payload: &[u8],
        mut handles: Vec<T::Handle>,
    ) -> Result<(), SendError> {
        if !handles.is_empty() && !self.0.may_attach {
            return Err(SendError::HandlesNotAllowed);
        }
        let mut outgoing = self.0.outgoing();
        if let Some(gone) = outgoing.gone {
            return Err(SendError::Gone(gone));
        }
        let mut bytes = std::mem::take(&mut outgoing.scratch);
        bytes.clear();
        if let Err(error) = outgoing
            .encoder
            .encode(destination, payload, handles.len(), &mut bytes)
        {
            outgoing.scratch = bytes;
            return Err(SendError::Encode(error));
        }
        // Straight to the transport, unless something waits: that goes
        // first.
        let mut written = 0;
        if outgoing.queue.is_empty() {
            match self.0.transport.write(&bytes, &mut handles) {
                Ok(taken) => written = taken.min(bytes.len()),
                Err(WriteError::WouldBlock) => {}
                Err(WriteError::Closed) => {
                    return Err(SendError::Gone(self.0.close(outgoing, Gone::PeerClosed)));
                }
            }
            if written == bytes.len() {
                if bytes.capacity() <= SCRATCH_KEPT {
                    outgoing.scratch = bytes;
                }
                return Ok(());
            }
        }
        let rest = bytes.len().saturating_sub(written);
        let Some(queued) = outgoing
            .queued
            .checked_add(rest)
            .filter(|&queued| queued <= self.0.queue_limit)
        else {
            return Err(SendError::Gone(self.0.close(outgoing, Gone::QueueFull)));
        };
        let first = outgoing.queue.is_empty();
        outgoing.queued = queued;
        outgoing.queue.push_back(Pending {
            bytes,
            written,
            handles,
        });
        drop(outgoing);
        if first {
            // The reading thread starts watching for room.
            self.0.transport.wake();
        }
        Ok(())
    }

    /// Writes what waits, as far as the transport takes it. For the
    /// reading thread, when the transport has room again.
    pub fn flush(&self) -> Flushed {
        let mut outgoing = self.0.outgoing();
        while let Some(front) = outgoing.queue.front_mut() {
            let left = front.bytes.get(front.written..).unwrap_or_default();
            let taken = match self.0.transport.write(left, &mut front.handles) {
                Ok(taken) => taken.min(left.len()),
                Err(WriteError::WouldBlock) => 0,
                Err(WriteError::Closed) => {
                    self.0.close(outgoing, Gone::PeerClosed);
                    return Flushed::Empty;
                }
            };
            if taken == 0 {
                return Flushed::Pending;
            }
            let whole = taken == left.len();
            front.written = front.written.saturating_add(taken);
            outgoing.queued = outgoing.queued.saturating_sub(taken);
            if whole {
                outgoing.queue.pop_front();
            }
        }
        Flushed::Empty
    }

    /// Closes the channel from this end: nothing more is sent, and the
    /// receiving side hears [`Gone::Closed`].
    pub fn close(&self) {
        self.0.close(self.0.outgoing(), Gone::Closed);
    }

    /// How many bytes wait to be written.
    pub fn queued(&self) -> usize {
        self.0.outgoing().queued
    }

    /// The transport the channel is over.
    pub fn transport(&self) -> &T {
        &self.0.transport
    }
}

/// A channel's receiving side: the thread that reads the transport feeds
/// it what it read. Each of its calls may answer that the channel is
/// gone, which exactly one of them does, once; after it no message is
/// given out.
pub struct Receiver<T: Transport> {
    shared: Arc<Shared<T>>,
    decoder: Decoder<T::Handle>,
    destinations: u32,
    /// The channel's end was answered.
    reported: bool,
}

impl<T: Transport> Receiver<T> {
    /// The answer that the channel is gone, if it is and wasn't answered.
    fn report(&mut self, gone: Option<Gone>) -> Option<Gone> {
        if self.reported {
            return None;
        }
        if gone.is_some() {
            self.reported = true;
            // What was read of a message that won't be finished, and its
            // handles, go now.
            self.decoder = Decoder::new(false);
        }
        gone
    }

    /// What the transport read: `sink` is given each message it
    /// completes, in the order sent, and refuses one it can make nothing
    /// of ([`BadMessage::Rejected`]). A bad message closes the channel.
    pub fn feed(
        &mut self,
        bytes: &[u8],
        handles: impl IntoIterator<Item = T::Handle>,
        mut sink: impl FnMut(Message<'_, T::Handle>) -> Result<(), BadMessage>,
    ) -> Option<Gone> {
        if self.reported {
            return None;
        }
        // Closed from another thread: what came is dropped.
        let gone = self.shared.outgoing().gone;
        if gone.is_some() {
            return self.report(gone);
        }
        let destinations = self.destinations;
        let fed = self.decoder.try_feed(bytes, handles, |message| {
            if message.destination >= destinations {
                return Err(BadMessage::UnknownDestination {
                    destination: message.destination,
                });
            }
            sink(message)
        });
        match fed {
            Ok(()) => None,
            Err(bad) => {
                let first = self
                    .shared
                    .close(self.shared.outgoing(), Gone::BadMessage(bad));
                self.report(Some(first))
            }
        }
    }

    /// The transport says the peer is gone (its end closed, the process
    /// ended).
    pub fn peer_gone(&mut self) -> Option<Gone> {
        if self.reported {
            return None;
        }
        let first = self.shared.close(self.shared.outgoing(), Gone::PeerClosed);
        self.report(Some(first))
    }

    /// The transport's `wake` came round: whether the channel was closed
    /// from another thread.
    pub fn woken(&mut self) -> Option<Gone> {
        let gone = self.shared.outgoing().gone;
        self.report(gone)
    }
}

/// A transport for the tests: what is written is kept, as far as the room
/// it was given goes.
#[cfg(test)]
mod wire {
    use super::super::sync::{Mutex, lock};
    use super::{Transport, WriteError};

    #[derive(Default)]
    pub(super) struct State<H> {
        /// Everything written, in order.
        pub(super) written: Vec<u8>,
        /// The handles written, alive.
        pub(super) handles: Vec<H>,
        /// How many more bytes are taken before `WouldBlock`.
        pub(super) room: usize,
        /// The most one write takes; 0: any number.
        pub(super) per_write: usize,
        /// A message is taken whole or not at all, as a Mach message is.
        pub(super) whole_only: bool,
        pub(super) closed: bool,
        pub(super) wakes: usize,
    }

    pub(super) struct Wire<H>(Mutex<State<H>>);

    impl<H> Wire<H> {
        pub(super) fn new(room: usize) -> Self {
            Self(Mutex::new(State {
                written: Vec::new(),
                handles: Vec::new(),
                room,
                per_write: 0,
                whole_only: false,
                closed: false,
                wakes: 0,
            }))
        }

        pub(super) fn with<R>(&self, f: impl FnOnce(&mut State<H>) -> R) -> R {
            f(&mut lock(&self.0))
        }
    }

    impl<H> Transport for Wire<H> {
        type Handle = H;

        fn write(&self, bytes: &[u8], handles: &mut Vec<H>) -> Result<usize, WriteError> {
            let mut state = lock(&self.0);
            if state.closed {
                return Err(WriteError::Closed);
            }
            let mut taken = bytes.len().min(state.room);
            if state.per_write != 0 {
                taken = taken.min(state.per_write);
            }
            if taken == 0 || (state.whole_only && taken < bytes.len()) {
                return Err(WriteError::WouldBlock);
            }
            state.written.extend_from_slice(&bytes[..taken]);
            state.room -= taken;
            state.handles.append(handles);
            Ok(taken)
        }

        fn wake(&self) {
            lock(&self.0).wakes += 1;
        }
    }
}

#[cfg(all(test, not(any(ipc_loom, ipc_shuttle))))]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::wire::Wire;
    use super::*;
    use crate::core::frame::{HEADER_LEN, MAX_PAYLOAD_LEN};

    /// A handle that says which it is, and counts itself when dropped.
    #[derive(Debug)]
    struct TestHandle(u32, Arc<AtomicUsize>);

    impl Drop for TestHandle {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test's count of drops: a handful"
        )]
        fn drop(&mut self) {
            self.1.fetch_add(1, Ordering::Relaxed);
        }
    }

    const CONFIG: Config = Config {
        destinations: 4,
        may_attach: true,
        peer_may_attach: true,
        queue_limit: 1000,
    };

    type TestSender = Sender<Wire<TestHandle>>;
    type TestReceiver = Receiver<Wire<TestHandle>>;

    fn pair(room: usize, config: Config) -> (TestSender, TestReceiver) {
        channel(Wire::new(room), config)
    }

    /// What was written, as the peer would read it: each message's
    /// destination, payload and handles.
    fn read_back(sender: &TestSender) -> Vec<(u32, Vec<u8>, Vec<u32>)> {
        let (bytes, handles) = sender.transport().with(|wire| {
            (
                wire.written.clone(),
                wire.handles
                    .iter()
                    .map(|handle| handle.0)
                    .collect::<Vec<_>>(),
            )
        });
        let mut decoder = Decoder::new(true);
        let mut messages = Vec::new();
        decoder
            .feed(&bytes, handles, |message| {
                messages.push((
                    message.destination,
                    message.payload.to_vec(),
                    message.handles,
                ));
            })
            .unwrap();
        assert_eq!(decoder.buffered(), 0, "a message was cut short");
        messages
    }

    /// A stream of messages as a peer sends them.
    fn from_peer(messages: &[(u32, &[u8])]) -> Vec<u8> {
        let mut encoder = Encoder::new();
        let mut bytes = Vec::new();
        for (destination, payload) in messages {
            encoder
                .encode(*destination, payload, 0, &mut bytes)
                .unwrap();
        }
        bytes
    }

    #[test]
    fn a_send_the_transport_takes_is_written_at_once() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, _receiver) = pair(usize::MAX, CONFIG);
        sender
            .send(1, b"one", vec![TestHandle(7, dropped.clone())])
            .unwrap();
        sender.send(2, b"", Vec::new()).unwrap();
        assert_eq!(sender.queued(), 0);
        assert_eq!(sender.transport().with(|wire| wire.wakes), 0);
        assert_eq!(
            read_back(&sender),
            [(1, b"one".to_vec(), vec![7]), (2, Vec::new(), vec![])]
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    /// What the transport doesn't take waits, and is written from where
    /// it stopped when there is room; what is sent meanwhile waits behind
    /// it, even if there is room by then.
    #[test]
    fn what_isnt_taken_waits_in_order() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, _receiver) = pair(HEADER_LEN + 2, CONFIG);
        // Cut after two bytes of its payload: its handle went with its
        // first byte.
        sender
            .send(1, b"first", vec![TestHandle(7, dropped.clone())])
            .unwrap();
        assert_eq!(sender.queued(), 3);
        assert_eq!(
            sender
                .transport()
                .with(|wire| (wire.wakes, wire.handles.len())),
            (1, 1)
        );
        // Room again, and a second message: behind the first's rest.
        sender.transport().with(|wire| wire.room = usize::MAX);
        sender
            .send(2, b"second", vec![TestHandle(8, dropped.clone())])
            .unwrap();
        assert_eq!(sender.queued(), 3 + HEADER_LEN + 6);
        // One wake, when the queue stopped being empty.
        assert_eq!(
            sender
                .transport()
                .with(|wire| (wire.wakes, wire.handles.len())),
            (1, 1)
        );
        assert_eq!(sender.flush(), Flushed::Empty);
        assert_eq!(sender.queued(), 0);
        assert_eq!(
            read_back(&sender),
            [
                (1, b"first".to_vec(), vec![7]),
                (2, b"second".to_vec(), vec![8])
            ]
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    /// A flush goes as far as the room does, a piece at a time.
    #[test]
    fn a_flush_goes_as_far_as_there_is_room() {
        let (sender, _receiver) = pair(0, CONFIG);
        sender.send(0, &[1; 30], Vec::new()).unwrap();
        sender.send(0, &[2; 30], Vec::new()).unwrap();
        let all = 2 * (HEADER_LEN + 30);
        assert_eq!(sender.queued(), all);
        assert_eq!(sender.flush(), Flushed::Pending);
        assert_eq!(sender.queued(), all);
        sender.transport().with(|wire| {
            wire.room = HEADER_LEN + 40;
            wire.per_write = 7;
        });
        assert_eq!(sender.flush(), Flushed::Pending);
        assert_eq!(sender.queued(), all - (HEADER_LEN + 40));
        sender.transport().with(|wire| wire.room = usize::MAX);
        assert_eq!(sender.flush(), Flushed::Empty);
        assert_eq!(
            read_back(&sender),
            [(0, vec![1; 30], vec![]), (0, vec![2; 30], vec![])]
        );
    }

    /// A transport that takes a message whole or not at all (a Mach
    /// port's) is never given two messages as one, nor part of one.
    #[test]
    fn a_write_is_one_messages_bytes() {
        let (sender, _receiver) = pair(0, CONFIG);
        sender.transport().with(|wire| wire.whole_only = true);
        sender.send(0, &[1; 10], Vec::new()).unwrap();
        sender.send(0, &[2; 20], Vec::new()).unwrap();
        // Room for the first and half of the second: only the first goes.
        sender
            .transport()
            .with(|wire| wire.room = HEADER_LEN + 10 + HEADER_LEN + 10);
        assert_eq!(sender.flush(), Flushed::Pending);
        assert_eq!(
            sender.transport().with(|wire| wire.written.len()),
            HEADER_LEN + 10
        );
        assert_eq!(sender.queued(), HEADER_LEN + 20);
    }

    /// More waiting than the limit: the peer isn't reading, and the
    /// channel is closed. What waited is dropped with its handles; the
    /// reading thread is woken and hears of it once.
    #[test]
    fn a_queue_past_its_limit_closes_the_channel() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let limit = 2 * (HEADER_LEN + 10);
        let (sender, mut receiver) = pair(
            0,
            Config {
                queue_limit: limit,
                ..CONFIG
            },
        );
        sender
            .send(0, &[0; 10], vec![TestHandle(1, dropped.clone())])
            .unwrap();
        // Right at the limit is within it.
        sender.send(0, &[0; 10], Vec::new()).unwrap();
        assert_eq!(sender.queued(), limit);
        assert_eq!(sender.transport().with(|wire| wire.wakes), 1);
        assert_eq!(
            sender.send(0, b"", vec![TestHandle(2, dropped.clone())]),
            Err(SendError::Gone(Gone::QueueFull))
        );
        assert_eq!(sender.queued(), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
        assert_eq!(sender.transport().with(|wire| wire.wakes), 2);
        // Nothing more goes, room or not.
        sender.transport().with(|wire| wire.room = usize::MAX);
        assert_eq!(
            sender.send(0, b"", Vec::new()),
            Err(SendError::Gone(Gone::QueueFull))
        );
        assert_eq!(sender.flush(), Flushed::Empty);
        assert_eq!(sender.transport().with(|wire| wire.written.len()), 0);
        assert_eq!(receiver.woken(), Some(Gone::QueueFull));
        assert_eq!(receiver.woken(), None);
        assert_eq!(receiver.peer_gone(), None);
    }

    #[test]
    fn a_write_to_a_peer_that_is_gone_closes_the_channel() {
        // At a send.
        let (sender, mut receiver) = pair(usize::MAX, CONFIG);
        sender.transport().with(|wire| wire.closed = true);
        assert_eq!(
            sender.send(0, b"x", Vec::new()),
            Err(SendError::Gone(Gone::PeerClosed))
        );
        assert_eq!(sender.transport().with(|wire| wire.wakes), 1);
        assert_eq!(receiver.woken(), Some(Gone::PeerClosed));
        // At a flush.
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, mut receiver) = pair(0, CONFIG);
        sender
            .send(0, b"x", vec![TestHandle(1, dropped.clone())])
            .unwrap();
        sender.transport().with(|wire| wire.closed = true);
        assert_eq!(sender.flush(), Flushed::Empty);
        assert_eq!(sender.queued(), 0);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert_eq!(receiver.woken(), Some(Gone::PeerClosed));
        assert_eq!(
            sender.send(0, b"x", Vec::new()),
            Err(SendError::Gone(Gone::PeerClosed))
        );
    }

    /// This end's own mistakes send nothing, use no number, and leave the
    /// channel as it was.
    #[test]
    fn a_send_refused_leaves_the_channel_as_it_was() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, _receiver) = pair(
            usize::MAX,
            Config {
                may_attach: false,
                ..CONFIG
            },
        );
        assert_eq!(
            sender.send(0, b"x", vec![TestHandle(1, dropped.clone())]),
            Err(SendError::HandlesNotAllowed)
        );
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        let long = vec![0u8; MAX_PAYLOAD_LEN as usize + 1];
        assert_eq!(
            sender.send(0, &long, Vec::new()),
            Err(SendError::Encode(EncodeError::TooLong { len: long.len() }))
        );
        assert_eq!(sender.transport().with(|wire| wire.written.len()), 0);
        // The next is the first: `read_back` checks its number.
        sender.send(3, b"ok", Vec::new()).unwrap();
        assert_eq!(read_back(&sender), [(3, b"ok".to_vec(), vec![])]);
    }

    /// The buffer a message is put together in is kept for the next, but
    /// not a large message's.
    #[test]
    fn a_large_messages_buffer_isnt_kept() {
        let (sender, _receiver) = pair(usize::MAX, CONFIG);
        sender.send(0, &[0; 100], Vec::new()).unwrap();
        let small = sender.0.outgoing().scratch.capacity();
        assert!((HEADER_LEN + 100..=SCRATCH_KEPT).contains(&small));
        sender
            .send(0, &vec![0; SCRATCH_KEPT * 2], Vec::new())
            .unwrap();
        assert_eq!(sender.0.outgoing().scratch.capacity(), 0);
    }

    #[test]
    fn messages_are_given_out_in_order_with_their_destinations() {
        let (_sender, mut receiver) = pair(0, CONFIG);
        let bytes = from_peer(&[(0, b"a"), (3, b"bc"), (1, b"")]);
        let mut got = Vec::new();
        // In two pieces, cut inside the second message.
        let cut = HEADER_LEN + 1 + HEADER_LEN + 1;
        for piece in [&bytes[..cut], &bytes[cut..]] {
            let gone = receiver.feed(piece, [], |message| {
                got.push((
                    message.destination,
                    message.sequence,
                    message.payload.to_vec(),
                ));
                Ok(())
            });
            assert_eq!(gone, None);
        }
        assert_eq!(
            got,
            [
                (0, 0, b"a".to_vec()),
                (3, 1, b"bc".to_vec()),
                (1, 2, Vec::new())
            ]
        );
    }

    /// A message for a destination this end hasn't got is a bad message:
    /// the channel is closed, heard of once, and nothing after it is
    /// given out or sent.
    #[test]
    fn an_unknown_destination_closes_the_channel() {
        let (sender, mut receiver) = pair(usize::MAX, CONFIG);
        let bytes = from_peer(&[(3, b"a"), (4, b"b"), (0, b"c")]);
        let mut got = Vec::new();
        let bad = Gone::BadMessage(BadMessage::UnknownDestination { destination: 4 });
        let gone = receiver.feed(&bytes, [], |message| {
            got.push(message.destination);
            Ok(())
        });
        assert_eq!(gone, Some(bad));
        assert_eq!(got, [3]);
        assert_eq!(sender.transport().with(|wire| wire.wakes), 1);
        assert_eq!(sender.send(0, b"x", Vec::new()), Err(SendError::Gone(bad)));
        // Heard of once; what comes after is dropped.
        let dropped = Arc::new(AtomicUsize::new(0));
        let more = from_peer(&[(0, b"d")]);
        let after = receiver.feed(&more, [TestHandle(1, dropped.clone())], |_| {
            panic!("a message after the end")
        });
        assert_eq!(after, None);
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        assert_eq!(receiver.woken(), None);
        assert_eq!(receiver.peer_gone(), None);
    }

    /// A payload its destination can make nothing of is as bad, and
    /// nothing after it in the same bytes is given out.
    #[test]
    fn a_payload_refused_closes_the_channel() {
        let (_sender, mut receiver) = pair(0, CONFIG);
        let bytes = from_peer(&[(0, b"good"), (0, b"bad"), (0, b"good")]);
        let mut got = 0;
        let gone = receiver.feed(&bytes, [], |message| {
            if message.payload == b"bad" {
                return Err(BadMessage::Rejected);
            }
            got += 1;
            Ok(())
        });
        assert_eq!(gone, Some(Gone::BadMessage(BadMessage::Rejected)));
        assert_eq!(got, 1);
    }

    /// The framing's own refusals come the same way.
    #[test]
    fn a_frame_refused_closes_the_channel() {
        let dropped = Arc::new(AtomicUsize::new(0));
        let (sender, mut receiver) = pair(
            0,
            Config {
                peer_may_attach: false,
                ..CONFIG
            },
        );
        sender.send(0, b"waits", Vec::new()).unwrap();
        let bytes = from_peer(&[(0, b"a")]);
        let gone = receiver.feed(&bytes, [TestHandle(1, dropped.clone())], |_| {
            panic!("no message")
        });
        assert_eq!(gone, Some(Gone::BadMessage(BadMessage::HandlesNotAllowed)));
        assert_eq!(dropped.load(Ordering::Relaxed), 1);
        // What waited to be sent is dropped too.
        assert_eq!(sender.queued(), 0);
    }

    /// The end is heard of once, whichever call finds it, and for the
    /// first reason there was.
    #[test]
    fn the_end_is_heard_of_once_for_its_first_reason() {
        // The peer gone: what was read of an unfinished message goes.
        let (sender, mut receiver) = pair(0, CONFIG);
        let bytes = from_peer(&[(0, b"abcdef")]);
        assert_eq!(
            receiver.feed(&bytes[..HEADER_LEN + 2], [], |_| panic!("no message")),
            None
        );
        assert_eq!(receiver.decoder.buffered(), HEADER_LEN + 2);
        assert_eq!(receiver.peer_gone(), Some(Gone::PeerClosed));
        assert_eq!(receiver.decoder.buffered(), 0);
        assert_eq!(receiver.peer_gone(), None);
        assert_eq!(receiver.woken(), None);
        assert_eq!(
            receiver.feed(&bytes[HEADER_LEN + 2..], [], |_| panic!("no message")),
            None
        );
        assert_eq!(
            sender.send(0, b"x", Vec::new()),
            Err(SendError::Gone(Gone::PeerClosed))
        );

        // Closed from this end, then the peer goes: the first reason.
        let (sender, mut receiver) = pair(0, CONFIG);
        sender.close();
        assert_eq!(sender.transport().with(|wire| wire.wakes), 1);
        assert_eq!(receiver.peer_gone(), Some(Gone::Closed));
        assert_eq!(receiver.woken(), None);

        // Closed from this end with bytes on their way in: they are
        // dropped, not given out.
        let (sender, mut receiver) = pair(0, CONFIG);
        sender.close();
        assert_eq!(
            receiver.feed(&bytes, [], |_| panic!("no message")),
            Some(Gone::Closed)
        );
        assert_eq!(receiver.feed(&bytes, [], |_| panic!("no message")), None);

        // Nothing is heard while it is open.
        let (_sender, mut receiver) = pair(0, CONFIG);
        assert_eq!(receiver.woken(), None);
    }
}

/// What the models check of a run: the bytes written are whole messages
/// numbered in order (the decoder refuses anything else), but for a last
/// one cut short when `closed`; each sender's messages are there in the
/// order it sent them. A payload is its sender's number and the message's
/// number with that sender. Gives how many messages of each sender got
/// written.
#[cfg(all(test, any(ipc_loom, ipc_shuttle)))]
fn written_in_order(wire: &wire::Wire<()>, senders: usize, closed: bool) -> Vec<u8> {
    let bytes = wire.with(|state| state.written.clone());
    let mut decoder = Decoder::<()>::new(false);
    let mut next = vec![0u8; senders];
    decoder
        .feed(&bytes, [], |message| {
            let [sender, number, ..] = *message.payload else {
                panic!("a payload cut short")
            };
            assert_eq!(
                number,
                next[usize::from(sender)],
                "sender {sender}'s messages out of order"
            );
            next[usize::from(sender)] += 1;
        })
        .expect("what was written is whole messages in order");
    assert!(
        closed || decoder.buffered() == 0,
        "a message cut short on an open channel"
    );
    next
}

/// Every interleaving of a few threads on a small channel
/// (`python3 scripts/test.py ipc models`).
#[cfg(all(test, ipc_loom))]
mod loom_tests {
    use loom::thread;

    use super::wire::Wire;
    use super::*;
    use crate::core::frame::HEADER_LEN;

    const CONFIG: Config = Config {
        destinations: 1,
        may_attach: false,
        peer_may_attach: false,
        queue_limit: 1000,
    };
    const MESSAGE: usize = HEADER_LEN + 2;

    /// Two threads send while a third makes room and flushes: every
    /// message is written once, whole, and the numbers on the wire are in
    /// order whichever thread wrote which piece.
    #[test]
    fn two_senders_and_a_flush() {
        loom::model(|| {
            // Room for less than one message: the first send is cut.
            let (sender, _receiver) = channel(Wire::<()>::new(MESSAGE - 5), CONFIG);
            let threads: Vec<_> = (0..2u8)
                .map(|number| {
                    let sender = sender.clone();
                    thread::spawn(move || sender.send(0, &[number, 0], Vec::new()).unwrap())
                })
                .collect();
            let flusher = {
                let sender = sender.clone();
                thread::spawn(move || {
                    sender.transport().with(|wire| wire.room += MESSAGE);
                    sender.flush();
                })
            };
            for thread in threads {
                thread.join().unwrap();
            }
            flusher.join().unwrap();
            sender.transport().with(|wire| wire.room = usize::MAX);
            assert_eq!(sender.flush(), Flushed::Empty);
            assert_eq!(sender.queued(), 0);
            assert_eq!(written_in_order(sender.transport(), 2, false), [1, 1]);
        });
    }

    /// A send against the peer going: the end is heard of once, the send
    /// went or was refused, and nothing waits or is sent afterwards.
    #[test]
    fn a_send_and_the_peer_going() {
        loom::model(|| {
            let (sender, mut receiver) = channel(Wire::<()>::new(MESSAGE - 5), CONFIG);
            let sending = {
                let sender = sender.clone();
                thread::spawn(move || sender.send(0, &[0, 0], Vec::new()))
            };
            let reading = thread::spawn(move || {
                let heard = receiver.peer_gone();
                (heard, receiver)
            });
            let sent = sending.join().unwrap();
            let (heard, mut receiver) = reading.join().unwrap();
            assert!(
                matches!(sent, Ok(()) | Err(SendError::Gone(Gone::PeerClosed))),
                "{sent:?}"
            );
            assert_eq!(heard, Some(Gone::PeerClosed));
            assert_eq!(receiver.woken(), None);
            assert_eq!(sender.queued(), 0);
            assert_eq!(
                sender.send(0, &[0, 1], Vec::new()),
                Err(SendError::Gone(Gone::PeerClosed))
            );
            // What got written before the end is the start of the one
            // message, never more.
            assert!(sender.transport().with(|wire| wire.written.len()) <= MESSAGE - 5);
            written_in_order(sender.transport(), 1, true);
        });
    }

    /// Two sends where one fits the queue: the other closes the channel,
    /// which is heard of once, for that reason.
    #[test]
    fn two_senders_and_a_full_queue() {
        loom::model(|| {
            let config = Config {
                queue_limit: MESSAGE,
                ..CONFIG
            };
            let (sender, mut receiver) = channel(Wire::<()>::new(0), config);
            let threads: Vec<_> = (0..2u8)
                .map(|number| {
                    let sender = sender.clone();
                    thread::spawn(move || sender.send(0, &[number, 0], Vec::new()))
                })
                .collect();
            let results: Vec<_> = threads
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect();
            let refused = results
                .iter()
                .filter(|result| **result == Err(SendError::Gone(Gone::QueueFull)))
                .count();
            assert_eq!(refused, 1, "{results:?}");
            assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
            assert_eq!(sender.queued(), 0);
            assert_eq!(receiver.woken(), Some(Gone::QueueFull));
            assert_eq!(receiver.woken(), None);
            // One wake for the queue's first message, one for the end.
            assert_eq!(sender.transport().with(|wire| wire.wakes), 2);
        });
    }
}

/// Random schedules of more threads and messages than loom can take
/// (`python3 scripts/test.py ipc models`).
#[cfg(all(test, ipc_shuttle))]
mod shuttle_tests {
    use shuttle::thread;

    use super::wire::Wire;
    use super::*;

    const SENDERS: u8 = 3;
    const MESSAGES: u8 = 12;
    const CONFIG: Config = Config {
        destinations: 1,
        may_attach: false,
        peer_may_attach: false,
        queue_limit: 100_000,
    };

    fn senders(sender: &Sender<Wire<()>>) -> Vec<thread::JoinHandle<u8>> {
        (0..SENDERS)
            .map(|number| {
                let sender = sender.clone();
                thread::spawn(move || {
                    let mut sent = 0;
                    for message in 0..MESSAGES {
                        // Payloads of several lengths.
                        let mut payload = vec![number, message];
                        payload.resize(2 + usize::from(message) * 5 % 23, 0xee);
                        if sender.send(0, &payload, Vec::new()).is_err() {
                            break;
                        }
                        sent += 1;
                    }
                    sent
                })
            })
            .collect()
    }

    /// The transport takes a few bytes at a time and a thread flushes as
    /// room comes: every message of every sender is written, each
    /// sender's in its order.
    #[test]
    fn senders_and_a_flusher() {
        shuttle::check_random(
            || {
                let (sender, _receiver) = channel(Wire::<()>::new(0), CONFIG);
                sender.transport().with(|wire| wire.per_write = 9);
                let sending = senders(&sender);
                let flusher = {
                    let sender = sender.clone();
                    thread::spawn(move || {
                        for _ in 0..40 {
                            sender.transport().with(|wire| wire.room += 31);
                            sender.flush();
                            thread::yield_now();
                        }
                    })
                };
                for thread in sending {
                    assert_eq!(thread.join().unwrap(), MESSAGES);
                }
                flusher.join().unwrap();
                sender.transport().with(|wire| wire.room = usize::MAX);
                assert_eq!(sender.flush(), Flushed::Empty);
                assert_eq!(
                    written_in_order(sender.transport(), usize::from(SENDERS), false),
                    [MESSAGES; SENDERS as usize]
                );
            },
            10_000,
        );
    }

    /// The same with the channel closed part of the way: the end is heard
    /// of once, and what was written by then is whole messages in order
    /// and at most the start of one more, none of them a message its
    /// sender was told didn't go.
    #[test]
    fn senders_and_a_close() {
        shuttle::check_random(
            || {
                let (sender, mut receiver) = channel(Wire::<()>::new(0), CONFIG);
                sender.transport().with(|wire| wire.per_write = 9);
                let sending = senders(&sender);
                let flusher = {
                    let sender = sender.clone();
                    thread::spawn(move || {
                        for round in 0..40 {
                            sender.transport().with(|wire| wire.room += 31);
                            sender.flush();
                            if round == 11 {
                                sender.close();
                            }
                            thread::yield_now();
                        }
                    })
                };
                let reading = thread::spawn(move || {
                    let mut heard = 0;
                    for _ in 0..20 {
                        heard += usize::from(receiver.woken().is_some());
                        thread::yield_now();
                    }
                    (heard, receiver)
                });
                let sent: Vec<u8> = sending
                    .into_iter()
                    .map(|thread| thread.join().unwrap())
                    .collect();
                flusher.join().unwrap();
                let (heard, mut receiver) = reading.join().unwrap();
                let heard = heard + usize::from(receiver.woken().is_some());
                assert_eq!(heard, 1);
                assert_eq!(receiver.peer_gone(), None);
                assert_eq!(sender.queued(), 0);
                let written = written_in_order(sender.transport(), usize::from(SENDERS), true);
                for (written, sent) in written.iter().zip(&sent) {
                    assert!(written <= sent, "{written} written of {sent} sent");
                }
            },
            10_000,
        );
    }
}
