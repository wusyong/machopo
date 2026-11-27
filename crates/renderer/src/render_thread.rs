use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use compositor::mojo_embedder::ChannelLayerTreeFrameSink;
use compositor::{
    CompositorScrollDelta, DocumentId, InputDispatch, LayerTreeHost, LayerTreeSettings, ProxyMain,
    SwapPromise, ToMainThread,
};
use dom::NavigationType;
use gfx::{PointF, RectF, SizeF};
use protocol::control_channel::BrowserSender;
use protocol::input::{InputEvent, Modifiers};
use protocol::network::{LoadResult, RedirectInfo};
use protocol::renderer::{HistoryEntryParams, RendererHostMsg, RendererMsg, VisualProperties};
use protocol::viz::{BeginFrameArgs, CompositorFrameSinkSender};
use url::Url;

use layout::TextContext;

use crate::canvas_encoder::CanvasEncoder;
use crate::clock::Clock;
use crate::frame_widget::FrameWidget;
use crate::loader::BlobUrls;
use crate::navigation_timing::NavigationTiming;
use crate::pending_user_input::{Pending, PendingUserInput};
use crate::task_queue::{
    InternalTask, MainThreadScheduler, MainThreadTask, RendererReceiver, TaskQueues,
};

mod css_agent;
mod devtools;
mod document_body;
mod documents;
mod dom_agent;
mod log_agent;
mod network_agent;
mod overlay_agent;
mod page_agent;
mod render_frame;
mod webdriver;
mod webdriver_commands;

/// The pipes a renderer is born with (Chromium hands these over as mojo
/// endpoints when the `RenderProcessHost` launches the process).
pub struct RendererEndpoints {
    /// The renderer's queues: the main thread's, which the browser's
    /// messages arrive on, and the compositor thread's, which Viz's
    /// `BeginFrame`s and the browser's input do (the other ends of
    /// [`crate::channel`]).
    pub from_browser: RendererReceiver,
    pub to_browser: BrowserSender,
    /// `CompositorFrameSink` to Viz, bound to the renderer's `FrameSinkId`
    /// by the browser. cc submits frames directly to Viz, not through the
    /// browser, as in Chromium.
    pub viz: CompositorFrameSinkSender,
    /// Whether Viz has a GPU for the renderer's shared images (Chromium's
    /// GPU channel, which the browser establishes for the renderer): the
    /// compositor makes them by messages on `viz`. Without one, tiles'
    /// pixels go with the frames.
    pub gpu: bool,
}

/// Where a renderer's text gets the system's fonts.
pub enum SystemFontSource {
    /// From the browser, as it needs them (Chromium's `FontDataService`):
    /// what the browser says of them to begin with. The renderer opens no
    /// font file and asks the system nothing.
    Browser(protocol::font::FontNames),
    /// As the main thread finds them: where the browser doesn't serve them
    /// (a platform with no sandbox), or couldn't (no font thread).
    FindHere,
}

/// Switches the browser passes down when it launches a renderer
/// (`RenderProcessHostImpl::PropagateBrowserCommandLineToRenderer`):
/// values only, which a renderer process of its own would get on its
/// command line.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RendererFlags {
    /// Only the built-in test fonts (Ahem, and Lato for every generic
    /// family), as `content_shell` does for web tests.
    pub test_fonts: bool,
    /// As content_shell's `--run-web-tests`: tell the browser whether a
    /// loaded document waits for `reftest-wait` to go
    /// (`DidFinishLoad::reftest_wait`), and when it goes
    /// (`ReftestWaitEnded`).
    pub web_tests: bool,
    /// The compositor draws each `BeginFrame` once the main frame it
    /// started has committed (headless and web tests: every run comes out
    /// the same), rather than at once from what it has (the window: a main
    /// thread busy with script doesn't stop scrolling). cc's
    /// `LayerTreeSettings::wait_for_all_pipeline_stages_before_draw`.
    pub wait_for_all_pipeline_stages_before_draw: bool,
    /// Answer `RendererMsg::QueryElement`, for automated checks of a
    /// browser window (`machopo --remote-debugging-pipe`, the scenario tests), as
    /// `content_shell --run-web-tests` exposes `internals` only to tests.
    pub automation: bool,
    /// The browser's user agent string, which `navigator.userAgent` gives
    /// (Chromium's `mojom::Renderer::SetUserAgent`).
    pub user_agent: String,
    /// The platform's system colors, which the browser reads (Chromium's
    /// `ColorProvider`, which it sends every renderer); Blink's defaults
    /// in headless runs and web tests.
    pub system_colors: gfx::SystemColors,
    /// The compositor's raster threads (Chromium's `--num-raster-threads`,
    /// which the browser passes its renderers); `None`: the compositor's
    /// default, its own thread.
    pub num_raster_threads: Option<usize>,
    /// The bytes of tiles the compositor keeps (cc's
    /// `ManagedMemoryPolicy`, which the browser works out for the screen
    /// and the machine: a renderer in its sandbox knows neither); `None`:
    /// the compositor's default.
    pub tile_memory_limit: Option<u64>,
    /// How long the caret stays shown, and then hidden, as it blinks
    /// (Chromium's `RendererPreferences::caret_blink_interval`); `None`:
    /// it doesn't blink, as in web tests.
    pub caret_blink_interval: Option<std::time::Duration>,
    /// How long the parser parses at a time once its document has its
    /// body, and how much text it is given at a time.
    pub parser_budget: ParserBudget,
    /// How long after a main frame's document commits the page before it
    /// stays shown while the new one can't be drawn (a sheet blocks it, or
    /// its body hasn't begun): then the new one shows empty till it can.
    pub new_content_rendering_delay: std::time::Duration,
}

/// Blink's `kNewContentRenderingDelay`, which the browser's
/// `RenderWidgetHostImpl` waits before it clears the page shown.
pub const NEW_CONTENT_RENDERING_DELAY: std::time::Duration = std::time::Duration::from_secs(4);

/// How long a document's parser parses before it lets the loop's other
/// work (input, frames, timers) run, once the document has its body
/// (Blink's `HTMLDocumentParser::PumpTokenizer` budgets), and how much of
/// the document's text it is given at a time: it can only stop between
/// those slices (html5ever's tokenizer takes all it has).
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct ParserBudget {
    /// The budget of the parser's first `long_after` runs past their first.
    pub budget: std::time::Duration,
    /// The budget after: the rest of the page in larger runs.
    pub long_budget: std::time::Duration,
    pub long_after: u32,
    /// The most bytes of text the parser is given at a time.
    pub slice_bytes: usize,
}

impl Default for ParserBudget {
    /// Blink's on the desktop: 10 ms for the first 6 runs after the first,
    /// 500 ms after; slices of about a millisecond's parsing.
    fn default() -> Self {
        Self {
            budget: std::time::Duration::from_millis(10),
            long_budget: std::time::Duration::from_millis(500),
            long_after: 6,
            slice_bytes: 16 * 1024,
        }
    }
}

impl ParserBudget {
    /// One that lets other work run after each slice of `slice_bytes`, as
    /// tests want: what they see doesn't depend on the machine's speed
    /// (Blink's `HTMLParserYieldAndDelayOftenForTesting`).
    pub fn yield_every_slice(slice_bytes: usize) -> Self {
        Self {
            budget: std::time::Duration::ZERO,
            long_budget: std::time::Duration::ZERO,
            long_after: 0,
            slice_bytes,
        }
    }

    /// The budget of a run after the parser let other work run `yielded`
    /// times.
    pub(crate) fn of_run(&self, yielded: u32) -> std::time::Duration {
        if yielded <= self.long_after {
            self.budget
        } else {
            self.long_budget
        }
    }
}

/// Makes the script engine, on the renderer's main thread (an engine
/// isn't `Send`), at the first document. The renderer names no engine:
/// whoever starts it picks one, such as `dom_js::V8Engine`; a
/// renderer process of its own would have its `main` pick it, as
/// Chromium's picks its `ContentRendererClient`.
#[derive(Clone, Copy)]
pub struct ScriptEngineFactory {
    pub engine: fn(dom_api::EngineSettings) -> Box<dyn dom_api::ScriptEngine>,
    /// The engine of a document that declares its scripts wasm
    /// (`<meta name="script" content="wasm">`), such as
    /// `dom_wasm::WasmEngine`; with none, such a document is as any other
    /// (its wasm scripts don't run).
    pub wasm_engine: Option<fn(dom_api::EngineSettings) -> Box<dyn dom_api::ScriptEngine>>,
    /// A dedicated worker's engine (`dom_api::WorkerEngine`), made on the
    /// worker's thread, such as `dom_js::V8WorkerEngine`; with none,
    /// `new Worker()` makes one that never runs (an `error` at it).
    pub worker_engine: Option<fn(dom_api::EngineSettings) -> Box<dyn dom_api::WorkerEngine>>,
}

/// What stops the script a renderer's main thread runs, from another
/// thread: for a renderer whose main thread is a thread of the browser's
/// process (`--single-process`, headless), which can't be killed as
/// machopo ends while its page's script runs on (V8's
/// `TerminateExecution`, as `Worker.terminate()` stops a worker's script,
/// Blink's `WorkerThread::EnsureScriptExecutionTerminates`). It stops the
/// engine's of the moment: the main thread puts each engine's in as it
/// makes the engine (a page whose scripts are wasm has another). Shared by
/// the two threads, so a lock, taken only then and as machopo ends.
#[derive(Clone, Default)]
pub struct MainThreadTerminator(Arc<Mutex<Option<Terminate>>>);

/// What an engine gives to stop its script (`ScriptEngine::terminator`).
type Terminate = Box<dyn Fn() + Send>;

impl MainThreadTerminator {
    /// Stops the script running now, if any; script after it is stopped
    /// too until its stack has unwound.
    pub fn terminate(&self) {
        if let Some(terminate) = self
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            terminate();
        }
    }

    fn set(&self, terminate: Terminate) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(terminate);
    }
}

/// Spawns the renderer's threads: the compositor thread (`Compositor`,
/// as `RenderThreadImpl::InitializeCompositorThread` does) and the main
/// thread (`CrRendererMain`), which owns the other and stops it when it
/// exits. The handle is the main thread's.
///
/// `system_fonts`: where the text gets the system's fonts (none of it with
/// `flags.test_fonts`).
pub fn spawn_renderer_main_thread(
    endpoints: RendererEndpoints,
    flags: RendererFlags,
    script_engine: ScriptEngineFactory,
    terminator: Option<MainThreadTerminator>,
    system_fonts: SystemFontSource,
) -> std::io::Result<JoinHandle<()>> {
    let RendererEndpoints {
        from_browser,
        to_browser,
        viz,
        gpu,
    } = endpoints;
    let RendererReceiver {
        scheduler,
        compositor,
    } = from_browser;
    // For the compositor thread to post to the main thread.
    let to_main = scheduler.clone();
    // The impl side, with the frame sink to Viz, goes to the compositor
    // thread; it asks the main thread for main frames, and passes on the
    // input it doesn't handle, on the main queue, and acknowledges what it
    // handles to the browser itself.
    // Tiles go to the GPU by messages to Viz, on the same channel as the
    // frames that show them, if Viz has a GPU to put them on.
    // From a process of the renderer's own a tile is drawn in a region
    // of the channel's, where there is a free one, so its pixels cross
    // without being copied.
    let shared_images = gpu.then(|| {
        let viz = viz.clone();
        let regions = viz.clone();
        compositor::SharedImages::with_regions(
            move |msg| {
                let _ = viz.send(msg);
            },
            move |len| regions.take_region(len),
        )
    });
    let frame_sink = ChannelLayerTreeFrameSink::new(viz);
    let defaults = LayerTreeSettings::default();
    // The input on its way to the main thread, counted as it is posted.
    let pending_input = PendingUserInput::default();
    let posted_input = pending_input.clone();
    let layer_tree_host = LayerTreeHost::new_threaded(
        LayerTreeSettings {
            wait_for_all_pipeline_stages_before_draw: flags
                .wait_for_all_pipeline_stages_before_draw,
            shared_images,
            num_raster_threads: flags
                .num_raster_threads
                .unwrap_or(defaults.num_raster_threads),
            tile_memory_limit: flags
                .tile_memory_limit
                .unwrap_or(defaults.tile_memory_limit),
            ..defaults
        },
        Box::new(frame_sink),
        &compositor,
        move |task| {
            if let ToMainThread::Input {
                event,
                attribution,
                document,
                ..
            } = &task
            {
                posted_input.enqueue(Pending::of(event, *attribution, *document));
            }
            to_main.post_compositor(task);
        },
        to_browser.clone(),
    );
    let compositor_thread = std::thread::Builder::new()
        .name("Compositor".into())
        .spawn(move || compositor.run())?;
    std::thread::Builder::new()
        .name("CrRendererMain".into())
        // As a renderer's main thread has on Linux (8 MiB, Chrome's): style,
        // layout and paint recurse once a level of the tree, and a page
        // may nest elements (or shadow trees) a thousand deep; Rust's
        // default of 2 MiB overflows there.
        .stack_size(8 << 20)
        .spawn(move || {
            let mut thread = RenderThread::new(
                to_browser,
                scheduler.clone(),
                layer_tree_host,
                pending_input,
                flags,
                script_engine,
                terminator,
                system_fonts,
            );
            thread.run();
            // Nothing is posted to the main thread from now on.
            scheduler.close();
            // Drop V8 state, and the main side of the compositor, which
            // stops the compositor thread; wait for it to finish. The
            // browser hears we are gone when the last sender of the channel
            // to it (ours, or the compositor's in a swap promise) is dropped.
            drop(thread);
            if compositor_thread.join().is_err() {
                tracing::error!("compositor thread panicked");
            }
        })
}

/// The session history's new entries the renderer made itself and told
/// the browser of (`DidCommitSameDocumentNavigation` neither `replace` nor
/// a traversal: `pushState`, a fragment's navigation), counted so that a
/// place the browser gives keeps those it hadn't heard of as it gave it:
/// the browser says how many it had (`entries_heard`), and the rest come
/// after. Blink counts its own new entries too
/// (`IncreaseHistoryListFromNavigation`) but takes a commit's place as it
/// is, and the browser broadcasts to every renderer but the committing
/// one; here every frame is in the one renderer, which hears of every
/// commit, a child frame's too. Not the engine's: a new engine (a page of
/// the other kind) doesn't start the count again, the browser's goes on.
///
/// The render thread reads it, its frame client counts them as it sends
/// them (in the engine's call): both the renderer's, on this thread, so
/// they share one cell, which outlasts the client an engine has (Chromium
/// keeps the count in content's `RenderViewImpl`, not in Blink).
#[derive(Clone, Default)]
pub(super) struct OwnHistoryEntries {
    sent: Rc<Cell<u64>>,
}

impl OwnHistoryEntries {
    /// One more is on its way to the browser.
    pub(super) fn sent_one(&self) {
        let sent = self
            .sent
            .get()
            .checked_add(1)
            .expect("fewer entries than a u64 counts");
        self.sent.set(sent);
    }

    /// The place the browser gave, `index` of `length` as it was when it
    /// had heard of `heard` of them, with those it hadn't after it.
    fn place(&self, index: u32, length: u32, heard: u64) -> dom::SessionHistory {
        // The browser counts them as it has their messages, which were
        // all sent before: it has never heard of more than were sent.
        let unheard = self.sent.get().saturating_sub(heard);
        dom::SessionHistory { index, length }.after_new_entries(unheard)
    }
}

/// Everything that lives on the renderer main thread.
///
/// Field order is drop order: the document (which roots its nodes) must go
/// before the engine whose heap they live in.
struct RenderThread {
    /// The thread's text context (its fonts and shaping caches), until the
    /// first document's frame takes it.
    text: TextContext,
    /// The thread the DOM's `toBlob` files are encoded on, once one is.
    canvas_encoder: CanvasEncoder,
    /// The dedicated workers its documents made (`worker_thread`): the
    /// engine starts them through its host of them.
    workers: Rc<crate::worker_thread::Workers>,
    to_browser: BrowserSender,
    /// The widget, until the first document's frame takes it: the frame's
    /// from then on (`widget`).
    widget: Option<FrameWidget>,
    visual_properties: VisualProperties,
    /// The clock's time the last `BeginMainFrame` came at: the next is
    /// expected a frame interval later, where an idle period ends.
    last_main_frame: f64,
    /// When the idle period running ends, if one is (Blink's
    /// `IdleHelper`): `requestIdleCallback`'s callbacks run in it.
    idle_period_end: Option<f64>,
    /// How long the caret stays shown, then hidden (`RendererFlags::
    /// caret_blink_interval`), and when it next shows or hides, while
    /// there is one to blink (Blink's `FrameCaret::caret_blink_timer_`).
    caret_blink_interval: Option<Duration>,
    next_caret_blink: Option<Instant>,
    /// `RendererFlags::parser_budget`.
    parser_budget: ParserBudget,
    /// `RendererFlags::new_content_rendering_delay`, and when it is up for
    /// the main frame's document that committed, until it is drawn
    /// (`RenderWidgetHostImpl::new_content_rendering_timeout_`); then,
    /// still blocked, the next main frame shows it empty.
    new_content_rendering_delay: Duration,
    new_content_due: Option<Instant>,
    empty_frame_wanted: bool,
    /// The browser has heard the top-level document has sticky activation
    /// (`RendererHostMsg::DidReceiveUserActivation`): once for each.
    activation_told: bool,
    /// The frame's pre-paint count, the DOM's intersection observations'
    /// version and, while an observer tracks visibility, the last paint,
    /// when the intersections were last computed: they are again only once
    /// one changed (Blink's `LocalFrameView::intersection_observation_state_`),
    /// as what they are made of all goes into pre-paint, and what covers a
    /// target into paint.
    intersections_computed: Option<(u64, u64, Option<paint::PaintId>)>,
    /// When an observation held back by its observer's `delay` is due, on
    /// the clock: a main frame then computes it (Blink's
    /// `LocalFrameView::ScheduleDelayedIntersection`).
    intersections_due: Option<f64>,
    /// The DOM's resize observer loop reported its error in this
    /// frame: the next frame delivers what it skipped (Chrome's
    /// `ScheduleAnimation` after the error).
    resize_loop_error: bool,
    /// The DOM's transitions and CSS animations run: each frame moves
    /// them on (`update_animations`).
    animations_running: bool,
    /// The URLs of the requests the render thread sent (their ids are
    /// unique per renderer, `loader::next_request_id`, so a response for a
    /// document that has gone is recognised and dropped).
    request_urls: crate::loader::RequestUrls,
    /// The blob URLs the realms registered.
    blob_urls: BlobUrls,
    /// The select whose dropdown the browser shows (Blink's
    /// `ExternalPopupMenu`, one at a time).
    popup: Option<dom::NodeId>,
    /// The select whose dropdown a press just closed: the press doesn't
    /// open it again (Blink's `WebViewImpl::HandleMouseDown`).
    popup_closed_by_press: Option<dom::NodeId>,
    /// The pointer's last move, dispatched at the next main frame (Chrome's
    /// rAF-aligned input), or before the next event that isn't a move.
    pending_mouse_move: Option<MouseMove>,
    /// The cursor last asked of the browser: `SetCursor` goes when it
    /// changes.
    cursor: protocol::renderer::CursorType,
    /// The top-level view's document and pre-paint count when what is
    /// under the pointer was last found: a commit past them finds it again
    /// (`recompute_under_mouse`). None before the pointer was over the
    /// page.
    under_mouse_pre_paints: Option<(DocumentId, u64)>,
    /// Acks owed for the moves coalesced into `pending_mouse_move`: each
    /// goes once the move is dispatched, as Chromium's
    /// `MainThreadEventQueue` acks a rAF-aligned event after it runs, so
    /// the browser (and automation waiting for the ack) knows its
    /// `mousemove` has been seen.
    mouse_move_acks: u32,
    /// The count of the move in `pending_mouse_move`, if the compositor
    /// handed it on: it waits until dispatched.
    pending_move_input: Option<Pending>,
    web_tests: bool,
    /// The browser asked for a frame once the thread is idle
    /// (`RendererMsg::RequestIdleFrame`, web tests' alone).
    idle_frame_wanted: bool,
    /// `RendererFlags::automation`: WebDriver's webdriver-active flag too.
    automation: bool,
    /// `RendererFlags::user_agent`.
    user_agent: Rc<str>,
    /// `RendererFlags::system_colors`.
    system_colors: gfx::SystemColors,
    /// The browser window's rect on the screen, as last told
    /// (`RendererMsg::UpdateScreenRects`).
    window_screen_rect: Option<RectF>,
    /// Whether the page is hidden (its tab isn't the one shown), as last
    /// told (`RendererMsg::WasHidden`, `WasShown`): the visibility state
    /// of every document of the renderer, whose frames are one page's.
    page_hidden: bool,
    /// Whether the page has the system's focus, as last told
    /// (`RendererMsg::SetFocus`), for an engine not made yet.
    page_focused: bool,
    own_history_entries: OwnHistoryEntries,
    /// The parser's scripts that wait for sheets were tried again in the
    /// task that runs (`InternalTask::RetryBlockedScripts`): not posted
    /// again for it.
    retried_blocked_scripts: bool,
    /// When the task for the intersections held back by their `delay` is
    /// posted for (the renderer's clock), if it is.
    /// The input the compositor thread handed on and the main thread
    /// hasn't taken yet (what `isInputPending()` reads).
    pending_input: PendingUserInput,
    /// The event loop's clock: timers count on it.
    clock: Clock,
    /// The redirects of the child frames' navigations in flight, by
    /// request: their navigation timing counts them.
    script_engine: ScriptEngineFactory,
    /// Where each engine made puts what stops its script, for a renderer
    /// of the browser's process.
    terminator: Option<MainThreadTerminator>,
    /// WebDriver's references and the scripts it waits for.
    webdriver_state: webdriver::WebDriverState,
    /// The browser's DevTools sessions.
    devtools_sessions: Vec<String>,
    /// What each of them has on, as last told to the browser.
    devtools_states: Rc<devtools::SessionStates>,
    /// A child frame was made, went or committed a document since
    /// DevTools last looked at the frames: they are looked at again after
    /// the task, the `Page` agent telling of a new frame's initial
    /// document, the `DOM` agent of an iframe's new document. (A commit,
    /// a frame attached or detached, the `Page` agent tells as they
    /// happen.)
    frames_changed: bool,
    /// The main thread's tasks (`MainThreadScheduler`): what the browser,
    /// the compositor and DevTools ask of it, which script paused takes some
    /// of too.
    scheduler: Arc<MainThreadScheduler>,
    /// The frames: each one's document loading or loaded (the top-level
    /// one's once one committed): before the engine, as fields drop in
    /// order and its roots go before their heap.
    render_frames: render_frame::RenderFrames,
    /// DevTools' `DOM` domain (Blink's `InspectorDOMAgent`): before the
    /// engine, as the nodes it keeps are roots of the engine's DOM.
    dom_agent: Rc<dom_agent::DomAgent>,
    /// DevTools' `CSS` domain (Blink's `InspectorCSSAgent`): before the
    /// engine, as the styles it keeps are of the engine's DOM.
    css_agent: Rc<css_agent::CssAgent>,
    /// The `<link>` sheets DevTools' `CSS` fetches again for their texts,
    /// by request: the agent's key of each, and its frame.
    css_refetches: std::collections::HashMap<u64, (usize, dom_api::FrameId)>,
    /// DevTools' `Overlay` domain (Blink's `InspectorOverlayAgent`).
    overlay_agent: Rc<overlay_agent::OverlayAgent>,
    /// The DOM's engine, made at its first document, or at DevTools'
    /// first command for its inspector (`make_engine`).
    engine: Option<Box<dyn dom_api::ScriptEngine>>,
    /// Whether `engine` is the wasm one (the page's document declared its
    /// scripts wasm): a document of the other kind takes another engine.
    engine_is_wasm: bool,
    /// DevTools' `Page` domain, the renderer's part (Blink's
    /// `InspectorPageAgent`).
    page_agent: Rc<page_agent::PageAgent>,
    /// DevTools' `Network` domain, the renderer's part (Blink's
    /// `InspectorNetworkAgent`).
    network_agent: Rc<network_agent::NetworkAgent>,
    /// The page's log and DevTools' `Log` domain (Blink's
    /// `ConsoleMessageStorage` and `InspectorLogAgent`).
    log_agent: Rc<log_agent::LogAgent>,
}

/// The nodes DevTools' `DOM` agent keeps go before the engine's DOM: the
/// agent outlives the thread's own field in the engine's DevTools queue.
impl Drop for RenderThread {
    fn drop(&mut self) {
        // The workers end first, their threads waited for (each lets go
        // of its own engine, on itself).
        self.workers.terminate_all();
        self.dom_agent.dom_gone();
        self.css_agent.dom_gone();
        self.overlay_agent.dom_gone();
    }
}

impl RenderThread {
    #[allow(clippy::too_many_arguments)]
    fn new(
        to_browser: BrowserSender,
        scheduler: Arc<MainThreadScheduler>,
        layer_tree_host: LayerTreeHost<ProxyMain>,
        pending_input: PendingUserInput,
        flags: RendererFlags,
        script_engine: ScriptEngineFactory,
        terminator: Option<MainThreadTerminator>,
        system_fonts: SystemFontSource,
    ) -> Self {
        let _span = trace_event::info_event!("RenderThread::new");
        let id_prefix = webdriver::new_id_prefix();
        let log_agent = Rc::new(log_agent::LogAgent::new(to_browser.clone()));
        let network_agent = Rc::new(network_agent::NetworkAgent::new(
            to_browser.clone(),
            id_prefix,
            log_agent.clone(),
        ));
        let page_agent = Rc::new(page_agent::PageAgent::new(
            to_browser.clone(),
            network_agent.clone(),
        ));
        let dom_agent = Rc::new(dom_agent::DomAgent::new(to_browser.clone()));
        let css_agent = Rc::new(css_agent::CssAgent::new(
            to_browser.clone(),
            dom_agent.clone(),
            page_agent.clone(),
            id_prefix,
        ));
        let overlay_agent = Rc::new(overlay_agent::OverlayAgent::new(
            to_browser.clone(),
            dom_agent.clone(),
        ));
        let blob_urls = BlobUrls::new(to_browser.clone());
        let devtools_states = Rc::new(devtools::SessionStates::new(to_browser.clone()));
        tracing::info!(raster = raster::BACKEND, "renderer main thread up");
        let _ = to_browser.send(RendererHostMsg::RendererReady);
        Self {
            text: match system_fonts {
                _ if flags.test_fonts => TextContext::new(true),
                SystemFontSource::Browser(names) => TextContext::with_font_service(
                    crate::font_data::font_names(names),
                    Box::new(crate::font_data::BrowserFontDataService {
                        to_browser: to_browser.clone(),
                        scheduler: scheduler.clone(),
                    }),
                ),
                SystemFontSource::FindHere => TextContext::new(false),
            },
            canvas_encoder: CanvasEncoder::new(scheduler.clone()),
            workers: Rc::new(crate::worker_thread::Workers::new(
                script_engine.worker_engine,
                dom_api::EngineSettings {
                    web_tests: flags.web_tests,
                },
                crate::worker_thread::Agent {
                    user_agent: flags.user_agent.clone(),
                    webdriver: flags.automation,
                },
                scheduler.clone(),
                to_browser.clone(),
                crate::worker_thread::Owner::Main,
                crate::worker_thread::WorkerLoaders::Documents,
            )),
            to_browser,
            widget: Some(FrameWidget::new(layer_tree_host)),
            visual_properties: VisualProperties::default(),
            last_main_frame: 0.0,
            idle_period_end: None,
            caret_blink_interval: flags.caret_blink_interval.filter(|i| !i.is_zero()),
            parser_budget: flags.parser_budget,
            new_content_rendering_delay: flags.new_content_rendering_delay,
            new_content_due: None,
            empty_frame_wanted: false,
            activation_told: false,
            next_caret_blink: None,
            intersections_computed: None,
            intersections_due: None,
            resize_loop_error: false,
            animations_running: false,
            request_urls: crate::loader::RequestUrls::default(),
            blob_urls,
            popup: None,
            popup_closed_by_press: None,
            pending_mouse_move: None,
            cursor: protocol::renderer::CursorType::Pointer,
            under_mouse_pre_paints: None,
            pending_move_input: None,
            mouse_move_acks: 0,
            idle_frame_wanted: false,
            web_tests: flags.web_tests,
            automation: flags.automation,
            user_agent: Rc::from(flags.user_agent.as_str()),
            system_colors: flags.system_colors,
            window_screen_rect: None,
            page_hidden: false,
            page_focused: false,
            own_history_entries: OwnHistoryEntries::default(),
            retried_blocked_scripts: false,
            pending_input,
            clock: Clock::new(),
            script_engine,
            terminator,
            webdriver_state: webdriver::WebDriverState::new(id_prefix),
            devtools_sessions: Vec::new(),
            devtools_states,
            frames_changed: false,
            page_agent,
            network_agent,
            log_agent,
            dom_agent,
            css_agent,
            css_refetches: std::collections::HashMap::new(),
            overlay_agent,
            scheduler,
            engine: None,
            engine_is_wasm: false,
            render_frames: render_frame::RenderFrames::default(),
        }
    }

    /// The event loop: block for a task, run it and every task already
    /// queued, run the engine's platform tasks (incremental marking, GC
    /// finalization, sweeping; and a checkpoint after them), ask for a main
    /// frame if the DOM changed, then run the "update the rendering" step
    /// if anything asked for a frame.
    ///
    /// Frames are paced by Viz through the compositor thread: the loop
    /// tells it whether a main frame is needed, and runs the rendering
    /// update when it asks for one (`begin_main_frame`).
    fn run(&mut self) {
        loop {
            #[cfg(debug_assertions)]
            self.assert_no_dom_task_lost();
            // Wait for the next task, or until an idle period's callback
            // may run: idle, for DevTools' profile of the script (as Blink
            // tells V8's inspector around each task).
            self.set_idle(true);
            let wait = self.until_idle();
            let task = self.scheduler.next(TaskQueues::ALL, wait);
            self.set_idle(false);
            match task {
                Some(task) => {
                    // Chrome's top-level slice for a task, in its traces.
                    let _task = trace_event::trace_event!("ThreadControllerImpl::RunTask");
                    if !self.run_task(task) {
                        break;
                    }
                }
                // None waits: an idle period's callback (Blink's scheduler
                // runs idle tasks when it has nothing else to).
                None => trace_event::in_scope(
                    trace_event::trace_event!("RenderThread::run_idle_callback"),
                    || self.run_idle_callback(),
                ),
            }
            self.after_task();
            // Whether the task wants a main frame, to the compositor as it
            // ends (Blink's `SetNeedsAnimate` as it is asked for), a task
            // waiting or not: a page that keeps queuing tasks still has its
            // frames.
            let needs = self.update_begin_main_frame();
            if !needs && self.idle_frame_wanted && !self.scheduler.has_task(TaskQueues::ALL) {
                self.idle_frame_wanted = false;
                self.request_idle_frame();
            }
        }
        tracing::debug!("renderer main thread exiting");
    }

    /// What a task leaves for when it ends (Chromium's task observers,
    /// `DidProcessTask`): the loads it asked for that the engine hasn't
    /// sent yet (a parser's scripts, the backgrounds a style update
    /// found), then the blob URLs it revoked, after those loads (each got
    /// its blob); then what DevTools is to hear of.
    /// The renderer's own work, a task of its own (`InternalTask`): what
    /// each does checks whether it is still to be done.
    fn run_internal(&mut self, task: InternalTask) {
        match task {
            InternalTask::ChildCommit(frame) => self.run_child_commit(frame),
            InternalTask::RetryBlockedScripts => {
                self.retry_scripts_blocked_by_sheets();
                // Not posted again for itself (`after_task`).
                self.retried_blocked_scripts = true;
            }
            InternalTask::CaretBlink => self.blink_caret(),
            InternalTask::Intersections => self.intersections_due_now(),
            InternalTask::NavigationCancellationWindowEnded(id) => {
                let _ = self
                    .to_browser
                    .send(RendererHostMsg::NavigationCancellationWindowEnded { ids: vec![id] });
            }
            InternalTask::NewContentTimeout => self.new_content_rendering_timeout(),
            // The engine's platform tasks, with a checkpoint after them
            // (Blink runs Oilpan's and V8's on the main thread's scheduler):
            // they may run script (FinalizationRegistry cleanup), and they
            // are what finalizes garbage.
            InternalTask::NetworkIdle => self.check_network_idle(),
            InternalTask::Guests => self.run_due_guests(),
            InternalTask::PlatformTasks => {
                self.scheduler.platform_tasks_taken();
                trace_event::in_scope(
                    trace_event::trace_event!("RenderThread::run_platform_tasks"),
                    || self.run_platform_tasks(),
                );
            }
        }
    }

    fn after_task(&mut self) {
        // What its script asked of the frame (a dropdown; the frames its
        // engine calls made and let go), now that none runs.
        self.handle_popup_requests();
        self.handle_frame_requests();
        self.start_image_fetches();
        self.start_script_fetches();
        // Its `toBlob` calls, to the encoder.
        self.start_canvas_encodes();
        self.send_webdriver_results();
        self.log_canvas_raster_failure();
        self.update_title();
        self.blob_urls.flush_revoked();
        // The parser's scripts that wait for a sheet that blocks scripts,
        // tried again after it: it may have removed the sheet's `<link>`,
        // or made it no stylesheet, so they run without its response.
        if !std::mem::take(&mut self.retried_blocked_scripts) && self.scripts_wait_for_sheets() {
            self.scheduler
                .post_internal(InternalTask::RetryBlockedScripts);
        }
        // The frames whose network went quiet, to DevTools; and a task for
        // when the next may have.
        self.check_network_idle();
        self.post_network_idle();
        // A task for the engine's guests' next wait to be over.
        self.post_guests();
        self.tell_devtools_after_task();
        // The sheets DevTools' `CSS` wants the texts of, asked for (those a
        // lifecycle update found it wants; a command's are asked for in its
        // task).
        self.start_css_refetches();
        // What DevTools' overlay shows follows its node at each frame,
        // and draws once it changes.
        if self.overlay_agent.showing()
            || self.overlay_agent.version() != self.widget().overlay_version
        {
            self.request_main_frame();
        }
        // A DOM change asks for a frame itself, as Blink's do
        // (`Document::ScheduleLayoutTreeUpdate`), whichever task made it.
        self.request_next_frame_if_dom_changed();
        // The loads the task asked for have started: the stacks of the
        // script that asked, and that no load took, go.
        self.clear_request_stacks();
    }

    /// A task of the DOM's due a while ago with nothing in the scheduler
    /// for it never runs (its token was lost: each task is run by its own,
    /// `run_dom_task`), and nothing else says so: debug builds stop there,
    /// as the thread is to wait. Due 5 ms ago, past the clocks' rounding.
    #[cfg(debug_assertions)]
    fn assert_no_dom_task_lost(&mut self) {
        if self.scheduler.has_task(TaskQueues::ALL) {
            return;
        }
        let lost = self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.has_dom_task_due_for(5.0));
        assert!(
            !lost,
            "a task of the DOM's is due with nothing in the scheduler to run it"
        );
    }

    /// The task that looks at the documents' network as the first may
    /// have been quiet long enough, unless one is posted for then or
    /// sooner (Blink's `IdlenessDetector` has a timer of its own).
    fn post_network_idle(&mut self) {
        let Some(due) = self.next_idleness_due() else {
            return;
        };
        let wait = due.saturating_since(ipc::TimeTicks::now());
        if let Some(at) = Instant::now().checked_add(wait) {
            self.scheduler
                .post_internal_once_at(InternalTask::NetworkIdle, at);
        }
    }

    /// The task that runs the engine's guests as the first's wait is over,
    /// unless one is posted for then or sooner.
    fn post_guests(&mut self) {
        let Some(wait) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.until_due_guests())
        else {
            return;
        };
        if let Some(at) = Instant::now().checked_add(wait) {
            self.scheduler
                .post_internal_once_at(InternalTask::Guests, at);
        }
    }

    /// Whether the compositor is to send a main frame (Blink's
    /// `SetNeedsAnimate`, as each asks): one was asked for, an animation
    /// frame callback waits, a resize loop error is to be reported,
    /// transitions and animations run, or a mouse move waits for the frame.
    fn update_begin_main_frame(&mut self) -> bool {
        let animation_frames = self.wants_animation_frame();
        // Running transitions and CSS animations move on at each frame
        // (script played or canceled one).
        let animations = self.animations_want_frames();
        let needs = self.widget().needs_begin_main_frame
            || animation_frames
            || self.resize_loop_error
            || animations
            || self.pending_mouse_move.is_some();
        self.widget()
            .layer_tree_host
            .set_needs_begin_main_frame(needs);
        needs
    }

    /// `RendererMsg::RequestIdleFrame`'s, now the thread has no task and
    /// wants no frame: a main frame, answered once it has gone to Viz.
    fn request_idle_frame(&mut self) {
        let to_browser = self.to_browser.clone();
        self.request_main_frame();
        self.widget()
            .pending_visual_states
            .push(SwapPromise::new(move |frame_token| {
                let _ = to_browser.send(RendererHostMsg::IdleFrameReady { frame_token });
            }));
        self.update_begin_main_frame();
    }

    /// What the dedicated worker numbered `id` sent its `Worker`, given to
    /// the engine (a message a task of the `Worker`'s realm, an error an
    /// event at it now, script run: what it reported sent, as after the
    /// platform's tasks); one that is no more, forgotten.
    fn worker_event(&mut self, id: u64, event: crate::worker_thread::WorkerEvent) {
        use crate::worker_thread::WorkerEvent;
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        match event {
            WorkerEvent::Message(message) => engine.worker_message(id, message),
            WorkerEvent::Error(error) => {
                engine.worker_error(id, &error);
                self.send_harness_results();
            }
            WorkerEvent::Closed => {
                engine.worker_closed(id);
                self.workers.forget(id);
            }
            WorkerEvent::ScriptFailed => {
                engine.worker_script_failed(id);
                self.workers.forget(id);
                self.send_harness_results();
            }
        }
    }

    /// Returns false on shutdown.
    fn run_task(&mut self, task: MainThreadTask) -> bool {
        match task {
            MainThreadTask::Browser(msg) => self.handle_message(msg),
            MainThreadTask::CanvasEncoded { id, file } => {
                self.canvas_encoded(id, file);
                true
            }
            MainThreadTask::FetchFailed { fetch } => {
                self.fetch_went_nowhere(fetch);
                true
            }
            MainThreadTask::Worker { id, event } => {
                self.worker_event(id, event);
                true
            }
            // Each a task of the DOM's, numbered now (a token each).
            MainThreadTask::PortMessages => {
                if let Some(engine) = &mut self.engine {
                    engine.port_messages_arrived();
                }
                true
            }
            MainThreadTask::DomTask { number } => {
                self.run_dom_task(number);
                true
            }
            MainThreadTask::Internal(task) => {
                self.run_internal(task);
                true
            }
            MainThreadTask::NavigationBody {
                frame,
                navigation_id,
            } => {
                self.read_body_of(frame, navigation_id);
                true
            }
            MainThreadTask::DevTools(queued) => {
                self.run_devtools_task(queued);
                true
            }
            MainThreadTask::Compositor(task) => {
                match task {
                    ToMainThread::BeginMainFrame { args, scroll } => {
                        self.apply_compositor_scroll(scroll);
                        self.begin_main_frame(args);
                    }
                    ToMainThread::Input {
                        event,
                        attribution,
                        document,
                        scroll,
                        dispatch,
                    } => {
                        // Taken to handle (Blink's
                        // `WillHandleInputEventOnMainThread`); a move once
                        // dispatched, the one it replaces now.
                        let pending = Pending::of(&event, attribution, document);
                        let counts = &self.pending_input;
                        if matches!(event, InputEvent::MouseMove { .. }) {
                            counts
                                .dequeue(std::mem::replace(&mut self.pending_move_input, pending));
                        } else {
                            counts.dequeue(pending);
                        }
                        self.apply_compositor_scroll(scroll);
                        match dispatch {
                            InputDispatch::Blocking { cancelable } => {
                                self.handle_forwarded_input(event, document, cancelable);
                            }
                            InputDispatch::NonBlocking => self.dispatch_wheel_only(event, document),
                        }
                        self.tell_user_activation();
                    }
                    ToMainThread::RasterFailed { message, count } => self.log_raster_failure(
                        log_agent::RasterFailed::Tile,
                        &raster::RasterFailure { message, count },
                    ),
                }
                true
            }
        }
    }

    /// `LayerTreeHost::ApplyCompositorChanges`: what the compositor
    /// scrolled the viewport and the containers that are layers by since it
    /// last said, applied first thing in the task, before anything else
    /// looks at the offsets (animation frames, hit testing, the ack's count
    /// of main frame requests) or decides there is nothing to update; only
    /// to the document it was scrolled in. It asks for no main frame of its
    /// own: the compositor already shows it, clamped to the same nodes, and
    /// a main frame with nothing else to update is aborted, which leaves it
    /// shown (cc's `kFinishedNoUpdates`). Unless a container is no longer a
    /// layer: that one is painted where it scrolled to.
    fn apply_compositor_scroll(&mut self, scroll: Option<CompositorScrollDelta>) {
        if let Some(scroll) = &scroll {
            self.apply_document_compositor_scroll(scroll);
        }
    }

    /// The widget: the thread's until the first document's frame takes it,
    /// the frame's from then on (the engine keeps the frame from then on,
    /// and lends it out only within its own calls).
    fn widget(&mut self) -> &mut FrameWidget {
        if let Some(widget) = &mut self.widget {
            return widget;
        }
        &mut documents::engine_frame(&mut self.engine)
            .expect("the frame has the widget once the thread hasn't")
            .1
            .widget
    }

    /// `ScheduleAnimation`: the rendering needs updating, at the next main
    /// frame.
    fn request_main_frame(&mut self) {
        self.widget().schedule_frame();
    }

    /// A frame's new document's navigation timing, from what the browser
    /// told of its load (`loaded`, and whether its iframe's referrer policy
    /// was `no-referrer` as it asked for it); its network's times marked in
    /// the trace, as Blink's `DocumentLoadTiming` marks them.
    fn commit_timing(
        &self,
        url: &str,
        mime_type: &str,
        loaded: Option<(&protocol::renderer::NavigationLoad, bool)>,
        navigation_type: protocol::renderer::FrameLoadType,
        time_origin: f64,
    ) -> NavigationTiming {
        use protocol::renderer::FrameLoadType;
        let url = Url::parse(url).unwrap_or_else(|_| Url::parse("about:blank").unwrap());
        let load = loaded.map(
            |(loaded, no_referrer)| crate::navigation_timing::DocumentLoad {
                url: &url,
                timing: loaded.timing,
                // The body's end and sizes come once it has all come
                // (`body_ended`).
                response_end: None,
                redirects: &loaded.redirects,
                no_referrer,
                protocol: &loaded.protocol,
                status: loaded.status,
                mime_type,
                content_encoding: loaded.content_encoding.as_deref(),
                encoded_body_size: 0,
                decoded_body_size: 0,
            },
        );
        let navigation_type = match navigation_type {
            FrameLoadType::Standard => NavigationType::Navigate,
            FrameLoadType::Reload | FrameLoadType::ReloadBypassingCache => NavigationType::Reload,
            FrameLoadType::BackForward => NavigationType::BackForward,
        };
        let clock = self.clock;
        let at =
            |time: ipc::TimeTicks| clock.before_now(ipc::TimeTicks::now().saturating_since(time));
        let timing = crate::navigation_timing::network_timing(
            load,
            navigation_type,
            time_origin,
            at,
            crate::navigation_timing::randomized_confidence(),
        );
        let now = clock.now();
        for (name, time) in [
            ("navigationStart", Some(time_origin)),
            ("fetchStart", timing.fetch_start),
            ("responseEnd", timing.response_end),
            ("commitNavigationEnd", Some(now)),
        ] {
            if let Some(time) = time {
                tracing::trace!(
                    target: "blink.user_timing",
                    mark = name,
                    ago_ns = base::numerics::saturated_cast::<i64, _>((now - time) * 1e6)
                );
            }
        }
        timing
    }

    /// Commits `commit`'s document (`RenderFrameImpl::CommitNavigation`):
    /// it is parsed as its `body` comes.
    fn commit_navigation_body(
        &mut self,
        commit: document_body::NavigationCommit,
        start: bytes::Bytes,
        pipe: Option<ipc::data_pipe::Consumer>,
    ) {
        let document_body::NavigationCommit {
            frame,
            loader,
            navigation_id,
            begin_id,
            url,
            error_page,
            mime_type,
            charset,
            content_language,
            history,
            navigation_start,
            loaded,
            navigation_type,
            paint_holding,
        } = commit;
        let time_origin = self
            .clock
            .before_now(ipc::TimeTicks::now().saturating_since(navigation_start));
        // Its subresources bypass the HTTP cache too, until its `load`.
        let bypass_cache =
            navigation_type == protocol::renderer::FrameLoadType::ReloadBypassingCache;
        // An error page's is a URL of no origin's, the browser still
        // hearing of the one that failed.
        let document_url = if error_page {
            protocol::renderer::ERROR_PAGE_URL.to_owned()
        } else {
            url.clone()
        };
        let main_frame = Some(frame) == self.render_frames.top_token();
        let no_referrer = !main_frame && self.child_no_referrer_of(frame, begin_id);
        let timing = self.commit_timing(
            &document_url,
            &mime_type,
            loaded.as_deref().map(|loaded| (loaded, no_referrer)),
            navigation_type,
            time_origin,
        );
        // The log names the document by it.
        let status = loaded
            .as_ref()
            .map(|loaded| (loaded.status, loaded.status_text.clone()));
        let body = document_body::NavigationBody {
            start,
            pipe,
            loaded,
            url: url.clone(),
        };
        let loaded_url = url.clone();
        let committed = if main_frame {
            let origin_source = begin_id
                .zip(self.render_frames.top())
                .zip(self.engine.as_mut())
                .and_then(|((id, top), engine)| engine.take_navigation(top, Some(id)))
                .and_then(|asked| asked.origin_source);
            self.commit_navigation(
                navigation_id,
                document_url,
                url,
                origin_source,
                &mime_type,
                charset.as_deref(),
                content_language.as_deref(),
                body,
                &history,
                time_origin,
                timing,
                loader,
                bypass_cache,
            )
        } else {
            let Ok(document_url) = Url::parse(&document_url) else {
                return;
            };
            self.commit_child_navigation(
                frame,
                begin_id,
                documents::ChildDocument {
                    url: document_url,
                    mime_type: &mime_type,
                    charset: charset.as_deref(),
                    body,
                },
                documents::ChildCommit {
                    time_origin,
                    timing,
                    content_language: content_language.as_deref(),
                    navigation_id,
                    token: frame,
                    url,
                    origin_source: None,
                    history,
                    loader,
                },
            )
        };
        if committed && main_frame && !paint_holding {
            self.end_paint_holding();
        }
        if committed
            && let Some((status, status_text)) = status
            && status >= 400
        {
            self.log_agent.add(log_agent::LogEntry::network(
                status_message(status, &status_text),
                loaded_url,
                navigation_id.to_string(),
            ));
        }
    }

    /// Returns false on shutdown.
    fn handle_message(&mut self, msg: RendererMsg) -> bool {
        match msg {
            RendererMsg::CommitNavigation {
                frame,
                loader,
                navigation_id,
                begin_id,
                url,
                error_page,
                mime_type,
                charset,
                content_language,
                body,
                body_pipe,
                history,
                navigation_start,
                loaded,
                navigation_type,
                paint_holding,
            } => {
                let commit = document_body::NavigationCommit {
                    frame,
                    loader,
                    navigation_id,
                    begin_id,
                    url,
                    error_page,
                    mime_type,
                    charset,
                    content_language,
                    history,
                    navigation_start,
                    loaded,
                    navigation_type,
                    paint_holding,
                };
                self.commit_navigation_body(commit, body.into_bytes(), body_pipe);
            }
            RendererMsg::NavigationLoadCompleted {
                frame,
                navigation_id,
                status,
            } => self.document_load_completed(frame, navigation_id, status),
            RendererMsg::NavigationDropped { frame, id } => self.navigation_dropped(frame, id),
            RendererMsg::CommitSameDocumentNavigation {
                frame,
                navigation_id,
                document,
                url,
                history,
            } => {
                self.commit_same_document_traversal(frame, navigation_id, document, &url, history);
            }
            RendererMsg::SetHistoryOffsetAndLength {
                index,
                length,
                entries_heard,
            } => {
                // The new entries script made were counted as they went
                // (`OwnHistoryEntries`), among those it hadn't heard of.
                let history = self.own_history_entries.place(index, length, entries_heard);
                if let Some(engine) = &mut self.engine {
                    engine.set_session_history(history);
                }
            }
            RendererMsg::ReceivedRedirect {
                request_id,
                worker: None,
                redirect,
            } => self.did_receive_redirect(request_id, &redirect),
            RendererMsg::ResourceResponse {
                request_id,
                worker: None,
                result,
            } => self.did_receive_resource(request_id, result),
            // A worker's go to its thread (`RendererSender::send`).
            RendererMsg::ReceivedRedirect {
                worker: Some(_), ..
            }
            | RendererMsg::ResourceResponse {
                worker: Some(_), ..
            }
            | RendererMsg::DedicatedWorkerScript { .. } => {}
            RendererMsg::UpdateVisualProperties(props) => {
                self.visual_properties = props;
                self.set_viewport();
                self.widget().layer_tree_host.set_viewport_rect_and_scale(
                    props.viewport_size,
                    props.device_scale_factor,
                    props.local_surface_id,
                );
                self.request_main_frame();
            }
            RendererMsg::UpdateScreenRects { window_screen_rect } => {
                self.window_screen_rect = Some(window_screen_rect);
                self.set_viewport();
            }
            RendererMsg::QueryElement {
                query_id,
                frames,
                selector,
            } => self.query_element(query_id, &frames, &selector),
            RendererMsg::ScrollIntoViewIfNeeded { frames, selector } => {
                self.scroll_into_view_if_needed(&frames, &selector)
            }
            RendererMsg::RequestVisualState { request_id } => self.request_visual_state(request_id),
            RendererMsg::RequestIdleFrame if self.web_tests => self.idle_frame_wanted = true,
            RendererMsg::RequestIdleFrame => {
                tracing::warn!("RequestIdleFrame without RendererFlags::web_tests; ignored");
            }
            RendererMsg::WebDriver {
                request_id,
                frame,
                command,
            } => self.webdriver(request_id, frame.as_deref(), command),
            RendererMsg::DevToolsAttach { .. }
            | RendererMsg::DevToolsDetach { .. }
            | RendererMsg::DevToolsMessage { .. } => {
                unreachable!("RendererSender puts DevTools' in the DevTools queue")
            }
            RendererMsg::DevToolsMainFrameToken(token) => {
                self.render_frames.set_top_token(token);
                // The realms made from now on are told by it.
                if let Some(engine) = &mut self.engine {
                    engine.set_main_frame_token(token);
                }
            }
            RendererMsg::WasHidden => self.set_page_visible(false),
            RendererMsg::WasShown => self.set_page_visible(true),
            RendererMsg::SetFocus(focused) => self.set_focus(focused),
            RendererMsg::SetInitialFocus { reverse } => self.set_initial_focus(reverse),
            RendererMsg::PopupMenuClosed(result) => {
                if let Some(select) = self.popup.take() {
                    self.close_popup(select, result);
                    self.handle_popup_requests();
                    self.request_next_frame_if_dom_changed();
                    self.send_harness_results();
                }
            }
            // The sender puts it aside for the call waiting for it.
            RendererMsg::StorageAreaOpened { .. }
            | RendererMsg::Cookies { .. }
            | RendererMsg::CookiesEnabled { .. }
            | RendererMsg::FontFamily { .. }
            | RendererMsg::FontFallback { .. }
            | RendererMsg::FontFiles { .. } => {}
            RendererMsg::StorageAreaChangeFailed { area, key, value } => {
                if let Some(engine) = self.engine.as_mut() {
                    engine.storage_change_failed(area, &key, value);
                }
            }
            RendererMsg::StorageAreaChanged {
                area,
                change,
                old_value,
                changed,
                url,
                source,
            } => {
                if let Some(engine) = self.engine.as_mut() {
                    let mutation = match change {
                        protocol::renderer::StorageChange::Set { key, value } => {
                            dom::StorageMutation::Set { key, value }
                        }
                        protocol::renderer::StorageChange::Remove { key } => {
                            dom::StorageMutation::Remove { key }
                        }
                        protocol::renderer::StorageChange::Clear => dom::StorageMutation::Clear,
                    };
                    engine.storage_area_changed(area, mutation, old_value, changed, url, source);
                }
            }
            RendererMsg::Shutdown => return false,
            RendererMsg::Unload => {
                // Its document goes as a navigation's would take it, the
                // documents of its frames after it; the renderer with it.
                if let Some(engine) = self.engine.as_mut() {
                    let top = engine.top_frame();
                    engine.unload_document(top);
                }
                return false;
            }
            // A process's IO thread acts on it before it gets here; threads
            // of the browser's can't crash without taking the browser
            // along.
            RendererMsg::Crash => {
                tracing::warn!("asked to crash, as the browser's threads: ignored");
            }
        }
        true
    }

    /// `RenderWidget::WasHidden` and `WasShown`, then Blink's
    /// `Page::SetVisibilityState`. Hidden, no frame is drawn until it is
    /// shown again, so the move waiting for one is dispatched now, and the
    /// input acks waiting for one go without one (the compositor answers
    /// those it already has). If the page's visibility changed, its
    /// documents run HTML's "update the visibility state"
    /// (`ScriptEngine::set_page_visible`). The browser tells a tab it is hidden
    /// before it commits anything (it opens a background tab hidden), so
    /// a document starts with its page's state.
    fn set_page_visible(&mut self, visible: bool) {
        self.widget().layer_tree_host.set_visible(visible);
        if !visible {
            self.dispatch_mouse_move();
            for ack in self.widget().pending_input_acks.drain(..) {
                ack.did_not_swap();
            }
        }
        if self.page_hidden == !visible {
            return;
        }
        self.page_hidden = !visible;
        if let Some(engine) = &mut self.engine {
            engine.set_page_visible(visible);
            self.request_next_frame_if_dom_changed();
        }
    }

    /// `RendererMsg::QueryElement`, with `RendererFlags::automation` only.
    fn query_element(&mut self, query_id: u64, frames: &[String], selector: &str) {
        if !self.automation {
            tracing::warn!("QueryElement without RendererFlags::automation; ignored");
            return;
        }
        self.run_query_element(query_id, frames, selector);
    }

    /// `RendererMsg::ScrollIntoViewIfNeeded`, with
    /// `RendererFlags::automation` only: a main frame scrolls.
    fn scroll_into_view_if_needed(&mut self, frames: &[String], selector: &str) {
        if !self.automation {
            tracing::warn!("ScrollIntoViewIfNeeded without RendererFlags::automation; ignored");
            return;
        }
        self.run_scroll_into_view_if_needed(frames, selector);
    }

    /// `RendererMsg::RequestVisualState`, with `RendererFlags::automation`
    /// only (`RenderFrameImpl::VisualStateRequest`): asks for a main frame,
    /// and answers once its frame is swapped, so that every frame before it
    /// is in Viz's queue ahead of what the browser asks of Viz next (a
    /// copy, for a screenshot). Answering at once when no main frame was
    /// wanted counted on the last commit's frame being in Viz already, but
    /// the compositor may not have drawn it yet: under load, a screenshot
    /// then showed the page before its last change (a reftest that ends in
    /// the task that makes its last change failed about one run in three
    /// with 32 at once). Chromium's queues a swap promise and asks for a
    /// commit too.
    fn request_visual_state(&mut self, request_id: u64) {
        let Some(promise) = visual_state_promise(&self.to_browser, self.automation, request_id)
        else {
            return;
        };
        self.request_main_frame();
        self.widget().pending_visual_states.push(promise);
    }

    /// `RenderFrameImpl::CommitNavigation`: decode and parse the body into
    /// a new document at `url`, which replaces the old one, then ask for
    /// its subresources; the browser hears it committed `reported_url`
    /// (an error page's, the URL that failed). A commit older than the
    /// document shown is dropped (false): the browser starts navigations
    /// in order, but their responses arrive in any order.
    #[allow(clippy::too_many_arguments)]
    fn commit_navigation(
        &mut self,
        navigation_id: u64,
        url: String,
        reported_url: String,
        origin_source: Option<dom::NodeRoot>,
        mime_type: &str,
        charset: Option<&str>,
        content_language: Option<&str>,
        body: document_body::NavigationBody,
        history: &HistoryEntryParams,
        time_origin: f64,
        timing: NavigationTiming,
        loader: u64,
        bypass_cache: bool,
    ) -> bool {
        if self
            .render_frames
            .top_document()
            .is_some_and(|next| next.navigation_id() > navigation_id)
        {
            tracing::debug!(%url, navigation_id, "dropping a stale commit");
            return false;
        }
        // The bodies DevTools kept and the messages logged are the old
        // document's.
        self.network_agent.main_frame_navigated();
        self.log_agent.main_frame_navigated();
        self.commit_document_navigation(
            navigation_id,
            url,
            reported_url,
            origin_source,
            mime_type,
            charset,
            content_language,
            body,
            history,
            time_origin,
            &timing,
            loader,
            bypass_cache,
        );
        true
    }

    /// How long until an idle period has a callback to run, or can start
    /// for one, or the first intersection observation held back by its
    /// `delay` is due, or an iframe's `load` for a navigation within a
    /// cross-origin document (zero if one is), or `None` if none.
    fn until_idle(&mut self) -> Option<Duration> {
        let now = self.clock.now();
        let due = self.idle_wake()?;
        Some(Duration::from_secs_f64((due - now).max(0.0) / 1000.0))
    }

    /// When the loop has idle callbacks to run: now if the idle period
    /// running has one it may run, or if one can start for those waiting;
    /// at the end of the one running, after which another can start;
    /// `None` while they wait for a frame (or there are none).
    fn idle_wake(&mut self) -> Option<f64> {
        let now = self.clock.now();
        let (waiting, runnable) = self.idle_callbacks();
        if !waiting {
            return None;
        }
        match self.current_idle_period_end(now) {
            Some(_) if runnable => Some(now),
            Some(end) => Some(end),
            None => self.idle_period_deadline().map(|_| now),
        }
    }

    /// When an idle period starting now would end (Blink's
    /// `IdleHelper::StartIdlePeriod`): while frames are drawn, at the next
    /// frame (`None` if that has come); with nothing to draw, a long idle
    /// period, up to 50 ms, ending sooner if a timer or task is due.
    fn idle_period_deadline(&mut self) -> Option<f64> {
        let now = self.clock.now();
        if let Some(engine_frame) = self.next_frame_due() {
            return (now < engine_frame).then_some(engine_frame);
        }
        // One due now ends it before it starts.
        let next = self
            .engine
            .as_mut()
            .and_then(|engine| engine.until_next_timer())
            .map(|wait| now + wait.as_secs_f64() * 1000.0);
        let end = sooner_ms(Some(now + LONG_IDLE_PERIOD), next)?;
        (now < end).then_some(end)
    }

    /// When the idle period running ends, if it hasn't by `now`: one that
    /// started with nothing to draw ends at the next frame once frames are
    /// drawn (Blink ends a long idle period as a frame is asked for).
    fn current_idle_period_end(&mut self, now: f64) -> Option<f64> {
        let frame = self.next_frame_due();
        self.idle_period_end
            .map(|end| frame.map_or(end, |frame| end.min(frame)))
            .filter(|&end| now < end)
    }

    /// When the next frame is due, while frames are drawn (one asked for, a
    /// mouse move to dispatch, animation frame callbacks waiting).
    fn next_frame_due(&mut self) -> Option<f64> {
        let drawing = self.widget().needs_begin_main_frame
            || self.pending_mouse_move.is_some()
            || self.wants_animation_frame();
        drawing.then_some(self.last_main_frame + FRAME_INTERVAL)
    }

    /// One callback of the idle period running, as a task of its own, if
    /// one may run (starting a period for those waiting if one can):
    /// HTML's "start an idle period" and "invoke idle callbacks", which
    /// runs one at a time, other tasks between.
    fn run_idle_callback(&mut self) {
        let now = self.clock.now();
        if !self.idle_callbacks().0 {
            return;
        }
        let deadline = match self.current_idle_period_end(now) {
            Some(end) => end,
            None => {
                let Some(end) = self.idle_period_deadline() else {
                    self.idle_period_end = None;
                    return;
                };
                self.start_idle_period();
                self.idle_period_end = Some(end);
                end
            }
        };
        self.run_document_idle_callback(deadline);
    }

    /// A main frame for the observations held back by their `delay`, once
    /// the first is due.
    fn intersections_due_now(&mut self) {
        let Some(due) = self.intersections_due else {
            return;
        };
        let now = self.clock.now();
        if due > now {
            // Taken early, by the clocks' rounding: when it is.
            self.post_intersections(due);
            return;
        }
        self.intersections_due = None;
        self.request_main_frame();
    }

    /// The task that asks for a frame for the observations held back by
    /// their `delay`, posted for `due` (the renderer's clock) unless one is
    /// for then or sooner.
    pub(super) fn post_intersections(&mut self, due: f64) {
        if let Some(at) = self.clock.instant_at(due) {
            self.scheduler
                .post_internal_once_at(InternalTask::Intersections, at);
        }
    }

    /// The main frame's new document still can't be drawn when its delay
    /// is up (`RenderWidgetHostImpl::ClearDisplayedGraphics`): the next main
    /// frame shows it empty, in place of the page before.
    fn new_content_rendering_timeout(&mut self) {
        let Some(due) = self.new_content_due else {
            return;
        };
        // One posted for a document before (its own is posted as it
        // commits).
        if Instant::now() < due {
            return;
        }
        self.new_content_due = None;
        if self.render_blocked() {
            self.empty_frame_wanted = true;
            self.request_main_frame();
        }
    }

    /// The main frame's new document isn't to show over the page before
    /// (the browser said: one of another origin the user didn't act on,
    /// Chromium's `ForceFirstFrameAfterNavigationTimeout`): its delay is up
    /// now, and if it can't be drawn yet it shows empty.
    fn end_paint_holding(&mut self) {
        let now = Instant::now();
        self.new_content_due = Some(now);
        self.scheduler
            .post_internal_at(InternalTask::NewContentTimeout, now);
    }

    /// The main frame's document committed: the page before stays shown
    /// till it is drawn, for its delay at most.
    fn start_new_content_rendering_timeout(&mut self) {
        self.empty_frame_wanted = false;
        self.new_content_due = Instant::now().checked_add(self.new_content_rendering_delay);
        if let Some(due) = self.new_content_due {
            self.scheduler
                .post_internal_at(InternalTask::NewContentTimeout, due);
        }
    }

    /// The caret shows or hides if it is time (Blink's `FrameCaret::
    /// CaretBlinkTimerFired`): its block paints again in a main frame.
    fn blink_caret(&mut self) {
        let (Some(due), Some(interval)) = (self.next_caret_blink, self.caret_blink_interval) else {
            return;
        };
        let now = Instant::now();
        // One posted for the caret before it came again (its own is
        // posted as it comes).
        if now < due {
            return;
        }
        let Some((_, frame)) = documents::engine_frame(&mut self.engine) else {
            self.next_caret_blink = None;
            return;
        };
        frame.view.blink_caret();

        // Each phase as long, however late this one came.
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the system's blink interval (a second or so) after an instant near now"
        )]
        let next = (due + interval).max(now);
        self.next_caret_blink = Some(next);
        self.scheduler
            .post_internal_at(InternalTask::CaretBlink, next);
        self.request_main_frame();
    }

    /// `RendererMsg::SetFocus` (Blink's `WebFrameWidgetImpl::SetFocus`):
    /// the page took or lost the system's focus; the focus events go to
    /// the focused frame (`ScriptEngine::set_page_focused`).
    fn set_focus(&mut self, focused: bool) {
        self.set_page_focused(focused);
    }

    /// `RendererMsg::SetInitialFocus`: Tab from the toolbar into the page.
    fn set_initial_focus(&mut self, reverse: bool) {
        self.set_document_initial_focus(reverse);
    }

    /// Input the compositor passed on (`MainThreadEventQueue`), aimed at
    /// `document`: the one in the last frame drawn, which is what the user
    /// saw. Until the frame of a new document is drawn, input still goes to
    /// the old one, which has gone, so it is dropped; a mouse move is not,
    /// as it only sets the cursor. The ack waits for the next frame if
    /// handling asked for one (script run by an event always does, so a
    /// `requestAnimationFrame` in a listener is covered), and goes at once
    /// otherwise, or while the tab is hidden and draws no frame.
    fn handle_forwarded_input(
        &mut self,
        event: InputEvent,
        document: Option<DocumentId>,
        cancelable: bool,
    ) {
        let current = self.frame_document_id();
        let stale = document.is_none() || document != current;
        if stale && !matches!(event, InputEvent::MouseMove { .. }) {
            // The move waiting for the frame came before it.
            self.dispatch_mouse_move();
            tracing::debug!(
                ?event,
                ?document,
                ?current,
                "dropping input aimed at another document"
            );
            let _ = self
                .to_browser
                .send(RendererHostMsg::InputEventAck { frame_token: None });
            return;
        }
        let requests = self.widget().main_frame_requests;
        let is_move = matches!(event, InputEvent::MouseMove { .. });
        self.handle_input_event(event, cancelable);
        if is_move && self.pending_mouse_move.is_some() {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "one per mouse move waiting for the next main frame: far fewer than u32::MAX"
            )]
            {
                self.mouse_move_acks += 1;
            }
            return;
        }
        // A dropdown the input opened or closed goes first, before the
        // browser hears the input is done. A navigation it asked for (a
        // link it followed, by a key too) went as it was asked, and one to
        // a fragment asked for the frame the ack then waits for.
        self.handle_popup_requests();
        self.popup_closed_by_press = None;
        if self.widget().main_frame_requests == requests
            || !self.widget().layer_tree_host.is_visible()
        {
            let _ = self
                .to_browser
                .send(RendererHostMsg::InputEventAck { frame_token: None });
            return;
        }
        let to_browser = self.to_browser.clone();
        self.widget()
            .pending_input_acks
            .push(SwapPromise::input_ack(move |frame_token| {
                let _ = to_browser.send(RendererHostMsg::InputEventAck { frame_token });
            }));
    }

    /// `WidgetInputHandler::HandleInputEvent`. Scrolling asks for a frame;
    /// one that only moved the viewport draws again from the tiles the
    /// compositor has. Mouse events are hit tested against the lifecycle
    /// brought up to pre-paint first, as Blink's `EventHandler` does
    /// (`LayoutView::HitTest`): what script changed since the last frame
    /// is where it is now.
    fn handle_input_event(&mut self, event: InputEvent, cancelable: bool) {
        if let InputEvent::MouseMove {
            x,
            y,
            buttons,
            modifiers,
        } = event
        {
            // The next main frame dispatches it, unless another comes
            // first; with none coming (hidden), now.
            self.pending_mouse_move = Some(MouseMove {
                point: PointF::new(x, y),
                buttons,
                modifiers,
            });
            if !self.widget().layer_tree_host.is_visible() {
                self.dispatch_mouse_move();
            }
            return;
        }
        // In order: the move waiting for the frame goes first.
        self.dispatch_mouse_move();
        match event {
            InputEvent::MouseWheel { .. } => self.handle_wheel(&event, cancelable, true),
            _ => self.handle_input(&event),
        }
    }

    /// A wheel the compositor scrolled (or had nothing to scroll with) and
    /// acked (`InputDispatch::NonBlocking`): only its `wheel` event, which
    /// can't cancel anything, if it is aimed at the document shown.
    fn dispatch_wheel_only(&mut self, event: InputEvent, document: Option<DocumentId>) {
        let current = self.frame_document_id();
        if !matches!(event, InputEvent::MouseWheel { .. }) {
            return;
        }
        if document.is_none() || document != current {
            return;
        }
        self.dispatch_mouse_move();
        self.handle_wheel(&event, false, false);
    }

    /// The pointer's move waiting for the frame, if any (Blink's
    /// `EventHandler::HandleMouseMoveEvent`): the cursor for what it is
    /// over, and `mousemove` at it, with how far it moved since the last.
    fn dispatch_mouse_move(&mut self) {
        let Some(MouseMove {
            point,
            buttons,
            modifiers,
        }) = self.pending_mouse_move.take()
        else {
            return;
        };
        self.pending_input.dequeue(self.pending_move_input.take());
        self.dispatch_pending_mouse_move(point, buttons, modifiers);
        for _ in 0..std::mem::take(&mut self.mouse_move_acks) {
            let _ = self
                .to_browser
                .send(RendererHostMsg::InputEventAck { frame_token: None });
        }
    }

    fn dispatch_pending_mouse_move(&mut self, point: PointF, buttons: u16, modifiers: Modifiers) {
        self.handle_input(&InputEvent::MouseMove {
            x: point.x,
            y: point.y,
            buttons,
            modifiers,
        });
    }

    /// A load reached a redirect (Blink's `ResourceLoader::WillFollowRedirect`):
    /// nothing in the renderer decides on redirects yet, so it goes on. A
    /// load the renderer cancelled is no longer the browser's, which
    /// ignores the answer.
    fn did_receive_redirect(&mut self, request_id: u64, redirect: &RedirectInfo) {
        self.network_agent.did_redirect(request_id, redirect);
        let _ = self.to_browser.send(RendererHostMsg::FollowRedirect {
            request_id,
            worker: None,
        });
    }

    /// What the page's log says of request `request_id`'s `result`, if
    /// anything: a response of 400 or more (Blink's
    /// `FrameConsole::ReportResourceResponseReceived`), a load that failed
    /// (`FrameConsole::DidFailLoading`; a cancelled one isn't answered).
    fn log_load(&self, request_id: u64, url: Option<&str>, result: &LoadResult) {
        let text = match result {
            Ok(response) if response.head.status >= 400 => {
                status_message(response.head.status, &response.head.status_text)
            }
            Ok(_) => return,
            Err(error) => format!("Failed to load resource: net::{}", error.code.name()),
        };
        let url = match result {
            Ok(response) => response.head.url.clone(),
            Err(_) => url.unwrap_or_default().to_owned(),
        };
        self.log_agent.add(log_agent::LogEntry::network(
            text,
            url,
            self.network_agent.request_id(request_id),
        ));
    }

    /// `URLLoaderClient::OnComplete` for a subresource, or a child frame's
    /// navigation, of whichever frame asked for it. One for a document
    /// that has gone (or a navigation another replaced) is dropped.
    fn did_receive_resource(&mut self, request_id: u64, result: LoadResult) {
        self.network_agent.did_finish(request_id, &result);
        // A `fetch()`'s, an image's and a queued script's are the DOM's
        // records'.
        let url = self.request_urls.answered(request_id).or_else(|| {
            let dom = self.engine.as_mut()?.dom_mut();
            dom.sent_fetch(request_id)
                .map(|sent| sent.url)
                .or_else(|| dom.sent_image(request_id).map(|sent| sent.url))
                .or_else(|| dom.sent_script(request_id).map(|sent| sent.url))
                .or_else(|| dom.sent_sheet(request_id).map(|sent| sent.url))
        });
        // DevTools' `CSS`'s: a sheet's text, its alone.
        if let Some((key, frame)) = self.css_refetches.remove(&request_id) {
            self.log_load(request_id, url.as_deref(), &result);
            self.css_refetched(key, frame, &result);
            return;
        }
        self.log_load(request_id, url.as_deref(), &result);
        self.record_resource(request_id, url.as_deref(), &result);
        self.resource(request_id, url.as_deref(), &result);
    }

    /// `WidgetBase::BeginMainFrame`: the compositor asks for a main frame,
    /// for the `BeginFrame` `args` is. Runs the animation frame callbacks,
    /// then the rendering update if anything asked for one, and ends the
    /// main frame either way, as the compositor waits for that: with a
    /// commit, or by saying there was nothing to commit.
    fn begin_main_frame(&mut self, args: BeginFrameArgs) {
        // The whole frame, the animation frame callbacks too, as Chrome's
        // trace has it; each step below is a span of its own.
        let _span = trace_event::trace_event!("ProxyMain::BeginMainFrame");
        self.last_main_frame = self.clock.now();
        self.resize_loop_error = false;
        {
            let _span = trace_event::trace_event!("WebFrameWidgetImpl::BeginMainFrame");
            self.dispatch_mouse_move();
            // HTML's "update the rendering": its documents found once, for
            // every step below (each walked the frame tree's documents for
            // their iframes again).
            if let Some(engine) = &mut self.engine {
                engine.begin_rendering_update();
            }
            self.flush_autofocus_candidates();
            self.run_scroll_steps();
            // The frame's time: now (Viz's `frame_time` is on its own
            // clock).
            let _ = args.frame_time;
            // HTML's "evaluate media queries and report changes", before
            // the animation frame callbacks.
            self.report_media_query_changes();
            let next_frame_time = std::time::Instant::now();
            self.update_animations(next_frame_time);
            self.run_animation_frames(next_frame_time);
        }
        self.run_resize_observer_steps();
        self.run_focus_fixup();
        // A title the callbacks set goes before the frame's swap promises
        // (Blink sends `DidChangeTitle` as the title changes): what waits
        // for the frame (`ReftestWaitEnded`) comes after it.
        self.update_title();
        if self.widget().needs_begin_main_frame {
            self.update_rendering();
        } else {
            self.widget().layer_tree_host.abort_main_frame();
        }
        if let Some(engine) = &mut self.engine {
            engine.end_rendering_update();
        }
    }

    /// `LocalFrameView::UpdateAllLifecyclePhases` -> `LayerTreeHost` commit
    /// (`update_rendering`), or an aborted main frame if there is
    /// nothing to commit.
    fn update_rendering(&mut self) {
        self.widget().needs_begin_main_frame = false;
        if !self.update_document_rendering() {
            self.widget().layer_tree_host.abort_main_frame();
        }
    }
}

/// The promise of a `VisualStateReady` for the request `request_id`, with
/// the frame's next commit; none without `RendererFlags::automation`, which
/// is what asks.
fn visual_state_promise(
    to_browser: &BrowserSender,
    automation: bool,
    request_id: u64,
) -> Option<SwapPromise> {
    if !automation {
        tracing::warn!("RequestVisualState without RendererFlags::automation; ignored");
        return None;
    }
    let to_browser = to_browser.clone();
    Some(SwapPromise::new(move |_| {
        let _ = to_browser.send(RendererHostMsg::VisualStateReady { request_id });
    }))
}

/// The page's log's message for a response of `status`, 400 or more.
fn status_message(status: u16, status_text: &str) -> String {
    format!(
        "Failed to load resource: the server responded with a status of {status} ({status_text})"
    )
}

/// A frame interval, ms: the compositor's begin frames come at 60 Hz.
const FRAME_INTERVAL: f64 = 1000.0 / 60.0;

/// The longest idle period, ms, with nothing to draw (Blink's
/// `kMaximumIdlePeriodMillis`).
const LONG_IDLE_PERIOD: f64 = 50.0;

/// The sooner of two times, either of which may be missing.
fn sooner_ms(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    [a, b].into_iter().flatten().reduce(f64::min)
}

/// A `console` message (`kind` `console`), or what script left uncaught
/// (`uncaught`), printed to stderr with the browser's log, as Chrome prints
/// console messages: one that is nothing (`console.log("")`, an array too
/// long to write) isn't printed, as Chrome's isn't. Not stdout, which a
/// page doesn't write to in Chrome either.
pub(crate) fn print_console(kind: &str, text: &str, url: &str, line: u32) {
    if text.is_empty() {
        return;
    }
    print_line(match (url.is_empty(), line) {
        (true, _) => format!("[{kind}] {text}\n"),
        (false, 0) => format!("[{kind}] {text} ({url})\n"),
        (false, line) => format!("[{kind}] {text} ({url}:{line})\n"),
    });
}

/// `line` (with its newline) to stderr in one write: the browser and its
/// other renderers, in processes of their own, write there too, and what
/// is written in pieces has theirs between the pieces. (`eprintln!` with
/// the text and its parentheses apart writes each as it comes; one string
/// is one write. By the macro, so that a test's run keeps it with the
/// test's output.)
pub(crate) fn print_line(line: String) {
    eprint!("{line}");
}

/// The main frame's viewport, CSS px, and the device pixels per CSS px.
fn viewport(properties: &VisualProperties) -> (SizeF, f32) {
    let dsf = properties.device_scale_factor.max(0.01);
    let size = SizeF::new(
        properties.viewport_size.width as f32 / dsf,
        properties.viewport_size.height as f32 / dsf,
    );
    (size, dsf)
}

/// The pointer's move, waiting for the next main frame.
#[derive(Debug, Clone, Copy)]
struct MouseMove {
    point: PointF,
    buttons: u16,
    modifiers: Modifiers,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The flags a renderer starts with are values a renderer process of
    /// its own could get (checked when this compiles).
    #[test]
    fn the_flags_can_cross_a_process() {
        fn crosses<T: serde::Serialize + serde::de::DeserializeOwned + Send + 'static>() {}
        crosses::<RendererFlags>();
    }

    /// Blink's budgets on the desktop: 10 ms for a run after each of the
    /// first 6 yields (and the first run), 500 ms after.
    #[test]
    fn the_parsers_budget_grows_after_six_yields() {
        let budget = ParserBudget::default();
        assert_eq!(budget.of_run(0), Duration::from_millis(10));
        assert_eq!(budget.of_run(6), Duration::from_millis(10));
        assert_eq!(budget.of_run(7), Duration::from_millis(500));
        assert_eq!(
            ParserBudget::yield_every_slice(64).of_run(0),
            Duration::ZERO
        );
    }
}
