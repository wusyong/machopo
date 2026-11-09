//! A realm's table of promises, without the DOM: a promise is kept until
//! it is both handed out and settled, in either order; it settles once;
//! numbers are the realm's own; the table empties when the realm's
//! `PromiseResolvers` goes, after which nothing is waiting.

use bindings::{PromiseResolvers, ScriptState, V8PerIsolateData};

fn setup() -> (V8PerIsolateData, ScriptState) {
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    (data, state)
}

fn state_of(scope: &mut v8::PinScope<'_, '_>, promise: v8::Local<'_, v8::Promise>) -> String {
    match promise.state() {
        v8::PromiseState::Pending => "pending".into(),
        v8::PromiseState::Fulfilled => {
            format!(
                "fulfilled {}",
                promise.result(scope).to_rust_string_lossy(scope)
            )
        }
        v8::PromiseState::Rejected => {
            format!(
                "rejected {}",
                promise.result(scope).to_rust_string_lossy(scope)
            )
        }
    }
}

#[test]
fn a_promise_is_kept_until_handed_out_and_settled() {
    let (mut data, state) = setup();
    let isolate = data.isolate();
    v8::scope_with_context!(let scope, isolate, state.context());
    let table = PromiseResolvers::new(scope);

    // Handed out, then settled.
    let (first, _) = bindings::create_promise(scope).unwrap();
    assert!(bindings::is_waiting(scope, first));
    let promise = bindings::hand_out(scope, first).unwrap();
    assert_eq!(state_of(scope, promise), "pending");
    let resolver = bindings::take_resolver(scope, first).unwrap();
    let one = v8::Number::new(scope, 1.0);
    resolver.resolve(scope, one.into());
    assert_eq!(state_of(scope, promise), "fulfilled 1");
    assert!(!bindings::is_waiting(scope, first));
    assert!(
        bindings::take_resolver(scope, first).is_none(),
        "settled twice"
    );
    assert!(bindings::hand_out(scope, first).is_none(), "kept once done");

    // Settled, then handed out: the same promise, settled.
    let (second, _) = bindings::create_promise(scope).unwrap();
    assert_ne!(second, first);
    let resolver = bindings::take_resolver(scope, second).unwrap();
    let two = v8::Number::new(scope, 2.0);
    resolver.reject(scope, two.into());
    assert!(!bindings::is_waiting(scope, second));
    assert!(
        bindings::take_resolver(scope, second).is_none(),
        "settled twice before it was handed out"
    );
    let promise = bindings::hand_out(scope, second).unwrap();
    assert_eq!(state_of(scope, promise), "rejected 2");
    assert!(
        bindings::hand_out(scope, second).is_none(),
        "kept once done"
    );

    // The realm goes: what waits goes with it.
    let (third, _) = bindings::create_promise(scope).unwrap();
    drop(table);
    assert!(!bindings::is_waiting(scope, third));
    assert!(bindings::take_resolver(scope, third).is_none());
}

/// A context without a table makes no promises.
#[test]
fn a_context_without_a_table_makes_none() {
    let (mut data, state) = setup();
    let isolate = data.isolate();
    v8::scope_with_context!(let scope, isolate, state.context());
    assert!(bindings::create_promise(scope).is_none());
    assert!(!bindings::is_waiting(scope, 1));
}
