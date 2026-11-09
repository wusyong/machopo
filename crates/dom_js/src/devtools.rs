//! The engine's frames as V8's inspector knows them (Blink's
//! `MainThreadDebugger::ContextCreated`, `ContextWillBeDestroyed`,
//! `DidClearContextsForFrame`): each frame's realm told as it is made, for
//! the frame DevTools calls it by, and as it goes (its frame navigated to
//! a new realm, or gone); all at once as the top-level frame navigates.
//! The exceptions reported, told to its console (`exception_thrown`), and
//! the host's DevTools queue, as the inspector takes from it.

use bindings::devtools::Queued;
use dom::NodeRoot;
use dom_api::{DevToolsQueue, DevToolsQueued, FrameId};

use crate::state::Scope;

/// The host's DevTools queue, as `bindings`' inspector takes from it.
pub(crate) struct Queue(pub(crate) Box<dyn DevToolsQueue>);

impl bindings::devtools::Queue for Queue {
    fn next(&self) -> Queued {
        match self.0.next() {
            DevToolsQueued::Attach(session) => Queued::Attach(session),
            DevToolsQueued::Detach(session) => Queued::Detach(session),
            DevToolsQueued::Message { session, message } => Queued::Message { session, message },
            DevToolsQueued::Quit => Queued::Quit,
            DevToolsQueued::Render => Queued::Render,
        }
    }

    fn take_interrupting(&self) -> Vec<(String, String)> {
        self.0.take_interrupting()
    }

    fn put_back(&self, queued: Vec<Queued>) {
        self.0.put_back(
            queued
                .into_iter()
                .map(|queued| match queued {
                    Queued::Attach(session) => DevToolsQueued::Attach(session),
                    Queued::Detach(session) => DevToolsQueued::Detach(session),
                    Queued::Message { session, message } => {
                        DevToolsQueued::Message { session, message }
                    }
                    Queued::Quit => DevToolsQueued::Quit,
                    Queued::Render => DevToolsQueued::Render,
                })
                .collect(),
        );
    }

    /// Lends the host the page through the isolate (`devtools_page`).
    fn answer_here(&self, isolate: &mut v8::Isolate, session: &str, message: &str) -> bool {
        self.0.answer_here(
            session,
            message,
            &mut crate::devtools_page::Page::new(isolate),
        )
    }

    fn paused(&self, isolate: &mut v8::Isolate) {
        self.0.paused(&mut crate::devtools_page::Page::new(isolate));
    }

    fn waiting(&self, isolate: &mut v8::Isolate) {
        self.0
            .waiting(&mut crate::devtools_page::Page::new(isolate));
    }

    fn render(&self, isolate: &mut v8::Isolate) {
        self.0.render(&mut crate::devtools_page::Page::new(isolate));
    }
}

/// Tells the inspector of `context`, the realm made for `frame`, whose
/// document has the origin `origin_of` has (an initial `about:blank`'s,
/// its parent's): for the frame DevTools calls it by, the top-level one's
/// where a command that names no context runs.
pub(crate) fn realm_created(
    scope: &mut Scope<'_, '_, '_>,
    context: v8::Local<'_, v8::Context>,
    frame: FrameId,
    origin_of: &NodeRoot,
) {
    let (origin, frame_id) = {
        let (state, _) = scope.state();
        // As Blink's `SecurityOrigin::ToRawString`, which Chrome gives: an
        // opaque one's scheme and host are empty, "://".
        let origin = state
            .dom
            .read(|cx| match dom_api::origin(cx, origin_of.get(cx.gc)) {
                Some(origin @ url::Origin::Tuple(..)) => origin.ascii_serialization(),
                _ => "://".to_owned(),
            });
        let frame_id = state.devtools_frame_ids.as_ref().and_then(|ids| ids(frame));
        (origin, frame_id)
    };
    bindings::devtools::context_created(
        scope.scope(),
        context,
        &origin,
        frame_id.as_deref(),
        frame == crate::frames::TOP,
    );
}

/// Tells the inspector `context`, a frame's realm, has gone: its frame
/// navigated to another, or went.
pub(crate) fn realm_destroyed(scope: &mut v8::PinScope<'_, '_>, context: &v8::Global<v8::Context>) {
    let weak = v8::Weak::new(scope, context);
    bindings::devtools::context_destroyed(scope, &weak);
}

/// Tells the inspector every realm has gone, as the top-level frame
/// navigates (`Runtime.executionContextsCleared`): the console messages
/// and the exceptions reported go with them.
pub(crate) fn realms_cleared(isolate: &v8::Isolate) {
    bindings::devtools::contexts_cleared(isolate);
}

/// Where DevTools is told an exception was (Blink's
/// `CaptureSourceLocation`): the top of its stack if that is in the script
/// it was thrown in (or that isn't known), else where it was thrown. The
/// stack is there while a session has `Runtime` on.
pub(crate) struct Place {
    pub(crate) url: String,
    /// 1-based, 0 if not known; so is `column`.
    pub(crate) line: u32,
    pub(crate) column: u32,
    /// 0 if not known.
    pub(crate) script_id: i32,
}

impl Place {
    /// Where V8's `message` of an exception says it was thrown.
    pub(crate) fn of_message(
        scope: &mut v8::PinScope<'_, '_>,
        message: Option<v8::Local<'_, v8::Message>>,
    ) -> Self {
        let Some(message) = message else {
            return Self::unknown();
        };
        Self {
            url: message
                .get_script_resource_name(scope)
                .filter(|name| name.is_string())
                .map(|name| name.to_rust_string_lossy(scope))
                .unwrap_or_default(),
            line: message
                .get_line_number(scope)
                .and_then(|line| u32::try_from(line).ok())
                .unwrap_or(0),
            // V8's is 0-based, -1 for none.
            column: u32::try_from(message.get_start_column()).map_or(0, |c| c.saturating_add(1)),
            script_id: message.get_script_id(),
        }
    }

    /// Nowhere: a muted error's.
    pub(crate) fn unknown() -> Self {
        Self {
            url: String::new(),
            line: 0,
            column: 0,
            script_id: 0,
        }
    }

    /// The top of `stack`, if it is in the script thrown in.
    fn at(
        self,
        scope: &mut v8::PinScope<'_, '_>,
        stack: Option<v8::Local<'_, v8::StackTrace>>,
    ) -> Self {
        let Some(top) = stack.and_then(|stack| stack.get_frame(scope, 0)) else {
            return self;
        };
        let script_id = i32::try_from(top.get_script_id()).unwrap_or(0);
        if self.script_id != 0 && self.line != 0 && script_id != self.script_id {
            return self;
        }
        let place = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        Self {
            url: top
                .get_script_name_or_source_url(scope)
                .map(|url| url.to_rust_string_lossy(scope))
                .unwrap_or_default(),
            line: place(top.get_line_number()),
            column: place(top.get_column()),
            script_id,
        }
    }
}

/// An exception reported in `context`'s realm that nothing canceled, told
/// to DevTools' console (`Runtime.exceptionThrown`), as Blink's
/// `ExecutionContext` reports an error event: `text` its kind
/// (`Uncaught`), `detailed` what the error event's message is, `thrown`
/// where it was thrown. Its id, to revoke it by; `None` without an
/// inspector.
pub(crate) fn exception_thrown(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    text: &str,
    detailed: &str,
    exception: v8::Local<'_, v8::Value>,
    thrown: Place,
) -> Option<u32> {
    let stack = v8::Exception::get_stack_trace(scope, exception);
    let place = thrown.at(scope, stack);
    bindings::devtools::exception_thrown(
        scope,
        context,
        text,
        detailed,
        exception,
        &place.url,
        place.line,
        place.column,
        stack,
        place.script_id,
    )
}

/// `promise`, rejected with `reason` and no handler, its
/// `unhandledrejection` not canceled: told to DevTools' console, which
/// strikes it out if a handler comes later (Blink's `RejectedPromises`),
/// with where the reason was made, if it is an error.
pub(crate) fn rejection_unhandled(
    scope: &mut v8::PinScope<'_, '_>,
    context: v8::Local<'_, v8::Context>,
    promise: v8::Local<'_, v8::Promise>,
    reason: v8::Local<'_, v8::Value>,
    reason_text: &str,
) {
    let message = v8::Exception::create_message(scope, reason);
    let made = Place::of_message(scope, Some(message));
    let id = exception_thrown(
        scope,
        context,
        "Uncaught (in promise)",
        &format!("Uncaught (in promise) {reason_text}"),
        reason,
        made,
    );
    if let Some(id) = id {
        bindings::devtools::rejection_reported(scope, promise, id);
    }
}
