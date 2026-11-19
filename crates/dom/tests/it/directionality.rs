//! HTML's directionality, cached on each element and kept up as the tree
//! changes: `dir` inherited down (into shadow trees), an `auto` element's
//! (`dir=auto`, a `<bdi>`) its text's or a form control's value's, a
//! telephone field `ltr`; and an element whose directionality changed
//! other than by its own `dir` noted for style.

use computed_style::Direction::{Ltr, Rtl};
use dom::{
    Cx, Dom, NodeRef, QualName, ShadowRootInit, ShadowRootMode, SlotAssignmentMode, StyleFlags,
};

/// A document's `<body>`, in its `<html>`.
fn body<'gc>(cx: &Cx<'_, 'gc>) -> NodeRef<'gc> {
    let document = cx.create_document(true);
    cx.create_window(document);
    let html = cx.create_element(document, QualName::html("html"));
    cx.append_child(document, html).unwrap();
    let body = cx.create_element(document, QualName::html("body"));
    cx.append_child(html, body).unwrap();
    body
}

fn element<'gc>(cx: &Cx<'_, 'gc>, parent: NodeRef<'gc>, name: &str) -> NodeRef<'gc> {
    let element = cx.create_element(cx.document_of(parent), QualName::html(name));
    cx.append_child(parent, element).unwrap();
    element
}

fn text<'gc>(cx: &Cx<'_, 'gc>, parent: NodeRef<'gc>, data: &str) -> NodeRef<'gc> {
    let text = cx.create_text(cx.document_of(parent), data);
    cx.append_child(parent, text).unwrap();
    text
}

/// A `dir` is inherited down, as far as one of the descendants' own;
/// another value is as none.
#[test]
fn dir_is_inherited() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        let div = element(cx, body, "div");
        let p = element(cx, div, "p");
        let span = element(cx, p, "span");
        let own = element(cx, p, "b");
        cx.set_attribute(own, "dir", "ltr");
        assert_eq!(cx.directionality(span), Ltr);
        cx.set_attribute(div, "dir", "RTL");
        assert_eq!(cx.directionality(div), Rtl);
        assert_eq!(cx.directionality(span), Rtl);
        assert_eq!(cx.directionality(own), Ltr, "its own");
        cx.set_attribute(div, "dir", "bogus");
        assert_eq!(cx.directionality(span), Ltr);
        cx.set_attribute(div, "dir", "rtl");
        cx.remove_attribute(div, "dir");
        assert_eq!(cx.directionality(span), Ltr);
    });
}

/// An element moved takes its new parent's, its subtree too; taken out,
/// it has none to take.
#[test]
fn a_moved_subtree_takes_its_new_parents() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        let rtl = element(cx, body, "div");
        cx.set_attribute(rtl, "dir", "rtl");
        let ltr = element(cx, body, "div");
        let moved = cx.create_element(cx.document_of(body), QualName::html("p"));
        let inner = element(cx, moved, "span");
        cx.append_child(rtl, moved).unwrap();
        assert_eq!(cx.directionality(inner), Rtl);
        cx.append_child(ltr, moved).unwrap();
        assert_eq!(cx.directionality(inner), Ltr);
        cx.append_child(rtl, moved).unwrap();
        cx.remove(moved);
        assert_eq!(cx.directionality(inner), Ltr);
    });
}

/// `dir=auto` is its text's first strong character's (`ltr` without one),
/// as the text comes, goes and changes; text in an element with its own
/// `dir` or a `<script>` doesn't count. Its descendants take it.
#[test]
fn auto_is_its_texts() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        cx.set_attribute(body, "dir", "rtl");
        let auto = element(cx, body, "div");
        cx.set_attribute(auto, "dir", "auto");
        let child = element(cx, auto, "span");
        assert_eq!(cx.directionality(auto), Ltr, "no strong character");
        let skipped = element(cx, auto, "b");
        cx.set_attribute(skipped, "dir", "ltr");
        let hebrew = text(cx, skipped, "\u{5d0}");
        let script = element(cx, auto, "script");
        text(cx, script, "\u{5d0}");
        assert_eq!(cx.directionality(auto), Ltr);
        cx.remove_attribute(skipped, "dir");
        assert_eq!(cx.directionality(auto), Rtl, "its text counts now");
        assert_eq!(cx.directionality(child), Rtl);
        cx.replace_data(hebrew, 0, 1, "1 a");
        assert_eq!(cx.directionality(auto), Ltr);
        let first = cx.create_text(cx.document_of(body), "\u{5d0}");
        cx.pre_insert(auto, first, cx.first_child(auto)).unwrap();
        assert_eq!(cx.directionality(auto), Rtl);
        cx.remove(first);
        assert_eq!(cx.directionality(auto), Ltr);
    });
}

/// A `<bdi>` without a valid `dir` is `auto`, made or inserted.
#[test]
fn a_bdi_is_auto() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        let bdi = cx.create_element(cx.document_of(body), QualName::html("bdi"));
        text(cx, bdi, "\u{5d0}");
        assert_eq!(cx.directionality(bdi), Rtl, "before it is inserted");
        cx.append_child(body, bdi).unwrap();
        assert_eq!(cx.directionality(bdi), Rtl);
        cx.set_attribute(bdi, "dir", "ltr");
        assert_eq!(cx.directionality(bdi), Ltr);
    });
}

/// An `auto` form control's is its value's: an input of a text type's,
/// a textarea's; a telephone field without a valid `dir` is `ltr`.
#[test]
fn form_controls() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        cx.set_attribute(body, "dir", "rtl");
        let input = element(cx, body, "input");
        cx.set_attribute(input, "dir", "auto");
        assert_eq!(cx.directionality(input), Ltr, "an empty value");
        cx.set_attribute(input, "value", "\u{5d0}");
        assert_eq!(cx.directionality(input), Rtl);
        cx.set_input_value(input, "a").unwrap();
        assert_eq!(cx.directionality(input), Ltr);
        cx.set_attribute(input, "type", "checkbox");
        assert_eq!(cx.directionality(input), Ltr, "its value doesn't count");
        let textarea = element(cx, body, "textarea");
        cx.set_attribute(textarea, "dir", "auto");
        cx.set_textarea_value(textarea, "\u{5d0}");
        assert_eq!(cx.directionality(textarea), Rtl);
        let tel = element(cx, body, "input");
        cx.set_attribute(tel, "type", "tel");
        assert_eq!(cx.directionality(tel), Ltr);
        cx.set_attribute(tel, "type", "text");
        assert_eq!(cx.directionality(tel), Rtl);
    });
}

/// The top of a shadow tree takes its host's, as the host's changes, and
/// a slot in it its host's whatever its parent's.
#[test]
fn a_shadow_tree_takes_its_hosts() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        let host = element(cx, body, "div");
        let root = cx.attach_shadow(
            host,
            ShadowRootInit {
                mode: ShadowRootMode::Open,
                delegates_focus: false,
                slot_assignment: SlotAssignmentMode::Named,
                clonable: false,
                serializable: false,
            },
        );
        let inside = element(cx, root, "p");
        cx.set_attribute(inside, "dir", "ltr");
        let slot = element(cx, inside, "slot");
        let below = element(cx, inside, "span");
        cx.set_attribute(host, "dir", "rtl");
        assert_eq!(cx.directionality(slot), Rtl, "a slot takes its host's");
        assert_eq!(cx.directionality(below), Ltr);
        cx.remove_attribute(inside, "dir");
        assert_eq!(cx.directionality(inside), Rtl);
        cx.set_attribute(host, "dir", "ltr");
        assert_eq!(cx.directionality(inside), Ltr);
        assert_eq!(cx.directionality(slot), Ltr);
    });
}

/// An `auto` element whose text changes its directionality is noted for
/// style: its `:dir()` state, and styled again for its `direction`.
#[test]
fn a_change_of_text_is_noted_for_style() {
    let mut dom = Dom::new();
    dom.mutate(|cx| {
        let body = body(cx);
        let auto = element(cx, body, "div");
        cx.set_attribute(auto, "dir", "auto");
        let data = text(cx, auto, "a");
        let document = cx.document_of(body);
        let mut at = Some(document);
        while let Some(node) = at {
            cx.clear_style_flags(node);
            at = cx.following_within(node, document);
        }
        cx.replace_data(data, 0, 1, "\u{5d0}");
        assert!(
            cx.style_flags(auto)
                .contains(StyleFlags::NEEDS_STYLE_RECALC)
        );
    });
}
