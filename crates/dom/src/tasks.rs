//! A page's tasks, as its host's scheduler runs them: each task the DOM
//! queues (an element task, a posted message, a port's once it may come)
//! or arms (a timer, as it is set and as an interval is set again; the
//! UA's own timers; an idle callback's timeout) gets a number from one
//! counter, and the host's scheduler is told of it, a token (`TaskWaker`:
//! due now, or at the instant a timer is due). The scheduler orders the
//! tokens with its other tasks (Chromium's `SequenceManager` by
//! `EnqueueOrder`, given as an immediate task is posted and as a delayed
//! one is due); as one comes, the host runs the task of its number
//! (`Cx::task_numbered`, and the numbered takes), wherever it is in its
//! queue, as each of Blink's tasks is a closure of its own: a
//! `setTimeout(f, 0)` set before a `postMessage` runs before its message,
//! whichever task source each is of.

use elise::Gc;

use crate::{Cx, Dom, ExecutionContext, NodeId, NodeRef};

/// What tells the host's scheduler of a task the DOM numbered
/// (`Dom::set_task_waker`): its number, and the instant it is due (none:
/// now).
pub type TaskWaker = Box<dyn Fn(u64, Option<std::time::Instant>)>;

/// A task's number this DOM gave and when it was queued, its token told to
/// the host's scheduler (`TaskWaker`): made only by `Cx::stamp_task`, its
/// fields this module's, so that what holds one holds a task that will run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Stamp {
    queued: f64,
    number: u64,
}

impl Stamp {
    /// When the task was queued (the DOM's clock).
    pub(crate) fn queued(self) -> f64 {
        self.queued
    }

    pub(crate) fn number(self) -> u64 {
        self.number
    }
}

/// The queue a task is in (`Cx::task_numbered`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskQueue {
    /// A script's timer (`setTimeout`, `setInterval`).
    Timer,
    /// One of the UA's own timers (`Cx::add_native_timer`).
    NativeTimer,
    /// An idle callback's timeout.
    IdleTimeout,
    /// An element task (`Cx::queue_element_task`): the window's `load`
    /// among them.
    Element,
    /// A message posted to the window, or to one of its enabled ports.
    Message,
}

impl Dom {
    /// The documents whose window's `load` task ran since the host last
    /// took them (`Cx::note_document_loaded`), in order.
    pub fn take_loaded_documents(&self) -> Vec<NodeId> {
        self.changes.loaded_documents.take()
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// Milliseconds since the DOM was made: the clock tasks are ordered
    /// on (windows' time origins differ).
    pub fn dom_now(&self) -> f64 {
        self.origin.elapsed().as_secs_f64() * 1000.0
    }

    /// `document`'s window's `load` task ran (`queue_window_load`): its host
    /// finishes its load (`Dom::take_loaded_documents`).
    pub fn note_document_loaded(&self, document: NodeRef<'gc>) {
        let mut loaded = self.changes.loaded_documents.take();
        loaded.push(self.node_id(document));
        self.changes.loaded_documents.set(loaded);
    }

    /// The next task's number, taken.
    pub(crate) fn next_task_number(&self) -> u64 {
        let number = self.changes.task_sequence.get();
        self.changes
            .task_sequence
            .set(number.checked_add(1).expect("fewer than 2^64 tasks"));
        number
    }

    /// A task queued now, to run as soon as it may: its time and number,
    /// the host's scheduler told.
    pub(crate) fn stamp_queued_task(&self) -> (f64, u64) {
        let number = self.next_task_number();
        self.wake_for_task(number, None);
        (self.dom_now(), number)
    }

    /// `stamp_queued_task`'s, as a `Stamp`: the one way to make one, so
    /// that a stamp is a token the host's scheduler was told of.
    pub(crate) fn stamp_task(&self) -> Stamp {
        let (queued, number) = self.stamp_queued_task();
        Stamp { queued, number }
    }

    /// The host's scheduler told of task `number` (`Dom::set_task_waker`):
    /// due now (`None`), or at `at` on the DOM's clock (`dom_now`'s
    /// milliseconds; one never due, infinite, isn't told).
    pub(crate) fn wake_for_task(&self, number: u64, at: Option<f64>) {
        let Some(waker) = &self.changes.task_waker else {
            return;
        };
        match at {
            None => waker(number, None),
            Some(at) if at.is_finite() => waker(number, self.instant_at(at)),
            Some(_) => {}
        }
    }

    /// The instant the DOM's clock says `at` (`dom_now`'s milliseconds),
    /// if there is one.
    fn instant_at(&self, at: f64) -> Option<std::time::Instant> {
        std::time::Duration::try_from_secs_f64(at.max(0.0) / 1000.0)
            .ok()
            .and_then(|since| self.origin.checked_add(since))
    }

    /// The instant `ms` milliseconds since `context`'s time origin is, as
    /// the host's scheduler is told of its tasks: a test's clock
    /// (`TaskTokens::take_due_by`).
    pub fn instant_in(
        &self,
        context: Gc<'gc, ExecutionContext>,
        ms: f64,
    ) -> Option<std::time::Instant> {
        self.instant_at(context.time_origin.get() + ms)
    }

    /// The queue of `context`'s task numbered `number`, the one a token of
    /// the host's scheduler stands for, wherever it is in that queue (as
    /// each of Blink's tasks is a closure of its own, whatever was queued
    /// before it): none if that task is no more (cancelled, run, its window
    /// gone). Whether it is due is the scheduler's to say: the host takes
    /// it by its number (`take_timer_numbered` and the others).
    pub fn task_numbered(
        &self,
        context: Gc<'gc, ExecutionContext>,
        number: u64,
    ) -> Option<TaskQueue> {
        if self.find_timer(context, number).is_some() {
            Some(TaskQueue::Timer)
        } else if self.find_native_timer(context, number).is_some() {
            Some(TaskQueue::NativeTimer)
        } else if self
            .context_window(context)
            .and_then(|window| self.find_idle_timeout(window, number))
            .is_some()
        {
            Some(TaskQueue::IdleTimeout)
        } else if self.find_element_task(context, number).is_some() {
            Some(TaskQueue::Element)
        } else if self.find_message(context, number).is_some() {
            Some(TaskQueue::Message)
        } else {
            None
        }
    }

    /// Whether a task of `context`'s of any kind is due by `by` (the DOM's
    /// clock), in no order: debug builds' check as the host's thread is to
    /// wait with nothing in its scheduler, when such a task's token was
    /// lost and it never runs. A new kind of task is to be asked of here
    /// too (one left out is only not caught).
    pub fn has_task_due_by(&self, context: Gc<'gc, ExecutionContext>, by: f64) -> bool {
        self.has_timer_due_by(context, by)
            || self.has_native_timer_due_by(context, by)
            || self
                .context_window(context)
                .is_some_and(|window| self.has_idle_timeout_due_by(window, by))
            || self.has_element_task_by(context, by)
            || self.has_message_by(context, by)
    }
}
