//! The DOM's engine for a frame (`dom_api::ScriptEngine`) on V8:
//! an isolate of its own, one realm at a time whose global is a window of
//! a new document (`create_window_realm`), its classic scripts and
//! microtasks; with web tests, a `testRunner` on the window (Blink's
//! `WebTestControlHost`'s `testRunner`, its `reportHarnessResults`).

use std::time::Instant;

use bindings::V8PerIsolateData;
use dom::{Dom, NodeId, NodeRoot, Window};
use dom_api::{
    ClassicScript, ConsoleMessage, ConsoleSink, DevToolsFrameIds, DevToolsInterrupter,
    DevToolsQueue, DevToolsSend, DocumentReadiness, EngineSettings, ScriptEngine,
};
use elise::Persistent;

use crate::listeners::{Host, ReportAt, report_caught};
use crate::state::{self, State};
use crate::templates::string;
use crate::window::create_window_realm;

/// V8 as the DOM's engine.
pub struct V8Engine {
    settings: EngineSettings,
    /// A context of no window, to make the first realm in (a realm is
    /// made in some context of its isolate).
    bootstrap: v8::Global<v8::Context>,
    /// The window's realm and its document, once there is one.
    context: Option<v8::Global<v8::Context>>,
    document: Option<NodeRoot>,
    /// The window's mouse state (a new one with each window).
    event_handler: dom_api::EventHandler,
    /// Whether the page has the system's focus (the host's): each new
    /// window starts with it.
    page_focused: bool,
    /// The page is hidden (`set_page_visible`): each window made takes it.
    page_hidden: bool,
    /// The documents of the rendering update that runs
    /// (`begin_rendering_update`), in tree order: none outside one.
    rendering_documents: Option<Vec<NodeId>>,
    /// The isolate, and in its state the DOM's heap: last, as fields drop
    /// in order, so that the roots above go first (a root never outlives
    /// its heap) and the `Global`s before their isolate.
    data: V8PerIsolateData,
}

impl V8Engine {
    pub fn new(settings: EngineSettings) -> Self {
        let mut data = V8PerIsolateData::new();
        // The isolate's DOM, made with the engine.
        state::install(data.isolate());
        let bootstrap = {
            let isolate = data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Context::new(scope, Default::default());
            v8::Global::new(scope, context)
        };
        Self {
            data,
            settings,
            bootstrap,
            context: None,
            document: None,
            event_handler: dom_api::EventHandler::default(),
            page_focused: false,
            page_hidden: false,
            rendering_documents: None,
        }
    }

    /// For whoever starts a renderer: its new DOM's engine.
    pub fn boxed(settings: EngineSettings) -> Box<dyn ScriptEngine> {
        Box::new(Self::new(settings))
    }

    fn state(&mut self) -> &mut State {
        bindings::isolate_state_mut::<State>(self.data.isolate()).expect("the engine's DOM")
    }

    /// `frame`'s realm, if it is one of the engine's.
    fn frame_context(&mut self, frame: dom_api::FrameId) -> Option<v8::Global<v8::Context>> {
        Some(self.state().frames.get(frame)?.context.clone())
    }

    fn frame_context_and_document(
        &mut self,
        frame: dom_api::FrameId,
    ) -> Option<(v8::Global<v8::Context>, NodeRoot)> {
        let frame = self.state().frames.get(frame)?;
        Some((frame.context.clone(), frame.document.clone()))
    }

    /// The realm of the frame showing `node`'s document: the top-level
    /// one's for a node of none.
    fn context_of_node(&mut self, node: &NodeRoot) -> Option<v8::Global<v8::Context>> {
        let state = self.state();
        let document = state
            .dom
            .read(|cx| cx.node_id(cx.document_of(node.get(cx.gc))));
        match state.frames.of_document(document) {
            Some(frame) => Some(frame.context.clone()),
            None => self.context.clone(),
        }
    }

    /// Whether a document at `url` in a child frame of `parent` is of the
    /// origin its initial `about:blank` has, its parent's: an `about:` one
    /// (`about:srcdoc`, `about:blank`), or one of the same tuple origin.
    fn inherits_origin(&mut self, parent: Option<dom_api::FrameId>, url: &str) -> bool {
        if url.starts_with("about:") {
            return true;
        }
        let Some(parent) = parent.and_then(|parent| self.state().frames.get(parent)) else {
            return false;
        };
        let document = parent.document.clone();
        let parent = self
            .state()
            .dom
            .read(|cx| dom_api::origin(cx, document.get(cx.gc)));
        let origin = url::Url::parse(url).map(|url| url.origin());
        matches!((parent, origin), (Some(parent), Ok(origin)) if parent.is_tuple() && parent == origin)
    }

    /// `create_window` for a child frame whose initial `about:blank`'s
    /// window stays (`inherits_origin`): a new document at `url` is that
    /// window's (`dom`'s `reuse_window`), in the same realm, loading,
    /// its iframe's content document.
    fn reuse_initial_window(
        &mut self,
        frame: dom_api::FrameId,
        context: &v8::Global<v8::Context>,
        url: &str,
        time_origin: Instant,
        parent: Option<dom_api::FrameId>,
        owner: &NodeRoot,
    ) -> NodeRoot {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let local = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, local);
        let global = local.global(scope);
        let mut scope = state::scope(scope).expect("the engine's DOM");
        let (state, mut no_gc) = scope.state_mut();
        let (document, document_id) = state.dom.mutate(|cx| {
            let window = no_gc
                .unwrap::<Window>(cx.gc, global)
                .expect("a realm's window");
            let document = cx.create_document(true);
            cx.set_document_url(document, url);
            cx.reuse_window(window, document);
            cx.set_window_time_origin(window, time_origin);
            cx.set_ready_state(document, dom::DocumentReadyState::Loading);
            cx.set_content_document(owner.get(cx.gc), Some(document));
            (cx.root(document), cx.node_id(document))
        });
        state.frames.set(crate::frames::Frame::new(
            frame,
            context.clone(),
            document.clone(),
            document_id,
            parent,
            Some(owner.clone()),
            false,
        ));
        // The global's `document` is the new one now.
        crate::window::update_cached_accessors(&mut scope, global);
        document
    }

    /// The frames in tree order (`ScriptEngine::frames_in_tree_order`), with
    /// their realms and documents: the documents of the top-level frame's
    /// tree that a frame shows.
    fn frames_with_contexts(
        &mut self,
    ) -> Vec<(dom_api::FrameId, v8::Global<v8::Context>, NodeRoot)> {
        let documents = match &self.rendering_documents {
            Some(documents) => documents.clone(),
            None => self.walk_documents(),
        };
        let state = self.state();
        documents
            .into_iter()
            .filter_map(|document| {
                let frame = state.frames.of_document(document)?;
                Some((frame.id, frame.context.clone(), frame.document.clone()))
            })
            .collect()
    }

    /// The top-level frame's tree's documents in tree order, found by
    /// walking each document's tree for its iframes.
    fn walk_documents(&mut self) -> Vec<NodeId> {
        let state = self.state();
        let Some(top) = state.frames.get(crate::frames::TOP) else {
            return Vec::new();
        };
        let top = top.document.clone();
        state.dom.read(|cx| {
            dom_api::documents_in_tree_order(cx, top.get(cx.gc))
                .into_iter()
                .map(|document| cx.node_id(document))
                .collect()
        })
    }

    /// The frames as `Frames::iter` gives them (no tree walked: what each
    /// turn of the host's loop asks), with their realms and documents.
    fn frames_as_made(&mut self) -> Vec<(dom_api::FrameId, v8::Global<v8::Context>, NodeRoot)> {
        self.state()
            .frames
            .iter()
            .map(|frame| (frame.id, frame.context.clone(), frame.document.clone()))
            .collect()
    }

    /// Whether `test` holds of a frame's window.
    fn any_window(
        &mut self,
        test: impl for<'gc> Fn(&dom::Cx<'_, 'gc>, elise::Gc<'gc, dom::Window>) -> bool,
    ) -> bool {
        let frames = self.frames_as_made();
        let dom = &self.state().dom;
        dom.read(|cx| {
            frames.iter().any(|(_, _, document)| {
                cx.window_of(document.get(cx.gc))
                    .is_some_and(|window| test(cx, window))
            })
        })
    }

    /// `f` in `context`, entered.
    fn in_context<R>(
        &mut self,
        context: &v8::Global<v8::Context>,
        f: impl FnOnce(&mut v8::PinScope<'_, '_>) -> R,
    ) -> R {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        f(scope)
    }

    /// The frame whose window's next timer is due first by the clock, of
    /// those due by their window's now as the timers began to run
    /// (`nows`, each window's taken the first time it is looked at: an
    /// interval due again after it waits for the next turn), with that now.
    /// Those due at once, the first frame made's.
    fn earliest_due_timer(
        &mut self,
        nows: &mut Vec<(dom_api::FrameId, f64)>,
    ) -> Option<(v8::Global<v8::Context>, f64)> {
        let frames = self.frames_as_made();
        let dom = &self.state().dom;
        let mut best: Option<(f64, v8::Global<v8::Context>, f64)> = None;
        for (frame, context, document) in frames {
            let found = dom.read(|cx| {
                let window = cx.window_of(document.get(cx.gc))?;
                let now = match nows.iter().find(|(id, _)| *id == frame) {
                    Some(&(_, now)) => now,
                    None => {
                        let now = cx.now_since(Some(window));
                        nows.push((frame, now));
                        now
                    }
                };
                let due = cx.next_due(window).filter(|due| *due <= now)?;
                // On the DOM's clock: windows' time origins differ.
                Some((cx.window_time_origin(window) + due, now))
            });
            if let Some((at, now)) = found
                && best.as_ref().is_none_or(|(best, ..)| at < *best)
            {
                best = Some((at, context, now));
            }
        }
        best.map(|(_, context, now)| (context, now))
    }
}

/// The document's parser's host: its DOM, and the custom elements it
/// constructs at once (HTML's "create an element for a token" that will
/// execute script: the parser runs as a task, the JS stack empty).
impl dom_api::ParserHost for V8Engine {
    fn parser_dom(&mut self) -> &mut Dom {
        self.dom_mut()
    }

    fn create_custom_element(
        &mut self,
        document: &NodeRoot,
        local: &str,
        attributes: &[(dom::QualName<'_>, &str)],
    ) -> Option<NodeRoot> {
        let definition = self.dom_mut().read(|cx| {
            cx.custom_element_definition_in(document.get(cx.gc), local)
                .map(|definition| Persistent::new(cx.gc, definition))
        })?;
        // The JS stack is empty: the microtasks first.
        self.perform_microtask_checkpoint();
        let context = self.context_of_node(document)?;
        let context = &context;
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut scope = state::scope(scope)?;
        scope.state_mut().0.dom.mutate(|cx| cx.enter_ce_reactions());
        let made = crate::listeners::run_constructor(&mut scope, |scope| {
            crate::front::construct_custom_element(scope, &definition, document)
        });
        let (element, left) = {
            let (state, mut no_gc) = scope.state_mut();
            state.dom.mutate(|cx| {
                let element = made.and_then(|made| no_gc.unwrap::<dom::Node>(cx.gc, made));
                if let Some(element) = element {
                    for (name, value) in attributes {
                        cx.set_attribute_ns(element, *name, value);
                    }
                }
                (
                    element.map(|element| cx.root(element)),
                    cx.leave_ce_reactions(),
                )
            })
        };
        if left {
            dom_api::invoke_left_ce_reactions(&mut Host { scope: &mut scope });
        }
        state::report(&mut scope);
        element
    }

    fn run_custom_element_reactions(&mut self) {
        let Some(context) = &self.context else {
            return;
        };
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let Some(mut scope) = state::scope(scope) else {
            return;
        };
        // The iframes' and scripts' steps first, as a member's call does
        // them.
        if scope.state().0.dom.read(|cx| cx.has_element_changes()) {
            state::update_elements(&mut scope);
        }
        dom_api::invoke_left_ce_reactions(&mut Host { scope: &mut scope });
        state::report(&mut scope);
    }
}

impl ScriptEngine for V8Engine {
    fn dom_mut(&mut self) -> &mut Dom {
        &mut bindings::isolate_state_mut::<State>(self.data.isolate())
            .expect("the engine's DOM")
            .dom
    }

    fn set_layout(&mut self, layout: Box<dyn dom_api::FrameLayout>) {
        bindings::isolate_state_mut::<State>(self.data.isolate())
            .expect("the engine's DOM")
            .layout
            .set(Some(layout));
    }

    fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn dom_api::FrameLayout>) {
        let state =
            bindings::isolate_state_mut::<State>(self.data.isolate()).expect("the engine's DOM");
        (&mut state.dom, state.layout.get_mut().as_deref_mut())
    }

    fn top_frame(&self) -> dom_api::FrameId {
        crate::frames::TOP
    }

    fn set_navigation_timing(&mut self, frame: dom_api::FrameId, timing: dom::NavigationTiming) {
        let Some((_, document)) = self.frame_context_and_document(frame) else {
            return;
        };
        self.dom_mut().mutate(|cx| {
            if let Some(window) = cx.window_of(document.get(cx.gc)) {
                cx.set_navigation_timing(window, timing);
            }
        });
    }

    fn create_window(
        &mut self,
        frame: dom_api::FrameId,
        url: &str,
        time_origin: Instant,
    ) -> Option<NodeRoot> {
        // HTML's: a child frame's initial `about:blank` navigated to a
        // document of its origin keeps its window, and what script put on
        // it (Blink reuses the `LocalDOMWindow`).
        let initial = self
            .state()
            .frames
            .get(frame)
            .filter(|known| known.initial)
            .and_then(|known| Some((known.parent, known.owner.clone()?, known.context.clone())));
        if let Some((parent, owner, context)) = initial
            && self.inherits_origin(parent, url)
        {
            return Some(self.reuse_initial_window(
                frame,
                &context,
                url,
                time_origin,
                parent,
                &owner,
            ));
        }
        let web_tests = self.settings.web_tests;
        let page_focused = self.page_focused;
        let page_hidden = self.page_hidden;
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let bootstrap = v8::Local::new(scope, &self.bootstrap);
        let scope = &mut v8::ContextScope::new(scope, bootstrap);
        // A child frame's place: its parent and iframe, as they were.
        let place = if frame == crate::frames::TOP {
            None
        } else {
            let mut state_scope = state::scope(scope).expect("the engine's DOM");
            let (state, _) = state_scope.state();
            let frame = state.frames.get(frame)?;
            Some((frame.parent, frame.owner.clone()))
        };
        // The frame's realm before, if it has one: it navigates.
        let before = {
            let mut state_scope = state::scope(scope).expect("the engine's DOM");
            let (state, _) = state_scope.state();
            state.frames.get(frame).map(|frame| frame.context.clone())
        };
        // A child frame that navigates: the frames of its document before
        // go with it.
        if place.is_some() && before.is_some() {
            let mut state_scope = state::scope(scope).expect("the engine's DOM");
            crate::frames::discard_descendants(&mut state_scope, frame);
        }
        // HTML's `WindowProxy`: the frame's global proxy goes on, the
        // realm before detached from it (Blink's `LocalWindowProxy`).
        let proxy = before.as_ref().map(|context| {
            let context = v8::Local::new(scope, context);
            crate::window_properties::detached(
                &mut state::scope(scope).expect("the engine's DOM"),
                context,
            );
            let proxy = context.global(scope);
            context.detach_global();
            proxy
        });
        // The top-level frame navigates: every realm has gone for the
        // inspector, before it hears of the new one (Blink's
        // `MainThreadDebugger::DidClearContextsForFrame`).
        if place.is_none() && before.is_some() {
            crate::devtools::realms_cleared(scope);
        }
        let context = create_window_realm(scope, url, proxy);
        {
            let mut state_scope = state::scope(scope).expect("the engine's DOM");
            let parent = place
                .as_ref()
                .and_then(|(parent, _)| *parent)
                .and_then(|parent| {
                    let (state, _) = state_scope.state();
                    state.frames.get(parent).map(|frame| frame.context.clone())
                });
            crate::frames::set_security_token(&mut state_scope, context, url, parent.as_ref());
        }
        let global = context.global(scope);
        let (document, document_id) = {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let (state, mut no_gc) = scope.state_mut();
            state.dom.mutate(|cx| {
                let window = no_gc
                    .unwrap::<Window>(cx.gc, global)
                    .expect("a realm's window");
                cx.set_window_time_origin(window, time_origin);
                // No element has the focus yet: no events.
                cx.set_page_focused(window, page_focused);
                cx.set_page_hidden(window, page_hidden);
                // The parser's: loading until it is parsed.
                let document = cx.window_document(window);
                cx.set_ready_state(document, dom::DocumentReadyState::Loading);
                if let Some((_, Some(owner))) = &place {
                    cx.set_content_document(owner.get(cx.gc), Some(document));
                }
                (cx.root(document), cx.node_id(document))
            })
        };
        {
            let scope = &mut v8::ContextScope::new(scope, context);
            // A child frame's realm before has gone for the inspector, and
            // it hears of the new one (Blink's
            // `MainThreadDebugger::ContextCreated`): V8's `console` reports
            // through it.
            if let Some(before) = before.as_ref().filter(|_| place.is_some()) {
                crate::devtools::realm_destroyed(scope, before);
            }
            crate::devtools::realm_created(
                &mut state::scope(scope).expect("the engine's DOM"),
                context,
                frame,
                &document,
            );
            if web_tests && place.is_none() {
                install_test_runner(scope, global);
            }
        }
        let global_context = v8::Global::new(scope, context);
        {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let (parent, owner) = place.clone().unwrap_or((None, None));
            if place.is_none() {
                scope.state_mut().0.frames.clear_children();
            }
            scope.state_mut().0.frames.set(crate::frames::Frame::new(
                frame,
                global_context.clone(),
                document.clone(),
                document_id,
                parent,
                owner,
                false,
            ));
        }
        if place.is_none() {
            self.context = Some(global_context);
            self.document = Some(document.clone());
            self.event_handler = dom_api::EventHandler::default();
        }
        Some(document)
    }

    fn take_frame_requests(&mut self) -> Vec<dom_api::FrameRequest> {
        let state = self.state();
        // The `fetch()`s since: their documents' frames' requests now.
        let fetches = state.dom.mutate(|cx| cx.take_fetch_requests());
        for fetch in fetches {
            let request = match fetch {
                dom::FetchHostRequest::Start(request) => {
                    let Some(frame) = state
                        .frames
                        .of_document(request.document)
                        .map(|frame| frame.id)
                    else {
                        continue;
                    };
                    dom_api::FrameRequest::Fetch { frame, request }
                }
                dom::FetchHostRequest::Cancel(id) => dom_api::FrameRequest::CancelFetch { id },
            };
            state.frames.requests.push(request);
        }
        std::mem::take(&mut self.state().frames.requests)
    }

    fn fetch_response(&mut self, id: u64, response: Result<dom_api::FetchedResponse, ()>) -> bool {
        // The window whose fetch it is.
        let frames = self.frames_as_made();
        let found = {
            let dom = &self.state().dom;
            frames.into_iter().find_map(|(_, context, document)| {
                dom.read(|cx| {
                    let window = cx.window_of(document.get(cx.gc))?;
                    cx.window_fetch(window, id).map(|_| ())
                })
                .map(|()| context)
            })
        };
        let Some(context) = found else {
            return false;
        };
        let mut response = Some(response);
        self.in_context(&context, |scope| {
            let context = scope.get_current_context();
            if let Some(response) = response.take() {
                crate::window::fetch_response(scope, context, id, response);
            }
        });
        self.perform_microtask_checkpoint();
        true
    }

    fn run_classic_script(&mut self, frame: dom_api::FrameId, script: &ClassicScript<'_>) {
        let Some(context) = self.frame_context(frame) else {
            return;
        };
        let context = &context;
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let global = context.global(scope);
        // What the parser changed before the script: its records' delivery
        // queued first.
        state::report(&mut state::scope(scope).expect("the engine's DOM"));
        v8::tc_scope!(let scope, scope);
        let (Some(source), Some(name)) = (
            v8::String::new(scope, script.source),
            v8::String::new(scope, script.url),
        ) else {
            return;
        };
        let line_offset = i32::try_from(script.line.saturating_sub(1)).unwrap_or(i32::MAX);
        // Its base URL and fetch options, which an `import()` in it takes.
        let options = bindings::script_options(
            scope,
            script.base_url.unwrap_or(script.url),
            Some(crate::frames::credentials_of(script)),
        );
        let origin = v8::ScriptOrigin::new(
            scope,
            name.into(),
            line_offset,
            0,
            !script.muted,
            0,
            None,
            false,
            false,
            false,
            options,
        );
        // Script entered until what it threw is reported (HTML's "run a
        // classic script" reports before its "clean up after running
        // script"): the `error` listeners don't each get a checkpoint.
        let _entry = bindings::trace::enter_script();
        let span = trace_event::trace_event!("EvaluateScript");
        let ran =
            v8::Script::compile(scope, source, Some(&origin)).and_then(|script| script.run(scope));
        drop(span);
        if ran.is_none() {
            report_caught(scope, ReportAt::Global(global));
        }
        state::report(&mut state::scope(scope).expect("the engine's DOM"));
    }

    fn start_module_graph(&mut self, document: &dom::NodeRoot, start: dom_api::ModuleStart) {
        let Some(context) = self.context_of_node(document) else {
            return;
        };
        self.in_context(&context, |scope| {
            if let Some(mut scope) = state::scope(scope) {
                crate::modules::start_graph(&mut scope, document, start);
                state::report(&mut scope);
            }
        });
    }

    fn module_fetched(
        &mut self,
        key: u64,
        response: Option<dom_api::module::FetchedModule>,
    ) -> bool {
        let Some(context) = self.context.clone() else {
            return false;
        };
        let found = self.in_context(&context, |scope| {
            let Some(mut scope) = state::scope(scope) else {
                return false;
            };
            let found = crate::modules::module_fetched(&mut scope, key, response);
            state::report(&mut scope);
            found
        });
        // A task's end: an `import()` it settled reacts now.
        if found {
            self.perform_microtask_checkpoint();
        }
        found
    }

    fn run_ready_script(&mut self, element: &dom::NodeRoot) {
        let Some(context) = self.context_of_node(element) else {
            return;
        };
        self.in_context(&context, |scope| {
            if let Some(mut scope) = state::scope(scope) {
                dom_api::run_script_task(&mut Host { scope: &mut scope }, element);
                state::report(&mut scope);
            }
        });
        self.perform_microtask_checkpoint();
    }

    fn perform_microtask_checkpoint(&mut self) {
        self.queue_mutation_delivery();
        let _span = trace_event::trace_event!("RunMicrotasks");
        // The microtasks are script.
        let entry = bindings::trace::enter_script();
        self.data.isolate().perform_microtask_checkpoint();
        drop(entry);
        self.notify_rejected_promises();
    }

    fn document_ready(&mut self, frame: dom_api::FrameId, readiness: DocumentReadiness) {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return;
        };
        let (context, document) = (&context, &document);
        {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::document_ready(&mut Host { scope: &mut scope }, document, readiness);
        }
        self.perform_microtask_checkpoint();
    }

    fn unload_document(&mut self, frame: dom_api::FrameId) -> Option<(Instant, Instant)> {
        let initial = self.state().frames.get(frame)?.initial;
        let (context, document) = self.frame_context_and_document(frame)?;
        let unloaded = if initial {
            None
        } else {
            let unloaded = {
                let isolate = self.data.isolate();
                v8::scope!(let scope, isolate);
                let context = v8::Local::new(scope, &context);
                let scope = &mut v8::ContextScope::new(scope, context);
                let mut scope = state::scope(scope).expect("the engine's DOM");
                dom_api::unload_document(&mut Host { scope: &mut scope }, &document)
            };
            self.perform_microtask_checkpoint();
            unloaded
        };
        // Then the frames below it, in the order they were made, as
        // Blink's `FrameLoader::DetachDocument` dispatches its document's
        // and then detaches its children (HTML's "unload a document and
        // its descendants" waits for theirs first).
        let children: Vec<dom_api::FrameId> = self
            .state()
            .frames
            .iter()
            .filter(|child| child.parent == Some(frame))
            .map(|child| child.id)
            .collect();
        for child in children {
            self.unload_document(child);
        }
        unloaded
    }

    fn fire_event(&mut self, target: &dom::NodeRoot, event_type: dom::NameId) {
        let Some(context) = self.context_of_node(target) else {
            return;
        };
        let context = &context;
        {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let target = dom::TargetRoot::Node(target.clone());
            dom_api::fire_event(
                &mut Host { scope: &mut scope },
                &target,
                event_type,
                false,
                false,
            );
        }
        self.perform_microtask_checkpoint();
    }

    fn fire_scroll_event(&mut self, target: dom::NodeId) {
        let Some((target, document)) = self.dom_mut().read(|cx| {
            let node = cx
                .node_by_id(target)
                .filter(|&node| cx.is_connected(node))?;
            Some((cx.root(node), cx.kind(node) == dom::NodeKind::Document))
        }) else {
            return;
        };
        let Some(context) = self.context_of_node(&target) else {
            return;
        };
        self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::fire_event(
                &mut Host { scope: &mut scope },
                &dom::TargetRoot::Node(target),
                dom::names::event::SCROLL,
                document,
                false,
            );
        });
        self.perform_microtask_checkpoint();
    }

    fn handle_wheel(&mut self, input: &dom_api::WheelInput) -> bool {
        let (Some(context), Some(document)) = (&self.context, &self.document) else {
            return true;
        };
        let not_canceled = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            self.event_handler
                .handle_wheel(&mut Host { scope: &mut scope }, document, input)
        };
        self.perform_microtask_checkpoint();
        not_canceled
    }

    fn handle_mouse(&mut self, input: &dom_api::MouseInput) -> dom_api::CursorKeyword {
        let (Some(context), Some(document)) = (&self.context, &self.document) else {
            return dom_api::CursorKeyword::Default;
        };
        let cursor = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            self.event_handler
                .handle_mouse(&mut Host { scope: &mut scope }, document, input)
        };
        self.perform_microtask_checkpoint();
        cursor
    }

    fn recompute_under_mouse(&mut self) -> Option<dom_api::CursorKeyword> {
        let (Some(context), Some(document)) = (&self.context, &self.document) else {
            return None;
        };
        let cursor = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            self.event_handler
                .recompute_under_mouse(&mut Host { scope: &mut scope }, document)
        };
        self.perform_microtask_checkpoint();
        cursor
    }

    fn handle_key(&mut self, input: &dom_api::KeyInput) -> dom_api::KeyResult {
        let (Some(context), Some(document)) = (&self.context, &self.document) else {
            return dom_api::KeyResult::Done;
        };
        let result = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            self.event_handler
                .handle_key(&mut Host { scope: &mut scope }, document, input)
        };
        self.perform_microtask_checkpoint();
        result
    }

    fn popup_menu_closed(&mut self, select: dom::NodeId, result: dom_api::PopupResult) {
        let Some(context) = &self.context else {
            return;
        };
        {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let mut host = Host { scope: &mut scope };
            let select = dom_api::typed::ScriptHost::read(&mut host, |cx| {
                cx.node_by_id(select).map(|node| cx.root(node))
            });
            if let Some(select) = select {
                dom_api::popup_menu_closed(&mut host, &select, result);
            }
        }
        self.perform_microtask_checkpoint();
    }

    fn set_initial_focus(&mut self, reverse: bool) {
        let (Some(context), Some(document)) = (&self.context, &self.document) else {
            return;
        };
        {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::set_initial_focus(&mut Host { scope: &mut scope }, document, reverse);
        }
        self.perform_microtask_checkpoint();
    }

    /// The DOM read through the isolate's state, no scope entered: the
    /// host asks at each turn of its loop.
    fn has_animation_frames(&mut self) -> bool {
        let State { dom, frames, .. } = self.state();
        dom.read(|cx| {
            frames.iter().any(|frame| {
                cx.window_of(frame.document.get(cx.gc))
                    .is_some_and(|window| cx.has_animation_frames(window))
            })
        })
    }

    fn frames_in_tree_order(&mut self) -> Vec<dom_api::FrameId> {
        self.frames_with_contexts()
            .into_iter()
            .map(|(frame, ..)| frame)
            .collect()
    }

    fn documents_in_tree_order(&mut self) -> Vec<NodeId> {
        let documents = match &self.rendering_documents {
            Some(documents) => documents.clone(),
            None => self.walk_documents(),
        };
        let state = self.state();
        documents
            .into_iter()
            .filter(|&document| state.frames.of_document(document).is_some())
            .collect()
    }

    fn begin_rendering_update(&mut self) {
        self.rendering_documents = Some(self.walk_documents());
    }

    fn end_rendering_update(&mut self) {
        self.rendering_documents = None;
    }

    fn run_animation_frames(&mut self, frame_time: Instant) {
        // The documents as the step began (HTML's `docs`).
        for (_, context, _) in self.frames_with_contexts() {
            self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::run_animation_frames(scope, context, frame_time);
            });
        }
        self.notify_rejected_promises();
    }

    fn set_page_focused(&mut self, focused: bool) {
        self.page_focused = focused;
        let Some(context) = &self.context else {
            return;
        };
        {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            crate::window::set_page_focused(scope, context, focused);
        }
        self.perform_microtask_checkpoint();
    }

    fn set_page_visible(&mut self, visible: bool) {
        self.page_hidden = !visible;
        let (Some(context), Some(document)) = (self.context.clone(), self.document.clone()) else {
            return;
        };
        self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::set_page_visible(&mut Host { scope: &mut scope }, &document, visible);
            state::report(&mut scope);
        });
        self.perform_microtask_checkpoint();
    }

    fn flush_autofocus_candidates(&mut self) {
        let (Some(context), Some(document)) = (self.context.clone(), self.document.clone()) else {
            return;
        };
        let flushed = self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let (state, _) = scope.state();
            if !state
                .dom
                .read(|cx| cx.has_autofocus_candidates(document.get(cx.gc)))
            {
                return false;
            }
            dom_api::flush_autofocus_candidates(&mut Host { scope: &mut scope }, &document);
            true
        });
        if flushed {
            self.perform_microtask_checkpoint();
        }
    }

    fn run_focus_fixup(&mut self) {
        for (_, context, document) in self.frames_with_contexts() {
            self.in_context(&context, |scope| {
                let mut scope = state::scope(scope).expect("the engine's DOM");
                dom_api::focus_fixup(&mut Host { scope: &mut scope }, &document);
            });
        }
        self.perform_microtask_checkpoint();
    }

    fn react_to_environment_changes(&mut self, frame: dom_api::FrameId) {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return;
        };
        self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::react_to_environment_changes(&mut Host { scope: &mut scope }, &document);
        });
        self.perform_microtask_checkpoint();
    }

    fn report_media_query_changes(&mut self, frame: dom_api::FrameId) {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return;
        };
        // A document that never made one: nothing to evaluate, no realm
        // entered.
        if !self
            .dom_mut()
            .read(|cx| cx.has_media_query_lists(document.get(cx.gc)))
        {
            return;
        }
        self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::report_media_query_changes(&mut Host { scope: &mut scope }, &document);
        });
        self.perform_microtask_checkpoint();
    }

    fn update_animations(
        &mut self,
        frame: dom_api::FrameId,
        frame_time: Instant,
    ) -> dom_api::AnimationsUpdate {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return dom_api::AnimationsUpdate::default();
        };
        // Its timeline's time: the frame's, from its window's time origin.
        let (now, any) = self.dom_mut().read(|cx| {
            let document = document.get(cx.gc);
            let now = cx
                .window_of(document)
                .map_or(0.0, |window| cx.time_at(window, frame_time));
            (now, cx.has_animations(document))
        });
        // A document with none (most): its timeline moves on, no realm
        // entered.
        if !any {
            self.dom_mut()
                .read(|cx| cx.tick_animations(document.get(cx.gc), now));
            return dom_api::AnimationsUpdate::default();
        }
        let update = self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::update_animations(&mut Host { scope: &mut scope }, &document, now)
        });
        self.perform_microtask_checkpoint();
        update
    }

    fn animations_want_frames(&mut self) -> bool {
        // Asked at every turn of the host's loop, of any frame: the frames
        // as made, no tree walked (Chrome's tasks don't look for every
        // frame either).
        let documents: Vec<NodeRoot> = self
            .frames_as_made()
            .into_iter()
            .map(|(_, _, document)| document)
            .collect();
        self.dom_mut().read(|cx| {
            documents
                .iter()
                .any(|document| cx.animations_want_frames(document.get(cx.gc)))
        })
    }

    fn set_session_history(&mut self, history: dom::SessionHistory) {
        self.dom_mut().read(|cx| cx.set_session_history(history));
    }

    fn traverse_within_document(
        &mut self,
        frame: dom_api::FrameId,
        url: &str,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
    ) -> bool {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return false;
        };
        let traversed = self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::traverse_within_document(
                &mut Host { scope: &mut scope },
                &document,
                url,
                state,
                manual_scroll_restoration,
            )
        });
        self.perform_microtask_checkpoint();
        traversed
    }

    fn restore_history(
        &mut self,
        frame: dom_api::FrameId,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
    ) {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return;
        };
        self.in_context(&context, |scope| {
            let mut scope = state::scope(scope).expect("the engine's DOM");
            dom_api::restore_history(
                &mut Host { scope: &mut scope },
                &document,
                state,
                manual_scroll_restoration,
            );
        });
    }

    fn start_pending_animations(&mut self, frame: dom_api::FrameId) -> bool {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return false;
        };
        let started = self
            .dom_mut()
            .read(|cx| cx.start_pending_animations(document.get(cx.gc)));
        // Their `ready` promises settle now (a realm entered only then).
        if started {
            self.in_context(&context, |scope| {
                let mut scope = state::scope(scope).expect("the engine's DOM");
                dom_api::settle_started_animations(&mut Host { scope: &mut scope }, &document);
            });
            self.perform_microtask_checkpoint();
        }
        started
            || self
                .dom_mut()
                .read(|cx| cx.animations_want_frames(document.get(cx.gc)))
    }

    fn webdriver_send_keys(
        &mut self,
        frame: dom_api::FrameId,
        element: dom::NodeId,
        text: &str,
    ) -> Result<bool, dom_api::WebDriverActionError> {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return Err(dom_api::WebDriverActionError::Query(
                dom_api::WebDriverQueryError::NotShown,
            ));
        };
        let result = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let result = dom_api::webdriver_send_keys(
                &mut Host { scope: &mut scope },
                &document,
                element,
                text,
            );
            state::report(&mut scope);
            result
        };
        self.perform_microtask_checkpoint();
        result
    }

    fn webdriver_clear(
        &mut self,
        frame: dom_api::FrameId,
        element: dom::NodeId,
    ) -> Result<(), dom_api::WebDriverActionError> {
        let Some((context, document)) = self.frame_context_and_document(frame) else {
            return Err(dom_api::WebDriverActionError::Query(
                dom_api::WebDriverQueryError::NotShown,
            ));
        };
        let result = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let mut scope = state::scope(scope).expect("the engine's DOM");
            let result =
                dom_api::webdriver_clear(&mut Host { scope: &mut scope }, &document, element);
            state::report(&mut scope);
            result
        };
        self.perform_microtask_checkpoint();
        result
    }

    fn webdriver_query(
        &mut self,
        frame: dom_api::FrameId,
        query: &dom_api::WebDriverQuery<'_>,
    ) -> Result<dom_api::WebDriverAnswer, dom_api::WebDriverQueryError> {
        let Some((_, document)) = self.frame_context_and_document(frame) else {
            return Err(dom_api::WebDriverQueryError::NotShown);
        };
        let state =
            bindings::isolate_state_mut::<State>(self.data.isolate()).expect("the engine's DOM");
        let State { dom, layout, .. } = state;
        dom.mutate(|cx| {
            dom_api::webdriver_answer(
                &dom_api::ApiCx::with_layout_cell(cx, layout),
                document.get(cx.gc),
                query,
            )
        })
    }

    fn execute_webdriver_script(
        &mut self,
        frame: dom_api::FrameId,
        id: u64,
        body: &str,
        args: &[dom_api::WebDriverValue],
        is_async: bool,
    ) -> Result<(), dom_api::WebDriverScriptError> {
        let Some(context) = self.frame_context(frame) else {
            return Err(dom_api::WebDriverScriptError(
                "the frame has no document".into(),
            ));
        };
        let started = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Local::new(scope, context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let _entry = bindings::trace::enter_script();
            crate::webdriver::execute(scope, id, body, args, is_async)
        };
        // What settled at once (an `async` function's promise settles in a
        // microtask).
        self.perform_microtask_checkpoint();
        started
    }

    fn take_webdriver_results(
        &mut self,
    ) -> Vec<(
        u64,
        Result<dom_api::WebDriverValue, dom_api::WebDriverScriptError>,
    )> {
        let state =
            bindings::isolate_state_mut::<State>(self.data.isolate()).expect("the engine's DOM");
        std::mem::take(&mut state.webdriver_results)
    }

    fn webdriver_element_property(
        &mut self,
        frame: dom_api::FrameId,
        element: dom::NodeId,
        name: &str,
    ) -> Result<dom_api::WebDriverValue, dom_api::WebDriverScriptError> {
        let Some(context) = self.frame_context(frame) else {
            return Err(dom_api::WebDriverScriptError(
                "the frame has no document".into(),
            ));
        };
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let _entry = bindings::trace::enter_script();
        crate::webdriver::element_property(scope, element, name)
    }

    fn until_next_timer(&mut self) -> Option<std::time::Duration> {
        let State { dom, frames, .. } = self.state();
        dom.read(|cx| {
            frames
                .iter()
                .filter_map(|frame| {
                    crate::window::until_next_timer(cx, cx.window_of(frame.document.get(cx.gc))?)
                })
                .min()
        })
    }

    fn run_timers(&mut self) -> bool {
        let mut nows = Vec::new();
        let mut ran = false;
        while let Some((context, now)) = self.earliest_due_timer(&mut nows) {
            let one = self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::run_next_timer(scope, context, now)
            });
            if !one {
                break;
            }
            ran = true;
        }
        if ran {
            // Each timer's own checkpoint ran; the rejections it left.
            self.notify_rejected_promises();
        }
        ran
    }

    fn run_due_tasks(&mut self) -> bool {
        let frames = self.frames_as_made();
        self.state().dom.read(|cx| cx.begin_task_pass());
        let mut ran = false;
        loop {
            // The clock read again before each: a long task lets the next
            // timers' turn come.
            let next = {
                let dom = &self.state().dom;
                dom.read(|cx| {
                    let now = cx.dom_now();
                    let mut best: Option<(dom::NextTask, usize, f64)> = None;
                    for (index, (_, _, document)) in frames.iter().enumerate() {
                        let Some(window) = cx.window_of(document.get(cx.gc)) else {
                            continue;
                        };
                        let Some(task) = cx.next_task(window, now) else {
                            continue;
                        };
                        if best.as_ref().is_none_or(|(best, ..)| task.before(best)) {
                            let since = now - cx.window_time_origin(window);
                            best = Some((task, index, since));
                        }
                    }
                    best
                })
            };
            let Some((task, index, now)) = next else {
                break;
            };
            let context = frames[index].1.clone();
            let one = self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::run_task(scope, context, task.queue, now)
            });
            if !one {
                break;
            }
            ran = true;
        }
        let frames = self.frames_as_made();
        self.state().dom.read(|cx| {
            let windows: Vec<_> = frames
                .iter()
                .filter_map(|(_, _, document)| cx.window_of(document.get(cx.gc)))
                .collect();
            cx.end_task_pass(&windows);
        });
        if ran {
            // Each task's own checkpoint ran; the rejections it left.
            self.notify_rejected_promises();
        }
        ran
    }

    fn has_resize_observations(&mut self, frame: dom_api::FrameId) -> bool {
        let Some(context) = self.frame_context(frame) else {
            return false;
        };
        self.in_context(&context, |scope| {
            let context = scope.get_current_context();
            crate::window::has_resize_observations(scope, context)
        })
    }

    fn gather_and_broadcast_resizes(
        &mut self,
        frame: dom_api::FrameId,
        depth: usize,
    ) -> dom_api::ResizeObserverRound {
        let Some(context) = self.frame_context(frame) else {
            return dom_api::ResizeObserverRound::default();
        };
        self.in_context(&context, |scope| {
            let context = scope.get_current_context();
            crate::window::gather_and_broadcast_resizes(scope, context, depth)
        })
    }

    fn report_resize_loop_error(&mut self, frame: dom_api::FrameId) {
        let Some(context) = self.frame_context(frame) else {
            return;
        };
        self.in_context(&context, |scope| {
            let context = scope.get_current_context();
            crate::window::report_resize_loop_error(scope, context);
        });
    }

    fn notify_intersection_observers(&mut self) {
        // Each window's task of its own.
        for (_, context, _) in self.frames_with_contexts() {
            self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::notify_intersection_observers(scope, context);
            });
            self.perform_microtask_checkpoint();
        }
    }

    fn run_posted_messages(&mut self) -> bool {
        let mut ran = false;
        for (_, context, _) in self.frames_as_made() {
            ran |= self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::run_posted_messages(scope, context) > 0
            });
        }
        if ran {
            // Each message's own checkpoint ran; the rejections it left.
            self.notify_rejected_promises();
        }
        ran
    }

    fn has_idle_callbacks(&mut self) -> bool {
        self.any_window(|cx, window| cx.has_idle_callbacks(window))
    }

    fn has_runnable_idle_callback(&mut self) -> bool {
        self.any_window(|cx, window| cx.has_runnable_idle_callback(window))
    }

    fn start_idle_period(&mut self) {
        let frames = self.frames_as_made();
        self.state().dom.mutate(|cx| {
            for (_, _, document) in &frames {
                if let Some(window) = cx.window_of(document.get(cx.gc)) {
                    cx.start_idle_period(window);
                }
            }
        });
    }

    fn run_idle_callback(&mut self, deadline: Instant) -> bool {
        for (_, context, _) in self.frames_with_contexts() {
            let ran = self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::run_idle_callback(scope, context, deadline)
            });
            if ran {
                self.notify_rejected_promises();
                return true;
            }
        }
        false
    }

    fn set_input_monitor(&mut self, monitor: Box<dyn dom_api::InputMonitor>) {
        self.state().input_monitor = Some(monitor);
    }

    fn run_element_tasks(&mut self) -> bool {
        let mut ran = false;
        for (_, context, _) in self.frames_as_made() {
            ran |= self.in_context(&context, |scope| {
                let context = scope.get_current_context();
                crate::window::run_element_tasks(scope, context) > 0
            });
        }
        if ran {
            // Each task's own checkpoint ran; the rejections it left.
            self.notify_rejected_promises();
        }
        ran
    }

    fn run_platform_tasks(&mut self) -> bool {
        // V8's own tasks may run script (a finalization registry's cleanup).
        let _entry = bindings::trace::enter_script();
        self.data.pump_platform_tasks()
    }

    fn set_load_sink(&mut self, sink: Box<dyn dom_api::LoadSink>) {
        self.state().frames.load_sink = Some(sink);
    }

    fn set_console_sink(&mut self, sink: ConsoleSink) {
        self.data.set_console_sink(Box::new(move |message| {
            sink(ConsoleMessage {
                text: message.text,
                url: message.url,
                line: message.line,
                uncaught: message.uncaught,
            });
        }));
    }

    fn take_harness_results(&mut self) -> Option<String> {
        bindings::isolate_state_mut::<State>(self.data.isolate())
            .and_then(|state| state.harness_results.take())
    }

    fn set_idle(&mut self, idle: bool) {
        self.data.set_idle(idle);
    }

    fn set_devtools_frame_ids(&mut self, ids: DevToolsFrameIds) {
        self.state().devtools_frame_ids = Some(ids);
    }

    fn devtools_attach(&mut self, session: String, send: DevToolsSend) {
        self.data.devtools_attach(session, send);
    }

    fn devtools_detach(&mut self, session: &str) {
        self.data.devtools_detach(session);
    }

    fn dispatch_devtools_message(&mut self, session: &str, message: &str) -> bool {
        // An entry of script: what the command runs reaches the DOM as any
        // script does (the state isn't borrowed meanwhile), and may pause.
        let entry = bindings::trace::enter_script();
        let dispatched = self.data.dispatch_devtools_message(session, message);
        drop(entry);
        if dispatched {
            self.perform_microtask_checkpoint();
        }
        dispatched
    }

    fn set_devtools_queue(&mut self, queue: Box<dyn DevToolsQueue>) -> DevToolsInterrupter {
        self.data
            .set_devtools_queue(Box::new(crate::devtools::Queue(queue)))
    }

    /// The DOM notes who asks while they are wanted; the stacks are
    /// captured as each operation ends (`state::report`), an `import()`'s
    /// as it runs.
    fn set_stacks_wanted(&mut self, requests: bool, frames: bool) {
        self.state().dom.set_stacks_wanted(requests, frames);
    }
}

impl V8Engine {
    /// The mutation observers' microtask queued, in the realm, if an
    /// operation out of script (the parser's) queued the first record.
    fn queue_mutation_delivery(&mut self) {
        let Some(context) = &self.context else {
            return;
        };
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, context);
        let scope = &mut v8::ContextScope::new(scope, context);
        state::report(&mut state::scope(scope).expect("the engine's DOM"));
    }

    /// HTML's "notify about rejected promises": each promise rejected with
    /// no handler since the last (and none added since: V8 tells), an
    /// `unhandledrejection` at its realm's window; one not canceled, or of
    /// a realm whose global isn't a window of the DOM, logged, and told
    /// to DevTools' console.
    fn notify_rejected_promises(&mut self) {
        let rejected = self.data.take_rejected_promises();
        if rejected.is_empty() {
            return;
        }
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        for rejection in rejected {
            let context = v8::Local::new(scope, &rejection.context);
            let scope = &mut v8::ContextScope::new(scope, context);
            let promise = v8::Local::new(scope, &rejection.promise);
            let reason = v8::Local::new(scope, &rejection.reason);
            let global = context.global(scope);
            {
                let mut scope = state::scope(scope).expect("the engine's DOM");
                let window = {
                    let (state, mut no_gc) = scope.state();
                    state.dom.read(|cx| {
                        no_gc
                            .unwrap::<Window>(cx.gc, global)
                            .map(|window| Persistent::new(cx.gc, window))
                    })
                };
                let not_canceled = match window {
                    Some(window) => dom_api::report_unhandled_rejection(
                        &mut Host { scope: &mut scope },
                        &window,
                        promise.into(),
                        reason,
                    ),
                    None => true,
                };
                if not_canceled {
                    let text = reason.to_rust_string_lossy(scope.scope());
                    bindings::devtools::report_uncaught(
                        scope.scope(),
                        bindings::devtools::ConsoleMessage {
                            text: format!("(in promise) {text}"),
                            url: String::new(),
                            line: 0,
                            uncaught: true,
                        },
                    );
                    let state = scope.state_mut().0;
                    state.uncaught_exceptions = state.uncaught_exceptions.wrapping_add(1);
                    crate::devtools::rejection_unhandled(
                        scope.scope(),
                        context,
                        promise,
                        reason,
                        &text,
                    );
                }
            }
            let _entry = bindings::trace::enter_script();
            scope.perform_microtask_checkpoint();
        }
    }
}

/// `testRunner` on `global` (web tests): `reportHarnessResults(text)` keeps
/// the text for the host.
fn install_test_runner(scope: &mut v8::PinScope<'_, '_>, global: v8::Local<'_, v8::Object>) {
    let runner = v8::Object::new(scope);
    let report = v8::FunctionTemplate::new(scope, report_harness_results);
    let report = report.get_function(scope).expect("a function");
    let name = string(scope, "reportHarnessResults");
    runner.set(scope, name.into(), report.into());
    let name = string(scope, "testRunner");
    global.define_own_property(
        scope,
        name.into(),
        runner.into(),
        v8::PropertyAttribute::DONT_ENUM,
    );
}

fn report_harness_results(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments<'_>,
    _: v8::ReturnValue<'_>,
) {
    let text = args.get(0).to_rust_string_lossy(scope);
    if let Some(state) = bindings::isolate_state_mut::<State>(scope) {
        state.harness_results = Some(text);
    }
}
