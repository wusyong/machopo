//! A data pipe (Mojo's `mojo::DataPipe`, `CreateDataPipe`): a one-way
//! stream of bytes from a [`Producer`] to a [`Consumer`], with at most its
//! capacity of them written and not yet read. A producer that writes more
//! waits (an `async` write: the network service runs on tokio) until the
//! consumer reads. The consumer never waits: it reads what there is, and
//! is told when there is more ([`Consumer::watch`], Mojo's
//! `SimpleWatcher`), so a thread with an event loop of its own (a
//! renderer's main thread) posts itself a task for it.
//!
//! Both ends are in one process for now: what is written is kept as the
//! `Bytes` it came in, and read out without a copy. Once the renderer is a
//! process the pipe becomes a ring in shared memory, with the same limit
//! (a ring is only ever written up to full), and reading becomes two-phase
//! (a span of the ring, then how much of it was used).

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard};

use bytes::Bytes;
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
        Consumer { shared },
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
    shared: Arc<Shared>,
}

impl Consumer {
    /// Takes at most `max` bytes (at least one) of what was written, without
    /// waiting: a chunk as it was written, or its first `max` bytes.
    /// [`Read::Empty`] asks for the watcher to be called once there is more.
    pub fn read(&mut self, max: usize) -> Read {
        let max = max.max(1);
        let part = {
            let mut state = self.shared.lock();
            let Some(front) = state.chunks.front_mut() else {
                if state.producer_closed {
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
        self.shared.writable.notify_one();
        Read::Data(part)
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
        let on_readable: OnReadable = Arc::new(on_readable);
        let readable = {
            let mut state = self.shared.lock();
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

impl Drop for Consumer {
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

impl fmt::Debug for Consumer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Consumer").finish_non_exhaustive()
    }
}
