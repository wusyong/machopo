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

/// Every agent goes on in a renderer launched in the place of one that has
/// gone, as the one before had it: the new renderer, asked a command that
/// changes nothing, finds the state the same as it was given, and tells
/// nothing (an agent left off would tell a state without it).
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
