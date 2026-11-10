//! The DOM's script engine as its host (the renderer) drives it: a
//! realm whose global is a window of a new document, the document's
//! classic scripts, its microtasks. Engine-agnostic, so the renderer names
//! no engine: whoever starts it gives one (V8's is `dom_js`'s). Blink's `ScriptController` and
//! `WindowProxy` are in the same binary as V8 and need no such trait.

use std::time::Instant;

use dom::{Dom, NodeId, NodeRoot};

use crate::{
    DocumentReadiness, FrameLayout, KeyInput, KeyResult, MouseInput, ParserHost,
    WebDriverActionError, WebDriverAnswer, WebDriverQuery, WebDriverQueryError,
    WebDriverScriptError, WebDriverValue, WheelInput,
};

/// A `console` message for the host to print: its text, and the script and
/// line it came from (empty and 0 when unknown).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleMessage {
    pub text: String,
    pub url: String,
    pub line: u32,
    /// An exception, or a promise's rejection, nothing handled (its text
    /// without `Uncaught `): not a `console` call.
    pub uncaught: bool,
}

/// Where an engine's `console` messages go (the host's console).
pub type ConsoleSink = Box<dyn Fn(ConsoleMessage)>;

/// The host's side of the loads script asks for, as it asks, before the
/// call returns (Blink's `FetchManager` starts its loader in `fetch()`; a
/// blob URL is the registry's at once): so that a `fetch()` and a blob
/// URL revoked after it in the same task go in that order, and a request
/// leaves while the script that made it goes on. The engine tells its
/// host what it sent after (`FrameRequest::FetchSent`).
pub trait LoadSink {
    /// `request`, of `frame`'s realm's `fetch()`, sent: its request's
    /// number, or none if it went nowhere (a revoked blob URL's, a network
    /// error).
    fn start_fetch(&self, frame: crate::FrameId, request: &dom::FetchRequest) -> Option<u64>;

    /// `url` made a blob URL for `data` in `frame`'s realm
    /// (`URL.createObjectURL`).
    fn register_blob_url(&self, frame: crate::FrameId, url: &str, data: &dom::BlobData);

    /// `url` revoked (`URL.revokeObjectURL`): no load after it gets it.
    fn revoke_blob_url(&self, url: &str);

    /// `frame` went (its iframe removed): the blob URLs its realm made are
    /// revoked now, for the loads script asks for after.
    fn frame_gone(&self, frame: crate::FrameId);
}

/// What an engine is made with, as its host's settings say.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EngineSettings {
    /// Web tests (content_shell's `--run-web-tests`): each window has a
    /// `testRunner`, which `reportHarnessResults` reports to the host
    /// (`ScriptEngine::take_harness_results`).
    pub web_tests: bool,
}

/// A wasm script to run (a component): its bytes and the URL it came from.
#[derive(Debug, Clone, Copy)]
pub struct WasmScript<'a> {
    pub bytes: &'a [u8],
    pub url: &'a str,
}

/// A classic script to run: its text, the URL it came from (an inline
/// one's is its document's), its base URL, the line its text starts on
/// (1-based), which error positions count from, whether its errors are
/// muted, and its fetch options.
#[derive(Debug, Clone, Copy)]
pub struct ClassicScript<'a> {
    pub source: &'a str,
    pub url: &'a str,
    /// What an `import()` in it resolves against: an inline one's is its
    /// document's base URL as it was prepared (HTML's "prepare the script
    /// element": a `<base>` counts, an `about:srcdoc` document's is its
    /// parent's); `None`, its URL (an external one's response's).
    pub base_url: Option<&'a str>,
    pub line: u32,
    /// Its errors are muted (HTML's "muted errors": another origin's,
    /// fetched `no-cors`): what it throws is reported as `Script error.`.
    pub muted: bool,
    /// Its fetch options' credentials mode is include (its `crossorigin`
    /// is `use-credentials`; else same-origin): what an `import()` in it
    /// fetches with.
    pub use_credentials: bool,
}

/// The DOM's engine for a page: its browsing contexts by `FrameId`
/// (the top-level one's, `top_frame`, and a child's for each iframe), each
/// one document and its window at a time; its documents' parsers' host too
/// (the parser constructs custom elements through it).
pub trait ScriptEngine: ParserHost {
    /// The DOM the engine's documents are in, as long as the engine is
    /// borrowed (none of its calls runs meanwhile): the parser builds a
    /// document in it.
    fn dom_mut(&mut self) -> &mut Dom;

    /// The frame's layout, the host's (its document's members bring it up
    /// to date and read it): replaces the one before.
    fn set_layout(&mut self, layout: Box<dyn FrameLayout>);

    /// The DOM and the frame's layout, if it has one, both borrowed (none
    /// of the engine's calls runs meanwhile): what the host's frame needs.
    fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn FrameLayout>);

    /// The top-level browsing context's number.
    fn top_frame(&self) -> crate::FrameId;

    /// `frame`'s document's navigation timing is `timing` (the network's
    /// half, which the browser told at the commit; times since its window's
    /// time origin): what `performance`'s navigation entry and
    /// `performance.timing` read, the document's own times marked on it as
    /// its events fire. Nothing for a frame that went.
    fn set_navigation_timing(&mut self, frame: crate::FrameId, timing: dom::NavigationTiming);

    /// `frame`'s new active document: a new realm whose global is a new
    /// window of a new HTML document at `url` (it replaces the one before;
    /// a child frame's is its iframe's content document), its time origin
    /// `time_origin` (the navigation's start): the document, loading, for
    /// the parser to fill. None for a frame that went.
    fn create_window(
        &mut self,
        frame: crate::FrameId,
        url: &str,
        time_origin: Instant,
    ) -> Option<NodeRoot>;

    /// What the engine asks the host to do for its frames since the last
    /// time (a child made, to navigate, gone, a `fetch()` to send), in
    /// order, taken.
    fn take_frame_requests(&mut self) -> Vec<crate::FrameRequest>;

    /// The response to the fetch numbered `id` came (`Err`: a network
    /// error), as a task of its own: its promise settled in its window's
    /// realm, a microtask checkpoint after. Whether the fetch was still in
    /// flight.
    fn fetch_response(&mut self, id: u64, response: Result<crate::FetchedResponse, ()>) -> bool;

    /// HTML's "run a classic script" in `frame`'s window's realm: what it
    /// throws (a syntax error too) is reported at the window, not
    /// rethrown. No microtask checkpoint: the host performs it.
    fn run_classic_script(&mut self, frame: crate::FrameId, script: &ClassicScript<'_>);

    /// A module script the parser prepared (`Prepared::Module`): its graph
    /// fetched in the realm of the frame showing `document`
    /// (`ScriptHost::start_module_graph`).
    fn start_module_graph(&mut self, document: &dom::NodeRoot, start: crate::ModuleStart);

    /// Runs a wasm script in `frame`'s document: instantiated, its `start`
    /// called with the document, done at its first wait. An engine that
    /// runs no wasm (a JS document's) runs none.
    fn run_wasm_script(&mut self, frame: crate::FrameId, script: &WasmScript<'_>) {
        let _ = (frame, script);
    }

    /// A module a realm's module map asked for (a `ScriptFetch` of a
    /// module), fetched (`None`: it failed): its graphs move on, those
    /// fetched told to their pending scripts. Whether it was one.
    fn module_fetched(&mut self, key: u64, response: Option<crate::module::FetchedModule>) -> bool;

    /// The parser's deferred module script `element`, its graph fetched
    /// (`Cx::pending_script_fetched`), its turn come: executed
    /// (`dom_api::run_script_task`), a microtask checkpoint after.
    fn run_ready_script(&mut self, element: &dom::NodeRoot);

    /// HTML's "perform a microtask checkpoint", then its "notify about
    /// rejected promises": an `unhandledrejection` at the window for each
    /// promise rejected with no handler since, logged if not canceled.
    fn perform_microtask_checkpoint(&mut self);

    /// HTML's "the end" of the loading of `frame`'s document at
    /// `readiness` (`dom_api::document_ready`), a microtask checkpoint
    /// after each event it fires.
    fn document_ready(&mut self, frame: crate::FrameId, readiness: DocumentReadiness);

    /// HTML's "unload a document and its descendants" of `frame`'s
    /// document as its frame navigates on (`dom_api::unload_document`,
    /// then the frames below it, as Blink orders them): when its own
    /// `unload` started and ended
    /// (its "unload timing info", the next document's navigation timing's
    /// if they are of one origin); none for an initial `about:blank`, or a
    /// frame the engine doesn't know.
    fn unload_document(&mut self, frame: crate::FrameId) -> Option<(Instant, Instant)>;

    /// HTML's "fire an event named `event_type`" at `target` (a node of a
    /// frame's document, in that frame's realm; neither bubbling nor
    /// cancelable), then a microtask checkpoint: a `<link>`'s `load` or
    /// `error`, an iframe's `load` (`dom::names::event`'s).
    fn fire_event(&mut self, target: &NodeRoot, event_type: dom::NameId);

    /// A `scroll` event at `target` (HTML's run the scroll steps: a
    /// document's, for its viewport, bubbles to its window; an element's
    /// doesn't), in its frame's realm, then a microtask checkpoint; none at
    /// one no longer in its document.
    fn fire_scroll_event(&mut self, target: dom::NodeId);

    /// The user's mouse `input` on the window's document
    /// (`EventHandler::handle_mouse`: hit tested in the frame's layout, a
    /// microtask checkpoint after each event), then a microtask
    /// checkpoint: the cursor for what the pointer is over (never `auto`).
    fn handle_mouse(&mut self, input: &MouseInput) -> crate::CursorKeyword;

    /// What the pointer is over, found again where it last was after a
    /// rendering update (`EventHandler::recompute_under_mouse`: the
    /// boundary events, no `mousemove`), then a microtask checkpoint: the
    /// cursor for it; none before the pointer was over the page.
    fn recompute_under_mouse(&mut self) -> Option<crate::CursorKeyword>;

    /// The user's wheel `input` on the window's document
    /// (`EventHandler::handle_wheel`), then a microtask checkpoint: whether
    /// nothing canceled its `wheel` event (the host scrolls then).
    fn handle_wheel(&mut self, input: &WheelInput) -> bool;

    /// A key going down or up on the window's document
    /// (`EventHandler::handle_key`), then a microtask checkpoint: what it
    /// left the host to do.
    fn handle_key(&mut self, input: &KeyInput) -> KeyResult;

    /// Tab from the browser's toolbar into the page (`set_initial_focus`),
    /// then a microtask checkpoint.
    fn set_initial_focus(&mut self, reverse: bool);

    /// The popup the host showed for `select` (one of the window's
    /// document's, by its id: `dom::PopupRequest`) closed with
    /// `result` (`popup_menu_closed`), then a microtask checkpoint.
    fn popup_menu_closed(&mut self, select: NodeId, result: crate::PopupResult);

    /// Whether a frame's window has an animation frame callback: the
    /// host asks for a frame.
    fn has_animation_frames(&mut self) -> bool;

    /// Whether a frame's document has animations that run, or events of
    /// theirs to send (`dom::Cx::animations_want_frames`): the host
    /// asks for a frame, as after script played or canceled one.
    fn animations_want_frames(&mut self) -> bool;

    /// The frames, the top-level one first, each followed by its child
    /// frames in their iframes' (shadow-including) tree order: HTML's order
    /// of the documents the rendering update goes through. Between
    /// `begin_rendering_update` and `end_rendering_update`, those of the
    /// frames found as it began that are still there.
    fn frames_in_tree_order(&mut self) -> Vec<crate::FrameId>;

    /// `frames_in_tree_order`'s documents.
    fn documents_in_tree_order(&mut self) -> Vec<NodeId>;

    /// HTML's "update the rendering" begins: its documents (its `docs`),
    /// found once by walking the frame tree's documents for their
    /// iframes, are what each of its steps goes through until
    /// `end_rendering_update`, rather than each step walking them again.
    /// A frame that goes in the meantime is left out; one that comes is
    /// the next update's (as HTML's `docs`, made as the update begins).
    fn begin_rendering_update(&mut self);

    /// The rendering update is over: `frames_in_tree_order` walks again.
    fn end_rendering_update(&mut self);

    /// HTML's "run the animation frame callbacks" of each frame's window
    /// in tree order (`frames_in_tree_order`, as the step began), the
    /// frame's time `frame_time` (`run_animation_frame_callbacks`), then a
    /// microtask checkpoint.
    fn run_animation_frames(&mut self, frame_time: Instant);

    /// The page took or lost the system's focus (the browser's `SetFocus`):
    /// kept for each window to come; the window's document's focus events
    /// (`set_page_focused`), then a microtask checkpoint.
    fn set_page_focused(&mut self, focused: bool);

    /// The page was hidden or shown (the browser's `WasHidden`,
    /// `WasShown`): kept for each window to come; each document of the frame
    /// tree's `visibilitychange` if it changed (`set_page_visible`), then a
    /// microtask checkpoint.
    fn set_page_visible(&mut self, visible: bool);

    /// What WebDriver asks of `frame`'s document
    /// (`dom_api::webdriver`'s `answer`), in an operation:
    /// `NotShown` for a frame that has gone.
    fn webdriver_query(
        &mut self,
        frame: crate::FrameId,
        query: &WebDriverQuery<'_>,
    ) -> Result<WebDriverAnswer, WebDriverQueryError>;

    /// WebDriver's Element Send Keys before its keys, of `frame`'s
    /// document's `element` (`dom_api::webdriver_send_keys`), then a
    /// microtask checkpoint: whether the driver types them.
    fn webdriver_send_keys(
        &mut self,
        frame: crate::FrameId,
        element: NodeId,
        text: &str,
    ) -> Result<bool, WebDriverActionError>;

    /// WebDriver's Element Clear of `frame`'s document's `element`
    /// (`dom_api::webdriver_clear`), then a microtask checkpoint.
    fn webdriver_clear(
        &mut self,
        frame: crate::FrameId,
        element: NodeId,
    ) -> Result<(), WebDriverActionError>;

    /// WebDriver's Execute (Async) Script `id` in `frame`'s realm: `body` a
    /// function body called with `args`; its result, once it settles,
    /// taken with `take_webdriver_results`.
    fn execute_webdriver_script(
        &mut self,
        frame: crate::FrameId,
        id: u64,
        body: &str,
        args: &[WebDriverValue],
        is_async: bool,
    ) -> Result<(), WebDriverScriptError>;

    /// The WebDriver scripts that settled since last asked.
    fn take_webdriver_results(
        &mut self,
    ) -> Vec<(u64, Result<WebDriverValue, WebDriverScriptError>)>;

    /// Get Element Property: `element`'s property `name` as script reads
    /// it, in `frame`'s realm.
    fn webdriver_element_property(
        &mut self,
        frame: crate::FrameId,
        element: NodeId,
        name: &str,
    ) -> Result<WebDriverValue, WebDriverScriptError>;

    /// HTML's "flush autofocus candidates" of the top-level document at
    /// the start of the rendering update (`flush_autofocus_candidates`),
    /// then a microtask checkpoint.
    fn flush_autofocus_candidates(&mut self);

    /// HTML's focus fixup of each frame's document in tree order at the
    /// rendering update (`focus_fixup`), then a microtask checkpoint.
    fn run_focus_fixup(&mut self);

    /// HTML's "react to environment changes" of `frame`'s document, whose
    /// viewport or device pixel ratio changed
    /// (`react_to_environment_changes`), then a microtask checkpoint.
    fn react_to_environment_changes(&mut self, frame: crate::FrameId);

    /// HTML's "evaluate media queries and report changes" of `frame`'s
    /// document (`report_media_query_changes`), then a microtask
    /// checkpoint.
    fn report_media_query_changes(&mut self, frame: crate::FrameId);

    /// HTML's "update animations and send events" of `frame`'s document,
    /// at `frame_time` (`update_animations`: its timeline's time is the
    /// frame's, as its animation frame callbacks get), then a microtask
    /// checkpoint.
    fn update_animations(
        &mut self,
        frame: crate::FrameId,
        frame_time: std::time::Instant,
    ) -> crate::AnimationsUpdate;

    /// The page's session history as the browser has it now: the current
    /// entry's index and how many there are (`history.length`).
    fn set_session_history(&mut self, history: dom::SessionHistory);

    /// The browser's traversal to another entry of `frame`'s document
    /// (`traverse_within_document`): its URL `url`, its history's state
    /// (serialized; none: `null`) and scroll restoration mode; `popstate`
    /// fires now, `hashchange` in a task if the fragment changed. Whether
    /// the frame shows a document with a window.
    fn traverse_within_document(
        &mut self,
        frame: crate::FrameId,
        url: &str,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
    ) -> bool;

    /// `frame`'s new document's entry's history state and scroll
    /// restoration mode, as the browser gives them at its commit
    /// (`restore_history`).
    fn restore_history(
        &mut self,
        frame: crate::FrameId,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
    );

    /// The animations of `frame`'s document that the rendering about to be
    /// committed shows start, at its timeline's time
    /// (`dom::Cx::start_pending_animations`). Whether it wants another
    /// frame: some started, or run.
    fn start_pending_animations(&mut self, frame: crate::FrameId) -> bool;

    /// How long until a frame's window's next timer is due (none with no
    /// timer; now with an element task queued).
    fn until_next_timer(&mut self) -> Option<std::time::Duration>;

    /// Runs the frames' windows' timers due now, the earliest due first
    /// across the windows (those due at once, the first frame made's), each in its
    /// window's realm and a microtask checkpoint after it: whether any
    /// ran.
    fn run_timers(&mut self) -> bool;

    /// A pass over the page's tasks that are due (the frames' windows'
    /// timers, the UA's own, idle callbacks' timeouts, element tasks and
    /// messages): one at a time, the earliest `(time, number)` across the
    /// windows and their queues first (`dom`'s `tasks`), the clock
    /// read again before each, each in its window's realm with its
    /// microtask checkpoint; only those numbered before the pass began, so
    /// that it ends. Whether any ran.
    fn run_due_tasks(&mut self) -> bool;

    /// Whether a frame's window has an idle callback waiting
    /// (`requestIdleCallback`'s); and one it may run in the idle period
    /// running.
    fn has_idle_callbacks(&mut self) -> bool;
    fn has_runnable_idle_callback(&mut self) -> bool;

    /// HTML's "start an idle period" in every frame's window: the host's
    /// scheduler has nothing else to do (Blink's `IdleHelper`).
    fn start_idle_period(&mut self);

    /// One idle callback of the idle period running, ending at `deadline`:
    /// the first window's, in tree order, that may run one
    /// (`dom_api::run_idle_callback`), in its realm, a microtask
    /// checkpoint after it. Whether one ran.
    fn run_idle_callback(&mut self, deadline: std::time::Instant) -> bool;

    /// What tells `isInputPending()` of the input waiting for the main
    /// thread (`InputMonitor`), the host's: replaces the one before.
    fn set_input_monitor(&mut self, monitor: Box<dyn crate::InputMonitor>);

    /// Runs the frames' windows' element tasks, a window's after another's
    /// (`dom_api::run_element_tasks`: a text control's `select`), a
    /// microtask checkpoint after each: whether any ran.
    fn run_element_tasks(&mut self) -> bool;

    /// Runs the frames' windows' messages (`run_posted_messages`: HTML's
    /// posted message task source), a window's after another's, a
    /// microtask checkpoint after each: whether any ran.
    fn run_posted_messages(&mut self) -> bool;

    /// The intersection observers' task of each frame's window in tree
    /// order (the host queued it as `update_intersections` asked):
    /// `notify_intersection_observers`, a microtask checkpoint after each.
    fn notify_intersection_observers(&mut self);

    /// Whether `frame`'s window's resize observers observe anything: the
    /// host's resize observer loop runs for its document only then.
    fn has_resize_observations(&mut self, frame: crate::FrameId) -> bool;

    /// A round of the host's resize observer loop for `frame`'s document,
    /// the frames laid out (`gather_and_broadcast` at `depth`), a
    /// microtask checkpoint after each callback.
    fn gather_and_broadcast_resizes(
        &mut self,
        frame: crate::FrameId,
        depth: usize,
    ) -> crate::ResizeObserverRound;

    /// The resize observer loop's error, at `frame`'s window
    /// (`report_loop_error`).
    fn report_resize_loop_error(&mut self, frame: crate::FrameId);

    /// Runs the engine's platform tasks that are ready (incremental
    /// marking steps, GC finalization), from the outermost loop: whether
    /// any ran (the host performs a checkpoint after them).
    fn run_platform_tasks(&mut self) -> bool;

    /// Where `console` messages go.
    fn set_console_sink(&mut self, sink: ConsoleSink);

    /// What sends the loads script asks for as it asks (`LoadSink`): the
    /// host's, in place of the frame requests for them
    /// (`FrameRequest::RegisterBlobUrl`, `RevokeBlobUrl`, `Fetch`).
    fn set_load_sink(&mut self, sink: Box<dyn LoadSink>);

    /// What `testRunner.reportHarnessResults` was given since the last
    /// ask, if anything (web tests only).
    fn take_harness_results(&mut self) -> Option<String>;

    /// Whether the host is waiting for its next task (`idle`) or in one,
    /// though not in script, for DevTools' `Profiler` to tell the two
    /// apart (`(idle)`, `(program)`), as Blink tells V8's inspector around
    /// each task. Not while script runs.
    fn set_idle(&mut self, idle: bool);

    /// What DevTools calls the frames (`Page.Frame`'s `id`), the host's:
    /// each realm is told to the engine's inspector with its frame's, as
    /// it is made (replaces the ones before; realms made already keep
    /// theirs).
    fn set_devtools_frame_ids(&mut self, ids: DevToolsFrameIds);

    /// A DevTools session of the browser's `session` for the engine's
    /// frames, whose answers and events go to `send`, as JSON (V8's
    /// inspector answers `Runtime`, `Debugger`...).
    fn devtools_attach(&mut self, session: String, send: DevToolsSend);

    /// The browser's `session` has gone (none of the engine's: nothing).
    fn devtools_detach(&mut self, session: &str);

    /// Hands `message`, a DevTools command of the browser's `session` as
    /// JSON, to the engine's inspector, as a task of its own: it may run
    /// page script (`Runtime.evaluate`), a microtask checkpoint after it.
    /// Whether the session is the engine's.
    fn dispatch_devtools_message(&mut self, session: &str, message: &str) -> bool;

    /// Gives the engine the host's DevTools queue, which its inspector
    /// takes from while script is on the stack: while script is paused (a
    /// breakpoint), and when a message is to interrupt script (Blink's
    /// `InspectorTaskRunner`). What the queue is to call, from any thread,
    /// when one is to interrupt it. Once, before any script runs.
    fn set_devtools_queue(&mut self, queue: Box<dyn DevToolsQueue>) -> DevToolsInterrupter;

    /// Whether DevTools wants the stacks of the script that asks for loads
    /// (`requests`: `Network.setAttachDebugStack`) and of the script whose
    /// iframe makes a frame (`frames`: `Page.frameAttached`'s): the engine
    /// captures them as script asks, for the host to take with the loads
    /// (`Dom::take_request_stack`, `LoadInitiator::Script`), as Blink's
    /// `InspectorNetworkAgent` captures `CaptureSourceLocation` (as
    /// `Runtime.StackTrace` JSON, with its async parents). An engine
    /// without an inspector captures none.
    fn set_stacks_wanted(&mut self, requests: bool, frames: bool);

    /// The page as the host's DevTools agents read it (`DevToolsPage`),
    /// while script doesn't run.
    fn devtools_page(&mut self) -> Box<dyn DevToolsPage + '_>;
}

/// What DevTools calls a frame: `None` for one it has no name for (the
/// top-level one before the browser named it).
pub type DevToolsFrameIds = Box<dyn Fn(crate::FrameId) -> Option<String>>;

/// Where a DevTools session's answers and events go, as JSON
/// ([`ScriptEngine::devtools_attach`]).
pub type DevToolsSend = Box<dyn Fn(String)>;

/// What has the engine's thread take the DevTools messages that are to
/// interrupt script, called from any thread
/// ([`ScriptEngine::set_devtools_queue`]).
pub type DevToolsInterrupter = Box<dyn Fn() + Send + Sync>;

/// What the browser asked of the page's DevTools sessions, in order, as
/// the host's DevTools queue holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevToolsQueued {
    Attach(String),
    Detach(String),
    Message {
        session: String,
        message: String,
    },
    /// The host is to end.
    Quit,
    /// The host has a frame to draw (a main frame, a frame asked for),
    /// which runs no script: drawn while script is paused
    /// ([`DevToolsQueue::render`]). Never queued: `next` says it.
    Render,
}

/// The host's DevTools queue, as an engine's inspector takes from it while
/// script is on the stack ([`ScriptEngine::set_devtools_queue`]).
pub trait DevToolsQueue {
    /// The next, waiting for one.
    fn next(&self) -> DevToolsQueued;
    /// The messages that are to interrupt script, `(session, message)`,
    /// taken out in order: those before any other kind.
    fn take_interrupting(&self) -> Vec<(String, String)>;
    /// Puts `queued` back first, in order: what waits for the pause to
    /// end.
    fn put_back(&self, queued: Vec<DevToolsQueued>);
    /// Answers `message`, a command of `session`'s, if the host does
    /// itself (its agents'), even while script is paused: whether it did.
    /// The engine's inspector takes the others. `page` is what the engine
    /// lends meanwhile.
    fn answer_here(&self, session: &str, message: &str, page: &mut dyn DevToolsPage) -> bool;
    /// Script paused: the host tells its sessions what it did up to there
    /// (the DOM's changes), before the pause takes the first message.
    fn paused(&self, page: &mut dyn DevToolsPage);
    /// Script paused waits for the next: what the last command changed
    /// (script the console ran) is told, and drawn at the next frame.
    fn waiting(&self, page: &mut dyn DevToolsPage);
    /// Script paused: the host draws the frame `next` said it has
    /// ([`DevToolsQueued::Render`]) with the page as it is.
    fn render(&self, page: &mut dyn DevToolsPage);
}

/// The page as an engine lends it to its host's DevTools agents, as they
/// answer a command, script paused or not (Blink's agents answer from a
/// pause's nested loop with the whole page): its DOM, the top-level
/// frame's layout, and what its inspector knows of script's objects.
pub trait DevToolsPage {
    /// The DOM, and the layout unless an operation that called the paused
    /// script holds it.
    fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn FrameLayout>);
    fn dom(&mut self) -> &mut Dom {
        self.dom_and_layout().0
    }
    fn objects(&mut self) -> &mut dyn DevToolsObjects;
}

/// What a host's DevTools agents ask of an engine's inspector about
/// script's objects, for the browser's session `session` (Blink's
/// `ResolveNode`, `NodeForRemoteObjectId`, `addInspectedObject` and
/// `EventListenersInfoForTarget`). An engine with no inspector has none of
/// them.
pub trait DevToolsObjects {
    /// `node`'s object in its frame's realm, as a `Runtime.RemoteObject`
    /// (JSON) in the object `group`: none if no frame shows its document.
    fn resolve_node(&mut self, session: &str, node: NodeId, group: &str) -> Option<String>;
    /// The node `object_id` is the object of; the error to answer with if
    /// it is none.
    fn node_of_object(&mut self, session: &str, object_id: &str) -> Result<NodeId, String>;
    /// `node` is what the console's `$0` is from now on.
    fn inspect_node(&mut self, session: &str, node: NodeId);
    /// The listeners on the target `object_id` is (and, a node, on those
    /// below it `depth` levels down; -1 all, through frames and shadow
    /// trees if `pierce`, text of white space alone left out unless
    /// `include_whitespace`), as `DOMDebugger.EventListener`s (JSON);
    /// the error to answer with if `object_id` is no object.
    fn event_listeners(
        &mut self,
        session: &str,
        object_id: &str,
        depth: i64,
        pierce: bool,
        include_whitespace: bool,
    ) -> Result<String, String>;
}
