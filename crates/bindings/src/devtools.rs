//! V8's inspector for the isolate (Blink's `MainThreadDebugger` and
//! `ThreadDebuggerCommonImpl`): what answers DevTools' `Runtime`,
//! `Debugger`, `Profiler`, `HeapProfiler` and `Console` for a renderer's
//! pages, and what V8's own `console` hands its calls to.
//!
//! One per isolate, made with it and gone with it: V8's builtin `console`
//! does nothing without the inspector as its delegate. The renderer's tab
//! is its one context group. It lives in an isolate slot, so script entries
//! and bindings alike reach it ([`context_created`], [`exception_thrown`]);
//! [`V8PerIsolateData`](crate::V8PerIsolateData) holds the sessions.
//!
//! A session's messages go in as V8 runs page script (`Runtime.evaluate`),
//! so they are handed to it through a script entry; its answers and
//! events come out through `send`, as JSON.

use std::cell::{Cell, OnceCell, RefCell};
use std::collections::HashMap;
use std::ffi::c_void;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use v8::UniquePtr;
use v8::inspector::{
    Channel, ChannelImpl, Inspectable, InspectableImpl, StringBuffer, StringView, V8Inspector,
    V8InspectorClient, V8InspectorClientImpl, V8InspectorClientTrustLevel, V8InspectorSession,
    V8StackTrace,
};

/// The renderer's tab: the inspector's one context group.
const CONTEXT_GROUP: i32 = 1;

/// The most promises reported as unhandled whose later handler is told to
/// DevTools (Blink's `kMaxReportedHandlersPendingResolution`).
const MAX_REPORTED_REJECTIONS: usize = 1000;

/// A `console` call, as V8's inspector formats it: its arguments as text,
/// joined by spaces, and where the call was; or what the embedder reports
/// as uncaught (`report_uncaught`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsoleMessage {
    pub text: String,
    /// Empty for code with no URL (`eval`'s).
    pub url: String,
    /// 1-based; 0 if unknown.
    pub line: u32,
    /// An exception, or a rejection, nothing handled: not a `console` call.
    pub uncaught: bool,
}

/// Where the isolate's `console` messages go (the renderer prints them).
pub type ConsoleSink = Box<dyn Fn(ConsoleMessage)>;

/// What DevTools is to call a value of the embedder's: a `Runtime.RemoteObject`
/// subtype (`node`) and its description (`div#app.main`), as Blink's
/// `ThreadDebuggerCommonImpl::valueSubtype` and `descriptionForValueSubtype`
/// tell them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subtype {
    pub subtype: &'static str,
    pub description: String,
}

/// The embedder's `Subtype` of a value, if it has one: called as V8's
/// inspector describes a value, with no state of the embedder's borrowed
/// (the inspector describes values in a command, or as script pauses).
pub type Subtypes =
    Box<dyn for<'s> Fn(&mut v8::PinScope<'s, '_>, v8::Local<'s, v8::Value>) -> Option<Subtype>>;

/// What the browser asked of the page's sessions, in order, as the
/// renderer's DevTools queue holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Queued {
    Attach(String),
    Detach(String),
    Message {
        session: String,
        message: String,
    },
    /// The renderer is to end.
    Quit,
}

/// The renderer's DevTools queue, as the inspector takes from it while
/// script is on the stack (Blink's `InspectorTaskRunner`): while it is
/// paused, and when a message is to interrupt it.
pub trait Queue {
    /// The next, waiting for one.
    fn next(&self) -> Queued;
    /// The messages that are to interrupt script (`Debugger.pause`...),
    /// taken out in order: those before any attach or detach, whose
    /// sessions are known.
    fn take_interrupting(&self) -> Vec<(String, String)>;
    /// Puts `queued` back first, in order: what waits for the pause to
    /// end.
    fn put_back(&self, queued: Vec<Queued>);
    /// Answers `message`, a command of `session`'s, if the renderer does
    /// itself: whether it did. Script is paused at a statement, so no one
    /// borrows the isolate's state: it may (`isolate_state_mut`).
    fn answer_here(&self, isolate: &mut v8::Isolate, session: &str, message: &str) -> bool;
    /// Script paused, before the loop takes the first message: the
    /// renderer's agents tell what script did up to there (the DOM's
    /// changes), with the isolate's state as `answer_here` borrows it.
    fn paused(&self, isolate: &mut v8::Isolate);
}

/// A session of the inspector's, and whether the browser still has it.
struct Attached {
    session: Rc<V8InspectorSession>,
    /// Off once the browser detached it: what it sends then is dropped.
    alive: Rc<Cell<bool>>,
}

/// What the inspector's client and the isolate's slot share.
#[derive(Default)]
struct Shared {
    /// By the browser's session ids.
    sessions: RefCell<HashMap<String, Attached>>,
    /// Set once, as the renderer starts: borrowed shared, so the pause's
    /// loop holds it as script it runs pauses again (a nested loop).
    queue: OnceCell<Box<dyn Queue>>,
    /// The inspector asked the pause's loop to end.
    quit: Cell<bool>,
}

impl Shared {
    fn session(&self, session: &str) -> Option<Rc<V8InspectorSession>> {
        self.sessions
            .borrow()
            .get(session)
            .map(|attached| attached.session.clone())
    }

    /// While script is paused (Blink's `ClientMessageLoopAdapter::Run`):
    /// the DevTools queue's messages alone, in order, until the inspector
    /// says to go on. A session attached meanwhile, and its messages, wait
    /// for after; one detached is sent no more and stops debugging, which
    /// lets script go on if no other session debugs, and goes once the
    /// loop has returned, as V8 asks (the detach is put back too, for the
    /// renderer's own agents). The renderer ending lets script go on, to
    /// its termination. `isolate` is the paused one, for the renderer's
    /// own agents to borrow its state through.
    fn run_paused(&self, isolate: &mut v8::Isolate) {
        let Some(queue) = self.queue.get() else {
            return;
        };
        self.quit.set(false);
        queue.paused(isolate);
        let mut after = Vec::new();
        let mut gone = Vec::new();
        while !self.quit.get() {
            match queue.next() {
                Queued::Message { session, message } => {
                    let waiting = after
                        .iter()
                        .any(|queued| matches!(queued, Queued::Attach(s) if *s == session));
                    if waiting {
                        after.push(Queued::Message { session, message });
                    } else if !queue.answer_here(isolate, &session, &message) {
                        match self.session(&session) {
                            Some(attached) => dispatch(&attached, &message),
                            None => after.push(Queued::Message { session, message }),
                        }
                    }
                }
                Queued::Attach(session) => after.push(Queued::Attach(session)),
                Queued::Detach(session) => {
                    let removed = self.sessions.borrow_mut().remove(&session);
                    if let Some(attached) = removed {
                        attached.alive.set(false);
                        dispatch(&attached.session, STOP_DEBUGGING);
                        gone.push(attached);
                    }
                    after.push(Queued::Detach(session));
                }
                Queued::Quit => {
                    let sessions: Vec<_> = self
                        .sessions
                        .borrow()
                        .values()
                        .map(|attached| {
                            attached.alive.set(false);
                            attached.session.clone()
                        })
                        .collect();
                    for session in sessions {
                        dispatch(&session, TERMINATE);
                    }
                    after.push(Queued::Quit);
                    break;
                }
            }
        }
        queue.put_back(after);
        drop(gone);
    }

    /// While script runs: the messages that are to interrupt it.
    fn dispatch_interrupting(&self) {
        let Some(queue) = self.queue.get() else {
            return;
        };
        for (session, message) in queue.take_interrupting() {
            match self.session(&session) {
                Some(attached) => dispatch(&attached, &message),
                None => tracing::warn!(session, "an interrupting DevTools message for no session"),
            }
        }
    }
}

/// A detached session stops debugging, and script paused for it goes on
/// unless another debugs (its answer is dropped).
const STOP_DEBUGGING: &str = r#"{"id":0,"method":"Debugger.disable"}"#;

/// The renderer ends: script paused goes on, to be terminated (its answer
/// is dropped).
const TERMINATE: &str =
    r#"{"id":0,"method":"Debugger.resume","params":{"terminateOnResume":true}}"#;

fn dispatch(session: &V8InspectorSession, message: &str) {
    session.dispatch_protocol_message(StringView::from(message.as_bytes()));
}

/// What answers V8's inspector's questions.
struct Client {
    shared: Rc<Shared>,
    console: Rc<RefCell<Option<ConsoleSink>>>,
    subtypes: Rc<RefCell<Option<Subtypes>>>,
    /// The main frame's realm, which a command that names no context runs
    /// in (Blink's `MainThreadDebugger::ensureDefaultContextInGroup`).
    default_context: Rc<RefCell<Option<v8::Weak<v8::Context>>>>,
    /// The isolate, for the default context's handle.
    isolate: v8::UnsafeRawIsolatePtr,
}

impl V8InspectorClientImpl for Client {
    fn run_message_loop_on_pause(&self, _context_group_id: i32) {
        // SAFETY: V8 calls this on the isolate's thread as script pauses at
        // a statement: the isolate is alive (the inspector goes before it),
        // and the Rust around the script has handed its scope to V8, so it
        // holds no borrow of the isolate's state while the loop lends it.
        let mut isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(self.isolate) };
        self.shared.run_paused(&mut isolate);
    }

    fn quit_message_loop_on_pause(&self) {
        self.shared.quit.set(true);
    }

    fn current_time_ms(&self) -> f64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0.0, |since| since.as_secs_f64() * 1000.0)
    }

    fn ensure_default_context_in_group(
        &self,
        _context_group_id: i32,
    ) -> Option<v8::Local<'_, v8::Context>> {
        let context = self.default_context.borrow();
        let context = context.as_ref()?;
        // SAFETY: V8's inspector calls this on the isolate's thread while it
        // looks for a command's context, in a handle scope of its own; the
        // isolate is alive (the inspector goes before it).
        let mut isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(self.isolate) };
        v8::callback_scope!(unsafe scope, &mut isolate);
        let context = context.to_local(scope)?;
        // SAFETY: the handle is in V8's handle scope, which outlives this
        // call (V8 reads it as it returns).
        Some(unsafe { context.extend_lifetime_unchecked() })
    }

    fn value_subtype<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        value: v8::Local<'s, v8::Value>,
    ) -> Option<v8::UniquePtr<StringBuffer>> {
        let subtype = (self.subtypes.borrow().as_ref()?)(scope, value)?;
        Some(StringBuffer::create(StringView::from(
            subtype.subtype.as_bytes(),
        )))
    }

    fn description_for_value_subtype<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        value: v8::Local<'s, v8::Value>,
    ) -> Option<v8::UniquePtr<StringBuffer>> {
        let subtype = (self.subtypes.borrow().as_ref()?)(scope, value)?;
        Some(StringBuffer::create(StringView::from(
            subtype.description.as_bytes(),
        )))
    }

    fn console_api_message(
        &self,
        _context_group_id: i32,
        _level: i32,
        message: &StringView,
        url: &StringView,
        line_number: u32,
        _column_number: u32,
        _stack_trace: &mut V8StackTrace,
    ) {
        if let Some(sink) = &*self.console.borrow() {
            sink(ConsoleMessage {
                text: message.to_string(),
                url: url.to_string(),
                line: line_number,
                uncaught: false,
            });
        }
    }
}

/// A session's way out: its answers and events, as JSON, while the
/// browser has it.
struct Outlet {
    send: Box<dyn Fn(String)>,
    alive: Rc<Cell<bool>>,
}

impl Outlet {
    fn send(&self, message: &UniquePtr<StringBuffer>) {
        if let Some(message) = message.as_ref()
            && self.alive.get()
        {
            (self.send)(message.string().to_string());
        }
    }
}

impl ChannelImpl for Outlet {
    fn send_response(&self, _call_id: i32, message: UniquePtr<StringBuffer>) {
        self.send(&message);
    }

    fn send_notification(&self, message: UniquePtr<StringBuffer>) {
        self.send(&message);
    }

    fn flush_protocol_notifications(&self) {}
}

/// A promise reported as unhandled, until its handler comes or it goes.
struct Reported {
    promise: v8::Weak<v8::Promise>,
    exception_id: u32,
}

/// The isolate's inspector, in an isolate slot.
pub(crate) struct DevTools {
    shared: Rc<Shared>,
    inspector: V8Inspector,
    console: Rc<RefCell<Option<ConsoleSink>>>,
    subtypes: Rc<RefCell<Option<Subtypes>>>,
    default_context: Rc<RefCell<Option<v8::Weak<v8::Context>>>>,
    /// Promises reported as unhandled, oldest first.
    reported: RefCell<Vec<Reported>>,
}

impl DevTools {
    pub(crate) fn new(isolate: &mut v8::Isolate) -> Self {
        let shared = Rc::new(Shared::default());
        let console = Rc::new(RefCell::new(None));
        let subtypes = Rc::new(RefCell::new(None));
        let default_context = Rc::new(RefCell::new(None));
        let client = V8InspectorClient::new(Box::new(Client {
            shared: shared.clone(),
            console: console.clone(),
            subtypes: subtypes.clone(),
            default_context: default_context.clone(),
            // SAFETY: only used while the client is: the inspector, which
            // owns it, goes before the isolate.
            isolate: unsafe { isolate.as_raw_isolate_ptr() },
        }));
        Self {
            shared,
            inspector: V8Inspector::create(isolate, client),
            console,
            subtypes,
            default_context,
            reported: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn idle_started(&self) {
        self.inspector.idle_started();
    }

    pub(crate) fn idle_finished(&self) {
        self.inspector.idle_finished();
    }

    pub(crate) fn set_console_sink(&self, sink: ConsoleSink) {
        *self.console.borrow_mut() = Some(sink);
    }

    /// A session of a DevTools client, the browser's `session`, whose
    /// answers and events go to `send`. Fully trusted: whoever reaches the
    /// renderer's sessions already controls the browser.
    pub(crate) fn attach(&self, session: String, send: Box<dyn Fn(String)>) {
        let alive = Rc::new(Cell::new(true));
        let outlet = Outlet {
            send,
            alive: alive.clone(),
        };
        let connected = self.inspector.connect(
            CONTEXT_GROUP,
            Channel::new(Box::new(outlet)),
            StringView::empty(),
            V8InspectorClientTrustLevel::FullyTrusted,
        );
        let attached = Attached {
            session: Rc::new(connected),
            alive,
        };
        // One it replaces goes after the borrow.
        let replaced = self.shared.sessions.borrow_mut().insert(session, attached);
        drop(replaced);
    }

    /// The browser's `session` has gone. Not while script is paused: the
    /// pause's loop takes detaches itself.
    pub(crate) fn detach(&self, session: &str) {
        let removed = self.shared.sessions.borrow_mut().remove(session);
        drop(removed);
    }

    pub(crate) fn session(&self, session: &str) -> Option<Rc<V8InspectorSession>> {
        self.shared.session(session)
    }

    /// The renderer's DevTools queue, for while script is on the stack:
    /// the first one given (the renderer gives one, as it starts).
    pub(crate) fn set_queue(&self, queue: Box<dyn Queue>) {
        if self.shared.queue.set(queue).is_err() {
            tracing::warn!("the inspector has a DevTools queue already");
        }
    }

    /// The embedder's subtypes of values, from now on.
    pub(crate) fn set_subtypes(&self, subtypes: Subtypes) {
        *self.subtypes.borrow_mut() = Some(subtypes);
    }

    /// [`capture_stack`], of this inspector's isolate.
    pub(crate) fn capture_stack(&self, max_async_depth: i32) -> Option<String> {
        let stack = self.inspector.capture_stack_trace(false);
        let stack = stack.as_ref().filter(|stack| !stack.is_empty())?;
        let json = v8::crdtp::cbor_to_json(&stack.build_inspector_object(max_async_depth))?;
        String::from_utf8(json).ok()
    }
}

impl Drop for DevTools {
    /// The sessions go before the inspector: the client shares them, and
    /// the inspector owns the client.
    fn drop(&mut self) {
        let sessions = std::mem::take(&mut *self.shared.sessions.borrow_mut());
        drop(sessions);
    }
}

/// What the renderer's DevTools queue calls, from any thread, when a
/// message is to interrupt script: V8 has the isolate's thread take it at
/// the next safe point of script it runs (`Isolate::RequestInterrupt`, as
/// Blink's `InspectorTaskRunner` does), or as script next runs.
pub(crate) fn interrupter(isolate: &v8::Isolate) -> Box<dyn Fn() + Send + Sync> {
    let handle = isolate.thread_safe_handle();
    Box::new(move || {
        handle.request_interrupt(interrupted, std::ptr::null_mut());
    })
}

/// V8's interrupt: the messages that are to interrupt script.
unsafe extern "C" fn interrupted(isolate: v8::UnsafeRawIsolatePtr, _: *mut c_void) {
    // SAFETY: V8 calls it on the isolate's thread, at a safe point of
    // script it runs, with the isolate it was requested of: alive.
    let isolate = unsafe { v8::Isolate::from_raw_isolate_ptr(isolate) };
    if let Some(devtools) = devtools(&isolate) {
        devtools.shared.dispatch_interrupting();
    }
}

/// The isolate's inspector.
fn devtools(isolate: &v8::Isolate) -> Option<Rc<DevTools>> {
    isolate.get_slot::<Rc<DevTools>>().cloned()
}

/// What DevTools is to call the embedder's values (`Subtypes`), from now
/// on.
pub fn set_subtypes(isolate: &v8::Isolate, subtypes: Subtypes) {
    if let Some(devtools) = devtools(isolate) {
        devtools.set_subtypes(subtypes);
    }
}

/// `value`, in `context`, as the browser's `session` is to see it: a
/// `Runtime.RemoteObject` (JSON) in its object `group`, with no preview
/// (V8's `wrapObject`, as Blink's `ResolveNode` and
/// `BuildObjectForEventListener` wrap theirs). None if there is no such
/// session or `context` isn't told to the inspector.
pub fn wrap_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    session: &str,
    context: v8::Local<'s, v8::Context>,
    value: v8::Local<'s, v8::Value>,
    group: &str,
) -> Option<String> {
    let session = devtools(scope)?.session(session)?;
    let wrapped = session.wrap_object(
        scope,
        context,
        value,
        StringView::from(group.as_bytes()),
        false,
    )?;
    let json = v8::crdtp::cbor_to_json(&wrapped.to_bytes())?;
    String::from_utf8(json).ok()
}

/// The value the browser's `session` has as `object_id`, its context and
/// its object group (V8's `unwrapObject`); the inspector's error if it has
/// none (Blink's `NodeForRemoteObjectId` answers with it).
pub fn unwrap_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    session: &str,
    object_id: &str,
) -> Result<(v8::Local<'s, v8::Value>, v8::Local<'s, v8::Context>, String), String> {
    let session = devtools(scope)
        .and_then(|devtools| devtools.session(session))
        .ok_or_else(|| "The agent has been detached".to_owned())?;
    session
        .unwrap_object(scope, StringView::from(object_id.as_bytes()))
        .map(|(value, context, group)| {
            let group = group
                .as_ref()
                .map(|group| group.string().to_string())
                .unwrap_or_default();
            (value, context, group)
        })
        .map_err(|error| {
            error
                .as_ref()
                .map(|error| error.string().to_string())
                .unwrap_or_default()
        })
}

/// What `$0` is for the browser's `session` from now on, `$1` what it was
/// (V8's `addInspectedObject`, as Blink's `setInspectedNode` gives its
/// `InspectableNode`).
pub fn add_inspected_object(
    isolate: &v8::Isolate,
    session: &str,
    inspected: Box<dyn InspectableImpl>,
) {
    if let Some(session) = devtools(isolate).and_then(|devtools| devtools.session(session)) {
        session.add_inspected_object(Inspectable::new(inspected));
    }
}

/// Tells the inspector of `context`, a frame's realm (DevTools'
/// `Runtime.executionContextCreated`): the frame's main world, for the
/// frame DevTools calls `frame_id`; the main frame's is where a command
/// that names no context runs.
pub fn context_created(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    origin: &str,
    frame_id: Option<&str>,
    main: bool,
) {
    let Some(devtools) = devtools(scope) else {
        return;
    };
    if main {
        *devtools.default_context.borrow_mut() = Some(v8::Weak::new(scope, context));
    }
    // As Blink's `MainThreadDebugger::ContextCreated` describes a frame's
    // main world.
    let mut aux = serde_json::json!({ "isDefault": true, "type": "default" });
    if let Some(frame_id) = frame_id {
        aux["frameId"] = frame_id.into();
    }
    let aux = aux.to_string();
    devtools.inspector.context_created_with_origin(
        context,
        CONTEXT_GROUP,
        StringView::empty(),
        StringView::from(origin.as_bytes()),
        StringView::from(aux.as_bytes()),
    );
}

/// Tells the inspector every context has gone, as the main frame's
/// document is discarded (`Runtime.executionContextsCleared`; Blink's
/// `MainThreadDebugger::DidClearContextsForFrame`): the console messages
/// and the exceptions reported go with them.
pub fn contexts_cleared(isolate: &v8::Isolate) {
    let Some(devtools) = devtools(isolate) else {
        return;
    };
    devtools.inspector.reset_context_group(CONTEXT_GROUP);
    devtools.reported.borrow_mut().clear();
}

/// Tells the inspector `context` has gone with its document
/// (`Runtime.executionContextDestroyed`).
pub fn context_destroyed(isolate: &mut v8::Isolate, context: &v8::Weak<v8::Context>) {
    let Some(devtools) = devtools(isolate) else {
        return;
    };
    v8::scope!(let scope, isolate);
    // Collected already: the inspector has heard of that itself.
    if let Some(context) = context.to_local(scope) {
        devtools.inspector.context_destroyed(context);
    }
}

/// An uncaught exception, as the page's error reporting printed it
/// (`Runtime.exceptionThrown`; Blink's `MainThreadDebugger::ExceptionThrown`
/// gives `message` as `Uncaught` and `detailed` as the error event's
/// message): its id, to revoke it by if it was a promise's rejection that
/// is handled later; `None` without an inspector. `line` and `column` are
/// 1-based, 0 if unknown; `script_id` is the script's (0 if unknown),
/// without which the inspector leaves the exception itself out.
#[expect(
    clippy::too_many_arguments,
    reason = "V8's `exceptionThrown` takes as many"
)]
pub fn exception_thrown(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    message: &str,
    detailed: &str,
    exception: v8::Local<'_, v8::Value>,
    url: &str,
    line: u32,
    column: u32,
    stack: Option<v8::Local<'_, v8::StackTrace>>,
    script_id: i32,
) -> Option<u32> {
    let devtools = devtools(scope)?;
    let stack = devtools.inspector.create_stack_trace(stack);
    Some(devtools.inspector.exception_thrown(
        context,
        StringView::from(message.as_bytes()),
        exception,
        StringView::from(detailed.as_bytes()),
        StringView::from(url.as_bytes()),
        line,
        column,
        stack,
        script_id,
    ))
}

/// `message`, an exception or a rejection nothing handled, to the
/// isolate's console sink, as `console` calls go (Blink adds such an error
/// to the console as its `ExecutionContext` reports it, which
/// content_shell prints).
pub fn report_uncaught(isolate: &v8::Isolate, message: ConsoleMessage) {
    let Some(devtools) = devtools(isolate) else {
        return;
    };
    if let Some(sink) = &*devtools.console.borrow() {
        sink(ConsoleMessage {
            uncaught: true,
            ..message
        });
    }
}

/// The stack as script runs, as DevTools' `Runtime.StackTrace` (JSON),
/// with up to `max_async_depth` async parents: what a request's or a
/// frame's initiator shows (Blink's `CaptureSourceLocation`, then
/// `BuildInspectorObject`). As many frames as a session with `Runtime`
/// enabled asks for, or one. `None` if no script is running, or outside a
/// context the inspector was told of.
pub fn capture_stack(isolate: &v8::Isolate, max_async_depth: i32) -> Option<String> {
    devtools(isolate)?.capture_stack(max_async_depth)
}

/// `promise`'s rejection was reported as unhandled with `exception_id`:
/// DevTools is told if a handler comes later.
pub fn rejection_reported(
    scope: &mut v8::PinScope<'_, '_>,
    promise: v8::Local<'_, v8::Promise>,
    exception_id: u32,
) {
    let Some(devtools) = devtools(scope) else {
        return;
    };
    let mut reported = devtools.reported.borrow_mut();
    reported.retain(|reported| !reported.promise.is_empty());
    if reported.len() >= MAX_REPORTED_REJECTIONS {
        reported.remove(0);
    }
    reported.push(Reported {
        promise: v8::Weak::new(scope, promise),
        exception_id,
    });
}

/// A handler was added to `promise` after its rejection was reported:
/// DevTools strikes the error out (`Runtime.exceptionRevoked`; Blink's
/// `RejectedPromises::HandlerAdded`).
pub(crate) fn rejection_handled(
    scope: &mut v8::PinScope<'_, '_>,
    promise: v8::Local<'_, v8::Promise>,
    context: v8::Local<'_, v8::Context>,
) {
    let Some(devtools) = devtools(scope) else {
        return;
    };
    let found = {
        let mut reported = devtools.reported.borrow_mut();
        let found = reported.iter().position(|reported| {
            reported
                .promise
                .to_local(scope)
                .is_some_and(|reported| reported == promise)
        });
        found.map(|index| reported.remove(index).exception_id)
    };
    if let Some(exception_id) = found {
        devtools.inspector.exception_revoked(
            context,
            exception_id,
            StringView::from(&b"Handler added to rejected promise"[..]),
        );
    }
}

/// Puts the inspector in `isolate`'s slot.
pub(crate) fn install(isolate: &mut v8::Isolate) -> Rc<DevTools> {
    let devtools = Rc::new(DevTools::new(isolate));
    isolate.set_slot(devtools.clone());
    devtools
}
