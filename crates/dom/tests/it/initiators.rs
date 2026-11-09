//! Who asked for the loads the DOM starts, for DevTools: each fetch the
//! host takes says (an `<img>`'s element, a background's stylesheet, a
//! script element's, a module's as the engine says); while the host wants
//! stacks, what script asks for in an operation is noted
//! (`Microtasks::stacks`: an `<img>`'s `src`, a `<link>` connected, a
//! script element inserted, an iframe connected or navigated, a
//! `fetch()`) for the engine to give the stack of after it, the first to
//! ask's kept until taken or cleared; nothing while it doesn't, nor what
//! the parser asks for.

use std::rc::Rc;
use std::sync::Arc;

use dom::{
    Cx, Dom, FetchHostRequest, FetchRequest, ImageFetch, LoadInitiator, NodeRef, QualName,
    RequestCredentials, RequestMode, RequestRedirect, ScriptRequest, StackAsker,
};

/// A document with a window and a `<body>`, at `https://example.test/`:
/// the `<body>`.
fn page<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    cx.set_document_url(document, "https://example.test/");
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

fn fetch(id: u64) -> FetchHostRequest {
    FetchHostRequest::Start(FetchRequest {
        id,
        document: dom::NodeId::from_raw(1),
        url: "https://example.test/data".into(),
        method: "GET".into(),
        headers: Vec::new(),
        body: None,
        mode: RequestMode::Cors,
        credentials: RequestCredentials::SameOrigin,
        redirect: RequestRedirect::Follow,
        initiator: "null".into(),
    })
}

/// What script asks for is noted while the host wants stacks, each asker
/// given the stack of the operation it asked in (the first to ask's), and
/// taken once; not before it wants them, nor once it doesn't.
#[test]
fn script_asks_with_the_stack_noted_after_the_operation() {
    let mut dom = Dom::new();
    let body = dom.mutate(|cx| cx.root(page(cx)));
    // Not wanted: nothing noted.
    let img = dom.mutate(|cx| {
        let body = body.get(cx.gc);
        let img = cx.create_element(cx.document_of(body), QualName::html("img"));
        cx.set_attribute(img, "src", "a.png");
        cx.append_child(body, img).unwrap();
        cx.root(img)
    });
    assert!(!dom.take_microtasks().stacks());
    dom.set_stacks_wanted(true, false);
    assert!(dom.request_stacks_wanted());
    let (img_id, link, script, iframe) = dom.mutate(|cx| {
        let (body, img) = (body.get(cx.gc), img.get(cx.gc));
        let document = cx.document_of(body);
        cx.set_attribute(img, "src", "b.png");
        let link = cx.create_element(document, QualName::html("link"));
        cx.set_attribute(link, "rel", "stylesheet");
        cx.set_attribute(link, "href", "s.css");
        cx.append_child(body, link).unwrap();
        let script = cx.create_element(document, QualName::html("script"));
        cx.set_attribute(script, "src", "a.js");
        cx.append_child(body, script).unwrap();
        let iframe = cx.create_element(document, QualName::html("iframe"));
        cx.append_child(body, iframe).unwrap();
        cx.request_fetch(fetch(9));
        (
            cx.node_id(img),
            cx.node_id(link),
            cx.node_id(script),
            cx.node_id(iframe),
        )
    });
    assert!(dom.take_microtasks().stacks());
    dom.note_request_stacks(Some(Rc::from("first")));
    // A later operation's stack is no earlier asker's.
    dom.mutate(|cx| cx.set_attribute(img.get(cx.gc), "src", "c.png"));
    assert!(dom.take_microtasks().stacks());
    dom.note_request_stacks(Some(Rc::from("second")));
    for asker in [
        StackAsker::Element(img_id),
        StackAsker::Element(link),
        StackAsker::Element(script),
        StackAsker::Element(iframe),
        StackAsker::Fetch(9),
    ] {
        assert_eq!(
            dom.request_stack(asker).as_deref(),
            Some("first"),
            "{asker:?}"
        );
        assert_eq!(dom.take_request_stack(asker).as_deref(), Some("first"));
        assert_eq!(dom.take_request_stack(asker), None, "once");
    }
    // No script ran (none known): nothing kept.
    dom.mutate(|cx| cx.set_attribute(img.get(cx.gc), "src", "d.png"));
    dom.note_request_stacks(None);
    dom.mutate(|cx| cx.request_fetch(fetch(10)));
    dom.note_request_stacks(Some(Rc::from("third")));
    assert_eq!(dom.take_request_stack(StackAsker::Element(img_id)), None);
    // What no load took goes.
    dom.clear_request_stacks();
    assert_eq!(dom.take_request_stack(StackAsker::Fetch(10)), None);
    // No more once not wanted.
    dom.mutate(|cx| cx.request_fetch(fetch(11)));
    assert!(dom.take_microtasks().stacks());
    dom.set_stacks_wanted(false, false);
    dom.note_request_stacks(Some(Rc::from("late")));
    assert_eq!(dom.take_request_stack(StackAsker::Fetch(11)), None);
    dom.mutate(|cx| cx.request_fetch(fetch(12)));
    assert!(!dom.take_microtasks().stacks());
}

/// With the frames' stacks wanted alone, an iframe that script connects
/// is noted, nothing else; what the parser does (its line set) is never
/// script's.
#[test]
fn frames_alone_and_the_parser() {
    let mut dom = Dom::new();
    let body = dom.mutate(|cx| cx.root(page(cx)));
    dom.set_stacks_wanted(false, true);
    let (img, iframe) = dom.mutate(|cx| {
        let body = body.get(cx.gc);
        let document = cx.document_of(body);
        let img = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(img, "src", "a.png");
        cx.append_child(body, img).unwrap();
        cx.request_fetch(fetch(1));
        let iframe = cx.create_element(document, QualName::html("iframe"));
        cx.append_child(body, iframe).unwrap();
        (cx.node_id(img), cx.node_id(iframe))
    });
    assert!(dom.take_microtasks().stacks());
    dom.note_request_stacks(Some(Rc::from("made")));
    assert_eq!(dom.take_request_stack(StackAsker::Element(img)), None);
    assert_eq!(dom.take_request_stack(StackAsker::Fetch(1)), None);
    assert_eq!(
        dom.take_request_stack(StackAsker::Element(iframe))
            .as_deref(),
        Some("made")
    );
    dom.set_stacks_wanted(true, true);
    dom.mutate(|cx| {
        let body = body.get(cx.gc);
        cx.set_parser_line(3);
        let img = cx.create_element(cx.document_of(body), QualName::html("img"));
        cx.set_attribute(img, "src", "b.png");
        cx.append_child(body, img).unwrap();
        cx.set_parser_line(0);
    });
    assert!(!dom.take_microtasks().stacks(), "the parser's");
}

/// The fetches the host takes say who asked: an `<img>` its element, a
/// background its stylesheet (none: no one), a script element its own, a
/// module what the engine said.
#[test]
fn fetches_say_who_asked() {
    let mut dom = Dom::new();
    let (img, script, document) = dom.mutate(|cx| {
        let body = page(cx);
        let document = cx.document_of(body);
        let img = cx.create_element(document, QualName::html("img"));
        cx.set_attribute(img, "src", "a.png");
        cx.append_child(body, img).unwrap();
        for element in cx.take_image_updates() {
            cx.run_image_update(element, cx.image_src(element));
        }
        let id = cx.node_id(document);
        let sheet: Arc<str> = Arc::from("https://example.test/s.css");
        assert!(
            cx.background_image("https://example.test/bg.png", id, Some(&sheet))
                .is_none()
        );
        assert!(
            cx.background_image("https://example.test/no.png", id, None)
                .is_none()
        );
        let script = cx.create_element(document, QualName::html("script"));
        cx.append_child(body, script).unwrap();
        let request = |url: &str| ScriptRequest {
            url: url.to_owned(),
            cors: None,
            charset: None,
        };
        cx.queue_script(script, request("https://example.test/a.js"), false);
        cx.queue_module_fetch(
            document,
            request("https://example.test/m.js"),
            LoadInitiator::Import("https://example.test/root.js".into()),
        );
        (cx.node_id(img), cx.node_id(script), id)
    });
    assert_eq!(
        dom.take_image_fetches(),
        [
            ImageFetch {
                url: "https://example.test/a.png".into(),
                document,
                initiator: LoadInitiator::Element(img),
            },
            ImageFetch {
                url: "https://example.test/bg.png".into(),
                document,
                initiator: LoadInitiator::Stylesheet(Arc::from("https://example.test/s.css")),
            },
            ImageFetch {
                url: "https://example.test/no.png".into(),
                document,
                initiator: LoadInitiator::Other,
            },
        ]
    );
    let scripts: Vec<LoadInitiator> = dom
        .take_script_fetches()
        .into_iter()
        .map(|fetch| fetch.initiator)
        .collect();
    assert_eq!(
        scripts,
        [
            LoadInitiator::Element(script),
            LoadInitiator::Import("https://example.test/root.js".into())
        ]
    );
}
