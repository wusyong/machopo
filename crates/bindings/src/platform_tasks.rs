//! V8's foreground tasks, the embedder's to schedule (Blink's
//! `gin::V8Platform` posts each to the renderer scheduler's task runner
//! for V8): the platform hands every task posted for an isolate to Rust
//! (`v8::PlatformImpl`, on any thread), which keeps it in that isolate's
//! queue and wakes the isolate's thread through the waker its owner set
//! (`V8PerIsolateData::set_platform_task_waker`); the thread runs them as
//! a task of its loop's, from the outermost loop
//! (`V8PerIsolateData::pump_platform_tasks`), as V8's own pump would.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// What wakes an isolate's thread for its platform tasks: one ready now
/// (`None`), or one delayed till the instant given.
pub type PlatformTaskWaker = Arc<dyn Fn(Option<Instant>) + Send + Sync>;

/// An isolate's platform tasks waiting for its thread, and what wakes it.
#[derive(Default)]
struct IsolateTasks {
    ready: VecDeque<v8::Task>,
    /// Those posted with a delay, with when each is due.
    delayed: Vec<(Instant, v8::Task)>,
    waker: Option<PlatformTaskWaker>,
}

/// Every isolate's, by its address (`isolate_key`): one lock, which V8's
/// threads take to post and an isolate's thread to take its tasks.
static TASKS: LazyLock<Mutex<HashMap<usize, IsolateTasks>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn tasks() -> MutexGuard<'static, HashMap<usize, IsolateTasks>> {
    TASKS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The platform's side: each task V8 posts for an isolate, kept for its
/// thread (`post`).
pub(crate) struct EmbedderPlatform;

impl v8::PlatformImpl for EmbedderPlatform {
    fn post_task(&self, isolate: *mut std::ffi::c_void, task: v8::Task) {
        post(isolate.addr(), task, None);
    }

    fn post_non_nestable_task(&self, isolate: *mut std::ffi::c_void, task: v8::Task) {
        // The loop never runs them nested.
        post(isolate.addr(), task, None);
    }

    fn post_delayed_task(&self, isolate: *mut std::ffi::c_void, task: v8::Task, delay: f64) {
        post(isolate.addr(), task, Some(delay));
    }

    fn post_non_nestable_delayed_task(
        &self,
        isolate: *mut std::ffi::c_void,
        task: v8::Task,
        delay: f64,
    ) {
        post(isolate.addr(), task, Some(delay));
    }

    /// None come: the platform is made without idle tasks (as before); one
    /// that came anyway goes unrun, not run on the posting thread.
    fn post_idle_task(&self, _: *mut std::ffi::c_void, task: v8::IdleTask) {
        drop(task);
    }
}

/// `task` for isolate `key`, due after `delay` seconds (none: now), kept
/// for its thread, which is woken.
fn post(key: usize, task: v8::Task, delay: Option<f64>) {
    let due = delay
        .filter(|delay| *delay > 0.0)
        .and_then(|delay| Duration::try_from_secs_f64(delay).ok())
        .and_then(|delay| Instant::now().checked_add(delay));
    let waker = {
        let mut tasks = tasks();
        let isolate = tasks.entry(key).or_default();
        match due {
            Some(due) => isolate.delayed.push((due, task)),
            None => isolate.ready.push_back(task),
        }
        isolate.waker.clone()
    };
    // Not under the lock: the waker takes its thread's scheduler's.
    if let Some(waker) = waker {
        waker(due);
    }
}

/// Isolate `key`'s thread is woken by `waker` from now on: at once for
/// the tasks already ready, and for the first delayed one as it is due.
pub(crate) fn set_waker(key: usize, waker: PlatformTaskWaker) {
    let (ready, first_delayed) = {
        let mut tasks = tasks();
        let isolate = tasks.entry(key).or_default();
        isolate.waker = Some(waker.clone());
        (
            !isolate.ready.is_empty(),
            isolate.delayed.iter().map(|(due, _)| *due).min(),
        )
    };
    if ready {
        waker(None);
    }
    if let Some(due) = first_delayed {
        waker(Some(due));
    }
}

/// Isolate `key`'s tasks to run now: those ready, then the delayed ones
/// due, earliest first, taken out.
pub(crate) fn take_due(key: usize) -> Vec<v8::Task> {
    let mut tasks = tasks();
    let Some(isolate) = tasks.get_mut(&key) else {
        return Vec::new();
    };
    let mut due: Vec<v8::Task> = isolate.ready.drain(..).collect();
    let now = Instant::now();
    let mut delayed = std::mem::take(&mut isolate.delayed);
    delayed.sort_by_key(|(at, _)| *at);
    for (at, task) in delayed {
        if at <= now {
            due.push(task);
        } else {
            isolate.delayed.push((at, task));
        }
    }
    due
}

/// Isolate `key` has gone: its tasks go unrun (dropped, past the lock).
pub(crate) fn forget(key: usize) {
    let gone = tasks().remove(&key);
    drop(gone);
}
