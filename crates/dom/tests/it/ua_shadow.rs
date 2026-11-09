//! Form controls' UA shadow trees: built when a control is connected to a
//! document with a window, at the next style update (not for one taken
//! out before, nor in a document without a window, nor for a type that
//! needs none; through the shadow trees inserted with it); the parts and
//! their shadow pseudo ids; the tree following the control at once (the
//! value set, the `value` attribute, a reset, `setRangeText()`, a
//! textarea's children, the placeholder shown and hidden, a search
//! field's clear button, a range's thumb, a button's label); a new type's
//! tree; never cloned; the native look's states flagged for layout.

use dom::{
    Cx, Dom, LayoutTreeFlags, NodeRef, QualName, SelectionMode, ShadowRootInit, ShadowRootMode,
    SlotAssignmentMode,
};

/// A document with a window, an `<html>` and a `<body>`: the body.
fn page<'gc>(cx: &Cx<'_, 'gc>, window: bool) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    if window {
        cx.create_window(document);
    }
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

fn input<'gc>(cx: &Cx<'_, 'gc>, body: NodeRef<'gc>, ty: &str) -> NodeRef<'gc> {
    let input = cx.create_element(cx.document_of(body), QualName::html("input"));
    cx.set_attribute(input, "type", ty);
    input
}

/// The data of `node`'s first child.
fn first_text<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> String {
    cx.first_child(node)
        .and_then(|text| cx.data(text))
        .map(|data| data.to_str().into_owned())
        .unwrap_or_default()
}

/// The value the inner editor shows.
fn shown<'gc>(cx: &Cx<'_, 'gc>, control: NodeRef<'gc>) -> String {
    cx.inner_editor_text(control)
        .and_then(|text| cx.data(text))
        .map(|data| data.to_str().into_owned())
        .expect("an inner editor's text")
}

/// The shadow pseudo ids of `node`'s descendants in its UA shadow tree, in
/// tree order, a child's after its parent's.
fn parts<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Vec<String> {
    fn walk<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>, out: &mut Vec<String>) {
        let mut child = cx.first_child(node);
        while let Some(at) = child {
            match cx.shadow_pseudo_id(at) {
                Some(id) => out.push(id.to_str().into_owned()),
                None => out.push(cx.node_name(at).to_string()),
            }
            walk(cx, at, out);
            child = cx.next_sibling(at);
        }
    }
    let mut out = Vec::new();
    if let Some(root) = cx.user_agent_shadow_root(node) {
        walk(cx, root, &mut out);
    }
    out
}

#[test]
fn a_tree_is_built_when_a_shown_control_is_styled_next() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let text = input(cx, body, "text");
        cx.set_attribute(text, "value", "hello");
        cx.append_child(body, text).unwrap();
        // Not until the next style update.
        assert!(cx.user_agent_shadow_root(text).is_none());
        // One taken out before it, a checkbox, one out of the document:
        // none.
        let gone = input(cx, body, "text");
        cx.append_child(body, gone).unwrap();
        cx.remove(gone);
        let checkbox = input(cx, body, "checkbox");
        cx.append_child(body, checkbox).unwrap();
        let detached = input(cx, body, "text");
        // A textarea in a host's shadow tree, connected with the host.
        let host = cx.create_element(cx.document_of(body), QualName::html("div"));
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: true,
                serializable: false,
            },
        );
        let area = cx.create_element(cx.document_of(body), QualName::html("textarea"));
        cx.append_child(root, area).unwrap();
        cx.append_child(body, host).unwrap();
        cx.create_scheduled_shadow_trees();
        let made = cx.user_agent_shadow_root(text).expect("its tree");
        assert_eq!(
            cx.shadow_root_init(made).map(|init| init.mode),
            Some(ShadowRootMode::UserAgent)
        );
        assert_eq!(parts(cx, text), ["-internal-inner-editor", "#text"]);
        assert_eq!(shown(cx, text), "hello");
        assert!(cx.user_agent_shadow_root(gone).is_none());
        assert!(cx.user_agent_shadow_root(checkbox).is_none());
        assert!(cx.user_agent_shadow_root(detached).is_none());
        assert_eq!(parts(cx, area), ["-internal-inner-editor", "#text"]);
        // Inserted again: its tree stays the one it has.
        cx.remove(text);
        cx.append_child(body, text).unwrap();
        cx.create_scheduled_shadow_trees();
        assert!(
            cx.user_agent_shadow_root(text)
                .is_some_and(|again| elise::Gc::ptr_eq(again, made))
        );
        // A copy has none (a UA shadow root isn't clonable).
        let copy = cx.clone_node(text, None, true);
        assert!(cx.shadow_root(copy).is_none());
        // In a document without a window: none, unless asked for.
        let other = page(cx, false);
        let elsewhere = input(cx, other, "text");
        cx.append_child(other, elsewhere).unwrap();
        cx.create_scheduled_shadow_trees();
        assert!(cx.user_agent_shadow_root(elsewhere).is_none());
        cx.ensure_shadow_tree(elsewhere);
        assert!(cx.user_agent_shadow_root(elsewhere).is_some());
        // The `pseudo` attribute of an element of an author's shadow tree
        // is no shadow pseudo id.
        let fake = cx.create_element(cx.document_of(body), QualName::html("div"));
        cx.set_attribute(fake, "pseudo", "-internal-inner-editor");
        cx.append_child(root, fake).unwrap();
        assert!(cx.shadow_pseudo_id(fake).is_none());
    });
}

#[test]
fn each_control_has_its_tree() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let make = |ty: &str| {
            let control = input(cx, body, ty);
            cx.append_child(body, control).unwrap();
            control
        };
        let (search, number, range, submit, reset, button) = (
            make("search"),
            make("number"),
            make("range"),
            make("submit"),
            make("reset"),
            make("button"),
        );
        cx.create_scheduled_shadow_trees();
        assert_eq!(
            parts(cx, search),
            [
                "-webkit-textfield-decoration-container",
                "-internal-editing-view-port",
                "-internal-inner-editor",
                "#text",
                "-webkit-search-cancel-button"
            ]
        );
        assert_eq!(parts(cx, number)[4], "-webkit-inner-spin-button");
        assert_eq!(
            parts(cx, range),
            [
                "-webkit-slider-container",
                "-webkit-slider-runnable-track",
                "-webkit-slider-thumb"
            ]
        );
        // A button's label: its value, else its type's; an empty one a
        // zero-width space.
        let root = |control| cx.user_agent_shadow_root(control).unwrap();
        assert_eq!(first_text(cx, root(submit)), "Submit");
        assert_eq!(first_text(cx, root(reset)), "Reset");
        assert_eq!(first_text(cx, root(button)), "\u{200B}");
        cx.set_attribute(submit, "value", "Go");
        assert_eq!(first_text(cx, root(submit)), "Go");
        cx.set_attribute(submit, "value", "");
        assert_eq!(first_text(cx, root(submit)), "\u{200B}");
    });
}

#[test]
fn the_tree_follows_the_control() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let text = input(cx, body, "text");
        cx.append_child(body, text).unwrap();
        let area = cx.create_element(cx.document_of(body), QualName::html("textarea"));
        cx.append_child(body, area).unwrap();
        let search = input(cx, body, "search");
        cx.append_child(body, search).unwrap();
        let range = input(cx, body, "range");
        cx.append_child(body, range).unwrap();
        cx.create_scheduled_shadow_trees();
        // The `value` attribute while it isn't dirty, the value set, a
        // reset, `setRangeText()`.
        cx.set_attribute(text, "value", "attr");
        assert_eq!(shown(cx, text), "attr");
        cx.set_input_value(text, "set\nnewline").unwrap();
        assert_eq!(shown(cx, text), "setnewline");
        cx.set_attribute(text, "value", "ignored");
        assert_eq!(shown(cx, text), "setnewline");
        let form = cx.create_element(cx.document_of(body), QualName::html("form"));
        cx.append_child(body, form).unwrap();
        cx.append_child(form, text).unwrap();
        cx.reset_form_controls(form);
        assert_eq!(shown(cx, text), "ignored");
        cx.set_range_text(text, "X", Some((0, 1)), SelectionMode::Preserve);
        assert_eq!(shown(cx, text), "Xgnored");
        // A textarea's children while it isn't dirty (newlines
        // normalized), its value set.
        let child = cx.create_text(cx.document_of(body), "a\r\nb");
        cx.append_child(area, child).unwrap();
        assert_eq!(shown(cx, area), "a\nb");
        cx.set_data(child, "c");
        assert_eq!(shown(cx, area), "c");
        cx.set_textarea_value(area, "own");
        assert_eq!(shown(cx, area), "own");
        cx.set_data(child, "d");
        assert_eq!(shown(cx, area), "own");
        cx.set_range_text(area, "!", Some((3, 3)), SelectionMode::Preserve);
        assert_eq!(shown(cx, area), "own!");
        // The user's edit, an input's and a textarea's.
        cx.replace_by_user(area, 0, 1, "O");
        assert_eq!(shown(cx, area), "Own!");
        cx.replace_by_user(text, 0, 0, ">");
        assert_eq!(shown(cx, text), ">Xgnored");
        // A form's reset: its children's again.
        let area_form = cx.create_element(cx.document_of(body), QualName::html("form"));
        cx.append_child(body, area_form).unwrap();
        cx.append_child(area_form, area).unwrap();
        cx.reset_form_controls(area_form);
        assert_eq!(shown(cx, area), "d");
        // The placeholder: there while the attribute is, shown while the
        // value is empty; a text field's on one line, a text area's not.
        assert!(cx.placeholder_element(text).is_none());
        cx.set_attribute(text, "placeholder", "type\nhere");
        let holder = cx.placeholder_element(text).expect("a placeholder");
        assert_eq!(first_text(cx, holder), "typehere");
        let display = |holder| {
            cx.attribute_value_ns(holder, None, "style")
                .map(|value| value.borrow().to_str().into_owned())
        };
        assert_eq!(display(holder).as_deref(), Some("display: none !important"));
        cx.set_input_value(text, "").unwrap();
        assert_eq!(
            display(holder).as_deref(),
            Some("display: block !important")
        );
        assert_eq!(
            parts(cx, text),
            [
                "-webkit-input-placeholder",
                "#text",
                "-internal-inner-editor",
                "#text"
            ]
        );
        cx.remove_attribute(text, "placeholder");
        assert!(cx.placeholder_element(text).is_none());
        cx.set_attribute(area, "placeholder", "two\nlines");
        assert_eq!(
            first_text(cx, cx.placeholder_element(area).unwrap()),
            "two\nlines"
        );
        // A search field's clear button: hidden while the value is empty.
        let cancel = *cx
            .first_child(cx.user_agent_shadow_root(search).unwrap())
            .map(|container| cx.last_child(container).unwrap())
            .as_ref()
            .unwrap();
        assert_eq!(display(cancel).as_deref(), Some("visibility: hidden"));
        cx.set_input_value(search, "q").unwrap();
        assert_eq!(display(cancel), None);
        // A range's thumb: where its value is between its minimum and
        // maximum, as they change.
        let thumb = cx
            .first_child(
                cx.first_child(
                    cx.first_child(cx.user_agent_shadow_root(range).unwrap())
                        .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
        let ratio = |thumb| {
            cx.attribute_value_ns(thumb, None, "ratio")
                .map(|value| value.borrow().to_str().into_owned())
        };
        assert_eq!(ratio(thumb).as_deref(), Some("0.5"));
        cx.set_input_value(range, "25").unwrap();
        assert_eq!(ratio(thumb).as_deref(), Some("0.25"));
        cx.set_attribute(range, "max", "50");
        assert_eq!(ratio(thumb).as_deref(), Some("0.5"));
        cx.set_attribute(range, "min", "25");
        assert_eq!(ratio(thumb).as_deref(), Some("0"));
        cx.set_attribute(range, "step", "20");
        cx.set_input_value(range, "40").unwrap();
        assert_eq!(ratio(thumb).as_deref(), Some("0.8"));
    });
}

#[test]
fn a_new_type_has_its_tree() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let control = input(cx, body, "text");
        cx.set_attribute(control, "value", "v");
        cx.append_child(body, control).unwrap();
        cx.create_scheduled_shadow_trees();
        let root = cx.user_agent_shadow_root(control).unwrap();
        // None for a checkbox: the root stays, empty.
        cx.set_attribute(control, "type", "checkbox");
        assert!(cx.first_child(root).is_none());
        // Another tree, made at once.
        cx.set_attribute(control, "type", "submit");
        assert_eq!(first_text(cx, root), "v");
        cx.set_attribute(control, "type", "search");
        assert_eq!(
            parts(cx, control)[0],
            "-webkit-textfield-decoration-container"
        );
        assert_eq!(shown(cx, control), "v");
        // Text to password: the same tree.
        cx.set_attribute(control, "type", "text");
        let editor = cx.inner_editor(control).unwrap();
        cx.set_attribute(control, "type", "password");
        assert!(
            cx.inner_editor(control)
                .is_some_and(|again| elise::Gc::ptr_eq(again, editor))
        );
        assert_eq!(
            parts(cx, control),
            ["-internal-inner-editor", "#text"],
            "not built again"
        );
        // One with no tree that comes to need one: scheduled.
        let checkbox = input(cx, body, "checkbox");
        cx.append_child(body, checkbox).unwrap();
        cx.create_scheduled_shadow_trees();
        cx.set_attribute(checkbox, "type", "email");
        assert!(cx.user_agent_shadow_root(checkbox).is_none());
        cx.create_scheduled_shadow_trees();
        assert!(cx.inner_editor(checkbox).is_some());
    });
}

/// Which of `nodes` are flagged as a native look's state changed, their
/// flags cleared.
fn looks_changed<'gc>(cx: &Cx<'_, 'gc>, nodes: &[NodeRef<'gc>]) -> Vec<bool> {
    nodes
        .iter()
        .map(|&node| {
            let changed = cx
                .layout_tree_flags(node)
                .contains(LayoutTreeFlags::CONTROL_STATE_CHANGED);
            cx.clear_layout_tree_flags(node);
            changed
        })
        .collect()
}

#[test]
fn a_controls_native_looks_states_are_flagged() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let checkbox = input(cx, body, "checkbox");
        cx.append_child(body, checkbox).unwrap();
        let text = input(cx, body, "number");
        cx.append_child(body, text).unwrap();
        let div = cx.create_element(cx.document_of(body), QualName::html("div"));
        cx.append_child(body, div).unwrap();
        cx.create_scheduled_shadow_trees();
        cx.set_checked(checkbox, true);
        // Its ancestors lead layout to it.
        assert!(
            cx.layout_tree_flags(body)
                .contains(LayoutTreeFlags::CHILD_NEEDS_UPDATE)
        );
        assert_eq!(
            looks_changed(cx, &[checkbox, text, body]),
            [true, false, false]
        );
        cx.set_checked(checkbox, true);
        assert_eq!(looks_changed(cx, &[checkbox]), [false], "unchanged");
        cx.set_attribute(text, "disabled", "");
        assert_eq!(looks_changed(cx, &[text]), [true]);
        cx.set_attribute(text, "readonly", "");
        assert_eq!(looks_changed(cx, &[text]), [true]);
        cx.set_attribute(text, "title", "no");
        assert_eq!(looks_changed(cx, &[text]), [false]);
        // The hover chain: the spin button, its decoration container (a
        // part too), the input; not the body nor another element.
        let spin = cx
            .last_child(
                cx.first_child(cx.user_agent_shadow_root(text).unwrap())
                    .unwrap(),
            )
            .unwrap();
        let container = cx.parent(spin).unwrap();
        cx.set_hover_element(cx.document_of(body), Some(spin));
        assert_eq!(
            looks_changed(cx, &[spin, container, text, body]),
            [true, true, true, false]
        );
        cx.set_hover_element(cx.document_of(body), Some(div));
        assert_eq!(looks_changed(cx, &[div, text]), [false, true]);
    });
}

/// A text control's selection that changes is a change of the trees (the
/// frame draws it: the caret, the selected text); one set as it was isn't.
#[test]
fn a_selection_change_is_a_change() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let text = input(cx, body, "text");
        cx.set_attribute(text, "value", "abc");
        let version = cx.tree_version();
        cx.set_selection_range(text, 1, 2, dom::SelectionDirection::None);
        assert!(cx.tree_version() > version);
        let version = cx.tree_version();
        cx.set_selection_range(text, 1, 2, dom::SelectionDirection::None);
        assert_eq!(cx.tree_version(), version);
    });
}

/// A `<select>` of `body`'s document with options of `texts` (a `+` before
/// one selects it), appended to `body`.
fn select<'gc>(cx: &Cx<'_, 'gc>, body: NodeRef<'gc>, texts: &[&str]) -> NodeRef<'gc> {
    let document = cx.document_of(body);
    let select = cx.create_element(document, QualName::html("select"));
    for text in texts {
        let option = cx.create_element(document, QualName::html("option"));
        let (selected, text) = match text.strip_prefix('+') {
            Some(text) => (true, text),
            None => (false, *text),
        };
        if selected {
            cx.set_attribute(option, "selected", "");
        }
        cx.append_child(option, cx.create_text(document, text))
            .unwrap();
        cx.append_child(select, option).unwrap();
    }
    cx.append_child(body, select).unwrap();
    select
}

/// A menu list's inner element: its text and its labels.
fn menu_list<'gc>(cx: &Cx<'_, 'gc>, select: NodeRef<'gc>) -> (String, String) {
    let root = cx.user_agent_shadow_root(select).expect("a tree");
    let inner = cx.first_child(root).expect("the inner element");
    let labels = cx
        .attribute_value_ns(inner, None, "labels")
        .map(|labels| labels.to_owned().to_str().into_owned())
        .unwrap_or_default();
    (first_text(cx, inner), labels)
}

#[test]
fn a_selects_options_and_optgroups_have_their_trees() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let menu = select(cx, body, &["a", "+b"]);
        let list = select(cx, body, &["x"]);
        cx.set_attribute(list, "size", "3");
        let document = cx.document_of(body);
        let group = cx.create_element(document, QualName::html("optgroup"));
        cx.set_attribute(group, "label", "G");
        cx.append_child(list, group).unwrap();
        cx.create_scheduled_shadow_trees();
        assert_eq!(parts(cx, menu), ["-internal-menulist-inner", "#text"]);
        assert_eq!(menu_list(cx, menu), ("b".to_owned(), "a\nb".to_owned()));
        assert_eq!(parts(cx, list), ["SLOT"]);
        assert_eq!(
            parts(cx, group),
            ["-internal-optgroup-label", "#text", "SLOT"]
        );
        let option = cx.first_child(menu).unwrap();
        assert_eq!(
            parts(cx, option),
            ["-internal-option-label-container", "#text", "SLOT"]
        );
        // A select's tree follows `multiple` and `size`: a list box's and
        // a menu list's anew.
        cx.set_attribute(menu, "size", "4");
        assert_eq!(parts(cx, menu), ["SLOT"]);
        cx.remove_attribute(list, "size");
        assert_eq!(parts(cx, list), ["-internal-menulist-inner", "#text"]);
        assert_eq!(menu_list(cx, list).0, "x");
    });
}

#[test]
fn a_menu_list_follows_its_options() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let menu = select(cx, body, &["a", "+b"]);
        cx.create_scheduled_shadow_trees();
        let document = cx.document_of(body);
        let a = cx.first_child(menu).unwrap();
        let b = cx.next_sibling(a).unwrap();
        // The selectedness.
        cx.set_option_selected(a, true);
        assert_eq!(menu_list(cx, menu).0, "a");
        // The selected one's text, a child's and a deeper one's.
        cx.set_data(cx.first_child(a).unwrap(), " apple ");
        // Stripped and collapsed, as `text`.
        assert_eq!(
            menu_list(cx, menu),
            ("apple".to_owned(), "apple\nb".to_owned())
        );
        let bold = cx.create_element(document, QualName::html("b"));
        cx.append_child(bold, cx.create_text(document, "!"))
            .unwrap();
        cx.append_child(a, bold).unwrap();
        assert_eq!(menu_list(cx, menu).0, "apple !");
        cx.set_data(cx.first_child(bold).unwrap(), "?");
        assert_eq!(menu_list(cx, menu).0, "apple ?");
        cx.remove(bold);
        assert_eq!(menu_list(cx, menu).0, "apple");
        cx.set_text_content(a, "avocado");
        assert_eq!(menu_list(cx, menu).0, "avocado");
        // A label as it is in the labels, stripped as the text shown.
        cx.set_attribute(a, "label", " A ");
        assert_eq!(menu_list(cx, menu), ("A".to_owned(), " A \nb".to_owned()));
        cx.remove_attribute(a, "label");
        // Another's label: only the labels, and the select's box built
        // again (its width); not when they stay.
        cx.clear_layout_tree_flags(menu);
        cx.set_attribute(b, "label", "Bee");
        assert_eq!(
            menu_list(cx, menu),
            ("avocado".to_owned(), "avocado\nBee".to_owned())
        );
        assert!(
            cx.layout_tree_flags(menu)
                .contains(LayoutTreeFlags::NEEDS_REATTACH)
        );
        cx.clear_layout_tree_flags(menu);
        cx.set_attribute(b, "title", "t");
        cx.set_option_selected(b, true);
        cx.set_option_selected(a, true);
        assert!(
            !cx.layout_tree_flags(menu)
                .contains(LayoutTreeFlags::NEEDS_REATTACH)
        );
        // The list of options: one inserted, in an optgroup, removed.
        let group = cx.create_element(document, QualName::html("optgroup"));
        let c = cx.create_element(document, QualName::html("option"));
        cx.append_child(c, cx.create_text(document, "c")).unwrap();
        cx.append_child(group, c).unwrap();
        cx.append_child(menu, group).unwrap();
        assert_eq!(menu_list(cx, menu).1, "avocado\nBee\n    c");
        cx.remove(a);
        assert_eq!(
            menu_list(cx, menu),
            ("Bee".to_owned(), "Bee\n    c".to_owned())
        );
        // `multiple` with `size=1`: how many are selected.
        cx.set_attribute(menu, "size", "1");
        cx.set_attribute(menu, "multiple", "");
        cx.set_option_selected(c, true);
        assert_eq!(
            menu_list(cx, menu),
            ("2 selected".to_owned(), "Bee\n    c\n2 selected".to_owned())
        );
        // Emptied and filled in one change.
        cx.set_text_content(menu, "");
        assert_eq!(
            menu_list(cx, menu),
            ("0 selected".to_owned(), "0 selected".to_owned())
        );
    });
}

fn style_of<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Option<String> {
    cx.attribute_value_ns(node, None, "style")
        .map(|style| style.to_owned().to_str().into_owned())
}

#[test]
fn an_options_and_an_optgroups_labels() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = page(cx, true);
        let list = select(cx, body, &["a"]);
        cx.set_attribute(list, "size", "2");
        let document = cx.document_of(body);
        let group = cx.create_element(document, QualName::html("optgroup"));
        cx.append_child(list, group).unwrap();
        cx.create_scheduled_shadow_trees();
        let option = cx.first_child(list).unwrap();
        let style = |node| style_of(cx, node);
        let root = cx.user_agent_shadow_root(option).unwrap();
        let (label, slot) = (cx.first_child(root).unwrap(), cx.last_child(root).unwrap());
        // No label: the children shown.
        assert_eq!(
            (style(label).as_deref(), style(slot)),
            (Some("display: none"), None)
        );
        cx.set_attribute(option, "label", "L");
        assert_eq!(
            (style(label), style(slot).as_deref(), first_text(cx, label)),
            (None, Some("display: none"), "L".to_owned())
        );
        // An empty one is none.
        cx.set_attribute(option, "label", "");
        assert_eq!(style(label).as_deref(), Some("display: none"));
        let group_label = cx
            .first_child(cx.user_agent_shadow_root(group).unwrap())
            .unwrap();
        assert_eq!(first_text(cx, group_label), "");
        cx.set_attribute(group, "label", "Group");
        assert_eq!(first_text(cx, group_label), "Group");
    });
}
