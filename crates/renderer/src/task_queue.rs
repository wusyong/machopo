//! The renderer main thread's task queue (Blink's `MainThreadSchedulerImpl`
//! and its task queues, without priorities yet). Everything the main thread
//! does arrives here as one task at a time, in the order it was posted,
//! whichever thread posted it: the main thread waits on this queue alone.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

use compositor::{CompositorReceiver, CompositorSender, ToMainThread};
use crossbeam_channel::{Receiver, Sender};
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
    /// The browser put something in the DevTools queue.
    DevTools,
}

/// A new renderer's queues: the browser's way to the main thread, Viz's and
/// the browser's way to the compositor thread, and the renderer's ends of
/// both.
pub fn channel() -> (RendererSender, CompositorSender, RendererReceiver) {
    let (tx, rx) = crossbeam_channel::unbounded();
    let (to_compositor, compositor) = compositor::channel();
    let devtools = Arc::new(DevToolsQueue::default());
    let receiver = RendererReceiver {
        rx,
        tx: tx.clone(),
        compositor,
        devtools: devtools.clone(),
    };
    (RendererSender { tx, devtools }, to_compositor, receiver)
}

/// The browser's end of a renderer's pipe (a `mojo::Remote` to the
/// renderer's interfaces). Each message becomes a task on the main thread's
/// queue, posted from the thread that sends it; once the renderer is a
/// process of its own, whatever reads the IPC channel posts them instead.
/// DevTools' go to the DevTools queue, which script paused or busy takes
/// from too (Chromium's DevTools messages go through the IO thread to
/// Blink's `InspectorTaskRunner`).
#[derive(Clone)]
pub struct RendererSender {
    tx: Sender<MainThreadTask>,
    devtools: Arc<DevToolsQueue>,
}

impl RendererSender {
    /// Fails once the main thread has gone; the message is dropped.
    pub fn send(&self, msg: RendererMsg) -> Result<(), RendererGone> {
        let (queued, interrupt) = match msg {
            RendererMsg::DevToolsAttach { session } => (DevToolsQueued::Attach(session), false),
            RendererMsg::DevToolsDetach { session } => (DevToolsQueued::Detach(session), false),
            RendererMsg::DevToolsMessage {
                session,
                message,
                interrupt,
            } => (DevToolsQueued::Message { session, message }, interrupt),
            msg => {
                // Script paused is to go on, for the main thread to get
                // to the message.
                if matches!(msg, RendererMsg::Shutdown) {
                    self.devtools.push(DevToolsQueued::Quit, false);
                }
                return self
                    .tx
                    .send(MainThreadTask::Browser(msg))
                    .map_err(|_| RendererGone);
            }
        };
        self.devtools.push(queued, interrupt);
        self.tx
            .send(MainThreadTask::DevTools)
            .map_err(|_| RendererGone)
    }
}

/// The renderer's DevTools queue: what the browser asked of the page's
/// sessions, in order. The main thread takes it at its loop's top; script
/// paused (a breakpoint) takes it alone, and a message to interrupt script
/// is taken at once (Blink's `InspectorTaskRunner`).
#[derive(Default)]
pub struct DevToolsQueue {
    /// Each with whether it is to interrupt script.
    queued: Mutex<VecDeque<(DevToolsQueued, bool)>>,
    ready: Condvar,
    /// The engine's, which has its thread take what interrupts script: set
    /// once, as the engine is made.
    interrupter: OnceLock<DevToolsInterrupter>,
}

impl DevToolsQueue {
    fn push(&self, queued: DevToolsQueued, interrupt: bool) {
        self.queued
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back((queued, interrupt));
        self.ready.notify_one();
        if interrupt && let Some(interrupter) = self.interrupter.get() {
            interrupter();
        }
    }

    /// The next, if there is one.
    pub(crate) fn try_next(&self) -> Option<DevToolsQueued> {
        self.queued
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
            .map(|(queued, _)| queued)
    }

    /// The engine's (`ScriptEngine::set_devtools_queue`): the first one given.
    pub(crate) fn set_interrupter(&self, interrupter: DevToolsInterrupter) {
        if self.interrupter.set(interrupter).is_err() {
            tracing::warn!("the DevTools queue has an interrupter already");
        }
    }

    /// The next, waiting for one (while script is paused).
    pub(crate) fn wait_next(&self) -> DevToolsQueued {
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some((next, _)) = queued.pop_front() {
                return next;
            }
            queued = self
                .ready
                .wait(queued)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The messages that are to interrupt script, taken out in order:
    /// those before any attach or detach (whose sessions the engine knows).
    pub(crate) fn take_interrupting(&self) -> Vec<(String, String)> {
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        let mut taken = Vec::new();
        let mut kept = VecDeque::new();
        let mut known = true;
        for (next, interrupt) in queued.drain(..) {
            // After an attach or detach, the sessions aren't known yet.
            known &= matches!(next, DevToolsQueued::Message { .. });
            match next {
                DevToolsQueued::Message { session, message } if interrupt && known => {
                    taken.push((session, message));
                }
                next => kept.push_back((next, interrupt)),
            }
        }
        *queued = kept;
        taken
    }

    /// Puts `back` first, in order: what waited for the pause to end.
    pub(crate) fn put_back(&self, back: Vec<DevToolsQueued>) {
        let mut queued = self.queued.lock().unwrap_or_else(PoisonError::into_inner);
        for next in back.into_iter().rev() {
            queued.push_front((next, false));
        }
    }
}

/// The renderer main thread has gone, and its queue with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RendererGone;

/// The renderer's ends of its queues: the main thread's, and the compositor
/// thread's.
pub struct RendererReceiver {
    pub(crate) rx: Receiver<MainThreadTask>,
    /// For the compositor thread to post `BeginMainFrame` and input.
    pub(crate) tx: Sender<MainThreadTask>,
    pub(crate) compositor: CompositorReceiver,
    pub(crate) devtools: Arc<DevToolsQueue>,
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

    /// A message to interrupt script calls the engine's interrupter, and is
    /// taken out alone, in order, but for one after an attach or detach
    /// (its session not known yet); what the pause leaves goes back first.
    #[test]
    fn what_interrupts_script_is_taken_out_alone() {
        let (sender, _compositor, receiver) = channel();
        let (interrupted, interrupts) = crossbeam_channel::unbounded();
        receiver.devtools.set_interrupter(Box::new(move || {
            let _ = interrupted.send(());
        }));
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
        let queue = &receiver.devtools;
        assert_eq!(
            queue.take_interrupting(),
            [
                ("a".to_owned(), "2".to_owned()),
                ("b".to_owned(), "3".to_owned())
            ]
        );
        queue.put_back(vec![queued("x", "0")]);
        let left: Vec<_> = std::iter::from_fn(|| queue.try_next()).collect();
        assert_eq!(
            left,
            [
                queued("x", "0"),
                queued("a", "1"),
                DevToolsQueued::Attach("c".to_owned()),
                queued("c", "4"),
            ]
        );
        // A task for each, on the main queue.
        assert_eq!(receiver.rx.try_iter().count(), 5);
    }

    /// The renderer told to end lets script paused go on: the DevTools
    /// queue has it too, which a pause waiting on it wakes to.
    #[test]
    fn a_shutdown_ends_a_pause() {
        let (sender, _compositor, receiver) = channel();
        let queue = receiver.devtools.clone();
        // Answered on a channel, not joined: a pause the shutdown doesn't
        // wake fails the test rather than hanging it.
        let (woke, woken) = std::sync::mpsc::channel();
        std::thread::spawn(move || woke.send(queue.wait_next()));
        sender.send(RendererMsg::Shutdown).unwrap();
        assert_eq!(
            woken.recv_timeout(std::time::Duration::from_secs(10)),
            Ok(DevToolsQueued::Quit),
            "the pause wasn't woken"
        );
        assert!(matches!(
            receiver.rx.try_recv(),
            Ok(MainThreadTask::Browser(RendererMsg::Shutdown))
        ));
    }
}
