//! The renderer main thread's scheduler (Blink's `MainThreadSchedulerImpl`
//! over a `SequenceManager`, without priorities yet): everything the main
//! thread does arrives as one task at a time, whichever thread posted it,
//! in task queues by where it comes from. Each task is numbered as it is
//! posted, and the thread takes the earliest of the queues it runs: the
//! event loop all of them, in the order they were posted; script paused at
//! a breakpoint only DevTools' and the rendering's (Blink's nested loop
//! runs the same scheduler with the page's queues paused), the others
//! staying where they are, in order.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use compositor::{CompositorReceiver, CompositorSender, ToMainThread};
use dom_api::{DevToolsInterrupter, DevToolsQueued};
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
    /// What the browser asked of the page's DevTools sessions.
    DevTools(DevToolsQueued),
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
}

impl TaskQueue {
    const ALL: [Self; 5] = [
        Self::Browser,
        Self::Rendering,
        Self::Input,
        Self::DevTools,
        Self::Canvas,
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
    pub(crate) const ALL: Self = Self(0b1_1111);
    /// DevTools' alone.
    pub(crate) const DEVTOOLS: Self = Self(1 << TaskQueue::DevTools as u8);
    /// The rendering's alone.
    pub(crate) const RENDERING: Self = Self(1 << TaskQueue::Rendering as u8);
    /// Script paused's: DevTools' and the rendering's, which run no script.
    pub(crate) const PAUSED: Self =
        Self((1 << TaskQueue::DevTools as u8) | (1 << TaskQueue::Rendering as u8));

    fn has(self, queue: TaskQueue) -> bool {
        self.0 & (1 << queue as u8) != 0
    }
}

/// A task as queued: its number, and (DevTools' messages) whether it is to
/// interrupt script.
struct Queued {
    sequence: u64,
    task: MainThreadTask,
    interrupt: bool,
}

#[derive(Default)]
struct State {
    queues: [VecDeque<Queued>; 5],
    /// The next task's number, from 1: 0 is what was put back first
    /// (`put_back`).
    next: u64,
    /// The main thread has gone: nothing is posted any more.
    closed: bool,
}

impl State {
    /// The queue among `queues` whose first task was posted earliest.
    fn earliest(&self, queues: TaskQueues) -> Option<TaskQueue> {
        TaskQueue::ALL
            .into_iter()
            .filter(|&queue| queues.has(queue))
            .filter_map(|queue| Some((self.queues[queue.index()].front()?.sequence, queue)))
            .min_by_key(|&(sequence, _)| sequence)
            .map(|(_, queue)| queue)
    }
}

/// The renderer main thread's task queues, which any thread posts to and
/// the main thread takes from: one lock and one condition for them all
/// (Blink's `SequenceManager` has a lock for what other threads post).
#[derive(Default)]
pub struct MainThreadScheduler {
    state: Mutex<State>,
    ready: Condvar,
    /// The engine's, which has its thread take what interrupts script: set
    /// once, as the engine is made.
    interrupter: OnceLock<DevToolsInterrupter>,
}

impl MainThreadScheduler {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Posts `task` to `queue`, after every task posted before (`interrupt`:
    /// a DevTools message that is to interrupt script). Fails once the main
    /// thread has gone.
    fn post(
        &self,
        queue: TaskQueue,
        task: MainThreadTask,
        interrupt: bool,
    ) -> Result<(), RendererGone> {
        {
            let mut state = self.lock();
            if state.closed {
                return Err(RendererGone);
            }
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "one per task the main thread is given: a u64 lasts centuries"
            )]
            {
                state.next += 1;
            }
            let sequence = state.next;
            state.queues[queue.index()].push_back(Queued {
                sequence,
                task,
                interrupt,
            });
        }
        self.ready.notify_one();
        if interrupt && let Some(interrupter) = self.interrupter.get() {
            interrupter();
        }
        Ok(())
    }

    /// The compositor thread's main frames and input.
    pub(crate) fn post_compositor(&self, task: ToMainThread) {
        let queue = match task {
            ToMainThread::BeginMainFrame { .. } => TaskQueue::Rendering,
            ToMainThread::Input { .. } => TaskQueue::Input,
        };
        let _ = self.post(queue, MainThreadTask::Compositor(task), false);
    }

    /// The canvas encoder's file of the `toBlob` call `id`.
    pub(crate) fn post_canvas_encoded(&self, id: u64, file: Option<Vec<u8>>) {
        let _ = self.post(
            TaskQueue::Canvas,
            MainThreadTask::CanvasEncoded { id, file },
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
        let deadline = timeout.and_then(|timeout| Instant::now().checked_add(timeout));
        let mut state = self.lock();
        loop {
            if let Some(queue) = state.earliest(queues) {
                return state.queues[queue.index()]
                    .pop_front()
                    .map(|queued| queued.task);
            }
            state = match (timeout, deadline) {
                (None, _) => self
                    .ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
                (Some(_), Some(deadline)) => {
                    let left = deadline.saturating_duration_since(Instant::now());
                    if left.is_zero() {
                        return None;
                    }
                    self.ready
                        .wait_timeout(state, left)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0
                }
                // A wait too long to count: forever.
                (Some(_), None) => self
                    .ready
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner),
            };
        }
    }

    /// The earliest task of `queues`, if there is one.
    pub(crate) fn try_next(&self, queues: TaskQueues) -> Option<MainThreadTask> {
        let mut state = self.lock();
        let queue = state.earliest(queues)?;
        state.queues[queue.index()]
            .pop_front()
            .map(|queued| queued.task)
    }

    /// Which of `queues` the earliest task is in, waiting for one: what
    /// script paused does next, left queued.
    pub(crate) fn wait_for(&self, queues: TaskQueues) -> TaskQueue {
        let mut state = self.lock();
        loop {
            if let Some(queue) = state.earliest(queues) {
                return queue;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Whether a task waits in `queues`.
    pub(crate) fn has_task(&self, queues: TaskQueues) -> bool {
        self.lock().earliest(queues).is_some()
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
        let mut state = self.lock();
        let devtools = &mut state.queues[TaskQueue::DevTools.index()];
        let mut taken = Vec::new();
        let mut kept = VecDeque::new();
        let mut known = true;
        for queued in devtools.drain(..) {
            // After an attach or detach, the sessions aren't known yet.
            known &= matches!(
                queued.task,
                MainThreadTask::DevTools(DevToolsQueued::Message { .. })
            );
            match queued.task {
                MainThreadTask::DevTools(DevToolsQueued::Message { session, message })
                    if queued.interrupt && known =>
                {
                    taken.push((session, message));
                }
                task => kept.push_back(Queued { task, ..queued }),
            }
        }
        *devtools = kept;
        taken
    }

    /// Puts `back` first, in order, before every task: what waited for a
    /// pause to end.
    pub(crate) fn put_back(&self, back: Vec<DevToolsQueued>) {
        if back.is_empty() {
            return;
        }
        {
            let mut state = self.lock();
            let devtools = &mut state.queues[TaskQueue::DevTools.index()];
            for queued in back.into_iter().rev() {
                devtools.push_front(Queued {
                    sequence: 0,
                    task: MainThreadTask::DevTools(queued),
                    interrupt: false,
                });
            }
        }
        self.ready.notify_one();
    }

    /// The main thread has gone: what is posted from now on fails.
    pub(crate) fn close(&self) {
        let mut state = self.lock();
        state.closed = true;
        for queue in &mut state.queues {
            queue.clear();
        }
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
    (RendererSender { scheduler }, to_compositor, receiver)
}

/// The browser's end of a renderer's pipe (a `mojo::Remote` to the
/// renderer's interfaces). Each message becomes a task on the main thread's
/// scheduler, posted from the thread that sends it; once the renderer is a
/// process of its own, whatever reads the IPC channel posts them instead.
/// DevTools' go to their own queue, which script paused or busy takes from
/// too (Chromium's DevTools messages go through the IO thread to Blink's
/// `InspectorTaskRunner`).
#[derive(Clone)]
pub struct RendererSender {
    scheduler: Arc<MainThreadScheduler>,
}

impl RendererSender {
    /// Fails once the main thread has gone; the message is dropped.
    pub fn send(&self, msg: RendererMsg) -> Result<(), RendererGone> {
        let scheduler = &self.scheduler;
        let (queued, interrupt) = match msg {
            RendererMsg::DevToolsAttach { session } => (DevToolsQueued::Attach(session), false),
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
                let shutdown = matches!(msg, RendererMsg::Shutdown);
                scheduler.post(TaskQueue::Browser, MainThreadTask::Browser(msg), false)?;
                if shutdown {
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
            MainThreadTask::CanvasEncoded { id, .. } => format!("canvas {id}"),
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
                "Attach(\"c\")",
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
}
