//! A wheel on a document through `EventHandler::handle_wheel`, the
//! frame's hit test the tests' layout (`geometry::Fake`'s point): its
//! `wheel` event (its target, data and interface, canceling it), none
//! without a wheel listener, DOM's default passive listeners, and the
//! legacy `mousewheel` listeners a trusted `wheel` gets where there are no
//! `wheel` ones.

use std::rc::Rc;

use dom::NodeRoot;
use dom_api::{EventHandler, MemberKind, WheelInput};
use gfx::{NodeId, PointF};

use crate::geometry::{append, fake, id, page};
use crate::{Action, Arg, TestDom, This, call_on, with_host};

/// A page of `<section><p></p></section>` in the body: the document, the
/// root, the body, the section, the paragraph.
fn wheel_page() -> (TestDom, Vec<Rc<NodeRoot>>) {
    let (mut dom, mut nodes, _) = page(false, &["section"]);
    let p = append(&mut dom, &nodes[3], "p");
    nodes.push(p);
    (dom, nodes)
}

fn listen(
    dom: &mut TestDom,
    node: &Rc<NodeRoot>,
    event_type: &'static str,
    callback: u64,
    options: Arg,
) {
    let (result, _) = call_on(
        dom,
        This::Node(node.clone()),
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[Arg::Str(event_type), Arg::Callback(callback), options],
    );
    result.unwrap();
}

/// A wheel over `at` at (10, 20), scrolling down 40 px, Ctrl held and the
/// primary button down: whether nothing canceled it.
fn wheel(dom: &mut TestDom, document: &Rc<NodeRoot>, at: Option<NodeId>, cancelable: bool) -> bool {
    fake(dom, |fake| fake.at = at);
    let input = WheelInput {
        point: PointF::new(10.0, 20.0),
        delta: PointF::new(0.0, 40.0),
        buttons: 1,
        modifiers: dom::modifiers::CTRL,
        cancelable,
    };
    with_host(dom, |host| {
        EventHandler::default().handle_wheel(host, document, &input)
    })
}

#[test]
fn a_wheel_goes_to_what_is_under_it() {
    let (mut dom, nodes) = wheel_page();
    let [document, _, _, section, p] = nodes.as_slice() else {
        unreachable!()
    };
    let p_id = id(&dom, p);
    listen(&mut dom, section, "wheel", 1, Arg::Bool(false));
    assert!(wheel(&mut dom, document, Some(p_id), true));
    let log = dom.1.log.borrow_mut().drain(..).collect::<Vec<_>>();
    let [call] = log.as_slice() else {
        panic!("one call: {log:?}")
    };
    assert_eq!(
        (
            call.event_type.as_str(),
            call.event_target.as_str(),
            call.target.as_str()
        ),
        ("wheel", "P", "SECTION")
    );
    assert!(call.trusted && call.cancelable && !call.passive);
    // Its wrapper is a `WheelEvent`'s, made from its data.
    assert_eq!(call.interface, "WheelEvent");
    let wheel_info = call.wheel.expect("a wheel event's deltas");
    assert_eq!(
        (
            wheel_info.delta_x,
            wheel_info.delta_y,
            wheel_info.delta_z,
            wheel_info.delta_mode
        ),
        (0.0, 40.0, 0.0, 0)
    );
    let mouse = call.mouse.expect("a mouse event's data");
    assert_eq!(
        (mouse.client_x, mouse.client_y, mouse.buttons, mouse.button),
        (10.0, 20.0, 1, 0)
    );
    assert_eq!(mouse.modifiers, dom::modifiers::CTRL);
    // Canceled: the host doesn't scroll.
    dom.1.actions.borrow_mut().insert(1, Action::Cancel);
    assert!(!wheel(&mut dom, document, Some(p_id), true));
    // One the compositor didn't wait for can't be canceled.
    dom.1.actions.borrow_mut().clear();
    dom.1.log.borrow_mut().clear();
    wheel(&mut dom, document, Some(p_id), false);
    assert!(!dom.1.log.borrow()[0].cancelable);
}

/// With no `wheel` or `mousewheel` listener in the DOM, nothing is
/// dispatched: one read of the DOM, no hit test.
#[test]
fn a_dom_with_no_wheel_listener_dispatches_none() {
    let (mut dom, nodes) = wheel_page();
    let [document, _, _, section, p] = nodes.as_slice() else {
        unreachable!()
    };
    let p_id = id(&dom, p);
    listen(&mut dom, section, "click", 1, Arg::Bool(false));
    let before = dom.1.operations.get();
    assert!(wheel(&mut dom, document, Some(p_id), true));
    assert_eq!(dom.1.operations.get() - before, 1);
    listen(&mut dom, section, "mousewheel", 2, Arg::Bool(false));
    let before = dom.1.operations.get();
    assert!(wheel(&mut dom, document, Some(p_id), true));
    assert!(dom.1.operations.get() - before > 1);
}

/// DOM's default passive value: a `wheel` listener on the document, its
/// root or its body is passive unless it says otherwise; elsewhere it
/// isn't.
#[test]
fn wheel_listeners_at_the_top_are_passive() {
    let (mut dom, nodes) = wheel_page();
    let [document, html, body, section, p] = nodes.as_slice() else {
        unreachable!()
    };
    let p_id = id(&dom, p);
    for (callback, node) in [(1, document), (2, html), (3, body), (4, section)] {
        listen(&mut dom, node, "wheel", callback, Arg::Bool(false));
    }
    listen(
        &mut dom,
        body,
        "wheel",
        5,
        Arg::Dict(vec![("passive", Some(false))]),
    );
    listen(
        &mut dom,
        section,
        "wheel",
        6,
        Arg::Dict(vec![("passive", Some(true))]),
    );
    wheel(&mut dom, document, Some(p_id), true);
    let passive: Vec<(u64, bool)> = dom
        .1
        .log
        .borrow()
        .iter()
        .map(|call| (call.callback, call.passive))
        .collect();
    assert_eq!(
        passive,
        [
            (4, false),
            (6, true),
            (3, true),
            (5, false),
            (2, true),
            (1, true)
        ]
    );
}

/// DOM's legacy event types: a trusted `wheel` calls a target's
/// `mousewheel` listeners, as that type, where it has no `wheel` ones; an
/// untrusted one doesn't.
#[test]
fn a_wheel_calls_mousewheel_listeners_where_there_are_no_wheel_ones() {
    let (mut dom, nodes) = wheel_page();
    let [document, _, body, section, p] = nodes.as_slice() else {
        unreachable!()
    };
    let p_id = id(&dom, p);
    listen(&mut dom, section, "mousewheel", 1, Arg::Bool(false));
    // The body has both: its `wheel` listener alone.
    listen(&mut dom, body, "mousewheel", 2, Arg::Bool(false));
    listen(&mut dom, body, "wheel", 3, Arg::Bool(false));
    listen(&mut dom, document, "wheel", 4, Arg::Bool(false));
    wheel(&mut dom, document, Some(p_id), true);
    let calls: Vec<(u64, String)> = dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .map(|call| (call.callback, call.event_type))
        .collect();
    assert_eq!(
        calls,
        [
            (1, "mousewheel".to_owned()),
            (3, "wheel".to_owned()),
            (4, "wheel".to_owned())
        ]
    );
    // Script's `wheel` event: no legacy type.
    let event = crate::dispatch::event(&mut dom, "wheel", true, false);
    let (result, log) = crate::dispatch::dispatch(&mut dom, &This::Node(p.clone()), &event);
    assert_eq!(result, Ok(true));
    let callbacks: Vec<u64> = log.iter().map(|call| call.callback).collect();
    assert_eq!(callbacks, [3, 4]);
}
