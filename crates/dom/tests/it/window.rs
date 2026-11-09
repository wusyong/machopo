//! A document's window and its location; the document's URL; a window's
//! time origin, listeners and place among event targets.

use dom::{Dom, EventFlags, ListenerFlags, ObjectRef, QualName, TargetRef};
use elise::HeapOptions;

#[test]
fn a_window_and_its_document_know_each_other() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        assert!(cx.window_of(document).is_none());
        let window = cx.create_window(document);
        assert!(elise::Gc::ptr_eq(cx.window_of(document).unwrap(), window));
        assert!(elise::Gc::ptr_eq(cx.window_document(window), document));
        // The location: made once, its window's.
        let location = cx.window_location(window);
        assert!(elise::Gc::ptr_eq(cx.window_location(window), location));
        assert!(elise::Gc::ptr_eq(cx.location_window(location), window));
        // A document's template document, made after, keeps the window.
        cx.template_document(document);
        assert!(elise::Gc::ptr_eq(cx.window_of(document).unwrap(), window));
    });
}

#[test]
#[should_panic(expected = "a document has one window")]
fn a_document_has_one_window() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        cx.create_window(document);
        cx.create_window(document);
    });
}

#[test]
fn a_documents_url() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        assert_eq!(cx.document_url(document), None);
        // With its data made (a window) and no URL still none.
        let windowed = cx.create_document(true);
        cx.create_window(windowed);
        assert_eq!(cx.document_url(windowed), None);
        cx.set_document_url(document, "https://example.test/a");
        assert_eq!(cx.document_url(document).unwrap(), "https://example.test/a");
        // Its listeners and window kept with its data.
        cx.add_listener(
            TargetRef::Node(document),
            dom::names::intern("x"),
            ListenerFlags::default(),
        );
        cx.create_window(document);
        assert_eq!(cx.document_url(document).unwrap(), "https://example.test/a");
        assert_eq!(cx.listeners(TargetRef::Node(document)).unwrap().len(), 1);
    });
}

/// A window's time origin is when it was made: times since it are times
/// since the DOM was less that; an event made for it counts from it.
#[test]
fn times_count_from_a_windows_time_origin() {
    let mut dom = Dom::new();
    std::thread::sleep(std::time::Duration::from_millis(3));
    let (origin, since_window, since_dom, event_time) = dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        let event = cx.create_event(dom::names::intern("x"), EventFlags::default(), Some(window));
        (
            cx.window_time_origin(window),
            cx.now_since(Some(window)),
            cx.now_since(None),
            cx.event_time_stamp(event),
        )
    });
    assert!(origin >= 3.0, "{origin}");
    assert!(since_window < since_dom && since_dom - since_window >= origin - 0.001);
    assert!(event_time >= 0.0 && event_time <= since_window + 0.001);
}

/// A window is an event target of its own: its listeners, read back from
/// an erased edge, its root through a GC; the window a target belongs to.
#[test]
fn a_window_among_targets() {
    let mut dom = Dom::with_options(HeapOptions {
        verify: true,
        ..HeapOptions::default()
    });
    let root = dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let target = TargetRef::Window(window);
        cx.add_listener(target, dom::names::intern("load"), ListenerFlags::default());
        assert_eq!(cx.listeners(target).unwrap().len(), 1);
        assert!(matches!(
            TargetRef::from_erased(target.erase()),
            TargetRef::Window(_)
        ));
        // The window a target belongs to.
        let div = cx.create_element(document, QualName::html("div"));
        assert!(
            cx.target_window(TargetRef::Node(div))
                .unwrap()
                .ptr_eq_window(window)
        );
        assert!(cx.target_window(target).unwrap().ptr_eq_window(window));
        assert!(
            cx.target_window(TargetRef::EventTarget(cx.create_event_target()))
                .is_none()
        );
        let other = cx.create_document(true);
        assert!(cx.target_window(TargetRef::Node(other)).is_none());
        assert!(std::ptr::eq(
            cx.object_wrapper_edge(ObjectRef::Window(window)),
            cx.object_wrapper_edge(ObjectRef::Window(window))
        ));
        cx.root_target(target)
    });
    dom.collect();
    dom.mutate(|cx| {
        let TargetRef::Window(window) = root.get(cx.gc) else {
            panic!("a window");
        };
        assert_eq!(cx.listeners(TargetRef::Window(window)).unwrap().len(), 1);
        assert_eq!(cx.kind(cx.window_document(window)), dom::NodeKind::Document);
    });
}

trait PtrEqWindow {
    fn ptr_eq_window(self, other: Self) -> bool;
}

impl PtrEqWindow for elise::Gc<'_, dom::Window> {
    fn ptr_eq_window(self, other: Self) -> bool {
        elise::Gc::ptr_eq(self, other)
    }
}

/// A window's timers: ids from 1, due a timeout from now (a negative one
/// 0), removed by id; the next due; the earliest due taken first, those
/// due at once in the order made; a timeout taken out, an interval due
/// again an interval later.
#[test]
fn a_windows_timers() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        assert_eq!(cx.next_due(window), None);
        assert!(cx.take_due(window, 1e9).is_none());
        let now = cx.now_since(Some(window));
        let late = cx.add_timer(window, 50.0, false);
        let first = cx.add_timer(window, -5.0, false);
        let second = cx.add_timer(window, 0.0, false);
        let interval = cx.add_timer(window, 10.0, true);
        let ids: Vec<_> = [late, first, second, interval]
            .map(|timer| cx.timer_state(timer).id)
            .into();
        assert_eq!(ids, [1, 2, 3, 4]);
        let first_due = cx.timer_state(first).due;
        assert!(first_due >= now && first_due <= cx.now_since(Some(window)));
        assert!(cx.timer_state(late).due >= now + 50.0);
        assert_eq!(cx.timer_state(interval).interval, Some(10.0));
        assert_eq!(cx.timer_state(first).interval, None);
        assert_eq!(cx.next_due(window), Some(first_due));
        // Nothing due before the first.
        assert!(cx.take_due(window, first_due - 1.0).is_none());
        // Due at once: in the order made, ahead of the later ones.
        let at = cx.timer_state(late).due + 1.0;
        let taken: Vec<_> = std::iter::from_fn(|| cx.take_due(window, at))
            .take(3)
            .map(|(timer, _)| cx.timer_state(timer).id)
            .collect();
        assert_eq!(taken, [2, 3, 4]);
        // The interval stays, due again an interval after `at`.
        assert_eq!(cx.timer_state(interval).due, at + 10.0);
        assert_eq!(cx.timer_state(cx.take_due(window, at).unwrap().0).id, 1);
        assert!(cx.take_due(window, at).is_none());
        assert_eq!(cx.next_due(window), Some(at + 10.0));
        // Removed by id (an unknown one nothing).
        cx.remove_timer(window, 99);
        cx.remove_timer(window, 4);
        assert_eq!(cx.next_due(window), None);
    });
}

/// Timers due at once go in the order made: two intervals taken at one
/// time are due again at exactly one time.
#[test]
fn timers_due_at_once_go_in_the_order_made() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        let a = cx.add_timer(window, 10.0, true);
        let b = cx.add_timer(window, 10.0, true);
        let at = 1e6;
        cx.take_due(window, at).unwrap();
        cx.take_due(window, at).unwrap();
        assert_eq!(cx.timer_state(a).due, cx.timer_state(b).due);
        let order: Vec<_> = std::iter::from_fn(|| cx.take_due(window, at + 10.0))
            .map(|(timer, _)| cx.timer_state(timer).id)
            .collect();
        assert_eq!(order, [1, 2]);
        // A later one due earlier goes first.
        let c = cx.add_timer(window, 0.0, false);
        assert!(cx.timer_state(c).due < cx.timer_state(a).due);
        assert_eq!(
            cx.timer_state(cx.take_due(window, at + 20.0).unwrap().0).id,
            3
        );
    });
}

/// A timer's nesting level is one more than the running timer's; one made
/// by a timer nested deeper than 5 waits at least 4 ms, and so does an
/// interval once it repeated past 5.
#[test]
fn nested_timers_wait_at_least_4_ms() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        assert_eq!(cx.timer_nesting(window), 0);
        let outer = cx.add_timer(window, 0.0, false);
        assert_eq!(cx.timer_state(outer).nesting, 1);
        // An interval's timeout, clamped as a timeout's is: not at 5.
        cx.set_timer_nesting(window, 5);
        let at_5 = cx.add_timer(window, 1.0, true);
        let state = cx.timer_state(at_5);
        assert_eq!((state.nesting, state.interval), (6, Some(1.0)));
        cx.set_timer_nesting(window, 6);
        let before = cx.now_since(Some(window));
        let at_6 = cx.add_timer(window, 1.0, false);
        let state = cx.timer_state(at_6);
        assert_eq!(state.nesting, 7);
        assert!(state.due >= before + 4.0, "{state:?}");
        let interval_6 = cx.add_timer(window, -1.0, true);
        assert_eq!(cx.timer_state(interval_6).interval, Some(4.0));
        cx.remove_timer(window, cx.timer_state(interval_6).id);
        cx.set_timer_nesting(window, 0);
        cx.remove_timer(window, cx.timer_state(outer).id);
        cx.remove_timer(window, cx.timer_state(at_5).id);
        cx.remove_timer(window, cx.timer_state(at_6).id);
        // An interval of 0: due again at once until it ran 6 times.
        let interval = cx.add_timer(window, 0.0, true);
        let at = 1e6;
        for level in 1..=6u8 {
            assert_eq!(cx.take_due(window, at).unwrap().1, level);
        }
        let state = cx.timer_state(interval);
        assert_eq!((state.nesting, state.due), (7, at + 4.0));
        assert!(cx.take_due(window, at).is_none());
    });
}

/// A window's "in error reporting mode" flag, and an error event: an
/// `error` event, cancelable, trusted, not bubbling, with its data; a plain
/// event has none.
#[test]
fn an_error_event_and_its_data() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        assert!(!cx.reporting(window));
        cx.set_reporting(window, true);
        assert!(cx.reporting(window));
        let info = dom::ErrorInfo {
            message: "Error: boom".into(),
            filename: "https://example.test/a.js".into(),
            lineno: 3,
            colno: 7,
        };
        let event = cx.create_error_event(&info, Some(window));
        assert_eq!(dom::names::get(cx.event_type(event)), "error");
        assert_eq!(
            cx.event_flags(event),
            EventFlags {
                cancelable: true,
                trusted: true,
                ..EventFlags::default()
            }
        );
        let data = cx.error_event_data(event).unwrap();
        assert_eq!(cx.error_info(data), info);
        assert!(
            cx.error_event_data(cx.create_event(
                dom::names::intern("error"),
                EventFlags::default(),
                None
            ))
            .is_none()
        );
    });
}

/// A timer and an error event's data live as long as what holds them
/// (their window; the event), through GCs, with the heap verified.
#[test]
fn timers_and_error_data_survive_gcs() {
    let mut dom = Dom::with_options(HeapOptions {
        verify: true,
        ..HeapOptions::default()
    });
    let (window, event) = dom.mutate(|cx| {
        let window = cx.create_window(cx.create_document(true));
        cx.add_timer(window, 5.0, true);
        let info = dom::ErrorInfo {
            message: "m".into(),
            filename: "f".into(),
            lineno: 1,
            colno: 2,
        };
        let event = cx.create_error_event(&info, Some(window));
        (
            elise::Persistent::new(cx.gc, window),
            elise::Persistent::new(cx.gc, event),
        )
    });
    dom.collect();
    dom.collect();
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        let (timer, _) = cx.take_due(window, 1e9).unwrap();
        assert_eq!(cx.timer_state(timer).interval, Some(5.0));
        let data = cx.error_event_data(event.get(cx.gc)).unwrap();
        assert_eq!(cx.error_info(data).colno, 2);
    });
}

/// A time as a window's animation frames get it: milliseconds since its
/// time origin, not since the DOM was made.
#[test]
fn a_time_since_the_windows_origin() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let origin = std::time::Instant::now() + std::time::Duration::from_millis(500);
        cx.set_window_time_origin(window, origin);
        let at = cx.time_at(window, origin + std::time::Duration::from_millis(50));
        assert!((at - 50.0).abs() < 1e-6, "{at}");
    });
}
