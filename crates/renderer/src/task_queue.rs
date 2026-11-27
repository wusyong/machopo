//! The renderer main thread's scheduler (Blink's `MainThreadSchedulerImpl`
//! over a `SequenceManager`, without priorities yet): everything the main
//! thread does arrives as one task at a time, whichever thread posted it,
//! in task queues by where it comes from. Each task is numbered as it is
//! posted, and the thread takes the earliest of the queues it runs: the
//! event loop all of them, in the order they were posted; script paused at
//! a breakpoint only DevTools' and the rendering's (Blink's nested loop
//! runs the same scheduler with the page's queues paused), the others
//! staying where they are, in order.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use base::AtomicSequenceNumber;
use base::sequence_manager::{SequenceManager, Spec};
use compositor::{CompositorReceiver, CompositorSender, ToMainThread};
use dom_api::{DevToolsInterrupter, DevToolsQueued};
use protocol::control_channel::{ChannelLink, send_over, to_renderer};
use protocol::renderer::RendererMsg;

/// One task for the renderer main thread.
pub(crate) enum MainThreadTask {
    /// A message from the browser (`content.mojom.Frame`,
    /// `blink.mojom.Widget`).
    Browser(RendererMsg),
    /// From the compositor thread: a main frame to run
    /// (`ProxyMain::BeginMainFrame`), or input it didn't handle.
    Compositor(ToMainThread),
    /// From the canvas encoder thread: the file of the DOM's `toBlob`
    /// call numbered `id` (none if it couldn't be encoded).
    CanvasEncoded { id: u64, file: Option<Vec<u8>> },
    /// The DOM's `fetch()` numbered `fetch` went nowhere (a revoked blob
    /// URL's): its network error, in a task of its own, as a response's.
    FetchFailed { fetch: u64 },
    /// What the browser asked of the page's DevTools sessions.
    DevTools(DevToolsQueued),
    /// From the thread of the dedicated worker numbered `id`: what it
    /// sends its `Worker` (`worker_thread`).
    Worker {
        id: u64,
        event: crate::worker_thread::WorkerEvent,
    },
    /// From another thread: the DOM's message ports may have messages
    /// (`dom::PortWaker`), which the DOM numbers as its tasks as this one
    /// ends, each a `DomTask` of its own.
    PortMessages,
    /// The DOM's task numbered `number` (`dom::TaskWaker`): queued as the
    /// DOM numbered it, or as it came due (a timer's).
    DomTask { number: u64 },
    /// The renderer's own work, posted by the main thread (`post_internal`,
    /// `post_internal_at`).
    Internal(InternalTask),
    /// From the network service's thread: the body of the document
    /// navigation `navigation_id` sent `frame` to commit has more in its
    /// pipe, or its pipe is over (the pipe's watcher, Blink's
    /// `NavigationBodyLoader::OnReadable`).
    NavigationBody {
        frame: protocol::FrameToken,
        navigation_id: u64,
    },
}

/// The renderer's own work that is a task of its own (Blink's internal
/// task types, kInternal* and kNetworking): what it does checks, as it
/// runs, whether it is still to be done (one cancelled or moved meanwhile
/// does nothing, as Blink's cancelled tasks are skipped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InternalTask {
    /// A child frame's document the renderer commits itself (a
    /// `javascript:` URL's, which the engine booked on the frame).
    ChildCommit(protocol::FrameToken),
    /// The parser's scripts that wait for a sheet that blocks scripts, tried
    /// again (Blink's `DidLoadAllScriptBlockingResources` posts it).
    RetryBlockedScripts,
    /// The caret blinks (Blink's `FrameCaret` timer).
    CaretBlink,
    /// The intersection observers' `delay` is up: a frame for them (Blink's
    /// `ScheduleDelayedIntersection`).
    Intersections,
    /// The browser may commit navigation numbered so now: the task that
    /// asked for it is over (Blink's `NotifyNavigationCancellationWindowEnded`).
    NavigationCancellationWindowEnded(u64),
    /// The main frame's new document's delay to be drawn is up
    /// (`RenderWidgetHostImpl`'s `TimeoutMonitor`).
    NewContentTimeout,
    /// The engine's platform tasks that are due (V8's foreground tasks:
    /// Blink's `kMainThreadTaskQueueV8`).
    PlatformTasks,
    /// A document's network may have been quiet long enough (Blink's
    /// `IdlenessDetector`'s timer).
    NetworkIdle,
    /// The engine's guests whose waits are over (a wasm guest's).
    Guests,
}

/// The scheduler's task queues (Blink's task queues by type), each in the
/// order its tasks were posted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TaskQueue {
    /// The browser's messages.
    Browser,
    /// What renders the page and runs no script: the compositor's main
    /// frames, and the browser's requests for a frame
    /// (`RequestVisualState`, which `Page.captureScreenshot` waits on).
    Rendering,
    /// The input the compositor passes on.
    Input,
    /// What the browser asked of DevTools' sessions.
    DevTools,
    /// The canvas encoder's files.
    Canvas,
    /// What dedicated workers send their `Worker`s (Blink posts them to
    /// the posted message task source's queue).
    Worker,
    /// The DOM's tasks (its timers, the UA's, idle callbacks' timeouts,
    /// element tasks, messages): Blink's frame task queues by task type,
    /// in one, of the same priority.
    Dom,
    /// The renderer's own work (`InternalTask`): Blink's internal and
    /// networking task queues, in one, of the same priority.
    Internal,
}

impl TaskQueue {
    const ALL: [Self; 8] = [
        Self::Browser,
        Self::Rendering,
        Self::Input,
        Self::DevTools,
        Self::Canvas,
        Self::Worker,
        Self::Dom,
        Self::Internal,
    ];

    fn index(self) -> usize {
        self as usize
    }
}

/// Which task queues a wait takes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TaskQueues(u8);

impl TaskQueues {
    /// The event loop's: every queue.
    pub(crate) const ALL: Self = Self(0b1111_1111);
    /// DevTools' alone.
    pub(crate) const DEVTOOLS: Self = Self(1 << TaskQueue::DevTools as u8);
    /// The rendering's alone.
    pub(crate) const RENDERING: Self = Self(1 << TaskQueue::Rendering as u8);
    /// The input's alone.
    pub(crate) const INPUT: Self = Self(1 << TaskQueue::Input as u8);
    /// Script paused's: DevTools' and the rendering's, which run no script.
    pub(crate) const PAUSED: Self =
        Self((1 << TaskQueue::DevTools as u8) | (1 << TaskQueue::Rendering as u8));

    fn has(self, queue: TaskQueue) -> bool {
        self.0 & (1 << queue as u8) != 0
    }
}

/// A task as queued, and (DevTools' messages) whether it is to interrupt
/// script.
struct Queued {
    task: MainThreadTask,
    interrupt: bool,
}

/// What the main thread keeps under its queues' lock.
#[derive(Default)]
struct MainExtra {
    /// The renderer's own tasks posted for the earliest of what keeps
    /// asking for them (`post_internal_once_at`), and when: until the
    /// first of each is taken.
    posted_once: Vec<(InternalTask, Instant)>,
    /// The browser said to shut down: a call waiting gives up, for the
    /// main thread to get to it.
    shutting_down: bool,
}

/// The main thread's queues as `base::sequence_manager` takes them.
struct MainSpec;

impl Spec for MainSpec {
    type Queue = TaskQueue;
    type Task = Queued;
    type Reply = RendererMsg;
    type Extra = MainExtra;

    const QUEUES: &'static [TaskQueue] = &TaskQueue::ALL;

    fn index(queue: TaskQueue) -> usize {
        queue.index()
    }

    /// The next of the same renderer's own task posted for the earliest
    /// is posted anew.
    fn taken(extra: &mut MainExtra, task: &Queued) {
        if let MainThreadTask::Internal(taken) = task.task {
            extra.posted_once.retain(|&(posted, _)| posted != taken);
        }
    }
}

/// The renderer main thread's task queues, which any thread posts to and
/// the main thread takes from (`base::sequence_manager`, as Blink's
/// `MainThreadSchedulerImpl` is over a `SequenceManager`).
#[derive(Default)]
pub struct MainThreadScheduler {
    state: SequenceManager<MainSpec>,
    /// The engine's, which has its thread take what interrupts script: set
    /// once, as the engine is made.
    interrupter: OnceLock<DevToolsInterrupter>,
    /// The dedicated workers' threads, by number, while they run: what the
    /// browser answers one (its script, its loads) goes there, not through
    /// the main thread (Chromium gives each worker its own pipes to the
    /// browser, bound on its thread). A lock of its own: a worker's thread
    /// is told while the main thread waits on `state`.
    worker_routes: Mutex<HashMap<u64, Arc<crate::worker_thread::WorkerScheduler>>>,
    /// The dedicated workers' numbers, from 1: the renderer's, as any
    /// thread may make one (a worker makes its own).
    worker_ids: WorkerIds,
    /// The numbers of the main thread's calls that wait for the browser
    /// (`sync_call`), from 1.
    call_ids: CallIds,
    /// A task for the engine's platform tasks ready waits
    /// (`platform_task_waker`): V8 posts many at once, from its threads.
    platform_tasks_posted: std::sync::atomic::AtomicBool,
}

/// The numbers of the calls that wait, from 1.
#[derive(Debug)]
struct CallIds(AtomicSequenceNumber<u64>);

impl Default for CallIds {
    fn default() -> Self {
        Self(AtomicSequenceNumber::<u64>::starting_at(1))
    }
}

/// The renderer's dedicated workers' numbers, from 1.
#[derive(Debug)]
struct WorkerIds(AtomicSequenceNumber<u64>);

impl Default for WorkerIds {
    fn default() -> Self {
        Self(AtomicSequenceNumber::<u64>::starting_at(1))
    }
}

impl MainThreadScheduler {
    /// Posts `task` to `queue`, after every task posted before (`interrupt`:
    /// a DevTools message that is to interrupt script). Fails once the main
    /// thread has gone.
    fn post(
        &self,
        queue: TaskQueue,
        task: MainThreadTask,
        interrupt: bool,
    ) -> Result<(), RendererGone> {
        self.state
            .post(queue, Queued { task, interrupt })
            .map_err(|_| RendererGone)?;
        if interrupt && let Some(interrupter) = self.interrupter.get() {
            interrupter();
        }
        Ok(())
    }

    /// The compositor thread's main frames and input.
    pub(crate) fn post_compositor(&self, task: ToMainThread) {
        let queue = match task {
            // A failed raster's message too: it waits for nothing a
            // frame doesn't.
            ToMainThread::BeginMainFrame { .. } | ToMainThread::RasterFailed { .. } => {
                TaskQueue::Rendering
            }
            ToMainThread::Input { .. } => TaskQueue::Input,
        };
        let _ = self.post(queue, MainThreadTask::Compositor(task), false);
    }

    /// What the dedicated worker numbered `id` sends its `Worker`, from its
    /// thread; dropped once the main thread has gone.
    pub(crate) fn post_worker(&self, id: u64, event: crate::worker_thread::WorkerEvent) {
        let _ = self.post(
            TaskQueue::Worker,
            MainThreadTask::Worker { id, event },
            false,
        );
    }

    /// What the pipe of the body of the document navigation
    /// `navigation_id` sent `frame` calls as it has more: a task in the
    /// browser's queue, after the commit that brought it (once the
    /// renderer is a process, the pipe's signal comes as the IPC
    /// channel's messages do); dropped once the main thread has gone.
    pub(crate) fn body_waker(
        self: &Arc<Self>,
        frame: protocol::FrameToken,
        navigation_id: u64,
    ) -> impl Fn() + Send + Sync + 'static {
        let scheduler = Arc::downgrade(self);
        move || {
            if let Some(scheduler) = scheduler.upgrade() {
                let _ = scheduler.post(
                    TaskQueue::Browser,
                    MainThreadTask::NavigationBody {
                        frame,
                        navigation_id,
                    },
                    false,
                );
            }
        }
    }

    /// What wakes the main thread for its ports' messages from workers'
    /// threads (a task in the queue of what workers send).
    pub(crate) fn port_waker(self: &Arc<Self>) -> dom::PortWaker {
        let scheduler = Arc::downgrade(self);
        Arc::new(move || {
            if let Some(scheduler) = scheduler.upgrade() {
                let _ = scheduler.post(TaskQueue::Worker, MainThreadTask::PortMessages, false);
            }
        })
    }

    /// What the main thread's DOM tells of its tasks (`dom::TaskWaker`):
    /// one due now queued in `TaskQueue::Dom`, a timer's held until it is
    /// due. On the main thread alone, which owns the DOM.
    pub(crate) fn dom_task_waker(self: &Arc<Self>) -> dom::TaskWaker {
        let scheduler = Arc::downgrade(self);
        Box::new(move |number, at| {
            let Some(scheduler) = scheduler.upgrade() else {
                return;
            };
            let task = MainThreadTask::DomTask { number };
            match at {
                Some(at) => scheduler.post_at(TaskQueue::Dom, task, at),
                None => {
                    let _ = scheduler.post(TaskQueue::Dom, task, false);
                }
            }
        })
    }

    /// `task` into `queue` as `at` comes (now, if it has).
    fn post_at(&self, queue: TaskQueue, task: MainThreadTask, at: Instant) {
        let task = Queued {
            task,
            interrupt: false,
        };
        let _ = self.state.post_at(queue, task, at);
    }

    /// What wakes the main thread for its engine's platform tasks, from any
    /// thread (`ScriptEngine::set_platform_task_waker`): one task while
    /// none waits for those ready, one as each delayed one is due.
    pub(crate) fn platform_task_waker(
        self: &Arc<Self>,
    ) -> std::sync::Arc<dyn Fn(Option<Instant>) + Send + Sync> {
        let scheduler = Arc::downgrade(self);
        std::sync::Arc::new(move |at| {
            let Some(scheduler) = scheduler.upgrade() else {
                return;
            };
            match at {
                Some(at) => scheduler.post_internal_at(InternalTask::PlatformTasks, at),
                None => {
                    if !scheduler
                        .platform_tasks_posted
                        .swap(true, std::sync::atomic::Ordering::AcqRel)
                    {
                        scheduler.post_internal(InternalTask::PlatformTasks);
                    }
                }
            }
        })
    }

    /// The main thread takes the platform tasks' task: those posted from
    /// now on post another.
    pub(crate) fn platform_tasks_taken(&self) {
        self.platform_tasks_posted
            .store(false, std::sync::atomic::Ordering::Release);
    }

    /// The renderer's own work, a task of its own, after every task posted
    /// before (Blink's `PostTask` to an internal task runner).
    pub(crate) fn post_internal(&self, task: InternalTask) {
        let _ = self.post(TaskQueue::Internal, MainThreadTask::Internal(task), false);
    }

    /// The renderer's own work, a task of its own as `at` comes (Blink's
    /// `PostDelayedTask`, a timer's).
    pub(crate) fn post_internal_at(&self, task: InternalTask, at: Instant) {
        self.post_at(TaskQueue::Internal, MainThreadTask::Internal(task), at);
    }

    /// The renderer's own work as `at` comes, unless the same is posted
    /// this way for then or sooner, or less than a millisecond later: one
    /// task for the earliest of a time asked for again and again (after
    /// each task, as each frame is made), which is converted to an
    /// `Instant` from another clock as it is asked for, and comes out a
    /// little different each time. One for a later time is posted all the
    /// same, and does nothing as it comes; once the one posted is taken,
    /// the next is posted anew.
    pub(crate) fn post_internal_once_at(&self, task: InternalTask, at: Instant) {
        let near = at.checked_add(Duration::from_millis(1)).unwrap_or(at);
        self.state.post_with(|extra| {
            if extra
                .posted_once
                .iter()
                .any(|&(posted, when)| posted == task && when <= near)
            {
                return None;
            }
            extra.posted_once.retain(|&(posted, _)| posted != task);
            extra.posted_once.push((task, at));
            let task = Queued {
                task: MainThreadTask::Internal(task),
                interrupt: false,
            };
            Some((TaskQueue::Internal, task, Some(at)))
        });
    }

    /// What wakes worker `id`'s thread for its ports' messages from other
    /// threads; nothing once it is no more.
    pub(crate) fn worker_port_waker(self: &Arc<Self>, id: u64) -> dom::PortWaker {
        let scheduler = Arc::downgrade(self);
        Arc::new(move || {
            if let Some(scheduler) = scheduler.upgrade()
                && let Some(tasks) = scheduler.lock_routes().get(&id)
            {
                crate::worker_thread::post_task(
                    tasks,
                    crate::worker_thread::WorkerTask::PortMessages,
                );
            }
        })
    }

    /// What the browser sends worker `id` goes to `tasks`, its thread's
    /// scheduler, from now on.
    pub(crate) fn add_worker_route(
        &self,
        id: u64,
        tasks: Arc<crate::worker_thread::WorkerScheduler>,
    ) {
        self.lock_routes().insert(id, tasks);
    }

    /// Worker `id` is no more: what the browser still sends it is dropped.
    pub(crate) fn remove_worker_route(&self, id: u64) {
        self.lock_routes().remove(&id);
    }

    fn lock_routes(
        &self,
    ) -> MutexGuard<'_, HashMap<u64, Arc<crate::worker_thread::WorkerScheduler>>> {
        self.worker_routes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// `msg` to worker `id`'s thread; dropped if it is no more.
    fn post_to_worker(&self, id: u64, msg: RendererMsg) {
        self.send_worker_task(id, crate::worker_thread::WorkerTask::Browser(Box::new(msg)));
    }

    /// `task` to worker `id`'s thread; dropped if it is no more.
    pub(crate) fn send_worker_task(&self, id: u64, task: crate::worker_thread::WorkerTask) {
        if let Some(tasks) = self.lock_routes().get(&id) {
            crate::worker_thread::post_task(tasks, task);
        }
    }

    /// A new dedicated worker's number, from 1, of whichever thread makes it.
    pub(crate) fn next_worker_id(&self) -> u64 {
        self.worker_ids.0.get_next()
    }

    /// The canvas encoder's file of the `toBlob` call `id`.
    pub(crate) fn post_canvas_encoded(&self, id: u64, file: Option<Vec<u8>>) {
        let _ = self.post(
            TaskQueue::Canvas,
            MainThreadTask::CanvasEncoded { id, file },
            false,
        );
    }

    /// The network error of the DOM's `fetch()` numbered `fetch`, which
    /// went nowhere: after the browser's messages posted before, as its
    /// response would have been.
    pub(crate) fn post_fetch_failed(&self, fetch: u64) {
        let _ = self.post(
            TaskQueue::Browser,
            MainThreadTask::FetchFailed { fetch },
            false,
        );
    }

    /// The network error of request `request_id`, which went nowhere (a
    /// revoked blob URL's script): after the browser's messages posted
    /// before, as the browser's answer would have been, with its error
    /// (`ERR_FILE_NOT_FOUND`, the browser's blob registry's).
    pub(crate) fn post_request_failed(&self, request_id: u64, url: &str) {
        let _ = self.post(
            TaskQueue::Browser,
            MainThreadTask::Browser(RendererMsg::ResourceResponse {
                request_id,
                worker: None,
                result: Err(protocol::network::NetError::new(
                    protocol::network::NetErrorCode::FileNotFound,
                    url,
                )),
            }),
            false,
        );
    }

    /// The earliest task of `queues`, waiting for one until `timeout` has
    /// passed (forever if none).
    pub(crate) fn next(
        &self,
        queues: TaskQueues,
        timeout: Option<Duration>,
    ) -> Option<MainThreadTask> {
        self.state
            .next(|queue| queues.has(queue), timeout)
            .map(|queued| queued.task)
    }

    /// The earliest task of `queues`, if there is one.
    pub(crate) fn try_next(&self, queues: TaskQueues) -> Option<MainThreadTask> {
        self.state
            .try_next(|queue| queues.has(queue))
            .map(|queued| queued.task)
    }

    /// Which of `queues` the earliest task is in, waiting for one: what
    /// script paused does next, left queued.
    pub(crate) fn wait_for(&self, queues: TaskQueues) -> TaskQueue {
        self.state.wait_for(|queue| queues.has(queue))
    }

    /// A call to the browser that waits for its answer (a mojo `[Sync]`
    /// call): `send` sends the message with the number given, and the
    /// main thread waits for the answer of that number, which the
    /// browser's sender puts aside for it (`post_reply`), every task left
    /// queued, in order. None if the renderer is shutting down or has
    /// gone first: nothing will answer.
    pub(crate) fn sync_call(&self, send: impl FnOnce(u64)) -> Option<RendererMsg> {
        let id = self.call_ids.0.get_next();
        send(id);
        self.state.sync_wait(id, |extra| extra.shutting_down)
    }

    /// The browser's answer to the call numbered `id` (`sync_call`), for
    /// that call alone.
    fn post_reply(&self, id: u64, reply: RendererMsg) -> Result<(), RendererGone> {
        self.state.post_reply(id, reply).map_err(|_| RendererGone)
    }

    /// The browser said to shut down: a call waiting gives up.
    fn shutting_down(&self) {
        self.state.with_extra(|extra| extra.shutting_down = true);
    }

    /// Whether a task waits in `queues`.
    pub(crate) fn has_task(&self, queues: TaskQueues) -> bool {
        self.state.has_task(|queue| queues.has(queue))
    }

    /// The engine's (`ScriptEngine::set_devtools_queue`): the first one given.
    pub(crate) fn set_interrupter(&self, interrupter: DevToolsInterrupter) {
        if self.interrupter.set(interrupter).is_err() {
            tracing::warn!("the scheduler has an interrupter already");
        }
    }

    /// The DevTools messages that are to interrupt script, taken out in
    /// order: those before any attach or detach (whose sessions the engine
    /// knows).
    pub(crate) fn take_interrupting(&self) -> Vec<(String, String)> {
        let mut known = true;
        self.state
            .take_matching(TaskQueue::DevTools, |queued| {
                // After an attach or detach, the sessions aren't known yet.
                known &= matches!(
                    queued.task,
                    MainThreadTask::DevTools(DevToolsQueued::Message { .. })
                );
                queued.interrupt && known
            })
            .into_iter()
            .filter_map(|queued| match queued.task {
                MainThreadTask::DevTools(DevToolsQueued::Message { session, message }) => {
                    Some((session, message))
                }
                _ => None,
            })
            .collect()
    }

    /// Puts `back` first, in order, before every task: what waited for a
    /// pause to end.
    pub(crate) fn put_back(&self, back: Vec<DevToolsQueued>) {
        let back = back
            .into_iter()
            .map(|queued| Queued {
                task: MainThreadTask::DevTools(queued),
                interrupt: false,
            })
            .collect();
        self.state.put_back(TaskQueue::DevTools, back);
    }

    /// The main thread has gone: what is posted from now on fails.
    pub(crate) fn close(&self) {
        self.state.close();
    }
}

/// A new renderer's queues: the browser's way to the main thread, Viz's and
/// the browser's way to the compositor thread, and the renderer's ends of
/// both.
pub fn channel() -> (RendererSender, CompositorSender, RendererReceiver) {
    let (to_compositor, compositor) = compositor::channel();
    let scheduler = Arc::new(MainThreadScheduler::default());
    let receiver = RendererReceiver {
        scheduler: scheduler.clone(),
        compositor,
    };
    (
        RendererSender(Way::Local(scheduler)),
        to_compositor,
        receiver,
    )
}

/// The browser's end of a renderer's pipe (a `mojo::Remote` to the
/// renderer's interfaces). For a renderer in the browser's process, each
/// message becomes a task on the main thread's scheduler, posted from the
/// thread that sends it. For one in a process of its own, the message is
/// written to its channel, and that process's IO thread, which reads it,
/// posts it with the sender it has of the first kind.
/// DevTools' go to their own queue, which script paused or busy takes from
/// too (Chromium's DevTools messages go through the IO thread to Blink's
/// `InspectorTaskRunner`).
#[derive(Clone)]
pub struct RendererSender(Way);

#[derive(Clone)]
enum Way {
    Local(Arc<MainThreadScheduler>),
    Channel(ChannelLink),
}

impl RendererSender {
    /// For the browser: the way to a renderer in a process of its own,
    /// whose channel `link` is.
    pub fn over(link: ChannelLink) -> Self {
        Self(Way::Channel(link))
    }

    /// Fails once the main thread has gone, or the way to it; the message
    /// is dropped.
    pub fn send(&self, msg: RendererMsg) -> Result<(), RendererGone> {
        let scheduler = match &self.0 {
            Way::Local(scheduler) => scheduler,
            Way::Channel(link) => {
                return send_over(link, to_renderer::MAIN, &msg).map_err(|_| RendererGone);
            }
        };
        // A dedicated worker's: to its thread.
        if let RendererMsg::ReceivedRedirect {
            worker: Some(worker),
            ..
        }
        | RendererMsg::ResourceResponse {
            worker: Some(worker),
            ..
        }
        | RendererMsg::DedicatedWorkerScript { worker, .. } = msg
        {
            scheduler.post_to_worker(worker, msg);
            return Ok(());
        }
        // An answer the main thread waits for, not a task.
        if let RendererMsg::StorageAreaOpened { request_id, .. }
        | RendererMsg::Cookies { request_id, .. }
        | RendererMsg::CookiesEnabled { request_id, .. }
        | RendererMsg::FontFamily { request_id, .. }
        | RendererMsg::FontFallback { request_id, .. }
        | RendererMsg::FontFiles { request_id, .. } = msg
        {
            return scheduler.post_reply(request_id, msg);
        }
        let (queued, interrupt) = match msg {
            RendererMsg::DevToolsAttach { session, state } => {
                (DevToolsQueued::Attach { session, state }, false)
            }
            RendererMsg::DevToolsDetach { session } => (DevToolsQueued::Detach(session), false),
            RendererMsg::DevToolsMessage {
                session,
                message,
                interrupt,
            } => (DevToolsQueued::Message { session, message }, interrupt),
            // A frame asked for runs while script is paused too.
            msg @ RendererMsg::RequestVisualState { .. } => {
                return scheduler.post(TaskQueue::Rendering, MainThreadTask::Browser(msg), false);
            }
            msg => {
                // Script paused is to go on, for the main thread to get
                // to the message.
                let shutdown = matches!(msg, RendererMsg::Shutdown | RendererMsg::Unload);
                scheduler.post(TaskQueue::Browser, MainThreadTask::Browser(msg), false)?;
                if shutdown {
                    scheduler.shutting_down();
                    scheduler.post(
                        TaskQueue::DevTools,
                        MainThreadTask::DevTools(DevToolsQueued::Quit),
                        false,
                    )?;
                }
                return Ok(());
            }
        };
        scheduler.post(
            TaskQueue::DevTools,
            MainThreadTask::DevTools(queued),
            interrupt,
        )
    }
}

/// The renderer main thread has gone, and its queue with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererGone;

/// The renderer's ends of its queues: the main thread's scheduler, and the
/// compositor thread's.
pub struct RendererReceiver {
    pub(crate) scheduler: Arc<MainThreadScheduler>,
    pub(crate) compositor: CompositorReceiver,
}

impl RendererReceiver {
    /// The browser's answer to the call numbered `request_id`, waited for
    /// as the main thread waits for it, for tests that stand in for a
    /// renderer.
    pub fn reply_for_testing(&self, request_id: u64) -> Option<RendererMsg> {
        self.scheduler
            .state
            .sync_wait(request_id, |extra| extra.shutting_down)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(session: &str, message: &str, interrupt: bool) -> RendererMsg {
        RendererMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_owned(),
            interrupt,
        }
    }

    fn queued(session: &str, message: &str) -> DevToolsQueued {
        DevToolsQueued::Message {
            session: session.to_owned(),
            message: message.to_owned(),
        }
    }

    /// What a task is, in short.
    fn kind(task: &MainThreadTask) -> String {
        match task {
            MainThreadTask::Browser(RendererMsg::RequestVisualState { request_id }) => {
                format!("visual {request_id}")
            }
            MainThreadTask::Browser(RendererMsg::Shutdown) => "shutdown".to_owned(),
            MainThreadTask::Browser(_) => "browser".to_owned(),
            MainThreadTask::Compositor(ToMainThread::BeginMainFrame { .. }) => "frame".to_owned(),
            MainThreadTask::Compositor(ToMainThread::Input { .. }) => "input".to_owned(),
            MainThreadTask::Compositor(ToMainThread::RasterFailed { .. }) => {
                "raster failed".to_owned()
            }
            MainThreadTask::CanvasEncoded { id, .. } => format!("canvas {id}"),
            MainThreadTask::FetchFailed { fetch } => format!("fetch failed {fetch}"),
            MainThreadTask::Worker { id, .. } => format!("worker {id}"),
            MainThreadTask::PortMessages => "ports".to_owned(),
            MainThreadTask::DomTask { number } => format!("dom {number}"),
            MainThreadTask::Internal(task) => format!("internal {task:?}"),
            MainThreadTask::NavigationBody { navigation_id, .. } => {
                format!("navigation body {navigation_id}")
            }
            MainThreadTask::DevTools(DevToolsQueued::Message { message, .. }) => {
                format!("devtools {message}")
            }
            MainThreadTask::DevTools(queued) => format!("{queued:?}"),
        }
    }

    fn frame() -> ToMainThread {
        ToMainThread::BeginMainFrame {
            args: protocol::viz::BeginFrameArgs {
                sequence: 1,
                frame_time: 0.0,
            },
            scroll: None,
        }
    }

    /// The event loop takes every queue's tasks in the order they were
    /// posted; script paused only DevTools' and the rendering's (a frame,
    /// a frame asked for), the others left in order for after.
    #[test]
    fn tasks_come_in_order_but_for_those_a_pause_leaves() {
        let (sender, _compositor, receiver) = channel();
        let scheduler = &receiver.scheduler;
        sender.send(RendererMsg::WasHidden).unwrap();
        scheduler.post_canvas_encoded(1, None);
        sender.send(message("a", "1", false)).unwrap();
        scheduler.post_compositor(frame());
        scheduler.post_compositor(ToMainThread::Input {
            event: protocol::input::InputEvent::MouseMove {
                x: 0.0,
                y: 0.0,
                buttons: 0,
                modifiers: protocol::input::Modifiers::default(),
            },
            attribution: compositor::InputAttribution::Unknown,
            document: None,
            scroll: None,
            dispatch: compositor::InputDispatch::NonBlocking,
        });
        sender
            .send(RendererMsg::RequestVisualState { request_id: 7 })
            .unwrap();
        sender.send(RendererMsg::SetFocus(true)).unwrap();
        sender.send(message("a", "2", false)).unwrap();

        let paused: Vec<String> = std::iter::from_fn(|| scheduler.try_next(TaskQueues::PAUSED))
            .map(|task| kind(&task))
            .collect();
        assert_eq!(paused, ["devtools 1", "frame", "visual 7", "devtools 2"]);
        sender.send(message("a", "3", false)).unwrap();
        let all: Vec<String> = std::iter::from_fn(|| scheduler.try_next(TaskQueues::ALL))
            .map(|task| kind(&task))
            .collect();
        assert_eq!(
            all,
            ["browser", "canvas 1", "input", "browser", "devtools 3"]
        );
        assert!(!scheduler.has_task(TaskQueues::ALL));
    }

    /// A wait for the paused queues says which has the earliest task and
    /// leaves it queued; one with a timeout gives up once it passes.
    #[test]
    fn waits_see_what_comes() {
        let (sender, _compositor, receiver) = channel();
        let scheduler = receiver.scheduler.clone();
        assert!(
            scheduler
                .next(TaskQueues::ALL, Some(Duration::from_millis(10)))
                .is_none()
        );
        sender.send(RendererMsg::WasHidden).unwrap();
        let waiter = std::thread::spawn({
            let scheduler = scheduler.clone();
            move || scheduler.wait_for(TaskQueues::PAUSED)
        });
        scheduler.post_compositor(frame());
        assert_eq!(waiter.join().unwrap(), TaskQueue::Rendering);
        assert!(matches!(
            scheduler.try_next(TaskQueues::PAUSED),
            Some(MainThreadTask::Compositor(
                ToMainThread::BeginMainFrame { .. }
            ))
        ));
        assert!(scheduler.next(TaskQueues::ALL, None).is_some());
    }

    /// What `f` returns on a thread of its own, which it does within 10
    /// seconds: a call that never gives up fails the test, not hangs it.
    fn within<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
        let (done, returned) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = done.send(f());
        });
        returned
            .recv_timeout(Duration::from_secs(10))
            .expect("the call returns")
    }

    fn opened(request_id: u64, area: u64) -> RendererMsg {
        RendererMsg::StorageAreaOpened {
            request_id,
            area: Some(protocol::renderer::OpenedStorageArea { area, items: None }),
        }
    }

    /// A call that waits takes its own answer alone: tasks posted before
    /// and after it stay queued, in order, and another call's answer
    /// stays for that call.
    #[test]
    fn a_call_that_waits_takes_its_answer_alone() {
        let (sender, _compositor, receiver) = channel();
        let scheduler = receiver.scheduler.clone();
        sender.send(RendererMsg::WasHidden).unwrap();
        // The next call's answer, there before this one's comes.
        sender.send(opened(2, 2)).unwrap();
        let reply = within({
            let scheduler = scheduler.clone();
            let sender = sender.clone();
            move || {
                scheduler.sync_call(|id| {
                    assert_eq!(id, 1);
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(50));
                        sender.send(RendererMsg::WasShown).unwrap();
                        sender.send(opened(id, 1)).unwrap();
                    });
                })
            }
        });
        assert!(matches!(
            reply,
            Some(RendererMsg::StorageAreaOpened {
                area: Some(protocol::renderer::OpenedStorageArea { area: 1, .. }),
                ..
            })
        ));
        let all: Vec<String> = std::iter::from_fn(|| scheduler.try_next(TaskQueues::ALL))
            .map(|task| kind(&task))
            .collect();
        assert_eq!(all, ["browser", "browser"]);
        // The other answer was kept for its own call.
        assert!(matches!(
            within(move || scheduler.sync_call(|_| {})),
            Some(RendererMsg::StorageAreaOpened {
                area: Some(protocol::renderer::OpenedStorageArea { area: 2, .. }),
                ..
            })
        ));
    }

    /// A call waiting gives up once the browser says to shut down, or the
    /// main thread has gone; an answer after that is no one's.
    #[test]
    fn a_call_gives_up_as_the_renderer_ends() {
        let (sender, _compositor, receiver) = channel();
        let scheduler = receiver.scheduler.clone();
        let reply = within({
            let scheduler = scheduler.clone();
            move || {
                scheduler.sync_call(|_| {
                    std::thread::spawn(move || sender.send(RendererMsg::Shutdown).unwrap());
                })
            }
        });
        assert!(reply.is_none());
        assert!(matches!(
            scheduler.try_next(TaskQueues::ALL),
            Some(MainThreadTask::Browser(RendererMsg::Shutdown))
        ));

        let (sender, _compositor, receiver) = channel();
        let scheduler = receiver.scheduler.clone();
        assert!(within(move || scheduler.sync_call(|_| scheduler.close())).is_none());
        assert!(sender.send(opened(1, 1)).is_err());
    }

    /// A message to interrupt script calls the engine's interrupter, and is
    /// taken out alone, in order, but for one after an attach or detach
    /// (its session not known yet); what the pause leaves goes back first,
    /// before every task.
    #[test]
    fn what_interrupts_script_is_taken_out_alone() {
        let (sender, _compositor, receiver) = channel();
        let (interrupted, interrupts) = crossbeam_channel::unbounded();
        receiver.scheduler.set_interrupter(Box::new(move || {
            let _ = interrupted.send(());
        }));
        sender.send(RendererMsg::WasHidden).unwrap();
        for msg in [
            message("a", "1", false),
            message("a", "2", true),
            message("b", "3", true),
            RendererMsg::DevToolsAttach {
                session: "c".to_owned(),
                state: None,
            },
            message("c", "4", true),
        ] {
            sender.send(msg).unwrap();
        }
        assert_eq!(interrupts.try_iter().count(), 3);
        let scheduler = &receiver.scheduler;
        assert_eq!(
            scheduler.take_interrupting(),
            [
                ("a".to_owned(), "2".to_owned()),
                ("b".to_owned(), "3".to_owned())
            ]
        );
        scheduler.put_back(vec![queued("x", "0"), queued("y", "0")]);
        let left: Vec<String> = std::iter::from_fn(|| scheduler.try_next(TaskQueues::ALL))
            .map(|task| kind(&task))
            .collect();
        assert_eq!(
            left,
            [
                "devtools 0",
                "devtools 0",
                "browser",
                "devtools 1",
                "Attach { session: \"c\", state: None }",
                "devtools 4"
            ]
        );
    }

    /// The renderer told to end lets script paused go on: DevTools' queue
    /// has it too, which a pause waiting on it wakes to; once the main
    /// thread has gone, sending fails.
    #[test]
    fn a_shutdown_ends_a_pause() {
        let (sender, _compositor, receiver) = channel();
        let scheduler = receiver.scheduler.clone();
        // Answered on a channel, not joined: a pause the shutdown doesn't
        // wake fails the test rather than hanging it.
        let (woke, woken) = std::sync::mpsc::channel();
        std::thread::spawn(move || woke.send(scheduler.wait_for(TaskQueues::PAUSED)));
        sender.send(RendererMsg::Shutdown).unwrap();
        assert_eq!(
            woken.recv_timeout(Duration::from_secs(10)),
            Ok(TaskQueue::DevTools),
            "the pause wasn't woken"
        );
        assert!(matches!(
            receiver.scheduler.try_next(TaskQueues::ALL),
            Some(MainThreadTask::Browser(RendererMsg::Shutdown))
        ));
        receiver.scheduler.close();
        assert_eq!(sender.send(RendererMsg::WasHidden), Err(RendererGone));
    }
    /// A renderer's own task asked for again and again is posted once for
    /// the earliest time: not again for the same time, a later one or one
    /// a little sooner (the clocks' converting), again for one sooner than
    /// that, and anew once it is taken.
    #[test]
    fn a_task_asked_for_again_is_posted_once() {
        let (_sender, _compositor, receiver) = channel();
        let scheduler = &receiver.scheduler;
        let now = Instant::now();
        let soon = now.checked_sub(Duration::from_millis(5)).unwrap_or(now);
        scheduler.post_internal_once_at(InternalTask::NetworkIdle, soon);
        scheduler.post_internal_once_at(InternalTask::NetworkIdle, soon);
        let later = soon.checked_add(Duration::from_millis(5)).unwrap();
        scheduler.post_internal_once_at(InternalTask::NetworkIdle, later);
        let near = soon.checked_sub(Duration::from_micros(500)).unwrap_or(soon);
        scheduler.post_internal_once_at(InternalTask::NetworkIdle, near);
        // Another task isn't the same.
        scheduler.post_internal_once_at(InternalTask::Guests, soon);
        let sooner = soon.checked_sub(Duration::from_millis(2)).unwrap_or(soon);
        scheduler.post_internal_once_at(InternalTask::NetworkIdle, sooner);
        let taken: Vec<String> = std::iter::from_fn(|| scheduler.try_next(TaskQueues::ALL))
            .map(|task| kind(&task))
            .collect();
        assert_eq!(
            taken,
            [
                "internal NetworkIdle",
                "internal Guests",
                "internal NetworkIdle"
            ]
        );
        // Taken: asked for again, it is posted again.
        scheduler.post_internal_once_at(InternalTask::NetworkIdle, sooner);
        assert_eq!(
            scheduler.try_next(TaskQueues::ALL).map(|task| kind(&task)),
            Some("internal NetworkIdle".to_owned())
        );
    }
}
