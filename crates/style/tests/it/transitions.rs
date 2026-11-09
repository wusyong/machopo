//! The `transition-*` properties and easing functions: what they read
//! back as declared and computed (Chrome's, as WPT's
//! `css/css-transitions/parsing/` and `css/css-easing/` check them).

use computed_style::ComputedStyle;
use style_resolver::{CssPropertyValueSet, computed_value, shorthand_longhands, shorthand_value};

use crate::Page;

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

/// The style of the element `#t` in `html`.
pub(crate) fn style_of(html: &str) -> ComputedStyle {
    (*Page::new(html).style("t")).clone()
}

/// What `getComputedStyle(#t)[property]` reads, `property: value` set on
/// `#t`; a shorthand made back from its computed longhands.
pub(crate) fn computed(property: &str, value: &str) -> String {
    let style = style_of(&format!(
        "<style>#t {{ {property}: {value} }}</style><div id=t></div>"
    ));
    match shorthand_longhands(property) {
        Some(longhands) => {
            let values: Vec<(&str, String)> = longhands
                .into_iter()
                .map(|name| (name, computed_value(&style, name).expect("a longhand")))
                .collect();
            shorthand_value(property, &values).expect("written as the shorthand")
        }
        None => computed_value(&style, property).expect("a longhand"),
    }
}

#[track_caller]
pub(crate) fn assert_computed(property: &str, cases: &[(&str, &str)]) {
    for &(value, expected) in cases {
        assert_eq!(computed(property, value), expected, "{property}: {value}");
    }
}

#[test]
fn times_keep_their_unit_and_compute_to_seconds() {
    assert_read_back(
        "transition-duration",
        &[
            ("0s", "0s"),
            ("500ms", "500ms"),
            ("1s, 2s", "1s, 2s"),
            ("calc(2 * 3s)", "6s"),
            ("calc(1s - 2s)", "0s"),
        ],
    );
    assert_invalid(
        "transition-duration",
        &[
            "infinite",
            "-500ms",
            "1s 2s",
            "1s, initial",
            "initial, 1s",
            "0",
            "1px",
        ],
    );
    assert_read_back(
        "transition-delay",
        &[("0s", "0s"), ("500ms", "500ms"), ("-1s, -2s", "-1s, -2s")],
    );
    assert_invalid(
        "transition-delay",
        &[
            "infinite",
            "0",
            "500ms 0.5s",
            "-3s, initial",
            "initial, -3s",
        ],
    );
    assert_computed(
        "transition-duration",
        &[
            ("500ms", "0.5s"),
            ("calc(2 * 3s)", "6s"),
            ("20s, 10s", "20s, 10s"),
        ],
    );
    assert_computed("transition-delay", &[("-500ms, calc(2 * 3s)", "-0.5s, 6s")]);
}

/// `linear()` computes to every point with its input filled in.
#[test]
fn linear_fills_in_its_inputs() {
    assert_computed(
        "transition-timing-function",
        &[
            ("linear(0, 1)", "linear(0 0%, 1 100%)"),
            (
                "linear(-10, -5, 0, 5, 10)",
                "linear(-10 0%, -5 25%, 0 50%, 5 75%, 10 100%)",
            ),
            (
                "linear(-10 -10%, -5 -5%, 0, 5, 10)",
                "linear(-10 -10%, -5 -5%, 0 30%, 5 65%, 10 100%)",
            ),
            (
                "linear(0 calc(50% - 50%), 0 calc(50% + 50%))",
                "linear(0 0%, 0 100%)",
            ),
            (
                "linear(0 0% 50%, 1 50% 100%)",
                "linear(0 0%, 0 50%, 1 50%, 1 100%)",
            ),
            (
                "linear(0, 0 40%, 1, 0.5, 1)",
                "linear(0 0%, 0 40%, 1 60%, 0.5 80%, 1 100%)",
            ),
            // An input below the one before it is raised to it.
            ("linear(0 50%, 1 20%)", "linear(0 50%, 1 50%)"),
            ("step-start", "steps(1, start)"),
            ("linear, ease, linear", "linear, ease, linear"),
        ],
    );
}

#[test]
fn transition_property_keeps_names_as_written() {
    for value in [
        "none",
        "all",
        "one",
        "one-two-three",
        "one, two, three",
        "width, all",
        "width, top, width",
        "all, top, all",
        "left, top, right, bottom",
    ] {
        assert_read_back("transition-property", &[(value, value)]);
        assert_computed("transition-property", &[(value, value)]);
    }
    assert_read_back(
        "transition-property",
        &[("ALL, INVALID, SYNTAX, SRC", "all, INVALID, SYNTAX, SRC")],
    );
    let mut invalid = vec!["one two three", "1, 2, 3", "none, one"];
    let keywords = [
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
        "default",
        "none",
    ];
    let lists: Vec<String> = keywords
        .iter()
        .flat_map(|k| [format!("top, {k}"), format!("{k}, top")])
        .collect();
    invalid.extend(lists.iter().map(String::as_str));
    assert_invalid("transition-property", &invalid);
}

#[test]
fn transition_behavior() {
    for value in ["normal", "allow-discrete", "normal, allow-discrete"] {
        assert_read_back("transition-behavior", &[(value, value)]);
        assert_computed("transition-behavior", &[(value, value)]);
    }
    assert_invalid(
        "transition-behavior",
        &["discrete", "normal allow-discrete"],
    );
}

/// The shorthand writes each transition's values but the initial ones,
/// in Chrome's order; the duration stays when there is a delay.
#[test]
fn the_transition_shorthand() {
    let cases = [
        ("1s", "1s"),
        ("cubic-bezier(0, -2, 1, 3)", "cubic-bezier(0, -2, 1, 3)"),
        ("1s -3s", "1s -3s"),
        // A delay needs the duration before it, even 0s.
        ("top 0s 1s", "top 0s 1s"),
        ("none", "none"),
        ("top", "top"),
        ("all", "all"),
        ("all 1s", "1s"),
        ("0s 0s ease all normal", "all"),
        (
            "1s -3s cubic-bezier(0, -2, 1, 3) top",
            "top 1s cubic-bezier(0, -2, 1, 3) -3s",
        ),
        (
            "1s -3s, cubic-bezier(0, -2, 1, 3) top",
            "1s -3s, top cubic-bezier(0, -2, 1, 3)",
        ),
        ("allow-discrete display", "display allow-discrete"),
        (
            "display 3s allow-discrete ease-in-out 1s",
            "display 3s ease-in-out 1s allow-discrete",
        ),
        ("color .2s ease-out", "color 0.2s ease-out"),
    ];
    assert_read_back("transition", &cases);
    assert_computed(
        "transition",
        &[
            (
                "1s -3s, cubic-bezier(0, -2, 1, 3) top",
                "1s -3s, top cubic-bezier(0, -2, 1, 3)",
            ),
            ("all, all", "all, all"),
            ("color 200ms ease-out", "color 0.2s ease-out"),
            ("none", "none"),
        ],
    );
    assert_invalid(
        "transition",
        &[
            "1s 2s 3s",
            "-1s -2s",
            "steps(1) steps(2)",
            "none top",
            "top, none",
            "initial 1s",
            "asdf display",
            "display asdf",
        ],
    );
    // The longhands it sets, the ones it leaves out initial.
    assert_eq!(
        read_back("transition-duration", "0s").as_deref(),
        Some("0s"),
        "a longhand on its own"
    );
    let mut block = CssPropertyValueSet::default();
    block.set_property("transition", "top 1s, left steps(2) 2s 3s", "");
    for (longhand, expected) in [
        ("transition-property", "top, left"),
        ("transition-duration", "1s, 2s"),
        ("transition-timing-function", "ease, steps(2)"),
        ("transition-delay", "0s, 3s"),
        ("transition-behavior", "normal, normal"),
    ] {
        assert_eq!(block.property_value(longhand), expected, "{longhand}");
    }
}

/// The properties aren't inherited: a child's transitions are its own.
#[test]
fn transitions_are_not_inherited() {
    let style =
        style_of("<style>#p { transition: color 1s } </style><div id=p><div id=t></div></div>");
    assert_eq!(
        computed_value(&style, "transition-property").as_deref(),
        Some("all")
    );
    assert_eq!(
        computed_value(&style, "transition-duration").as_deref(),
        Some("0s")
    );
    let style = style_of(
        "<style>#p { transition: color 1s } #t { transition: inherit }</style>\
         <div id=p><div id=t></div></div>",
    );
    assert_eq!(
        computed_value(&style, "transition-property").as_deref(),
        Some("color")
    );
    assert_eq!(
        computed_value(&style, "transition-duration").as_deref(),
        Some("1s")
    );
}

#[track_caller]
fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-4,
        "{what}: {actual}, not {expected}"
    );
}

/// `linear()`: between the points around the input, the first or the
/// last two points extended outside them, a point repeated at a jump.
#[test]
fn linear_easing() {
    let style = style_of(
        "<style>#t { transition-timing-function: linear(0, 0.25 75%, 1) }</style><div id=t></div>",
    );
    let easing = &style.transitions.timing_function[0];
    assert_close(easing.evaluate(0.0, false), 0.0, "start");
    assert_close(easing.evaluate(0.375, false), 0.125, "first segment");
    assert_close(easing.evaluate(0.75, false), 0.25, "the point");
    assert_close(easing.evaluate(0.875, false), 0.625, "last segment");
    assert_close(easing.evaluate(1.25, false), 1.75, "extended past the end");
    assert_close(
        easing.evaluate(-0.75, false),
        -0.25,
        "extended before the start",
    );
    let style = style_of(
        "<style>#t { transition-timing-function: linear(0 0% 50%, 1 50% 100%) }</style><div id=t></div>",
    );
    let jump = &style.transitions.timing_function[0];
    assert_close(jump.evaluate(0.25, false), 0.0, "flat before the jump");
    assert_close(
        jump.evaluate(0.5, false),
        1.0,
        "the later point at the jump",
    );
    assert_close(jump.evaluate(0.75, false), 1.0, "flat after the jump");
    // A jump at 20% (not exact in binary): a progress of 0.2 is at it, so
    // past it, as `steps(5, jump-start)` is (WPT's
    // `linear-timing-functions-output.html`).
    let style = style_of(
        "<style>#t { transition-timing-function: linear(0.2 0% 20%, 0.4 20% 40%, 0.6 40% 60%, 0.8 60% 80%, 1 80% 100%) }</style><div id=t></div>",
    );
    let steps = &style.transitions.timing_function[0];
    assert_close(steps.evaluate(0.2, false), 0.4, "the later point at 20%");
    // And at 60%, which an `f32` holds as a little more than 0.6.
    assert_close(steps.evaluate(0.6, false), 0.8, "the later point at 60%");
    // A control point as written, not as an `f32`: a progress of 0.3 is
    // 0.3 (Blink's doubles).
    let b = (8.0 * 0.3 - 1.0) / 6.0;
    let style = style_of(&format!(
        "<style>#t {{ transition-timing-function: cubic-bezier(0, {b}, 1, {b}) }}</style><div id=t></div>"
    ));
    let curve = &style.transitions.timing_function[0];
    let progress = curve.evaluate(0.5, false);
    assert!((progress - 0.3).abs() < 1e-12, "{progress}");
}
