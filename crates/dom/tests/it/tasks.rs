//! The page's tasks in one numbered order (`Cx::next_task`): a timer, an
//! element task and a message run in the order they were queued, whatever
//! their task sources; a pass of the host's runs only those numbered before
//! it began; `Dom::has_queued_tasks` says whether one that is due now may
//! wait; the window's `load` is an element task after those queued before
//! it.

use dom::{
    Cx, Dom, DomString, ElementTaskKind, NodeRef, QualName, SerializedMessage, TaskQueue, Window,
    names,
};
use elise::Gc;

fn window<'gc>(cx: &Cx<'_, 'gc>) -> (Gc<'gc, Window>, NodeRef<'gc>) {
    let document = cx.create_document(true);
    (cx.create_window(document), document)
}

fn message() -> SerializedMessage {
    SerializedMessage {
        bytes: Vec::new(),
        transferred: None,
    }
}

/// The queue of the task `window` runs next by now, if any.
fn next<'gc>(cx: &Cx<'_, 'gc>, window: Gc<'gc, Window>) -> Option<TaskQueue> {
    // A little past now: the timer of 0 ms is due.
    cx.next_task(window, cx.dom_now() + 1.0)
        .map(|task| task.queue)
}

/// The task `next` names, taken.
fn take<'gc>(cx: &Cx<'_, 'gc>, window: Gc<'gc, Window>, queue: TaskQueue) {
    let now = cx.now_since(Some(window)) + 1.0;
    match queue {
        TaskQueue::Timer => assert!(cx.take_due(window, now).is_some()),
        TaskQueue::Element => assert!(cx.take_element_task(window).is_some()),
        TaskQueue::Message => assert!(cx.take_posted_message(window).is_some()),
        other => panic!("{other:?}"),
    }
}

#[test]
fn tasks_run_in_the_order_queued_across_their_sources() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.post_window_message(window, message(), DomString::new(""), None, &[]);
        cx.add_timer(window, 0.0, false);
        cx.queue_element_task(element, names::event::SELECT);
        cx.post_window_message(window, message(), DomString::new(""), None, &[]);
        let mut order = Vec::new();
        while let Some(queue) = next(cx, window) {
            order.push(queue);
            take(cx, window, queue);
        }
        assert_eq!(
            order,
            [
                TaskQueue::Message,
                TaskQueue::Timer,
                TaskQueue::Element,
                TaskQueue::Message
            ]
        );
    });
}

/// A timer not due yet isn't the next task, though queued first; an
/// interval set again is numbered after what was queued meanwhile.
#[test]
fn a_timer_waits_for_its_time_and_an_interval_goes_after() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.add_timer(window, 60_000.0, false);
        let interval = cx.add_timer(window, 0.0, true);
        assert_eq!(next(cx, window), Some(TaskQueue::Timer));
        let now = cx.now_since(Some(window)) + 1.0;
        let (taken, _) = cx.take_due(window, now).expect("due");
        assert!(Gc::ptr_eq(taken, interval));
        cx.queue_element_task(element, names::event::SELECT);
        // Due again at once (its interval clamped to 0), numbered after
        // the element task.
        assert_eq!(next(cx, window), Some(TaskQueue::Element));
    });
}

/// A timer not due yet is no task at all, the only one there is.
#[test]
fn a_timer_not_due_is_no_task() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.add_timer(window, 60_000.0, false);
        assert_eq!(next(cx, window), None);
    });
}

/// An interval set again in a pass waits for the next pass, though due at
/// once: it is numbered as it is set again, after the pass began.
#[test]
fn an_interval_set_again_waits_for_the_next_pass() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.add_timer(window, 0.0, true);
        cx.begin_task_pass();
        assert_eq!(next(cx, window), Some(TaskQueue::Timer));
        take(cx, window, TaskQueue::Timer);
        assert_eq!(next(cx, window), None, "held back for the next pass");
        cx.end_task_pass(&[window]);
        assert_eq!(next(cx, window), Some(TaskQueue::Timer));
    });
}

/// An element task queued in a pass isn't taken in it, whoever asks.
#[test]
fn an_element_task_queued_in_a_pass_waits_for_the_next() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.begin_task_pass();
        cx.queue_element_task(element, names::event::SELECT);
        assert!(cx.take_element_task(window).is_none(), "held back");
        cx.end_task_pass(&[window]);
        assert!(cx.take_element_task(window).is_some());
    });
}

/// A pass runs only the tasks numbered before it began: one queued in it
/// waits for the next, so tasks that queue others give way between
/// passes.
#[test]
fn a_pass_runs_what_was_queued_before_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.post_window_message(window, message(), DomString::new(""), None, &[]);
        cx.begin_task_pass();
        assert_eq!(next(cx, window), Some(TaskQueue::Message));
        take(cx, window, TaskQueue::Message);
        // Its listener posts another.
        cx.post_window_message(window, message(), DomString::new(""), None, &[]);
        assert_eq!(next(cx, window), None, "held back for the next pass");
        assert!(cx.take_posted_message(window).is_none());
        cx.end_task_pass(&[window]);
        assert_eq!(next(cx, window), Some(TaskQueue::Message));
    });
}

/// `has_queued_tasks`: set as an element task or a message is queued (a
/// timer waits for its time instead), cleared by the pass that leaves none
/// queued, and kept by one that held some back.
#[test]
fn the_flag_says_a_task_may_be_queued() {
    let mut dom = Dom::new();
    let window = dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.add_timer(window, 0.0, false);
        elise::Persistent::new(cx.gc, window)
    });
    assert!(!dom.has_queued_tasks(), "a timer waits for its time");
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        cx.post_window_message(window, message(), DomString::new(""), None, &[]);
    });
    assert!(dom.has_queued_tasks());
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        cx.begin_task_pass();
        take(cx, window, TaskQueue::Message);
        cx.post_window_message(window, message(), DomString::new(""), None, &[]);
        cx.end_task_pass(&[window]);
    });
    assert!(dom.has_queued_tasks(), "one held back");
    dom.mutate(|cx| {
        let window = window.get(cx.gc);
        cx.begin_task_pass();
        take(cx, window, TaskQueue::Message);
        cx.end_task_pass(&[window]);
    });
    assert!(!dom.has_queued_tasks());
}

/// The window's `load` is an element task, after those queued before it;
/// its document is the host's to finish once it ran.
#[test]
fn the_windows_load_is_an_element_task() {
    let mut dom = Dom::new();
    let id = dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.queue_element_task(element, names::event::ERROR);
        cx.queue_window_load(document);
        let (_, kind, _) = cx.take_element_task(window).expect("the error first");
        assert_eq!(kind, ElementTaskKind::Fire(names::event::ERROR));
        let (target, kind, _) = cx.take_element_task(window).expect("then the load");
        assert_eq!(kind, ElementTaskKind::WindowLoad);
        assert!(Gc::ptr_eq(target, document));
        cx.note_document_loaded(document);
        cx.node_id(document)
    });
    assert_eq!(dom.take_loaded_documents(), [id]);
    assert!(dom.take_loaded_documents().is_empty());
}
