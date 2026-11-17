//! A `Worker` (`Cx::create_worker_object`): an event target of the realm
//! that made it, with its worker's number, terminated for good once.

use dom::{Dom, DomString, TargetRef};
use elise::Gc;

#[test]
fn a_worker_is_a_target_of_its_realm_with_its_worker_s_number() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let document = cx.create_document(true);
        let window = cx.create_window(document);
        let context = cx.window_context(window);
        let worker = cx.create_worker_object(context, 7);
        assert!(cx.is_worker(worker));
        assert!(!cx.is_worker(cx.create_event_target()));
        assert_eq!(cx.worker_id(worker), 7);
        let target = TargetRef::EventTarget(worker);
        assert!(Gc::ptr_eq(cx.target_context(target).unwrap(), context));
        assert!(Gc::ptr_eq(cx.target_window(target).unwrap(), window));
        assert!(!cx.worker_terminated(worker));
        cx.terminate_worker_object(worker);
        assert!(cx.worker_terminated(worker));
    });
}

/// A worker's global has the name its worker was made with.
#[test]
fn a_worker_s_global_has_its_name() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let global = cx.create_worker_global(
            url::Url::parse("https://example.test/w.js").unwrap(),
            url::Url::parse("https://example.test/").unwrap().origin(),
            DomString::new("fib"),
        );
        assert_eq!(cx.worker_global_name(global).to_string(), "fib");
    });
}
