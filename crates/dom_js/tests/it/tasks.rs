//! The engine's pass over the page's tasks (`ScriptEngine::run_due_tasks`):
//! timers, element tasks and messages in the order they were queued, one
//! pass running only those queued before it; the load sink a `fetch()`
//! and the blob URLs go through as script calls them; the user agent every
//! frame's `navigator` answers, the DOM's.

use std::cell::RefCell;
use std::rc::Rc;

use dom_api::{FrameId, FrameRequest, LoadSink, ScriptEngine};

use crate::history::Page;

/// A timer of 0 ms set before a message runs before it, and one set after
/// after it: one order across their task sources, not each source's tasks
/// in a batch.
#[test]
fn timers_and_messages_run_in_the_order_queued() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var log = [];
         onmessage = () => log.push('message');
         setTimeout(() => log.push('first timer'));
         postMessage('', '*');
         setTimeout(() => log.push('second timer'));
         ''",
    );
    // A little time, so that the timers of 0 ms are due.
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(page.engine.run_due_tasks());
    assert_eq!(page.eval("log.join()"), "first timer,message,second timer");
    assert!(!page.engine.run_due_tasks());
}

/// A message a message's listener posts waits for the next pass: windows
/// posting to each other forever give the loop back between passes, and
/// the DOM says one is queued.
#[test]
fn a_pass_runs_only_what_was_queued_before_it() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var count = 0;
         onmessage = () => { count++; postMessage('', '*'); };
         postMessage('', '*');
         ''",
    );
    assert!(page.engine.run_due_tasks());
    assert_eq!(page.eval("String(count)"), "1");
    assert!(page.engine.dom_mut().has_queued_tasks());
    assert!(page.engine.run_due_tasks());
    assert_eq!(page.eval("String(count)"), "2");
}

/// What the test's load sink was asked, in order.
#[derive(Default)]
struct Sink {
    calls: Rc<RefCell<Vec<String>>>,
}

impl LoadSink for Sink {
    fn start_fetch(&self, _: FrameId, request: &dom::FetchRequest) -> Option<u64> {
        self.calls
            .borrow_mut()
            .push(format!("fetch {}", request.url));
        (!request.url.ends_with("nowhere")).then_some(7)
    }

    fn register_blob_url(&self, _: FrameId, _: &str, _: &dom::BlobData) {
        self.calls.borrow_mut().push("register".to_owned());
    }

    fn revoke_blob_url(&self, _: &str) {
        self.calls.borrow_mut().push("revoke".to_owned());
    }

    fn frame_gone(&self, _: FrameId) {
        self.calls.borrow_mut().push("frame gone".to_owned());
    }
}

/// With a load sink, a `fetch()` is sent as it is called, in order with the
/// blob URLs registered and revoked around it, and the host told after
/// what went (`FetchSent`, its request's number; none for one the sink
/// sent nowhere); none waits for the host's look.
#[test]
fn a_fetch_goes_to_the_load_sink_as_it_is_called() {
    let mut page = Page::new("https://example.test/");
    let calls = Rc::new(RefCell::new(Vec::new()));
    page.engine.set_load_sink(Box::new(Sink {
        calls: calls.clone(),
    }));
    page.eval(
        "var url = URL.createObjectURL(new Blob(['x']));
         fetch('/data');
         URL.revokeObjectURL(url);
         fetch('/nowhere');
         ''",
    );
    assert_eq!(
        *calls.borrow(),
        [
            "register",
            "fetch https://example.test/data",
            "revoke",
            "fetch https://example.test/nowhere"
        ]
    );
    let sent: Vec<(String, Option<u64>)> = page
        .engine
        .take_frame_requests()
        .into_iter()
        .filter_map(|request| match request {
            FrameRequest::FetchSent {
                request,
                request_id,
                ..
            } => Some((request.url, request_id)),
            FrameRequest::Fetch { .. }
            | FrameRequest::RegisterBlobUrl { .. }
            | FrameRequest::RevokeBlobUrl { .. } => panic!("the sink's"),
            _ => None,
        })
        .collect();
    assert_eq!(
        sent,
        [
            ("https://example.test/data".to_owned(), Some(7)),
            ("https://example.test/nowhere".to_owned(), None)
        ]
    );
}

/// The user agent and the webdriver flag are the DOM's: a new iframe's
/// `about:blank` answers them too, before its host has heard of its frame.
#[test]
fn every_frames_navigator_answers_the_doms_user_agent() {
    let mut page = Page::new("https://example.test/");
    page.engine.dom_mut().set_agent("Machopo/1", true);
    assert_eq!(
        page.eval(
            "var f = document.createElement('iframe');
             document.documentElement.append(f);
             [navigator.userAgent, navigator.webdriver,
              f.contentWindow.navigator.userAgent, f.contentWindow.navigator.webdriver].join()"
        ),
        "Machopo/1,true,Machopo/1,true"
    );
}
