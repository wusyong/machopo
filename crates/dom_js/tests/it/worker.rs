//! A dedicated worker's engine (`V8WorkerEngine`): its realm's global a
//! worker's global, with the interfaces a dedicated worker has (and none a
//! window alone has), its members the global's own; its timers and the
//! messages its `Worker` posts are its tasks; what it posts, `close()` and
//! what it throws that its `onerror` doesn't cancel go to its host; its
//! script stops from another thread; `importScripts()` runs the scripts
//! its host fetches, in turn. And a window's side (`V8Engine`): a
//! `Worker` starts, posts and terminates through the window's workers'
//! host; what its worker sends comes to it as its realm's tasks.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dom_api::{
    ClassicScript, EngineSettings, ScriptEngine, WorkerEngine, WorkerError, WorkerGlobalHost,
    WorkerHost, WorkerStart,
};
use dom_js::{V8Engine, V8WorkerEngine};

/// What the worker sent its host, and what it fetched of it.
#[derive(Default)]
struct Sent {
    messages: Vec<dom::SerializedMessage>,
    closed: bool,
    errors: Vec<dom_api::WorkerError>,
    /// The scripts its host has, by URL; another can't be had.
    scripts: HashMap<String, dom_api::ImportedScript>,
    /// The URLs it fetched, in order.
    fetched: Vec<String>,
}

struct Host(Rc<RefCell<Sent>>);

impl WorkerGlobalHost for Host {
    fn post_to_owner(&self, message: dom::SerializedMessage) {
        self.0.borrow_mut().messages.push(message);
    }

    fn close(&self) {
        self.0.borrow_mut().closed = true;
    }

    fn report_error(&self, error: dom_api::WorkerError) {
        self.0.borrow_mut().errors.push(error);
    }

    fn fetch_script(&self, url: &url::Url) -> Option<dom_api::ImportedScript> {
        let mut sent = self.0.borrow_mut();
        sent.fetched.push(url.to_string());
        sent.scripts.get(url.as_str()).cloned()
    }
}

/// `source` is the script at `url` (of muted errors if `muted`).
fn script(sent: &Rc<RefCell<Sent>>, url: &str, source: &str, muted: bool) {
    sent.borrow_mut().scripts.insert(
        url.to_owned(),
        dom_api::ImportedScript {
            source: source.to_owned(),
            url: url.to_owned(),
            muted,
        },
    );
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

/// What its script throws and its `onerror` doesn't cancel goes to its
/// host, for its `Worker`; what it cancels doesn't.
#[test]
fn a_worker_s_uncaught_errors_go_to_its_host() {
    let (mut engine, sent) = worker();
    run(
        &mut engine,
        "onerror = (message) => message.includes('canceled'); \
         setTimeout(() => { throw new Error('canceled'); }, 0); \
         throw new TypeError('reported');",
    );
    run_tasks_until(&mut engine, |engine| engine.until_next_task().is_none());
    assert_eq!(
        sent.borrow().errors,
        [WorkerError {
            message: "Uncaught TypeError: reported".to_owned(),
            filename: "https://example.test/w.js".to_owned(),
            lineno: 1,
            colno: 109,
        }]
    );
}

/// `importScripts()` runs what its host fetched, in turn, in its realm,
/// each URL parsed against its own first (none with nothing given).
#[test]
fn import_scripts_runs_each_in_turn() {
    let (mut engine, sent) = worker();
    script(&sent, "https://example.test/a.js", "var got = ['a'];", false);
    script(&sent, "https://example.test/lib/b.js", "got.push('b', typeof self);", false);
    assert_eq!(
        result(
            &mut engine,
            "(importScripts(), importScripts('a.js', '/lib/b.js'), got.join())"
        ),
        "a,b,object"
    );
    assert_eq!(
        sent.borrow().fetched,
        ["https://example.test/a.js", "https://example.test/lib/b.js"]
    );
}

/// A URL that doesn't parse is a `SyntaxError`, before any is fetched; a
/// script that can't be had a `NetworkError`, and those after it aren't
/// fetched.
#[test]
fn import_scripts_fails_as_html_says() {
    let (mut engine, sent) = worker();
    script(&sent, "https://example.test/a.js", "var a = 1;", false);
    assert_eq!(
        result(&mut engine, "importScripts('a.js', 'https://[')"),
        "threw SyntaxError: Failed to execute 'importScripts' on 'WorkerGlobalScope': \
         The URL 'https://[' is invalid."
    );
    assert!(sent.borrow().fetched.is_empty());
    assert_eq!(
        result(
            &mut engine,
            "(() => { try { importScripts('gone.js', 'a.js'); } \
               catch (e) { return e.name + ' ' + e.code + ' ' + (e instanceof DOMException) \
                 + ' ' + typeof a; } })()"
        ),
        "NetworkError 19 true undefined"
    );
    assert_eq!(sent.borrow().fetched, ["https://example.test/gone.js"]);
}

/// What a script throws is what `importScripts()` throws, the very value,
/// and those after it don't run; a syntax error too. One of muted errors
/// (another origin's) throws a `NetworkError` instead.
#[test]
fn import_scripts_throws_what_a_script_threw() {
    let (mut engine, sent) = worker();
    script(&sent, "https://example.test/throws.js", "throw self;", false);
    script(&sent, "https://example.test/bad.js", "(", false);
    script(&sent, "https://example.test/after.js", "var after = 1;", false);
    script(&sent, "https://other.test/muted.js", "throw self;", true);
    assert_eq!(
        result(
            &mut engine,
            "(() => { try { importScripts('throws.js', 'after.js'); } \
               catch (e) { return (e === self) + ' ' + typeof after; } })()"
        ),
        "true undefined"
    );
    assert_eq!(
        result(
            &mut engine,
            "(() => { try { importScripts('bad.js'); } catch (e) { return e.name; } })()"
        ),
        "SyntaxError"
    );
    assert_eq!(
        result(
            &mut engine,
            "(() => { try { importScripts('https://other.test/muted.js'); } \
               catch (e) { return e.name; } })()"
        ),
        "NetworkError"
    );
    // Nothing reported: each was thrown to the caller.
    assert!(sent.borrow().errors.is_empty());
}

/// What a function an imported script made throws later is reported as
/// that script's: its message and URL, unless its errors are muted (another
/// origin's): then `Script error.` and nothing of where.
#[test]
fn an_imported_script_s_errors_are_its_own() {
    let (mut engine, sent) = worker();
    let boom = "var booms = (self.booms || []).concat(() => { throw new Error('boom'); });";
    script(&sent, "https://example.test/own.js", boom, false);
    script(&sent, "https://other.test/muted.js", boom, true);
    run(
        &mut engine,
        "importScripts('own.js', 'https://other.test/muted.js'); \
         for (const boom of booms) setTimeout(boom, 0);",
    );
    run_tasks_until(&mut engine, |engine| engine.until_next_task().is_none());
    let errors: Vec<_> = sent
        .borrow()
        .errors
        .iter()
        .map(|error| format!("{} {}", error.message, error.filename))
        .collect();
    assert_eq!(
        errors,
        ["Uncaught Error: boom https://example.test/own.js", "Script error. "]
    );
}

/// What a window's workers' host was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Asked {
    Start(WorkerStart),
    Post(u64, Vec<u8>),
    Terminate(u64),
}

/// A window's workers' host: it numbers its workers from 1, and keeps the
/// messages posted to them whole (`posted`), to give back as theirs.
#[derive(Default)]
struct Workers {
    asked: RefCell<Vec<Asked>>,
    posted: RefCell<Vec<dom::SerializedMessage>>,
}

struct OwnerHost(Rc<Workers>);

impl WorkerHost for OwnerHost {
    fn start_worker(&self, start: WorkerStart) -> u64 {
        let mut asked = self.0.asked.borrow_mut();
        asked.push(Asked::Start(start));
        u64::try_from(
            asked
                .iter()
                .filter(|asked| matches!(asked, Asked::Start(_)))
                .count(),
        )
        .unwrap()
    }

    fn post_to_worker(&self, id: u64, message: dom::SerializedMessage) {
        self.0
            .asked
            .borrow_mut()
            .push(Asked::Post(id, message.bytes.clone()));
        self.0.posted.borrow_mut().push(message);
    }

    fn terminate_worker(&self, id: u64) {
        self.0.asked.borrow_mut().push(Asked::Terminate(id));
    }
}

/// A window's engine at `https://example.test/page.html`, with a workers'
/// host.
fn page() -> (V8Engine, Rc<Workers>) {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    let workers = Rc::new(Workers::default());
    engine.set_worker_host(Box::new(OwnerHost(workers.clone())));
    (engine, workers)
}

fn page_result(engine: &mut V8Engine, source: &str) -> String {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source: &format!(
                "try {{ testRunner.reportHarnessResults(String({source})); }} \
                 catch (e) {{ testRunner.reportHarnessResults('threw ' + e); }}"
            ),
            url: "https://example.test/page.html",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
    engine.take_harness_results().expect("a result")
}

/// `new Worker()` starts its worker through the window's host; what the
/// worker posts is a `message` at the `Worker` as its realm's task, `null`
/// its source and empty its origin; what it threw an `error` at the
/// `Worker`, and at the window if that isn't canceled.
#[test]
fn a_window_s_worker_hears_from_its_worker() {
    let (mut engine, workers) = page();
    assert_eq!(
        page_result(
            &mut engine,
            "(() => { globalThis.got = []; \
               globalThis.worker = new Worker('w.js', { name: 'fib' }); \
               worker.onmessage = e => got.push('message ' + e.data.n + ' ' + (e.source === null) + ' ' + JSON.stringify(e.origin)); \
               worker.onerror = e => { got.push('worker error ' + e.message + ' ' + e.lineno + ' ' + e.error); \
                 if (e.message.includes('handled')) e.preventDefault(); }; \
               onerror = message => got.push('window error ' + message); \
               worker.postMessage({ n: 40 }); return typeof worker; })()"
        ),
        "object"
    );
    let origin = url::Url::parse("https://example.test/").unwrap().origin();
    assert!(matches!(
        &workers.asked.borrow()[..],
        [Asked::Start(start), Asked::Post(1, _)]
            if start.url.as_str() == "https://example.test/w.js"
                && start.name == "fib"
                && start.owner_origin == Some(origin)
    ));
    // What it posted comes back as its worker's.
    let message = workers.posted.borrow_mut().pop().unwrap();
    engine.worker_message(1, message);
    engine.run_due_tasks();
    for (message, lineno) in [("Uncaught Error: handled", 2), ("Uncaught Error: boom", 3)] {
        engine.worker_error(
            1,
            &WorkerError {
                message: message.to_owned(),
                filename: "https://example.test/w.js".to_owned(),
                lineno,
                colno: 7,
            },
        );
    }
    // A worker that isn't the window's: nothing.
    engine.worker_error(
        2,
        &WorkerError {
            message: "elsewhere".to_owned(),
            filename: String::new(),
            lineno: 0,
            colno: 0,
        },
    );
    assert_eq!(
        page_result(&mut engine, "got.join('|')"),
        "message 40 true \"\"|worker error Uncaught Error: handled 2 null|\
         worker error Uncaught Error: boom 3 null|window error Uncaught Error: boom"
    );
}

/// After `terminate()`, its host terminates its worker once, nothing more
/// is posted to it, and nothing of it comes to the `Worker`, even what it
/// sent before (HTML's: its port's queue emptied); after its worker closed
/// itself, nothing comes either.
#[test]
fn a_terminated_worker_is_heard_no_more() {
    let (mut engine, workers) = page();
    page_result(
        &mut engine,
        "(() => { globalThis.got = []; \
           globalThis.a = new Worker('a.js'); a.onmessage = e => got.push('a ' + e.data); \
           globalThis.b = new Worker('b.js'); b.onmessage = e => got.push('b ' + e.data); \
           a.postMessage('x'); b.postMessage('y'); })()",
    );
    let y = workers.posted.borrow_mut().pop().unwrap();
    let x = workers.posted.borrow_mut().pop().unwrap();
    // `a`'s message queued, then `a` terminated before it ran.
    engine.worker_message(1, x);
    page_result(
        &mut engine,
        "(a.terminate(), a.terminate(), a.postMessage('late'))",
    );
    engine.run_due_tasks();
    // `b` closed itself: its later message is no more its `Worker`'s.
    engine.worker_closed(2);
    engine.worker_message(2, y);
    engine.worker_error(
        2,
        &WorkerError {
            message: "late".to_owned(),
            filename: String::new(),
            lineno: 0,
            colno: 0,
        },
    );
    engine.run_due_tasks();
    assert_eq!(page_result(&mut engine, "got.length"), "0");
    assert_eq!(
        workers
            .asked
            .borrow()
            .iter()
            .filter(|asked| !matches!(asked, Asked::Start(_)))
            .cloned()
            .collect::<Vec<_>>()
            .iter()
            .map(|asked| match asked {
                Asked::Post(id, _) => format!("post {id}"),
                Asked::Terminate(id) => format!("terminate {id}"),
                Asked::Start(_) => unreachable!(),
            })
            .collect::<Vec<_>>(),
        ["post 1", "post 2", "terminate 1"]
    );
}

/// A document's workers end with it: an iframe's as it is removed, a
/// window's as its frame gets another document.
#[test]
fn a_document_s_workers_end_with_it() {
    let (mut engine, workers) = page();
    page_result(
        &mut engine,
        "(() => { document.append(document.createElement('html')); \
           const f = document.createElement('iframe'); document.documentElement.append(f); \
           globalThis.inner = new f.contentWindow.Worker('inner.js'); \
           globalThis.outer = new Worker('outer.js'); f.remove(); })()",
    );
    let ended = |workers: &Workers| {
        workers
            .asked
            .borrow()
            .iter()
            .filter_map(|asked| match asked {
                Asked::Terminate(id) => Some(*id),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    // The iframe's (the first made) ended; the window's runs on.
    assert_eq!(ended(&workers), [1]);
    engine
        .create_window(engine.top_frame(), "https://example.test/next.html", Instant::now())
        .expect("a window");
    assert_eq!(ended(&workers), [1, 2]);
}
