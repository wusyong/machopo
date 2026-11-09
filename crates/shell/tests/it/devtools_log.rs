//! DevTools' `Log` domain: the page's loads that got a response of 400 or
//! more, or failed, are logged whether or not DevTools is there, printed
//! to stderr, kept for a session that turns `Log` on later and told to the
//! sessions that have it on, each linked to its request in `Network`; a
//! main frame's new document starts afresh. A session that asks of
//! `Network` what machopo can't do is warned in its own log. A warning about
//! what script asked (an observer of an entry type machopo doesn't make) is
//! the page's, as a load's error is.

use net::test_server::{Route, unused_port};
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
    let refused = format!("http://127.0.0.1:{}/x", unused_port());
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
