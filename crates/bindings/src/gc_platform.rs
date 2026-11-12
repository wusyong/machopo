//! The elise heap's parallel work on V8's platform (`elise::Platform` over the
//! fork's `v8::machopo::post_job`): a heap V8 drives shares V8's workers with
//! V8's and cppgc's own GCs, as cppgc's does in Chrome (gin's
//! `V8Platform`, over `base::ThreadPool`), instead of a pool of its own
//! that doubles the threads while both mark.
//!
//! A job's runs share a `Task`: the job, and its runs' first panic,
//! resumed where `elise::Platform` says (`run`'s return, the handle's
//! `cancel`). A panic never unwinds into V8: a run caught panicking makes
//! the job's later runs return at once and its concurrency 0.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::sync::{Arc, Mutex, PoisonError};

/// The process's V8 platform, as `elise::Platform` (initialising V8 first if
/// needed), for a heap V8 drives.
pub fn gc_platform() -> Arc<dyn elise::Platform> {
    Arc::new(V8GcPlatform(crate::platform()))
}

struct V8GcPlatform(v8::SharedRef<v8::Platform>);

impl elise::Platform for V8GcPlatform {
    /// Posts the job (user-blocking: the heap's thread waits for it), runs
    /// it here as its joining run, and then cancels it, which returns once
    /// every worker's run has. Not V8's `Join`: it runs the job on the
    /// calling thread only while the job asks for a worker, but a job's
    /// first items may be the joining run's alone (its seed), asking for
    /// none.
    fn run(&self, job: &dyn elise::Job) {
        // SAFETY: the reference lives past every run of the job: the
        // cancel below returns once the workers' runs have, and starts no
        // new one; V8 may drop the task (and the `Arc` holding the
        // reference) later, which never reads it.
        let job: &'static dyn elise::Job =
            unsafe { std::mem::transmute::<&dyn elise::Job, _>(job) };
        let task = Arc::new(Task {
            job: Shared::Borrowed(job),
            panic: Mutex::new(None),
        });
        let handle = self.post(&task, true);
        let joining = catch_unwind(AssertUnwindSafe(|| job.run(&Joining(&handle))));
        if let Err(panic) = joining {
            task.panicked(panic);
        }
        handle.stop();
    }

    fn post_job(&self, job: Arc<dyn elise::Job + Send>) -> Option<Box<dyn elise::JobHandle>> {
        let task = Arc::new(Task {
            job: Shared::Owned(job),
            panic: Mutex::new(None),
        });
        Some(Box::new(self.post(&task, false)))
    }

    fn posts_jobs(&self) -> bool {
        true
    }

    /// The heap's phases as spans on machopo's trace (`MACHOPO_TRACE`), on the
    /// thread that runs them: inside V8's GC events when V8 drives the
    /// heap (its steps and pause), as cppgc's are; a worker's run of a job
    /// on the worker's track. A load and a branch with no trace.
    fn tracing_controller(&self) -> &'static dyn elise::TracingController {
        crate::trace::gc_tracing_controller()
    }
}

impl V8GcPlatform {
    fn post(&self, task: &Arc<Task>, user_blocking: bool) -> Handle {
        // SAFETY: the platform lives as long as the process (`PLATFORM`).
        let handle =
            unsafe { v8::machopo::post_job(&self.0, user_blocking, Box::new(Run(task.clone()))) };
        Handle {
            handle: Mutex::new(Some(handle)),
            task: task.clone(),
        }
    }
}

/// What a job's runs share: the job, and the first panic of one.
struct Task {
    job: Shared,
    panic: Mutex<Option<Box<dyn Any + Send>>>,
}

enum Shared {
    /// `run`'s, which outlives the job's runs.
    Borrowed(&'static dyn elise::Job),
    /// A posted job's.
    Owned(Arc<dyn elise::Job + Send>),
}

impl Task {
    fn job(&self) -> &dyn elise::Job {
        match &self.job {
            Shared::Borrowed(job) => *job,
            Shared::Owned(job) => &**job,
        }
    }

    fn kept(&self) -> std::sync::MutexGuard<'_, Option<Box<dyn Any + Send>>> {
        self.panic.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn has_panicked(&self) -> bool {
        self.kept().is_some()
    }

    /// Keeps `panic`, unless one was kept before.
    fn panicked(&self, panic: Box<dyn Any + Send>) {
        self.kept().get_or_insert(panic);
    }

    fn resume_panic(&self) {
        let panic = self.kept().take();
        if let Some(panic) = panic {
            resume_unwind(panic);
        }
    }
}

/// The task V8 runs on its workers.
struct Run(Arc<Task>);

impl v8::machopo::JobTask for Run {
    fn run(&self, delegate: &v8::machopo::JobDelegate) {
        if self.0.has_panicked() {
            return;
        }
        let ran = catch_unwind(AssertUnwindSafe(|| self.0.job().run(&Delegate(delegate))));
        if let Err(panic) = ran {
            self.0.panicked(panic);
        }
    }

    /// Under V8's job's lock.
    fn max_concurrency(&self, worker_count: usize) -> usize {
        if self.0.has_panicked() {
            return 0;
        }
        catch_unwind(AssertUnwindSafe(|| {
            self.0.job().max_concurrency(worker_count)
        }))
        .unwrap_or_else(|panic| {
            self.0.panicked(panic);
            0
        })
    }
}

/// A posted job's V8 `JobHandle` (none once cancelled), and its task.
/// Dropped, the V8 handle cancels the job and waits for its runs (a job
/// isn't left running past its handle, as `elise::ThreadPool`'s); a run's
/// panic then goes with it, as when unwinding.
struct Handle {
    handle: Mutex<Option<v8::machopo::JobHandle>>,
    task: Arc<Task>,
}

impl Handle {
    /// Cancels the job, unless it was: its runs yield, and this returns
    /// once they have, resuming a run's panic.
    fn stop(&self) {
        let handle = self
            .handle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some(handle) = handle else {
            return;
        };
        handle.cancel();
        self.task.resume_panic();
    }
}

impl elise::JobHandle for Handle {
    fn notify_concurrency_increase(&self) {
        if let Some(handle) = &*self.handle.lock().unwrap_or_else(PoisonError::into_inner) {
            handle.notify_concurrency_increase();
        }
    }

    fn cancel(&self) {
        self.stop();
    }
}

/// The calling thread's run of `V8GcPlatform::run`'s job.
struct Joining<'a>(&'a Handle);

impl elise::JobDelegate for Joining<'_> {
    fn notify_concurrency_increase(&self) {
        elise::JobHandle::notify_concurrency_increase(self.0);
    }

    fn is_joining_thread(&self) -> bool {
        true
    }
}

/// A run on V8's worker.
struct Delegate<'a>(&'a v8::machopo::JobDelegate);

impl elise::JobDelegate for Delegate<'_> {
    fn notify_concurrency_increase(&self) {
        self.0.notify_concurrency_increase();
    }

    fn is_joining_thread(&self) -> bool {
        // V8 runs it on its workers only: the calling thread of `run` runs
        // it itself, with `Joining`.
        false
    }

    fn should_yield(&self) -> bool {
        self.0.should_yield()
    }
}
