//! The DOM's side of the renderer: a navigation commits a document of
//! the DOM (`FrameDocument`) in a realm of the DOM's engine
//! (`dom_api::ScriptEngine`), parsed by the parser that pauses for
//! scripts (`html::parser::Parser`), its parser-blocking and deferred
//! classic scripts run in order (an external one fetched first, none while
//! a stylesheet blocks scripts), the DOM's other external scripts fetched
//! as the engine sends them (`RendererLoadSink::start_script_fetch`),
//! shown by its own frame view (`FrameView`: style, layout and paint of
//! the DOM), told to DevTools' `Page` agent as the main frame; its
//! iframes' child frames load and show their documents alike
//! (`render_frame`'s `RenderFrames`, a table of the frames).

use std::rc::Rc;
use std::sync::Arc;

use compositor::DocumentId;
use dom::{Dom, NodeRoot};
use dom_api::FrameLayout as _;
use dom_api::document_loader::ParseOutcome;
use dom_api::encoding;
use dom_api::{
    ChildNavigation, FrameId, FrameRequest, KeyInput, KeyResult, MouseAction, MouseInput,
    ScriptEngine, WheelInput,
};
use gfx::PointF;
use protocol::control_channel::BrowserSender;
use protocol::input::{InputEvent, Modifiers, MouseButton};
use protocol::network::{
    CredentialsMode, LoadResult, RequestDestination, RequestMode, ResourceRequest, ResponseTainting,
};
use protocol::renderer::{
    CursorType, MenuItem, MenuItemKind, PopupMenu, PopupMenuResult, RendererHostMsg,
};
use url::Url;

use super::document_body::{BodyRead, DocumentBody, NavigationBody};
use super::network_agent::RequestFrame;
use super::page_agent::{FrameInfo, FrameResources};
use super::{RenderThread, SwapPromise};
use crate::document;
use crate::frame_view::{Frame, FrameView};
use crate::initiator::Initiator;
use crate::pending_user_input::PendingUserInputMonitor;
use crate::svg_image::SvgImage;
use crate::task_queue::{InternalTask, TaskQueues};

/// A committed document of the DOM: its navigation (its URL is the
/// DOM's, `document_url`), its parser until the input is all parsed, its parser-blocking script and
/// deferred scripts. A child frame's parent and iframe are the engine's
/// (`RenderThread::child_parent`).
pub(super) struct FrameDocument {
    /// The browsing context it is the document of.
    frame: FrameId,
    navigation_id: u64,
    /// Its MIME type and markup as they came, for DevTools (the markup
    /// once its body has all come: the engine's loading of it has it until
    /// then, `document_source`).
    mime_type: String,
    source: Rc<str>,
    /// Its body, until it has all come and its load has ended: what of it
    /// comes is handed to the engine's loading of the document, whose
    /// parser waits for the rest.
    body: Option<DocumentBody>,
    /// Its window's time origin, on the renderer's clock.
    time_origin: f64,
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
    /// `DidFinishLoad` went with a frame: its load ended (the DOM's,
    /// `LoadProgress::Loaded`), told once.
    load_told: bool,
    /// `DidFinishLoad` said the root element had `reftest-wait`:
    /// `ReftestWaitEnded` goes with the frame that shows it gone.
    waiting_for_reftest: bool,
    /// What the document's text was decoded as: its sheets' default.
    encoding: &'static encoding_rs::Encoding,
    /// When its network goes quiet, for DevTools (Blink's
    /// `IdlenessDetector`, one a frame), from `DOMContentLoaded` on.
    idleness: crate::idleness_detector::IdlenessDetector,
    /// The subresources it loaded, for DevTools' `Page.getResourceTree`
    /// (shared with what the `Page` agent was told: copied as it changes
    /// after).
    resources: Rc<FrameResources>,
    /// A child frame's initial `about:blank`, which no navigation brought.
    initial: bool,
}

impl FrameDocument {
    /// `frame`'s new document, loading: nothing asked for, run or rendered
    /// yet.
    fn new(
        frame: FrameId,
        mime_type: &str,
        source: Rc<str>,
        document: NodeRoot,
        encoding: &'static encoding_rs::Encoding,
    ) -> Self {
        Self {
            frame,
            navigation_id: 0,
            mime_type: mime_type.to_owned(),
            source,
            body: None,
            time_origin: 0.0,
            document,
            rendered_version: 0,
            rendered_intersections: 0,
            rendered_resizes: 0,
            rendered_canvas_draws: 0,
            load_told: false,
            waiting_for_reftest: false,
            encoding,
            idleness: crate::idleness_detector::IdlenessDetector::default(),
            resources: Rc::default(),
            initial: false,
        }
    }

    pub(super) fn navigation_id(&self) -> u64 {
        self.navigation_id
    }

    /// `frame`'s document `document`, of no parser: the frames' table's
    /// tests'.
    #[cfg(test)]
    pub(super) fn for_test(frame: FrameId, document: NodeRoot) -> Self {
        Self::new(
            frame,
            "text/html",
            Rc::from(""),
            document,
            encoding_rs::UTF_8,
        )
    }

    /// The DOM's document it is of.
    #[cfg(test)]
    pub(super) fn node(&self) -> &NodeRoot {
        &self.document
    }
}

use protocol::renderer::RENDERER_COMMIT;

/// A child frame's document as the browser sent it: its URL (an error
/// page's own), its type and its body.
pub(super) struct ChildDocument<'a> {
    pub(super) url: Url,
    pub(super) mime_type: &'a str,
    pub(super) charset: Option<&'a str>,
    pub(super) body: NavigationBody,
}

/// What a child frame's document the browser loaded commits with: its
/// time origin (when its navigation started) and navigation timing, its
/// response's `Content-Language`, its loader (the navigation, for
/// DevTools), and what the browser hears it committed
/// (`DidCommitNavigation`: its token, the navigation and its URL).
pub(super) struct ChildCommit<'a> {
    pub(super) time_origin: f64,
    pub(super) timing: crate::navigation_timing::NavigationTiming,
    pub(super) content_language: Option<&'a str>,
    pub(super) navigation_id: u64,
    pub(super) token: protocol::FrameToken,
    pub(super) url: String,
    /// The document it has its origin from, if not its URL or its parent.
    pub(super) origin_source: Option<NodeRoot>,
    /// Its item's state, and where its entry is in the session history.
    pub(super) history: protocol::renderer::HistoryEntryParams,
    /// The loader the browser gave it.
    pub(super) loader: u64,
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
    token: Option<protocol::FrameToken>,
    navigation_id: u64,
    url: &Url,
    mime_type: &str,
    source: &Rc<str>,
    resources: &Rc<FrameResources>,
) -> Option<FrameInfo> {
    let token = token?;
    // As Blink's `SecurityOrigin::ToRawString`: an opaque one's scheme
    // and host are empty.
    let origin = match url.origin() {
        origin @ url::Origin::Tuple(..) => origin.ascii_serialization(),
        url::Origin::Opaque(_) => "://".to_owned(),
    };
    Some(FrameInfo {
        id: token.to_string(),
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
    })
}

/// The frame's lifecycle brought up to date and its paint handed to its
/// widget's layers, DevTools' overlay over them (Blink's
/// `WebFrameWidgetImpl::UpdateLifecycle` and `LayerTreeHost::DoUpdateLayers`):
/// what the event loop's rendering update and script paused's draw alike,
/// before they commit.
/// `frames`: the frames as they are now, for DevTools' `CSS` to list the
/// sheets of (none: the `DOM` agent's, as last told: while script is
/// paused, when the engine's frames can't be asked).
pub(super) fn update_lifecycle_and_layers(
    dom: &mut Dom,
    frame: &mut Frame,
    overlay_agent: &super::overlay_agent::OverlayAgent,
    css_agent: &super::css_agent::CssAgent,
    frames: Option<&[super::dom_agent::DomFrame]>,
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
    css_agent.sheets_updated(dom, frame, frames);
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
    // The compositor's position, from the document's start: the offset and
    // the scroll origin (cc's scroll position).
    let (offset, origin) = (view.viewport_scroll_offset(), view.viewport_scroll_origin());
    host.set_root_scroll_offset(gfx::PointF::new(offset.x + origin.x, offset.y + origin.y));
    host.set_root_scroll_origin(origin);
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
        reported_url: String,
        origin_source: Option<NodeRoot>,
        mime_type: &str,
        charset: Option<&str>,
        content_language: Option<&str>,
        body: NavigationBody,
        history: &protocol::renderer::HistoryEntryParams,
        time_origin: f64,
        timing: &crate::navigation_timing::NavigationTiming,
        loader: u64,
        bypass_cache: bool,
    ) {
        let _span = trace_event::info_event!("commit_navigation", %url);
        let document_id = crate::frame_view::next_document_id();
        // The engine the document's scripts are of, known before it is
        // parsed: a page whose engine is the other kind takes a new one.
        let wasm = self.script_engine.wasm_engine.is_some()
            && mime_type == "text/html"
            && encoding::declares_wasm_scripts(&body.start);
        let replacing = self.engine.is_some() && self.engine_is_wasm != wasm;
        // The initiator's document is the engine before's, which goes with
        // its DOM (what the renderer holds of it let go first): no other
        // document is of its origin then.
        let origin_source = origin_source.filter(|_| !replacing);
        let replaced = replacing.then(|| {
            let previous = self.top_document_url();
            (previous, self.replace_engine(wasm))
        });
        // The URL of the document it shows now, before the engine is
        // borrowed (by the engine before, if the page took a new one).
        let kept_url = replaced
            .is_none()
            .then(|| self.top_document_url())
            .flatten();
        self.engine_is_wasm = wasm;
        if self.engine.is_none() {
            self.engine = Some(self.make_engine());
        }
        let engine = self.engine.as_mut().expect("just made");
        // The system's focus may have come before the first document (and
        // its engine): the window starts with it.
        engine.set_page_focused(self.page_focused);
        // And the page's visibility (a background tab opens hidden).
        engine.set_page_visible(!self.page_hidden);
        let more = body.pipe.is_some();
        let (body, start) = DocumentBody::new(body);
        let source: Rc<str> = Rc::from("");
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
                self.render_frames.top_token(),
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
        // (By the engine before, if the page took a new one.)
        let (previous, unloaded) = match replaced {
            Some(replaced) => replaced,
            None => (kept_url, engine.unload_document(top)),
        };
        // Its documents' `fetch()`es go with them, those of their `unload`
        // too (by the engine before, if the page took a new one).
        cancel_sent_fetches(engine.dom_mut(), &self.to_browser, &self.network_agent);
        let clock = self.clock;
        let unloaded = unloaded.map(|(start, end)| (clock.at(start), clock.at(end)));
        let timing = crate::navigation_timing::with_unload(
            timing.clone(),
            unloaded,
            previous.as_ref(),
            &document_url,
        );
        let document = engine
            .create_window(top, document_url.as_str(), origin)
            .expect("the top-level frame is the engine's");
        // Its loading: its body decoded as its start says, and parsed.
        let loading = dom_api::DocumentLoader::new(
            engine.dom_mut(),
            &document,
            false,
            mime_type,
            charset,
            &start,
            more,
        );
        // A wasm document's scripts are wasm (its top-level frame's: it
        // has no other).
        let loading = if wasm {
            loading.with_wasm_scripts()
        } else {
            loading
        };
        let document_encoding = loading.encoding();
        engine.set_document_loader(top, loading);
        if let Some(source) = &origin_source {
            engine.inherit_origin(top, &document, source);
        }
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
        engine.set_session_history(self.own_history_entries.place(
            history.index,
            history.length,
            history.entries_heard,
        ));
        engine.restore_history(
            top,
            history.state.as_deref().map(<[u8]>::to_vec),
            history.manual_scroll_restoration,
        );
        let node = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
        // Its subresources, its scripts' as it parses too, go through the
        // browser's loader for it.
        engine.dom_mut().mutate(|cx| {
            cx.set_document_loader(
                document.get(cx.gc),
                dom::DocumentLoader {
                    loader,
                    navigation: Some(navigation_id),
                },
            );
            if bypass_cache {
                cx.set_reloaded_past_cache(document.get(cx.gc));
            }
        });
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
        // The child frames of the document before go with it
        // (`commit_top`).
        // DevTools' agents look at the frames again after the task (the
        // `DOM` agent's frames have the new document).
        self.frames_changed = true;
        self.render_frames.commit_top(
            top,
            FrameDocument {
                navigation_id,
                body: Some(body),
                time_origin,
                ..FrameDocument::new(top, mime_type, source, document, document_encoding)
            },
        );
        self.request_main_frame();
        if let Some(frame) = self.render_frames.top_token() {
            let _ = self.to_browser.send(RendererHostMsg::DidCommitNavigation {
                frame,
                navigation_id,
                url: reported_url,
            });
        }
        // The pointer may have been over a link of the old document: the
        // arrow until it moves, or the new one is laid out under it.
        self.set_cursor(CursorType::Pointer);
        self.start_new_content_rendering_timeout();
        self.start_body(top, start);
    }

    /// The page's engine replaced by one of the other kind (`wasm`: the
    /// wasm one), for a document whose scripts are of it: the document it
    /// shows unloaded (its `unload` timed), everything the renderer holds
    /// of its DOM let go before its heap, and the frame's text and widget
    /// taken back for the next engine's frame.
    fn replace_engine(&mut self, wasm: bool) -> Option<(std::time::Instant, std::time::Instant)> {
        let mut engine = self.engine.take()?;
        let top = engine.top_frame();
        let unloaded = engine.unload_document(top);
        self.dom_agent.dom_gone();
        self.css_agent.dom_gone();
        self.overlay_agent.dom_gone();
        self.render_frames.clear();
        // What was asked for its documents: a response still to come is
        // no one's (the next DOM's nodes are numbered from the start).
        // Its `fetch()`es', images' and scripts' requests go with its DOM.
        self.css_refetches.clear();
        // Its documents' workers with them.
        self.workers.terminate_running();
        self.popup = None;
        self.popup_closed_by_press = None;
        self.under_mouse_pre_paints = None;
        self.intersections_computed = None;
        if let Some(layout) = engine.take_layout() {
            let layout: Box<dyn std::any::Any> = layout;
            if let Ok(frame) = layout.downcast::<Frame>() {
                let (text, widget) = frame.into_thread_parts();
                self.text = text;
                self.widget = Some(widget);
            }
        }
        cancel_sent_fetches(engine.dom_mut(), &self.to_browser, &self.network_agent);
        drop(engine);
        // Its `localStorage` areas went with its DOM: the next engine's
        // documents get them again.
        let _ = self.to_browser.send(RendererHostMsg::ForgetStorageAreas);
        self.engine_is_wasm = wasm;
        self.engine = Some(self.make_engine());
        unloaded
    }

    /// What the renderer keeps of `frame`'s document (the top-level one's
    /// or a child's), if the engine has the frame (`has_frame_document`).
    fn doc_mut(&mut self, frame: FrameId) -> Option<&mut FrameDocument> {
        if !self.has_frame_document(frame) {
            return None;
        }
        self.render_frames.document_mut(frame)
    }

    /// The loader `frame`'s document's subresources go through: the one
    /// the browser gave it, or for a document it didn't commit (a child
    /// frame's first `about:blank`, a `javascript:` URL's) the one of the
    /// frame it is in. 0, which the browser knows as none, if it has none.
    fn loader_of(&mut self, frame: FrameId) -> u64 {
        let mut at = Some(frame);
        while let Some(frame) = at {
            if let Some(loader) = self
                .document_of_frame(frame)
                .and_then(|id| self.loader_in_dom(id))
            {
                return loader;
            }
            at = self.child_parent(frame).map(|(parent, _)| parent);
        }
        0
    }

    /// What the renderer had of the frames the engine let go since it
    /// last looked (`ScriptEngine::take_frames_changed`), after a task:
    /// gone (their documents' roots and bodies with them), the `fetch()`es
    /// of documents discarded with them cancelled, DevTools' frames
    /// compared again. The engine's frames are the one list: what reads
    /// the renderer's of a frame asks the engine whether it has it first
    /// (`doc`), so this only lets go of what nothing reads (Chrome's
    /// `RenderFrameImpl` goes in the engine's call; the renderer's state
    /// isn't the engine's to reach).
    fn forget_gone_frames(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        if !engine.take_frames_changed() {
            return;
        }
        self.frames_changed = true;
        let known: Vec<FrameId> = self.render_frames.children().collect();
        let gone: Vec<FrameId> = known
            .into_iter()
            .filter(|frame| self.child_parent(*frame).is_none())
            .collect();
        self.render_frames.remove(&gone);
        // Their iframes, which changed the DOM, asked for the frame that
        // shows them made or gone.
        self.cancel_fetches_of_gone_documents();
    }

    /// Whether the renderer has what it keeps of `frame`'s document, as
    /// the engine has the frame: the top-level frame's from its commit; a
    /// child frame's from its commit, or its initial `about:blank`'s
    /// (Blink's has its loader too: what script inserts into it loads, its
    /// subresources are its own; it is complete already), made now if not
    /// yet. None for a frame the engine no longer has.
    fn has_frame_document(&mut self, frame: FrameId) -> bool {
        if self.render_frames.top() == Some(frame) {
            return self.render_frames.contains(frame);
        }
        let Some((_, owner)) = self.child_parent(frame) else {
            return false;
        };
        if self.render_frames.contains(frame) {
            return true;
        }
        let Some((dom, view)) = engine_frame(&mut self.engine) else {
            return false;
        };
        let document = dom.read(|cx| {
            cx.content_document(owner.get(cx.gc))
                .map(|document| cx.root(document))
        });
        let Some(document) = document else {
            return false;
        };
        // Shown already as its iframe was inserted (the engine has the
        // frame's layout from the first document on, and a paused page's
        // DevTools puts it back before script runs).
        debug_assert!(
            view.view.child_mut(frame).is_some(),
            "child frame {frame:?} not shown as it was made"
        );
        let mut initial = FrameDocument::new(
            frame,
            "text/html",
            Rc::from(""),
            document,
            encoding_rs::UTF_8,
        );
        initial.initial = true;
        self.render_frames.set_child_document(frame, initial);
        true
    }

    /// The frames the engine has, in tree order (the top-level one first),
    /// with what the renderer keeps of each.
    fn frames_with_documents(&mut self) -> Vec<FrameId> {
        let mut frames = self
            .engine
            .as_mut()
            .map(|engine| engine.frames_in_tree_order())
            .unwrap_or_default();
        frames.retain(|frame| self.has_frame_document(*frame));
        frames
    }

    /// `frame`'s document's URL, the DOM's (what its `location` reads; the
    /// renderer keeps no copy of it): `about:blank` for one with none.
    pub(super) fn document_url(&mut self, frame: FrameId) -> Option<Url> {
        let document = self.doc(frame)?.document.clone();
        let engine = self.engine.as_mut()?;
        let url = engine.dom_mut().read(|cx| {
            cx.document_url(document.get(cx.gc))
                .map(|url| url.to_string())
        });
        Some(
            url.and_then(|url| Url::parse(&url).ok())
                .unwrap_or_else(|| Url::parse("about:blank").expect("about:blank")),
        )
    }

    /// The top-level frame's document's URL (`document_url`).
    fn top_document_url(&mut self) -> Option<Url> {
        let top = self.render_frames.top()?;
        self.document_url(top)
    }

    /// Child frame `frame`'s token, the engine's (its frames are the one
    /// list of them).
    pub(super) fn child_token(&mut self, frame: FrameId) -> Option<protocol::FrameToken> {
        self.engine.as_mut()?.frame_token(frame)
    }

    /// The child frame whose token is `token`, if the engine has it.
    pub(super) fn child_of_token(&mut self, token: protocol::FrameToken) -> Option<FrameId> {
        self.engine.as_mut()?.frame_of_token(token)
    }

    /// Child frame `frame`'s parent and iframe, the engine's: none for the
    /// top-level frame, or a frame gone (as its iframe went).
    pub(super) fn child_parent(&mut self, frame: FrameId) -> Option<(FrameId, NodeRoot)> {
        self.engine.as_mut()?.frame_parent(frame)
    }

    /// The frames below `frame`, each with its parent and iframe, as the
    /// engine has them: parents before their children.
    pub(super) fn child_frames_below(
        &mut self,
        frame: FrameId,
    ) -> Vec<(FrameId, FrameId, NodeRoot)> {
        let Some(engine) = self.engine.as_mut() else {
            return Vec::new();
        };
        let mut below = Vec::new();
        let mut parents = std::collections::VecDeque::from([frame]);
        while let Some(parent) = parents.pop_front() {
            for child in engine.child_frames(parent) {
                if let Some((_, owner)) = engine.frame_parent(child) {
                    below.push((child, parent, owner));
                    parents.push_back(child);
                }
            }
        }
        below
    }

    /// The loader the DOM has for the document `document`
    /// (`Cx::document_loader`).
    /// How the document `frame` shows has its subresources use the HTTP
    /// cache (`Cx::subresource_cache_mode`); as usual for a frame that shows
    /// none of the browser's.
    fn subresource_cache_mode(&mut self, frame: FrameId) -> dom::SubresourceCacheMode {
        let Some(document) = self.document_of_frame(frame) else {
            return dom::SubresourceCacheMode::Default;
        };
        let Some(engine) = self.engine.as_mut() else {
            return dom::SubresourceCacheMode::Default;
        };
        engine.dom_mut().read(|cx| {
            cx.node_by_id(document)
                .map(|document| cx.subresource_cache_mode(document))
                .unwrap_or_default()
        })
    }

    fn loader_in_dom(&mut self, document: dom::NodeId) -> Option<u64> {
        let engine = self.engine.as_mut()?;
        engine.dom_mut().read(|cx| {
            cx.document_loader(cx.node_by_id(document)?)
                .map(|loader| loader.loader)
        })
    }

    /// The DOM's number for the document the browser last committed in
    /// `frame`; none for a frame that shows none of the browser's (its
    /// first `about:blank`).
    fn document_of_frame(&mut self, frame: FrameId) -> Option<dom::NodeId> {
        let document = self.doc(frame)?.document.clone();
        let engine = self.engine.as_mut()?;
        Some(engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc))))
    }

    fn doc(&mut self, frame: FrameId) -> Option<&FrameDocument> {
        if !self.has_frame_document(frame) {
            return None;
        }
        self.render_frames.document(frame)
    }

    /// `frame`'s document's markup as it came: so far while its body comes
    /// (the engine's loading of it has it, a copy each time DevTools'
    /// `Page` agent is told of the frames), whole once it has.
    fn document_source(&mut self, frame: FrameId) -> Rc<str> {
        let loading = self.doc(frame).is_some_and(|next| next.body.is_some());
        if loading
            && let Some(loader) = self
                .engine
                .as_mut()
                .and_then(|engine| engine.document_loader(frame))
        {
            return Rc::from(loader.text());
        }
        self.doc(frame)
            .map_or_else(|| Rc::from(""), |next| next.source.clone())
    }

    /// `frame`'s document as the engine has it now (its iframe's content
    /// document; the top-level frame's, its commit's), without what the
    /// renderer keeps of it.
    fn engine_document(&mut self, frame: FrameId) -> Option<NodeRoot> {
        if self.render_frames.top() == Some(frame) {
            return self
                .render_frames
                .top_document()
                .map(|next| next.document.clone());
        }
        let (_, owner) = self.child_parent(frame)?;
        let engine = self.engine.as_mut()?;
        engine.dom_mut().read(|cx| {
            cx.content_document(owner.get(cx.gc))
                .map(|document| cx.root(document))
        })
    }

    /// After a task: what the renderer had of the frames the engine let go
    /// let go of (`forget_gone_frames`), then the blob URLs the engine
    /// asked for (`ScriptEngine::take_frame_requests`: an engine with no
    /// load sink's; the renderer's sink does them as they come).
    /// Navigations are booked on the engine's frames as they are sent.
    pub(super) fn handle_frame_requests(&mut self) {
        loop {
            self.forget_gone_frames();
            let Some(engine) = &mut self.engine else {
                return;
            };
            let requests = engine.take_frame_requests();
            if requests.is_empty() {
                return;
            }
            for request in requests {
                match request {
                    FrameRequest::RegisterBlobUrl { frame, url, data } => {
                        self.blob_urls
                            .register(&url, frame, data.type_(), data.shared_bytes());
                    }
                    FrameRequest::RevokeBlobUrl { url } => self.blob_urls.revoke(&url),
                }
            }
        }
    }

    /// The token the browser knows `frame` by: the main frame's, the
    /// browser's; a child frame's, the engine's.
    pub(super) fn frame_token(&mut self, frame: FrameId) -> Option<protocol::FrameToken> {
        if self.render_frames.top() == Some(frame) {
            self.render_frames.top_token()
        } else {
            self.child_token(frame)
        }
    }

    /// `RendererMsg::CommitSameDocumentNavigation` for the DOM: the
    /// browser's traversal to another entry of the top-level frame's
    /// document (`ScriptEngine::traverse_within_document`: its URL, state
    /// and mode; `popstate` now, `hashchange` in a task), which scrolls
    /// to the fragment unless the mode is manual. If the frame shows
    /// another document by now, the browser loads the entry.
    pub(super) fn commit_same_document_traversal(
        &mut self,
        token: protocol::FrameToken,
        navigation_id: u64,
        document: u64,
        url: &str,
        history: protocol::renderer::HistoryEntryParams,
    ) {
        let frame = if self.render_frames.top_token() == Some(token) {
            self.render_frames.top()
        } else {
            self.child_of_token(token)
        };
        let frame = frame.filter(|frame| {
            self.doc(*frame)
                .is_some_and(|next| next.navigation_id == document)
        });
        let token = frame.and_then(|frame| self.frame_token(frame));
        let (Some(frame), Ok(new_url), Some(engine)) = (frame, Url::parse(url), &mut self.engine)
        else {
            let _ = self
                .to_browser
                .send(RendererHostMsg::SameDocumentNavigationFailed { navigation_id });
            return;
        };
        // As the browser says: the new entries of the renderer's own it
        // hears of before the traversal is in come before it, and it says
        // where things are once it is (`SetHistoryOffsetAndLength`).
        engine.set_session_history(dom::SessionHistory {
            index: history.index,
            length: history.length,
        });
        // The browser hears it committed before `popstate` fires, as
        // Blink tells it: a navigation a handler asks for comes after.
        let to_browser = &self.to_browser;
        let mut committed = || {
            if let Some(token) = token {
                let _ = to_browser.send(RendererHostMsg::DidCommitSameDocumentNavigation {
                    frame: token,
                    url: new_url.as_str().into(),
                    replace: false,
                    state: None,
                    navigation_id: Some(navigation_id),
                });
            }
        };
        if !engine.traverse_within_document(
            frame,
            url,
            history.state.map(Vec::from),
            history.manual_scroll_restoration,
            &mut committed,
        ) {
            let _ = self
                .to_browser
                .send(RendererHostMsg::SameDocumentNavigationFailed { navigation_id });
            return;
        }
        self.request_next_frame_if_dom_changed();
        self.send_harness_results();
    }

    /// The browser's commit of child frame `token`'s navigation
    /// `navigation_id` (`CommitNavigation`), the renderer's `begin_id` if
    /// it asked for it: nothing for a frame gone, one that asked for
    /// another since (or committed an `about:` document), or one that
    /// shows a later navigation's document (an older commit whose body
    /// stopped short as the later one replaced it). Whether it committed.
    pub(super) fn commit_child_navigation(
        &mut self,
        token: protocol::FrameToken,
        begin_id: Option<u64>,
        document: ChildDocument<'_>,
        navigation: ChildCommit<'_>,
    ) -> bool {
        let Some(frame) = self.child_of_token(token) else {
            return false;
        };
        if let Some(begin_id) = begin_id
            && self
                .engine
                .as_mut()
                .and_then(|engine| engine.navigation(frame))
                .is_none_or(|asked| asked.begin_id != begin_id)
        {
            tracing::debug!(url = %document.url, "dropping a stale commit of a child frame");
            return false;
        }
        if self
            .render_frames
            .document(frame)
            .is_some_and(|next| next.navigation_id > navigation.navigation_id)
        {
            tracing::debug!(url = %document.url, "dropping a commit older than the child frame's document");
            return false;
        }
        // One the browser started takes the renderer's place too.
        let asked = self
            .engine
            .as_mut()
            .and_then(|engine| engine.take_navigation(frame, begin_id));
        let navigation = ChildCommit {
            origin_source: asked.and_then(|asked| asked.origin_source),
            ..navigation
        };
        self.commit_child(
            frame,
            document.url,
            document.mime_type,
            document.charset,
            document.body,
            Some(navigation),
        );
        true
    }

    /// Whether the browser's commit of child frame `token` for the
    /// renderer's `begin_id` reads its response's timing with a
    /// `no-referrer` policy (its iframe's, as it asked).
    pub(super) fn child_no_referrer_of(
        &mut self,
        token: protocol::FrameToken,
        begin_id: Option<u64>,
    ) -> bool {
        let frame = self.child_of_token(token);
        frame
            .zip(self.engine.as_mut())
            .and_then(|(frame, engine)| engine.navigation(frame))
            .is_some_and(|asked| Some(asked.begin_id) == begin_id && asked.no_referrer)
    }

    /// The browser dropped navigation `id` of child frame `token`
    /// (`NavigationDropped`): it no longer loads, and its parent's `load`
    /// no longer waits for it, unless it asked for another since.
    pub(super) fn navigation_dropped(&mut self, token: protocol::FrameToken, id: u64) {
        let Some(frame) = self.child_of_token(token) else {
            return;
        };
        if self
            .engine
            .as_mut()
            .and_then(|engine| engine.take_navigation(frame, Some(id)))
            .is_none()
        {
            return;
        }
        let parent = self.child_parent(frame).map(|(parent, _)| parent);
        if let Some(parent) = parent {
            self.finish_load(parent);
        }
    }

    /// Child frame `token`'s navigation to a `javascript:` URL, which the
    /// engine booked and told of (`FrameClient::commit_here`), in a task of
    /// its own: an empty document commits, as one fetched would once its
    /// response came (Chrome's goes through a navigation request too).
    /// Nothing if a navigation took its place since, or the frame went.
    pub(super) fn run_child_commit(&mut self, token: protocol::FrameToken) {
        let Some(frame) = self.child_of_token(token) else {
            return;
        };
        let Some(url) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.take_commit_here(frame))
        else {
            return;
        };
        let _task = trace_event::trace_event!("ThreadControllerImpl::RunTask");
        let reported = url.to_string();
        let body = NavigationBody {
            start: bytes::Bytes::new(),
            pipe: None,
            loaded: None,
            url: url.to_string(),
        };
        self.commit_child(frame, url, "text/html", None, body, None);
        // The browser hears of it as of a commit it didn't send (Chromium's
        // renderer's synchronous commit, which the browser hears of
        // after): its URL is the frame's.
        if let Some(token) = self.child_token(frame) {
            let _ = self.to_browser.send(RendererHostMsg::DidCommitNavigation {
                frame: token,
                navigation_id: RENDERER_COMMIT,
                url: reported,
            });
        }
        self.send_harness_results();
    }

    /// A child frame's new document, as `commit_navigation` commits
    /// the top-level one's: its realm and document made (its iframe's
    /// content document), its view in its parent's, the body pushed to its
    /// parser, then parsed and its scripts run; `navigation`, the
    /// browser's that loaded it (none for an `about:` document the
    /// renderer commits itself, whose time origin is now), told as it
    /// commits. Nothing for a frame that went.
    fn commit_child(
        &mut self,
        frame: FrameId,
        url: Url,
        mime_type: &str,
        charset: Option<&str>,
        body: NavigationBody,
        navigation: Option<ChildCommit<'_>>,
    ) {
        let _span = trace_event::info_event!("commit_child", %url, %mime_type);
        let Some((parent, owner)) = self.child_parent(frame) else {
            return;
        };
        // The browser's loader for it; one the renderer commits itself (a
        // `javascript:` URL's) goes through the frame's document's before.
        let loader = match &navigation {
            Some(navigation) => navigation.loader,
            None => self.loader_of(frame),
        };
        let document_id = crate::frame_view::next_document_id();
        let (now, time_origin, timing) = match &navigation {
            Some(navigation) => (
                self.clock
                    .instant_at(navigation.time_origin)
                    .unwrap_or(self.clock.start()),
                navigation.time_origin,
                navigation.timing.clone(),
            ),
            None => {
                let now = std::time::Instant::now();
                let time_origin = self.clock.at(now);
                let clock = self.clock;
                let timing = crate::navigation_timing::network_timing(
                    None,
                    dom::NavigationType::Navigate,
                    time_origin,
                    |time: ipc::TimeTicks| {
                        clock.before_now(ipc::TimeTicks::now().saturating_since(time))
                    },
                    crate::navigation_timing::randomized_confidence(),
                );
                (now, time_origin, timing)
            }
        };
        let previous = self.document_url(frame);
        // DevTools hears of the new document as the engine makes it, before
        // its realm (`PageAgent::child_document_created`).
        if !self.devtools_sessions.is_empty()
            && let Some(info) =
                self.committing_child_info(frame, &url, mime_type, navigation.as_ref())
        {
            self.page_agent.child_committing(info, navigation.is_none());
        }
        // The realm before it goes, and the blob URLs it made.
        self.blob_urls.revoke_frame(frame);
        let clock = self.clock;
        let Some(engine) = &mut self.engine else {
            return;
        };
        let more = body.pipe.is_some();
        let (body, start) = DocumentBody::new(body);
        // The document it shows unloaded first, its `unload` timed.
        let unloaded = engine
            .unload_document(frame)
            .map(|(start, end)| (clock.at(start), clock.at(end)));
        let timing =
            crate::navigation_timing::with_unload(timing, unloaded, previous.as_ref(), &url);
        let Some(document) = engine.create_window(frame, url.as_str(), now) else {
            return;
        };
        // Its loading: its body decoded as its start says, and parsed (an
        // iframe's `srcdoc` document never in quirks mode).
        let loading = dom_api::DocumentLoader::new(
            engine.dom_mut(),
            &document,
            url.as_str() == "about:srcdoc",
            mime_type,
            charset,
            &start,
            more,
        );
        let document_encoding = loading.encoding();
        engine.set_document_loader(frame, loading);
        if let Some(source) = navigation
            .as_ref()
            .and_then(|navigation| navigation.origin_source.as_ref())
        {
            engine.inherit_origin(frame, &document, source);
        }
        engine.set_navigation_timing(
            frame,
            crate::navigation_timing::document_timing(&timing, time_origin, self.clock.now()),
        );
        // Its own response's, as a top-level document's is.
        let content_language = navigation
            .as_ref()
            .and_then(|navigation| navigation.content_language);
        set_document_metadata(
            engine.dom_mut(),
            &document,
            mime_type,
            document_encoding,
            content_language,
        );
        // Where its entry is in the session history (every frame's), and
        // its item's state.
        if let Some(navigation) = &navigation {
            let history = &navigation.history;
            engine.set_session_history(self.own_history_entries.place(
                history.index,
                history.length,
                history.entries_heard,
            ));
            engine.restore_history(
                frame,
                history.state.as_deref().map(<[u8]>::to_vec),
                history.manual_scroll_restoration,
            );
        }
        let Some((dom, next)) = engine_frame(&mut self.engine) else {
            return;
        };
        let (node, owner_id) = dom.read(|cx| {
            (
                cx.node_id(document.get(cx.gc)),
                cx.node_id(owner.get(cx.gc)),
            )
        });
        dom.mutate(|cx| {
            cx.set_document_loader(
                document.get(cx.gc),
                dom::DocumentLoader {
                    loader,
                    navigation: navigation
                        .as_ref()
                        .map(|navigation| navigation.navigation_id),
                },
            );
        });
        let view = FrameView::new(document.clone(), node, document_id, self.system_colors)
            .into_child(frame, owner_id);
        let top = self.render_frames.top();
        next.view
            .show_child((Some(parent) != top).then_some(parent), view);
        // The document before's `fetch()`es go with it (the DOM has it
        // discarded).
        self.cancel_fetches_of_gone_documents();
        let navigation_id = navigation
            .as_ref()
            .map_or(0, |navigation| navigation.navigation_id);
        // DevTools' `DOM` agent tells of the iframe's new document as the
        // frames are looked at after the task.
        self.frames_changed = true;
        self.render_frames.set_child_document(
            frame,
            FrameDocument {
                navigation_id,
                body: Some(body),
                time_origin,
                ..FrameDocument::new(frame, mime_type, Rc::from(""), document, document_encoding)
            },
        );
        // Its children before went with the document before, the engine's
        // as it made this one (`create_window`): what the renderer had of
        // them is let go of after the task (`forget_gone_frames`).
        self.request_main_frame();
        // The browser hears of it before of the frames it makes.
        if let Some(navigation) = navigation {
            let _ = self.to_browser.send(RendererHostMsg::DidCommitNavigation {
                frame: navigation.token,
                navigation_id: navigation.navigation_id,
                url: navigation.url,
            });
        }
        self.start_body(frame, start);
    }

    /// `frame`'s document's body starts with `start` (all of it, if the
    /// body has no pipe): it goes to the parser, then what the pipe has
    /// now (`read_body`).
    fn start_body(&mut self, frame: FrameId, start: bytes::Bytes) {
        let Some(body) = self.doc_mut(frame).and_then(|next| next.body.as_mut()) else {
            return;
        };
        let last = body.pipe_mut().is_none();
        body.take(&start);
        self.body_data(frame, &start, last);
        self.read_body(frame);
    }

    /// What `frame`'s document's body's pipe has now, at most
    /// `BYTES_PER_TASK` (the rest in a task of its own), each piece handed
    /// to its parser (Blink's `NavigationBodyLoader::ReadFromDataPipe`),
    /// the pipe watched once it has none; once the body has all come and
    /// its load has ended, `body_ended`. Then the parser goes on, unless it
    /// waits for a script (it parses what came after the script once the
    /// script has run).
    fn read_body(&mut self, frame: FrameId) {
        use super::document_body::BYTES_PER_TASK;
        let token = self.frame_token(frame);
        // Its watcher's tasks are posted from the network service's thread.
        let scheduler = Arc::clone(&self.scheduler);
        let mut read = 0usize;
        loop {
            let Some(next) = self.doc_mut(frame) else {
                return;
            };
            let navigation_id = next.navigation_id;
            let Some(body) = &mut next.body else {
                break;
            };
            let left = BYTES_PER_TASK.saturating_sub(read);
            if left == 0 {
                // Read on in a task of its own, as if the pipe had just
                // become readable.
                if let Some(token) = token {
                    (scheduler.body_waker(token, navigation_id))();
                }
                break;
            }
            match body.read(left) {
                BodyRead::Data(bytes) => {
                    read = read.saturating_add(bytes.len());
                    self.body_data(frame, &bytes, false);
                }
                BodyRead::Waiting => {
                    if let Some(token) = token
                        && let Some(pipe) = body.pipe_mut()
                    {
                        pipe.watch(scheduler.body_waker(token, navigation_id));
                    }
                    break;
                }
                BodyRead::Over => {
                    self.body_data(frame, &[], true);
                    break;
                }
                BodyRead::Ended => break,
            }
        }
        if self
            .doc(frame)
            .and_then(|next| next.body.as_ref())
            .is_some_and(DocumentBody::is_over)
        {
            self.body_ended(frame);
        }
        // A parser that let other work run parses what came in the task it
        // posted (Blink's parser, once scheduled, isn't pumped by what
        // comes).
        self.parse_document(frame, dom_api::document_loader::parse_unless_due);
    }

    /// A parser stopped for now (it let other work run, or waits for the
    /// rest of its body): the loads of its `<link>`s (its iframes'
    /// documents' too) start, so the sheets block its rendering before a
    /// frame comes.
    fn parser_stopped(&mut self) {
        self.start_loads();
    }

    /// `bytes` of `frame`'s document's body came (`last`: the end of it):
    /// they go to the engine's loading of the document, which decodes them
    /// to the end of its parser's input, and DevTools hears of the bytes of
    /// a document it loaded (`Network.dataReceived`, Blink's
    /// `probe::DidReceiveData`).
    fn body_data(&mut self, frame: FrameId, bytes: &[u8], last: bool) {
        if let Some(loader) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.document_loader(frame))
        {
            loader.body_data(bytes, last);
        }
        let length = bytes.len();
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        let navigation_id = next.navigation_id;
        let loaded = next
            .body
            .as_ref()
            .is_some_and(|body| body.loaded().is_some());
        if loaded && length > 0 {
            self.network_agent
                .navigation_data_received(navigation_id, length);
        }
    }

    /// The document `token`'s navigation `navigation_id` committed in it,
    /// while its body comes: its frame.
    fn frame_with_body(
        &mut self,
        token: protocol::FrameToken,
        navigation_id: u64,
    ) -> Option<FrameId> {
        let frame = if Some(token) == self.render_frames.top_token() {
            self.render_frames.top()?
        } else {
            self.child_of_token(token)?
        };
        self.doc(frame)
            .filter(|next| next.navigation_id == navigation_id && next.body.is_some())
            .map(|next| next.frame)
    }

    /// The pipe of the body of the document navigation `navigation_id`
    /// committed in `token` has more, or is over (its watcher's task):
    /// read. Nothing if that document has gone.
    pub(super) fn read_body_of(&mut self, token: protocol::FrameToken, navigation_id: u64) {
        if let Some(frame) = self.frame_with_body(token, navigation_id) {
            self.read_body(frame);
        }
    }

    /// The load of the document navigation `navigation_id` committed in
    /// `token` ended as `status` (`NavigationLoadCompleted`): its body is
    /// over once its pipe is too.
    pub(super) fn document_load_completed(
        &mut self,
        token: protocol::FrameToken,
        navigation_id: u64,
        status: protocol::network::CompletionStatus,
    ) {
        let Some(frame) = self.frame_with_body(token, navigation_id) else {
            return;
        };
        if let Some(body) = self.doc_mut(frame).and_then(|next| next.body.as_mut()) {
            body.completed(status);
        }
        self.read_body(frame);
    }

    /// `frame`'s document's body has all come and its load has ended
    /// (Blink's `DocumentLoader::BodyLoadingFinished`): its source is
    /// whole, its navigation timing has its response's end and body sizes,
    /// DevTools hears it finished (or failed: its parser is aborted once
    /// what came is parsed), and the log has a body that stopped short.
    fn body_ended(&mut self, frame: FrameId) {
        let source = self.document_source(frame);
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        let Some(body) = next.body.take() else {
            return;
        };
        let finished = body.finish();
        next.source = source;
        // Stopped short (an error, or its load was stopped), as its load's
        // end says: its parser is aborted once what came is parsed (HTML's
        // "abort a parser"), so it is complete with no `DOMContentLoaded`
        // or `load`.
        let aborted = finished
            .loaded
            .as_ref()
            .and_then(|loaded| loaded.end.as_ref())
            .is_some_and(|end| end.error.is_some());
        if let Some(loader) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.document_loader(frame))
        {
            loader.body_ended(aborted);
        }
        let Some(next) = self.doc_mut(frame) else {
            return;
        };
        let navigation_id = next.navigation_id;
        let time_origin = next.time_origin;
        let mime_type = next.mime_type.clone();
        let Some(loaded) = finished.loaded else {
            return;
        };
        if let Some(end) = &loaded.end
            && let Some(engine) = &mut self.engine
        {
            let clock = self.clock;
            let response_end =
                clock.before_now(ipc::TimeTicks::now().saturating_since(end.completion_time));
            // In machopo's trace too, a mark named as Chrome's
            // (`DocumentLoadTiming::SetResponseEnd`), at its time.
            tracing::trace!(
                target: "blink.user_timing",
                mark = "responseEnd",
                ago_ns = base::numerics::saturated_cast::<i64, _>(
                    (clock.now() - response_end) * 1e6
                )
            );
            let decoded = u64::try_from(finished.bytes.len()).unwrap_or(u64::MAX);
            engine.navigation_body_ended(
                frame,
                response_end - time_origin,
                end.encoded_body_length,
                decoded,
            );
        }
        self.network_agent
            .navigation_finished(navigation_id, &finished.bytes, &mime_type, &loaded);
        // A body that stopped short (Blink's `FrameConsole::
        // DidFailLoading`; a load stopped isn't told).
        if let Some(error) = loaded.end.as_ref().and_then(|end| end.error.as_ref())
            && error.code != protocol::network::NetErrorCode::Aborted
        {
            self.log_agent.add(super::log_agent::LogEntry::network(
                format!("Failed to load resource: net::{}", error.code.name()),
                finished.url,
                navigation_id.to_string(),
            ));
        }
    }

    /// Parses `frame`'s document for a run of its budget (the engine's
    /// loading of it, `dom_api::document_loader::parse`: the parser's
    /// scripts run as it stops at them, each once it is ready), then what
    /// came of it: the frames its iframes made and the loads of its
    /// `<link>`s start (so its sheets block its rendering before a frame
    /// comes), and, as it stopped, it parses on in a task of its own, or is
    /// parsed (HTML's "the end", `document_parsed`).
    pub(super) fn advance_document(&mut self, frame: FrameId) {
        self.parse_document(frame, dom_api::document_loader::parse);
    }

    /// `entry` (a `dom_api::document_loader` one that parses) run for
    /// `frame`'s document with the renderer's budget (`parse_run`), the
    /// engine lent to it, then what came of it.
    fn parse_document(&mut self, frame: FrameId, entry: dom_api::document_loader::ParseEntry) {
        if self.doc(frame).is_none() {
            return;
        }
        let Some(lent) = self.engine.take() else {
            return;
        };
        let (outcome, engine) = self.parse_run(|run| entry(lent, frame, run));
        self.engine = Some(engine);
        self.send_harness_results();
        match outcome {
            ParseOutcome::Stopped => {}
            // It parses on in a task it posted (`Cx::queue_parse_on`).
            ParseOutcome::WaitsForScript | ParseOutcome::WaitsForBody | ParseOutcome::Yielded => {
                self.parser_stopped()
            }
            // HTML's "the end" ran as far as it can in the call
            // (`dom_api::document_loader::parse`): it may be rendered now,
            // body or not.
            ParseOutcome::Parsed => self.request_main_frame(),
        }
    }

    /// The parser's scripts that wait for a sheet that blocks scripts, tried
    /// again after the tasks: script may have removed its `<link>`, or made
    /// it no stylesheet, so they run without waiting for its response.
    /// Whether a parser's scripts wait for a sheet that blocks scripts.
    pub(super) fn scripts_wait_for_sheets(&mut self) -> bool {
        !self.scripts_blocked_by_sheets().is_empty()
    }

    pub(super) fn retry_scripts_blocked_by_sheets(&mut self) {
        for (frame, parser_blocked) in self.scripts_blocked_by_sheets() {
            if parser_blocked {
                self.advance_document(frame);
            } else {
                self.run_deferred_scripts(frame);
            }
        }
    }

    /// The frames whose parser's script, or deferred scripts, a sheet that
    /// blocks scripts may hold back: whether it is the parser's.
    fn scripts_blocked_by_sheets(&mut self) -> Vec<(FrameId, bool)> {
        let documents: Vec<(FrameId, NodeRoot)> = self
            .frames_with_documents()
            .into_iter()
            .filter_map(|frame| self.doc(frame).map(|next| (frame, next.document.clone())))
            .collect();
        let Some(engine) = &mut self.engine else {
            return Vec::new();
        };
        documents
            .into_iter()
            .filter_map(|(frame, document)| {
                let parser_blocked = engine
                    .document_loader(frame)
                    .is_some_and(|loader| loader.waits_for_script());
                let dom = engine.dom_mut();
                let deferring = dom.read(|cx| cx.load_progress(document.get(cx.gc)))
                    == dom::LoadProgress::Deferring;
                ((parser_blocked || deferring) && sheets_pending_for_scripts(dom, &document))
                    .then_some((frame, parser_blocked))
            })
            .collect()
    }

    /// The document's deferred scripts run in order, each once fetched and
    /// no stylesheet blocks scripts (their arrival runs this again), while
    /// it defers; once none is left, `DOMContentLoaded` and the rest of
    /// HTML's "the end" (`dom_api::document_loader::run_deferred_scripts`).
    fn run_deferred_scripts(&mut self, frame: FrameId) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        dom_api::document_loader::run_deferred_scripts(engine.as_mut(), frame);
        self.send_harness_results();
        // A wasm script may have drawn.
        if self.engine_is_wasm {
            self.request_main_frame();
        }
    }

    /// The scripts the DOM queued that wait for no parser
    /// (`Dom::take_script_fetches`), fetched: each handed back
    /// (`Cx::script_fetched`) as its response comes (`resource`). The
    /// engine sent those its calls prepared already; the rest (the
    /// parser's) it sends now (`RendererLoadSink::start_script_fetch`).
    pub(super) fn start_script_fetches(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.send_script_fetches();
        }
    }

    /// `url`, a subresource of `frame`'s document, failed (or was of no
    /// use): noted in the DOM (`Cx::note_load_failed`), the browser hears of
    /// it with the document's load.
    fn note_failed(&mut self, frame: FrameId, url: String) {
        let Some(document) = self.doc(frame).map(|next| next.document.clone()) else {
            return;
        };
        if let Some(engine) = &mut self.engine {
            engine
                .dom_mut()
                .mutate(|cx| cx.note_load_failed(document.get(cx.gc), url));
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
        let frame = self.engine.as_mut()?.frame_of_document(document)?;
        self.has_frame_document(frame).then_some(frame)
    }

    /// `finish_load` of every frame the engine has, the children's before
    /// their parents' (whose `load` waits for theirs).
    fn finish_loads(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        let mut frames = engine.frames_in_tree_order();
        frames.reverse();
        for frame in frames {
            self.finish_load(frame);
        }
    }

    /// HTML's "the end" for `frame`'s document as far as it can go
    /// (`dom_api::document_loader::finish_load`): complete and its window's
    /// `load` task once nothing delays it.
    fn finish_load(&mut self, frame: FrameId) {
        let Some(document) = self.engine_document(frame) else {
            return;
        };
        if let Some(engine) = &mut self.engine {
            dom_api::document_loader::finish_load(engine.as_mut(), &document);
        }
    }

    /// The tab's title, the DOM's document's (Blink's
    /// `Document::UpdateTitle` and `DispatchDidReceiveTitle`), after each
    /// task: told by the engine through the frame client if it differs
    /// from the last one told (`ScriptEngine::tell_title`: the first,
    /// empty or not, at once; the browser shows the URL for an empty one,
    /// as before any).
    pub(super) fn update_title(&mut self) {
        if let Some(engine) = &mut self.engine {
            engine.tell_title();
        }
    }

    /// The main frame and the DOM's document in it as DevTools tells
    /// them, once DevTools gave the frame its token.
    pub(super) fn frame_info(&mut self) -> Option<FrameInfo> {
        let url = self.top_document_url()?;
        let source = self.document_source(self.render_frames.top()?);
        let next = self.render_frames.top_document()?;
        main_frame_info(
            self.render_frames.top_token(),
            next.navigation_id,
            &url,
            &next.mime_type,
            &source,
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
        let top = self.render_frames.top();
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
        let Some(top) = self.render_frames.top() else {
            return Vec::new();
        };
        let mut children = self.child_frames_below(top);
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
        let Some(top) = self
            .render_frames
            .top_document()
            .map(|next| next.document.clone())
        else {
            return Vec::new();
        };
        let top_frame = self.render_frames.top();
        let children: Vec<(FrameId, NodeRoot)> = top_frame
            .map(|frame| self.child_frames_below(frame))
            .unwrap_or_default()
            .into_iter()
            .map(|(frame, _, owner)| (frame, owner))
            .collect();
        let ids: Vec<String> = children
            .iter()
            .map(|(frame, _)| self.frame_id(*frame))
            .collect();
        let main = self
            .render_frames
            .top_token()
            .map(|token| token.to_string())
            .unwrap_or_default();
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
        let navigating = self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.navigation(frame).is_some());
        let id = self.frame_id(frame);
        let source = self.document_source(frame);
        let committed = self.doc(frame).map(|next| {
            (
                next.mime_type.clone(),
                source,
                next.resources.clone(),
                next.initial,
            )
        });
        let engine = self.engine.as_mut()?;
        let (document, url, origin, name, loader) = engine.dom_mut().read(|cx| {
            let owner = owner.get(cx.gc);
            let document = cx.content_document(owner)?;
            let loader = cx.document_loader(document);
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
            Some((cx.node_id(document), url, origin, name, loader))
        })?;
        let loader_id = crate::loader::devtools_loader_id(loader, document);
        let document = u64::from(document.get());
        let secure_context = super::devtools::secure_context(&url, Some(parent.secure_context));
        let (mime_type, source, resources, initial) = committed
            .unwrap_or_else(|| ("text/html".to_owned(), Rc::from(""), Rc::default(), true));
        Some(FrameInfo {
            id,
            parent_id: Some(parent.id.clone()),
            // Its request's, if one brought it.
            loader_id,
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
        })
    }

    /// Child frame `frame`'s document committing at `url`, of `mime_type`,
    /// as DevTools tells it, before the engine makes it: its number to come
    /// (`PageAgent::child_document_created`), its loader the browser's
    /// `navigation`'s (none: the document's number, to come too), its
    /// origin the document's it inherits one from, if it does, as the
    /// commit gives it (`ScriptEngine::inherit_origin`). None for a frame
    /// DevTools doesn't have now.
    fn committing_child_info(
        &mut self,
        frame: FrameId,
        url: &Url,
        mime_type: &str,
        navigation: Option<&ChildCommit<'_>>,
    ) -> Option<FrameInfo> {
        let frames = self.document_frame_infos();
        let id = self.frame_id(frame);
        let known = frames.iter().find(|info| info.id == id)?;
        let parent = frames
            .iter()
            .find(|info| Some(&info.id) == known.parent_id.as_ref())?;
        // As Blink's `SecurityOrigin::ToRawString`: an opaque one's scheme
        // and host are empty.
        let serialize = |origin: Option<url::Origin>| match origin {
            Some(origin @ url::Origin::Tuple(..)) => origin.ascii_serialization(),
            _ => "://".to_owned(),
        };
        let origin = match navigation.and_then(|navigation| navigation.origin_source.as_ref()) {
            Some(source) => serialize(
                self.engine
                    .as_mut()?
                    .dom_mut()
                    .read(|cx| cx.document_origin(source.get(cx.gc))),
            ),
            // An `about:` document's own is its parent's (`origin_document`).
            None if matches!(url.scheme(), "about" | "javascript") => parent.origin.clone(),
            None => serialize(Some(url.origin())),
        };
        Some(FrameInfo {
            id,
            parent_id: known.parent_id.clone(),
            loader_id: navigation
                .map(|navigation| navigation.navigation_id.to_string())
                .unwrap_or_default(),
            document: 0,
            name: known.name.clone(),
            initial: false,
            // Committing: its navigation was taken.
            navigating: false,
            url: url.to_string(),
            origin,
            mime_type: mime_type.to_owned(),
            secure_context: super::devtools::secure_context(
                url.as_str(),
                Some(parent.secure_context),
            ),
            source: Rc::from(""),
            resources: Rc::default(),
        })
    }

    /// The requests in flight that `document` made (its
    /// `ResourceFetcher`'s, as Blink's `IdlenessDetector` counts): its
    /// sheets, its scripts (the parser's and the others), the images it
    /// asked for first, its `fetch()`es, as the DOM records them. Not a
    /// child frame's `src` (its own document's request).
    fn requests_in_flight(&mut self, document: dom::NodeId) -> usize {
        let (fetches, images, queued, sheets) =
            self.engine.as_mut().map_or((0, 0, 0, 0), |engine| {
                let dom = engine.dom_mut();
                (
                    dom.sent_fetches_of(document),
                    dom.sent_images_of(document),
                    dom.sent_scripts_of(document),
                    dom.sent_sheets_of(document),
                )
            });
        // Each fewer than the requests the renderer has made.
        sheets
            .saturating_add(queued)
            .saturating_add(images)
            .saturating_add(fetches)
    }

    /// The DOM's frames' documents whose network has been quiet long
    /// enough (`check_network_idle`'s).
    pub(super) fn check_network_idle(&mut self) {
        // Each document's network watched for quiet from its
        // `DOMContentLoaded` on (the DOM's: the engine fired it in its call).
        let roots: Vec<(FrameId, NodeRoot)> = self
            .frames_with_documents()
            .into_iter()
            .filter_map(|frame| self.doc(frame).map(|next| (frame, next.document.clone())))
            .collect();
        let Some(engine) = &mut self.engine else {
            return;
        };
        let documents: Vec<(FrameId, dom::NodeId, bool)> = roots
            .into_iter()
            .map(|(frame, document)| {
                let (id, content_loaded) = engine.dom_mut().read(|cx| {
                    let document = document.get(cx.gc);
                    (cx.node_id(document), cx.content_loaded(document))
                });
                (frame, id, content_loaded)
            })
            .collect();

        let now = ipc::TimeTicks::now();
        let mut due = Vec::new();
        for (frame, document, content_loaded) in documents {
            let active = self.requests_in_flight(document);
            if let Some(next) = self.doc_mut(frame) {
                if content_loaded {
                    next.idleness.start();
                }
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
        self.render_frames
            .documents()
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
        if let Some(engine) = &mut self.engine {
            engine.send_sheet_fetches();
        }
    }

    /// Whether the top-level frame's document isn't to be rendered yet
    /// (`Cx::render_blocked`: a sheet that blocks rendering is loading, or
    /// its parser hasn't made its body): no frame till then.
    pub(super) fn render_blocked(&mut self) -> bool {
        self.render_frames
            .top()
            .is_some_and(|top| self.frame_render_blocked(top))
    }

    /// `render_blocked`'s, for any frame's document: the DOM's, as it is
    /// now.
    fn frame_render_blocked(&mut self, frame: FrameId) -> bool {
        let Some(document) = self.engine_document(frame) else {
            return false;
        };
        self.engine.as_mut().is_some_and(|engine| {
            engine
                .dom_mut()
                .read(|cx| cx.render_blocked(document.get(cx.gc)))
        })
    }

    /// The main frame's document, which can't be drawn yet, shown empty in
    /// place of the page before (`RenderWidgetHostImpl::
    /// ClearDisplayedGraphics`): no layers but the background, as the
    /// document's, so input goes to it; the input waiting for a frame goes
    /// with it, a visual state and the load's end wait for it to be drawn.
    fn commit_empty_frame(&mut self) -> bool {
        let Some((_, frame)) = engine_frame(&mut self.engine) else {
            return false;
        };
        let Frame { view, widget, .. } = frame;
        let host = &mut widget.layer_tree_host;
        host.set_layers(
            Vec::<compositor::Layer>::new(),
            compositor::PropertyTrees::default(),
        );
        host.set_scroll_offsets(compositor::ScrollOffsets::default());
        host.set_background_color(gfx::Color::WHITE);
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 0.0));
        host.set_root_scroll_origin(gfx::PointF::new(0.0, 0.0));
        host.set_document_id(view.document_id());
        host.set_viewport_scroll_node(None);
        let promises = std::mem::take(&mut widget.pending_input_acks);
        let _span = trace_event::trace_event!("RenderWidgetHostImpl::ClearDisplayedGraphics");
        widget.layer_tree_host.commit(promises);
        true
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
                let rtl = |style: &computed_style::ComputedStyle| {
                    style.rare_inherited.direction == computed_style::Direction::Rtl
                };
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
                            rtl: cx
                                .node_by_id(item.node)
                                .and_then(|item| frame.computed_style(cx, item))
                                .is_some_and(|style| rtl(&style)),
                        })
                        .collect(),
                    selected,
                    right_aligned: rtl(&style),
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
        let top = self.render_frames.top();
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
        let (Some(engine), Some(next)) = (&mut self.engine, self.render_frames.top_document_mut())
        else {
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
        let Some(next) = self.render_frames.top_document_mut() else {
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
            return std::mem::take(&mut self.empty_frame_wanted) && self.commit_empty_frame();
        }
        // Drawn: the page before goes now.
        self.new_content_due = None;
        self.empty_frame_wanted = false;
        // The frames as the engine has them now, if DevTools' `CSS` lists
        // their sheets: a frame's document committed since the last frame
        // is one of them already.
        let frames = self.css_agent.on().then(|| self.dom_frames());
        let (Some((dom, frame)), Some(next)) = (
            engine_frame(&mut self.engine),
            self.render_frames.top_document_mut(),
        ) else {
            return false;
        };
        (next.rendered_version, next.rendered_canvas_draws) =
            dom.read(|cx| (cx.tree_version(), cx.canvas_draws_version()));
        update_lifecycle_and_layers(
            dom,
            frame,
            &self.overlay_agent,
            &self.css_agent,
            frames.as_deref(),
        );
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
            if let Some(due) = self.next_caret_blink {
                self.scheduler
                    .post_internal_at(InternalTask::CaretBlink, due);
            }
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
        let load_ended =
            dom.read(|cx| cx.load_progress(next.document.get(cx.gc))) == dom::LoadProgress::Loaded;
        if load_ended && !std::mem::replace(&mut next.load_told, true) {
            next.waiting_for_reftest = reftest_wait;
            // The DOM's, which whoever found each noted.
            let failed_subresources = dom.read(|cx| cx.take_failed_loads(next.document.get(cx.gc)));
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
        // The child frames' documents whose rendering is blocked, as the
        // engine has them.
        let top = self.render_frames.top();
        let frames = self
            .engine
            .as_mut()
            .map(|engine| engine.frames_in_tree_order())
            .unwrap_or_default();
        let mut blocked: Vec<NodeRoot> = Vec::new();
        for frame in frames.into_iter().filter(|frame| Some(*frame) != top) {
            if let Some(document) = self.engine_document(frame)
                && let Some(engine) = &mut self.engine
                && sheets_block_render(engine.dom_mut(), &document)
            {
                blocked.push(document);
            }
        }
        let documents = self
            .engine
            .as_mut()
            .map(|engine| engine.documents_in_tree_order())
            .unwrap_or_default();
        if let Some((dom, frame)) = engine_frame(&mut self.engine) {
            let _span =
                trace_event::trace_event!("LocalFrameView::UpdateViewportIntersectionsForSubtree");
            let due = compute_intersections(
                dom,
                frame,
                &documents,
                &blocked,
                &mut self.intersections_computed,
            );
            self.intersections_due = due.map(|wait| self.clock.now() + wait);
            if let Some(due) = self.intersections_due {
                self.post_intersections(due);
            }
        }
        self.recompute_under_mouse();
        true
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

    /// The DOM's task numbered `number`, as the scheduler took its token
    /// (`ScriptEngine::run_dom_task`; nothing if it is no more), then what
    /// it finished: documents it loaded, testharness results it gave.
    pub(super) fn run_dom_task(&mut self, number: u64) {
        let Some(lent) = self.engine.take() else {
            return;
        };
        // A parser's task to parse on in among them, run by `dom_api`'s
        // loading of its document with the renderer's budget.
        let (ran, engine) =
            self.parse_run(|run| dom_api::document_loader::run_dom_task(lent, number, run));
        self.engine = Some(engine);
        if ran {
            self.after_dom_tasks();
        }
    }

    /// `f` given the parser's run as the renderer has it (`ParserBudget`,
    /// its flags'): its budget by how many times it let other work run,
    /// what a script in the body leaves of it, its slices, and whether
    /// input waits in the scheduler (Blink's `ShouldYieldForHighPriorityWork`).
    fn parse_run<R>(&self, f: impl FnOnce(&dom_api::document_loader::ParseRun<'_>) -> R) -> R {
        let parser_budget = self.parser_budget;
        let scheduler = self.scheduler.clone();
        let input_waits = move || scheduler.has_task(TaskQueues::INPUT);
        let budget = move |yields| parser_budget.of_run(yields);
        f(&dom_api::document_loader::ParseRun {
            budget: &budget,
            after_script: parser_budget.budget,
            slice: parser_budget.slice_bytes,
            input_waits: &input_waits,
        })
    }

    /// What the DOM's tasks that ran finished: the documents whose window's
    /// `load` ran, testharness results, a `load` an image's waited for, a
    /// frame for what they changed.
    fn after_dom_tasks(&mut self) {
        self.send_harness_results();
        // A task's image `load` may have been what a `load` waited for.
        self.finish_loads();
        self.request_next_frame_if_dom_changed();
    }

    /// The engine's guests whose waits are over (a wasm guest's, not the
    /// DOM's tasks: `until_due_guests` woke the loop for them).
    pub(super) fn run_due_guests(&mut self) {
        let Some(engine) = &mut self.engine else {
            return;
        };
        if engine.run_due_guests() {
            self.after_dom_tasks();
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
    /// (`Dom::take_image_fetches`), fetched: those the engine's calls
    /// queued it sent already, in the image microtask.
    pub(super) fn start_image_fetches(&mut self) {
        // The engine's load sink sends them (`RendererLoadSink::
        // start_image_fetch`): those it hasn't yet, a background's that a
        // style update found among them.
        if let Some(engine) = &mut self.engine {
            engine.send_image_fetches();
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

    /// Raster panicked drawing `what` and left it out: a warning in the
    /// page's log, with the top-level document's URL, which the thread
    /// that drew doesn't know. (Printed with the log's other messages; the
    /// thread that drew logged what it knew as it happened.)
    pub(super) fn log_raster_failure(
        &mut self,
        what: super::log_agent::RasterFailed,
        failure: &raster::RasterFailure,
    ) {
        let url = self
            .top_document_url()
            .map(String::from)
            .unwrap_or_default();
        let url = url.as_str();
        self.log_agent
            .add(super::log_agent::LogEntry::raster_failed(
                what, failure, url,
            ));
    }

    /// The canvas drawing that failed, on the canvas raster thread or
    /// here, if one is to be told of: logged as a tile's is.
    pub(super) fn log_canvas_raster_failure(&mut self) {
        let failure = engine_frame(&mut self.engine)
            .and_then(|(_, frame)| frame.canvas_raster.cache().take_failure());
        if let Some(failure) = failure {
            self.log_raster_failure(super::log_agent::RasterFailed::Canvas, &failure);
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
            if response.head.mime_type == "image/svg+xml" {
                let (dom, frame) = engine_frame(&mut self.engine)?;
                let image = SvgImage::new(
                    dom,
                    &mut frame.token,
                    response.head.charset.as_deref(),
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
            Ok(response) if response.head.tainting == ResponseTainting::Opaque
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
        let (Some(engine), Some(next)) = (&mut self.engine, self.render_frames.top_document())
        else {
            return;
        };
        if let Some(results) = engine.take_harness_results() {
            let _ = self.to_browser.send(RendererHostMsg::TestharnessResults {
                navigation_id: next.navigation_id,
                results,
            });
        }
    }

    /// The `fetch()` numbered `fetch` went nowhere (a revoked blob URL): a
    /// network error, in a task of its own (`MainThreadTask::FetchFailed`).
    pub(super) fn fetch_went_nowhere(&mut self, fetch: u64) {
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
        let Some(engine) = &mut self.engine else {
            return;
        };
        let dom = engine.dom_mut();
        let fetches = dom.sent_fetches();
        if fetches.is_empty() {
            return;
        }
        let gone: Vec<u64> = dom.read(|cx| {
            fetches
                .iter()
                .filter(|(_, sent)| {
                    cx.node_by_id(sent.document)
                        .is_none_or(|document| cx.is_discarded(document))
                })
                .map(|(request_id, _)| *request_id)
                .collect()
        });
        for request_id in gone {
            dom.take_sent_fetch(request_id);
            cancel_request(&self.to_browser, &self.network_agent, request_id);
        }
    }

    /// The response to a `fetch()` of the DOM's, or its network error:
    /// its promise settled, as a task of its own (this one).
    fn fetch_answered(&mut self, id: u64, result: &LoadResult) {
        let response = crate::loader::fetched_response(result);
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
    ) -> bool {
        if let Some(sent) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.dom_mut().take_sent_fetch(request_id))
        {
            self.fetch_answered(sent.fetch, result);
            return true;
        }
        let image = self
            .engine
            .as_mut()
            .and_then(|engine| engine.dom_mut().take_sent_image(request_id));
        if let Some(dom::SentImage { url, document }) = image {
            if !self.image_loaded(&url, result) {
                let frame = self
                    .frame_of_document(document)
                    .or_else(|| self.render_frames.top());
                if let Some(frame) = frame {
                    self.note_failed(frame, failed_url(result, Some(&url)));
                }
            }
            return true;
        }
        // A script the DOM queued, or a `<link>`'s sheet: handed to the
        // engine's side, which decodes it and goes on with what waited for
        // it (`dom_api::document_loader`), from the frame showing its
        // document and in that document's encoding.
        let failed = match result {
            Ok(_) => String::new(),
            Err(error) => asked.map_or_else(|| error.detail.clone(), str::to_owned),
        };
        let response = crate::loader::subresource(result, &failed);
        let sent = self.engine.as_mut().and_then(|engine| {
            let dom = engine.dom_mut();
            dom.sent_script(request_id)
                .map(|script| (script.document, false))
                .or_else(|| {
                    dom.sent_sheet(request_id)
                        .map(|sheet| (sheet.document, true))
                })
        });
        let Some((document, sheet)) = sent else {
            return false;
        };
        let at = self
            .frame_of_document(document)
            .and_then(|frame| self.doc(frame).map(|next| (frame, next.encoding)));
        let Some(lent) = self.engine.take() else {
            return true;
        };
        let keep_text = !self.devtools_sessions.is_empty();
        let engine = if sheet {
            let (_, kept, engine) = self.parse_run(|run| {
                dom_api::document_loader::sheet_fetched(
                    lent, request_id, &response, at, keep_text, run,
                )
            });
            // Its text as it came, for DevTools (Chrome's from its cache);
            // kept only while a session is attached.
            if let Some((contents, css)) = kept {
                self.css_agent.keep_link_text(&contents, &css);
            }
            engine
        } else {
            self.parse_run(|run| {
                dom_api::document_loader::script_fetched(lent, request_id, &response, at, run)
            })
            .1
        };
        self.engine = Some(engine);
        self.send_harness_results();
        // A wasm script may have drawn.
        if self.engine_is_wasm {
            self.request_main_frame();
        }
        true
    }
}

/// DevTools' view of the DOM's loads: each request told to the
/// `Network` agent as it goes, from its frame, with who asked for it, while
/// a session has `Network` on (nothing is looked at otherwise); the
/// subresources each document loaded, for `Page.getResourceTree`, kept
/// whether or not DevTools is there.
impl RenderThread {
    /// Sends `request`, of `frame`'s document, told to DevTools' `Network`
    /// agent first, if a session has it on, as asked for by whom
    /// `initiator` finds then. Its id.
    fn send_request_with_initiator(
        &mut self,
        frame: FrameId,
        mut request: ResourceRequest,
        initiator: impl FnOnce(&mut Self) -> Option<Initiator>,
    ) -> u64 {
        let mode = self.subresource_cache_mode(frame);
        request.load_flags = crate::loader::subresource_load_flags(&request, mode);
        if self.network_agent.cache_disabled() {
            request.load_flags = crate::loader::cache_disabled_load_flags(request.load_flags);
        }
        let request_id = crate::loader::next_request_id();
        if self.network_agent.enabled() {
            let initiator = initiator(self);
            if let Some(at) = self.request_frame(frame) {
                self.network_agent
                    .will_send(request_id, &request, at, initiator.as_ref());
            }
        }
        let loader = self.loader_of(frame);
        self.send_request(request_id, loader, request);
        request_id
    }

    /// Sends `request` through `loader` (its document's).
    fn send_request(&mut self, request_id: u64, loader: u64, request: ResourceRequest) {
        self.request_urls.sent(request_id, &request);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            worker: None,
            loader,
            request,
        });
    }

    /// Where a request goes from, as DevTools names it: `frame` and its
    /// document's loader and URL. None before DevTools named the top-level
    /// frame, or once the document has gone.
    fn request_frame(&mut self, frame: FrameId) -> Option<RequestFrame> {
        let top = self.render_frames.top();
        let frame_id = if Some(frame) == top {
            self.render_frames.top_token()?.to_string()
        } else {
            self.frame_id(frame)
        };
        let loader_id = self.loader_id(frame)?;
        let document_url = self.document_url(frame)?.to_string();
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
        let document = self.doc(frame)?.document.clone();
        let engine = self.engine.as_mut()?;
        Some(engine.dom_mut().read(|cx| {
            let document = document.get(cx.gc);
            crate::loader::devtools_loader_id(cx.document_loader(document), cx.node_id(document))
        }))
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
        let image = self
            .engine
            .as_mut()
            .and_then(|engine| engine.dom_mut().sent_image(request_id));
        let found = if let Some(image) = image {
            self.frame_of_document(image.document)
                .map(|frame| (frame, "Image"))
        } else if let Some(script) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.dom_mut().sent_script(request_id))
        {
            self.frame_of_document(script.document)
                .map(|frame| (frame, "Script"))
        } else if let Some(sheet) = self
            .engine
            .as_mut()
            .and_then(|engine| engine.dom_mut().sent_sheet(request_id))
        {
            self.frame_of_document(sheet.document)
                .map(|frame| (frame, "Stylesheet"))
        } else {
            None
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
        let text = dom_api::subresource::stylesheet_text(
            &crate::loader::subresource(result, ""),
            quirks,
            encoding,
        );
        let base = result
            .as_ref()
            .ok()
            .and_then(|response| Url::parse(&response.head.url).ok())
            .map(|url| {
                let referrer = url.to_string();
                css::UrlContext::new(url, &referrer)
            });
        self.css_agent.refetched(key, text, base);
    }
}

/// What the DOM's engine tells of its child frames (`dom_api::FrameClient`).
pub(super) struct RendererFrameClient {
    pub(super) to_browser: BrowserSender,
    /// DevTools' agents, told of a frame as it comes and goes (Blink's
    /// `probe::FrameAttachedToParent`): observers on this thread, which
    /// the hook may hold as the load sink holds the `Network` one, asked
    /// whether a session is on before they do anything.
    pub(super) page_agent: Rc<super::page_agent::PageAgent>,
    pub(super) dom_agent: Rc<super::dom_agent::DomAgent>,
    /// For who asked for an iframe's navigation, as DevTools' `Network`
    /// has it.
    pub(super) network_agent: Rc<super::network_agent::NetworkAgent>,
    /// The render thread's scheduler: a navigation sent may commit once
    /// the task that asked for it is over (`InternalTask::
    /// NavigationCancellationWindowEnded`).
    pub(super) scheduler: std::sync::Arc<crate::task_queue::MainThreadScheduler>,
    /// The session history's entries the renderer told the browser of
    /// (`OwnHistoryEntries`): one more as a navigation within a document
    /// that adds one is sent.
    pub(super) own_history_entries: super::OwnHistoryEntries,
}

/// The engine's child frames told to the browser as they are made and go,
/// before the call returns (content's `RenderFrameImpl::CreateChildFrame`
/// and `FrameDetached` send `CreateChildFrame` and `Detach` in Blink's
/// call): a frame the browser has is one script has, and one gone is gone
/// for the browser before anything after its removal is.
impl dom_api::FrameClient for RendererFrameClient {
    fn title_changed(&mut self, title: &str) {
        let _ = self
            .to_browser
            .send(RendererHostMsg::UpdateTitle(title.to_owned()));
    }

    fn document_lifecycle(
        &mut self,
        frame: protocol::FrameToken,
        top: bool,
        document: dom::NodeId,
        loader: Option<dom::DocumentLoader>,
        event: dom_api::DocumentLifecycle,
    ) {
        let at = ipc::TimeTicks::now();
        let id = frame.to_string();
        // DevTools' names for the document, as the frames are told
        // (`main_frame_info`, `child_info`): the main frame's its
        // navigation's, a child frame's its number and its loader's.
        let (key, loader_id) = if top {
            let Some(navigation) = loader.and_then(|loader| loader.navigation) else {
                return;
            };
            (navigation, navigation.to_string())
        } else {
            (
                u64::from(document.get()),
                crate::loader::devtools_loader_id(loader, document),
            )
        };
        match event {
            dom_api::DocumentLifecycle::DomContentLoaded => {
                if top {
                    self.page_agent.dom_content_loaded();
                    self.dom_agent.dom_content_loaded();
                }
                self.page_agent
                    .lifecycle_event(&id, key, &loader_id, "DOMContentLoaded", at);
            }
            dom_api::DocumentLifecycle::Load => {
                if top {
                    self.page_agent.load_fired();
                }
                self.page_agent
                    .lifecycle_event(&id, key, &loader_id, "load", at);
                self.page_agent.stopped_loading(&id);
            }
        }
    }

    fn frame_detached(&mut self, frame: protocol::FrameToken) {
        let _ = self
            .to_browser
            .send(RendererHostMsg::DidDetachFrame { frame });
        self.page_agent.frame_detached(&frame.to_string());
    }

    fn child_frame_created(
        &mut self,
        parent: protocol::FrameToken,
        frame: protocol::FrameToken,
        name: &str,
        created_by_script: bool,
        stack: Option<&str>,
        dom: &dom::Dom,
    ) {
        let _ = self.to_browser.send(RendererHostMsg::DidCreateChildFrame {
            parent,
            frame,
            name: name.to_owned(),
            created_by_script,
        });
        // DevTools' queued events first (its iframe's `childNodeInserted`),
        // as Blink's `frameAttached` flushes the session's queue.
        if self.dom_agent.any_enabled() {
            self.dom_agent.take_changes(dom);
        }
        self.page_agent
            .frame_attached(frame.to_string(), parent.to_string(), stack);
    }

    fn child_document_created(&mut self, frame: protocol::FrameToken, document: dom::NodeId) {
        self.page_agent
            .child_document_created(&frame.to_string(), u64::from(document.get()));
    }

    /// What goes to the browser of script's navigation, sent as it is
    /// asked (content's `RenderFrameImpl::BeginNavigation`, from Blink's
    /// setter): one to another document and a reload (`BeginNavigation`,
    /// numbered here), one within the document, a traversal, a new tab,
    /// the scroll restoration mode. A child frame's to a fragment of its
    /// document or to a `javascript:` URL goes nowhere (the engine's DOM
    /// does those itself: `history::navigate`).
    fn navigate(&mut self, asked: &dom_api::NavigationAsked<'_>, _dom: &dom::Dom) -> Option<u64> {
        use dom::FrameNavigation;
        use protocol::renderer::BeginNavigationKind;
        let kind = match asked.navigation {
            FrameNavigation::To { url, replace } => BeginNavigationKind::To {
                url: url.clone(),
                replace: *replace,
            },
            FrameNavigation::Reload => BeginNavigationKind::Reload,
            FrameNavigation::SameDocument {
                url,
                replace,
                state,
                ..
            } => {
                // Every frame's: a child frame's URL is the browser's too
                // (what its reload loads).
                if let Some(frame) = asked.frame {
                    let _ =
                        self.to_browser
                            .send(RendererHostMsg::DidCommitSameDocumentNavigation {
                                frame,
                                url: url.clone(),
                                replace: *replace,
                                state: state.clone().map(protocol::BigBuffer::from),
                                navigation_id: None,
                            });
                    // A new entry the browser hears of.
                    if !*replace {
                        self.own_history_entries.sent_one();
                    }
                }
                return None;
            }
            // Whichever frame's link: the browser opens the tab.
            FrameNavigation::NewTab { url, background } => {
                let disposition = if *background {
                    protocol::renderer::WindowOpenDisposition::NewBackgroundTab
                } else {
                    protocol::renderer::WindowOpenDisposition::NewForegroundTab
                };
                let _ = self.to_browser.send(RendererHostMsg::OpenUrl {
                    url: url.clone(),
                    disposition,
                });
                return None;
            }
            FrameNavigation::GoToOffset(delta) => {
                let _ = self
                    .to_browser
                    .send(RendererHostMsg::GoToEntryAtOffset(*delta));
                return None;
            }
            FrameNavigation::ScrollRestoration { manual } => {
                if let Some(frame) = asked.frame {
                    let _ = self.to_browser.send(RendererHostMsg::UpdateState {
                        frame,
                        manual_scroll_restoration: *manual,
                    });
                }
                return None;
            }
        };
        // A reload is the frame's own.
        let initiator = match kind {
            BeginNavigationKind::Reload => asked.frame,
            BeginNavigationKind::To { .. } => asked.initiator,
        };
        let (Some(frame), Some(initiator)) = (asked.frame, initiator) else {
            tracing::warn!(?kind, "a navigation of a frame the browser doesn't know");
            return None;
        };
        let id = crate::loader::next_begin_navigation_id();
        let _ = self.to_browser.send(RendererHostMsg::BeginNavigation {
            frame,
            initiator,
            id,
            kind,
            devtools_initiator: None,
            history_in_new_child_frame: false,
            load_flags: protocol::network::LoadFlags::default(),
        });
        self.end_cancellation_window_after_task(id);
        Some(id)
    }
    /// An iframe's navigation, sent as it is asked: the browser hears of
    /// its `srcdoc` as it is now, which an `about:srcdoc` document is
    /// (Blink tells it as the frame is made and as the attribute changes,
    /// each of which navigates it); then the navigation (`BeginNavigation`),
    /// with who asked as DevTools' `Network` has it (the script that set
    /// the attribute or inserted the iframe, or the parser's line), unless
    /// it goes nowhere (a fragment of the frame's document, a `javascript:`
    /// URL: the render thread does it after the call).
    fn commit_here(&mut self, frame: protocol::FrameToken) {
        self.scheduler
            .post_internal(InternalTask::ChildCommit(frame));
    }

    fn navigate_child(
        &mut self,
        asked: &dom_api::ChildNavigationAsked<'_>,
        dom: &dom::Dom,
    ) -> Option<u64> {
        let srcdoc = match asked.navigation {
            ChildNavigation::Srcdoc(markup) => markup.clone(),
            ChildNavigation::Src(_) | ChildNavigation::AboutBlank => String::new(),
        };
        if let Some(frame) = asked.frame {
            let _ = self
                .to_browser
                .send(RendererHostMsg::DidChangeSrcDoc { frame, srcdoc });
        }
        // The engine does it (a fragment, a `javascript:` URL).
        if asked.here {
            return None;
        }
        let url = child_navigation_url(asked.navigation.clone());
        let kind = protocol::renderer::BeginNavigationKind::To {
            url,
            replace: false,
        };
        let (Some(frame), Some(initiator)) = (asked.frame, asked.parent) else {
            tracing::warn!(?kind, "a navigation of a frame the browser doesn't know");
            return None;
        };
        if asked.initial && self.page_agent.any_enabled() {
            let document = dom.read(|cx| u64::from(cx.node_id(asked.document.get(cx.gc)).get()));
            self.page_agent
                .initial_document_navigated_from(&frame.to_string(), document);
        }
        let devtools_initiator = self
            .network_agent
            .enabled()
            .then(|| self.iframe_initiator(asked, dom));
        let id = crate::loader::next_begin_navigation_id();
        let _ = self.to_browser.send(RendererHostMsg::BeginNavigation {
            frame,
            initiator,
            id,
            kind,
            devtools_initiator,
            history_in_new_child_frame: history_navigation_in_new_child_frame(
                asked.owner,
                asked.initial,
                dom,
            ),
            load_flags: child_navigation_load_flags(asked.owner, dom),
        });
        self.end_cancellation_window_after_task(id);
        Some(id)
    }
}

impl RendererFrameClient {
    /// Navigation `id`, just sent, may commit once the task that asked for
    /// it is over (`NavigationCancellationWindowEnded`; Blink posts
    /// `NotifyNavigationCancellationWindowEnded` to run after the task):
    /// after what the task asked before it went (its loads, the blob URLs
    /// it revoked, which go after the task), and with none the same task
    /// asked for after it (which takes its place) committing first.
    fn end_cancellation_window_after_task(&self, id: u64) {
        self.scheduler
            .post_internal(InternalTask::NavigationCancellationWindowEnded(id));
    }

    /// Who asked for an iframe's navigation, as DevTools'
    /// `Network.Initiator` (JSON): the script that asked, if `Network`
    /// wants stacks and script ran, else the line the parser made the
    /// iframe at, in its document.
    fn iframe_initiator(
        &self,
        asked: &dom_api::ChildNavigationAsked<'_>,
        dom: &dom::Dom,
    ) -> String {
        let stacks = self.network_agent.wants_stacks();
        let initiator = match asked.stack.filter(|_| stacks) {
            Some(stack) => Some(Initiator::Script(Rc::from(stack))),
            None => dom.read(|cx| {
                let owner = asked.owner.get(cx.gc);
                let line = cx.take_parser_load_line(owner)?;
                let url = cx.document_url(cx.document_of(owner))?.to_string();
                let url = url.split('#').next().unwrap_or_default().to_owned();
                Some(Initiator::Parser {
                    url,
                    line: line.saturating_sub(1),
                })
            }),
        };
        Initiator::to_json(initiator.as_ref()).to_string()
    }
}

/// The URL an iframe's navigation goes to: its `src`, `about:srcdoc` for
/// its `srcdoc`, or `about:blank`.
fn child_navigation_url(navigation: ChildNavigation) -> String {
    match navigation {
        ChildNavigation::Src(src) => src,
        ChildNavigation::Srcdoc(_) => "about:srcdoc".to_owned(),
        ChildNavigation::AboutBlank => "about:blank".to_owned(),
    }
}

/// How an iframe's navigation uses the HTTP cache by its parent's document's
/// load (`owner`'s, its iframe's document: Blink's `DetermineFrameCacheMode`
/// for the frame), until that one's `load`: past the cache in a reload that
/// bypassed it, stale or not back or forward.
fn child_navigation_load_flags(owner: &NodeRoot, dom: &dom::Dom) -> protocol::network::LoadFlags {
    let mode = dom.read(|cx| cx.subresource_cache_mode(cx.document_of(owner.get(cx.gc))));
    let mut flags = protocol::network::LoadFlags::default();
    match mode {
        dom::SubresourceCacheMode::Default => {}
        dom::SubresourceCacheMode::BypassCache => flags.bypass_cache = true,
        dom::SubresourceCacheMode::ForceCache => flags.skip_cache_validation = true,
    }
    flags
}

/// Whether an iframe's navigation of a frame that shows its initial
/// `about:blank`, `initial`, is the session history's (Blink's
/// `IsBackForwardNavigationInProgress`): its parent's document, its
/// iframe's (`owner`'s), was back or forward loaded and hasn't had its
/// `load` yet. A frame script makes after isn't.
fn history_navigation_in_new_child_frame(owner: &NodeRoot, initial: bool, dom: &dom::Dom) -> bool {
    initial
        && dom.read(|cx| {
            let parent = cx.document_of(owner.get(cx.gc));
            let history_load = cx.window_of(parent).is_some_and(|window| {
                cx.navigation_timing(window).is_some_and(|timing| {
                    timing.navigation_type == dom::NavigationType::BackForward
                })
            });
            history_load && !cx.is_completely_loaded(parent)
        })
}

impl RendererLoadSink {
    /// `request`, of `document`, sent as `request_id` through its loader,
    /// told to DevTools as it is: who asked, the script's stack if it wants
    /// stacks, else the parser's line, else the stylesheet or the importing
    /// module.
    fn send(
        &self,
        request_id: u64,
        document: dom::NodeId,
        mut request: ResourceRequest,
        origin: &dom_api::FetchOrigin,
    ) {
        request.load_flags = crate::loader::subresource_load_flags(&request, origin.cache_mode);
        if self.network_agent.cache_disabled() {
            request.load_flags = crate::loader::cache_disabled_load_flags(request.load_flags);
        }
        if self.network_agent.enabled()
            && let (Some(frame_id), Some(document_url)) =
                (origin.frame_token.clone(), origin.document_url.clone())
        {
            let initiator = if let Some(stack) = origin
                .stack
                .clone()
                .filter(|_| self.network_agent.wants_stacks())
            {
                Some(Initiator::Script(stack))
            } else if let Some(line) = origin.parser_line {
                Some(Initiator::Parser {
                    url: document_url
                        .split('#')
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                    line: line.saturating_sub(1),
                })
            } else if let Some(sheet) = &origin.stylesheet {
                Some(Initiator::Css(sheet.clone()))
            } else {
                origin.importer.clone().map(Initiator::Import)
            };
            let at = super::network_agent::RequestFrame {
                frame_id,
                loader_id: crate::loader::devtools_loader_id(origin.loader, document),
                document_url,
            };
            self.network_agent
                .will_send(request_id, &request, at, initiator.as_ref());
        }
        let loader = origin.loader.map_or(0, |loader| loader.loader);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            worker: None,
            loader,
            request,
        });
    }
}

/// What the DOM's engine sends as script asks (`dom_api::LoadSink`): a
/// `fetch()` at the call, told to DevTools as it goes, and cancelled at
/// once; a blob URL registered and revoked at once (Blink's `FetchManager`
/// starts its loader in the call). The engine records the `fetch()`es sent
/// (`Dom::note_fetch_sent`), which the render thread gives their responses
/// back by.
pub(super) struct RendererLoadSink {
    pub(super) to_browser: BrowserSender,
    /// Where a `fetch()` that went nowhere gets its network error, in a
    /// task of its own (`MainThreadTask::FetchFailed`).
    pub(super) scheduler: std::sync::Arc<crate::task_queue::MainThreadScheduler>,
    pub(super) blob_urls: crate::loader::BlobUrls,
    /// Told of each `fetch()` as it is sent and cancelled. Shared (`Rc`):
    /// DevTools' agents are the render thread's, observers of what the
    /// page does as it does it (Blink's are its probes' `GarbageCollected`
    /// objects); it asks whether it is on before anything else.
    pub(super) network_agent: Rc<super::network_agent::NetworkAgent>,
}

impl dom_api::LoadSink for RendererLoadSink {
    fn start_fetch(
        &mut self,
        _: FrameId,
        request: &dom::FetchRequest,
        origin: &dom_api::FetchOrigin,
    ) -> Option<u64> {
        let document = request.document?;
        if fetch_of_revoked_blob(&self.blob_urls, request) {
            self.scheduler.post_fetch_failed(request.id);
            return None;
        }
        let mut resource = crate::loader::fetch_resource_request(request);
        resource.load_flags = crate::loader::subresource_load_flags(&resource, origin.cache_mode);
        if self.network_agent.cache_disabled() {
            resource.load_flags = crate::loader::cache_disabled_load_flags(resource.load_flags);
        }
        let request_id = crate::loader::next_request_id();
        // Told to DevTools as it is sent, before anything of its response
        // can come (Blink's `ResourceLoadObserverForFrame::WillSendRequest`).
        if self.network_agent.enabled()
            && let (Some(frame_id), Some(document_url)) =
                (origin.frame_token.clone(), origin.document_url.clone())
        {
            let initiator = origin
                .stack
                .clone()
                .filter(|_| self.network_agent.wants_stacks())
                .map(Initiator::Script);
            let at = super::network_agent::RequestFrame {
                frame_id,
                loader_id: crate::loader::devtools_loader_id(origin.loader, document),
                document_url,
            };
            self.network_agent
                .will_send(request_id, &resource, at, initiator.as_ref());
        }
        // Through its document's loader: 0, which the browser knows as
        // none, for a document it gave none.
        let loader = origin.loader.map_or(0, |loader| loader.loader);
        let _ = self.to_browser.send(RendererHostMsg::FetchResource {
            request_id,
            worker: None,
            loader,
            request: resource,
        });
        Some(request_id)
    }

    fn cancel_fetch(&mut self, request_id: u64) {
        cancel_request(&self.to_browser, &self.network_agent, request_id);
    }

    fn start_image_fetch(
        &mut self,
        _: FrameId,
        document: dom::NodeId,
        url: &str,
        request_origin: Option<&str>,
        origin: &dom_api::FetchOrigin,
    ) -> Option<u64> {
        // A blob URL revoked before the element resolved it: a network
        // error, without asking.
        if let Ok(parsed) = Url::parse(url)
            && parsed.scheme() == "blob"
            && self.blob_urls.is_revoked(&parsed)
        {
            return None;
        }
        let mut request = ResourceRequest::new(url, RequestDestination::Image);
        request.initiator = request_origin.map(String::from);
        let request_id = crate::loader::next_request_id();
        self.send(request_id, document, request, origin);
        Some(request_id)
    }

    fn start_script_fetch(
        &mut self,
        _: FrameId,
        document: dom::NodeId,
        script: &dom::ScriptRequest,
        request_origin: Option<&str>,
        origin: &dom_api::FetchOrigin,
    ) -> Option<u64> {
        let request_id = crate::loader::next_request_id();
        // A blob URL revoked before (the browser hears of it only as the
        // loop turns): its network error, as the browser's would have
        // been, in a task of its own.
        if let Ok(parsed) = Url::parse(&script.url)
            && parsed.scheme() == "blob"
            && self.blob_urls.is_revoked(&parsed)
        {
            self.scheduler.post_request_failed(request_id, &script.url);
            return Some(request_id);
        }
        let mut request = ResourceRequest::new(&script.url, RequestDestination::Script);
        request.initiator = request_origin.map(String::from);
        // A module's (and a `crossorigin` one's) by CORS, its credentials
        // mode the module map's or the attribute's.
        if let Some(cors) = script.cors {
            request.mode = RequestMode::Cors;
            request.credentials = crate::loader::script_credentials(cors);
        }
        self.send(request_id, document, request, origin);
        Some(request_id)
    }

    fn start_sheet_fetch(
        &mut self,
        _: FrameId,
        document: dom::NodeId,
        url: &str,
        cors: Option<dom::ScriptCors>,
        request_origin: Option<&str>,
        origin: &dom_api::FetchOrigin,
    ) -> Option<u64> {
        let mut request = ResourceRequest::new(url, RequestDestination::Style);
        request.initiator = request_origin.map(String::from);
        // By CORS, which an origin that allows it makes origin-clean (and
        // one that doesn't, a network error).
        if let Some(cors) = cors {
            request.mode = RequestMode::Cors;
            request.credentials = crate::loader::script_credentials(cors);
        }
        let request_id = crate::loader::next_request_id();
        self.send(request_id, document, request, origin);
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

/// Every `fetch()` of `dom`'s documents still on its way cancelled, in the
/// order they were sent: the documents all go (the top-level one is
/// replaced; Blink's `FetchManager` cancels its loaders as its execution
/// context is destroyed).
fn cancel_sent_fetches(
    dom: &Dom,
    to_browser: &BrowserSender,
    network_agent: &super::network_agent::NetworkAgent,
) {
    let mut sent: Vec<u64> = dom
        .sent_fetches()
        .into_iter()
        .map(|(request_id, _)| request_id)
        .collect();
    sent.sort_unstable();
    for request_id in sent {
        dom.take_sent_fetch(request_id);
        cancel_request(to_browser, network_agent, request_id);
    }
}

/// The renderer's request `request_id` cancelled: DevTools told, the
/// browser's load stopped.
fn cancel_request(
    to_browser: &BrowserSender,
    network_agent: &super::network_agent::NetworkAgent,
    request_id: u64,
) {
    network_agent.did_cancel(request_id);
    let _ = to_browser.send(RendererHostMsg::CancelResource {
        request_id,
        worker: None,
    });
}

/// What opens the frames' `localStorage` and tells the browser what script
/// changes in it (Blink's `StorageController` and its areas' pipes to the
/// browser's `StorageArea`s): an area opened is the answer the main thread
/// waits for, every task left queued (`[Sync] GetAll`); a change is sent
/// as it is made (`Put`, `Delete`, `DeleteAll`).
pub(super) struct RendererStorageHost {
    pub(super) to_browser: BrowserSender,
    pub(super) scheduler: std::sync::Arc<crate::task_queue::MainThreadScheduler>,
}

impl dom_api::StorageHost for RendererStorageHost {
    fn open_storage_area(
        &self,
        frame: protocol::FrameToken,
        kind: dom::StorageKind,
    ) -> Option<dom_api::OpenedStorageArea> {
        let kind = match kind {
            dom::StorageKind::Local => protocol::renderer::StorageKind::Local,
            dom::StorageKind::Session => protocol::renderer::StorageKind::Session,
        };
        let reply = self.scheduler.sync_call(|request_id| {
            let _ = self.to_browser.send(RendererHostMsg::OpenStorageArea {
                frame,
                kind,
                request_id,
            });
        })?;
        match reply {
            protocol::renderer::RendererMsg::StorageAreaOpened { area, .. } => area,
            _ => None,
        }
    }

    fn storage_changed(&self, change: dom::StorageChange) {
        let mutation = match change.mutation {
            dom::StorageMutation::Set { key, value } => {
                protocol::renderer::StorageChange::Set { key, value }
            }
            dom::StorageMutation::Remove { key } => {
                protocol::renderer::StorageChange::Remove { key }
            }
            dom::StorageMutation::Clear => protocol::renderer::StorageChange::Clear,
        };
        let _ = self.to_browser.send(RendererHostMsg::ChangeStorage {
            area: change.area,
            change: mutation,
            source: change.source,
            url: change.url,
        });
    }
}

/// What reads and writes the frames' cookies (Blink's `CookieJar` and the
/// frame's `RestrictedCookieManager`), through each document's loader: a
/// read is the answer the main thread waits for, every task left queued
/// (`[Sync] GetCookiesString`); a write is sent as the engine hands it
/// (`SetCookieFromString`). The engine gives the document's loader.
pub(super) struct RendererCookieHost {
    pub(super) to_browser: BrowserSender,
    pub(super) scheduler: std::sync::Arc<crate::task_queue::MainThreadScheduler>,
}

impl dom_api::CookieHost for RendererCookieHost {
    fn cookies(&self, loader: u64, url: &str) -> String {
        let reply = self.scheduler.sync_call(|request_id| {
            let _ = self.to_browser.send(RendererHostMsg::GetCookies {
                loader,
                url: url.to_owned(),
                request_id,
            });
        });
        match reply {
            Some(protocol::renderer::RendererMsg::Cookies { cookies, .. }) => cookies,
            _ => String::new(),
        }
    }

    fn set_cookie(&self, loader: u64, url: &str, line: &str) {
        let _ = self.to_browser.send(RendererHostMsg::SetCookie {
            loader,
            url: url.to_owned(),
            line: line.to_owned(),
        });
    }

    fn cookies_enabled(&self, loader: u64, url: &str) -> bool {
        let reply = self.scheduler.sync_call(|request_id| {
            let _ = self.to_browser.send(RendererHostMsg::CookiesEnabled {
                loader,
                url: url.to_owned(),
                request_id,
            });
        });
        matches!(
            reply,
            Some(protocol::renderer::RendererMsg::CookiesEnabled { enabled: true, .. })
        )
    }
}

/// Whether `request` is of a blob URL revoked before the element or the
/// call resolved it (in its task, before the browser heard): a network
/// error, without asking.
fn fetch_of_revoked_blob(blob_urls: &crate::loader::BlobUrls, request: &dom::FetchRequest) -> bool {
    Url::parse(&request.url).is_ok_and(|url| url.scheme() == "blob" && blob_urls.is_revoked(&url))
}

/// What a subresource that failed is named by, in `DidFinishLoad`: where
/// its response came from (after redirects), or the URL asked for (`asked`)
/// if none came.
fn failed_url(result: &LoadResult, asked: Option<&str>) -> String {
    match result {
        Ok(response) => response.head.url.clone(),
        Err(error) => asked.map_or_else(|| error.detail.clone(), str::to_owned),
    }
}

/// Whether a `<link>`'s sheet that blocks `document`'s rendering is
/// loading (the DOM's, `Dom::sheets_block_render`).
fn sheets_block_render(dom: &Dom, document: &NodeRoot) -> bool {
    dom.sheets_block_render(dom.read(|cx| cx.node_id(document.get(cx.gc))))
}

/// Whether a `<link>`'s sheet that blocks `document`'s parser's scripts is
/// loading, though script may have taken its `<link>` away
/// (`Dom::sheets_pending_for_scripts`).
fn sheets_pending_for_scripts(dom: &Dom, document: &NodeRoot) -> bool {
    dom.sheets_pending_for_scripts(dom.read(|cx| cx.node_id(document.get(cx.gc))))
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
/// time (`computed`). In how many milliseconds the first held back is
/// due. The entries' task is the DOM's to queue (`Cx::queue_intersection_entry`).
fn compute_intersections(
    dom: &mut Dom,
    frame: &mut Frame,
    documents: &[gfx::NodeId],
    blocked: &[NodeRoot],
    computed: &mut Option<(u64, u64, Option<paint::PaintId>)>,
) -> Option<f64> {
    let view = &frame.view;
    let document = view.document().clone();
    dom.mutate(|cx| {
        let top = document.get(cx.gc);
        cx.window_of(top)?;
        let tracked = computed.is_some_and(|(.., paint)| paint.is_some());
        let state = (
            view.pre_paints(),
            cx.intersection_version(),
            view.last_paint().filter(|_| tracked),
        );
        if *computed == Some(state) {
            return None;
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
            return due;
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
        // The DOM queues the task that delivers what they bring.
        for ((_, window, now), results) in documents.iter().zip(&by_window) {
            if !results.is_empty() {
                dom_api::update_intersections(&dom_api::ApiCx::new(cx), *window, results, *now);
            }
        }
        due
    })
}
