//! The form controls' default actions besides typing, through the user's
//! input (`EventHandler`), the frame the tests' layout (`geometry::Fake`):
//! Space clicking a check box, a radio button and a button (on its
//! release, on the element it went down on), Enter a button; a number's
//! ↑ and ↓, a range's keys, a search field's Escape; a number's spin
//! button held (its steps, its repeat by the window's native timer, the
//! half the pointer is in, its release's commit) and a search field's
//! clear button; a menu list's keys, its popup asked for and closed with
//! the user's pick, `showPicker()` and the user activation it needs.

use std::rc::Rc;

use dom::NodeRoot;
use dom_api::{EventHandler, KeyInput, MemberKind, MouseAction, MouseButton, MouseInput};
use gfx::{NodeId, PointF, RectF};

use crate::geometry::{fake, id, page};
use crate::{Action, Arg, TestDom, This, call, call_on, with_host};

struct Page {
    document: Rc<NodeRoot>,
    controls: Vec<Rc<NodeRoot>>,
    window: elise::Persistent<dom::Window>,
    handler: EventHandler,
    dom: TestDom,
}

const TYPES: [&str; 6] = ["click", "input", "change", "search", "keyup", "mouseup"];

/// A page of inputs of `types` (`button` is a `<button>`, `select` a
/// `<select>`), all with
/// boxes, the page focused; `click`, `input`, `change`, `search` and
/// `keyup` listened to at the document.
fn controls_page(types: &[&str]) -> Page {
    let locals: Vec<&str> = types
        .iter()
        .map(|ty| match *ty {
            "button" | "select" => *ty,
            _ => "input",
        })
        .collect();
    let (mut dom, nodes, _) = page(false, &locals);
    let document = nodes[0].clone();
    let controls: Vec<Rc<NodeRoot>> = nodes[3..].to_vec();
    let window = dom.mutate(|cx| {
        for (control, ty) in controls.iter().zip(types) {
            if *ty != "button" && *ty != "select" {
                cx.set_attribute(control.get(cx.gc), "type", ty);
            }
        }
        let window = cx.create_window(document.get(cx.gc));
        cx.set_page_focused(window, true);
        for control in &controls {
            cx.ensure_shadow_tree(control.get(cx.gc));
        }
        elise::Persistent::new(cx.gc, window)
    });
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        for &node in &ids[1..] {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
    });
    for (index, event_type) in TYPES.iter().enumerate() {
        listen(&mut dom, &document, event_type, index as u64 + 1);
    }
    Page {
        document,
        controls,
        window,
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

/// The events since the last call, `click`'s with whether it is trusted.
fn events(page: &Page) -> Vec<String> {
    page.dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback <= 6)
        .map(|call| {
            if call.event_type == "click" {
                format!("click {}", call.trusted)
            } else {
                call.event_type
            }
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
}

fn key(page: &mut Page, down: bool, name: &str, text: &str) {
    key_with(page, down, name, text, 0);
}

fn key_with(page: &mut Page, down: bool, name: &str, text: &str, modifiers: u16) {
    let input = KeyInput {
        down,
        key: name.to_owned(),
        code: name.to_owned(),
        key_code: 0,
        text: text.to_owned(),
        location: 0,
        repeat: false,
        modifiers,
    };
    let document = page.document.clone();
    let handler = &mut page.handler;
    with_host(&page.dom, |host| {
        handler.handle_key(host, &document, &input)
    });
}

fn press(page: &mut Page, name: &str) {
    key(page, true, name, "");
    key(page, false, name, "");
}

fn mouse(page: &mut Page, action: MouseAction, at: NodeId, point: PointF) {
    mouse_with(page, action, at, point, 0);
}

fn mouse_with(page: &mut Page, action: MouseAction, at: NodeId, point: PointF, modifiers: u16) {
    fake(&page.dom, |fake| fake.at = Some(at));
    let input = MouseInput {
        action,
        point,
        buttons: u16::from(matches!(
            action,
            MouseAction::Down { .. } | MouseAction::Move
        )),
        modifiers,
    };
    let document = page.document.clone();
    let handler = &mut page.handler;
    with_host(&page.dom, |host| {
        handler.handle_mouse(host, &document, &input)
    });
}

fn value(page: &Page, element: &Rc<NodeRoot>) -> String {
    page.dom
        .read(|cx| cx.input_value(element.get(cx.gc)).to_str().into_owned())
}

fn checked(page: &Page, element: &Rc<NodeRoot>) -> bool {
    page.dom.read(|cx| cx.checkedness(element.get(cx.gc)))
}

#[test]
fn keys_click_check_boxes_and_buttons() {
    let mut page = controls_page(&["checkbox", "radio", "submit", "button"]);
    let [checkbox, radio, submit, button] = page.controls.clone().try_into().unwrap();
    focus(&mut page, &checkbox);
    events(&page);
    // Space's release clicks, trusted.
    key(&mut page, true, " ", " ");
    assert!(!checked(&page, &checkbox), "not on the press");
    key(&mut page, false, " ", "");
    assert!(checked(&page, &checkbox));
    assert_eq!(events(&page), ["keyup", "click true", "input", "change"]);
    // Not if the focus moved between the press and the release.
    key(&mut page, true, " ", " ");
    focus(&mut page, &radio);
    key(&mut page, false, " ", "");
    assert!(checked(&page, &checkbox) && !checked(&page, &radio));
    // A canceled keyup clicks nothing.
    page.dom.1.actions.borrow_mut().insert(20, Action::Cancel);
    listen(&mut page.dom, &page.document.clone(), "keyup", 20);
    press(&mut page, " ");
    assert!(!checked(&page, &radio));
    page.dom.1.actions.borrow_mut().insert(20, Action::Nothing);
    press(&mut page, " ");
    assert!(checked(&page, &radio));
    // Enter clicks a button, not a check box (implicit submission).
    focus(&mut page, &checkbox);
    events(&page);
    key(&mut page, true, "Enter", "\r");
    assert!(checked(&page, &checkbox));
    focus(&mut page, &button);
    events(&page);
    key(&mut page, true, "Enter", "\r");
    assert_eq!(events(&page), ["click true"]);
    focus(&mut page, &submit);
    events(&page);
    press(&mut page, " ");
    assert_eq!(events(&page), ["keyup", "click true"]);
}

#[test]
fn a_numbers_and_a_ranges_keys() {
    let mut page = controls_page(&["number", "range", "number", "number", "number", "range"]);
    let [number, range, read_only, based, over, plain] = page.controls.clone().try_into().unwrap();
    page.dom.mutate(|cx| {
        cx.set_attribute(number.get(cx.gc), "max", "2");
        cx.set_attribute(range.get(cx.gc), "step", "10");
        cx.set_attribute(read_only.get(cx.gc), "readonly", "");
    });
    // An empty number goes to 1; up to its maximum, `input` then `change`
    // (committed at once), nothing past it.
    focus(&mut page, &number);
    events(&page);
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(value(&page, &number), "1");
    assert_eq!(events(&page), ["input", "change"]);
    key(&mut page, true, "ArrowUp", "");
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(value(&page, &number), "2");
    events(&page);
    key(&mut page, true, "ArrowDown", "");
    assert_eq!(value(&page, &number), "1");
    // A read-only one doesn't step.
    focus(&mut page, &read_only);
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(value(&page, &read_only), "");
    // The step base is the `value` attribute without a `min`.
    page.dom
        .mutate(|cx| cx.set_attribute(based.get(cx.gc), "value", "0.5"));
    focus(&mut page, &based);
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(value(&page, &based), "1.5");
    // Above the maximum, down goes to it (not to the step below it).
    page.dom.mutate(|cx| {
        let over = over.get(cx.gc);
        cx.set_attribute(over, "max", "10");
        cx.set_attribute(over, "step", "3");
        cx.set_input_value(over, "20").unwrap();
    });
    focus(&mut page, &over);
    key(&mut page, true, "ArrowDown", "");
    assert_eq!(value(&page, &over), "10");
    // At the maximum, up does nothing, though the maximum isn't aligned
    // (stepping on would align it down to 9).
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(value(&page, &over), "10");
    // A range's page keys: a tenth of its range.
    focus(&mut page, &plain);
    key(&mut page, true, "PageUp", "");
    assert_eq!(value(&page, &plain), "60");
    // Disabled while it has the focus: no keys.
    page.dom
        .mutate(|cx| cx.set_attribute(plain.get(cx.gc), "disabled", ""));
    key(&mut page, true, "ArrowRight", "");
    assert_eq!(value(&page, &plain), "60");
    // A range by its step, a tenth of its range with the page keys, to
    // its ends with Home and End; `input` and `change` each time it moves.
    focus(&mut page, &range);
    events(&page);
    key(&mut page, true, "ArrowRight", "");
    assert_eq!(value(&page, &range), "60");
    assert_eq!(events(&page), ["input", "change"]);
    key(&mut page, true, "ArrowDown", "");
    key(&mut page, true, "ArrowLeft", "");
    assert_eq!(value(&page, &range), "40");
    key(&mut page, true, "End", "");
    assert_eq!(value(&page, &range), "100");
    events(&page);
    key(&mut page, true, "PageUp", "");
    assert_eq!(events(&page), Vec::<String>::new(), "at the end: no events");
    key(&mut page, true, "Home", "");
    key(&mut page, true, "PageUp", "");
    assert_eq!(value(&page, &range), "10");
}

#[test]
fn a_search_fields_escape_and_clear_button() {
    let mut page = controls_page(&["search"]);
    let [search] = page.controls.clone().try_into().unwrap();
    page.dom
        .mutate(|cx| cx.set_input_value(search.get(cx.gc), "query").unwrap());
    focus(&mut page, &search);
    events(&page);
    // Escape empties it for the user (`input`, its change waiting for the
    // commit) and fires `search`; empty, Escape does nothing.
    key(&mut page, true, "Escape", "");
    assert_eq!(value(&page, &search), "");
    assert_eq!(events(&page), ["input", "search"]);
    key(&mut page, true, "Escape", "");
    assert_eq!(events(&page), Vec::<String>::new());
    // The clear button's click.
    page.dom
        .mutate(|cx| cx.set_input_value(search.get(cx.gc), "again").unwrap());
    let cancel = page.dom.mutate(|cx| {
        let root = cx.user_agent_shadow_root(search.get(cx.gc)).unwrap();
        let container = cx.first_child(root).unwrap();
        cx.node_id(cx.last_child(container).unwrap())
    });
    events(&page);
    let primary = MouseButton::Primary;
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        cancel,
        PointF::new(1.0, 1.0),
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        cancel,
        PointF::new(1.0, 1.0),
    );
    assert_eq!(value(&page, &search), "");
    assert_eq!(events(&page), ["mouseup", "click true", "input", "search"]);
    // Read-only: nothing.
    page.dom.mutate(|cx| {
        cx.set_input_value(search.get(cx.gc), "kept").unwrap();
        cx.set_attribute(search.get(cx.gc), "readonly", "");
    });
    key(&mut page, true, "Escape", "");
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        cancel,
        PointF::new(1.0, 1.0),
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        cancel,
        PointF::new(1.0, 1.0),
    );
    assert_eq!(value(&page, &search), "kept");
}

#[test]
fn a_spin_button_held() {
    let mut page = controls_page(&["number"]);
    let [number] = page.controls.clone().try_into().unwrap();
    let spin = page.dom.mutate(|cx| {
        let root = cx.user_agent_shadow_root(number.get(cx.gc)).unwrap();
        let container = cx.first_child(root).unwrap();
        cx.node_id(cx.last_child(container).unwrap())
    });
    // Its upper half is above y 10.
    fake(&page.dom, |fake| {
        fake.boxes.insert(spin, crate::geometry::block_box(spin));
        fake.rects
            .insert(spin, vec![RectF::new(0.0, 0.0, 10.0, 20.0)]);
    });
    let primary = MouseButton::Primary;
    let upper = PointF::new(5.0, 5.0);
    events(&page);
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        spin,
        upper,
    );
    // The press focuses the field and steps it up at once.
    assert_eq!(value(&page, &number), "1");
    let active = page.dom.read(|cx| {
        cx.focused_element(page.document.get(cx.gc))
            .map(|f| cx.node_id(f))
    });
    assert_eq!(active, Some(id(&page.dom, &number)));
    assert_eq!(events(&page), ["input"]);
    // Held: again after 250 ms, then every 50 ms, by the window's timers
    // (the host's clock: milliseconds from the press), which the next due
    // time counts.
    let window = page.window.clone();
    let pressed = page.dom.read(|cx| cx.now_since(Some(window.get(cx.gc))));
    let due = page.dom.read(|cx| cx.next_due(window.get(cx.gc))).unwrap();
    assert!((due - pressed - 250.0).abs() < 50.0, "{due} {pressed}");
    let run = |page: &mut Page, at: f64| {
        with_host(&page.dom, |host| {
            dom_api::run_due(host, &window, pressed + at)
        })
    };
    assert_eq!(run(&mut page, 100.0), 0, "not yet");
    assert_eq!(run(&mut page, 260.0), 1);
    assert_eq!(value(&page, &number), "2");
    assert_eq!(run(&mut page, 300.0), 0);
    assert_eq!(run(&mut page, 320.0), 1);
    assert_eq!(value(&page, &number), "3");
    // The pointer in the lower half: no steps (Windows' and Linux's).
    mouse(&mut page, MouseAction::Move, spin, PointF::new(5.0, 15.0));
    run(&mut page, 380.0);
    if !cfg!(target_os = "macos") {
        assert_eq!(value(&page, &number), "3");
    }
    // Released: the change committed; the repeat stops.
    events(&page);
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        spin,
        upper,
    );
    assert!(events(&page).contains(&"change".to_owned()));
    let before = value(&page, &number);
    run(&mut page, 2000.0);
    assert_eq!(value(&page, &number), before);
    // The lower half steps down.
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        spin,
        PointF::new(5.0, 15.0),
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        spin,
        PointF::new(5.0, 15.0),
    );
    let after = (before.parse::<i32>().unwrap() - 1).to_string();
    assert_eq!(value(&page, &number), after);
    // Read-only: the press does nothing.
    page.dom
        .mutate(|cx| cx.set_attribute(number.get(cx.gc), "readonly", ""));
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        spin,
        upper,
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        spin,
        upper,
    );
    assert_eq!(value(&page, &number), after);
}

/// `select`'s options of `texts` (a `-` before one disables it).
fn add_options(page: &mut Page, select: &Rc<NodeRoot>, texts: &[&str]) {
    page.dom.mutate(|cx| {
        let select = select.get(cx.gc);
        let document = cx.document_of(select);
        for text in texts {
            let option = cx.create_element(document, dom::QualName::html("option"));
            let text = match text.strip_prefix('-') {
                Some(text) => {
                    cx.set_attribute(option, "disabled", "");
                    text
                }
                None => text,
            };
            cx.append_child(option, cx.create_text(document, text))
                .unwrap();
            cx.append_child(select, option).unwrap();
        }
    });
}

fn selected_text(page: &Page, select: &Rc<NodeRoot>) -> String {
    page.dom.read(|cx| {
        cx.first_selected_option(select.get(cx.gc))
            .map(|option| cx.option_text(option))
            .unwrap_or_default()
    })
}

/// What the selects asked of their popups since, and whether `select`'s is
/// open now.
fn popups(page: &mut Page, select: &Rc<NodeRoot>) -> (Vec<&'static str>, bool) {
    page.dom.mutate(|cx| {
        let requests = cx
            .take_popup_requests()
            .into_iter()
            .map(|request| match request {
                dom::PopupRequest::Show(_) => "show",
                dom::PopupRequest::Hide(_) => "hide",
            })
            .collect();
        (requests, cx.select_popup_open(select.get(cx.gc)))
    })
}

fn closed(page: &mut Page, select: &Rc<NodeRoot>, result: dom_api::PopupResult) {
    with_host(&page.dom, |host| {
        dom_api::popup_menu_closed(host, select, result)
    });
}

#[test]
fn a_menu_lists_keys() {
    let mut page = controls_page(&["select"]);
    let select = page.controls[0].clone();
    add_options(&mut page, &select, &["a", "-b", "c", "d"]);
    focus(&mut page, &select);
    events(&page);
    // The next that isn't disabled, `input` and `change`.
    key(&mut page, true, "ArrowDown", "");
    assert_eq!(
        (selected_text(&page, &select), events(&page)),
        (
            "c".to_owned(),
            vec!["input".to_owned(), "change".to_owned()]
        )
    );
    key(&mut page, true, "End", "");
    key(&mut page, true, "ArrowRight", "");
    assert_eq!(selected_text(&page, &select), "d");
    assert_eq!(
        events(&page),
        ["input", "change"],
        "past the last: no change"
    );
    key(&mut page, true, "Home", "");
    assert_eq!(selected_text(&page, &select), "a");
    key(&mut page, true, "ArrowRight", "");
    assert_eq!(selected_text(&page, &select), "c");
    key(&mut page, true, "Home", "");
    key(&mut page, true, "PageDown", "");
    assert_eq!(selected_text(&page, &select), "d", "three away");
    key(&mut page, true, "ArrowUp", "");
    key(&mut page, true, "ArrowLeft", "");
    assert_eq!(
        selected_text(&page, &select),
        "a",
        "the disabled one passed"
    );
    // With a modifier, and on a disabled select: nothing.
    events(&page);
    key_with(&mut page, true, "ArrowDown", "", dom::modifiers::SHIFT);
    page.dom
        .mutate(|cx| cx.set_attribute(select.get(cx.gc), "disabled", ""));
    key(&mut page, true, "ArrowDown", "");
    assert_eq!(
        (selected_text(&page, &select), events(&page)),
        ("a".to_owned(), Vec::<String>::new())
    );
}

#[test]
fn a_menu_lists_popup() {
    use dom_api::PopupResult::{Accepted, Canceled};
    let mut page = controls_page(&["select", "button"]);
    let select = page.controls[0].clone();
    let button = page.controls[1].clone();
    add_options(&mut page, &select, &["a", "-b", "c"]);
    focus(&mut page, &select);
    popups(&mut page, &select);
    // Alt+↓ and F4 open it, once.
    key_with(&mut page, true, "ArrowDown", "", dom::modifiers::ALT);
    assert_eq!(popups(&mut page, &select), (vec!["show"], true));
    key(&mut page, true, "F4", "");
    assert_eq!(popups(&mut page, &select), (vec![], true));
    // The user's pick by a key: `input`, `change`, then `click`.
    events(&page);
    closed(
        &mut page,
        &select,
        Accepted {
            index: 2,
            by_keyboard: true,
        },
    );
    assert_eq!(selected_text(&page, &select), "c");
    assert_eq!(events(&page), ["input", "change", "click true"]);
    assert_eq!(popups(&mut page, &select), (vec![], false));
    // Space opens it; a pick by the pointer of a disabled one changes
    // nothing, `mouseup` and `click` still.
    key(&mut page, true, " ", " ");
    assert_eq!(popups(&mut page, &select), (vec!["show"], true));
    events(&page);
    closed(
        &mut page,
        &select,
        Accepted {
            index: 1,
            by_keyboard: false,
        },
    );
    assert_eq!(
        (selected_text(&page, &select), events(&page)),
        (
            "c".to_owned(),
            vec!["mouseup".to_owned(), "click true".to_owned()]
        )
    );
    // The one selected picked: no `input` or `change`.
    key(&mut page, true, "F4", "");
    popups(&mut page, &select);
    closed(
        &mut page,
        &select,
        Accepted {
            index: 2,
            by_keyboard: true,
        },
    );
    assert_eq!(events(&page), ["click true"]);
    // Closed with no pick: nothing; one not open: nothing either.
    key(&mut page, true, "Enter", "\r");
    assert_eq!(popups(&mut page, &select), (vec!["show"], true));
    closed(&mut page, &select, Canceled);
    closed(
        &mut page,
        &select,
        Accepted {
            index: 0,
            by_keyboard: true,
        },
    );
    assert_eq!(
        (selected_text(&page, &select), events(&page)),
        ("c".to_owned(), Vec::<String>::new())
    );
    // The primary button's press opens it, and closes it open.
    let at = id(&page.dom, &select);
    let primary = MouseButton::Primary;
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
    );
    assert_eq!(popups(&mut page, &select), (vec!["show"], true));
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
    );
    assert_eq!(popups(&mut page, &select), (vec!["hide"], false));
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
    );
    // The focus leaving closes it.
    key(&mut page, true, "F4", "");
    focus(&mut page, &button);
    assert_eq!(popups(&mut page, &select), (vec!["show", "hide"], false));
    // Its items: an option's label, a disabled one, an optgroup, an `<hr>`;
    // the selected one's.
    page.dom.mutate(|cx| {
        let node = select.get(cx.gc);
        let document = cx.document_of(node);
        let group = cx.create_element(document, dom::QualName::html("optgroup"));
        cx.set_attribute(group, "label", " G  g ");
        cx.set_attribute(group, "disabled", "");
        cx.append_child(node, group).unwrap();
        cx.append_child(node, cx.create_element(document, dom::QualName::html("hr")))
            .unwrap();
    });
    let items = page
        .dom
        .read(|cx| dom_api::popup_items(&dom_api::ApiCx::new(cx), select.get(cx.gc)));
    let described: Vec<String> = items
        .0
        .iter()
        .map(|item| format!("{:?} {} {}", item.kind, item.label, item.enabled))
        .collect();
    assert_eq!(
        (described, items.1),
        (
            vec![
                "Option a true".to_owned(),
                "Option b false".to_owned(),
                "Option c true".to_owned(),
                "Group G g false".to_owned(),
                "Separator  false".to_owned(),
            ],
            Some(2)
        )
    );
    // A disabled select can't keep it.
    let keeps = |page: &Page| {
        page.dom
            .read(|cx| dom_api::can_keep_popup(&dom_api::ApiCx::new(cx), select.get(cx.gc)))
    };
    assert!(keeps(&page));
    page.dom
        .mutate(|cx| cx.set_attribute(select.get(cx.gc), "disabled", ""));
    assert!(!keeps(&page));
    page.dom
        .mutate(|cx| cx.remove_attribute(select.get(cx.gc), "disabled"));
    // With `multiple`, none.
    focus(&mut page, &select);
    page.dom.mutate(|cx| {
        cx.set_attribute(select.get(cx.gc), "multiple", "");
        cx.set_attribute(select.get(cx.gc), "size", "1");
    });
    key(&mut page, true, "F4", "");
    assert_eq!(popups(&mut page, &select), (vec![], false));
}

#[test]
fn show_picker_needs_the_users_activation() {
    let mut page = controls_page(&["select"]);
    let select = page.controls[0].clone();
    add_options(&mut page, &select, &["a"]);
    let show = |page: &mut Page| {
        call(
            &mut page.dom,
            &select,
            "HTMLSelectElement",
            "showPicker",
            MemberKind::Method,
            &[],
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    };
    let not_allowed = Err("NotAllowedError: showPicker() requires a user gesture.".to_owned());
    assert_eq!(show(&mut page), not_allowed);
    // A key's press activates the window; the picker uses it up.
    focus(&mut page, &select);
    press(&mut page, "x");
    assert_eq!(show(&mut page), Ok(()));
    assert_eq!(popups(&mut page, &select), (vec!["show"], true));
    assert_eq!(show(&mut page), not_allowed);
    // Escape doesn't activate it.
    press(&mut page, "Escape");
    assert_eq!(show(&mut page), not_allowed);
    // A press does.
    let at = id(&page.dom, &select);
    let primary = MouseButton::Primary;
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
    );
    popups(&mut page, &select);
    with_host(&page.dom, |host| {
        dom_api::popup_menu_closed(host, &select, dom_api::PopupResult::Canceled)
    });
    assert_eq!(show(&mut page), Ok(()));
    // A disabled one is an `InvalidStateError`.
    press(&mut page, "x");
    page.dom
        .mutate(|cx| cx.set_attribute(select.get(cx.gc), "disabled", ""));
    assert_eq!(
        show(&mut page),
        Err("InvalidStateError: showPicker() cannot be used on immutable controls.".to_owned())
    );
}

/// The selected options' texts, joined.
fn picked(page: &Page, select: &Rc<NodeRoot>) -> String {
    page.dom.read(|cx| {
        cx.list_of_options(select.get(cx.gc))
            .into_iter()
            .filter(|option| cx.option_selectedness(*option))
            .map(|option| cx.option_text(option))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

/// A list box of `size` rows, `multiple` or not, of `texts`' options;
/// focused, its events since dropped.
fn list_box_page(multiple: bool, texts: &[&str]) -> (Page, Rc<NodeRoot>, Vec<NodeId>) {
    let mut page = controls_page(&["select"]);
    let select = page.controls[0].clone();
    page.dom.mutate(|cx| {
        cx.set_attribute(select.get(cx.gc), "size", "3");
        if multiple {
            cx.set_attribute(select.get(cx.gc), "multiple", "");
        }
    });
    add_options(&mut page, &select, texts);
    let options = page.dom.mutate(|cx| {
        cx.list_of_options(select.get(cx.gc))
            .into_iter()
            .map(|option| cx.node_id(option))
            .collect()
    });
    focus(&mut page, &select);
    events(&page);
    (page, select, options)
}

fn press_at(page: &mut Page, at: NodeId, modifiers: u16) {
    let primary = MouseButton::Primary;
    mouse_with(
        page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
        modifiers,
    );
    mouse_with(
        page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        at,
        PointF::new(5.0, 5.0),
        modifiers,
    );
}

fn scrolls(page: &Page) -> Vec<NodeId> {
    let mut taken = Vec::new();
    fake(&page.dom, |fake| {
        taken = std::mem::take(&mut fake.scrolls)
            .into_iter()
            .map(|(_, element)| element)
            .collect()
    });
    taken
}

#[test]
fn a_list_boxs_presses() {
    use dom::modifiers::{CTRL, META, SHIFT};
    let adds = if cfg!(target_os = "macos") {
        META
    } else {
        CTRL
    };
    let (mut page, select, options) = list_box_page(true, &["a", "b", "-c", "d", "e"]);
    // A press selects one alone; `input` and `change` on the release.
    press_at(&mut page, options[1], 0);
    assert_eq!(picked(&page, &select), "b");
    // Dirty: its `selected` attribute no longer does.
    page.dom.mutate(|cx| {
        let b = cx.node_by_id(options[1]).unwrap();
        cx.set_attribute(b, "selected", "");
        cx.remove_attribute(b, "selected");
    });
    assert_eq!(picked(&page, &select), "b");
    assert_eq!(events(&page), ["mouseup", "input", "change", "click true"]);
    assert_eq!(scrolls(&page), [options[1]]);
    // Shift: the range from it (the disabled one left out).
    press_at(&mut page, options[3], SHIFT);
    assert_eq!(picked(&page, &select), "b d");
    // Ctrl: one more, one less.
    press_at(&mut page, options[0], adds);
    assert_eq!(picked(&page, &select), "a b d");
    press_at(&mut page, options[1], adds);
    assert_eq!(picked(&page, &select), "a d");
    events(&page);
    // The same again: no `input` or `change`; a disabled one: nothing.
    press_at(&mut page, options[4], 0);
    events(&page);
    press_at(&mut page, options[4], 0);
    press_at(&mut page, options[2], 0);
    assert_eq!(
        (picked(&page, &select), events(&page)),
        (
            "e".to_owned(),
            vec![
                "mouseup".to_owned(),
                "click true".to_owned(),
                "mouseup".to_owned(),
                "click true".to_owned()
            ]
        )
    );
    // A drag moves the range's end.
    let primary = MouseButton::Primary;
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        options[0],
        PointF::new(5.0, 5.0),
    );
    mouse(
        &mut page,
        MouseAction::Move,
        options[1],
        PointF::new(5.0, 15.0),
    );
    assert_eq!(picked(&page, &select), "a b");
    mouse(
        &mut page,
        MouseAction::Move,
        options[3],
        PointF::new(5.0, 35.0),
    );
    assert_eq!(picked(&page, &select), "a b d");
    mouse(
        &mut page,
        MouseAction::Move,
        options[0],
        PointF::new(5.0, 5.0),
    );
    assert_eq!(picked(&page, &select), "a", "back: the others as they were");
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        options[0],
        PointF::new(5.0, 5.0),
    );
    // A move with no button held isn't a drag.
    fake(&page.dom, |fake| fake.at = Some(options[3]));
    let document = page.document.clone();
    let handler = &mut page.handler;
    with_host(&page.dom, |host| {
        handler.handle_mouse(
            host,
            &document,
            &MouseInput {
                action: MouseAction::Move,
                point: PointF::new(5.0, 35.0),
                buttons: 0,
                modifiers: 0,
            },
        );
    });
    assert_eq!(picked(&page, &select), "a");
    // Its focus lost: a drag over it after a press elsewhere changes
    // nothing, nor does a release.
    call(
        &mut page.dom,
        &select,
        "HTMLElement",
        "blur",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    events(&page);
    mouse(
        &mut page,
        MouseAction::Move,
        options[3],
        PointF::new(5.0, 35.0),
    );
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        options[3],
        PointF::new(5.0, 35.0),
    );
    assert_eq!(
        (picked(&page, &select), events(&page)),
        ("a".to_owned(), vec!["mouseup".to_owned()])
    );
}

#[test]
fn a_list_box_without_multiple() {
    let (mut page, select, options) = list_box_page(false, &["a", "b", "c"]);
    use dom::modifiers::SHIFT;
    press_at(&mut page, options[0], SHIFT);
    press_at(&mut page, options[2], SHIFT);
    assert_eq!(picked(&page, &select), "c", "one at a time");
    // A drag moves the one selected.
    let primary = MouseButton::Primary;
    mouse(
        &mut page,
        MouseAction::Down {
            button: primary,
            click_count: 1,
        },
        options[0],
        PointF::new(5.0, 5.0),
    );
    mouse(
        &mut page,
        MouseAction::Move,
        options[1],
        PointF::new(5.0, 15.0),
    );
    assert_eq!(picked(&page, &select), "b");
    mouse(
        &mut page,
        MouseAction::Up {
            button: primary,
            click_count: 1,
        },
        options[1],
        PointF::new(5.0, 15.0),
    );
    // Keys: the next, the one before, Home, End, a page away.
    events(&page);
    key(&mut page, true, "ArrowDown", "");
    assert_eq!(
        (picked(&page, &select), events(&page)),
        (
            "c".to_owned(),
            vec!["input".to_owned(), "change".to_owned()]
        )
    );
    key(&mut page, true, "Home", "");
    assert_eq!(picked(&page, &select), "a");
    key(&mut page, true, "PageDown", "");
    assert_eq!(picked(&page, &select), "c", "size − 1 away");
    key(&mut page, true, "PageUp", "");
    assert_eq!(picked(&page, &select), "a");
    key(&mut page, true, "End", "");
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(picked(&page, &select), "b");
    // A disabled list box: nothing.
    page.dom
        .mutate(|cx| cx.set_attribute(select.get(cx.gc), "disabled", ""));
    key(&mut page, true, "ArrowDown", "");
    press_at(&mut page, options[0], 0);
    assert_eq!(picked(&page, &select), "b");
}

#[test]
fn a_list_boxs_keys_with_multiple() {
    use dom::modifiers::{CTRL, META, SHIFT};
    let adds = if cfg!(target_os = "macos") {
        META
    } else {
        CTRL
    };
    let (mut page, select, options) = list_box_page(true, &["a", "b", "c", "d"]);
    let focused = |page: &Page| -> Vec<bool> {
        page.dom.read(|cx| {
            cx.list_of_options(select.get(cx.gc))
                .into_iter()
                .map(|o| cx.matches_multi_select_focus(o))
                .collect()
        })
    };
    // With none selected, ↓ goes to the first.
    key(&mut page, true, "ArrowDown", "");
    assert_eq!(picked(&page, &select), "a");
    // Shift: the range from the anchor.
    key_with(&mut page, true, "ArrowDown", "", SHIFT);
    key_with(&mut page, true, "ArrowDown", "", SHIFT);
    assert_eq!(picked(&page, &select), "a b c");
    assert_eq!(scrolls(&page).last(), Some(&options[2]));
    // Ctrl: the end alone moves, `:-internal-multi-select-focus` on it;
    // Space (its `keypress`) and Ctrl+Space toggle it.
    events(&page);
    key_with(&mut page, true, "ArrowDown", "", adds);
    assert_eq!(
        (picked(&page, &select), events(&page)),
        ("a b c".to_owned(), Vec::<String>::new())
    );
    assert_eq!(focused(&page), [false, false, false, true]);
    key(&mut page, true, " ", " ");
    assert_eq!(picked(&page, &select), "a b c d");
    assert_eq!(events(&page), ["input", "change"]);
    key_with(&mut page, true, "ArrowUp", "", adds);
    key_with(&mut page, true, " ", "", adds);
    assert_eq!(picked(&page, &select), "a b d");
    // A disabled end isn't it.
    key_with(&mut page, true, "ArrowDown", "", adds);
    assert_eq!(focused(&page), [false, false, false, true]);
    page.dom
        .mutate(|cx| cx.set_attribute(cx.node_by_id(options[3]).unwrap(), "disabled", ""));
    assert_eq!(focused(&page), [false; 4]);
    page.dom
        .mutate(|cx| cx.remove_attribute(cx.node_by_id(options[3]).unwrap(), "disabled"));
    key_with(&mut page, true, "ArrowUp", "", adds);
    // Without Ctrl, the selection starts again: a disabled one selected
    // by script unselected too.
    page.dom.mutate(|cx| {
        let a = cx.node_by_id(options[0]).unwrap();
        cx.set_attribute(a, "disabled", "");
        cx.set_option_selected_dirty(a, true);
    });
    key(&mut page, true, "ArrowUp", "");
    assert_eq!(picked(&page, &select), "b");
    assert_eq!(focused(&page), [false; 4]);
    page.dom
        .mutate(|cx| cx.remove_attribute(cx.node_by_id(options[0]).unwrap(), "disabled"));
    // The anchor taken out: Shift's range starts at the end it goes to.
    key_with(&mut page, true, "ArrowDown", "", SHIFT);
    assert_eq!(picked(&page, &select), "b c");
    page.dom
        .mutate(|cx| cx.remove(cx.node_by_id(options[1]).unwrap()));
    key_with(&mut page, true, "ArrowDown", "", SHIFT);
    assert_eq!(picked(&page, &select), "c d");
    // Space without Ctrl's move: nothing; Enter taken.
    key(&mut page, true, " ", " ");
    assert_eq!(picked(&page, &select), "c d");
}
