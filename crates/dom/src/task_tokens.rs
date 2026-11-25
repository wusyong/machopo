//! The least of a host's scheduler for a DOM's task tokens
//! (`Dom::set_task_waker`), for hosts with no thread of their own to run
//! them, which is tests (Chromium's `base::test::TaskEnvironment`): the
//! tokens go into a `base` sequence manager, the one the renderer's threads
//! use, so that the order tests see is theirs; `take_due` hands those due
//! now, to run by their numbers (`Cx::task_numbered` and the numbered
//! takes, or an engine's `run_dom_task`).

use std::rc::Rc;
use std::time::Instant;

use ::base::sequence_manager::{SequenceManager, Spec};

use crate::TaskWaker;

/// A DOM's task tokens, in the order a thread's scheduler takes them.
pub struct TaskTokens {
    // Shared with the waker the DOM holds, on this thread alone (a
    // `TaskWaker` isn't `Send`): what the DOM tells, the test takes.
    manager: Rc<SequenceManager<TokenSpec>>,
}

struct TokenSpec;

impl Spec for TokenSpec {
    type Queue = ();
    type Task = u64;
    type Reply = ();
    type Extra = ();

    const QUEUES: &'static [()] = &[()];

    fn index((): ()) -> usize {
        0
    }
}

impl Default for TaskTokens {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskTokens {
    pub fn new() -> Self {
        Self {
            manager: Rc::new(SequenceManager::default()),
        }
    }

    /// What tells it of each task its DOM numbers (`Dom::set_task_waker`):
    /// one due now queued, one for later held till it is due.
    pub fn waker(&self) -> TaskWaker {
        let manager = self.manager.clone();
        Box::new(move |number, at: Option<Instant>| {
            let posted = match at {
                Some(at) => manager.post_at((), number, at),
                None => manager.post((), number),
            };
            posted.expect("its manager is never closed");
        })
    }

    /// The numbers of the tasks due now, taken, in their order: those
    /// queued as they run wait for the next call (Blink's
    /// `test::RunPendingTasks`), so that an interval of no delay doesn't
    /// keep it going.
    pub fn take_due(&self) -> Vec<u64> {
        self.take_due_by(Instant::now())
    }

    /// `take_due`'s, as if it were `at` (a test's clock, past the real one:
    /// `Cx::instant_in` gives a window's time as an instant).
    pub fn take_due_by(&self, at: Instant) -> Vec<u64> {
        std::iter::from_fn(|| self.manager.try_next_by(|()| true, at)).collect()
    }
}
