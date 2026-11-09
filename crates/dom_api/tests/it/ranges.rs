//! A range's rects through its members, the tests' layout (`geometry::Fake`)
//! laid out: the elements it holds whole (a `display: contents` one's
//! children), the parts of its texts, the viewport's scroll for a text in
//! a fixed element; none out of a document. `createContextualFragment`'s
//! scripts aren't already started (`innerHTML`'s are).

use std::rc::Rc;

use dom::{NodeRoot, ObjectRoot, QualName};
use dom_api::MemberKind;
use gfx::RectF;

use crate::geometry::{fake, id, object, page, rect_values};
use crate::{Arg, Ret, TestDom, This, call, call_on, rc};

fn range(dom: &mut TestDom, document: &Rc<NodeRoot>) -> Rc<ObjectRoot> {
    object(
        call(
            dom,
            document,
            "Document",
            "createRange",
            MemberKind::Method,
            &[],
        )
        .unwrap(),
    )
}

fn on(dom: &mut TestDom, range: &Rc<ObjectRoot>, name: &str, args: &[Arg]) -> Ret {
    call_on(
        dom,
        This::Object(range.clone()),
        "Range",
        name,
        MemberKind::Method,
        args,
    )
    .0
    .unwrap()
}

/// The range's `getClientRects()`, each `[x, y, width, height]`.
fn rects(dom: &mut TestDom, range: &Rc<ObjectRoot>) -> Vec<[f64; 4]> {
    let list = object(on(dom, range, "getClientRects", &[]));
    let mut all = Vec::new();
    for index in 0.. {
        let item = call_on(
            dom,
            This::Object(list.clone()),
            "DOMRectList",
            "item",
            MemberKind::Method,
            &[Arg::Number(f64::from(index))],
        )
        .0
        .unwrap();
        let Ret::Object(rect) = item else {
            break;
        };
        let values = rect_values(dom, &Rc::new(rect));
        all.push([values[0], values[1], values[2], values[3]]);
    }
    all
}

/// A page: `<div>`, a text `abcd`, a `display: contents` `<span>` with a
/// text, a second `<div>` with an `<em>`, each laid out with a rect of
/// its own.
fn laid_out() -> (TestDom, Vec<Rc<NodeRoot>>, crate::geometry::Log) {
    let (mut dom, mut nodes, log) = page(false, &["div"]);
    let body = nodes[2].clone();
    let added = dom.mutate(|cx| {
        let body = body.get(cx.gc);
        let document = cx.document_of(body);
        let text = cx.create_text(document, "abcd");
        cx.append_child(body, text).unwrap();
        let span = cx.create_element(document, QualName::html("span"));
        cx.append_child(body, span).unwrap();
        let inner = cx.create_text(document, "ef");
        cx.append_child(span, inner).unwrap();
        let second = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, second).unwrap();
        let nested = cx.create_element(document, QualName::html("em"));
        cx.append_child(second, nested).unwrap();
        [text, span, inner, second, nested].map(|node| Rc::new(cx.root(node)))
    });
    nodes.extend(added);
    let ids: Vec<_> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        fake.rects
            .insert(ids[3], vec![RectF::new(0.0, 0.0, 10.0, 10.0)]);
        fake.texts.insert(ids[4], RectF::new(1.0, 1.0, 4.0, 2.0));
        fake.contents.push(ids[5]);
        fake.texts.insert(ids[6], RectF::new(2.0, 2.0, 3.0, 3.0));
        fake.rects
            .insert(ids[7], vec![RectF::new(5.0, 5.0, 6.0, 6.0)]);
        fake.rects
            .insert(ids[5], vec![RectF::new(99.0, 99.0, 1.0, 1.0)]);
        fake.rects
            .insert(ids[8], vec![RectF::new(7.0, 7.0, 1.0, 1.0)]);
    });
    (dom, nodes, log)
}

#[test]
fn a_ranges_rects_are_its_elements_and_its_texts_parts() {
    let (mut dom, nodes, log) = laid_out();
    let (document, body, text) = (&nodes[0], &nodes[2], &nodes[4]);
    let r = range(&mut dom, document);
    on(
        &mut dom,
        &r,
        "setStart",
        &[Arg::Node(text.clone()), Arg::Number(1.0)],
    );
    on(
        &mut dom,
        &r,
        "setEnd",
        &[Arg::Node(body.clone()), Arg::Number(4.0)],
    );
    log.borrow_mut().clear();
    // The text from 1; the contents element's text (its own box none);
    // that text again, which it holds; the second div whole (not its
    // `<em>`, which it holds).
    assert_eq!(
        rects(&mut dom, &r),
        [
            [1.0, 1.0, 4.0, 2.0],
            [2.0, 2.0, 3.0, 3.0],
            [2.0, 2.0, 3.0, 3.0],
            [5.0, 5.0, 6.0, 6.0]
        ]
    );
    assert_eq!(
        log.borrow().as_slice(),
        [
            "layout",
            "text 1 4294967295 false",
            "text 0 4294967295 false",
            "text 0 4294967295 false"
        ]
    );
    // Its bounding rect: their union.
    let bounding = object(on(&mut dom, &r, "getBoundingClientRect", &[]));
    assert_eq!(
        rect_values(&mut dom, &bounding)[..4],
        [1.0, 1.0, 10.0, 10.0]
    );
    // From the body's start: the first div whole, the text whole.
    on(
        &mut dom,
        &r,
        "setStart",
        &[Arg::Node(body.clone()), Arg::Number(0.0)],
    );
    on(
        &mut dom,
        &r,
        "setEnd",
        &[Arg::Node(body.clone()), Arg::Number(2.0)],
    );
    log.borrow_mut().clear();
    assert_eq!(
        rects(&mut dom, &r),
        [[0.0, 0.0, 10.0, 10.0], [1.0, 1.0, 4.0, 2.0]]
    );
    assert_eq!(
        log.borrow().as_slice(),
        ["layout", "text 0 4294967295 false"]
    );
}

#[test]
fn a_collapsed_or_fixed_or_detached_ranges_rects() {
    let (mut dom, nodes, log) = laid_out();
    let (document, body, text) = (&nodes[0], &nodes[2], &nodes[4]);
    let r = range(&mut dom, document);
    on(
        &mut dom,
        &r,
        "setStart",
        &[Arg::Node(text.clone()), Arg::Number(2.0)],
    );
    on(&mut dom, &r, "collapse", &[Arg::Bool(true)]);
    log.borrow_mut().clear();
    assert_eq!(rects(&mut dom, &r), [[1.0, 1.0, 4.0, 2.0]]);
    assert_eq!(log.borrow().as_slice(), ["layout", "text 2 2 false"]);
    // The element it is in fixed to the viewport.
    let body_id = id(&dom, body);
    fake(&dom, |fake| {
        fake.positions
            .insert(body_id, computed_style::Position::Fixed);
    });
    log.borrow_mut().clear();
    rects(&mut dom, &r);
    assert_eq!(log.borrow().as_slice(), ["layout", "text 2 2 true"]);
    // Out of the document: nothing asked.
    let lone =
        dom.mutate(|cx| Rc::new(cx.root(cx.create_text(cx.document_of(body.get(cx.gc)), "x"))));
    on(&mut dom, &r, "selectNodeContents", &[Arg::Node(lone)]);
    log.borrow_mut().clear();
    assert!(rects(&mut dom, &r).is_empty());
    assert!(log.borrow().is_empty());
    let bounding = object(on(&mut dom, &r, "getBoundingClientRect", &[]));
    assert_eq!(rect_values(&mut dom, &bounding)[..4], [0.0, 0.0, 0.0, 0.0]);
}

#[test]
fn a_contextual_fragments_scripts_arent_already_started() {
    let (mut dom, nodes, _) = page(false, &["div"]);
    let (document, div) = (&nodes[0], &nodes[3]);
    let r = range(&mut dom, document);
    on(
        &mut dom,
        &r,
        "selectNodeContents",
        &[Arg::Node(div.clone())],
    );
    let fragment = rc(on(
        &mut dom,
        &r,
        "createContextualFragment",
        &[Arg::Str("<script>1</script><p>a</p>")],
    ));
    call(
        &mut dom,
        div,
        "Element",
        "innerHTML",
        MemberKind::Setter,
        &[Arg::Str("<script>2</script>")],
    )
    .unwrap();
    let (contextual, inner) = dom.read(|cx| {
        let script = cx.first_child(fragment.get(cx.gc)).unwrap();
        let inner = cx.first_child(div.get(cx.gc)).unwrap();
        (cx.script_flags(script), cx.script_flags(inner))
    });
    assert_eq!(contextual, dom::ScriptFlags::default());
    assert!(inner.already_started && inner.parser_inserted);
}
