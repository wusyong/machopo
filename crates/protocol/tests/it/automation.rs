//! `protocol::automation`'s JSON: CDP's envelope both ways, a method by its
//! CDP name with its parameters checked, and CDP's errors for what isn't
//! a command machopo takes.

use protocol::automation::{
    AttachToTarget, Bounds, Command, CreateTarget, ElementParams, Error, Event, EventKind,
    ExecuteScript, GetTargetInfo, INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, Message,
    Method, Navigate, NavigateToHistoryEntry, Reload, Response, SetDiscoverTargets,
    SetLifecycleEventsEnabled, SetWindowBounds, SwitchToFrame, TargetInfo, TargetParams,
};
use serde_json::{Value, json};

fn every_method() -> Vec<Method> {
    vec![
        Method::BrowserGetVersion,
        Method::BrowserClose,
        Method::TargetGetTargets,
        Method::TargetGetTargetInfo(GetTargetInfo::default()),
        Method::TargetGetTargetInfo(GetTargetInfo {
            target_id: Some("T".into()),
        }),
        Method::TargetAttachToTarget(AttachToTarget {
            target_id: "T".into(),
            flatten: true,
        }),
        Method::TargetSetDiscoverTargets(SetDiscoverTargets { discover: true }),
        Method::PageEnable,
        Method::PageSetLifecycleEventsEnabled(SetLifecycleEventsEnabled { enabled: true }),
        Method::PageNavigate(Navigate {
            url: "about:blank".into(),
        }),
        Method::PageReload(Reload::default()),
        Method::PageReload(Reload {
            ignore_cache: Some(true),
            script_to_evaluate_on_load: None,
            loader_id: None,
        }),
        Method::PageGetNavigationHistory,
        Method::PageNavigateToHistoryEntry(NavigateToHistoryEntry { entry_id: 3 }),
        Method::TargetCreateTarget(CreateTarget {
            url: "about:blank".into(),
            background: true,
        }),
        Method::TargetCloseTarget(TargetParams {
            target_id: "T".into(),
        }),
        Method::TargetActivateTarget(TargetParams {
            target_id: "T".into(),
        }),
        Method::BrowserSetWindowBounds(SetWindowBounds {
            window_id: 1,
            bounds: Bounds {
                left: Some(-10),
                top: Some(20),
                width: Some(640),
                height: None,
            },
        }),
        Method::MachopoExecuteScript(ExecuteScript {
            frame: Some("F".into()),
            script: "return 1".into(),
            args: vec![json!(1)],
            is_async: false,
        }),
        Method::MachopoGetElementTagName(ElementParams {
            frame: None,
            element: "E".into(),
        }),
        Method::MachopoSwitchToFrame(SwitchToFrame {
            frame: Some("F".into()),
            index: Some(2),
            element: None,
        }),
        Method::MachopoSwitchToFrame(SwitchToFrame::default()),
    ]
}

#[test]
fn commands_go_both_ways() {
    for (id, method) in every_method().into_iter().enumerate() {
        let command = Command {
            id: id as i32,
            session_id: (id % 2 == 0).then(|| "S".to_owned()),
            method,
        };
        let json = command.to_json();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["method"], command.method.name());
        assert!(value["params"].is_object(), "{json}");
        assert_eq!(Command::from_json(&json), Ok(command), "{json}");
    }
}

#[test]
fn the_wire_is_cdps() {
    let command = Command::from_json(
        r#"{"id": 7, "method": "Page.navigate", "params": {"url": "https://example.com/"}, "sessionId": "S"}"#,
    )
    .unwrap();
    assert_eq!(command.id, 7);
    assert_eq!(command.session_id.as_deref(), Some("S"));
    assert_eq!(
        command.method,
        Method::PageNavigate(Navigate {
            url: "https://example.com/".into()
        })
    );
    // No params: as `{}`.
    assert_eq!(
        Command::from_json(r#"{"id": 1, "method": "Page.enable"}"#)
            .unwrap()
            .method,
        Method::PageEnable
    );
    let event = Message::Event(Event {
        session_id: Some("S".into()),
        event: EventKind::LifecycleEvent {
            frame_id: "F".into(),
            loader_id: "3".into(),
            name: "load".into(),
            timestamp: 1.5,
        },
    });
    assert_eq!(
        serde_json::from_str::<Value>(&event.to_json()).unwrap(),
        json!({
            "method": "Page.lifecycleEvent",
            "params": { "frameId": "F", "loaderId": "3", "name": "load", "timestamp": 1.5 },
            "sessionId": "S",
        })
    );
    // CDP's names; the top-level frame by no `frame` at all.
    let switch = Command {
        id: 3,
        session_id: Some("S".into()),
        method: Method::MachopoSwitchToFrame(SwitchToFrame {
            frame: None,
            index: None,
            element: Some("E".into()),
        }),
    };
    assert_eq!(
        serde_json::from_str::<Value>(&switch.to_json()).unwrap(),
        json!({
            "id": 3,
            "method": "Machopo.switchToFrame",
            "params": { "element": "E" },
            "sessionId": "S",
        })
    );
    let create = Command::from_json(
        r#"{"id": 4, "method": "Target.createTarget", "params": {"url": "about:blank"}}"#,
    )
    .unwrap();
    assert_eq!(
        create.method,
        Method::TargetCreateTarget(CreateTarget {
            url: "about:blank".into(),
            background: false,
        })
    );
    let error = Message::Response(Response {
        id: 2,
        session_id: None,
        result: Err(Error::new(METHOD_NOT_FOUND, "'X' wasn't found")),
    });
    assert_eq!(
        serde_json::from_str::<Value>(&error.to_json()).unwrap(),
        json!({ "id": 2, "error": { "code": -32601, "message": "'X' wasn't found" } })
    );
}

#[test]
fn messages_go_both_ways() {
    let info = TargetInfo {
        target_id: "T".into(),
        kind: "page".into(),
        title: "A".into(),
        url: "about:blank".into(),
        attached: true,
        can_access_opener: false,
    };
    let events = [
        EventKind::TargetCreated { target_info: info },
        EventKind::TargetDestroyed {
            target_id: "T".into(),
        },
        EventKind::TargetCrashed {
            target_id: "T".into(),
            status: "crashed".into(),
            error_code: 0,
        },
        EventKind::FrameStartedNavigating {
            frame_id: "F".into(),
            url: "about:blank".into(),
            loader_id: "1".into(),
            navigation_type: "reload".into(),
        },
        EventKind::NavigatedWithinDocument {
            frame_id: "F".into(),
            url: "about:blank#a".into(),
            navigation_type: "fragment".into(),
        },
        EventKind::LifecycleEvent {
            frame_id: "F".into(),
            loader_id: "1".into(),
            name: "DOMContentLoaded".into(),
            timestamp: 0.25,
        },
        EventKind::InspectorDetached {
            reason: "target_closed".into(),
        },
    ];
    for event in events {
        let message = Message::Event(Event {
            session_id: None,
            event,
        });
        assert_eq!(Message::from_json(&message.to_json()), Some(message));
    }
    for result in [Ok(json!({ "a": 1 })), Err(Error::new(-32000, "no"))] {
        let message = Message::Response(Response {
            id: 9,
            session_id: Some("S".into()),
            result,
        });
        assert_eq!(Message::from_json(&message.to_json()), Some(message));
    }
    // Any other object as it is: a page's event from V8's inspector, an
    // error with no id.
    let raw = Message::from_json(r#"{"method": "Page.fly", "params": {}, "sessionId": "S"}"#);
    let Some(Message::Raw { session_id, object }) = raw else {
        panic!("{raw:?}");
    };
    assert_eq!(session_id.as_deref(), Some("S"));
    assert_eq!(
        Value::Object(object),
        json!({ "method": "Page.fly", "params": {} })
    );
    let error = Message::error_without_id(Error::new(-32600, "no id"));
    assert_eq!(
        serde_json::from_str::<Value>(&error.to_json()).unwrap(),
        json!({ "error": { "code": -32600, "message": "no id" } })
    );
    assert_eq!(Message::from_json(&error.to_json()), Some(error));
    assert_eq!(Message::from_json("[1]"), None);
}

/// A command's `id` is an `int`, as Chrome's dispatcher takes it: negative
/// ones and `1e3` too, not `2^31` or `1.5`, which have no id to answer.
#[test]
fn command_ids_are_ints() {
    let id = |text: &str| Command::from_json(text).map(|command| command.id);
    let method = r#""method": "Browser.getVersion""#;
    assert_eq!(
        id(&format!(r#"{{"id": 2147483647, {method}}}"#)),
        Ok(i32::MAX)
    );
    assert_eq!(id(&format!(r#"{{"id": -1, {method}}}"#)), Ok(-1));
    assert_eq!(id(&format!(r#"{{"id": 1e3, {method}}}"#)), Ok(1000));
    for wrong in [
        "2147483648",
        "-2147483649",
        "1.5",
        "\"1\"",
        "18446744073709551616",
    ] {
        let (id, error) = id(&format!(r#"{{"id": {wrong}, {method}}}"#)).unwrap_err();
        assert_eq!(id, None, "{wrong}");
        assert_eq!(
            error.message, "Message must have integer 'id' property",
            "{wrong}"
        );
    }
}

/// A method the browser doesn't have is a page's, for its renderer: as
/// it is, its parameters unread. machopo's own domain has only machopo's methods.
#[test]
fn other_methods_are_passed_on() {
    let command = Command::from_json(
        r#"{"id": 1, "method": "Runtime.evaluate", "params": {"expression": "1"}, "sessionId": "S"}"#,
    )
    .unwrap();
    assert_eq!(
        command.method,
        Method::Other {
            name: "Runtime.evaluate".into(),
            params: json!({ "expression": "1" }),
        }
    );
    assert_eq!(command.method.name(), "Runtime.evaluate");
    assert_eq!(
        serde_json::from_str::<Value>(&command.to_json()).unwrap(),
        json!({ "id": 1, "method": "Runtime.evaluate", "params": { "expression": "1" }, "sessionId": "S" })
    );
}

/// What isn't a command is answered as CDP's dispatcher answers it, with
/// the command's id when it has one.
#[test]
fn wrong_commands_get_cdps_errors() {
    let error = |text: &str| Command::from_json(text).map(|_| ()).unwrap_err();
    let code = |text: &str| {
        let (id, error) = error(text);
        (id, error.code)
    };
    assert_eq!(code("{"), (None, INVALID_REQUEST));
    assert_eq!(code("[]"), (None, INVALID_REQUEST));
    assert_eq!(
        code(r#"{"method": "Page.enable"}"#),
        (None, INVALID_REQUEST)
    );
    assert_eq!(code(r#"{"id": 1}"#), (Some(1), INVALID_REQUEST));
    assert_eq!(
        code(r#"{"id": 1, "method": "Page.enable", "extra": 1}"#),
        (Some(1), INVALID_REQUEST)
    );
    assert_eq!(
        code(r#"{"id": 1, "method": "Page.enable", "sessionId": 3}"#),
        (Some(1), INVALID_REQUEST)
    );
    assert_eq!(
        code(r#"{"id": 1, "method": "Page.enable", "params": []}"#),
        (Some(1), INVALID_PARAMS)
    );
    assert_eq!(
        code(r#"{"id": 1, "method": "Machopo.fly"}"#),
        (Some(1), METHOD_NOT_FOUND)
    );
    // A parameter machopo doesn't take, a missing one, one of the wrong type,
    // and any for a method that takes none.
    for params in [
        r#"{"url": "a:b", "transitionType": "typed"}"#,
        r#"{}"#,
        r#"{"url": 1}"#,
    ] {
        let text = format!(r#"{{"id": 1, "method": "Page.navigate", "params": {params}}}"#);
        assert_eq!(code(&text), (Some(1), INVALID_PARAMS), "{params}");
    }
    assert_eq!(
        code(r#"{"id": 1, "method": "Page.enable", "params": {"x": 1}}"#),
        (Some(1), INVALID_PARAMS)
    );
}
