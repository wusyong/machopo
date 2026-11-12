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
mod platform;
mod promise_resolvers;
mod script_state;
pub mod trace;
mod unified_heap;
mod v8_text;

pub use gc_platform::gc_platform;
pub use isolate_state::{
    InfoNoGc, InfoState, NamedOptions, NoGc, PropertyKey, Receiver, Signed, StateAccessCheck,
    StateCallback, StateIndexInfo, StateIndexedGetter, StateIndexedSetter, StateInfo,
    StateInfoFunction, StateInfoIndexedGetter, StateInfoMethod, StateInfoNamedGetter,
    StateInfoNamedSetter, StateMethod, StateNameInfo, StateNamedProperties, StateScope, Unsigned,
    drop_isolate_state, info_named_getter, info_named_setter, info_scopes_for_testing,
    isolate_state_mut, set_isolate_state, set_state_access_check,
    set_state_call_as_function_handler, set_state_indexed_getter, set_state_indexed_properties,
    set_state_info_indexed_getter, set_state_named_properties, set_state_refused_access,
    state_cached_getter_template, state_function_template, state_info_function_template,
    state_info_method_template, state_method_template,
};
pub use module_record::{
    Compiled, Evaluation, ModuleRecords, Request, ResolveSpecifier, begin_dynamic_import,
    finish_dynamic_import, reject_dynamic_import,
};
pub use platform::{initialize, initialize_with_flags, platform, v8_version};

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
    Detachedness, EmbedderHeap, EmbedderHeapState, EmbedderRegistration, HeapGraph, OpReturn,
    ReturnSlot, is_wrapper, receiver_unwraps_for_testing, register_embedder_heap,
    report_embedder_used_size, return_wrapper, set_isolate_state_with_embedder_heap, set_strong,
    set_strong_value, set_wrapper, strong, strong_value, unwrap, unwrap_object, unwrap_receiver,
    wrap, wrapper,
};
pub use v8_text::{LentString, ascii_scans_for_testing, borrowed_texts};
