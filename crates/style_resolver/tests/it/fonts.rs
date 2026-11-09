//! The `font-variant` longhands and shorthand as declared: what they read
//! back as (Chrome's, as WPT's `css/css-fonts/parsing/` checks them).

use style_resolver::CssPropertyValueSet;

use crate::transitions::{assert_invalid, assert_read_back};

fn css_text(css: &str) -> String {
    CssPropertyValueSet::parse(css).css_text()
}

/// The `font-variant-*` longhands: their parts in css-fonts 4's order, one
/// of each kind.
#[test]
fn font_variants() {
    assert_read_back(
        "font-variant-ligatures",
        &[
            ("none", "none"),
            ("normal", "normal"),
            ("no-common-ligatures", "no-common-ligatures"),
            (
                "contextual historical-ligatures no-discretionary-ligatures common-ligatures",
                "common-ligatures no-discretionary-ligatures historical-ligatures contextual",
            ),
        ],
    );
    assert_invalid(
        "font-variant-ligatures",
        &[
            "none common-ligatures",
            "common-ligatures no-common-ligatures",
            "normal contextual",
        ],
    );
    assert_read_back(
        "font-variant-numeric",
        &[
            (
                "slashed-zero ordinal tabular-nums oldstyle-nums diagonal-fractions",
                "oldstyle-nums tabular-nums diagonal-fractions ordinal slashed-zero",
            ),
            ("lining-nums", "lining-nums"),
        ],
    );
    assert_invalid(
        "font-variant-numeric",
        &["lining-nums oldstyle-nums", "ordinal ordinal", "none"],
    );
    assert_read_back(
        "font-variant-east-asian",
        &[
            ("ruby full-width jis78", "jis78 full-width ruby"),
            (
                "proportional-width simplified",
                "simplified proportional-width",
            ),
        ],
    );
    assert_invalid("font-variant-east-asian", &["jis78 jis83", "ruby ruby"]);
    assert_read_back(
        "font-variant-caps",
        &[
            ("SMALL-CAPS", "small-caps"),
            ("titling-caps", "titling-caps"),
        ],
    );
    assert_invalid("font-variant-caps", &["small-caps all-small-caps"]);
    assert_read_back(
        "font-variant-alternates",
        &[
            ("historical-forms", "historical-forms"),
            (
                "styleset(a, b) historical-forms stylistic(foo) annotation(f) swash(d)",
                "stylistic(foo) historical-forms styleset(a, b) swash(d) annotation(f)",
            ),
        ],
    );
    assert_invalid(
        "font-variant-alternates",
        &[
            "swash(one) swash(two)",
            "stylistic(a, b)",
            "annotation()",
            "swash",
        ],
    );
    assert_read_back(
        "font-variant-position",
        &[("super", "super"), ("sub", "sub")],
    );
    assert_read_back(
        "font-variant-emoji",
        &[("emoji", "emoji"), ("unicode", "unicode")],
    );
}

/// `font-variant`: `normal` resets its seven longhands, `none` sets the
/// ligatures' alone; written back in css-fonts 4's order, `none` only with
/// the others normal.
#[test]
fn font_variant() {
    assert_read_back(
        "font-variant",
        &[
            ("small-caps", "small-caps"),
            ("none", "none"),
            ("normal", "normal"),
            (
                "small-caps slashed-zero common-ligatures",
                "common-ligatures small-caps slashed-zero",
            ),
            ("all-small-caps sub emoji", "all-small-caps sub emoji"),
            ("historical-forms jis78", "historical-forms jis78"),
        ],
    );
    assert_invalid(
        "font-variant",
        &["none small-caps", "small-caps small-caps", "normal sub"],
    );
    assert_eq!(
        css_text("font-variant: small-caps; font-variant-ligatures: none"),
        "font-variant-numeric: normal; font-variant-east-asian: normal; \
         font-variant-caps: small-caps; font-variant-alternates: normal; \
         font-variant-position: normal; font-variant-emoji: normal; \
         font-variant-ligatures: none;"
    );
    assert_eq!(
        css_text("font-variant: normal; font-variant-numeric: tabular-nums"),
        "font-variant: tabular-nums;"
    );
}
