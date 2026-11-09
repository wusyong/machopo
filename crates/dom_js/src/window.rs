//! A realm whose global is a window of the DOM (Blink's
//! `LocalWindowProxy::CreateContext`): its context made from `Window`'s
//! template, its global proxy wrapping a new `Window` of a new document,
//! the table's interface objects on it; and its timers run by the host's
//! clock.

use dom::Window;
use dom_api::ApiCx;
use elise::Persistent;

use crate::front::wrap_new_object;
use crate::listeners::Host;
use crate::state::{self, Scope};
use crate::templates;

/// A new realm of the isolate whose global is a new window, of a new HTML
/// document at `url`; its global proxy `proxy` if given (a window proxy
/// going on from a realm whose global it was, detached: HTML's
/// `WindowProxy`, Blink's `LocalWindowProxy`). `scope` is in any context of
/// the isolate (the interfaces' templates are the isolate's, made in it at
/// the first); the new context is entered only here, the caller enters it
/// to run script.
pub fn create_window_realm<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    url: &str,
    proxy: Option<v8::Local<'s, v8::Object>>,
) -> v8::Local<'s, v8::Context> {
    state::install(scope);
    let index = dom_api::INTERFACES
        .iter()
        .position(|interface| interface.name == "Window")
        .expect("the table's Window");
    let template = {
        let mut scope = state::scope(scope).expect("installed");
        templates::template(&mut scope, index)
    };
    let global = template.instance_template(scope);
    let context = v8::Context::new(
        scope,
        v8::ContextOptions {
            global_template: Some(global),
            global_object: proxy.map(Into::into),
            ..Default::default()
        },
    );
    let scope = &mut v8::ContextScope::new(scope, context);
    let proxy = context.global(scope);
    {
        let mut scope = state::scope(scope).expect("installed");
        let (state, mut no_gc) = scope.state_mut();
        state.dom.mutate(|cx| {
            let document = cx.create_document(true);
            cx.set_document_url(document, url);
            let window = cx.create_window(document);
            wrap_new_object(&mut no_gc, cx, proxy, window.into());
        });
    }
    templates::install(scope, proxy);
    update_cached_accessors(&mut state::scope(scope).expect("installed"), proxy);
    crate::window_properties::install_in_realm(&mut state::scope(scope).expect("installed"), proxy);
    install_queue_microtask(scope, proxy);
    state::report(&mut state::scope(scope).expect("installed"));
    context
}

/// The accessors whose values V8 reads from a private property of the
/// global once it has one (Blink's `[CachedAccessor]`,
/// `V8PrivateProperty::CachedAccessor`): V8's inline caches can't keep an
/// API getter on the global object (its properties are a dictionary), so
/// without the property each read of a global `document` calls into the
/// getter through V8's runtime; with it, V8 loads it as data and its
/// optimizing compilers fold it in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CachedAccessor {
    /// `window`: the window proxy (Blink's `kWindowProxy`).
    WindowProxy,
    /// `document`: the window's document (Blink's `kWindowDocument`).
    WindowDocument,
}

impl CachedAccessor {
    /// The cached accessor that the getter of `interface`'s `member` is.
    pub(crate) fn of(interface: &str, member: &str) -> Option<Self> {
        match (interface, member) {
            ("Window", "window") => Some(Self::WindowProxy),
            ("Window", "document") => Some(Self::WindowDocument),
            _ => None,
        }
    }

    /// Its private property's key, the isolate's.
    pub(crate) fn key<'s>(self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Private> {
        let name = templates::string(
            scope,
            match self {
                Self::WindowProxy => "machopo:cached-window-proxy",
                Self::WindowDocument => "machopo:cached-window-document",
            },
        );
        v8::Private::for_api(scope, Some(name))
    }
}

/// The values of `proxy`'s realm's cached accessors set as they are now:
/// the window proxy, and the window's document's wrapper (made if it has
/// none). As the realm is made, and again when its window gets another
/// document (Blink's `LocalWindowProxy::UpdateDocumentProperty`): V8 reads
/// the property instead of calling the getter, so a stale one would be the
/// old document.
pub(crate) fn update_cached_accessors(
    scope: &mut Scope<'_, '_, '_>,
    proxy: v8::Local<'_, v8::Object>,
) {
    let (document, interface) = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let window = no_gc
                .unwrap::<Window>(cx.gc, proxy)
                .expect("a realm's window");
            let document = cx.window_document(window);
            (
                cx.root_object(document.into()),
                crate::front::interface_of(cx, document),
            )
        })
    };
    let wrapper = crate::front::wrapper_of(scope, &document, interface);
    let scope = scope.scope();
    for (cached, value) in [
        (CachedAccessor::WindowProxy, proxy),
        (CachedAccessor::WindowDocument, wrapper),
    ] {
        let key = cached.key(scope);
        proxy
            .set_private(scope, key, value.into())
            .expect("a private property of a global, set");
    }
}

/// The window that is `context`'s global, if it is one of the DOM's.
fn window_of(
    scope: &mut Scope<'_, '_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> Option<Persistent<Window>> {
    let global = context.global(scope.scope());
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        no_gc
            .unwrap::<Window>(cx.gc, global)
            .map(|window| Persistent::new(cx.gc, window))
    })
}

/// Runs the timers of the window that is `context`'s global due by `now`
/// (milliseconds since its time origin; its clock's now if none), each
/// followed by a microtask checkpoint (each is HTML's task); `scope` is in
/// `context`, with no script running. How many ran: none for a context
/// whose global isn't a window of the DOM.
pub fn run_timers(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    now: Option<f64>,
) -> usize {
    let Some(mut scope) = state::scope(scope) else {
        return 0;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return 0;
    };
    let now = now.unwrap_or_else(|| {
        let (state, _) = scope.state();
        state.dom.read(|cx| cx.now_since(Some(window.get(cx.gc))))
    });
    let ran = dom_api::run_due(&mut Host { scope: &mut scope }, &window, now);
    state::report(&mut scope);
    ran
}

/// Runs the first timer due by `now` (milliseconds since its time origin)
/// of the window that is `context`'s global, followed by a microtask
/// checkpoint (`dom_api::run_next_due`); `scope` is in `context`,
/// with no script running. Whether one ran.
pub(crate) fn run_next_timer(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    now: f64,
) -> bool {
    let Some(mut scope) = state::scope(scope) else {
        return false;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return false;
    };
    let ran = dom_api::run_next_due(&mut Host { scope: &mut scope }, &window, now);
    state::report(&mut scope);
    ran
}

/// Runs the task `queue` names of the window that is `context`'s global
/// (`dom_api::run_task`: the one `Cx::next_task` picked), timers by
/// `now` (milliseconds since its time origin), with its microtask
/// checkpoint; `scope` is in `context`, with no script running. Whether
/// one ran.
pub(crate) fn run_task(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    queue: dom::TaskQueue,
    now: f64,
) -> bool {
    let Some(mut scope) = state::scope(scope) else {
        return false;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return false;
    };
    let ran = dom_api::run_task(&mut Host { scope: &mut scope }, &window, queue, now);
    state::report(&mut scope);
    ran
}

/// Runs the element tasks of the window that is `context`'s global
/// (`dom_api::run_element_tasks`), each followed by a microtask
/// checkpoint; `scope` is in `context`, with no script running. How many
/// ran.
pub fn run_element_tasks(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> usize {
    let Some(mut scope) = state::scope(scope) else {
        return 0;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return 0;
    };
    let ran = dom_api::run_element_tasks(&mut Host { scope: &mut scope }, &window);
    state::report(&mut scope);
    ran
}

/// Runs the messages of the window that is `context`'s global
/// (`dom_api::run_posted_messages`), each followed by a microtask
/// checkpoint; `scope` is in `context`, with no script running. How many
/// ran.
pub fn run_posted_messages(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> usize {
    let Some(mut scope) = state::scope(scope) else {
        return 0;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return 0;
    };
    let ran = dom_api::run_posted_messages(&mut Host { scope: &mut scope }, &window);
    state::report(&mut scope);
    ran
}

/// Intersection Observer's "notify intersection observers" of the window
/// that is `context`'s global.
pub fn notify_intersection_observers(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
) {
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return;
    };
    dom_api::notify_intersection_observers(&mut Host { scope: &mut scope }, &window);
    state::report(&mut scope);
}

/// Whether the resize observers of the window that is `context`'s global
/// observe anything (`dom_api::has_resize_observations`).
pub fn has_resize_observations(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> bool {
    let Some(mut scope) = state::scope(scope) else {
        return false;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return false;
    };
    let (state, _) = scope.state();
    state
        .dom
        .read(|cx| dom_api::has_resize_observations(&ApiCx::new(cx), window.get(cx.gc)))
}

/// A round of the rendering update's resize observer loop of the window
/// that is `context`'s global (`dom_api::gather_and_broadcast`).
pub fn gather_and_broadcast_resizes(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    depth: usize,
) -> dom_api::ResizeObserverRound {
    let Some(mut scope) = state::scope(scope) else {
        return dom_api::ResizeObserverRound::default();
    };
    let Some(window) = window_of(&mut scope, context) else {
        return dom_api::ResizeObserverRound::default();
    };
    let round = dom_api::gather_and_broadcast(&mut Host { scope: &mut scope }, &window, depth);
    state::report(&mut scope);
    round
}

/// The resize observer loop's error at the window that is `context`'s
/// global (`dom_api::report_loop_error`), logged unless canceled.
pub fn report_resize_loop_error(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
) {
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return;
    };
    let reported = dom_api::report_loop_error(&mut Host { scope: &mut scope }, &window);
    if reported == dom_api::Reported::NotCanceled {
        bindings::devtools::report_uncaught(
            scope.scope(),
            bindings::devtools::ConsoleMessage {
                text: "ResizeObserver loop completed with undelivered notifications.".to_owned(),
                url: String::new(),
                line: 0,
                uncaught: true,
            },
        );
    }
    state::report(&mut scope);
}

/// HTML's "run the animation frame callbacks" of the window that is
/// `context`'s global, at `frame_time`.
pub fn run_animation_frames(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    frame_time: std::time::Instant,
) {
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return;
    };
    let now = {
        let (state, _) = scope.state();
        state
            .dom
            .read(|cx| cx.time_at(window.get(cx.gc), frame_time))
    };
    dom_api::run_animation_frame_callbacks(&mut Host { scope: &mut scope }, &window, now);
    state::report(&mut scope);
}

/// The response to the window's fetch numbered `id`, at the window that is
/// `context`'s global (`dom_api::did_receive_response`).
pub fn fetch_response(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    id: u64,
    response: Result<dom_api::FetchedResponse, ()>,
) {
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return;
    };
    dom_api::did_receive_response(&mut Host { scope: &mut scope }, &window, id, response);
    state::report(&mut scope);
}

/// One idle callback of the window that is `context`'s global
/// (`dom_api::run_idle_callback`), of the idle period ending at
/// `deadline`; `scope` is in `context`, with no script running. Whether
/// one ran.
pub fn run_idle_callback(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    deadline: std::time::Instant,
) -> bool {
    let Some(mut scope) = state::scope(scope) else {
        return false;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return false;
    };
    let deadline = {
        let (state, _) = scope.state();
        state.dom.read(|cx| cx.time_at(window.get(cx.gc), deadline))
    };
    let ran = dom_api::run_idle_callback(&mut Host { scope: &mut scope }, &window, deadline);
    state::report(&mut scope);
    ran
}

/// The page took or lost the system's focus, at the window that is
/// `context`'s global (`dom_api::set_page_focused`).
pub fn set_page_focused(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    focused: bool,
) {
    let Some(mut scope) = state::scope(scope) else {
        return;
    };
    let Some(window) = window_of(&mut scope, context) else {
        return;
    };
    dom_api::set_page_focused(&mut Host { scope: &mut scope }, &window, focused);
    state::report(&mut scope);
}

/// When the next timer of the window that is `context`'s global is due
/// (milliseconds since its time origin), for the host's clock; none with
/// no timer, or for a context whose global isn't a window of the DOM.
pub fn next_timer(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> Option<f64> {
    let mut scope = state::scope(scope)?;
    let window = window_of(&mut scope, context)?;
    dom_api::next_due(&scope.state().0.dom, &window)
}

/// How long until `window`'s next timer is due, by its clock (now with an
/// element task queued); none with no timer.
pub(crate) fn until_next_timer<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    window: elise::Gc<'gc, Window>,
) -> Option<std::time::Duration> {
    // An element task or a message queued is due now.
    let wait = if cx.has_element_task(window) || cx.has_posted_message(window) {
        0.0
    } else {
        cx.next_due(window)? - cx.now_since(Some(window))
    };
    Some(std::time::Duration::from_secs_f64(wait.max(0.0) / 1000.0))
}

/// `queueMicrotask` on `global` (HTML's `WindowOrWorkerGlobalScope`): the
/// callback in the engine's own microtask queue, wrapped so that what it
/// throws is reported at its realm's window (HTML's "report the
/// exception"), as a listener's. Not the declaration table's: it touches
/// no DOM state, only the engine's queue. A writable, configurable,
/// enumerable data property, as a `[Global]` interface's operations are.
fn install_queue_microtask(scope: &mut v8::PinScope<'_, '_>, global: v8::Local<'_, v8::Object>) {
    let template = v8::FunctionTemplate::builder(queue_microtask)
        .length(1)
        .constructor_behavior(v8::ConstructorBehavior::Throw)
        .build(scope);
    // Its `this` a window the caller may reach, as a member's.
    template.set_accept_any_receiver(false);
    let function = template.get_function(scope).expect("a function");
    let name = crate::templates::string(scope, "queueMicrotask");
    function.set_name(name);
    global.set(scope, name.into(), function.into());
}

fn queue_microtask(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments<'_>,
    _: v8::ReturnValue<'_>,
) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.get(0)) else {
        crate::throw_type_error(
            scope,
            "Failed to execute 'queueMicrotask' on 'Window': parameter 1 is not of type 'Function'.",
        );
        return;
    };
    let Some(job) = v8::Function::builder(run_microtask)
        .data(callback.into())
        .build(scope)
    else {
        return;
    };
    scope.enqueue_microtask(job);
}

/// A `queueMicrotask` callback run, `this` undefined; what it throws
/// reported, not rethrown into the checkpoint.
fn run_microtask(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments<'_>,
    _: v8::ReturnValue<'_>,
) {
    let Ok(callback) = v8::Local::<v8::Function>::try_from(args.data()) else {
        return;
    };
    let undefined = v8::undefined(scope).into();
    v8::tc_scope!(let scope, scope);
    if callback.call(scope, undefined, &[]).is_none() {
        crate::listeners::report_caught(
            scope,
            crate::listeners::ReportAt::RealmOf(callback.into()),
        );
    }
}
