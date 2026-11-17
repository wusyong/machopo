//! Dedicated workers in a whole browser, their scripts `data:` URLs': each
//! on a thread of its own, its messages and errors coming back as tasks of
//! the page's; `terminate()` stops one busy forever, and nothing of it
//! comes after; `close()` ends one after its task; a script that can't be
//! had is an `error` at its `Worker`; the renderer ends with a worker still
//! busy.

use std::collections::HashMap;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use shell::headless;

/// What the page `body` (a script, with `worker(source, options)` making a
/// worker of a `data:` URL of `source`) reports.
fn report(body: &str) -> String {
    let page = format!(
        "<!DOCTYPE html><script>\n\
         const worker = (source, options) => \
           new Worker('data:text/javascript,' + encodeURIComponent(source), options);\n\
         const report = text => testRunner.reportHarnessResults(String(text));\n\
         {body}\n</script>"
    );
    let server = TestServer::start(HashMap::from([(
        "/".to_string(),
        Route::ok("text/html", page),
    )]))
    .expect("test server starts");
    headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports")
}

/// A message each way, cloned; the worker has its name, and none of a
/// window's (`document`, `window`).
#[test]
fn a_page_and_its_worker_talk() {
    assert_eq!(
        report(
            "const w = worker(`onmessage = e => postMessage({ \
               n: e.data.n * 2, name, window: typeof window, document: typeof document });`, \
               { name: 'fib' }); \
             const sent = { n: 21 }; \
             w.onmessage = e => report([e.data.n, e.data.name, e.data.window, e.data.document, \
               e.data === sent].join(' ')); \
             w.postMessage(sent); sent.n = 0;"
        ),
        "42 fib undefined undefined false"
    );
}

/// What the worker throws is an `error` at its `Worker`, its message and
/// line; at the window too unless the `Worker`'s listener cancels it.
#[test]
fn a_worker_s_error_reaches_its_worker_and_the_window() {
    assert_eq!(
        report(
            "const got = []; \
             onerror = message => { got.push('window ' + message); }; \
             const w = worker(`setTimeout(() => { throw new Error('later'); }, 0); \
               throw new Error('first');`); \
             w.onerror = e => { got.push('worker ' + e.message + ' ' + e.lineno + ' ' + e.error); \
               if (e.message.includes('first')) e.preventDefault(); }; \
             setTimeout(() => report(got.join(' | ')), 300);"
        ),
        "worker Uncaught Error: first 1 null | worker Uncaught Error: later 1 null | \
         window Uncaught Error: later"
    );
}

/// A worker busy forever is stopped by `terminate()`: nothing it posted
/// comes after, and the page goes on.
#[test]
fn a_busy_worker_is_terminated() {
    assert_eq!(
        report(
            "let got = 0; \
             const w = worker(`postMessage('started'); for (;;) postMessage('busy');`); \
             w.onmessage = e => { \
               if (e.data !== 'started') { got++; return; } \
               w.terminate(); const at = got; \
               setTimeout(() => report(got === at ? 'stopped' : 'went on: ' + (got - at)), 300); };"
        ),
        "stopped"
    );
}

/// `close()`: what it posted in that task comes, nothing after (its timer
/// doesn't run).
#[test]
fn a_worker_closes_itself() {
    assert_eq!(
        report(
            "const got = []; \
             const w = worker(`postMessage('a'); close(); postMessage('b'); \
               setTimeout(() => postMessage('c'), 0);`); \
             w.onmessage = e => got.push(e.data); \
             setTimeout(() => report(got.join()), 300);"
        ),
        "a,b"
    );
}

/// A script that isn't a `data:` URL's isn't fetched yet: an `error`
/// event (an `Event`, not an `ErrorEvent`) at its `Worker`, and no more.
/// Made after `load`: no task of the page's comes after it to send what it
/// reported but the worker's.
#[test]
fn a_worker_whose_script_can_t_be_had_errs() {
    assert_eq!(
        report(
            "onload = () => { const w = new Worker('/worker.js'); \
               w.onerror = e => report(e.constructor.name + ' ' + e.type + ' ' + e.bubbles); };"
        ),
        "Event error false"
    );
}

/// The page done while its worker is busy forever: the renderer stops it
/// and ends (the test would time out otherwise).
#[test]
fn a_busy_worker_ends_with_the_renderer() {
    assert_eq!(
        report(
            "const w = worker(`postMessage('started'); for (;;);`); \
             w.onmessage = () => report('started');"
        ),
        "started"
    );
}
