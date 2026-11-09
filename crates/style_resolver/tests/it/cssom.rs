//! Declaration blocks as CSSOM sees them: what each value reads back as,
//! shorthands made back from their longhands, `cssText`, and changing a
//! block. The expected text is Chrome's, as WPT's `css/*/parsing/` and
//! `css/cssom/` tests check it.

use style_resolver::{CssPropertyValueSet, css_property_attributes, css_property_for_attribute};

/// What `property: value` reads back as on its own, or `None` if not set.
fn read_back(property: &str, value: &str) -> Option<String> {
    let mut block = CssPropertyValueSet::default();
    block.set_property(property, value, "");
    let text = block.property_value(property);
    (!text.is_empty()).then_some(text)
}

fn css_text(css: &str) -> String {
    CssPropertyValueSet::parse(css).css_text()
}

#[track_caller]
fn assert_values(cases: &[(&str, &str, &str)]) {
    for &(property, value, expected) in cases {
        assert_eq!(
            read_back(property, value).as_deref(),
            Some(expected),
            "{property}: {value}"
        );
    }
}

/// `caret-color` as Chrome 152 writes it back: `auto`, `currentcolor` and
/// `transparent` as keywords, a color as colors are; `none` is no value.
#[test]
fn caret_color() {
    assert_values(&[
        ("caret-color", "AUTO", "auto"),
        ("caret-color", "currentColor", "currentcolor"),
        ("caret-color", "transparent", "transparent"),
        ("caret-color", "red", "red"),
        ("caret-color", "rgb(1, 2, 3)", "rgb(1, 2, 3)"),
    ]);
    assert_eq!(read_back("caret-color", "none"), None);
    assert_eq!(read_back("caret-color", "auto red"), None);
}

/// `overflow-anchor` as Chrome 152 writes it back: `auto` and `none`,
/// in lowercase; nothing else.
#[test]
fn overflow_anchor() {
    assert_values(&[
        ("overflow-anchor", "auto", "auto"),
        ("overflow-anchor", "NONE", "none"),
    ]);
    assert_eq!(read_back("overflow-anchor", "scroll"), None);
    assert_eq!(read_back("overflow-anchor", "auto none"), None);
}

/// `cursor` as Chrome writes it back: a `-webkit-` alias as written, the
/// hot spot's numbers whole (toward 0, within an `i32`), math functions
/// done.
#[test]
fn cursors() {
    assert_values(&[
        ("cursor", "POINTER", "pointer"),
        ("cursor", "nwse-resize", "nwse-resize"),
        ("cursor", "-webkit-zoom-in", "-webkit-zoom-in"),
        ("cursor", "-WEBKIT-GRAB", "-webkit-grab"),
        ("cursor", "url(a.png), pointer", "url(\"a.png\"), pointer"),
        (
            "cursor",
            "url(a.png) 2.5 -3.7, auto",
            "url(\"a.png\") 2 -3, auto",
        ),
        (
            "cursor",
            "url('a b.cur') calc(1 + 2) 1e1, url(c.png), move",
            "url(\"a b.cur\") 3 10, url(\"c.png\"), move",
        ),
        (
            "cursor",
            "url(a.png) 99999999999 0, none",
            "url(\"a.png\") 2147483647 0, none",
        ),
    ]);
}

/// `image-set()` as Chrome 152 writes it back: `-webkit-image-set()` and
/// any case as `image-set()`, a string as `url("…")`, an omitted
/// resolution as `1x`, the unit as written (a math function as
/// `calc(<n>dppx)`, a negative one too), `type()` after it; in each
/// property that takes an `<image>`, and in `cursor` with URLs only.
#[test]
fn image_sets() {
    assert_values(&[
        (
            "background-image",
            "image-set(url(a.png) 1x, url(b.png) 2x)",
            r#"image-set(url("a.png") 1x, url("b.png") 2x)"#,
        ),
        (
            "background-image",
            "-webkit-image-set(url(a.png) 1x, url(b.png) 2x)",
            r#"image-set(url("a.png") 1x, url("b.png") 2x)"#,
        ),
        (
            "background-image",
            r#"IMAGE-SET("a.png" 1X, "b.png")"#,
            r#"image-set(url("a.png") 1x, url("b.png") 1x)"#,
        ),
        (
            "background-image",
            "image-set(url(a.png) 1dppx, url(b.png) 192dpi, url(c.png) 1dpcm)",
            r#"image-set(url("a.png") 1dppx, url("b.png") 192dpi, url("c.png") 1dpcm)"#,
        ),
        (
            "background-image",
            r#"image-set(url(a.png) type("image/png") 1.5x, url(b.png) 0x type("IMAGE/x"))"#,
            r#"image-set(url("a.png") 1.5x type("image/png"), url("b.png") 0x type("IMAGE/x"))"#,
        ),
        (
            "background-image",
            "image-set(url(a.png) 1x, linear-gradient(red, blue) 2x), url(c.png)",
            r#"image-set(url("a.png") 1x, linear-gradient(red, blue) 2x), url("c.png")"#,
        ),
        (
            "background-image",
            "image-set(url(a.png) calc(1x + 1x), url(b.png) min(1x, 2dppx))",
            r#"image-set(url("a.png") calc(2dppx), url("b.png") calc(1dppx))"#,
        ),
        (
            "background-image",
            "image-set(url(a.png) calc(-1x), url(b.png) calc(96dpi))",
            r#"image-set(url("a.png") calc(-1dppx), url("b.png") calc(1dppx))"#,
        ),
        (
            "background-image",
            "image-set(url(a.png) 1e1x, url(b.png) 0.000001x)",
            r#"image-set(url("a.png") 10x, url("b.png") 1e-06x)"#,
        ),
        (
            "background",
            "image-set(url(a.png) 1x, url(b.png) 2x) no-repeat, url(c.png)",
            r#"image-set(url("a.png") 1x, url("b.png") 2x) no-repeat, url("c.png")"#,
        ),
        (
            "border-image-source",
            "image-set(url(a.png) 1x, url(b.png) 2x)",
            r#"image-set(url("a.png") 1x, url("b.png") 2x)"#,
        ),
        (
            "cursor",
            r#"-webkit-image-set("a.png" 2x type("image/png")) 3 4, pointer"#,
            r#"image-set(url("a.png") 2x type("image/png")) 3 4, pointer"#,
        ),
    ]);
    for value in [
        "image-set()",
        "image-set(url(a.png) 1x,)",
        "image-set(url(a.png) -1x)",
        "image-set(url(a.png) 1x 2x)",
        r#"image-set(url(a.png) type("a") type("b"))"#,
        r#"image-set(url(a.png) 1x type("image/png") 2x)"#,
        "image-set(url(a.png) type(a))",
        "image-set(url(a.png) type())",
        "image-set(none 1x)",
        "image-set(image-set(url(a.png) 1x) 1x)",
        "image-set(url(a.png) 1px)",
        "image-set(url(a.png) calc(1x + 1px))",
    ] {
        assert_eq!(read_back("background-image", value), None, "{value}");
    }
    // A resolution adds to nothing else in a math function.
    assert_eq!(read_back("width", "calc(1px + 1x)"), None);
}

/// The `text-decoration` longhands, `text-underline-*`, `text-shadow` and
/// `-webkit-text-fill-color`; `text-decoration` in any order, written back
/// with only what isn't initial, `none` if nothing.
#[test]
fn text_decorations() {
    assert_values(&[
        ("text-decoration-line", "BLINK underline", "underline blink"),
        ("text-decoration-style", "WAVY", "wavy"),
        ("text-decoration-color", "currentColor", "currentcolor"),
        ("text-decoration-thickness", "FROM-FONT", "from-font"),
        ("text-decoration-thickness", "-1em", "-1em"),
        ("text-decoration-thickness", "10%", "10%"),
        ("text-underline-offset", "AUTO", "auto"),
        ("text-underline-offset", "-2px", "-2px"),
        ("text-underline-position", "right under", "under right"),
        ("text-underline-position", "from-font", "from-font"),
        ("text-underline-position", "left", "left"),
        ("text-shadow", "NONE", "none"),
        (
            "text-shadow",
            "1px 2px red, blue 0 0 3px",
            "red 1px 2px, blue 0px 0px 3px",
        ),
        ("-webkit-text-fill-color", "transparent", "transparent"),
        ("text-decoration", "solid", "none"),
        ("text-decoration", "currentcolor", "none"),
        ("text-decoration", "auto", "none"),
        (
            "text-decoration",
            "double overline underline",
            "underline overline double",
        ),
        (
            "text-decoration",
            "rgba(10, 20, 30, 0.4) dotted",
            "dotted rgba(10, 20, 30, 0.4)",
        ),
        (
            "text-decoration",
            "overline green from-font",
            "overline from-font green",
        ),
        ("text-decoration", "underline 3em", "underline 3em"),
    ]);
    for (property, value) in [
        ("text-decoration-line", "underline underline"),
        ("text-decoration-line", "none underline"),
        ("text-decoration-style", "none"),
        ("text-underline-position", "under from-font"),
        ("text-underline-position", "left right"),
        ("text-underline-position", "auto under"),
        ("text-shadow", "1px 2px 3px 4px"),
        ("text-shadow", "inset 1px 2px"),
        ("text-shadow", "1px 2px -3px"),
        ("text-decoration", "solid dotted"),
        ("text-decoration", "red blue"),
    ] {
        assert_eq!(read_back(property, value), None, "{property}: {value}");
    }
    // The longhands a shorthand set read back as it.
    assert_eq!(
        css_text("text-decoration: underline wavy"),
        "text-decoration: underline wavy;"
    );
}

/// `filter`'s functions as written (an argument left out stays out, a
/// grayscale, invert, opacity or sepia past 100% written as 100%), and
/// `isolation`.
#[test]
fn filters() {
    assert_values(&[
        ("filter", "NONE", "none"),
        ("filter", "blur(0)", "blur(0px)"),
        ("filter", "blur()", "blur()"),
        (
            "filter",
            "BRIGHTNESS(300%) contrast(0.5)",
            "brightness(300%) contrast(0.5)",
        ),
        (
            "filter",
            "grayscale(300%) invert(2) opacity(150%) sepia(1.5)",
            "grayscale(100%) invert(1) opacity(100%) sepia(1)",
        ),
        (
            "filter",
            "saturate(2) hue-rotate(0)",
            "saturate(2) hue-rotate(0deg)",
        ),
        ("filter", "hue-rotate(0.5turn)", "hue-rotate(0.5turn)"),
        (
            "filter",
            "drop-shadow(1px 2px rgb(4, 5, 6))",
            "drop-shadow(rgb(4, 5, 6) 1px 2px)",
        ),
        ("filter", "drop-shadow(0 0 3px)", "drop-shadow(0px 0px 3px)"),
        ("isolation", "ISOLATE", "isolate"),
    ]);
    for value in [
        "auto",
        "none blur(1px)",
        "blur(10)",
        "blur(-1px)",
        "brightness(-20)",
        "contrast(30px)",
        "hue-rotate(90)",
        "drop-shadow(1px)",
        "drop-shadow(1px 2px 3px 4px)",
        "drop-shadow(inset 1px 2px)",
        "url(#f)",
    ] {
        assert_eq!(read_back("filter", value), None, "filter: {value}");
    }
    assert_eq!(read_back("isolation", "none"), None);
}

/// Chrome's (152) keywords, by either name: the alias reads back as
/// `appearance`; its old keywords don't parse.
#[test]
fn appearance_and_its_alias() {
    assert_values(&[
        ("appearance", "AUTO", "auto"),
        ("appearance", "none", "none"),
        ("appearance", "menulist-button", "menulist-button"),
        ("appearance", "progress-bar", "progress-bar"),
        ("appearance", "base-select", "base-select"),
        ("-webkit-appearance", "textfield", "textfield"),
    ]);
    for value in [
        "push-button",
        "square-button",
        "inner-spin-button",
        "caret",
        "auto none",
    ] {
        assert_eq!(read_back("appearance", value), None, "{value}");
    }
    assert_eq!(
        css_text("-webkit-appearance: checkbox"),
        "appearance: checkbox;"
    );
}

/// `-webkit-text-security`'s keywords; nothing else parses.
#[test]
fn text_security() {
    assert_values(&[
        ("-webkit-text-security", "DISC", "disc"),
        ("-webkit-text-security", "circle", "circle"),
        ("-webkit-text-security", "square", "square"),
        ("-webkit-text-security", "none", "none"),
    ]);
    for value in ["auto", "disc circle", "1px"] {
        assert_eq!(read_back("-webkit-text-security", value), None, "{value}");
    }
}

/// `border-image`'s longhands, as few sides as `margin`'s; the shorthand
/// as Chrome writes it: the source alone while the rest is initial, else
/// all five.
#[test]
fn border_images() {
    assert_values(&[
        ("border-image-source", "NONE", "none"),
        ("border-image-source", "url(a.png)", "url(\"a.png\")"),
        (
            "border-image-source",
            "linear-gradient(red, blue)",
            "linear-gradient(red, blue)",
        ),
        ("border-image-slice", "1 2 1 2", "1 2"),
        ("border-image-slice", "10% FILL", "10% fill"),
        ("border-image-slice", "fill 10", "10 fill"),
        ("border-image-slice", "0 1 2 3", "0 1 2 3"),
        ("border-image-width", "1px 2px 1px 2px", "1px 2px"),
        ("border-image-width", "auto 0 10%", "auto 0 10%"),
        ("border-image-outset", "0px", "0px"),
        ("border-image-outset", "1 2px", "1 2px"),
        ("border-image-repeat", "round round", "round"),
        ("border-image-repeat", "repeat SPACE", "repeat space"),
        ("border-image", "none", "none"),
        ("border-image", "url(a.png)", "url(\"a.png\")"),
        (
            "border-image",
            "url(a.png) 30",
            "url(\"a.png\") 30 / 1 / 0 stretch",
        ),
        ("border-image", "30", "none 30 / 1 / 0 stretch"),
        ("border-image", "round", "none 100% / 1 / 0 round"),
        (
            "border-image",
            "url(a.png) 30 / 2",
            "url(\"a.png\") 30 / 2 / 0 stretch",
        ),
        (
            "border-image",
            "url(a.png) 30 / / 3",
            "url(\"a.png\") 30 / 1 / 3 stretch",
        ),
        (
            "border-image",
            "url(a.png) fill 30 repeat space",
            "url(\"a.png\") 30 fill / 1 / 0 repeat space",
        ),
        (
            "border-image",
            "10% fill / auto / 2px",
            "none 10% fill / auto / 2px stretch",
        ),
        (
            "border-image",
            "round url(a.png)",
            "url(\"a.png\") 100% / 1 / 0 round",
        ),
    ]);
    for (property, value) in [
        ("border-image-source", "red"),
        ("border-image-slice", "-1"),
        ("border-image-slice", "-10%"),
        ("border-image-slice", "10 fill 20"),
        ("border-image-slice", "fill"),
        ("border-image-slice", "1px"),
        ("border-image-width", "-1px"),
        ("border-image-width", "1 2 3 4 5"),
        ("border-image-outset", "-1"),
        ("border-image-outset", "10%"),
        ("border-image-repeat", "stretch stretch stretch"),
        ("border-image", "url(a.png) / 2"),
        ("border-image", "url(a.png) 30 /"),
        ("border-image", "url(a.png) 30 / 1 /"),
        ("border-image", "round round round"),
        ("border-image", "none none"),
    ] {
        assert_eq!(read_back(property, value), None, "{property}: {value}");
    }
    assert_eq!(
        css_text("border-image: ; color: red"),
        "color: red;",
        "nothing"
    );
    // The longhands set alike as their initial values: the source alone.
    assert_eq!(
        css_text(
            "border-image-source: url(a.png); border-image-slice: 100%; border-image-width: 1; \
             border-image-outset: 0; border-image-repeat: stretch"
        ),
        "border-image: url(\"a.png\");"
    );
    assert_eq!(
        css_text("border-image: url(a.png) 30 round; border-image-width: initial"),
        "border-image-source: url(\"a.png\"); border-image-slice: 30; border-image-outset: 0; \
         border-image-repeat: round; border-image-width: initial;"
    );
}

/// The properties machopo parses but doesn't act on yet (the user's selection
/// and editing, a resizer, smooth scrolling, a mobile browser's): Chrome's
/// values, in lowercase; its `-webkit-` aliases write the property's name.
#[test]
fn the_properties_parsed_only() {
    assert_values(&[
        ("user-select", "NONE", "none"),
        ("user-select", "all", "all"),
        ("user-select", "text", "text"),
        ("user-select", "auto", "auto"),
        ("-webkit-user-select", "none", "none"),
        (
            "-webkit-user-modify",
            "read-write-plaintext-only",
            "read-write-plaintext-only",
        ),
        ("-webkit-user-modify", "READ-WRITE", "read-write"),
        ("-webkit-user-modify", "read-only", "read-only"),
        ("hyphens", "auto", "auto"),
        ("hyphens", "manual", "manual"),
        ("hyphens", "none", "none"),
        (
            "forced-color-adjust",
            "preserve-parent-color",
            "preserve-parent-color",
        ),
        ("forced-color-adjust", "none", "none"),
        (
            "-webkit-tap-highlight-color",
            "rgba(0, 0, 0, 0)",
            "rgba(0, 0, 0, 0)",
        ),
        ("-webkit-tap-highlight-color", "transparent", "transparent"),
        ("text-size-adjust", "none", "none"),
        ("text-size-adjust", "AUTO", "auto"),
        ("text-size-adjust", "80%", "80%"),
        ("text-size-adjust", "calc(50% + 10%)", "60%"),
        ("-webkit-text-size-adjust", "none", "none"),
        ("text-rendering", "optimizeLegibility", "optimizelegibility"),
        ("text-rendering", "optimizeSpeed", "optimizespeed"),
        ("text-rendering", "geometricPrecision", "geometricprecision"),
        ("-webkit-font-smoothing", "antialiased", "antialiased"),
        (
            "-webkit-font-smoothing",
            "subpixel-antialiased",
            "subpixel-antialiased",
        ),
        ("-webkit-font-smoothing", "none", "none"),
        ("resize", "auto", "auto"),
        ("resize", "BOTH", "both"),
        ("resize", "block", "block"),
        ("resize", "inline", "inline"),
        ("scroll-behavior", "smooth", "smooth"),
    ]);
    for (property, value) in [
        ("user-select", "contain"),
        ("user-select", "element"),
        ("-webkit-user-modify", "auto"),
        ("hyphens", "all"),
        ("forced-color-adjust", "preserve"),
        ("text-size-adjust", "-1%"),
        ("text-size-adjust", "1px"),
        ("text-size-adjust", "1"),
        ("text-rendering", "optimize-speed"),
        ("-webkit-font-smoothing", "grayscale"),
        ("resize", "none both"),
        ("scroll-behavior", "instant"),
    ] {
        assert_eq!(read_back(property, value), None, "{property}: {value}");
    }
    assert_eq!(
        css_text("-webkit-user-select: none; -webkit-text-size-adjust: 50%"),
        "user-select: none; text-size-adjust: 50%;"
    );
}

/// A system font: the shorthand reads back as the keyword, its longhands
/// as nothing (pending till they compute, as Chrome's); another longhand
/// set over one leaves no shorthand.
#[test]
fn system_fonts() {
    for keyword in [
        "caption",
        "icon",
        "menu",
        "message-box",
        "small-caption",
        "status-bar",
        "-webkit-small-control",
        "-webkit-mini-control",
        "-webkit-control",
    ] {
        assert_eq!(
            read_back("font", &keyword.to_uppercase()).as_deref(),
            Some(keyword)
        );
        assert_eq!(
            css_text(&format!("font: {keyword}")),
            format!("font: {keyword};")
        );
    }
    for longhand in [
        "font-family",
        "font-size",
        "font-weight",
        "font-style",
        "line-height",
    ] {
        let mut block = CssPropertyValueSet::default();
        block.set_property("font", "menu", "");
        assert_eq!(block.property_value(longhand), "", "{longhand}");
    }
    let mut block = CssPropertyValueSet::default();
    block.set_property("font", "menu", "");
    block.set_property("font-style", "italic", "");
    assert_eq!(block.property_value("font"), "");
    for value in ["caption bold", "bold caption", "menu, serif", "menu 12px"] {
        assert_eq!(read_back("font", value), None, "{value}");
    }
    // Only the shorthand takes one.
    assert_eq!(
        read_back("font-family", "caption").as_deref(),
        Some("caption")
    );
}

#[test]
fn system_colors_keep_their_keywords() {
    assert_values(&[
        ("color", "ButtonText", "buttontext"),
        ("background-color", "CANVAS", "canvas"),
        ("border-top-color", "-webkit-link", "-webkit-link"),
        ("outline-color", "ThreeDDarkShadow", "threeddarkshadow"),
        ("box-shadow", "1px 1px Highlight", "highlight 1px 1px"),
    ]);
}

#[test]
fn lengths_keep_their_units() {
    assert_values(&[
        ("width", "10px", "10px"),
        ("width", "1in", "1in"),
        ("width", "2.54CM", "2.54cm"),
        ("width", "10mm", "10mm"),
        ("width", "4Q", "4q"),
        ("width", "12pt", "12pt"),
        ("width", "1pc", "1pc"),
        ("width", "2em", "2em"),
        ("width", "1.5rem", "1.5rem"),
        ("width", "50%", "50%"),
        ("top", "1.5em", "1.5em"),
        ("left", "-3%", "-3%"),
        // A unitless zero is a length.
        ("width", "0", "0px"),
        ("margin-left", "-3px", "-3px"),
        ("width", "auto", "auto"),
        ("width", "min-content", "min-content"),
        ("width", "max-content", "max-content"),
        ("width", "fit-content", "fit-content"),
        ("flex-basis", "content", "content"),
        ("min-width", "auto", "auto"),
        ("max-width", "none", "none"),
        ("max-height", "3px", "3px"),
        ("padding-top", "4%", "4%"),
        ("border-top-width", "thin", "thin"),
        ("border-top-width", "MEDIUM", "medium"),
        ("border-top-width", "thick", "thick"),
        ("border-top-width", "1in", "1in"),
    ]);
}

#[test]
fn numbers_read_back_with_six_significant_digits() {
    assert_values(&[
        ("width", "0.5px", "0.5px"),
        ("width", "1.23456789px", "1.23457px"),
        ("width", "100000px", "100000px"),
        ("width", "1234567px", "1.23457e+06px"),
        ("width", "1000000px", "1e+06px"),
        ("width", "0.0001px", "0.0001px"),
        ("width", "0.00001234px", "1.234e-05px"),
        ("width", ".0px", "0px"),
        ("flex-grow", "23.4e5", "2.34e+06"),
        ("flex-grow", "+.678E9", "6.78e+08"),
        ("flex-grow", "0.1", "0.1"),
        ("line-height", "1.5", "1.5"),
        ("opacity", "0.25", "0.25"),
    ]);
}

#[test]
fn colors_keep_names_and_write_the_rest_as_rgb() {
    assert_values(&[
        ("color", "red", "red"),
        ("color", "RED", "red"),
        ("color", "Magenta", "magenta"),
        ("color", "transparent", "transparent"),
        ("color", "currentColor", "currentcolor"),
        ("color", "#234", "rgb(34, 51, 68)"),
        ("color", "#FEDCBA", "rgb(254, 220, 186)"),
        ("color", "rgb(2, 3, 4)", "rgb(2, 3, 4)"),
        ("color", "rgb(100%, 0%, 0%)", "rgb(255, 0, 0)"),
        ("color", "rgb(-2, 3, 4)", "rgb(0, 3, 4)"),
        ("color", "rgb(100, 200, 300)", "rgb(100, 200, 255)"),
        ("color", "rgb(2 3 4 / 50%)", "rgba(2, 3, 4, 0.5)"),
        ("color", "rgba(2, 3, 4, 0.5)", "rgba(2, 3, 4, 0.5)"),
        ("color", "rgba(2, 3, 4, 0)", "rgba(2, 3, 4, 0)"),
        // The fewest decimals, two or three, that give back the same
        // 8-bit alpha.
        ("color", "rgba(2, 3, 4, 0.2)", "rgba(2, 3, 4, 0.2)"),
        ("color", "rgba(2, 3, 4, 0.004)", "rgba(2, 3, 4, 0.004)"),
        ("color", "#0f08", "rgba(0, 255, 0, 0.533)"),
        ("background-color", "#00ff0080", "rgba(0, 255, 0, 0.5)"),
        ("border-left-color", "blue", "blue"),
    ]);
}

#[test]
fn keywords_read_back_in_lowercase() {
    assert_values(&[
        ("display", "BLOCK", "block"),
        ("position", "RELATIVE", "relative"),
        ("position", "static", "static"),
        ("display", "inline-flex", "inline-flex"),
        ("display", "FLOW-ROOT", "flow-root"),
        ("display", "-webkit-box", "-webkit-box"),
        ("display", "-WEBKIT-INLINE-BOX", "-webkit-inline-box"),
        ("display", "none", "none"),
        ("box-sizing", "border-box", "border-box"),
        ("border-top-style", "Solid", "solid"),
        ("border-top-style", "inset", "inset"),
        ("overflow-x", "visible", "visible"),
        ("overflow-x", "clip", "clip"),
        // A legacy alias, resolved when parsed.
        ("overflow-y", "overlay", "auto"),
        ("overflow-y", "scroll", "scroll"),
        ("flex-direction", "column-reverse", "column-reverse"),
        ("flex-wrap", "wrap-reverse", "wrap-reverse"),
        ("justify-content", "space-evenly", "space-evenly"),
        ("align-content", "flex-end", "flex-end"),
        ("align-items", "first baseline", "baseline"),
        ("align-items", "self-start", "self-start"),
        ("justify-items", "self-end", "self-end"),
        ("align-self", "auto", "auto"),
        ("justify-self", "center", "center"),
        ("font-style", "italic", "italic"),
        ("text-align", "center", "center"),
        ("white-space", "pre-wrap", "pre-wrap"),
        ("white-space", "break-spaces", "break-spaces"),
        // The longhands' pairs with a keyword read back as it.
        ("white-space", "PRESERVE nowrap", "pre"),
        ("white-space", "wrap preserve-breaks", "pre-line"),
        (
            "white-space",
            "preserve-breaks nowrap",
            "preserve-breaks nowrap",
        ),
        (
            "white-space",
            "nowrap preserve-breaks",
            "preserve-breaks nowrap",
        ),
        ("white-space-collapse", "PRESERVE-breaks", "preserve-breaks"),
        ("text-wrap-mode", "NOWRAP", "nowrap"),
        ("text-wrap", "nowrap", "nowrap"),
        ("text-transform", "UPPERCASE", "uppercase"),
        ("text-overflow", "ELLIPSIS", "ellipsis"),
        ("float", "LEFT", "left"),
        ("float", "none", "none"),
        ("clear", "Both", "both"),
        ("clear", "right", "right"),
        ("content", "NORMAL", "normal"),
        ("content", "none", "none"),
        ("content", "'a'", "\"a\""),
        (
            "content",
            "\"x\" ATTR(data-x) ''",
            "\"x\" attr(data-x) \"\"",
        ),
        ("display", "TABLE", "table"),
        ("display", "inline-table", "inline-table"),
        ("display", "table-cell", "table-cell"),
        ("display", "LIST-ITEM", "list-item"),
        // Counters: the default value written out; `decimal`, the default
        // style, left out; a style machopo doesn't have kept as written.
        (
            "content",
            "COUNTER(a) counter(b, Upper-Roman) counters(c, '.', decimal) counter(d, foo)",
            "counter(a) counter(b, upper-roman) counters(c, \".\") counter(d, foo)",
        ),
        ("counter-reset", "a b -2", "a 0 b -2"),
        (
            "counter-reset",
            "reversed(a) b reversed(c) 3",
            "reversed(a) b 0 reversed(c) 3",
        ),
        ("counter-reset", "NONE", "none"),
        ("counter-increment", "a b -2", "a 1 b -2"),
        ("counter-set", "a", "a 0"),
        ("list-style-type", "LOWER-ALPHA", "lower-alpha"),
        ("list-style-type", "lower-latin", "lower-latin"),
        ("list-style-type", "'x'", "\"x\""),
        ("list-style-type", "Armenian", "Armenian"),
        ("list-style-position", "INSIDE", "inside"),
        ("list-style-image", "none", "none"),
        // The values that aren't initial; a lone `none` for both.
        ("list-style", "none", "none"),
        ("list-style", "disc outside none", "outside"),
        ("list-style", "none inside", "inside none"),
        ("list-style", "square", "square"),
        ("list-style", "outside outside", "outside outside"),
        ("vertical-align", "MIDDLE", "middle"),
        ("vertical-align", "text-bottom", "text-bottom"),
        ("word-break", "break-all", "break-all"),
        ("overflow-wrap", "anywhere", "anywhere"),
        // A legacy alias.
        ("word-wrap", "break-word", "break-word"),
        ("width", "INHERIT", "inherit"),
        ("color", "initial", "initial"),
        ("display", "unset", "unset"),
    ]);
}

#[test]
fn other_values_read_back_in_canonical_form() {
    assert_values(&[
        ("text-decoration-line", "none", "none"),
        (
            "text-decoration-line",
            "line-through overline underline",
            "underline overline line-through",
        ),
        // Out of range is clamped only when computed; a percentage is a
        // number.
        ("opacity", "-2", "-2"),
        ("opacity", "3", "3"),
        ("opacity", "50%", "0.5"),
        ("order", "-2", "-2"),
        ("z-index", "auto", "auto"),
        ("z-index", "-789", "-789"),
        ("row-gap", "normal", "normal"),
        ("letter-spacing", "normal", "normal"),
        ("letter-spacing", "-0.5px", "-0.5px"),
        ("word-spacing", "1em", "1em"),
        ("text-indent", "10%", "10%"),
        ("text-indent", "-2px", "-2px"),
        ("vertical-align", "-10px", "-10px"),
        ("vertical-align", "20%", "20%"),
        ("column-gap", "0", "0px"),
        ("font-size", "xx-small", "xx-small"),
        ("font-size", "larger", "larger"),
        ("font-size", "20%", "20%"),
        ("font-weight", "normal", "normal"),
        ("font-weight", "BOLD", "bold"),
        ("font-weight", "bolder", "bolder"),
        ("font-weight", "150.25", "150.25"),
        ("line-height", "normal", "normal"),
        ("line-height", "0", "0"),
        ("line-height", "200%", "200%"),
        ("font-family", "Serif", "serif"),
        ("font-family", "system-ui", "system-ui"),
        (
            "font-family",
            "\"New Century Schoolbook\", serif",
            "New Century Schoolbook, serif",
        ),
        // Quoted where unquoted would read back as something else.
        (
            "font-family",
            "'21st Century', fantasy",
            "\"21st Century\", fantasy",
        ),
        (
            "font-family",
            "\"inherit\", \"serif\"",
            "\"inherit\", \"serif\"",
        ),
        ("font-family", "\"Serif\"", "\"Serif\""),
        ("font-family", "\"a serif\"", "\"a serif\""),
        ("font-family", "\"a  b\"", "\"a  b\""),
        ("font-family", "-zπ", "-zπ"),
        ("font-family", "\"-1a\", \"a b\"", "\"-1a\", a b"),
        ("transform", "none", "none"),
        ("transform", "translate(1px)", "translate(1px)"),
        ("transform", "translate(1px, 0%)", "translate(1px, 0%)"),
        ("transform", "TRANSLATEX(-4px)", "translateX(-4px)"),
        ("transform", "translateY(5%)", "translateY(5%)"),
        ("transform", "scale(2)", "scale(2)"),
        ("transform", "scale(3, 4)", "scale(3, 4)"),
        ("transform", "scalex(7)", "scaleX(7)"),
        ("transform", "scaleY(-8)", "scaleY(-8)"),
        (
            "transform",
            "matrix(-0.1, -0.2, -0.3, -0.4, -0.5, -0.6)",
            "matrix(-0.1, -0.2, -0.3, -0.4, -0.5, -0.6)",
        ),
        (
            "transform",
            "translate(1px, 2%) scale(3, 4)",
            "translate(1px, 2%) scale(3, 4)",
        ),
        // Angles keep their units; a unitless 0 is `0deg`.
        ("transform", "rotate(0)", "rotate(0deg)"),
        ("transform", "rotate(0.25TURN)", "rotate(0.25turn)"),
        ("transform", "ROTATEZ(1rad)", "rotateZ(1rad)"),
        ("transform", "skew(90deg, 0)", "skew(90deg, 0deg)"),
        ("transform", "skew(0)", "skew(0deg)"),
        ("transform", "skewx(10grad)", "skewX(10grad)"),
        ("transform", "skewY(-90deg)", "skewY(-90deg)"),
        (
            "transform",
            "translate(1px, 2%) scale(3, 4) rotate(-90deg)",
            "translate(1px, 2%) scale(3, 4) rotate(-90deg)",
        ),
        ("-webkit-transform", "rotate(90deg)", "rotate(90deg)"),
        // The keywords on their axes, the horizontal first; one value's
        // other axis is `center`.
        ("transform-origin", "left", "left center"),
        ("transform-origin", "top", "center top"),
        ("transform-origin", "-1px", "-1px center"),
        ("transform-origin", "center left 6px", "left center 6px"),
        ("transform-origin", "bottom right 7px", "right bottom 7px"),
        ("transform-origin", "top center", "center top"),
        ("transform-origin", "right -4%", "right -4%"),
        ("transform-origin", "-1px -2px -3px", "-1px -2px -3px"),
        ("-webkit-transform-origin", "0 0", "0px 0px"),
        ("transform-box", "fill-box", "fill-box"),
        ("transform-box", "VIEW-BOX", "view-box"),
        ("tab-size", "4", "4"),
        ("tab-size", "0", "0"),
        ("tab-size", "2.5", "2.5"),
        ("tab-size", "1em", "1em"),
        ("tab-size", "0px", "0px"),
        ("object-fit", "COVER", "cover"),
        ("-webkit-box-orient", "VERTICAL", "vertical"),
        ("-webkit-box-orient", "horizontal", "horizontal"),
        ("-webkit-box-direction", "reverse", "reverse"),
        ("-webkit-box-pack", "justify", "justify"),
        ("-webkit-box-align", "baseline", "baseline"),
        ("-webkit-box-flex", "-1.5", "-1.5"),
        ("-webkit-box-ordinal-group", "3", "3"),
        ("-webkit-line-clamp", "NONE", "none"),
        ("contain", "NONE", "none"),
        ("clip", "AUTO", "auto"),
        ("clip", "rect(1px 2px 3px 4px)", "rect(1px, 2px, 3px, 4px)"),
        (
            "clip",
            "rect(1px, auto, -3em, 0)",
            "rect(1px, auto, -3em, 0px)",
        ),
        ("clip-path", "NONE", "none"),
        ("clip-path", "inset(0 1px)", "inset(0px 1px)"),
        (
            "clip-path",
            "inset(1px 2px 1px 2px round 0)",
            "inset(1px 2px)",
        ),
        ("clip-path", "inset(1px 2px 3px)", "inset(1px 2px 3px)"),
        (
            "clip-path",
            "ellipse(1px closest-side)",
            "ellipse(1px closest-side)",
        ),
        (
            "clip-path",
            "inset(10px round 20% / 0px 1px 2% 3em)",
            "inset(10px round 20% / 0px 1px 2% 3em)",
        ),
        (
            "clip-path",
            "rect(0 100% auto 4em round 0 1px)",
            "rect(0px 100% auto 4em round 0px 1px)",
        ),
        ("clip-path", "xywh(0 1% 2px 3em)", "xywh(0px 1% 2px 3em)"),
        ("clip-path", "circle(closest-side)", "circle()"),
        (
            "clip-path",
            "circle(4% at top right)",
            "circle(4% at right top)",
        ),
        (
            "clip-path",
            "ellipse(closest-side closest-side at 10% 20%)",
            "ellipse(at 10% 20%)",
        ),
        (
            "clip-path",
            "ellipse(farthest-side 4%)",
            "ellipse(farthest-side 4%)",
        ),
        (
            "clip-path",
            "polygon(nonzero round 0px, 1px 2px, 3em 4em)",
            "polygon(1px 2px, 3em 4em)",
        ),
        (
            "clip-path",
            "polygon(evenodd round 1px, 1px 2px)",
            "polygon(evenodd round 1px, 1px 2px)",
        ),
        (
            "clip-path",
            "path(nonzero, 'M20,20h60z')",
            "path(\"M 20 20 h 60 Z\")",
        ),
        (
            "clip-path",
            "path(evenodd, 'M 0 0 H 1')",
            "path(evenodd, \"M 0 0 H 1\")",
        ),
        (
            "clip-path",
            "circle(7% at 8% 9%) border-box",
            "circle(7% at 8% 9%)",
        ),
        ("clip-path", "padding-box circle()", "circle() padding-box"),
        ("clip-path", "view-box", "view-box"),
        ("clip-path", "url(#c)", "url(\"#c\")"),
        ("clip-rule", "EVENODD", "evenodd"),
        ("contain", "Strict", "strict"),
        ("contain", "content", "content"),
        ("contain", "paint size", "size paint"),
        ("contain", "paint layout style", "layout style paint"),
        (
            "contain",
            "style inline-size layout",
            "inline-size layout style",
        ),
        ("-webkit-line-clamp", "2", "2"),
        ("object-fit", "scale-down", "scale-down"),
        ("object-position", "left", "left center"),
        ("object-position", "top", "center top"),
        (
            "object-position",
            "bottom 10% right 20%",
            "right 20% bottom 10%",
        ),
        ("object-position", "-20% -30px", "-20% -30px"),
    ]);
}

/// What the parser takes no more than Chrome does: an unquoted family
/// name with a keyword in it; `span` between a grid line's number and name.
#[test]
fn invalid_values_are_not_set() {
    for (property, value) in [
        ("font-family", "cursive serif"),
        ("font-family", "a, Initial, b"),
        ("font-family", "a, b, revert-rule"),
        ("font-family", "default, a"),
        ("font-family", "a inherit"),
        ("grid-row", "2 span first / last"),
        ("grid-column", "first span 1 / last"),
        ("grid-row-start", "a span 2"),
        // Chrome takes no percentage here, no `text-wrap-style`, and not
        // the spec's `preserve-spaces`.
        ("word-spacing", "10%"),
        // `cursor` ends in a keyword, with both numbers of a hot spot or
        // neither; `hand` isn't Chrome's or machopo's, nor an `image-set()` of
        // anything but URLs (Chrome 152).
        ("cursor", "hand"),
        ("cursor", "url(a.png)"),
        ("cursor", "url(a.png) 2, pointer"),
        ("cursor", "url(a.png) 1 2 3, pointer"),
        ("cursor", "url(a.png) 2px 3, pointer"),
        ("cursor", "default, pointer"),
        ("cursor", "pointer, url(a.png)"),
        ("cursor", "url(a.png) pointer"),
        (
            "cursor",
            "image-set(linear-gradient(red, blue) 1x), pointer",
        ),
        ("text-wrap", "balance"),
        ("white-space-collapse", "preserve-spaces"),
        ("letter-spacing", "1"),
        ("vertical-align", "first baseline"),
        // 3D transform functions, which machopo doesn't have; a function's
        // arguments past what it takes.
        ("transform", "translate3d(0, 0, 0)"),
        ("transform", "rotate(45deg) translateZ(0)"),
        ("transform", "rotate(0, 0)"),
        ("transform", "skewX(0, 0)"),
        ("transform", "rotate(1)"),
        // css-transforms 1's grammar (Chrome takes `top 1px`).
        ("transform-origin", "1px 2px 3%"),
        ("transform-origin", "1px 2px left"),
        ("transform-origin", "1px 2px 3px 4px"),
        ("transform-origin", "1px left"),
        ("transform-origin", "top 1px"),
        ("transform-origin", "right left"),
        ("transform-origin", "top bottom"),
        ("transform-origin", "bottom 10% right 20%"),
        ("transform-box", "padding-box"),
        // One keyword, as Chrome; `<position>` has no three values.
        ("tab-size", "-1"),
        ("tab-size", "-1px"),
        ("tab-size", "auto"),
        ("object-fit", "cover scale-down"),
        ("-webkit-box-orient", "inline-axis"),
        ("-webkit-box-orient", "block-axis"),
        ("-webkit-box-pack", "space-between"),
        ("-webkit-box-align", "auto"),
        ("-webkit-box-flex", "1px"),
        ("-webkit-box-ordinal-group", "0"),
        ("-webkit-box-ordinal-group", "1.5"),
        ("-webkit-line-clamp", "0"),
        ("contain", "size inline-size"),
        ("clip", "rect(1px, 2px 3px, 4px)"),
        ("clip", "rect(10%, auto, auto, auto)"),
        ("clip", "rect(1px, 2px, 3px)"),
        ("clip-path", "inset()"),
        ("clip-path", "inset(1px round -2px)"),
        ("clip-path", "circle(1px 2px)"),
        ("clip-path", "circle(-1px)"),
        ("clip-path", "ellipse(3%)"),
        ("clip-path", "polygon(round 1%, 1px 2px)"),
        ("clip-path", "polygon(round 1px nonzero, 1px 2px)"),
        ("clip-path", "polygon(1%)"),
        ("clip-path", "path(evenodd, '')"),
        ("clip-path", "path('M 0 0 X')"),
        ("clip-path", "xywh(0 0 -1px 1px)"),
        ("clip-path", "border-box margin-box"),
        ("clip-path", "circle() circle()"),
        ("clip-path", "url(#c) border-box"),
        ("clip-rule", "auto"),
        ("contain", "inline-size size"),
        ("contain", "paint paint"),
        ("contain", "strict paint"),
        ("contain", "none size"),
        ("contain", "block-size"),
        ("-webkit-line-clamp", "-1"),
        ("-webkit-line-clamp", "2 3"),
        ("display", "-webkit-flex-box"),
        ("object-fit", "auto"),
        ("object-position", "center left 1px"),
        ("object-position", "left right"),
        ("object-position", "1px 2px 3px"),
        // Not yet: images, quotes, the alternative text; and no item is no
        // value. `counters()` needs its separator.
        ("content", "url(a.png)"),
        ("content", "open-quote"),
        ("content", "'a' / 'b'"),
        ("content", "attr()"),
        ("content", "normal 'a'"),
        ("content", "counters(x)"),
        ("content", "counter(none)"),
        ("content", "counter(x, 'a')"),
        ("content", "counters(x, '.', none)"),
        ("content", "counter(x, symbols('a'))"),
        // A counter's name isn't `none`, and its value is an integer.
        ("counter-reset", "none a"),
        ("counter-reset", "a 1.5"),
        ("counter-increment", "reversed(a)"),
        ("counter-set", "inherit 1"),
        // No marker images; `none` at most twice, once with a type.
        ("list-style-image", "url(a.png)"),
        ("list-style", "none none none"),
        ("list-style", "none none disc"),
        ("list-style-type", "symbols('a')"),
        ("list-style-position", "middle"),
        // The flow-relative values aren't supported.
        ("float", "inline-start"),
        ("clear", "inline-end"),
        ("float", "center"),
    ] {
        assert_eq!(read_back(property, value), None, "{property}: {value}");
    }
}

#[test]
fn grid_values_read_back_in_canonical_form() {
    assert_values(&[
        ("grid-template-columns", "none", "none"),
        ("grid-template-columns", "[] 150px [] 1fr []", "150px 1fr"),
        (
            "grid-template-columns",
            "minmax(20%, max-content) fit-content(10px) auto min-content",
            "minmax(20%, max-content) fit-content(10px) auto min-content",
        ),
        (
            "grid-template-columns",
            "repeat(1, [] 10px [])",
            "repeat(1, 10px)",
        ),
        (
            "grid-template-rows",
            "[one] repeat(2, minmax(10px, auto)) [two] 30px [three] repeat(auto-fill, 10px) 40px [four five] repeat(3, [a] 20% [b]) [six]",
            "[one] repeat(2, minmax(10px, auto)) [two] 30px [three] repeat(auto-fill, 10px) 40px [four five] repeat(3, [a] 20% [b]) [six]",
        ),
        ("grid-auto-rows", "1fr 10px", "1fr 10px"),
        ("grid-auto-flow", "row", "row"),
        ("grid-auto-flow", "dense row", "dense"),
        ("grid-auto-flow", "dense column", "column dense"),
        ("grid-template-areas", "none", "none"),
        (
            "grid-template-areas",
            "\"first second\" \"third .\"",
            "\"first second\" \"third .\"",
        ),
        ("grid-template-areas", "\"  a  \t  b  \"", "\"a b\""),
        ("grid-template-areas", "\"first ...\"", "\"first .\""),
        ("grid-row-start", "auto", "auto"),
        ("grid-row-start", "+90", "90"),
        ("grid-row-start", "a", "a"),
        ("grid-row-start", "Z -44", "-44 Z"),
        ("grid-row-start", "span 2", "span 2"),
        ("grid-row-start", "i 2 SpAn", "span 2 i"),
        ("grid-row-start", "span 1 i", "span i"),
        ("grid-row-start", "\\31st", "\\31 st"),
    ]);
}

#[test]
fn shorthands_read_back_from_their_longhands() {
    assert_values(&[
        ("margin", "1px 2px 3px 4px", "1px 2px 3px 4px"),
        ("margin", "1px 2px 3px 2px", "1px 2px 3px"),
        ("margin", "1px 2px 1px 2px", "1px 2px"),
        ("margin", "20px 20px 20px 20px", "20px"),
        ("padding", "1in 96px", "1in 96px"),
        ("inset", "1px auto 1px auto", "1px auto"),
        (
            "border-width",
            "2px thin medium thick",
            "2px thin medium thick",
        ),
        // A border side leaves out what wasn't given.
        ("border-top", "thin", "thin"),
        ("border-top", "red solid 1px", "1px solid red"),
        ("border", "green solid thin", "thin solid green"),
        ("border", "red", "red"),
        (
            "border",
            "medium none currentcolor",
            "medium none currentcolor",
        ),
        ("background", "red", "red"),
        ("background", "none", "none"),
        ("font", "10px/1 Ahem", "10px / 1 Ahem"),
        ("font", "normal normal 12px serif", "12px serif"),
        (
            "font",
            "italic bold 12px/30px Georgia, serif",
            "italic bold 12px / 30px Georgia, serif",
        ),
        (
            "text-decoration",
            "overline underline",
            "underline overline",
        ),
        ("overflow", "clip clip", "clip"),
        ("overflow", "hidden visible", "hidden visible"),
        ("flex", "none", "0 0 auto"),
        ("flex", "1", "1 1 0%"),
        ("flex", "6px 4", "4 1 6px"),
        ("flex-flow", "column nowrap", "column"),
        ("flex-flow", "nowrap", "row"),
        ("flex-flow", "row wrap", "wrap"),
        ("flex-flow", "wrap row-reverse", "row-reverse wrap"),
        ("gap", "10px 10px", "10px"),
        ("gap", "normal 10px", "normal 10px"),
        ("grid-gap", "1px 2px", "1px 2px"),
        ("place-items", "center", "center"),
        ("place-self", "auto center", "auto center"),
        (
            "place-content",
            "space-between start",
            "space-between start",
        ),
        ("grid-row", "auto/auto", "auto"),
        ("grid-row", "i / i", "i"),
        ("grid-row", "i / auto", "i / auto"),
        ("grid-row", "2 i / auto", "2 i"),
        ("grid-column", "2 j / span 3 k", "2 j / span 3 k"),
        ("grid-area", "auto / auto / auto / auto", "auto"),
        ("grid-area", "auto / i / auto / i", "auto / i"),
        ("grid-area", "1 / auto / i / auto", "1 / auto / i"),
        ("grid-area", "1 / i / 2 / auto", "1 / i / 2 / auto"),
        ("grid-area", "i / auto / i / auto", "i / auto"),
        ("grid-area", "i / i / auto / auto", "i / i / auto / auto"),
        ("grid-template", "none / none", "none"),
        ("grid-template", "auto / auto", "auto / auto"),
        ("grid-template", "none / [a] 0px", "none / [a] 0px"),
        ("grid-template", "[a] \"a\" 10px []", "[a] \"a\" 10px"),
        ("grid-template", "\"a\" auto", "\"a\""),
        ("grid-template", "\"a\" [a] [b] \"b\"", "\"a\" [a b] \"b\""),
        (
            "grid-template",
            "\"a\" auto [a] \"b\" auto [b] / 10px",
            "\"a\" [a] \"b\" [b] / 10px",
        ),
        ("margin", "inherit", "inherit"),
        ("grid-area", "initial", "initial"),
    ]);
}

#[test]
fn shorthands_need_all_their_longhands_alike() {
    let block =
        CssPropertyValueSet::parse("margin-top: 1px; margin-right: 1px; margin-bottom: 1px");
    assert_eq!(block.property_value("margin"), "");
    // Not all of the same importance.
    let block = CssPropertyValueSet::parse("margin: 1px; margin-top: 1px !important");
    assert_eq!(block.property_value("margin"), "");
    assert_eq!(block.property_priority("margin"), "");
    let block = CssPropertyValueSet::parse("margin: 1px !important");
    assert_eq!(block.property_value("margin"), "1px");
    assert_eq!(block.property_priority("margin"), "important");
    // A CSS-wide keyword only if it is all of them's.
    let block = CssPropertyValueSet::parse("margin: initial; margin-top: 1px");
    assert_eq!(block.property_value("margin"), "");
    let block = CssPropertyValueSet::parse("margin: initial; margin-top: inherit");
    assert_eq!(block.property_value("margin"), "");
    // `border` needs every side alike, including what was left out.
    let block = CssPropertyValueSet::parse("border: 1px; border-top-color: red");
    assert_eq!(block.property_value("border"), "");
    let block = CssPropertyValueSet::parse("border: 1px; border-top-color: currentcolor");
    assert_eq!(block.property_value("border"), "");
    // Chrome's `border`, `font` and `text-decoration` have longhands machopo
    // doesn't, which only the shorthand itself sets.
    let block = CssPropertyValueSet::parse("text-decoration-line: underline");
    assert_eq!(block.property_value("text-decoration"), "");
    let block = CssPropertyValueSet::parse(
        "font-style: italic; font-weight: bold; font-size: 1px; line-height: 2; font-family: a",
    );
    assert_eq!(block.property_value("font"), "");
    // `background` needs every longhand, each a value per layer.
    let block = CssPropertyValueSet::parse("background-color: red");
    assert_eq!(block.property_value("background"), "");
    let block = CssPropertyValueSet::parse("background: red; background-color: blue");
    assert_eq!(block.property_value("background"), "blue");
    let block = CssPropertyValueSet::parse("background: url(a), url(b); background-size: 1px");
    assert_eq!(block.property_value("background"), "");
    let block = CssPropertyValueSet::parse("border: solid; border-style: none");
    assert_eq!(block.property_value("border"), "none");
    // `grid-template` writes the areas with the rows: one size each, no
    // `repeat()`.
    let block = CssPropertyValueSet::parse("grid-template: \"a\" \"b\"; grid-template-rows: 1px");
    assert_eq!(block.property_value("grid-template"), "");
    let block = CssPropertyValueSet::parse(
        "grid-template: \"a\" \"b\"; grid-template-rows: repeat(2, 1px)",
    );
    assert_eq!(block.property_value("grid-template"), "");
    let block = CssPropertyValueSet::parse(
        "grid-template: \"a\" \"b\"; grid-template-rows: repeat(1, 1px) 2px",
    );
    assert_eq!(block.property_value("grid-template"), "");
    let block =
        CssPropertyValueSet::parse("grid-template: \"a\" \"b\"; grid-template-rows: 1px auto");
    assert_eq!(block.property_value("grid-template"), "\"a\" 1px \"b\"");
}

#[test]
fn css_text_groups_longhands_into_shorthands() {
    for (css, expected) in [
        ("", ""),
        ("z-index: 33; invalid", "z-index: 33;"),
        ("display: inline!important", "display: inline !important;"),
        ("color: black ! important;", "color: black !important;"),
        ("margin: initial;", "margin: initial;"),
        ("font: 10px/1 Ahem", "font: 10px / 1 Ahem;"),
        ("background: red", "background: red;"),
        ("background-color: green", "background-color: green;"),
        (
            "background-color: blue; background: red !important; background-color: green;",
            "background: red !important;",
        ),
        // Where the first of the longhands is.
        (
            "margin-top: 1px; color: red; margin-right: 2px; margin-bottom: 3px; margin-left: 4px",
            "margin: 1px 2px 3px 4px; color: red;",
        ),
        (
            "margin-right: 10px; margin-left: 10px; margin-top: 10px; margin-bottom: 10px;",
            "margin: 10px;",
        ),
        (
            "margin-right: 10px; margin-left: 10px; margin-top: 10px; margin-bottom: 10px!important;",
            "margin-right: 10px; margin-left: 10px; margin-top: 10px; margin-bottom: 10px !important;",
        ),
        // The larger shorthand first; `border` only with its
        // `border-image` too, which it resets (none set by the sides).
        ("border: 1px; border-top: 1px;", "border: 1px;"),
        ("border: 1px solid red;", "border: 1px solid red;"),
        (
            "border-top: 1px; border-right: 1px; border-bottom: 1px; border-left: 1px;",
            "border-width: 1px; border-style: none; border-color: currentcolor;",
        ),
        (
            "border: 1px; border-top: 2px;",
            "border-width: 2px 1px 1px; border-style: none; border-color: currentcolor; \
             border-image: none;",
        ),
        (
            "border: 1px; border-top: 1px !important;",
            "border-right: 1px; border-bottom: 1px; border-left: 1px; border-image: none; \
             border-top: 1px !important;",
        ),
        (
            "border: 1px; border-top-color: red;",
            "border-width: 1px; border-style: none; border-color: red currentcolor currentcolor; \
             border-image: none;",
        ),
        (
            "border: 1px solid red; border-image-slice: 10",
            "border-width: 1px; border-style: solid; border-color: red; \
             border-image: none 10 / 1 / 0 stretch;",
        ),
        (
            "border-image: url(a.png) 30 round; border: 1px",
            "border: 1px;",
        ),
        (
            "border: 1px !important; border-image: none",
            "border: 1px !important;",
        ),
        (
            "border: 1px; border-image-source: none !important",
            "border-width: 1px; border-style: none; border-color: currentcolor; \
             border-image-slice: 100%; border-image-width: 1; border-image-outset: 0; \
             border-image-repeat: stretch; border-image-source: none !important;",
        ),
        ("border-width: 1px;", "border-width: 1px;"),
        (
            "overflow-x: scroll; overflow-y: hidden;",
            "overflow: scroll hidden;",
        ),
        (
            "font-family: sans-serif; line-height: 2em; font-size: 3em; font-style: italic; font-weight: bold;",
            "font-family: sans-serif; line-height: 2em; font-size: 3em; font-style: italic; font-weight: bold;",
        ),
        // All four of `grid-area`'s, the larger shorthand.
        ("grid-row: a; grid-column: b", "grid-area: a / b;"),
        ("grid-row: a", "grid-row: a;"),
        // Not a shorthand any of whose longhands is already written.
        (
            "border-width: 1px; border-top-style: solid; border-top-color: red",
            "border-width: 1px; border-top-style: solid; border-top-color: red;",
        ),
        ("grid-area: a", "grid-area: a;"),
        // `white-space` from its longhands, as a keyword where the pair has
        // one; `text-wrap` only where it was set (Chrome's also sets
        // `text-wrap-style`).
        (
            "white-space-collapse: preserve; text-wrap-mode: nowrap",
            "white-space: pre;",
        ),
        ("text-wrap-mode: nowrap", "text-wrap-mode: nowrap;"),
        ("text-wrap: nowrap", "text-wrap: nowrap;"),
    ] {
        assert_eq!(css_text(css), expected, "{css}");
    }
}

#[test]
fn a_parsed_block_keeps_one_declaration_per_property() {
    // The last one, where it is; an earlier `!important` one beats it.
    // The normal ones come before the important ones (Chrome's order).
    let block = CssPropertyValueSet::parse(
        "color: red; width: 1px; color: blue; height: 2px !important; height: 3px; display: block",
    );
    assert_eq!(
        block.css_text(),
        "width: 1px; color: blue; display: block; height: 2px !important;"
    );
    assert_eq!(block.len(), 4);
    let names: Vec<_> = (0..5).map(|i| block.item(i)).collect();
    assert_eq!(
        names,
        [
            Some("width"),
            Some("color"),
            Some("display"),
            Some("height"),
            None
        ]
    );
    assert_eq!(block.property_priority("height"), "important");
    assert_eq!(block.property_priority("color"), "");
    // A shorthand adds longhands; an alias is its property.
    let block = CssPropertyValueSet::parse(
        "padding: 10px !important; padding-left: 20px; grid-row-gap: 1px",
    );
    assert_eq!(block.property_value("padding-left"), "10px");
    assert_eq!(block.len(), 5);
    assert_eq!(block.item(0), Some("row-gap"));
    assert_eq!(block.property_value("grid-row-gap"), "1px");
    // Names ignore ASCII case.
    assert_eq!(block.property_value("PADDING-left"), "10px");
}

#[test]
fn set_property_tells_whether_the_block_changed() {
    let mut block = CssPropertyValueSet::parse("color: red; width: 1px");
    // In place.
    assert!(block.set_property("color", "blue", ""));
    assert_eq!(block.css_text(), "color: blue; width: 1px;");
    // The same value and importance changes nothing.
    assert!(!block.set_property("COLOR", "blue", ""));
    assert!(block.set_property("color", "blue", "IMPORTANT"));
    assert_eq!(block.css_text(), "color: blue !important; width: 1px;");
    // Otherwise at the end.
    assert!(block.set_property("height", "2px", ""));
    assert_eq!(
        block.css_text(),
        "color: blue !important; width: 1px; height: 2px;"
    );
    // Nothing for an unsupported property, a value we can't parse, or a
    // priority other than `important`.
    for (name, value, priority) in [
        ("mix-blend-mode", "multiply", ""),
        ("position", "sticky-ish", ""),
        ("width", "red", ""),
        ("width", "2px !important", ""),
        ("width", "2px;", ""),
        ("width", "2px", "!important"),
        ("width", "2px", "x"),
        ("margin", "1px 2px 3px 4px 5px", ""),
    ] {
        assert!(
            !block.set_property(name, value, priority),
            "{name}: {value}"
        );
    }
    assert_eq!(
        block.css_text(),
        "color: blue !important; width: 1px; height: 2px;"
    );
    // An empty value removes.
    assert!(block.set_property("width", "", ""));
    assert!(!block.set_property("width", "", ""));
    assert_eq!(block.css_text(), "color: blue !important; height: 2px;");
    // A shorthand sets each longhand; changed if any did.
    assert!(block.set_property("margin", "1px", ""));
    assert!(!block.set_property("margin", "1px", ""));
    assert!(block.set_property("margin", "1px 2px", ""));
    assert_eq!(
        block.css_text(),
        "color: blue !important; height: 2px; margin: 1px 2px;"
    );
}

#[test]
fn remove_property_returns_the_old_value() {
    let mut block = CssPropertyValueSet::parse("margin: 1px 2px; color: red; --x: y");
    assert_eq!(
        block.remove_property("margin-top"),
        ("1px".to_owned(), true)
    );
    // A shorthand's value is "" once a longhand is gone; its other
    // longhands are still removed.
    assert_eq!(block.remove_property("margin"), (String::new(), true));
    assert_eq!(block.remove_property("margin"), (String::new(), false));
    assert_eq!(block.remove_property("position"), (String::new(), false));
    assert_eq!(block.remove_property("--X"), (String::new(), false));
    assert_eq!(block.remove_property("--x"), ("y".to_owned(), true));
    assert_eq!(block.remove_property("Color"), ("red".to_owned(), true));
    assert!(block.is_empty());
}

#[test]
fn custom_properties_are_kept_as_written() {
    let block = CssPropertyValueSet::parse(
        "--a:   foo  bar  !important ; color: red; --A: 2px; --b: ; --c: f(!); --a: x",
    );
    assert_eq!(
        block.css_text(),
        "color: red; --A: 2px; --b: ; --c: f(!); --a: foo  bar !important;"
    );
    // Names are case-sensitive.
    assert_eq!(block.property_value("--a"), "foo  bar");
    assert_eq!(block.property_value("--A"), "2px");
    assert_eq!(block.property_priority("--a"), "important");
    assert_eq!(block.item(1), Some("--A"));
    // `;`, `!` and unmatched closing brackets only inside blocks.
    let mut block = CssPropertyValueSet::default();
    for value in ["a;b", "a ! b", "a)", "]", "x }"] {
        assert!(!block.set_property("--x", value, ""), "{value}");
    }
    assert!(block.set_property("--x", " [a; !] {b} ", ""));
    assert_eq!(block.property_value("--x"), "[a; !] {b}");
    assert!(!block.set_property("--x", "[a; !] {b}", ""));
    // `--` alone is not a custom property.
    assert!(!block.set_property("--", "1", ""));
    assert!(block.set_property("--x", "", ""));
    assert!(block.is_empty());
}

#[test]
fn attribute_names_map_to_properties() {
    for (attribute, property) in [
        ("color", Some("color")),
        ("backgroundColor", Some("background-color")),
        ("background-color", Some("background-color")),
        ("borderTopLeftRadius", Some("border-top-left-radius")),
        ("boxShadow", Some("box-shadow")),
        ("textEmphasisColor", None),
        ("gridRowGap", Some("row-gap")),
        ("gridGap", Some("gap")),
        ("margin", Some("margin")),
        // Capitals and dashes don't mix.
        ("grid-rowGap", None),
        ("BackgroundColor", None),
        ("Color", None),
        ("--x", None),
        ("", None),
        ("mixBlendMode", None),
        // `float` has a second attribute (CSSOM, "cssFloat").
        ("float", Some("float")),
        ("cssFloat", Some("float")),
        ("CssFloat", None),
        // A `-webkit-` alias, by either capitalization (Chrome's
        // `webkitFoo` and `WebkitFoo`).
        ("webkitAppearance", Some("appearance")),
        ("WebkitAppearance", Some("appearance")),
        ("-webkit-appearance", Some("appearance")),
        ("webkit-appearance", None),
        ("webkitColor", None),
    ] {
        assert_eq!(
            css_property_for_attribute(attribute),
            property,
            "{attribute}"
        );
    }
    // Every property's camel-cased name, each once and by its standard
    // name, maps back to it.
    let names = css_property_attributes();
    for name in [
        "backgroundColor",
        "gridTemplateAreas",
        "margin",
        "rowGap",
        "float",
        "cssFloat",
    ] {
        assert!(names.iter().any(|n| n == name), "{name}");
    }
    assert!(!names.iter().any(|n| n == "gridRowGap"), "an alias");
    let mut unique = names.clone();
    unique.dedup();
    assert_eq!(unique, names);
    for name in &names {
        assert!(css_property_for_attribute(name).is_some(), "{name}");
    }
}

/// The background longhands and shorthands as declared, as Chrome writes
/// them (WPT's `css/css-backgrounds/parsing/*-valid.html` and
/// `background-shorthand-serialization.html`).
#[test]
fn background_serialization() {
    for (property, value, expected) in [
        (
            "background-image",
            "url(a.png), none",
            r#"url("a.png"), none"#,
        ),
        (
            "background-image",
            "linear-gradient(to bottom left, red, 10%, blue 1em 2em)",
            "linear-gradient(to left bottom, red, 10%, blue 1em, blue 2em)",
        ),
        (
            "background-position-x",
            "right 10px, center",
            "right 10px, center",
        ),
        ("background-position-y", "bottom, 2em", "bottom, 2em"),
        ("background-position", "center left", "left center"),
        (
            "background-position",
            "top 17px right 18px",
            "right 18px top 17px",
        ),
        (
            "background-position",
            "1px, bottom",
            "1px center, center bottom",
        ),
        ("background-size", "1px", "1px auto"),
        ("background-size", "auto auto", "auto"),
        (
            "background-size",
            "auto 1px, 2% 3%, contain",
            "auto 1px, 2% 3%, contain",
        ),
        (
            "background-repeat",
            "repeat repeat, repeat no-repeat",
            "repeat, repeat-x",
        ),
        ("background-repeat", "space round", "space round"),
        ("background-attachment", "local, fixed", "local, fixed"),
        (
            "background-clip",
            "content-box, padding-box",
            "content-box, padding-box",
        ),
        ("background", "yellow", "yellow"),
        ("background", "none", "none"),
        (
            "background",
            "no-repeat url(/favicon.ico)",
            r#"url("/favicon.ico") no-repeat"#,
        ),
        (
            "background",
            "url(/favicon.ico) no-repeat, url(/favicon.ico) no-repeat",
            r#"url("/favicon.ico") no-repeat, url("/favicon.ico") no-repeat"#,
        ),
        (
            "background",
            "url(/favicon.ico) 0% 0% / 10rem",
            r#"url("/favicon.ico") 0% 0% / 10rem auto"#,
        ),
        (
            "background",
            "url(/favicon.ico) left top no-repeat, \
             url(/favicon.ico) center center / 100% 100% no-repeat, url(/favicon.ico) white",
            r#"url("/favicon.ico") left top no-repeat, url("/favicon.ico") center center / 100% 100% no-repeat, url("/favicon.ico") white"#,
        ),
        (
            "background",
            "url(x) 1px 2px / 3px 4px space round local padding-box content-box, \
             rgb(5, 6, 7) url(x) 1px 2px / 3px 4px space round local padding-box content-box",
            r#"url("x") 1px 2px / 3px 4px space round local padding-box content-box, url("x") 1px 2px / 3px 4px space round local padding-box content-box rgb(5, 6, 7)"#,
        ),
        ("background", "content-box", "content-box"),
        // A value at its initial value is left out, even when given.
        ("background", "padding-box border-box", "none"),
        (
            "background",
            "url(a) repeat scroll 0% 0% transparent",
            r#"url("a")"#,
        ),
        (
            "background",
            "padding-box content-box",
            "padding-box content-box",
        ),
    ] {
        let mut block = CssPropertyValueSet::default();
        assert!(
            block.set_property(property, value, ""),
            "{property}: {value}"
        );
        assert_eq!(
            block.property_value(property),
            expected,
            "{property}: {value}"
        );
    }
    for (property, value) in [
        ("background", "red, url(a)"),
        ("background", "url(a) url(b)"),
        ("background", "1px / 2px 3px 4px"),
        ("background", "/ 1px"),
        ("background", "border-box padding-box content-box"),
        ("background-image", "'a.png'"),
        ("background-position", "left right"),
        ("background-position-x", "top"),
        ("background-size", "-1px"),
        ("background-repeat", "repeat-x repeat"),
        ("background-clip", "text"),
    ] {
        let mut block = CssPropertyValueSet::default();
        assert!(
            !block.set_property(property, value, ""),
            "{property}: {value}"
        );
    }
}

/// `border-radius` and its corners as declared, as Chrome writes them (WPT's
/// `css/css-backgrounds/parsing/border-radius-valid.html` and `-invalid`).
#[test]
fn border_radius_serialization() {
    for (property, value, expected) in [
        ("border-radius", "1px", "1px"),
        ("border-radius", "1px 5%", "1px 5%"),
        ("border-radius", "1px 2% 3px", "1px 2% 3px"),
        ("border-radius", "1px / 2px", "1px / 2px"),
        (
            "border-radius",
            "5em / 1px 2% 3px 4%",
            "5em / 1px 2% 3px 4%",
        ),
        (
            "border-radius",
            "1px 2px 3em / 1px 2px 3%",
            "1px 2px 3em / 1px 2px 3%",
        ),
        (
            "border-radius",
            "1px 1px 1px 2% / 1px 2% 1px 2%",
            "1px 1px 1px 2% / 1px 2%",
        ),
        (
            "border-radius",
            "1px 1px 1px 1px / 1px 1px 2% 1px",
            "1px / 1px 1px 2%",
        ),
        (
            "border-radius",
            "1px 2% 2% 2% / 1px 2% 3px 2%",
            "1px 2% 2% / 1px 2% 3px",
        ),
        ("border-top-left-radius", "10px", "10px"),
        ("border-top-left-radius", "10px 10px", "10px"),
        ("border-bottom-right-radius", "30px 40%", "30px 40%"),
    ] {
        let mut block = CssPropertyValueSet::default();
        assert!(
            block.set_property(property, value, ""),
            "{property}: {value}"
        );
        assert_eq!(
            block.property_value(property),
            expected,
            "{property}: {value}"
        );
    }
    // The shorthand sets every corner.
    let block = CssPropertyValueSet::parse("border-radius: 1px 2px / 3px");
    assert_eq!(block.property_value("border-top-right-radius"), "2px 3px");
    assert_eq!(block.property_value("border-bottom-left-radius"), "2px 3px");
    for (property, value) in [
        ("border-radius", "auto"),
        ("border-radius", "-1px"),
        ("border-radius", "1px 2px 3px 4px 5px"),
        ("border-radius", "1em /"),
        ("border-radius", "1px / 2px / 3px"),
        ("border-radius", "4 / 5"),
        ("border-top-left-radius", "10px 20px 30px"),
        ("border-top-left-radius", "1px -2px"),
    ] {
        let mut block = CssPropertyValueSet::default();
        assert!(
            !block.set_property(property, value, ""),
            "{property}: {value}"
        );
    }
}

#[test]
fn box_shadow_and_outline_serialization() {
    for (property, value, expected) in [
        ("box-shadow", "none", "none"),
        // Blink's order: the color, the lengths, then `inset`; what was
        // left out stays out.
        ("box-shadow", "1px 2px", "1px 2px"),
        (
            "box-shadow",
            "inset 1px 2px 3px red",
            "red 1px 2px 3px inset",
        ),
        (
            "box-shadow",
            "1px 2px 3px -4px rgb(0, 0, 255) inset",
            "rgb(0, 0, 255) 1px 2px 3px -4px inset",
        ),
        (
            "box-shadow",
            "1em 0 currentcolor, inset 0 0 2px",
            "currentcolor 1em 0px, 0px 0px 2px inset",
        ),
        ("outline-style", "auto", "auto"),
        (
            "outline-color",
            "-webkit-focus-ring-color",
            "-webkit-focus-ring-color",
        ),
        ("outline-offset", "-2px", "-2px"),
        ("outline-width", "thin", "thin"),
        // Color, style, width, as Blink's longhands go; what the shorthand
        // left out isn't written.
        ("outline", "1px solid red", "red solid 1px"),
        ("outline", "auto", "auto"),
        (
            "outline",
            "auto 1px -webkit-focus-ring-color",
            "-webkit-focus-ring-color auto 1px",
        ),
        ("outline", "double 4px", "double 4px"),
        ("border", "1px dotted red", "1px dotted red"),
        (
            "border-style",
            "dashed double groove ridge",
            "dashed double groove ridge",
        ),
        ("border-style", "hidden outset", "hidden outset"),
    ] {
        let mut block = CssPropertyValueSet::default();
        assert!(
            block.set_property(property, value, ""),
            "{property}: {value}"
        );
        assert_eq!(
            block.property_value(property),
            expected,
            "{property}: {value}"
        );
    }
    for (property, value) in [
        ("box-shadow", "1px"),
        ("box-shadow", "1px 2px -3px"),
        ("box-shadow", "1px 2px 3px 4px 5px"),
        ("box-shadow", "inset inset 1px 2px"),
        ("box-shadow", "red 1px red 2px"),
        ("box-shadow", "1px 2px, none"),
        ("box-shadow", "1px 2% 3px"),
        ("outline-style", "hidden"),
        ("outline-width", "-1px"),
        ("outline-offset", "10%"),
        ("outline", "solid solid"),
    ] {
        let mut block = CssPropertyValueSet::default();
        assert!(
            !block.set_property(property, value, ""),
            "{property}: {value}"
        );
    }
}

/// What DevTools lists of a block it has no text of: each longhand in
/// `item()`'s order (the important ones last, as parsed) with its value
/// and priority, those a shorthand left out implicit,
/// the shorthand that set it, custom properties as written.
#[test]
fn declarations_are_listed_for_devtools() {
    let block =
        CssPropertyValueSet::parse("color: red !important; margin: 1px 2px; border-top: solid; --x: { a }");
    let listed: Vec<_> = block
        .inspector_declarations()
        .into_iter()
        .map(|declaration| {
            (
                declaration.name,
                declaration.value,
                declaration.important,
                declaration.implicit,
                declaration.shorthand,
            )
        })
        .collect();
    let row = |name: &str, value: &str, important, implicit, shorthand| {
        (name.to_owned(), value.to_owned(), important, implicit, shorthand)
    };
    assert_eq!(
        listed,
        [
            row("margin-top", "1px", false, false, Some("margin")),
            row("margin-right", "2px", false, false, Some("margin")),
            row("margin-bottom", "1px", false, false, Some("margin")),
            row("margin-left", "2px", false, false, Some("margin")),
            row("border-top-width", "medium", false, true, Some("border-top")),
            row("border-top-style", "solid", false, false, Some("border-top")),
            row("border-top-color", "currentcolor", false, true, Some("border-top")),
            row("--x", "{ a }", false, false, None),
            row("color", "red", true, false, None),
        ]
    );
}
