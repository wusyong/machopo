//! WebDriver's script on the DOM's engine: a function body run with
//! the window as `this` (sync, awaiting, async with the function to call
//! last), its arguments made JS, its result JSON cloned (elements and the
//! window as ids, collections as arrays, an own `toJSON`, cycles and what
//! has no JSON refused), what it throws its error; an element's property.

use std::time::Instant;

use dom::NodeId;
use dom_api::{ClassicScript, EngineSettings, ScriptEngine, WebDriverScriptError, WebDriverValue};
use dom_js::V8Engine;

use WebDriverValue::{Array, Bool, Null, Number, Object};

fn run(engine: &mut V8Engine, source: &str) {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source,
            url: "https://example.test/page.html",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
    engine.perform_microtask_checkpoint();
}

/// An engine with a window whose document has an `<html id=x>`: the
/// engine, the document's id and the element's.
fn page() -> (V8Engine, NodeId, NodeId) {
    let mut engine = V8Engine::new(EngineSettings { web_tests: false });
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "const h = document.createElement('html'); h.id = 'x'; document.appendChild(h)",
    );
    let (document, html) = engine.dom_mut().read(|cx| {
        let document = document.get(cx.gc);
        (
            cx.node_id(document),
            cx.node_id(cx.first_child(document).expect("the html")),
        )
    });
    (engine, document, html)
}

/// What script `id` settled with, now.
fn result(engine: &mut V8Engine, id: u64) -> Option<Result<WebDriverValue, WebDriverScriptError>> {
    let mut results = engine.take_webdriver_results();
    assert!(results.len() <= 1, "one script at a time: {results:?}");
    results.pop().map(|(got, result)| {
        assert_eq!(got, id);
        result
    })
}

fn execute(
    engine: &mut V8Engine,
    body: &str,
    args: &[WebDriverValue],
) -> Result<WebDriverValue, WebDriverScriptError> {
    engine.execute_webdriver_script(engine.top_frame(), 1, body, args, false)?;
    result(engine, 1).expect("settled")
}

fn execute_async(
    engine: &mut V8Engine,
    body: &str,
) -> Option<Result<WebDriverValue, WebDriverScriptError>> {
    let top = engine.top_frame();
    engine
        .execute_webdriver_script(top, 2, body, &[], true)
        .expect("started");
    result(engine, 2)
}

fn string(text: &str) -> WebDriverValue {
    WebDriverValue::String(text.to_owned())
}

fn message(result: Result<WebDriverValue, WebDriverScriptError>) -> String {
    result.expect_err("an error").0
}

#[test]
fn a_scripts_value_is_cloned() {
    let (mut engine, _, _) = page();
    assert_eq!(execute(&mut engine, "return 1 + 2", &[]), Ok(Number(3.0)));
    assert_eq!(
        execute(&mut engine, "return 'a' // a comment ends the line", &[]),
        Ok(string("a"))
    );
    assert_eq!(execute(&mut engine, "return true", &[]), Ok(Bool(true)));
    assert_eq!(execute(&mut engine, "", &[]), Ok(Null), "undefined");
    assert_eq!(
        execute(&mut engine, "return this === window", &[]),
        Ok(Bool(true))
    );
    assert_eq!(
        execute(&mut engine, "'use strict'; return this === window", &[]),
        Ok(Bool(true)),
        "given as this, not undefined"
    );
    assert_eq!(
        execute(&mut engine, "return await Promise.resolve(9)", &[]),
        Ok(Number(9.0)),
        "an async function's body"
    );
    assert_eq!(
        execute(
            &mut engine,
            "return { b: 1, a: [null, 'x'], [Symbol()]: 2 }",
            &[]
        ),
        Ok(Object(vec![
            ("b".to_owned(), Number(1.0)),
            ("a".to_owned(), Array(vec![Null, string("x")])),
        ])),
        "own enumerable properties named by strings, in order"
    );
    assert_eq!(
        execute(
            &mut engine,
            "return Object.defineProperty({}, 'hidden', { value: 1 })",
            &[]
        ),
        Ok(Object(vec![]))
    );
    // An own `toJSON` alone is called.
    assert_eq!(
        execute(&mut engine, "return { toJSON() { return 5 } }", &[]),
        Ok(Number(5.0))
    );
    assert_eq!(
        execute(
            &mut engine,
            "return Object.create({ toJSON() { return 5 } })",
            &[]
        ),
        Ok(Object(vec![]))
    );
    assert_eq!(
        execute(
            &mut engine,
            "return arguments",
            &[Number(1.0), string("two")]
        ),
        Ok(Array(vec![Number(1.0), string("two")])),
        "an arguments object is an array"
    );
    // The same object twice side by side; one within itself.
    assert_eq!(
        execute(&mut engine, "const o = {}; return [o, o]", &[]),
        Ok(Array(vec![Object(vec![]), Object(vec![])]))
    );
    assert!(
        message(execute(
            &mut engine,
            "const a = []; a.push(a); return a",
            &[]
        ))
        .contains("cyclic")
    );
    assert!(
        message(execute(
            &mut engine,
            "const o = {}; o.o = { o }; return o",
            &[]
        ))
        .contains("cyclic")
    );
    assert_eq!(
        message(execute(&mut engine, "return Symbol()", &[])),
        "a symbol has no JSON"
    );
    assert_eq!(
        message(execute(&mut engine, "return 1n", &[])),
        "a bigint has no JSON"
    );
}

#[test]
fn elements_and_the_window_go_out_and_come_in() {
    let (mut engine, document, html) = page();
    assert_eq!(
        execute(&mut engine, "return document.documentElement", &[]),
        Ok(WebDriverValue::Element(html))
    );
    assert_eq!(
        execute(&mut engine, "return window", &[]),
        Ok(WebDriverValue::Window(document))
    );
    // A node that isn't an element is an object: its own enumerable
    // properties, a document's unforgeable `location` (as Chrome's).
    let location = Object(vec![
        ("href".to_owned(), string("https://example.test/page.html")),
        ("origin".to_owned(), string("https://example.test")),
        ("protocol".to_owned(), string("https:")),
        ("host".to_owned(), string("example.test")),
        ("hostname".to_owned(), string("example.test")),
        ("port".to_owned(), string("")),
        ("pathname".to_owned(), string("/page.html")),
        ("search".to_owned(), string("")),
        ("hash".to_owned(), string("")),
        ("assign".to_owned(), Object(vec![])),
        ("replace".to_owned(), Object(vec![])),
        ("reload".to_owned(), Object(vec![])),
        ("toString".to_owned(), Object(vec![])),
    ]);
    assert_eq!(
        execute(&mut engine, "return document", &[]),
        Ok(Object(vec![("location".to_owned(), location)])),
    );
    // Collections are arrays.
    assert_eq!(
        execute(
            &mut engine,
            "return document.getElementsByTagName('html')",
            &[]
        ),
        Ok(Array(vec![WebDriverValue::Element(html)]))
    );
    assert_eq!(
        execute(&mut engine, "return document.querySelectorAll('html')", &[]),
        Ok(Array(vec![WebDriverValue::Element(html)]))
    );
    assert_eq!(
        execute(
            &mut engine,
            "const h = document.documentElement; h.className = 'a b'; return h.classList",
            &[]
        ),
        Ok(Array(vec![string("a"), string("b")]))
    );
    assert_eq!(
        execute(
            &mut engine,
            "return arguments[0].id + (arguments[1] === window)",
            &[
                WebDriverValue::Element(html),
                WebDriverValue::Window(document),
            ]
        ),
        Ok(string("xtrue"))
    );
    assert_eq!(
        execute(
            &mut engine,
            "return arguments[0]",
            &[Object(vec![(
                "e".to_owned(),
                WebDriverValue::Element(html)
            )])]
        ),
        Ok(Object(vec![(
            "e".to_owned(),
            WebDriverValue::Element(html)
        )]))
    );
    // An element's property as script reads it; null for undefined.
    assert_eq!(
        engine.webdriver_element_property(engine.top_frame(), html, "id"),
        Ok(string("x"))
    );
    assert_eq!(
        engine.webdriver_element_property(engine.top_frame(), html, "nope"),
        Ok(Null)
    );
    assert_eq!(
        engine.webdriver_element_property(engine.top_frame(), html, "getElementsByTagName"),
        Ok(Object(vec![])),
        "a function is an object"
    );
    assert_eq!(
        engine.webdriver_element_property(engine.top_frame(), html, "tagName"),
        Ok(string("HTML"))
    );
}

#[test]
fn what_a_script_throws_is_its_error() {
    let (mut engine, _, _) = page();
    assert_eq!(
        message(execute(&mut engine, "throw new Error('boom')", &[])),
        "Error: boom"
    );
    assert_eq!(
        message(execute(
            &mut engine,
            "await null; throw new TypeError('later')",
            &[]
        )),
        "TypeError: later"
    );
    assert!(
        engine
            .execute_webdriver_script(engine.top_frame(), 1, "(", &[], false)
            .is_err(),
        "a body that doesn't parse fails at once"
    );
    assert!(engine.take_webdriver_results().is_empty());
}

#[test]
fn an_async_script_settles_with_the_first_call() {
    let (mut engine, _, _) = page();
    assert_eq!(
        execute_async(&mut engine, "arguments[0](7)"),
        Some(Ok(Number(7.0)))
    );
    assert_eq!(
        execute_async(&mut engine, "arguments[0](1); arguments[0](2)"),
        Some(Ok(Number(1.0)))
    );
    assert_eq!(
        execute_async(&mut engine, "arguments[0](Promise.resolve(3))"),
        Some(Ok(Number(3.0))),
        "a promise it is given settles first"
    );
    assert_eq!(
        execute_async(&mut engine, "arguments[0](1); throw new Error('after')"),
        Some(Ok(Number(1.0))),
        "settled before it threw"
    );
    assert_eq!(
        execute_async(&mut engine, "await null; throw new Error('before')").map(message),
        Some("Error: before".to_owned())
    );
    assert_eq!(
        execute_async(&mut engine, "return Promise.reject(new Error('no'))").map(message),
        Some("Error: no".to_owned())
    );
    // The function kept to call later: not settled when the body returns.
    assert_eq!(
        execute_async(&mut engine, "window.later = arguments[0]; return 1"),
        None
    );
    run(&mut engine, "later(4)");
    assert_eq!(result(&mut engine, 2), Some(Ok(Number(4.0))));
}

/// A child frame's script and properties run in its realm (its
/// `FrameId`'s); a child frame's window is its document's window as a
/// value, and given back as an argument it is that frame's `WindowProxy`
/// (another origin's too); a frame that has gone has no realm.
#[test]
fn a_child_frames_script_runs_in_its_realm() {
    let (mut engine, top_document, _) = page();
    run(
        &mut engine,
        "window.f = document.documentElement.appendChild(document.createElement('iframe'));
         window.g = document.documentElement.appendChild(document.createElement('iframe'));",
    );
    let frames: Vec<_> = engine
        .take_frame_requests()
        .into_iter()
        .filter_map(|request| match request {
            dom_api::FrameRequest::Created { frame, .. } => Some(frame),
            _ => None,
        })
        .collect();
    let [child, other] = frames[..] else {
        panic!("{frames:?}");
    };
    let documents = [
        engine.create_window(child, "https://example.test/child.html", Instant::now()),
        engine.create_window(other, "https://other.test/", Instant::now()),
    ]
    .map(|document| document.expect("its frame"));
    let [child_document, other_document] = documents
        .each_ref()
        .map(|document| engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc))));
    engine
        .execute_webdriver_script(
            child,
            1,
            "return [document.URL, parent === top, window]",
            &[],
            false,
        )
        .expect("started");
    assert_eq!(
        result(&mut engine, 1).expect("settled"),
        Ok(Array(vec![
            string("https://example.test/child.html"),
            Bool(true),
            WebDriverValue::Window(child_document),
        ]))
    );
    // From the top-level frame: its child windows, and given back.
    assert_eq!(
        execute(&mut engine, "return [window, frames[0], frames[1]]", &[]),
        Ok(Array(vec![
            WebDriverValue::Window(top_document),
            WebDriverValue::Window(child_document),
            WebDriverValue::Window(other_document),
        ]))
    );
    assert_eq!(
        execute(
            &mut engine,
            "return [arguments[0] === frames[0], arguments[1] === frames[1], arguments[2] === window]",
            &[
                WebDriverValue::Window(child_document),
                WebDriverValue::Window(other_document),
                WebDriverValue::Window(top_document),
            ],
        ),
        Ok(Array(vec![Bool(true), Bool(true), Bool(true)]))
    );
    engine.run_classic_script(
        child,
        &ClassicScript {
            source: "document.appendChild(document.createElement('html')).marker = location.href",
            url: "https://example.test/child.html",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
    let html = engine
        .dom_mut()
        .read(|cx| cx.node_id(cx.first_child(documents[0].get(cx.gc)).expect("the html")));
    assert_eq!(
        engine.webdriver_element_property(child, html, "marker"),
        Ok(string("https://example.test/child.html"))
    );
    // Gone: its iframe removed.
    run(&mut engine, "f.remove()");
    assert!(
        engine
            .execute_webdriver_script(child, 2, "return 1", &[], false)
            .is_err()
    );
    assert!(
        engine
            .webdriver_element_property(child, html, "id")
            .is_err()
    );
}
