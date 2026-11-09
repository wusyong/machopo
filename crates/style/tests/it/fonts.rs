//! The font longhands beside the ones that pick and size the font, and the
//! `font`, `font-variant` and `font-synthesis` shorthands: what they read
//! back as declared and computed (Chrome's, as WPT's
//! `css/css-fonts/parsing/` checks them), what inherits, and what the
//! `font` shorthand resets.

use std::sync::Arc;

use style_resolver::CssPropertyValueSet;

use crate::transitions::{assert_computed, assert_invalid, assert_read_back, style_of};

fn css_text(css: &str) -> String {
    CssPropertyValueSet::parse(css).css_text()
}

/// `font-stretch`: a keyword stays one as written, and computes to its
/// percentage.
#[test]
fn font_stretch() {
    assert_read_back(
        "font-stretch",
        &[
            ("condensed", "condensed"),
            ("NORMAL", "normal"),
            ("75%", "75%"),
            ("0%", "0%"),
            ("calc(50% + 25%)", "75%"),
        ],
    );
    assert_invalid("font-stretch", &["-1%", "1px", "50", "wider"]);
    assert_computed(
        "font-stretch",
        &[
            ("condensed", "75%"),
            ("semi-expanded", "112.5%"),
            ("normal", "100%"),
            ("200%", "200%"),
        ],
    );
}

/// `font-feature-settings` as written, `1` left out; computed one per tag
/// (the last) sorted by tag. `font-variation-settings` likewise.
#[test]
fn feature_and_variation_settings() {
    assert_read_back(
        "font-feature-settings",
        &[
            ("'liga' 0", "\"liga\" 0"),
            ("\"liga\" 1", "\"liga\""),
            ("\"liga\" on", "\"liga\""),
            ("\"liga\" off", "\"liga\" 0"),
            ("\"liga\" 0, \"liga\" 1", "\"liga\" 0, \"liga\""),
            ("normal", "normal"),
        ],
    );
    assert_invalid(
        "font-feature-settings",
        &[
            "\"lig\"",
            "\"silly\" off",
            "dlig",
            "\"liga\" -1",
            "normal \"dlig\"",
            "\"tnum\" \"hist\"",
        ],
    );
    assert_computed(
        "font-feature-settings",
        &[
            ("\"tnum\", \"hist\"", "\"hist\", \"tnum\""),
            ("\"dlig\", \"smcp\", \"dlig\" 0", "\"dlig\" 0, \"smcp\""),
        ],
    );
    assert_read_back(
        "font-variation-settings",
        &[
            ("\"wght\" 1e3", "\"wght\" 1000"),
            ("'wght' 400, \"wdth\" 50", "\"wght\" 400, \"wdth\" 50"),
        ],
    );
    assert_invalid(
        "font-variation-settings",
        &[
            "700",
            "\"XHGT\"",
            "\"wgt\" 700",
            "'wght' 100px",
            "'wght' 42%",
        ],
    );
    assert_computed(
        "font-variation-settings",
        &[
            (
                "\"wght\" 100, \"wdth\" 200, \"wght\" 300, \"WDTH\" 400",
                "\"WDTH\" 400, \"wdth\" 200, \"wght\" 300",
            ),
            ("normal", "normal"),
        ],
    );
}

/// The rest: `font-kerning`, `font-optical-sizing`, `font-size-adjust`
/// (`ex-height` left out), `font-language-override` (one to four
/// characters, the trailing spaces dropped), the `font-synthesis-*`
/// longhands and their shorthand.
#[test]
fn the_other_font_longhands() {
    assert_read_back("font-kerning", &[("none", "none"), ("Normal", "normal")]);
    assert_read_back("font-optical-sizing", &[("none", "none"), ("auto", "auto")]);
    assert_read_back(
        "font-size-adjust",
        &[
            ("0.5", "0.5"),
            ("ex-height 0.5", "0.5"),
            ("cap-height from-font", "cap-height from-font"),
            ("from-font", "from-font"),
            ("none", "none"),
        ],
    );
    assert_invalid(
        "font-size-adjust",
        &["-1", "auto", "cap-height", "0.5 cap-height"],
    );
    assert_read_back(
        "font-language-override",
        &[
            ("\"TRK\"", "\"TRK\""),
            ("\"ENG \"", "\"ENG\""),
            ("\" en \"", "\" en\""),
            ("normal", "normal"),
        ],
    );
    assert_invalid(
        "font-language-override",
        &["auto", "\"turkish\"", "\"\"", "\"xøx\"", "\"ENG  \""],
    );
    assert_read_back(
        "font-synthesis",
        &[
            ("none", "none"),
            ("style weight", "weight style"),
            ("small-caps style", "style small-caps"),
            ("weight style small-caps", "weight style small-caps"),
        ],
    );
    assert_invalid("font-synthesis", &["weight weight", "none weight", "auto"]);
    assert_read_back(
        "font-synthesis-weight",
        &[("none", "none"), ("auto", "auto")],
    );
    assert_eq!(
        css_text(
            "font-synthesis-weight: none; font-synthesis-style: none; font-synthesis-small-caps: none"
        ),
        "font-synthesis: none;"
    );
    assert_eq!(
        css_text("font-synthesis-weight: none"),
        "font-synthesis-weight: none;"
    );
    assert_computed("font-synthesis", &[("weight", "weight")]);
}

/// `font` takes `small-caps` and the stretch keywords, and resets the
/// other font longhands but the `font-synthesis-*` ones: written back only
/// while they are as it resets them (Chrome's).
#[test]
fn the_font_shorthand_resets_the_font_longhands() {
    assert_read_back(
        "font",
        &[
            ("12px serif", "12px serif"),
            ("small-caps 12px serif", "small-caps 12px serif"),
            ("condensed 12px serif", "condensed 12px serif"),
            (
                "italic small-caps bold condensed 12px/2 serif",
                "italic small-caps bold condensed 12px / 2 serif",
            ),
            ("caption", "caption"),
        ],
    );
    assert_invalid("font", &["all-small-caps 12px serif", "75% 12px serif"]);
    assert_eq!(
        css_text("font: 12px serif; font-synthesis: none"),
        "font: 12px serif; font-synthesis: none;"
    );
    assert_eq!(
        css_text("font: 12px serif; font-variant-ligatures: none"),
        "font-style: normal; font-variant: none; font-size-adjust: none; \
         font-language-override: normal; font-kerning: auto; font-optical-sizing: auto; \
         font-feature-settings: normal; font-variation-settings: normal; font-weight: normal; \
         font-stretch: normal; font-size: 12px; line-height: normal; font-family: serif;"
    );
    for (css, longhand) in [
        ("font-variant-caps: all-small-caps", "font-variant"),
        ("font-stretch: 80%", "font-stretch"),
        ("font-kerning: none", "font-kerning"),
        ("font-feature-settings: \"liga\" 0", "font-feature-settings"),
        ("font-size-adjust: 0.5", "font-size-adjust"),
        ("font-optical-sizing: none", "font-optical-sizing"),
    ] {
        let block = CssPropertyValueSet::parse(&format!("font: 12px serif; {css}"));
        assert_eq!(block.property_value("font"), "", "{css}");
        assert!(block.css_text().contains(longhand), "{css}");
    }
    // A percentage that is a keyword is written as it.
    assert_eq!(
        CssPropertyValueSet::parse("font: 12px serif; font-stretch: 75%").property_value("font"),
        "condensed 12px serif"
    );
    // `font` resets what was set before it.
    let style = style_of(
        "<style>#t { font-kerning: none; font-variant-numeric: tabular-nums; \
         font-synthesis: none; font: 12px serif }</style><div id=t></div>",
    );
    assert_eq!(style.font.kerning, computed_style::FontKerning::Auto);
    assert_eq!(style.font.variant_numeric, Default::default());
    assert!(!style.font.synthesis_weight, "not reset");
    assert_computed(
        "font",
        &[
            (
                "italic small-caps bold condensed 12px serif",
                "italic small-caps 700 condensed 12px serif",
            ),
            ("400 12px serif", "12px serif"),
        ],
    );
}

/// The font longhands inherit, and set alike keep the font group shared.
#[test]
fn the_font_longhands_inherit() {
    let style = style_of(
        "<style>div { font-variant-caps: small-caps; font-feature-settings: \"liga\" 0; \
         font-stretch: condensed; font-kerning: none; font-variation-settings: \"wght\" 300; \
         font-synthesis: none; font-optical-sizing: none } </style>\
         <div><span id=t></span></div>",
    );
    let font = &style.font;
    assert_eq!(
        font.variant_caps,
        computed_style::FontVariantCaps::SmallCaps
    );
    assert_eq!(font.stretch, 75.0);
    assert_eq!(font.kerning, computed_style::FontKerning::None);
    assert_eq!(
        font.feature_settings,
        Arc::from([computed_style::FontFeatureSetting {
            tag: *b"liga",
            value: 0
        }])
    );
    assert_eq!(font.variation_settings.len(), 1);
    assert!(!font.synthesis_weight && !font.synthesis_style && !font.synthesis_small_caps);
    assert!(!font.optical_sizing);
}
