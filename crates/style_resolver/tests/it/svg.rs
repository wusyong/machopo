//! The SVG properties as declared: what they read back as (Chrome 152's
//! `element.style`).

use style_resolver::CssPropertyValueSet;

#[test]
fn declared_values_read_back_as_written() {
    let block = CssPropertyValueSet::parse(
        "fill: URL(#a) Red; stroke-dasharray: 5 3; stroke-width: 2; cx: 5; \
         d: path('m5,5l1,1'); visibility: Hidden; stroke-linejoin: miter-clip; \
         stroke: none; rx: auto; fill-rule: evenodd; stroke-linecap: round",
    );
    assert_eq!(
        block.css_text(),
        "fill: url(\"#a\") red; stroke-dasharray: 5, 3; stroke-width: 2; cx: 5; \
         d: path(\"m 5 5 l 1 1\"); visibility: hidden; stroke: none; rx: auto; \
         fill-rule: evenodd; stroke-linecap: round;"
    );
}
