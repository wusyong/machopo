//! `machopo --remote-debugging-pipe --headless`: the browser driven through
//! a pipe at its file descriptors 3 and 4 (on Windows, the handles
//! `--remote-debugging-io-pipes` names) with DevTools-protocol messages,
//! each followed by a NUL, as Chrome's `--remote-debugging-pipe` is. What a driver relies on: the tabs as targets, a
//! session's navigations, their loaders and lifecycle events, the history,
//! the errors, and the browser ending when its input does or at
//! `Browser.close`; and the window, as machopo's `Machopo` domain and `Browser`'s
//! window methods use it.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, PipeReader, PipeWriter, Read, Write};
use std::process::{Child, Command as Process, Stdio};
use std::time::Duration;

use base64::Engine;
use crossbeam_channel::{Receiver, unbounded};
use net::test_server::{Route, TestServer};
use protocol::automation::{
    Command, Error, Event, EventKind, INVALID_PARAMS, METHOD_NOT_FOUND, Message, Method,
    SERVER_ERROR, SESSION_NOT_FOUND,
};
use serde_json::{Value, json};

/// Set to run these tests with `machopo --single-process`: each tab's
/// renderer threads of machopo's own process, not the process of its own
/// it is by default.
const SINGLE_PROCESS: &str = "MACHOPO_SINGLE_PROCESS";

/// The switches that say where a tab's renderer runs.
const PROCESS_SWITCHES: [&str; 2] = ["--process-per-tab", "--single-process"];

const TIMEOUT: Duration = Duration::from_secs(30);

/// A `machopo --remote-debugging-pipe --headless`, and what it has sent that wasn't asked for
/// yet. Other modules drive machopo with it too (`trace`).
pub(crate) struct Browser {
    pub(crate) child: Child,
    input: Option<PipeWriter>,
    messages: Receiver<Message>,
    next_id: i32,
    events: Vec<Event>,
}

/// Whether `object`, a message from a renderer, is an event of its
/// (`Page.frameNavigated`, which a session with `Page.enable` hears): not an
/// answer.
fn renderer_event(object: &serde_json::Map<String, Value>) -> bool {
    object.contains_key("method") && !object.contains_key("id")
}

impl Browser {
    fn start() -> Self {
        Self::start_with(&[])
    }

    /// With `args` too.
    fn start_with(args: &[&str]) -> Self {
        Self::start_logging(args, Stdio::null())
    }

    /// With its stderr (the browser's log and the page's output) going to
    /// `stderr`.
    fn start_logging(args: &[&str], stderr: Stdio) -> Self {
        Self::start_in(args, &[], stderr)
    }

    /// With the environment variables `env` set.
    pub(crate) fn start_with_env(env: &[(&str, &std::ffi::OsStr)]) -> Self {
        Self::start_in(&[], env, Stdio::null())
    }

    /// With the arguments `args` after its own, and `env` set.
    pub(crate) fn start_with_args_and_env(args: &[&str], env: &[(&str, &std::ffi::OsStr)]) -> Self {
        Self::start_in(args, env, Stdio::null())
    }

    /// With the system's fonts, not the built-in ones every other test
    /// here has: what is drawn is the machine's.
    pub(crate) fn start_with_system_fonts(args: &[&str]) -> Self {
        Self::start_headless_or_not(true, false, &[], args, &[], Stdio::null())
    }

    pub(crate) fn start_in(args: &[&str], env: &[(&str, &std::ffi::OsStr)], stderr: Stdio) -> Self {
        Self::start_headless_or_not(true, true, &[], args, env, stderr)
    }

    /// In the platform's window (no `--headless`).
    fn start_in_the_real_window() -> Self {
        Self::start_headless_or_not(false, true, &[], &[], &[], Stdio::null())
    }

    /// [`Browser::start_logging`], machopo started by the program and
    /// arguments `wrapper` (`machopo`'s path and arguments after them).
    #[cfg_attr(
        not(target_os = "linux"),
        expect(dead_code, reason = "only Linux's sandbox tests start machopo so")
    )]
    pub(crate) fn start_wrapped(wrapper: &[&str], args: &[&str], stderr: Stdio) -> Self {
        Self::start_headless_or_not(true, true, wrapper, args, &[], stderr)
    }

    fn start_headless_or_not(
        headless: bool,
        test_fonts: bool,
        wrapper: &[&str],
        args: &[&str],
        env: &[(&str, &std::ffi::OsStr)],
        stderr: Stdio,
    ) -> Self {
        let machopo = env!("CARGO_BIN_EXE_machopo");
        let mut command = match wrapper {
            [] => Process::new(machopo),
            [program, arguments @ ..] => {
                let mut command = Process::new(program);
                command.args(arguments).arg(machopo);
                command
            }
        };
        command
            .arg("--remote-debugging-pipe")
            .args(test_fonts.then_some("--test-fonts"))
            .args(headless.then_some("--headless"))
            // The same tests with each tab's renderer in machopo's own
            // process, when the run asks for it (but for a test that says
            // which itself).
            .args(
                std::env::var_os(SINGLE_PROCESS)
                    .filter(|_| !args.iter().any(|arg| PROCESS_SWITCHES.contains(arg)))
                    .map(|_| "--single-process"),
            )
            .args(args)
            .envs(env.iter().copied())
            .stdout(Stdio::null())
            .stderr(stderr);
        let (child, input, output) = start_machopo(&mut command);
        let input = Some(input);
        let (tx, messages) = unbounded();
        std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            let mut bytes = Vec::new();
            loop {
                bytes.clear();
                match output.read_until(0, &mut bytes) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                assert_eq!(bytes.pop(), Some(0), "every message ends with a NUL");
                let text = String::from_utf8(bytes.clone()).expect("UTF-8");
                let message = Message::from_json(&text).unwrap_or_else(|| panic!("{text}"));
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            input,
            messages,
            next_id: 0,
            events: Vec::new(),
        }
    }

    fn write(&mut self, text: &str) {
        let input = self.input.as_mut().expect("open");
        input.write_all(text.as_bytes()).unwrap();
        input.write_all(&[0]).unwrap();
        input.flush().unwrap();
    }

    /// Sends `method` with `params`, for `session`; its answer. The events
    /// before it are kept.
    pub(crate) fn call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Result<Value, Error> {
        let id = self.send(session, method, params);
        self.answer(id)
    }

    /// Sends `method` for `session`, not waiting for its answer: its id.
    fn send(&mut self, session: Option<&str>, method: &str, params: Value) -> i32 {
        self.next_id += 1;
        let id = self.next_id;
        let mut command = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            command["sessionId"] = session.into();
        }
        self.write(&command.to_string());
        id
    }

    fn answer(&mut self, id: i32) -> Result<Value, Error> {
        loop {
            match self
                .messages
                .recv_timeout(TIMEOUT)
                .expect("machopo answers")
            {
                Message::Response(response) => {
                    assert_eq!(response.id, id);
                    return response.result;
                }
                Message::Event(event) => self.events.push(event),
                // The renderer's `Page` events (`frameNavigated`...).
                Message::Raw { object, .. } if renderer_event(&object) => {}
                Message::Raw { object, .. } => panic!("unasked {object:?}"),
            }
        }
    }

    /// The first event since the last call (kept or to come) that `wanted`
    /// picks, and the ones before it are dropped.
    fn wait_for<T>(&mut self, mut wanted: impl FnMut(&Event) -> Option<T>) -> T {
        while !self.events.is_empty() {
            let event = self.events.remove(0);
            if let Some(found) = wanted(&event) {
                return found;
            }
        }
        loop {
            match self.messages.recv_timeout(TIMEOUT).expect("an event comes") {
                Message::Event(event) => {
                    if let Some(found) = wanted(&event) {
                        return found;
                    }
                }
                Message::Response(response) => panic!("unasked answer {response:?}"),
                // The renderer's `Page` events (`frameNavigated`...).
                Message::Raw { object, .. } if renderer_event(&object) => {}
                Message::Raw { object, .. } => panic!("unasked {object:?}"),
            }
        }
    }

    /// Attaches to the first tab, with its page and lifecycle events: the
    /// session.
    pub(crate) fn attach(&mut self) -> String {
        let target = self.targets()[0].clone();
        self.attach_to(&target)
    }

    /// The tabs' target ids, in the tab strip's order.
    fn targets(&mut self) -> Vec<String> {
        let targets = self.call(None, "Target.getTargets", json!({})).unwrap();
        targets["targetInfos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|info| info["targetId"].as_str().unwrap().to_owned())
            .collect()
    }

    /// [`Self::attach`], to the tab `target`.
    fn attach_to(&mut self, target: &str) -> String {
        let attached = self
            .call(
                None,
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
            )
            .unwrap();
        let session = attached["sessionId"].as_str().unwrap().to_owned();
        self.call(Some(&session), "Page.enable", json!({})).unwrap();
        self.call(
            Some(&session),
            "Page.setLifecycleEventsEnabled",
            json!({ "enabled": true }),
        )
        .unwrap();
        session
    }

    /// Navigates and waits for the loader's `load`: its id.
    pub(crate) fn navigate(&mut self, session: &str, url: &str) -> String {
        let loader = self.start_navigation(session, url);
        self.load(&loader);
        loader
    }

    /// Starts navigating to `url`, without waiting for its `load`: its
    /// loader.
    pub(crate) fn start_navigation(&mut self, session: &str, url: &str) -> String {
        let result = self
            .call(Some(session), "Page.navigate", json!({ "url": url }))
            .unwrap();
        result["loaderId"].as_str().unwrap().to_owned()
    }

    /// Waits for `loader`'s document's `load`, then for the frame that
    /// shows it to be drawn (`Machopo.waitForLoad`), as machopodriver does: what
    /// comes next (input, a screenshot) finds the page on the screen.
    fn load(&mut self, loader: &str) {
        self.wait_for(|event| match &event.event {
            EventKind::LifecycleEvent {
                loader_id, name, ..
            } if loader_id == loader && name == "load" => Some(()),
            _ => None,
        });
        self.call(None, "Machopo.waitForLoad", json!({})).unwrap();
    }

    /// `Machopo.queryElement`'s result for `selector`.
    pub(crate) fn query(&mut self, selector: &str) -> Value {
        self.call(
            None,
            "Machopo.queryElement",
            json!({ "selector": selector }),
        )
        .unwrap()
    }

    /// The center of what `selector` picks, CSS px of the window.
    fn center(&mut self, selector: &str) -> (f64, f64) {
        let rect = &self.query(selector)["element"]["rect"];
        let at = |start: &str, size: &str| {
            rect[start].as_f64().unwrap() + rect[size].as_f64().unwrap() / 2.0
        };
        (at("x", "width"), at("y", "height"))
    }

    fn input(&mut self, input: Value) {
        self.call(None, "Machopo.dispatchWindowInput", input)
            .unwrap();
        self.call(None, "Machopo.waitForInputHandled", json!({}))
            .unwrap();
    }

    fn click(&mut self, selector: &str) {
        let (x, y) = self.center(selector);
        self.input(json!({ "type": "click", "x": x, "y": y }));
    }

    /// The window's next frame: its size, and its pixels.
    fn capture(&mut self) -> ((u64, u64), Vec<u8>) {
        let frame = self.call(None, "Machopo.captureWindow", json!({})).unwrap();
        let pixels = base64::engine::general_purpose::STANDARD
            .decode(frame["data"].as_str().unwrap())
            .unwrap();
        let size = (
            frame["width"].as_u64().unwrap(),
            frame["height"].as_u64().unwrap(),
        );
        assert_eq!(pixels.len() as u64, size.0 * size.1 * 4);
        (size, pixels)
    }

    /// The window's size, as `Browser.getWindowForTarget` has it.
    fn window_size(&mut self) -> (u64, u64) {
        let window = self
            .call(None, "Browser.getWindowForTarget", json!({}))
            .unwrap();
        assert_eq!(window["windowId"], 1);
        let bounds = &window["bounds"];
        (
            bounds["width"].as_u64().unwrap(),
            bounds["height"].as_u64().unwrap(),
        )
    }

    /// Closes the input: machopo ends, with the status it ends with.
    pub(crate) fn end(mut self) -> std::process::ExitStatus {
        drop(self.input.take());
        wait(&mut self.child)
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Starts `command` (machopo's executable, with `--remote-debugging-pipe`) with
/// a pipe, as Chrome's clients start Chrome: what is written to the
/// [`PipeWriter`] is its input, and the [`PipeReader`] has what it writes.
/// Its standard input is nothing; its output is what `command` says.
pub(crate) fn start_machopo(command: &mut Process) -> (Child, PipeWriter, PipeReader) {
    command.stdin(Stdio::null());
    ipc::platform::spawn_with_pipe(command).expect("machopo starts")
}

fn wait(child: &mut Child) -> std::process::ExitStatus {
    let deadline = std::time::Instant::now() + TIMEOUT;
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        assert!(std::time::Instant::now() < deadline, "machopo didn't end");
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

/// A navigation's loader is told as it starts, then its
/// `DOMContentLoaded` and `load`; the target shows its URL and title.
#[test]
fn a_session_navigates_and_hears_the_loaders_lifecycle() {
    let server = server(&[(
        "/",
        Route::ok("text/html", "<!DOCTYPE html><title>One</title>one"),
    )]);
    let mut browser = Browser::start();
    let session = browser.attach();
    let url = server.url("/");
    let result = browser
        .call(Some(&session), "Page.navigate", json!({ "url": url }))
        .unwrap();
    let loader = result["loaderId"].as_str().unwrap().to_owned();
    let target = result["frameId"].as_str().unwrap().to_owned();
    // Told before the answer.
    assert!(
        browser.events.iter().any(|event| event.event
            == EventKind::FrameStartedNavigating {
                frame_id: target.clone(),
                url: url.clone(),
                loader_id: loader.clone(),
                navigation_type: "differentDocument".into(),
            }
            && event.session_id.as_deref() == Some(&session)),
        "{:?}",
        browser.events
    );
    let mut names = Vec::new();
    browser.wait_for(|event| match &event.event {
        EventKind::LifecycleEvent {
            loader_id, name, ..
        } if *loader_id == loader => {
            names.push(name.clone());
            (name == "load").then_some(())
        }
        _ => None,
    });
    assert_eq!(names, ["init", "DOMContentLoaded", "load"]);
    let info = browser
        .call(Some(&session), "Target.getTargetInfo", json!({}))
        .unwrap();
    assert_eq!(info["targetInfo"]["url"], url.as_str());
    assert_eq!(info["targetInfo"]["title"], "One");
    assert_eq!(info["targetInfo"]["targetId"], target.as_str());
    assert_eq!(info["targetInfo"]["attached"], true);
    assert!(browser.end().success());
}

/// The target's URL is the address bar's (`RenderFrameDevToolsAgentHost::
/// GetURL`): where a navigation the browser started goes as it loads, the
/// page shown's while the page's own navigation is on its way; and
/// `Page.frameStartedNavigating` tells where the page's goes.
#[test]
fn the_targets_url_is_the_address_bars() {
    let slow = || Route::ok("text/html", "<!DOCTYPE html>slow").with_delay(Duration::from_secs(2));
    let server = server(&[("/slow", slow()), ("/page-slow", slow())]);
    let mut browser = Browser::start();
    let session = browser.attach();
    let slow = server.url("/slow");
    // `Page.navigate` answers once its navigation has committed: what
    // `Target.getTargetInfo`, sent after it, answers comes first.
    let navigate = browser.send(Some(&session), "Page.navigate", json!({ "url": slow }));
    let info = browser.send(Some(&session), "Target.getTargetInfo", json!({}));
    let url = |info: Value| info["targetInfo"]["url"].as_str().unwrap().to_owned();
    assert_eq!(url(browser.answer(info).unwrap()), slow, "on its way");
    let loader = browser.answer(navigate).unwrap()["loaderId"]
        .as_str()
        .unwrap()
        .to_owned();
    browser.load(&loader);
    browser
        .call(Some(&session), "Page.enable", json!({}))
        .unwrap();
    browser
        .call(
            Some(&session),
            "Runtime.evaluate",
            json!({ "expression": "location = '/page-slow'" }),
        )
        .unwrap();
    let started = browser.wait_for(|event| match &event.event {
        EventKind::FrameStartedNavigating { url, .. } if url.ends_with("/page-slow") => {
            Some(url.clone())
        }
        _ => None,
    });
    assert_eq!(started, server.url("/page-slow"));
    let info = browser
        .call(Some(&session), "Target.getTargetInfo", json!({}))
        .unwrap();
    assert_eq!(url(info), slow, "the page's on its way");
    assert!(browser.end().success());
}

/// A navigation of the session's that loads as the tab's renderer is
/// ended (`about:crash`) goes on, and commits in a new renderer, which the
/// session is attached to: its commands go there.
#[test]
fn a_navigation_on_its_way_as_the_renderer_goes_commits_in_a_new_one() {
    let server = server(&[(
        "/slow",
        Route::ok("text/html", "<!DOCTYPE html><title>slow</title>")
            .with_delay(Duration::from_millis(500)),
    )]);
    let mut browser = Browser::start();
    let session = browser.attach();
    let slow = server.url("/slow");
    // `Page.navigate` answers as its navigation is about to commit;
    // `about:crash` starts none, and is answered with the one on its way.
    let navigate = browser.send(Some(&session), "Page.navigate", json!({ "url": slow }));
    let crash = browser.send(
        Some(&session),
        "Page.navigate",
        json!({ "url": "about:crash" }),
    );
    let loader = browser.answer(navigate).unwrap()["loaderId"]
        .as_str()
        .unwrap()
        .to_owned();
    browser.answer(crash).unwrap();
    browser.load(&loader);
    let title = browser
        .call(
            Some(&session),
            "Runtime.evaluate",
            json!({ "expression": "document.title", "returnByValue": true }),
        )
        .unwrap();
    assert_eq!(title["result"]["value"], "slow");
    assert!(browser.end().success());
}

/// The answers to the commands `ids`, in whatever order they come: the
/// events meanwhile are kept.
fn answers(browser: &mut Browser, ids: &[i32]) -> HashMap<i32, Result<Value, Error>> {
    let mut answers = HashMap::new();
    while answers.len() < ids.len() {
        let Ok(message) = browser.messages.recv_timeout(TIMEOUT) else {
            panic!("only {answers:?} of {ids:?}");
        };
        match message {
            Message::Response(response) if ids.contains(&response.id) => {
                answers.insert(response.id, response.result);
            }
            Message::Response(response) => panic!("unasked answer {response:?}"),
            Message::Event(event) => browser.events.push(event),
            // The renderer's `Page` events (`frameNavigated`...).
            Message::Raw { object, .. } if renderer_event(&object) => {}
            Message::Raw { object, .. } => panic!("unasked {object:?}"),
        }
    }
    answers
}

/// Two sites on one server: its address's, and `localhost`'s.
fn other_site(server: &TestServer, path: &str) -> String {
    server.url_for_host("localhost", path)
}

/// Commands on their way to the tab's renderer as a renderer of another
/// site takes its place (Chromium's `DevToolsSession` as it attaches to
/// the new agent): one that may run the page's script fails, the document
/// it was for being no more; any other goes to the new renderer, and is
/// answered there.
#[test]
fn commands_on_their_way_as_another_sites_renderer_takes_the_tabs_place() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>a")),
        (
            "/b",
            Route::ok("text/html", "<!DOCTYPE html><title>b</title>"),
        ),
    ]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    // The page keeps its renderer busy a while, and what is sent it
    // meanwhile waits there.
    script(
        &mut browser,
        &session,
        "setTimeout(() => { const end = Date.now() + 3000; while (Date.now() < end); }, 0)",
        json!([]),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let waits = browser.send(Some(&session), "Page.getFrameTree", json!({}));
    let runs = browser.send(
        Some(&session),
        "Runtime.evaluate",
        json!({ "expression": "1" }),
    );
    let navigate = browser.send(
        Some(&session),
        "Page.navigate",
        json!({ "url": other_site(&server, "/b") }),
    );
    let answers = answers(&mut browser, &[waits, runs, navigate]);
    assert!(answers[&navigate].is_ok(), "{answers:?}");
    let error = answers[&runs].clone().unwrap_err();
    assert_eq!(error.code, SERVER_ERROR);
    assert_eq!(error.message, "Inspected target navigated or closed");
    let tree = answers[&waits].clone().unwrap();
    assert!(tree["frameTree"]["frame"]["id"].is_string(), "{tree}");
    let loader = answers[&navigate].clone().unwrap()["loaderId"]
        .as_str()
        .unwrap()
        .to_owned();
    browser.load(&loader);
    let title = browser
        .call(
            Some(&session),
            "Runtime.evaluate",
            json!({ "expression": "document.title", "returnByValue": true }),
        )
        .unwrap();
    assert_eq!(title["result"]["value"], "b");
    assert!(browser.end().success());
}

/// The document a renderer of another site takes the place of is unloaded
/// in its own renderer, its `pagehide` and `unload` run: what they write
/// to `localStorage` is there when the tab is back at its site. With the
/// renderer a process of its own, and threads of machopo's.
#[test]
fn a_documents_unload_runs_as_another_site_takes_its_place() {
    let server = server(&[
        (
            "/a",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>localStorage.x = 'start';
                addEventListener('pagehide', () => { localStorage.x = 'pagehide'; });
                addEventListener('unload', () => { localStorage.x += ',unload'; });</script>",
            ),
        ),
        ("/b", Route::ok("text/html", "<!DOCTYPE html>b")),
        ("/c", Route::ok("text/html", "<!DOCTYPE html>c")),
    ]);
    for mode in PROCESS_SWITCHES {
        let mut browser = Browser::start_with(&[mode]);
        let session = browser.attach();
        browser.navigate(&session, &server.url("/a"));
        browser.navigate(&session, &other_site(&server, "/b"));
        browser.navigate(&session, &server.url("/c"));
        let x = script(&mut browser, &session, "return localStorage.x", json!([])).unwrap();
        assert_eq!(x, "pagehide,unload", "{mode}");
        assert!(browser.end().success(), "{mode}");
    }
}

/// A renderer paused in the debugger as another site's takes the tab's
/// place goes on, to unload its document: the session is the new
/// renderer's, and nothing would end the pause.
#[test]
fn a_renderer_paused_in_the_debugger_unloads_as_another_site_takes_its_place() {
    let server = server(&[
        (
            "/a",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>localStorage.y = 'start';
                addEventListener('pagehide', () => { localStorage.y = 'pagehide'; });</script>",
            ),
        ),
        ("/b", Route::ok("text/html", "<!DOCTYPE html>b")),
        ("/c", Route::ok("text/html", "<!DOCTYPE html>c")),
    ]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/a"));
    browser
        .call(Some(&session), "Debugger.enable", json!({}))
        .unwrap();
    script(
        &mut browser,
        &session,
        "setTimeout(() => { debugger; })",
        json!([]),
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    browser.navigate(&session, &other_site(&server, "/b"));
    browser.navigate(&session, &server.url("/c"));
    let y = script(&mut browser, &session, "return localStorage.y", json!([])).unwrap();
    assert_eq!(y, "pagehide");
    assert!(browser.end().success());
}

/// A renderer whose document's `unload` never ends is ended: the tab's
/// next document, another site's, doesn't wait for it, and its process is
/// gone a moment later (Chromium's `kUnloadTimeout`, 500 ms).
#[cfg(unix)]
#[test]
fn a_renderer_whose_unload_doesnt_end_is_ended() {
    let server = server(&[
        (
            "/a",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>addEventListener('unload', () => { for (;;) {} });</script>",
            ),
        ),
        (
            "/b",
            Route::ok("text/html", "<!DOCTYPE html><title>b</title>"),
        ),
    ]);
    let mut browser = Browser::start_with(&["--process-per-tab"]);
    let session = browser.attach();
    browser.navigate(&session, &server.url("/a"));
    let pid = renderer_pids(&mut browser)[0];
    browser.navigate(&session, &other_site(&server, "/b"));
    assert_ne!(renderer_pids(&mut browser), [pid]);
    let alive = || {
        Process::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    let asked = std::time::Instant::now();
    while alive() {
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "the renderer process {pid} is still there"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(browser.end().success());
}

/// The session history: back to another document loads it; back to an
/// entry the document made of itself (`pushState`) doesn't, and is told
/// as a navigation within the document; a reload is a loader of its own.
#[test]
fn a_session_goes_through_the_history() {
    let server = server(&[
        ("/a", Route::ok("text/html", "<!DOCTYPE html>a")),
        (
            "/b",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>onload = () => history.pushState(null, '', '/b2')</script>",
            ),
        ),
    ]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/a"));
    let result = browser
        .call(
            Some(&session),
            "Page.navigate",
            json!({ "url": server.url("/b") }),
        )
        .unwrap();
    let b = result["loaderId"].as_str().unwrap().to_owned();
    // `onload`'s `pushState` may come before or after the loader's `load`.
    let (mut loaded, mut pushed) = (false, None);
    browser.wait_for(|event| {
        match &event.event {
            EventKind::LifecycleEvent {
                loader_id, name, ..
            } if *loader_id == b && name == "load" => loaded = true,
            EventKind::NavigatedWithinDocument {
                url,
                navigation_type,
                ..
            } => pushed = Some((url.clone(), navigation_type.clone())),
            _ => {}
        }
        (loaded && pushed.is_some()).then_some(())
    });
    assert_eq!(pushed, Some((server.url("/b2"), "historyApi".into())));
    let history = browser
        .call(Some(&session), "Page.getNavigationHistory", json!({}))
        .unwrap();
    let urls: Vec<&str> = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["url"].as_str().unwrap())
        .collect();
    assert_eq!(
        urls,
        [server.url("/a"), server.url("/b"), server.url("/b2")]
    );
    assert_eq!(history["currentIndex"], 2);
    let entry = |index: u64| history["entries"][index as usize]["id"].clone();

    // Within the document: no loader.
    browser
        .call(
            Some(&session),
            "Page.navigateToHistoryEntry",
            json!({ "entryId": entry(1) }),
        )
        .unwrap();
    let within = browser.wait_for(|event| match &event.event {
        EventKind::NavigatedWithinDocument {
            url,
            navigation_type,
            ..
        } => Some((url.clone(), navigation_type.clone())),
        EventKind::FrameStartedNavigating { .. } => panic!("{event:?}"),
        _ => None,
    });
    assert_eq!(within, (server.url("/b"), "other".into()));

    // To another document: a loader.
    browser
        .call(
            Some(&session),
            "Page.navigateToHistoryEntry",
            json!({ "entryId": entry(0) }),
        )
        .unwrap();
    let (loader, kind) = browser.wait_for(|event| match &event.event {
        EventKind::FrameStartedNavigating {
            loader_id,
            navigation_type,
            url,
            ..
        } => {
            assert_eq!(*url, server.url("/a"));
            Some((loader_id.clone(), navigation_type.clone()))
        }
        _ => None,
    });
    assert_eq!(kind, "historyDifferentDocument");
    assert!(loader.parse::<u64>().unwrap() > b.parse::<u64>().unwrap());
    browser.load(&loader);

    browser
        .call(Some(&session), "Page.reload", json!({}))
        .unwrap();
    let kind = browser.wait_for(|event| match &event.event {
        EventKind::FrameStartedNavigating {
            navigation_type, ..
        } => Some(navigation_type.clone()),
        _ => None,
    });
    assert_eq!(kind, "reload");
    let missing = browser
        .call(
            Some(&session),
            "Page.navigateToHistoryEntry",
            json!({ "entryId": 99 }),
        )
        .unwrap_err();
    assert_eq!(missing.message, "No entry with passed id");
    assert!(browser.end().success());
}

/// With target discovery on, the tabs there are and a tab whose renderer
/// ends are told. (A tab a link opens needs input, which this test
/// doesn't give.)
#[test]
fn discovery_tells_of_the_tabs() {
    let mut browser = Browser::start();
    let session = browser.attach();
    browser
        .call(
            None,
            "Target.setDiscoverTargets",
            json!({ "discover": true }),
        )
        .unwrap();
    let first = browser.wait_for(|event| match &event.event {
        EventKind::TargetCreated { target_info } => Some(target_info.clone()),
        _ => None,
    });
    assert!(first.attached);
    browser
        .call(
            Some(&session),
            "Page.navigate",
            json!({ "url": "about:crash" }),
        )
        .unwrap();
    let crashed = browser.wait_for(|event| match &event.event {
        EventKind::TargetCrashed { target_id, .. } => Some(target_id.clone()),
        _ => None,
    });
    assert_eq!(crashed, first.target_id);
    // The tab stays, with its sad tab.
    let targets = browser.call(None, "Target.getTargets", json!({})).unwrap();
    assert_eq!(targets["targetInfos"].as_array().unwrap().len(), 1);
    assert!(browser.end().success());
}

/// What isn't right is answered with CDP's errors: a method machopo doesn't
/// have, a parameter it doesn't take, a page's method without a session
/// or with one that isn't.
#[test]
fn wrong_commands_get_cdps_errors() {
    let mut browser = Browser::start();
    let code = |result: Result<Value, Error>| result.unwrap_err().code;
    assert_eq!(
        code(browser.call(None, "Page.fly", json!({}))),
        METHOD_NOT_FOUND
    );
    assert_eq!(
        code(browser.call(None, "Page.enable", json!({}))),
        METHOD_NOT_FOUND
    );
    assert_eq!(
        code(browser.call(Some("nope"), "Page.enable", json!({}))),
        SESSION_NOT_FOUND
    );
    let session = browser.attach();
    let unknown_param = browser.call(
        Some(&session),
        "Page.navigate",
        json!({ "url": "about:blank", "transitionType": "typed" }),
    );
    assert_eq!(code(unknown_param), INVALID_PARAMS);
    assert_eq!(
        code(browser.call(Some(&session), "Page.navigate", json!({ "url": 1 }))),
        INVALID_PARAMS
    );
    assert_eq!(
        code(browser.call(Some(&session), "Page.enable", json!({ "x": 1 }))),
        INVALID_PARAMS
    );
    let invalid = browser
        .call(
            Some(&session),
            "Page.navigate",
            json!({ "url": "not a url" }),
        )
        .unwrap_err();
    assert_eq!(invalid.message, "Cannot navigate to invalid URL");
    // A message that isn't JSON, or whose id isn't an `int`, has no id to
    // answer: the error comes without one, as Chrome's; the next one is
    // answered.
    for wrong in ["{", r#"{"id": 2147483648, "method": "Browser.getVersion"}"#] {
        browser.write(wrong);
        let (session_id, object) = loop {
            match browser.messages.recv_timeout(TIMEOUT).unwrap() {
                Message::Raw { object, .. } if renderer_event(&object) => {}
                Message::Raw { session_id, object } => break (session_id, object),
                other => panic!("an error with no id: {other:?}"),
            }
        };
        assert_eq!(session_id, None);
        assert!(object.get("id").is_none(), "{object:?}");
        assert_eq!(object["error"]["code"], -32600, "{object:?}");
    }
    // Negative ids and `1e3` are `int`s.
    browser.write(r#"{"id": -1, "method": "Browser.getVersion"}"#);
    assert!(browser.answer(-1).is_ok());
    browser.write(r#"{"id": 1e3, "method": "Browser.getVersion"}"#);
    assert!(browser.answer(1000).is_ok());
    browser.next_id += 1;
    let next = browser.next_id;
    browser.write(
        &Command {
            id: next,
            session_id: None,
            method: Method::BrowserGetVersion,
        }
        .to_json(),
    );
    let version = browser.answer(next).unwrap();
    assert!(
        version["product"]
            .as_str()
            .unwrap()
            .starts_with("Headlessmachopo/")
    );
    assert!(browser.end().success());
}

/// `Browser.close` is answered, then machopo ends.
#[test]
fn browser_close_ends_machopo() {
    let mut browser = Browser::start();
    assert_eq!(
        browser.call(None, "Browser.close", json!({})),
        Ok(json!({}))
    );
    assert!(wait(&mut browser.child).success());
}

/// A page with a child frame loads: `load` waits for the child's.
#[test]
fn a_page_with_a_child_frame_loads() {
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><iframe src='/child'></iframe><img />",
            ),
        ),
        ("/child", Route::ok("text/html", "<!DOCTYPE html>child")),
    ]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    assert!(browser.end().success());
}

/// What the page prints (`console.log`, an uncaught exception, a rejection
/// nothing handled) goes to standard error: standard output is the pipe's,
/// and a line on it would make the next message unreadable.
#[test]
fn the_pages_output_stays_off_the_pipe() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html><script>console.log('hello', [1, 2]); console.error('oops');
             Promise.reject(new Error('rejected'));
             throw new Error('thrown')</script>",
        ),
    )]);
    let mut browser = Browser::start_logging(&[], Stdio::piped());
    let mut stderr = browser.child.stderr.take().expect("piped");
    let log = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    assert!(browser.end().success());
    // It goes to stderr instead, with the browser's log.
    let log = log.join().unwrap();
    for line in [
        "[console] hello 1,2",
        "[console] oops",
        "[uncaught] Error: thrown",
        "[uncaught] (in promise) Error: rejected",
    ] {
        assert!(log.contains(line), "{line} in {log}");
    }
}

/// A session gets the page's events only once it asked for them
/// (`Page.enable`, `Page.setLifecycleEventsEnabled`), and the browser's
/// target events only with discovery on.
#[test]
fn events_come_only_when_asked_for() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>a"))]);
    let mut browser = Browser::start();
    let targets = browser.call(None, "Target.getTargets", json!({})).unwrap();
    let target = targets["targetInfos"][0]["targetId"].clone();
    let attached = browser
        .call(
            None,
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
        )
        .unwrap();
    let session = attached["sessionId"].as_str().unwrap().to_owned();
    browser
        .call(
            Some(&session),
            "Page.navigate",
            json!({ "url": server.url("/") }),
        )
        .unwrap();
    // Long enough for the page to load, and its events to come if any did.
    std::thread::sleep(Duration::from_millis(500));
    browser.call(None, "Browser.getVersion", json!({})).unwrap();
    assert!(browser.events.is_empty(), "{:?}", browser.events);
    assert!(browser.end().success());
}

/// `Browser.getVersion` tells a browser without the platform's window, as
/// Chrome's headless mode does; the page's user agent is the same.
#[test]
fn the_version_tells_a_browser_without_a_window() {
    let mut browser = Browser::start();
    let version = browser.call(None, "Browser.getVersion", json!({})).unwrap();
    let product = version["product"].as_str().unwrap();
    assert!(product.starts_with("Headlessmachopo/"), "{product}");
    assert!(!version["userAgent"].as_str().unwrap().contains("Headless"));
    assert!(browser.end().success());
}

/// `SystemInfo.getInfo` tells the GPU adapter machopo draws with, as Chrome
/// tells its GPU: the device, and machopo's adapter type and backend.
#[test]
fn the_system_info_tells_the_gpu_adapter() {
    let mut browser = Browser::start();
    // Known once Viz has made its renderer, just after machopo starts.
    let start = std::time::Instant::now();
    let info = loop {
        let info = browser.call(None, "SystemInfo.getInfo", json!({})).unwrap();
        if !info["gpu"]["devices"].as_array().unwrap().is_empty() {
            break info;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{info}");
        std::thread::sleep(Duration::from_millis(20));
    };
    let gpu = &info["gpu"];
    let device = &gpu["devices"][0];
    assert!(
        !device["deviceString"].as_str().unwrap().is_empty(),
        "{info}"
    );
    let kinds = ["DiscreteGpu", "IntegratedGpu", "VirtualGpu", "Other", "Cpu"];
    let kind = gpu["auxAttributes"]["adapterType"].as_str().unwrap();
    assert!(kinds.contains(&kind), "{info}");
    assert!(!gpu["auxAttributes"]["backend"].as_str().unwrap().is_empty());
    assert!(browser.end().success());
}

/// `SystemInfo.getProcessInfo` tells the processes, as Chrome's
/// `{processInfo: [{type, id}]}`: the browser's first, then each tab's
/// renderer process, each tab its own.
#[test]
fn the_process_info_tells_the_browser_and_each_renderer() {
    let mut browser = Browser::start_with(&["--process-per-tab"]);
    browser
        .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
        .unwrap();
    let start = std::time::Instant::now();
    let infos = loop {
        let info = browser
            .call(None, "SystemInfo.getProcessInfo", json!({}))
            .unwrap();
        let infos = info["processInfo"].as_array().unwrap().clone();
        if infos.len() == 3 {
            break infos;
        }
        assert!(start.elapsed() < Duration::from_secs(10), "{info}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(infos[0]["type"], "browser");
    assert_eq!(infos[0]["id"], browser.child.id());
    let renderers: Vec<u64> = infos[1..]
        .iter()
        .map(|info| {
            assert_eq!(info["type"], "renderer", "{info}");
            info["id"].as_u64().unwrap()
        })
        .collect();
    assert_ne!(renderers[0], renderers[1]);
    assert!(!renderers.contains(&u64::from(browser.child.id())));
    assert!(browser.end().success());
}

/// With the `census` feature, a renderer process counts into a file of its
/// own, `MACHOPO_CENSUS`'s with its id after it: the browser's process
/// writes that one, and two writing one file would lose one's counts.
#[cfg(feature = "census")]
#[test]
fn a_renderer_process_counts_into_a_census_of_its_own() {
    let server = server(&[(
        "/",
        Route::ok(
            "text/html",
            "<!DOCTYPE html><script>document.title = document.body ? 'a' : 'b';</script>",
        ),
    )]);
    let folder = std::env::temp_dir().join(format!("machopo-census-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let counts = folder.join("c.tsv");
    let mut browser = Browser::start_with_args_and_env(
        &["--process-per-tab"],
        &[("MACHOPO_CENSUS", counts.as_os_str())],
    );
    let session = browser.attach();
    let loader = browser.start_navigation(&session, &server.url("/"));
    browser.load(&loader);
    let info = browser
        .call(None, "SystemInfo.getProcessInfo", json!({}))
        .unwrap();
    let renderer = info["processInfo"][1]["id"].as_u64().unwrap();
    assert!(browser.end().success());
    let own = folder.join(format!("c.tsv.{renderer}"));
    let written = std::fs::read_to_string(&own);
    let _ = std::fs::remove_dir_all(&folder);
    let written = written.unwrap_or_else(|e| panic!("{}: {e}", own.display()));
    assert!(written.starts_with("entry\t"), "{written}");
    assert!(written.lines().count() > 1, "{written}");
}

/// With `--single-process` the browser's process is the only one.
#[test]
fn the_process_info_of_a_single_process() {
    let mut browser = Browser::start_with(&["--single-process"]);
    let info = browser
        .call(None, "SystemInfo.getProcessInfo", json!({}))
        .unwrap();
    assert_eq!(
        info["processInfo"],
        json!([{ "type": "browser", "id": browser.child.id() }])
    );
    assert!(browser.end().success());
}

/// For the `Machopo` domain's tests: a button and what it did, something far
/// below, and stripes of different heights, so every scroll position looks
/// different.
fn machopo_page() -> String {
    let stripes: String = (1..=60)
        .map(|i| {
            format!(
                "<div style='height: {}px; background: {}'></div>",
                20 + i,
                ["red", "blue", "white"][i % 3]
            )
        })
        .collect();
    format!(
        "<!DOCTYPE html><style>html, body {{ margin: 0 }} \
         #b {{ display: block; width: 100px; height: 40px; background: blue }}</style>\
         <title>Machopo</title><span id=b onclick='clicked()'></span><p id=out>not yet</p>\
         {stripes}<p id=far>far</p><script>function clicked() {{ \
         document.getElementById('out').textContent = 'clicked'; }}</script>"
    )
}

/// The `Machopo` domain uses the window as a person would: the address bar,
/// clicks where an element is, keys, what is in the page and its frames.
#[test]
fn the_machopo_domain_uses_the_window() {
    let server = server(&[("/", Route::ok("text/html", machopo_page()))]);
    let mut browser = Browser::start();
    let url = server.url("/");
    browser
        .call(
            None,
            "Machopo.navigateFromAddressBar",
            json!({ "url": url }),
        )
        .unwrap();
    browser
        .call(None, "Machopo.waitForLoad", json!({}))
        .unwrap();
    let targets = browser.call(None, "Target.getTargets", json!({})).unwrap();
    assert_eq!(targets["targetInfos"][0]["url"], url.as_str());

    // What is in the page, and where.
    let found = browser.query("#out");
    assert_eq!(found["element"]["text"], "not yet");
    assert_eq!(found["scale"], 1.0);
    let page = &found["page"];
    let rect = &found["element"]["rect"];
    assert!(rect["y"].as_f64() >= page["y"].as_f64(), "{found}");
    assert_eq!(browser.query("#none")["element"], Value::Null);

    browser.click("#b");
    assert_eq!(browser.query("#out")["element"]["text"], "clicked");

    // Below the window, then scrolled into it.
    let bottom = |found: &Value| {
        found["page"]["y"].as_f64().unwrap() + found["page"]["height"].as_f64().unwrap()
    };
    let far = browser.query("#far");
    assert!(far["element"]["rect"]["y"].as_f64().unwrap() > bottom(&far));
    browser
        .call(
            None,
            "Machopo.scrollIntoViewIfNeeded",
            json!({ "selector": "#far" }),
        )
        .unwrap();
    let far = browser.query("#far");
    assert!(
        far["element"]["rect"]["y"].as_f64().unwrap() < bottom(&far),
        "{far}"
    );

    // Keys, with their modifiers held: Ctrl+End is a shortcut, which
    // doesn't reach the page; Home scrolls it.
    let (size, before) = browser.capture();
    assert_eq!(size, (800, 600));
    browser.input(json!({ "type": "key", "key": "Home", "modifiers": 2 }));
    assert_eq!(browser.capture().1, before);
    browser.input(json!({ "type": "key", "key": "Home" }));
    assert_ne!(browser.capture().1, before);
    // What is drawn now, without waiting for the input to be shown.
    let now = browser
        .call(None, "Machopo.captureWindow", json!({ "now": true }))
        .unwrap();
    assert_eq!(
        (now["width"].as_u64(), now["height"].as_u64()),
        (Some(800), Some(600))
    );

    // A wheel scrolls what is under the pointer.
    let (_, top) = browser.capture();
    let (x, y) = browser.center("#b");
    browser.input(json!({ "type": "wheel", "x": x, "y": y, "deltaY": 200 }));
    assert_ne!(browser.capture().1, top);

    for (input, why) in [
        (json!({ "type": "tap" }), "no input of type \"tap\""),
        (json!({ "type": "key" }), "a key needs `key`"),
        (
            json!({ "type": "key", "key": "Nope" }),
            "unknown key `Nope`",
        ),
    ] {
        let error = browser
            .call(None, "Machopo.dispatchWindowInput", input)
            .unwrap_err();
        assert_eq!(error.code, INVALID_PARAMS);
        assert!(error.message.ends_with(why), "{error:?}");
    }
    assert!(browser.end().success());
}

/// machopo has one window, whose size is set as `Browser.setWindowBounds`
/// says, up to the texture Viz draws it into: what is drawn after is that
/// size.
#[test]
fn the_window_has_bounds_to_set() {
    let server = server(&[("/", Route::ok("text/html", machopo_page()))]);
    let mut browser = Browser::start();
    browser
        .call(
            None,
            "Machopo.navigateFromAddressBar",
            json!({ "url": server.url("/") }),
        )
        .unwrap();
    browser
        .call(None, "Machopo.waitForLoad", json!({}))
        .unwrap();
    assert_eq!(browser.window_size(), (800, 600));
    let targets = browser.call(None, "Target.getTargets", json!({})).unwrap();
    let target = targets["targetInfos"][0]["targetId"].clone();
    let window = browser
        .call(
            None,
            "Browser.getWindowForTarget",
            json!({ "targetId": target }),
        )
        .unwrap();
    assert_eq!(window["windowId"], 1);
    let error = browser
        .call(
            None,
            "Browser.getWindowForTarget",
            json!({ "targetId": "nope" }),
        )
        .unwrap_err();
    assert_eq!(error.code, SERVER_ERROR);

    let set = |browser: &mut Browser, window: u64, bounds: Value| {
        browser.call(
            None,
            "Browser.setWindowBounds",
            json!({ "windowId": window, "bounds": bounds }),
        )
    };
    set(&mut browser, 1, json!({ "width": 400, "height": 300 })).unwrap();
    assert_eq!(browser.window_size(), (400, 300));
    assert_eq!(browser.capture().0, (400, 300));
    // No side past the texture Viz draws the window into.
    set(
        &mut browser,
        1,
        json!({ "width": 100_000, "height": u32::MAX }),
    )
    .unwrap();
    // (Not captured at that size: a quarter of a gigabyte of pixels, which
    // a busy machine doesn't draw, encode and write in time.)
    assert_eq!(browser.window_size(), (8192, 8192));
    set(&mut browser, 1, json!({ "width": 400, "height": 300 })).unwrap();
    // What isn't given stays.
    set(&mut browser, 1, json!({ "width": 500 })).unwrap();
    assert_eq!(browser.window_size(), (500, 300));
    // The page fits the window.
    let found = browser.query("#out");
    assert_eq!(found["page"]["width"], 500.0);

    let error = set(&mut browser, 2, json!({ "width": 400 })).unwrap_err();
    assert_eq!(error.code, SERVER_ERROR);
    let error = set(&mut browser, 1, json!({ "windowState": "maximized" })).unwrap_err();
    assert_eq!(error.code, INVALID_PARAMS);
    assert_eq!(browser.window_size(), (500, 300));

    // Where it is: without the platform's window, only kept; a coordinate
    // not given stays.
    let position = |browser: &mut Browser| {
        let window = browser
            .call(None, "Browser.getWindowForTarget", json!({}))
            .unwrap();
        (
            window["bounds"]["left"].clone(),
            window["bounds"]["top"].clone(),
        )
    };
    assert_eq!(position(&mut browser), (json!(0), json!(0)));
    set(&mut browser, 1, json!({ "left": -10, "top": 20 })).unwrap();
    assert_eq!(position(&mut browser), (json!(-10), json!(20)));
    set(&mut browser, 1, json!({ "top": 30 })).unwrap();
    assert_eq!(position(&mut browser), (json!(-10), json!(30)));
    assert_eq!(browser.window_size(), (500, 300));
    assert!(browser.end().success());
}

/// A capture shows what the input given before it did, and a scroll is
/// answered once the frame showing it is drawn, however long the page
/// keeps its main thread busy: what is drawn next (`now`) shows it then.
#[test]
fn waits_end_with_the_frame_that_shows_them() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
        span { display: block; width: 100px; height: 40px; background: blue }</style>\
        <span id=late onclick='late()'></span><span id=slow onclick='busy(1000)'></span>\
        <div style='height: 3000px'></div><p id=far style='height: 600px; background: red'>far</p>\
        <script>function busy(ms) { const end = Date.now() + ms; while (Date.now() < end) {} }\
        function late() { busy(300); \
        document.getElementById('late').setAttribute('style', 'background: lime'); }</script>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    browser
        .call(
            None,
            "Machopo.navigateFromAddressBar",
            json!({ "url": server.url("/") }),
        )
        .unwrap();
    browser
        .call(None, "Machopo.waitForLoad", json!({}))
        .unwrap();

    // The click's listener changes the color after 300 ms; the capture,
    // asked for once the click is given, waits for it.
    let (x, y) = browser.center("#late");
    browser
        .call(
            None,
            "Machopo.dispatchWindowInput",
            json!({ "type": "click", "x": x, "y": y }),
        )
        .unwrap();
    let ((width, _), pixels) = browser.capture();
    let i = (y as usize * width as usize + x as usize) * 4;
    assert_eq!(
        pixels[i..i + 3],
        [0, 255, 0],
        "the capture shows the click's change"
    );

    // The click keeps the page busy for a second, before or after the
    // scroll.
    let (_, before) = browser.capture();
    let (x, y) = browser.center("#slow");
    browser
        .call(
            None,
            "Machopo.dispatchWindowInput",
            json!({ "type": "click", "x": x, "y": y }),
        )
        .unwrap();
    browser
        .call(
            None,
            "Machopo.scrollIntoViewIfNeeded",
            json!({ "selector": "#far" }),
        )
        .unwrap();
    let now = browser
        .call(None, "Machopo.captureWindow", json!({ "now": true }))
        .unwrap();
    let now = base64::engine::general_purpose::STANDARD
        .decode(now["data"].as_str().unwrap())
        .unwrap();
    assert_ne!(now, before, "the frame drawn next shows the scroll");
    assert!(browser.end().success());
}

/// Without `--headless`: the same, in the real window. It needs a display
/// (and a GPU), so it runs only with `MACHOPO_WINDOW_TESTS` (`test.py
/// scenarios --window`).
#[test]
fn the_real_window_is_driven_the_same() {
    if std::env::var_os("MACHOPO_WINDOW_TESTS").is_none() {
        eprintln!("skipped without MACHOPO_WINDOW_TESTS");
        return;
    }
    let server = server(&[("/", Route::ok("text/html", machopo_page()))]);
    let mut browser = Browser::start_in_the_real_window();
    // The platform's window, not one without it.
    let version = browser.call(None, "Browser.getVersion", json!({})).unwrap();
    let product = version["product"].as_str().unwrap();
    assert!(product.starts_with("machopo/"), "{product}");
    browser
        .call(
            None,
            "Machopo.navigateFromAddressBar",
            json!({ "url": server.url("/") }),
        )
        .unwrap();
    browser
        .call(None, "Machopo.waitForLoad", json!({}))
        .unwrap();
    browser.click("#b");
    assert_eq!(browser.query("#out")["element"]["text"], "clicked");
    let (_, before) = browser.capture();
    browser.input(json!({ "type": "key", "key": "End" }));
    assert_ne!(browser.capture().1, before);
    browser
        .call(
            None,
            "Browser.setWindowBounds",
            json!({ "windowId": 1, "bounds": { "width": 400, "height": 300 } }),
        )
        .unwrap();
    assert_eq!(browser.window_size(), (400, 300));
    browser.call(None, "Browser.close", json!({})).unwrap();
    assert!(wait(&mut browser.child).success());
}

/// What `Machopo`'s WebDriver methods answer: `{value}`, or WebDriver's error
/// code (the error's `data`).
fn webdriver(
    browser: &mut Browser,
    session: &str,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    browser
        .call(Some(session), method, params)
        .map(|result| result["value"].clone())
        .map_err(|error| {
            error
                .data
                .unwrap_or_else(|| format!("no data: {}", error.message))
        })
}

pub(crate) fn script(
    browser: &mut Browser,
    session: &str,
    body: &str,
    args: Value,
) -> Result<Value, String> {
    let params = json!({ "script": body, "args": args });
    webdriver(browser, session, "Machopo.executeScript", params)
}

fn async_script(browser: &mut Browser, session: &str, body: &str) -> Result<Value, String> {
    let params = json!({ "script": body, "async": true });
    webdriver(browser, session, "Machopo.executeScript", params)
}

/// The element reference `found` is: its id.
fn element_id(found: &Value) -> String {
    found[protocol::webdriver::ELEMENT_KEY]
        .as_str()
        .unwrap_or_else(|| panic!("not an element reference: {found}"))
        .to_owned()
}

/// Execute Script's values: a function body, its value cloned as JSON
/// (references for elements and the window), promises waited for, errors
/// as WebDriver's.
#[test]
fn scripts_run_and_their_values_are_cloned() {
    let page = "<!DOCTYPE html><title>Script</title><p id=a class='no cheese'>a</p><p>b</p>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let mut run = |body: &str| script(&mut browser, &session, body, json!([]));
    assert_eq!(run("return 1 + 1"), Ok(json!(2)));
    assert_eq!(run("return 1; // a comment"), Ok(json!(1)));
    assert_eq!(run("return undefined"), Ok(Value::Null));
    assert_eq!(run(r"return 'a\u0000b'"), Ok(json!("a\u{0}b")));
    assert_eq!(run("return 0.5"), Ok(json!(0.5)));
    assert_eq!(
        run("return {a: [1, true, null], b: {c: 'd'}}"),
        Ok(json!({ "a": [1, true, null], "b": { "c": "d" } }))
    );
    // The same object twice side by side isn't a cycle; within itself is.
    assert_eq!(run("const a = [1]; return [a, a]"), Ok(json!([[1], [1]])));
    assert_eq!(
        run("const a = []; a.push(a); return a"),
        Err("javascript error".into())
    );
    assert_eq!(
        run("const o = {}; o.o = o; return [o]"),
        Err("javascript error".into())
    );
    assert_eq!(run("return {toJSON() { return ['x'] }}"), Ok(json!(["x"])));
    assert_eq!(
        run("return {toJSON() { throw new Error('no') }}"),
        Err("javascript error".into())
    );
    // Collections are arrays.
    let found = run("return document.querySelectorAll('p')").unwrap();
    assert_eq!(found.as_array().map(Vec::len), Some(2));
    element_id(&found[0]);
    let children = run("return document.body.children").unwrap();
    assert_eq!(children.as_array().map(Vec::len), Some(2));
    assert_eq!(
        run("return document.getElementById('a').classList"),
        Ok(json!(["no", "cheese"]))
    );
    assert_eq!(
        run("return (function () { return arguments })(1, 2)"),
        Ok(json!([1, 2]))
    );
    // A node that isn't an element is an object.
    assert_eq!(
        run("return document.getElementById('a').firstChild"),
        Ok(json!({}))
    );
    // Errors and promises.
    assert_eq!(run("throw new Error('no')"), Err("javascript error".into()));
    assert_eq!(run("return {"), Err("javascript error".into()));
    assert_eq!(run("return Promise.resolve('p')"), Ok(json!("p")));
    assert_eq!(
        run("return new Promise(r => setTimeout(() => r('later'), 20))"),
        Ok(json!("later"))
    );
    assert_eq!(run("return await Promise.resolve(3)"), Ok(json!(3)));
    assert_eq!(
        run("return Promise.reject(new Error('no'))"),
        Err("javascript error".into())
    );
    assert_eq!(run("return Symbol()"), Err("javascript error".into()));
    // The page's own globals don't get in the way.
    assert_eq!(
        run("JSON.stringify = null; Array.prototype.map = null; return [1]"),
        Ok(json!([1]))
    );
    // The window: its handle is the tab's target id.
    let targets = browser.call(None, "Target.getTargets", json!({})).unwrap();
    let target = targets["targetInfos"][0]["targetId"].clone();
    assert_eq!(
        script(&mut browser, &session, "return window", json!([])),
        Ok(json!({ protocol::webdriver::WINDOW_KEY: target }))
    );
    assert!(browser.end().success());
}

/// Execute Async Script: settled by the first call of the function it
/// gets last, whenever that is; its body only by throwing first.
#[test]
fn async_scripts_settle_when_called_back() {
    let mut browser = Browser::start();
    let session = browser.attach();
    let mut run = |body: &str| async_script(&mut browser, &session, body);
    assert_eq!(run("arguments[0](1)"), Ok(json!(1)));
    // Called later: the body's returning doesn't settle it.
    assert_eq!(
        run("const done = arguments[arguments.length - 1]; setTimeout(() => done('late'), 20)"),
        Ok(json!("late"))
    );
    assert_eq!(run("arguments[0](Promise.resolve('p'))"), Ok(json!("p")));
    assert_eq!(
        run("arguments[0]('first'); arguments[0]('second')"),
        Ok(json!("first"))
    );
    // The first call settles it, however long what it was given takes.
    assert_eq!(
        run(
            "arguments[0](new Promise(r => setTimeout(() => r('slow'), 50))); arguments[0]('fast')"
        ),
        Ok(json!("slow"))
    );
    assert_eq!(
        run("await null; throw new Error('no')"),
        Err("javascript error".into())
    );
    assert_eq!(
        run("arguments[0]('kept'); throw new Error('no')"),
        Ok(json!("kept"))
    );
    assert!(browser.end().success());
}

/// Arguments are deserialized: JSON as JS, references as the elements and
/// window they are; a reference that can't be is WebDriver's error.
#[test]
fn script_arguments_are_deserialized() {
    let page = "<!DOCTYPE html><p id=a>a</p>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let mut run = |body: &str, args: Value| script(&mut browser, &session, body, args);
    assert_eq!(
        run(
            "return [typeof arguments[0], arguments[0], Array.isArray(arguments[1])]",
            json!([{ "a": 1 }, [2]])
        ),
        Ok(json!(["object", { "a": 1 }, true]))
    );
    let element = run("return document.getElementById('a')", json!([])).unwrap();
    assert_eq!(
        run("return arguments[0].id", json!([element])),
        Ok(json!("a"))
    );
    let window = run("return window", json!([])).unwrap();
    assert_eq!(
        run("return arguments[0] === window", json!([window])),
        Ok(json!(true))
    );
    let key = protocol::webdriver::ELEMENT_KEY;
    assert_eq!(
        run("return 1", json!([{ key: "nope" }])),
        Err("no such element".into())
    );
    assert_eq!(
        run("return 1", json!([{ key: 42 }])),
        Err("invalid argument".into())
    );
    let other = json!({ protocol::webdriver::WINDOW_KEY: "nope" });
    assert_eq!(
        run("return 1", json!([other])),
        Err("no such window".into())
    );
    // Gone from the document: stale, as a result and as an argument.
    assert_eq!(
        run(
            "const p = arguments[0]; p.remove(); return p",
            json!([element])
        ),
        Err("stale element reference".into())
    );
    assert_eq!(
        run("return 1", json!([element])),
        Err("stale element reference".into())
    );
    assert!(browser.end().success());
}

/// Find Elements by CSS selector and tag name, from the document or an
/// element; reading an element's rect, tag name, attributes and
/// properties.
#[test]
fn elements_are_found_and_read() {
    let page = "<!DOCTYPE html><style>body, p { margin: 0 } \
        #box { margin: 10px; width: 50px; height: 20px }</style>\
        <div id=box><p class=x>one</p><span><p>two</p></span></div><p class=x>three</p>\
        <input id=check type=checkbox checked value=v><a id=link href='/next'>next</a>\
        <div style='height: 3000px'></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let mut find = |using: &str, value: &str, element: Option<&str>| {
        let mut params = json!({ "using": using, "value": value });
        if let Some(element) = element {
            params["element"] = json!(element);
        }
        webdriver(&mut browser, &session, "Machopo.findElements", params)
    };
    let count = |found: Result<Value, String>| found.unwrap().as_array().unwrap().len();
    assert_eq!(count(find("css selector", "p", None)), 3);
    let boxed = element_id(&find("css selector", "#box", None).unwrap()[0]);
    assert_eq!(count(find("css selector", "p", Some(&boxed))), 2);
    assert_eq!(count(find("tag name", "P", None)), 3);
    assert_eq!(count(find("tag name", "span", Some(&boxed))), 1);
    assert_eq!(find("css selector", "nothing", None), Ok(json!([])));
    assert_eq!(
        find("css selector", "p[", None),
        Err("invalid selector".into())
    );
    assert_eq!(
        find("xpath", "//p", None),
        Err("unsupported operation".into())
    );
    assert_eq!(find("by magic", "p", None), Err("invalid argument".into()));
    assert_eq!(
        find("css selector", "p", Some("nope")),
        Err("no such element".into())
    );
    // The same element is the same reference.
    let again = element_id(&find("css selector", "#box", None).unwrap()[0]);
    assert_eq!(again, boxed);
    let check = element_id(&find("css selector", "#check", None).unwrap()[0]);
    let link = element_id(&find("css selector", "#link", None).unwrap()[0]);

    let mut read = |method: &str, element: &str, name: Option<&str>| {
        let mut params = json!({ "element": element });
        if let Some(name) = name {
            params["name"] = json!(name);
        }
        webdriver(&mut browser, &session, method, params)
    };
    let rect = json!({ "x": 10, "y": 10, "width": 50, "height": 20 });
    assert_eq!(
        read("Machopo.getElementRect", &boxed, None),
        Ok(rect.clone())
    );
    assert_eq!(
        read("Machopo.getElementTagName", &boxed, None),
        Ok(json!("div"))
    );
    assert_eq!(
        read("Machopo.getElementAttribute", &boxed, Some("id")),
        Ok(json!("box"))
    );
    assert_eq!(
        read("Machopo.getElementAttribute", &boxed, Some("title")),
        Ok(Value::Null)
    );
    // A boolean attribute is "true", whatever its value.
    assert_eq!(
        read("Machopo.getElementAttribute", &check, Some("checked")),
        Ok(json!("true"))
    );
    assert_eq!(
        read("Machopo.getElementAttribute", &check, Some("disabled")),
        Ok(Value::Null)
    );
    // A property is as script reads it: an attribute's IDL one, an
    // element.
    assert_eq!(
        read("Machopo.getElementProperty", &link, Some("id")),
        Ok(json!("link"))
    );
    assert_eq!(
        read("Machopo.getElementProperty", &check, Some("nothing")),
        Ok(Value::Null)
    );
    // The qualified name, its prefix and case as they are.
    let prefixed = script(
        &mut browser,
        &session,
        "const e = document.createElementNS('http://www.w3.org/2000/svg', 'SvG:linearGradient'); \
         document.body.appendChild(e); return e",
        json!([]),
    )
    .unwrap();
    let mut read = |method: &str, element: &str, name: Option<&str>| {
        let mut params = json!({ "element": element });
        if let Some(name) = name {
            params["name"] = json!(name);
        }
        webdriver(&mut browser, &session, method, params)
    };
    assert_eq!(
        read("Machopo.getElementTagName", &element_id(&prefixed), None),
        Ok(json!("SvG:linearGradient"))
    );
    let first = read(
        "Machopo.getElementProperty",
        &boxed,
        Some("firstElementChild"),
    )
    .unwrap();
    assert_eq!(
        read("Machopo.getElementTagName", &element_id(&first), None),
        Ok(json!("p"))
    );
    // Scrolled, the rect is the document's still.
    script(&mut browser, &session, "window.scrollTo(0, 100)", json!([])).unwrap();
    let params = json!({ "element": boxed });
    assert_eq!(
        webdriver(
            &mut browser,
            &session,
            "Machopo.getElementRect",
            params.clone()
        ),
        Ok(rect)
    );
    // After a navigation: stale, not unknown.
    browser.navigate(&session, &server.url("/"));
    assert_eq!(
        webdriver(&mut browser, &session, "Machopo.getElementTagName", params),
        Err("stale element reference".into())
    );
    assert!(browser.end().success());
}

/// Without a session, the WebDriver methods are a page's that aren't
/// there.
#[test]
fn webdriver_methods_need_a_session() {
    let mut browser = Browser::start();
    let error = browser
        .call(
            None,
            "Machopo.executeScript",
            json!({ "script": "return 1" }),
        )
        .unwrap_err();
    assert_eq!(error.code, METHOD_NOT_FOUND);
    assert!(browser.end().success());
}

/// A reference is this renderer's, of the top-level document, or isn't
/// one machopo knows: another prefix (another tab's renderer, or this tab's
/// before a crash), or a document never seen, is "no such element", even
/// if its node id is an element's here.
#[test]
fn references_are_only_this_renderers() {
    let page = "<!DOCTYPE html><p id=a>a</p>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let found = script(
        &mut browser,
        &session,
        "return document.getElementById('a')",
        json!([]),
    )
    .unwrap();
    let id = element_id(&found);
    let parts: Vec<&str> = id.split('.').collect();
    assert_eq!(parts.len(), 3, "{id}");
    let prefix = u64::from_str_radix(parts[0], 16).unwrap();
    let document: u64 = parts[1].parse().unwrap();
    let other_prefix = format!("{:016x}.{}.{}", prefix ^ 1, parts[1], parts[2]);
    let other_document = format!("{}.{}.{}", parts[0], document + 1000, parts[2]);
    let key = protocol::webdriver::ELEMENT_KEY;
    for other in [other_prefix, other_document] {
        assert_eq!(
            script(&mut browser, &session, "return 1", json!([{ key: other }])),
            Err("no such element".into()),
            "{other}"
        );
    }
    assert_eq!(
        script(
            &mut browser,
            &session,
            "return arguments[0].id",
            json!([found])
        ),
        Ok(json!("a"))
    );
    assert!(browser.end().success());
}

/// A script that hasn't settled when its document goes away, or its
/// renderer, is answered with an error, not left waiting.
#[test]
fn a_script_left_waiting_is_answered() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    for gone in [server.url("/"), "about:crash".to_owned()] {
        // The script, unanswered, then what takes its document away.
        browser.next_id += 1;
        let pending = browser.next_id;
        let command = json!({
            "id": pending,
            "sessionId": session,
            "method": "Machopo.executeScript",
            "params": { "script": "return new Promise(() => {})" },
        });
        browser.write(&command.to_string());
        browser.next_id += 1;
        let navigate = browser.next_id;
        let command = json!({
            "id": navigate,
            "sessionId": session,
            "method": "Page.navigate",
            "params": { "url": gone },
        });
        browser.write(&command.to_string());
        let mut answers = HashMap::new();
        while answers.len() < 2 {
            let Ok(message) = browser.messages.recv_timeout(TIMEOUT) else {
                panic!("only {answers:?} for {pending} and {navigate} ({gone})");
            };
            match message {
                Message::Response(response) => {
                    answers.insert(response.id, response.result);
                }
                Message::Event(event) => browser.events.push(event),
                // The renderer's `Page` events (`frameNavigated`...).
                Message::Raw { object, .. } if renderer_event(&object) => {}
                Message::Raw { object, .. } => panic!("unasked {object:?}"),
            }
        }
        assert!(answers[&navigate].is_ok(), "{answers:?}");
        let error = answers[&pending].clone().unwrap_err();
        assert_eq!(error.code, protocol::automation::SERVER_ERROR, "{gone}");
        if gone.starts_with("http") {
            assert_eq!(error.data.as_deref(), Some("javascript error"));
        }
        browser.events.clear();
    }
    assert!(browser.end().success());
}

/// What `id` names in WebDriver's JSON: a reference of `key`.
fn reference(key: &str, id: &str) -> Value {
    json!({ key: id })
}

/// `Machopo.waitForLoad` of a page session is the session's tab's, shown or
/// not (machopodriver's window may be behind another tab): its tab has loaded,
/// though the tab shown is still loading. (A tab behind draws nothing: one
/// still loading there is loaded once it is shown.)
#[test]
fn waiting_for_a_load_is_the_sessions_tabs() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>page")),
        (
            "/slow",
            Route::ok("text/html", "<!DOCTYPE html>slow").with_delay(Duration::from_millis(500)),
        ),
    ]);
    let mut browser = Browser::start();
    let first = browser.attach();
    browser.navigate(&first, &server.url("/"));
    // Another tab in front, loading.
    let created = browser
        .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
        .unwrap();
    let second = browser.attach_to(created["targetId"].as_str().unwrap());
    // Answered once its slow response comes: not waited for.
    browser.send(
        Some(&second),
        "Page.navigate",
        json!({ "url": server.url("/slow") }),
    );
    std::thread::sleep(Duration::from_millis(50));
    let started = std::time::Instant::now();
    browser
        .call(Some(&first), "Machopo.waitForLoad", json!({}))
        .unwrap();
    assert!(
        started.elapsed() < Duration::from_millis(400),
        "waited for the tab in front"
    );
}

/// `Target.createTarget` opens a tab after the others, showing
/// `about:blank` as its initial entry, behind the tab shown or in front;
/// `Target.activateTarget` shows one, where it draws (animation frames
/// run); `Target.closeTarget` closes one, and the last ends machopo.
#[test]
fn tabs_are_opened_shown_and_closed() {
    let page = |id: &str| format!("<!DOCTYPE html><p id={id}>{id}</p>");
    let server = server(&[
        ("/a", Route::ok("text/html", page("a"))),
        ("/b", Route::ok("text/html", page("b"))),
    ]);
    let mut browser = Browser::start();
    let first = browser.targets()[0].clone();
    let session = browser.attach_to(&first);
    browser.navigate(&session, &server.url("/a"));
    let created = browser
        .call(
            None,
            "Target.createTarget",
            json!({ "url": "about:blank", "background": true }),
        )
        .unwrap();
    let second = created["targetId"].as_str().unwrap().to_owned();
    assert_eq!(browser.targets(), [first.clone(), second.clone()]);
    // Behind: the first is still the tab shown.
    assert!(browser.query("#a")["element"].is_object());
    let behind = browser.attach_to(&second);
    assert_eq!(
        script(&mut browser, &behind, "return location.href", json!([])),
        Ok(json!("about:blank"))
    );
    // Told where the window is and how big, though it isn't shown.
    assert_eq!(
        script(
            &mut browser,
            &behind,
            "return [outerWidth, outerHeight]",
            json!([])
        ),
        Ok(json!([800, 600]))
    );
    let navigated = browser
        .call(
            Some(&behind),
            "Page.navigate",
            json!({ "url": server.url("/b") }),
        )
        .unwrap();
    // The initial entry was taken the place of.
    let history = browser
        .call(Some(&behind), "Page.getNavigationHistory", json!({}))
        .unwrap();
    assert_eq!(history["entries"].as_array().map(Vec::len), Some(1));
    assert!(browser.query("#b")["element"].is_null());
    browser
        .call(None, "Target.activateTarget", json!({ "targetId": second }))
        .unwrap();
    // A tab not shown draws nothing: its `load` is told with the frame
    // that shows it.
    browser.load(navigated["loaderId"].as_str().unwrap());
    assert!(browser.query("#b")["element"].is_object());
    assert_eq!(
        script(
            &mut browser,
            &behind,
            "return new Promise(r => requestAnimationFrame(() => r('drawn')))",
            json!([])
        ),
        Ok(json!("drawn"))
    );
    // In front, after the others. Its `about:blank`, loaded, is the
    // initial entry still: the first navigation takes its place.
    let created = browser
        .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
        .unwrap();
    let third = created["targetId"].as_str().unwrap().to_owned();
    browser
        .call(None, "Machopo.waitForLoad", json!({}))
        .unwrap();
    let front = browser.attach_to(&third);
    browser.navigate(&front, &server.url("/a"));
    let history = browser
        .call(Some(&front), "Page.getNavigationHistory", json!({}))
        .unwrap();
    assert_eq!(history["entries"].as_array().map(Vec::len), Some(1));
    assert!(browser.query("#a")["element"].is_object());
    assert_eq!(
        browser.targets(),
        [first.clone(), second.clone(), third.clone()]
    );

    for (method, params) in [
        ("Target.activateTarget", json!({ "targetId": "nope" })),
        ("Target.closeTarget", json!({ "targetId": "nope" })),
        ("Target.createTarget", json!({ "url": "not a url" })),
    ] {
        let error = browser.call(None, method, params).unwrap_err();
        assert_eq!(error.code, SERVER_ERROR, "{method}");
    }
    // machopo has one window.
    let error = browser
        .call(
            None,
            "Target.createTarget",
            json!({ "url": "about:blank", "newWindow": true }),
        )
        .unwrap_err();
    assert_eq!(error.code, INVALID_PARAMS);

    let close = |browser: &mut Browser, target: &str| {
        browser.call(None, "Target.closeTarget", json!({ "targetId": target }))
    };
    assert_eq!(close(&mut browser, &third), Ok(json!({ "success": true })));
    assert_eq!(browser.targets(), [first.clone(), second.clone()]);
    close(&mut browser, &first).unwrap();
    assert_eq!(browser.targets(), std::slice::from_ref(&second));
    // The last: the window closes, and machopo ends.
    close(&mut browser, &second).unwrap();
    assert!(wait(&mut browser.child).success());
}

/// Execute Script in `frame` (the top-level one if none).
fn script_in(
    browser: &mut Browser,
    session: &str,
    frame: Option<&str>,
    body: &str,
    args: Value,
) -> Result<Value, String> {
    let mut params = json!({ "script": body, "args": args });
    if let Some(frame) = frame {
        params["frame"] = json!(frame);
    }
    webdriver(browser, session, "Machopo.executeScript", params)
}

/// `Machopo.switchToFrame` in `frame` (the top-level one if none).
fn switch_in(
    browser: &mut Browser,
    session: &str,
    frame: Option<&str>,
    mut params: Value,
) -> Result<Value, String> {
    if let Some(frame) = frame {
        params["frame"] = json!(frame);
    }
    webdriver(browser, session, "Machopo.switchToFrame", params)
}

/// A frame its parent's script removes: the next command in it, which comes
/// before the renderer's loop has turned again, finds it gone (`no such
/// window`), as Chrome detaches a frame as its iframe goes; not a
/// `javascript error` of a frame the renderer still held.
#[test]
fn a_removed_frame_is_gone_for_the_next_command() {
    let server = server(&[
        (
            "/top",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><iframe id=one src=/one></iframe>",
            ),
        ),
        ("/one", Route::ok("text/html", "<!DOCTYPE html><p>one</p>")),
    ]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/top"));
    let b = &mut browser;
    let one = switch_in(b, &session, None, json!({ "index": 0 }))
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned();
    // Removed; its answer sent, the renderer busy in a task the script
    // queued (a message's), while the next command comes.
    let script = "one.remove(); onmessage = () => { const until = Date.now() + 300; while (Date.now() < until); }; \
                  postMessage('', '*'); return 0";
    script_in(b, &session, None, script, json!([])).unwrap();
    assert_eq!(
        script_in(b, &session, Some(&one), "return 1", json!([])),
        Err("no such window".into())
    );
}

/// A frame made and removed in one script, gone before the renderer takes
/// the engine's note of its making: nothing of it is kept, and the page
/// goes on (debug builds assert that a frame still there was shown as it
/// was made).
#[test]
fn a_frame_made_and_removed_in_one_script_leaves_nothing() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html><p>top</p>"))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let b = &mut browser;
    let script = "const f = document.body.appendChild(document.createElement('iframe')); \
                  f.remove(); return frames.length";
    assert_eq!(
        script_in(b, &session, None, script, json!([])),
        Ok(json!(0))
    );
    assert_eq!(
        script_in(
            b,
            &session,
            None,
            "return document.body.textContent",
            json!([])
        ),
        Ok(json!("top"))
    );
}

/// `Machopo.switchToFrame` gives a child frame's id, by its index in its
/// parent's `window` or by its iframe; the `Machopo` WebDriver methods work in
/// the frame they are given. A frame's id stays its own as it navigates,
/// and is gone with its iframe. Element references are their frame's: in
/// another, "no such element"; once the frame shows another document,
/// stale.
#[test]
fn frames_are_switched_to_and_kept_through_their_navigations() {
    let server = server(&[
        (
            "/top",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><iframe id=one src=/one></iframe><p id=top>top</p>\
                 <iframe id=three src=/three></iframe>",
            ),
        ),
        (
            "/one",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><p id=in>one</p><iframe src=/two></iframe>",
            ),
        ),
        (
            "/two",
            Route::ok("text/html", "<!DOCTYPE html><p id=in>two</p>"),
        ),
        (
            "/three",
            Route::ok("text/html", "<!DOCTYPE html><p id=in>three</p>"),
        ),
    ]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/top"));
    let b = &mut browser;
    let id = |found: Result<Value, String>| found.unwrap().as_str().unwrap().to_owned();
    let one = id(switch_in(b, &session, None, json!({ "index": 0 })));
    let three = id(switch_in(b, &session, None, json!({ "index": 1 })));
    assert_ne!(three, one);
    let two = id(switch_in(b, &session, Some(&one), json!({ "index": 0 })));
    // With neither, the frame itself; the top-level one is null.
    assert_eq!(
        switch_in(b, &session, Some(&one), json!({})),
        Ok(json!(one))
    );
    assert_eq!(switch_in(b, &session, None, json!({})), Ok(Value::Null));
    let no_frame = Err("no such frame".into());
    assert_eq!(
        switch_in(b, &session, None, json!({ "index": 2 })),
        no_frame
    );
    assert_eq!(
        switch_in(b, &session, Some(&two), json!({ "index": 0 })),
        no_frame
    );
    assert_eq!(
        switch_in(b, &session, None, json!({ "index": 0, "element": "x" })),
        Err("invalid argument".into())
    );

    let path = "return location.pathname";
    assert_eq!(
        script_in(b, &session, None, path, json!([])),
        Ok(json!("/top"))
    );
    assert_eq!(
        script_in(b, &session, Some(&one), path, json!([])),
        Ok(json!("/one"))
    );
    assert_eq!(
        script_in(b, &session, Some(&two), path, json!([])),
        Ok(json!("/two"))
    );
    // One that settles later, in its frame.
    let later = "return new Promise(r => setTimeout(() => r(location.pathname), 20))";
    assert_eq!(
        script_in(b, &session, Some(&two), later, json!([])),
        Ok(json!("/two"))
    );
    // By its iframe: the same frame as by its index; not an iframe, none.
    let iframe = script_in(
        b,
        &session,
        None,
        "return document.getElementById('one')",
        json!([]),
    );
    let iframe = element_id(&iframe.unwrap());
    let top_p = script_in(
        b,
        &session,
        None,
        "return document.getElementById('top')",
        json!([]),
    );
    let top_p = top_p.unwrap();
    assert_eq!(
        switch_in(b, &session, None, json!({ "element": iframe })),
        Ok(json!(one))
    );
    assert_eq!(
        switch_in(b, &session, None, json!({ "element": element_id(&top_p) })),
        no_frame
    );

    // Windows: the top-level one by its handle, a child frame's by its id,
    // either way.
    let frame_key = protocol::webdriver::FRAME_KEY;
    assert_eq!(
        script_in(b, &session, None, "return frames[0]", json!([])),
        Ok(reference(frame_key, &one))
    );
    assert_eq!(
        script_in(b, &session, Some(&one), "return window", json!([])),
        Ok(reference(frame_key, &one))
    );
    let window = script_in(b, &session, None, "return window", json!([])).unwrap();
    assert_eq!(
        script_in(b, &session, Some(&two), "return top", json!([])),
        Ok(window)
    );
    let two_ref = json!([reference(frame_key, &two)]);
    assert_eq!(
        script_in(
            b,
            &session,
            None,
            "return arguments[0].location.pathname",
            two_ref
        ),
        Ok(json!("/two"))
    );
    let nope = json!([reference(frame_key, "nope")]);
    assert_eq!(script_in(b, &session, None, "return 1", nope), no_frame);
    // The top-level frame's own token is no child frame's id.
    let top_id = element_id(&top_p);
    let (prefix, rest) = top_id.split_once('.').unwrap();
    let (document, _) = rest.split_once('.').unwrap();
    let top_frame = format!("{prefix}.{document}");
    let top_ref = json!([reference(frame_key, &top_frame)]);
    assert_eq!(script_in(b, &session, None, "return 1", top_ref), no_frame);
    assert_eq!(
        switch_in(b, &session, Some(&top_frame), json!({})),
        Err("no such window".into())
    );

    // Elements: their frame's.
    let found = script_in(
        b,
        &session,
        Some(&one),
        "return document.getElementById('in')",
        json!([]),
    );
    let inner = found.unwrap();
    assert_eq!(
        script_in(
            b,
            &session,
            Some(&one),
            "return arguments[0].textContent",
            json!([inner])
        ),
        Ok(json!("one"))
    );
    let no_element = Err("no such element".into());
    assert_eq!(
        script_in(b, &session, None, "return 1", json!([inner])),
        no_element
    );
    assert_eq!(
        script_in(b, &session, Some(&two), "return 1", json!([inner])),
        no_element
    );
    assert_eq!(
        script_in(b, &session, Some(&one), "return 1", json!([top_p])),
        no_element
    );
    let params = json!({ "frame": one, "using": "css selector", "value": "#in" });
    let found = webdriver(b, &session, "Machopo.findElements", params).unwrap();
    assert_eq!(found, json!([inner]));
    let params = json!({ "frame": one, "element": element_id(&inner) });
    assert_eq!(
        webdriver(b, &session, "Machopo.getElementTagName", params),
        Ok(json!("p"))
    );

    // The frame navigates: the same id, another document.
    let navigated = "const f = document.getElementById('one'); \
        f.onload = () => arguments[0]('loaded'); f.src = '/three'";
    let params = json!({ "script": navigated, "async": true });
    assert_eq!(
        webdriver(b, &session, "Machopo.executeScript", params),
        Ok(json!("loaded"))
    );
    assert_eq!(
        script_in(b, &session, Some(&one), path, json!([])),
        Ok(json!("/three"))
    );
    assert_eq!(
        script_in(b, &session, Some(&one), "return 1", json!([inner])),
        Err("stale element reference".into())
    );
    // Its child frame went with its document.
    let gone = Err("no such window".into());
    assert_eq!(script_in(b, &session, Some(&two), path, json!([])), gone);

    // Its iframe removed: the frame is gone, and its elements are no
    // frame's here.
    let remove = "document.getElementById('one').remove()";
    assert_eq!(
        script_in(b, &session, None, remove, json!([])),
        Ok(Value::Null)
    );
    assert_eq!(script_in(b, &session, Some(&one), path, json!([])), gone);
    assert_eq!(switch_in(b, &session, Some(&one), json!({})), gone);
    assert_eq!(
        script_in(b, &session, None, "return 1", json!([inner])),
        no_element
    );
    // The top-level document navigates: its frames go.
    assert_eq!(
        script_in(b, &session, Some(&three), path, json!([])),
        Ok(json!("/three"))
    );
    browser.navigate(&session, &server.url("/top"));
    let b = &mut browser;
    assert_eq!(script_in(b, &session, Some(&three), path, json!([])), gone);
    assert!(browser.end().success());
}

/// `outerWidth` and `outerHeight` are the window's size, in every frame,
/// as it is resized, and in the renderer a crash brought anew.
#[test]
fn the_outer_size_is_the_windows() {
    let page = "<!DOCTYPE html><iframe srcdoc='child'></iframe>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let outer = "return [outerWidth, outerHeight, frames[0].outerWidth, innerHeight < outerHeight]";
    assert_eq!(
        script(&mut browser, &session, outer, json!([])),
        Ok(json!([800, 600, 800, true]))
    );
    browser
        .call(
            None,
            "Browser.setWindowBounds",
            json!({ "windowId": 1, "bounds": { "width": 640, "height": 480 } }),
        )
        .unwrap();
    assert_eq!(
        script(&mut browser, &session, outer, json!([])),
        Ok(json!([640, 480, 640, true]))
    );
    browser
        .call(
            Some(&session),
            "Page.navigate",
            json!({ "url": "about:crash" }),
        )
        .unwrap();
    browser.navigate(&session, &server.url("/"));
    assert_eq!(
        script(&mut browser, &session, outer, json!([])),
        Ok(json!([640, 480, 640, true]))
    );
    assert!(browser.end().success());
}

/// A PNG in base64: its size and its pixels, RGBA.
pub(crate) fn png(data: &str) -> ((u32, u32), Vec<u8>) {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .expect("base64");
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().expect("a PNG");
    let mut pixels = vec![0; reader.output_buffer_size().expect("its size")];
    let info = reader.next_frame(&mut pixels).expect("its pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba);
    pixels.truncate(info.buffer_size());
    ((info.width, info.height), pixels)
}

/// The pixel at `x`, `y` of an RGBA image `width` wide, but its alpha.
pub(crate) fn pixel(pixels: &[u8], width: u32, x: u32, y: u32) -> [u8; 3] {
    let i = ((y * width + x) * 4) as usize;
    [pixels[i], pixels[i + 1], pixels[i + 2]]
}

/// `Page.captureScreenshot` is the page's view as a PNG, as the next frame
/// shows it (what script just did included), physical px; only PNG, and
/// only of the tab shown. `Machopo.captureElementScreenshot` is the part of an
/// element in view, after scrolling it there; none of it is "unable to
/// capture screen".
#[test]
fn pages_are_captured_as_png() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
        #red { width: 100px; height: 50px; background: red } \
        #far { margin: 2000px 0 0 40px; width: 30px; height: 20px; background: blue }</style>\
        <div id=red></div><div id=far></div><div id=none hidden></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let size = script(
        &mut browser,
        &session,
        "return [innerWidth, innerHeight]",
        json!([]),
    );
    let size = size.unwrap();
    let viewport = (
        size[0].as_u64().unwrap() as u32,
        size[1].as_u64().unwrap() as u32,
    );
    let capture = |browser: &mut Browser, params: Value| {
        browser.call(Some(&session), "Page.captureScreenshot", params)
    };
    let shot = capture(&mut browser, json!({})).unwrap();
    let ((width, height), pixels) = png(shot["data"].as_str().unwrap());
    assert_eq!((width, height), viewport);
    assert_eq!(pixel(&pixels, width, 50, 25), [255, 0, 0]);
    assert_eq!(pixel(&pixels, width, 150, 25), [255, 255, 255]);
    // What script just did shows.
    let green = "document.getElementById('red').style.background = 'lime'";
    script(&mut browser, &session, green, json!([])).unwrap();
    let shot = capture(&mut browser, json!({ "format": "png" })).unwrap();
    let ((width, _), pixels) = png(shot["data"].as_str().unwrap());
    assert_eq!(pixel(&pixels, width, 50, 25), [0, 255, 0]);
    let error = capture(&mut browser, json!({ "format": "jpeg" })).unwrap_err();
    assert_eq!(error.code, INVALID_PARAMS);
    let error = capture(&mut browser, json!({ "clip": {} })).unwrap_err();
    assert_eq!(error.code, INVALID_PARAMS);

    // An element: scrolled into view first.
    let far = script(
        &mut browser,
        &session,
        "return document.getElementById('far')",
        json!([]),
    );
    let far = element_id(&far.unwrap());
    let params = json!({ "element": far });
    let shot = webdriver(
        &mut browser,
        &session,
        "Machopo.captureElementScreenshot",
        params,
    )
    .unwrap();
    let ((width, height), pixels) = png(shot.as_str().unwrap());
    assert_eq!((width, height), (30, 20));
    assert_eq!(pixel(&pixels, width, 15, 10), [0, 0, 255]);
    assert!(
        script(&mut browser, &session, "return scrollY", json!([]))
            .unwrap()
            .as_f64()
            > Some(1000.0)
    );
    let none = script(
        &mut browser,
        &session,
        "return document.getElementById('none')",
        json!([]),
    );
    let params = json!({ "element": element_id(&none.unwrap()) });
    assert_eq!(
        webdriver(
            &mut browser,
            &session,
            "Machopo.captureElementScreenshot",
            params
        ),
        Err("unable to capture screen".into())
    );

    // Not the tab shown: nothing to capture.
    let created = browser
        .call(
            None,
            "Target.createTarget",
            json!({ "url": "about:blank", "background": true }),
        )
        .unwrap();
    let behind = browser.attach_to(created["targetId"].as_str().unwrap());
    let error = browser
        .call(Some(&behind), "Page.captureScreenshot", json!({}))
        .unwrap_err();
    assert_eq!(error.code, SERVER_ERROR);
    assert!(browser.end().success());
}

/// `--scale 2`: the page has two device pixels per CSS px, and its
/// screenshots are twice its CSS size (as wptrunner's reftests at 2x, a
/// subsuite with Chromium's `--force-device-scale-factor`, see them).
#[test]
fn the_scale_is_the_device_scale_factor() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
        div { width: 100px; height: 50px; background: red }</style><div></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start_with(&["--scale", "2"]);
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let seen = script(
        &mut browser,
        &session,
        "return [innerWidth, innerHeight, devicePixelRatio]",
        json!([]),
    )
    .unwrap();
    assert_eq!(seen[2], json!(2));
    let viewport = (
        seen[0].as_u64().unwrap() as u32,
        seen[1].as_u64().unwrap() as u32,
    );
    let shot = browser
        .call(Some(&session), "Page.captureScreenshot", json!({}))
        .unwrap();
    let ((width, height), pixels) = png(shot["data"].as_str().unwrap());
    assert_eq!((width, height), (viewport.0 * 2, viewport.1 * 2));
    // 100 CSS px wide: 200 device pixels.
    assert_eq!(pixel(&pixels, width, 199, 99), [255, 0, 0]);
    assert_eq!(pixel(&pixels, width, 201, 50), [255, 255, 255]);
    assert!(browser.end().success());
}

/// `Input.dispatchMouseEvent` goes straight to the tab's renderer, in its
/// CSS px, as the page's events; answered once handled (a move, which the
/// renderer dispatches at its next frame, once dispatched). Only the
/// moves, presses, releases and wheels machopo has.
#[test]
fn mouse_events_go_straight_to_the_page() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } \
        #b { width: 100px; height: 40px; margin-top: 30px }</style>\
        <div id=b></div><script>window.log = []; \
        for (const type of ['mousemove', 'mousedown', 'mouseup', 'click']) \
        document.addEventListener(type, e => \
        log.push([type, e.target.id, e.clientX, e.clientY, e.button, e.buttons, e.detail, e.shiftKey]));</script>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let mut mouse =
        |params: Value| browser.call(Some(&session), "Input.dispatchMouseEvent", params);
    mouse(json!({ "type": "mouseMoved", "x": 10, "y": 40 })).unwrap();
    assert_eq!(
        script(&mut browser, &session, "return log.length", json!([])),
        Ok(json!(1))
    );
    let mut mouse =
        |params: Value| browser.call(Some(&session), "Input.dispatchMouseEvent", params);
    let press = json!({ "x": 10, "y": 40, "button": "left", "clickCount": 1, "modifiers": 8 });
    let mut pressed = press.clone();
    pressed["type"] = json!("mousePressed");
    pressed["buttons"] = json!(1);
    mouse(pressed).unwrap();
    let mut released = press;
    released["type"] = json!("mouseReleased");
    mouse(released).unwrap();
    for params in [
        json!({ "type": "mouseDragged", "x": 1, "y": 1 }),
        json!({ "type": "mouseMoved", "x": 1, "y": 1, "button": "left" }),
        json!({ "type": "mousePressed", "x": 1, "y": 1 }),
        json!({ "type": "mousePressed", "x": 1, "y": 1, "button": "back" }),
        json!({ "type": "mousePressed", "x": 1, "y": 1, "button": "left", "force": 1 }),
    ] {
        assert_eq!(
            mouse(params.clone()).unwrap_err().code,
            INVALID_PARAMS,
            "{params}"
        );
    }
    assert_eq!(
        script(&mut browser, &session, "return log", json!([])),
        Ok(json!([
            ["mousemove", "b", 10, 40, 0, 0, 0, false],
            ["mousedown", "b", 10, 40, 0, 1, 1, true],
            ["mouseup", "b", 10, 40, 0, 0, 1, true],
            ["click", "b", 10, 40, 0, 0, 1, true],
        ]))
    );
    assert!(browser.end().success());
}

/// `Input.dispatchMouseEvent`'s `mouseWheel`: a wheel at the point, by
/// CSS px, with the keys held, as the user's: its `wheel` event, then the
/// scroll if nobody canceled it.
#[test]
fn wheels_scroll_the_page() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } #tall { height: 3000px }</style>\
        <div id=tall></div><script>window.log = []; \
        addEventListener('wheel', e => { log.push([e.target.id, e.clientX, e.clientY, \
        e.deltaX, e.deltaY, e.deltaMode, e.shiftKey]); if (log.length > 1) e.preventDefault(); }, \
        { passive: false });</script>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let wheel = json!({ "type": "mouseWheel", "x": 10, "y": 20, "deltaY": 100, "modifiers": 8 });
    browser
        .call(Some(&session), "Input.dispatchMouseEvent", wheel.clone())
        .unwrap();
    // The second, canceled, doesn't scroll.
    browser
        .call(Some(&session), "Input.dispatchMouseEvent", wheel)
        .unwrap();
    let got = "return [log, Math.round(scrollY)]";
    assert_eq!(
        script(&mut browser, &session, got, json!([])),
        Ok(json!([
            [
                ["tall", 10, 20, 0, 100, 0, true],
                ["tall", 10, 20, 0, 100, 0, true],
            ],
            100
        ]))
    );
    assert!(browser.end().success());
}

/// `Input.dispatchKeyEvent` goes straight to the tab's renderer, past the
/// browser's shortcuts, as CDP's: `keyDown` with what it types (a
/// `keypress`), `rawKeyDown` typing nothing, `keyUp`; with the DOM's
/// `key`, `code`, `location`, the legacy `keyCode`, the repeat and the
/// modifiers. Answered once handled. No `char`.
#[test]
fn key_events_go_straight_to_the_page() {
    let page = "<!DOCTYPE html><script>window.log = []; \
        for (const type of ['keydown', 'keypress', 'keyup']) \
        addEventListener(type, e => log.push([type, e.key, e.code, e.keyCode, e.location, \
        e.repeat, e.shiftKey, e.ctrlKey]));</script>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let mut key = |params: Value| browser.call(Some(&session), "Input.dispatchKeyEvent", params);
    key(
        json!({ "type": "rawKeyDown", "key": "Shift", "code": "ShiftLeft",
                "windowsVirtualKeyCode": 16, "location": 1, "modifiers": 8 }),
    )
    .unwrap();
    key(
        json!({ "type": "keyDown", "key": "A", "code": "KeyA", "text": "A",
                "windowsVirtualKeyCode": 65, "modifiers": 8 }),
    )
    .unwrap();
    key(
        json!({ "type": "keyDown", "key": "A", "code": "KeyA", "text": "A",
                "windowsVirtualKeyCode": 65, "modifiers": 8, "autoRepeat": true }),
    )
    .unwrap();
    key(
        json!({ "type": "keyUp", "key": "A", "code": "KeyA", "text": "A",
                "windowsVirtualKeyCode": 65, "modifiers": 8 }),
    )
    .unwrap();
    key(
        json!({ "type": "keyUp", "key": "Shift", "code": "ShiftLeft",
                "windowsVirtualKeyCode": 16, "location": 1 }),
    )
    .unwrap();
    // A rawKeyDown types nothing, whatever its text.
    key(json!({ "type": "rawKeyDown", "key": "b", "code": "KeyB", "text": "b" })).unwrap();
    // The browser's new-tab shortcut: the page's.
    key(
        json!({ "type": "keyDown", "key": "t", "code": "KeyT", "text": "t",
                "windowsVirtualKeyCode": 84, "modifiers": 2 }),
    )
    .unwrap();
    key(
        json!({ "type": "keyDown", "key": "t", "code": "KeyT", "text": "t",
                "windowsVirtualKeyCode": 84, "modifiers": 4 }),
    )
    .unwrap();
    for params in [
        json!({ "type": "char", "text": "a" }),
        json!({ "type": "keyDown", "key": "a", "location": 4 }),
        json!({ "type": "keyDown", "key": "a", "isKeypad": true }),
    ] {
        assert_eq!(
            key(params.clone()).unwrap_err().code,
            INVALID_PARAMS,
            "{params}"
        );
    }
    assert_eq!(
        script(&mut browser, &session, "return log", json!([])),
        Ok(json!([
            ["keydown", "Shift", "ShiftLeft", 16, 1, false, true, false],
            ["keydown", "A", "KeyA", 65, 0, false, true, false],
            ["keypress", "A", "KeyA", 65, 0, false, true, false],
            ["keydown", "A", "KeyA", 65, 0, true, true, false],
            ["keypress", "A", "KeyA", 65, 0, true, true, false],
            ["keyup", "A", "KeyA", 65, 0, false, true, false],
            ["keyup", "Shift", "ShiftLeft", 16, 1, false, false, false],
            ["keydown", "b", "KeyB", 0, 0, false, false, false],
            ["keydown", "t", "KeyT", 84, 0, false, false, true],
            ["keydown", "t", "KeyT", 84, 0, false, false, false],
        ]))
    );
    let targets = browser.call(None, "Target.getTargets", json!({})).unwrap();
    assert_eq!(targets["targetInfos"].as_array().map(Vec::len), Some(1));
    assert!(browser.end().success());
}

/// Perform Actions' points: `Machopo.getFrameViewport`, a frame's viewport in
/// the top-level one; `Machopo.getElementClickPoint` with `inViewCenterOnly`,
/// the element's in-view center point in its frame's viewport, nothing
/// scrolled or checked (outside the viewport if the element is); "move
/// target out of bounds" with no box.
#[test]
fn action_points_are_in_the_frame() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } div { height: 40px } \
        #a { width: 100px } #over { margin-top: -40px } \
        iframe { border: 0; margin-left: 20px; width: 200px; height: 100px; display: block } \
        #far { margin-top: 2000px; width: 60px }</style>\
        <div id=a></div><div id=under></div><div id=over></div>\
        <iframe srcdoc='<body style=\"margin: 0\"><div id=x style=\"margin: 10px 0 0 -10px; width: 50px; height: 20px\"></div>'></iframe>\
        <div id=far></div><div id=none hidden></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let b = &mut browser;
    let find = |b: &mut Browser, frame: Option<&str>, id: &str| {
        let get = format!("return document.getElementById('{id}')");
        element_id(&script_in(b, &session, frame, &get, json!([])).unwrap())
    };
    let point = |b: &mut Browser, frame: Option<&str>, element: &str| {
        let mut params = json!({ "element": element, "inViewCenterOnly": true });
        if let Some(frame) = frame {
            params["frame"] = json!(frame);
        }
        webdriver(b, &session, "Machopo.getElementClickPoint", params)
    };
    let viewport = |b: &mut Browser, frame: Option<&str>| {
        let params = frame.map_or(json!({}), |frame| json!({ "frame": frame }));
        webdriver(b, &session, "Machopo.getFrameViewport", params)
    };
    let size = script(b, &session, "return [innerWidth, innerHeight]", json!([])).unwrap();
    assert_eq!(
        viewport(b, None),
        Ok(json!({ "x": 0, "y": 0, "width": size[0], "height": size[1] }))
    );
    let a = find(b, None, "a");
    assert_eq!(point(b, None, &a), Ok(json!({ "x": 50, "y": 20 })));
    // Under another: its own point all the same.
    let under = find(b, None, "under");
    let middle = (size[0].as_f64().unwrap() / 2.0).floor() as i64;
    assert_eq!(point(b, None, &under), Ok(json!({ "x": middle, "y": 60 })));
    // Out of view: not scrolled to; its point, clamped to the viewport, is
    // outside it.
    let far = find(b, None, "far");
    let height = size[1].as_f64().unwrap();
    let below = point(b, None, &far).unwrap();
    assert_eq!(below["x"], 30);
    assert!(below["y"].as_f64().unwrap() > height, "{below}");
    assert_eq!(
        script(b, &session, "return scrollY", json!([])),
        Ok(json!(0))
    );
    let none = find(b, None, "none");
    assert_eq!(
        point(b, None, &none),
        Err("move target out of bounds".into())
    );
    // In the child frame: its viewport, and the points in it (the part of
    // `x` left of it cut off).
    let frame = switch_in(b, &session, None, json!({ "index": 0 })).unwrap();
    let frame = frame.as_str().unwrap().to_owned();
    assert_eq!(
        viewport(b, Some(&frame)),
        Ok(json!({ "x": 20, "y": 80, "width": 200, "height": 100 }))
    );
    let x = find(b, Some(&frame), "x");
    assert_eq!(point(b, Some(&frame), &x), Ok(json!({ "x": 20, "y": 20 })));
    assert!(browser.end().success());
}

/// `Machopo.getElementClickPoint`: the element scrolled into view, the center
/// of its part in view, in the top-level viewport's CSS px (through the
/// iframes around it); "element not interactable" with none of it in
/// view, "element click intercepted" where something else is on top.
#[test]
fn click_points_are_where_the_element_is_hit() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 } div { height: 40px } \
        #a { width: 100px } #over { margin-top: -40px } \
        #edge { margin-left: -50px; width: 100px } \
        iframe { border: 0; margin-left: 20px; width: 200px; height: 100px; display: block } \
        #far { margin-top: 2000px; width: 60px }</style>\
        <div id=a><span id=inside>in</span></div>\
        <div id=under></div><div id=over></div>\
        <div id=edge></div>\
        <iframe srcdoc='<body style=\"margin: 0\"><div id=x style=\"margin-top: 10px; width: 50px; height: 20px\"></div>'></iframe>\
        <iframe id=right style='margin-left: 300px' srcdoc='<body style=\"margin: 0\"><div id=out style=\"margin-left: -100px; width: 50px; height: 20px\"></div>'></iframe>\
        <div id=far></div><div id=none hidden></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let b = &mut browser;
    let find = |b: &mut Browser, frame: Option<&str>, id: &str| {
        let found = script_in(
            b,
            &session,
            frame,
            &format!("return document.getElementById('{id}')"),
            json!([]),
        );
        element_id(&found.unwrap())
    };
    let point = |b: &mut Browser, frame: Option<&str>, element: &str| {
        let mut params = json!({ "element": element });
        if let Some(frame) = frame {
            params["frame"] = json!(frame);
        }
        webdriver(b, &session, "Machopo.getElementClickPoint", params)
    };
    let a = find(b, None, "a");
    assert_eq!(point(b, None, &a), Ok(json!({ "x": 50, "y": 20 })));
    // Partly out of view: the center of what is in view.
    let edge = find(b, None, "edge");

    assert_eq!(point(b, None, &edge), Ok(json!({ "x": 25, "y": 100 })));
    let under = find(b, None, "under");
    assert_eq!(
        point(b, None, &under),
        Err("element click intercepted".into())
    );
    let none = find(b, None, "none");
    assert_eq!(
        point(b, None, &none),
        Err("element not interactable".into())
    );
    // In a child frame: the top-level viewport's.
    let frame = switch_in(b, &session, None, json!({ "index": 0 })).unwrap();
    let frame = frame.as_str().unwrap().to_owned();
    let x = find(b, Some(&frame), "x");
    assert_eq!(point(b, Some(&frame), &x), Ok(json!({ "x": 45, "y": 140 })));
    // In the page's viewport, but not in its frame's.
    let right = switch_in(b, &session, None, json!({ "index": 1 })).unwrap();
    let right = right.as_str().unwrap().to_owned();
    let out = find(b, Some(&right), "out");
    assert_eq!(
        point(b, Some(&right), &out),
        Err("element not interactable".into())
    );
    // An iframe: what is hit in its frame is the iframe's.
    let iframe = script_in(
        b,
        &session,
        None,
        "return document.querySelector('iframe')",
        json!([]),
    );
    let iframe = element_id(&iframe.unwrap());
    assert_eq!(point(b, None, &iframe), Ok(json!({ "x": 120, "y": 170 })));
    // Out of view: scrolled to its end, at the viewport's bottom.
    let far = find(b, None, "far");
    let at = point(b, None, &far).unwrap();
    let height = script(b, &session, "return innerHeight", json!([])).unwrap();
    assert_eq!(at, json!({ "x": 30, "y": height.as_i64().unwrap() - 20 }));
    assert!(browser.end().success());
}

/// The click point is the center of the element's first box (a link over
/// two lines: on the first), floored to whole CSS px.
#[test]
fn click_points_take_the_first_box_floored() {
    let page = "<!DOCTYPE html><style>body { margin: 0 } p { width: 80px; margin: 0 }</style>\
        <p><a id=wrap href='#'>aaaa bbbb cccc dddd</a></p>\
        <div id=half style='margin-top: 0.5px; width: 3px; height: 3px'></div>";
    let server = server(&[("/", Route::ok("text/html", page))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let b = &mut browser;
    for id in ["wrap", "half"] {
        let get = format!("return document.getElementById('{id}')");
        let element = element_id(&script(b, &session, &get, json!([])).unwrap());
        let first = format!(
            "const r = document.getElementById('{id}').getClientRects()[0]; \
             return {{ x: Math.floor(r.x + r.width / 2), y: Math.floor(r.y + r.height / 2) }}"
        );
        let expected = script(b, &session, &first, json!([])).unwrap();
        let params = json!({ "element": element });
        assert_eq!(
            webdriver(b, &session, "Machopo.getElementClickPoint", params),
            Ok(expected),
            "{id}"
        );
    }
    let lines = "return document.getElementById('wrap').getClientRects().length";
    assert!(script(b, &session, lines, json!([])).unwrap().as_u64() > Some(1));
    assert!(browser.end().success());
}

/// `Profiler`: the tab's renderer's inspector samples its script from
/// `start` to `stop`, which gives DevTools' `Profile`: the page's
/// functions, with their URL, a sample each interval asked for. Each round
/// runs a function
/// of its own: V8 optimizes a busy loop on the stack (OSR) and keeps that
/// code only on the function's feedback vector, which a profiler started
/// later doesn't learn of (its samples go to the caller), so a profile
/// must start before what it measures is compiled.
#[test]
fn the_profiler_samples_the_pages_script() {
    let busy = |name: &str| {
        format!(
            "function {name}(ms) {{ const end = Date.now() + ms; let n = 0; \
             while (Date.now() < end) n++; return n; }}"
        )
    };
    let page = format!(
        "<!DOCTYPE html><script>{}{}</script>",
        busy("coarse"),
        busy("fine")
    );
    let server = server(&[("/", Route::ok("text/html", page.as_str()))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    let url = server.url("/");
    browser.navigate(&session, &url);
    let b = &mut browser;
    let call =
        |b: &mut Browser, method: &str, params: Value| b.call(Some(&session), method, params);
    assert_eq!(
        call(b, "Profiler.start", json!({})).unwrap_err().code,
        SERVER_ERROR,
        "not enabled"
    );
    call(b, "Profiler.enable", json!({})).unwrap();
    assert_eq!(
        call(b, "Profiler.stop", json!({})).unwrap_err().code,
        SERVER_ERROR,
        "not started"
    );
    let mut samples = Vec::new();
    for (name, interval) in [("coarse", 20_000), ("fine", 500)] {
        call(
            b,
            "Profiler.setSamplingInterval",
            json!({ "interval": interval }),
        )
        .unwrap();
        call(b, "Profiler.start", json!({})).unwrap();
        let run = format!("return {name}(200) > 0");
        script(b, &session, &run, json!([])).unwrap();
        let profile = call(b, "Profiler.stop", json!({})).unwrap()["profile"].clone();
        let node = profile["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["callFrame"]["functionName"] == name);
        assert_eq!(node.expect(name)["callFrame"]["url"], url.as_str());
        let count = profile["samples"].as_array().unwrap().len();
        // A delta may be negative: V8 takes some samples out of order.
        let mut deltas: Vec<i64> = profile["timeDeltas"]
            .as_array()
            .unwrap()
            .iter()
            .map(|delta| delta.as_i64().unwrap())
            .collect();
        assert_eq!(deltas.len(), count);
        assert!(count >= 3, "{name}: {count} samples");
        deltas.sort_unstable();
        samples.push(deltas[deltas.len() / 2]);
    }
    // The time between samples (µs), not how many came: the sampler takes
    // what CPU it gets. 20 ms apart, and 0.5 ms (a starved sampler's more,
    // still far less).
    assert!(
        samples[0] >= 15_000 && samples[1] <= samples[0] / 4,
        "{samples:?}"
    );
    // A tab whose renderer has gone: an error at once.
    browser
        .call(
            None,
            "Target.setDiscoverTargets",
            json!({ "discover": true }),
        )
        .unwrap();
    browser.start_navigation(&session, "about:crash");
    browser.wait_for(|event| match &event.event {
        EventKind::TargetCrashed { .. } => Some(()),
        _ => None,
    });
    let error = browser
        .call(Some(&session), "Profiler.start", json!({}))
        .unwrap_err();
    assert_eq!(error.code, SERVER_ERROR);
    assert!(browser.end().success());
}

/// In a profile, the renderer waiting for its next task is `(idle)`, and
/// drawing a frame (style, layout and paint, no script) is `(program)`, as
/// Chrome's are with DevTools attached.
#[test]
fn the_profile_tells_waiting_from_rendering() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html><body>"))]);
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let b = &mut browser;
    let call =
        |b: &mut Browser, method: &str, params: Value| b.call(Some(&session), method, params);
    call(b, "Profiler.enable", json!({})).unwrap();
    call(
        b,
        "Profiler.setSamplingInterval",
        json!({ "interval": 500 }),
    )
    .unwrap();
    call(b, "Profiler.start", json!({})).unwrap();
    let fill = "for (let i = 0; i < 5000; i++) { const p = document.createElement('p'); \
                p.textContent = 'paragraph ' + i; document.body.append(p); } return 1";
    script(b, &session, fill, json!([])).unwrap();
    // A frame laid out and painted, then nothing to do.
    call(b, "Page.captureScreenshot", json!({})).unwrap();
    std::thread::sleep(Duration::from_millis(200));
    let profile = call(b, "Profiler.stop", json!({})).unwrap()["profile"].clone();
    // The time (µs) in each, from the time before each sample: how many
    // samples came is what CPU the sampler got. A delta may be negative:
    // V8 takes some samples out of order.
    let time = |name: &str| -> i64 {
        let nodes: Vec<u64> = profile["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["callFrame"]["functionName"] == name)
            .map(|node| node["id"].as_u64().unwrap())
            .collect();
        let samples = profile["samples"].as_array().unwrap();
        let deltas = profile["timeDeltas"].as_array().unwrap();
        samples
            .iter()
            .zip(deltas)
            .filter(|(sample, _)| nodes.contains(&sample.as_u64().unwrap()))
            .map(|(_, delta)| delta.as_i64().unwrap())
            .sum()
    };
    let (idle, program) = (time("(idle)"), time("(program)"));
    // 200 ms waiting: most of it.
    assert!(idle >= 100_000, "(idle) {idle} µs, (program) {program} µs");
    assert!(program >= 2_000, "(idle) {idle} µs, (program) {program} µs");
    assert!(browser.end().success());
}

/// A profile's folder is one machopo's at a time: a second one given it
/// doesn't start, and says why; the first goes on.
#[test]
fn a_second_machopo_on_a_profiles_folder_doesnt_start() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-profile-in-use");
    let _ = std::fs::remove_dir_all(&dir);
    let dir_arg = dir.to_str().expect("UTF-8");
    let mut first = Browser::start_with(&["--user-data-dir", dir_arg]);
    let session = first.attach();
    // Its folder is made and locked by now.
    first.navigate(&session, "about:blank");
    let mut second = Process::new(env!("CARGO_BIN_EXE_machopo"));
    second
        .args([
            "--remote-debugging-pipe",
            "--headless",
            "--user-data-dir",
            dir_arg,
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let (second, _input, _output) = start_machopo(&mut second);
    let second = second.wait_with_output().expect("machopo runs");
    assert!(!second.status.success());
    let said = String::from_utf8_lossy(&second.stderr);
    assert!(said.contains("in use by another machopo"), "{said}");
    assert!(said.contains(dir_arg), "{said}");
    first.navigate(&session, "about:blank");
    assert!(first.end().success());
}

/// What a page stores is in its profile's folder once machopo has ended:
/// its cookies and its `localStorage`, which are written in batches, the
/// last as machopo winds down (it then ends at once, its destructors not
/// run), and what the HTTP cache keeps, so the next machopo on the folder
/// has them. Winding down as it
/// should, it says nothing of giving up.
#[test]
fn a_profile_is_written_as_machopo_ends() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-profile-written");
    let _ = std::fs::remove_dir_all(&dir);
    let dir_arg = dir.to_str().expect("UTF-8");
    let log = dir.with_extension("log");
    let server = server(&[
        (
            "/",
            Route::ok("text/html", "<!DOCTYPE html><script src=/kept.js></script>"),
        ),
        (
            "/kept.js",
            Route::ok("text/javascript", "document.title = 'kept'")
                .with_header("Cache-Control", "max-age=3600"),
        ),
    ]);
    let mut first = Browser::start_logging(
        &["--user-data-dir", dir_arg],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let session = first.attach();
    first.navigate(&session, &server.url("/"));
    let stored = "document.cookie = 'a=1; max-age=3600'; localStorage.setItem('k', 'v')";
    script(&mut first, &session, stored, json!([])).unwrap();
    assert!(first.end().success());
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(!said.contains(browser::shutdown::GAVE_UP), "{said}");

    let mut second = Browser::start_with(&["--user-data-dir", dir_arg]);
    let session = second.attach();
    second.navigate(&session, &server.url("/"));
    let read = "return [document.cookie, localStorage.getItem('k')]";
    assert_eq!(
        script(&mut second, &session, read, json!([])),
        Ok(json!(["a=1", "v"]))
    );
    assert!(second.end().success());
    let scripts = server.requests().into_iter();
    assert_eq!(scripts.filter(|r| r.target == "/kept.js").count(), 1);
}

/// Renderers whose script never returns don't hold machopo up past its
/// limit, however many they are: each is asked to exit at once, all are
/// waited for until one deadline, and those still running then are
/// killed, a line for each: none is left once machopo has ended.
#[test]
fn hung_renderers_take_the_limit_once() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let log =
        std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-hung-renderers.log");
    let mut browser = Browser::start_logging(
        &["--process-per-tab"],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    for tab in 0..3 {
        let session = if tab == 0 {
            browser.attach()
        } else {
            let created = browser
                .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
                .unwrap();
            browser.attach_to(created["targetId"].as_str().unwrap())
        };
        browser.navigate(&session, &server.url("/"));
        browser.send(
            Some(&session),
            "Machopo.executeScript",
            json!({ "script": "while (true) {}", "async": false }),
        );
    }
    let renderers = renderer_pids(&mut browser);
    assert_eq!(renderers.len(), 3, "{renderers:?}");
    let watched = Watched::new(&renderers);
    std::thread::sleep(Duration::from_millis(300));
    let asked = std::time::Instant::now();
    assert!(browser.end().success());
    let took = asked.elapsed();
    // The limit, once, and the time to start and end; one renderer after
    // another was five seconds each.
    assert!(took < Duration::from_secs(4), "{took:?}");
    let said = std::fs::read_to_string(&log).unwrap();
    // Killed and reaped before machopo ended (its log says how each
    // ended). On Windows a renderer's job ends it with machopo anyway, so
    // there this sees only that none outlives it.
    assert!(!watched.any_alive(), "{renderers:?}: {said}");
    let killed = said
        .lines()
        .filter(|line| {
            line.starts_with(browser::shutdown::GAVE_UP) && line.contains("renderer process")
        })
        .count();
    assert_eq!(killed, 3, "{said}");
}

/// With `--single-process` a renderer is a thread, which can't be killed:
/// one whose script never returns has it stopped as machopo ends, and so
/// has the script of what it runs after (an interval's), until the
/// renderer gets to its end. Each ends by itself, nothing given up on.
#[test]
fn a_single_processs_hung_renderers_have_their_script_stopped() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-hung-threads.log");
    let mut browser = Browser::start_logging(
        &["--single-process"],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let scripts = [
        "while (true) {}",
        "setInterval(() => { for (;;) {} }, 0); for (;;) {}",
    ];
    for (tab, script) in scripts.into_iter().enumerate() {
        let session = if tab == 0 {
            browser.attach()
        } else {
            let created = browser
                .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
                .unwrap();
            browser.attach_to(created["targetId"].as_str().unwrap())
        };
        browser.navigate(&session, &server.url("/"));
        browser.send(
            Some(&session),
            "Machopo.executeScript",
            json!({ "script": script, "async": false }),
        );
    }
    std::thread::sleep(Duration::from_millis(300));
    let asked = std::time::Instant::now();
    assert!(browser.end().success());
    let took = asked.elapsed();
    assert!(took < Duration::from_secs(4), "{took:?}");
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(!said.contains(browser::shutdown::GAVE_UP), "{said}");
}

/// Sends `signal` (`TERM`, `INT`, `HUP`) to machopo's process.
#[cfg(unix)]
fn signal(browser: &Browser, signal: &str) {
    let sent = Process::new("kill")
        .args([format!("-{signal}"), browser.child.id().to_string()])
        .status()
        .expect("kill runs");
    assert!(sent.success());
}

/// SIGTERM ends machopo as closing it does: what the page stored is in
/// the profile for the next machopo, and it ends with success, saying
/// nothing of giving up; SIGINT and SIGHUP too.
#[cfg(unix)]
#[test]
fn a_signal_ends_machopo_as_closing_it_does() {
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-profile-signalled");
    let _ = std::fs::remove_dir_all(&dir);
    let dir_arg = dir.to_str().expect("UTF-8");
    let log = dir.with_extension("log");
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let mut first = Browser::start_logging(
        &["--user-data-dir", dir_arg],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let session = first.attach();
    first.navigate(&session, &server.url("/"));
    let stored = "document.cookie = 'a=1; max-age=3600'; localStorage.setItem('k', 'v')";
    script(&mut first, &session, stored, json!([])).unwrap();
    signal(&first, "TERM");
    assert!(wait(&mut first.child).success());
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(!said.contains(browser::shutdown::GAVE_UP), "{said}");

    let mut second = Browser::start_with(&["--user-data-dir", dir_arg]);
    let session = second.attach();
    second.navigate(&session, &server.url("/"));
    let read = "return [document.cookie, localStorage.getItem('k')]";
    assert_eq!(
        script(&mut second, &session, read, json!([])),
        Ok(json!(["a=1", "v"]))
    );
    drop(second);

    for name in ["INT", "HUP"] {
        let mut browser = Browser::start();
        let session = browser.attach();
        browser.navigate(&session, "about:blank");
        signal(&browser, name);
        assert!(wait(&mut browser.child).success(), "SIG{name}");
    }
}

/// A second signal, while machopo winds down (here waiting for a hung
/// renderer), ends it at once, saying so, with the code a shell gives a
/// process the signal ended.
#[cfg(unix)]
#[test]
fn a_second_signal_ends_machopo_at_once() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-second-signal.log");
    let mut browser = Browser::start_logging(
        &["--process-per-tab"],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    browser.send(
        Some(&session),
        "Machopo.executeScript",
        json!({ "script": "while (true) {}", "async": false }),
    );
    std::thread::sleep(Duration::from_millis(300));
    signal(&browser, "TERM");
    std::thread::sleep(Duration::from_millis(300));
    let asked = std::time::Instant::now();
    signal(&browser, "TERM");
    let status = wait(&mut browser.child);
    // Before the renderer's limit, two seconds after the first.
    assert!(asked.elapsed() < Duration::from_millis(1500));
    assert_eq!(status.code(), Some(128 + 15));
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(
        said.contains("a second signal (15), ending at once"),
        "{said}"
    );
}

/// A signal machopo was started ignoring (`nohup`'s SIGHUP) stays ignored.
#[cfg(unix)]
#[test]
fn a_signal_started_ignored_stays_so() {
    let mut nohup = Process::new("nohup");
    nohup
        .args([
            env!("CARGO_BIN_EXE_machopo"),
            "--remote-debugging-pipe",
            "--headless",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // `nohup` hands the pipe on, as it is at 3 and 4.
    let (mut child, mut input, output) = start_machopo(&mut nohup);
    // Up once it answers.
    input
        .write_all(br#"{"id":1,"method":"Browser.getVersion"}"#)
        .unwrap();
    input.write_all(&[0]).unwrap();
    let mut output = BufReader::new(output);
    let mut answer = Vec::new();
    output.read_until(0, &mut answer).unwrap();
    let sent = Process::new("kill")
        .args(["-HUP".to_owned(), child.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
    std::thread::sleep(Duration::from_millis(500));
    assert!(child.try_wait().unwrap().is_none(), "SIGHUP ended it");
    drop(input);
    assert!(wait(&mut child).success());
}

/// The commands a client sent before its input's end are each answered
/// before machopo ends, as they came: none lost to the end, which reaches
/// machopo by another way than they do.
#[test]
fn the_commands_before_the_inputs_end_are_answered() {
    let mut command = Process::new(env!("CARGO_BIN_EXE_machopo"));
    command
        .args(["--remote-debugging-pipe", "--headless"])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, mut input, mut output) = start_machopo(&mut command);
    const SENT: usize = 1000;
    for id in 0..SENT {
        let command = format!(r#"{{"id":{id},"method":"Browser.getVersion"}}"#);
        input.write_all(command.as_bytes()).unwrap();
        input.write_all(&[0]).unwrap();
    }
    drop(input);
    let mut answers = Vec::new();
    output.read_to_end(&mut answers).unwrap();
    assert!(wait(&mut child).success());
    let ids: Vec<u64> = answers
        .split(|byte| *byte == 0)
        .filter(|message| !message.is_empty())
        .filter_map(|message| serde_json::from_slice::<Value>(message).ok()?["id"].as_u64())
        .collect();
    assert_eq!(ids, (0..SENT as u64).collect::<Vec<_>>());
}

/// A client that never reads machopo's answers doesn't hold it up as it
/// ends: the pipe full, its writer can't finish, and is left once the
/// limit has passed, with a line.
#[test]
fn a_client_not_reading_doesnt_hold_machopo_up() {
    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-not-read.log");
    let mut command = Process::new(env!("CARGO_BIN_EXE_machopo"));
    command
        .args(["--remote-debugging-pipe", "--headless"])
        .stdout(Stdio::null())
        .stderr(Stdio::from(std::fs::File::create(&log).unwrap()));
    // Its output is kept and never read.
    let (mut child, mut input, _output) = start_machopo(&mut command);
    // Far more answers than a pipe holds (64 KiB on Linux, less on macOS).
    for id in 0..5000 {
        let command = format!(r#"{{"id":{id},"method":"Browser.getVersion"}}"#);
        input.write_all(command.as_bytes()).unwrap();
        input.write_all(&[0]).unwrap();
    }
    drop(input);
    let asked = std::time::Instant::now();
    let status = wait(&mut child);
    let took = asked.elapsed();
    // The limit (2 s), and the time to read and answer 5000 commands
    // first: past 5 s on a busy CI runner. A writer waited for without a
    // limit is `wait`'s to catch.
    assert!(took < Duration::from_secs(10), "{took:?}");
    assert!(status.success());
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(
        said.contains("gave up on the automation pipe's writer, left, its client not reading"),
        "{said}"
    );
}

/// A renderer that takes the whole limit doesn't cost a client that reads
/// the end of what machopo writes: the writer, done a moment after the
/// limit, isn't given up on.
#[test]
fn a_hung_renderer_leaves_the_writer_its_time() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    let log = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-writer-time.log");
    let mut browser = Browser::start_logging(
        &["--process-per-tab"],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    browser.send(
        Some(&session),
        "Machopo.executeScript",
        json!({ "script": "while (true) {}", "async": false }),
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        browser.call(None, "Browser.close", json!({})),
        Ok(json!({}))
    );
    assert!(wait(&mut browser.child).success());
    let said = std::fs::read_to_string(&log).unwrap();
    assert!(said.contains("renderer process"), "{said}");
    assert!(!said.contains("writer"), "{said}");
}

/// Processes watched from before they may end, to be asked whether they
/// have. On Windows by a handle to each, held from the start: the system
/// gives an ended process's id to the next one soon (and the tests start
/// many machopos at once), so a later look by id may find another
/// process, which the handle keeps from being given it. Elsewhere by id
/// (`kill -0`): Linux and macOS give an id out again only once they have
/// gone round the others.
struct Watched {
    #[cfg(unix)]
    pids: Vec<u64>,
    #[cfg(windows)]
    processes: Vec<std::os::windows::io::OwnedHandle>,
}

impl Watched {
    fn new(pids: &[u64]) -> Self {
        Self {
            #[cfg(unix)]
            pids: pids.to_vec(),
            #[cfg(windows)]
            processes: pids
                .iter()
                .map(|&pid| {
                    let id = u32::try_from(pid).expect("a process id");
                    sunaba::windows::open(id).unwrap_or_else(|e| panic!("{pid}: {e}"))
                })
                .collect(),
        }
    }

    /// Whether any is running (or not yet reaped).
    fn any_alive(&self) -> bool {
        #[cfg(unix)]
        let alive = self.pids.iter().any(|pid| {
            Process::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(Stdio::null())
                .status()
                .expect("kill runs")
                .success()
        });
        #[cfg(windows)]
        let alive = self.processes.iter().any(|process| {
            use std::os::windows::io::AsHandle as _;
            sunaba::windows::running(process.as_handle())
        });
        alive
    }
}

/// The ids of machopo's renderer processes (`SystemInfo.getProcessInfo`).
pub(crate) fn renderer_pids(browser: &mut Browser) -> Vec<u64> {
    let info = browser
        .call(None, "SystemInfo.getProcessInfo", json!({}))
        .unwrap();
    info["processInfo"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|process| process["type"] == "renderer")
        .map(|process| process["id"].as_u64().unwrap())
        .collect()
}

/// A renderer process doesn't have machopo's automation pipe: its standard
/// input is nothing, and its standard error machopo's, so a client hears
/// the pipe end once machopo has. (Its standard output is machopo's, as
/// `renderer_process` tests.)
#[cfg(target_os = "linux")]
#[test]
fn a_renderer_process_hasnt_the_pipe() {
    let log =
        std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-renderer-stdio.log");
    let mut browser = Browser::start_logging(
        &["--process-per-tab"],
        Stdio::from(std::fs::File::create(&log).unwrap()),
    );
    let session = browser.attach();
    browser.navigate(&session, "about:blank");
    let pids = renderer_pids(&mut browser);
    assert_eq!(pids.len(), 1, "{pids:?}");
    let fd = |n: u32| std::fs::read_link(format!("/proc/{}/fd/{n}", pids[0])).unwrap();
    assert_eq!(fd(0), std::path::Path::new("/dev/null"));
    assert_eq!(fd(2), log);
    assert!(browser.end().success());
}

/// Starts a machopo with a tab for each of `scripts`, in a renderer
/// process of its own that is left running its script (or doing nothing),
/// and kills the machopo: whether every renderer went with it.
fn renderers_go_with_a_killed_browser(scripts: &[Option<&str>]) {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>page"))]);
    // Whatever the run asks for: this is about the processes.
    let mut browser = Browser::start_with(&["--process-per-tab"]);
    for (tab, script) in scripts.iter().enumerate() {
        let session = if tab == 0 {
            browser.attach()
        } else {
            let created = browser
                .call(None, "Target.createTarget", json!({ "url": "about:blank" }))
                .unwrap();
            browser.attach_to(created["targetId"].as_str().unwrap())
        };
        browser.navigate(&session, &server.url("/"));
        if let Some(script) = script {
            browser.send(
                Some(&session),
                "Machopo.executeScript",
                json!({ "script": script, "async": false }),
            );
        }
    }
    let pids = renderer_pids(&mut browser);
    assert_eq!(pids.len(), scripts.len(), "{pids:?}");
    let watched = Watched::new(&pids);
    std::thread::sleep(Duration::from_secs(1));
    browser.child.kill().unwrap();
    browser.child.wait().unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while watched.any_alive() {
        assert!(
            std::time::Instant::now() < deadline,
            "a renderer outlived its browser"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Busy for far longer than a test waits; should the test fail, what it
/// left behind stops by itself.
const HUNG: &str = "const end = Date.now() + 30000; while (Date.now() < end) {}";

/// A browser killed takes its renderers with it, one that would never
/// read that it is gone too: nothing is left running, or holding open
/// the output it inherited.
#[test]
fn a_killed_browsers_hung_renderer_goes_with_it() {
    renderers_go_with_a_killed_browser(&[Some(HUNG)]);
}

/// One with nothing to do goes as well: it is the same end, not one of
/// its own that a renderer with time for it takes.
#[test]
fn a_killed_browsers_idle_renderer_goes_with_it() {
    renderers_go_with_a_killed_browser(&[None]);
}

/// One whose script keeps the collector busy: the process ends with
/// threads of the engine's at work.
#[test]
fn a_killed_browsers_allocating_renderer_goes_with_it() {
    let allocating = "const end = Date.now() + 30000; let kept = []; \
        while (Date.now() < end) { kept = [kept.length, new Array(4096).fill(kept)]; }";
    renderers_go_with_a_killed_browser(&[Some(allocating)]);
}

/// Each tab's renderer ends for itself: none waits for another.
#[test]
fn a_killed_browsers_hung_renderers_all_go_with_it() {
    renderers_go_with_a_killed_browser(&[Some(HUNG), Some(HUNG)]);
}

/// The page's loader after a navigation that `method` starts with `params`,
/// once the page has loaded.
fn navigated(browser: &mut Browser, session: &str, method: &str, params: Value) {
    browser.call(Some(session), method, params).unwrap();
    let loader = browser.wait_for(|event| match &event.event {
        EventKind::FrameStartedNavigating { loader_id, .. } => Some(loader_id.clone()),
        _ => None,
    });
    browser.load(&loader);
}

/// A reload validates the page, saying `max-age=0`, and its subresources go
/// as usual (a fresh one isn't asked for, a stale one is validated), its
/// iframe's document and its subresources too; one that ignores the cache
/// asks for all of them again, saying `no-cache`; back to the page takes
/// what is stored, stale or not.
#[test]
fn reloads_and_history_use_the_http_cache() {
    let stale = |content_type: &str, body: &str, etag: &str| {
        Route::ok(content_type, body)
            .with_header("Cache-Control", "no-cache")
            .with_header("ETag", etag)
            .revalidated()
    };
    let server = server(&[
        (
            "/a",
            stale(
                "text/html",
                "<!DOCTYPE html><link rel=stylesheet href=/fresh.css><script src=/stale.js></script>\
                 <iframe src=/frame></iframe><script>fetch('/fetched')</script><img src=/fresh.png>a",
                "\"a\"",
            ),
        ),
        (
            "/fresh.css",
            Route::ok("text/css", "p {}").with_header("Cache-Control", "max-age=60"),
        ),
        ("/stale.js", stale("text/javascript", "", "\"s\"")),
        (
            "/frame",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script src=/frame.js></script>frame",
            )
            .with_header("Cache-Control", "max-age=60"),
        ),
        (
            "/frame.js",
            Route::ok("text/javascript", "").with_header("Cache-Control", "max-age=60"),
        ),
        (
            "/fetched",
            Route::ok("text/plain", "").with_header("Cache-Control", "max-age=60"),
        ),
        (
            "/fresh.png",
            Route::ok("image/png", "").with_header("Cache-Control", "max-age=60"),
        ),
        ("/b", Route::ok("text/html", "<!DOCTYPE html>b")),
    ]);
    // What each request of `target` said: its `Cache-Control`, `Pragma` and
    // `If-None-Match`.
    let sent = |target: &str| -> Vec<(Option<String>, Option<String>, Option<String>)> {
        server
            .requests()
            .into_iter()
            .filter(|request| request.target == target)
            .map(|request| {
                (
                    request.header("cache-control").map(str::to_owned),
                    request.header("pragma").map(str::to_owned),
                    request.header("if-none-match").map(str::to_owned),
                )
            })
            .collect()
    };
    let some = |value: &str| Some(value.to_owned());
    let mut browser = Browser::start();
    let session = browser.attach();
    browser.navigate(&session, &server.url("/a"));
    navigated(&mut browser, &session, "Page.reload", json!({}));
    navigated(
        &mut browser,
        &session,
        "Page.reload",
        json!({ "ignoreCache": true }),
    );
    browser.navigate(&session, &server.url("/b"));
    let history = browser
        .call(Some(&session), "Page.getNavigationHistory", json!({}))
        .unwrap();
    let a = history["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["url"] == server.url("/a"))
        .unwrap()["id"]
        .clone();
    navigated(
        &mut browser,
        &session,
        "Page.navigateToHistoryEntry",
        json!({ "entryId": a }),
    );
    let no_cache = (some("no-cache"), some("no-cache"), None);
    assert_eq!(
        sent("/a"),
        [
            (None, None, None),
            (some("max-age=0"), None, some("\"a\"")),
            no_cache.clone(),
        ]
    );
    assert_eq!(sent("/fresh.css"), [(None, None, None), no_cache.clone()]);
    assert_eq!(
        sent("/stale.js"),
        [
            (None, None, None),
            (None, None, some("\"s\"")),
            no_cache.clone()
        ]
    );
    assert_eq!(sent("/frame"), [(None, None, None), no_cache.clone()]);
    assert_eq!(sent("/frame.js"), [(None, None, None), no_cache.clone()]);
    assert_eq!(sent("/fetched"), [(None, None, None), no_cache.clone()]);
    assert_eq!(sent("/fresh.png"), [(None, None, None), no_cache]);
    assert!(browser.end().success());
}

/// Started with nothing at 3 and 4 (by hand, in a shell), machopo says
/// what is missing and how a shell gives it, and doesn't start.
#[cfg(unix)]
#[test]
fn without_its_pipe_machopo_says_so() {
    let said = Process::new("sh")
        .arg("-c")
        .arg(r#"exec "$0" --remote-debugging-pipe --headless 3<&- 4>&-"#)
        .arg(env!("CARGO_BIN_EXE_machopo"))
        .stdin(Stdio::null())
        .output()
        .expect("machopo runs");
    assert_eq!(said.status.code(), Some(2));
    let said = String::from_utf8_lossy(&said.stderr);
    assert!(said.contains("nothing is open at descriptor 3"), "{said}");
    assert!(said.contains("3<&0 4>&1"), "{said}");
}

/// Standard input is no part of the pipe: Puppeteer's is a pipe it keeps
/// open and never writes. machopo answers on the pipe, and ends with the
/// pipe's input, the other still open.
#[test]
fn standard_input_left_open_is_no_part_of_the_pipe() {
    let mut command = Process::new(env!("CARGO_BIN_EXE_machopo"));
    command
        .args(["--remote-debugging-pipe", "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let (mut child, mut input, output) = ipc::platform::spawn_with_pipe(&mut command).unwrap();
    // Open till the test ends, never written.
    let _stdin = child.stdin.take().expect("piped");
    input
        .write_all(br#"{"id":1,"method":"Browser.getVersion"}"#)
        .unwrap();
    input.write_all(&[0]).unwrap();
    let mut output = BufReader::new(output);
    let mut answer = Vec::new();
    output.read_until(0, &mut answer).unwrap();
    assert!(String::from_utf8_lossy(&answer).contains(r#""id":1"#));
    drop(input);
    assert!(wait(&mut child).success());
}
