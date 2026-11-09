//! The DOM's side of the renderer: a navigation commits a document of
//! the DOM (`FrameDocument`) in a realm of the DOM's engine
//! (`dom_api::ScriptEngine`), parsed by the parser that pauses for
//! scripts (`html::parser::Parser`), its parser-blocking and deferred
//! classic scripts run in order (an external one fetched first, none while
//! a stylesheet blocks scripts), the DOM's other external scripts fetched
//! for it (`Dom::take_script_fetches`), shown by its own frame
//! view (`FrameView`: style, layout and paint of the DOM), told
//! to DevTools' `Page` agent as the main frame; its iframes' child frames
//! load and show their documents alike (`ChildFrames`).

use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use compositor::DocumentId;
use dom::{Dom, NodeRoot};
use dom_api::FrameLayout as _;
use dom_api::{
    ChildNavigation, ClassicScript, DocumentReadiness, FrameId, FrameRequest, KeyInput, KeyResult,
    MouseAction, MouseInput, Prepared, ScriptEngine, WheelInput,
};
use gfx::PointF;
use html::parser::{Parser, ParserState, SinkHandle};
use protocol::input::{InputEvent, Modifiers, MouseButton};
use protocol::network::{
    CredentialsMode, LoadResult, RequestDestination, RequestMode, ResourceRequest, ResponseTainting,
};
use protocol::renderer::{
    CursorType, MenuItem, MenuItemKind, PopupMenu, PopupMenuResult, RendererHostMsg,
};
use url::Url;

use super::network_agent::RequestFrame;
use super::page_agent::{FrameInfo, FrameResources};
use super::{ChildNavigationTiming, RenderThread, SwapPromise};
use crate::frame_view::{Frame, FrameView};
use crate::initiator::Initiator;
use crate::pending_user_input::PendingUserInputMonitor;
use crate::svg_image::SvgImage;
use crate::{document, encoding};

/// A committed document of the DOM: its navigation and URL, its
/// parser until the input is all parsed, its parser-blocking script and
/// deferred scripts.
pub(super) struct FrameDocument {
    /// The browsing context it is the document of; a child frame's
    /// parent and iframe.
    frame: FrameId,
    child_of: Option<(FrameId, NodeRoot)>,
    navigation_id: u64,
    url: Url,
    /// Its MIME type and markup as they came, for DevTools.
    mime_type: String,
    source: Rc<str>,
    parser: Option<Parser<dyn ScriptEngine>>,
    /// HTML's "pending parsing-blocking script", and its "list of scripts
    /// that will execute when the document has finished parsing" (Blink's
    /// `HTMLParserScriptRunner`'s).
    blocking_script: Option<ParserScript>,
    deferred_scripts: std::collections::VecDeque<ParserScript>,
    /// The document; its view is the engine's (`engine_frame`).
    document: NodeRoot,
    /// The DOM's tree version the rendering was last asked for at, and its
    /// intersection observations' (an observer made, a target observed:
    /// the next rendering update computes them).
    pub(super) rendered_version: u64,
    rendered_intersections: u64,
    /// The DOM's resize observations' version the rendering was last asked
    /// for at (a target observed: the next rendering update gathers it).
    rendered_resizes: u64,
    /// How many times its canvases drew when the rendering was last asked
    /// for, or last updated.
    rendered_canvas_draws: u64,
    /// The document's `load` came and the frame that shows it hasn't been
    /// committed yet: `DidFinishLoad` goes with it.
    pub(super) load_finished: bool,
    /// `DidFinishLoad` said the root element had `reftest-wait`:
    /// `ReftestWaitEnded` goes with the frame that shows it gone.
    waiting_for_reftest: bool,
    /// What the document's text was decoded as: its sheets' default.
    encoding: &'static encoding_rs::Encoding,
    /// The `<link>`s whose sheet was asked for, by element: each loads
    /// once.
    requested_links: dom::NodeIdHashSet,
    /// The `<link>`s' sheets loading, by request.
    pending_sheets: HashMap<u64, PendingSheet>,
    /// Where its loading is: `load` waits for the sheets.
    readiness: Readiness,
    /// When its network goes quiet, for DevTools (Blink's
    /// `IdlenessDetector`, one a frame), from `DOMContentLoaded` on.
    idleness: crate::idleness_detector::IdlenessDetector,
    /// The title last sent to the browser, and the tree's version it was
    /// read at.
    sent_title: Option<String>,
    title_version: u64,
    /// The line each element its parser made that loads something starts
    /// on (1-based), until its load starts: DevTools names the parser as
    /// having asked for it there.
    parser_lines: dom::NodeIdHashMap<u32>,
    /// What DevTools calls its loader, if not its document's number: a
    /// child frame's document's request (the top-level one's is its
    /// navigation's).
    loader_id: Option<String>,
    /// The subresources it loaded, for DevTools' `Page.getResourceTree`
    /// (shared with what the `Page` agent was told: copied as it changes
    /// after).
    resources: Rc<FrameResources>,
    /// A child frame's initial `about:blank`, which no navigation brought.
    initial: bool,
    /// The URLs of its subresources that failed to load, or whose content
    /// was of no use (an image that doesn't decode, a script of no
    /// JavaScript type, a `<link>`'s `href` that doesn't parse), until its
    /// `DidFinishLoad` tells the browser.
    failed: Vec<String>,
}

/// A `<link>`'s sheet being fetched: its element, whether it blocks
/// rendering (one that isn't alternate does), and whether it blocks the
/// parser's scripts (the document's parser made it: HTML's "script-blocking
/// style sheet", Blink's pending script-blocking sheets).
struct PendingSheet {
    link: NodeRoot,
    /// The URL asked for: the sheet's `href`.
    url: Url,
    blocking: bool,
    blocks_scripts: bool,
}

/// How far the document's loading is (HTML's "the end").
#[derive(Clone, Copy, PartialEq, Eq)]
enum Readiness {
    Loading,
    /// Parsed and interactive: its deferred scripts run, then
    /// `DOMContentLoaded`.
    Deferring,
    /// Parsed, its `DOMContentLoaded` fired; `load` waits for the sheets.
    Parsed,
    /// Its window's `load` task queued (`Cx::queue_window_load`).
    LoadQueued,
    Loaded,
}

/// A script the parser waits for, or that runs after parsing: its
/// element, its fetch (none for an inline one), and once it is ready what
/// it runs (none: it failed to load, `error` is due).
struct ParserScript {
    element: NodeRoot,
    /// A deferred module script: the DOM holds what its graph fetched
    /// (`Cx::pending_script_fetched`), and the engine runs it.
    module: bool,
    request_id: Option<u64>,
    /// An external one's `charset` attribute's label.
    charset: Option<String>,
    ready: Option<Option<ParserSource>>,
}

/// What the parser's script runs: an inline one's text (the line it
/// starts on, and its base URL as it was prepared), or what an external
/// one fetched.
enum ParserSource {
    Inline {
        source: String,
        line: u32,
        base_url: Option<String>,
    },
    External(dom::LoadedScript),
}

/// A script the DOM queued (`Dom::take_script_fetches`) being fetched: its
/// number, to hand it back by, the frame whose document it is, and its
/// `charset` attribute's label.
pub(super) struct PendingScriptFetch {
    key: u64,
    frame: FrameId,
    charset: Option<String>,
    /// A module the engine's module map asked for.
    module: bool,
}

impl FrameDocument {
    /// `frame`'s new document (a child frame's, of `child_of`), loading:
    /// nothing asked for, run or rendered yet.
    #[expect(clippy::too_many_arguments, reason = "a document has that many parts")]
    fn new(
        frame: FrameId,
        child_of: Option<(FrameId, NodeRoot)>,
        url: Url,
        mime_type: &str,
        source: Rc<str>,
        parser: Option<Parser<dyn ScriptEngine>>,
        document: NodeRoot,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        Self {
            frame,
            child_of,
            navigation_id: 0,
            url,
            mime_type: mime_type.to_owned(),
            source,
            parser,
            blocking_script: None,
            deferred_scripts: std::collections::VecDeque::new(),
            document,
            rendered_version: 0,
            rendered_intersections: 0,
            rendered_resizes: 0,
            rendered_canvas_draws: 0,
            load_finished: false,
            waiting_for_reftest: false,
            encoding,
            requested_links: dom::NodeIdHashSet::default(),
            pending_sheets: HashMap::new(),
            readiness: Readiness::Loading,
            idleness: crate::idleness_detector::IdlenessDetector::default(),
            sent_title: None,
            title_version: 0,
            parser_lines: dom::NodeIdHashMap::default(),
            loader_id: None,
            resources: Rc::default(),
            initial: false,
            failed: Vec::new(),
        }
    }

    pub(super) fn navigation_id(&self) -> u64 {
        self.navigation_id
    }
}

/// The DOM's child frames (`FrameRequest::Created`): each one's parent
/// and iframe, the documents loading in them, and the `src`s being
/// fetched for them, by request.
#[derive(Default)]
pub(super) struct ChildFrames {
    frames: HashMap<FrameId, (FrameId, NodeRoot)>,
    documents: Vec<FrameDocument>,
    /// A child frame's `src` being fetched: its frame, the URL, when its
    /// navigation started (its next document's time origin), and what its
    /// navigation timing reads of it.
    fetches: HashMap<u64, (FrameId, Url, std::time::Instant, ChildNavigationTiming)>,
    /// A child frame's `srcdoc` or `about:` document, which needs no fetch
    /// but commits in a task of its own all the same, as a response's
    /// would (Chrome's goes through a navigation request too): its frame,
    /// its URL and its markup, in order (`run_child_commits`).
    commits: std::collections::VecDeque<(FrameId, Url, String)>,
    /// The stack of the script whose iframe made each frame, if DevTools
    /// wanted it then (`Page.frameAttached`'s).
    creation_stacks: HashMap<FrameId, Rc<str>>,
}

impl ChildFrames {
    /// `frame` and the frames below it gone: their documents and fetches.
    fn remove(&mut self, frame: FrameId) {
        let mut gone = vec![frame];
        let mut at = 0;
        while let Some(&parent) = gone.get(at) {
            gone.extend(
                self.frames
                    .iter()
                    .filter(|(_, (of, _))| *of == parent)
                    .map(|(id, _)| *id),
            );
            at = at.saturating_add(1);
        }
        self.frames.retain(|id, _| !gone.contains(id));
        self.documents
            .retain(|document| !gone.contains(&document.frame));
        self.fetches.retain(|_, (id, ..)| !gone.contains(id));
        self.commits.retain(|(id, _, _)| !gone.contains(id));
        self.creation_stacks.retain(|id, _| !gone.contains(id));
    }

    /// `frame`'s navigation started: one it started before doesn't
    /// commit (one navigation a frame).
    fn navigating(&mut self, frame: FrameId) {
        self.fetches.retain(|_, (id, ..)| *id != frame);
        self.commits.retain(|(id, _, _)| *id != frame);
    }

    /// Whether a child of `frame` is still loading: its `src` fetched, its
    /// document to commit, or not loaded (HTML's: an iframe delays its
    /// document's `load` event).
    fn loading_in(&self, frame: FrameId) -> bool {
        let child_of_frame = |id: &FrameId| {
            self.frames
                .get(id)
                .is_some_and(|(parent, _)| *parent == frame)
        };
        self.fetches.values().any(|(id, ..)| child_of_frame(id))
            || self.commits.iter().any(|(id, _, _)| child_of_frame(id))
            || self.documents.iter().any(|document| {
                document
                    .child_of
                    .as_ref()
                    .is_some_and(|(parent, _)| *parent == frame)
                    && document.readiness != Readiness::Loaded
            })
    }
}

/// A committed document's content type (its MIME type's essence: the
/// type and subtype, lowercased), the encoding its bytes were decoded as
/// (`contentType`, `characterSet`), and its response's `Content-Language`
/// (its content language: `:lang()`'s, with no `lang` around).
fn set_document_metadata(
    dom: &mut Dom,
    document: &NodeRoot,
    mime_type: &str,
    encoding: &'static encoding_rs::Encoding,
    content_language: Option<&str>,
) {
    let essence = mime_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        if !essence.is_empty() {
            cx.set_document_content_type(document, &essence);
        }
        cx.set_document_character_set(document, encoding.name());
        cx.set_content_language(document, content_language);
    });
}

/// The main frame, DevTools' `token`, and its document of navigation
/// `navigation_id` as DevTools tells them; none before DevTools gave the
/// frame its token.
fn main_frame_info(
    token: &str,
    navigation_id: u64,
    url: &Url,
    mime_type: &str,
    source: &Rc<str>,
    resources: &Rc<FrameResources>,
) -> Option<FrameInfo> {
    if token.is_empty() {
        return None;
    }
    // As Blink's `SecurityOrigin::ToRawString`: an opaque one's scheme
    // and host are empty.
    let origin = match url.origin() {
        origin @ url::Origin::Tuple(..) => origin.ascii_serialization(),
        url::Origin::Opaque(_) => "://".to_owned(),
    };
    Some(FrameInfo {
        id: token.to_owned(),
        parent_id: None,
        loader_id: navigation_id.to_string(),
        document: navigation_id,
        name: None,
        initial: false,
        navigating: false,
        url: url.to_string(),
        origin,
        mime_type: mime_type.to_owned(),
        secure_context: super::devtools::secure_context(url.as_str(), None),
        source: source.clone(),
        resources: resources.clone(),
        creation_stack: None,
    })
}

/// The frame's lifecycle brought up to date and its paint handed to its
/// widget's layers, DevTools' overlay over them (Blink's
/// `WebFrameWidgetImpl::UpdateLifecycle` and `LayerTreeHost::DoUpdateLayers`):
/// what the event loop's rendering update and script paused's draw alike,
/// before they commit.
pub(super) fn update_lifecycle_and_layers(
    dom: &mut Dom,
    frame: &mut Frame,
    overlay_agent: &super::overlay_agent::OverlayAgent,
    css_agent: &super::css_agent::CssAgent,
) {
    let lifecycle = trace_event::trace_event!("WebFrameWidgetImpl::UpdateLifecycle");
    // What the canvases shown recorded, onto their bitmaps.
    trace_event::in_scope(trace_event::trace_event!("canvas::flush_shown"), || {
        frame.flush_shown_canvases(dom)
    });
    frame.widget.rendered_tree_version = dom.read(|cx| cx.tree_version());
    let output = frame.update_all_lifecycle_phases(dom);
    drop(lifecycle);
    // The pseudo-elements that came and went, for DevTools' `DOM`; the
    // node its Computed pane follows, if its style changed, and the sheets,
    // if they did.
    css_agent.dom_agent().lifecycle_updated(dom, frame);
    css_agent.styles_updated(dom, frame);
    css_agent.sheets_updated(dom, frame);
    let update_layers = trace_event::trace_event!("LayerTreeHost::DoUpdateLayers");
    let unchanged = frame
        .widget
        .painted
        .as_ref()
        .is_some_and(|last| Arc::ptr_eq(last, &output));
    // DevTools' overlay, over the page's layers, drawn again only as it
    // changes.
    frame.widget.overlay_version = overlay_agent.version();
    let overlay = dom.read(|cx| overlay_agent.layer(cx, frame));
    let overlay_changed = overlay.is_some();
    if let Some(layer) = overlay {
        frame.widget.overlay_layer = layer;
    }
    let Frame { view, widget, .. } = frame;
    if !unchanged || overlay_changed {
        let mut layers = output.layers.clone();
        layers.extend(widget.overlay_layer.clone().map(compositor::Layer::from));
        widget
            .layer_tree_host
            .set_layers(layers, output.property_trees.clone());
        widget.painted = Some(output);
    }
    let host = &mut widget.layer_tree_host;
    host.set_scroll_offsets(view.compositor_scroll_offsets());
    host.set_background_color(gfx::Color::WHITE);
    host.set_root_scroll_offset(view.viewport_scroll_offset());
    host.set_document_id(view.document_id());
    host.set_viewport_scroll_node(view.viewport_scroll_node());
    drop(update_layers);
}

pub(super) fn engine_frame(
    engine: &mut Option<Box<dyn ScriptEngine>>,
) -> Option<(&mut Dom, &mut Frame)> {
    let (dom, layout) = engine.as_mut()?.dom_and_layout();
    Some((dom, layout?.as_any_mut().downcast_mut::<Frame>()?))
}

impl RenderThread {
    /// `commit_navigation` for the DOM: a new realm and document at
    /// `url` (its time origin the navigation's start, `time_origin` on the
    /// renderer's clock), the decoded body pushed to its parser, then
    /// parsed and its scripts run.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn commit_document_navigation(
        &mut self,
        navigation_id: u64,
        url: String,
        mime_type: &str,
        charset: Option<&str>,
        content_language: Option<&str>,
        body: &[u8],
        history: &protocol::renderer::HistoryEntryParams,
        time_origin: f64,
        timing: &crate::navigation_timing::NavigationTiming,
    ) {
        let _span = trace_event::info_event!("commit_navigation", %url);
        let document_id = self.new_document_id();
        if self.engine.is_none() {
            self.engine = Some(self.make_engine());
        }
        let engine = self.engine.as_mut().expect("just made");
        // The system's focus may have come before the first document (and
        // its engine): the window starts with it.
        engine.set_page_focused(self.page_focused);
        // And the page's visibility (a background tab opens hidden).
        engine.set_page_visible(!self.page_hidden);
        let (text, document_encoding) = encoding::decode_document(mime_type, charset, body);
        let markup = document::markup_for(mime_type, &text);
        let source: Rc<str> = Rc::from(&*text);
        let document_url =
            Url::parse(&url).unwrap_or_else(|_| Url::parse("about:blank").expect("about:blank"));
        // The browser's milliseconds since the clock's start, before it for
        // a navigation that started before this renderer did (its network
        // times count from there too); one not a number or past what an
        // `Instant` holds is the start.
        let origin = self
            .clock
            .instant_at(time_origin)
            .unwrap_or(self.clock.start());
        let top = engine.top_frame();
        // The realm before it goes, and the blob URLs it made.
        self.blob_urls.revoke_frame(top);
        // DevTools hears of the frame's new document before its realm (as
        // Chrome tells `Page.frameNavigated` before
        // `Runtime.executionContextCreated`): the old document's frames go
        // with it.
        if !self.devtools_sessions.is_empty()
            && let Some(main) = main_frame_info(
                &self.devtools_main_frame_token,
                navigation_id,
                &document_url,
                mime_type,
                &source,
                &Rc::default(),
            )
        {
            self.page_agent.update(vec![main]);
        }
        // The document it shows unloaded first, its `unload` timed.
        let previous = self.document.as_ref().map(|next| next.url.clone());
        let clock = self.clock;
        let unloaded = engine
            .unload_document(top)
            .map(|(start, end)| (clock.at(start), clock.at(end)));
        let timing = crate::navigation_timing::with_unload(
            timing.clone(),
            unloaded,
            previous.as_ref(),
            &document_url,
        );
        let document = engine
            .create_window(top, document_url.as_str(), origin)
            .expect("the top-level frame is the engine's");
        let document_node = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
        self.dom_agent.document_committed(document_node);
        engine.set_navigation_timing(
            top,
            crate::navigation_timing::document_timing(&timing, time_origin, self.clock.now()),
        );
        set_document_metadata(
            engine.dom_mut(),
            &document,
            mime_type,
            document_encoding,
            content_language,
        );
        // Where its entry is in the session history, and its state.
        engine.set_session_history(dom::SessionHistory {
            index: history.index,
            length: history.length,
        });
        engine.restore_history(
            top,
            history.state.clone(),
            history.manual_scroll_restoration,
        );
        let mut parser = Parser::new(engine.dom_mut(), &document);
        parser.push(&markup);
        let node = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
        let view = FrameView::new(document.clone(), node, document_id, self.system_colors);
        match engine
            .dom_and_layout()
            .1
            .and_then(|layout| layout.as_any_mut().downcast_mut::<Frame>())
        {
            Some(frame) => frame.show(view, node),
            None => {
                // One token a thread, for every document of the DOM it
                // shows; the thread's text context is the frame's from now
                // on.
                let text = std::mem::replace(&mut self.text, layout::TextContext::new(true));
                let widget = self
                    .widget
                    .take()
                    .expect("the thread's widget, before any frame");
                let frame = Frame::new(view, node, dom::LocalToken::new(), text, widget);
                engine.set_layout(Box::new(frame));
            }
        }
        // What `isInputPending()` reads: the counts, and the document drawn
        // now, which input aimed at no child frame is for.
        engine.set_input_monitor(Box::new(PendingUserInputMonitor::new(
            self.pending_input.clone(),
            document_id,
            node,
        )));
        self.set_viewport();
        // The child frames of the document before go with it.
        self.children = ChildFrames::default();
        self.document = Some(FrameDocument {
            navigation_id,
            ..FrameDocument::new(
                top,
                None,
                document_url,
                mime_type,
                source,
                Some(parser),
                document,
                document_encoding,
            )
        });
        self.request_main_frame();
        let _ = self
            .to_browser
            .send(RendererHostMsg::DidCommitNavigation { navigation_id, url });
        // The pointer may have been over a link of the old document: the
        // arrow until it moves, or the new one is laid out under it.
        self.set_cursor(CursorType::Pointer);
        self.advance_document(top);
    }

    /// The document loading in `frame` (the top-level one's or a child's).
    fn doc_mut(&mut self, frame: FrameId) -> Option<&mut FrameDocument> {
        if self
            .document
            .as_ref()
            .is_some_and(|next| next.frame == frame)
        {
            return self.document.as_mut();
        }
        self.children
            .documents
            .iter_mut()
            .find(|next| next.frame == frame)
    }

    fn doc(&self, frame: FrameId) -> Option<&FrameDocument> {
        match &self.document {
            Some(next) if next.frame == frame => Some(next),
            _ => self
                .children
                .documents
                .iter()
                .find(|next| next.frame == frame),
        }
    }

    /// The frames whose documents are loading: the top-level one's, then
    /// the children's.
    fn frames(&self) -> Vec<FrameId> {
        self.document
            .iter()
            .chain(&self.children.documents)
            .map(|next| next.frame)
            .collect()
    }

    /// What the engine asked for its frames (`ScriptEngine::
    /// take_frame_requests`), after the script that made it so: a child
    /// frame's view shown in its parent's, its navigation started, a frame
    /// that went dropped with its view and loading.
    pub(super) fn handle_frame_requests(&mut self) {
        loop {
            let Some(engine) = &mut self.engine else {
                return;
            };
            let requests = engine.take_frame_requests();
            if requests.is_empty() {
                return;
            }
            for request in requests {
                match request {
                    FrameRequest::Created {
                        frame,
                        parent,
                        owner,
                        document,
                    } => {
                        let document_id = self.new_document_id();
                        let Some((dom, next)) = engine_frame(&mut self.engine) else {
                            continue;
                        };
                        let (node, owner_id) = dom.read(|cx| {
                            (
                                cx.node_id(document.get(cx.gc)),
                                cx.node_id(owner.get(cx.gc)),
                            )
                        });
                        // Shown already as its iframe was inserted
                        // (`child_frame_created`), unless its parent's
                        // frame wasn't yet: given its compositor id.
                        match next.view.child_mut(frame) {
                            Some(view) => view.set_document_id(document_id),
                            None => {
                                let view = FrameView::new(
                                    document.clone(),
                                    node,
                                    document_id,
                                    self.system_colors,
                                )
                                .into_child(frame, owner_id);
                                let top = self.document.as_ref().map(|next| next.frame);
                                next.view
                                    .show_child((Some(parent) != top).then_some(parent), view);
                            }
                        }
                        // The stack of the script whose iframe made it, for
                        // DevTools' `Page.frameAttached` (its navigation
                        // takes it too).
                        if self.page_agent.any_enabled()
                            && let Some(stack) =
                                dom.request_stack(dom::StackAsker::Element(owner_id))
                        {
                            self.children.creation_stacks.insert(frame, stack);
                        }
                        // Its initial `about:blank` is a document as any other
                        // (Blink's has its loader too): what script inserts
                        // into it loads, its subresources are its own. It
                        // is complete already: it delays nothing.
                        let url = Url::parse("about:blank").expect("about:blank");
                        let mut initial = FrameDocument::new(
                            frame,
                            Some((parent, owner.clone())),
                            url,
                            "text/html",
                            Rc::from(""),
                            None,
                            document,
                            encoding_rs::UTF_8,
                        );
                        initial.readiness = Readiness::Loaded;
                        initial.initial = true;
                        self.children.documents.push(initial);
                        self.children.frames.insert(frame, (parent, owner));
                        self.request_main_frame();
                    }
                    FrameRequest::Navigate { frame, navigation } => {
                        self.navigate_child(frame, navigation);
                    }
                    FrameRequest::Navigation { frame, navigation } => {
                        self.frame_navigated(frame, navigation);
                    }
                    FrameRequest::RegisterBlobUrl { frame, url, data } => {
                        self.blob_urls
                            .register(&url, frame, data.type_(), data.shared_bytes());
                    }
                    FrameRequest::RevokeBlobUrl { url } => self.blob_urls.revoke(&url),
                    FrameRequest::Fetch { frame, request } => self.start_fetch(frame, request),
                    FrameRequest::FetchSent {
                        frame,
                        request,
                        request_id,
                    } => self.fetch_sent(frame, request, request_id),
                    FrameRequest::CancelFetch { id } => self.cancel_fetch(id),
                    FrameRequest::Removed { frame } => {
                        self.blob_urls.revoke_frame(frame);
                        self.cancel_fetches_of_gone_documents();
                        if let Some((_, next)) = engine_frame(&mut self.engine) {
                            next.view.remove_child(frame);
                        }
                        let parent = self.children.frames.get(&frame).map(|(parent, _)| *parent);
                        self.children.remove(frame);
                        self.request_main_frame();
                        // Its parent's `load` may have waited for it.
                        if let Some(parent) = parent {
                            self.finish_load(parent);
                        }
                    }
                }
            }
        }
    }

    /// What script's navigation of `frame` comes to (`FrameNavigation`):
    /// the top-level frame's to another document, its reload, a
    /// traversal and a scroll restoration mode go to the browser (its
    /// session history: Blink's `LocalFrameHost`); a child frame's loads
    /// here, as its iframe's `src` does (its traversal the browser's too);
    /// one within the document scrolls to the fragment, if it is to, and
    /// the browser hears of the top-level frame's (a child frame's isn't
    /// in the session history).
    fn frame_navigated(&mut self, frame: FrameId, navigation: dom::FrameNavigation) {
        use dom::FrameNavigation;
        let top = self
            .document
            .as_ref()
            .is_some_and(|next| next.frame == frame);
        match navigation {
            // A `javascript:` URL's script runs in the frame.
            FrameNavigation::To { url, .. } if url.starts_with("javascript:") => {
                self.run_javascript_url(frame, &url);
            }
            FrameNavigation::To { url, replace } if top => {
                let _ = self.to_browser.send(RendererHostMsg::OpenUrl {
                    url,
                    disposition: protocol::renderer::WindowOpenDisposition::CurrentTab,
                    replace,
                });
            }
            FrameNavigation::To { url, .. } => {
                self.navigate_child(frame, ChildNavigation::Src(url))
            }
            FrameNavigation::Reload if top => {
                let _ = self.to_browser.send(RendererHostMsg::Reload);
            }
            FrameNavigation::Reload => {
                if let Some(url) = self.doc(frame).map(|next| next.url.to_string()) {
                    self.navigate_child_as(
                        frame,
                        ChildNavigation::Src(url),
                        dom::NavigationType::Reload,
                    );
                }
            }
            FrameNavigation::SameDocument {
                url,
                replace,
                state,
                scroll,
                ..
            } => {
                let Ok(parsed) = Url::parse(&url) else {
                    return;
                };
                let Some(next) = self.doc_mut(frame) else {
                    return;
                };
                next.url = parsed;
                if scroll {
                    self.scroll_to_fragment(frame);
                }
                if top {
                    let _ =
                        self.to_browser
                            .send(RendererHostMsg::DidCommitSameDocumentNavigation {
                                url,
                                replace,
                                state,
                                navigation_id: None,
                            });
                }
                self.request_main_frame();
            }
            // Whichever frame's link: the browser opens the tab.
            FrameNavigation::NewTab { url, background } => {
                let disposition = if background {
                    protocol::renderer::WindowOpenDisposition::NewBackgroundTab
                } else {
                    protocol::renderer::WindowOpenDisposition::NewForegroundTab
                };
                let _ = self.to_browser.send(RendererHostMsg::OpenUrl {
                    url,
                    disposition,
                    replace: false,
                });
            }
            FrameNavigation::GoToOffset(delta) => {
                let _ = self
                    .to_browser
                    .send(RendererHostMsg::GoToEntryAtOffset(delta));
            }
            FrameNavigation::ScrollRestoration { manual } if top => {
                let _ = self.to_browser.send(RendererHostMsg::UpdateState {
                    manual_scroll_restoration: manual,
                });
            }
            FrameNavigation::ScrollRestoration { .. } => {}
        }
    }

    /// A `javascript:` URL navigated to in `frame`: its source (the rest
    /// of the URL, percent-decoded) run as a classic script of the frame's
    /// document.
    fn run_javascript_url(&mut self, frame: FrameId, url: &str) {
        let source = document::percent_decode(&url["javascript:".len()..]);
        let Some((document_url, document)) = self
            .doc(frame)
            .map(|next| (next.url.to_string(), next.document.clone()))
        else {
            return;
        };
        let Some(engine) = &mut self.engine else {
            return;
        };
        // HTML's: its base URL is its document's.
        let base_url = engine
            .dom_mut()
            .read(|cx| cx.document_base_url(document.get(cx.gc)).map(String::from));
        engine.run_classic_script(
            frame,
            &dom_api::ClassicScript {
                source: &source,
                url: &document_url,
                base_url: base_url.as_deref(),
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
        self.handle_frame_requests();
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// `RendererMsg::CommitSameDocumentNavigation` for the DOM: the
    /// browser's traversal to another entry of the top-level frame's
    /// document (`ScriptEngine::traverse_within_document`: its URL, state
    /// and mode; `popstate` now, `hashchange` in a task), which scrolls
    /// to the fragment unless the mode is manual. If the frame shows
    /// another document by now, the browser loads the entry.
    pub(super) fn commit_same_document_traversal(
        &mut self,
        navigation_id: u64,
        document: u64,
        url: &str,
        history: protocol::renderer::HistoryEntryParams,
    ) {
        let frame = self
            .document
            .as_ref()
            .filter(|next| next.navigation_id == document)
            .map(|next| next.frame);
        let (Some(frame), Ok(new_url), Some(engine)) = (frame, Url::parse(url), &mut self.engine)
        else {
            let _ = self
                .to_browser
                .send(RendererHostMsg::SameDocumentNavigationFailed { navigation_id });
            return;
        };
        engine.set_session_history(dom::SessionHistory {
            index: history.index,
            length: history.length,
        });
        if !engine.traverse_within_document(
            frame,
            url,
            history.state,
            history.manual_scroll_restoration,
        ) {
            let _ = self
                .to_browser
                .send(RendererHostMsg::SameDocumentNavigationFailed { navigation_id });
            return;
        }
        let fragment = new_url.fragment().is_some();
        if let Some(next) = self.doc_mut(frame) {
            next.url = new_url.clone();
        }
        if !history.manual_scroll_restoration && fragment {
            self.scroll_to_fragment(frame);
        }
        self.request_main_frame();
        let _ = self
            .to_browser
            .send(RendererHostMsg::DidCommitSameDocumentNavigation {
                url: new_url.into(),
                replace: false,
                state: None,
                navigation_id: Some(navigation_id),
            });
        self.handle_frame_requests();
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// A child frame's navigation (HTML's "navigate" for its iframe's
    /// `src` or `srcdoc`): `src` fetched as an iframe's (the renderer loads
    /// it), `srcdoc`'s markup and `about:blank` committed at once.
    fn navigate_child(&mut self, frame: FrameId, navigation: ChildNavigation) {
        self.navigate_child_as(frame, navigation, dom::NavigationType::Navigate);
    }

    /// Whether `frame`'s iframe's referrer policy is `no-referrer` (its
    /// `referrerpolicy`, ASCII case-insensitively).
    fn child_no_referrer(&mut self, frame: FrameId) -> bool {
        let Some((_, owner)) = self.children.frames.get(&frame) else {
            return false;
        };
        let Some(engine) = self.engine.as_mut() else {
            return false;
        };
        engine.dom_mut().read(|cx| {
            cx.attribute_value_by_id(owner.get(cx.gc), dom::names::local::REFERRERPOLICY)
                .is_some_and(|policy| policy.borrow().to_str().eq_ignore_ascii_case("no-referrer"))
        })
    }

    /// `navigate_child`, its document's navigation of `navigation_type`.
    fn navigate_child_as(
        &mut self,
        frame: FrameId,
        navigation: ChildNavigation,
        navigation_type: dom::NavigationType,
    ) {
        match navigation {
            ChildNavigation::Src(src) => {
                let Ok(url) = Url::parse(&src) else {
                    return;
                };
                // To a fragment of the document it shows (HTML's navigate to
                // a fragment): its URL is that, scrolled there; nothing
                // loads. Not a reload, which loads its URL, fragment and all.
                if navigation_type != dom::NavigationType::Reload
                    && url.fragment().is_some()
                    && self.navigate_to_fragment(frame, &url)
                {
                    return;
                }
                if url.scheme() == "about" {
                    self.queue_child_commit(frame, url, String::new());
                    return;
                }
                let started = std::time::Instant::now();
                let timing = ChildNavigationTiming {
                    navigation_type,
                    no_referrer: self.child_no_referrer(frame),
                };
                // Its iframe asked, in its parent's document.
                let request = ResourceRequest::new(url.as_str(), RequestDestination::Iframe);
                let request_id = self.send_request_with_initiator(frame, request, |this| {
                    let (parent, owner) = this.children.frames.get(&frame)?.clone();
                    let engine = this.engine.as_mut()?;
                    let owner = engine.dom_mut().read(|cx| cx.node_id(owner.get(cx.gc)));
                    this.initiator(parent, &dom::LoadInitiator::Element(owner))
                });
                // One navigation a frame: an earlier one's response is
                // dropped.
                self.children.navigating(frame);
                self.children
                    .fetches
                    .insert(request_id, (frame, url, started, timing));
            }
            ChildNavigation::Srcdoc(markup) => {
                let url = Url::parse("about:srcdoc").expect("about:srcdoc");
                self.queue_child_commit(frame, url, markup);
            }
            ChildNavigation::AboutBlank => {
                let url = Url::parse("about:blank").expect("about:blank");
                self.queue_child_commit(frame, url, String::new());
            }
        }
    }

    /// `frame`'s navigation to `url`, an `about:` document of `markup`
    /// (`srcdoc`'s), which needs no fetch: it commits in a task of its own
    /// (`run_child_commits`), as one fetched would once its response
    /// came, so the parser and the script that navigated it go on first
    /// (an iframe's `onload` set after it is parsed hears its `load`).
    /// Its iframe delays its parent's `load` meanwhile.
    fn queue_child_commit(&mut self, frame: FrameId, url: Url, markup: String) {
        self.children.navigating(frame);
        self.children.commits.push_back((frame, url, markup));
    }

    /// Whether a child frame's `about:` document waits to commit: the loop
    /// doesn't wait for a task to come first.
    pub(super) fn child_commits_due(&self) -> bool {
        !self.children.commits.is_empty()
    }

    /// The child frames' `about:` documents queued to commit, each in a task
    /// of its own, in order (`queue_child_commit`).
    pub(super) fn run_child_commits(&mut self) {
        while let Some((frame, url, markup)) = self.children.commits.pop_front() {
            let _task = trace_event::trace_event!("ThreadControllerImpl::RunTask");
            self.commit_child(frame, url, "text/html", None, markup.as_bytes(), None, None);
            self.send_harness_results();
        }
    }

    /// A child frame's new document, as `commit_navigation` commits
    /// the top-level one's: its realm and document made (its iframe's
    /// content document), its view in its parent's, the body pushed to its
    /// parser, then parsed and its scripts run; `loader_id`, what DevTools
    /// calls its loader (its request), if it was fetched. Nothing for a
    /// frame that went.
    #[expect(clippy::too_many_arguments, reason = "a document has that many parts")]
    fn commit_child(
        &mut self,
        frame: FrameId,
        url: Url,
        mime_type: &str,
        charset: Option<&str>,
        body: &[u8],
        load: Option<(
            &LoadResult,
            std::time::Instant,
            &[protocol::renderer::RedirectHop],
            ChildNavigationTiming,
        )>,
        loader_id: Option<String>,
    ) {
        let _span = trace_event::info_event!("commit_child", %url, %mime_type);
        let Some((parent, owner)) = self.children.frames.get(&frame).cloned() else {
            return;
        };
        let document_id = self.new_document_id();
        // Its time origin: when its navigation started (its `src`'s fetch;
        // an `about:` document's, now).
        let now = load.map_or_else(std::time::Instant::now, |(_, started, ..)| started);
        let time_origin = self.clock.at(now);
        // Its navigation timing: its load's, or none's.
        let timing = match load {
            Some((result, _, redirects, navigation)) => {
                self.child_timing(result, redirects, navigation, time_origin)
            }
            None => {
                let clock = self.clock;
                crate::navigation_timing::network_timing(
                    None,
                    dom::NavigationType::Navigate,
                    time_origin,
                    |time: ipc::TimeTicks| {
                        clock.before_now(ipc::TimeTicks::now().saturating_since(time))
                    },
                    crate::navigation_timing::randomized_confidence(),
                )
            }
        };
        let previous = self.doc(frame).map(|next| next.url.clone());
        // The realm before it goes, and the blob URLs it made.
        self.blob_urls.revoke_frame(frame);
        let clock = self.clock;
        let Some(engine) = &mut self.engine else {
            return;
        };
        let (text, document_encoding) = encoding::decode_document(mime_type, charset, body);
        let markup = document::markup_for(mime_type, &text);
        // The document it shows unloaded first, its `unload` timed.
        let unloaded = engine
            .unload_document(frame)
            .map(|(start, end)| (clock.at(start), clock.at(end)));
        let timing =
            crate::navigation_timing::with_unload(timing, unloaded, previous.as_ref(), &url);
        let Some(document) = engine.create_window(frame, url.as_str(), now) else {
            return;
        };
        engine.set_navigation_timing(
            frame,
            crate::navigation_timing::document_timing(&timing, time_origin, self.clock.now()),
        );
        // Its own response's, as a top-level document's is.
        let content_language = load
            .and_then(|(result, ..)| result.as_ref().ok())
            .and_then(|response| {
                response
                    .headers
                    .iter()
                    .find(|(name, _)| name == "content-language")
            })
            .map(|(_, value)| value.as_str());
        set_document_metadata(
            engine.dom_mut(),
            &document,
            mime_type,
            document_encoding,
            content_language,
        );
        let mut parser = if url.as_str() == "about:srcdoc" {
            Parser::new_srcdoc(engine.dom_mut(), &document)
        } else {
            Parser::new(engine.dom_mut(), &document)
        };
        parser.push(&markup);
        let Some((dom, next)) = engine_frame(&mut self.engine) else {
            return;
        };
        let (node, owner_id) = dom.read(|cx| {
            (
                cx.node_id(document.get(cx.gc)),
                cx.node_id(owner.get(cx.gc)),
            )
        });
        let view = FrameView::new(document.clone(), node, document_id, self.system_colors)
            .into_child(frame, owner_id);
        let top = self.document.as_ref().map(|next| next.frame);
        next.view
            .show_child((Some(parent) != top).then_some(parent), view);
        // Its children before go with the document before.
        let below: Vec<FrameId> = self
            .children
            .frames
            .iter()
            .filter(|(_, (of, _))| *of == frame)
            .map(|(id, _)| *id)
            .collect();
        for child in below {
            self.children.remove(child);
        }
        self.children.documents.retain(|next| next.frame != frame);
        // The document before's `fetch()`es go with it.
        self.cancel_fetches_of_gone_documents();
        self.children.documents.push(FrameDocument {
            loader_id,
            ..FrameDocument::new(
                frame,
                Some((parent, owner)),
                url,
                mime_type,
                Rc::from(&*text),
                Some(parser),
                document,
                document_encoding,
            )
        });
        self.request_main_frame();
        self.advance_document(frame);
    }

    /// Parses the DOM's document until a script must be fetched or the
    /// input is all parsed, running each parser-inserted classic script as
    /// the parser stops at it (HTML's "parser pause"): an inline one at
    /// once, an external one once fetched (`resource`), neither while
    /// a stylesheet that blocks scripts loads; a deferred one fetched to
    /// run after parsing (`run_deferred_scripts`), one that waits for
    /// no parser queued in the DOM.
    pub(super) fn advance_document(&mut self, frame: FrameId) {
        loop {
            self.start_loads();
            self.handle_frame_requests();
            if self.engine.is_none() {
                return;
            }
            if self
                .doc(frame)
                .is_some_and(|next| next.blocking_script.is_some())
            {
                if !self.script_ready(frame, |next| next.blocking_script.as_ref()) {
                    return;
                }
                let Some(script) = self
                    .doc_mut(frame)
                    .and_then(|next| next.blocking_script.take())
                else {
                    return;
                };
                self.execute_parser_script(frame, script);
                continue;
            }
            let Some(next) = self.doc_mut(frame) else {
                return;
            };
            let Some(mut parser) = next.parser.take() else {
                return;
            };
            // The engine lent to the parser for its run (it constructs the
            // custom elements it defines at once), and given back.
            let lent = self.engine.take().expect("the engine, checked above");
            let (state, engine) = parser.resume(lent);
            self.engine = Some(engine);
            // Where the elements it made start, before their loads do, for
            // DevTools' `Network` (kept only while a session has it on).
            let lines = parser.take_load_lines();
            self.keep_parser_lines(frame, lines);
            match state {
                ParserState::NeedsInput => {
                    if let Some(engine) = self.engine.take() {
                        let (engine, lines) = parser.finish_taking_load_lines(engine);
                        self.engine = Some(engine);
                        self.keep_parser_lines(frame, lines);
                    }
                    self.handle_frame_requests();
                    // The `<link>`s the last of the input brought start
                    // loading first: the deferred scripts wait for them.
                    self.start_loads();
                    self.document_parsed(frame);
                    return;
                }
                ParserState::Script { element, line } => {
                    // The title parsed so far goes before the script runs
                    // (Blink tells it as its `<title>` ends; a script may
                    // set another at once).
                    self.update_title();
                    let Some(next) = self.doc_mut(frame) else {
                        return;
                    };
                    next.parser = Some(parser);
                    let script = element.root().clone();
                    // HTML's "an end tag whose tag name is script": a
                    // microtask checkpoint first, so the mutation observers
                    // hear of what the parser inserted before the script is
                    // prepared (Blink's
                    // `HTMLParserScriptRunner::ProcessScriptElement`).
                    if let Some(engine) = &mut self.engine {
                        engine.perform_microtask_checkpoint();
                    }
                    let line = u32::try_from(line).unwrap_or(u32::MAX);
                    match self.prepare_script(frame, &element) {
                        Prepared::Nothing | Prepared::Queued => {}
                        Prepared::Inline { source, base_url } => {
                            let ready = Some(Some(ParserSource::Inline {
                                source,
                                line,
                                base_url,
                            }));
                            if let Some(next) = self.doc_mut(frame) {
                                next.blocking_script = Some(ParserScript {
                                    element: script,
                                    module: false,
                                    request_id: None,
                                    charset: None,
                                    ready,
                                });
                            }
                        }
                        Prepared::Blocking(request) => {
                            self.fetch_parser_script(frame, script, &request, false)
                        }
                        Prepared::Defer(request) => {
                            self.fetch_parser_script(frame, script, &request, true)
                        }
                        Prepared::Module(start) => {
                            self.start_parser_module(frame, script, start, line)
                        }
                    }
                }
            }
        }
    }

    /// The parser's external script `element` fetched (`request`): the
    /// parser waits for it, or (`defer`) it runs after parsing, in order.
    fn fetch_parser_script(
        &mut self,
        frame: FrameId,
        element: NodeRoot,
        request: &dom::ScriptRequest,
        defer: bool,
    ) {
        // The parser's element, for DevTools (no one looks otherwise).
        let asker = self
            .engine
            .as_mut()
            .filter(|_| self.network_agent.enabled())
            .map_or(dom::LoadInitiator::Other, |engine| {
                let id = engine.dom_mut().read(|cx| cx.node_id(element.get(cx.gc)));
                dom::LoadInitiator::Element(id)
            });
        let request_id = self.fetch_script(frame, request, asker);
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        let script = ParserScript {
            element,
            module: false,
            request_id: Some(request_id),
            charset: request.charset.clone(),
            ready: None,
        };
        if defer {
            next.deferred_scripts.push_back(script);
        } else {
            next.blocking_script = Some(script);
        }
    }

    /// The parser's module script `element` (`start`, its inline text from
    /// `line`): its graph fetched by the engine; a deferred one runs after
    /// parsing in its turn, an `async` one as soon as it can (the DOM's).
    fn start_parser_module(
        &mut self,
        frame: FrameId,
        element: NodeRoot,
        mut start: dom_api::ModuleStart,
        line: u32,
    ) {
        if let dom_api::ModuleSource::Inline { line: at, .. } = &mut start.source {
            *at = line;
        }
        let deferred = start.deferred;
        let Some(document) = self.doc(frame).map(|next| next.document.clone()) else {
            return;
        };
        if deferred && let Some(next) = self.doc_mut(frame) {
            next.deferred_scripts.push_back(ParserScript {
                element,
                module: true,
                request_id: None,
                charset: None,
                ready: None,
            });
        }
        if let Some(engine) = &mut self.engine {
            engine.start_module_graph(&document, start);
        }
    }

    /// Whether the parser's script `which` picks is ready to run: fetched
    /// (a module script's graph, the DOM says), and no stylesheet that
    /// blocks scripts is loading.
    fn script_ready(
        &mut self,
        frame: FrameId,
        which: impl FnOnce(&FrameDocument) -> Option<&ParserScript>,
    ) -> bool {
        let Some(next) = self.doc(frame) else {
            return false;
        };
        // A sheet stops blocking them once script removed its `<link>` or
        // made it no stylesheet, though it still loads (Blink's
        // `StyleEngine::RemovePendingBlockingSheet`, as the link goes).
        let blocking: Vec<NodeRoot> = next
            .pending_sheets
            .values()
            .filter(|sheet| sheet.blocks_scripts)
            .map(|sheet| sheet.link.clone())
            .collect();
        if !blocking.is_empty() {
            let document = next.document.clone();
            let blocked = self.engine.as_mut().is_some_and(|engine| {
                engine.dom_mut().read(|cx| {
                    let document = document.get(cx.gc);
                    blocking.iter().any(|link| {
                        let link = link.get(cx.gc);
                        cx.is_connected(link)
                            && cx.containing_shadow_root(link).is_none()
                            && cx.node_id(cx.document_of(link)) == cx.node_id(document)
                            && cx.stylesheet_link_href(link).is_some()
                    })
                })
            });
            if blocked {
                return false;
            }
        }
        let Some(next) = self.doc(frame) else {
            return false;
        };
        let Some(script) = which(next) else {
            return false;
        };
        if !script.module {
            return script.ready.is_some();
        }
        let element = script.element.clone();
        self.engine.as_mut().is_some_and(|engine| {
            engine
                .dom_mut()
                .read(|cx| cx.pending_script_fetched(element.get(cx.gc)))
        })
    }

    /// The parser's scripts that wait for a sheet that blocks scripts, tried
    /// again after the tasks: script may have removed its `<link>`, or made
    /// it no stylesheet, so they run without waiting for its response.
    pub(super) fn retry_scripts_blocked_by_sheets(&mut self) {
        let waiting: Vec<(FrameId, bool)> = self
            .document
            .iter()
            .chain(&self.children.documents)
            .filter(|next| {
                (next.blocking_script.is_some() || next.readiness == Readiness::Deferring)
                    && next
                        .pending_sheets
                        .values()
                        .any(|sheet| sheet.blocks_scripts)
            })
            .map(|next| (next.frame, next.blocking_script.is_some()))
            .collect();
        for (frame, parser_blocked) in waiting {
            if parser_blocked {
                self.advance_document(frame);
            } else {
                self.run_deferred_scripts(frame);
            }
        }
    }

    /// HTML's "execute the script element" for the parser's script: none if
    /// script moved it to another document meanwhile; its `error` if it
    /// failed to load; else it runs (an external one from its response's
    /// URL, then its `load`).
    fn execute_parser_script(&mut self, frame: FrameId, script: ParserScript) {
        // A module script's, the engine's (which looks at where it is).
        if script.module {
            if let Some(engine) = &mut self.engine {
                engine.run_ready_script(&script.element);
            }
            self.send_harness_results();
            return;
        }
        let Some(next) = self.doc(frame) else {
            return;
        };
        let (document, url) = (next.document.clone(), next.url.clone());
        let Some(engine) = &mut self.engine else {
            return;
        };
        let moved = engine.dom_mut().read(|cx| {
            cx.node_id(cx.document_of(script.element.get(cx.gc))) != cx.node_id(document.get(cx.gc))
        });
        if moved {
            return;
        }
        match script.ready.flatten() {
            None => {
                engine.fire_event(&script.element, dom::names::event::ERROR);
                self.send_harness_results();
            }
            Some(ParserSource::Inline {
                source,
                line,
                base_url,
            }) => {
                self.run_script(
                    frame,
                    &script.element,
                    &ClassicScript {
                        source: &source,
                        url: url.as_str(),
                        base_url: base_url.as_deref(),
                        line,
                        muted: false,
                        use_credentials: false,
                    },
                );
            }
            Some(ParserSource::External(loaded)) => {
                self.run_script(
                    frame,
                    &script.element,
                    &ClassicScript {
                        source: &loaded.source,
                        url: &loaded.url,
                        base_url: None,
                        line: 1,
                        muted: loaded.muted,
                        use_credentials: false,
                    },
                );
                if let Some(engine) = &mut self.engine {
                    engine.fire_event(&script.element, dom::names::event::LOAD);
                }
                self.send_harness_results();
            }
        }
    }

    /// HTML's "prepare the script element" for a parser-inserted script
    /// (`dom_api::prepare_script`), in the document the parser is
    /// for: one script moved to another document meanwhile doesn't run.
    fn prepare_script(&mut self, frame: FrameId, element: &SinkHandle) -> Prepared {
        let Some(document) = self.doc(frame).map(|next| next.document.clone()) else {
            return Prepared::Nothing;
        };
        let Some(engine) = &mut self.engine else {
            return Prepared::Nothing;
        };
        engine.dom_mut().mutate(|cx| {
            let script = element.root().get(cx.gc);
            if cx.node_id(cx.document_of(script)) != cx.node_id(document.get(cx.gc)) {
                return Prepared::Nothing;
            }
            dom_api::prepare_script(cx, script)
        })
    }

    /// Runs `classic`, the classic script of element `script` of the new
    /// DOM's document (its credentials mode the element's), a microtask checkpoint before it (the tree builder's at a
    /// script's end tag: what the parser changed reaches the mutation
    /// observers first) and after it (HTML's "clean up after running
    /// script"), the document's `currentScript` it meanwhile (HTML's
    /// "execute the script element": none for one in a shadow tree), and
    /// reports testharness's results if it gave any.
    fn run_script(&mut self, frame: FrameId, script: &NodeRoot, classic: &ClassicScript<'_>) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.perform_microtask_checkpoint();
        // The document it was set on gets it back, if the script moved
        // the element meanwhile.
        let (document, before, use_credentials) = engine.dom_mut().mutate(|cx| {
            let script = script.get(cx.gc);
            let document = cx.document_of(script);
            let current = cx
                .containing_shadow_root(script)
                .is_none()
                .then_some(script);
            let before = cx
                .set_current_script(document, current)
                .map(|before| cx.root(before));
            (
                cx.root(document),
                before,
                dom_api::uses_credentials(cx, script),
            )
        });
        // Its fetch options' credentials mode, its element's as it runs.
        engine.run_classic_script(
            frame,
            &ClassicScript {
                use_credentials,
                ..*classic
            },
        );
        engine.dom_mut().mutate(|cx| {
            cx.set_current_script(
                document.get(cx.gc),
                before.as_ref().map(|before| before.get(cx.gc)),
            );
        });
        engine.perform_microtask_checkpoint();
        self.handle_frame_requests();
        self.send_harness_results();
    }

    /// HTML's "the end", once the document is parsed and its
    /// parser-blocking scripts ran: interactive, then its deferred scripts
    /// (`run_deferred_scripts`).
    fn document_parsed(&mut self, frame: FrameId) {
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        next.readiness = Readiness::Deferring;
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.document_ready(frame, DocumentReadiness::Interactive);
        self.handle_frame_requests();
        self.send_harness_results();
        self.run_deferred_scripts(frame);
    }

    /// The document's deferred scripts run in order, each once fetched and
    /// no stylesheet blocks scripts (their arrival runs this again); once
    /// none is left, `DOMContentLoaded`, then complete and the window's
    /// `load` once nothing delays it (`finish_load`).
    fn run_deferred_scripts(&mut self, frame: FrameId) {
        loop {
            let Some(next) = self.doc(frame) else {
                return;
            };
            if next.readiness != Readiness::Deferring {
                return;
            }
            if next.deferred_scripts.is_empty() {
                break;
            }
            if !self.script_ready(frame, |next| next.deferred_scripts.front()) {
                return;
            }
            let Some(script) = self
                .doc_mut(frame)
                .and_then(|next| next.deferred_scripts.pop_front())
            else {
                return;
            };
            self.execute_parser_script(frame, script);
        }
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        next.readiness = Readiness::Parsed;
        let top = next.child_of.is_none();
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.document_ready(frame, DocumentReadiness::ContentLoaded);
        self.handle_frame_requests();
        self.scroll_to_fragment(frame);
        if top {
            self.send_harness_results();
            self.page_agent.dom_content_loaded();
            self.dom_agent.dom_content_loaded();
        }
        // Its network watched for quiet from now on.
        if let Some(next) = self.doc_mut(frame) {
            next.idleness.start();
        }
        self.tell_lifecycle(frame, "DOMContentLoaded", ipc::TimeTicks::now());
        self.finish_load(frame);
    }

    /// A classic script of `frame`'s document fetched (`request`, HTML's
    /// "fetch a classic script"): from the document's origin (the network
    /// service taints another origin's `no-cors` response opaque: its
    /// errors are muted), by CORS with `crossorigin`'s credentials mode;
    /// `asker` asked for it. The request's number.
    fn fetch_script(
        &mut self,
        frame: FrameId,
        request: &dom::ScriptRequest,
        asker: dom::LoadInitiator,
    ) -> u64 {
        let initiator = self.document_origin(frame);
        let mut fetching = ResourceRequest::new(&request.url, RequestDestination::Script);
        fetching.initiator = initiator;
        if let Some(cors) = request.cors {
            fetching.mode = RequestMode::Cors;
            fetching.credentials = match cors {
                dom::ScriptCors::Anonymous => CredentialsMode::SameOrigin,
                dom::ScriptCors::UseCredentials => CredentialsMode::Include,
            };
        }
        self.send_request_with_initiator(frame, fetching, |this| this.initiator(frame, &asker))
    }

    /// The scripts the DOM queued that wait for no parser
    /// (`Dom::take_script_fetches`), fetched: each handed back
    /// (`Cx::script_fetched`) as its response comes (`resource`).
    pub(super) fn start_script_fetches(&mut self) {
        // As `start_image_fetches`: a blob URL registered first.
        self.handle_frame_requests();
        let Some(engine) = &mut self.engine else {
            return;
        };
        let fetches = engine.dom_mut().take_script_fetches();
        for script in fetches {
            // One whose document no frame shows is never fetched (its
            // document's load waits for nothing).
            let Some(frame) = self.frame_of_document(script.document) else {
                continue;
            };
            // A module's request has its credentials mode (by CORS), the
            // module map's.
            let request = script.request;
            let request_id = self.fetch_script(frame, &request, script.initiator);
            self.script_fetches.insert(
                request_id,
                PendingScriptFetch {
                    key: script.key,
                    frame,
                    charset: request.charset,
                    module: script.module,
                },
            );
        }
    }

    /// `url`, a subresource of `frame`'s document, failed (or was of no
    /// use): the browser hears of it with the document's load.
    fn note_failed(&mut self, frame: FrameId, url: String) {
        if let Some(next) = self.doc_mut(frame) {
            next.failed.push(url);
        }
    }

    /// `frame`'s document's origin, as its requests' initiator gives it
    /// (Blink's `SecurityOrigin::ToRawString`: an opaque one's is `null`):
    /// an `about:srcdoc` or `about:blank` document's is the one it
    /// inherited, not its URL's.
    fn document_origin(&mut self, frame: FrameId) -> Option<String> {
        let document = self.doc(frame)?.document.clone();
        let engine = self.engine.as_mut()?;
        let origin = engine
            .dom_mut()
            .read(|cx| dom_api::origin(cx, document.get(cx.gc)));
        Some(match origin {
            Some(origin @ url::Origin::Tuple(..)) => origin.ascii_serialization(),
            _ => "null".to_owned(),
        })
    }

    /// The frame whose document `document` is, if one is.
    fn frame_of_document(&mut self, document: dom::NodeId) -> Option<FrameId> {
        let engine = self.engine.as_mut()?;
        self.document
            .iter()
            .chain(&self.children.documents)
            .find(|next| {
                engine
                    .dom_mut()
                    .read(|cx| cx.node_id(next.document.get(cx.gc)) == document)
            })
            .map(|next| next.frame)
    }

    /// The window's `load`, if the document is parsed and no sheet is
    /// loading.
    /// The document of `frame` scrolled to its URL's fragment once laid
    /// out (HTML's "scroll to the fragment": its indicated part, the
    /// element whose id is the fragment, decoded, or the first `<a>` whose
    /// `name` is), as it is parsed.
    /// `frame`'s navigation to `url`, if `url` is its document's but for
    /// the fragment: the document's URL `url`, scrolled to the fragment.
    /// Whether it was.
    fn navigate_to_fragment(&mut self, frame: FrameId, url: &Url) -> bool {
        let Some(next) = self.doc_mut(frame) else {
            return false;
        };
        let (mut current, mut wanted) = (next.url.clone(), url.clone());
        current.set_fragment(None);
        wanted.set_fragment(None);
        if current != wanted {
            return false;
        }
        next.url = url.clone();
        let document = next.document.clone();
        if let Some(engine) = &mut self.engine {
            engine
                .dom_mut()
                .mutate(|cx| cx.set_document_url(document.get(cx.gc), url.as_str()));
        }
        self.scroll_to_fragment(frame);
        self.request_main_frame();
        true
    }

    fn scroll_to_fragment(&mut self, frame: FrameId) {
        let Some(next) = self.doc(frame) else {
            return;
        };
        let url = next.url.clone();
        let document = next.document.clone();
        let Some((dom, view)) = engine_frame(&mut self.engine) else {
            return;
        };
        // HTML's scroll to the fragment: its indicated element is the
        // document's target element (`:target`), then in view; with none,
        // an empty fragment or `top` (ASCII case-insensitively) is the top
        // of the document.
        let found = dom.mutate(|cx| {
            let document = document.get(cx.gc);
            dom_api::update_target_element(cx, document, &url);
            let top = url.fragment().is_some_and(|fragment| {
                fragment.is_empty() || fragment.eq_ignore_ascii_case("top")
            });
            let element = match cx.target_element(document) {
                Some(element) => cx.node_id(element),
                None if top => cx.node_id(document),
                None => return None,
            };
            Some((cx.node_id(document), element))
        });
        if let Some((document, element)) = found
            && let Some(view) = view.view.view_of_mut(document)
        {
            view.scroll_into_view_after_layout(element);
        }
        self.request_main_frame();
    }

    /// `finish_load` of every frame, the children's before their
    /// parents' (whose `load` waits for theirs).
    fn finish_loads(&mut self) {
        let mut frames = self.frames();
        frames.reverse();
        for frame in frames {
            self.finish_load(frame);
        }
    }

    fn finish_load(&mut self, frame: FrameId) {
        // The `<link>`s the last of the input brought looked for before the
        // style update below takes the flag that says the sheets changed.
        self.start_loads();
        let Some(next) = self.doc(frame) else {
            return;
        };
        if next.readiness != Readiness::Parsed
            || !next.pending_sheets.is_empty()
            || self.children.loading_in(frame)
        {
            return;
        }
        // Its backgrounds' images looked for first: its style brought up to
        // date (Blink finds them in the style recalc before `load`), those
        // it fetches delaying it.
        let document = next.document.clone();
        if let Some((dom, view)) = engine_frame(&mut self.engine) {
            dom.mutate(|cx| view.update_style_and_layout(cx));
        }
        self.start_image_fetches();
        let Some(engine) = &mut self.engine else {
            return;
        };
        // Its images that delay it (not a lazy one's, as in Chrome), and
        // the backgrounds' (Chrome's too).
        if engine
            .dom_mut()
            .read(|cx| cx.background_images_loading() || cx.load_event_delayed(document.get(cx.gc)))
        {
            return;
        }
        // HTML's "the end": complete and the window's `load` are a task of
        // the DOM manipulation task source, after the element tasks queued
        // before it (a script's `error` for an empty `src`, an image's
        // `load`); its iframe still delays its parent's `load` meanwhile
        // (`ChildFrames::loading_in`). Its turn ends it
        // (`document_loaded`).
        engine
            .dom_mut()
            .mutate(|cx| cx.queue_window_load(document.get(cx.gc)));
        if let Some(next) = self.doc_mut(frame) {
            next.readiness = Readiness::LoadQueued;
        }
    }

    /// The window `load` tasks that ran since the last look
    /// (`Dom::take_loaded_documents`): each document's load ends.
    fn finish_loaded_documents(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        let loaded = engine.dom_mut().take_loaded_documents();
        for document in loaded {
            if let Some(frame) = self.frame_of_document(document) {
                self.document_loaded(frame);
            }
        }
    }

    /// `frame`'s document's window `load` task ran: a child frame's iframe
    /// fires `load` (HTML's "iframe load event steps", in its parent's
    /// realm) and its parent's `load` may come; the top-level frame's
    /// `DidFinishLoad` goes with the next frame. DevTools hears of either.
    fn document_loaded(&mut self, frame: FrameId) {
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        if next.readiness != Readiness::LoadQueued {
            return;
        }
        next.readiness = Readiness::Loaded;
        let child_of = next.child_of.clone();
        let Some(engine) = &mut self.engine else {
            return;
        };
        match child_of {
            // A child frame's: its iframe's `load` (HTML's "iframe load
            // event steps", in its parent's realm), and its parent's
            // `load` if it waited for it.
            Some((parent, owner)) => {
                engine.fire_event(&owner, dom::names::event::LOAD);
                self.handle_frame_requests();
                self.send_harness_results();
                if let Some(id) = self.tell_lifecycle(frame, "load", ipc::TimeTicks::now()) {
                    self.page_agent.stopped_loading(&id);
                }
                self.request_main_frame();
                self.finish_load(parent);
            }
            None => {
                self.handle_frame_requests();
                self.send_harness_results();
                self.page_agent.load_fired();
                if let Some(id) = self.tell_lifecycle(frame, "load", ipc::TimeTicks::now()) {
                    self.page_agent.stopped_loading(&id);
                }
                if let Some(next) = &mut self.document {
                    next.load_finished = true;
                }
                self.request_main_frame();
            }
        }
    }

    /// The tab's title, the DOM's document's (Blink's
    /// `Document::UpdateTitle` and `DispatchDidReceiveTitle`), after each
    /// task: read again once the tree changed, sent if it differs from the
    /// last one sent (the first, empty or not, at once: the browser shows
    /// the URL for an empty one, as before any).
    pub(super) fn update_title(&mut self) {
        let (Some(engine), Some(next)) = (&mut self.engine, &mut self.document) else {
            return;
        };
        let version = engine.dom_mut().read(|cx| cx.tree_version());
        if next.sent_title.is_some() && version == next.title_version {
            return;
        }
        next.title_version = version;
        let document = &next.document;
        let title = engine
            .dom_mut()
            .read(|cx| dom_api::document_title(&dom_api::ApiCx::new(cx), document.get(cx.gc)));
        if next.sent_title.as_deref() != Some(title.as_str()) {
            let _ = self
                .to_browser
                .send(RendererHostMsg::UpdateTitle(title.clone()));
            next.sent_title = Some(title);
        }
    }

    /// The main frame and the DOM's document in it as DevTools tells
    /// them, once DevTools gave the frame its token.
    pub(super) fn frame_info(&self) -> Option<FrameInfo> {
        let next = self.document.as_ref()?;
        main_frame_info(
            &self.devtools_main_frame_token,
            next.navigation_id,
            &next.url,
            &next.mime_type,
            &next.source,
            &next.resources,
        )
    }

    /// `frame`'s document reached `name` at `at` (`Page.lifecycleEvent`),
    /// after DevTools has been told of the frames as they are now (its
    /// `init` and `frameNavigated` come first): the frame's id, if DevTools
    /// knows it.
    fn tell_lifecycle(
        &mut self,
        frame: FrameId,
        name: &'static str,
        at: ipc::TimeTicks,
    ) -> Option<String> {
        // A title the task set goes first, on the same channel: what hears
        // the lifecycle and asks for the title then has it (Blink sends
        // the title as it changes, before `load`).
        self.update_title();
        // Kept for the sessions to come too (`tell_frames` tells the
        // frames to those there are).
        let frames = self.document_frame_infos();
        if !self.devtools_sessions.is_empty() {
            self.page_agent.update(frames.clone());
        }
        let top = self.document.as_ref().map(|next| next.frame);
        let id = if Some(frame) == top {
            frames.first()?.id.clone()
        } else {
            self.frame_id(frame)
        };
        let info = frames.iter().find(|info| info.id == id)?;
        self.page_agent
            .lifecycle_event(&info.id, info.document, &info.loader_id, name, at);
        Some(id)
    }

    /// The DOM's frames as DevTools tells them, parents first: the
    /// top-level one (`frame_info`), then each child frame (Chrome's
    /// frame tree): its id WebDriver's, its document's number its key and
    /// loader id, its iframe's `name`; its initial `about:blank` until a
    /// document of its own commits.
    pub(super) fn document_frame_infos(&mut self) -> Vec<FrameInfo> {
        let Some(main) = self.frame_info() else {
            return Vec::new();
        };
        let Some(top) = self.document.as_ref().map(|next| next.frame) else {
            return Vec::new();
        };
        let mut children: Vec<(FrameId, FrameId, NodeRoot)> = self
            .children
            .frames
            .iter()
            .map(|(frame, (parent, owner))| (*frame, *parent, owner.clone()))
            .collect();
        // In the order they were made, each after its parent below.
        children.sort_by_key(|(frame, _, _)| *frame);
        let mut frames = vec![(top, main)];
        let mut at = 0;
        while let Some((parent, parent_info)) = frames.get(at).cloned() {
            at = at.saturating_add(1);
            for (frame, of, owner) in &children {
                if *of != parent {
                    continue;
                }
                if let Some(info) = self.child_info(*frame, owner, &parent_info) {
                    frames.push((*frame, info));
                }
            }
        }
        frames.into_iter().map(|(_, info)| info).collect()
    }

    /// The frames as DevTools' `DOM` agent knows them: the top-level one
    /// first, then each child frame with its iframe; one whose iframe has
    /// no document now left out.
    pub(super) fn dom_frames(&mut self) -> Vec<super::dom_agent::DomFrame> {
        let Some(top) = self.document.as_ref().map(|next| next.document.clone()) else {
            return Vec::new();
        };
        let children: Vec<(FrameId, NodeRoot)> = self
            .children
            .frames
            .iter()
            .map(|(frame, (_, owner))| (*frame, owner.clone()))
            .collect();
        let ids: Vec<String> = children
            .iter()
            .map(|(frame, _)| self.frame_id(*frame))
            .collect();
        let main = self.devtools_main_frame_token.clone();
        let Some(engine) = self.engine.as_mut() else {
            return Vec::new();
        };
        engine.dom_mut().read(|cx| {
            let mut frames = vec![super::dom_agent::DomFrame {
                id: main,
                document: cx.node_id(top.get(cx.gc)),
                owner: None,
            }];
            for ((_, owner), id) in children.iter().zip(ids) {
                let owner = owner.get(cx.gc);
                if let Some(document) = cx.content_document(owner) {
                    frames.push(super::dom_agent::DomFrame {
                        id,
                        document: cx.node_id(document),
                        owner: Some(cx.node_id(owner)),
                    });
                }
            }
            frames
        })
    }

    /// Child frame `frame` of `parent`'s, its iframe `owner`, as DevTools
    /// tells it; none if its iframe has no document now.
    fn child_info(
        &mut self,
        frame: FrameId,
        owner: &NodeRoot,
        parent: &FrameInfo,
    ) -> Option<FrameInfo> {
        let navigating = self.children.fetches.values().any(|(of, ..)| *of == frame);
        let id = self.frame_id(frame);
        let committed = self.doc(frame).map(|next| {
            (
                next.mime_type.clone(),
                next.source.clone(),
                next.loader_id.clone(),
                next.resources.clone(),
                next.initial,
            )
        });
        let creation_stack = self.children.creation_stacks.get(&frame).cloned();
        let engine = self.engine.as_mut()?;
        let (document, url, origin, name) = engine.dom_mut().read(|cx| {
            let owner = owner.get(cx.gc);
            let document = cx.content_document(owner)?;
            let url: String = cx
                .document_url(document)
                .map_or_else(|| "about:blank".to_owned(), |url| url.to_str().into_owned());
            // As Blink's `SecurityOrigin::ToRawString`: an opaque one's
            // scheme and host are empty.
            let origin = match dom_api::origin(cx, document) {
                Some(origin @ url::Origin::Tuple(..)) => origin.ascii_serialization(),
                _ => "://".to_owned(),
            };
            let name = cx
                .attribute_value_by_id(owner, dom::names::local::NAME)
                .map(|name| name.borrow().to_str().into_owned())
                .unwrap_or_default();
            Some((u64::from(cx.node_id(document).get()), url, origin, name))
        })?;
        let secure_context = super::devtools::secure_context(&url, Some(parent.secure_context));
        let (mime_type, source, loader_id, resources, initial) = committed.unwrap_or_else(|| {
            (
                "text/html".to_owned(),
                Rc::from(""),
                None,
                Rc::default(),
                true,
            )
        });
        Some(FrameInfo {
            id,
            parent_id: Some(parent.id.clone()),
            // Its request's, if one brought it.
            loader_id: loader_id.unwrap_or_else(|| document.to_string()),
            document,
            name: Some(name),
            initial,
            navigating,
            url,
            origin,
            mime_type,
            secure_context,
            source,
            resources,
            creation_stack,
        })
    }

    /// The requests in flight that `frame`'s document (`document`) made
    /// (its document's `ResourceFetcher`'s, as Blink's `IdlenessDetector`
    /// counts): its sheets, its parser's scripts and those its DOM queued,
    /// the images it asked for first, its `fetch()`es. Not a child frame's
    /// `src` (its own document's request).
    fn requests_in_flight(&self, frame: FrameId, document: dom::NodeId) -> usize {
        let Some(next) = self.doc(frame) else {
            return 0;
        };
        let parser = next
            .blocking_script
            .iter()
            .chain(&next.deferred_scripts)
            .filter(|script| script.request_id.is_some() && script.ready.is_none())
            .count();
        let queued = self
            .script_fetches
            .values()
            .filter(|fetching| fetching.frame == frame)
            .count();
        let images = self
            .images
            .values()
            .filter(|(_, of)| *of == document)
            .count();
        let fetches = self
            .fetches
            .values()
            .filter(|(_, of)| *of == document)
            .count();
        // Each fewer than the requests the renderer has made.
        next.pending_sheets
            .len()
            .saturating_add(parser)
            .saturating_add(queued)
            .saturating_add(images)
            .saturating_add(fetches)
    }

    /// The DOM's frames' documents whose network has been quiet long
    /// enough (`check_network_idle`'s).
    pub(super) fn check_network_idle(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        let documents: Vec<(FrameId, dom::NodeId)> = self
            .document
            .iter()
            .chain(&self.children.documents)
            .map(|next| {
                (
                    next.frame,
                    engine
                        .dom_mut()
                        .read(|cx| cx.node_id(next.document.get(cx.gc))),
                )
            })
            .collect();

        let now = ipc::TimeTicks::now();
        let mut due = Vec::new();
        for (frame, document) in documents {
            let active = self.requests_in_flight(frame, document);
            if let Some(next) = self.doc_mut(frame) {
                due.extend(
                    next.idleness
                        .update(active, now)
                        .into_iter()
                        .map(|(name, at)| (frame, name, at)),
                );
            }
        }
        for (frame, name, at) in due {
            self.tell_lifecycle(frame, name, at);
        }
    }

    /// When a new DOM's document's network may next have been quiet long
    /// enough.
    pub(super) fn next_idleness_due(&self) -> Option<ipc::TimeTicks> {
        self.document
            .iter()
            .chain(&self.children.documents)
            .filter_map(|next| next.idleness.next_due())
            .min()
    }

    /// Asks for the sheet of each `<link>` of the document's tree that has
    /// one to load and hasn't asked yet, if its sheets may have changed
    /// (a `<link>` came, or one's attributes changed), and of each shadow
    /// tree's whose sheets may have (which blocks nothing: Blink's
    /// render-blocking sheets are the document tree's): resolved against
    /// the document's URL; one that doesn't resolve asks for nothing. Called
    /// whenever the parser stops and after tasks that changed the DOM, so
    /// loads start before the document is complete and include what
    /// script added.
    fn start_loads(&mut self) {
        for frame in self.frames() {
            self.start_loads_of(frame);
        }
    }

    /// `start_loads` for the document loading in `frame`: its tree's
    /// `<link>`s, and those of the shadow trees in it.
    fn start_loads_of(&mut self, frame: FrameId) {
        let Some(next) = self.doc(frame) else {
            return;
        };
        let document = next.document.clone();
        let requested = next.requested_links.clone();
        let Some(engine) = &mut self.engine else {
            return;
        };
        let links: Vec<LinkToLoad> = engine.dom_mut().read(|cx| {
            let document = document.get(cx.gc);
            let mut roots: Vec<(dom::NodeRef<'_>, bool)> = cx
                .sheet_changed_roots(document)
                .into_iter()
                .map(|root| (root, true))
                .collect();
            if cx
                .style_flags(document)
                .contains(dom::StyleFlags::STYLE_SHEETS_CHANGED)
            {
                roots.insert(0, (document, false));
            }
            let mut links = Vec::new();
            for (root, in_shadow) in roots {
                let mut node = cx.first_child(root);
                while let Some(at) = node {
                    if let Some(href) = cx.stylesheet_link_href(at) {
                        let id = cx.node_id(at);
                        if !requested.contains(&id) {
                            // HTML's CORS settings attribute: by CORS with
                            // one, `use-credentials` including them.
                            let cors = cx.get_attribute(at, "crossorigin").map(|value| {
                                if value.to_str().eq_ignore_ascii_case("use-credentials") {
                                    CredentialsMode::Include
                                } else {
                                    CredentialsMode::SameOrigin
                                }
                            });
                            links.push(LinkToLoad {
                                link: cx.root(at),
                                id,
                                href,
                                cors,
                                blocks_nothing: in_shadow || cx.is_alternate_stylesheet(at),
                                // The document's parser made it: it blocks
                                // the parser's scripts while it loads.
                                parsers: cx.script_flags(at).parser_inserted,
                                media: cx
                                    .get_attribute(at, "media")
                                    .map(|media| media.to_str().into_owned()),
                            });
                        }
                    }
                    node = next_in_tree_order(cx, at, root);
                }
            }
            links
        });
        for LinkToLoad {
            link,
            id,
            href,
            cors,
            blocks_nothing,
            parsers,
            media,
        } in links
        {
            // The parser's, for media that match as it starts loading:
            // the parser's scripts wait for it (HTML's "a style sheet that
            // is blocking scripts"; Blink's `PendingSheetType::kBlocking`
            // looks at its media then too). A child frame given no size
            // yet matches, so its scripts may as well wait.
            let blocks_scripts = !blocks_nothing
                && parsers
                && media.is_none_or(|media| {
                    let Some((dom, top)) = engine_frame(&mut self.engine) else {
                        return true;
                    };
                    let document = dom.read(|cx| cx.node_id(cx.document_of(link.get(cx.gc))));
                    top.media_environment(document).is_none_or(|environment| {
                        css::media::MediaQueryList::parse(&media).matches(&environment)
                    })
                });
            // Against the document's base URL.
            let Some(document) = self.doc(frame).map(|next| next.document.clone()) else {
                return;
            };
            let url = self.engine.as_mut().and_then(|engine| {
                engine
                    .dom_mut()
                    .read(|cx| cx.resolve_url(document.get(cx.gc), href.trim()))
            });
            let initiator = self.document_origin(frame);
            let Some(next) = self.doc_mut(frame) else {
                return;
            };
            next.requested_links.insert(id);
            let Some(url) = url else {
                next.failed.push(href);
                continue;
            };
            let mut request = ResourceRequest::new(url.as_str(), RequestDestination::Style);
            request.initiator = initiator;
            // By CORS, which an origin that allows it makes origin-clean
            // (and one that doesn't, a network error).
            if let Some(credentials) = cors {
                request.mode = RequestMode::Cors;
                request.credentials = credentials;
            }
            let request_id = self.send_request_with_initiator(frame, request, |this| {
                this.initiator(frame, &dom::LoadInitiator::Element(id))
            });
            let Some(next) = self.doc_mut(frame) else {
                return;
            };
            // An alternate one (or a shadow tree's) loads, but blocks
            // nothing.
            next.pending_sheets.insert(
                request_id,
                PendingSheet {
                    link,
                    url,
                    blocking: !blocks_nothing,
                    blocks_scripts,
                },
            );
        }
        self.update_child_blocking(frame);
    }

    /// Whether a sheet that blocks rendering is loading: no frame till
    /// it is in.
    pub(super) fn render_blocked(&self) -> bool {
        self.document
            .as_ref()
            .is_some_and(|next| next.pending_sheets.values().any(|sheet| sheet.blocking))
    }

    /// A child frame's view told whether a sheet blocks its rendering, as
    /// its document's sheets start and finish loading: it isn't styled or
    /// laid out meanwhile.
    fn update_child_blocking(&mut self, frame: FrameId) {
        if self.doc(frame).is_none_or(|next| next.child_of.is_none()) {
            return;
        }
        let blocked = self.frame_render_blocked(frame);
        if let Some((_, top)) = engine_frame(&mut self.engine)
            && let Some(view) = top.view.child_mut(frame)
        {
            view.set_render_blocked(blocked);
        }
    }

    /// Whether `frame`'s document waits for a sheet that blocks its
    /// rendering (`render_blocked`'s, for any frame).
    fn frame_render_blocked(&self, frame: FrameId) -> bool {
        self.doc(frame)
            .is_some_and(|next| next.pending_sheets.values().any(|sheet| sheet.blocking))
    }

    /// The document of the DOM's frame, as the compositor knows it.
    pub(super) fn frame_document_id(&mut self) -> Option<DocumentId> {
        engine_frame(&mut self.engine).map(|(_, frame)| frame.view.document_id())
    }

    /// A mouse or key input for the DOM's document
    /// (`ScriptEngine::handle_mouse`, `handle_key`: the hit test, hover and
    /// the events are `dom_api`'s), the frame asked for if it changed
    /// the DOM. A wheel is `handle_wheel`'s.
    /// What the DOM's document's selects asked of their popups, now
    /// that no script runs (Blink's `ExternalPopupMenu`): one to show goes
    /// to the browser with where the select is and its font size and
    /// color, in place of any other shown; one with no box, or that a press
    /// just closed, closes again at once. Then the one shown closes if its
    /// select can't keep it.
    pub(super) fn handle_popup_requests(&mut self) {
        // The selects' events may ask for more; a few rounds at most.
        for _ in 0..4 {
            let Some(engine) = &mut self.engine else {
                return;
            };
            let requests = engine.dom_mut().mutate(|cx| cx.take_popup_requests());
            if requests.is_empty() {
                break;
            }
            for request in requests {
                match request {
                    dom::PopupRequest::Show(select) => self.show_popup(select),
                    dom::PopupRequest::Hide(select) => {
                        if self.popup == Some(select) {
                            self.popup = None;
                            let _ = self.to_browser.send(RendererHostMsg::HidePopupMenu);
                        }
                    }
                }
            }
        }
        let gone = self.popup.is_some_and(|select| {
            self.engine.as_mut().is_none_or(|engine| {
                !engine.dom_mut().read(|cx| {
                    cx.node_by_id(select)
                        .is_some_and(|node| dom_api::can_keep_popup(&dom_api::ApiCx::new(cx), node))
                })
            })
        });
        if gone && let Some(select) = self.popup.take() {
            let _ = self.to_browser.send(RendererHostMsg::HidePopupMenu);
            self.close_popup(select, PopupMenuResult::Canceled { by_press: false });
        }
    }

    /// Shows `select`'s popup, laid out first: its items, where it is.
    fn show_popup(&mut self, select: dom::NodeId) {
        let canceled = PopupMenuResult::Canceled { by_press: false };
        if self.popup_closed_by_press == Some(select) {
            self.close_popup(select, canceled);
            return;
        }
        if let Some(other) = self.popup.take() {
            self.close_popup(other, canceled);
        }
        let menu = engine_frame(&mut self.engine).and_then(|(dom, frame)| {
            dom.mutate(|cx| frame.update_style_and_layout(cx));
            dom.read(|cx| {
                let node = cx.node_by_id(select)?;
                let bounds = frame.client_rects(select).first().copied()?;
                let style = frame.computed_style(cx, node)?;
                let (items, selected) = dom_api::popup_items(&dom_api::ApiCx::new(cx), node);
                Some(PopupMenu {
                    bounds,
                    font_size: style.font.size,
                    color: style.color,
                    items: items
                        .into_iter()
                        .map(|item| MenuItem {
                            label: item.label,
                            kind: match item.kind {
                                dom_api::PopupItemKind::Option => MenuItemKind::Option,
                                dom_api::PopupItemKind::Group => MenuItemKind::Group,
                                dom_api::PopupItemKind::Separator => MenuItemKind::Separator,
                            },
                            enabled: item.enabled,
                        })
                        .collect(),
                    selected,
                    // machopo's pages have no `direction` yet.
                    right_aligned: false,
                })
            })
        });
        match menu {
            Some(menu) => {
                let _ = self.to_browser.send(RendererHostMsg::ShowPopupMenu(menu));
                self.popup = Some(select);
            }
            None => self.close_popup(select, canceled),
        }
    }

    /// Tells `select` its popup closed with `result`.
    pub(super) fn close_popup(&mut self, select: dom::NodeId, result: PopupMenuResult) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        let result = match result {
            PopupMenuResult::Accepted { index, by_keyboard } => {
                dom_api::PopupResult::Accepted { index, by_keyboard }
            }
            PopupMenuResult::Canceled { .. } => dom_api::PopupResult::Canceled,
        };
        engine.popup_menu_closed(select, result);
    }

    pub(super) fn handle_input(&mut self, event: &InputEvent) {
        // DevTools' inspect mode takes the mouse: what it is over is
        // highlighted, a click inspects it, and the page sees none of it
        // (Blink's `InspectorOverlayAgent::HandleInputEvent`).
        if self.overlay_agent.inspecting()
            && let Some(engine) = &mut self.engine
        {
            let mut page = engine.devtools_page();
            match *event {
                InputEvent::MouseMove { x, y, .. } => {
                    self.overlay_agent
                        .mouse_moved(PointF::new(x, y), &mut *page);
                    return;
                }
                InputEvent::MouseDown { x, y, .. } => {
                    self.overlay_agent.clicked(PointF::new(x, y), &mut *page);
                    return;
                }
                InputEvent::MouseUp { .. } => return,
                _ => {}
            }
        }
        if let InputEvent::Key(key) = event {
            let Some(engine) = &mut self.engine else {
                return;
            };
            let result = engine.handle_key(&KeyInput {
                down: key.down,
                key: key.key.clone(),
                code: key.code.clone(),
                key_code: key.key_code.into(),
                text: key.text.clone(),
                location: key.location.into(),
                repeat: key.repeat,
                modifiers: dom_modifiers(key.modifiers),
            });
            if let KeyResult::TakeFocus { reverse } = result {
                let _ = self.to_browser.send(RendererHostMsg::TakeFocus { reverse });
            }
            self.request_next_frame_if_dom_changed();
            self.send_harness_results();
            return;
        }
        let (point, action, buttons, modifiers) = match *event {
            InputEvent::MouseMove {
                x,
                y,
                buttons,
                modifiers,
            } => (PointF::new(x, y), MouseAction::Move, buttons, modifiers),
            InputEvent::MouseDown {
                x,
                y,
                button,
                buttons,
                modifiers,
                click_count,
            } => (
                PointF::new(x, y),
                MouseAction::Down {
                    button: dom_button(button),
                    click_count: base::numerics::saturated_cast(click_count),
                },
                buttons,
                modifiers,
            ),
            InputEvent::MouseUp {
                x,
                y,
                button,
                buttons,
                modifiers,
                click_count,
            } => (
                PointF::new(x, y),
                MouseAction::Up {
                    button: dom_button(button),
                    click_count: base::numerics::saturated_cast(click_count),
                },
                buttons,
                modifiers,
            ),
            InputEvent::MouseWheel { .. } | InputEvent::Key(_) => return,
        };
        // A press on the page closes the popup (the browser hands it on
        // from outside the list), before it is dispatched; the press
        // doesn't open it again.
        if matches!(
            action,
            MouseAction::Down {
                button: dom_api::MouseButton::Primary,
                ..
            }
        ) && let Some(select) = self.popup.take()
        {
            let _ = self.to_browser.send(RendererHostMsg::HidePopupMenu);
            self.close_popup(select, PopupMenuResult::Canceled { by_press: true });
            self.popup_closed_by_press = Some(select);
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        let cursor = engine.handle_mouse(&MouseInput {
            action,
            point,
            buttons,
            modifiers: dom_modifiers(modifiers),
        });
        self.note_under_mouse();
        self.set_cursor(cursor_type(cursor));
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// What is under the pointer was just found, as the top-level view is
    /// pre-painted now.
    fn note_under_mouse(&mut self) {
        self.under_mouse_pre_paints = engine_frame(&mut self.engine)
            .map(|(_, frame)| (frame.view.document_id(), frame.view.pre_paints()));
    }

    /// What is under a pointer that stays, found again after a commit
    /// whose pre-paint (a layout, a scroll, a new style) may have moved it
    /// (Blink's `MouseEventManager::RecomputeMouseHoverState` once the
    /// hover state is dirty, and `EventHandler::ScheduleCursorUpdate`): the
    /// boundary events and the cursor, no `mousemove`. Not while a move
    /// waits (it finds it), nor before the pointer was over the page; one
    /// hit test, only when the pre-paint moved.
    pub(super) fn recompute_under_mouse(&mut self) {
        if self.pending_mouse_move.is_some() {
            return;
        }
        let Some(noted) = self.under_mouse_pre_paints else {
            return;
        };
        let now = engine_frame(&mut self.engine)
            .map(|(_, frame)| (frame.view.document_id(), frame.view.pre_paints()));
        if now.is_none_or(|now| now == noted) {
            return;
        }
        let _span = trace_event::trace_event!("MouseEventManager::RecomputeMouseHoverState");
        let Some(engine) = &mut self.engine else {
            return;
        };
        let cursor = engine.recompute_under_mouse();
        self.note_under_mouse();
        if let Some(cursor) = cursor {
            self.set_cursor(cursor_type(cursor));
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// `WidgetHost::SetCursor`, when it changes (Blink's
    /// `ChromeClientImpl::SetCursor`).
    pub(super) fn set_cursor(&mut self, cursor: CursorType) {
        if cursor != self.cursor {
            self.cursor = cursor;
            let _ = self.to_browser.send(RendererHostMsg::SetCursor(cursor));
        }
    }

    /// A wheel for the DOM's document: its `wheel` event
    /// (`ScriptEngine::handle_wheel`, cancelable if the compositor waits for
    /// it), then, if `scroll` (the compositor left it to the main thread)
    /// and nothing canceled it, the scroll the main thread does
    /// (`Frame::wheel`); a frame asked for if anything moved or the DOM
    /// changed.
    pub(super) fn handle_wheel(&mut self, event: &InputEvent, cancelable: bool, scroll: bool) {
        let InputEvent::MouseWheel {
            x,
            y,
            delta,
            buttons,
            modifiers,
        } = *event
        else {
            return;
        };
        let Some(engine) = &mut self.engine else {
            return;
        };
        let point = PointF::new(x, y);
        let step = compositor::wheel_scroll_delta(delta);
        let not_canceled = engine.handle_wheel(&WheelInput {
            point,
            delta: step,
            buttons,
            modifiers: dom_modifiers(modifiers),
            cancelable,
        });
        if scroll
            && not_canceled
            && let Some((dom, frame)) = engine_frame(&mut self.engine)
            && frame.wheel(dom, point, step)
        {
            self.request_main_frame();
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// Whether the DOM's window has an animation frame callback: a
    /// frame is wanted.
    pub(super) fn wants_animation_frame(&mut self) -> bool {
        self.engine
            .as_mut()
            .is_some_and(|engine| engine.has_animation_frames())
    }

    /// Whether the DOM's documents have animations that run, or events
    /// of theirs to send (script played or canceled one since the last
    /// frame): a frame is wanted.
    pub(super) fn animations_want_frames(&mut self) -> bool {
        self.animations_running
            || self
                .engine
                .as_mut()
                .is_some_and(|engine| engine.animations_want_frames())
    }

    /// HTML's "evaluate media queries and report changes" of each of the
    /// new DOM's documents in tree order (`ScriptEngine::
    /// report_media_query_changes`), before the animation frame callbacks.
    pub(super) fn report_media_query_changes(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        for frame in engine.frames_in_tree_order() {
            engine.report_media_query_changes(frame);
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// HTML's "update animations and send events" of each of the DOM's
    /// documents in tree order (`ScriptEngine::update_animations`), at this
    /// main frame's time, before the animation frame callbacks: what moved
    /// on is styled in this frame's rendering update; frames are wanted
    /// while they run.
    pub(super) fn update_animations(&mut self, frame_time: std::time::Instant) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        let _span =
            trace_event::trace_event!("DocumentAnimations::UpdateAnimationTimingForAnimationFrame");
        let mut update = dom_api::AnimationsUpdate::default();
        for frame in engine.frames_in_tree_order() {
            let frame = engine.update_animations(frame, frame_time);
            update.changed |= frame.changed;
            update.running |= frame.running;
        }
        self.animations_running = update.running;
        if update.changed {
            self.request_main_frame();
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// The DOM's animations that the rendering about to be committed
    /// shows start at its time (`ScriptEngine::start_pending_animations`,
    /// Blink's `PendingAnimations::Update`); another frame moves them on.
    /// Only the frames it rendered: a frame made since (by script the
    /// promises settled here ran) had nothing styled, its keyframes not
    /// computed, and waits for the next.
    fn start_pending_animations(&mut self) {
        let _span = trace_event::trace_event!("PendingAnimations::Update");
        let mut shown = Vec::new();
        if let Some((_, frame)) = engine_frame(&mut self.engine) {
            frame.view.child_frames_shown(&mut shown);
        }
        let top = self.document.as_ref().map(|next| next.frame);
        let Some(engine) = &mut self.engine else {
            return;
        };
        let mut running = false;
        for frame in engine.frames_in_tree_order() {
            if Some(frame) == top || shown.contains(&frame) {
                running |= engine.start_pending_animations(frame);
            }
        }
        self.animations_running |= running;
    }

    /// HTML's "run the animation frame callbacks" of the DOM's window
    /// (`ScriptEngine::run_animation_frames`), at this main frame's time,
    /// before its rendering update: what they change is drawn in it.
    pub(super) fn run_animation_frames(&mut self, frame_time: std::time::Instant) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        if !engine.has_animation_frames() {
            return;
        }
        let _span = trace_event::trace_event!("PageAnimator::serviceScriptedAnimations");
        engine.run_animation_frames(frame_time);
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// The rendering update's resize observer loop for each of the DOM's
    /// documents (`run_resize_observer_steps`'s, after the animation frame
    /// callbacks): each round lays the frame out up to pre-paint, then the
    /// window gathers what changed deeper than its depth so far and calls
    /// its observers back; another round while it did (Chrome's
    /// `LocalFrameView::UpdateLifecyclePhases` repeats the lifecycle while
    /// `RunResizeObserverSteps` delivers). One it skipped (no deeper than
    /// the depth) reports the loop error, and a later frame delivers it.
    /// Nothing while the document is render-blocked.
    pub(super) fn run_resize_observer_steps(&mut self) {
        if self.render_blocked() {
            return;
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        // Each document's loop in tree order (HTML's: for each of the
        // rendering update's documents).
        for frame in engine.frames_in_tree_order() {
            self.run_resize_observer_loop(frame);
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// `run_resize_observer_steps`' loop for `frame`'s document.
    fn run_resize_observer_loop(&mut self, frame: FrameId) {
        if self.frame_render_blocked(frame) {
            return;
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        if !engine.has_resize_observations(frame) {
            return;
        }
        let _span = trace_event::trace_event!("LocalFrameView::NotifyResizeObservers");
        let mut depth = 0;
        let skipped = loop {
            if let Some((dom, view)) = engine_frame(&mut self.engine) {
                dom.mutate(|cx| view.update_lifecycle_to_pre_paint(cx));
            }
            let Some(engine) = &mut self.engine else {
                return;
            };
            let round = engine.gather_and_broadcast_resizes(frame, depth);
            match round.broadcast {
                Some(deeper) => depth = deeper,
                None => break round.skipped,
            }
        };
        if skipped && let Some(engine) = &mut self.engine {
            engine.report_resize_loop_error(frame);
            self.resize_loop_error = true;
        }
    }

    /// The page took or lost the system's focus (`RendererMsg::SetFocus`)
    /// on the DOM: the engine keeps it for each window and fires the
    /// focus events (the render thread keeps it too, for an engine not made
    /// yet); what `:focus` matches is drawn with the next frame.
    pub(super) fn set_page_focused(&mut self, focused: bool) {
        // Kept for the first document, which may come later.
        let changed = std::mem::replace(&mut self.page_focused, focused) != focused;
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.set_page_focused(focused);
        // `:window-inactive` may match otherwise: style looks at the next
        // frame (it changes no DOM).
        if changed {
            self.request_main_frame();
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// `RendererMsg::SetInitialFocus` on the DOM: Tab from the toolbar
    /// into the page (`ScriptEngine::set_initial_focus`).
    pub(super) fn set_document_initial_focus(&mut self, reverse: bool) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.set_initial_focus(reverse);
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// HTML's "flush autofocus candidates" of the DOM's document at the
    /// start of the rendering update
    /// (`ScriptEngine::flush_autofocus_candidates`).
    pub(super) fn flush_autofocus_candidates(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.flush_autofocus_candidates();
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// HTML's focus fixup of the DOM's document at the rendering update
    /// (`ScriptEngine::run_focus_fixup`).
    pub(super) fn run_focus_fixup(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.run_focus_fixup();
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// Asks for a main frame if the DOM's trees changed since the
    /// rendering was last asked for (`request_main_frame_if_dom_changed`).
    pub(super) fn request_next_frame_if_dom_changed(&mut self) {
        let (Some(engine), Some(next)) = (&mut self.engine, &mut self.document) else {
            return;
        };
        let (version, intersections, resizes, canvas_draws) = engine.dom_mut().read(|cx| {
            (
                cx.tree_version(),
                cx.intersection_version(),
                cx.resize_version(),
                cx.canvas_draws_version(),
            )
        });
        // A canvas drew: the next frame shows it.
        let observed = intersections != next.rendered_intersections
            || resizes != next.rendered_resizes
            || canvas_draws != next.rendered_canvas_draws;
        next.rendered_intersections = intersections;
        next.rendered_resizes = resizes;
        next.rendered_canvas_draws = canvas_draws;
        // Script's scrolling, which changes no DOM, asks for one too.
        let scrolled = engine_frame(&mut self.engine)
            .is_some_and(|(_, frame)| frame.view.take_scrolled_by_script());
        let Some(next) = &mut self.document else {
            return;
        };
        if version != next.rendered_version {
            next.rendered_version = version;
            self.start_loads();
            self.request_main_frame();
        } else if observed || scrolled {
            self.request_main_frame();
        }
    }

    /// The window's size and scale given the DOM's frame as soon as
    /// they are known (at the commit, at each change), as Blink's frame
    /// view has its size at once: what script's layout uses before the
    /// next frame. The browser window's rect on the screen with them.
    pub(super) fn set_viewport(&mut self) {
        let (viewport, dsf) = super::viewport(&self.visual_properties);
        let window = self.window_screen_rect;
        let Some((_, frame)) = engine_frame(&mut self.engine) else {
            return;
        };
        let changed = frame.set_viewport(viewport, dsf);
        frame.set_window_screen_rect(window);
        // Its `<img>`s with `srcset` or in a `<picture>` select again.
        if changed && let Some(engine) = &mut self.engine {
            let top = engine.top_frame();
            engine.react_to_environment_changes(top);
        }
    }

    /// `update_rendering` for the DOM's document: its lifecycle, then
    /// the commit, `DidFinishLoad` with the first frame after its `load`.
    /// Whether there was one to update.
    pub(super) fn update_document_rendering(&mut self) -> bool {
        // A sheet's response asks for the next frame.
        if self.render_blocked() {
            return false;
        }
        let (Some((dom, frame)), Some(next)) = (engine_frame(&mut self.engine), &mut self.document)
        else {
            return false;
        };
        (next.rendered_version, next.rendered_canvas_draws) =
            dom.read(|cx| (cx.tree_version(), cx.canvas_draws_version()));
        update_lifecycle_and_layers(dom, frame, &self.overlay_agent, &self.css_agent);
        // A caret that came or moved shows, then blinks from there.
        if frame.view.take_caret_moved() {
            self.next_caret_blink = self
                .caret_blink_interval
                .filter(|_| frame.view.has_caret())
                .map(|interval| {
                    #[expect(
                        clippy::arithmetic_side_effects,
                        reason = "the system's blink interval (a second or so) after now"
                    )]
                    let due = std::time::Instant::now() + interval;
                    due
                });
        }
        let Frame { view, widget, .. } = frame;
        let reftest_wait = self.web_tests && {
            let document = view.document();
            dom.read(|cx| document::root_has_class(cx, document.get(cx.gc), "reftest-wait"))
        };
        let mut promises = std::mem::take(&mut widget.pending_input_acks);
        promises.append(&mut widget.pending_visual_states);
        let to_browser = self.to_browser.clone();
        let navigation_id = next.navigation_id;
        if std::mem::take(&mut next.load_finished) {
            next.waiting_for_reftest = reftest_wait;
            let failed_subresources = std::mem::take(&mut next.failed);
            promises.push(SwapPromise::new(move |frame_token| {
                let _ = to_browser.send(RendererHostMsg::DidFinishLoad {
                    navigation_id,
                    frame_token,
                    failed_subresources,
                    reftest_wait,
                });
            }));
        } else if next.waiting_for_reftest && !reftest_wait {
            next.waiting_for_reftest = false;
            promises.push(SwapPromise::new(move |frame_token| {
                let _ = to_browser.send(RendererHostMsg::ReftestWaitEnded {
                    navigation_id,
                    frame_token,
                });
            }));
        }
        // The transitions and CSS animations this frame's style started,
        // which it shows at their start, start at its time.
        self.start_pending_animations();
        trace_event::in_scope(
            trace_event::trace_event!("ProxyMain::BeginMainFrame::commit"),
            || self.widget().layer_tree_host.commit(promises),
        );
        // The child frames laid out at another size: their documents'
        // environment changed (their `<img>`s select again), and the next
        // rendering update evaluates their media queries (Blink's
        // `MediaQueryAffectingValueChanged` schedules a frame).
        let resized = engine_frame(&mut self.engine)
            .map(|(_, frame)| frame.view.take_resized_frames())
            .unwrap_or_default();
        // Layout clamping a scroll offset moved it: its `scroll` event goes
        // in the next frame's scroll steps.
        if engine_frame(&mut self.engine).is_some_and(|(_, frame)| frame.view.has_scroll_events()) {
            self.request_main_frame();
        }
        if let Some(engine) = &mut self.engine {
            let any = !resized.is_empty();
            for frame in resized {
                engine.react_to_environment_changes(frame);
            }
            if any {
                self.request_main_frame();
            }
        }
        // The intersections this frame's pre-paint makes (Blink computes
        // them in `RunPostLifecycleSteps`), the entries queued now, the
        // task that delivers them run between tasks.
        // The child frames' documents whose rendering is blocked.
        let blocked: Vec<NodeRoot> = self
            .children
            .documents
            .iter()
            .filter(|next| next.pending_sheets.values().any(|sheet| sheet.blocking))
            .map(|next| next.document.clone())
            .collect();
        let documents = self
            .engine
            .as_mut()
            .map(|engine| engine.documents_in_tree_order())
            .unwrap_or_default();
        if let Some((dom, frame)) = engine_frame(&mut self.engine) {
            let _span =
                trace_event::trace_event!("LocalFrameView::UpdateViewportIntersectionsForSubtree");
            let (queue, due) = compute_intersections(
                dom,
                frame,
                &documents,
                &blocked,
                &mut self.intersections_computed,
            );
            self.intersection_task |= queue;
            self.intersections_due = due.map(|wait| self.clock.now() + wait);
        }
        self.recompute_under_mouse();
        true
    }

    /// The DOM's window's intersection observers' task, if the
    /// rendering update queued it (`dom_api::update_intersections`).
    pub(super) fn run_intersection_task(&mut self) {
        if !std::mem::take(&mut self.intersection_task) {
            return;
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.notify_intersection_observers();
        self.send_harness_results();
    }

    /// `run_scroll_steps` for the DOM's documents (HTML's "run the
    /// scroll steps"): a `scroll` event at each target that moved since
    /// the last frame (the compositor's, a wheel the main thread took,
    /// script's), in the order it first moved; and this frame updated, so
    /// that its intersections are computed again.
    pub(super) fn run_scroll_steps(&mut self) {
        let Some((_, frame)) = engine_frame(&mut self.engine) else {
            return;
        };
        let mut due = Vec::new();
        frame.view.take_scroll_events(&mut due);
        if due.is_empty() {
            return;
        }
        self.request_main_frame();
        let Some(engine) = &mut self.engine else {
            return;
        };
        for target in due {
            engine.fire_scroll_event(target);
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// `apply_compositor_scroll` for the DOM's document.
    pub(super) fn apply_document_compositor_scroll(
        &mut self,
        scroll: &compositor::CompositorScrollDelta,
    ) {
        let Some((_, frame)) = engine_frame(&mut self.engine) else {
            return;
        };
        if frame.view.document_id() == scroll.document && frame.view.apply_compositor_scroll(scroll)
        {
            self.request_main_frame();
        }
    }

    /// The DOM's document's timers due now (`until_next_due` woke the
    /// loop for them), then its testharness results if they gave any.
    pub(super) fn run_due_tasks(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        if engine.run_due_tasks() {
            self.finish_loaded_documents();
            self.send_harness_results();
            // A task's image `load` may have been what a `load` waited
            // for.
            self.finish_loads();
            self.request_next_frame_if_dom_changed();
        }
    }

    /// `run_due_tasks` after one of the loop's own tasks (input, the
    /// browser's messages), if the DOM may have one queued: what that task
    /// queued (a fragment navigation's `hashchange`) runs before the next
    /// such task. One `bool` read otherwise.
    pub(super) fn run_queued_tasks(&mut self) {
        if self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.dom_mut().has_queued_tasks())
        {
            self.run_due_tasks();
        }
    }

    /// Whether the DOM's windows have idle callbacks waiting, and one
    /// they may run in the idle period running.
    pub(super) fn idle_callbacks(&mut self) -> (bool, bool) {
        let Some(engine) = &mut self.engine else {
            return (false, false);
        };
        if !engine.has_idle_callbacks() {
            return (false, false);
        }
        (true, engine.has_runnable_idle_callback())
    }

    /// An idle period starts in the DOM's windows too.
    pub(super) fn start_idle_period(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.start_idle_period();
        }
    }

    /// One of the DOM's idle callbacks of the idle period ending at
    /// `deadline` (on the clock), as a task of its own, then its
    /// testharness results if it gave any.
    pub(super) fn run_document_idle_callback(&mut self, deadline: f64) {
        let clock = self.clock;
        let Some(engine) = &mut self.engine else {
            return;
        };
        // A deadline past what an `Instant` holds: now's.
        let deadline = clock
            .instant_at(deadline)
            .unwrap_or_else(std::time::Instant::now);
        if engine.run_idle_callback(deadline) {
            self.send_harness_results();
            self.request_next_frame_if_dom_changed();
        }
    }

    /// The images the DOM's list of available images waits for
    /// (`Dom::take_image_fetches`), fetched.
    pub(super) fn start_image_fetches(&mut self) {
        // What the engine asked for since the frame requests were last
        // handled (a task's blob URLs among them): the browser hears of a
        // blob URL before a load of it.
        self.handle_frame_requests();
        let Some(engine) = &mut self.engine else {
            return;
        };
        // From the document's origin (HTML's "create a potential-CORS
        // request", no-cors): the network service taints another origin's
        // response (opaque), which a canvas that draws it can't be read
        // after.
        let initiator = self.document.as_ref().map(|next| match next.url.origin() {
            origin @ url::Origin::Tuple(..) => origin.ascii_serialization(),
            url::Origin::Opaque(_) => "null".to_owned(),
        });
        let fetches = engine.dom_mut().take_image_fetches();
        for dom::ImageFetch {
            url,
            document,
            initiator: asker,
        } in fetches
        {
            // A blob URL revoked before the element resolved it (in its
            // task, before the browser heard): a network error, without
            // asking.
            if let Ok(parsed) = Url::parse(&url)
                && parsed.scheme() == "blob"
                && self.blob_urls.is_revoked(&parsed)
            {
                if let Some(engine) = &mut self.engine {
                    engine
                        .dom_mut()
                        .mutate(|cx| cx.image_fetched(&url, None, true));
                }
                continue;
            }
            let mut request = ResourceRequest::new(&url, RequestDestination::Image);
            request.initiator = initiator.clone();
            // Told from its document's frame, which asked first (the
            // top-level one's, if none shows it now).
            let frame = if self.network_agent.enabled() {
                self.frame_of_document(document)
                    .or_else(|| self.document.as_ref().map(|next| next.frame))
            } else {
                None
            };
            let request_id = match frame {
                Some(frame) => self.send_request_with_initiator(frame, request, |this| {
                    this.initiator(frame, &asker)
                }),
                None => self.send_request_untold(request),
            };
            self.images.insert(request_id, (url, document));
        }
    }

    /// What the DOM's `toBlob` calls are to encode
    /// (`Dom::take_canvas_encodes`), sent to the encoder thread, in order.
    pub(super) fn start_canvas_encodes(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        for encode in engine.dom_mut().take_canvas_encodes() {
            self.canvas_encoder.send(encode);
        }
    }

    /// The file of the DOM's `toBlob` call `id`, encoded: handed to the
    /// DOM, which queues the task that calls its callback
    /// (`Cx::canvas_encoded`).
    pub(super) fn canvas_encoded(&mut self, id: u64, file: Option<Vec<u8>>) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.dom_mut().mutate(|cx| cx.canvas_encoded(id, file));
    }

    /// An image the DOM's list of available images fetched: its
    /// natural size read from its header (raster decodes it as it draws
    /// it), handed to the DOM, which settles the requests waiting for it;
    /// the frame drawn again, the document's `load` if it waited for it.
    /// Whether it was an image (one that doesn't decode failed).
    fn image_loaded(&mut self, url: &str, result: &LoadResult) -> bool {
        if self.engine.is_none() {
            return true;
        }
        // Whatever the status: an image a 404 sends shows, as in Blink. An
        // SVG image is parsed into a document of its own first, in the
        // page's DOM (Blink's `ImageResourceContent` makes an `SVGImage` for
        // its MIME type), kept by the frame.
        let image = result.as_ref().ok().and_then(|response| {
            if response.mime_type == "image/svg+xml" {
                let (dom, frame) = engine_frame(&mut self.engine)?;
                let image = SvgImage::new(
                    dom,
                    &mut frame.token,
                    response.charset.as_deref(),
                    &response.body,
                )?;
                let natural = image.natural();
                return Some(dom::Image::Svg(frame.svg_images.add(image), natural));
            }
            let size = raster::image_size(&response.body)?;
            Some(dom::Image::Raster(
                gfx::ImageData::shared(response.body.clone()),
                size,
            ))
        });
        let Some(engine) = &mut self.engine else {
            return true;
        };
        let origin_clean = !matches!(
            result,
            Ok(response) if response.tainting == ResponseTainting::Opaque
        );
        let decoded = image.is_some();
        engine
            .dom_mut()
            .mutate(|cx| cx.image_fetched(url, image, origin_clean));
        if let Some((_, frame)) = engine_frame(&mut self.engine) {
            frame.view.images_changed();
        }
        self.request_main_frame();
        self.finish_loads();
        decoded
    }

    /// The DOM's engine's platform tasks that are ready, with a
    /// microtask checkpoint after them.
    pub(super) fn run_platform_tasks(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        if engine.run_platform_tasks() {
            engine.perform_microtask_checkpoint();
            self.send_harness_results();
        }
    }

    /// The DOM's document's testharness results, once its page
    /// reported them (web tests only).
    pub(super) fn send_harness_results(&mut self) {
        let (Some(engine), Some(next)) = (&mut self.engine, &self.document) else {
            return;
        };
        if let Some(results) = engine.take_harness_results() {
            let _ = self.to_browser.send(RendererHostMsg::TestharnessResults {
                navigation_id: next.navigation_id,
                results,
            });
        }
    }

    /// Whether `request_id` is a new DOM's child frame's `src` (whose
    /// redirects its navigation timing counts).
    pub(super) fn is_child_fetch(&self, request_id: u64) -> bool {
        self.children.fetches.contains_key(&request_id)
    }

    /// A response the DOM's document asked for: its parser-blocking
    /// script runs (one that failed to load doesn't), then parsing goes
    /// on. Whether it was the document's.
    /// A `fetch()`'s request sent (Blink's `FetchManager::Loader`), its
    /// response the engine's by its number (`fetch_answered`); a blob
    /// URL revoked before is a network error, now.
    fn start_fetch(&mut self, frame: FrameId, request: dom::FetchRequest) {
        if fetch_of_revoked_blob(&self.blob_urls, &request) {
            self.fetch_went_nowhere(request.id);
            return;
        }
        let resource = fetch_resource_request(&request);
        // Script asked: its stack, if DevTools wants it (none: no one it
        // names).
        let fetch = request.id;
        let request_id = self.send_request_with_initiator(frame, resource, |this| {
            let stack = this
                .engine
                .as_mut()?
                .dom_mut()
                .take_request_stack(dom::StackAsker::Fetch(fetch))?;
            this.network_agent
                .wants_stacks()
                .then_some(Initiator::Script(stack))
        });
        self.fetches
            .insert(request_id, (request.id, request.document));
    }

    /// A `fetch()` the load sink sent as script called it
    /// (`RendererLoadSink`, `request_id`): kept to give its response back by,
    /// and told to DevTools' `Network` now, with the stack of the script
    /// that asked; one it sent nowhere (a revoked blob URL's) a network
    /// error now.
    fn fetch_sent(&mut self, frame: FrameId, request: dom::FetchRequest, request_id: Option<u64>) {
        let Some(request_id) = request_id else {
            self.fetch_went_nowhere(request.id);
            return;
        };
        let fetch = request.id;
        if self.network_agent.enabled() {
            let resource = fetch_resource_request(&request);
            let initiator = self
                .engine
                .as_mut()
                .and_then(|engine| {
                    engine
                        .dom_mut()
                        .take_request_stack(dom::StackAsker::Fetch(fetch))
                })
                .filter(|_| self.network_agent.wants_stacks())
                .map(Initiator::Script);
            if let Some(at) = self.request_frame(frame, &resource) {
                self.network_agent
                    .will_send(request_id, &resource, at, initiator.as_ref());
            }
        }
        self.fetches.insert(request_id, (fetch, request.document));
    }

    /// The `fetch()` numbered `fetch` went nowhere (a revoked blob URL): a
    /// network error, in a task of its own (this one).
    fn fetch_went_nowhere(&mut self, fetch: u64) {
        if let Some(engine) = &mut self.engine {
            engine.fetch_response(fetch, Err(()));
        }
        self.send_harness_results();
    }

    /// The `fetch()`es still on their way whose documents have gone, or
    /// were discarded (their iframe went, or navigated): cancelled (Blink's
    /// `FetchManager` cancels its loaders as its execution context is
    /// destroyed), their promises never settled.
    fn cancel_fetches_of_gone_documents(&mut self) {
        if self.fetches.is_empty() {
            return;
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        let gone: Vec<u64> = engine.dom_mut().read(|cx| {
            self.fetches
                .values()
                .filter(|(_, document)| {
                    cx.node_by_id(*document)
                        .is_none_or(|document| cx.is_discarded(document))
                })
                .map(|(fetch, _)| *fetch)
                .collect()
        });
        for fetch in gone {
            self.cancel_fetch(fetch);
        }
    }

    /// The fetch numbered `id` aborted: its request cancelled, if it is
    /// still on its way.
    fn cancel_fetch(&mut self, id: u64) {
        let Some(request_id) = self
            .fetches
            .iter()
            .find(|&(_, &(fetch, _))| fetch == id)
            .map(|(&request_id, _)| request_id)
        else {
            return;
        };
        self.fetches.remove(&request_id);
        self.next_request_id.cancelled(request_id);
        self.network_agent.did_cancel(request_id);
        let _ = self
            .to_browser
            .send(RendererHostMsg::CancelResource { request_id });
    }

    /// The response to a `fetch()` of the DOM's, or its network error:
    /// its promise settled, as a task of its own (this one).
    fn fetch_answered(&mut self, id: u64, result: &LoadResult) {
        let response = match result {
            Ok(response) => Ok(dom_api::FetchedResponse {
                tainting: response.tainting,
                url: response.url.clone(),
                redirected: response.redirected,
                status: response.status,
                status_text: response.status_text.clone(),
                headers: response.headers.clone(),
                body: Arc::from(&response.body[..]),
            }),
            // Script learns no more of why (Fetch's network error).
            Err(_) => Err(()),
        };
        let Some(engine) = &mut self.engine else {
            return;
        };
        if engine.fetch_response(id, response) {
            self.send_harness_results();
            self.request_next_frame_if_dom_changed();
        }
    }

    pub(super) fn resource(
        &mut self,
        request_id: u64,
        asked: Option<&str>,
        result: &LoadResult,
        redirects: &[protocol::renderer::RedirectHop],
    ) -> bool {
        if let Some((id, _)) = self.fetches.remove(&request_id) {
            self.fetch_answered(id, result);
            return true;
        }
        if let Some((url, document)) = self.images.remove(&request_id) {
            if !self.image_loaded(&url, result) {
                let frame = self
                    .frame_of_document(document)
                    .or_else(|| self.document.as_ref().map(|next| next.frame));
                if let Some(frame) = frame {
                    self.note_failed(frame, failed_url(result, Some(&url)));
                }
            }
            return true;
        }
        // A child frame's `src`: its document committed (a failed load's
        // empty, where Chromium commits an error page).
        if let Some((frame, url, started, navigation)) = self.children.fetches.remove(&request_id) {
            let loader_id = Some(self.network_agent.request_id(request_id));
            match result {
                Ok(response) => {
                    // Its fragment the request's, kept by `net` through
                    // redirects.
                    let url = Url::parse(&response.url).unwrap_or(url);
                    self.commit_child(
                        frame,
                        url,
                        &response.mime_type,
                        response.charset.as_deref(),
                        &response.body,
                        Some((result, started, redirects, navigation)),
                        loader_id,
                    );
                }
                Err(_) => self.commit_child(
                    frame,
                    url,
                    "text/html",
                    None,
                    b"",
                    Some((result, started, redirects, navigation)),
                    loader_id,
                ),
            }
            return true;
        }
        // A script the DOM queued: handed back, its turn a task of its own.
        if let Some(fetching) = self.script_fetches.remove(&request_id) {
            if fetching.module {
                let module = module_loaded(result);
                if module.is_none() {
                    self.note_failed(fetching.frame, failed_url(result, asked));
                }
                if let Some(engine) = &mut self.engine {
                    engine.module_fetched(fetching.key, module);
                }
                // A deferred module script's graph may be in.
                for frame in self.frames() {
                    self.run_deferred_scripts(frame);
                }
                return true;
            }
            let encoding = self.doc(fetching.frame).map(|next| next.encoding);
            let loaded = encoding
                .and_then(|encoding| script_loaded(result, fetching.charset.as_deref(), encoding));
            if loaded.is_none() {
                self.note_failed(fetching.frame, failed_url(result, asked));
            }
            if let Some(engine) = &mut self.engine {
                engine
                    .dom_mut()
                    .mutate(|cx| cx.script_fetched(fetching.key, loaded));
            }
            return true;
        }
        for frame in self.frames() {
            let Some(next) = self.doc_mut(frame) else {
                continue;
            };
            if let Some(sheet) = next.pending_sheets.remove(&request_id) {
                self.update_child_blocking(frame);
                self.sheet_loaded(frame, sheet, result);
                return true;
            }
            let encoding = next.encoding;
            // The parser's: the one it waits for, or a deferred one.
            let (script, blocking) = match &mut next.blocking_script {
                Some(script) if script.request_id == Some(request_id) => (script, true),
                _ => match next
                    .deferred_scripts
                    .iter_mut()
                    .find(|script| script.request_id == Some(request_id))
                {
                    Some(script) => (script, false),
                    None => continue,
                },
            };
            let loaded = script_loaded(result, script.charset.as_deref(), encoding);
            if loaded.is_none() {
                next.failed.push(failed_url(result, asked));
            }
            script.ready = Some(loaded.map(ParserSource::External));
            if blocking {
                self.advance_document(frame);
            } else {
                self.run_deferred_scripts(frame);
            }
            return true;
        }
        false
    }
}

/// DevTools' view of the DOM's loads: each request told to the
/// `Network` agent as it goes, from its frame, with who asked for it, while
/// a session has `Network` on (nothing is looked at otherwise); the
/// subresources each document loaded, for `Page.getResourceTree`, kept
/// whether or not DevTools is there.
impl RenderThread {
    /// Sends `request`, of `frame`'s document (an iframe's document's: of
    /// the frame it navigates), told to DevTools' `Network` agent first, if
    /// a session has it on, as asked for by whom `initiator` finds then.
    /// Its id.
    fn send_request_with_initiator(
        &mut self,
        frame: FrameId,
        request: ResourceRequest,
        initiator: impl FnOnce(&mut Self) -> Option<Initiator>,
    ) -> u64 {
        let request_id = self.next_request_id.next();
        if self.network_agent.enabled() {
            let initiator = initiator(self);
            if let Some(at) = self.request_frame(frame, &request) {
                self.network_agent
                    .will_send(request_id, &request, at, initiator.as_ref());
            }
        }
        self.send_request(request_id, request);
        request_id
    }

    /// Sends `request`, of no frame DevTools knows. Its id.
    fn send_request_untold(&mut self, request: ResourceRequest) -> u64 {
        let request_id = self.next_request_id.next();
        self.send_request(request_id, request);
        request_id
    }

    fn send_request(&self, request_id: u64, request: ResourceRequest) {
        self.next_request_id.sent(request_id, &request);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            request,
        });
    }

    /// Where `request` goes from, as DevTools names it: `frame` and its
    /// document's loader and URL (an iframe's document's: the frame it
    /// navigates, the request its loader). None before DevTools named the
    /// top-level frame, or once the document has gone.
    fn request_frame(&mut self, frame: FrameId, request: &ResourceRequest) -> Option<RequestFrame> {
        let top = self.document.as_ref().map(|next| next.frame);
        let frame_id = if Some(frame) == top {
            let token = &self.devtools_main_frame_token;
            (!token.is_empty()).then(|| token.clone())?
        } else {
            self.frame_id(frame)
        };
        if request.destination == RequestDestination::Iframe {
            return Some(RequestFrame {
                frame_id,
                // The agent's: the request's own.
                loader_id: String::new(),
                document_url: request.url.clone(),
            });
        }
        let loader_id = self.loader_id(frame)?;
        let document_url = self.doc(frame)?.url.to_string();
        Some(RequestFrame {
            frame_id,
            loader_id,
            document_url,
        })
    }

    /// What DevTools calls the loader of `frame`'s document: the top-level
    /// one's navigation; a child frame's, its request, or for one none
    /// brought its document's number (as `child_info` tells it).
    fn loader_id(&mut self, frame: FrameId) -> Option<String> {
        let next = self.doc(frame)?;
        if next.child_of.is_none() {
            return Some(next.navigation_id.to_string());
        }
        if let Some(loader_id) = &next.loader_id {
            return Some(loader_id.clone());
        }
        let document = next.document.clone();
        let engine = self.engine.as_mut()?;
        let number = engine
            .dom_mut()
            .read(|cx| u64::from(cx.node_id(document.get(cx.gc)).get()));
        Some(number.to_string())
    }

    /// Who asked for a load, as the DOM says (`initiator`), as DevTools
    /// names it: for an element, the script that asked (its stack, if a
    /// session wants stacks), else the parser of `frame`'s document that
    /// made it (its start tag's line, once: a later load of it is
    /// script's); a stylesheet, an importing module, `import()`'s script.
    /// None: no one it names.
    fn initiator(&mut self, frame: FrameId, initiator: &dom::LoadInitiator) -> Option<Initiator> {
        use dom::LoadInitiator;
        let stacks = self.network_agent.wants_stacks();
        match initiator {
            LoadInitiator::Other => None,
            LoadInitiator::Stylesheet(url) => Some(Initiator::Css(url.clone())),
            LoadInitiator::Import(url) => Some(Initiator::Import(url.clone())),
            LoadInitiator::Script(stack) => stacks.then(|| Initiator::Script(stack.clone())),
            LoadInitiator::Element(element) => {
                let stack = self.engine.as_mut().and_then(|engine| {
                    engine
                        .dom_mut()
                        .take_request_stack(dom::StackAsker::Element(*element))
                });
                if let Some(stack) = stack.filter(|_| stacks) {
                    return Some(Initiator::Script(stack));
                }
                let next = self.doc_mut(frame)?;
                let line = next.parser_lines.remove(element)?;
                let url = next.url.as_str().split('#').next().unwrap_or_default();
                Some(Initiator::Parser {
                    url: url.to_owned(),
                    line: line.saturating_sub(1),
                })
            }
        }
    }

    /// The lines the elements `frame`'s document's parser made start on
    /// (`lines`), kept for their loads' initiators while a session has
    /// `Network` on (nothing is told of the loads otherwise).
    fn keep_parser_lines(&mut self, frame: FrameId, lines: dom::NodeIdHashMap<u32>) {
        if lines.is_empty() || !self.network_agent.enabled() {
            return;
        }
        if let Some(next) = self.doc_mut(frame) {
            next.parser_lines.extend(lines);
        }
    }

    /// Notes request `request_id`'s `result` in the subresources of the
    /// document whose it is (`url`, the request's, names one that failed):
    /// its sheets, scripts and images, not its `fetch()`es or its child
    /// frames' documents.
    pub(super) fn record_resource(
        &mut self,
        request_id: u64,
        url: Option<&str>,
        result: &LoadResult,
    ) {
        let found = if let Some(&(_, document)) = self.images.get(&request_id) {
            self.frame_of_document(document)
                .map(|frame| (frame, "Image"))
        } else if let Some(fetching) = self.script_fetches.get(&request_id) {
            Some((fetching.frame, "Script"))
        } else {
            self.frames().into_iter().find_map(|frame| {
                let next = self.doc(frame)?;
                if next.pending_sheets.contains_key(&request_id) {
                    return Some((frame, "Stylesheet"));
                }
                next.blocking_script
                    .iter()
                    .chain(&next.deferred_scripts)
                    .any(|script| script.request_id == Some(request_id))
                    .then_some((frame, "Script"))
            })
        };
        let Some((frame, kind)) = found else {
            return;
        };
        if let Some(next) = self.doc_mut(frame) {
            Rc::make_mut(&mut next.resources).add_result(kind, url, result);
        }
    }

    /// The loads asked for this round have started: the stacks of the
    /// script that asked that no load took go (an image the list had, a
    /// `<link>` with nothing to load), so no later load takes them.
    pub(super) fn clear_request_stacks(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.dom_mut().clear_request_stacks();
        }
    }

    /// The engine captures the stacks DevTools' sessions want now: of the
    /// script asking for loads while one asked for them
    /// (`Network.setAttachDebugStack`), of the script making frames while
    /// one has `Page` on.
    pub(super) fn update_stacks_wanted(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.set_stacks_wanted(
                self.network_agent.wants_stacks(),
                self.page_agent.any_enabled(),
            );
        }
    }
}

impl RenderThread {
    /// A `<link>`'s sheet came: to the link (a failed one is a sheet with
    /// no rules, as Chrome's), its `url()`s against where it ended up;
    /// then the link's `load` or `error`, the next frame, and the
    /// document's `load` if it waited for this.
    /// The `<link>` sheets DevTools' `CSS` wants the texts of and hasn't
    /// (loaded before it came), fetched again as their `<link>`s fetched
    /// them; their responses go to it (`css_refetched`).
    pub(super) fn start_css_refetches(&mut self) {
        for (key, href, cors, document) in self.css_agent.take_refetches() {
            let Some(frame) = self.frame_of_document(document) else {
                self.css_agent.refetched(key, None, None);
                continue;
            };
            let Ok(url) = Url::parse(&href) else {
                self.css_agent.refetched(key, None, None);
                continue;
            };
            let mut request = ResourceRequest::new(url.as_str(), RequestDestination::Style);
            request.initiator = self.document_origin(frame);
            if let Some(include) = cors {
                request.mode = RequestMode::Cors;
                request.credentials = if include {
                    CredentialsMode::Include
                } else {
                    CredentialsMode::SameOrigin
                };
            }
            let request_id = self.send_request_with_initiator(frame, request, |_| None);
            self.css_refetches.insert(request_id, (key, frame));
        }
    }

    /// A sheet DevTools' `CSS` fetched again came: decoded as its `<link>`'s
    /// was, handed to the agent.
    pub(super) fn css_refetched(&mut self, key: usize, frame: FrameId, result: &LoadResult) {
        let Some(next) = self.doc(frame) else {
            self.css_agent.refetched(key, None, None);
            return;
        };
        let (document, encoding) = (next.document.clone(), next.encoding);
        let quirks = self.engine.as_mut().is_some_and(|engine| {
            engine
                .dom_mut()
                .read(|cx| cx.quirks_mode(document.get(cx.gc)) == dom::QuirksMode::Quirks)
        });
        let text = stylesheet_text(result, quirks, encoding);
        let base = result
            .as_ref()
            .ok()
            .and_then(|response| Url::parse(&response.url).ok())
            .map(|url| {
                let referrer = url.to_string();
                css::UrlContext::new(url, &referrer)
            });
        self.css_agent.refetched(key, text, base);
    }

    fn sheet_loaded(&mut self, frame: FrameId, sheet: PendingSheet, result: &LoadResult) {
        let Some(next) = self.doc(frame) else {
            return;
        };
        let (document, encoding) = (next.document.clone(), next.encoding);
        // Script may read the rules of one of the document's origin or that
        // CORS allowed; of a network error (a CORS failure among them), if
        // it asked its own origin.
        let origin_clean = match result {
            Ok(response) => response.tainting != ResponseTainting::Opaque,
            Err(_) => sheet.url.scheme() == "data" || sheet.url.origin() == next.url.origin(),
        };
        let Some(engine) = &mut self.engine else {
            return;
        };
        let quirks = engine
            .dom_mut()
            .read(|cx| cx.quirks_mode(document.get(cx.gc)) == dom::QuirksMode::Quirks);
        let css = stylesheet_text(result, quirks, encoding);
        if css.is_none()
            && let Some(next) = self.doc_mut(frame)
        {
            next.failed
                .push(failed_url(result, Some(sheet.url.as_str())));
        }
        let base = result
            .as_ref()
            .ok()
            .and_then(|response| Url::parse(&response.url).ok())
            .map(|url| {
                let referrer = url.to_string();
                css::UrlContext::new(url, &referrer)
            });
        let contents = Arc::new(css::StyleSheetContents::parse(
            css.as_deref().unwrap_or_default(),
            base,
        ));
        // Its text as it came, for DevTools (Chrome's from its cache); kept
        // only while a session is attached.
        if let Some(css) = &css
            && !self.devtools_sessions.is_empty()
        {
            self.css_agent.keep_link_text(&contents, css);
        }
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.dom_mut().mutate(|cx| {
            cx.set_link_sheet(
                sheet.link.get(cx.gc),
                contents,
                sheet.url.as_str(),
                origin_clean,
            );
        });
        let Some(engine) = &mut self.engine else {
            return;
        };
        engine.fire_event(
            &sheet.link,
            if css.is_some() {
                dom::names::event::LOAD
            } else {
                dom::names::event::ERROR
            },
        );
        self.send_harness_results();
        self.request_main_frame();
        // The parser's scripts it blocked may run now.
        if sheet.blocks_scripts {
            self.advance_document(frame);
            self.run_deferred_scripts(frame);
        }
        self.finish_load(frame);
    }
}

/// A classic script's response as HTML's "fetch a classic script" takes
/// it: none for a failed load (a network error, a CORS failure), a status
/// that isn't ok, or a MIME type no script is (`image/`, `audio/`,
/// `video/`, `text/csv`, as Blink's `AllowedByNosniff::MimeTypeAsScript`
/// blocks); else its text decoded (a BOM, the response's charset, the
/// `charset` attribute's label, the document's encoding, in that order),
/// its URL after redirects, and
/// whether its errors are muted (another origin's, `no-cors`: tainted
/// opaque).
fn script_loaded(
    result: &LoadResult,
    charset: Option<&str>,
    document_encoding: &'static encoding_rs::Encoding,
) -> Option<dom::LoadedScript> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    let mime = response.mime_type.as_str();
    if ["image/", "audio/", "video/"]
        .iter()
        .any(|prefix| mime.starts_with(prefix))
        || mime == "text/csv"
    {
        return None;
    }
    let fallback = charset
        .and_then(|label| encoding_rs::Encoding::for_label(label.trim().as_bytes()))
        .unwrap_or(document_encoding);
    Some(dom::LoadedScript {
        source: encoding::decode_script(response.charset.as_deref(), &response.body, fallback),
        url: response.url.clone(),
        muted: response.tainting == ResponseTainting::Opaque,
    })
}

/// What the DOM's engine sends as script asks (`dom_api::
/// LoadSink`): a `fetch()` at the call, a blob URL registered and revoked
/// at once (Blink's `FetchManager` starts its loader in the call), on the
/// handles of the renderer's request ids and blob URLs it shares with the
/// render thread (which hears of each `fetch()` after,
/// `FrameRequest::FetchSent`).
pub(super) struct RendererLoadSink {
    pub(super) to_browser: crossbeam_channel::Sender<RendererHostMsg>,
    pub(super) ids: crate::loader::RequestIds,
    pub(super) blob_urls: crate::loader::BlobUrls,
}

impl dom_api::LoadSink for RendererLoadSink {
    fn start_fetch(&self, _: FrameId, request: &dom::FetchRequest) -> Option<u64> {
        if fetch_of_revoked_blob(&self.blob_urls, request) {
            return None;
        }
        let resource = fetch_resource_request(request);
        let request_id = self.ids.next();
        self.ids.sent(request_id, &resource);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            request: resource,
        });
        Some(request_id)
    }

    fn register_blob_url(&self, frame: FrameId, url: &str, data: &dom::BlobData) {
        self.blob_urls
            .register(url, frame, data.type_(), data.shared_bytes());
    }

    fn revoke_blob_url(&self, url: &str) {
        self.blob_urls.revoke(url);
    }

    fn frame_gone(&self, frame: FrameId) {
        self.blob_urls.revoke_frame(frame);
    }
}

/// Whether `request` is of a blob URL revoked before the element or the
/// call resolved it (in its task, before the browser heard): a network
/// error, without asking.
fn fetch_of_revoked_blob(blob_urls: &crate::loader::BlobUrls, request: &dom::FetchRequest) -> bool {
    Url::parse(&request.url).is_ok_and(|url| url.scheme() == "blob" && blob_urls.is_revoked(&url))
}

/// The network service's request for a `fetch()`'s.
fn fetch_resource_request(request: &dom::FetchRequest) -> ResourceRequest {
    let mut resource = ResourceRequest::new(&request.url, RequestDestination::Empty);
    resource.method.clone_from(&request.method);
    resource.headers.clone_from(&request.headers);
    resource.initiator = Some(request.initiator.clone());
    resource.body = request.body.as_ref().map(|body| body.to_vec());
    resource.redirect = match request.redirect {
        dom::RequestRedirect::Follow => protocol::network::RedirectMode::Follow,
        dom::RequestRedirect::Error => protocol::network::RedirectMode::Error,
        dom::RequestRedirect::Manual => protocol::network::RedirectMode::Manual,
    };
    resource.mode = match request.mode {
        dom::RequestMode::SameOrigin => RequestMode::SameOrigin,
        dom::RequestMode::NoCors => RequestMode::NoCors,
        // A `Request` script makes is never `navigate`.
        dom::RequestMode::Cors | dom::RequestMode::Navigate => RequestMode::Cors,
    };
    resource.credentials = match request.credentials {
        dom::RequestCredentials::Omit => CredentialsMode::Omit,
        dom::RequestCredentials::SameOrigin => CredentialsMode::SameOrigin,
        dom::RequestCredentials::Include => CredentialsMode::Include,
    };
    resource
}

/// What a subresource that failed is named by, in `DidFinishLoad`: where
/// its response came from (after redirects), or the URL asked for (`asked`)
/// if none came.
fn failed_url(result: &LoadResult, asked: Option<&str>) -> String {
    match result {
        Ok(response) => response.url.clone(),
        Err(error) => asked.map_or_else(|| error.detail.clone(), str::to_owned),
    }
}

/// A module's response as HTML's "fetch a single module script" takes it:
/// none for a failed load (a network error, a CORS failure), a status
/// that isn't ok, or a MIME type that isn't JavaScript's; else its text,
/// UTF-8, and its URL after redirects (its base URL).
fn module_loaded(result: &LoadResult) -> Option<dom_api::module::FetchedModule> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status)
        || !dom_api::is_javascript_mime_type(&response.mime_type)
    {
        return None;
    }
    let (source, _) = encoding_rs::UTF_8.decode_with_bom_removal(&response.body);
    Some(dom_api::module::FetchedModule {
        url: Url::parse(&response.url).ok()?,
        source: source.into_owned(),
    })
}

/// A sheet's text, decoded: none for a failed load, an error status, or
/// (but in quirks mode) a type that isn't `text/css`.
fn stylesheet_text(
    result: &LoadResult,
    quirks: bool,
    document_encoding: &'static encoding_rs::Encoding,
) -> Option<String> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.status) {
        return None;
    }
    if response.mime_type != "text/css" && !quirks {
        return None;
    }
    Some(encoding::decode_stylesheet(
        response.charset.as_deref(),
        &response.body,
        document_encoding,
    ))
}

/// A `<link>` of a sheet to load: its element, its `href`, its CORS
/// credentials mode (none: no CORS), whether it blocks nothing (an
/// alternate one, a shadow tree's), whether the parser made it, and its
/// `media`.
struct LinkToLoad {
    link: NodeRoot,
    id: dom::NodeId,
    href: String,
    cors: Option<CredentialsMode>,
    blocks_nothing: bool,
    parsers: bool,
    /// Its `media`, if it has one.
    media: Option<String>,
}

/// The node after `node` in tree order, within `root`'s subtree.
fn next_in_tree_order<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    node: dom::NodeRef<'gc>,
    root: dom::NodeRef<'gc>,
) -> Option<dom::NodeRef<'gc>> {
    if let Some(child) = cx.first_child(node) {
        return Some(child);
    }
    let mut at = node;
    loop {
        if dom::NodeRef::ptr_eq(at, root) {
            return None;
        }
        if let Some(next) = cx.next_sibling(at) {
            return Some(next);
        }
        at = cx.parent(at)?;
    }
}

/// The DOM's name of a button.
/// The pointer the cursor `keyword` asks for (`auto` resolved already by
/// `dom_api`'s `select_cursor`; were it not, the arrow).
fn cursor_type(keyword: computed_style::CursorKeyword) -> CursorType {
    use computed_style::CursorKeyword as K;
    match keyword {
        K::Auto | K::Default => CursorType::Pointer,
        K::None => CursorType::None,
        K::ContextMenu => CursorType::ContextMenu,
        K::Help => CursorType::Help,
        K::Pointer => CursorType::Hand,
        K::Progress => CursorType::Progress,
        K::Wait => CursorType::Wait,
        K::Cell => CursorType::Cell,
        K::Crosshair => CursorType::Cross,
        K::Text => CursorType::IBeam,
        K::VerticalText => CursorType::VerticalText,
        K::Alias => CursorType::Alias,
        K::Copy => CursorType::Copy,
        K::Move | K::AllScroll => CursorType::Move,
        K::NoDrop => CursorType::NoDrop,
        K::NotAllowed => CursorType::NotAllowed,
        K::Grab => CursorType::Grab,
        K::Grabbing => CursorType::Grabbing,
        K::EResize => CursorType::EastResize,
        K::NResize => CursorType::NorthResize,
        K::NeResize => CursorType::NorthEastResize,
        K::NwResize => CursorType::NorthWestResize,
        K::SResize => CursorType::SouthResize,
        K::SeResize => CursorType::SouthEastResize,
        K::SwResize => CursorType::SouthWestResize,
        K::WResize => CursorType::WestResize,
        K::EwResize => CursorType::EastWestResize,
        K::NsResize => CursorType::NorthSouthResize,
        K::NeswResize => CursorType::NorthEastSouthWestResize,
        K::NwseResize => CursorType::NorthWestSouthEastResize,
        K::ColResize => CursorType::ColumnResize,
        K::RowResize => CursorType::RowResize,
        K::ZoomIn => CursorType::ZoomIn,
        K::ZoomOut => CursorType::ZoomOut,
    }
}

fn dom_button(button: MouseButton) -> dom_api::MouseButton {
    match button {
        MouseButton::Left => dom_api::MouseButton::Primary,
        MouseButton::Middle => dom_api::MouseButton::Auxiliary,
        MouseButton::Right => dom_api::MouseButton::Secondary,
    }
}

/// The modifier keys held, as the DOM's bits.
fn dom_modifiers(modifiers: Modifiers) -> u16 {
    [
        (modifiers.ctrl, dom::modifiers::CTRL),
        (modifiers.shift, dom::modifiers::SHIFT),
        (modifiers.alt, dom::modifiers::ALT),
        (modifiers.meta, dom::modifiers::META),
    ]
    .into_iter()
    .filter(|&(held, _)| held)
    .fold(0, |bits, (_, bit)| bits | bit)
}

/// The DOM's run of the update intersection observations steps, after
/// the frame's lifecycle (Blink's `IntersectionObserverController::
/// ComputeIntersections`): each observation of the observers of the
/// windows of the frame tree's `documents` (the rendering update's, in tree
/// order: `ScriptEngine::documents_in_tree_order`; but those in `blocked`,
/// whose rendering is blocked) computed (`paint::intersections`,
/// in the top-level frame's viewport) unless its observer's `delay` holds
/// it back, the results handed to `dom_api::update_intersections` by
/// window.
/// Nothing is done again while the frame's pre-paint, the observations
/// and (for one that tracks visibility) the paint are what they were last
/// time (`computed`). Whether the task is to be queued, and in how many
/// milliseconds the first held back is due.
fn compute_intersections(
    dom: &mut Dom,
    frame: &mut Frame,
    documents: &[gfx::NodeId],
    blocked: &[NodeRoot],
    computed: &mut Option<(u64, u64, Option<paint::PaintId>)>,
) -> (bool, Option<f64>) {
    let view = &frame.view;
    let document = view.document().clone();
    dom.mutate(|cx| {
        let top = document.get(cx.gc);
        if cx.window_of(top).is_none() {
            return (false, None);
        }
        let tracked = computed.is_some_and(|(.., paint)| paint.is_some());
        let state = (
            view.pre_paints(),
            cx.intersection_version(),
            view.last_paint().filter(|_| tracked),
        );
        if *computed == Some(state) {
            return (false, None);
        }
        // Each document of the frame tree in tree order, but those whose
        // rendering is blocked, with its window and its now.
        let blocked: Vec<gfx::NodeId> = blocked
            .iter()
            .map(|document| cx.node_id(document.get(cx.gc)))
            .collect();
        let documents: Vec<_> = documents
            .iter()
            .filter_map(|&document| cx.node_by_id(document))
            .filter(|document| !blocked.contains(&cx.node_id(*document)))
            .filter_map(|document| {
                let window = cx.window_of(document)?;
                Some((document, window, cx.now_since(Some(window))))
            })
            .collect();
        let shown: Vec<gfx::NodeId> = documents
            .iter()
            .map(|&(document, ..)| cx.node_id(document))
            .collect();
        let mut tracking = false;
        let mut due: Option<f64> = None;
        // By its window's index in `documents`.
        let mut observations = Vec::new();
        for (index, &(_, window, now)) in documents.iter().enumerate() {
            for observer in cx.window_intersection_observers(window) {
                let root = match cx.intersection_observer_root(observer) {
                    dom::IntersectionObserverRoot::Implicit => paint::IntersectionRoot::Implicit,
                    dom::IntersectionObserverRoot::Node(node)
                        if cx.kind(node) == dom::NodeKind::Document =>
                    {
                        paint::IntersectionRoot::Document(cx.node_id(node))
                    }
                    dom::IntersectionObserverRoot::Node(node) => {
                        paint::IntersectionRoot::Element(cx.node_id(node))
                    }
                    dom::IntersectionObserverRoot::Gone => continue,
                };
                let options = cx.intersection_observer_options(observer);
                tracking |= options.track_visibility;
                #[expect(
                    clippy::cast_precision_loss,
                    reason = "milliseconds on the window's timeline, a double as Blink's"
                )]
                let delay = options.delay_micros as f64 / 1000.0;
                for observation in cx.intersection_observations(observer) {
                    if let Some(last) = observation.last_run
                        && now < last + delay
                    {
                        let wait = last + delay - now;
                        due = Some(due.map_or(wait, |due: f64| due.min(wait)));
                        continue;
                    }
                    // With the implicit root, one in a document no frame
                    // shows waits until it is in one (Chrome gives no entry
                    // then).
                    if matches!(root, paint::IntersectionRoot::Implicit)
                        && !shown.contains(&cx.node_id(cx.document_of(observation.target)))
                    {
                        continue;
                    }
                    // The top-level viewport's rect and margin aren't given
                    // to a target of another origin (Intersection Observer's
                    // `rootBounds` null, the margin not applied).
                    let other = matches!(root, paint::IntersectionRoot::Implicit)
                        && !dom_api::same_origin(cx, cx.document_of(observation.target), top);
                    observations.push((index, observer, observation.target, root, options, other));
                }
            }
        }
        *computed = due
            .is_none()
            .then(|| (state.0, state.1, view.last_paint().filter(|_| tracking)));
        if observations.is_empty() {
            return (false, due);
        }
        for &(index, observer, target, ..) in &observations {
            cx.set_intersection_observation_run(observer, target, documents[index].2);
        }
        let requests: Vec<paint::IntersectionRequest> = observations
            .iter()
            .map(
                |&(_, _, target, root, options, other)| paint::IntersectionRequest {
                    target: cx.node_id(target),
                    root,
                    root_margin: (!other).then_some(options.root_margin),
                    scroll_margin: options.scroll_margin,
                    visibility: options.track_visibility,
                },
            )
            .collect();
        // In the flat tree: a shadow root's host is around it (Blink's hit
        // test stops at a target, what is in it is the target); and through
        // the frames: a child frame's document is in its iframe.
        let contains = |target: gfx::NodeId, node: gfx::NodeId| {
            let mut at = cx.node_by_id(node);
            while let Some(n) = at {
                if cx.node_id(n) == target {
                    return true;
                }
                at = cx
                    .parent(n)
                    .or_else(|| cx.shadow_host(n))
                    .or_else(|| cx.frame_owner(n));
            }
            false
        };
        let geometry = view.intersections(cx.node_id(top), &requests, &contains);
        let mut by_window: Vec<Vec<dom_api::ComputedIntersection<'_>>> =
            documents.iter().map(|_| Vec::new()).collect();
        for (&(index, observer, target, .., other), geometry) in observations.iter().zip(geometry) {
            by_window[index].push(dom_api::ComputedIntersection {
                observer,
                target,
                target_rect: geometry.target,
                intersection_rect: geometry.intersection,
                // A target with no rect has a zero root rect too, as in
                // Chrome (WPT's `display-none.html`).
                root_bounds: match (other, geometry.target) {
                    (true, _) => None,
                    (false, None) => Some(gfx::RectF::default()),
                    (false, Some(_)) => Some(geometry.root.unwrap_or_default()),
                },
                is_visible: geometry.visible,
            });
        }
        let mut queue = false;
        for ((_, window, now), results) in documents.iter().zip(&by_window) {
            if !results.is_empty() {
                queue |=
                    dom_api::update_intersections(&dom_api::ApiCx::new(cx), *window, results, *now);
            }
        }
        (queue, due)
    })
}
