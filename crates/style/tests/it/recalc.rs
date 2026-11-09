//! What a recalc styles again: nothing when nothing changed (every style
//! the same `Arc`), a changed element with its subtree and not its
//! siblings, everything when a sheet changed; the flags cleared.

use std::sync::Arc;

use dom::StyleFlags;
use gfx::Color;

use crate::Page;

const PAGE: &str = "<!doctype html><style>.a { color: rgb(0, 0, 255) } .a span { color: rgb(255, 0, 0) }</style>\
    <div id=d><span id=s>s</span></div><div id=o><span id=os>o</span></div>";

#[test]
fn nothing_changed_styles_nothing() {
    let mut page = Page::new(PAGE);
    let before = [page.style("d"), page.style("s"), page.style("o")];
    page.recalc();
    let after = [page.style("d"), page.style("s"), page.style("o")];
    for (a, b) in before.iter().zip(&after) {
        assert!(Arc::ptr_eq(a, b));
    }
    let clean = page.with("d", |cx, d| cx.style_flags(cx.document_of(d)));
    assert_eq!(clean, StyleFlags::CLEAN);
}

#[test]
fn a_changed_element_is_styled_again_with_its_subtree() {
    let mut page = Page::new(PAGE);
    let other = page.style("o");
    // Laid out since the first recalc.
    for id in ["d", "o"] {
        page.with(id, |cx, node| cx.clear_layout_tree_flags(node));
    }
    page.with("d", |cx, d| cx.set_attribute(d, "class", "a"));
    page.recalc();
    assert_eq!(page.style("d").color, Color::rgb(0, 0, 255));
    assert_eq!(page.style("s").color, Color::rgb(255, 0, 0), "its subtree");
    assert!(Arc::ptr_eq(&other, &page.style("o")), "not its sibling");
    let flags = page.with("d", |cx, d| cx.style_flags(d));
    assert_eq!(flags, StyleFlags::CLEAN);
    // Layout is told of the style that changed, not of the one that didn't.
    let changed = |page: &mut Page, id| {
        page.with(id, |cx, node| {
            let changed = cx
                .layout_tree_flags(node)
                .contains(dom::LayoutTreeFlags::STYLE_CHANGED);
            cx.clear_layout_tree_flags(node);
            changed
        })
    };
    assert!(changed(&mut page, "d"));
    assert!(!changed(&mut page, "o"));
    page.with("d", |cx, d| cx.set_attribute(d, "title", "same style"));
    page.recalc();
    assert!(!changed(&mut page, "d"), "styled again, the same");
}

#[test]
fn a_sheet_that_changed_styles_what_its_rules_match() {
    let mut page = Page::new(
        "<!doctype html><style id=sheet>p { color: rgb(255, 0, 0) }</style><p id=p>p</p>",
    );
    assert_eq!(page.style("p").color, Color::rgb(255, 0, 0));
    page.with("sheet", |cx, style| {
        let text = cx.first_child(style).unwrap();
        cx.set_data(text, "p { color: rgb(0, 128, 0) }");
    });
    page.recalc();
    assert_eq!(page.style("p").color, Color::rgb(0, 128, 0));
    // A sheet that went.
    page.with("sheet", |cx, style| cx.remove(style));
    page.recalc();
    assert_eq!(page.style("p").color, Color::rgb(0, 0, 0));
}

#[test]
fn an_inserted_element_is_styled() {
    let mut page = Page::new(PAGE);
    page.with("d", |cx, d| {
        let document = cx.document_of(d);
        let span = cx.create_element(document, dom::QualName::html("span"));
        cx.set_attribute(span, "id", "new");
        cx.append_child(d, span).unwrap();
        cx.set_attribute(d, "class", "a");
    });
    page.recalc();
    assert_eq!(page.style("new").color, Color::rgb(255, 0, 0));
}

#[test]
fn the_style_attribute_is_parsed_again_as_it_changes() {
    let mut page = Page::new("<!doctype html><p id=p style='color: rgb(255, 0, 0)'>p</p>");
    assert_eq!(page.style("p").color, Color::rgb(255, 0, 0));
    page.with("p", |cx, p| {
        cx.set_attribute(p, "style", "color: rgb(0, 0, 255)")
    });
    page.recalc();
    assert_eq!(page.style("p").color, Color::rgb(0, 0, 255));
    page.with("p", |cx, p| {
        cx.remove_attribute(p, "style");
    });
    page.recalc();
    assert_eq!(page.style("p").color, Color::rgb(0, 0, 0));
}

/// CSSOM's declarations are what style takes (the element's one copy),
/// the attribute stale until read; the attribute set again replaces them.
#[test]
fn cssoms_declarations_are_styled() {
    let mut page =
        Page::new("<!doctype html><p id=p style='color: rgb(255, 0, 0)'>p</p><span id=s>s</span>");
    page.with("p", |cx, p| {
        let mut block = cx.take_inline_style(p);
        std::sync::Arc::make_mut(&mut block).set_property("color", "rgb(0, 128, 0)", "");
        cx.set_inline_style(p, block);
    });
    page.recalc();
    assert_eq!(page.style("p").color, Color::rgb(0, 128, 0));
    assert_eq!(
        page.style("s").color,
        Color::rgb(0, 0, 0),
        "a sibling as it was"
    );
    page.with("p", |cx, p| {
        cx.set_attribute(p, "style", "color: rgb(0, 0, 255)")
    });
    page.recalc();
    assert_eq!(page.style("p").color, Color::rgb(0, 0, 255));
    // An element with no attribute.
    page.with("s", |cx, s| {
        let mut block = cx.take_inline_style(s);
        std::sync::Arc::make_mut(&mut block).set_property("color", "rgb(1, 2, 3)", "");
        cx.set_inline_style(s, block);
    });
    page.recalc();
    assert_eq!(page.style("s").color, Color::rgb(1, 2, 3));
}
