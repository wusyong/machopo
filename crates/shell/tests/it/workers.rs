//! Dedicated workers in a whole browser: each on a thread of its own, its
//! messages and errors coming back as tasks of the page's; `terminate()`
//! stops one busy forever, and nothing of it comes after; `close()` ends
//! one after its task; a script that can't be had is an `error` at its
//! `Worker`; the renderer ends with a worker still busy. Their scripts the
//! browser loads, `importScripts()`'s too: same-origin, redirects followed,
//! lax of MIME types as Chrome is. Message ports go to and from workers,
//! the two of a channel then on two threads.

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

/// A worker's time origin is when its page made it, not when its script
/// came (300 ms later); its `navigator` says the page's user agent; its
/// `location` is its script's URL.
#[test]
fn a_worker_s_time_origin_agent_and_location() {
    assert_eq!(
        report_served(
            "const made = performance.timeOrigin + performance.now(); \
             const w = new Worker('/slow.js'); \
             w.onmessage = e => report([Math.abs(e.data[0] - made) < 150, \
               e.data[1] === navigator.userAgent, e.data[2] === location.origin + '/slow.js'].join());",
            |_| {
                vec![(
                    "/slow.js",
                    js("postMessage([performance.timeOrigin, navigator.userAgent, location.href]);")
                        .with_delay(Duration::from_millis(300)),
                )]
            },
            &[],
        ),
        "true,true,true"
    );
}

/// What the worker throws is an `error` at its `Worker`, its message and
/// line; at the window too unless the `Worker`'s listener cancels it, its
/// value null there (it comes without it).
#[test]
fn a_worker_s_error_reaches_its_worker_and_the_window() {
    assert_eq!(
        report(
            "const got = []; \
             onerror = (message, file, line, column, error) => { \
               got.push('window ' + message + ' ' + error); }; \
             const w = worker(`setTimeout(() => { throw new Error('later'); }, 0); \
               throw new Error('first');`); \
             w.onerror = e => { got.push('worker ' + e.message + ' ' + e.lineno + ' ' + e.error); \
               if (e.message.includes('first')) e.preventDefault(); }; \
             setTimeout(() => report(got.join(' | ')), 300);"
        ),
        "worker Uncaught Error: first 1 null | worker Uncaught Error: later 1 null | \
         window Uncaught Error: later null"
    );
}

/// What the worker posted before it threw comes before its error (each a
/// task of the page's, in order).
#[test]
fn a_worker_s_message_comes_before_its_error() {
    assert_eq!(
        report(
            "const got = []; const w = worker(`postMessage('posted'); throw new Error('thrown');`); \
             w.onmessage = e => got.push(e.data); \
             w.onerror = e => { e.preventDefault(); got.push('error'); report(got.join()); };"
        ),
        "posted,error"
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

/// A script that doesn't parse: a plain `error` at its `Worker` (not an
/// `ErrorEvent`), none at the window, and nothing of it runs.
#[test]
fn a_worker_whose_script_doesn_t_parse_errs() {
    assert_eq!(
        report(
            "const got = []; onerror = () => got.push('window'); \
             const w = worker('postMessage(1); ('); w.onmessage = () => got.push('ran'); \
             w.onerror = e => { got.push(e.constructor.name); \
               setTimeout(() => report(got.join()), 300); };"
        ),
        "Event"
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
/// (`image/`, `text/csv`), or not JavaScript's under `nosniff`: an `error`
/// at its `Worker`. Another MIME type runs (`text/plain`, as Chrome lets
/// it), as any `data:` URL's does.
#[test]
fn a_worker_s_script_is_had_as_chrome_has_it() {
    let ran = "postMessage('ran');";
    assert_eq!(
        report_served(
            "const got = []; \
             const tried = (name, url) => { const w = new Worker(url); \
               w.onmessage = e => done(name + ' ' + e.data); w.onerror = () => done(name + ' error'); }; \
             const done = what => { got.push(what); if (got.length === 9) report(got.sort().join(', ')); }; \
             tried('other', OTHER + '/w.js'); tried('redirected', '/to-other.js'); \
             tried('missing', '/missing.js'); tried('image', '/image.js'); tried('csv', '/csv.js'); \
             tried('plain', '/plain.js'); tried('data', 'data:image/png,postMessage(\"ran\")'); \
             tried('nosniff', '/nosniff.js'); tried('nosniff-js', '/nosniff-js.js');",
            |other| {
                vec![
                    (
                        "/to-other.js",
                        Route::redirect(302, &format!("{other}/w.js")),
                    ),
                    ("/missing.js", Route::new(404, ran)),
                    ("/image.js", Route::ok("image/png", ran)),
                    ("/csv.js", Route::ok("text/csv", ran)),
                    ("/plain.js", Route::ok("text/plain", ran)),
                    (
                        "/nosniff.js",
                        Route::ok("text/plain", ran)
                            .with_header("X-Content-Type-Options", "NoSniff"),
                    ),
                    (
                        "/nosniff-js.js",
                        js(ran).with_header("X-Content-Type-Options", "nosniff"),
                    ),
                ]
            },
            &[("/w.js", js(ran))],
        ),
        "csv error, data ran, image error, missing error, nosniff error, nosniff-js ran, other error, \
         plain ran, redirected error"
    );
}

/// `importScripts()` of a script that can't be had (a status that isn't
/// ok, a MIME type no script is, or not JavaScript's under `nosniff`) throws a `NetworkError`, as one of
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
                               '{other}/throws.js', '/plain.js', '/nosniff.js'].map(tried).join());"
                        )),
                    ),
                    ("/missing.js", Route::new(404, "")),
                    ("/image.js", Route::ok("image/png", "")),
                    ("/throws.js", js("throw self;")),
                    ("/plain.js", Route::ok("text/plain", "")),
                    (
                        "/nosniff.js",
                        Route::ok("text/plain", "")
                            .with_header("X-Content-Type-Options", "nosniff"),
                    ),
                ]
            },
            &[("/throws.js", js("throw self;"))],
        ),
        "NetworkError,NetworkError,self,NetworkError,ran,NetworkError"
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
        |_| {
            vec![(
                "/slow.js",
                js("postMessage(1);").with_delay(Duration::from_secs(1)),
            )]
        },
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

/// A worker's `fetch()`: its own origin's, another's by CORS (as its
/// origin) and `no-cors` (opaque); a blob URL it makes is had by its
/// `fetch()`, its `importScripts()` and its page's `fetch()`, until the
/// worker goes (the browser lets go of its URLs then).
#[test]
fn a_worker_s_fetch_and_blob_urls() {
    assert_eq!(
        report_served(
            "const w = new Worker('/w.js'); \
             w.onmessage = async e => { \
               const [got, url] = e.data; \
               const page = await (await fetch(url)).text(); \
               w.terminate(); \
               const after = await fetch(url).then(() => 'had', e => e.name); \
               report([...got, page, after].join(' | ')); };",
            |other| {
                let worker = format!(
                    "(async () => {{ \
                       const own = await (await fetch('/data.txt')).text(); \
                       const cors = await (await fetch('{other}/cors.txt')).text(); \
                       const opaque = (await fetch('{other}/opaque.txt', {{ mode: 'no-cors' }})).type; \
                       const url = URL.createObjectURL(new Blob(['from the worker'])); \
                       const blob = await (await fetch(url)).text(); \
                       importScripts(URL.createObjectURL( \
                         new Blob(['self.imported = 1;'], {{ type: 'text/javascript' }}))); \
                       postMessage([[own, cors, opaque, blob, self.imported], url]); \
                     }})();"
                );
                vec![
                    ("/w.js", js(&worker)),
                    ("/data.txt", Route::ok("text/plain", "own")),
                ]
            },
            &[
                (
                    "/cors.txt",
                    Route::ok("text/plain", "cors").with_header("Access-Control-Allow-Origin", "*"),
                ),
                ("/opaque.txt", Route::ok("text/plain", "opaque")),
            ],
        ),
        "own | cors | opaque | from the worker | 1 | from the worker | TypeError"
    );
}

/// An aborted `fetch()` of a worker's has its load closed, and rejects.
/// The page stays past when the server would have answered (the renderer
/// ending would close the load too).
#[test]
fn a_worker_s_aborted_fetch_has_its_load_closed() {
    let (report, server) = run_served(
        "const w = new Worker('/w.js');          w.onmessage = e => setTimeout(() => report(e.data), 2500);",
        |_| {
            vec![
                (
                    "/w.js",
                    js("const c = new AbortController(); \
                        fetch('/slow.txt', { signal: c.signal }).then( \
                          () => postMessage('had'), e => postMessage(e.name)); \
                        setTimeout(() => c.abort(), 200);"),
                ),
                (
                    "/slow.txt",
                    Route::ok("text/plain", "late").with_delay(Duration::from_secs(2)),
                ),
            ]
        },
        &[],
    );
    assert_eq!(report, "AbortError");
    assert!(server.wait_for_abandoned("/slow.txt", Duration::from_secs(2)));
}

/// A worker's `fetch()` follows a redirect; one answered while the worker
/// waits for an import is had after it.
#[test]
fn a_worker_s_fetch_is_redirected_and_answered_while_it_imports() {
    assert_eq!(
        report_served(
            "const w = new Worker('/w.js'); w.onmessage = e => report(e.data);",
            |_| {
                vec![
                    (
                        "/w.js",
                        js("fetch('/moved').then(r => r.text().then(t => \
                              postMessage([r.redirected, r.url.endsWith('/data.txt'), t, \
                                self.imported].join()))); \
                            importScripts('/slow.js');"),
                    ),
                    (
                        "/moved",
                        Route::new(302, "").with_header("Location", "/data.txt"),
                    ),
                    ("/data.txt", Route::ok("text/plain", "here")),
                    (
                        "/slow.js",
                        js("self.imported = 1;").with_delay(Duration::from_millis(500)),
                    ),
                ]
            },
            &[],
        ),
        "true,true,here,1"
    );
}

/// A port goes to a worker with what was posted to it before (in order),
/// answers there; one the worker makes comes back to the page and talks
/// from the worker's thread. (Two ports' messages come in no order.)
#[test]
fn ports_go_to_and_from_a_worker() {
    assert_eq!(
        report(
            "const got = []; let back; \
             const done = () => got.length === 3 && back && report(got.join() + ' ' + back); \
             const w = worker(`onmessage = e => { \
               const port = e.ports[0]; \
               port.onmessage = m => port.postMessage(m.data * 2); \
               const back = new MessageChannel(); \
               postMessage('back', [back.port1]); \
               back.port2.postMessage('from the worker'); };`); \
             const c = new MessageChannel(); \
             c.port1.postMessage(1); c.port1.postMessage(2); \
             w.postMessage('port', [c.port2]); \
             c.port1.onmessage = e => { got.push(e.data); \
               if (got.length === 2) c.port1.postMessage(3); done(); }; \
             w.onmessage = e => e.ports[0].onmessage = m => { back = m.data; done(); };"
        ),
        "2,4,6 from the worker"
    );
}

/// Two workers given the two ports of a channel talk through it while the
/// page is busy: nothing goes through the main thread.
#[test]
fn two_workers_talk_through_a_port_while_the_page_is_busy() {
    assert_eq!(
        report(
            "const a = worker(`onmessage = e => { const port = e.ports[0]; \
               port.onmessage = m => postMessage([m.data, performance.timeOrigin + performance.now()]); \
               port.postMessage('ping'); };`); \
             const b = worker(`onmessage = e => { const port = e.ports[0]; \
               port.onmessage = m => port.postMessage(m.data === 'ping' ? 'pong' : 'what'); };`); \
             const c = new MessageChannel(); \
             a.postMessage('', [c.port1]); b.postMessage('', [c.port2]); \
             const until = performance.now() + 1500; \
             while (performance.now() < until) {} \
             const busy = performance.timeOrigin + until; \
             a.onmessage = e => report([e.data[0], e.data[1] < busy].join());"
        ),
        "pong,true"
    );
}

/// A worker's message through a port wakes the page at once, nothing else
/// of the page's to run meanwhile: it comes well before the page would
/// have looked again.
#[test]
fn a_port_s_message_from_a_worker_wakes_the_page() {
    assert_eq!(
        report(
            "const w = worker(`onmessage = e => setTimeout(() => \
               e.ports[0].postMessage(performance.timeOrigin + performance.now()), 300);`); \
             const c = new MessageChannel(); \
             w.postMessage('', [c.port2]); \
             c.port1.onmessage = e => \
               report(performance.timeOrigin + performance.now() - e.data < 100 ? 'woken' : 'late');"
        ),
        "woken"
    );
}
