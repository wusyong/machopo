//! Listeners and the DOM's dispatch, through `EventTarget`'s members, with
//! the tests' callbacks (numbers whose calls are logged, `Listeners`): the
//! passes' order, what a listener does to the rest, adding and removing,
//! a trusted event the user agent fires, an event's state after.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRef, ObjectRoot, QualName, TargetRoot};
use dom_api::{ApiError, MemberKind};

use crate::{Action, Arg, Call, Ret, TestDom, This, call_on};

/// A document, its `<html>`, and a `<div>` in it.
fn tree(dom: &mut TestDom) -> [Rc<NodeRoot>; 3] {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        let div = cx.create_element(document, QualName::html("div"));
        cx.append_child(document, html).unwrap();
        cx.append_child(html, div).unwrap();
        [document, html, div].map(|node| Rc::new(cx.root(node)))
    })
}

fn listen(dom: &mut TestDom, target: &This, event_type: &'static str, callback: u64, options: Arg) {
    let (result, _) = call_on(
        dom,
        target.clone(),
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[Arg::Str(event_type), Arg::Callback(callback), options],
    );
    assert!(matches!(result, Ok(Ret::Undefined)), "{:?}", result.err());
}

fn unlisten(
    dom: &mut TestDom,
    target: &This,
    event_type: &'static str,
    callback: u64,
    options: Arg,
) {
    let (result, _) = call_on(
        dom,
        target.clone(),
        "EventTarget",
        "removeEventListener",
        MemberKind::Method,
        &[Arg::Str(event_type), Arg::Callback(callback), options],
    );
    assert!(result.is_ok());
}

pub(crate) fn event(
    dom: &mut TestDom,
    event_type: &'static str,
    bubbles: bool,
    cancelable: bool,
) -> Rc<ObjectRoot> {
    let (result, _) = call_on(
        dom,
        This::None,
        "Event",
        "constructor",
        MemberKind::Constructor,
        &[
            Arg::Str(event_type),
            Arg::Dict(vec![
                ("bubbles", Some(bubbles)),
                ("cancelable", Some(cancelable)),
            ]),
        ],
    );
    match result.unwrap() {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

/// `target.dispatchEvent(event)`'s result, and the calls it made.
pub(crate) fn dispatch(
    dom: &mut TestDom,
    target: &This,
    event: &Rc<ObjectRoot>,
) -> (Result<bool, ApiError>, Vec<Call>) {
    dom.1.log.borrow_mut().clear();
    let (result, _) = call_on(
        dom,
        target.clone(),
        "EventTarget",
        "dispatchEvent",
        MemberKind::Method,
        &[Arg::Object(event.clone())],
    );
    let result = result.map(|ret| match ret {
        Ret::Bool(value) => value,
        other => panic!("{other:?}"),
    });
    (result, dom.1.log.borrow().clone())
}

fn calls(log: &[Call]) -> Vec<(u64, u16, &str)> {
    log.iter()
        .map(|call| (call.callback, call.phase, call.target.as_str()))
        .collect()
}

fn get(dom: &mut TestDom, event: &Rc<ObjectRoot>, name: &str) -> Ret {
    call_on(
        dom,
        This::Object(event.clone()),
        "Event",
        name,
        MemberKind::Getter,
        &[],
    )
    .0
    .unwrap()
}

/// The capture pass from the root, the target's capture listeners then
/// its others (`AT_TARGET`), the bubble pass back, if the event bubbles.
#[test]
fn the_passes_go_down_and_back_up() {
    let mut dom = TestDom::new();
    let [document, html, div] = tree(&mut dom).map(This::Node);
    for (target, base) in [(&document, 10), (&html, 20), (&div, 30)] {
        // The bubble listener first: at the target, capture ones run first
        // all the same.
        listen(&mut dom, target, "x", base + 1, Arg::Bool(false));
        listen(&mut dom, target, "x", base, Arg::Bool(true));
    }
    let bubbling = event(&mut dom, "x", true, false);
    let (result, log) = dispatch(&mut dom, &div, &bubbling);
    assert_eq!(result, Ok(true));
    assert_eq!(
        calls(&log),
        [
            (10, 1, "#document"),
            (20, 1, "HTML"),
            (30, 2, "DIV"),
            (31, 2, "DIV"),
            (21, 3, "HTML"),
            (11, 3, "#document"),
        ]
    );
    let flat = event(&mut dom, "x", false, false);
    let (_, log) = dispatch(&mut dom, &div, &flat);
    assert_eq!(
        calls(&log),
        [
            (10, 1, "#document"),
            (20, 1, "HTML"),
            (30, 2, "DIV"),
            (31, 2, "DIV")
        ]
    );
    // Another type's listeners aren't called.
    let other = event(&mut dom, "y", true, false);
    assert!(dispatch(&mut dom, &div, &other).1.is_empty());
    // A dictionary's `capture` captures as `true` does.
    listen(
        &mut dom,
        &html,
        "z",
        40,
        Arg::Dict(vec![("capture", Some(true))]),
    );
    listen(&mut dom, &div, "z", 41, Arg::Undefined);
    let z = event(&mut dom, "z", false, false);
    assert_eq!(
        calls(&dispatch(&mut dom, &div, &z).1),
        [(40, 1, "HTML"), (41, 2, "DIV")]
    );
}

/// After a dispatch: no phase nor current target, an empty path, the
/// target kept; not trusted (script dispatched it).
#[test]
fn a_dispatch_leaves_its_target() {
    let mut dom = TestDom::new();
    let [_, html, div] = tree(&mut dom);
    let event = event(&mut dom, "x", true, false);
    dispatch(&mut dom, &This::Node(div.clone()), &event)
        .0
        .unwrap();
    assert!(matches!(get(&mut dom, &event, "eventPhase"), Ret::U32(0)));
    assert!(matches!(get(&mut dom, &event, "currentTarget"), Ret::Null));
    let Ret::Object(target) = get(&mut dom, &event, "target") else {
        panic!("a target");
    };
    assert!(dom.read(|cx| matches!(target.get(cx.gc), ObjectRef::Node(node) if elise::Gc::ptr_eq(node, div.get(cx.gc)))));
    let path = call_on(
        &mut dom,
        This::Object(event.clone()),
        "Event",
        "composedPath",
        MemberKind::Method,
        &[],
    )
    .0;
    assert!(matches!(path, Ok(Ret::Objects(objects)) if objects.is_empty()));
    assert!(matches!(
        get(&mut dom, &event, "isTrusted"),
        Ret::Bool(false)
    ));
    let _ = html;
}

/// Stopping propagation lets the current target's other listeners run,
/// not the next target's; stopping it immediately stops at once.
#[test]
fn stopping_propagation() {
    let mut dom = TestDom::new();
    let [document, html, div] = tree(&mut dom).map(This::Node);
    for (target, base) in [(&document, 10), (&html, 20), (&div, 30)] {
        listen(&mut dom, target, "x", base, Arg::Undefined);
        listen(&mut dom, target, "x", base + 1, Arg::Undefined);
    }
    dom.1
        .actions
        .borrow_mut()
        .insert(30, Action::StopPropagation);
    let event_ = event(&mut dom, "x", true, false);
    assert_eq!(
        calls(&dispatch(&mut dom, &div, &event_).1),
        [(30, 2, "DIV"), (31, 2, "DIV")]
    );
    dom.1
        .actions
        .borrow_mut()
        .insert(30, Action::StopImmediatePropagation);
    let event_ = event(&mut dom, "x", true, false);
    assert_eq!(
        calls(&dispatch(&mut dom, &div, &event_).1),
        [(30, 2, "DIV")]
    );
    // The flags are cleared after: the event dispatched again goes on.
    dom.1.actions.borrow_mut().clear();
    assert_eq!(dispatch(&mut dom, &div, &event_).1.len(), 6);
}

/// A callback added again (same type and capture) is one listener; with
/// another capture, another. Removing takes the one of its capture.
#[test]
fn listeners_are_added_once_and_removed_by_capture() {
    let mut dom = TestDom::new();
    let [_, _, div] = tree(&mut dom).map(This::Node);
    listen(&mut dom, &div, "x", 1, Arg::Undefined);
    listen(
        &mut dom,
        &div,
        "x",
        1,
        Arg::Dict(vec![("once", Some(true))]),
    );
    listen(&mut dom, &div, "x", 1, Arg::Bool(true));
    let event_ = event(&mut dom, "x", false, false);
    assert_eq!(
        calls(&dispatch(&mut dom, &div, &event_).1),
        [(1, 2, "DIV"), (1, 2, "DIV")]
    );
    unlisten(
        &mut dom,
        &div,
        "x",
        1,
        Arg::Dict(vec![("capture", Some(true))]),
    );
    let log = dispatch(&mut dom, &div, &event_).1;
    assert_eq!(calls(&log), [(1, 2, "DIV")]);
    unlisten(&mut dom, &div, "x", 1, Arg::Bool(true));
    assert_eq!(
        dispatch(&mut dom, &div, &event_).1.len(),
        1,
        "not of its capture"
    );
    unlisten(&mut dom, &div, "x", 1, Arg::Undefined);
    assert!(dispatch(&mut dom, &div, &event_).1.is_empty());
    // A null callback adds and removes nothing; another value throws.
    let (result, _) = call_on(
        &mut dom,
        div.clone(),
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[Arg::Str("x"), Arg::Null],
    );
    assert!(matches!(result, Ok(Ret::Undefined)));
    let (result, _) = call_on(
        &mut dom,
        div.clone(),
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[Arg::Str("x"), Arg::Str("f")],
    );
    assert_eq!(
        result.unwrap_err(),
        ApiError::type_error("not a EventListener")
    );
}

/// A `once` listener is removed before its call; a passive one runs as
/// one; one removed by an earlier listener isn't called.
#[test]
fn once_passive_and_removed_listeners() {
    let mut dom = TestDom::new();
    let [_, _, div] = tree(&mut dom).map(This::Node);
    listen(
        &mut dom,
        &div,
        "x",
        1,
        Arg::Dict(vec![("once", Some(true))]),
    );
    listen(
        &mut dom,
        &div,
        "x",
        2,
        Arg::Dict(vec![("passive", Some(true))]),
    );
    listen(&mut dom, &div, "x", 3, Arg::Undefined);
    listen(&mut dom, &div, "x", 4, Arg::Undefined);
    dom.1.actions.borrow_mut().insert(3, Action::Remove(4));
    let event_ = event(&mut dom, "x", false, false);
    let log = dispatch(&mut dom, &div, &event_).1;
    assert_eq!(calls(&log), [(1, 2, "DIV"), (2, 2, "DIV"), (3, 2, "DIV")]);
    assert_eq!(
        log.iter().map(|call| call.passive).collect::<Vec<_>>(),
        [false, true, false]
    );
    assert_eq!(
        calls(&dispatch(&mut dom, &div, &event_).1),
        [(2, 2, "DIV"), (3, 2, "DIV")]
    );
}

/// A plain target is its event's whole path; the user agent's event is
/// trusted.
#[test]
fn a_plain_target_and_a_fired_event() {
    let mut dom = TestDom::new();
    let target = match call_on(
        &mut dom,
        This::None,
        "EventTarget",
        "constructor",
        MemberKind::Constructor,
        &[],
    )
    .0
    .unwrap()
    {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    };
    let this = This::Object(target.clone());
    listen(&mut dom, &this, "x", 1, Arg::Bool(true));
    listen(&mut dom, &this, "x", 2, Arg::Undefined);
    let event_ = event(&mut dom, "x", true, false);
    assert_eq!(
        calls(&dispatch(&mut dom, &this, &event_).1),
        [(1, 2, "#target"), (2, 2, "#target")]
    );

    // `fire_event`, with the tests' front end as the script host.
    let [_, _, div] = tree(&mut dom);
    let div_this = This::Node(div.clone());
    listen(&mut dom, &div_this, "load", 3, Arg::Undefined);
    dom.1.log.borrow_mut().clear();
    let root = TargetRoot::Node(dom_next_root(&mut dom, &div));
    let fired = crate::fire(&mut dom, &root, "load");
    assert!(fired);
    assert_eq!(calls(&dom.1.log.borrow()), [(3, 2, "DIV")]);
    assert!(dom.1.log.borrow()[0].trusted);
    let scripts = event(&mut dom, "load", false, false);
    assert!(!dispatch(&mut dom, &div_this, &scripts).1[0].trusted);
    // An event the user agent made, dispatched by script, isn't trusted
    // any more.
    let trusted = Rc::new(dom.mutate(|cx| {
        let flags = dom::EventFlags {
            trusted: true,
            ..dom::EventFlags::default()
        };
        cx.root_object(ObjectRef::Event(cx.create_event(
            dom::names::intern("load"),
            flags,
            None,
        )))
    }));
    assert!(!dispatch(&mut dom, &div_this, &trusted).1[0].trusted);
    assert!(matches!(
        get(&mut dom, &trusted, "isTrusted"),
        Ret::Bool(false)
    ));
}

fn dom_next_root(dom: &mut TestDom, node: &Rc<NodeRoot>) -> NodeRoot {
    dom.read(|cx| cx.root(node.get(cx.gc)))
}

/// An event dispatched again allocates nothing: its path's and its
/// snapshot's lists are reused.
#[test]
fn an_event_dispatched_again_allocates_nothing() {
    let mut dom = TestDom::new();
    let [document, _, div] = tree(&mut dom).map(This::Node);
    listen(&mut dom, &document, "x", 1, Arg::Undefined);
    listen(&mut dom, &div, "x", 2, Arg::Undefined);
    let event_ = event(&mut dom, "x", true, false);
    assert_eq!(dispatch(&mut dom, &div, &event_).0, Ok(true));
    let before = dom.stats();
    assert_eq!(dispatch(&mut dom, &div, &event_).1.len(), 2);
    let after = dom.stats();
    assert_eq!((after.gcs, after.scavenges), (before.gcs, before.scavenges));
    assert_eq!(after.used_bytes, before.used_bytes);
}

/// The targets with no listeners cost no operation of their own: a
/// dispatch through twenty of them takes as many operations as through
/// one, the listeners at the ends called as before; one added to a target
/// further up while the event goes is called when it gets there.
#[test]
fn targets_with_no_listeners_are_passed_over() {
    let operations = |depth: usize| {
        let mut dom = TestDom::new();
        let (document, deepest) = dom.mutate(|cx| {
            let document = cx.create_document(true);
            let mut parent = document;
            for _ in 0..depth {
                let div = cx.create_element(document, QualName::html("div"));
                cx.append_child(parent, div).unwrap();
                parent = div;
            }
            (Rc::new(cx.root(document)), Rc::new(cx.root(parent)))
        });
        let (document, deepest) = (This::Node(document), This::Node(deepest));
        listen(&mut dom, &document, "x", 1, Arg::Bool(true));
        listen(&mut dom, &deepest, "x", 2, Arg::Bool(false));
        listen(&mut dom, &document, "x", 3, Arg::Bool(false));
        let bubbling = event(&mut dom, "x", true, false);
        let before = dom.1.operations.get();
        let (result, log) = dispatch(&mut dom, &deepest, &bubbling);
        assert_eq!(result, Ok(true));
        assert_eq!(
            calls(&log)
                .iter()
                .map(|&(callback, ..)| callback)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        let used = dom.1.operations.get() - before;
        // The target's listener adds one to its parent, which had none.
        if depth > 1 {
            let parent = dom.read(|cx| {
                let This::Node(deepest) = &deepest else {
                    unreachable!()
                };
                Rc::new(cx.root(cx.parent(deepest.get(cx.gc)).expect("a parent")))
            });
            *dom.1.action_node.borrow_mut() = Some(parent);
            dom.1
                .actions
                .borrow_mut()
                .insert(2, Action::ListenAtNode(4));
            let (_, log) = dispatch(&mut dom, &deepest, &bubbling);
            assert_eq!(
                calls(&log)
                    .iter()
                    .map(|&(callback, ..)| callback)
                    .collect::<Vec<_>>(),
                [1, 2, 4, 3]
            );
        }
        used
    };
    assert_eq!(operations(20), operations(1));
}
