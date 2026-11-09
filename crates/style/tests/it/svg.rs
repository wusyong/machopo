//! The SVG properties: their values in CSS, what they compute to, and the
//! presentation attributes that set them. The expected text is Chrome
//! 152's (`getComputedStyle` and `element.style`).

use std::sync::Arc;

use style_resolver::computed_value;

use crate::{Page, styles};

/// `property`'s computed value on each of `elements` (markup inside an
/// `<svg>`, each with an `id` of its index).
fn computed(elements: &[String], property: &str) -> Vec<String> {
    let markup: String = elements
        .iter()
        .enumerate()
        .map(|(i, element)| element.replacen(' ', &format!(" id=e{i} "), 1))
        .collect();
    let styles = styles(&format!("<svg>{markup}</svg>"));
    (0..elements.len())
        .map(|i| {
            let style = &styles[&format!("e{i}")];
            computed_value(style, property).expect("a supported property")
        })
        .collect()
}

/// `property: value` in a `style` attribute on `tag`, computed.
#[track_caller]
fn assert_css(tag: &str, property: &str, cases: &[(&str, &str)]) {
    let elements: Vec<String> = cases
        .iter()
        .map(|(value, _)| format!("<{tag} style='{property}: {value}'></{tag}>"))
        .collect();
    let expected: Vec<&str> = cases.iter().map(|(_, expected)| *expected).collect();
    assert_eq!(computed(&elements, property), expected, "{property}");
}

/// `attribute="value"` on `tag`, computed as `property`.
#[track_caller]
fn assert_attribute(tag: &str, attribute: &str, property: &str, cases: &[(&str, &str)]) {
    let elements: Vec<String> = cases
        .iter()
        .map(|(value, _)| format!("<{tag} {attribute}='{value}'></{tag}>"))
        .collect();
    let expected: Vec<&str> = cases.iter().map(|(_, expected)| *expected).collect();
    assert_eq!(
        computed(&elements, property),
        expected,
        "{attribute} on {tag}"
    );
}

#[test]
fn initial_values() {
    let cases = [
        ("fill", "rgb(0, 0, 0)"),
        ("stroke", "none"),
        ("fill-opacity", "1"),
        ("stroke-opacity", "1"),
        ("fill-rule", "nonzero"),
        ("stroke-width", "1px"),
        ("stroke-linecap", "butt"),
        ("stroke-linejoin", "miter"),
        ("stroke-miterlimit", "4"),
        ("stroke-dasharray", "none"),
        ("stroke-dashoffset", "0px"),
        ("text-anchor", "start"),
        ("stop-color", "rgb(0, 0, 0)"),
        ("stop-opacity", "1"),
        ("cx", "0px"),
        ("cy", "0px"),
        ("r", "0px"),
        ("rx", "auto"),
        ("ry", "auto"),
        ("x", "0px"),
        ("y", "0px"),
        ("d", "none"),
        ("visibility", "visible"),
    ];
    for (property, expected) in cases {
        assert_eq!(computed(&["<rect ></rect>".into()], property), [expected]);
    }
}

#[test]
fn paints() {
    assert_css(
        "rect",
        "fill",
        &[
            ("red", "rgb(255, 0, 0)"),
            ("none", "none"),
            ("currentcolor", "rgb(0, 0, 0)"),
            ("url(#a)", "url(\"#a\")"),
            ("url(#a) none", "url(\"#a\") none"),
            ("url(\"#a\") blue", "url(\"#a\") rgb(0, 0, 255)"),
            ("transparent", "rgba(0, 0, 0, 0)"),
            // Not machopo's: Chrome's `context-fill`, for markers.
            ("context-fill", "rgb(0, 0, 0)"),
            ("red blue", "rgb(0, 0, 0)"),
        ],
    );
    // `currentcolor` is the element's color, a fallback's too.
    assert_eq!(
        computed(
            &["<rect style='color: lime; stroke: url(#g) currentcolor'></rect>".into()],
            "stroke"
        ),
        ["url(\"#g\") rgb(0, 255, 0)"]
    );
}

#[test]
fn opacities_and_rules() {
    assert_css(
        "rect",
        "fill-opacity",
        &[("0.5", "0.5"), ("50%", "0.5"), ("2", "1"), ("-1", "0")],
    );
    assert_css("rect", "stroke-opacity", &[("0.25", "0.25")]);
    assert_css(
        "rect",
        "fill-rule",
        &[
            ("evenodd", "evenodd"),
            ("nonzero", "nonzero"),
            ("odd", "nonzero"),
        ],
    );
}

#[test]
fn stroke_properties() {
    assert_css(
        "rect",
        "stroke-width",
        &[
            ("2", "2px"),
            ("2px", "2px"),
            ("10%", "10%"),
            ("1em", "16px"),
            ("-1px", "1px"),
        ],
    );
    assert_css(
        "rect",
        "stroke-miterlimit",
        &[("0", "0"), ("0.5", "0.5"), ("10", "10"), ("-1", "4")],
    );
    assert_css(
        "rect",
        "stroke-dasharray",
        &[
            ("none", "none"),
            ("5", "5px"),
            ("5 3", "5px, 3px"),
            ("5,3", "5px, 3px"),
            ("5 , 3 ,1em", "5px, 3px, 16px"),
            ("5px, 10%", "5px, 10%"),
            ("0", "0px"),
            ("-1", "none"),
            ("5, -1", "none"),
            ("5,,3", "none"),
            ("5,", "none"),
        ],
    );
    assert_css(
        "rect",
        "stroke-dashoffset",
        &[("3", "3px"), ("-3px", "-3px"), ("10%", "10%")],
    );
    assert_css(
        "rect",
        "stroke-linecap",
        &[("round", "round"), ("square", "square"), ("butt", "butt")],
    );
    assert_css(
        "rect",
        "stroke-linejoin",
        &[
            ("round", "round"),
            ("bevel", "bevel"),
            // SVG 2's, not Chrome's.
            ("miter-clip", "miter"),
            ("arcs", "miter"),
        ],
    );
}

#[test]
fn geometry_properties() {
    assert_css(
        "circle",
        "cx",
        &[
            ("5", "5px"),
            ("5px", "5px"),
            ("10%", "10%"),
            ("-3px", "-3px"),
            ("1em", "16px"),
        ],
    );
    assert_css("circle", "cy", &[("-2", "-2px")]);
    assert_css("rect", "x", &[("7", "7px")]);
    assert_css("rect", "y", &[("-7%", "-7%")]);
    assert_css(
        "circle",
        "r",
        &[("5", "5px"), ("10%", "10%"), ("-1", "0px"), ("auto", "0px")],
    );
    assert_css(
        "ellipse",
        "rx",
        &[("auto", "auto"), ("5px", "5px"), ("-1px", "auto")],
    );
    assert_css("ellipse", "ry", &[("3", "3px")]);
}

#[test]
fn path_data_is_computed_absolute() {
    assert_css(
        "path",
        "d",
        &[
            ("none", "none"),
            ("path(\"M 0 0 L 10 10\")", "path(\"M 0 0 L 10 10\")"),
            (
                "path(\"m5,5l10,10h5v5z\")",
                "path(\"M 5 5 L 15 15 H 20 V 20 Z\")",
            ),
            ("path(\"M0 0 10 10\")", "path(\"M 0 0 L 10 10\")"),
            ("path(\"M1.5.5L2e1-3\")", "path(\"M 1.5 0.5 L 20 -3\")"),
            (
                "path(\"M 0 0 C 1 2 3 4 5 6 S 7 8 9 10 Q 1 1 2 2 T 3 3\")",
                "path(\"M 0 0 C 1 2 3 4 5 6 S 7 8 9 10 Q 1 1 2 2 T 3 3\")",
            ),
            (
                "path(\"M0 0 a5 5 30 01 10 10\")",
                "path(\"M 0 0 A 5 5 30 0 1 10 10\")",
            ),
            // After a close, from the subpath's start.
            (
                "path(\"M 2 2 L 5 5 Z l 1 1\")",
                "path(\"M 2 2 L 5 5 Z L 3 3\")",
            ),
            // Invalid as a whole in CSS.
            ("path(\"\")", "none"),
            ("path(\"M 0 0 L\")", "none"),
            ("path(\"L 1 1\")", "none"),
            ("path(evenodd, \"M 0 0 L 1 1\")", "none"),
        ],
    );
}

#[test]
fn visibility() {
    assert_css(
        "rect",
        "visibility",
        &[
            ("hidden", "hidden"),
            ("collapse", "collapse"),
            ("visible", "visible"),
        ],
    );
    // HTML's too, inherited.
    let styles = styles("<div style='visibility: hidden'><p id=p></p></div>");
    assert_eq!(
        computed_value(&styles["p"], "visibility").unwrap(),
        "hidden"
    );
}

#[test]
fn pointer_events() {
    assert_css(
        "rect",
        "pointer-events",
        &[
            ("auto", "auto"),
            ("none", "none"),
            ("visiblePainted", "visiblepainted"),
            ("VISIBLEFILL", "visiblefill"),
            ("visibleStroke", "visiblestroke"),
            ("visible", "visible"),
            ("painted", "painted"),
            ("fill", "fill"),
            ("stroke", "stroke"),
            ("all", "all"),
            ("bounding-box", "bounding-box"),
            ("bogus", "auto"),
        ],
    );
    assert_attribute(
        "rect",
        "pointer-events",
        "pointer-events",
        &[("visibleFill", "visiblefill")],
    );
    // Inherited, by HTML too.
    let styles = styles("<div style='pointer-events: none'><p id=p></p></div>");
    assert_eq!(
        computed_value(&styles["p"], "pointer-events").unwrap(),
        "none"
    );
}

/// A gradient's stops' colors: a color (`currentcolor` the stop's
/// `color`) and an opacity (a percentage of 1, clamped when computed),
/// properties and presentation attributes, not inherited.
#[test]
fn stop_colors() {
    assert_css(
        "stop",
        "stop-color",
        &[
            ("red", "rgb(255, 0, 0)"),
            ("rgba(0, 0, 255, 0.5)", "rgba(0, 0, 255, 0.5)"),
            ("currentcolor", "rgb(0, 0, 0)"),
            ("none", "rgb(0, 0, 0)"),
        ],
    );
    assert_css(
        "stop",
        "stop-opacity",
        &[
            ("0.5", "0.5"),
            ("25%", "0.25"),
            ("2", "1"),
            ("-1", "0"),
            ("auto", "1"),
        ],
    );
    assert_attribute(
        "stop",
        "stop-color",
        "stop-color",
        &[("lime", "rgb(0, 255, 0)")],
    );
    assert_attribute(
        "stop",
        "stop-opacity",
        "stop-opacity",
        &[("0.25", "0.25"), ("x", "1")],
    );
    let styles = styles(
        "<svg><linearGradient style='stop-color: red; stop-opacity: 0.5'>\
         <stop id=plain></stop><stop id=current style='color: lime; stop-color: currentcolor'></stop>\
         </linearGradient></svg>",
    );
    let value = |id: &str, property| computed_value(&styles[id], property).unwrap();
    assert_eq!(
        value("plain", "stop-color"),
        "rgb(0, 0, 0)",
        "not inherited"
    );
    assert_eq!(value("plain", "stop-opacity"), "1");
    assert_eq!(value("current", "stop-color"), "rgb(0, 255, 0)");
}

/// `text-anchor`: a keyword, inherited, a presentation attribute.
#[test]
fn text_anchor() {
    assert_css(
        "text",
        "text-anchor",
        &[("middle", "middle"), ("END", "end"), ("left", "start")],
    );
    assert_attribute(
        "tspan",
        "text-anchor",
        "text-anchor",
        &[("end", "end"), ("x", "start")],
    );
    let styles = styles("<svg><g text-anchor=middle><text id=t></text></g></svg>");
    assert_eq!(
        computed_value(&styles["t"], "text-anchor").unwrap(),
        "middle"
    );
}

#[test]
fn painting_inherits_and_geometry_doesnt() {
    let styles = styles(
        "<svg><g style='fill: red; stroke-width: 3; stroke-dasharray: 1 2; cx: 5'>\
         <circle id=c></circle></g></svg>",
    );
    let value = |property| computed_value(&styles["c"], property).unwrap();
    assert_eq!(value("fill"), "rgb(255, 0, 0)");
    assert_eq!(value("stroke-width"), "3px");
    assert_eq!(value("stroke-dasharray"), "1px, 2px");
    assert_eq!(value("cx"), "0px");
}

#[test]
fn css_wide_keywords() {
    let styles = styles(
        "<svg><g style='fill: red; stroke: blue; stroke-width: 3; cx: 5; d: path(\"M 1 1\"); \
         visibility: hidden; fill-rule: evenodd'>\
         <path id=p style='fill: initial; stroke: unset; stroke-width: initial; cx: inherit; \
         d: inherit; visibility: initial; fill-rule: inherit'></path></g></svg>",
    );
    let value = |property| computed_value(&styles["p"], property).unwrap();
    assert_eq!(value("fill"), "rgb(0, 0, 0)");
    assert_eq!(
        value("stroke"),
        "rgb(0, 0, 255)",
        "inherited, so unset inherits"
    );
    assert_eq!(value("stroke-width"), "1px");
    assert_eq!(value("cx"), "5px");
    assert_eq!(value("d"), "path(\"M 1 1\")");
    assert_eq!(value("visibility"), "visible");
    assert_eq!(value("fill-rule"), "evenodd");
    // `unset` inherits each painting property.
    let unset =
        self::styles("<svg><g style='fill: red'><path id=p style='fill: unset'></path></g></svg>");
    assert_eq!(
        computed_value(&unset["p"], "fill").unwrap(),
        "rgb(255, 0, 0)"
    );
}

#[test]
fn presentation_attributes_take_plain_numbers() {
    assert_attribute(
        "rect",
        "fill",
        "fill",
        &[
            ("red", "rgb(255, 0, 0)"),
            ("url(#a)", "url(\"#a\")"),
            ("bogus", "rgb(0, 0, 0)"),
            ("red !important", "rgb(0, 0, 0)"),
        ],
    );
    assert_attribute(
        "rect",
        "stroke-width",
        "stroke-width",
        &[
            ("2", "2px"),
            (" 2 ", "2px"),
            ("5%", "5%"),
            ("2em", "32px"),
            ("-1", "1px"),
            ("bogus", "1px"),
        ],
    );
    assert_attribute(
        "rect",
        "stroke-dasharray",
        "stroke-dasharray",
        &[("5 3", "5px, 3px"), ("5px 3", "5px, 3px")],
    );
    assert_attribute("rect", "fill-opacity", "fill-opacity", &[("50%", "0.5")]);
    assert_attribute("rect", "visibility", "visibility", &[("hidden", "hidden")]);
    assert_attribute("rect", "opacity", "opacity", &[("0.5", "0.5")]);
    // A number is px even where CSS takes none.
    assert_attribute("rect", "font-size", "font-size", &[("20", "20px")]);
    assert_attribute("rect", "width", "width", &[("5", "5px"), ("50%", "50%")]);
    assert_attribute(
        "circle",
        "cx",
        "cx",
        &[("5", "5px"), ("10%", "10%"), ("5 6", "0px")],
    );
    assert_attribute("circle", "r", "r", &[("5", "5px")]);
    assert_attribute("ellipse", "rx", "rx", &[("5", "5px")]);
}

#[test]
fn geometry_attributes_are_on_their_elements_only() {
    let geometry = [
        ("cx", "cx", &["circle", "ellipse"][..], &["rect", "g"][..]),
        ("r", "r", &["circle"], &["ellipse", "g"]),
        ("rx", "rx", &["rect", "ellipse"], &["circle", "g"]),
        (
            "x",
            "x",
            &[
                "rect",
                "svg",
                "image",
                "foreignObject",
                "use",
                "symbol",
                "mask",
            ],
            &["circle", "text", "pattern", "filter", "g"],
        ),
        (
            "width",
            "width",
            &["rect", "svg", "image", "foreignObject", "symbol", "mask"],
            &["use", "g"],
        ),
    ];
    for (attribute, property, on, off) in geometry {
        for tag in on {
            assert_attribute(tag, attribute, property, &[("7", "7px")]);
        }
        for tag in off {
            let initial = if property == "rx" || property == "width" {
                "auto"
            } else {
                "0px"
            };
            assert_attribute(tag, attribute, property, &[("7", initial)]);
        }
    }
    assert_attribute(
        "path",
        "d",
        "d",
        &[("M 0 0 L 1 1", "path(\"M 0 0 L 1 1\")")],
    );
    assert_attribute("rect", "d", "d", &[("M 0 0 L 1 1", "none")]);
    // The animation elements' `fill` is their own.
    assert_attribute("animate", "fill", "fill", &[("blue", "rgb(0, 0, 0)")]);
    assert_attribute("set", "fill", "fill", &[("blue", "rgb(0, 0, 0)")]);
    assert_attribute("g", "fill", "fill", &[("blue", "rgb(0, 0, 255)")]);
}

#[test]
fn the_d_attribute_is_drawn_up_to_its_first_error() {
    assert_attribute(
        "path",
        "d",
        "d",
        &[
            ("m0,0l10,10z", "path(\"M 0 0 L 10 10 Z\")"),
            ("M 0 0 L 10 10 bogus", "path(\"M 0 0 L 10 10\")"),
            ("bogus", "none"),
            ("", "none"),
        ],
    );
}

#[test]
fn transform_attributes() {
    assert_attribute(
        "rect",
        "transform",
        "transform",
        &[
            ("translate(10 20)", "matrix(1, 0, 0, 1, 10, 20)"),
            ("rotate(90 5 5)", "matrix(0, 1, -1, 0, 10, 0)"),
            ("scale(2)", "matrix(2, 0, 0, 2, 0, 0)"),
            ("translate(10,20) scale(2)", "matrix(2, 0, 0, 2, 10, 20)"),
            ("", "none"),
            ("matrix(1 0 0 1 3 4)", "matrix(1, 0, 0, 1, 3, 4)"),
            ("skewX(45)", "matrix(1, 0, 1, 1, 0, 0)"),
            ("bogus", "none"),
            ("scale(2) bogus", "none"),
        ],
    );
    let scale = [("scale(2)", "matrix(2, 0, 0, 2, 0, 0)")];
    for tag in ["g", "svg", "symbol", "use", "clipPath", "defs", "text"] {
        assert_attribute(tag, "transform", "transform", &scale);
    }
    for tag in ["linearGradient", "pattern", "tspan", "marker", "stop"] {
        assert_attribute(tag, "transform", "transform", &[("scale(2)", "none")]);
    }
    assert_attribute("linearGradient", "gradientTransform", "transform", &scale);
    assert_attribute("radialGradient", "gradientTransform", "transform", &scale);
    assert_attribute("pattern", "patternTransform", "transform", &scale);
    assert_attribute(
        "g",
        "gradientTransform",
        "transform",
        &[("scale(2)", "none")],
    );
    assert_attribute(
        "g",
        "patternTransform",
        "transform",
        &[("scale(2)", "none")],
    );
}

#[test]
fn presentation_attributes_lose_to_css_and_are_svgs_only() {
    let styles = styles(
        "<style>#b { fill: lime }</style>\
         <svg><rect id=a fill=red></rect><rect id=b fill=red></rect>\
         <rect id=c fill=red style='fill: blue'></rect>\
         <rect id=d FILL=red></rect></svg>\
         <div id=e fill=red stroke-width=3></div>",
    );
    let fill = |id: &str| computed_value(&styles[id], "fill").unwrap();
    assert_eq!(fill("a"), "rgb(255, 0, 0)");
    assert_eq!(fill("b"), "rgb(0, 255, 0)");
    assert_eq!(fill("c"), "rgb(0, 0, 255)");
    // The HTML parser lowercases it on an SVG element: `fill`.
    assert_eq!(fill("d"), "rgb(255, 0, 0)");
    assert_eq!(fill("e"), "rgb(0, 0, 0)");
    assert_eq!(computed_value(&styles["e"], "stroke-width").unwrap(), "1px");
}

/// An SVG element's presentation attributes are parsed once and kept on
/// it: another value of one is parsed again, another `class` keeps them;
/// elements with the same attribute and value share its parse, but for a
/// path's `d`. (In a debug build what is kept is also parsed again and
/// compared, in every test.)
#[test]
fn presentation_attributes_are_kept_until_they_change() {
    let mut page = Page::new(
        "<svg><rect id=a fill=red width=10></rect>\
         <path id=b fill=red d='M0 0L10 10'></path></svg>",
    );
    let value = |page: &mut Page, id: &str, property: &str| {
        computed_value(&page.style(id), property).unwrap()
    };
    let kept = |page: &mut Page, id: &str| {
        let Page {
            dom,
            document,
            token,
            ..
        } = page;
        dom.read(|cx| {
            let element = crate::find(cx, document.get(cx.gc), id).unwrap();
            cx.element_style(element, token)
                .unwrap()
                .presentation
                .clone()
                .unwrap()
                .0
        })
    };
    assert_eq!(value(&mut page, "a", "fill"), "rgb(255, 0, 0)");
    // `fill="red"` once for both, `width="10"`; not `d`.
    assert_eq!(page.engine.presentation_attributes_for_testing(), 2);

    let before = kept(&mut page, "a");
    page.with("a", |cx, a| cx.set_attribute(a, "class", "x"));
    page.recalc();
    assert!(Arc::ptr_eq(&before, &kept(&mut page, "a")), "kept");

    page.with("a", |cx, a| cx.set_attribute(a, "fill", "blue"));
    page.with("b", |cx, b| cx.set_attribute(b, "d", "M0 0L20 20"));
    page.recalc();
    assert_eq!(value(&mut page, "a", "fill"), "rgb(0, 0, 255)");
    assert_eq!(value(&mut page, "b", "d"), "path(\"M 0 0 L 20 20\")");
    assert_eq!(page.engine.presentation_attributes_for_testing(), 3);
}

/// The presentation attribute cache starts again once it keeps 4096
/// values.
#[test]
fn the_presentation_attribute_cache_starts_again() {
    let rects: String = (0..4097)
        .map(|i| format!("<rect width={i}></rect>"))
        .collect();
    let page = Page::new(&format!("<svg>{rects}</svg>"));
    assert_eq!(page.engine.presentation_attributes_for_testing(), 1);
}
