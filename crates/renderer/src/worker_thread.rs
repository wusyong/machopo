//! Dedicated workers: each its own thread (Blink's `DedicatedWorkerThread`,
//! over a `WorkerBackingThread` of its own), its own engine (an isolate, a
//! DOM), and a small event loop of its own; and the side of them of the
//! thread that made them (Blink's `DedicatedWorkerMessagingProxy`): the
//! main thread, whose page's engine starts them through
//! (`dom_api::WorkerHost`), or a worker's, whose engine starts its own (a
//! nested worker), each thread's `Workers`.
//!
//! What goes to a worker (a message, the order to stop, the browser's
//! answers, its DOM's tasks) is a task of its thread's scheduler's
//! (`WorkerScheduler`, Blink's `WorkerThreadScheduler`: the same
//! `base::sequence_manager` as the main thread's, a task at a time, the
//! earliest posted of its queues); what it sends its `Worker` (a message,
//! an error, that it closed, that its script couldn't be had) is a task of
//! the thread that made it (`Owner`: the main thread's scheduler's, or that
//! worker's), as Blink's worker posts a cross-thread task to its parent's.
//! `terminate()` stops its script at once (V8's `TerminateExecution`, from
//! the thread that made it) and its loop ends; the thread stops the
//! workers it made and waits for their threads, then lets go of its engine,
//! DOM and isolate in that order, on itself. A worker runs till it is
//! terminated, closes itself, the worker that made it ends, its document
//! goes or the renderer does (Chrome's `DedicatedWorker::
//! HasPendingActivity`): the renderer waits for the threads it made as it
//! ends, each of them for its own.
//!
//! Its script the browser loads (Chromium's `DedicatedWorkerHost` fetches
//! it, `WorkerMainScriptLoader` hands it to the worker): the thread that
//! made it tells the browser of the worker as it starts it
//! (`DedicatedWorkerCreated`), and the answer goes straight to the worker's
//! thread (`MainThreadScheduler`'s routes), which waits for it before it
//! runs anything, what its `Worker` posts meanwhile left queued.
//! `importScripts()` asks the browser itself, as a load of the worker's
//! (`FetchResource` of it), and waits for the answer (Blink's
//! `WorkerClassicScriptLoader::LoadSynchronously`), its other tasks left
//! queued. A `fetch()` goes the same way, its answer a task of the
//! worker's (Blink's `FetchManager` on the worker's thread); a blob URL the
//! worker makes the browser hears of from its thread too, and lets go of
//! as the worker goes (its `BlobURLStore`'s).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;

use base::sequence_manager::{SequenceManager, Spec};

use dom_api::{ClassicScript, EngineSettings, WorkerEngine, WorkerError, WorkerStart};
use protocol::control_channel::BrowserSender;
use protocol::network::{
    CredentialsMode, LoadResult, RequestDestination, RequestMode, ResourceRequest, ResponseTainting,
};
use protocol::renderer::{RendererHostMsg, RendererMsg};

use crate::task_queue::MainThreadScheduler;

/// What a worker sends its `Worker`, as a task of the thread that made it.
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
    /// From another thread: its DOM's message ports may have messages
    /// (`dom::PortWaker`), numbered as the task ends.
    PortMessages,
    /// From any thread: its engine has platform tasks ready (V8's
    /// foreground tasks).
    PlatformTasks,
    /// Its DOM's task numbered so (`dom::TaskWaker`): its token.
    DomTask { number: u64 },
    /// What the worker it made numbered `id` sends its `Worker`. Boxed, as
    /// `Browser`.
    Child { id: u64, event: Box<WorkerEvent> },
}

impl WorkerTask {
    /// The queue it goes to.
    fn queue(&self) -> WorkerQueue {
        match self {
            Self::Stop => WorkerQueue::Control,
            Self::Message(_) | Self::Child { .. } => WorkerQueue::Owner,
            Self::Browser(_) => WorkerQueue::Browser,
            Self::DomTask { .. } => WorkerQueue::Dom,
            Self::PortMessages | Self::PlatformTasks => WorkerQueue::Internal,
        }
    }
}

/// A worker's thread's task queues (Blink's `WorkerThreadScheduler`, its
/// task queues by type), each in the order posted, of the same priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkerQueue {
    /// Being terminated.
    Control,
    /// What its `Worker` posts it, and what the workers it made send
    /// theirs (Blink posts both to the posted message task source).
    Owner,
    /// The browser's answers: its main script, its loads'.
    Browser,
    /// Its DOM's tasks: timers, element tasks, messages.
    Dom,
    /// Its own work: its engine's platform tasks, its ports' messages from
    /// other threads to number.
    Internal,
}

/// A worker's queues as `base::sequence_manager` takes them.
pub(crate) struct WorkerSpec;

impl Spec for WorkerSpec {
    type Queue = WorkerQueue;
    type Task = WorkerTask;
    type Reply = ();
    type Extra = ();

    const QUEUES: &'static [WorkerQueue] = &[
        WorkerQueue::Control,
        WorkerQueue::Owner,
        WorkerQueue::Browser,
        WorkerQueue::Dom,
        WorkerQueue::Internal,
    ];

    fn index(queue: WorkerQueue) -> usize {
        queue as usize
    }
}

/// A worker's thread's scheduler: any thread posts, its thread takes.
pub(crate) type WorkerScheduler = SequenceManager<WorkerSpec>;

/// Posts `task` to the worker whose scheduler `tasks` is, in its queue;
/// dropped once its thread has gone.
pub(crate) fn post_task(tasks: &WorkerScheduler, task: WorkerTask) {
    let _ = tasks.post(task.queue(), task);
}

/// The thread that made a worker, which hears from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The main thread: a task of its scheduler's.
    Main,
    /// The worker numbered so: a task in its channel (through the
    /// scheduler's routes: none once it is no more).
    Worker(u64),
}

impl Owner {
    /// `event` of the worker `id` to this thread.
    fn post(self, scheduler: &MainThreadScheduler, id: u64, event: WorkerEvent) {
        match self {
            Self::Main => scheduler.post_worker(id, event),
            Self::Worker(owner) => scheduler.send_worker_task(
                owner,
                WorkerTask::Child {
                    id,
                    event: Box::new(event),
                },
            ),
        }
    }
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
    tasks: Arc<WorkerScheduler>,
    shared: Arc<Shared>,
}

/// The dedicated workers a thread made, on it: its engine starts and stops
/// them through it (`host`); the thread hears from them, and ends them as
/// their documents go (the main thread's) and as it does.
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
    to_browser: BrowserSender,
    /// The thread they are of, which hears from them.
    home: Owner,
    /// The loaders their loads go through: their creators'.
    loaders: WorkerLoaders,
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
        to_browser: BrowserSender,
        home: Owner,
        loaders: WorkerLoaders,
    ) -> Self {
        Self {
            engine,
            settings,
            agent,
            scheduler,
            to_browser,
            home,
            loaders,
            running: RefCell::new(HashMap::new()),
            threads: RefCell::new(Vec::new()),
        }
    }

    /// Their thread's engine's host of them: an `Rc` of the same, as the
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
            post_task(&worker.tasks, WorkerTask::Stop);
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
        // The renderer's: the browser and the routes know it by it.
        let id = self.scheduler.next_worker_id();
        let Some(engine) = self.engine else {
            // No engine for workers: as a script that couldn't be had.
            self.home
                .post(&self.scheduler, id, WorkerEvent::ScriptFailed);
            return id;
        };
        let tasks = Arc::new(WorkerScheduler::default());
        let shared = Arc::new(Shared::default());
        let request = script_request(&start);
        let loader = self.loaders.of(&start);
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
                let owner = self.home;
                let tasks = tasks.clone();
                move || {
                    run(WorkerThread {
                        id,
                        start,
                        time_origin,
                        agent,
                        make: engine,
                        settings,
                        tasks,
                        shared,
                        to_browser,
                        scheduler,
                        owner,
                        loader,
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
                        loader,
                        request,
                    });
            }
            Err(error) => {
                tracing::warn!(%error, "no thread for a worker");
                self.home
                    .post(&self.scheduler, id, WorkerEvent::ScriptFailed);
            }
        }
        id
    }

    fn post(&self, id: u64, message: dom::SerializedMessage) {
        if let Some(worker) = self.running.borrow().get(&id) {
            post_task(&worker.tasks, WorkerTask::Message(message));
        }
    }
}

/// The loaders of the workers a thread makes: the main thread's, their
/// documents' (the engine gives each, `WorkerStart::owner_loader`); a
/// worker's, its own.
pub(crate) enum WorkerLoaders {
    Documents,
    Worker(u64),
}

impl WorkerLoaders {
    /// The loader of a worker `start` makes: its owner document's, or the
    /// worker's that made it. None for a document the browser gave none:
    /// the browser refuses such a worker's loads.
    fn of(&self, start: &WorkerStart) -> u64 {
        match self {
            Self::Documents => start.owner_loader.unwrap_or(0),
            Self::Worker(loader) => *loader,
        }
    }
}

/// A thread's engine's host of the workers it makes (`Workers::host`).
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

    fn port_waker(&self) -> Option<dom::PortWaker> {
        Some(match self.0.home {
            Owner::Main => self.0.scheduler.port_waker(),
            Owner::Worker(id) => self.0.scheduler.worker_port_waker(id),
        })
    }
}

/// A worker's global's host, on its thread: what it sends goes to the
/// thread that made it for its `Worker`, what it loads to the browser.
struct GlobalHost {
    id: u64,
    owner: Owner,
    scheduler: Arc<MainThreadScheduler>,
    to_browser: BrowserSender,
    /// Its thread's scheduler, which it waits on for a load.
    tasks: Arc<WorkerScheduler>,
    /// Its origin, serialized: its loads' initiator.
    origin: String,
    /// The loader its loads go through, its creator's.
    loader: u64,
    /// The origin of the realm that made it, serialized: its module
    /// worker's graph's loads' (HTML's outside settings).
    owner_origin: String,
    /// The last of its loads' request ids, from 1 (the browser knows them
    /// as the worker's).
    last_request: Cell<u64>,
    /// Its `fetch()`es on their way: each one's number by its request id.
    /// Shared with its loop, which hands their answers to its engine.
    fetches: Rc<RefCell<HashMap<u64, u64>>>,
    /// Its modules on their way, by request id (the module map's numbers
    /// for them), shared with its loop as `fetches`.
    module_fetches: Rc<RefCell<HashSet<u64>>>,
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
            loader: self.loader,
            request,
        });
        // Only the browser's answers and the order to stop are looked at:
        // its other tasks stay queued, in order. Other loads' answers (a
        // `fetch()`'s) are taken out meanwhile and put back first, in order.
        let mut others = Vec::new();
        let answered = loop {
            if self.shared.stopping() {
                break None;
            }
            let wanted = |queue| matches!(queue, WorkerQueue::Control | WorkerQueue::Browser);
            match self.tasks.next(wanted, None) {
                Some(WorkerTask::Browser(msg)) => match *msg {
                    RendererMsg::ResourceResponse {
                        request_id: answered,
                        result,
                        ..
                    } if answered == request_id => break Some(result),
                    RendererMsg::ReceivedRedirect {
                        request_id: redirected,
                        ..
                    } if redirected == request_id => {
                        let _ = self
                            .to_browser
                            .send(RendererHostMsg::FollowRedirect { request_id, worker });
                    }
                    msg => others.push(WorkerTask::Browser(Box::new(msg))),
                },
                // Only to wake it: being stopped is `shared`'s, which its
                // loop looks at first.
                Some(WorkerTask::Stop) => break None,
                _ => {}
            }
        };
        self.tasks.put_back(WorkerQueue::Browser, others);
        if answered.is_some() {
            return answered;
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
        self.owner
            .post(&self.scheduler, self.id, WorkerEvent::Message(message));
    }

    fn close(&self) {
        self.closing.set(true);
    }

    fn report_error(&self, error: WorkerError) {
        self.owner
            .post(&self.scheduler, self.id, WorkerEvent::Error(error));
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
            loader: self.loader,
            request: crate::loader::fetch_resource_request(request),
        });
    }

    /// By CORS, for a `script` (HTML's "fetch a single module script":
    /// the destination of a module worker's descendants is `worker`, as
    /// Fetch's script-like ones go alike here), its owner's origin or its
    /// own the initiator.
    fn start_module_fetch(
        &self,
        url: &url::Url,
        credentials: dom::ScriptCors,
        outside: bool,
    ) -> u64 {
        let request_id = self.next_request_id();
        if self.shared.stopping() {
            return request_id;
        }
        let mut request = ResourceRequest::new(url.as_str(), RequestDestination::Script);
        request.mode = RequestMode::Cors;
        request.credentials = crate::loader::script_credentials(credentials);
        request.initiator = Some(if outside {
            self.owner_origin.clone()
        } else {
            self.origin.clone()
        });
        self.module_fetches.borrow_mut().insert(request_id);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            worker: Some(self.id),
            loader: self.loader,
            request,
        });
        request_id
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
            bytes: data.shared_bytes().into(),
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

    fn port_waker(&self) -> Option<dom::PortWaker> {
        Some(self.scheduler.worker_port_waker(self.id))
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
    /// Its thread's scheduler.
    tasks: Arc<WorkerScheduler>,
    shared: Arc<Shared>,
    to_browser: BrowserSender,
    scheduler: Arc<MainThreadScheduler>,
    /// The thread that made it.
    owner: Owner,
    /// The loader its loads go through, its creator's.
    loader: u64,
}

/// A worker's thread: its engine made, its global, its script run, then
/// its loop (its tasks due, the messages from its `Worker`, what the
/// workers it made send) till it is stopped or closes itself; the workers
/// it made stopped and their threads waited for, and its engine let go of,
/// as it ends, on this thread.
fn run(thread: WorkerThread) {
    let WorkerThread {
        id,
        start,
        time_origin,
        agent,
        make,
        settings,
        tasks,
        shared,
        to_browser,
        scheduler,
        owner,
        loader,
    } = thread;
    let mut engine = make(settings);
    // Its engine's platform tasks wake it as V8 posts them, from any
    // thread: one task while none waits for those ready, one as each
    // delayed one is due (as the main thread's).
    let platform_posted = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let tasks = Arc::downgrade(&tasks);
        let posted = platform_posted.clone();
        engine.set_platform_task_waker(Arc::new(move |at| {
            let Some(tasks) = tasks.upgrade() else {
                return;
            };
            match at {
                Some(at) => {
                    let _ = tasks.post_at(WorkerQueue::Internal, WorkerTask::PlatformTasks, at);
                }
                None => {
                    if !posted.swap(true, Ordering::AcqRel) {
                        post_task(&tasks, WorkerTask::PlatformTasks);
                    }
                }
            }
        }));
    }
    // Its DOM's tasks, each a task of its scheduler's in its turn with its
    // own (Blink's worker task runners).
    {
        let tasks = Arc::downgrade(&tasks);
        engine.set_task_waker(Box::new(move |number, at| {
            let Some(tasks) = tasks.upgrade() else {
                return;
            };
            let task = WorkerTask::DomTask { number };
            match at {
                Some(at) => {
                    let _ = tasks.post_at(WorkerQueue::Dom, task, at);
                }
                None => post_task(&tasks, task),
            }
        }));
    }
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
    // Its script; what its `Worker` posts meanwhile stays queued till it
    // has run (HTML enables its port's queue then).
    let script = |queue| matches!(queue, WorkerQueue::Control | WorkerQueue::Browser);
    let result = loop {
        match tasks.next(script, None) {
            Some(WorkerTask::Browser(msg)) => {
                // It asked nothing else yet.
                if let RendererMsg::DedicatedWorkerScript { result, .. } = *msg {
                    break result;
                }
            }
            Some(WorkerTask::Stop) | None => return,
            Some(_) => {}
        }
    };
    if shared.stopping() {
        return;
    }
    // A classic worker's script, or a module worker's graph's root (as a
    // module script is taken: a JavaScript MIME type, whatever its URL).
    let main = match start.module {
        None => worker_script(&result).map(MainScript::Classic),
        Some(credentials) => {
            crate::loader::module_loaded(&result).map(|root| MainScript::Module(root, credentials))
        }
    };
    let Some(main) = main else {
        owner.post(&scheduler, id, WorkerEvent::ScriptFailed);
        return;
    };
    let url = match &main {
        MainScript::Classic(script) => script.url.clone(),
        MainScript::Module(root, _) => root.url.clone(),
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
    let module_fetches = Rc::new(RefCell::new(HashSet::new()));
    let host = GlobalHost {
        id,
        owner,
        scheduler: scheduler.clone(),
        to_browser: to_browser.clone(),
        tasks: tasks.clone(),
        origin: origin.ascii_serialization(),
        loader,
        owner_origin: start
            .owner_origin
            .as_ref()
            .map_or_else(|| "null".to_owned(), url::Origin::ascii_serialization),
        last_request: Cell::new(0),
        fetches: fetches.clone(),
        module_fetches: module_fetches.clone(),
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
            user_agent: agent.user_agent.clone(),
            webdriver: agent.webdriver,
            module: start.module.is_some(),
        },
        Box::new(host),
    );
    // The workers it makes, its thread hearing from them.
    let workers = Rc::new(Workers::new(
        Some(make),
        settings,
        agent,
        scheduler.clone(),
        to_browser.clone(),
        Owner::Worker(id),
        WorkerLoaders::Worker(loader),
    ));
    engine.set_worker_host(workers.host());
    // Run (`Some(true)`), not runnable (`Some(false)`: HTML's "run a worker"
    // fires a plain `error` at its `Worker`, and it runs no more), or a
    // module graph whose modules are yet to come (none).
    let ran = match main {
        MainScript::Classic(script) => Some(engine.run_classic_script(&ClassicScript {
            source: &script.source,
            url: script.url.as_str(),
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        })),
        MainScript::Module(root, credentials) => {
            engine.run_module_graph(&start.url, root, credentials)
        }
    };
    if ran == Some(false) {
        owner.post(&scheduler, id, WorkerEvent::ScriptFailed);
        return;
    }
    // What its `Worker` posts waits in its queue till its script has run:
    // a module worker's graph may be yet to come.
    let mut ran = ran == Some(true);
    let mut failed = false;
    loop {
        if shared.stopping() {
            break;
        }
        // Closed by its script or the task before (Blink's `WorkerThread::
        // DidProcessTask`): what is queued goes, and it runs no more (HTML's
        // "close a worker": the tasks queued are discarded).
        if closing.get() {
            tasks.close();
            break;
        }
        let wanted = |queue| ran || queue != WorkerQueue::Owner;
        // A task of the DOM's due a while ago with nothing in the scheduler
        // for it never runs (its token was lost), and nothing else says
        // so: debug builds stop there, as the main thread does. Due 5 ms
        // ago, past the clocks' rounding.
        #[cfg(debug_assertions)]
        assert!(
            tasks.has_task(|_| true) || !engine.has_dom_task_due_for(5.0),
            "a task of the worker's DOM is due with nothing in its scheduler to run it"
        );
        let Some(task) = tasks.next(wanted, None) else {
            continue;
        };
        match task {
            WorkerTask::Message(message) => engine.post_message(message),
            WorkerTask::Stop => break,
            WorkerTask::DomTask { number } => {
                engine.run_dom_task(number);
            }
            // Each a task of the DOM's, numbered now (a token each).
            WorkerTask::PortMessages => engine.port_messages_arrived(),
            WorkerTask::PlatformTasks => {
                platform_posted.store(false, Ordering::Release);
                engine.run_platform_tasks();
            }
            // What a worker it made sends its `Worker`, as the main thread
            // takes it (`RenderThread::worker_event`).
            WorkerTask::Child { id: child, event } => match *event {
                WorkerEvent::Message(message) => engine.worker_message(child, message),
                WorkerEvent::Error(error) => engine.worker_error(child, &error),
                WorkerEvent::Closed => {
                    engine.worker_closed(child);
                    workers.forget(child);
                }
                WorkerEvent::ScriptFailed => {
                    engine.worker_script_failed(child);
                    workers.forget(child);
                }
            },
            WorkerTask::Browser(msg) => match *msg {
                // A `fetch()`'s: its promise settled.
                RendererMsg::ResourceResponse {
                    request_id, result, ..
                } => {
                    let fetch = fetches.borrow_mut().remove(&request_id);
                    if let Some(fetch) = fetch {
                        engine.fetch_response(fetch, crate::loader::fetched_response(&result));
                    } else if module_fetches.borrow_mut().remove(&request_id) {
                        let module = crate::loader::module_loaded(&result);
                        // Its graph, if it was the last of it: its
                        // `Worker`'s messages may come now.
                        match engine.module_fetched(request_id, module) {
                            Some(true) => ran = true,
                            Some(false) => {
                                failed = true;
                                break;
                            }
                            None => {}
                        }
                    }
                }
                RendererMsg::ReceivedRedirect { request_id, .. }
                    if fetches.borrow().contains_key(&request_id)
                        || module_fetches.borrow().contains(&request_id) =>
                {
                    let _ = to_browser.send(RendererHostMsg::FollowRedirect {
                        request_id,
                        worker: Some(id),
                    });
                }
                // An answer to a load it no longer waits for.
                _ => {}
            },
        }
    }
    // The workers it made end with it (HTML: no owner left; Blink's
    // `DedicatedWorker::Dispose` terminates each), before its engine goes.
    workers.terminate_all();
    if failed {
        owner.post(&scheduler, id, WorkerEvent::ScriptFailed);
    } else if closing.get() && !shared.stopping() {
        owner.post(&scheduler, id, WorkerEvent::Closed);
    }
}

/// A worker's main script, as it came: a classic script, or a module
/// worker's graph's root and the credentials mode of its descendants.
enum MainScript {
    Classic(WorkerScript),
    Module(dom_api::module::FetchedModule, dom::ScriptCors),
}

/// The request for the main script of the worker `start` makes, for a
/// `worker`, same-origin with its owner: a classic one's of credentials
/// same-origin (HTML's "fetch a classic worker script"), a module
/// worker's of its `credentials` ("fetch a module worker script graph"'s
/// root).
fn script_request(start: &WorkerStart) -> ResourceRequest {
    let mut request = ResourceRequest::new(start.url.as_str(), RequestDestination::Worker);
    match start.module {
        None => {
            request.mode = RequestMode::SameOrigin;
            request.credentials = CredentialsMode::SameOrigin;
        }
        // HTML's "fetch a module worker script graph": its root
        // same-origin too ("fetch a single module script" of a worker's
        // top level), with its `credentials`.
        Some(credentials) => {
            request.mode = RequestMode::SameOrigin;
            request.credentials = crate::loader::script_credentials(credentials);
        }
    }
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
    if !(200..300).contains(&response.head.status) {
        return None;
    }
    let url = url::Url::parse(&response.head.url).ok()?;
    let mime = response.head.mime_type.as_str();
    if !matches!(url.scheme(), "data" | "blob")
        && (crate::loader::is_blocked_as_script(mime)
            || (crate::loader::is_nosniff(&response.head.headers)
                && !dom_api::is_javascript_mime_type(mime)))
    {
        return None;
    }
    let source = crate::encoding::decode_script(
        response.head.charset.as_deref(),
        &response.body,
        encoding_rs::UTF_8,
    );
    Some(WorkerScript {
        source,
        url,
        tainting: response.head.tainting,
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

        fn set_task_waker(&mut self, _: dom::TaskWaker) {}

        fn run_dom_task(&mut self, _: u64) -> bool {
            false
        }

        fn port_messages_arrived(&mut self) {}

        fn has_dom_task_due_for(&mut self, _: f64) -> bool {
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
            crossbeam_channel::unbounded().0.into(),
            Owner::Main,
            WorkerLoaders::Worker(1),
        ));
        workers.host().start_worker(WorkerStart {
            url: url::Url::parse("data:text/javascript,").unwrap(),
            name: String::new(),
            owner_origin: None,
            owner_document: None,
            owner_loader: None,
            module: None,
        });
        workers.terminate_all();
        assert!(ENGINE_GONE.load(Ordering::Acquire));
    }
}
