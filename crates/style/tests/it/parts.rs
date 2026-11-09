//! A form control's UA shadow tree's parts, matched as their host's
//! pseudo-elements: the UA's rules for them (`input::-internal-inner-
//! editor`, `::placeholder`), the author's (the host's tree's: the
//! document's, or the shadow tree's it is in, not another), styled again
//! with their host; a control's native look (`effective_appearance`), its
//! parts' too, none where the author styles it.

use computed_style::{ComputedStyle, Display, EffectiveAppearance, WhiteSpaceCollapse};
use dom::{ShadowRootInit, ShadowRootMode, SlotAssignmentMode};
use gfx::Color;

use crate::Page;

/// The style of `part` of the control whose `id` is `id` (its UA shadow
/// tree built first, if it has none): its inner editor if `editor`, else
/// its placeholder.
fn part_style(page: &mut Page, id: &str, editor: bool) -> std::sync::Arc<ComputedStyle> {
    page.with(id, |cx, control| cx.ensure_shadow_tree(control));
    page.recalc();
    let token = &page.token;
    let document = &page.document;
    page.dom.read(|cx| {
        let control = crate::by_id(cx, document.get(cx.gc), id).expect("the control");
        let node = if editor {
            cx.inner_editor(control)
        } else {
            cx.placeholder_element(control)
        }
        .expect("the part");
        cx.computed_style(node, token).expect("styled").clone()
    })
}

#[test]
fn the_uas_and_the_authors_rules() {
    let mut page = Page::new(
        "<!doctype html><style>input::placeholder { color: rgb(1, 2, 3) } .other::placeholder { color: rgb(9, 9, 9) }</style>\
         <input id=t placeholder=p><textarea id=a placeholder=p></textarea><div id=host></div>",
    );
    // The UA's: an inner editor is a block, a text field's on one line.
    let style = part_style(&mut page, "t", true);
    assert_eq!(style.display, Display::Block);
    assert_eq!(style.white_space_collapse, WhiteSpaceCollapse::Preserve);
    // The author's beat the UA's color; another class's doesn't match; the
    // inner editor isn't the placeholder.
    assert_eq!(part_style(&mut page, "t", false).color, Color::rgb(1, 2, 3));
    assert_ne!(part_style(&mut page, "t", true).color, Color::rgb(1, 2, 3));
    // A text area's placeholder: the UA's color (the author's rule is an
    // input's), its line breaks kept.
    let area = part_style(&mut page, "a", false);
    assert_eq!(area.color, Color::rgb(117, 117, 117));
    assert_eq!(area.text_wrap_mode, computed_style::TextWrapMode::Wrap);
    assert_eq!(part_style(&mut page, "a", true).display, Display::Block);
    // A control in a shadow tree: that tree's rules, not the document's.
    page.with("host", |cx, host| {
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
        let document = cx.document_of(host);
        let style = cx.create_element(document, dom::QualName::html("style"));
        let text = cx.create_text(document, "input::placeholder { color: rgb(4, 5, 6) }");
        cx.append_child(style, text).unwrap();
        cx.append_child(root, style).unwrap();
        let inner = cx.create_element(document, dom::QualName::html("input"));
        cx.set_attribute(inner, "id", "inner");
        cx.set_attribute(inner, "placeholder", "p");
        cx.append_child(root, inner).unwrap();
        cx.ensure_shadow_tree(inner);
    });
    page.recalc();
    let token = &page.token;
    let document = &page.document;
    let color = page.dom.read(|cx| {
        let host = crate::by_id(cx, document.get(cx.gc), "host").unwrap();
        let root = cx.shadow_root(host).unwrap();
        let inner = crate::by_id(cx, root, "inner").unwrap();
        let holder = cx.placeholder_element(inner).unwrap();
        cx.computed_style(holder, token).unwrap().color
    });
    assert_eq!(color, Color::rgb(4, 5, 6));
}

#[test]
fn a_part_is_styled_again_with_its_host() {
    let mut page = Page::new(
        "<!doctype html><style>input.on::placeholder { color: rgb(1, 2, 3) } input:disabled::placeholder { color: rgb(4, 5, 6) }</style>\
         <input id=t placeholder=p>",
    );
    assert_ne!(part_style(&mut page, "t", false).color, Color::rgb(1, 2, 3));
    page.with("t", |cx, t| cx.set_attribute(t, "class", "on"));
    assert_eq!(part_style(&mut page, "t", false).color, Color::rgb(1, 2, 3));
    page.with("t", |cx, t| cx.set_attribute(t, "disabled", ""));
    assert_eq!(part_style(&mut page, "t", false).color, Color::rgb(4, 5, 6));
}

#[test]
fn a_controls_native_look() {
    let mut page = Page::new(
        "<!doctype html><style>#styled { background: red } #none { appearance: none }</style>\
         <input id=t><input id=c type=checkbox><input id=r type=range><input id=b type=submit><button id=bb></button>\
         <textarea id=a></textarea><select id=s></select><select id=l size=3></select><input id=h type=hidden>\
         <input id=styled><input id=none type=checkbox><input id=q type=search><div id=d></div>",
    );
    let look = |page: &mut Page, id: &str| page.style(id).effective_appearance;
    assert_eq!(look(&mut page, "t"), EffectiveAppearance::TextField);
    assert_eq!(look(&mut page, "c"), EffectiveAppearance::Checkbox);
    assert_eq!(look(&mut page, "r"), EffectiveAppearance::SliderHorizontal);
    assert_eq!(look(&mut page, "b"), EffectiveAppearance::PushButton);
    assert_eq!(look(&mut page, "bb"), EffectiveAppearance::PushButton);
    assert_eq!(look(&mut page, "a"), EffectiveAppearance::TextArea);
    assert_eq!(look(&mut page, "s"), EffectiveAppearance::Menulist);
    assert_eq!(look(&mut page, "l"), EffectiveAppearance::Listbox);
    assert_eq!(look(&mut page, "h"), EffectiveAppearance::None);
    assert_eq!(
        look(&mut page, "styled"),
        EffectiveAppearance::None,
        "the author's background"
    );
    assert_eq!(look(&mut page, "none"), EffectiveAppearance::None);
    assert_eq!(look(&mut page, "d"), EffectiveAppearance::None);
    // A check box is the theme's size.
    assert_eq!(
        page.style("c").width,
        computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Px(13.0))
    );
    // A search field's clear button, a part.
    page.with("q", |cx, q| cx.ensure_shadow_tree(q));
    page.recalc();
    let token = &page.token;
    let document = &page.document;
    let cancel = page.dom.read(|cx| {
        let q = crate::by_id(cx, document.get(cx.gc), "q").unwrap();
        let container = cx
            .first_child(cx.user_agent_shadow_root(q).unwrap())
            .unwrap();
        let cancel = cx.last_child(container).unwrap();
        cx.computed_style(cancel, token)
            .unwrap()
            .effective_appearance
    });
    assert_eq!(cancel, EffectiveAppearance::SearchFieldCancelButton);
}
