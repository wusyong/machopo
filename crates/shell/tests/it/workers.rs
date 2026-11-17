//! Dedicated workers in a whole browser: each on a thread of its own, its
//! messages and errors coming back as tasks of the page's; `terminate()`
//! stops one busy forever, and nothing of it comes after; `close()` ends
//! one after its task; a script that can't be had is an `error` at its
//! `Worker`; the renderer ends with a worker still busy. Their scripts the
//! browser loads, `importScripts()`'s too: same-origin, redirects followed,
//! lax of MIME types as Chrome is.

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

/// What the page `body` at `/` reports, the server having the routes
/// `routes` gives too; another origin's server, of `other_routes`, the
/// page knows as `OTHER`, as `routes` is given it (its URL, no `/` at its
/// end).
fn report_served(
    body: &str,
    routes: impl FnOnce(&str) -> Vec<(&'static str, Route)>,
    other_routes: &[(&str, Route)],
) -> String {
    run_served(body, routes, other_routes).0
}

/// `report_served`, and the page's server, to ask what it was asked.
fn run_served(
    body: &str,
    routes: impl FnOnce(&str) -> Vec<(&'static str, Route)>,
    other_routes: &[(&str, Route)],
) -> (String, TestServer) {
    let other = serve(other_routes);
    let other = other.url("");
    let other = other.trim_end_matches('/');
    let page = format!(
        "<!DOCTYPE html><script>\n\
         const OTHER = '{other}';\n\
         const report = text => testRunner.reportHarnessResults(String(text));\n\
         {body}\n</script>",
    );
    let mut routes = routes(other);
    routes.push(("/", Route::ok("text/html", page)));
    let server = serve(&routes);
    let report = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    (report, server)
}

fn serve(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| ((*path).to_owned(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn js(source: &str) -> Route {
    Route::ok("text/javascript", source)
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

/// A script that can't be had (none at its URL): an `error` event (an
/// `Event`, not an `ErrorEvent`) at its `Worker`, and no more. Made after
/// `load`: no task of the page's comes after it to send what it reported
/// but the worker's.
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

/// Its script loaded, a redirect followed: its URL after it is the base
/// of its imports (`moved.js` beside it, which an import follows to
/// `lib.js`); another origin's import runs too (`no-cors`); what the page
/// posted before it ran comes after.
#[test]
fn a_worker_s_script_and_its_imports_are_loaded() {
    assert_eq!(
        report_served(
            "const w = new Worker('/start.js'); \
             w.onmessage = e => report(e.data); w.postMessage('early');",
            |other| {
                vec![
                    ("/start.js", Route::redirect(302, "/w/worker.js")),
                    (
                        "/w/worker.js",
                        js(&format!(
                            "importScripts('moved.js', '{other}/other.js'); \
                             onmessage = e => postMessage([lib, other, e.data].join(' '));"
                        )),
                    ),
                    ("/w/moved.js", Route::redirect(302, "/w/lib.js")),
                    ("/w/lib.js", js("var lib = 'lib';")),
                ]
            },
            &[("/other.js", js("var other = 'other';"))],
        ),
        "lib other early"
    );
}

/// Its script isn't had from another origin (asked for, or redirected
/// to), with a status that isn't ok, or of a MIME type no script is
/// (`image/`, `text/csv`): an `error` at its `Worker`. Another MIME type
/// runs (`text/plain`, as Chrome lets it), as any `data:` URL's does.
#[test]
fn a_worker_s_script_is_had_as_chrome_has_it() {
    let ran = "postMessage('ran');";
    assert_eq!(
        report_served(
            "const got = []; \
             const tried = (name, url) => { const w = new Worker(url); \
               w.onmessage = e => done(name + ' ' + e.data); w.onerror = () => done(name + ' error'); }; \
             const done = what => { got.push(what); if (got.length === 7) report(got.sort().join(', ')); }; \
             tried('other', OTHER + '/w.js'); tried('redirected', '/to-other.js'); \
             tried('missing', '/missing.js'); tried('image', '/image.js'); tried('csv', '/csv.js'); \
             tried('plain', '/plain.js'); tried('data', 'data:image/png,postMessage(\"ran\")');",
            |other| {
                vec![
                    ("/to-other.js", Route::redirect(302, &format!("{other}/w.js"))),
                    ("/missing.js", Route::new(404, ran)),
                    ("/image.js", Route::ok("image/png", ran)),
                    ("/csv.js", Route::ok("text/csv", ran)),
                    ("/plain.js", Route::ok("text/plain", ran)),
                ]
            },
            &[("/w.js", js(ran))],
        ),
        "csv error, data ran, image error, missing error, other error, plain ran, redirected error"
    );
}

/// `importScripts()` of a script that can't be had (a status that isn't
/// ok, a MIME type no script is) throws a `NetworkError`, as one of
/// another origin's that throws does (its errors muted); what one of its
/// own throws, it throws.
#[test]
fn import_scripts_throws_as_html_says() {
    assert_eq!(
        report_served(
            "const w = new Worker('/worker.js'); w.onmessage = e => report(e.data);",
            |other| {
                vec![
                    (
                        "/worker.js",
                        js(&format!(
                            "const tried = url => {{ try {{ importScripts(url); return 'ran'; }} \
                               catch (e) {{ return e === self ? 'self' : e.name; }} }}; \
                             postMessage(['/missing.js', '/image.js', '/throws.js', \
                               '{other}/throws.js', '/plain.js'].map(tried).join());"
                        )),
                    ),
                    ("/missing.js", Route::new(404, "")),
                    ("/image.js", Route::ok("image/png", "")),
                    ("/throws.js", js("throw self;")),
                    ("/plain.js", Route::ok("text/plain", "")),
                ]
            },
            &[("/throws.js", js("throw self;"))],
        ),
        "NetworkError,NetworkError,self,NetworkError,ran"
    );
}

/// While `importScripts()` waits for its script, what the page posts waits
/// for the worker's task to end.
#[test]
fn a_worker_s_messages_wait_for_its_import() {
    assert_eq!(
        report_served(
            "const got = []; const w = new Worker('/worker.js'); \
             w.onmessage = e => { got.push(e.data); \
               if (e.data === 'importing') w.postMessage('meanwhile'); \
               if (got.length === 3) report(got.join()); };",
            |_| {
                vec![
                    (
                        "/worker.js",
                        js("onmessage = e => postMessage('got ' + e.data); \
                            postMessage('importing'); importScripts('/slow.js'); \
                            postMessage('imported');"),
                    ),
                    ("/slow.js", js("").with_delay(Duration::from_millis(300))),
                ]
            },
            &[],
        ),
        "importing,imported,got meanwhile"
    );
}

/// A worker terminated while `importScripts()` waits: nothing more of it,
/// and its load is closed (the server, answering after the worker went
/// but before the page is done, finds it gone). Terminated a while after
/// it said it imports: its request has gone to the browser by then (one
/// after it went the browser refuses).
#[test]
fn a_worker_waiting_for_its_import_is_terminated() {
    let (report, server) = run_served(
        "const got = []; const w = new Worker('/worker.js'); \
         w.onmessage = e => { got.push(e.data); setTimeout(() => w.terminate(), 200); \
           setTimeout(() => report(got.join()), 2000); };",
        |_| {
            vec![
                (
                    "/worker.js",
                    js("postMessage('importing'); \
                        try { importScripts('/slow.js'); } catch (e) { postMessage(e.name); } \
                        postMessage('imported');"),
                ),
                ("/slow.js", js("").with_delay(Duration::from_secs(1))),
            ]
        },
        &[],
    );
    assert_eq!(report, "importing");
    assert!(server.wait_for_abandoned("/slow.js", Duration::from_secs(2)));
}
/// A worker terminated before its script came: its load is closed (the
/// worker isn't there to close it: the browser does, told it went). A
/// while after it was made, for its request to have reached the server.
#[test]
fn a_worker_terminated_before_its_script_came_has_its_load_closed() {
    let (report, server) = run_served(
        "const w = new Worker('/slow.js'); w.onmessage = () => report('ran'); \
         setTimeout(() => w.terminate(), 200); setTimeout(() => report('done'), 2000);",
        |_| vec![("/slow.js", js("postMessage(1);").with_delay(Duration::from_secs(1)))],
        &[],
    );
    assert_eq!(report, "done");
    assert!(server.wait_for_abandoned("/slow.js", Duration::from_secs(2)));
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
