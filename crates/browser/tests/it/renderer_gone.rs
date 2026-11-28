//! A navigation as the tab's renderer goes (it crashes, or is ended for a
//! bad message or `about:crash`), as Chromium has it: what was sent to
//! commit goes with the renderer; a page's own navigation is cancelled (it
//! may have been ended for what it sent); the browser's own goes on, and
//! commits in a new renderer, launched as it commits. The renderer has
//! nothing at its end: the test reports each commit itself.

use std::time::Duration;

use browser::WebContents;
use net::test_server::{Route, TestServer};
use protocol::renderer::RendererHostMsg;

use crate::navigation::server;
use crate::visible_url::{committed, load, page_navigates, tab, until_sent, window_ended};

/// A page at `/`, one at `/b` and one at `/c` whose responses take a
/// little, so a renderer can go while they load.
fn pages() -> TestServer {
    let page = || Route::ok("text/html", "<!DOCTYPE html>");
    let slow = || page().with_delay(Duration::from_millis(200));
    server(&[("/", page()), ("/b", slow()), ("/c", slow())])
}

/// A way the tab's renderer goes, named.
type Going = (&'static str, fn(&mut WebContents));

/// Each of the ways a renderer goes.
const GOINGS: [Going; 3] = [
    ("it crashed", |web_contents| {
        web_contents.on_renderer_message(&RendererHostMsg::RendererGone)
    }),
    ("it sent a bad message", |web_contents| {
        web_contents.received_bad_message(browser::BadMessageReason::UnknownLoader)
    }),
    ("about:crash", |web_contents| {
        web_contents.navigate("about:crash")
    }),
];

/// The browser's own navigation goes on as the renderer goes, and commits
/// in a new renderer, with a frame sink of its own: no sad tab left with
/// the address bar at where it went.
#[test]
fn the_browsers_navigation_commits_in_a_new_renderer() {
    for (how, go) in GOINGS {
        let server = pages();
        let (a, b) = (server.url("/"), server.url("/b"));
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &a);
        let (renderer, sink) = (web_contents.renderer_id(), web_contents.frame_sink_id());
        web_contents.navigate(&b);
        let id = web_contents.latest_navigation_id();
        go(&mut web_contents);
        assert!(web_contents.is_crashed(), "{how}");
        assert!(web_contents.is_navigating(id), "{how}: it goes on");
        until_sent(&mut web_contents, &host_rx, id);
        assert!(!web_contents.is_crashed(), "{how}");
        assert_ne!(web_contents.renderer_id(), renderer, "{how}");
        assert_ne!(web_contents.frame_sink_id(), sink, "{how}");
        assert_eq!(web_contents.visible_url(), Some(b.as_str()), "{how}");
        committed(&mut web_contents, id, &b);
        assert!(
            !web_contents.is_crashed(),
            "{how}: the commit is the new renderer's"
        );
        assert_eq!(web_contents.last_committed_url(), Some(b.as_str()), "{how}");
        web_contents.shutdown();
    }
}

/// A page's own navigation goes with its renderer: the tab stays at the
/// page it showed, crashed, the history as it was.
#[test]
fn a_pages_navigation_goes_with_its_renderer() {
    for (how, go) in GOINGS {
        let server = pages();
        let (a, b) = (server.url("/"), server.url("/b"));
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &a);
        page_navigates(&mut web_contents, 1, &b);
        window_ended(&mut web_contents, 1);
        let id = web_contents.latest_navigation_id();
        go(&mut web_contents);
        assert!(!web_contents.is_navigating(id), "{how}: cancelled");
        assert!(web_contents.is_crashed(), "{how}");
        assert_eq!(web_contents.visible_url(), Some(a.as_str()), "{how}");
        assert_eq!(web_contents.controller().entry_count(), 1, "{how}");
        // Nothing comes of its load.
        std::thread::sleep(Duration::from_millis(400));
        while let Ok(event) = host_rx.try_recv() {
            web_contents.on_navigation_event(event);
        }
        assert!(
            web_contents.is_crashed(),
            "{how}: no renderer launched for it"
        );
        web_contents.shutdown();
    }
}

/// The page's own back and forward (`history.back()`) go with its
/// renderer too; the browser's own (the back button) go on.
#[test]
fn a_pages_traversal_goes_with_its_renderer_and_the_browsers_goes_on() {
    let server = pages();
    let (a, c) = (server.url("/"), server.url("/c"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &c);
    load(&mut web_contents, &host_rx, &a);
    web_contents.on_renderer_message(&RendererHostMsg::GoToEntryAtOffset {
        frame: web_contents.devtools_frame_token(),
        offset: -1,
    });
    let id = web_contents.latest_navigation_id();
    assert!(web_contents.is_navigating(id));
    web_contents.on_renderer_message(&RendererHostMsg::RendererGone);
    assert!(!web_contents.is_navigating(id), "the page's is cancelled");
    assert!(web_contents.go_back());
    let id = web_contents.latest_navigation_id();
    until_sent(&mut web_contents, &host_rx, id);
    assert!(
        !web_contents.is_crashed(),
        "the browser's commits in a new renderer"
    );
    committed(&mut web_contents, id, &c);
    assert_eq!(web_contents.last_committed_url(), Some(c.as_str()));
    web_contents.shutdown();
}

/// What was sent to commit goes with the renderer it went to: nothing
/// commits it, and the tab shows the page it showed.
#[test]
fn a_navigation_sent_to_commit_goes_with_the_renderer() {
    let server = pages();
    let (a, b) = (server.url("/"), server.url("/b"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &a);
    web_contents.navigate(&b);
    let id = web_contents.latest_navigation_id();
    until_sent(&mut web_contents, &host_rx, id);
    assert_eq!(web_contents.visible_url(), Some(b.as_str()));
    web_contents.on_renderer_message(&RendererHostMsg::RendererGone);
    assert!(web_contents.is_crashed());
    assert_eq!(web_contents.visible_url(), Some(a.as_str()));
    web_contents.shutdown();
}
