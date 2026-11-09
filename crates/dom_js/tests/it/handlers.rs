//! HTML's event handlers on the DOM's engine: the IDL attributes (a
//! function, null, what isn't an object; their place among the target's
//! listeners), the content attributes compiled when first needed (their
//! scopes, their `event`, a syntax error reported and the handler null),
//! the return value cancelling, a `<body>`'s for its window, a window's
//! `onerror` with an error's parts, and the parser's line.

use std::time::Instant;

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

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

fn report(engine: &mut V8Engine, expression: &str) -> String {
    run(
        engine,
        &format!("testRunner.reportHarnessResults(String({expression}))"),
    );
    engine.take_harness_results().unwrap_or_default()
}

/// An engine whose window's document has `<html><body><div id=d>`.
fn page() -> V8Engine {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "const html = document.createElement('html'); document.appendChild(html); \
         const body = document.createElement('body'); html.appendChild(body); \
         const d = document.createElement('div'); d.id = 'd'; body.appendChild(d); var log = [];",
    );
    engine
}

#[test]
fn an_idl_attribute_sets_a_handler_in_its_place() {
    let mut engine = page();
    run(
        &mut engine,
        "d.addEventListener('click', () => log.push('a')); \
         d.onclick = function (e) { log.push('h ' + (this === d) + ' ' + (e.type)); return false; }; \
         d.addEventListener('click', () => log.push('b'));",
    );
    assert_eq!(report(&mut engine, "typeof d.onclick"), "function");
    assert_eq!(
        report(&mut engine, "typeof document.onclick"),
        "object",
        "null"
    );
    let click = "new Event('click', { cancelable: true })";
    assert_eq!(
        report(
            &mut engine,
            &format!("[d.dispatchEvent({click}), log.join()]")
        ),
        "false,a,h true click,b"
    );
    // Set again: in the same place; null takes it out, set after that it
    // comes last.
    run(&mut engine, "log = []; d.onclick = () => log.push('h2');");
    assert_eq!(
        report(
            &mut engine,
            &format!("[d.dispatchEvent({click}), log.join()]")
        ),
        "true,a,h2,b"
    );
    run(
        &mut engine,
        "log = []; d.onclick = null; d.dispatchEvent(new Event('click'));",
    );
    assert_eq!(report(&mut engine, "[d.onclick, log.join()]"), ",a,b");
    run(
        &mut engine,
        "log = []; d.onclick = () => log.push('h3'); d.dispatchEvent(new Event('click'));",
    );
    assert_eq!(report(&mut engine, "log.join()"), "a,b,h3");
    // What isn't an object is null; an object that isn't a function is
    // kept and never called.
    run(&mut engine, "d.onclick = 'log.push(1)';");
    assert_eq!(report(&mut engine, "d.onclick"), "null");
    run(
        &mut engine,
        "log = []; const o = {}; d.onclick = o; d.dispatchEvent(new Event('click'));",
    );
    assert_eq!(
        report(&mut engine, "[d.onclick === o, log.join()]"),
        "true,a,b"
    );
    // A handler's function is no listener's: added as one too, it is
    // called twice; removed as one, the handler stays.
    run(
        &mut engine,
        "log = []; const f = () => log.push('f'); d.onclick = f; d.addEventListener('click', f); d.dispatchEvent(new Event('click'));",
    );
    assert_eq!(report(&mut engine, "log.join()"), "a,b,f,f");
    run(
        &mut engine,
        "log = []; d.removeEventListener('click', f); d.dispatchEvent(new Event('click'));",
    );
    assert_eq!(
        report(&mut engine, "[d.onclick === f, log.join()]"),
        "true,a,b,f"
    );
    // A window's and a document's are their own.
    run(
        &mut engine,
        "log = []; window.onfoo = 1; document.onclick = () => log.push('doc'); window.onclick = () => log.push('win');",
    );
    run(
        &mut engine,
        "d.onclick = null; d.dispatchEvent(new Event('click', { bubbles: true }));",
    );
    assert_eq!(report(&mut engine, "log.join()"), "a,b,doc,win");
}

#[test]
fn a_content_attribute_is_compiled_when_first_needed() {
    let mut engine = page();
    run(
        &mut engine,
        "d.setAttribute('onclick', 'log.push(id + \" \" + typeof event + \" \" + (this === d) + \" \" + (typeof createElement)); return false');",
    );
    // The element's and its document's names are in scope.
    let click = "new Event('click', { cancelable: true })";
    assert_eq!(
        report(
            &mut engine,
            &format!("[d.dispatchEvent({click}), log.join()]")
        ),
        "false,d object true function"
    );
    assert_eq!(report(&mut engine, "typeof d.onclick"), "function");
    // Changed: compiled again; removed: none.
    run(
        &mut engine,
        "log = []; d.setAttribute('onclick', 'log.push(2)'); d.dispatchEvent(new Event('click'));",
    );
    assert_eq!(report(&mut engine, "log.join()"), "2");
    run(&mut engine, "d.removeAttribute('onclick');");
    assert_eq!(report(&mut engine, "d.onclick"), "null");
    // A source that doesn't compile: reported at the window, null.
    run(
        &mut engine,
        "log = []; addEventListener('error', e => log.push(e.message.split(':')[0])); d.setAttribute('onclick', '(');",
    );
    assert_eq!(
        report(&mut engine, "[d.onclick, log.join()]"),
        ",Uncaught SyntaxError"
    );
    // An attribute of another namespace, or one that isn't a handler's,
    // sets none.
    run(
        &mut engine,
        "d.setAttributeNS('urn:x', 'onclick', 'log.push(3)'); d.setAttribute('onnothing', 'x');",
    );
    assert_eq!(report(&mut engine, "d.onclick"), "null");
}

#[test]
fn a_bodys_window_handlers_are_its_windows() {
    let mut engine = page();
    run(
        &mut engine,
        "body.setAttribute('onload', 'log.push(\"load \" + (this === window))'); body.setAttribute('onclick', 'log.push(\"body\")');",
    );
    assert_eq!(
        report(
            &mut engine,
            "[typeof window.onload, typeof body.onload, body.onload === window.onload]"
        ),
        "function,function,true"
    );
    run(
        &mut engine,
        "dispatchEvent(new Event('load')); body.dispatchEvent(new Event('click'));",
    );
    assert_eq!(report(&mut engine, "log.join()"), "load true,body");
    assert_eq!(
        report(&mut engine, "typeof window.onclick"),
        "object",
        "a global one stays the body's"
    );
    run(&mut engine, "body.onload = null;");
    assert_eq!(report(&mut engine, "window.onload"), "null");
}

#[test]
fn a_windows_onerror_gets_the_errors_parts() {
    let mut engine = page();
    run(
        &mut engine,
        "window.onerror = function (message, source, line, column, error) { \
           log.push([message, source, line, column, error && error.message].join(' ')); return true; }; \
         addEventListener('error', e => log.push('canceled ' + e.defaultPrevented)); \
         d.addEventListener('x', () => { throw new Error('boom'); }); d.dispatchEvent(new Event('x'));",
    );
    assert_eq!(
        report(&mut engine, "log.join()"),
        "Uncaught Error: boom https://example.test/page.html 1 269 boom,canceled true",
        "its true cancels the event"
    );
    // A `<body>`'s `onerror` content attribute: the window's, compiled
    // with the five parameters.
    run(
        &mut engine,
        "log = []; window.onerror = null; \
         body.setAttribute('onerror', 'log.push([typeof event, typeof source, lineno, colno, error.message].join(\" \"))'); \
         d.dispatchEvent(new Event('x'));",
    );
    // Set to null before, its listener went: it comes after the `error`
    // listener now. Its return value isn't `true`: not canceled.
    assert_eq!(
        report(&mut engine, "log.join()"),
        "canceled false,string string 1 269 boom"
    );
}

#[test]
fn the_parsers_handlers_are_at_their_lines() {
    let mut engine = Box::new(V8Engine::new(EngineSettings { web_tests: true }));
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    let mut parser = html::parser::Parser::<V8Engine>::new(engine.dom_mut(), &document);
    parser.push("<!DOCTYPE html>\n<body>\n\n<div id=d onclick=\"\n  null.x\"></div>");
    let (_, back) = parser.resume(engine);
    engine = parser.finish(back);
    run(
        &mut engine,
        "var log = []; addEventListener('error', e => log.push(e.lineno)); \
         document.getElementById('d').dispatchEvent(new Event('click'));",
    );
    // The parser's line as it sets the attributes: where the start tag
    // ends (line 5), as Blink's `TextPosition` then; the throw is on the
    // source's second line.
    assert_eq!(report(&mut engine, "log.join()"), "6");
}
