//! Interpolating computed values: each longhand's animation type, and the
//! value at a progress between two styles, read back as
//! `getComputedStyle` writes it (the values Chrome's
//! `*-interpolation.html` tests in WPT expect, where machopo has the value).

use computed_style::ComputedStyle;
use css::interpolate::{interpolable, interpolate, same_value};
use css::properties::LonghandId;
use style_resolver::{computable_properties, computed_value};

use crate::Page;

/// The style of `<div id=t style="{css}">` in `<div style="{parent}">`.
fn style_in(parent: &str, css: &str) -> ComputedStyle {
    let html = format!("<div style=\"{parent}\"><div id=t style=\"{css}\"></div></div>");
    (*Page::new(&html).style("t")).clone()
}

fn style(css: &str) -> ComputedStyle {
    style_in("", css)
}

fn id(name: &str) -> LonghandId {
    LonghandId::from_name(name).expect("a longhand")
}

/// `property`'s computed value at `progress` from `from` to `to`.
fn at(property: &str, from: &str, to: &str, progress: f64) -> String {
    let (a, b) = (
        style(&format!("{property}: {from}")),
        style(&format!("{property}: {to}")),
    );
    let mut out = a.clone();
    interpolate(id(property), &a, &b, progress, &mut out);
    computed_value(&out, property).expect("a longhand")
}

#[track_caller]
fn assert_at(property: &str, from: &str, to: &str, cases: &[(f64, &str)]) {
    for &(progress, expected) in cases {
        assert_eq!(
            at(property, from, to, progress),
            expected,
            "{property} from {from} to {to} at {progress}"
        );
    }
}

/// Whether `property`'s values `from` and `to` interpolate.
fn interpolates(property: &str, from: &str, to: &str) -> bool {
    let (a, b) = (
        style(&format!("{property}: {from}")),
        style(&format!("{property}: {to}")),
    );
    interpolable(id(property), &a, &b)
}

/// Every longhand has an animation type and goes through each operation;
/// a style has the same value as itself.
#[test]
fn every_longhand_animates_somehow() {
    let a = ComputedStyle::initial();
    let b = style(
        "width: 10px; color: red; opacity: 0.5; transform: scale(2); visibility: hidden; \
         box-shadow: 1px 1px red; display: block; transition: color 1s",
    );
    for name in computable_properties() {
        let id = id(name);
        assert!(same_value(id, &a, &a), "{name}: the same as itself");
        let _ = id.animation_type();
        let _ = interpolable(id, &a, &b);
        for progress in [-0.5, 0.0, 0.3, 0.5, 1.0, 1.5] {
            let mut out = a.clone();
            interpolate(id, &a, &b, progress, &mut out);
            assert!(computed_value(&out, name).is_some(), "{name}");
        }
    }
    assert!(!same_value(id("width"), &a, &b));
    assert!(!same_value(id("color"), &a, &b));
}

/// `caret-color`: colors as colors, `auto` steps (css-ui 4; WPT's
/// `caret-color-interpolation.html`).
#[test]
fn caret_color() {
    assert_at(
        "caret-color",
        "rgb(0, 0, 0)",
        "rgb(200, 100, 0)",
        &[(0.5, "rgb(100, 50, 0)")],
    );
    assert!(!interpolates("caret-color", "auto", "rgb(200, 100, 0)"));
    // `auto` reads back as `color` (black here).
    assert_eq!(
        at("caret-color", "auto", "rgb(200, 100, 0)", 0.4),
        "rgb(0, 0, 0)"
    );
    assert_eq!(
        at("caret-color", "auto", "rgb(200, 100, 0)", 0.6),
        "rgb(200, 100, 0)"
    );
}

/// `text-size-adjust`'s percentages interpolate, never below 0 (WPT's
/// `text-size-adjust-interpolation.html`), `auto` steps;
/// `-webkit-tap-highlight-color` as a color; the properties machopo only
/// parses otherwise step.
#[test]
fn the_properties_parsed_only() {
    assert_at(
        "text-size-adjust",
        "70%",
        "50%",
        &[(-2.0, "110%"), (0.3, "64%"), (1.5, "40%"), (5.0, "0%")],
    );
    assert!(!interpolates("text-size-adjust", "auto", "50%"));
    assert_eq!(at("text-size-adjust", "auto", "50%", 0.6), "50%");
    assert_at(
        "-webkit-tap-highlight-color",
        "rgb(0, 0, 0)",
        "rgb(200, 100, 0)",
        &[(0.5, "rgb(100, 50, 0)")],
    );
    assert_eq!(at("user-select", "none", "text", 0.4), "none");
    assert_eq!(at("user-select", "none", "text", 0.6), "text");
}

/// `border-image-slice`, `-width` and `-outset` side by side, numbers
/// with numbers, lengths and percentages with their kind, never below 0
/// (WPT's `border-image-*-interpolation.html`); another kind on a side, or
/// another `fill`, steps, as the source and the repeat do.
#[test]
fn border_images() {
    assert_at(
        "border-image-slice",
        "0% 10% 20% 30%",
        "40% 50% 60% 70%",
        &[
            (-0.5, "0% 0% 0% 10%"),
            (0.3, "12% 22% 32% 42%"),
            (1.5, "60% 70% 80% 90%"),
        ],
    );
    assert_at(
        "border-image-slice",
        "10 fill",
        "20 fill",
        &[(0.5, "15 fill")],
    );
    assert!(!interpolates("border-image-slice", "10", "20 fill"));
    assert!(!interpolates("border-image-slice", "10", "20%"));
    assert_at(
        "border-image-width",
        "0px",
        "20px",
        &[(-0.3, "0px"), (0.5, "10px")],
    );
    assert_at("border-image-width", "1 2", "3 4", &[(0.5, "2 3")]);
    assert_at(
        "border-image-width",
        "10%",
        "20px",
        &[(0.5, "calc(5% + 10px)")],
    );
    assert!(!interpolates("border-image-width", "1", "20px"));
    assert!(!interpolates("border-image-width", "auto", "20px"));
    assert_at(
        "border-image-outset",
        "0",
        "2",
        &[(-0.3, "0"), (0.75, "1.5")],
    );
    assert_at("border-image-outset", "0px", "10px", &[(0.5, "5px")]);
    assert!(!interpolates("border-image-outset", "1", "10px"));
    assert!(!interpolates("border-image-repeat", "round", "space"));
    assert_eq!(at("border-image-repeat", "round", "space", 0.6), "space");
    assert!(!interpolates("border-image-source", "none", "url(a.png)"));
}

/// `font-stretch`'s percentages, never below 0; `font-variation-settings`
/// axis by axis when both have the same ones; `font-size-adjust`'s numbers
/// for the same metric (WPT's `css/css-fonts/animations/`). The other font
/// longhands step.
#[test]
fn font_longhands() {
    assert_at(
        "font-stretch",
        "50%",
        "150%",
        &[(0.5, "100%"), (-1.0, "0%"), (0.25, "75%")],
    );
    assert_at("font-stretch", "condensed", "expanded", &[(0.5, "100%")]);
    assert_at(
        "font-variation-settings",
        "'wght' 100, 'wdth' 50",
        "'wdth' 150, 'wght' 300",
        &[(0.5, "\"wdth\" 100, \"wght\" 200")],
    );
    assert!(!interpolates(
        "font-variation-settings",
        "'wght' 100",
        "'wdth' 100"
    ));
    assert!(!interpolates(
        "font-variation-settings",
        "normal",
        "'wght' 100"
    ));
    assert_at("font-size-adjust", "0.5", "1.5", &[(0.5, "1"), (-1.0, "0")]);
    assert!(!interpolates("font-size-adjust", "0.5", "cap-height 1"));
    assert!(!interpolates("font-size-adjust", "none", "0.5"));
    assert!(!interpolates(
        "font-feature-settings",
        "'liga' 0",
        "'liga' 1"
    ));
    assert_eq!(
        at("font-variant-caps", "normal", "small-caps", 0.6),
        "small-caps"
    );
}

/// Lengths in px, percentages, and a mix as `calc()`; `auto` steps.
#[test]
fn lengths_and_percentages() {
    assert_at(
        "width",
        "10px",
        "30px",
        &[(0.0, "10px"), (0.25, "15px"), (1.0, "30px")],
    );
    assert_at("width", "10%", "30%", &[(0.5, "20%")]);
    assert_at("width", "10px", "30%", &[(0.5, "calc(15% + 5px)")]);
    // A length that comes to 0 goes, as Chrome simplifies the mix; a zero
    // percentage stays.
    assert_at(
        "width",
        "40px",
        "40%",
        &[(0.0, "calc(0% + 40px)"), (1.0, "40%")],
    );
    // `vertical-align`'s `baseline` is `0px` against a length.
    assert_at(
        "vertical-align",
        "baseline",
        "100px",
        &[(0.3, "30px"), (-0.5, "-50px")],
    );
    assert_at("vertical-align", "super", "100px", &[(0.3, "super")]);
    assert_at("margin-left", "10px", "-10px", &[(0.75, "-5px")]);
    // Past the ends, as an easing function overshoots; not below 0 where
    // the property takes no negative values.
    assert_at("width", "10px", "30px", &[(1.5, "40px"), (-1.0, "0px")]);
    assert_at("margin-left", "10px", "30px", &[(-1.0, "-10px")]);
    assert_at("padding-top", "10px", "20px", &[(-2.0, "0px")]);
    // `auto` against a length steps at 50%.
    assert_at("width", "auto", "10px", &[(0.4, "auto"), (0.5, "10px")]);
    assert_at("top", "auto", "10px", &[(0.4, "auto"), (0.6, "10px")]);
    assert!(!interpolates("width", "auto", "10px"));
    assert!(interpolates("width", "0px", "10%"));
    // A zero length is a zero percentage too.
    assert_at("width", "0px", "20%", &[(0.5, "10%")]);
    // A zero percentage is a percentage: against a length, a mix
    // (`translate()`'s y, which a zero percentage fills in, is a length).
    assert_at(
        "text-indent",
        "0%",
        "100px",
        &[(0.0, "0%"), (0.2, "calc(0% + 20px)")],
    );
    assert_at(
        "transform",
        "translate(10px)",
        "translate(20px, 10px)",
        &[(0.5, "translate(15px, 5px)")],
    );
    // `normal` spacing is 0, but stays at the start.
    assert_at(
        "letter-spacing",
        "normal",
        "10px",
        &[(0.0, "normal"), (0.5, "5px"), (1.0, "10px")],
    );
    // A mix below 0 is clamped where it is used.
    let (a, b) = (style("width: 10px"), style("width: 30%"));
    let mut out = a.clone();
    interpolate(id("width"), &a, &b, -1.0, &mut out);
    assert!(
        matches!(
            out.width,
            computed_style::Size::LengthPercentage(computed_style::LengthPercentage::Calc {
                non_negative: true,
                ..
            })
        ),
        "{:?}",
        out.width
    );
    // A flow-relative longhand animates its physical one.
    assert_at("margin-inline-start", "0px", "20px", &[(0.5, "10px")]);
    assert_eq!(
        {
            let (a, b) = (
                style("margin-inline-start: 0px"),
                style("margin-left: 20px"),
            );
            let mut out = a.clone();
            interpolate(id("margin-inline-start"), &a, &b, 0.5, &mut out);
            computed_value(&out, "margin-left")
        }
        .as_deref(),
        Some("10px")
    );
}

/// Numbers, clamped to the property's range; integers rounded, halves up.
#[test]
fn numbers_and_integers() {
    assert_at(
        "opacity",
        "0",
        "1",
        &[(0.3, "0.3"), (1.5, "1"), (-0.5, "0")],
    );
    assert_at("flex-grow", "1", "3", &[(0.5, "2"), (-1.0, "0")]);
    assert_at("font-weight", "100", "900", &[(0.5, "500"), (2.0, "1000")]);
    assert_at("z-index", "1", "4", &[(0.5, "3"), (0.4, "2")]);
    assert_at(
        "order",
        "-4",
        "0",
        &[(0.5, "-2"), (0.375, "-2"), (0.625, "-1")],
    );
    assert_at("z-index", "auto", "4", &[(0.4, "auto"), (0.5, "4")]);
    assert_at("line-height", "10px", "20px", &[(0.5, "15px")]);
    // A number reads back in px, as Chrome writes it (16px text).
    assert_at("line-height", "1", "3", &[(0.5, "32px")]);
    assert!(
        !interpolates("line-height", "1", "20px"),
        "a number and a length step"
    );
    assert!(!interpolates("line-height", "normal", "20px"));
    assert_at("letter-spacing", "normal", "4px", &[(0.5, "2px")]);
    assert_at("font-size", "10px", "20px", &[(0.5, "15px"), (-1.0, "0px")]);
}

/// Colors mix with premultiplied alpha; `currentcolor` is the style's
/// own `color`.
#[test]
fn colors() {
    assert_at(
        "color",
        "red",
        "blue",
        &[(0.5, "rgb(128, 0, 128)"), (0.0, "rgb(255, 0, 0)")],
    );
    assert_at(
        "background-color",
        "transparent",
        "blue",
        &[(0.5, "rgba(0, 0, 255, 0.5)")],
    );
    assert_at(
        "background-color",
        "rgba(255, 0, 0, 0.2)",
        "rgba(0, 0, 255, 0.6)",
        &[(0.5, "rgba(64, 0, 191, 0.4)")],
    );
    let (a, b) = (
        style("color: red; border-top-color: currentcolor"),
        style("color: red; border-top-color: blue"),
    );
    let mut out = a.clone();
    interpolate(id("border-top-color"), &a, &b, 0.5, &mut out);
    assert_eq!(
        computed_value(&out, "border-top-color").as_deref(),
        Some("rgb(128, 0, 128)"),
        "currentcolor is red here"
    );
    // Each style's own: red's here, against lime.
    let (a, b) = (
        style("color: red; border-top-color: currentcolor"),
        style("color: blue; border-top-color: lime"),
    );
    let mut out = a.clone();
    interpolate(id("border-top-color"), &a, &b, 0.5, &mut out);
    assert_eq!(
        computed_value(&out, "border-top-color").as_deref(),
        Some("rgb(128, 128, 0)")
    );
    // Two of it stay the keyword, which follows `color`.
    let (a, b) = (style("color: red"), style("color: blue"));
    let mut out = a.clone();
    interpolate(id("border-top-color"), &a, &b, 0.5, &mut out);
    assert_eq!(
        out.border_color.top,
        computed_style::StyleColor::CurrentColor
    );
    assert_at("fill", "red", "blue", &[(0.5, "rgb(128, 0, 128)")]);
    assert!(!interpolates("fill", "none", "blue"));
}

/// `visible` all the way between 0 and 1 when either end is.
#[test]
fn visibility() {
    assert_at(
        "visibility",
        "hidden",
        "visible",
        &[(0.0, "hidden"), (0.1, "visible"), (1.0, "visible")],
    );
    assert_at(
        "visibility",
        "visible",
        "hidden",
        &[
            (0.9, "visible"),
            (1.0, "hidden"),
            (1.5, "hidden"),
            (-0.5, "visible"),
        ],
    );
    assert_at(
        "visibility",
        "hidden",
        "collapse",
        &[(0.4, "hidden"), (0.6, "collapse")],
    );
    assert!(!interpolates("visibility", "hidden", "collapse"));
}

/// Shadows one by one, the shorter list padded with transparent ones;
/// `inset` against not steps.
#[test]
fn shadows() {
    assert_at(
        "box-shadow",
        "2px 4px 6px 8px red",
        "4px 8px 10px 12px blue",
        &[(0.5, "rgb(128, 0, 128) 3px 6px 8px 10px")],
    );
    assert_at(
        "box-shadow",
        "none",
        "2px 2px red",
        &[(0.5, "rgba(255, 0, 0, 0.5) 1px 1px 0px 0px")],
    );
    assert_at(
        "box-shadow",
        "2px 2px red, 4px 4px blue",
        "4px 4px red",
        &[(
            0.5,
            "rgb(255, 0, 0) 3px 3px 0px 0px, rgba(0, 0, 255, 0.5) 2px 2px 0px 0px",
        )],
    );
    // Blur doesn't go below 0.
    assert_at(
        "box-shadow",
        "0 0 2px red",
        "0 0 4px red",
        &[(-2.0, "rgb(255, 0, 0) 0px 0px 0px 0px")],
    );
    assert!(!interpolates(
        "box-shadow",
        "1px 1px red inset",
        "1px 1px red"
    ));
}

/// Function by function when the lists match; else as matrices,
/// decomposed and interpolated (a rotation the short way round).
#[test]
fn transforms() {
    assert_at(
        "transform",
        "translate(10px)",
        "translate(30px, 10px)",
        &[(0.5, "translate(20px, 5px)")],
    );
    assert_at("transform", "none", "scale(3)", &[(0.5, "scale(2, 2)")]);
    assert_at("transform", "scale(3)", "none", &[(0.5, "scale(2, 2)")]);
    assert_at(
        "transform",
        "translate(10%) scale(2)",
        "translate(30%) scale(4)",
        &[(0.5, "translate(20%, 0px) scale(3, 3)")],
    );
    // Different functions: one matrix each.
    assert_at(
        "transform",
        "translate(10px)",
        "scale(2)",
        &[(0.5, "matrix(1.5, 0, 0, 1.5, 5, 0)")],
    );
    // The list as one matrix, in order: the translation is scaled.
    assert_at(
        "transform",
        "translate(10px) scale(2)",
        "scale(4)",
        &[(0.5, "matrix(3, 0, 0, 3, 5, 0)")],
    );
    // A flip is a negative scale (css-transforms 1, "Decomposing a 2D
    // matrix"; Chrome decomposes every matrix in 3D, and differs here).
    assert_at(
        "transform",
        "matrix(-1, 0, 0, 1, 0, 0)",
        "matrix(1, 0, 0, 1, 0, 0)",
        &[(0.5, "matrix(0, 0, 0, 1, 0, 0)")],
    );
    // A quarter turn, by matrices: halfway is an eighth.
    assert_at(
        "transform",
        "matrix(1, 0, 0, 1, 0, 0)",
        "matrix(0, 1, -1, 0, 0, 0)",
        &[(0.5, "matrix(0.707107, 0.707107, -0.707107, 0.707107, 0, 0)")],
    );
    // Half a turn, its sine exactly 0.
    assert_at(
        "transform",
        "matrix(1, 0, 0, 1, 0, 0)",
        "matrix(-1, 0, 0, -1, 0, 0)",
        &[(1.0, "matrix(-1, 0, 0, -1, 0, 0)")],
    );
    // Three quarters of a turn the other way is a quarter turn back.
    assert_at(
        "transform",
        "matrix(1, 0, 0, 1, 0, 0)",
        "matrix(0, -1, 1, 0, 0, 0)",
        &[(0.5, "matrix(0.707107, -0.707107, 0.707107, 0.707107, 0, 0)")],
    );
    // A matrix composed back from its parts is itself.
    assert_at(
        "transform",
        "matrix(1, 0.5, -0.25, 2, 10, 20)",
        "matrix(1, 0.5, -0.25, 2, 10, 20)",
        &[(0.3, "matrix(1, 0.5, -0.25, 2, 10, 20)")],
    );
    assert!(interpolates("transform", "translate(50%)", "none"));
}

/// Angles interpolate as angles: a whole turn turns, `rotate` and
/// `rotateZ` are one primitive.
#[test]
fn rotations_and_skews() {
    assert_at(
        "transform",
        "rotate(0)",
        "rotate(360deg)",
        &[(0.25, "rotate(90deg)"), (0.5, "rotate(180deg)")],
    );
    // The long way round, as written, not the short way a matrix takes.
    assert_at(
        "transform",
        "rotate(0)",
        "rotate(270deg)",
        &[(0.5, "rotate(135deg)")],
    );
    assert_at(
        "transform",
        "rotate(0deg)",
        "rotateZ(0.25turn)",
        &[(0.5, "rotate(45deg)")],
    );
    assert_at(
        "transform",
        "none",
        "rotate(90deg)",
        &[(0.5, "rotate(45deg)")],
    );
    assert_at(
        "transform",
        "skew(10deg)",
        "skew(0deg, 20deg)",
        &[(0.5, "skew(5deg, 10deg)")],
    );
    assert_at(
        "transform",
        "skewX(10deg)",
        "skewX(30deg)",
        &[(0.5, "skewX(20deg)")],
    );
    // `skewX` and `skewY` are each their own kind: as matrices, two
    // identities here, whatever the turn after them does.
    assert_at(
        "transform",
        "skewX(0deg) rotate(0deg)",
        "skewY(0deg) rotate(360deg)",
        &[(0.5, "matrix(1, 0, 0, 1, 0, 0)")],
    );
}

/// Function by function as far as the primitives match, the rest as a
/// matrix; the end of the shorter list stands for identities.
#[test]
fn transform_prefixes() {
    assert_at(
        "transform",
        "translate(10px) rotate(0deg)",
        "translate(20px) rotate(90deg) scale(2)",
        &[(0.5, "translate(15px, 0px) rotate(45deg) scale(1.5, 1.5)")],
    );
    assert_at(
        "transform",
        "translate(10px) rotate(0deg)",
        "translate(20px) scale(2)",
        &[(0.5, "translate(15px, 0px) matrix(1.5, 0, 0, 1.5, 0, 0)")],
    );
}

/// A rest that needs the box for its matrix (a percentage) waits for the
/// box: the same interpolation is another matrix on another box.
#[test]
fn transforms_wait_for_the_box() {
    assert!(interpolates("transform", "translate(50%)", "scale(2)"));
    let (a, b) = (
        style("transform: translate(50%)"),
        style("transform: scale(2)"),
    );
    let mut out = a.clone();
    interpolate(id("transform"), &a, &b, 0.5, &mut out);
    let on = |w: f32, h: f32| out.transform_matrix(gfx::SizeF::new(w, h)).0;
    // Halfway from 50px across to twice the size, and from 100px across.
    assert_eq!(on(100.0, 10.0), [1.5, 0.0, 0.0, 1.5, 25.0, 0.0]);
    assert_eq!(on(200.0, 10.0), [1.5, 0.0, 0.0, 1.5, 50.0, 0.0]);
}

#[test]
fn transform_origins() {
    assert_at(
        "transform-origin",
        "0 0",
        "100% 10px 4px",
        &[(0.5, "50% 5px 2px")],
    );
    assert_at(
        "transform-origin",
        "10px",
        "20%",
        &[(0.5, "calc(10% + 5px) 50%")],
    );
    assert!(!interpolates("transform-box", "border-box", "fill-box"));
}

#[test]
fn object_positions() {
    assert_at("object-position", "0 0", "100% 20px", &[(0.5, "50% 10px")]);
    assert!(!interpolates("object-fit", "fill", "cover"));
}

/// `text-shadow` as `box-shadow` (shadow by shadow, the shorter list
/// padded); `text-decoration-thickness` and `text-underline-offset` as
/// lengths, their keywords stepping; the colors; the keywords stepping.
#[test]
fn text_decorations() {
    assert_at(
        "text-shadow",
        "red 0px 0px",
        "blue 10px 20px 30px, lime 2px 2px",
        &[(
            0.5,
            "rgb(128, 0, 128) 5px 10px 15px, rgba(0, 255, 0, 0.5) 1px 1px 0px",
        )],
    );
    assert_at("text-decoration-thickness", "0px", "10px", &[(0.5, "5px")]);
    assert_at(
        "text-underline-offset",
        "10px",
        "20%",
        &[(0.5, "calc(10% + 5px)")],
    );
    assert_at(
        "text-decoration-color",
        "red",
        "blue",
        &[(0.5, "rgb(128, 0, 128)")],
    );
    assert_at(
        "-webkit-text-fill-color",
        "red",
        "blue",
        &[(0.5, "rgb(128, 0, 128)")],
    );
    assert!(!interpolates("text-decoration-thickness", "auto", "10px"));
    assert!(!interpolates(
        "text-decoration-thickness",
        "from-font",
        "auto"
    ));
    assert!(!interpolates("text-underline-offset", "auto", "10px"));
    assert!(interpolates("text-underline-offset", "auto", "auto"));
    assert!(!interpolates("text-decoration-style", "solid", "wavy"));
    assert!(!interpolates("text-underline-position", "under", "auto"));
}

/// `filter`: function by function of the same kinds in order, the shorter
/// list padded with each function's initial value, an amount kept in its
/// range; other kinds step; `isolation` steps.
#[test]
fn filters() {
    assert_at(
        "filter",
        "blur(0px) brightness(1)",
        "blur(10px) brightness(3)",
        &[
            (0.5, "blur(5px) brightness(2)"),
            (-1.0, "blur(0px) brightness(0)"),
        ],
    );
    assert_at(
        "filter",
        "none",
        "opacity(0) hue-rotate(90deg)",
        &[(0.5, "opacity(0.5) hue-rotate(45deg)")],
    );
    assert_at(
        "filter",
        "grayscale(0)",
        "grayscale(1)",
        &[(1.5, "grayscale(1)")],
    );
    assert_at(
        "filter",
        "drop-shadow(red 0px 0px 0px)",
        "drop-shadow(blue 10px 20px 30px)",
        &[(0.5, "drop-shadow(rgb(128, 0, 128) 5px 10px 15px)")],
    );
    assert_at(
        "filter",
        "sepia(1)",
        "sepia(1) invert(1)",
        &[(0.5, "sepia(1) invert(0.5)")],
    );
    assert!(!interpolates("filter", "blur(1px)", "sepia(1)"));
    assert!(!interpolates("isolation", "auto", "isolate"));
}

/// `clip-path`: a shape with one of its kind on the same box, each value
/// (a radius never below 0, a keyword not even with itself, a polygon's
/// points with as many, a path's numbers with the same commands); `clip`:
/// each edge, `auto` only with `auto`.
#[test]
fn clips() {
    assert_at(
        "clip-path",
        "inset(0px)",
        "inset(20px round 10px)",
        &[(0.5, "inset(10px round 5px)")],
    );
    assert_at(
        "clip-path",
        "circle(10px at 0% 0%)",
        "circle(30px at 100% 50%)",
        &[
            (0.5, "circle(20px at 50% 25%)"),
            (-1.0, "circle(0px at -100% -50%)"),
        ],
    );
    assert_at(
        "clip-path",
        "ellipse(10px 20% at 0% 0%) padding-box",
        "ellipse(30px 40% at 0% 0%) padding-box",
        &[(0.5, "ellipse(20px 30% at 0% 0%) padding-box")],
    );
    assert_at(
        "clip-path",
        "polygon(round 2px, 0px 0px, 10px 10px)",
        "polygon(round 4px, 10px 0px, 30px 10px)",
        &[(0.5, "polygon(round 3px, 5px 0px, 20px 10px)")],
    );
    assert_at(
        "clip-path",
        "path('M 0 0 l 10 10 A 1 1 0 0 1 4 4')",
        "path('M 10 10 l 30 30 A 3 3 0 0 1 8 8')",
        &[(0.5, "path(\"M 5 5 l 20 20 A 2 2 0 0 1 6 6\")")],
    );
    // A center not given stays so, or is the reference box's.
    assert_at(
        "clip-path",
        "circle(10px)",
        "circle(20px)",
        &[(0.5, "circle(15px)")],
    );
    assert_at(
        "clip-path",
        "circle(10px)",
        "circle(20px at 0% 0%)",
        &[(0.5, "circle(15px at 25% 25%)")],
    );
    assert!(interpolates("clip-path", "none", "none"));
    assert!(!interpolates(
        "clip-path",
        "circle(closest-side)",
        "circle(10px)"
    ));
    assert!(!interpolates(
        "clip-path",
        "circle(farthest-side at 0 0)",
        "circle(farthest-side at 9px 0)"
    ));
    assert!(!interpolates(
        "clip-path",
        "circle(10px) padding-box",
        "circle(20px)"
    ));
    assert!(!interpolates("clip-path", "inset(1px)", "circle(20px)"));
    assert!(!interpolates(
        "clip-path",
        "polygon(0 0, 1px 1px)",
        "polygon(0 0, 1px 1px, 2px 2px)"
    ));
    assert!(!interpolates(
        "clip-path",
        "polygon(0 0)",
        "polygon(evenodd, 0 0)"
    ));
    assert!(!interpolates(
        "clip-path",
        "path('M 0 0 L 1 1')",
        "path('M 0 0 l 1 1')"
    ));
    assert!(!interpolates(
        "clip-path",
        "path('M 0 0 A 1 1 0 0 1 4 4')",
        "path('M 0 0 A 1 1 0 1 1 4 4')"
    ));
    assert!(!interpolates(
        "clip-path",
        "path('M 0 0')",
        "path(evenodd, 'M 0 0')"
    ));
    assert!(!interpolates("clip-path", "none", "inset(1px)"));
    assert!(!interpolates("clip-path", "url(#a)", "url(#b)"));
    assert!(!interpolates("clip-path", "border-box", "border-box"));
    assert_at(
        "clip",
        "rect(0px, 10px, 20px, auto)",
        "rect(10px, 30px, 40px, auto)",
        &[(0.5, "rect(5px, 20px, 30px, auto)")],
    );
    assert!(!interpolates(
        "clip",
        "rect(0px, 10px, 20px, auto)",
        "rect(0px, 10px, 20px, 5px)"
    ));
    assert!(!interpolates("clip", "auto", "rect(0px, 10px, 20px, 5px)"));
    assert!(interpolates("clip", "auto", "auto"));
}

/// `tab-size`: numbers with numbers, lengths with lengths, never below 0.
#[test]
fn tab_sizes() {
    assert_at("tab-size", "2", "6", &[(0.5, "4"), (-1.0, "0")]);
    assert_at("tab-size", "10px", "20px", &[(0.5, "15px")]);
    assert!(!interpolates("tab-size", "4", "20px"));
}

/// `-webkit-box-flex` is a number, but steps (Blink's isn't animatable).
#[test]
fn webkit_box_flexes() {
    assert_at("-webkit-box-flex", "1", "3", &[(0.4, "1"), (0.6, "3")]);
    assert!(!interpolates("-webkit-box-flex", "1", "3"));
}

/// Lists repeat to their least common multiple; `none` steps.
#[test]
fn lists() {
    assert_at(
        "stroke-dasharray",
        "1px, 2px",
        "3px",
        &[(0.5, "2px, 2.5px")],
    );
    assert!(!interpolates("stroke-dasharray", "none", "3px"));
    assert_at(
        "stroke-dasharray",
        "1px, 2px",
        "3px, 4px, 5px",
        &[(0.5, "2px, 3px, 3px, 2.5px, 2.5px, 3.5px")],
    );
    // `right 10px` is `calc(100% - 10px)` from the start.
    assert_at(
        "background-position-x",
        "right 10px",
        "0%",
        &[(0.5, "calc(50% - 5px)")],
    );
    assert_at(
        "background-size",
        "10px auto",
        "30px auto",
        &[(0.5, "20px auto")],
    );
    assert!(!interpolates("background-size", "cover", "10px"));
    // The same keyword on both sides stays; the rest of the list goes on.
    assert_at(
        "background-size",
        "0px auto, contain",
        "40px auto, contain",
        &[(0.25, "10px auto, contain")],
    );
    assert_at(
        "border-top-left-radius",
        "10px 20%",
        "30px 40%",
        &[(0.5, "20px 30%")],
    );
    // `border-spacing`'s longhands, each a length.
    assert_at(
        "-webkit-border-horizontal-spacing",
        "2px",
        "6px",
        &[(0.5, "4px"), (-2.0, "0px")],
    );
    assert_at(
        "-webkit-border-vertical-spacing",
        "4px",
        "6px",
        &[(0.5, "5px"), (-3.0, "0px")],
    );
}

/// A discrete step at 50% for the properties that aren't interpolated.
#[test]
fn discrete_properties_step() {
    assert_at(
        "position",
        "static",
        "absolute",
        &[(0.49, "static"), (0.5, "absolute")],
    );
    assert_at(
        "text-align",
        "left",
        "right",
        &[(-0.5, "left"), (1.5, "right")],
    );
    assert!(!interpolates("position", "static", "absolute"));
    assert!(!interpolates("font-family", "serif", "monospace"));
}

/// Inherited values come from each style's parent: a child's `color`
/// with no declaration of its own is its parent's.
#[test]
fn inherited_values_are_the_styles_own() {
    let a = style_in("color: red", "");
    let b = style_in("color: blue", "");
    let mut out = a.clone();
    interpolate(id("color"), &a, &b, 0.5, &mut out);
    assert_eq!(
        computed_value(&out, "color").as_deref(),
        Some("rgb(128, 0, 128)")
    );
}
