//! DevTools' `Overlay` domain and the box model `DOM` gives, which the
//! tab's renderer answers: a node's content, padding, border and margin
//! boxes as quads in the top-level frame's viewport (scrolled,
//! transformed, in an iframe); the node at a point; a node highlighted on
//! the page, drawn over it with the colors asked, and followed as it moves;
//! the inspect mode, where the mouse over the page is the overlay's, not
//! the page's.

use net::test_server::Route;
use serde_json::{Value, json};

use crate::automation_pipe::{pixel, png, server};
use crate::devtools::{evaluate, load, page};
use crate::remote_debugging::{Client, Machopo};

fn result(answer: Value) -> Value {
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"].clone()
}

fn error(answer: Value) -> String {
    answer["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("an error: {answer}"))
        .to_owned()
}

const PAGE: &str = "<!DOCTYPE html><style>html, body { margin: 0 } \
    span { font: 10px/10px Ahem; padding: 4px; border: 1px solid } \
    #a { position: absolute; left: 10px; top: 20px; width: 100px; height: 50px; \
    padding: 5px; border: 2px solid black; margin: 3px } \
    #t { position: absolute; left: 200px; top: 200px; width: 100px; height: 100px; \
    transform: rotate(90deg) } \
    #far { position: absolute; top: 3000px; width: 10px; height: 10px }</style>\
    <div id=a>a</div><div id=t></div><div id=far></div>\
    <p style='position: absolute; top: 500px; margin: 0'><span>xy</span></p>\
    <div id=f style='position: fixed; left: 300px; top: 0; width: 10px; height: 10px'></div>\
    <input style='position: absolute; left: 0; top: 400px; width: 100px; height: 20px; \
    border: 0; padding: 0'>\
    <iframe style='position: absolute; left: 400px; top: 300px; width: 200px; height: 100px; \
    border: 0' srcdoc='<body style=\"margin: 0\"><p style=\"margin: 10px; width: 20px; height: 20px\"></p>'>\
    </iframe>";

/// The page loaded, `DOM` on and its document asked for: the session, and
/// the number of the node `selector` finds.
fn dom_page(routes: &[(&str, Route)]) -> (Machopo, Client, Value, net::test_server::TestServer) {
    let server = server(routes);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    result(page.call(None, "DOM.enable", json!({})));
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    (machopo, page, root["nodeId"].clone(), server)
}

fn node(page: &mut Client, document: &Value, selector: &str) -> Value {
    result(page.call(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": selector }),
    ))["nodeId"]
        .clone()
}

/// A quad's corners as `(x, y)` pairs.
fn corners(quad: &Value) -> Vec<(f64, f64)> {
    let numbers: Vec<f64> = quad
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_f64().unwrap())
        .collect();
    numbers.chunks(2).map(|pair| (pair[0], pair[1])).collect()
}

/// The box model: each box's quad clockwise from the top left, margins
/// outside the border; the border box's size; scrolled with the viewport;
/// through a transform; an iframe's in the top-level viewport; content
/// quads; none for a node with no box.
#[test]
fn nodes_have_box_models() {
    let (_machopo, mut page, document, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let a = node(&mut page, &document, "#a");
    let model = result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": a })))["model"].clone();
    assert_eq!(
        corners(&model["margin"]),
        [(10.0, 20.0), (130.0, 20.0), (130.0, 90.0), (10.0, 90.0)]
    );
    assert_eq!(
        corners(&model["border"]),
        [(13.0, 23.0), (127.0, 23.0), (127.0, 87.0), (13.0, 87.0)]
    );
    assert_eq!(
        corners(&model["padding"]),
        [(15.0, 25.0), (125.0, 25.0), (125.0, 85.0), (15.0, 85.0)]
    );
    assert_eq!(
        corners(&model["content"]),
        [(20.0, 30.0), (120.0, 30.0), (120.0, 80.0), (20.0, 80.0)]
    );
    assert_eq!(
        (model["width"].clone(), model["height"].clone()),
        (json!(114.0), json!(64.0))
    );

    // Turned a quarter: the top left corner is at the top right.
    let t = node(&mut page, &document, "#t");
    let model = result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": t })))["model"].clone();
    let border = corners(&model["border"]);
    assert!(
        (border[0].0 - 300.0).abs() < 0.01 && (border[0].1 - 200.0).abs() < 0.01,
        "{border:?}"
    );
    let quads =
        result(page.call(None, "DOM.getContentQuads", json!({ "nodeId": t })))["quads"].clone();
    let quad = corners(&quads[0]);
    assert!(
        (quad[0].0 - 300.0).abs() < 0.01 && (quad[0].1 - 200.0).abs() < 0.01,
        "{quad:?}"
    );

    // In the iframe: its own offset added.
    let iframe = node(&mut page, &document, "iframe");
    let described = result(page.call(
        None,
        "DOM.describeNode",
        json!({ "nodeId": iframe, "depth": -1, "pierce": true }),
    ))["node"]
        .clone();
    let p =
        described["contentDocument"]["children"][0]["children"][1]["children"][0]["backendNodeId"]
            .clone();
    let model =
        result(page.call(None, "DOM.getBoxModel", json!({ "backendNodeId": p })))["model"].clone();
    assert_eq!(corners(&model["border"])[0], (410.0, 310.0), "{model}");

    // Scrolled: where it shows; a box fixed to the viewport where it is.
    evaluate(&mut page, "scrollTo(0, 15)", None);
    let model = result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": a })))["model"].clone();
    assert_eq!(corners(&model["border"])[0], (13.0, 8.0));
    let fixed = node(&mut page, &document, "#f");
    let model =
        result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": fixed })))["model"].clone();
    assert_eq!(corners(&model["border"])[0], (300.0, 0.0));
    let quads =
        result(page.call(None, "DOM.getContentQuads", json!({ "nodeId": a })))["quads"].clone();
    assert_eq!(corners(&quads[0])[0], (13.0, 8.0));

    // An inline element: around its box on the line, its border and
    // padding inside; no margin above or below.
    let span = node(&mut page, &document, "span");
    let model =
        result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": span })))["model"].clone();
    let (border, padding, content) = (
        corners(&model["border"]),
        corners(&model["padding"]),
        corners(&model["content"]),
    );
    assert_eq!(
        (padding[0].0 - border[0].0, padding[0].1 - border[0].1),
        (1.0, 1.0),
        "{model}"
    );
    assert_eq!(
        (content[0].0 - padding[0].0, content[2].1 - padding[2].1),
        (4.0, -4.0),
        "{model}"
    );
    assert_eq!(corners(&model["margin"]), border);
    assert_eq!(
        content[1].0 - content[0].0,
        20.0,
        "two Ahem squares: {model}"
    );

    // A box the user scrolls, as last laid out: one with more than it
    // shows, not one with room to spare.
    evaluate(
        &mut page,
        "for (const [id, height] of [['s', 20], ['n', 200]]) {
             const scroller = document.createElement('div');
             scroller.id = id;
             scroller.style = `overflow: auto; height: ${height}px`;
             scroller.innerHTML = '<div style=\"height: 100px\"></div>';
             document.body.append(scroller);
             scroller.offsetHeight;
         }",
        None,
    );
    for (selector, scrollable) in [("#s", Some(true)), ("#n", None)] {
        let id = node(&mut page, &document, selector);
        let described =
            result(page.call(None, "DOM.describeNode", json!({ "nodeId": id })))["node"].clone();
        assert_eq!(
            described["isScrollable"].as_bool(),
            scrollable,
            "{described}"
        );
    }

    // A node with no box: a `<head>`.
    let head = node(&mut page, &document, "head");
    assert_eq!(
        error(page.call(None, "DOM.getBoxModel", json!({ "nodeId": head }))),
        "Could not compute box model."
    );
    assert_eq!(
        result(page.call(None, "DOM.getContentQuads", json!({ "nodeId": head })))["quads"],
        json!([])
    );
}

/// The node at a point of the document (scrolled into it): an element, a
/// run of text's, one in an iframe; numbered with the path sent once the
/// document was asked for; nothing outside.
#[test]
fn nodes_are_found_at_points() {
    let (_machopo, mut page, document, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let a = node(&mut page, &document, "#a");
    let found = result(page.call(None, "DOM.getNodeForLocation", json!({ "x": 50, "y": 50 })));
    assert_eq!(found["nodeId"], a, "{found}");
    assert!(found["frameId"].is_string());
    // The text `a`, at the content box's top left: its element.
    let found = result(page.call(None, "DOM.getNodeForLocation", json!({ "x": 22, "y": 33 })));
    assert_eq!(found["nodeId"], a, "{found}");
    // In the iframe.
    let found = result(page.call(
        None,
        "DOM.getNodeForLocation",
        json!({ "x": 415, "y": 315 }),
    ));
    let described = result(page.call(
        None,
        "DOM.describeNode",
        json!({ "backendNodeId": found["backendNodeId"] }),
    ))["node"]
        .clone();
    assert_eq!(described["localName"], "p");
    assert_ne!(
        found["frameId"],
        result(page.call(None, "DOM.getNodeForLocation", json!({ "x": 50, "y": 50 })))["frameId"]
    );
    // In a control's user-agent shadow tree: the control, unless asked.
    let input = node(&mut page, &document, "input");
    let found = result(page.call(None, "DOM.getNodeForLocation", json!({ "x": 5, "y": 405 })));
    assert_eq!(found["nodeId"], input, "{found}");
    let inner = result(page.call(
        None,
        "DOM.getNodeForLocation",
        json!({ "x": 5, "y": 405, "includeUserAgentShadowDOM": true }),
    ));
    assert_ne!(inner["backendNodeId"], found["backendNodeId"], "{inner}");
    // Below the viewport, what is there isn't hit, as Chrome's: `#far`
    // until scrolled to.
    assert_eq!(
        error(page.call(None, "DOM.getNodeForLocation", json!({ "x": 5, "y": 3005 }))),
        "No node found at given location"
    );
    // Scrolled: the point is the document's.
    evaluate(&mut page, "scrollTo(0, 2990)", None);
    let far = node(&mut page, &document, "#far");
    let found = result(page.call(None, "DOM.getNodeForLocation", json!({ "x": 5, "y": 3005 })));
    assert_eq!(found["nodeId"], far, "{found}");
}

/// While script is paused, wherever it pauses (in script, a timer, a
/// listener `click()` or `focus()` called, an animation frame, a resize
/// observer), the box model is there, laid out again with what script did
/// up to there.
#[test]
fn box_models_are_there_while_paused() {
    let html = "<!DOCTYPE html><style>body { margin: 0 } div { height: 10px }</style><body></body>";
    let (_machopo, mut page, document, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    let root = node(&mut page, &document, "html");
    page.call(None, "Debugger.enable", json!({}));
    let expression = "
        const add = () => document.body.append(document.createElement('div'));
        add(); debugger;
        setTimeout(() => {
            add(); debugger;
            const button = document.createElement('button');
            button.onclick = () => { add(); debugger; };
            button.click();
            const input = document.createElement('input');
            input.style.display = 'block';
            input.style.height = '0';
            input.style.border = 'none';
            input.style.padding = '0';
            document.body.append(input);
            input.onfocus = () => { add(); debugger; };
            input.focus();
            requestAnimationFrame(() => {
                add(); debugger;
                new ResizeObserver(() => {
                    if (window.observed) return;
                    window.observed = true;
                    add(); debugger;
                }).observe(document.body);
            });
        });
        1";
    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": expression }),
    );
    let mut heights = Vec::new();
    for _ in 0..6 {
        page.event("Debugger.paused");
        let model =
            result(page.call(None, "DOM.getBoxModel", json!({ "nodeId": root })))["model"].clone();
        heights.push(model["height"].as_f64().unwrap());
        page.call(None, "Debugger.resume", json!({}));
    }
    page.answer(evaluated);
    assert_eq!(heights, [10.0, 20.0, 30.0, 40.0, 50.0, 60.0]);
}

/// While script is paused the page is drawn as it is (Chrome draws it
/// too): what the console's script changes, the node highlighted, DevTools'
/// message over the page dimmed; once it goes on, the page draws as before.
#[test]
fn the_page_is_drawn_while_paused() {
    let html = "<!DOCTYPE html><style>html, body { margin: 0 } \
                #a { width: 100px; height: 100px }</style><div id=a></div>";
    let (_machopo, mut page, document, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    result(page.call(None, "Overlay.enable", json!({})));
    let a = node(&mut page, &document, "#a");
    page.call(None, "Debugger.enable", json!({}));
    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "debugger; 1" }),
    );
    page.event("Debugger.paused");
    assert_eq!(screen(&mut page, 50, 150), [255, 255, 255]);
    result(page.call(
        None,
        "Runtime.evaluate",
        json!({ "expression": "document.body.style.background = 'rgb(0, 128, 0)'" }),
    ));
    assert_eq!(screen(&mut page, 50, 150), [0, 128, 0]);
    let config = json!({ "contentColor": rgba(255, 0, 0) });
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": a, "highlightConfig": config }),
    ));
    assert_eq!(screen(&mut page, 50, 50), [255, 0, 0]);
    result(page.call(None, "Overlay.hideHighlight", json!({})));
    assert_eq!(screen(&mut page, 50, 50), [0, 128, 0]);
    // DevTools' message: the page dimmed, the message's box at its top, in
    // its middle; gone once DevTools takes it back.
    result(page.call(
        None,
        "Overlay.setPausedInDebuggerMessage",
        json!({ "message": "Paused in debugger" }),
    ));
    let shot = result(page.call(None, "Page.captureScreenshot", json!({})));
    let ((width, _), pixels) = png(shot["data"].as_str().unwrap());
    let dimmed = pixel(&pixels, width, 50, 150);
    assert!(
        dimmed[0] == 0 && dimmed[1].abs_diff(88) <= 2 && dimmed[2] == 0,
        "{dimmed:?}"
    );
    assert_eq!(pixel(&pixels, width, width / 2, 12), [255, 255, 194]);
    // A session with `Overlay` off shows none, its message given after
    // too.
    result(page.call(None, "Overlay.disable", json!({})));
    assert_eq!(screen(&mut page, 50, 150), [0, 128, 0]);
    result(page.call(
        None,
        "Overlay.setPausedInDebuggerMessage",
        json!({ "message": "Paused in debugger" }),
    ));
    assert_eq!(screen(&mut page, 50, 150), [0, 128, 0]);
    result(page.call(None, "Overlay.enable", json!({})));
    result(page.call(None, "Overlay.setPausedInDebuggerMessage", json!({})));
    assert_eq!(screen(&mut page, 50, 150), [0, 128, 0]);
    page.call(None, "Debugger.resume", json!({}));
    page.answer(evaluated);
    evaluate(
        &mut page,
        "document.body.style.background = 'rgb(0, 0, 255)'",
        None,
    );
    assert_eq!(screen(&mut page, 50, 150), [0, 0, 255]);
}

/// Input while script is paused waits for it to go on (only DevTools'
/// tasks and the rendering's run while paused), then is handled in order.
#[test]
fn input_waits_for_the_pause() {
    let html = "<!DOCTYPE html><style>html, body { margin: 0 } button { width: 100px; height: 100px }</style>\
                <button onclick='window.clicked = true'>b</button>";
    let (_machopo, mut page, _document, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    // A frame of the page first: input goes to the document the compositor
    // last had a frame of.
    result(page.call(None, "Page.captureScreenshot", json!({})));
    page.call(None, "Debugger.enable", json!({}));
    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "debugger; 1" }),
    );
    page.event("Debugger.paused");
    let click =
        |kind: &str| json!({ "type": kind, "x": 50, "y": 50, "button": "left", "clickCount": 1 });
    let pressed = page.send(None, "Input.dispatchMouseEvent", click("mousePressed"));
    let released = page.send(None, "Input.dispatchMouseEvent", click("mouseReleased"));
    // A frame drawn meanwhile: the input is still waiting.
    result(page.call(None, "Page.captureScreenshot", json!({})));
    let clicked = result(page.call(
        None,
        "Runtime.evaluate",
        json!({ "expression": "window.clicked === true", "returnByValue": true }),
    ));
    assert_eq!(clicked["result"]["value"], false);
    page.call(None, "Debugger.resume", json!({}));
    page.answer(evaluated);
    page.answer(pressed);
    page.answer(released);
    assert_eq!(evaluate(&mut page, "window.clicked === true", None), true);
}

/// A wheel while script is paused scrolls on the compositor, which hands
/// the main thread the scroll with the next frame: the frame drawn while
/// paused keeps it (no jump back), and script sees it.
#[test]
fn a_scroll_while_paused_stays() {
    let html = "<!DOCTYPE html><style>html, body { margin: 0 } div { height: 5000px; \
                background: linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)) }</style><div></div>";
    let (_machopo, mut page, _document, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    // The gradient's blue at the top: none before the scroll, some after.
    assert!(screen(&mut page, 50, 10)[2] < 5);
    page.call(None, "Debugger.enable", json!({}));
    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "debugger; 1" }),
    );
    page.event("Debugger.paused");
    let wheel = json!({ "type": "mouseWheel", "x": 50, "y": 50, "deltaX": 0, "deltaY": 300 });
    result(page.call(None, "Input.dispatchMouseEvent", wheel));
    let scrolled = screen(&mut page, 50, 10);
    assert!(scrolled[2] > 10, "{scrolled:?}");
    let y = result(page.call(
        None,
        "Runtime.evaluate",
        json!({ "expression": "scrollY", "returnByValue": true }),
    ));
    assert_eq!(y["result"]["value"], 300);
    assert_eq!(screen(&mut page, 50, 10), scrolled);
    page.call(None, "Debugger.resume", json!({}));
    page.answer(evaluated);
    assert_eq!(evaluate(&mut page, "scrollY", None), 300);
}

/// The page's pixel at `x`, `y`.
fn screen(page: &mut Client, x: u32, y: u32) -> [u8; 3] {
    let shot = result(page.call(None, "Page.captureScreenshot", json!({})));
    let ((width, _), pixels) = png(shot["data"].as_str().unwrap());
    pixel(&pixels, width, x, y)
}

/// How many of the page's pixels from `from` to `to` (corners, exclusive)
/// aren't white.
pub(crate) fn non_white(page: &mut Client, from: (u32, u32), to: (u32, u32)) -> usize {
    let shot = result(page.call(None, "Page.captureScreenshot", json!({})));
    let ((width, _), pixels) = png(shot["data"].as_str().unwrap());
    (from.1..to.1)
        .flat_map(|y| (from.0..to.0).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(&pixels, width, x, y) != [255, 255, 255])
        .count()
}

pub(crate) fn rgba(r: u8, g: u8, b: u8) -> Value {
    json!({ "r": r, "g": g, "b": b, "a": 1 })
}

/// `Overlay` needs `DOM`; a highlighted node's boxes are drawn over the
/// page in the colors asked, its label below it, and go once hidden; the
/// highlight follows the node as script moves it; the highlight object as
/// Blink builds it.
#[test]
fn highlighted_nodes_are_drawn() {
    let server = server(&[("/", Route::ok("text/html", PAGE))]);
    let machopo = Machopo::start();
    let (mut page, _) = page(&machopo);
    load(&mut page, &server.url("/"));
    assert_eq!(
        error(page.call(None, "Overlay.enable", json!({}))),
        "DOM should be enabled first"
    );
    result(page.call(None, "DOM.enable", json!({})));
    let document = result(page.call(None, "DOM.getDocument", json!({})))["root"]["nodeId"].clone();
    result(page.call(None, "Overlay.enable", json!({})));
    let a = node(&mut page, &document, "#a");
    assert_eq!(screen(&mut page, 50, 50), [255, 255, 255]);
    let config = json!({
        "contentColor": rgba(255, 0, 0),
        "paddingColor": rgba(0, 255, 0),
        "borderColor": rgba(0, 0, 255),
        "marginColor": rgba(255, 255, 0),
        "showInfo": true,
    });
    let refused = page.call(None, "Overlay.highlightNode", json!({ "nodeId": a }));
    assert_eq!(refused["error"]["code"], -32602, "{refused}");
    assert_eq!(error(refused), "Invalid parameters");
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": a, "highlightConfig": config }),
    ));
    // Each box's ring in its color, the box inside it left out, as
    // Chrome's overlay cuts each out of the one around it.
    assert_eq!(screen(&mut page, 11, 50), [255, 255, 0], "margin");
    assert_eq!(screen(&mut page, 14, 50), [0, 0, 255], "border");
    assert_eq!(screen(&mut page, 17, 50), [0, 255, 0], "padding");
    assert_eq!(screen(&mut page, 50, 50), [255, 0, 0], "content");
    // Its label below it: text drawn there, none once asked for no info.
    assert!(non_white(&mut page, (13, 95), (120, 125)) > 0);
    let mut plain = config.clone();
    plain["showInfo"] = false.into();
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": a, "highlightConfig": plain }),
    ));
    assert_eq!(non_white(&mut page, (13, 95), (120, 125)), 0);
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": a, "highlightConfig": config }),
    ));
    // Script moves it: the highlight follows.
    evaluate(
        &mut page,
        "document.getElementById('a').style.left = '500px'",
        None,
    );
    assert_eq!(screen(&mut page, 11, 50), [255, 255, 255]);
    assert_eq!(screen(&mut page, 501, 50), [255, 255, 0]);
    result(page.call(None, "Overlay.hideHighlight", json!({})));
    assert_eq!(screen(&mut page, 501, 50), [255, 255, 255]);

    let highlight = result(page.call(
        None,
        "Overlay.getHighlightObjectForTest",
        json!({ "nodeId": a }),
    ))["highlight"]
        .clone();
    let names: Vec<&str> = highlight["paths"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["content", "padding", "border", "margin"]);
    assert_eq!(highlight["paths"][0]["fillColor"], "rgba(255, 0, 0, 0)");
    assert_eq!(highlight["paths"][0]["outlineColor"], "rgba(128, 0, 0, 0)");
    assert_eq!(
        highlight["paths"][2]["path"],
        json!([
            "M", 503.0, 23.0, "L", 617.0, 23.0, "L", 617.0, 87.0, "L", 503.0, 87.0, "Z"
        ])
    );
    let info = &highlight["elementInfo"];
    assert_eq!(info["tagName"], "div");
    assert_eq!(info["idValue"], "a");
    assert_eq!(
        (info["nodeWidth"].clone(), info["nodeHeight"].clone()),
        (json!("114"), json!("64"))
    );
    assert_eq!(highlight["colorFormat"], "hex");
}

/// The label goes by all of the element's boxes, as Chrome's: above its
/// margin, not over it.
#[test]
fn labels_stay_off_the_margin() {
    let html = "<!DOCTYPE html><style>html, body { margin: 0 } \
                #m { position: absolute; left: 50px; top: 200px; width: 100px; height: 40px; margin: 30px }</style>\
                <div id=m></div>";
    let (_machopo, mut page, document, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    result(page.call(None, "Overlay.enable", json!({})));
    let m = node(&mut page, &document, "#m");
    let config = json!({ "contentColor": rgba(255, 0, 0), "marginColor": rgba(255, 255, 0), "showInfo": true });
    result(page.call(
        None,
        "Overlay.highlightNode",
        json!({ "nodeId": m, "highlightConfig": config }),
    ));
    // The margin ring above the border box, all of it: the label is above
    // it.
    assert_eq!(non_white(&mut page, (50, 200), (210, 230)), 160 * 30);
    assert_eq!(screen(&mut page, 60, 215), [255, 255, 0]);
    assert!(
        non_white(&mut page, (50, 160), (210, 192)) > 0,
        "the label above"
    );
}

/// The inspect mode: the mouse over the page highlights what it is over
/// and tells DevTools (the path to it sent first); a press asks DevTools
/// to inspect it; the page sees none of it; off again, the page does.
#[test]
fn the_inspect_mode_takes_the_mouse() {
    let html = format!(
        "{PAGE}<script>window.seen = 0; \
         for (const type of ['mousemove', 'mousedown', 'mouseup', 'click']) \
         document.addEventListener(type, () => seen++);</script>"
    );
    let (_machopo, mut page, document, _server) =
        dom_page(&[("/", Route::ok("text/html", html.as_str()))]);
    result(page.call(None, "Overlay.enable", json!({})));
    let a = node(&mut page, &document, "#a");
    assert_eq!(
        error(page.call(None, "Overlay.setInspectMode", json!({ "mode": "lasso" }))),
        "Unknown mode \"lasso\" was provided."
    );
    result(page.call(
        None,
        "Overlay.setInspectMode",
        json!({ "mode": "searchForNode", "highlightConfig": { "contentColor": rgba(255, 0, 0), "showInfo": true } }),
    ));
    let mouse = |page: &mut Client, kind: &str| {
        let mut params = json!({ "type": kind, "x": 50, "y": 50 });
        if kind != "mouseMoved" {
            params["button"] = "left".into();
            params["clickCount"] = 1.into();
        }
        result(page.call(None, "Input.dispatchMouseEvent", params));
    };
    mouse(&mut page, "mouseMoved");
    let requested = page.event("Overlay.nodeHighlightRequested");
    assert_eq!(requested["params"]["nodeId"], a);
    assert_eq!(screen(&mut page, 50, 50), [255, 0, 0]);
    mouse(&mut page, "mousePressed");
    mouse(&mut page, "mouseReleased");
    let inspect = page.event("Overlay.inspectNodeRequested");
    let backend = result(page.call(None, "DOM.describeNode", json!({ "nodeId": a })))["node"]["backendNodeId"].clone();
    assert_eq!(inspect["params"]["backendNodeId"], backend);
    assert_eq!(evaluate(&mut page, "seen", None), 0);
    result(page.call(None, "Overlay.setInspectMode", json!({ "mode": "none" })));
    assert_eq!(screen(&mut page, 50, 50), [255, 255, 255]);
    mouse(&mut page, "mouseMoved");
    assert!(evaluate(&mut page, "seen", None).as_i64().unwrap() > 0);
}

/// What DevTools asks of the overlays machopo doesn't draw: none shown is
/// so; one to show is an error that says so.
#[test]
fn other_overlays_are_answered() {
    let (_machopo, mut page, _document, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    result(page.call(None, "Overlay.enable", json!({})));
    result(page.call(
        None,
        "Overlay.setShowGridOverlays",
        json!({ "gridNodeHighlightConfigs": [] }),
    ));
    result(page.call(
        None,
        "Overlay.setShowViewportSizeOnResize",
        json!({ "show": false }),
    ));
    assert_eq!(
        error(page.call(None, "Overlay.setShowPaintRects", json!({ "result": true }))),
        "Overlay.setShowPaintRects: machopo can't show it"
    );
}
