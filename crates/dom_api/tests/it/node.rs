//! `Node`'s members through their typed paths: what they read, the tree's
//! changes and their errors, and the arguments' conversions.

use dom_api::ApiError;

use crate::{Arg, Ret, TestDom, document, exception, get, method, node, rc, set};

#[test]
fn a_nodes_type_name_links_and_connection() {
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
    let text = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createTextNode",
        &[Arg::Str("t")],
    )
    .unwrap());
    assert!(matches!(
        get(&mut dom, &doc, "Node", "nodeType"),
        Ret::U32(9)
    ));
    assert!(matches!(
        get(&mut dom, &html, "Node", "nodeType"),
        Ret::U32(1)
    ));
    assert!(matches!(
        get(&mut dom, &text, "Node", "nodeType"),
        Ret::U32(3)
    ));
    assert_eq!(get(&mut dom, &html, "Node", "nodeName").string(), "HTML");
    assert_eq!(
        get(&mut dom, &doc, "Node", "nodeName").string(),
        "#document"
    );
    assert!(matches!(
        get(&mut dom, &doc, "Node", "ownerDocument"),
        Ret::Null
    ));
    assert!(matches!(
        get(&mut dom, &html, "Node", "isConnected"),
        Ret::Bool(false)
    ));
    assert!(matches!(
        get(&mut dom, &doc, "Node", "isConnected"),
        Ret::Bool(true)
    ));
    assert!(matches!(
        method(&mut dom, &doc, "Node", "hasChildNodes", &[]),
        Ok(Ret::Bool(false))
    ));
    let arg = node(&html, &mut dom);
    method(&mut dom, &doc, "Node", "appendChild", &[arg]).unwrap();
    let arg = node(&text, &mut dom);
    method(&mut dom, &html, "Node", "appendChild", &[arg]).unwrap();
    assert!(matches!(
        get(&mut dom, &text, "Node", "isConnected"),
        Ret::Bool(true)
    ));
    assert!(matches!(
        method(&mut dom, &doc, "Node", "hasChildNodes", &[]),
        Ok(Ret::Bool(true))
    ));
    assert!(matches!(
        method(&mut dom, &text, "Node", "hasChildNodes", &[]),
        Ok(Ret::Bool(false))
    ));
    // The links, and the parent element (none above the root element).
    let same = |dom: &mut TestDom, a: &Ret, b: &dom::NodeRoot| {
        dom.mutate(|cx| elise::Gc::ptr_eq(a.node().get(cx.gc), b.get(cx.gc)))
    };
    let owner = get(&mut dom, &text, "Node", "ownerDocument");
    assert!(same(&mut dom, &owner, &doc));
    let parent = get(&mut dom, &text, "Node", "parentNode");
    assert!(same(&mut dom, &parent, &html));
    let parent = get(&mut dom, &text, "Node", "parentElement");
    assert!(same(&mut dom, &parent, &html));
    assert!(matches!(
        get(&mut dom, &html, "Node", "parentElement"),
        Ret::Null
    ));
    let first = get(&mut dom, &doc, "Node", "firstChild");
    assert!(same(&mut dom, &first, &html));
    let last = get(&mut dom, &html, "Node", "lastChild");
    assert!(same(&mut dom, &last, &text));
    assert!(matches!(
        get(&mut dom, &text, "Node", "previousSibling"),
        Ret::Null
    ));
    assert!(matches!(
        get(&mut dom, &text, "Node", "nextSibling"),
        Ret::Null
    ));
}

trait CloneBool {
    fn clone_bool(&self) -> Option<bool>;
}

impl CloneBool for Ret {
    fn clone_bool(&self) -> Option<bool> {
        match self {
            Ret::Bool(value) => Some(*value),
            _ => None,
        }
    }
}

#[test]
fn siblings_and_contains() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let parent = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("div")],
    )
    .unwrap());
    let [a, b] = ["a", "b"].map(|name| {
        rc(method(
            &mut dom,
            &doc,
            "Document",
            "createElement",
            &[Arg::Str(name)],
        )
        .unwrap())
    });
    for child in [&a, &b] {
        let arg = node(child, &mut dom);
        method(&mut dom, &parent, "Node", "appendChild", &[arg]).unwrap();
    }
    let next = get(&mut dom, &a, "Node", "nextSibling");
    dom.mutate(|cx| assert!(elise::Gc::ptr_eq(next.node().get(cx.gc), b.get(cx.gc))));
    let previous = get(&mut dom, &b, "Node", "previousSibling");
    dom.mutate(|cx| assert!(elise::Gc::ptr_eq(previous.node().get(cx.gc), a.get(cx.gc))));
    let contains = |dom: &mut TestDom, this, other: Arg| {
        method(dom, this, "Node", "contains", &[other])
            .unwrap()
            .clone_bool()
            .unwrap()
    };
    let arg = node(&a, &mut dom);
    assert!(contains(&mut dom, &parent, arg));
    let arg = node(&parent, &mut dom);
    assert!(contains(&mut dom, &parent, arg), "inclusive");
    let arg = node(&parent, &mut dom);
    assert!(!contains(&mut dom, &a, arg));
    assert!(!contains(&mut dom, &parent, Arg::Null));
}

#[test]
fn text_content_reads_and_replaces_and_null_is_empty() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let div = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("div")],
    )
    .unwrap());
    assert_eq!(get(&mut dom, &div, "Node", "textContent").string(), "");
    set(&mut dom, &div, "Node", "textContent", Arg::Str("hello")).unwrap();
    assert_eq!(get(&mut dom, &div, "Node", "textContent").string(), "hello");
    set(&mut dom, &div, "Node", "textContent", Arg::Null).unwrap();
    assert!(matches!(
        get(&mut dom, &div, "Node", "firstChild"),
        Ret::Null
    ));
    assert!(matches!(
        get(&mut dom, &doc, "Node", "textContent"),
        Ret::Null
    ));
}

#[test]
fn the_trees_changes_return_the_node_and_throw_the_doms_errors() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let parent = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("div")],
    )
    .unwrap());
    let child = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("p")],
    )
    .unwrap());
    let other = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("i")],
    )
    .unwrap());
    let text = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createTextNode",
        &[Arg::Str("t")],
    )
    .unwrap());
    let arg = node(&child, &mut dom);
    let appended = method(&mut dom, &parent, "Node", "appendChild", &[arg]).unwrap();
    dom.mutate(|cx| {
        assert!(elise::Gc::ptr_eq(
            appended.node().get(cx.gc),
            child.get(cx.gc)
        ))
    });
    let (arg, before) = (node(&other, &mut dom), node(&child, &mut dom));
    method(&mut dom, &parent, "Node", "insertBefore", &[arg, before]).unwrap();
    let first = get(&mut dom, &parent, "Node", "firstChild");
    dom.mutate(|cx| assert!(elise::Gc::ptr_eq(first.node().get(cx.gc), other.get(cx.gc))));
    // A null reference child: last.
    let arg = node(&other, &mut dom);
    method(&mut dom, &parent, "Node", "insertBefore", &[arg, Arg::Null]).unwrap();
    let last = get(&mut dom, &parent, "Node", "lastChild");
    dom.mutate(|cx| assert!(elise::Gc::ptr_eq(last.node().get(cx.gc), other.get(cx.gc))));
    // The DOM's errors.
    let arg = node(&parent, &mut dom);
    let error = method(&mut dom, &text, "Node", "appendChild", &[arg]).unwrap_err();
    assert_eq!(exception(error), "HierarchyRequestError");
    let (arg, stranger) = (node(&child, &mut dom), node(&text, &mut dom));
    let error = method(&mut dom, &parent, "Node", "insertBefore", &[arg, stranger]).unwrap_err();
    assert_eq!(exception(error), "NotFoundError");
    let arg = node(&text, &mut dom);
    let error = method(&mut dom, &parent, "Node", "removeChild", &[arg]).unwrap_err();
    assert_eq!(exception(error), "NotFoundError");
    let arg = node(&child, &mut dom);
    let removed = method(&mut dom, &parent, "Node", "removeChild", &[arg]).unwrap();
    dom.mutate(|cx| {
        assert!(elise::Gc::ptr_eq(
            removed.node().get(cx.gc),
            child.get(cx.gc)
        ));
        assert!(cx.parent(child.get(cx.gc)).is_none());
    });
}

/// The arguments' conversions: too few, a non-node for a node, null for a
/// node that can't be null; each a `TypeError`, with nothing done.
#[test]
fn bad_arguments_are_type_errors() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let parent = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("div")],
    )
    .unwrap());
    let type_error = |result: Result<Ret, ApiError>| matches!(result, Err(ApiError::Type(_)));
    assert!(type_error(method(
        &mut dom,
        &parent,
        "Node",
        "appendChild",
        &[]
    )));
    let arg = node(&parent, &mut dom);
    assert!(type_error(method(
        &mut dom,
        &parent,
        "Node",
        "insertBefore",
        &[arg]
    )));
    assert!(type_error(method(
        &mut dom,
        &parent,
        "Node",
        "appendChild",
        &[Arg::Str("x")]
    )));
    assert!(type_error(method(
        &mut dom,
        &parent,
        "Node",
        "appendChild",
        &[Arg::Null]
    )));
    assert!(type_error(method(
        &mut dom,
        &parent,
        "Node",
        "removeChild",
        &[Arg::Undefined]
    )));
    let error = method(&mut dom, &doc, "Document", "createElement", &[]).unwrap_err();
    let ApiError::Type(message) = error else {
        panic!()
    };
    assert!(message.contains("createElement"), "{message}");
    assert!(matches!(
        get(&mut dom, &parent, "Node", "firstChild"),
        Ret::Null
    ));
}

/// A getter's call has no safepoint, so no GC's work, its nursery full or
/// not; a method's has: the next operation's scavenges.
#[test]
fn a_getter_has_no_safepoint_and_a_method_has() {
    let mut dom = TestDom::new();
    let doc = document(&mut dom, true);
    let div = rc(method(
        &mut dom,
        &doc,
        "Document",
        "createElement",
        &[Arg::Str("div")],
    )
    .unwrap());
    // Young elements nothing holds, until a safepoint scavenges them.
    while dom.stats().scavenges == 0 {
        dom.mutate(|cx| {
            let doc = doc.get(cx.gc);
            for _ in 0..1000 {
                cx.create_element(doc, dom::QualName::html("i"));
            }
        });
    }
    let before = dom.stats().scavenges;
    // Fill it again, then a getter: no scavenge.
    dom.read(|cx| {
        let doc = doc.get(cx.gc);
        for _ in 0..100_000 {
            cx.create_element(doc, dom::QualName::html("i"));
        }
    });
    get(&mut dom, &div, "Node", "nodeName");
    assert_eq!(dom.stats().scavenges, before, "a getter has no safepoint");
    method(&mut dom, &div, "Node", "hasChildNodes", &[]).unwrap();
    assert!(dom.stats().scavenges > before, "a method has one");
}
