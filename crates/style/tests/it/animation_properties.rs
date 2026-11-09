//! The `animation-*` properties and the `animation` shorthand: what they
//! read back as declared and computed (Chrome's, as WPT's
//! `css/css-animations/parsing/` checks them).

use computed_style::{AnimationName, AnimationPlayState, FillMode, PlaybackDirection};
use style_resolver::{computed_value, shorthand_longhands, shorthand_value};

use crate::transitions::{assert_computed, assert_invalid, assert_read_back, style_of};

#[test]
fn animation_name_is_an_identifier_or_a_string() {
    assert_read_back(
        "animation-name",
        &[
            ("none", "none"),
            ("NONE", "none"),
            ("Both", "Both"),
            ("ease-in", "ease-in"),
            ("first, second, third", "first, second, third"),
            ("\"something\"", "something"),
            ("\"---\\22---\"", "---\\\"---"),
            ("\"multi word string\"", "multi\\ word\\ string"),
            ("\"none\"", "\"none\""),
            ("\"NoNe\"", "\"NoNe\""),
            (
                "\"none\", \"INITIAL\", \"inherit\"",
                "\"none\", \"INITIAL\", \"inherit\"",
            ),
            ("\"revert-layer\"", "\"revert-layer\""),
            ("\"default\"", "\"default\""),
            ("anim, none", "anim, none"),
        ],
    );
    assert_invalid(
        "animation-name",
        &[
            "12",
            "one two",
            "one, initial",
            "default, two",
            "revert-layer, four",
            "\"\"",
        ],
    );
    assert_computed(
        "animation-name",
        &[("\"something\"", "something"), ("\"none\"", "\"none\"")],
    );
    // An empty string is no name: the declaration is dropped.
    let style =
        style_of("<style>#t { animation-name: a; animation-name: \"\" }</style><div id=t></div>");
    assert_eq!(
        &*style.rare.animations.name,
        [AnimationName::Name("a".into())]
    );
}

#[test]
fn animation_longhands() {
    assert_read_back(
        "animation-duration",
        &[
            ("3s", "3s"),
            ("500ms", "500ms"),
            ("auto", "auto"),
            ("1s, auto", "1s, auto"),
        ],
    );
    assert_invalid(
        "animation-duration",
        &["-3s", "0", "infinite", "1s 2s", "initial, 1s"],
    );
    assert_computed(
        "animation-duration",
        &[
            ("500ms", "0.5s"),
            ("calc(2 * 3s)", "6s"),
            ("auto, auto", "0s, 0s"),
        ],
    );
    assert_read_back(
        "animation-delay",
        &[("-5ms", "-5ms"), ("20s, 10s", "20s, 10s")],
    );
    assert_invalid("animation-delay", &["infinite", "0", "1s 2s"]);
    assert_computed("animation-delay", &[("-500ms", "-0.5s")]);
    assert_read_back(
        "animation-iteration-count",
        &[
            ("0", "0"),
            ("4.5", "4.5"),
            ("infinite", "infinite"),
            ("0, infinite, 3", "0, infinite, 3"),
        ],
    );
    assert_invalid(
        "animation-iteration-count",
        &["auto", "-2", "3 4", "initial, 4"],
    );
    assert_computed(
        "animation-iteration-count",
        &[("calc(-2)", "0"), ("INFINITE", "infinite")],
    );
    assert_read_back(
        "animation-direction",
        &[(
            "normal, reverse, alternate, alternate-reverse",
            "normal, reverse, alternate, alternate-reverse",
        )],
    );
    assert_invalid("animation-direction", &["auto", "normal reverse"]);
    assert_read_back(
        "animation-fill-mode",
        &[(
            "none, forwards, backwards, both",
            "none, forwards, backwards, both",
        )],
    );
    assert_invalid("animation-fill-mode", &["auto", "forwards backwards"]);
    assert_read_back(
        "animation-play-state",
        &[("running, paused", "running, paused")],
    );
    assert_invalid("animation-play-state", &["auto", "paused running"]);
    assert_read_back(
        "animation-timing-function",
        &[("steps(1), ease", "steps(1), ease")],
    );
    assert_invalid("animation-timing-function", &["steps(2, ())"]);
}

#[test]
fn the_animation_shorthand() {
    assert_read_back(
        "animation",
        &[
            ("1s", "1s"),
            ("cubic-bezier( 0, -2, 1, 3 )", "cubic-bezier(0, -2, 1, 3)"),
            ("1s -3s", "1s -3s"),
            ("4", "4"),
            ("reverse", "reverse"),
            ("both", "both"),
            ("paused", "paused"),
            ("none", "none"),
            ("anim", "anim"),
            (
                "anim paused both reverse 4 1s -3s cubic-bezier(0, -2, 1, 3)",
                "1s cubic-bezier(0, -2, 1, 3) -3s 4 reverse both paused anim",
            ),
            (
                "anim paused both reverse, 4 1s -3s cubic-bezier(0, -2, 1, 3)",
                "reverse both paused anim, 1s cubic-bezier(0, -2, 1, 3) -3s 4",
            ),
            // A keyword a longhand before the name takes is the name only
            // once that longhand has its value, and is written so.
            ("ease ease", "ease ease"),
            ("ease-in ease", "ease-in ease"),
            ("infinite infinite", "infinite infinite"),
            ("1 infinite", "1 infinite"),
            ("normal normal", "normal normal"),
            ("both forwards", "both forwards"),
            ("paused running", "paused running"),
            ("auto auto", "auto auto"),
            ("none none", "none"),
            ("\"none\"", "\"none\""),
            // A negative time can only be the delay.
            ("-3s", "auto -3s"),
            ("auto 2s", "auto 2s"),
        ],
    );
    assert_invalid(
        "animation",
        &[
            "1s 2s 3s",
            "-1s -2s",
            "steps(1) steps(2)",
            "1 2",
            "reverse alternate alternate-reverse anim",
            "both backwards forwards anim",
            "paused running paused anim",
            "anim1 timeline1 anim2",
            "anim,",
        ],
    );
    assert_computed(
        "animation",
        &[
            ("1s", "1s"),
            ("ease-in-out", "ease-in-out"),
            ("none", "none"),
            ("none, none", "none, none"),
            ("anim", "anim"),
            (
                "anim paused both reverse, 4 1s -3s cubic-bezier(0, -2, 1, 3)",
                "reverse both paused anim, 1s cubic-bezier(0, -2, 1, 3) -3s 4",
            ),
            ("0s 1s", "0s 1s"),
        ],
    );
}

#[test]
fn lists_of_different_lengths_are_no_shorthand() {
    let style =
        style_of("<style>#t { animation: a 1s; animation-delay: 1s, 2s }</style><div id=t></div>");
    let values: Vec<(&str, String)> = shorthand_longhands("animation")
        .expect("a shorthand")
        .into_iter()
        .map(|name| (name, computed_value(&style, name).expect("a longhand")))
        .collect();
    assert_eq!(shorthand_value("animation", &values), None);
}

#[test]
fn animation_values_compute() {
    let style = style_of(
        "<style>#t { animation: spin 2s linear -1s infinite alternate-reverse both paused, \
         \"fade\" 500ms }</style><div id=t></div>",
    );
    let animations = &style.rare.animations;
    assert_eq!(
        &*animations.name,
        [
            AnimationName::Name("spin".into()),
            AnimationName::Name("fade".into())
        ]
    );
    assert_eq!(&*animations.duration, [2.0, 0.5]);
    assert_eq!(&*animations.delay, [-1.0, 0.0]);
    assert_eq!(&*animations.iteration_count, [f32::INFINITY, 1.0]);
    assert_eq!(
        &*animations.direction,
        [
            PlaybackDirection::AlternateReverse,
            PlaybackDirection::Normal
        ]
    );
    assert_eq!(&*animations.fill_mode, [FillMode::Both, FillMode::None]);
    assert_eq!(
        &*animations.play_state,
        [AnimationPlayState::Paused, AnimationPlayState::Running]
    );
    let timing = animations.timing(3);
    assert_eq!((timing.duration, timing.iteration_count), (0.5, 1.0));
}

#[test]
fn animations_are_not_inherited() {
    let style = style_of(
        "<style>#p { animation: spin 2s } #t { animation-duration: inherit }</style>\
         <div id=p><div id=t></div></div>",
    );
    assert_eq!(&*style.rare.animations.name, [AnimationName::None]);
    assert_eq!(&*style.rare.animations.duration, [2.0]);
}

/// `animation-timeline` and `animation-range`'s longhands take only the
/// values machopo's animations follow (`auto`, `normal`), lists of them; the
/// rest (a scroll or view timeline, a range) isn't supported, so that
/// `@supports` says so. `animation-range` is its longhands' shorthand.
#[test]
fn the_timeline_and_the_range() {
    assert_read_back(
        "animation-timeline",
        &[("AUTO", "auto"), ("auto, auto", "auto, auto")],
    );
    assert_invalid(
        "animation-timeline",
        &["none", "--foo", "scroll()", "view()", "auto auto"],
    );
    for property in ["animation-range-start", "animation-range-end"] {
        assert_read_back(
            property,
            &[("normal", "normal"), ("normal, normal", "normal, normal")],
        );
        assert_invalid(property, &["10%", "entry", "entry 10%", "auto"]);
    }
    assert_read_back(
        "animation-range",
        &[
            ("normal", "normal"),
            ("normal normal", "normal"),
            ("normal, normal normal", "normal, normal"),
        ],
    );
    assert_invalid("animation-range", &["normal 10%", "normal normal normal"]);
    let css_text = |css: &str| style_resolver::CssPropertyValueSet::parse(css).css_text();
    assert_eq!(
        css_text("animation-range-start: normal; animation-range-end: normal"),
        "animation-range: normal;"
    );
    assert_eq!(
        css_text("animation-range-start: normal, normal; animation-range-end: normal"),
        "animation-range-start: normal, normal; animation-range-end: normal;"
    );
    assert_computed(
        "animation-timeline",
        &[("auto", "auto"), ("auto, auto", "auto, auto")],
    );
    assert_computed(
        "animation-range-end",
        &[("normal, normal", "normal, normal")],
    );
}

/// `animation` resets `animation-timeline` and `animation-range` (it can't
/// set them), and is written only while they are as it resets them, one
/// `auto` and one `normal` (Chrome's).
#[test]
fn the_animation_shorthand_resets_the_timeline_and_the_range() {
    let css_text = |css: &str| style_resolver::CssPropertyValueSet::parse(css).css_text();
    assert_eq!(
        css_text("animation-timeline: auto, auto; animation: a 1s, b 2s"),
        "animation: 1s a, 2s b;"
    );
    assert_eq!(
        css_text("animation: a 1s, b 2s; animation-timeline: auto"),
        "animation: 1s a, 2s b;"
    );
    assert_eq!(
        css_text("animation: a 1s; animation-range: normal normal"),
        "animation: 1s a;"
    );
    let split = css_text("animation: a 1s, b 2s; animation-timeline: auto, auto");
    assert!(split.starts_with("animation-duration: 1s, 2s;"), "{split}");
    assert!(
        split.ends_with(
            "animation-name: a, b; animation-range: normal; animation-timeline: auto, auto;"
        ),
        "{split}"
    );
    let split = css_text("animation: a 1s; animation-timeline: initial");
    assert!(split.ends_with("animation-timeline: initial;"), "{split}");
    // The computed shorthand too.
    let style = style_of(
        "<style>#t { animation: a 1s, b 2s; animation-timeline: auto, auto }</style><div id=t></div>",
    );
    let values: Vec<(&str, String)> = shorthand_longhands("animation")
        .expect("a shorthand")
        .into_iter()
        .map(|name| (name, computed_value(&style, name).expect("a longhand")))
        .collect();
    assert_eq!(shorthand_value("animation", &values), None);
}
