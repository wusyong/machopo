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
    ClassicScript, DevToolsPage, DevToolsQueue, DevToolsQueued, EngineSettings, FrameId,
    FrameToken, ScriptEngine,
};
use serde_json::{Value, json};

/// An engine with a DevTools session, `"s"`, whose messages `sent` has.
/// `parent`'s one child frame, and its token, as the engine has them.
fn only_child(engine: &mut dom_js::V8Engine, parent: FrameId) -> (FrameId, FrameToken) {
    let children = engine.child_frames(parent);
    let [child] = children[..] else {
        panic!("{children:?}");
    };
    (
        child,
        engine.frame_token(child).expect("a child frame's token"),
    )
}

struct Inspected {
    engine: crate::TestEngine,
    sent: Rc<RefCell<Vec<Value>>>,
    next_id: u64,
}

/// The main frame's token the browser gives the engine here.
const TOP_TOKEN: FrameToken = FrameToken([0xAA; 16]);

impl Inspected {
    /// Its top-level frame's token `TOP_TOKEN`; a child frame's, the one
    /// the engine drew.
    fn new() -> Self {
        Self::going_on_from(None)
    }

    /// [`Self::new`], its session going on from `state` (another engine's
    /// `devtools_state`), if given.
    fn going_on_from(state: Option<&[u8]>) -> Self {
        let mut engine = crate::TestEngine::new(EngineSettings::default());
        engine.set_main_frame_token(TOP_TOKEN);
        let sent: Rc<RefCell<Vec<Value>>> = Rc::default();
        let to = sent.clone();
        engine.devtools_attach(
            "s".to_owned(),
            Box::new(move |message| {
                to.borrow_mut()
                    .push(serde_json::from_str(&message).unwrap())
            }),
            state,
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
        [format!("created {TOP_TOKEN} https://example.test 1")]
    );

    // An iframe's initial `about:blank`, of its parent's origin.
    page.run(
        top,
        "document.appendChild(document.createElement('html'));
         document.documentElement.appendChild(document.createElement('iframe'));",
    );
    let (child, token) = only_child(&mut page.engine, top);
    assert_ne!(token, TOP_TOKEN);
    assert_eq!(
        contexts(&page.events()),
        [format!("created {token} https://example.test 2")]
    );
    // Its document of another origin, in a realm of its own: the one
    // before has gone. An opaque origin is "://", as Chrome gives it.
    page.engine
        .create_window(child, "data:text/html,x", Instant::now())
        .unwrap();
    assert_eq!(
        contexts(&page.events()),
        ["destroyed 2".to_owned(), format!("created {token} :// 3")]
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
        [
            "cleared".to_owned(),
            format!("created {TOP_TOKEN} https://other.test 4")
        ]
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
    let (child, token) = only_child(&mut page.engine, top);
    page.engine
        .create_window(child, "https://other.test/", Instant::now())
        .unwrap();
    page.run(child, iframe);
    let (grandchild, grandchild_token) = only_child(&mut page.engine, child);
    page.events();
    page.engine
        .create_window(child, "https://third.test/", Instant::now())
        .unwrap();
    assert_eq!(
        contexts(&page.events()),
        [
            "destroyed 4".to_owned(),
            "destroyed 3".to_owned(),
            format!("created {token} https://third.test 5")
        ]
    );
    assert_eq!(
        (
            page.engine
                .frame_parent(grandchild)
                .map(|(parent, _)| parent),
            page.engine.frame_of_token(grandchild_token)
        ),
        (None, None),
        "the grandchild's frame is gone"
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
    let (child, _) = only_child(&mut page.engine, top);
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
    /// The document `Test.countElements` counts the elements of.
    document: std::cell::Cell<Option<dom::NodeId>>,
    /// The elements `Test.countElements` counted while paused, in turn.
    counted: RefCell<Vec<usize>>,
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

    fn answer_here(&self, _session: &str, message: &str, page: &mut dyn DevToolsPage) -> bool {
        if message.contains("Test.countElements") {
            let document = self.0.document.get().expect("the test's document");
            let count = page.dom().read(|cx| {
                let mut count = 0;
                let mut stack: Vec<_> = cx.node_by_id(document).into_iter().collect();
                while let Some(node) = stack.pop() {
                    if cx.kind(node) == dom::NodeKind::Element {
                        count += 1;
                    }
                    let mut child = cx.first_child(node);
                    while let Some(next) = child {
                        stack.push(next);
                        child = cx.next_sibling(next);
                    }
                }
                count
            });
            self.0.counted.borrow_mut().push(count);
            return true;
        }
        message.contains("Page.")
    }

    fn paused(&self, _page: &mut dyn DevToolsPage) {}

    fn waiting(&self, _page: &mut dyn DevToolsPage) {}

    // The queue says no frame to draw.
    fn render(&self, _page: &mut dyn DevToolsPage) {}
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
    let attach = DevToolsQueued::Attach {
        session: "t".to_owned(),
        state: None,
    };
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

/// The host reads the DOM while script is paused, wherever it pauses: in
/// a script, in a listener a DOM call dispatched to (`click()`), in a
/// custom element's constructor a DOM call ran (`createElement`), and in a
/// mutation observer's callback at the microtask checkpoint; each time the
/// DOM as script has made it so far.
#[test]
fn the_host_reads_the_dom_while_paused() {
    let mut page = Inspected::new();
    let queue = Rc::new(Queue::default());
    let _interrupter = page
        .engine
        .set_devtools_queue(Box::new(SharedQueue(queue.clone())));
    let top = page.engine.top_frame();
    page.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    queue
        .document
        .set(page.engine.documents_in_tree_order().first().copied());
    page.call("Debugger.enable", json!({}));
    for round in 0..4 {
        queue.queued.borrow_mut().extend([
            message(100 + 2 * round, "Test.countElements"),
            message(101 + 2 * round, "Debugger.resume"),
        ]);
    }
    page.run(
        top,
        "if (!document.documentElement) document.append(document.createElement('html'));
         const html = document.documentElement;
         html.append(document.createElement('p'));
         debugger;
         const button = document.createElement('button');
         button.onclick = () => { html.append(document.createElement('p')); debugger; };
         html.append(button);
         button.click();
         new MutationObserver(() => { debugger; }).observe(html, { childList: true });
         html.append(document.createElement('p'));
         customElements.define('x-a', class extends HTMLElement {
             constructor() { super(); debugger; }
         });
         document.createElement('x-a');",
    );
    // The host's checkpoint, as after any script.
    page.engine.perform_microtask_checkpoint();
    assert_eq!(*queue.counted.borrow(), [2, 4, 5, 5]);
    assert!(queue.queued.borrow().is_empty());
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
        None,
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
    let sink = crate::fetch::Sink::default();
    page.engine.set_load_sink(Box::new(sink.clone()));
    page.run(
        top,
        "function addImage() { new Image().src = 'x.png'; }
         function go() { fetch('d.json'); }
         function addFrame() { document.body.append(document.createElement('iframe')); }
         function lazily() { import('./m.js'); }
         addImage(); go(); addFrame(); lazily();",
    );
    page.engine.perform_microtask_checkpoint();
    // The `fetch()`'s, the `import()`'s module and the image's, handed to
    // the load sink with them (the image's in its microtask, after the
    // others).
    let sent = sink.take();
    let [
        crate::fetch::Sent::Fetch(_, origin),
        crate::fetch::Sent::Script(request, module),
        crate::fetch::Sent::Image(url, request_origin, image),
    ] = sent.as_slice()
    else {
        panic!("{sent:?}");
    };
    assert_eq!(request.url, "https://example.test/a/m.js");
    assert_eq!(top_function(module.stack.clone()), "lazily");
    assert_eq!(top_function(origin.stack.clone()), "go");
    assert_eq!(url, "https://example.test/a/x.png");
    assert_eq!(
        request_origin.as_deref(),
        Some("https://example.test"),
        "its document's"
    );
    assert_eq!(top_function(image.stack.clone()), "addImage");
    let top = page.engine.top_frame();
    let (child, _) = only_child(&mut page.engine, top);
    let (_, owner) = page.engine.frame_parent(child).expect("the frame");
    let dom = page.engine.dom_mut();
    let owner = dom.read(|cx| cx.node_id(owner.get(cx.gc)));
    assert_eq!(
        top_function(dom.take_request_stack(StackAsker::Element(owner))),
        "addFrame"
    );
}

/// A session goes on in another engine (the tab's next renderer's) from
/// the state the first gave: `Runtime` on (the new realm is told), the
/// pause on exceptions, and the breakpoint set by URL, with the id the
/// client knows it by. The breakpoint set again is no one's to answer.
#[test]
fn a_session_goes_on_in_another_engine_from_its_state() {
    let mut first = Inspected::new();
    first.call("Runtime.enable", json!({}));
    first.call("Debugger.enable", json!({}));
    first.call("Debugger.setPauseOnExceptions", json!({ "state": "all" }));
    let set = first.call(
        "Debugger.setBreakpointByUrl",
        json!({ "url": "https://example.test/b.js", "lineNumber": 1 }),
    );
    let id = set["result"]["breakpointId"].clone();
    assert!(id.is_string(), "{set}");
    let state = first.engine.devtools_state("s").expect("the session's");

    let mut next = Inspected::going_on_from(Some(&state));
    assert!(
        next.sent
            .borrow()
            .iter()
            .all(|message| message.get("id").is_none()),
        "{:?}",
        next.sent.borrow()
    );
    let queue = Rc::new(Queue::default());
    let _interrupter = next
        .engine
        .set_devtools_queue(Box::new(SharedQueue(queue.clone())));
    queue.queued.borrow_mut().extend([
        message(100, "Debugger.resume"),
        message(101, "Debugger.resume"),
    ]);
    let top = next.engine.top_frame();
    next.engine
        .create_window(top, "https://example.test/a/", Instant::now())
        .unwrap();
    next.engine.run_classic_script(
        top,
        &ClassicScript {
            source: "window.a = 1;\nwindow.b = 2;\ntry { throw 1; } catch {}",
            url: "https://example.test/b.js",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
    let events = next.events();
    assert!(
        events
            .iter()
            .any(|(method, _)| method == "Runtime.executionContextCreated"),
        "{events:?}"
    );
    let paused: Vec<(&str, &Value)> = events
        .iter()
        .filter(|(method, _)| method == "Debugger.paused")
        .map(|(_, params)| {
            (
                params["reason"].as_str().unwrap_or_default(),
                &params["hitBreakpoints"],
            )
        })
        .collect();
    assert_eq!(paused.len(), 2, "{events:?}");
    assert_eq!(paused[0].1, &json!([id]), "{events:?}");
    assert_eq!(paused[1].0, "exception", "{events:?}");
}

/// A breakpoint removed, or `Debugger` turned off, goes from the state:
/// another engine going on from it doesn't pause there.
#[test]
fn a_breakpoint_removed_goes_from_the_state() {
    for removal in ["Debugger.removeBreakpoint", "Debugger.disable"] {
        let mut first = Inspected::new();
        first.call("Debugger.enable", json!({}));
        let set = first.call(
            "Debugger.setBreakpointByUrl",
            json!({ "url": "https://example.test/b.js", "lineNumber": 0 }),
        );
        let id = set["result"]["breakpointId"].clone();
        first.call(removal, json!({ "breakpointId": id }));
        first.call("Debugger.enable", json!({}));
        let state = first.engine.devtools_state("s").expect("the session's");
        let mut next = Inspected::going_on_from(Some(&state));
        let top = next.engine.top_frame();
        next.engine
            .create_window(top, "https://example.test/a/", Instant::now())
            .unwrap();
        next.engine.run_classic_script(
            top,
            &ClassicScript {
                source: "window.a = 1;",
                url: "https://example.test/b.js",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
        assert_eq!(pauses(&next.events()), Vec::<&str>::new(), "{removal}");
    }
}

/// State that isn't one (a renderer wrote it) is taken as none: the
/// session starts afresh.
#[test]
fn a_state_that_isnt_one_starts_the_session_afresh() {
    for state in [
        &b""[..],
        b"\x05\x00",
        b"\xff\xff\xff\xff",
        b"\x00\x00\x00\x00[1]",
    ] {
        let mut next = Inspected::going_on_from(Some(state));
        let top = next.engine.top_frame();
        next.engine
            .create_window(top, "https://example.test/a/", Instant::now())
            .unwrap();
        let answer = next.call(
            "Runtime.evaluate",
            json!({ "expression": "1", "returnByValue": true }),
        );
        assert_eq!(
            answer["result"]["result"]["value"], 1,
            "{state:?}: {answer}"
        );
    }
}
