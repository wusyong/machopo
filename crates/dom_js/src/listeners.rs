//! A listener or a timer called (`ScriptHost`, Blink's
//! `JSEventListener::InvokeInternal` and `ScheduledAction::Execute`): out
//! of any operation of the DOM. A listener's callback is a function called
//! with the event's current target as `this`, or an object whose
//! `handleEvent` is (WebIDL's "call a user object's operation"), the event
//! its argument; a timer's is a function called with its window as `this`
//! and its arguments. What either throws is reported at the window of the
//! callback's realm (HTML's "report an exception": an `ErrorEvent`), not
//! rethrown: the dispatch, or the timers, go on.

use dom::{ErrorInfo, ObjectRef, ObjectRoot, Timer, Window};
use dom_api::{ApiCx, Reported, ScriptHost};
use elise::Persistent;

use crate::front::wrapper_of;
use crate::state::{self, Scope};
use crate::templates::string;

/// A wrapper there is, or the object (and its interface) to wrap.
pub(crate) enum Have<'s> {
    Wrapper(v8::Local<'s, v8::Object>),
    Make(ObjectRoot, usize),
}

// Inlined: a listener's call takes two (the target's, the event's), and
// with the observers' callbacks calling it too it no longer was.
#[inline(always)]
pub(crate) fn have<'s, 'gc>(
    no_gc: &mut bindings::NoGc<'_, 's, '_>,
    cx: &dom::Cx<'_, 'gc>,
    object: ObjectRef<'gc>,
) -> Have<'s> {
    match no_gc.wrapper(cx.object_wrapper_edge(object)) {
        Some(wrapper) => Have::Wrapper(wrapper),
        None => Have::Make(
            cx.root_object(object),
            dom_api::interface_of(&ApiCx::new(cx), object),
        ),
    }
}

pub(crate) fn wrap<'s>(scope: &mut Scope<'_, 's, '_>, have: Have<'s>) -> v8::Local<'s, v8::Object> {
    match have {
        Have::Wrapper(wrapper) => wrapper,
        Have::Make(object, interface) => wrapper_of(scope, &object, interface),
    }
}

/// V8 as the DOM's script host out of any member's call: what runs the
/// timers (`run_timers`), reports an exception (the `error` event's
/// listeners) and a rejection; the values it stores, V8's.
pub(crate) struct Host<'a, 'b, 's, 'i> {
    pub(crate) scope: &'a mut Scope<'b, 's, 'i>,
}

impl<'s> ScriptHost for Host<'_, '_, 's, '_> {
    type Value = v8::Local<'s, v8::Value>;

    fn read<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        let (state, _) = self.scope.state();
        state
            .dom
            .read(|cx| f(&ApiCx::with_layout_cell(cx, &state.layout)))
    }

    fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        let (state, _) = self.scope.state_mut();
        let crate::state::State { dom, layout, .. } = state;
        dom.mutate(|cx| f(&ApiCx::with_layout_cell(cx, layout)))
    }

    /// Stores as strong edges of any value (an exception, a promise, a
    /// reason), which writing a traced handle allows in an operation.
    fn mutate_storing<R>(
        &mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut dyn FnMut(&elise::JsEdge, Self::Value)) -> R,
    ) -> R {
        let (state, mut no_gc) = self.scope.state_mut();
        state.dom.mutate(|cx| {
            f(&ApiCx::new(cx), &mut |edge, value| {
                no_gc.set_strong_value(edge, value)
            })
        })
    }

    fn call_listener(&mut self, event: &Persistent<dom::Event>, index: usize) {
        call_listener(self.scope, event, index, true);
    }

    fn compile_event_handler(&mut self, target: &dom::TargetRoot, event_type: dom::NameId) {
        crate::handlers::compile_event_handler(self.scope, target, event_type);
    }

    fn call_timer(&mut self, window: &Persistent<Window>, timer: &Persistent<Timer>) {
        // Its checkpoint is the caller's: the timer's nesting level goes
        // back to 0 first.
        call_function(self.scope, |scope| call_timer(scope, window, timer));
    }

    /// In the realm of the frame showing `window`'s document (the current
    /// one's for a window no frame shows), its ports' wrappers made there.
    fn deserialize_message(
        &mut self,
        window: &Persistent<Window>,
        payload: dom::SerializedMessage,
        ports: &[Persistent<dom::EventTargetImpl>],
    ) -> Option<Self::Value> {
        let realm = {
            let (state, _) = self.scope.state();
            let document = state
                .dom
                .read(|cx| cx.node_id(cx.window_document(window.get(cx.gc))));
            state
                .frames
                .of_document(document)
                .map(|frame| frame.context.clone())
        };
        let scope = self.scope.scope();
        let context = match &realm {
            Some(realm) => v8::Local::new(scope, realm),
            None => scope.get_current_context(),
        };
        let scope = &mut v8::ContextScope::new(scope, context);
        let mut scope = crate::state::scope(scope)?;
        let wrappers: Vec<_> = ports
            .iter()
            .map(|port| {
                let have = {
                    let (state, mut no_gc) = scope.state();
                    state
                        .dom
                        .read(|cx| have(&mut no_gc, cx, ObjectRef::EventTarget(port.get(cx.gc))))
                };
                let wrapper = wrap(&mut scope, have);
                v8::Global::new(scope.scope(), wrapper)
            })
            .collect();
        crate::serialize::deserialize_message(scope.scope(), payload, wrappers)
    }

    fn call_animation_frame(
        &mut self,
        _: &Persistent<Window>,
        frame: &Persistent<dom::AnimationFrame>,
        time: f64,
    ) {
        run_callback(self.scope, |scope| call_animation_frame(scope, frame, time));
    }

    fn run_classic_script(
        &mut self,
        document: &dom::NodeRoot,
        script: &dom_api::ClassicScript<'_>,
    ) {
        run_script(self.scope, |scope| {
            crate::frames::run_classic_script(scope, document, script)
        });
    }

    fn start_module_graph(&mut self, document: &dom::NodeRoot, start: dom_api::ModuleStart) {
        crate::modules::start_graph(self.scope, document, start);
    }

    fn run_module_script(&mut self, document: &dom::NodeRoot, root: u32) {
        run_script(self.scope, |scope| {
            crate::modules::run_module_script(scope, document, root)
        });
    }

    fn preload_module(&mut self, link: &dom::NodeRoot) {
        crate::modules::preload(self.scope, link);
    }

    fn create_child_window(&mut self, owner: &dom::NodeRoot) -> Option<dom::NodeRoot> {
        crate::frames::create_child(self.scope, owner)
    }

    fn navigate_child(&mut self, owner: &dom::NodeRoot, navigation: dom_api::ChildNavigation) {
        crate::frames::navigate_child(self.scope, owner, navigation);
    }

    fn navigate(&mut self, document: &dom::NodeRoot, navigation: dom::FrameNavigation) {
        crate::frames::navigate(self.scope, document, navigation);
    }

    fn register_blob_url(&mut self, document: &dom::NodeRoot, url: &str, data: &dom::BlobData) {
        crate::frames::register_blob_url(self.scope, document, url, data);
    }

    fn revoke_blob_url(&mut self, url: &str) {
        crate::frames::revoke_blob_url(self.scope, url);
    }

    fn copy_history_state(
        &mut self,
        event: &Persistent<dom::Event>,
        history: &Persistent<dom::History>,
    ) {
        let state = crate::front::history_state(self.scope, history);
        let (state_, mut no_gc) = self.scope.state();
        state_.dom.read(|cx| {
            if let Some((edge, _)) = cx.pop_state_event_data(event.get(cx.gc)) {
                no_gc.set_strong_value(edge, state);
            }
        });
    }

    fn discard_child(&mut self, document: &dom::NodeRoot) {
        crate::frames::discard_child(self.scope, document);
    }

    fn perform_microtask_checkpoint(&mut self) {
        perform_microtask_checkpoint(self.scope);
    }

    fn call_mutation_callback(
        &mut self,
        observer: &Persistent<dom::MutationObserver>,
        records: &[Persistent<dom::MutationRecord>],
    ) {
        run_callback(self.scope, |scope| {
            call_mutation_callback(scope, observer, records)
        });
    }

    fn upgrade_element(
        &mut self,
        definition: &Persistent<dom::CustomElementDefinition>,
        element: &Persistent<dom::Node>,
    ) -> bool {
        run_constructor(self.scope, |scope| {
            crate::front::upgrade_element(scope, definition, element)
        })
    }

    fn call_intersection_callback(
        &mut self,
        observer: &Persistent<dom::IntersectionObserver>,
        entries: &[Persistent<dom::IntersectionObserverEntry>],
    ) {
        run_callback(self.scope, |scope| {
            call_intersection_callback(scope, observer, entries)
        });
    }

    fn call_blob_callback(&mut self, callback: &Persistent<dom::CanvasBlobCallback>) {
        run_callback(self.scope, |scope| call_blob_callback(scope, callback));
    }

    fn call_resize_callback(
        &mut self,
        observer: &Persistent<dom::ResizeObserver>,
        entries: &[Persistent<dom::ResizeObserverEntry>],
    ) {
        run_callback(self.scope, |scope| {
            call_resize_callback(scope, observer, entries)
        });
    }

    fn call_performance_callback(
        &mut self,
        observer: &Persistent<dom::PerformanceObserver>,
        list: &Persistent<dom::PerformanceObserverEntryList>,
        dropped: Option<u32>,
    ) {
        run_callback(self.scope, |scope| {
            call_performance_callback(scope, observer, list, dropped)
        });
    }

    fn call_idle_callback(
        &mut self,
        callback: &Persistent<dom::IdleCallback>,
        deadline: &Persistent<dom::IdleDeadline>,
    ) {
        run_callback(self.scope, |scope| {
            call_idle_callback(scope, callback, deadline)
        });
    }

    fn call_geolocation_error(
        &mut self,
        request: &Persistent<dom::GeoRequest>,
        error: &Persistent<dom::GeolocationPositionError>,
    ) {
        run_callback(self.scope, |scope| {
            call_geolocation_error(scope, request, error)
        });
    }

    fn resolve_promise_with(&mut self, promise: u64, object: &ObjectRoot) {
        in_script(self.scope, |scope| {
            resolve_promise_with(scope, promise, object)
        });
    }

    fn resolve_promise_with_read(&mut self, promise: u64, read: dom::BlobRead) {
        in_script(self.scope, |scope| {
            crate::front::resolve_promise_with_read(scope, promise, read)
        });
    }

    fn pending_input(&mut self, include_continuous: bool) -> Vec<dom_api::PendingInput> {
        pending_input(self.scope, include_continuous)
    }

    fn set_abort_reason(
        &mut self,
        signal: &Persistent<dom::EventTargetImpl>,
        name: dom_api::ExceptionName,
        message: &str,
    ) {
        set_abort_reason(self.scope, signal, name, message);
    }

    fn reject_promise_with_reason(
        &mut self,
        promise: u64,
        signal: &Persistent<dom::EventTargetImpl>,
    ) {
        in_script(self.scope, |scope| {
            reject_promise_with_reason(scope, promise, signal)
        });
    }

    fn settle_animation_notices(&mut self, object: &Persistent<dom::EventTargetImpl>) {
        in_script(self.scope, |scope| {
            crate::front::settle_animation_notices(scope, object)
        });
    }

    fn settle_promises(&mut self, promises: &[u64], error: Option<dom_api::ApiError>) {
        in_script(self.scope, |scope| {
            crate::front::settle_promises(scope, promises, error.as_ref())
        });
    }

    fn call_lifecycle_callback(
        &mut self,
        element: &Persistent<dom::Node>,
        definition: &Persistent<dom::CustomElementDefinition>,
        callback: dom_api::LifecycleCallback,
    ) {
        run_callback(self.scope, |scope| {
            crate::front::call_lifecycle_callback(scope, element, definition, callback)
        });
    }
}

/// The engine's entry into script for a callback (HTML's "prepare to run
/// script" and "clean up after running script"): `call_function`'s, then,
/// as it returns with no script left on the stack, a microtask
/// checkpoint: between the listeners of an event the UA dispatches, after
/// a task's callback, not in a `dispatchEvent()` from script (Blink's
/// `V8RunMicrotasksScope` around each call).
pub(crate) fn run_callback<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    then_checkpoint(scope, |scope| call_function(scope, f))
}

/// `run_callback` for a classic or module script: script, but no call of
/// a function (its own span is `EvaluateScript`'s or V8's
/// `Evaluation::evaluate`'s).
pub(crate) fn run_script<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    then_checkpoint(scope, |scope| {
        let _entry = bindings::trace::enter_script();
        f(scope)
    })
}

/// `run_callback` for a custom element's constructor, constructed
/// (`call_constructor`).
pub(crate) fn run_constructor<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    then_checkpoint(scope, |scope| call_constructor(scope, f))
}

/// `f`, then a microtask checkpoint if it left no script on the stack.
fn then_checkpoint<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    let result = f(scope);
    if !bindings::trace::in_script() {
        perform_microtask_checkpoint(scope);
    }
    result
}

/// A call of a callback function or of a callback interface's operation,
/// in script while `f` runs (`bindings::trace`'s count, which the DOM's
/// members check they are called in): Chrome's trace's `v8.callFunction`
/// around it, and a `FunctionCall` too when it is script's outermost
/// (Blink's `V8ScriptRunner::CallFunction`).
pub(crate) fn call_function<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    let _call = trace_event::trace_event!("v8.callFunction");
    let entry = bindings::trace::enter_script();
    let _outermost = if entry.is_outermost() {
        trace_event::trace_event!("FunctionCall")
    } else {
        None
    };
    f(scope)
}

/// A custom element's constructor constructed, in script while `f` runs:
/// Chrome's trace's `v8.callAsConstructor` around it (Blink's
/// `V8ScriptRunner::CallAsConstructor`).
pub(crate) fn call_constructor<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    let _span = trace_event::trace_event!("v8.callAsConstructor");
    let _entry = bindings::trace::enter_script();
    f(scope)
}

/// Script entered without a checkpoint after: a promise settled (its
/// value's `then` getter may run; Blink's resolver runs no microtasks).
fn in_script<'s, R>(
    scope: &mut Scope<'_, 's, '_>,
    f: impl FnOnce(&mut Scope<'_, 's, '_>) -> R,
) -> R {
    let _entry = bindings::trace::enter_script();
    f(scope)
}

/// Mutation observers' delivery queued first, as the engine's own
/// checkpoint does; unhandled rejections wait for that one. The microtasks
/// are script.
fn perform_microtask_checkpoint(scope: &mut Scope<'_, '_, '_>) {
    let _span = trace_event::trace_event!("RunMicrotasks");
    let _entry = bindings::trace::enter_script();
    crate::state::report(scope);
    scope.scope().perform_microtask_checkpoint();
}

/// DOM's "notify mutation observers" for one observer: its callback
/// called with its records (a new array) and it, `this` it; what it throws
/// reported.
fn call_mutation_callback(
    scope: &mut Scope<'_, '_, '_>,
    observer: &Persistent<dom::MutationObserver>,
    records: &[Persistent<dom::MutationRecord>],
) {
    let (callback, this, records) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let observer = observer.get(cx.gc);
            let records: Vec<Have<'_>> = records
                .iter()
                .map(|record| have(&mut no_gc, cx, ObjectRef::MutationRecord(record.get(cx.gc))))
                .collect();
            (
                no_gc.wrapper(cx.observer_callback_edge(observer)),
                have(&mut no_gc, cx, ObjectRef::MutationObserver(observer)),
                records,
            )
        })
    };
    let Some(callback) = callback else {
        return;
    };
    let this = wrap(scope, this);
    let records: Vec<v8::Local<'_, v8::Value>> = records
        .into_iter()
        .map(|record| wrap(scope, record).into())
        .collect();
    let records = v8::Array::new_with_elements(scope.scope(), &records);
    v8::tc_scope!(let scope, scope.scope());
    let called = v8::Local::<v8::Function>::try_from(callback)
        .ok()
        .and_then(|function| function.call(scope, this.into(), &[records.into(), this.into()]));
    if called.is_none() {
        report_caught(scope, ReportAt::RealmOf(callback));
    }
}

/// Intersection Observer's "notify intersection observers" for one
/// observer: its callback called with its entries (a new array) and it,
/// `this` it; what it throws reported.
fn call_intersection_callback(
    scope: &mut Scope<'_, '_, '_>,
    observer: &Persistent<dom::IntersectionObserver>,
    entries: &[Persistent<dom::IntersectionObserverEntry>],
) {
    let (callback, this, entries) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let observer = observer.get(cx.gc);
            let entries: Vec<Have<'_>> = entries
                .iter()
                .map(|entry| {
                    have(
                        &mut no_gc,
                        cx,
                        ObjectRef::IntersectionObserverEntry(entry.get(cx.gc)),
                    )
                })
                .collect();
            (
                no_gc.wrapper(cx.intersection_callback_edge(observer)),
                have(&mut no_gc, cx, ObjectRef::IntersectionObserver(observer)),
                entries,
            )
        })
    };
    let Some(callback) = callback else {
        return;
    };
    let this = wrap(scope, this);
    let entries: Vec<v8::Local<'_, v8::Value>> = entries
        .into_iter()
        .map(|entry| wrap(scope, entry).into())
        .collect();
    let entries = v8::Array::new_with_elements(scope.scope(), &entries);
    v8::tc_scope!(let scope, scope.scope());
    let called = v8::Local::<v8::Function>::try_from(callback)
        .ok()
        .and_then(|function| function.call(scope, this.into(), &[entries.into(), this.into()]));
    if called.is_none() {
        report_caught(scope, ReportAt::RealmOf(callback));
    }
}

/// Performance Timeline's delivery to one observer: its callback called
/// with the entry list, it, and the options (a new object, with
/// `droppedEntriesCount` if given), `this` it; what it throws reported.
fn call_performance_callback(
    scope: &mut Scope<'_, '_, '_>,
    observer: &Persistent<dom::PerformanceObserver>,
    list: &Persistent<dom::PerformanceObserverEntryList>,
    dropped: Option<u32>,
) {
    let (callback, this, list) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let observer = observer.get(cx.gc);
            (
                no_gc.wrapper(cx.performance_observer_callback_edge(observer)),
                have(&mut no_gc, cx, ObjectRef::PerformanceObserver(observer)),
                have(
                    &mut no_gc,
                    cx,
                    ObjectRef::PerformanceObserverEntryList(list.get(cx.gc)),
                ),
            )
        })
    };
    let Some(callback) = callback else {
        return;
    };
    let this = wrap(scope, this);
    let list = wrap(scope, list);
    let options = v8::Object::new(scope.scope());
    if let Some(dropped) = dropped {
        let key = crate::templates::string(scope.scope(), "droppedEntriesCount");
        let value = v8::Integer::new_from_unsigned(scope.scope(), dropped);
        options.create_data_property(scope.scope(), key.into(), value.into());
    }
    v8::tc_scope!(let scope, scope.scope());
    let called = v8::Local::<v8::Function>::try_from(callback)
        .ok()
        .and_then(|function| {
            function.call(
                scope,
                this.into(),
                &[list.into(), this.into(), options.into()],
            )
        });
    if called.is_none() {
        report_caught(scope, ReportAt::RealmOf(callback));
    }
}

/// Resize Observer's "broadcast active resize observations" for one
/// observer: its callback called with its entries (a new array) and it,
/// `this` it; what it throws reported.
fn call_resize_callback(
    scope: &mut Scope<'_, '_, '_>,
    observer: &Persistent<dom::ResizeObserver>,
    entries: &[Persistent<dom::ResizeObserverEntry>],
) {
    let (callback, this, entries) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let observer = observer.get(cx.gc);
            let entries: Vec<Have<'_>> = entries
                .iter()
                .map(|entry| {
                    have(
                        &mut no_gc,
                        cx,
                        ObjectRef::ResizeObserverEntry(entry.get(cx.gc)),
                    )
                })
                .collect();
            (
                no_gc.wrapper(cx.resize_callback_edge(observer)),
                have(&mut no_gc, cx, ObjectRef::ResizeObserver(observer)),
                entries,
            )
        })
    };
    let Some(callback) = callback else {
        return;
    };
    let this = wrap(scope, this);
    let entries: Vec<v8::Local<'_, v8::Value>> = entries
        .into_iter()
        .map(|entry| wrap(scope, entry).into())
        .collect();
    let entries = v8::Array::new_with_elements(scope.scope(), &entries);
    v8::tc_scope!(let scope, scope.scope());
    let called = v8::Local::<v8::Function>::try_from(callback)
        .ok()
        .and_then(|function| function.call(scope, this.into(), &[entries.into(), this.into()]));
    if called.is_none() {
        report_caught(scope, ReportAt::RealmOf(callback));
    }
}

/// How an event handler's listener is called (HTML's "the event handler
/// processing algorithm"), as a plain one isn't.
enum HandlerCall<'s> {
    /// With the event; its return value `false` cancels it.
    Event,
    /// A window's `onerror` with an `ErrorEvent`: its message, file, line,
    /// column and error; its return value `true` cancels it.
    OnError(ErrorInfo, Option<v8::Local<'s, v8::Value>>),
}

/// DOM's "call a user object's operation" for the listener at `index` of
/// `event`'s snapshot: a function called with the current target as
/// `this`, or an object's `handleEvent`; what it throws reported. An event
/// handler's is its function only (an object that isn't one does nothing:
/// `[LegacyTreatNonObjectAsNull]`), with HTML's arguments and its return
/// value's cancellation (`HandlerCall`).
pub(crate) fn call_listener(
    scope: &mut Scope<'_, '_, '_>,
    event: &Persistent<dom::Event>,
    index: usize,
    entry: bool,
) {
    let (callback, this, argument, handler, in_shadow_tree) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let event = event.get(cx.gc);
            let listener = cx
                .snapshot_at(event, index)
                .expect("a listener of the snapshot");
            let callback = no_gc.wrapper(cx.callback_edge(listener));
            let target = cx
                .event_current_target(event)
                .expect("a listener is called at a target");
            // DOM's "invocation-target-in-shadow-tree", as the path was
            // made: none of a path without a shadow tree in it.
            let in_shadow_tree = cx
                .event_shadow_current(event)
                .is_some_and(|entry| entry.in_shadow);
            let handler = cx.listener_is_handler(listener).then(|| {
                let error = cx
                    .error_event_data(event)
                    .filter(|_| matches!(target, dom::TargetRef::Window(_)))
                    .filter(|_| cx.listener_type(listener) == dom::names::event::ERROR);
                match error {
                    Some(data) => HandlerCall::OnError(
                        cx.error_info(data),
                        no_gc.strong_value(cx.error_edge(data)),
                    ),
                    None => HandlerCall::Event,
                }
            });
            (
                callback,
                have(&mut no_gc, cx, target.into()),
                have(&mut no_gc, cx, ObjectRef::Event(event)),
                handler,
                in_shadow_tree,
            )
        })
    };
    let Some(callback) = callback else {
        return;
    };
    let this = wrap(scope, this);
    let argument = wrap(scope, argument);
    let current = enter_current_event(scope, callback, event, in_shadow_tree);
    // The microtask checkpoint after it (`run_callback`) before its
    // current event is the one before again: the microtasks see it (HTML's
    // "call a user object's operation" cleans up inside "inner invoke", as
    // Blink's `V8ScriptRunner::CallFunction` runs them). Not if script
    // dispatched it (`entry` false: script is on the stack, no checkpoint).
    if entry {
        run_callback(scope, |scope| match handler {
            Some(handler) => call_handler(scope, event, callback, this, argument, handler),
            None => call_callback(scope, callback, this, argument),
        });
    } else {
        // Script is on the stack: a call nested in it, never the outermost.
        let _call = trace_event::trace_event!("v8.callFunction");
        match handler {
            Some(handler) => call_handler(scope, event, callback, this, argument, handler),
            None => call_callback(scope, callback, this, argument),
        }
    }
    if let Some(current) = current {
        leave_current_event(scope, current);
    }
}

/// The window of the callback's realm and its current event before the
/// call: HTML's "inner invoke" makes `event` its current event while the
/// callback runs (not for an invocation target in a shadow tree), the one
/// before again after (`leave_current_event`). None for a callback of
/// another realm than a window's.
struct CurrentEvent<'s> {
    global: v8::Local<'s, v8::Object>,
    /// Rooted only when there is one (a dispatch in a listener's).
    previous: Option<Persistent<dom::Event>>,
}

fn enter_current_event<'s>(
    scope: &mut Scope<'_, 's, '_>,
    callback: v8::Local<'s, v8::Object>,
    event: &Persistent<dom::Event>,
    in_shadow_tree: bool,
) -> Option<CurrentEvent<'s>> {
    let global = callback
        .get_creation_context(scope.scope())?
        .global(scope.scope());
    let (state, mut no_gc) = scope.state();
    let previous = state.dom.read(|cx| {
        let window = no_gc.unwrap::<Window>(cx.gc, global)?;
        let previous = cx.window_current_event(window);
        if !in_shadow_tree {
            cx.set_window_current_event(window, Some(event.get(cx.gc)));
        }
        Some(previous.map(|previous| Persistent::new(cx.gc, previous)))
    })?;
    Some(CurrentEvent { global, previous })
}

fn leave_current_event(scope: &mut Scope<'_, '_, '_>, current: CurrentEvent<'_>) {
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        if let Some(window) = no_gc.unwrap::<Window>(cx.gc, current.global) {
            let previous = current
                .previous
                .as_ref()
                .map(|previous| previous.get(cx.gc));
            cx.set_window_current_event(window, previous);
        }
    });
}

/// A listener's callback called (`call_listener`'s): a function with
/// `this` the current target, or an object's `handleEvent` with `this` the
/// object; what it throws reported.
fn call_callback<'s>(
    scope: &mut Scope<'_, 's, '_>,
    callback: v8::Local<'s, v8::Object>,
    this: v8::Local<'s, v8::Object>,
    argument: v8::Local<'s, v8::Object>,
) {
    v8::tc_scope!(let scope, scope.scope());
    let result = match v8::Local::<v8::Function>::try_from(callback) {
        Ok(function) => function.call(scope, this.into(), &[argument.into()]),
        Err(_) => {
            let key = string(scope, "handleEvent");
            callback.get(scope, key.into()).and_then(|handler| {
                match v8::Local::<v8::Function>::try_from(handler) {
                    Ok(handler) => handler.call(scope, callback.into(), &[argument.into()]),
                    Err(_) => {
                        crate::throw_type_error(
                            scope,
                            "The listener's 'handleEvent' is not callable.",
                        );
                        None
                    }
                }
            })
        }
    };
    if result.is_none() {
        // The callback's realm's global (WebIDL's "invoke a callback
        // function" reports in it).
        report_caught(scope, ReportAt::RealmOf(callback));
    }
}

/// A traversal's filter called with `node` (DOM's "filter", its call of
/// a user object's operation: a function with `this` undefined, or an
/// object's `acceptNode` with `this` the object), in a `TryCatch`; its
/// result `ToUint16`'s. What it throws (or the `TypeError` of an
/// `acceptNode` that isn't callable, or of a filter whose realm can't run
/// script: its window's document discarded, an iframe's that went),
/// caught: the member throws it again once the traversal's flag is unset.
pub(crate) fn call_node_filter<'s>(
    scope: &mut Scope<'_, 's, '_>,
    traversal: &dom::TraversalRoot,
    node: &Persistent<dom::Node>,
) -> Result<u16, v8::Local<'s, v8::Value>> {
    let (filter, argument) = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let filter = no_gc.strong_value(cx.traversal_filter_edge(traversal.get(cx.gc)));
            (
                filter,
                have(&mut no_gc, cx, ObjectRef::Node(node.get(cx.gc))),
            )
        })
    };
    let filter = filter
        .and_then(|filter| v8::Local::<v8::Object>::try_from(filter).ok())
        .expect("a traversal made with a filter has it");
    // Blink's `IsCallbackFunctionRunnable`: its realm's document fully
    // active (an error, Blink's message; the table has `TypeError`).
    let global = filter
        .get_creation_context(scope.scope())
        .map(|context| context.global(scope.scope()));
    let runnable = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            global
                .and_then(|global| no_gc.unwrap::<dom::Window>(cx.gc, global))
                .is_some_and(|window| !cx.is_discarded(cx.window_document(window)))
        })
    };
    let argument = wrap(scope, argument);
    v8::tc_scope!(let tc, scope.scope());
    if !runnable {
        crate::throw_type_error(tc, "The provided callback is no longer runnable.");
        return Err(tc.exception().unwrap_or_else(|| v8::undefined(tc).into()));
    }
    let result = match v8::Local::<v8::Function>::try_from(filter) {
        Ok(function) => {
            let undefined = v8::undefined(tc);
            function.call(tc, undefined.into(), &[argument.into()])
        }
        Err(_) => {
            let key = string(tc, "acceptNode");
            filter.get(tc, key.into()).and_then(|accept| {
                match v8::Local::<v8::Function>::try_from(accept) {
                    Ok(accept) => accept.call(tc, filter.into(), &[argument.into()]),
                    Err(_) => {
                        crate::throw_type_error(tc, "The provided callback is not callable: 'acceptNode' is not a function.");
                        None
                    }
                }
            })
        }
    };
    // WebIDL's `unsigned short`: `ToUint32`'s low bits.
    let converted = result.and_then(|value| crate::front::to_uint32(tc, value));
    match converted {
        Some(value) => Ok(u16::try_from(value & 0xFFFF).expect("the low 16 bits")),
        None => Err(tc.exception().unwrap_or_else(|| v8::undefined(tc).into())),
    }
}

/// An event handler's call (`call_listener`'s): its function with the
/// event, or a window's `onerror` with the error's parts; what it throws
/// reported; its return value `false` (`onerror`'s `true`) cancels the
/// event (DOM's "set the canceled flag").
fn call_handler<'s>(
    scope: &mut Scope<'_, 's, '_>,
    event: &Persistent<dom::Event>,
    callback: v8::Local<'s, v8::Object>,
    this: v8::Local<'s, v8::Object>,
    argument: v8::Local<'s, v8::Object>,
    handler: HandlerCall<'s>,
) {
    let Ok(function) = v8::Local::<v8::Function>::try_from(callback) else {
        return;
    };
    let cancels = {
        let v8_scope = scope.scope();
        let (arguments, cancel_on): (Vec<v8::Local<'_, v8::Value>>, bool) = match handler {
            HandlerCall::Event => (vec![argument.into()], false),
            HandlerCall::OnError(info, error) => {
                let undefined = v8::undefined(v8_scope).into();
                (
                    vec![
                        string(v8_scope, &info.message.to_str()).into(),
                        string(v8_scope, &info.filename.to_str()).into(),
                        v8::Integer::new_from_unsigned(v8_scope, info.lineno).into(),
                        v8::Integer::new_from_unsigned(v8_scope, info.colno).into(),
                        error.unwrap_or(undefined),
                    ],
                    true,
                )
            }
        };
        v8::tc_scope!(let tc, v8_scope);
        match function.call(tc, this.into(), &arguments) {
            Some(returned) => {
                if cancel_on {
                    returned.is_true()
                } else {
                    returned.is_false()
                }
            }
            None => {
                report_caught(tc, ReportAt::RealmOf(callback));
                false
            }
        }
    };
    if cancels {
        let (state, _) = scope.state();
        state
            .dom
            .read(|cx| dom_api::cancel_event(&ApiCx::new(cx), event.get(cx.gc)));
    }
}

/// An animation frame callback called with the frame's time, `this`
/// undefined (WebIDL's "invoke a callback function"); what it throws
/// reported in its realm.
fn call_animation_frame(
    scope: &mut Scope<'_, '_, '_>,
    frame: &Persistent<dom::AnimationFrame>,
    time: f64,
) {
    let callback = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state
            .dom
            .read(|cx| no_gc.strong_value(cx.animation_frame_callback_edge(frame.get(cx.gc))))
    };
    let Some(callback) = callback else {
        return;
    };
    let scope = scope.scope();
    let Ok(function) = v8::Local::<v8::Function>::try_from(callback) else {
        return;
    };
    let time = v8::Number::new(scope, time);
    let this = v8::undefined(scope);
    v8::tc_scope!(let scope, scope);
    if function.call(scope, this.into(), &[time.into()]).is_none() {
        report_caught(scope, ReportAt::RealmOf(function.into()));
    }
}

/// An idle callback with its `IdleDeadline` (a wrapper made for it),
/// `this` undefined; what it throws reported.
fn call_idle_callback(
    scope: &mut Scope<'_, '_, '_>,
    callback: &Persistent<dom::IdleCallback>,
    deadline: &Persistent<dom::IdleDeadline>,
) {
    let (function, deadline) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            (
                no_gc.strong_value(cx.idle_callback_edge(callback.get(cx.gc))),
                have(&mut no_gc, cx, ObjectRef::IdleDeadline(deadline.get(cx.gc))),
            )
        })
    };
    call_with_object(scope, function, deadline);
}

/// A geolocation request's error callback, if it has one, with `error` (a
/// wrapper made for one with none), `this` undefined; what it throws
/// reported.
fn call_geolocation_error(
    scope: &mut Scope<'_, '_, '_>,
    request: &Persistent<dom::GeoRequest>,
    error: &Persistent<dom::GeolocationPositionError>,
) {
    let (function, error) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            (
                no_gc.strong_value(cx.geolocation_request_edges(request.get(cx.gc)).1),
                have(
                    &mut no_gc,
                    cx,
                    ObjectRef::GeolocationPositionError(error.get(cx.gc)),
                ),
            )
        })
    };
    call_with_object(scope, function, error);
}

/// `function`, if it is one, called with `object`'s wrapper, `this`
/// undefined; what it throws reported at its realm.
fn call_with_object<'s>(
    scope: &mut Scope<'_, 's, '_>,
    function: Option<v8::Local<'s, v8::Value>>,
    object: Have<'s>,
) {
    let Some(function) = function.and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    else {
        return;
    };
    // A new wrapper in the callback's realm (Blink's callback's relevant
    // `ScriptState`): another frame's function given an `IdleDeadline`
    // gets one of its own frame's.
    let current = scope.scope().get_current_context();
    let object = match function.get_creation_context(scope.scope()) {
        Some(context) if context != current => {
            let scope = &mut v8::ContextScope::new(scope.scope(), context);
            let mut scope = crate::state::scope(scope).expect("the isolate's state");
            wrap(&mut scope, object)
        }
        _ => wrap(scope, object),
    };
    let scope = scope.scope();
    let this = v8::undefined(scope);
    v8::tc_scope!(let scope, scope);
    if function
        .call(scope, this.into(), &[object.into()])
        .is_none()
    {
        report_caught(scope, ReportAt::RealmOf(function.into()));
    }
}

/// The DOM's pending promise numbered `promise` resolved with `object`'s
/// wrapper (one made for it if it has none); nothing if its resolver is
/// gone.
fn resolve_promise_with(scope: &mut Scope<'_, '_, '_>, promise: u64, object: &ObjectRoot) {
    let Some(resolver) = scope.state_mut().0.when_defined.remove(&promise) else {
        return;
    };
    let object = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| have(&mut no_gc, cx, object.get(cx.gc)))
    };
    let value = wrap(scope, object);
    let scope = scope.scope();
    let resolver = v8::Local::new(scope, resolver);
    resolver.resolve(scope, value.into());
}

/// `signal`'s abort reason a new `DOMException` of `name` and `message`,
/// made in the current realm (the signal's window's, whose task this is).
fn set_abort_reason(
    scope: &mut Scope<'_, '_, '_>,
    signal: &Persistent<dom::EventTargetImpl>,
    name: dom_api::ExceptionName,
    message: &str,
) {
    let Some(exception) = crate::exception::new_dom_exception(scope, name.name(), message) else {
        return;
    };
    let (state, mut no_gc) = scope.state();
    // Allocates nothing: an edge set.
    state.dom.read(|cx| {
        no_gc.set_strong_value(cx.abort_reason_edge(signal.get(cx.gc)), exception.into())
    });
}

/// The DOM's pending promise numbered `promise` rejected with `signal`'s
/// abort reason; nothing if its resolver is gone.
pub(crate) fn reject_promise_with_reason(
    scope: &mut Scope<'_, '_, '_>,
    promise: u64,
    signal: &Persistent<dom::EventTargetImpl>,
) {
    let Some(resolver) = scope.state_mut().0.when_defined.remove(&promise) else {
        return;
    };
    let reason = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            cx.abort_reason(signal.get(cx.gc))
                .and_then(|edge| no_gc.strong_value(edge))
        })
    };
    let scope = scope.scope();
    let resolver = v8::Local::new(scope, resolver);
    let reason = reason.unwrap_or_else(|| v8::undefined(scope).into());
    resolver.reject(scope, reason);
}

/// What the host's input monitor says waits for the main thread; none
/// without one.
pub(crate) fn pending_input(
    scope: &mut Scope<'_, '_, '_>,
    include_continuous: bool,
) -> Vec<dom_api::PendingInput> {
    let (state, _) = scope.state();
    state
        .input_monitor
        .as_ref()
        .map_or_else(Vec::new, |monitor| {
            monitor.pending_input(include_continuous)
        })
}

/// A `toBlob` call's callback, with its blob (a wrapper made for one with
/// none) or null, `this` undefined.
fn call_blob_callback(
    scope: &mut Scope<'_, '_, '_>,
    callback: &Persistent<dom::CanvasBlobCallback>,
) {
    let (function, blob) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let (edge, blob) = cx.canvas_blob_parts(callback.get(cx.gc));
            (
                no_gc.strong_value(edge),
                blob.map(|blob| have(&mut no_gc, cx, ObjectRef::Blob(blob))),
            )
        })
    };
    let Some(function) = function else {
        return;
    };
    let blob: v8::Local<'_, v8::Value> = match blob {
        Some(blob) => wrap(scope, blob).into(),
        None => v8::null(scope.scope()).into(),
    };
    let scope = scope.scope();
    let Ok(function) = v8::Local::<v8::Function>::try_from(function) else {
        return;
    };
    let this = v8::undefined(scope);
    v8::tc_scope!(let scope, scope);
    if function.call(scope, this.into(), &[blob]).is_none() {
        report_caught(scope, ReportAt::RealmOf(function.into()));
    }
}

/// HTML's timer task's callback: the timer's function, `this` its window
/// (the global proxy), its arguments the timer's.
pub(crate) fn call_timer(
    scope: &mut Scope<'_, '_, '_>,
    window: &Persistent<Window>,
    timer: &Persistent<Timer>,
) {
    let (callback, arguments, this, url, base_url) = {
        let (state, mut no_gc) = scope.state();
        // Allocates nothing: a read.
        state.dom.read(|cx| {
            let timer = timer.get(cx.gc);
            let window = window.get(cx.gc);
            let document = cx.window_document(window);
            (
                no_gc.strong_value(cx.timer_callback_edge(timer)),
                no_gc.strong_value(cx.timer_arguments_edge(timer)),
                no_gc.wrapper(cx.object_wrapper_edge(window.into())),
                cx.document_url(document),
                cx.document_base_url(document),
            )
        })
    };
    let (Some(callback), Some(this)) = (callback, this) else {
        return;
    };
    let scope = scope.scope();
    match v8::Local::<v8::Function>::try_from(callback) {
        Ok(function) => {
            let arguments: Vec<v8::Local<'_, v8::Value>> = match arguments {
                Some(arguments) => {
                    let arguments = v8::Local::<v8::Array>::try_from(arguments)
                        .expect("a timer's arguments' array");
                    (0..arguments.length())
                        .filter_map(|index| arguments.get_index(scope, index))
                        .collect()
                }
                None => Vec::new(),
            };
            v8::tc_scope!(let scope, scope);
            // What it throws is reported in its own realm (WebIDL's "invoke
            // a callback function", Blink's `InvokeAndReportException`):
            // another frame's function's at that frame's window.
            if function.call(scope, this.into(), &arguments).is_none() {
                report_caught(scope, ReportAt::RealmOf(function.into()));
            }
        }
        // A string: HTML's timer task runs it as a classic script without
        // a URL (Blink's `ScheduledAction`: its stack says `<anonymous>`),
        // what it throws reported as its document's (its arguments
        // ignored).
        Err(_) => {
            let url = url.map_or_else(|| "about:blank".to_owned(), |url| url.to_string());
            v8::tc_scope!(let scope, scope);
            let Ok(source) = v8::Local::<v8::String>::try_from(callback) else {
                return;
            };
            let name = v8::String::empty(scope);
            // What an `import()` in it resolves against: its document's base
            // URL (HTML's settings object's API base URL).
            let options = base_url
                .as_ref()
                .map(|base| bindings::script_options(scope, base.as_str(), Some("same-origin")));
            let origin = v8::ScriptOrigin::new(
                scope,
                name.into(),
                0,
                0,
                true,
                0,
                None,
                false,
                false,
                false,
                options.flatten(),
            );
            let ran = v8::Script::compile(scope, source, Some(&origin))
                .and_then(|script| script.run(scope));
            if ran.is_none() {
                report_caught_of(scope, ReportAt::Global(this), Some(&url));
            }
        }
    }
}

/// Where what a callback threw is reported: at the global of the
/// callback's realm, or at a given one (a timer's window).
pub(crate) enum ReportAt<'s> {
    RealmOf(v8::Local<'s, v8::Object>),
    Global(v8::Local<'s, v8::Object>),
}

/// What `tc` caught, if anything, reported (`report`), the `TryCatch`
/// reset first: out of line, so that a callback's call pays a branch for
/// it, not the registers its work takes.
#[cold]
#[inline(never)]
pub(crate) fn report_caught(
    tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>,
    at: ReportAt<'_>,
) {
    report_caught_of(tc, at, None);
}

/// `report_caught`, its filename `url` if V8's has none (a timer's string,
/// a script of no URL of its document's).
#[cold]
#[inline(never)]
fn report_caught_of(
    tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>,
    at: ReportAt<'_>,
    url: Option<&str>,
) {
    let Some(mut thrown) = Thrown::caught(tc) else {
        return;
    };
    if let Some(url) = url
        && thrown.info.filename.is_empty()
    {
        thrown.info.filename = url.into();
    }
    tc.reset();
    let global = match at {
        ReportAt::RealmOf(callback) => callback
            .get_creation_context(tc)
            .map(|context| context.global(tc)),
        ReportAt::Global(global) => Some(global),
    };
    let Some(mut scope) = state::scope(tc) else {
        return;
    };
    report(&mut scope, global, thrown);
}
/// `value`, a module's evaluation's rejection, reported at the window
/// `global` is (HTML's "report the exception"), from V8's message made of
/// it (where an error was made; muted if its script's errors are).
pub(crate) fn report_value(
    scope: &mut v8::PinScope<'_, '_>,
    global: v8::Local<'_, v8::Object>,
    value: v8::Local<'_, v8::Value>,
) {
    let message = v8::Exception::create_message(scope, value);
    let thrown = Thrown::of(scope, value, Some(message));
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    report(&mut scope, Some(global), thrown);
}

/// What a callback threw and where (V8's message of it), held out of the
/// `TryCatch` that caught it.
struct Thrown {
    exception: v8::Global<v8::Value>,
    info: ErrorInfo,
    /// Where it was thrown, for DevTools.
    place: crate::devtools::Place,
}

impl Thrown {
    /// What `tc` caught, if anything (none once execution is terminating).
    fn caught(tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>) -> Option<Self> {
        let exception = tc.exception()?;
        let message = tc.message();
        Some(Self::of(tc, exception, message))
    }

    /// `exception`, thrown, with V8's `message` of it.
    fn of(
        tc: &mut v8::PinScope<'_, '_>,
        exception: v8::Local<'_, v8::Value>,
        message: Option<v8::Local<'_, v8::Message>>,
    ) -> Self {
        // HTML's muted errors (Blink's `SanitizeScriptErrors`): another
        // origin's script fetched `no-cors` isn't shared cross-origin; what
        // it throws is reported as `Script error.`, from nowhere, its value
        // null.
        if message.is_some_and(|message| !message.is_shared_cross_origin()) {
            let null: v8::Local<'_, v8::Value> = v8::null(tc).into();
            return Self {
                exception: v8::Global::new(tc, null),
                info: ErrorInfo {
                    message: "Script error.".into(),
                    filename: "".into(),
                    lineno: 0,
                    colno: 0,
                },
                place: crate::devtools::Place::unknown(),
            };
        }
        // The exception as a string after `Uncaught ` (`Uncaught Error:
        // boom`, V8's message's and so Blink's), the script's URL, and where
        // in it (none for an error made without a position).
        let place = crate::devtools::Place::of_message(tc, message);
        let info = ErrorInfo {
            message: format!("Uncaught {}", exception.to_rust_string_lossy(tc))
                .as_str()
                .into(),
            filename: place.url.as_str().into(),
            lineno: place.line,
            colno: place.column,
        };
        Self {
            exception: v8::Global::new(tc, exception),
            info,
            place,
        }
    }
}

/// HTML's "report an exception" at the window `global` is, if it is one of
/// the DOM's: its `error` event; logged unless a listener canceled it
/// (always without a window).
fn report(
    scope: &mut Scope<'_, '_, '_>,
    global: Option<v8::Local<'_, v8::Object>>,
    thrown: Thrown,
) {
    let window = global.and_then(|global| {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            no_gc
                .unwrap::<Window>(cx.gc, global)
                .map(|window| Persistent::new(cx.gc, window))
        })
    });
    let exception = v8::Local::new(scope.scope(), &thrown.exception);
    let reported = match window {
        Some(window) => {
            dom_api::report_exception(&mut Host { scope }, &window, &thrown.info, Some(exception))
        }
        None => Reported::NotCanceled,
    };
    if reported != Reported::Canceled {
        let ErrorInfo {
            message,
            filename,
            lineno,
            colno: _,
        } = &thrown.info;
        // Printed with the page's `console` (its text without the
        // `Uncaught ` the event's message has).
        bindings::devtools::report_uncaught(
            scope.scope(),
            bindings::devtools::ConsoleMessage {
                text: {
                    let message = message.to_string();
                    match message.strip_prefix("Uncaught ") {
                        Some(rest) => rest.to_owned(),
                        None => message,
                    }
                },
                url: filename.to_string(),
                line: *lineno,
                uncaught: true,
            },
        );
        let state = scope.state_mut().0;
        state.uncaught_exceptions = state.uncaught_exceptions.wrapping_add(1);
        // And to DevTools' console, in the window's realm, as Blink's
        // `ExecutionContext` reports an error event nothing canceled.
        let scope = scope.scope();
        let context = global
            .and_then(|global| global.get_creation_context(scope))
            .unwrap_or_else(|| scope.get_current_context());
        let detailed = thrown.info.message.to_string();
        crate::devtools::exception_thrown(
            scope,
            context,
            "Uncaught",
            &detailed,
            exception,
            thrown.place,
        );
    }
}
