//! A page's lifecycle as DevTools' `Page` domain tells it: each frame's
//! document's `init`, `DOMContentLoaded`, `load`, `networkAlmostIdle` and
//! `networkIdle` (`Page.lifecycleEvent`, from the renderer, as Blink's),
//! `frameStartedLoading` and `frameStoppedLoading`; told again as the
//! events are turned on. `Page.navigate` answered as its navigation is
//! about to commit, with what went wrong. `Target.setAutoAttach`,
//! `detachFromTarget`, and `detachedFromTarget` as a tab goes. What
//! Puppeteer's `goto(…, { waitUntil: 'networkidle0' })` waits on.

use std::time::{Duration, Instant};

use net::test_server::{Route, unused_port};
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::devtools::{evaluate, page};
use crate::remote_debugging::{Client, Machopo};

/// `Page` and its lifecycle events on.
fn enable(page: &mut Client) {
    page.call(None, "Page.enable", json!({}));
    page.call(
        None,
        "Page.setLifecycleEventsEnabled",
        json!({ "enabled": true }),
    );
}

/// The `Page.` events up to (and with) the one `last` picks, in order: those
/// that came while waiting for answers first. The other messages are kept.
fn page_events_until(page: &mut Client, last: impl Fn(&Value) -> bool) -> Vec<Value> {
    let mut events = page.take_events("Page.");
    while !events.iter().any(&last) {
        let event = page.read().expect("events");
        if event["method"]
            .as_str()
            .is_some_and(|method| method.starts_with("Page."))
        {
            events.push(event);
        } else {
            page.keep(event);
        }
    }
    events
}

/// `events` as `method frame name`, for comparing orders; `Page.
/// lifecycleEvent`s as their names, frames as `main` or `child`.
fn outline(events: &[Value], main: &str) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| {
            let params = &event["params"];
            let frame = params["frameId"]
                .as_str()
                .or_else(|| params["frame"]["id"].as_str())?;
            let frame = if frame == main { "main" } else { "child" };
            let what = match event["method"].as_str()? {
                "Page.lifecycleEvent" => params["name"].as_str()?.to_owned(),
                method => method.trim_start_matches("Page.").to_owned(),
            };
            Some(format!("{frame} {what}"))
        })
        .collect()
}

/// The lifecycle event `name` of `frame`, among `events`.
fn lifecycle<'a>(events: &'a [Value], frame: &str, name: &str) -> &'a Value {
    events
        .iter()
        .find(|event| {
            event["method"] == "Page.lifecycleEvent"
                && event["params"]["frameId"] == frame
                && event["params"]["name"] == name
        })
        .map(|event| &event["params"])
        .unwrap_or_else(|| panic!("no {name} of {frame}: {events:#?}"))
}

/// What Puppeteer's `goto` waits on: `Page.navigate`'s loader, then that
/// loader's `init`, `DOMContentLoaded`, `load`, its network almost idle
/// and idle (a `fetch()` 300 ms long after `load` puts them off, each at
/// when its quiet began); the frame started and stopped loading, as
/// Chrome tells them, the child frame's document's in between.
#[test]
fn a_navigation_is_told_through_its_lifecycle() {
    a_navigation_is_told_through_its_lifecycle_with(&[]);
}

fn a_navigation_is_told_through_its_lifecycle_with(args: &[&str]) {
    let html = "<!DOCTYPE html><iframe src='/child'></iframe>\
                <script>addEventListener('load', () => fetch('/slow'))</script>";
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>child")),
        (
            "/slow",
            Route::ok("text/plain", "slow").with_delay(Duration::from_millis(300)),
        ),
    ]);
    let machopo = Machopo::start_with(args);
    let (mut page, main) = page(&machopo);
    enable(&mut page);
    page.take_events("Page.");
    let started = Instant::now();
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let loader = navigated["result"]["loaderId"].clone();
    assert_eq!(navigated["result"]["frameId"], main.as_str(), "{navigated}");
    assert_eq!(navigated["result"]["isDownload"], false, "{navigated}");
    assert!(
        navigated["result"].get("errorText").is_none(),
        "{navigated}"
    );
    let events = page_events_until(&mut page, |event| {
        event["params"]["name"] == "networkIdle" && event["params"]["frameId"] == main.as_str()
    });
    // Off by then: half a second of quiet after the fetch.
    assert!(started.elapsed() >= Duration::from_millis(800));
    let order = outline(&events, &main);
    let at = |what: &str| {
        order
            .iter()
            .position(|told| told == what)
            .unwrap_or_else(|| panic!("no {what}: {order:#?}"))
    };
    for (before, after) in [
        ("main frameStartedLoading", "main init"),
        ("main init", "main frameNavigated"),
        ("main frameNavigated", "child frameAttached"),
        ("child frameAttached", "child init"),
        ("main DOMContentLoaded", "main load"),
        ("child DOMContentLoaded", "child load"),
        ("child load", "child frameStoppedLoading"),
        ("child load", "main load"),
        ("main load", "main frameStoppedLoading"),
        ("main load", "main networkAlmostIdle"),
        ("main networkAlmostIdle", "main networkIdle"),
    ] {
        assert!(
            at(before) < at(after),
            "{before} before {after}: {order:#?}"
        );
    }
    // The child frame's initial `about:blank` starts too, as Blink's.
    let child_inits = order.iter().filter(|told| *told == "child init").count();
    assert_eq!(child_inits, 2, "{order:#?}");
    let init = lifecycle(&events, &main, "init");
    assert_eq!(init["loaderId"], loader, "the navigation's loader: {init}");
    let load = lifecycle(&events, &main, "load");
    let idle = lifecycle(&events, &main, "networkIdle");
    for event in [load, idle] {
        assert_eq!(event["loaderId"], loader, "{event}");
    }
    // The quiet began as the fetch ended, 300 ms after `load`.
    let (load_at, idle_at) = (
        load["timestamp"].as_f64().unwrap(),
        idle["timestamp"].as_f64().unwrap(),
    );
    assert!(idle_at - load_at >= 0.25, "{load} {idle}");
    // The child frame's document's, by its own loader.
    let child = events
        .iter()
        .find(|event| event["method"] == "Page.frameAttached")
        .map(|event| event["params"]["frameId"].as_str().unwrap().to_owned())
        .unwrap();
    let child_load = lifecycle(&events, &child, "load");
    assert_ne!(child_load["loaderId"], loader, "{child_load}");
}

/// A subresource on its way keeps the network from being idle, though no
/// `fetch()` is: the quiet begins as the image comes, after
/// `DOMContentLoaded`.
#[test]
fn a_subresource_on_its_way_keeps_the_network_busy() {
    a_subresource_on_its_way_keeps_the_network_busy_with(&[]);
}

fn a_subresource_on_its_way_keeps_the_network_busy_with(args: &[&str]) {
    let html = "<!DOCTYPE html><img src='/slow.png'>";
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        (
            "/slow.png",
            Route::ok("image/png", vec![0x89]).with_delay(Duration::from_millis(400)),
        ),
    ]);
    let machopo = Machopo::start_with(args);
    let (mut page, main) = page(&machopo);
    enable(&mut page);
    page.take_events("Page.");
    page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let events = page_events_until(&mut page, |event| {
        event["params"]["name"] == "networkIdle" && event["params"]["frameId"] == main.as_str()
    });
    let loaded = lifecycle(&events, &main, "DOMContentLoaded")["timestamp"]
        .as_f64()
        .unwrap();
    let idle = lifecycle(&events, &main, "networkIdle")["timestamp"]
        .as_f64()
        .unwrap();
    // At when the quiet began: the image came, not half a second after.
    assert!(
        (0.3..0.8).contains(&(idle - loaded)),
        "idle at {idle}, DOMContentLoaded at {loaded}"
    );
}

/// An iframe with no `src` keeps its initial `about:blank`: loaded as it
/// starts, its whole lifecycle is told (as Chrome's: Puppeteer waits for
/// every frame's), and it isn't navigated. One whose document is on its
/// way has only its initial document's `init` before it.
#[test]
fn an_iframe_that_stays_blank_has_loaded() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        (
            "/slow",
            Route::ok("text/html", "<!DOCTYPE html>slow").with_delay(Duration::from_millis(300)),
        ),
    ]);
    let machopo = Machopo::start();
    let (mut page, main) = page(&machopo);
    enable(&mut page);
    crate::devtools::load(&mut page, &server.url("/"));
    page.take_events("Page.");
    evaluate(
        &mut page,
        "document.body.append(document.createElement('iframe')); 0",
        None,
    );
    let events = page_events_until(&mut page, |event| {
        event["params"]["name"] == "networkIdle" && event["params"]["frameId"] != main.as_str()
    });
    // The main frame's last load may still be ending.
    let child: Vec<String> = outline(&events, &main)
        .into_iter()
        .filter(|told| told.starts_with("child"))
        .collect();
    assert_eq!(
        child,
        [
            "child frameAttached",
            "child init",
            "child frameStartedLoading",
            "child DOMContentLoaded",
            "child load",
            "child frameStoppedLoading",
            "child networkAlmostIdle",
            "child networkIdle",
        ]
    );

    evaluate(
        &mut page,
        "document.body.lastChild.remove(); const f = document.createElement('iframe'); \
         f.src = '/slow'; document.body.append(f); 0",
        None,
    );
    let events = page_events_until(&mut page, |event| event["method"] == "Page.frameNavigated");
    let child: Vec<String> = outline(&events, &main)
        .into_iter()
        .filter(|told| told.starts_with("child") && !told.ends_with("frameDetached"))
        .collect();
    assert_eq!(
        child,
        [
            "child frameAttached",
            "child init",
            "child frameStartedLoading",
            "child init",
            "child frameNavigated",
        ]
    );
}

/// A tab's new renderer (the last one ended) has `Page` and the lifecycle
/// events on as they were: the next document's are told; what the browser
/// asked of it to turn them on is answered to no client.
#[test]
fn a_new_renderer_has_the_lifecycle_on() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    enable(&mut page);
    page.call(None, "Page.navigate", json!({ "url": "about:crash" }));
    page.take_events("Page.");
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let loader = navigated["result"]["loaderId"].clone();
    loop {
        let event = page.event("Page.lifecycleEvent");
        if event["params"]["name"] == "load" && event["params"]["loaderId"] == loader {
            break;
        }
    }
    assert_eq!(page.answers(), Vec::<&Value>::new());
}

/// Turned on later, the events of each frame's document so far are told,
/// after a `commit`, with the times they had; each time they are turned
/// on, whether `Page` is or not. The events to come go only to a session
/// with `Page` on, and `Page.disable` turns them off.
#[test]
fn the_lifecycle_so_far_is_told_as_it_is_turned_on() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let machopo = Machopo::start();
    let (mut page, main) = page(&machopo);
    crate::devtools::load(&mut page, &server.url("/"));
    // The page's network has gone quiet by now.
    std::thread::sleep(Duration::from_millis(700));
    let told = |page: &mut Client| {
        page.call(
            None,
            "Page.setLifecycleEventsEnabled",
            json!({ "enabled": true }),
        );
        let events = page.take_events("Page.lifecycleEvent");
        outline(&events, &main)
    };
    let so_far = [
        "main commit",
        "main DOMContentLoaded",
        "main load",
        "main networkAlmostIdle",
        "main networkIdle",
    ];
    assert_eq!(told(&mut page), so_far);
    assert_eq!(told(&mut page), so_far);
    // Not live without `Page` (what came is read in as the evaluation is
    // answered).
    page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    std::thread::sleep(Duration::from_millis(300));
    evaluate(&mut page, "0", None);
    assert!(!page.came("Page.lifecycleEvent"));
    enable(&mut page);
    page.take_events("Page.");
    page.call(None, "Page.disable", json!({}));
    page.call(None, "Page.enable", json!({}));
    page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    std::thread::sleep(Duration::from_millis(300));
    evaluate(&mut page, "0", None);
    assert!(!page.came("Page.lifecycleEvent"));
}

/// `Page.navigate` is answered as the navigation is about to commit:
/// after its response is told, with its loader; with `errorText` if the
/// load failed (its error page commits) or a newer navigation took its
/// place (`net::ERR_ABORTED`); at once for a page of machopo's own.
#[test]
fn a_navigation_is_answered_as_it_is_ready_to_commit() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        (
            "/slow",
            Route::ok("text/html", "<!DOCTYPE html>slow").with_delay(Duration::from_millis(500)),
        ),
    ]);
    let machopo = Machopo::start();
    let (mut page, main) = page(&machopo);
    page.call(None, "Network.enable", json!({}));
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let loader = navigated["result"]["loaderId"].clone();
    // Its response before the answer.
    assert!(
        page.take_events("Network.responseReceived")
            .iter()
            .any(|event| event["params"]["loaderId"] == loader),
        "{navigated}"
    );
    let refused = format!("http://127.0.0.1:{}/", unused_port());
    let failed = page.call(None, "Page.navigate", json!({ "url": refused }));
    assert_eq!(
        failed["result"]["errorText"], "net::ERR_CONNECTION_REFUSED",
        "{failed}"
    );
    assert_eq!(failed["result"]["frameId"], main.as_str());
    assert!(failed["result"]["loaderId"].is_string(), "{failed}");
    // A newer one takes the slow one's place.
    let slow = page.send(None, "Page.navigate", json!({ "url": server.url("/slow") }));
    std::thread::sleep(Duration::from_millis(100));
    let newer = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let replaced = page.answer(slow);
    assert_eq!(
        replaced["result"]["errorText"], "net::ERR_ABORTED",
        "{replaced}"
    );
    assert!(newer["result"].get("errorText").is_none(), "{newer}");
    let blank = page.call(None, "Page.navigate", json!({ "url": "about:blank" }));
    assert!(blank["result"]["loaderId"].is_string(), "{blank}");
    let invalid = page.call(None, "Page.navigate", json!({ "url": "not a url" }));
    assert!(invalid.get("error").is_some(), "{invalid}");
}

/// `Target.setAutoAttach` is answered (nothing attaches on its own);
/// `detachFromTarget` ends a flat session, told as
/// `Target.detachedFromTarget`, as is one whose tab closes.
#[test]
fn flat_sessions_detach() {
    let machopo = Machopo::start();
    let mut browser = machopo.connect_browser();
    let target = machopo.targets()[0]["id"].as_str().unwrap().to_owned();
    let auto = browser.call(
        None,
        "Target.setAutoAttach",
        json!({ "autoAttach": true, "waitForDebuggerOnStart": true, "flatten": true, "filter": [] }),
    );
    assert_eq!(auto["result"], json!({}), "{auto}");
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
    let session = attach(&mut browser, &target);
    let detached = browser.call(
        None,
        "Target.detachFromTarget",
        json!({ "sessionId": session }),
    );
    assert_eq!(detached["result"], json!({}), "{detached}");
    let told = browser.event("Target.detachedFromTarget")["params"].clone();
    assert_eq!(told, json!({ "sessionId": session, "targetId": target }));
    let gone = browser.call(
        Some(&session),
        "Runtime.evaluate",
        json!({ "expression": "1" }),
    );
    assert!(gone.get("error").is_some(), "{gone}");
    let again = browser.call(
        None,
        "Target.detachFromTarget",
        json!({ "sessionId": session }),
    );
    assert!(again.get("error").is_some(), "{again}");

    // A tab that closes, a navigation of it on its way: that fails.
    let server = server(&[(
        "/slow",
        Route::ok("text/html", "<!DOCTYPE html>slow").with_delay(Duration::from_millis(500)),
    )]);
    let created = browser.call(None, "Target.createTarget", json!({ "url": "about:blank" }));
    let tab = created["result"]["targetId"].as_str().unwrap().to_owned();
    let session = attach(&mut browser, &tab);
    let navigating = browser.send(
        Some(&session),
        "Page.navigate",
        json!({ "url": server.url("/slow") }),
    );
    std::thread::sleep(Duration::from_millis(100));
    browser.call(None, "Target.closeTarget", json!({ "targetId": tab }));
    let told = browser.event("Target.detachedFromTarget")["params"].clone();
    assert_eq!(told, json!({ "sessionId": session, "targetId": tab }));
    let navigated = browser.answer(navigating);
    assert!(navigated.get("error").is_some(), "{navigated}");
}

/// The document told: its frame navigated (its URL,
/// by the navigation's loader), then its `DOMContentLoaded` and `load`,
/// the events and the lifecycle's, and the frame stopped loading; its
/// markup is its content.
#[test]
fn the_document_is_told_through_its_lifecycle() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>new"))]);
    let machopo = Machopo::start_with(&[]);
    let (mut page, main) = page(&machopo);
    enable(&mut page);
    page.take_events("Page.");
    let navigated = page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let loader = navigated["result"]["loaderId"].clone();
    let events = page_events_until(&mut page, |event| {
        event["method"] == "Page.frameStoppedLoading" && event["params"]["frameId"] == main.as_str()
    });
    let order = outline(&events, &main);
    let at = |what: &str| {
        order
            .iter()
            .position(|told| told == what)
            .unwrap_or_else(|| panic!("no {what}: {order:#?}"))
    };
    for (before, after) in [
        ("main init", "main frameNavigated"),
        ("main frameNavigated", "main DOMContentLoaded"),
        ("main DOMContentLoaded", "main load"),
        ("main load", "main frameStoppedLoading"),
    ] {
        assert!(
            at(before) < at(after),
            "{before} before {after}: {order:#?}"
        );
    }
    let frame = events
        .iter()
        .find(|event| event["method"] == "Page.frameNavigated")
        .map(|event| &event["params"]["frame"])
        .unwrap();
    assert_eq!(frame["url"], server.url("/"), "{frame}");
    assert_eq!(frame["loaderId"], loader, "{frame}");
    assert_eq!(frame["mimeType"], "text/html", "{frame}");
    assert_eq!(
        frame["securityOrigin"],
        server.url("/").trim_end_matches('/'),
        "{frame}"
    );
    for name in ["DOMContentLoaded", "load"] {
        assert_eq!(lifecycle(&events, &main, name)["loaderId"], loader);
    }
    for method in ["Page.domContentEventFired", "Page.loadEventFired"] {
        assert!(
            events.iter().any(|event| event["method"] == method),
            "no {method}: {events:#?}"
        );
    }
    let content = page.call(
        None,
        "Page.getResourceContent",
        json!({ "frameId": main, "url": server.url("/") }),
    );
    assert_eq!(
        content["result"]["content"], "<!DOCTYPE html>new",
        "{content}"
    );
}

/// The child frames told: each attached to its
/// parent, by its iframe's `name`; one that keeps its initial
/// `about:blank` loaded as it starts, its whole lifecycle told; one whose
/// document is on its way its initial document's `init`, then navigated,
/// its own lifecycle, its network idle as its own requests end; one its
/// iframe left detached. From the page's markup and script: DevTools'
/// `Runtime` doesn't run script in the DOM's document.
#[test]
fn the_child_frames_are_told() {
    let html = "<!DOCTYPE html><iframe name=blank></iframe>\
                <iframe name=slow src='/slow'></iframe>\
                <script>document.getElementsByName('slow')[0].onload = () => \
                  setTimeout(() => document.getElementsByName('blank')[0].remove(), 600)</script>";
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        (
            "/slow",
            Route::ok("text/html", "<!DOCTYPE html><img src='/slow.png'>")
                .with_delay(Duration::from_millis(200)),
        ),
        (
            "/slow.png",
            Route::ok("image/png", vec![0x89]).with_delay(Duration::from_millis(300)),
        ),
    ]);
    let machopo = Machopo::start_with(&[]);
    let (mut page, main) = page(&machopo);
    enable(&mut page);
    page.take_events("Page.");
    page.call(None, "Page.navigate", json!({ "url": server.url("/") }));
    let events = page_events_until(&mut page, |event| event["method"] == "Page.frameDetached");
    let attached: Vec<String> = events
        .iter()
        .filter(|event| event["method"] == "Page.frameAttached")
        .map(|event| {
            assert_eq!(event["params"]["parentFrameId"], main.as_str(), "{event}");
            event["params"]["frameId"].as_str().unwrap().to_owned()
        })
        .collect();
    assert_eq!(attached.len(), 2, "{events:#?}");
    // A child's events as `method` or the lifecycle's names.
    let told = |frame: &str| -> Vec<String> {
        events
            .iter()
            .filter(|event| {
                let params = &event["params"];
                params["frameId"] == frame || params["frame"]["id"] == frame
            })
            .map(|event| match event["method"].as_str().unwrap() {
                "Page.lifecycleEvent" => event["params"]["name"].as_str().unwrap().to_owned(),
                method => method.trim_start_matches("Page.").to_owned(),
            })
            .collect()
    };
    assert_eq!(
        told(&attached[0]),
        [
            "frameAttached",
            "init",
            "frameStartedLoading",
            "DOMContentLoaded",
            "load",
            "frameStoppedLoading",
            "networkAlmostIdle",
            "networkIdle",
            "frameDetached",
        ]
    );
    assert_eq!(
        told(&attached[1]),
        [
            "frameAttached",
            "init",
            "frameStartedLoading",
            "init",
            "frameNavigated",
            "DOMContentLoaded",
            "load",
            "frameStoppedLoading",
            "networkAlmostIdle",
            "networkIdle",
        ]
    );
    let navigated = events
        .iter()
        .find(|event| {
            event["method"] == "Page.frameNavigated"
                && event["params"]["frame"]["id"] == attached[1].as_str()
        })
        .map(|event| &event["params"]["frame"])
        .unwrap();
    assert_eq!(navigated["name"], "slow", "{navigated}");
    assert_eq!(navigated["parentId"], main.as_str(), "{navigated}");
    assert_eq!(
        navigated["url"],
        server.url("/slow").as_str(),
        "{navigated}"
    );
    // Its network idle as its image came, after its `DOMContentLoaded`.
    let loaded = lifecycle(&events, &attached[1], "DOMContentLoaded")["timestamp"]
        .as_f64()
        .unwrap();
    let idle = lifecycle(&events, &attached[1], "networkIdle")["timestamp"]
        .as_f64()
        .unwrap();
    assert!(
        (0.2..0.8).contains(&(idle - loaded)),
        "idle at {idle}, DOMContentLoaded at {loaded}"
    );
    // The top-level document's quiet from its `DOMContentLoaded` on: its
    // children's requests aren't its own (their image).
    let top_idle = lifecycle(&events, &main, "networkIdle")["timestamp"]
        .as_f64()
        .unwrap();
    let top_loaded = lifecycle(&events, &main, "DOMContentLoaded")["timestamp"]
        .as_f64()
        .unwrap();
    assert!(
        top_idle - top_loaded < 0.1 && top_idle < loaded,
        "the top-level one idle at {top_idle}, its DOMContentLoaded at {top_loaded}, \
         the child's at {loaded}"
    );
}
