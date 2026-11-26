//! `machopo --type=renderer`: the executable as a renderer process. That
//! a renderer in a process of its own loads a document is `browser`'s
//! test, with a test run for the process; here is the executable's own
//! way in.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(unix)]
use crate::automation_pipe::start_machopo;

pub(crate) const MACHOPO: &str = env!("CARGO_BIN_EXE_machopo");

/// What the process exited with, if it did before long.
fn exit_of(child: &mut std::process::Child) -> Option<i32> {
    let started = Instant::now();
    loop {
        match child.try_wait().expect("the child is ours to wait for") {
            Some(status) => return status.code(),
            None if started.elapsed() > Duration::from_secs(20) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// `machopo` with `args`, as a person starts it: from a shell, not from
/// this test run, which is a host to the renderers its other tests start
/// (a renderer asks its parent for its end, and one a host knows nothing
/// of waits to be told of).
#[cfg(unix)]
fn by_hand(args: &[&str]) -> Command {
    let mut command = Command::new("sh");
    // Not the shell's last command, which it would become instead of
    // starting.
    command
        .arg("-c")
        .arg(r#""$0" "$@"; exit $?"#)
        .arg(MACHOPO)
        .args(args);
    command
}

#[cfg(not(unix))]
fn by_hand(args: &[&str]) -> Command {
    let mut command = Command::new(MACHOPO);
    command.args(args);
    command
}

#[test]
fn a_renderer_started_by_hand_says_it_has_no_browser() {
    // What would be a URL to open, were this a browser.
    let out = by_hand(&[shell::RENDERER_PROCESS_SWITCH, "about:blank"])
        .stdin(Stdio::null())
        .output()
        .expect("machopo runs");
    assert_eq!(out.status.code(), Some(i32::from(shell::NO_CHANNEL_EXIT)));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(said.contains("no browser to render for"), "{said}");
    assert!(out.stdout.is_empty());
}

#[test]
fn a_renderer_whose_browser_goes_ends() {
    let mut command = Command::new(MACHOPO);
    command
        .arg(shell::RENDERER_PROCESS_SWITCH)
        // It says when it reads its channel.
        .env("RUST_LOG", "renderer=info")
        .stderr(Stdio::piped());
    let (mut child, channel) = ipc::platform::spawn(&mut command).expect("machopo starts");
    let mut said = BufReader::new(child.stderr.take().expect("its standard error"));
    let mut line = String::new();
    loop {
        line.clear();
        let read = said.read_line(&mut line).expect("what it says is text");
        assert!(read != 0, "it ended before it had its channel");
        if line.contains("the renderer process has its channel") {
            break;
        }
    }
    // The browser's end goes before it said what to start with: the
    // renderer hears, and ends without having rendered.
    drop(channel);
    assert_eq!(exit_of(&mut child), Some(0));
}

/// Reads what the process says until it says `what`.
fn until_it_says(said: &mut impl BufRead, what: &str) {
    let mut line = String::new();
    loop {
        line.clear();
        let read = said.read_line(&mut line).expect("what it says is text");
        assert!(read != 0, "it ended before it said: {what}");
        if line.contains(what) {
            return;
        }
    }
}

/// A browser's side of a renderer's channel that makes nothing of what
/// the renderer sends.
struct Deaf;

impl ipc::endpoint::Handler<ipc::platform::Channel> for Deaf {
    fn message(
        &mut self,
        _message: ipc::endpoint::Received<'_, ipc::platform::Channel>,
    ) -> Result<(), ipc::core::frame::BadMessage> {
        Ok(())
    }
}

/// A renderer process with its threads running: started with `switches`
/// after `--type=renderer`, told what to start with, and heard to have
/// started. Its browser's side goes when [`RunningRenderer::browser_goes`]
/// is called.
pub(crate) struct RunningRenderer {
    pub(crate) child: std::process::Child,
    link: ipc::endpoint::Link<ipc::platform::Channel>,
    io_handle: ipc::endpoint::LoopHandle<ipc::platform::Channel>,
    io_thread: std::thread::JoinHandle<()>,
}

impl RunningRenderer {
    pub(crate) fn start(switches: &[&str]) -> Self {
        use protocol::control_channel::{config, send_over, to_renderer};

        let mut command = Command::new(MACHOPO);
        command
            .arg(shell::RENDERER_PROCESS_SWITCH)
            .args(switches)
            .env("RUST_LOG", "renderer=info")
            .stderr(Stdio::piped());
        let (mut child, channel) = ipc::platform::spawn(&mut command).expect("machopo starts");
        let mut said = BufReader::new(child.stderr.take().expect("its standard error"));
        // The browser's side: an IO thread, and what a renderer starts with.
        let (link, endpoint) = ipc::endpoint::endpoint(channel, config(true));
        let (io, io_handle) = ipc::endpoint::IoLoop::new().expect("a way to wait");
        io_handle.serve(endpoint, Deaf);
        let io_thread = std::thread::spawn(move || io.run());
        let init = browser::ChildInit {
            flags: browser::RendererFlags {
                test_fonts: true,
                web_tests: false,
                wait_for_all_pipeline_stages_before_draw: true,
                automation: false,
                user_agent: browser::user_agent(),
                system_colors: gfx::SystemColors::DEFAULT,
                num_raster_threads: None,
                caret_blink_interval: None,
                parser_budget: Default::default(),
                new_content_rendering_delay: browser::NEW_CONTENT_RENDERING_DELAY,
            },
            gpu: false,
        };
        send_over(&link, to_renderer::MAIN, &init).expect("the channel is there");
        until_it_says(&mut said, "the renderer's threads are started");
        Self {
            child,
            link,
            io_handle,
            io_thread,
        }
    }

    /// The browser goes, with the renderer's threads running and nothing
    /// said to them: what the renderer's process exits with.
    pub(crate) fn browser_goes(mut self) -> Option<i32> {
        self.io_handle.stop();
        self.io_thread.join().expect("the IO thread ends");
        drop(self.link);
        exit_of(&mut self.child)
    }
}

#[test]
fn a_running_renderer_whose_browser_goes_ends() {
    // They are ended for it.
    assert_eq!(RunningRenderer::start(&[]).browser_goes(), Some(0));
}

#[test]
fn a_renderer_whose_browsers_end_was_gone_already_has_no_browser() {
    let mut command = Command::new(MACHOPO);
    command
        .arg(shell::RENDERER_PROCESS_SWITCH)
        .stderr(Stdio::null());
    let (mut child, channel) = ipc::platform::spawn(&mut command).expect("machopo starts");
    // Gone at once: before the renderer took its end (it finds none to
    // talk to), or just after (it hears). Either way it doesn't stay.
    drop(channel);
    let exit = exit_of(&mut child);
    assert!(
        exit == Some(0) || exit == Some(i32::from(shell::NO_CHANNEL_EXIT)),
        "{exit:?}"
    );
}

#[test]
fn the_switch_is_one_only_as_the_first_argument() {
    // Anywhere else it is an argument of the browser's, which has none
    // such.
    let out = Command::new(MACHOPO)
        .args(["--test-fonts", shell::RENDERER_PROCESS_SWITCH])
        .stdin(Stdio::null())
        .output()
        .expect("machopo runs");
    assert_eq!(out.status.code(), Some(2));
    let said = String::from_utf8_lossy(&out.stderr);
    assert!(!said.contains("no browser to render for"), "{said}");
}

/// The renderer processes the machopo `browser` has started and that are
/// still there.
#[cfg(unix)]
fn renderers_of(browser: u32) -> usize {
    let found = Command::new("pgrep")
        // Without its dashes: `pgrep` would take them for a switch of its
        // own.
        .args(["-P", &browser.to_string(), "-f"])
        .arg(shell::RENDERER_PROCESS_SWITCH.trim_start_matches('-'))
        .output()
        .expect("pgrep runs");
    String::from_utf8_lossy(&found.stdout).lines().count()
}

#[cfg(unix)]
#[test]
fn tabs_renderers_are_processes_unless_machopo_is_told_otherwise() {
    // The pipe's machopo has a tab from the start: its renderer is a
    // process of its own, with the switch that says so or without, and
    // threads of machopo's with the other.
    for (switch, renderers) in [
        (Some("--process-per-tab"), 1),
        (None, 1),
        (Some("--single-process"), 0),
    ] {
        let mut command = Command::new(MACHOPO);
        command
            .args(["--remote-debugging-pipe", "--headless", "--test-fonts"])
            .args(switch)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let (mut browser, input, _output) = start_machopo(&mut command);
        // As long as a renderer takes to be started, and to be seen not
        // to have been.
        let started = Instant::now();
        while renderers_of(browser.id()) != renderers || started.elapsed() < Duration::from_secs(2)
        {
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "{switch:?}: {} renderer processes",
                renderers_of(browser.id())
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        // The pipe closed is the browser's end, and its renderer's with
        // it.
        let pid = browser.id();
        drop(input);
        assert_eq!(exit_of(&mut browser), Some(0), "{switch:?}");
        assert_eq!(renderers_of(pid), 0, "{switch:?}");
    }
}

/// What descriptor 1 of process `pid` is, as the system tells it: the
/// same for two processes that write to the same pipe.
#[cfg(target_os = "linux")]
fn standard_output_of(pid: u32) -> String {
    std::fs::read_link(format!("/proc/{pid}/fd/1"))
        .expect("its standard output")
        .to_string_lossy()
        .into_owned()
}

/// What descriptor 1 of process `pid` is, as `lsof` tells it: its type,
/// and for a pipe the other end's address, the same for two processes
/// that write to the same pipe.
#[cfg(target_os = "macos")]
fn standard_output_of(pid: u32) -> String {
    let found = Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "1", "-F", "tn"])
        .output()
        .expect("lsof runs");
    String::from_utf8_lossy(&found.stdout)
        .lines()
        .filter(|line| line.starts_with('t') || line.starts_with('n'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// A renderer's standard output is the browser's, as a Chromium child's
/// is: the pipe is apart from it, at 3 and 4.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn a_renderers_standard_output_is_the_browsers() {
    let mut command = Command::new(MACHOPO);
    command
        .args(["--remote-debugging-pipe", "--headless", "--test-fonts"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let (mut browser, input, _output) = start_machopo(&mut command);
    let started = Instant::now();
    let renderer = loop {
        let found = Command::new("pgrep")
            .args(["-P", &browser.id().to_string(), "-f"])
            .arg(shell::RENDERER_PROCESS_SWITCH.trim_start_matches('-'))
            .output()
            .expect("pgrep runs");
        if let Some(pid) = String::from_utf8_lossy(&found.stdout)
            .lines()
            .next()
            .and_then(|line| line.trim().parse::<u32>().ok())
        {
            break pid;
        }
        assert!(started.elapsed() < Duration::from_secs(20), "no renderer");
        std::thread::sleep(Duration::from_millis(50));
    };
    let browsers = standard_output_of(browser.id());
    assert!(browsers.to_lowercase().contains("pipe"), "{browsers}");
    assert_eq!(standard_output_of(renderer), browsers);
    drop(input);
    assert_eq!(exit_of(&mut browser), Some(0));
}
