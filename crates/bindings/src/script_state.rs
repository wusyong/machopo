use std::ffi::CStr;
use std::fmt;
use std::rc::Rc;
use std::sync::Once;

/// One V8 isolate with its `CppHeap` attached (`blink::V8PerIsolateData`).
///
/// Lives on the renderer main thread and must not leave it (`OwnedIsolate`
/// is `!Send`). The DOM's objects are on a elise heap of their own, which
/// marks with V8's major GCs through the `CppHeap` (`unified_heap`): it is
/// V8's only API for an embedder to trace with its GCs, so one GC traces JS
/// wrappers and the DOM together, as with Oilpan. Nothing of machopo's is
/// allocated on the `CppHeap` itself.
pub struct V8PerIsolateData {
    /// The isolate's inspector, and DevTools' sessions of it (also in a
    /// slot, which `Drop` empties first, so it goes before the isolate).
    devtools: Rc<crate::devtools::DevTools>,
    isolate: v8::OwnedIsolate,
}

impl V8PerIsolateData {
    pub fn new() -> Self {
        crate::initialize();
        let heap = v8::cppgc::Heap::create(
            v8::V8::get_current_platform(),
            v8::cppgc::HeapCreateParams {
                // Incremental: V8 advances the elise heap's marking with this
                // one's steps, and its parallel work is the elise heap's own
                // (`gc_platform`), so concurrent marking here would mark
                // nothing (nothing of machopo's is on this heap).
                marking_support: v8::cppgc::MarkingType::Incremental,
                // V8 overrides this when the heap is attached to the isolate
                // (`CppHeap::UpdateGCCapabilitiesFromFlags`).
                sweeping_support: v8::cppgc::SweepingType::IncrementalAndConcurrent,
            },
        );
        let mut isolate = v8::Isolate::new(v8::CreateParams::default().cpp_heap(heap));
        // HTML's event loop decides when microtasks run, not V8.
        isolate.set_microtasks_policy(v8::MicrotasksPolicy::Explicit);
        // V8 records an error's full stack for DevTools only while a session
        // has `Runtime` on: the inspector turns it on and off, as in Chrome.
        isolate.set_promise_reject_callback(promise_reject_callback);
        isolate.set_host_initialize_import_meta_object_callback(
            crate::module_record::initialize_import_meta,
        );
        // Whether V8 checks wrap tags is fixed when it is built, so one
        // isolate is enough.
        static WRAP_TAGS_CHECKED: Once = Once::new();
        WRAP_TAGS_CHECKED.call_once(|| check_wrap_tags(&mut isolate));
        // With the isolate, as Blink's `MainThreadDebugger`: V8's builtin
        // `console` reaches the page through it.
        let devtools = crate::devtools::install(&mut isolate);
        // A string's text read in place (`borrow_ascii`) is safe on it.
        crate::v8_text::guard_gcs(&mut isolate);
        Self { devtools, isolate }
    }

    /// Where `console`'s messages go (the renderer prints them).
    pub fn set_console_sink(&mut self, sink: crate::devtools::ConsoleSink) {
        self.devtools.set_console_sink(sink);
    }

    /// What captures the stack of the script running, with every async
    /// parent ([`devtools::capture_stack`](crate::devtools::capture_stack)),
    /// for whoever has no scope of the isolate (the renderer's frames, as
    /// script asks them for a load). `None` once the isolate has gone.
    pub fn stack_capturer(&self) -> Rc<dyn Fn() -> Option<String>> {
        let devtools = Rc::downgrade(&self.devtools);
        Rc::new(move || devtools.upgrade()?.capture_stack(i32::MAX))
    }

    /// A DevTools session of the browser's `session`, whose answers and
    /// events go to `send`, as JSON.
    pub fn devtools_attach(&mut self, session: String, send: Box<dyn Fn(String)>) {
        self.devtools.attach(session, send);
    }

    /// The browser's `session` has gone.
    pub fn devtools_detach(&mut self, session: &str) {
        self.devtools.detach(session);
    }

    /// The renderer's DevTools queue, which the inspector takes from while
    /// script is paused or a message is to interrupt it (the first one
    /// given): what the queue is to call, from any thread, when one is.
    pub fn set_devtools_queue(
        &mut self,
        queue: Box<dyn crate::devtools::Queue>,
    ) -> Box<dyn Fn() + Send + Sync> {
        self.devtools.set_queue(queue);
        crate::devtools::interrupter(&self.isolate)
    }

    /// Hands `message`, a DevTools command as JSON, to the browser's
    /// `session`; a session that has gone takes none. V8 may run page
    /// script for it (`Runtime.evaluate`), so only a script entry calls it.
    pub fn dispatch_devtools_message(&mut self, session: &str, message: &str) -> bool {
        let Some(session) = self.devtools.session(session) else {
            return false;
        };
        session.dispatch_protocol_message(v8::inspector::StringView::from(message.as_bytes()));
        true
    }

    /// Tells DevTools `context` has gone with its document.
    pub fn devtools_context_destroyed(&mut self, context: &v8::Weak<v8::Context>) {
        crate::devtools::context_destroyed(&mut self.isolate, context);
    }

    pub fn isolate(&mut self) -> &mut v8::Isolate {
        &mut self.isolate
    }

    /// Whether the isolate's thread is waiting for its next task or in
    /// one, though not in script: DevTools' `Profiler` tells the two apart
    /// (`(idle)`, `(program)`), as Blink tells the inspector around each
    /// task (`ThreadDebugger::IdleStarted`, `IdleFinished`). Not while
    /// script runs.
    pub fn set_idle(&mut self, idle: bool) {
        if idle {
            self.devtools.idle_started();
        } else {
            self.devtools.idle_finished();
        }
    }

    /// HTML "perform a microtask checkpoint".
    pub fn perform_microtask_checkpoint(&mut self) {
        self.isolate.perform_microtask_checkpoint();
    }

    /// The promises rejected without a handler since the last call, and
    /// not handled since (HTML's "about-to-be-notified rejected promises
    /// list", Blink's `RejectedPromises`), oldest first. The caller
    /// notifies about them ("notify about rejected promises") after a
    /// microtask checkpoint.
    pub fn take_rejected_promises(&mut self) -> Vec<RejectedPromise> {
        self.isolate
            .get_slot_mut::<RejectedPromises>()
            .map(|pending| std::mem::take(&mut pending.0))
            .unwrap_or_default()
    }

    /// Full V8 GC, the elise heap's marking with it, that doesn't scan the stack (Blink's
    /// `PreciselyCollectGarbage`): a stale pointer a returned call left on
    /// it would keep garbage alive now and then. Requires `--expose-gc`; for
    /// tests that shake out missing `trace()` edges, and check what was
    /// collected. Only call it with nothing on the stack that needs to stay
    /// alive: everything rooted by a handle or a root, as outside the DOM's
    /// operations.
    pub fn collect_garbage_for_testing(&mut self) {
        self.isolate
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Full,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }

    /// Runs the V8 platform tasks that are ready, e.g. incremental marking
    /// steps and GC finalization. Some of them GC with `kNoHeapPointers`
    /// (no stack scan), so this must only be called from the outermost event
    /// loop, never while unrooted GC pointers are on the stack. Taking
    /// `&mut self` guarantees no DOM scope is alive. Whether any task ran.
    pub fn pump_platform_tasks(&mut self) -> bool {
        let platform = v8::V8::get_current_platform();
        let mut ran = false;
        #[allow(clippy::disallowed_methods)] // This is the sanctioned wrapper.
        while v8::Platform::pump_message_loop(&platform, &self.isolate, false) {
            ran = true;
        }
        ran
    }
}

/// A promise rejected while it had no handler: what
/// [`V8PerIsolateData::take_rejected_promises`] returns.
pub struct RejectedPromise {
    pub promise: v8::Global<v8::Promise>,
    /// What it was rejected with (`undefined` if V8 gave nothing).
    pub reason: v8::Global<v8::Value>,
    /// The realm the promise was made in (HTML's "relevant settings
    /// object"), whose global is notified.
    pub context: v8::Global<v8::Context>,
}

/// The isolate's rejected promises not yet notified about, in a slot.
#[derive(Default)]
struct RejectedPromises(Vec<RejectedPromise>);

/// V8's report of a promise rejected without a handler, or of a handler
/// added to one afterwards (HTML's `HostPromiseRejectionTracker`). A
/// handler added before the notification takes the promise off the list;
/// one added after it would be `rejectionhandled`, which isn't
/// implemented.
/// Whether an API wrapper is a native error (`set_native_error_check`).
pub type NativeErrorCheck = fn(&mut v8::PinScope<'_, '_>, v8::Local<'_, v8::Object>) -> bool;

/// What `Error.isError` answers for an API wrapper of `isolate` (V8 asks
/// of no other object): whether `check` says it is a native error, as
/// Blink's `V8Initializer` answers true for a `DOMException`'s (WebIDL:
/// one has [[ErrorData]]). `check` runs in the middle of script's call:
/// it reads the object and runs no JS.
pub fn set_native_error_check(isolate: &mut v8::Isolate, check: NativeErrorCheck) {
    isolate.set_slot(NativeErrorSlot(check));
    isolate.set_is_js_api_wrapper_native_error_callback(is_native_error_wrapper);
}

struct NativeErrorSlot(NativeErrorCheck);

/// V8's `IsJSApiWrapperNativeErrorCallback`: the isolate's check's answer.
unsafe extern "C" fn is_native_error_wrapper(
    _: v8::UnsafeRawIsolatePtr,
    object: v8::Local<v8::Object>,
) -> bool {
    // SAFETY: V8 calls this on the isolate's thread as script runs
    // `Error.isError`, in its context (the isolate entered): a callback
    // scope may be opened on the object.
    v8::callback_scope!(unsafe scope, object);
    let Some(NativeErrorSlot(check)) = scope.get_slot::<NativeErrorSlot>() else {
        return false;
    };
    let check = *check;
    let object = v8::Local::new(scope, object);
    check(scope, object)
}

extern "C" fn promise_reject_callback(message: v8::PromiseRejectMessage) {
    // SAFETY: V8 calls this on the isolate's thread with a valid message,
    // inside script execution, so a callback scope may be opened on it.
    v8::callback_scope!(unsafe scope, &message);
    let promise = message.get_promise();
    match message.get_event() {
        v8::PromiseRejectEvent::PromiseRejectWithNoHandler => {
            let reason = message
                .get_value()
                .unwrap_or_else(|| v8::undefined(scope).into());
            let context = promise
                .get_creation_context(scope)
                .unwrap_or_else(|| scope.get_current_context());
            let rejected = RejectedPromise {
                promise: v8::Global::new(scope, promise),
                reason: v8::Global::new(scope, reason),
                context: v8::Global::new(scope, context),
            };
            if scope.get_slot::<RejectedPromises>().is_none() {
                scope.set_slot(RejectedPromises::default());
            }
            scope
                .get_slot_mut::<RejectedPromises>()
                .expect("just set")
                .0
                .push(rejected);
        }
        v8::PromiseRejectEvent::PromiseHandlerAddedAfterReject => {
            let context = promise
                .get_creation_context(scope)
                .unwrap_or_else(|| scope.get_current_context());
            // Reported already: DevTools strikes it out.
            crate::devtools::rejection_handled(scope, promise, context);
            let promise = v8::Global::new(scope, promise);
            if let Some(pending) = scope.get_slot_mut::<RejectedPromises>() {
                pending.0.retain(|rejected| rejected.promise != promise);
            }
        }
        // Resolving or rejecting a settled promise: nothing to report.
        v8::PromiseRejectEvent::PromiseRejectAfterResolved
        | v8::PromiseRejectEvent::PromiseResolveAfterResolved => {}
    }
}

/// The `v8::Object::wrap` tag `check_wrap_tags` wraps with; it unwraps
/// with the next one as well. Both are embedder tags, below V8's own
/// (`kFirstV8InternalTag`, 0x6000); the probe never reaches JS, so they may
/// equal tags the DOM uses.
const CHECK_TAG: u16 = 0x5ffe;

/// The cppgc object `check_wrap_tags` wraps.
struct TagProbe;

// SAFETY: no cppgc pointers, nothing to trace.
unsafe impl v8::cppgc::GarbageCollected for TagProbe {
    fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}
    fn get_name(&self) -> &'static CStr {
        c"TagProbe"
    }
}

/// Panics unless `v8::Object::unwrap` checks the tag `wrap` was given:
/// an object wrapped with one tag must unwrap to nothing with another.
///
/// `unified_heap` relies on it to tell the elise heap's types apart, and on V8's own
/// wrapped objects (such as `WebAssembly.MemoryMapDescriptor`) not
/// unwrapping as one of them. V8 checks only when built with pointer
/// compression; without it, `unwrap` returns the raw field whatever the tag,
/// and running on would let script pass one type off as another. So this
/// stops the process instead.
fn check_wrap_tags(isolate: &mut v8::Isolate) {
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let template = v8::FunctionTemplate::new(
        scope,
        |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
    );
    let object = template
        .get_function(scope)
        .and_then(|f| f.new_instance(scope, &[]))
        .expect("an object from a FunctionTemplate");
    // SAFETY: the probe stays on the stack until `object` wraps it (which
    // keeps it alive), and the unwrapped pointers are only compared with
    // null.
    let checks_tags = unsafe {
        let heap = scope
            .get_cpp_heap()
            .expect("isolate was created with a CppHeap");
        let probe = v8::cppgc::make_garbage_collected(heap, TagProbe);
        v8::Object::wrap::<CHECK_TAG, TagProbe>(scope, object, &probe);
        v8::Object::unwrap::<CHECK_TAG, TagProbe>(scope, object).is_some()
            && v8::Object::unwrap::<{ CHECK_TAG + 1 }, TagProbe>(scope, object).is_none()
    };
    assert!(
        checks_tags,
        "this V8 doesn't check v8::Object::wrap tags (built without pointer \
         compression?); machopo's DOM bindings are unsound without it. Link the \
         `_ptrcomp` V8 (the `v8_enable_pointer_compression` feature)."
    );
}

impl Default for V8PerIsolateData {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for V8PerIsolateData {
    /// The embedder's state goes first (`set_isolate_state`: the DOM's
    /// heap, registered with V8, and its handles), then the inspector: the
    /// slot's handle, then the sessions and the last handle as the fields
    /// drop, before the isolate.
    fn drop(&mut self) {
        // SAFETY: the isolate's owner drops it, as it goes: no callback of
        // it runs after.
        unsafe { crate::drop_isolate_state(&mut self.isolate) };
        self.isolate.remove_slot::<Rc<crate::devtools::DevTools>>();
    }
}

/// Where a script came from, for stack traces and error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScriptLocation<'a> {
    pub url: &'a str,
    /// The line of `url` the script's first line is (1-based): an inline
    /// script starts where its start tag ends.
    pub line: u32,
    /// The script's base URL, which an `import()` in it resolves against
    /// (Blink's `ReferrerScriptInfo`): an external script's URL, the
    /// document's base URL for an inline one. V8 keeps it as the script's
    /// host-defined options and hands it back to the dynamic import
    /// callback ([`referrer_base_url`]); `None`, and the callback finds
    /// none (the importer then takes the document's).
    pub base_url: Option<&'a str>,
}

/// The host-defined options of a script whose base URL is `base_url`
/// (see [`ScriptLocation::base_url`]): a one-string array.
pub(crate) fn referrer_options<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    base_url: Option<&str>,
) -> Option<v8::Local<'s, v8::Data>> {
    script_options(scope, base_url?, None)
}

/// The host-defined options of a script whose base URL is `base_url`, and,
/// if `credentials` names one, whose fetch options' credentials mode it is
/// (HTML's referencing script's fetch options, which an `import()` in it
/// fetches with: Blink's `ReferrerScriptInfo`): a one- or two-string
/// array, for `ScriptOrigin`'s host-defined options.
pub fn script_options<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    base_url: &str,
    credentials: Option<&str>,
) -> Option<v8::Local<'s, v8::Data>> {
    let base_url = v8::String::new(scope, base_url)?;
    let credentials = match credentials {
        Some(credentials) => Some(v8::String::new(scope, credentials)?),
        None => None,
    };
    let options = v8::PrimitiveArray::new(scope, if credentials.is_some() { 2 } else { 1 });
    options.set(scope, 0, base_url.into());
    if let Some(credentials) = credentials {
        options.set(scope, 1, credentials.into());
    }
    Some(options.into())
}

/// The credentials mode a script was compiled with
/// ([`script_options`]), from the host-defined options V8 hands the
/// dynamic import callback; `None` for a script without one.
pub fn referrer_credentials(
    scope: &mut v8::PinScope<'_, '_>,
    options: v8::Local<'_, v8::Data>,
) -> Option<String> {
    let options = v8::Local::<v8::FixedArray>::try_from(options).ok()?;
    if options.length() != 2 {
        return None;
    }
    let credentials = options.get(scope, 1)?;
    let credentials = v8::Local::<v8::Value>::try_from(credentials).ok()?;
    let credentials = v8::Local::<v8::String>::try_from(credentials).ok()?;
    Some(credentials.to_rust_string_lossy(scope))
}

/// The base URL a script was compiled with, from the host-defined options
/// V8 hands the dynamic import callback; `None` for a script without one
/// (an event handler's), or none running.
pub fn referrer_base_url(
    scope: &mut v8::PinScope<'_, '_>,
    options: v8::Local<'_, v8::Data>,
) -> Option<String> {
    // A `PrimitiveArray` is a `FixedArray` inside V8, and comes back as
    // one.
    let options = v8::Local::<v8::FixedArray>::try_from(options).ok()?;
    if !(1..=2).contains(&options.length()) {
        return None;
    }
    let base_url = options.get(scope, 0)?;
    let base_url = v8::Local::<v8::Value>::try_from(base_url).ok()?;
    let base_url = v8::Local::<v8::String>::try_from(base_url).ok()?;
    Some(base_url.to_rust_string_lossy(scope))
}

/// An uncaught exception from script.
#[derive(Clone, PartialEq)]
pub struct ScriptError {
    pub message: String,
    pub line: Option<usize>,
    /// 1-based, as HTML's `ErrorEvent.colno`.
    pub column: Option<usize>,
    /// The value thrown, for HTML's "report an exception" (`None` if the
    /// script didn't run: the source was too large).
    pub exception: Option<v8::Global<v8::Value>>,
    /// The id of the script it was thrown in, for DevTools (0: none).
    pub script_id: i32,
}

impl fmt::Debug for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScriptError")
            .field("message", &self.message)
            .field("line", &self.line)
            .field("column", &self.column)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for ScriptError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "{} (line {line})", self.message),
            None => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for ScriptError {}

/// Makes the template of a realm's global object
/// (`ScriptState::with_global_template`).
pub type GlobalTemplate =
    for<'s, 'i> fn(&mut v8::PinScope<'s, 'i, ()>) -> v8::Local<'s, v8::ObjectTemplate>;

/// One JS realm (`blink::ScriptState`): a `v8::Context` plus the globals we
/// install into it. One per frame.
pub struct ScriptState {
    context: v8::Global<v8::Context>,
}

impl ScriptState {
    pub fn new(data: &mut V8PerIsolateData) -> Self {
        Self::create(data, None)
    }

    /// The realm of `context`, made earlier by one of the constructors:
    /// for code that has a context (a promise's creation context, say) and
    /// needs its `ScriptState`.
    pub fn from_context(context: v8::Global<v8::Context>) -> Self {
        Self { context }
    }

    /// A realm whose global object is made from the template `global`
    /// returns (Blink's `Window` instance template): its own properties and
    /// its prototype chain come from the template.
    pub fn with_global_template(data: &mut V8PerIsolateData, global: GlobalTemplate) -> Self {
        Self::create(data, Some(global))
    }

    /// [`with_global_template`](Self::with_global_template)'s realm, made
    /// in a scope of the isolate while script runs (a binding's): Blink
    /// makes an iframe's context as the iframe is inserted.
    pub fn with_global_template_in(
        scope: &mut v8::PinScope<'_, '_, ()>,
        global: GlobalTemplate,
    ) -> Self {
        Self::create_in(scope, Some(global))
    }

    fn create(data: &mut V8PerIsolateData, global: Option<GlobalTemplate>) -> Self {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        Self::create_in(scope, global)
    }

    fn create_in(scope: &mut v8::PinScope<'_, '_, ()>, global: Option<GlobalTemplate>) -> Self {
        let options = v8::ContextOptions {
            global_template: global.map(|global| global(scope)),
            ..Default::default()
        };
        let context = v8::Context::new(scope, options);
        let scope = &mut v8::ContextScope::new(scope, context);
        Self {
            context: v8::Global::new(scope, context),
        }
    }

    /// The context, e.g. for installing globals into it.
    pub fn context(&self) -> &v8::Global<v8::Context> {
        &self.context
    }

    /// Compiles and runs a classic script (`ClassicScript::RunScript`).
    /// Returns the completion value stringified, like a devtools console.
    /// Microtasks are *not* run; the caller's event loop does that.
    pub fn evaluate_classic_script(
        &self,
        data: &mut V8PerIsolateData,
        source: &str,
    ) -> Result<String, ScriptError> {
        self.evaluate_classic_script_from(data, source, None)
    }

    /// [`evaluate_classic_script`](Self::evaluate_classic_script) of a
    /// script from `location` (V8's script origin: what stack traces and
    /// error messages show).
    pub fn evaluate_classic_script_from(
        &self,
        data: &mut V8PerIsolateData,
        source: &str,
        location: Option<ScriptLocation<'_>>,
    ) -> Result<String, ScriptError> {
        let _span = trace_event::trace_event!("EvaluateScript");
        let _entry = crate::trace::enter_script();
        let isolate = data.isolate();
        v8::scope_with_context!(let scope, isolate, &self.context);
        compile_and_run(scope, source, location)
    }
}

/// Runs `source` as a classic script from `location` in `context`
/// (another realm's, say), from inside a binding's `scope` (HTML's
/// "execute the script element" for one script inserted, which runs before
/// the call that inserted it returns). Microtasks are not run: script is
/// running already.
pub fn run_classic_script_in<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    context: v8::Local<'s, v8::Context>,
    source: &str,
    location: ScriptLocation<'_>,
) -> Result<(), ScriptError> {
    let scope = &mut v8::ContextScope::new(scope, context);
    compile_and_run(scope, source, Some(location)).map(|_| ())
}

/// Compiles and runs `source` in the scope's context: its completion value,
/// stringified, or what it threw.
fn compile_and_run(
    scope: &mut v8::PinScope<'_, '_>,
    source: &str,
    location: Option<ScriptLocation<'_>>,
) -> Result<String, ScriptError> {
    v8::tc_scope!(let tc, scope);
    let error = |tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>| {
        let line = tc.message().and_then(|m| m.get_line_number(tc));
        let column = tc
            .message()
            .and_then(|m| m.get_start_column().checked_add(1));
        let exception = tc.exception();
        let message = exception
            .map(|e| e.to_rust_string_lossy(tc))
            .unwrap_or_else(|| "unknown error".into());
        ScriptError {
            message,
            line,
            column,
            exception: exception.map(|e| v8::Global::new(tc, e)),
            script_id: tc.message().map_or(0, |m| m.get_script_id()),
        }
    };

    let Some(code) = v8::String::new(tc, source) else {
        return Err(ScriptError {
            message: "source too large".into(),
            line: None,
            column: None,
            exception: None,
            script_id: 0,
        });
    };
    let origin = location.map(|location| {
        let name = v8::String::new(tc, location.url).unwrap_or_else(|| v8::String::empty(tc));
        let line_offset = i32::try_from(location.line.saturating_sub(1)).unwrap_or(i32::MAX);
        let options = referrer_options(tc, location.base_url);
        v8::ScriptOrigin::new(
            tc,
            name.into(),
            line_offset,
            0,
            // Shared cross-origin: machopo doesn't mute any script's errors
            // (Blink's `SanitizeScriptErrors::kDoNotSanitize`), so DevTools
            // gets the exception itself.
            true,
            0,
            None,
            false,
            false,
            false,
            options,
        )
    });
    let Some(script) = v8::Script::compile(tc, code, origin.as_ref()) else {
        return Err(error(tc));
    };
    match script.run(tc) {
        Some(value) => Ok(value.to_rust_string_lossy(tc)),
        None => Err(error(tc)),
    }
}
