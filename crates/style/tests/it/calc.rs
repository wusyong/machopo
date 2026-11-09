//! Math functions (`calc()`, `min()`, `max()`, `clamp()`, and the
//! stepped value, sign, trigonometric and exponential ones) in lengths,
//! percentages, numbers and angles: parsed and simplified, serialized as
//! CSSOM writes them, and computed.

use computed_style::{LengthPercentage, LengthPercentageOrAuto, Size};
use style_resolver::{CssPropertyValueSet, computed_value};

use crate::{Page, styles};

/// `property: value` as the declaration block writes it back; `None` if
/// it doesn't parse.
fn specified(property: &str, value: &str) -> Option<String> {
    let mut block = CssPropertyValueSet::default();
    block
        .set_property(property, value, "")
        .then(|| block.property_value(property))
}

fn px(v: f32) -> Size {
    Size::LengthPercentage(LengthPercentage::Px(v))
}

#[test]
fn parsed_simplified_and_written_back() {
    for (value, written) in [
        ("calc(10px)", "calc(10px)"),
        ("calc(1px + 2px)", "calc(3px)"),
        // Absolute units are px when added up; font-relative ones stay;
        // the terms go by unit (css-values-4).
        ("calc(1in + 4px)", "calc(100px)"),
        ("calc(2em + 10px - 1em)", "calc(1em + 10px)"),
        ("calc(100% - 20px)", "calc(100% - 20px)"),
        ("calc(-20px + 100%)", "calc(100% - 20px)"),
        ("CALC(50% + 0px)", "calc(50% + 0px)"),
        ("calc(2 * (10px + 5%))", "calc(10% + 20px)"),
        ("calc((1 + 1) * 2px)", "calc(4px)"),
        ("calc((10px + 5%) / 2)", "calc(2.5% + 5px)"),
        ("calc(calc(1px * 3) + 1em)", "calc(1em + 3px)"),
        ("min(10px, 2em)", "min(10px, 2em)"),
        ("max(10px, 20px)", "calc(20px)"),
        ("clamp(10px, 50px, 20px)", "calc(20px)"),
        ("clamp(1em, 2px, 3em)", "clamp(1em, 2px, 3em)"),
        ("calc(2 * min(1px, 1em))", "calc(2 * min(1px, 1em))"),
        // A product in a sum in parentheses; one subtracted after a ` - `,
        // one with a negative number added (as Chrome 152 writes them).
        (
            "calc(10px - 2 * min(1px, 1em))",
            "calc(10px - (2 * min(1px, 1em)))",
        ),
        (
            "calc(-2 * min(1px, 1em) + 1em)",
            "calc(1em + (-2 * min(1px, 1em)))",
        ),
        (
            "calc(1em + 2 * max(1px, 1em) - 3 * clamp(1px, 1em, 2px))",
            "calc(1em + (2 * max(1px, 1em)) - (3 * clamp(1px, 1em, 2px)))",
        ),
        ("calc(10px * pi / pi)", "calc(10px)"),
        ("min(10%, 20%)", "calc(10%)"),
    ] {
        assert_eq!(
            specified("width", value).as_deref(),
            Some(written),
            "{value}"
        );
    }
}

#[test]
fn what_doesnt_parse() {
    for value in [
        // The operators need white space around them.
        "calc(1px+2px)",
        "calc(1px -2px)",
        "calc((1px)-(2px))",
        "calc((1px) -(2px))",
        "calc((1px)- (2px))",
        // A number isn't a length, nor 0 within calc().
        "calc(10)",
        "calc(0 + 5px)",
        // Lengths times lengths, by a length.
        "calc(1px * 2px)",
        "calc(10px / 2px)",
        "calc(1px + 2)",
        "calc()",
        "min()",
        "clamp(1px, 2px)",
        "foo(1px)",
        "calc(1deg)",
        // Lengths and percentages mixed in min(), max() or clamp() (see
        // the limitations).
        "min(100%, 600px)",
        "max(calc(10% + 1px), 2px)",
        "clamp(1px, 50%, 3px)",
    ] {
        assert_eq!(specified("width", value), None, "{value}");
    }
    // Where only lengths go, no percentage.
    assert_eq!(specified("border-top-width", "calc(10% + 1px)"), None);
    assert_eq!(
        specified("border-top-width", "calc(1px + 1em)").as_deref(),
        Some("calc(1em + 1px)")
    );
    // An unclosed one at the end is closed, as any block is.
    assert_eq!(specified("width", "calc(1px").as_deref(), Some("calc(1px)"));
}

#[test]
fn computed() {
    let s = styles(
        "<div style='font-size: 10px'>
           <p id=a style='width: calc(2em + 5px); height: calc(50% - 10px)'>
           <p id=b style='width: min(1em, 30px); margin-left: calc(10% + 1em);
                          padding-left: calc(-5px); margin-top: calc(-5px)'>
           <p id=c style='font-size: calc(1em + 2px); width: calc(1em)'>
           <p id=d style='font-size: calc(200% + 2px)'>
           <p id=e style='width: calc(50%); line-height: calc(1em + 50%)'>
         </div>",
    );
    assert_eq!(s["a"].width, px(25.0));
    assert_eq!(
        s["a"].height,
        Size::LengthPercentage(LengthPercentage::Calc {
            px: -10.0,
            percent: 50.0,
            non_negative: true,
        })
    );
    assert_eq!(s["b"].width, px(10.0));
    assert_eq!(
        s["b"].margin.left,
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Calc {
            px: 10.0,
            percent: 10.0,
            non_negative: false,
        })
    );
    // Clamped where negative isn't allowed, kept where it is.
    assert_eq!(s["b"].padding.left, LengthPercentage::Px(0.0));
    assert_eq!(
        s["b"].margin.top,
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(-5.0))
    );
    // `em` in `font-size` is the parent's, elsewhere the element's own.
    assert_eq!(s["c"].font.size, 12.0);
    assert_eq!(s["c"].width, px(12.0));
    // A percentage in `font-size` is of the parent's.
    assert_eq!(s["d"].font.size, 22.0);
    assert_eq!(
        s["e"].width,
        Size::LengthPercentage(LengthPercentage::Percent(50.0))
    );
    // `line-height`'s percentage is of the font size.
    assert_eq!(s["e"].line_height, computed_style::LineHeight::Px(15.0));
}

#[test]
fn with_var() {
    let s = styles(
        "<style>:root { --gap: 8px; --n: 3 }</style>
         <p id=a style='width: calc(100% - var(--gap) * var(--n))'>
         <p id=b style='width: calc(var(--gap) * 2)'>",
    );
    assert_eq!(
        s["a"].width,
        Size::LengthPercentage(LengthPercentage::Calc {
            px: -24.0,
            percent: 100.0,
            non_negative: true,
        })
    );
    assert_eq!(s["b"].width, px(16.0));
}

/// Values too big for an f32, infinite or NaN (css-values-4's constants,
/// and dividing by 0), read back as Chrome 152 has them: specified, kept
/// finite and written as the constant times one of the unit; computed,
/// clamped as Blink clamps them (lengths to its `LayoutUnit`'s range,
/// font sizes to 10000px, angles to a multiple of 360, NaN to 0), and a
/// font size taken from the parent's grows no further.
#[test]
fn huge_and_non_finite_values_as_chrome_has_them() {
    for (property, value, written) in [
        ("width", "1e39px", "3.40282e+38px"),
        ("margin-left", "-1e39px", "-3.40282e+38px"),
        ("width", "1e39%", "3.40282e+38%"),
        ("width", "calc(1e39px + 10%)", "calc(10% + 3.40282e+38px)"),
        ("width", "calc(10px / 0)", "calc(infinity * 1px)"),
        ("width", "calc(INFINITY * 1px)", "calc(infinity * 1px)"),
        ("width", "calc(nan * 1px)", "calc(NaN * 1px)"),
        ("width", "calc(-infinity * 1px)", "calc(-infinity * 1px)"),
        ("width", "calc(1px * (1 / 0))", "calc(infinity * 1px)"),
        ("width", "calc(1e39px - 1e30px)", "calc(3.40282e+38px)"),
        ("width", "min(NaN * 1px, 10px)", "calc(NaN * 1px)"),
        ("width", "max(NaN * 1px, 10px)", "calc(NaN * 1px)"),
        ("width", "clamp(0px, NaN * 1px, 10px)", "calc(NaN * 1px)"),
        ("width", "min(1em, NaN * 1px)", "min(1em, NaN * 1px)"),
        (
            "width",
            "calc(infinity * 1px - infinity * 1px)",
            "calc(NaN * 1px)",
        ),
        ("width", "calc(0px / 0)", "calc(NaN * 1px)"),
        ("width", "calc(-1px / 0)", "calc(-infinity * 1px)"),
        ("margin-left", "calc(1px / -0)", "calc(-infinity * 1px)"),
    ] {
        assert_eq!(
            specified(property, value).as_deref(),
            Some(written),
            "{property}: {value}"
        );
    }
    let nested = "<div style='font-size: 1000%'>".repeat(8);
    let s = styles(&format!(
        "<p id=a style='width: 1e39px; margin-left: -1e39px; border-left: 1.5e9px solid;
                        letter-spacing: 1e39px; text-indent: 1e39em'>
         <p id=b style='width: calc(infinity * 1px); margin-left: calc(-infinity * 1px);
                        font-size: 1e39px; z-index: calc(infinity); line-height: calc(infinity);
                        transform: rotate(calc(infinity * 1deg)); flex-grow: calc(infinity);
                        transition-duration: calc(-infinity * 1s)'>
         <p id=c style='width: calc(NaN * 1px); font-size: calc(NaN * 1px); opacity: calc(NaN);
                        z-index: calc(-infinity)'>
         <p id=d style='font-size: calc(infinity * 1px); z-index: calc(NaN)'>
         <p id=e style='font-size: 20000px'>
         <p id=g style='width: calc(1e39px - 1e30px); margin-left: calc(1px / -0);
                        margin-right: calc(1e39px + 1e39%); padding-left: 1e39%'>
         <p id=h style='width: min(1em, NaN * 1px); margin-left: calc(infinity * 1px - infinity * 1px)'>
         <p id=i style='width: calc(1e30em - 1e30px); margin-left: calc(1e30px - 1e30em);
                        margin-right: max(NaN * 1px, 1em)'>
         {nested}<div id=f></div>"
    ));
    // Not what CSSOM reads back (a used value, once laid out), but what
    // layout gets: each part of a length-percentage in range.
    assert_eq!(
        s["g"].margin.right,
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Calc {
            px: computed_style::MAX_CSS_LENGTH,
            percent: computed_style::MAX_CSS_LENGTH,
            non_negative: false,
        })
    );
    assert_eq!(
        s["g"].padding.left,
        LengthPercentage::Percent(computed_style::MAX_CSS_LENGTH)
    );
    // NaN is 0, not a NaN CSSOM would write as 0 too.
    assert_eq!(s["c"].width, px(0.0));
    assert_eq!(s["c"].opacity, 0.0);
    assert_eq!(
        s["h"].margin.left,
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(0.0))
    );
    for (id, property, value) in [
        ("a", "width", "3.35544e+07px"),
        ("a", "margin-left", "-3.35544e+07px"),
        ("a", "border-left-width", "3.35544e+07px"),
        ("a", "letter-spacing", "3.35544e+07px"),
        ("a", "text-indent", "3.35544e+07px"),
        ("b", "width", "3.35544e+07px"),
        ("b", "margin-left", "-3.35544e+07px"),
        ("b", "font-size", "10000px"),
        ("b", "z-index", "2147483647"),
        ("b", "line-height", "3.40282e+38px"),
        ("b", "transform", "rotate(2.86708e+15deg)"),
        ("b", "flex-grow", "3.40282e+38"),
        ("b", "transition-duration", "0s"),
        ("c", "width", "0px"),
        ("c", "font-size", "0px"),
        ("c", "opacity", "0"),
        ("c", "z-index", "-2147483648"),
        ("d", "font-size", "10000px"),
        ("d", "z-index", "0"),
        ("e", "font-size", "10000px"),
        ("f", "font-size", "10000px"),
        // A math function's parts aren't clamped before its result is.
        ("g", "width", "3.35544e+07px"),
        ("g", "margin-left", "-3.35544e+07px"),
        ("h", "width", "0px"),
        ("h", "margin-left", "0px"),
        ("i", "width", "3.35544e+07px"),
        ("i", "margin-left", "-3.35544e+07px"),
        ("i", "margin-right", "0px"),
    ] {
        assert_eq!(
            computed_value(&s[id], property).as_deref(),
            Some(value),
            "{id}'s {property}"
        );
    }
}

#[test]
fn computed_values_are_written_as_chrome_writes_them() {
    let s = styles(
        "<p id=a style='margin-left: calc(50% - 10px); background-position: calc(10% + 5px) 0;
                        transform: translate(calc(50% + 1px))'>
         <p id=b style='background-position: right calc(10% + 5px) top 0'>",
    );
    assert_eq!(
        computed_value(&s["a"], "margin-left").as_deref(),
        Some("calc(50% - 10px)")
    );
    assert_eq!(
        computed_value(&s["a"], "background-position-x").as_deref(),
        Some("calc(10% + 5px)")
    );
    // From the end side: 100% less it.
    assert_eq!(
        computed_value(&s["b"], "background-position-x").as_deref(),
        Some("calc(90% - 5px)")
    );
}

#[test]
fn numbers_integers_and_angles_are_worked_out_when_parsed() {
    let s = styles(
        "<p id=a style='opacity: calc(1 / 2); z-index: calc(2.5); position: relative;
                        order: calc(-1.5); flex-grow: calc(1 - 3); line-height: calc(3 / 2);
                        font-weight: max(2000, 10); transform: scale(calc(2 * 2), min(1, 3))'>
         <p id=b style='background-image: linear-gradient(calc(45deg * 2), red, blue)'>
         <p id=c style='background-image: linear-gradient(calc(0.25turn + 0.5rad - 0.5rad), red, blue)'>",
    );
    let a = &s["a"];
    assert_eq!(a.opacity, 0.5);
    // Integers round to the nearest, halves up.
    assert_eq!(a.z_index, Some(3));
    assert_eq!(a.order, -1);
    // Clamped to what the property takes.
    assert_eq!(a.flex_grow, 0.0);
    assert_eq!(a.font.weight, 1000.0);
    assert_eq!(a.line_height, computed_style::LineHeight::Number(1.5));
    assert_eq!(
        a.transform[..],
        [computed_style::TransformOperation::Scale(4.0, 1.0)]
    );
    for id in ["b", "c"] {
        let computed_style::BackgroundImage::Gradient(gradient) =
            &s[id].backgrounds.background_image[0]
        else {
            panic!("#{id} has no gradient");
        };
        assert_eq!(
            gradient.kind,
            computed_style::GradientKind::Linear(computed_style::LinearDirection::Angle(90.0)),
            "#{id}"
        );
    }
    // An angle where a length goes, and a length where a number does.
    assert_eq!(specified("width", "calc(10deg)"), None);
    assert_eq!(specified("opacity", "calc(1px)"), None);
    assert_eq!(specified("opacity", "calc(1deg)"), None);
}

#[test]
fn viewport_units() {
    let mut page = Page::parsed(
        "<div id=v style='font-size: 2vw'>
           <p id=child style='width: 10em'></p>
         </div>
         <p id=w style='width: 50vw; height: 10vh; margin-left: calc(100vw - 10px)'>
         <p id=m style='width: 10vmin; height: 10vmax; padding-left: 10svw'>
         <p id=plain style='width: 10px'>",
    );
    // Nothing uses the viewport yet: no recalc for it.
    assert!(!page.set_viewport(gfx::SizeF::new(800.0, 600.0), 1.0));
    page.recalc();
    let w = page.style("w");
    assert_eq!(w.width, px(400.0));
    assert_eq!(w.height, px(60.0));
    assert_eq!(
        w.margin.left,
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(790.0))
    );
    assert!(w.has_viewport_units);
    let m = page.style("m");
    assert_eq!(m.width, px(60.0));
    assert_eq!(m.height, px(80.0));
    assert_eq!(m.padding.left, LengthPercentage::Px(80.0));
    assert_eq!(page.style("v").font.size, 16.0);
    assert_eq!(page.style("child").width, px(160.0));
    assert!(!page.style("plain").has_viewport_units);
    // The child's `em` uses the parent's computed px: no viewport of its
    // own.
    assert!(!page.style("child").has_viewport_units);

    // A new viewport: those that used it are styled again, their subtrees
    // with them; the rest keep their styles.
    let plain = page.style("plain");
    assert!(page.set_viewport(gfx::SizeF::new(1000.0, 500.0), 1.0));
    assert!(
        !page.set_viewport(gfx::SizeF::new(1000.0, 500.0), 1.0),
        "the same again"
    );
    page.recalc();
    assert_eq!(page.style("w").width, px(500.0));
    assert_eq!(page.style("m").width, px(50.0));
    assert_eq!(page.style("v").font.size, 20.0);
    assert_eq!(page.style("child").width, px(200.0));
    assert!(std::sync::Arc::ptr_eq(&page.style("plain"), &plain));
    drop(page);

    // Serialized as written.
    assert_eq!(specified("width", "10SVW").as_deref(), Some("10svw"));
    assert_eq!(
        specified("width", "calc(100vw - 10px)").as_deref(),
        Some("calc(-10px + 100vw)")
    );
}

/// The functions besides `calc()`, `min()`, `max()` and `clamp()`, written
/// back as Chrome 152 writes them: worked out if their arguments are all
/// in canonical units (px, deg; numbers), else kept (a relative length, a
/// percentage), `nearest` left out; a product or quotient by a number
/// only an element works out kept, its leaves multiplied into one first,
/// in parentheses in a sum, after a ` - ` if subtracted; a sum in a
/// product in parentheses.
#[test]
fn the_other_math_functions_written_back() {
    for (value, written) in [
        ("round(1em, 3px)", "round(1em, 3px)"),
        ("round(up, 1em, 3px)", "round(up, 1em, 3px)"),
        ("round(nearest, 1em, 3px)", "round(1em, 3px)"),
        ("round(down, 1em, 3px)", "round(down, 1em, 3px)"),
        ("round(to-zero, 1em, 3px)", "round(to-zero, 1em, 3px)"),
        ("round(7em, 2em)", "round(7em, 2em)"),
        ("round(down, 7px, 2px)", "calc(6px)"),
        ("round(to-zero, -7px, 2px)", "calc(-6px)"),
        ("round(-0.5px, 1px)", "calc(0px)"),
        ("round(10%, 3%)", "round(10%, 3%)"),
        ("mod(1em, 3px)", "mod(1em, 3px)"),
        ("mod(-7px, 3px)", "calc(2px)"),
        ("rem(-7px, 3px)", "calc(-1px)"),
        ("mod(1em, -3px)", "mod(1em, -3px)"),
        ("abs(-1em)", "abs(-1em)"),
        ("abs(-3px)", "calc(3px)"),
        ("abs(1em - 3px)", "abs(1em - 3px)"),
        ("hypot(3em, 4px)", "hypot(3em, 4px)"),
        ("hypot(3px, 4px)", "calc(5px)"),
        ("hypot(-3px)", "calc(3px)"),
        ("hypot(1em)", "hypot(1em)"),
        ("calc(1px * sin(30deg))", "calc(0.5px)"),
        ("calc(100px * cos(0.5))", "calc(87.7583px)"),
        ("calc(1px * tan(90deg))", "calc(infinity * 1px)"),
        ("calc(1px * pow(2, 3))", "calc(8px)"),
        ("calc(1px * sqrt(16))", "calc(4px)"),
        ("calc(1px * exp(1))", "calc(2.71828px)"),
        ("calc(1px * log(8, 2))", "calc(3px)"),
        ("calc(1px * log(e))", "calc(1px)"),
        ("calc(10px * abs(-2))", "calc(20px)"),
        ("calc(1px * round(2.5))", "calc(3px)"),
        ("calc(1px * round(-2.5))", "calc(-2px)"),
        ("calc(1px * mod(-1, 3))", "calc(2px)"),
        ("round(10px, 0px)", "calc(NaN * 1px)"),
        ("mod(10px, 0px)", "calc(NaN * 1px)"),
        ("round(infinity * 1px, 3px)", "calc(infinity * 1px)"),
        ("calc(1px * sign(-0px))", "calc(0px)"),
        ("calc(1px / sign(0px))", "calc(infinity * 1px)"),
        // Signed zeros: `mod()`'s has the step's sign, a `round()` up to 0
        // is -0, down to it +0 (1 / -0 is -infinity).
        ("calc(1px / sign(mod(-1, 1)))", "calc(infinity * 1px)"),
        ("calc(1px / sign(mod(1, -1)))", "calc(-infinity * 1px)"),
        ("calc(1px / sign(rem(-1, 1)))", "calc(-infinity * 1px)"),
        ("calc(1px / sign(round(-0.4, 1)))", "calc(-infinity * 1px)"),
        ("calc(1px / sign(round(0.4, 1)))", "calc(infinity * 1px)"),
        ("calc(1px / sign(min(-0px, 0px)))", "calc(-infinity * 1px)"),
        ("calc(1px / sign(max(-0px, 0px)))", "calc(infinity * 1px)"),
        ("calc(1px / sign(max(0px, -0px)))", "calc(infinity * 1px)"),
        ("calc(1px / sign(min(0px, -0px)))", "calc(-infinity * 1px)"),
        ("max(1px, round(1em, 3px))", "max(1px, round(1em, 3px))"),
        ("calc(round(1em, 3px) * 2)", "calc(2 * round(1em, 3px))"),
        ("calc(round(1em, 3px) / 2)", "calc(0.5 * round(1em, 3px))"),
        ("calc(-1 * round(1em, 3px))", "calc(-1 * round(1em, 3px))"),
        (
            "calc(round(1em, 3px) - 1px)",
            "calc(-1px + round(1em, 3px))",
        ),
        ("calc(10px * sign(1em))", "calc(10px * sign(1em))"),
        ("calc(sign(1em) * 10px)", "calc(10px * sign(1em))"),
        ("calc(1em * sign(1em))", "calc(1em * sign(1em))"),
        ("calc(sign(1em) * 2 * 10px)", "calc(20px * sign(1em))"),
        (
            "calc(10px * sign(1em) * sign(1em))",
            "calc(10px * sign(1em) * sign(1em))",
        ),
        (
            "calc(10px * sign(1em) * sign(1em) * 1)",
            "calc(10px * sign(1em) * sign(1em))",
        ),
        ("calc(10px * sign(-3px))", "calc(-10px)"),
        ("calc(10px / sign(-1em))", "calc(10px / sign(-1em))"),
        ("calc(2 * (10px / sign(-1em)))", "calc(20px / sign(-1em))"),
        (
            "calc(1px * abs(sign(1em) * sign(1em) * 1))",
            "calc(1px * abs(sign(1em) * sign(1em)))",
        ),
        (
            "calc(1px + 2px * sign(1em))",
            "calc(1px + (2px * sign(1em)))",
        ),
        (
            "calc(1px - 2px * sign(1em))",
            "calc(1px - (2px * sign(1em)))",
        ),
        ("calc(1px * (1 + sign(1em)))", "calc(1px * (1 + sign(1em)))"),
        (
            "calc(1px * pow(sign(1em), 2))",
            "calc(1px * pow(sign(1em), 2))",
        ),
        ("calc(1px * sin(sign(1em)))", "calc(1px * sin(sign(1em)))"),
    ] {
        assert_eq!(
            specified("width", value).as_deref(),
            Some(written),
            "{value}"
        );
    }
    // Numbers and angles are worked out as they are parsed (Chrome writes
    // `calc(3)`: see the limitations).
    for (property, value, written) in [
        ("opacity", "sin(1)", "0.841471"),
        ("opacity", "round(2.5)", "3"),
        ("z-index", "round(-2.5)", "-2"),
        ("z-index", "calc(2 * sign(-3))", "-2"),
        ("transform", "rotate(atan2(1, 1))", "rotate(45deg)"),
        ("transform", "rotate(atan2(1px, -1px))", "rotate(135deg)"),
        ("transform", "rotate(acos(0.5))", "rotate(60deg)"),
        ("transform", "rotate(mod(400deg, 360deg))", "rotate(40deg)"),
        ("transform", "rotate(calc(asin(2)))", "rotate(0deg)"),
        ("transition-delay", "calc(1s * hypot(3, 4))", "5s"),
        // Percentages where only a percentage goes, worked out as parsed
        // (Chrome keeps `opacity(round(50%, 30%))`).
        ("filter", "opacity(round(50%, 30%))", "opacity(60%)"),
        ("font-stretch", "round(55%, 20%)", "60%"),
    ] {
        assert_eq!(
            specified(property, value).as_deref(),
            Some(written),
            "{property}: {value}"
        );
    }
}

/// Their arguments' kinds as css-values-4 has them: one kind for all,
/// numbers or angles for the trigonometric ones, numbers for the inverse
/// and exponential ones, a step for `round()` only of a number; and, in
/// machopo, no length mixed with a percentage, nor a number only an element
/// works out where a number is taken (Chrome takes both: see the
/// limitations).
#[test]
fn the_other_math_functions_that_dont_parse() {
    for value in [
        "round(117px)",
        "round(1px, 2)",
        "round(foo, 1px, 2px)",
        "round(up 1px, 2px)",
        "mod(1px)",
        "rem(1px, 2px, 3px)",
        "calc(1px * sin(1px))",
        "calc(1px * pow(1px, 2))",
        "calc(1px * asin(1deg))",
        "calc(1px * atan2(1px, 1))",
        "calc(1px * exp(1, 2))",
        "calc(1px * log())",
        "hypot()",
        "calc(10px * sign(1em) * 2px)",
        "calc(10px / round(1em, 3px))",
        "round(1em, 3%)",
        "abs(10% - 1px)",
    ] {
        assert_eq!(specified("width", value), None, "{value}");
    }
    assert_eq!(specified("opacity", "sign(1em)"), None);
    assert_eq!(specified("opacity", "exp(1deg)"), None);
    assert_eq!(specified("transform", "rotate(asin(1deg))"), None);
    assert_eq!(specified("z-index", "round(1em, 1px)"), None);
}

/// Worked out for the element (10px fonts): as Chrome 152 computes them
/// (its used lengths are in 1/64 px).
#[test]
fn the_other_math_functions_computed() {
    let cases = [
        ("round(1em, 3px)", 9.0),
        ("round(up, 1em, 3px)", 12.0),
        ("round(down, 1em, 3px)", 9.0),
        ("round(to-zero, -1em, 3px)", -9.0),
        ("round(7em, 2em)", 80.0),
        ("round(1.5em, 1em)", 20.0),
        ("mod(1em, 3px)", 1.0),
        ("mod(-1em, 3px)", 2.0),
        ("mod(1em, -3px)", -2.0),
        ("rem(-1em, 3px)", -1.0),
        ("rem(1em, -3px)", 1.0),
        ("abs(-1em)", 10.0),
        ("abs(1em - 3px)", 7.0),
        ("hypot(3em, 40px)", 50.0),
        ("calc(10px * sign(1em))", 10.0),
        ("calc(10px / sign(-1em))", -10.0),
        ("calc(2 * (10px / sign(-1em)))", -20.0),
        ("calc(1px + 2px * sign(1em))", 3.0),
        ("calc(1px - 2px * sign(1em))", -1.0),
        ("calc(1px * (1 + sign(1em)))", 2.0),
        ("calc(2 * round(1em, 3px))", 18.0),
        ("calc(-1px + round(1em, 3px))", 8.0),
        ("calc(1px * pow(sign(-1em), 2))", 1.0),
        ("calc(10px * sign(1em - 10px))", 0.0),
        ("max(1px, round(1em, 3px))", 9.0),
        // A -0 of a relative length is the smaller zero (1 / -0 is
        // -infinity, clamped).
        (
            "calc(1px / sign(min(-0em, 0px)))",
            -computed_style::MAX_CSS_LENGTH,
        ),
        (
            "calc(1px / sign(max(-0em, 0px)))",
            computed_style::MAX_CSS_LENGTH,
        ),
        ("calc(1px / sign(-0em))", -computed_style::MAX_CSS_LENGTH),
    ];
    let html: String = cases
        .iter()
        .enumerate()
        .map(|(i, (value, _))| format!("<p id=p{i} style='margin-left: {value}'>"))
        .collect();
    let s = styles(&format!("<div style='font-size: 10px'>{html}</div>"));
    for (i, (value, px)) in cases.iter().enumerate() {
        assert_eq!(
            s[&format!("p{i}")].margin.left,
            LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(*px)),
            "{value}"
        );
    }
    // Of percentages: a percentage still.
    let s = styles(
        "<p id=a style='width: round(10%, 3%)'><p id=b style='width: calc(10% * sign(1em))'>",
    );
    assert_eq!(
        s["a"].width,
        Size::LengthPercentage(LengthPercentage::Percent(9.0))
    );
    assert_eq!(
        s["b"].width,
        Size::LengthPercentage(LengthPercentage::Percent(10.0))
    );
}
