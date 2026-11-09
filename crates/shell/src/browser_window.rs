//! A browser window without the platform's window (Chromium's
//! `content::Shell`, minus its `ShellPlatformDelegate`): the tabs and their
//! renderers, the Viz thread, and the [`BrowserView`]. The `machopo` window puts
//! winit and wgpu around it; the scenario tests run it with a software
//! renderer, so they go through what the window does, its `RendererFlags`
//! and Viz's own `BeginFrame`s included.

use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use browser::{
    ContentsEventCallback, NetworkContext, NetworkContextParams, RendererFlags, WebContents,
};
use crossbeam_channel::Sender;
use gfx::Size;
use protocol::TabId;
use protocol::viz::{DisplayMsg, SharedImageInterface, VizMsg};
use viz::DirectRenderer;

use crate::browser_view::{BROWSER_FRAME_SINK, BrowserView, TabFactory};
use crate::omnibox::Clipboard;

/// How the window's tabs run.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// `RendererFlags::test_fonts`, the toolbar's text too.
    pub test_fonts: bool,
    /// `RendererFlags::automation`.
    pub automation: bool,
    /// `RendererFlags::num_raster_threads`.
    pub num_raster_threads: Option<usize>,
    /// `RendererFlags::caret_blink_interval`.
    pub caret_blink_interval: Option<std::time::Duration>,
    /// The network service's: the user agent is the browser's whatever it
    /// says; web tests map WPT's domains and trust its root certificate.
    pub network: NetworkContextParams,
}

pub struct BrowserWindow {
    view: BrowserView,
    viz: Sender<VizMsg>,
    viz_thread: Option<JoinHandle<()>>,
}

impl BrowserWindow {
    /// Starts the first tab's renderer and the Viz thread, drawing with
    /// what `make_renderer` makes there, into an output `size` physical px
    /// at `scale`. `gpu` is where tiles go on the GPU Viz draws with
    /// (with `None`, to Viz as pixels it uploads). Every tab's renderer's
    /// messages go to
    /// `on_message`, on the IO thread; the embedder hands them to the view
    /// on its own.
    pub fn start<R, F>(
        options: Options,
        size: Size,
        scale: f32,
        gpu: Option<Arc<dyn SharedImageInterface>>,
        make_renderer: F,
        clipboard: Box<dyn Clipboard>,
        on_message: ContentsEventCallback,
    ) -> Result<Self, String>
    where
        R: DirectRenderer,
        F: FnOnce() -> Result<R, String> + Send + 'static,
    {
        let (viz_tx, viz_rx) = crossbeam_channel::unbounded();
        // The one user agent: requests send it, and pages read it.
        let user_agent = browser::user_agent();
        let network = NetworkContext::start(NetworkContextParams {
            user_agent: user_agent.clone(),
            ..options.network
        })
        .map_err(|e| format!("start the network service: {e}"))?;
        let mut tabs = WindowTabs {
            flags: RendererFlags {
                test_fonts: options.test_fonts,
                web_tests: false,
                // Scrolling goes on while the page's script runs.
                wait_for_all_pipeline_stages_before_draw: false,
                automation: options.automation,
                user_agent,
                system_colors: crate::system_colors::for_window(options.test_fonts),
                num_raster_threads: options.num_raster_threads,
                caret_blink_interval: options.caret_blink_interval,
            },
            viz: viz_tx.clone(),
            gpu: gpu.clone(),
            network,
            on_message: Arc::new(Mutex::new(on_message)),
        };
        // Registered with Viz before it starts: the message waits for it.
        let web_contents = tabs.create(TabId(0))?;

        // The browser's own frame is the root; it embeds the active tab's.
        // Each tab's renderer asks Viz for `BeginFrame`s; the browser's UI
        // submits frames on its own.
        let viz_thread = viz::spawn_viz_compositor_thread(
            viz_rx,
            BROWSER_FRAME_SINK,
            viz::BeginFrameSource::new(),
            make_renderer,
        )
        .map_err(|e| format!("spawn viz thread: {e}"))?;

        let mut view = BrowserView::new(
            web_contents,
            viz_tx.clone(),
            gpu,
            options.test_fonts,
            clipboard,
        );
        view.set_tab_factory(Box::new(tabs));
        view.resize(size, scale);
        Ok(Self {
            view,
            viz: viz_tx,
            viz_thread: Some(viz_thread),
        })
    }

    pub fn view(&self) -> &BrowserView {
        &self.view
    }

    pub fn view_mut(&mut self) -> &mut BrowserView {
        &mut self.view
    }

    /// Viz's end, for what the window asks of the display.
    pub fn viz(&self) -> &Sender<VizMsg> {
        &self.viz
    }

    /// The output is now `size` physical px, at `scale`.
    pub fn resize(&mut self, size: Size, scale: f32) {
        let _ = self.viz.send(VizMsg::Display(DisplayMsg::Resize(size)));
        self.view.resize(size, scale);
    }

    /// Stops the tabs' renderers and the Viz thread, waiting for Viz.
    pub fn shutdown(&mut self) {
        self.view.shutdown();
        let _ = self.viz.send(VizMsg::Display(DisplayMsg::Shutdown));
        if let Some(t) = self.viz_thread.take() {
            let _ = t.join();
        }
    }
}

/// Makes the window's tabs, all alike (Chromium's `Browser` with the
/// window's profile), and tells Viz of their frame sinks
/// (`HostFrameSinkManager`).
struct WindowTabs {
    flags: RendererFlags,
    viz: Sender<VizMsg>,
    gpu: Option<Arc<dyn SharedImageInterface>>,
    network: NetworkContext,
    /// Shared by every tab's renderer.
    on_message: Arc<Mutex<ContentsEventCallback>>,
}

impl TabFactory for WindowTabs {
    fn create(&mut self, tab: TabId) -> Result<WebContents, String> {
        let on_message = self.on_message.clone();
        let contents = WebContents::new(
            tab,
            self.flags.clone(),
            crate::SCRIPT_ENGINES,
            self.viz.clone(),
            self.gpu.clone(),
            self.network.clone(),
            Box::new(move |tab, msg| {
                let on_message = on_message.lock().unwrap_or_else(|e| e.into_inner());
                on_message(tab, msg);
            }),
        )
        .map_err(|e| format!("failed to start renderer: {e}"))?;
        let _ = self.viz.send(VizMsg::RegisterFrameSinkId(
            contents.frame_sink_id(),
            contents.begin_frame_observer(),
        ));
        Ok(contents)
    }

    fn destroy(&mut self, mut contents: WebContents) {
        let sink = contents.frame_sink_id();
        contents.shutdown();
        let _ = self.viz.send(VizMsg::InvalidateFrameSinkId(sink));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Viz hears of a tab's frame sink when the tab is made, and that it
    /// has gone when the tab is closed.
    #[test]
    fn window_tabs_tell_viz_of_their_frame_sinks() {
        let (viz, viz_rx) = crossbeam_channel::unbounded();
        let mut tabs = WindowTabs {
            flags: RendererFlags {
                test_fonts: true,
                web_tests: false,
                wait_for_all_pipeline_stages_before_draw: false,
                automation: false,
                user_agent: browser::user_agent(),
                system_colors: gfx::SystemColors::DEFAULT,
                num_raster_threads: None,
                caret_blink_interval: None,
            },
            viz,
            gpu: None,
            network: NetworkContext::start(NetworkContextParams {
                user_agent: browser::user_agent(),
                ..Default::default()
            })
            .unwrap(),
            on_message: Arc::new(Mutex::new(Box::new(|_, _| {}))),
        };
        let contents = tabs.create(TabId(3)).unwrap();
        let sink = contents.frame_sink_id();
        assert!(
            viz_rx
                .try_iter()
                .any(|msg| matches!(msg, VizMsg::RegisterFrameSinkId(id, _) if id == sink))
        );
        tabs.destroy(contents);
        assert!(
            viz_rx
                .try_iter()
                .any(|msg| matches!(msg, VizMsg::InvalidateFrameSinkId(id) if id == sink))
        );
    }
}
