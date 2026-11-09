//! WebDriver's script on V8 for the DOM: "execute a function body" and
//! "JSON clone", natively rather than in JS the page could see or change
//! (Chromium's chromedriver injects `call_function.js` into the page, so a
//! page's own `Array.prototype` or `JSON` affect it).
//!
//! The body is an async function's (WebDriver's current text, so `await`
//! works in it), made in the realm and called with the window as `this`.
//! Execute Script settles with what the function's promise does; Execute
//! Async Script with the first call of the function it gets last, its own
//! promise only rejecting it before that (an exception thrown after an
//! `await`), so that a script that keeps the function to call later, as
//! wptrunner's do, isn't settled when its body returns. Either way the
//! result goes into the isolate's state once the promise settles, from the
//! handlers it was given, which run as microtasks; the engine hands it to
//! the host (`ScriptEngine::take_webdriver_results`).

use dom::{DomTokenList, HtmlCollection, Node, NodeKind, NodeList, ObjectRef, Window};
use dom_api::{WebDriverScriptError, WebDriverValue};

use crate::listeners::{have, wrap};
use crate::state::{self, Scope};
use crate::templates;

/// Why a step failed: V8 threw (the `TryCatch` around it has it), or
/// WebDriver's own reason.
enum Failed {
    Threw,
    Because(String),
}

/// What `tc` caught, as WebDriver's error's message.
fn caught(tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>) -> String {
    tc.exception().map_or_else(
        || "script threw".to_owned(),
        |exception| exception.to_rust_string_lossy(tc),
    )
}

fn failure(
    tc: &mut v8::PinnedRef<'_, v8::TryCatch<v8::HandleScope>>,
    failed: Failed,
) -> WebDriverScriptError {
    match failed {
        Failed::Because(message) => WebDriverScriptError(message),
        Failed::Threw => WebDriverScriptError(caught(tc)),
    }
}

/// Starts script `id` in `scope`'s realm: its function made and called,
/// its promise handed the handlers that settle it.
pub(crate) fn execute(
    scope: &mut v8::PinScope<'_, '_>,
    id: u64,
    body: &str,
    args: &[WebDriverValue],
    is_async: bool,
) -> Result<(), WebDriverScriptError> {
    v8::tc_scope!(let tc, scope);
    let started = match state::scope(tc) {
        Some(mut scope) => start(&mut scope, id, body, args, is_async),
        None => Err(Failed::Because("no script can run here".into())),
    };
    started.map_err(|failed| failure(tc, failed))
}

/// Get Element Property: `element`'s property `name` as script reads it,
/// cloned (undefined, as any, null).
pub(crate) fn element_property(
    scope: &mut v8::PinScope<'_, '_>,
    element: dom::NodeId,
    name: &str,
) -> Result<WebDriverValue, WebDriverScriptError> {
    v8::tc_scope!(let tc, scope);
    let got = match state::scope(tc) {
        Some(mut scope) => property(&mut scope, element, name),
        None => Err(Failed::Because("no script can run here".into())),
    };
    got.map_err(|failed| failure(tc, failed))
}

fn property(
    scope: &mut Scope<'_, '_, '_>,
    element: dom::NodeId,
    name: &str,
) -> Result<WebDriverValue, Failed> {
    let wrapper = node_wrapper(scope, element)
        .ok_or_else(|| Failed::Because("the element has gone".into()))?;
    let v8_scope = scope.scope();
    let key = templates::string(v8_scope, name);
    let value = wrapper.get(v8_scope, key.into()).ok_or(Failed::Threw)?;
    clone(scope, value, &mut Vec::new())
}

/// The wrapper of the node `id` names, made if it has none.
fn node_wrapper<'s>(
    scope: &mut Scope<'_, 's, '_>,
    id: dom::NodeId,
) -> Option<v8::Local<'s, v8::Object>> {
    let have = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            cx.node_by_id(id)
                .map(|node| have(&mut no_gc, cx, ObjectRef::Node(node)))
        })?
    };
    Some(wrap(scope, have))
}

/// Makes the function, calls it, and hands its promise the handlers that
/// settle it.
fn start(
    scope: &mut Scope<'_, '_, '_>,
    id: u64,
    body: &str,
    args: &[WebDriverValue],
    is_async: bool,
) -> Result<(), Failed> {
    let mut values = Vec::with_capacity(args.len().saturating_add(1));
    for arg in args {
        values.push(to_js(scope, arg)?);
    }
    let scope = scope.scope();
    // On a line of its own, so a `// comment` ending the body ends there;
    // a `"use strict"` opening it is the function's directive.
    let source = templates::string(scope, &format!("(async function () {{\n{body}\n}})"));
    // Shared cross-origin, as the page's own scripts: what a callback it
    // made throws later isn't muted (V8's origin without one is).
    let name = templates::string(scope, "");
    let origin = v8::ScriptOrigin::new(
        scope,
        name.into(),
        0,
        0,
        true,
        0,
        None,
        false,
        false,
        false,
        None,
    );
    let script = v8::Script::compile(scope, source, Some(&origin)).ok_or(Failed::Threw)?;
    let function = script.run(scope).ok_or(Failed::Threw)?;
    let function = v8::Local::<v8::Function>::try_from(function)
        .map_err(|_| Failed::Because("the body isn't a function body".into()))?;
    let window = scope.get_current_context().global(scope);
    if !is_async {
        let returned = function
            .call(scope, window.into(), &values)
            .ok_or(Failed::Threw)?;
        let promise = v8::Local::<v8::Promise>::try_from(returned)
            .map_err(|_| Failed::Because("an async function returned no promise".into()))?;
        return await_script(scope, id, promise).ok_or(Failed::Threw);
    }
    // The script's id, and whether it has settled: what both its functions
    // carry.
    let state = [
        id_number(scope, id).into(),
        v8::Boolean::new(scope, false).into(),
    ];
    let state = v8::Array::new_with_elements(scope, &state);
    let resolve = v8::Function::builder(resolve_async_script)
        .data(state.into())
        .build(scope)
        .ok_or(Failed::Threw)?;
    values.push(resolve.into());
    let returned = function
        .call(scope, window.into(), &values)
        .ok_or(Failed::Threw)?;
    if let Ok(returned) = v8::Local::<v8::Promise>::try_from(returned) {
        let reject = v8::Function::builder(reject_async_script)
            .data(state.into())
            .build(scope)
            .ok_or(Failed::Threw)?;
        returned.catch(scope, reject).ok_or(Failed::Threw)?;
    }
    Ok(())
}

/// A script's id as a number: ids stay below 2^53, exact as one.
#[expect(clippy::cast_precision_loss, reason = "a request's id, below 2^53")]
fn id_number<'s>(scope: &mut v8::PinScope<'s, '_>, id: u64) -> v8::Local<'s, v8::Number> {
    v8::Number::new(scope, id as f64)
}

/// Hands `promise` the handlers that settle script `id`. `None` if V8
/// threw.
fn await_script(
    scope: &mut v8::PinScope<'_, '_>,
    id: u64,
    promise: v8::Local<'_, v8::Promise>,
) -> Option<()> {
    let id = id_number(scope, id);
    let fulfilled = v8::Function::builder(script_fulfilled)
        .data(id.into())
        .build(scope)?;
    let rejected = v8::Function::builder(script_rejected)
        .data(id.into())
        .build(scope)?;
    promise.then2(scope, fulfilled, rejected)?;
    Some(())
}

/// WebDriver's "JSON deserialize", past the references (the renderer has
/// found their nodes): `value` as JS.
fn to_js<'s>(
    scope: &mut Scope<'_, 's, '_>,
    value: &WebDriverValue,
) -> Result<v8::Local<'s, v8::Value>, Failed> {
    Ok(match value {
        WebDriverValue::Null => v8::null(scope.scope()).into(),
        WebDriverValue::Bool(b) => v8::Boolean::new(scope.scope(), *b).into(),
        WebDriverValue::Number(n) => v8::Number::new(scope.scope(), *n).into(),
        WebDriverValue::String(s) => templates::string(scope.scope(), s).into(),
        WebDriverValue::Array(items) => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(to_js(scope, item)?);
            }
            v8::Array::new_with_elements(scope.scope(), &values).into()
        }
        WebDriverValue::Object(properties) => {
            let object = v8::Object::new(scope.scope());
            for (name, value) in properties {
                let value = to_js(scope, value)?;
                let v8_scope = scope.scope();
                let key = templates::string(v8_scope, name);
                object
                    .create_data_property(v8_scope, key.into(), value)
                    .ok_or(Failed::Threw)?;
            }
            object.into()
        }
        WebDriverValue::Element(id) => node_wrapper(scope, *id)
            .ok_or_else(|| Failed::Because("an argument's element has gone".into()))?
            .into(),
        // The window of a frame showing that document: its global proxy
        // (a child frame's for a frame's reference).
        WebDriverValue::Window(document) => {
            let context = {
                let (state, _) = scope.state();
                state
                    .frames
                    .of_document(*document)
                    .map(|frame| frame.context.clone())
            }
            .ok_or_else(|| Failed::Because("an argument's window has gone".into()))?;
            let v8_scope = scope.scope();
            v8::Local::new(v8_scope, context).global(v8_scope).into()
        }
    })
}

/// What a JS object is of the DOM's, for "JSON clone": an element, a
/// window, a collection (cloned as an array), or nothing of those.
enum Kind {
    Element(dom::NodeId),
    Window(dom::NodeId),
    Collection,
    Other,
}

fn kind_of(scope: &mut Scope<'_, '_, '_>, object: v8::Local<'_, v8::Object>) -> Kind {
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        if let Some(node) = no_gc.unwrap::<Node>(cx.gc, object) {
            return if cx.kind(node) == NodeKind::Element {
                Kind::Element(cx.node_id(node))
            } else {
                Kind::Other
            };
        }
        if let Some(window) = no_gc.unwrap::<Window>(cx.gc, object) {
            return Kind::Window(cx.node_id(cx.window_document(window)));
        }
        if no_gc.unwrap::<HtmlCollection>(cx.gc, object).is_some()
            || no_gc.unwrap::<NodeList>(cx.gc, object).is_some()
            || no_gc.unwrap::<DomTokenList>(cx.gc, object).is_some()
        {
            return Kind::Collection;
        }
        Kind::Other
    })
}

/// WebDriver's "JSON clone" of `value`, `seen` holding the objects it is
/// within (a cycle back to one of them is an error; the same object twice
/// side by side isn't).
fn clone<'s>(
    scope: &mut Scope<'_, 's, '_>,
    value: v8::Local<'s, v8::Value>,
    seen: &mut Vec<v8::Local<'s, v8::Object>>,
) -> Result<WebDriverValue, Failed> {
    if value.is_null_or_undefined() {
        return Ok(WebDriverValue::Null);
    }
    if value.is_boolean() {
        return Ok(WebDriverValue::Bool(value.boolean_value(scope.scope())));
    }
    if let Ok(number) = v8::Local::<v8::Number>::try_from(value) {
        return Ok(WebDriverValue::Number(number.value()));
    }
    if let Ok(string) = v8::Local::<v8::String>::try_from(value) {
        return Ok(WebDriverValue::String(
            string.to_rust_string_lossy(scope.scope()),
        ));
    }
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        // A symbol or a BigInt.
        let v8_scope = scope.scope();
        let kind = value.type_of(v8_scope).to_rust_string_lossy(v8_scope);
        return Err(Failed::Because(format!("a {kind} has no JSON")));
    };
    let kind = kind_of(scope, object);
    match kind {
        Kind::Element(id) => return Ok(WebDriverValue::Element(id)),
        Kind::Window(document) => return Ok(WebDriverValue::Window(document)),
        Kind::Collection | Kind::Other => {}
    }
    // An own `toJSON` that can be called: what it returns.
    let v8_scope = scope.scope();
    let to_json = templates::string(v8_scope, "toJSON");
    if object
        .has_own_property(v8_scope, to_json.into())
        .ok_or(Failed::Threw)?
    {
        let method = object.get(v8_scope, to_json.into()).ok_or(Failed::Threw)?;
        if let Ok(method) = v8::Local::<v8::Function>::try_from(method) {
            let result = method
                .call(v8_scope, object.into(), &[])
                .ok_or(Failed::Threw)?;
            return clone(scope, result, seen);
        }
    }
    if seen
        .iter()
        .any(|within| within.strict_equals(object.into()))
    {
        return Err(Failed::Because("the value is cyclic".into()));
    }
    seen.push(object);
    let collection =
        value.is_array() || value.is_arguments_object() || matches!(kind, Kind::Collection);
    let cloned = if collection {
        clone_items(scope, object, seen).map(WebDriverValue::Array)
    } else {
        clone_properties(scope, object, seen).map(WebDriverValue::Object)
    };
    seen.pop();
    cloned
}

/// Each of a collection's items, up to its `length`.
fn clone_items<'s>(
    scope: &mut Scope<'_, 's, '_>,
    object: v8::Local<'s, v8::Object>,
    seen: &mut Vec<v8::Local<'s, v8::Object>>,
) -> Result<Vec<WebDriverValue>, Failed> {
    let v8_scope = scope.scope();
    let length = templates::string(v8_scope, "length");
    let length = object.get(v8_scope, length.into()).ok_or(Failed::Threw)?;
    let length = length.uint32_value(v8_scope).ok_or(Failed::Threw)?;
    let mut items = Vec::new();
    for index in 0..length {
        let item = object
            .get_index(scope.scope(), index)
            .ok_or(Failed::Threw)?;
        items.push(clone(scope, item, seen)?);
    }
    Ok(items)
}

/// Each of an object's own enumerable properties named by a string.
fn clone_properties<'s>(
    scope: &mut Scope<'_, 's, '_>,
    object: v8::Local<'s, v8::Object>,
    seen: &mut Vec<v8::Local<'s, v8::Object>>,
) -> Result<Vec<(String, WebDriverValue)>, Failed> {
    let v8_scope = scope.scope();
    let keys = object
        .get_own_property_names(
            v8_scope,
            v8::GetPropertyNamesArgs {
                mode: v8::KeyCollectionMode::OwnOnly,
                property_filter: v8::PropertyFilter::ONLY_ENUMERABLE
                    | v8::PropertyFilter::SKIP_SYMBOLS,
                index_filter: v8::IndexFilter::IncludeIndices,
                key_conversion: v8::KeyConversionMode::ConvertToString,
            },
        )
        .ok_or(Failed::Threw)?;
    let mut properties = Vec::new();
    for index in 0..keys.length() {
        let v8_scope = scope.scope();
        let key = keys.get_index(v8_scope, index).ok_or(Failed::Threw)?;
        let name = key.to_rust_string_lossy(v8_scope);
        let value = object.get(v8_scope, key).ok_or(Failed::Threw)?;
        properties.push((name, clone(scope, value, seen)?));
    }
    Ok(properties)
}

/// Script `id` has settled: its result kept for the host.
fn settled(
    scope: &mut v8::PinScope<'_, '_>,
    id: u64,
    result: Result<WebDriverValue, WebDriverScriptError>,
) {
    if let Some(mut scope) = state::scope(scope) {
        scope.state_mut().0.webdriver_results.push((id, result));
    }
}

/// The id a handler carries.
fn script_id(args: &v8::FunctionCallbackArguments) -> u64 {
    // Ours, made from a `u64`.
    base::numerics::saturated_cast(
        v8::Local::<v8::Number>::try_from(args.data())
            .expect("a script's handler carries its id")
            .value(),
    )
}

/// The script's promise fulfilled: its value cloned.
fn script_fulfilled(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let id = script_id(&args);
    v8::tc_scope!(let tc, scope);
    let value = v8::Local::new(tc, args.get(0));
    let cloned = match state::scope(tc) {
        Some(mut scope) => clone(&mut scope, value, &mut Vec::new()),
        None => return,
    };
    let result = cloned.map_err(|failed| failure(tc, failed));
    settled(tc, id, result);
}

/// The script's promise rejected: WebDriver's "javascript error", with
/// the reason as a string.
fn script_rejected(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let id = script_id(&args);
    let message = args.get(0).to_rust_string_lossy(scope);
    settled(scope, id, Err(WebDriverScriptError(message)));
}

/// An async script's id from the state its functions carry, the first
/// time either is called: then it is settled.
fn settle_once(
    scope: &mut v8::PinScope<'_, '_>,
    args: &v8::FunctionCallbackArguments,
) -> Option<u64> {
    let state = v8::Local::<v8::Array>::try_from(args.data())
        .expect("an async script's function carries its state");
    let id = base::numerics::saturated_cast(state.get_index(scope, 0)?.number_value(scope)?);
    if state.get_index(scope, 1)?.boolean_value(scope) {
        return None;
    }
    let settled = v8::Boolean::new(scope, true);
    state.set_index(scope, 1, settled.into())?;
    Some(id)
}

/// The function an async script gets last: settles it, the first time,
/// with its argument, once that settles too (it may be a promise).
fn resolve_async_script(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let Some(id) = settle_once(scope, &args) else {
        return;
    };
    let Some(resolver) = v8::PromiseResolver::new(scope) else {
        return;
    };
    resolver.resolve(scope, args.get(0));
    let promise = resolver.get_promise(scope);
    let _ = await_script(scope, id, promise);
}

/// An async script's body rejected before the script was settled: that
/// is its error.
fn reject_async_script(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let Some(id) = settle_once(scope, &args) else {
        return;
    };
    let message = args.get(0).to_rust_string_lossy(scope);
    settled(scope, id, Err(WebDriverScriptError(message)));
}
