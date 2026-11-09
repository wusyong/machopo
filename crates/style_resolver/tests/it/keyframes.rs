//! `@keyframes`: what it parses to and how CSSOM writes it (Chrome's, as
//! WPT's `css/css-animations/parsing/` and `css/cssom/CSSKeyframe*` check
//! it).

use style_resolver::{CssRule, StyleSheetContents};

fn css_texts(css: &str) -> Vec<String> {
    StyleSheetContents::parse(css, None)
        .rules()
        .iter()
        .map(CssRule::css_text)
        .collect()
}

/// The keyframes of the sheet's only rule, a `@keyframes`, by `keyText`
/// and declarations.
fn keyframes(css: &str) -> Vec<(String, String)> {
    let contents = StyleSheetContents::parse(css, None);
    let [CssRule::Keyframes(rule)] = contents.rules() else {
        panic!("not one @keyframes: {css}");
    };
    rule.keyframes()
        .map(|keyframe| (keyframe.key_text(), keyframe.block.css_text()))
        .collect()
}

#[test]
fn keyframes_serialize_as_chrome_writes_them() {
    assert_eq!(
        css_texts("@keyframes foo { 0% { top: 0px } 100% { top: 200px } } @keyframes empty {}"),
        [
            "@keyframes foo { \n  0% { top: 0px; }\n  100% { top: 200px; }\n}",
            "@keyframes empty { \n}",
        ]
    );
    assert_eq!(
        css_texts("@keyframes \"initial\" {} @keyframes \"none\" {} @keyframes \"a b\" {}"),
        [
            "@keyframes \"initial\" { \n}",
            "@keyframes \"none\" { \n}",
            "@keyframes a\\ b { \n}",
        ]
    );
    assert_eq!(
        css_texts("@keyframes a { from {} }"),
        ["@keyframes a { \n  0% { }\n}"]
    );
}

#[test]
fn a_keyframes_name_is_an_identifier_or_a_string() {
    for name in [
        "foo",
        "-foo",
        "_bar",
        "EXAMPLE",
        "ease-out",
        "auto",
        "normal",
        "\"none\"",
        "\"initial\"",
        "\"one, two\"",
    ] {
        assert_eq!(
            css_texts(&format!("@keyframes {name} {{}}")).len(),
            1,
            "{name}"
        );
    }
    for name in [
        "none",
        "NONE",
        "default",
        "initial",
        "inherit",
        "unset",
        "revert",
        "revert-layer",
        "12",
        "foo.bar",
        "one two",
        "one, two",
        "\"\"",
    ] {
        assert!(
            css_texts(&format!("@keyframes {name} {{}}")).is_empty(),
            "{name}"
        );
    }
    let contents = StyleSheetContents::parse("@keyframes \"a b\" {} @keyframes Foo {}", None);
    let names: Vec<&str> = contents
        .rules()
        .iter()
        .map(|rule| match rule {
            CssRule::Keyframes(rule) => &*rule.name,
            _ => panic!("a @keyframes"),
        })
        .collect();
    assert_eq!(names, ["a b", "Foo"]);
}

#[test]
fn keyframe_selectors_are_offsets() {
    assert_eq!(
        keyframes(
            "@keyframes t { from { opacity: 0 } to { opacity: 1 } 50% { opacity: 0.5 } \
             25%, 75% { opacity: 0.3 } .5% {} 1e1% {} 100.0% {} calc(10% + 5%) {} \
             FROM, To {} }"
        )
        .into_iter()
        .map(|(key, _)| key)
        .collect::<Vec<_>>(),
        [
            "0%", "100%", "50%", "25%, 75%", "0.5%", "10%", "100%", "15%", "0%, 100%"
        ]
    );
    // A keyframe whose selector isn't valid is dropped, not the rule.
    assert_eq!(
        keyframes(
            "@keyframes t { -10% {} 120% {} entry 10% {} 10 {} 10%, {} foo {} 20% {} \
             @media all { 30% {} } }"
        ),
        [("20%".to_owned(), String::new())]
    );
    assert_eq!(
        style_resolver::parse_key_text("from, 50%"),
        Some(vec![0.0, 0.5])
    );
    assert_eq!(style_resolver::parse_key_text(" 1e1% "), Some(vec![0.1]));
    for invalid in ["", "invalid", "150%", "-5%", "10% 20%", "10%,"] {
        assert_eq!(style_resolver::parse_key_text(invalid), None, "{invalid:?}");
    }
}

#[test]
fn a_keyframe_drops_important_and_animation_declarations() {
    assert_eq!(
        keyframes(
            "@keyframes foo { from { margin-top: 10px; animation-timing-function: ease; \
             animation-name: none; animation-duration: 1s; animation-iteration-count: 1; \
             animation-direction: normal; animation-play-state: running; \
             animation-delay: 0s; animation-fill-mode: none; animation: bar 1s infinite; \
             animation-timeline: auto; animation-range: normal; \
             color: red !important; width: 5px } }"
        ),
        [(
            "0%".to_owned(),
            "margin-top: 10px; animation-timing-function: ease; width: 5px;".to_owned()
        )]
    );
    // The shorthand is dropped whole, its `animation-timing-function` too.
    assert_eq!(
        keyframes(
            "@keyframes foo { to { animation-timing-function: linear; \
             animation: bar 1s steps(2) } }"
        ),
        [(
            "100%".to_owned(),
            "animation-timing-function: linear;".to_owned()
        )]
    );
    // An `!important` one is dropped, not taken over a normal one.
    assert_eq!(
        keyframes("@keyframes foo { to { color: red; color: blue !important } }"),
        [("100%".to_owned(), "color: red;".to_owned())]
    );
    // A style rule's are as ever.
    assert_eq!(
        css_texts(".a { animation-name: x; color: red !important }"),
        [".a { animation-name: x; color: red !important; }"]
    );
}

#[test]
fn keyframes_are_in_group_rules_and_insert_rule_takes_them() {
    // The group rule indents only its own lines, as Blink's
    // `AppendCSSTextForItems` does.
    assert_eq!(
        css_texts("@media screen { @keyframes a { to { top: 1px } } }"),
        ["@media screen {\n  @keyframes a { \n  100% { top: 1px; }\n}\n}"]
    );
    let contents = StyleSheetContents::parse(".a {}", None);
    let contents = contents
        .insert_rule(
            "@keyframes cm-blink { 0% {} 50% { opacity: 0 } 100% {} }",
            1,
        )
        .expect("a @keyframes is a rule");
    assert_eq!(
        contents
            .rules()
            .iter()
            .map(CssRule::css_text)
            .collect::<Vec<_>>(),
        [
            ".a { }",
            "@keyframes cm-blink { \n  0% { }\n  50% { opacity: 0; }\n  100% { }\n}"
        ]
    );
    // A keyframe alone is no rule of a sheet.
    assert!(contents.insert_rule("50% { opacity: 0 }", 0).is_err());
}
