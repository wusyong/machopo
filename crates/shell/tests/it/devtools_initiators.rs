//! Who asked for each request, as DevTools' `Network.requestWillBeSent`
//! tells it (`initiator`): the parser, at the line of the element's start
//! tag; a stylesheet, for its `url()`s; the module that imports; and, once
//! a session asked for stacks (`Network.setAttachDebugStack`), the script
//! that asked, with its stack across `await`s, as the iframes it makes are
//! told (`Page.frameAttached`). The rest are `other`.

use net::test_server::Route;
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::devtools::{evaluate, load, page};
use crate::remote_debugging::{Client, Machopo};

/// The `requestWillBeSent`s that came, until one for each of `paths` has
/// (those that came while waiting for answers first); the other messages
/// read are kept.
fn sent_for(page: &mut Client, paths: &[&str]) -> Vec<Value> {
    let came = |sent: &[Value], path: &str| {
        sent.iter().any(|event| {
            event["params"]["request"]["url"]
                .as_str()
                .is_some_and(|url| url.ends_with(path))
        })
    };
    let mut sent: Vec<Value> = page.take_events("Network.requestWillBeSent");
    while let Some(path) = paths.iter().find(|path| !came(&sent, path)) {
        let event = page.read().unwrap_or_else(|| {
            let urls: Vec<&Value> = sent
                .iter()
                .map(|event| &event["params"]["request"]["url"])
                .collect();
            panic!("no request of {path} came; these did: {urls:?}")
        });
        if event["method"] == "Network.requestWillBeSent" {
            sent.push(event);
        } else {
            page.keep(event);
        }
    }
    sent
}

/// The initiator of the request for `path`, among `sent`.
fn initiator<'a>(sent: &'a [Value], path: &str) -> &'a Value {
    sent.iter()
        .find(|event| {
            event["params"]["request"]["url"]
                .as_str()
                .is_some_and(|url| url.ends_with(path))
        })
        .map(|event| &event["params"]["initiator"])
        .unwrap_or_else(|| panic!("no request of {path}: {sent:#?}"))
}

/// The function names of `stack`'s frames, top first.
fn functions(stack: &Value) -> Vec<&str> {
    stack["callFrames"]
        .as_array()
        .unwrap_or_else(|| panic!("no frames: {stack}"))
        .iter()
        .map(|frame| frame["functionName"].as_str().unwrap())
        .collect()
}

/// What the parser asked for is `parser`, at its start tag's line
/// (0-based), the document's URL without its fragment; a stylesheet's
/// `url()` is `parser` with the stylesheet's URL, and the document's (not
/// its base URL) for one in a `<style>` or a `style` attribute; a static
/// import's is its importer's.
#[test]
fn the_parser_and_stylesheets_ask() {
    let html = "<!DOCTYPE html>\n\
                <link rel=stylesheet href=/s.css>\n\
                <img src=/i.png>\n\
                <div style='background-image: url(/inline.png)'>x</div>\n\
                <script src=/a.js></script>\n\
                <script type=module src=/m.js></script>\n\
                <iframe src=/f.html></iframe>\n\
                <base href=/elsewhere/>\n\
                <style>span { background-image: url(/styled.png) }</style>\n\
                <span>x</span><p style='background-image: url(/based.png)'>x</p>";
    let server = server(&[
        ("/", Route::ok("text/html", html)),
        (
            "/s.css",
            Route::ok("text/css", "body { background-image: url(/bg.png) }"),
        ),
        ("/i.png", Route::ok("image/png", vec![0x89])),
        ("/bg.png", Route::ok("image/png", vec![0x89])),
        ("/inline.png", Route::ok("image/png", vec![0x89])),
        ("/styled.png", Route::ok("image/png", vec![0x89])),
        ("/based.png", Route::ok("image/png", vec![0x89])),
        ("/a.js", Route::ok("text/javascript", "0")),
        ("/m.js", Route::ok("text/javascript", "import './dep.js';")),
        ("/dep.js", Route::ok("text/javascript", "")),
        ("/f.html", Route::ok("text/html", "<!DOCTYPE html>frame")),
        ("/again.png", Route::ok("image/png", vec![0x89])),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    page.call(None, "Network.enable", json!({}));
    let url = server.url("/");
    load(&mut page, &format!("{url}#here"));
    let sent = sent_for(
        &mut page,
        &[
            "/s.css",
            "/i.png",
            "/a.js",
            "/m.js",
            "/f.html",
            "/bg.png",
            "/inline.png",
            "/styled.png",
            "/based.png",
            "/dep.js",
        ],
    );
    for (path, line) in [
        ("/s.css", 1),
        ("/i.png", 2),
        ("/a.js", 4),
        ("/m.js", 5),
        ("/f.html", 6),
    ] {
        assert_eq!(
            initiator(&sent, path),
            &json!({ "type": "parser", "url": url, "lineNumber": line }),
            "{path}"
        );
    }
    assert_eq!(
        initiator(&sent, "/bg.png"),
        &json!({ "type": "parser", "url": server.url("/s.css") })
    );
    // The document's, though their base URL is another.
    for path in ["/inline.png", "/styled.png", "/based.png"] {
        assert_eq!(
            initiator(&sent, path),
            &json!({ "type": "parser", "url": url }),
            "{path}"
        );
    }
    assert_eq!(
        initiator(&sent, "/dep.js"),
        &json!({ "type": "script", "url": server.url("/m.js") })
    );
    // The parser's line goes with the load it asked for: the element's
    // next one, script's (no stacks asked for), isn't the parser's.
    evaluate(
        &mut page,
        "document.querySelector('img').src = '/again.png'; 0",
        None,
    );
    let sent = sent_for(&mut page, &["/again.png"]);
    assert_eq!(initiator(&sent, "/again.png"), &json!({ "type": "other" }));
}

/// With stacks asked for, what script asks for is told with its stack:
/// an image, a stylesheet, a script and an iframe it inserts, a `fetch()`
/// after an `await` (whose stack goes on in its async parent), an
/// `import()`; and an iframe it makes is attached with it. Without, they
/// are `other`.
#[test]
fn script_asks_with_its_stack() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html><body>")),
        ("/x.png", Route::ok("image/png", vec![0x89])),
        ("/t.css", Route::ok("text/css", "")),
        ("/s.js", Route::ok("text/javascript", "")),
        ("/f.html", Route::ok("text/html", "<!DOCTYPE html>frame")),
        ("/data.json", Route::ok("application/json", "{}")),
        ("/lazy.js", Route::ok("text/javascript", "")),
        ("/plain.png", Route::ok("image/png", vec![0x89])),
        ("/y.png", Route::ok("image/png", vec![0x89])),
        ("/u.css", Route::ok("text/css", "")),
    ]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    page.call(None, "Network.enable", json!({}));
    // Not asked for: `other`.
    evaluate(
        &mut page,
        "function plain() { new Image().src = '/plain.png'; } plain(); 0",
        None,
    );
    let sent = sent_for(&mut page, &["/plain.png"]);
    assert_eq!(initiator(&sent, "/plain.png"), &json!({ "type": "other" }));

    // As DevTools asks once it connects.
    page.call(None, "Page.enable", json!({}));
    page.call(None, "Runtime.enable", json!({}));
    page.call(None, "Debugger.enable", json!({}));
    page.call(
        None,
        "Debugger.setAsyncCallStackDepth",
        json!({ "maxDepth": 32 }),
    );
    page.call(
        None,
        "Network.setAttachDebugStack",
        json!({ "enabled": true }),
    );
    evaluate(
        &mut page,
        "function addImage() { const i = new Image(); i.src = '/x.png'; document.body.append(i); }
         function addLink() {
           const l = document.createElement('link');
           l.rel = 'stylesheet';
           l.href = '/t.css';
           document.head.append(l);
         }
         function addScript() {
           const s = document.createElement('script');
           s.src = '/s.js';
           document.body.append(s);
         }
         function addFrame() {
           const f = document.createElement('iframe');
           f.src = '/f.html';
           document.body.append(f);
         }
         async function later() { await null; fetch('/data.json'); }
         function importLazily() { import('/lazy.js'); }
         addImage(); addLink(); addScript(); addFrame(); later(); importLazily(); 0",
        None,
    );
    let sent = sent_for(
        &mut page,
        &[
            "/x.png",
            "/t.css",
            "/s.js",
            "/f.html",
            "/data.json",
            "/lazy.js",
        ],
    );
    for (path, function) in [
        ("/x.png", "addImage"),
        ("/t.css", "addLink"),
        ("/s.js", "addScript"),
        ("/f.html", "addFrame"),
        ("/lazy.js", "importLazily"),
    ] {
        let initiator = initiator(&sent, path);
        assert_eq!(initiator["type"], "script", "{path}: {initiator}");
        assert_eq!(functions(&initiator["stack"])[0], function, "{path}");
    }
    let fetched = initiator(&sent, "/data.json");
    assert_eq!(functions(&fetched["stack"]), ["later"], "{fetched}");
    let parent = &fetched["stack"]["parent"];
    assert_eq!(parent["description"], "await", "{fetched}");
    assert_eq!(functions(parent)[0], "", "{fetched}");

    let attached = page.event("Page.frameAttached")["params"].clone();
    assert_eq!(functions(&attached["stack"])[0], "addFrame", "{attached}");

    // A stack that asked for nothing new (an image it has) doesn't stay
    // for the next time the element asks.
    evaluate(
        &mut page,
        "function again() { document.querySelector('img').src = '/x.png'; } again(); 0",
        None,
    );
    evaluate(
        &mut page,
        "function anew() { document.querySelector('img').src = '/y.png'; } anew(); 0",
        None,
    );
    let sent = sent_for(&mut page, &["/y.png"]);
    assert_eq!(functions(&initiator(&sent, "/y.png")["stack"])[0], "anew");
    // Nor one no load took at all (a sheet `<link>` with no `href` yet):
    // the stack is the one that gives it its `href`.
    evaluate(
        &mut page,
        "function bare() {
           window.bareLink = document.createElement('link');
           bareLink.rel = 'stylesheet';
           document.head.append(bareLink);
         }
         bare(); 0",
        None,
    );
    evaluate(
        &mut page,
        "function giveHref() { bareLink.href = '/u.css'; } giveHref(); 0",
        None,
    );
    let sent = sent_for(&mut page, &["/u.css"]);
    assert_eq!(
        functions(&initiator(&sent, "/u.css")["stack"])[0],
        "giveHref"
    );
}
