//! DevTools' `Log` domain: the page's loads that got a response of 400 or
//! more, or failed, are logged whether or not DevTools is there, printed
//! to stderr, kept for a session that turns `Log` on later and told to the
//! sessions that have it on, each linked to its request in `Network`; a
//! main frame's new document starts afresh. A session that asks of
//! `Network` what machopo can't do is warned in its own log. A warning about
//! what script asked (an observer of an entry type machopo doesn't make) is
//! the page's, as a load's error is.

use net::test_server::{Route, refused_port};
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::devtools::{evaluate, load, page};
use crate::remote_debugging::{Client, Machopo};

/// The entries of the `Log.entryAdded` events that came, in order.
fn entries(page: &mut Client) -> Vec<Value> {
    page.take_events("Log.entryAdded")
        .into_iter()
        .map(|event| event["params"]["entry"].clone())
        .collect()
}

/// The next `Log.entryAdded`'s entry.
fn next_entry(page: &mut Client) -> Value {
    page.event("Log.entryAdded")["params"]["entry"].clone()
}

const NOT_FOUND: &str =
    "Failed to load resource: the server responded with a status of 404 (Not Found)";

#[test]
fn failed_loads_are_logged_and_kept() {
    let html = "<!DOCTYPE html><img src=/missing.png><link rel=stylesheet href=/s.css>";
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        ("/missing.png", Route::new(404, "no")),
        ("/s.css", Route::ok("text/css", "p {}")),
        ("/next", Route::ok("text/html", "<!DOCTYPE html>next")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Network.enable", json!({}));
    load(&mut page, &server.url("/"));
    let missing = server.url("/missing.png");
    assert_eq!(
        machopo.error_line("[console]"),
        format!("[console] {NOT_FOUND} ({missing})")
    );
    let refused_port = refused_port();
    let refused = format!("http://127.0.0.1:{}/x", refused_port.port());
    evaluate(
        &mut page,
        &format!("fetch({refused:?}).catch(() => {{}}); 0"),
        None,
    );
    assert_eq!(
        machopo.error_line("[console]"),
        format!("[console] Failed to load resource: net::ERR_CONNECTION_REFUSED ({refused})")
    );

    // Kept for a session that turns `Log` on later, before its answer.
    let enabled = page.call(None, "Log.enable", json!({}));
    assert!(enabled.get("result").is_some(), "{enabled}");
    let kept = entries(&mut page);
    assert_eq!(kept.len(), 2, "{kept:#?}");
    let (not_found, failed) = (&kept[0], &kept[1]);
    assert_eq!(not_found["source"], "network");
    assert_eq!(not_found["level"], "error");
    assert_eq!(not_found["text"], NOT_FOUND);
    assert_eq!(not_found["url"], missing.as_str());
    assert!(
        not_found["timestamp"].as_f64() > Some(1.6e12),
        "{not_found}"
    );
    assert_eq!(failed["url"], refused.as_str());
    // Each is linked to its request.
    let sent: Vec<Value> = page.take_events("Network.requestWillBeSent");
    for (entry, url) in [(not_found, &missing), (failed, &refused)] {
        let request = sent
            .iter()
            .find(|event| event["params"]["request"]["url"] == url.as_str())
            .unwrap_or_else(|| panic!("no request of {url}: {sent:#?}"));
        assert_eq!(
            entry["networkRequestId"], request["params"]["requestId"],
            "{entry}"
        );
    }

    // Told as it comes, while on.
    evaluate(
        &mut page,
        &format!("fetch({refused:?}).catch(() => {{}}); 0"),
        None,
    );
    assert_eq!(next_entry(&mut page)["url"], refused.as_str());

    // Cleared, and gone with the document.
    page.call(None, "Log.clear", json!({}));
    page.call(None, "Log.disable", json!({}));
    page.call(None, "Log.enable", json!({}));
    assert_eq!(entries(&mut page), Vec::<Value>::new());
    page.call(None, "Log.disable", json!({}));
    load(&mut page, &server.url("/"));
    load(&mut page, &server.url("/next"));
    page.call(None, "Log.enable", json!({}));
    assert_eq!(entries(&mut page), Vec::<Value>::new());
}

/// A `<link>`'s sheet that went nowhere is logged with the URL it asked
/// for (no response tells one).
#[test]
fn a_sheet_that_fails_is_logged_with_its_url() {
    let refused_port = refused_port();
    let refused = format!("http://127.0.0.1:{}/s.css", refused_port.port());
    let html = format!("<!DOCTYPE html><link rel=stylesheet href={refused:?}>");
    let server = server(&[("/", Route::ok("text/html", html))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    assert_eq!(
        machopo.error_line("[console]"),
        format!("[console] Failed to load resource: net::ERR_CONNECTION_REFUSED ({refused})")
    );
}

/// A document of 404 is logged too, linked to its navigation.
#[test]
fn a_document_not_found_is_logged() {
    let server = server(&[("/", Route::new(404, "<!DOCTYPE html>gone"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let entry = next_entry(&mut page);
    assert_eq!(entry["text"], NOT_FOUND);
    assert_eq!(entry["url"], server.url("/").as_str());
    assert_eq!(entry["networkRequestId"], navigated["result"]["loaderId"]);
}

/// `data` as a zlib stream (`Content-Encoding: deflate`) that stops short:
/// one stored block that isn't the last, and no more.
pub(crate) fn deflate_cut_short(data: &[u8]) -> Vec<u8> {
    let length = u16::try_from(data.len()).expect("a stored block's");
    let mut stream = vec![0x78, 0x01, 0x00];
    stream.extend(length.to_le_bytes());
    stream.extend((!length).to_le_bytes());
    stream.extend(data);
    stream
}

/// A document whose body stops short after its head is logged as failed,
/// linked to its navigation (its document keeps what came).
#[test]
fn a_document_whose_body_stops_short_is_logged() {
    let server = server(&[(
        "/",
        Route::ok("text/html", deflate_cut_short(b"<!DOCTYPE html>part"))
            .with_header("Content-Encoding", "deflate"),
    )]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let entry = next_entry(&mut page);
    assert_eq!(
        entry["text"],
        "Failed to load resource: net::ERR_CONTENT_DECODING_FAILED"
    );
    assert_eq!(entry["url"], server.url("/").as_str());
    assert_eq!(entry["networkRequestId"], navigated["result"]["loaderId"]);
}

/// Its markup is what came (`Page.getResourceContent`), once what came is
/// in: an aborted document has no `DOMContentLoaded` or `load` to tell the
/// `Page` agent of it then.
#[test]
fn a_document_whose_body_stops_short_has_what_came_as_its_content() {
    let html = "<!DOCTYPE html>part";
    let server = server(&[(
        "/",
        Route::ok("text/html", deflate_cut_short(html.as_bytes()))
            .with_header("Content-Encoding", "deflate"),
    )]);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    page.call(None, "Page.enable", json!({}));
    page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    // Logged as its end comes.
    next_entry(&mut page);
    let got = page.call(
        None,
        "Page.getResourceContent",
        json!({ "frameId": target, "url": server.url("/") }),
    );
    assert_eq!(got["result"]["content"], html, "{got}");
}

/// What machopo can't do, the renderer's `Network` and the browser's alike,
/// is answered as an error and warned of in the session's log, if it has
/// `Log` on.
#[test]
fn commands_machopo_cant_carry_out_are_warned_of() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    let headers = json!({ "headers": { "X": "1" } });
    let offline = json!({
        "offline": true,
        "latency": 0,
        "downloadThroughput": -1,
        "uploadThroughput": -1,
    });
    // Not without `Log`.
    let answer = page.call(None, "Network.setExtraHTTPHeaders", headers.clone());
    assert!(answer.get("error").is_some(), "{answer}");
    page.call(None, "Network.emulateNetworkConditions", offline.clone());
    assert!(!page.came("Log.entryAdded"));

    page.call(None, "Log.enable", json!({}));
    let answer = page.call(None, "Network.setExtraHTTPHeaders", headers);
    assert_eq!(
        answer["error"]["message"], "machopo can't add headers to requests",
        "{answer}"
    );
    let warning = next_entry(&mut page);
    assert_eq!(warning["source"], "other");
    assert_eq!(warning["level"], "warning");
    assert_eq!(
        warning["text"],
        "Network.setExtraHTTPHeaders: machopo can't add headers to requests"
    );
    let answer = page.call(None, "Network.emulateNetworkConditions", offline);
    assert!(answer.get("error").is_some(), "{answer}");
    let warning = next_entry(&mut page);
    assert_eq!(warning["level"], "warning");
    assert_eq!(
        warning["text"],
        "Network.emulateNetworkConditions: machopo can't take the network offline or throttle it"
    );
    // Not kept: another session that turns `Log` on hears of none.
    page.call(None, "Log.disable", json!({}));
    page.call(None, "Log.enable", json!({}));
    assert_eq!(entries(&mut page), Vec::<Value>::new());
}

/// What script asked that earns a warning is logged as the page's: a
/// `javascript` warning, printed and told to the session, with Chrome
/// 152's text.
#[test]
fn script_warnings_are_logged() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Log.enable", json!({}));
    evaluate(
        &mut page,
        "new PerformanceObserver(() => {}).observe({ type: 'nope' }); 0",
        None,
    );
    let text = "The entry type 'nope' does not exist or isn't supported.";
    assert_eq!(machopo.error_line("[console]"), format!("[console] {text}"));
    let entry = next_entry(&mut page);
    assert_eq!(entry["source"], "javascript");
    assert_eq!(entry["level"], "warning");
    assert_eq!(entry["text"], text);
    evaluate(
        &mut page,
        "new PerformanceObserver(() => {}).observe({ entryTypes: ['mark'], buffered: true }); 0",
        None,
    );
    let text =
        "The PerformanceObserver does not support buffered flag with the entryTypes argument.";
    assert_eq!(machopo.error_line("[console]"), format!("[console] {text}"));
    assert_eq!(next_entry(&mut page)["text"], text);
}

/// A page no DevTools session is attached to logs script's warnings too
/// (printed), as they come: those of its parser's script, and those of a
/// timer's that comes when nothing else does.
#[test]
fn script_warnings_are_logged_without_a_session() {
    let html = "<!DOCTYPE html><script>
         new PerformanceObserver(() => {}).observe({ type: 'first' });
         setTimeout(() => new PerformanceObserver(() => {}).observe({ type: 'later' }), 300);
       </script>";
    let server = server(&[("/", Route::ok("text/html", html))]);
    let machopo = Machopo::start();
    let mut browser = machopo.connect_browser();
    let created = browser.call(
        None,
        "Target.createTarget",
        json!({ "url": server.url("/") }),
    );
    assert!(created.get("result").is_some(), "{created}");
    for name in ["first", "later"] {
        assert_eq!(
            machopo.error_line("[console]"),
            format!("[console] The entry type '{name}' does not exist or isn't supported.")
        );
    }
}

/// `entry` without its time, which no two runs share.
fn timeless(mut entry: Value) -> Value {
    entry.as_object_mut().expect("an entry").remove("timestamp");
    entry
}

/// What the browser refuses a frame, and an `X-Frame-Options` it can't
/// read, is in the page's log as Chrome logs it: `X-Frame-Options`'
/// messages of the frame's document once it commits (its error page, or
/// the document an invalid one didn't refuse), line 0; a `frame-ancestors`
/// violation a `security` message of no URL.
#[test]
fn what_the_browser_refuses_is_logged_as_chrome_logs_it() {
    let html = "<!DOCTYPE html>\
                <iframe src=/deny></iframe>\
                <iframe src=/invalid></iframe>\
                <iframe src=/framed></iframe>";
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        (
            "/deny",
            Route::ok("text/html", "deny").with_header("X-Frame-Options", "DENY"),
        ),
        (
            "/invalid",
            Route::ok("text/html", "invalid").with_header("X-Frame-Options", "bogus"),
        ),
        (
            "/framed",
            Route::ok("text/html", "framed")
                .with_header("Content-Security-Policy", "frame-ancestors 'none'"),
        ),
    ]);
    let origin = server.url("/");
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    load(&mut page, &origin);
    // The frames load side by side: their messages in any order.
    let mut logged: Vec<Value> = (0..3).map(|_| timeless(next_entry(&mut page))).collect();
    logged.sort_by_key(|entry| entry["text"].as_str().map(str::to_owned));
    assert_eq!(
        logged,
        [
            json!({
                "source": "security",
                "level": "error",
                "text": format!("Framing '{origin}' violates the following Content Security Policy directive: \"frame-ancestors 'none'\". The request has been blocked.\n"),
            }),
            json!({
                "source": "other",
                "level": "error",
                "text": format!("Invalid 'X-Frame-Options' header encountered when loading '{origin}': 'bogus' is not a recognized directive. The header will be ignored."),
                "url": server.url("/invalid"),
                "lineNumber": 0,
            }),
            json!({
                "source": "other",
                "level": "error",
                "text": format!("Refused to display '{origin}' in a frame because it set 'X-Frame-Options' to 'deny'."),
                "url": "chrome-error://chromewebdata/",
                "lineNumber": 0,
            }),
        ]
    );
    // Nothing else.
    evaluate(&mut page, "0", None);
    assert_eq!(entries(&mut page), Vec::<Value>::new());
}

/// What ORB blocks isn't logged, as Blink leaves it to the network
/// service's DevTools issues; what CORP blocks is, with its reason. The
/// image CORP blocks is asked for once the one ORB blocks has failed, so
/// that the first message would be ORB's, were there one.
#[test]
fn orb_is_not_logged_and_corp_is_with_its_reason() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        ("/json", Route::ok("application/json", "{\"a\": 1}")),
        (
            "/corp",
            Route::ok("image/png", "png")
                .with_header("Cross-Origin-Resource-Policy", "same-origin"),
        ),
    ]);
    let (json_url, corp_url) = (
        server.url_for_host("localhost", "/json"),
        server.url_for_host("localhost", "/corp"),
    );
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    load(&mut page, &server.url("/"));
    evaluate(
        &mut page,
        &format!(
            "const orb = new Image();\
             orb.onerror = () => {{ new Image().src = {corp_url:?}; }};\
             orb.src = {json_url:?}; 0"
        ),
        None,
    );
    let logged = next_entry(&mut page);
    assert_eq!(
        (&logged["source"], &logged["text"], &logged["url"]),
        (
            &json!("network"),
            &json!("Failed to load resource: net::ERR_BLOCKED_BY_RESPONSE.NotSameOrigin"),
            &json!(corp_url)
        )
    );
}

/// What a sandbox blocks is in the page's log as Chrome logs it (each
/// message as it was measured): a script of a document sandboxed without
/// `allow-scripts` a `security` error at the document's URL, of no line; a
/// navigation of the top-level frame a `javascript` error, a new window
/// without `allow-popups` a `security` one, both at the script's line.
#[test]
fn what_a_sandbox_blocks_is_logged_as_chrome_logs_it() {
    let html = "<!DOCTYPE html>\
                <iframe sandbox srcdoc='<script>0</script>'></iframe>\
                <iframe sandbox=allow-scripts srcdoc=\"<script>\n\
                  try { top.location.href = '/away'; } catch (e) {}</script>\"></iframe>\
                <iframe sandbox=allow-scripts srcdoc=\"<a href=/new target=_blank></a><script>\n\n\
                  document.querySelector('a').click();</script>\"></iframe>";
    let server = server(&[("/", Route::ok("text/html", html))]);
    let origin = server.url("/");
    let top_origin = origin.trim_end_matches('/');
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    load(&mut page, &origin);
    // The frames load side by side: their messages in any order.
    let mut logged: Vec<Value> = (0..3).map(|_| timeless(next_entry(&mut page))).collect();
    logged.sort_by_key(|entry| entry["text"].as_str().map(str::to_owned));
    assert_eq!(
        logged,
        [
            json!({
                "source": "security",
                "level": "error",
                "text": format!("Blocked opening '{origin}new' in a new window because the request was made in a sandboxed frame whose 'allow-popups' permission is not set."),
                "url": "about:srcdoc",
                "lineNumber": 2,
            }),
            json!({
                "source": "security",
                "level": "error",
                "text": "Blocked script execution in 'about:srcdoc' because the document's frame is sandboxed and the 'allow-scripts' permission is not set.",
                "url": "about:srcdoc",
            }),
            json!({
                "source": "javascript",
                "level": "error",
                "text": format!("Unsafe attempt to initiate navigation for frame with origin '{top_origin}' from frame with URL 'about:srcdoc'. The frame attempting navigation of the top-level window is sandboxed, but the flag of 'allow-top-navigation' or 'allow-top-navigation-by-user-activation' is not set.\n"),
                "url": "about:srcdoc",
                "lineNumber": 1,
            }),
        ]
    );
    // Nothing else.
    evaluate(&mut page, "0", None);
    assert_eq!(entries(&mut page), Vec::<Value>::new());
}

/// The user's click on a link to a new window in a frame sandboxed
/// without `allow-popups`: the message is at the frame's document's URL,
/// of no line (no script runs to place it at, as Blink's message with no
/// script on the stack).
#[test]
fn a_new_window_a_click_doesnt_open_is_logged_at_the_document() {
    let html = "<!DOCTYPE html><style>body { margin: 0 } iframe { display: block; border: 0 }</style>\
                <iframe sandbox srcdoc=\"<style>body { margin: 0 } a { display: block; height: 50px }</style>\
                  <a href=/new target=_blank>new</a>\"></iframe>";
    let server = server(&[("/", Route::ok("text/html", html))]);
    let origin = server.url("/");
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Log.enable", json!({}));
    load(&mut page, &origin);
    for kind in ["mousePressed", "mouseReleased"] {
        let click = json!({ "type": kind, "x": 10, "y": 10, "button": "left", "clickCount": 1 });
        page.call(None, "Input.dispatchMouseEvent", click);
    }
    assert_eq!(
        timeless(next_entry(&mut page)),
        json!({
            "source": "security",
            "level": "error",
            "text": format!("Blocked opening '{origin}new' in a new window because the request was made in a sandboxed frame whose 'allow-popups' permission is not set."),
            "url": "about:srcdoc",
        })
    );
}
