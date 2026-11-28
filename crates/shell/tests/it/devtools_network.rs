//! DevTools' `Network` domain: the requests a page's renderer sends are
//! told to each session that turned it on, as they go, redirect and end,
//! with their responses as they came (all their headers, which script may
//! not see all of) and their bodies, kept for `Network.getResponseBody`
//! within each session's own limits; the commands machopo can't carry out are
//! errors, the rest succeed.

use net::test_server::{Route, refused_port};
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::devtools::{evaluate, load, page};
use crate::remote_debugging::{Client, Machopo};

/// The `Network` events up to the `count`th request's end
/// (`loadingFinished` or `loadingFailed`), in order: those that came while
/// waiting for an answer first. The other messages read are kept.
fn network_events(page: &mut Client, count: usize) -> Vec<Value> {
    let is_end = |event: &Value| {
        matches!(
            event["method"].as_str(),
            Some("Network.loadingFinished" | "Network.loadingFailed")
        )
    };
    let mut events = page.take_events("Network.");
    let mut ended = events.iter().filter(|event| is_end(event)).count();
    while ended < count {
        let event = page.read().expect("events");
        if !event["method"]
            .as_str()
            .is_some_and(|method| method.starts_with("Network."))
        {
            page.keep(event);
            continue;
        }
        if is_end(&event) {
            ended += 1;
        }
        events.push(event);
    }
    events
}

/// The params of the first event `method` of request `id`.
fn of<'a>(events: &'a [Value], method: &str, id: &Value) -> &'a Value {
    events
        .iter()
        .find(|event| event["method"] == method && &event["params"]["requestId"] == id)
        .map(|event| &event["params"])
        .unwrap_or_else(|| panic!("no {method} of {id}: {events:#?}"))
}

/// The id of the request for `path` (its first `requestWillBeSent`).
fn id_of(events: &[Value], path: &str) -> Value {
    events
        .iter()
        .find(|event| {
            event["method"] == "Network.requestWillBeSent"
                && event["params"]["request"]["url"]
                    .as_str()
                    .is_some_and(|url| url.ends_with(path))
        })
        .map(|event| event["params"]["requestId"].clone())
        .unwrap_or_else(|| panic!("no request of {path}: {events:#?}"))
}

/// The page's subresources and `fetch()`es: each told from its frame and
/// document, of its type, with its response as it came (a CORS one with
/// the headers script doesn't see) and its timing; their bodies as text or
/// base64.
#[test]
fn requests_are_told_with_their_responses_and_bodies() {
    let other = server(&[(
        "/shared",
        Route::ok("text/plain", "shared")
            .with_header("Access-Control-Allow-Origin", "*")
            .with_header("X-Secret", "1"),
    )]);
    let page_html = "<!DOCTYPE html><link rel=stylesheet href=/s.css><img src=/i.png>";
    let server = server(&[
        ("/", Route::ok("text/html", page_html)),
        ("/s.css", Route::ok("text/css", "p { color: green }")),
        ("/i.png", Route::ok("image/png", vec![0xff, 0x00, 0x10])),
        ("/data.json", Route::ok("application/json", r#"{"a":1}"#)),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    let enabled = page.call(None, "Network.enable", json!({ "maxPostDataSize": 65536 }));
    assert!(enabled.get("result").is_some(), "{enabled}");
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let loader = navigated["result"]["loaderId"].clone();
    // The document's, the stylesheet's and the image's.
    let mut events = network_events(&mut page, 3);
    evaluate(
        &mut page,
        "fetch('/data.json', { method: 'POST', body: 'q=1' }).then(r => r.text()); 0",
        None,
    );
    events.extend(network_events(&mut page, 1));
    let cors = format!(
        "fetch({:?}).then(r => window.secret = r.headers.get('x-secret')); 0",
        other.url("/shared")
    );
    evaluate(&mut page, &cors, None);
    events.extend(network_events(&mut page, 1));

    for (path, kind) in [
        ("/s.css", "Stylesheet"),
        ("/i.png", "Image"),
        ("/data.json", "Fetch"),
        ("/shared", "Fetch"),
    ] {
        let id = id_of(&events, path);
        let sent = of(&events, "Network.requestWillBeSent", &id);
        assert_eq!(sent["type"], kind, "{sent}");
        assert_eq!(sent["frameId"], target.as_str(), "{sent}");
        assert_eq!(sent["loaderId"], loader, "{sent}");
        assert_eq!(sent["documentURL"], server.url("/").as_str(), "{sent}");
        // The parser's (the subresources), or no one's (script asked,
        // but stacks weren't asked for).
        let by = if kind == "Fetch" { "other" } else { "parser" };
        assert_eq!(sent["initiator"]["type"], by, "{sent}");
        let (prefix, number) = id.as_str().unwrap().split_once('.').unwrap();
        assert_eq!(prefix.len(), 16, "{id}");
        assert!(number.parse::<u64>().is_ok(), "{id}");
        let received = of(&events, "Network.responseReceived", &id);
        assert_eq!(received["type"], kind, "{received}");
        assert_eq!(received["response"]["status"], 200, "{received}");
        assert_eq!(received["response"]["protocol"], "http/1.1", "{received}");
        assert_eq!(
            received["response"]["remoteIPAddress"], "127.0.0.1",
            "{received}"
        );
        assert!(received["response"]["timing"]["requestTime"].as_f64() > Some(0.0));
        let finished = of(&events, "Network.loadingFinished", &id);
        assert!(
            finished["encodedDataLength"].as_u64() > Some(0),
            "{finished}"
        );
        assert!(finished["timestamp"].as_f64() >= sent["timestamp"].as_f64());
    }

    let json_id = id_of(&events, "/data.json");
    let received = of(&events, "Network.responseReceived", &json_id);
    assert_eq!(received["response"]["mimeType"], "application/json");
    // Its body came in one piece; its request's body is short enough to be
    // told with it.
    let data = of(&events, "Network.dataReceived", &json_id);
    assert_eq!(
        (&data["dataLength"], &data["encodedDataLength"]),
        (&json!(7), &json!(7))
    );
    let sent = of(&events, "Network.requestWillBeSent", &json_id);
    assert_eq!(sent["request"]["postData"], "q=1", "{sent}");
    let body = page.call(
        None,
        "Network.getResponseBody",
        json!({ "requestId": json_id }),
    );
    assert_eq!(
        body["result"],
        json!({ "body": r#"{"a":1}"#, "base64Encoded": false })
    );
    let image = page.call(
        None,
        "Network.getResponseBody",
        json!({ "requestId": id_of(&events, "/i.png") }),
    );
    assert_eq!(
        image["result"],
        json!({ "body": "/wAQ", "base64Encoded": true })
    );

    // DevTools sees the CORS response's headers as they came; script, the
    // ones exposed.
    let shared = of(
        &events,
        "Network.responseReceived",
        &id_of(&events, "/shared"),
    );
    assert_eq!(shared["response"]["headers"]["x-secret"], "1", "{shared}");
    assert_eq!(evaluate(&mut page, "window.secret", None), Value::Null);

    let unknown = page.call(
        None,
        "Network.getResponseBody",
        json!({ "requestId": "nope" }),
    );
    assert_eq!(
        unknown["error"]["message"],
        "No resource with given identifier found"
    );
    // A new document: the bodies kept go.
    load(&mut page, &server.url("/data.json"));
    let gone = page.call(
        None,
        "Network.getResponseBody",
        json!({ "requestId": json_id }),
    );
    assert!(gone.get("error").is_some(), "{gone}");
}

/// A document whose body stops short after its head ends in one
/// `loadingFailed`, the renderer's (its document committed), after what
/// came of its body.
#[test]
fn a_document_whose_body_stops_short_fails_once() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            crate::devtools_log::deflate_cut_short(b"<!DOCTYPE html>part"),
        )
        .with_header("Content-Encoding", "deflate"),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Network.enable", json!({}));
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let id = navigated["result"]["loaderId"].clone();
    let mut events = network_events(&mut page, 1);
    // Anything after it is told by now.
    evaluate(&mut page, "0", None);
    events.extend(page.take_events("Network."));
    let ends: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event["params"]["requestId"] == id
                && matches!(
                    event["method"].as_str(),
                    Some("Network.loadingFinished" | "Network.loadingFailed")
                )
        })
        .collect();
    assert_eq!(ends.len(), 1, "{events:#?}");
    assert_eq!(ends[0]["method"], "Network.loadingFailed");
    assert_eq!(
        (
            &ends[0]["params"]["errorText"],
            &ends[0]["params"]["canceled"]
        ),
        (&json!("net::ERR_CONTENT_DECODING_FAILED"), &json!(false))
    );
    of(&events, "Network.dataReceived", &id);
}

/// A frame the browser refuses its response (`X-Frame-Options: DENY`)
/// ends in one `loadingFailed`, `net::ERR_BLOCKED_BY_RESPONSE` blocked
/// for `other`, after its `responseReceived`, as Chrome tells it; its
/// error page commits.
#[test]
fn a_refused_frame_fails_after_its_response() {
    let server = server(&[
        (
            "/",
            Route::ok("text/html", "<!DOCTYPE html><iframe src=/framed></iframe>"),
        ),
        (
            "/framed",
            Route::ok("text/html", "<!DOCTYPE html>framed").with_header("X-Frame-Options", "DENY"),
        ),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Network.enable", json!({}));
    load(&mut page, &server.url("/"));
    let mut events = network_events(&mut page, 2);
    evaluate(&mut page, "0", None);
    events.extend(page.take_events("Network."));
    let id = id_of(&events, "/framed");
    let ends: Vec<&Value> = events
        .iter()
        .filter(|event| {
            event["params"]["requestId"] == id
                && matches!(
                    event["method"].as_str(),
                    Some("Network.loadingFinished" | "Network.loadingFailed")
                )
        })
        .collect();
    assert_eq!(ends.len(), 1, "{events:#?}");
    assert_eq!(ends[0]["method"], "Network.loadingFailed");
    assert_eq!(
        (
            &ends[0]["params"]["errorText"],
            &ends[0]["params"]["blockedReason"]
        ),
        (&json!("net::ERR_BLOCKED_BY_RESPONSE"), &json!("other"))
    );
    assert_eq!(
        of(&events, "Network.responseReceived", &id)["response"]["status"],
        json!(200)
    );
}

/// A response CORP blocks fails with its reason, in its `errorText` and as
/// its `blockedReason` (Chrome's `corp-not-same-origin`,
/// `corp-not-same-site`); one ORB blocks with none, as in Chrome.
#[test]
fn a_blocked_response_fails_with_its_reason() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/json", Route::ok("application/json", "{\"a\": 1}")),
        (
            "/corp",
            Route::ok("image/png", "png")
                .with_header("Cross-Origin-Resource-Policy", "same-origin"),
        ),
        (
            "/site",
            Route::ok("image/png", "png").with_header("Cross-Origin-Resource-Policy", "same-site"),
        ),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    // The page is of an IP address, which no other origin is the same
    // site as (CORP's `same-site` takes none).
    let urls: Vec<String> = ["/json", "/corp", "/site"]
        .iter()
        .map(|path| server.url_for_host("localhost", path))
        .collect();
    evaluate(
        &mut page,
        &format!("for (const url of {urls:?}) new Image().src = url; 0"),
        None,
    );
    let events = network_events(&mut page, 3);
    let failed = |path: &str| {
        let params = of(&events, "Network.loadingFailed", &id_of(&events, path));
        (
            params["errorText"].clone(),
            params.get("blockedReason").cloned(),
        )
    };
    assert_eq!(failed("/json"), (json!("net::ERR_BLOCKED_BY_ORB"), None));
    assert_eq!(
        failed("/corp"),
        (
            json!("net::ERR_BLOCKED_BY_RESPONSE.NotSameOrigin"),
            Some(json!("corp-not-same-origin"))
        )
    );
    assert_eq!(
        failed("/site"),
        (
            json!("net::ERR_BLOCKED_BY_RESPONSE.NotSameSite"),
            Some(json!("corp-not-same-site"))
        )
    );
}

/// A `fetch()` is told as it is sent: its redirect, which comes while the
/// script that sent it is busy, is told after it, as the same request's
/// second `requestWillBeSent` (were it told only once the script's task
/// had ended, the redirect would find no request to be of).
#[test]
fn a_fetch_is_told_as_it_is_sent_before_its_redirect() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/moved", Route::redirect(302, "/plain")),
        ("/plain", Route::ok("text/plain", "plain")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    evaluate(
        &mut page,
        "fetch('/moved'); { const until = Date.now() + 200; while (Date.now() < until); } 0",
        None,
    );
    let events = network_events(&mut page, 1);
    let sent = events
        .iter()
        .filter(|event| event["method"] == "Network.requestWillBeSent")
        .count();
    assert_eq!(sent, 2, "{events:?}");
}

/// An image's request is sent in the microtask that updates its image
/// data (Blink's `ImageLoader`), told to DevTools then: before a `fetch()`
/// of a task queued after it.
#[test]
fn an_image_is_requested_in_its_microtask() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/a.png", Route::ok("image/png", "")),
        ("/plain", Route::ok("text/plain", "plain")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    evaluate(
        &mut page,
        "new Image().src = '/a.png'; setTimeout(() => fetch('/plain'), 0); 0",
        None,
    );
    let events = network_events(&mut page, 2);
    let sent: Vec<String> = events
        .iter()
        .filter(|event| event["method"] == "Network.requestWillBeSent")
        .map(|event| {
            event["params"]["request"]["url"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(sent, [server.url("/a.png"), server.url("/plain")]);
}

/// A script script inserts is requested as it is prepared (Blink's
/// `ScriptLoader::PrepareScript`), told to DevTools then: before a
/// `fetch()` of a task queued after it.
#[test]
fn a_script_is_requested_as_it_is_prepared() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/s.js", Route::ok("text/javascript", "0")),
        ("/plain", Route::ok("text/plain", "plain")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    evaluate(
        &mut page,
        "const s = document.createElement('script'); s.src = '/s.js'; document.body.append(s);
         setTimeout(() => fetch('/plain'), 0); 0",
        None,
    );
    let events = network_events(&mut page, 2);
    let sent: Vec<String> = events
        .iter()
        .filter(|event| event["method"] == "Network.requestWillBeSent")
        .map(|event| {
            event["params"]["request"]["url"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect();
    assert_eq!(sent, [server.url("/s.js"), server.url("/plain")]);
}

/// A `fetch()`'s `requestWillBeSent` is stamped as it is sent, not once the
/// script that sent it is done: before its `loadingFinished`, which may
/// come while the script is busy.
#[test]
fn a_fetch_is_stamped_as_it_is_sent() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/plain", Route::ok("text/plain", "plain")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    evaluate(
        &mut page,
        "fetch('/plain'); { const until = Date.now() + 200; while (Date.now() < until); } 0",
        None,
    );
    let events = network_events(&mut page, 1);
    let at = |method: &str| {
        events
            .iter()
            .find(|event| event["method"] == method)
            .and_then(|event| event["params"]["timestamp"].as_f64())
            .expect(method)
    };
    assert!(
        at("Network.loadingFinished") >= at("Network.requestWillBeSent"),
        "{events:?}"
    );
}

/// A document replaced by the next takes its `fetch()`es still on their
/// way with it, the one its `pagehide` sends too: each ends in a cancelled
/// `loadingFailed` (Chrome cancels a gone context's loads), not in its
/// response; the same when the next is a wasm document, the page's engine
/// replaced with the document.
#[test]
fn a_replaced_documents_fetches_are_cancelled() {
    use std::time::Duration;
    let slow = || Route::ok("text/plain", "late").with_delay(Duration::from_secs(2));
    for next in ["/js", "/wasm"] {
        let server = server(&[
            (
                "/",
                Route::ok(
                    "text/html",
                    "<!DOCTYPE html><script>onpagehide = () => fetch('/hidden');</script>page",
                ),
            ),
            ("/slow", slow()),
            ("/hidden", slow()),
            ("/js", Route::ok("text/html", "<!DOCTYPE html>next")),
            (
                "/wasm",
                Route::ok(
                    "text/html",
                    r#"<!DOCTYPE html><meta name="script" content="wasm">next"#,
                ),
            ),
        ]);
        let machopo = Machopo::start();
        let (mut page, _) = page(&machopo);
        load(&mut page, &server.url("/"));
        page.call(None, "Network.enable", json!({}));
        evaluate(&mut page, "fetch('/slow'); 0", None);
        load(&mut page, &server.url(next));
        // Each fetch's request, and how it ended.
        let mut events = page.take_events("Network.");
        let ended = |events: &[Value], path: &str| {
            let id = events
                .iter()
                .find(|event| {
                    event["method"] == "Network.requestWillBeSent"
                        && event["params"]["request"]["url"]
                            .as_str()
                            .is_some_and(|url| url.ends_with(path))
                })
                .map(|event| event["params"]["requestId"].clone())?;
            events
                .iter()
                .find(|event| {
                    event["params"]["requestId"] == id
                        && matches!(
                            event["method"].as_str(),
                            Some("Network.loadingFinished" | "Network.loadingFailed")
                        )
                })
                .cloned()
        };
        while ended(&events, "/slow").is_none() || ended(&events, "/hidden").is_none() {
            events.push(page.read().expect("events"));
        }
        for path in ["/slow", "/hidden"] {
            let end = ended(&events, path).expect("ended");
            assert_eq!(
                (end["method"].as_str(), end["params"]["canceled"].as_bool()),
                (Some("Network.loadingFailed"), Some(true)),
                "{next} {path}: {end}"
            );
        }
    }
}

/// A redirect is another `requestWillBeSent` of the same request, with the
/// redirect's response; a load that fails, and one cancelled, end in
/// `loadingFailed`.
#[test]
fn redirects_and_failures_are_told() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/moved", Route::redirect(302, "/plain")),
        ("/plain", Route::ok("text/plain", "plain")),
        (
            "/slow",
            Route::ok("text/plain", "slow").with_delay(std::time::Duration::from_millis(500)),
        ),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));

    evaluate(&mut page, "fetch('/moved'); 0", None);
    let events = network_events(&mut page, 1);
    let sent: Vec<&Value> = events
        .iter()
        .filter(|event| event["method"] == "Network.requestWillBeSent")
        .map(|event| &event["params"])
        .collect();
    assert_eq!(sent.len(), 2, "{events:#?}");
    assert_eq!(sent[0]["requestId"], sent[1]["requestId"]);
    assert!(sent[0].get("redirectResponse").is_none());
    assert_eq!(sent[1]["redirectResponse"]["status"], 302, "{}", sent[1]);
    assert_eq!(sent[1]["request"]["url"], server.url("/plain").as_str());
    let received = of(&events, "Network.responseReceived", &sent[0]["requestId"]);
    assert_eq!(received["response"]["url"], server.url("/plain").as_str());

    let refused_port = refused_port();
    let refused = format!("http://127.0.0.1:{}/", refused_port.port());
    evaluate(
        &mut page,
        &format!("fetch({refused:?}).catch(() => 0); 0"),
        None,
    );
    let events = network_events(&mut page, 1);
    let failed = &events.last().unwrap()["params"];
    assert_eq!(
        (&failed["errorText"], &failed["canceled"]),
        (&json!("net::ERR_CONNECTION_REFUSED"), &json!(false)),
        "{failed}"
    );

    evaluate(
        &mut page,
        "const c = new AbortController(); fetch('/slow', { signal: c.signal }).catch(() => 0); c.abort(); 0",
        None,
    );
    let events = network_events(&mut page, 1);
    let cancelled = &events.last().unwrap()["params"];
    assert_eq!(
        (&cancelled["errorText"], &cancelled["canceled"]),
        (&json!("net::ERR_ABORTED"), &json!(true)),
        "{cancelled}"
    );
}

/// Each session keeps its own: what it turned on, its limits, the bodies
/// it holds and the requests' bodies; one without `Network` on hears of
/// nothing.
#[test]
fn each_session_keeps_its_own() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/a", Route::ok("text/plain", "aaaaaaaaaa")),
        ("/b", Route::ok("text/plain", "bbbbbbbbbb")),
        ("/post", Route::ok("text/plain", "posted")),
    ]);
    let machopo = Machopo::start();
    let (mut first, _) = page(&machopo);
    let (mut second, target) = page(&machopo);
    let (mut quiet, _) = page(&machopo);
    load(&mut first, &server.url("/"));
    first.call(
        None,
        "Network.enable",
        json!({ "maxTotalBufferSize": 15, "maxPostDataSize": 3 }),
    );
    second.call(
        None,
        "Network.enable",
        json!({ "maxResourceBufferSize": 5 }),
    );

    // One after the other: the older is the one dropped.
    evaluate(&mut first, "fetch('/a'); 0", None);
    let mut firsts = network_events(&mut first, 1);
    evaluate(&mut first, "fetch('/b'); 0", None);
    firsts.extend(network_events(&mut first, 1));
    let seconds = network_events(&mut second, 2);
    let body = |client: &mut Client, events: &[Value], path: &str| {
        client.call(
            None,
            "Network.getResponseBody",
            json!({ "requestId": id_of(events, path) }),
        )
    };
    // Ten bytes each, fifteen in all: `/a` made way for `/b`.
    let evicted = "Request content was evicted from inspector cache";
    assert_eq!(body(&mut first, &firsts, "/a")["error"]["message"], evicted);
    assert_eq!(
        body(&mut first, &firsts, "/b")["result"]["body"],
        "bbbbbbbbbb"
    );
    // Five bytes a body at most: neither.
    assert_eq!(
        body(&mut second, &seconds, "/a")["error"]["message"],
        evicted
    );
    assert_eq!(
        body(&mut second, &seconds, "/b")["error"]["message"],
        evicted
    );

    evaluate(
        &mut first,
        "fetch('/post', { method: 'POST', body: 'x=12345' }); 0",
        None,
    );
    let firsts = network_events(&mut first, 1);
    let sent = of(
        &firsts,
        "Network.requestWillBeSent",
        &id_of(&firsts, "/post"),
    );
    assert_eq!(sent["request"]["hasPostData"], true, "{sent}");
    assert!(
        sent["request"].get("postData").is_none(),
        "longer than 3: {sent}"
    );
    let post = first.call(
        None,
        "Network.getRequestPostData",
        json!({ "requestId": id_of(&firsts, "/post") }),
    );
    assert_eq!(post["result"]["postData"], "x=12345");
    network_events(&mut second, 1);

    // One session turning `Network` off leaves the other's on.
    second.call(None, "Network.disable", json!({}));
    let after = body(&mut second, &seconds, "/b");
    assert_eq!(
        after["error"]["message"],
        "No resource with given identifier found"
    );
    evaluate(&mut first, "fetch('/a'); 0", None);
    network_events(&mut first, 1);
    assert_eq!(
        evaluate(&mut quiet, "1", None),
        1,
        "the quiet session is still there"
    );
    assert!(!quiet.came("Network.requestWillBeSent"));
    assert!(!second.came("Network.requestWillBeSent"));
    assert!(!target.is_empty());
}

/// An iframe's document is a request of its own, from the child frame: its
/// id is the loader of the document it brings.
#[test]
fn an_iframes_document_is_its_own_request() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>top")),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>child")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Page.enable", json!({}));
    page.call(None, "Network.enable", json!({}));
    evaluate(
        &mut page,
        &format!(
            "const f = document.createElement('iframe'); f.src = '{}'; document.body.append(f); 0",
            server.url("/child")
        ),
        None,
    );
    let child = page.event("Page.frameAttached")["params"]["frameId"].clone();
    let sent = page.event("Network.requestWillBeSent")["params"].clone();
    let id = sent["requestId"].clone();
    assert_eq!(
        (&sent["type"], &sent["frameId"], &sent["loaderId"]),
        (&json!("Document"), &child, &id),
        "{sent}"
    );
    let navigated = page.event("Page.frameNavigated")["params"]["frame"].clone();
    assert_eq!(navigated["loaderId"], id, "{navigated}");
}

/// What holds in machopo anyway succeeds (no service workers, the network
/// as it is); what it can't do is an error. Cookies are
/// `devtools_cookies`'.
#[test]
fn commands_machopo_cant_carry_out_are_errors() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    let ok = |page: &mut Client, method: &str, params: Value| {
        let answer = page.call(None, method, params);
        assert!(answer.get("result").is_some(), "{method}: {answer}");
        answer["result"].clone()
    };
    let error = |page: &mut Client, method: &str, params: Value| {
        let answer = page.call(None, method, params);
        assert!(answer.get("error").is_some(), "{method}: {answer}");
    };
    error(
        &mut page,
        "Network.setAttachDebugStack",
        json!({ "enabled": true }),
    );
    ok(&mut page, "Network.enable", json!({}));
    ok(
        &mut page,
        "Network.setAttachDebugStack",
        json!({ "enabled": true }),
    );
    ok(
        &mut page,
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": true }),
    );
    ok(
        &mut page,
        "Network.setBypassServiceWorker",
        json!({ "bypass": true }),
    );
    ok(
        &mut page,
        "Network.setBlockedURLs",
        json!({ "urlPatterns": [] }),
    );
    error(
        &mut page,
        "Network.setBlockedURLs",
        json!({ "urlPatterns": [{ "urlPattern": "*", "block": true }] }),
    );
    ok(
        &mut page,
        "Network.setExtraHTTPHeaders",
        json!({ "headers": {} }),
    );
    error(
        &mut page,
        "Network.setExtraHTTPHeaders",
        json!({ "headers": { "X": "1" } }),
    );
    let state = |offline: bool, latency: f64| {
        json!({
            "offline": offline,
            "latency": latency,
            "downloadThroughput": -1,
            "uploadThroughput": -1,
        })
    };
    ok(&mut page, "Network.overrideNetworkState", state(false, 0.0));
    error(&mut page, "Network.overrideNetworkState", state(true, 0.0));
    error(
        &mut page,
        "Network.emulateNetworkConditions",
        state(false, 100.0),
    );
    ok(
        &mut page,
        "Network.emulateNetworkConditions",
        state(false, 0.0),
    );
    let rules = ok(
        &mut page,
        "Network.emulateNetworkConditionsByRule",
        json!({ "offline": false, "matchedNetworkConditions": [] }),
    );
    assert_eq!(rules, json!({ "ruleIds": [] }));
    error(
        &mut page,
        "Network.emulateNetworkConditionsByRule",
        json!({ "offline": true, "matchedNetworkConditions": [] }),
    );
    error(
        &mut page,
        "Network.emulateNetworkConditionsByRule",
        json!({
            "matchedNetworkConditions": [{
                "urlPattern": "",
                "latency": 100,
                "downloadThroughput": -1,
                "uploadThroughput": -1,
            }],
        }),
    );
    ok(&mut page, "Network.clearBrowserCache", json!({}));
    assert_eq!(
        ok(&mut page, "Network.canEmulateNetworkConditions", json!({})),
        json!({ "result": false })
    );
}

/// A response from the HTTP cache is told as `fromDiskCache`; while a
/// session with `Network` on has the cache disabled, the tab's navigations
/// and subresources go past it, until it turns it back on, turns `Network`
/// off or goes; `clearBrowserCache` empties it.
#[test]
fn devtools_sees_the_http_cache_and_can_go_past_it_or_clear_it() {
    let fresh = |content_type: &str, body: &str| {
        Route::ok(content_type, body).with_header("Cache-Control", "max-age=60")
    };
    let server = server(&[
        (
            "/",
            fresh(
                "text/html",
                "<!DOCTYPE html><link rel=stylesheet href=/s.css><img src=/i.png>\
                 <script>fetch('/f')</script>page",
            ),
        ),
        ("/s.css", fresh("text/css", "p {}")),
        ("/i.png", fresh("image/png", "")),
        ("/f", fresh("text/plain", "")),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    let ok = |page: &mut Client, session: Option<&str>, method: &str, params: Value| {
        let answer = page.call(session, method, params);
        assert!(answer.get("result").is_some(), "{method}: {answer}");
    };
    // Loads `/`: whether the document, its stylesheet, image and fetch came
    // from the cache, and how many times each has reached the server.
    let paths = ["/", "/s.css", "/i.png", "/f"];
    let visit = |page: &mut Client| {
        load(page, &server.url("/"));
        let events = network_events(page, paths.len());
        paths.map(|path| {
            let response =
                &of(&events, "Network.responseReceived", &id_of(&events, path))["response"];
            let reached = server
                .requests()
                .iter()
                .filter(|request| request.target == path)
                .count();
            (response["fromDiskCache"] == true, reached)
        })
    };
    ok(&mut page, None, "Network.enable", json!({}));
    assert_eq!(visit(&mut page), [(false, 1); 4]);
    assert_eq!(visit(&mut page), [(true, 1); 4]);

    ok(
        &mut page,
        None,
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": true }),
    );
    assert_eq!(visit(&mut page), [(false, 2); 4]);
    for path in paths {
        let request = server
            .requests()
            .into_iter()
            .rfind(|request| request.target == path)
            .unwrap();
        assert_eq!(request.header("cache-control"), Some("no-cache"), "{path}");
        assert_eq!(request.header("pragma"), Some("no-cache"), "{path}");
    }
    // What went past it is kept.
    ok(
        &mut page,
        None,
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": false }),
    );
    assert_eq!(visit(&mut page), [(true, 2); 4]);

    ok(&mut page, None, "Network.clearBrowserCache", json!({}));
    assert_eq!(visit(&mut page), [(false, 3); 4]);

    ok(
        &mut page,
        None,
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": true }),
    );
    ok(&mut page, None, "Network.disable", json!({}));
    ok(&mut page, None, "Network.enable", json!({}));
    assert_eq!(visit(&mut page), [(true, 3); 4]);

    // Another session's: the tab goes past the cache while one of them has
    // it disabled.
    let mut browser = machopo.connect_browser();
    let attach = |browser: &mut Client, target: &str| {
        browser.call(
            None,
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
        )["result"]["sessionId"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    let disable_cache = |browser: &mut Client, session: &str| {
        ok(browser, Some(session), "Network.enable", json!({}));
        ok(
            browser,
            Some(session),
            "Network.setCacheDisabled",
            json!({ "cacheDisabled": true }),
        );
    };
    let session = attach(&mut browser, &target);
    // Not while its `Network` is off.
    let refused = browser.call(
        Some(&session),
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": true }),
    );
    assert!(refused.get("error").is_some(), "{refused}");
    assert_eq!(visit(&mut page), [(true, 3); 4]);
    disable_cache(&mut browser, &session);
    assert_eq!(visit(&mut page), [(false, 4); 4]);
    ok(
        &mut browser,
        None,
        "Target.detachFromTarget",
        json!({ "sessionId": session }),
    );
    assert_eq!(visit(&mut page), [(true, 4); 4]);

    // Another tab's session: that tab's.
    let other = browser.call(None, "Target.createTarget", json!({ "url": "about:blank" }));
    let other = other["result"]["targetId"].as_str().unwrap().to_owned();
    let other = attach(&mut browser, &other);
    disable_cache(&mut browser, &other);
    // This tab's own, told again, isn't overruled by it.
    ok(
        &mut page,
        None,
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": false }),
    );
    assert_eq!(visit(&mut page), [(true, 4); 4]);

    // A client gone takes its sessions' with it.
    let session = attach(&mut browser, &target);
    disable_cache(&mut browser, &session);
    assert_eq!(visit(&mut page), [(false, 5); 4]);
    drop(browser);
    let start = std::time::Instant::now();
    while !visit(&mut page).iter().all(|(cached, _)| *cached) {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(10),
            "the cache stays disabled"
        );
    }
}

/// A navigation of the main frame is a request too: the browser tells of
/// it as it starts, redirects and its response comes, the renderer of its
/// body as its document commits; then the document's `DOMContentLoaded`
/// and `load`. Its id is its loader; its body is kept.
#[test]
fn a_navigation_is_told_by_the_browser_and_the_renderer() {
    let html = "<!DOCTYPE html><title>Here</title>";
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>first")),
        ("/old", Route::redirect(302, "/new")),
        ("/new", Route::ok("text/html", html)),
    ]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Page.enable", json!({}));
    page.call(
        None,
        "Page.setLifecycleEventsEnabled",
        json!({ "enabled": true }),
    );
    page.call(None, "Network.enable", json!({}));
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/old") }));
    let loader = navigated["result"]["loaderId"].clone();
    let events = network_events(&mut page, 1);
    let methods: Vec<&str> = events
        .iter()
        .filter(|event| event["params"]["requestId"] == loader)
        .map(|event| event["method"].as_str().unwrap())
        .collect();
    assert_eq!(
        methods,
        [
            "Network.requestWillBeSent",
            "Network.requestWillBeSent",
            "Network.responseReceived",
            "Network.dataReceived",
            "Network.loadingFinished",
        ],
        "{events:#?}"
    );
    let sent = of(&events, "Network.requestWillBeSent", &loader);
    assert_eq!(
        (&sent["type"], &sent["loaderId"], &sent["frameId"]),
        (&json!("Document"), &loader, &json!(target)),
        "{sent}"
    );
    assert_eq!(sent["request"]["url"], server.url("/old").as_str());
    let redirected = events
        .iter()
        .find(|event| event["params"].get("redirectResponse").is_some())
        .unwrap();
    assert_eq!(redirected["params"]["redirectResponse"]["status"], 302);
    assert_eq!(
        redirected["params"]["request"]["url"],
        server.url("/new").as_str()
    );
    let received = of(&events, "Network.responseReceived", &loader);
    assert_eq!(received["response"]["url"], server.url("/new").as_str());
    assert_eq!(received["response"]["mimeType"], "text/html");
    // Its body as it came (here at once); what came over the network,
    // only its end tells.
    let data = of(&events, "Network.dataReceived", &loader);
    assert_eq!(
        (&data["dataLength"], &data["encodedDataLength"]),
        (&json!(html.len()), &json!(0))
    );
    let finished = of(&events, "Network.loadingFinished", &loader);
    assert!(finished["encodedDataLength"].as_u64() > Some(html.len() as u64));
    // The same clock as the browser's own events.
    assert!(finished["timestamp"].as_f64() >= sent["timestamp"].as_f64());
    let body = page.call(
        None,
        "Network.getResponseBody",
        json!({ "requestId": loader }),
    );
    assert_eq!(body["result"]["body"], html, "{body}");

    let dom_content_loaded = page.event("Page.domContentEventFired")["params"]["timestamp"].clone();
    let loaded = page.event("Page.loadEventFired")["params"]["timestamp"].clone();
    assert!(dom_content_loaded.as_f64() >= sent["timestamp"].as_f64());
    assert!(loaded.as_f64() >= dom_content_loaded.as_f64());
    // The lifecycle events, on the same clock.
    let lifecycle = loop {
        let event = page.event("Page.lifecycleEvent")["params"].clone();
        if event["name"] == "load" && event["loaderId"] == loader {
            break event;
        }
    };
    let (at, sent_at) = (
        lifecycle["timestamp"].as_f64().unwrap(),
        sent["timestamp"].as_f64().unwrap(),
    );
    assert!(
        at >= sent_at && at - sent_at < 30.0,
        "{lifecycle} after {sent}"
    );
}

/// A navigation that fails is a failed request; its error page is no
/// request of its own.
#[test]
fn a_navigation_that_fails_is_a_failed_request() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>first"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    let refused_port = refused_port();
    let refused = format!("http://127.0.0.1:{}/", refused_port.port());
    let navigated = page.call(None, "Page.navigate", json!({ "url": refused }));
    let loader = navigated["result"]["loaderId"].clone();
    let events = network_events(&mut page, 1);
    let failed = of(&events, "Network.loadingFailed", &loader);
    assert_eq!(
        failed["errorText"], "net::ERR_CONNECTION_REFUSED",
        "{failed}"
    );
    assert_eq!(failed["type"], "Document");
    assert!(
        !events
            .iter()
            .any(|event| event["method"] == "Network.responseReceived"),
        "{events:#?}"
    );
    // Off, the browser tells of no more navigations.
    page.call(None, "Network.disable", json!({}));
    load(&mut page, &server.url("/"));
    assert!(!page.came("Network.requestWillBeSent"));
}

/// The browser's own pages make no request.
#[test]
fn the_browsers_own_pages_are_no_requests() {
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Network.enable", json!({}));
    load(&mut page, "about:blank");
    assert!(!page.came("Network.requestWillBeSent"));
}

/// DevTools' reload: `Page.reload` with its parameters reloads the page,
/// whose requests the session hears of; a script to run on load machopo can't,
/// nor reload a document no longer shown.
#[test]
fn devtools_reloads_the_page() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    let loader =
        page.call(None, "Page.getFrameTree", json!({}))["result"]["frameTree"]["frame"]["loaderId"]
            .clone();
    let script = page.call(
        None,
        "Page.reload",
        json!({ "ignoreCache": true, "scriptToEvaluateOnLoad": "1" }),
    );
    assert!(script.get("error").is_some(), "{script}");
    let stale = page.call(None, "Page.reload", json!({ "loaderId": "0" }));
    assert!(stale.get("error").is_some(), "{stale}");
    let reloaded = page.call(
        None,
        "Page.reload",
        json!({ "ignoreCache": false, "loaderId": loader }),
    );
    assert!(reloaded.get("result").is_some(), "{reloaded}");
    let events = network_events(&mut page, 1);
    let sent = &events[0]["params"];
    assert_eq!(
        (&sent["type"], &sent["request"]["url"]),
        (&json!("Document"), &json!(server.url("/"))),
        "{sent}"
    );
    assert_ne!(sent["loaderId"], loader, "a new document");
}
