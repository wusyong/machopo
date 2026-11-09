//! CSSOM View's members that scroll, through a frame's layout of the
//! tests' (`Fake`, which logs what it is asked): the frame laid out first
//! (but `Window`'s to (0, 0)), the offset from the current one (a missing
//! axis kept, by, a non-finite number 0), snapped to device pixels; an
//! element that is the scrolling element scrolls the viewport, one that
//! isn't a scroll container nothing; `scrollIntoView`'s parameters.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRef, ObjectRoot, QualName};
use dom_api::MemberKind;
use gfx::PointF;

use crate::geometry::{block, fake, id, page};
use crate::{Arg, Ret, TestDom, This, call_on};

fn window(dom: &mut TestDom, nodes: &[Rc<NodeRoot>]) -> Rc<ObjectRoot> {
    let document = nodes[0].clone();
    Rc::new(dom.mutate(|cx| {
        let window = cx.create_window(document.get(cx.gc));
        cx.root_object(ObjectRef::Window(window))
    }))
}

fn on(
    dom: &mut TestDom,
    this: This,
    interface: &str,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Result<Ret, dom_api::ApiError> {
    call_on(dom, this, interface, name, kind, args).0
}

/// A scroll container `div` of the page, scrolled to (3, 4).
fn container(dom: &mut TestDom, nodes: &[Rc<NodeRoot>]) -> gfx::NodeId {
    let div = id(dom, &nodes[3]);
    fake(dom, |fake| {
        let mut b = block(0.0, 0.0, 50.0, 50.0, [0.0; 4]);
        b.block.as_mut().unwrap().scroll = Some(PointF::new(3.0, 4.0));
        fake.boxes.insert(div, b);
    });
    div
}

#[test]
fn an_element_scrolls_from_where_it_is() {
    let (mut dom, nodes, log) = page(false, &["div", "span"]);
    let div = container(&mut dom, &nodes);
    let this = || This::Node(nodes[3].clone());
    on(
        &mut dom,
        this(),
        "Element",
        "scrollTop",
        MemberKind::Setter,
        &[Arg::Number(20.0)],
    )
    .unwrap();
    on(
        &mut dom,
        this(),
        "Element",
        "scrollLeft",
        MemberKind::Setter,
        &[Arg::Number(f64::NAN)],
    )
    .unwrap();
    on(
        &mut dom,
        this(),
        "Element",
        "scrollTo",
        MemberKind::Method,
        &[Arg::Number(7.0), Arg::Number(8.0)],
    )
    .unwrap();
    on(
        &mut dom,
        this(),
        "Element",
        "scrollBy",
        MemberKind::Method,
        &[Arg::Number(1.0), Arg::Number(f64::INFINITY)],
    )
    .unwrap();
    let d = div.get();
    assert_eq!(
        *log.borrow(),
        [
            "layout".to_owned(),
            format!("scroll {d} 3 20"),
            "layout".to_owned(),
            format!("scroll {d} 0 4"),
            "layout".to_owned(),
            format!("scroll {d} 7 8"),
            "layout".to_owned(),
            format!("scroll {d} 4 4"),
        ],
        "the axis not given stays, a NaN is 0, by adds"
    );
    // Not a scroll container (the span has no box): nothing.
    log.borrow_mut().clear();
    on(
        &mut dom,
        This::Node(nodes[4].clone()),
        "Element",
        "scrollTop",
        MemberKind::Setter,
        &[Arg::Number(5.0)],
    )
    .unwrap();
    assert_eq!(*log.borrow(), ["layout"]);
}

/// Offsets snap to device pixels; the scrolling element and `Window`
/// scroll the viewport; `Window`'s to (0, 0) doesn't lay out.
#[test]
fn the_viewport_scrolls() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let window = window(&mut dom, &nodes);
    fake(&dom, |fake| {
        fake.viewport.device_scale_factor = 2.0;
        fake.viewport.scroll = PointF::new(10.0, 10.0);
    });
    let root = This::Node(nodes[1].clone());
    on(
        &mut dom,
        root,
        "Element",
        "scrollTop",
        MemberKind::Setter,
        &[Arg::Number(20.3)],
    )
    .unwrap();
    let w = || This::Object(window.clone());
    on(
        &mut dom,
        w(),
        "Window",
        "scrollBy",
        MemberKind::Method,
        &[Arg::Number(0.1), Arg::Number(-2.0)],
    )
    .unwrap();
    on(
        &mut dom,
        w(),
        "Window",
        "scrollTo",
        MemberKind::Method,
        &[Arg::Number(0.0), Arg::Number(0.0)],
    )
    .unwrap();
    on(
        &mut dom,
        w(),
        "Window",
        "scroll",
        MemberKind::Method,
        &[Arg::Number(0.0), Arg::Number(1.0)],
    )
    .unwrap();
    assert_eq!(
        *log.borrow(),
        [
            "layout",
            "scroll viewport 10 20.5",
            "layout",
            "scroll viewport 10 8",
            "scroll viewport 0 0",
            "layout",
            "scroll viewport 0 1",
        ]
    );
}

/// No options: no axis, where it is (the dictionary forms are
/// `dom_js`'s `scrolling.rs`').
#[test]
fn no_options_scroll_to_where_it_is() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let div = container(&mut dom, &nodes);
    on(
        &mut dom,
        This::Node(nodes[3].clone()),
        "Element",
        "scrollTo",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    assert_eq!(
        *log.borrow(),
        ["layout".to_owned(), format!("scroll {} 3 4", div.get())]
    );
}

/// `scrollIntoView`'s argument: none or `true` the block's start, `false`
/// its end, the inline axis the nearest; the layout asked once laid out.
#[test]
fn scroll_into_view_arguments() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let div = id(&dom, &nodes[3]).get();
    let this = || This::Node(nodes[3].clone());
    on(
        &mut dom,
        this(),
        "Element",
        "scrollIntoView",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    on(
        &mut dom,
        this(),
        "Element",
        "scrollIntoView",
        MemberKind::Method,
        &[Arg::Bool(false)],
    )
    .unwrap();
    assert_eq!(
        *log.borrow(),
        [
            "layout".to_owned(),
            format!("into view {div} Start Nearest false"),
            "layout".to_owned(),
            format!("into view {div} End Nearest false"),
        ]
    );
    // Not in a document: nothing.
    log.borrow_mut().clear();
    let lone = Rc::new(dom.mutate(|cx| {
        let document = nodes[0].get(cx.gc);
        cx.root(cx.create_element(document, QualName::html("p")))
    }));
    on(
        &mut dom,
        This::Node(lone),
        "Element",
        "scrollIntoView",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    assert!(log.borrow().is_empty());
}
