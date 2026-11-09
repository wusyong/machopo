//! DevTools' `DOM` domain, which the tab's renderer answers: the document
//! as Chrome's DevTools asks for it (a few levels at first, children as
//! asked, text of white space alone left out), its nodes numbered for the
//! session; script's changes told for the nodes the session has, in
//! order; queries that send the nodes above what they find; iframes'
//! documents, and an iframe told again with its next one; a new document
//! told; answers while script is paused, with what script did up to there.

use net::test_server::Route;
use serde_json::{Value, json};

use crate::automation_pipe::server;
use crate::devtools::{evaluate, load, page};
use crate::remote_debugging::{Client, Machopo};

const PAGE: &str = "<!DOCTYPE html><html><head><title>T</title></head><body>\n  \
                    <div id=a class='x y'>text</div>\n  <p></p>\n</body></html>";

/// The answer's `result`, failing on an error.
fn result(answer: Value) -> Value {
    assert!(answer.get("error").is_none(), "{answer}");
    answer["result"].clone()
}

/// The answer's error message, failing on a result.
fn error(answer: Value) -> String {
    answer["error"]["message"]
        .as_str()
        .unwrap_or_else(|| panic!("an error: {answer}"))
        .to_owned()
}

/// The `DOM` events of what script did so far: the agent tells them
/// before it answers a command.
fn dom_events(page: &mut Client) -> Vec<Value> {
    result(page.call(None, "DOM.getTopLayerElements", json!({})));
    page.take_events("DOM.")
}

/// The methods of `events`, in order.
fn methods(events: &[Value]) -> Vec<&str> {
    events
        .iter()
        .map(|event| event["method"].as_str().unwrap())
        .collect()
}

/// A page served from `/`, loaded in machopo's first tab, with `DOM` on.
fn dom_page(routes: &[(&str, Route)]) -> (Machopo, Client, String, net::test_server::TestServer) {
    let server = server(routes);
    let machopo = Machopo::start();
    let (mut page, target) = page(&machopo);
    load(&mut page, &server.url("/"));
    result(page.call(None, "DOM.enable", json!({})));
    (machopo, page, target, server)
}

/// The child of `node` named `name`.
fn child<'a>(node: &'a Value, name: &str) -> &'a Value {
    node["children"]
        .as_array()
        .unwrap_or_else(|| panic!("children: {node}"))
        .iter()
        .find(|child| child["nodeName"] == name)
        .unwrap_or_else(|| panic!("no {name} in {node}"))
}

/// The document as `getDocument` sends it: two levels down, every
/// container with its count of children; the doctype's ids, the root
/// element's frame; the body's children once asked for, as
/// `setChildNodes` before the answer, white space left out and a lone
/// text child sent with its element.
#[test]
fn the_document_is_sent_as_asked_for() {
    let (_machopo, mut page, target, server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    assert_eq!(root["nodeType"], 9, "{root}");
    assert_eq!(root["nodeName"], "#document");
    assert_eq!(root["documentURL"], server.url("/"));
    assert_eq!(root["compatibilityMode"], "NoQuirksMode");
    assert_eq!(root["childNodeCount"], 2);
    let doctype = child(&root, "html");
    assert_eq!(doctype["nodeType"], 10);
    assert_eq!(doctype["publicId"], "");
    let html = child(&root, "HTML");
    assert_eq!(html["localName"], "html");
    assert_eq!(html["frameId"], target.as_str());
    assert_eq!(html["childNodeCount"], 2);
    let body = child(html, "BODY");
    assert_eq!(body["childNodeCount"], 2, "white space left out: {body}");
    assert!(body.get("children").is_none(), "{body}");
    assert_eq!(body["parentId"], html["nodeId"]);
    assert_ne!(body["nodeId"], html["nodeId"]);
    assert!(body["backendNodeId"].as_u64().unwrap() > 0);

    let id = page.send(
        None,
        "DOM.requestChildNodes",
        json!({ "nodeId": body["nodeId"] }),
    );
    let set = page.event("DOM.setChildNodes");
    // Before the answer: the front end reads the children once answered.
    assert!(!page.answered(id));
    result(page.answer(id));
    assert_eq!(set["params"]["parentId"], body["nodeId"]);
    let nodes = set["params"]["nodes"].as_array().unwrap();
    assert_eq!(
        nodes
            .iter()
            .map(|node| &node["nodeName"])
            .collect::<Vec<_>>(),
        ["DIV", "P"]
    );
    let div = &nodes[0];
    assert_eq!(div["attributes"], json!(["id", "a", "class", "x y"]));
    assert_eq!(div["children"][0]["nodeValue"], "text");
    assert_eq!(div["children"][0]["parentId"], div["nodeId"]);
    assert!(nodes[1].get("children").is_none(), "{}", nodes[1]);

    assert_eq!(
        error(page.call(
            None,
            "DOM.requestChildNodes",
            json!({ "nodeId": body["nodeId"], "depth": 0 })
        )),
        "Please provide a positive integer as a depth or -1 for entire subtree"
    );
    // Asked for again: sent already, not again.
    result(page.call(
        None,
        "DOM.requestChildNodes",
        json!({ "nodeId": body["nodeId"] }),
    ));
    assert!(page.take_events("DOM.setChildNodes").is_empty());

    // Described, the children it wasn't sent get no numbers.
    let head = child(html, "HEAD");
    let described = result(page.call(
        None,
        "DOM.describeNode",
        json!({ "nodeId": head["nodeId"], "depth": 1 }),
    ))["node"]
        .clone();
    assert_eq!(described["nodeId"], head["nodeId"]);
    assert_eq!(described["children"][0]["nodeName"], "TITLE");
    assert_eq!(described["children"][0]["nodeId"], 0, "{described}");

    // The document asked for again: new numbers, the old ones gone.
    let again = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    assert_ne!(again["nodeId"], root["nodeId"]);
    assert_eq!(
        error(page.call(
            None,
            "DOM.getAttributes",
            json!({ "nodeId": div["nodeId"] })
        )),
        "Could not find node with given id"
    );
}

/// What script changes is told for the nodes the session has: a child put
/// in after the last one it has, attributes set and removed, text, a
/// count of children it wasn't sent, an inline style, a shadow root, a
/// child taken out; text of white space alone isn't.
#[test]
fn script_changes_are_told() {
    let (_machopo, mut page, _target, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    let body = child(child(&root, "HTML"), "BODY")["nodeId"].clone();
    result(page.call(None, "DOM.requestChildNodes", json!({ "nodeId": body })));
    let set = page.event("DOM.setChildNodes");
    let div = set["params"]["nodes"][0]["nodeId"].clone();
    let text = set["params"]["nodes"][0]["children"][0]["nodeId"].clone();
    let p = set["params"]["nodes"][1]["nodeId"].clone();

    evaluate(
        &mut page,
        "document.body.append(document.createElement('span'), ' ')",
        None,
    );
    let events = dom_events(&mut page);
    assert_eq!(methods(&events), ["DOM.childNodeInserted"], "{events:?}");
    let inserted = &events[0]["params"];
    assert_eq!(inserted["parentNodeId"], body);
    assert_eq!(inserted["previousNodeId"], p);
    assert_eq!(inserted["node"]["localName"], "span");
    let span = inserted["node"]["nodeId"].clone();

    evaluate(
        &mut page,
        "const a = document.getElementById('a');
         a.setAttribute('title', 'hi');
         a.removeAttribute('class');
         a.firstChild.data = 'changed';
         document.querySelector('p').append('x');
         a.style.color = 'red';",
        None,
    );
    let events = dom_events(&mut page);
    assert_eq!(
        methods(&events),
        [
            "DOM.attributeModified",
            "DOM.attributeRemoved",
            "DOM.characterDataModified",
            "DOM.childNodeCountUpdated",
            "DOM.inlineStyleInvalidated",
        ],
        "{events:?}"
    );
    assert_eq!(
        events[0]["params"],
        json!({ "nodeId": div, "name": "title", "value": "hi" })
    );
    assert_eq!(
        events[1]["params"],
        json!({ "nodeId": div, "name": "class" })
    );
    assert_eq!(
        events[2]["params"],
        json!({ "nodeId": text, "characterData": "changed" })
    );
    assert_eq!(
        events[3]["params"],
        json!({ "nodeId": p, "childNodeCount": 1 })
    );
    // Not as the `style` attribute: CSSOM changed the declarations, the
    // attribute written from them when next read (as Blink tells it).
    assert_eq!(events[4]["params"], json!({ "nodeIds": [div] }));

    evaluate(
        &mut page,
        "document.querySelector('p').attachShadow({ mode: 'open' });
         document.querySelector('span').remove();",
        None,
    );
    let events = dom_events(&mut page);
    assert_eq!(
        methods(&events),
        [
            "DOM.setChildNodes",
            "DOM.shadowRootPushed",
            "DOM.childNodeRemoved"
        ],
        "{events:?}"
    );
    assert_eq!(events[0]["params"]["parentId"], p);
    assert_eq!(events[1]["params"]["hostId"], p);
    assert_eq!(events[1]["params"]["root"]["shadowRootType"], "open");
    assert_eq!(
        events[2]["params"],
        json!({ "parentNodeId": body, "nodeId": span })
    );
    // Taken out, it has no number now.
    assert_eq!(
        error(page.call(None, "DOM.getAttributes", json!({ "nodeId": span }))),
        "Could not find node with given id"
    );
}

/// The changes of a batch are told as the DOM is once it is through: nodes
/// put in out of order each follow the closest sibling the session has,
/// so they end in the DOM's order; a node put in and taken out again
/// isn't told.
#[test]
fn a_batch_is_told_in_the_doms_order() {
    let (_machopo, mut page, _target, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    let body = child(child(&root, "HTML"), "BODY")["nodeId"].clone();
    result(page.call(None, "DOM.requestChildNodes", json!({ "nodeId": body })));
    let set = page.event("DOM.setChildNodes");
    let p = set["params"]["nodes"][1]["nodeId"].clone();
    evaluate(
        &mut page,
        "const b = document.createElement('b');
         document.body.append(b);
         document.body.insertBefore(document.createElement('i'), b);
         const u = document.createElement('u');
         document.body.append(u);
         u.remove();",
        None,
    );
    let events = dom_events(&mut page);
    assert_eq!(
        methods(&events),
        ["DOM.childNodeInserted", "DOM.childNodeInserted"],
        "{events:?}"
    );
    assert_eq!(events[0]["params"]["node"]["localName"], "b");
    assert_eq!(events[0]["params"]["previousNodeId"], p);
    assert_eq!(events[1]["params"]["node"]["localName"], "i");
    assert_eq!(events[1]["params"]["previousNodeId"], p);
}

/// Queries send the nodes above what they find; a node is described by
/// its backend id with no number given; its markup; a frame's iframe.
#[test]
fn queries_send_the_path_to_what_they_find() {
    let html = "<!DOCTYPE html><body><section><div class='a b'><em>x</em></div></section>\
                <iframe src='/child'></iframe></body>";
    let (_machopo, mut page, _target, _server) = dom_page(&[
        ("/", Route::ok("text/html", html)),
        (
            "/child",
            Route::ok("text/html", "<!DOCTYPE html><p class=c>child</p>"),
        ),
    ]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    let document = root["nodeId"].clone();
    let found = result(page.call(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": "em" }),
    ))["nodeId"]
        .clone();
    let sets = page.take_events("DOM.setChildNodes");
    // The body's children, the section's, the div's: the path down.
    assert_eq!(sets.len(), 3, "{sets:?}");
    assert_eq!(sets[2]["params"]["nodes"][0]["nodeId"], found);
    let all = result(page.call(
        None,
        "DOM.querySelectorAll",
        json!({ "nodeId": document, "selector": "div, em" }),
    ))["nodeIds"]
        .clone();
    assert_eq!(all.as_array().unwrap().len(), 2);
    assert_eq!(all[1], found);
    assert_eq!(
        result(page.call(None, "DOM.getAttributes", json!({ "nodeId": all[0] })))["attributes"],
        json!(["class", "a b"])
    );
    assert_eq!(
        result(page.call(
            None,
            "DOM.collectClassNamesFromSubtree",
            json!({ "nodeId": document })
        ))["classNames"],
        json!(["a", "b"])
    );
    let described =
        result(page.call(None, "DOM.describeNode", json!({ "nodeId": found })))["node"].clone();
    let backend = described["backendNodeId"].clone();
    let by_backend = result(page.call(
        None,
        "DOM.describeNode",
        json!({ "backendNodeId": backend, "depth": 1 }),
    ))["node"]
        .clone();
    assert_eq!(by_backend["nodeId"], found);
    assert_eq!(by_backend["children"][0]["nodeValue"], "x");
    assert_eq!(
        result(page.call(None, "DOM.getOuterHTML", json!({ "nodeId": all[0] })))["outerHTML"],
        "<div class=\"a b\"><em>x</em></div>"
    );
    assert_eq!(
        result(page.call(
            None,
            "DOM.pushNodesByBackendIdsToFrontend",
            json!({ "backendNodeIds": [backend, 999_999_999] })
        ))["nodeIds"],
        json!([found, 0])
    );
    assert_eq!(
        error(page.call(
            None,
            "DOM.querySelector",
            json!({ "nodeId": 999_999, "selector": "p" })
        )),
        "Could not find node with given id"
    );
    assert_eq!(
        error(page.call(
            None,
            "DOM.querySelector",
            json!({ "nodeId": document, "selector": "p[" })
        )),
        "DOM Error while querying"
    );

    // The iframe: its frame, its document (none of its children), and the
    // frame's owner back.
    let iframe = result(page.call(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": "iframe" }),
    ))["nodeId"]
        .clone();
    let described =
        result(page.call(None, "DOM.describeNode", json!({ "nodeId": iframe })))["node"].clone();
    let frame = described["frameId"].as_str().unwrap().to_owned();
    assert_eq!(described["contentDocument"]["nodeName"], "#document");
    assert_eq!(described["contentDocument"]["childNodeCount"], 2);
    let owner = result(page.call(None, "DOM.getFrameOwner", json!({ "frameId": frame })));
    assert_eq!(owner["nodeId"], iframe);
    assert_eq!(owner["backendNodeId"], described["backendNodeId"]);
}

/// An iframe navigated is told taken out and put back, with its new
/// document; a new top-level document is told once parsed, and the old
/// numbers go.
#[test]
fn new_documents_are_told() {
    let html = "<!DOCTYPE html><body><iframe src='/child'></iframe></body>";
    let (_machopo, mut page, _target, server) = dom_page(&[
        ("/", Route::ok("text/html", html)),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>one")),
        ("/other", Route::ok("text/html", "<!DOCTYPE html>two")),
        ("/next", Route::ok("text/html", "<!DOCTYPE html>next")),
    ]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    let body = child(child(&root, "HTML"), "BODY")["nodeId"].clone();
    result(page.call(None, "DOM.requestChildNodes", json!({ "nodeId": body })));
    let iframe = page.event("DOM.setChildNodes")["params"]["nodes"][0].clone();
    let before = iframe["contentDocument"]["backendNodeId"].clone();
    evaluate(
        &mut page,
        "document.querySelector('iframe').src = '/other'",
        None,
    );
    let inserted = page.event("DOM.childNodeInserted");
    let removed = page.take_events("DOM.childNodeRemoved");
    assert_eq!(
        removed[0]["params"],
        json!({ "parentNodeId": body, "nodeId": iframe["nodeId"] })
    );
    let node = &inserted["params"]["node"];
    assert_eq!(inserted["params"]["parentNodeId"], body);
    assert_eq!(node["localName"], "iframe");
    assert_ne!(node["contentDocument"]["backendNodeId"], before);

    load(&mut page, &server.url("/next"));
    page.event("DOM.documentUpdated");
    // The old numbers are gone.
    assert_eq!(
        error(page.call(
            None,
            "DOM.getAttributes",
            json!({ "nodeId": iframe["nodeId"] })
        )),
        "Could not find node with given id"
    );
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    assert_eq!(root["documentURL"], server.url("/next"));
}

/// While script is paused, the session is told what it did up to there,
/// and its commands are answered with the DOM as it is then.
#[test]
fn the_dom_is_there_while_paused() {
    let (_machopo, mut page, _target, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    let document = root["nodeId"].clone();
    let body = child(child(&root, "HTML"), "BODY")["nodeId"].clone();
    result(page.call(None, "DOM.requestChildNodes", json!({ "nodeId": body })));
    page.event("DOM.setChildNodes");
    page.call(None, "Debugger.enable", json!({}));
    let evaluated = page.send(
        None,
        "Runtime.evaluate",
        json!({ "expression": "document.body.append(document.createElement('em')); debugger; 1" }),
    );
    page.event("Debugger.paused");
    let inserted = page.event("DOM.childNodeInserted");
    assert_eq!(inserted["params"]["node"]["localName"], "em");
    let found = result(page.call(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": "em" }),
    ))["nodeId"]
        .clone();
    assert_eq!(found, inserted["params"]["node"]["nodeId"]);
    // What script run while paused did, told before the next answer: no
    // task ends meanwhile.
    evaluate(
        &mut page,
        "document.body.append(document.createElement('strong'))",
        None,
    );
    let id = page.send(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": "strong" }),
    );
    let inserted = page.event("DOM.childNodeInserted");
    assert!(!page.answered(id), "told before the answer");
    let found = result(page.answer(id))["nodeId"].clone();
    assert_eq!(found, inserted["params"]["node"]["nodeId"]);
    // The node's object, while paused.
    let resolved =
        result(page.call(None, "DOM.resolveNode", json!({ "nodeId": found })))["object"].clone();
    assert_eq!(resolved["description"], "strong");
    page.call(None, "Debugger.resume", json!({}));
    page.answer(evaluated);
}

/// `DOM` turned off, it can't be turned off again; a session's numbers
/// are its own, and go with it.
#[test]
fn each_session_has_its_own_numbers() {
    let (machopo, mut page, target, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let root = result(page.call(None, "DOM.getDocument", json!({ "depth": -1 })))["root"].clone();
    let url = format!("ws://127.0.0.1:{}/devtools/page/{target}", machopo.port);
    let mut other = machopo.connect(&url, None).expect("connects");
    // Not asked for its document: none of the first's numbers.
    let html = child(&root, "HTML")["nodeId"].clone();
    assert_eq!(
        error(other.call(None, "DOM.getAttributes", json!({ "nodeId": html }))),
        "Could not find node with given id"
    );
    let theirs = result(other.call(None, "DOM.getDocument", json!({})))["root"].clone();
    assert_eq!(theirs["nodeId"], root["nodeId"], "both number from 1");
    result(page.call(None, "DOM.disable", json!({})));
    assert_eq!(
        error(page.call(None, "DOM.disable", json!({}))),
        "DOM agent hasn't been enabled"
    );
    // The other still has its numbers.
    let attributes = other.call(
        None,
        "DOM.getAttributes",
        json!({ "nodeId": child(&theirs, "HTML")["nodeId"] }),
    );
    assert_eq!(result(attributes)["attributes"], json!([]));
}

/// `Runtime.evaluate`'s RemoteObject of `expression`.
fn object(page: &mut Client, expression: &str) -> Value {
    let answer = page.call(
        None,
        "Runtime.evaluate",
        json!({ "expression": expression, "objectGroup": "test" }),
    );
    answer["result"]["result"].clone()
}

/// Nodes and script's objects for them, both ways: the console's objects
/// of nodes are nodes, described as Chrome describes them; an object's
/// node numbered (with the path to it), a node's object made; `$0` the
/// node inspected; a node no document has sent as a child of none and
/// kept.
#[test]
fn nodes_and_their_objects_are_linked() {
    let (_machopo, mut page, _target, _server) = dom_page(&[("/", Route::ok("text/html", PAGE))]);
    let root = result(page.call(None, "DOM.getDocument", json!({})))["root"].clone();
    let document = root["nodeId"].clone();

    let div = object(&mut page, "document.getElementById('a')");
    assert_eq!(div["subtype"], "node", "{div}");
    assert_eq!(div["className"], "HTMLDivElement");
    assert_eq!(div["description"], "div#a.x.y");
    assert_eq!(object(&mut page, "document")["description"], "#document");
    assert_eq!(
        object(&mut page, "document.doctype")["description"],
        "<!DOCTYPE html>"
    );
    assert_eq!(
        object(&mut page, "document.getElementById('a').firstChild")["description"],
        "#text"
    );
    assert!(object(&mut page, "({})").get("subtype").is_none());

    // The object's node, numbered with the path down to it.
    let id = result(page.call(
        None,
        "DOM.requestNode",
        json!({ "objectId": div["objectId"] }),
    ))["nodeId"]
        .clone();
    assert!(!page.take_events("DOM.setChildNodes").is_empty());
    let found = result(page.call(
        None,
        "DOM.querySelector",
        json!({ "nodeId": document, "selector": "#a" }),
    ))["nodeId"]
        .clone();
    assert_eq!(id, found);
    assert_eq!(
        result(page.call(
            None,
            "DOM.describeNode",
            json!({ "objectId": div["objectId"] })
        ))["node"]["nodeId"],
        id
    );

    // The node's object, in the group asked for.
    let resolved = result(page.call(
        None,
        "DOM.resolveNode",
        json!({ "nodeId": id, "objectGroup": "g" }),
    ))["object"]
        .clone();
    assert_eq!(resolved["description"], "div#a.x.y");
    let called = page.call(
        None,
        "Runtime.callFunctionOn",
        json!({
            "objectId": resolved["objectId"],
            "functionDeclaration": "function() { return this.className }",
            "returnByValue": true,
        }),
    );
    assert_eq!(called["result"]["result"]["value"], "x y", "{called}");
    let backend = result(page.call(None, "DOM.describeNode", json!({ "nodeId": id })))["node"]
        ["backendNodeId"]
        .clone();
    assert_eq!(
        result(page.call(None, "DOM.resolveNode", json!({ "backendNodeId": backend })))["object"]["description"],
        "div#a.x.y"
    );
    assert_eq!(
        error(page.call(None, "DOM.resolveNode", json!({}))),
        "Either nodeId or backendNodeId must be specified."
    );
    assert_eq!(
        error(page.call(None, "DOM.resolveNode", json!({ "nodeId": 999_999 }))),
        "No node with given id found"
    );
    assert_eq!(
        error(page.call(
            None,
            "DOM.resolveNode",
            json!({ "backendNodeId": 999_999_999 })
        )),
        "No node with given id found"
    );
    let plain = object(&mut page, "({})");
    assert_eq!(
        error(page.call(
            None,
            "DOM.requestNode",
            json!({ "objectId": plain["objectId"] })
        )),
        "Object id doesn't reference a Node"
    );
    assert!(
        page.call(None, "DOM.requestNode", json!({ "objectId": "nothing" }))["error"].is_object()
    );

    // `$0`.
    result(page.call(None, "DOM.setInspectedNode", json!({ "nodeId": id })));
    let inspected = page.call(
        None,
        "Runtime.evaluate",
        json!({
            "expression": "$0 === document.getElementById('a')",
            "includeCommandLineAPI": true,
            "returnByValue": true,
        }),
    );
    assert_eq!(inspected["result"]["result"]["value"], true, "{inspected}");

    // A node of no document's tree: its subtree's root sent as a child of
    // none, then the path down; kept while the session has it.
    let loose = object(
        &mut page,
        "window.loose = document.createElement('section');
         loose.append(document.createElement('b'));
         loose.firstChild",
    );
    let b = result(page.call(
        None,
        "DOM.requestNode",
        json!({ "objectId": loose["objectId"] }),
    ))["nodeId"]
        .clone();
    let sets = page.take_events("DOM.setChildNodes");
    assert_eq!(sets[0]["params"]["parentId"], 0, "{sets:?}");
    assert_eq!(sets[0]["params"]["nodes"][0]["localName"], "section");
    assert_eq!(sets[1]["params"]["nodes"][0]["nodeId"], b);
    // Script lets go of it, and the GC runs: the session keeps it.
    evaluate(&mut page, "window.loose = null", None);
    page.call(
        None,
        "Runtime.releaseObjectGroup",
        json!({ "objectGroup": "test" }),
    );
    result(page.call(None, "HeapProfiler.collectGarbage", json!({})));
    assert_eq!(
        result(page.call(None, "DOM.getAttributes", json!({ "nodeId": b })))["attributes"],
        json!([])
    );
}

/// A target's listeners as the Elements panel lists them: those that
/// capture first, then by type as the types were first added; a content
/// attribute's handler compiled; an object's `handleEvent`; their flags,
/// where their functions are, their objects in the group asked; the
/// nodes below `depth` levels down; a window's, with no node.
#[test]
fn event_listeners_are_listed() {
    let html = "<!DOCTYPE html><body><button id=b onclick='go()'>x</button><script>\n\
                function go() {}\n\
                b.addEventListener('click', function f() {}, true);\n\
                b.addEventListener('mouseover', { handleEvent() {} });\n\
                document.body.addEventListener('keydown', () => {}, { passive: true, once: true });\n\
                window.addEventListener('load', function w() {});\n\
                </script>";
    let (_machopo, mut page, _target, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    let button = object(&mut page, "b");
    let listeners = result(page.call(
        None,
        "DOMDebugger.getEventListeners",
        json!({ "objectId": button["objectId"] }),
    ))["listeners"]
        .clone();
    let listed: Vec<(String, bool, String)> = listeners
        .as_array()
        .unwrap()
        .iter()
        .map(|listener| {
            (
                listener["type"].as_str().unwrap().to_owned(),
                listener["useCapture"].as_bool().unwrap(),
                listener["handler"]["description"]
                    .as_str()
                    .unwrap_or_default()
                    .replace('\n', " "),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [
            ("click".to_owned(), true, "function f() {}".to_owned()),
            // Compiled with no name (Chrome's is `function onclick(event)`).
            (
                "click".to_owned(),
                false,
                "function (event) { go() }".to_owned()
            ),
            ("mouseover".to_owned(), false, "handleEvent() {}".to_owned()),
        ],
        "{listeners}"
    );
    let f = &listeners[0];
    assert_eq!(f["lineNumber"], 2, "{f}");
    assert!(f["scriptId"].as_str().is_some_and(|id| !id.is_empty()));
    assert_eq!(
        f["originalHandler"]["description"],
        f["handler"]["description"]
    );
    assert_eq!(listeners[2]["originalHandler"]["type"], "object");
    let backend = result(page.call(
        None,
        "DOM.describeNode",
        json!({ "objectId": button["objectId"] }),
    ))["node"]["backendNodeId"]
        .clone();
    assert_eq!(f["backendNodeId"], backend);

    // The body's, and below it all the way down.
    let body = object(&mut page, "document.body");
    let all = result(page.call(
        None,
        "DOMDebugger.getEventListeners",
        json!({ "objectId": body["objectId"], "depth": -1 }),
    ))["listeners"]
        .clone();
    let types: Vec<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|listener| listener["type"].as_str().unwrap())
        .collect();
    assert_eq!(types, ["click", "keydown", "click", "mouseover"]);
    let keydown = &all[1];
    assert_eq!(keydown["passive"], true);
    assert_eq!(keydown["once"], true);

    // The window's: no node.
    let window = object(&mut page, "window");
    let theirs = result(page.call(
        None,
        "DOMDebugger.getEventListeners",
        json!({ "objectId": window["objectId"] }),
    ))["listeners"]
        .clone();
    assert_eq!(theirs[0]["type"], "load");
    assert!(theirs[0].get("backendNodeId").is_none(), "{theirs}");
}

/// The Elements panel's search: a selector's matches, text, a tag's name
/// (`<p>` the whole name), an attribute's name or value; the results
/// pushed as asked for, the range checked, discarded.
#[test]
fn the_document_is_searched() {
    let html = "<!DOCTYPE html><body><p class=note>Hello there</p><pre data-kind=code>hello</pre>\
                <section title=codebase><!-- a hello comment --></section></body>";
    let (_machopo, mut page, _target, _server) = dom_page(&[("/", Route::ok("text/html", html))]);
    result(page.call(None, "DOM.getDocument", json!({})));
    let search = |page: &mut Client, query: &str| {
        let answer = result(page.call(None, "DOM.performSearch", json!({ "query": query })));
        (
            answer["searchId"].as_str().unwrap().to_owned(),
            answer["resultCount"].clone(),
        )
    };
    // The text, the `<pre>`'s, the comment.
    assert_eq!(search(&mut page, "hello").1, 3);
    // A selector, then the text that has it.
    assert_eq!(search(&mut page, ".note").1, 1);
    assert_eq!(search(&mut page, "<p>").1, 1);
    // An attribute's whole value quoted, a part of it not.
    assert_eq!(search(&mut page, "\"code\"").1, 1);
    assert_eq!(search(&mut page, "code").1, 2);
    assert_eq!(search(&mut page, "data-kind").1, 1);
    let (id, count) = search(&mut page, "hello");
    let count = count.as_u64().unwrap();
    let ids = result(page.call(
        None,
        "DOM.getSearchResults",
        json!({ "searchId": id, "fromIndex": 0, "toIndex": count }),
    ))["nodeIds"]
        .clone();
    assert!(
        ids.as_array()
            .unwrap()
            .iter()
            .all(|id| id.as_i64().unwrap() > 0),
        "{ids}"
    );
    assert_eq!(
        error(page.call(
            None,
            "DOM.getSearchResults",
            json!({ "searchId": id, "fromIndex": 0, "toIndex": count + 1 }),
        )),
        "Invalid search result range"
    );
    result(page.call(None, "DOM.discardSearchResults", json!({ "searchId": id })));
    assert_eq!(
        error(page.call(
            None,
            "DOM.getSearchResults",
            json!({ "searchId": id, "fromIndex": 0, "toIndex": 1 }),
        )),
        "No search session with given id found"
    );
}
