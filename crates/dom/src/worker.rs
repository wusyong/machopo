//! A dedicated worker's object in the realm that made it (HTML's `Worker`,
//! Blink's `DedicatedWorker`): an event target of that realm, with the
//! number its host knows the worker by. The worker runs elsewhere (a thread
//! of its own, a DOM of its own); what goes between the two is the host's.

use std::cell::Cell;

use elise::{Gc, GcObject};

use crate::{Cx, EventTargetImpl, ExecutionContext, sites};

#[derive(GcObject)]
pub struct WorkerObjectData {
    /// The number its host knows its worker by.
    id: Cell<u64>,
    /// Whether it was terminated (`terminate()`): nothing more goes to its
    /// worker, nor comes from it.
    terminated: Cell<bool>,
}

impl<'gc> Cx<'_, 'gc> {
    /// A new `Worker` of `context`'s realm, for the worker its host knows
    /// as `id`.
    pub fn create_worker_object(
        &self,
        context: Gc<'gc, ExecutionContext>,
        id: u64,
    ) -> Gc<'gc, EventTargetImpl> {
        let target = self.create_event_target();
        self.set_event_target_context(target, Some(context));
        let data = self.gc.alloc_in(
            &sites::OTHER,
            WorkerObjectDataInit {
                id: Cell::new(id),
                terminated: Cell::new(false),
            },
        );
        target.worker.set(self.gc, Some(data));
        target
    }

    /// Whether `target` is a `Worker`.
    pub fn is_worker(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        target.worker.get(self.gc).is_some()
    }

    /// The number `worker`'s host knows its worker by.
    ///
    /// # Panics
    ///
    /// If `worker` is no `Worker`.
    pub fn worker_id(&self, worker: Gc<'gc, EventTargetImpl>) -> u64 {
        self.worker_data(worker).id.get()
    }

    /// Whether `worker` was terminated.
    pub fn worker_terminated(&self, worker: Gc<'gc, EventTargetImpl>) -> bool {
        self.worker_data(worker).terminated.get()
    }

    /// `worker` is terminated: for good.
    pub fn terminate_worker_object(&self, worker: Gc<'gc, EventTargetImpl>) {
        self.worker_data(worker).terminated.set(true);
    }

    fn worker_data(&self, worker: Gc<'gc, EventTargetImpl>) -> Gc<'gc, WorkerObjectData> {
        worker.worker.get(self.gc).expect("a `Worker`")
    }
}
