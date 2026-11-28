//! A DevTools session's state (Chromium's state cookie): the renderer tells
//! the tab what a session has on as a command changes it, the tab keeps
//! it, and a renderer launched in the place of one that has gone is
//! attached with it, before it loads anything, going on from there.

use std::time::{Duration, Instant};

use browser::{ContentsEvent, WebContents};
use crossbeam_channel::Receiver;
use net::test_server::Route;
use protocol::renderer::{DevToolsSessionState, RendererHostMsg};
use serde_json::{Value, json};

use crate::navigation::{TIMEOUT, in_memory, next_message, server, tab_in};

const SESSION: &str = "s";

/// The renderer's messages until `wanted` picks one, the tab having them
/// all; those before it, and it.
fn until(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
    mut wanted: impl FnMut(&RendererHostMsg) -> bool,
) -> Vec<RendererHostMsg> {
    let deadline = Instant::now() + TIMEOUT;
    let mut seen = Vec::new();
    loop {
        let msg = next_message(
            web_contents,
            host_rx,
            deadline.saturating_duration_since(Instant::now()),
        )
        .expect("the renderer answers");
        web_contents.on_renderer_message(&msg);
        let found = wanted(&msg);
        seen.push(msg);
        if found {
            return seen;
        }
    }
}

/// The session's command `method` with `params`, number `id`, and the
/// renderer's messages up to its answer.
fn command(
    web_contents: &mut WebContents,
    host_rx: &Receiver<ContentsEvent>,
    id: u64,
    method: &str,
    params: Value,
) -> Vec<RendererHostMsg> {
    let message = json!({ "id": id, "method": method, "params": params });
    web_contents.devtools_message(SESSION, message.to_string(), false);
    let mut seen = until(web_contents, host_rx, |msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["id"] == id))
    });
    // What the renderer tells of the command's change comes right after.
    while let Ok(msg) = next_message(web_contents, host_rx, Duration::from_millis(200)) {
        web_contents.on_renderer_message(&msg);
        seen.push(msg);
    }
    seen
}

/// Whether the renderer told a state of the session among `seen` before
/// the answer numbered `id`: a renderer ended as soon as the answer is read
/// (the tab ends it at once for `about:crash`, and drops what it sent
/// after) has left it to the next.
fn told_before_answer(seen: &[RendererHostMsg], id: u64) -> bool {
    let answer = seen.iter().position(|msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["id"] == id))
    });
    let told = seen.iter().position(|msg| {
        matches!(msg, RendererHostMsg::DevToolsSessionState { session, .. } if session == SESSION)
    });
    matches!((told, answer), (Some(told), Some(answer)) if told < answer)
}

/// The states the renderer told of the session among `seen`.
fn states(seen: &[RendererHostMsg]) -> Vec<&DevToolsSessionState> {
    seen.iter()
        .filter_map(|msg| match msg {
            RendererHostMsg::DevToolsSessionState { session, state } if session == SESSION => {
                Some(state)
            }
            _ => None,
        })
        .collect()
}

/// A state's `domain`, as its agent wrote it.
fn agent(state: &DevToolsSessionState, domain: &str) -> Option<Value> {
    state
        .agents
        .iter()
        .find(|(of, _)| of == domain)
        .map(|(_, state)| serde_json::from_str(state).unwrap())
}

/// The renderer tells what the session has on once a command changes it,
/// and not for one that changes nothing.
#[test]
fn a_sessions_state_is_told_as_a_command_changes_it() {
    let (mut web_contents, host_rx) = tab_in(in_memory());
    web_contents.devtools_attach(SESSION);
    let seen = command(&mut web_contents, &host_rx, 1, "Network.enable", json!({}));
    let told = states(&seen);
    assert_eq!(told.len(), 1, "{seen:?}");
    let network = agent(told[0], "Network").expect("Network on");
    assert_eq!(network["cacheDisabled"], false);
    let seen = command(&mut web_contents, &host_rx, 2, "Network.enable", json!({}));
    assert_eq!(
        states(&seen),
        Vec::<&DevToolsSessionState>::new(),
        "nothing changed"
    );
    let seen = command(
        &mut web_contents,
        &host_rx,
        3,
        "Network.setCacheDisabled",
        json!({ "cacheDisabled": true }),
    );
    let told = states(&seen);
    assert_eq!(told.len(), 1, "{seen:?}");
    assert_eq!(agent(told[0], "Network").unwrap()["cacheDisabled"], true);
    let seen = command(&mut web_contents, &host_rx, 4, "Network.disable", json!({}));
    let told = states(&seen);
    assert_eq!(told.len(), 1, "{seen:?}");
    assert_eq!(agent(told[0], "Network"), None);
    web_contents.shutdown();
}

/// A renderer launched in the place of one that has gone goes on with the
/// session as the one before had it: `Network` on, its new document's
/// requests are told without the session asking again.
#[test]
fn a_new_renderer_goes_on_with_the_sessions_state() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>")),
        (
            "/b",
            Route::ok("text/html", "<!DOCTYPE html><img src=/image.png>"),
        ),
        ("/image.png", Route::ok("image/png", Vec::new())),
    ]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    web_contents.navigate(server.url("/"));
    let loaded = web_contents.latest_navigation_id();
    until(
        &mut web_contents,
        &host_rx,
        |msg| matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == loaded),
    );
    web_contents.devtools_attach(SESSION);
    let seen = command(&mut web_contents, &host_rx, 1, "Network.enable", json!({}));
    assert_eq!(states(&seen).len(), 1, "{seen:?}");
    let renderer = web_contents.renderer_id();
    web_contents.navigate("about:crash");
    web_contents.navigate(server.url("/b"));
    assert_ne!(web_contents.renderer_id(), renderer);
    let image = server.url("/image.png");
    until(&mut web_contents, &host_rx, |msg| {
        let RendererHostMsg::DevToolsMessage { session, message } = msg else {
            return false;
        };
        let message: Value = serde_json::from_str(message).unwrap();
        session == SESSION
            && message["method"] == "Network.requestWillBeSent"
            && message["params"]["request"]["url"] == image.as_str()
    });
    web_contents.shutdown();
}

/// What a command turns on in the engine's inspector the renderer tells
/// before its answer, with what the agents have on: a renderer ended as
/// soon as the answer is read (the tab ends it at once for `about:crash`,
/// and drops what it sent after) leaves it to the next, which pauses on
/// the exception as asked.
#[test]
fn the_engines_state_comes_before_the_answer() {
    let server = server(&[
        ("/", Route::ok("text/html", "<!DOCTYPE html>")),
        (
            "/b",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><script>try { throw 1; } catch {}</script>",
            ),
        ),
    ]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    web_contents.navigate(server.url("/"));
    let loaded = web_contents.latest_navigation_id();
    until(
        &mut web_contents,
        &host_rx,
        |msg| matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == loaded),
    );
    web_contents.devtools_attach(SESSION);
    command(&mut web_contents, &host_rx, 1, "Network.enable", json!({}));
    command(&mut web_contents, &host_rx, 2, "Debugger.enable", json!({}));
    let message = json!({
        "id": 3,
        "method": "Debugger.setPauseOnExceptions",
        "params": { "state": "all" },
    });
    web_contents.devtools_message(SESSION, message.to_string(), false);
    let seen = until(&mut web_contents, &host_rx, |msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["id"] == 3))
    });
    let told = states(&seen);
    assert_eq!(told.len(), 1, "{seen:?}");
    assert!(told[0].engine.is_some(), "{seen:?}");
    assert!(agent(told[0], "Network").is_some(), "{seen:?}");
    web_contents.navigate("about:crash");
    web_contents.navigate(server.url("/b"));
    until(&mut web_contents, &host_rx, |msg| {
        let RendererHostMsg::DevToolsMessage { session, message } = msg else {
            return false;
        };
        let message: Value = serde_json::from_str(message).unwrap();
        session == SESSION
            && message["method"] == "Debugger.paused"
            && message["params"]["reason"] == "exception"
    });
    let resume = json!({ "id": 4, "method": "Debugger.resume", "params": {} });
    web_contents.devtools_message(SESSION, resume.to_string(), false);
    web_contents.shutdown();
}

/// Every agent goes on in a renderer launched in the place of one that has
/// gone, as the one before had it: the new renderer, asked a command that
/// changes nothing, finds the state the same as it was given, and tells
/// nothing (an agent left off would tell a state without it). What each
/// command turned on is told before its answer.
#[test]
fn every_agent_goes_on_in_a_new_renderer() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>"))]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    web_contents.navigate(server.url("/"));
    let loaded = web_contents.latest_navigation_id();
    until(
        &mut web_contents,
        &host_rx,
        |msg| matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == loaded),
    );
    web_contents.devtools_attach(SESSION);
    let commands = [
        ("Page.enable", json!({})),
        ("Page.setLifecycleEventsEnabled", json!({ "enabled": true })),
        ("Network.enable", json!({ "maxTotalBufferSize": 1000 })),
        ("Log.enable", json!({})),
        ("DOM.enable", json!({ "includeWhitespace": "all" })),
        ("CSS.enable", json!({})),
        ("Overlay.enable", json!({})),
        (
            "Overlay.setInspectMode",
            json!({ "mode": "searchForNode", "highlightConfig": {} }),
        ),
    ];
    let mut last = None;
    for (id, (method, params)) in (1..).zip(commands) {
        let seen = command(&mut web_contents, &host_rx, id, method, params);
        let told = states(&seen);
        assert_eq!(told.len(), 1, "{method}: {seen:?}");
        assert!(told_before_answer(&seen, id), "{method}: {seen:?}");
        last = Some(told[0].clone());
    }
    let last = last.unwrap();
    let domains: Vec<&str> = last
        .agents
        .iter()
        .map(|(domain, _)| domain.as_str())
        .collect();
    assert_eq!(domains, ["Page", "Network", "Log", "DOM", "CSS", "Overlay"]);
    web_contents.navigate("about:crash");
    web_contents.navigate(server.url("/"));
    let loaded = web_contents.latest_navigation_id();
    until(
        &mut web_contents,
        &host_rx,
        |msg| matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == loaded),
    );
    let seen = command(
        &mut web_contents,
        &host_rx,
        100,
        "Page.getFrameTree",
        json!({}),
    );
    assert_eq!(
        states(&seen),
        Vec::<&DevToolsSessionState>::new(),
        "{seen:?}"
    );
    web_contents.shutdown();
}

/// A command of an agent's answered while script is paused: what it turned
/// on is told before its answer too.
#[test]
fn an_agents_state_comes_before_the_answer_while_script_is_paused() {
    let server = server(&[("/", Route::ok("text/html", "<!DOCTYPE html>"))]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    web_contents.navigate(server.url("/"));
    let loaded = web_contents.latest_navigation_id();
    until(
        &mut web_contents,
        &host_rx,
        |msg| matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == loaded),
    );
    web_contents.devtools_attach(SESSION);
    command(&mut web_contents, &host_rx, 1, "Debugger.enable", json!({}));
    let evaluate = json!({
        "id": 2,
        "method": "Runtime.evaluate",
        "params": { "expression": "debugger" },
    });
    web_contents.devtools_message(SESSION, evaluate.to_string(), false);
    until(&mut web_contents, &host_rx, |msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["method"] == "Debugger.paused"))
    });
    let enable = json!({ "id": 3, "method": "Network.enable", "params": {} });
    web_contents.devtools_message(SESSION, enable.to_string(), false);
    let seen = until(&mut web_contents, &host_rx, |msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["id"] == 3))
    });
    assert!(told_before_answer(&seen, 3), "{seen:?}");
    let told = states(&seen);
    assert!(agent(told[0], "Network").is_some(), "{seen:?}");
    let resume = json!({ "id": 4, "method": "Debugger.resume", "params": {} });
    web_contents.devtools_message(SESSION, resume.to_string(), false);
    until(&mut web_contents, &host_rx, |msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["id"] == 2))
    });
    web_contents.shutdown();
}

/// An answer an agent gives later (`CSS.getStyleSheetText` of a `<link>`'s
/// sheet loaded before DevTools came, fetched again) is sent once it comes,
/// and nothing before it: every message the renderer sends is an answer or
/// an event.
#[test]
fn an_answer_given_later_is_sent_once() {
    let sheet = "p { color: red }";
    let server = server(&[
        (
            "/",
            Route::ok(
                "text/html",
                "<!DOCTYPE html><link rel=stylesheet href=/a.css><p>p",
            ),
        ),
        ("/a.css", Route::ok("text/css", sheet)),
    ]);
    let (mut web_contents, host_rx) = tab_in(in_memory());
    web_contents.navigate(server.url("/"));
    let loaded = web_contents.latest_navigation_id();
    until(
        &mut web_contents,
        &host_rx,
        |msg| matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == loaded),
    );
    web_contents.devtools_attach(SESSION);
    command(&mut web_contents, &host_rx, 1, "DOM.enable", json!({}));
    let seen = command(&mut web_contents, &host_rx, 2, "CSS.enable", json!({}));
    let added = seen
        .iter()
        .find_map(|msg| {
            let RendererHostMsg::DevToolsMessage { message, .. } = msg else {
                return None;
            };
            let message: Value = serde_json::from_str(message).ok()?;
            (message["method"] == "CSS.styleSheetAdded")
                .then(|| message["params"]["header"]["styleSheetId"].clone())
        })
        .unwrap_or_else(|| panic!("{seen:?}"));
    let asked = json!({
        "id": 3,
        "method": "CSS.getStyleSheetText",
        "params": { "styleSheetId": added },
    });
    web_contents.devtools_message(SESSION, asked.to_string(), false);
    let seen = until(&mut web_contents, &host_rx, |msg| {
        matches!(msg, RendererHostMsg::DevToolsMessage { message, .. }
            if serde_json::from_str::<Value>(message).is_ok_and(|message| message["id"] == 3))
    });
    let messages: Vec<Value> = seen
        .iter()
        .filter_map(|msg| match msg {
            RendererHostMsg::DevToolsMessage { message, .. } => {
                Some(serde_json::from_str(message).unwrap())
            }
            _ => None,
        })
        .collect();
    assert!(
        messages
            .iter()
            .all(|message| message.get("id").is_some() || message.get("method").is_some()),
        "{messages:?}"
    );
    assert_eq!(messages.last().unwrap()["result"]["text"], sheet);
    web_contents.shutdown();
}

/// A session's state over the most a renderer may give is a bad message:
/// the renderer is ended.
#[test]
fn a_sessions_state_too_large_is_a_bad_message() {
    let (mut web_contents, _host_rx) = tab_in(in_memory());
    web_contents.devtools_attach(SESSION);
    let state = DevToolsSessionState {
        agents: vec![(
            "Page".to_owned(),
            "x".repeat(DevToolsSessionState::MAX_BYTES),
        )],
        engine: None,
    };
    web_contents.on_renderer_message(&RendererHostMsg::DevToolsSessionState {
        session: SESSION.to_owned(),
        state,
    });
    assert!(web_contents.is_crashed());
    web_contents.shutdown();
}
