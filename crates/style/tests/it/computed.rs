//! Computed values as `getComputedStyle` reads them: what each property
//! writes, and the list the declaration lists.

use computed_style::ComputedStyle;
use style_resolver::{computable_properties, computed_value, shorthand_longhands, shorthand_value};

use crate::styles;

/// What `value(html, id, property)` writes for one element.
fn value(html: &str, id: &str, property: &str) -> String {
    let styles = styles(html);
    let style = styles.get(id).expect("the element has a style");
    computed_value(style, property).expect("a supported property")
}

/// Every longhand writes something: a property added to the set without a
/// computed value here fails this.
#[test]
fn every_longhand_has_a_computed_value() {
    let style = ComputedStyle::initial();
    for property in computable_properties() {
        let written = computed_value(&style, property);
        assert!(written.is_some(), "{property} has no computed value");
        assert!(
            !written.as_deref().unwrap_or_default().is_empty(),
            "{property}'s computed value is empty"
        );
    }
    assert_eq!(
        computed_value(&style, "mix-blend-mode"),
        None,
        "not supported"
    );
    assert_eq!(computed_value(&style, "widht"), None, "not a property name");
    assert_eq!(
        computed_value(&style, "WIDTH").as_deref(),
        Some("auto"),
        "property names are ASCII case-insensitive, as they are in Chrome"
    );
}

/// The list is every longhand, in name order with the prefixed ones last
/// (Blink keeps its `ComputableProperties` so), with no shorthand in it.
#[test]
fn the_list_is_the_longhands_in_name_order() {
    let names = computable_properties();
    let first_prefixed = names
        .iter()
        .position(|name| name.starts_with('-'))
        .unwrap_or(names.len());
    let (standard, prefixed) = names.split_at(first_prefixed);
    assert!(
        prefixed.iter().all(|name| name.starts_with('-')),
        "prefixed last"
    );
    for part in [standard, prefixed] {
        let mut sorted = part.to_vec();
        sorted.sort_unstable();
        assert_eq!(part, &sorted[..], "in name order");
    }
    assert!(prefixed.contains(&"-webkit-text-security"));
    assert!(names.contains(&"background-color"));
    assert!(names.contains(&"grid-template-columns"));
    assert!(!names.contains(&"margin"), "no shorthand");
    assert!(!names.contains(&"flex"), "no shorthand");
}

/// Lengths are absolute, percentages stay percentages, and keywords stay
/// keywords (the computed value, not the used one).
#[test]
fn lengths_are_absolute_and_percentages_stay() {
    let html = r#"<style>
      #a { font-size: 20px; width: 10em; height: 50%; padding: 1px 2% 3pt 0;
           margin-left: auto; max-width: none; min-height: 2em }
    </style><p id=a>"#;
    let styles = styles(html);
    let style = styles.get("a").expect("styled");
    let v = |property: &str| computed_value(style, property).expect("supported");
    assert_eq!(v("font-size"), "20px");
    assert_eq!(v("width"), "200px", "em against this element's font size");
    assert_eq!(v("height"), "50%");
    assert_eq!(v("padding-top"), "1px");
    assert_eq!(v("padding-right"), "2%");
    assert_eq!(v("padding-bottom"), "4px", "3pt is 4px");
    assert_eq!(v("padding-left"), "0px");
    assert_eq!(v("margin-left"), "auto");
    assert_eq!(v("max-width"), "none");
    assert_eq!(v("min-height"), "40px");
}

/// Colors resolve to `rgb()` / `rgba()`, and `currentcolor` to the
/// element's own `color`, as Chrome writes them.
#[test]
fn colors_are_resolved() {
    let html = r#"<style>
      #a { color: red; background-color: rgba(0, 128, 0, 0.5);
           border: 1px solid }
    </style><p id=a>"#;
    assert_eq!(value(html, "a", "color"), "rgb(255, 0, 0)");
    assert_eq!(
        value(html, "a", "background-color"),
        "rgba(0, 128, 0, 0.5)",
        "the shortest alpha that comes back to the same byte"
    );
    assert_eq!(
        value(html, "a", "border-top-color"),
        "rgb(255, 0, 0)",
        "currentcolor is the element's color"
    );
}

/// A number resolves against the font size, as Chrome's does; `normal`
/// stays `normal`.
#[test]
fn line_height_resolves_a_number() {
    let html = r#"<style>
      #a { font-size: 16px; line-height: 1.5 }
      #b { font-size: 16px; line-height: 24px }
      #c { font-size: 16px }
    </style><p id=a><p id=b><p id=c>"#;
    assert_eq!(value(html, "a", "line-height"), "24px");
    assert_eq!(value(html, "b", "line-height"), "24px");
    assert_eq!(value(html, "c", "line-height"), "normal");
}

/// The values that aren't lengths: keywords, numbers, lists and the
/// grid's track lists.
#[test]
fn keywords_lists_and_tracks() {
    let html = r#"<style>
      #a { display: flex; flex: 2 1 30%; order: -1; z-index: 3;
           align-self: center; overflow: hidden; opacity: .5;
           font-family: "Fira Sans", monospace; font-weight: bold;
           text-decoration-line: underline overline; white-space: pre }
      #b { display: grid; grid-template-columns: repeat(2, [a] 10px 1fr);
           grid-auto-rows: minmax(10px, auto); grid-column: 2 / span 3;
           transform: translate(10px, 50%) scale(2) }
      #c { }
    </style><div id=a></div><div id=b></div><div id=c></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "display"), "flex");
    assert_eq!(v("a", "flex-grow"), "2");
    assert_eq!(v("a", "flex-shrink"), "1");
    assert_eq!(v("a", "flex-basis"), "30%");
    assert_eq!(v("a", "order"), "-1");
    assert_eq!(v("a", "z-index"), "3");
    assert_eq!(v("a", "align-self"), "center");
    assert_eq!(v("a", "overflow-x"), "hidden");
    assert_eq!(v("a", "opacity"), "0.5");
    assert_eq!(
        v("a", "font-family"),
        "Fira Sans, monospace",
        "quoted only where unquoted would read back as something else"
    );
    assert_eq!(v("a", "font-weight"), "700");
    assert_eq!(v("a", "text-decoration-line"), "underline overline");
    assert_eq!(v("a", "white-space-collapse"), "preserve");
    assert_eq!(v("a", "text-wrap-mode"), "nowrap");
    assert_eq!(v("b", "grid-template-columns"), "repeat(2, [a] 10px 1fr)");
    assert_eq!(v("b", "grid-auto-rows"), "minmax(10px, auto)");
    assert_eq!(v("b", "grid-column-start"), "2");
    assert_eq!(v("b", "grid-column-end"), "span 3");
    assert_eq!(
        v("b", "transform"),
        "translate(10px, 50%) scale(2, 2)",
        "the computed functions; Chrome's resolved value is a matrix, which needs the box"
    );
    assert_eq!(v("c", "z-index"), "auto");
    assert_eq!(v("c", "align-self"), "auto");
    assert_eq!(v("c", "transform"), "none");
    assert_eq!(v("c", "grid-template-columns"), "none");
}

/// `transform-origin` computes its keywords to percentages and its
/// lengths to px; an SVG element's starts at its viewport's origin, but
/// for an outermost `<svg>`'s, a CSS box's (the UA stylesheet, as
/// Chrome's `svg.css`).
#[test]
fn transform_origins_and_boxes() {
    let html = r#"<style>
      #a { transform-origin: right 2em 3px; font-size: 10px; transform-box: content-box }
      #f { transform-origin: 1px 2px }
    </style><div id=a></div><div id=b></div>
    <svg id=svg><g id=g></g><svg id=nested></svg>
      <rect id=attr transform-origin="75 center" /><rect id=attr2 transform-origin="center -7.5" />
      <foreignObject><svg id=inner></svg></foreignObject></svg>
    <svg id=f></svg>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "transform-origin"), "100% 20px 3px");
    assert_eq!(v("a", "transform-box"), "content-box");
    assert_eq!(v("b", "transform-origin"), "50% 50%");
    assert_eq!(v("b", "transform-box"), "view-box");
    assert_eq!(v("svg", "transform-origin"), "50% 50%");
    assert_eq!(v("g", "transform-origin"), "0px 0px");
    assert_eq!(v("nested", "transform-origin"), "0px 0px");
    assert_eq!(v("inner", "transform-origin"), "50% 50%");
    // The presentation attribute, each plain number a length in user
    // units.
    assert_eq!(v("attr", "transform-origin"), "75px 50%");
    assert_eq!(v("attr2", "transform-origin"), "50% -7.5px");
    // An author's rule wins over the UA stylesheet's.
    assert_eq!(v("f", "transform-origin"), "1px 2px");
}

/// `object-fit` is its keyword; `object-position` its two axes' offsets,
/// one from the end side as `calc(100% - …)` (as Chrome writes them).
#[test]
fn object_fits_and_positions() {
    let html = r#"<style>
      #a { object-fit: none; object-position: right 20px bottom 10% }
    </style><img id=a><img id=b>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "object-fit"), "none");
    assert_eq!(v("a", "object-position"), "calc(100% - 20px) 90%");
    assert_eq!(v("b", "object-fit"), "fill");
    assert_eq!(v("b", "object-position"), "50% 50%");
    assert_eq!(v("b", "overflow-x"), "clip");
}

/// The `-webkit-box` longhands: their initial values, not inherited.
/// `-webkit-box` and `-webkit-inline-box` stay themselves but with a
/// vertical `-webkit-line-clamp`: then `flow-root` and `inline-block`
/// (blockified first), and only those clamp (Blink's
/// `IsSpecifiedDisplayWebkitBox`). A `-webkit-box`'s children aren't
/// blockified, but their `z-index` applies; a flex container's are both.
#[test]
fn webkit_boxes() {
    let html = r#"<div id=a style="-webkit-box-pack: end; -webkit-box-flex: 2"><span id=b></span></div>
      <div id=c style="display: -webkit-box; -webkit-line-clamp: 2"><span id=d style="z-index: 3"></span></div>
      <div id=e style="display: -webkit-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2"></div>
      <div id=f style="display: -webkit-inline-box; -webkit-box-orient: vertical; -webkit-line-clamp: 2"></div>
      <div id=g style="display: -webkit-inline-box; float: left; -webkit-box-orient: vertical; -webkit-line-clamp: 2"></div>
      <div id=h style="display: flow-root; -webkit-box-orient: vertical; -webkit-line-clamp: 2"></div>
      <div style="display: flex"><span id=i style="z-index: 3"></span></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "-webkit-box-orient"), "horizontal");
    assert_eq!(v("a", "-webkit-box-direction"), "normal");
    assert_eq!(v("a", "-webkit-box-align"), "stretch");
    assert_eq!(v("a", "-webkit-box-ordinal-group"), "1");
    assert_eq!(v("a", "-webkit-line-clamp"), "none");
    assert_eq!(v("a", "-webkit-box-pack"), "end");
    assert_eq!(v("b", "-webkit-box-pack"), "start");
    assert_eq!(v("a", "-webkit-box-flex"), "2");
    assert_eq!(v("b", "-webkit-box-flex"), "0");
    assert_eq!(v("c", "display"), "-webkit-box");
    assert_eq!(v("e", "display"), "flow-root");
    assert_eq!(v("f", "display"), "inline-block");
    assert_eq!(v("g", "display"), "flow-root");
    assert_eq!(v("h", "display"), "flow-root");
    let styles = styles(html);
    let clamp = |id: &str| styles[id].line_clamp();
    assert_eq!(
        ["c", "e", "f", "g", "h"].map(clamp),
        [None, Some(2), Some(2), Some(2), None]
    );
    assert_eq!(v("d", "display"), "inline");
    assert_eq!(styles["d"].effective_z_index, Some(3));
    assert_eq!(v("i", "display"), "block");
    assert_eq!(styles["i"].effective_z_index, Some(3));
}

/// The text decorations' properties: lengths in px, percentages kept (of
/// the font size, which they are used with); `text-shadow`'s color
/// resolved, its blur written; the `text-underline-*`, `text-shadow` and
/// `-webkit-text-fill-color` inherited, the `text-decoration-*` not.
#[test]
fn text_decorations() {
    let html = r#"<div id=a style="font-size: 20px; color: blue; text-decoration: overline wavy 1em red;
           text-underline-offset: 0.5em; text-underline-position: under left;
           text-shadow: 1em 2px, red 0 0 3px; -webkit-text-fill-color: lime">
      <span id=b style="text-decoration-thickness: 10%; text-underline-offset: calc(10% + 1em)"></span>
      <span id=c style="text-decoration-color: unset; text-underline-position: unset; text-shadow: unset;
           -webkit-text-fill-color: unset"></span></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "text-decoration-line"), "overline");
    assert_eq!(v("a", "text-decoration-style"), "wavy");
    assert_eq!(v("a", "text-decoration-color"), "rgb(255, 0, 0)");
    assert_eq!(v("a", "text-decoration-thickness"), "20px");
    assert_eq!(v("a", "text-underline-offset"), "10px");
    assert_eq!(v("a", "text-underline-position"), "under left");
    assert_eq!(
        v("a", "text-shadow"),
        "rgb(0, 0, 255) 20px 2px 0px, rgb(255, 0, 0) 0px 0px 3px"
    );
    assert_eq!(v("a", "-webkit-text-fill-color"), "rgb(0, 255, 0)");
    assert_eq!(v("b", "text-decoration-line"), "none");
    assert_eq!(v("b", "text-decoration-style"), "solid");
    assert_eq!(v("b", "text-decoration-color"), "rgb(0, 0, 255)");
    assert_eq!(v("b", "text-decoration-thickness"), "10%");
    assert_eq!(v("b", "text-underline-offset"), "calc(10% + 20px)");
    assert_eq!(v("b", "text-underline-position"), "under left");
    assert_eq!(
        v("b", "text-shadow"),
        "rgb(0, 0, 255) 20px 2px 0px, rgb(255, 0, 0) 0px 0px 3px"
    );
    assert_eq!(v("b", "-webkit-text-fill-color"), "rgb(0, 255, 0)");
    // `unset`: what each would be without it.
    assert_eq!(v("c", "text-decoration-color"), "rgb(0, 0, 255)");
    assert_eq!(v("c", "text-underline-position"), "under left");
    assert_eq!(
        v("c", "text-shadow"),
        "rgb(0, 0, 255) 20px 2px 0px, rgb(255, 0, 0) 0px 0px 3px"
    );
    assert_eq!(v("c", "-webkit-text-fill-color"), "rgb(0, 255, 0)");
}

/// `filter`: amounts as numbers, at least 0, at most 1 where they stop
/// there; lengths in px, angles in degrees, the shadow's color resolved;
/// not inherited, nor `isolation`.
#[test]
fn filters() {
    let html = r#"<div id=a style="font-size: 20px; color: lime; isolation: isolate;
           filter: blur(1em) brightness(300%) grayscale(calc(3)) opacity(calc(-10%)) hue-rotate(0.25turn) drop-shadow(1px 2px) contrast()">
      <span id=b></span></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(
        v("a", "filter"),
        "blur(20px) brightness(3) grayscale(1) opacity(0) hue-rotate(90deg) drop-shadow(rgb(0, 255, 0) 1px 2px 0px) contrast(1)"
    );
    assert_eq!(v("a", "isolation"), "isolate");
    assert_eq!(
        (v("b", "filter"), v("b", "isolation")),
        ("none".into(), "auto".into())
    );
}

/// `clip` and `clip-path`: lengths in px, `rect()` and `xywh()` as the
/// `inset()` they are, the center given, `closest-side` and a zero
/// rounding left out (Blink's `ValueForBasicShape`); not inherited.
#[test]
fn clips() {
    let html = r#"<div style="font-size: 40px">
      <p id=a style="clip: rect(1em, auto, calc(-1em + 10px), 2px); clip-path: rect(0 1% 2px 0.5em)"><span id=b></span></p>
      <p id=c style="clip-path: xywh(5px 1% 2px 0.5em round 0 1px)"></p>
      <p id=d style="clip-path: circle(calc(10px - 0.5em) at 50% -50%)"></p>
      <p id=e style="clip-path: ellipse(farthest-side 1em at right 10px top 0%) content-box"></p>
      <p id=f style="clip-path: polygon(evenodd round 1em, 1px 2px, 3em 4em)"></p>
      <p id=g style="clip-path: circle()"></p>
      <p id=h style="clip-path: rect(auto auto auto auto)"></p>
      <p id=i style="clip-path: margin-box"></p>
      <p id=j style="clip-path: polygon(round calc(1px - 1em), 1px 2px)"></p><p id=k style="clip-path: circle(at 10% 20%)"></p>
      <p id=l style="clip-path: url(#c); clip-rule: evenodd"><span id=m></span><span id=n style="clip-rule: unset"></span></p></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "clip"), "rect(40px, auto, -30px, 2px)");
    assert_eq!(v("a", "clip-path"), "inset(0px 99% calc(100% - 2px) 20px)");
    assert_eq!(
        (v("b", "clip"), v("b", "clip-path")),
        ("auto".into(), "none".into())
    );
    assert_eq!(
        v("c", "clip-path"),
        "inset(1% calc(100% - 7px) calc(99% - 20px) 5px round 0px 1px)"
    );
    assert_eq!(v("d", "clip-path"), "circle(0px at 50% -50%)");
    assert_eq!(
        v("e", "clip-path"),
        "ellipse(farthest-side 40px at calc(100% - 10px) 0%) content-box"
    );
    assert_eq!(
        v("f", "clip-path"),
        "polygon(evenodd round 40px, 1px 2px, 120px 160px)"
    );
    // A center not given stays so.
    assert_eq!(v("g", "clip-path"), "circle()");
    assert_eq!(v("h", "clip-path"), "inset(0%)");
    assert_eq!(v("i", "clip-path"), "margin-box");
    assert_eq!(v("j", "clip-path"), "polygon(1px 2px)");
    assert_eq!(v("k", "clip-path"), "circle(at 10% 20%)");
    // A fragment alone stays as written; `clip-rule` is inherited.
    assert_eq!(v("l", "clip-path"), "url(\"#c\")");
    assert_eq!(
        (v("b", "clip-rule"), v("m", "clip-rule")),
        ("nonzero".into(), "evenodd".into())
    );
    assert_eq!(v("n", "clip-rule"), "evenodd");
}

/// `contain`: `none` at first, not inherited; exactly `strict` or
/// `content` is that keyword, else what it has in order.
#[test]
fn contains() {
    let html = r#"<div id=a style="contain: size layout style paint"><span id=b></span></div>
      <div id=c style="contain: paint layout style"></div><div id=d style="contain: style inline-size"></div>"#;
    let v = |id: &str| value(html, id, "contain");
    assert_eq!(v("a"), "strict");
    assert_eq!(v("b"), "none");
    assert_eq!(v("c"), "content");
    assert_eq!(v("d"), "inline-size style");
}

/// `tab-size`: a number stays one, a length is px; it is inherited, and 8
/// at first.
#[test]
fn tab_sizes() {
    let html = r#"<div id=a style="tab-size: 2em; font-size: 10px"><span id=b></span></div>
      <div id=c style="tab-size: calc(-1)"></div><div id=d></div>
      <div id=e style="tab-size: calc(-2px)"></div>"#;
    let v = |id: &str| value(html, id, "tab-size");
    assert_eq!(v("a"), "20px");
    assert_eq!(v("b"), "20px");
    assert_eq!(v("c"), "0");
    assert_eq!(v("d"), "8");
    assert_eq!(v("e"), "0px");
}

/// The text properties' computed values: `letter-spacing: normal` stays
/// `normal`, `word-spacing: normal` is `0px` (as Chrome computes it), a
/// `text-indent` percentage stays one.
#[test]
fn text_properties_compute() {
    let html = r#"<style>
      #a { font-size: 10px; letter-spacing: 0.5em; word-spacing: normal; text-indent: 10%;
           text-transform: capitalize; word-break: keep-all; overflow-wrap: break-word;
           text-overflow: ellipsis;
           white-space-collapse: preserve-breaks; text-wrap-mode: nowrap }
      #b { font-size: 10px; letter-spacing: normal; word-spacing: 2em; text-wrap: nowrap }
      #c { letter-spacing: 0px; vertical-align: 1em; font-size: 10px }
      #d { vertical-align: 50% }
    </style><div id=a><span id=in-a></span></div><div id=b></div><div id=c></div><div id=d></div>
    <sub id=e>x</sub><sup id=f>x</sup>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "letter-spacing"), "5px");
    assert_eq!(v("a", "word-spacing"), "0px");
    assert_eq!(v("a", "text-indent"), "10%");
    assert_eq!(v("a", "text-transform"), "capitalize");
    assert_eq!(v("a", "text-overflow"), "ellipsis");
    assert_eq!(v("in-a", "text-overflow"), "clip", "not inherited");
    assert_eq!(v("a", "word-break"), "keep-all");
    assert_eq!(v("a", "overflow-wrap"), "break-word");
    assert_eq!(v("a", "white-space-collapse"), "preserve-breaks");
    assert_eq!(v("a", "text-wrap-mode"), "nowrap");
    assert_eq!(v("b", "letter-spacing"), "normal");
    assert_eq!(
        v("c", "letter-spacing"),
        "normal",
        "0 is normal, as in Chrome"
    );
    assert_eq!(v("b", "word-spacing"), "20px");
    assert_eq!(v("b", "text-wrap-mode"), "nowrap");
    assert_eq!(v("b", "white-space-collapse"), "collapse");
    assert_eq!(v("c", "vertical-align"), "10px");
    assert_eq!(v("d", "vertical-align"), "50%", "a percentage stays one");
    assert_eq!(v("b", "vertical-align"), "baseline");
    // The UA stylesheet's.
    assert_eq!(v("e", "vertical-align"), "sub");
    assert_eq!(v("f", "vertical-align"), "super");
}

/// A shorthand is written from its computed longhands, as Chrome writes
/// it; one of Chrome's that also sets longhands machopo hasn't comes out
/// empty (`None`).
#[test]
fn shorthands_come_from_their_computed_longhands() {
    let html = r#"<style>
      #a { font-size: 10px; margin: 1px 2em; padding: 0; overflow: hidden;
           flex: 2 1 30%; gap: 1px 2px; grid-area: 2 / 3; border: 1px solid red;
           align-items: center; justify-items: start; text-decoration: underline }
    </style><div id=a></div>"#;
    let styles = styles(html);
    let style = styles.get("a").expect("styled");
    // What `getComputedStyle` does for an element with no box: each
    // longhand's computed value, written back as the shorthand.
    let v = |property: &str| {
        let values: Vec<(&str, String)> = shorthand_longhands(property)?
            .into_iter()
            .map(|name| (name, computed_value(style, name).expect("a longhand")))
            .collect();
        shorthand_value(property, &values)
    };
    assert_eq!(v("margin").as_deref(), Some("1px 20px"));
    assert_eq!(v("padding").as_deref(), Some("0px"));
    assert_eq!(v("overflow").as_deref(), Some("hidden"));
    assert_eq!(v("flex").as_deref(), Some("2 1 30%"));
    assert_eq!(v("gap").as_deref(), Some("1px 2px"));
    // The same shortest form 1a writes (trailing `auto` inferred).
    assert_eq!(v("grid-area").as_deref(), Some("2 / 3"));
    assert_eq!(v("place-items").as_deref(), Some("center start"));
    assert_eq!(v("border-width").as_deref(), Some("1px"));
    assert_eq!(v("border-color").as_deref(), Some("rgb(255, 0, 0)"));
    assert_eq!(v("border-top").as_deref(), Some("1px solid rgb(255, 0, 0)"));
    // 1a's serialization throughout: the initial `nowrap` is left out.
    assert_eq!(v("flex-flow").as_deref(), Some("row"));
    // Only what isn't initial; all of it initial is `outside`.
    assert_eq!(v("list-style").as_deref(), Some("outside"));
    assert_eq!(v("grid-template").as_deref(), Some("none"));
    // `background` as Chrome resolves it: every value of every layer.
    assert_eq!(
        v("background").as_deref(),
        Some("rgba(0, 0, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box")
    );
    // `font` as Chrome resolves it (the weight 400 left out); `border`
    // from its sides, whatever its `border-image`.
    assert_eq!(v("font").as_deref(), Some("10px serif"));
    assert_eq!(v("border").as_deref(), Some("1px solid rgb(255, 0, 0)"));
    // `text-decoration` is written from the style, the color left out
    // while it is `currentcolor` (which its resolved value doesn't tell).
    assert_eq!(
        v("text-decoration").as_deref(),
        Some("underline rgb(0, 0, 0)")
    );
    assert_eq!(
        style_resolver::shorthand_from_style(style, "text-decoration").as_deref(),
        Some("underline")
    );
    assert_eq!(style_resolver::shorthand_from_style(style, "margin"), None);
    assert_eq!(v("margin-bogus"), None);
}

/// The background longhands, computed as Chrome writes them (WPT's
/// `css/css-backgrounds/parsing/*-computed.html`): keywords become
/// percentages, a length from the end side `calc()`, the lists as long as
/// declared, gradients with their defaults left out.
#[test]
fn background_values() {
    let html = r#"<style>
      #a { font-size: 20px; background-image: url(http://x.test/a.png), none;
           background-position: right -18px top 17px, center left;
           background-size: 1px, auto auto, contain;
           background-repeat: repeat repeat, repeat-x, space round;
           background-attachment: local, fixed;
           background-origin: content-box; background-clip: padding-box, border-box }
      #b { background: url(http://x.test/b.png) right 11% bottom / 2em auto no-repeat
                       border-box content-box, red }
      #c { background-image: linear-gradient(to left bottom, red, blue),
                             linear-gradient(to bottom, red, blue) }
      #d { background-image: radial-gradient(farthest-corner at 50%, red, blue),
                             radial-gradient(farthest-side at 10px 10px, red, currentcolor),
                             repeating-radial-gradient(circle, red 0, 25%, blue 1em 2em),
                             radial-gradient(ellipse 10% 2em at left top, red, blue),
                             linear-gradient(0.25turn, red, blue) }
    </style><div id=a></div><div id=b></div><div id=c></div><div id=d style="color: blue"></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(
        v("a", "background-image"),
        r#"url("http://x.test/a.png"), none"#
    );
    assert_eq!(v("a", "background-position-x"), "calc(100% + 18px), 0%");
    assert_eq!(v("a", "background-position-y"), "17px, 50%");
    assert_eq!(v("a", "background-size"), "1px auto, auto, contain");
    assert_eq!(v("a", "background-repeat"), "repeat, repeat-x, space round");
    assert_eq!(v("a", "background-attachment"), "local, fixed");
    assert_eq!(v("a", "background-origin"), "content-box");
    assert_eq!(v("a", "background-clip"), "padding-box, border-box");
    assert_eq!(v("b", "background-position-x"), "89%, 0%");
    assert_eq!(v("b", "background-position-y"), "100%, 0%");
    assert_eq!(v("b", "background-size"), "32px auto, auto");
    assert_eq!(v("b", "background-color"), "rgb(255, 0, 0)");
    assert_eq!(
        v("c", "background-image"),
        // `to bottom`, the default, left out.
        "linear-gradient(to left bottom, rgb(255, 0, 0), rgb(0, 0, 255)), \
         linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255))"
    );
    // The shorthand: every value of every layer, the color first in the
    // last one.
    let styles = styles(html);
    let b = styles.get("b").unwrap();
    let values: Vec<(&str, String)> = shorthand_longhands("background")
        .unwrap()
        .into_iter()
        .map(|name| (name, computed_value(b, name).unwrap()))
        .collect();
    assert_eq!(
        shorthand_value("background", &values).as_deref(),
        Some(
            "url(\"http://x.test/b.png\") no-repeat scroll 89% 100% / 32px auto border-box \
             content-box, rgb(255, 0, 0) none repeat scroll 0% 0% / auto padding-box border-box"
        )
    );
    assert_eq!(
        v("d", "background-image"),
        "radial-gradient(rgb(255, 0, 0), rgb(0, 0, 255)), \
         radial-gradient(farthest-side at 10px 10px, rgb(255, 0, 0), rgb(0, 0, 255)), \
         repeating-radial-gradient(circle, rgb(255, 0, 0) 0px, 25%, rgb(0, 0, 255) 16px, \
         rgb(0, 0, 255) 32px), \
         radial-gradient(10% 32px at 0% 0%, rgb(255, 0, 0), rgb(0, 0, 255)), \
         linear-gradient(90deg, rgb(255, 0, 0), rgb(0, 0, 255))"
    );
}

/// `image-set()` computed, as Chrome 152 has it: the whole set, URLs
/// absolute, resolutions in dppx (`calc()` worked out, a negative one 0),
/// gradients computed; `cursor` and `border-image-source` too.
#[test]
fn image_sets_compute_to_dppx() {
    let html = r#"<style>
      #a { background-image: image-set(url(http://x.test/a.png) 1x, "http://x.test/b.png" 192dpi,
                                       url(http://x.test/c.png) 1dpcm type("image/png")) }
      #b { background-image: -webkit-image-set(linear-gradient(red, blue) calc(1x + 1x),
                                               url(http://x.test/d.png) calc(-1x)), none }
      #c { cursor: image-set(url(http://x.test/e.cur) 2x) 1 2, auto;
           border-image-source: image-set(url(http://x.test/f.png) 1x) }
    </style><div id=a></div><div id=b></div><div id=c></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(
        v("a", "background-image"),
        r#"image-set(url("http://x.test/a.png") 1dppx, url("http://x.test/b.png") 2dppx, url("http://x.test/c.png") 0.0264583dppx type("image/png"))"#
    );
    assert_eq!(
        v("b", "background-image"),
        r#"image-set(linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)) 2dppx, url("http://x.test/d.png") 0dppx), none"#
    );
    assert_eq!(
        v("c", "cursor"),
        r#"image-set(url("http://x.test/e.cur") 2dppx) 1 2, auto"#
    );
    assert_eq!(
        v("c", "border-image-source"),
        r#"image-set(url("http://x.test/f.png") 1dppx)"#
    );
}

/// `overflow-anchor` computes to its keyword and isn't inherited.
#[test]
fn overflow_anchor_is_not_inherited() {
    let html = "<div id=p style='overflow-anchor: none'><div id=c></div></div>";
    assert_eq!(value(html, "p", "overflow-anchor"), "none");
    assert_eq!(value(html, "c", "overflow-anchor"), "auto");
}

/// `position` and the insets, computed: the keyword, and lengths made
/// absolute with the percentages and `auto` left alone (the resolved value
/// of an inset is the used offset, which needs the box).
#[test]
fn position_and_insets() {
    let html = r#"<style>
      #a { position: relative; font-size: 20px; top: 1.5em; right: 25%;
           bottom: 0; left: auto }
      #b { }
    </style><div id=a></div><div id=b></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "position"), "relative");
    assert_eq!(v("a", "top"), "30px");
    assert_eq!(v("a", "right"), "25%");
    assert_eq!(v("a", "bottom"), "0px");
    assert_eq!(v("a", "left"), "auto");
    assert_eq!(v("b", "position"), "static");
    assert_eq!(v("b", "top"), "auto");
    let shorthand = |id: &str, property: &str| {
        let values: Vec<(&str, String)> = shorthand_longhands(property)
            .expect("a shorthand")
            .into_iter()
            .map(|name| (name, value(html, id, name)))
            .collect();
        shorthand_value(property, &values)
    };
    assert_eq!(
        shorthand("b", "inset").as_deref(),
        Some("auto"),
        "the shorthand too"
    );
}

/// `float` and `clear`, not inherited; an out-of-flow box doesn't float
/// (CSS 2 § 9.7), and a float is blockified.
#[test]
fn float_and_clear() {
    let html = r#"<style>
      #left { float: left; clear: both }
      #right { float: RIGHT; clear: right }
      #abs { float: left; position: absolute }
      #fixed { float: right; position: fixed }
    </style><div id=left><span id=child></span></div><span id=right></span>
    <div id=abs></div><div id=fixed></div><div id=none></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(
        (v("left", "float"), v("left", "clear")),
        ("left".into(), "both".into())
    );
    assert_eq!(
        (v("right", "float"), v("right", "clear")),
        ("right".into(), "right".into())
    );
    assert_eq!(
        (v("child", "float"), v("child", "clear")),
        ("none".into(), "none".into())
    );
    assert_eq!(v("abs", "float"), "none");
    assert_eq!(v("fixed", "float"), "none");
    assert_eq!(v("none", "float"), "none");
    // A floated `<span>` is a block.
    assert_eq!(v("right", "display"), "block");
}

/// The table `display` values are what an element's `display` computes to,
/// blockified as the others are: `inline-table` in a flex container is
/// `table`, a table-internal value `block` (Blink's
/// `EquivalentBlockDisplay`).
#[test]
fn table_displays() {
    let html = r#"<style>
      #t { display: table } #c { display: table-cell } #i { display: inline-table }
      #fc { display: table-cell } #fr { display: table-row } #ft { display: table }
    </style><div id=t></div><div id=c></div><div style="display: flex"><span id=i></span>
      <span id=fc></span><span id=fr></span><span id=ft></span></div>
      <span id=float style="display: table-caption; float: left"></span>"#;
    let v = |id: &str| value(html, id, "display");
    assert_eq!(v("t"), "table");
    assert_eq!(v("c"), "table-cell");
    assert_eq!(v("i"), "table");
    assert_eq!(v("fc"), "block");
    assert_eq!(v("fr"), "block");
    assert_eq!(v("ft"), "table");
    assert_eq!(v("float"), "block");
}

/// The corners' radii, computed: lengths absolute, percentages kept, one
/// value when both are the same; the shorthand made back from them.
#[test]
fn border_radius_values() {
    let html = r#"<style>
      #a { font-size: 20px; border-radius: 5em / 1px 2% 3px 4% }
      #b { border-top-left-radius: 40px 0px; border-bottom-right-radius: 10px 10px }
      #c { border-bottom-left-radius: inherit }
      #d { border-top-left-radius: inherit }
    </style><div id=a><div id=c></div><div id=d></div></div><div id=b></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("a", "border-top-left-radius"), "100px 1px");
    assert_eq!(v("a", "border-top-right-radius"), "100px 2%");
    assert_eq!(v("b", "border-top-left-radius"), "40px 0px");
    assert_eq!(v("b", "border-bottom-right-radius"), "10px");
    assert_eq!(v("b", "border-top-right-radius"), "0px");
    // `inherit` copies the parent's computed radii, its `em` already px.
    assert_eq!(v("c", "border-bottom-left-radius"), "100px 4%");
    assert_eq!(v("c", "border-top-left-radius"), "0px");
    assert_eq!(v("d", "border-top-left-radius"), "100px 1px");
    assert_eq!(v("d", "border-bottom-left-radius"), "0px");
    let styles = styles(html);
    let a = styles.get("a").unwrap();
    let values: Vec<(&str, String)> = shorthand_longhands("border-radius")
        .unwrap()
        .into_iter()
        .map(|name| (name, computed_value(a, name).unwrap()))
        .collect();
    assert_eq!(
        shorthand_value("border-radius", &values).as_deref(),
        Some("100px / 1px 2% 3px 4%")
    );
}

#[test]
fn box_shadow_and_outline_values() {
    let html = r#"<style>
      #a { color: blue; font-size: 10px; box-shadow: 1em 2px red, inset 0 0 3px 4px }
      #b { outline: auto 1px -webkit-focus-ring-color; outline-offset: 1em }
      #c { outline-style: solid; border-top-style: solid }
      #d { outline-width: 5px; border-top-width: 5px }
      #e { outline-style: solid; outline-width: initial; border-top: solid; border-top-width: initial }
      #f { border-top: 5px hidden; border-left: 5px dotted; border-right-style: groove; outline: 2px dashed }
    </style><div id=a></div><div id=b></div><div id=c></div><div id=d></div><div id=e></div><div id=f></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    // Every length, the color resolved, `inset` last.
    assert_eq!(
        v("a", "box-shadow"),
        "rgb(255, 0, 0) 10px 2px 0px 0px, rgb(0, 0, 255) 0px 0px 3px 4px inset"
    );
    assert_eq!(v("b", "box-shadow"), "none");
    assert_eq!(v("b", "outline-style"), "auto");
    assert_eq!(v("b", "outline-color"), "rgb(16, 16, 16)");
    assert_eq!(v("b", "outline-width"), "1px");
    assert_eq!(v("b", "outline-offset"), "16px");
    // A style with no width is `medium`, 3px, for outlines and borders;
    // with style `none` a border's width is 0 however wide it was
    // written, an outline's as written (css-ui 4).
    assert_eq!(v("c", "outline-width"), "3px");
    assert_eq!(v("c", "border-top-width"), "3px");
    assert_eq!(v("d", "outline-width"), "5px");
    assert_eq!(v("d", "border-top-width"), "0px");
    // `hidden` is drawn as `none`, and zeroes the width as it does; the
    // other styles keep it.
    assert_eq!(v("f", "border-top-style"), "hidden");
    assert_eq!(v("f", "border-top-width"), "0px");
    assert_eq!(v("f", "border-left-style"), "dotted");
    assert_eq!(v("f", "border-left-width"), "5px");
    assert_eq!(v("f", "border-right-style"), "groove");
    assert_eq!(v("f", "border-right-width"), "3px");
    assert_eq!(v("f", "outline-style"), "dashed");
    // `initial` is `medium` too.
    assert_eq!(v("e", "outline-width"), "3px");
    assert_eq!(v("e", "border-top-width"), "3px");
    let styles = styles(html);
    let b = styles.get("b").unwrap();
    let values: Vec<(&str, String)> = shorthand_longhands("outline")
        .unwrap()
        .into_iter()
        .map(|name| (name, computed_value(b, name).unwrap()))
        .collect();
    assert_eq!(
        shorthand_value("outline", &values).as_deref(),
        Some("rgb(16, 16, 16) auto 1px")
    );
}

/// The lists' user-agent styles and presentational hints (HTML, "Lists"):
/// `<li>` is a list item; `<ol>`, `<ul>` reset `list-item`, `start` and
/// `reversed` as theirs, `<li value>` sets it; the bullets by depth; the
/// `type`s. The list item's own `counter-increment` stays `none`: its
/// increment of `list-item` is implicit.
#[test]
fn lists_and_their_hints() {
    let html = r#"<ul id=ul><li id=li>
        <ul id=ul2><li><ul id=ul3></ul></ul>
        <ol id=ol2></ol></ul>
      <ol id=ol></ol><ol id=start start=" 3x"></ol><ol id=reversed reversed></ol>
      <ol id=both reversed start=5 type=A></ol><ol id=bad type=disc></ol>
      <ul id=square type=SQUARE></ul><ol><li id=value value=10 type=i></ol>
      <ul><li id=lower type=a></ul><div id=div style="list-style: inside"></div>"#;
    let v = |id: &str, property: &str| value(html, id, property);
    assert_eq!(v("li", "display"), "list-item");
    assert_eq!(v("li", "counter-increment"), "none");
    assert_eq!(v("ul2", "list-style-type"), "circle");
    assert_eq!(v("ul3", "list-style-type"), "square");
    assert_eq!(v("ol2", "list-style-type"), "decimal");
    assert_eq!(v("ul", "counter-reset"), "list-item 0");
    assert_eq!(v("ol", "list-style-type"), "decimal");
    assert_eq!(v("start", "counter-reset"), "list-item 2");
    assert_eq!(v("reversed", "counter-reset"), "reversed(list-item)");
    assert_eq!(v("both", "counter-reset"), "reversed(list-item) 6");
    assert_eq!(v("both", "list-style-type"), "upper-alpha");
    assert_eq!(v("bad", "list-style-type"), "decimal");
    assert_eq!(v("square", "list-style-type"), "square");
    assert_eq!(v("value", "counter-set"), "list-item 10");
    assert_eq!(v("value", "list-style-type"), "lower-roman");
    assert_eq!(v("lower", "list-style-type"), "lower-alpha");
    assert_eq!(v("div", "list-style-position"), "inside");
    assert_eq!(v("div", "counter-reset"), "none");
}

/// The properties machopo parses but doesn't act on yet: Chrome's initial
/// values, which inherit as Chrome's do (`user-select` too, which css-ui 4
/// has not inherited: `unset` takes the parent's), and what they compute to: `resize: auto` to `both`
/// (a `<textarea>`'s, from the UA stylesheet), `text-size-adjust: none` to
/// 100%.
#[test]
fn the_properties_parsed_only() {
    let html = r#"<style>
      #p { user-select: none; -webkit-user-modify: read-write; hyphens: auto;
           forced-color-adjust: none; -webkit-tap-highlight-color: currentcolor;
           color: red; text-size-adjust: none; text-rendering: optimizeLegibility;
           -webkit-font-smoothing: antialiased; resize: vertical; scroll-behavior: smooth }
      #auto { resize: auto }
      #unset { user-select: unset; resize: unset }
    </style><div id=initial></div><div id=p><span id=child></span><span id=unset></span></div>
    <textarea id=area></textarea><div id=auto></div>"#;
    let styles = styles(html);
    let v = |id: &str, property: &str| {
        computed_value(&styles[id], property).expect("a supported property")
    };
    for (property, initial, set) in [
        ("user-select", "auto", "none"),
        ("-webkit-user-modify", "read-only", "read-write"),
        ("hyphens", "manual", "auto"),
        ("forced-color-adjust", "auto", "none"),
        (
            "-webkit-tap-highlight-color",
            "rgba(0, 0, 0, 0.18)",
            "rgb(255, 0, 0)",
        ),
        ("text-size-adjust", "auto", "100%"),
        ("text-rendering", "auto", "optimizelegibility"),
        ("-webkit-font-smoothing", "auto", "antialiased"),
    ] {
        assert_eq!(v("initial", property), initial, "{property}");
        assert_eq!(v("p", property), set, "{property}");
        assert_eq!(v("child", property), set, "{property} is inherited");
    }
    assert_eq!(v("unset", "user-select"), "none", "unset inherits it");
    assert_eq!(v("unset", "resize"), "none", "unset makes it initial");
    for (property, initial, set) in [
        ("resize", "none", "vertical"),
        ("scroll-behavior", "auto", "smooth"),
    ] {
        assert_eq!(v("initial", property), initial, "{property}");
        assert_eq!(v("p", property), set, "{property}");
        assert_eq!(v("child", property), initial, "{property} isn't inherited");
    }
    assert_eq!(v("area", "resize"), "both");
    assert_eq!(v("auto", "resize"), "both");
}

/// `cursor`: `auto` at first, inherited; its images' URLs absolute (an
/// empty one and a fragment as written, as Chrome has them), a
/// `-webkit-` alias the keyword it stands for.
#[test]
fn cursors() {
    let html = r#"<style>
      #a { cursor: url(http://example.com/a.png) 2 3, url(""), url(#c), -webkit-grab }
      #b { cursor: crosshair }
      #unset { cursor: unset }
    </style><div id=initial></div><div id=a><span id=child></span></div>
    <div id=b><span id=unset></span></div>"#;
    let styles = styles(html);
    let v = |id: &str| computed_value(&styles[id], "cursor").expect("a supported property");
    let a = "url(\"http://example.com/a.png\") 2 3, url(\"\"), url(\"#c\"), grab";
    assert_eq!(v("initial"), "auto");
    assert_eq!(v("a"), a);
    assert_eq!(v("child"), a, "inherited");
    assert_eq!(v("b"), "crosshair");
    assert_eq!(v("unset"), "crosshair", "unset inherits it");
}

/// `border-image`'s longhands: initial, and as computed (lengths in px,
/// numbers as numbers, URLs absolute). The shorthand is `none` without a
/// source, else all five (Chrome's); `border` is written from its sides
/// whatever its `border-image`.
#[test]
fn border_images() {
    let html = r#"<style>
      #a { border: 1px solid red; border-image: url(http://example.com/a.png) 30 fill / 0.5 2em / 1em round;
           font-size: 10px }
      #b { border-image: 10 }
    </style><div id=initial></div><div id=a></div><div id=b></div>"#;
    let styles = styles(html);
    let v = |id: &str, property: &str| {
        computed_value(&styles[id], property).expect("a supported property")
    };
    for (property, initial, set) in [
        (
            "border-image-source",
            "none",
            "url(\"http://example.com/a.png\")",
        ),
        ("border-image-slice", "100%", "30 fill"),
        ("border-image-width", "1", "0.5 20px"),
        ("border-image-outset", "0", "10px"),
        ("border-image-repeat", "stretch", "round"),
    ] {
        assert_eq!(v("initial", property), initial, "{property}");
        assert_eq!(v("a", property), set, "{property}");
    }
    let shorthand = |id: &str, property: &str| {
        let longhands = style_resolver::shorthand_longhands(property).expect("a shorthand");
        let values: Vec<(&str, String)> = longhands
            .iter()
            .map(|name| (*name, computed_value(&styles[id], name).unwrap()))
            .collect();
        style_resolver::shorthand_value(property, &values)
    };
    assert_eq!(
        shorthand("initial", "border-image").as_deref(),
        Some("none")
    );
    assert_eq!(shorthand("b", "border-image").as_deref(), Some("none"));
    assert_eq!(
        shorthand("a", "border-image").as_deref(),
        Some("url(\"http://example.com/a.png\") 30 fill / 0.5 20px / 10px round")
    );
    assert_eq!(
        shorthand("a", "border").as_deref(),
        Some("1px solid rgb(255, 0, 0)")
    );
}
