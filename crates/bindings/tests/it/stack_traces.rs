//! The stack as script runs, as V8's inspector captures it for DevTools
//! (`devtools::capture_stack`): a request's initiator, with the calls that
//! led to it across `await`s.

use std::cell::RefCell;

use bindings::{ScriptState, V8PerIsolateData};
use serde_json::{Value, json};

use crate::devtools_session::Session;

thread_local! {
    /// What each `capture()` call saw.
    static CAPTURED: RefCell<Vec<Option<Value>>> = const { RefCell::new(Vec::new()) };
}

/// `capture()`: the stack, with every async parent.
fn capture(scope: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue) {
    let stack = bindings::devtools::capture_stack(scope, i32::MAX)
        .map(|json| serde_json::from_str(&json).unwrap());
    CAPTURED.with_borrow_mut(|captured| captured.push(stack));
}

/// A realm with `capture()`, the inspector told of it if `told`.
fn setup(told: bool) -> (V8PerIsolateData, ScriptState) {
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    {
        let isolate = data.isolate();
        v8::scope_with_context!(let scope, isolate, state.context());
        let context = scope.get_current_context();
        let function = v8::Function::new(scope, capture).unwrap();
        let name = v8::String::new(scope, "capture").unwrap();
        context
            .global(scope)
            .set(scope, name.into(), function.into());
        if told {
            bindings::devtools::context_created(scope, context, "https://example.test", None, true);
        }
    }
    CAPTURED.with_borrow_mut(Vec::clear);
    (data, state)
}

fn captured() -> Vec<Option<Value>> {
    CAPTURED.with_borrow_mut(std::mem::take)
}

fn function_names(stack: &Value) -> Vec<&str> {
    stack["callFrames"]
        .as_array()
        .unwrap()
        .iter()
        .map(|frame| frame["functionName"].as_str().unwrap())
        .collect()
}

const NESTED: &str = "function inner() { capture(); } function outer() { inner(); } outer();";

/// Without a session that enabled `Runtime`, the inspector keeps one frame
/// (V8's `V8Debugger::captureStackTrace`); with one, the whole stack, each
/// frame with its script's URL and 0-based position.
#[test]
fn the_stack_of_a_call() {
    let (mut data, state) = setup(true);
    state.evaluate_classic_script(&mut data, NESTED).unwrap();
    let [Some(top)] = &captured()[..] else {
        panic!("one stack");
    };
    assert_eq!(function_names(top), ["inner"]);

    let mut session = Session::attach(&mut data);
    session.call(&mut data, "Runtime.enable", json!({}));
    state
        .evaluate_classic_script_from(
            &mut data,
            NESTED,
            Some(bindings::ScriptLocation {
                url: "https://example.test/nested.js",
                line: 1,
                base_url: None,
            }),
        )
        .unwrap();
    let [Some(stack)] = &captured()[..] else {
        panic!("one stack");
    };
    assert_eq!(function_names(stack), ["inner", "outer", ""]);
    let inner = &stack["callFrames"][0];
    assert_eq!(inner["url"], "https://example.test/nested.js");
    assert_eq!(inner["lineNumber"], 0);
    assert_eq!(inner["columnNumber"], 19);
    assert!(stack.get("parent").is_none(), "{stack}");
}

/// After an `await`, the stack goes on in its async parent: the calls that
/// started the async function, as DevTools shows a `fetch()`'s initiator.
/// Only while a session has `Debugger` record async stacks
/// (`Debugger.setAsyncCallStackDepth`), as DevTools does.
#[test]
fn the_stack_across_an_await() {
    let (mut data, state) = setup(true);
    let mut session = Session::attach(&mut data);
    session.call(&mut data, "Runtime.enable", json!({}));
    session.call(&mut data, "Debugger.enable", json!({}));
    session.call(
        &mut data,
        "Debugger.setAsyncCallStackDepth",
        json!({ "maxDepth": 32 }),
    );
    state
        .evaluate_classic_script(
            &mut data,
            "async function load() { await null; capture(); } function start() { load(); } start();",
        )
        .unwrap();
    data.perform_microtask_checkpoint();
    let [Some(stack)] = &captured()[..] else {
        panic!("one stack");
    };
    assert_eq!(function_names(stack), ["load"]);
    let parent = &stack["parent"];
    assert_eq!(parent["description"], "await", "{stack}");
    assert_eq!(function_names(parent), ["start", ""]);

    // A reaction that is `capture` itself has no frames of its own, only
    // the async parent: no stack, as Blink takes an empty one
    // (`CaptureSourceLocation`).
    state
        .evaluate_classic_script(&mut data, "Promise.resolve().then(capture);")
        .unwrap();
    data.perform_microtask_checkpoint();
    assert_eq!(captured(), [None]);
}

/// No stack outside script, in a realm or not, or in a realm the
/// inspector wasn't told of.
#[test]
fn no_stack_without_script() {
    let (mut data, state) = setup(false);
    state
        .evaluate_classic_script(&mut data, "capture();")
        .unwrap();
    assert_eq!(captured(), [None]);

    let (mut data, state) = setup(true);
    let isolate = data.isolate();
    {
        v8::scope!(let scope, isolate);
        assert_eq!(bindings::devtools::capture_stack(scope, i32::MAX), None);
    }
    // In the realm, but with no script running (the renderer's steps after
    // a parser's): the inspector's stack is there, and empty.
    v8::scope_with_context!(let scope, isolate, state.context());
    assert_eq!(bindings::devtools::capture_stack(scope, i32::MAX), None);
}
