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

/// `source`'s value and what its statements before the first that
/// dispatches an event (`dispatchEvent`, the tests' `fire`) asked of V8
/// (`env::slow_paths`): the dispatch is run after the count, as it is the
/// whole front end's, in a scope of its own. Statements as one expression
/// (the comma operator), unless `source` is one already.
fn counted_before_dispatch(engine: &mut V8Engine, source: &str) -> (String, u64) {
    let expression = |statements: &str| format!("({})", statements.replace("; ", ", "));
    if source.starts_with("(() =>") {
        let before = crate::env::slow_paths();
        let found = report(engine, source);
        return (found, crate::env::slow_paths() - before);
    }
    let statements: Vec<&str> = source.split("; ").collect();
    let dispatch = statements
        .iter()
        .position(|statement| statement.contains("dispatchEvent(") || statement.contains("fire("));
    match dispatch {
        Some(at) if at > 0 => {
            let before = crate::env::slow_paths();
            run(engine, &format!("{};", statements[..at].join("; ")));
            let made = crate::env::slow_paths() - before;
            (
                report(engine, &expression(&statements[at..].join("; "))),
                made,
            )
        }
        _ => {
            let before = crate::env::slow_paths();
            let found = report(engine, &expression(source));
            (found, crate::env::slow_paths() - before)
        }
    }
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

/// A callback is its argument, handed to the DOM's function and back as
/// the front end's handle (nothing of it kept by the front end): an event
/// handler attribute's value, a listener to remove, an animation frame's
/// callback. A call given an object makes no scope; an options dictionary
/// is read by the whole front end when one is given.
#[test]
fn a_callback_is_its_arguments_handle() {
    let mut engine = page();
    run(
        &mut engine,
        "var f = () => log.push('f'); var g = () => log.push('g'); \
         d.addEventListener('x', f); d.addEventListener('x', g); d.addEventListener('y', f, true);",
    );
    for (source, value, at_once) in [
        // A handler set and replaced; what isn't an object is null.
        ("d.onclick = f; d.onclick === f", "true", true),
        ("d.onclick = g; d.onclick === g", "true", true),
        ("d.onclick = 7; d.onclick", "null", true),
        ("d.onclick = f; d.onclick = null; d.onclick", "null", true),
        // The listener removed is the one whose callback the argument
        // is, of the same capture.
        (
            "d.removeEventListener('x', f); d.dispatchEvent(new Event('x')); log.splice(0).join()",
            "g",
            true,
        ),
        (
            "d.removeEventListener('y', f); d.dispatchEvent(new Event('y')); log.splice(0).join()",
            "f",
            true,
        ),
        (
            "d.removeEventListener('y', f, true); d.dispatchEvent(new Event('y')); \
             log.splice(0).join()",
            "",
            true,
        ),
        ("d.removeEventListener('x', null)", "undefined", true),
        // Not the first of its type: the one whose callback it is.
        (
            "d.addEventListener('v', f); d.addEventListener('v', g); \
             d.removeEventListener('v', g); d.dispatchEvent(new Event('v')); log.splice(0).join()",
            "f",
            true,
        ),
        // An options dictionary: its member read (a scope).
        (
            "d.addEventListener('z', f, true); d.removeEventListener('z', f, { capture: true }); \
             d.dispatchEvent(new Event('z')); log.splice(0).join()",
            "",
            false,
        ),
        (
            "d.addEventListener('w', f, true); d.removeEventListener('w', f, { capture: false }); \
             d.dispatchEvent(new Event('w')); log.splice(0).join()",
            "f",
            false,
        ),
        // What isn't a callback.
        (
            "(() => { try { d.removeEventListener('x', 7); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
        (
            "(() => { try { requestAnimationFrame({}); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
        ("typeof requestAnimationFrame(f)", "number", true),
    ] {
        // (What dispatches after the call under test is run after the
        // count: a dispatch is the whole front end's, in a scope.)
        let (found, made) = counted_before_dispatch(&mut engine, source);
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        } else {
            assert!(made >= 1, "{source}: a scope");
        }
    }
    // The handler set is the one called.
    run(&mut engine, "d.onclick = () => log.push('h');");
    assert_eq!(
        report(
            &mut engine,
            "(d.dispatchEvent(new Event('click')), log.splice(0).join())"
        ),
        "h"
    );
}

/// A value of any type, a call's rest arguments and a dictionary's object
/// (`signal`) are the front end's handles, taken before the operation and
/// handed back to it to store: a listener added with no options or a
/// boolean, a timer set with no more arguments and a `detail` given make
/// no scope; a dictionary's members are read, more arguments put in an
/// array and a missing `detail` made null in one.
#[test]
fn a_value_rest_arguments_and_a_signal_are_handles() {
    let mut engine = page();
    run(
        &mut engine,
        "var f = () => log.push('f'); var g = (...more) => log.push('g' + more.join('')); \
         var fire = (type) => (d.dispatchEvent(new Event(type)), log.splice(0).join()); \
         var controller = new AbortController(); var aborted = AbortSignal.abort(); \
         var custom = new CustomEvent('c'); var detail = { a: 1 }; \
         var fresh = new CustomEvent('n'); var unaborted = new AbortController().signal; \
         var walker = document.createTreeWalker(d); var registry = customElements; \
         var sheet = new CSSStyleSheet(); sheet.insertRule('a {}'); \
         d.addEventListener('first', f); setTimeout(f, 100000);",
    );
    for (source, value, at_once) in [
        // No options, a boolean: the listener added, capturing or not.
        ("d.addEventListener('a', f); fire('a')", "f", true),
        ("d.addEventListener('a', f); fire('a')", "f", true),
        (
            "d.addEventListener('b', f, true); d.removeEventListener('b', f); fire('b')",
            "f",
            true,
        ),
        (
            "d.addEventListener('b', f, undefined); fire('b')",
            "f,f",
            true,
        ),
        ("d.addEventListener('b', null)", "undefined", true),
        // A dictionary: its members read.
        (
            "d.addEventListener('c', f, { once: true }); fire('c') + fire('c')",
            "f",
            false,
        ),
        (
            "d.addEventListener('e', f, { capture: true }); d.removeEventListener('e', f, true); fire('e')",
            "",
            false,
        ),
        // Its `signal`: the listener removed as it aborts; none added for
        // one aborted already; a `TypeError` for what isn't one.
        (
            "d.addEventListener('s', f, { signal: controller.signal }); \
             [fire('s'), (controller.abort(), fire('s'))].join('|')",
            "f|",
            false,
        ),
        (
            "d.addEventListener('s', f, { signal: aborted }); fire('s')",
            "",
            false,
        ),
        (
            "(() => { try { d.addEventListener('s', f, { signal: d }); return 'none'; } \
             catch (e) { return e.name + fire('s'); } })()",
            "TypeError",
            false,
        ),
        (
            "(() => { try { d.addEventListener('s', f, { get signal() { throw new RangeError('no'); } }); \
             return 'none'; } catch (e) { return e.name + fire('s'); } })()",
            "RangeError",
            false,
        ),
        // A `detail` given is the event's; a missing one null.
        (
            "custom.initCustomEvent('c', false, false, detail); custom.detail === detail",
            "true",
            true,
        ),
        (
            "custom.initCustomEvent('c', false, false, 7); custom.detail",
            "7",
            true,
        ),
        (
            "custom.initCustomEvent('c', false, false); custom.detail",
            "null",
            false,
        ),
        (
            "custom.initCustomEvent('c', false, false, undefined); custom.detail",
            "null",
            false,
        ),
        // A value read back is the one stored, or what its type gives
        // for none: null, undefined.
        ("custom.detail === custom.detail", "true", true),
        ("fresh.detail", "null", true),
        (
            "controller.signal.reason === controller.signal.reason",
            "true",
            true,
        ),
        ("unaborted.reason", "undefined", true),
        ("registry.get('x-none')", "undefined", true),
        ("walker.filter", "null", true),
        // A missing optional integer is 0, as `ToUint32` of undefined.
        ("sheet.insertRule('b {}')", "0", true),
        ("sheet.insertRule('c {}', 2)", "2", true),
        // A timer with no more arguments; one with some, in an array.
        ("typeof setTimeout(f, 0)", "number", true),
        ("typeof setInterval(f, 100000)", "number", true),
        ("typeof setTimeout(f)", "number", true),
        ("typeof setTimeout(g, 0, 1, 'x', 2)", "number", false),
        // A string to compile is taken by the whole front end.
        ("typeof setTimeout('log.push(\\'s\\')', 0)", "number", false),
    ] {
        let (found, made) = counted_before_dispatch(&mut engine, source);
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        } else {
            assert!(made >= 1, "{source}: a scope");
        }
    }
    // The timers' callbacks are called with what they were set with.
    assert!(engine.run_timers());
    assert_eq!(report(&mut engine, "log.splice(0).join()"), "f,f,g1x2,s");
}

/// An event handler's getter gives its listener's callback in its one
/// operation, from the listener's own handle: no scope. A content
/// attribute's source is compiled after the operation, the first time
/// (a scope: script), and is the function from then on; one that doesn't
/// compile is null.
#[test]
fn a_handlers_getter_gives_its_callback_in_the_operation() {
    let mut engine = page();
    run(
        &mut engine,
        "var f = () => log.push('f'); d.onclick = f; window.onresize = f; document.onkeydown = f; \
         d.setAttribute('onfocus', 'log.push(\\'attr\\')'); d.setAttribute('onblur', 'this is not script('); \
         body.setAttribute('onload', 'log.push(\\'load\\')'); d.id;",
    );
    for (source, value, at_once) in [
        ("d.onclick === f", "true", true),
        ("d.oninput", "null", true),
        ("window.onresize === f", "true", true),
        ("window.onscroll", "null", true),
        ("document.onkeydown === f", "true", true),
        ("document.onkeyup", "null", true),
        // `<body>`'s window-reflecting one is its window's.
        ("body.onresize === f", "true", true),
        // Compiled the first time; the same function then.
        ("typeof d.onfocus", "function", false),
        ("typeof d.onfocus", "function", true),
        ("d.onfocus === d.onfocus", "true", true),
        ("d.onfocus(); log.splice(0).join()", "attr", true),
        ("typeof window.onload", "function", false),
        ("body.onload === window.onload", "true", true),
        // A source that doesn't compile: null, then and after.
        ("d.onblur", "null", false),
        ("d.onblur", "null", true),
        // Set again, the attribute's source is uncompiled again.
        (
            "d.setAttribute('onfocus', 'log.push(2)'); typeof d.onfocus",
            "function",
            false,
        ),
        ("d.onfocus(); log.splice(0).join()", "2", true),
        // A handler set over it is the handler.
        ("d.onfocus = f; d.onfocus === f", "true", true),
        ("d.onfocus = null; d.onfocus", "null", true),
    ] {
        let before = crate::env::slow_paths();
        let found = report(&mut engine, &format!("({})", source.replace("; ", ", ")));
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        } else {
            assert!(made >= 1, "{source}: a scope");
        }
    }
}

/// A result the engine allocates for (an array, a dictionary, a promise,
/// a value made at the first ask) leaves the operation as its type's own
/// data and is made after it, in a scope: the members giving one take
/// the path with the callback's info alone.
#[test]
fn a_result_to_allocate_for_is_made_after_the_operation() {
    let mut engine = page();
    run(
        &mut engine,
        "d.setAttribute('title', 't'); var a = d.animate([{ opacity: 0 }, { opacity: 1 }], 1000); \
         var effect = a.effect; var observer = new MutationObserver(() => {}); \
         observer.observe(d, { attributes: true }); var ctx = document.createElement('canvas').getContext('2d'); \
         ctx.setLineDash([1, 2]); customElements.define('x-now', class extends HTMLElement {}); var io = new IntersectionObserver(() => {}, { threshold: [0, 0.5] }); var mark = performance.mark('m', { detail: { k: 1 } }); d.id;",
    );
    for (source, value) in [
        ("d.getAttributeNames().join()", "id,title"),
        ("Array.isArray(d.getAttributeNames())", "true"),
        ("d.getAttributeNames() !== d.getAttributeNames()", "true"),
        (
            "d.getAnimations().length + ' ' + (d.getAnimations()[0] === a)",
            "1 true",
        ),
        ("document.getAnimations()[0] === a", "true"),
        (
            "d.setAttribute('lang', 'x'); observer.takeRecords().length + ' ' + observer.takeRecords().length",
            "1 0",
        ),
        ("ctx.getLineDash().join()", "1,2"),
        // A frozen array: the same while its values are.
        ("navigator.languages.join()", "en-US,en"),
        ("io.thresholds === io.thresholds", "true"),
        (
            "Object.isFrozen(io.thresholds) + ' ' + io.thresholds.join()",
            "true 0,0.5",
        ),
        // A dictionary; records.
        ("effect.getTiming().duration", "1000"),
        (
            "effect.getKeyframes().length + ' ' + effect.getKeyframes()[1].opacity",
            "2 1",
        ),
        // Promises: one kept on its object, made at the first ask.
        ("a.ready instanceof Promise", "true"),
        ("a.ready === a.ready", "true"),
        ("a.finished !== a.ready", "true"),
        // A pending promise is the same until it settles; one of a name
        // defined is new each time.
        (
            "customElements.whenDefined('x-later') === customElements.whenDefined('x-later')",
            "true",
        ),
        (
            "customElements.whenDefined('x-now') !== customElements.whenDefined('x-now')",
            "true",
        ),
        (
            "customElements.whenDefined('x-now') instanceof Promise",
            "true",
        ),
        (
            "document.createElement('img').decode() instanceof Promise",
            "true",
        ),
        // Values made at the first ask, the same after.
        ("history.state", "null"),
        (
            "mark.detail.k + ' ' + (mark.detail === mark.detail)",
            "1 true",
        ),
    ] {
        let before = crate::env::slow_paths();
        let found = report(&mut engine, &format!("({})", source.replace("; ", ", ")));
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        assert!(made >= 1, "{source}: a scope");
    }
    // What the promises settle with.
    run(
        &mut engine,
        "customElements.whenDefined('x-now').then((c) => log.push(c === customElements.get('x-now'))); \
         customElements.whenDefined('x-later').then((c) => log.push(c.name)); \
         customElements.define('x-later', class Later extends HTMLElement {}); \
         customElements.whenDefined('not a name').catch((e) => log.push(e.name));",
    );
    assert_eq!(
        report(&mut engine, "log.splice(0).join()"),
        "true,Later,SyntaxError"
    );
}

/// A static operation takes the path with the callback's info alone: it
/// has no receiver (its `this` is anything), and the current realm's
/// window it may ask for is found in its operation. An overloaded
/// method's function picks its form from the arguments with no scope and
/// calls it the same way.
#[test]
fn static_operations_and_overloads_are_given_the_info_alone() {
    let mut engine = page();
    run(
        &mut engine,
        "var canParse = URL.canParse; var supports = HTMLScriptElement.supports; \
         var canvas = document.createElement('canvas'); var ctx = canvas.getContext('2d'); \
         var path = new Path2D(); path.rect(0, 0, 10, 10); ctx.rect(0, 0, 10, 10); \
         var data = ctx.createImageData(1, 1); d.id;",
    );
    for (source, value, at_once) in [
        // Static: booleans at once, whatever `this` is.
        ("URL.canParse('https://a.test/')", "true", true),
        ("URL.canParse('nope')", "false", true),
        ("canParse('https://a.test/')", "true", true),
        ("canParse.call(7, 'x', 'https://a.test/')", "true", true),
        ("HTMLScriptElement.supports('module')", "true", true),
        ("supports('nothing')", "false", true),
        // A new string, a new object: made after the operation.
        ("CSS.escape('a b')", "a\\ b", false),
        ("URL.parse('https://a.test/b').pathname", "/b", false),
        ("URL.parse('nope')", "null", true),
        ("Response.error().type", "error", false),
        // The current realm's window found: the signal is its.
        ("AbortSignal.abort() instanceof AbortSignal", "true", false),
        ("AbortSignal.abort().aborted", "true", false),
        ("AbortSignal.abort().reason.name", "AbortError", false),
        ("AbortSignal.abort(7).reason", "7", false),
        ("AbortSignal.abort(d).reason === d", "true", false),
        // An argument missing: the `TypeError` of a call given too few.
        (
            "(() => { try { URL.canParse(); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
        // Overloads: the form by the number of arguments, and by what
        // the first is.
        ("ctx.isPointInPath(1, 1)", "true", true),
        ("ctx.isPointInPath(20, 20)", "false", true),
        ("ctx.isPointInPath(path, 1, 1)", "true", true),
        ("ctx.isPointInPath(path, 20, 1)", "false", true),
        (
            "ctx.fill(); ctx.fill('evenodd'); ctx.fill(path); ctx.fill(path, 'nonzero')",
            "undefined",
            true,
        ),
        (
            "ctx.setTransform(1, 0, 0, 1, 0, 0); ctx.setTransform()",
            "undefined",
            true,
        ),
        ("CSS.supports('color', 'red')", "true", true),
        ("CSS.supports('color: red')", "true", true),
        ("CSS.supports('not-a-property', 'red')", "false", true),
        // A number of arguments none of a static overload's forms takes.
        (
            "(() => { try { CSS.supports(); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
        // A form that isn't plain is called as before, in a scope.
        ("ctx.putImageData(data, 0, 0)", "undefined", true),
        (
            "ctx.putImageData(data, 0, 0, 0, 0, 1, 1)",
            "undefined",
            true,
        ),
        ("d.scrollTo(0, 0) instanceof Promise", "true", false),
        ("d.scrollTo({ top: 0 }) instanceof Promise", "true", false),
        // A number of arguments no form takes.
        (
            "(() => { try { ctx.putImageData(data, 1, 2, 3); return 'none'; } catch (e) { return e.name + ': ' + e.message; } })()",
            "TypeError: Failed to execute 'putImageData': Valid arities are: [3, 7], but 4 arguments provided.",
            false,
        ),
        // A method's receiver is still checked.
        (
            "(() => { try { ctx.fill.call(d); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            true,
        ),
    ] {
        let before = crate::env::slow_paths();
        let expression = if source.starts_with("(() =>") {
            source.to_owned()
        } else {
            format!("({})", source.replace("; ", ", "))
        };
        let found = report(&mut engine, &expression);
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        } else {
            assert!(made >= 1, "{source}: a scope");
        }
    }
}

/// What a member's function asks of the front end's side in its operation
/// the front end that is the callback's info has too, so the members
/// whose functions ask take that path: an aborted signal's reason thrown
/// (the error carries the signal out; the reason is read as it is
/// thrown), an `ImageData`'s pixels (carried out with the object, its
/// array made after), an array's bytes read in place, `HTMLElement`'s
/// constructor's `new.target`.
#[test]
fn what_a_function_asks_of_the_operations_front_end_every_front_end_has() {
    let mut engine = page();
    run(
        &mut engine,
        "var reason = { why: 1 }; var controller = new AbortController(); controller.abort(reason); \
         var signal = controller.signal; var fine = new AbortController().signal; \
         var ctx = document.createElement('canvas').getContext('2d'); \
         var data = ctx.createImageData(1, 1); data.data.set([1, 2, 3, 255]); ctx.putImageData(data, 0, 0); \
         class Mine extends HTMLElement { constructor() { super(); this.made = true; } } \
         customElements.define('x-mine', Mine); var upgraded = document.createElement('x-later'); \
         d.appendChild(upgraded); d.id;",
    );
    for (source, value, at_once) in [
        // The reason itself, thrown: the same object.
        (
            "(() => { try { signal.throwIfAborted(); return 'none'; } catch (e) { return e === reason; } })()",
            "true",
            false,
        ),
        ("fine.throwIfAborted()", "undefined", true),
        (
            "(() => { try { AbortSignal.abort().throwIfAborted(); return 'none'; } catch (e) { return e.name; } })()",
            "AbortError",
            false,
        ),
        // Pixels: read from the bitmap, in a new array each time; a new
        // `ImageData`'s zeroed; written back with no scope.
        ("data.data.join()", "1,2,3,255", true),
        (
            "ctx.getImageData(0, 0, 1, 1).data.join()",
            "1,2,3,255",
            false,
        ),
        (
            "ctx.getImageData(0, 0, 1, 1).data !== data.data",
            "true",
            false,
        ),
        ("ctx.createImageData(1, 1).data.join()", "0,0,0,0", false),
        ("ctx.createImageData(data).width", "1", false),
        (
            "new ImageData(2, 1).data.length + ' ' + new ImageData(2, 1).width",
            "8 2",
            false,
        ),
        (
            "data.data[1] = 9; ctx.putImageData(data, 1, 1); ctx.getImageData(1, 1, 1, 1).data.join()",
            "1,9,3,255",
            false,
        ),
        (
            "(() => { try { new ImageData(0, 1); return 'none'; } catch (e) { return e.name; } })()",
            "IndexSizeError",
            false,
        ),
        // `HTMLElement`'s constructor: a new element of the definition
        // `new.target` is the constructor of; itself, or a function with
        // no definition, a `TypeError`; an element upgraded is the one
        // that was there, of the class's prototype.
        ("new Mine() instanceof Mine", "true", false),
        (
            "new Mine().localName + ' ' + new Mine().made",
            "x-mine true",
            false,
        ),
        (
            "(() => { try { new HTMLElement(); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
        (
            "(() => { try { Reflect.construct(HTMLElement, [], function () {}); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
        (
            "(() => { customElements.define('x-later', class Later extends HTMLElement { constructor() { super(); this.up = 1; } }); \
             return upgraded.up + ' ' + (d.lastChild === upgraded) + ' ' + upgraded.constructor.name; })()",
            "1 true Later",
            false,
        ),
    ] {
        let before = crate::env::slow_paths();
        let expression = if source.starts_with("(() =>") {
            source.to_owned()
        } else {
            format!("({})", source.replace("; ", ", "))
        };
        let found = report(&mut engine, &expression);
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        } else {
            assert!(made >= 1, "{source}: a scope");
        }
    }
    // A body's read rejects with its signal's reason.
    run(
        &mut engine,
        "var aborting = new AbortController(); \
         fetch('https://example.test/x', { signal: aborting.signal }).catch((e) => log.push(e === reason)); \
         aborting.abort(reason);",
    );
    assert_eq!(report(&mut engine, "log.splice(0).join()"), "true");
}

/// A constructor takes the path with the callback's info alone: called
/// with `new` and enough arguments, its new object (V8's, of `new.target`'s
/// prototype) is made the wrapper of what it constructs, and the current
/// realm's window is found for the ones that ask, with no scope. An
/// interface's several constructors are one function that picks.
#[test]
fn a_constructor_is_given_the_info_alone() {
    let mut engine = page();
    run(
        &mut engine,
        "class Sub extends Event { get sub() { return true; } } var init = { bubbles: true }; \
         var bytes = new Uint8ClampedArray(8); new Event('first'); d.id;",
    );
    for (source, value, at_once) in [
        ("new Event('x') instanceof Event", "true", true),
        ("new Event('x').type", "x", false),
        ("new Event('x') !== new Event('x')", "true", true),
        ("new EventTarget() instanceof EventTarget", "true", true),
        ("new AbortController().signal.aborted", "false", false),
        (
            "new MessageChannel() instanceof MessageChannel",
            "true",
            true,
        ),
        ("new DocumentFragment().nodeType", "11", true),
        (
            "new Text('abc').data + ' ' + (new Text('a').ownerDocument === document)",
            "abc true",
            false,
        ),
        ("new Comment().nodeType", "8", true),
        // The window found: the event's time is its clock's (a number
        // that is no small integer is the return value's own to make).
        ("new Event('x').timeStamp >= 0", "true", true),
        // A dictionary's members read in a scope.
        ("new Event('x', init).bubbles", "true", false),
        (
            "new CustomEvent('c', { detail: d }).detail === d",
            "true",
            false,
        ),
        // `new.target`'s prototype.
        ("new Sub('x') instanceof Sub", "true", true),
        (
            "new Sub('x').sub + ' ' + new Sub('y').type",
            "true y",
            false,
        ),
        ("Reflect.construct(Event, ['z'], Sub).sub", "true", true),
        // Not called with `new`; too few arguments.
        (
            "(() => { try { Event('x'); return 'none'; } catch (e) { return e.name + ': ' + e.message; } })()",
            "TypeError: Failed to construct 'Event': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
            false,
        ),
        (
            "(() => { try { new Event(); return 'none'; } catch (e) { return e.name + ': ' + e.message; } })()",
            "TypeError: Failed to construct 'Event': 1 argument required, but only 0 present.",
            false,
        ),
        // Several constructors: by the arguments.
        (
            "new ImageData(bytes, 2).height + ' ' + new ImageData(2, 3).height",
            "1 3",
            false,
        ),
        (
            "(() => { try { new ImageData(); return 'none'; } catch (e) { return e.name; } })()",
            "TypeError",
            false,
        ),
    ] {
        let before = crate::env::slow_paths();
        let expression = if source.starts_with("(() =>") {
            source.to_owned()
        } else {
            format!("({})", source.replace("; ", ", "))
        };
        let found = report(&mut engine, &expression);
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        } else {
            assert!(made >= 1, "{source}: a scope");
        }
    }
    // A dictionary's members are all read in the one scope made for
    // them, however many it has.
    for source in [
        "new Event('x', { bubbles: true, cancelable: true, composed: true }).cancelable",
        "(d.addEventListener('q', f, { capture: true, once: true, passive: true }), true)",
    ] {
        // (Once before: a listener added grows the DOM's heap, which the
        // call after may tell V8 of, in a scope of its own.)
        run(
            &mut engine,
            &format!("var f = f || (() => {{}}); {source}; {source};"),
        );
        let before = crate::env::slow_paths();
        assert_eq!(report(&mut engine, source), "true", "{source}");
        assert_eq!(crate::env::slow_paths() - before, 1, "{source}");
    }
}

/// An init dictionary's `view` and `relatedTarget` and a `Uint8ClampedArray`
/// argument are the engine's own handles, made the DOM's in the operation:
/// their constructors take the path with the callback's info alone (the
/// dictionary read in a scope), the object given is the one read back, and
/// a value of another type is a `TypeError` before anything is made.
#[test]
fn an_objects_handle_is_taken_as_it_is() {
    let mut engine = page();
    run(
        &mut engine,
        "var et = new EventTarget(); var bytes = new Uint8ClampedArray(8); \
         var thrown = (f) => { try { f(); return 'none'; } catch (e) { return e.name + ': ' + e.message; } }; \
         new MouseEvent('first', { view: window, relatedTarget: d }); new ImageData(bytes, 2); d.id;",
    );
    for (source, value, scopes) in [
        (
            "new UIEvent('x', { view: window }).view === window",
            "true",
            Some(true),
        ),
        ("new UIEvent('x', { view: null }).view", "null", Some(true)),
        (
            "new UIEvent('x', { view: undefined }).view",
            "null",
            Some(true),
        ),
        ("new UIEvent('x', {}).view", "null", Some(true)),
        ("new UIEvent('x').view", "null", None),
        (
            "new MouseEvent('x', { view: window, relatedTarget: d }).relatedTarget === d",
            "true",
            Some(true),
        ),
        (
            "new MouseEvent('x', { relatedTarget: window }).relatedTarget === window",
            "true",
            Some(true),
        ),
        (
            "new MouseEvent('x', { relatedTarget: et }).relatedTarget === et",
            "true",
            Some(true),
        ),
        (
            "new MouseEvent('x', { relatedTarget: null }).relatedTarget",
            "null",
            Some(true),
        ),
        (
            "new WheelEvent('x', { view: window, relatedTarget: d, deltaY: 3 }).deltaY",
            "3",
            Some(true),
        ),
        (
            "new FocusEvent('x', { relatedTarget: d }).relatedTarget === d",
            "true",
            Some(true),
        ),
        (
            "new KeyboardEvent('x', { view: window, key: 'a' }).view === window",
            "true",
            Some(true),
        ),
        (
            "new InputEvent('x', { view: window, data: 'a' }).view === window",
            "true",
            Some(true),
        ),
        (
            "thrown(() => new UIEvent('x', { view: d }))",
            "TypeError: Failed to read the 'view' property: Failed to convert value to 'Window'.",
            Some(true),
        ),
        (
            "thrown(() => new UIEvent('x', { view: 5 }))",
            "TypeError: Failed to read the 'view' property: Failed to convert value to 'Window'.",
            Some(true),
        ),
        (
            "thrown(() => new MouseEvent('x', { relatedTarget: {} }))",
            "TypeError: Failed to read the 'relatedTarget' property: Failed to convert value to 'EventTarget'.",
            Some(true),
        ),
        (
            "thrown(() => new MouseEvent('x', { relatedTarget: new Event('e') }))",
            "TypeError: Failed to read the 'relatedTarget' property: Failed to convert value to 'EventTarget'.",
            Some(true),
        ),
        // The array itself is the new object's: no scope to take it.
        (
            "new ImageData(bytes, 2).data === bytes",
            "true",
            Some(false),
        ),
        ("new ImageData(bytes, 1, 2).height", "2", None),
        // A body's blob and `URLSearchParams`, an init's signal.
        (
            "var c = new AbortController(); new Request('https://example.test/', { signal: c.signal }).signal.aborted",
            "false",
            None,
        ),
        (
            "c.abort(); new Request('https://example.test/', { signal: c.signal }).signal.aborted",
            "true",
            None,
        ),
        (
            "new Request('https://example.test/', { signal: null }).signal.aborted",
            "false",
            None,
        ),
        // A null signal is given (the input's isn't followed); a missing
        // one isn't.
        (
            "var c2 = new AbortController(); var from = new Request('https://example.test/', { signal: c2.signal }); \
             var unfollowing = new Request(from, { signal: null }); var following = new Request(from, {}); \
             c2.abort(); unfollowing.signal.aborted + ' ' + following.signal.aborted",
            "false true",
            None,
        ),
        (
            "thrown(() => new Request('https://example.test/', { signal: et }))",
            "TypeError: Failed to construct 'Request': Failed to read the 'signal' property from 'RequestInit': Failed to convert value to 'AbortSignal'.",
            None,
        ),
        (
            "new Response(new Blob(['ab'], { type: 'text/x' })).headers.get('content-type')",
            "text/x",
            None,
        ),
        (
            "new Response(new URLSearchParams('a=1')).headers.get('content-type')",
            "application/x-www-form-urlencoded;charset=UTF-8",
            None,
        ),
        (
            "new Request('https://example.test/', { method: 'POST', body: new Blob(['ab'], { type: 'text/y' }) }).headers.get('content-type')",
            "text/y",
            None,
        ),
        (
            "thrown(() => new ImageData(new Uint8ClampedArray(6), 1))",
            "InvalidStateError: The input data length is not a multiple of 4.",
            Some(true),
        ),
        // Another typed array is no `Uint8ClampedArray`: the other
        // constructor's width, zero.
        (
            "thrown(() => new ImageData(new Uint8Array(8), 2)).slice(0, 14)",
            "IndexSizeError",
            None,
        ),
    ] {
        let before = crate::env::slow_paths();
        let found = match source.rsplit_once("; ") {
            Some((first, last)) => {
                run(&mut engine, first);
                report(&mut engine, last)
            }
            None => report(&mut engine, source),
        };
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        match scopes {
            Some(true) => assert!(made >= 1, "{source}: a scope"),
            Some(false) => assert_eq!(made, 0, "{source}"),
            None => {}
        }
    }
}

/// A member whose argument is read whole into the DOM's values before the
/// operation (a sequence, a dictionary of its own shape, a serialization,
/// keyframes) takes the path with the callback's info alone: the argument
/// is read in a scope made for it, and one that is a boolean, a small
/// integer or a wrapper already is told with none.
#[test]
fn an_argument_read_whole_is_read_in_a_scope() {
    let mut engine = page();
    run(
        &mut engine,
        "var canvas = document.createElement('canvas'); var ctx = canvas.getContext('2d'); \
         var gradient = ctx.createLinearGradient(0, 0, 1, 1); var view = new Uint8Array(8); \
         var c = new AbortController(); var channel = new MessageChannel(); \
         var thrown = (f) => { try { f(); return 'none'; } catch (e) { return e.name + ': ' + e.message; } }; \
         var path = new Path2D(); var big = new Uint8Array(64); var part = new Uint8Array(new ArrayBuffer(40), 8, 32); var effect = new KeyframeEffect(d, null); \
         d.hidden = false; ctx.fillStyle = 'blue'; ctx.setLineDash([1]); \
         crypto.getRandomValues(view); structuredClone(1); new Blob(['a']); d.id;",
    );
    for (source, value, scopes) in [
        // Told with no scope: a boolean, a small integer, a wrapper.
        ("d.hidden = true; d.hidden", "true", Some(false)),
        ("d.hidden = 0; d.hidden", "false", Some(false)),
        ("d.hidden = 'until-found'; d.hidden", "until-found", None),
        // (A string converted: the scope tells the path taken.)
        (
            "d.hidden = { toString() { return 'until-found'; } }; d.hidden",
            "until-found",
            Some(true),
        ),
        (
            "ctx.fillStyle = { toString() { return '#00ff00'; } }; ctx.fillStyle",
            "#00ff00",
            Some(true),
        ),
        (
            "ctx.fillStyle = gradient; ctx.fillStyle === gradient",
            "true",
            Some(false),
        ),
        ("ctx.fillStyle = '#ff0000'; ctx.fillStyle", "#ff0000", None),
        // A sequence, a dictionary, a serialization: a scope.
        (
            "ctx.setLineDash([2, 3]); ctx.getLineDash().join()",
            "2,3",
            Some(true),
        ),
        (
            "thrown(() => ctx.setLineDash(5)).slice(0, 9)",
            "TypeError",
            Some(true),
        ),
        (
            "ctx.roundRect(0, 0, 4, 4, [1, 2]); path.roundRect(0, 0, 4, 4, 1); true",
            "true",
            Some(true),
        ),
        // (No raster in the tests: an empty canvas's URL.)
        (
            "canvas.toDataURL('image/png', 0.5).slice(0, 5)",
            "data:",
            Some(true),
        ),
        ("crypto.getRandomValues(view) === view", "true", Some(true)),
        (
            "crypto.getRandomValues(big); big.some((byte) => byte !== 0)",
            "true",
            Some(true),
        ),
        // A view of part of a buffer: the bytes before it aren't written.
        (
            "crypto.getRandomValues(part); new Uint8Array(part.buffer).slice(0, 8).every((byte) => byte === 0) \
             + ' ' + part.some((byte) => byte !== 0)",
            "true true",
            Some(true),
        ),
        (
            "thrown(() => crypto.getRandomValues(new Float32Array(4))).slice(0, 17)",
            "TypeMismatchError",
            Some(true),
        ),
        (
            "thrown(() => crypto.getRandomValues({})).slice(0, 9)",
            "TypeError",
            Some(true),
        ),
        ("structuredClone({ a: [1, 2] }).a.join()", "1,2", Some(true)),
        ("new Blob(['ab', new Uint8Array(3)]).size", "5", Some(true)),
        ("new File(['abc'], 'f.txt').name", "f.txt", Some(true)),
        ("new Headers([['a', 'b']]).get('a')", "b", Some(true)),
        ("new Headers({ c: 'd' }).get('c')", "d", Some(true)),
        (
            "new URLSearchParams([['a', '1']]).get('a')",
            "1",
            Some(true),
        ),
        ("new URLSearchParams('b=2').get('b')", "2", None),
        ("AbortSignal.any([c.signal]).aborted", "false", Some(true)),
        (
            "thrown(() => AbortSignal.any([d])).slice(0, 9)",
            "TypeError",
            Some(true),
        ),
        (
            "Response.json({ a: 1 }).headers.get('content-type')",
            "application/json",
            Some(true),
        ),
        (
            "new Response('body', { status: 201, headers: { x: 'y' } }).headers.get('x')",
            "y",
            Some(true),
        ),
        (
            "var request = new Request('https://example.test/a', { headers: [['p', 'q']] }); \
             new Request(request).headers.get('p')",
            "q",
            None,
        ),
        (
            "new StaticRange({ startContainer: d, startOffset: 0, endContainer: d, endOffset: 0 }).startContainer === d",
            "true",
            Some(true),
        ),
        (
            "d.getHTML({ serializableShadowRoots: true })",
            "",
            Some(true),
        ),
        (
            "history.pushState({ a: 7 }, ''); history.state.a",
            "7",
            Some(true),
        ),
        (
            "history.replaceState({ a: 8 }, ''); history.state.a",
            "8",
            Some(true),
        ),
        (
            "performance.mark('m', { detail: { b: 1 } }).detail.b",
            "1",
            Some(true),
        ),
        (
            "new PerformanceMark('n', { startTime: 5 }).startTime",
            "5",
            Some(true),
        ),
        ("performance.measure('span', 'm').name", "span", None),
        (
            "d.animate([{ opacity: 0 }, { opacity: 1 }], 100) instanceof Animation",
            "true",
            Some(true),
        ),
        (
            "new KeyframeEffect(d, { opacity: [0, 1] }, { duration: 50 }).getTiming().duration",
            "50",
            Some(true),
        ),
        (
            "effect.setKeyframes([{ opacity: 0.5 }]); effect.updateTiming({ duration: 9 }); \
             effect.getKeyframes().length + ' ' + effect.getTiming().duration",
            "1 9",
            Some(true),
        ),
        (
            "new Animation(new KeyframeEffect(d, null), null).timeline",
            "null",
            Some(true),
        ),
        (
            "new IntersectionObserver(() => {}, { root: d, threshold: [0, 1] }).thresholds.join()",
            "0,1",
            Some(true),
        ),
        (
            "channel.port1.postMessage({ a: 1 }, []); new Path2D('M0 0L1 1') instanceof Path2D",
            "true",
            Some(true),
        ),
        ("new Path2D(path) instanceof Path2D", "true", None),
    ] {
        let (first, last) = match source.rsplit_once("; ") {
            Some((first, last)) => (Some(first), last),
            None => (None, source),
        };
        if let Some(first) = first.filter(|first| first.starts_with("var ")) {
            run(&mut engine, first);
        }
        let before = crate::env::slow_paths();
        if let Some(first) = first.filter(|first| !first.starts_with("var ")) {
            run(&mut engine, first);
        }
        let found = report(&mut engine, last);
        let made = crate::env::slow_paths() - before;
        assert_eq!(found, value, "{source}");
        match scopes {
            Some(true) => assert!(made >= 1, "{source}: a scope"),
            Some(false) => assert_eq!(made, 0, "{source}"),
            None => {}
        }
    }
}

/// A result that is a number but no small integer (a fraction, -0, NaN,
/// an infinity, an integer past 31 bits) is V8's heap number, made by the
/// return value itself: no scope, and the number is the one given.
#[test]
fn a_number_that_is_no_small_integer_is_returned_with_no_scope() {
    let mut engine = page();
    run(
        &mut engine,
        "var ctx = document.createElement('canvas').getContext('2d'); \
         var walker = document.createTreeWalker(document); var input = document.createElement('input'); \
         input.type = 'number'; ctx.lineWidth; walker.whatToShow; d.id;",
    );
    for (setup, source, value) in [
        ("ctx.lineWidth = 1.5", "ctx.lineWidth", "1.5"),
        (
            "ctx.lineWidth = 0.1",
            "ctx.lineWidth + 0.2",
            "0.30000000000000004",
        ),
        ("ctx.lineWidth = 3", "ctx.lineWidth", "3"),
        (
            "ctx.lineDashOffset = -0",
            "Object.is(ctx.lineDashOffset, -0)",
            "true",
        ),
        ("ctx.lineDashOffset = -2.25", "ctx.lineDashOffset", "-2.25"),
        ("ctx.globalAlpha = 0.5", "ctx.globalAlpha", "0.5"),
        ("ctx.miterLimit = 1e300", "ctx.miterLimit", "1e+300"),
        // An unsigned long past a small integer's range.
        ("0", "walker.whatToShow", "4294967295"),
        ("0", "walker.whatToShow === 0xFFFFFFFF", "true"),
        (
            "input.value = ''",
            "Number.isNaN(input.valueAsNumber)",
            "true",
        ),
        ("input.value = '2.5'", "input.valueAsNumber", "2.5"),
    ] {
        run(&mut engine, setup);
        let before = crate::env::slow_paths();
        run(&mut engine, &format!("var got = {source};"));
        let made = crate::env::slow_paths() - before;
        assert_eq!(report(&mut engine, "got"), value, "{source}");
        assert_eq!(made, 0, "{source}");
    }
}

/// A cross-origin property and a member that finds another realm's
/// window are given V8's callback info alone too, and make the scope
/// they need (a message's to serialize it; V8 asked for the realm in
/// it), and none when they need none (a cross-origin getter or method
/// that makes nothing); a
/// legacy factory function is called as a constructor is, and gives what
/// it gave.
#[test]
fn a_member_that_needs_a_scope_is_given_the_info_and_makes_it() {
    let mut engine = page();
    run(
        &mut engine,
        "var blob = new Blob(['ab']); var got = []; location.href; new Image(); d.id;",
    );
    for (source, value) in [
        // A cross-origin property, on this window.
        ("(window.postMessage('m', '*'), 'posted')", "posted"),
        // The realm's window found for it.
        ("blob.text() instanceof Promise", "true"),
    ] {
        let before = crate::env::info_scopes();
        let found = report(&mut engine, source);
        let made = crate::env::info_scopes() - before;
        assert_eq!(found, value, "{source}");
        assert!(made >= 1, "{source}: a scope, of an info callback");
    }
    // A cross-origin getter or method with nothing for V8 to make: as any
    // member.
    for (source, value) in [
        // (Its string V8 has already: read above.)
        ("location.href", "https://example.test/page.html"),
        ("(document.location === location)", "true"),
        ("window.closed", "false"),
        ("window.length", "0"),
        ("(window.focus(), 'focused')", "focused"),
    ] {
        let before = crate::env::info_scopes();
        assert_eq!(report(&mut engine, source), value, "{source}");
        assert_eq!(crate::env::info_scopes(), before, "{source}: no scope");
    }
    for (source, value) in [
        ("new Image(3, 4).width", "3"),
        ("new Option('t', 'v').value", "v"),
        ("new Image() instanceof HTMLImageElement", "true"),
        ("new Option() instanceof HTMLOptionElement", "true"),
        (
            "Object.getPrototypeOf(new Image()) === HTMLImageElement.prototype",
            "true",
        ),
        (
            "(() => { try { Image(); } catch (e) { return e.constructor.name + ': ' + e.message; } })()",
            "TypeError: Failed to construct 'HTMLImageElement': Please use the 'new' operator, this DOM object constructor cannot be called as a function.",
        ),
        ("document.all(0) === document.documentElement", "true"),
        (
            "document.all.item.call(document.all, 0) === document.documentElement",
            "true",
        ),
        (
            "window.window === window && window.document === document",
            "true",
        ),
    ] {
        assert_eq!(report(&mut engine, source), value, "{source}");
    }
}

/// An argument that isn't the callback or the function its parameter
/// takes is a `TypeError` naming the parameter by its position, counted
/// from one.
#[test]
fn a_callback_that_isnt_one_is_named_by_its_position() {
    let mut engine = page();
    for (source, message) in [
        (
            "document.addEventListener('x', 5)",
            "parameter 2 is not of type 'EventListener'.",
        ),
        (
            "requestAnimationFrame(5)",
            "parameter 1 is not of type 'FrameRequestCallback'.",
        ),
    ] {
        let found = report(
            &mut engine,
            &format!(
                "(() => {{ try {{ {source}; return 'no error'; }} catch (e) {{ return e.constructor.name + ': ' + e.message; }} }})()"
            ),
        );
        assert!(
            found.starts_with("TypeError: ") && found.ends_with(message),
            "{source}: {found}"
        );
    }
}

/// A location's strings read for the first time (V8 has no copy yet),
/// and the nodes a walk with a filter gives (the filter is script; the
/// nodes have no wrappers yet): made after the operation.
#[test]
fn first_reads_and_a_filtered_walks_nodes_are_made_after_the_operation() {
    let mut engine = page();
    // The first read of each: nothing cached.
    assert_eq!(
        report(&mut engine, "location.href"),
        "https://example.test/page.html"
    );
    assert_eq!(report(&mut engine, "location.protocol"), "https:");
    run(
        &mut engine,
        "var root = document.createElement('div'); root.innerHTML = '<a><b></b></a><i></i>'; \
         var walker = document.createTreeWalker(root, NodeFilter.SHOW_ELEMENT, () => NodeFilter.FILTER_ACCEPT);",
    );
    assert_eq!(
        report(
            &mut engine,
            "[walker.nextNode(), walker.nextNode(), walker.nextNode(), walker.nextNode()].map(n => n && n.nodeName)"
        ),
        "A,B,I,"
    );
}
