//! `@media`, `@supports`, and the `media` of `<style>` and `<link>`: which
//! rules apply in which environment (viewport and device pixel ratio), and
//! the recalc when it changes.

use std::collections::BTreeMap;
use std::sync::Arc;

use computed_style::{ComputedStyle, LengthPercentage, Size};
use gfx::SizeF;

use crate::Page;

impl Page {
    /// Sets the environment and styles the document: whether the engine
    /// styled anything again for it (nothing the first time, with no
    /// styles to compare against).
    fn style_in(&mut self, width: f32, height: f32, device_pixel_ratio: f32) -> bool {
        let styled = self.all_styles().values().any(Option::is_some);
        self.set_viewport(SizeF::new(width, height), device_pixel_ratio);
        self.recalc();
        styled && !self.restyled().is_empty()
    }

    /// Each element with an id and a style: its width, in px, or `None`
    /// for `auto`.
    fn widths(&mut self) -> BTreeMap<String, Option<f32>> {
        self.styles()
            .into_iter()
            .map(|(id, style)| (id, width(&style)))
            .collect()
    }
}

fn width(style: &ComputedStyle) -> Option<f32> {
    match style.width {
        Size::LengthPercentage(LengthPercentage::Px(px)) => Some(px),
        _ => None,
    }
}

/// The widths of the elements named, in order.
fn of(widths: &BTreeMap<String, Option<f32>>, ids: &[&str]) -> Vec<Option<f32>> {
    ids.iter().map(|id| widths[*id]).collect()
}

#[test]
fn media_rules_apply_while_their_queries_match() {
    let mut page = Page::parsed(
        "<style>
           #a { width: 1px }
           @media (min-width: 500px) { #a { width: 2px } }
           @media (min-width: 500px) { @media (max-width: 900px) { #b { width: 3px } } }
           @media print { #c { width: 4px } }
           @media { #d { width: 5px } }
           @media (fancy), (min-width: 1px) { #e { width: 6px } }
           @media screen and (fancy) { #f { width: 7px } }
           @media (min-resolution: 2dppx) { #g { width: 8px } }
           @media !nonsense { #h { width: 9px } }
           @MEDIA (orientation: landscape) { #i { width: 10px } }
         </style>
         <p id=a><p id=b><p id=c><p id=d><p id=e><p id=f><p id=g><p id=h><p id=i>",
    );
    let ids = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
    page.style_in(800.0, 600.0, 1.0);
    // An empty list matches; a query that doesn't parse is `not all`, the
    // others in its list still count; an unknown feature matches nothing.
    assert_eq!(
        of(&page.widths(), &ids),
        [
            Some(2.0),
            Some(3.0),
            None,
            Some(5.0),
            Some(6.0),
            None,
            None,
            None,
            Some(10.0)
        ]
    );
    page.style_in(1000.0, 1200.0, 2.0);
    assert_eq!(
        of(&page.widths(), &["a", "b", "g", "i"]),
        [Some(2.0), None, Some(8.0), None]
    );
    page.style_in(400.0, 300.0, 1.0);
    assert_eq!(
        of(&page.widths(), &["a", "b", "g"]),
        [Some(1.0), None, None]
    );
}

#[test]
fn supports_rules_apply_if_their_condition_holds() {
    let mut page = Page::parsed(
        "<style>
           @supports (display: grid) { #a { width: 1px } }
           @supports (display: nonsense) { #b { width: 2px } }
           @supports not (display: nonsense) { #c { width: 3px } }
           @supports (display: grid) and (not (color: nonsense)) {
             @media (min-width: 1px) { #d { width: 4px } }
           }
           @supports display: grid { #e { width: 5px } }
           @supports selector(p > a) { #f { width: 6px } }
           @media (min-width: 1px) { @supports (color: red) { #g { width: 7px } } }
           @supports selector(input::-webkit-slider-thumb) { #h { width: 8px } }
           @supports selector(input::-webkit-unknown-pseudo) { #i { width: 9px } }
           @supports selector(::-webkit-unknown-pseudo:hover) { #j { width: 10px } }
         </style>
         <p id=a><p id=b><p id=c><p id=d><p id=e><p id=f><p id=g><p id=h><p id=i><p id=j>",
    );
    page.style_in(800.0, 600.0, 1.0);
    // A condition that doesn't parse drops the rule; a `::-webkit-`
    // pseudo-element Chrome doesn't know parses, but isn't supported.
    assert_eq!(
        of(
            &page.widths(),
            &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]
        ),
        [
            Some(1.0),
            None,
            Some(3.0),
            Some(4.0),
            None,
            Some(6.0),
            Some(7.0),
            Some(8.0),
            None,
            None
        ]
    );
}

#[test]
fn group_rules_keep_their_place_in_the_cascade() {
    let mut page = Page::parsed(
        "<style>
           @media all { #a { width: 1px } }
           #a { width: 2px }
           #b { width: 2px }
           @media all { @supports (color: red) { #b { width: 1px } } }
           @media all { p#c { width: 1px } }
           #c { width: 2px }
         </style>
         <p id=a><p id=b><p id=c>",
    );
    page.style_in(800.0, 600.0, 1.0);
    // Later wins, by source order whatever the nesting; specificity
    // first.
    assert_eq!(
        of(&page.widths(), &["a", "b", "c"]),
        [Some(2.0), Some(1.0), Some(1.0)]
    );
}

#[test]
fn style_and_link_elements_apply_while_their_media_matches() {
    let mut page = Page::parsed(
        "<style id=style-a media='(min-width: 500px)'>#a { width: 1px }</style>
         <style media=print>#b { width: 2px }</style>
         <style media=''>#c { width: 3px }</style>
         <style media='screen, print'>#d { width: 4px }</style>
         <svg><style media=print>#e { width: 5px }</style></svg>
         <link id=wide rel=stylesheet href=wide.css media='(min-width: 500px)'>
         <link id=narrow rel=stylesheet href=narrow.css media='(max-width: 499px)'>
         <p id=a><p id=b><p id=c><p id=d><p id=e><p id=f><p id=g>",
    );
    crate::give_link_sheet(&mut page, "wide", "#f { width: 6px }");
    crate::give_link_sheet(&mut page, "narrow", "#g { width: 7px }");
    page.style_in(800.0, 600.0, 1.0);
    let ids = ["a", "b", "c", "d", "e", "f", "g"];
    assert_eq!(
        of(&page.widths(), &ids),
        [Some(1.0), None, Some(3.0), Some(4.0), None, Some(6.0), None]
    );
    assert!(page.style_in(400.0, 600.0, 1.0));
    assert_eq!(
        of(&page.widths(), &["a", "f", "g"]),
        [None, None, Some(7.0)]
    );

    // Another `media` is another sheet.
    page.with("style-a", |cx, style| {
        cx.set_attribute(style, "media", "(max-width: 499px)");
    });
    page.style_in(400.0, 600.0, 1.0);
    assert_eq!(of(&page.widths(), &["a"]), [Some(1.0)]);
}

#[test]
fn a_new_environment_restyles_only_if_a_query_comes_out_otherwise() {
    let mut page = Page::parsed(
        "<style>
           @media (min-width: 500px) { #a { width: 2px } }
           @media (resolution: 2dppx) { #b { width: 3px } }
         </style>
         <p id=a><p id=b><p id=plain>",
    );
    // Not styled yet: nothing to compare against.
    assert!(!page.style_in(800.0, 600.0, 1.0));
    assert_eq!(of(&page.widths(), &["a", "b"]), [Some(2.0), None]);

    // The same results: no recalc for it, and none styled again.
    let plain = page.style("plain");
    assert!(!page.style_in(900.0, 700.0, 1.0));
    assert!(Arc::ptr_eq(&page.style("plain"), &plain));
    assert!(!page.style_in(900.0, 700.0, 1.0), "the same again");

    // Another result: everything is styled again (the same style object
    // for one whose values are the same).
    assert!(page.style_in(400.0, 700.0, 1.0));
    assert_eq!(of(&page.widths(), &["a", "b"]), [None, None]);
    assert!(page.restyled().contains(&"plain".into()), "styled again");
    assert!(Arc::ptr_eq(&page.style("plain"), &plain), "the same style");

    // The device pixel ratio counts too.
    assert!(page.style_in(400.0, 700.0, 2.0));
    assert_eq!(of(&page.widths(), &["a", "b"]), [None, Some(3.0)]);
}

#[test]
fn adopted_sheets_apply_while_their_media_matches() {
    let mut page = Page::parsed("<p id=a><p id=b>");
    adopt(
        &mut page,
        &[
            ("#a { width: 1px }", "(min-width: 500px)"),
            ("@media (max-width: 499px) { #b { width: 2px } }", ""),
        ],
    );
    page.style_in(800.0, 600.0, 1.0);
    assert_eq!(of(&page.widths(), &["a", "b"]), [Some(1.0), None]);
    // The sheet's media, and the media rule in the other, come out
    // otherwise.
    assert!(page.style_in(400.0, 600.0, 1.0));
    assert_eq!(of(&page.widths(), &["a", "b"]), [None, Some(2.0)]);
    assert!(!page.style_in(300.0, 600.0, 1.0));
    assert!(page.style_in(800.0, 600.0, 1.0));
    assert_eq!(of(&page.widths(), &["a", "b"]), [Some(1.0), None]);

    // Only a sheet's own media comes out otherwise.
    drop(page);
    let mut page = Page::parsed("<p id=a>");
    adopt(&mut page, &[("#a { width: 1px }", "(min-width: 500px)")]);
    page.style_in(800.0, 600.0, 1.0);
    assert!(page.style_in(400.0, 600.0, 1.0));
    assert_eq!(of(&page.widths(), &["a"]), [None]);
}

/// A viewport unit in a pseudo-element's style (`::before`, `::marker`)
/// follows the viewport, though its element's has none.
#[test]
fn a_pseudo_elements_viewport_units_follow_the_viewport() {
    let mut page = Page::parsed(
        "<style>p::before { content: 'x'; font-size: 2vw } li::marker { font-size: 3vw }</style>
         <p id=p></p><ul><li id=li></li></ul>",
    );
    let font_size = |page: &mut Page, id: &str, pseudo: computed_style::PseudoId| {
        page.pseudo(id)
            .and_then(|styles| styles.get(pseudo).cloned())
            .expect("styled")
            .font
            .size
    };
    use computed_style::PseudoId::{Before, Marker};
    page.style_in(500.0, 400.0, 1.0);
    assert_eq!(font_size(&mut page, "p", Before), 10.0);
    assert_eq!(font_size(&mut page, "li", Marker), 15.0);
    assert!(page.style_in(1000.0, 400.0, 1.0), "a viewport unit is used");
    assert_eq!(font_size(&mut page, "p", Before), 20.0);
    assert_eq!(font_size(&mut page, "li", Marker), 30.0);
    // A marker's alone.
    drop(page);
    let mut page =
        Page::parsed("<style>li::marker { font-size: 3vw }</style><ul><li id=li></li></ul>");
    page.style_in(500.0, 400.0, 1.0);
    assert!(page.style_in(1000.0, 400.0, 1.0), "a viewport unit is used");
    assert_eq!(font_size(&mut page, "li", Marker), 30.0);
}

/// The document given constructed sheets of each `(css, media)`, adopted
/// in order.
fn adopt(page: &mut Page, sheets: &[(&str, &str)]) {
    page.with_document(|cx, document| {
        for (at, (css, media)) in sheets.iter().enumerate() {
            let sheet = cx.create_style_sheet(document, crate::sheet(css), media, false);
            cx.set_adopted_style_sheet(document, at, sheet);
        }
    });
}
