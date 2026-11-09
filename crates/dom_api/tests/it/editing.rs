//! Typing in a text control through the user's keys (`EventHandler::
//! handle_key`), the frame the tests' layout (`geometry::Fake`): text at
//! the selection with `beforeinput`, `textInput` and `input` (their
//! `inputType` and data), each that cancels it; Backspace and Delete by
//! grapheme cluster, the arrows, Home and End, Shift extending; `maxlength`;
//! read-only; Enter in a textarea and in an input; `change` as the user's
//! change is committed (Enter, the focus going), not without a change; no
//! typing with Ctrl, nor with a canceled `keydown` or `keypress`; the
//! user's selection queues `select`. WebDriver's Element Send Keys and
//! Element Clear.

use std::rc::Rc;

use dom::NodeRoot;
use dom_api::{EventHandler, KeyInput, MemberKind};
use gfx::NodeId;

use crate::geometry::{fake, id, page};
use crate::{Action, Arg, TestDom, This, call, call_on, with_host};

struct Page {
    document: Rc<NodeRoot>,
    field: Rc<NodeRoot>,
    area: Rc<NodeRoot>,
    other: Rc<NodeRoot>,
    handler: EventHandler,
    dom: TestDom,
}

const TYPES: [&str; 7] = [
    "keydown",
    "keypress",
    "beforeinput",
    "textInput",
    "input",
    "change",
    "blur",
];

/// A page of an input, a textarea and a div that can take the focus, all
/// with boxes, the page focused; the editing events listened to at the
/// document.
fn editing_page() -> Page {
    let (mut dom, nodes, _) = page(false, &["input", "textarea", "div"]);
    let [document, _, _, field, area, other] = nodes.as_slice() else {
        unreachable!()
    };
    dom.mutate(|cx| {
        cx.set_attribute(other.get(cx.gc), "tabindex", "0");
        let window = cx.create_window(document.get(cx.gc));
        cx.set_page_focused(window, true);
    });
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        for &node in &ids[1..] {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
    });
    for (index, event_type) in TYPES.iter().enumerate() {
        listen(&mut dom, document, event_type, index as u64 + 1);
    }
    Page {
        document: document.clone(),
        field: field.clone(),
        area: area.clone(),
        other: other.clone(),
        handler: EventHandler::default(),
        dom,
    }
}

fn listen(dom: &mut TestDom, document: &Rc<NodeRoot>, event_type: &'static str, callback: u64) {
    let (result, _) = call_on(
        dom,
        This::Node(document.clone()),
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[
            Arg::Str(event_type),
            Arg::Callback(callback),
            Arg::Bool(true),
        ],
    );
    result.unwrap();
}

/// Cancels the events of `event_type` (a listener at the document).
fn cancel(page: &mut Page, event_type: &'static str, callback: u64) {
    page.dom
        .1
        .actions
        .borrow_mut()
        .insert(callback, Action::Cancel);
    listen(&mut page.dom, &page.document.clone(), event_type, callback);
}

/// The events since the last call, the editing ones with their
/// `inputType` and data (`textInput`'s its text).
fn events(page: &Page) -> Vec<String> {
    page.dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback <= 7)
        .map(|call| match (&call.input, &call.text) {
            (Some(input), _) => format!(
                "{} {} {}",
                call.event_type,
                input.input_type.to_str(),
                input
                    .data
                    .as_ref()
                    .map_or_else(|| "null".to_owned(), |data| data.to_str().into_owned())
            ),
            (None, Some(text)) => format!("{} {text}", call.event_type),
            (None, None) => call.event_type,
        })
        .collect()
}

fn focus(page: &mut Page, element: &Rc<NodeRoot>) {
    call(
        &mut page.dom,
        element,
        "HTMLElement",
        "focus",
        MemberKind::Method,
        &[Arg::Dict(vec![])],
    )
    .unwrap();
    events(page);
}

fn key(name: &str, text: &str, modifiers: u16) -> KeyInput {
    KeyInput {
        down: true,
        key: name.to_owned(),
        code: name.to_owned(),
        key_code: 0,
        text: text.to_owned(),
        location: 0,
        repeat: false,
        modifiers,
    }
}

/// The user types `text`, a key each character.
fn type_text(page: &mut Page, text: &str) {
    for c in text.chars() {
        let text = c.to_string();
        press_key(page, key(&text, &text, 0));
    }
}

fn press(page: &mut Page, name: &str, modifiers: u16) {
    press_key(page, key(name, "", modifiers));
}

fn press_key(page: &mut Page, input: KeyInput) {
    let document = page.document.clone();
    let handler = &mut page.handler;
    with_host(&page.dom, |host| {
        handler.handle_key(host, &document, &input)
    });
}

/// The control's value and its selection (`start,end,direction`).
fn state(page: &Page, element: &Rc<NodeRoot>) -> (String, String) {
    page.dom.read(|cx| {
        let element = element.get(cx.gc);
        let selection = cx.text_selection(element).unwrap();
        (
            cx.text_control_value(element).to_str().into_owned(),
            format!(
                "{},{},{}",
                selection.start,
                selection.end,
                selection.direction.keyword()
            ),
        )
    })
}

fn set_value(page: &mut Page, element: &Rc<NodeRoot>, value: &str) {
    page.dom.mutate(|cx| {
        let element = element.get(cx.gc);
        if cx.is_textarea(element) {
            cx.set_textarea_value(element, value);
        } else {
            cx.set_input_value(element, value).unwrap();
        }
    });
}

#[test]
fn typing_inserts_at_the_selection() {
    let mut page = editing_page();
    let field = page.field.clone();
    focus(&mut page, &field);
    type_text(&mut page, "a");
    assert_eq!(
        events(&page),
        [
            "keydown",
            "keypress",
            "beforeinput insertText a",
            "textInput a",
            "input insertText a"
        ]
    );
    type_text(&mut page, "bc");
    assert_eq!(
        state(&page, &field),
        ("abc".to_owned(), "3,3,none".to_owned())
    );
    page.dom
        .mutate(|cx| cx.set_selection_range(field.get(cx.gc), 1, 2, dom::SelectionDirection::None));
    type_text(&mut page, "\u{1F600}");
    assert_eq!(
        state(&page, &field),
        ("a\u{1F600}c".to_owned(), "3,3,none".to_owned()),
        "in place of the selection"
    );
    // With Ctrl, nothing is typed (a shortcut's); with Ctrl+Alt (AltGr) it is.
    events(&page);
    press_key(&mut page, key("z", "z", dom::modifiers::CTRL));
    assert_eq!(state(&page, &field).0, "a\u{1F600}c");
    press_key(
        &mut page,
        key("z", "z", dom::modifiers::CTRL | dom::modifiers::ALT),
    );
    assert_eq!(state(&page, &field).0, "a\u{1F600}zc");
    // Not in an element that isn't a text control.
    let other = page.other.clone();
    focus(&mut page, &other);
    type_text(&mut page, "q");
    assert_eq!(events(&page), ["keydown", "keypress"]);
}

#[test]
fn each_event_can_cancel_it() {
    let mut page = editing_page();
    let field = page.field.clone();
    focus(&mut page, &field);
    cancel(&mut page, "beforeinput", 20);
    type_text(&mut page, "a");
    assert_eq!(
        events(&page),
        ["keydown", "keypress", "beforeinput insertText a"]
    );
    assert_eq!(state(&page, &field).0, "");
    page.dom.1.actions.borrow_mut().insert(20, Action::Nothing);
    cancel(&mut page, "textInput", 21);
    type_text(&mut page, "a");
    assert_eq!(
        events(&page),
        [
            "keydown",
            "keypress",
            "beforeinput insertText a",
            "textInput a"
        ]
    );
    assert_eq!(state(&page, &field).0, "");
    page.dom.1.actions.borrow_mut().insert(21, Action::Nothing);
    cancel(&mut page, "keypress", 22);
    type_text(&mut page, "a");
    assert_eq!(events(&page), ["keydown", "keypress"]);
    page.dom.1.actions.borrow_mut().insert(22, Action::Nothing);
    set_value(&mut page, &field, "ab");
    cancel(&mut page, "keydown", 23);
    press(&mut page, "Backspace", 0);
    assert_eq!(
        state(&page, &field).0,
        "ab",
        "a canceled keydown deletes nothing"
    );
    press(&mut page, "ArrowLeft", 0);
    assert_eq!(state(&page, &field).1, "2,2,none", "nor moves");
}

#[test]
fn deleting_and_moving_by_grapheme_cluster() {
    let mut page = editing_page();
    let field = page.field.clone();
    focus(&mut page, &field);
    // e + combining acute, an emoji with a skin tone, x.
    set_value(&mut page, &field, "e\u{301}\u{1F44D}\u{1F3FD}x");
    press(&mut page, "ArrowLeft", 0);
    assert_eq!(state(&page, &field).1, "6,6,none");
    press(&mut page, "Backspace", 0);
    assert_eq!(
        state(&page, &field),
        ("e\u{301}x".to_owned(), "2,2,none".to_owned())
    );
    events(&page);
    press(&mut page, "Delete", 0);
    assert_eq!(
        state(&page, &field),
        ("e\u{301}".to_owned(), "2,2,none".to_owned())
    );
    assert_eq!(
        events(&page),
        [
            "keydown",
            "beforeinput deleteContentForward null",
            "input deleteContentForward null"
        ]
    );
    // Nothing to delete: no events.
    press(&mut page, "Delete", 0);
    assert_eq!(events(&page), ["keydown"]);
    // Delete takes a whole cluster after the cursor.
    set_value(&mut page, &field, "a\u{1F44D}\u{1F3FD}");
    page.dom
        .mutate(|cx| cx.set_selection_range(field.get(cx.gc), 1, 1, dom::SelectionDirection::None));
    press(&mut page, "Delete", 0);
    assert_eq!(state(&page, &field).0, "a");
    // With Ctrl, an edit key does nothing (no moves by word yet).
    set_value(&mut page, &field, "ab");
    press(&mut page, "Backspace", dom::modifiers::CTRL);
    press(&mut page, "ArrowLeft", dom::modifiers::CTRL);
    assert_eq!(
        state(&page, &field),
        ("ab".to_owned(), "2,2,none".to_owned())
    );
    // Shift extends from the anchor; an arrow collapses to the side.
    set_value(&mut page, &field, "abcd");
    press(&mut page, "Home", 0);
    let tasks = |page: &mut Page| {
        let document = page.document.clone();
        page.dom.mutate(|cx| {
            let window = cx.window_of(document.get(cx.gc)).unwrap();
            std::iter::from_fn(|| cx.take_element_task(window)).count()
        })
    };
    tasks(&mut page);
    press(&mut page, "ArrowRight", dom::modifiers::SHIFT);
    press(&mut page, "ArrowRight", dom::modifiers::SHIFT);
    assert_eq!(state(&page, &field).1, "0,2,forward");
    assert_eq!(tasks(&mut page), 2, "the user's selection queues `select`");
    press(&mut page, "ArrowLeft", 0);
    assert_eq!(state(&page, &field).1, "0,0,none");
    assert_eq!(tasks(&mut page), 0, "not a collapsed one");
    press(&mut page, "End", dom::modifiers::SHIFT);
    press(&mut page, "ArrowRight", 0);
    assert_eq!(state(&page, &field).1, "4,4,none");
    press(&mut page, "ArrowLeft", dom::modifiers::SHIFT);
    assert_eq!(state(&page, &field).1, "3,4,backward");
    // A selection's Backspace deletes it.
    events(&page);
    press(&mut page, "Backspace", 0);
    assert_eq!(
        state(&page, &field),
        ("abc".to_owned(), "3,3,none".to_owned())
    );
    assert_eq!(
        events(&page),
        [
            "keydown",
            "beforeinput deleteContentBackward null",
            "input deleteContentBackward null"
        ]
    );
    // Home and End go to the line's ends in a textarea.
    let area = page.area.clone();
    focus(&mut page, &area);
    set_value(&mut page, &area, "ab\ncd\nef");
    page.dom
        .mutate(|cx| cx.set_selection_range(area.get(cx.gc), 4, 4, dom::SelectionDirection::None));
    press(&mut page, "Home", 0);
    assert_eq!(state(&page, &area).1, "3,3,none");
    press(&mut page, "End", 0);
    assert_eq!(state(&page, &area).1, "5,5,none");
}

#[test]
fn max_length_and_read_only() {
    let mut page = editing_page();
    let field = page.field.clone();
    page.dom
        .mutate(|cx| cx.set_attribute(field.get(cx.gc), "maxlength", " 3 "));
    focus(&mut page, &field);
    set_value(&mut page, &field, "ab");
    type_text(&mut page, "cd");
    assert_eq!(state(&page, &field).0, "abc");
    events(&page);
    type_text(&mut page, "e");
    assert_eq!(events(&page), ["keydown", "keypress"], "no room: no events");
    // In place of a selection, as long as the rest leaves room.
    page.dom
        .mutate(|cx| cx.set_selection_range(field.get(cx.gc), 0, 3, dom::SelectionDirection::None));
    type_text(&mut page, "\u{1F600}");
    assert_eq!(state(&page, &field).0, "\u{1F600}");
    // Read-only: no typing, no deleting (no `beforeinput`); moving still.
    page.dom
        .mutate(|cx| cx.set_attribute(field.get(cx.gc), "readonly", ""));
    events(&page);
    type_text(&mut page, "x");
    press(&mut page, "Backspace", 0);
    assert_eq!(state(&page, &field).0, "\u{1F600}");
    assert_eq!(events(&page), ["keydown", "keypress", "keydown"]);
    press(&mut page, "Home", 0);
    assert_eq!(state(&page, &field).1, "0,0,none");
}

#[test]
fn enter_and_committing_the_change() {
    let mut page = editing_page();
    let (field, area, other) = (page.field.clone(), page.area.clone(), page.other.clone());
    // A textarea's Enter is a line break.
    focus(&mut page, &area);
    press_key(&mut page, key("Enter", "\r", 0));
    assert_eq!(state(&page, &area).0, "\n");
    assert_eq!(
        events(&page),
        [
            "keydown",
            "keypress",
            "beforeinput insertLineBreak null",
            "textInput \n",
            "input insertLineBreak null"
        ]
    );
    // Its change is committed as the focus goes: `change`, then `blur`.
    call(
        &mut page.dom,
        &other,
        "HTMLElement",
        "focus",
        MemberKind::Method,
        &[Arg::Dict(vec![])],
    )
    .unwrap();
    assert_eq!(events(&page), ["change", "blur"]);
    // An input's Enter commits; nothing more to commit after.
    focus(&mut page, &field);
    type_text(&mut page, "x");
    events(&page);
    press_key(&mut page, key("Enter", "\r", 0));
    assert_eq!(events(&page), ["keydown", "keypress", "change"]);
    assert_eq!(state(&page, &field).0, "x");
    call(
        &mut page.dom,
        &other,
        "HTMLElement",
        "focus",
        MemberKind::Method,
        &[Arg::Dict(vec![])],
    )
    .unwrap();
    assert_eq!(events(&page), ["blur"]);
    // A change undone before the commit: none.
    focus(&mut page, &field);
    type_text(&mut page, "y");
    press(&mut page, "Backspace", 0);
    events(&page);
    call(
        &mut page.dom,
        &other,
        "HTMLElement",
        "focus",
        MemberKind::Method,
        &[Arg::Dict(vec![])],
    )
    .unwrap();
    assert_eq!(events(&page), ["blur"]);
}

/// WebDriver's Element Send Keys before the keys (`webdriver_send_keys`)
/// and Element Clear (`webdriver_clear`).
#[test]
fn webdriver_send_keys_and_clear() {
    let mut page = editing_page();
    let (field, area, other) = (page.field.clone(), page.area.clone(), page.other.clone());
    let document = page.document.clone();
    let id_of = |page: &Page, node: &Rc<NodeRoot>| id(&page.dom, node);
    let send = |page: &mut Page, node: &Rc<NodeRoot>, text: &str| {
        let element = id_of(page, node);
        with_host(&page.dom, |host| {
            dom_api::webdriver_send_keys(host, &document, element, text)
        })
    };
    // A text control: focused, its cursor at the end; the driver types.
    set_value(&mut page, &field, "abc");
    page.dom
        .mutate(|cx| cx.set_selection_range(field.get(cx.gc), 0, 0, dom::SelectionDirection::None));
    assert_eq!(send(&mut page, &field, "x"), Ok(true));
    assert_eq!(state(&page, &field).1, "3,3,none");
    // Focused already: the cursor stays.
    page.dom
        .mutate(|cx| cx.set_selection_range(field.get(cx.gc), 1, 1, dom::SelectionDirection::None));
    assert_eq!(send(&mut page, &field, "x"), Ok(true));
    assert_eq!(state(&page, &field).1, "1,1,none");
    // A non-typeable control: its value set, no keys; read-only, not
    // interactable; a file input unsupported.
    page.dom
        .mutate(|cx| cx.set_attribute(field.get(cx.gc), "type", "range"));
    assert_eq!(send(&mut page, &field, "30"), Ok(false));
    assert_eq!(state_value(&page, &field), "30");
    page.dom
        .mutate(|cx| cx.set_attribute(field.get(cx.gc), "readonly", ""));
    assert!(matches!(
        send(&mut page, &field, "40"),
        Err(dom_api::WebDriverActionError::NotInteractable(_))
    ));
    page.dom
        .mutate(|cx| cx.set_attribute(field.get(cx.gc), "type", "file"));
    assert!(matches!(
        send(&mut page, &field, "f"),
        Err(dom_api::WebDriverActionError::Unsupported(_))
    ));
    // One with no box can't take the focus: not interactable.
    let other_id = id_of(&page, &other);
    fake(&page.dom, |fake| {
        fake.boxes.remove(&other_id);
    });
    assert!(matches!(
        send(&mut page, &other, "x"),
        Err(dom_api::WebDriverActionError::NotInteractable(_))
    ));
    // Clear: emptied, `input` and `change`, the focus taken and given up.
    events(&page);
    set_value(&mut page, &area, "text");
    let area_id = id_of(&page, &area);
    fake(&page.dom, |fake| {
        fake.rects
            .insert(area_id, vec![gfx::RectF::new(0.0, 0.0, 10.0, 10.0)]);
    });
    let clear = |page: &mut Page, node: &Rc<NodeRoot>| {
        let element = id_of(page, node);
        with_host(&page.dom, |host| {
            dom_api::webdriver_clear(host, &document, element)
        })
    };
    assert_eq!(clear(&mut page, &area), Ok(()));
    assert_eq!(state_value(&page, &area), "");
    // (The input that had the focus loses it first.)
    assert_eq!(events(&page), ["blur", "input", "change", "blur"]);
    // Empty: nothing.
    assert_eq!(clear(&mut page, &area), Ok(()));
    assert_eq!(events(&page), Vec::<String>::new());
    // Not editable: invalid element state (a read-only one, a div).
    page.dom
        .mutate(|cx| cx.set_attribute(area.get(cx.gc), "readonly", ""));
    assert!(matches!(
        clear(&mut page, &area),
        Err(dom_api::WebDriverActionError::InvalidElementState(_))
    ));
    assert!(matches!(
        clear(&mut page, &other),
        Err(dom_api::WebDriverActionError::InvalidElementState(_))
    ));
    // Out of view: not interactable.
    page.dom
        .mutate(|cx| cx.remove_attribute(area.get(cx.gc), "readonly"));
    set_value(&mut page, &area, "text");
    fake(&page.dom, |fake| {
        fake.rects
            .insert(area_id, vec![gfx::RectF::new(0.0, 5000.0, 10.0, 10.0)]);
    });
    assert!(matches!(
        clear(&mut page, &area),
        Err(dom_api::WebDriverActionError::NotInteractable(_))
    ));
}

fn state_value(page: &Page, element: &Rc<NodeRoot>) -> String {
    page.dom.read(|cx| {
        cx.text_control_value(element.get(cx.gc))
            .to_str()
            .into_owned()
    })
}
