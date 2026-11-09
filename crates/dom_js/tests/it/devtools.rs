//! The engine's inspector (`ScriptEngine`'s DevTools calls): each frame's
//! realm told as it is made, for the frame DevTools calls it by, with its
//! document's origin, and as it goes (its frame navigated to another realm
//! or gone, all at once as the top-level frame navigates); the exceptions
//! reported and the rejections unhandled told, a handler added later
//! striking one out; script paused takes the host's DevTools queue alone,
//! and what interrupts script is taken from it as script runs.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Instant;

use dom_api::{
    ClassicScript, DevToolsQueue, DevToolsQueued, EngineSettings, FrameId, FrameRequest,
    ScriptEngine,
};
use dom_js::V8Engine;
use serde_json::{Value, json};

/// An engine with a DevTools session, `"s"`, whose messages `sent` has.
struct Inspected {
    engine: V8Engine,
    sent: Rc<RefCell<Vec<Value>>>,
    next_id: u64,
}

impl Inspected {
    /// Its frames named `top`, and `child <n>`.
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings::default());
        let top = engine.top_frame();
        engine.set_devtools_frame_ids(Box::new(move |frame: FrameId| {
            Some(if frame == top {
                "top".to_owned()
            } else {
                format!("child {}", frame.0)
            })
        }));
        let sent: Rc<RefCell<Vec<Value>>> = Rc::default();
        let to = sent.clone();
        engine.devtools_attach(
            "s".to_owned(),
            Box::new(move |message| {
                to.borrow_mut()
                    .push(serde_json::from_str(&message).unwrap())
            }),
        );
        Self {
            engine,
            sent,
            next_id: 0,
        }
    }

    /// Sends `method` with `params`: its answer.
    fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let command = json!({ "id": id, "method": method, "params": params });
        assert!(
            self.engine
                .dispatch_devtools_message("s", &command.to_string())
        );
        let sent = self.sent.borrow();
        sent.iter()
            .find(|message| message["id"] == id)
            .unwrap_or_else(|| panic!("{method} unanswered: {sent:?}"))
            .clone()
    }

    /// The events sent since the last call, taken: their methods and
    /// params.
    fn events(&mut self) -> Vec<(String, Value)> {
        self.sent
            .borrow_mut()
            .drain(..)
            .filter_map(|message| {
                Some((
                    message["method"].as_str()?.to_owned(),
                    message["params"].clone(),
                ))
            })
            .collect()
    }

    fn run(&mut self, frame: FrameId, source: &str) {
        self.engine.run_classic_script(
            frame,
            &ClassicScript {
                source,
                url: "https://example.test/a/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    }
}

/// The contexts told created (their frames, origins and ids) and
/// destroyed (their ids), and all of them cleared, in `events`.
fn contexts(events: &[(String, Value)]) -> Vec<String> {
    events
        .iter()
        .filter_map(|(method, params)| match method.as_str() {
            "Runtime.executionContextCreated" => {
                let context = &params["context"];
                Some(format!(
                    "created {} {} {}",
                    context["auxData"]["frameId"].as_str().unwrap_or("none"),
                    context["origin"].as_str().unwrap(),
                    context["id"]
                ))
            }
            "Runtime.executionContextDestroyed" => {
                Some(format!("destroyed {}", params["executionContextId"]))
            }
            "Runtime.executionContextsCleared" => Some("cleared".to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn each_frames_realm_is_told_as_it_comes_and_goes() {
    let mut page = Inspected::new();
    page.call("Runtime.enable", json!({}));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    assert_eq!(
        contexts(&page.events()),
        ["created top https://example.test 1"]
    );

    // An iframe's initial `about:blank`, of its parent's origin.
    page.run(
        top,
        "document.appendChild(document.createElement('html'));
         document.documentElement.appendChild(document.createElement('iframe'));",
    );
    let child = match page.engine.take_frame_requests().as_slice() {
        [FrameRequest::Created { frame, .. }] => *frame,
        requests => panic!("{requests:?}"),
    };
    assert_eq!(
        contexts(&page.events()),
        [format!("created child {} https://example.test 2", child.0)]
    );
    // Its document of another origin, in a realm of its own: the one
    // before has gone. An opaque origin is "://", as Chrome gives it.
    page.engine
        .create_window(child, "data:text/html,x", Instant::now())
        .unwrap();
    assert_eq!(
        contexts(&page.events()),
        [
            "destroyed 2".to_owned(),
            format!("created child {} :// 3", child.0)
        ]
    );
    // The iframe gone takes its realm.
    page.run(top, "document.querySelector('iframe').remove();");
    assert_eq!(contexts(&page.events()), ["destroyed 3"]);

    // A command that names no context runs in the top-level frame's.
    let answer = page.call(
        "Runtime.evaluate",
        json!({ "expression": "location.href", "returnByValue": true }),
    );
    assert_eq!(
        answer["result"]["result"]["value"], "https://example.test/a/",
        "{answer}"
    );

    // The top-level frame navigates: every realm goes at once, before
    // the new one comes.
    page.engine
        .create_window(top, "https://other.test/", Instant::now())
        .unwrap();
    assert_eq!(
        contexts(&page.events()),
        ["cleared", "created top https://other.test 4"]
    );
}

/// A child frame that navigates takes the frames of its document before
/// with it: their realms are told gone first, and the host is told to
/// remove them.
#[test]
fn a_child_frames_navigation_takes_its_frames() {
    let mut page = Inspected::new();
    page.call("Runtime.enable", json!({}));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    let iframe = "document.appendChild(document.createElement('html'));
         document.documentElement.appendChild(document.createElement('iframe'));";
    page.run(top, iframe);
    let child = match page.engine.take_frame_requests().as_slice() {
        [FrameRequest::Created { frame, .. }] => *frame,
        requests => panic!("{requests:?}"),
    };
    page.engine
        .create_window(child, "https://other.test/", Instant::now())
        .unwrap();
    page.run(child, iframe);
    let grandchild = match page.engine.take_frame_requests().as_slice() {
        [FrameRequest::Created { frame, .. }] => *frame,
        requests => panic!("{requests:?}"),
    };
    page.events();
    page.engine
        .create_window(child, "https://third.test/", Instant::now())
        .unwrap();
    assert_eq!(
        contexts(&page.events()),
        [
            "destroyed 4".to_owned(),
            "destroyed 3".to_owned(),
            format!("created child {} https://third.test 5", child.0)
        ]
    );
    assert!(
        page.engine.take_frame_requests().iter().any(
            |request| matches!(request, FrameRequest::Removed { frame } if *frame == grandchild)
        ),
        "the host removes the grandchild's frame"
    );
}

/// An initial `about:blank` that stays, its window reused by its first
/// document of its origin, keeps its realm: nothing is told.
#[test]
fn a_realm_kept_by_its_frames_next_document_is_told_once() {
    let mut page = Inspected::new();
    page.call("Runtime.enable", json!({}));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    page.run(
        top,
        "document.appendChild(document.createElement('html'));
         document.documentElement.appendChild(document.createElement('iframe'));",
    );
    let child = match page.engine.take_frame_requests().as_slice() {
        [FrameRequest::Created { frame, .. }] => *frame,
        requests => panic!("{requests:?}"),
    };
    page.events();
    page.engine
        .create_window(child, "https://example.test/b", Instant::now())
        .unwrap();
    assert_eq!(contexts(&page.events()), Vec::<String>::new());
}

#[test]
fn uncaught_exceptions_and_rejections_are_told() {
    let mut page = Inspected::new();
    page.call("Runtime.enable", json!({}));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    page.events();
    page.run(top, "\n  throw new Error('boom');");
    let thrown = page
        .events()
        .into_iter()
        .find(|(method, _)| method == "Runtime.exceptionThrown")
        .expect("told")
        .1["exceptionDetails"]
        .clone();
    assert_eq!(thrown["text"], "Uncaught", "{thrown}");
    assert_eq!(thrown["url"], "https://example.test/a/", "{thrown}");
    // 0-based: where the error was made.
    assert_eq!(
        (&thrown["lineNumber"], &thrown["columnNumber"]),
        (&json!(1), &json!(8)),
        "{thrown}"
    );
    assert!(
        thrown["exception"]["description"]
            .as_str()
            .is_some_and(|description| description.starts_with("Error: boom")),
        "{thrown}"
    );

    // A listener that cancels the error event keeps it from DevTools.
    page.run(
        top,
        "addEventListener('error', (e) => e.preventDefault()); throw new Error('quiet');",
    );
    assert!(
        !page
            .events()
            .iter()
            .any(|(method, _)| method == "Runtime.exceptionThrown")
    );

    page.run(top, "window.p = Promise.reject(new Error('later'));");
    page.engine.perform_microtask_checkpoint();
    let rejected = page
        .events()
        .into_iter()
        .find(|(method, _)| method == "Runtime.exceptionThrown")
        .expect("told")
        .1["exceptionDetails"]
        .clone();
    assert_eq!(rejected["text"], "Uncaught (in promise)", "{rejected}");
    // A handler added later strikes it out.
    page.run(top, "p.catch(() => {});");
    let revoked = page
        .events()
        .into_iter()
        .find(|(method, _)| method == "Runtime.exceptionRevoked")
        .expect("struck out")
        .1;
    assert_eq!(revoked["exceptionId"], rejected["exceptionId"], "{revoked}");
}

/// The host's DevTools queue, as the tests fill it: what script paused
/// takes, what is to interrupt it, and what was put back.
#[derive(Default)]
struct Queue {
    queued: RefCell<VecDeque<DevToolsQueued>>,
    interrupting: RefCell<Vec<(String, String)>>,
    put_back: RefCell<Vec<DevToolsQueued>>,
}

struct SharedQueue(Rc<Queue>);

impl DevToolsQueue for SharedQueue {
    fn next(&self) -> DevToolsQueued {
        self.0
            .queued
            .borrow_mut()
            .pop_front()
            .expect("script paused waits for no more than the test gives")
    }

    fn take_interrupting(&self) -> Vec<(String, String)> {
        std::mem::take(&mut *self.0.interrupting.borrow_mut())
    }

    fn put_back(&self, queued: Vec<DevToolsQueued>) {
        self.0.put_back.borrow_mut().extend(queued);
    }

    fn answer_here(&self, _session: &str, message: &str) -> bool {
        message.contains("Page.")
    }
}

/// The pauses and resumes told in `events`.
fn pauses(events: &[(String, Value)]) -> Vec<&str> {
    events
        .iter()
        .map(|(method, _)| method.as_str())
        .filter(|method| matches!(*method, "Debugger.paused" | "Debugger.resumed"))
        .collect()
}

fn message(id: u64, method: &str) -> DevToolsQueued {
    DevToolsQueued::Message {
        session: "s".to_owned(),
        message: json!({ "id": id, "method": method, "params": {} }).to_string(),
    }
}

/// Script paused takes the queue alone, in order: the inspector's commands
/// (evaluating on the paused frame), the host's own (answered there), an
/// attach and the commands after it waiting for the pause to end, until
/// one resumes it.
#[test]
fn script_paused_takes_the_queue() {
    let mut page = Inspected::new();
    let queue = Rc::new(Queue::default());
    let _interrupter = page
        .engine
        .set_devtools_queue(Box::new(SharedQueue(queue.clone())));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    page.call("Debugger.enable", json!({}));
    let attach = DevToolsQueued::Attach("t".to_owned());
    let waiting = DevToolsQueued::Message {
        session: "t".to_owned(),
        message: r#"{"id":1,"method":"Runtime.enable"}"#.to_owned(),
    };
    queue.queued.borrow_mut().extend([
        message(100, "Page.getFrameTree"),
        attach.clone(),
        waiting.clone(),
        message(101, "Runtime.evaluate"),
        message(102, "Debugger.resume"),
    ]);
    page.events();
    page.run(top, "window.after = 1; debugger; window.after = 2;");
    // The page's own command went to the host; the inspector's were
    // answered.
    {
        let sent = page.sent.borrow();
        assert!(sent.iter().all(|message| message["id"] != 100));
        assert!(sent.iter().any(|message| message["id"] == 101));
    }
    assert_eq!(
        pauses(&page.events()),
        ["Debugger.paused", "Debugger.resumed"]
    );
    assert!(queue.queued.borrow().is_empty());
    assert_eq!(*queue.put_back.borrow(), [attach, waiting]);
    let answer = page.call(
        "Runtime.evaluate",
        json!({ "expression": "after", "returnByValue": true }),
    );
    assert_eq!(answer["result"]["result"]["value"], 2, "{answer}");
}

/// A session detached while script is paused stops debugging: script goes
/// on, and the detach is put back for the host's own agents.
#[test]
fn a_session_detached_while_paused_lets_script_go_on() {
    let mut page = Inspected::new();
    let queue = Rc::new(Queue::default());
    let _interrupter = page
        .engine
        .set_devtools_queue(Box::new(SharedQueue(queue.clone())));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    page.call("Debugger.enable", json!({}));
    let detach = DevToolsQueued::Detach("s".to_owned());
    queue.queued.borrow_mut().push_back(detach.clone());
    page.events();
    page.run(top, "window.after = 1; debugger; window.after = 2;");
    // Told no more once detached.
    assert_eq!(pauses(&page.events()), ["Debugger.paused"]);
    assert_eq!(*queue.put_back.borrow(), [detach]);
    assert!(!page.engine.dispatch_devtools_message("s", "{}"));
    // Script went on, to its end.
    let to = page.sent.clone();
    page.engine.devtools_attach(
        "s".to_owned(),
        Box::new(move |message| {
            to.borrow_mut()
                .push(serde_json::from_str(&message).unwrap())
        }),
    );
    let answer = page.call(
        "Runtime.evaluate",
        json!({ "expression": "after", "returnByValue": true }),
    );
    assert_eq!(answer["result"]["result"]["value"], 2, "{answer}");
}

/// What is to interrupt script is taken as it runs, at its next safe point
/// once the interrupter is called: `Debugger.pause` pauses it there.
#[test]
fn what_interrupts_script_is_taken_as_it_runs() {
    let mut page = Inspected::new();
    let queue = Rc::new(Queue::default());
    let interrupter = page
        .engine
        .set_devtools_queue(Box::new(SharedQueue(queue.clone())));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    page.call("Debugger.enable", json!({}));
    page.events();
    queue.interrupting.borrow_mut().push((
        "s".to_owned(),
        json!({ "id": 200, "method": "Debugger.pause" }).to_string(),
    ));
    queue
        .queued
        .borrow_mut()
        .push_back(message(201, "Debugger.resume"));
    interrupter();
    page.run(top, "let n = 0; for (let i = 0; i < 1000; i++) n += i;");
    assert_eq!(
        pauses(&page.events()),
        ["Debugger.paused", "Debugger.resumed"]
    );
    assert!(queue.interrupting.borrow().is_empty());
}

/// The top function of `stack`, DevTools' `Runtime.StackTrace` as JSON.
fn top_function(stack: Option<Rc<str>>) -> String {
    let stack: Value = serde_json::from_str(&stack.expect("a stack")).unwrap();
    stack["callFrames"][0]["functionName"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// While DevTools wants stacks, the engine captures the stack of the
/// script that asks for each load as the call that asked ends (an image's
/// `src`, a `fetch()`, an iframe that makes a frame), an `import()`'s as it
/// runs; none before it wants them.
#[test]
fn the_stacks_of_script_that_asks_for_loads_are_captured() {
    use dom::{LoadInitiator, StackAsker};
    let mut page = Inspected::new();
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    page.run(
        top,
        "document.append(document.createElement('html'));
         document.documentElement.append(document.createElement('body'));
         function early() { new Image().src = 'early.png'; } early();",
    );
    page.engine.perform_microtask_checkpoint();
    let early = page.engine.dom_mut().take_image_fetches();
    let [
        dom::ImageFetch {
            initiator: LoadInitiator::Element(early),
            ..
        },
    ] = early.as_slice()
    else {
        panic!("{early:?}");
    };
    assert_eq!(
        page.engine
            .dom_mut()
            .take_request_stack(StackAsker::Element(*early)),
        None,
        "not wanted yet"
    );

    page.engine.set_stacks_wanted(true, true);
    page.run(
        top,
        "function addImage() { new Image().src = 'x.png'; }
         function go() { fetch('d.json'); }
         function addFrame() { document.body.append(document.createElement('iframe')); }
         function lazily() { import('./m.js'); }
         addImage(); go(); addFrame(); lazily();",
    );
    page.engine.perform_microtask_checkpoint();
    let dom = page.engine.dom_mut();
    let images = dom.take_image_fetches();
    let [
        dom::ImageFetch {
            initiator: LoadInitiator::Element(image),
            ..
        },
    ] = images.as_slice()
    else {
        panic!("{images:?}");
    };
    assert_eq!(
        top_function(dom.take_request_stack(StackAsker::Element(*image))),
        "addImage"
    );
    let scripts = dom.take_script_fetches();
    let [
        dom::ScriptFetch {
            initiator: LoadInitiator::Script(stack),
            ..
        },
    ] = scripts.as_slice()
    else {
        panic!("{scripts:?}");
    };
    assert_eq!(top_function(Some(stack.clone())), "lazily");
    let requests = page.engine.take_frame_requests();
    let fetch = requests
        .iter()
        .find_map(|request| match request {
            FrameRequest::Fetch { request, .. } => Some(request.id),
            _ => None,
        })
        .expect("the fetch");
    let owner = requests
        .iter()
        .find_map(|request| match request {
            FrameRequest::Created { owner, .. } => Some(owner.clone()),
            _ => None,
        })
        .expect("the frame");
    let dom = page.engine.dom_mut();
    assert_eq!(
        top_function(dom.take_request_stack(StackAsker::Fetch(fetch))),
        "go"
    );
    let owner = dom.read(|cx| cx.node_id(owner.get(cx.gc)));
    assert_eq!(
        top_function(dom.take_request_stack(StackAsker::Element(owner))),
        "addFrame"
    );
    drop(requests);
}
