//! A data pipe (Mojo's `mojo::DataPipe`, `CreateDataPipe`): a one-way
//! stream of bytes from a [`Producer`] to a [`Consumer`], with at most its
//! capacity of them written and not yet read. A producer that writes more
//! waits (an `async` write: the network service runs on tokio) until the
//! consumer reads. The consumer never waits: it reads what there is, and
//! is told when there is more ([`Consumer::watch`], Mojo's
//! `SimpleWatcher`), so a thread with an event loop of its own (a
//! renderer's main thread) posts itself a task for it.
//!
//! Both ends are in one process: what is written is kept as the `Bytes`
//! it came in, and read out without a copy. Between two processes there
//! is one of these in each, and the bytes go from the one to the other as
//! messages on the channel, with the same limit (`core::pipe`): the
//! receiving side writes with [`Producer::try_write`].
//!
//! A [`Consumer`] in a message crosses with it: serializing it, which
//! only this crate's codec can, takes the pipe out of the value and opens
//! one over the channel for it, and the value read at the other end is
//! the consumer of the pipe that end made (Mojo's handles in a message).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser};
use tokio::sync::Notify;

/// A pipe that holds at most `capacity` unread bytes (at least one).
pub fn create(capacity: usize) -> (Producer, Consumer) {
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            chunks: VecDeque::new(),
            unread: 0,
            capacity: capacity.max(1),
            producer_closed: false,
            consumer_closed: false,
            on_readable: None,
            armed: false,
        }),
        writable: Notify::new(),
    });
    (
        Producer {
            shared: shared.clone(),
        },
        Consumer::of(Ends {
            shared,
            observer: None,
        }),
    )
}

/// The other end of the pipe is gone: a write that can never be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed;

impl fmt::Display for Closed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the data pipe's consumer is gone")
    }
}

impl std::error::Error for Closed {}

/// Why [`Producer::try_write`] wrote nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryWriteError {
    /// There isn't room for all of it.
    Full,
    /// The consumer is gone.
    Closed,
}

/// What [`Consumer::read`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Read {
    /// Bytes, in the order they were written: never empty.
    Data(Bytes),
    /// Nothing yet; the producer may write more.
    Empty,
    /// Nothing more: everything written has been read, and the producer is
    /// gone.
    Done,
}

/// What is told of a consumer's reads ([`Consumer::observe`]): for the
/// receiving end of a pipe between processes, which has to tell the
/// sending end how much was read. Dropped with the consumer.
pub trait ReadObserver: Send {
    /// `count` bytes were read.
    fn read(&mut self, count: usize);

    /// The consumer read to the end: the producer is gone and nothing is
    /// left.
    fn done(&mut self);
}

/// What the consumer gave to be called when the pipe becomes readable.
type OnReadable = Arc<dyn Fn() + Send + Sync>;

/// Both ends' (each on a thread of its own): the state behind one lock,
/// held only for a few field updates, never while calling out.
struct Shared {
    state: Mutex<State>,
    /// Wakes a producer waiting for room, as the consumer reads or goes.
    /// One producer: a wake that comes while it isn't waiting is kept for
    /// its next wait (`notify_one`'s permit), so none is lost.
    writable: Notify,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        // Nothing panics while holding it (the watcher is called after it
        // is let go).
        self.state.lock().expect("a data pipe's state")
    }
}

struct State {
    /// What was written and not yet read, in order, as it came.
    chunks: VecDeque<Bytes>,
    /// Their bytes: at most `capacity`.
    unread: usize,
    capacity: usize,
    producer_closed: bool,
    consumer_closed: bool,
    on_readable: Option<OnReadable>,
    /// The consumer found the pipe empty since it was last told (or since
    /// it began to watch): the next write, or the producer's end, tells it.
    armed: bool,
}

impl State {
    fn readable(&self) -> bool {
        !self.chunks.is_empty() || self.producer_closed
    }

    /// The watcher to call now that the pipe became readable, if the
    /// consumer is waiting to be told.
    fn wake_consumer(&mut self) -> Option<OnReadable> {
        if !std::mem::take(&mut self.armed) {
            return None;
        }
        self.on_readable.clone()
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "`part` is no longer than the room left, `capacity - unread`: `unread` stays at most `capacity`"
    )]
    fn push(&mut self, part: Bytes) {
        self.unread += part.len();
        self.chunks.push_back(part);
    }

    #[expect(
        clippy::arithmetic_side_effects,
        reason = "`part` was taken out of `chunks`, whose bytes `unread` counts"
    )]
    fn taken(&mut self, part: &Bytes) {
        self.unread -= part.len();
    }
}

/// The writing end (Mojo's `DataPipeProducerHandle`). Dropping it ends the
/// stream: the consumer reads what is left, then [`Read::Done`].
pub struct Producer {
    shared: Arc<Shared>,
}

impl Producer {
    /// Writes all of `data`, as much at a time as there is room for,
    /// waiting while the pipe is full. An error once the consumer is gone:
    /// nothing written will be read (the load it was for was cancelled).
    pub async fn write(&mut self, mut data: Bytes) -> Result<(), Closed> {
        while !data.is_empty() {
            let wrote = {
                let mut state = self.shared.lock();
                if state.consumer_closed {
                    return Err(Closed);
                }
                let room = state.capacity.saturating_sub(state.unread);
                if room == 0 {
                    None
                } else {
                    let part = if data.len() > room {
                        data.split_to(room)
                    } else {
                        std::mem::take(&mut data)
                    };
                    state.push(part);
                    Some(state.wake_consumer())
                }
            };
            match wrote {
                Some(wake) => {
                    if let Some(on_readable) = wake {
                        on_readable();
                    }
                }
                None => self.shared.writable.notified().await,
            }
        }
        Ok(())
    }
}

impl Producer {
    /// Writes all of `data` now, or none of it: never waits. For a writer
    /// that is told how much room there is before it is given the bytes
    /// (the receiving end of a pipe between processes, whose sender keeps
    /// to the capacity): [`TryWriteError::Full`] is then the sender's
    /// mistake.
    pub fn try_write(&mut self, data: Bytes) -> Result<(), TryWriteError> {
        let wake = {
            let mut state = self.shared.lock();
            if state.consumer_closed {
                return Err(TryWriteError::Closed);
            }
            if data.is_empty() {
                return Ok(());
            }
            if data.len() > state.capacity.saturating_sub(state.unread) {
                return Err(TryWriteError::Full);
            }
            state.push(data);
            state.wake_consumer()
        };
        if let Some(on_readable) = wake {
            on_readable();
        }
        Ok(())
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        let wake = {
            let mut state = self.shared.lock();
            state.producer_closed = true;
            state.wake_consumer()
        };
        if let Some(on_readable) = wake {
            on_readable();
        }
    }
}

impl fmt::Debug for Producer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Producer").finish_non_exhaustive()
    }
}

/// The reading end (Mojo's `DataPipeConsumerHandle`). Dropping it closes
/// the pipe: what is unread is let go, and the producer's writes fail.
pub struct Consumer {
    /// None once the pipe was taken out, to cross a channel: what is left
    /// reads as a pipe that is over, and closes nothing when dropped.
    // A lock only because serializing borrows the value it takes the
    // pipe out of; every other use has the consumer to itself (`&mut`),
    // and locks nothing.
    ends: Mutex<Option<Ends>>,
}

/// A consumer's hold on its pipe.
struct Ends {
    shared: Arc<Shared>,
    observer: Option<Box<dyn ReadObserver>>,
}

impl Consumer {
    fn of(ends: Ends) -> Self {
        Self {
            ends: Mutex::new(Some(ends)),
        }
    }

    fn ends(&mut self) -> Option<&mut Ends> {
        self.ends
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
    }

    /// The pipe, taken out of this value: none if it was already.
    fn take(&self) -> Option<Self> {
        self.ends
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
            .map(Self::of)
    }

    /// How many unread bytes the pipe holds at most; 0 once it was taken
    /// out.
    pub fn capacity(&mut self) -> usize {
        self.ends().map_or(0, |ends| ends.shared.lock().capacity)
    }

    /// Takes at most `max` bytes (at least one) of what was written, without
    /// waiting: a chunk as it was written, or its first `max` bytes.
    /// [`Read::Empty`] asks for the watcher to be called once there is more.
    pub fn read(&mut self, max: usize) -> Read {
        let max = max.max(1);
        let Some(ends) = self.ends() else {
            return Read::Done;
        };
        let part = {
            let mut state = ends.shared.lock();
            let Some(front) = state.chunks.front_mut() else {
                if state.producer_closed {
                    drop(state);
                    if let Some(observer) = &mut ends.observer {
                        observer.done();
                    }
                    return Read::Done;
                }
                state.armed = true;
                return Read::Empty;
            };
            let part = if front.len() > max {
                front.split_to(max)
            } else {
                state.chunks.pop_front().expect("the front, just looked at")
            };
            state.taken(&part);
            part
        };
        ends.shared.writable.notify_one();
        if let Some(observer) = &mut ends.observer {
            observer.read(part.len());
        }
        Read::Data(part)
    }

    /// Has `observer` told of every read from now on, and of the end.
    /// Replaces the one given before.
    pub fn observe(&mut self, observer: Box<dyn ReadObserver>) {
        if let Some(ends) = self.ends() {
            ends.observer = Some(observer);
        }
    }

    /// Calls `on_readable` each time the pipe becomes readable after a
    /// [`read`](Self::read) found it empty: at once (here) if it is
    /// readable already, and then, after each read that returns
    /// [`Read::Empty`], once on the next write or the producer's end (on
    /// the producer's thread). Reading until `Empty` or `Done` is what asks
    /// for the next call; a consumer that stops reading earlier (a task
    /// that read its share) reads on without one. Replaces the one given
    /// before.
    pub fn watch(&mut self, on_readable: impl Fn() + Send + Sync + 'static) {
        let Some(ends) = self.ends() else {
            return;
        };
        let on_readable: OnReadable = Arc::new(on_readable);
        let readable = {
            let mut state = ends.shared.lock();
            state.on_readable = Some(on_readable.clone());
            let readable = state.readable();
            state.armed = !readable;
            readable
        };
        if readable {
            on_readable();
        }
    }
}

impl Drop for Ends {
    fn drop(&mut self) {
        {
            let mut state = self.shared.lock();
            state.consumer_closed = true;
            state.chunks.clear();
            state.unread = 0;
            state.on_readable = None;
        }
        self.shared.writable.notify_one();
    }
}

/// What a serializer is told a consumer is (a newtype struct of this
/// name): how this crate's codec knows one, in a value of any type.
pub(crate) const CONSUMER_NAME: &str = "$ipc::data_pipe::Consumer";

thread_local! {
    /// The consumer on its way between a value and the codec, for the
    /// length of one call: serde's data model has no place for a pipe, so
    /// it goes beside it. Set and taken on the one thread that is
    /// serializing or deserializing the value.
    static CROSSING: RefCell<Option<Consumer>> = const { RefCell::new(None) };
}

/// The consumer a value being serialized is giving up, for the codec.
pub(crate) fn take_crossing() -> Option<Consumer> {
    CROSSING.with(|crossing| crossing.borrow_mut().take())
}

/// The consumer the codec has for the value it is about to hand its
/// visitor.
pub(crate) fn set_crossing(consumer: Consumer) {
    CROSSING.with(|crossing| *crossing.borrow_mut() = Some(consumer));
}

/// What a serializer that isn't the codec gets to write of a consumer:
/// nothing.
struct OnlyInAMessage;

impl Serialize for OnlyInAMessage {
    fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(ser::Error::custom(
            "a data pipe's consumer is serialized only into an ipc message",
        ))
    }
}

/// Takes the pipe out of the value: it crosses with the message, and the
/// value left behind is a pipe that is over. Only this crate's codec
/// serializes one; any other serializer is refused, and the value keeps
/// its pipe. A value whose pipe was taken already is refused too.
impl Serialize for Consumer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let Some(pipe) = self.take() else {
            return Err(ser::Error::custom("a data pipe's consumer crosses once"));
        };
        set_crossing(pipe);
        let written = serializer.serialize_newtype_struct(CONSUMER_NAME, &OnlyInAMessage);
        // Not taken: the serializer wasn't the codec. The pipe stays the
        // value's.
        if let Some(pipe) = take_crossing() {
            *self.ends.lock().unwrap_or_else(PoisonError::into_inner) = pipe
                .ends
                .into_inner()
                .unwrap_or_else(PoisonError::into_inner);
        }
        written
    }
}

/// The consumer of the pipe the message's sender put there, which only
/// this crate's codec has to give.
impl<'de> Deserialize<'de> for Consumer {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Arrived;

        impl<'de> de::Visitor<'de> for Arrived {
            type Value = Consumer;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a data pipe's consumer, in an ipc message")
            }

            fn visit_u32<E: de::Error>(self, _number: u32) -> Result<Consumer, E> {
                take_crossing().ok_or_else(|| {
                    E::custom("a data pipe's consumer is read only out of an ipc message")
                })
            }
        }

        deserializer.deserialize_newtype_struct(CONSUMER_NAME, Arrived)
    }
}

impl fmt::Debug for Consumer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Consumer").finish_non_exhaustive()
    }
}
