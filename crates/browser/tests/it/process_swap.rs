//! A main frame navigation to another site commits in a new renderer
//! (Chromium's `RenderFrameHostManager` swapping for a cross-site
//! navigation), the one before unloading and ending; one within the
//! site, a reload of the same page and an opaque `about:blank` stay in the
//! renderer the tab has. Where a document goes is whose it is: a reload
//! that lands elsewhere, and an `about:blank` of another site's origin, go
//! where that site's do.

use crossbeam_channel::Receiver;
use net::test_server::{Route, TestServer};
use protocol::renderer::RendererHostMsg;

use browser::{ContentsEvent, WebContents};

use crate::navigation::{TIMEOUT, in_memory, next_message, server, tab_in};

/// Navigates to `url` and waits for its document's load: the renderer it
/// committed in.
fn load(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
    url: &str,
) -> browser::RendererId {
    web_contents.navigate(url);
    wait_for_load(web_contents, host_rx)
}

/// Waits for the latest navigation's document's load: the renderer it
/// committed in.
fn wait_for_load(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
) -> browser::RendererId {
    let loaded = web_contents.latest_navigation_id();
    loop {
        let msg = next_message(web_contents, host_rx, TIMEOUT).expect("the renderer answers");
        web_contents.on_renderer_message(&msg);
        if let RendererHostMsg::DidFinishLoad { navigation_id, .. } = msg
            && navigation_id == loaded
        {
            return web_contents.renderer_id();
        }
    }
}

/// Two sites on one server: its address, and `localhost`.
fn two_sites() -> (TestServer, String, String) {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>a")),
        ("/x", Route::ok("text/html", "<!DOCTYPE html>x")),
    ]);
    let a = server.url("/");
    let b = server.url_for_host("localhost", "/");
    (server, a, b)
}

#[test]
fn a_navigation_to_another_site_commits_in_a_new_renderer() {
    let (server, a, b) = two_sites();
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let first = web_contents.renderer_id();
    // The tab's first site takes the renderer it was made with.
    let in_a = load(&mut web_contents, &host_rx, &a);
    assert_eq!(in_a, first);
    assert!(!web_contents.take_renderer_replaced());
    let in_b = load(&mut web_contents, &host_rx, &b);
    assert_ne!(in_b, in_a);
    assert!(web_contents.take_renderer_replaced());
    assert!(!web_contents.is_current_renderer(in_a), "replaced");
    assert_eq!(web_contents.last_committed_url(), Some(b.as_str()));
    // Within the site, the same renderer.
    let x = server.url_for_host("localhost", "/x");
    assert_eq!(load(&mut web_contents, &host_rx, &x), in_b);
    assert!(!web_contents.take_renderer_replaced());
    web_contents.shutdown();
}

/// Back to the site before is another site again: a new renderer, not
/// the one before (which has ended).
#[test]
fn going_back_to_the_site_before_commits_in_a_new_renderer() {
    let (_server, a, b) = two_sites();
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let in_a = load(&mut web_contents, &host_rx, &a);
    let in_b = load(&mut web_contents, &host_rx, &b);
    assert!(web_contents.go_back());
    let back = wait_for_load(&mut web_contents, &host_rx);
    assert_ne!(back, in_b);
    assert_ne!(back, in_a);
    assert_eq!(web_contents.last_committed_url(), Some(a.as_str()));
    assert_eq!(web_contents.controller().current_index(), Some(0));
    web_contents.shutdown();
}

/// A reload of the same page stays in the renderer it is in; an opaque
/// `about:blank` (the browser's own navigation to it) stays too, whatever
/// the renderer holds, and leaves it the site it had.
#[test]
fn a_reload_and_about_blank_stay() {
    let (server, a, _b) = two_sites();
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let in_a = load(&mut web_contents, &host_rx, &a);
    web_contents.reload();
    assert_eq!(wait_for_load(&mut web_contents, &host_rx), in_a);
    assert_eq!(load(&mut web_contents, &host_rx, "about:blank"), in_a);
    // Still `a`'s: its site's next document stays as well.
    assert_eq!(load(&mut web_contents, &host_rx, &server.url("/x")), in_a);
    web_contents.shutdown();
}

/// A reload goes by where it lands: a page that moved to another site
/// meanwhile (its server redirects there now) is a new renderer's; the
/// renderer of the site before takes none of that site's.
#[test]
fn a_reload_that_lands_on_another_site_commits_in_a_new_renderer() {
    let elsewhere = server(&[("/gone", Route::ok("text/html", "<!DOCTYPE html>gone"))]);
    let gone = elsewhere.url_for_host("localhost", "/gone");
    let server = server(&[(
        "/moving",
        Route::ok("text/html", "<!DOCTYPE html>here").then(Route::redirect(302, &gone)),
    )]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let here = load(&mut web_contents, &host_rx, &server.url("/moving"));
    web_contents.reload();
    let landed = wait_for_load(&mut web_contents, &host_rx);
    assert_ne!(landed, here);
    assert_eq!(web_contents.last_committed_url(), Some(gone.as_str()));
    web_contents.shutdown();
}

/// An `about:blank` the main frame is navigated to by its child frame of
/// another site is of that site (its origin the child's): a new renderer,
/// that site's, not the page's.
#[test]
fn about_blank_of_another_sites_origin_commits_in_that_sites_renderer() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>const frame = document.createElement('iframe'); \
                 frame.src = `http://localhost:${location.port}/child`; \
                 document.documentElement.append(frame);</script>",
            ),
        ),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>child")),
    ]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let page = load(&mut web_contents, &host_rx, &server.url("/"));
    let main = web_contents.devtools_frame_token();
    let child = web_contents.frame_tree().children(main)[0];
    let child_origin = url::Url::parse(&server.url_for_host("localhost", "/"))
        .unwrap()
        .origin();
    assert_eq!(web_contents.frame_tree().origin(child), Some(&child_origin));
    // The renderer's, as the child frame's `top.location = 'about:blank'`.
    web_contents.on_renderer_message(&RendererHostMsg::BeginNavigation {
        frame: main,
        initiator: child,
        id: 1,
        kind: protocol::renderer::BeginNavigationKind::To {
            url: "about:blank".into(),
            replace: false,
        },
        devtools_initiator: None,
        history_in_new_child_frame: false,
        load_flags: Default::default(),
    });
    // Its task is over: the browser may commit it.
    web_contents
        .on_renderer_message(&RendererHostMsg::NavigationCancellationWindowEnded { ids: vec![1] });
    let blank = wait_for_load(&mut web_contents, &host_rx);
    assert_ne!(blank, page);
    assert_eq!(web_contents.frame_tree().origin(main), Some(&child_origin));
    web_contents.shutdown();
}

/// A `data:` URL's origin is opaque, the same site as nothing: each
/// navigation to one is a new renderer, but for a reload of it, and a
/// traversal to its entry, which keep the site it committed as.
#[test]
fn every_data_url_is_a_site_of_its_own() {
    let (_server, a, _b) = two_sites();
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let in_a = load(&mut web_contents, &host_rx, &a);
    let data = "data:text/html,<!DOCTYPE html>d";
    let first = load(&mut web_contents, &host_rx, data);
    assert_ne!(first, in_a);
    let again = load(&mut web_contents, &host_rx, data);
    assert_ne!(again, first);
    web_contents.reload();
    assert_eq!(wait_for_load(&mut web_contents, &host_rx), again);
    // Back to it, and forward: each entry's site the one it committed as,
    // the renderer the one there is for it now.
    assert!(web_contents.go_back());
    let back = wait_for_load(&mut web_contents, &host_rx);
    assert_ne!(back, again);
    assert!(web_contents.go_forward());
    let forward = wait_for_load(&mut web_contents, &host_rx);
    assert_ne!(forward, back);
    web_contents.reload();
    assert_eq!(wait_for_load(&mut web_contents, &host_rx), forward);
    // An opaque `about:blank` stays in its renderer; back from it to the
    // `data:` entry is that entry's site again: the same renderer.
    assert_eq!(load(&mut web_contents, &host_rx, "about:blank"), forward);
    assert!(web_contents.go_back());
    assert_eq!(wait_for_load(&mut web_contents, &host_rx), forward);
    web_contents.shutdown();
}

/// A page of the browser's own is a site of its own: to it from a web
/// page, and back, are new renderers.
#[test]
fn a_page_of_the_browsers_own_is_a_site_of_its_own() {
    let (_server, a, _b) = two_sites();
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let in_a = load(&mut web_contents, &host_rx, &a);
    let demo = load(&mut web_contents, &host_rx, "about:demo");
    assert_ne!(demo, in_a);
    assert_ne!(load(&mut web_contents, &host_rx, &a), demo);
    web_contents.shutdown();
}

/// The browser's error page for a URL goes by that URL's site: a failed
/// navigation to another site is a new renderer.
#[test]
fn an_error_page_goes_by_its_urls_site() {
    let (_server, a, _b) = two_sites();
    let refused = net::test_server::refused_port();
    let (mut web_contents, host_rx) = tab_in(in_memory());
    let in_a = load(&mut web_contents, &host_rx, &a);
    let failed = format!("http://localhost:{}/", refused.port());
    assert_ne!(load(&mut web_contents, &host_rx, &failed), in_a);
    assert_eq!(web_contents.last_committed_url(), Some(failed.as_str()));
    web_contents.shutdown();
}

/// The document before's frames go with its renderer: the new renderer
/// tells of its own.
#[test]
fn the_frames_of_the_document_before_go_with_its_renderer() {
    let server = server(&[
        (
            "/",
            Route::ok("text/html", "<!DOCTYPE html><iframe src=/x></iframe>"),
        ),
        ("/x", Route::ok("text/html", "<!DOCTYPE html>x")),
    ]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    load(&mut web_contents, &host_rx, &server.url("/"));
    assert_eq!(web_contents.frame_urls().len(), 2);
    let b = server.url_for_host("localhost", "/x");
    load(&mut web_contents, &host_rx, &b);
    assert_eq!(web_contents.frame_urls(), [b]);
    web_contents.shutdown();
}

/// The frame sink of the renderer before stands in for the new one's till
/// that draws, if the user acted on the page before and it drew; if not,
/// Viz forgets it at once.
#[test]
fn the_sink_before_stands_in_till_the_new_one_draws_if_the_user_acted() {
    for acted in [false, true] {
        let (_server, a, b) = two_sites();
        let (mut web_contents, host_rx) = tab_in(in_memory());
        load(&mut web_contents, &host_rx, &a);
        let before = web_contents.frame_sink_id();
        // As Viz tells (there is none here).
        web_contents.on_navigation_event(ContentsEvent::FirstFrame(before));
        if acted {
            web_contents.on_renderer_message(&RendererHostMsg::DidReceiveUserActivation);
        }
        load(&mut web_contents, &host_rx, &b);
        assert_ne!(web_contents.frame_sink_id(), before);
        assert_eq!(
            web_contents
                .fallback_surface_id()
                .map(|surface| surface.frame_sink_id),
            acted.then_some(before),
            "acted on: {acted}"
        );
        let after = web_contents.frame_sink_id();
        web_contents.on_navigation_event(ContentsEvent::FirstFrame(after));
        assert_eq!(web_contents.fallback_surface_id(), None, "the new one drew");
        web_contents.shutdown();
    }
}
