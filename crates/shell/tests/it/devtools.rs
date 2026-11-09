//! A page's DevTools session, which V8's inspector answers through the
//! tab's renderer: what Chrome's DevTools relies on. Each frame's realm is a
//! context, for the frame DevTools calls it by, gone with its document;
//! `console` reaches DevTools (and the terminal, with where the call was);
//! uncaught exceptions and unhandled rejections are reported, and a
//! rejection handled later struck out; a session stays the tab's when its
//! renderer goes and a new one comes. A wasm document has no inspector.

use std::time::{Duration, Instant};

use net::test_server::{Route, unused_port};
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::remote_debugging::{Client, Machopo};

/// A websocket straight to machopo's first tab, and the tab's target id.
pub(crate) fn page(machopo: &Machopo) -> (Client, String) {
    let target = machopo.targets()[0]["id"].as_str().unwrap().to_owned();
    let url = format!("ws://127.0.0.1:{}/devtools/page/{target}", machopo.port);
    (machopo.connect(&url, None).expect("connects"), target)
}

/// Navigates the tab to `url` and waits for its document's `load`. The
/// lifecycle events go only to a session with `Page` on, as Chrome's: on
/// for the load if the test hasn't turned it on, and its events dropped.
pub(crate) fn load(page: &mut Client, url: &str) {
    let page_was_on = page.page_enabled();
    if !page_was_on {
        page.call(None, "Page.enable", json!({}));
    }
    page.call(
        None,
        "Page.setLifecycleEventsEnabled",
        json!({ "enabled": true }),
    );
    // The document shown's, told as the events are turned on.
    page.take_events("Page.lifecycleEvent");
    let navigated = page.call(None, "Page.navigate", json!({ "url": url }));
    assert!(navigated.get("result").is_some(), "{navigated}");
    let loader = navigated["result"]["loaderId"].clone();
    loop {
        let event = page.event("Page.lifecycleEvent");
        if event["params"]["name"] == "load" && event["params"]["loaderId"] == loader {
            break;
        }
    }
    if !page_was_on {
        page.call(None, "Page.disable", json!({}));
        page.take_events("Page.");
    }
}

/// `expression`'s value, evaluated in `context` (`None`: the main frame's).
pub(crate) fn evaluate(page: &mut Client, expression: &str, context: Option<i64>) -> Value {
    let mut params = json!({ "expression": expression, "returnByValue": true });
    if let Some(context) = context {
        params["contextId"] = context.into();
    }
    let answer = page.call(None, "Runtime.evaluate", params);
    answer["result"]["result"]["value"].clone()
}

/// Each frame's realm is a context of its frame (the main frame's id is
/// the target id), with its document's origin; `Runtime.evaluate` runs in
/// the one named, or the main frame's; a document discarded takes its
/// context with it, the main frame's all of them at once.
#[test]
fn every_frame_has_a_context_that_goes_with_its_document() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><title>Top</title><iframe src='/child'></iframe>\
                 <iframe src='data:text/html,<title>Data</title>'></iframe>",
            ),
        ),
        (
            "/child",
            Route::ok("text/html", "<!DOCTYPE html><title>Child</title>child"),
        ),
        (
            "/next",
            Route::ok("text/html", "<!DOCTYPE html><title>Next</title>"),
        ),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    load(&mut page, &server.url("/"));

    // What there is now, told as `Runtime.enable` turns the domain on: the
    // child frames' `about:blank` went with their first documents.
    let enabled = page.call(None, "Runtime.enable", json!({}));
    assert!(enabled.get("result").is_some(), "{enabled}");
    let mut contexts = Vec::new();
    for _ in 0..3 {
        let created = page.event("Runtime.executionContextCreated");
        contexts.push(created["params"]["context"].clone());
    }
    let main = contexts
        .iter()
        .find(|context| context["auxData"]["frameId"] == target.as_str())
        .unwrap_or_else(|| panic!("{contexts:?}"))
        .clone();
    let child = contexts
        .iter()
        .find(|context| {
            context["auxData"]["frameId"] != target.as_str() && context["origin"] != "://"
        })
        .unwrap_or_else(|| panic!("{contexts:?}"))
        .clone();
    // A `data:` document's origin is opaque, which Chrome gives as "://".
    assert!(
        contexts.iter().any(|context| context["origin"] == "://"),
        "{contexts:?}"
    );
    assert_eq!(main["auxData"]["isDefault"], true);
    let origin = server.url("/");
    let origin = origin.trim_end_matches('/');
    assert_eq!(
        (&main["origin"], &child["origin"]),
        (&json!(origin), &json!(origin))
    );
    assert!(
        child["auxData"]["frameId"]
            .as_str()
            .is_some_and(|id| id.contains('.')),
        "{child}"
    );
    let (main, child) = (main["id"].as_i64().unwrap(), child["id"].as_i64().unwrap());
    assert_eq!(evaluate(&mut page, "document.title", Some(child)), "Child");
    assert_eq!(evaluate(&mut page, "document.title", Some(main)), "Top");
    assert_eq!(evaluate(&mut page, "document.title", None), "Top");

    evaluate(&mut page, "document.querySelector('iframe').remove()", None);
    let destroyed = page.event("Runtime.executionContextDestroyed");
    assert_eq!(destroyed["params"]["executionContextId"], child);

    load(&mut page, &server.url("/next"));
    page.event("Runtime.executionContextsCleared");
    let created = page.event("Runtime.executionContextCreated");
    assert_eq!(
        created["params"]["context"]["auxData"]["frameId"],
        target.as_str()
    );
    assert_eq!(evaluate(&mut page, "document.title", None), "Next");
}

/// `console` reaches DevTools (its arguments as objects, the inspector's
/// time, where the call was) and the terminal, with where the call was.
#[test]
fn console_reaches_devtools_and_the_terminal() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html>\n<script>console.log('hi', 1 + 2, {a: 1});</script>",
        ),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Runtime.enable", json!({}));
    let url = server.url("/");
    load(&mut page, &url);
    let called = page.event("Runtime.consoleAPICalled")["params"].clone();
    assert_eq!(called["type"], "log");
    let args = called["args"].as_array().unwrap();
    assert_eq!(
        (&args[0]["value"], &args[1]["value"]),
        (&json!("hi"), &json!(3))
    );
    assert_eq!(args[2]["type"], "object", "{called}");
    assert!(args[2]["objectId"].is_string(), "{called}");
    // The inspector's clock is the wall clock (ms since the epoch).
    assert!(called["timestamp"].as_f64().unwrap() > 1.6e12, "{called}");
    let frame = &called["stackTrace"]["callFrames"][0];
    assert_eq!(
        (&frame["url"], &frame["lineNumber"]),
        (&json!(url), &json!(1))
    );
    assert_eq!(
        machopo.error_line("[console]"),
        format!("[console] hi 3 [object Object] ({url}:2)")
    );
    // Code without a URL has no place.
    evaluate(&mut page, "console.log('bare')", None);
    assert_eq!(machopo.error_line("[console]"), "[console] bare");
}

/// A session that turns `Runtime` on later is told the page's console
/// messages so far (before its answer); not the previous page's, which
/// went with its contexts.
#[test]
fn console_messages_are_the_pages() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>console.log('first')</script>",
            ),
        ),
        ("/next", Route::ok("text/html", "<!DOCTYPE html>next")),
    ]);
    let machopo = Machopo::start();
    let (mut page_session, _) = page(&machopo);
    load(&mut page_session, &server.url("/"));
    let (mut late, _) = page(&machopo);
    late.call(None, "Runtime.enable", json!({}));
    assert!(late.came("Runtime.consoleAPICalled"));
    let called = late.event("Runtime.consoleAPICalled");
    assert_eq!(called["params"]["args"][0]["value"], "first", "{called}");

    load(&mut page_session, &server.url("/next"));
    let (mut later, _) = page(&machopo);
    later.call(None, "Runtime.enable", json!({}));
    assert!(!later.came("Runtime.consoleAPICalled"));
}

/// An exception's stack is kept for DevTools only while a session has
/// `Runtime` on, as in Chrome: it costs every error made.
#[test]
fn stacks_are_kept_while_runtime_is_on() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    let throw = json!({ "expression": "throw new Error('x')" });
    let thrown = page.call(None, "Runtime.evaluate", throw.clone());
    let details = &thrown["result"]["exceptionDetails"];
    assert!(details.get("exception").is_some(), "{thrown}");
    assert!(details.get("stackTrace").is_none(), "{thrown}");
    page.call(None, "Runtime.enable", json!({}));
    let thrown = page.call(None, "Runtime.evaluate", throw.clone());
    assert!(
        thrown["result"]["exceptionDetails"]
            .get("stackTrace")
            .is_some(),
        "{thrown}"
    );
    page.call(None, "Runtime.disable", json!({}));
    let thrown = page.call(None, "Runtime.evaluate", throw);
    assert!(
        thrown["result"]["exceptionDetails"]
            .get("stackTrace")
            .is_none(),
        "{thrown}"
    );
}

/// Exceptions from before DevTools turned `Runtime` on are told as it
/// does, with the exceptions themselves and where they were: machopo gives
/// the place and the script, as there was no stack then.
#[test]
fn exceptions_before_runtime_is_on_are_told_later() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html>\n<script>\nthrow new Error('boom');</script>\n\
             <script>\nwindow.p = Promise.reject(new Error('later'));</script>",
        ),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    let url = server.url("/");
    load(&mut page, &url);
    page.call(None, "Runtime.enable", json!({}));
    for (text, line, description) in [
        ("Uncaught", 2, "Error: boom"),
        ("Uncaught (in promise)", 4, "Error: later"),
    ] {
        let thrown = page.event("Runtime.exceptionThrown")["params"]["exceptionDetails"].clone();
        assert_eq!(thrown["text"], text, "{thrown}");
        assert_eq!(
            (&thrown["url"], &thrown["lineNumber"]),
            (&json!(url), &json!(line)),
            "{thrown}"
        );
        assert!(
            thrown["exception"]["description"]
                .as_str()
                .is_some_and(|d| d.starts_with(description)),
            "{thrown}"
        );
    }
}

/// An uncaught exception and an unhandled rejection go to DevTools'
/// console, where the error was made, with the exception itself; a
/// handler added to a rejection later strikes it out, any of them.
#[test]
fn uncaught_exceptions_and_rejections_reach_devtools() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html>\n<script>\nthrow new Error('boom');</script>\n\
             <script>\nwindow.p = Promise.reject(new Error('later'));\n\
             window.q = Promise.reject(new Error('second'));</script>",
        ),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Runtime.enable", json!({}));
    let url = server.url("/");
    load(&mut page, &url);
    let thrown = page.event("Runtime.exceptionThrown")["params"]["exceptionDetails"].clone();
    assert_eq!(thrown["text"], "Uncaught", "{thrown}");
    assert_eq!(thrown["url"], url.as_str(), "{thrown}");
    // Where the error was made (its stack's top), not the `throw`.
    assert_eq!(
        (&thrown["lineNumber"], &thrown["columnNumber"]),
        (&json!(2), &json!(6)),
        "{thrown}"
    );
    assert!(
        thrown["exception"]["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Error: boom")),
        "{thrown}"
    );
    let rejected = page.event("Runtime.exceptionThrown")["params"]["exceptionDetails"].clone();
    assert_eq!(rejected["text"], "Uncaught (in promise)", "{rejected}");
    assert_eq!(rejected["url"], url.as_str(), "{rejected}");
    assert_eq!(rejected["lineNumber"], 4, "{rejected}");
    assert!(
        rejected["exception"]["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Error: later")),
        "{rejected}"
    );
    let id = rejected["exceptionId"].clone();
    page.event("Runtime.exceptionThrown");
    // Struck out as the handler is added, before the answer.
    evaluate(&mut page, "p.catch(() => {}); 0", None);
    assert!(page.came("Runtime.exceptionRevoked"));
    let revoked = page.event("Runtime.exceptionRevoked")["params"].clone();
    assert_eq!(revoked["exceptionId"], id, "{revoked}");
    assert_eq!(revoked["reason"], "Handler added to rejected promise");
}

/// The tab's session stays when its renderer goes: the next renderer has
/// it too, and the main frame is the same frame for DevTools.
#[test]
fn a_session_outlives_its_renderer() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>back"))]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    assert_eq!(evaluate(&mut page, "1 + 1", None), 2);
    page.call(None, "Page.navigate", json!({ "url": "about:crash" }));
    let gone = loop {
        let answer = page.call(None, "Runtime.evaluate", json!({ "expression": "1" }));
        if answer.get("error").is_some() {
            break answer;
        }
    };
    assert_eq!(gone["error"]["code"], -32000, "{gone}");
    load(&mut page, &server.url("/"));
    assert_eq!(
        evaluate(&mut page, "document.body.textContent", None),
        "back"
    );
    page.call(None, "Runtime.enable", json!({}));
    let created = page.event("Runtime.executionContextCreated");
    assert_eq!(
        created["params"]["context"]["auxData"]["frameId"],
        target.as_str()
    );
}

/// Of the rejections reported, the latest 1000 are struck out if a
/// handler comes later (Blink's `kMaxReportedHandlersPendingResolution`).
#[test]
fn the_latest_1000_rejections_are_struck_out() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html><script>\
             window.ps = Array.from({length: 1001}, (_, i) => Promise.reject(i));</script>",
        ),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Runtime.enable", json!({}));
    evaluate(&mut page, "ps[0].catch(() => {}); 0", None);
    assert!(!page.came("Runtime.exceptionRevoked"));
    evaluate(&mut page, "ps[1].catch(() => {}); 0", None);
    assert!(page.came("Runtime.exceptionRevoked"));
}

/// A client gone takes its session out of the renderer too: what it
/// turned on there (`Runtime`, which keeps every error's stack) is off.
#[test]
fn a_client_gone_takes_its_session_with_it() {
    let machopo = Machopo::start();
    let (mut first, _) = page(&machopo);
    first.call(None, "Runtime.enable", json!({}));
    drop(first);
    let (mut second, _) = page(&machopo);
    let throw = json!({ "expression": "throw new Error('x')" });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let thrown = second.call(None, "Runtime.evaluate", throw.clone());
        if thrown["result"]["exceptionDetails"]
            .get("stackTrace")
            .is_none()
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the first session is still on: {thrown}"
        );
    }
}

/// Text goes to the inspector and back as UTF-8, whatever it is.
#[test]
fn text_is_utf8_both_ways() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    assert_eq!(evaluate(&mut page, "'é中😀'", None), "é中😀");
    assert_eq!(evaluate(&mut page, "'é中😀'.length", None), 4);
}

/// A command the renderer hasn't answered when it goes is answered that
/// it has gone; one it answered, not again.
#[test]
fn a_command_left_waiting_is_answered_when_the_renderer_goes() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    let done = page.send(None, "Runtime.evaluate", json!({ "expression": "1" }));
    page.answer(done);
    // Answered once a promise that never settles does.
    let waiting = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "new Promise(() => {})", "awaitPromise": true }),
    );
    page.call(None, "Page.navigate", json!({ "url": "about:crash" }));
    let answer = page.answer(waiting);
    assert_eq!(answer["error"]["code"], -32000, "{answer}");
    assert_eq!(answer["error"]["message"], "the tab's renderer has gone");
    // What came with it has come by the next answer.
    page.call(None, "Runtime.evaluate", json!({ "expression": "1" }));
    assert!(!page.answered(done));
}

/// `Debugger.evaluateOnCallFrame`'s value on the paused frame `frame`.
fn on_frame(page: &mut Client, frame: &Value, expression: &str) -> Value {
    let params = json!({ "callFrameId": frame, "expression": expression, "returnByValue": true });
    let answer = page.call(None, "Debugger.evaluateOnCallFrame", params);
    answer["result"]["result"]["value"].clone()
}

/// `expression`, evaluated in the tab until it is true, for up to 10 s.
fn until_true(page: &mut Client, expression: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while evaluate(page, expression, None) != true {
        assert!(Instant::now() < deadline, "{expression} never came true");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A `debugger` statement pauses the page until DevTools resumes it: its
/// other tasks wait (a timer doesn't tick), DevTools' commands go on
/// (evaluating on the paused frame), and what paused finishes after.
#[test]
fn a_debugger_statement_pauses_the_page_until_resumed() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html><script>window.ticks = 0; setInterval(() => ticks++, 5);</script>",
        ),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Debugger.enable", json!({}));
    let expression = json!({ "expression": "debugger; 1 + 1", "returnByValue": true });
    let evaluated = page.send(None, "Runtime.evaluate", expression);
    let paused = page.event("Debugger.paused")["params"].clone();
    assert_eq!(paused["reason"], "other", "{paused}");
    let frame = paused["callFrames"][0]["callFrameId"].clone();
    let ticks = on_frame(&mut page, &frame, "ticks");
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(
        on_frame(&mut page, &frame, "ticks"),
        ticks,
        "a timer ran while paused"
    );
    assert_eq!(on_frame(&mut page, &frame, "6 * 7"), 42);
    page.call(None, "Debugger.resume", json!({}));
    page.event("Debugger.resumed");
    assert_eq!(page.answer(evaluated)["result"]["result"]["value"], 2);
    until_true(&mut page, &format!("ticks > {ticks}"));
}

/// A breakpoint by URL and line pauses there; stepping goes on a line;
/// DevTools reads the script's source.
#[test]
fn a_breakpoint_pauses_and_stepping_goes_on() {
    let html = "<!DOCTYPE html>\n<script>\nfunction add(a, b) {\n  const sum = a + b;\n  return sum;\n}\n</script>";
    let server = server(&[("/", Route::ok("text/html", html))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    let url = server.url("/");
    load(&mut page, &url);
    page.call(None, "Debugger.enable", json!({}));
    let set = page.call(
        None,
        "Debugger.setBreakpointByUrl",
        json!({ "url": url, "lineNumber": 3 }),
    );
    let breakpoint = set["result"]["breakpointId"].clone();
    assert_eq!(set["result"]["locations"][0]["lineNumber"], 3, "{set}");
    let script = set["result"]["locations"][0]["scriptId"].clone();
    let source = page.call(
        None,
        "Debugger.getScriptSource",
        json!({ "scriptId": script }),
    );
    assert!(
        source["result"]["scriptSource"]
            .as_str()
            .is_some_and(|source| source.contains("return sum;")),
        "{source}"
    );

    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "add(1, 2)", "returnByValue": true }),
    );
    let paused = page.event("Debugger.paused")["params"].clone();
    assert_eq!(paused["hitBreakpoints"][0], breakpoint, "{paused}");
    let top = &paused["callFrames"][0];
    assert_eq!(
        (&top["functionName"], &top["location"]["lineNumber"]),
        (&json!("add"), &json!(3))
    );
    page.call(None, "Debugger.stepOver", json!({}));
    let stepped = page.event("Debugger.paused")["params"].clone();
    let top = &stepped["callFrames"][0];
    assert_eq!(top["location"]["lineNumber"], 4, "{stepped}");
    assert_eq!(on_frame(&mut page, &top["callFrameId"].clone(), "sum"), 3);
    page.call(None, "Debugger.resume", json!({}));
    assert_eq!(page.answer(evaluated)["result"]["result"]["value"], 3);
}

/// `Debugger.pause` pauses script that runs on and on (it interrupts it);
/// terminated, it ends, and the page runs script after.
#[test]
fn a_busy_script_is_paused_and_terminated() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Debugger.enable", json!({}));
    let busy = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "let n = 0; for (;;) n++" }),
    );
    std::thread::sleep(Duration::from_millis(100));
    page.call(None, "Debugger.pause", json!({}));
    page.event("Debugger.paused");
    // Answered once script ends.
    let terminated = page.send(None, "Runtime.terminateExecution", json!({}));
    page.call(None, "Debugger.resume", json!({}));
    let busy = page.answer(busy);
    assert_eq!(
        busy["error"]["message"], "Execution was terminated",
        "{busy}"
    );
    assert!(page.answer(terminated).get("result").is_some());
    assert_eq!(evaluate(&mut page, "1 + 2", None), 3);
}

/// `Runtime.evaluate` doesn't interrupt script that runs: it waits for it
/// to end, as Chrome's does (only `Debugger.pause` and the like
/// interrupt).
#[test]
fn evaluating_waits_for_script_that_runs() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    let busy = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "const end = Date.now() + 300; while (Date.now() < end); window.done = true" }),
    );
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(evaluate(&mut page, "window.done === true", None), true);
    assert!(page.answer(busy).get("result").is_some());
}

/// A client gone while it has the page paused stops debugging it: the page
/// goes on.
#[test]
fn a_client_gone_while_paused_lets_the_page_go_on() {
    let machopo = Machopo::start();
    let (mut debugger, _) = page(&machopo);
    debugger.call(None, "Debugger.enable", json!({}));
    evaluate(
        &mut debugger,
        "setTimeout(() => { debugger; window.after = true; }, 0); 0",
        None,
    );
    debugger.event("Debugger.paused");
    drop(debugger);
    let (mut other, _) = page(&machopo);
    until_true(&mut other, "window.after === true");
}

/// A client that connects while the page is paused is answered once the
/// page goes on: its commands wait, none is lost.
#[test]
fn a_client_attached_while_paused_is_answered_after() {
    let machopo = Machopo::start();
    let (mut debugger, _) = page(&machopo);
    debugger.call(None, "Debugger.enable", json!({}));
    evaluate(&mut debugger, "setTimeout(() => { debugger; }, 0); 0", None);
    debugger.event("Debugger.paused");
    let (mut other, _) = page(&machopo);
    let asked = other.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "40 + 2", "returnByValue": true }),
    );
    std::thread::sleep(Duration::from_millis(100));
    debugger.call(None, "Debugger.resume", json!({}));
    assert_eq!(other.answer(asked)["result"]["result"]["value"], 42);
}

/// A tab closed while paused ends with its renderer, the script paused
/// terminated (it would run on); the browser and its other tabs go on.
#[test]
fn a_paused_tab_closes() {
    let machopo = Machopo::start();
    let (mut debugger, target) = page(&machopo);
    let (status, _) = machopo.http("PUT", "/json/new?about:blank");
    assert_eq!(status, 200);
    debugger.call(None, "Debugger.enable", json!({}));
    evaluate(
        &mut debugger,
        "setTimeout(() => { debugger; for (;;) {} }, 0); 0",
        None,
    );
    debugger.event("Debugger.paused");
    let (status, _) = machopo.http("GET", &format!("/json/close/{target}"));
    assert_eq!(status, 200);
    assert!(debugger.closes());
    assert_eq!(machopo.targets().len(), 1);
}

/// A heap snapshot has the DOM's objects (the elise heap's, by their types'
/// names) as well as JS's: the divs script made and never inserted are
/// detached, the document attached.
#[test]
fn a_heap_snapshot_has_the_dom() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    evaluate(
        &mut page,
        "window.kept = Array.from({length: 100}, () => document.createElement('div')); 0",
        None,
    );
    let taken = page.send(None, "HeapProfiler.takeHeapSnapshot", json!({}));
    let mut snapshot = String::new();
    loop {
        let message = page.read().expect("the snapshot");
        if message["id"] == taken {
            break;
        }
        if message["method"] == "HeapProfiler.addHeapSnapshotChunk" {
            snapshot.push_str(message["params"]["chunk"].as_str().unwrap());
        }
    }
    let snapshot: Value = serde_json::from_str(&snapshot).unwrap();
    let fields = snapshot["snapshot"]["meta"]["node_fields"]
        .as_array()
        .unwrap();
    let width = fields.len();
    let name = fields.iter().position(|field| field == "name").unwrap();
    let detachedness = fields
        .iter()
        .position(|field| field == "detachedness")
        .unwrap();
    let strings = snapshot["strings"].as_array().unwrap();
    let nodes: Vec<(&str, u64)> = snapshot["nodes"]
        .as_array()
        .unwrap()
        .chunks(width)
        .map(|node| {
            (
                strings[node[name].as_u64().unwrap() as usize]
                    .as_str()
                    .unwrap(),
                node[detachedness].as_u64().unwrap(),
            )
        })
        .collect();
    let dom = |state: u64| {
        nodes
            .iter()
            .filter(|&&(name, detachedness)| name == "Node" && detachedness == state)
            .count()
    };
    // Detached (2): the divs; attached (1): the document and its tree.
    assert!(dom(2) >= 100, "{} detached DOM nodes", dom(2));
    assert!(dom(1) >= 1, "the document");
    assert!(
        nodes.iter().any(|&(name, _)| name == "HTMLDivElement"),
        "the wrappers' interface"
    );
}

/// The page's frames, as `Page.getResourceTree` and `getFrameTree` give
/// them: the main frame (the target's id, its document's URL, origin and
/// MIME type, as secure as an HTTP page of the machine itself is) and its
/// iframes' frames below it, by their `name`. Its document loads no
/// subresources: none listed (an iframe's document is a frame, not one).
#[test]
fn the_frame_tree_has_every_frame() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><iframe name=first src='/child'></iframe>\
                 <iframe src='data:text/html,data'></iframe>",
            ),
        ),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>child")),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    let url = server.url("/");
    load(&mut page, &url);
    let tree = page.call(None, "Page.getResourceTree", json!({}))["result"]["frameTree"].clone();
    let main = &tree["frame"];
    assert_eq!(main["id"], target.as_str(), "{tree}");
    assert_eq!(main["url"], url.as_str());
    assert_eq!(main["securityOrigin"], url.trim_end_matches('/'));
    assert_eq!(main["mimeType"], "text/html");
    assert_eq!(main["secureContextType"], "SecureLocalhost");
    assert!(main.get("parentId").is_none());
    assert!(main.get("name").is_none(), "{main}");
    assert_eq!(main["securityOriginDetails"]["isLocalhost"], true);
    assert_eq!(tree["resources"], json!([]));
    let children = tree["childFrames"].as_array().unwrap();
    assert_eq!(children.len(), 2, "{tree}");
    for child in children {
        assert_eq!(child["frame"]["parentId"], target.as_str());
    }
    let named = children
        .iter()
        .find(|child| child["frame"]["name"] == "first")
        .unwrap_or_else(|| panic!("{tree}"));
    assert_eq!(named["frame"]["url"], server.url("/child").as_str());
    let data = children
        .iter()
        .find(|child| child["frame"]["securityOrigin"] == "://")
        .unwrap_or_else(|| panic!("{tree}"));
    // `data:` is potentially trustworthy: as its parent.
    assert_eq!(data["frame"]["secureContextType"], "SecureLocalhost");
    assert_eq!(data["frame"]["name"], "");
    let frames = page.call(None, "Page.getFrameTree", json!({}))["result"]["frameTree"].clone();
    assert_eq!(frames["frame"], tree["frame"]);
    assert!(frames.get("resources").is_none(), "{frames}");
}

/// `Page.getResourceContent` gives a frame's document's markup as it came;
/// what it didn't load isn't there.
#[test]
fn a_documents_markup_is_its_content() {
    let html = "<!DOCTYPE html>\n<title>é</title><script>let x = 1;</script>";
    let server = server(&[("/", Route::ok("text/html", html))]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    let url = server.url("/");
    load(&mut page, &url);
    let content = |page: &mut Client, frame: &str, url: &str| {
        page.call(
            None,
            "Page.getResourceContent",
            json!({ "frameId": frame, "url": url }),
        )
    };
    // Only with `Page` on, as Chrome's.
    let off = content(&mut page, &target, &url);
    assert_eq!(off["error"]["message"], "Agent is not enabled.", "{off}");
    page.call(None, "Page.enable", json!({}));
    let got = content(&mut page, &target, &format!("{url}#here"));
    assert_eq!(got["result"]["content"], html, "{got}");
    assert_eq!(got["result"]["base64Encoded"], false);
    let other = content(&mut page, &target, &server.url("/style.css"));
    assert_eq!(
        other["error"]["message"],
        "No resource with given URL found"
    );
    let nowhere = content(&mut page, "nowhere", &url);
    assert_eq!(nowhere["error"]["message"], "No frame for given id found");
}

/// `Page.getResourceTree` lists the subresources each document loaded
/// (not its `fetch()`es), whether or not DevTools was there, a failed one
/// as failed; `getResourceContent` gives those `Network` kept, loaded
/// since it was turned on, as text or base64.
#[test]
fn subresources_are_listed_with_their_content() {
    let refused = format!("http://127.0.0.1:{}/gone.png", unused_port());
    let html = format!(
        "<!DOCTYPE html><link rel=stylesheet href=/s.css><img src=/i.png>\
         <img src={refused:?}><script src=/a.js></script>"
    );
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        ("/s.css", Route::ok("text/css", "p { color: green }")),
        ("/i.png", Route::ok("image/png", vec![0xff, 0x00, 0x10])),
        ("/a.js", Route::ok("text/javascript", "fetch('/data.json')")),
        ("/data.json", Route::ok("application/json", "{}")),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    let url = server.url("/");
    load(&mut page, &url);
    let tree = page.call(None, "Page.getResourceTree", json!({}))["result"]["frameTree"].clone();
    let resources = tree["resources"].as_array().unwrap();
    let listed = |path: &str| {
        resources
            .iter()
            .find(|resource| {
                resource["url"]
                    .as_str()
                    .is_some_and(|url| url.ends_with(path))
            })
            .unwrap_or_else(|| panic!("no {path}: {tree}"))
    };
    let css = listed("/s.css");
    assert_eq!(css["type"], "Stylesheet");
    assert_eq!(css["mimeType"], "text/css");
    assert_eq!(css["contentSize"], 18);
    assert!(css.get("failed").is_none(), "{css}");
    assert_eq!(listed("/i.png")["type"], "Image");
    assert_eq!(listed("/a.js")["type"], "Script");
    let gone = listed("/gone.png");
    assert_eq!(
        (&gone["type"], &gone["failed"]),
        (&json!("Image"), &json!(true))
    );
    assert_eq!(resources.len(), 4, "{tree}");

    page.call(None, "Page.enable", json!({}));
    let content = |page: &mut Client, path: &str| {
        page.call(
            None,
            "Page.getResourceContent",
            json!({ "frameId": target, "url": server.url(path) }),
        )
    };
    // Loaded before `Network` was on: not kept.
    let before = content(&mut page, "/s.css");
    assert_eq!(
        before["error"]["message"], "No resource with given URL found",
        "{before}"
    );
    page.call(None, "Network.enable", json!({}));
    load(&mut page, &url);
    let css = content(&mut page, "/s.css")["result"].clone();
    assert_eq!(
        css,
        json!({ "content": "p { color: green }", "base64Encoded": false })
    );
    let png = content(&mut page, "/i.png")["result"].clone();
    assert_eq!(png, json!({ "content": "/wAQ", "base64Encoded": true }));
}

/// With `Page.enable`, frames attached, navigated and detached are told:
/// an iframe's new frame and its document; the iframe gone; the main
/// frame's new document before its realm, its old frames gone first.
/// `Page.disable` stops them.
#[test]
fn frames_coming_and_going_are_told() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>top")),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>child")),
        ("/next", Route::ok("text/html", "<!DOCTYPE html>next")),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    load(&mut page, &server.url("/"));
    let enabled = page.call(None, "Page.enable", json!({}));
    assert!(enabled.get("result").is_some(), "{enabled}");
    evaluate(
        &mut page,
        &format!(
            "const f = document.createElement('iframe'); f.src = '{}'; document.body.append(f); 0",
            server.url("/child")
        ),
        None,
    );
    let attached = page.event("Page.frameAttached")["params"].clone();
    assert_eq!(attached["parentFrameId"], target.as_str(), "{attached}");
    let child = attached["frameId"].clone();
    // Its document, not the initial `about:blank` before it.
    let navigated = page.event("Page.frameNavigated")["params"]["frame"].clone();
    assert_eq!(
        navigated["url"],
        server.url("/child").as_str(),
        "{navigated}"
    );
    assert_eq!(navigated["id"], child, "{navigated}");
    evaluate(
        &mut page,
        "document.querySelector('iframe').remove(); 0",
        None,
    );
    let detached = page.event("Page.frameDetached")["params"].clone();
    assert_eq!(
        (&detached["frameId"], &detached["reason"]),
        (&child, &json!("remove"))
    );

    page.call(None, "Runtime.enable", json!({}));
    page.event("Runtime.executionContextCreated");
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/next") }));
    let loader = navigated["result"]["loaderId"].clone();
    // In order: the main frame's new document, then its realm.
    let (frame, context) = loop {
        let message = page.read().expect("events");
        match message["method"].as_str() {
            Some("Page.frameNavigated") => {
                break (message, page.event("Runtime.executionContextCreated"));
            }
            Some("Runtime.executionContextCreated") => panic!("the realm came first: {message}"),
            _ => {}
        }
    };
    assert_eq!(frame["params"]["frame"]["id"], target.as_str(), "{frame}");
    assert_eq!(frame["params"]["frame"]["loaderId"], loader, "{frame}");
    assert_eq!(
        context["params"]["context"]["auxData"]["frameId"],
        target.as_str()
    );
    // The browser's own event of it, with the same loader.
    let started = page.event("Page.frameStartedNavigating");
    assert_eq!(started["params"]["loaderId"], loader, "{started}");

    page.call(None, "Page.disable", json!({}));
    load_quietly(&mut page, &server.url("/"));
    // The renderer's and the browser's alike.
    assert!(!page.came("Page.frameNavigated"));
    assert!(!page.came("Page.frameStartedNavigating"));
}

/// Navigates to `url` without lifecycle events and waits until its document
/// is the tab's.
fn load_quietly(page: &mut Client, url: &str) {
    page.call(None, "Page.navigate", json!({ "url": url }));
    until_true(
        page,
        &format!("location.href === {url:?} && document.readyState === 'complete'"),
    );
}

/// A client's first command, as it connects to a tab no session had yet,
/// has the frames already.
#[test]
fn the_frames_are_there_from_the_first_command() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>first"))]);
    let machopo = Machopo::start();
    let (status, body) = machopo.http("PUT", &format!("/json/new?{}", server.url("/")));
    assert_eq!(status, 200, "{body}");
    let opened: Value = serde_json::from_str(&body).unwrap();
    let target = opened["id"].as_str().unwrap();
    let url = format!("ws://127.0.0.1:{}/devtools/page/{target}", machopo.port);
    let mut fresh = machopo.connect(&url, None).expect("connects");
    let tree = fresh.call(None, "Page.getResourceTree", json!({}));
    assert_eq!(tree["result"]["frameTree"]["frame"]["id"], target, "{tree}");
}

/// Before its first document commits (its response is slow), the tab's
/// main frame is there with none, as Chrome 152 has it (its initial empty
/// document: no URL, an opaque origin); a session that turned `Page` on
/// hears of the document as it commits.
#[test]
fn the_main_frame_before_its_first_document() {
    let slow = Route::ok("text/html", "<!DOCTYPE html>slow").with_delay(Duration::from_secs(3));
    let server = server(&[("/", slow)]);
    let machopo = Machopo::start();
    let (status, body) = machopo.http("PUT", &format!("/json/new?{}", server.url("/")));
    assert_eq!(status, 200, "{body}");
    let opened: Value = serde_json::from_str(&body).unwrap();
    let target = opened["id"].as_str().unwrap();
    let url = format!("ws://127.0.0.1:{}/devtools/page/{target}", machopo.port);
    let mut fresh = machopo.connect(&url, None).expect("connects");
    let tree = fresh.call(None, "Page.getResourceTree", json!({}));
    let frame = &tree["result"]["frameTree"]["frame"];
    assert_eq!(
        [
            &frame["id"],
            &frame["url"],
            &frame["securityOrigin"],
            &frame["mimeType"],
            &frame["secureContextType"]
        ],
        [
            &json!(target),
            &json!(""),
            &json!("://"),
            &json!("text/html"),
            &json!("InsecureScheme")
        ],
        "{tree}"
    );
    assert_eq!(
        tree["result"]["frameTree"]["resources"],
        json!([]),
        "{tree}"
    );
    fresh.call(None, "Page.enable", json!({}));
    let navigated = fresh.event("Page.frameNavigated")["params"]["frame"].clone();
    assert_eq!(
        (&navigated["id"], &navigated["url"]),
        (&json!(target), &json!(server.url("/"))),
        "{navigated}"
    );
}

/// While script is paused, the frame tree and the documents' markup are
/// still there (Sources opens the HTML a breakpoint is in).
#[test]
fn the_frames_are_there_while_paused() {
    let html = "<!DOCTYPE html><script>window.ready = true</script>";
    let server = server(&[("/", Route::ok("text/html", html))]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    let url = server.url("/");
    load(&mut page, &url);
    page.call(None, "Debugger.enable", json!({}));
    page.call(None, "Page.enable", json!({}));
    let paused = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "debugger; 1" }),
    );
    page.event("Debugger.paused");
    let tree = page.call(None, "Page.getResourceTree", json!({}));
    assert_eq!(
        tree["result"]["frameTree"]["frame"]["id"],
        target.as_str(),
        "{tree}"
    );
    let content = page.call(
        None,
        "Page.getResourceContent",
        json!({ "frameId": target, "url": url }),
    );
    assert_eq!(content["result"]["content"], html, "{content}");
    page.call(None, "Debugger.resume", json!({}));
    page.answer(paused);
}
