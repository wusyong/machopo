//! A renderer in a process of its own (Chromium's `RendererMain` and the
//! `ChildProcess` it makes): the process's one IO thread reads the
//! channel to the browser and posts what comes to the renderer's threads,
//! which send by the channel themselves.
//!
//! The renderer's threads are the same as in the browser's process: they
//! know nothing of which they are in.

use std::sync::mpsc;

use compositor::CompositorSender;
use ipc::core::channel::Gone;
use ipc::core::frame::BadMessage;
use ipc::endpoint::{self, Handler, IoLoop, LoopHandle, Received};
use ipc::platform::Channel;
use protocol::control_channel::{self, BrowserSender, to_renderer};
use protocol::renderer::{CompositorMsg, RendererMsg};
use protocol::viz::CompositorFrameSinkSender;
use serde::{Deserialize, Serialize};

use crate::{
    RendererEndpoints, RendererFlags, RendererSender, ScriptEngineFactory,
    spawn_renderer_main_thread,
};

/// What a renderer process starts with: the first message on its channel,
/// to the main thread's destination. Not its command line: a value has a
/// type here, and whatever starts the process (a test that runs itself
/// again) needs no arguments of the renderer's.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChildInit {
    pub flags: RendererFlags,
    /// Whether Viz has a GPU for the renderer's shared images
    /// ([`RendererEndpoints::gpu`]).
    pub gpu: bool,
}

/// How a renderer process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildExit {
    /// Its main thread ended as it was told to, or its browser went.
    Ended,
    /// It could not start: no IO thread, or no threads of the renderer's.
    NotStarted,
    /// Its main thread panicked.
    Crashed,
}

/// The IO thread's side of the channel: each message to the thread it is
/// for.
struct ChildHandler {
    /// Where the first message goes; none once it came.
    init: Option<mpsc::Sender<ChildInit>>,
    to_main: RendererSender,
    to_compositor: CompositorSender,
    io: LoopHandle<Channel>,
}

impl Handler<Channel> for ChildHandler {
    fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
        // A thread that has gone takes nothing: the renderer is ending,
        // and the browser hears of it by the channel.
        match message.destination() {
            to_renderer::MAIN => {
                if let Some(init) = self.init.take() {
                    let _ = init.send(message.decode::<ChildInit>()?);
                } else {
                    let msg = message.decode::<RendererMsg>()?;
                    if matches!(msg, RendererMsg::Crash) {
                        // Here, not on the main thread: a renderer that
                        // hangs crashes too. Abnormally, as a crash is:
                        // nothing is unwound or flushed.
                        std::process::abort();
                    }
                    let _ = self.to_main.send(msg);
                }
            }
            to_renderer::COMPOSITOR => {
                let _ = self.to_compositor.send(message.decode::<CompositorMsg>()?);
            }
            _ => return Err(BadMessage::Rejected),
        }
        Ok(())
    }

    fn gone(&mut self, gone: Gone) {
        // No browser: nothing to render for. Whoever waits for the first
        // message hears there is none, and a renderer that runs ends.
        tracing::debug!(?gone, "the channel to the browser is over");
        self.init = None;
        let _ = self.to_main.send(RendererMsg::Shutdown);
        self.io.stop();
    }
}

/// Runs a renderer in this process until it ends: `channel` is the
/// process's end of the channel to its browser
/// (`ipc::platform::inherited`), and `script_engine` the engines whoever
/// built the process picked. Returns on the thread it was called on, once
/// the renderer's main thread and the IO thread are over.
pub fn run_child_process(channel: Channel, script_engine: ScriptEngineFactory) -> ChildExit {
    let (link, endpoint) = endpoint::endpoint(channel, control_channel::config(false));
    let Some((io, io_handle)) = IoLoop::new() else {
        return ChildExit::NotStarted;
    };
    let (to_main, to_compositor, from_browser) = crate::channel();
    let (init, first) = mpsc::channel();
    io_handle.serve(
        endpoint,
        ChildHandler {
            init: Some(init),
            to_main,
            to_compositor,
            io: io_handle.clone(),
        },
    );
    // Chromium's `Chrome_ChildIOThread`.
    let Ok(io_thread) = std::thread::Builder::new()
        .name("Chrome_ChildIOThread".into())
        .spawn(move || io.run())
    else {
        return ChildExit::NotStarted;
    };
    // Said once the process reads its channel: whoever watches a renderer
    // start (a test) knows from here that it hears its browser go.
    tracing::info!("the renderer process has its channel");
    let exit = match first.recv() {
        Ok(ChildInit { flags, gpu }) => {
            let endpoints = RendererEndpoints {
                from_browser,
                to_browser: BrowserSender::over(link.clone()),
                viz: CompositorFrameSinkSender::over(link.clone()),
                gpu,
            };
            match spawn_renderer_main_thread(endpoints, flags, script_engine) {
                Ok(main) => {
                    tracing::info!("the renderer's threads are started");
                    match main.join() {
                        Ok(()) => ChildExit::Ended,
                        Err(_) => ChildExit::Crashed,
                    }
                }
                Err(_) => ChildExit::NotStarted,
            }
        }
        // The browser went before it said what to start with.
        Err(mpsc::RecvError) => ChildExit::Ended,
    };
    // The renderer is over: the browser hears it by the channel.
    link.close();
    io_handle.stop();
    let _ = io_thread.join();
    exit
}
