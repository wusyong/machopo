//! The mouse on a document through `EventHandler`, the frame's hit test
//! the tests' layout (`geometry::Fake`'s point): the boundary events and
//! their related targets, `mousemove` and its movement, clicks and their
//! targets, the secondary button's events, `:hover` and `:active`, a
//! target taken out by a listener, a checkpoint after each event; and the
//! keys' events at the body.

use std::rc::Rc;

use dom::NodeRoot;
use dom_api::{EventHandler, KeyInput, MouseAction, MouseButton, MouseInput};
use gfx::{NodeId, PointF};

use crate::geometry::{append, fake, id, page};
use crate::{Action, Arg, TestDom, This, call_on, with_host};

const TYPES: [&str; 11] = [
    "mouseover",
    "mouseout",
    "mouseenter",
    "mouseleave",
    "mousemove",
    "mousedown",
    "mouseup",
    "click",
    "dblclick",
    "auxclick",
    "contextmenu",
];

/// A page of `<section><p></p></section><div></div>` in the
/// body, every mouse event listened to at the document (capturing: the
/// boundary events don't bubble); the document, the root, the body, the
/// section (`a`), the paragraph (`b`), the div (`c`).
fn mouse_page() -> (TestDom, Vec<Rc<NodeRoot>>) {
    let (mut dom, mut nodes, _) = page(false, &["section", "div"]);
    let b = append(&mut dom, &nodes[3], "p");
    let c = nodes.remove(4);
    nodes.push(b);
    nodes.push(c);
    for (index, &event_type) in TYPES.iter().enumerate() {
        let (result, _) = call_on(
            &mut dom,
            This::Node(nodes[0].clone()),
            "EventTarget",
            "addEventListener",
            dom_api::MemberKind::Method,
            &[
                Arg::Str(event_type),
                Arg::Callback(index as u64 + 1),
                Arg::Bool(true),
            ],
        );
        result.unwrap();
    }
    (dom, nodes)
}

/// The mouse over `at` (none: nothing the hit test finds, the root) at
/// `point`, doing `action` with `buttons` down after it.
fn mouse(
    dom: &mut TestDom,
    handler: &mut EventHandler,
    document: &Rc<NodeRoot>,
    at: Option<NodeId>,
    input: MouseInput,
) {
    fake(dom, |fake| fake.at = at);
    with_host(dom, |host| handler.handle_mouse(host, document, &input));
}

fn input(action: MouseAction, x: f32, y: f32, buttons: u16) -> MouseInput {
    MouseInput {
        action,
        point: PointF::new(x, y),
        buttons,
        modifiers: dom::modifiers::SHIFT,
    }
}

/// The events logged since the last call by the document's capturing
/// listeners: type, target, related target.
fn events(dom: &TestDom) -> Vec<String> {
    dom.1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback <= TYPES.len() as u64)
        .map(|call| {
            let mut entry = format!("{} {}", call.event_type, call.event_target);
            if !call.related.is_empty() {
                entry.push('<');
                entry.push_str(&call.related);
            }
            entry
        })
        .collect()
}

#[test]
fn boundary_events_follow_the_pointer() {
    let (mut dom, nodes) = mouse_page();
    let [document, _, _, _, b, c] = nodes.as_slice() else {
        unreachable!()
    };
    let (b_id, c_id) = (id(&dom, b), id(&dom, c));
    // Bubbling: `mouseenter` and `mouseleave` reach it only at the document.
    let (result, _) = call_on(
        &mut dom,
        This::Node(document.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("mouseenter"), Arg::Callback(50), Arg::Bool(false)],
    );
    result.unwrap();
    let (result, _) = call_on(
        &mut dom,
        This::Node(document.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("mouseleave"), Arg::Callback(51), Arg::Bool(false)],
    );
    result.unwrap();
    let mut handler = EventHandler::default();
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(MouseAction::Move, 10.0, 10.0, 0),
    );
    let bubbled: Vec<String> = dom
        .1
        .log
        .borrow()
        .iter()
        .filter(|call| call.callback == 50)
        .map(|call| call.event_target.clone())
        .collect();
    assert_eq!(bubbled, ["#document"]);
    assert_eq!(
        events(&dom),
        [
            "mouseover P",
            "mouseenter #document",
            "mouseenter HTML",
            "mouseenter BODY",
            "mouseenter SECTION",
            "mouseenter P",
            "mousemove P"
        ]
    );
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(MouseAction::Move, 15.0, 7.0, 0),
    );
    let log = dom.1.log.borrow().clone();
    assert!(
        !log.iter().any(|call| call.callback == 51),
        "the elements' mouseleave doesn't bubble to the document"
    );
    assert_eq!(
        events(&dom),
        [
            "mouseout P<DIV",
            "mouseleave P<DIV",
            "mouseleave SECTION<DIV",
            "mouseover DIV<P",
            "mouseenter DIV<P",
            "mousemove DIV"
        ]
    );
    // From the last point; the modifiers and the buttons as given, the
    // boundary events with no movement.
    let moved = log.last().unwrap().mouse.unwrap();
    assert_eq!(
        (
            moved.movement_x,
            moved.movement_y,
            moved.client_x,
            moved.client_y
        ),
        (5.0, -3.0, 15.0, 7.0)
    );
    assert_eq!(
        (moved.modifiers, moved.buttons, moved.button, moved.detail),
        (dom::modifiers::SHIFT, 0, 0, 0)
    );
    assert_eq!(log[0].mouse.unwrap().movement_x, 0.0);
    assert!(log.iter().all(|call| call.trusted));
    // The same element: no boundary events.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(MouseAction::Move, 16.0, 7.0, 0),
    );
    assert_eq!(events(&dom), ["mousemove DIV"]);
    // Out of the viewport: nothing is under it.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(MouseAction::Move, 900.0, 7.0, 0),
    );
    assert_eq!(
        events(&dom),
        [
            "mouseout DIV",
            "mouseleave DIV",
            "mouseleave BODY",
            "mouseleave HTML",
            "mouseleave #document"
        ]
    );
    // Nothing hit in the viewport: the root.
    mouse(
        &mut dom,
        &mut handler,
        document,
        None,
        input(MouseAction::Move, 1.0, 1.0, 0),
    );
    assert_eq!(
        events(&dom),
        [
            "mouseover HTML",
            "mouseenter #document",
            "mouseenter HTML",
            "mousemove HTML"
        ]
    );
    // A checkpoint after each event.
    assert_eq!(dom.1.checkpoints.get(), 7 + 6 + 1 + 5 + 4);
}

#[test]
fn a_click_goes_to_what_the_press_and_the_release_share() {
    let (mut dom, nodes) = mouse_page();
    let [document, _, body, _, b, c] = nodes.as_slice() else {
        unreachable!()
    };
    let (b_id, c_id) = (id(&dom, b), id(&dom, c));
    let mut handler = EventHandler::default();
    let down = |button, count| MouseAction::Down {
        button,
        click_count: count,
    };
    let up = |button, count| MouseAction::Up {
        button,
        click_count: count,
    };
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(down(MouseButton::Primary, 1), 1.0, 1.0, 1),
    );
    events(&dom);
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(up(MouseButton::Primary, 1), 1.0, 1.0, 0),
    );
    let log = dom.1.log.borrow().clone();
    assert_eq!(events(&dom), ["mouseup P", "click P"]);
    let clicked = log[1].mouse.unwrap();
    assert_eq!((clicked.button, clicked.buttons, clicked.detail), (0, 0, 1));
    // A second click: `dblclick` after it.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(down(MouseButton::Primary, 2), 1.0, 1.0, 1),
    );
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(up(MouseButton::Primary, 2), 1.0, 1.0, 0),
    );
    assert_eq!(
        events(&dom),
        ["mousedown P", "mouseup P", "click P", "dblclick P"]
    );
    // Down on `b`, up on `c`: the click at the body, which has both.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(down(MouseButton::Primary, 1), 1.0, 1.0, 1),
    );
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(up(MouseButton::Primary, 1), 1.0, 1.0, 0),
    );
    let log = events(&dom);
    assert_eq!(log[0], "mousedown P");
    assert_eq!(&log[log.len() - 2..], ["mouseup DIV", "click BODY"]);
    let _ = body;
    // The auxiliary button: `auxclick`, no `click`, its own `button`.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(down(MouseButton::Auxiliary, 1), 1.0, 1.0, 4),
    );
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(up(MouseButton::Auxiliary, 1), 1.0, 1.0, 0),
    );
    let log = dom.1.log.borrow().clone();
    assert_eq!(
        events(&dom),
        ["mousedown DIV", "mouseup DIV", "auxclick DIV"]
    );
    assert_eq!(
        (log[0].mouse.unwrap().button, log[0].mouse.unwrap().buttons),
        (1, 4)
    );
    // The secondary one: `contextmenu` (on the press, but on Windows),
    // `auxclick`.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(down(MouseButton::Secondary, 1), 1.0, 1.0, 2),
    );
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(up(MouseButton::Secondary, 1), 1.0, 1.0, 0),
    );
    let expected: &[&str] = if cfg!(windows) {
        &[
            "mousedown DIV",
            "mouseup DIV",
            "auxclick DIV",
            "contextmenu DIV",
        ]
    } else {
        &[
            "mousedown DIV",
            "contextmenu DIV",
            "mouseup DIV",
            "auxclick DIV",
        ]
    };
    let log = dom.1.log.borrow().clone();
    assert_eq!(events(&dom), expected);
    let menu = log
        .iter()
        .find(|call| call.event_type == "contextmenu")
        .unwrap()
        .mouse
        .unwrap();
    assert_eq!((menu.button, menu.detail), (2, 0));
    // A release with no press: no click.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(up(MouseButton::Primary, 1), 1.0, 1.0, 0),
    );
    assert_eq!(events(&dom), ["mouseup DIV"]);
}

#[test]
fn hover_and_active_chains() {
    let (mut dom, nodes) = mouse_page();
    let [document, html, body, a, b, c] = nodes.as_slice() else {
        unreachable!()
    };
    let (b_id, c_id) = (id(&dom, b), id(&dom, c));
    let mut handler = EventHandler::default();
    let states = |dom: &TestDom| {
        dom.read(|cx| {
            [html, body, a, b, c].map(|node| {
                let node = node.get(cx.gc);
                (cx.is_hovered(node), cx.is_active(node))
            })
        })
    };
    let (none, hover, both, active) = ((false, false), (true, false), (true, true), (false, true));
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(MouseAction::Move, 1.0, 1.0, 0),
    );
    assert_eq!(states(&dom), [hover, hover, hover, hover, none]);
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(
            MouseAction::Down {
                button: MouseButton::Primary,
                click_count: 1,
            },
            1.0,
            1.0,
            1,
        ),
    );
    assert_eq!(states(&dom), [both, both, both, both, none]);
    // With the button down, `:hover` keeps to the active chain.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(MouseAction::Move, 1.0, 1.0, 1),
    );
    assert_eq!(states(&dom), [both, both, active, active, none]);
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(
            MouseAction::Up {
                button: MouseButton::Primary,
                click_count: 1,
            },
            1.0,
            1.0,
            0,
        ),
    );
    assert_eq!(states(&dom), [hover, hover, none, none, hover]);
    assert!(dom.read(|cx| {
        let document = document.get(cx.gc);
        cx.active_element(document).is_none()
            && cx
                .hover_element(document)
                .is_some_and(|hover| elise::Gc::ptr_eq(hover, c.get(cx.gc)))
    }));
    // Outside: nothing.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(MouseAction::Move, -1.0, 1.0, 0),
    );
    assert_eq!(states(&dom), [none; 5]);
}

/// A listener takes the element the pointer just entered out: the move's
/// own event goes to the nearest ancestor still there, `:hover` moves
/// there, and leaving it sends no `mouseout` at the element gone.
#[test]
fn a_target_taken_out_by_a_listener() {
    let (mut dom, nodes) = mouse_page();
    let [document, _, _, a, b, c] = nodes.as_slice() else {
        unreachable!()
    };
    let (b_id, c_id) = (id(&dom, b), id(&dom, c));
    // The paragraph's own `mouseover` listener.
    dom.1.actions.borrow_mut().insert(100, Action::Detach);
    let mut handler = EventHandler::default();
    let (result, _) = call_on(
        &mut dom,
        This::Node(b.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("mouseover"), Arg::Callback(100), Arg::Bool(false)],
    );
    result.unwrap();
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(MouseAction::Move, 1.0, 1.0, 0),
    );
    assert_eq!(
        events(&dom),
        [
            "mouseover P",
            "mouseenter #document",
            "mouseenter HTML",
            "mouseenter BODY",
            "mouseenter SECTION",
            "mousemove SECTION"
        ],
        "the paragraph's `mouseenter` out of the document's path"
    );
    assert!(dom.read(|cx| {
        let (a, b) = (a.get(cx.gc), b.get(cx.gc));
        !cx.is_connected(b)
            && !cx.is_hovered(b)
            && cx.is_hovered(a)
            && cx
                .hover_element(document.get(cx.gc))
                .is_some_and(|hover| elise::Gc::ptr_eq(hover, a))
    }));
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(c_id),
        input(MouseAction::Move, 1.0, 1.0, 0),
    );
    assert_eq!(
        events(&dom),
        [
            "mouseleave SECTION<DIV",
            "mouseover DIV<SECTION",
            "mouseenter DIV<SECTION",
            "mousemove DIV"
        ]
    );
}

#[test]
fn nothing_without_a_layout() {
    let (dom, nodes) = mouse_page();
    dom.1.layout.take();
    let mut handler = EventHandler::default();
    with_host(&dom, |host| {
        handler.handle_mouse(host, &nodes[0], &input(MouseAction::Move, 1.0, 1.0, 0));
    });
    assert!(events(&dom).is_empty());
}

/// A UI event's and a mouse event's wrappers are of their interfaces,
/// whoever made them.
#[test]
fn ui_events_are_of_their_interfaces() {
    let mut dom = TestDom::new();
    let (ui, mouse, plain) = dom.mutate(|cx| {
        let api = dom_api::ApiCx::new(cx);
        let flags = dom::EventFlags::default();
        let ui = cx.create_event(dom::names::intern("u"), flags, None);
        cx.set_ui_event_data(ui, None, 1);
        let mouse = cx.create_event(dom::names::intern("m"), flags, None);
        cx.set_mouse_event_data(mouse, None, &dom::MouseEventInfo::default(), None);
        let plain = cx.create_event(dom::names::intern("e"), flags, None);
        let interface = |event| {
            dom_api::INTERFACES[dom_api::interface_of(&api, dom::ObjectRef::Event(event))].name
        };
        (interface(ui), interface(mouse), interface(plain))
    });
    assert_eq!((ui, mouse, plain), ("UIEvent", "MouseEvent", "Event"));
}

fn key(down: bool, key: &str, text: &str, modifiers: u16) -> KeyInput {
    KeyInput {
        down,
        key: key.to_owned(),
        code: "KeyA".to_owned(),
        key_code: 65,
        text: text.to_owned(),
        location: 0,
        repeat: false,
        modifiers,
    }
}

/// A key's events at the body (no focus yet), else the root: `keydown`,
/// `keypress` for one that types something (its character the codes)
/// unless the `keydown` was canceled or it is a shortcut, `keyup`.
#[test]
fn keys_go_to_the_body() {
    let (mut dom, nodes) = mouse_page();
    let document = &nodes[0];
    for event_type in ["keydown", "keypress", "keyup"] {
        let (result, _) = call_on(
            &mut dom,
            This::Node(document.clone()),
            "EventTarget",
            "addEventListener",
            dom_api::MemberKind::Method,
            &[Arg::Str(event_type), Arg::Callback(1), Arg::Bool(true)],
        );
        result.unwrap();
    }
    let mut handler = EventHandler::default();
    let keys = |dom: &mut TestDom, handler: &mut EventHandler, input: KeyInput| {
        with_host(dom, |host| handler.handle_key(host, document, &input));
        dom.1
            .log
            .borrow_mut()
            .drain(..)
            .map(|call| format!("{} {} {}", call.event_type, call.event_target, call.trusted))
            .collect::<Vec<_>>()
    };
    let checkpoints = dom.1.checkpoints.get();
    assert_eq!(
        keys(&mut dom, &mut handler, key(true, "a", "a", 0)),
        ["keydown BODY true", "keypress BODY true"]
    );
    assert_eq!(dom.1.checkpoints.get(), checkpoints + 2);
    assert_eq!(
        keys(&mut dom, &mut handler, key(false, "a", "", 0)),
        ["keyup BODY true"]
    );
    // Enter's carriage return types; other control characters, a
    // shortcut and a key that types nothing don't.
    assert_eq!(
        keys(&mut dom, &mut handler, key(true, "Enter", "\r", 0)).len(),
        2
    );
    assert_eq!(
        keys(&mut dom, &mut handler, key(true, "Backspace", "\u{8}", 0)).len(),
        1
    );
    assert_eq!(
        keys(
            &mut dom,
            &mut handler,
            key(true, "a", "a", dom::modifiers::CTRL)
        )
        .len(),
        1
    );
    assert_eq!(
        keys(
            &mut dom,
            &mut handler,
            key(true, "a", "a", dom::modifiers::META)
        )
        .len(),
        1
    );
    assert_eq!(
        keys(
            &mut dom,
            &mut handler,
            key(true, "a", "a", dom::modifiers::CTRL | dom::modifiers::ALT)
        )
        .len(),
        2,
        "AltGr"
    );
    assert_eq!(
        keys(
            &mut dom,
            &mut handler,
            key(true, "Shift", "", dom::modifiers::SHIFT)
        )
        .len(),
        1
    );
    // A canceled `keydown`: no `keypress`.
    dom.1.actions.borrow_mut().insert(1, Action::Cancel);
    assert_eq!(
        keys(&mut dom, &mut handler, key(true, "a", "a", 0)),
        ["keydown BODY true"]
    );
    dom.1.actions.borrow_mut().clear();
    // No body: the root.
    dom.mutate(|cx| cx.remove(nodes[2].get(cx.gc)));
    assert_eq!(
        keys(&mut dom, &mut handler, key(false, "a", "", 0)),
        ["keyup HTML true"]
    );
}

/// A key event's data: the input's, a `keypress`'s codes its character.
#[test]
fn a_key_events_data() {
    let (mut dom, nodes) = mouse_page();
    let document = &nodes[0];
    let mut handler = EventHandler::default();
    let (result, _) = call_on(
        &mut dom,
        This::Node(document.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("keypress"), Arg::Callback(1), Arg::Bool(true)],
    );
    result.unwrap();
    let input = KeyInput {
        location: 3,
        repeat: true,
        ..key(true, "é", "é", dom::modifiers::SHIFT)
    };
    with_host(&dom, |host| handler.handle_key(host, document, &input));
    let keypress = dom.1.keys.borrow().last().cloned().expect("a keypress");
    assert_eq!(
        (
            keypress.key.to_string(),
            keypress.code.to_string(),
            keypress.location,
            keypress.repeat,
            keypress.modifiers
        ),
        (
            "é".to_owned(),
            "KeyA".to_owned(),
            3,
            true,
            dom::modifiers::SHIFT
        )
    );
    assert_eq!((keypress.char_code, keypress.key_code), (233, 233));
}

/// The pointer over a slotted node: the chains are the flat tree's, the
/// slot among them (its own `mouseenter` and `mouseleave`, which aren't
/// composed: heard at it, not at the document), the host the common
/// ancestor with what is next to it in the shadow tree.
#[test]
fn boundary_events_go_up_the_flat_tree() {
    let (mut dom, nodes) = mouse_page();
    let [document, _, _, a, b, _] = nodes.as_slice() else {
        unreachable!()
    };
    // The section a host: its paragraph in its shadow tree's slot, beside
    // an `<i>` of the shadow tree.
    let (slot, i) = dom.mutate(|cx| {
        let host = a.get(cx.gc);
        let owner = cx.document_of(host);
        let root = cx.attach_shadow(
            host,
            dom::ShadowRootInit {
                mode: dom::ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: dom::SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let slot = cx.create_element(owner, dom::QualName::html("slot"));
        let i = cx.create_element(owner, dom::QualName::html("i"));
        cx.append_child(root, slot).unwrap();
        cx.append_child(root, i).unwrap();
        cx.recalc_slot_assignments();
        (Rc::new(cx.root(slot)), Rc::new(cx.root(i)))
    });
    for (event_type, callback) in [("mouseenter", 60), ("mouseleave", 61)] {
        let (result, _) = call_on(
            &mut dom,
            This::Node(slot.clone()),
            "EventTarget",
            "addEventListener",
            dom_api::MemberKind::Method,
            &[Arg::Str(event_type), Arg::Callback(callback)],
        );
        result.unwrap();
    }
    let at_slot = |dom: &TestDom| -> Vec<String> {
        dom.1
            .log
            .borrow()
            .iter()
            .filter(|call| call.callback >= 60)
            .map(|call| call.event_type.clone())
            .collect()
    };
    let (b_id, i_id) = (id(&dom, b), id(&dom, &i));
    let mut handler = EventHandler::default();
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(MouseAction::Move, 10.0, 10.0, 0),
    );
    assert_eq!(at_slot(&dom), ["mouseenter"]);
    assert_eq!(
        events(&dom),
        [
            "mouseover P",
            "mouseenter #document",
            "mouseenter HTML",
            "mouseenter BODY",
            "mouseenter SECTION",
            "mouseenter P",
            "mousemove P"
        ]
    );
    // To the shadow tree's `<i>`: out of the slot and the paragraph, not
    // out of the host.
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(i_id),
        input(MouseAction::Move, 12.0, 10.0, 0),
    );
    // (`events` took the log: the slot's since then.)
    assert_eq!(at_slot(&dom), ["mouseleave"]);
    // (The log's related target is the event's own, not retargeted.)
    assert_eq!(
        events(&dom),
        [
            "mouseout P<I",
            "mouseleave P<I",
            "mouseover SECTION<P",
            "mousemove SECTION"
        ]
    );
    // `:hover` the flat tree's: the host's with the `<i>`'s.
    dom.mutate(|cx| {
        assert!(cx.is_hovered(i.get(cx.gc)) && cx.is_hovered(a.get(cx.gc)));
        assert!(!cx.is_hovered(slot.get(cx.gc)));
    });
}

/// The user's click on a disabled form control, or in one, isn't
/// dispatched; the press and the release are.
#[test]
fn a_disabled_control_isnt_clicked() {
    let (mut dom, nodes, _) = page(false, &["button"]);
    let [document, _, _, button] = nodes.as_slice() else {
        unreachable!()
    };
    let span = append(&mut dom, button, "span");
    for event_type in ["mousedown", "mouseup", "click"] {
        let (result, _) = call_on(
            &mut dom,
            This::Node(document.clone()),
            "EventTarget",
            "addEventListener",
            dom_api::MemberKind::Method,
            &[Arg::Str(event_type), Arg::Callback(1), Arg::Bool(true)],
        );
        result.unwrap();
    }
    let span_id = id(&dom, &span);
    let mut handler = EventHandler::default();
    let mut click = |dom: &mut TestDom| {
        for action in [
            MouseAction::Down {
                button: MouseButton::Primary,
                click_count: 1,
            },
            MouseAction::Up {
                button: MouseButton::Primary,
                click_count: 1,
            },
        ] {
            let buttons = if matches!(action, MouseAction::Down { .. }) {
                1
            } else {
                0
            };
            mouse(
                dom,
                &mut handler,
                document,
                Some(span_id),
                input(action, 1.0, 1.0, buttons),
            );
        }
        dom.1
            .log
            .borrow_mut()
            .drain(..)
            .map(|call| call.event_type)
            .collect::<Vec<_>>()
            .join(",")
    };
    assert_eq!(click(&mut dom), "mousedown,mouseup,click");
    dom.mutate(|cx| cx.set_attribute(button.get(cx.gc), "disabled", ""));
    assert_eq!(click(&mut dom), "mousedown,mouseup");
}

/// A page whose body has an `<iframe>` whose child frame's document
/// (`<html><body><p>`) is at (100, 50) in the viewport, every mouse event
/// listened to at both documents: the document, its iframe, the child's
/// document and its paragraph.
fn frame_page() -> (TestDom, [Rc<NodeRoot>; 4]) {
    let (mut dom, nodes) = mouse_page();
    let document = nodes[0].clone();
    let iframe = append(&mut dom, &nodes[2], "iframe");
    let (child, p) = dom.mutate(|cx| {
        let child = cx.create_document(true);
        let html = cx.create_element(child, dom::QualName::html("html"));
        cx.append_child(child, html).unwrap();
        let body = cx.create_element(child, dom::QualName::html("body"));
        cx.append_child(html, body).unwrap();
        let p = cx.create_element(child, dom::QualName::html("p"));
        cx.append_child(body, p).unwrap();
        cx.set_content_document(iframe.get(cx.gc), Some(child));
        (Rc::new(cx.root(child)), Rc::new(cx.root(p)))
    });
    for (index, &event_type) in TYPES.iter().enumerate() {
        let (result, _) = call_on(
            &mut dom,
            This::Node(child.clone()),
            "EventTarget",
            "addEventListener",
            dom_api::MemberKind::Method,
            &[
                Arg::Str(event_type),
                Arg::Callback(index as u64 + 1),
                Arg::Bool(true),
            ],
        );
        result.unwrap();
    }
    let child_id = id(&dom, &child);
    fake(&dom, |fake| {
        fake.frames.insert(child_id, PointF::new(100.0, 50.0));
    });
    (dom, [document, iframe, child, p])
}

/// The client coordinates of the events logged since the last call, with
/// their types and targets.
fn events_at(dom: &TestDom) -> Vec<String> {
    dom.1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback <= TYPES.len() as u64)
        .map(|call| {
            let mouse = call.mouse.unwrap_or_default();
            format!(
                "{} {} {},{}",
                call.event_type, call.event_target, mouse.client_x, mouse.client_y
            )
        })
        .collect()
}

/// Over a child frame, the input goes on to it: its events at its
/// element, at the point in its viewport; the iframe is what the parent
/// has it over (its boundary events, `:hover`, `:active` while a button is
/// down) with no mouse event of its own; a button down in the child frame
/// captures it until none is (the moves and the release outside go there);
/// leaving the child frame, it is over nothing there.
#[test]
fn the_mouse_goes_on_into_a_child_frame() {
    let (mut dom, [document, iframe, child, p]) = frame_page();
    let mut handler = EventHandler::default();
    let (p_id, body_id) = (id(&dom, &p), id(&dom, &(nodes_body(&dom, &document))));
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(p_id),
        input(MouseAction::Move, 110.0, 60.0, 0),
    );
    let log = events_at(&dom);
    assert!(
        log.contains(&"mouseover IFRAME 110,60".to_owned()),
        "{log:?}"
    );
    assert!(log.contains(&"mouseover P 10,10".to_owned()), "{log:?}");
    assert!(log.contains(&"mousemove P 10,10".to_owned()), "{log:?}");
    assert!(
        !log.iter()
            .any(|entry| entry.starts_with("mousemove IFRAME")),
        "{log:?}"
    );
    let hovered = |dom: &mut TestDom| {
        dom.mutate(|cx| {
            let (document, child) = (document.get(cx.gc), child.get(cx.gc));
            (
                cx.hover_element(document)
                    .map(|node| cx.node_name(node).to_string()),
                cx.hover_element(child)
                    .map(|node| cx.node_name(node).to_string()),
                cx.active_element(document)
                    .map(|node| cx.node_name(node).to_string()),
            )
        })
    };
    assert_eq!(
        hovered(&mut dom),
        (Some("IFRAME".into()), Some("P".into()), None)
    );
    // Down in it, moved out and released: the child frame has them.
    let down = MouseAction::Down {
        button: MouseButton::Primary,
        click_count: 1,
    };
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(p_id),
        input(down, 110.0, 60.0, 1),
    );
    assert_eq!(events_at(&dom), ["mousedown P 10,10"]);
    assert_eq!(hovered(&mut dom).2.as_deref(), Some("IFRAME"));
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(body_id),
        input(MouseAction::Move, 20.0, 20.0, 1),
    );
    let log = events_at(&dom);
    assert!(
        log.contains(&"mousemove HTML -80,-30".to_owned()),
        "captured: {log:?}"
    );
    assert!(
        !log.iter().any(|entry| entry.contains(" BODY 20")),
        "captured: {log:?}"
    );
    let up = MouseAction::Up {
        button: MouseButton::Primary,
        click_count: 1,
    };
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(body_id),
        input(up, 20.0, 20.0, 0),
    );
    assert_eq!(hovered(&mut dom).2, None);
    events_at(&dom);
    // No button down: the parent's body; the child frame has it over
    // nothing.
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(body_id),
        input(MouseAction::Move, 21.0, 20.0, 0),
    );
    assert_eq!(
        events_at(&dom),
        [
            "mouseout HTML -79,-30",
            "mouseleave HTML -79,-30",
            "mouseleave #document -79,-30",
            "mouseout IFRAME 21,20",
            "mouseleave IFRAME 21,20",
            "mouseover BODY 21,20",
            "mousemove BODY 21,20"
        ],
        "the child frame's first"
    );
    assert_eq!(hovered(&mut dom).0.as_deref(), Some("BODY"));
    assert_eq!(hovered(&mut dom).1, None);
    let _ = iframe;
}

/// The page's `<body>`.
fn nodes_body(dom: &TestDom, document: &Rc<NodeRoot>) -> Rc<NodeRoot> {
    Rc::new(dom.0.borrow_mut().read(|cx| {
        let html = cx.first_child(document.get(cx.gc)).unwrap();
        cx.root(cx.last_child(html).unwrap())
    }))
}

/// On the iframe's own border (the hit test finds the iframe), the input
/// stays the parent's: its events at the iframe, none in the child frame.
/// A wheel over the child frame's element: its `wheel` there, at the point
/// in its viewport. The child frame's state goes with its iframe.
#[test]
fn the_iframe_itself_and_wheels_in_a_child_frame() {
    let (mut dom, [document, iframe, child, p]) = frame_page();
    let mut handler = EventHandler::default();
    let (iframe_id, p_id) = (id(&dom, &iframe), id(&dom, &p));
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(iframe_id),
        input(MouseAction::Move, 99.0, 49.0, 0),
    );
    assert_eq!(
        events_at(&dom),
        [
            "mouseover IFRAME 99,49",
            "mouseenter #document 99,49",
            "mouseenter HTML 99,49",
            "mouseenter BODY 99,49",
            "mouseenter IFRAME 99,49",
            "mousemove IFRAME 99,49"
        ]
    );
    let (result, _) = call_on(
        &mut dom,
        This::Node(child.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("wheel"), Arg::Callback(60), Arg::Bool(true)],
    );
    result.unwrap();
    fake(&dom, |fake| fake.at = Some(p_id));
    let wheel = dom_api::WheelInput {
        point: PointF::new(130.0, 70.0),
        delta: PointF::new(0.0, 10.0),
        buttons: 0,
        modifiers: 0,
        cancelable: false,
    };
    with_host(&dom, |host| handler.handle_wheel(host, &document, &wheel));
    let wheels: Vec<String> = dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback == 60)
        .map(|call| {
            let mouse = call.mouse.unwrap_or_default();
            format!(
                "{} {},{}",
                call.event_target, mouse.client_x, mouse.client_y
            )
        })
        .collect();
    assert_eq!(wheels, ["P 30,20"]);
    // Into the child frame, then its iframe removed: its state let go of.
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(p_id),
        input(MouseAction::Move, 110.0, 60.0, 0),
    );
    events_at(&dom);
    dom.mutate(|cx| cx.remove(iframe.get(cx.gc)));
    let body = nodes_body(&dom, &document);
    let body_id = id(&dom, &body);
    mouse(
        &mut dom,
        &mut handler,
        &document,
        Some(body_id),
        input(MouseAction::Move, 10.0, 10.0, 0),
    );
    let log = events_at(&dom);
    assert!(
        !log.iter().any(|entry| entry.contains(" P ")),
        "the child frame is gone: {log:?}"
    );
}

/// `elementFromPoint` over a child frame finds the iframe (its hit test
/// doesn't go into child frames), the child frame's own at its point its
/// element; one of the parent's there, the child's root.
#[test]
fn element_from_point_stays_in_its_document() {
    let (mut dom, [document, iframe, child, p]) = frame_page();
    let p_id = id(&dom, &p);
    let body = nodes_body(&dom, &document);
    let body_id = id(&dom, &body);
    let found = |dom: &mut TestDom, at: &Rc<NodeRoot>, hit: NodeId| {
        fake(dom, |fake| fake.at = Some(hit));
        let (result, _) = call_on(
            dom,
            This::Node(at.clone()),
            "Document",
            "elementFromPoint",
            dom_api::MemberKind::Method,
            &[Arg::Number(10.0), Arg::Number(10.0)],
        );
        match result.unwrap() {
            crate::Ret::Node(node) => dom.mutate(|cx| cx.node_name(node.get(cx.gc)).to_string()),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(found(&mut dom, &document, p_id), "IFRAME");
    assert_eq!(found(&mut dom, &child, p_id), "P");
    assert_eq!(found(&mut dom, &child, body_id), "HTML");
    let _ = iframe;
}

/// The cursor is what the pointer is over asks for, as the hit test found
/// it before the events (`auto` resolved): the hand over a link or in one,
/// the I-beam over text that can be selected, the arrow beside it, over
/// text `user-select: none` makes unselectable, or over nothing; any
/// other keyword as it is.
#[test]
fn the_cursor_is_what_the_pointer_is_over() {
    use dom_api::CursorKeyword;
    let (mut dom, nodes) = mouse_page();
    let [document, _, _, a, b, c] = nodes.as_slice() else {
        unreachable!()
    };
    let link = append(&mut dom, a, "a");
    let in_link = append(&mut dom, &link, "span");
    let (b_id, c_id, link_id, in_link_id) = (
        id(&dom, b),
        id(&dom, c),
        id(&dom, &link),
        id(&dom, &in_link),
    );
    let mut handler = EventHandler::default();
    let mut cursor = |dom: &mut TestDom, at: Option<NodeId>, text: bool| {
        fake(dom, |fake| {
            fake.at = at;
            fake.over_text = text;
        });
        let point = input(MouseAction::Move, 5.0, 5.0, 0);
        with_host(dom, |host| handler.handle_mouse(host, document, &point))
    };
    assert_eq!(cursor(&mut dom, Some(c_id), false), CursorKeyword::Default);
    assert_eq!(cursor(&mut dom, Some(c_id), true), CursorKeyword::Text);
    fake(&dom, |fake| fake.unselectable.push(c_id));
    assert_eq!(cursor(&mut dom, Some(c_id), true), CursorKeyword::Default);
    // Not a link without an `href`.
    assert_eq!(
        cursor(&mut dom, Some(in_link_id), true),
        CursorKeyword::Text
    );
    dom.mutate(|cx| cx.set_attribute(link.get(cx.gc), "href", "/next"));
    assert_eq!(
        cursor(&mut dom, Some(link_id), false),
        CursorKeyword::Pointer
    );
    assert_eq!(
        cursor(&mut dom, Some(in_link_id), true),
        CursorKeyword::Pointer
    );
    fake(&dom, |fake| {
        fake.cursors.insert(b_id, CursorKeyword::Crosshair);
        fake.cursors.insert(in_link_id, CursorKeyword::Move);
    });
    assert_eq!(cursor(&mut dom, Some(b_id), true), CursorKeyword::Crosshair);
    assert_eq!(
        cursor(&mut dom, Some(in_link_id), true),
        CursorKeyword::Move
    );
    // Nothing hit: the root, styled `auto`.
    assert_eq!(cursor(&mut dom, None, false), CursorKeyword::Default);
    // Out of the viewport, no button down: over nothing.
    fake(&dom, |fake| fake.at = Some(link_id));
    let out = input(MouseAction::Move, 900.0, 5.0, 0);
    assert_eq!(
        with_host(&dom, |host| handler.handle_mouse(host, document, &out)),
        CursorKeyword::Default
    );
}

/// What is under a pointer that stays is found again where it last was,
/// as after a layout: the boundary events (with the keys held now) and
/// `:hover`, no `mousemove`, and its cursor; nothing before the pointer
/// was over the page.
#[test]
fn what_is_under_a_still_pointer_is_found_again() {
    use dom_api::CursorKeyword;
    let (mut dom, nodes) = mouse_page();
    let [document, _, _, _, b, c] = nodes.as_slice() else {
        unreachable!()
    };
    let (b_id, c_id) = (id(&dom, b), id(&dom, c));
    let mut handler = EventHandler::default();
    assert_eq!(
        with_host(&dom, |host| handler.recompute_under_mouse(host, document)),
        None
    );
    mouse(
        &mut dom,
        &mut handler,
        document,
        Some(b_id),
        input(MouseAction::Move, 3.0, 4.0, 0),
    );
    events(&dom);
    // The move had Shift; Ctrl alone is held now (a key says so).
    with_host(&dom, |host| {
        handler.handle_key(
            host,
            document,
            &key(true, "Control", "", dom::modifiers::CTRL),
        )
    });
    fake(&dom, |fake| {
        fake.at = Some(c_id);
        fake.cursors.insert(c_id, CursorKeyword::Wait);
    });
    assert_eq!(
        with_host(&dom, |host| handler.recompute_under_mouse(host, document)),
        Some(CursorKeyword::Wait)
    );
    let modifiers: Vec<u16> = dom
        .1
        .log
        .borrow()
        .iter()
        .filter_map(|call| call.mouse.as_ref().map(|mouse| mouse.modifiers))
        .collect();
    assert_eq!(modifiers.len(), 5);
    assert!(
        modifiers
            .iter()
            .all(|&modifiers| modifiers == dom::modifiers::CTRL),
        "{modifiers:?}"
    );
    assert_eq!(
        events(&dom),
        [
            "mouseout P<DIV",
            "mouseleave P<DIV",
            "mouseleave SECTION<DIV",
            "mouseover DIV<P",
            "mouseenter DIV<P"
        ]
    );
    let hovered = dom.read(|cx| {
        cx.hover_element(document.get(cx.gc))
            .is_some_and(|hover| elise::Gc::ptr_eq(hover, c.get(cx.gc)))
    });
    assert!(hovered, ":hover follows");
    // Nothing changed under it: no events.
    with_host(&dom, |host| handler.recompute_under_mouse(host, document));
    assert!(events(&dom).is_empty());
}

/// A scrolling key nobody took scrolls from the focused element out (its
/// ancestors and its document: the layout's chain), else from what the
/// mouse last went down on, else from the document; not one canceled,
/// with Ctrl, Alt or Meta, nor a Space a button takes (its release clicks
/// it), nor one typed into a text control, nor one whose `keypress` was
/// canceled.
#[test]
fn a_key_scrolls_from_the_focused_element() {
    let (mut dom, nodes, log) = page(false, &["div", "button", "input"]);
    let [document, html, body, div, button, field] = nodes.as_slice() else {
        unreachable!()
    };
    let ids: Vec<String> = [div, body, html, document]
        .iter()
        .map(|node| id(&dom, node).get().to_string())
        .collect();
    let from_div = ids.join(",");
    let from_document = ids[3].clone();
    let mut handler = EventHandler::default();
    let mut press = |dom: &mut TestDom, input: KeyInput| {
        with_host(dom, |host| handler.handle_key(host, document, &input));
        log.borrow_mut()
            .drain(..)
            .filter(|entry| entry.starts_with("key "))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        press(&mut dom, key(true, "ArrowDown", "", 0)),
        [format!("key ArrowDown false {from_document}")]
    );
    assert!(
        press(&mut dom, key(false, "ArrowDown", "", 0)).is_empty(),
        "the release"
    );
    dom.mutate(|cx| {
        cx.set_focused_element(document.get(cx.gc), Some(div.get(cx.gc)), false);
    });
    assert_eq!(
        press(&mut dom, key(true, "End", "", dom::modifiers::SHIFT)),
        [format!("key End true {from_div}")]
    );
    assert_eq!(
        press(&mut dom, key(true, " ", " ", 0)),
        [format!("key   false {from_div}")]
    );
    for modifier in [
        dom::modifiers::CTRL,
        dom::modifiers::ALT,
        dom::modifiers::META,
    ] {
        assert!(press(&mut dom, key(true, "End", "", modifier)).is_empty());
    }
    // A button's Space is the button's.
    dom.mutate(|cx| {
        cx.set_focused_element(document.get(cx.gc), Some(button.get(cx.gc)), false);
    });
    assert!(press(&mut dom, key(true, " ", " ", 0)).is_empty());
    assert_eq!(
        press(&mut dom, key(true, "PageDown", "", 0)).len(),
        1,
        "another key"
    );
    // A text field's Space types: its `keypress` is the field's.
    dom.mutate(|cx| {
        cx.set_focused_element(document.get(cx.gc), Some(field.get(cx.gc)), false);
    });
    assert!(press(&mut dom, key(true, " ", " ", 0)).is_empty());
    let typed = dom.read(|cx| cx.input_value(field.get(cx.gc)).to_string());
    assert_eq!(typed, " ", "typed");
    // A `keypress` canceled.
    dom.mutate(|cx| {
        cx.set_focused_element(document.get(cx.gc), Some(div.get(cx.gc)), false);
    });
    let (result, _) = call_on(
        &mut dom,
        This::Node(document.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("keypress"), Arg::Callback(91), Arg::Bool(false)],
    );
    result.unwrap();
    dom.1.actions.borrow_mut().insert(91, Action::Cancel);
    assert!(press(&mut dom, key(true, " ", " ", 0)).is_empty());
    assert_eq!(
        press(&mut dom, key(true, "PageDown", "", 0)).len(),
        1,
        "no keypress"
    );
    // Canceled.
    let (result, _) = call_on(
        &mut dom,
        This::Node(document.clone()),
        "EventTarget",
        "addEventListener",
        dom_api::MemberKind::Method,
        &[Arg::Str("keydown"), Arg::Callback(90), Arg::Bool(false)],
    );
    result.unwrap();
    dom.1.actions.borrow_mut().insert(90, Action::Cancel);
    assert!(press(&mut dom, key(true, "PageDown", "", 0)).is_empty());
}

/// With no focused element, a key scrolls from what the mouse last went
/// down on in the focused frame, after its release too.
#[test]
fn a_key_scrolls_from_where_the_mouse_went_down() {
    let (mut dom, nodes, log) = page(false, &["div"]);
    let [document, html, body, div] = nodes.as_slice() else {
        unreachable!()
    };
    let chain: Vec<String> = [div, body, html, document]
        .iter()
        .map(|node| id(&dom, node).get().to_string())
        .collect();
    let mut handler = EventHandler::default();
    let div_id = id(&dom, div);
    let button = MouseButton::Primary;
    for action in [
        MouseAction::Down {
            button,
            click_count: 1,
        },
        MouseAction::Up {
            button,
            click_count: 1,
        },
    ] {
        mouse(
            &mut dom,
            &mut handler,
            document,
            Some(div_id),
            input(action, 5.0, 5.0, 0),
        );
    }
    log.borrow_mut().clear();
    // The press focused nothing (a div isn't focusable).
    with_host(&dom, |host| {
        handler.handle_key(host, document, &key(true, "ArrowDown", "", 0))
    });
    assert_eq!(
        log.borrow()
            .iter()
            .filter(|e| e.starts_with("key "))
            .collect::<Vec<_>>(),
        [&format!("key ArrowDown false {}", chain.join(","))]
    );
}
