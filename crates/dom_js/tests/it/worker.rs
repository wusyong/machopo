//! A dedicated worker's engine (`V8WorkerEngine`): its realm's global a
//! worker's global, with the interfaces a dedicated worker has (and none a
//! window alone has), its members the global's own; its timers and the
//! messages its `Worker` posts are its tasks; what it posts and `close()`
//! go to its host; what it throws reaches its `onerror`; its script stops
//! from another thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dom_api::{ClassicScript, EngineSettings, WorkerEngine, WorkerGlobalHost};
use dom_js::V8WorkerEngine;

/// What the worker sent its host.
#[derive(Default)]
struct Sent {
    messages: Vec<dom::SerializedMessage>,
    closed: bool,
}

struct Host(Rc<RefCell<Sent>>);

impl WorkerGlobalHost for Host {
    fn post_to_owner(&self, message: dom::SerializedMessage) {
        self.0.borrow_mut().messages.push(message);
    }

    fn close(&self) {
        self.0.borrow_mut().closed = true;
    }
}

/// A worker's engine, its global of `https://example.test/w.js`, named
/// `fib`, and what it sends.
fn worker() -> (V8WorkerEngine, Rc<RefCell<Sent>>) {
    let mut engine = V8WorkerEngine::new(EngineSettings { web_tests: true });
    let sent = Rc::new(RefCell::new(Sent::default()));
    let url = url::Url::parse("https://example.test/w.js").unwrap();
    engine.create_global(&url, url.origin(), "fib", Box::new(Host(sent.clone())));
    (engine, sent)
}

fn run(engine: &mut V8WorkerEngine, source: &str) {
    engine.run_classic_script(&ClassicScript {
        source,
        url: "https://example.test/w.js",
        base_url: None,
        line: 1,
        muted: false,
        use_credentials: false,
    });
}

fn result(engine: &mut V8WorkerEngine, source: &str) -> String {
    // What it throws reported instead.
    run(
        engine,
        &format!(
            "try {{ testRunner.reportHarnessResults(String({source})); }} \
             catch (e) {{ testRunner.reportHarnessResults('threw ' + e); }}"
        ),
    );
    engine.take_harness_results().expect("a result")
}

/// Its tasks run until `done`, or a second passes.
fn run_tasks_until(engine: &mut V8WorkerEngine, mut done: impl FnMut(&mut V8WorkerEngine) -> bool) {
    let until = Instant::now() + Duration::from_secs(1);
    while !done(engine) {
        assert!(Instant::now() < until, "its tasks ran in time");
        if !engine.run_due_tasks() {
            std::thread::sleep(engine.until_next_task().unwrap_or(Duration::from_millis(1)));
        }
    }
}

/// `self` is the global, a `DedicatedWorkerGlobalScope` and a
/// `WorkerGlobalScope`; their members are its own properties (WebIDL's
/// `[Global]`); it has no `window`, `document`, `Image` or `Node`; what is
/// made of its realm is of its execution context (`AbortSignal.any()`).
#[test]
fn a_worker_s_global_is_its_realm_s() {
    let (mut engine, _) = worker();
    assert_eq!(
        result(
            &mut engine,
            "[self === globalThis, self instanceof DedicatedWorkerGlobalScope, \
             self instanceof WorkerGlobalScope, self instanceof EventTarget, name, \
             Object.hasOwn(self, 'postMessage'), Object.hasOwn(self, 'setTimeout'), \
             Object.hasOwn(self, 'self'), typeof queueMicrotask, typeof window, \
             typeof document, typeof Image, typeof Node, typeof NodeFilter, \
             AbortSignal.any([]).aborted].join()"
        ),
        "true,true,true,true,fib,true,true,true,function,undefined,undefined,undefined,undefined,undefined,false"
    );
}

/// A microtask its script queued runs as the script ends (HTML's "clean
/// up after running script"), before its next.
#[test]
fn a_worker_s_microtasks_run_after_its_script() {
    let (mut engine, _) = worker();
    run(
        &mut engine,
        "globalThis.ran = []; queueMicrotask(() => ran.push('microtask')); \
         Promise.resolve().then(() => ran.push('then'));",
    );
    assert_eq!(result(&mut engine, "ran.join()"), "microtask,then");
}

/// Each of the table's interfaces is there if a dedicated worker has it,
/// and isn't if not (a window's alone), as are `DOMException`'s.
#[test]
fn a_worker_s_realm_has_the_interfaces_exposed_there() {
    let (mut engine, _) = worker();
    let names: Vec<String> = dom_api::INTERFACES
        .iter()
        .filter(|desc| !desc.namespace)
        .map(|desc| format!("[{:?}, {}]", desc.name, desc.exposed.in_dedicated_worker()))
        .collect();
    let wrong = result(
        &mut engine,
        &format!(
            "[{}, ['DOMException', true]].filter(([name, there]) => \
             (typeof globalThis[name] === 'function') !== there).map(([name]) => name).join()",
            names.join(", ")
        ),
    );
    assert_eq!(wrong, "");
}

/// A timer is its task: a timeout runs once, an interval again till
/// cleared, `this` the global.
#[test]
fn a_worker_s_timers_are_its_tasks() {
    let (mut engine, _) = worker();
    run(
        &mut engine,
        "globalThis.log = []; \
         setTimeout(function () { log.push('timeout ' + (this === self)); }, 0); \
         const id = setInterval(() => { log.push('interval'); \
           if (log.filter(entry => entry === 'interval').length === 3) clearInterval(id); }, 1);",
    );
    assert!(engine.until_next_task().is_some());
    run_tasks_until(&mut engine, |engine| result(engine, "log.length") == "4");
    assert_eq!(
        result(&mut engine, "log.join()"),
        "timeout true,interval,interval,interval"
    );
    // Cleared: none is due.
    assert!(engine.until_next_task().is_none());
}

/// `postMessage()` gives its host the message, serialized; a message its
/// `Worker` posts is a `message` event at the global (its `onmessage` and
/// its listeners), of no source and no origin; `close()` tells the host.
#[test]
fn a_worker_s_messages_go_both_ways() {
    let (mut engine, sent) = worker();
    run(
        &mut engine,
        "globalThis.got = []; \
         onmessage = e => got.push('onmessage ' + e.data.n + ' ' + (e.source === null) + ' ' + JSON.stringify(e.origin)); \
         addEventListener('message', e => got.push('listener ' + (e.target === self))); \
         postMessage({ n: 40 });",
    );
    let message = sent.borrow_mut().messages.pop().expect("its message");
    assert!(sent.borrow().messages.is_empty());
    // The message it sent comes back to it as one from its `Worker`.
    engine.post_message(message);
    assert_eq!(engine.until_next_task(), Some(Duration::ZERO));
    run_tasks_until(&mut engine, |engine| result(engine, "got.length") == "2");
    assert_eq!(
        result(&mut engine, "got.join()"),
        "onmessage 40 true \"\",listener true"
    );
    run(&mut engine, "close()");
    assert!(sent.borrow().closed);
}

/// What its script throws reaches its `onerror` with the error's parts (a
/// global's `OnErrorEventHandler`), and returning `true` cancels it; a
/// timer's too.
#[test]
fn a_worker_s_errors_reach_its_onerror() {
    let (mut engine, _) = worker();
    run(
        &mut engine,
        "globalThis.errors = []; \
         onerror = (message, url, line, column, error) => { \
           errors.push([message, url, line, error.message].join(' ')); return true; }; \
         setTimeout(() => { throw new Error('later'); }, 0); \
         throw new Error('boom');",
    );
    run_tasks_until(&mut engine, |engine| result(engine, "errors.length") == "2");
    assert_eq!(
        result(&mut engine, "errors.join('|')"),
        "Uncaught Error: boom https://example.test/w.js 1 boom|Uncaught Error: later https://example.test/w.js 1 later"
    );
}

/// `new Worker()` in a worker throws: its engine has no workers' host (no
/// nested workers).
#[test]
fn a_worker_makes_no_worker() {
    let (mut engine, _) = worker();
    assert_eq!(
        result(
            &mut engine,
            "(() => { try { new Worker('w.js'); return 'made'; } catch (e) { return e.name; } })()"
        ),
        "NotSupportedError"
    );
}

/// Its script stops when its terminator is called from another thread.
#[test]
fn a_worker_s_script_stops_from_another_thread() {
    let (terminators, terminator) = std::sync::mpsc::channel();
    let (ran, done) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (mut engine, _) = worker();
        terminators.send(engine.terminator()).unwrap();
        run(&mut engine, "for (;;);");
        ran.send(()).unwrap();
    });
    let terminator = terminator.recv().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    terminator();
    done.recv_timeout(Duration::from_secs(10))
        .expect("its script stopped");
    thread.join().unwrap();
}
