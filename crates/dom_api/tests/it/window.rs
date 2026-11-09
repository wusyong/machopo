//! A window's and its location's members; events' paths through a
//! document's window; an event's time from its realm's window.

use std::rc::Rc;

use dom::{ObjectRef, ObjectRoot, QualName, TargetRoot};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call_on};

/// A document at `url` and its window, with an `<html>` in it.
fn window(dom: &mut TestDom, url: &str) -> (Rc<ObjectRoot>, Rc<dom::NodeRoot>) {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        cx.set_document_url(document, url);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let window = cx.create_window(document);
        (
            Rc::new(cx.root_object(ObjectRef::Window(window))),
            Rc::new(cx.root(html)),
        )
    })
}

fn get(dom: &mut TestDom, this: &Rc<ObjectRoot>, interface: &str, name: &str) -> Ret {
    call_on(
        dom,
        This::Object(this.clone()),
        interface,
        name,
        MemberKind::Getter,
        &[],
    )
    .0
    .unwrap()
}

fn object(ret: Ret) -> Rc<ObjectRoot> {
    match ret {
        Ret::Object(object) => Rc::new(object),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_windows_members() {
    let mut dom = TestDom::new();
    let (window, _) = window(&mut dom, "https://example.test/dir/page.html?q");
    for name in ["window", "self", "frames", "parent", "top"] {
        let got = object(get(&mut dom, &window, "Window", name));
        assert!(
            dom.read(|cx| matches!((got.get(cx.gc), window.get(cx.gc)),
            (ObjectRef::Window(a), ObjectRef::Window(b)) if elise::Gc::ptr_eq(a, b))),
            "{name}"
        );
    }
    assert!(matches!(
        get(&mut dom, &window, "Window", "document"),
        Ret::Node(_)
    ));
    let location = object(get(&mut dom, &window, "Window", "location"));
    let again = object(get(&mut dom, &window, "Window", "location"));
    assert!(
        dom.read(|cx| matches!((location.get(cx.gc), again.get(cx.gc)),
        (ObjectRef::Location(a), ObjectRef::Location(b)) if elise::Gc::ptr_eq(a, b)))
    );
    assert_eq!(
        get(&mut dom, &location, "Location", "href").string(),
        "https://example.test/dir/page.html?q"
    );
    assert_eq!(
        get(&mut dom, &location, "Location", "pathname").string(),
        "/dir/page.html"
    );
    let to_string = call_on(
        &mut dom,
        This::Object(location.clone()),
        "Location",
        "toString",
        MemberKind::Method,
        &[],
    );
    assert_eq!(
        to_string.0.unwrap().string(),
        "https://example.test/dir/page.html?q"
    );
    // A document with no URL is about:blank, whose path is opaque.
    let (blank, _) = self::window(&mut dom, "");
    let blank_location = object(get(&mut dom, &blank, "Window", "location"));
    assert_eq!(
        get(&mut dom, &blank_location, "Location", "href").string(),
        "about:blank"
    );
    assert_eq!(
        get(&mut dom, &blank_location, "Location", "pathname").string(),
        "blank"
    );
}

/// The window is the document's parent in an event's path, but for `load`;
/// an event the user agent fires at a window is its whole path, its time
/// since the window's time origin.
#[test]
fn events_reach_the_window() {
    let mut dom = TestDom::new();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let (window, html) = window(&mut dom, "about:blank");
    let listen =
        |dom: &mut TestDom, this: This, event_type: &'static str, callback: u64, capture: bool| {
            let (result, _) = call_on(
                dom,
                this,
                "EventTarget",
                "addEventListener",
                MemberKind::Method,
                &[
                    Arg::Str(event_type),
                    Arg::Callback(callback),
                    Arg::Bool(capture),
                ],
            );
            result.unwrap();
        };
    for event_type in ["x", "load"] {
        listen(&mut dom, This::Object(window.clone()), event_type, 1, true);
        listen(&mut dom, This::Object(window.clone()), event_type, 2, false);
    }
    let event = |dom: &mut TestDom, event_type: &'static str| {
        object(
            call_on(
                dom,
                This::None,
                "Event",
                "constructor",
                MemberKind::Constructor,
                &[
                    Arg::Str(event_type),
                    Arg::Dict(vec![("bubbles", Some(true))]),
                ],
            )
            .0
            .unwrap(),
        )
    };
    let dispatch = |dom: &mut TestDom, event: &Rc<ObjectRoot>| {
        dom.1.log.borrow_mut().clear();
        call_on(
            dom,
            This::Node(html.clone()),
            "EventTarget",
            "dispatchEvent",
            MemberKind::Method,
            &[Arg::Object(event.clone())],
        )
        .0
        .unwrap();
        dom.1
            .log
            .borrow()
            .iter()
            .map(|call| (call.callback, call.phase, call.target.clone()))
            .collect::<Vec<_>>()
    };
    let x = event(&mut dom, "x");
    assert_eq!(
        dispatch(&mut dom, &x),
        [(1, 1, "#window".to_owned()), (2, 3, "#window".to_owned())]
    );
    let load = event(&mut dom, "load");
    assert!(
        dispatch(&mut dom, &load).is_empty(),
        "a load event stops at the document"
    );

    dom.1.log.borrow_mut().clear();
    // The window's time origin well after the DOM's start: the event's
    // time counts from it.
    std::thread::sleep(std::time::Duration::from_millis(20));
    let target = dom.read(|cx| match window.get(cx.gc) {
        ObjectRef::Window(window) => {
            cx.set_window_time_origin(window, std::time::Instant::now());
            TargetRoot::Window(elise::Persistent::new(cx.gc, window))
        }
        _ => unreachable!(),
    });
    assert!(crate::fire(&mut dom, &target, "load"));
    let log = dom.1.log.borrow().clone();
    assert_eq!(
        log.iter()
            .map(|call| (call.callback, call.phase))
            .collect::<Vec<_>>(),
        [(1, 2), (2, 2)]
    );
    assert!(log.iter().all(|call| call.trusted));
    assert!(
        log[0].time < 15.0,
        "since the window's time origin: {}",
        log[0].time
    );
}

/// `new Event` in a realm whose global is a window counts its time from
/// the window's time origin, not the DOM's start.
#[test]
fn a_new_event_counts_from_its_realms_window() {
    let mut dom = TestDom::new();
    std::thread::sleep(std::time::Duration::from_millis(30));
    let (window, _) = window(&mut dom, "about:blank");
    let time = |dom: &mut TestDom| {
        let event = object(
            call_on(
                dom,
                This::None,
                "Event",
                "constructor",
                MemberKind::Constructor,
                &[Arg::Str("x")],
            )
            .0
            .unwrap(),
        );
        match get(dom, &event, "Event", "timeStamp") {
            Ret::F64(time) => time,
            other => panic!("{other:?}"),
        }
    };
    let since_dom = time(&mut dom);
    // Its time origin now (the test's, not however long making the window
    // took): 30 ms and more after the DOM's start.
    *dom.1.current_window.borrow_mut() = Some(dom.read(|cx| match window.get(cx.gc) {
        ObjectRef::Window(window) => {
            cx.set_window_time_origin(window, std::time::Instant::now());
            elise::Persistent::new(cx.gc, window)
        }
        _ => unreachable!(),
    }));
    let since_window = time(&mut dom);
    assert!(since_dom >= 30.0, "{since_dom}");
    assert!(
        since_window < since_dom - 15.0,
        "{since_window} {since_dom}"
    );
}
