//! A renderer in a process of its own: the test starts itself again as
//! the renderer (`child`, below), as an embedder starts its own
//! executable. The browser's side is the same `WebContents` as with a
//! renderer in its process.

use std::process::Command;
use std::sync::Arc;

use browser::RendererLauncher;

use protocol::renderer::RendererHostMsg;
use protocol::viz::{CompositorFrameSinkMsg, VizMsg};

use crate::navigation::{
    SCRIPT_ENGINES, TIMEOUT, in_memory, next_commit, next_message, tab_launched, tab_with_launcher,
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
        web_contents.navigate(&format!("data:text/html,<title>tab {n}</title>"));
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
        match viz_rx.recv_timeout(std::time::Duration::from_millis(20)) {
            Ok(VizMsg::FrameSink(from, msg)) => {
                assert_eq!(from, sink);
                begin_frames.did_receive(from, &msg);
                submitted = matches!(msg, CompositorFrameSinkMsg::SubmitCompositorFrame { .. });
            }
            Ok(_) | Err(_) => {}
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
