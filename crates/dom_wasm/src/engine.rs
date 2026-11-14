//! wasmtime as a page's script engine (`dom_api::ScriptEngine`): a document
//! whose scripts are wasm components. The engine owns the page's DOM and
//! lends it to the guests for each turn of theirs (`WasmDom::lend`); each
//! script is a store, whose `start` gets the page's document, and whose
//! listeners the DOM's dispatch calls through its `handle-event`. There is
//! no JS: no realm, no timer or observer of script's, no inspector, so the
//! calls for those do nothing, and the DOM's own work (the default actions
//! of input, focus, the lifecycle's events, the tasks of elements) runs
//! through `dom_api` as it does under V8.

use std::pin::pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::{Duration, Instant};

use dom::{Dom, NodeId, NodeRoot, Window};
use dom_api::typed::ScriptHost;
use dom_api::{
    ApiCx, ConsoleMessage, ConsoleSink, DevToolsFrameIds, DevToolsInterrupter, DevToolsObjects,
    DevToolsPage, DevToolsQueue, DevToolsSend, DocumentReadiness, EngineSettings, FrameId,
    FrameLayout, LayoutCell, ScriptEngine, WasmScript,
};
use elise::Persistent;
use wasmtime::component::{Component, Linker};
use wasmtime::{AsContextMut, Config, Engine, Store, StoreSet};
use wasmtime_wasi::{WasiCtxView, WasiView};

use crate::{GuestClock, GuestClockView, Output, WasmDom, WasmStore, WasmWasi, poll_until_idle};

/// The page's one frame: a wasm document has no child frames.
const TOP: FrameId = FrameId(0);

/// A script's store's data.
struct Data {
    wasi: WasmWasi,
    dom: WasmStore,
}

impl WasiView for Data {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        self.wasi.view()
    }
}

impl GuestClockView for Data {
    fn guest_clock(&mut self) -> &mut GuestClock {
        self.wasi.clock()
    }
}

/// wasmtime's engine and the guests' imports: a page's engine's own,
/// made with its first script (a renderer shares nothing with another).
struct Runtime {
    engine: Engine,
    linker: Linker<Data>,
}

impl Runtime {
    fn new() -> wasmtime::Result<Self> {
        let mut config = Config::new();
        crate::configure(&mut config);
        let engine = Engine::new(&config)?;
        let mut linker = Linker::new(&engine);
        crate::add_wasi_to_linker(&mut linker)?;
        crate::add_to_linker(&mut linker, |data: &Data| &data.dom)?;
        Ok(Self { engine, linker })
    }
}

/// The embedder's time for the guests' clocks: nanoseconds, never back.
type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// A script's store in the set, and when it next has something to do.
struct Script {
    index: usize,
    /// The store's number in the DOM, which its listeners' callbacks carry.
    number: u32,
    url: String,
    /// The earliest deadline its guest waits for, as it was after its last
    /// turn (the clock is in the store, reached only in a turn).
    deadline: Option<u64>,
}

/// What the DOM's work calls script for, in a wasm document: the guests'
/// listeners (a guest can't add a timer or an observer, so each of those
/// is a call of nothing). The operations are the engine's DOM's.
struct Host<'a> {
    /// The engine's DOM: lent to a guest for each listener of its.
    dom: &'a mut Option<Dom>,
    layout: &'a LayoutCell,
    /// The window's document's number, and what the DOM's work asks of the
    /// host for its frame (`ScriptEngine::take_frame_requests`).
    top: Option<NodeId>,
    requests: &'a mut Vec<dom_api::FrameRequest>,
    wasm: &'a WasmDom,
    set: &'a mut Option<StoreSet<Data>>,
    scripts: &'a mut Vec<Script>,
    console: &'a Option<ConsoleSink>,
}

impl Host<'_> {
    fn dom(&mut self) -> &mut Dom {
        self.dom.as_mut().expect("the DOM, outside a guest's turn")
    }
}

/// A message for the console, about `url`.
fn report(console: &Option<ConsoleSink>, url: &str, text: String, uncaught: bool) {
    if let Some(console) = console {
        console(ConsoleMessage {
            text,
            url: url.to_owned(),
            line: 0,
            uncaught,
        });
    }
}

impl ScriptHost for Host<'_> {
    type Value = ();
    /// None: no member that defines custom elements is called through it.
    type Callback = std::convert::Infallible;
    type Lifecycle = std::convert::Infallible;

    fn read<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        let layout = self.layout;
        self.dom()
            .read(|cx| f(&ApiCx::with_layout_cell(cx, layout)))
    }

    fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        let layout = self.layout;
        self.dom()
            .mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout)))
    }

    /// The window's document navigated, or is to (a link followed, a form
    /// submitted, a fragment gone to): the host told.
    fn navigate(&mut self, document: &NodeRoot, navigation: dom::FrameNavigation) {
        let id = self.dom().read(|cx| cx.node_id(document.get(cx.gc)));
        if self.top == Some(id) {
            self.requests.push(dom_api::FrameRequest::Navigation {
                frame: TOP,
                navigation,
            });
        }
    }

    /// A guest's listener, through its store's `handle-event`: the DOM
    /// lent for the call, the event's handle the store's until it returns.
    /// A guest that traps (or waits: the export is a sync function) is
    /// reported and gone with its store, its other listeners with it. An
    /// event handler attribute's listener (`onclick=""`) is JS: not run.
    fn call_listener(&mut self, event: &Persistent<dom::Event>, index: usize) {
        let listener = self.dom().read(|cx| {
            let listener = cx.snapshot_at(event.get(cx.gc), index)?;
            crate::listener_of(cx.callback_edge(listener).embedder_word(cx.gc))
        });
        let Some((store, callback)) = listener else {
            return;
        };
        let Some(at) = self
            .scripts
            .iter()
            .position(|script| script.number == store)
        else {
            return;
        };
        let (Some(set), Some(script)) = (self.set.as_mut(), self.scripts.get(at)) else {
            return;
        };
        let index = script.index;
        let called = self.wasm.lend_page(self.dom, self.layout, || {
            let called = set
                .with(index, |mut cx| -> wasmtime::Result<()> {
                    let Some(handler) = cx.data().dom.handler() else {
                        return Ok(());
                    };
                    let handle = cx.data().dom.give_event(event)?;
                    let called = match poll_until_idle(pin!(
                        handler.call_async(&mut cx, (callback, handle))
                    )) {
                        Poll::Ready(called) => called,
                        Poll::Pending => Err(wasmtime::format_err!("a listener waits")),
                    };
                    cx.data().dom.release_if_held(handle);
                    called
                })
                .and_then(|called| called);
            if called.is_err() {
                // A trap ends its store (the Component Model's rule).
                let _ = set.remove(index);
            }
            called
        });
        if let Err(trap) = called {
            let url = self.scripts.remove(at).url;
            report(self.console, &url, format!("{trap:?}"), true);
        }
    }

    fn call_timer(&mut self, _: &Persistent<Window>, _: &Persistent<dom::Timer>) {}

    fn call_mutation_callback(
        &mut self,
        _: &Persistent<dom::MutationObserver>,
        _: &[Persistent<dom::MutationRecord>],
    ) {
    }

    // The DOM's own animations (CSS's) and image loads have these to
    // settle; their promises are script's, and there is none.
    fn settle_animation_notices(&mut self, _: &Persistent<dom::EventTargetImpl>) {}

    fn settle_promises(&mut self, _: &[u64], _: Option<dom_api::ApiError>) {}
}

/// wasmtime as the DOM's engine.
pub struct WasmEngine {
    /// The window's document, once there is one, and its number.
    document: Option<NodeRoot>,
    document_id: Option<NodeId>,
    /// What the DOM's work asked of the host since it last took them.
    requests: Vec<dom_api::FrameRequest>,
    /// The window's mouse state (a new one with each window).
    event_handler: dom_api::EventHandler,
    page_focused: bool,
    page_hidden: bool,
    /// The documents of the rendering update that runs, in tree order.
    rendering_documents: Option<Vec<NodeId>>,
    layout: Box<LayoutCell>,
    console: Option<ConsoleSink>,
    /// The lines the guests wrote since the last turn: their sink is the
    /// stores' (`Send`), the console's isn't.
    output: Arc<Mutex<Vec<(Output, String)>>>,
    clock: Clock,
    /// The document's scripts' stores, in the set.
    scripts: Vec<Script>,
    /// Made with the first script, and dropped, as each store is, while
    /// the DOM is lent.
    set: Option<StoreSet<Data>>,
    /// Made with the first script; after the stores, which are its.
    runtime: Option<Runtime>,
    /// The thread's place for the DOM while a guest runs, and the stores'
    /// tables: before the DOM, whose heap's roots they are.
    wasm: WasmDom,
    /// The page's DOM: here but in a turn of the guests.
    dom: Option<Dom>,
}

impl WasmEngine {
    pub fn new(settings: EngineSettings) -> Self {
        let start = Instant::now();
        Self::with_clock(
            settings,
            Arc::new(move || u64::try_from(start.elapsed().as_nanos()).unwrap_or(u64::MAX)),
        )
    }

    /// An engine whose guests' clocks read `clock` (nanoseconds, never
    /// back): the tests', which move it themselves.
    pub fn with_clock(_: EngineSettings, clock: Arc<dyn Fn() -> u64 + Send + Sync>) -> Self {
        let wasm = WasmDom::new();
        let dom = Some(wasm.take());
        Self {
            document: None,
            document_id: None,
            requests: Vec::new(),
            event_handler: dom_api::EventHandler::default(),
            page_focused: false,
            page_hidden: false,
            rendering_documents: None,
            layout: Box::new(std::cell::Cell::new(None)),
            console: None,
            output: Arc::default(),
            clock,
            scripts: Vec::new(),
            set: None,
            runtime: None,
            wasm,
            dom,
        }
    }

    /// For whoever starts a renderer: its new DOM's engine.
    pub fn boxed(settings: EngineSettings) -> Box<dyn ScriptEngine> {
        Box::new(Self::new(settings))
    }

    fn dom(&mut self) -> &mut Dom {
        self.dom.as_mut().expect("the DOM, outside a guest's turn")
    }

    /// The DOM's side of a call of `dom_api`'s, and the window's document.
    fn host(&mut self) -> Host<'_> {
        self.host_and_input().0
    }

    /// `host`, with the window's mouse state and its document: what the
    /// input's handling takes beside it.
    fn host_and_input(&mut self) -> (Host<'_>, &mut dom_api::EventHandler, Option<&NodeRoot>) {
        let host = Host {
            dom: &mut self.dom,
            layout: &self.layout,
            top: self.document_id,
            requests: &mut self.requests,
            wasm: &self.wasm,
            set: &mut self.set,
            scripts: &mut self.scripts,
            console: &self.console,
        };
        (host, &mut self.event_handler, self.document.as_ref())
    }

    fn window(&mut self) -> Option<Persistent<Window>> {
        let document = self.document.clone()?;
        self.dom().read(|cx| {
            cx.window_of(document.get(cx.gc))
                .map(|window| Persistent::new(cx.gc, window))
        })
    }

    /// A message for the console, about `url`.
    fn report(&self, url: &str, text: String, uncaught: bool) {
        report(&self.console, url, text, uncaught);
    }

    /// The lines the guests wrote, to the console.
    fn flush_output(&self) {
        let lines = std::mem::take(&mut *self.output.lock().expect("no writer panics"));
        for (_, line) in lines {
            self.report("", line, false);
        }
    }

    /// The stores gone (the document is: a navigation, or the engine),
    /// while the DOM is lent.
    fn drop_scripts(&mut self) {
        let Self {
            wasm,
            dom,
            set,
            scripts,
            ..
        } = self;
        if set.is_none() {
            return;
        }
        wasm.lend(dom, || {
            scripts.clear();
            *set = None;
        });
        self.flush_output();
    }

    /// One turn of the script at `at` (its store lent with the DOM): `f`
    /// starts what the turn is for, if anything; then the store's event
    /// loop runs until nothing moves. A guest that trapped is reported and
    /// gone with its store.
    fn turn(
        &mut self,
        at: usize,
        f: impl FnOnce(&mut wasmtime::StoreContextMut<'_, Data>) -> wasmtime::Result<()>,
    ) {
        let Self {
            wasm,
            dom,
            layout,
            set,
            scripts,
            ..
        } = self;
        let (Some(set), Some(script)) = (set.as_mut(), scripts.get_mut(at)) else {
            return;
        };
        let index = script.index;
        let ran = wasm.lend_page(dom, layout, || {
            let ran = set
                .with(index, |mut cx| -> wasmtime::Result<Option<u64>> {
                    cx.data_mut().guest_clock().wake_due();
                    f(&mut cx)?;
                    // Its tasks, and the host functions' futures the wake
                    // marked: until none moves.
                    {
                        let turn = pin!(
                            cx.as_context_mut()
                                .run_concurrent(async |_| std::future::pending::<()>().await)
                        );
                        if let Poll::Ready(Err(trap)) = poll_until_idle(turn) {
                            return Err(trap);
                        }
                    }
                    Ok(cx.data_mut().guest_clock().next_deadline())
                })
                .and_then(|ran| ran);
            if ran.is_err() {
                // A trap ends its store (the Component Model's rule).
                let _ = set.remove(index);
            }
            ran
        });
        match ran {
            Ok(deadline) => script.deadline = deadline,
            Err(trap) => {
                let url = scripts.remove(at).url;
                self.report(&url, format!("{trap:?}"), true);
            }
        }
        self.flush_output();
        self.perform_microtask_checkpoint();
    }

    /// `script`'s store made and instantiated, and its `start` called with
    /// `document`.
    fn start_script(&mut self, runtime: &Runtime, document: &NodeRoot, script: &WasmScript<'_>) {
        let (engine, linker) = (&runtime.engine, &runtime.linker);
        let component = match Component::new(engine, script.bytes) {
            Ok(component) => component,
            Err(error) => return self.report(script.url, format!("{error:?}"), true),
        };
        let (clock, output) = (self.clock.clone(), self.output.clone());
        let Self { wasm, dom, set, .. } = self;
        // Its store, in the set (made with the first one).
        let made = wasm.lend(dom, || -> wasmtime::Result<(usize, u32)> {
            let set = match set {
                Some(set) => set,
                None => set.insert(wasm.store_set(engine)?),
            };
            let dom = WasmStore::new();
            let number = dom.number();
            let data = Data {
                wasi: WasmWasi::new(
                    move || clock(),
                    Arc::new(move |output_of, line: &str| {
                        output
                            .lock()
                            .expect("no writer panics")
                            .push((output_of, line.to_owned()));
                    }),
                ),
                dom,
            };
            Ok((set.insert(Store::new(engine, data))?, number))
        });
        let (index, number) = match made {
            Ok(index) => index,
            Err(error) => return self.report(script.url, format!("{error:?}"), true),
        };
        self.scripts.push(Script {
            index,
            number,
            url: script.url.to_owned(),
            deadline: None,
        });
        let at = self.scripts.len().saturating_sub(1);
        // Instantiated, then `start(document)`: done at its first wait (the
        // scripts after it and the document's load don't wait for it).
        self.turn(at, |cx| {
            let instance =
                match poll_until_idle(pin!(linker.instantiate_async(&mut *cx, &component))) {
                    Poll::Ready(instance) => instance?,
                    Poll::Pending => wasmtime::bail!("the component waits as it is instantiated"),
                };
            let start = instance.get_typed_func::<(u32,), ()>(&mut *cx, "start")?;
            // What its listeners are called through.
            let handler = instance.get_typed_func::<(u32, u32), ()>(&mut *cx, "handle-event")?;
            cx.data_mut().dom.set_handler(handler);
            let handle = cx.data().dom.give(document)?;
            // The call's future goes with this poll; the task it started
            // is the store's, and runs in the turn.
            let call = pin!(cx.as_context_mut().run_concurrent(async |accessor| {
                start.call_concurrent(accessor, (handle,)).await
            }));
            match poll_until_idle(call) {
                Poll::Ready(result) => result?,
                Poll::Pending => Ok(()),
            }
        });
    }

    /// A turn of each store whose guest's wait is over; whether any ran.
    fn run_due_guests(&mut self) -> bool {
        let now = (self.clock)();
        let mut ran = false;
        // A turn may end a store (a trap): by its place in the set.
        let due: Vec<usize> = self
            .scripts
            .iter()
            .filter(|script| script.deadline.is_some_and(|deadline| deadline <= now))
            .map(|script| script.index)
            .collect();
        for index in due {
            if let Some(at) = self.scripts.iter().position(|script| script.index == index) {
                self.turn(at, |_| Ok(()));
                ran = true;
            }
        }
        ran
    }
}

impl Drop for WasmEngine {
    fn drop(&mut self) {
        self.drop_scripts();
        // The roots of the heap before it.
        self.document = None;
        self.event_handler = dom_api::EventHandler::default();
    }
}

impl dom_api::ParserHost for WasmEngine {
    fn parser_dom(&mut self) -> &mut Dom {
        self.dom()
    }

    // No guest defines a custom element.
    fn create_custom_element(
        &mut self,
        _: &NodeRoot,
        _: &str,
        _: &[(dom::QualName<'_>, &str)],
    ) -> Option<NodeRoot> {
        None
    }

    fn run_custom_element_reactions(&mut self) {
        if self.dom().read(|cx| cx.has_element_changes()) {
            dom_api::update_elements(&mut self.host());
        }
    }
}

impl ScriptEngine for WasmEngine {
    fn dom_mut(&mut self) -> &mut Dom {
        self.dom()
    }

    fn set_layout(&mut self, layout: Box<dyn FrameLayout>) {
        self.layout.set(Some(layout));
    }

    fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn FrameLayout>) {
        (
            self.dom.as_mut().expect("the DOM, outside a guest's turn"),
            self.layout.get_mut().as_deref_mut(),
        )
    }

    fn take_layout(&mut self) -> Option<Box<dyn FrameLayout>> {
        self.layout.take()
    }

    fn top_frame(&self) -> FrameId {
        TOP
    }

    fn set_navigation_timing(&mut self, frame: FrameId, timing: dom::NavigationTiming) {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return;
        };
        self.dom().mutate(|cx| {
            if let Some(window) = cx.window_of(document.get(cx.gc)) {
                cx.set_navigation_timing(window, timing);
            }
        });
    }

    fn create_window(
        &mut self,
        frame: FrameId,
        url: &str,
        time_origin: Instant,
    ) -> Option<NodeRoot> {
        if frame != TOP {
            return None;
        }
        // The document before goes, its scripts with it.
        self.drop_scripts();
        let (page_focused, page_hidden) = (self.page_focused, self.page_hidden);
        let document = self.dom().mutate(|cx| {
            let document = cx.create_document(true);
            cx.set_document_url(document, url);
            let window = cx.create_window(document);
            cx.set_window_time_origin(window, time_origin);
            // No element has the focus yet: no events.
            cx.set_page_focused(window, page_focused);
            cx.set_page_hidden(window, page_hidden);
            // The parser's: loading until it is parsed.
            cx.set_ready_state(document, dom::DocumentReadyState::Loading);
            (cx.root(document), cx.node_id(document))
        });
        let (document, id) = document;
        self.document = Some(document.clone());
        self.document_id = Some(id);
        self.requests.clear();
        self.event_handler = dom_api::EventHandler::default();
        Some(document)
    }

    fn take_frame_requests(&mut self) -> Vec<dom_api::FrameRequest> {
        std::mem::take(&mut self.requests)
    }

    fn fetch_response(&mut self, _: u64, _: Result<dom_api::FetchedResponse, ()>) -> bool {
        false
    }

    // JS, in a wasm document: not run.
    fn run_classic_script(&mut self, _: FrameId, _: &dom_api::ClassicScript<'_>) {}

    fn start_module_graph(&mut self, _: &NodeRoot, _: dom_api::ModuleStart) {}

    fn module_fetched(&mut self, _: u64, _: Option<dom_api::module::FetchedModule>) -> bool {
        false
    }

    fn run_wasm_script(&mut self, frame: FrameId, script: &WasmScript<'_>) {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return;
        };
        // Out of `self` while its turn borrows it; back after.
        let runtime = match self.runtime.take().map_or_else(Runtime::new, Ok) {
            Ok(runtime) => runtime,
            Err(error) => return self.report(script.url, format!("{error:?}"), true),
        };
        self.start_script(&runtime, &document, script);
        self.runtime = Some(runtime);
    }

    fn run_ready_script(&mut self, element: &NodeRoot) {
        dom_api::run_script_task(&mut self.host(), element);
        self.perform_microtask_checkpoint();
    }

    /// The DOM's own microtasks (there is no script's queue): the image
    /// updates, the steps of elements that changed.
    fn perform_microtask_checkpoint(&mut self) {
        loop {
            let microtasks = self.dom().take_microtasks();
            if microtasks.is_empty() {
                break;
            }
            if microtasks.images() {
                dom_api::run_image_updates(&mut self.host());
            }
            if microtasks.elements() {
                dom_api::update_elements(&mut self.host());
            }
            if microtasks.animations() {
                dom_api::settle_animation_notices(&mut self.host());
            }
        }
        // What the guests' listeners wrote.
        self.flush_output();
    }

    fn document_ready(&mut self, frame: FrameId, readiness: DocumentReadiness) {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return;
        };
        dom_api::document_ready(&mut self.host(), &document, readiness);
        self.perform_microtask_checkpoint();
    }

    fn unload_document(&mut self, frame: FrameId) -> Option<(Instant, Instant)> {
        let document = self.document.clone().filter(|_| frame == TOP)?;
        let unloaded = dom_api::unload_document(&mut self.host(), &document);
        self.perform_microtask_checkpoint();
        unloaded
    }

    fn fire_event(&mut self, target: &NodeRoot, event_type: dom::NameId) {
        let target = dom::TargetRoot::Node(target.clone());
        dom_api::fire_event(&mut self.host(), &target, event_type, false, false);
        self.perform_microtask_checkpoint();
    }

    fn fire_scroll_event(&mut self, target: NodeId) {
        let Some((target, document)) = self.dom().read(|cx| {
            let node = cx
                .node_by_id(target)
                .filter(|&node| cx.is_connected(node))?;
            Some((cx.root(node), cx.kind(node) == dom::NodeKind::Document))
        }) else {
            return;
        };
        dom_api::fire_event(
            &mut self.host(),
            &dom::TargetRoot::Node(target),
            dom::names::event::SCROLL,
            document,
            false,
        );
        self.perform_microtask_checkpoint();
    }

    fn handle_mouse(&mut self, input: &dom_api::MouseInput) -> dom_api::CursorKeyword {
        let (mut host, event_handler, document) = self.host_and_input();
        let Some(document) = document else {
            return dom_api::CursorKeyword::Default;
        };
        let cursor = event_handler.handle_mouse(&mut host, document, input);
        self.perform_microtask_checkpoint();
        cursor
    }

    fn recompute_under_mouse(&mut self) -> Option<dom_api::CursorKeyword> {
        let (mut host, event_handler, document) = self.host_and_input();
        let document = document?;
        let cursor = event_handler.recompute_under_mouse(&mut host, document);
        self.perform_microtask_checkpoint();
        cursor
    }

    fn handle_wheel(&mut self, input: &dom_api::WheelInput) -> bool {
        let (mut host, event_handler, document) = self.host_and_input();
        let Some(document) = document else {
            return true;
        };
        let not_canceled = event_handler.handle_wheel(&mut host, document, input);
        self.perform_microtask_checkpoint();
        not_canceled
    }

    fn handle_key(&mut self, input: &dom_api::KeyInput) -> dom_api::KeyResult {
        let (mut host, event_handler, document) = self.host_and_input();
        let Some(document) = document else {
            return dom_api::KeyResult::Done;
        };
        let result = event_handler.handle_key(&mut host, document, input);
        self.perform_microtask_checkpoint();
        result
    }

    fn set_initial_focus(&mut self, reverse: bool) {
        let Some(document) = self.document.clone() else {
            return;
        };
        dom_api::set_initial_focus(&mut self.host(), &document, reverse);
        self.perform_microtask_checkpoint();
    }

    fn popup_menu_closed(&mut self, select: NodeId, result: dom_api::PopupResult) {
        if self.document.is_none() {
            return;
        }
        let select = self
            .dom()
            .read(|cx| cx.node_by_id(select).map(|node| cx.root(node)));
        if let Some(select) = select {
            dom_api::popup_menu_closed(&mut self.host(), &select, result);
        }
        self.perform_microtask_checkpoint();
    }

    // No guest asks for an animation frame.
    fn has_animation_frames(&mut self) -> bool {
        false
    }

    fn animations_want_frames(&mut self) -> bool {
        let Some(document) = self.document.clone() else {
            return false;
        };
        self.dom()
            .read(|cx| cx.animations_want_frames(document.get(cx.gc)))
    }

    fn frames_in_tree_order(&mut self) -> Vec<FrameId> {
        self.document.iter().map(|_| TOP).collect()
    }

    fn documents_in_tree_order(&mut self) -> Vec<NodeId> {
        if let Some(documents) = &self.rendering_documents {
            return documents.clone();
        }
        let Some(document) = self.document.clone() else {
            return Vec::new();
        };
        self.dom().read(|cx| vec![cx.node_id(document.get(cx.gc))])
    }

    fn begin_rendering_update(&mut self) {
        self.rendering_documents = None;
        self.rendering_documents = Some(self.documents_in_tree_order());
    }

    fn end_rendering_update(&mut self) {
        self.rendering_documents = None;
    }

    fn run_animation_frames(&mut self, _: Instant) {}

    fn set_page_focused(&mut self, focused: bool) {
        self.page_focused = focused;
        let Some(window) = self.window() else {
            return;
        };
        dom_api::set_page_focused(&mut self.host(), &window, focused);
        self.perform_microtask_checkpoint();
    }

    fn set_page_visible(&mut self, visible: bool) {
        self.page_hidden = !visible;
        let Some(document) = self.document.clone() else {
            return;
        };
        dom_api::set_page_visible(&mut self.host(), &document, visible);
        self.perform_microtask_checkpoint();
    }

    fn webdriver_query(
        &mut self,
        frame: FrameId,
        query: &dom_api::WebDriverQuery<'_>,
    ) -> Result<dom_api::WebDriverAnswer, dom_api::WebDriverQueryError> {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return Err(dom_api::WebDriverQueryError::NotShown);
        };
        let Self { dom, layout, .. } = self;
        let dom = dom.as_mut().expect("the DOM, outside a guest's turn");
        dom.mutate(|cx| {
            dom_api::webdriver_answer(
                &ApiCx::with_layout_cell(cx, layout),
                document.get(cx.gc),
                query,
            )
        })
    }

    fn webdriver_send_keys(
        &mut self,
        frame: FrameId,
        element: NodeId,
        text: &str,
    ) -> Result<bool, dom_api::WebDriverActionError> {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return Err(dom_api::WebDriverActionError::Query(
                dom_api::WebDriverQueryError::NotShown,
            ));
        };
        let result = dom_api::webdriver_send_keys(&mut self.host(), &document, element, text);
        self.perform_microtask_checkpoint();
        result
    }

    fn webdriver_clear(
        &mut self,
        frame: FrameId,
        element: NodeId,
    ) -> Result<(), dom_api::WebDriverActionError> {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return Err(dom_api::WebDriverActionError::Query(
                dom_api::WebDriverQueryError::NotShown,
            ));
        };
        let result = dom_api::webdriver_clear(&mut self.host(), &document, element);
        self.perform_microtask_checkpoint();
        result
    }

    // WebDriver's scripts are JS.
    fn execute_webdriver_script(
        &mut self,
        _: FrameId,
        _: u64,
        _: &str,
        _: &[dom_api::WebDriverValue],
        _: bool,
    ) -> Result<(), dom_api::WebDriverScriptError> {
        Err(dom_api::WebDriverScriptError(
            "a wasm document runs no JavaScript".into(),
        ))
    }

    fn take_webdriver_results(
        &mut self,
    ) -> Vec<(
        u64,
        Result<dom_api::WebDriverValue, dom_api::WebDriverScriptError>,
    )> {
        Vec::new()
    }

    fn webdriver_element_property(
        &mut self,
        _: FrameId,
        _: NodeId,
        _: &str,
    ) -> Result<dom_api::WebDriverValue, dom_api::WebDriverScriptError> {
        Err(dom_api::WebDriverScriptError(
            "a wasm document runs no JavaScript".into(),
        ))
    }

    fn flush_autofocus_candidates(&mut self) {
        let Some(document) = self.document.clone() else {
            return;
        };
        if !self
            .dom()
            .read(|cx| cx.has_autofocus_candidates(document.get(cx.gc)))
        {
            return;
        }
        dom_api::flush_autofocus_candidates(&mut self.host(), &document);
        self.perform_microtask_checkpoint();
    }

    fn run_focus_fixup(&mut self) {
        let Some(document) = self.document.clone() else {
            return;
        };
        dom_api::focus_fixup(&mut self.host(), &document);
        self.perform_microtask_checkpoint();
    }

    fn react_to_environment_changes(&mut self, frame: FrameId) {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return;
        };
        dom_api::react_to_environment_changes(&mut self.host(), &document);
        self.perform_microtask_checkpoint();
    }

    // A media query list is script's.
    fn report_media_query_changes(&mut self, _: FrameId) {}

    fn update_animations(
        &mut self,
        frame: FrameId,
        frame_time: Instant,
    ) -> dom_api::AnimationsUpdate {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return dom_api::AnimationsUpdate::default();
        };
        let (now, any) = self.dom().read(|cx| {
            let document = document.get(cx.gc);
            let now = cx
                .window_of(document)
                .map_or(0.0, |window| cx.time_at(window, frame_time));
            (now, cx.has_animations(document))
        });
        if !any {
            self.dom()
                .read(|cx| cx.tick_animations(document.get(cx.gc), now));
            return dom_api::AnimationsUpdate::default();
        }
        let update = dom_api::update_animations(&mut self.host(), &document, now);
        self.perform_microtask_checkpoint();
        update
    }

    fn set_session_history(&mut self, history: dom::SessionHistory) {
        self.dom().read(|cx| cx.set_session_history(history));
    }

    fn traverse_within_document(
        &mut self,
        frame: FrameId,
        url: &str,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
    ) -> bool {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return false;
        };
        let traversed = dom_api::traverse_within_document(
            &mut self.host(),
            &document,
            url,
            state,
            manual_scroll_restoration,
        );
        self.perform_microtask_checkpoint();
        traversed
    }

    fn restore_history(
        &mut self,
        frame: FrameId,
        state: Option<Vec<u8>>,
        manual_scroll_restoration: bool,
    ) {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return;
        };
        dom_api::restore_history(
            &mut self.host(),
            &document,
            state,
            manual_scroll_restoration,
        );
    }

    fn start_pending_animations(&mut self, frame: FrameId) -> bool {
        let Some(document) = self.document.clone().filter(|_| frame == TOP) else {
            return false;
        };
        let started = self
            .dom()
            .read(|cx| cx.start_pending_animations(document.get(cx.gc)));
        if started {
            dom_api::settle_started_animations(&mut self.host(), &document);
            self.perform_microtask_checkpoint();
        }
        started
            || self
                .dom()
                .read(|cx| cx.animations_want_frames(document.get(cx.gc)))
    }

    /// The window's next task of the DOM's own, or the earliest deadline a
    /// guest waits for, whichever is first.
    fn until_next_timer(&mut self) -> Option<Duration> {
        let now = (self.clock)();
        let guests = self
            .scripts
            .iter()
            .filter_map(|script| script.deadline)
            .min()
            .map(|deadline| Duration::from_nanos(deadline.saturating_sub(now)));
        let document = self.document.clone()?;
        let own = self
            .dom()
            .read(|cx| dom_api::until_next_due(cx, cx.window_of(document.get(cx.gc))?));
        match (guests, own) {
            (Some(guests), Some(own)) => Some(guests.min(own)),
            (guests, own) => guests.or(own),
        }
    }

    fn run_timers(&mut self) -> bool {
        self.run_due_guests()
    }

    /// The guests whose waits are over (the host's loop wakes for them at
    /// `until_next_timer`), then the window's own tasks that are due.
    fn run_due_tasks(&mut self) -> bool {
        let mut ran = self.run_due_guests();
        let Some(window) = self.window() else {
            return ran;
        };
        self.dom().read(|cx| cx.begin_task_pass());
        loop {
            let next = self.dom().read(|cx| {
                let now = cx.dom_now();
                let window = window.get(cx.gc);
                let task = cx.next_task(window, now)?;
                Some((task, now - cx.window_time_origin(window)))
            });
            let Some((task, now)) = next else {
                break;
            };
            if !dom_api::run_task(&mut self.host(), &window, task.queue, now) {
                break;
            }
            ran = true;
        }
        self.dom().read(|cx| cx.end_task_pass(&[window.get(cx.gc)]));
        ran
    }

    fn has_idle_callbacks(&mut self) -> bool {
        false
    }

    fn has_runnable_idle_callback(&mut self) -> bool {
        false
    }

    fn start_idle_period(&mut self) {}

    fn run_idle_callback(&mut self, _: Instant) -> bool {
        false
    }

    // What `isInputPending` asks: script's.
    fn set_input_monitor(&mut self, _: Box<dyn dom_api::InputMonitor>) {}

    fn run_element_tasks(&mut self) -> bool {
        let Some(window) = self.window() else {
            return false;
        };
        let ran = dom_api::run_element_tasks(&mut self.host(), &window) > 0;
        if ran {
            self.perform_microtask_checkpoint();
        }
        ran
    }

    fn run_posted_messages(&mut self) -> bool {
        false
    }

    fn notify_intersection_observers(&mut self) {}

    fn has_resize_observations(&mut self, _: FrameId) -> bool {
        false
    }

    fn gather_and_broadcast_resizes(
        &mut self,
        _: FrameId,
        _: usize,
    ) -> dom_api::ResizeObserverRound {
        dom_api::ResizeObserverRound::default()
    }

    fn report_resize_loop_error(&mut self, _: FrameId) {}

    // The DOM's heap's GC runs on its own workers: no platform tasks.
    fn run_platform_tasks(&mut self) -> bool {
        false
    }

    fn set_console_sink(&mut self, sink: ConsoleSink) {
        self.console = Some(sink);
    }

    // A guest starts no load of its own.
    fn set_load_sink(&mut self, _: Box<dyn dom_api::LoadSink>) {}

    fn take_harness_results(&mut self) -> Option<String> {
        None
    }

    fn set_idle(&mut self, _: bool) {}

    fn set_devtools_frame_ids(&mut self, _: DevToolsFrameIds) {}

    // No inspector: DevTools' sessions hear nothing of script here.
    fn devtools_attach(&mut self, _: String, _: DevToolsSend) {}

    fn devtools_detach(&mut self, _: &str) {}

    fn dispatch_devtools_message(&mut self, _: &str, _: &str) -> bool {
        false
    }

    fn set_devtools_queue(&mut self, _: Box<dyn DevToolsQueue>) -> DevToolsInterrupter {
        Box::new(|| {})
    }

    fn set_stacks_wanted(&mut self, _: bool, _: bool) {}

    fn devtools_page(&mut self) -> Box<dyn DevToolsPage + '_> {
        Box::new(Page {
            dom: self.dom.as_mut().expect("the DOM, outside a guest's turn"),
            layout: &mut self.layout,
            objects: NoObjects,
        })
    }
}

/// The page for DevTools' agents: its DOM and layout, and no objects of
/// script's.
struct Page<'a> {
    dom: &'a mut Dom,
    layout: &'a mut LayoutCell,
    objects: NoObjects,
}

impl DevToolsPage for Page<'_> {
    fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn FrameLayout>) {
        (self.dom, self.layout.get_mut().as_deref_mut())
    }

    fn objects(&mut self) -> &mut dyn DevToolsObjects {
        &mut self.objects
    }
}

struct NoObjects;

impl DevToolsObjects for NoObjects {
    fn resolve_node(&mut self, _: &str, _: NodeId, _: &str) -> Option<String> {
        None
    }

    fn node_of_object(&mut self, _: &str, _: &str) -> Result<NodeId, String> {
        Err("a wasm document has no script objects".into())
    }

    fn inspect_node(&mut self, _: &str, _: NodeId) {}

    fn event_listeners(
        &mut self,
        _: &str,
        _: &str,
        _: i64,
        _: bool,
        _: bool,
    ) -> Result<String, String> {
        Err("a wasm document has no script objects".into())
    }
}
