//! Hit testing (`PaintLayer::HitTest` and `BoxFragmentPainter::NodeAtPoint`):
//! what is at a point. It walks the fragment tree in the reverse of paint
//! order, so the first thing found is the one painted on top, and maps the
//! point through the same property trees paint used: transforms, clips and
//! scroll offsets.
//!
//! What the walk would find, over everything instead of up to a point,
//! paint records as it goes ([`crate::HitTestData`]): where the compositor
//! can't pick the scroller under a wheel by itself
//! ([`main_thread_scroll_hit_test_region`]) is made from that, without a
//! walk.

use computed_style::PointerEvents;
use gfx::{NodeId, NodeIdHashMap, NodeIdHashSet, PointF, RRectF, RectF, Transform};
use layout::{BoxFragment, BoxRole, SvgNode, SvgNodeKind, SvgTextCell};

use crate::layerize::{composited_scroller_of, composited_sticky_of};
use crate::svg::{shape_bounds, shape_hit, text_cell_hit};
use crate::{
    ClipNodeId, EffectNodeId, EmbeddedFrame, HitTestData, LinePart, PaintArtifact, PrePaint,
    PropertyTreeState, PropertyTrees, ScrollNode, ScrollNodeId, TransformNodeId, children_on_lines,
    composited_sticky, line_parts, paints_children_whole, stacking_children,
};

/// What [`hit_test`] found (`blink::HitTestResult`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HitTestResult {
    /// The innermost element at the point: the element a run of text is
    /// in, an inline element's box, or the nearest element around an
    /// anonymous block. `None` where only the canvas is.
    pub node: Option<NodeId>,
    /// The text node of the run of text hit, if one was: `node` is the
    /// element whose style the text has, its parent in the flat tree,
    /// which isn't its parent for a host's child assigned to a slot.
    pub text: Option<NodeId>,
    /// The innermost scroll container there (the box hit, if it is one):
    /// where a wheel starts scrolling. `None` if only the viewport.
    pub scroll: Option<ScrollNodeId>,
}

/// Where a wheel needs the main thread to pick the scroller (cc's
/// `main_thread_scroll_hit_test_region`, which Blink gathers from its scroll
/// hit test data), by what moves it when the compositor scrolls: the rects
/// in a container that is a layer move with its content.
///
/// The compositor picks the innermost container that is a layer under the
/// pointer by its border box, cut by the clips around it, and scrolls it and
/// those around it that are layers, then the viewport. That is what the main
/// thread would do, except where the region sends the wheel to it:
/// - over a scroll container that isn't a layer and that the user can
///   scroll, which the compositor can't;
/// - over a container that is a layer and that something painted after it,
///   outside it, may cover: there the main thread may find that instead.
///
/// A container that isn't a layer and that the user can't scroll is left
/// out: the main thread passes the whole wheel on from it, as the compositor
/// does by not knowing it. One that is a layer inside one the user can
/// scroll needs nothing of its own: the outer one's rect covers it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ScrollHitTestRegion {
    /// In the document's space: what no container that is a layer moves.
    pub root: Vec<RectF>,
    /// By scroll node, in its scroll translation's space: what it moves.
    /// Empty for a container that isn't a layer.
    pub scrollers: Vec<Vec<RectF>>,
    /// By sticky box's translation, in its space: what the compositor
    /// moves as the box sticks ([`crate::composited_sticky`]), in the
    /// order the translations are in.
    pub sticky: Vec<(TransformNodeId, Vec<RectF>)>,
}

impl ScrollHitTestRegion {
    fn new(trees: &PropertyTrees) -> Self {
        Self {
            root: Vec::new(),
            scrollers: vec![Vec::new(); trees.scroll.len()],
            sticky: Vec::new(),
        }
    }

    /// `rect`, in the root's space as `to_root` has it, into the region of
    /// what moves it.
    fn push(&mut self, trees: &PropertyTrees, to_root: &[Transform], mover: Mover, rect: RectF) {
        match mover {
            Mover::Document => self.root.push(rect),
            Mover::Scroller(scroller) => {
                let space = trees.scroll(scroller).scroll_translation;
                let origin = translation(&to_root[space.0]);
                self.scrollers[scroller.0].push(rect.offset(-origin.x, -origin.y));
            }
            Mover::Sticky(sticky) => {
                let origin = translation(&to_root[sticky.0]);
                let rect = rect.offset(-origin.x, -origin.y);
                match self
                    .sticky
                    .binary_search_by_key(&sticky.0, |(node, _)| node.0)
                {
                    Ok(index) => self.sticky[index].1.push(rect),
                    Err(index) => self.sticky.insert(index, (sticky, vec![rect])),
                }
            }
        }
    }
}

/// What moves a space as the compositor scrolls, besides the viewport:
/// nothing, a container that is a layer, or a sticky box it moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mover {
    Document,
    Scroller(ScrollNodeId),
    Sticky(TransformNodeId),
}

/// For each transform node, the innermost [`Mover`] of its space, with the
/// containers that are layers `composited`; and which nodes are the
/// translations of sticky boxes the compositor moves.
fn movers(trees: &PropertyTrees, composited: &[bool]) -> (Vec<Mover>, Vec<bool>) {
    let scroller_of = composited_scroller_of(trees, composited);
    let sticky_of = composited_sticky_of(trees, composited, &scroller_of);
    let of = scroller_of
        .iter()
        .zip(&sticky_of)
        .map(|(scroller, sticky)| match (sticky, scroller) {
            (Some(sticky), _) => Mover::Sticky(*sticky),
            (None, Some(scroller)) => Mover::Scroller(*scroller),
            (None, None) => Mover::Document,
        })
        .collect();
    (of, composited_sticky(trees, composited))
}

/// The region of the scroll containers `pre` has, `composited` (by scroll
/// node) being those that are layers, `artifact` painted from `pre`.
///
/// Each rect is a container's border box, made whole CSS px outward in its
/// own space first (its content is clipped to its padding box with each
/// edge rounded, which a scale would otherwise stretch past a border box
/// rounded only at the end), mapped to the space that moves it, cut by the
/// clips there (not by those outside: the compositor moves the space within
/// them, and cuts by them itself; not by the viewport: it scrolls the
/// viewport without painting again, and may bring a container into view),
/// made whole px outward again and grown by 1px for the rounding the two
/// threads' offsets may differ by. Larger only sends a wheel the compositor
/// could have handled to the main thread; smaller would have the compositor
/// scroll something else than the main thread would.
pub fn main_thread_scroll_hit_test_region(
    artifact: &PaintArtifact,
    pre: &PrePaint,
    composited: &[bool],
) -> ScrollHitTestRegion {
    let trees = &pre.trees;
    let to_root = trees.to_root();
    let scroller_of = composited_scroller_of(trees, composited);
    let (mover_of, sticky) = movers(trees, composited);
    let mut region = ScrollHitTestRegion::new(trees);
    // Only a composited container can be covered.
    let covered = if composited.contains(&true) {
        let (covered, over) = covered_scrollers(
            &artifact.hit_test,
            pre,
            composited,
            &to_root,
            &scroller_of,
            &sticky,
        );
        // What a sticky box may move over a container, or a container
        // moved by one under: where it is.
        for (chunk, rect) in over {
            let mover = mover_of[artifact.hit_test.chunks[chunk].state.transform.0];
            region.push(trees, &to_root, mover, enclosing(rect).inset(-1.0));
        }
        covered
    } else {
        vec![false; trees.scroll.len()]
    };
    for (id, node) in trees.scroll_nodes() {
        let index = id.0;
        let needs_main_thread = if composited[index] {
            covered[index]
        } else {
            user_can_scroll(node)
        };
        if !needs_main_thread {
            continue;
        }
        let moved_by = mover_of[node.transform.0];
        let mut rect = enclosing(to_root[node.transform.0].map_rect(enclosing(node.border_box)));
        for clip in trees.clip_chain(node.clip) {
            let clip = &trees.clip[clip.0];
            if let Some(clip_rect) = clip.rect
                && mover_of[clip.transform.0] == moved_by
            {
                rect = rect.intersect(&to_root[clip.transform.0].map_rect(clip_rect));
            }
        }
        if rect.is_empty() {
            continue;
        }
        region.push(trees, &to_root, moved_by, enclosing(rect).inset(-1.0));
    }
    region
}

/// The wheel event handler region (cc's `wheel_event_region`, which Blink
/// gathers from its hit test data): where a wheel waits for the main thread
/// because a listener there may cancel its scrolling, by what moves it when
/// the compositor scrolls, as [`main_thread_scroll_hit_test_region`]. Each
/// target `artifact` recorded inside a blocking `wheel` listener (its
/// style's `inside_blocking_wheel_event_handler`), its rect cut by the
/// clips that move with it, made whole px outward and grown by 1px, the
/// same way; with `whole_document` (the main frame's root element, document
/// or window has one), all of it.
pub fn wheel_event_handler_region(
    artifact: &PaintArtifact,
    pre: &PrePaint,
    composited: &[bool],
    whole_document: bool,
) -> ScrollHitTestRegion {
    let trees = &pre.trees;
    let mut region = ScrollHitTestRegion::new(trees);
    if whole_document {
        // Wherever the document is scrolled to.
        region.root.push(RectF::new(-1e7, -1e7, 2e7, 2e7));
        return region;
    }
    let data = &artifact.hit_test;
    if !data.chunks.iter().any(|chunk| chunk.wheel) {
        return region;
    }
    let to_root = trees.to_root();
    let (mover_of, _) = movers(trees, composited);
    for chunk in data.chunks.iter().filter(|chunk| chunk.wheel) {
        let state = chunk.state;
        let moved_by = mover_of[state.transform.0];
        // The clips that move with it, in the root's space.
        let clips: Vec<RectF> = trees
            .clip_chain(state.clip)
            .iter()
            .filter_map(|clip| {
                let clip = &trees.clip[clip.0];
                let rect = clip.rect?;
                (mover_of[clip.transform.0] == moved_by)
                    .then(|| to_root[clip.transform.0].map_rect(rect))
            })
            .collect();
        for &target in &data.rects[chunk.rects.clone()] {
            let mut rect = enclosing(to_root[state.transform.0].map_rect(target));
            for clip in &clips {
                rect = rect.intersect(clip);
            }
            if rect.is_empty() {
                continue;
            }
            region.push(trees, &to_root, moved_by, enclosing(rect).inset(-1.0));
        }
    }
    region
}

/// Which frame input at a point is for (cc's `FindFrameElementIdAtPoint`,
/// over the layers Blink marks with each frame's `visible_frame_element_id`),
/// as paint's hit test data has it rather than layers: what the compositor
/// tags the input it hands the main thread with, for `isInputPending()`.
///
/// Each child frame's canvas is a rect of its frame; and each target painted
/// after a child frame (not inside it) is a rect of the frame it is in, where
/// it may be over that child frame: cc gives up where what is on top is of a
/// frame the other things there aren't all ancestors of. The compositor
/// finds the rects at the point (in their spaces, as it scrolls them) and the
/// one painted last (the compositor's `PropertyTrees::frame_attribution`):
/// its frame, unless
/// something else there is of a frame that isn't it or one around it, or it
/// is a child frame under a rounded clip or a `clip-path` (cc's
/// `HitTestMayBeAffectedByMask`): then no frame is known. Nothing here is the
/// main frame's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameAttributionRegion {
    /// The child frames, each after the one it is in.
    pub frames: Vec<AttributedFrame>,
    /// In paint order.
    pub rects: Vec<AttributionRect>,
}

/// A child frame: its document, and the child frame it is in (an index in
/// [`FrameAttributionRegion::frames`]), `None` for the main frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributedFrame {
    pub document: NodeId,
    pub parent: Option<usize>,
}

/// Where something of a frame is, in the space that moves it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributionRect {
    pub rect: RectF,
    pub space: AttributionSpace,
    /// Its place in paint order: the one at a point painted last is on top.
    pub order: u32,
    /// Its frame (an index in [`FrameAttributionRegion::frames`]); `None`
    /// for the main frame's.
    pub frame: Option<usize>,
    /// A child frame's canvas under a rounded clip or a `clip-path`.
    pub masked: bool,
}

/// What moves an [`AttributionRect`] as the compositor scrolls, besides the
/// viewport: its space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionSpace {
    /// The document's.
    Document,
    /// The scroll translation's of a container that is a layer.
    Scroller(ScrollNodeId),
    /// A sticky box's translation's, which the compositor moves.
    Sticky(TransformNodeId),
}

/// The [`FrameAttributionRegion`] of `artifact`, painted from `pre`, with the
/// containers that are layers `composited`. Each rect is its target's, cut by
/// the clips that move with it, made whole px outward, in the space that
/// moves it. A target painted after a child frame is one only if it may be
/// over that frame: it overlaps it now, or something the compositor scrolls
/// moves one and not the other.
pub fn frame_attribution_region(
    artifact: &PaintArtifact,
    pre: &PrePaint,
    composited: &[bool],
) -> FrameAttributionRegion {
    let trees = &pre.trees;
    let mut region = FrameAttributionRegion::default();
    // Each scroll node's child frame, by the iframe the frame is in; the
    // frames by their viewports (the outermost node of each), parents
    // first, as the scroll nodes are.
    let mut frame_of_iframe: Vec<(NodeId, usize)> = Vec::new();
    for (_, node) in trees.scroll_nodes() {
        let Some(iframe) = node.frame else {
            continue;
        };
        let outer = node.parent.and_then(|p| trees.scroll(p).frame);
        if outer == Some(iframe) {
            continue;
        }
        let parent = outer.and_then(|o| frame_index(&frame_of_iframe, o));
        frame_of_iframe.push((iframe, region.frames.len()));
        region.frames.push(AttributedFrame {
            document: node.element,
            parent,
        });
    }
    if region.frames.is_empty() {
        return region;
    }
    let data = &artifact.hit_test;
    let to_root = trees.to_root();
    let (mover_of, _) = movers(trees, composited);
    let frame_of = |scroll: Option<ScrollNodeId>| {
        scroll
            .and_then(|s| trees.scroll(s).frame)
            .and_then(|iframe| frame_index(&frame_of_iframe, iframe))
    };
    // Whether `frame` is `of` or one around it.
    let around = |frame: Option<usize>, of: Option<usize>| {
        let mut at = of;
        loop {
            if at == frame {
                return true;
            }
            match at {
                Some(index) => at = region.frames[index].parent,
                None => return false,
            }
        }
    };
    // The child frames' canvases so far: their frame, root rect and mover.
    let mut canvases: Vec<(Option<usize>, RectF, Mover)> = Vec::new();
    let mut rects = Vec::new();
    for (order, chunk) in data.chunks.iter().enumerate() {
        let state = chunk.state;
        let frame = frame_of(chunk.scroll);
        // A child frame's canvas: the chunk of its viewport's own (the
        // outermost container of the frame).
        let canvas = chunk.container.is_some_and(|c| {
            let node = trees.scroll(c);
            node.frame.is_some() && node.parent.and_then(|p| trees.scroll(p).frame) != node.frame
        });
        if !canvas && canvases.is_empty() {
            continue;
        }
        let moved_by = mover_of[state.transform.0];
        let chain = trees.clip_chain(state.clip);
        let masked = canvas
            && chain.iter().any(|clip| {
                let clip = &trees.clip[clip.0];
                clip.is_rounded() || clip.paths.is_some()
            });
        // The clips that move with it, in the root's space.
        let clips: Vec<RectF> = chain
            .iter()
            .filter_map(|clip| {
                let clip = &trees.clip[clip.0];
                let rect = clip.rect?;
                (mover_of[clip.transform.0] == moved_by)
                    .then(|| to_root[clip.transform.0].map_rect(rect))
            })
            .collect();
        for &target in &data.rects[chunk.rects.clone()] {
            let mut rect = enclosing(to_root[state.transform.0].map_rect(target));
            for clip in &clips {
                rect = rect.intersect(clip);
            }
            if rect.is_empty() {
                continue;
            }
            if canvas {
                canvases.push((frame, rect, moved_by));
            } else if !canvases.iter().any(|&(under, at, mover)| {
                !around(under, frame) && (mover != moved_by || !at.intersect(&rect).is_empty())
            }) {
                continue;
            }
            let mut attributed = AttributionRect {
                rect: enclosing(rect),
                space: AttributionSpace::Document,
                order: base::numerics::saturated_cast(order),
                frame,
                masked,
            };
            place(&mut attributed, trees, &to_root, moved_by);
            rects.push(attributed);
        }
    }
    region.rects = rects;
    region
}

/// The index of the child frame in `iframe`.
fn frame_index(frames: &[(NodeId, usize)], iframe: NodeId) -> Option<usize> {
    frames.iter().find(|(i, _)| *i == iframe).map(|(_, f)| *f)
}

/// Moves `rect`, in the root's space, into the space of what moves it.
fn place(rect: &mut AttributionRect, trees: &PropertyTrees, to_root: &[Transform], mover: Mover) {
    let (space, origin) = match mover {
        Mover::Document => (AttributionSpace::Document, PointF::new(0.0, 0.0)),
        Mover::Scroller(scroller) => {
            let space = trees.scroll(scroller).scroll_translation;
            (
                AttributionSpace::Scroller(scroller),
                translation(&to_root[space.0]),
            )
        }
        Mover::Sticky(sticky) => (
            AttributionSpace::Sticky(sticky),
            translation(&to_root[sticky.0]),
        ),
    };
    rect.space = space;
    rect.rect = rect.rect.offset(-origin.x, -origin.y);
}

/// Whether a wheel can move `node`: on an axis the user may scroll, with
/// something to scroll.
fn user_can_scroll(node: &ScrollNode) -> bool {
    (node.user_scrollable_x && node.contents.width > node.container.width)
        || (node.user_scrollable_y && node.contents.height > node.container.height)
}

/// Where a space only moved by translations is, from the root's.
fn translation(to_root: &Transform) -> PointF {
    to_root
        .as_translation()
        .expect("a container that is a layer is only translated")
}

/// For each scroll node that is a layer, whether something hit testing can
/// find, painted after its border box and not in its scroll chain, may be
/// over it, wherever the compositor scrolls what it can.
///
/// Where each thing may be: its rect, cut by its clips, as painted; or, if
/// the compositor scrolls something around it that doesn't move the
/// container too, all of the outermost such container's border box (and
/// the same for the container, the other way). So only an overlap the
/// compositor's scrolling can't change is ruled out. A chunk of targets is
/// ruled out by its bounds first, and looked into only if they overlap.
///
/// Where a sticky box the compositor moves (`sticky`) moves one of the two
/// and not the other, the two may be anywhere their boxes may stick to; if
/// they may overlap, the targets' rects (by their chunk) are the second
/// part of what this gives: the region where they are, moving with them,
/// is the main thread's, rather than all of the container.
fn covered_scrollers(
    data: &HitTestData,
    pre: &PrePaint,
    composited: &[bool],
    to_root: &[Transform],
    scroller_of: &[Option<ScrollNodeId>],
    sticky: &[bool],
) -> (Vec<bool>, Vec<(usize, RectF)>) {
    let trees = &pre.trees;
    // The sticky boxes the compositor moves around a space.
    let sticky_around = |transform: TransformNodeId| {
        let mut out = Vec::new();
        let mut node = Some(transform);
        while let Some(n) = node {
            if sticky[n.0] {
                out.push(n);
            }
            node = trees.transform[n.0].parent;
        }
        out
    };
    // `rect` wherever the sticky boxes of `nodes` may move it, from where
    // they are now.
    let spread = |rect: RectF, nodes: &[TransformNodeId]| {
        nodes.iter().fold(rect, |rect, node| {
            let node = &trees.transform[node.0];
            let at = node.matrix.as_translation().unwrap_or_default();
            let reach = node.sticky.map(|s| s.reach).unwrap_or_default();
            RectF::new(
                rect.x - at.x + reach.x,
                rect.y - at.y + reach.y,
                rect.width + reach.width,
                rect.height + reach.height,
            )
        })
    };
    let mut over: Vec<(usize, RectF)> = Vec::new();
    let mut over_chunks = vec![false; data.chunks.len()];
    // The containers that are layers moving a space, the innermost first.
    let movers = |mut transform: TransformNodeId| {
        let mut out = Vec::new();
        while let Some(scroller) = scroller_of[transform.0] {
            out.push(scroller);
            transform = trees.scroll(scroller).transform;
        }
        out
    };
    let clipped = |rect: RectF, state: PropertyTreeState| {
        let mut rect = to_root[state.transform.0].map_rect(rect);
        for clip in trees.clip_chain(state.clip) {
            let clip = &trees.clip[clip.0];
            if let Some(clip_rect) = clip.rect {
                rect = rect.intersect(&to_root[clip.transform.0].map_rect(clip_rect));
            }
        }
        rect
    };
    // Where a wheel picks a container: its border box, cut by its clips.
    let picked_at = |scroller: ScrollNodeId| {
        let node = &trees.scroll(scroller);
        clipped(
            node.border_box,
            PropertyTreeState {
                transform: node.transform,
                clip: node.clip,
                effect: node.effect,
            },
        )
    };
    // All of the outermost of `own` that doesn't move what it is compared
    // with (moved by `other`), if there is one.
    let anywhere_in = |own: &[ScrollNodeId], other: &[ScrollNodeId]| {
        own.iter()
            .rev()
            .find(|scroller| !other.contains(scroller))
            .map(|&outermost| picked_at(outermost))
    };
    let overlap = |a: RectF, b: RectF| !a.intersect(&b).is_empty();

    let covered = (0..trees.scroll.len())
        .map(|index| {
            if !composited[index] {
                return false;
            }
            let scroller = ScrollNodeId(index);
            let Some(own_box) = data
                .chunks
                .iter()
                .position(|chunk| chunk.container == Some(scroller))
            else {
                // No box to pick it by: let the main thread decide.
                return true;
            };
            let moves_it = movers(trees.scroll(scroller).transform);
            let sticks_it = sticky_around(trees.scroll(scroller).transform);
            let it = picked_at(scroller);
            // The chunks after its own box, by their index.
            data.chunks
                .iter()
                .enumerate()
                .skip(own_box)
                .skip(1)
                .any(|(index, chunk)| {
                    if in_scroll_chain(trees, chunk.scroll, scroller) {
                        return false;
                    }
                    let moves_target = movers(chunk.state.transform);
                    let it_at = anywhere_in(&moves_it, &moves_target).unwrap_or(it);
                    let sticks_target = sticky_around(chunk.state.transform);
                    let (only_it, only_target): (Vec<_>, Vec<_>) = (
                        sticks_it
                            .iter()
                            .copied()
                            .filter(|n| !sticks_target.contains(n))
                            .collect(),
                        sticks_target
                            .iter()
                            .copied()
                            .filter(|n| !sticks_it.contains(n))
                            .collect(),
                    );
                    if !only_it.is_empty() || !only_target.is_empty() {
                        let target_at =
                            anywhere_in(&moves_target, &moves_it).unwrap_or_else(|| {
                                to_root[chunk.state.transform.0].map_rect(chunk.bounds)
                            });
                        if !over_chunks[index]
                            && overlap(spread(target_at, &only_target), spread(it_at, &only_it))
                        {
                            over_chunks[index] = true;
                            let to = &to_root[chunk.state.transform.0];
                            over.extend(
                                data.rects[chunk.rects.clone()]
                                    .iter()
                                    .map(|&rect| (index, to.map_rect(rect))),
                            );
                        }
                        return false;
                    }
                    if let Some(target_at) = anywhere_in(&moves_target, &moves_it) {
                        return overlap(target_at, it_at);
                    }
                    let covers = |rect: RectF| overlap(clipped(rect, chunk.state), it_at);
                    let found = covers(chunk.bounds)
                        && data.rects[chunk.rects.clone()]
                            .iter()
                            .any(|&rect| covers(rect));
                    debug_assert_eq!(
                        found,
                        data.rects[chunk.rects.clone()]
                            .iter()
                            .any(|&rect| covers(rect)),
                        "the bounds rule out only what no rect in them covers"
                    );
                    found
                })
        })
        .collect();
    (covered, over)
}

/// Checks that paint recorded in `data` what the walk finds, in paint
/// order, each in its state and scroll container. The rects by their bits:
/// layout gives a run on an empty line no position (infinite) and so no
/// width (NaN).
#[cfg(debug_assertions)]
pub(crate) fn assert_recorded(layout_view: &BoxFragment, pre: &PrePaint, data: &HitTestData) {
    let bits = |r: RectF| [r.x, r.y, r.width, r.height].map(f32::to_bits);
    let mut walked = Vec::new();
    Walker {
        pre,
        pointer_events_none: false,
        visit: |target: &Target<'_>| {
            walked.push((
                bits(target.rect.rect),
                target.state,
                target.scroll,
                target.container,
                target.wheel,
            ));
            false
        },
    }
    .stacking_context(layout_view, None);
    walked.reverse();
    let recorded: Vec<_> = data
        .chunks
        .iter()
        .flat_map(|chunk| {
            data.rects[chunk.rects.clone()].iter().map(|&rect| {
                (
                    bits(rect),
                    chunk.state,
                    chunk.scroll,
                    chunk.container,
                    chunk.wheel,
                )
            })
        })
        .collect();
    assert!(
        walked == recorded,
        "paint recorded other targets than hit testing finds:\n{walked:?}\n{recorded:?}"
    );
}

/// Whether `scroller` is `from` or around it.
fn in_scroll_chain(
    trees: &PropertyTrees,
    mut from: Option<ScrollNodeId>,
    scroller: ScrollNodeId,
) -> bool {
    while let Some(node) = from {
        if node == scroller {
            return true;
        }
        from = trees.scroll(node).parent;
    }
    false
}

/// The smallest rect of whole px around `rect`.
fn enclosing(rect: RectF) -> RectF {
    let (x0, y0) = (rect.x.floor(), rect.y.floor());
    let (x1, y1) = (rect.right().ceil(), rect.bottom().ceil());
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

/// What is at `point`, in the root transform space: the document's
/// coordinates, so a point in the viewport needs the viewport's scroll
/// offset added first.
pub fn hit_test(layout_view: &BoxFragment, pre: &PrePaint, point: PointF) -> HitTestResult {
    let _span = trace_event::trace_event!("HitTest");
    let hits = hits_at(pre, point);
    Walker {
        pre,
        pointer_events_none: false,
        visit: hits,
    }
    .stacking_context(layout_view, None)
    .unwrap_or_default()
}

/// Every element at `point` (in the root space, as for [`hit_test`]), the
/// topmost first, each once: what is under the topmost too (Blink's
/// list-based, penetrating hit test, `elementsFromPoint`'s). An element by
/// its own box only, not by its text or an anonymous box in it: Blink's
/// list holds the text node, which `TreeScope::ElementsFromHitTestResult`
/// drops, and no node for an anonymous box (`NodeForHitTest`); so an
/// element under another's text comes before it. Nothing for the canvas.
pub fn hit_test_all(layout_view: &BoxFragment, pre: &PrePaint, point: PointF) -> Vec<NodeId> {
    let hits = hits_at(pre, point);
    let mut nodes = Vec::new();
    Walker {
        pre,
        pointer_events_none: false,
        visit: |target: &Target<'_>| {
            if let Some(node) = target.node
                && target.own_box
                && hits(target)
                && !nodes.contains(&node)
            {
                nodes.push(node);
            }
            false
        },
    }
    .stacking_context(layout_view, None);
    nodes
}

/// How far each element's paint reaches (Blink's ink overflow, its
/// filters' included): its display items' visual rects, in the root space
/// (the document's coordinates), cut by their clips and grown by the
/// filters they are under; a run's glyphs by its text box (an SVG run's
/// by its cells), not the room the visual rect leaves for them (as
/// Blink's text fragments are hit).
/// What the occlusion test finds an element by besides its boxes: its
/// shadows, its outline, its text's shadows.
#[derive(Debug, Default)]
pub(crate) struct VisualOverflow(NodeIdHashMap<Vec<RectF>>);

impl VisualOverflow {
    /// `artifact`'s, as `pre` places it.
    pub(crate) fn of(artifact: &PaintArtifact, pre: &PrePaint) -> Self {
        let trees = &pre.trees;
        let to_root = trees.to_root();
        let mut by_element: NodeIdHashMap<Vec<RectF>> = NodeIdHashMap::default();
        for chunk in &artifact.chunks {
            let state = chunk.state;
            let clip = clip_bounds(pre, &to_root, state.clip);
            // Each filter's reach, as far as its matrix stretches it (as
            // raster invalidation grows what an item touches).
            let reach: f32 = effects(pre, state.effect)
                .filter_map(|effect| {
                    let node = &trees.effect[effect.0];
                    let filter = node.filter.as_ref()?;
                    let [a, b, c, d, ..] = to_root[node.transform.0].0;
                    Some(crate::filter::reach(filter) * (a.abs() + c.abs()).max(b.abs() + d.abs()))
                })
                .sum();
            for item in &artifact.items[chunk.items.clone()] {
                let Some(client) = item.client else {
                    continue;
                };
                let glyphs = match (item.kind, glyph_size(&item.op)) {
                    (crate::DisplayItemKind::Text, Some(size)) => crate::glyph_reach(size),
                    (crate::DisplayItemKind::SvgText, Some(size)) => {
                        let reach = crate::svg::svg_glyph_reach(size);
                        (reach, reach)
                    }
                    _ => (0.0, 0.0),
                };
                let r = item.visual_rect;
                let visual_rect = RectF::new(
                    r.x + glyphs.0,
                    r.y + glyphs.1,
                    r.width - 2.0 * glyphs.0,
                    r.height - 2.0 * glyphs.1,
                );
                let mut rect = bounds(&in_document(pre, &to_root, state.transform, visual_rect));
                if let Some(clip) = clip {
                    rect = rect.intersect(&clip);
                }
                if rect.is_empty() {
                    continue;
                }
                by_element
                    .entry(client)
                    .or_default()
                    .push(rect.inset(-reach));
            }
        }
        Self(by_element)
    }
}

/// Whether, at `rect` (in the root space), what is on top is something
/// other than `target` or what is in it (`contains`, the DOM's flat tree,
/// says): the occlusion test of `IntersectionObserver`'s visibility
/// (Blink's `LayoutObject::HitTestForOcclusion`, from the main frame).
/// What is painted after it in any frame counts by its boxes and its
/// visual overflow, `pointer-events: none` too; what is under an opacity
/// of 0 doesn't, nor what clips cut away, nor what only meets `rect` at
/// an edge. Where nothing is, it isn't.
pub(crate) fn occluded(
    layout_view: &BoxFragment,
    pre: &PrePaint,
    overflow: &VisualOverflow,
    rect: RectF,
    target: NodeId,
    contains: &dyn Fn(NodeId, NodeId) -> bool,
) -> bool {
    let _span = trace_event::trace_event!("LayoutObject::HitTestForOcclusion");
    let to_root = pre.trees.to_root();
    let mut overflow_seen = NodeIdHashSet::default();
    let hit = Walker {
        pre,
        pointer_events_none: true,
        visit: |found: &Target<'_>| {
            let Some(node) = found.node else {
                return false;
            };
            if effects(pre, found.state.effect).any(|e| pre.trees.effect[e.0].opacity == 0.0) {
                return false;
            }
            let area = match clip_bounds(pre, &to_root, found.state.clip) {
                Some(clip) => rect.intersect(&clip),
                None => rect,
            };
            // An SVG shape by what it paints only: its rect here is the
            // room hit testing gives it, past its geometry.
            let shape = found.svg.is_some() && found.cell.is_none();
            let corners = in_document(pre, &to_root, found.state.transform, found.rect.rect);
            (!shape && !area.is_empty() && overlaps(corners, area))
                || (overflow_seen.insert(node)
                    && overflow
                        .0
                        .get(&node)
                        .is_some_and(|rects| rects.iter().any(|r| !r.intersect(&rect).is_empty())))
        },
    }
    .stacking_context(layout_view, None);
    hit.and_then(|hit| hit.node)
        .is_some_and(|node| !contains(target, node))
}

/// The size of the glyphs `op` draws, if it draws a run's (cut to one side
/// of a selection, or not).
fn glyph_size(op: &compositor_paint::PaintOp) -> Option<f32> {
    match op {
        compositor_paint::PaintOp::DrawGlyphs { size, .. } => Some(*size),
        compositor_paint::PaintOp::DrawClipped { op, .. } => glyph_size(op),
        _ => None,
    }
}

/// `effect` and the effects around it.
fn effects(pre: &PrePaint, effect: EffectNodeId) -> impl Iterator<Item = EffectNodeId> + '_ {
    std::iter::successors(Some(effect), |e| pre.trees.effect[e.0].parent)
}

/// `rect`, in `transform`'s space, as four corners in the document's
/// coordinates: one fixed to the main frame's viewport shows at its
/// layout position, where the viewport is now.
fn in_document(
    pre: &PrePaint,
    to_root: &[Transform],
    transform: TransformNodeId,
    rect: RectF,
) -> [PointF; 4] {
    let matrix = &to_root[transform.0];
    let scroll = if pre.trees.is_fixed(transform) {
        pre.viewport_scroll
    } else {
        PointF::new(0.0, 0.0)
    };
    [
        PointF::new(rect.x, rect.y),
        PointF::new(rect.right(), rect.y),
        PointF::new(rect.right(), rect.bottom()),
        PointF::new(rect.x, rect.bottom()),
    ]
    .map(|p| {
        let p = matrix.map_point(p);
        PointF::new(p.x + scroll.x, p.y + scroll.y)
    })
}

/// The smallest rect around `corners`.
fn bounds(corners: &[PointF; 4]) -> RectF {
    let (x0, x1) = corners
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.x), hi.max(p.x))
        });
    let (y0, y1) = corners
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p.y), hi.max(p.y))
        });
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

/// What `clip` and the clips around it leave, in the document's
/// coordinates: each one's rect (and its paths' bounds) as far as it
/// reaches there; `None` if nothing clips. (A rounded or transformed clip
/// counts by the rect around it.)
fn clip_bounds(pre: &PrePaint, to_root: &[Transform], clip: ClipNodeId) -> Option<RectF> {
    pre.trees
        .clip_chain(clip)
        .iter()
        .filter_map(|clip| {
            let node = &pre.trees.clip[clip.0];
            let mut rect = node.rect?;
            if let Some(paths) = &node.paths {
                let shape = paths
                    .iter()
                    .filter_map(gfx::Path::bounds)
                    .reduce(|a, b| a.union(&b))
                    .unwrap_or_default();
                rect = rect.intersect(&shape);
            }
            Some(bounds(&in_document(pre, to_root, node.transform, rect)))
        })
        .reduce(|a, b| a.intersect(&b))
}

/// Whether the parallelogram `corners` and `rect` share some area (Blink's
/// `HitTestLocation::Intersects` of a transformed rect): separated along
/// neither axis of `rect` nor either of the parallelogram's.
fn overlaps(corners: [PointF; 4], rect: RectF) -> bool {
    let around = bounds(&corners);
    if around.right() <= rect.x
        || around.x >= rect.right()
        || around.bottom() <= rect.y
        || around.y >= rect.bottom()
    {
        return false;
    }
    let rect_corners = [
        PointF::new(rect.x, rect.y),
        PointF::new(rect.right(), rect.y),
        PointF::new(rect.right(), rect.bottom()),
        PointF::new(rect.x, rect.bottom()),
    ];
    corners[..3].windows(2).all(|edge| {
        let normal = PointF::new(edge[0].y - edge[1].y, edge[1].x - edge[0].x);
        let project = |points: &[PointF; 4]| {
            points
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| {
                    let along = p.x * normal.x + p.y * normal.y;
                    (lo.min(along), hi.max(along))
                })
        };
        let ((q0, q1), (r0, r1)) = (project(&corners), project(&rect_corners));
        q1 > r0 && r1 > q0
    })
}

/// Whether a target is at `point` (in the root space): in its rect, or
/// on an SVG shape, in its transform space, and not clipped away.
fn hits_at(pre: &PrePaint, point: PointF) -> impl Fn(&Target<'_>) -> bool + '_ {
    let from_root: Vec<Option<Transform>> =
        pre.trees.to_root().iter().map(Transform::inverse).collect();
    let trees = &pre.trees;
    let scroll = pre.viewport_scroll;
    move |target: &Target<'_>| {
        // The point in `state`'s transform space, if no clip there cuts
        // it away.
        let in_state = |state: PropertyTreeState| -> Option<PointF> {
            // A box fixed to the main frame's viewport is painted at its
            // layout position, which is where it shows: the point comes in
            // the document's coordinates, so take the scroll back off it.
            let point = if trees.is_fixed(state.transform) {
                PointF::new(point.x - scroll.x, point.y - scroll.y)
            } else {
                point
            };
            let at =
                |transform: TransformNodeId| from_root[transform.0].map(|m| m.map_point(point));
            let unclipped = trees.clip_chain(state.clip).iter().all(|clip| {
                let clip = &trees.clip[clip.0];
                // Inside its rounded corners too (Blink's
                // `FloatRoundedRect::IntersectsQuad`), and inside one of
                // its paths (`ClipPathClipper::HitTest`).
                match (clip.rrect(), at(clip.transform)) {
                    (Some(rrect), Some(p)) => {
                        rrect.contains(p)
                            && clip.paths.as_ref().is_none_or(|paths| {
                                paths.iter().any(|path| path.contains(p, path.fill_rule()))
                            })
                    }
                    (None, _) => true,
                    (Some(_), None) => false,
                }
            });
            if unclipped { at(state.transform) } else { None }
        };
        match (target.svg, target.cell) {
            (Some(node), Some(cell)) => in_state(pre.svg_states[&(node as *const SvgNode)])
                .is_some_and(|p| text_cell_hit(cell, p)),
            (Some(node), None) => svg_hit(pre, &in_state, node),
            (None, _) => in_state(target.state).is_some_and(|p| target.rect.contains(p)),
        }
    }
}

/// A character's cell's bounds, in the `<text>`'s space: what the
/// regions go by.
pub(crate) fn cell_bounds(cell: &SvgTextCell) -> RectF {
    let corners = cell.corners();
    let (mut x0, mut y0, mut x1, mut y1) = (
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    );
    for p in corners {
        (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
    }
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

/// Whether an SVG element is at the point `in_state` maps: a shape by its
/// geometry, a container by any shape in it.
fn svg_hit(
    pre: &PrePaint,
    in_state: &dyn Fn(PropertyTreeState) -> Option<PointF>,
    node: &SvgNode,
) -> bool {
    match &node.kind {
        SvgNodeKind::Shape(shape) => in_state(pre.svg_states[&(node as *const SvgNode)])
            .is_some_and(|p| shape_hit(node, shape, p)),
        SvgNodeKind::Container(children) => {
            children.iter().any(|child| svg_hit(pre, in_state, child))
        }
        SvgNodeKind::Hidden(_) => false,
        SvgNodeKind::Text(text) => in_state(pre.svg_states[&(node as *const SvgNode)])
            .is_some_and(|p| text.cells.iter().any(|cell| text_cell_hit(cell, p))),
    }
}

/// Where `element` is, in the root transform space (the document's
/// coordinates): the union of its border boxes as `pre` places them, through
/// transforms and scroll offsets, not cut by clips; `None` if it has no box.
/// What `Element::getBoundingClientRect` gives before the viewport's scroll
/// offset is taken off (Blink unites the element's `AbsoluteQuads`): its
/// box, or an inline element's box on each line; not what overflows them.
/// In the child frames pre-paint went into too.
pub fn element_bounds(layout_view: &BoxFragment, pre: &PrePaint, element: NodeId) -> Option<RectF> {
    let to_root = pre.trees.to_root();
    let scroll = pre.viewport_scroll;
    let fixed = |transform: TransformNodeId| pre.trees.is_fixed(transform);
    let mut bounds: Option<RectF> = None;
    let mut add = |rect: RectF, transform: TransformNodeId| {
        let rect = to_root[transform.0].map_rect(rect);
        // Fixed to the viewport: it shows at its layout position, so in
        // the document's coordinates it is where the viewport is now.
        let rect = if fixed(transform) {
            rect.offset(scroll.x, scroll.y)
        } else {
            rect
        };
        bounds = Some(bounds.map_or(rect, |bounds| bounds.union(&rect)));
    };
    let mut stack = vec![layout_view];
    while let Some(fragment) = stack.pop() {
        stack.extend(fragment.children.iter().map(|child| &*child.fragment));
        if let Some(frame) = pre.child_frame(fragment) {
            stack.push(&frame.layout_view);
        }
        let properties = pre.properties(fragment);
        let origin = properties.offset;
        if fragment.node == Some(element) && fragment.pseudo.is_none() {
            let border_box = RectF::new(
                origin.x,
                origin.y,
                fragment.size.width,
                fragment.size.height,
            );
            add(border_box, properties.local.transform);
        }
        let on_lines = fragment
            .lines
            .iter()
            .flat_map(|line| line.boxes.iter().map(|b| (b.node, b.pseudo, b.rect)));
        for (node, pseudo, rect) in on_lines {
            if node == element && pseudo.is_none() {
                add(
                    rect.offset(origin.x, origin.y),
                    properties.contents.transform,
                );
            }
        }
    }
    bounds
}

/// Where `element`'s first box's border box starts in its transform space,
/// as `pre` places it (Blink's `FragmentData::PaintOffset` of its first
/// fragment): what a size in device pixels is snapped with. In the child
/// frames pre-paint went into too; `None` if it has no box of its own (an
/// inline element's boxes on the lines have none either, as in Blink).
pub fn paint_offset(layout_view: &BoxFragment, pre: &PrePaint, element: NodeId) -> Option<PointF> {
    let mut stack = vec![layout_view];
    while let Some(fragment) = stack.pop() {
        if fragment.node == Some(element) && fragment.pseudo.is_none() {
            return Some(pre.properties(fragment).offset);
        }
        // Tree order: the first box is the first fragment.
        if let Some(frame) = pre.child_frame(fragment) {
            stack.push(&frame.layout_view);
        }
        stack.extend(fragment.children.iter().rev().map(|child| &*child.fragment));
    }
    None
}

/// Something hit testing can find: a rect in `state`'s transform space,
/// under its clips; or an SVG element, by its shapes.
#[derive(Debug, Clone, Copy)]
struct Target<'a> {
    /// With its rounded corners: a point outside a corner's curve isn't
    /// on the box (Blink's `HitTestClippedOutByBorder`).
    rect: RRectF,
    state: PropertyTreeState,
    node: Option<NodeId>,
    /// The text node, for a run of text.
    text: Option<NodeId>,
    /// The innermost scroll container it is in, or its own.
    scroll: Option<ScrollNodeId>,
    /// The scroll container whose own border box this is. (Only debug
    /// builds read it and `wheel`: to check paint records what the walk
    /// finds.)
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    container: Option<ScrollNodeId>,
    /// `node`'s own box: an element's border box or inline box; not a run
    /// of text or an anonymous box in it, nor a child frame's canvas.
    own_box: bool,
    /// An SVG element in an `<svg>`'s content: found by its geometry, not
    /// `rect`, which holds its shape's bounds (empty for a container).
    svg: Option<&'a SvgNode>,
    /// One character's cell of `svg`'s text: found by it alone.
    cell: Option<&'a SvgTextCell>,
    /// Inside a blocking `wheel` listener, as its style says.
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    wheel: bool,
}

/// Walks what hit testing can find in the reverse of paint order, until
/// `visit` says it is the one.
struct Walker<'a, V> {
    pre: &'a PrePaint,
    /// Whether what is `pointer-events: none` is found too (Blink's
    /// `kIgnorePointerEventsNone`, the occlusion test's).
    pointer_events_none: bool,
    visit: V,
}

impl<V: FnMut(&Target<'_>) -> bool> Walker<'_, V> {
    /// Whether what a box or a run with `style` paints is found:
    /// [`hittable`], or only visible if `pointer_events_none`.
    fn finds(&self, style: &computed_style::ComputedStyle) -> bool {
        if self.pointer_events_none {
            style.is_visible()
        } else {
            hittable(style)
        }
    }

    fn found(&mut self, target: Target<'_>) -> Option<HitTestResult> {
        (self.visit)(&target).then_some(HitTestResult {
            node: target.node,
            text: target.text,
            scroll: target.scroll,
        })
    }

    /// A stacking context, the reverse of `Painter::stacking_context`: the
    /// child stacking contexts from the last down to `z-index` 0, the lines
    /// (with the flex items), the other backgrounds, the child stacking
    /// contexts with a negative `z-index`, then its own border box.
    /// `around`: the nearest element around `root`.
    fn stacking_context(
        &mut self,
        root: &BoxFragment,
        around: Option<NodeId>,
    ) -> Option<HitTestResult> {
        let children = stacking_children(root, around);
        let negative = children
            .iter()
            .take_while(|(child, _)| child.style.stack_level() < 0)
            .count();
        let node = root.node.or(around);
        for &(child, around) in children[negative..].iter().rev() {
            if let Some(hit) = self.stacking_context(child, around) {
                return Some(hit);
            }
        }
        if let Some(hit) = self
            .lines(root, true, around)
            .or_else(|| self.floats(root, node))
            .or_else(|| self.child_backgrounds(root, node))
        {
            return Some(hit);
        }
        for &(child, around) in children[..negative].iter().rev() {
            if let Some(hit) = self.stacking_context(child, around) {
                return Some(hit);
            }
        }
        self.border_box(root, node)
    }

    /// The line phase backwards: children from the last (a flex item
    /// whole: its lines, then its backgrounds), then each line from the
    /// last, and in a line its runs, atomic inlines (whole) and inline
    /// boxes in the reverse of their paint order.
    fn lines(
        &mut self,
        fragment: &BoxFragment,
        is_root: bool,
        around: Option<NodeId>,
    ) -> Option<HitTestResult> {
        if !is_root && (fragment.style.is_stacked() || fragment.floating) {
            return None;
        }
        let node = fragment.node.or(around);
        let whole = paints_children_whole(fragment);
        // Atomic inlines are found with their lines, below.
        let children = if children_on_lines(fragment) {
            &[][..]
        } else {
            &fragment.children[..]
        };
        for child in children.iter().rev() {
            let hit = self.lines(&child.fragment, false, node).or_else(|| {
                whole
                    .then(|| {
                        self.floats(&child.fragment, node)
                            .or_else(|| self.backgrounds(&child.fragment, node))
                    })
                    .flatten()
            });
            if hit.is_some() {
                return hit;
            }
        }
        // An `<svg>`'s content, where it was painted: each shape says
        // whether it is hit.
        if let Some(svg) = &fragment.svg {
            let scroll = self.pre.properties(fragment).scroll;
            if let Some(hit) = svg
                .children
                .iter()
                .rev()
                .find_map(|n| self.svg_node(n, scroll))
            {
                return Some(hit);
            }
        }
        // An iframe's child frame, where it was painted.
        if let Some(frame) = self.pre.child_frame(fragment) {
            if !self.finds(&fragment.style) {
                return None;
            }
            return self.child_frame(frame);
        }
        if fragment.lines.is_empty() {
            return None;
        }
        let properties = self.pre.properties(fragment);
        let origin = properties.offset;
        let at = |r: RectF| RectF::new(origin.x + r.x, origin.y + r.y, r.width, r.height);
        for line in fragment.lines.iter().rev() {
            // In the reverse of paint order.
            for part in line_parts(line).into_iter().rev() {
                let rect = match part {
                    // `visibility: hidden` isn't hit, as it isn't painted.
                    // Nor is `pointer-events: none`.
                    LinePart::InlineBox(b) if !self.finds(&b.style) => None,
                    LinePart::Run(run) if !self.finds(&run.style) => None,
                    LinePart::InlineBox(b) => Some((b.rect, Some(b.node), None, true, &b.style)),
                    // The text's own box: its advance as far as it is
                    // visible (`text-overflow` hides the rest), and the
                    // font's ascent and descent around the baseline.
                    LinePart::Run(run) => Some((
                        RectF::new(
                            run.x,
                            run.baseline - run.ascent,
                            run.visible_advance,
                            run.ascent + run.descent,
                        ),
                        run.node.or(node),
                        run.source.as_ref().map(|(text, _)| *text),
                        false,
                        &run.style,
                    )),
                    LinePart::Atomic(_) => None,
                };
                let hit = match (part, rect) {
                    (_, Some((rect, node, text, own_box, style))) => self.found(Target {
                        text,
                        rect: RRectF::from_rect(at(rect)),
                        state: properties.contents,
                        node,
                        scroll: properties.scroll,
                        container: None,
                        own_box,
                        svg: None,
                        cell: None,
                        wheel: style.inside_blocking_wheel_event_handler,
                    }),
                    (LinePart::Atomic(child), None) => {
                        let atomic = &fragment.children[child].fragment;
                        self.lines(atomic, false, node)
                            .or_else(|| self.floats(atomic, node))
                            .or_else(|| self.backgrounds(atomic, node))
                    }
                    (_, None) => None,
                };
                if hit.is_some() {
                    return hit;
                }
            }
        }
        None
    }

    /// An SVG element of an `<svg>`'s content, backwards: a container's
    /// children from the last (and itself, for `bounding-box`), a shape by
    /// its geometry, text by its characters.
    fn svg_node(&mut self, node: &SvgNode, scroll: Option<ScrollNodeId>) -> Option<HitTestResult> {
        let rect = match &node.kind {
            // Its children, from the last. As Chrome 152 does (not as
            // `elementsFromPoint`'s tests ask), a container isn't found
            // itself, unless its `pointer-events` is `bounding-box`: then
            // where something in it is.
            SvgNodeKind::Container(children) => {
                if let Some(hit) = children.iter().rev().find_map(|n| self.svg_node(n, scroll)) {
                    return Some(hit);
                }
                if node.style.pointer_events != PointerEvents::BoundingBox {
                    return None;
                }
                RectF::default()
            }
            SvgNodeKind::Shape(shape) => shape_bounds(node, shape),
            SvgNodeKind::Hidden(_) => return None,
            // Each character, from the last, as the element it is in; the
            // `<text>` is found by its own characters only.
            SvgNodeKind::Text(text) => {
                let state = self.pre.svg_states[&(node as *const SvgNode)];
                for cell in text.cells.iter().rev() {
                    let hit = self.found(Target {
                        text: None,
                        rect: RRectF::from_rect(cell_bounds(cell)),
                        state,
                        node: Some(cell.node),
                        scroll,
                        container: None,
                        own_box: true,
                        svg: Some(node),
                        cell: Some(cell),
                        wheel: node.style.inside_blocking_wheel_event_handler,
                    });
                    if hit.is_some() {
                        return hit;
                    }
                }
                return None;
            }
        };
        self.found(Target {
            text: None,
            rect: RRectF::from_rect(rect),
            state: self.pre.svg_states[&(node as *const SvgNode)],
            node: Some(node.node),
            scroll,
            container: None,
            own_box: true,
            svg: Some(node),
            cell: None,
            wheel: node.style.inside_blocking_wheel_event_handler,
        })
    }

    /// A child frame's document (Blink's `LayoutEmbeddedContent::
    /// NodeAtPointOverEmbeddedContent`): what is hit in it, or else its
    /// canvas, which is its root element's, within the iframe's content
    /// box: what its viewport, a scroll container, picks a wheel by.
    fn child_frame(&mut self, frame: &EmbeddedFrame) -> Option<HitTestResult> {
        if let Some(hit) = self.stacking_context(&frame.layout_view, None) {
            return Some(hit);
        }
        let root = frame
            .layout_view
            .children
            .first()
            .filter(|link| link.fragment.role == BoxRole::RootElement)
            .and_then(|link| link.fragment.node);
        self.found(Target {
            text: None,
            rect: RRectF::from_rect(RectF::new(
                0.0,
                0.0,
                frame.document_size.width,
                frame.document_size.height,
            )),
            state: frame.state,
            node: root,
            scroll: Some(frame.scroll),
            container: Some(frame.scroll),
            own_box: false,
            svg: None,
            cell: None,
            wheel: frame.root_in_blocking_wheel(),
        })
    }

    /// The background phase backwards below `fragment`: children from the
    /// last, then the box's own border box; not into child stacking
    /// contexts, nor flex items, found whole with the lines.
    fn backgrounds(
        &mut self,
        fragment: &BoxFragment,
        around: Option<NodeId>,
    ) -> Option<HitTestResult> {
        if fragment.style.is_stacked() || fragment.floating {
            return None;
        }
        let node = fragment.node.or(around);
        self.child_backgrounds(fragment, node)
            .or_else(|| self.border_box(fragment, node))
    }

    /// The float phase below `fragment`, the reverse of `Painter::floats`:
    /// each float from the last, whole (its lines, its floats, its
    /// backgrounds, its own border box). `node`: the nearest element
    /// around `fragment`'s children.
    fn floats(&mut self, fragment: &BoxFragment, node: Option<NodeId>) -> Option<HitTestResult> {
        if paints_children_whole(fragment) {
            return None;
        }
        let on_lines = children_on_lines(fragment);
        fragment.children.iter().rev().find_map(|child| {
            let child = &child.fragment;
            if child.style.is_stacked() {
                return None;
            }
            let inner = child.node.or(node);
            if child.floating {
                self.lines(child, true, node)
                    .or_else(|| self.floats(child, inner))
                    .or_else(|| self.child_backgrounds(child, inner))
                    .or_else(|| self.border_box(child, inner))
            } else if !on_lines {
                self.floats(child, inner)
            } else {
                None
            }
        })
    }

    /// `fragment`'s children's background phase, backwards. `node`: the
    /// nearest element around them.
    fn child_backgrounds(
        &mut self,
        fragment: &BoxFragment,
        node: Option<NodeId>,
    ) -> Option<HitTestResult> {
        if paints_children_whole(fragment) || children_on_lines(fragment) {
            return None;
        }
        fragment
            .children
            .iter()
            .rev()
            .find_map(|child| self.backgrounds(&child.fragment, node))
    }

    /// `fragment`'s own border box, as `node`.
    fn border_box(
        &mut self,
        fragment: &BoxFragment,
        node: Option<NodeId>,
    ) -> Option<HitTestResult> {
        if fragment.role == BoxRole::LayoutView || !self.finds(&fragment.style) {
            return None;
        }
        let properties = self.pre.properties(fragment);
        let border_box = RectF::new(
            properties.offset.x,
            properties.offset.y,
            fragment.size.width,
            fragment.size.height,
        );
        let container = properties
            .scroll
            .filter(|s| fragment.node == Some(self.pre.trees.scroll(*s).element));
        self.found(Target {
            text: None,
            rect: RRectF::new(
                border_box,
                crate::border_radii(&fragment.style, fragment.size),
            ),
            state: properties.local,
            node,
            scroll: properties.scroll,
            container,
            own_box: fragment.node.is_some(),
            svg: None,
            cell: None,
            wheel: fragment.style.inside_blocking_wheel_event_handler,
        })
    }
}

/// Whether hit testing finds what a box or a run with `style` paints:
/// not where it is `visibility: hidden` or `pointer-events: none` (an
/// HTML box's other values are `auto`).
pub(crate) fn hittable(style: &computed_style::ComputedStyle) -> bool {
    style.is_visible() && style.pointer_events != PointerEvents::None
}
