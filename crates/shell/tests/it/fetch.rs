//! `fetch()` in a whole browser, pages and data from a local test server:
//! what the page reports through `testRunner` (`headless::run_testharness`)
//! and what the server saw. The renderer sends the request, the browser's
//! network service loads it, and the response settles the promise.

use std::collections::HashMap;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use shell::headless;

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

/// What the page at `url` reports.
fn report(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

/// A page whose script runs `body` (an async function's), reporting what
/// it pushes onto `log`, or the error it threw.
fn page(body: &str) -> String {
    format!(
        r#"<!DOCTYPE html><script>
const log = [];
(async () => {{ {body} }})().then(
  () => testRunner.reportHarnessResults(log.join(' / ')),
  e => testRunner.reportHarnessResults('threw ' + e));
</script>"#
    )
}

/// A same-origin `fetch()`: the response's status, status text, URL, type
/// and headers (the server's, but `Set-Cookie`; immutable), and its body
/// read as text and as JSON; a 404 resolves too, not ok; a 204's body is
/// null (reading it doesn't use it).
#[test]
fn a_same_origin_fetch() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "const r = await fetch('data.json');
                     log.push(r.status, r.statusText, r.ok, r.type, r.redirected, new URL(r.url).pathname);
                     log.push(r.headers.get('content-type'), r.headers.get('x-machopo'), r.headers.has('set-cookie'));
                     try { r.headers.append('a', 'b'); } catch (e) { log.push(e.name); }
                     const data = await r.json();
                     log.push(data.items.join(','));
                     const missing = await fetch('/missing');
                     log.push(missing.status, missing.ok, await missing.text());
                     const empty = await fetch('/empty');
                     log.push(empty.status, JSON.stringify(await empty.text()), empty.bodyUsed);",
                ),
            ),
        ),
        (
            "/data.json",
            Route::ok("application/json", r#"{"items": [1, 2, 3]}"#)
                .with_header("X-Machopo", "yes")
                .with_header("Set-Cookie", "a=b"),
        ),
        ("/missing", Route::new(404, "gone")),
        ("/empty", Route::new(204, "")),
    ]);
    assert_eq!(
        report(&server.url("/")),
        "200 / OK / true / basic / false / /data.json / application/json / yes / false / \
         TypeError / 1,2,3 / 404 / false / gone / 204 / \"\" / false"
    );
}

/// What the request carries: the page's headers but a forbidden one
/// (`Cookie`), `Accept: */*` unless the page gives one.
#[test]
fn the_requests_headers() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "await fetch('plain', { headers: { 'X-One': '1', Cookie: 'a=b' } });
                     await fetch('accept', { headers: [['Accept', 'text/x-machopo']] });",
                ),
            ),
        ),
        ("/plain", Route::ok("text/plain", "")),
        ("/accept", Route::ok("text/plain", "")),
    ]);
    assert_eq!(report(&server.url("/")), "");
    let requests = server.requests();
    let plain = requests.iter().find(|r| r.target == "/plain").unwrap();
    assert_eq!(plain.header("x-one"), Some("1"));
    assert_eq!(plain.header("cookie"), None);
    assert_eq!(plain.header("accept"), Some("*/*"));
    let accept = requests.iter().find(|r| r.target == "/accept").unwrap();
    assert_eq!(accept.header("accept"), Some("text/x-machopo"));
}

/// A redirect to the same origin is followed (`redirected`, the final
/// URL, without the fragment it kept). Another origin's resource goes by
/// CORS: one that doesn't allow this origin is a network error (a
/// `TypeError`), straight or by a redirect; one that does is a CORS
/// response, its headers filtered; a `no-cors` request's is opaque. `file:`
/// is a `TypeError`; a `data:` URL is read.
#[test]
fn where_a_fetch_may_go() {
    // Another origin: another port.
    let other = server(&[
        ("/plain", Route::ok("text/plain", "elsewhere")),
        (
            "/open",
            Route::ok("text/plain", "open")
                .with_header("Access-Control-Allow-Origin", "*")
                .with_header("X-Secret", "1"),
        ),
    ]);
    let elsewhere = other.url("/plain");
    let open = other.url("/open");
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(&format!(
                    "const moved = await fetch('moved#x');
                     log.push(moved.redirected, new URL(moved.url).pathname, await moved.text());
                     log.push(JSON.stringify(new URL(moved.url).hash));
                     for (const [url, init] of [['{elsewhere}', {{}}], ['away', {{}}],
                                                ['file:///etc/hosts', {{}}]]) {{
                       try {{ await fetch(url, init); log.push('fetched'); }} catch (e) {{ log.push(e.name); }}
                     }}
                     const allowed = await fetch('{open}');
                     log.push(allowed.type, allowed.headers.get('content-type'),
                              allowed.headers.has('x-secret'), await allowed.text());
                     const opaque = await fetch('{elsewhere}', {{ mode: 'no-cors' }});
                     log.push(opaque.type, opaque.status, JSON.stringify(opaque.url),
                              [...opaque.headers].length, JSON.stringify(await opaque.text()));
                     const data = await fetch('data:text/plain;charset=utf-8,h%C3%A9');
                     log.push(data.type, data.headers.get('content-type'), await data.text());"
                )),
            ),
        ),
        ("/moved", Route::redirect(302, "/plain")),
        ("/plain", Route::ok("text/plain", "here")),
        ("/away", Route::redirect(302, &elsewhere)),
    ]);
    assert_eq!(
        report(&server.url("/")),
        "true / /plain / here / \"\" / TypeError / TypeError / TypeError / \
         cors / text/plain / false / open / opaque / 0 / \"\" / 0 / \"\" / \
         basic / text/plain;charset=utf-8 / h\u{e9}"
    );
    let targets: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
    assert_eq!(targets, ["/", "/moved", "/plain", "/away"]);
    // The other origin was asked, with this one's `Origin` but for the
    // `no-cors` request.
    let origin = server.url("").trim_end_matches('/').to_owned();
    let asked: Vec<(String, Option<String>)> = other
        .requests()
        .into_iter()
        .map(|r| (r.target.clone(), r.header("origin").map(str::to_owned)))
        .collect();
    assert_eq!(
        asked,
        [
            ("/plain".to_owned(), Some(origin.clone())),
            ("/plain".to_owned(), Some(origin.clone())),
            ("/open".to_owned(), Some(origin)),
            ("/plain".to_owned(), None),
        ]
    );
}

/// A `fetch()` doesn't hold the page's `load` back: a slow one settles
/// after it.
#[test]
fn a_fetch_does_not_delay_load() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><script>
const log = [];
fetch('slow').then(r => r.text()).then(t => {
  log.push(t);
  testRunner.reportHarnessResults(log.join(' / '));
});
addEventListener('load', () => log.push('load'));
</script>"#,
            ),
        ),
        (
            "/slow",
            Route::ok("text/plain", "fetched").with_delay(Duration::from_millis(500)),
        ),
    ]);
    assert_eq!(report(&server.url("/")), "load / fetched");
}

/// A `fetch()` and the page's own loads number their requests from one
/// count: a stylesheet the page loads while a fetch waits doesn't answer it.
#[test]
fn a_fetch_and_the_pages_loads() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                r#"<!DOCTYPE html><script>
fetch('slow').then(r => r.text()).then(t => testRunner.reportHarnessResults(t));
</script><link rel="stylesheet" href="style.css">"#,
            ),
        ),
        (
            "/slow",
            Route::ok("text/plain", "fetched").with_delay(Duration::from_millis(500)),
        ),
        ("/style.css", Route::ok("text/css", "p { color: green }")),
    ]);
    assert_eq!(report(&server.url("/")), "fetched");
    let targets: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
    assert!(targets.contains(&"/style.css".to_owned()), "{targets:?}");
}

/// Any method, with a body (and the `Content-Type` it implies): the server
/// gets them, from a string's `fetch()` and from a `Request`'s; a 303 goes
/// on as a `GET` without the body. A redirect is a `TypeError` for an
/// `error` request, and an opaque-redirect response for a `manual` one.
#[test]
fn methods_bodies_and_redirect_modes() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "let r = await fetch('echo', { method: 'post', body: '{\"a\": 1}',
                                                   headers: { 'Content-Type': 'application/json' } });
                     log.push(r.status, await r.text());
                     r = await fetch(new Request('echo', { method: 'PUT', body: new URLSearchParams('b=2') }));
                     log.push(r.status);
                     r = await fetch('see-other', { method: 'POST', body: 'gone' });
                     log.push(r.redirected, new URL(r.url).pathname);
                     try { await fetch('see-other', { redirect: 'error' }); } catch (e) { log.push(e.name); }
                     r = await fetch('see-other', { redirect: 'manual' });
                     log.push(r.type, r.status, JSON.stringify(r.statusText), new URL(r.url).pathname,
                              [...r.headers].length, JSON.stringify(await r.text()));",
                ),
            ),
        ),
        ("/echo", Route::ok("text/plain", "echoed")),
        ("/see-other", Route::redirect(303, "/echo")),
    ]);
    assert_eq!(
        report(&server.url("/")),
        "200 / echoed / 200 / true / /echo / TypeError / opaqueredirect / 0 / \"\" / /see-other / 0 / \"\""
    );
    let requests = server.requests();
    let seen: Vec<(String, String, String, Option<String>)> = requests
        .iter()
        .skip(1)
        .map(|r| {
            (
                r.method.clone(),
                r.target.clone(),
                String::from_utf8(r.body.clone()).unwrap(),
                r.header("content-type").map(str::to_owned),
            )
        })
        .collect();
    let plain =
        |method: &str, target: &str| (method.to_owned(), target.to_owned(), String::new(), None);
    assert_eq!(
        seen,
        [
            (
                "POST".to_owned(),
                "/echo".to_owned(),
                "{\"a\": 1}".to_owned(),
                Some("application/json".to_owned())
            ),
            (
                "PUT".to_owned(),
                "/echo".to_owned(),
                "b=2".to_owned(),
                Some("application/x-www-form-urlencoded;charset=UTF-8".to_owned())
            ),
            (
                "POST".to_owned(),
                "/see-other".to_owned(),
                "gone".to_owned(),
                Some("text/plain;charset=UTF-8".to_owned())
            ),
            plain("GET", "/echo"),
            plain("GET", "/see-other"),
            plain("GET", "/see-other"),
        ]
    );
}

/// A `fetch()` aborted while its response is slow to come rejects with
/// the reason at once, and the browser cancels the load (the server sees
/// it given up); `AbortSignal.timeout()` aborts one too; a response's body
/// can't be read once its signal aborted.
#[test]
fn an_aborted_fetch() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "const start = performance.now();
                     const controller = new AbortController();
                     const slow = fetch('slow', { signal: controller.signal });
                     // Late enough for the request to reach the server.
                     setTimeout(() => controller.abort('stop'), 200);
                     try { await slow; } catch (e) { log.push(e, performance.now() - start < 1000); }
                     try { await fetch('slow', { signal: AbortSignal.timeout(200) }); }
                     catch (e) { log.push(e.name); }
                     const later = new AbortController();
                     const r = await fetch('fast', { signal: later.signal });
                     later.abort();
                     try { await r.text(); } catch (e) { log.push('body: ' + e.name); }",
                ),
            ),
        ),
        (
            "/slow",
            Route::ok("text/plain", "late").with_delay(Duration::from_millis(1500)),
        ),
        ("/fast", Route::ok("text/plain", "fast")),
    ]);
    assert_eq!(
        report(&server.url("/")),
        "stop / true / TimeoutError / body: AbortError"
    );
    assert_eq!(abandoned(&server, "/slow", 2), 2);
}

/// A frame navigated to another document cancels the `fetch()`es of the
/// one it leaves (its document is discarded), as removing its iframe does.
#[test]
fn a_frame_navigated_away_cancels_its_documents_fetches() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "const frame = document.createElement('iframe');
                     frame.srcdoc = `<script>fetch('slow'); parent.ready();<\\/script>`;
                     await new Promise(resolve => { window.ready = resolve; document.documentElement.append(frame); });
                     // Time for the request to reach the server.
                     await new Promise(resolve => setTimeout(resolve, 200));
                     await new Promise(resolve => { frame.onload = resolve; frame.srcdoc = '<p>next</p>'; });
                     log.push('navigated');",
                ),
            ),
        ),
        (
            "/slow",
            Route::ok("text/plain", "late").with_delay(Duration::from_millis(1500)),
        ),
    ]);
    assert_eq!(report(&server.url("/")), "navigated");
    assert_eq!(abandoned(&server, "/slow", 1), 1);
}

/// How many requests of `target` the server saw given up, waiting until
/// there are `expected` (or a few seconds).
fn abandoned(server: &TestServer, target: &str, expected: usize) -> usize {
    let count = || {
        server
            .requests()
            .iter()
            .filter(|r| r.target == target && r.abandoned)
            .count()
    };
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while count() < expected && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    count()
}

/// A frame's `fetch()` aborted from another realm (its parent's controller)
/// rejects in its own; removing an iframe cancels its document's
/// `fetch()`es (the server sees them given up), and only theirs: the
/// page's own goes on.
#[test]
fn an_abort_from_another_realm() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "window.controller = new AbortController();
                     const slow = fetch('slow');
                     const frame = document.createElement('iframe');
                     frame.srcdoc = `<script>
                       parent.child = fetch('medium', { signal: parent.controller.signal })
                         .then(() => 'fetched', e => 'child: ' + e);
                       parent.ready();
                     <\\/script>`;
                     // Time for the requests to reach the server, before they go;
                     // 'medium' answers before the page is done, unless cancelled.
                     const settle = () => new Promise(resolve => setTimeout(resolve, 200));
                     await new Promise(resolve => { window.ready = resolve; document.documentElement.append(frame); });
                     await settle();
                     controller.abort('from parent');
                     log.push(await child);
                     const second = document.createElement('iframe');
                     second.srcdoc = `<script>fetch('medium'); parent.ready();<\\/script>`;
                     await new Promise(resolve => { window.ready = resolve; document.documentElement.append(second); });
                     await settle();
                     second.remove();
                     log.push(await (await slow).text());",
                ),
            ),
        ),
        (
            "/slow",
            Route::ok("text/plain", "late").with_delay(Duration::from_millis(1500)),
        ),
        (
            "/medium",
            Route::ok("text/plain", "medium").with_delay(Duration::from_millis(400)),
        ),
    ]);
    assert_eq!(report(&server.url("/")), "child: from parent / late");
    // Both answered before the page was done, had they not been
    // cancelled; the page's own went on.
    assert_eq!(abandoned(&server, "/medium", 2), 2);
    assert_eq!(abandoned(&server, "/slow", 0), 0);
}

/// A cross-origin `fetch()` that isn't simple goes after a preflight the
/// other origin answers for: an `OPTIONS` asking for the method and the
/// headers, then the request; one it doesn't answer for is a `TypeError`,
/// and the request isn't sent.
#[test]
fn a_preflighted_fetch() {
    let other = server(&[
        (
            "OPTIONS /data",
            Route::new(204, "")
                .with_header("Access-Control-Allow-Origin", "*")
                .with_header("Access-Control-Allow-Methods", "PUT")
                .with_header("Access-Control-Allow-Headers", "x-machopo"),
        ),
        (
            "/data",
            Route::ok("text/plain", "stored").with_header("Access-Control-Allow-Origin", "*"),
        ),
    ]);
    let data = other.url("/data");
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            page(&format!(
                "const r = await fetch('{data}', {{ method: 'PUT', headers: {{ 'X-Machopo': '1' }}, body: 'x' }});
                 log.push(r.type, await r.text());
                 try {{ await fetch('{data}', {{ method: 'DELETE' }}); }} catch (e) {{ log.push(e.name); }}"
            )),
        ),
    )]);
    assert_eq!(report(&server.url("/")), "cors / stored / TypeError");
    let seen: Vec<(String, Option<String>, Option<String>)> = other
        .requests()
        .into_iter()
        .map(|r| {
            (
                r.method.clone(),
                r.header("access-control-request-method").map(str::to_owned),
                r.header("access-control-request-headers")
                    .map(str::to_owned),
            )
        })
        .collect();
    let preflight = |method: &str, headers: Option<&str>| {
        (
            "OPTIONS".to_owned(),
            Some(method.to_owned()),
            headers.map(str::to_owned),
        )
    };
    assert_eq!(
        seen,
        [
            // `text/plain;charset=UTF-8`, the string body's, is safelisted.
            preflight("PUT", Some("x-machopo")),
            ("PUT".to_owned(), None, None),
            preflight("DELETE", None),
        ]
    );
}

/// A redirect waits for the renderer to say go on (`FollowRedirect`):
/// while its main thread runs script, the request it leads to isn't sent.
#[test]
fn a_redirect_waits_for_the_renderer() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                page(
                    "const moved = fetch('/moved');
                     const end = performance.now() + 1500;
                     while (performance.now() < end) {}
                     const r = await moved;
                     log.push(r.redirected, await r.text());",
                ),
            ),
        ),
        ("/moved", Route::redirect(302, "/plain")),
        ("/plain", Route::ok("text/plain", "plain")),
    ]);
    let url = server.url("/");
    let page = std::thread::spawn(move || report(&url));
    let requested = |path: &str| server.requests().iter().any(|r| r.target == path);
    let start = std::time::Instant::now();
    while !requested("/moved") {
        assert!(start.elapsed() < Duration::from_secs(30), "the fetch goes");
        std::thread::sleep(Duration::from_millis(10));
    }
    // Well inside the page's busy second and a half.
    std::thread::sleep(Duration::from_millis(500));
    assert!(!requested("/plain"), "followed while the renderer was busy");
    assert_eq!(page.join().unwrap(), "true / plain");
    assert!(requested("/plain"));
}

/// What a CORS response shows script of its headers depends on the
/// request: `*` exposes them all, unless its credentials are included.
#[test]
fn a_star_exposes_every_header_but_with_credentials() {
    let page_server = server(&[(
        "/",
        Route::ok(
            "text/html",
            page(
                "const other = new URLSearchParams(location.search).get('other');
                 const omitted = await fetch(other, { credentials: 'omit' });
                 const included = await fetch(other, { credentials: 'include' });
                 log.push(String(omitted.headers.get('x-secret')),
                          String(included.headers.get('x-secret')));",
            ),
        ),
    )]);
    let origin = page_server.url("/");
    let origin = origin.trim_end_matches('/');
    let other = server(&[(
        "/data",
        Route::ok("text/plain", "data")
            .with_header("Access-Control-Allow-Origin", origin)
            .with_header("Access-Control-Allow-Credentials", "true")
            .with_header("Access-Control-Expose-Headers", "*")
            .with_header("X-Secret", "1"),
    )]);
    let url = format!("{}?other={}", page_server.url("/"), other.url("/data"));
    assert_eq!(report(&url), "1 / null");
}
