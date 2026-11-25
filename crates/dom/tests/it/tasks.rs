//! The page's tasks as its host's scheduler runs them (`crate::run`): a
//! timer, an element task and a message in the order they were queued,
//! whatever their task sources; a timer once it is due, an interval again
//! as it is taken; the window's `load` an element task after those queued
//! before it; whether a task of each kind is due, in no order
//! (`Cx::has_task_due_by`, debug builds' check for a lost token).

use std::time::Duration;

use dom::{
    Cx, DomString, ElementTaskKind, NativeAction, NodeRef, QualName, SerializedMessage, TaskQueue,
    Window, names,
};
use elise::Gc;

use crate::run::{self, Taken};

fn window<'gc>(cx: &Cx<'_, 'gc>) -> (Gc<'gc, Window>, NodeRef<'gc>) {
    let document = cx.create_document(true);
    (cx.create_window(document), document)
}

fn message() -> SerializedMessage {
    SerializedMessage {
        bytes: Vec::new(),
        transferred: None,
        ports: Vec::new(),
    }
}

/// The queues of `window`'s tasks due now, each taken.
fn take_due<'gc>(cx: &Cx<'_, 'gc>, window: Gc<'gc, Window>) -> Vec<TaskQueue> {
    run::take_due(cx, &[cx.window_context(window)])
        .iter()
        .map(Taken::queue)
        .collect()
}

#[test]
fn tasks_run_in_the_order_queued_across_their_sources() {
    let mut dom = run::dom();
    dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.post_window_message(window, message(), DomString::new(""), None);
        cx.add_timer(cx.window_context(window), 0.0, false);
        cx.queue_element_task(element, names::event::SELECT);
        cx.post_window_message(window, message(), DomString::new(""), None);
        assert_eq!(
            take_due(cx, window),
            [
                TaskQueue::Message,
                TaskQueue::Timer,
                TaskQueue::Element,
                TaskQueue::Message
            ]
        );
    });
}

/// A timer not due yet is no task, though set first; an interval is set
/// again as it is taken, before what its task queues.
#[test]
fn a_timer_waits_for_its_time_and_an_interval_comes_again() {
    let mut dom = run::dom();
    dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.add_timer(cx.window_context(window), 60_000.0, false);
        let interval = cx.add_timer(cx.window_context(window), 0.0, true);
        let taken = run::take_due(cx, &[cx.window_context(window)]);
        assert!(matches!(taken[..], [Taken::Timer(timer)] if Gc::ptr_eq(timer, interval)));
        // What its callback would queue.
        cx.queue_element_task(element, names::event::SELECT);
        assert_eq!(take_due(cx, window), [TaskQueue::Timer, TaskQueue::Element]);
    });
}

/// A timer not due yet is no task at all, the only one there is.
#[test]
fn a_timer_not_due_is_no_task() {
    let mut dom = run::dom();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.add_timer(cx.window_context(window), 60_000.0, false);
        assert_eq!(take_due(cx, window), []);
    });
}

/// An interval set again as it is taken comes in the next turn, not the
/// one it was taken in (the host's thread goes on to its other tasks).
#[test]
fn an_interval_set_again_comes_in_the_next_turn() {
    let mut dom = run::dom();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.add_timer(cx.window_context(window), 0.0, true);
        assert_eq!(take_due(cx, window), [TaskQueue::Timer]);
        assert_eq!(take_due(cx, window), [TaskQueue::Timer]);
    });
}

/// The window's `load` is an element task, after those queued before it;
/// its document is the host's to finish once it ran.
#[test]
fn the_windows_load_is_an_element_task() {
    let mut dom = run::dom();
    dom.mutate(|cx| {
        let (window, document) = window(cx);
        let element = cx.create_element(document, QualName::html("p"));
        cx.queue_element_task(element, names::event::ERROR);
        cx.queue_window_load(document);
        let tasks = run::take_element_tasks(cx, &[cx.window_context(window)]);
        let [(_, error, _), (target, load, _)] = &tasks[..] else {
            panic!("the error, then the load");
        };
        assert_eq!(*error, ElementTaskKind::Fire(names::event::ERROR));
        assert_eq!(*load, ElementTaskKind::WindowLoad);
        assert!(Gc::ptr_eq(target.expect("its document"), document));
    });
}

/// A window's idle callback with a timeout is one of its tasks once the
/// timeout is due; one with none never is.
#[test]
fn an_idle_callback_s_timeout_is_a_task_once_due() {
    let mut dom = run::dom();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        cx.add_idle_callback(window, 0);
        cx.add_idle_callback(window, 5);
        assert_eq!(take_due(cx, window), []);
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(take_due(cx, window), [TaskQueue::IdleTimeout]);
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(take_due(cx, window), []);
    });
}

/// Each kind of task is due by a time a little past now, none before it:
/// a script timer, a UA timer, an idle callback's timeout, an element task,
/// a window's message, and a port's (once its port is enabled).
#[test]
fn a_task_of_each_kind_is_due() {
    type Add = for<'c, 'gc> fn(&Cx<'c, 'gc>, Gc<'gc, Window>, NodeRef<'gc>);
    let kinds: [Add; 6] = [
        |cx, window, _| {
            cx.add_timer(cx.window_context(window), 0.0, false);
        },
        |cx, _, document| {
            let element = cx.create_element(document, QualName::html("input"));
            cx.append_child(document, element).unwrap();
            cx.add_native_timer(element, NativeAction::SpinRepeat, 0, 0.0, None);
        },
        |cx, window, _| {
            cx.add_idle_callback(window, 1);
        },
        |cx, _, document| {
            let element = cx.create_element(document, QualName::html("p"));
            cx.queue_element_task(element, names::event::SELECT);
        },
        |cx, window, _| cx.post_window_message(window, message(), DomString::new(""), None),
        |cx, window, _| {
            let channel = cx.create_message_channel(Some(cx.window_context(window)));
            let (one, two) = cx.message_channel_ports(channel);
            cx.post_port_message(one, message());
            cx.start_message_port(two);
        },
    ];
    for (index, add) in kinds.iter().enumerate() {
        let mut dom = dom::Dom::new();
        dom.mutate(|cx| {
            let (window, document) = window(cx);
            let context = cx.window_context(window);
            add(cx, window, document);
            assert!(
                !cx.has_task_due_by(context, cx.dom_now() - 1.0),
                "kind {index}"
            );
            assert!(
                cx.has_task_due_by(context, cx.dom_now() + 2.0),
                "kind {index}"
            );
        });
    }
}

/// A port's message isn't a task due till its port is enabled.
#[test]
fn a_port_s_message_is_no_task_due_before_its_port_is_enabled() {
    let mut dom = dom::Dom::new();
    dom.mutate(|cx| {
        let (window, _) = window(cx);
        let context = cx.window_context(window);
        let (one, _) = cx.message_channel_ports(cx.create_message_channel(Some(context)));
        cx.post_port_message(one, message());
        assert!(!cx.has_task_due_by(context, cx.dom_now() + 2.0));
    });
}
