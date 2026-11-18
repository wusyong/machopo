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
//! Its script the browser loads (Chromium's `DedicatedWorkerHost` fetches
//! it, `WorkerMainScriptLoader` hands it to the worker): the main thread
//! tells the browser of the worker as it starts it
//! (`DedicatedWorkerCreated`), and the answer goes straight to the worker's
//! thread (`MainThreadScheduler`'s routes), which waits for it before it
//! runs anything, what its `Worker` posts meanwhile kept for after.
//! `importScripts()` asks the browser itself, as a load of the worker's
//! (`FetchResource` of it), and waits for the answer (Blink's
//! `WorkerClassicScriptLoader::LoadSynchronously`), its other tasks kept
//! for after. A `fetch()` goes the same way, its answer a task of the
//! worker's (Blink's `FetchManager` on the worker's thread); a blob URL the
//! worker makes the browser hears of from its thread too, and lets go of
//! as the worker goes (its `BlobURLStore`'s).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvError, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use crossbeam_channel::Sender as BrowserSender;
use dom_api::{ClassicScript, EngineSettings, WorkerEngine, WorkerError, WorkerStart};
use protocol::network::{
    CredentialsMode, LoadResult, RequestDestination, RequestMode, ResourceRequest, ResponseTainting,
};
use protocol::renderer::{RendererHostMsg, RendererMsg};

use crate::task_queue::MainThreadScheduler;

/// What a worker sends its `Worker`, as a task of the main thread's.
pub(crate) enum WorkerEvent {
    Message(dom::SerializedMessage),
    /// What its script threw and its `onerror` didn't cancel.
    Error(WorkerError),
    /// It closed itself (`close()`): no more of it.
    Closed,
    /// Its script couldn't be had, or didn't parse (HTML's: an `error`
    /// event at its `Worker`): it ran nothing.
    ScriptFailed,
}

/// A task of a worker's thread.
pub(crate) enum WorkerTask {
    /// A message from its `Worker`.
    Message(dom::SerializedMessage),
    /// Terminated: its loop ends.
    Stop,
    /// What the browser answers it: its main script
    /// (`RendererMsg::DedicatedWorkerScript`), or a load's response or
    /// redirect. Boxed: much bigger than the others, and rare.
    Browser(Box<RendererMsg>),
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
    /// The browser's user agent string and whether automation controls it,
    /// for each worker's `navigator` (as the page's is told).
    agent: Agent,
    scheduler: Arc<MainThreadScheduler>,
    /// The browser's end: told of each worker as it starts and goes; what
    /// a worker's thread asks of it goes there too.
    to_browser: BrowserSender<RendererHostMsg>,
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
        agent: Agent,
        scheduler: Arc<MainThreadScheduler>,
        to_browser: BrowserSender<RendererHostMsg>,
    ) -> Self {
        Self {
            engine,
            settings,
            agent,
            scheduler,
            to_browser,
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
        let gone = self.running.borrow_mut().remove(&id);
        if gone.is_some() {
            self.gone(id);
        }
    }

    /// Stops worker `id`: its script at once, its loop as it next looks.
    pub(crate) fn terminate(&self, id: u64) {
        let gone = self.running.borrow_mut().remove(&id);
        if let Some(worker) = gone {
            worker.shared.stop();
            let _ = worker.tasks.send(WorkerTask::Stop);
            self.gone(id);
        }
    }

    /// What the browser sends worker `id` is dropped from now on, and the
    /// browser closes its loads and lets none more through.
    fn gone(&self, id: u64) {
        self.scheduler.remove_worker_route(id);
        let _ = self
            .to_browser
            .send(RendererHostMsg::DedicatedWorkerGone { worker: id });
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
        let request = script_request(&start);
        // Its time origin (Blink's `WorkerThread` takes it as it is made).
        let time_origin = std::time::Instant::now();
        let thread = std::thread::Builder::new()
            .name("DedicatedWorker thread".into())
            .spawn({
                let shared = shared.clone();
                let scheduler = self.scheduler.clone();
                let settings = self.settings;
                let agent = self.agent.clone();
                let to_browser = self.to_browser.clone();
                move || {
                    run(WorkerThread {
                        id,
                        start,
                        time_origin,
                        agent,
                        make: engine,
                        settings,
                        from_owner,
                        shared,
                        to_browser,
                        scheduler,
                    });
                }
            });
        match thread {
            Ok(thread) => {
                self.threads.borrow_mut().push(thread);
                // Its route there before the browser can answer.
                self.scheduler.add_worker_route(id, tasks.clone());
                self.running
                    .borrow_mut()
                    .insert(id, Running { tasks, shared });
                let _ = self
                    .to_browser
                    .send(RendererHostMsg::DedicatedWorkerCreated {
                        worker: id,
                        request,
                    });
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

/// A worker's tasks as its thread takes them: those it put off while it
/// waited for a load (`importScripts()`'s) first, in order.
struct Inbox {
    from_owner: Receiver<WorkerTask>,
    put_off: RefCell<VecDeque<WorkerTask>>,
}

impl Inbox {
    fn put_off(&self) -> Option<WorkerTask> {
        self.put_off.borrow_mut().pop_front()
    }

    fn recv(&self) -> Result<WorkerTask, RecvError> {
        match self.put_off() {
            Some(task) => Ok(task),
            None => self.from_owner.recv(),
        }
    }

    fn recv_timeout(&self, wait: std::time::Duration) -> Result<WorkerTask, RecvTimeoutError> {
        match self.put_off() {
            Some(task) => Ok(task),
            None => self.from_owner.recv_timeout(wait),
        }
    }
}

/// A worker's global's host, on its thread: what it sends goes to the main
/// thread for its `Worker`, what it loads to the browser.
struct GlobalHost {
    id: u64,
    scheduler: Arc<MainThreadScheduler>,
    to_browser: BrowserSender<RendererHostMsg>,
    /// Its tasks, which it waits on for a load.
    inbox: Rc<Inbox>,
    /// Its origin, serialized: its loads' initiator.
    origin: String,
    /// The last of its loads' request ids, from 1 (the browser knows them
    /// as the worker's).
    last_request: Cell<u64>,
    /// Its `fetch()`es on their way: each one's number by its request id.
    /// Shared with its loop, which hands their answers to its engine.
    fetches: Rc<RefCell<HashMap<u64, u64>>>,
    /// The blob URLs it registered and hasn't revoked.
    blob_urls: RefCell<HashSet<String>>,
    /// Set by `close()`: its loop ends after the task that closed it.
    closing: Rc<Cell<bool>>,
    shared: Arc<Shared>,
}

impl GlobalHost {
    /// Loads `request` and waits for it, following its redirects; what
    /// else comes meanwhile put off. None if the worker is stopped first
    /// (its load closed).
    fn load(&self, request: ResourceRequest) -> Option<LoadResult> {
        let request_id = self.next_request_id();
        let worker = Some(self.id);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            worker,
            request,
        });
        loop {
            match self.inbox.from_owner.recv() {
                Ok(WorkerTask::Browser(msg)) => match *msg {
                    RendererMsg::ResourceResponse {
                        request_id: answered,
                        result,
                        ..
                    } if answered == request_id => return Some(result),
                    RendererMsg::ReceivedRedirect {
                        request_id: redirected,
                        ..
                    } if redirected == request_id => {
                        let _ = self
                            .to_browser
                            .send(RendererHostMsg::FollowRedirect { request_id, worker });
                    }
                    // Another load's (a `fetch()`'s): for after.
                    msg => self
                        .inbox
                        .put_off
                        .borrow_mut()
                        .push_back(WorkerTask::Browser(Box::new(msg))),
                },
                Ok(task @ WorkerTask::Stop) => {
                    self.inbox.put_off.borrow_mut().push_back(task);
                    break;
                }
                Ok(task @ WorkerTask::Message(_)) => {
                    self.inbox.put_off.borrow_mut().push_back(task);
                }
                Err(_) => break,
            }
            if self.shared.stopping() {
                break;
            }
        }
        let _ = self
            .to_browser
            .send(RendererHostMsg::CancelResource { request_id, worker });
        None
    }

    fn next_request_id(&self) -> u64 {
        let request_id = self
            .last_request
            .get()
            .checked_add(1)
            .expect("fewer than 2^64 loads");
        self.last_request.set(request_id);
        request_id
    }
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

    /// `no-cors`, credentials same-origin, for a `script` (HTML's "fetch a
    /// classic worker-imported script"); taken as the main script is.
    fn fetch_script(&self, url: &url::Url) -> Option<dom_api::ImportedScript> {
        if self.shared.stopping() {
            return None;
        }
        let mut request = ResourceRequest::new(url.as_str(), RequestDestination::Script);
        request.mode = RequestMode::NoCors;
        request.credentials = CredentialsMode::SameOrigin;
        request.initiator = Some(self.origin.clone());
        let result = self.load(request)?;
        let script = worker_script(&result)?;
        Some(dom_api::ImportedScript {
            source: script.source,
            url: script.url.into(),
            muted: script.tainting == ResponseTainting::Opaque,
        })
    }

    fn start_fetch(&self, request: &dom::FetchRequest) {
        if self.shared.stopping() {
            return;
        }
        let request_id = self.next_request_id();
        self.fetches.borrow_mut().insert(request_id, request.id);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            worker: Some(self.id),
            request: crate::loader::fetch_resource_request(request),
        });
    }

    fn cancel_fetch(&self, id: u64) {
        let request_id = {
            let mut fetches = self.fetches.borrow_mut();
            let Some(request_id) = fetches
                .iter()
                .find(|&(_, &fetch)| fetch == id)
                .map(|(&request_id, _)| request_id)
            else {
                return;
            };
            fetches.remove(&request_id);
            request_id
        };
        let _ = self.to_browser.send(RendererHostMsg::CancelResource {
            request_id,
            worker: Some(self.id),
        });
    }

    /// Before any load of it, on the same channel.
    fn register_blob_url(&self, url: &str, data: &dom::BlobData) {
        self.blob_urls.borrow_mut().insert(url.to_owned());
        let _ = self.to_browser.send(RendererHostMsg::RegisterBlobUrl {
            worker: Some(self.id),
            url: url.to_owned(),
            mime_type: data.type_().to_owned(),
            bytes: data.shared_bytes(),
        });
    }

    /// At once: its loads asked for before it went already (a `fetch()` is
    /// sent as it is called, `importScripts()` waits for its script).
    fn revoke_blob_url(&self, url: &str) {
        if self.blob_urls.borrow_mut().remove(url) {
            let _ = self.to_browser.send(RendererHostMsg::RevokeBlobUrl {
                url: url.to_owned(),
            });
        }
    }
}

/// The browser's user agent string and whether automation controls it
/// (WebDriver's webdriver-active flag): what a worker's `navigator` says.
#[derive(Debug, Clone, Default)]
pub(crate) struct Agent {
    pub(crate) user_agent: String,
    pub(crate) webdriver: bool,
}

/// What a worker's thread starts with (Blink's `DedicatedWorkerThread`'s
/// startup data).
struct WorkerThread {
    id: u64,
    start: WorkerStart,
    time_origin: std::time::Instant,
    agent: Agent,
    make: fn(EngineSettings) -> Box<dyn WorkerEngine>,
    settings: EngineSettings,
    from_owner: Receiver<WorkerTask>,
    shared: Arc<Shared>,
    to_browser: BrowserSender<RendererHostMsg>,
    scheduler: Arc<MainThreadScheduler>,
}

/// A worker's thread: its engine made, its global, its script run, then
/// its loop (its tasks due, the messages from its `Worker`) till it is
/// stopped or closes itself; its engine let go of as it ends, on this
/// thread.
fn run(thread: WorkerThread) {
    let WorkerThread {
        id,
        start,
        time_origin,
        agent,
        make,
        settings,
        from_owner,
        shared,
        to_browser,
        scheduler,
    } = thread;
    let inbox = Rc::new(Inbox {
        from_owner,
        put_off: RefCell::new(VecDeque::new()),
    });
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
    // Its script; what its `Worker` posts meanwhile waits for it to have
    // run (HTML enables its port's queue then).
    let mut early = Vec::new();
    let result = loop {
        match inbox.recv() {
            Ok(WorkerTask::Browser(msg)) => {
                // It asked nothing else yet.
                if let RendererMsg::DedicatedWorkerScript { result, .. } = *msg {
                    break result;
                }
            }
            Ok(WorkerTask::Message(message)) => early.push(message),
            Ok(WorkerTask::Stop) | Err(_) => return,
        }
    };
    if shared.stopping() {
        return;
    }
    let Some(WorkerScript { source, url, .. }) = worker_script(&result) else {
        scheduler.post_worker(id, WorkerEvent::ScriptFailed);
        return;
    };
    // HTML's "set up a worker environment settings object": a `data:`
    // URL's worker is of an opaque origin; another its owner's.
    let origin = if url.scheme() == "data" {
        url::Origin::new_opaque()
    } else {
        start.owner_origin.clone().unwrap_or_else(|| url.origin())
    };
    let closing = Rc::new(Cell::new(false));
    let fetches = Rc::new(RefCell::new(HashMap::new()));
    let host = GlobalHost {
        id,
        scheduler: scheduler.clone(),
        to_browser: to_browser.clone(),
        inbox: inbox.clone(),
        origin: origin.ascii_serialization(),
        last_request: Cell::new(0),
        fetches: fetches.clone(),
        blob_urls: RefCell::new(HashSet::new()),
        closing: closing.clone(),
        shared: shared.clone(),
    };
    engine.create_global(
        dom_api::WorkerGlobalParams {
            url: url.clone(),
            origin,
            name: start.name.clone(),
            time_origin,
            user_agent: agent.user_agent,
            webdriver: agent.webdriver,
        },
        Box::new(host),
    );
    let parsed = engine.run_classic_script(&ClassicScript {
        source: &source,
        url: url.as_str(),
        base_url: None,
        line: 1,
        muted: false,
        use_credentials: false,
    });
    // A script that didn't parse: as one that couldn't be had (HTML's "run
    // a worker": a plain `error` at its `Worker`, and it runs no more).
    if !parsed {
        scheduler.post_worker(id, WorkerEvent::ScriptFailed);
        return;
    }
    for message in early {
        engine.post_message(message);
    }
    loop {
        if shared.stopping() || closing.get() {
            break;
        }
        engine.run_due_tasks();
        if shared.stopping() || closing.get() {
            break;
        }
        let task = match engine.until_next_task() {
            Some(wait) => match inbox.recv_timeout(wait) {
                Ok(task) => Some(task),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            },
            None => match inbox.recv() {
                Ok(task) => Some(task),
                Err(_) => break,
            },
        };
        match task {
            Some(WorkerTask::Message(message)) => engine.post_message(message),
            Some(WorkerTask::Stop) => break,
            Some(WorkerTask::Browser(msg)) => match *msg {
                // A `fetch()`'s: its promise settled.
                RendererMsg::ResourceResponse {
                    request_id, result, ..
                } => {
                    let fetch = fetches.borrow_mut().remove(&request_id);
                    if let Some(fetch) = fetch {
                        engine.fetch_response(fetch, crate::loader::fetched_response(&result));
                    }
                }
                RendererMsg::ReceivedRedirect { request_id, .. }
                    if fetches.borrow().contains_key(&request_id) =>
                {
                    let _ = to_browser.send(RendererHostMsg::FollowRedirect {
                        request_id,
                        worker: Some(id),
                    });
                }
                // An answer to a load it no longer waits for.
                _ => {}
            },
            None => {}
        }
    }
    if closing.get() && !shared.stopping() {
        scheduler.post_worker(id, WorkerEvent::Closed);
    }
}

/// The request for the main script of the worker `start` makes (HTML's
/// "fetch a classic worker script"): same-origin with its owner, of
/// credentials same-origin, for a `worker`.
fn script_request(start: &WorkerStart) -> ResourceRequest {
    let mut request = ResourceRequest::new(start.url.as_str(), RequestDestination::Worker);
    request.mode = RequestMode::SameOrigin;
    request.credentials = CredentialsMode::SameOrigin;
    request.initiator = Some(
        start
            .owner_origin
            .as_ref()
            .map_or_else(|| "null".to_owned(), url::Origin::ascii_serialization),
    );
    request
}

/// A worker's script, its main one or one `importScripts()` asked for.
struct WorkerScript {
    source: String,
    /// After redirects: its global's, for the main one.
    url: url::Url,
    tainting: ResponseTainting,
}

/// A worker's script as Chrome takes it (`WorkerClassicScriptLoader`):
/// none for a failed load (a network error, another origin's main
/// script), a status that isn't ok, or a MIME type no script is (lax, as
/// for a `<script>`: HTML asks for a JavaScript one), or that isn't
/// JavaScript's under `X-Content-Type-Options: nosniff`; not checked for a
/// `data:` or `blob:` URL's, whose text the worker could have had anyway.
/// Its text decoded as a `<script>`'s with UTF-8 to fall back on (a BOM,
/// the response's charset; HTML decodes it as UTF-8 whatever it says).
fn worker_script(result: &LoadResult) -> Option<WorkerScript> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    let url = url::Url::parse(&response.url).ok()?;
    let mime = response.mime_type.as_str();
    if !matches!(url.scheme(), "data" | "blob")
        && (crate::loader::is_blocked_as_script(mime)
            || (crate::loader::is_nosniff(&response.headers)
                && !dom_api::is_javascript_mime_type(mime)))
    {
        return None;
    }
    let source = crate::encoding::decode_script(
        response.charset.as_deref(),
        &response.body,
        encoding_rs::UTF_8,
    );
    Some(WorkerScript {
        source,
        url,
        tainting: response.tainting,
    })
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
            _: dom_api::WorkerGlobalParams,
            _: Box<dyn dom_api::WorkerGlobalHost>,
        ) {
        }

        fn run_classic_script(&mut self, _: &ClassicScript<'_>) -> bool {
            true
        }

        fn post_message(&mut self, _: dom::SerializedMessage) {}

        fn fetch_response(&mut self, _: u64, _: Result<dom_api::FetchedResponse, ()>) {}

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
            Agent::default(),
            Arc::new(MainThreadScheduler::default()),
            crossbeam_channel::unbounded().0,
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
