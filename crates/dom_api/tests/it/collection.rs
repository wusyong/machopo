//! `getElementsByTagName` and `HTMLCollection` through their members; an
//! HTML element's interface by its local name, and `HTMLMetaElement`'s
//! reflected attributes.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRef, ObjectRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call_on, document, get, method, rc, set};

fn object(ret: Ret) -> Rc<ObjectRoot> {
    match ret {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

fn collection_call(
    dom: &mut TestDom,
    collection: &Rc<ObjectRoot>,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Ret {
    call_on(
        dom,
        This::Object(collection.clone()),
        "HTMLCollection",
        name,
        kind,
        args,
    )
    .0
    .unwrap()
}

/// `<html><meta name=a><body><meta id=b></body></html>`, its collection of
/// metas from the document and from the body.
#[test]
fn get_elements_by_tag_name_gives_a_live_collection() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let html = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("html")],
    )
    .unwrap());
    let body = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("body")],
    )
    .unwrap());
    let metas: Vec<Rc<NodeRoot>> = (0..2)
        .map(|_| {
            rc(method(
                &mut dom,
                &doc,
                "Document",
                "createElement",
                &[Arg::Str("meta")],
            )
            .unwrap())
        })
        .collect();
    let node = |dom: &mut TestDom, root: &Rc<NodeRoot>| crate::node(root, dom);
    let args = [node(&mut dom, &html)];
    method(&mut dom, &doc, "Node", "appendChild", &args).unwrap();
    for (parent, child) in [(&html, &metas[0]), (&html, &body), (&body, &metas[1])] {
        let args = [node(&mut dom, child)];
        method(&mut dom, parent, "Node", "appendChild", &args).unwrap();
    }
    set(
        &mut dom,
        &metas[0],
        "HTMLMetaElement",
        "name",
        Arg::Str("a"),
    )
    .unwrap();
    set(&mut dom, &metas[1], "Element", "id", Arg::Str("b")).unwrap();

    let all = object(
        method(
            &mut dom,
            &doc,
            "Document",
            "getElementsByTagName",
            &[Arg::Str("META")],
        )
        .unwrap(),
    );
    let again = object(
        method(
            &mut dom,
            &doc,
            "Document",
            "getElementsByTagName",
            &[Arg::Str("meta")],
        )
        .unwrap(),
    );
    assert!(
        dom.read(|cx| !matches!((all.get(cx.gc), again.get(cx.gc)),
            (ObjectRef::HtmlCollection(a), ObjectRef::HtmlCollection(b)) if elise::Gc::ptr_eq(a, b))),
        "a new collection each call"
    );
    assert!(matches!(
        collection_call(&mut dom, &all, "length", MemberKind::Getter, &[]),
        Ret::U32(2)
    ));
    let in_body = object(
        method(
            &mut dom,
            &body,
            "Element",
            "getElementsByTagName",
            &[Arg::Str("meta")],
        )
        .unwrap(),
    );
    assert!(matches!(
        collection_call(&mut dom, &in_body, "length", MemberKind::Getter, &[]),
        Ret::U32(1)
    ));

    let is = |dom: &mut TestDom, ret: Ret, expected: Option<&Rc<NodeRoot>>| match (ret, expected) {
        (Ret::Node(node), Some(expected)) => {
            dom.read(|cx| elise::Gc::ptr_eq(node.get(cx.gc), expected.get(cx.gc)))
        }
        (Ret::Null, None) => true,
        _ => false,
    };
    let first = collection_call(
        &mut dom,
        &all,
        "item",
        MemberKind::Method,
        &[Arg::Number(0.0)],
    );
    assert!(is(&mut dom, first, Some(&metas[0])));
    let second = collection_call(
        &mut dom,
        &all,
        "item",
        MemberKind::Method,
        &[Arg::Number(1.0)],
    );
    assert!(is(&mut dom, second, Some(&metas[1])));
    let past = collection_call(
        &mut dom,
        &all,
        "item",
        MemberKind::Method,
        &[Arg::Number(2.0)],
    );
    assert!(is(&mut dom, past, None));
    // `unsigned long`: -1 is 2^32 - 1.
    let wrapped = collection_call(
        &mut dom,
        &all,
        "item",
        MemberKind::Method,
        &[Arg::Number(-1.0)],
    );
    assert!(is(&mut dom, wrapped, None));
    let named = collection_call(
        &mut dom,
        &all,
        "namedItem",
        MemberKind::Method,
        &[Arg::Str("b")],
    );
    assert!(is(&mut dom, named, Some(&metas[1])));
    let by_name = collection_call(
        &mut dom,
        &all,
        "namedItem",
        MemberKind::Method,
        &[Arg::Str("a")],
    );
    assert!(is(&mut dom, by_name, Some(&metas[0])));

    // Live.
    let args = [node(&mut dom, &metas[0])];
    method(&mut dom, &html, "Node", "removeChild", &args).unwrap();
    assert!(matches!(
        collection_call(&mut dom, &all, "length", MemberKind::Getter, &[]),
        Ret::U32(1)
    ));
}

/// An element's wrapper's interface: an HTML element's by its local name
/// (`meta` its own, `section` none: `HTMLElement`), an SVG element's by its own
/// (`rect` its own, others `SVGElement`), another namespace's `Element`;
/// an interface's node arguments and `this` check the name for an element
/// interface.
#[test]
fn an_html_elements_interface_is_by_its_name() {
    let mut dom = TestDom::new();
    let index = |name: &str| {
        dom_api::INTERFACES
            .iter()
            .position(|interface| interface.name == name)
            .unwrap()
    };
    let (meta, div, svg, text) = (
        index("HTMLMetaElement"),
        index("HTMLElement"),
        index("SVGElement"),
        index("Text"),
    );
    let (rect, geometry, element) = (
        index("SVGRectElement"),
        index("SVGGeometryElement"),
        index("Element"),
    );
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let xml = cx.create_document(false);
        let m = cx.create_element(document, QualName::html("meta"));
        let d = cx.create_element(document, QualName::html("section"));
        let s = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "meta",
            },
        );
        let r = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "rect",
            },
        );
        let math = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/1998/Math/MathML"),
                prefix: None,
                local: "math",
            },
        );
        let x = cx.create_element(xml, QualName::html("meta"));
        let t = cx.create_text(document, "t");
        assert_eq!(dom_api::node_interface_of(cx, m), meta);
        assert_eq!(dom_api::node_interface_of(cx, d), div);
        assert_eq!(dom_api::node_interface_of(cx, s), svg);
        assert_eq!(dom_api::node_interface_of(cx, r), rect);
        assert_eq!(dom_api::node_interface_of(cx, math), element);
        assert_eq!(
            dom_api::node_interface_of(cx, x),
            meta,
            "an HTML element in an XML document too"
        );
        assert_eq!(dom_api::node_interface_of(cx, t), text);
        let api = dom_api::ApiCx::new(cx);
        assert_eq!(dom_api::interface_of(&api, ObjectRef::Node(m)), meta);
        // `accepts_node`: an element interface checks the name.
        assert!(dom_api::accepts_node(cx, meta, m));
        assert!(!dom_api::accepts_node(cx, meta, d));
        assert!(!dom_api::accepts_node(cx, meta, s));
        assert!(dom_api::accepts_node(cx, div, m), "a descendant's");
        assert!(dom_api::accepts_node(cx, div, d));
        assert!(!dom_api::accepts_node(cx, div, s));
        assert!(!dom_api::accepts_node(cx, div, t));
        assert!(dom_api::accepts_node(cx, svg, s));
        assert!(dom_api::accepts_node(cx, svg, r), "a descendant's");
        assert!(
            dom_api::accepts_node(cx, geometry, r),
            "an interface with no names"
        );
        assert!(!dom_api::accepts_node(cx, geometry, s));
        assert!(!dom_api::accepts_node(cx, rect, s));
        assert!(!dom_api::accepts_node(cx, svg, math));
        assert!(dom_api::accepts_node(cx, element, math));
        // An interface past the first 128, in the second word.
        let last = cx.create_element(
            document,
            QualName {
                namespace: Some("http://www.w3.org/2000/svg"),
                prefix: None,
                local: "animateTransform",
            },
        );
        let animate_transform = index("SVGAnimateTransformElement");
        assert!(animate_transform >= 128, "{animate_transform}");
        assert_eq!(dom_api::node_interface_of(cx, last), animate_transform);
        assert!(dom_api::accepts_node(cx, svg, last));
        assert!(dom_api::accepts_node(cx, animate_transform, last));
        assert!(!dom_api::accepts_node(cx, rect, last));
        assert!(!dom_api::accepts_node(cx, animate_transform, r));
        assert!(dom_api::accepts_node(cx, index("Node"), t));
    });
}

/// `HTMLMetaElement`'s `name` and `content` reflect their attributes.
#[test]
fn a_metas_attributes_are_reflected() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let meta = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("meta")],
    )
    .unwrap());
    assert_eq!(
        get(&mut dom, &meta, "HTMLMetaElement", "content").string(),
        ""
    );
    set(
        &mut dom,
        &meta,
        "HTMLMetaElement",
        "content",
        Arg::Str("long"),
    )
    .unwrap();
    set(
        &mut dom,
        &meta,
        "HTMLMetaElement",
        "name",
        Arg::Str("timeout"),
    )
    .unwrap();
    assert_eq!(
        get(&mut dom, &meta, "HTMLMetaElement", "content").string(),
        "long"
    );
    assert_eq!(
        method(
            &mut dom,
            &meta,
            "Element",
            "getAttribute",
            &[Arg::Str("name")]
        )
        .unwrap()
        .string(),
        "timeout"
    );
    assert_eq!(
        get(&mut dom, &meta, "HTMLMetaElement", "name").string(),
        "timeout"
    );
}
