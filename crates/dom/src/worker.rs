//! A dedicated worker's object in the realm that made it (HTML's `Worker`,
//! Blink's `DedicatedWorker`): an event target of that realm, with the
//! number its host knows the worker by. The worker runs elsewhere (a thread
//! of its own, a DOM of its own); what goes between the two is the host's.
//! What its worker threw waits in it for its task (`queue_worker_error`).

use std::cell::Cell;

use elise::{Gc, GcObject, LocalExternal};

use crate::{Cx, ErrorInfo, EventTargetImpl, ExecutionContext, sites};

#[derive(GcObject)]
pub struct WorkerObjectData {
    /// The number its host knows its worker by.
    id: Cell<u64>,
    /// Whether it was terminated (`terminate()`): nothing more goes to its
    /// worker, nor comes from it.
    terminated: Cell<bool>,
    /// What its worker threw, in order, each waiting for its task.
    errors: LocalExternal<Cell<Vec<ErrorInfo>>>,
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
                errors: LocalExternal::new(Cell::new(Vec::new())),
            },
        );
        target.worker.set(self.gc, Some(data));
        let workers = match context.workers.get(self.gc) {
            Some(workers) => workers,
            None => {
                let workers = elise::HeapVector::new(self.gc, 1);
                context.workers.set(self.gc, Some(workers));
                workers
            }
        };
        workers.push(self.gc, target);
        target
    }

    /// `context`'s `Worker`s not terminated, in the order made.
    pub fn context_workers(
        &self,
        context: Gc<'gc, ExecutionContext>,
    ) -> Vec<Gc<'gc, EventTargetImpl>> {
        context
            .workers
            .get(self.gc)
            .map_or_else(Vec::new, |workers| {
                (0..workers.len())
                    .filter_map(|index| workers.get(self.gc, index))
                    .collect()
            })
    }

    /// `context`'s `Worker` of the worker numbered `id`, if it isn't
    /// terminated.
    pub fn context_worker(
        &self,
        context: Gc<'gc, ExecutionContext>,
        id: u64,
    ) -> Option<Gc<'gc, EventTargetImpl>> {
        let workers = context.workers.get(self.gc)?;
        (0..workers.len())
            .filter_map(|index| workers.get(self.gc, index))
            .find(|&worker| self.worker_id(worker) == id)
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

    /// `worker` is terminated: for good, no more its realm's (nothing of
    /// its worker comes to it).
    pub fn terminate_worker_object(&self, worker: Gc<'gc, EventTargetImpl>) {
        self.worker_data(worker).terminated.set(true);
        let workers = self
            .target_context(crate::TargetRef::EventTarget(worker))
            .and_then(|context| context.workers.get(self.gc));
        if let Some(workers) = workers {
            let index = (0..workers.len()).find(|&index| {
                workers
                    .get(self.gc, index)
                    .is_some_and(|w| Gc::ptr_eq(w, worker))
            });
            if let Some(index) = index {
                workers.remove(self.gc, index);
            }
        }
    }

    /// What `worker`'s worker threw and didn't cancel, `info`: a task of
    /// `worker`'s realm fires it (HTML's "queue a global task" to fire an
    /// `ErrorEvent` at it), after what the worker posted before it; not
    /// fired if it is terminated by then.
    pub fn queue_worker_error(&self, worker: Gc<'gc, EventTargetImpl>, info: ErrorInfo) {
        let Some(context) = self.target_context(crate::TargetRef::EventTarget(worker)) else {
            return;
        };
        let cell = self.worker_data(worker).into_ref().errors.get();
        let mut queued = cell.take();
        queued.push(info);
        cell.set(queued);
        self.push_target_task(context, worker, crate::window::WORKER_ERROR, 0);
    }

    /// The first of `worker`'s errors queued (its task runs).
    pub(crate) fn take_worker_error(&self, worker: Gc<'gc, EventTargetImpl>) -> Option<ErrorInfo> {
        let cell = self.worker_data(worker).into_ref().errors.get();
        let mut queued = cell.take();
        let first = (!queued.is_empty()).then(|| queued.remove(0));
        cell.set(queued);
        first
    }

    fn worker_data(&self, worker: Gc<'gc, EventTargetImpl>) -> Gc<'gc, WorkerObjectData> {
        worker.worker.get(self.gc).expect("a `Worker`")
    }
}
