//! The isolate's DOM (Blink's `V8PerIsolateData`, with the thread's
//! `ThreadState`): its heap, which V8's major GCs mark (the state is the
//! isolate's embedder heap, registered as it is made, as Chrome attaches a
//! thread's cppgc heap to its isolate), and what its bindings keep: the
//! strings' cache, the interfaces' templates and the wrappers'
//! boilerplates. In the isolate's state (`bindings::set_isolate_state_with_embedder_heap`),
//! dropped with the isolate (`V8PerIsolateData`'s drop), borrowed only
//! through the isolate (`Scope`): its holder can't call V8 meanwhile but
//! for what allocates nothing (`bindings::NoGc`), so V8's GC, which reaches
//! the DOM through the state, never runs in an operation of it.

use std::cell::Cell;
use std::sync::Arc;
use std::time::Duration;

use dom::Dom;

use crate::named::NamedCalls;
use crate::strings::StringCache;

/// How far the DOM's used bytes move before V8 is told (cppgc tells it as
/// its allocation buffers fill).
const REPORT_BYTES: usize = 64 << 10;

/// A wrapper boilerplate, and the context it was made in.
type Boilerplate = (v8::Global<v8::Context>, v8::Global<v8::Object>);

/// A scope of an isolate whose state is the DOM's.
pub(crate) type Scope<'a, 's, 'i> = bindings::StateScope<'a, 's, 'i, State>;

pub(crate) struct State {
    /// The heap's counted bytes (`Dom::counted_bytes`) when V8 was last
    /// told its used ones (a major GC's end tells it the live ones itself:
    /// the next report, past `REPORT_BYTES` from these, tells it the size
    /// since).
    reported: usize,
    /// What a member's operation reads (a borrow of it alone, beside the
    /// DOM's: `front::V8Op`).
    pub(crate) caches: Caches,
    /// Each interface's template, by its index in `INTERFACES`, and
    /// `DOMException`'s after them: made at the first use.
    pub(crate) templates: Vec<Option<v8::Global<v8::FunctionTemplate>>>,
    /// Each interface's wrapper boilerplate, wrapping nothing, and its
    /// context: a new wrapper in that context is a copy of it (Blink's
    /// `V8PerContextData`'s wrapper boilerplates); in another, it is made
    /// anew there.
    pub(crate) boilerplates: Vec<Option<Boilerplate>>,
    /// Each interface's named properties' special operations, by its index
    /// in `INTERFACES`: found as its template is made.
    pub(crate) named: Vec<NamedCalls>,
    /// The cross-origin objects' functions' templates, by their numbers in
    /// `cross_origin::FUNCTIONS`: made at the first use.
    pub(crate) cross_origin: Vec<Option<v8::Global<v8::FunctionTemplate>>>,
    /// The mutation observers' microtask's function's template, made at
    /// the first delivery.
    pub(crate) deliver: Option<v8::Global<v8::FunctionTemplate>>,
    /// The backup element queue's microtask's function's template, made
    /// at the first.
    pub(crate) process_reactions: Option<v8::Global<v8::FunctionTemplate>>,
    /// The `<img>`s' image data's microtask's function (`UpdateImages`),
    /// made at the first.
    pub(crate) update_images: Option<v8::Global<v8::FunctionTemplate>>,
    /// The observable array attributes' operations' templates, as their
    /// interfaces' templates are made: what an attribute's accessors are
    /// given the number of (`observable_array`).
    pub(crate) observable: Vec<crate::observable_array::ObservableTemplates>,
    /// The functions of the members returning a promise, each called by
    /// the one installed in its place (`templates::rejecting`), by its
    /// number there.
    pub(crate) rejecting: Vec<v8::Global<v8::FunctionTemplate>>,
    /// The names' V8 strings made (`Caches::names`), for the tests.
    pub(crate) names_made: u64,
    /// The members' string arguments converted by `ToString`
    /// (`strings::arg_string`: none that was a string already), for the
    /// tests.
    pub(crate) string_conversions: u64,
    /// Exceptions listeners and timers threw that no `error` listener
    /// canceled (logged), for the tests.
    pub(crate) uncaught_exceptions: u64,
    /// What `testRunner.reportHarnessResults` was last given, until the
    /// host takes it (`ScriptEngine::take_harness_results`).
    pub(crate) harness_results: Option<String>,
    /// The WebDriver scripts that have settled since the host last asked
    /// (`ScriptEngine::take_webdriver_results`): their ids and results.
    pub(crate) webdriver_results: Vec<(
        u64,
        Result<dom_api::WebDriverValue, dom_api::WebDriverScriptError>,
    )>,
    /// The resolvers of the DOM's pending promises (`whenDefined`'s,
    /// `decode()`'s), by the number the DOM keeps for each
    /// (`Cx::add_when_defined`, `Cx::image_decode`): V8's handles,
    /// which the DOM's heap can't hold but as an object's (a resolver
    /// isn't one V8 lets be cast back to). A registry gone with a promise
    /// pending leaves it here, as a window's going does now.
    pub(crate) when_defined: rustc_hash::FxHashMap<u64, v8::Global<v8::PromiseResolver>>,
    /// What tells `isInputPending()` of the input waiting, the host's
    /// (`ScriptEngine::set_input_monitor`); none without one.
    pub(crate) input_monitor: Option<Box<dyn dom_api::InputMonitor>>,
    /// What DevTools calls the frames, the host's
    /// (`ScriptEngine::set_devtools_frame_ids`): a realm is told to the
    /// inspector with its frame's as it is made, in a call that inserts an
    /// iframe too.
    pub(crate) devtools_frame_ids: Option<dom_api::DevToolsFrameIds>,
    /// The frame's layout, the host's (`ScriptEngine::set_layout`): lent to
    /// each operation by a move (`dom_api::ApiCx::with_layout_cell`).
    /// In a box of its own: in the state, beside the fields each call
    /// writes, every operation that is handed its address ran slower
    /// (`setAttribute` 256 → 332 cycles, the same instructions), though
    /// none touched it.
    pub(crate) layout: Box<dom_api::LayoutCell>,
    /// The browsing contexts: their realms and documents (`frames`).
    /// Before the DOM: their roots go first.
    pub(crate) frames: crate::frames::Frames,
    /// The DOM: each operation borrows it from the isolate (`Front::read`,
    /// `Front::mutate`); V8's major GCs between them (`EmbedderHeapState`).
    /// Last, as fields drop in order: the roots above (the frame's, in
    /// `layout`) go before their heap.
    pub(crate) dom: Dom,
}

/// The state's caches a member's operation reads.
pub(crate) struct Caches {
    pub(crate) strings: StringCache,
    /// The names table's V8 strings, by number, made the first time one is
    /// returned: held strongly, as the names are (the table never shrinks).
    pub(crate) names: Vec<Option<v8::Global<v8::String>>>,
    /// Wrappers made, for the tests: counted in an operation too, where the
    /// caches are shared.
    pub(crate) wrappers_made: Cell<u64>,
    /// Wrappers made ahead of a new node (`Front::prepare_new_node`) that
    /// weren't its interface's, for the tests.
    pub(crate) wrappers_wasted: Cell<u64>,
}

impl bindings::EmbedderHeapState for State {
    fn start(&mut self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
        Some(self.dom.start_marking(sink))
    }

    fn advance(&mut self, budget: Duration, atomic: bool) -> bool {
        self.dom.advance_marking(atomic, budget)
    }

    fn end(&mut self) -> usize {
        self.dom.finish_marking()
    }

    /// The DOM in a heap snapshot, after V8's GC: every object of its heap,
    /// a node detached when it is out of every document's tree (Blink's
    /// `V8EmbedderGraphBuilder`).
    fn snapshot(&mut self, graph: &mut bindings::HeapGraph<'_, '_, '_, '_>) {
        self.dom.walk(|cx, walk| {
            graph.add_heap(walk, |object| match cx.is_detached(object.get()) {
                Some(true) => bindings::Detachedness::Detached,
                Some(false) => bindings::Detachedness::Attached,
                None => bindings::Detachedness::Unknown,
            });
        });
    }
}

/// The DOM's heap's options. No nursery: what has a wrapper is old from its
/// allocation (V8's reference to it never moves), the data hung on it with
/// it, and the few short-lived objects left were no cheaper scavenged than
/// left to the old generation's GCs (Blink's heap has no young generation
/// either). `MACHOPO_GC_OPTIONS` (`given`, comma-separated) changes them for
/// the GC's test runs: `stress` (a GC's work at every safepoint), `verify`
/// (the heap checked after each GC), `nursery=BYTES`; an option it doesn't
/// know is warned of and left out, as V8 does an unknown flag.
pub(crate) fn heap_options(given: Option<&str>) -> elise::HeapOptions {
    let mut options = elise::HeapOptions {
        driven: true,
        nursery_bytes: 0,
        ..elise::HeapOptions::default()
    };
    for option in given
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|option| !option.is_empty())
    {
        match option.split_once('=') {
            None if option == "stress" => options.stress = true,
            None if option == "verify" => options.verify = true,
            Some(("nursery", bytes)) if let Ok(bytes) = bytes.parse() => {
                options.nursery_bytes = bytes;
            }
            _ => tracing::warn!(
                option,
                "MACHOPO_GC_OPTIONS: an option the DOM's heap doesn't know"
            ),
        }
    }
    options
}

/// Makes the isolate's DOM and registers it with V8, unless it has it.
///
/// # Panics
///
/// If the isolate has another embedder state.
pub(crate) fn install(isolate: &mut v8::Isolate) {
    if bindings::isolate_state_mut::<State>(isolate).is_some() {
        return;
    }
    // And `DOMException`'s and `QuotaExceededError`'s.
    let slots = dom_api::INTERFACES.len().saturating_add(2);
    bindings::set_isolate_state_with_embedder_heap(
        isolate,
        State {
            dom: Dom::with_platform(
                heap_options(std::env::var("MACHOPO_GC_OPTIONS").ok().as_deref()),
                bindings::gc_platform(),
            ),
            reported: 0,
            caches: Caches {
                strings: StringCache::new(),
                names: Vec::new(),
                wrappers_made: Cell::new(0),
                wrappers_wasted: Cell::new(0),
            },
            templates: (0..slots).map(|_| None).collect(),
            boilerplates: (0..slots).map(|_| None).collect(),
            named: vec![NamedCalls::default(); slots],
            cross_origin: (0..crate::cross_origin::FUNCTION_COUNT)
                .map(|_| None)
                .collect(),
            deliver: None,
            process_reactions: None,
            update_images: None,
            observable: Vec::new(),
            rejecting: Vec::new(),
            names_made: 0,
            string_conversions: 0,
            uncaught_exceptions: 0,
            harness_results: None,
            webdriver_results: Vec::new(),
            when_defined: rustc_hash::FxHashMap::default(),
            input_monitor: None,
            devtools_frame_ids: None,
            layout: Box::new(std::cell::Cell::new(None)),
            frames: crate::frames::Frames::default(),
        },
    );
    // A delete, a define or a prototype set across origins: a
    // `SecurityError`, not V8's `TypeError`.
    bindings::set_state_refused_access::<State, crate::cross_origin::CrossOrigin>(isolate);
    isolate.set_host_import_module_dynamically_callback(crate::modules::import_dynamically);
    // `Error.isError` of a `DOMException`.
    bindings::set_native_error_check(isolate, crate::exception::is_dom_exception);
}

/// `scope`, if its isolate has the DOM (checked once).
pub(crate) fn scope<'a, 's, 'i>(scope: &'a mut v8::PinScope<'s, 'i>) -> Option<Scope<'a, 's, 'i>> {
    bindings::StateScope::new(scope)
}

/// After an operation that may allocate (or change the DOM): the mutation
/// observers' microtask queued if the operation queued the first record;
/// V8 told the DOM's used bytes once they moved enough, as V8 may start or
/// advance a GC now, the state no longer borrowed.
pub(crate) fn report(scope: &mut Scope<'_, '_, '_>) {
    let (microtasks, used) = {
        let (state, _) = scope.state_mut();
        let microtasks = state.dom.take_microtasks();
        // How far it moved, by what the heap counts (at once, its buffers
        // whole); what it uses, exactly, only when V8 is told.
        let counted = state.dom.counted_bytes();
        let moved = counted.abs_diff(state.reported) >= REPORT_BYTES;
        if moved {
            state.reported = counted;
        }
        (microtasks, moved.then(|| state.dom.used_bytes()))
    };
    if !microtasks.is_empty() {
        queue_microtasks(scope, microtasks);
    }
    if let Some(used) = used {
        bindings::report_embedder_used_size(scope.scope(), used);
    }
}

/// HTML's "queue a mutation observer microtask": a microtask that delivers
/// the records (`dom_api::deliver_mutations`), queued after the
/// operation that queued the first since the last delivery (V8 allocates:
/// not in one). Out of line: most calls queue none.
#[cold]
#[inline(never)]
pub(crate) fn queue_mutation_delivery(scope: &mut Scope<'_, '_, '_>) {
    let cached = {
        let (state, mut no_gc) = scope.state();
        state.deliver.as_ref().map(|template| no_gc.local(template))
    };
    let template = match cached {
        Some(template) => template,
        None => {
            let template =
                bindings::state_function_template::<State, Deliver>(scope.scope(), |builder| {
                    builder
                });
            let global = v8::Global::new(scope.scope(), template);
            scope.state_mut().0.deliver = Some(global);
            template
        }
    };
    let scope = scope.scope();
    if let Some(function) = template.get_function(scope) {
        scope.enqueue_microtask(function);
    }
}

/// The iframes' and script elements' steps for what happened to them in
/// the operation, before the call returns (`dom_api::update_elements`),
/// or before the custom element reactions it left. Out of line: most calls
/// change none.
#[cold]
#[inline(never)]
pub(crate) fn update_elements(scope: &mut Scope<'_, '_, '_>) {
    dom_api::update_elements(&mut crate::listeners::Host { scope });
}

/// The microtasks the operations asked for, queued, and the iframes' and
/// scripts' steps they left run: out of line, most calls queue none.
#[cold]
#[inline(never)]
fn queue_microtasks(scope: &mut Scope<'_, '_, '_>, microtasks: dom::Microtasks) {
    if microtasks.mutations() {
        queue_mutation_delivery(scope);
    }
    if microtasks.reactions() {
        queue_backup_element_queue(scope);
    }
    if microtasks.images() {
        queue_image_updates(scope);
    }
    // Not a microtask: the iframes' and scripts' steps, before the call
    // returns.
    if microtasks.elements() {
        update_elements(scope);
    }
    // Neither: the promises the operation's animations settled, before the
    // call returns.
    if microtasks.animations() {
        dom_api::settle_animation_notices(&mut crate::listeners::Host { scope });
    }
    // Nor the stack of the script that asked for loads in the operation,
    // for DevTools (V8 allocates: not in one).
    if microtasks.stacks() {
        let stack = capture_stack(scope);
        scope.state().0.dom.note_request_stacks(stack);
    }
    // Nor the `fetch()`es: sent before the call returns, after the blob
    // URLs the operation registered (`update_elements`).
    if microtasks.fetches() {
        crate::frames::send_fetches(scope);
    }
}

/// The stack of the script running, with every async parent, as DevTools'
/// `Runtime.StackTrace` (JSON): what a request's or a frame's initiator
/// shows. None outside script, or without the inspector.
pub(crate) fn capture_stack(scope: &mut Scope<'_, '_, '_>) -> Option<std::rc::Rc<str>> {
    bindings::devtools::capture_stack(scope.scope(), i32::MAX).map(std::rc::Rc::from)
}

/// HTML's microtask of "update the image data" (`Cx::take_image_updates`),
/// queued after the operation that queued the first since the last.
#[cold]
#[inline(never)]
fn queue_image_updates(scope: &mut Scope<'_, '_, '_>) {
    let cached = {
        let (state, mut no_gc) = scope.state();
        state
            .update_images
            .as_ref()
            .map(|template| no_gc.local(template))
    };
    let template = match cached {
        Some(template) => template,
        None => {
            let template = bindings::state_function_template::<State, UpdateImages>(
                scope.scope(),
                |builder| builder,
            );
            let global = v8::Global::new(scope.scope(), template);
            scope.state_mut().0.update_images = Some(global);
            template
        }
    };
    let scope = scope.scope();
    if let Some(function) = template.get_function(scope) {
        scope.enqueue_microtask(function);
    }
}

/// The `<img>`s' image data's microtask, out of any operation.
struct UpdateImages;

impl bindings::StateCallback<State> for UpdateImages {
    fn call<'s>(
        mut scope: Scope<'_, 's, '_>,
        _: v8::FunctionCallbackArguments<'s>,
        _: v8::ReturnValue<'s>,
    ) {
        dom_api::run_image_updates(&mut crate::listeners::Host { scope: &mut scope });
        report(&mut scope);
    }
}

/// HTML's microtask processing the backup element queue (the elements
/// whose reactions were enqueued out of any `[CEReactions]` member: the
/// parser's, an event's), queued after the operation that put the first
/// in it. Out of line: most calls queue none.
#[cold]
#[inline(never)]
fn queue_backup_element_queue(scope: &mut Scope<'_, '_, '_>) {
    let cached = {
        let (state, mut no_gc) = scope.state();
        state
            .process_reactions
            .as_ref()
            .map(|template| no_gc.local(template))
    };
    let template = match cached {
        Some(template) => template,
        None => {
            let template = bindings::state_function_template::<State, ProcessReactions>(
                scope.scope(),
                |builder| builder,
            );
            let global = v8::Global::new(scope.scope(), template);
            scope.state_mut().0.process_reactions = Some(global);
            template
        }
    };
    let scope = scope.scope();
    if let Some(function) = template.get_function(scope) {
        scope.enqueue_microtask(function);
    }
}

/// The backup element queue's microtask: its reactions invoked, out of
/// any operation.
struct ProcessReactions;

impl bindings::StateCallback<State> for ProcessReactions {
    fn call<'s>(
        mut scope: Scope<'_, 's, '_>,
        _: v8::FunctionCallbackArguments<'s>,
        _: v8::ReturnValue<'s>,
    ) {
        dom_api::process_backup_element_queue(&mut crate::listeners::Host { scope: &mut scope });
        report(&mut scope);
    }
}

/// The mutation observers' microtask: the records delivered, out of any
/// operation.
struct Deliver;

impl bindings::StateCallback<State> for Deliver {
    fn call<'s>(
        mut scope: Scope<'_, 's, '_>,
        _: v8::FunctionCallbackArguments<'s>,
        _: v8::ReturnValue<'s>,
    ) {
        dom_api::deliver_mutations(&mut crate::listeners::Host { scope: &mut scope });
        // What the callbacks changed may queue another delivery.
        report(&mut scope);
    }
}

#[cfg(test)]
mod tests {
    use super::heap_options;

    #[test]
    fn the_heaps_options_are_the_renderers_unless_given() {
        let options = heap_options(None);
        assert!(options.driven && !options.stress && !options.verify);
        assert_eq!(options.nursery_bytes, 0);
        let options = heap_options(Some(" stress, verify,nursery=1048576,"));
        assert!(options.driven && options.stress && options.verify);
        assert_eq!(options.nursery_bytes, 1 << 20);
        // Unknown and malformed ones are left out.
        let options = heap_options(Some("fast,nursery=many,stress"));
        assert!(options.stress && !options.verify);
        assert_eq!(options.nursery_bytes, 0);
    }
}
