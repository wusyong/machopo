//! Dedicated workers through the table: `new Worker()` starts one through
//! the engine's host (its URL against the realm's base URL, its name, the
//! realm's origin), `postMessage()` and `terminate()` go to the host at
//! once; a worker's global's members (its timers, `name`, `postMessage()`,
//! `close()`, `self`, `crypto`) are its own and its execution context's.

use std::rc::Rc;

use dom::{ObjectRef, ObjectRoot};
use dom_api::{ApiError, ExceptionName, MemberKind, WorkerStart};
use elise::Persistent;

use crate::{Arg, Ret, TestDom, This, WorkerCall, call_on};

/// A window at `https://example.test/dir/page.html`, the realm's; with a
/// workers' host if `hosted`.
fn page(hosted: bool) -> TestDom {
    let dom = TestDom::new();
    let window = dom.read(|cx| {
        let document = cx.create_document(true);
        cx.set_document_url(document, "https://example.test/dir/page.html");
        Persistent::new(cx.gc, cx.create_window(document))
    });
    *dom.1.current_window.borrow_mut() = Some(window);
    dom.1.workers.borrow_mut().hosted = hosted;
    dom
}

fn construct(dom: &mut TestDom, args: &[Arg]) -> Result<Rc<ObjectRoot>, ApiError> {
    call_on(
        dom,
        This::None,
        "Worker",
        "constructor",
        MemberKind::Constructor,
        args,
    )
    .0
    .map(|ret| match ret {
        Ret::Object(object) => Rc::new(object),
        other => panic!("a constructor gave {other:?}"),
    })
}

fn call(
    dom: &mut TestDom,
    this: &Rc<ObjectRoot>,
    interface: &str,
    name: &str,
    args: &[Arg],
) -> Result<Ret, ApiError> {
    call_on(
        dom,
        This::Object(this.clone()),
        interface,
        name,
        MemberKind::Method,
        args,
    )
    .0
}

fn calls(dom: &TestDom) -> Vec<WorkerCall> {
    std::mem::take(&mut dom.1.workers.borrow_mut().calls)
}

fn dom_error(error: Result<Rc<ObjectRoot>, ApiError>) -> ExceptionName {
    match error {
        Err(ApiError::Dom(exception)) => exception.name,
        Err(other) => panic!("{other:?}"),
        Ok(_) => panic!("no error"),
    }
}

/// `new Worker(url, { name })`: a `Worker` of the realm, its worker started
/// with the URL parsed against the realm's base URL, the name and the
/// realm's origin; the host's number is the worker's.
#[test]
fn a_worker_starts_its_worker() {
    let mut dom = page(true);
    let worker = construct(&mut dom, &[Arg::Str("w.js"), Arg::StrDict(vec![("name", "fib")])])
        .expect("a worker");
    let origin = url::Url::parse("https://example.test/").unwrap().origin();
    assert_eq!(
        calls(&dom),
        [WorkerCall::Start(WorkerStart {
            url: url::Url::parse("https://example.test/dir/w.js").unwrap(),
            name: "fib".to_owned(),
            owner_origin: Some(origin),
        })]
    );
    dom.read(|cx| {
        let ObjectRef::EventTarget(target) = worker.get(cx.gc) else {
            panic!("a target");
        };
        assert!(cx.is_worker(target));
        assert_eq!(cx.worker_id(target), 1);
        assert_eq!(
            dom_api::interface_of(&dom_api::ApiCx::new(cx), worker.get(cx.gc)),
            const { dom_api::declare::interface_index("Worker") }
        );
    });
    // No name: an empty one.
    construct(&mut dom, &[Arg::Str("https://example.test/other.js")]).unwrap();
    assert!(matches!(&calls(&dom)[..], [WorkerCall::Start(start)] if start.name.is_empty()));
}

/// A URL that doesn't parse is a `SyntaxError`, `type: "module"` a
/// `NotSupportedError`, a `type` of neither WebIDL's `TypeError`, a realm
/// whose engine has no workers' host a `NotSupportedError`: none starts a
/// worker.
#[test]
fn a_worker_that_can_t_start_throws() {
    let mut dom = page(true);
    assert_eq!(
        dom_error(construct(&mut dom, &[Arg::Str("https://[")])),
        ExceptionName::Syntax
    );
    assert_eq!(
        dom_error(construct(
            &mut dom,
            &[Arg::Str("w.js"), Arg::StrDict(vec![("type", "module")])]
        )),
        ExceptionName::NotSupported
    );
    assert!(matches!(
        construct(&mut dom, &[Arg::Str("w.js"), Arg::StrDict(vec![("type", "wasm")])]),
        Err(ApiError::Type(_))
    ));
    // `classic`, given: it starts.
    construct(&mut dom, &[Arg::Str("w.js"), Arg::StrDict(vec![("type", "classic")])]).unwrap();
    assert_eq!(calls(&dom).len(), 1);
    let mut unhosted = page(false);
    assert_eq!(
        dom_error(construct(&mut unhosted, &[Arg::Str("w.js")])),
        ExceptionName::NotSupported
    );
    assert!(calls(&unhosted).is_empty());
}

/// `postMessage()` gives the host the message for its worker; a port in
/// the transfer list is a `DataCloneError`; `terminate()` terminates it
/// once, and nothing is posted after.
#[test]
fn a_worker_posts_and_terminates_through_its_host() {
    let mut dom = page(true);
    let worker = construct(&mut dom, &[Arg::Str("w.js")]).unwrap();
    calls(&dom);
    call(&mut dom, &worker, "Worker", "postMessage", &[Arg::Str("hi")]).unwrap();
    assert_eq!(calls(&dom), [WorkerCall::Post(1, b"hi".to_vec())]);

    let port = dom.read(|cx| {
        let channel = cx.create_message_channel(None);
        Rc::new(cx.root_object(ObjectRef::EventTarget(cx.message_channel_ports(channel).0)))
    });
    let transferred = call(
        &mut dom,
        &worker,
        "Worker",
        "postMessage",
        &[Arg::Str("port"), Arg::Object(port)],
    );
    assert!(matches!(transferred, Err(ApiError::Dom(e)) if e.name == ExceptionName::DataClone));
    assert!(calls(&dom).is_empty());

    call(&mut dom, &worker, "Worker", "terminate", &[]).unwrap();
    call(&mut dom, &worker, "Worker", "terminate", &[]).unwrap();
    call(&mut dom, &worker, "Worker", "postMessage", &[Arg::Str("late")]).unwrap();
    assert_eq!(calls(&dom), [WorkerCall::Terminate(1)]);
}

/// A worker's global, the realm's, named `fib`.
fn worker_global(hosted: bool) -> (TestDom, Rc<ObjectRoot>) {
    let dom = TestDom::new();
    let (global, root) = dom.read(|cx| {
        let global = cx.create_worker_global(
            url::Url::parse("https://example.test/w.js").unwrap(),
            url::Url::parse("https://example.test/").unwrap().origin(),
            dom::DomString::new("fib"),
        );
        (
            Persistent::new(cx.gc, global),
            Rc::new(cx.root_object(ObjectRef::WorkerGlobal(global))),
        )
    });
    *dom.1.current_worker.borrow_mut() = Some(global);
    dom.1.workers.borrow_mut().hosted = hosted;
    (dom, root)
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

/// Its timers are its execution context's: set there, cleared there.
#[test]
fn a_worker_s_global_has_its_own_timers() {
    let (mut dom, global) = worker_global(true);
    let id = match call(
        &mut dom,
        &global,
        "WorkerGlobalScope",
        "setTimeout",
        &[Arg::Callback(1), Arg::Number(10.0)],
    )
    .unwrap()
    {
        Ret::F64(id) => id,
        other => panic!("{other:?}"),
    };
    let due = |dom: &mut TestDom| {
        dom.read(|cx| match global.get(cx.gc) {
            ObjectRef::WorkerGlobal(global) => cx.next_due(cx.worker_global_context(global)),
            _ => unreachable!(),
        })
    };
    assert!(due(&mut dom).is_some());
    call(&mut dom, &global, "WorkerGlobalScope", "clearTimeout", &[Arg::Number(id)]).unwrap();
    assert!(due(&mut dom).is_none());
    call(
        &mut dom,
        &global,
        "WorkerGlobalScope",
        "setInterval",
        &[Arg::Callback(1), Arg::Number(10.0)],
    )
    .unwrap();
    assert!(due(&mut dom).is_some());
    call(&mut dom, &global, "WorkerGlobalScope", "clearInterval", &[Arg::Number(2.0)]).unwrap();
    assert!(due(&mut dom).is_none());
}

/// `name` is the one it was made with, `self` itself, `crypto` its
/// context's (the same each time); `postMessage()` and `close()` go to the
/// host for its `Worker`, a port in the transfer list a `DataCloneError`.
#[test]
fn a_worker_s_global_s_own_members() {
    let (mut dom, global) = worker_global(true);
    assert!(matches!(get(&mut dom, &global, "DedicatedWorkerGlobalScope", "name"), Ret::Str(name) if name.to_string() == "fib"));
    let Ret::Object(this) = get(&mut dom, &global, "WorkerGlobalScope", "self") else {
        panic!("an object");
    };
    dom.read(|cx| assert!(elise::Gc::ptr_eq(this.get(cx.gc).erase(), global.get(cx.gc).erase())));
    let crypto = |dom: &mut TestDom| match get(dom, &global, "WorkerGlobalScope", "crypto") {
        Ret::Object(crypto) => crypto,
        other => panic!("{other:?}"),
    };
    let (one, two) = (crypto(&mut dom), crypto(&mut dom));
    dom.read(|cx| assert!(elise::Gc::ptr_eq(one.get(cx.gc).erase(), two.get(cx.gc).erase())));
    dom.read(|cx| {
        assert_eq!(
            dom_api::interface_of(&dom_api::ApiCx::new(cx), global.get(cx.gc)),
            const { dom_api::declare::interface_index("DedicatedWorkerGlobalScope") }
        );
    });

    call(&mut dom, &global, "DedicatedWorkerGlobalScope", "postMessage", &[Arg::Str("done")]).unwrap();
    let port = dom.read(|cx| {
        let channel = cx.create_message_channel(None);
        Rc::new(cx.root_object(ObjectRef::EventTarget(cx.message_channel_ports(channel).0)))
    });
    let transferred = call(
        &mut dom,
        &global,
        "DedicatedWorkerGlobalScope",
        "postMessage",
        &[Arg::Str("port"), Arg::Object(port)],
    );
    assert!(matches!(transferred, Err(ApiError::Dom(e)) if e.name == ExceptionName::DataClone));
    call(&mut dom, &global, "DedicatedWorkerGlobalScope", "close", &[]).unwrap();
    assert_eq!(
        calls(&dom),
        [WorkerCall::PostToOwner(b"done".to_vec()), WorkerCall::Close]
    );
}
