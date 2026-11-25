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
use crate::tasks::Stamp;

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

/// One end's state, a small state machine: its messages wait unnumbered
/// (`incoming`) till its port is enabled and home (`running`); then each
/// is numbered as it comes, a token to its home's scheduler (`ready`), the
/// order of the two queues the order posted (Chromium's: a port's messages
/// stay in its Mojo pipe till `start()`, then are read on its thread's
/// task runner, a task each). Only `PortEnd`'s methods change it: those
/// in `ready` all have tokens, those in `incoming` none.
#[derive(Default)]
struct EndState {
    /// Posted to it, not numbered yet: its port not enabled, away (in a
    /// message), or they came from another thread since its thread last
    /// looked.
    incoming: VecDeque<SerializedMessage>,
    /// Numbered, their tokens told: its port enabled and home. Before
    /// those in `incoming`.
    ready: VecDeque<(Stamp, SerializedMessage)>,
    /// Its port is enabled (`start()`, `onmessage`) and on its home's
    /// thread: messages that reach it there are numbered as they come.
    running: bool,
    closed: bool,
    /// Its port's thread: none while it travels in a message.
    home: Option<Home>,
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

    /// Its port is at `home` now (none: it travels in a message): not
    /// running there till it is enabled; the messages numbered for its old
    /// home wait unnumbered again, first, in order (their tokens there find
    /// nothing: they are taken by number).
    pub(crate) fn move_to(&self, home: Option<Home>) {
        let mut ends = self.lock();
        let end = &mut ends[self.side];
        end.running = false;
        let ready = std::mem::take(&mut end.ready);
        for (_, message) in ready.into_iter().rev() {
            end.incoming.push_front(message);
        }
        end.home = home;
    }

    /// Its port is enabled, at home (`start()`, an `onmessage`): running
    /// from now on, the messages waiting numbered by `stamp`, in order.
    pub(crate) fn enable(&self, mut stamp: impl FnMut() -> Stamp) {
        let mut ends = self.lock();
        let end = &mut ends[self.side];
        end.running = true;
        while let Some(message) = end.incoming.pop_front() {
            end.ready.push_back((stamp(), message));
        }
    }

    /// Its thread woken for messages from another thread (`PortWaker`): if
    /// it runs, those that came numbered by `stamp`, in order.
    pub(crate) fn arrived(&self, mut stamp: impl FnMut() -> Stamp) {
        let mut ends = self.lock();
        let end = &mut ends[self.side];
        if !end.running {
            return;
        }
        while let Some(message) = end.incoming.pop_front() {
            end.ready.push_back((stamp(), message));
        }
    }

    /// `message` posted to the other end: numbered by `stamp` (its DOM's
    /// number for it, a token) if that end runs on this thread, those that
    /// came before it unnumbered (from this end's old thread, before it
    /// was shipped here) first; else left for it unnumbered, its thread
    /// woken if it is another's. Given back, to be dropped, if the other
    /// end is closed.
    pub(crate) fn post(
        &self,
        message: SerializedMessage,
        mut stamp: impl FnMut() -> Stamp,
    ) -> Result<(), SerializedMessage> {
        let waker = {
            let mut ends = self.lock();
            let other = &mut ends[self.other()];
            if other.closed {
                return Err(message);
            }
            let here = other.home.as_ref().is_some_and(Home::is_current);
            if here && other.running {
                while let Some(earlier) = other.incoming.pop_front() {
                    other.ready.push_back((stamp(), earlier));
                }
                other.ready.push_back((stamp(), message));
            } else {
                other.incoming.push_back(message);
            }
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

    /// Whether a message waits for it, numbered or not.
    pub(crate) fn has_message(&self) -> bool {
        let ends = self.lock();
        let end = &ends[self.side];
        !end.ready.is_empty() || !end.incoming.is_empty()
    }

    /// Its first message's stamp, if it is numbered (it runs, and the
    /// message reached it here).
    pub(crate) fn first_stamp(&self) -> Option<Stamp> {
        self.lock()[self.side].ready.front().map(|&(stamp, _)| stamp)
    }

    /// Its first numbered message, taken.
    pub(crate) fn take(&self) -> Option<SerializedMessage> {
        self.lock()[self.side]
            .ready
            .pop_front()
            .map(|(_, message)| message)
    }

    /// Whether no message is to come: none waits, and the other end is
    /// closed.
    pub(crate) fn is_done(&self) -> bool {
        let ends = self.lock();
        let end = &ends[self.side];
        end.ready.is_empty() && end.incoming.is_empty() && ends[self.other()].closed
    }
}

impl Drop for PortEnd {
    /// Closed: the messages that wait for it are dropped (once the lock is
    /// let go: those carry ends too, maybe this pipe's), and the other
    /// end's thread is woken, its port to stop waiting (this thread's too:
    /// nothing else looks at its ports but the task its waker posts).
    fn drop(&mut self) {
        let (lost, waker) = {
            let mut ends = self.lock();
            let end = &mut ends[self.side];
            end.closed = true;
            end.home = None;
            end.running = false;
            let lost = (std::mem::take(&mut end.incoming), std::mem::take(&mut end.ready));
            let waker = ends[self.other()]
                .home
                .as_ref()
                .and_then(|home| home.waker.clone());
            (lost, waker)
        };
        drop(lost);
        if let Some(waker) = waker {
            waker();
        }
    }
}
