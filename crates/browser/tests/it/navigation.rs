//! Navigations end to end: the browser fetches the body, the renderer
//! parses it and reports the document's title back. HTTP goes to a local
//! test server, never off the machine.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use base::AtomicSequenceNumber;
use browser::{ContentsEvent, NetworkContext, NetworkContextParams, OpenUrlParams, WebContents};
use crossbeam_channel::{Receiver, RecvTimeoutError, unbounded};
use net::test_server::{Route, TestServer, unused_port};
use protocol::TabId;
use protocol::renderer::{RendererHostMsg, WindowOpenDisposition};
use protocol::viz::{
    CompositorFrameSinkMsg, ContextLostObserver, DisplayMsg, Mailbox, SharedImage,
    SharedImageInterface, VizMsg,
};
use viz::BeginFrameSource;

const TIMEOUT: Duration = Duration::from_secs(30);

/// A fresh tab, the channel its renderer's messages arrive on, Viz's end
/// of its frame sink, and the `BeginFrameSource` that paces it: for tests
/// that play Viz themselves ([`DrivenViz`]).
fn tab_with_viz() -> (
    WebContents,
    Receiver<ContentsEvent>,
    Receiver<VizMsg>,
    BeginFrameSource,
) {
    tab_with_viz_drawing(true)
}

/// [`tab_with_viz`], whose compositor draws each `BeginFrame` once the
/// main frame it started has committed (`wait`, as headless does), or at
/// once from what it has (as the window does).
fn tab_with_viz_drawing(
    wait: bool,
) -> (
    WebContents,
    Receiver<ContentsEvent>,
    Receiver<VizMsg>,
    BeginFrameSource,
) {
    tab_with_viz_flags(wait, false)
}

/// [`tab_with_viz_drawing`], answering `QueryElement` if `automation`.
fn tab_with_viz_flags(
    wait: bool,
    automation: bool,
) -> (
    WebContents,
    Receiver<ContentsEvent>,
    Receiver<VizMsg>,
    BeginFrameSource,
) {
    tab_with_viz_gpu(wait, automation, None)
}

/// [`tab_with_viz_flags`], whose compositor puts its tiles on `gpu`.
fn tab_with_viz_gpu(
    wait: bool,
    automation: bool,
    gpu: Option<Arc<dyn SharedImageInterface>>,
) -> (
    WebContents,
    Receiver<ContentsEvent>,
    Receiver<VizMsg>,
    BeginFrameSource,
) {
    tab_with_viz_colors(wait, automation, gpu, gfx::SystemColors::DEFAULT)
}

/// [`tab_with_viz_gpu`], whose renderer has `system_colors` as the
/// platform's.
fn tab_with_viz_colors(
    wait: bool,
    automation: bool,
    gpu: Option<Arc<dyn SharedImageInterface>>,
    system_colors: gfx::SystemColors,
) -> (
    WebContents,
    Receiver<ContentsEvent>,
    Receiver<VizMsg>,
    BeginFrameSource,
) {
    let (viz_tx, viz_rx) = unbounded::<VizMsg>();
    let (host_tx, host_rx) = unbounded();
    let web_contents = WebContents::new(
        TabId(0),
        browser::RendererFlags {
            test_fonts: false,
            web_tests: false,
            wait_for_all_pipeline_stages_before_draw: wait,
            automation,
            user_agent: browser::user_agent(),
            system_colors,
            num_raster_threads: None,
            caret_blink_interval: None,
        },
        browser::ScriptEngineFactory {
            engine: dom_js::V8Engine::boxed,
        },
        viz_tx,
        gpu,
        NetworkContext::start(NetworkContextParams {
            user_agent: browser::user_agent(),
            ..Default::default()
        })
        .expect("network service starts"),
        Box::new(move |_tab, msg| {
            let _ = host_tx.send(msg);
        }),
    )
    .expect("renderer starts");
    let mut begin_frames = BeginFrameSource::new();
    begin_frames.add_client(
        web_contents.frame_sink_id(),
        Box::new(web_contents.begin_frame_sender()),
    );
    (web_contents, host_rx, viz_rx, begin_frames)
}

/// The renderer's message `event` is, or `None` for a navigation's
/// redirect or end, which the tab follows or commits (as the embedder
/// hands it over).
fn renderer_message(
    web_contents: &mut WebContents,
    event: ContentsEvent,
) -> Option<RendererHostMsg> {
    match event {
        ContentsEvent::Renderer(msg) => Some(msg),
        ContentsEvent::NavigationRedirected {
            navigation_id,
            redirect,
        } => {
            web_contents.on_navigation_redirected(navigation_id, &redirect);
            None
        }
        ContentsEvent::NavigationCompleted {
            navigation_id,
            result,
        } => {
            web_contents.on_navigation_completed(navigation_id, result);
            None
        }
    }
}

/// The renderer's next message within `timeout`, the tab following the
/// redirects that come before it.
fn next_message(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
    timeout: Duration,
) -> Result<RendererHostMsg, RecvTimeoutError> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        let event =
            host_rx.recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))?;
        if let Some(msg) = renderer_message(web_contents, event) {
            return Ok(msg);
        }
    }
}

/// A fresh tab with a 200x200 viewport, and the channel its renderer's
/// messages arrive on. The viewport matters to input: the renderer only
/// takes a click aimed at a document it has drawn.
fn tab() -> (WebContents, Receiver<ContentsEvent>) {
    tab_for(false)
}

/// [`tab`], answering `QueryElement` if `automation`.
fn tab_for(automation: bool) -> (WebContents, Receiver<ContentsEvent>) {
    tab_for_colors(automation, gfx::SystemColors::DEFAULT)
}

/// [`tab_for`], whose renderer has `system_colors` as the platform's.
fn tab_for_colors(
    automation: bool,
    system_colors: gfx::SystemColors,
) -> (WebContents, Receiver<ContentsEvent>) {
    let (mut web_contents, host_rx, viz_rx, mut begin_frames) =
        tab_with_viz_colors(true, automation, None, system_colors);
    web_contents.resize(gfx::Size::new(200, 200), 1.0);
    // Viz, minus the drawing: frames are dropped, and `BeginFrame`s go to the
    // renderer back to back, as headless sends them.
    std::thread::spawn(move || {
        let mut frame_time = 0.0;
        loop {
            if begin_frames.wants_begin_frame() {
                begin_frames.issue(frame_time);
                frame_time += 1000.0 / 60.0;
            }
            match viz_rx.recv() {
                Ok(VizMsg::FrameSink(sink, msg)) => begin_frames.did_receive(sink, &msg),
                // A renderer launched again after one has gone.
                Ok(VizMsg::InvalidateFrameSinkId(sink)) => begin_frames.remove_client(sink),
                Ok(VizMsg::RegisterFrameSinkId(sink, observer)) => begin_frames
                    .add_client(sink, Box::new(move |args| observer.on_begin_frame(args))),
                Ok(_) => {}
                Err(_) => break,
            }
        }
    });
    (web_contents, host_rx)
}

/// Viz, played by the test on its own thread: sends a `BeginFrame`
/// whenever the renderer wants one, back to back (unless `held`), and
/// hands the renderer's messages to the tab meanwhile, so a test can see
/// when the renderer stops asking.
struct DrivenViz {
    begin_frames: BeginFrameSource,
    viz_rx: Receiver<VizMsg>,
    frame_time: f64,
    /// `BeginFrame`s sent so far.
    issued: usize,
    /// While set, no `BeginFrame` goes out: nothing new is drawn.
    held: bool,
    /// The tokens of the frames the renderer submitted.
    frames: Vec<u64>,
    /// The resources (rastered tiles) of each frame, by token.
    resources: HashMap<u64, Vec<u64>>,
    /// The navigations whose load finished, whichever wait saw it: a wait
    /// for something else takes the renderer's messages too.
    loaded: HashSet<u64>,
}

impl DrivenViz {
    fn new(viz_rx: Receiver<VizMsg>, begin_frames: BeginFrameSource) -> Self {
        Self {
            begin_frames,
            viz_rx,
            frame_time: 0.0,
            issued: 0,
            held: false,
            frames: Vec::new(),
            resources: HashMap::new(),
            loaded: HashSet::new(),
        }
    }

    /// Whether the renderer wants no `BeginFrame` and has answered the
    /// last.
    fn idle(&self) -> bool {
        !self.begin_frames.wants_begin_frame() && !self.begin_frames.is_pending()
    }

    /// Runs Viz until `done` (asked before each step, and with each of the
    /// renderer's messages) says so. Panics after `TIMEOUT`, naming `what`.
    fn run_until(
        &mut self,
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        what: &str,
        mut done: impl FnMut(&Self, Option<&RendererHostMsg>) -> bool,
    ) {
        let deadline = std::time::Instant::now() + TIMEOUT;
        loop {
            if done(self, None) {
                return;
            }
            if !self.held && self.begin_frames.wants_begin_frame() {
                self.begin_frames.issue(self.frame_time);
                self.frame_time += 1000.0 / 60.0;
                self.issued += 1;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            crossbeam_channel::select! {
                recv(self.viz_rx) -> msg => {
                    if let Ok(VizMsg::FrameSink(sink, msg)) = msg {
                        if let CompositorFrameSinkMsg::SubmitCompositorFrame { frame, .. } = &msg {
                            self.frames.push(frame.metadata.frame_token);
                            self.resources.insert(
                                frame.metadata.frame_token,
                                frame.resource_list.iter().map(|r| r.id.0).collect(),
                            );
                        }
                        self.begin_frames.did_receive(sink, &msg);
                    }
                }
                recv(host_rx) -> event => {
                    let Some(msg) = renderer_message(web_contents, event.expect("the renderer is up")) else {
                        continue;
                    };
                    web_contents.on_renderer_message(&msg);
                    if let RendererHostMsg::DidFinishLoad { navigation_id, .. } = &msg {
                        self.loaded.insert(*navigation_id);
                    }
                    if done(self, Some(&msg)) {
                        return;
                    }
                }
                default(left) => panic!("timed out waiting for {what}"),
            }
        }
    }

    /// Sends one `BeginFrame` now if the renderer wants one, held or not,
    /// and returns at once: what the compositor gets after it, it handles
    /// after it.
    fn begin_frame(&mut self) {
        assert!(self.begin_frames.wants_begin_frame());
        self.begin_frames.issue(self.frame_time);
        self.frame_time += 1000.0 / 60.0;
        self.issued += 1;
    }

    /// Stops sending `BeginFrame`s, and runs until the last is answered:
    /// from then on, nothing new is drawn.
    fn hold(&mut self, web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>) {
        self.held = true;
        self.run_until(
            web_contents,
            host_rx,
            "the BeginFrame's answer",
            |viz, _| !viz.begin_frames.is_pending(),
        );
    }

    /// Runs Viz until the latest navigation has loaded: at once if an
    /// earlier wait saw it load.
    fn until_loaded(&mut self, web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>) {
        let latest = web_contents.latest_navigation_id();
        self.run_until(web_contents, host_rx, "the load", |viz, _| {
            viz.loaded.contains(&latest)
        });
    }
}

/// A renderer given a viewport before it has a document asks for a frame.
/// The main frame for it has nothing to update, and must still end, or
/// the compositor never answers that `BeginFrame` and nothing is drawn
/// again. The other tests navigate first, so their first main frame has a
/// document.
#[test]
fn a_begin_frame_before_any_document_is_answered() {
    let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
    let mut viz = DrivenViz::new(viz_rx, begin_frames);
    web_contents.resize(gfx::Size::new(200, 200), 1.0);
    viz.run_until(
        &mut web_contents,
        &host_rx,
        "the BeginFrame's answer",
        // Checked before each BeginFrame goes out: the first is answered.
        |viz, _| viz.issued >= 1 && !viz.begin_frames.is_pending(),
    );

    // A document after it still loads and is drawn.
    web_contents.navigate("about:blank");
    let latest = web_contents.latest_navigation_id();
    viz.run_until(
        &mut web_contents,
        &host_rx,
        "the load",
        |_, msg| match msg {
            Some(RendererHostMsg::DidFinishLoad {
                navigation_id,
                frame_token,
                ..
            }) if *navigation_id == latest => {
                assert!(frame_token.is_some(), "the loaded page is drawn");
                true
            }
            _ => false,
        },
    );
    web_contents.shutdown();
}

/// The browser hears a renderer has gone once the channel from it closes,
/// so `RendererGone` comes once and last: nothing the renderer's threads
/// sent, the compositor's swap promises included, can follow it.
#[test]
fn the_renderer_going_is_the_last_message() {
    let (mut web_contents, host_rx) = tab();
    web_contents.navigate("about:blank");
    let latest = web_contents.latest_navigation_id();
    loop {
        let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("renderer answers");
        web_contents.on_renderer_message(&msg);
        if matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if navigation_id == latest)
        {
            break;
        }
    }
    web_contents.shutdown();
    // Shut down, the IO thread has gone too, and the channel to the test
    // with it: everything that is left is here. (Bounded: a channel that
    // stays open fails the test rather than hanging it.)
    let mut rest = Vec::new();
    loop {
        match next_message(&mut web_contents, &host_rx, TIMEOUT) {
            Ok(msg) => rest.push(msg),
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => panic!("the channel stays open: {rest:?}"),
        }
    }
    let gone = rest
        .iter()
        .filter(|m| matches!(m, RendererHostMsg::RendererGone))
        .count();
    assert_eq!(gone, 1, "{rest:?}");
    assert!(
        matches!(rest.last(), Some(RendererHostMsg::RendererGone)),
        "{rest:?}"
    );
}

/// A document's timers and animation frames go with it. The old one's
/// `requestAnimationFrame` loop would otherwise keep the renderer asking for
/// frames (and running script for a page no one sees) forever; the new
/// page looks the same either way.
#[test]
fn a_new_document_stops_the_old_ones_animation_frames() {
    let server = server(&[
        (
            "/animated",
            Route::ok(
                "text/html",
                "<script>function f() { requestAnimationFrame(f) } requestAnimationFrame(f)</script>",
            ),
        ),
        ("/still", Route::ok("text/html", "<p>still</p>")),
    ]);
    let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
    let mut viz = DrivenViz::new(viz_rx, begin_frames);
    web_contents.resize(gfx::Size::new(200, 200), 1.0);

    web_contents.navigate(server.url("/animated"));
    viz.until_loaded(&mut web_contents, &host_rx);
    let loaded = viz.issued;
    viz.run_until(
        &mut web_contents,
        &host_rx,
        "the animation's frames",
        |viz, _| viz.issued >= loaded + 10,
    );

    web_contents.navigate(server.url("/still"));
    viz.until_loaded(&mut web_contents, &host_rx);
    viz.run_until(
        &mut web_contents,
        &host_rx,
        "the renderer to stop asking for frames",
        |viz, _| viz.idle(),
    );
    web_contents.shutdown();
}

/// Waits for the next commit and the title that follows it.
fn next_commit(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
) -> (u64, String, String) {
    let mut committed = None;
    loop {
        let msg = next_message(web_contents, host_rx, TIMEOUT).expect("renderer answers");
        web_contents.on_renderer_message(&msg);
        match msg {
            RendererHostMsg::DidCommitNavigation { navigation_id, url } => {
                committed = Some((navigation_id, url))
            }
            RendererHostMsg::UpdateTitle(title) => {
                let (id, url) = committed.expect("commit before title");
                return (id, url, title);
            }
            _ => {}
        }
    }
}

/// Navigates a fresh tab to `url` and returns the committed URL and the
/// title the renderer reported.
fn navigate(url: &str) -> (String, String) {
    let (mut web_contents, host_rx) = tab();
    web_contents.navigate(url);
    let (_, committed, title) = next_commit(&mut web_contents, &host_rx);
    web_contents.shutdown();
    (committed, title)
}

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

#[test]
fn file_url_is_fetched_by_the_browser_and_parsed_by_the_renderer() {
    let dir = std::env::temp_dir().join(format!("machopo-navigation-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("page one.html");
    std::fs::write(
        &path,
        "<!DOCTYPE html><title>  Hello \n world </title><p>body",
    )
    .unwrap();
    let url = browser::loader::file_url(&path).unwrap();

    let (committed, title) = navigate(&url);
    assert_eq!(committed, url);
    assert_eq!(title, "Hello world");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn http_with_redirect_and_charset() {
    let server = server(&[
        ("/old", Route::redirect(302, "/new")),
        // "日本" in Shift_JIS, declared only in the header.
        (
            "/new",
            Route::ok(
                "text/html; charset=shift_jis",
                &b"<title>\x93\xfa\x96\x7b</title>"[..],
            ),
        ),
    ]);
    let (committed, title) = navigate(&server.url("/old"));
    assert_eq!(committed, server.url("/new"));
    assert_eq!(title, "日本");
}

/// The next redirect a navigation of the tab reaches, held: the tab isn't
/// told of it. The renderer's messages before it go to the tab.
fn held_redirect(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
) -> (u64, protocol::network::RedirectInfo) {
    loop {
        match host_rx.recv_timeout(TIMEOUT).expect("the redirect comes") {
            ContentsEvent::NavigationRedirected {
                navigation_id,
                redirect,
            } => return (navigation_id, redirect),
            ContentsEvent::Renderer(msg) => web_contents.on_renderer_message(&msg),
            ContentsEvent::NavigationCompleted {
                navigation_id,
                result,
            } => web_contents.on_navigation_completed(navigation_id, result),
        }
    }
}

/// The next navigation of the tab to end, held: the tab isn't told of it.
fn held_completion(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
) -> (u64, protocol::network::LoadResult) {
    loop {
        match host_rx.recv_timeout(TIMEOUT).expect("the navigation ends") {
            ContentsEvent::NavigationCompleted {
                navigation_id,
                result,
            } => return (navigation_id, result),
            event => {
                if let Some(msg) = renderer_message(web_contents, event) {
                    web_contents.on_renderer_message(&msg);
                }
            }
        }
    }
}

/// A navigation commits on the UI thread once it has loaded; one that
/// another replaced commits nothing, however late its end comes.
#[test]
fn a_replaced_navigations_end_commits_nothing() {
    let server = server(&[
        ("/a", Route::ok("text/html", "<title>a</title>")),
        ("/b", Route::ok("text/html", "<title>b</title>")),
    ]);
    let (mut web_contents, host_rx) = tab();
    web_contents.navigate(server.url("/a"));
    let (a, a_result) = held_completion(&mut web_contents, &host_rx);
    web_contents.navigate(server.url("/b"));
    web_contents.on_navigation_completed(a, a_result);
    let (_, committed, title) = next_commit(&mut web_contents, &host_rx);
    assert_eq!(
        (committed.as_str(), title.as_str()),
        (server.url("/b").as_str(), "b")
    );
    web_contents.shutdown();
}

/// A navigation waits at a redirect until the tab, on the UI thread, lets
/// it go on; one that another navigation replaced goes no further, and
/// letting it go on lets nothing else go.
#[test]
fn a_navigation_waits_at_a_redirect_for_the_tab() {
    let server = server(&[
        ("/old", Route::redirect(302, "/new")),
        ("/new", Route::ok("text/html", "<title>new</title>")),
        ("/other", Route::redirect(302, "/last")),
        ("/last", Route::ok("text/html", "<title>last</title>")),
    ]);
    let requested = |path: &str| server.requests().iter().any(|r| r.target == path);
    let (mut web_contents, host_rx) = tab();

    web_contents.navigate(server.url("/old"));
    let (old, old_redirect) = held_redirect(&mut web_contents, &host_rx);
    assert_eq!(old, web_contents.latest_navigation_id());
    assert_eq!(old_redirect.new_url, server.url("/new"));
    assert_eq!(old_redirect.response.status, 302);

    web_contents.navigate(server.url("/other"));
    let (other, other_redirect) = held_redirect(&mut web_contents, &host_rx);
    assert_ne!(other, old);
    // The replaced navigation's redirect: neither it nor the latest goes on.
    web_contents.on_navigation_redirected(old, &old_redirect);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!requested("/new"), "the replaced navigation went on");
    assert!(
        !requested("/last"),
        "the latest went on before its redirect was"
    );

    web_contents.on_navigation_redirected(other, &other_redirect);
    let (_, committed, title) = next_commit(&mut web_contents, &host_rx);
    assert_eq!(
        (committed.as_str(), title.as_str()),
        (server.url("/last").as_str(), "last")
    );
    assert!(!requested("/new"));
    web_contents.shutdown();
}

/// The tab's title follows `document.title`: the
/// renderer sends the main frame's when it may have changed and differs
/// from the last it sent, as parsed, set by script, set again to the same
/// (not sent; in the same task, and in one of its own), changed by a timer
/// (twice in one task: sent once), longer
/// than a tab takes (cut to `MAX_TITLE_CHARS`), and gone; a child frame's,
/// parsed or set, never.
#[test]
fn the_tab_title_follows_document_title() {
    let page = r#"<!DOCTYPE html>
<title>  First  </title>
<iframe srcdoc="<title>child</title><script>document.title = 'child set'</script>"></iframe>
<script>
document.title = 'Second';
document.title = 'Second';
onload = () => {
  // The same title, in a task of its own: not sent again.
  setTimeout(() => document.title = 'Second', 50);
  setTimeout(() => {
    document.querySelector('title').textContent = 'not sent';
    document.title = 'Third';
    setTimeout(() => {
      document.title = 'x'.repeat(5000);
      setTimeout(() => document.querySelector('title').remove());
    });
  }, 100);
};
</script>"#;
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (mut web_contents, host_rx) = tab();
    web_contents.navigate(server.url("/"));
    let mut titles = Vec::new();
    let mut long = None;
    while titles.last().is_none_or(|title: &String| !title.is_empty()) {
        let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("renderer answers");
        web_contents.on_renderer_message(&msg);
        if let RendererHostMsg::UpdateTitle(title) = msg {
            if title.len() > 100 {
                long = Some(web_contents.title().to_owned());
            }
            titles.push(if title.len() > 100 {
                format!("{} x", title.len())
            } else {
                title
            });
        }
    }
    // Nothing more comes.
    let late: Vec<_> = std::iter::from_fn(|| {
        next_message(&mut web_contents, &host_rx, Duration::from_millis(300)).ok()
    })
    .filter_map(|msg| match msg {
        RendererHostMsg::UpdateTitle(title) => Some(title),
        _ => None,
    })
    .collect();
    assert_eq!(titles, ["First", "Second", "Third", "5000 x", ""]);
    assert!(late.is_empty(), "{late:?}");
    assert_eq!(
        long.map(|title| title.chars().count()),
        Some(browser::MAX_TITLE_CHARS)
    );
    web_contents.shutdown();
}

/// A `srcdoc` document, which commits in a task of its own, commits with
/// nothing else to wake the renderer, though script set it late in the
/// loop's turn (a message's listener, which runs after the turn's queued
/// commits): no `BeginFrame` (no one plays Viz), no timer, and the page's
/// parser waits for a script whose response never comes, so no
/// `DOMContentLoaded` and no network-quiet timer either. The child's
/// script sets the page's title, which the browser hears.
#[test]
fn a_srcdoc_document_commits_with_nothing_else_to_wake_the_renderer() {
    let page = r#"<!DOCTYPE html>
<iframe></iframe>
<script>
onmessage = () => {
  document.querySelector('iframe').srcdoc =
      "<script>parent.document.title = 'from the srcdoc'<\/script>";
};
postMessage('go', '*');
</script>
<script src="/held.js"></script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/held.js",
            Route::ok("text/javascript", "").with_delay(Duration::from_secs(600)),
        ),
    ]);
    let (mut web_contents, host_rx, _viz_rx, _begin_frames) = tab_with_viz();
    web_contents.navigate(server.url("/"));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let msg = next_message(&mut web_contents, &host_rx, left)
            .expect("the srcdoc document's script sets the title");
        web_contents.on_renderer_message(&msg);
        if matches!(&msg, RendererHostMsg::UpdateTitle(title) if title == "from the srcdoc") {
            break;
        }
    }
    web_contents.shutdown();
}

/// A title the `load` handler sets reaches the browser before DevTools
/// hears the `load`: a client that asks for the title then (WebDriver's,
/// after a navigation) has it.
#[test]
fn the_title_goes_before_devtools_hears_the_load() {
    let page = "<!DOCTYPE html><body onload=\"document.title = 'One'\">";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let (mut web_contents, host_rx) = tab();
    web_contents.devtools_attach("s");
    for command in [
        r#"{"id":1,"method":"Page.enable"}"#,
        r#"{"id":2,"method":"Page.setLifecycleEventsEnabled","params":{"enabled":true}}"#,
    ] {
        web_contents.devtools_message("s", command.into(), false);
    }
    web_contents.navigate(server.url("/"));
    let mut titles = Vec::new();
    loop {
        let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("renderer answers");
        web_contents.on_renderer_message(&msg);
        match msg {
            RendererHostMsg::UpdateTitle(title) => titles.push(title),
            RendererHostMsg::DevToolsMessage { message, .. }
                if message.contains("\"Page.lifecycleEvent\"")
                    && message.contains("\"name\":\"load\"") =>
            {
                break;
            }
            _ => {}
        }
    }
    assert_eq!(titles.last().map(String::as_str), Some("One"), "{titles:?}");
    web_contents.shutdown();
}

/// The platform's system colors the browser gives its renderer are what
/// a page's style and its canvas's colors compute to.
#[test]
fn the_renderer_has_the_browsers_system_colors() {
    let page = r#"<!DOCTYPE html>
<p style="color: ButtonText; background-color: field">text</p>
<canvas></canvas>
<script>
onload = () => {
  const style = getComputedStyle(document.querySelector('p'));
  const ctx = document.querySelector('canvas').getContext('2d');
  ctx.fillStyle = 'GrayText';
  document.title = [style.color, style.backgroundColor, ctx.fillStyle].join(' / ');
};
</script>"#;
    let server = server(&[("/", Route::ok("text/html", page))]);
    let colors = gfx::SystemColors {
        button_text: gfx::Color::rgb(1, 2, 3),
        field: gfx::Color::rgb(4, 5, 6),
        gray_text: gfx::Color::rgb(7, 8, 9),
        ..gfx::SystemColors::DEFAULT
    };
    let (mut web_contents, host_rx) = tab_for_colors(false, colors);
    web_contents.navigate(server.url("/"));
    let title = loop {
        let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("renderer answers");
        web_contents.on_renderer_message(&msg);
        // The new document's empty one first.
        if let RendererHostMsg::UpdateTitle(title) = msg
            && !title.is_empty()
        {
            break title;
        }
    };
    assert_eq!(title, "rgb(1, 2, 3) / rgb(4, 5, 6) / #070809");
    web_contents.shutdown();
}

/// `about:gpucrash` has Viz lose the GPU device, and navigates nowhere
/// (Chromium's debug URLs).
#[test]
fn the_gpu_crash_url_loses_the_gpu_and_navigates_nowhere() {
    let (mut web_contents, _host_rx, viz_rx, _begin_frames) = tab_with_viz();
    web_contents.navigate("about:gpucrash");
    assert_eq!(web_contents.latest_navigation_id(), 0);
    assert_eq!(web_contents.url(), None);
    let sent: Vec<VizMsg> = viz_rx.try_iter().collect();
    let lost = sent
        .iter()
        .filter(|msg| matches!(msg, VizMsg::Display(DisplayMsg::LoseContext)))
        .count();
    assert_eq!(lost, 1, "{sent:?}");
    web_contents.shutdown();
}

/// Viz's GPU, played by the test: a shared image is the pixels it was
/// made from, remembered to see when it is let go; `lose` plays Viz
/// losing the device.
#[derive(Default)]
struct RecordingGpu {
    mailboxes: AtomicSequenceNumber<u64>,
    made: Mutex<Vec<Weak<Vec<u8>>>>,
    observers: Mutex<Vec<Weak<dyn ContextLostObserver>>>,
}

impl RecordingGpu {
    /// The shared images something still holds.
    fn alive(&self) -> usize {
        let made = self.made.lock().unwrap();
        made.iter().filter(|image| image.strong_count() > 0).count()
    }

    /// Tells the observers still there; how many.
    fn lose(&self) -> usize {
        let observers: Vec<_> = {
            let observers = self.observers.lock().unwrap();
            observers.iter().filter_map(Weak::upgrade).collect()
        };
        for observer in &observers {
            observer.on_context_lost();
        }
        observers.len()
    }
}

impl SharedImageInterface for RecordingGpu {
    fn create_shared_image(&self, _: gfx::Size, pixels: &[u8]) -> Option<SharedImage> {
        let pixels = Arc::new(pixels.to_vec());
        self.made.lock().unwrap().push(Arc::downgrade(&pixels));
        let mailbox = Mailbox(self.mailboxes.get_next());
        Some(SharedImage::new(mailbox, pixels))
    }

    fn add_context_lost_observer(&self, observer: Weak<dyn ContextLostObserver>) {
        self.observers.lock().unwrap().push(observer);
    }
}

/// When Viz loses the GPU, the browser (the one observer, not the
/// renderer) tells the renderer's compositor with a message, and the
/// compositor lets go of every shared image it made.
#[test]
fn losing_the_gpu_drops_the_renderers_shared_images() {
    let gpu = Arc::new(RecordingGpu::default());
    let (mut web_contents, host_rx, viz_rx, begin_frames) =
        tab_with_viz_gpu(true, false, Some(gpu.clone()));
    web_contents.resize(gfx::Size::new(200, 200), 1.0);
    web_contents.navigate("data:text/html,<p>tiles");
    let mut viz = DrivenViz::new(viz_rx, begin_frames);
    viz.until_loaded(&mut web_contents, &host_rx);
    viz.run_until(&mut web_contents, &host_rx, "tiles on the GPU", |_, _| {
        gpu.alive() > 0
    });
    assert_eq!(gpu.lose(), 1, "only the browser watches the GPU");
    let deadline = std::time::Instant::now() + TIMEOUT;
    while gpu.alive() > 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "the compositor kept its shared images"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    web_contents.shutdown();
}

/// A link to a new tab isn't navigated to here: the embedder opens it,
/// once it passes the same checks (no other schemes, no local file from a
/// page that isn't one).
#[test]
fn a_link_to_a_new_tab_is_handed_to_the_embedder() {
    let (mut web_contents, _host_rx, _viz_rx, _begin_frames) = tab_with_viz();
    let open = |web_contents: &mut WebContents, url: &str, disposition| {
        web_contents.on_renderer_message(&RendererHostMsg::OpenUrl {
            url: url.into(),
            disposition,
            replace: false,
        });
    };
    open(
        &mut web_contents,
        "https://a.test/",
        WindowOpenDisposition::NewBackgroundTab,
    );
    open(
        &mut web_contents,
        "https://b.test/",
        WindowOpenDisposition::NewForegroundTab,
    );
    open(
        &mut web_contents,
        "mailto:a@b.test",
        WindowOpenDisposition::NewForegroundTab,
    );
    open(
        &mut web_contents,
        "file:///etc/hosts",
        WindowOpenDisposition::NewForegroundTab,
    );
    assert_eq!(web_contents.latest_navigation_id(), 0, "not here");
    assert_eq!(
        web_contents.take_opened(),
        [
            OpenUrlParams {
                url: "https://a.test/".into(),
                disposition: WindowOpenDisposition::NewBackgroundTab,
            },
            OpenUrlParams {
                url: "https://b.test/".into(),
                disposition: WindowOpenDisposition::NewForegroundTab,
            },
        ]
    );
    assert_eq!(web_contents.take_opened(), []);
}

/// A page may not navigate to a `data:` URL, here or in a new tab
/// (Chromium's `DataUrlNavigationThrottle`); the user may.
#[test]
fn a_page_may_not_navigate_to_a_data_url() {
    let (mut web_contents, _host_rx, _viz_rx, _begin_frames) = tab_with_viz();
    for disposition in [
        WindowOpenDisposition::CurrentTab,
        WindowOpenDisposition::NewForegroundTab,
    ] {
        web_contents.on_renderer_message(&RendererHostMsg::OpenUrl {
            url: "data:text/html,x".into(),
            disposition,
            replace: false,
        });
    }
    assert_eq!(web_contents.latest_navigation_id(), 0);
    assert_eq!(web_contents.take_opened(), []);
    web_contents.navigate("data:text/html,x");
    assert_eq!(web_contents.latest_navigation_id(), 1);
    web_contents.shutdown();
}

/// A link to `about:gpucrash` is navigated to like any other URL (and
/// fails): a page can't crash the GPU (Chromium's debug URLs are only
/// for explicit navigations).
#[test]
fn a_link_to_the_gpu_crash_url_is_only_a_navigation() {
    let (mut web_contents, _host_rx, viz_rx, _begin_frames) = tab_with_viz();
    web_contents.on_renderer_message(&RendererHostMsg::OpenUrl {
        url: "about:gpucrash".into(),
        disposition: WindowOpenDisposition::CurrentTab,
        replace: false,
    });
    assert_eq!(web_contents.latest_navigation_id(), 1);
    let sent: Vec<VizMsg> = viz_rx.try_iter().collect();
    assert!(
        !sent
            .iter()
            .any(|msg| matches!(msg, VizMsg::Display(DisplayMsg::LoseContext))),
        "{sent:?}"
    );
    web_contents.shutdown();
}

#[test]
fn built_in_pages() {
    assert_eq!(navigate("about:demo").1, "machopo demo");
    assert_eq!(navigate("about:blank").1, "");
}

#[test]
fn failed_loads_commit_an_error_page() {
    assert_eq!(
        navigate("file:///definitely/not/here.html").1,
        "ERR_FILE_NOT_FOUND"
    );
    assert_eq!(
        navigate(&format!("http://127.0.0.1:{}/", unused_port())).1,
        "ERR_CONNECTION_REFUSED"
    );
    assert_eq!(
        navigate("gopher://example.com/").1,
        "ERR_UNKNOWN_URL_SCHEME"
    );
}

#[test]
fn the_last_navigation_wins() {
    let (mut web_contents, host_rx) = tab();
    // The first goes to the network, the second commits at once; the
    // first's commit then arrives last and must be dropped.
    let server = server(&[("/slow", Route::ok("text/html", "<title>first</title>"))]);
    web_contents.navigate(server.url("/slow"));
    web_contents.navigate("about:demo");
    let (id, _, title) = next_commit(&mut web_contents, &host_rx);
    assert_eq!((id, title.as_str()), (2, "machopo demo"));
    // Nothing else commits.
    let late = next_message(&mut web_contents, &host_rx, Duration::from_secs(2));
    assert!(
        !matches!(late, Ok(RendererHostMsg::DidCommitNavigation { .. })),
        "{late:?}"
    );
    web_contents.shutdown();
}

mod links {
    //! Links the renderer follows, and the history they make: input goes
    //! to the renderer, which hit tests the page it laid out; and when the
    //! browser hears the input was handled.

    use protocol::input::{InputEvent, Modifiers, MouseButton, ScrollDelta};
    use protocol::renderer::CursorType;

    use super::*;

    /// Links as 50px-high blocks, one under another.
    fn page(links: &[&str]) -> String {
        let links: String = links
            .iter()
            .map(|href| format!("<a href='{href}'></a>"))
            .collect();
        format!(
            "<!DOCTYPE html><style>html, body {{ margin: 0 }} a {{ display: block; height: 50px }}</style>{links}<div id=x></div>"
        )
    }

    /// Hands every message to `web_contents` and returns the first that
    /// `wanted` picks.
    fn wait_for<T>(
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        mut wanted: impl FnMut(&RendererHostMsg) -> Option<T>,
    ) -> T {
        loop {
            let msg = next_message(web_contents, host_rx, TIMEOUT).expect("renderer answers");
            web_contents.on_renderer_message(&msg);
            if let Some(found) = wanted(&msg) {
                return found;
            }
        }
    }

    /// Navigates and waits until the page is laid out, so input can hit
    /// it.
    /// `about:crash` ends the renderer, and nothing more comes from it; the
    /// tab stays, and its next navigation launches another renderer, with
    /// the history it had and a newer surface for Viz.
    #[test]
    fn the_next_navigation_after_a_crash_launches_a_renderer() {
        let server = server(&[("/", Route::ok("text/html", page(&["/a"])))]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        let before = web_contents.surface_id();
        web_contents.navigate("about:crash");
        assert!(web_contents.is_crashed());
        assert_eq!(web_contents.url(), Some(server.url("/").as_str()));
        // Not even `RendererGone`: the tab knows.
        while let Ok(msg) = next_message(&mut web_contents, &host_rx, Duration::from_millis(300)) {
            assert!(!matches!(msg, RendererHostMsg::RendererGone), "{msg:?}");
        }
        web_contents.reload();
        assert!(!web_contents.is_crashed());
        wait_until_loaded(&mut web_contents, &host_rx);
        assert_eq!(history(&web_contents), (vec![server.url("/")], Some(0)));
        assert!(web_contents.surface_id().local_surface_id > before.local_surface_id);
        // A renderer that goes on its own: the same.
        web_contents.on_renderer_message(&RendererHostMsg::RendererGone);
        assert!(web_contents.is_crashed());
        load(&mut web_contents, &host_rx, &server.url("/a"));
        assert!(!web_contents.is_crashed());
        web_contents.shutdown();
    }

    fn load(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>, url: &str) {
        web_contents.navigate(url);
        wait_until_loaded(web_contents, host_rx);
    }

    fn wait_until_loaded(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>) {
        let latest = web_contents.latest_navigation_id();
        wait_for(web_contents, host_rx, |msg| match msg {
            RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == latest => {
                Some(())
            }
            _ => None,
        });
    }

    fn press(web_contents: &mut WebContents, x: f32, y: f32, button: MouseButton) {
        web_contents.forward_input_event(InputEvent::mouse_down(x, y, button, Modifiers::NONE));
    }

    fn release(web_contents: &mut WebContents, x: f32, y: f32, button: MouseButton) {
        web_contents.forward_input_event(InputEvent::mouse_up(x, y, button, Modifiers::NONE));
    }

    fn click(web_contents: &mut WebContents, x: f32, y: f32) {
        press(web_contents, x, y, MouseButton::Left);
        release(web_contents, x, y, MouseButton::Left);
    }

    fn committed(msg: &RendererHostMsg) -> Option<String> {
        match msg {
            RendererHostMsg::DidCommitNavigation { url, .. } => Some(url.clone()),
            _ => None,
        }
    }

    fn opened(msg: &RendererHostMsg) -> Option<String> {
        match msg {
            RendererHostMsg::OpenUrl { url, .. } => Some(url.clone()),
            _ => None,
        }
    }

    fn history(web_contents: &WebContents) -> (Vec<String>, Option<usize>) {
        let controller = web_contents.controller();
        (
            controller.entries().iter().map(|e| e.url.clone()).collect(),
            controller.current_index(),
        )
    }

    #[test]
    fn a_link_navigates_and_history_goes_back_and_forward() {
        let server = server(&[
            ("/", Route::ok("text/html", page(&["two"]))),
            ("/two", Route::ok("text/html", page(&[]))),
        ]);
        let (one, two) = (server.url("/"), server.url("/two"));
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &one);
        click(&mut web_contents, 10.0, 10.0);
        // The renderer asks; the browser navigates.
        assert_eq!(wait_for(&mut web_contents, &host_rx, opened), two);
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), two);
        assert_eq!(
            history(&web_contents),
            (vec![one.clone(), two.clone()], Some(1))
        );
        let controller = web_contents.controller();
        assert!(controller.can_go_back() && !controller.can_go_forward());

        assert!(web_contents.go_back());
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), one);
        assert_eq!(
            history(&web_contents),
            (vec![one.clone(), two.clone()], Some(0))
        );
        assert!(!web_contents.go_back());
        assert!(web_contents.go_forward());
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), two);
        web_contents.reload();
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), two);
        assert_eq!(history(&web_contents), (vec![one, two], Some(1)));
        // Back, forward and reload each loaded the page again.
        let loads = |path: &str| {
            server
                .requests()
                .iter()
                .filter(|r| r.target == path)
                .count()
        };
        assert_eq!((loads("/"), loads("/two")), (2, 3));
        web_contents.shutdown();
    }

    /// A click is the primary button going down and up; it goes to the
    /// nearest element around both, and follows a link only if that is in
    /// one. The middle button's is a link's too, to a tab behind.
    #[test]
    fn a_click_is_the_primary_button_down_and_up_on_one_link() {
        let server = server(&[("/", Route::ok("text/html", page(&["/a", "/b"])))]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        // Down on `/b`, up on `/a`: the click goes to `<body>`.
        press(&mut web_contents, 10.0, 60.0, MouseButton::Left);
        release(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        // The primary one going down and the middle one coming up: none.
        press(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        release(&mut web_contents, 10.0, 10.0, MouseButton::Middle);
        // Below the links: nothing to follow.
        click(&mut web_contents, 10.0, 150.0);
        // The middle button on `/a`: a tab behind.
        press(&mut web_contents, 10.0, 10.0, MouseButton::Middle);
        release(&mut web_contents, 10.0, 10.0, MouseButton::Middle);
        // So the first link the renderer opens is `/a` behind, then `/b`
        // here, now clicked.
        click(&mut web_contents, 10.0, 60.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url("/a")
        );
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url("/b")
        );
        assert_eq!(
            web_contents
                .take_opened()
                .into_iter()
                .map(|params| (params.url, params.disposition))
                .collect::<Vec<_>>(),
            [(server.url("/a"), WindowOpenDisposition::NewBackgroundTab)]
        );
        web_contents.shutdown();
    }

    /// The click is an event: a listener or a handler canceling it keeps
    /// the link from being followed.
    #[test]
    fn script_can_cancel_a_click_on_a_link() {
        let html = "<!DOCTYPE html><style>html, body { margin: 0 } a { display: block; height: 50px }</style>\
             <a href='/a' onclick='return false'></a>\
             <a href='/b' id=b></a>\
             <a href='/c'></a>\
             <script>document.getElementById('b').addEventListener('click', (e) => e.preventDefault())</script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        click(&mut web_contents, 10.0, 10.0);
        click(&mut web_contents, 10.0, 60.0);
        click(&mut web_contents, 10.0, 110.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url("/c")
        );
        web_contents.shutdown();
    }

    /// Script follows a link by sending it a `click` (only a `MouseEvent`
    /// does); the page sees `DOMContentLoaded` after its deferred script,
    /// then `load`, at the document.
    #[test]
    fn script_follows_links_and_sees_the_load_events() {
        let html = "<!DOCTYPE html><a id=out></a>\
             <script>\
               const log = [];\
               document.addEventListener('DOMContentLoaded', (e) =>\
                 log.push(`dcl:${e.target === document}:${e.isTrusted}:${e.bubbles}`));\
               addEventListener('load', (e) => {\
                 log.push(`load:${e.target === document}:${e.isTrusted}`);\
                 const out = document.getElementById('out');\
                 out.setAttribute('href', '/done?' + log.join(','));\
                 out.dispatchEvent(new Event('click'));\
                 out.dispatchEvent(new MouseEvent('click'));\
               });\
             </script>\
             <script defer src='/defer.js'></script>";
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            (
                "/defer.js",
                Route::ok("text/javascript", "log.push('defer')"),
            ),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url("/done?defer,dcl:true:true:true,load:true:true")
        );
        web_contents.shutdown();
    }

    /// HTML's document readiness: loading while it parses, interactive
    /// once parsed and before its deferred scripts run, complete just
    /// before `load`; `readystatechange` (trusted, not bubbling) at each
    /// change. A document made otherwise (an iframe's initial
    /// `about:blank`, `createHTMLDocument`'s) is complete.
    #[test]
    fn the_document_readiness_moves_on_as_it_loads() {
        let html = "<!DOCTYPE html><a id=out></a><iframe></iframe>\
             <script>\
               const log = [document.readyState,\
                 document.querySelector('iframe').contentDocument.readyState,\
                 document.implementation.createHTMLDocument('').readyState];\
               document.addEventListener('readystatechange', (e) =>\
                 log.push(`rsc:${document.readyState}:${e.isTrusted}:${e.bubbles}`));\
               document.addEventListener('DOMContentLoaded', () => log.push('dcl'));\
               addEventListener('load', () => {\
                 log.push(`load:${document.readyState}`);\
                 const out = document.getElementById('out');\
                 out.setAttribute('href', '/done?' + log.join(','));\
                 out.dispatchEvent(new MouseEvent('click'));\
               });\
             </script>\
             <script defer src='/defer.js'></script>";
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            (
                "/defer.js",
                Route::ok(
                    "text/javascript",
                    "log.push(`defer:${document.readyState}`)",
                ),
            ),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url(
                "/done?loading,complete,complete,rsc:interactive:true:false,\
                 defer:interactive,dcl,rsc:complete:true:false,load:complete"
            )
        );
        web_contents.shutdown();
    }

    /// A tab's documents have its visibility. Hidden before it commits
    /// anything, its first document starts hidden (a document no frame
    /// shows always is). Each change fires `visibilitychange` (trusted,
    /// bubbling, not cancelable) at every document, the main frame's
    /// first, all of them in the new state already; being told the state
    /// it has fires nothing. (The page's title says what it started with:
    /// following a link would navigate away.)
    #[test]
    fn documents_have_the_tabs_visibility() {
        let html = "<!DOCTYPE html><a id=out></a><iframe></iframe>\
             <script>\
               const child = document.querySelector('iframe').contentDocument;\
               document.title = [document.visibilityState, document.hidden,\
                 child.visibilityState, child.hidden,\
                 document.implementation.createHTMLDocument('').visibilityState].join();\
               const log = [];\
               const record = (name) => (e) => log.push(\
                 `${name}:${document.visibilityState}:${child.visibilityState}:\
${e.isTrusted}:${e.bubbles}:${e.cancelable}`);\
               document.addEventListener('visibilitychange', record('main'));\
               child.addEventListener('visibilitychange', record('child'));\
               child.addEventListener('visibilitychange', () => {\
                 if (log.length < 4) return;\
                 const out = document.getElementById('out');\
                 out.setAttribute('href', '/done?' + log.join(','));\
                 out.dispatchEvent(new MouseEvent('click'));\
               });\
             </script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        web_contents.was_hidden();
        web_contents.navigate(server.url("/"));
        let title = |msg: &RendererHostMsg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        };
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, title),
            "hidden,true,hidden,true,hidden"
        );
        web_contents.was_hidden();
        web_contents.was_shown();
        web_contents.was_shown();
        web_contents.was_hidden();
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url(
                "/done?main:visible:visible:true:true:false,child:visible:visible:true:true:false,\
                 main:hidden:hidden:true:true:false,child:hidden:hidden:true:true:false"
            )
        );
        web_contents.shutdown();
    }

    /// `<img>` and `Image()` as Chrome 152 has them (the page,
    /// `resources/img-elements.html`, ran there, and each line is what it
    /// logged): `complete` and the natural size as it loads, one in the
    /// memory cache available at once (its `load` still in a task), broken,
    /// missing, empty, removed and unparsable `src`s, `width` and `height`
    /// rendered or not, `decode()` (and one whose `src` is replaced while
    /// it loads, or set to the same again: it still resolves, as the image
    /// it waits for is the same), two updates in one task (one event), a
    /// `src` replaced
    /// while it loads, the same `src` again, the reflected members and
    /// `Image`.
    #[test]
    fn img_elements() {
        let png = |name: &str| {
            std::fs::read(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../tests/reftests/images/resources")
                    .join(name),
            )
            .unwrap()
        };
        let server = server(&[
            (
                "/p.html",
                Route::ok("text/html", include_str!("../resources/img-elements.html")),
            ),
            ("/a.png", Route::ok("image/png", png("green-40x30.png"))),
            ("/b.png", Route::ok("image/png", png("green-20x40.png"))),
            ("/bad.png", Route::ok("image/png", "not an image")),
            (
                "/slow.png",
                Route::ok("image/png", png("green-40x30.png"))
                    .with_delay(Duration::from_millis(300)),
            ),
            (
                "/slow2.png",
                Route::ok("image/png", png("green-40x30.png"))
                    .with_delay(Duration::from_millis(300)),
            ),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.resize(gfx::Size::new(400, 300), 1.0);
        web_contents.navigate(server.url("/p.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if title.starts_with("new Image=") => {
                Some(title.clone())
            }
            _ => None,
        });
        let expected = [
            r#"new Image=[[true,0,0,0,0,"",""],"<img>"]"#,
            r#"new Image(5,7)=[[true,0,0,5,7,"",""],"<img width=\"5\" height=\"7\">"]"#,
            r#"after src sync=[false,0,0,0,0,"","/a.png"]"#,
            r#"after tick=[false,0,0,0,0,"","/a.png"]"#,
            r#"after load=[[true,40,30,40,30,"/a.png","/a.png"],["load"]]"#,
            r#"cached sync=[true,40,30,40,30,"/a.png","/a.png"]"#,
            r#"cached tick=[[true,40,30,40,30,"/a.png","/a.png"],["load"]]"#,
            r#"bad=[[true,0,0,0,0,"/bad.png","/bad.png"],["error"]]"#,
            r#"missing=[[true,0,0,0,0,"/missing.png","/missing.png"],["error"]]"#,
            r#"empty sync=[true,0,0,0,0,"","/p.html"]"#,
            r#"empty=[[true,0,0,0,0,"","/p.html"],["error"]]"#,
            r#"removed sync=[true,0,0,0,0,"",""]"#,
            r#"removed=[[true,0,0,0,0,"",""],[]]"#,
            r#"invalid=[[true,0,0,0,0,"http://[","http://["],["error"]]"#,
            r#"width attr unrendered=[true,20,40,33,40,"/b.png","/b.png"]"#,
            r#"rendered=[true,20,40,33,66,"/b.png","/b.png"]"#,
            r#"rendered styled=[true,20,40,50,100,"/b.png","/b.png"]"#,
            r#"display none=[true,20,40,33,40,"/b.png","/b.png"]"#,
            r#"bad dims=[true,20,40,20,40,"/b.png","/b.png"]"#,
            r#"set width="12""#,
            r#"decode=["decoded true"]"#,
            r#"decode bad=["EncodingError:The source image cannot be decoded."]"#,
            r#"decode no src=["EncodingError:The source image cannot be decoded."]"#,
            r#"decode changed=["EncodingError:The source image cannot be decoded."]"#,
            r#"empty twice=["error"]"#,
            r#"bad twice=["error"]"#,
            r#"decode loading then src=["EncodingError"]"#,
            r#"decode loading then same src=[["ok true"],["load"]]"#,
            r#"change during load=[["load b.png"],[true,20,40,20,40,"/b.png","/b.png"]]"#,
            r#"same src again=["load"]"#,
            r#"refl=["","anonymous","lazy","sync",false,"","","","","auto","","","",0,0,"",""]"#,
            r#"props="align alt border complete constructor crossOrigin currentSrc decode decoding fetchPriority height hspace isMap loading longDesc lowsrc name naturalHeight naturalWidth referrerPolicy sizes src srcset useMap vspace width""#,
            r#"Image=["Image",0,true,"function"]"#,
        ];
        let lines: Vec<&str> = title.split(" | ").collect();
        for (line, expected) in lines.iter().zip(expected) {
            assert_eq!(*line, expected);
        }
        assert_eq!(lines.len(), expected.len(), "{title}");
        web_contents.shutdown();
    }

    /// The page with `srcset`s and `<picture>`s
    /// (`resources/srcset.html`) on a test server, each candidate an image
    /// of its own (40×30).
    fn srcset_server() -> TestServer {
        let png = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/reftests/images/resources/green-40x30.png"),
        )
        .unwrap();
        let mut routes = vec![(
            "/srcset.html".to_owned(),
            Route::ok("text/html", include_str!("../resources/srcset.html")),
        )];
        for name in [
            "x1", "x2", "x3", "s", "w200", "w400", "w800", "w1600", "wide", "narrow", "fallback",
            "avif", "png", "p100", "p300", "p600", "big", "small",
        ] {
            routes.push((format!("/{name}.png"), Route::ok("image/png", png.clone())));
        }
        TestServer::start(routes.into_iter().collect()).expect("test server starts")
    }

    /// The page's title once it starts with `prefix`, as lines.
    fn title_lines(
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        prefix: &str,
    ) -> Vec<String> {
        let title = wait_for(web_contents, host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if title.starts_with(prefix) => {
                Some(title[prefix.len()..].to_owned())
            }
            _ => None,
        });
        title.split(" | ").map(str::to_owned).collect()
    }

    /// What `<img srcset>` and `<picture>` select, and their natural and
    /// laid-out sizes (the density-corrected ones), as Chrome 152 does at
    /// 800×457 at 1x, then at 500×457 once resized (it selects again),
    /// then once a `<source>`'s `media` stops matching. Each line is what
    /// Chrome logged there, but for one: Chrome decodes AVIF, so its
    /// `<picture>` takes the `type="image/avif"` source; machopo skips it for
    /// the PNG one after.
    ///
    /// Once resized, `c4`, `c5` and `c8` keep the denser candidate already
    /// loaded (`w800.png`, `p300.png`) instead of fetching the one their
    /// new size asks for, as Blink's selection does. Chrome did so in about
    /// half of the runs; in the others it fetched `w200.png` and
    /// `p100.png`.
    #[test]
    fn srcset_and_picture() {
        let server = srcset_server();
        let (mut web_contents, host_rx) = tab();
        web_contents.resize(gfx::Size::new(800, 457), 1.0);
        web_contents.navigate(server.url("/srcset.html"));
        let expected = [
            r#"[800,457,1]"#,
            r#"c1=["x1.png",40,30,40,30,true]"#,
            r#"c2=["s.png",40,30,40,30,true]"#,
            r#"c3=["w800.png",40,30,40,30,true]"#,
            r#"c4=["w400.png",40,30,40,30,true]"#,
            r#"c5=["w200.png",30,22,30,23,true]"#,
            r#"c6=["wide.png",40,30,40,30,true]"#,
            r#"c7=["png.png",40,30,40,30,true]"#,
            r#"c8=["p300.png",40,30,40,30,true]"#,
            r#"c9=["s.png",40,30,40,30,true]"#,
            r#"c10=["",0,0,0,0,true]"#,
            r#"c11=["big.png",20,15,20,15,true]"#,
            r#"c12=["fallback.png",40,30,40,30,true]"#,
        ];
        assert_eq!(title_lines(&mut web_contents, &host_rx, "1 "), expected);
        web_contents.resize(gfx::Size::new(500, 457), 1.0);
        let expected = [
            r#"[500,457,1]"#,
            r#"c1=["x1.png",40,30,40,30,true]"#,
            r#"c2=["s.png",40,30,40,30,true]"#,
            r#"c3=["w800.png",25,18,25,19,true]"#,
            r#"c4=["w800.png",5,3,5,4,true]"#,
            r#"c5=["w800.png",7,5,8,6,true]"#,
            r#"c6=["narrow.png",40,30,40,30,true]"#,
            r#"c7=["png.png",40,30,40,30,true]"#,
            r#"c8=["p300.png",13,10,13,10,true]"#,
            r#"c9=["s.png",40,30,40,30,true]"#,
            r#"c10=["",0,0,0,0,true]"#,
            r#"c11=["big.png",20,15,20,15,true]"#,
            r#"c12=["fallback.png",40,30,40,30,true]"#,
        ];
        assert_eq!(title_lines(&mut web_contents, &host_rx, "2 "), expected);
        assert_eq!(
            title_lines(&mut web_contents, &host_rx, "3 "),
            [r#"c6 after media=["fallback.png",40,30,40,30,true]"#]
        );
        web_contents.shutdown();
    }

    /// The same page at 2x, as Chrome 152 at 800×457 at 2x. Chrome's `c4`
    /// took `w1600.png` in some runs: its preload scanner had fetched it
    /// for `c3` before `c4` selected, and the denser candidate already
    /// loaded wins. machopo has no preload scanner; nothing has loaded when
    /// the page's images select, as in Chrome's other runs.
    #[test]
    fn srcset_and_picture_at_2x() {
        let server = srcset_server();
        let (mut web_contents, host_rx) = tab();
        web_contents.resize(gfx::Size::new(1600, 914), 2.0);
        web_contents.navigate(server.url("/srcset.html"));
        let expected = [
            r#"[800,457,2]"#,
            r#"c1=["x2.png",20,15,20,15,true]"#,
            r#"c2=["x2.png",20,15,20,15,true]"#,
            r#"c3=["w1600.png",20,15,20,15,true]"#,
            r#"c4=["w800.png",20,15,20,15,true]"#,
            r#"c5=["w400.png",15,11,15,11,true]"#,
            r#"c6=["wide.png",40,30,40,30,true]"#,
            r#"c7=["png.png",40,30,40,30,true]"#,
            r#"c8=["p600.png",20,15,20,15,true]"#,
            r#"c9=["s.png",40,30,40,30,true]"#,
            r#"c10=["",0,0,0,0,true]"#,
            r#"c11=["big.png",20,15,20,15,true]"#,
            r#"c12=["x2.png",20,15,20,15,true]"#,
        ];
        assert_eq!(title_lines(&mut web_contents, &host_rx, "1 "), expected);
        web_contents.shutdown();
    }

    /// `requestIdleCallback` as Chrome 152 has it (the page,
    /// `resources/idle.html`, ran there, and the line is what it logged):
    /// with nothing to draw, long idle periods (at most 50 ms left), a
    /// callback requested in one running in a later one; while frames are
    /// drawn, at most a frame's time left; a timeout that runs it while
    /// tasks keep the loop busy, `didTimeout` and nothing left; the order
    /// requested, but for a cancelled one; `IdleDeadline` itself.
    #[test]
    fn idle_callbacks() {
        let server = TestServer::start(
            [(
                "/idle.html".to_owned(),
                Route::ok("text/html", include_str!("../resources/idle.html")),
            )]
            .into_iter()
            .collect(),
        )
        .expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/idle.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        assert_eq!(
            title,
            "handle number | chain true false true | chain true false true | \
             chain true false true | animated true | animated true | timeout true true 0 true | \
             order abd | proto constructor didTimeout timeRemaining | new TypeError length 1 0 | \
             toString [object IdleDeadline]"
        );
        web_contents.shutdown();
    }

    /// Images on a canvas, drawn by the renderer and read back
    /// (`resources/canvas-images.html`): an `<img>` whole, scaled and in
    /// part, as a pattern repeated across only, and through another
    /// canvas; one of another origin (another port) drawn, after which the
    /// canvas's pixels, its `toDataURL` and its `toBlob` are a
    /// `SecurityError`.
    #[test]
    fn canvas_images() {
        let png = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/reftests/images/resources/green-40x30.png"),
        )
        .unwrap();
        let other = server(&[("/green.png", Route::ok("image/png", png.clone()))]);
        let page = server(&[
            (
                "/canvas-images.html",
                Route::ok("text/html", include_str!("../resources/canvas-images.html")),
            ),
            ("/green.png", Route::ok("image/png", png)),
        ]);
        let (mut web_contents, host_rx) = tab();
        let url = format!(
            "{}?other={}",
            page.url("/canvas-images.html"),
            other.url("/green.png")
        );
        web_contents.navigate(url);
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        assert_eq!(
            title,
            "whole 0,0,0,0 0,128,0,255 0,128,0,255 0,0,0,0 | \
             scaled 0,128,0,255 0,0,0,0 | part 0,128,0,255 0,0,0,0 | \
             pattern 0,128,0,255 0,0,0,0 | canvas 0,128,0,255 | \
             other SecurityError SecurityError SecurityError"
        );
        web_contents.shutdown();
    }

    /// A canvas's bitmap as an image file (`resources/canvas-export.html`):
    /// `toDataURL`'s PNG back through an `<img>` as the pixels it was, a
    /// JPEG's quality only as a number (a lower one smaller), WebP as PNG,
    /// and no pixels as `data:,`; `toBlob`'s the same files in a later
    /// task, of the bitmap when it was called, typed as encoded, and null
    /// for no pixels.
    #[test]
    fn canvas_export() {
        let server = TestServer::start(
            [(
                "/canvas-export.html".to_owned(),
                Route::ok("text/html", include_str!("../resources/canvas-export.html")),
            )]
            .into_iter()
            .collect(),
        )
        .expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/canvas-export.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        assert_eq!(
            title,
            "data:image/png;base64, | jpeg data:image/jpeg;base64, true true true | \
             webp data:image/png;base64, | empty data:, | blob false image/png true | \
             image/jpeg true | image/png null | back 255,0,0,255 0,0,255,128"
        );
        web_contents.shutdown();
    }

    /// A canvas's 2D context drawn by the renderer and read back
    /// (`resources/canvas.html`): `getImageData` draws what was recorded
    /// first; fills over fills, premultiplied and back; `clearRect` whatever
    /// `globalAlpha` is; `save` and `restore` of the state, the transforms;
    /// a stroked rect's outline; an opaque canvas's black; a new width
    /// clearing the bitmap and the state; transparent pixels out of the
    /// bitmap and a negative size; `data` the same array, keeping what a
    /// script writes; linear and conic gradients; a canvas read back twice
    /// drawn on the main thread after, with what it had; text filled and
    /// stroked where it is placed, squeezed into `maxWidth`, and measured
    /// where it is drawn.
    #[test]
    fn canvas_2d() {
        let server = TestServer::start(
            [(
                "/canvas.html".to_owned(),
                Route::ok("text/html", include_str!("../resources/canvas.html")),
            )]
            .into_iter()
            .collect(),
        )
        .expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/canvas.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        assert_eq!(
            title,
            "fill 255,0,0,255 127,0,128,255 0,0,0,0 | \
             alpha 0,255,0,128 0,255,0,255 0,0,0,0 0,255,0,128 | \
             transform 0,0,0,255 0,0,0,255 0,0,0,0 | \
             stroke 0,0,255,255 0,0,255,255 0,0,0,0 | \
             opaque 0,0,0,255 255,255,255,255 0,0,0,255 | \
             reset 0,0,0,255 0,0,0,0 #000000 | \
             bounds 3x1 0,0,0,0,0,0,0,255,0,0,0,255 | \
             same true 7 true | new 24 srgb | zero IndexSizeError | \
             triangle 0,0,0,255 0,0,0,0 | evenodd 0,0,0,255 0,0,0,0 | \
             circle 0,0,0,255 0,0,0,255 0,0,0,0 | \
             clip 0,0,0,0 0,0,0,255 0,0,0,0 0,0,0,255 | \
             dash 0,0,0,255 0,0,0,0 0,0,0,255 | round 0,0,0,0 0,0,0,255 | \
             caps 0,0,0,0 0,0,0,255 | \
             linear 254,0,1,255 126,0,129,255 1,0,254,255 | conic true true | \
             source-in 0,0,0,0 0,0,255,255 | xor 0,0,0,255 0,0,0,0 0,0,0,255 | \
             copy 0,0,0,0 0,255,0,255 | opaque-out 0,0,0,255 255,255,255,255 | \
             shadow 0,0,0,255 0,0,0,0 0,255,0,255 0,0,0,0 | blurred true | \
             path2d 0,0,0,255 0,0,0,0 true false true | clip2d 0,0,0,255 0,0,0,0 | \
             put 255,0,0,255 0,0,255,255 0,255,0,128 255,0,0,255 | \
             taken 255,0,0,255 255,0,0,255 255,0,0,255 0,0,255,255 0,255,0,255 255,0,0,255 | \
             text true true | squeezed true true | right true true | nan 200 -1 | stroked true | \
             measured true true true true true true"
        );
        web_contents.shutdown();
    }

    /// `Blob`, bodies of bytes and blob URLs as Chrome 152 has them (the
    /// page, `resources/blob.html`, ran there, and the line is what it
    /// logged): reading a blob settles in a task, after the microtasks
    /// queued before; a body's `blob()` type is its `Content-Type`,
    /// lowercased; `fetch()` of a blob URL gives the blob, its type and
    /// length, a range of it, not to a `POST`, and not once revoked, but a
    /// `fetch()`, a script or an iframe asked for before the revoke still
    /// gets it (an `<img>` resolves its URL later, after the revoke), and a
    /// script asked for in a later task doesn't; a
    /// child frame's blob URLs go with its document, and another document
    /// of the origin may revoke one; the page navigates to a blob URL it
    /// revoked at once, which is of its origin.
    #[test]
    fn blobs() {
        let server = TestServer::start(
            [(
                "/blob.html".to_owned(),
                Route::ok("text/html", include_str!("../resources/blob.html")),
            )]
            .into_iter()
            .collect(),
        )
        .expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/blob.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title)
                if title.contains("navigated") || title.starts_with("error") =>
            {
                Some(title.clone())
            }
            _ => None,
        });
        assert_eq!(
            title,
            "text \"h\u{e9}!\" micro,text | arrayBuffer ArrayBuffer 187,191,104 | bytes Uint8Array 7 true | \
             response blob 2 text/html; charset=\"utf-8\" true | \
             response bytes 97,98 239,187,191,104,195,169,33 | blob of no type \"\" | \
             request blob a/b 2,1 | \
             fetch 200 \"OK\" text/plain;charset=utf-8 5 basic true hello | \
             range 206 bytes 1-3/5 ell | fragment hello | post TypeError | no type \"\" | \
             revoked after hello | revoked before TypeError | other origin true | img 0x0 | \
             script 42 | script revoked before error | iframe child true | discarded x TypeError | \
             revoked by another TypeError | navigated true blob:"
        );
        // Revoked since: going there again is a network error.
        let url = web_contents.url().expect("the blob URL").to_owned();
        web_contents.navigate(url);
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if title.starts_with("ERR_") => Some(title.clone()),
            _ => None,
        });
        assert_eq!(title, "ERR_FILE_NOT_FOUND");
        web_contents.shutdown();
    }

    /// `<img loading=lazy>` as Chrome 152 has it (the page,
    /// `resources/lazy.html`, ran there at 800×513, and each line is what
    /// it logged): one in the viewport loads once laid out, after the
    /// window's `load` (it doesn't delay it); one far below waits
    /// (`complete` false, no `currentSrc`), and loads once scrolled near,
    /// as does one in an iframe below the viewport (the top-level viewport
    /// counts), and one beside it within 625px (not one further); `loading`
    /// set to eager loads it now, and only then; `sizes="auto"` is the
    /// laid-out width of a lazy one (its content box; laid out at another
    /// width, it selects again), and `100vw` (the whole `sizes`) of an
    /// eager one, laid out or not. What was fetched is what Chrome fetched: not the one with
    /// `display: none`, nor a lazy `new Image()` out of the document.
    #[test]
    fn lazy_images() {
        let png = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/reftests/images/resources/green-40x30.png"),
        )
        .unwrap();
        let mut routes = vec![
            (
                "/lazy.html".to_owned(),
                Route::ok("text/html", include_str!("../resources/lazy.html")),
            ),
            (
                "/lazy-child.html".to_owned(),
                Route::ok("text/html", include_str!("../resources/lazy-child.html")),
            ),
            (
                "/slow-a.png".to_owned(),
                Route::ok("image/png", png.clone()).with_delay(Duration::from_millis(500)),
            ),
        ];
        for name in [
            "none",
            "e50",
            "e51",
            "far",
            "far2",
            "w50",
            "w51",
            "disconnected",
            "in-frame",
            "side",
            "side2",
            "p50",
            "p51",
            "f50",
            "f51",
        ] {
            routes.push((format!("/{name}.png"), Route::ok("image/png", png.clone())));
        }
        let server = TestServer::start(routes.into_iter().collect()).expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.resize(gfx::Size::new(800, 457), 1.0);
        web_contents.navigate(server.url("/lazy.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        let lines: Vec<&str> = title.split(" | ").collect();
        assert_eq!(
            lines,
            [
                r#"far sync [false,"",0]"#,
                r#"window load, a [false,"",0]"#,
                "a load",
                r#"far [false,"",0] child [false,"",0] autopad [true,"p50.png",8]"#,
                r#"far2 after eager [true,"far2.png",40]"#,
                r#"autoeager [true,"e51.png",627] autoeager2 [true,"f51.png",627]"#,
                r#"after scroll far [true,"far.png",40] auto [true,"w50.png",8] child [true,"in-frame.png",40]"#,
                r#"auto at 100px [true,"w51.png",78] far2 loads 1 side [true,"side.png",40] side2 [false,"",0]"#,
            ]
        );
        let mut fetched: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
        fetched.sort();
        assert_eq!(
            fetched,
            [
                "/e51.png",
                "/f51.png",
                "/far.png",
                "/far2.png",
                "/in-frame.png",
                "/lazy-child.html",
                "/lazy.html",
                "/p50.png",
                "/side.png",
                "/slow-a.png",
                "/w50.png",
                "/w51.png",
            ]
        );
        web_contents.shutdown();
    }

    /// A child frame's `<picture>` selects for its iframe's size from the
    /// start, before the frame is laid out for a frame of its own: the
    /// narrow image, which a 0 by 0 viewport would select, is never
    /// fetched. Chrome 152 fetched only the wide one too.
    #[test]
    fn a_child_frames_images_select_for_its_iframes_size() {
        let main = r#"<!doctype html><body style="margin:0"><iframe src="child.html" style="width:300px;height:100px;border:0"></iframe><script>
addEventListener('load', () => {
  const i = frames[0].document.getElementById('i');
  document.title = i.currentSrc.split('/').pop() + ' ' + frames[0].innerWidth;
});
</script>"#;
        let child = r#"<!doctype html><picture><source media="(min-width: 200px)" srcset="wide.png"><img id=i src="narrow.png"></picture>"#;
        let png = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/reftests/images/resources/green-40x30.png"),
        )
        .unwrap();
        let server = TestServer::start(
            [
                ("/main.html".to_owned(), Route::ok("text/html", main)),
                ("/child.html".to_owned(), Route::ok("text/html", child)),
                ("/wide.png".to_owned(), Route::ok("image/png", png.clone())),
                ("/narrow.png".to_owned(), Route::ok("image/png", png)),
            ]
            .into_iter()
            .collect(),
        )
        .expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/main.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        assert_eq!(title, "wide.png 300");
        let paths: Vec<String> = server.requests().into_iter().map(|r| r.target).collect();
        assert!(!paths.iter().any(|path| path == "/narrow.png"), "{paths:?}");
        web_contents.shutdown();
    }

    /// A new iframe's initial `about:blank`, filled by its `onload` as the
    /// iframe is inserted (before the render thread has the frame), resolves
    /// URLs against its parent's base URL as of the iframe's creation, a
    /// `<base>` changed after it (in the same script, before the render
    /// thread has the frame, and later) notwithstanding: what Chrome 152
    /// gave.
    #[test]
    fn a_new_iframes_about_blank_has_its_parents_base_url() {
        let page = r#"<!doctype html><base href="/dir/"><body><script>
const iframe = document.createElement('iframe');
iframe.onload = () => {
  const doc = iframe.contentDocument;
  doc.body.innerHTML = '<a href="next.html"></a>';
  document.querySelector('base').setAttribute('href', '/other/');
  const later = doc.createElement('a');
  later.href = 'b.html';
  const out = [doc.querySelector('a').href, later.href];
  setTimeout(() => {
    const last = doc.createElement('a');
    last.href = 'c.html';
    out.push(last.href);
    document.title = out.join(' ');
  }, 100);
};
document.body.appendChild(iframe);
document.querySelector('base').setAttribute('href', '/later/');
</script>"#;
        let server = TestServer::start(
            [("/page.html".to_owned(), Route::ok("text/html", page))]
                .into_iter()
                .collect(),
        )
        .expect("test server starts");
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/page.html"));
        let title = wait_for(&mut web_contents, &host_rx, |msg| match msg {
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => Some(title.clone()),
            _ => None,
        });
        let dir = server.url("/dir/");
        assert_eq!(title, format!("{dir}next.html {dir}b.html {dir}c.html"));
        web_contents.shutdown();
    }

    /// An `<img>`'s request delays its document's `load` until the
    /// image's own `load` has fired, in the document or not (`new
    /// Image()`), as in Chrome 152: the window's comes last.
    #[test]
    fn img_loads_delay_the_window_load() {
        let png = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tests/reftests/images/resources/green-40x30.png"),
        )
        .unwrap();
        let html = "<!DOCTYPE html><a id=out></a><script>\
               const log = [];\
               const off = new Image(); off.onload = () => log.push('off-tree');\
               off.onerror = () => log.push('off-tree error');\
               off.src = '/slow1.png';\
               addEventListener('load', () => {\
                 log.push('window');\
                 const out = document.getElementById('out');\
                 out.setAttribute('href', '/done?' + log.join(','));\
                 out.dispatchEvent(new MouseEvent('click'));\
               });\
             </script>\
             <img src='/slow2.png' onload=\"log.push('in-document')\"\
               onerror=\"log.push('in-document error')\">";
        let slow = Route::ok("image/png", png).with_delay(Duration::from_millis(300));
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            ("/slow1.png", slow.clone()),
            ("/slow2.png", slow),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        let done = wait_for(&mut web_contents, &host_rx, opened);
        assert!(
            done == server.url("/done?off-tree,in-document,window")
                || done == server.url("/done?in-document,off-tree,window"),
            "{done}"
        );
        web_contents.shutdown();
    }

    /// `navigator.webdriver` is WebDriver's webdriver-active flag: true in
    /// a renderer automation controls, in a child frame too; false
    /// otherwise.
    #[test]
    fn navigator_webdriver_is_whether_automation_controls_the_browser() {
        let html = "<!DOCTYPE html><a id=out></a><iframe></iframe>\
             <script>\
               const child = document.querySelector('iframe').contentWindow.navigator;\
               const out = document.getElementById('out');\
               out.setAttribute('href', `/done?${navigator.webdriver},${child.webdriver}`);\
               out.dispatchEvent(new MouseEvent('click'));\
             </script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        for automation in [false, true] {
            let (mut web_contents, host_rx) = tab_for(automation);
            web_contents.navigate(server.url("/"));
            assert_eq!(
                wait_for(&mut web_contents, &host_rx, opened),
                server.url(&format!("/done?{automation},{automation}"))
            );
            web_contents.shutdown();
        }
    }

    /// `load` waits for the document's images: with the image's response
    /// 300 ms late, the page sees `load` at least that long after its
    /// script ran (the image is asked for before it runs). The page
    /// measures, so the browser's startup doesn't count.
    #[test]
    fn load_waits_for_images() {
        let html = "<!DOCTYPE html><a id=out></a><img src='/slow.png'>\
             <script>\
               const start = Date.now();\
               addEventListener('load', () => {\
                 const out = document.getElementById('out');\
                 out.setAttribute('href', '/loaded?' + (Date.now() - start >= 250));\
                 out.dispatchEvent(new MouseEvent('click'));\
               });\
             </script>";
        let png = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/reftests/images/resources/green-40x30.png"
        ))
        .expect("the test image");
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            (
                "/slow.png",
                Route::ok("image/png", png).with_delay(Duration::from_millis(300)),
            ),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            server.url("/loaded?true"),
            "load came before the image"
        );
        web_contents.shutdown();
    }

    /// Links script follows from a timer or an animation frame callback
    /// are followed too (each runs outside any message).
    #[test]
    fn links_followed_from_timers_and_animation_frames() {
        for schedule in ["setTimeout", "requestAnimationFrame"] {
            let html = format!(
                "<!DOCTYPE html><a href='/later' id=a></a><script>\
                   {schedule}(() => document.getElementById('a').dispatchEvent(new MouseEvent('click')));\
                 </script>"
            );
            let server = server(&[("/", Route::ok("text/html", html))]);
            let (mut web_contents, host_rx) = tab();
            web_contents.navigate(server.url("/"));
            assert_eq!(
                wait_for(&mut web_contents, &host_rx, opened),
                server.url("/later"),
                "{schedule}"
            );
            web_contents.shutdown();
        }
    }

    #[test]
    fn a_link_to_a_fragment_stays_in_the_document() {
        let server = server(&[("/", Route::ok("text/html", page(&["#x", "/#y"])))]);
        let (mut web_contents, host_rx) = tab();
        let url = server.url("/");
        load(&mut web_contents, &host_rx, &url);
        click(&mut web_contents, 10.0, 10.0);
        click(&mut web_contents, 10.0, 60.0);
        let same_document = |msg: &RendererHostMsg| match msg {
            RendererHostMsg::DidCommitSameDocumentNavigation { url, .. } => Some(url.clone()),
            RendererHostMsg::DidCommitNavigation { url, .. } => panic!("loaded {url}"),
            _ => None,
        };
        // `#x`, then `/#y`: the same document with another fragment.
        let x = wait_for(&mut web_contents, &host_rx, same_document);
        let y = wait_for(&mut web_contents, &host_rx, same_document);
        assert_eq!(
            (x.as_str(), y.as_str()),
            (&*format!("{url}#x"), &*format!("{url}#y"))
        );
        assert_eq!(history(&web_contents), (vec![url, x, y.clone()], Some(2)));
        assert_eq!(web_contents.url(), Some(y.as_str()));
        assert_eq!(server.requests().len(), 1);
        web_contents.shutdown();
    }

    #[test]
    fn a_web_page_opens_neither_local_files_nor_unknown_schemes() {
        let server = server(&[(
            "/",
            Route::ok(
                "text/html",
                page(&["file:///etc/hosts", "mailto:a@b.test", "javascript:void(0)"]),
            ),
        )]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        let before = web_contents.latest_navigation_id();
        // There is no script to run: the renderer doesn't even ask.
        click(&mut web_contents, 10.0, 110.0);
        click(&mut web_contents, 10.0, 10.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            "file:///etc/hosts"
        );
        click(&mut web_contents, 10.0, 60.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, opened),
            "mailto:a@b.test"
        );
        // The browser started no navigation for either.
        assert_eq!(web_contents.latest_navigation_id(), before);
        web_contents.shutdown();
    }

    #[test]
    fn a_local_file_may_open_another() {
        let dir = std::env::temp_dir().join(format!("machopo-links-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("one.html"), page(&["two.html"])).unwrap();
        std::fs::write(dir.join("two.html"), "<title>two</title>").unwrap();
        let url = |name: &str| browser::loader::file_url(&dir.join(name)).unwrap();
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &url("one.html"));
        click(&mut web_contents, 10.0, 10.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, committed),
            url("two.html")
        );
        web_contents.shutdown();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_new_document_resets_the_cursor() {
        let server = server(&[
            ("/", Route::ok("text/html", page(&["/next"]))),
            ("/next", Route::ok("text/html", page(&["/a"]))),
        ]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        let cursor = |msg: &RendererHostMsg| match msg {
            RendererHostMsg::SetCursor(cursor) => Some(*cursor),
            _ => None,
        };
        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 10.0));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, cursor),
            CursorType::Hand
        );
        // The hand came from the old document's link: the arrow is back
        // with the new one, until the pointer moves.
        click(&mut web_contents, 10.0, 10.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, cursor),
            CursorType::Pointer
        );
        web_contents.shutdown();
    }

    /// The cursor is what `cursor` asks for, of the element under the
    /// pointer (the keyword after its images); `auto` is the I-beam over
    /// text that can be selected, the arrow beside it; the UA stylesheet's
    /// as Chrome's: a link's hand, a text field's I-beam, a button's arrow.
    #[test]
    fn the_cursor_is_what_css_asks_for() {
        let html = "<!DOCTYPE html><style>html, body { margin: 0 } \
              div, a, input, button { display: block; width: 400px; height: 20px; margin: 0; \
                padding: 0 } \
              input, button { width: 100px; border: 0 }</style>\
            <div style='cursor: crosshair'></div>\
            <div>text</div>\
            <div style='user-select: none'>text</div>\
            <a href=/a style='cursor: move'>link</a>\
            <div style='cursor: url(a.png) 1 2, -webkit-zoom-in'></div>\
            <input><button>button</button>\
            <div style='cursor: none'></div>\
            <a href=/b>link</a>\
            <a href=/c style='cursor: auto'>link</a>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        let cursor = |msg: &RendererHostMsg| match msg {
            RendererHostMsg::SetCursor(cursor) => Some(*cursor),
            _ => None,
        };
        for (x, y, expected) in [
            (5.0, 10.0, CursorType::Cross),
            (5.0, 30.0, CursorType::IBeam),
            // Beside the text.
            (300.0, 30.0, CursorType::Pointer),
            (5.0, 30.0, CursorType::IBeam),
            (5.0, 50.0, CursorType::Pointer),
            (5.0, 70.0, CursorType::Move),
            (5.0, 90.0, CursorType::ZoomIn),
            (5.0, 110.0, CursorType::IBeam),
            // Over the button's text.
            (50.0, 130.0, CursorType::Pointer),
            (5.0, 150.0, CursorType::None),
            (5.0, 170.0, CursorType::Hand),
            (5.0, 150.0, CursorType::None),
            // `auto` over a link's text: the hand, not the I-beam.
            (5.0, 190.0, CursorType::Hand),
        ] {
            web_contents.forward_input_event(InputEvent::mouse_move(x, y));
            assert_eq!(
                wait_for(&mut web_contents, &host_rx, cursor),
                expected,
                "at {x}, {y}"
            );
        }
        web_contents.shutdown();
    }

    #[test]
    fn the_cursor_is_a_hand_over_links() {
        let server = server(&[("/", Route::ok("text/html", page(&["/a"])))]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        let cursor = |msg: &RendererHostMsg| match msg {
            RendererHostMsg::SetCursor(cursor) => Some(*cursor),
            _ => None,
        };
        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 10.0));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, cursor),
            CursorType::Hand
        );
        // Still over it: no message. Then off it.
        web_contents.forward_input_event(InputEvent::mouse_move(20.0, 20.0));
        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 100.0));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, cursor),
            CursorType::Pointer
        );
        web_contents.shutdown();
    }
    /// The acks of the input that arrives until `count` have come, and
    /// everything else that came with them.
    fn until_acks(
        viz: &mut DrivenViz,
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        count: usize,
    ) -> (Vec<Option<u64>>, Vec<RendererHostMsg>) {
        let (mut acks, mut others) = (Vec::new(), Vec::new());
        viz.run_until(web_contents, host_rx, "the acks", |_, msg| {
            match msg {
                Some(RendererHostMsg::InputEventAck { frame_token }) => acks.push(*frame_token),
                Some(other) => others.push(other.clone()),
                None => {}
            }
            acks.len() == count
        });
        (acks, others)
    }

    /// Input is aimed at the document the user sees, the one in the last
    /// frame drawn. With no viewport nothing is, so a click on a link laid
    /// out underneath does nothing and is acknowledged at once; a mouse move
    /// still sets the cursor. Once a frame is drawn, the same click follows
    /// the link.
    #[test]
    fn a_click_needs_a_drawn_document() {
        let server = server(&[("/", Route::ok("text/html", page(&["/next"])))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        assert!(viz.frames.is_empty(), "with no viewport, nothing is drawn");

        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 10.0));
        click(&mut web_contents, 10.0, 10.0);
        assert_eq!(web_contents.input_in_flight(), 3);
        let (acks, others) = until_acks(&mut viz, &mut web_contents, &host_rx, 3);
        assert_eq!(acks, [None, None, None]);
        assert_eq!(web_contents.input_in_flight(), 0);
        assert!(
            others
                .iter()
                .any(|m| matches!(m, RendererHostMsg::SetCursor(CursorType::Hand))),
            "{others:?}"
        );
        assert!(
            !others
                .iter()
                .any(|m| matches!(m, RendererHostMsg::OpenUrl { .. })),
            "{others:?}"
        );

        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        viz.run_until(&mut web_contents, &host_rx, "a frame", |viz, _| {
            !viz.frames.is_empty()
        });
        click(&mut web_contents, 10.0, 10.0);
        let next = server.url("/next");
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "the link",
            |_, msg| matches!(msg, Some(RendererHostMsg::OpenUrl { url, .. }) if *url == next),
        );
        web_contents.shutdown();
    }

    /// An ack waiting for a frame waits through render-blocking: the click
    /// adds a stylesheet that comes 300 ms later, the main frames until
    /// then are aborted, and the ack goes with the commit after it loads.
    #[test]
    fn an_ack_waits_through_render_blocking() {
        let html = "<!DOCTYPE html><div style='height: 100px'></div>\
             <script>\
               document.addEventListener('click', () => {\
                 const link = document.createElement('link');\
                 link.setAttribute('rel', 'stylesheet');\
                 link.setAttribute('href', '/slow.css');\
                 document.body.appendChild(link);\
               });\
             </script>";
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            (
                "/slow.css",
                Route::ok("text/css", "div { background: green }")
                    .with_delay(Duration::from_millis(300)),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        let clicked = std::time::Instant::now();
        click(&mut web_contents, 10.0, 10.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let [Some(_), Some(token)] = acks[..] else {
            panic!("the button going down's and the click's acks name frames: {acks:?}");
        };
        // The stylesheet's response is sent 300 ms after it was asked for,
        // which the click did.
        assert!(
            clicked.elapsed() >= Duration::from_millis(300),
            "the ack came before the stylesheet: {:?}",
            clicked.elapsed()
        );
        viz.run_until(&mut web_contents, &host_rx, "the frame acked", |viz, _| {
            viz.frames.contains(&token)
        });
        web_contents.shutdown();
    }

    /// An input's ack waits for the frame that shows what it did, if it
    /// asked for one, and comes at once otherwise. While Viz holds the
    /// frames back, a new document is committed but not drawn: a click is
    /// still aimed at the old one, and goes nowhere.
    #[test]
    fn an_ack_waits_for_the_frame_its_input_asked_for() {
        let server = server(&[
            ("/one", Route::ok("text/html", page(&["/a"]))),
            (
                "/two",
                Route::ok("text/html", "<div style='height: 1000px'></div>"),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/one"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        web_contents.navigate(server.url("/two"));
        let two = web_contents.latest_navigation_id();
        viz.run_until(&mut web_contents, &host_rx, "the commit", |_, msg| {
            matches!(msg, Some(RendererHostMsg::DidCommitNavigation { navigation_id, .. }) if *navigation_id == two)
        });
        click(&mut web_contents, 10.0, 10.0);
        let (acks, others) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        assert_eq!(acks, [None, None], "the old document is gone");
        assert!(
            !others
                .iter()
                .any(|m| matches!(m, RendererHostMsg::OpenUrl { .. })),
            "{others:?}"
        );

        viz.held = false;
        viz.until_loaded(&mut web_contents, &host_rx);
        // A click runs script, which may change anything: its ack waits;
        // so does the button going down's (`mousedown`, and the focus).
        click(&mut web_contents, 10.0, 10.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let [Some(_), Some(token)] = acks[..] else {
            panic!("the button going down and the click ask for frames: {acks:?}");
        };
        viz.run_until(&mut web_contents, &host_rx, "the frame acked", |viz, _| {
            viz.frames.contains(&token)
        });

        // Held again: the scroll's ack waits for its frame to be drawn; the
        // mouse move's after it waits only for the main frame that
        // dispatches it (rAF-aligned input), so it comes first, and asks
        // for no frame.
        viz.hold(&mut web_contents, &host_rx);
        web_contents.forward_input_event(InputEvent::wheel(
            10.0,
            10.0,
            ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        ));
        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 10.0));
        viz.held = false;
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let [None, Some(token)] = acks[..] else {
            panic!("the move's ack comes first, then the scroll's, naming a frame: {acks:?}");
        };
        viz.run_until(&mut web_contents, &host_rx, "the frame acked", |viz, _| {
            viz.frames.contains(&token)
        });
        web_contents.shutdown();
    }

    /// A hidden tab draws no frame for an ack to wait for. Hiding it sends
    /// the acks waiting for one: a click's, which the main thread holds
    /// until a main frame, and a wheel's, which the compositor scrolled and
    /// holds until it draws. Input it handles while hidden is acked at once.
    /// No frame is drawn for any of them.
    #[test]
    fn hiding_a_tab_acks_the_input_waiting_for_a_frame() {
        let html = "<!DOCTYPE html><div style='height: 1000px'></div><script>\
             document.addEventListener('click', () => { document.body.style.background = 'green'; });\
             </script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        let drawn = viz.frames.len();
        click(&mut web_contents, 10.0, 10.0);
        wheel(&mut web_contents, 10.0, 10.0, 100.0);
        assert_eq!(web_contents.input_in_flight(), 3, "all wait for a frame");

        web_contents.was_hidden();
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 3);
        assert_eq!(acks, [None, None, None]);
        assert_eq!(web_contents.input_in_flight(), 0);

        click(&mut web_contents, 10.0, 10.0);
        wheel(&mut web_contents, 10.0, 10.0, 100.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 3);
        assert_eq!(acks, [None, None, None], "hidden");
        assert_eq!(viz.frames.len(), drawn, "nothing was drawn");
        web_contents.shutdown();
    }

    /// A hidden tab draws no frame for the pointer's move to wait for: it
    /// is handled at once (the cursor over a link).
    #[test]
    fn a_hidden_tab_handles_a_move_at_once() {
        let server = server(&[("/", Route::ok("text/html", page(&["/next"])))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        web_contents.was_hidden();
        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 10.0));
        let cursor = |msg: &RendererHostMsg| match msg {
            RendererHostMsg::SetCursor(cursor) => Some(*cursor),
            _ => None,
        };
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, cursor),
            CursorType::Hand
        );
        web_contents.shutdown();
    }

    /// A wheel the compositor scrolled goes on to the listeners of the
    /// document it was drawn with: one committed since but not drawn yet
    /// (Viz holds the frames back) doesn't get it, but the next one, once
    /// it is drawn.
    #[test]
    fn a_wheel_goes_to_the_listeners_of_the_document_drawn() {
        let counting = |name: &str| {
            format!(
                "<!DOCTYPE html><div style='height: 1000px'></div><script>\
                 document.title = '{name}'; let n = 0;\
                 document.addEventListener('wheel', () => {{ document.title = String(++n); }});\
                 </script>"
            )
        };
        let server = server(&[
            ("/one", Route::ok("text/html", counting("one"))),
            ("/two", Route::ok("text/html", counting("two"))),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/one"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        web_contents.navigate(server.url("/two"));
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "two's script",
            |_, msg| matches!(msg, Some(RendererHostMsg::UpdateTitle(title)) if title == "two"),
        );
        // Scrolled in one, the one drawn: two's listener doesn't get it.
        wheel(&mut web_contents, 10.0, 10.0, 50.0);
        viz.held = false;
        viz.until_loaded(&mut web_contents, &host_rx);
        wheel(&mut web_contents, 10.0, 10.0, 50.0);
        let mut titles = Vec::new();
        viz.run_until(&mut web_contents, &host_rx, "a count", |_, msg| {
            if let Some(RendererHostMsg::UpdateTitle(title)) = msg {
                titles.push(title.clone());
            }
            titles.iter().any(|title| title.parse::<u32>().is_ok())
        });
        assert_eq!(titles.last().map(String::as_str), Some("1"), "{titles:?}");
        web_contents.shutdown();
    }

    /// A wheel: `dy` CSS px down at (`x`, `y`).
    fn wheel(web_contents: &mut WebContents, x: f32, y: f32, dy: f32) {
        web_contents.forward_input_event(InputEvent::wheel(
            x,
            y,
            ScrollDelta::Pixels { x: 0.0, y: dy },
        ));
    }

    /// Links, 50px high, at the document's `ys`.
    fn links_at(links: &[(&str, f32)]) -> String {
        let mut html = String::from(
            "<!DOCTYPE html><style>html, body { margin: 0 } a { display: block; height: 50px }</style>",
        );
        let mut at = 0.0;
        for (href, y) in links {
            html += &format!(
                "<div style='height: {}px'></div><a href='{href}'></a>",
                y - at
            );
            at = y + 50.0;
        }
        html + "<div style='height: 1000px'></div>"
    }

    /// While Viz holds the frames back, the compositor scrolls (and draws
    /// nothing): a click then is forwarded with what it scrolled, so the
    /// main thread hits what the user would see once it is drawn.
    #[test]
    fn a_click_after_a_compositor_scroll_lands_where_the_page_scrolled_to() {
        let server = server(&[(
            "/",
            Route::ok("text/html", links_at(&[("/top", 0.0), ("/far", 500.0)])),
        )]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        wheel(&mut web_contents, 150.0, 100.0, 450.0);
        // At 60 down the viewport: the document's 510, the far link.
        click(&mut web_contents, 10.0, 60.0);
        let far = server.url("/far");
        let mut acks = Vec::new();
        viz.run_until(&mut web_contents, &host_rx, "the far link", |_, msg| {
            if let Some(RendererHostMsg::InputEventAck { frame_token }) = msg {
                acks.push(*frame_token);
            }
            matches!(msg, Some(RendererHostMsg::OpenUrl { url, .. }) if *url == far)
        });
        assert_eq!(acks, [], "they all wait for a frame");
        assert_eq!(web_contents.input_in_flight(), 3, "in flight until drawn");
        // The wheel's, the button's and the click's come with the frame
        // that shows them.
        viz.held = false;
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 3);
        assert!(acks.iter().all(Option::is_some), "{acks:?}");
        assert_eq!(web_contents.input_in_flight(), 0);
        web_contents.shutdown();
    }

    /// A wheel over a scroll container scrolls the container, elsewhere the
    /// viewport; the compositor scrolls both, as the container is a layer.
    /// The container starts below the viewport: the compositor brings it
    /// into view, and finds it where it is drawn. The main thread hit tests
    /// a click where the compositor scrolled the container to, though it
    /// had no main frame to update in.
    #[test]
    fn a_container_takes_the_wheels_over_it() {
        let html = "<!DOCTYPE html><style>html, body { margin: 0 } a { display: block; height: 50px }</style>\
             <div style='height: 600px'></div>\
             <div style='width: 100px; height: 100px; overflow: auto'>\
               <div style='height: 250px'></div><a href='/inner'></a><div style='height: 100px'></div>\
             </div>\
             <div style='height: 1000px'></div>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        // Beside where the container will be: the viewport, to 600.
        wheel(&mut web_contents, 150.0, 50.0, 600.0);
        until_acks(&mut viz, &mut web_contents, &host_rx, 1);
        // Over the container, now at the top: its link comes up to 50.
        wheel(&mut web_contents, 50.0, 50.0, 200.0);
        until_acks(&mut viz, &mut web_contents, &host_rx, 1);
        click(&mut web_contents, 10.0, 60.0);
        let (_, others) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let opened: Vec<_> = others
            .iter()
            .filter_map(|m| match m {
                RendererHostMsg::OpenUrl { url, .. } => Some(url.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(opened, [server.url("/inner")]);
        web_contents.shutdown();
    }

    /// The old page scrolled while the new one was committed but not drawn
    /// yet: the new one starts at its top all the same.
    #[test]
    fn a_new_page_starts_at_the_top_whatever_the_old_one_scrolled() {
        let server = server(&[
            ("/one", Route::ok("text/html", links_at(&[]))),
            (
                "/two",
                Route::ok("text/html", links_at(&[("/top", 0.0), ("/far", 300.0)])),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/one"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        web_contents.navigate(server.url("/two"));
        let two = web_contents.latest_navigation_id();
        viz.run_until(&mut web_contents, &host_rx, "the commit", |_, msg| {
            matches!(msg, Some(RendererHostMsg::DidCommitNavigation { navigation_id, .. }) if *navigation_id == two)
        });
        // Still the first page on screen: the compositor scrolls it.
        wheel(&mut web_contents, 150.0, 100.0, 300.0);
        viz.held = false;
        viz.until_loaded(&mut web_contents, &host_rx);
        click(&mut web_contents, 10.0, 10.0);
        let top = server.url("/top");
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "the top link",
            |_, msg| matches!(msg, Some(RendererHostMsg::OpenUrl { url, .. }) if *url == top),
        );
        web_contents.shutdown();
    }

    /// Scrolling the old page can still reach the main thread after the
    /// new one is laid out: taken along by input forwarded once the new
    /// page's first main frame is on its way, which the main thread handles
    /// after that main frame. It is the old page's, so the new one doesn't
    /// move. The new page's first animation frame keeps the main thread
    /// busy, so the input is queued behind that main frame, and its commit
    /// comes after the input reached the compositor.
    #[test]
    fn scrolling_handed_over_for_the_old_page_doesnt_move_the_new_one() {
        let busy = "<script>requestAnimationFrame(() => { const end = Date.now() + 300; while (Date.now() < end) {} })</script>";
        let server = server(&[
            ("/one", Route::ok("text/html", links_at(&[]))),
            (
                "/two",
                Route::ok(
                    "text/html",
                    links_at(&[("/top", 0.0), ("/far", 300.0)]) + busy,
                ),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/one"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        web_contents.navigate(server.url("/two"));
        let two = web_contents.latest_navigation_id();
        viz.run_until(&mut web_contents, &host_rx, "the commit", |_, msg| {
            matches!(msg, Some(RendererHostMsg::DidCommitNavigation { navigation_id, .. }) if *navigation_id == two)
        });
        // The new page's first main frame goes out, once it asks for one
        // (the compositor gets the BeginFrame before the input below)...
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "a main frame wanted",
            |viz, _| viz.begin_frames.wants_begin_frame(),
        );
        viz.begin_frame();
        // ...then the old page, still on screen, scrolls, and a move takes
        // that along.
        wheel(&mut web_contents, 150.0, 100.0, 300.0);
        web_contents.forward_input_event(InputEvent::mouse_move(0.0, 0.0));
        viz.held = false;
        viz.until_loaded(&mut web_contents, &host_rx);
        click(&mut web_contents, 10.0, 10.0);
        let top = server.url("/top");
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "the top link",
            |_, msg| matches!(msg, Some(RendererHostMsg::OpenUrl { url, .. }) if *url == top),
        );
        web_contents.shutdown();
    }

    /// A click on the old page, still on screen, can reach the main thread
    /// after the new one is laid out (as the scroll above): it was aimed at
    /// the old page's document, so it doesn't follow the link the new page
    /// has under the pointer. Once the new page is drawn, the same click
    /// does.
    #[test]
    fn a_click_on_the_old_page_doesnt_reach_the_new_one() {
        let busy = "<script>requestAnimationFrame(() => { const end = Date.now() + 300; while (Date.now() < end) {} })</script>";
        let server = server(&[
            ("/one", Route::ok("text/html", links_at(&[]))),
            (
                "/two",
                Route::ok("text/html", links_at(&[("/top", 0.0)]) + busy),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/one"));
        viz.until_loaded(&mut web_contents, &host_rx);

        viz.hold(&mut web_contents, &host_rx);
        web_contents.navigate(server.url("/two"));
        let two = web_contents.latest_navigation_id();
        viz.run_until(&mut web_contents, &host_rx, "the commit", |_, msg| {
            matches!(msg, Some(RendererHostMsg::DidCommitNavigation { navigation_id, .. }) if *navigation_id == two)
        });
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "a main frame wanted",
            |viz, _| viz.begin_frames.wants_begin_frame(),
        );
        viz.begin_frame();
        // The old page, still on screen, is clicked where the new one has
        // its link.
        click(&mut web_contents, 10.0, 10.0);
        viz.held = false;
        let (_, others) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        assert!(
            !others
                .iter()
                .any(|m| matches!(m, RendererHostMsg::OpenUrl { .. })),
            "{others:?}"
        );
        viz.until_loaded(&mut web_contents, &host_rx);
        click(&mut web_contents, 10.0, 10.0);
        let top = server.url("/top");
        viz.run_until(
            &mut web_contents,
            &host_rx,
            "the top link",
            |_, msg| matches!(msg, Some(RendererHostMsg::OpenUrl { url, .. }) if *url == top),
        );
        web_contents.shutdown();
    }

    /// The compositor acks what it scrolled with the next frame, which is
    /// drawn even while rendering is blocked (the main frame is aborted):
    /// ahead of a click whose ack waits for the stylesheet.
    #[test]
    fn a_compositor_scroll_is_acked_while_rendering_is_blocked() {
        let html = "<!DOCTYPE html><div style='height: 1000px'></div>\
             <script>\
               document.addEventListener('click', () => {\
                 const link = document.createElement('link');\
                 link.setAttribute('rel', 'stylesheet');\
                 link.setAttribute('href', '/slow.css');\
                 document.body.appendChild(link);\
               });\
             </script>";
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            (
                "/slow.css",
                Route::ok("text/css", "div { background: green }")
                    .with_delay(Duration::from_millis(300)),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        // The button going down first, its frame drawn.
        press(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        until_acks(&mut viz, &mut web_contents, &host_rx, 1);
        release(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        wheel(&mut web_contents, 10.0, 10.0, 100.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let [Some(scrolled), Some(clicked)] = acks[..] else {
            panic!("the wheel, then the click: {acks:?}");
        };
        assert!(scrolled < clicked, "{acks:?}");
        web_contents.shutdown();
    }

    /// End to end: while the page's script keeps the main thread
    /// busy, the compositor scrolls and draws, and acks the wheel, all ahead
    /// of the busy click's ack; once the script is done, a click lands where
    /// the page scrolled to. The tab draws as the window does, not waiting
    /// for the main thread. Only the order of messages is checked, not
    /// time.
    #[test]
    fn scrolling_goes_on_while_script_keeps_the_main_thread_busy() {
        let html = "<!DOCTYPE html><style>html, body { margin: 0 } a, #busy { display: block; height: 50px }</style>\
             <div id=busy></div><div style='height: 450px'></div><a href='/far'></a><div style='height: 1000px'></div>\
             <script>\
               document.getElementById('busy').addEventListener('click', () => {\
                 const end = Date.now() + 500;\
                 while (Date.now() < end) {}\
               });\
             </script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_drawing(false);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        // The button going down first, its frame drawn; then the wheel,
        // then the click, which waited for the script.
        press(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        until_acks(&mut viz, &mut web_contents, &host_rx, 1);
        release(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        wheel(&mut web_contents, 150.0, 100.0, 450.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let [Some(scrolled), Some(clicked)] = acks[..] else {
            panic!("the wheel's ack, then the click's, name frames: {acks:?}");
        };
        assert!(scrolled < clicked, "{acks:?}");
        assert!(viz.frames.contains(&scrolled));

        // 60 down the viewport is the document's 510: the link.
        click(&mut web_contents, 10.0, 60.0);
        let (_, others) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        assert!(
            others.iter().any(
                |m| matches!(m, RendererHostMsg::OpenUrl { url, .. } if *url == server.url("/far"))
            ),
            "{others:?}"
        );
        web_contents.shutdown();
    }

    /// A scroll container that is a layer scrolls by its offset in the
    /// compositor: the compositor scrolls it (a wheel over it) and nothing
    /// is painted or rastered again. One that is scaled isn't a layer, and
    /// is rastered again where it scrolled to.
    /// While script keeps the main thread busy, a container
    /// that is a layer scrolls too, and what then comes into view is where
    /// the main thread hit tests once the script is done. A container that
    /// something painted after it covers is the main thread's to scroll:
    /// its wheel waits for the script.
    #[test]
    fn a_container_layer_scrolls_while_script_keeps_the_main_thread_busy() {
        let html = "<!DOCTYPE html><style>html, body { margin: 0 } a, #busy { display: block; height: 50px }</style>\
             <div id=busy></div>\
             <div style='width: 100px; height: 100px; overflow: auto'>\
               <div style='height: 250px'></div><a href='/inner'></a><div style='height: 100px'></div>\
             </div>\
             <div style='width: 100px; height: 100px; overflow: auto'><div style='height: 300px'></div></div>\
             <div style='width: 20px; height: 20px; margin-top: -10px'></div>\
             <div style='height: 1000px'></div>\
             <script>\
               document.getElementById('busy').addEventListener('click', () => {\
                 const end = Date.now() + 500;\
                 while (Date.now() < end) {}\
               });\
             </script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_drawing(false);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(200, 300), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        // The button going down first, its frame drawn.
        press(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        until_acks(&mut viz, &mut web_contents, &host_rx, 1);
        release(&mut web_contents, 10.0, 10.0, MouseButton::Left);
        // The first container (50 to 150): its link comes up to 100.
        wheel(&mut web_contents, 50.0, 100.0, 200.0);
        // The covered one (150 to 250).
        wheel(&mut web_contents, 50.0, 200.0, 50.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 3);
        // The first wheel, then the click, which waited for the script,
        // then the covered container's wheel, which waited behind it.
        let [Some(scrolled), Some(clicked), Some(covered)] = acks[..] else {
            panic!("the wheel's, the click's and the covered wheel's acks: {acks:?}");
        };
        assert!(scrolled < clicked && clicked <= covered, "{acks:?}");
        assert!(viz.frames.contains(&scrolled));

        click(&mut web_contents, 10.0, 120.0);
        let (_, others) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        assert!(
            others.iter().any(
                |m| matches!(m, RendererHostMsg::OpenUrl { url, .. } if *url == server.url("/inner"))
            ),
            "{others:?}"
        );
        web_contents.shutdown();
    }

    /// Script that runs every frame but changes no DOM needs no style,
    /// layout or paint: nothing is committed or drawn. One that changes a
    /// box's width each frame is drawn each frame, as a check (an attribute
    /// no selector reads changes nothing to draw, as in Chrome).
    #[test]
    fn script_that_changes_nothing_draws_nothing() {
        let page = |change: &str| {
            format!(
                "<!DOCTYPE html><div id=box style='height: 10px; background: green'></div><script>\
                   let n = 0;\
                   function tick() {{ n++; {change} requestAnimationFrame(tick); }}\
                   requestAnimationFrame(tick);\
                 </script>"
            )
        };
        let server = server(&[
            ("/still", Route::ok("text/html", page(""))),
            (
                "/moving",
                Route::ok(
                    "text/html",
                    page("document.getElementById('box').style.width = (10 + n % 50) + 'px';"),
                ),
            ),
        ]);
        // About one each, give or take the frames under way.
        for (path, frames) in [("/still", 0..=0), ("/moving", 8..=12)] {
            let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
            let mut viz = DrivenViz::new(viz_rx, begin_frames);
            web_contents.resize(gfx::Size::new(200, 200), 1.0);
            web_contents.navigate(server.url(path));
            viz.until_loaded(&mut web_contents, &host_rx);
            let (issued, drawn) = (viz.issued, viz.frames.len());
            viz.run_until(&mut web_contents, &host_rx, "ten frames", |viz, _| {
                viz.issued >= issued + 10 && !viz.begin_frames.is_pending()
            });
            let new = viz.frames.len() - drawn;
            assert!(frames.contains(&new), "{path}: {new} frames drawn");
            web_contents.shutdown();
        }
    }

    /// Raster invalidation: script changes one small box, and the frame
    /// that shows it rasters only the tile under it, of the four the 400px
    /// viewport shows (256px tiles).
    #[test]
    fn a_change_rasters_only_the_tile_it_is_in() {
        let html = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
             <div id=box style='margin: 10px; width: 10px; height: 10px; background-color: green'></div>\
             <div style='height: 1000px; background-color: blue'></div>\
             <script>\
               const box = document.getElementById('box');\
               box.addEventListener('click', () => box.setAttribute('style', 'margin: 10px; width: 10px; height: 10px; background-color: red'));\
             </script>";
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.resize(gfx::Size::new(400, 400), 1.0);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let seen: HashSet<u64> = viz.resources.values().flatten().copied().collect();

        click(&mut web_contents, 15.0, 15.0);
        let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 2);
        let Some(token) = acks[1] else {
            panic!("the click changed the page: {acks:?}");
        };
        viz.run_until(&mut web_contents, &host_rx, "the frame", |viz, _| {
            viz.resources.contains_key(&token)
        });
        let shown = &viz.resources[&token];
        // Of the four tiles in view, the two at the bottom are all blue,
        // drawn without pixels.
        assert_eq!(shown.len(), 2, "the two tiles in view with pixels");
        assert_eq!(
            shown.iter().filter(|id| !seen.contains(id)).count(),
            1,
            "only the one the box is in rastered again"
        );
        web_contents.shutdown();
    }

    #[test]
    fn scrolling_a_container_layer_rasters_nothing_again() {
        let page = |style: &str| {
            format!(
                "<!DOCTYPE html><style>html, body {{ margin: 0 }}</style>\
                 <div style='width: 100px; height: 100px; overflow: auto; transform-origin: 0 0; {style}'>\
                   <div style='height: 150px; background-color: green'></div>\
                   <div style='height: 150px; background-color: blue'></div>\
                 </div>"
            )
        };
        let server = server(&[
            ("/layer", Route::ok("text/html", page(""))),
            (
                "/scaled",
                Route::ok("text/html", page("transform: scale(1.5)")),
            ),
        ]);
        for (path, rasters_again) in [("/layer", false), ("/scaled", true)] {
            let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz();
            let mut viz = DrivenViz::new(viz_rx, begin_frames);
            // Tall enough that all of the container's content, scrolled or
            // not, is drawn (not only rastered) from the first frame.
            web_contents.resize(gfx::Size::new(200, 400), 1.0);
            web_contents.navigate(server.url(path));
            viz.until_loaded(&mut web_contents, &host_rx);
            let seen: HashSet<u64> = viz.resources.values().flatten().copied().collect();

            wheel(&mut web_contents, 50.0, 50.0, 100.0);
            let (acks, _) = until_acks(&mut viz, &mut web_contents, &host_rx, 1);
            let [Some(token)] = acks[..] else {
                panic!("the container scrolled: {acks:?}");
            };
            viz.run_until(&mut web_contents, &host_rx, "the frame", |viz, _| {
                viz.resources.contains_key(&token)
            });
            let new = viz.resources[&token].iter().any(|id| !seen.contains(id));
            assert_eq!(new, rasters_again, "{path}");
            web_contents.shutdown();
        }
    }

    /// What the page reported: the query of the next navigation it asked
    /// for to `/report?…` (script's titles don't reach the browser, its
    /// navigations do). The messages before go to `web_contents`.
    fn report(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>) -> String {
        wait_for(web_contents, host_rx, |msg| match msg {
            RendererHostMsg::OpenUrl { url, .. } if url.contains("/report?") => {
                Some(url.split_once("/report?").expect("a report").1.to_owned())
            }
            _ => None,
        })
    }

    /// A page that defines `report(text)`, reports `hashchange`
    /// (`hashchange-<old fragment>-<new fragment>-<location.hash>`), and
    /// runs `script` once it is completely loaded (after its `load`).
    fn after_load(script: &str) -> String {
        format!(
            "<!DOCTYPE html><script>\
               window.report = text => location.href = '/report?' + text;\
               addEventListener('hashchange', e => report(\
                 `hashchange-${{e.oldURL.split('#')[1]}}-${{e.newURL.split('#')[1]}}-${{location.hash.slice(1)}}`));\
               onload = () => setTimeout(() => {{ {script} }});\
             </script>"
        )
    }

    /// The next navigation to a fragment the browser hears of: the URL, and
    /// whether it replaces the current entry.
    fn same_document(msg: &RendererHostMsg) -> Option<(String, bool)> {
        match msg {
            RendererHostMsg::DidCommitSameDocumentNavigation { url, replace, .. } => {
                Some((url.clone(), *replace))
            }
            _ => None,
        }
    }

    /// The requests for `/`: a navigation to a fragment loads nothing.
    fn loads_of_the_page(server: &TestServer) -> usize {
        server.requests().iter().filter(|r| r.target == "/").count()
    }

    /// `location.hash`: the document's URL changes at once, the browser
    /// hears of a new entry, and `hashchange` comes in a task of its own,
    /// after the script that set it; nothing is loaded.
    #[test]
    fn location_hash_navigates_to_a_fragment() {
        let html = after_load("location.hash = 'a'; report(`set-${document.URL.split('#')[1]}`);");
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        let url = server.url("/");
        web_contents.navigate(&url);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, same_document),
            (format!("{url}#a"), false)
        );
        assert_eq!(
            history(&web_contents),
            (vec![url.clone(), format!("{url}#a")], Some(1))
        );
        assert_eq!(report(&mut web_contents, &host_rx), "set-a");
        assert_eq!(
            report(&mut web_contents, &host_rx),
            "hashchange-undefined-a-a"
        );
        assert_eq!(loads_of_the_page(&server), 1);
        web_contents.shutdown();
    }

    /// A link to a fragment fires `hashchange` too, and the document's URL
    /// follows it.
    #[test]
    fn a_link_to_a_fragment_fires_hashchange() {
        let html = format!(
            "{}<a href='#b' style='display: block; height: 50px'>b</a>",
            after_load("")
        );
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        let url = server.url("/");
        load(&mut web_contents, &host_rx, &url);
        click(&mut web_contents, 10.0, 10.0);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, same_document),
            (format!("{url}#b"), false)
        );
        assert_eq!(
            report(&mut web_contents, &host_rx),
            "hashchange-undefined-b-b"
        );
        assert_eq!(loads_of_the_page(&server), 1);
        web_contents.shutdown();
    }

    /// `location.replace` with a fragment replaces the current entry.
    #[test]
    fn location_replace_with_a_fragment_replaces_the_entry() {
        let server = server(&[(
            "/",
            Route::ok("text/html", after_load("location.replace('#c');")),
        )]);
        let (mut web_contents, host_rx) = tab();
        let url = server.url("/");
        web_contents.navigate(&url);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, same_document),
            (format!("{url}#c"), true)
        );
        assert_eq!(history(&web_contents), (vec![format!("{url}#c")], Some(0)));
        assert_eq!(
            report(&mut web_contents, &host_rx),
            "hashchange-undefined-c-c"
        );
        web_contents.shutdown();
    }

    /// `location` to another document goes through the browser: `href` adds
    /// an entry, `replace` takes the current one's place, and so does any
    /// navigation before the document is completely loaded; of two in one
    /// task, only the second.
    #[test]
    fn location_navigates_to_other_documents_through_the_browser() {
        let server = server(&[
            (
                "/",
                Route::ok("text/html", after_load("location.href = '/b';")),
            ),
            (
                "/b",
                Route::ok("text/html", after_load("location.replace('/c');")),
            ),
            // While it loads: replaces.
            (
                "/c",
                Route::ok("text/html", "<script>location.href = '/d';</script>"),
            ),
            (
                "/d",
                Route::ok(
                    "text/html",
                    after_load("location.href = '/e'; location.assign('/f');"),
                ),
            ),
            ("/f", Route::ok("text/html", "f")),
        ]);
        let (mut web_contents, host_rx) = tab();
        let url = |path: &str| server.url(path);
        web_contents.navigate(url("/"));
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), url("/"));
        for path in ["/b", "/c", "/d", "/f"] {
            assert_eq!(wait_for(&mut web_contents, &host_rx, committed), url(path));
        }
        assert_eq!(
            history(&web_contents),
            (vec![url("/"), url("/d"), url("/f")], Some(2))
        );
        let requested: Vec<String> = server.requests().iter().map(|r| r.target.clone()).collect();
        assert!(!requested.contains(&"/e".to_owned()), "{requested:?}");
        web_contents.shutdown();
    }

    /// `location.reload()`: the browser loads the current entry again.
    #[test]
    fn location_reload_loads_the_entry_again() {
        let server = server(&[(
            "/",
            Route::ok("text/html", after_load("location.reload();")),
        )]);
        let (mut web_contents, host_rx) = tab();
        let url = server.url("/");
        web_contents.navigate(&url);
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), url);
        wait_for(&mut web_contents, &host_rx, |msg| {
            matches!(msg, RendererHostMsg::Reload).then_some(())
        });
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), url);
        assert_eq!(history(&web_contents), (vec![url], Some(0)));
        web_contents.shutdown();
    }

    /// A `javascript:` URL given to `location` runs in the document.
    #[test]
    fn location_runs_a_javascript_url() {
        let server = server(&[(
            "/",
            Route::ok(
                "text/html",
                after_load("location.href = \"javascript:report('ran')\";"),
            ),
        )]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(report(&mut web_contents, &host_rx), "ran");
        web_contents.shutdown();
    }

    /// A child frame's navigation to a fragment happens in it (its own
    /// `hashchange`), but isn't in the session history; its navigation to
    /// another document is ignored: not supported yet.
    #[test]
    fn a_child_frame_s_location() {
        let child = "<script>\
              addEventListener('hashchange', () => parent.report(`child-${location.hash.slice(1)}`));\
              onload = () => setTimeout(() => { location.href = '/elsewhere'; location.hash = 'x'; });\
            </script>";
        let html = format!("{}<iframe srcdoc=\"{child}\"></iframe>", after_load(""));
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        let url = server.url("/");
        web_contents.navigate(&url);
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, |msg| match msg {
                RendererHostMsg::OpenUrl { url, .. } if url.contains("/report?") => {
                    Some(url.split_once("/report?").expect("a report").1.to_owned())
                }
                RendererHostMsg::DidCommitSameDocumentNavigation { url, .. }
                | RendererHostMsg::OpenUrl { url, .. } => panic!("the browser heard of {url}"),
                _ => None,
            }),
            "child-x"
        );
        assert_eq!(history(&web_contents), (vec![url], Some(0)));
        web_contents.shutdown();
    }

    /// What the page logged: the next `machoporeport:` URL it navigated to.
    /// The browser opens no such scheme, so reporting doesn't navigate
    /// (`report`'s `/report?` does, which would race with traversals).
    fn logged(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>) -> String {
        wait_for(web_contents, host_rx, |msg| match msg {
            RendererHostMsg::OpenUrl { url, .. } => {
                url.strip_prefix("machoporeport:").map(str::to_owned)
            }
            _ => None,
        })
    }

    /// A page that defines `report(text)` (to `logged`), runs `script` once
    /// it is completely loaded, and logs `popstate` (`popstate-<state's n
    /// or null>-<path><fragment>-<whether its state is history.state>`)
    /// and `hashchange` (`hashchange-<old fragment>-<new fragment>`): each
    /// event reports the whole log, `_` between entries (each report
    /// reaches the browser, a task's too: `logged_until` waits for the
    /// last).
    fn logging(script: &str) -> String {
        format!(
            "<!DOCTYPE html><script>\
               window.report = text => location.href = 'machoporeport:' + text;\
               const log = [];\
               window.note = text => {{ log.push(text); report(log.join('_')); }};\
               addEventListener('popstate', e => note(\
                 `popstate-${{e.state === null ? 'null' : e.state.n}}-${{location.pathname}}${{location.hash.slice(1)}}-${{e.state === history.state}}`));\
               addEventListener('hashchange', e => note(\
                 `hashchange-${{e.oldURL.split('#')[1]}}-${{e.newURL.split('#')[1]}}`));\
               onload = () => setTimeout(() => {{ {script} }});\
             </script>"
        )
    }

    /// `pushState` adds entries of the same document; back and forward
    /// between them load nothing: the URL and `history.state` change and
    /// `popstate` fires, its state `history.state` itself. `length` counts
    /// them at once.
    #[test]
    fn push_state_entries_are_traversed_within_the_document() {
        let html = logging(
            "history.pushState({n: 1}, '', '/p');
             history.pushState({n: 2}, '', '/q');
             report(`len-${history.length}`);",
        );
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        let url = |path: &str| server.url(path);
        web_contents.navigate(url("/"));
        assert_eq!(logged(&mut web_contents, &host_rx), "len-3");
        assert_eq!(
            history(&web_contents),
            (vec![url("/"), url("/p"), url("/q")], Some(2))
        );
        assert!(web_contents.go_back());
        assert_eq!(logged(&mut web_contents, &host_rx), "popstate-1-/p-true");
        assert_eq!(history(&web_contents).1, Some(1));
        assert!(web_contents.go_back());
        assert_eq!(
            logged(&mut web_contents, &host_rx),
            "popstate-1-/p-true_popstate-null-/-true"
        );
        assert!(web_contents.go_forward());
        assert_eq!(
            logged(&mut web_contents, &host_rx),
            "popstate-1-/p-true_popstate-null-/-true_popstate-1-/p-true"
        );
        assert_eq!(
            history(&web_contents),
            (vec![url("/"), url("/p"), url("/q")], Some(1))
        );
        assert_eq!(loads_of_the_page(&server), 1);
        web_contents.shutdown();
    }

    /// Waits until the page has logged `expected` (`logged`'s, which
    /// grows with each event).
    fn logged_until(
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        expected: &str,
    ) {
        loop {
            let log = logged(web_contents, host_rx);
            if log == expected {
                return;
            }
            assert!(expected.starts_with(&log), "logged {log}, not {expected}");
        }
    }

    /// A navigation to a fragment fires `popstate` at once (no state),
    /// `hashchange` in a task of its own; back between fragments too.
    #[test]
    fn navigations_between_fragments_fire_popstate_then_hashchange() {
        let html = logging("location.hash = 'a'; location.hash = 'b';");
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        let set = concat!(
            "popstate-null-/a-true_popstate-null-/b-true_",
            "hashchange-undefined-a_hashchange-a-b"
        );
        logged_until(&mut web_contents, &host_rx, set);
        assert!(web_contents.go_back());
        logged_until(
            &mut web_contents,
            &host_rx,
            &format!("{set}_popstate-null-/a-true_hashchange-b-a"),
        );
        assert_eq!(loads_of_the_page(&server), 1);
        web_contents.shutdown();
    }

    /// A link to a fragment fires `popstate` too, before `hashchange`.
    #[test]
    fn a_link_to_a_fragment_fires_popstate() {
        let html = format!(
            "{}<a href='#b' style='display: block; height: 50px'>b</a>",
            logging("")
        );
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        load(&mut web_contents, &host_rx, &server.url("/"));
        click(&mut web_contents, 10.0, 10.0);
        logged_until(
            &mut web_contents,
            &host_rx,
            "popstate-null-/b-true_hashchange-undefined-b",
        );
        web_contents.shutdown();
    }

    /// The state outlives its document: a reload, and back from another
    /// document, load the entry again with its state.
    #[test]
    fn the_state_comes_back_with_the_entry() {
        let html = logging(
            "if (history.state) report(`state-${history.state.n}`);
             else { history.replaceState({n: 7}, ''); report('replaced'); }",
        );
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            ("/other", Route::ok("text/html", "other")),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(logged(&mut web_contents, &host_rx), "replaced");
        web_contents.reload();
        assert_eq!(logged(&mut web_contents, &host_rx), "state-7");
        web_contents.navigate(server.url("/other"));
        assert_eq!(
            wait_for(&mut web_contents, &host_rx, committed),
            server.url("/other")
        );
        assert!(web_contents.go_back());
        assert_eq!(logged(&mut web_contents, &host_rx), "state-7");
        assert_eq!(loads_of_the_page(&server), 3);
        web_contents.shutdown();
    }

    /// `history.back()` from script, a child frame's too (the main
    /// frame's session history, where the child frame's own `pushState`
    /// isn't), after a
    /// `pushState` in the same task: back to the entry before it (and its
    /// fragment, so `hashchange` too).
    #[test]
    fn script_goes_back_through_the_browser() {
        let html = format!(
            "{}<iframe src='/child'></iframe>",
            logging(
                "frames[0].history.pushState(null, '', '#child');
                 history.pushState({n: 1}, '', '#one');
                 history.pushState({n: 2}, '', '#two');
                 note(`len-${frames[0].history.length}`);
                 frames[0].history.back();",
            )
        );
        let server = server(&[
            ("/", Route::ok("text/html", html)),
            ("/child", Route::ok("text/html", "child")),
        ]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(logged(&mut web_contents, &host_rx), "len-3");
        logged_until(
            &mut web_contents,
            &host_rx,
            "len-3_popstate-1-/one-true_hashchange-two-one",
        );
        assert_eq!(history(&web_contents).1, Some(1));
        assert_eq!(loads_of_the_page(&server), 1);
        web_contents.shutdown();
    }

    /// A traversal within a document the renderer no longer shows (the
    /// browser hadn't heard of the new one's commit yet): the renderer
    /// says so, and the browser loads the entry instead.
    #[test]
    fn a_traversal_within_a_document_gone_loads_the_entry() {
        let server = server(&[
            (
                "/",
                Route::ok(
                    "text/html",
                    logging("location.hash = 'x'; report('pushed');"),
                ),
            ),
            ("/other", Route::ok("text/html", "other")),
        ]);
        let (mut web_contents, host_rx) = tab();
        let url = |path: &str| server.url(path);
        web_contents.navigate(url("/"));
        // The fragment's `popstate` reports first, in the same task.
        while logged(&mut web_contents, &host_rx) != "pushed" {}
        // `/other` commits in the renderer; the browser hears of it only
        // after it asked for the traversal.
        web_contents.navigate(url("/other"));
        let mut held = Vec::new();
        loop {
            let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("renderer answers");
            let committed = matches!(&msg, RendererHostMsg::DidCommitNavigation { .. });
            held.push(msg);
            if committed {
                break;
            }
        }
        assert!(web_contents.go_back());
        for msg in &held {
            web_contents.on_renderer_message(msg);
        }
        wait_for(&mut web_contents, &host_rx, |msg| {
            matches!(msg, RendererHostMsg::SameDocumentNavigationFailed { .. }).then_some(())
        });
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), url("/"));
        assert_eq!(
            history(&web_contents),
            (
                vec![url("/"), format!("{}#x", url("/")), url("/other")],
                Some(0)
            )
        );
        assert_eq!(loads_of_the_page(&server), 2);
        web_contents.shutdown();
    }

    /// The renderer learns the session history's length once its commit
    /// is in (`SetHistoryOffsetAndLength`): here the navigation started
    /// before the browser heard of the old document's `pushState`, so what
    /// it said at the start is one entry short.
    #[test]
    fn the_length_is_set_again_after_the_commit() {
        let server = server(&[
            (
                "/",
                Route::ok(
                    "text/html",
                    "<script>onload = () => setTimeout(() => {\
                       location.hash = 'x'; location.href = 'machoporeport:pushed';\
                     });</script>",
                ),
            ),
            (
                "/b",
                Route::ok(
                    "text/html",
                    "<script>\
                       const timer = setInterval(() => {\
                         if (history.length === 3) {\
                           clearInterval(timer); location.href = 'machoporeport:len-3';\
                         }\
                       }, 5);\
                     </script>",
                ),
            ),
        ]);
        let (mut web_contents, host_rx) = tab();
        let url = |path: &str| server.url(path);
        web_contents.navigate(url("/"));
        assert_eq!(wait_for(&mut web_contents, &host_rx, committed), url("/"));
        let mut held = Vec::new();
        loop {
            let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("renderer answers");
            let pushed = matches!(&msg, RendererHostMsg::OpenUrl { url, .. } if url == "machoporeport:pushed");
            held.push(msg);
            if pushed {
                break;
            }
        }
        web_contents.navigate(url("/b"));
        for msg in &held {
            web_contents.on_renderer_message(msg);
        }
        assert_eq!(logged(&mut web_contents, &host_rx), "len-3");
        assert_eq!(
            history(&web_contents),
            (
                vec![url("/"), format!("{}#x", url("/")), url("/b")],
                Some(2)
            )
        );
        web_contents.shutdown();
    }

    /// The scroll restoration mode is the entry's: a new entry takes the
    /// current one's, and a traversal brings each back. A child frame's is
    /// its own.
    #[test]
    fn scroll_restoration_is_the_entry_s() {
        let html = format!(
            "{}<iframe srcdoc='child'></iframe>",
            logging(
                "history.scrollRestoration = 'manual';
                 history.pushState({n: 1}, '', '/one');
                 const copied = history.scrollRestoration;
                 history.scrollRestoration = 'auto';
                 frames[0].history.scrollRestoration = 'manual';
                 addEventListener('popstate', () => note(`mode-${history.scrollRestoration}`));
                 report(`copied-${copied}`);",
            )
        );
        let server = server(&[("/", Route::ok("text/html", html))]);
        let (mut web_contents, host_rx) = tab();
        web_contents.navigate(server.url("/"));
        assert_eq!(logged(&mut web_contents, &host_rx), "copied-manual");
        assert!(web_contents.go_back());
        logged_until(
            &mut web_contents,
            &host_rx,
            "popstate-null-/-true_mode-manual",
        );
        let entries = web_contents.controller().entries();
        assert!(entries[0].manual_scroll_restoration && !entries[1].manual_scroll_restoration);
        web_contents.shutdown();
    }
}

mod query_element {
    //! `QueryElement`, for automated checks of a browser window: which
    //! element a selector picks, its text, and where it is in the viewport.

    use gfx::RectF;
    use protocol::input::{InputEvent, MouseButton};
    use protocol::renderer::{CursorType, QueriedElement};

    use super::*;

    fn wait_until_loaded(web_contents: &mut WebContents, host_rx: &Receiver<ContentsEvent>) {
        let latest = web_contents.latest_navigation_id();
        loop {
            let msg = next_message(web_contents, host_rx, TIMEOUT).expect("renderer answers");
            web_contents.on_renderer_message(&msg);
            if let RendererHostMsg::QueryElementResult { .. }
            | RendererHostMsg::VisualStateReady { .. } = msg
            {
                panic!("an answer to an automation request: {msg:?}");
            }
            if matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if navigation_id == latest)
            {
                return;
            }
        }
    }

    fn query(
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        query_id: u64,
        selector: &str,
    ) -> Option<QueriedElement> {
        query_in(web_contents, host_rx, query_id, &[], selector)
    }

    /// [`query`] in the child frame of the first iframe matching each of
    /// `frames` in turn.
    fn query_in(
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        query_id: u64,
        frames: &[&str],
        selector: &str,
    ) -> Option<QueriedElement> {
        let frames: Vec<String> = frames.iter().map(|f| f.to_string()).collect();
        web_contents.query_element(query_id, &frames, selector);
        loop {
            let msg = next_message(web_contents, host_rx, TIMEOUT).expect("renderer answers");
            web_contents.on_renderer_message(&msg);
            if let RendererHostMsg::QueryElementResult {
                query_id: id,
                element,
            } = msg
            {
                assert_eq!(id, query_id);
                return element;
            }
        }
    }

    const PAGE: &str = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
        <div id=a style='margin-top: 20px; height: 30px'>One <!-- not text --><b>two</b></div>\
        <p class=x style='margin: 0; height: 20px'>three</p><div id=empty></div>\
        <div id=far style='margin-top: 1000px; height: 10px'>far</div>\
        <div style='height: 1000px'></div>";

    #[test]
    fn the_first_match_its_text_and_box() {
        let server = server(&[("/", Route::ok("text/html", PAGE))]);
        let (mut web_contents, host_rx) = tab_for(true);
        web_contents.navigate(server.url("/"));
        wait_until_loaded(&mut web_contents, &host_rx);
        let a = query(&mut web_contents, &host_rx, 1, "#a").expect("found");
        assert_eq!(a.text, "One two");
        assert_eq!(a.rect, Some(RectF::new(0.0, 20.0, 200.0, 30.0)));
        // The first of several.
        let first = query(&mut web_contents, &host_rx, 6, "div").expect("found");
        assert_eq!(first, a);
        let p = query(&mut web_contents, &host_rx, 2, "div ~ .x").expect("found");
        assert_eq!(p.text, "three");
        assert_eq!(p.rect.map(|r| r.y), Some(50.0));
        // An element with no text and no height still has a box.
        let empty = query(&mut web_contents, &host_rx, 3, "#empty").expect("found");
        assert_eq!(empty.text, "");
        assert!(empty.rect.is_some());
        assert_eq!(query(&mut web_contents, &host_rx, 4, "#none"), None);
        assert_eq!(query(&mut web_contents, &host_rx, 5, "[["), None);
        web_contents.shutdown();
    }

    /// The box is where the viewport shows it: scrolled to a fragment, the
    /// element is at the top.
    #[test]
    fn the_box_is_in_the_viewport() {
        let server = server(&[("/", Route::ok("text/html", PAGE))]);
        let (mut web_contents, host_rx) = tab_for(true);
        web_contents.navigate(server.url("/") + "#far");
        wait_until_loaded(&mut web_contents, &host_rx);
        let far = query(&mut web_contents, &host_rx, 1, "#far").expect("found");
        assert_eq!(far.rect, Some(RectF::new(0.0, 0.0, 200.0, 10.0)));
        let a = query(&mut web_contents, &host_rx, 2, "#a").expect("found");
        assert_eq!(a.rect.map(|r| r.y), Some(20.0 - 1070.0));
        web_contents.shutdown();
    }

    /// In a child frame: laid out in its iframe's
    /// content box, its box in the viewport of the document shown, moved
    /// by the iframe's border and padding, through nested frames. An
    /// iframe without a box still has its document, laid out at 0×0: its
    /// elements have no box in the viewport. The iframe resized lays its
    /// document out again, and so does a change to its document alone.
    #[test]
    fn in_child_frames() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
            <button id=narrow style='display: block; width: 50px; height: 10px; border: 0' \
              onclick=\"document.getElementById('a').contentDocument.getElementById('half').style.width = '10px'\">\
            </button>\
            <iframe id=a style='width: 50%; height: 100px; border: 5px solid; padding: 3px' \
              srcdoc=\"<style>body { margin: 0 }</style>\
                <div id=half style='width: 50%; height: 20px'>half</div>\
                <iframe id=inner style='display: block; margin-left: 7px; border: 0' \
                  srcdoc='<p id=deep style=margin:0>deep</p>'></iframe>\"></iframe>\
            <iframe id=hidden style='display: none' srcdoc='<p id=there>still there</p>'></iframe>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        let half = query_in(&mut web_contents, &host_rx, 1, &["#a"], "#half").expect("found");
        assert_eq!(half.text, "half");
        // The iframe is 100 wide (half the viewport), its content box at
        // (8, 18).
        assert_eq!(half.rect, Some(RectF::new(8.0, 18.0, 50.0, 20.0)));
        let deep =
            query_in(&mut web_contents, &host_rx, 2, &["#a", "#inner"], "#deep").expect("found");
        assert_eq!(deep.text, "deep");
        // The inner iframe is 300 wide (the default object size), below
        // `#half`; its document's body has its default 8px margin.
        assert_eq!(
            deep.rect.map(|r| (r.x, r.y, r.width)),
            Some((15.0 + 8.0, 38.0 + 8.0, 300.0 - 16.0))
        );
        // Selectors pick in the child frame's document only.
        assert_eq!(
            query_in(&mut web_contents, &host_rx, 3, &["#a"], "#a"),
            None
        );
        assert_eq!(
            query_in(&mut web_contents, &host_rx, 4, &["#none"], "p"),
            None
        );
        assert_eq!(
            query_in(&mut web_contents, &host_rx, 5, &["div"], "p"),
            None
        );

        let there =
            query_in(&mut web_contents, &host_rx, 6, &["#hidden"], "#there").expect("found");
        assert_eq!(there.text, "still there");
        assert_eq!(there.rect, None);

        let frames = viz.frames.len();
        web_contents.resize(gfx::Size::new(400, 200), 1.0);
        viz.run_until(&mut web_contents, &host_rx, "the resize drawn", |viz, _| {
            viz.frames.len() > frames && viz.idle()
        });
        let half = query_in(&mut web_contents, &host_rx, 7, &["#a"], "#half").expect("found");
        assert_eq!(half.rect, Some(RectF::new(8.0, 18.0, 100.0, 20.0)));

        // The button changes only the child frame's document: the frame
        // looks at its own document's style.
        for event in [
            InputEvent::mouse_down(
                10.0,
                5.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
            InputEvent::mouse_up(
                10.0,
                5.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
        ] {
            web_contents.forward_input_event(event);
        }
        let mut acks = 0;
        viz.run_until(&mut web_contents, &host_rx, "the click's acks", |_, msg| {
            if let Some(RendererHostMsg::InputEventAck { .. }) = msg {
                acks += 1;
            }
            acks == 2
        });
        web_contents.request_visual_state(8);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", |_, msg| {
            matches!(
                msg,
                Some(RendererHostMsg::VisualStateReady { request_id: 8 })
            )
        });
        let half = query_in(&mut web_contents, &host_rx, 9, &["#a"], "#half").expect("found");
        assert_eq!(half.rect.map(|r| r.width), Some(10.0));
        web_contents.shutdown();
    }

    /// What is under a pointer that doesn't move changes (a script sets
    /// another `cursor`, a scroll moves another element under it): the
    /// cursor follows once the frame is pre-painted, with no move (Blink's
    /// `EventHandler::ScheduleCursorUpdate`).
    #[test]
    fn the_cursor_follows_what_comes_under_a_still_pointer() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
              div { height: 150px }</style>\
            <div id=a style='cursor: crosshair' \
              onmousemove=\"this.style.cursor = 'wait'\"></div>\
            <div style='cursor: help'></div><div style='height: 400px'></div>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let mut until = |web_contents: &mut WebContents, cursor: CursorType| {
            viz.run_until(
                web_contents,
                &host_rx,
                "the cursor",
                |_, msg| matches!(msg, Some(RendererHostMsg::SetCursor(c)) if *c == cursor),
            );
        };
        web_contents.forward_input_event(InputEvent::mouse_move(10.0, 100.0));
        until(&mut web_contents, CursorType::Cross);
        // `mousemove` set another.
        until(&mut web_contents, CursorType::Wait);
        // The second block scrolls under the pointer.
        web_contents.forward_input_event(InputEvent::wheel(
            10.0,
            100.0,
            protocol::input::ScrollDelta::Pixels { x: 0.0, y: 100.0 },
        ));
        until(&mut web_contents, CursorType::Help);
        web_contents.shutdown();
    }

    /// Input into a child frame: a click goes to the
    /// element hit in the child frame's document, its `clientX` and
    /// `clientY` in the child frame's viewport; a link there to a fragment
    /// of its document scrolls the child frame (the middle button opens it
    /// in a tab instead), one to another document navigates the child
    /// frame, not the tab; the cursor is a hand over its links.
    #[test]
    fn clicks_go_into_child_frames() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
            <iframe id=a src=/child style='display: block; border: 0; margin-left: 20px; \
              width: 150px; height: 100px'></iframe>";
        let child = "<!DOCTYPE html><style>body { margin: 0 } a { display: block; height: 20px }</style>\
            <div id=c style='height: 30px' \
              onclick=\"this.textContent = event.clientX + ',' + event.clientY\"></div>\
            <a id=away href=/elsewhere>away</a><a id=to href=#t>to</a>\
            <div style='height: 300px'></div><div id=t style='height: 10px'>t</div>\
            <a id=away-too href=/elsewhere>away</a><div style='height: 300px'></div>";
        let server = server(&[
            ("/", Route::ok("text/html", page)),
            ("/child", Route::ok("text/html", child)),
            (
                "/elsewhere",
                Route::ok("text/html", "<!DOCTYPE html><p id=e>elsewhere</p>"),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        // Over `#away`, a link in the child frame, then over `#c`.
        for (y, cursor) in [(40.0, CursorType::Hand), (12.0, CursorType::Pointer)] {
            web_contents.forward_input_event(InputEvent::mouse_move(30.0, y));
            viz.run_until(
                &mut web_contents,
                &host_rx,
                "the cursor",
                |_, msg| matches!(msg, Some(RendererHostMsg::SetCursor(c)) if *c == cursor),
            );
        }

        let navigation = web_contents.latest_navigation_id();
        let mut opened = Vec::new();
        let mut click = |web_contents: &mut WebContents, x: f32, y: f32, button, id: u64| {
            for event in [
                InputEvent::mouse_down(x, y, button, protocol::input::Modifiers::NONE),
                InputEvent::mouse_up(x, y, button, protocol::input::Modifiers::NONE),
            ] {
                web_contents.forward_input_event(event);
            }
            let mut acks = 0;
            viz.run_until(web_contents, &host_rx, "the click's acks", |_, msg| {
                match msg {
                    Some(RendererHostMsg::InputEventAck { .. }) => acks += 1,
                    Some(RendererHostMsg::OpenUrl { url, .. }) => opened.push(url.clone()),
                    _ => {}
                }
                acks == 2
            });
            web_contents.request_visual_state(id);
            viz.run_until(web_contents, &host_rx, "the visual state", |_, msg| {
                matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id)
            });
        };
        // `#c` is at (0, 0) in the child frame, whose viewport is at (20, 0).
        click(&mut web_contents, 30.0, 12.0, MouseButton::Left, 1);
        // The link to a fragment with the middle button: a new tab for it,
        // the child frame not scrolled.
        click(&mut web_contents, 30.0, 60.0, MouseButton::Middle, 2);
        let t = query_in(&mut web_contents, &host_rx, 7, &["#a"], "#t").expect("found");
        assert_eq!(
            t.rect.map(|r| r.y),
            Some(370.0),
            "not scrolled for a new tab"
        );
        // With the primary one: the child frame scrolls to `#t`.
        click(&mut web_contents, 30.0, 60.0, MouseButton::Left, 3);
        let c = query_in(&mut web_contents, &host_rx, 4, &["#a"], "#c").expect("found");
        assert_eq!(c.text, "10,12");
        let t = query_in(&mut web_contents, &host_rx, 5, &["#a"], "#t").expect("found");
        assert_eq!(
            t.rect.map(|r| r.y),
            Some(0.0),
            "the child frame scrolled to #t"
        );
        // A link to another document, just below `#t`: the child frame
        // navigates there, the tab doesn't.
        click(&mut web_contents, 30.0, 20.0, MouseButton::Left, 6);
        drop(click);
        assert_eq!(opened, [server.url("/child#t")], "only the new tab's");
        let mut query = 8;
        while query_in(&mut web_contents, &host_rx, query, &["#a"], "#e").is_none() {
            query += 1;
            assert!(query < 200, "the child frame never got to /elsewhere");
        }
        assert_eq!(web_contents.latest_navigation_id(), navigation);
        web_contents.shutdown();
    }

    /// A wheel: `dy` CSS px down at (`x`, `y`), then a visual state
    /// (`request_id`) once it is drawn, so queries see where it scrolled.
    fn wheel_drawn(
        web_contents: &mut WebContents,
        host_rx: &Receiver<ContentsEvent>,
        viz: &mut DrivenViz,
        (x, y, dy): (f32, f32, f32),
        request_id: u64,
    ) {
        web_contents.forward_input_event(InputEvent::wheel(
            x,
            y,
            protocol::input::ScrollDelta::Pixels { x: 0.0, y: dy },
        ));
        viz.run_until(web_contents, host_rx, "the wheel's ack", |_, msg| {
            matches!(msg, Some(RendererHostMsg::InputEventAck { .. }))
        });
        web_contents.request_visual_state(request_id);
        viz.run_until(web_contents, host_rx, "the visual state", |_, msg| {
            matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id: id }) if *id == request_id)
        });
    }

    /// Wheels into child frames: a child frame's
    /// viewport scrolls, then passes what it can't take to the frame
    /// around it; a container in it scrolls first. A scaled iframe's
    /// child frame isn't a layer: the main thread scrolls it.
    /// `scrolling="no"` keeps the user from scrolling the child frame: the
    /// page scrolls instead.
    #[test]
    fn wheels_scroll_child_frames() {
        // 310 high: 210 to scroll in a 100px iframe; `#s` from 60 to 110,
        // with 150 to scroll.
        let child = "<style>body { margin: 0 }</style><div id=top style='height: 10px'></div>\
            <div style='height: 50px'></div>\
            <div style='height: 50px; overflow: auto'><div style='height: 200px'></div></div>\
            <div style='height: 200px'></div>";
        let page = format!(
            "<!DOCTYPE html><style>html, body {{ margin: 0 }} \
               iframe {{ display: block; border: 0; width: 100px; height: 100px }}</style>\
             <iframe id=a srcdoc=\"{child}\"></iframe>\
             <iframe id=b srcdoc=\"{child}\" style='transform: scale(0.5)'></iframe>\
             <iframe id=c srcdoc=\"{child}\" scrolling=no></iframe>\
             <iframe id=d srcdoc=\"<style>html {{ overflow: hidden }}</style>{child}\"></iframe>\
             <div style='height: 1000px'></div>"
        );
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 400), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let mut next = 0;
        let mut y = |web_contents: &mut WebContents, frames: &[&str], selector: &str| {
            next += 1;
            query_in(web_contents, &host_rx, 100 + next, frames, selector)
                .and_then(|e| e.rect)
                .map(|r| r.y)
        };

        // `#a`'s viewport, as a layer.
        wheel_drawn(&mut web_contents, &host_rx, &mut viz, (50.0, 50.0, 60.0), 1);
        assert_eq!(y(&mut web_contents, &["#a"], "#top"), Some(-60.0));
        assert_eq!(y(&mut web_contents, &[], "#a"), Some(0.0));
        // Over its container, now at 0 to 50 in the viewport.
        wheel_drawn(&mut web_contents, &host_rx, &mut viz, (50.0, 30.0, 20.0), 2);
        assert_eq!(
            y(&mut web_contents, &["#a"], "#top"),
            Some(-60.0),
            "the container took it"
        );
        // Below it: to the viewport's end (150 more), then 50 for the page.
        wheel_drawn(
            &mut web_contents,
            &host_rx,
            &mut viz,
            (50.0, 80.0, 200.0),
            3,
        );
        assert_eq!(y(&mut web_contents, &[], "#a"), Some(-50.0));
        assert_eq!(y(&mut web_contents, &["#a"], "#top"), Some(-50.0 - 210.0));

        // `#b`, at 50 to 150 now, shown at half its size around its center.
        let before = y(&mut web_contents, &["#b"], "#top").expect("a box");
        wheel_drawn(
            &mut web_contents,
            &host_rx,
            &mut viz,
            (50.0, 100.0, 30.0),
            4,
        );
        assert_eq!(y(&mut web_contents, &["#b"], "#top"), Some(before - 15.0));
        assert_eq!(
            y(&mut web_contents, &[], "#b"),
            Some(75.0),
            "the page didn't move (shown from 50 + 25)"
        );

        // `#c`, at 150 to 250: the page scrolls.
        wheel_drawn(
            &mut web_contents,
            &host_rx,
            &mut viz,
            (50.0, 200.0, 10.0),
            5,
        );
        assert_eq!(y(&mut web_contents, &[], "#c"), Some(140.0));
        assert_eq!(y(&mut web_contents, &["#c"], "#top"), Some(140.0));

        // `#d`, at 240 to 340, `overflow: hidden`, above its `#s`: the page
        // scrolls.
        wheel_drawn(
            &mut web_contents,
            &host_rx,
            &mut viz,
            (50.0, 270.0, 10.0),
            6,
        );
        assert_eq!(y(&mut web_contents, &[], "#d"), Some(230.0));
        assert_eq!(y(&mut web_contents, &["#d"], "#top"), Some(230.0));
        web_contents.shutdown();
    }

    /// A child frame's child frame, scrolled by the compositor: its offset
    /// gets to its frame, two down.
    #[test]
    fn wheels_scroll_nested_child_frames() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
            <iframe id=a style='display: block; border: 0; width: 150px; height: 150px' \
              srcdoc=\"<style>body { margin: 0 }</style>\
                <iframe id=in style='display: block; border: 0; width: 100px; height: 100px' \
                  srcdoc='<style>body { margin: 0 }</style><div id=top style=height:10px></div>\
                    <div style=height:500px></div>'></iframe>\"></iframe>\
            <div style='height: 1000px'></div>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        wheel_drawn(&mut web_contents, &host_rx, &mut viz, (50.0, 50.0, 40.0), 1);
        let top = query_in(&mut web_contents, &host_rx, 2, &["#a", "#in"], "#top").expect("found");
        assert_eq!(top.rect.map(|r| r.y), Some(-40.0));
        web_contents.shutdown();
    }

    /// `ScrollIntoViewIfNeeded` in a child frame: the
    /// element is scrolled into view in its frame, and the iframe in the
    /// page, from the inside out; one shown already moves nothing; one the
    /// child frame hides, in an iframe the page shows, scrolls only the
    /// child frame.
    #[test]
    fn an_element_in_a_child_frame_is_scrolled_into_view() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
              iframe { display: block; border: 0; width: 100px; height: 100px }</style>\
            <iframe id=near srcdoc=\"<style>body { margin: 0 }</style>\
              <div id=t style='margin-top: 150px; height: 10px'></div>\
              <div style='height: 300px'></div>\"></iframe>\
            <div style='height: 400px'></div>\
            <iframe id=far srcdoc=\"<style>body { margin: 0 }</style>\
              <div id=shown style='height: 10px'></div>\
              <div id=t style='margin-top: 300px; height: 10px'></div>\
              <div style='height: 300px'></div>\"></iframe>\
            <div style='height: 1000px'></div>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let frames = |frame: &str| vec![frame.to_owned()];
        let mut scroll = |web_contents: &mut WebContents, frame: &str, selector, id| {
            web_contents.scroll_into_view_if_needed(&frames(frame), selector);
            web_contents.request_visual_state(id);
            viz.run_until(web_contents, &host_rx, "the visual state", |_, msg| {
                matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id)
            });
        };
        let y = |web_contents: &mut WebContents, id, frame: &[&str], selector| {
            query_in(web_contents, &host_rx, id, frame, selector)
                .and_then(|e| e.rect)
                .map(|r| r.y)
        };

        // In `#near`, at 150 in the window (which shows 200) but below its
        // child frame's viewport (100).
        scroll(&mut web_contents, "#near", "#t", 1);
        assert_eq!(y(&mut web_contents, 2, &["#near"], "#t"), Some(0.0));
        assert_eq!(
            y(&mut web_contents, 3, &[], "#near"),
            Some(0.0),
            "the page didn't move"
        );
        // In `#far`, below the page's viewport: both scroll.
        scroll(&mut web_contents, "#far", "#t", 4);
        assert_eq!(y(&mut web_contents, 5, &["#far"], "#t"), Some(0.0));
        assert_eq!(y(&mut web_contents, 6, &[], "#far"), Some(0.0));
        // Shown already: nothing moves.
        scroll(&mut web_contents, "#far", "#t", 7);
        assert_eq!(y(&mut web_contents, 8, &[], "#far"), Some(0.0));
        assert_eq!(y(&mut web_contents, 9, &["#far"], "#shown"), Some(-310.0));
        web_contents.shutdown();
    }

    /// The compositor scrolling a container that is a layer: pre-paint is
    /// done again for hit testing, without a main frame, through the child
    /// frames as the last paint went.
    #[test]
    fn child_frames_stay_where_they_are_after_a_compositor_scroll() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
            <div style='width: 100px; height: 100px; overflow: auto'>\
              <div style='height: 300px'></div></div>\
            <iframe id=a style='display: block; border: 0' \
              srcdoc='<p id=c style=margin:0>c</p>'></iframe>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let c = query_in(&mut web_contents, &host_rx, 1, &["#a"], "#c").expect("found");
        assert_eq!(c.rect.map(|r| (r.x, r.y)), Some((8.0, 108.0)));

        web_contents.forward_input_event(InputEvent::wheel(
            50.0,
            50.0,
            protocol::input::ScrollDelta::Pixels { x: 0.0, y: 50.0 },
        ));
        viz.run_until(&mut web_contents, &host_rx, "the wheel's ack", |_, msg| {
            matches!(msg, Some(RendererHostMsg::InputEventAck { .. }))
        });
        let c = query_in(&mut web_contents, &host_rx, 2, &["#a"], "#c").expect("found");
        assert_eq!(c.rect.map(|r| (r.x, r.y)), Some((8.0, 108.0)));
        web_contents.shutdown();
    }

    /// A child frame whose stylesheets are loading isn't laid out, so
    /// isn't shown unstyled, while the page is: its elements have no box
    /// until the stylesheet has come. One that was laid out before is
    /// painted as it was meanwhile, not blank.
    #[test]
    fn a_child_frame_waits_for_its_stylesheets() {
        let server = server(&[
            (
                "/",
                Route::ok(
                    "text/html",
                    "<!DOCTYPE html>\
                     <button style='display: block; width: 50px; height: 10px; border: 0' \
                       onclick=\"const d = document.getElementById('a').contentDocument; \
                         const l = d.createElement('link'); l.setAttribute('rel', 'stylesheet'); \
                         l.setAttribute('href', '/slower.css'); d.body.appendChild(l)\"></button>\
                     <iframe id=a src=/child></iframe>",
                ),
            ),
            (
                "/slower.css",
                Route::ok("text/css", "#c { height: 41px }").with_delay(Duration::from_secs(1)),
            ),
            (
                "/child",
                Route::ok(
                    "text/html",
                    "<!DOCTYPE html><link rel=stylesheet href=/slow.css><div id=c>c</div>",
                ),
            ),
            (
                "/slow.css",
                Route::ok("text/css", "#c { height: 37px }").with_delay(Duration::from_secs(1)),
            ),
        ]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        let deadline = std::time::Instant::now() + TIMEOUT;
        let mut id = 0;
        while query_in(&mut web_contents, &host_rx, id, &["#a"], "#c").is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the child is never parsed"
            );
            id += 1;
        }
        web_contents.request_visual_state(id + 1);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", |_, msg| {
            matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id + 1)
        });
        let c = query_in(&mut web_contents, &host_rx, id + 2, &["#a"], "#c").expect("found");
        assert_eq!(c.rect, None, "laid out before its stylesheet came");
        viz.until_loaded(&mut web_contents, &host_rx);
        let c = query_in(&mut web_contents, &host_rx, id + 3, &["#a"], "#c").expect("found");
        assert_eq!(c.rect.map(|r| r.height), Some(37.0));

        // Another stylesheet, added by script (the button is inside the
        // body's margin).
        for event in [
            InputEvent::mouse_down(
                20.0,
                12.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
            InputEvent::mouse_up(
                20.0,
                12.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
        ] {
            web_contents.forward_input_event(event);
        }
        let mut acks = 0;
        viz.run_until(&mut web_contents, &host_rx, "the click's acks", |_, msg| {
            if let Some(RendererHostMsg::InputEventAck { .. }) = msg {
                acks += 1;
            }
            acks == 2
        });
        let mut id = id + 4;
        web_contents.request_visual_state(id);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", |_, msg| {
            matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id)
        });
        let c = query_in(&mut web_contents, &host_rx, id + 1, &["#a"], "#c").expect("found");
        assert_eq!(
            c.rect.map(|r| r.height),
            Some(37.0),
            "painted as last laid out while the stylesheet loads"
        );
        let deadline = std::time::Instant::now() + TIMEOUT;
        loop {
            id += 2;
            web_contents.request_visual_state(id);
            viz.run_until(&mut web_contents, &host_rx, "the visual state", |_, msg| {
                matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id)
            });
            let c = query_in(&mut web_contents, &host_rx, id + 1, &["#a"], "#c").expect("found");
            if c.rect.map(|r| r.height) == Some(41.0) {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the stylesheet never came"
            );
        }
        web_contents.shutdown();
    }

    /// `ScrollIntoViewIfNeeded`: an element whose center is out of the
    /// viewport is scrolled to as a fragment is, its top at the top, and
    /// the visual state asked for after it comes with the frame that shows
    /// that. One in view, or none, moves nothing.
    #[test]
    fn an_element_out_of_view_is_scrolled_into_it() {
        let server = server(&[("/", Route::ok("text/html", PAGE))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let ready = |id: u64| move |_: &DrivenViz, msg: Option<&RendererHostMsg>| matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id);

        web_contents.scroll_into_view_if_needed(&[], "#a");
        web_contents.scroll_into_view_if_needed(&[], "#none");
        web_contents.request_visual_state(1);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", ready(1));
        let a = query(&mut web_contents, &host_rx, 2, "#a").expect("found");
        assert_eq!(a.rect.map(|r| r.y), Some(20.0), "in view already");

        let frames = viz.frames.len();
        web_contents.scroll_into_view_if_needed(&[], "#far");
        web_contents.request_visual_state(3);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", ready(3));
        assert!(viz.frames.len() > frames, "a frame shows the scroll");
        let far = query(&mut web_contents, &host_rx, 4, "#far").expect("found");
        assert_eq!(far.rect, Some(RectF::new(0.0, 0.0, 200.0, 10.0)));
        web_contents.shutdown();
    }

    /// Out of view only sideways, or only in its scroll container (whose
    /// box is in view): scrolled to all the same, and the box asked for
    /// next is where it is now.
    #[test]
    fn an_element_out_of_view_sideways_or_in_its_container_is_scrolled_to() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
            <div id=box style='height: 100px; overflow: auto'>\
              <div style='height: 300px'></div><div id=inner style='height: 10px'></div>\
              <div style='height: 300px'></div></div>\
            <div style='width: 1000px; height: 10px'>\
              <div id=right style='margin-left: 600px; width: 10px; height: 10px'></div></div>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        let ready = |id: u64| move |_: &DrivenViz, msg: Option<&RendererHostMsg>| matches!(msg, Some(RendererHostMsg::VisualStateReady { request_id }) if *request_id == id);

        let inner = query(&mut web_contents, &host_rx, 1, "#inner").expect("found");
        assert_eq!(inner.rect.map(|r| r.y), Some(300.0));
        web_contents.scroll_into_view_if_needed(&[], "#inner");
        web_contents.request_visual_state(2);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", ready(2));
        let inner = query(&mut web_contents, &host_rx, 3, "#inner").expect("found");
        assert_eq!(inner.rect.map(|r| r.y), Some(0.0), "the container scrolled");

        let right = query(&mut web_contents, &host_rx, 4, "#right").expect("found");
        assert_eq!(right.rect.map(|r| (r.x, r.y)), Some((600.0, 100.0)));
        web_contents.scroll_into_view_if_needed(&[], "#right");
        web_contents.request_visual_state(5);
        viz.run_until(&mut web_contents, &host_rx, "the visual state", ready(5));
        let right = query(&mut web_contents, &host_rx, 6, "#right").expect("found");
        let x = right.rect.map(|r| r.x).expect("a box");
        assert!((0.0..200.0).contains(&x), "scrolled sideways: {x}");
        web_contents.shutdown();
    }

    /// Without `RendererFlags::automation` nothing scrolls: a click where
    /// the element would have come to goes to what was there.
    #[test]
    fn nothing_scrolls_without_automation() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }</style>\
            <div style='height: 1000px'></div>\
            <a id=far href='/far' style='display: block; height: 50px'>far</a>\
            <div style='height: 1000px'></div>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, false);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);

        web_contents.scroll_into_view_if_needed(&[], "#far");
        // A resize goes to the main thread after the request, and its main
        // frame would scroll too: once that is drawn, so would the scroll.
        let frames = viz.frames.len();
        web_contents.resize(gfx::Size::new(200, 210), 1.0);
        viz.run_until(&mut web_contents, &host_rx, "the resize drawn", |viz, _| {
            viz.frames.len() > frames && viz.idle()
        });
        for event in [
            InputEvent::mouse_down(
                10.0,
                25.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
            InputEvent::mouse_up(
                10.0,
                25.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
        ] {
            web_contents.forward_input_event(event);
        }
        let mut followed = false;
        let mut acks = 0;
        viz.run_until(&mut web_contents, &host_rx, "the click's acks", |_, msg| {
            match msg {
                Some(RendererHostMsg::InputEventAck { .. }) => acks += 1,
                Some(RendererHostMsg::OpenUrl { .. }) => followed = true,
                _ => {}
            }
            acks == 2
        });
        assert!(!followed, "the link was scrolled under the click");
        web_contents.shutdown();
    }

    /// Without `RendererFlags::automation` no page hears of it: no answer
    /// comes before what the renderer does next.
    #[test]
    fn it_is_ignored_without_automation() {
        let server = server(&[("/", Route::ok("text/html", PAGE))]);
        let (mut web_contents, host_rx) = tab_for(false);
        web_contents.navigate(server.url("/"));
        wait_until_loaded(&mut web_contents, &host_rx);
        web_contents.query_element(1, &[], "#a");
        web_contents.scroll_into_view_if_needed(&[], "#far");
        web_contents.request_visual_state(2);
        web_contents.navigate(server.url("/"));
        wait_until_loaded(&mut web_contents, &host_rx);
        web_contents.shutdown();
    }

    /// `RequestVisualState` is answered once a frame drawn after it has gone
    /// to Viz, and not while Viz sends no `BeginFrame`: after a change, so
    /// the frame shows it; with nothing new to draw too, as the last
    /// commit's frame may not have gone to Viz yet.
    #[test]
    fn the_visual_state_waits_for_the_frame_a_change_asked_for() {
        let page = "<!DOCTYPE html><style>html, body { margin: 0 }             p { margin: 0; height: 50px }</style>            <p id=b onclick='this.textContent = `changed`'>before</p>";
        let server = server(&[("/", Route::ok("text/html", page))]);
        let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_viz_flags(true, true);
        web_contents.resize(gfx::Size::new(200, 200), 1.0);
        let mut viz = DrivenViz::new(viz_rx, begin_frames);
        web_contents.navigate(server.url("/"));
        viz.until_loaded(&mut web_contents, &host_rx);
        viz.hold(&mut web_contents, &host_rx);

        let ready = |msg: &RendererHostMsg, id: u64| matches!(msg, RendererHostMsg::VisualStateReady { request_id } if *request_id == id);
        // Nothing new to draw: still a frame first.
        web_contents.request_visual_state(1);
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while let Ok(msg) = next_message(
            &mut web_contents,
            &host_rx,
            deadline.saturating_duration_since(std::time::Instant::now()),
        ) {
            assert!(!ready(&msg, 1), "answered before a frame was drawn");
        }
        let frames = viz.frames.len();
        viz.held = false;
        viz.run_until(&mut web_contents, &host_rx, "the visual state", |_, msg| {
            msg.is_some_and(|msg| ready(msg, 1))
        });
        assert!(
            viz.frames.len() > frames,
            "a frame went to Viz before the answer"
        );
        viz.hold(&mut web_contents, &host_rx);

        // A change: the renderer wants a frame, which no BeginFrame lets it
        // draw.
        for event in [
            InputEvent::mouse_down(
                10.0,
                10.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
            InputEvent::mouse_up(
                10.0,
                10.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            ),
        ] {
            web_contents.forward_input_event(event);
        }
        let mut id = 10;
        loop {
            id += 1;
            if query(&mut web_contents, &host_rx, id, "#b").map(|b| b.text)
                == Some("changed".into())
            {
                break;
            }
        }
        web_contents.request_visual_state(2);
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        while let Ok(msg) = next_message(
            &mut web_contents,
            &host_rx,
            deadline.saturating_duration_since(std::time::Instant::now()),
        ) {
            assert!(!ready(&msg, 2), "answered before a frame was drawn");
        }
        let frames = viz.frames.len();
        viz.held = false;
        viz.run_until(&mut web_contents, &host_rx, "the visual state", |_, msg| {
            msg.is_some_and(|msg| ready(msg, 2))
        });
        assert!(
            viz.frames.len() > frames,
            "a frame went to Viz before the answer"
        );
        web_contents.shutdown();
    }
}
