//! Script stopped from another thread (`TerminateExecution`) while the
//! embedder calls it back (`bindings::stop_again`): stopped again when the
//! embedder caught the stop, it stops the script under the callback though
//! the embedder's next call into V8 clears the stop, and the next outermost
//! script runs.

use std::sync::mpsc;
use std::time::Duration;

use bindings::{ScriptState, V8PerIsolateData};

/// `callBack(first, then)`: calls `first`; caught stopped, asks for the
/// stop again; then calls `then`, which clears what V8 has pending first
/// (V8's `PrepareForExecutionScope`), as a member's next call into V8
/// after a listener would.
fn call_back(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let (Ok(first), Ok(then)) = (
        v8::Local::<v8::Function>::try_from(args.get(0)),
        v8::Local::<v8::Function>::try_from(args.get(1)),
    ) else {
        return;
    };
    let undefined = v8::undefined(scope).into();
    {
        v8::tc_scope!(let tc, scope);
        if first.call(tc, undefined, &[]).is_none() && tc.has_terminated() {
            bindings::stop_again(tc);
        }
    }
    let _ = then.call(scope, undefined, &[]);
}

/// A realm with `callBack()`.
fn setup() -> (V8PerIsolateData, ScriptState) {
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    {
        let isolate = data.isolate();
        v8::scope_with_context!(let scope, isolate, state.context());
        let context = scope.get_current_context();
        let function = v8::Function::new(scope, call_back).unwrap();
        let name = v8::String::new(scope, "callBack").unwrap();
        context
            .global(scope)
            .set(scope, name.into(), function.into());
    }
    (data, state)
}

/// The callback stopped 30 ms in, from this thread: the script under it
/// stops at once (nothing after `callBack` runs, though `then` cleared the
/// stop), and the next script runs.
#[test]
fn a_stop_asked_for_again_stops_the_script_under_the_callback() {
    let (handles, handle) = mpsc::channel();
    let (results, result) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (mut data, state) = setup();
        handles.send(data.isolate().thread_safe_handle()).unwrap();
        let stopped = state.evaluate_classic_script(
            &mut data,
            "globalThis.after = 0; callBack(() => { for (;;); }, () => {}); globalThis.after = 1; for (;;);",
        );
        let next = state.evaluate_classic_script(&mut data, "String(globalThis.after)");
        results.send((stopped.is_err(), next.ok())).unwrap();
    });
    let handle = handle.recv().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    handle.terminate_execution();
    let (stopped, next) = result
        .recv_timeout(Duration::from_secs(10))
        .expect("the script under the callback stops");
    thread.join().unwrap();
    assert!(stopped);
    assert_eq!(next.as_deref(), Some("0"), "nothing after `callBack` ran");
}

/// Asked to stop again with no script on the stack (a task's callback,
/// caught stopped): the next outermost script runs, its entry ending the
/// stop first.
#[test]
fn a_stop_asked_for_again_ends_before_the_next_script() {
    let (mut data, state) = setup();
    bindings::stop_again(data.isolate());
    assert_eq!(
        state
            .evaluate_classic_script(&mut data, "1 + 1")
            .ok()
            .as_deref(),
        Some("2")
    );
}
