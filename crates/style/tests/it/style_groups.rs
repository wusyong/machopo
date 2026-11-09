//! `ComputedStyle`'s shared groups: the rare non-inherited properties, the
//! rare inherited ones, the font, the background layers' lists and the SVG
//! properties
//! are each one `Arc` that styles share until one of them is set, and
//! compare by their values.

use std::sync::Arc;

use computed_style::ComputedStyle;

use crate::Page;

/// `html` styled, and the computed style of each element with an id.
fn styles(html: &str) -> std::collections::BTreeMap<String, Arc<ComputedStyle>> {
    Page::new(html).styles()
}

/// Elements that set none of a group's properties share one copy of it,
/// however else they differ; one that sets one has its own.
#[test]
fn a_group_is_shared_until_one_of_its_properties_is_set() {
    let s = styles(
        "<!DOCTYPE html><div id=a style='width: 1px'></div><p id=b style='color: red'></p>\
         <div id=grid style='display: grid; grid-template-columns: 1px'></div>\
         <div id=bg style='background-image: linear-gradient(red, blue)'></div>\
         <div id=border style='border: 1px solid'></div>\
         <div id=image style='border: 1px solid; border-image-slice: 10'></div>",
    );
    assert!(Arc::ptr_eq(&s["a"].rare, &s["b"].rare));
    assert!(Arc::ptr_eq(&s["a"].backgrounds, &s["b"].backgrounds));
    assert!(!Arc::ptr_eq(&s["a"].rare, &s["grid"].rare), "its own grid");
    assert!(Arc::ptr_eq(&s["a"].backgrounds, &s["grid"].backgrounds));
    assert!(!Arc::ptr_eq(&s["a"].backgrounds, &s["bg"].backgrounds));
    assert!(Arc::ptr_eq(&s["a"].rare, &s["bg"].rare));
    // `border` resets `border-image` to what it is already.
    assert!(Arc::ptr_eq(&s["a"].rare, &s["border"].rare));
    assert!(!Arc::ptr_eq(&s["a"].rare, &s["image"].rare));
}

/// The SVG properties of an element whose own are its parent's (the
/// inherited ones, and the rest initial) are its parent's copy.
#[test]
fn the_svg_properties_are_the_parents_if_the_same() {
    let s = styles(
        "<!DOCTYPE html><div id=a></div><div id=red style='fill: red'><span id=child></span>\
         <span id=x style='stroke: blue'></span></div>",
    );
    assert!(!Arc::ptr_eq(&s["a"].svg, &s["red"].svg), "its own fill");
    assert!(Arc::ptr_eq(&s["red"].svg, &s["child"].svg), "the parent's");
    assert!(!Arc::ptr_eq(&s["red"].svg, &s["x"].svg), "its own stroke");
    assert_eq!(s["child"].svg.fill, s["red"].svg.fill);
}

/// The rare inherited properties are the parent's copy, through any
/// number of elements that set none of them, a text decoration's line
/// included (the decorations its box draws are among them); one that sets
/// one has its own, which its children share.
#[test]
fn the_rare_inherited_properties_are_the_parents_until_one_is_set() {
    let s = styles(
        "<!DOCTYPE html><div id=a><p id=b style='color: red'><span id=c></span></p>\
         <div id=indent style='text-indent: 5px'><span id=d></span>\
         <span id=under style='text-decoration: underline'></span>\
         <span id=float style='float: left'></span></div></div>",
    );
    assert!(Arc::ptr_eq(&s["a"].rare_inherited, &s["b"].rare_inherited));
    assert!(Arc::ptr_eq(&s["a"].rare_inherited, &s["c"].rare_inherited));
    assert!(!Arc::ptr_eq(
        &s["a"].rare_inherited,
        &s["indent"].rare_inherited
    ));
    assert!(Arc::ptr_eq(
        &s["indent"].rare_inherited,
        &s["d"].rare_inherited
    ));
    assert!(
        !Arc::ptr_eq(&s["indent"].rare_inherited, &s["under"].rare_inherited),
        "its line"
    );
    assert!(Arc::ptr_eq(
        &s["indent"].rare_inherited,
        &s["float"].rare_inherited
    ));
}

/// The font properties are the parent's copy until one is set to another
/// value: `font: inherit` and a size the same as the parent's keep it.
#[test]
fn the_font_is_the_parents_until_one_differs() {
    let s = styles(
        "<!DOCTYPE html><div id=a style='font-size: 20px'><p id=b style='color: red'></p>\
         <p id=c style='font: inherit'></p><p id=d style='font-size: 20px'></p>\
         <p id=e style='font-weight: bold'></p></div>",
    );
    assert!(Arc::ptr_eq(&s["a"].font, &s["b"].font));
    assert!(Arc::ptr_eq(&s["a"].font, &s["c"].font), "font: inherit");
    assert!(Arc::ptr_eq(&s["a"].font, &s["d"].font), "the same size");
    assert!(!Arc::ptr_eq(&s["a"].font, &s["e"].font), "its own weight");
}

/// Styles compare by the groups' values: a copy of a group made to set
/// nothing new is the same style.
#[test]
fn styles_compare_by_the_groups_values() {
    let initial = ComputedStyle::initial();
    let mut copied = initial.clone();
    copied.rare_mut();
    copied.backgrounds_mut();
    copied.svg_mut();
    copied.rare_inherited_mut();
    assert!(!Arc::ptr_eq(&initial.rare, &copied.rare));
    assert!(!Arc::ptr_eq(
        &initial.rare_inherited,
        &copied.rare_inherited
    ));
    assert!(initial.same_inherited(&copied));
    assert_eq!(initial, copied);
    copied.rare_mut().counter_reset = Arc::new([]);
    assert_eq!(initial, copied, "the initial value again");
    copied.rare_mut().grid_row_start = computed_style::GridLine::Span(2);
    assert_ne!(initial, copied);
}
