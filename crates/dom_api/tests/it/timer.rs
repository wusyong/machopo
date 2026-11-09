//! A window's timers through its members, run by the host (`run_due`);
//! exceptions reported at a window (`report_exception`) and the
//! `ErrorEvent` they fire.

use std::rc::Rc;

use dom::{ObjectRef, ObjectRoot};
use dom_api::{MemberKind, Reported};
use elise::Persistent;

use crate::{Action, Arg, Ret, TestDom, This, TimerCall, call_on, error_info, with_host};

/// A new window, rooted as an object and as a window.
fn window(dom: &mut TestDom) -> (Rc<ObjectRoot>, Persistent<dom::Window>) {
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        (
            Rc::new(cx.root_object(ObjectRef::Window(window))),
            Persistent::new(cx.gc, window),
        )
    })
}

fn call(
    dom: &mut TestDom,
    window: &Rc<ObjectRoot>,
    name: &str,
    args: &[Arg],
) -> Result<Ret, dom_api::ApiError> {
    call_on(
        dom,
        This::Object(window.clone()),
        "Window",
        name,
        MemberKind::Method,
        args,
    )
    .0
}

fn id(ret: Result<Ret, dom_api::ApiError>) -> f64 {
    match ret.unwrap() {
        Ret::F64(id) => id,
        other => panic!("{other:?}"),
    }
}

fn run(dom: &mut TestDom, window: &Persistent<dom::Window>, now: f64) -> usize {
    with_host(dom, |host| dom_api::run_due(host, window, now))
}

fn calls(dom: &TestDom) -> Vec<(u64, u64, u8)> {
    dom.1
        .timer_log
        .borrow_mut()
        .drain(..)
        .map(
            |TimerCall {
                 callback,
                 arguments,
                 nesting,
             }| (callback, arguments, nesting),
        )
        .collect()
}

/// `setTimeout` and `setInterval` give ids from 1, keep their callback and
/// rest arguments, and run when due, in order; `clearTimeout` and
/// `clearInterval` clear either; a handler that isn't a function is a
/// string handler (run by the engine); the handler is required.
#[test]
fn timers_through_the_windows_members() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    let later = id(call(
        &mut dom,
        &object,
        "setTimeout",
        &[Arg::Callback(1), Arg::Number(50.0)],
    ));
    let soon = id(call(
        &mut dom,
        &object,
        "setTimeout",
        &[
            Arg::Callback(2),
            Arg::Undefined,
            Arg::Str("a"),
            Arg::Number(2.0),
        ],
    ));
    let interval = id(call(
        &mut dom,
        &object,
        "setInterval",
        &[Arg::Callback(3), Arg::Number(10.0)],
    ));
    assert_eq!([later, soon, interval], [1.0, 2.0, 3.0]);
    assert!(
        call(&mut dom, &object, "setTimeout", &[]).is_err(),
        "a handler is required"
    );

    let start = dom_api::next_due(&dom.dom(), &window).unwrap();
    assert_eq!(run(&mut dom, &window, start), 1);
    // The rest arguments' count, the tests' word; nested one level.
    assert_eq!(calls(&dom), [(2, 2, 1)]);
    assert_eq!(run(&mut dom, &window, start + 20.0), 1);
    assert_eq!(calls(&dom), [(3, 0, 1)]);
    // `clearTimeout` clears the interval, `clearInterval` the timeout.
    call(&mut dom, &object, "clearTimeout", &[Arg::Number(interval)]).unwrap();
    call(&mut dom, &object, "clearInterval", &[Arg::Number(later)]).unwrap();
    call(&mut dom, &object, "clearTimeout", &[]).unwrap();
    assert_eq!(run(&mut dom, &window, start + 1000.0), 0);
    assert_eq!(dom_api::next_due(&dom.dom(), &window), None);
    // The window's nesting level is back to 0 out of the timers.
    assert_eq!(dom.read(|cx| cx.timer_nesting(window.get(cx.gc))), 0);
}

/// A handler that isn't a function is kept as a string handler (the
/// tests' engine word for one), which the engine runs as a script.
#[test]
fn a_string_handler_is_kept() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    id(call(&mut dom, &object, "setTimeout", &[Arg::Str("code()")]));
    id(call(
        &mut dom,
        &object,
        "setInterval",
        &[Arg::Bool(true), Arg::Number(1000.0)],
    ));
    assert_eq!(run(&mut dom, &window, 1e9), 2);
    assert_eq!(
        calls(&dom),
        [(crate::STRING_HANDLER, 0, 1), (crate::STRING_HANDLER, 0, 1)]
    );
}

/// A timeout's `long`: wrapped (`ToInt32`), a negative one 0.
#[test]
fn a_timeouts_long() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    let now = dom.read(|cx| cx.now_since(Some(window.get(cx.gc))));
    // 2^32 + 5 wraps to 5.
    id(call(
        &mut dom,
        &object,
        "setTimeout",
        &[Arg::Callback(1), Arg::Number(4_294_967_301.0)],
    ));
    let due = dom_api::next_due(&dom.dom(), &window).unwrap();
    // Made a moment after `now` (well under 50 ms, even when busy).
    assert!((5.0..55.0).contains(&(due - now)), "{}", due - now);
    call(&mut dom, &object, "clearTimeout", &[Arg::Number(1.0)]).unwrap();
    id(call(
        &mut dom,
        &object,
        "setTimeout",
        &[Arg::Callback(1), Arg::Number(-30.0)],
    ));
    let due = dom_api::next_due(&dom.dom(), &window).unwrap();
    assert!(due - now < 50.0, "{}", due - now);
}

/// A timer's callback makes a timer one level deeper; in one run, the new
/// ones run too while due. An interval of 0 runs again in the same run
/// until it ran at level 6: past 5 it waits 4 ms, so the run ends.
#[test]
fn timers_made_by_timers_nest() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    // 107 makes 7, 7 nothing (`TestHost::call_timer`).
    id(call(&mut dom, &object, "setTimeout", &[Arg::Callback(107)]));
    assert_eq!(run(&mut dom, &window, 1e9), 2);
    assert_eq!(calls(&dom), [(107, 0, 1), (7, 0, 2)]);
    // 700 makes 600, ... 100 makes 0: levels 1 to 8.
    id(call(&mut dom, &object, "setTimeout", &[Arg::Callback(700)]));
    assert_eq!(run(&mut dom, &window, 2e9), 8);
    let levels: Vec<u8> = calls(&dom).iter().map(|call| call.2).collect();
    assert_eq!(levels, [1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(dom.read(|cx| cx.timer_nesting(window.get(cx.gc))), 0);
    let interval = id(call(
        &mut dom,
        &object,
        "setInterval",
        &[Arg::Callback(9), Arg::Number(0.0)],
    ));
    let at = 3e9;
    assert_eq!(run(&mut dom, &window, at), 6);
    assert_eq!(dom_api::next_due(&dom.dom(), &window), Some(at + 4.0));
    let levels: Vec<u8> = calls(&dom).iter().map(|call| call.2).collect();
    assert_eq!(levels, [1, 2, 3, 4, 5, 6]);
    call(&mut dom, &object, "clearInterval", &[Arg::Number(interval)]).unwrap();
}

#[test]
#[should_panic(expected = "timers run by a finite time")]
fn timers_run_by_a_finite_time() {
    let mut dom = TestDom::new();
    let (_, window) = window(&mut dom);
    run(&mut dom, &window, f64::INFINITY);
}

fn listen(dom: &mut TestDom, window: &Rc<ObjectRoot>, callback: u64, action: Action) {
    dom.1.actions.borrow_mut().insert(callback, action);
    call_on(
        dom,
        This::Object(window.clone()),
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[Arg::Str("error"), Arg::Callback(callback)],
    )
    .0
    .unwrap();
}

fn report(dom: &mut TestDom, window: &Persistent<dom::Window>) -> Reported {
    with_host(dom, |host| {
        dom_api::report_exception(host, window, &error_info(), Some(42))
    })
}

/// An exception reported fires an `ErrorEvent` at the window, trusted and
/// cancelable: a listener canceling it keeps it from the log. Its members
/// read where it was thrown and the value thrown.
#[test]
fn an_exception_reported_fires_an_error_event() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    assert_eq!(
        report(&mut dom, &window),
        Reported::NotCanceled,
        "no listener"
    );
    listen(&mut dom, &object, 1, Action::Nothing);
    assert_eq!(report(&mut dom, &window), Reported::NotCanceled);
    let log = dom.1.log.borrow().clone();
    assert_eq!(log.len(), 1);
    assert_eq!(
        (
            log[0].target.as_str(),
            log[0].phase,
            log[0].trusted,
            log[0].error
        ),
        ("#window", 2, true, 42),
        "the error the host stored"
    );
    listen(&mut dom, &object, 2, Action::Cancel);
    assert_eq!(report(&mut dom, &window), Reported::Canceled);
    assert!(
        !dom.read(|cx| cx.reporting(window.get(cx.gc))),
        "no longer reporting"
    );

    // The event's members, its interface ErrorEvent.
    let event = dom.mutate(|cx| {
        let event = cx.create_error_event(&error_info(), Some(window.get(cx.gc)));
        let data = cx.error_event_data(event).unwrap();
        cx.error_edge(data).set_embedder_word(cx.gc, 42);
        let api = dom_api::ApiCx::new(cx);
        assert_eq!(
            dom_api::INTERFACES[dom_api::interface_of(&api, ObjectRef::Event(event))].name,
            "ErrorEvent"
        );
        let plain = cx.create_event(
            dom::names::intern("error"),
            dom::EventFlags::default(),
            None,
        );
        assert_eq!(
            dom_api::INTERFACES[dom_api::interface_of(&api, ObjectRef::Event(plain))].name,
            "Event"
        );
        Rc::new(cx.root_object(ObjectRef::Event(event)))
    });
    let get = |dom: &mut TestDom, name: &str| {
        call_on(
            dom,
            This::Object(event.clone()),
            "ErrorEvent",
            name,
            MemberKind::Getter,
            &[],
        )
        .0
        .unwrap()
    };
    assert_eq!(get(&mut dom, "message").string(), "Error: boom");
    assert_eq!(
        get(&mut dom, "filename").string(),
        "https://example.test/a.js"
    );
    assert!(matches!(get(&mut dom, "lineno"), Ret::U32(3)));
    assert!(matches!(get(&mut dom, "colno"), Ret::U32(7)));
    assert!(matches!(get(&mut dom, "error"), Ret::Any(42)));
    // Event's members on it.
    assert_eq!(
        call_on(
            &mut dom,
            This::Object(event.clone()),
            "Event",
            "type",
            MemberKind::Getter,
            &[]
        )
        .0
        .unwrap()
        .string(),
        "error"
    );
}

/// An exception thrown while the window reports another (by an `error`
/// listener) isn't reported again; the window reports again after.
#[test]
fn an_exception_while_reporting_is_not_reported_again() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    listen(&mut dom, &object, 1, Action::ReportAgain);
    assert_eq!(report(&mut dom, &window), Reported::NotCanceled);
    assert_eq!(*dom.1.reported.borrow(), [Reported::WhileReporting]);
    assert_eq!(dom.1.log.borrow().len(), 1, "the listener called once");
    assert_eq!(report(&mut dom, &window), Reported::NotCanceled);
    assert_eq!(dom.1.log.borrow().len(), 2);
}

/// HTML's "the end": the document's readiness and its events in order,
/// `readystatechange` and `DOMContentLoaded` (bubbling) at the document,
/// then `readystatechange` and the window's `load`, whose target is the
/// document; a document's readiness is complete if no parser made it.
#[test]
fn a_documents_loading_fires_its_events() {
    let mut dom = TestDom::new();
    let (object, window) = window(&mut dom);
    let document = dom.read(|cx| cx.root(cx.window_document(window.get(cx.gc))));
    assert_eq!(
        dom.read(|cx| cx.ready_state(document.get(cx.gc))),
        dom::DocumentReadyState::Complete
    );
    dom.read(|cx| cx.set_ready_state(document.get(cx.gc), dom::DocumentReadyState::Loading));
    let document_object =
        Rc::new(dom.read(|cx| cx.root_object(ObjectRef::Node(document.get(cx.gc)))));
    for (this, event_type, callback) in [
        (&document_object, "readystatechange", 1),
        (&document_object, "DOMContentLoaded", 2),
        (&object, "DOMContentLoaded", 3),
        (&object, "load", 4),
        (&document_object, "load", 5),
    ] {
        call_on(
            &mut dom,
            This::Object(this.clone()),
            "EventTarget",
            "addEventListener",
            MemberKind::Method,
            &[Arg::Str(event_type), Arg::Callback(callback)],
        )
        .0
        .unwrap();
    }
    let ready = |dom: &mut TestDom| {
        let state = call_on(
            dom,
            This::Object(document_object.clone()),
            "Document",
            "readyState",
            MemberKind::Getter,
            &[],
        )
        .0
        .unwrap();
        state.string().to_string()
    };
    assert_eq!(ready(&mut dom), "loading");
    let events = |dom: &mut TestDom, readiness| {
        dom.1.log.borrow_mut().clear();
        with_host(dom, |host| {
            dom_api::document_ready(host, &document, readiness)
        });
        dom.1
            .log
            .borrow()
            .iter()
            .map(|call| {
                (
                    call.callback,
                    call.event_type.clone(),
                    call.target.clone(),
                    call.event_target.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    let mut parsed = events(&mut dom, dom_api::DocumentReadiness::Interactive);
    assert_eq!(ready(&mut dom), "interactive");
    parsed.extend(events(&mut dom, dom_api::DocumentReadiness::ContentLoaded));
    assert_eq!(ready(&mut dom), "interactive");
    let s = |text: &str| text.to_owned();
    assert_eq!(
        parsed,
        [
            (1, s("readystatechange"), s("#document"), s("#document")),
            (2, s("DOMContentLoaded"), s("#document"), s("#document")),
            (3, s("DOMContentLoaded"), s("#window"), s("#document")),
        ]
    );
    let loaded = events(&mut dom, dom_api::DocumentReadiness::Loaded);
    assert_eq!(ready(&mut dom), "complete");
    assert_eq!(
        loaded,
        [
            (1, s("readystatechange"), s("#document"), s("#document")),
            (4, s("load"), s("#window"), s("#document")),
        ],
        "load is the window's alone, its target the document"
    );
}
