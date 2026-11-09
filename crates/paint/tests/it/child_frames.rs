//! Child frames: pre-paint and paint go from the main
//! frame through an iframe into its child frame's document, laid out on
//! its own, as Blink's walks go through the local frame tree.

use std::sync::Arc;

use compositor_paint::{PaintFlags, PaintOp};
use gfx::{Color, NodeId, PointF, RectF, SizeF};
use layout::BoxFragment;
use paint::{ChildFrame, ChildFrames, DisplayItemKind, PaintArtifact, PaintController, PrePaint};

use crate::page::Frames;

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

/// Each rect item's kind, and its rect and color in the root space (not
/// the borders, which are paths).
fn fills(pre: &PrePaint, artifact: &PaintArtifact) -> Vec<(DisplayItemKind, RectF, Color)> {
    let to_root = pre.trees.to_root();
    artifact
        .chunks
        .iter()
        .flat_map(|chunk| {
            artifact.items[chunk.items.clone()]
                .iter()
                .map(move |item| (chunk.state, item))
        })
        .filter_map(|(state, item)| match item.op {
            PaintOp::DrawRect {
                rect,
                flags: PaintFlags { color, .. },
            } => Some((item.kind, to_root[state.transform.0].map_rect(rect), color)),
            _ => None,
        })
        .collect()
}

const PAGE: &str = "<style>body { margin: 0 }</style>\
    <div style='height: 10px'></div>\
    <iframe id=f style='display: block; width: 100px; height: 60px; \
      border: 5px solid blue; padding: 3px'></iframe>\
    <div id=after style='height: 10px; background: yellow'></div>";

const CHILD: &str = "<style>html { background: lime } body { margin: 0 }</style>\
    <div id=c style='width: 50px; height: 20px; margin-left: 7px; background: navy'></div>";

const NAVY: Color = Color::rgb(0, 0, 128);
const LIME: Color = Color::rgb(0, 255, 0);

/// The child frame's document is painted where the iframe's content would
/// be, after the page's backgrounds (the replaced content's phase): its
/// canvas over the content box, its boxes placed there, clipped to it.
#[test]
fn a_child_frame_is_painted_in_its_iframe() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let child_frames = ChildFrames::from_iter([(
        frames.id("f"),
        frames.child_frame(&child, PointF::new(0.0, 0.0)),
    )]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
    let painted = fills(&pre, &artifact);
    let yellow = painted
        .iter()
        .position(|(_, _, color)| *color == Color::rgb(255, 255, 0))
        .expect("#after painted");
    assert_eq!(
        painted[yellow + 1..],
        [
            (
                DisplayItemKind::DocumentBackground,
                RectF::new(8.0, 18.0, 100.0, 60.0),
                LIME
            ),
            (
                DisplayItemKind::BoxDecorationBackground,
                RectF::new(15.0, 18.0, 50.0, 20.0),
                NAVY
            ),
        ]
    );
    // Clipped to the content box, in the page's space.
    let state = artifact.chunks.last().expect("the child's chunk").state;
    let clips: Vec<_> = pre
        .trees
        .clip_chain(state.clip)
        .into_iter()
        .map(|clip| pre.trees.clip[clip.0].rect)
        .collect();
    assert_eq!(clips, [Some(RectF::new(8.0, 18.0, 100.0, 60.0))]);
    // Where its elements are, as queries see them.
    assert_eq!(
        paint::element_bounds(&page, &pre, frames.id("c")),
        Some(RectF::new(15.0, 18.0, 50.0, 20.0))
    );
    // Without the child frame (never laid out), the iframe shows nothing.
    let pre = paint::pre_paint(&page, &Default::default(), 1.0);
    let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
    assert_eq!(fills(&pre, &artifact).len(), yellow + 1);
    assert_eq!(paint::element_bounds(&page, &pre, frames.id("c")), None);
}

/// A `visibility: hidden` iframe shows nothing of its child frame, which
/// isn't hit either (Chrome's `LayoutEmbeddedContent` paints and hits
/// nothing then).
#[test]
fn a_hidden_iframe_shows_no_child_frame() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <div id=under style='height: 60px'></div>\
         <iframe id=f style='display: block; width: 100px; height: 60px; margin-top: -60px; \
           border: 0; visibility: hidden'></iframe>",
        VIEWPORT,
    );
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let child_frames = ChildFrames::from_iter([(
        frames.id("f"),
        frames.child_frame(&child, PointF::new(0.0, 0.0)),
    )]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
    assert!(
        fills(&pre, &artifact)
            .iter()
            .all(|(_, _, color)| *color != LIME && *color != NAVY),
        "nothing of the child frame"
    );
    let node = paint::hit_test(&page, &pre, PointF::new(20.0, 10.0)).node;
    assert_eq!(node, Some(frames.id("under")));
}

/// A `pointer-events: none` iframe's child frame isn't hit either.
#[test]
fn a_child_frame_under_pointer_events_none_isnt_hit() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <div id=under style='height: 60px'></div>\
         <iframe id=f style='display: block; width: 100px; height: 60px; margin-top: -60px; \
           border: 0; pointer-events: none'></iframe>",
        VIEWPORT,
    );
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let child_frames = ChildFrames::from_iter([(
        frames.id("f"),
        frames.child_frame(&child, PointF::new(0.0, 0.0)),
    )]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let node = paint::hit_test(&page, &pre, PointF::new(20.0, 10.0)).node;
    assert_eq!(node, Some(frames.id("under")));
}

/// A child frame's viewport scroll moves its document within the iframe;
/// its scroll containers are moved by their offsets. The viewport is a
/// scroll node in the iframe (its element the document), around the child
/// frame's containers and in what is around the iframe; each says which
/// frame's it is.
#[test]
fn a_child_frame_is_scrolled_and_has_scroll_nodes() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <div id=outer style='height: 50px; overflow: auto'><div style='height: 100px'></div></div>\
         <iframe id=f style='display: block; border: 0; width: 100px; height: 60px'></iframe>",
        VIEWPORT,
    );
    let child = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <div id=s style='height: 30px; overflow: auto'>\
           <div id=inner style='height: 100px'></div></div>\
         <div style='height: 1000px'></div>",
        SizeF::new(100.0, 60.0),
    );
    let mut frame = frames.child_frame(&child, PointF::new(0.0, 5.0));
    frame.scroll.insert(frames.id("s"), PointF::new(0.0, 7.0));
    let child_frames = ChildFrames::from_iter([(frames.id("f"), frame)]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    assert_eq!(
        paint::element_bounds(&page, &pre, frames.id("s")).map(|r| r.y),
        Some(50.0 - 5.0)
    );
    assert_eq!(
        paint::element_bounds(&page, &pre, frames.id("inner")).map(|r| r.y),
        Some(50.0 - 5.0 - 7.0)
    );
    let (f, document) = (frames.id("f"), frame_document(&frames, &child));
    let scrollers: Vec<_> = pre
        .trees
        .scroll
        .iter()
        .flatten()
        .map(|node| (node.element, node.frame, node.parent.map(|p| p.0)))
        .collect();
    assert_eq!(
        scrollers,
        [
            (frames.id("outer"), None, None),
            (document, Some(f), None),
            (frames.id("s"), Some(f), Some(1)),
        ]
    );
    let viewport = &pre.trees.scroll(paint::ScrollNodeId(1));
    assert_eq!(viewport.container, RectF::new(0.0, 0.0, 100.0, 60.0));
    assert_eq!(viewport.border_box, viewport.container);
    assert_eq!(viewport.contents, layout::document_size(&child));
    assert_eq!(
        pre.trees.to_root()[viewport.transform.0].map_point(PointF::new(0.0, 0.0)),
        PointF::new(0.0, 50.0),
        "the content box, unscrolled"
    );
    assert_eq!(
        pre.trees.to_root()[viewport.scroll_translation.0].map_point(PointF::new(0.0, 0.0)),
        PointF::new(0.0, 45.0),
        "scrolled"
    );
}

/// The document a child frame's layout is of.
fn frame_document(frames: &Frames, view: &Arc<BoxFragment>) -> NodeId {
    frames.document(view)
}

/// A frame in a child frame's document: the walks go all the way down.
#[test]
fn nested_child_frames() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <iframe id=inner style='display: block; margin-left: 4px; border: 0; \
           width: 40px; height: 30px'></iframe>",
        SizeF::new(100.0, 60.0),
    );
    let grandchild = frames.lay_out(CHILD, SizeF::new(40.0, 30.0));
    let child_frames = ChildFrames::from_iter([
        (
            frames.id("f"),
            frames.child_frame(&child, PointF::new(0.0, 0.0)),
        ),
        (
            frames.id("inner"),
            frames.child_frame(&grandchild, PointF::new(0.0, 0.0)),
        ),
    ]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
    assert_eq!(
        fills(&pre, &artifact).last(),
        Some(&(
            DisplayItemKind::BoxDecorationBackground,
            RectF::new(8.0 + 4.0 + 7.0, 18.0, 50.0, 20.0),
            NAVY
        ))
    );
    assert_eq!(
        pre.trees
            .clip_chain(artifact.chunks.last().expect("a chunk").state.clip)
            .len(),
        2,
        "clipped by both iframes"
    );
}

/// The iframe's line-phase items are never taken from the last paint as
/// they were: they hold its child frame's, which paint from what the child
/// frame is now, taking what they can themselves.
#[test]
fn a_child_frame_paints_anew_in_an_unchanged_iframe() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let one = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let two = frames.lay_out(&CHILD.replace("navy", "red"), SizeF::new(100.0, 60.0));
    let mut controller = PaintController::default();
    let mut paint_with = |child: &Arc<BoxFragment>| {
        let child_frames = ChildFrames::from_iter([(
            frames.id("f"),
            frames.child_frame(child, PointF::new(0.0, 0.0)),
        )]);
        let pre = paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &child_frames,
            1.0,
            PointF::new(0.0, 0.0),
            None,
        );
        let artifact = controller.paint(&page, &pre, layout::document_size(&page), &());
        fills(&pre, &artifact).last().map(|(_, _, color)| *color)
    };
    assert_eq!(paint_with(&one), Some(NAVY));
    assert_eq!(paint_with(&two), Some(Color::rgb(255, 0, 0)));
    assert_eq!(paint_with(&two), Some(Color::rgb(255, 0, 0)));
}

/// An iframe whose child frame went (navigated, not loaded yet) paints none
/// of it, though nothing else about it changed: its line-phase items held
/// the child frame's, not taken from the last paint.
#[test]
fn an_iframe_whose_child_frame_went_paints_none_of_it() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let mut controller = PaintController::default();
    let mut paint_with = |child_frames: ChildFrames| {
        let pre = paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &child_frames,
            1.0,
            PointF::new(0.0, 0.0),
            None,
        );
        let artifact = controller.paint(&page, &pre, layout::document_size(&page), &());
        fills(&pre, &artifact)
            .iter()
            .any(|(_, _, color)| *color == NAVY)
    };
    let f = frames.id("f");
    assert!(paint_with(ChildFrames::from_iter([(
        f,
        frames.child_frame(&child, PointF::new(0.0, 0.0))
    )])));
    assert!(!paint_with(ChildFrames::default()));
}

/// Hit testing goes into a child frame where it was painted: what is hit in
/// its document, or else its root element (its canvas) within the
/// iframe's content box; the iframe on its border and padding; the page
/// where the child frame's document is clipped away. Points come out in the
/// child frame's viewport for its events. Listing every element there goes
/// into the child frame too, but not for its canvas.
#[test]
fn hit_testing_goes_into_child_frames() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style><div style='height: 10px'></div>\
         <iframe id=f style='display: block; width: 100px; height: 60px; \
           border: 5px solid blue; padding: 3px'></iframe>\
         <div id=below style='height: 50px'></div>",
        VIEWPORT,
    );
    let child = frames.lay_out(
        "<html id=root><style>body { margin: 0 }</style>\
         <div id=c style='width: 50px; height: 20px; margin-left: 7px'></div>\
         <div id=wide style='width: 500px; height: 20px'></div>",
        SizeF::new(100.0, 60.0),
    );
    let child_frames = ChildFrames::from_iter([(
        frames.id("f"),
        frames.child_frame(&child, PointF::new(0.0, 4.0)),
    )]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let at = |x: f32, y: f32| {
        let node = paint::hit_test(&page, &pre, PointF::new(x, y)).node;
        frames
            .ids
            .iter()
            .find(|(_, id)| Some(**id) == node)
            .map(|(name, _)| name.as_str())
    };
    // The content box starts at (8, 18); the child frame is scrolled 4 down.
    assert_eq!(at(20.0, 18.0), Some("c"));
    assert_eq!(at(20.0, 33.0), Some("c"));
    assert_eq!(at(20.0, 34.0), Some("wide"));
    // Below its body and root element, which end at 40 in the child.
    assert_eq!(at(100.0, 70.0), Some("root"), "the child frame's canvas");
    assert_eq!(at(6.0, 30.0), Some("f"), "the iframe's padding");
    assert_eq!(at(2.0, 30.0), Some("f"), "the iframe's border");
    // `#wide` reaches past the content box: clipped there.
    assert_eq!(at(110.0, 40.0), Some("f"));
    assert_eq!(at(150.0, 40.0), None);
    assert_eq!(at(20.0, 100.0), Some("below"));
    // The child frame's elements, then the iframe and the page's body and
    // root element (no ids). Its canvas isn't one: Blink lists its
    // document, which `elementsFromPoint` skips.
    let all = |x: f32, y: f32| -> Vec<Option<&str>> {
        paint::hit_test_all(&page, &pre, PointF::new(x, y))
            .iter()
            .map(|node| {
                frames
                    .ids
                    .iter()
                    .find(|(_, id)| *id == node)
                    .map(|(name, _)| name.as_str())
            })
            .collect()
    };
    assert_eq!(
        all(20.0, 18.0),
        [Some("c"), None, Some("root"), Some("f"), None, None]
    );
    assert_eq!(all(100.0, 70.0), [Some("f"), None, None]);
    // A wheel starts at the child frame's viewport (the only scroll node),
    // over its content and its canvas, not over the iframe's border.
    let scroll = |x: f32, y: f32| paint::hit_test(&page, &pre, PointF::new(x, y)).scroll;
    let viewport = Some(paint::ScrollNodeId(0));
    assert_eq!(
        pre.trees.scroll(paint::ScrollNodeId(0)).element,
        frame_document(&frames, &child)
    );
    assert_eq!(scroll(20.0, 18.0), viewport);
    assert_eq!(scroll(100.0, 70.0), viewport);
    assert_eq!(scroll(2.0, 30.0), None);
    assert_eq!(
        pre.point_in_child_frame(frames.id("f"), PointF::new(20.0, 30.0)),
        Some(PointF::new(12.0, 12.0)),
        "less the content box's position, as the viewport shows it"
    );
    assert_eq!(
        pre.point_in_child_frame(frames.id("c"), PointF::new(20.0, 30.0)),
        None
    );
}

/// Through nested child frames, and an iframe's transform.
#[test]
fn hit_testing_nested_and_transformed_child_frames() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <iframe id=f style='display: block; border: 0; width: 100px; height: 60px; \
           transform: translate(30px, 40px)'></iframe>",
        VIEWPORT,
    );
    let child = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <iframe id=inner style='display: block; margin-left: 4px; border: 0; \
           width: 40px; height: 30px'></iframe>",
        SizeF::new(100.0, 60.0),
    );
    let grandchild = frames.lay_out(CHILD, SizeF::new(40.0, 30.0));
    let child_frames = ChildFrames::from_iter([
        (
            frames.id("f"),
            frames.child_frame(&child, PointF::new(0.0, 0.0)),
        ),
        (
            frames.id("inner"),
            frames.child_frame(&grandchild, PointF::new(0.0, 0.0)),
        ),
    ]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    // `#c` is at (7, 0) in the grandchild, which is at (4, 0) in the child,
    // which is at (30, 40).
    let point = PointF::new(30.0 + 4.0 + 7.0 + 1.0, 41.0);
    assert_eq!(
        paint::hit_test(&page, &pre, point).node,
        Some(frames.id("c"))
    );
    assert_eq!(
        pre.point_in_child_frame(frames.id("inner"), point),
        Some(PointF::new(8.0, 1.0))
    );
    assert_eq!(
        pre.point_in_child_frame(frames.id("f"), point),
        Some(PointF::new(12.0, 1.0))
    );
}

/// A child frame's viewport that is a layer is the compositor's to scroll,
/// unless something painted after the iframe may cover it: then a wheel
/// there needs the main thread, as for any container. So does one hit
/// testing doesn't find, in a `pointer-events: none` iframe: what is under
/// it is the main thread's to find.
#[test]
fn child_frames_viewports_in_the_scroll_hit_test_region() {
    let region_with = |iframe: &str, cover: &str| {
        let mut frames = Frames::new();
        let page = frames.lay_out(
            &format!(
                "<style>body {{ margin: 0 }}</style>\
                 <iframe id=f style='display: block; border: 0; width: 100px; \
                   height: 60px; {iframe}'></iframe>{cover}"
            ),
            VIEWPORT,
        );
        let child = frames.lay_out(
            "<style>body { margin: 0 }</style><div style='height: 200px'></div>",
            SizeF::new(100.0, 60.0),
        );
        let child_frames = ChildFrames::from_iter([(
            frames.id("f"),
            frames.child_frame(&child, PointF::new(0.0, 0.0)),
        )]);
        let pre = paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &child_frames,
            1.0,
            PointF::new(0.0, 0.0),
            None,
        );
        let composited = paint::composited_scrollers(&pre.trees);
        assert_eq!(composited, [true], "the viewport is a layer");
        let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
        paint::main_thread_scroll_hit_test_region(&artifact, &pre, &composited)
    };
    let region = |cover: &str| region_with("", cover);
    assert_eq!(
        region(""),
        paint::ScrollHitTestRegion {
            root: Vec::new(),
            scrollers: vec![Vec::new()],
            sticky: Vec::new(),
        }
    );
    // A block's background is under the iframe's content (painted in the
    // line phase); a stacking context is over it.
    let under = region("<div style='margin-top: -10px; width: 20px; height: 20px'></div>");
    assert_eq!(under.root, []);
    let covered =
        region("<div style='margin-top: -10px; width: 20px; height: 20px; opacity: 0.5'></div>");
    assert_eq!(covered.root, [RectF::new(-1.0, -1.0, 102.0, 62.0)]);
    let not_hit = region_with("pointer-events: none", "");
    assert_eq!(not_hit.root, [RectF::new(-1.0, -1.0, 102.0, 62.0)]);
}

/// A child frame's root element inside a blocking `wheel` listener has its
/// canvas inside it too: the wheel event handler region covers the
/// iframe's content box.
#[test]
fn a_child_frames_listener_is_over_its_canvas() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <iframe id=f style='display: block; border: 0; width: 100px; height: 60px'></iframe>",
        VIEWPORT,
    );
    let child = frames.lay_out(
        "<html data-wheel><style>html, body { margin: 0; height: 10px }</style></html>",
        SizeF::new(100.0, 60.0),
    );
    let child_frames = ChildFrames::from_iter([(
        frames.id("f"),
        frames.child_frame(&child, PointF::new(0.0, 0.0)),
    )]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let composited = paint::composited_scrollers(&pre.trees);
    let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
    let region = paint::wheel_event_handler_region(&artifact, &pre, &composited, false);
    let all: Vec<RectF> = region
        .root
        .iter()
        .chain(region.scrollers.iter().flatten())
        .copied()
        .collect();
    assert!(
        all.iter().any(|r| r.contains(PointF::new(50.0, 50.0))),
        "{region:?}"
    );
}

/// An iframe in a scroll container: its child frame's viewport is in that
/// container's chain, so a wheel goes on to it from the child frame.
#[test]
fn a_child_frames_viewport_is_in_the_chain_around_its_iframe() {
    let mut frames = Frames::new();
    let page = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <div id=outer style='height: 50px; overflow: auto'>\
           <iframe id=f style='display: block; border: 0; width: 100px; height: 60px'></iframe>\
         </div>",
        VIEWPORT,
    );
    let child = frames.lay_out(
        "<style>body { margin: 0 }</style><div style='height: 200px'></div>",
        SizeF::new(100.0, 60.0),
    );
    let child_frames = ChildFrames::from_iter([(
        frames.id("f"),
        frames.child_frame(&child, PointF::new(0.0, 0.0)),
    )]);
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let chain: Vec<_> = pre
        .trees
        .scroll
        .iter()
        .flatten()
        .map(|node| (node.element, node.parent.map(|p| p.0)))
        .collect();
    assert_eq!(
        chain,
        [
            (frames.id("outer"), None),
            (frame_document(&frames, &child), Some(0)),
        ]
    );
}

/// A child frame laid out again is walked into again, though its iframe's
/// fragment is the one walked before; scrolling its viewport moves it in
/// place. (Debug builds check each against a pre-paint from scratch.)
#[test]
fn a_child_frames_new_layout_and_scroll_are_walked_into() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let f = frames.id("f");
    let pre_paint = |frame: ChildFrame, previous: Option<PrePaint>| {
        paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &ChildFrames::from_iter([(f, frame)]),
            1.0,
            PointF::new(0.0, 0.0),
            previous,
        )
    };
    let first = pre_paint(frames.child_frame(&child, PointF::new(0.0, 0.0)), None);
    let again = frames.lay_out(
        "<div style='width: 30px; height: 300px; opacity: 0.5'></div>",
        SizeF::new(100.0, 60.0),
    );
    let second = pre_paint(
        frames.child_frame(&again, PointF::new(0.0, 0.0)),
        Some(first),
    );
    assert!(std::ptr::eq(
        second.child_frame_layout_view(f).unwrap(),
        &*again
    ));
    let document = frames.document(&again);
    let scrolled = pre_paint(
        frames.child_frame(&again, PointF::new(0.0, 20.0)),
        Some(second),
    );
    let (_, viewport) = scrolled
        .trees
        .scroll_nodes()
        .find(|(_, node)| node.element == document)
        .unwrap();
    assert_eq!(
        scrolled.trees.transform[viewport.scroll_translation.0].matrix,
        gfx::Transform::translate(0.0, -20.0)
    );
}

/// An iframe given a child frame is walked into, though nothing around it
/// changed; one whose child frame is gone isn't.
#[test]
fn an_iframe_given_a_child_frame_is_walked_into() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(CHILD, SizeF::new(100.0, 60.0));
    let f = frames.id("f");
    let first = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &ChildFrames::default(),
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    assert!(first.child_frame_layout_view(f).is_none());
    let second = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &ChildFrames::from_iter([(f, frames.child_frame(&child, PointF::new(0.0, 0.0)))]),
        1.0,
        PointF::new(0.0, 0.0),
        Some(first),
    );
    assert!(second.child_frame_layout_view(f).is_some());
    // And one whose child frame is gone lets go of what was in it.
    let third = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &ChildFrames::default(),
        1.0,
        PointF::new(0.0, 0.0),
        Some(second),
    );
    assert!(third.child_frame_layout_view(f).is_none());
    assert_eq!(third.trees.scroll_nodes().count(), 0);
}

/// Scrolling a container in a child frame sets its scroll translation in
/// place, to its offset in its frame's.
#[test]
fn a_container_in_a_child_frame_scrolls_in_place() {
    let mut frames = Frames::new();
    let page = frames.lay_out(PAGE, VIEWPORT);
    let child = frames.lay_out(
        "<style>body { margin: 0 }</style>\
         <div id=s style='width: 50px; height: 20px; overflow: scroll'><div style='height: 100px'></div></div>",
        SizeF::new(100.0, 60.0),
    );
    let (f, s) = (frames.id("f"), frames.id("s"));
    let pre_paint = |at: PointF, previous: Option<PrePaint>| {
        let mut frame = frames.child_frame(&child, PointF::new(0.0, 0.0));
        frame.scroll.insert(s, at);
        paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &ChildFrames::from_iter([(f, frame)]),
            1.0,
            PointF::new(0.0, 0.0),
            previous,
        )
    };
    let first = pre_paint(PointF::new(0.0, 0.0), None);
    let scrolled = pre_paint(PointF::new(0.0, 15.0), Some(first));
    let (_, node) = scrolled
        .trees
        .scroll_nodes()
        .find(|(_, node)| node.element == s)
        .unwrap();
    assert_eq!(
        scrolled.trees.transform[node.scroll_translation.0].matrix,
        gfx::Transform::translate(0.0, -15.0)
    );
}

/// A child frame's sticky box sticks in its frame's viewport, a scroll
/// node of the frame tree's: the compositor moves it when that viewport is
/// a layer. One in a box fixed to the child frame's viewport isn't moved by
/// any scroll, not even that of a container around the iframe: its offset
/// is painted.
#[test]
fn a_child_frames_sticky_box_sticks_in_its_viewport() {
    let in_scroller = "<style>body { margin: 0 }</style>\
        <div style='height: 100px; overflow: auto'>\
          <iframe id=f style='display: block; width: 100px; height: 60px'></iframe>\
          <div style='height: 500px'></div>\
        </div>";
    for page in [PAGE, in_scroller] {
        let mut frames = Frames::new();
        let page = frames.lay_out(page, VIEWPORT);
        let child = frames.lay_out(
            "<style>body { margin: 0 }</style>\
             <div id=sticky style='position: sticky; top: 0; height: 10px; background: navy'></div>\
             <div style='position: fixed; top: 0; height: 30px; width: 30px'>\
               <div id=fixed style='position: sticky; top: 0; height: 10px; background: navy'></div>\
             </div>\
             <div style='height: 500px'></div>",
            SizeF::new(100.0, 60.0),
        );
        let child_frames = ChildFrames::from_iter([(
            frames.id("f"),
            frames.child_frame(&child, PointF::new(0.0, 0.0)),
        )]);
        let pre = paint::pre_paint_frame_tree(
            &page,
            &Default::default(),
            &Default::default(),
            &child_frames,
            1.0,
            PointF::new(0.0, 0.0),
            None,
        );
        let trees = &pre.trees;
        let viewport = trees
            .scroll_nodes()
            .find(|(_, node)| node.frame == Some(frames.id("f")))
            .map(|(id, _)| id)
            .expect("the child frame's viewport");
        let scroll = |id: &str| {
            trees
                .transform
                .iter()
                .find_map(|node| node.sticky.filter(|s| s.key.0 == frames.id(id)))
                .map(|s| s.scroll)
        };
        assert_eq!(
            scroll("sticky"),
            Some(paint::StickyScroll::Scroller(viewport))
        );
        assert_eq!(scroll("fixed"), Some(paint::StickyScroll::Fixed));
        let moved = paint::composited_sticky(trees, &paint::composited_scrollers(trees));
        assert_eq!(moved.iter().filter(|m| **m).count(), 1);
    }
}

/// Paints `page` with the child frames `children` (each iframe's id with its
/// document's markup, laid out at the iframe's size) and gives the frame
/// attribution region, and the children's documents, in order.
fn attribution(
    frames: &mut Frames,
    page: &str,
    children: &[(&str, &str, SizeF)],
) -> (paint::FrameAttributionRegion, Vec<NodeId>) {
    let page = frames.lay_out(page, VIEWPORT);
    let mut documents = Vec::new();
    let child_frames: ChildFrames = children
        .iter()
        .map(|(iframe, html, size)| {
            let child = frames.lay_out(html, *size);
            let frame = frames.child_frame(&child, PointF::new(0.0, 0.0));
            documents.push(frame.document);
            (frames.id(iframe), frame)
        })
        .collect();
    let pre = paint::pre_paint_frame_tree(
        &page,
        &Default::default(),
        &Default::default(),
        &child_frames,
        1.0,
        PointF::new(0.0, 0.0),
        None,
    );
    let composited = paint::composited_scrollers(&pre.trees);
    let artifact = paint::paint(&page, &pre, layout::document_size(&page), &());
    (
        paint::frame_attribution_region(&artifact, &pre, &composited),
        documents,
    )
}

const IFRAME: &str = "display: block; border: 0; width: 100px; height: 60px";
const SIZE: SizeF = SizeF::new(100.0, 60.0);

/// What hit testing finds painted after a child frame, over it, is a rect
/// of the main frame's (`None`): an empty box and one of `opacity: 0` too,
/// not one of `pointer-events: none` or `visibility: hidden`, nor one
/// beside it (Chrome 152's isInputPending()). The child frame's canvas is
/// its own frame's, and before them.
#[test]
fn what_is_painted_over_a_child_frame_is_its_parents() {
    let mut frames = Frames::new();
    let (region, _) = attribution(
        &mut frames,
        &format!(
            "<style>body {{ margin: 0 }} div {{ position: absolute; width: 10px; height: 10px }}</style>\
             <iframe id=f style='{IFRAME}'></iframe>\
             <div style='left: 10px; top: 10px'></div>\
             <div style='left: 30px; top: 10px; opacity: 0'></div>\
             <div style='left: 50px; top: 10px; pointer-events: none; background: red'></div>\
             <div style='left: 70px; top: 10px; visibility: hidden; background: red'></div>\
             <div style='left: 200px; top: 10px'></div>"
        ),
        &[("f", "", SIZE)],
    );
    assert_eq!(region.frames.len(), 1);
    assert_eq!(region.frames[0].parent, None);
    let rects: Vec<_> = region
        .rects
        .iter()
        .map(|r| (r.rect, r.frame, r.masked))
        .collect();
    assert_eq!(
        rects,
        [
            (RectF::new(0.0, 0.0, 100.0, 60.0), Some(0), false),
            (RectF::new(10.0, 10.0, 10.0, 10.0), None, false),
            (RectF::new(30.0, 10.0, 10.0, 10.0), None, false),
        ]
    );
    assert!(region.rects.windows(2).all(|w| w[0].order < w[1].order));
    assert!(
        region
            .rects
            .iter()
            .all(|r| r.space == paint::AttributionSpace::Document)
    );
}

/// A child frame's canvas under a rounded clip or a `clip-path`, its own
/// `border-radius` included, is masked (cc's `HitTestMayBeAffectedByMask`):
/// no frame can be told there. One under a plain clip isn't.
#[test]
fn a_child_frame_under_a_rounded_clip_is_masked() {
    let mut frames = Frames::new();
    let (region, documents) = attribution(
        &mut frames,
        &format!(
            "<style>body {{ margin: 0 }}</style>\
             <iframe id=a style='{IFRAME}; border-radius: 10px'></iframe>\
             <div style='clip-path: inset(0 round 5px)'><iframe id=b style='{IFRAME}'></iframe></div>\
             <div style='overflow: hidden'><iframe id=c style='{IFRAME}'></iframe></div>"
        ),
        &[("a", "", SIZE), ("b", "", SIZE), ("c", "", SIZE)],
    );
    let masked: Vec<_> = documents
        .iter()
        .map(|document| {
            let frame = region.frames.iter().position(|f| f.document == *document);
            region
                .rects
                .iter()
                .find(|r| r.frame == frame)
                .expect("a canvas")
                .masked
        })
        .collect();
    assert_eq!(masked, [true, true, false]);
}

/// A child frame's canvas is what the clips that move with it leave of
/// it: an `overflow: clip` box smaller than the frame cuts it. An
/// `overflow: hidden` one is a container that is a layer: the canvas is in
/// its space, whole, and the compositor cuts it where the container is.
#[test]
fn a_child_frames_canvas_is_cut_by_its_clips() {
    let mut frames = Frames::new();
    let (region, _) = attribution(
        &mut frames,
        &format!(
            "<style>body {{ margin: 0 }} div {{ width: 50px; height: 40px }}</style>\
             <div style='overflow: clip'><iframe id=a style='{IFRAME}'></iframe></div>\
             <div style='overflow: hidden'><iframe id=b style='{IFRAME}'></iframe></div>"
        ),
        &[("a", "", SIZE), ("b", "", SIZE)],
    );
    // Each frame's canvas: its first rect (what the second paints, which
    // moves apart from the first, comes after them).
    let canvas = |frame| {
        let rect = region
            .rects
            .iter()
            .find(|r| r.frame == Some(frame))
            .expect("a canvas");
        (rect.rect, rect.space)
    };
    assert_eq!(
        canvas(0),
        (
            RectF::new(0.0, 0.0, 50.0, 40.0),
            paint::AttributionSpace::Document
        )
    );
    assert_eq!(
        canvas(1),
        (
            RectF::new(0.0, 40.0, 100.0, 60.0),
            paint::AttributionSpace::Scroller(paint::ScrollNodeId(1))
        )
    );
}

/// A child frame in a child frame is in that one; what the outer child
/// frame paints over the inner one is the outer one's.
#[test]
fn a_child_frame_in_a_child_frame() {
    let mut frames = Frames::new();
    let (region, _) = attribution(
        &mut frames,
        &format!("<style>body {{ margin: 0 }}</style><iframe id=outer style='{IFRAME}'></iframe>"),
        &[
            (
                "outer",
                "<style>body { margin: 0 }</style>\
                 <iframe id=inner style='display: block; border: 0; width: 50px; height: 30px'></iframe>\
                 <div style='position: absolute; left: 5px; top: 5px; width: 10px; height: 10px'></div>",
                SIZE,
            ),
            ("inner", "", SizeF::new(50.0, 30.0)),
        ],
    );
    assert_eq!(region.frames.len(), 2);
    assert_eq!(region.frames[1].parent, Some(0));
    let rects: Vec<_> = region.rects.iter().map(|r| (r.rect, r.frame)).collect();
    assert_eq!(
        rects,
        [
            (RectF::new(0.0, 0.0, 100.0, 60.0), Some(0)),
            (RectF::new(0.0, 0.0, 50.0, 30.0), Some(1)),
            (RectF::new(5.0, 5.0, 10.0, 10.0), Some(0)),
        ]
    );
}

/// A child frame in a container that is a layer is in that container's
/// space, as the compositor scrolls it; what the main frame paints after it
/// (over the container, a stacking context) that the compositor doesn't
/// scroll with it is a rect wherever it is: the two may overlap once the
/// container scrolls.
#[test]
fn a_child_frame_in_a_container_that_is_a_layer() {
    let mut frames = Frames::new();
    let (region, _) = attribution(
        &mut frames,
        &format!(
            "<style>body {{ margin: 0 }}</style>\
             <div style='height: 80px; overflow: scroll; will-change: transform'>\
               <iframe id=f style='{IFRAME}'></iframe><div style='height: 300px'></div>\
             </div>\
             <div style='position: relative; z-index: 1; height: 10px'></div>"
        ),
        &[("f", "", SIZE)],
    );
    assert!(
        matches!(region.rects[0].space, paint::AttributionSpace::Scroller(_)),
        "{region:?}"
    );
    assert!(
        region
            .rects
            .iter()
            .any(|r| r.space == paint::AttributionSpace::Document && r.frame.is_none()),
        "{region:?}"
    );
}

/// Without child frames there is nothing; an iframe of `pointer-events:
/// none` has its frame but no canvas: hit testing doesn't find it, so the
/// page's frame is under it (Blink's `GetVisibleToHitTesting`).
#[test]
fn nothing_without_a_child_frame_hit_testing_finds() {
    let mut frames = Frames::new();
    let (none, _) = attribution(&mut frames, "<div style='height: 10px'></div>", &[]);
    assert_eq!(none, paint::FrameAttributionRegion::default());
    let (not_hit, _) = attribution(
        &mut frames,
        &format!("<iframe id=f style='{IFRAME}; pointer-events: none'></iframe><div></div>"),
        &[("f", "", SIZE)],
    );
    assert_eq!(not_hit.frames.len(), 1);
    assert!(not_hit.rects.is_empty(), "{not_hit:?}");
}
