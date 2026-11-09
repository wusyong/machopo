//! Custom properties and `var()`: computing them, substituting them into
//! other properties, and how CSSOM reads declarations that use them.

use std::collections::BTreeMap;

use computed_style::{
    ComputedStyle, LengthPercentage, LengthPercentageOrAuto, Sides, Size, StyleColor,
};
use gfx::Color;
use style::StyleEngine;
use style_resolver::{CssPropertyValueSet, computed_value, supports_declaration};

use crate::{Page, styles};

/// Custom property `name` of `#id`, as `getComputedStyle` reads it.
fn var(s: &BTreeMap<String, ComputedStyle>, id: &str, name: &str) -> String {
    computed_value(&s[id], name).expect("a custom property")
}

fn px(v: f32) -> Size {
    Size::LengthPercentage(LengthPercentage::Px(v))
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgb(r, g, b)
}

#[test]
fn var_substitutes_and_custom_properties_inherit() {
    let s = styles(
        "<style>
           :root { --c: rgb(1, 2, 3); --w: 10px }
           #inner { --w: 20px }
           div { color: var(--c); width: var(--w) }
         </style>
         <div id=outer><div id=inner><div id=deep></div></div></div>",
    );
    assert_eq!(s["outer"].color, rgb(1, 2, 3));
    assert_eq!(s["outer"].width, px(10.0));
    assert_eq!(s["inner"].width, px(20.0));
    // Inherited from #inner, not the root.
    assert_eq!(s["deep"].width, px(20.0));
    assert_eq!(var(&s, "deep", "--c"), "rgb(1, 2, 3)");
    assert_eq!(var(&s, "deep", "--w"), "20px");
    // Not set: the empty string.
    assert_eq!(var(&s, "deep", "--nope"), "");
}

#[test]
fn fallbacks_and_invalid_at_computed_value_time() {
    let s = styles(
        "<style>
           body { color: rgb(9, 9, 9) }
           #a { width: var(--missing, 5px) }
           #b { width: var(--missing, var(--also-missing, 7px)) }
           #c { color: rgb(1, 1, 1) }
           #c { color: var(--missing) }
           #d { width: 3px }
           #d { width: var(--missing) }
           #e { --bad: red; width: var(--bad) }
           #f { width: var(--missing,) }
           #g { --empty:; width: var(--empty) 4px }
           #h { --x: var(--missing, var(--also-missing)); width: var(--x, 5px) }
         </style>
         <body><p id=a><p id=b><p id=c><p id=d><p id=e><p id=f><p id=g><p id=h></body>",
    );
    assert_eq!(s["a"].width, px(5.0));
    assert_eq!(s["b"].width, px(7.0));
    // The winning declaration doesn't substitute: `unset`, not the
    // earlier rule's red. `color` inherits.
    assert_eq!(s["c"].color, rgb(9, 9, 9));
    // `width` doesn't: its initial value.
    assert_eq!(s["d"].width, Size::Auto);
    // Substitutes, but not as a width.
    assert_eq!(s["e"].width, Size::Auto);
    // An empty fallback: nothing, which isn't a width.
    assert_eq!(s["f"].width, Size::Auto);
    // An empty value substitutes as nothing.
    assert_eq!(s["g"].width, px(4.0));
    assert_eq!(var(&s, "g", "--empty"), "");
    // A fallback that fails fails the whole value: invalid, not empty.
    assert_eq!(s["h"].width, px(5.0));
}

#[test]
fn cycles_are_invalid_and_references_to_them_take_their_fallback() {
    let s = styles(
        "<style>
           #a {
             --one: var(--two);
             --two: var(--one, 1px);
             --self: var(--self, 2px);
             --outside: var(--one, 3px);
             --fine: 4px;
             --uses-fine: var(--fine);
             width: var(--one, 9px);
             height: var(--outside);
           }
         </style>
         <p id=a>",
    );
    assert_eq!(var(&s, "a", "--one"), "");
    // In the cycle: its fallback doesn't help.
    assert_eq!(var(&s, "a", "--two"), "");
    assert_eq!(var(&s, "a", "--self"), "");
    // Not in it: it refers to one in it, and takes its own fallback.
    assert_eq!(var(&s, "a", "--outside"), "3px");
    assert_eq!(var(&s, "a", "--uses-fine"), "4px");
    assert_eq!(s["a"].width, px(9.0));
    assert_eq!(s["a"].height, px(3.0));
}

#[test]
fn custom_properties_cascade_like_any_other() {
    let s = styles(
        "<style>
           div { --w: 1px !important; --h: 1px }
           #a { --w: 2px; --h: 2px }
           #b { --W: 5px }
         </style>
         <div id=a style='--h: 3px; width: var(--w); height: var(--h)'></div>
         <div id=b style='width: var(--W, 6px); height: var(--w)'></div>",
    );
    assert_eq!(s["a"].width, px(1.0));
    assert_eq!(s["a"].height, px(3.0));
    // Names are case-sensitive.
    assert_eq!(s["b"].width, px(5.0));
    assert_eq!(s["b"].height, px(1.0));
}

#[test]
fn css_wide_keywords() {
    let s = styles(
        "<style>
           #outer { --x: 3px; color: rgb(5, 5, 5) }
           #initial { --x: initial; width: var(--x, 1px) }
           #inherit { --x: 4px }
           #inherit > p { --x: inherit; width: var(--x) }
           #unset { --x: 6px }
           #unset > p { --x: unset; width: var(--x) }
           #fallback { color: rgb(1, 1, 1); color: var(--nope, inherit) }
         </style>
         <div id=outer>
           <p id=initial>
           <div id=inherit><p id=inherit-child></div>
           <div id=unset><p id=unset-child></div>
           <p id=fallback>
         </div>",
    );
    // `initial` is the guaranteed-invalid value.
    assert_eq!(s["initial"].width, px(1.0));
    assert_eq!(var(&s, "initial", "--x"), "");
    assert_eq!(s["inherit-child"].width, px(4.0));
    // Custom properties inherit: `unset` is `inherit`.
    assert_eq!(s["unset-child"].width, px(6.0));
    // A CSS-wide keyword from a fallback works as one.
    assert_eq!(s["fallback"].color, rgb(5, 5, 5));
}

#[test]
fn shorthands_substitute_into_each_longhand() {
    let s = styles(
        "<style>
           :root { --m: 1px 2px; --c: rgb(1, 2, 3) }
           #a { margin: var(--m) }
           #b { margin: 9px; margin: var(--m) 3px var(--missing) }
           #c { border: 2px solid var(--c) }
           #d { margin: 7px; margin-left: var(--m) }
         </style>
         <p id=a><p id=b><p id=c><p id=d>",
    );
    let lp = |v| LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v));
    assert_eq!(
        s["a"].margin,
        Sides {
            top: lp(1.0),
            right: lp(2.0),
            bottom: lp(1.0),
            left: lp(2.0),
        }
    );
    // The shorthand doesn't substitute: every longhand is `unset`.
    assert_eq!(s["b"].margin, Sides::all(lp(0.0)));
    assert_eq!(s["c"].border_width, Sides::all(2.0));
    assert_eq!(s["c"].border_color.top, StyleColor::Rgba(rgb(1, 2, 3)));
    // A longhand that doesn't substitute: only it is `unset`.
    assert_eq!(s["d"].margin.top, lp(7.0));
    assert_eq!(s["d"].margin.left, lp(0.0));
}

#[test]
fn substitution_is_of_tokens() {
    let s = styles(
        "<style>
           :root { --n: 10; --unit: px; --len: 5px; --neg: -; --sum: 1px + 2px }
           #a { width: var(--n)px }
           #b { width: 10var(--unit) }
           #c { width: var(--len) }
           #d { --twice: var(--len)var(--len); width: var(--len)var(--n) }
           #e { --cat: a var(--n) b; --tight: var(--neg)var(--n) }
         </style>
         <p id=a><p id=b><p id=c><p id=d><p id=e>",
    );
    // A number then an identifier, not `10px`.
    assert_eq!(s["a"].width, Size::Auto);
    assert_eq!(s["b"].width, Size::Auto);
    assert_eq!(s["c"].width, px(5.0));
    assert_eq!(s["d"].width, Size::Auto);
    // Kept apart where they would run together, as written elsewhere.
    assert_eq!(var(&s, "d", "--twice"), "5px/**/5px");
    assert_eq!(var(&s, "e", "--cat"), "a 10 b");
    assert_eq!(var(&s, "e", "--tight"), "-/**/10");
}

#[test]
fn values_compute_where_they_are_used() {
    let s = styles(
        "<style>
           #outer { font-size: 10px; --w: 2em }
           #inner { font-size: 20px; width: var(--w) }
           #rem { font-size: var(--size) }
           :root { --size: 30px }
         </style>
         <div id=outer><p id=inner></div><p id=rem>",
    );
    // `em` is the element's own, at the `var()`.
    assert_eq!(s["inner"].width, px(40.0));
    assert_eq!(var(&s, "inner", "--w"), "2em");
    assert_eq!(s["rem"].font.size, 30.0);
}

#[test]
fn substitution_has_a_limit() {
    // Each level doubles: 2^30 bytes long at the end, if it were allowed.
    let mut css = String::from("--v0: xx;");
    for i in 1..=30 {
        css.push_str(&format!("--v{i}: var(--v{}) var(--v{});", i - 1, i - 1));
    }
    let s = styles(&format!(
        "<style>#a {{ {css} width: var(--v30, 8px) }}</style><p id=a>"
    ));
    assert_eq!(var(&s, "a", "--v30"), "");
    assert_eq!(var(&s, "a", "--v3").len(), "xx xx xx xx xx xx xx xx".len());
    assert_eq!(s["a"].width, px(8.0));
}

#[test]
fn a_style_attribute_and_presentation_attributes_take_var() {
    let s = styles(
        "<style>:root { --w: 12px; --fill: rgb(1, 2, 3) }</style>
         <p id=a style='width: var(--w)'>
         <svg><rect id=r fill='var(--fill)' /></svg>",
    );
    assert_eq!(s["a"].width, px(12.0));
    assert_eq!(
        s["r"].svg.fill,
        computed_style::SvgPaint::Color(StyleColor::Rgba(rgb(1, 2, 3)))
    );
}

#[test]
fn equal_styles_compare_equal() {
    let s = styles(
        "<style>:root { --a: 1px } .x { --b: 2px }</style>
         <p id=one class=x><p id=two class=x><p id=three>",
    );
    assert_eq!(s["one"], s["two"]);
    assert_ne!(s["one"], s["three"]);
}

#[test]
fn declarations_with_var_keep_their_text() {
    let mut block =
        CssPropertyValueSet::parse("color: var(--c, red); margin: var(--m) 0 !important");
    assert_eq!(block.property_value("color"), "var(--c, red)");
    // A shorthand's value is its own; its longhands have none, as in
    // Chrome.
    assert_eq!(block.property_value("margin"), "var(--m) 0");
    assert_eq!(block.property_value("margin-top"), "");
    assert_eq!(block.property_priority("margin"), "important");
    assert_eq!(
        block.css_text(),
        "color: var(--c, red); margin: var(--m) 0 !important;"
    );
    // One longhand set on its own: the shorthand can't be read or
    // written; the other longhands are written with empty values, as
    // Chrome writes them (which don't parse back: an inline style keeps
    // its declarations, not this text).
    assert!(block.set_property("margin-left", "1px", "important"));
    assert_eq!(block.property_value("margin"), "");
    assert_eq!(
        block.css_text(),
        "color: var(--c, red); margin-top:  !important; margin-right:  !important; \
         margin-bottom:  !important; margin-left: 1px !important;"
    );
    assert!(block.set_property("width", "  calc(var(--w)) ", ""));
    assert_eq!(block.property_value("width"), "calc(var(--w))");
    // `var()`'s own syntax is checked.
    for value in [
        "var(w)",
        "var(--w x)",
        "var()",
        "var(--)",
        "a var(--w) ;",
        // The fallback is a declaration value too.
        "var(--w, !)",
        "var(--w, a;b)",
        "var(--w, !important)",
    ] {
        assert!(!block.set_property("height", value, ""), "{value}");
    }
    assert!(block.set_property("height", "var(--w,)", ""));
    // Unclosed at the end, as any block may be.
    assert!(block.set_property("height", "var(--w, 1px", ""));
    assert!(block.set_property("height", "VAR(--w, var(--h, {a; !}))", ""));
}

#[test]
fn css_supports_custom_properties_and_var() {
    assert!(supports_declaration("--x", "1"));
    assert!(supports_declaration("--x", ""));
    assert!(!supports_declaration("--x", "a;b"));
    assert!(!supports_declaration("--x", ")"));
    assert!(supports_declaration("color", "var(--x)"));
    assert!(supports_declaration("margin", "var(--x) 1px"));
    assert!(!supports_declaration("color", "var(x)"));
    assert!(!supports_declaration("nope", "var(--x)"));
}

#[test]
fn cycles_are_found_past_a_failed_reference_and_end_where_they_end() {
    let s = styles(
        "<style>
           #a {
             --x: var(--y);
             --y: var(--x) var(--z);
             --z: var(--y, 13px);
           }
           #b {
             --x: var(--y);
             --y: var(--x, 1px) var(--out);
             --out: var(--fine);
             --fine: 2px;
           }
           #c { --x: var(--y); --y: var(--x); --z: var(--y, 13px) }
           #d { --y: 5px; --x: var(--y, var(--x)) }
         </style>
         <p id=a><p id=b><p id=c><p id=d>",
    );
    // `--y` fails at `--x` (a cycle) before it gets to `--z`, which is in
    // a cycle with it too.
    assert_eq!(var(&s, "a", "--z"), "");
    // `--out` is resolved while `--x` and `--y`'s cycle is: it isn't in it.
    assert_eq!(var(&s, "b", "--y"), "");
    assert_eq!(var(&s, "b", "--out"), "2px");
    // Outside the cycle, referring to it: its own fallback.
    assert_eq!(var(&s, "c", "--z"), "13px");
    // A fallback that isn't used is no reference.
    assert_eq!(var(&s, "d", "--x"), "5px");
}

#[test]
fn a_css_wide_keyword_after_substitution_is_one() {
    let s = styles(
        "<style>
           #outer { --x: 3px }
           #outer > p { --empty: ; }
           #initial { --x: var(--empty) initial; width: var(--x, 1px) }
           #inherit { --x: var(--nope, inherit); width: var(--x) }
           #literal { --x: var(--empty) initial x }
         </style>
         <div id=outer><p id=initial><p id=inherit><p id=literal></div>",
    );
    assert_eq!(var(&s, "initial", "--x"), "");
    assert_eq!(s["initial"].width, px(1.0));
    assert_eq!(var(&s, "inherit", "--x"), "3px");
    assert_eq!(s["inherit"].width, px(3.0));
    assert_eq!(var(&s, "literal", "--x"), "initial x");
}

#[test]
fn a_value_with_var_drops_comments_around_it() {
    let block = CssPropertyValueSet::parse(
        "color: /* gone */ var(--c)  /* kept */ var(--d) /* gone */ ; margin: /**/ var(--m) /**/",
    );
    assert_eq!(
        block.property_value("color"),
        "var(--c)  /* kept */ var(--d)"
    );
    assert_eq!(block.property_value("margin"), "var(--m)");
}

#[test]
fn css_text_writes_what_it_cant_as_empty_longhands() {
    // A longhand set after its shorthand's `var()` (WPT's
    // `variable-cssText.html`, target9).
    let block = CssPropertyValueSet::parse("margin: var(--m); margin-top: 10px");
    assert_eq!(
        block.css_text(),
        "margin-right: ; margin-bottom: ; margin-left: ; margin-top: 10px;"
    );
    // All of them still the shorthand's: the shorthand.
    let block = CssPropertyValueSet::parse("margin: 1px; margin: var(--m)");
    assert_eq!(block.css_text(), "margin: var(--m);");
}

#[test]
fn once_a_value_fails_its_fallbacks_refer_to_nothing() {
    let s = styles(
        "<style>
           #a {
             --x: var(--y);
             --y: var(--x) var(--nope, var(--z));
             --z: var(--y, 13px);
           }
         </style>
         <p id=a>",
    );
    // `--y` fails at `--x`, so the fallback that would refer to `--z` isn't
    // used, and `--z` is in no cycle with it.
    assert_eq!(var(&s, "a", "--y"), "");
    assert_eq!(var(&s, "a", "--z"), "13px");
}

#[test]
fn styled_again_the_same_custom_properties_keep_their_map() {
    let mut page = Page::new(
        "<style>:root { --a: 1px; --b: 2px } .x { --c: 3px }</style>
         <div id=outer><p id=plain><p id=own class=x></div>",
    );
    // Each engine styles the whole document: new styles, the same values.
    let before: Vec<_> = ["outer", "plain", "own"]
        .iter()
        .map(|id| page.style(id))
        .collect();
    page.engine = StyleEngine::new();
    page.recalc();
    let restyled = page.restyled();
    for (id, old) in ["outer", "plain", "own"].iter().zip(&before) {
        assert!(restyled.contains(&id.to_string()), "#{id}: styled again");
        let new = page.style(id);
        assert!(new.variables.same_map(&old.variables), "#{id}");
    }
}

#[test]
fn the_cascade_reads_the_inline_style_cssom_left() {
    let mut page =
        Page::parsed("<style>:root { --m: 4px }</style><p id=a style='margin: var(--m)'>");
    // As CSSOM leaves it: its text, `margin-top: 10px; margin-right: ;
    // ...`, would parse to `margin-top` alone.
    let mut declarations = CssPropertyValueSet::parse("margin: var(--m)");
    assert!(declarations.set_property("margin-top", "10px", ""));
    page.with("a", |cx, a| {
        cx.set_inline_style(a, std::sync::Arc::new(declarations))
    });
    page.recalc();
    let style = page.style("a");
    let lp = |v| LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v));
    assert_eq!(style.margin.top, lp(10.0));
    assert_eq!(style.margin.left, lp(4.0));
}
