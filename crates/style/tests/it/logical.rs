//! Flow-relative (logical) properties: each its own property, which the
//! cascade maps to a physical one (machopo has only `horizontal-tb` and `ltr`:
//! block-start is top, inline-start is left) and weighs against it in
//! declaration order; CSSOM reads them back as written.

use computed_style::ComputedStyle;
use style_resolver::CssPropertyValueSet;

use crate::styles;

/// `property`'s computed value in `style`.
fn value(style: &ComputedStyle, property: &str) -> String {
    style_resolver::computed_value(style, property).unwrap_or_else(|| panic!("{property}"))
}

/// Each flow-relative longhand sets its physical one, and reads back as
/// it: block-start is top, block-end bottom, inline-start left, inline-end
/// right, the inline size the width and the block size the height.
#[test]
fn each_sets_its_physical_longhand() {
    // (flow-relative, physical, value).
    let cases = [
        ("margin-block-start", "margin-top", "1px"),
        ("margin-block-end", "margin-bottom", "2px"),
        ("margin-inline-start", "margin-left", "3px"),
        ("margin-inline-end", "margin-right", "4px"),
        ("padding-block-start", "padding-top", "5px"),
        ("padding-block-end", "padding-bottom", "6px"),
        ("padding-inline-start", "padding-left", "7px"),
        ("padding-inline-end", "padding-right", "8px"),
        ("inset-block-start", "top", "9px"),
        ("inset-block-end", "bottom", "10px"),
        ("inset-inline-start", "left", "11px"),
        ("inset-inline-end", "right", "12px"),
        ("border-block-start-style", "border-top-style", "solid"),
        ("border-block-end-style", "border-bottom-style", "dashed"),
        ("border-inline-start-style", "border-left-style", "dotted"),
        ("border-inline-end-style", "border-right-style", "double"),
        ("border-block-start-width", "border-top-width", "1px"),
        ("border-block-end-width", "border-bottom-width", "2px"),
        ("border-inline-start-width", "border-left-width", "3px"),
        ("border-inline-end-width", "border-right-width", "4px"),
        (
            "border-block-start-color",
            "border-top-color",
            "rgb(255, 0, 0)",
        ),
        (
            "border-block-end-color",
            "border-bottom-color",
            "rgb(0, 128, 0)",
        ),
        (
            "border-inline-start-color",
            "border-left-color",
            "rgb(0, 0, 255)",
        ),
        (
            "border-inline-end-color",
            "border-right-color",
            "rgb(255, 255, 0)",
        ),
        ("inline-size", "width", "13px"),
        ("block-size", "height", "14px"),
        ("min-inline-size", "min-width", "15px"),
        ("min-block-size", "min-height", "16px"),
        ("max-inline-size", "max-width", "17px"),
        ("max-block-size", "max-height", "18px"),
        // A corner's block side first.
        ("border-start-start-radius", "border-top-left-radius", "1px"),
        (
            "border-start-end-radius",
            "border-top-right-radius",
            "2px 3px",
        ),
        (
            "border-end-start-radius",
            "border-bottom-left-radius",
            "4px",
        ),
        ("border-end-end-radius", "border-bottom-right-radius", "5%"),
    ];
    let declarations: String = cases
        .iter()
        .map(|(logical, _, value)| format!("{logical}: {value}; "))
        .collect();
    let styles = styles(&format!(
        "<style>#a {{ position: relative; {declarations} }}</style><div id=a></div>"
    ));
    let a = &styles["a"];
    for (logical, physical, expected) in cases {
        assert_eq!(value(a, physical), expected, "{logical}");
        assert_eq!(value(a, logical), expected, "{logical} reads back");
    }
}

/// The flow-relative shorthands: `margin-block` and its kin take the start
/// and then the end (the same if left out); `border-block-start` and its
/// kin `border-top`'s value for one side, `border-block` and
/// `border-inline` for both.
#[test]
fn shorthands_set_both_sides() {
    let styles = styles(
        "<style>
           #a { margin-block: 1px 2px; margin-inline: 3px; padding-block: 4px;
                padding-inline: 5px 6px; position: relative; inset-block: 7px 8px;
                inset-inline: auto 9px }
           #b { border-block: 2px solid red; border-inline-start: 3px dashed blue }
           #c { border-block-width: 4px 5px; border-block-style: dotted;
                border-block-color: lime blue; border-inline-width: 6px;
                border-inline-style: solid double; border-inline-color: red }
           #d { border-block-end: thin dotted }
         </style><div id=a></div><div id=b></div><div id=c></div><div id=d></div>",
    );
    let expect = |id: &str, cases: &[(&str, &str)]| {
        for (property, expected) in cases {
            assert_eq!(value(&styles[id], property), *expected, "#{id} {property}");
        }
    };
    expect(
        "a",
        &[
            ("margin-top", "1px"),
            ("margin-bottom", "2px"),
            ("margin-left", "3px"),
            ("margin-right", "3px"),
            ("padding-top", "4px"),
            ("padding-bottom", "4px"),
            ("padding-left", "5px"),
            ("padding-right", "6px"),
            ("top", "7px"),
            ("bottom", "8px"),
            ("left", "auto"),
            ("right", "9px"),
        ],
    );
    expect(
        "b",
        &[
            ("border-top-width", "2px"),
            ("border-top-style", "solid"),
            ("border-top-color", "rgb(255, 0, 0)"),
            ("border-bottom-width", "2px"),
            ("border-bottom-style", "solid"),
            ("border-bottom-color", "rgb(255, 0, 0)"),
            ("border-left-width", "3px"),
            ("border-left-style", "dashed"),
            ("border-left-color", "rgb(0, 0, 255)"),
            ("border-right-style", "none"),
        ],
    );
    expect(
        "c",
        &[
            ("border-top-width", "4px"),
            ("border-bottom-width", "5px"),
            ("border-top-style", "dotted"),
            ("border-bottom-style", "dotted"),
            ("border-top-color", "rgb(0, 255, 0)"),
            ("border-bottom-color", "rgb(0, 0, 255)"),
            ("border-left-width", "6px"),
            ("border-right-width", "6px"),
            ("border-left-style", "solid"),
            ("border-right-style", "double"),
            ("border-left-color", "rgb(255, 0, 0)"),
            ("border-right-color", "rgb(255, 0, 0)"),
        ],
    );
    // Left out: the initial color, `currentcolor`.
    expect(
        "d",
        &[
            ("border-bottom-width", "1px"),
            ("border-bottom-style", "dotted"),
            ("border-bottom-color", "rgb(0, 0, 0)"),
            ("border-top-style", "none"),
        ],
    );
}

/// A flow-relative property and its physical one are weighed in the
/// cascade as one: the later in a block wins, whichever it is, shorthands
/// too; across blocks, specificity, importance and the style attribute
/// decide as for any property. Another group is left alone.
#[test]
fn logical_and_physical_go_by_the_cascade_order() {
    let styles = styles(
        "<style>
           #a { margin-left: 1px; margin-inline-start: 2px }
           #b { margin-inline-start: 2px; margin-left: 1px }
           #c { margin-left: 1px; margin-inline-start: 2px; margin-left: 3px }
           #d { margin-inline: 4px; margin: 5px }
           #e { margin: 5px; margin-inline: 4px }
           div#f { margin-inline-start: 6px }
           #f { margin-left: 7px }
           #g { margin-inline-start: 8px !important; margin-left: 9px }
           #h { inline-size: 1px; width: 2px; max-block-size: 3px; max-height: 4px;
                border-block-start: 1px solid; border-top-width: 2px }
           #i { margin-inline-start: 1px; padding-left: 2px }
           #j { margin-left: 3px }
           #k { border-start-end-radius: 1px; border-radius: 2px }
           #l { border-radius: 2px; border-start-end-radius: 1px }
         </style>
         <div id=a></div><div id=b></div><div id=c></div><div id=d></div>
         <div id=e></div><div id=f></div><div id=g></div><div id=h></div>
         <div id=i></div><div id=j style='margin-inline-start: 4px'></div>
         <div id=k></div><div id=l></div>",
    );
    let left = |id: &str| value(&styles[id], "margin-left");
    assert_eq!(left("a"), "2px", "the flow-relative one after");
    assert_eq!(left("b"), "1px", "the physical one after");
    assert_eq!(left("c"), "3px", "the last of a property counts");
    assert_eq!(left("d"), "5px", "a physical shorthand after");
    assert_eq!(left("e"), "4px", "a flow-relative shorthand after");
    assert_eq!(value(&styles["e"], "margin-top"), "5px");
    assert_eq!(left("f"), "6px", "by specificity");
    assert_eq!(left("g"), "8px", "by importance");
    assert_eq!(left("j"), "4px", "the style attribute");
    let h = &styles["h"];
    assert_eq!(value(h, "width"), "2px");
    assert_eq!(value(h, "max-height"), "4px");
    assert_eq!(value(h, "border-top-width"), "2px");
    assert_eq!(value(h, "border-top-style"), "solid");
    let corner = |id: &str| value(&styles[id], "border-top-right-radius");
    assert_eq!(corner("k"), "2px", "border-radius after a corner");
    assert_eq!(corner("l"), "1px", "a corner after border-radius");
    let i = &styles["i"];
    assert_eq!(
        (left("i"), value(i, "padding-left")),
        ("1px".into(), "2px".into())
    );
}

/// A flow-relative property takes `var()`, also as a shorthand, and the
/// CSS-wide keywords, which copy its physical one's value.
#[test]
fn var_and_css_wide_keywords() {
    let styles = styles(
        "<style>
           #a { --w: 3px; margin-inline-start: var(--w); margin-block: var(--w) 4px }
           #p { margin-left: 7px; width: 8px }
           #c { margin-inline-start: inherit; inline-size: inherit }
           #d { margin-left: 3px; margin-inline-start: initial }
         </style>
         <div id=a></div><div id=p><div id=c></div><div id=d></div></div>",
    );
    let a = &styles["a"];
    assert_eq!(value(a, "margin-left"), "3px");
    assert_eq!(value(a, "margin-top"), "3px");
    assert_eq!(value(a, "margin-bottom"), "4px");
    assert_eq!(value(&styles["c"], "margin-left"), "7px");
    assert_eq!(value(&styles["c"], "width"), "8px");
    assert_eq!(value(&styles["d"], "margin-left"), "0px");
}

/// CSSOM reads each back as written, the shorthands from their longhands
/// (one value when both are the same, `border-block` only when both sides
/// are), groups them into their shorthands in `cssText`, and names them in
/// camel case.
#[test]
fn cssom_reads_them_back() {
    let read_back = |property: &str, value: &str| {
        let mut block = CssPropertyValueSet::default();
        block.set_property(property, value, "");
        block.property_value(property)
    };
    assert_eq!(read_back("margin-inline-start", "1em"), "1em");
    assert_eq!(read_back("inline-size", "50%"), "50%");
    assert_eq!(read_back("margin-inline", "1px 2px"), "1px 2px");
    assert_eq!(read_back("margin-inline", "auto auto"), "auto");
    assert_eq!(read_back("inset-block", "-1px"), "-1px");
    assert_eq!(read_back("border-inline-color", "red blue"), "red blue");
    assert_eq!(read_back("border-block", "1px solid red"), "1px solid red");
    assert_eq!(read_back("border-inline-end", "dotted"), "dotted");
    assert_eq!(read_back("padding-block", "-1px"), "", "not negative");
    assert_eq!(read_back("margin-inline", "1px 2px 3px"), "", "two at most");

    let mut block = CssPropertyValueSet::default();
    block.set_property("border-block-start", "1px solid red", "");
    block.set_property("border-block-end-color", "blue", "");
    assert_eq!(block.property_value("border-block"), "", "the sides differ");
    assert_eq!(block.property_value("border-block-start"), "1px solid red");
    assert_eq!(
        block.property_value("border-block-width"),
        "",
        "no end width"
    );
    assert_eq!(block.property_value("border-block-color"), "red blue");
    block.set_property("border-block-end", "1px solid", "");
    block.set_property("border-block-end-color", "blue", "");
    assert_eq!(
        block.property_value("border-block"),
        "",
        "the colors differ"
    );
    block.set_property("border-block-end-color", "red", "");
    assert_eq!(block.property_value("border-block"), "1px solid red");

    let css_text = |css: &str| CssPropertyValueSet::parse(css).css_text();
    assert_eq!(
        css_text("margin-inline-start: 1px; margin-inline-end: 2px"),
        "margin-inline: 1px 2px;"
    );
    assert_eq!(
        css_text("border-block-start: 1px solid red; border-block-end: 1px solid red"),
        "border-block: 1px solid red;"
    );
    assert_eq!(
        css_text("margin-left: 1px; margin-inline-start: 2px"),
        "margin-left: 1px; margin-inline-start: 2px;"
    );
    // A logical corner stays as written, beside `border-radius` (Chrome's).
    assert_eq!(read_back("border-start-start-radius", "4px 6px"), "4px 6px");
    assert_eq!(
        read_back("border-end-end-radius", "-1px"),
        "",
        "not negative"
    );
    assert_eq!(
        css_text("border-radius: 3px; border-start-start-radius: 4px"),
        "border-radius: 3px; border-start-start-radius: 4px;"
    );

    assert_eq!(
        style_resolver::css_property_for_attribute("marginInlineStart"),
        Some("margin-inline-start")
    );
    assert_eq!(
        style_resolver::css_property_for_attribute("borderBlock"),
        Some("border-block")
    );
    let attributes = style_resolver::css_property_attributes();
    for name in ["maxBlockSize", "borderInlineEndColor", "insetInline"] {
        assert!(attributes.iter().any(|a| a == name), "{name}");
    }
}

/// `setProperty` puts a declaration that has one of the same group and
/// the other mapping logic after it at the end, so that it wins as the
/// last set (CSSOM's "set a CSS declaration"), even with the same value;
/// one with only others after it stays where it is.
#[test]
fn set_property_moves_past_the_other_mapping() {
    let mut block = CssPropertyValueSet::parse("margin-inline-start: 1px; margin-left: 2px");
    assert!(block.set_property("margin-inline-start", "1px", ""));
    assert_eq!(
        block.css_text(),
        "margin-left: 2px; margin-inline-start: 1px;"
    );
    assert!(
        !block.set_property("margin-inline-start", "1px", ""),
        "now last"
    );
    assert!(block.set_property("margin-left", "3px", ""));
    assert_eq!(
        block.css_text(),
        "margin-inline-start: 1px; margin-left: 3px;"
    );
    // A shorthand's longhands each.
    assert!(block.set_property("margin-inline", "4px", ""));
    assert_eq!(block.css_text(), "margin-left: 3px; margin-inline: 4px;");

    // Another group, or the same mapping, after it: in place.
    let mut block = CssPropertyValueSet::parse("margin-inline-start: 1px; padding-left: 2px");
    block.set_property("margin-inline-start", "3px", "");
    assert_eq!(
        block.css_text(),
        "margin-inline-start: 3px; padding-left: 2px;"
    );
    let mut block = CssPropertyValueSet::parse("width: 1px; height: 2px; min-inline-size: 3px");
    block.set_property("width", "4px", "");
    assert_eq!(
        block.css_text(),
        "width: 4px; height: 2px; min-inline-size: 3px;"
    );
    let mut block = CssPropertyValueSet::parse("inline-size: 1px; height: 2px");
    block.set_property("inline-size", "4px", "");
    assert_eq!(block.css_text(), "height: 2px; inline-size: 4px;");
    // The corners are a group too.
    let mut block =
        CssPropertyValueSet::parse("border-start-end-radius: 1px; border-top-left-radius: 2px");
    block.set_property("border-start-end-radius", "3px", "");
    assert_eq!(
        block.css_text(),
        "border-top-left-radius: 2px; border-start-end-radius: 3px;"
    );
}

/// `getComputedStyle` lists them, and resolves each as its physical one,
/// which may need the box.
#[test]
fn computed_style_lists_and_resolves_them() {
    let names = style_resolver::computable_properties();
    for name in [
        "block-size",
        "margin-inline-end",
        "border-block-start-color",
    ] {
        assert!(names.contains(&name), "{name}");
    }
    assert_eq!(
        style_resolver::physical_property("Margin-Inline-Start"),
        Some("margin-left")
    );
    assert_eq!(
        style_resolver::physical_property("max-block-size"),
        Some("max-height")
    );
    assert_eq!(style_resolver::physical_property("margin-left"), None);
    assert_eq!(
        style_resolver::physical_property("margin-inline"),
        None,
        "a shorthand"
    );
    assert!(style_resolver::is_layout_dependent("inline-size"));
    assert!(style_resolver::is_layout_dependent("padding-block-end"));
    assert!(!style_resolver::is_layout_dependent(
        "border-inline-start-width"
    ));
}
