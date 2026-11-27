//! The URL a tab shows (`WebContents::visible_url`) as its main frame
//! navigates: a navigation's own before it commits only if the browser
//! started it for a new entry, or the tab has shown nothing but its first
//! document (Chromium's `NavigationControllerImpl::GetVisibleEntry`); the
//! page shown's otherwise. The renderer has nothing at its end: the test
//! reports each commit itself, so the time between a commit sent and its
//! report is the test's to look at.

use std::time::{Duration, Instant};

use browser::{ContentsEvent, RendererLauncher, WebContents};
use crossbeam_channel::Receiver;
use net::test_server::{Route, TestServer};
use protocol::renderer::{BeginNavigationKind, RendererHostMsg};

use crate::navigation::{TIMEOUT, in_memory, server, tab_launched};

fn tab() -> (WebContents, Receiver<ContentsEvent>) {
    tab_launched(
        RendererLauncher::Detached,
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    )
}

/// A page at `/` and `/b`, and one at `/slow` whose response takes long.
fn pages() -> TestServer {
    let page = || Route::ok("text/html", "<!DOCTYPE html>");
    server(&[
        ("/", page()),
        ("/b", page()),
        ("/slow", page().with_delay(Duration::from_secs(30))),
    ])
}

/// Runs the tab's navigation events until navigation `id` is sent to
/// commit.
fn until_sent(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>, id: u64) {
    let deadline = Instant::now() + TIMEOUT;
    while web_contents.is_navigating(id) {
        let event = host_rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("the navigation is sent to commit");
        match event {
            ContentsEvent::Renderer { .. }
            | ContentsEvent::BadMessage { .. }
            | ContentsEvent::NavigationStarted(_) => {}
            event => web_contents.on_navigation_event(event),
        }
    }
}

/// The renderer reports the main frame's navigation `id` committed, at
/// `url`.
fn committed(web_contents: &mut WebContents, id: u64, url: &str) {
    web_contents.on_renderer_message(&RendererHostMsg::DidCommitNavigation {
        frame: web_contents.devtools_frame_token(),
        navigation_id: id,
        url: url.into(),
    });
}

/// The browser's navigation to `url`, committed.
fn load(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>, url: &str) {
    web_contents.navigate(url);
    let id = web_contents.latest_navigation_id();
    until_sent(web_contents, host_rx, id);
    committed(web_contents, id, url);
}

/// The page asks for its main frame to navigate to `url`, its navigation
/// `begin_id`.
fn page_navigates(web_contents: &mut WebContents, begin_id: u64, url: &str) {
    let main = web_contents.devtools_frame_token();
    web_contents.on_renderer_message(&RendererHostMsg::BeginNavigation {
        frame: main,
        initiator: main,
        id: begin_id,
        kind: BeginNavigationKind::To {
            url: url.into(),
            replace: false,
        },
        devtools_initiator: None,
        history_in_new_child_frame: false,
        load_flags: Default::default(),
    });
}

/// The task that asked for the page's navigation `begin_id` is over: it
/// may commit.
fn window_ended(web_contents: &mut WebContents, begin_id: u64) {
    web_contents.on_renderer_message(&RendererHostMsg::NavigationCancellationWindowEnded {
        ids: vec![begin_id],
    });
}

/// A page's own navigation leaves the page's URL shown till it commits,
/// sent to commit included; then where it went shows.
#[test]
fn a_pages_navigation_shows_once_it_commits() {
    let server = pages();
    let (a, b) = (server.url("/"), server.url("/b"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &a);
    page_navigates(&mut web_contents, 1, &b);
    let id = web_contents.latest_navigation_id();
    assert_eq!(web_contents.latest_navigation_url(), Some(b.as_str()));
    assert_eq!(web_contents.visible_url(), Some(a.as_str()));
    window_ended(&mut web_contents, 1);
    until_sent(&mut web_contents, &host_rx, id);
    assert_eq!(
        web_contents.visible_url(),
        Some(a.as_str()),
        "sent to commit"
    );
    assert_eq!(web_contents.last_committed_url(), Some(a.as_str()));
    committed(&mut web_contents, id, &b);
    assert_eq!(web_contents.visible_url(), Some(b.as_str()));
    assert_eq!(web_contents.last_committed_url(), Some(b.as_str()));
    web_contents.shutdown();
}

/// A page that keeps navigating, each navigation in place of the one
/// before, never has the tab show where it goes (the spoof Chromium's
/// `GetVisibleEntry` is for): none of them commits.
#[test]
fn a_page_that_keeps_navigating_never_shows_where_to() {
    let server = pages();
    let (a, slow) = (server.url("/"), server.url("/slow"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &a);
    for begin_id in 1..=20 {
        page_navigates(&mut web_contents, begin_id, &slow);
        window_ended(&mut web_contents, begin_id);
        assert_eq!(web_contents.visible_url(), Some(a.as_str()), "{begin_id}");
    }
    assert_eq!(web_contents.latest_navigation_url(), Some(slow.as_str()));
    web_contents.shutdown();
}

/// The browser's own navigation for a new entry (the address typed,
/// DevTools' `Page.navigate`) shows where it goes as it loads, and as it is
/// sent to commit; stopped, the page shown's URL shows again.
#[test]
fn the_browsers_navigation_shows_where_it_goes_till_it_stops() {
    let server = pages();
    let (a, b, slow) = (server.url("/"), server.url("/b"), server.url("/slow"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &a);
    web_contents.navigate(&slow);
    assert_eq!(web_contents.visible_url(), Some(slow.as_str()));
    assert_eq!(web_contents.last_committed_url(), Some(a.as_str()));
    // Back to the entry shown: what is on its way stops.
    assert!(web_contents.go_to_offset(0));
    assert_eq!(web_contents.visible_url(), Some(a.as_str()), "stopped");
    web_contents.navigate(&b);
    let id = web_contents.latest_navigation_id();
    until_sent(&mut web_contents, &host_rx, id);
    assert_eq!(
        web_contents.visible_url(),
        Some(b.as_str()),
        "sent to commit"
    );
    committed(&mut web_contents, id, &b);
    assert_eq!(web_contents.visible_url(), Some(b.as_str()));
    web_contents.shutdown();
}

/// A page's navigation in place of the browser's leaves the page shown's
/// URL shown: neither navigation's.
#[test]
fn a_pages_navigation_in_place_of_the_browsers_shows_the_page() {
    let server = pages();
    let (a, b, slow) = (server.url("/"), server.url("/b"), server.url("/slow"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &a);
    web_contents.navigate(&slow);
    assert_eq!(web_contents.visible_url(), Some(slow.as_str()));
    page_navigates(&mut web_contents, 1, &b);
    assert_eq!(web_contents.visible_url(), Some(a.as_str()));
    web_contents.shutdown();
}

/// Back, forward and reload leave the page shown's URL shown till they
/// commit, the browser's own too: a history entry's URL is what the pages
/// put in the history.
#[test]
fn history_navigations_show_once_they_commit() {
    let server = pages();
    let (a, b) = (server.url("/"), server.url("/b"));
    let (mut web_contents, host_rx) = tab();
    load(&mut web_contents, &host_rx, &a);
    load(&mut web_contents, &host_rx, &b);
    assert!(web_contents.go_back());
    let id = web_contents.latest_navigation_id();
    assert_eq!(web_contents.latest_navigation_url(), Some(a.as_str()));
    assert_eq!(web_contents.visible_url(), Some(b.as_str()));
    until_sent(&mut web_contents, &host_rx, id);
    assert_eq!(
        web_contents.visible_url(),
        Some(b.as_str()),
        "sent to commit"
    );
    committed(&mut web_contents, id, &a);
    assert_eq!(web_contents.visible_url(), Some(a.as_str()));
    web_contents.reload();
    assert_eq!(web_contents.visible_url(), Some(a.as_str()));
    web_contents.shutdown();
}

/// A tab that has shown nothing yet, or nothing but its first document
/// (`navigate_initial`'s), shows where a navigation goes as it loads, a
/// page's own too: there is no page of its to spoof (Chromium's
/// `IsUnmodifiedBlankTab`).
#[test]
fn a_tabs_first_navigation_shows_where_it_goes() {
    let server = pages();
    let (b, slow) = (server.url("/b"), server.url("/slow"));
    let (mut web_contents, _host_rx) = tab();
    assert_eq!(web_contents.visible_url(), None);
    page_navigates(&mut web_contents, 1, &slow);
    assert_eq!(
        web_contents.visible_url(),
        Some(slow.as_str()),
        "nothing shown"
    );
    assert_eq!(web_contents.last_committed_url(), None);
    // `about:blank` commits as it starts.
    web_contents.navigate_initial("about:blank");
    let id = web_contents.latest_navigation_id();
    committed(&mut web_contents, id, "about:blank");
    page_navigates(&mut web_contents, 2, &b);
    assert_eq!(
        web_contents.visible_url(),
        Some(b.as_str()),
        "the first document"
    );
    web_contents.shutdown();
}
