//! The order a page's tasks run in, across its windows and their task
//! sources: each task the DOM queues (an element task, a posted or port
//! message) or arms (a timer, as it is set and as an interval is set
//! again; the UA's own timers; an idle callback's timeout) gets a number
//! from one counter, and the time it may run at, on the DOM's clock (when
//! it was queued, or when a timer is due). The host runs the earliest
//! `(time, number)` first, one at a time (Chromium's `SequenceManager`
//! picks among its queues by `EnqueueOrder`, given as an immediate task is
//! posted and as a delayed one is due): a `setTimeout(f, 0)` set before a
//! `postMessage` runs before its message, whichever task source each is
//! of.
//!
//! A pass of the host's (`begin_task_pass`) runs only the tasks numbered
//! before it began: a task one of them queues, or an interval it sets
//! again, waits for the next pass, so pages that post to each other
//! forever give way to rendering and input.

use elise::Gc;

use crate::{Cx, Dom, NodeId, NodeRef, Window};

/// The queue a window's next task is in (`Cx::next_task`).
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

/// A window's next task: when it may run (milliseconds since the DOM was
/// made), its number, and its queue.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NextTask {
    pub at: f64,
    pub number: u64,
    pub queue: TaskQueue,
}

impl NextTask {
    /// Whether it runs before `other`: the earlier, ties by number.
    pub fn before(&self, other: &Self) -> bool {
        self.at
            .total_cmp(&other.at)
            .then(self.number.cmp(&other.number))
            .is_lt()
    }
}

impl Dom {
    /// Whether a task that is due now (an element task, a message) may be
    /// queued: one `bool`, which the host reads after each of its own
    /// tasks. It may still be set once they ran (a pass clears it as it
    /// ends, `Cx::end_task_pass`).
    pub fn has_queued_tasks(&self) -> bool {
        self.changes.tasks_queued.get()
    }

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

    /// A task queued now, to run as soon as it may: its time and number;
    /// `has_queued_tasks` is set.
    pub(crate) fn stamp_queued_task(&self) -> (f64, u64) {
        self.changes.tasks_queued.set(true);
        (self.dom_now(), self.next_task_number())
    }

    /// Whether a task of number `number` may run now: in a pass, only one
    /// numbered before it began.
    pub(crate) fn task_may_run(&self, number: u64) -> bool {
        self.changes
            .task_limit
            .get()
            .is_none_or(|limit| number < limit)
    }

    /// A pass of the host's begins: until it ends, only the tasks numbered
    /// so far run.
    pub fn begin_task_pass(&self) {
        self.changes
            .task_limit
            .set(Some(self.changes.task_sequence.get()));
    }

    /// The pass ends: `has_queued_tasks` says whether one of `windows` has
    /// a task waiting that doesn't wait for a time (one the pass held
    /// back, or queued meanwhile).
    pub fn end_task_pass(&self, windows: &[Gc<'gc, Window>]) {
        self.changes.task_limit.set(None);
        let queued = windows
            .iter()
            .any(|&window| self.has_element_task(window) || self.has_posted_message(window));
        self.changes.tasks_queued.set(queued);
    }

    /// `window`'s task to run next by `now` (the DOM's clock), of those
    /// the pass lets run: the earliest `(time, number)` of its queues.
    pub fn next_task(&self, window: Gc<'gc, Window>, now: f64) -> Option<NextTask> {
        let origin = self.window_time_origin(window);
        let candidates = [
            self.next_due_timer(window)
                .map(|(due, number)| (origin + due, number, TaskQueue::Timer)),
            self.next_due_native_timer(window)
                .map(|(due, number)| (origin + due, number, TaskQueue::NativeTimer)),
            self.next_due_idle_timeout(window)
                .map(|(due, number)| (origin + due, number, TaskQueue::IdleTimeout)),
            self.next_element_task(window)
                .map(|(at, number)| (at, number, TaskQueue::Element)),
            self.next_posted_message(window)
                .map(|(at, number)| (at, number, TaskQueue::Message)),
        ];
        candidates
            .into_iter()
            .flatten()
            .filter(|&(at, _, _)| at <= now)
            .map(|(at, number, queue)| NextTask { at, number, queue })
            .reduce(|best, task| if task.before(&best) { task } else { best })
    }
}
