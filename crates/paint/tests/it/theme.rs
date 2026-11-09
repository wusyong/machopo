//! Form controls' native looks: the ops each part paints in place of its
//! CSS background and border, `NativeThemeBase`'s geometry in Chromium's
//! light colors (`kColorWebNativeControl*`) for its state.

use std::collections::HashMap;

use compositor_paint::{PaintOp, PaintStyle};
use dom::{Cx, NodeKind, NodeRef};
use gfx::{Color, NodeId, RectF, SizeF};
use paint::{DisplayItemKind, PaintArtifact};

use crate::page::{self, Page};

/// What the page asks of the pointer before it is painted.
#[derive(Default)]
struct Pointer<'a> {
    hovered: Option<&'a str>,
    pressed: Option<&'a str>,
}

/// `html` painted, its controls' UA shadow trees as the renderer makes
/// them, the elements `pointer` names hovered or pressed; with each
/// element's id (a UA shadow tree's part's as its control's id and its
/// shadow pseudo id, `search::-webkit-search-cancel-button`).
fn paint(html: &str, pointer: Pointer<'_>) -> (PaintArtifact, HashMap<String, NodeId>) {
    paint_scaled(html, pointer, 1.0)
}

/// [`paint`] at `scale` device pixels per CSS pixel.
fn paint_scaled(
    html: &str,
    pointer: Pointer<'_>,
    scale: f32,
) -> (PaintArtifact, HashMap<String, NodeId>) {
    paint_with(html, pointer, scale, |_, _| {})
}

/// [`paint_scaled`], the document changed by `change` before it is styled.
fn paint_with(
    html: &str,
    pointer: Pointer<'_>,
    scale: f32,
    change: impl for<'gc> FnOnce(&Cx<'_, 'gc>, NodeRef<'gc>),
) -> (PaintArtifact, HashMap<String, NodeId>) {
    let mut ids = HashMap::new();
    let mut page = Page::with(html, |cx, document| {
        page::ensure_shadow_trees(cx, document);
        collect_ids(cx, document, None, &mut ids);
        let element = |id: &str| cx.node_by_id(ids[id]).expect("in the document");
        if let Some(id) = pointer.hovered {
            cx.set_hover_element(document, Some(element(id)));
        }
        if let Some(id) = pointer.pressed {
            cx.set_active_element(document, Some(element(id)));
        }
        change(cx, document);
    });
    let view = page.lay_out(SizeF::new(800.0, 600.0));
    let pre = paint::pre_paint(&view, &Default::default(), scale);
    let artifact = paint::paint(&view, &pre, layout::document_size(&view), &());
    (artifact, ids)
}

/// Each element in `node` with an id, into shadow trees: a UA shadow
/// tree's parts by `host`'s id (the control's) and their pseudo ids.
fn collect_ids<'gc>(
    cx: &Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    host: Option<&str>,
    ids: &mut HashMap<String, NodeId>,
) {
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        if cx.kind(at) == NodeKind::Element {
            let id = cx.get_attribute(at, "id").map(|id| id.to_string());
            if let Some(id) = &id {
                ids.insert(id.clone(), cx.node_id(at));
            }
            if let (Some(host), Some(pseudo)) = (host, cx.get_attribute(at, "pseudo")) {
                ids.insert(format!("{host}::{pseudo}"), cx.node_id(at));
            }
            if let Some(root) = cx.shadow_root(at) {
                collect_ids(cx, root, id.as_deref().or(host), ids);
            }
        }
        collect_ids(cx, at, host, ids);
        child = cx.next_sibling(at);
    }
}

/// The ops `id`'s box paints as its background and border.
fn ops<'a>(painted: &'a (PaintArtifact, HashMap<String, NodeId>), id: &str) -> Vec<&'a PaintOp> {
    let node = painted.1[id];
    painted
        .0
        .items
        .iter()
        .filter(|item| {
            item.client == Some(node) && item.kind == DisplayItemKind::BoxDecorationBackground
        })
        .map(|item| &item.op)
        .collect()
}

/// An op's shape and color, short: `fill rrect 13x13 #0075ff` and the
/// like (a stroke's width after its color).
fn describe(op: &PaintOp) -> String {
    let color = |c: Color| {
        if c.a == 255 {
            format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
        }
    };
    let style = |flags: &compositor_paint::PaintFlags| match flags.style {
        PaintStyle::Fill => format!("fill {}", color(flags.color)),
        PaintStyle::Stroke { width } => format!("stroke {} {width}", color(flags.color)),
    };
    let size = |rect: RectF| format!("{}x{}", rect.width, rect.height);
    match op {
        PaintOp::DrawRRect { rrect, flags } => {
            format!(
                "{} rrect {} r{}",
                style(flags),
                size(rrect.rect),
                rrect.radii[0].width
            )
        }
        PaintOp::DrawRect { rect, flags } => format!("{} rect {}", style(flags), size(*rect)),
        PaintOp::DrawPath { flags, .. } => format!("{} path", style(flags)),
        other => format!("{other:?}"),
    }
}

fn described(painted: &(PaintArtifact, HashMap<String, NodeId>), id: &str) -> Vec<String> {
    ops(painted, id).into_iter().map(describe).collect()
}

/// A check box: a white rounded square with its border; checked, an
/// accent one with a white check; indeterminate, a white dash; disabled,
/// faded (Chromium's colors and `NativeThemeBase`'s geometry).
#[test]
fn check_boxes() {
    let p = paint(
        "<input type=checkbox id=off><input type=checkbox checked id=on>
         <input type=checkbox checked disabled id=disabled>
         <input type=checkbox id=mixed>",
        Pointer::default(),
    );
    assert_eq!(
        described(&p, "off"),
        [
            "fill #ffffff rrect 12.6x12.6 r2",
            "stroke #767676 1 rrect 12x12 r2"
        ]
    );
    assert_eq!(
        described(&p, "on"),
        ["fill #0075ff rrect 13x13 r2", "stroke #ffffff 2.08 path"]
    );
    assert_eq!(
        described(&p, "disabled"),
        [
            "fill #7676764d rrect 13x13 r2",
            "stroke #ffffff99 2.08 path"
        ]
    );
    // The CSS border and background aren't painted under it: these are
    // all its ops.
    assert_eq!(ops(&p, "on").len(), 2);
    // The check: from (0.2, 0.5) to (0.4, 0.7) to (0.8, 0.3) of the box.
    let PaintOp::DrawPath { path, .. } = ops(&p, "on")[1] else {
        panic!("a path");
    };
    let origin = match ops(&p, "on")[0] {
        PaintOp::DrawRRect { rrect, .. } => rrect.rect,
        other => panic!("{other:?}"),
    };
    let points: Vec<(f32, f32)> = path
        .elements()
        .iter()
        .filter_map(|element| match element {
            gfx::PathElement::MoveTo(p) | gfx::PathElement::LineTo(p) => {
                Some((p.x - origin.x, p.y - origin.y))
            }
            _ => None,
        })
        .map(|(x, y)| ((x * 100.0).round() / 100.0, (y * 100.0).round() / 100.0))
        .collect();
    assert_eq!(points, [(2.6, 6.5), (5.2, 9.1), (10.4, 3.9)]);
}

/// An indeterminate check box: the accent square with an 8×2 dash.
#[test]
fn an_indeterminate_check_box() {
    let p = paint_indeterminate();
    assert_eq!(
        described(&p, "mixed"),
        ["fill #0075ff rrect 13x13 r2", "fill #ffffff rect 8x2"]
    );
}

/// [`paint`] of a check box made indeterminate (its state, which script
/// sets: no attribute says it).
fn paint_indeterminate() -> (PaintArtifact, HashMap<String, NodeId>) {
    paint_with(
        "<input type=checkbox id=mixed>",
        Pointer::default(),
        1.0,
        |cx, document| cx.set_indeterminate(page::find(cx, document, "mixed"), true),
    )
}

/// A radio button: a white circle with its border; checked, the border in
/// the accent color and an accent dot 60% across.
#[test]
fn radio_buttons() {
    let p = paint(
        "<input type=radio id=off><input type=radio checked id=on>",
        Pointer::default(),
    );
    assert_eq!(
        described(&p, "off"),
        [
            "fill #ffffff rrect 12.6x12.6 r6.3",
            "stroke #767676 1 rrect 12x12 r6"
        ]
    );
    assert_eq!(
        described(&p, "on"),
        [
            "fill #ffffff rrect 12.6x12.6 r6.3",
            "stroke #0075ff 1 rrect 12x12 r6",
            "fill #0075ff rrect 7.8x7.8 r3.9"
        ]
    );
}

/// A button, filled and bordered, in its states: hovered, pressed (over
/// hovered) and disabled (lightened first).
#[test]
fn buttons_in_their_states() {
    let html =
        "<button id=b>Go</button><input type=submit id=s><button disabled id=off>Go</button>";
    let normal = paint(html, Pointer::default());
    let width = ops_width(&normal, "b");
    assert_eq!(
        described(&normal, "b"),
        [
            format!("fill #efefef rrect {width}x21 r2"),
            format!("stroke #767676 1 rrect {width}x21 r2")
        ]
    );
    assert_eq!(colors(&normal, "s"), ["#efefef", "#767676"]);
    assert_eq!(
        colors(&normal, "off"),
        ["#a9a9a933", "#efefef4d", "#7676764d"]
    );
    let hovered = paint(
        html,
        Pointer {
            hovered: Some("b"),
            pressed: None,
        },
    );
    assert_eq!(colors(&hovered, "b"), ["#e5e5e5", "#4f4f4f"]);
    let pressed = paint(
        html,
        Pointer {
            hovered: Some("b"),
            pressed: Some("b"),
        },
    );
    assert_eq!(colors(&pressed, "b"), ["#f5f5f5", "#8d8d8d"]);
}

/// The width of the first rounded rect `id` paints.
fn ops_width(painted: &(PaintArtifact, HashMap<String, NodeId>), id: &str) -> f32 {
    match ops(painted, id)[0] {
        PaintOp::DrawRRect { rrect, .. } => rrect.rect.width,
        other => panic!("{other:?}"),
    }
}

/// The colors of the ops `id` paints, in order.
fn colors(painted: &(PaintArtifact, HashMap<String, NodeId>), id: &str) -> Vec<String> {
    described(painted, id)
        .iter()
        .map(|op| {
            op.split_whitespace()
                .find(|word| word.starts_with('#'))
                .unwrap()
                .to_owned()
        })
        .collect()
}

/// A text field and a text area: white, with a 1px rounded border, faded
/// when it can't be edited; the author's background draws one as CSS does.
#[test]
fn text_fields() {
    let p = paint(
        "<input id=field><textarea id=area></textarea><input readonly id=read-only>
         <input disabled id=disabled><input style='background: rgb(255, 0, 0)' id=styled>",
        Pointer::default(),
    );
    assert_eq!(colors(&p, "field"), ["#ffffff", "#767676"]);
    assert_eq!(colors(&p, "area"), ["#ffffff", "#767676"]);
    // The UA's CSS colors a text area the same; its native look is rounded.
    let area = described(&p, "area");
    assert!(
        area.iter()
            .all(|op| op.contains(" rrect ") && op.ends_with(" r2")),
        "{area:?}"
    );
    assert_eq!(colors(&p, "read-only"), ["#ffffff", "#7676764d"]);
    assert_eq!(colors(&p, "disabled"), ["#ffffff", "#7676764d"]);
    let styled = described(&p, "styled");
    assert!(styled.iter().any(|op| op.contains("#ff0000")), "{styled:?}");
    assert!(!styled.iter().any(|op| op.contains("rrect")), "{styled:?}");
}

/// A range: its track (an 8px pill, its value's part in the accent color
/// up to its thumb's middle, under a half transparent border), and its
/// thumb (an accent circle) at its value; an author's thumb with no native
/// look is placed the same.
#[test]
fn ranges() {
    let p = paint(
        "<style>input { margin: 0 } #plain-thumb::-webkit-slider-thumb { appearance: none; width: 30px; height: 4px }</style>
         <div><input type=range value=25 id=quarter></div>
         <div><input type=range value=25 id=plain-thumb></div>
         <div><input type=range disabled id=disabled></div>",
        Pointer::default(),
    );
    let track = described(&p, "quarter");
    // The thumb's middle: 16 / 2 + 25% of (129 − 16).
    let middle = 8.0 + 0.25 * (129.0 - 16.0);
    assert_eq!(
        track,
        [
            "fill #efefef rrect 129x8 r4".to_owned(),
            format!("fill #0075ff rrect {middle}x8 r4"),
            // Its radius fits the 7px it is across.
            "stroke #76767680 1 rrect 128x7 r3.5".to_owned()
        ]
    );
    assert_eq!(
        described(&p, "quarter::-webkit-slider-thumb"),
        ["fill #0075ff rrect 15x15 r7.5"]
    );
    // 30px wide, a quarter of (129 − 30) in.
    let plain = described(&p, "plain-thumb");
    assert_eq!(
        plain[1],
        format!("fill #0075ff rrect {}x8 r4", 15.0 + 0.25 * (129.0 - 30.0))
    );
    assert!(ops(&p, "plain-thumb::-webkit-slider-thumb").is_empty());
    assert_eq!(
        colors(&p, "disabled"),
        ["#efefef4d", "#cbcbcb", "#7676764d"]
    );
    assert_eq!(colors(&p, "disabled::-webkit-slider-thumb"), ["#cbcbcb"]);
}

/// A search field's clear button (a cross) while it has a value, a number
/// field's spin button (two arrows): drawn in their boxes, transparent
/// until the field has the pointer or the focus.
#[test]
fn a_search_fields_and_a_number_fields_parts() {
    let p = paint(
        "<input type=search value=q id=search><input type=search id=empty><input type=number id=number>",
        Pointer::default(),
    );
    assert_eq!(
        described(&p, "search::-webkit-search-cancel-button"),
        ["stroke #2e5089 2 path"]
    );
    assert!(
        ops(&p, "empty::-webkit-search-cancel-button").is_empty(),
        "hidden while empty"
    );
    assert_eq!(
        described(&p, "number::-webkit-inner-spin-button"),
        ["fill #8b8b8b path"]
    );
}

/// A menu list: a text field's rounded box (filled with a check box's
/// background, its border in its state's color), and its arrow: a `∨` 8px
/// by 4px, in the middle of the 15px left of its right border, halfway
/// down, stroked 2 device pixels in its `color`; styled, its CSS
/// background and border, and the arrow over them.
#[test]
fn menu_lists() {
    let html = "<select id=plain><option>A</option></select>
         <select id=disabled disabled><option>A</option></select>
         <select id=red style='color: rgb(255, 0, 0)'><option>A</option></select>
         <select id=styled style='background: rgb(255, 255, 0)'><option>A</option></select>
         <select id=none style='appearance: none'><option>A</option></select>";
    let p = paint(html, Pointer::default());
    let plain = described(&p, "plain");
    assert_eq!(
        plain,
        [
            "fill #ffffff rrect 31x19 r2",
            "stroke #767676 1 rrect 31x19 r2",
            "stroke #000000 2 path"
        ]
    );
    // The arrow, from the select's border box (the box's 0.5px out).
    let (rrect, arrow) = match ops(&p, "plain")[..] {
        [
            PaintOp::DrawRRect { rrect, .. },
            _,
            PaintOp::DrawPath { path, .. },
        ] => (rrect.rect, path.bounds().unwrap()),
        ref other => panic!("{other:?}"),
    };
    assert_eq!(
        (
            arrow.x - rrect.x + 0.5,
            arrow.y - rrect.y + 0.5,
            arrow.width,
            arrow.height
        ),
        (32.0 - 1.0 - 11.5, 10.0 - 2.0, 8.0, 4.0)
    );
    assert_eq!(
        colors(&p, "disabled"),
        ["#ffffff99", "#7676764d", "#808080"]
    );
    assert_eq!(colors(&p, "red"), ["#ffffff", "#767676", "#ff0000"]);
    let styled = described(&p, "styled");
    assert!(styled[0].contains("#ffff00"), "{styled:?}");
    assert!(!styled.iter().any(|op| op.contains("rrect")), "{styled:?}");
    assert_eq!(
        styled.last().map(String::as_str),
        Some("stroke #000000 2 path")
    );
    // Without a native look, no arrow (the CSS border is filled).
    let none = described(&p, "none");
    assert!(!none.iter().any(|op| op.starts_with("stroke")), "{none:?}");
    let hovered = paint(
        html,
        Pointer {
            hovered: Some("plain"),
            pressed: None,
        },
    );
    assert_eq!(colors(&hovered, "plain"), ["#ffffff", "#4f4f4f", "#000000"]);
    // 2 device pixels at any scale.
    let scaled = paint_scaled(html, Pointer::default(), 2.0);
    assert_eq!(described(&scaled, "plain")[2], "stroke #000000 1 path");
}
