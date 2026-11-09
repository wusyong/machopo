//! Paint (`blink/renderer/core/paint`, `platform/graphics/paint`): pre-paint
//! builds the property trees, paint records the fragment tree into a
//! `PaintArtifact` (display items grouped into paint chunks), taking what
//! didn't change from the last paint (`PaintController`); the chunks are
//! then split into layers (`layerize`), each with its own `cc` display
//! list.
//!
//! Painting follows CSS 2 Appendix E. A stacking context is the root, or
//! a box with `opacity < 1`, a transform, or a `z-index` that applies (on
//! a positioned box, a flex or grid item). Within one: its own
//! background, the child stacking contexts with a negative `z-index`,
//! every other block's background and solid borders in tree order, then
//! the lines in tree order, then the other child stacking contexts and the
//! positioned boxes by `z-index` (in tree order among equal ones, `auto`
//! as 0): they paint over their normal-flow siblings even when they come
//! first. A positioned box with `z-index: auto` paints as if it were a
//! stacking context, but the positioned boxes and stacking contexts in it
//! are its stacking context's (Appendix E step 8).
//! Within a line, inline elements' backgrounds and borders and the text
//! interleave in tree order (Appendix E step 7.2.1). An atomic inline (an
//! `inline-block`, an `<img>`) is painted whole where it comes in its line,
//! and a flex or grid item whole in the line phase, where it comes in
//! `order`-modified order, as an inline-block is.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

mod background;
mod clip_path;
mod filter;
mod hit_test;
mod intersection;
mod layerize;
mod paint_controller;
mod property_trees;
mod raster_invalidator;
mod svg;
mod text_decoration;
mod theme;

use std::ops::Range;
use std::sync::Arc;

use base::numerics::saturated_cast;
use compositor_paint::{Dash, DisplayItemList, FilterQuality, LineCap, PaintFlags, PaintOp};
use computed_style::BackgroundBox;
use computed_style::{
    AppliedTextDecoration, BorderStyle, BoxShadow, CaptionSide, ComputedStyle, EmptyCells,
    OutlineStyle, Overflow, Sides, TableDisplay,
};
use gfx::{
    Color, ImageData, NodeId, NodeIdHashMap, NodeIdHashSet, Path, PointF, RRectF, RectF, SizeF,
    Transform,
};
use layout::{
    BoxFragment, BoxRole, CollapsedBorders, InlineBoxFragment, LineEndKind, LineFragment,
    TextFragment,
};

pub use background::{BackgroundImages, Caret, CaretPosition, SelectionColors, TextSelection};
use hit_test::hittable;
pub use hit_test::{
    AttributedFrame, AttributionRect, AttributionSpace, FrameAttributionRegion, HitTestResult,
    ScrollHitTestRegion, element_bounds, frame_attribution_region, hit_test, hit_test_all,
    main_thread_scroll_hit_test_region, paint_offset, wheel_event_handler_region,
};
pub use intersection::{
    IntersectionGeometry, IntersectionRequest, IntersectionRoot, Occlusion, intersections,
};
pub use layerize::{
    CanvasLayer, LAYER_GRID, PendingLayer, composited_scrollers, composited_sticky, layerize,
};
pub use paint_controller::{CachedItems, PaintController, PaintId};
use paint_controller::{PaintCache, PaintedBox, Previous, Start};
pub use property_trees::{
    ClipNode, ClipNodeId, EffectNode, EffectNodeId, PropertyTreeState, PropertyTrees, ScrollNode,
    ScrollNodeId, StickyScroll, StickyTranslation, TransformNode, TransformNodeId,
};
use property_trees::{NodeIds, NodeKey, NodeOwner, NodePlace, NodeRole};
pub use raster_invalidator::PaintedLayer;
pub use svg::svg_shape_contains;

pub use layout::{ScrollOffsets, StickyOffsets, StickyState};

/// A child frame's document as its frame last laid it out, which pre-paint
/// and paint go into at its iframe (Blink's `LocalFrameView` of a
/// `LayoutEmbeddedContent`: one walk goes through the local frame tree).
#[derive(Debug, Clone)]
pub struct ChildFrame {
    pub layout_view: Arc<BoxFragment>,
    /// Its document: the element its viewport's scroll node is under.
    pub document: NodeId,
    /// How far its viewport is scrolled.
    pub viewport_scroll: PointF,
    /// Whether the user can scroll its viewport on each axis (its
    /// `overflow`, and the iframe's `scrolling`).
    pub user_scrollable_x: bool,
    pub user_scrollable_y: bool,
    /// Its scroll containers' offsets.
    pub scroll: ScrollOffsets,
    /// The canvas its document background covers.
    pub document_size: SizeF,
}

/// The same layout, scrolled the same: painted the same.
impl PartialEq for ChildFrame {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.layout_view, &other.layout_view)
            && self.document == other.document
            && self.viewport_scroll == other.viewport_scroll
            && self.user_scrollable_x == other.user_scrollable_x
            && self.user_scrollable_y == other.user_scrollable_y
            && self.scroll == other.scroll
            && self.document_size == other.document_size
    }
}

/// The child frames of a frame tree, all the way down, by their iframe.
pub type ChildFrames = NodeIdHashMap<ChildFrame>;

/// How far past its text box (its advance, and the font's ascent and
/// descent) a run's glyphs at `size` may draw, across and up and down:
/// what its item's visual rect allows for, glyphs' ink being unknown here.
pub(crate) fn glyph_reach(size: f32) -> (f32, f32) {
    (size, size / 2.0)
}

/// What a display item paints (`blink::DisplayItem::Type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayItemKind {
    /// The canvas background, taken from the root element or `<body>`.
    DocumentBackground,
    BoxDecorationBackground,
    /// Glyphs of one text fragment.
    Text,
    /// The background of the selected part of a text fragment (Blink's
    /// `HighlightPainter`, part of its text's painting).
    SelectionBackground,
    /// The caret, after its block's lines (Blink's `CaretDisplayItemClient`).
    Caret,
    /// An underline, overline or line-through.
    TextDecoration,
    /// A replaced element's image.
    Image,
    /// A canvas's bitmap, in a chunk of its own: one that could be a layer
    /// of its own (Blink's foreign layer of a composited canvas).
    Canvas,
    /// An outline or focus ring.
    Outline,
    /// An SVG shape's fill or stroke.
    SvgShape,
    /// SVG text's glyphs, filled or stroked.
    SvgText,
}

/// One recorded paint operation.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayItem {
    /// The element it paints; `None` for the canvas background of a
    /// document whose root has none.
    pub client: Option<NodeId>,
    pub kind: DisplayItemKind,
    /// What it may touch, in the chunk's transform space.
    pub visual_rect: RectF,
    pub op: PaintOp,
}

/// A run of display items painted under the same property tree state.
#[derive(Debug, Clone, PartialEq)]
pub struct PaintChunk {
    pub items: Range<usize>,
    pub state: PropertyTreeState,
}

/// What hit testing can find, in paint order, as paint recorded it (Blink's
/// `HitTestData` of the chunks): what the compositor's regions are made
/// from, without walking the tree. Kept apart from the display items, so
/// what paints nothing makes no empty chunk.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct HitTestData {
    pub chunks: Vec<HitTestChunk>,
    /// Each target's rect, in its chunk's transform space: a box's border
    /// box, a run of text, an inline box on a line, an SVG shape's or
    /// character's bounds, a child frame's canvas.
    pub rects: Vec<RectF>,
}

/// A run of targets under the same property tree state, in the same scroll
/// container.
#[derive(Debug, Clone, PartialEq)]
pub struct HitTestChunk {
    /// Its targets' rects, in [`HitTestData::rects`].
    pub rects: Range<usize>,
    pub state: PropertyTreeState,
    /// The innermost scroll container they are in, or their own.
    pub scroll: Option<ScrollNodeId>,
    /// The scroll container whose own border box (or child frame's canvas)
    /// this is, a chunk of its own (Blink's `ScrollHitTestDisplayItem`):
    /// the chunks after it are what is painted after it.
    pub container: Option<ScrollNodeId>,
    /// They are inside a blocking `wheel` listener (their style's
    /// `inside_blocking_wheel_event_handler`): the wheel event handler
    /// region.
    pub wheel: bool,
    /// Around its rects, in `state`'s transform space: what rules it out at
    /// once where it covers nothing.
    pub bounds: RectF,
}

/// Paint's output (`blink::PaintArtifact`).
#[derive(Debug, Clone, PartialEq)]
pub struct PaintArtifact {
    pub items: Vec<DisplayItem>,
    pub chunks: Vec<PaintChunk>,
    pub hit_test: HitTestData,
    /// Device pixels per CSS px: the grid the edges were snapped to.
    pub device_scale_factor: f32,
    pub id: PaintId,
    /// The items taken unchanged from the paint before, if any were.
    pub cached: Option<CachedItems>,
}

impl PaintArtifact {
    /// Whether item `index` is item `old` of `previous`, copied unchanged:
    /// no need to compare what they paint.
    pub fn is_cached_from(&self, index: usize, previous: &PaintArtifact, old: usize) -> bool {
        self.cached
            .as_ref()
            .is_some_and(|cached| cached.from == previous.id && cached.indices[index] == Some(old))
    }
}

impl Default for PaintArtifact {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            chunks: Vec::new(),
            hit_test: HitTestData::default(),
            device_scale_factor: 1.0,
            id: PaintId::next(),
            cached: None,
        }
    }
}

/// What pre-paint decided for one fragment (Blink's `FragmentData`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FragmentProperties {
    /// The border box's origin in its transform space.
    pub offset: PointF,
    /// For the box's own background and border
    /// (`LocalBorderBoxProperties`).
    pub local: PropertyTreeState,
    /// For its content: text and children (`ContentsProperties`); adds
    /// the overflow clip.
    pub contents: PropertyTreeState,
    /// The innermost scroll container the box is in, or the box itself if
    /// it is one; `None` if only the viewport scrolls it.
    pub scroll: Option<ScrollNodeId>,
}

/// Pre-paint's output: the trees, and each fragment's properties.
pub struct PrePaint {
    pub trees: PropertyTrees,
    /// How far the main frame's viewport is scrolled: a box fixed to it
    /// is painted at its layout position (the compositor keeps it
    /// there), so script's geometry and hit testing,
    /// which work in the document's coordinates, add this to it.
    pub viewport_scroll: PointF,
    /// Device pixels per CSS px: the grid clips, scroll offsets and what is
    /// painted snap to.
    pub device_scale_factor: f32,
    /// What the walk found of each fragment, by its address (the record
    /// holds the fragment, so the address isn't another's). Shared with a
    /// copy that differs in matrices only (`unscrolled`).
    records: Arc<rustc_hash::FxHashMap<*const BoxFragment, Walked>>,
    /// The child frames gone into, by their iframe.
    child_frames: NodeIdHashMap<EmbeddedFrame>,
    /// What each SVG element in an outermost `<svg>`'s content is painted
    /// under (its own transform and opacity), by its node's address in
    /// the fragment tree (a `<use>`'s copies are each drawn as the `<use>`,
    /// so its element doesn't tell them apart).
    pub(crate) svg_states: Arc<rustc_hash::FxHashMap<*const layout::SvgNode, PropertyTreeState>>,
    /// The effect each out-of-flow box is under, by its fragment's address:
    /// that of the box's parent's children, where its parent places it in
    /// tree order, set as the parent is walked (before the box, its
    /// containing block's child: its containing block's own children come
    /// first) and kept for walks of the box alone.
    out_of_flow_effects: Arc<rustc_hash::FxHashMap<*const BoxFragment, EffectNodeId>>,
    /// The ids its nodes have by key, for the next pre-paint to give the
    /// same.
    ids: NodeIds,
    /// The fragment tree walked, for the next pre-paint to compare with.
    root: Option<Arc<BoxFragment>>,
    /// Which pre-paint of the tree this is: what a record was last walked
    /// in.
    epoch: u64,
    /// Which line of pre-paints, each from the last, this is in: one from
    /// scratch starts another.
    lineage: PaintId,
    /// The iframes the walk had child frames of, walked or not.
    frame_iframes: rustc_hash::FxHashSet<NodeId>,
    /// While walking: whether a fragment walked before, as it was, may be
    /// left as it is (not when another iframe has a child frame, which a
    /// subtree left as it is wouldn't go into).
    reuse: bool,
    /// While walking: the last pre-paint's child frames not walked yet.
    previous_frames: NodeIdHashMap<EmbeddedFrame>,
    /// While walking: the nodes of the fragments being walked, each
    /// fragment's after its ancestors'.
    walking: Vec<NodePlace>,
    /// How many fragments the walk went to (for tests).
    walked: usize,
}

/// What pre-paint found of a fragment, and what it was found with: the same
/// fragment found with the same is left as it is, and all in it (Blink's
/// `PrePaintTreeWalk` skips what doesn't need a property update).
#[derive(Clone)]
struct Walked {
    /// `None` for a root walked without one held.
    fragment: Option<Arc<BoxFragment>>,
    inputs: WalkInputs,
    properties: FragmentProperties,
    /// Its own nodes (and its SVG content's, and its child frame's place's).
    nodes: Vec<NodePlace>,
    /// Whether an iframe in it (or it) has a child frame: walked again each
    /// time, the frame's layout being apart from the iframe's fragment.
    frames_below: bool,
    epoch: u64,
}

/// What a fragment's walk depends on but the fragment, and the scroll
/// offsets (whose nodes' matrices are set in place).
#[derive(Clone, Copy, PartialEq)]
struct WalkInputs {
    offset: PointF,
    parent: PropertyTreeState,
    /// For an out-of-flow box, the effect it is under in its parent.
    out_of_flow_effect: Option<EffectNodeId>,
    parent_scroll: Option<ScrollNodeId>,
    frame: Option<NodeId>,
    fixed: PropertyTreeState,
    fixed_scroll: Option<ScrollNodeId>,
}

/// A child frame as pre-paint placed it in its iframe.
#[derive(Clone)]
pub(crate) struct EmbeddedFrame {
    pub(crate) layout_view: Arc<BoxFragment>,
    /// What its document is painted under: the iframe's content box's
    /// clip, and its position less the viewport's scroll offset.
    pub(crate) state: PropertyTreeState,
    pub(crate) document_size: SizeF,
    /// Its viewport's scroll offset, as snapped for `state`.
    viewport_scroll: PointF,
    /// Its viewport's scroll node.
    pub(crate) scroll: ScrollNodeId,
}

impl EmbeddedFrame {
    /// Whether its root element is inside a blocking `wheel` listener (of
    /// its own, its document's or its window's): its canvas is too.
    pub(crate) fn root_in_blocking_wheel(&self) -> bool {
        self.layout_view
            .children
            .first()
            .filter(|link| link.fragment.role == BoxRole::RootElement)
            .is_some_and(|link| link.fragment.style.inside_blocking_wheel_event_handler)
    }
}

impl PrePaint {
    /// How many fragments this pre-paint walked: those of the last left as
    /// they were aren't. For tests.
    pub fn walked_for_testing(&self) -> usize {
        self.walked
    }

    /// Whether the fragment at `address` is in the tree pre-paint walked:
    /// it keeps a record of each fragment in it, and only of those.
    pub(crate) fn has_record(&self, address: *const BoxFragment) -> bool {
        self.records.contains_key(&address)
    }

    /// Which pre-paint this is, for [`Self::unchanged_since`].
    pub(crate) fn generation(&self) -> (PaintId, u64) {
        (self.lineage, self.epoch)
    }

    /// Whether `fragment` and all in it have the properties they had in
    /// pre-paint `generation` (of this line): it wasn't walked since, and
    /// no child frame is in it.
    pub(crate) fn unchanged_since(
        &self,
        fragment: &BoxFragment,
        generation: (PaintId, u64),
    ) -> bool {
        self.lineage == generation.0
            && self
                .records
                .get(&(fragment as *const BoxFragment))
                .is_some_and(|record| record.epoch <= generation.1 && !record.frames_below)
    }

    pub fn properties(&self, fragment: &BoxFragment) -> FragmentProperties {
        self.records[&(fragment as *const BoxFragment)].properties
    }

    /// The child frame `fragment`'s iframe shows, if pre-paint went into
    /// one.
    pub(crate) fn child_frame(&self, fragment: &BoxFragment) -> Option<&EmbeddedFrame> {
        self.child_frames.get(&fragment.node?)
    }

    /// `point`, in the root space, in the viewport of the child frame
    /// `iframe` shows (wherever it is in the frame tree): through the
    /// transforms around it, less where its viewport is (a `MouseEvent`'s
    /// `clientX` and `clientY` there). `None` if pre-paint didn't go into
    /// it, or a transform can't be undone.
    /// The layout view of the child frame `iframe` shows, as pre-paint
    /// went into it: where hit testing within that frame starts.
    pub fn child_frame_layout_view(&self, iframe: NodeId) -> Option<&BoxFragment> {
        Some(&self.child_frames.get(&iframe)?.layout_view)
    }

    /// `point` in the viewport of the child frame `iframe` shows, in the
    /// root space: [`Self::point_in_child_frame`] the other way.
    pub fn point_from_child_frame(&self, iframe: NodeId, point: PointF) -> Option<PointF> {
        let frame = self.child_frames.get(&iframe)?;
        let in_document = PointF::new(
            point.x + frame.viewport_scroll.x,
            point.y + frame.viewport_scroll.y,
        );
        Some(self.trees.to_root()[frame.state.transform.0].map_point(in_document))
    }

    pub fn point_in_child_frame(&self, iframe: NodeId, point: PointF) -> Option<PointF> {
        let frame = self.child_frames.get(&iframe)?;
        let in_document = self.trees.to_root()[frame.state.transform.0]
            .inverse()?
            .map_point(point);
        Some(PointF::new(
            in_document.x - frame.viewport_scroll.x,
            in_document.y - frame.viewport_scroll.y,
        ))
    }
}

/// What the walk carries from a frame into its child frames.
struct WalkContext<'a> {
    scroll: &'a ScrollOffsets,
    child_frames: &'a ChildFrames,
    /// The iframe whose child frame the walk is in; `None` in the main
    /// frame.
    frame: Option<NodeId>,
    /// Where a box fixed to this frame's viewport hangs: the state
    /// above the frame's scroll translation, so
    /// scrolling the viewport doesn't move it. In the main frame, whose
    /// viewport the compositor scrolls without a node of its own, it is
    /// the root with a transform node marked fixed instead.
    fixed: PropertyTreeState,
    /// The scroll node a fixed box is under: the one around this frame,
    /// not this frame's own viewport.
    fixed_scroll: Option<ScrollNodeId>,
}

/// `PrePaintTreeWalk` / `PaintPropertyTreeBuilder`. `scroll` gives the
/// scroll containers' offsets; `device_scale_factor`, the device pixels per
/// CSS px, which edges snap to.
pub fn pre_paint(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    device_scale_factor: f32,
) -> PrePaint {
    pre_paint_tree(
        layout_view,
        None,
        scroll,
        &StickyState::default(),
        &ChildFrames::default(),
        device_scale_factor,
        PointF::new(0.0, 0.0),
        None,
    )
}

/// [`pre_paint`] through a frame tree: at an iframe in `child_frames`, into
/// its document, placed in the iframe's content box (Blink's
/// `PrePaintTreeWalk` into a `LayoutEmbeddedContent`'s frame); `sticky`
/// says how far each sticky box in the frame tree sticks. With
/// `previous`, the last pre-paint of the same tree, a node keeps the id it
/// had while its owner has it (`NodeKey`): what was painted under it is the
/// same. A fragment of the last tree walked with the same inputs is left
/// as it is, with all in it; the scroll offsets are set in place.
pub fn pre_paint_frame_tree(
    layout_view: &Arc<BoxFragment>,
    scroll: &ScrollOffsets,
    sticky: &StickyState,
    child_frames: &ChildFrames,
    device_scale_factor: f32,
    viewport_scroll: PointF,
    previous: Option<PrePaint>,
) -> PrePaint {
    pre_paint_tree(
        layout_view,
        Some(layout_view),
        scroll,
        sticky,
        child_frames,
        device_scale_factor,
        viewport_scroll,
        previous,
    )
}

#[allow(clippy::too_many_arguments)]
fn pre_paint_tree(
    layout_view: &BoxFragment,
    root: Option<&Arc<BoxFragment>>,
    scroll: &ScrollOffsets,
    sticky: &StickyState,
    child_frames: &ChildFrames,
    device_scale_factor: f32,
    viewport_scroll: PointF,
    previous: Option<PrePaint>,
) -> PrePaint {
    let _span = trace_event::trace_event!("PrePaint");
    #[cfg(debug_assertions)]
    let check = previous.is_some();
    // Trees with more empty places than nodes start again (a node new
    // under one at the end can't take a place before it), as does a
    // pre-paint at another scale (every edge snaps elsewhere).
    let previous = previous.filter(|previous| {
        !previous.ids.mostly_free() && previous.device_scale_factor == device_scale_factor
    });
    let frame_iframes: rustc_hash::FxHashSet<NodeId> = child_frames.keys().copied().collect();
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "one epoch per pre-paint: a u64 lasts centuries"
    )]
    let mut pre = match previous {
        Some(previous) => PrePaint {
            viewport_scroll,
            epoch: previous.epoch + 1,
            reuse: previous.frame_iframes == frame_iframes,
            previous_frames: previous.child_frames,
            child_frames: NodeIdHashMap::default(),
            frame_iframes,
            walking: Vec::new(),
            walked: 0,
            ..previous
        },
        None => PrePaint {
            trees: PropertyTrees::root_only(),
            viewport_scroll,
            device_scale_factor,
            records: Arc::default(),
            child_frames: NodeIdHashMap::default(),
            svg_states: Arc::default(),
            out_of_flow_effects: Arc::default(),
            ids: NodeIds::root_only(),
            root: None,
            epoch: 0,
            lineage: PaintId::next(),
            frame_iframes,
            reuse: false,
            previous_frames: NodeIdHashMap::default(),
            walking: Vec::new(),
            walked: 0,
        },
    };
    let context = WalkContext {
        scroll,
        child_frames,
        frame: None,
        fixed: PropertyTreeState::ROOT,
        fixed_scroll: None,
    };
    let old_root = pre.root.take();
    pre.walk(
        layout_view,
        root,
        old_root.as_ref(),
        PointF::new(0.0, 0.0),
        PropertyTreeState::ROOT,
        None,
        &context,
    );
    pre.root = root.cloned();
    // The child frames whose iframes the walk didn't go into again.
    for (_, frame) in std::mem::take(&mut pre.previous_frames) {
        pre.drop_subtree(&frame.layout_view);
    }
    pre.empty_unused();
    pre.scroll_in_place(scroll, child_frames);
    pre.sticky_in_place(sticky);
    // What it gave is what a pre-paint from scratch gives, but for the ids,
    // and it holds the nodes and records that one does.
    #[cfg(debug_assertions)]
    if check {
        let fresh = pre_paint_tree(
            layout_view,
            root,
            scroll,
            sticky,
            child_frames,
            device_scale_factor,
            viewport_scroll,
            None,
        );
        pre.assert_resolves_as(&fresh);
        assert_eq!(
            pre.ids.transform.nodes(),
            fresh.trees.transform.len(),
            "transform nodes"
        );
        assert_eq!(pre.ids.clip.nodes(), fresh.trees.clip.len(), "clip nodes");
        assert_eq!(
            pre.ids.effect.nodes(),
            fresh.trees.effect.len(),
            "effect nodes"
        );
        assert_eq!(
            pre.ids.scroll.nodes(),
            fresh.trees.scroll.len(),
            "scroll nodes"
        );
    }
    pre
}

impl PrePaint {
    /// The transform node of `key` (or a new one without), under `parent`:
    /// at its last id, if it had one.
    fn transform_node(
        &mut self,
        key: Option<NodeKey>,
        parent: TransformNodeId,
        matrix: Transform,
        fixed: bool,
    ) -> TransformNodeId {
        let place = self.ids.transform.give(key, Some(parent.0));
        self.walking.push(NodePlace::Transform(place));
        let node = TransformNode {
            parent: Some(parent),
            matrix,
            fixed,
            sticky: None,
        };
        put(&mut self.trees.transform, place, node);
        TransformNodeId(place)
    }

    /// [`Self::transform_node`] for a clip node.
    fn clip_node(
        &mut self,
        key: Option<NodeKey>,
        parent: ClipNodeId,
        transform: TransformNodeId,
        rrect: RRectF,
    ) -> ClipNodeId {
        self.path_clip_node(key, parent, transform, rrect, None)
    }

    /// [`Self::clip_node`] that clips to `paths` too (their union), in
    /// `rrect`.
    fn path_clip_node(
        &mut self,
        key: Option<NodeKey>,
        parent: ClipNodeId,
        transform: TransformNodeId,
        rrect: RRectF,
        paths: Option<Arc<[Path]>>,
    ) -> ClipNodeId {
        let place = self.ids.clip.give(key, Some(parent.0));
        self.walking.push(NodePlace::Clip(place));
        let node = ClipNode {
            parent: Some(parent),
            transform,
            rect: Some(rrect.rect),
            radii: rrect.radii,
            paths,
        };
        put(&mut self.trees.clip, place, node);
        ClipNodeId(place)
    }

    /// A clip node to the union of `paths` (none: everything is cut
    /// away) moved by `at`, in their bounds.
    fn clip_paths_node(
        &mut self,
        key: Option<NodeKey>,
        parent: ClipNodeId,
        transform: TransformNodeId,
        paths: &[Path],
        at: Transform,
    ) -> ClipNodeId {
        let paths: Arc<[Path]> = paths
            .iter()
            .map(|path| {
                let mut path = path.clone();
                path.transform(&at);
                path
            })
            .collect();
        let bounds = paths
            .iter()
            .filter_map(Path::bounds)
            .reduce(|a, b| a.union(&b))
            .unwrap_or_else(|| at.map_rect(RectF::new(0.0, 0.0, 0.0, 0.0)));
        self.path_clip_node(
            key,
            parent,
            transform,
            RRectF::from_rect(bounds),
            Some(paths),
        )
    }

    /// [`Self::transform_node`] for an effect node: `opacity` and
    /// `filter` on what `state` holds, the filter in its transform's
    /// space, what it makes cut by its clip.
    fn effect_node(
        &mut self,
        key: Option<NodeKey>,
        state: PropertyTreeState,
        opacity: f32,
        filter: Option<compositor_paint::ImageFilter>,
    ) -> EffectNodeId {
        let parent = state.effect;
        let place = self.ids.effect.give(key, Some(parent.0));
        self.walking.push(NodePlace::Effect(place));
        let node = EffectNode {
            parent: Some(parent),
            opacity,
            filter,
            transform: state.transform,
            output_clip: state.clip,
        };
        put(&mut self.trees.effect, place, node);
        EffectNodeId(place)
    }

    /// [`Self::transform_node`] for a scroll node.
    fn scroll_node(&mut self, key: Option<NodeKey>, node: ScrollNode) -> ScrollNodeId {
        let place = self
            .ids
            .scroll
            .give(key, node.parent.map(|parent| parent.0));
        self.walking.push(NodePlace::Scroll(place));
        put(&mut self.trees.scroll, place, Some(node));
        ScrollNodeId(place)
    }

    /// Lets go of what fragment `address` had, unless walked in this
    /// pre-paint: its record, its nodes (for its owner's keys to take again,
    /// or the end to free) and its SVG content's states. Whether it had.
    fn release(&mut self, address: *const BoxFragment) -> bool {
        if self
            .records
            .get(&address)
            .is_none_or(|record| record.epoch == self.epoch)
        {
            return false;
        }
        let record = Arc::make_mut(&mut self.records)
            .remove(&address)
            .expect("just found");
        if self.out_of_flow_effects.contains_key(&address) {
            Arc::make_mut(&mut self.out_of_flow_effects).remove(&address);
        }
        for &place in &record.nodes {
            self.ids.release(place);
        }
        if let Some(svg) = record.fragment.as_ref().and_then(|f| f.svg.as_ref()) {
            fn forget(
                states: &mut rustc_hash::FxHashMap<*const layout::SvgNode, PropertyTreeState>,
                nodes: &[layout::SvgNode],
            ) {
                for node in nodes {
                    states.remove(&(node as *const layout::SvgNode));
                    if let layout::SvgNodeKind::Container(children) = &node.kind {
                        forget(states, children);
                    }
                }
            }
            forget(Arc::make_mut(&mut self.svg_states), &svg.children);
        }
        true
    }

    /// Lets go of what `old` (a fragment gone from the tree) had, and all
    /// in it, its child frame's too: those walked in this pre-paint (gone
    /// elsewhere in the tree) stay.
    fn drop_subtree(&mut self, old: &Arc<BoxFragment>) {
        if !self.release(Arc::as_ptr(old)) {
            return;
        }
        for child in &old.children {
            self.drop_subtree(&child.fragment);
        }
        if let Some(frame) = old.node.and_then(|node| self.previous_frames.remove(&node)) {
            self.drop_subtree(&frame.layout_view);
        }
    }

    /// Sets each scroll container's scroll translation to its offset in
    /// `scroll` (one in a child frame, to its offset in its frame's),
    /// snapped to device pixels so the content stays crisp: scrolling
    /// changes no other node (Blink updates only the `ScrollTranslation`).
    /// A child frame's viewport's is set as its iframe is walked, which it
    /// is each time.
    fn scroll_in_place(&mut self, scroll: &ScrollOffsets, child_frames: &ChildFrames) {
        let dsf = self.device_scale_factor;
        let snapped = |at: PointF| PointF::new(snap_to(at.x, dsf), snap_to(at.y, dsf));
        let viewports: rustc_hash::FxHashSet<ScrollNodeId> = self
            .child_frames
            .values()
            .map(|frame| frame.scroll)
            .collect();
        for index in 0..self.trees.scroll.len() {
            let Some(node) = &self.trees.scroll[index] else {
                continue;
            };
            if viewports.contains(&ScrollNodeId(index)) {
                continue;
            }
            let offsets = match node.frame {
                None => scroll,
                Some(iframe) => &child_frames[&iframe].scroll,
            };
            let at = snapped(
                offsets
                    .get(&node.element)
                    .copied()
                    .unwrap_or(PointF::new(0.0, 0.0)),
            );
            let translation = node.scroll_translation;
            self.trees.transform[translation.0].matrix = Transform::translate(-at.x, -at.y);
        }
    }

    /// Each sticky box's translation set to how far `sticky` says it
    /// sticks, snapped as a scroll offset is, and how far it may: as its
    /// scrollport scrolls, the box isn't walked again.
    fn sticky_in_place(&mut self, sticky: &StickyState) {
        if sticky.offsets.is_empty() {
            return;
        }
        let dsf = self.device_scale_factor;
        for node in &mut self.trees.transform {
            if let Some(StickyTranslation { key, at, reach, .. }) = &mut node.sticky {
                *reach = sticky.reaches.get(key).copied().unwrap_or_default();
                let (key, at) = (*key, *at);
                let by = sticky.offsets.get(&key).copied().unwrap_or_default();
                let snapped = |at: f32, by: f32| snap_to(at + by, dsf) - snap_to(at, dsf);
                node.matrix = Transform::translate(snapped(at.x, by.x), snapped(at.y, by.y));
            }
        }
    }

    /// The walk is over: the places of the nodes let go of and not taken
    /// again (their owners are gone) hold nothing now, for the next
    /// pre-paint to give.
    fn empty_unused(&mut self) {
        for place in self.ids.transform.end() {
            self.trees.transform[place] = TransformNode::EMPTY;
        }
        for place in self.ids.clip.end() {
            self.trees.clip[place] = ClipNode::empty();
        }
        for place in self.ids.effect.end() {
            self.trees.effect[place] = EffectNode::EMPTY;
        }
        for place in self.ids.scroll.end() {
            self.trees.scroll[place] = None;
        }
    }

    /// The same pre-paint with the containers whose elements are in
    /// `composited` (and the child frames whose documents are) unscrolled,
    /// and the sticky boxes the compositor moves unmoved:
    /// what paint paints them with, as the compositor moves them by their
    /// offsets. Their nodes are the same; only their scroll translations
    /// are other matrices.
    pub fn unscrolled(&self, composited: &NodeIdHashSet) -> PrePaint {
        let mut trees = self.trees.clone();
        for (_, node) in self.trees.scroll_nodes() {
            if composited.contains(&node.element) {
                trees.transform[node.scroll_translation.0].matrix = Transform::IDENTITY;
            }
        }
        // The sticky boxes the compositor moves as their scrollports
        // scroll: painted where they were laid out.
        let layers: Vec<bool> = self
            .trees
            .scroll
            .iter()
            .map(|node| {
                node.as_ref()
                    .is_some_and(|n| composited.contains(&n.element))
            })
            .collect();
        for (index, sticky) in composited_sticky(&self.trees, &layers)
            .into_iter()
            .enumerate()
        {
            if sticky {
                trees.transform[index].matrix = Transform::IDENTITY;
            }
        }
        let child_frames = self
            .child_frames
            .iter()
            .map(|(&iframe, frame)| {
                let mut frame = frame.clone();
                if composited.contains(&self.trees.scroll(frame.scroll).element) {
                    frame.viewport_scroll = PointF::new(0.0, 0.0);
                }
                (iframe, frame)
            })
            .collect();
        PrePaint {
            trees,
            viewport_scroll: self.viewport_scroll,
            device_scale_factor: self.device_scale_factor,
            records: self.records.clone(),
            child_frames,
            svg_states: self.svg_states.clone(),
            out_of_flow_effects: self.out_of_flow_effects.clone(),
            ids: NodeIds::root_only(),
            root: None,
            epoch: self.epoch,
            lineage: self.lineage,
            frame_iframes: self.frame_iframes.clone(),
            reuse: false,
            previous_frames: NodeIdHashMap::default(),
            walking: Vec::new(),
            walked: 0,
        }
    }
}

/// What a property tree state comes to, ids aside: its transform to the
/// root space, its clips (each rect, its corners and its space's transform
/// to the root), outermost first, and its effects, outermost first: each
/// its opacity, its filter, the filter's space's transform to the root
/// and how many clips its output clip is in.
#[cfg(debug_assertions)]
type Resolved = (
    Transform,
    Vec<(Option<RRectF>, Transform)>,
    Vec<(f32, Option<compositor_paint::ImageFilter>, Transform, usize)>,
);

#[cfg(debug_assertions)]
impl PrePaint {
    fn resolve(&self, to_root: &[Transform], state: PropertyTreeState) -> Resolved {
        (
            to_root[state.transform.0],
            self.trees
                .clip_chain(state.clip)
                .into_iter()
                .map(|clip| {
                    let node = &self.trees.clip[clip.0];
                    (node.rrect(), to_root[node.transform.0])
                })
                .collect(),
            self.trees
                .effect_chain(state.effect)
                .into_iter()
                .map(|effect| {
                    let node = &self.trees.effect[effect.0];
                    (
                        node.opacity,
                        node.filter.clone(),
                        to_root[node.transform.0],
                        self.trees.clip_chain(node.output_clip).len(),
                    )
                })
                .collect(),
        )
    }

    /// What scroll node `id` comes to, ids aside.
    fn resolve_scroll(
        &self,
        to_root: &[Transform],
        id: ScrollNodeId,
    ) -> (ScrollNode, Resolved, Resolved, Option<NodeId>) {
        let node = self.trees.scroll(id);
        let outside = PropertyTreeState {
            transform: node.transform,
            clip: node.clip,
            effect: node.effect,
        };
        let inside = PropertyTreeState {
            transform: node.scroll_translation,
            clip: node.overflow_clip,
            effect: node.effect,
        };
        let data = ScrollNode {
            parent: None,
            transform: TransformNodeId(0),
            clip: ClipNodeId(0),
            overflow_clip: ClipNodeId(0),
            effect: EffectNodeId(0),
            scroll_translation: TransformNodeId(0),
            ..node.clone()
        };
        (
            data,
            self.resolve(to_root, outside),
            self.resolve(to_root, inside),
            node.parent.map(|parent| self.trees.scroll(parent).element),
        )
    }

    /// Panics unless this pre-paint comes to what `other` (of the same
    /// tree) does, but for the ids it gave its nodes: each fragment's
    /// properties, each SVG element's state, each child frame's place,
    /// and the scroll nodes. For debug builds' checks.
    pub fn assert_resolves_as(&self, other: &PrePaint) {
        let (to_root, other_to_root) = (self.trees.to_root(), other.trees.to_root());
        assert_eq!(self.records.len(), other.records.len(), "fragments");
        assert_eq!(
            self.out_of_flow_effects.len(),
            other.out_of_flow_effects.len(),
            "out-of-flow boxes"
        );
        for (fragment, record) in self.records.iter() {
            let properties = &record.properties;
            let theirs = other.records[fragment].properties;
            assert_eq!(properties.offset, theirs.offset);
            assert_eq!(
                self.resolve(&to_root, properties.local),
                other.resolve(&other_to_root, theirs.local)
            );
            assert_eq!(
                self.resolve(&to_root, properties.contents),
                other.resolve(&other_to_root, theirs.contents)
            );
            assert_eq!(
                properties.scroll.map(|s| self.resolve_scroll(&to_root, s)),
                theirs
                    .scroll
                    .map(|s| other.resolve_scroll(&other_to_root, s))
            );
        }
        assert_eq!(
            self.svg_states.len(),
            other.svg_states.len(),
            "SVG elements"
        );
        for (node, state) in self.svg_states.iter() {
            assert_eq!(
                self.resolve(&to_root, *state),
                other.resolve(&other_to_root, other.svg_states[node])
            );
        }
        assert_eq!(
            self.child_frames.len(),
            other.child_frames.len(),
            "child frames"
        );
        for (iframe, frame) in &self.child_frames {
            let theirs = &other.child_frames[iframe];
            assert_eq!(frame.viewport_scroll, theirs.viewport_scroll);
            assert_eq!(frame.document_size, theirs.document_size);
            assert_eq!(
                self.resolve(&to_root, frame.state),
                other.resolve(&other_to_root, theirs.state)
            );
            assert_eq!(
                self.resolve_scroll(&to_root, frame.scroll),
                other.resolve_scroll(&other_to_root, theirs.scroll)
            );
        }
        let scrolls = |pre: &PrePaint, to_root: &[Transform]| {
            pre.trees
                .scroll_nodes()
                .map(|(id, _)| format!("{:?}", pre.resolve_scroll(to_root, id)))
                .collect::<std::collections::BTreeSet<_>>()
        };
        assert_eq!(
            scrolls(self, &to_root),
            scrolls(other, &other_to_root),
            "scroll nodes"
        );
    }
}

/// Puts `node` at `place` in `nodes`, at the end if it is the length.
fn put<T>(nodes: &mut Vec<T>, place: usize, node: T) {
    if place == nodes.len() {
        nodes.push(node);
    } else {
        nodes[place] = node;
    }
}

impl PrePaint {
    /// `fragment` (held by `arc`, unless the root is walked without one),
    /// at `offset` in `parent`'s space, in place of `old`, the fragment of
    /// the last pre-paint it is (the same one, or its box's last one).
    /// Returns whether an iframe in it has a child frame.
    #[allow(clippy::too_many_arguments)]
    fn walk(
        &mut self,
        fragment: &BoxFragment,
        arc: Option<&Arc<BoxFragment>>,
        old: Option<&Arc<BoxFragment>>,
        offset: PointF,
        parent: PropertyTreeState,
        parent_scroll: Option<ScrollNodeId>,
        context: &WalkContext<'_>,
    ) -> bool {
        let address = fragment as *const BoxFragment;
        // An out-of-flow box is under the effects of the elements around
        // it where its parent places it in tree order, not its containing
        // block's: its transform and clips are its containing block's
        // (Blink's `PaintPropertyTreeBuilder` takes them from its
        // `ContainingBlockContext`, its effect from its parent).
        let out_of_flow_effect = fragment
            .style
            .is_out_of_flow()
            .then(|| self.out_of_flow_effects.get(&address).copied())
            .flatten();
        let inputs = WalkInputs {
            offset,
            parent,
            out_of_flow_effect,
            parent_scroll,
            frame: context.frame,
            fixed: context.fixed,
            fixed_scroll: context.fixed_scroll,
        };
        // The same fragment, found with the same: as it was, and all in it.
        if self.reuse
            && old.is_some_and(|old| std::ptr::eq(Arc::as_ptr(old), address))
            && self
                .records
                .get(&address)
                .is_some_and(|record| record.inputs == inputs && !record.frames_below)
        {
            return false;
        }
        if let Some(old) = old {
            self.release(Arc::as_ptr(old));
        }
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per fragment walked: fewer than usize::MAX"
        )]
        {
            self.walked += 1;
        }
        let start = self.walking.len();
        let style = &fragment.style;
        // Its nodes' keys: an anonymous box's have none (new ids each
        // pre-paint).
        let owner = fragment.node.map(|node| NodeOwner::Box {
            frame: context.frame,
            node,
            pseudo: fragment.pseudo,
        });
        let key = |role| owner.map(|owner| NodeKey { owner, role });
        // Fixed to its frame's viewport: it hangs where the frame's
        // content hangs before that viewport's scroll, and its offset
        // is from the viewport's origin, as
        // layout laid it out against the `LayoutView`.
        let (parent, parent_scroll, offset) = if fragment.fixed_to_viewport {
            let mut state = context.fixed;
            if context.frame.is_none() {
                // The main frame: the compositor scrolls its viewport with
                // no node here, so the layer carries the word instead.
                state.transform = self.transform_node(
                    key(NodeRole::Fixed),
                    state.transform,
                    Transform::IDENTITY,
                    true,
                );
            }
            (state, context.fixed_scroll, offset)
        } else {
            (parent, parent_scroll, offset)
        };
        let parent = PropertyTreeState {
            effect: out_of_flow_effect.unwrap_or(parent.effect),
            ..parent
        };
        // A sticky box moves as its scrollport scrolls, and all in it: a
        // translation node, its matrix set in place after the walk as a
        // scroll translation's is (Blink's sticky translation node).
        let parent = match fragment.node {
            Some(node) if layout::sticks(fragment) => {
                let transform = self.transform_node(
                    key(NodeRole::StickyTranslation),
                    parent.transform,
                    Transform::IDENTITY,
                    false,
                );
                // In a fixed box: its frame's viewport doesn't move it. A
                // child frame's viewport is a scroll node; the main frame's
                // has none.
                let scroll = match parent_scroll {
                    None if context.frame.is_some() || self.trees.is_fixed(parent.transform) => {
                        StickyScroll::Fixed
                    }
                    None => StickyScroll::Viewport,
                    Some(s) if self.trees.scroll(s).frame != context.frame => StickyScroll::Fixed,
                    Some(s) => StickyScroll::Scroller(s),
                };
                self.trees.transform[transform.0].sticky = Some(StickyTranslation {
                    key: (node, fragment.pseudo),
                    at: offset,
                    scroll,
                    effect: parent.effect,
                    clip: parent.clip,
                    reach: RectF::default(),
                });
                PropertyTreeState {
                    transform,
                    ..parent
                }
            }
            _ => parent,
        };
        let mut local = parent;
        let mut offset = offset;
        if let Some(transform) = layout::box_transform(fragment) {
            // Into the box's own space, whose origin is its border box's:
            // the box's position, then its transform (about its center).
            let matrix = Transform::translate(offset.x, offset.y).then(&transform);
            local.transform =
                self.transform_node(key(NodeRole::Transform), parent.transform, matrix, false);
            offset = PointF::new(0.0, 0.0);
        }
        // `clip` and `clip-path` cut the box and all in it: a rect, and
        // a path in its bounds. (A fixed box inside that no box contains
        // isn't in it here but under the viewport, so they don't cut it;
        // Blink's `CssClipFixedPosition` does.)
        if let Some(rect) = clip_path::css_clip(style, offset, fragment.size) {
            local.clip = self.clip_node(
                key(NodeRole::CssClip),
                local.clip,
                local.transform,
                RRectF::from_rect(rect),
            );
        }
        if let Some(clip_path) = &fragment.clip_path {
            let at = Transform::translate(offset.x, offset.y);
            for (i, paths) in clip_path.iter().enumerate() {
                local.clip = self.clip_paths_node(
                    key(NodeRole::ClipPath(i)),
                    local.clip,
                    local.transform,
                    paths,
                    at,
                );
            }
        }
        // Its opacity and filter on it and all in it, as a group: what the
        // filter makes reaches past what it filtered, and is cut by those
        // clips, after them (Blink's effect node's `OutputClip`).
        let filter = filter::image_filter(style);
        if style.opacity < 1.0 || filter.is_some() {
            let state = PropertyTreeState {
                effect: parent.effect,
                ..local
            };
            local.effect = self.effect_node(key(NodeRole::Effect), state, style.opacity, filter);
        }
        // Hidden by layout (a collapsed table row or column's cell): it and
        // what is in it are clipped away, neither painted nor hit (Blink
        // doesn't paint a fragment hidden for paint).
        if fragment.hidden {
            local.clip = self.clip_node(
                key(NodeRole::Hidden),
                local.clip,
                local.transform,
                RRectF::from_rect(RectF::new(offset.x, offset.y, 0.0, 0.0)),
            );
        }
        let mut contents = local;
        let mut scroll_node = parent_scroll;
        // An `<svg>` clips its content to its content box, its viewport,
        // and doesn't scroll (Chrome's `LayoutSVGRoot`).
        if layout::clips_content(style) && fragment.svg.is_some() {
            let (b, p) = (&fragment.border, &fragment.padding);
            let content_box = RectF::new(
                offset.x + b.left + p.left,
                offset.y + b.top + p.top,
                fragment.size.width - b.left - b.right - p.left - p.right,
                fragment.size.height - b.top - b.bottom - p.top - p.bottom,
            );
            contents.clip = self.clip_node(
                key(NodeRole::SvgViewport),
                local.clip,
                local.transform,
                RRectF::from_rect(one_axis(snap(content_box, self.device_scale_factor), style)),
            );
        }
        let replaced = fragment.object_rect.is_some()
            || fragment.svg.is_some()
            || fragment
                .node
                .is_some_and(|n| context.child_frames.contains_key(&n));
        // A replaced element's `overflow: clip` (the UA stylesheet's) cuts
        // its content to its content box, below: Chrome's UA
        // `overflow-clip-margin: content-box`, which machopo has no property
        // for.
        let replaced_clip =
            replaced && layout::clips_content(style) && !style.is_scroll_container();
        // The element whose `overflow` went to the viewport (which the
        // compositor scrolls) has `visible` here: layout's used value.
        if layout::clips_content(style) && fragment.svg.is_none() && !replaced_clip {
            let b = fragment.border;
            let padding_box = RectF::new(
                offset.x + b.left,
                offset.y + b.top,
                fragment.size.width - b.left - b.right,
                fragment.size.height - b.top - b.bottom,
            );
            // Snapped like the backgrounds, so a clip edge never cuts a
            // pixel in half (Blink's `PixelSnappedIntRect` for the clip);
            // rounded by `border-radius` inside the border.
            let dsf = self.device_scale_factor;
            let mut clip = border_rrect(fragment, offset, dsf).inset(
                snap_border_width(b.top, dsf),
                snap_border_width(b.right, dsf),
                snap_border_width(b.bottom, dsf),
                snap_border_width(b.left, dsf),
            );
            // `clip` on one axis and `visible` on the other cuts only the
            // first, without the corners' curve; paint containment cuts
            // both.
            if !layout::paint_contained(style)
                && (style.overflow_x == Overflow::Visible || style.overflow_y == Overflow::Visible)
            {
                clip = RRectF::from_rect(one_axis(clip.rect, style));
            }
            contents.clip = self.clip_node(
                key(NodeRole::OverflowClip),
                local.clip,
                local.transform,
                clip,
            );
            // A scroll container: its content moves by its offset, which
            // is snapped to device pixels so the content stays crisp.
            if style.is_scroll_container()
                && let Some(element) = fragment.node
            {
                let at = context
                    .scroll
                    .get(&element)
                    .copied()
                    .unwrap_or(PointF::new(0.0, 0.0));
                let translation = Transform::translate(-snap_to(at.x, dsf), -snap_to(at.y, dsf));
                contents.transform = self.transform_node(
                    key(NodeRole::ScrollTranslation),
                    local.transform,
                    translation,
                    false,
                );
                scroll_node = Some(self.scroll_node(
                    key(NodeRole::Scroll),
                    ScrollNode {
                        frame: context.frame,
                        parent: parent_scroll,
                        element,
                        container: padding_box,
                        border_box: RectF::new(
                            offset.x,
                            offset.y,
                            fragment.size.width,
                            fragment.size.height,
                        ),
                        transform: local.transform,
                        clip: local.clip,
                        overflow_clip: contents.clip,
                        effect: local.effect,
                        scroll_translation: contents.transform,
                        contents: layout::scrollable_overflow(fragment),
                        user_scrollable_x: style.overflow_x.user_scrollable(),
                        user_scrollable_y: style.overflow_y.user_scrollable(),
                    },
                ));
            }
        }
        // A replaced element's content (an image, a child frame) is cut to
        // its content box's curve (Blink's `InnerBorderRadiusClip`), and to
        // its content box with `overflow: clip` (where `object-fit` puts it
        // past it; Blink's `ReplacedContentClip`).
        // Only where it cuts something: a curve, or content past the box.
        let (b, p) = (&fragment.border, &fragment.padding);
        let content_box = RectF::new(
            b.left + p.left,
            b.top + p.top,
            fragment.size.width - b.left - p.left - b.right - p.right,
            fragment.size.height - b.top - p.top - b.bottom - p.bottom,
        );
        let overflows = fragment
            .object_rect
            .is_some_and(|object| object.union(&content_box) != content_box);
        if replaced
            && ((replaced_clip && (overflows || style.has_border_radius()))
                || (style.has_border_radius() && !layout::clips_content(style)))
        {
            let dsf = self.device_scale_factor;
            let (b, p) = (&fragment.border, &fragment.padding);
            let content = border_rrect(fragment, offset, dsf)
                .inset(
                    snap_border_width(b.top, dsf),
                    snap_border_width(b.right, dsf),
                    snap_border_width(b.bottom, dsf),
                    snap_border_width(b.left, dsf),
                )
                .inset(p.top, p.right, p.bottom, p.left);
            let content = RRectF {
                rect: snap(content.rect, dsf),
                radii: content.radii,
            };
            contents.clip = self.clip_node(
                key(NodeRole::InnerClip),
                contents.clip,
                contents.transform,
                content,
            );
        }
        let properties = FragmentProperties {
            offset,
            local,
            contents,
            scroll: scroll_node,
        };
        self.walk_svg(fragment, offset, contents, context.frame);
        let mut frames_below =
            self.walk_children(fragment, old, offset, contents, scroll_node, context);
        if let Some(frame) = fragment.node.and_then(|n| context.child_frames.get(&n)) {
            self.walk_child_frame(fragment, offset, contents, scroll_node, context, frame);
            frames_below = true;
        }
        let nodes = self.walking.drain(start..).collect();
        // Let go of with its last record (when it was walked again too).
        if let Some(effect) = out_of_flow_effect {
            Arc::make_mut(&mut self.out_of_flow_effects).insert(address, effect);
        }
        Arc::make_mut(&mut self.records).insert(
            address,
            Walked {
                fragment: arc.cloned(),
                inputs,
                properties,
                nodes,
                frames_below,
                epoch: self.epoch,
            },
        );
        frames_below
    }

    /// `fragment`'s children, each in place of its old one among `old`'s:
    /// the same fragment, else its box's last one. Those of `old`'s none is
    /// are gone, with all in them: let go of first, for their nodes' owners
    /// in the new children to take again. Returns whether an iframe in
    /// them has a child frame.
    fn walk_children(
        &mut self,
        fragment: &BoxFragment,
        old: Option<&Arc<BoxFragment>>,
        offset: PointF,
        contents: PropertyTreeState,
        scroll_node: Option<ScrollNodeId>,
        context: &WalkContext<'_>,
    ) -> bool {
        if !fragment.out_of_flow.is_empty() {
            let effects = Arc::make_mut(&mut self.out_of_flow_effects);
            for place in &fragment.out_of_flow {
                effects.insert(Arc::as_ptr(&place.fragment), contents.effect);
            }
        }
        let old_children: &[layout::FragmentLink] = old.map_or(&[], |old| &old.children);
        let mut matched: Vec<Option<usize>> = vec![None; fragment.children.len()];
        if old.is_some_and(|old| std::ptr::eq(Arc::as_ptr(old), fragment)) {
            // The same fragment: the same children.
            for (i, matched) in matched.iter_mut().enumerate() {
                *matched = Some(i);
            }
        } else if !old_children.is_empty() {
            let mut taken = vec![false; old_children.len()];
            let mut by_address: Option<rustc_hash::FxHashMap<*const BoxFragment, usize>> = None;
            for (i, child) in fragment.children.iter().enumerate() {
                let address = Arc::as_ptr(&child.fragment);
                let j = if old_children
                    .get(i)
                    .is_some_and(|o| Arc::as_ptr(&o.fragment) == address)
                {
                    Some(i)
                } else {
                    by_address
                        .get_or_insert_with(|| {
                            old_children
                                .iter()
                                .enumerate()
                                .map(|(j, o)| (Arc::as_ptr(&o.fragment), j))
                                .collect()
                        })
                        .get(&address)
                        .copied()
                };
                if let Some(j) = j
                    && !taken[j]
                {
                    taken[j] = true;
                    matched[i] = Some(j);
                }
            }
            // The others, by their boxes.
            let owner = |f: &BoxFragment| f.node.map(|node| (node, f.pseudo));
            if matched
                .iter()
                .zip(&fragment.children)
                .any(|(m, child)| m.is_none() && child.fragment.node.is_some())
            {
                let mut by_owner: rustc_hash::FxHashMap<
                    (NodeId, Option<computed_style::PseudoId>),
                    usize,
                > = rustc_hash::FxHashMap::default();
                for (j, o) in old_children.iter().enumerate() {
                    if !taken[j]
                        && let Some(owner) = owner(&o.fragment)
                    {
                        by_owner.entry(owner).or_insert(j);
                    }
                }
                for (i, child) in fragment.children.iter().enumerate() {
                    if matched[i].is_none()
                        && let Some(&j) = owner(&child.fragment).and_then(|o| by_owner.get(&o))
                        && !taken[j]
                    {
                        taken[j] = true;
                        matched[i] = Some(j);
                    }
                }
            }
            for (j, o) in old_children.iter().enumerate() {
                if !taken[j] {
                    self.drop_subtree(&o.fragment);
                }
            }
        }
        let mut frames_below = false;
        for (i, child) in fragment.children.iter().enumerate() {
            let at = PointF::new(offset.x + child.offset.x, offset.y + child.offset.y);
            let old = matched[i].map(|j| &old_children[j].fragment);
            frames_below |= self.walk(
                &child.fragment,
                Some(&child.fragment),
                old,
                at,
                contents,
                scroll_node,
                context,
            );
        }
        frames_below
    }

    /// Into the child frame an iframe shows: its document clipped to the
    /// iframe's content box and placed there (Blink's
    /// `ReplacedContentTransform`), less its viewport's scroll offset (the
    /// frame's scroll translation, apart: what the compositor moves when
    /// it scrolls the viewport as a layer); both snapped to device pixels,
    /// as a scroll container's content is. The viewport is a scroll
    /// container in the iframe, its element the document.
    fn walk_child_frame(
        &mut self,
        iframe: &BoxFragment,
        offset: PointF,
        contents: PropertyTreeState,
        scroll_node: Option<ScrollNodeId>,
        context: &WalkContext<'_>,
        frame: &ChildFrame,
    ) {
        let dsf = self.device_scale_factor;
        let (b, p) = (&iframe.border, &iframe.padding);
        let content_box = RectF::new(
            offset.x + b.left + p.left,
            offset.y + b.top + p.top,
            iframe.size.width - b.left - p.left - b.right - p.right,
            iframe.size.height - b.top - p.top - b.bottom - p.bottom,
        );
        let iframe_node = iframe.node.expect("an iframe's box");
        let key = |role| {
            Some(NodeKey {
                owner: NodeOwner::Box {
                    frame: context.frame,
                    node: iframe_node,
                    pseudo: None,
                },
                role,
            })
        };
        let mut state = contents;
        state.clip = self.clip_node(
            key(NodeRole::FrameClip),
            contents.clip,
            contents.transform,
            RRectF::from_rect(snap(content_box, dsf)),
        );
        let origin = self.transform_node(
            key(NodeRole::FrameOrigin),
            contents.transform,
            Transform::translate(snap_to(content_box.x, dsf), snap_to(content_box.y, dsf)),
            false,
        );
        let viewport_scroll = PointF::new(
            snap_to(frame.viewport_scroll.x, dsf),
            snap_to(frame.viewport_scroll.y, dsf),
        );
        state.transform = self.transform_node(
            key(NodeRole::FrameScrollTranslation),
            origin,
            Transform::translate(-viewport_scroll.x, -viewport_scroll.y),
            false,
        );
        let port = RectF::new(0.0, 0.0, content_box.width, content_box.height);
        let viewport = self.scroll_node(
            key(NodeRole::FrameScroll),
            ScrollNode {
                frame: Some(iframe_node),
                parent: scroll_node,
                element: frame.document,
                container: port,
                border_box: port,
                transform: origin,
                clip: contents.clip,
                overflow_clip: state.clip,
                effect: contents.effect,
                scroll_translation: state.transform,
                contents: frame.document_size,
                user_scrollable_x: frame.user_scrollable_x,
                user_scrollable_y: frame.user_scrollable_y,
            },
        );
        self.child_frames.insert(
            iframe_node,
            EmbeddedFrame {
                layout_view: frame.layout_view.clone(),
                state,
                document_size: frame.document_size,
                viewport_scroll,
                scroll: viewport,
            },
        );
        let context = WalkContext {
            scroll: &frame.scroll,
            child_frames: context.child_frames,
            frame: Some(iframe_node),
            // A box fixed to this frame's viewport hangs above its scroll
            // translation, at the iframe's content box.
            fixed: PropertyTreeState {
                transform: origin,
                clip: state.clip,
                effect: state.effect,
            },
            fixed_scroll: scroll_node,
        };
        let old = self
            .previous_frames
            .remove(&iframe_node)
            .map(|frame| frame.layout_view);
        self.walk(
            &frame.layout_view,
            Some(&frame.layout_view),
            old.as_ref(),
            PointF::new(0.0, 0.0),
            state,
            Some(viewport),
            &context,
        );
    }
}

/// Paints the fragment tree. `document_size` is the canvas the document
/// background covers; `images`, where the background images are.
/// Paints everything; [`PaintController`] takes what didn't change from
/// the last paint.
pub fn paint(
    layout_view: &Arc<BoxFragment>,
    pre: &PrePaint,
    document_size: SizeF,
    images: &dyn BackgroundImages,
) -> PaintArtifact {
    paint_with(
        layout_view,
        pre,
        document_size,
        images,
        None,
        &mut PaintCache::default(),
    )
    .0
}

/// [`paint`], taking the items of the boxes and stacking contexts that
/// paint as they did from `previous`, and keeping in `cache` what each
/// painted, for the next.
fn paint_with(
    layout_view: &Arc<BoxFragment>,
    pre: &PrePaint,
    document_size: SizeF,
    images: &dyn BackgroundImages,
    previous: Option<&Previous>,
    cache: &mut PaintCache,
) -> (PaintArtifact, (usize, usize)) {
    let _span = trace_event::trace_event!("Paint");
    let mut painter = Painter {
        pre,
        images,
        artifact: PaintArtifact {
            device_scale_factor: pre.device_scale_factor,
            ..PaintArtifact::default()
        },
        skip_background: None,
        previous,
        cached: Vec::new(),
        cache,
        stacking: Vec::new(),
        hit_testable: true,
        painted: 0,
        walked: 0,
        subsequences: 0,
        glyph_ids: Vec::new(),
        glyph_positions: Vec::new(),
    };

    painter.document(layout_view, document_size, PropertyTreeState::ROOT);
    tracing::debug!(
        painted = painter.painted,
        cached = painter.walked.saturating_sub(painter.painted),
        subsequences = painter.subsequences,
        "paint"
    );
    let walked = (painter.walked, painter.subsequences);
    let mut artifact = painter.artifact;
    #[cfg(debug_assertions)]
    hit_test::assert_recorded(layout_view, pre, &artifact.hit_test);
    if let Some(previous) = previous {
        artifact.cached = Some(CachedItems {
            from: previous.artifact.id,
            indices: painter.cached,
        });
    }
    (artifact, walked)
}

struct Painter<'a> {
    pre: &'a PrePaint,
    images: &'a dyn BackgroundImages,
    artifact: PaintArtifact,
    /// The box whose background went to the canvas.
    skip_background: Option<NodeId>,
    previous: Option<&'a Previous>,
    /// For each item, its index in the previous paint if copied from it.
    cached: Vec<Option<usize>>,
    /// What each box and stacking context painted, by its fragment's
    /// address: the last paint's until painted in this one.
    cache: &'a mut PaintCache,
    /// The stacking contexts being painted, innermost last: each one's
    /// root, where its items and targets start, and the stacking contexts
    /// painted in it.
    stacking: Vec<(*const BoxFragment, Start, Vec<*const BoxFragment>)>,
    /// Hit testing goes into what is painted: not into a child frame whose
    /// iframe it doesn't find.
    hit_testable: bool,
    /// The boxes painted anew, not taken from the previous paint.
    painted: usize,
    /// The boxes walked.
    walked: usize,
    /// The stacking contexts taken whole from the previous paint.
    subsequences: usize,
    /// A text run's glyph ids and positions as they are placed, before
    /// they go into its `DrawGlyphs`: kept for the next run, so that a
    /// run allocates only what it keeps.
    glyph_ids: Vec<u16>,
    glyph_positions: Vec<PointF>,
}

impl Painter<'_> {
    /// A document: its canvas, then its root stacking context. The canvas
    /// takes the root element's background, or else `<body>`'s, and that
    /// box then paints none of its own (css-backgrounds, "the canvas
    /// background and the root element"); not `<body>`'s if either has
    /// containment (css-contain 2).
    fn document(
        &mut self,
        layout_view: &Arc<BoxFragment>,
        document_size: SizeF,
        state: PropertyTreeState,
    ) {
        let root = layout_view
            .children
            .first()
            .map(|link| &link.fragment)
            .filter(|f| f.role == BoxRole::RootElement);
        let body = root.and_then(|root| {
            root.children
                .iter()
                .map(|link| &link.fragment)
                .find(|f| f.role == BoxRole::Body)
        });
        let contained = root.is_some_and(|root| layout::applies_any_containment(&root.style))
            || body.is_some_and(|body| layout::applies_any_containment(&body.style));
        let canvas_source = root
            .filter(|root| {
                root.style.resolve(root.style.background_color).a != 0
                    || root.style.has_background_image()
            })
            .or(body.filter(|_| !contained));
        // A child frame's document has its own canvas, inside a document
        // whose canvas has been painted.
        let outer = self.skip_background;
        self.skip_background = None;
        if let Some(source) = canvas_source {
            // Its images are placed against the root element's box,
            // whichever element they came from ("as if specified on the
            // root element").
            let root = root.expect("the source is the root or its body");
            let at = self.pre.properties(root).offset;
            let geometry = BoxGeometry {
                border_box: RectF::new(at.x, at.y, root.size.width, root.size.height),
                border: root.border,
                padding: root.padding,
                radii: border_radii(&root.style, root.size),
            };
            self.box_background(
                source.node,
                DisplayItemKind::DocumentBackground,
                &source.style,
                &geometry,
                Some(RectF::new(
                    0.0,
                    0.0,
                    document_size.width,
                    document_size.height,
                )),
                state,
            );
            self.skip_background = source.node;
        }
        self.stacking_context(layout_view);
        self.skip_background = outer;
    }

    /// Appends an item, starting a new chunk when the state changes.
    fn push(&mut self, state: PropertyTreeState, item: DisplayItem) {
        self.push_item(state, item, None);
    }

    /// Appends item `index` of the previous paint as it was.
    fn push_cached(&mut self, state: PropertyTreeState, index: usize) {
        let previous = self.previous.expect("cached from a previous paint");
        let item = previous.artifact.items[index].clone();
        self.push_item(state, item, Some(index));
    }

    fn push_item(&mut self, state: PropertyTreeState, item: DisplayItem, cached: Option<usize>) {
        let index = self.artifact.items.len();
        let alone = item.kind == DisplayItemKind::Canvas;
        self.artifact.items.push(item);
        self.cached.push(cached);
        let items = &self.artifact.items;
        let end = items.len();
        match self.artifact.chunks.last_mut() {
            // A canvas's item is a chunk on its own, as Blink's foreign
            // layers are.
            Some(chunk)
                if chunk.state == state
                    && !alone
                    && items[chunk.items.clone()]
                        .last()
                        .is_some_and(|last| last.kind != DisplayItemKind::Canvas) =>
            {
                chunk.items.end = end
            }
            _ => self.artifact.chunks.push(PaintChunk {
                items: index..end,
                state,
            }),
        }
    }

    /// Paints a stacking context whose root is `root` (CSS 2 Appendix E):
    /// its own background, the child stacking contexts with a negative
    /// `z-index`, the other backgrounds, the lines with the flex items
    /// painted whole among them, then the other child stacking contexts
    /// and positioned boxes, `z-index` 0 and `auto` in tree order and the
    /// positive ones by `z-index`. A positioned box that isn't a stacking
    /// context paints the same with no children of its own there. The
    /// outlines come after the lines and before those children, where
    /// Blink paints them (`PaintPhase::kOutline`), not last (step 10).
    fn stacking_context(&mut self, root: &Arc<BoxFragment>) {
        let address = Arc::as_ptr(root);
        if self.take_subsequence(root) {
            if let Some((_, _, nested)) = self.stacking.last_mut() {
                nested.push(address);
            }
            return;
        }
        let start = self.start();
        let skip_background = self.skip_background;
        self.stacking.push((address, start, Vec::new()));
        self.stacking_context_items(root);
        let (_, _, nested) = self.stacking.pop().expect("pushed above");
        let parent = self.stacking.last().map(|&(parent, _, _)| parent);
        let out_of_flow = self.cache.out_of_flow_in(&nested);
        self.cache.subsequences.insert(
            address,
            paint_controller::Subsequence {
                fragment: root.clone(),
                paint: self.artifact.id,
                items: start.items..self.artifact.items.len(),
                hits: start.hits..self.artifact.hit_test.rects.len(),
                nested,
                out_of_flow,
                parent,
                skip_background,
                hit_testable: self.hit_testable,
            },
        );
        if let Some((_, _, nested)) = self.stacking.last_mut() {
            nested.push(address);
        }
    }

    /// Takes stacking context `root`'s items and targets from the previous
    /// paint, if it paints as it did: in their chunks' states, which give
    /// the same chunks painting it gives. Whether it took them.
    fn take_subsequence(&mut self, root: &BoxFragment) -> bool {
        let Some(previous) = self.previous else {
            return false;
        };
        let Some(subsequence) = previous.subsequence(
            self.cache,
            self.pre,
            root,
            self.skip_background,
            self.hit_testable,
        ) else {
            return false;
        };
        let (old, old_hits) = (subsequence.items.clone(), subsequence.hits.clone());
        let start = self.start();
        let chunks = &previous.artifact.chunks;
        let mut chunk = chunks.partition_point(|chunk| chunk.items.end <= old.start);
        for index in old.clone() {
            while chunks[chunk].items.end <= index {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "a chunk ends after `index`, an item of the last paint's"
                )]
                {
                    chunk += 1;
                }
            }
            self.push_cached(chunks[chunk].state, index);
        }
        self.push_hits_cached(old_hits.clone());
        let paint = self.artifact.id;
        let parent = self.stacking.last().map(|&(parent, _, _)| parent);
        #[expect(
            clippy::arithmetic_side_effects,
            clippy::cast_possible_wrap,
            reason = "lengths of Vecs, at most isize::MAX: they and their difference fit"
        )]
        let moved_by = (
            start.items as isize - old.start as isize,
            start.hits as isize - old_hits.start as isize,
        );
        self.cache
            .moved(root as *const BoxFragment, parent, paint, moved_by);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per subsequence painted: fewer than usize::MAX"
        )]
        {
            self.subsequences += 1;
        }
        true
    }

    /// Where the next item and target go.
    fn start(&self) -> Start {
        Start {
            items: self.artifact.items.len(),
            hits: self.artifact.hit_test.rects.len(),
        }
    }

    /// Records a target for hit testing (Blink's `HitTestDisplayItem`, or
    /// `ScrollHitTestDisplayItem` for a scroll container's own box,
    /// `container`), `wheel` if inside a blocking `wheel` listener,
    /// starting a new chunk when any of them but the rect changes.
    fn push_hit(
        &mut self,
        rect: RectF,
        state: PropertyTreeState,
        scroll: Option<ScrollNodeId>,
        container: Option<ScrollNodeId>,
        wheel: bool,
    ) {
        if !self.hit_testable {
            return;
        }
        let data = &mut self.artifact.hit_test;
        let index = data.rects.len();
        data.rects.push(rect);
        let end = data.rects.len();
        match data.chunks.last_mut() {
            Some(chunk)
                if chunk.state == state
                    && chunk.scroll == scroll
                    && chunk.container == container
                    && chunk.wheel == wheel =>
            {
                chunk.rects.end = end;
                chunk.bounds = chunk.bounds.union(&rect);
            }
            _ => data.chunks.push(HitTestChunk {
                rects: index..end,
                state,
                scroll,
                container,
                wheel,
                bounds: rect,
            }),
        }
    }

    /// Records the previous paint's targets `old` again, as they were.
    fn push_hits_cached(&mut self, old: Range<usize>) {
        if old.is_empty() {
            return;
        }
        let previous = self.previous.expect("cached from a previous paint");
        let data = &previous.artifact.hit_test;
        let mut chunk = data
            .chunks
            .partition_point(|chunk| chunk.rects.end <= old.start);
        for index in old {
            while data.chunks[chunk].rects.end <= index {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "a chunk ends after `index`, a target of the last paint's"
                )]
                {
                    chunk += 1;
                }
            }
            let c = &data.chunks[chunk];
            self.push_hit(data.rects[index], c.state, c.scroll, c.container, c.wheel);
        }
    }

    /// [`Self::stacking_context`]'s items, painted.
    fn stacking_context_items(&mut self, root: &Arc<BoxFragment>) {
        let children = stacking_children(root, None);
        let negative = children
            .iter()
            .take_while(|(child, _)| child.style.stack_level() < 0)
            .count();
        self.background(root);
        for (child, _) in &children[..negative] {
            self.stacking_context(child);
        }
        self.child_backgrounds(root);
        self.floats(root);
        self.text(root, true);
        self.outlines(root, true);
        for (child, _) in &children[negative..] {
            self.stacking_context(child);
        }
    }

    /// The float phase below `fragment` (CSS 2 Appendix E step 5; Blink's
    /// `PaintPhase::kFloat`): each float, in tree order, painted whole as
    /// a stacking context is (its backgrounds, its floats, its lines, its
    /// outlines), though the positioned boxes and stacking contexts in it
    /// are its stacking context's. Not into child stacking contexts, nor
    /// into what is painted whole elsewhere (an atomic inline, a flex
    /// item), which paints its own floats then.
    fn floats(&mut self, fragment: &Arc<BoxFragment>) {
        if paints_children_whole(fragment) {
            return;
        }
        let on_lines = children_on_lines(fragment);
        for child in &fragment.children {
            let child = &child.fragment;
            if child.style.is_stacked() {
                continue;
            }
            if child.floating {
                self.background(child);
                self.child_backgrounds(child);
                self.floats(child);
                self.text(child, true);
                self.outlines(child, true);
            } else if !on_lines {
                self.floats(child);
            }
        }
    }

    /// The background phase below `fragment`, skipping child stacking
    /// contexts, floats (painted whole in the float phase) and the flex
    /// items, painted whole in the line phase.
    fn backgrounds(&mut self, fragment: &Arc<BoxFragment>) {
        if fragment.style.is_stacked() || fragment.floating {
            return;
        }
        self.background(fragment);
        self.child_backgrounds(fragment);
    }

    fn child_backgrounds(&mut self, fragment: &Arc<BoxFragment>) {
        if paints_children_whole(fragment) || children_on_lines(fragment) {
            return;
        }
        for child in &fragment.children {
            self.backgrounds(&child.fragment);
        }
        // Over its cells' backgrounds, under their content (Blink's
        // `TablePainter::PaintCollapsedBorders`).
        if let Some(borders) = &fragment.collapsed_borders {
            self.collapsed_borders(fragment, borders);
        }
    }

    /// A table's collapsed borders (Blink's `TablePainter::
    /// PaintCollapsedBorders`): each edge on its grid line, as wide as its
    /// border, centered on it, as long as its row or column; at each joint,
    /// stretched over it or shortened out of it as the edges there say
    /// (`edge_joints`).
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "indices of the table's edges, a Vec of (rows + 1) × (columns + 1) × 2, \
                  and of its offsets, checked to be rows + 1 and columns + 1"
    )]
    fn collapsed_borders(&mut self, fragment: &BoxFragment, borders: &CollapsedBorders) {
        let (rows, columns) = (borders.rows, borders.columns);
        if !fragment.style.is_visible()
            || borders.row_offsets.len() != rows + 1
            || borders.column_offsets.len() != columns + 1
        {
            return;
        }
        let properties = self.pre.properties(fragment);
        let origin = properties.offset;
        let dsf = self.pre.device_scale_factor;
        let per_row = (columns + 1) * 2;
        for index in 0..borders.edges.len() {
            let edge = borders.edges[index];
            let (row, column) = (index / per_row, index % per_row / 2);
            let horizontal = index % 2 == 1;
            if !edge.can_paint()
                || (horizontal && column >= columns)
                || (!horizontal && row >= rows)
            {
                continue;
            }
            let (start, end) = edge_joints(borders, index);
            let (x0, y0) = (borders.column_offsets[column], borders.row_offsets[row]);
            let w = edge.width;
            let rect = if horizontal {
                let mut left = x0;
                let mut right = borders.column_offsets[column + 1];
                left += if start.wins {
                    -start.vertical / 2.0
                } else {
                    start.vertical / 2.0
                };
                right += if end.wins {
                    end.vertical / 2.0
                } else {
                    -end.vertical / 2.0
                };
                RectF::new(left, y0 - w / 2.0, right - left, w)
            } else {
                let mut top = y0;
                let mut bottom = borders.row_offsets[row + 1];
                top += if start.wins {
                    -start.horizontal / 2.0
                } else {
                    start.horizontal / 2.0
                };
                bottom += if end.wins {
                    end.horizontal / 2.0
                } else {
                    -end.horizontal / 2.0
                };
                RectF::new(x0 - w / 2.0, top, w, bottom - top)
            };
            let rect = snap(
                RectF::new(
                    origin.x + rect.x,
                    origin.y + rect.y,
                    rect.width,
                    rect.height,
                ),
                dsf,
            );
            let side = BorderSide {
                width: if horizontal { rect.height } else { rect.width },
                style: edge.style,
                color: edge.color,
            };
            self.border_side(
                fragment.node,
                rect,
                if horizontal { 0 } else { 3 },
                side,
                properties.local,
            );
        }
    }

    /// One side of a border drawn in `rect` (the top, `i` 0, or the left,
    /// `i` 3, as its style draws it there; Blink's `BoxBorderPainter::
    /// DrawBoxSide`): its bands across the rect, or its dashes along it.
    fn border_side(
        &mut self,
        client: Option<NodeId>,
        rect: RectF,
        i: usize,
        side: BorderSide,
        state: PropertyTreeState,
    ) {
        if !side.is_visible() || rect.is_empty() {
            return;
        }
        let item = |op| DisplayItem {
            client,
            kind: DisplayItemKind::BoxDecorationBackground,
            visual_rect: rect,
            op,
        };
        match SideLook::of(side, i, self.pre.device_scale_factor) {
            SideLook::Bands(bands) => {
                for (from, to, color) in bands {
                    let band = if i == 0 {
                        RectF::new(
                            rect.x,
                            rect.y + rect.height * from,
                            rect.width,
                            rect.height * (to - from),
                        )
                    } else {
                        RectF::new(
                            rect.x + rect.width * from,
                            rect.y,
                            rect.width * (to - from),
                            rect.height,
                        )
                    };
                    self.push(
                        state,
                        item(PaintOp::DrawRect {
                            rect: band,
                            flags: PaintFlags::fill(color),
                        }),
                    );
                }
            }
            SideLook::Dashes(style) => {
                let mut sides = [BorderSide {
                    width: 0.0,
                    style: BorderStyle::None,
                    color: side.color,
                }; 4];
                sides[i] = side;
                let op = self.dashed_side(RRectF::from_rect(rect), sides, i, style);
                self.push(state, item(op));
            }
        }
    }

    /// A box's own background and border. Each box is looked up in the
    /// previous paint here, for both phases.
    fn background(&mut self, fragment: &Arc<BoxFragment>) {
        if fragment.role == BoxRole::LayoutView {
            return;
        }
        let properties = self.pre.properties(fragment);
        let skips_background =
            self.skip_background.is_some() && fragment.node == self.skip_background;
        let cached = self.previous.and_then(|previous| {
            previous.cached(
                self.cache,
                fragment,
                &properties,
                skips_background,
                self.hit_testable,
            )
        });
        let start = self.artifact.items.len();
        match &cached {
            Some(cached) => {
                for index in cached.backgrounds.clone() {
                    self.push_cached(properties.local, index);
                }
            }
            None => {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "one per fragment painted: fewer than usize::MAX"
                )]
                {
                    self.painted += 1;
                }
                self.box_decoration(fragment, &properties, skips_background);
            }
        }
        let end = self.artifact.items.len();
        // Its border box, found by hit testing unless hidden or `pointer-
        // events: none` (the target is cheaper made again than kept).
        if hittable(&fragment.style) {
            let container = properties
                .scroll
                .filter(|s| fragment.node == Some(self.pre.trees.scroll(*s).element));
            self.push_hit(
                RectF::new(
                    properties.offset.x,
                    properties.offset.y,
                    fragment.size.width,
                    fragment.size.height,
                ),
                properties.local,
                properties.scroll,
                container,
                fragment.style.inside_blocking_wheel_event_handler,
            );
        }
        let hits = self.artifact.hit_test.rects.len();
        let &(owner, from, _) = self.stacking.last().expect("in a stacking context");
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per fragment walked: fewer than usize::MAX"
        )]
        {
            self.walked += 1;
        }
        if fragment.pseudo.is_none()
            && let Some(node) = fragment.node
        {
            self.cache.by_node.insert(node, Arc::as_ptr(fragment));
        }
        self.cache.boxes.insert(
            Arc::as_ptr(fragment),
            PaintedBox {
                fragment: fragment.clone(),
                properties,
                skips_background,
                owner,
                backgrounds: relative(start, end, from.items),
                text: relative(end, end, from.items),
                text_hits: relative(hits, hits, from.hits),
                hit_testable: self.hit_testable,
                child_frame: self.pre.child_frame(fragment).is_some(),
                cached,
            },
        );
    }

    /// `BoxPainter`: a box's outer shadows, its background unless it went
    /// to the canvas, its inset shadows and its border. A table's go around
    /// its grid, not its captions; a row group's or row's background goes
    /// into its cells, and it has no border (Blink's `TablePainter` and
    /// `TableCellPainter`, separated borders); an empty cell with
    /// `empty-cells: hide` paints nothing.
    fn box_decoration(
        &mut self,
        fragment: &BoxFragment,
        properties: &FragmentProperties,
        skips_background: bool,
    ) {
        let mut rect = RectF::new(
            properties.offset.x,
            properties.offset.y,
            fragment.size.width,
            fragment.size.height,
        );
        match table_part(fragment) {
            Some(TableDisplay::RowGroup | TableDisplay::Row) => {
                self.part_backgrounds(fragment, properties);
                return;
            }
            Some(TableDisplay::Cell) if hides_empty_cell(fragment) => return,
            Some(TableDisplay::Table) => {
                let grid = table_grid(fragment);
                rect = RectF::new(rect.x + grid.x, rect.y + grid.y, grid.width, grid.height);
            }
            _ => {}
        }
        let geometry = BoxGeometry {
            border_box: rect,
            border: fragment.border,
            padding: fragment.padding,
            radii: border_radii(&fragment.style, rect.size()),
        };
        let (node, style, state) = (fragment.node, &fragment.style, properties.local);
        // `visibility: hidden`: laid out, not painted (its descendants
        // may be).
        if style.is_visible() {
            self.box_shadows(node, style, &geometry, false, state);
            // A native look is drawn in place of the CSS background and
            // border (Blink's `ThemePainter::Paint`).
            if let Some(ops) = theme::ops(fragment, rect, self.pre.device_scale_factor) {
                for op in ops {
                    self.push(
                        state,
                        DisplayItem {
                            client: node,
                            kind: DisplayItemKind::BoxDecorationBackground,
                            visual_rect: rect,
                            op,
                        },
                    );
                }
                self.box_shadows(node, style, &geometry, true, state);
                self.column_backgrounds(fragment, properties);
                return;
            }
            if !skips_background {
                let inside = fragment
                    .background_inside_border
                    .then(|| inside_border(rect, fragment.border));
                self.box_background(
                    node,
                    DisplayItemKind::BoxDecorationBackground,
                    style,
                    &geometry,
                    inside,
                    state,
                );
            }
            self.box_shadows(node, style, &geometry, true, state);
            // Collapsed, its table draws its border with the others.
            if !fragment.border_collapsed {
                self.border(
                    fragment.node,
                    &fragment.style,
                    fragment.border,
                    rect,
                    border_radii(&fragment.style, rect.size()),
                    properties.local,
                );
            }
            // What of a native look is drawn over them.
            for op in theme::decorations(fragment, rect, self.pre.device_scale_factor)
                .into_iter()
                .flatten()
            {
                self.push(
                    state,
                    DisplayItem {
                        client: node,
                        kind: DisplayItemKind::BoxDecorationBackground,
                        visual_rect: rect,
                        op,
                    },
                );
            }
        }
        self.column_backgrounds(fragment, properties);
    }

    /// A table's column groups' and columns' backgrounds, placed in their
    /// own boxes (across the row groups), each painted in the cells that
    /// start in its columns (Blink's `TablePainter::
    /// PaintBoxDecorationBackground` and `TableRowPainter::
    /// PaintColumnsBackground`): over the table's background, under the row
    /// groups' and rows'; a group's under its columns'.
    fn column_backgrounds(&mut self, fragment: &BoxFragment, properties: &FragmentProperties) {
        if fragment.columns.is_empty() {
            return;
        }
        let origin = properties.offset;
        let mut cells = Vec::new();
        for child in &fragment.children {
            if table_part(&child.fragment) == Some(TableDisplay::RowGroup) {
                let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
                table_cells(&child.fragment, at, &mut cells);
            }
        }
        for column in &fragment.columns {
            let rect = RectF::new(
                origin.x + column.rect.x,
                origin.y + column.rect.y,
                column.rect.width,
                column.rect.height,
            );
            let geometry = BoxGeometry {
                border_box: rect,
                border: Sides::all(0.0),
                padding: Sides::all(0.0),
                radii: [SizeF::new(0.0, 0.0); 4],
            };
            for &cell in &cells {
                // The cells that start in its columns.
                if cell.x < rect.x || cell.x >= rect.x + rect.width {
                    continue;
                }
                self.box_background(
                    column.node,
                    DisplayItemKind::BoxDecorationBackground,
                    &column.style,
                    &geometry,
                    Some(cell),
                    properties.local,
                );
            }
        }
    }

    /// A row group's or row's background, placed in its own box, painted
    /// in each of its cells' border boxes (css-tables 3, "Drawing cell
    /// backgrounds"; Blink's `TableCellPainter::
    /// PaintBackgroundForTablePart`): not in the border spacing.
    fn part_backgrounds(&mut self, fragment: &BoxFragment, properties: &FragmentProperties) {
        let style = &fragment.style;
        if !style.is_visible() {
            return;
        }
        let origin = properties.offset;
        let geometry = BoxGeometry {
            border_box: RectF::new(
                origin.x,
                origin.y,
                fragment.size.width,
                fragment.size.height,
            ),
            border: Sides::all(0.0),
            padding: Sides::all(0.0),
            radii: [SizeF::new(0.0, 0.0); 4],
        };
        let mut cells = Vec::new();
        table_cells(fragment, origin, &mut cells);
        for cell in cells {
            self.box_background(
                fragment.node,
                DisplayItemKind::BoxDecorationBackground,
                style,
                &geometry,
                Some(cell),
                properties.local,
            );
        }
    }

    /// The line phase: every line in tree order, skipping child stacking
    /// contexts as `backgrounds` does. An atomic inline is painted whole
    /// where it comes in its line (CSS 2 Appendix E step 7.2.1.4; Blink's
    /// `PaintAllPhasesAtomically`), and so is a flex item where it comes
    /// (css-flexbox 1, "Flex Item Z-Ordering"): its backgrounds, then its
    /// lines; the stacking contexts in it are its stacking context's, as
    /// any others.
    fn text(&mut self, fragment: &Arc<BoxFragment>, is_root: bool) {
        if !is_root && (fragment.style.is_stacked() || fragment.floating) {
            return;
        }
        if fragment.role != BoxRole::LayoutView {
            let address = Arc::as_ptr(fragment);
            let painted = &self.cache.boxes[&address];
            let (properties, cached) = (painted.properties, painted.cached.clone());
            let start = self.start();
            match cached {
                // With atomic inlines, its lines are painted again: they
                // come between its runs, each under its own properties,
                // and take what they can from the last paint themselves.
                // So is an iframe's child frame, whose boxes do the same,
                // and what an iframe whose child frame went paints now.
                // Nor an `<svg>`'s content, each shape under its own
                // properties.
                Some(cached)
                    if !children_on_lines(fragment)
                        && self.pre.child_frame(fragment).is_none()
                        && !cached.child_frame
                        && fragment.svg.is_none() =>
                {
                    for index in cached.text {
                        self.push_cached(properties.contents, index);
                    }
                    self.push_hits_cached(cached.text_hits);
                }
                _ => self.contents(fragment, &properties),
            }
            let end = self.start();
            let from = self.stacking.last().expect("in a stacking context").1;
            let painted = self
                .cache
                .boxes
                .get_mut(&address)
                .expect("looked up in the background phase");
            painted.text = relative(start.items, end.items, from.items);
            painted.text_hits = relative(start.hits, end.hits, from.hits);
            // The caret after it all, painted anew each time: taking the
            // box's items from the last paint leaves it out.
            self.caret(fragment, &properties);
        }
        if children_on_lines(fragment) {
            return;
        }
        let whole = paints_children_whole(fragment);
        for child in &fragment.children {
            if whole {
                self.backgrounds(&child.fragment);
                self.floats(&child.fragment);
            }
            self.text(&child.fragment, false);
            if whole {
                self.outlines(&child.fragment, false);
            }
        }
    }

    /// A box's own line-phase items: a replaced element's image, in tree
    /// order with the lines (Appendix E step 7.1), then its lines. In a
    /// line, each inline element's background and border go right before
    /// its first text there, and each atomic inline whole where it comes.
    fn contents(&mut self, fragment: &BoxFragment, properties: &FragmentProperties) {
        let visible = fragment.style.is_visible();
        let (b, p) = (&fragment.border, &fragment.padding);
        let content = RectF::new(
            properties.offset.x + b.left + p.left,
            properties.offset.y + b.top + p.top,
            fragment.size.width - b.left - p.left - b.right - p.right,
            fragment.size.height - b.top - p.top - b.bottom - p.bottom,
        );
        // Where `object-fit` and `object-position` put the content.
        let object = fragment.object_rect.map_or(content, |r| {
            RectF::new(
                properties.offset.x + r.x,
                properties.offset.y + r.y,
                r.width,
                r.height,
            )
        });
        if let Some(image) = fragment.image.as_ref().filter(|_| visible) {
            self.image(
                fragment.node,
                DisplayItemKind::Image,
                image,
                FilterQuality::Medium,
                object,
                properties.contents,
            );
        }
        // An SVG image, its paint scaled into the content box (Blink's
        // `SVGImage` through `ImagePainter`).
        if let Some(image) = fragment.svg_image.filter(|_| visible)
            && let Some((record, size)) = self.images.svg_image(image, object.size(), fragment.node)
        {
            let dst = snap(object, self.pre.device_scale_factor);
            if dst.width > 0.0 && dst.height > 0.0 {
                self.push(
                    properties.contents,
                    DisplayItem {
                        client: fragment.node,
                        kind: DisplayItemKind::Image,
                        visual_rect: dst,
                        op: PaintOp::DrawRecord { record, size, dst },
                    },
                );
            }
        }
        // A canvas's bitmap, scaled into its content box (Blink's
        // `HTMLCanvasPainter::PaintReplaced`, sampled as
        // `CanvasDefaultInterpolationQuality`).
        if fragment.canvas
            && visible
            && let Some(snapshot) = fragment.node.and_then(|node| self.images.canvas(node))
        {
            self.image(
                fragment.node,
                DisplayItemKind::Canvas,
                &snapshot,
                FilterQuality::Low,
                object,
                properties.contents,
            );
        }
        // An iframe's child frame, where an image would be (Blink's
        // `EmbeddedContentPainter`).
        // Hit testing finds its canvas (within its content box, where its
        // viewport picks a wheel), then what is in it, unless the iframe is
        // `pointer-events: none`.
        let pre = self.pre;
        if let Some(frame) = pre.child_frame(fragment).filter(|_| visible) {
            let outer = self.hit_testable;
            self.hit_testable &= hittable(&fragment.style);
            let canvas = RectF::new(
                0.0,
                0.0,
                frame.document_size.width,
                frame.document_size.height,
            );
            let scroll = Some(frame.scroll);
            self.push_hit(
                canvas,
                frame.state,
                scroll,
                scroll,
                frame.root_in_blocking_wheel(),
            );
            self.document(&frame.layout_view, frame.document_size, frame.state);
            self.hit_testable = outer;
        }
        // An `<svg>`'s content, where an image would be: each shape says
        // whether it is visible.
        self.svg(fragment, properties.scroll);
        let (origin, state) = (properties.offset, properties.contents);
        for line in &fragment.lines {
            for part in line_parts(line) {
                // The targets on the line: an inline box, a run of text by
                // its advance as far as it is visible (`text-overflow` hides
                // the rest) and the font's ascent and descent around the
                // baseline.
                let target = match part {
                    LinePart::InlineBox(b) if hittable(&b.style) => Some((b.rect, &b.style)),
                    LinePart::Run(run) if hittable(&run.style) => Some((
                        RectF::new(
                            run.x,
                            run.baseline - run.ascent,
                            run.visible_advance,
                            run.ascent + run.descent,
                        ),
                        &run.style,
                    )),
                    _ => None,
                };
                if let Some((r, style)) = target {
                    let rect = RectF::new(origin.x + r.x, origin.y + r.y, r.width, r.height);
                    let wheel = style.inside_blocking_wheel_event_handler;
                    self.push_hit(rect, state, properties.scroll, None, wheel);
                }
                match part {
                    LinePart::InlineBox(inline_box) => self.inline_box(inline_box, origin, state),
                    LinePart::Atomic(child) => {
                        let child = &fragment.children[child].fragment;
                        self.backgrounds(child);
                        self.floats(child);
                        self.text(child, false);
                        self.outlines(child, false);
                    }
                    LinePart::Run(run) => {
                        self.text_run(run.node.or(fragment.node), run, line, origin, state)
                    }
                }
            }
            if let Some((client, selected)) = self.selected_end(line, origin) {
                let rect = selected.rect;
                let op = PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(selected.background),
                };
                let item = (client.or(fragment.node), state);
                self.text_item(item, DisplayItemKind::SelectionBackground, op, rect);
            }
        }
    }

    /// The caret, if it is in `fragment` (Blink's `CaretDisplayItemClient::
    /// PaintCaret`, with its block's foreground): 1 CSS px wide and as
    /// tall as the font where it is (Chrome 152), in that text's
    /// `caret-color` (`auto` is its `color`).
    fn caret(&mut self, fragment: &BoxFragment, properties: &FragmentProperties) {
        let Some(caret) = self.images.caret() else {
            return;
        };
        // Its block's box, or an anonymous block in it.
        let in_block = match fragment.node {
            Some(node) => node == caret.block,
            None => fragment.role == BoxRole::AnonymousBlock,
        };
        if !in_block {
            return;
        }
        let Some((x, top, bottom, style)) = caret_place(fragment, caret.at) else {
            return;
        };
        let dsf = self.pre.device_scale_factor;
        let origin = properties.offset;
        let (left, top) = (snap_to(origin.x + x, dsf), snap_to(origin.y + top, dsf));
        let bottom = snap_to(origin.y + bottom, dsf);
        let rect = RectF::new(left, top, 1.0, bottom - top);
        let color = style.rare_inherited.caret_color.resolve(style.color);
        let op = PaintOp::DrawRect {
            rect,
            flags: PaintFlags::fill(color),
        };
        let item = (fragment.node, properties.contents);
        self.text_item(item, DisplayItemKind::Caret, op, rect);
    }

    /// `InlineBoxPainter`: an inline element's background and border on one
    /// line. The border box may reach past the line box into its
    /// neighbours; lines paint in order, so a later line covers an earlier
    /// one's text there.
    fn inline_box(
        &mut self,
        inline_box: &InlineBoxFragment,
        origin: PointF,
        state: PropertyTreeState,
    ) {
        let r = inline_box.rect;
        let rect = RectF::new(origin.x + r.x, origin.y + r.y, r.width, r.height);
        let style = &inline_box.style;
        if !style.is_visible() {
            return;
        }
        let node = Some(inline_box.node);
        // Each line's part on its own (`box-decoration-break: slice`): the
        // start's corners only where it starts, the end's where it ends.
        let mut radii = border_radii(style, r.size());
        if !inline_box.first {
            radii[0] = SizeF::new(0.0, 0.0);
            radii[3] = SizeF::new(0.0, 0.0);
        }
        if !inline_box.last {
            radii[1] = SizeF::new(0.0, 0.0);
            radii[2] = SizeF::new(0.0, 0.0);
        }
        let geometry = BoxGeometry {
            border_box: rect,
            border: inline_box.border,
            padding: inline_box.padding,
            radii,
        };
        self.box_shadows(node, style, &geometry, false, state);
        self.box_background(
            node,
            DisplayItemKind::BoxDecorationBackground,
            style,
            &geometry,
            None,
            state,
        );
        self.box_shadows(node, style, &geometry, true, state);
        self.border(node, style, inline_box.border, rect, radii, state);
    }

    /// The outline phase below `fragment`, in tree order: each box's
    /// outline, then its lines' inline elements', skipping child stacking
    /// contexts and what is painted whole (where it comes in the line
    /// phase, all its phases there). Painted anew each time, not taken
    /// from the last paint: outlines are rare, and their items are under
    /// the box's state and its lines'.
    fn outlines(&mut self, fragment: &Arc<BoxFragment>, is_root: bool) {
        if !is_root && (fragment.style.is_stacked() || fragment.floating) {
            return;
        }
        if fragment.role != BoxRole::LayoutView {
            let properties = self.cache.boxes[&Arc::as_ptr(fragment)].properties;
            let rect = RectF::new(
                properties.offset.x,
                properties.offset.y,
                fragment.size.width,
                fragment.size.height,
            );
            let radii = border_radii(&fragment.style, fragment.size);
            self.outline(
                fragment.node,
                &fragment.style,
                rect,
                radii,
                properties.local,
            );
            let (origin, state) = (properties.offset, properties.contents);
            for line in &fragment.lines {
                for part in line_parts(line) {
                    if let LinePart::InlineBox(inline_box) = part {
                        let r = inline_box.rect;
                        let rect = RectF::new(origin.x + r.x, origin.y + r.y, r.width, r.height);
                        let radii = border_radii(&inline_box.style, r.size());
                        self.outline(Some(inline_box.node), &inline_box.style, rect, radii, state);
                    }
                }
            }
        }
        if children_on_lines(fragment) || paints_children_whole(fragment) {
            return;
        }
        for child in &fragment.children {
            self.outlines(&child.fragment, false);
        }
    }

    /// `OutlinePainter`: the outline around the border box `rect` with
    /// corners `radii`, `outline-offset` out from it, its corners grown
    /// with it. `auto` is the platform's focus ring.
    fn outline(
        &mut self,
        client: Option<NodeId>,
        style: &ComputedStyle,
        rect: RectF,
        radii: [SizeF; 4],
        state: PropertyTreeState,
    ) {
        if !style.outline_style.is_drawn() || !style.is_visible() {
            return;
        }
        let dsf = self.pre.device_scale_factor;
        let color = style.resolve(style.outline_color);
        let offset = snap_to(style.outline_offset, dsf);
        let edge = RRectF::new(snap(rect, dsf), radii).spread(offset);
        let mut ring = |inner: RRectF, outer: RRectF, color: Color| {
            if color.a == 0 || outer.rect.width <= 0.0 || outer.rect.height <= 0.0 {
                return;
            }
            let flags = PaintFlags::fill(color);
            let op = if inner.rect.width <= 0.0 || inner.rect.height <= 0.0 {
                PaintOp::DrawRRect {
                    rrect: outer,
                    flags,
                }
            } else {
                PaintOp::DrawDRRect {
                    outer,
                    inner,
                    clip: None,
                    flags,
                }
            };
            self.push(
                state,
                DisplayItem {
                    client,
                    kind: DisplayItemKind::Outline,
                    visual_rect: outer.rect,
                    op,
                },
            );
        };
        match style.outline_style {
            // Chrome's focus ring (`PaintFocusRing`): 2px of the outline
            // color against the box, and 1px of white around that, so it
            // shows on a dark page too; its corners rounded by the stroke
            // width where the box's aren't (`DefaultFocusRingCornerRadius`).
            OutlineStyle::Auto => {
                let edge = if edge.is_rect() {
                    RRectF::uniform(edge.rect, FOCUS_RING_WIDTH)
                } else {
                    edge
                };
                let dark = edge.spread(FOCUS_RING_WIDTH - 1.0);
                ring(dark, edge.spread(FOCUS_RING_WIDTH), Color::WHITE);
                ring(edge, dark, color);
            }
            // An offset so negative it turns the outline inside out leaves
            // it at least twice its width each way, around the box's
            // center (css-ui 4, "outline-offset").
            OutlineStyle::Line(_) => {
                let width = snap_border_width(style.outline_width, dsf);
                if width > 0.0 {
                    let b = snap(rect, dsf);
                    let mut outer = RRectF::new(b, radii).spread(offset + width);
                    let r = outer.rect;
                    let center = PointF::new(b.x + b.width / 2.0, b.y + b.height / 2.0);
                    let (w, h) = (r.width.max(2.0 * width), r.height.max(2.0 * width));
                    outer.rect = RectF::new(center.x - w / 2.0, center.y - h / 2.0, w, h);
                    let OutlineStyle::Line(line) = style.outline_style else {
                        unreachable!()
                    };
                    let side = BorderSide {
                        width,
                        style: line,
                        color,
                    };
                    self.border_ring(client, DisplayItemKind::Outline, outer, [side; 4], state);
                }
            }
        }
    }

    /// `BoxPainterBase::PaintNormalBoxShadow` (`inset` false: around the
    /// box, not under it) or `PaintInsetBoxShadow` (inside the padding
    /// box), the last shadow first so the first is on top. The blur
    /// radius is twice the standard deviation (Blink's
    /// `BlurRadiusToStdDev`).
    fn box_shadows(
        &mut self,
        client: Option<NodeId>,
        style: &ComputedStyle,
        geometry: &BoxGeometry,
        inset: bool,
        state: PropertyTreeState,
    ) {
        if style.box_shadow.is_empty() {
            return;
        }
        let dsf = self.pre.device_scale_factor;
        let border_box = geometry.rrect(BackgroundBox::BorderBox, dsf);
        let padding_box = geometry.rrect(BackgroundBox::PaddingBox, dsf);
        for shadow in style.box_shadow.iter().rev().filter(|s| s.inset == inset) {
            let color = style.resolve(shadow.color);
            if color.a == 0 {
                continue;
            }
            let sigma = shadow.blur / 2.0;
            let flags = PaintFlags {
                blur: sigma,
                ..PaintFlags::fill(color)
            };
            let (op, visual_rect) = if inset {
                // The padding box's ring outside the hole: the padding box
                // moved by the offset and shrunk by the spread.
                let hole = padding_box
                    .offset(shadow.x, shadow.y)
                    .spread(-shadow.spread);
                let hole = (hole.rect.width > 0.0 && hole.rect.height > 0.0).then_some(hole);
                let reach =
                    shadow.x.abs() + shadow.y.abs() + shadow.spread.abs() + 3.0 * sigma + 1.0;
                let shape = RRectF::from_rect(padding_box.rect.inset(-reach));
                let op = PaintOp::DrawShadow {
                    shape,
                    hole,
                    clip: padding_box,
                    inside: true,
                    flags,
                };
                (op, padding_box.rect)
            } else {
                let shape = border_box.offset(shadow.x, shadow.y).spread(shadow.spread);
                if shape.rect.width <= 0.0 || shape.rect.height <= 0.0 {
                    continue;
                }
                let visual_rect = shape.rect.inset(-3.0 * sigma);
                let op = PaintOp::DrawShadow {
                    shape,
                    hole: None,
                    clip: border_box,
                    inside: false,
                    flags,
                };
                (op, visual_rect)
            };
            self.push(
                state,
                DisplayItem {
                    client,
                    kind: DisplayItemKind::BoxDecorationBackground,
                    visual_rect,
                    op,
                },
            );
        }
    }

    /// The image scaled into `rect`, snapped to device pixels like
    /// backgrounds.
    fn image(
        &mut self,
        client: Option<NodeId>,
        kind: DisplayItemKind,
        image: &ImageData,
        filter: FilterQuality,
        rect: RectF,
        state: PropertyTreeState,
    ) {
        let rect = snap(rect, self.pre.device_scale_factor);
        if rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        self.push(
            state,
            DisplayItem {
                client,
                kind,
                visual_rect: rect,
                op: PaintOp::DrawImage {
                    image: image.clone(),
                    src: None,
                    dst: rect,
                    filter,
                    flags: None,
                },
            },
        );
    }

    /// `TextFragmentPainter`: the glyphs' shadows, each decoration's
    /// underline and overline, the glyphs, then each one's line-through;
    /// the lines after their shadows (Blink's `PaintWithTextShadow`), all
    /// shadows the last one first. The baseline snaps to a device pixel,
    /// like Blink's.
    fn text_run(
        &mut self,
        client: Option<NodeId>,
        run: &TextFragment,
        line: &LineFragment,
        origin: PointF,
        state: PropertyTreeState,
    ) {
        let style = &run.style;
        if !style.is_visible() {
            return;
        }
        let dsf = self.pre.device_scale_factor;
        let exact = origin.y + run.baseline;
        let baseline = snap_to(exact, dsf);
        let shift = baseline - exact;
        // Placed from the shaped text the run is a span of.
        let (ids, positions) = (&mut self.glyph_ids, &mut self.glyph_positions);
        ids.clear();
        positions.clear();
        run.for_each_glyph(run.hidden_from.unwrap_or(usize::MAX), |id, at| {
            ids.push(glyph_id(id));
            positions.push(PointF::new(origin.x + at.x, origin.y + at.y + shift));
        });
        if ids.is_empty() {
            return;
        }
        let (ids, positions): (Arc<[u16]>, Arc<[PointF]>) =
            (Arc::from(&ids[..]), Arc::from(&positions[..]));
        let x = origin.x + run.x;
        let place = (x, baseline - run.ascent, run.visible_advance);
        let decorations = style.rare_inherited.applied_text_decorations.clone();
        let lines = |line: text_decoration::Line| {
            decorations
                .iter()
                .filter(move |decoration| match line {
                    text_decoration::Line::Under => decoration.lines.underline,
                    text_decoration::Line::Over => decoration.lines.overline,
                    text_decoration::Line::Through => decoration.lines.line_through,
                })
                .flat_map(move |decoration| {
                    text_decoration::line_ops(run, decoration, line, place, decoration.color, dsf)
                })
        };
        let under_over: Vec<(PaintOp, RectF)> = lines(text_decoration::Line::Under)
            .chain(lines(text_decoration::Line::Over))
            .collect();
        let through: Vec<(PaintOp, RectF)> = lines(text_decoration::Line::Through).collect();

        let size = run.font_size;
        let (across, up) = glyph_reach(size);
        let bounds = RectF::new(
            x - across,
            baseline - run.ascent - up,
            run.visible_advance + 2.0 * across,
            run.ascent + run.descent + 2.0 * up,
        );
        let glyphs = (
            PaintOp::DrawGlyphs {
                font: run.font.clone(),
                size,
                glyphs: ids,
                positions,
                rotations: Arc::default(),
                flags: PaintFlags::fill(style.resolve(style.rare_inherited.text_fill_color)),
                embolden: run.embolden,
                skew: run.skew,
                variations: run.variations.clone(),
                bounds,
            },
            bounds,
        );

        let item = (client, state);
        let selected = self.selected(run, line, origin);
        let shadows = &style.rare_inherited.text_shadow;
        self.text_shadows(
            item,
            DisplayItemKind::Text,
            std::slice::from_ref(&glyphs),
            shadows,
            style.color,
        );
        self.text_shadows(
            item,
            DisplayItemKind::TextDecoration,
            &under_over,
            shadows,
            style.color,
        );
        // The text and its decorations as they are outside the selection
        // only (its shadows aren't cut: Chrome 152 shows them under a
        // translucent selection).
        let outside = selected.as_ref().map(|selected| selected.rect);
        for (op, visual_rect) in under_over {
            let kind = DisplayItemKind::TextDecoration;
            self.text_item_outside(item, kind, op, visual_rect, visual_rect, outside);
        }
        // Drawn again over the selection, in its color.
        let (glyphs, visual_rect) = glyphs;
        let selected_glyphs = outside.is_some().then(|| glyphs.clone());
        self.text_item_outside(
            item,
            DisplayItemKind::Text,
            glyphs,
            visual_rect,
            bounds,
            outside,
        );
        self.text_shadows(
            item,
            DisplayItemKind::TextDecoration,
            &through,
            shadows,
            style.color,
        );
        for (op, visual_rect) in through {
            let kind = DisplayItemKind::TextDecoration;
            self.text_item_outside(item, kind, op, visual_rect, visual_rect, outside);
        }
        // The selected part over it all (css-pseudo 4's highlight overlay;
        // Blink's `HighlightPainter`), clipped to it: its background, then
        // the highlight's shadows, decorations and text. The text's own
        // decorations are drawn again in the highlight's color, and its
        // shadows aren't (Chrome 152).
        if let Some(selected) = selected {
            let rect = selected.rect;
            let background = PaintOp::DrawRect {
                rect,
                flags: PaintFlags::fill(selected.background),
            };
            self.text_item(item, DisplayItemKind::SelectionBackground, background, rect);
            let highlight = style.rare_inherited.highlight.as_deref();
            let own = highlight.map(|highlight| AppliedTextDecoration {
                lines: highlight.text_decoration_line,
                style: highlight.text_decoration_style,
                color: match highlight.text_decoration_color {
                    computed_style::StyleColor::CurrentColor => selected.text,
                    computed_style::StyleColor::Rgba(color) => color,
                },
                thickness: highlight.text_decoration_thickness,
                underline_offset: highlight.text_underline_offset,
            });
            let decorations: Vec<AppliedTextDecoration> = decorations
                .iter()
                .map(|decoration| AppliedTextDecoration {
                    color: selected.text,
                    ..*decoration
                })
                .chain(own)
                .collect();
            let clip = |(op, visual_rect): (PaintOp, RectF)| {
                (clipped(&op, rect.x, rect.right(), visual_rect), visual_rect)
            };
            let lines = |line: text_decoration::Line| {
                decorations
                    .iter()
                    .filter(move |decoration| match line {
                        text_decoration::Line::Under => decoration.lines.underline,
                        text_decoration::Line::Over => decoration.lines.overline,
                        text_decoration::Line::Through => decoration.lines.line_through,
                    })
                    .flat_map(move |decoration| {
                        text_decoration::line_ops(
                            run,
                            decoration,
                            line,
                            place,
                            decoration.color,
                            dsf,
                        )
                    })
                    .map(clip)
            };
            let under_over: Vec<(PaintOp, RectF)> = lines(text_decoration::Line::Under)
                .chain(lines(text_decoration::Line::Over))
                .collect();
            let through: Vec<(PaintOp, RectF)> = lines(text_decoration::Line::Through).collect();
            let mut text = selected_glyphs.expect("kept for the selection");
            if let PaintOp::DrawGlyphs { flags, .. } = &mut text {
                *flags = PaintFlags::fill(selected.text);
            }
            let text = (clipped(&text, rect.x, rect.right(), bounds), visual_rect);
            let shadows = highlight.map_or(&[][..], |highlight| &highlight.text_shadow[..]);
            self.text_shadows(
                item,
                DisplayItemKind::Text,
                std::slice::from_ref(&text),
                shadows,
                selected.text,
            );
            for (kind, ops) in [
                (DisplayItemKind::TextDecoration, under_over),
                (DisplayItemKind::Text, vec![text]),
                (DisplayItemKind::TextDecoration, through),
            ] {
                if kind == DisplayItemKind::TextDecoration {
                    self.text_shadows(item, kind, &ops, shadows, selected.text);
                }
                for (op, visual_rect) in ops {
                    self.text_item(item, kind, op, visual_rect);
                }
            }
        }
    }

    /// Where `run` (on `line`, painted at `origin`) is selected, and the
    /// colors it is drawn in there, if any of it is: the part of its text
    /// node's data the selection has, unless its text can't be selected
    /// (`user-select: none`, which Blink passes over), from where that
    /// starts to where it ends, snapped to device pixels, as tall as
    /// [`Self::selection_rect`] says.
    fn selected(
        &self,
        run: &TextFragment,
        line: &LineFragment,
        origin: PointF,
    ) -> Option<Selected> {
        let (text, _) = run.source.as_ref()?;
        if run.style.rare_inherited.user_select == computed_style::UserSelect::None {
            return None;
        }
        let selection = self.images.selection(*text)?;
        let (left, right) = run.x_range(selection.range.start, selection.range.end);
        let font = (run.baseline - run.ascent, run.baseline + run.descent);
        self.selection_rect(
            &run.style,
            font,
            line,
            origin,
            (left, right),
            selection.active,
        )
    }

    /// What a selection over the end of `line` draws, if one is: a space's
    /// box after its text (Chrome 152), and the element it is painted as
    /// (`LineEnd`).
    fn selected_end(
        &self,
        line: &LineFragment,
        origin: PointF,
    ) -> Option<(Option<NodeId>, Selected)> {
        let end = line.end.as_ref().filter(|_| !line.hidden)?;
        if end.style.rare_inherited.user_select == computed_style::UserSelect::None {
            return None;
        }
        // Where in its text the line ends only if it is selected: that
        // takes the text's clusters.
        if let Some(text) = end.text() {
            self.images.selection(text)?;
        }
        let selection = match end.kind() {
            LineEndKind::Wrap { text, offset } => self
                .images
                .selection(text)
                .filter(|s| s.range.start < offset && s.range.end > offset)?,
            LineEndKind::End { text, offset } => self
                .images
                .selection(text)
                .filter(|s| s.range.start < offset && s.range.end == u32::MAX)?,
            LineEndKind::Newline { text, offset } => self
                .images
                .selection(text)
                .filter(|s| s.range.start <= offset && s.range.end > offset)?,
            LineEndKind::Break(br) => self.images.selection(br)?,
        };
        let font = (end.baseline - end.ascent, end.baseline + end.descent);
        let selected = self.selection_rect(
            &end.style,
            font,
            line,
            origin,
            (end.x(), end.x() + end.width),
            selection.active,
        )?;
        Some((end.node, selected))
    }

    /// The box from `left` to `right` of a selection on `line` over text
    /// whose font reaches from `font`'s top to its bottom, painted at
    /// `origin`, snapped to device pixels, as tall as the line or the font,
    /// whichever reaches further (Chrome 152); and its colors: `style`'s
    /// `::selection` style's (the background's `currentcolor` that style's
    /// `color`, and that the element's), or the default ones, active or
    /// not. `None` if it is empty.
    fn selection_rect(
        &self,
        style: &ComputedStyle,
        (font_top, font_bottom): (f32, f32),
        line: &LineFragment,
        origin: PointF,
        (left, right): (f32, f32),
        active: bool,
    ) -> Option<Selected> {
        let dsf = self.pre.device_scale_factor;
        let (left, right) = (
            snap_to(origin.x + left, dsf),
            snap_to(origin.x + right, dsf),
        );
        if right <= left {
            return None;
        }
        let top = line.rect.y.min(font_top);
        let bottom = line.rect.bottom().max(font_bottom);
        let (top, bottom) = (
            snap_to(origin.y + top, dsf),
            snap_to(origin.y + bottom, dsf),
        );
        let (background, text) = match &style.rare_inherited.highlight {
            Some(highlight) if highlight.author_colors => {
                let color = match highlight.color {
                    computed_style::StyleColor::CurrentColor => style.color,
                    computed_style::StyleColor::Rgba(color) => color,
                };
                let background = match highlight.background_color {
                    computed_style::StyleColor::CurrentColor => color,
                    computed_style::StyleColor::Rgba(background) => background,
                };
                (background, color)
            }
            _ => {
                let colors = self.images.selection_colors(active);
                (colors.background, colors.text)
            }
        };
        Some(Selected {
            rect: RectF::new(left, top, right - left, bottom - top),
            background,
            text,
        })
    }

    /// `ops`' shadows in `shadows` (a `text-shadow`), of text in
    /// `current_color`: each shadow of all of them (Blink draws them black
    /// in a layer that casts it), the last first.
    fn text_shadows(
        &mut self,
        item: (Option<NodeId>, PropertyTreeState),
        kind: DisplayItemKind,
        ops: &[(PaintOp, RectF)],
        shadows: &[BoxShadow],
        current_color: Color,
    ) {
        for shadow in shadows.iter().rev() {
            for (op, visual_rect) in ops {
                let (op, visual_rect) =
                    text_decoration::shadow_of(op, *visual_rect, shadow, current_color);
                self.text_item(item, kind, op, visual_rect);
            }
        }
    }

    /// A text run's item `op`, which reaches over `bounds`, as it is
    /// outside `selection` only, on either side, if there is one.
    fn text_item_outside(
        &mut self,
        item: (Option<NodeId>, PropertyTreeState),
        kind: DisplayItemKind,
        op: PaintOp,
        visual_rect: RectF,
        bounds: RectF,
        selection: Option<RectF>,
    ) {
        let Some(selection) = selection else {
            return self.text_item(item, kind, op, visual_rect);
        };
        for (left, right) in [(bounds.x, selection.x), (selection.right(), bounds.right())] {
            if right > left {
                self.text_item(item, kind, clipped(&op, left, right, bounds), visual_rect);
            }
        }
    }

    /// One of a text run's items, unless it draws nothing (transparent).
    fn text_item(
        &mut self,
        (client, state): (Option<NodeId>, PropertyTreeState),
        kind: DisplayItemKind,
        op: PaintOp,
        visual_rect: RectF,
    ) {
        // A part of one (a wave, glyphs, a selected part) is clipped.
        fn flags(op: &PaintOp) -> &PaintFlags {
            match op {
                PaintOp::DrawClipped { op, .. } => flags(op),
                PaintOp::DrawRect { flags, .. }
                | PaintOp::DrawLine { flags, .. }
                | PaintOp::DrawPath { flags, .. }
                | PaintOp::DrawGlyphs { flags, .. } => flags,
                _ => unreachable!("a text's op"),
            }
        }
        if flags(&op).color.a == 0 {
            return;
        }
        self.push(
            state,
            DisplayItem {
                client,
                kind,
                visual_rect,
                op,
            },
        );
    }

    /// `BoxBorderPainter`: the border of the box `rect` with corners
    /// `radii`, each side as its style says ([`Self::border_ring`]). The
    /// outer edge snaps like a background, and each side's width as a
    /// border width (css-values): down to whole device pixels, at least one
    /// if it has any, so a 1px border at 1.5x is one sharp device pixel,
    /// not two faint ones. Layout keeps the unsnapped widths (Blink snaps
    /// them in style, before layout): what is inside moves by less than a
    /// device pixel.
    fn border(
        &mut self,
        client: Option<NodeId>,
        style: &ComputedStyle,
        width: Sides<f32>,
        rect: RectF,
        radii: [SizeF; 4],
        state: PropertyTreeState,
    ) {
        let dsf = self.pre.device_scale_factor;
        let side = |width: f32, border_style, color| BorderSide {
            width: snap_border_width(width, dsf),
            style: border_style,
            color: style.resolve(color),
        };
        let (s, c) = (&style.border_style, &style.border_color);
        let sides = [
            side(width.top, s.top, c.top),
            side(width.right, s.right, c.right),
            side(width.bottom, s.bottom, c.bottom),
            side(width.left, s.left, c.left),
        ];
        let outer = RRectF::new(snap(rect, dsf), radii);
        self.border_ring(
            client,
            DisplayItemKind::BoxDecorationBackground,
            outer,
            sides,
            state,
        );
    }

    /// A ring of four sides inside `outer` (a border, or an outline's
    /// line), each drawn as its style says (Blink's `BoxBorderPainter`):
    /// filled bands, shaded for `inset`, `outset`, `groove` and `ridge`,
    /// two for `double`, or dashes and dots. Each side is the part of the
    /// ring in the trapezoid between the edges and the corners' diagonals.
    /// When the four are the same, the ring is drawn whole, with no
    /// diagonal joins for anti-aliasing to leave a pixel of what is under
    /// it through (Blink's `DrawDRRect` fast path); on a box without
    /// rounded corners, the sides of one color share a path, for the same
    /// reason.
    fn border_ring(
        &mut self,
        client: Option<NodeId>,
        kind: DisplayItemKind,
        outer: RRectF,
        sides: [BorderSide; 4],
        state: PropertyTreeState,
    ) {
        let [top, right, bottom, left] = sides.map(|side| side.width);
        let inner = outer.inset(top, right, bottom, left);
        let polygons = side_polygons(outer.rect, inner.rect);
        let dsf = self.pre.device_scale_factor;
        let looks: Vec<(usize, SideLook)> = (0..4)
            .filter(|&i| sides[i].is_visible())
            .map(|i| (i, SideLook::of(sides[i], i, dsf)))
            .collect();
        if looks.is_empty() {
            return;
        }
        let item = |op| DisplayItem {
            client,
            kind,
            visual_rect: outer.rect,
            op,
        };
        // A band of every side: the ring between those fractions of each
        // side's width in from the outer edge.
        let band = |from: f32, to: f32| {
            let at = |f: f32| outer.inset(top * f, right * f, bottom * f, left * f);
            (at(from), at(to))
        };
        // Every side with a width drawn the same (a side with none doesn't
        // change the ring, unless its missing corners would: rounded).
        let present = sides.iter().filter(|side| side.width > 0.0).count();
        let uniform = looks.len() == present
            && (outer.is_rect() || present == 4)
            && looks.iter().all(|(_, look)| *look == looks[0].1);
        if uniform && let SideLook::Bands(bands) = &looks[0].1 {
            for &(from, to, color) in bands {
                let (o, i) = band(from, to);
                let op = if outer.is_rect() {
                    PaintOp::DrawPath {
                        path: Path::polygons(ring_polygons(o.rect, i.rect)),
                        flags: PaintFlags::fill(color),
                    }
                } else {
                    PaintOp::DrawDRRect {
                        outer: o,
                        inner: i,
                        clip: None,
                        flags: PaintFlags::fill(color),
                    }
                };
                self.push(state, item(op));
            }
            return;
        }
        // Without rounded corners, each band's trapezoids, grouped by color.
        let mut groups: Vec<(Color, Vec<Vec<PointF>>)> = Vec::new();
        for (i, look) in looks {
            match look {
                SideLook::Bands(bands) => {
                    for (from, to, color) in bands {
                        let (o, inside) = band(from, to);
                        if outer.is_rect() {
                            let polygon = side_polygons(o.rect, inside.rect)[i].clone();
                            match groups.iter_mut().find(|(c, _)| *c == color) {
                                Some((_, polygons)) => polygons.push(polygon),
                                None => groups.push((color, vec![polygon])),
                            }
                        } else {
                            let op = PaintOp::DrawDRRect {
                                outer: o,
                                inner: inside,
                                clip: Some(polygons[i].clone().into()),
                                flags: PaintFlags::fill(color),
                            };
                            self.push(state, item(op));
                        }
                    }
                }
                SideLook::Dashes(style) => {
                    let op = self.dashed_side(outer, sides, i, style);
                    self.push(
                        state,
                        item(PaintOp::DrawClipped {
                            polygon: polygons[i].clone().into(),
                            op: Box::new(op),
                        }),
                    );
                }
            }
        }
        for (color, polygons) in groups {
            self.push(
                state,
                item(PaintOp::DrawPath {
                    path: Path::polygons(polygons),
                    flags: PaintFlags::fill(color),
                }),
            );
        }
    }

    /// Side `i` of the ring in `outer` drawn `dotted` or `dashed`: a stroke
    /// as wide as the side along its middle, corner to corner (on a box
    /// with rounded corners, the ring's middle curve all the way round),
    /// its dashes spaced to end on the corners (Blink's
    /// `StrokeData::SelectBestDashGap`).
    fn dashed_side(
        &self,
        outer: RRectF,
        sides: [BorderSide; 4],
        i: usize,
        style: BorderStyle,
    ) -> PaintOp {
        let [top, right, bottom, left] = sides.map(|side| side.width);
        let BorderSide { width, color, .. } = sides[i];
        // Dots are dashes of length 0 with round caps.
        let stroke = |dash: Option<Dash>| PaintFlags {
            cap: if style == BorderStyle::Dotted && dash.is_some() {
                LineCap::Round
            } else {
                LineCap::Butt
            },
            dash,
            ..PaintFlags::stroke(color, width)
        };
        if !outer.is_rect() {
            let middle = outer.inset(top / 2.0, right / 2.0, bottom / 2.0, left / 2.0);
            let length = rrect_perimeter(&middle);
            return PaintOp::DrawRRect {
                rrect: middle,
                flags: stroke(dash_for(style, width, length, true)),
            };
        }
        let o = outer.rect;
        let (mut p0, mut p1) = match i {
            0 => (
                PointF::new(o.x, o.y + top / 2.0),
                PointF::new(o.right(), o.y + top / 2.0),
            ),
            1 => (
                PointF::new(o.right() - right / 2.0, o.y),
                PointF::new(o.right() - right / 2.0, o.bottom()),
            ),
            2 => (
                PointF::new(o.right(), o.bottom() - bottom / 2.0),
                PointF::new(o.x, o.bottom() - bottom / 2.0),
            ),
            _ => (
                PointF::new(o.x + left / 2.0, o.bottom()),
                PointF::new(o.x + left / 2.0, o.y),
            ),
        };
        let length = (p1.x - p0.x).abs() + (p1.y - p0.y).abs();
        // Dots are centred half a width in from the corners, so the first
        // and last are whole.
        if style == BorderStyle::Dotted && length > width {
            let (dx, dy) = ((p1.x - p0.x) / length, (p1.y - p0.y) / length);
            let h = width / 2.0;
            p0 = PointF::new(p0.x + dx * h, p0.y + dy * h);
            p1 = PointF::new(p1.x - dx * h, p1.y - dy * h);
        }
        PaintOp::DrawLine {
            p0,
            p1,
            flags: stroke(dash_for(style, width, length, false)),
        }
    }

    /// `BoxPainterBase::PaintFillLayers`: the background color, clipped
    /// as the bottom layer says, then each layer's image, bottom first.
    /// `canvas` is the area the root element's background covers instead
    /// of its clip box (the whole canvas), its images still placed in its
    /// box.
    fn box_background(
        &mut self,
        client: Option<NodeId>,
        kind: DisplayItemKind,
        style: &ComputedStyle,
        geometry: &BoxGeometry,
        canvas: Option<RectF>,
        state: PropertyTreeState,
    ) {
        let dsf = self.pre.device_scale_factor;
        let layers = style.background_layers();
        let bottom = layers.last().expect("at least one layer");
        let shrink = geometry.border_obscures_background(style, dsf);
        let area = |clip: BackgroundBox| match canvas {
            Some(canvas) => RRectF::from_rect(snap(canvas, dsf)),
            None if shrink && clip == BackgroundBox::BorderBox => geometry.shrunk_border_box(dsf),
            None => geometry.rrect(clip, dsf),
        };
        let color_area = area(bottom.clip);
        let color = style.resolve(style.background_color);
        if color_area.is_rect() {
            self.fill(client, kind, color_area.rect, color, state);
        } else {
            self.fill_rrect(client, kind, color_area, PaintFlags::fill(color), state);
        }
        for layer in layers.iter().rev() {
            let painting_area = area(layer.clip);
            let positioning = geometry.rect(layer.origin, dsf);
            let Some(paint) = background::layer_paint(
                style,
                layer,
                self.images,
                client,
                painting_area.rect,
                positioning,
                dsf,
            ) else {
                continue;
            };
            if painting_area.is_rect() {
                self.push(
                    state,
                    DisplayItem {
                        client,
                        kind,
                        visual_rect: paint.rect,
                        op: PaintOp::DrawRect {
                            rect: paint.rect,
                            flags: PaintFlags::shader(paint.shader),
                        },
                    },
                );
            } else {
                // The whole rounded area: the shader draws nothing past a
                // tile on an axis that doesn't repeat.
                let shader = compositor_paint::TiledShader {
                    decal: true,
                    ..paint.shader
                };
                self.fill_rrect(
                    client,
                    kind,
                    painting_area,
                    PaintFlags::shader(shader),
                    state,
                );
            }
        }
    }

    /// `rrect` filled with `flags` (a color or a shader), unless it is
    /// empty or a transparent color.
    fn fill_rrect(
        &mut self,
        client: Option<NodeId>,
        kind: DisplayItemKind,
        rrect: RRectF,
        flags: PaintFlags,
        state: PropertyTreeState,
    ) {
        if rrect.rect.width <= 0.0
            || rrect.rect.height <= 0.0
            || (flags.shader.is_none() && flags.color.a == 0)
        {
            return;
        }
        self.push(
            state,
            DisplayItem {
                client,
                kind,
                visual_rect: rrect.rect,
                op: PaintOp::DrawRRect { rrect, flags },
            },
        );
    }

    fn fill(
        &mut self,
        client: Option<NodeId>,
        kind: DisplayItemKind,
        rect: RectF,
        color: Color,
        state: PropertyTreeState,
    ) {
        let rect = snap(rect, self.pre.device_scale_factor);
        if color.a == 0 || rect.width <= 0.0 || rect.height <= 0.0 {
            return;
        }
        self.push(
            state,
            DisplayItem {
                client,
                kind,
                visual_rect: rect,
                op: PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(color),
                },
            },
        );
    }
}

/// Which table box `fragment` is, by its `display` (a table, a row group,
/// a row, a cell or a caption), if it is one.
pub(crate) fn table_part(fragment: &BoxFragment) -> Option<TableDisplay> {
    use computed_style::Display;
    match fragment.style.display {
        Display::Table(TableDisplay::Table) | Display::InlineTable => Some(TableDisplay::Table),
        Display::Table(
            TableDisplay::RowGroup | TableDisplay::HeaderGroup | TableDisplay::FooterGroup,
        ) => Some(TableDisplay::RowGroup),
        Display::Table(part) => Some(part),
        _ => None,
    }
}

/// Whether `fragment` is a cell with `empty-cells: hide` and no content
/// (css-tables 3: no in-flow content; Blink's `IsEmptyTableCell`).
fn hides_empty_cell(fragment: &BoxFragment) -> bool {
    !fragment.border_collapsed
        && fragment.style.empty_cells == EmptyCells::Hide
        && fragment.lines.is_empty()
        && fragment
            .children
            .iter()
            .all(|child| child.fragment.style.is_out_of_flow())
}

/// `rect` without its `border`.
fn inside_border(rect: RectF, border: Sides<f32>) -> RectF {
    RectF::new(
        rect.x + border.left,
        rect.y + border.top,
        (rect.width - border.left - border.right).max(0.0),
        (rect.height - border.top - border.bottom).max(0.0),
    )
}

/// Where two edges of a table's collapsed borders meet one of theirs, at
/// the start or end of an edge (Blink's `ComputeEdgeJoints`): whether the
/// edge takes the joint (stretches over it, rather than stopping short of
/// it), and the widths of the vertical and horizontal edges that win it.
struct Joint {
    wins: bool,
    vertical: f32,
    horizontal: f32,
}

/// The joints at the start and end of edge `index` of `borders`: of the
/// two horizontal edges (before and after) and the two vertical ones (over
/// and under) there, each pair's winner, then the winner of those. A
/// horizontal edge takes a joint its side wins, but not a start joint
/// where the edge before it wins (nor an end joint where the edge after
/// it does); the same for a vertical edge with those over and under it.
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    reason = "offsets from an index of the table's edges, a Vec (at most isize::MAX \
              long): an index made of one is checked against its length before use"
)]
fn edge_joints(borders: &CollapsedBorders, index: usize) -> (Joint, Joint) {
    let per_row = ((borders.columns + 1) * 2) as isize;
    let horizontal = index % 2 == 1;
    let at = |offset: isize| {
        let i = index as isize + offset;
        (0..borders.edges.len() as isize)
            .contains(&i)
            .then_some(i as usize)
    };
    let width = |i: Option<usize>| i.map_or(0.0, |i| borders.edges[i].width);
    let joint = |[before, after, over, under]: [Option<usize>; 4], start: bool| {
        let inline = compare_for_paint(borders, before, after);
        let block = compare_for_paint(borders, over, under);
        let (h, v) = (
            if inline == 1 { before } else { after },
            if block == 1 { over } else { under },
        );
        let both = compare_for_paint(borders, h, v);
        let lost = if start { 1 } else { -1 };
        Joint {
            wins: if horizontal {
                both != -1 && inline != lost
            } else {
                both != 1 && block != lost
            },
            vertical: width(v),
            horizontal: width(h),
        }
    };
    let (start, end) = if horizontal {
        (
            [at(-2), at(0), at(-(per_row + 1)), at(-1)],
            [at(0), at(2), at(-(per_row - 1)), at(1)],
        )
    } else {
        (
            [at(-1), at(1), at(-per_row), at(0)],
            [at(per_row - 1), at(per_row + 1), at(0), at(per_row)],
        )
    };
    (joint(start, true), joint(end, false))
}

/// Which of two edges draws at a joint (Blink's `CompareForPaint`): 1 for
/// `lhs`, -1 for `rhs`, 0 for neither. One that draws over one that
/// doesn't; then the wider; then the higher style; then the lower box
/// order.
fn compare_for_paint(borders: &CollapsedBorders, lhs: Option<usize>, rhs: Option<usize>) -> i8 {
    if lhs == rhs {
        return 0;
    }
    let get = |i: Option<usize>| i.map(|i| borders.edges[i]).filter(|e| e.can_paint());
    match (get(lhs), get(rhs)) {
        (Some(l), Some(r)) => {
            if l.width != r.width {
                return if l.width > r.width { 1 } else { -1 };
            }
            if l.style == r.style {
                return match l.box_order.cmp(&r.box_order) {
                    std::cmp::Ordering::Less => 1,
                    std::cmp::Ordering::Greater => -1,
                    std::cmp::Ordering::Equal => 0,
                };
            }
            if style_rank(l.style) > style_rank(r.style) {
                1
            } else {
                -1
            }
        }
        (None, None) => 0,
        (None, Some(_)) => -1,
        (Some(_), None) => 1,
    }
}

/// A border style's rank against another (Blink's `EBorderStyle` order):
/// `double` over `solid`, `dashed`, `dotted`, `ridge`, `outset`, `groove`,
/// `inset`.
fn style_rank(style: BorderStyle) -> u8 {
    match style {
        BorderStyle::None => 0,
        BorderStyle::Hidden => 1,
        BorderStyle::Inset => 2,
        BorderStyle::Groove => 3,
        BorderStyle::Outset => 4,
        BorderStyle::Ridge => 5,
        BorderStyle::Dotted => 6,
        BorderStyle::Dashed => 7,
        BorderStyle::Solid => 8,
        BorderStyle::Double => 9,
    }
}

/// A table fragment's grid, its border box without its captions, from its
/// border box's corner: Blink puts the table's background and border
/// there.
pub(crate) fn table_grid(fragment: &BoxFragment) -> RectF {
    let (mut top, mut bottom) = (0.0, 0.0);
    for child in &fragment.children {
        let caption = &child.fragment;
        if table_part(caption) != Some(TableDisplay::Caption) || caption.style.is_out_of_flow() {
            continue;
        }
        let height = caption.size.height + caption.margin.top + caption.margin.bottom;
        match caption.style.caption_side {
            CaptionSide::Top => top += height,
            CaptionSide::Bottom => bottom += height,
        }
    }
    RectF::new(
        0.0,
        top,
        fragment.size.width,
        (fragment.size.height - top - bottom).max(0.0),
    )
}

/// The border boxes of the cells in row group or row `fragment`, whose
/// border box is at `origin` (inside their border if their backgrounds
/// stay there), but those hidden as empty or by their `visibility` (in
/// which Blink paints no table part's background).
fn table_cells(fragment: &BoxFragment, origin: PointF, out: &mut Vec<RectF>) {
    for child in &fragment.children {
        let at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
        let part = &child.fragment;
        match table_part(part) {
            Some(TableDisplay::Row) => table_cells(part, at, out),
            Some(TableDisplay::Cell) if !hides_empty_cell(part) && part.style.is_visible() => {
                let rect = RectF::new(at.x, at.y, part.size.width, part.size.height);
                out.push(if part.background_inside_border {
                    inside_border(rect, part.border)
                } else {
                    rect
                });
            }
            _ => {}
        }
    }
}

/// A box's border box and the widths in it: where its background boxes
/// are.
struct BoxGeometry {
    border_box: RectF,
    border: Sides<f32>,
    padding: Sides<f32>,
    /// `border-radius`, resolved against the border box.
    radii: [SizeF; 4],
}

impl BoxGeometry {
    /// The border, padding or content box (`background-clip`,
    /// `background-origin`), snapped to device pixels as the border is
    /// painted: the border box's edges, less the border widths snapped as
    /// border widths, so the background meets the border with no gap;
    /// with the corners' curves inside the border (css-backgrounds 3,
    /// "corner shaping").
    fn rrect(&self, which: BackgroundBox, dsf: f32) -> RRectF {
        let outer = RRectF::new(snap(self.border_box, dsf), self.radii);
        let w = |width: f32| snap_border_width(width, dsf);
        let padding_box = || {
            outer.inset(
                w(self.border.top),
                w(self.border.right),
                w(self.border.bottom),
                w(self.border.left),
            )
        };
        match which {
            BackgroundBox::BorderBox => outer,
            BackgroundBox::PaddingBox => padding_box(),
            BackgroundBox::ContentBox => {
                let p = &self.padding;
                let content = padding_box().inset(p.top, p.right, p.bottom, p.left);
                RRectF {
                    rect: snap(content.rect, dsf),
                    radii: content.radii,
                }
            }
        }
    }

    fn rect(&self, which: BackgroundBox, dsf: f32) -> RectF {
        self.rrect(which, dsf).rect
    }

    /// Whether the border hides the edge of a background clipped to the
    /// border box: its corners are rounded, and every side is there, drawn
    /// solid and opaque (Blink's `BorderObscuresBackgroundEdge`, which
    /// picks `kBackgroundBleedShrinkBackground`).
    fn border_obscures_background(&self, style: &ComputedStyle, dsf: f32) -> bool {
        let w = |width: f32| snap_border_width(width, dsf);
        let b = &self.border;
        let sides = [
            (b.top, style.border_style.top, style.border_color.top),
            (b.right, style.border_style.right, style.border_color.right),
            (
                b.bottom,
                style.border_style.bottom,
                style.border_color.bottom,
            ),
            (b.left, style.border_style.left, style.border_color.left),
        ];
        !RRectF::new(snap(self.border_box, dsf), self.radii).is_rect()
            && sides.iter().all(|&(width, border_style, color)| {
                w(width) > 0.0 && border_style.is_filled() && style.resolve(color).a == 255
            })
    }

    /// The border box less half the border: where a background clipped to
    /// it stops when the border hides its edge. Else the border's
    /// anti-aliased outer curve lets the background's, drawn on the same
    /// pixels, show through (Blink's
    /// `BackgroundRoundedRectAdjustedForBleedAvoidance`).
    fn shrunk_border_box(&self, dsf: f32) -> RRectF {
        let w = |width: f32| snap_border_width(width, dsf) / 2.0;
        let b = &self.border;
        self.rrect(BackgroundBox::BorderBox, dsf).inset(
            w(b.top),
            w(b.right),
            w(b.bottom),
            w(b.left),
        )
    }
}

/// `style`'s `border-radius` for a border box of `size`: each corner's
/// radii, a percentage of the box's width or height (before they are made
/// to fit, which `RRectF::new` does).
pub(crate) fn border_radii(style: &ComputedStyle, size: SizeF) -> [SizeF; 4] {
    let resolve = |v: computed_style::LengthPercentage, basis: f32| v.resolve(basis);
    let r = &style.rare.border_radius;
    [r.top_left, r.top_right, r.bottom_right, r.bottom_left]
        .map(|c| SizeF::new(resolve(c.x, size.width), resolve(c.y, size.height)))
}

/// `fragment`'s border box at `offset`, snapped, with its corners.
pub(crate) fn border_rrect(fragment: &BoxFragment, offset: PointF, dsf: f32) -> RRectF {
    let rect = RectF::new(
        offset.x,
        offset.y,
        fragment.size.width,
        fragment.size.height,
    );
    RRectF::new(
        snap(rect, dsf),
        border_radii(&fragment.style, fragment.size),
    )
}

/// One side of a border (or an outline's line) as drawn: its width,
/// snapped, its style and its color, resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
struct BorderSide {
    width: f32,
    style: BorderStyle,
    color: Color,
}

impl BorderSide {
    fn is_visible(&self) -> bool {
        self.width > 0.0 && self.style.is_drawn() && self.color.a != 0
    }
}

/// How one side is drawn.
#[derive(Debug, Clone, PartialEq)]
enum SideLook {
    /// Filled bands, each from and to a fraction of the side's width in
    /// from the outer edge, with its color.
    Bands(Vec<(f32, f32, Color)>),
    /// `dotted` or `dashed`.
    Dashes(BorderStyle),
}

impl SideLook {
    /// Side `i` (top, right, bottom, left) as `side`'s style draws it, at
    /// `dsf` device pixels per CSS px.
    fn of(side: BorderSide, i: usize, dsf: f32) -> Self {
        use BorderStyle::*;
        // Lit from the top left: `inset` darkens the top and left sides,
        // `outset` the others (Blink's `CalculateBorderStyleColor`).
        let top_left = i == 0 || i == 3;
        let shaded = |inset: bool| shade(side.color, top_left == inset);
        let bands = |bands: &[(f32, f32, Color)]| Self::Bands(bands.to_vec());
        match side.style {
            Dotted | Dashed => Self::Dashes(side.style),
            // Two lines of a third each, in whole device pixels (Blink's
            // `BorderEdge::GetDoubleBorderStripeWidths`); too thin for
            // that, one.
            Double => {
                // However wide: no product to overflow (`d * 2 / 3` is
                // `d - ceil(d / 3)`), and the width saturates.
                let d: i64 = base::numerics::clamp_round(side.width * dsf);
                if d < 3 {
                    return bands(&[(0.0, 1.0, side.color)]);
                }
                let third = d / 3;
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "d >= 3: a third of it, plus one, or it less its third (and one), \
                              stays between 0 and d"
                )]
                let (outer, inner) = (
                    third + i64::from(d % 3 == 2),
                    d - third - i64::from(d % 3 != 0) + i64::from(d % 3 == 1),
                );
                let d = d as f32;
                bands(&[
                    (0.0, outer as f32 / d, side.color),
                    (inner as f32 / d, 1.0, side.color),
                ])
            }
            // Halves: `groove`'s outer one as `inset`, its inner one as
            // `outset`; `ridge` the other way round.
            Groove | Ridge => {
                let groove = side.style == Groove;
                bands(&[(0.0, 0.5, shaded(groove)), (0.5, 1.0, shaded(!groove))])
            }
            Inset => bands(&[(0.0, 1.0, shaded(true))]),
            Outset => bands(&[(0.0, 1.0, shaded(false))]),
            None | Hidden | Solid => bands(&[(0.0, 1.0, side.color)]),
        }
    }
}

/// `color` darkened, or lightened, for a shaded side, unless it already
/// is about as dark (or light) as can be (Blink's
/// `CalculateBorderStyleColor` with `Color::Dark` and `Color::Light`).
fn shade(color: Color, darken: bool) -> Color {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "differences of bytes, squared and summed: at most 3 × 255²"
    )]
    let distance = |c: Color, v: u8| {
        [c.r, c.g, c.b]
            .iter()
            .map(|&x| (i32::from(x) - i32::from(v)).pow(2))
            .sum::<i32>()
    };
    let gray = |v: u8| Color::rgba(v, v, v, color.a);
    let v = f32::from(color.r.max(color.g).max(color.b)) / 255.0;
    let scale = |multiplier: f32| {
        let channel = |x: u8| {
            saturated_cast::<u8, _>((f32::from(x) / 255.0 * multiplier * 255.999).min(255.0))
        };
        Color::rgba(
            channel(color.r),
            channel(color.g),
            channel(color.b),
            color.a,
        )
    };
    if darken {
        // White comes to rgb(171, 171, 171), which Blink writes out.
        if distance(color, 0) <= 3 * 0x20 * 0x20 {
            color
        } else {
            scale(((v - 0.33) / v).max(0.0))
        }
    } else if distance(color, 255) <= 3 * (255 - 0xEB) * (255 - 0xEB) {
        color
    } else if v == 0.0 {
        gray(0x54)
    } else {
        scale((v + 0.33).min(1.0) / v)
    }
}

/// The trapezoid of each side (top, right, bottom, left) of the ring
/// between `outer` and `inner`, split at the corners' diagonals.
fn side_polygons(outer: RectF, inner: RectF) -> [Vec<PointF>; 4] {
    let (x0, y0, x1, y1) = (outer.x, outer.y, outer.right(), outer.bottom());
    let (ix0, iy0, ix1, iy1) = (inner.x, inner.y, inner.right(), inner.bottom());
    let p = PointF::new;
    [
        vec![p(x0, y0), p(x1, y0), p(ix1, iy0), p(ix0, iy0)],
        vec![p(x1, y0), p(x1, y1), p(ix1, iy1), p(ix1, iy0)],
        vec![p(x1, y1), p(x0, y1), p(ix0, iy1), p(ix1, iy1)],
        vec![p(x0, y1), p(x0, y0), p(ix0, iy0), p(ix0, iy1)],
    ]
}

/// The ring between `outer` and `inner` as one path: `inner`
/// counter-clockwise, a hole under non-zero winding.
fn ring_polygons(outer: RectF, inner: RectF) -> Vec<Vec<PointF>> {
    let p = PointF::new;
    let (x0, y0, x1, y1) = (outer.x, outer.y, outer.right(), outer.bottom());
    let (ix0, iy0, ix1, iy1) = (inner.x, inner.y, inner.right(), inner.bottom());
    vec![
        vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)],
        vec![p(ix0, iy0), p(ix0, iy1), p(ix1, iy1), p(ix1, iy0)],
    ]
}

/// About how long the path round `rrect` is: its straight edges and its
/// corners' quarter ellipses (Ramanujan's approximation).
fn rrect_perimeter(rrect: &RRectF) -> f32 {
    let r = rrect.rect;
    let [tl, tr, br, bl] = rrect.radii;
    let straight = (r.width - tl.width - tr.width)
        + (r.width - bl.width - br.width)
        + (r.height - tl.height - bl.height)
        + (r.height - tr.height - br.height);
    let quarter = |c: SizeF| {
        let (a, b) = (c.width, c.height);
        std::f32::consts::PI / 4.0 * (3.0 * (a + b) - ((3.0 * a + b) * (a + 3.0 * b)).sqrt())
    };
    straight + [tl, tr, br, bl].into_iter().map(quarter).sum::<f32>()
}

/// The dashes of a `dotted` or `dashed` line `width` wide and `length`
/// long (round a `closed` path): Blink's lengths, a dash twice the width
/// and a gap of one (three and two below 3px), the gap fitted so the line
/// starts and ends with a dash; dots (dashes of length 0, with round caps)
/// as wide as the line about two widths apart, the first and last at the
/// ends. `None` where there is no room for that: a solid line.
fn dash_for(style: BorderStyle, width: f32, length: f32, closed: bool) -> Option<Dash> {
    match style {
        BorderStyle::Dashed => {
            let (dash, gap) = if width >= 3.0 {
                (2.0 * width, width)
            } else {
                (3.0 * width, 2.0 * width)
            };
            if length < 2.0 * dash + gap {
                return None;
            }
            Dash::new(&[dash, best_dash_gap(length, dash, gap, closed)], 0.0)
        }
        BorderStyle::Dotted => {
            let span = if closed { length } else { length - width };
            if span <= 0.0 {
                return None;
            }
            let gaps = (span / (2.0 * width)).round().max(1.0);
            // A hair short, so the last dot isn't lost to rounding.
            Dash::new(&[0.0, span / gaps - 1e-3], 0.0)
        }
        _ => None,
    }
}

/// Blink's `StrokeData::SelectBestDashGap`: of the whole numbers of dashes
/// round the given gap, the one whose gap is closest to it.
fn best_dash_gap(length: f32, dash: f32, gap: f32, closed: bool) -> f32 {
    let min_dashes = ((length + gap) / (dash + gap)).floor();
    let max_dashes = min_dashes + 1.0;
    let gaps = |dashes: f32| if closed { dashes } else { dashes - 1.0 };
    if gaps(min_dashes) <= 0.0 {
        return gap;
    }
    let min_gap = (length - min_dashes * dash) / gaps(min_dashes);
    let max_gap = (length - max_dashes * dash) / gaps(max_dashes);
    if max_gap <= 0.0 || (min_gap - gap).abs() < (max_gap - gap).abs() {
        min_gap
    } else {
        max_gap
    }
}

/// The focus ring's width (Chrome's for `outline: auto 1px`): 2px of the
/// outline color and 1px of white.
const FOCUS_RING_WIDTH: f32 = 3.0;

/// Rounds each edge to the nearest device pixel (`dsf` of them per CSS px),
/// like Blink's `PixelSnappedIntRect`, so adjacent boxes share an edge
/// instead of blending at it. Layout stays in CSS px; Blink lays out in
/// device pixels (zoom for DSF) and snaps there, which comes to the same
/// pixels.
/// Whether `fragment`'s children are painted whole, each where it comes
/// in the line phase: a flex or grid container's items (Blink's
/// `IsPaintedAtomically`).
pub(crate) fn paints_children_whole(fragment: &BoxFragment) -> bool {
    fragment.style.display.is_flexible_or_grid()
}

/// What a line paints, in the order it paints it.
pub(crate) enum LinePart<'a> {
    /// An inline element's background and border.
    InlineBox(&'a InlineBoxFragment),
    /// An atomic inline, whole: its index in the block's children.
    Atomic(usize),
    Run(&'a TextFragment),
}

/// `line`'s parts in tree order (CSS 2 Appendix E step 7.2.1): before
/// each run, the inline elements that start there and the atomic inlines,
/// each atomic inline after the elements before it. An atomic inline
/// `text-overflow` hides isn't a part.
pub(crate) fn line_parts(line: &LineFragment) -> Vec<LinePart<'_>> {
    let mut parts = Vec::new();
    // A line a line clamp hides has nothing to paint or hit.
    if line.hidden {
        return parts;
    }
    let (mut boxes, mut atomics) = (0, 0);
    for index in 0..=line.runs.len() {
        loop {
            if let Some(atomic) = line
                .atomic_inlines
                .get(atomics)
                .filter(|a| a.paint_before <= index && a.after_boxes <= boxes)
            {
                if !atomic.hidden {
                    parts.push(LinePart::Atomic(atomic.child));
                }
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "atomics < atomic_inlines.len()"
                )]
                {
                    atomics += 1;
                }
            } else if let Some(inline_box) =
                line.boxes.get(boxes).filter(|b| b.paint_before <= index)
            {
                parts.push(LinePart::InlineBox(inline_box));
                #[expect(clippy::arithmetic_side_effects, reason = "boxes < line.boxes.len()")]
                {
                    boxes += 1;
                }
            } else {
                break;
            }
        }
        if let Some(run) = line.runs.get(index) {
            parts.push(LinePart::Run(run));
        }
    }
    parts
}

/// Whether `fragment`'s children are atomic inlines, each painted whole
/// where its lines place it, not in the order of the children.
pub(crate) fn children_on_lines(fragment: &BoxFragment) -> bool {
    !fragment.lines.is_empty() && !fragment.children.is_empty()
}

/// The child stacking contexts and positioned boxes of the one rooted at
/// `root`, each with the nearest element around it (`around` above
/// `root`): in tree order (flex items in `order`-modified order, and what
/// is in them too), then sorted by `z-index`, keeping tree order among
/// equal ones. A positioned box that isn't a stacking context (Blink's
/// stacked, non-stacking-context `PaintLayer`) has none: the ones in it
/// are its stacking context's, after it. An out-of-flow box is where its
/// parent places it, not where its containing block holds it, so it is
/// the stacking context's it is in, if its containing block is outside
/// (Blink's `PaintLayer` tree follows the layout tree). The `LayoutView`
/// roots the outermost stacking context.
pub(crate) fn stacking_children(
    root: &BoxFragment,
    around: Option<NodeId>,
) -> Vec<(&Arc<BoxFragment>, Option<NodeId>)> {
    fn collect<'f>(
        fragment: &'f BoxFragment,
        around: Option<NodeId>,
        out: &mut Vec<(&'f Arc<BoxFragment>, Option<NodeId>)>,
    ) {
        let node = fragment.node.or(around);
        let mut take = |child: &'f Arc<BoxFragment>| {
            if child.style.is_stacked() {
                out.push((child, node));
            }
            if !child.style.is_stacking_context() {
                collect(child, node, out);
            }
        };
        let mut places = fragment.out_of_flow.iter().peekable();
        for (i, child) in fragment.children.iter().enumerate() {
            while let Some(place) = places.next_if(|place| place.before == i) {
                take(&place.fragment);
            }
            // Where its parent places it, above.
            if !child.fragment.style.is_out_of_flow() {
                take(&child.fragment);
            }
        }
        places.for_each(|place| take(&place.fragment));
    }
    let mut out = Vec::new();
    if root.role == BoxRole::LayoutView || root.style.is_stacking_context() {
        collect(root, around, &mut out);
    }
    // A stable sort.
    out.sort_by_key(|(child, _)| child.style.stack_level());
    out
}

pub(crate) fn snap(rect: RectF, dsf: f32) -> RectF {
    let (x0, y0) = (snap_to(rect.x, dsf), snap_to(rect.y, dsf));
    let (x1, y1) = (
        snap_to(rect.x + rect.width, dsf),
        snap_to(rect.y + rect.height, dsf),
    );
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

/// `v` (CSS px) at the nearest device pixel.
fn snap_to(v: f32, dsf: f32) -> f32 {
    (v * dsf).round() / dsf
}

/// css-values' "snap as a border width": whole device pixels, rounded
/// down, but at least one if there is any width.
fn snap_border_width(width: f32, dsf: f32) -> f32 {
    if width <= 0.0 {
        0.0
    } else {
        (width * dsf).floor().max(1.0) / dsf
    }
}

/// `PaintChunksToCcLayer`: every chunk into one display list, at the root:
/// the document, unlayered, played whole (an SVG image's record).
pub fn to_display_list(artifact: &PaintArtifact, trees: &PropertyTrees) -> DisplayItemList {
    chunks_to_display_list(
        artifact,
        trees,
        &artifact.chunks,
        LayerSpace::ROOT,
        DisplayItemList::record(),
    )
}

/// The space a layer's content is drawn in: a transform node, the clips the
/// compositor applies to the whole layer (that node's and those around it),
/// and where in the node's space the layer's origin is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LayerSpace {
    pub(crate) transform: TransformNodeId,
    pub(crate) clip: ClipNodeId,
    pub(crate) origin: PointF,
}

impl LayerSpace {
    /// Each transform node's matrix into the layer's space, its origin at
    /// `origin`.
    pub(crate) fn to_layer(self, trees: &PropertyTrees) -> impl Fn(TransformNodeId) -> Transform {
        let to_root = trees.to_root();
        let from_root = to_root[self.transform.0]
            .inverse()
            .expect("a layer's space is a translation of the root's");
        let origin = Transform::translate(-self.origin.x, -self.origin.y).then(&from_root);
        move |transform: TransformNodeId| origin.then(&to_root[transform.0])
    }
}

impl LayerSpace {
    const ROOT: Self = Self {
        transform: TransformNodeId(0),
        clip: ClipNodeId(0),
        origin: PointF::new(0.0, 0.0),
    };
}

/// `PaintChunksToCcLayer`: `chunks` into `list` (empty: a layer's, or a
/// record's), in `space`.
///
/// Effects are groups: an effect's `SaveLayerAlpha` stays open across all
/// consecutive chunks under it, so overlapping content inside is composited
/// once (css-color's group opacity). Inside, each chunk that isn't at the
/// layer's own state gets its clips and matrix (all relative to the layer,
/// via `SetMatrix`) between a `Save` and a `Restore`. The clips `space`
/// already has are the compositor's, so they are left out.
pub(crate) fn chunks_to_display_list(
    artifact: &PaintArtifact,
    trees: &PropertyTrees,
    chunks: &[PaintChunk],
    space: LayerSpace,
    mut list: DisplayItemList,
) -> DisplayItemList {
    let to_layer = space.to_layer(trees);
    let outside = trees.clip_chain(space.clip).len();
    // The clips `clips` (inside the layer's), each in its space.
    let push_clips = |list: &mut DisplayItemList, clips: &[ClipNodeId]| {
        for clip in clips {
            let node = &trees.clip[clip.0];
            list.push(
                PaintOp::SetMatrix {
                    transform: to_layer(node.transform),
                },
                None,
            );
            let rrect = node.rrect().expect("only the root clip has no rect");
            let op = if rrect.is_rect() {
                PaintOp::ClipRect {
                    rect: rrect.rect,
                    anti_alias: true,
                }
            } else {
                PaintOp::ClipRRect {
                    rrect,
                    anti_alias: true,
                }
            };
            list.push(op, None);
            if let Some(paths) = &node.paths {
                let op = match &paths[..] {
                    [path] => PaintOp::ClipPath {
                        path: path.clone(),
                        anti_alias: true,
                    },
                    paths => PaintOp::ClipPaths {
                        paths: paths.to_vec(),
                        anti_alias: true,
                    },
                };
                list.push(op, None);
            }
        }
    };
    // The effects open, each with how many restores close it.
    let mut open_effects: Vec<(EffectNodeId, usize)> = Vec::new();
    for chunk in chunks {
        let state = chunk.state;
        let effects = trees.effect_chain(state.effect);
        let common = open_effects
            .iter()
            .zip(&effects)
            .take_while(|((a, _), b)| a == *b)
            .count();
        while open_effects.len() > common {
            let (_, restores) = open_effects.pop().expect("more than in common");
            for _ in 0..restores {
                list.push(PaintOp::Restore, None);
            }
        }
        for &effect in &effects[common..] {
            let node = &trees.effect[effect.0];
            let Some(filter) = &node.filter else {
                list.push(
                    PaintOp::SaveLayerAlpha {
                        bounds: None,
                        alpha: node.opacity,
                    },
                    None,
                );
                open_effects.push((effect, 1));
                continue;
            };
            // What the filter makes cut by its output clip, the filter in
            // its space (Skia's layer filters go by the matrix as the layer
            // is saved): cc's `SaveLayerOp` with the effect's `PaintFilter`,
            // in `ConversionContext::StartEffect`.
            let output_clips = trees.clip_chain(node.output_clip);
            debug_assert_eq!(
                output_clips[..outside],
                trees.clip_chain(space.clip)[..],
                "an effect is under its layer's clips"
            );
            list.push(PaintOp::Save, None);
            push_clips(&mut list, &output_clips[outside..]);
            list.push(
                PaintOp::SetMatrix {
                    transform: to_layer(node.transform),
                },
                None,
            );
            list.push(
                PaintOp::SaveLayer {
                    bounds: None,
                    flags: compositor_paint::LayerFlags {
                        alpha: node.opacity,
                        image_filter: Some(filter.clone()),
                        ..compositor_paint::LayerFlags::default()
                    },
                },
                None,
            );
            open_effects.push((effect, 2));
        }

        let clips = trees.clip_chain(state.clip);
        debug_assert_eq!(
            clips[..outside],
            trees.clip_chain(space.clip)[..],
            "a chunk is under its layer's clips"
        );
        let clips = &clips[outside..];
        let matrix = to_layer(state.transform);
        let transformed = !matrix.is_identity() || !clips.is_empty();
        if transformed {
            list.push(PaintOp::Save, None);
            push_clips(&mut list, clips);
            list.push(PaintOp::SetMatrix { transform: matrix }, None);
        }
        for item in &artifact.items[chunk.items.clone()] {
            list.push(item.op.clone(), Some(matrix.map_rect(item.visual_rect)));
        }
        if transformed {
            list.push(PaintOp::Restore, None);
        }
    }
    for (_, restores) in open_effects {
        for _ in 0..restores {
            list.push(PaintOp::Restore, None);
        }
    }
    list.finalize();
    list
}

/// `clip` stretched along an axis whose `overflow` is `visible` (the other
/// one is `clip`): it cuts only the other (Blink's `OverflowClipRect`
/// along one axis).
fn one_axis(clip: RectF, style: &ComputedStyle) -> RectF {
    const FAR: f32 = 1.0e7;
    // Paint containment cuts both, whatever `overflow` is.
    let both = layout::paint_contained(style);
    let (cut_x, cut_y) = (
        both || style.overflow_x != Overflow::Visible,
        both || style.overflow_y != Overflow::Visible,
    );
    RectF::new(
        if cut_x { clip.x } else { -FAR },
        if cut_y { clip.y } else { -FAR },
        if cut_x { clip.width } else { 2.0 * FAR },
        if cut_y { clip.height } else { 2.0 * FAR },
    )
}

/// The part of a run of text that is selected (`Painter::selected`): its
/// box, and the colors drawn there.
struct Selected {
    rect: RectF,
    background: Color,
    text: Color,
}

/// `op` (a run's glyphs, within `bounds`) drawn only from `left` to
/// `right`: the part of a run on one side of its selection.
/// The rect a caret at `at` is drawn in, in `fragment`'s own space,
/// unsnapped: 1 CSS px wide, as tall as the font there (`Painter::
/// caret`'s); `None` if it isn't on `fragment`'s lines.
pub fn caret_rect(fragment: &BoxFragment, at: CaretPosition) -> Option<RectF> {
    let (x, top, bottom, _) = caret_place(fragment, at)?;
    Some(RectF::new(x, top, 1.0, bottom - top))
}

/// Where in `fragment` (its own space) a caret at `at` is: its x, the
/// top and bottom of the font there, and the style it is drawn in; `None`
/// if it isn't on `fragment`'s lines. Downstream: at a soft wrap, at the
/// start of the next line; after a newline kept at a line's end, on the
/// line after.
fn caret_place(
    fragment: &BoxFragment,
    at: CaretPosition,
) -> Option<(f32, f32, f32, &Arc<ComputedStyle>)> {
    let font = |run: &TextFragment| (run.baseline - run.ascent, run.baseline + run.descent);
    match at {
        CaretPosition::Text { text, offset } => {
            // The run that ends at it, if none has it.
            let mut ending = None;
            for (index, line) in fragment.lines.iter().enumerate() {
                if line.hidden {
                    continue;
                }
                for run in &line.runs {
                    let Some((node, source)) = &run.source else {
                        continue;
                    };
                    if *node != text {
                        continue;
                    }
                    if source.start <= offset && offset < source.end {
                        let (top, bottom) = font(run);
                        return Some((run.x_range(offset, offset).0, top, bottom, &run.style));
                    }
                    if offset == source.end {
                        ending = Some((index, run));
                    }
                }
            }
            let (index, run) = ending?;
            let after_newline = fragment.lines[index].end.as_ref().is_some_and(|end| {
                end.kind()
                    == LineEndKind::Newline {
                        text,
                        offset: offset.wrapping_sub(1),
                    }
            });
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "index < fragment.lines.len()"
            )]
            let next = index + 1;
            match fragment.lines.get(next).filter(|_| after_newline) {
                // That line's font, centered in it as on its line.
                Some(next) => {
                    let height = run.ascent + run.descent;
                    let top = next.rect.y + (next.rect.height - height) / 2.0;
                    Some((next.rect.x, top, top + height, &run.style))
                }
                None => {
                    let (top, bottom) = font(run);
                    Some((run.x_range(offset, offset).0, top, bottom, &run.style))
                }
            }
        }
        CaretPosition::Break(br) => {
            let end = fragment
                .lines
                .iter()
                .filter(|line| !line.hidden)
                .find_map(|line| line.end.as_ref().filter(|end| end.br() == Some(br)))?;
            let top = end.baseline - end.ascent;
            Some((end.x(), top, end.baseline + end.descent, &end.style))
        }
        CaretPosition::Empty {
            element,
            ascent,
            descent,
            line_height,
        } => {
            if fragment.node != Some(element) || !fragment.lines.is_empty() {
                return None;
            }
            let style = &fragment.style;
            let (b, p) = (&fragment.border, &fragment.padding);
            let left = b.left + p.left;
            let width = fragment.size.width - left - b.right - p.right;
            let indent = style.rare_inherited.text_indent.resolve(width);
            // Its right edge, inside the content box, at the end.
            let x = match style.text_align {
                computed_style::TextAlign::Start | computed_style::TextAlign::Left => left + indent,
                computed_style::TextAlign::Center => left + width / 2.0,
                computed_style::TextAlign::End | computed_style::TextAlign::Right => {
                    left + width - 1.0
                }
            };
            let height = ascent + descent;
            let top = b.top + p.top + (line_height - height) / 2.0;
            Some((x, top, top + height, style))
        }
    }
}

fn clipped(op: &PaintOp, left: f32, right: f32, bounds: RectF) -> PaintOp {
    let (top, bottom) = (bounds.y, bounds.bottom());
    PaintOp::DrawClipped {
        polygon: Arc::from([
            PointF::new(left, top),
            PointF::new(right, top),
            PointF::new(right, bottom),
            PointF::new(left, bottom),
        ]),
        op: Box::new(op.clone()),
    }
}

/// `start..end` counted from `from`, where a stacking context's items (or
/// targets) begin.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "start and end are at or after `from`: items are only appended"
)]
fn relative(start: usize, end: usize, from: usize) -> Range<usize> {
    start - from..end - from
}

/// A glyph id as Skia takes it.
#[expect(
    clippy::cast_possible_truncation,
    reason = "an OpenType glyph id is 16 bits"
)]
fn glyph_id(id: u32) -> u16 {
    id as u16
}
