//! The script side's basics: V8 runs, microtasks are explicit, and
//! the isolate's CppHeap is attached and collects unreachable wrappers.
//!
//! These link the real V8 static library, so run them locally:
//! `cargo test -p bindings`.

#![allow(
    unsafe_code,
    reason = "a GC type and a forced GC, as the `v8` crate requires"
)]

use std::sync::atomic::{AtomicUsize, Ordering};

use bindings::{ScriptState, V8PerIsolateData};

fn setup() -> (V8PerIsolateData, ScriptState) {
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    (data, state)
}

#[test]
fn evaluates_script() {
    let (mut data, state) = setup();
    assert_eq!(
        state.evaluate_classic_script(&mut data, "6 * 7").unwrap(),
        "42"
    );
    assert_eq!(
        state
            .evaluate_classic_script(&mut data, "typeof console.log")
            .unwrap(),
        "function"
    );
}

#[test]
fn reports_uncaught_exceptions() {
    let (mut data, state) = setup();
    let err = state
        .evaluate_classic_script(&mut data, "\nthrow new Error('boom')")
        .unwrap_err();
    assert_eq!(err.message, "Error: boom");
    assert_eq!(err.line, Some(2));
    let err = state
        .evaluate_classic_script(&mut data, "let = ;")
        .unwrap_err();
    assert!(err.message.starts_with("SyntaxError"), "{err}");
}

/// A script's location: errors name its URL, and count lines from where it
/// starts in it.
#[test]
fn errors_count_lines_from_the_script_location() {
    let (mut data, state) = setup();
    let location = bindings::ScriptLocation {
        url: "https://example.test/page.html",
        line: 10,
        base_url: None,
    };
    let err = state
        .evaluate_classic_script_from(&mut data, "\nthrow new Error('boom')", Some(location))
        .unwrap_err();
    assert_eq!(err.line, Some(11));
}

#[test]
fn microtasks_wait_for_checkpoint() {
    let (mut data, state) = setup();
    state
        .evaluate_classic_script(
            &mut data,
            "globalThis.log = []; Promise.resolve().then(() => log.push('micro')); log.push('sync');",
        )
        .unwrap();
    assert_eq!(
        state
            .evaluate_classic_script(&mut data, "log.join()")
            .unwrap(),
        "sync"
    );
    data.perform_microtask_checkpoint();
    assert_eq!(
        state
            .evaluate_classic_script(&mut data, "log.join()")
            .unwrap(),
        "sync,micro"
    );
}

static DROPS: AtomicUsize = AtomicUsize::new(0);

struct Probe;

// SAFETY: `Probe` holds no cppgc pointers, so there is nothing to trace.
unsafe impl v8::cppgc::GarbageCollected for Probe {
    fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}
    fn get_name(&self) -> &'static std::ffi::CStr {
        c"Probe"
    }
}

impl Drop for Probe {
    fn drop(&mut self) {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test's count: far from wrapping"
        )]
        DROPS.fetch_add(1, Ordering::SeqCst);
    }
}

const TAG: u16 = 1;

/// `makeProbe()`: returns a JS object wrapping a fresh cppgc `Probe`.
fn make_probe(
    scope: &mut v8::PinScope<'_, '_>,
    _args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let templ = v8::FunctionTemplate::new(
        scope,
        |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
    );
    let obj = templ
        .get_function(scope)
        .unwrap()
        .new_instance(scope, &[])
        .unwrap();
    assert!(obj.is_api_wrapper());
    // SAFETY: the pointer lives on the stack only until it is stored into
    // the wrapper, which then keeps it alive for as long as the JS object is.
    unsafe {
        let probe = v8::cppgc::make_garbage_collected(scope.get_cpp_heap().unwrap(), Probe);
        v8::Object::wrap::<TAG, Probe>(scope, obj, &probe);
    }
    rv.set(obj.into());
}

#[test]
fn cpp_heap_collects_unreachable_wrappers() {
    let (mut data, state) = setup();
    // The context must outlive the GC below: once its last handle is gone the
    // whole realm, `globalThis.kept` included, is garbage too.
    let context = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let f = v8::Function::new(scope, make_probe).unwrap();
        let name = v8::String::new(scope, "makeProbe").unwrap();
        context.global(scope).set(scope, name.into(), f.into());
        let src = v8::String::new(scope, "makeProbe(); globalThis.kept = makeProbe();").unwrap();
        v8::Script::compile(scope, src, None)
            .unwrap()
            .run(scope)
            .unwrap();
        v8::Global::new(scope, context)
    };
    // A GC V8 started on its own (as `test.py fuzz`'s
    // `--cppgc-random-gc-interval` does, during the script) may have
    // collected the unreachable one already; never the kept one.
    assert!(DROPS.load(Ordering::SeqCst) <= 1, "the kept one died");
    // One V8 GC traces JS and the CppHeap together: the unreachable wrapper
    // and its Probe die, the one held by `globalThis.kept` survives.
    data.collect_garbage_for_testing();
    assert_eq!(DROPS.load(Ordering::SeqCst), 1);
    drop(context);
    drop(state);
}

/// A GC type of its own, so the wrap-tag tests don't touch `Probe`'s drop
/// count.
struct Tagged;

// SAFETY: no cppgc pointers, nothing to trace.
unsafe impl v8::cppgc::GarbageCollected for Tagged {
    fn trace(&self, _visitor: &mut v8::cppgc::Visitor) {}
    fn get_name(&self) -> &'static std::ffi::CStr {
        c"Tagged"
    }
}

/// With pointer compression (`v8_enable_pointer_compression`, which machopo
/// builds V8 with), `unwrap` checks the tag `wrap` was given: an object
/// wrapped with one tag unwraps to nothing with another. Without it, V8
/// returns the raw field whatever the tag. (`V8PerIsolateData::new` checks
/// the same at startup.) Our fork's `is_wrapping` sees a wrap whatever the
/// tag.
#[test]
fn unwrap_checks_the_wrap_tag() {
    let (mut data, _state) = setup();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    let template = v8::FunctionTemplate::new(
        scope,
        |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
    );
    let object = template
        .get_function(scope)
        .unwrap()
        .new_instance(scope, &[])
        .unwrap();
    let plain = v8::Object::new(scope);
    assert!(!v8::Object::is_wrapping(scope, plain));
    assert!(!v8::Object::is_wrapping(scope, object));
    // SAFETY: the pointer stays on the stack until the wrapper holds it, and
    // is then only compared with null.
    unsafe {
        let tagged = v8::cppgc::make_garbage_collected(scope.get_cpp_heap().unwrap(), Tagged);
        v8::Object::wrap::<TAG, Tagged>(scope, object, &tagged);
        assert!(v8::Object::unwrap::<TAG, Tagged>(scope, object).is_some());
        assert!(
            v8::Object::unwrap::<{ TAG + 1 }, Tagged>(scope, object).is_none(),
            "unwrap ignored the tag: V8 built without pointer compression?"
        );
    }
    assert!(v8::Object::is_wrapping(scope, object));
}
