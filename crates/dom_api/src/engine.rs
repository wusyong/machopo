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
/// leaves while the script that made it goes on; and told to DevTools
/// as it is sent (Blink's `ResourceLoadObserverForFrame::WillSendRequest`).
/// The engine records what was sent (`Dom::note_fetch_sent`), which the
/// host gives the responses back by (`ScriptEngine::fetch_response`).
pub trait LoadSink {
    /// `request`, of `frame`'s realm's `fetch()`, sent, `origin` naming
    /// where it comes from: its request's number; none if it went nowhere
    /// (a revoked blob URL's), for which the host gives a network error
    /// back in a task of its own.
    fn start_fetch(
        &mut self,
        frame: crate::FrameId,
        request: &dom::FetchRequest,
        origin: &FetchOrigin,
    ) -> Option<u64>;

    /// The request numbered `request_id` cancelled (its fetch aborted, or
    /// its document gone): no response is wanted.
    fn cancel_fetch(&mut self, request_id: u64);

    /// The image at `url`, which `document` (`frame`'s, or one no frame
    /// shows) asked for first, its request sent from `request_origin`
    /// (HTML's "create a potential-CORS request", no-cors: none for an
    /// opaque origin), `origin` naming where it comes from, as the DOM
    /// asks (`send_image_fetches`; Blink's `ImageLoader` in its microtask):
    /// its request's number; none if it went nowhere (a revoked blob
    /// URL's), which fails it at once. None by default.
    fn start_image_fetch(
        &mut self,
        frame: crate::FrameId,
        document: dom::NodeId,
        url: &str,
        request_origin: Option<&str>,
        origin: &FetchOrigin,
    ) -> Option<u64> {
        let _ = (frame, document, url, request_origin, origin);
        None
    }

    /// The script `request` (a classic script's that waits for no parser,
    /// or a module's), which `document` (`frame`'s) asked for, its request
    /// sent from `request_origin`, `origin` naming where it comes from, as
    /// the DOM asks (`send_script_fetches`; Blink's `ScriptLoader::
    /// PrepareScript` as it prepares it): its request's number; none if it
    /// went nowhere, which the host fails itself. None by default.
    fn start_script_fetch(
        &mut self,
        frame: crate::FrameId,
        document: dom::NodeId,
        request: &dom::ScriptRequest,
        request_origin: Option<&str>,
        origin: &FetchOrigin,
    ) -> Option<u64> {
        let _ = (frame, document, request, request_origin, origin);
        None
    }

    /// `url` made a blob URL for `data` in `frame`'s realm
    /// (`URL.createObjectURL`).
    fn register_blob_url(&self, frame: crate::FrameId, url: &str, data: &dom::BlobData);

    /// `url` revoked (`URL.revokeObjectURL`): no load after it gets it.
    fn revoke_blob_url(&self, url: &str);

    /// `frame` went (its iframe removed): the blob URLs its realm made are
    /// revoked now, for the loads script asks for after.
    fn frame_gone(&self, frame: crate::FrameId);
}

/// The host's side of the engine's frames, told as they are made and as
/// they go, and of their navigations as script asks, before the call
/// returns (Blink's `LocalFrameClient`, whose `CreateFrame`, `Detached`
/// and `BeginNavigation` content's `RenderFrameImpl` answers by telling
/// the browser at once): so that the browser's frame tree has a frame as
/// script goes on with it, and has it no more as its iframe goes, and the
/// browser starts a navigation as script asks for it.
/// Frames are named by their tokens, the browser's names for them.
pub trait FrameClient {
    /// Child frame `frame` made for an iframe in the frame `parent`, its
    /// iframe's name `name`; `created_by_script` if script was running
    /// (Blink's `is_created_by_script`), `stack` that script's if the DOM
    /// wants frames' stacks (`Dom::frame_stacks_wanted`). Before its realm
    /// is (as Blink's `LocalFrame` constructor tells DevTools, before the
    /// frame's context comes); `dom` as it is then, for what DevTools has
    /// queued of it to go first.
    fn child_frame_created(
        &mut self,
        parent: crate::FrameToken,
        frame: crate::FrameToken,
        name: &str,
        created_by_script: bool,
        stack: Option<&str>,
        dom: &dom::Dom,
    );

    /// Child frame `frame` has its new document `document`, a navigation's
    /// (`ScriptEngine::create_window`): before its realm is told to the
    /// inspector and any of its script runs, as Chrome tells
    /// `Page.frameNavigated` as the navigation commits, before the
    /// frame's context comes.
    fn child_document_created(&mut self, frame: crate::FrameToken, document: dom::NodeId);

    /// Child frame `frame` gone, and the frames in it with it: its iframe
    /// removed, or the document it was in replaced.
    fn frame_detached(&mut self, frame: crate::FrameToken);

    /// Script's navigation of a frame, `asked`, as the operation that
    /// asked for it ends (Blink's `LocalFrameClient::BeginNavigation`,
    /// called from the setter): what goes to the browser goes now, before
    /// script goes on (a navigation to another document, a reload, one
    /// within the document, a traversal, a new tab, the scroll restoration
    /// mode). The number the host gave a navigation to another document it
    /// sent (`BeginNavigation`'s); none for what it didn't send, or isn't
    /// one: what the host does after the call (`FrameRequest::Navigation`)
    /// goes by it. `dom` as it is then.
    fn navigate(&mut self, asked: &NavigationAsked<'_>, dom: &dom::Dom) -> Option<u64>;

    /// An iframe's navigation of its child frame, `asked` (its `src` or
    /// `srcdoc`, HTML's "process the iframe attributes"), as the operation
    /// that asked for it ends: as `navigate`, what goes to the browser
    /// goes now (Blink's frame owner navigates its frame as its attribute
    /// changes or it is inserted), and the number of the navigation sent,
    /// if one was (`FrameRequest::Navigate`).
    fn navigate_child(&mut self, asked: &ChildNavigationAsked<'_>, dom: &dom::Dom) -> Option<u64>;
}

/// An iframe's navigation of its frame, as the engine tells its host's
/// `FrameClient::navigate_child`.
#[derive(Debug, Clone, Copy)]
pub struct ChildNavigationAsked<'a> {
    /// The frame navigated, and its parent: their tokens.
    pub frame: Option<crate::FrameToken>,
    pub parent: Option<crate::FrameToken>,
    /// The iframe.
    pub owner: &'a dom::NodeRoot,
    /// The document the frame shows now, and whether it is the initial
    /// `about:blank` the frame was made with.
    pub document: &'a dom::NodeRoot,
    pub initial: bool,
    pub navigation: &'a crate::ChildNavigation,
    /// The stack of the script that asked, if the DOM wants requests'
    /// stacks (`Dom::request_stacks_wanted`) and script ran.
    pub stack: Option<&'a str>,
}

/// A navigation script asked for, as the engine tells its host's
/// `FrameClient::navigate`.
#[derive(Debug, Clone, Copy)]
pub struct NavigationAsked<'a> {
    /// The frame navigated: its token, the top-level frame's the
    /// browser's (none before the browser told it).
    pub frame: Option<crate::FrameToken>,
    /// The frame whose script asked.
    pub initiator: Option<crate::FrameToken>,
    /// The frame navigated is the top-level one.
    pub top: bool,
    /// The document it shows now.
    pub document: &'a dom::NodeRoot,
    pub navigation: &'a dom::FrameNavigation,
}

/// Where a `fetch()` comes from, as the engine knows it as it is sent:
/// its document's loader (`Cx::document_loader`), which the request goes
/// through; and for DevTools to name it by, its frame's token (the
/// top-level frame's, the browser's), its document's URL and loader
/// (`DocumentLoader::devtools_id`), and the stack of the script that
/// asked, if one was kept (`Dom::take_request_stack`).
#[derive(Debug, Clone, Default)]
pub struct FetchOrigin {
    pub loader: Option<dom::DocumentLoader>,
    pub frame_token: Option<String>,
    pub document_url: Option<String>,
    pub stack: Option<std::rc::Rc<str>>,
    /// An element's load the document's parser asked for: the line it
    /// made the element at (`Cx::take_parser_load_line`).
    pub parser_line: Option<u32>,
    /// A background's: the URL of the stylesheet whose `url()` it is.
    pub stylesheet: Option<std::sync::Arc<str>>,
    /// A module's static `import`: the importing module's URL.
    pub importer: Option<String>,
    /// How the document's load has its subresources use the HTTP cache
    /// (`Cx::subresource_cache_mode`), for a request that doesn't say.
    pub cache_mode: dom::SubresourceCacheMode,
}

/// The frame a document's load goes from, as an engine finds it for the
/// DOM's queued loads (`send_image_fetches`, `send_script_fetches`): the
/// frame, its token (DevTools' name for it) and its document.
pub type LoadFrame = (crate::FrameId, Option<String>, Option<dom::NodeRoot>);

/// Where a load `document` asked for comes from, as its host sends it
/// (`send_image_fetches`, `send_script_fetches`): through the document's
/// loader (the frame's document's, `frame_document`, for one that has none:
/// an initial `about:blank` whose iframe went), named for DevTools by
/// `frame_token`, the document's URL and who asked (`initiator`: the
/// script's stack, the line the parser made the element at, a stylesheet,
/// an importing module); and the origin the request is from (HTML's
/// "create a potential-CORS request": `null` for an opaque one, none for
/// no document).
pub(crate) fn load_origin(
    dom: &dom::Dom,
    document: dom::NodeId,
    frame_token: Option<String>,
    frame_document: Option<&dom::NodeRoot>,
    initiator: &dom::LoadInitiator,
) -> (FetchOrigin, Option<String>) {
    let (loader, document_url, request_origin, parser_line, cache_mode) = dom.read(|cx| {
        let node = cx.node_by_id(document);
        let loader = node
            .and_then(|node| cx.document_loader(node))
            .or_else(|| cx.document_loader(frame_document?.get(cx.gc)));
        let parser_line = match initiator {
            dom::LoadInitiator::Element(element) => cx
                .node_by_id(*element)
                .and_then(|element| cx.take_parser_load_line(element)),
            _ => None,
        };
        (
            loader,
            node.and_then(|node| cx.document_url(node))
                .map(|url| url.to_str().to_string()),
            node.map(|node| match crate::origin(cx, node) {
                Some(origin @ url::Origin::Tuple(..)) => origin.ascii_serialization(),
                _ => "null".to_owned(),
            }),
            parser_line,
            node.map(|node| cx.subresource_cache_mode(node))
                .unwrap_or_default(),
        )
    });
    let stack = match initiator {
        dom::LoadInitiator::Element(element) => {
            dom.take_request_stack(dom::StackAsker::Element(*element))
        }
        dom::LoadInitiator::Script(stack) => Some(stack.clone()),
        _ => None,
    };
    let origin = FetchOrigin {
        loader,
        frame_token,
        document_url,
        stack,
        parser_line,
        stylesheet: match initiator {
            dom::LoadInitiator::Stylesheet(url) => Some(url.clone()),
            _ => None,
        },
        importer: match initiator {
            dom::LoadInitiator::Import(url) => Some(url.clone()),
            _ => None,
        },
        cache_mode,
    };
    (origin, request_origin)
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

    /// Takes the layout back (`set_layout`'s), as the page changes
    /// engines: what it holds of the host's goes to the next one's.
    fn take_layout(&mut self) -> Option<Box<dyn FrameLayout>>;

    /// The top-level browsing context's number.
    fn top_frame(&self) -> crate::FrameId;

    /// Child frame `frame`'s token, which the browser and DevTools call it
    /// by; none for one the engine doesn't have (any more: it goes as its
    /// iframe does). The engine's frames are the one list of them (Blink's
    /// `FrameTree`): the host asks, keeping none of its own.
    fn frame_token(&mut self, _frame: crate::FrameId) -> Option<crate::FrameToken> {
        None
    }

    /// The child frame whose token is `token`, if the engine has it.
    fn frame_of_token(&mut self, _token: crate::FrameToken) -> Option<crate::FrameId> {
        None
    }

    /// Child frame `frame`'s parent and iframe; none for the top-level
    /// frame, or one the engine doesn't have.
    fn frame_parent(&mut self, _frame: crate::FrameId) -> Option<(crate::FrameId, NodeRoot)> {
        None
    }

    /// The child frames of `frame`, in the order they were made.
    fn child_frames(&mut self, _frame: crate::FrameId) -> Vec<crate::FrameId> {
        Vec::new()
    }

    /// `frame`'s document's navigation timing is `timing` (the network's
    /// half, which the browser told at the commit; times since its window's
    /// time origin): what `performance`'s navigation entry and
    /// `performance.timing` read, the document's own times marked on it as
    /// its events fire. Nothing for a frame that went.
    fn set_navigation_timing(&mut self, frame: crate::FrameId, timing: dom::NavigationTiming);

    /// `frame`'s document's body has all come, after it committed: its
    /// navigation timing's `responseEnd` (since its window's time origin)
    /// and body sizes, which only the end tells (`Cx::
    /// navigation_body_ended`). Nothing for a frame that went.
    fn navigation_body_ended(
        &mut self,
        frame: crate::FrameId,
        response_end: f64,
        encoded_body_size: u64,
        decoded_body_size: u64,
    );

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

    /// `document`, `frame`'s, just made (`create_window`), has its origin
    /// from `source` (HTML's: an `about:blank` its initiator navigated it
    /// to), as `source` has it now, before any of its script runs. The
    /// DOM's origin by default; an engine whose realms have their own say
    /// (V8's security tokens) has them follow.
    fn inherit_origin(&mut self, frame: crate::FrameId, document: &NodeRoot, source: &NodeRoot) {
        let _ = frame;
        self.dom_mut()
            .mutate(|cx| cx.set_origin_source(document.get(cx.gc), source.get(cx.gc)));
    }

    /// What the engine asks the host to do for its frames since the last
    /// time (a child to navigate, a navigation script made, a blob URL
    /// registered or revoked), in order, taken.
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
    /// (serialized; none: `null`) and scroll restoration mode; `committed`
    /// called then, before `popstate` fires (for the host to tell the
    /// browser first), `hashchange` in a task if the fragment changed.
    /// Whether the frame shows a document with a window.
    fn traverse_within_document(
        &mut self,
        frame: crate::FrameId,
        url: &str,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
        committed: &mut dyn FnMut(),
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

    /// Runs the DOM's task numbered `number`, the one a token of the
    /// host's scheduler stands for (`Dom::set_task_waker`), in its window's
    /// realm with its microtask checkpoint: if it is still next in its
    /// queue (`Cx::task_numbered`); its time come, as the scheduler took
    /// it. Whether it ran. Nothing by default: an engine whose DOM tells
    /// no scheduler.
    fn run_dom_task(&mut self, number: u64) -> bool {
        let _ = number;
        false
    }

    /// The messages that came to the windows' enabled ports from other
    /// threads, numbered as the DOM's tasks (each told to the host's
    /// scheduler) if they weren't (`Cx::number_port_messages`).
    fn number_port_messages(&mut self) {}

    /// How long until a guest of the engine's own waits no more (a wasm
    /// guest's wait, which isn't a task of the DOM's); none with none.
    fn until_due_guests(&mut self) -> Option<std::time::Duration> {
        None
    }

    /// Whether a task of the DOM's in the windows was due `ms` or more
    /// before now (the DOM's clock): with nothing in the host's scheduler
    /// for it, its token was lost and it never runs (debug builds check
    /// as the thread is to wait). None by default.
    fn has_dom_task_due_for(&mut self, ms: f64) -> bool {
        let _ = ms;
        false
    }

    /// The guests whose waits are over (`until_due_guests`), run. Whether
    /// one ran.
    fn run_due_guests(&mut self) -> bool {
        false
    }

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

    /// What opens the frames' `localStorage` and hears what script changes
    /// in it (`StorageHost`), the host's. An engine without `localStorage`
    /// drops it.
    fn set_storage_host(&mut self, host: Box<dyn crate::StorageHost>) {
        let _ = host;
    }

    /// What reads and writes the frames' cookies (`CookieHost`), the
    /// host's. An engine without `document.cookie` drops it.
    fn set_cookie_host(&mut self, host: Box<dyn crate::CookieHost>) {
        let _ = host;
    }

    /// The browser's news of a change to the `localStorage` area `area`,
    /// which this engine's DOM opened (`dom`'s `storage_area_changed`):
    /// taken into the DOM's copy, `storage` events queued at its windows.
    /// An engine without `localStorage` drops it.
    fn storage_area_changed(
        &mut self,
        area: u64,
        mutation: dom::StorageMutation,
        old_value: Option<String>,
        changed: bool,
        url: String,
        source: Option<u64>,
    ) {
        let _ = (area, mutation, old_value, changed, url, source);
    }

    /// The browser refused a change this engine's DOM made to the
    /// `localStorage` area `area` (`dom`'s `storage_change_failed`): `key`
    /// is `value` again. An engine without `localStorage` drops it.
    fn storage_change_failed(&mut self, area: u64, key: &str, value: Option<String>) {
        let _ = (area, key, value);
    }

    /// Runs the frames' windows' element tasks, a window's after another's
    /// (`dom_api::run_element_tasks`: a text control's `select`), a
    /// microtask checkpoint after each: whether any ran.
    fn run_element_tasks(&mut self) -> bool;

    /// Runs the frames' windows' messages (`run_posted_messages`: HTML's
    /// posted message task source), a window's after another's, a
    /// microtask checkpoint after each: whether any ran.
    fn run_posted_messages(&mut self) -> bool;

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

    /// What wakes the host's thread for the engine's platform tasks (none
    /// is due: now; else the instant the next is), as they are posted from
    /// any thread: the host runs them in a task of its own
    /// (`run_platform_tasks`). Nothing by default: an engine whose
    /// platform's tasks the host's loop looks for itself.
    fn set_platform_task_waker(
        &mut self,
        waker: std::sync::Arc<dyn Fn(Option<std::time::Instant>) + Send + Sync>,
    ) {
        let _ = waker;
    }

    /// What stops the script the engine runs, from any thread, until its
    /// stack has unwound (V8's `TerminateExecution`): a renderer's owner's
    /// in its own process, as machopo ends while a page's script runs on.
    fn terminator(&mut self) -> Box<dyn Fn() + Send>;

    /// Where `console` messages go.
    fn set_console_sink(&mut self, sink: ConsoleSink);

    /// What sends the loads script asks for as it asks (`LoadSink`): the
    /// host's, in place of the frame requests for blob URLs
    /// (`FrameRequest::RegisterBlobUrl`, `RevokeBlobUrl`). Without one, a
    /// `fetch()` goes nowhere: its promise stays pending.
    fn set_load_sink(&mut self, sink: Box<dyn LoadSink>);

    /// The images the DOM queued to fetch, sent now through the load sink
    /// (`send_image_fetches`): those queued outside the engine's calls (a
    /// background the host's style update found) and any still waiting.
    /// Nothing by default.
    fn send_image_fetches(&mut self) {}

    /// The scripts the DOM queued to fetch, sent now through the load sink
    /// (`send_script_fetches`): those queued outside the engine's calls
    /// (a module's imports its response brought, the parser's) and any
    /// still waiting. Nothing by default.
    fn send_script_fetches(&mut self) {}

    /// Whether a child frame was made or went since the last ask: what the
    /// host keeps of its frames follows them then (`child_frames`,
    /// `frame_parent`). None by default: an engine with no child frames.
    fn take_frames_changed(&mut self) -> bool {
        false
    }

    /// What the engine tells of its child frames as they are made and go,
    /// and of script's navigations as they are asked (`FrameClient`).
    /// Nothing by default: an engine with no frames to tell of.
    fn set_frame_client(&mut self, client: Box<dyn FrameClient>) {
        let _ = client;
    }

    /// What `testRunner.reportHarnessResults` was given since the last
    /// ask, if anything (web tests only).
    fn take_harness_results(&mut self) -> Option<String>;

    /// What starts the dedicated workers its realms make and carries what
    /// goes to them (`new Worker()` throws `NotSupportedError` without
    /// one).
    fn set_worker_host(&mut self, host: Box<dyn crate::WorkerHost>) {
        let _ = host;
    }

    /// `message` from the worker numbered `id`: a task of its `Worker`'s
    /// realm, a `message` event at the `Worker` as it runs. Nothing if the
    /// `Worker` was terminated.
    fn worker_message(&mut self, id: u64, message: dom::SerializedMessage) {
        let _ = (id, message);
    }

    /// What the worker numbered `id` threw and its global didn't cancel: an
    /// `error` event at its `Worker`, reported at its realm's global if not
    /// canceled (`worker_error`).
    fn worker_error(&mut self, id: u64, error: &crate::WorkerError) {
        let _ = (id, error);
    }

    /// The worker numbered `id` closed itself (`close()`): its `Worker`
    /// gets nothing more of it (what it posted before still comes, in its
    /// tasks' turn), and isn't kept alive by it any more.
    fn worker_closed(&mut self, id: u64) {
        let _ = id;
    }

    /// The script of the worker numbered `id` couldn't be had (HTML's "run
    /// a worker": an `error` event at its `Worker`, which gets nothing more
    /// of it).
    fn worker_script_failed(&mut self, id: u64) {
        let _ = id;
    }

    /// Whether the host is waiting for its next task (`idle`) or in one,
    /// though not in script, for DevTools' `Profiler` to tell the two
    /// apart (`(idle)`, `(program)`), as Blink tells V8's inspector around
    /// each task. Not while script runs.
    fn set_idle(&mut self, idle: bool);

    /// The top-level frame's token, the browser's: what DevTools calls it
    /// (`Page.Frame`'s `id`), as it does a child frame by the token the
    /// engine drew for it. Each realm is told to the engine's inspector
    /// with its frame's as it is made: those of the top-level frame made
    /// before this have none.
    fn set_main_frame_token(&mut self, token: crate::FrameToken);

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

/// What a dedicated worker's global is made with (Blink's
/// `GlobalScopeCreationParams`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerGlobalParams {
    /// Its URL: its script's response's.
    pub url: url::Url,
    pub origin: url::Origin,
    pub name: String,
    /// Its time origin: when its page made it (`new Worker()`), as
    /// Blink's `WorkerThread` takes the time as it is made.
    pub time_origin: std::time::Instant,
    /// The browser's user agent string, and whether automation controls it
    /// (its `navigator`'s, as a window's are the host's: `Dom::set_agent`).
    pub user_agent: String,
    pub webdriver: bool,
    /// A module worker's (`type: "module"`): `importScripts()` throws.
    pub module: bool,
}

/// A dedicated worker's engine, on the worker's thread (Blink's
/// `WorkerOrWorkletScriptController`, of the worker's global): its realm,
/// whose global is a worker's global, and what runs its script and its
/// tasks. The worker's thread's loop drives it, as a frame's drives a
/// `ScriptEngine`, one task at a time.
pub trait WorkerEngine {
    /// Its realm, made: its global a dedicated worker's global of `params`,
    /// what it sends to its `Worker` going to `host`.
    fn create_global(&mut self, params: WorkerGlobalParams, host: Box<dyn crate::WorkerGlobalHost>);

    /// Runs its classic script (its main script's), what it throws reported
    /// at its global (HTML's "run a classic script"); a microtask
    /// checkpoint after. False if it didn't parse: nothing ran or was
    /// reported (HTML's "run a worker" then fires a plain `error` at its
    /// `Worker`, as for a script that couldn't be had).
    fn run_classic_script(&mut self, script: &ClassicScript<'_>) -> bool;

    /// A message from its `Worker`: a task of its global's, a `message` at
    /// it as it runs.
    fn post_message(&mut self, message: dom::SerializedMessage);

    /// The response to its `fetch()` numbered `id` (`Err` a network
    /// error), in a task of its own: its promise settled, a microtask
    /// checkpoint after. Nothing if it was aborted.
    fn fetch_response(&mut self, id: u64, response: Result<crate::FetchedResponse, ()>);

    /// How long until it has a task to run (none with none: it waits for a
    /// message).
    fn until_next_task(&mut self) -> Option<std::time::Duration>;

    /// Runs its engine's platform tasks that are due (V8's foreground
    /// tasks), from its outermost loop: whether any ran. None by default.
    fn run_platform_tasks(&mut self) -> bool {
        false
    }

    /// What wakes its thread for its engine's platform tasks, as they are
    /// posted from any thread (`ScriptEngine::set_platform_task_waker`).
    fn set_platform_task_waker(
        &mut self,
        waker: std::sync::Arc<dyn Fn(Option<std::time::Instant>) + Send + Sync>,
    ) {
        let _ = waker;
    }

    /// Runs its tasks due now, each with its microtask checkpoint, in the
    /// DOM's order (`dom::Cx::next_task`). Whether one ran.
    fn run_due_tasks(&mut self) -> bool;

    /// What tells its thread's scheduler of each DOM task as its DOM
    /// numbers it (`dom::Dom::set_task_waker`): the thread runs each in its
    /// turn (`run_dom_task`), as the main thread runs a window's.
    fn set_task_waker(&mut self, waker: dom::TaskWaker);

    /// Runs the DOM task numbered `number` alone, wherever it is in its
    /// queue (a token of its thread's scheduler's), with its microtask
    /// checkpoint: whether it ran (none if it has gone: cancelled, or run).
    fn run_dom_task(&mut self, number: u64) -> bool;

    /// Numbers the messages its ports got from other threads, each a token
    /// to its thread's scheduler (`dom::Cx::number_port_messages`).
    fn number_port_messages(&mut self);

    /// What stops its script from another thread (V8's
    /// `TerminateExecution`): `Worker.terminate()`'s.
    fn terminator(&mut self) -> Box<dyn Fn() + Send>;

    /// Where `console` messages go.
    fn set_console_sink(&mut self, sink: ConsoleSink);

    /// What `testRunner.reportHarnessResults` was given since the last
    /// ask, if anything (web tests only).
    fn take_harness_results(&mut self) -> Option<String>;

    /// What starts the dedicated workers its realm makes (a worker made in
    /// a worker: HTML's nested worker) and carries what goes to them, as a
    /// page's engine's (`ScriptEngine::set_worker_host`): `new Worker()`
    /// throws `NotSupportedError` without one.
    fn set_worker_host(&mut self, host: Box<dyn crate::WorkerHost>) {
        let _ = host;
    }

    /// `message` from the worker it made numbered `id`
    /// (`ScriptEngine::worker_message`'s).
    fn worker_message(&mut self, id: u64, message: dom::SerializedMessage) {
        let _ = (id, message);
    }

    /// What the worker it made numbered `id` threw
    /// (`ScriptEngine::worker_error`'s).
    fn worker_error(&mut self, id: u64, error: &crate::WorkerError) {
        let _ = (id, error);
    }

    /// The worker it made numbered `id` closed itself
    /// (`ScriptEngine::worker_closed`'s).
    fn worker_closed(&mut self, id: u64) {
        let _ = id;
    }

    /// The script of the worker it made numbered `id` couldn't be had
    /// (`ScriptEngine::worker_script_failed`'s).
    fn worker_script_failed(&mut self, id: u64) {
        let _ = id;
    }

    /// A module worker's script graph (HTML's "fetch a module worker
    /// script graph"): its root its main script, fetched (by the browser)
    /// as `url`, its descendants through its host
    /// (`WorkerGlobalHost::start_module_fetch`) with `credentials`. What
    /// became of it, if it is done now: `Some(true)` run (evaluated, a
    /// microtask checkpoint after), `Some(false)` a module failed to fetch
    /// or parse (HTML's "run a worker": an `error` at its `Worker`, and it
    /// runs no more); none: its modules are yet to come (`module_fetched`).
    fn run_module_graph(
        &mut self,
        url: &url::Url,
        root: crate::module::FetchedModule,
        credentials: dom::ScriptCors,
    ) -> Option<bool> {
        let _ = (url, root, credentials);
        Some(false)
    }

    /// A module its realm asked for (`WorkerGlobalHost::start_module_fetch`'s
    /// `id`), fetched (none: it failed): its graphs move on, an `import()`'s
    /// settled, in a task of its own. What became of its module worker's
    /// graph, as `run_module_graph`'s, if it is done now.
    fn module_fetched(
        &mut self,
        id: u64,
        response: Option<crate::module::FetchedModule>,
    ) -> Option<bool> {
        let _ = (id, response);
        None
    }
}

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
