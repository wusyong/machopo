//! machopodriver against machopo: WebDriver's HTTP API, with the driver
//! in this process and each session's machopo the `machopo` binary. Requests are
//! plain HTTP/1.1, as any client's.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use net::test_server::{Route, TestServer};
use serde_json::{Value, json};

/// A driver for the `machopo` this test was built with.
fn driver() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let options = driver::Options {
        binary: env!("CARGO_BIN_EXE_machopo").into(),
        binary_args: vec!["--test-fonts".into()],
    };
    driver::start(listener, options).unwrap().addr()
}

/// An HTTP request, with these headers on top of `Host`; its status and
/// its body's JSON.
fn raw(
    addr: SocketAddr,
    method: &str,
    path: &str,
    body: Option<&str>,
    headers: &[(&str, &str)],
) -> (u16, Value) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(60)))
        .unwrap();
    let body = body.unwrap_or("");
    let mut request = format!("{method} {path} HTTP/1.1\r\nConnection: close\r\n");
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        request += &format!("Host: {addr}\r\n");
    }
    for (name, value) in headers {
        request += &format!("{name}: {value}\r\n");
    }
    request += &format!("Content-Length: {}\r\n\r\n{body}", body.len());
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response[9..12].parse().unwrap();
    let (_, body) = response.split_once("\r\n\r\n").unwrap();
    (status, serde_json::from_str(body).unwrap_or(Value::Null))
}

fn request(addr: SocketAddr, method: &str, path: &str, body: Option<Value>) -> (u16, Value) {
    let body = body.map(|body| body.to_string());
    raw(
        addr,
        method,
        path,
        body.as_deref(),
        &[("Content-Type", "application/json")],
    )
}

/// The value of a successful answer.
fn ok(answer: (u16, Value)) -> Value {
    assert_eq!(answer.0, 200, "{}", answer.1);
    answer.1["value"].clone()
}

/// The status and error of a failed one.
fn error(answer: (u16, Value)) -> (u16, String) {
    (
        answer.0,
        answer.1["value"]["error"].as_str().unwrap().to_owned(),
    )
}

struct Session {
    addr: SocketAddr,
    id: String,
}

impl Session {
    fn new(addr: SocketAddr, capabilities: Value) -> (Self, Value) {
        let created = ok(request(
            addr,
            "POST",
            "/session",
            Some(json!({ "capabilities": { "alwaysMatch": capabilities } })),
        ));
        let id = created["sessionId"].as_str().unwrap().to_owned();
        (Self { addr, id }, created["capabilities"].clone())
    }

    fn call(&self, method: &str, command: &str, body: Option<Value>) -> (u16, Value) {
        let path = format!("/session/{}{command}", self.id);
        let body = body.or_else(|| (method == "POST").then(|| json!({})));
        request(self.addr, method, &path, body)
    }

    fn get(&self, command: &str) -> Value {
        ok(self.call("GET", command, None))
    }

    fn navigate(&self, url: &str) -> (u16, Value) {
        self.call("POST", "/url", Some(json!({ "url": url })))
    }

    fn delete(self) {
        ok(request(
            self.addr,
            "DELETE",
            &format!("/session/{}", self.id),
            None,
        ));
    }
}

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

/// One session at a time: Status says whether one can start; its
/// capabilities are machopo's, with WebDriver's defaults.
#[test]
fn a_session_starts_and_ends() {
    let addr = driver();
    assert_eq!(ok(request(addr, "GET", "/status", None))["ready"], true);
    let (session, capabilities) = Session::new(addr, json!({}));
    assert_eq!(capabilities["browserName"], "machopo");
    assert_eq!(capabilities["pageLoadStrategy"], "normal");
    assert_eq!(capabilities["setWindowRect"], true);
    assert_eq!(capabilities["acceptInsecureCerts"], false);
    assert_eq!(
        capabilities["timeouts"],
        json!({ "script": 30000, "pageLoad": 300000, "implicit": 0 })
    );
    assert!(
        capabilities["userAgent"]
            .as_str()
            .unwrap()
            .contains("Chrome/")
    );
    assert_eq!(ok(request(addr, "GET", "/status", None))["ready"], false);
    let second = request(
        addr,
        "POST",
        "/session",
        Some(json!({ "capabilities": {} })),
    );
    assert_eq!(error(second), (500, "session not created".into()));
    // Another session's id: not this one's.
    assert_eq!(
        error(request(addr, "GET", "/session/nope/url", None)),
        (404, "invalid session id".into())
    );
    let handle = session.get("/window");
    assert_eq!(session.get("/window/handles"), json!([handle]));
    session.delete();
    assert_eq!(ok(request(addr, "GET", "/status", None))["ready"], true);
    // A capability machopo can't meet: no session.
    let refused = request(
        addr,
        "POST",
        "/session",
        Some(json!({ "capabilities": { "alwaysMatch": { "browserName": "firefox" } } })),
    );
    assert_eq!(error(refused), (500, "session not created".into()));
}

/// Navigate To waits for the page; the URL and title are the page's;
/// back, forward and refresh wait for the document they show.
#[test]
fn a_session_navigates() {
    let server = server(&[
        (
            "/a",
            Route::ok("text/html", "<!DOCTYPE html><title>A</title>a"),
        ),
        (
            "/b",
            Route::ok("text/html", "<!DOCTYPE html><title>B</title>b"),
        ),
    ]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    let (a, b) = (server.url("/a"), server.url("/b"));
    ok(session.navigate(&a));
    assert_eq!(session.get("/url"), a.as_str());
    assert_eq!(session.get("/title"), "A");
    ok(session.navigate(&b));
    assert_eq!(session.get("/title"), "B");
    ok(session.call("POST", "/back", None));
    assert_eq!(session.get("/url"), a.as_str());
    assert_eq!(session.get("/title"), "A");
    ok(session.call("POST", "/forward", None));
    assert_eq!(session.get("/url"), b.as_str());
    // Forward again: there is nothing forward.
    ok(session.call("POST", "/forward", None));
    assert_eq!(session.get("/url"), b.as_str());
    ok(session.call("POST", "/refresh", None));
    assert_eq!(session.get("/title"), "B");
    // Navigated to, forward to (no back-forward cache), refreshed.
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|r| r.target == "/b")
            .count(),
        3
    );
    let (status, value) = session.navigate("not a url");
    assert_eq!(error((status, value)), (400, "invalid argument".into()));
    session.delete();
}

/// The page load strategy: `normal` waits for `load` (the slow image),
/// `eager` only for `DOMContentLoaded`; past the page load timeout,
/// `timeout`.
#[test]
fn navigations_wait_as_the_page_load_strategy_says() {
    let server = server(&[
        (
            "/slow",
            Route::ok("text/html", "<!DOCTYPE html><img src='/image.png'>"),
        ),
        (
            "/image.png",
            Route::ok("image/png", Vec::new()).with_delay(Duration::from_secs(3)),
        ),
    ]);
    let addr = driver();
    let slow = server.url("/slow");
    let short = json!({ "pageLoad": 1000 });

    let (session, _) = Session::new(addr, json!({ "timeouts": short }));
    assert_eq!(error(session.navigate(&slow)), (500, "timeout".into()));
    session.delete();

    let (session, capabilities) = Session::new(
        addr,
        json!({ "pageLoadStrategy": "eager", "timeouts": short }),
    );
    assert_eq!(capabilities["pageLoadStrategy"], "eager");
    ok(session.navigate(&slow));
    session.delete();

    let (session, _) = Session::new(addr, json!({ "pageLoadStrategy": "none" }));
    let start = Instant::now();
    ok(session.navigate(&slow));
    assert!(start.elapsed() < Duration::from_secs(3));
    session.delete();
}

/// Timeouts: what was set comes back; each may be null (none), and a
/// whole number of ms may be written as a float.
#[test]
fn timeouts_are_kept() {
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.call(
        "POST",
        "/timeouts",
        Some(json!({ "script": null, "pageLoad": 5, "implicit": 7 })),
    ));
    assert_eq!(
        session.get("/timeouts"),
        json!({ "script": null, "pageLoad": 5, "implicit": 7 })
    );
    ok(session.call(
        "POST",
        "/timeouts",
        Some(json!({ "pageLoad": null, "implicit": null })),
    ));
    assert_eq!(
        session.get("/timeouts"),
        json!({ "script": null, "pageLoad": null, "implicit": null })
    );
    let negative = session.call("POST", "/timeouts", Some(json!({ "implicit": -1 })));
    assert_eq!(error(negative), (400, "invalid argument".into()));
    session.delete();
    let (session, capabilities) = Session::new(
        addr,
        json!({ "timeouts": { "script": 2.0, "implicit": null } }),
    );
    assert_eq!(
        capabilities["timeouts"],
        json!({ "script": 2, "pageLoad": 300000, "implicit": null })
    );
    session.delete();
}

/// A tab whose renderer has gone: chromedriver's `tab crashed`, which
/// wptrunner records as a crash; navigating again works.
#[test]
fn a_crashed_tab_is_told() {
    let server = server(&[(
        "/",
        Route::ok("text/html", "<!DOCTYPE html><title>T</title>"),
    )]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({ "timeouts": { "pageLoad": 5000 } }));
    assert_eq!(
        error(session.navigate("about:crash")),
        (500, "tab crashed".into())
    );
    assert_eq!(
        error(session.call("GET", "/title", None)),
        (500, "tab crashed".into())
    );
    ok(session.navigate(&server.url("/")));
    assert_eq!(session.get("/title"), "T");
    session.delete();
}

/// A navigation whose load fails is an error, as chromedriver's (its error
/// page commits all the same); the next one goes on.
#[test]
fn a_navigation_that_fails_is_an_error() {
    let server = server(&[(
        "/",
        Route::ok("text/html", "<!DOCTYPE html><title>T</title>"),
    )]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    let refused = format!("http://127.0.0.1:{}/", net::test_server::unused_port());
    let (status, answer) = session.navigate(&refused);
    assert_eq!(status, 500, "{answer}");
    assert_eq!(answer["value"]["error"], "unknown error", "{answer}");
    assert_eq!(
        answer["value"]["message"], "net::ERR_CONNECTION_REFUSED",
        "{answer}"
    );
    ok(session.navigate(&server.url("/")));
    assert_eq!(session.get("/title"), "T");
    session.delete();
}

/// What isn't a command, or isn't this session's, or comes from a page.
#[test]
fn wrong_requests_are_refused() {
    let addr = driver();
    assert_eq!(
        error(request(addr, "GET", "/fly", None)),
        (404, "unknown command".into())
    );
    assert_eq!(
        error(request(addr, "PUT", "/status", None)),
        (405, "unknown method".into())
    );
    assert_eq!(
        error(request(addr, "GET", "/session/nope/url", None)),
        (404, "invalid session id".into())
    );
    let refused =
        |headers: &[(&str, &str)]| error(raw(addr, "POST", "/session", Some("{}"), headers));
    assert_eq!(
        refused(&[("Host", "example.com")]),
        (500, "unknown error".into())
    );
    assert_eq!(
        refused(&[("Origin", "http://example.com")]),
        (500, "unknown error".into())
    );
    assert_eq!(
        refused(&[("Content-Type", "text/plain")]),
        (500, "unknown error".into())
    );
    // Nothing started.
    assert_eq!(ok(request(addr, "GET", "/status", None))["ready"], true);
}

/// Execute (Async) Script through the driver: the value, WebDriver's
/// errors with their statuses, and the script timeout, which the driver
/// keeps.
#[test]
fn scripts_run_until_the_script_timeout() {
    let addr = driver();
    let (session, _) = Session::new(addr, json!({ "timeouts": { "script": 300 } }));
    let run = |command: &str, script: &str, args: Value| {
        session.call(
            "POST",
            command,
            Some(json!({ "script": script, "args": args })),
        )
    };
    assert_eq!(
        ok(run("/execute/sync", "return arguments[0] + 1", json!([1]))),
        2
    );
    assert_eq!(
        ok(run("/execute/async", "arguments[0]('done')", json!([]))),
        "done"
    );
    assert_eq!(
        error(run("/execute/sync", "throw new Error('no')", json!([]))),
        (500, "javascript error".into())
    );
    assert_eq!(
        error(run(
            "/execute/sync",
            "return new Promise(() => {})",
            json!([])
        )),
        (500, "script timeout".into())
    );
    assert_eq!(
        error(run("/execute/async", "", json!([]))),
        (500, "script timeout".into())
    );
    // The late answer doesn't get in the way of the next.
    assert_eq!(ok(run("/execute/sync", "return 3", json!([]))), 3);
    // An element reference machopo never made.
    let element = json!({ "element-6066-11e4-a52e-4f735466cecf": "nope" });
    assert_eq!(
        error(run("/execute/sync", "return 1", json!([element]))),
        (404, "no such element".into())
    );
    session.delete();
}

/// Find Element(s) through the driver: one, or all, from the document or
/// an element, tried again until the implicit wait is over; and the
/// element commands, by reference.
#[test]
fn elements_are_found_after_an_implicit_wait() {
    let page = "<!DOCTYPE html><title>Find</title><div id=a><p>one</p></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let find = |command: &str, value: &str| {
        session.call(
            "POST",
            command,
            Some(json!({ "using": "css selector", "value": value })),
        )
    };
    // No implicit wait: not there yet.
    assert_eq!(
        error(find("/element", "#late")),
        (404, "no such element".into())
    );
    assert_eq!(ok(find("/elements", "#late")), json!([]));
    ok(session.call("POST", "/timeouts", Some(json!({ "implicit": 5000 }))));
    // The element comes after the checks above, however long they took.
    ok(session.execute(
        "setTimeout(() => { const p = document.createElement('p'); \
         p.id = 'late'; document.body.appendChild(p); }, 200)",
    ));
    let late = ok(find("/element", "#late"));
    let late = late["element-6066-11e4-a52e-4f735466cecf"]
        .as_str()
        .unwrap();
    assert_eq!(session.get(&format!("/element/{late}/name")), "p");
    let a = ok(find("/element", "#a"));
    let a = a["element-6066-11e4-a52e-4f735466cecf"].as_str().unwrap();
    let inside = ok(session.call(
        "POST",
        &format!("/element/{a}/elements"),
        Some(json!({ "using": "tag name", "value": "p" })),
    ));
    assert_eq!(inside.as_array().map(Vec::len), Some(1));
    assert_eq!(session.get(&format!("/element/{a}/attribute/id")), "a");
    assert_eq!(session.get(&format!("/element/{a}/property/id")), "a");
    let rect = session.get(&format!("/element/{a}/rect"));
    assert!(rect["width"].as_f64().unwrap() > 0.0, "{rect}");
    // Strategies machopo doesn't find with.
    let xpath = session.call(
        "POST",
        "/element",
        Some(json!({ "using": "xpath", "value": "//p" })),
    );
    assert_eq!(error(xpath), (500, "unsupported operation".into()));
    session.delete();
}

/// An element in a shadow tree is connected with its host: a script gives
/// a reference to it that the element commands take, until its host leaves
/// the document (then it is stale).
#[test]
fn elements_in_shadow_trees_are_referenced() {
    let page = "<!DOCTYPE html><title>Shadow</title><div id=host></div><script>\
                globalThis.root = document.getElementById('host').attachShadow({ mode: 'closed' });\
                root.innerHTML = '<p id=in>x</p>';</script>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let p = element(&ok(session.execute("return root.firstChild")));
    assert_eq!(session.get(&format!("/element/{p}/name")), "p");
    assert_eq!(session.get(&format!("/element/{p}/property/id")), "in");
    ok(session.execute("document.getElementById('host').remove()"));
    assert_eq!(
        error(session.call("GET", &format!("/element/{p}/name"), None)),
        (404, "stale element reference".into())
    );
    session.delete();
}

fn element(found: &Value) -> String {
    found["element-6066-11e4-a52e-4f735466cecf"]
        .as_str()
        .unwrap_or_else(|| panic!("not an element: {found}"))
        .to_owned()
}

impl Session {
    fn post(&self, command: &str, body: Value) -> (u16, Value) {
        self.call("POST", command, Some(body))
    }

    fn find(&self, selector: &str) -> (u16, Value) {
        self.post(
            "/element",
            json!({ "using": "css selector", "value": selector }),
        )
    }

    fn execute(&self, script: &str) -> (u16, Value) {
        self.post("/execute/sync", json!({ "script": script, "args": [] }))
    }
}

/// Windows are tabs: New Window opens one behind the one shown, listed
/// after the others; Switch To Window goes to one and shows it (where
/// animation frames run); Close Window closes the session's, and the last
/// ends the session.
#[test]
fn windows_are_tabs() {
    let server = server(&[
        (
            "/a",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><title>A</title><iframe srcdoc='a'></iframe>",
            ),
        ),
        (
            "/b",
            Route::ok("text/html", "<!DOCTYPE html><title>B</title>b"),
        ),
    ]);
    let addr = driver();
    // A load in a tab not shown would wait for it to be shown.
    let timeouts = json!({ "timeouts": { "pageLoad": 10000, "script": 10000 } });
    let (session, _) = Session::new(addr, timeouts);
    ok(session.navigate(&server.url("/a")));
    ok(session.post("/frame", json!({ "id": 0 })));
    let first = session.get("/window");
    // A tab, whatever the type asked.
    let created = ok(session.post("/window/new", json!({ "type": "window" })));
    assert_eq!(created["type"], "tab");
    let second = created["handle"].clone();
    assert_ne!(second, first);
    assert_eq!(session.get("/window/handles"), json!([first, second]));
    // The session's window stays, and is still the tab shown.
    assert_eq!(session.get("/window"), first);
    assert_eq!(session.get("/title"), "A");
    let drawn = "return new Promise(r => requestAnimationFrame(() => r('drawn')))";
    assert_eq!(ok(session.execute(drawn)), "drawn");
    ok(session.post("/window", json!({ "handle": second })));
    assert_eq!(session.get("/window"), second);
    assert_eq!(session.get("/url"), "about:blank");
    // At its top-level browsing context, not the other window's frame.
    assert_eq!(ok(session.execute("return location.href")), "about:blank");
    ok(session.navigate(&server.url("/b")));
    assert_eq!(session.get("/title"), "B");
    assert_eq!(ok(session.execute(drawn)), "drawn");
    assert_eq!(
        error(session.post("/window", json!({ "handle": "nope" }))),
        (404, "no such window".into())
    );
    // Closed: the handles left; the session's window is no more.
    assert_eq!(ok(session.call("DELETE", "/window", None)), json!([first]));
    assert_eq!(
        error(session.call("GET", "/url", None)),
        (404, "no such window".into())
    );
    assert_eq!(
        error(session.call("DELETE", "/window", None)),
        (404, "no such window".into())
    );
    // The window before the URL.
    assert_eq!(
        error(session.navigate("not a url")),
        (404, "no such window".into())
    );
    ok(session.post("/window", json!({ "handle": first })));
    // At its top-level browsing context, not the frame it was left in.
    assert_eq!(ok(session.execute("return location.pathname")), "/a");
    // The last: the session ends with it.
    assert_eq!(ok(session.call("DELETE", "/window", None)), json!([]));
    assert_eq!(
        error(session.call("GET", "/url", None)),
        (404, "invalid session id".into())
    );
    assert_eq!(ok(request(addr, "GET", "/status", None))["ready"], true);
}

/// Switch To Frame by index, by iframe or to the top; Switch To Parent
/// Frame back up; commands work in the current frame; a child frame's
/// window is a frame reference, which is that window given back. A frame
/// removed is no such window; navigating goes back to the top.
#[test]
fn frames_are_switched_to_and_from() {
    switch_frames(driver());
}

fn switch_frames(addr: SocketAddr) {
    let server = server(&[
        (
            "/top",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><title>Top</title><iframe src=/child></iframe><p id=top>",
            ),
        ),
        (
            "/child",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><p id=child><iframe srcdoc='<p id=grand>'></iframe>",
            ),
        ),
    ]);
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/top")));
    let here = |selector: &str| session.find(selector).0 == 200;
    assert!(here("#top") && !here("#child"));
    let frame = ok(session.execute("return frames[0]"));
    assert!(
        frame.get("frame-075b-4da1-b6ba-e579c2d3230a").is_some(),
        "{frame}"
    );
    assert_eq!(
        ok(session.post(
            "/execute/sync",
            json!({ "script": "return arguments[0] === frames[0]", "args": [frame] })
        )),
        true
    );
    ok(session.post("/frame", json!({ "id": 0 })));
    assert!(here("#child") && !here("#top"));
    // The title is the top-level document's still.
    assert_eq!(session.get("/title"), "Top");
    ok(session.post("/frame", json!({ "id": 0 })));
    assert!(here("#grand"));
    ok(session.call("POST", "/frame/parent", None));
    assert!(here("#child"));
    ok(session.call("POST", "/frame/parent", None));
    assert!(here("#top"));
    // At the top, nowhere further.
    ok(session.call("POST", "/frame/parent", None));
    assert!(here("#top"));
    let iframe = element(&ok(session.find("iframe")));
    ok(session.post(
        "/frame",
        json!({ "id": { "element-6066-11e4-a52e-4f735466cecf": iframe } }),
    ));
    assert!(here("#child"));
    ok(session.post("/frame", json!({ "id": null })));
    assert!(here("#top"));
    assert_eq!(
        error(session.post("/frame", json!({ "id": 5 }))),
        (404, "no such frame".into())
    );

    // From the grandchild, its parent's iframe removed: both frames gone.
    ok(session.post("/frame", json!({ "id": 0 })));
    ok(session.post("/frame", json!({ "id": 0 })));
    ok(session.execute("parent.frameElement.remove()"));
    assert_eq!(error(session.find("p")), (404, "no such window".into()));
    assert_eq!(
        error(session.call("POST", "/frame/parent", None)),
        (404, "no such window".into())
    );
    assert_eq!(
        error(session.call("DELETE", "/actions", None)),
        (404, "no such window".into())
    );
    ok(session.navigate(&server.url("/top")));
    assert!(here("#top"));
    session.delete();
}

/// Frames script inserted are documents of their own at once: a reference
/// to an element of one isn't another's (no such element there, not a
/// stale reference to one the frame showed).
#[test]
fn inserted_frames_documents_are_their_own() {
    let page = "<!DOCTYPE html><body><script>for (const id of ['a', 'b']) {\
                const frame = document.body.appendChild(document.createElement('iframe'));\
                frame.contentDocument.body.innerHTML = `<p id=${id}>`; }</script>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    ok(session.post("/frame", json!({ "id": 0 })));
    let a = element(&ok(session.find("#a")));
    assert_eq!(session.get(&format!("/element/{a}/property/id")), "a");
    ok(session.call("POST", "/frame/parent", None));
    ok(session.post("/frame", json!({ "id": 1 })));
    assert!(session.find("#b").0 == 200);
    assert_eq!(
        error(session.call("GET", &format!("/element/{a}/property/id"), None)),
        (404, "no such element".into())
    );
    session.delete();
}

/// The window's rect: machopo's one window, its position and size as far as
/// given (the rest stays); the page's `screenX` and `outerWidth` follow.
#[test]
fn the_window_rect_is_set() {
    let addr = driver();
    let (session, capabilities) = Session::new(addr, json!({}));
    assert_eq!(capabilities["setWindowRect"], true);
    assert_eq!(
        session.get("/window/rect"),
        json!({ "x": 0, "y": 0, "width": 800, "height": 600 })
    );
    let set = |rect: Value| session.post("/window/rect", rect);
    assert_eq!(
        ok(set(json!({ "width": 640, "height": 480 }))),
        json!({ "x": 0, "y": 0, "width": 640, "height": 480 })
    );
    assert_eq!(
        ok(set(json!({ "x": -10, "y": 20 }))),
        json!({ "x": -10, "y": 20, "width": 640, "height": 480 })
    );
    let page = "return [screenX, screenY, outerWidth, outerHeight]";
    assert_eq!(ok(session.execute(page)), json!([-10, 20, 640, 480]));
    // Part of it: the rest stays.
    assert_eq!(
        ok(set(json!({ "width": 700 }))),
        json!({ "x": -10, "y": 20, "width": 700, "height": 480 })
    );
    assert_eq!(
        ok(set(json!({ "y": 5, "height": 500 }))),
        json!({ "x": -10, "y": 5, "width": 700, "height": 500 })
    );
    let same = json!({ "x": -10, "y": 5, "width": 700, "height": 500 });
    assert_eq!(ok(set(json!({}))), same);
    assert_eq!(ok(set(json!({ "x": null, "width": null }))), same);
    assert_eq!(session.get("/window/rect"), same);
    assert_eq!(
        error(set(json!({ "width": -1, "height": 10 }))),
        (400, "invalid argument".into())
    );
    session.delete();
}

/// Element Click: the element's click handler runs; a link followed is
/// waited for, as Navigate To is; something on top is "element click
/// intercepted", nothing in view "element not interactable" (both 400);
/// in a child frame too. Release Actions has nothing to release (the
/// click's button isn't an input source's).
#[test]
fn elements_are_clicked() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><title>Start</title><style>div { height: 40px } \
                 #over { margin-top: -40px }</style>\
                 <div id=count onclick='this.textContent = +this.textContent + 1'>0</div>\
                 <div id=under></div><div id=over></div><div id=none hidden></div>\
                 <iframe srcdoc='<button onclick=\"parent.framed = 1\">b</button>'></iframe>\
                 <p><a id=link href='/slow'>next</a></p>",
            ),
        ),
        (
            "/slow",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><title>Next</title><img src='/image.png'>",
            ),
        ),
        (
            "/image.png",
            Route::ok("image/png", Vec::new()).with_delay(Duration::from_millis(500)),
        ),
    ]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let click = |selector: &str| {
        let found = element(&ok(session.find(selector)));
        session.call("POST", &format!("/element/{found}/click"), None)
    };
    ok(click("#count"));
    ok(click("#count"));
    let count = "return document.getElementById('count').textContent";
    assert_eq!(ok(session.execute(count)), "2");
    assert_eq!(
        error(click("#under")),
        (400, "element click intercepted".into())
    );
    assert_eq!(
        error(click("#none")),
        (400, "element not interactable".into())
    );
    ok(session.post("/frame", json!({ "id": 0 })));
    ok(click("button"));
    ok(session.post("/frame", json!({ "id": null })));
    assert_eq!(ok(session.execute("return window.framed")), 1);
    ok(session.call("DELETE", "/actions", None));
    // The link: Element Click answers once the next page has loaded.
    ok(click("#link"));
    assert_eq!(session.get("/url"), server.url("/slow").as_str());
    assert_eq!(session.get("/title"), "Next");
    assert_eq!(
        ok(session.execute("return document.readyState")),
        "complete"
    );
    session.delete();
}

/// What is in view for Element Click and Element Send Keys: an element
/// partly in view is left where it is (only one out of view is scrolled
/// into it); one that hit testing doesn't find at its in-view center
/// point (it or something in it) is "element not interactable":
/// `pointer-events: none`, or cut off by an `overflow` clip that scrolling
/// can't move it into; a text control is hit by its UA shadow tree, which
/// is the control's.
#[test]
fn elements_in_view_are_not_scrolled_to() {
    not_scrolled_to(driver());
}

fn not_scrolled_to(addr: SocketAddr) {
    let page = "<!DOCTYPE html><style>body { margin: 0 } \
         #top { height: 72px } #spacer { height: calc(100vh - 92px) } \
         #part { height: 200px } #rest { height: 2000px } \
         #clip { margin-left: 150px; overflow: hidden; width: 50px; height: 20px } \
         #clipped { margin-left: -100px; width: 80px; height: 20px }</style>\
         <div id=top><input type=button id=button value=b onclick='window.button = 1'>\
         <input id=text><div id=through style='pointer-events: none'>\
         <div style='pointer-events: auto; height: 20px' onclick='window.inner = 1'></div></div></div>\
         <div id=nopointer style='pointer-events: none; height: 10px'></div>\
         <div id=spacer></div><div id=part onclick='window.part = 1'></div>\
         <div id=clip><div id=clipped></div></div><div id=rest></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let click = |selector: &str| {
        let found = element(&ok(session.find(selector)));
        session.call("POST", &format!("/element/{found}/click"), None)
    };
    let scrolled = || ok(session.execute("return scrollY"));
    // 10 px of it at the bottom of the viewport: clicked there.
    ok(click("#part"));
    assert_eq!(ok(session.execute("return window.part")), 1);
    assert_eq!(scrolled(), 0);
    ok(click("#button"));
    assert_eq!(ok(session.execute("return window.button")), 1);
    // What is hit of a text control is its UA shadow tree's editor.
    ok(click("#text"));
    assert_eq!(
        ok(session.execute("return document.activeElement.id")),
        "text"
    );
    // Not hit itself, but what is in it is: the click goes there.
    ok(click("#through"));
    assert_eq!(ok(session.execute("return window.inner")), 1);
    assert_eq!(
        error(click("#nopointer")),
        (400, "element not interactable".into())
    );
    assert_eq!(
        error(click("#clipped")),
        (400, "element not interactable".into())
    );
    // Partly in view after the page scrolled a little: typed in where it is.
    ok(session.execute("scrollTo(0, 0); scrollTo(0, 5)"));
    ok(session.send_keys("#text", "a"));
    assert_eq!(scrolled(), 5);
    assert_eq!(
        ok(session.execute("return document.getElementById('text').value")),
        "a"
    );
    // In view where the page is scrolled to.
    ok(session.execute("scrollTo(0, 100)"));
    ok(click("#part"));
    assert_eq!(scrolled(), 100);
    session.delete();
}

/// Element Click, Send Keys, Clear and Take Element Screenshot scroll an
/// element out of view into it first (WebDriver's "scroll into view":
/// `block: end`, `inline: nearest`), its bottom at the viewport's, in a
/// child frame through the frames around it too.
#[test]
fn elements_out_of_view_are_scrolled_to() {
    scrolled_to(driver());
}

fn scrolled_to(addr: SocketAddr) {
    let page = "<!DOCTYPE html><style>body { margin: 0 } .far { margin-top: 3000px; display: block; height: 30px }          iframe { display: block; margin-top: 3000px; height: 100px; border: 0 }</style>\
         <button class=far id=button onclick='window.clicked = 1'>b</button>\
         <input class=far id=input><input class=far id=cleared value=abc><div class=far id=shot></div>\
         <iframe srcdoc='<style>body { margin: 0 } button { margin-top: 2000px; height: 30px; display: block }</style>\
         <button onclick=\"parent.framed = 1\">f</button>'></iframe><div style='height: 3000px'></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let found = |selector: &str| element(&ok(session.find(selector)));
    // Where the element's bottom is from the viewport's, and how far the
    // page scrolled.
    let at = |id: &str| {
        ok(session.execute(&format!(
            "const r = document.getElementById('{id}').getBoundingClientRect(); \
             return [Math.round(innerHeight - r.bottom), scrollY > 0]"
        )))
    };
    let top = || ok(session.execute("scrollTo(0, 0)"));
    ok(session.call(
        "POST",
        &format!("/element/{}/click", found("#button")),
        None,
    ));
    assert_eq!(ok(session.execute("return window.clicked")), 1);
    assert_eq!(at("button"), json!([0, true]));
    top();
    ok(session.send_keys("#input", "a"));
    assert_eq!(at("input"), json!([0, true]));
    assert_eq!(
        ok(session.execute("return document.getElementById('input').value")),
        "a"
    );
    top();
    ok(session.clear("#cleared"));
    assert_eq!(at("cleared"), json!([0, true]));
    top();
    ok(session.call(
        "GET",
        &format!("/element/{}/screenshot", found("#shot")),
        None,
    ));
    assert_eq!(at("shot"), json!([0, true]));
    top();
    // In the child frame: the frame's scrolled, then the page.
    ok(session.post("/frame", json!({ "id": 0 })));
    ok(session.call("POST", &format!("/element/{}/click", found("button")), None));
    assert_eq!(
        ok(session.execute("return [scrollY > 0, parent.framed]")),
        json!([true, 1])
    );
    ok(session.post("/frame", json!({ "id": null })));
    assert_eq!(
        ok(session.execute(
            "const r = document.querySelector('iframe').getBoundingClientRect(); \
                            return Math.round(innerHeight - r.bottom)"
        )),
        0
    );
    session.delete();
}

/// A click in one document leaves the next one's focus from script with a
/// ring (`:focus-visible`), as wptrunner has it: it clicks the test
/// window's `about:blank` before it loads each test there.
#[test]
fn a_new_document_forgets_the_mouses_focus() {
    let page = "<!DOCTYPE html><input id=text><input id=other>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    let visible = || {
        ok(session.execute(
            "const i = document.getElementById('text'); i.focus(); \
             return i.matches(':focus-visible')",
        ))
    };
    ok(session.navigate(&server.url("/")));
    // The mouse's focus: script's next one has no ring in this document.
    let other = element(&ok(session.find("#other")));
    ok(session.call("POST", &format!("/element/{other}/click"), None));
    assert_eq!(visible(), false);
    ok(session.navigate("about:blank"));
    let root = element(&ok(session.execute("return document.documentElement")));
    ok(session.call("POST", &format!("/element/{root}/click"), None));
    ok(session.navigate(&server.url("/")));
    assert_eq!(visible(), true);
    session.delete();
}

/// A page that logs its key, mouse and wheel events, with a box at the
/// top, a child frame logging its moves under it, and room to scroll.
const ACTIONS_PAGE: &str = "<!DOCTYPE html><style>html, body { margin: 0 } \
    #box { width: 100px; height: 50px } \
    iframe { border: 0; display: block; margin-left: 30px; width: 200px; height: 100px } \
    #tall { height: 3000px }</style>\
    <div id=box></div>\
    <iframe srcdoc=\"<body style='margin: 0'><script>window.log = []; \
    addEventListener('mousemove', e => log.push([e.clientX, e.clientY]));</script>\"></iframe>\
    <div id=tall></div><script>window.log = []; \
    for (const type of ['keydown', 'keyup']) \
    addEventListener(type, e => log.push(`${type} ${e.key} ${e.shiftKey}`)); \
    for (const type of ['mousedown', 'mouseup', 'click', 'dblclick']) \
    addEventListener(type, e => log.push(\
    `${type} ${e.target.id} ${e.clientX},${e.clientY} ${e.detail} ${e.shiftKey}`)); \
    addEventListener('wheel', e => log.push(`wheel ${e.deltaY} ${e.shiftKey}`));</script>";

impl Session {
    /// The page's log, emptied.
    fn log(&self) -> Value {
        ok(self.execute("const got = log.slice(); log.length = 0; return got"))
    }

    fn perform(&self, actions: Value) -> (u16, Value) {
        self.post("/actions", json!({ "actions": actions }))
    }
}

/// Perform Actions: keys and the mouse as the user's, in ticks (a key
/// held through the clicks of another source), at an element's center or
/// a point of the viewport; a double click; a wheel scrolling the page; a
/// pause lasting; in a child frame, its viewport's points. Release
/// Actions lets go of what is still pressed, the last first, and forgets
/// the sources.
#[test]
fn actions_are_the_users_input() {
    let server = server(&[("/", Route::ok("text/html", ACTIONS_PAGE))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let r#box =
        json!({ "element-6066-11e4-a52e-4f735466cecf": element(&ok(session.find("#box"))) });
    let down = json!({ "type": "pointerDown", "button": 0 });
    let up = json!({ "type": "pointerUp", "button": 0 });
    let pause = json!({ "type": "pause" });
    ok(session.perform(json!([
        { "type": "key", "id": "keys", "actions": [
            { "type": "keyDown", "value": "\u{E008}" },
            pause, pause, pause, pause, pause,
            { "type": "keyUp", "value": "\u{E008}" },
        ]},
        { "type": "pointer", "id": "mouse", "actions": [
            pause,
            { "type": "pointerMove", "x": 0, "y": 0, "origin": r#box },
            down, up, down, up,
        ]},
    ])));
    assert_eq!(
        session.log(),
        json!([
            "keydown Shift true",
            "mousedown box 50,25 1 true",
            "mouseup box 50,25 1 true",
            "click box 50,25 1 true",
            "mousedown box 50,25 2 true",
            "mouseup box 50,25 2 true",
            "click box 50,25 2 true",
            "dblclick box 50,25 2 true",
            "keyup Shift false",
        ])
    );
    // The same click later, in another Perform Actions: a single one.
    ok(session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [down, up] }])));
    assert_eq!(
        session.log(),
        json!([
            "mousedown box 50,25 1 false",
            "mouseup box 50,25 1 false",
            "click box 50,25 1 false",
        ])
    );
    // A pause lasts (given as a whole float, as wptrunner's are).
    let started = Instant::now();
    let pause = r#"{"actions": [{"type": "none", "id": "n", "actions": [
        {"type": "pause", "duration": 300.0}]}]}"#;
    let path = format!("/session/{}/actions", session.id);
    let (status, _) = raw(
        addr,
        "POST",
        &path,
        Some(pause),
        &[("Content-Type", "application/json")],
    );
    assert_eq!(status, 200);
    assert!(started.elapsed() >= Duration::from_millis(300));
    // Out of the viewport.
    let outside = session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
        { "type": "pointerMove", "x": -1, "y": 0 },
    ]}]));
    assert_eq!(error(outside), (500, "move target out of bounds".into()));
    // In the child frame: its viewport's points.
    ok(session.post("/frame", json!({ "id": 0 })));
    ok(
        session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
            { "type": "pointerMove", "x": 5, "y": 6 },
            { "type": "pointerMove", "x": 1, "y": 1, "origin": "pointer" },
        ]}])),
    );
    assert_eq!(session.log(), json!([[5, 6], [6, 7]]));
    let past = session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
        { "type": "pointerMove", "x": 201, "y": 0 },
    ]}]));
    assert_eq!(error(past), (500, "move target out of bounds".into()));
    ok(session.post("/frame", json!({ "id": null })));
    // A wheel, Shift held: the page scrolls.
    ok(session.perform(json!([
        { "type": "key", "id": "keys", "actions": [{ "type": "keyDown", "value": "\u{E008}" }] },
        { "type": "wheel", "id": "wheel", "actions": [
            { "type": "scroll", "x": 10, "y": 10, "deltaX": 0, "deltaY": 100 },
        ]},
    ])));
    assert_eq!(
        session.log(),
        json!(["keydown Shift true", "wheel 100 true"])
    );
    assert_eq!(ok(session.execute("return Math.round(scrollY)")), 100);
    // Still held, with a button: Release Actions lets go of the button,
    // then the key; then nothing is held.
    ok(
        session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
            { "type": "pointerMove", "x": 10, "y": 60 },
            down,
        ]}])),
    );
    ok(session.call("DELETE", "/actions", None));
    assert_eq!(
        session.log(),
        json!([
            "mousedown tall 10,60 1 true",
            "mouseup tall 10,60 1 true",
            "click tall 10,60 1 true",
            "keyup Shift false",
        ])
    );
    ok(session.call("DELETE", "/actions", None));
    assert_eq!(session.log(), json!([]));
    // The sources went too: `keys` can be a pointer now.
    ok(session.perform(json!([{ "type": "pointer", "id": "keys", "actions": [] }])));
    // What machopo's mouse doesn't have.
    let pen = session.perform(json!([{ "type": "pointer", "id": "pen", "actions": [],
        "parameters": { "pointerType": "pen" } }]));
    assert_eq!(error(pen), (500, "unsupported operation".into()));
    let back = session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
        { "type": "pointerDown", "button": 3 },
    ]}]));
    assert_eq!(error(back), (500, "unsupported operation".into()));
    session.delete();
}

/// Each window has its input state: a key held in one isn't in another,
/// and stays held in its own; closing a window drops its state.
#[test]
fn input_state_is_each_windows() {
    let server = server(&[("/", Route::ok("text/html", ACTIONS_PAGE))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let first = session.get("/window");
    let press = |value: &str| {
        json!([{ "type": "key", "id": "typing", "actions": [
            { "type": "keyDown", "value": value },
            { "type": "keyUp", "value": value },
        ]}])
    };
    let shift = json!([{ "type": "key", "id": "keys", "actions": [
        { "type": "keyDown", "value": "\u{E008}" },
    ]}]);
    ok(session.perform(shift.clone()));
    let second = ok(session.post("/window/new", json!({})))["handle"].clone();
    ok(session.post("/window", json!({ "handle": second })));
    ok(session.navigate(&server.url("/")));
    // A source of another type by the same id: this window's own.
    let mouse = json!([{ "type": "pointer", "id": "keys", "actions": [] }]);
    ok(session.perform(mouse.clone()));
    ok(session.perform(press("x")));
    assert_eq!(session.log(), json!(["keydown x false", "keyup x false"]));
    ok(session.post("/window", json!({ "handle": first })));
    assert_eq!(
        error(session.perform(mouse)),
        (400, "invalid argument".into())
    );
    ok(session.perform(press("y")));
    assert_eq!(
        session.log(),
        json!(["keydown Shift true", "keydown y true", "keyup y true"])
    );
    // Closed with Shift held: the other window has nothing of it to let go
    // of.
    ok(session.call("DELETE", "/window", None));
    ok(session.post("/window", json!({ "handle": second })));
    ok(session.call("DELETE", "/actions", None));
    assert_eq!(session.log(), json!([]));
    session.delete();
}

/// A page whose text controls log their events.
const TYPING_PAGE: &str = "<!DOCTYPE html><style>#far { position: absolute; left: -1000px }</style>\
    <input id=i value=a><textarea id=t></textarea><input id=r readonly value=r>\
    <input id=d type=date><input id=f type=file><input id=c type=checkbox>\
    <input id=far value=x><input id=g type=range value=42><input id=v value=v>\
    <div id=plain>text</div><button id=b>b</button><input id=late hidden>\
    <script>window.log = []; \
    for (const el of document.querySelectorAll('input, textarea')) \
    for (const type of ['focus', 'blur', 'keydown', 'keypress', 'beforeinput', \
    'textInput', 'input', 'keyup', 'change']) \
    el.addEventListener(type, e => log.push(`${el.id}:${type}${e.key ? ':' + e.key : ''}`));</script>";

impl Session {
    fn send_keys(&self, selector: &str, text: &str) -> (u16, Value) {
        let found = element(&ok(self.find(selector)));
        self.post(&format!("/element/{found}/value"), json!({ "text": text }))
    }

    fn clear(&self, selector: &str) -> (u16, Value) {
        let found = element(&ok(self.find(selector)));
        self.call("POST", &format!("/element/{found}/clear"), None)
    }
}

/// Element Send Keys: the element takes the focus, the cursor at the end
/// of its value unless it had it; each character's key goes to it, with
/// Shift for the shifted ones; a read-only one takes the keys and keeps
/// its value; one that can't take the focus is "element not
/// interactable", asked again for the implicit wait; a non-typeable
/// control's value is set; a file input is "unsupported operation".
/// Leaving a control commits the change the keys made.
#[test]
fn keys_are_sent_to_elements() {
    let server = server(&[("/", Route::ok("text/html", TYPING_PAGE))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    ok(session.send_keys("#i", "bC"));
    assert_eq!(
        session.log(),
        json!([
            "i:focus",
            "i:keydown:b",
            "i:keypress:b",
            "i:beforeinput",
            "i:textInput",
            "i:input",
            "i:keyup:b",
            "i:keydown:Shift",
            "i:keydown:C",
            "i:keypress:C",
            "i:beforeinput",
            "i:textInput",
            "i:input",
            "i:keyup:C",
            "i:keyup:Shift",
        ])
    );
    let value =
        |id: &str| ok(session.execute(&format!("return document.getElementById('{id}').value")));
    assert_eq!(value("i"), "abC");
    // Focused already: where its cursor is.
    ok(session.execute("document.getElementById('i').setSelectionRange(0, 0)"));
    ok(session.send_keys("#i", "z"));
    assert_eq!(value("i"), "zabC");
    session.log();
    // Another control: the first commits its change as it loses the focus.
    ok(session.send_keys("#t", "x\u{E007}y"));
    assert_eq!(value("t"), "x\ny");
    let log = session.log();
    assert_eq!(log[0], "i:change");
    assert_eq!(log[1], "i:blur");
    ok(session.send_keys("#r", "q"));
    assert_eq!(value("r"), "r");
    ok(session.send_keys("#d", "2017-12-26"));
    assert_eq!(value("d"), "2017-12-26");
    assert_eq!(
        error(session.send_keys("#f", "C:\\file.txt")),
        (500, "unsupported operation".into())
    );
    assert_eq!(
        error(session.send_keys("#plain", "a")),
        (400, "element not interactable".into())
    );
    // Not interactable until it is shown, within the implicit wait.
    ok(session.post("/timeouts", json!({ "implicit": 5000 })));
    ok(session.execute(
        "setTimeout(() => document.getElementById('late').removeAttribute('hidden'), 200)",
    ));
    ok(session.send_keys("#late", "w"));
    assert_eq!(value("late"), "w");
    session.delete();
}

/// Element Clear: an editable control's value emptied (`input` and
/// `change`, between its focus and its blur; one change, whatever the
/// user typed before); an empty one untouched; a
/// read-only one or a checkbox "invalid element state"; one out of view
/// "element not interactable" (one under another element is cleared:
/// nothing is hit tested).
#[test]
fn elements_are_cleared() {
    let page = TYPING_PAGE.replace(
        "<button id=b>b</button>",
        "<button id=b>b</button><div style='margin-top: -60px; height: 80px'></div>",
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    ok(session.clear("#i"));
    let value =
        |id: &str| ok(session.execute(&format!("return document.getElementById('{id}').value")));
    assert_eq!(value("i"), "");
    assert_eq!(
        session.log(),
        json!(["i:focus", "i:input", "i:change", "i:blur"])
    );
    assert_eq!(
        ok(session.execute("return document.activeElement.localName")),
        "body"
    );
    ok(session.clear("#t"));
    assert_eq!(session.log(), json!([]));
    // Typed in first: cleared, the change typed and left to commit is
    // dropped (no second `change` as it loses the focus).
    ok(session.send_keys("#v", "w"));
    session.log();
    ok(session.clear("#v"));
    assert_eq!(session.log(), json!(["v:input", "v:change", "v:blur"]));
    ok(session.clear("#g"));
    assert_eq!(value("g"), "50");
    session.log();
    assert_eq!(
        error(session.clear("#r")),
        (400, "invalid element state".into())
    );
    assert_eq!(
        error(session.clear("#c")),
        (400, "invalid element state".into())
    );
    assert_eq!(
        error(session.clear("#far")),
        (400, "element not interactable".into())
    );
    assert_eq!(value("far"), "x");
    session.delete();
}

/// Navigate To may go to a `data:` URL (it is the user's navigation, not
/// the page's), and goes to it whole: its percent-encoded `?` and `#` are
/// part of the body, not a query or fragment. An iframe may show one.
#[test]
fn data_urls_are_navigated_to_whole() {
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    let url = "data:text/html,%3Cp%3Ea%3Fb%23c%3C/p%3E";
    ok(session.navigate(url));
    assert_eq!(session.get("/url"), url);
    assert_eq!(
        ok(session.execute("return [document.body.innerHTML, location.href]")),
        json!(["<p>a?b#c</p>", url])
    );
    // A child frame may have one too: only a page's own top-level
    // navigations may not.
    ok(session.navigate(
        "data:text/html,%3Ciframe%20src='data:text/html,%253Cp%2520id=inner%253E'%3E%3C/iframe%3E",
    ));
    ok(session.post("/frame", json!({ "id": 0 })));
    assert_eq!(session.find("#inner").0, 200);
    session.delete();
}

/// Take Screenshot is the page's view as a PNG; Take Element Screenshot
/// the element's part of it, scrolled into view.
#[test]
fn screenshots_are_pngs() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
        #red { width: 100px; height: 50px; background: red } \
        #far { margin-top: 2000px; width: 30px; height: 20px; background: blue }</style>\
        <div id=red></div>\
        <iframe srcdoc='<div id=green style=\"width: 10px; height: 10px; background: lime\"></div>'></iframe>\
        <div id=far></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    let size = ok(session.execute("return [innerWidth, innerHeight]"));
    let shot = session.get("/screenshot");
    let ((width, height), pixels) = png(shot.as_str().unwrap());
    assert_eq!(json!([width, height]), size);
    assert_eq!(&pixels[..3], [255, 0, 0]);
    let far = element(&ok(session.find("#far")));
    let shot = session.get(&format!("/element/{far}/screenshot"));
    let ((width, height), pixels) = png(shot.as_str().unwrap());
    assert_eq!((width, height), (30, 20));
    assert_eq!(&pixels[..3], [0, 0, 255]);
    // Nothing of it to capture: the spec's 500.
    ok(session.execute("document.getElementById('red').style.width = '0'"));
    let red = element(&ok(session.find("#red")));
    assert_eq!(
        error(session.call("GET", &format!("/element/{red}/screenshot"), None)),
        (500, "unable to capture screen".into())
    );
    // In a frame: the element's of the current one.
    ok(session.post("/frame", json!({ "id": 0 })));
    let green = element(&ok(session.find("#green")));
    let shot = session.get(&format!("/element/{green}/screenshot"));
    let ((width, height), pixels) = png(shot.as_str().unwrap());
    assert_eq!((width, height), (10, 10));
    assert_eq!(&pixels[..3], [0, 255, 0]);
    session.delete();
}

/// A PNG in base64: its size and its pixels, RGBA.
fn png(data: &str) -> ((u32, u32), Vec<u8>) {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("base64");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("a PNG");
    let mut pixels = vec![0; reader.output_buffer_size().expect("its size")];
    let info = reader.next_frame(&mut pixels).expect("its pixels");
    pixels.truncate(info.buffer_size());
    ((info.width, info.height), pixels)
}

/// WebDriver on the DOM's document: scripts (sync, async, throwing,
/// elements and the window out, elements in), Find Element(s) (from the
/// document and from an element), an element's rect, tag name, attribute
/// and property, Element Click (and what intercepts it or hides the
/// element), an action at an element, an element's screenshot, a new
/// window size read at once (and the browser window's), a stale
/// reference; no form controls (child frames are WPT's WebDriver
/// tests').
#[test]
fn the_document_is_driven() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html><style>html, body { margin: 0 } div { height: 40px; width: 100px } \
             #a { margin: 20px 0 0 10px; background: blue } #over { margin-top: -40px }</style>\
             <div id=a data-x=1 hidden-not=1></div><div id=count>0</div><div id=under></div><div id=over></div>\
             <div id=none style='display: none'></div><p hidden>p</p>\
             <script>document.getElementById('count').addEventListener('click', e => \
               e.target.textContent = +e.target.textContent + 1)</script>",
        ),
    )]);
    let addr = driver();
    let (session, _) = Session::new(addr, json!({}));
    ok(session.navigate(&server.url("/")));
    assert_eq!(ok(session.execute("return 1 + 2")), 3);
    let a = element(&ok(session.execute("return document.getElementById('a')")));
    assert_eq!(
        ok(session.post("/execute/sync", json!({ "script": "return arguments[0].id + arguments[1].x", "args": [{ "element-6066-11e4-a52e-4f735466cecf": a }, { "x": 2 }] }))),
        "a2"
    );
    assert_eq!(
        ok(session.post(
            "/execute/async",
            json!({ "script": "const done = arguments[0]; setTimeout(() => done([document.querySelectorAll('div').length, null]), 10)", "args": [] })
        )),
        json!([5, null])
    );
    assert_eq!(error(session.execute("throw new Error('boom')")).0, 500);
    assert_eq!(
        error(session.post(
            "/execute/async",
            json!({ "script": "return Promise.reject(new Error('no'))", "args": [] })
        ))
        .1,
        "javascript error"
    );
    let window = ok(session.execute("return window"));
    assert!(
        window.get("window-fcc6-11e5-b4f8-330a88ab9d7f").is_some(),
        "{window}"
    );
    assert_eq!(
        ok(session.post(
            "/execute/sync",
            json!({ "script": "return arguments[0] === window", "args": [window] })
        )),
        true
    );
    assert_eq!(
        error(session.execute("return document.createElement('div')")),
        (404, "stale element reference".into()),
        "an element out of the document"
    );
    assert_eq!(element(&ok(session.find("#a"))), a, "the same reference");
    let divs = ok(session.post("/elements", json!({ "using": "tag name", "value": "div" })));
    assert_eq!(divs.as_array().unwrap().len(), 5);
    assert_eq!(error(session.find("[")).1, "invalid selector");
    let body = element(&ok(session.find("body")));
    let inside = ok(session.post(
        &format!("/element/{body}/elements"),
        json!({ "using": "css selector", "value": "#a, html" }),
    ));
    assert_eq!(
        inside.as_array().unwrap().len(),
        1,
        "the element's descendants alone"
    );
    assert_eq!(
        session.get(&format!("/element/{a}/rect")),
        json!({ "x": 10, "y": 20, "width": 100, "height": 40 })
    );
    assert_eq!(session.get(&format!("/element/{a}/name")), "div");
    assert_eq!(session.get(&format!("/element/{a}/attribute/data-x")), "1");
    assert_eq!(
        session.get(&format!("/element/{a}/attribute/nope")),
        Value::Null
    );
    let p = element(&ok(session.find("p")));
    assert_eq!(
        session.get(&format!("/element/{p}/attribute/hidden")),
        "true",
        "a boolean attribute"
    );
    assert_eq!(session.get(&format!("/element/{a}/property/id")), "a");
    assert_eq!(session.get(&format!("/element/{a}/property/tabIndex")), -1);
    // Clicks: the mouse's input on the DOM.
    let click = |selector: &str| {
        let found = element(&ok(session.find(selector)));
        session.call("POST", &format!("/element/{found}/click"), None)
    };
    ok(click("#count"));
    ok(click("#count"));
    assert_eq!(
        ok(session.execute("return document.getElementById('count').textContent")),
        "2"
    );
    assert_eq!(
        error(click("#under")),
        (400, "element click intercepted".into())
    );
    assert_eq!(
        error(click("#none")),
        (400, "element not interactable".into())
    );
    let count = element(&ok(session.find("#count")));
    ok(session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
        { "type": "pointerMove", "origin": { "element-6066-11e4-a52e-4f735466cecf": count }, "x": 0, "y": 0 },
        { "type": "pointerDown", "button": 0 },
        { "type": "pointerUp", "button": 0 },
    ]}])));
    assert_eq!(
        ok(session.execute("return document.getElementById('count').textContent")),
        "3"
    );
    let shot = session.get(&format!("/element/{a}/screenshot"));
    let ((width, height), pixels) = png(shot.as_str().unwrap());
    assert_eq!((width, height), (100, 40));
    assert_eq!(&pixels[..3], [0, 0, 255]);
    // Taken out: stale.
    ok(session.execute(
        "document.getElementById('a').parentNode.removeChild(document.getElementById('a'))",
    ));
    assert_eq!(
        error(session.call("GET", &format!("/element/{a}/name"), None)),
        (404, "stale element reference".into())
    );
    assert_eq!(
        error(session.post("/frame", json!({ "id": 0 }))).1,
        "no such frame"
    );
    ok(session.post("/frame", json!({ "id": null })));
    // Another renderer's reference, or another document's it never gave.
    let (prefix, rest) = count.split_once('.').unwrap();
    for unknown in [
        format!("0000000000000000.{rest}"),
        format!("{prefix}.999.1"),
    ] {
        assert_eq!(
            error(session.call("GET", &format!("/element/{unknown}/name"), None)),
            (404, "no such element".into()),
            "{unknown}"
        );
    }
    // The viewport's bounds.
    ok(
        session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
            { "type": "pointerMove", "x": 700, "y": 10 },
        ]}])),
    );
    assert_eq!(
        error(
            session.perform(json!([{ "type": "pointer", "id": "mouse", "actions": [
                { "type": "pointerMove", "x": 900, "y": 10 },
            ]}]))
        ),
        (500, "move target out of bounds".into())
    );
    // Element Send Keys to an element that takes no keys.
    assert_eq!(
        error(session.post(&format!("/element/{count}/value"), json!({ "text": "x" }))).1,
        "element not interactable"
    );
    // A new size: script reads it at once, before the next frame; the
    // browser window's too.
    assert_ne!(ok(session.execute("return innerWidth")), 640);
    let rect = ok(session.post("/window/rect", json!({ "width": 640, "height": 480 })));
    let got = ok(session.execute("return [innerWidth, innerHeight, outerWidth, outerHeight]"));
    assert_eq!(
        (&got[0], &got[2], &got[3]),
        (&json!(640), &rect["width"], &rect["height"]),
        "{got}"
    );
    assert!(
        (1..480).contains(&got[1].as_i64().unwrap()),
        "the toolbar's height off: {got}"
    );
    // Another document: the old one's references are stale.
    ok(session.navigate(&server.url("/")));
    assert_eq!(
        error(session.call("GET", &format!("/element/{count}/name"), None)),
        (404, "stale element reference".into())
    );
    session.delete();
}
