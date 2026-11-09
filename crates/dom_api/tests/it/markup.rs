//! `innerHTML`, `outerHTML` and `insertAdjacentHTML` through their members.

use std::rc::Rc;

use dom::{NodeRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, call, exception, get, method, set};

/// A document (HTML or XML) with `<html><body><div id=d>` in it: the
/// document, the body and the div.
fn tree(dom: &mut TestDom, html: bool) -> [Rc<NodeRoot>; 3] {
    dom.mutate(|cx| {
        let doc = cx.create_document(html);
        let root = cx.create_element(doc, QualName::html("html"));
        cx.append_child(doc, root).unwrap();
        let body = cx.create_element(doc, QualName::html("body"));
        cx.append_child(root, body).unwrap();
        let div = cx.create_element(doc, QualName::html("div"));
        cx.set_attribute(div, "id", "d");
        cx.append_child(body, div).unwrap();
        [doc, body, div].map(|node| Rc::new(cx.root(node)))
    })
}

fn string(ret: Ret) -> String {
    ret.string().to_string()
}

fn inner(dom: &mut TestDom, node: &Rc<NodeRoot>) -> String {
    string(get(dom, node, "Element", "innerHTML"))
}

#[test]
fn inner_html_parses_and_serializes() {
    let mut dom = TestDom::new();
    let [_, body, div] = tree(&mut dom, true);
    set(
        &mut dom,
        &div,
        "Element",
        "innerHTML",
        Arg::Str("<p class=a>x &amp; <b>y</b></p><!--c--><br>t"),
    )
    .unwrap();
    assert_eq!(
        inner(&mut dom, &div),
        "<p class=\"a\">x &amp; <b>y</b></p><!--c--><br>t"
    );
    // The fragment's children replace the old ones.
    set(&mut dom, &div, "Element", "innerHTML", Arg::Str("<i>z</i>")).unwrap();
    assert_eq!(inner(&mut dom, &div), "<i>z</i>");
    // Null is the empty string.
    set(&mut dom, &div, "Element", "innerHTML", Arg::Null).unwrap();
    assert_eq!(inner(&mut dom, &div), "");
    // The context decides how it parses: a `<td>` alone needs a table.
    set(
        &mut dom,
        &div,
        "Element",
        "innerHTML",
        Arg::Str("<td>a</td>"),
    )
    .unwrap();
    assert_eq!(inner(&mut dom, &div), "a");
    assert_eq!(inner(&mut dom, &body), "<div id=\"d\">a</div>");
    // The parsed tree is the document's: ids are found.
    set(
        &mut dom,
        &div,
        "Element",
        "innerHTML",
        Arg::Str("<span id=s></span>"),
    )
    .unwrap();
    let doc = dom.read(|cx| cx.root(cx.document_of(div.get(cx.gc))));
    let found = dom.read(|cx| cx.element_by_id(doc.get(cx.gc), "s").is_some());
    assert!(found);
}

#[test]
fn a_templates_inner_html_is_its_contents() {
    let mut dom = TestDom::new();
    let [_, _, div] = tree(&mut dom, true);
    set(
        &mut dom,
        &div,
        "Element",
        "innerHTML",
        Arg::Str("<template><p>in</p></template>"),
    )
    .unwrap();
    let template = dom.read(|cx| cx.root(cx.first_child(div.get(cx.gc)).unwrap()));
    let template = Rc::new(template);
    assert_eq!(inner(&mut dom, &template), "<p>in</p>");
    set(
        &mut dom,
        &template,
        "Element",
        "innerHTML",
        Arg::Str("<b>new</b>"),
    )
    .unwrap();
    assert_eq!(inner(&mut dom, &template), "<b>new</b>");
    assert!(
        dom.read(|cx| cx.first_child(template.get(cx.gc)).is_none()),
        "into its contents, not its children"
    );
}

#[test]
fn markup_in_an_xml_document_is_not_supported() {
    let mut dom = TestDom::new();
    let [_, _, div] = tree(&mut dom, false);
    let error = call(
        &mut dom,
        &div,
        "Element",
        "innerHTML",
        MemberKind::Getter,
        &[],
    )
    .unwrap_err();
    assert_eq!(exception(error), "NotSupportedError");
    let error = set(&mut dom, &div, "Element", "outerHTML", Arg::Str("x")).unwrap_err();
    assert_eq!(exception(error), "NotSupportedError");
}

#[test]
fn outer_html_replaces_the_element() {
    let mut dom = TestDom::new();
    let [doc, body, div] = tree(&mut dom, true);
    assert_eq!(
        string(get(&mut dom, &div, "Element", "outerHTML")),
        "<div id=\"d\"></div>"
    );
    dom.mutate(|cx| {
        let span = cx.create_element(doc.get(cx.gc), QualName::html("span"));
        cx.append_child(body.get(cx.gc), span).unwrap();
    });
    set(
        &mut dom,
        &div,
        "Element",
        "outerHTML",
        Arg::Str("<p>1</p>2"),
    )
    .unwrap();
    assert_eq!(
        inner(&mut dom, &body),
        "<p>1</p>2<span></span>",
        "where the element was"
    );
    // No parent: nothing.
    assert!(dom.read(|cx| cx.parent(div.get(cx.gc)).is_none()));
    set(&mut dom, &div, "Element", "outerHTML", Arg::Str("<p>x</p>")).unwrap();
    // A document parent.
    let root = dom.read(|cx| Rc::new(cx.root(cx.first_child(doc.get(cx.gc)).unwrap())));
    let error = set(&mut dom, &root, "Element", "outerHTML", Arg::Str("<html>")).unwrap_err();
    assert_eq!(exception(error), "NoModificationAllowedError");
    // A fragment parent: a `<body>` is the context.
    let (fragment, child) = dom.mutate(|cx| {
        let fragment = cx.create_document_fragment(doc.get(cx.gc));
        let child = cx.create_element(doc.get(cx.gc), QualName::html("span"));
        cx.append_child(fragment, child).unwrap();
        (cx.root(fragment), Rc::new(cx.root(child)))
    });
    set(
        &mut dom,
        &child,
        "Element",
        "outerHTML",
        Arg::Str("<tr><td>c</td></tr><i>i</i>"),
    )
    .unwrap();
    let kids = dom.read(|cx| {
        let mut names = Vec::new();
        let mut at = cx.first_child(fragment.get(cx.gc));
        while let Some(node) = at {
            names.push(cx.node_name(node).to_string());
            at = cx.next_sibling(node);
        }
        names
    });
    assert_eq!(kids, ["#text", "I"], "a body context drops the table parts");
}

#[test]
fn insert_adjacent_html_at_each_position() {
    let mut dom = TestDom::new();
    let [doc, body, div] = tree(&mut dom, true);
    set(&mut dom, &div, "Element", "innerHTML", Arg::Str("<i>m</i>")).unwrap();
    for (position, markup) in [
        ("beforebegin", "<a>1</a>"),
        ("AfterBegin", "<b>2</b>"),
        ("beforeEnd", "<s>3</s>"),
        ("afterend", "<u>4</u>"),
    ] {
        method(
            &mut dom,
            &div,
            "Element",
            "insertAdjacentHTML",
            &[Arg::Str(position), Arg::Str(markup)],
        )
        .unwrap();
    }
    assert_eq!(
        inner(&mut dom, &body),
        "<a>1</a><div id=\"d\"><b>2</b><i>m</i><s>3</s></div><u>4</u>"
    );
    let error = method(
        &mut dom,
        &div,
        "Element",
        "insertAdjacentHTML",
        &[Arg::Str("inside"), Arg::Str("x")],
    )
    .unwrap_err();
    assert_eq!(exception(error), "SyntaxError");
    // The document's element has no parent element.
    let root = dom.read(|cx| Rc::new(cx.root(cx.first_child(doc.get(cx.gc)).unwrap())));
    let error = method(
        &mut dom,
        &root,
        "Element",
        "insertAdjacentHTML",
        &[Arg::Str("afterend"), Arg::Str("x")],
    )
    .unwrap_err();
    assert_eq!(exception(error), "NoModificationAllowedError");
    // `<html>` as the context is a `<body>`: a `<td>` is dropped there.
    method(
        &mut dom,
        &root,
        "Element",
        "insertAdjacentHTML",
        &[Arg::Str("beforeend"), Arg::Str("<td>t</td>")],
    )
    .unwrap();
    let last = dom.read(|cx| {
        cx.node_name(cx.last_child(root.get(cx.gc)).unwrap())
            .to_string()
    });
    assert_eq!(last, "#text");
}

/// What serializes otherwise than its tree: a void element's contents,
/// another namespace's element by its qualified name, the `xlink:`
/// attributes by their prefix.
#[test]
fn serializing_follows_htmls_rules() {
    let mut dom = TestDom::new();
    let [doc, _, div] = tree(&mut dom, true);
    let (br, other) = dom.mutate(|cx| {
        let doc = doc.get(cx.gc);
        let br = cx.create_element(doc, QualName::html("br"));
        let text = cx.create_text(doc, "inside");
        cx.append_child(br, text).unwrap();
        let other = cx.create_element(
            doc,
            QualName {
                namespace: Some("urn:x"),
                prefix: Some("p"),
                local: "q",
            },
        );
        cx.set_attribute_ns(
            other,
            QualName {
                namespace: Some("http://www.w3.org/1999/xlink"),
                prefix: Some("xlink"),
                local: "href",
            },
            "#h",
        );
        cx.set_attribute_ns(
            other,
            QualName {
                namespace: Some("urn:y"),
                prefix: Some("y"),
                local: "z",
            },
            "1",
        );
        (Rc::new(cx.root(br)), Rc::new(cx.root(other)))
    });
    assert_eq!(inner(&mut dom, &br), "", "a void element holds nothing");
    assert_eq!(string(get(&mut dom, &br, "Element", "outerHTML")), "<br>");
    assert_eq!(
        string(get(&mut dom, &other, "Element", "outerHTML")),
        "<p:q xlink:href=\"#h\" y:z=\"1\"></p:q>"
    );
    set(
        &mut dom,
        &div,
        "Element",
        "innerHTML",
        Arg::Str("<style>a < b</style><p>a < b</p>"),
    )
    .unwrap();
    assert_eq!(
        inner(&mut dom, &div),
        "<style>a < b</style><p>a &lt; b</p>",
        "raw text in a style"
    );
    let style = dom.read(|cx| Rc::new(cx.root(cx.first_child(div.get(cx.gc)).unwrap())));
    assert_eq!(
        inner(&mut dom, &style),
        "a < b",
        "a style's own contents too"
    );
}
