//! A window's animation frame callbacks through its members and the
//! host's frame (`run_animation_frame_callbacks`): handles, cancelling,
//! the order, one requested or cancelled while they run, the time, a
//! checkpoint after each.

use std::rc::Rc;

use dom::{ObjectRef, ObjectRoot, QualName};
use dom_api::MemberKind;

use crate::{Arg, Ret, TestDom, This, call_on, with_host};

fn window(dom: &mut TestDom) -> (Rc<ObjectRoot>, elise::Persistent<dom::Window>) {
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let html = cx.create_element(document, QualName::html("html"));
        cx.append_child(document, html).unwrap();
        let window = cx.create_window(document);
        (
            Rc::new(cx.root_object(ObjectRef::Window(window))),
            elise::Persistent::new(cx.gc, window),
        )
    })
}

fn request(dom: &mut TestDom, window: &Rc<ObjectRoot>, callback: u64) -> f64 {
    match call_on(
        dom,
        This::Object(window.clone()),
        "Window",
        "requestAnimationFrame",
        MemberKind::Method,
        &[Arg::Callback(callback)],
    )
    .0
    .unwrap()
    {
        Ret::F64(handle) => handle,
        other => panic!("{other:?}"),
    }
}

fn cancel(dom: &mut TestDom, window: &Rc<ObjectRoot>, handle: f64) {
    call_on(
        dom,
        This::Object(window.clone()),
        "Window",
        "cancelAnimationFrame",
        MemberKind::Method,
        &[Arg::Number(handle)],
    )
    .0
    .unwrap();
}

/// What a frame at `now` called: callback numbers and times.
fn frame(dom: &TestDom, window: &elise::Persistent<dom::Window>, now: f64) -> Vec<(u64, f64)> {
    with_host(dom, |host| {
        dom_api::run_animation_frame_callbacks(host, window, now)
    });
    dom.1.frame_log.borrow_mut().drain(..).collect()
}

#[test]
fn callbacks_run_in_order_once() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    assert_eq!(
        [1, 2, 3].map(|callback| request(&mut dom, &object, callback)),
        [1.0, 2.0, 3.0]
    );
    cancel(&mut dom, &object, 2.0);
    // An unknown handle: nothing.
    cancel(&mut dom, &object, 9.0);
    assert!(dom.read(|cx| cx.has_animation_frames(window.get(cx.gc))));
    let checkpoints = dom.1.checkpoints.get();
    // The frame's time, coarsened as `performance.now()` is.
    assert_eq!(frame(&dom, &window, 16.789), [(1, 16.7), (3, 16.7)]);
    assert_eq!(dom.1.checkpoints.get(), checkpoints + 2);
    assert!(
        !dom.read(|cx| cx.has_animation_frames(window.get(cx.gc))),
        "each runs once"
    );
    assert!(frame(&dom, &window, 33.0).is_empty());
    // Handles go on from the last.
    assert_eq!(request(&mut dom, &object, 1), 4.0);
}

/// One requested while they run waits for the next frame; one an earlier
/// callback cancels doesn't run.
#[test]
fn what_callbacks_request_and_cancel() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    // Requests one of number 7.
    request(&mut dom, &object, 107);
    // Cancels handle 3, the next.
    request(&mut dom, &object, 203);
    request(&mut dom, &object, 5);
    assert_eq!(frame(&dom, &window, 1.0), [(107, 1.0), (203, 1.0)]);
    assert_eq!(frame(&dom, &window, 2.0), [(7, 2.0)]);
}
