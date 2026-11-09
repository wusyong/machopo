//! A composed event with a related target through a shadow tree (one
//! script can't make yet: `MouseEventInit` has no `relatedTarget`): its
//! path ends at the host its retargeted related target is, and goes on
//! past a host to a related target outside the shadow tree.

use std::rc::Rc;

use dom::{
    EventFlags, MouseEventInfo, NodeRoot, QualName, ShadowRootInit, ShadowRootMode,
    SlotAssignmentMode, TargetRef, TargetRoot, names,
};
use dom_api::MemberKind;
use elise::Persistent;

use crate::{Arg, TestDom, This, call_on, with_host};

/// `<html><body><div>` with an open shadow root holding `<span>` and
/// `<i>`: the body, the host, its root, the span, the `<i>`; a listener of
/// `m` on each of the first four, numbered from 1.
fn page() -> (TestDom, [Rc<NodeRoot>; 5]) {
    let mut dom = TestDom::new();
    let nodes = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let body = cx.create_element(document, QualName::html("body"));
        cx.append_child(html, body).unwrap();
        let host = cx.create_element(document, QualName::html("div"));
        cx.append_child(body, host).unwrap();
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let span = cx.create_element(document, QualName::html("span"));
        cx.append_child(root, span).unwrap();
        let other = cx.create_element(document, QualName::html("i"));
        cx.append_child(root, other).unwrap();
        [body, host, root, span, other].map(|node| Rc::new(cx.root(node)))
    });
    for (callback, node) in nodes.iter().take(4).enumerate() {
        let (result, _) = call_on(
            &mut dom,
            This::Node(node.clone()),
            "EventTarget",
            "addEventListener",
            MemberKind::Method,
            &[
                Arg::Str("m"),
                Arg::Callback(callback as u64 + 1),
                Arg::Bool(false),
            ],
        );
        result.unwrap();
    }
    (dom, nodes)
}

/// A composed, bubbling `m` with `related` dispatched at `target`: the
/// current targets and the targets they saw.
fn dispatch(
    dom: &mut TestDom,
    target: &Rc<NodeRoot>,
    related: &Rc<NodeRoot>,
) -> Vec<(String, String)> {
    dom.1.log.borrow_mut().clear();
    let event = dom.mutate(|cx| {
        let flags = EventFlags {
            bubbles: true,
            composed: true,
            ..EventFlags::default()
        };
        let event = cx.create_event(names::intern("m"), flags, None);
        let related = TargetRef::Node(related.get(cx.gc));
        cx.set_mouse_event_data(event, None, &MouseEventInfo::default(), Some(related));
        Persistent::new(cx.gc, event)
    });
    let at = TargetRoot::Node(dom.read(|cx| cx.root(target.get(cx.gc))));
    with_host(dom, |host| dom_api::dispatch(host, &at, &event));
    dom.1
        .log
        .borrow()
        .iter()
        .map(|call| (call.target.clone(), call.event_target.clone()))
        .collect()
}

fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
    list.iter()
        .map(|&(a, b)| (a.to_owned(), b.to_owned()))
        .collect()
}

/// From one node of the shadow tree to another: the host is the related
/// target outside, so the path stops before it.
#[test]
fn a_path_ends_at_the_host_its_related_target_is() {
    let (mut dom, [_, _, _, span, other]) = page();
    assert_eq!(
        dispatch(&mut dom, &span, &other),
        pairs(&[("SPAN", "SPAN"), ("#document-fragment", "SPAN")])
    );
}

/// To a related target outside: past the host, which is the target the
/// rest of the path sees, on to the related target's ancestors too.
#[test]
fn a_path_goes_past_the_host_to_an_outside_related_target() {
    let (mut dom, [body, _, _, span, _]) = page();
    assert_eq!(
        dispatch(&mut dom, &span, &body),
        pairs(&[
            ("SPAN", "SPAN"),
            ("#document-fragment", "SPAN"),
            ("DIV", "DIV"),
            ("BODY", "DIV")
        ])
    );
}
