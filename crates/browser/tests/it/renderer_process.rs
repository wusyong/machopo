//! A renderer in a process of its own: the test starts itself again as
//! the renderer (`child`, below), as an embedder starts its own
//! executable. The browser's side is the same `WebContents` as with a
//! renderer in its process.

use std::process::Command;
use std::sync::Arc;

use browser::RendererLauncher;

use ipc::core::frame::BadMessage;
use ipc::endpoint::{self, Handler, IoLoop, Received};
use ipc::platform::Channel;
use net::test_server::Route;
use protocol::control_channel::{ChannelLink, config, to_browser};
use protocol::renderer::RendererHostMsg;
use protocol::viz::{CompositorFrameSinkMsg, VizMsg};

use crate::navigation::{
    DrivenViz, RecordingGpu, SCRIPT_ENGINES, TIMEOUT, in_memory, next_commit, next_message,
    tab_launched, tab_with_launcher,
};

/// Set in the test run that is to be a renderer.
const RENDERER: &str = "MACHOPO_TEST_RENDERER";

/// The renderer's side: is a renderer when `RENDERER` says so, and exits
/// with how that ended. Without it (the test run itself) it does nothing.
#[test]
fn child() {
    let Some(role) = std::env::var_os(RENDERER) else {
        return;
    };
    // Whatever it is, it ends by itself in the end: one the browser's
    // side failed to end (the test of that failing) keeps the test run's
    // output open, and the run waits on it.
    std::thread::spawn(|| {
        std::thread::sleep(TIMEOUT * 3);
        std::process::exit(9);
    });
    match role.to_str() {
        // Not a renderer of ours: what no renderer sends, and then it
        // stays, whatever becomes of its channel.
        Some("garbage") => rogue(|link| {
            let _ = link.send(to_browser::HOST, "no message of a renderer's");
        }),
        Some("another's cookies") => rogue(|link| {
            let cookie = RendererHostMsg::SetCookie {
                loader: 999,
                url: "https://example.test/".into(),
                line: "a=b".into(),
            };
            let _ = link.send(to_browser::HOST, &cookie);
        }),
        _ => {}
    }
    let exit = match ipc::platform::inherited() {
        Some(channel) => match renderer::run_child_process(channel, SCRIPT_ENGINES, None) {
            renderer::ChildExit::Ended => 0,
            renderer::ChildExit::NotStarted => 3,
            renderer::ChildExit::Crashed => 4,
        },
        None => 2,
    };
    std::process::exit(exit);
}

/// A process that takes a renderer's end of the channel, sends what
/// `send` does, and then goes on (far longer than a test waits): it reads
/// its channel and heeds nothing, as a renderer that was taken over
/// would.
fn rogue(send: impl FnOnce(&ChannelLink)) -> ! {
    struct Deaf;
    impl Handler<Channel> for Deaf {
        fn message(&mut self, _: Received<'_, Channel>) -> Result<(), BadMessage> {
            Ok(())
        }
    }
    let channel = ipc::platform::inherited().expect("its end of the channel");
    let (link, endpoint) = endpoint::endpoint(channel, config(false));
    let (io, io_handle) = IoLoop::new().expect("a way to wait");
    io_handle.serve(endpoint, Deaf);
    std::thread::spawn(move || io.run());
    send(&link);
    loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
    }
}

/// Starts this test binary again as a renderer.
fn as_a_process() -> RendererLauncher {
    launching("a renderer")
}

/// Starts this test binary again as what `role` says ([`child`]).
fn launching(role: &'static str) -> RendererLauncher {
    RendererLauncher::Process(Arc::new(move || {
        let mut command = Command::new(std::env::current_exe().expect("the test's own path"));
        command
            .args([
                "--exact",
                "renderer_process::child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(RENDERER, role);
        command
    }))
}

#[test]
fn a_document_loads_in_a_renderer_process() {
    let (mut web_contents, host_rx) = tab_launched(
        as_a_process(),
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    );
    let renderer = web_contents.renderer_pid().expect("a process of its own");
    assert_ne!(renderer, std::process::id());
    // Its messages come over the channel, and the browser's go by it: a
    // navigation commits, and the document the renderer parsed says its
    // title. (A document this short is in the commit itself.)
    web_contents.navigate("about:blank");
    let (_, committed, title) = next_commit(&mut web_contents, &host_rx);
    assert_eq!((committed.as_str(), title.as_str()), ("about:blank", ""));
    web_contents.navigate("data:text/html,<title>from a process</title><p>hello");
    let (_, committed, title) = next_commit(&mut web_contents, &host_rx);
    assert!(committed.starts_with("data:text/html,"), "{committed}");
    assert_eq!(title, "from a process");
    // The same process served both.
    assert_eq!(web_contents.renderer_pid(), Some(renderer));
    web_contents.shutdown();
}

#[test]
fn a_document_longer_than_its_commit_carries_loads_in_a_renderer_process() {
    // Most of the body comes after the commit, by the body's pipe, which
    // crosses with the commit: the title is the last thing in it, past
    // more than the pipe holds at once, so the renderer says it only if
    // every byte came, in order, as the renderer read.
    let mut page = String::from("<!DOCTYPE html><p>");
    let mut n = 0u32;
    while page.len() < 1_500_000 {
        page.push_str(&format!("{n} "));
        n += 1;
    }
    page.push_str(&format!("<title>after {n} numbers</title>"));
    let server = crate::navigation::server(&[("/long", Route::ok("text/html", page.as_str()))]);
    let (mut web_contents, host_rx) = tab_launched(
        as_a_process(),
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    );
    let url = server.url("/long");
    web_contents.navigate(&url);
    // The document commits with its first bytes, with no title yet; the
    // title comes when the parser reaches it.
    let mut committed = None;
    let title = loop {
        let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("the renderer says");
        web_contents.on_renderer_message(&msg);
        match msg {
            RendererHostMsg::DidCommitNavigation { url, .. } => committed = Some(url),
            RendererHostMsg::UpdateTitle(title) if !title.is_empty() => break title,
            RendererHostMsg::RendererGone => panic!("the renderer went"),
            _ => {}
        }
    };
    assert_eq!(committed.as_deref(), Some(url.as_str()));
    assert_eq!(title, format!("after {n} numbers"));
    web_contents.shutdown();
}

/// A tab with a renderer process and a Viz that has a GPU, played by the
/// test.
fn tab_with_a_gpu() -> (
    browser::WebContents,
    crossbeam_channel::Receiver<browser::ContentsEvent>,
    DrivenViz,
    Arc<RecordingGpu>,
) {
    let gpu = Arc::new(RecordingGpu::default());
    let (mut web_contents, host_rx, viz_rx, begin_frames) = tab_with_launcher(
        as_a_process(),
        true,
        false,
        Some(gpu.clone()),
        gfx::SystemColors::DEFAULT,
        Default::default(),
        browser::NEW_CONTENT_RENDERING_DELAY,
        in_memory(),
    );
    web_contents.resize(gfx::Size::new(600, 600), 1.0);
    (
        web_contents,
        host_rx,
        DrivenViz::new(viz_rx, begin_frames),
        gpu,
    )
}

/// A page with more than one tile of text.
const TILES: &str = "data:text/html,<p style='font-size:80px'>tiles tiles tiles tiles tiles";

#[test]
fn a_renderer_processs_tiles_go_to_the_gpu_in_shared_memory() {
    // The renderer is told Viz has a GPU, so its compositor sends each
    // tile's pixels as a shared image as soon as they are drawn, and its
    // frames name them, from the renderer's sink. The pixels aren't in
    // the messages: each tile is drawn in shared memory the browser gave
    // the renderer when it started it, which is what Viz is handed, to
    // copy to the GPU, once.
    let (mut web_contents, host_rx, mut viz, _gpu) = tab_with_a_gpu();
    viz.keep_pixels = true;
    web_contents.navigate(TILES);
    viz.until_loaded(&mut web_contents, &host_rx);
    viz.run_until(&mut web_contents, &host_rx, "a frame of tiles", |viz, _| {
        viz.shared_images.len() >= 2 && !viz.frames.is_empty()
    });
    assert_eq!(viz.pixels_own, 0, "a tile's pixels copied into its message");
    assert!(viz.pixels_shared >= 2);
    // What is read there is what was drawn: tiles of a page of text, of
    // more than one color each (a tile of one is sent as its color).
    for pixels in &viz.kept_pixels {
        assert!(pixels.as_bytes().is_none());
        let mut read = vec![0; pixels.len()];
        assert!(pixels.copy_to(0, &mut read));
        assert!(read.chunks(4).any(|pixel| pixel != &read[..4]));
    }
    web_contents.shutdown();
}

/// A page of more tiles than the renderer starts with shared memory for:
/// 1600 pixels each way, every tile with text in it.
fn many_tiles() -> String {
    let line = "tiles ".repeat(60);
    let lines = format!("<div>{line}</div>").repeat(60);
    format!("data:text/html,<body style='margin:0;font-size:30px;white-space:nowrap'>{lines}")
}

#[test]
fn a_renderer_process_out_of_shared_memory_copies_its_tiles_and_is_given_more() {
    // A Viz that never lets go of a tile's pixels: the renderer has no
    // shared memory to draw the next in once what it started with is all
    // out. It doesn't wait: those tiles' pixels go in their messages.
    // And it asked for more, which the browser gave: later tiles are in
    // shared memory again, past what it started with.
    let (mut web_contents, host_rx, mut viz, _gpu) = tab_with_a_gpu();
    viz.keep_pixels = true;
    web_contents.resize(gfx::Size::new(1600, 1600), 1.0);
    web_contents.navigate(many_tiles());
    viz.until_loaded(&mut web_contents, &host_rx);
    viz.run_until(
        &mut web_contents,
        &host_rx,
        "more tiles in shared memory than the renderer started with",
        |viz, _| viz.pixels_shared > 16 && viz.pixels_own > 0,
    );
    // Every one of them whole, wherever it came.
    for pixels in &viz.kept_pixels {
        let mut read = vec![0; pixels.len()];
        assert!(pixels.copy_to(0, &mut read));
        assert!(read.chunks(4).any(|pixel| pixel != &read[..4]));
    }
    web_contents.shutdown();
}

#[test]
fn losing_the_gpu_has_a_renderer_process_make_its_tiles_again() {
    let (mut web_contents, host_rx, mut viz, gpu) = tab_with_a_gpu();
    web_contents.navigate(TILES);
    viz.until_loaded(&mut web_contents, &host_rx);
    viz.run_until(&mut web_contents, &host_rx, "tiles on the GPU", |viz, _| {
        !viz.shared_images.is_empty()
    });
    let before = viz.shared_images.clone();
    // The browser is told by Viz, and tells the renderer's compositor
    // over the channel.
    assert_eq!(gpu.lose(), 1, "only the browser watches the GPU");
    viz.run_until(
        &mut web_contents,
        &host_rx,
        "the lost GPU's shared images to go and new ones to come",
        |viz, _| before.is_disjoint(&viz.shared_images) && !viz.shared_images.is_empty(),
    );
    web_contents.shutdown();
}

#[test]
fn what_the_browser_sends_a_renderer_process_that_is_large_goes_by_shared_memory() {
    // A script of 300 KiB, which the browser loads and sends whole: by a
    // region the browser makes for this renderer, not in the message.
    let mut script = String::from("document.title = 'ran';\n//");
    script.push_str(&"x".repeat(300 << 10));
    let server = crate::navigation::server(&[
        (
            "/page",
            Route::ok("text/html", "<!DOCTYPE html><script src=/big.js></script>"),
        ),
        ("/big.js", Route::ok("text/javascript", script.as_str())),
    ]);
    let (mut web_contents, host_rx) = tab_launched(
        as_a_process(),
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    );
    assert_eq!(web_contents.renderer_shared_memory(), Some((0, 0)));
    web_contents.navigate(server.url("/page"));
    loop {
        let msg = next_message(&mut web_contents, &host_rx, TIMEOUT).expect("the renderer says");
        web_contents.on_renderer_message(&msg);
        if matches!(&msg, RendererHostMsg::UpdateTitle(title) if title == "ran") {
            break;
        }
    }
    // One region, long enough for it.
    let (bytes, _free) = web_contents
        .renderer_shared_memory()
        .expect("a process of its own");
    assert_eq!(bytes, 512 << 10);
    web_contents.shutdown();
}

#[test]
fn each_tab_has_a_renderer_process_of_its_own() {
    // Both on the browser's one IO thread, each heard as itself.
    let mut tabs: Vec<_> = (0..2)
        .map(|_| {
            tab_launched(
                as_a_process(),
                false,
                gfx::SystemColors::DEFAULT,
                in_memory(),
            )
        })
        .collect();
    assert_ne!(tabs[0].0.renderer_pid(), tabs[1].0.renderer_pid());
    for (n, (web_contents, _)) in tabs.iter_mut().enumerate() {
        web_contents.navigate(format!("data:text/html,<title>tab {n}</title>"));
    }
    for (n, (web_contents, host_rx)) in tabs.iter_mut().enumerate() {
        let (_, _, title) = next_commit(web_contents, host_rx);
        assert_eq!(title, format!("tab {n}"));
    }
    for (web_contents, _) in &mut tabs {
        web_contents.shutdown();
    }
}

#[test]
fn a_renderer_processs_frames_are_its_tabs_sinks() {
    let (mut web_contents, host_rx, viz_rx, mut begin_frames) = tab_with_launcher(
        as_a_process(),
        true,
        false,
        None,
        gfx::SystemColors::DEFAULT,
        Default::default(),
        browser::NEW_CONTENT_RENDERING_DELAY,
        in_memory(),
    );
    web_contents.resize(gfx::Size::new(200, 200), 1.0);
    web_contents.navigate("data:text/html,<title>drawn</title><p>hello");
    // The test is Viz: it sends `BeginFrame`s (over the channel, to the
    // renderer's compositor) and takes what the renderer's compositor
    // sends back, which names no sink: the browser says whose it is.
    let sink = web_contents.frame_sink_id();
    let deadline = std::time::Instant::now() + TIMEOUT;
    let mut frame_time = 0.0;
    let mut submitted = false;
    while !submitted {
        assert!(std::time::Instant::now() < deadline, "no frame came");
        if begin_frames.wants_begin_frame() {
            begin_frames.issue(frame_time);
            frame_time += 1000.0 / 60.0;
        }
        while let Ok(event) = host_rx.try_recv() {
            if let browser::ContentsEvent::Renderer { msg, .. } = &event {
                web_contents.on_renderer_message(msg);
            } else {
                web_contents.on_navigation_event(event);
            }
        }
        if let Ok(VizMsg::FrameSink(from, msg)) =
            viz_rx.recv_timeout(std::time::Duration::from_millis(20))
        {
            assert_eq!(from, sink);
            begin_frames.did_receive(from, &msg);
            submitted = matches!(msg, CompositorFrameSinkMsg::SubmitCompositorFrame { .. });
        }
    }
    web_contents.shutdown();
}

/// Ends the process `pid` at once, as a crash does.
#[cfg(unix)]
fn end_at_once(pid: u32) {
    let killed = Command::new("kill")
        .args(["-9", &pid.to_string()])
        .status()
        .expect("kill runs");
    assert!(killed.success());
}

#[cfg(unix)]
#[test]
fn a_renderer_process_that_dies_is_reported_gone() {
    let (mut web_contents, host_rx) = tab_launched(
        as_a_process(),
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    );
    web_contents.navigate("data:text/html,<title>alive</title>");
    let (_, _, title) = next_commit(&mut web_contents, &host_rx);
    assert_eq!(title, "alive");
    end_at_once(web_contents.renderer_pid().expect("a process of its own"));
    // Its channel is over: the tab hears it once, after what it sent.
    loop {
        match next_message(&mut web_contents, &host_rx, TIMEOUT).expect("the tab hears") {
            RendererHostMsg::RendererGone => break,
            msg => web_contents.on_renderer_message(&msg),
        }
    }
    web_contents.on_renderer_message(&RendererHostMsg::RendererGone);
    assert!(web_contents.is_crashed());
    web_contents.shutdown();
}

#[test]
fn a_renderer_in_the_browsers_process_has_no_process() {
    let (mut web_contents, _host_rx) = crate::navigation::tab_in(in_memory());
    assert_eq!(web_contents.renderer_pid(), None);
    web_contents.shutdown();
}

/// How the system has the process: its state's letters (`Z` for one that
/// ended and nobody waited for), or none once it is gone.
#[cfg(unix)]
fn state_of(pid: u32) -> Option<String> {
    let out = Command::new("ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .expect("ps runs");
    let state = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!state.is_empty()).then_some(state)
}

/// The signal `abort` ends a process with, and the one that kills it.
#[cfg(unix)]
const ABORTED: i32 = 6;
#[cfg(unix)]
const KILLED: i32 = 9;

/// The signal that ended the tab's renderer process, if one did.
#[cfg(unix)]
fn ended_by(web_contents: &browser::WebContents) -> Option<i32> {
    use std::os::unix::process::ExitStatusExt;
    web_contents
        .renderer_exit()
        .and_then(|status| status.signal())
}

#[cfg(unix)]
#[test]
fn a_renderer_process_told_to_crash_does_and_another_is_started_for_a_reload() {
    let (mut web_contents, host_rx) = tab_launched(
        as_a_process(),
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    );
    web_contents.navigate("data:text/html,<title>one</title>");
    assert_eq!(next_commit(&mut web_contents, &host_rx).2, "one");
    let first = web_contents.renderer_pid().expect("a process of its own");
    // The renderer ends itself (abnormally: the system says it aborted,
    // not that it was killed), and has by the time the tab is asked
    // anything more.
    web_contents.navigate("about:crash");
    assert!(web_contents.is_crashed());
    assert_eq!(ended_by(&web_contents), Some(ABORTED));
    // Seen to its end: nothing is left of the process, not even what
    // waits to be waited for.
    assert_eq!(state_of(first), None);
    assert_eq!(web_contents.renderer_pid(), None);
    assert_eq!(web_contents.renderer_shared_memory(), None);
    // A reload starts another.
    web_contents.reload();
    assert_eq!(next_commit(&mut web_contents, &host_rx).2, "one");
    assert!(!web_contents.is_crashed());
    let second = web_contents.renderer_pid().expect("a process of its own");
    assert_ne!(second, first);
    web_contents.shutdown();
    assert_eq!(state_of(second), None);
}

#[cfg(unix)]
#[test]
fn a_renderer_process_that_hangs_crashes_when_told_to() {
    let (mut web_contents, host_rx) = tab_launched(
        as_a_process(),
        false,
        gfx::SystemColors::DEFAULT,
        in_memory(),
    );
    // Its main thread never comes back from this script.
    web_contents.navigate("data:text/html,<title>stuck</title><script>for(;;){}</script>");
    assert_eq!(next_commit(&mut web_contents, &host_rx).2, "stuck");
    let pid = web_contents.renderer_pid().expect("a process of its own");
    std::thread::sleep(std::time::Duration::from_millis(300));
    web_contents.navigate("about:crash");
    assert!(web_contents.is_crashed());
    assert_eq!(ended_by(&web_contents), Some(ABORTED));
    assert_eq!(state_of(pid), None);
    web_contents.shutdown();
}

/// What the tab is told was wrong with what its renderer sent: the tab
/// ends the renderer for it, as an embedder has it do.
#[cfg(unix)]
fn bad_message_of(
    web_contents: &mut browser::WebContents,
    host_rx: &crossbeam_channel::Receiver<browser::ContentsEvent>,
) -> browser::BadMessageReason {
    loop {
        match host_rx.recv_timeout(TIMEOUT).expect("the tab hears") {
            browser::ContentsEvent::BadMessage { renderer, reason } => {
                assert!(web_contents.is_current_renderer(renderer));
                web_contents.received_bad_message(reason);
                return reason;
            }
            browser::ContentsEvent::Renderer { msg, .. } => {
                assert!(
                    !matches!(msg, RendererHostMsg::RendererGone),
                    "gone, with nothing said of a bad message"
                );
            }
            _ => {}
        }
    }
}

#[cfg(unix)]
#[test]
fn a_process_that_sends_what_no_renderer_does_is_killed() {
    // What the channel itself refuses, and what the browser's checks do:
    // either way the process, which goes on running, is ended, and the
    // tab shows that its renderer is gone.
    for (role, reason) in [
        ("garbage", browser::BadMessageReason::Channel),
        (
            "another's cookies",
            browser::BadMessageReason::UnknownLoader,
        ),
    ] {
        let (mut web_contents, host_rx) = tab_launched(
            launching(role),
            false,
            gfx::SystemColors::DEFAULT,
            in_memory(),
        );
        let pid = web_contents.renderer_pid().expect("a process of its own");
        assert_eq!(
            bad_message_of(&mut web_contents, &host_rx),
            reason,
            "{role}"
        );
        assert!(web_contents.is_crashed(), "{role}");
        assert_eq!(ended_by(&web_contents), Some(KILLED), "{role}");
        assert_eq!(state_of(pid), None, "{role}: still there");
        assert_eq!(web_contents.renderer_pid(), None, "{role}");
        web_contents.shutdown();
    }
}
