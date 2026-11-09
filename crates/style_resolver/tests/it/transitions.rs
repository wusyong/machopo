//! The `transition-*` properties and easing functions as declared: what
//! they read back as (Chrome's, as WPT's `css/css-transitions/parsing/`
//! and `css/css-easing/` check them), and how an easing function maps
//! progress.

use computed_style::{EasingFunction, StepPosition};
use style_resolver::CssPropertyValueSet;

/// What `property: value` reads back as on its own, or `None` if not set.
pub(crate) fn read_back(property: &str, value: &str) -> Option<String> {
    let mut block = CssPropertyValueSet::default();
    block.set_property(property, value, "");
    let text = block.property_value(property);
    (!text.is_empty()).then_some(text)
}

#[track_caller]
pub(crate) fn assert_read_back(property: &str, cases: &[(&str, &str)]) {
    for &(value, expected) in cases {
        assert_eq!(
            read_back(property, value).as_deref(),
            Some(expected),
            "{property}: {value}"
        );
    }
}

#[track_caller]
pub(crate) fn assert_invalid(property: &str, values: &[&str]) {
    for value in values {
        assert_eq!(read_back(property, value), None, "{property}: {value}");
    }
}

#[track_caller]
fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "{what}: {actual}, not {expected}"
    );
}

#[test]
fn easing_functions_read_back_as_chrome_writes_them() {
    let same = [
        "linear",
        "ease",
        "ease-in",
        "ease-out",
        "ease-in-out",
        "cubic-bezier(0.1, 0.2, 0.8, 0.9)",
        "cubic-bezier(0, -2, 1, 3)",
        "steps(4)",
        "steps(4, start)",
        "steps(2, jump-start)",
        "steps(2, jump-both)",
        "steps(2, jump-none)",
        "linear(0 0%, 0.5 50%, 1 100%)",
        "linear(0, 1)",
        "linear(0 0% 50%, 1 50% 100%)",
        "linear, ease, linear",
    ];
    for value in same {
        assert_read_back("transition-timing-function", &[(value, value)]);
    }
    assert_read_back(
        "transition-timing-function",
        &[
            ("step-start", "steps(1, start)"),
            ("step-end", "steps(1)"),
            ("steps(2, end)", "steps(2)"),
            ("steps(2, jump-end)", "steps(2)"),
            ("steps( 2, end )", "steps(2)"),
            ("linear( 0 0%, 1 100% )", "linear(0 0%, 1 100%)"),
            (
                "linear(0 calc(50% - 50%), 0 calc(50% + 50%))",
                "linear(0 calc(0%), 0 calc(100%))",
            ),
            // A math function's count is rounded and clamped; a count
            // past Blink's integer, to its largest (Chrome 152).
            ("steps(calc(-10), start)", "steps(1, start)"),
            ("steps(calc(5 / 2), start)", "steps(3, start)"),
            ("steps(calc(0 / 0))", "steps(1)"),
            ("steps(calc(1.5), jump-none)", "steps(2, jump-none)"),
            (
                "steps(calc(infinity), jump-none)",
                "steps(2147483647, jump-none)",
            ),
            ("steps(3000000000)", "steps(2147483647)"),
            ("steps(2147483647)", "steps(2147483647)"),
            (
                "cubic-bezier(calc(-2), 0.5, calc(1.5), 0)",
                "cubic-bezier(0, 0.5, 1, 0)",
            ),
        ],
    );
    assert_invalid(
        "transition-timing-function",
        &[
            "auto",
            "ease-in ease-out",
            "cubic-bezier(foobar)",
            "cubic-bezier(1, 2, 3)",
            "cubic-bezier(1, 2, 3, infinite)",
            "cubic-bezier(1, 2, 3, 4, 5)",
            "cubic-bezier(-0.1, 0.1, 0.5, 0.9)",
            "cubic-bezier(0.5, 0.1, 1.1, 0.9)",
            "steps(foobar)",
            "steps(3.3, end)",
            "steps(3, top)",
            "steps(0, jump-start)",
            "steps(0, start)",
            "steps(1, jump-none)",
            // Too few for `jump-none` as written, as Chrome 152 has it.
            "steps(calc(1), jump-none)",
            "steps(calc(1.4), jump-none)",
            "steps(calc(0 / 0), jump-none)",
            "steps(2, ())",
            "linear()",
            "linear(0)",
            "linear(100%)",
            "linear(0% 1 50%)",
            "linear(0 0% 100%)",
            "initial, cubic-bezier(0, -2, 1, 3)",
        ],
    );
}

/// Lists of different lengths can't be written as the shorthand.
#[test]
fn lists_of_different_lengths_are_no_shorthand() {
    let mut block = CssPropertyValueSet::default();
    block.set_property("transition", "top 1s", "");
    block.set_property("transition-duration", "1s, 2s", "");
    assert_eq!(block.property_value("transition"), "");
}

/// The keywords and `cubic-bezier()` (css-easing 2): the curve's y at x,
/// and its tangents outside 0 to 1.
#[test]
fn cubic_bezier_easing() {
    let ease = EasingFunction::Ease;
    assert_eq!(ease.evaluate(0.0, false), 0.0);
    assert_close(ease.evaluate(1.0, false), 1.0, "ease at 1");
    // The curves' exact values (found by bisection).
    assert_close(ease.evaluate(0.25, false), 0.408_511, "ease at 0.25");
    assert_close(ease.evaluate(0.5, false), 0.802_403, "ease at 0.5");
    assert_close(
        EasingFunction::EaseIn.evaluate(0.5, false),
        0.315_357,
        "ease-in",
    );
    assert_close(
        EasingFunction::EaseOut.evaluate(0.5, false),
        0.684_643,
        "ease-out",
    );
    assert_close(
        EasingFunction::EaseInOut.evaluate(0.5, false),
        0.5,
        "ease-in-out",
    );
    assert_eq!(EasingFunction::Linear.evaluate(0.3, false), 0.3);
    // A straight line is the identity.
    let straight = EasingFunction::CubicBezier([0.25, 0.25, 0.75, 0.75]);
    assert_close(straight.evaluate(0.3, false), 0.3, "straight");
    // Outside 0 to 1, the tangent at the end goes on: here from the
    // control points, the second where the first is at the start.
    let curve = EasingFunction::CubicBezier([0.5, 1.0, 0.5, 1.0]);
    assert_close(
        curve.evaluate(-0.5, false),
        -1.0,
        "slope 2 before the start",
    );
    assert_close(curve.evaluate(1.5, false), 1.0, "flat after the end");
    let curve = EasingFunction::CubicBezier([0.0, 0.0, 0.5, 2.0]);
    assert_close(
        curve.evaluate(-1.0, false),
        -4.0,
        "the second point's slope",
    );
    // Both control points at an end: a straight line there.
    let ends = EasingFunction::CubicBezier([1.0, 1.0, 1.0, 1.0]);
    assert_close(ends.evaluate(1.5, false), 1.5, "slope 1 after the end");
    let starts = EasingFunction::CubicBezier([0.0, 0.0, 0.0, 0.0]);
    assert_close(
        starts.evaluate(-0.5, false),
        -0.5,
        "slope 1 before the start",
    );
    // Overshooting outputs.
    let back = EasingFunction::CubicBezier([0.0, -2.0, 1.0, 3.0]);
    assert!(back.evaluate(0.1, false) < 0.0);
    assert!(back.evaluate(0.9, false) > 1.0);
}

/// `steps()` (css-easing 2, "the step easing functions"), each position,
/// and the before flag at a jump.
#[test]
fn step_easing() {
    use StepPosition::*;
    let at = |steps, position, input| EasingFunction::Steps(steps, position).evaluate(input, false);
    assert_eq!(at(4, End, 0.0), 0.0);
    assert_eq!(at(4, End, 0.3), 0.25);
    assert_eq!(at(4, End, 0.99), 0.75);
    assert_eq!(at(4, End, 1.0), 1.0);
    assert_eq!(at(4, JumpEnd, 0.3), 0.25);
    assert_eq!(at(4, Start, 0.0), 0.25);
    assert_eq!(at(4, JumpStart, 0.3), 0.5);
    assert_eq!(at(4, Start, 1.0), 1.0, "not past the end");
    assert_eq!(at(3, JumpNone, 0.0), 0.0);
    assert_eq!(at(3, JumpNone, 0.5), 0.5);
    assert_eq!(at(3, JumpNone, 1.0), 1.0);
    assert_eq!(at(1, JumpBoth, 0.0), 0.5);
    assert_eq!(at(1, JumpBoth, 1.0), 1.0);
    // Before the active interval, at a jump, the step before it.
    let start = EasingFunction::Steps(1, Start);
    assert_eq!(start.evaluate(0.0, false), 1.0);
    assert_eq!(start.evaluate(0.0, true), 0.0, "the before flag");
    // Outside 0 to 1.
    assert_eq!(at(4, End, -0.5), -0.5);
    assert_eq!(at(4, End, 1.5), 1.5);
}
