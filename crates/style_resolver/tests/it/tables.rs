//! The table properties as declared: what they parse to and read back as
//! (Chrome's).

use style_resolver::CssPropertyValueSet;

/// What `property: value` reads back as on its own, or `None` if dropped.
fn read_back(property: &str, value: &str) -> Option<String> {
    let mut block = CssPropertyValueSet::default();
    block.set_property(property, value, "");
    let text = block.property_value(property);
    (!text.is_empty()).then_some(text)
}

#[test]
fn the_table_properties_parse_and_serialize() {
    for (property, value, expected) in [
        ("border-collapse", "COLLAPSE", Some("collapse")),
        ("border-collapse", "separate", Some("separate")),
        ("border-collapse", "auto", None),
        ("table-layout", "fixed", Some("fixed")),
        ("table-layout", "auto", Some("auto")),
        ("table-layout", "none", None),
        ("caption-side", "bottom", Some("bottom")),
        ("caption-side", "top", Some("top")),
        // Firefox's, not Chrome's.
        ("caption-side", "left", None),
        ("caption-side", "block-start", None),
        ("empty-cells", "hide", Some("hide")),
        ("empty-cells", "show", Some("show")),
        ("empty-cells", "none", None),
        // One length when both are the same.
        ("border-spacing", "0", Some("0px")),
        ("border-spacing", "10px 20px", Some("10px 20px")),
        ("border-spacing", "3px 3px", Some("3px")),
        ("border-spacing", "1em 2px", Some("1em 2px")),
        (
            "border-spacing",
            "calc(10px + 0.5em) calc(10px - 0.5em)",
            Some("calc(0.5em + 10px) calc(-0.5em + 10px)"),
        ),
        ("border-spacing", "inherit", Some("inherit")),
        ("border-spacing", "-1px", None),
        ("border-spacing", "1px -1px", None),
        ("border-spacing", "10%", None),
        ("border-spacing", "1px 2px 3px", None),
        ("border-spacing", "auto", None),
    ] {
        assert_eq!(
            read_back(property, value).as_deref(),
            expected,
            "{property}: {value}"
        );
    }
}
