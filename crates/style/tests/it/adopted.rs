//! Adopted sheets (`adoptedStyleSheets`): a document's after its own, a
//! shadow root's after its tree's; a sheet replaced, disabled or taken out
//! styles again what its old and new rules find, in every scope that
//! adopted it; shadow roots adopting one sheet share its index.

use std::sync::Arc;

use css::StyleSheetContents;
use dom::{Cx, NodeRef};

use crate::{Page, attach_shadow, find, restyled};

fn px(px: f32) -> computed_style::Size {
    computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(px))
}

fn width(page: &mut Page, id: &str) -> computed_style::Size {
    let Page {
        dom,
        document,
        token,
        ..
    } = page;
    dom.read(|cx| {
        let element = find(cx, document.get(cx.gc), id).expect("an element with the id");
        cx.computed_style(element, token).expect("styled").width
    })
}

type Sheet = elise::Persistent<dom::CssStyleSheet>;

/// A new constructed sheet of `css`, rooted.
fn sheet(page: &mut Page, css: &str) -> Sheet {
    let document = &page.document;
    page.dom.mutate(|cx| {
        let sheet = cx.create_style_sheet(
            document.get(cx.gc),
            Arc::new(StyleSheetContents::parse(css, None)),
            "",
            false,
        );
        elise::Persistent::new(cx.gc, sheet)
    })
}

/// `sheet` adopted by `scope`, at the end.
fn adopt<'gc>(cx: &Cx<'_, 'gc>, scope: NodeRef<'gc>, sheet: &Sheet) {
    let at = cx.adopted_style_sheets(scope).len();
    cx.set_adopted_style_sheet(scope, at, sheet.get(cx.gc));
}

#[test]
fn a_documents_adopted_sheet_styles_what_its_rules_match() {
    let mut page = Page::new(
        "<!doctype html><style>p { width: 1px }</style><p id=one class=a></p><p id=two></p>",
    );
    let s = sheet(&mut page, ".a { width: 2px }");
    // After the document's own: the same specificity, later.
    assert_eq!(
        restyled(&mut page, |cx, document| adopt(cx, document, &s)),
        ["one"]
    );
    assert_eq!(width(&mut page, "one"), px(2.0));
    // Replaced: what its old and new rules find.
    let changed = restyled(&mut page, |cx, _| {
        cx.replace_style_sheet_contents(s.get(cx.gc), crate::sheet("#two { width: 3px }"));
    });
    assert_eq!(changed, ["one", "two"]);
    assert_eq!(width(&mut page, "one"), px(1.0));
    assert_eq!(width(&mut page, "two"), px(3.0));
    // Disabled, enabled again, taken out.
    assert_eq!(
        restyled(&mut page, |cx, _| cx
            .set_style_sheet_disabled(s.get(cx.gc), true)),
        ["two"]
    );
    assert_eq!(width(&mut page, "two"), px(1.0));
    assert_eq!(
        restyled(&mut page, |cx, _| cx
            .set_style_sheet_disabled(s.get(cx.gc), false)),
        ["two"]
    );
    assert_eq!(width(&mut page, "two"), px(3.0));
    assert_eq!(
        restyled(&mut page, |cx, document| cx
            .remove_last_adopted_style_sheet(document)),
        ["two"]
    );
    assert_eq!(width(&mut page, "two"), px(1.0));
    // Changed once out: nothing.
    assert!(
        restyled(&mut page, |cx, _| cx
            .set_style_sheet_disabled(s.get(cx.gc), true))
        .is_empty()
    );
    drop(s);
    drop(page);
}

/// Shadow roots adopting one sheet: each its own tree's elements and host,
/// one index between them; the sheet replaced styles again what its rules
/// find in each.
#[test]
fn shadow_roots_adopting_a_sheet_share_it() {
    let mut page =
        Page::new("<!doctype html><div id=h1></div><div id=h2></div><p id=outside class=a></p>");
    let s = sheet(&mut page, ".a { width: 2px } :host { width: 5px }");
    restyled(&mut page, |cx, document| {
        let h1 = find(cx, document, "h1").unwrap();
        let h2 = find(cx, document, "h2").unwrap();
        attach_shadow(cx, h1, "<p id=p1 class=a></p><i id=i1></i>");
        attach_shadow(cx, h2, "<p id=p2 class=a></p>");
        adopt(cx, cx.shadow_root(h1).unwrap(), &s);
        adopt(cx, cx.shadow_root(h2).unwrap(), &s);
    });
    assert_eq!(width(&mut page, "p1"), px(2.0));
    assert_eq!(width(&mut page, "p2"), px(2.0));
    assert_eq!(width(&mut page, "h1"), px(5.0));
    assert_eq!(
        width(&mut page, "outside"),
        computed_style::Size::Auto,
        "not the document's"
    );
    assert_eq!(page.engine.shadow_indexes_for_testing(), 1);
    let changed = restyled(&mut page, |cx, _| {
        cx.replace_style_sheet_contents(s.get(cx.gc), crate::sheet(".a { width: 3px }"));
    });
    assert_eq!(
        changed,
        ["h1", "h2", "p1", "p2"],
        "the hosts for `:host`, the `.a`s, not `i1`"
    );
    assert_eq!(width(&mut page, "p2"), px(3.0));
    assert_eq!(width(&mut page, "h1"), computed_style::Size::Auto);
    // Disabled: its rules' elements alone, in each.
    assert_eq!(
        restyled(&mut page, |cx, _| cx
            .set_style_sheet_disabled(s.get(cx.gc), true)),
        ["p1", "p2"]
    );
    assert_eq!(width(&mut page, "p1"), computed_style::Size::Auto);
    drop(s);
    drop(page);
}

/// A shadow root's adopted sheet comes after its tree's `<style>`.
#[test]
fn a_shadow_roots_adopted_sheet_comes_after_its_style() {
    let mut page = Page::new("<!doctype html><div id=h></div>");
    let s = sheet(&mut page, "p { width: 2px }");
    restyled(&mut page, |cx, document| {
        let h = find(cx, document, "h").unwrap();
        attach_shadow(cx, h, "<style>p { width: 1px }</style><p id=p></p>");
        adopt(cx, cx.shadow_root(h).unwrap(), &s);
    });
    assert_eq!(width(&mut page, "p"), px(2.0));
    drop(s);
    drop(page);
}
