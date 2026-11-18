//! The pipe two entangled message ports share (Chromium's Mojo message
//! pipe, which a `MessagePortDescriptorPair` makes for every
//! `MessageChannel`): each port holds one end, and the messages posted
//! through one wait in the pipe for the other until its thread takes them.
//! An end travels in a message as it is (a port shipped: its messages not
//! delivered yet stay in the pipe), so the two ports may be on different
//! threads, each its own DOM's.
//!
//! The pipe is shared by the two ends, which may be on two threads, so it
//! is an `Arc` and a `Mutex` (one lock per message posted or taken, which
//! no one else wants but at the moment a thread posts to the other). An
//! end dropped closes it: a port's when it is closed, collected (on the
//! GC's sweep worker) or its DOM goes, one in a message that is dropped
//! undelivered; the other end then gets no message again, once it took
//! those that came before. Nothing is dropped while the lock is held: an
//! end in a message dropped would take its own pipe's.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::ThreadId;

use crate::SerializedMessage;

/// What wakes a DOM's thread when one of its ports may have a message to
/// deliver, or stopped waiting for one (the port it is entangled with
/// closed): the host's (`Dom::set_port_waker`). Called from any thread,
/// the GC's sweep workers too.
pub type PortWaker = Arc<dyn Fn() + Send + Sync>;

/// One end of a pipe (Chromium's `MessagePortDescriptor`): a port's, or one
/// traveling in a message. Dropped, it is closed.
pub struct PortEnd {
    pipe: Arc<Pipe>,
    /// Which of the pipe's two it is.
    side: usize,
}

struct Pipe {
    ends: Mutex<[EndState; 2]>,
}

#[derive(Default)]
struct EndState {
    /// The messages posted to it, in order.
    queue: VecDeque<Queued>,
    closed: bool,
    /// Its port's thread: none while it travels in a message.
    home: Option<Home>,
}

struct Queued {
    message: SerializedMessage,
    /// When its home's DOM queued it as a task, and the task's number
    /// (`Cx::stamp_queued_task`): none till its thread first looks (one
    /// posted from another thread, or before the end came home).
    stamp: Option<(f64, u64)>,
}

/// Where an end's port is: its DOM's thread, and what wakes it (none for a
/// DOM no other thread posts to).
#[derive(Clone)]
pub(crate) struct Home {
    pub(crate) thread: ThreadId,
    pub(crate) waker: Option<PortWaker>,
}

impl Home {
    fn is_current(&self) -> bool {
        self.thread == std::thread::current().id()
    }
}

impl PortEnd {
    /// A new pipe's two ends.
    pub(crate) fn pair() -> (Self, Self) {
        let pipe = Arc::new(Pipe {
            ends: Mutex::new([EndState::default(), EndState::default()]),
        });
        (
            Self {
                pipe: pipe.clone(),
                side: 0,
            },
            Self { pipe, side: 1 },
        )
    }

    fn lock(&self) -> MutexGuard<'_, [EndState; 2]> {
        self.pipe
            .ends
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn other(&self) -> usize {
        self.side ^ 1
    }

    /// Whether `other` is the end this one is entangled with.
    pub(crate) fn is_entangled_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.pipe, &other.pipe) && self.side != other.side
    }

    /// Its port is at `home` now (none: it travels); the messages waiting
    /// for it are stamped anew when its thread looks.
    pub(crate) fn set_home(&self, home: Option<Home>) {
        let mut ends = self.lock();
        let end = &mut ends[self.side];
        for queued in &mut end.queue {
            queued.stamp = None;
        }
        end.home = home;
    }

    /// `message` posted to the other end: stamped by `stamp` if that end's
    /// port is on this thread (its DOM's number for it), else when its
    /// thread looks, which is woken. Given back, to be dropped, if the
    /// other end is closed.
    pub(crate) fn post(
        &self,
        message: SerializedMessage,
        stamp: impl FnOnce() -> (f64, u64),
    ) -> Result<(), SerializedMessage> {
        let waker = {
            let mut ends = self.lock();
            let other = &mut ends[self.other()];
            if other.closed {
                return Err(message);
            }
            let here = other.home.as_ref().is_some_and(Home::is_current);
            other.queue.push_back(Queued {
                message,
                stamp: here.then(stamp),
            });
            if here {
                None
            } else {
                other.home.as_ref().and_then(|home| home.waker.clone())
            }
        };
        if let Some(waker) = waker {
            waker();
        }
        Ok(())
    }

    /// Whether a message waits for it.
    pub(crate) fn has_message(&self) -> bool {
        !self.lock()[self.side].queue.is_empty()
    }

    /// Its first message's stamp, those not stamped yet stamped by `stamp`
    /// (in their order) first.
    pub(crate) fn first_stamp(&self, mut stamp: impl FnMut() -> (f64, u64)) -> Option<(f64, u64)> {
        let mut ends = self.lock();
        let queue = &mut ends[self.side].queue;
        for queued in queue.iter_mut().filter(|queued| queued.stamp.is_none()) {
            queued.stamp = Some(stamp());
        }
        queue.front().and_then(|queued| queued.stamp)
    }

    /// Its first message, taken.
    pub(crate) fn take(&self) -> Option<SerializedMessage> {
        self.lock()[self.side]
            .queue
            .pop_front()
            .map(|queued| queued.message)
    }

    /// Whether no message is to come: none waits, and the other end is
    /// closed.
    pub(crate) fn is_done(&self) -> bool {
        let ends = self.lock();
        ends[self.side].queue.is_empty() && ends[self.other()].closed
    }
}

impl Drop for PortEnd {
    /// Closed: the messages that wait for it are dropped (once the lock is
    /// let go: those carry ends too, maybe this pipe's), and the other
    /// end's thread is woken, its port to stop waiting.
    fn drop(&mut self) {
        let (lost, waker) = {
            let mut ends = self.lock();
            let end = &mut ends[self.side];
            end.closed = true;
            end.home = None;
            let lost = std::mem::take(&mut end.queue);
            let waker = ends[self.other()]
                .home
                .as_ref()
                .filter(|home| !home.is_current())
                .and_then(|home| home.waker.clone());
            (lost, waker)
        };
        drop(lost);
        if let Some(waker) = waker {
            waker();
        }
    }
}
