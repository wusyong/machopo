//! Finding elements through their members: `getElementById`,
//! `querySelector(All)` and its `NodeList`, `matches` and `closest`.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call_on, exception, method};

/// `<div id=a class=x><p class=x></p><p id=a></p></div>` in a document,
/// and a fragment holding `<span id=f>`: the document, the div, its two
/// paragraphs, the fragment and its span.
pub(crate) struct Tree {
    pub doc: Rc<NodeRoot>,
    pub div: Rc<NodeRoot>,
    pub p1: Rc<NodeRoot>,
    pub p2: Rc<NodeRoot>,
    pub fragment: Rc<NodeRoot>,
    pub span: Rc<NodeRoot>,
}

pub(crate) fn tree(dom: &mut TestDom) -> Tree {
    let roots = dom.mutate(|cx| {
        let doc = cx.create_document(true);
        let html = cx.create_element(doc, QualName::html("html"));
        cx.append_child(doc, html).unwrap();
        let div = cx.create_element(doc, QualName::html("div"));
        cx.set_attribute(div, "id", "a");
        cx.set_attribute(div, "class", "x");
        cx.append_child(html, div).unwrap();
        let p1 = cx.create_element(doc, QualName::html("p"));
        cx.set_attribute(p1, "class", "x");
        let p2 = cx.create_element(doc, QualName::html("p"));
        cx.set_attribute(p2, "id", "a");
        cx.append_child(div, p1).unwrap();
        cx.append_child(div, p2).unwrap();
        let fragment = cx.create_document_fragment(doc);
        let span = cx.create_element(doc, QualName::html("span"));
        cx.set_attribute(span, "id", "f");
        cx.append_child(fragment, span).unwrap();
        [doc, div, p1, p2, fragment, span].map(|node| Rc::new(cx.root(node)))
    });
    let [doc, div, p1, p2, fragment, span] = roots;
    Tree {
        doc,
        div,
        p1,
        p2,
        fragment,
        span,
    }
}

pub(crate) fn same(dom: &TestDom, ret: &Ret, expected: Option<&Rc<NodeRoot>>) -> bool {
    match (ret, expected) {
        (Ret::Node(node), Some(expected)) => {
            dom.read(|cx| elise::Gc::ptr_eq(node.get(cx.gc), expected.get(cx.gc)))
        }
        (Ret::Null, None) => true,
        _ => false,
    }
}

fn list(ret: Ret) -> Rc<ObjectRoot> {
    match ret {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

fn list_call(
    dom: &mut TestDom,
    list: &Rc<ObjectRoot>,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Ret {
    call_on(
        dom,
        This::Object(list.clone()),
        "NodeList",
        name,
        kind,
        args,
    )
    .0
    .unwrap()
}

#[test]
fn get_element_by_id_finds_the_first_in_tree_order() {
    let mut dom = TestDom::new();
    let tree = tree(&mut dom);
    let found = method(
        &mut dom,
        &tree.doc,
        "Document",
        "getElementById",
        &[Arg::Str("a")],
    )
    .unwrap();
    assert!(same(&dom, &found, Some(&tree.div)));
    let none = method(
        &mut dom,
        &tree.doc,
        "Document",
        "getElementById",
        &[Arg::Str("f")],
    )
    .unwrap();
    assert!(same(&dom, &none, None), "not in the document's tree");
    let in_fragment = method(
        &mut dom,
        &tree.fragment,
        "DocumentFragment",
        "getElementById",
        &[Arg::Str("f")],
    )
    .unwrap();
    assert!(same(&dom, &in_fragment, Some(&tree.span)));
    let empty = method(
        &mut dom,
        &tree.doc,
        "Document",
        "getElementById",
        &[Arg::Str("")],
    )
    .unwrap();
    assert!(same(&dom, &empty, None));
}

#[test]
fn query_selector_finds_descendants_in_tree_order() {
    let mut dom = TestDom::new();
    let tree = tree(&mut dom);
    let first = method(
        &mut dom,
        &tree.doc,
        "Document",
        "querySelector",
        &[Arg::Str(".x")],
    )
    .unwrap();
    assert!(same(&dom, &first, Some(&tree.div)));
    // The root itself is no match.
    let below = method(
        &mut dom,
        &tree.div,
        "Element",
        "querySelector",
        &[Arg::Str(".x")],
    )
    .unwrap();
    assert!(same(&dom, &below, Some(&tree.p1)));
    // `:scope` is the element.
    let child = method(
        &mut dom,
        &tree.div,
        "Element",
        "querySelector",
        &[Arg::Str(":scope > #a")],
    )
    .unwrap();
    assert!(same(&dom, &child, Some(&tree.p2)));
    let by_id = method(
        &mut dom,
        &tree.doc,
        "Document",
        "querySelector",
        &[Arg::Str("#a")],
    )
    .unwrap();
    assert!(
        same(&dom, &by_id, Some(&tree.div)),
        "the id's first element"
    );
    let in_fragment = method(
        &mut dom,
        &tree.fragment,
        "DocumentFragment",
        "querySelector",
        &[Arg::Str("span")],
    )
    .unwrap();
    assert!(same(&dom, &in_fragment, Some(&tree.span)));
    let none = method(
        &mut dom,
        &tree.doc,
        "Document",
        "querySelector",
        &[Arg::Str("table")],
    )
    .unwrap();
    assert!(same(&dom, &none, None));
}

#[test]
fn query_selector_all_gives_a_static_list() {
    let mut dom = TestDom::new();
    let tree = tree(&mut dom);
    let all = list(
        method(
            &mut dom,
            &tree.doc,
            "Document",
            "querySelectorAll",
            &[Arg::Str("p, div")],
        )
        .unwrap(),
    );
    assert!(matches!(
        list_call(&mut dom, &all, "length", MemberKind::Getter, &[]),
        Ret::U32(3)
    ));
    for (index, expected) in [&tree.div, &tree.p1, &tree.p2].into_iter().enumerate() {
        let item = list_call(
            &mut dom,
            &all,
            "item",
            MemberKind::Method,
            &[Arg::Number(index as f64)],
        );
        assert!(same(&dom, &item, Some(expected)), "item {index}");
    }
    let past = list_call(
        &mut dom,
        &all,
        "item",
        MemberKind::Method,
        &[Arg::Number(3.0)],
    );
    assert!(same(&dom, &past, None));
    // Static: a child removed is still in it.
    dom.mutate(|cx| cx.remove(tree.p1.get(cx.gc)));
    assert!(matches!(
        list_call(&mut dom, &all, "length", MemberKind::Getter, &[]),
        Ret::U32(3)
    ));
    let empty = list(
        method(
            &mut dom,
            &tree.doc,
            "Document",
            "querySelectorAll",
            &[Arg::Str("table")],
        )
        .unwrap(),
    );
    assert!(matches!(
        list_call(&mut dom, &empty, "length", MemberKind::Getter, &[]),
        Ret::U32(0)
    ));
}

#[test]
fn matches_and_closest_take_the_element_as_scope() {
    let mut dom = TestDom::new();
    let tree = tree(&mut dom);
    for name in ["matches", "webkitMatchesSelector"] {
        assert!(matches!(
            method(&mut dom, &tree.p1, "Element", name, &[Arg::Str("div > .x")]).unwrap(),
            Ret::Bool(true)
        ));
        assert!(matches!(
            method(&mut dom, &tree.p1, "Element", name, &[Arg::Str("#a")]).unwrap(),
            Ret::Bool(false)
        ));
    }
    assert!(matches!(
        method(
            &mut dom,
            &tree.p1,
            "Element",
            "matches",
            &[Arg::Str(":scope")]
        )
        .unwrap(),
        Ret::Bool(true)
    ));
    let itself = method(&mut dom, &tree.p1, "Element", "closest", &[Arg::Str(".x")]).unwrap();
    assert!(same(&dom, &itself, Some(&tree.p1)));
    let up = method(&mut dom, &tree.p2, "Element", "closest", &[Arg::Str(".x")]).unwrap();
    assert!(same(&dom, &up, Some(&tree.div)));
    let none = method(
        &mut dom,
        &tree.p2,
        "Element",
        "closest",
        &[Arg::Str("table")],
    )
    .unwrap();
    assert!(same(&dom, &none, None));
}

#[test]
fn an_invalid_selector_is_a_syntax_error() {
    let mut dom = TestDom::new();
    let tree = tree(&mut dom);
    for (this, interface, name) in [
        (&tree.doc, "Document", "querySelector"),
        (&tree.doc, "Document", "querySelectorAll"),
        (&tree.fragment, "DocumentFragment", "querySelector"),
        (&tree.div, "Element", "querySelectorAll"),
        (&tree.div, "Element", "matches"),
        (&tree.div, "Element", "closest"),
    ] {
        let error = method(&mut dom, this, interface, name, &[Arg::Str("p[")]).unwrap_err();
        assert_eq!(exception(error), "SyntaxError", "{interface}.{name}");
    }
}
