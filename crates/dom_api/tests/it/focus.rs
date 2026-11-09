//! Focus through its members and the host's calls (`set_page_focused`,
//! `focus_fixup`), the frame the tests' layout (`geometry::Fake`): what
//! can take it, the events and their related targets, `activeElement`,
//! the page's focus, `focusVisible`, a listener taking the element out,
//! the fixup, an element taken out, `tabIndex`; a form control's (not a
//! disabled one), from a text control's `select()` and its label's click.

use std::rc::Rc;

use dom::{NodeRoot, ObjectRef, ObjectRoot};
use dom_api::MemberKind;
use gfx::NodeId;

use crate::geometry::{append, fake, id, page};
use crate::{Action, Arg, Ret, TestDom, This, call, call_on, with_host};

const TYPES: [&str; 6] = [
    "focus",
    "blur",
    "focusin",
    "focusout",
    "DOMFocusIn",
    "DOMFocusOut",
];

struct Page {
    document: Rc<NodeRoot>,
    body: Rc<NodeRoot>,
    /// `<div tabindex=0>`, `<div tabindex=-1>`, `<a href>`, `<div>`.
    a: Rc<NodeRoot>,
    b: Rc<NodeRoot>,
    link: Rc<NodeRoot>,
    plain: Rc<NodeRoot>,
    window: elise::Persistent<dom::Window>,
    window_object: Rc<ObjectRoot>,
    /// Last: fields drop in order, and the roots above go before its heap.
    dom: TestDom,
}

/// A page whose elements all have boxes, its window's page focused if
/// `focused`, the focus events listened to at the document (capturing:
/// `focus` and `blur` don't bubble) and the window.
fn focus_page(focused: bool) -> Page {
    let (mut dom, nodes, _) = page(false, &["div", "div", "a", "div"]);
    let [document, _, body, a, b, link, plain] = nodes.as_slice() else {
        unreachable!()
    };
    let (window, window_object) = dom.mutate(|cx| {
        cx.set_attribute(a.get(cx.gc), "tabindex", "0");
        cx.set_attribute(b.get(cx.gc), "tabindex", "-1");
        cx.set_attribute(link.get(cx.gc), "href", "#");
        let window = cx.create_window(document.get(cx.gc));
        cx.set_page_focused(window, focused);
        (
            elise::Persistent::new(cx.gc, window),
            Rc::new(cx.root_object(ObjectRef::Window(window))),
        )
    });
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        for &node in &ids[1..] {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
    });
    for (index, &event_type) in TYPES.iter().enumerate() {
        listen(
            &mut dom,
            This::Node(document.clone()),
            event_type,
            index as u64 + 1,
            true,
        );
    }
    for event_type in ["focus", "blur"] {
        listen(
            &mut dom,
            This::Object(window_object.clone()),
            event_type,
            7,
            false,
        );
    }
    Page {
        dom,
        document: document.clone(),
        body: body.clone(),
        a: a.clone(),
        b: b.clone(),
        link: link.clone(),
        plain: plain.clone(),
        window,
        window_object,
    }
}

fn listen(dom: &mut TestDom, target: This, event_type: &'static str, callback: u64, capture: bool) {
    let (result, _) = call_on(
        dom,
        target,
        "EventTarget",
        "addEventListener",
        MemberKind::Method,
        &[
            Arg::Str(event_type),
            Arg::Callback(callback),
            Arg::Bool(capture),
        ],
    );
    result.unwrap();
}

/// The events since the last call: type, target, related target.
fn events(page: &Page) -> Vec<String> {
    page.dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback <= 7)
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

fn focus(page: &mut Page, element: &Rc<NodeRoot>, options: Vec<(&'static str, Option<bool>)>) {
    call(
        &mut page.dom,
        element,
        "HTMLElement",
        "focus",
        MemberKind::Method,
        &[Arg::Dict(options)],
    )
    .unwrap();
}

fn blur(page: &mut Page, element: &Rc<NodeRoot>) {
    call(
        &mut page.dom,
        element,
        "HTMLElement",
        "blur",
        MemberKind::Method,
        &[],
    )
    .unwrap();
}

/// `activeElement`, as a node's name (with a tabindex's value for a div).
fn active(page: &mut Page) -> String {
    match crate::get(&mut page.dom, &page.document, "Document", "activeElement") {
        Ret::Node(node) => page.dom.read(|cx| {
            let node = node.get(cx.gc);
            let tabindex = cx.attribute_value_by_id(node, dom::names::local::TABINDEX);
            format!(
                "{}{}",
                cx.node_name(node),
                tabindex.map_or_else(String::new, |value| format!("[{}]", value.borrow()))
            )
        }),
        other => panic!("{other:?}"),
    }
}

/// `:focus`, `:focus-within`, `:focus-visible` of `element`.
fn states(page: &Page, element: &Rc<NodeRoot>) -> (bool, bool, bool) {
    page.dom.read(|cx| {
        let element = element.get(cx.gc);
        (
            cx.is_focused(element),
            cx.has_focus_within(element),
            cx.is_focus_visible(element),
        )
    })
}

#[test]
fn the_focus_moves_with_its_events() {
    let mut page = focus_page(true);
    let (a, b) = (page.a.clone(), page.b.clone());
    focus(&mut page, &a, vec![]);
    assert_eq!(
        events(&page),
        ["focus DIV", "focusin DIV", "DOMFocusIn DIV"]
    );
    assert_eq!(active(&mut page), "DIV[0]");
    assert_eq!(states(&page, &a), (true, true, true));
    let body = page.body.clone();
    assert_eq!(states(&page, &body), (false, true, false));
    focus(&mut page, &b, vec![]);
    assert_eq!(
        events(&page),
        [
            "blur DIV<DIV",
            "focusout DIV<DIV",
            "DOMFocusOut DIV<DIV",
            "focus DIV<DIV",
            "focusin DIV<DIV",
            "DOMFocusIn DIV<DIV"
        ]
    );
    assert_eq!(active(&mut page), "DIV[-1]");
    assert_eq!(states(&page, &a), (false, false, false));
    // Again: nothing.
    focus(&mut page, &b, vec![]);
    assert!(events(&page).is_empty());
    blur(&mut page, &b);
    assert_eq!(
        events(&page),
        ["blur DIV", "focusout DIV", "DOMFocusOut DIV"]
    );
    assert_eq!(active(&mut page), "BODY");
    assert_eq!(states(&page, &body), (false, false, false));
    // `blur()` of one without the focus: nothing.
    blur(&mut page, &a);
    assert!(events(&page).is_empty());
    // A link takes it; `focusVisible: false` shows no ring.
    let link = page.link.clone();
    focus(&mut page, &link, vec![("focusVisible", Some(false))]);
    assert_eq!(active(&mut page), "A");
    assert_eq!(states(&page, &link), (true, true, false));
}

/// `focusin`, `focusout` and the `DOMFocus` ones bubble (a bubbling
/// listener at the body hears them), `focus` and `blur` don't.
#[test]
fn which_focus_events_bubble() {
    let mut page = focus_page(true);
    let (a, body) = (page.a.clone(), page.body.clone());
    for (index, &event_type) in TYPES.iter().enumerate() {
        listen(
            &mut page.dom,
            This::Node(body.clone()),
            event_type,
            index as u64 + 20,
            false,
        );
    }
    focus(&mut page, &a, vec![]);
    blur(&mut page, &a);
    let heard: Vec<String> = page
        .dom
        .1
        .log
        .borrow()
        .iter()
        .filter(|call| call.callback >= 20)
        .map(|call| call.event_type.clone())
        .collect();
    assert_eq!(heard, ["focusin", "DOMFocusIn", "focusout", "DOMFocusOut"]);
}

/// Only a focusable area takes it: a valid `tabindex` or a link, with a
/// box that isn't `visibility: hidden`.
#[test]
fn what_can_take_the_focus() {
    let mut page = focus_page(true);
    let plain = page.plain.clone();
    focus(&mut page, &plain, vec![]);
    assert_eq!(active(&mut page), "BODY", "no tabindex");
    page.dom
        .mutate(|cx| cx.set_attribute(plain.get(cx.gc), "tabindex", "x"));
    focus(&mut page, &plain, vec![]);
    assert_eq!(active(&mut page), "BODY", "not an integer");
    page.dom
        .mutate(|cx| cx.set_attribute(plain.get(cx.gc), "tabindex", " +3x"));
    focus(&mut page, &plain, vec![]);
    assert_eq!(active(&mut page), "DIV[ +3x]", "HTML's integers");
    let (a, link) = (page.a.clone(), page.link.clone());
    let a_id = id(&page.dom, &a);
    fake(&page.dom, |fake| fake.hidden.push(a_id));
    focus(&mut page, &a, vec![]);
    assert_eq!(active(&mut page), "DIV[ +3x]", "visibility: hidden");
    fake(&page.dom, |fake| {
        fake.hidden.clear();
        fake.boxes.remove(&a_id);
    });
    focus(&mut page, &a, vec![]);
    assert_eq!(active(&mut page), "DIV[ +3x]", "no box");
    page.dom.mutate(|cx| {
        cx.remove_attribute(link.get(cx.gc), "href");
    });
    focus(&mut page, &link, vec![]);
    assert_eq!(active(&mut page), "DIV[ +3x]", "a link without an href");
    // An element out of the document has no box in the frame.
    let detached = page.dom.mutate(|cx| {
        let element = cx.create_element(page.document.get(cx.gc), dom::QualName::html("div"));
        cx.set_attribute(element, "tabindex", "0");
        Rc::new(cx.root(element))
    });
    focus(&mut page, &detached, vec![]);
    assert_eq!(active(&mut page), "DIV[ +3x]");
}

/// Without the system's focus the element has it with no events and
/// matches nothing; the page taking and losing it fires them, the
/// window's first in and last out.
#[test]
fn the_pages_focus() {
    let mut page = focus_page(false);
    let a = page.a.clone();
    focus(&mut page, &a, vec![]);
    assert!(events(&page).is_empty());
    assert_eq!(active(&mut page), "DIV[0]");
    assert_eq!(states(&page, &a), (false, false, false));
    let has_focus = |page: &mut Page| {
        matches!(
            call(
                &mut page.dom,
                &page.document,
                "Document",
                "hasFocus",
                MemberKind::Method,
                &[]
            )
            .unwrap(),
            Ret::Bool(true)
        )
    };
    assert!(!has_focus(&mut page));
    let window = page.window.clone();
    with_host(&page.dom, |host| {
        dom_api::set_page_focused(host, &window, true)
    });
    assert_eq!(
        events(&page),
        [
            "focus #window",
            "focus DIV",
            "focusin DIV",
            "DOMFocusIn DIV"
        ]
    );
    assert_eq!(states(&page, &a), (true, true, true));
    assert!(has_focus(&mut page));
    // Again: nothing.
    with_host(&page.dom, |host| {
        dom_api::set_page_focused(host, &window, true)
    });
    assert!(events(&page).is_empty());
    with_host(&page.dom, |host| {
        dom_api::set_page_focused(host, &window, false)
    });
    assert_eq!(
        events(&page),
        [
            "blur DIV",
            "focusout DIV",
            "DOMFocusOut DIV",
            "blur #window"
        ]
    );
    assert_eq!(states(&page, &a), (false, false, false));
    assert_eq!(active(&mut page), "DIV[0]", "it keeps the focus");
    let _ = &page.window_object;
}

/// A `focus` listener taking the element out: its other events don't go,
/// and the viewport has the focus.
#[test]
fn a_listener_takes_it_out() {
    let mut page = focus_page(true);
    let a = page.a.clone();
    page.dom.1.actions.borrow_mut().insert(100, Action::Detach);
    listen(&mut page.dom, This::Node(a.clone()), "focus", 100, false);
    focus(&mut page, &a, vec![]);
    assert_eq!(events(&page), ["focus DIV"]);
    assert_eq!(active(&mut page), "BODY");
}

/// The fixup: the focused element with no box any more loses the focus
/// with `blur` and `focusout`; one taken out loses it at once with no
/// events, and its ancestors match `:focus-within` no more.
#[test]
fn the_fixup_and_an_element_taken_out() {
    let mut page = focus_page(true);
    let (a, b) = (page.a.clone(), page.b.clone());
    focus(&mut page, &a, vec![]);
    events(&page);
    let document = page.document.clone();
    with_host(&page.dom, |host| dom_api::focus_fixup(host, &document));
    assert!(events(&page).is_empty(), "still focusable");
    let a_id = id(&page.dom, &a);
    fake(&page.dom, |fake| {
        fake.boxes.remove(&a_id);
    });
    with_host(&page.dom, |host| dom_api::focus_fixup(host, &document));
    assert_eq!(
        events(&page),
        ["blur DIV", "focusout DIV", "DOMFocusOut DIV"]
    );
    assert_eq!(active(&mut page), "BODY");
    focus(&mut page, &b, vec![]);
    events(&page);
    page.dom.mutate(|cx| cx.remove(b.get(cx.gc)));
    assert!(events(&page).is_empty());
    assert_eq!(active(&mut page), "BODY");
    let body = page.body.clone();
    assert_eq!(states(&page, &body), (false, false, false));
    assert_eq!(states(&page, &b), (false, false, false));
    // A child of the focused element taken out: it keeps the focus.
    let child_parent = page.link.clone();
    focus(&mut page, &child_parent, vec![]);
    let child = append(&mut page.dom, &child_parent, "span");
    page.dom.mutate(|cx| cx.remove(child.get(cx.gc)));
    assert_eq!(active(&mut page), "A");
}

#[test]
fn tab_index() {
    let mut page = focus_page(true);
    let tab_index = |page: &mut Page, element: &Rc<NodeRoot>| match crate::get(
        &mut page.dom,
        element,
        "HTMLElement",
        "tabIndex",
    ) {
        Ret::F64(value) => value,
        other => panic!("{other:?}"),
    };
    let (a, b, link, plain) = (
        page.a.clone(),
        page.b.clone(),
        page.link.clone(),
        page.plain.clone(),
    );
    assert_eq!(
        [&a, &b, &link, &plain].map(|element| tab_index(&mut page, element)),
        [0.0, -1.0, 0.0, -1.0]
    );
    call(
        &mut page.dom,
        &plain,
        "HTMLElement",
        "tabIndex",
        MemberKind::Setter,
        &[Arg::Number(5.0)],
    )
    .unwrap();
    assert_eq!(tab_index(&mut page, &plain), 5.0);
    assert!(page.dom.read(|cx| {
        cx.attribute_value_by_id(plain.get(cx.gc), dom::names::local::TABINDEX)
            .is_some_and(|value| *value.borrow() == *"5")
    }));
}

/// A listener moving the focus while it moves (as a `focus()` in it
/// would): during the old one's `blur`, its other events go on without a
/// related target and the new one doesn't take it; during the new one's
/// `focus`, its other events stop. One a listener takes out doesn't take
/// it.
#[test]
fn a_listener_moves_the_focus() {
    let mut page = focus_page(true);
    let (a, b, link) = (page.a.clone(), page.b.clone(), page.link.clone());
    focus(&mut page, &a, vec![]);
    events(&page);
    *page.dom.1.action_node.borrow_mut() = Some(link.clone());
    page.dom
        .1
        .actions
        .borrow_mut()
        .insert(101, Action::MoveFocus);
    listen(&mut page.dom, This::Node(a.clone()), "blur", 101, false);
    focus(&mut page, &b, vec![]);
    assert_eq!(
        events(&page),
        ["blur DIV<DIV", "focusout DIV", "DOMFocusOut DIV"]
    );
    assert_eq!(active(&mut page), "A");
    // During the new one's `focus`.
    let plain = page.plain.clone();
    page.dom
        .mutate(|cx| cx.set_attribute(plain.get(cx.gc), "tabindex", "2"));
    *page.dom.1.action_node.borrow_mut() = Some(a.clone());
    listen(
        &mut page.dom,
        This::Node(plain.clone()),
        "focus",
        101,
        false,
    );
    focus(&mut page, &plain, vec![]);
    assert_eq!(
        events(&page),
        [
            "blur A<DIV",
            "focusout A<DIV",
            "DOMFocusOut A<DIV",
            "focus DIV<A"
        ]
    );
    assert_eq!(active(&mut page), "DIV[0]");
    // The new one taken out during the old one's `blur`.
    let fresh = append(&mut page.dom, &page.body.clone(), "div");
    let fresh_id = id(&page.dom, &fresh);
    page.dom
        .mutate(|cx| cx.set_attribute(fresh.get(cx.gc), "tabindex", "0"));
    fake(&page.dom, |fake| {
        fake.boxes
            .insert(fresh_id, crate::geometry::block_box(fresh_id));
    });
    *page.dom.1.action_node.borrow_mut() = Some(fresh.clone());
    page.dom
        .1
        .actions
        .borrow_mut()
        .insert(102, Action::DetachNode);
    listen(&mut page.dom, This::Node(a.clone()), "blur", 102, false);
    focus(&mut page, &fresh, vec![]);
    assert_eq!(active(&mut page), "BODY");
}

/// `focusin` and `focusout` bubble, `focus` and `blur` don't; `blur()` of
/// an element without the focus leaves the focus where it is.
#[test]
fn which_bubble_and_blur_of_another() {
    let mut page = focus_page(true);
    let (a, link) = (page.a.clone(), page.link.clone());
    let body = page.body.clone();
    for event_type in ["focus", "focusin", "blur", "focusout"] {
        listen(
            &mut page.dom,
            This::Node(body.clone()),
            event_type,
            50,
            false,
        );
    }
    focus(&mut page, &a, vec![]);
    focus(&mut page, &link, vec![]);
    let bubbled: Vec<String> = page
        .dom
        .1
        .log
        .borrow()
        .iter()
        .filter(|call| call.callback == 50)
        .map(|call| call.event_type.clone())
        .collect();
    assert_eq!(bubbled, ["focusin", "focusout", "focusin"]);
    blur(&mut page, &a);
    assert_eq!(active(&mut page), "A");
}

fn key(down: bool, name: &str, modifiers: u16) -> dom_api::KeyInput {
    dom_api::KeyInput {
        down,
        key: name.to_owned(),
        code: name.to_owned(),
        key_code: 9,
        text: String::new(),
        location: 0,
        repeat: false,
        modifiers,
    }
}

fn press(
    page: &mut Page,
    handler: &mut dom_api::EventHandler,
    name: &str,
    modifiers: u16,
) -> dom_api::KeyResult {
    let document = page.document.clone();
    with_host(&page.dom, |host| {
        handler.handle_key(host, &document, &key(true, name, modifiers))
    })
}

/// Tab and Shift+Tab: positive `tabindex`es first by their values, then 0
/// and the links in tree order; none with a negative one or that can't take
/// the focus; past the ends the browser takes the focus back. A canceled
/// Tab moves nothing. Tab from the toolbar starts at the first (the last).
#[test]
fn tab_moves_the_focus() {
    let mut page = focus_page(true);
    let (a, b, link, plain) = (
        page.a.clone(),
        page.b.clone(),
        page.link.clone(),
        page.plain.clone(),
    );
    let late = append(&mut page.dom, &page.body.clone(), "div");
    let late_id = id(&page.dom, &late);
    page.dom.mutate(|cx| {
        cx.set_attribute(plain.get(cx.gc), "tabindex", "2");
        cx.set_attribute(late.get(cx.gc), "tabindex", "1");
    });
    fake(&page.dom, |fake| {
        fake.boxes
            .insert(late_id, crate::geometry::block_box(late_id));
    });
    let mut handler = dom_api::EventHandler::default();
    let mut order = Vec::new();
    for _ in 0..4 {
        assert_eq!(
            press(&mut page, &mut handler, "Tab", 0),
            dom_api::KeyResult::Done
        );
        order.push(active(&mut page));
    }
    assert_eq!(order, ["DIV[1]", "DIV[2]", "DIV[0]", "A"]);
    assert!(states(&page, &link).2, "a ring");
    assert_eq!(
        press(&mut page, &mut handler, "Tab", 0),
        dom_api::KeyResult::TakeFocus { reverse: false }
    );
    let shift = dom::modifiers::SHIFT;
    assert_eq!(
        press(&mut page, &mut handler, "Tab", shift),
        dom_api::KeyResult::Done
    );
    assert_eq!(active(&mut page), "DIV[0]");
    // From one not in the order (`tabindex=-1`): the start.
    focus(&mut page, &b, vec![]);
    press(&mut page, &mut handler, "Tab", 0);
    assert_eq!(active(&mut page), "DIV[1]");
    assert_eq!(
        press(&mut page, &mut handler, "Tab", shift),
        dom_api::KeyResult::TakeFocus { reverse: true }
    );
    // One that can't take it is skipped.
    let a_id = id(&page.dom, &a);
    fake(&page.dom, |fake| {
        fake.boxes.remove(&a_id);
    });
    focus(&mut page, &plain, vec![]);
    press(&mut page, &mut handler, "Tab", 0);
    assert_eq!(active(&mut page), "A");
    // With Ctrl it's the browser's; canceled, nothing (from one with a
    // next).
    focus(&mut page, &plain, vec![]);
    press(&mut page, &mut handler, "Tab", dom::modifiers::CTRL);
    assert_eq!(active(&mut page), "DIV[2]");
    page.dom.1.actions.borrow_mut().insert(60, Action::Cancel);
    listen(
        &mut page.dom,
        This::Node(page.document.clone()),
        "keydown",
        60,
        true,
    );
    press(&mut page, &mut handler, "Tab", 0);
    assert_eq!(active(&mut page), "DIV[2]");
    // From the toolbar: the first, the last.
    let document = page.document.clone();
    with_host(&page.dom, |host| {
        dom_api::set_initial_focus(host, &document, false)
    });
    assert_eq!(active(&mut page), "DIV[1]");
    with_host(&page.dom, |host| {
        dom_api::set_initial_focus(host, &document, true)
    });
    assert_eq!(active(&mut page), "A");
}

/// Keys go to the focused element; a key shows rings from then on.
#[test]
fn keys_go_to_the_focused_element() {
    let mut page = focus_page(true);
    let a = page.a.clone();
    listen(
        &mut page.dom,
        This::Node(page.document.clone()),
        "keydown",
        1,
        true,
    );
    let mut handler = dom_api::EventHandler::default();
    press(&mut page, &mut handler, "x", 0);
    assert_eq!(events(&page), ["keydown BODY"]);
    page.dom.mutate(|cx| {
        let window = page.window.get(cx.gc);
        cx.set_focus_visible(window, false);
    });
    focus(&mut page, &a, vec![]);
    events(&page);
    assert!(!states(&page, &a).2, "no ring after a press of the mouse");
    press(&mut page, &mut handler, "x", 0);
    assert_eq!(events(&page), ["keydown DIV"]);
    let b = page.b.clone();
    focus(&mut page, &b, vec![]);
    assert!(states(&page, &b).2, "a key shows rings");
}

/// A press of the mouse nobody canceled gives the focus to the nearest
/// element around its target that can take it, without a ring; with none,
/// to the viewport. Script's focus after it shows none either.
#[test]
fn the_mouse_gives_the_focus() {
    let mut page = focus_page(true);
    let (a, link, plain) = (page.a.clone(), page.link.clone(), page.plain.clone());
    let inner = append(&mut page.dom, &a, "span");
    let (inner_id, plain_id) = (id(&page.dom, &inner), id(&page.dom, &plain));
    let mut handler = dom_api::EventHandler::default();
    let document = page.document.clone();
    let mut mouse = |page: &mut Page, at: NodeId| {
        fake(&page.dom, |fake| fake.at = Some(at));
        let down = dom_api::MouseInput {
            action: dom_api::MouseAction::Down {
                button: dom_api::MouseButton::Primary,
                click_count: 1,
            },
            point: gfx::PointF::new(1.0, 1.0),
            buttons: 1,
            modifiers: 0,
        };
        with_host(&page.dom, |host| {
            handler.handle_mouse(host, &document, &down)
        });
    };
    mouse(&mut page, inner_id);
    assert_eq!(active(&mut page), "DIV[0]", "the nearest around it");
    assert_eq!(states(&page, &a), (true, true, false), "no ring");
    focus(&mut page, &link, vec![]);
    assert!(
        !states(&page, &link).2,
        "script's after the mouse: no ring either"
    );
    mouse(&mut page, plain_id);
    assert_eq!(active(&mut page), "BODY", "none around it: the viewport");
    // A canceled `mousedown`: the focus stays.
    focus(&mut page, &link, vec![]);
    page.dom.1.actions.borrow_mut().insert(61, Action::Cancel);
    listen(
        &mut page.dom,
        This::Node(page.document.clone()),
        "mousedown",
        61,
        true,
    );
    mouse(&mut page, plain_id);
    assert_eq!(active(&mut page), "A");
}

/// `select()` selects a text control's value and focuses it, as it does a
/// control of any type that can take the focus; not a disabled one.
#[test]
fn select_focuses_the_control() {
    let (mut dom, nodes, _) = page(false, &["input", "input", "input"]);
    let [document, _, _, text, checkbox, disabled] = nodes.as_slice() else {
        unreachable!()
    };
    dom.mutate(|cx| {
        cx.set_attribute(text.get(cx.gc), "value", "hello");
        cx.set_attribute(checkbox.get(cx.gc), "type", "checkbox");
        cx.set_attribute(disabled.get(cx.gc), "disabled", "");
        cx.create_window(document.get(cx.gc));
    });
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        for &node in &ids[1..] {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
    });
    let active = |dom: &mut TestDom| match crate::get(dom, document, "Document", "activeElement") {
        Ret::Node(node) => dom.read(|cx| cx.node_id(node.get(cx.gc))),
        other => panic!("{other:?}"),
    };
    call(
        &mut dom,
        text,
        "HTMLInputElement",
        "select",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    assert_eq!(active(&mut dom), ids[3]);
    assert!(matches!(
        crate::get(&mut dom, text, "HTMLInputElement", "selectionEnd"),
        Ret::U32(5)
    ));
    call(
        &mut dom,
        checkbox,
        "HTMLInputElement",
        "select",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    assert_eq!(active(&mut dom), ids[4], "a checkbox");
    call(
        &mut dom,
        disabled,
        "HTMLInputElement",
        "select",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    assert_eq!(active(&mut dom), ids[4], "not a disabled one");
}

/// A label's click focuses its control, if it can take the focus.
#[test]
fn a_labels_click_focuses_its_control() {
    let (mut dom, nodes, _) = page(false, &["label", "label"]);
    let [document, _, _, label, other] = nodes.as_slice() else {
        unreachable!()
    };
    let (control, hidden) = dom.mutate(|cx| {
        let document = document.get(cx.gc);
        cx.create_window(document);
        let control = cx.create_element(document, dom::QualName::html("textarea"));
        cx.append_child(label.get(cx.gc), control).unwrap();
        let hidden = cx.create_element(document, dom::QualName::html("input"));
        cx.set_attribute(hidden, "type", "hidden");
        cx.append_child(other.get(cx.gc), hidden).unwrap();
        (Rc::new(cx.root(control)), Rc::new(cx.root(hidden)))
    });
    let ids: Vec<NodeId> = nodes
        .iter()
        .chain([&control, &hidden])
        .map(|node| id(&dom, node))
        .collect();
    fake(&dom, |fake| {
        for &node in &ids[1..] {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
    });
    let active = |dom: &mut TestDom| match crate::get(dom, document, "Document", "activeElement") {
        Ret::Node(node) => dom.read(|cx| cx.node_id(node.get(cx.gc))),
        other => panic!("{other:?}"),
    };
    call(
        &mut dom,
        &hidden,
        "HTMLElement",
        "focus",
        MemberKind::Method,
        &[Arg::Dict(vec![])],
    )
    .unwrap();
    assert_eq!(active(&mut dom), ids[2], "a hidden input takes none");
    call(
        &mut dom,
        label,
        "HTMLElement",
        "click",
        MemberKind::Method,
        &[],
    )
    .unwrap();
    assert_eq!(active(&mut dom), ids[5]);
}

/// Tab goes through the form controls in tree order (a hidden input and a
/// disabled control aside).
#[test]
fn tab_goes_through_form_controls() {
    let (mut dom, nodes, _) = page(false, &["input", "input", "textarea", "button", "select"]);
    let [document, _, _, _, hidden, _, button, _] = nodes.as_slice() else {
        unreachable!()
    };
    dom.mutate(|cx| {
        cx.set_attribute(hidden.get(cx.gc), "type", "hidden");
        cx.set_attribute(button.get(cx.gc), "disabled", "");
        let window = cx.create_window(document.get(cx.gc));
        cx.set_page_focused(window, true);
    });
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        for &node in &ids[1..] {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
    });
    let mut handler = dom_api::EventHandler::default();
    let mut order = Vec::new();
    for _ in 0..3 {
        let document = document.clone();
        with_host(&dom, |host| {
            handler.handle_key(host, &document, &key(true, "Tab", 0))
        });
        order.push(
            match crate::get(&mut dom, &document, "Document", "activeElement") {
                Ret::Node(node) => dom.read(|cx| cx.node_id(node.get(cx.gc))),
                other => panic!("{other:?}"),
            },
        );
    }
    // The text input, the textarea, the select.
    assert_eq!(order, [ids[3], ids[5], ids[7]]);
}

/// A press of the mouse on a text control's UA shadow tree (its inner
/// editor, what the hit test finds) focuses the control.
#[test]
fn the_mouse_focuses_a_text_control_through_its_tree() {
    let (mut dom, nodes, _) = page(false, &["input"]);
    let [document, _, _, field] = nodes.as_slice() else {
        unreachable!()
    };
    let editor = dom.mutate(|cx| {
        let window = cx.create_window(document.get(cx.gc));
        cx.set_page_focused(window, true);
        let field = field.get(cx.gc);
        cx.ensure_shadow_tree(field);
        cx.node_id(cx.inner_editor(field).unwrap())
    });
    let ids: Vec<NodeId> = nodes.iter().map(|node| id(&dom, node)).collect();
    fake(&dom, |fake| {
        for &node in ids[1..].iter().chain([&editor]) {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
        fake.at = Some(editor);
    });
    let mut handler = dom_api::EventHandler::default();
    let down = dom_api::MouseInput {
        action: dom_api::MouseAction::Down {
            button: dom_api::MouseButton::Primary,
            click_count: 1,
        },
        point: gfx::PointF::new(1.0, 1.0),
        buttons: 1,
        modifiers: 0,
    };
    let document_root = document.clone();
    with_host(&dom, |host| {
        handler.handle_mouse(host, &document_root, &down)
    });
    match crate::get(&mut dom, document, "Document", "activeElement") {
        Ret::Node(node) => assert_eq!(dom.read(|cx| cx.node_id(node.get(cx.gc))), ids[3]),
        other => panic!("{other:?}"),
    }
}

/// A page (`focus_page`) whose body has an `<iframe>` showing a child
/// frame's document with a `<div tabindex=0>` and a `<div tabindex=-1>`,
/// boxes for all, its window's page focused; the focus events listened to
/// at the child's document (callbacks 11 to 16) and window (17). The
/// iframe, the child's document and its two divs.
fn frame_page(page: &mut Page) -> [Rc<NodeRoot>; 4] {
    let iframe = append(&mut page.dom, &page.body.clone(), "iframe");
    let (child, c, d, window) = page.dom.mutate(|cx| {
        let child = cx.create_document(true);
        let window = cx.create_window(child);
        cx.set_page_focused(window, true);
        let html = cx.create_element(child, dom::QualName::html("html"));
        cx.append_child(child, html).unwrap();
        let body = cx.create_element(child, dom::QualName::html("body"));
        cx.append_child(html, body).unwrap();
        let c = cx.create_element(child, dom::QualName::html("div"));
        cx.set_attribute(c, "tabindex", "0");
        cx.append_child(body, c).unwrap();
        let d = cx.create_element(child, dom::QualName::html("div"));
        cx.set_attribute(d, "tabindex", "-1");
        cx.append_child(body, d).unwrap();
        cx.set_content_document(iframe.get(cx.gc), Some(child));
        (
            Rc::new(cx.root(child)),
            Rc::new(cx.root(c)),
            Rc::new(cx.root(d)),
            Rc::new(cx.root_object(ObjectRef::Window(window))),
        )
    });
    let ids: Vec<NodeId> = [&iframe, &c, &d]
        .iter()
        .map(|node| id(&page.dom, node))
        .chain(page.dom.read(|cx| {
            let child = child.get(cx.gc);
            let html = cx.first_child(child).unwrap();
            vec![cx.node_id(html), cx.node_id(cx.first_child(html).unwrap())]
        }))
        .collect();
    let child_id = id(&page.dom, &child);
    fake(&page.dom, |fake| {
        for &node in &ids {
            fake.boxes.insert(node, crate::geometry::block_box(node));
        }
        fake.frames.insert(child_id, gfx::PointF::new(0.0, 100.0));
    });
    for (index, &event_type) in TYPES.iter().enumerate() {
        listen(
            &mut page.dom,
            This::Node(child.clone()),
            event_type,
            index as u64 + 11,
            true,
        );
    }
    for event_type in ["focus", "blur"] {
        listen(
            &mut page.dom,
            This::Object(window.clone()),
            event_type,
            17,
            false,
        );
    }
    [iframe, child, c, d]
}

/// The events since the last call of both documents and windows: the
/// child's marked.
fn frame_events(page: &Page) -> Vec<String> {
    page.dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback <= 7 || (11..=17).contains(&call.callback))
        .map(|call| {
            let child = if call.callback > 7 { "child " } else { "" };
            format!("{child}{} {}", call.event_type, call.event_target)
        })
        .collect()
}

/// The focus into a child frame: it becomes the focused frame (the old
/// one's element `blur`s, the old window `blur`s and the new one `focus`es),
/// the iframe is the page's focused element (`activeElement`,
/// `:focus-within` around it; no focus events of its own), `hasFocus()` in
/// both; back out, the other way. Focusing the iframe focuses its child
/// frame's viewport.
#[test]
fn the_focus_goes_into_a_child_frame() {
    let mut page = focus_page(true);
    let [iframe, child, c, _] = frame_page(&mut page);
    let a = page.a.clone();
    focus(&mut page, &a, vec![]);
    frame_events(&page);
    focus(&mut page, &c, vec![]);
    assert_eq!(
        frame_events(&page),
        [
            "blur DIV",
            "focusout DIV",
            "DOMFocusOut DIV",
            "blur #window",
            "child focus #window",
            "child focus DIV",
            "child focusin DIV",
            "child DOMFocusIn DIV"
        ]
    );
    assert_eq!(active(&mut page), "IFRAME");
    assert!(states(&page, &iframe).1, ":focus-within");
    assert!(states(&page, &page.body.clone()).1);
    let has_focus = |page: &mut Page, document: &Rc<NodeRoot>| match crate::call(
        &mut page.dom,
        document,
        "Document",
        "hasFocus",
        MemberKind::Method,
        &[],
    ) {
        Ok(Ret::Bool(focused)) => focused,
        other => panic!("{other:?}"),
    };
    let document = page.document.clone();
    assert!(has_focus(&mut page, &document) && has_focus(&mut page, &child));
    // Back to the page's element.
    focus(&mut page, &a, vec![]);
    assert_eq!(
        frame_events(&page),
        [
            "child blur DIV",
            "child focusout DIV",
            "child DOMFocusOut DIV",
            "child blur #window",
            "focus #window",
            "focus DIV",
            "focusin DIV",
            "DOMFocusIn DIV"
        ]
    );
    assert!(has_focus(&mut page, &document) && !has_focus(&mut page, &child));
    assert!(!states(&page, &iframe).1);
    // The iframe itself: its child frame's viewport.
    focus(&mut page, &iframe, vec![]);
    assert_eq!(active(&mut page), "IFRAME");
    assert!(has_focus(&mut page, &child));
    // Its `blur()`: the page's viewport; the iframe has no events of its
    // own.
    focus(&mut page, &c, vec![]);
    frame_events(&page);
    blur(&mut page, &iframe);
    assert_eq!(
        frame_events(&page),
        [
            "child blur DIV",
            "child focusout DIV",
            "child DOMFocusOut DIV",
            "child blur #window",
            "focus #window"
        ]
    );
    assert_eq!(active(&mut page), "BODY");
}

/// Tab goes through the child frame's document where its iframe is in the
/// page's order, and back out; keys go to the focused frame's focused
/// element; the page's focus events go to the focused frame's.
#[test]
fn tab_and_keys_go_through_the_frames() {
    let mut page = focus_page(true);
    let [_, _, c, _] = frame_page(&mut page);
    let mut handler = dom_api::EventHandler::default();
    let focused_c = |page: &Page| page.dom.read(|cx| cx.is_focused(c.get(cx.gc)));
    let mut order = Vec::new();
    for _ in 0..3 {
        press(&mut page, &mut handler, "Tab", 0);
        order.push(active(&mut page));
    }
    // DIV[0], A, then the child's DIV[0] (the page's is its iframe).
    assert_eq!(order, ["DIV[0]", "A", "IFRAME"]);
    assert!(focused_c(&page));
    assert_eq!(
        press(&mut page, &mut handler, "Tab", 0),
        dom_api::KeyResult::TakeFocus { reverse: false }
    );
    press(&mut page, &mut handler, "Tab", dom::modifiers::SHIFT);
    assert!(!focused_c(&page));
    assert_eq!(active(&mut page), "A");
    press(&mut page, &mut handler, "Tab", 0);
    assert!(focused_c(&page));
    // A key: at the child's focused element.
    listen(&mut page.dom, This::Node(c.clone()), "keydown", 18, false);
    press(&mut page, &mut handler, "x", 0);
    let keys: Vec<String> = page
        .dom
        .1
        .log
        .borrow_mut()
        .drain(..)
        .filter(|call| call.callback == 18)
        .map(|call| call.event_target.clone())
        .collect();
    assert_eq!(keys, ["DIV"]);
    // The page loses the system's focus: the child frame's element and
    // window blur.
    let window = page.window.clone();
    with_host(&page.dom, |host| {
        dom_api::set_page_focused(host, &window, false)
    });
    let log = frame_events(&page);
    assert_eq!(
        log,
        [
            "child blur DIV",
            "child focusout DIV",
            "child DOMFocusOut DIV",
            "child blur #window"
        ]
    );
    // Every window of the tree knows: the child's document has no focus.
    let child = page.dom.read(|cx| cx.root(cx.document_of(c.get(cx.gc))));
    match crate::call(
        &mut page.dom,
        &Rc::new(child),
        "Document",
        "hasFocus",
        MemberKind::Method,
        &[],
    ) {
        Ok(Ret::Bool(focused)) => assert!(!focused),
        other => panic!("{other:?}"),
    }
}

/// A shadow root's `activeElement`: the focused element if it is in the
/// root's tree, none for one outside it; the document's is the host.
#[test]
fn a_shadow_roots_active_element() {
    let mut page = focus_page(true);
    let (root, inner) = page.dom.mutate(|cx| {
        let host = page.plain.get(cx.gc);
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
        let document = cx.document_of(host);
        let inner = cx.create_element(document, dom::QualName::html("div"));
        cx.set_attribute(inner, "tabindex", "0");
        cx.append_child(root, inner).unwrap();
        (Rc::new(cx.root(root)), Rc::new(cx.root(inner)))
    });
    let inner_id = id(&page.dom, &inner);
    fake(&page.dom, |fake| {
        fake.boxes
            .insert(inner_id, crate::geometry::block_box(inner_id));
    });
    let shadow_active =
        |page: &mut Page| match crate::get(&mut page.dom, &root, "ShadowRoot", "activeElement") {
            Ret::Node(node) => page
                .dom
                .read(|cx| cx.node_name(node.get(cx.gc)).to_string()),
            Ret::Null => "null".to_owned(),
            other => panic!("{other:?}"),
        };
    assert_eq!(shadow_active(&mut page), "null");
    let a = page.a.clone();
    focus(&mut page, &a, vec![]);
    assert_eq!(shadow_active(&mut page), "null", "outside it");
    focus(&mut page, &inner, vec![]);
    assert_eq!(shadow_active(&mut page), "DIV");
    assert_eq!(active(&mut page), "DIV", "the host, for the document");
}
