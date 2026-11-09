//! The paint artifact for a parsed, styled, laid-out document.

use std::collections::HashMap;

use compositor_paint::{PaintFlags, PaintOp};
use gfx::{Color, NodeId, PointF, RectF, SizeF};
use paint::{DisplayItemKind, PaintArtifact, PropertyTreeState};

use crate::page::{self, Page};

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

fn paint_html(html: &str) -> PaintArtifact {
    paint_html_at(html, &Default::default(), 1.0).1
}

/// `html` painted with the scroll containers at `scroll`, snapped to
/// `dsf` device pixels per CSS px.
fn paint_html_at(
    html: &str,
    scroll: &paint::ScrollOffsets,
    dsf: f32,
) -> (paint::PrePaint, PaintArtifact, HashMap<String, NodeId>) {
    paint_html_with(html, scroll, dsf, &())
}

/// [`paint_html_at`] with background images from `images`.
fn paint_html_with(
    html: &str,
    scroll: &paint::ScrollOffsets,
    dsf: f32,
    images: &dyn paint::BackgroundImages,
) -> (paint::PrePaint, PaintArtifact, HashMap<String, NodeId>) {
    paint_html_with_images(html, scroll, dsf, None, images)
}

/// [`paint_html_with`], and every `<img>`'s image `img_image`.
fn paint_html_with_images(
    html: &str,
    scroll: &paint::ScrollOffsets,
    dsf: f32,
    img_image: Option<&dom::Image>,
    images: &dyn paint::BackgroundImages,
) -> (paint::PrePaint, PaintArtifact, HashMap<String, NodeId>) {
    let mut page = match img_image {
        Some(image) => Page::with_images(html, image),
        None => Page::new(html),
    };
    let view = page.lay_out(VIEWPORT);
    let ids = page.ids();
    let pre = paint::pre_paint(&view, scroll, dsf);
    let artifact = paint::paint(&view, &pre, layout::document_size(&view), images);
    (pre, artifact, ids)
}

/// An image of `width` by `height`, its pixels never read.
fn image(width: u32, height: u32) -> dom::Image {
    dom::Image::Raster(gfx::ImageData::new(&b""[..]), gfx::Size::new(width, height))
}

/// `device` device pixels, in CSS px at `dsf`.
fn css(device: f32, dsf: f32) -> f32 {
    device / dsf
}

fn fills(artifact: &PaintArtifact) -> Vec<(DisplayItemKind, RectF, Color)> {
    artifact
        .items
        .iter()
        .map(|item| match item.op {
            PaintOp::DrawRect {
                rect,
                flags: PaintFlags { color, .. },
            } => {
                assert_eq!(rect, item.visual_rect);
                (item.kind, rect, color)
            }
            ref op => panic!("unexpected op {op:?}"),
        })
        .collect()
}

#[test]
fn backgrounds_in_tree_order_with_the_body_on_the_canvas() {
    let artifact = paint_html(
        "<style>html, body { margin: 0 }</style>
         <body style='background-color: blue'>
           <div style='height: 10.4px; background-color: red'>
             <div style='height: 5px; width: 20.6px; background-color: lime'></div>
           </div>
           <div style='height: 10px'></div>
           <div style='height: 0; background-color: red'></div>
           <div style='height: 10px; background-color: transparent'></div>
           <div style='height: 1000px; background-color: black'></div>",
    );
    let (red, lime, blue) = (
        Color::rgb(255, 0, 0),
        Color::rgb(0, 255, 0),
        Color::rgb(0, 0, 255),
    );
    assert_eq!(
        fills(&artifact),
        [
            // The body's background covers the whole (taller than the
            // viewport) document, and the body paints none of its own.
            (
                DisplayItemKind::DocumentBackground,
                RectF::new(0.0, 0.0, 800.0, 1030.0),
                blue
            ),
            // Edges snap to whole pixels.
            (
                DisplayItemKind::BoxDecorationBackground,
                RectF::new(0.0, 0.0, 800.0, 10.0),
                red
            ),
            (
                DisplayItemKind::BoxDecorationBackground,
                RectF::new(0.0, 0.0, 21.0, 5.0),
                lime
            ),
            // Nothing for the empty, transparent or zero-height boxes.
            (
                DisplayItemKind::BoxDecorationBackground,
                RectF::new(0.0, 30.0, 800.0, 1000.0),
                Color::BLACK
            ),
        ]
    );
    assert_eq!(artifact.chunks.len(), 1);
    assert_eq!(artifact.chunks[0].items, 0..4);
    assert_eq!(artifact.chunks[0].state, PropertyTreeState::ROOT);
}

#[test]
fn nothing_to_paint() {
    let artifact = paint_html("<p></p><div style='color: transparent'>invisible text</div>");
    assert!(artifact.items.is_empty());
    assert!(artifact.chunks.is_empty());
    let list = paint::to_display_list(&artifact, &paint::PropertyTrees::root_only());
    assert!(list.is_empty());
}

fn paths(artifact: &PaintArtifact) -> Vec<(Color, Vec<Vec<gfx::PointF>>)> {
    artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawPath { path, flags } => Some((flags.color, polygons(path))),
            _ => None,
        })
        .collect()
}

/// The polygons a path of closed straight-edged subpaths is made of.
fn polygons(path: &gfx::Path) -> Vec<Vec<gfx::PointF>> {
    let mut out: Vec<Vec<gfx::PointF>> = Vec::new();
    for element in path.elements() {
        match *element {
            gfx::PathElement::MoveTo(p) => out.push(vec![p]),
            gfx::PathElement::LineTo(p) => out.last_mut().unwrap().push(p),
            gfx::PathElement::Close => {}
            element => panic!("{element:?}"),
        }
    }
    out
}

#[test]
fn borders_are_trapezoids_grouped_by_color() {
    let p = gfx::PointF::new;
    let (red, blue) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 255));
    // Top and bottom red, left and right blue, left 0 wide.
    let artifact = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 10px; height: 10px; border: solid red; border-width: 1px 2px 3px 0;
                     border-right-color: blue; border-left-color: blue; background-color: lime'></div>",
    );
    // The background first (under the border), then one path per color in
    // side order.
    assert_eq!(
        artifact.items[0].visual_rect,
        RectF::new(0.0, 0.0, 12.0, 14.0)
    );
    assert_eq!(
        paths(&artifact),
        [
            (
                red,
                vec![
                    vec![p(0.0, 0.0), p(12.0, 0.0), p(10.0, 1.0), p(0.0, 1.0)],
                    vec![p(12.0, 14.0), p(0.0, 14.0), p(0.0, 11.0), p(10.0, 11.0)],
                ]
            ),
            (
                blue,
                vec![vec![
                    p(12.0, 0.0),
                    p(12.0, 14.0),
                    p(10.0, 11.0),
                    p(10.0, 1.0)
                ]]
            ),
        ]
    );
}

#[test]
fn invisible_borders_paint_nothing() {
    let artifact = paint_html(
        "<div style='border: 5px solid transparent'></div>
         <div style='border-width: 5px; border-color: red'></div>
         <div style='border: 0 solid red'></div>",
    );
    assert!(paths(&artifact).is_empty());
}

#[test]
fn border_color_defaults_to_the_element_color() {
    let artifact = paint_html(
        "<div style='color: green'><div style='width: 0; border: 50px solid'></div></div>",
    );
    let found = paths(&artifact);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, Color::rgb(0, 128, 0));
    // A uniform border is one ring: the outer rect and the inner one (here
    // a point, the middle of the 100x100 square) as a hole.
    assert_eq!(found[0].1.len(), 2);
}

/// A float is painted once, after the backgrounds of the blocks around
/// it, the later ones too (CSS 2 Appendix E step 5).
#[test]
fn a_float_is_painted_once_after_the_blocks_backgrounds() {
    let (_, artifact, ids) = paint_html_at(
        "<style>html, body { margin: 0 }</style>
         <div id=float style='float: left; width: 50px; height: 50px; background: green'></div>
         <div id=after style='height: 10px; background: blue'></div>",
        &Default::default(),
        1.0,
    );
    let clients: Vec<Option<NodeId>> = artifact.items.iter().map(|i| i.client).collect();
    assert_eq!(clients, [Some(ids["after"]), Some(ids["float"])]);
}

/// A float in a line is painted in the float phase too, not with the line
/// (its atomic inlines are, even one after it).
#[test]
fn a_float_in_a_line_is_painted_before_the_lines() {
    let (_, artifact, ids) = paint_html_at(
        "<style>html, body { margin: 0 }</style>
         <div id=para style='font: 20px/1 Ahem; background: blue'>X<span
           id=float style='float: left; width: 20px; height: 20px; background: green'></span><span
           id=atomic style='display: inline-block; width: 10px; height: 10px; background: red'></span></div>",
        &Default::default(),
        1.0,
    );
    let clients: Vec<Option<NodeId>> = artifact.items.iter().map(|i| i.client).collect();
    assert_eq!(
        clients,
        [
            Some(ids["para"]),
            Some(ids["float"]),
            Some(ids["para"]),
            Some(ids["atomic"])
        ]
    );
}

/// A pseudo-element's text is painted as its element's, where it comes:
/// `::before` before the element's own text.
#[test]
fn generated_text_is_painted_as_its_elements() {
    let (_, artifact, ids) = paint_html_at(
        "<style>html, body { margin: 0 } #d::before { content: 'A'; color: red }</style>
         <div id=d style='font: 20px/1 Ahem'>X</div>",
        &Default::default(),
        1.0,
    );
    let texts: Vec<(Option<NodeId>, usize)> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawGlyphs { glyphs, flags, .. } => Some((
                item.client,
                glyphs.len() * usize::from(flags.color.r == 255),
            )),
            _ => None,
        })
        .collect();
    // The red `A`, then the black `X`.
    assert_eq!(texts, [(Some(ids["d"]), 1), (Some(ids["d"]), 0)]);
}

/// A list item's marker is painted as its list item's, in its own color,
/// first on the item's first line: an outside one as its first atomic
/// inline, an inside one as its first inline box.
#[test]
fn markers_are_painted_as_their_list_items() {
    let (_, artifact, ids) = paint_html_at(
        "<style>html, body { margin: 0 } ol { padding-left: 100px; font: 20px/1 Ahem }
           ::marker { color: red }</style>
         <ol><li id=a>X<li id=b style='list-style-position: inside'>X</ol>",
        &Default::default(),
        1.0,
    );
    let texts: Vec<(Option<NodeId>, usize, bool)> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawGlyphs { glyphs, flags, .. } => {
                Some((item.client, glyphs.len(), flags.color.r == 255))
            }
            _ => None,
        })
        .collect();
    // The red `1. `, then `X`; the red `2. `, then `X`.
    assert_eq!(
        texts,
        [
            (Some(ids["a"]), 3, true),
            (Some(ids["a"]), 1, false),
            (Some(ids["b"]), 3, true),
            (Some(ids["b"]), 1, false)
        ]
    );
}

/// What `text-overflow` hides isn't painted: a run cut by the ellipsis
/// paints the glyphs before the cut, one wholly past it nothing at all.
/// The glyphs are where their box is.
#[test]
fn what_text_overflow_hides_is_not_painted() {
    let artifact = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='margin-left: 30px; width: 110px; font: 20px/1 Ahem; overflow: hidden;
                     text-overflow: ellipsis; white-space: nowrap'>X<span>XXXXXXX</span>X</div>",
    );
    let texts: Vec<(usize, f32)> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawGlyphs {
                glyphs, positions, ..
            } => Some((glyphs.len(), positions[0].x)),
            _ => None,
        })
        .collect();
    // The block's square, three of the span's, then the ellipsis (one
    // square in Ahem); the last square isn't painted.
    assert_eq!(texts, [(1, 30.0), (3, 50.0), (1, 110.0)]);
}

#[test]
fn text_after_all_backgrounds_with_decorations_around_it() {
    let artifact = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='font: 20px/1 Ahem; color: lime; background-color: red;
                     text-decoration: underline line-through'>X</div>
         <div style='height: 10px; background-color: blue'></div>",
    );
    let kinds: Vec<DisplayItemKind> = artifact.items.iter().map(|i| i.kind).collect();
    assert_eq!(
        kinds,
        [
            DisplayItemKind::BoxDecorationBackground,
            DisplayItemKind::BoxDecorationBackground,
            // Underline under the glyphs, line-through over them.
            DisplayItemKind::TextDecoration,
            DisplayItemKind::Text,
            DisplayItemKind::TextDecoration,
        ]
    );
    let PaintOp::DrawGlyphs {
        glyphs,
        positions,
        flags,
        size,
        rotations,
        ..
    } = &artifact.items[3].op
    else {
        panic!("{:?}", artifact.items[3].op)
    };
    assert_eq!(
        (glyphs.len(), *size, flags.color),
        (1, 20.0, Color::rgb(0, 255, 0))
    );
    // Filled, unturned.
    assert_eq!(flags.style, compositor_paint::PaintStyle::Fill);
    assert!(rotations.is_empty());
    // On the baseline: Ahem's ascent is 0.8em.
    assert_eq!(positions[0], gfx::PointF::new(0.0, 16.0));
    // The decorations span the run and sit below / across the baseline.
    let (under, through) = (artifact.items[2].visual_rect, artifact.items[4].visual_rect);
    assert_eq!((under.x, under.width), (0.0, 20.0));
    assert!(under.y >= 16.0, "{under:?}");
    assert!(through.y < 16.0 && through.y > 0.0, "{through:?}");
}

/// The decorations' ops in `html` (one text run in Ahem, 20px, line
/// height 1, at the top: its ascent 16px), in paint order.
fn decoration_ops(html: &str) -> Vec<PaintOp> {
    paint_html(&format!(
        "<style>html, body {{ margin: 0 }} div {{ font: 20px/1 Ahem }}</style>{html}"
    ))
    .items
    .iter()
    .filter(|item| item.kind == DisplayItemKind::TextDecoration)
    .map(|item| item.op.clone())
    .collect()
}

fn rect_op(op: &PaintOp) -> (RectF, Color) {
    match op {
        PaintOp::DrawRect { rect, flags } => (*rect, flags.color),
        op => panic!("{op:?}"),
    }
}

/// Where Blink puts each line (`TextDecorationOffset`, in Ahem's 16px of
/// ascent from the text's top): an `auto` underline a tenth of the font
/// size thick, below the baseline by half that (at least 1px); one moved
/// by `text-underline-offset` without the gap; an overline on the text's
/// top, a line-through two thirds down the ascent; each line's top at a
/// whole pixel, as long as the run.
#[test]
fn decorations_are_where_blink_puts_them() {
    let ops = decoration_ops(
        "<div style='text-decoration: underline overline line-through; color: red'>XXXXX</div>",
    );
    let red = Color::rgb(255, 0, 0);
    assert_eq!(
        ops.iter().map(rect_op).collect::<Vec<_>>(),
        [
            (RectF::new(0.0, 17.0, 100.0, 2.0), red),
            (RectF::new(0.0, -2.0, 100.0, 2.0), red),
            (RectF::new(0.0, 10.0, 100.0, 2.0), red),
        ]
    );
    let ops = decoration_ops(
        "<div style='text-decoration: underline 3px; text-underline-offset: 4px'>X</div>
         <div style='text-decoration: underline 25%'>X</div>
         <div style='text-decoration: underline -5px'>X</div>
         <div style='text-decoration: underline; text-underline-position: under'>X</div>
         <div style='text-decoration: underline from-font; text-underline-position: from-font'>X</div>
         <div style='text-decoration: underline 2.7px'>X</div>",
    );
    let rects: Vec<RectF> = ops.iter().map(|op| rect_op(op).0).collect();
    assert_eq!(
        rects,
        [
            // At the baseline moved by 4px, no gap.
            RectF::new(0.0, 20.0, 20.0, 3.0),
            // A quarter of the font size, 5px: a gap of 3.
            RectF::new(0.0, 39.0, 20.0, 5.0),
            // At least 1px thick.
            RectF::new(0.0, 57.0, 20.0, 1.0),
            // Below the em box, which Ahem's ascent and descent fill.
            RectF::new(0.0, 81.0, 20.0, 2.0),
            // Ahem's: 2.66px below the baseline, 0.4px thick (at least 1).
            RectF::new(0.0, 99.0, 20.0, 1.0),
            // A length rounded to whole pixels: 3px, a gap of 2.
            RectF::new(0.0, 118.0, 20.0, 3.0),
        ]
    );
}

/// `double` is two lines a thickness and a pixel apart (below for an
/// underline); `dotted` and `dashed` a stroke along the middle, dashes as
/// Blink sizes them (2px dots are square dashes as long as they are wide;
/// dashes three widths long, two apart, below 3px); `wavy` a stroked curve
/// cut to the run.
#[test]
fn decoration_styles() {
    let ops = decoration_ops(
        "<div style='text-decoration: underline double'>XXXXX</div>
         <div style='text-decoration: underline dotted'>XXXXX</div>
         <div style='text-decoration: underline dashed'>XXXXX</div>
         <div style='text-decoration: underline wavy'>XXXXX</div>
         <div style='text-decoration: underline dotted 4px'>XXXXX</div>
         <div style='text-decoration: underline dashed 3px'>XXXXX</div>",
    );
    assert_eq!(rect_op(&ops[0]).0, RectF::new(0.0, 17.0, 100.0, 2.0));
    assert_eq!(rect_op(&ops[1]).0, RectF::new(0.0, 20.0, 100.0, 2.0));
    let line = |op: &PaintOp| match op {
        PaintOp::DrawLine { p0, p1, flags } => (*p0, *p1, flags.clone()),
        op => panic!("{op:?}"),
    };
    let (p0, p1, dotted) = line(&ops[2]);
    // In the second line box, 20px down.
    assert_eq!((p0, p1), (PointF::new(0.0, 38.0), PointF::new(100.0, 38.0)));
    assert_eq!(
        dotted.style,
        compositor_paint::PaintStyle::Stroke { width: 2.0 }
    );
    assert_eq!(dotted.dash, compositor_paint::Dash::new(&[2.0, 2.0], 0.0));
    let (_, _, dashed) = line(&ops[3]);
    assert_eq!(dashed.cap, gfx::LineCap::Butt);
    // 6px dashes, the gaps fitted to 100px: 10 dashes.
    assert_eq!(
        dashed.dash,
        compositor_paint::Dash::new(&[6.0, 40.0 / 9.0], 0.0)
    );
    match &ops[4] {
        PaintOp::DrawClipped { polygon, op } => {
            let PaintOp::DrawPath { path, flags } = &**op else {
                panic!("{op:?}")
            };
            assert_eq!(
                flags.style,
                compositor_paint::PaintStyle::Stroke { width: 2.0 }
            );
            // Cut to the run's length; from a wavelength (11px for 2px)
            // before it, as Blink's tile.
            let xs: Vec<f32> = polygon.iter().map(|p| p.x).collect();
            assert_eq!(xs, [0.0, 100.0, 100.0, 0.0]);
            assert_eq!(path.bounds().map(|b| b.x), Some(-11.0));
        }
        op => panic!("{op:?}"),
    }
    // Wider dots are round, inset by half a dot, about two widths apart.
    let (p0, p1, dots) = line(&ops[5]);
    assert_eq!((p0.x, p1.x), (2.0, 98.0));
    assert_eq!(dots.cap, gfx::LineCap::Round);
    assert_eq!(
        dots.dash,
        compositor_paint::Dash::new(&[0.0, 8.0 - 1e-2], 0.0)
    );
    // An odd width's middle on a half pixel.
    let (p0, _, dashed) = line(&ops[6]);
    assert_eq!(p0.y, 119.5);
    assert_eq!(dashed.dash, compositor_paint::Dash::new(&[6.0, 3.4], 0.0));
}

/// A decoration is its decorating box's: its color (`currentcolor` its
/// `color`, not the text's), style and thickness on the text of the boxes
/// in it; `-webkit-text-fill-color` fills the glyphs alone.
#[test]
fn decorations_are_their_decorating_boxs() {
    let artifact = paint_html(
        "<style>html, body { margin: 0 } div { font: 20px/1 Ahem }</style>
         <div style='color: red; text-decoration: underline dashed; -webkit-text-fill-color: lime'>
           X<span style='color: blue; text-decoration: overline 4px'>X</span></div>",
    );
    let ops: Vec<(DisplayItemKind, Color)> = artifact
        .items
        .iter()
        .map(|item| {
            let color = match &item.op {
                PaintOp::DrawRect { flags, .. }
                | PaintOp::DrawLine { flags, .. }
                | PaintOp::DrawGlyphs { flags, .. } => flags.color,
                op => panic!("{op:?}"),
            };
            (item.kind, color)
        })
        .collect();
    let (red, blue, lime) = (
        Color::rgb(255, 0, 0),
        Color::rgb(0, 0, 255),
        Color::rgb(0, 255, 0),
    );
    assert_eq!(
        ops,
        [
            (DisplayItemKind::TextDecoration, red),
            (DisplayItemKind::Text, lime),
            // The span's: the div's red underline, its own blue overline.
            (DisplayItemKind::TextDecoration, red),
            (DisplayItemKind::TextDecoration, blue),
            (DisplayItemKind::Text, lime),
        ]
    );
    let PaintOp::DrawRect { rect, .. } = &artifact.items[3].op else {
        panic!()
    };
    assert_eq!(rect.height, 4.0);
}

/// `text-shadow`: the glyphs' shadows first, then the underline's (each
/// shadow of all the lines) and the underline, the glyphs, the
/// line-through's and the line-through; each list's last shadow first,
/// moved, blurred by half its radius, its own color even on transparent
/// text, which itself paints nothing.
#[test]
fn text_shadows_are_under_the_text_and_its_lines() {
    let artifact = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='font: 20px/1 Ahem; color: transparent; text-decoration: underline line-through lime;
                     text-shadow: 1px 2px 4px red, blue 3px 3px'>X</div>",
    );
    let ops: Vec<(DisplayItemKind, Color, f32, RectF)> = artifact
        .items
        .iter()
        .map(|item| {
            let (color, blur) = match &item.op {
                PaintOp::DrawRect { flags, .. } | PaintOp::DrawGlyphs { flags, .. } => {
                    (flags.color, flags.blur)
                }
                op => panic!("{op:?}"),
            };
            (item.kind, color, blur, item.visual_rect)
        })
        .collect();
    let (red, blue, lime) = (
        Color::rgb(255, 0, 0),
        Color::rgb(0, 0, 255),
        Color::rgb(0, 255, 0),
    );
    let kinds: Vec<(DisplayItemKind, Color, f32)> = ops
        .iter()
        .map(|&(kind, color, blur, _)| (kind, color, blur))
        .collect();
    use DisplayItemKind::{Text, TextDecoration};
    assert_eq!(
        kinds,
        [
            (Text, blue, 0.0),
            (Text, red, 2.0),
            (TextDecoration, blue, 0.0),
            (TextDecoration, red, 2.0),
            (TextDecoration, lime, 0.0),
            (TextDecoration, blue, 0.0),
            (TextDecoration, red, 2.0),
            (TextDecoration, lime, 0.0),
        ]
    );
    // Moved, and spread by three times the blur's sigma.
    let underline = ops[4].3;
    assert_eq!(ops[2].3, RectF::new(3.0, underline.y + 3.0, 20.0, 2.0));
    assert_eq!(
        ops[3].3,
        RectF::new(1.0 - 6.0, underline.y + 2.0 - 6.0, 32.0, 14.0)
    );
    let PaintOp::DrawGlyphs { positions, .. } = &artifact.items[0].op else {
        panic!()
    };
    assert_eq!(positions[0], PointF::new(3.0, 19.0));
}

/// A filter is a layer composited through it, opacity and all, the filter
/// in the box's own space (its matrix set before the layer is saved) and
/// what it makes cut by the clips around the box (an ancestor's overflow
/// clip, the box's own `clip-path`), set before it too; the functions in
/// order, each an image filter (a drop shadow's sigma half its radius).
#[test]
fn filters_are_layers_cut_by_the_clips_around_them() {
    use compositor_paint::{ImageFilter, LayerFlags};
    let (view, _) = page::lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='overflow: hidden; height: 50px'>
           <div style='height: 10px; background-color: blue; opacity: 0.5; transform: scale(2);
                       clip-path: inset(1px); filter: invert(1) drop-shadow(red 1px 2px 6px)'></div>
         </div>",
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let artifact = paint::paint(&view, &pre, layout::document_size(&view), &());
    let list = paint::to_display_list(&artifact, &pre.trees);
    let ops = list.ops();
    let layer = ops
        .iter()
        .position(|op| matches!(op, PaintOp::SaveLayer { .. }))
        .expect("a layer");
    let PaintOp::SaveLayer { flags, .. } = &ops[layer] else {
        unreachable!()
    };
    let mut invert = [0.0; 20];
    for row in 0..4 {
        invert[row * 5 + row] = if row == 3 { 1.0 } else { -1.0 };
        if row < 3 {
            invert[row * 5 + 4] = 1.0;
        }
    }
    assert_eq!(
        *flags,
        LayerFlags {
            alpha: 0.5,
            image_filter: Some(ImageFilter::Chain(
                [
                    ImageFilter::ColorMatrix(invert),
                    ImageFilter::DropShadow {
                        offset: (1.0, 2.0),
                        sigma: 3.0,
                        color: Color::rgb(255, 0, 0),
                        shadow_only: false,
                    },
                ]
                .into()
            )),
            ..LayerFlags::default()
        }
    );
    // Before it: the scroller's clip, the clip-path's, then the box's
    // own space, scaled about its center (the filter scales with it).
    let names: Vec<&str> = ops[..layer]
        .iter()
        .map(|op| match op {
            PaintOp::Save => "save",
            PaintOp::SetMatrix { .. } => "matrix",
            PaintOp::ClipRect { .. } => "clip",
            PaintOp::ClipPath { .. } => "path",
            _ => "other",
        })
        .collect();
    assert_eq!(
        names,
        ["save", "matrix", "clip", "matrix", "clip", "path", "matrix"]
    );
    let PaintOp::SetMatrix { transform } = &ops[layer - 1] else {
        unreachable!()
    };
    assert_eq!(transform.0[..4], [2.0, 0.0, 0.0, 2.0]);
}

#[test]
fn effects_span_chunks_and_transforms_set_the_matrix() {
    let (view, _) = page::lay_out(
        "<style>html, body { margin: 0 }</style>
         <div style='opacity: 0.5; height: 10px; background-color: red'>
           <div style='height: 10px; background-color: blue; transform: translate(5px, 0)'></div>
         </div>
         <div style='overflow: hidden; height: 10px'><div style='height: 20px; background-color: lime'></div></div>",
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    // Root, the transform and the clipping box's scroll translation (it is
    // a scroll container, if only a script or a `#fragment` can scroll
    // it); root, the effect; root, the clip.
    assert_eq!(
        (
            pre.trees.transform.len(),
            pre.trees.effect.len(),
            pre.trees.clip.len()
        ),
        (3, 2, 2)
    );
    assert_eq!(pre.trees.scroll.len(), 1);
    let artifact = paint::paint(&view, &pre, layout::document_size(&view), &());
    let list = paint::to_display_list(&artifact, &pre.trees);
    let names: Vec<&str> = list
        .ops()
        .iter()
        .map(|op| match op {
            PaintOp::SaveLayerAlpha { .. } => "layer",
            PaintOp::Save => "save",
            PaintOp::Restore => "restore",
            PaintOp::SetMatrix { .. } => "matrix",
            PaintOp::ClipRect { .. } => "clip",
            PaintOp::DrawRect { .. } => "rect",
            _ => "other",
        })
        .collect();
    assert_eq!(
        names,
        [
            // The clipped lime child is in normal flow of the root context;
            // the opacity group, a stacking context, paints after it.
            "save", "matrix", "clip", "matrix", "rect", "restore",
            // One layer for both chunks of the group.
            "layer", "rect", "save", "matrix", "rect", "restore", "restore",
        ]
    );
}

#[test]
fn edges_snap_to_device_pixels() {
    let html = "<style>html, body { margin: 0 }</style>
        <div style='margin: 0.25px 0 0 10.25px; width: 20.5px; height: 10px; background-color: lime'></div>";
    // x 10.25 to 30.75, y 0.25 to 10.25.
    for (dsf, x0, x1, y0, y1) in [
        (1.0, 10.0, 31.0, 0.0, 10.0),
        (2.0, 21.0, 62.0, 1.0, 21.0),
        (1.5, 15.0, 46.0, 0.0, 15.0),
    ] {
        let (_, artifact, _) = paint_html_at(html, &Default::default(), dsf);
        let (_, rect, _) = fills(&artifact)[0];
        let (x0, x1, y0, y1) = (css(x0, dsf), css(x1, dsf), css(y0, dsf), css(y1, dsf));
        assert_eq!(rect, RectF::new(x0, y0, x1 - x0, y1 - y0), "at {dsf}x");
        assert_eq!(artifact.device_scale_factor, dsf);
    }
}

#[test]
fn border_widths_snap_down_to_whole_device_pixels_but_not_to_none() {
    let p = gfx::PointF::new;
    // 1px, 0.25px, 2px and none: at 1.5x 1.5, 0.375, 3 and 0 device px, so
    // 1, 1, 3 and 0.
    let (_, artifact, _) = paint_html_at(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 10px; height: 10px; border: solid red; border-width: 1px 0.25px 2px 0'></div>",
        &Default::default(),
        1.5,
    );
    let d = |device: f32| css(device, 1.5);
    // The border box, 10.25 by 13: 15.375 by 19.5 device px, snapped to 15
    // by 20.
    let (outer, inner) = (
        RectF::new(0.0, 0.0, d(15.0), d(20.0)),
        RectF::new(0.0, d(1.0), d(15.0) - d(1.0), d(20.0) - d(1.0) - d(3.0)),
    );
    assert_eq!(
        paths(&artifact),
        [(
            Color::rgb(255, 0, 0),
            vec![
                vec![
                    p(outer.x, outer.y),
                    p(outer.right(), outer.y),
                    p(outer.right(), outer.bottom()),
                    p(outer.x, outer.bottom()),
                ],
                vec![
                    p(inner.x, inner.y),
                    p(inner.x, inner.bottom()),
                    p(inner.right(), inner.bottom()),
                    p(inner.right(), inner.y),
                ],
            ]
        )]
    );
}

#[test]
fn the_baseline_snaps_to_a_device_pixel() {
    let html = "<style>html, body { margin: 0 }</style>
        <div style='margin-top: 10.25px; font: 20px/1 Ahem'>X</div>";
    // The baseline: 10.25 plus Ahem's ascent, 16.
    for (dsf, baseline) in [(1.0, 26.0), (2.0, 26.5)] {
        let (_, artifact, _) = paint_html_at(html, &Default::default(), dsf);
        let y = artifact
            .items
            .iter()
            .find_map(|item| match &item.op {
                PaintOp::DrawGlyphs { positions, .. } => Some(positions[0].y),
                _ => None,
            })
            .expect("the glyph");
        assert_eq!(y, baseline, "at {dsf}x");
    }
}

#[test]
fn clips_and_scroll_offsets_snap_to_device_pixels() {
    let html = "<style>html, body { margin: 0 }</style>
        <div id=box style='margin-left: 10.25px; width: 100px; height: 50px; overflow: auto'>
          <div style='height: 300px'></div>
        </div>";
    for (dsf, clip_x, scrolled) in [(1.0, 10.0, 10.0), (2.0, 10.5, 10.5)] {
        let (pre, _, ids) = paint_html_at(html, &Default::default(), dsf);
        let scroll: paint::ScrollOffsets = [(ids["box"], gfx::PointF::new(0.0, 10.25))]
            .into_iter()
            .collect();
        let (pre_scrolled, _, _) = paint_html_at(html, &scroll, dsf);
        let node = &pre.trees.scroll(paint::ScrollNodeId(0));
        let clip = pre.trees.clip[node.overflow_clip.0].rect.expect("a clip");
        assert_eq!(clip.x, clip_x, "at {dsf}x");
        let node = &pre_scrolled.trees.scroll(paint::ScrollNodeId(0));
        let translation = pre_scrolled.trees.transform[node.scroll_translation.0]
            .matrix
            .as_translation()
            .expect("a translation");
        assert_eq!(translation.y, -scrolled, "at {dsf}x");
    }
}

/// The kind and element `id` of each item painted for an element with one,
/// in paint order.
fn painted(html: &str) -> Vec<(DisplayItemKind, String)> {
    let (_, artifact, ids) = paint_html_at(html, &Default::default(), 1.0);
    artifact
        .items
        .iter()
        .filter_map(|item| {
            let client = item.client?;
            let (name, _) = ids.iter().find(|(_, id)| **id == client)?;
            Some((item.kind, name.clone()))
        })
        .collect()
}

fn background(id: &str) -> (DisplayItemKind, String) {
    (DisplayItemKind::BoxDecorationBackground, id.into())
}

fn text(id: &str) -> (DisplayItemKind, String) {
    (DisplayItemKind::Text, id.into())
}

#[test]
fn flex_items_paint_whole_with_the_lines() {
    let order = painted(
        "<style>body { margin: 0; font: 20px/1 Ahem } .c div { width: 40px; height: 40px }</style>
         <div class=c style='display: flex'>
           <div id=a style='background-color: red; margin-right: -20px'>A</div>
           <div id=b style='background-color: blue'>B</div>
         </div>
         <div id=after style='height: 20px; margin-top: -30px; background-color: green'></div>",
    );
    // The block after them paints its background with the others; each
    // item its background then its text, in turn.
    assert_eq!(
        order,
        [
            background("after"),
            background("a"),
            text("a"),
            background("b"),
            text("b"),
        ]
    );
}

/// `visibility: hidden`: laid out, nothing of its own painted, its
/// background, border, outline, text and inline boxes alike; descendants
/// that are `visible` are.
#[test]
fn hidden_boxes_paint_only_their_visible_descendants() {
    let order = painted(
        "<style>body { margin: 0; font: 10px/1 Ahem }
           div, p { height: 10px; margin: 0; background: red; outline: 1px solid red }</style>
         <div id=a style='visibility: hidden; border: 1px solid'>
           <div id=b style='visibility: visible'>B</div><p id=c>C</p></div>
         <p id=d>D<span id=e style='visibility: hidden; background: red'>E</span><span id=f
           style='background: lime'>F</span></p>",
    );
    let outline = |id: &str| (DisplayItemKind::Outline, id.to_owned());
    assert_eq!(
        order,
        [
            background("b"),
            background("d"),
            text("b"),
            text("d"),
            background("f"),
            text("f"),
            outline("b"),
            outline("d"),
        ]
    );
}

#[test]
fn flex_items_paint_in_order_modified_order() {
    let order = painted(
        "<style>div div { width: 10px; height: 10px; background-color: red }</style>
         <div style='display: flex'>
           <div id=a style='order: 1'></div><div id=b></div>
         </div>",
    );
    assert_eq!(order, [background("b"), background("a")]);
}

#[test]
fn stacking_contexts_paint_by_z_index() {
    let order = painted(
        "<style>div div, #block { width: 10px; height: 10px; background-color: red }</style>
         <div id=c style='display: flex; background-color: blue'>
           <div id=positive style='z-index: 2'></div>
           <div id=negative style='z-index: -1'></div>
           <div id=zero style='z-index: 0'></div>
           <div id=plain></div>
           <div id=translucent style='opacity: 0.5'></div>
           <div id=one style='z-index: 1'></div>
           <div id=also-negative style='z-index: -1'></div>
         </div>
         <div id=block style='z-index: 5'></div>",
    );
    // The negative ones under every block background, `#c`'s too; the
    // items with no stacking context with the lines; then 0 and `auto` in
    // tree order, then the positive ones by `z-index`. `#block` is no flex
    // item: its `z-index` doesn't apply.
    assert_eq!(
        order,
        [
            background("negative"),
            background("also-negative"),
            background("c"),
            background("block"),
            background("plain"),
            background("zero"),
            background("translucent"),
            background("one"),
            background("positive"),
        ]
    );
}

/// An out-of-flow box paints where its parent places it in tree order,
/// not after its containing block's in-flow boxes: in the stacking context
/// of the elements around it there, under their opacity (Blink's
/// `PaintLayer` tree follows the layout tree), though its containing block
/// is outside.
#[test]
fn out_of_flow_boxes_paint_where_the_tree_places_them() {
    let (_, artifact, ids) = paint_html_at(
        "<style>div, i, b { display: block; width: 10px; height: 10px; background-color: red }
           .w { width: auto; height: auto; background-color: transparent }</style>
         <div class=w style='position: relative'>
           <div class=w><div id=abs style='position: absolute'></div></div>
           <div id=translucent style='opacity: 0.5'></div>
           <div id=rel style='position: relative'></div>
           <p><b style='float: left'></b><span><i id=in-line style='position: absolute'></i></span><b id=inline-block style='display: inline-block; position: relative'></b></p>
         </div>
         <div id=faded style='opacity: 0.5'>
           <div class=w><div id=escaped style='position: absolute'></div></div>
           <div id=fixed style='position: fixed'></div>
         </div>
         <div id=after style='position: relative'></div>",
        &Default::default(),
        1.0,
    );
    let painted: Vec<(String, paint::EffectNodeId)> = artifact
        .items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            let (name, _) = ids.iter().find(|(_, id)| Some(**id) == item.client)?;
            let chunk = artifact
                .chunks
                .iter()
                .find(|chunk| chunk.items.contains(&index))
                .expect("in a chunk");
            Some((name.clone(), chunk.state.effect))
        })
        .collect();
    let names: Vec<&str> = painted.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "abs",
            "translucent",
            "rel",
            "in-line",
            "inline-block",
            "faded",
            "escaped",
            "fixed",
            "after"
        ]
    );
    let effect = |name: &str| painted.iter().find(|(n, _)| n == name).unwrap().1;
    assert_ne!(effect("faded"), paint::EffectNodeId(0));
    assert_eq!(
        (effect("escaped"), effect("fixed")),
        (effect("faded"), effect("faded"))
    );
    assert_eq!(effect("after"), paint::EffectNodeId(0));
}

#[test]
fn a_stacking_context_in_a_flex_item_is_its_parents() {
    let order = painted(
        "<style>div div { width: 10px; height: 10px; background-color: red }</style>
         <div style='display: flex'>
           <div id=item><div id=inner style='opacity: 0.5'></div></div>
           <div id=next></div>
         </div>",
    );
    // Not painted with the item, but after every item, with the stacking
    // contexts of the stacking context the item is in.
    assert_eq!(
        order,
        [background("item"), background("next"), background("inner")]
    );
}

#[test]
fn a_negative_z_index_is_over_its_stacking_contexts_own_background() {
    let order = painted(
        "<style>div div { width: 10px; height: 10px; background-color: red }</style>
         <div id=root style='display: flex; opacity: 0.5; background-color: blue'>
           <div id=item></div><div id=negative style='z-index: -1'></div>
         </div>",
    );
    assert_eq!(
        order,
        [
            background("root"),
            background("negative"),
            background("item"),
        ]
    );
}

#[test]
fn grid_items_paint_whole_and_by_z_index() {
    // Both items in the same cell: the later one covers the earlier one's
    // text, unless `z-index` says otherwise.
    let order = painted(
        "<style>body { margin: 0; font: 20px/1 Ahem } .g div { grid-area: 1 / 1; background-color: red }</style>
         <div class=g style='display: grid; grid-template-columns: 40px'>
           <div id=a>A</div><div id=b>B</div><div id=raised style='z-index: 1'>C</div>
         </div>",
    );
    assert_eq!(
        order,
        [
            background("a"),
            text("a"),
            background("b"),
            text("b"),
            background("raised"),
            text("raised"),
        ]
    );
}

#[test]
fn inline_blocks_paint_whole_where_they_come_in_their_line() {
    let order = painted(
        "<style>body { margin: 0; font: 20px/1 Ahem } span { display: inline-block }</style>
         <div id=d style='background-color: gray'><b id=before style='background-color: blue'>A</b><i
           id=around style='background-color: blue'><span
           id=ib style='background-color: red'><div id=inner style='background-color: green'>B</div></span></i><b
           id=after style='background-color: blue'>C</b><span
           id=faded style='opacity: 0.5; background-color: red'>D</span></div>
         <div id=later style='background-color: gray'>E</div>",
    );
    assert_eq!(
        order,
        [
            // The blocks' backgrounds, not the inline-blocks'.
            background("d"),
            background("later"),
            // The line in order: `#ib` whole, its block's background and
            // text, between the inline elements around it.
            background("before"),
            text("before"),
            // The element it is in first.
            background("around"),
            background("ib"),
            background("inner"),
            text("inner"),
            background("after"),
            text("after"),
            text("later"),
            // A stacking context: after the lines.
            background("faded"),
            text("faded"),
        ]
    );
}

#[test]
fn positioned_boxes_paint_after_the_flow_as_if_stacking_contexts() {
    let order = painted(
        "<style>body { margin: 0; font: 20px/1 Ahem } div { height: 20px }</style>
         <div id=positioned style='position: relative; background-color: red'>A<div
           id=inner style='position: relative; background-color: blue'>B</div><div
           id=sunk style='position: relative; z-index: -1; background-color: green'></div><div
           id=plain style='background-color: yellow'>C</div></div>
         <div id=after style='background-color: gray'>D</div>",
    );
    assert_eq!(
        order,
        [
            // The negative `z-index` in it is its stacking context's: first.
            background("sunk"),
            // The flow after it, under it.
            background("after"),
            text("after"),
            // Then it, as if it were a stacking context, but for the
            // positioned box in it, which comes after it.
            background("positioned"),
            background("plain"),
            text("positioned"),
            text("plain"),
            background("inner"),
            text("inner"),
        ]
    );
}

/// Background images by URL, every one 20x10.
struct TestImages;

impl paint::BackgroundImages for TestImages {
    fn image(&self, url: &str) -> Option<(gfx::ImageData, gfx::Size)> {
        url.starts_with("http://t/")
            .then(|| (gfx::ImageData::new(url.as_bytes()), gfx::Size::new(20, 10)))
    }
}

/// What each item fills: a color, or a shader with its tile.
#[derive(Debug, PartialEq)]
enum Fill {
    Color(Color),
    Image(RectF, bool),
    Gradient(RectF, bool),
}

fn layer_fills(artifact: &PaintArtifact) -> Vec<(DisplayItemKind, RectF, Fill)> {
    use compositor_paint::{PaintShader, ShaderImage};
    artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawRect { rect, flags } => {
                assert_eq!(*rect, item.visual_rect);
                let fill = match &flags.shader {
                    None => Fill::Color(flags.color),
                    Some(shader) => {
                        let PaintShader::Tiled(shader) = &**shader else {
                            panic!("a background's shader is tiled");
                        };
                        let repeats = shader.repeat_x && shader.repeat_y;
                        match shader.image {
                            ShaderImage::Image(_) | ShaderImage::Record { .. } => {
                                Fill::Image(shader.tile, repeats)
                            }
                            ShaderImage::Gradient(_) => Fill::Gradient(shader.tile, repeats),
                        }
                    }
                };
                Some((item.kind, *rect, fill))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn background_layers_paint_bottom_up_in_their_boxes() {
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; border: 5px solid black; padding: 10px;
           background: url(http://t/a.png) no-repeat content-box,
                       url(http://t/b.png) right 3px bottom / auto 20px repeat-x border-box,
                       linear-gradient(red, blue) padding-box, padding-box green'></div>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &TestImages);
    let bg = DisplayItemKind::BoxDecorationBackground;
    let padding_box = RectF::new(5.0, 5.0, 120.0, 70.0);
    assert_eq!(
        layer_fills(&artifact),
        [
            // The color, clipped as the bottom layer is.
            (bg, padding_box, Fill::Color(Color::rgb(0, 128, 0))),
            // A gradient fills its positioning area, repeated.
            (bg, padding_box, Fill::Gradient(padding_box, true)),
            // 40x20 (the ratio kept), 3px from the right and at the
            // bottom of the border box; repeated across it only.
            (
                bg,
                RectF::new(0.0, 60.0, 130.0, 20.0),
                Fill::Image(RectF::new(87.0, 60.0, 40.0, 20.0), false)
            ),
            // The top layer last: at the content box's corner, drawn
            // only there.
            (
                bg,
                RectF::new(15.0, 15.0, 20.0, 10.0),
                Fill::Image(RectF::new(15.0, 15.0, 20.0, 10.0), false)
            ),
        ]
    );
    // An image not loaded yet isn't painted; the rest are.
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    assert_eq!(layer_fills(&artifact).len(), 2);
}

#[test]
fn the_root_background_covers_the_canvas_placed_in_the_root_box() {
    let html = "<html style='margin: 7px; height: 100px;
                  background: url(http://t/a.png) 10px 20px no-repeat'>
                <body style='margin: 0'></body>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &TestImages);
    assert_eq!(
        layer_fills(&artifact),
        [(
            DisplayItemKind::DocumentBackground,
            RectF::new(17.0, 27.0, 20.0, 10.0),
            Fill::Image(RectF::new(17.0, 27.0, 20.0, 10.0), false)
        )]
    );
    // From the body when the root has none, still placed in the root's
    // box; repeated over the whole canvas, not just the box.
    let html = "<html style='margin: 7px; height: 100px'>
                <body style='margin: 0; background: url(http://t/a.png) 10px 20px repeat-x'>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &TestImages);
    assert_eq!(
        layer_fills(&artifact),
        [(
            DisplayItemKind::DocumentBackground,
            RectF::new(0.0, 27.0, 800.0, 10.0),
            Fill::Image(RectF::new(17.0, 27.0, 20.0, 10.0), false)
        )]
    );
}

#[test]
fn an_inline_element_paints_its_background_on_each_line() {
    let html = "<style>html, body { margin: 0 } body { font: 10px/10px Ahem; width: 50px }</style>
                <span style='background: linear-gradient(red, blue)'>XXX XXX</span>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let gradients: Vec<RectF> = layer_fills(&artifact)
        .into_iter()
        .filter_map(|(_, rect, fill)| match fill {
            Fill::Gradient(tile, _) => {
                assert_eq!(tile, rect);
                Some(rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(gradients.len(), 2);
    assert_ne!(gradients[0].y, gradients[1].y);
}

#[test]
fn a_loaded_background_image_paints_its_element_anew() {
    let (view, ids) = page::lay_out(
        "<div id=d style='width: 20px; height: 10px; background: url(http://t/a.png)'></div>
         <div style='width: 20px; height: 10px; background: red'></div>
         <p>Some <span id=s style='background: url(http://t/b.png)'>text</span></p>",
        VIEWPORT,
    );
    let (d, span) = (ids["d"], ids["s"]);
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let size = layout::document_size(&view);
    let mut controller = paint::PaintController::default();
    let shaders = |artifact: &PaintArtifact| {
        layer_fills(artifact)
            .into_iter()
            .filter(|(_, _, fill)| matches!(fill, Fill::Image(..)))
            .count()
    };
    assert_eq!(shaders(&controller.paint(&view, &pre, size, &())), 0);
    // Nothing says it changed: the items of the last paint.
    assert_eq!(
        shaders(&controller.paint(&view, &pre, size, &TestImages)),
        0
    );
    controller.invalidate([d]);
    let artifact = controller.paint(&view, &pre, size, &TestImages);
    assert_eq!(shaders(&artifact), 1);
    // The other box's items are still the last paint's.
    let red = artifact
        .items
        .iter()
        .position(|item| {
            matches!(&item.op, PaintOp::DrawRect { flags, .. }
                if flags.color == Color::rgb(255, 0, 0))
        })
        .unwrap();
    assert!(artifact.cached.as_ref().unwrap().indices[red].is_some());
    // Once.
    let again = controller.paint(&view, &pre, size, &TestImages);
    assert!(
        again
            .cached
            .as_ref()
            .unwrap()
            .indices
            .iter()
            .all(Option::is_some)
    );
    // An inline element's background is among its line's items: its block
    // paints its lines anew (and `#d` its items from the last paint).
    let mut controller = paint::PaintController::default();
    controller.paint(&view, &pre, size, &());
    controller.paint(&view, &pre, size, &TestImages);
    controller.invalidate([span]);
    assert_eq!(
        shaders(&controller.paint(&view, &pre, size, &TestImages)),
        1
    );
}

/// A column's and a column group's backgrounds are painted in their cells
/// by their table, among its items: once their image loads, the table
/// paints anew (WPT's `table-backgrounds-bc-column-001.xht` caught it, its
/// image loading after the first paint).
#[test]
fn a_loaded_background_image_paints_its_columns_table_anew() {
    let (view, ids) = page::lay_out(
        "<style>td { width: 20px; height: 10px; padding: 0 }</style>
         <table><colgroup id=g style='background: url(http://t/g.png)'><col></colgroup>
           <col id=c style='background: url(http://t/c.png)'>
           <tr><td></td><td></td></tr></table>",
        VIEWPORT,
    );
    let (group, column) = (ids["g"], ids["c"]);
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let size = layout::document_size(&view);
    let shaders = |artifact: &PaintArtifact| {
        layer_fills(artifact)
            .into_iter()
            .filter(|(_, _, fill)| matches!(fill, Fill::Image(..)))
            .count()
    };
    for invalidated in [group, column] {
        let mut controller = paint::PaintController::default();
        assert_eq!(shaders(&controller.paint(&view, &pre, size, &())), 0);
        controller.invalidate([invalidated]);
        // Both: the table paints each of them again.
        assert_eq!(
            shaders(&controller.paint(&view, &pre, size, &TestImages)),
            2
        );
    }
}

#[test]
fn a_background_meets_its_snapped_border() {
    // 49.75px borders are painted 49px wide (snapped as border widths):
    // the padding box takes the 2px between them, not the half pixel
    // layout has.
    let html = "<style>html, body { margin: 0 }</style>
         <div style='box-sizing: border-box; width: 100px; height: 100px;
           border-top: 49.75px solid green; border-bottom: 49.75px solid green;
           background: green padding-box'></div>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    assert_eq!(
        layer_fills(&artifact),
        [(
            DisplayItemKind::BoxDecorationBackground,
            RectF::new(0.0, 49.0, 100.0, 2.0),
            Fill::Color(Color::rgb(0, 128, 0))
        )]
    );
}

#[test]
fn rounded_backgrounds_and_borders() {
    use gfx::{RRectF, SizeF};
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 80px; height: 30px; border: 10px solid green; border-radius: 20px;
           background: blue padding-box'></div>
         <div style='width: 100px; height: 50px; border-radius: 10px / 50%;
           background: url(http://t/a.png) no-repeat, red'></div>
         <div style='width: 80px; height: 30px; border: 10px solid green;
           border-top-color: blue; border-radius: 5px'></div>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &TestImages);
    let ops: Vec<&PaintOp> = artifact.items.iter().map(|item| &item.op).collect();
    let r = |w: f32, h: f32| SizeF::new(w, h);
    // The color inside the border: the radii less its width.
    assert!(matches!(ops[0], PaintOp::DrawRRect { rrect, flags }
        if *rrect == RRectF::new(RectF::new(10.0, 10.0, 80.0, 30.0), [r(10.0, 10.0); 4])
            && flags.color == Color::rgb(0, 0, 255)));
    // A uniform border: the ring between the curves.
    assert!(
        matches!(ops[1], PaintOp::DrawDRRect { outer, inner, clip: None, flags }
        if *outer == RRectF::new(RectF::new(0.0, 0.0, 100.0, 50.0), [r(20.0, 20.0); 4])
            && inner.rect == RectF::new(10.0, 10.0, 80.0, 30.0)
            && flags.color == Color::rgb(0, 128, 0))
    );
    // Elliptical corners, 10 by 25 (50% of 50): the color, then the image
    // over the whole rounded area, drawing nothing past its tile.
    let rounded = RRectF::new(RectF::new(0.0, 50.0, 100.0, 50.0), [r(10.0, 25.0); 4]);
    assert!(matches!(ops[2], PaintOp::DrawRRect { rrect, .. } if *rrect == rounded));
    assert!(matches!(ops[3], PaintOp::DrawRRect { rrect, flags }
        if *rrect == rounded
            && flags.shader.as_deref().is_some_and(|s| matches!(s,
                compositor_paint::PaintShader::Tiled(s) if s.decal && s.tile == RectF::new(0.0, 50.0, 20.0, 10.0)))));
    // Sides of two colors: each side's part, cut by its trapezoid.
    let sides: Vec<_> = ops[4..]
        .iter()
        .map(|op| match op {
            PaintOp::DrawDRRect {
                clip: Some(clip),
                flags,
                ..
            } => (clip.len(), flags.color),
            op => panic!("{op:?}"),
        })
        .collect();
    assert_eq!(sides.len(), 4);
    assert_eq!(sides[0].1, Color::rgb(0, 0, 255));
    assert!(
        sides[1..]
            .iter()
            .all(|(_, color)| *color == Color::rgb(0, 128, 0))
    );
}

#[test]
fn a_rounded_border_with_a_side_missing_is_its_sides() {
    // One color, but no left side: not the whole ring, whose left corners
    // would draw a sliver where there is no border.
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 80px; height: 30px; border: 10px solid green; border-left: none;
           border-radius: 20px'></div>";
    let artifact = paint_html(html);
    let clips: Vec<usize> = artifact
        .items
        .iter()
        .map(|item| match &item.op {
            PaintOp::DrawDRRect {
                clip: Some(clip), ..
            } => clip.len(),
            op => panic!("{op:?}"),
        })
        .collect();
    assert_eq!(clips, [4, 4, 4]);
}

#[test]
fn a_background_under_a_rounded_opaque_border_stops_halfway_into_it() {
    use gfx::{RRectF, SizeF};
    // Blink's `kBackgroundBleedShrinkBackground`: the border hides the
    // background's edge, so the background stops half a border width in,
    // and the border's anti-aliased curve can't let it show through. Not
    // when a side is missing or lets the background through.
    let html = "<style>html, body { margin: 0 }
                div { width: 80px; height: 30px; border: 10px solid green; border-radius: 20px;
                      background: blue }</style>
         <div></div>
         <div style='border-top-color: rgba(0, 128, 0, 0.5)'></div>
         <div style='border-left-width: 0'></div>
         <div style='border-style: dashed'></div>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let backgrounds: Vec<RRectF> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawRRect { rrect, flags } if flags.color == Color::rgb(0, 0, 255) => {
                Some(*rrect)
            }
            _ => None,
        })
        .collect();
    let border_box = |y: f32, width: f32| {
        RRectF::new(RectF::new(0.0, y, width, 50.0), [SizeF::new(20.0, 20.0); 4])
    };
    assert_eq!(
        backgrounds,
        [
            border_box(0.0, 100.0).inset(5.0, 5.0, 5.0, 5.0),
            border_box(50.0, 100.0),
            border_box(100.0, 90.0),
            // Dashes leave gaps.
            border_box(150.0, 100.0),
        ]
    );
}

#[test]
fn rounded_clips_go_into_the_display_list() {
    // `overflow: hidden` with rounded corners, and an image's content.
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 100px; border: 5px solid; border-radius: 30px;
           overflow: hidden'><div style='height: 200px; background: green'></div></div>
         <img src='a.png' style='width: 40px; height: 40px; border-radius: 50%'>";
    let (pre, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let rounded: Vec<_> = pre.trees.clip.iter().filter(|c| c.is_rounded()).collect();
    // The overflow clip, and the image's content box's curve (there even
    // before the image loads, as its `overflow: clip` cuts what it draws).
    assert_eq!(rounded.len(), 2);
    let clip = rounded[0].rrect().unwrap();
    // The padding box, the radii less the border.
    assert_eq!(clip.rect, RectF::new(5.0, 5.0, 100.0, 100.0));
    assert_eq!(clip.radii[0], SizeF::new(25.0, 25.0));
    let list = paint::to_display_list(&artifact, &pre.trees);
    assert!(
        list.ops()
            .iter()
            .any(|op| matches!(op, PaintOp::ClipRRect { rrect, .. } if *rrect == clip))
    );
}

/// A `visibility: hidden` image draws neither its box nor its image.
#[test]
fn a_hidden_image_draws_nothing() {
    let loaded = image(20, 20);
    let images = |visibility: &str| {
        let html = format!(
            "<img src='a.png' style='display: block; border: 1px solid; visibility: {visibility}'>"
        );
        let (_, artifact, _) =
            paint_html_with_images(&html, &Default::default(), 1.0, Some(&loaded), &());
        artifact
            .items
            .iter()
            .filter(|item| matches!(item.op, PaintOp::DrawImage { .. }))
            .count()
    };
    assert_eq!(images("visible"), 1);
    assert_eq!(images("hidden"), 0);
}

/// `object-fit` sizes a 40×20 image in a 100×100 content box and
/// `object-position` places it (css-images 3): `fill` stretches it,
/// `contain` keeps its ratio inside the box, `cover` over it, `none` keeps
/// its size, `scale-down` is the smaller of `none` and `contain`. What
/// reaches past the box is cut to it (the UA's `overflow: clip`), unless
/// `overflow` is `visible`.
#[test]
fn object_fit_sizes_and_object_position_places_an_image() {
    let loaded = image(40, 20);
    let html = "<style>html, body { margin: 0 } img { display: block; width: 100px; height: 100px }</style>
         <img src='a.png'>
         <img src='a.png' style='object-fit: contain'>
         <img src='a.png' style='object-fit: cover'>
         <img src='a.png' style='object-fit: none'>
         <img src='a.png' style='object-fit: scale-down'>
         <img src='a.png' style='object-fit: none; object-position: right 10px top 0'>
         <img src='a.png' style='object-fit: cover; overflow: visible'>";
    let (pre, artifact, _) =
        paint_html_with_images(html, &Default::default(), 1.0, Some(&loaded), &());
    let dsts: Vec<RectF> = artifact
        .items
        .iter()
        .filter_map(|item| match item.op {
            PaintOp::DrawImage { dst, .. } => Some(dst),
            _ => None,
        })
        .collect();
    let r = RectF::new;
    assert_eq!(
        dsts,
        [
            r(0.0, 0.0, 100.0, 100.0),
            r(0.0, 125.0, 100.0, 50.0),
            r(-50.0, 200.0, 200.0, 100.0),
            r(30.0, 340.0, 40.0, 20.0),
            r(30.0, 440.0, 40.0, 20.0),
            r(50.0, 500.0, 40.0, 20.0),
            r(-50.0, 600.0, 200.0, 100.0),
        ]
    );
    // Only the `cover` one that clips has a clip: the others fit, and the
    // last shows what reaches past its box.
    let clips: Vec<RectF> = pre
        .trees
        .clip
        .iter()
        .filter_map(|c| c.rrect())
        .map(|c| c.rect)
        .filter(|rect| rect.width == 100.0 && rect.height == 100.0)
        .collect();
    assert_eq!(clips, [r(0.0, 200.0, 100.0, 100.0)]);
}

/// `overflow: clip` cuts at the padding box but makes no scroll node:
/// nothing scrolls it. With `visible` on the other axis it cuts one axis
/// only, without the corners' curve.
#[test]
fn overflow_clip_cuts_without_scrolling() {
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; overflow: clip; border-radius: 10px'>
           <div style='height: 200px'></div></div>
         <div style='width: 100px; height: 50px; overflow: clip visible; border-radius: 10px'>
           <div style='width: 300px; height: 200px'></div></div>";
    let (pre, _, _) = paint_html_with(html, &Default::default(), 1.0, &());
    assert_eq!(pre.trees.scroll_nodes().count(), 0);
    let clips: Vec<gfx::RRectF> = pre.trees.clip.iter().filter_map(|c| c.rrect()).collect();
    // The first's padding box, with its curve; the second's, across only,
    // reaching as far as anything can down.
    assert!(clips.iter().any(
        |c| c.rect == RectF::new(0.0, 0.0, 100.0, 50.0) && c.radii[0] == SizeF::new(10.0, 10.0)
    ));
    assert!(clips.iter().any(|c| c.rect.x == 0.0
        && c.rect.width == 100.0
        && c.rect.y < -1.0e6
        && c.rect.height > 2.0e6
        && c.radii[0] == SizeF::new(0.0, 0.0)));
}

/// Paint containment cuts at the padding box on both axes, its
/// `overflow` visible, without scrolling.
#[test]
fn paint_containment_cuts_without_scrolling() {
    let loaded = image(40, 20);
    let html = "<style>html, body { margin: 0 } img, svg { display: block; padding: 5px }</style>
         <div style='width: 100px; height: 50px; contain: paint; border: 5px solid'>
           <div style='width: 300px; height: 200px'></div></div>
         <img src='a.png' style='width: 100px; height: 100px; object-fit: cover; overflow: visible; contain: paint'>
         <svg width=100 height=50 style='overflow: visible; contain: paint'><rect width=300 height=300 /></svg>";
    let (pre, _, _) = paint_html_with_images(html, &Default::default(), 1.0, Some(&loaded), &());
    assert_eq!(pre.trees.scroll_nodes().count(), 0);
    let clips: Vec<RectF> = pre
        .trees
        .clip
        .iter()
        .filter_map(|c| c.rrect())
        .map(|c| c.rect)
        .collect();
    // The box's padding box; a replaced element's and an `<svg>`'s
    // content box.
    for clip in [
        RectF::new(5.0, 5.0, 100.0, 50.0),
        RectF::new(5.0, 65.0, 100.0, 100.0),
        RectF::new(5.0, 175.0, 100.0, 50.0),
    ] {
        assert!(clips.contains(&clip), "{clip:?} not in {clips:?}");
    }
}

/// `clip-path` is a clip node with its path, in the path's bounds, drawn
/// as the clip and a `ClipPath`; `clip` a rect, on an absolutely
/// positioned box alone, its `auto` edges the border box's.
#[test]
fn clip_paths_and_clips_are_clip_nodes() {
    let html = "<style>html, body { margin: 0 } div { width: 100px; height: 100px; background-color: green }</style>
         <div style='clip-path: inset(10px 20px)'></div>
         <div style='position: absolute; top: 100px; clip: rect(5px, auto, 50px, 10px)'></div>
         <div style='margin-top: 100px; clip: rect(5px, auto, 50px, 10px)'></div>";
    let (pre, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let clips: Vec<(RectF, bool)> = pre
        .trees
        .clip
        .iter()
        .filter_map(|c| Some((c.rect?, c.paths.is_some())))
        .collect();
    assert_eq!(
        clips,
        [
            (RectF::new(20.0, 10.0, 60.0, 80.0), true),
            (RectF::new(10.0, 105.0, 90.0, 45.0), false),
        ]
    );
    let list = paint::to_display_list(&artifact, &pre.trees);
    let paths: Vec<Option<RectF>> = list
        .ops()
        .iter()
        .filter_map(|op| match op {
            PaintOp::ClipPath { path, .. } => Some(path.bounds()),
            _ => None,
        })
        .collect();
    assert_eq!(paths, [Some(RectF::new(20.0, 10.0, 60.0, 80.0))]);
}

/// A `<clipPath>` of several shapes is one clip node of all their paths,
/// drawn as their union; the one its own `clip-path` names, another.
#[test]
fn a_clip_path_element_of_shapes_is_their_union() {
    let html = "<style>html, body { margin: 0 }</style>
         <svg width=0 height=0 style='position: absolute'>
           <clipPath id=c clip-path='url(#d)'><rect width=10 height=10 /><rect x=20 width=10 height=10 /></clipPath>
           <clipPath id=d><rect width=25 height=5 /></clipPath>
         </svg>
         <div style='width: 100px; height: 100px; background-color: green; clip-path: url(#c)'></div>";
    let (pre, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let unions: Vec<(usize, Option<RectF>)> = pre
        .trees
        .clip
        .iter()
        .filter_map(|c| c.paths.as_ref().map(|paths| (paths.len(), c.rect)))
        .collect();
    // Each in the bounds of all its paths.
    assert_eq!(
        unions,
        [
            (2, Some(RectF::new(0.0, 0.0, 30.0, 10.0))),
            (1, Some(RectF::new(0.0, 0.0, 25.0, 5.0)))
        ]
    );
    let list = paint::to_display_list(&artifact, &pre.trees);
    let ops: Vec<usize> = list
        .ops()
        .iter()
        .filter_map(|op| match op {
            PaintOp::ClipPaths { paths, .. } => Some(paths.len()),
            PaintOp::ClipPath { .. } => Some(1),
            _ => None,
        })
        .collect();
    assert_eq!(ops, [2, 1]);
}

/// Containment on the root element or `<body>` keeps `<body>`'s
/// background off the canvas.
#[test]
fn containment_keeps_the_bodys_background_to_itself() {
    let blue = Color::rgb(0, 0, 255);
    let on_canvas = |html: &str| {
        fills(&paint_html(html))
            .iter()
            .any(|&(kind, _, color)| kind == DisplayItemKind::DocumentBackground && color == blue)
    };
    assert!(on_canvas("<body style='background-color: blue'>"));
    assert!(!on_canvas(
        "<body style='background-color: blue; contain: paint'>"
    ));
    assert!(!on_canvas(
        "<html style='contain: size'><body style='background-color: blue'>"
    ));
}

#[test]
fn a_rounded_images_content_is_cut_to_its_content_box_curve() {
    use gfx::RRectF;
    // Every `<img>` loaded, 20x20 (Blink's `InnerBorderRadiusClip`).
    let loaded = image(20, 20);
    let html = "<style>html, body { margin: 0 }</style>
         <img src='a.png' style='display: block; width: 40px; height: 40px; border: 2px solid;
           padding: 3px; border-radius: 50%'>";
    let (pre, _, _) = paint_html_with_images(html, &Default::default(), 1.0, Some(&loaded), &());
    let rounded: Vec<RRectF> = pre
        .trees
        .clip
        .iter()
        .filter(|c| c.is_rounded())
        .filter_map(|c| c.rrect())
        .collect();
    // The border box is 50 wide, its radii 25; the content box is 5 in,
    // its radii 20.
    assert_eq!(
        rounded,
        [RRectF::new(
            RectF::new(5.0, 5.0, 40.0, 40.0),
            [SizeF::new(20.0, 20.0); 4]
        )]
    );
}

#[test]
fn an_inline_element_rounds_only_where_it_starts_and_ends() {
    let html = "<style>html, body { margin: 0 } body { font: 10px/10px Ahem; width: 50px }</style>
                <span style='background: blue; border-radius: 4px'>XXX XXX</span>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let radii: Vec<[SizeF; 4]> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawRRect { rrect, .. } => Some(rrect.radii),
            _ => None,
        })
        .collect();
    let (zero, four) = (SizeF::new(0.0, 0.0), SizeF::new(4.0, 4.0));
    // First line: its start's corners (top left, bottom left); second:
    // its end's (top right, bottom right).
    assert_eq!(radii, [[four, zero, zero, four], [zero, four, four, zero]]);
}

#[test]
fn an_inline_elements_content_box_is_inside_its_padding_where_it_starts_and_ends() {
    // `padding: 10%` is 10% of the containing block's width (100px), and
    // only the line where the element starts has its start's padding, the
    // line where it ends its end's.
    let html = "<style>html, body { margin: 0 } body { font: 10px/10px Ahem; width: 100px }</style>
                <span style='padding: 0 10%; background: blue content-box'>XXXXXX XXXXXX</span>";
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &());
    let rects: Vec<RectF> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawRect { rect, flags } if flags.color == Color::rgb(0, 0, 255) => {
                Some(*rect)
            }
            _ => None,
        })
        .collect();
    // The first line: 10px of padding, then six Xs; the second: six Xs,
    // then the padding.
    assert_eq!(
        rects,
        [
            RectF::new(10.0, 0.0, 60.0, 10.0),
            RectF::new(0.0, 10.0, 60.0, 10.0)
        ]
    );
}

#[test]
fn box_shadows_paint_around_and_inside_the_box() {
    use gfx::RRectF;
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; background: green;
           box-shadow: 10px 5px 4px 2px red, 1px 1px blue, inset 3px 3px 0 2px rgb(0, 0, 1)'></div>";
    let artifact = paint_html(html);
    let ops: Vec<(&PaintOp, RectF)> = artifact
        .items
        .iter()
        .map(|item| (&item.op, item.visual_rect))
        .collect();
    let border_box = RRectF::from_rect(RectF::new(0.0, 0.0, 100.0, 50.0));
    // The outer shadows, the last first so the first is on top, then the
    // background, then the inset shadow.
    assert_eq!(ops.len(), 4, "{ops:?}");
    assert!(
        matches!(ops[0].0, PaintOp::DrawShadow { shape, clip, inside: false, flags, .. }
        if *shape == border_box.offset(1.0, 1.0) && *clip == border_box
            && flags.color == Color::rgb(0, 0, 255) && flags.blur == 0.0)
    );
    // Offset, then spread by 2; a blur radius of 4 is a deviation of 2,
    // which reaches 6 past the shape.
    let shape = RRectF::from_rect(RectF::new(8.0, 3.0, 104.0, 54.0));
    assert!(
        matches!(ops[1].0, PaintOp::DrawShadow { shape: s, inside: false, flags, .. }
        if *s == shape && flags.color == Color::rgb(255, 0, 0) && flags.blur == 2.0)
    );
    assert_eq!(ops[1].1, shape.rect.inset(-6.0));
    assert!(
        matches!(ops[2].0, PaintOp::DrawRect { flags, .. } if flags.color == Color::rgb(0, 128, 0))
    );
    // Inset: the padding box less the hole, the padding box moved by the
    // offset and shrunk by the spread.
    assert!(
        matches!(ops[3].0, PaintOp::DrawShadow { hole: Some(hole), clip, inside: true, flags, .. }
        if *hole == border_box.offset(3.0, 3.0).spread(-2.0) && *clip == border_box
            && flags.color == Color::rgb(0, 0, 1))
    );
    assert_eq!(ops[3].1, border_box.rect);
    // An inline element's, on its line, around its background.
    let html = "<style>html, body { margin: 0 } body { font: 10px/10px Ahem }</style>
         <span style='background: green; box-shadow: 2px 2px red, inset 1px 1px blue'>X</span>";
    let artifact = paint_html(html);
    let kinds: Vec<&str> = artifact
        .items
        .iter()
        .filter_map(|item| match &item.op {
            PaintOp::DrawShadow { inside: false, .. } => Some("outer"),
            PaintOp::DrawShadow { inside: true, .. } => Some("inset"),
            PaintOp::DrawRect { flags, .. } if flags.color == Color::rgb(0, 128, 0) => {
                Some("background")
            }
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["outer", "background", "inset"]);
}

#[test]
fn an_outline_line_is_drawn_as_a_border_of_its_style() {
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; outline: 3px dashed red'></div>";
    let artifact = paint_html(html);
    let dashes: Vec<compositor_paint::Dash> = artifact
        .items
        .iter()
        .filter(|item| item.kind == DisplayItemKind::Outline)
        .filter_map(|item| match &item.op {
            PaintOp::DrawClipped { op, .. } => match &**op {
                PaintOp::DrawLine { flags, .. } => flags.dash.clone(),
                _ => None,
            },
            _ => None,
        })
        .collect();
    assert_eq!(dashes.len(), 4);
}

#[test]
fn outlines_paint_after_the_lines_around_their_boxes() {
    use gfx::RRectF;
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; outline: 2px solid red; outline-offset: 3px'>
           <div style='height: 10px; background: green'></div></div>
         <div style='width: 40px; height: 20px; border-radius: 5px; outline: 1px solid blue'></div>";
    let artifact = paint_html(html);
    let kinds: Vec<DisplayItemKind> = artifact.items.iter().map(|item| item.kind).collect();
    // The child's background before the outline over it.
    assert_eq!(
        kinds,
        [
            DisplayItemKind::BoxDecorationBackground,
            DisplayItemKind::Outline,
            DisplayItemKind::Outline,
        ]
    );
    // Square: one ring path, as a border's, from 5px out to 3px out.
    let edge = RRectF::from_rect(RectF::new(0.0, 0.0, 100.0, 50.0)).spread(3.0);
    let corners = |r: RectF| {
        let p = gfx::PointF::new;
        [
            p(r.x, r.y),
            p(r.right(), r.y),
            p(r.right(), r.bottom()),
            p(r.x, r.bottom()),
        ]
    };
    assert!(
        matches!(&artifact.items[1].op, PaintOp::DrawPath { path, flags }
        if polygons(path)[0] == corners(edge.spread(2.0).rect)
            && polygons(path)[1].iter().all(|p| corners(edge.rect).contains(p))
            && flags.color == Color::rgb(255, 0, 0))
    );
    assert_eq!(artifact.items[1].visual_rect, edge.spread(2.0).rect);
    // Rounded with the box's corners.
    assert!(
        matches!(&artifact.items[2].op, PaintOp::DrawDRRect { outer, inner, .. }
        if inner.radii[0] == SizeF::new(5.0, 5.0) && outer.radii[0] == SizeF::new(6.0, 6.0))
    );
    // What is painted whole (an inline-block, a flex item) paints its own
    // outline where it is painted, once: before what comes after it.
    let html = "<style>html, body { margin: 0 } body { font: 10px/10px Ahem }
                .o { outline: 1px solid red } .g { background: green; height: 5px }</style>
         <div><span class=o style='display: inline-block; width: 10px'>X</span>X</div>
         <div style='display: flex'><div class=o style='width: 10px'>X</div>
           <div class=g style='width: 10px'></div></div>";
    let artifact = paint_html(html);
    let kinds: Vec<DisplayItemKind> = artifact
        .items
        .iter()
        .map(|item| item.kind)
        .filter(|kind| *kind != DisplayItemKind::Text)
        .collect();
    assert_eq!(
        kinds,
        [
            DisplayItemKind::Outline,
            DisplayItemKind::Outline,
            DisplayItemKind::BoxDecorationBackground,
        ]
    );
}

#[test]
fn a_focus_ring_is_a_dark_ring_and_a_white_one() {
    use gfx::RRectF;
    let html = "<style>html, body { margin: 0 } body { font: 10px/10px Ahem }</style>
         <div style='width: 40px; height: 20px; outline: auto 1px -webkit-focus-ring-color'></div>
         <span style='outline: 2px solid red'>X</span>";
    let artifact = paint_html(html);
    let outlines: Vec<&paint::DisplayItem> = artifact
        .items
        .iter()
        .filter(|item| item.kind == DisplayItemKind::Outline)
        .collect();
    let rings: Vec<(RRectF, RRectF, Color)> = outlines[..2]
        .iter()
        .map(|item| match &item.op {
            PaintOp::DrawDRRect {
                outer,
                inner,
                flags,
                ..
            } => (*outer, *inner, flags.color),
            op => panic!("{op:?}"),
        })
        .collect();
    // Its corners rounded by its width, the box's being square: white
    // outside, the focus ring color against the box.
    let edge = RRectF::uniform(RectF::new(0.0, 0.0, 40.0, 20.0), 3.0);
    assert_eq!(rings[0], (edge.spread(3.0), edge.spread(2.0), Color::WHITE));
    assert_eq!(rings[1], (edge.spread(2.0), edge, Color::rgb(16, 16, 16)));
    // An inline element's outline, around its part of the line.
    assert_eq!(outlines[2].visual_rect, RectF::new(-2.0, 18.0, 14.0, 14.0));
    assert_eq!(outlines.len(), 3);
}

#[test]
fn border_styles_draw_their_bands_and_shades() {
    let html = "<style>html, body { margin: 0 } div { width: 88px; height: 38px }</style>
         <div style='border: 6px double red'></div>
         <div style='border: 6px inset rgb(0, 128, 0)'></div>
         <div style='border: 6px groove black'></div>";
    let artifact = paint_html(html);
    let paths: Vec<(Color, usize)> = artifact
        .items
        .iter()
        .map(|item| match &item.op {
            PaintOp::DrawPath { path, flags } => (flags.color, polygons(path).len()),
            op => panic!("{op:?}"),
        })
        .collect();
    let (red, black) = (Color::rgb(255, 0, 0), Color::rgb(0, 0, 0));
    // Double: two rings, a third of the width each (2px, a 2px gap, 2px).
    assert_eq!(paths[..2], [(red, 2), (red, 2)]);
    assert!(
        matches!(&artifact.items[0].op, PaintOp::DrawPath { path, .. }
        if polygons(path)[1][0] == gfx::PointF::new(2.0, 2.0))
    );
    assert!(
        matches!(&artifact.items[1].op, PaintOp::DrawPath { path, .. }
        if polygons(path)[0][0] == gfx::PointF::new(4.0, 4.0))
    );
    // Inset: the top and left darker, the bottom and right lighter (Blink's
    // `Color::Dark` and `Color::Light`), each pair one path.
    assert_eq!(
        paths[2..4],
        [(Color::rgb(0, 44, 0), 2), (Color::rgb(0, 212, 0), 2)]
    );
    // Groove, black: the outer half of the top and left stays black (as
    // dark as can be) and the inner half of the bottom and right; the
    // others are lightened.
    let gray = Color::rgb(0x54, 0x54, 0x54);
    let groove: Vec<(Color, usize)> = paths[4..].to_vec();
    assert_eq!(groove, [(black, 4), (gray, 4)]);
}

#[test]
fn dashes_and_dots_fit_their_sides() {
    let html = "<style>html, body { margin: 0 }</style>
         <div style='width: 94px; height: 44px; border: 3px dashed red'></div>
         <div style='width: 92px; height: 42px; border: 4px dotted red'></div>
         <div style='width: 94px; height: 44px; border: 3px dashed red; border-radius: 10px'></div>";
    let artifact = paint_html(html);
    let strokes: Vec<(&PaintOp, &compositor_paint::PaintFlags, usize)> = artifact
        .items
        .iter()
        .map(|item| match &item.op {
            PaintOp::DrawClipped { polygon, op } => match &**op {
                PaintOp::DrawLine { flags, .. } | PaintOp::DrawRRect { flags, .. } => {
                    (&**op, flags, polygon.len())
                }
                op => panic!("{op:?}"),
            },
            op => panic!("{op:?}"),
        })
        .collect();
    assert_eq!(
        strokes.len(),
        12,
        "each side on its own, clipped to its trapezoid"
    );
    // A 100px top side 3px wide: dashes of 6, gaps of 3 fitted to 3.4, so
    // eleven dashes end on the corners (Blink's `SelectBestDashGap`).
    assert_eq!(strokes[0].1.cap, compositor_paint::LineCap::Butt);
    let dash = strokes[0].1.dash.as_ref().unwrap().intervals();
    assert_eq!(dash[0], 6.0);
    assert!((dash[1] - 3.4).abs() < 1e-4, "{dash:?}");
    assert!(matches!(strokes[0].0, PaintOp::DrawLine { p0, p1, .. }
        if *p0 == gfx::PointF::new(0.0, 51.5 - 50.0) && *p1 == gfx::PointF::new(100.0, 1.5)));
    // Dots 4 wide, from 2px in at each end of the 100px side: 96px in 12
    // gaps of 8.
    assert_eq!(strokes[4].1.cap, compositor_paint::LineCap::Round);
    let dot = strokes[4].1.dash.as_ref().unwrap().intervals();
    assert_eq!(dot[0], 0.0);
    assert!((dot[1] - 8.0).abs() < 1e-2, "{dot:?}");
    assert!(matches!(strokes[4].0, PaintOp::DrawLine { p0, p1, .. }
        if *p0 == gfx::PointF::new(2.0, 52.0) && *p1 == gfx::PointF::new(98.0, 52.0)));
    // Rounded: the ring's middle curve, all the way round.
    assert!(matches!(strokes[8].0, PaintOp::DrawRRect { rrect, .. }
        if rrect.rect == RectF::new(1.5, 101.5, 97.0, 47.0)));
}

#[test]
fn shades_and_double_lines_follow_blink() {
    // The colors each style gives its four sides, the top's first.
    let colors = |border: &str| -> Vec<Color> {
        let html = format!(
            "<style>html, body {{ margin: 0 }}</style>
             <div style='width: 50px; height: 20px; border: {border}'></div>"
        );
        let artifact = paint_html(&html);
        artifact
            .items
            .iter()
            .map(|item| match &item.op {
                PaintOp::DrawPath { flags, .. } => flags.color,
                op => panic!("{op:?}"),
            })
            .collect()
    };
    let rgb = Color::rgb;
    // Outset: the top and left lighter, the bottom and right darker.
    assert_eq!(
        colors("6px outset rgb(0, 128, 0)"),
        [rgb(0, 212, 0), rgb(0, 44, 0)]
    );
    // About as dark (within rgb(32, 32, 32)) or light (within rgb(235,
    // 235, 235)) as can be: left as it is.
    // (Lightened, rgb(16, 16, 16) is rgb(100, 100, 100): 0.33 more of its
    // brightest channel, scaled.)
    assert_eq!(
        colors("6px inset rgb(16, 16, 16)"),
        [rgb(16, 16, 16), rgb(100, 100, 100)]
    );
    assert_eq!(
        colors("6px inset rgb(250, 250, 250)"),
        [rgb(166, 166, 166), rgb(250, 250, 250)]
    );
    // White darkened is rgb(171, 171, 171) (Blink's `kDarkenedWhite`).
    assert_eq!(
        colors("6px inset white"),
        [rgb(171, 171, 171), rgb(255, 255, 255)]
    );
    // Double: 5px is 2, 1 and 2 (whole device pixels); 2px, one line.
    let rings = |border: &str| -> Vec<(f32, f32)> {
        let html = format!(
            "<style>html, body {{ margin: 0 }}</style>
             <div style='width: 50px; height: 20px; border: {border}'></div>"
        );
        paint_html(&html)
            .items
            .iter()
            .map(|item| match &item.op {
                PaintOp::DrawPath { path, .. } => {
                    let polygons = polygons(path);
                    (polygons[0][0].x, polygons[1][0].x)
                }
                op => panic!("{op:?}"),
            })
            .collect()
    };
    assert_eq!(rings("5px double red"), [(0.0, 2.0), (3.0, 5.0)]);
    assert_eq!(rings("6px double red"), [(0.0, 2.0), (4.0, 6.0)]);
    assert_eq!(rings("7px double red"), [(0.0, 2.0), (5.0, 7.0)]);
    assert_eq!(rings("8px double red"), [(0.0, 3.0), (5.0, 8.0)]);
    assert_eq!(rings("2px double red"), [(0.0, 2.0)]);
    // Wider than a CSS length goes: clamped to 33554428px first, in thirds
    // still (the inner line's start, 22369619, is an f32 apart).
    assert_eq!(
        rings("1.5e9px double red"),
        [(0.0, 11_184_809.0), (22_369_620.0, 33_554_428.0)]
    );
}

#[test]
fn dash_lengths_follow_the_width_and_the_path() {
    let dashes = |style: &str| -> Vec<Option<compositor_paint::Dash>> {
        let html = format!("<style>html, body {{ margin: 0 }}</style><div style='{style}'></div>");
        paint_html(&html)
            .items
            .iter()
            .map(|item| match &item.op {
                PaintOp::DrawClipped { op, .. } => match &**op {
                    PaintOp::DrawLine { flags, .. } | PaintOp::DrawRRect { flags, .. } => {
                        flags.dash.clone()
                    }
                    op => panic!("{op:?}"),
                },
                op => panic!("{op:?}"),
            })
            .collect()
    };
    // Below 3px: dashes three widths long, gaps of two (fitted: ten dashes
    // of 6 in 100px leave nine gaps of 4.44).
    let thin = dashes("width: 96px; height: 20px; border: 2px dashed red")[0]
        .clone()
        .unwrap();
    let thin = thin.intervals();
    assert_eq!(thin[0], 6.0);
    assert!((thin[1] - 40.0 / 9.0).abs() < 1e-3, "{thin:?}");
    // No room for two dashes and a gap: solid.
    assert_eq!(
        dashes("width: 4px; height: 20px; border: 3px dashed red")[0],
        None
    );
    // Round a rounded ring, a closed path: as many gaps as dashes. The
    // middle curve is about 273.4 long, 30 dashes of 6 and gaps of 3.11.
    let round = dashes("width: 94px; height: 44px; border: 3px dashed red; border-radius: 10px")[0]
        .clone()
        .unwrap();
    assert!((round.intervals()[1] - 3.1136).abs() < 1e-2, "{round:?}");
}

/// Every canvas's snapshot is `0`, or none.
struct Canvases(Option<gfx::ImageData>);

impl paint::BackgroundImages for Canvases {
    fn image(&self, _: &str) -> Option<(gfx::ImageData, gfx::Size)> {
        None
    }

    fn canvas(&self, _: NodeId) -> Option<gfx::ImageData> {
        self.0.clone()
    }
}

/// A canvas's snapshot is drawn into its content box (Blink's
/// `HTMLCanvasPainter`), sampled bilinearly without mipmaps, as an item of
/// its own kind in a chunk of its own: what is painted before and after
/// it, under the same state, goes in other chunks. No snapshot, no item.
#[test]
fn a_canvas_paints_its_snapshot_in_a_chunk_of_its_own() {
    let html = "<body style='margin: 0'>
        <div style='height: 10px; background: red'></div>
        <canvas width=2 height=1 style='display: block; border: 1px solid blue; padding: 2px'></canvas>
        <div>after</div>";
    let snapshot = gfx::ImageData::decoded(gfx::Size::new(2, 1), vec![0; 8]);
    let (_, artifact, _) = paint_html_with(
        html,
        &Default::default(),
        1.0,
        &Canvases(Some(snapshot.clone())),
    );
    let index = artifact
        .items
        .iter()
        .position(|item| item.kind == DisplayItemKind::Canvas)
        .expect("a canvas item");
    let item = &artifact.items[index];
    assert!(
        matches!(&item.op, PaintOp::DrawImage { image, dst, filter, .. }
            if *image == snapshot
                && *dst == RectF::new(3.0, 13.0, 2.0, 1.0)
                && *filter == compositor_paint::FilterQuality::Low),
        "{:?}",
        item.op
    );
    assert_eq!(item.visual_rect, RectF::new(3.0, 13.0, 2.0, 1.0));
    let chunk = |i: usize| {
        artifact
            .chunks
            .iter()
            .position(|chunk| chunk.items.contains(&i))
            .expect("in a chunk")
    };
    // Alone in its chunk, the items round it (the backgrounds before, the
    // text after) in others, though all are under the same state.
    assert_eq!(artifact.items[index + 1].kind, DisplayItemKind::Text);
    let own = &artifact.chunks[chunk(index)];
    assert_eq!(own.items, index..index + 1);
    assert_ne!(chunk(index - 1), chunk(index));
    assert_ne!(chunk(index + 1), chunk(index));
    assert_eq!(artifact.chunks[chunk(index - 1)].state, own.state);
    assert_eq!(artifact.chunks[chunk(index + 1)].state, own.state);
    // Nothing to show: no item, and the items round it in one chunk.
    let (_, artifact, _) = paint_html_with(html, &Default::default(), 1.0, &Canvases(None));
    assert!(
        artifact
            .items
            .iter()
            .all(|item| item.kind != DisplayItemKind::Canvas)
    );
    assert_eq!(artifact.chunks.len(), 1, "{:?}", artifact.chunks);
}

/// A canvas's bitmap under `border-radius` is cut to its content box's
/// curve, as an image is.
#[test]
fn a_canvas_is_clipped_to_its_rounded_content_box() {
    let html = "<body style='margin: 0'>
        <canvas width=20 height=20 style='display: block; border-radius: 10px'></canvas>";
    let snapshot = gfx::ImageData::decoded(gfx::Size::new(20, 20), vec![0; 1600]);
    let (pre, artifact, _) =
        paint_html_with(html, &Default::default(), 1.0, &Canvases(Some(snapshot)));
    let item = artifact
        .items
        .iter()
        .position(|item| item.kind == DisplayItemKind::Canvas)
        .expect("a canvas item");
    let chunk = artifact
        .chunks
        .iter()
        .find(|chunk| chunk.items.contains(&item))
        .unwrap();
    assert!(
        pre.trees.clip[chunk.state.clip.0].is_rounded(),
        "under a rounded clip"
    );
}
