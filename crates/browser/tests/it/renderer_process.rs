//! A renderer in a process of its own: the test starts itself again as
//! the renderer (`child`, below), as an embedder starts its own
//! executable. The browser's side is the same `WebContents` as with a
//! renderer in its process.

use std::process::Command;
use std::sync::Arc;

use browser::RendererLauncher;

use net::test_server::Route;
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
    if std::env::var_os(RENDERER).is_none() {
        return;
    }
    let exit = match ipc::platform::inherited() {
        Some(channel) => match renderer::run_child_process(channel, SCRIPT_ENGINES) {
            renderer::ChildExit::Ended => 0,
            renderer::ChildExit::NotStarted => 3,
            renderer::ChildExit::Crashed => 4,
        },
        None => 2,
    };
    std::process::exit(exit);
}

/// Starts this test binary again as a renderer.
fn as_a_process() -> RendererLauncher {
    RendererLauncher::Process(Arc::new(|| {
        let mut command = Command::new(std::env::current_exe().expect("the test's own path"));
        command
            .args([
                "--exact",
                "renderer_process::child",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(RENDERER, "1");
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
fn a_renderer_processs_tiles_go_to_the_gpu() {
    // The renderer is told Viz has a GPU, so its compositor sends each
    // tile's pixels as a shared image as soon as they are drawn, and its
    // frames name them: the pixels cross in the messages (as large bytes
    // do), from the renderer's sink.
    let (mut web_contents, host_rx, mut viz, _gpu) = tab_with_a_gpu();
    web_contents.navigate(TILES);
    viz.until_loaded(&mut web_contents, &host_rx);
    viz.run_until(&mut web_contents, &host_rx, "a frame of tiles", |viz, _| {
        viz.shared_images.len() >= 2 && !viz.frames.is_empty()
    });
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
