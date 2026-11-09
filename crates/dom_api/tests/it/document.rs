//! `Document`'s members: creating elements (names checked, lowercased in an
//! HTML document, "validate and extract" for a namespaced one) and texts,
//! the document element and `ParentNode`'s members.

use dom_api::{ApiError, MemberKind};

use crate::{Arg, Ret, TestDom, document, exception, get, method, node, rc};

fn create(
    dom: &mut TestDom,
    doc: &std::rc::Rc<dom::NodeRoot>,
    name: &'static str,
) -> Result<Ret, ApiError> {
    method(dom, doc, "Document", "createElement", &[Arg::Str(name)])
}

#[test]
fn create_element_checks_the_name_and_lowercases_it_in_an_html_document() {
    let mut dom = TestDom::new();
    let html = document(&mut dom, true);
    let xml = document(&mut dom, false);
    for name in ["", "a b", "a>", "a/b", "1x", "-a", "_a b", "é>"] {
        let error = create(&mut dom, &html, name).unwrap_err();
        assert_eq!(exception(error), "InvalidCharacterError", "{name:?}");
    }
    for name in ["a<b", "a=b", "_a", ":a", "é-1", "_a.b:c"] {
        assert!(create(&mut dom, &html, name).is_ok(), "{name:?}");
    }
    let div = rc(create(&mut dom, &html, "DiV").unwrap());
    assert_eq!(get(&mut dom, &div, "Element", "localName").string(), "div");
    assert_eq!(
        get(&mut dom, &div, "Element", "namespaceURI").string(),
        "http://www.w3.org/1999/xhtml"
    );
    assert_eq!(get(&mut dom, &div, "Element", "tagName").string(), "DIV");
    // An XML document: the name as it is, in no namespace.
    let in_xml = rc(create(&mut dom, &xml, "DiV").unwrap());
    assert_eq!(
        get(&mut dom, &in_xml, "Element", "localName").string(),
        "DiV"
    );
    assert!(matches!(
        get(&mut dom, &in_xml, "Element", "namespaceURI"),
        Ret::Null
    ));
    assert_eq!(get(&mut dom, &in_xml, "Element", "tagName").string(), "DiV");
}

#[test]
fn create_element_ns_validates_and_extracts() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let create = |dom: &mut TestDom, namespace: Arg, qualified: &'static str| {
        method(
            dom,
            &doc,
            "Document",
            "createElementNS",
            &[namespace, Arg::Str(qualified)],
        )
    };
    const SVG: &str = "http://www.w3.org/2000/svg";
    let svg = rc(create(&mut dom, Arg::Str(SVG), "s:rect").unwrap());
    assert_eq!(get(&mut dom, &svg, "Element", "prefix").string(), "s");
    assert_eq!(get(&mut dom, &svg, "Element", "localName").string(), "rect");
    assert_eq!(get(&mut dom, &svg, "Element", "namespaceURI").string(), SVG);
    assert_eq!(get(&mut dom, &svg, "Element", "tagName").string(), "s:rect");
    // The empty namespace is null; no prefix.
    let plain = rc(create(&mut dom, Arg::Str(""), "Plain").unwrap());
    assert!(matches!(
        get(&mut dom, &plain, "Element", "namespaceURI"),
        Ret::Null
    ));
    assert!(matches!(
        get(&mut dom, &plain, "Element", "prefix"),
        Ret::Null
    ));
    assert_eq!(
        get(&mut dom, &plain, "Element", "localName").string(),
        "Plain"
    );
    let errors: [(Arg, &str, &str); 9] = [
        // An invalid prefix or local name.
        (Arg::Str(SVG), " :a", "InvalidCharacterError"),
        (Arg::Str(SVG), ":a", "InvalidCharacterError"),
        (Arg::Str(SVG), "a:", "InvalidCharacterError"),
        (Arg::Str(SVG), "a:1b", "InvalidCharacterError"),
        // A prefix with no namespace.
        (Arg::Null, "a:b", "NamespaceError"),
        (Arg::Str(""), "a:b", "NamespaceError"),
        // `xml` only in its namespace; `xmlns` only in its, and its only.
        (Arg::Str(SVG), "xml:a", "NamespaceError"),
        (Arg::Str(SVG), "xmlns", "NamespaceError"),
        (
            Arg::Str("http://www.w3.org/2000/xmlns/"),
            "a",
            "NamespaceError",
        ),
    ];
    for (namespace, qualified, name) in errors {
        let error = create(&mut dom, namespace, qualified).unwrap_err();
        assert_eq!(exception(error), name, "{qualified:?}");
    }
    let xml = rc(create(
        &mut dom,
        Arg::Str("http://www.w3.org/XML/1998/namespace"),
        "xml:a",
    )
    .unwrap());
    assert_eq!(get(&mut dom, &xml, "Element", "prefix").string(), "xml");
    let xmlns = create(
        &mut dom,
        Arg::Str("http://www.w3.org/2000/xmlns/"),
        "xmlns:a",
    );
    assert!(xmlns.is_ok());
    let xmlns = create(&mut dom, Arg::Str("http://www.w3.org/2000/xmlns/"), "xmlns");
    assert!(xmlns.is_ok());
}

#[test]
fn the_document_element_and_parent_node_members() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    assert!(matches!(
        get(&mut dom, &doc, "Document", "documentElement"),
        Ret::Null
    ));
    assert!(matches!(
        get(&mut dom, &doc, "Document", "childElementCount"),
        Ret::U32(0)
    ));
    let html = rc(create(&mut dom, &doc, "html").unwrap());
    let arg = node(&html, &mut dom);
    method(&mut dom, &doc, "Node", "appendChild", &[arg]).unwrap();
    let root = get(&mut dom, &doc, "Document", "documentElement");
    dom.mutate(|cx| assert!(elise::Gc::ptr_eq(root.node().get(cx.gc), html.get(cx.gc))));
    // An element's: texts between are skipped.
    let [a, b] = ["a", "b"].map(|name| rc(create(&mut dom, &doc, name).unwrap()));
    for (child, text) in [(&a, "1"), (&b, "2")] {
        let text = rc(method(
            &mut dom,
            &doc,
            "Document",
            "createTextNode",
            &[Arg::Str(text)],
        )
        .unwrap());
        for node_arg in [node(&text, &mut dom), node(child, &mut dom)] {
            method(&mut dom, &html, "Node", "appendChild", &[node_arg]).unwrap();
        }
    }
    let text = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createTextNode",
        &[Arg::Str("3")],
    )
    .unwrap());
    let arg = node(&text, &mut dom);
    method(&mut dom, &html, "Node", "appendChild", &[arg]).unwrap();
    let first = get(&mut dom, &html, "Element", "firstElementChild");
    let last = get(&mut dom, &html, "Element", "lastElementChild");
    dom.mutate(|cx| {
        assert!(elise::Gc::ptr_eq(first.node().get(cx.gc), a.get(cx.gc)));
        assert!(elise::Gc::ptr_eq(last.node().get(cx.gc), b.get(cx.gc)));
    });
    assert!(matches!(
        get(&mut dom, &html, "Element", "childElementCount"),
        Ret::U32(2)
    ));
    assert!(matches!(
        get(&mut dom, &a, "Element", "firstElementChild"),
        Ret::Null
    ));
    assert!(matches!(
        get(&mut dom, &a, "Element", "lastElementChild"),
        Ret::Null
    ));
}

/// A `[NewObject]` member's wrapper is made ahead, of its interface, once
/// its arguments are taken (none if they threw), and its result given as
/// the new node's; another member's result isn't, and asks nothing ahead.
#[test]
fn a_new_objects_wrapper_is_made_ahead() {
    let index = |name: &str| {
        dom_api::INTERFACES
            .iter()
            .position(|i| i.name == name)
            .unwrap()
    };
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let (made, hooks) = crate::call_with_hooks(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        MemberKind::Method,
        &[Arg::Str("p")],
    );
    assert!(made.is_ok());
    // An element's wrapper made ahead is the interface most are of: an HTML
    // element's.
    assert_eq!(
        hooks,
        crate::Hooks {
            prepared: vec![index("HTMLElement")],
            new_nodes: 1,
            finished: true,
            string_refs: None,
            dict_reads: Vec::new(),
        }
    );
    let (_, hooks) = crate::call_with_hooks(
        &mut dom,
        &doc,
        "Document",
        "createTextNode",
        MemberKind::Method,
        &[Arg::Str("t")],
    );
    assert_eq!(hooks.prepared, vec![index("Text")]);
    let (_, hooks) = crate::call_with_hooks(
        &mut dom,
        &doc,
        "Document",
        "createElementNS",
        MemberKind::Method,
        &[Arg::Null, Arg::Str("q")],
    );
    // A namespaced one is no likelier HTML's: `Element`'s.
    assert_eq!(hooks.prepared, vec![index("Element")]);
    // Thrown in the operation: made ahead, but no result, not finished.
    let (thrown, hooks) = crate::call_with_hooks(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        MemberKind::Method,
        &[Arg::Str("1")],
    );
    assert!(thrown.is_err());
    assert_eq!(
        (hooks.prepared.len(), hooks.new_nodes, hooks.finished),
        (1, 0, false)
    );
    // Thrown as the arguments are taken: nothing made ahead.
    let (thrown, hooks) = crate::call_with_hooks(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        MemberKind::Method,
        &[],
    );
    assert!(thrown.is_err());
    assert_eq!(hooks, crate::Hooks::default());
    // Not a new object.
    let (_, hooks) = crate::call_with_hooks(
        &mut dom,
        &doc,
        "Node",
        "firstChild",
        MemberKind::Getter,
        &[],
    );
    assert_eq!(
        (hooks.prepared.len(), hooks.new_nodes, hooks.finished),
        (0, 0, true)
    );
}

/// The title the frame tells the browser: the first HTML `title` element's
/// text children, ASCII whitespace stripped and collapsed (not its other
/// descendants', nor another namespace's `title`); empty without one.
#[test]
fn the_documents_title() {
    let mut dom = TestDom::new();
    let title = |dom: &TestDom, document: &std::rc::Rc<dom::NodeRoot>| {
        dom.read(|cx| dom_api::document_title(&dom_api::ApiCx::new(cx), document.get(cx.gc)))
    };
    let (document, first) = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, dom::QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let svg = cx.create_element(
            document,
            dom::QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "title",
            },
        );
        cx.append_child(html, svg).unwrap();
        let text = cx.create_text(document, "svg");
        cx.append_child(svg, text).unwrap();
        (
            std::rc::Rc::new(cx.root(document)),
            std::rc::Rc::new(cx.root(html)),
        )
    });
    assert_eq!(title(&dom, &document), "", "no HTML title");
    dom.mutate(|cx| {
        let document = document.get(cx.gc);
        let head = cx.create_element(document, dom::QualName::html("head"));
        cx.append_child(first.get(cx.gc), head).unwrap();
        for words in ["\t one \n", "two  "] {
            let element = cx.create_element(document, dom::QualName::html("title"));
            cx.append_child(head, element).unwrap();
            let text = cx.create_text(document, words);
            cx.append_child(element, text).unwrap();
            let b = cx.create_element(document, dom::QualName::html("b"));
            cx.append_child(element, b).unwrap();
            let inner = cx.create_text(document, "bold");
            cx.append_child(b, inner).unwrap();
            let more = cx.create_text(document, " more");
            cx.append_child(element, more).unwrap();
        }
    });
    assert_eq!(
        title(&dom, &document),
        "one more",
        "the first one's text children"
    );
}
