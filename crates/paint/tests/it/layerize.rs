//! Which paint chunks go into which compositor layer
//! (`PaintArtifactCompositor`): only scroll containers that are merely
//! translated, under no opacity, with something to scroll, become layers,
//! and the chunks go into layers in paint order.

use std::collections::HashMap;

use compositor_paint::PaintOp;
use gfx::{NodeId, RectF, SizeF, Transform};
use layout::BoxFragment;
use paint::{ClipNodeId, PaintArtifact, PendingLayer, PrePaint, ScrollNodeId, TransformNodeId};

use crate::page;

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

struct Painted {
    view: std::sync::Arc<BoxFragment>,
    pre: PrePaint,
    artifact: PaintArtifact,
    /// Each element with an `id`.
    ids: HashMap<String, NodeId>,
}

fn paint_html(html: &str) -> Painted {
    paint_html_at(html, 1.0)
}

/// `html` painted on `dsf` device pixels per CSS px.
fn paint_html_at(html: &str, dsf: f32) -> Painted {
    paint_html_with(html, dsf, &())
}

/// `html` painted on `dsf` device pixels per CSS px, with `images`.
fn paint_html_with(html: &str, dsf: f32, images: &dyn paint::BackgroundImages) -> Painted {
    let (view, ids) = page::lay_out(html, VIEWPORT);
    let pre = paint::pre_paint(&view, &Default::default(), dsf);
    let artifact = paint::paint(&view, &pre, layout::document_size(&view), images);
    Painted {
        view,
        pre,
        artifact,
        ids,
    }
}

const SCROLLER: &str = "<style>html, body { margin: 0 } div { width: 100px }</style>
     <div style='height: 50px; background-color: red'></div>
     <div style='height: 100px; overflow: auto; STYLE'>
       <div style='height: 300px; background-color: green'></div>
     </div>
     <div style='height: 50px; background-color: blue'></div>";

fn scroller(style: &str) -> Painted {
    paint_html(&SCROLLER.replace("STYLE", style))
}

#[test]
fn a_translated_scroller_is_a_layer_between_what_is_before_and_after_it() {
    let p = scroller("");
    assert_eq!(paint::composited_scrollers(&p.pre.trees), [true]);
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    let scrollers: Vec<_> = layers.iter().map(|l| l.scroller).collect();
    assert_eq!(scrollers, [None, Some(ScrollNodeId(0)), None]);
    // Every chunk once, in order.
    let mut next = 0;
    for layer in &layers {
        assert_eq!(layer.chunks.start, next);
        next = layer.chunks.end;
    }
    assert_eq!(next, p.artifact.chunks.len());
    // The scroller's layer is its padding box, as big as what scrolls.
    let document = layout::document_size(&p.view);
    assert_eq!(layers[1].bounds, RectF::new(0.0, 50.0, 100.0, 300.0));
    assert_eq!(
        layers[0].bounds,
        RectF::new(0.0, 0.0, document.width, document.height)
    );
}

#[test]
fn scrollers_that_arent_only_translated_stay_painted() {
    for style in [
        "transform: scale(2)",
        "opacity: 0.5",
        // The compositor's clips are rounded rects.
        "clip-path: inset(0px)",
        // Nothing to scroll.
        "height: 400px",
    ] {
        let p = scroller(style);
        assert_eq!(
            paint::composited_scrollers(&p.pre.trees),
            [false],
            "{style}"
        );
        let layers = paint::layerize(
            &p.artifact,
            &p.pre.trees,
            layout::document_size(&p.view),
            crate::MAX_TEXTURE_SIZE,
        );
        let document = layout::document_size(&p.view);
        assert_eq!(
            layers,
            [PendingLayer {
                scroller: None,
                sticky: None,
                fixed: false,
                chunks: 0..p.artifact.chunks.len(),
                bounds: RectF::new(0.0, 0.0, document.width, document.height),
                canvas: None,
            }],
            "{style}"
        );
    }
    // Moved is fine.
    let p = scroller("transform: translate(10px, 20px)");
    assert_eq!(paint::composited_scrollers(&p.pre.trees), [true]);
}

#[test]
fn the_documents_layer_comes_first_even_with_nothing_in_it() {
    let p = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 100px; overflow: auto'>
           <div style='height: 300px; background-color: green'></div>
         </div>",
    );
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    assert_eq!(layers[0].scroller, None);
    assert!(layers[0].chunks.is_empty());
    assert_eq!(layers[1].scroller, Some(ScrollNodeId(0)));
    let p = paint_html("<div></div>");
    assert_eq!(
        paint::layerize(&p.artifact, &p.pre.trees, VIEWPORT, crate::MAX_TEXTURE_SIZE),
        [PendingLayer {
            scroller: None,
            sticky: None,
            fixed: false,
            chunks: 0..0,
            bounds: RectF::new(0.0, 0.0, VIEWPORT.width, VIEWPORT.height),
            canvas: None,
        }]
    );
}

/// The scroller's content is in its own space with the origin at the
/// padding box's top left, and unscrolled: its display list doesn't depend
/// on the offset, which the compositor applies.
#[test]
fn a_scroller_layers_content_is_in_its_own_space() {
    let p = scroller("border-top: 10px solid");
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    let layer = &layers[1];
    assert_eq!(layer.bounds.y, 60.0, "under the border");
    let list = layer.display_list(&p.artifact, &p.pre.trees);
    // The green content, at 60 in the scroller's space, drawn moved up by
    // the layer's origin: at the layer's top, whatever the offset.
    let ops = list.ops();
    let at = ops
        .iter()
        .position(|op| matches!(op, PaintOp::DrawRect { rect, .. } if *rect == RectF::new(0.0, 60.0, 100.0, 300.0)))
        .expect("the content");
    let matrix = ops[..at].iter().rev().find_map(|op| match op {
        PaintOp::SetMatrix { transform } => Some(*transform),
        _ => None,
    });
    assert_eq!(matrix, Some(Transform::translate(0.0, -60.0)), "{ops:?}");
}

/// A scroller at a fractional position: its layer's origin is on the
/// device pixel grid too, so its content, snapped in the document's space,
/// lands on whole device pixels in the layer as well.
#[test]
fn a_scroller_layer_is_on_the_device_pixel_grid() {
    let html = SCROLLER.replace("STYLE", "margin-top: 0.25px");
    for (dsf, y) in [(1.0, 50.0), (2.0, 50.5)] {
        let p = paint_html_at(&html, dsf);
        let layers = paint::layerize(
            &p.artifact,
            &p.pre.trees,
            layout::document_size(&p.view),
            crate::MAX_TEXTURE_SIZE,
        );
        let bounds = layers[1].bounds;
        assert_eq!(bounds.y, y, "at {dsf}x");
        assert_eq!(bounds.bottom(), y + 300.0, "at {dsf}x");
    }
}

/// The bounds of the layer of what is painted after a container, in CSS
/// px: `html` painted at `dsf`.
fn after_bounds(html: &str, dsf: f32) -> RectF {
    let p = paint_html_at(html, dsf);
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    assert_eq!(layers.len(), 3, "before, the scroller, after");
    layers[2].bounds
}

/// What is painted after a container is a layer as big as it, out to whole
/// tiles of device pixels: not the whole document.
#[test]
fn the_layer_after_a_scroller_is_as_big_as_what_it_paints() {
    let html = SCROLLER.replace("STYLE", "");
    // The blue box, 0,150 100x50, out to 256 device px.
    assert_eq!(after_bounds(&html, 1.0), RectF::new(0.0, 0.0, 256.0, 256.0));
    assert_eq!(
        after_bounds(&html, 2.0),
        RectF::new(0.0, 128.0, 128.0, 128.0)
    );
    // Off the grid on the left too: 300 to 400 is the second tile.
    let right = html.replace(
        "height: 50px; background-color: blue",
        "height: 50px; margin-left: 300px; background-color: blue",
    );
    assert_eq!(
        after_bounds(&right, 1.0),
        RectF::new(256.0, 0.0, 256.0, 256.0)
    );
    // Growing inside its tiles, it keeps its origin; past them, it grows.
    let taller = html.replace(
        "height: 50px; background-color: blue",
        "height: 100px; background-color: blue",
    );
    assert_eq!(
        after_bounds(&taller, 1.0),
        RectF::new(0.0, 0.0, 256.0, 256.0)
    );
    let tallest = html.replace(
        "height: 50px; background-color: blue",
        "height: 200px; background-color: blue",
    );
    assert_eq!(
        after_bounds(&tallest, 1.0),
        RectF::new(0.0, 0.0, 256.0, 512.0)
    );
}

/// What is painted is cut by its clips, and taken through its transforms.
#[test]
fn the_layer_after_a_scroller_is_what_its_clips_and_transforms_leave() {
    let after = |style: &str| {
        after_bounds(
            &SCROLLER.replace(
                "<div style='height: 50px; background-color: blue'></div>",
                // The second box is all clipped away: nothing of it counts.
                &format!(
                    "<div style='{style}'><div style='height: 2000px; background-color: blue'></div>\
                     <div style='height: 50px; background-color: red'></div></div>"
                ),
            ),
            1.0,
        )
    };
    // Clipped to 100px: the 2000px inside doesn't count. (With opacity, so
    // it isn't a layer of its own.)
    let clipped = "height: 100px; overflow: hidden; opacity: 0.9";
    assert_eq!(after(clipped), RectF::new(0.0, 0.0, 256.0, 256.0));
    // Moved down a tile and more.
    let moved = after(&format!("{clipped}; transform: translate(0, 300px)"));
    assert_eq!(moved, RectF::new(0.0, 256.0, 256.0, 512.0));
}

/// What is painted outside the document isn't seen: the layer stops at it.
#[test]
fn the_layer_after_a_scroller_stays_in_the_document() {
    let html = SCROLLER.replace(
        "<div style='height: 50px; background-color: blue'></div>",
        "<div style='height: 50px; margin-left: -300px; width: 400px; background-color: blue'></div>",
    );
    assert_eq!(after_bounds(&html, 1.0).x, 0.0);
}

/// Anti-aliasing reaches a pixel past what isn't drawn on whole pixels: a
/// scaled box ending on a tile's edge takes the next tile too.
#[test]
fn the_layer_after_a_scroller_takes_the_pixel_anti_aliasing_reaches() {
    // At 181 (after the 150 above), 50px high, scaled 2x about its center:
    // 156 to 256.
    let html = SCROLLER.replace(
        "<div style='height: 50px; background-color: blue'></div>",
        "<div style='margin-top: 31px; height: 50px; background-color: blue; transform: scale(2)'></div>",
    );
    assert_eq!(after_bounds(&html, 1.0), RectF::new(0.0, 0.0, 256.0, 512.0));
}

/// The layer holds everything after the container, each side taken from
/// another box: the first painted is in the middle, one further right and
/// down, and the last (a stacking context, so painted after) moved up to
/// the left.
#[test]
fn the_layer_after_a_scroller_holds_all_it_paints() {
    let html = SCROLLER.replace(
        "<div style='height: 50px; background-color: blue'></div>",
        "<div style='margin: 120px 0 0 300px; height: 50px; background-color: blue'></div>\
         <div style='height: 50px; background-color: blue; transform: translate(0, -300px)'></div>\
         <div style='margin: 100px 0 0 450px; height: 50px; background-color: blue'></div>",
    );
    // 300,270 to 400,320; 450,470 to 550,520; 0,20 to 100,70.
    assert_eq!(after_bounds(&html, 1.0), RectF::new(0.0, 0.0, 768.0, 768.0));
}

/// Content fixed to the main frame's viewport is a layer of its own, as
/// big as what it paints, so the compositor can keep it where it is while
/// it scrolls the rest.
#[test]
fn fixed_content_is_a_layer_of_its_own() {
    let p = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='height: 50px; background-color: red'></div>
         <div id=bar style='position: fixed; top: 0; left: 0; width: 200px; height: 30px;
           background-color: green'>
           <div style='transform: translate(5px); width: 10px; height: 10px;
             background-color: red'></div>
         </div>
         <div style='height: 2000px; background-color: blue'></div>",
    );
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    let fixed: Vec<&PendingLayer> = layers.iter().filter(|layer| layer.fixed).collect();
    // One layer for all of it: what is under the fixed box is fixed too,
    // transform of its own or not.
    assert_eq!(fixed.len(), 1, "one fixed layer: {layers:?}");
    assert_eq!(fixed[0].chunks.len(), 2, "the box and the transformed one");
    // What it paints, grown to the layer grid, as any layer that isn't a
    // scroller's is.
    assert_eq!(fixed[0].bounds, RectF::new(0.0, 0.0, 256.0, 256.0));
    // The document's own layer is still the whole document, and the flow
    // around the fixed box stays in it.
    assert!(!layers[0].fixed);
    assert_eq!(
        layers[0].bounds,
        RectF::new(0.0, 0.0, 800.0, layout::document_size(&p.view).height)
    );
}

#[test]
fn a_scroller_is_a_layer_under_one_rounded_clip_not_two() {
    // Its own rounded clip: Viz cuts its quads to it.
    let p = scroller("border-radius: 10px");
    assert_eq!(paint::composited_scrollers(&p.pre.trees), [true]);
    // In another rounded clip as well: two would need a render surface.
    let p = paint_html(
        "<div style='width: 200px; height: 200px; overflow: hidden; border-radius: 20px'>
           <div style='height: 100px; overflow: auto; border-radius: 10px'>
             <div style='height: 300px'></div>
           </div>
         </div>",
    );
    assert_eq!(paint::composited_scrollers(&p.pre.trees), [false, false]);
    // A rounded clip around a square scroller is the one.
    let p = paint_html(
        "<div style='width: 200px; height: 200px; overflow: hidden; border-radius: 20px'>
           <div style='height: 100px; overflow: auto'>
             <div style='height: 300px'></div>
           </div>
         </div>",
    );
    assert_eq!(paint::composited_scrollers(&p.pre.trees), [false, true]);
}

/// Every canvas shows a 1×1 snapshot.
struct Snapshots;

impl paint::BackgroundImages for Snapshots {
    fn image(&self, _: &str) -> Option<(gfx::ImageData, gfx::Size)> {
        None
    }

    fn canvas(&self, _: NodeId) -> Option<gfx::ImageData> {
        Some(gfx::ImageData::decoded(gfx::Size::new(1, 1), vec![0; 4]))
    }
}

/// `html` painted with every canvas showing a snapshot.
fn paint_canvases(html: &str) -> Painted {
    paint_html_with(html, 1.0, &Snapshots)
}

fn layers_of(p: &Painted) -> Vec<PendingLayer> {
    paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    )
}

/// A canvas in `style`, with something before and after it.
fn canvas(style: &str) -> Painted {
    paint_canvases(&format!(
        "<style>html, body {{ margin: 0 }}</style>
         <div style='height: 10px; background: red'></div>
         <div style='{style}'><canvas width=20 height=10 style='display: block; border: 1px solid'></canvas></div>
         <div>after</div>"
    ))
}

/// A canvas is a layer of its own (Blink's foreign layer of a composited
/// canvas), as big as its content box, in its own space; what is painted
/// before and after it is in other layers, the one after above it.
#[test]
fn a_canvas_is_a_layer_of_its_own_between_what_is_before_and_after_it() {
    let p = canvas("");
    let layers = layers_of(&p);
    let index = layers
        .iter()
        .position(|layer| layer.canvas.is_some())
        .expect("a canvas layer");
    let layer = &layers[index];
    assert_eq!(layer.bounds, RectF::new(1.0, 11.0, 20.0, 10.0));
    assert_eq!(layer.chunks.len(), 1);
    let chunk = &p.artifact.chunks[layer.chunks.start];
    assert_eq!(
        p.artifact.items[chunk.items.start].kind,
        paint::DisplayItemKind::Canvas
    );
    let canvas = layer.canvas.unwrap();
    assert_eq!(canvas.state, chunk.state);
    assert_eq!(layer.transform(&p.pre.trees), chunk.state.transform);
    assert_eq!(layer.clip(&p.pre.trees), chunk.state.clip);
    // The red and the border before it, the text after.
    assert_eq!(index, 1, "{layers:?}");
    assert_eq!(layers.len(), 3, "{layers:?}");
}

/// Under a transform other than a translation, or opacity, a canvas is
/// painted into the page: the compositor and Viz only translate layers.
#[test]
fn a_canvas_under_more_than_a_translation_or_opacity_is_painted_into_the_page() {
    for style in [
        "transform: matrix(1, 0.2, 0, 1, 0, 0)",
        "transform: scale(2)",
        "opacity: 0.5",
    ] {
        let p = canvas(style);
        let layers = layers_of(&p);
        assert!(layers.iter().all(|layer| layer.canvas.is_none()), "{style}");
        // Its item is in the page's layer.
        assert!(
            p.artifact
                .items
                .iter()
                .any(|item| item.kind == paint::DisplayItemKind::Canvas),
            "{style}"
        );
    }
    // Under a translation, a layer in its own space (not the root's).
    let p = canvas("transform: translate(5px, 5px)");
    let layer = layers_of(&p)
        .into_iter()
        .find(|layer| layer.canvas.is_some())
        .expect("a canvas layer");
    let chunk = &p.artifact.chunks[layer.chunks.start];
    assert_ne!(chunk.state.transform, TransformNodeId(0));
    assert_eq!(layer.transform(&p.pre.trees), chunk.state.transform);
}

/// In a scroller that is a layer, a canvas's layer is moved with it.
#[test]
fn a_canvas_in_a_scroller_layer_scrolls_with_it() {
    let p = paint_canvases(
        "<style>html, body { margin: 0 }</style>
         <div style='width: 100px; height: 50px; overflow: auto'>
           <canvas width=20 height=10 style='display: block'></canvas>
           <div style='height: 300px'></div>
         </div>",
    );
    assert_eq!(paint::composited_scrollers(&p.pre.trees), [true]);
    let layer = layers_of(&p)
        .into_iter()
        .find(|layer| layer.canvas.is_some())
        .expect("a canvas layer");
    assert_eq!(layer.scroller, Some(ScrollNodeId(0)));
    // Its space is under the scroller's scroll translation.
    let scrolled = p
        .pre
        .trees
        .scroll(paint::ScrollNodeId(0))
        .scroll_translation;
    let mut node = Some(layer.transform(&p.pre.trees));
    let mut under = false;
    while let Some(t) = node {
        under |= t == scrolled;
        node = p.pre.trees.transform[t.0].parent;
    }
    assert!(under);
}

/// A canvas under one rounded clip (its own `border-radius`) is a layer,
/// under two it is painted into the page.
#[test]
fn a_canvas_is_a_layer_under_one_rounded_clip_not_two() {
    let rounded = paint_canvases(
        "<canvas width=20 height=10 style='display: block; border-radius: 5px'></canvas>",
    );
    let layer = layers_of(&rounded)
        .into_iter()
        .find(|layer| layer.canvas.is_some())
        .expect("a canvas layer");
    // Cut to its own rounded clip.
    let chunk = &rounded.artifact.chunks[layer.chunks.start];
    assert_ne!(chunk.state.clip, ClipNodeId(0));
    assert_eq!(layer.clip(&rounded.pre.trees), chunk.state.clip);
    let two = paint_canvases(
        "<div style='width: 200px; height: 200px; overflow: hidden; border-radius: 20px'>
           <canvas width=20 height=10 style='display: block; border-radius: 5px'></canvas>
         </div>",
    );
    assert!(layers_of(&two).iter().all(|layer| layer.canvas.is_none()));
}

/// A fixed box a `transform` around it contains scrolls with it: it is
/// no fixed layer, nor is anything else.
#[test]
fn a_contained_fixed_box_is_no_fixed_layer() {
    let p = paint_html(
        "<style>html, body { margin: 0 }</style>
         <div style='transform: translate(0); height: 50px'>
           <div style='position: fixed; top: 0; width: 200px; height: 30px; background-color: green'></div>
         </div>
         <div style='height: 2000px; background-color: blue'></div>",
    );
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    assert!(layers.iter().all(|layer| !layer.fixed), "{layers:?}");
}

/// The keys of the sticky boxes of `p`, by their `id`s, and whether the
/// compositor moves each.
fn sticky_moved(p: &Painted) -> Vec<(String, bool)> {
    let trees = &p.pre.trees;
    let moved = paint::composited_sticky(trees, &paint::composited_scrollers(trees));
    let names = names(&p.ids);
    trees
        .transform
        .iter()
        .zip(moved)
        .filter_map(|(node, moved)| {
            let key = node.sticky?.key;
            Some((names[&key.0].clone(), moved))
        })
        .collect()
}

/// Each element's `id` by its node.
fn names(ids: &HashMap<String, NodeId>) -> HashMap<NodeId, String> {
    ids.iter().map(|(name, &id)| (id, name.clone())).collect()
}

const STICKY: &str = "<style>html, body { margin: 0 } .s { position: sticky; top: 0; height: 20px }
       .c { height: 100px; overflow: auto } .tall { height: 300px }</style>
     <div class=s id=viewport></div>
     <div class=c><div class=s id=layer></div><div class=tall></div></div>
     <div class=c style='opacity: 0.5'><div class=s id=painted></div><div class=tall></div></div>
     <div class=c><div class=s id=still></div></div>
     <div style='opacity: 0.5; height: 1000px'><div class=s id=faded></div></div>
     <div style='position: fixed; top: 0; height: 50px; width: 50px'><div class=s id=fixed></div></div>
     <div style='height: 2000px'></div>";

/// The compositor moves a sticky box in the main frame's viewport or in a
/// container that is a layer, if what is in it can be a layer; not one in a
/// container painted with what is around it (under an opacity, or with
/// nothing to scroll), under an opacity, or in a box fixed to the viewport.
#[test]
fn the_compositor_moves_the_sticky_boxes_that_can_be_layers() {
    let p = paint_html(STICKY);
    assert_eq!(
        sticky_moved(&p),
        [
            ("viewport".into(), true),
            ("layer".into(), true),
            ("painted".into(), false),
            ("still".into(), false),
            ("faded".into(), false),
            ("fixed".into(), false),
        ]
    );
}

/// A sticky box the compositor moves is a layer of its own, in its
/// translation's space, as big as what it paints there: not cut by the
/// clips around it (the compositor's, where it is drawn), but by those in
/// it. It is stacked: painted after the content around it, in flow.
#[test]
fn a_sticky_box_the_compositor_moves_is_a_layer() {
    let html = "<style>html, body { margin: 0 }</style>
        <div style='height: 100px; width: 100px; overflow: hidden'>
          <div style='height: 200px'></div>
          <div style='position: sticky; bottom: 0; height: 20px; overflow: clip; background: green'>
            <div style='height: 50px; width: 300px; background: blue'></div>
          </div>
          <div style='height: 100px; background: red'></div>
        </div>
        <div style='position: sticky; top: 0; height: 20px; background: green'></div>
        <div style='height: 2000px; background: red'></div>";
    let p = paint_html(html);
    let layers = paint::layerize(
        &p.artifact,
        &p.pre.trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    let owners: Vec<_> = layers
        .iter()
        .map(|l| (l.scroller, l.sticky.is_some()))
        .collect();
    assert_eq!(
        owners,
        [
            (None, false),
            (Some(ScrollNodeId(0)), false),
            (None, false),
            (Some(ScrollNodeId(0)), true),
            (None, true),
        ]
    );
    // The bottom one is laid out at 200, below its container's 100: what
    // it paints, cut to its own 100x20, out to the 256px grid.
    let bottom = &layers[3];
    assert_eq!(bottom.bounds, RectF::new(0.0, 0.0, 256.0, 256.0));
    assert_eq!(bottom.transform(&p.pre.trees), bottom.sticky.unwrap());
    assert_eq!(
        bottom.clip(&p.pre.trees),
        p.pre.trees.scroll(ScrollNodeId(0)).overflow_clip
    );
    // The top one, 800px wide at 100.
    let top = &layers[4];
    assert_eq!(top.bounds, RectF::new(0.0, 0.0, 1024.0, 256.0));
    assert_eq!(top.clip(&p.pre.trees), ClipNodeId(0));
}

/// A container in a sticky box the compositor moves is a layer moved by
/// both: its content in its own layer, in its scroll translation's space
/// under the box's translation.
#[test]
fn a_container_in_a_sticky_box_moves_with_it() {
    let html = "<style>html, body { margin: 0 }</style>
        <div style='position: sticky; top: 0; background: green'>
          <div style='height: 100px; width: 100px; overflow: auto'>
            <div style='height: 300px; background: blue'></div>
          </div>
        </div>
        <div style='height: 2000px'></div>";
    let p = paint_html(html);
    let trees = &p.pre.trees;
    let layers = paint::layerize(
        &p.artifact,
        trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    let owners: Vec<_> = layers
        .iter()
        .map(|l| (l.scroller, l.sticky.is_some()))
        .collect();
    assert_eq!(
        owners,
        [(None, false), (None, true), (Some(ScrollNodeId(0)), false)]
    );
    let sticky = layers[1].sticky.unwrap();
    let mut node = Some(layers[2].transform(trees));
    let mut chain = Vec::new();
    while let Some(n) = node {
        chain.push(n);
        node = trees.transform[n.0].parent;
    }
    assert!(chain.contains(&sticky));
}

/// What paint paints with: the sticky boxes the compositor moves where
/// they were laid out, those it doesn't where they stick.
#[test]
fn unscrolled_unmoves_the_sticky_boxes_the_compositor_moves() {
    let (view, ids) = page::lay_out(STICKY, VIEWPORT);
    let positions = layout::StickyPositions::new(&view, VIEWPORT);
    let stuck = layout::StickyState {
        offsets: positions.offsets(&Default::default(), gfx::PointF::new(0.0, 500.0)),
        reaches: positions.reaches(),
    };
    let pre = paint::pre_paint_frame_tree(
        &view,
        &Default::default(),
        &stuck,
        &Default::default(),
        1.0,
        gfx::PointF::new(0.0, 500.0),
        None,
    );
    let names = names(&ids);
    let matrix = |pre: &PrePaint, id: &str| {
        pre.trees
            .transform
            .iter()
            .find(|node| {
                node.sticky
                    .is_some_and(|s| names.get(&s.key.0).is_some_and(|name| name == id))
            })
            .map(|node| node.matrix)
            .unwrap()
    };
    // Scrolled 500: the first at 0 moves 500 down, the faded one at 320
    // (after 20 and three 100px containers) 180.
    assert_eq!(matrix(&pre, "viewport"), Transform::translate(0.0, 500.0));
    assert_eq!(matrix(&pre, "faded"), Transform::translate(0.0, 180.0));
    let unscrolled = pre.unscrolled(&Default::default());
    assert_eq!(matrix(&unscrolled, "viewport"), Transform::IDENTITY);
    assert_eq!(matrix(&unscrolled, "faded"), matrix(&pre, "faded"));
}

/// A sticky box's layer is under the clips around the box, which don't move
/// with it: an `overflow: clip` ancestor's, not only its scroller's.
#[test]
fn a_sticky_layer_is_under_the_clips_around_it() {
    let html = "<style>html, body { margin: 0 }</style>
        <div style='height: 300px; overflow: clip'>
          <div style='position: sticky; top: 0; height: 20px; background: green'></div>
        </div>
        <div style='height: 2000px'></div>";
    let p = paint_html(html);
    let trees = &p.pre.trees;
    let layers = paint::layerize(
        &p.artifact,
        trees,
        layout::document_size(&p.view),
        crate::MAX_TEXTURE_SIZE,
    );
    let sticky = layers
        .iter()
        .find(|l| l.sticky.is_some())
        .expect("a sticky layer");
    let clip = sticky.clip(trees);
    assert_ne!(clip, ClipNodeId(0));
    assert_eq!(
        trees.clip[clip.0].rect,
        Some(RectF::new(0.0, 0.0, 800.0, 300.0))
    );
}
