//! A worker's global (`Cx::create_worker_global`): its execution context
//! answers its URL, base URL and origin from it, not from a document; its
//! timers, tasks and messages are its context's, whatever other realm's
//! the DOM has; it is a target of its own (an event's, read back as
//! one), of no window.

use dom::{
    Cx, Dom, DomString, EventFlags, SerializedMessage, TargetRef, TaskQueue, WorkerGlobalScope,
};
use elise::Gc;

fn worker<'gc>(cx: &Cx<'_, 'gc>, url: &str, origin: url::Origin) -> Gc<'gc, WorkerGlobalScope> {
    cx.create_worker_global(url::Url::parse(url).unwrap(), origin, DomString::new(""))
}

fn message() -> SerializedMessage {
    SerializedMessage {
        bytes: Vec::new(),
        transferred: None,
        ports: Vec::new(),
    }
}

/// Its context's URL and base URL are its URL, its origin the one given (an
/// opaque one for a script of a `data:` URL, its owner's otherwise); the
/// context's global is it, and it is no window's.
#[test]
fn a_worker_s_global_answers_its_url_and_origin() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let url = "https://example.test/scripts/w.js";
        let origin = url::Url::parse("https://example.test/").unwrap().origin();
        let global = worker(cx, url, origin.clone());
        let context = cx.worker_global_context(global);
        assert_eq!(cx.context_base_url(context).unwrap().as_str(), url);
        assert_eq!(cx.context_url(context).unwrap().to_string(), url);
        assert_eq!(cx.context_origin(context), Some(origin));
        assert!(Gc::ptr_eq(
            cx.context_worker_global(context).unwrap(),
            global
        ));
        let target = TargetRef::WorkerGlobal(global);
        assert!(Gc::ptr_eq(cx.target_context(target).unwrap(), context));
        assert!(cx.target_window(target).is_none());

        let opaque = url::Origin::new_opaque();
        let data = worker(cx, "data:text/javascript,", opaque.clone());
        let context = cx.worker_global_context(data);
        assert_eq!(cx.context_origin(context), Some(opaque));
        // A window's context has no worker's global.
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        assert!(
            cx.context_worker_global(cx.window_context(window))
                .is_none()
        );
    });
}

/// A timer set in a worker's global's context is due and run there, and
/// cleared there: a window's in the same DOM doesn't see it, nor it the
/// window's.
#[test]
fn a_worker_s_global_runs_its_own_timers() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let origin = url::Url::parse("https://example.test/").unwrap().origin();
        let context = cx.worker_global_context(worker(cx, "https://example.test/w.js", origin));
        let document = cx.create_document(true);
        let window = cx.window_context(cx.create_window(document));
        cx.add_timer(window, 50_000.0, false);

        let soon = cx.now_in(Some(context)) + 1.0;
        assert!(cx.next_task(context, cx.dom_now() + 1.0).is_none());
        let timer = cx.add_timer(context, 0.0, true);
        let next = cx
            .next_task(context, cx.dom_now() + 1.0)
            .expect("its timer is due");
        assert_eq!(next.queue, TaskQueue::Timer);
        assert!(cx.next_due(context).is_some_and(|due| due <= soon));
        let (taken, _) = cx.take_due(context, soon).expect("its timer");
        assert!(Gc::ptr_eq(taken, timer));
        // The window's, due later, is the window's only.
        assert!(cx.take_due(window, soon).is_none());
        cx.remove_timer(context, cx.timer_state(timer).id);
        assert!(cx.next_due(context).is_none());
        assert!(cx.next_due(window).is_some());
    });
}

/// A message to a port of a worker's global's context is its context's
/// task, for that port.
#[test]
fn a_worker_s_global_delivers_its_ports_messages() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let origin = url::Url::parse("https://example.test/").unwrap().origin();
        let context = cx.worker_global_context(worker(cx, "https://example.test/w.js", origin));
        let channel = cx.create_message_channel(Some(context));
        let (one, two) = cx.message_channel_ports(channel);
        cx.start_message_port(two);
        assert!(!cx.has_posted_message(context));
        cx.post_port_message(one, message());
        assert!(cx.has_posted_message(context));
        let next = cx
            .next_task(context, cx.dom_now() + 1.0)
            .expect("its message");
        assert_eq!(next.queue, TaskQueue::Message);
        let taken = cx.take_posted_message(context).expect("its message");
        assert!(matches!(taken.target, TargetRef::EventTarget(port) if Gc::ptr_eq(port, two)));
        assert!(!cx.has_posted_message(context));
    });
}

/// An event's target a worker's global is read back as one.
#[test]
fn an_event_at_a_worker_s_global_is_at_it() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let origin = url::Url::parse("https://example.test/").unwrap().origin();
        let global = worker(cx, "https://example.test/w.js", origin);
        let context = cx.worker_global_context(global);
        let event = cx.create_event(
            dom::names::intern("x"),
            EventFlags::default(),
            Some(context),
        );
        cx.set_event_target(event, Some(TargetRef::WorkerGlobal(global)));
        assert!(matches!(
            cx.event_target(event),
            Some(TargetRef::WorkerGlobal(target)) if Gc::ptr_eq(target, global)
        ));
    });
}
