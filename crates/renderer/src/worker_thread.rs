//! Dedicated workers: each its own thread (Blink's `DedicatedWorkerThread`,
//! over a `WorkerBackingThread` of its own), its own engine (an isolate, a
//! DOM), and a small event loop of its own; and the page's side of them
//! (Blink's `DedicatedWorkerMessagingProxy`), on the main thread, which the
//! page's engine starts them through (`dom_api::WorkerHost`).
//!
//! What goes to a worker (a message, the order to stop) is a task of its
//! thread's, in a channel; what it sends its `Worker` (a message, an error,
//! that it closed, that its script couldn't be had) is a task of the main
//! thread's (`MainThreadScheduler::post_worker`), as Blink's worker posts a
//! cross-thread task to its parent's. `terminate()` stops its script at
//! once (V8's `TerminateExecution`, from the main thread) and its loop
//! ends; the thread lets go of its engine, DOM and isolate in that order,
//! on itself. A worker runs till it is terminated, closes itself, its
//! document goes or the renderer does (Chrome's `DedicatedWorker::
//! HasPendingActivity`): the renderer waits for each thread as it ends.
//!
//! Its script: a `data:` URL's, decoded here (of an opaque origin, HTML's);
//! any other can't be had yet, an `error` at its `Worker`.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use dom_api::{ClassicScript, EngineSettings, WorkerEngine, WorkerError, WorkerStart};

use crate::task_queue::MainThreadScheduler;

/// What a worker sends its `Worker`, as a task of the main thread's.
pub(crate) enum WorkerEvent {
    Message(dom::SerializedMessage),
    /// What its script threw and its `onerror` didn't cancel.
    Error(WorkerError),
    /// It closed itself (`close()`): no more of it.
    Closed,
    /// Its script couldn't be had (HTML's: an `error` event at its
    /// `Worker`): it ran nothing.
    ScriptFailed,
}

/// A task of a worker's thread.
enum WorkerTask {
    /// A message from its `Worker`.
    Message(dom::SerializedMessage),
    /// Terminated: its loop ends.
    Stop,
}

/// What a worker's thread and the main thread share: whether it is to stop,
/// and what stops its script (its engine's, once the thread made it).
#[derive(Default)]
struct Shared {
    stopping: AtomicBool,
    terminator: Mutex<Option<Box<dyn Fn() + Send>>>,
}

impl Shared {
    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::Acquire)
    }

    /// Stops it: its script (if it runs now), and its loop as it next
    /// looks.
    fn stop(&self) {
        self.stopping.store(true, Ordering::Release);
        let terminator = self
            .terminator
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(terminate) = terminator.as_ref() {
            terminate();
        }
    }
}

/// A worker's, on the main thread: what goes to it, and what it shares.
struct Running {
    tasks: Sender<WorkerTask>,
    shared: Arc<Shared>,
}

/// The renderer's dedicated workers, on its main thread: the page's engine
/// starts and stops them through it (`host`); the render thread hears from
/// them, and ends them with their documents and with itself.
pub(crate) struct Workers {
    /// What makes a worker's engine (on its thread); none: workers can't
    /// be made.
    engine: Option<fn(EngineSettings) -> Box<dyn WorkerEngine>>,
    settings: EngineSettings,
    scheduler: Arc<MainThreadScheduler>,
    /// The last worker's number, from 1.
    last_id: Cell<u64>,
    running: RefCell<HashMap<u64, Running>>,
    /// The workers' threads, joined as the renderer goes (each ends soon
    /// after it is stopped).
    threads: RefCell<Vec<JoinHandle<()>>>,
}

impl Workers {
    pub(crate) fn new(
        engine: Option<fn(EngineSettings) -> Box<dyn WorkerEngine>>,
        settings: EngineSettings,
        scheduler: Arc<MainThreadScheduler>,
    ) -> Self {
        Self {
            engine,
            settings,
            scheduler,
            last_id: Cell::new(0),
            running: RefCell::new(HashMap::new()),
            threads: RefCell::new(Vec::new()),
        }
    }

    /// The page's engine's host of them: an `Rc` of the same, as the render
    /// thread needs them too (to end them as their documents go and as it
    /// does).
    pub(crate) fn host(self: &Rc<Self>) -> Box<dyn dom_api::WorkerHost> {
        Box::new(WorkersHost(self.clone()))
    }

    /// Worker `id` is no more (terminated, closed, its script not had):
    /// what goes to it is dropped.
    pub(crate) fn forget(&self, id: u64) {
        self.running.borrow_mut().remove(&id);
    }

    /// Stops worker `id`: its script at once, its loop as it next looks.
    pub(crate) fn terminate(&self, id: u64) {
        if let Some(worker) = self.running.borrow_mut().remove(&id) {
            worker.shared.stop();
            let _ = worker.tasks.send(WorkerTask::Stop);
        }
    }

    /// Stops every worker (their engine goes with its documents).
    pub(crate) fn terminate_running(&self) {
        let ids: Vec<u64> = self.running.borrow().keys().copied().collect();
        for id in ids {
            self.terminate(id);
        }
    }

    /// Stops every worker, and waits for their threads to end: the
    /// renderer goes.
    pub(crate) fn terminate_all(&self) {
        self.terminate_running();
        for thread in self.threads.borrow_mut().drain(..) {
            let _ = thread.join();
        }
    }

    fn start(&self, start: WorkerStart) -> u64 {
        let id = self
            .last_id
            .get()
            .checked_add(1)
            .expect("fewer than 2^64 workers");
        self.last_id.set(id);
        let Some(engine) = self.engine else {
            // No engine for workers: as a script that couldn't be had.
            self.scheduler.post_worker(id, WorkerEvent::ScriptFailed);
            return id;
        };
        let (tasks, from_owner) = std::sync::mpsc::channel();
        let shared = Arc::new(Shared::default());
        let thread = std::thread::Builder::new()
            .name("DedicatedWorker thread".into())
            .spawn({
                let shared = shared.clone();
                let scheduler = self.scheduler.clone();
                let settings = self.settings;
                move || run(id, start, engine, settings, from_owner, &shared, &scheduler)
            });
        match thread {
            Ok(thread) => {
                self.threads.borrow_mut().push(thread);
                self.running
                    .borrow_mut()
                    .insert(id, Running { tasks, shared });
            }
            Err(error) => {
                tracing::warn!(%error, "no thread for a worker");
                self.scheduler.post_worker(id, WorkerEvent::ScriptFailed);
            }
        }
        id
    }

    fn post(&self, id: u64, message: dom::SerializedMessage) {
        if let Some(worker) = self.running.borrow().get(&id) {
            let _ = worker.tasks.send(WorkerTask::Message(message));
        }
    }
}

/// The page's engine's host of the workers (`Workers::host`).
struct WorkersHost(Rc<Workers>);

impl dom_api::WorkerHost for WorkersHost {
    fn start_worker(&self, start: WorkerStart) -> u64 {
        self.0.start(start)
    }

    fn post_to_worker(&self, id: u64, message: dom::SerializedMessage) {
        self.0.post(id, message);
    }

    fn terminate_worker(&self, id: u64) {
        self.0.terminate(id);
    }
}

/// A worker's global's host, on its thread: what it sends goes to the main
/// thread for its `Worker`.
struct GlobalHost {
    id: u64,
    scheduler: Arc<MainThreadScheduler>,
    /// Set by `close()`: its loop ends after the task that closed it.
    closing: Rc<Cell<bool>>,
}

impl dom_api::WorkerGlobalHost for GlobalHost {
    fn post_to_owner(&self, message: dom::SerializedMessage) {
        self.scheduler
            .post_worker(self.id, WorkerEvent::Message(message));
    }

    fn close(&self) {
        self.closing.set(true);
    }

    fn report_error(&self, error: WorkerError) {
        self.scheduler
            .post_worker(self.id, WorkerEvent::Error(error));
    }
}

/// A worker's thread: its engine made, its global, its script run, then
/// its loop (its tasks due, the messages from its `Worker`) till it is
/// stopped or closes itself; its engine let go of as it ends, on this
/// thread.
fn run(
    id: u64,
    start: WorkerStart,
    make: fn(EngineSettings) -> Box<dyn WorkerEngine>,
    settings: EngineSettings,
    from_owner: Receiver<WorkerTask>,
    shared: &Shared,
    scheduler: &Arc<MainThreadScheduler>,
) {
    let mut engine = make(settings);
    *shared
        .terminator
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(engine.terminator());
    // Terminated before its engine was there to stop.
    if shared.stopping() {
        return;
    }
    engine.set_console_sink(Box::new(|message| {
        let kind = if message.uncaught {
            "uncaught"
        } else {
            "console"
        };
        crate::render_thread::print_console(kind, &message.text, &message.url, message.line);
    }));
    let Some(source) = script_of(&start.url) else {
        scheduler.post_worker(id, WorkerEvent::ScriptFailed);
        return;
    };
    // HTML's "set up a worker environment settings object": a `data:`
    // URL's worker is of an opaque origin; another its owner's.
    let origin = if start.url.scheme() == "data" {
        url::Origin::new_opaque()
    } else {
        start
            .owner_origin
            .clone()
            .unwrap_or_else(|| start.url.origin())
    };
    let closing = Rc::new(Cell::new(false));
    engine.create_global(
        &start.url,
        origin,
        &start.name,
        Box::new(GlobalHost {
            id,
            scheduler: scheduler.clone(),
            closing: closing.clone(),
        }),
    );
    engine.run_classic_script(&ClassicScript {
        source: &source,
        url: start.url.as_str(),
        base_url: None,
        line: 1,
        muted: false,
        use_credentials: false,
    });
    loop {
        if shared.stopping() || closing.get() {
            break;
        }
        engine.run_due_tasks();
        if shared.stopping() || closing.get() {
            break;
        }
        let task = match engine.until_next_task() {
            Some(wait) => match from_owner.recv_timeout(wait) {
                Ok(task) => Some(task),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match from_owner.recv() {
                Ok(task) => Some(task),
                Err(_) => break,
            },
        };
        match task {
            Some(WorkerTask::Message(message)) => engine.post_message(message),
            Some(WorkerTask::Stop) => break,
            None => {}
        }
    }
    if closing.get() && !shared.stopping() {
        scheduler.post_worker(id, WorkerEvent::Closed);
    }
}

/// A worker's script, decoded as UTF-8 (HTML's classic worker script):
/// a `data:` URL's body; none for another URL (not fetched yet) or a
/// `data:` URL that doesn't parse.
fn script_of(url: &url::Url) -> Option<String> {
    if url.scheme() != "data" {
        return None;
    }
    let data = data_url::DataUrl::process(url.as_str()).ok()?;
    let (body, _) = data.decode_to_vec().ok()?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set as the slow engine's drop ends.
    static ENGINE_GONE: AtomicBool = AtomicBool::new(false);

    /// An engine that takes its time to go, and does nothing else.
    struct Slow;

    impl WorkerEngine for Slow {
        fn create_global(
            &mut self,
            _: &url::Url,
            _: url::Origin,
            _: &str,
            _: Box<dyn dom_api::WorkerGlobalHost>,
        ) {
        }

        fn run_classic_script(&mut self, _: &ClassicScript<'_>) {}

        fn post_message(&mut self, _: dom::SerializedMessage) {}

        fn until_next_task(&mut self) -> Option<std::time::Duration> {
            None
        }

        fn run_due_tasks(&mut self) -> bool {
            false
        }

        fn terminator(&mut self) -> Box<dyn Fn() + Send> {
            Box::new(|| {})
        }

        fn set_console_sink(&mut self, _: dom_api::ConsoleSink) {}

        fn take_harness_results(&mut self) -> Option<String> {
            None
        }
    }

    impl Drop for Slow {
        fn drop(&mut self) {
            std::thread::sleep(std::time::Duration::from_millis(100));
            ENGINE_GONE.store(true, Ordering::Release);
        }
    }

    fn slow(_: EngineSettings) -> Box<dyn WorkerEngine> {
        Box::new(Slow)
    }

    /// As the renderer goes, it stops its workers and waits for their
    /// threads: each has let go of its engine by then.
    #[test]
    fn the_renderer_waits_for_its_workers_threads() {
        let workers = Rc::new(Workers::new(
            Some(slow),
            EngineSettings { web_tests: false },
            Arc::new(MainThreadScheduler::default()),
        ));
        workers.host().start_worker(WorkerStart {
            url: url::Url::parse("data:text/javascript,").unwrap(),
            name: String::new(),
            owner_origin: None,
        });
        workers.terminate_all();
        assert!(ENGINE_GONE.load(Ordering::Acquire));
    }
}
