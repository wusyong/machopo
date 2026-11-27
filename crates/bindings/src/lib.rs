//! V8 integration (`blink/renderer/platform/bindings`).
//!
//! Knows nothing about the DOM (`dom`, on elise's heap). The
//! JS wrappers for DOM types (Blink's `bindings/core/v8`) need both, so they
//! are a crate of their own, `dom_js`.
//!
//! Every V8 call in the project goes through this crate (and
//! `dom_js`) so a `v8` crate upgrade touches one place. The main pieces:
//!
//! | Type | Blink counterpart |
//! | --- | --- |
//! | [`initialize`] | `V8Initializer::InitializeMainThread` |
//! | [`V8PerIsolateData`] | `V8PerIsolateData` (one isolate + its `CppHeap` per renderer main thread) |
//! | [`ScriptState`] | `ScriptState` (one `v8::Context` per frame) |
//! | [`ModuleRecords`] | `ModuleRecord`, and the V8 half of `ModuleScript` and `ModuleRecordResolverImpl` (a realm's modules) |
//! | [`PromiseResolvers`] | the `ScriptPromiseResolver`s a realm's pending work keeps (its promises settled later, by number) |
//! | [`trace`] | Chromium's `TracingControllerImpl`, for one process: a Perfetto trace with V8's events and the embedder's spans; which entry into script is the outermost |
//! | [`set_isolate_state`], [`isolate_state_mut`], [`StateScope`], [`NoGc`], [`state_function_template`] | `V8PerIsolateData` in `gin::kEmbedderBlink`'s isolate data slot: the embedder's state (the DOM's binding's), read with a load and borrowed only through the isolate, so that its holder can't call V8 meanwhile but for what allocates nothing (`NoGc`); a callback on a template made where the state is its type borrows it with no check |
//! | [`register_embedder_heap`], [`set_isolate_state_with_embedder_heap`], [`wrap`], [`unwrap`], [`set_wrapper`], [`set_strong`], [`set_strong_value`] | `UnifiedHeapController`, `V8EmbedderRootsHandler`, `ScriptWrappable`'s wrapper, `TraceWrapperV8Reference`: a elise heap marking with V8's major GCs (registered with the isolate's state, for a binding without `unsafe`), its objects wrapped by address, its strong references to V8's values (a listener's callback, an error event's error) |
//! | [`HeapGraph`], [`Detachedness`] | `V8EmbedderGraphBuilder`: the elise heap's objects in heap snapshots, merged with their wrappers |
//! | [`gc_platform`] | gin's `V8Platform` as cppgc's platform: the elise heap's parallel work on V8's workers |
//!
//! Microtasks use the *explicit* policy: the renderer's event loop calls
//! [`V8PerIsolateData::perform_microtask_checkpoint`] after every task, as
//! the HTML spec requires, and `dom_js` checkpoints after each callback a
//! task calls with no script on the stack ([`trace::in_script`]), as
//! Blink's `MicrotasksScope` does.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]
#![allow(unsafe_code, reason = "FFI into V8 and cppgc")]

pub mod devtools;
mod gc_platform;
mod isolate_state;
mod module_record;
mod oom;
mod platform;
mod platform_tasks;
mod promise_resolvers;
mod script_state;
pub mod trace;
mod unified_heap;
mod v8_text;

pub use gc_platform::gc_platform;
pub use isolate_state::{
    InfoHandles, InfoNoGc, InfoState, NamedAssigned, NamedDeleted, NamedOptions, NamedQueried,
    NoGc, PropertyKey, Receiver, Signed, StateAccessCheck, StateCallback, StateIndexInfo,
    StateInfo, StateInfoFunction, StateInfoIndexedGetter, StateInfoIndexedSetter, StateInfoMethod,
    StateInfoNamed, StateInfoNamedGetter, StateInfoNamedSetter, StateNameInfo, StateScope,
    Unsigned, drop_isolate_state, info_handles_for_testing, info_named_getter, info_named_setter,
    info_scopes_for_testing, isolate_state_mut, set_isolate_state, set_state_access_check,
    set_state_call_as_function_handler, set_state_info_indexed_getter, set_state_named_properties,
    set_state_refused_access, state_cached_getter_template, state_function_template,
    state_info_function_template, state_info_method_template,
};
pub use module_record::{
    Compiled, Evaluation, ModuleRecords, Request, ResolveSpecifier, begin_dynamic_import,
    finish_dynamic_import, reject_dynamic_import,
};
pub use oom::OUT_OF_MEMORY_EXIT;
pub use platform::{initialize, initialize_with_flags, platform, v8_version};
pub use platform_tasks::PlatformTaskWaker;

// For `initialize_v8_before_main!`: its users need not depend on `ctor`.
#[doc(hidden)]
pub use ctor as __ctor;
pub use promise_resolvers::{
    PromiseResolvers, create_kept_promise, create_promise, hand_out, is_waiting,
    release as release_promise, take_resolver,
};
pub use script_state::{
    GlobalTemplate, NativeErrorCheck, RejectedPromise, ScriptError, ScriptLocation, ScriptState,
    V8PerIsolateData, referrer_base_url, referrer_credentials, run_classic_script_in,
    script_options, set_native_error_check,
};
pub use unified_heap::{
    Boilerplate, BoilerplateLocal, Detachedness, EmbedderHeap, EmbedderHeapState,
    EmbedderRegistration, HeapGraph, NewWrapper, OpReturn, ReturnSlot, is_wrapper, new_wrapper,
    receiver_unwraps_for_testing, register_embedder_heap, report_embedder_used_size,
    return_wrapper, set_isolate_state_with_embedder_heap, set_strong, set_strong_value,
    set_wrapper, strong, strong_value, unwrap, unwrap_object, unwrap_receiver, wrap, wrapper,
};
pub use v8_text::{LentString, ascii_scans_for_testing, borrowed_texts};

/// Throws `exception` in `scope`'s isolate (`Isolate::ThrowException`),
/// unless its script is being stopped (`TerminateExecution`, a worker's
/// `terminate()`): V8's `ThrowException` clears the exception pending
/// first, and a termination replaced by an exception would be one script
/// can catch, so that it runs on. As Blink's `V8ThrowException::
/// ThrowException`, which its `ExceptionState` throws through.
pub fn throw(scope: &mut v8::PinScope<'_, '_>, exception: v8::Local<'_, v8::Value>) {
    if scope.is_execution_terminating() {
        return;
    }
    scope.throw_exception(exception);
}

/// A callback's script stopped (`TerminateExecution`), caught by the
/// embedder: asked to stop again, until the next outermost entry into
/// script ([`trace::enter_script`]). V8 throws the stop on into the script
/// under the callback (the script that dispatched the event, defined the
/// element) as the `TryCatch` ends, but the next of its API that runs
/// script (making a wrapper, settling a promise, calling a function)
/// clears it first, and that script would run on; asked again, V8 stops
/// whatever script runs next at its first check (a function's entry, a
/// loop's jump back). With no script under it (a task's callback), the
/// next task's script runs: its entry ends the stop first (a window's,
/// stopped by DevTools' `Runtime.terminateExecution`, as V8's inspector
/// ends its stop as the outermost call completes; a worker's loop runs
/// none). Asked for whether script is under or not: V8's API can't tell
/// while a stop is pending (capturing the stack instantiates functions,
/// which reports it). Blink asks once (`WorkerThread::
/// EnsureScriptExecutionTerminates`): it stops a worker's script only
/// after 2 s, its members rarely midway.
#[cold]
#[inline(never)]
pub fn stop_again(scope: &v8::Isolate) {
    scope.terminate_execution();
    trace::stop_until_next_entry(scope.thread_safe_handle());
}
