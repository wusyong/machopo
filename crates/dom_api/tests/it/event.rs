//! `EventTarget`'s and `Event`'s constructors, and an event's members
//! before any dispatch; `EventInit`, a dictionary, read as WebIDL reads
//! one.

use std::rc::Rc;

use dom::{ObjectRef, ObjectRoot};
use dom_api::{ApiError, MemberKind, coarsen};

use crate::{Arg, Hooks, Ret, TestDom, This, call_on};

/// `new INTERFACE(args)`: the new object, and what the call asked of its
/// front end.
fn construct(
    dom: &mut TestDom,
    interface: &str,
    args: &[Arg],
) -> (Result<ObjectRoot, ApiError>, Hooks) {
    let (result, hooks) = call_on(
        dom,
        This::None,
        interface,
        "constructor",
        MemberKind::Constructor,
        args,
    );
    let object = result.map(|ret| match ret {
        Ret::Object(object) => object,
        other => panic!("a constructor gave {other:?}"),
    });
    (object, hooks)
}

fn event(dom: &mut TestDom, args: &[Arg]) -> Rc<ObjectRoot> {
    let object = construct(dom, "Event", args).0.unwrap();
    assert!(dom.read(|cx| matches!(object.get(cx.gc), ObjectRef::Event(_))));
    Rc::new(object)
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

fn flags(dom: &mut TestDom, event: &Rc<ObjectRoot>) -> [bool; 4] {
    ["bubbles", "cancelable", "composed", "isTrusted"].map(|name| match get(dom, event, name) {
        Ret::Bool(value) => value,
        other => panic!("{name}: {other:?}"),
    })
}

#[test]
fn new_event_target_makes_a_target_that_is_nothing_else() {
    let mut dom = TestDom::new();
    let (target, _) = construct(&mut dom, "EventTarget", &[]);
    let target = target.unwrap();
    assert!(dom.read(|cx| matches!(target.get(cx.gc), ObjectRef::EventTarget(_))));
}

/// An event made by script: its type, not trusted, `NONE` its phase, its
/// flags `EventInit`'s defaults with no dictionary (or undefined, or null).
#[test]
fn a_new_event_has_its_type_and_the_defaults() {
    let mut dom = TestDom::new();
    for args in [
        vec![Arg::Str("x")],
        vec![Arg::Str("x"), Arg::Undefined],
        vec![Arg::Str("x"), Arg::Null],
    ] {
        let event = event(&mut dom, &args);
        assert_eq!(get(&mut dom, &event, "type").string(), "x");
        assert_eq!(flags(&mut dom, &event), [false; 4]);
        assert!(matches!(get(&mut dom, &event, "eventPhase"), Ret::U32(0)));
    }
    // The type is a string: WebIDL's `ToString`.
    let event = event(&mut dom, &[Arg::Null]);
    assert_eq!(get(&mut dom, &event, "type").string(), "null");
}

/// Equal types are one name's number (`load`: Blink's fixed one).
#[test]
fn equal_types_are_one_name() {
    let mut dom = TestDom::new();
    let a = event(&mut dom, &[Arg::Str("load")]);
    let b = event(&mut dom, &[Arg::Str("load")]);
    assert!(dom.read(|cx| {
        let (ObjectRef::Event(a), ObjectRef::Event(b)) = (a.get(cx.gc), b.get(cx.gc)) else {
            unreachable!()
        };
        cx.event_type(a) == cx.event_type(b) && cx.event_type(a) == dom::names::event::LOAD
    }));
}

/// `EventInit`'s members, read in their names' order whatever the order
/// given; a missing one is its default.
#[test]
fn event_init_is_read_in_its_members_order() {
    let mut dom = TestDom::new();
    let dict = Arg::Dict(vec![("composed", Some(true)), ("bubbles", Some(true))]);
    let (event, hooks) = construct(&mut dom, "Event", &[Arg::Str("x"), dict]);
    assert_eq!(hooks.dict_reads, ["bubbles", "composed"]);
    let event = Rc::new(event.unwrap());
    assert_eq!(flags(&mut dom, &event), [true, false, true, false]);
    let dict = Arg::Dict(vec![
        ("composed", Some(false)),
        ("cancelable", Some(true)),
        ("bubbles", Some(false)),
    ]);
    let (event, hooks) = construct(&mut dom, "Event", &[Arg::Str("x"), dict]);
    assert_eq!(hooks.dict_reads, ["bubbles", "cancelable", "composed"]);
    assert_eq!(
        flags(&mut dom, &Rc::new(event.unwrap())),
        [false, true, false, false]
    );
}

/// A dictionary that isn't an object is a `TypeError`; a member whose
/// getter throws stops the reading, and nothing is made.
#[test]
fn a_bad_event_init_throws_and_makes_nothing() {
    let mut dom = TestDom::new();
    let (result, _) = construct(&mut dom, "Event", &[Arg::Str("x"), Arg::Str("init")]);
    assert_eq!(result.unwrap_err(), ApiError::type_error("not a EventInit"));
    let dict = Arg::Dict(vec![("cancelable", None), ("composed", Some(true))]);
    let (result, hooks) = construct(&mut dom, "Event", &[Arg::Str("x"), dict]);
    assert_eq!(
        result.unwrap_err(),
        ApiError::type_error("cancelable's getter threw")
    );
    assert_eq!(hooks.dict_reads, ["cancelable"]);
    assert!(!hooks.finished);
}

/// `type` is required, the dictionary optional: `length` is 1.
#[test]
fn an_event_needs_its_type() {
    let mut dom = TestDom::new();
    let (result, _) = construct(&mut dom, "Event", &[]);
    assert_eq!(
        result.unwrap_err(),
        ApiError::type_error("Event: 1 arguments required")
    );
    let constructor = dom_api::INTERFACES
        .iter()
        .find(|desc| desc.name == "Event")
        .unwrap()
        .members
        .iter()
        .find(|member| member.kind == MemberKind::Constructor)
        .unwrap();
    assert_eq!(constructor.required(), 1);
    assert!(constructor.params[1].optional && !constructor.params[0].optional);
}

/// `timeStamp`: since the DOM was made, coarsened to 100 µs; a later
/// event's isn't earlier.
#[test]
fn time_stamps_are_coarsened_and_in_order() {
    let mut dom = TestDom::new();
    let time = |dom: &mut TestDom, event: &Rc<ObjectRoot>| match get(dom, event, "timeStamp") {
        Ret::F64(time) => time,
        other => panic!("{other:?}"),
    };
    let first = event(&mut dom, &[Arg::Str("x")]);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = event(&mut dom, &[Arg::Str("x")]);
    let (a, b) = (time(&mut dom, &first), time(&mut dom, &second));
    assert!(a >= 0.0 && b >= a + 1.9, "{a} then {b}");
    for time in [a, b] {
        assert_eq!(coarsen(time), time, "{time} is coarsened");
    }
    assert_eq!(coarsen(1.234_56), 1.2);
    assert_eq!(coarsen(0.099_9), 0.0);
}

/// The phases' constants.
#[test]
fn event_has_the_phases_constants() {
    let event = dom_api::INTERFACES
        .iter()
        .find(|desc| desc.name == "Event")
        .unwrap();
    let constants: Vec<_> = event
        .members
        .iter()
        .filter_map(|member| match member.kind {
            MemberKind::Constant(value) => Some((member.name, value)),
            _ => None,
        })
        .collect();
    assert_eq!(
        constants,
        [
            ("NONE", 0),
            ("CAPTURING_PHASE", 1),
            ("AT_TARGET", 2),
            ("BUBBLING_PHASE", 3)
        ]
    );
}
