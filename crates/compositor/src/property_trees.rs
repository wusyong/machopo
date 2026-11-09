//! The compositor's property trees (cc's `TransformTree`, `ClipTree` and
//! `ScrollTree`): where each layer is in the document, what clips it, and
//! what the compositor may scroll, and where. The renderer builds them from
//! paint's (Blink's `PropertyTreeManager`).
//!
//! Only scroll containers become layers, and only those that are merely
//! translated, so a transform node is an offset. The scroll offsets of
//! those containers are applied here rather than painted: a container's
//! scroll translation node moves its content by its element's offset, so
//! scrolling it, on either thread, changes no display list.

use std::collections::HashMap;
use std::sync::Arc;

use gfx::{PointF, RRectF, RectF, SizeF};

use crate::ScrollNode;
use crate::proxy_impl::InputAttribution;

/// `cc::ElementId`: the element a scroll offset belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ElementId(pub u64);

impl ElementId {
    /// The main frame's viewport, the root of cc's scroll tree, which has
    /// no node here: its offset is in the offsets only where a sticky box
    /// sticks in it.
    pub const VIEWPORT: Self = Self(u64::MAX);
}

/// Each scroll container's offset, CSS px.
pub type ScrollOffsets = HashMap<ElementId, PointF>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TransformNodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ClipNodeId(pub usize);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScrollNodeId(pub usize);

/// `cc::TransformNode`, translations only.
#[derive(Debug, Clone, PartialEq)]
pub struct TransformNode {
    pub parent: Option<TransformNodeId>,
    /// Where this node's space is in its parent's, CSS px.
    pub offset: PointF,
    /// A scroll container's scroll translation: its content also moves up
    /// and left by that element's scroll offset.
    pub scroll: Option<ElementId>,
    /// A sticky box's translation: it also moves by how far the box sticks
    /// (cc's `StickyPositionNodeData`).
    pub sticky: Option<StickyPositionConstraint>,
}

/// cc's `StickyPositionConstraint`, with what moves it: a sticky box's
/// rects in its scroller's content space before it scrolls, its insets,
/// and the sticky box around it whose moving moves its containing block.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StickyPositionConstraint {
    /// The container whose offset moves the scrollport;
    /// [`ElementId::VIEWPORT`] for the main frame's viewport.
    pub scroller: ElementId,
    /// What it stays inside: the scrollport's content box, unscrolled.
    pub constraining_rect: RectF,
    pub sticky_box: RectF,
    /// As far as it may move.
    pub containing_block: RectF,
    /// `top`, `right`, `bottom`, `left`; `None` for `auto`.
    pub inset: [Option<f32>; 4],
    /// The sticky box's translation that moves its containing block (cc's
    /// `nearest_node_shifting_containing_block`).
    pub shifting_containing_block: Option<TransformNodeId>,
    /// Where the box is in the node's space, and the device pixels per
    /// CSS px: the box is painted snapped there, so its offset is snapped
    /// with it (Blink's `pixel_snap_offset`), as the main thread snaps it.
    pub pixel_snap_offset: PointF,
    pub device_scale_factor: f32,
}

/// `cc::ClipNode`: an axis-aligned rect, its corners maybe rounded.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipNode {
    pub parent: Option<ClipNodeId>,
    /// The space `rect` is in.
    pub transform: TransformNodeId,
    /// `None` for the root: no clip.
    pub rect: Option<RectF>,
    /// Its corners' radii (top left, top right, bottom right, bottom
    /// left), all 0 for a plain rect: a scroll container's
    /// `border-radius`, which Viz cuts the layer's quads to.
    pub radii: [SizeF; 4],
}

/// A node of cc's `ScrollTree`: a scroll container that is a layer, which
/// the compositor scrolls by its element's offset. The viewport, the root
/// of cc's tree, is kept apart (`LayerTreeHost::set_viewport_scroll_node`).
#[derive(Debug, Clone, PartialEq)]
pub struct ScrollTreeNode {
    /// The nearest container around it that is a layer too, which takes
    /// what it can't; `None`: the viewport. A container between them that
    /// isn't a layer can't be scrolled by the user, or a wheel there goes
    /// to the main thread.
    pub parent: Option<ScrollNodeId>,
    pub element: ElementId,
    /// Its port and content.
    pub scroller: ScrollNode,
    /// Where a wheel picks it: its border box, in `transform`'s space (the
    /// one it is in, outside its own scroll offset), cut by the clips
    /// around it (`clip`).
    pub transform: TransformNodeId,
    pub border_box: RectF,
    pub clip: ClipNodeId,
    /// The space its content is in, which its offset moves.
    pub scroll_translation: TransformNodeId,
    /// Where over its content a wheel needs the main thread, in
    /// `scroll_translation`'s space (cc's
    /// `main_thread_scroll_hit_test_region`).
    pub main_thread_scroll_hit_test_region: Arc<[RectF]>,
    /// Where over its content a wheel waits for the main thread, as a
    /// listener there may cancel it (cc's `wheel_event_region`), in the
    /// same space.
    pub wheel_event_handler_region: Arc<[RectF]>,
}

/// The trees; each node's parent comes before it.
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyTrees {
    pub transform: Vec<TransformNode>,
    pub clip: Vec<ClipNode>,
    /// Each container a scroll translation node scrolls by its element.
    pub scroll: Vec<ScrollTreeNode>,
    /// Where a wheel needs the main thread, in the document's space,
    /// outside every container in `scroll`. Painted with the layers, so the
    /// two always commit together.
    pub main_thread_scroll_hit_test_region: Arc<[RectF]>,
    /// Where a wheel waits for the main thread's `wheel` listeners, which
    /// may cancel it, in the document's space, outside every container in
    /// `scroll`.
    pub wheel_event_handler_region: Arc<[RectF]>,
    /// Whether the page may have `wheel` listeners at all (cc's
    /// `EventListenerProperties` for the wheel): if so, the main thread
    /// gets the wheels the compositor scrolled too, to dispatch.
    pub wheel_event_listeners: bool,
    /// The two regions of what a sticky box's translation moves, in its
    /// space: they move with the box as it sticks.
    pub sticky_regions: Vec<StickyRegions>,
    /// Which frame input at a point is for, as paint has it.
    pub frame_attribution: FrameAttributionRegion,
}

/// Which frame input at a point is for (cc's `FindFrameElementIdAtPoint`
/// over its layers' frames), as paint's `frame_attribution_region` gives it:
/// the child frames, and where their canvases and what is painted after them
/// are. Empty without child frames: all of it is the main frame's.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FrameAttributionRegion {
    /// Each after the one it is in.
    pub frames: Arc<[AttributedFrame]>,
    /// In paint order.
    pub rects: Arc<[AttributionRect]>,
}

/// A child frame: its document, and the child frame it is in (an index in
/// the frames), `None` for the main frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributedFrame {
    pub document: ElementId,
    pub parent: Option<usize>,
}

/// Where something of a frame is, in its space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttributionRect {
    pub rect: RectF,
    pub space: AttributionSpace,
    /// Its place in paint order.
    pub order: u32,
    /// Its child frame (an index in the frames); `None` for the main
    /// frame's.
    pub frame: Option<usize>,
    /// A child frame's canvas under a rounded clip or a `clip-path`.
    pub masked: bool,
}

/// An [`AttributionRect`]'s space: the document's, a container's that is a
/// layer (its scroll translation's), or a sticky box's translation's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionSpace {
    Document,
    Scroller(ScrollNodeId),
    Sticky(TransformNodeId),
}

/// The regions in a sticky box's translation's space.
#[derive(Debug, Clone, PartialEq)]
pub struct StickyRegions {
    pub transform: TransformNodeId,
    pub main_thread_scroll_hit_test_region: Arc<[RectF]>,
    pub wheel_event_handler_region: Arc<[RectF]>,
}

/// What a wheel at a point is for (cc's `ScrollTree` hit test, with
/// `InputHandler`'s main thread hit test).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollHitTest {
    /// The main thread picks the scroller; `blocking` if a `wheel`
    /// listener may cancel the scrolling first (the wheel event handler
    /// region).
    MainThread { blocking: bool },
    /// The innermost container that is a layer there, then those around
    /// it; `None`: only the viewport.
    Scroll(Option<ScrollNodeId>),
}

impl Default for PropertyTrees {
    /// Just the roots: the document, unclipped.
    fn default() -> Self {
        Self {
            transform: vec![TransformNode {
                parent: None,
                offset: PointF::new(0.0, 0.0),
                scroll: None,
                sticky: None,
            }],
            clip: vec![ClipNode {
                parent: None,
                transform: TransformNodeId(0),
                rect: None,
                radii: Default::default(),
            }],
            scroll: Vec::new(),
            main_thread_scroll_hit_test_region: Arc::from([]),
            wheel_event_handler_region: Arc::from([]),
            wheel_event_listeners: false,
            sticky_regions: Vec::new(),
            frame_attribution: FrameAttributionRegion::default(),
        }
    }
}

impl PropertyTrees {
    /// Where `node`'s origin is in the document, CSS px, each container
    /// scrolled by its offset in `offsets`: those drawn, which the caller
    /// has snapped to device pixels.
    pub(crate) fn position(&self, mut node: TransformNodeId, offsets: &ScrollOffsets) -> PointF {
        let mut at = PointF::new(0.0, 0.0);
        loop {
            let n = &self.transform[node.0];
            at = PointF::new(at.x + n.offset.x, at.y + n.offset.y);
            if let Some(scroll) = n.scroll.and_then(|element| offsets.get(&element)) {
                at = PointF::new(at.x - scroll.x, at.y - scroll.y);
            }
            if let Some(sticky) = &n.sticky {
                let by = sticky.snapped(self.sticky_offset(node, offsets));
                at = PointF::new(at.x + by.x, at.y + by.y);
            }
            match n.parent {
                Some(parent) => node = parent,
                None => return at,
            }
        }
    }

    /// How far the sticky box of translation `node` moves with the
    /// scrollers at `offsets`, unsnapped (cc's `StickyPositionOffset`, the
    /// main thread's `ComputeStickyOffset`): into its scrollport by its
    /// insets, no further than its containing block, after the sticky box
    /// around that moved.
    pub fn sticky_offset(&self, node: TransformNodeId, offsets: &ScrollOffsets) -> PointF {
        let Some(c) = &self.transform[node.0].sticky else {
            return PointF::new(0.0, 0.0);
        };
        let ancestor = c
            .shifting_containing_block
            .map_or(PointF::new(0.0, 0.0), |n| {
                self.total_sticky_offset(n, offsets)
            });
        let scrolled = offsets.get(&c.scroller).copied().unwrap_or_default();
        let sticky_box = c.sticky_box.offset(ancestor.x, ancestor.y);
        let containing = c.containing_block.offset(ancestor.x, ancestor.y);
        let content = c.constraining_rect.offset(scrolled.x, scrolled.y);
        let [top, right, bottom, left] = c.inset;
        PointF::new(
            sticky_axis_offset(
                (sticky_box.x, sticky_box.right()),
                (containing.x, containing.right()),
                (content.x, content.right()),
                left,
                right,
            ),
            sticky_axis_offset(
                (sticky_box.y, sticky_box.bottom()),
                (containing.y, containing.bottom()),
                (content.y, content.bottom()),
                top,
                bottom,
            ),
        )
    }

    /// The sticky box of `node`'s offset with those of the sticky boxes
    /// moving its containing block (cc's
    /// `total_containing_block_sticky_offset`).
    fn total_sticky_offset(&self, node: TransformNodeId, offsets: &ScrollOffsets) -> PointF {
        let own = self.sticky_offset(node, offsets);
        let around = self.transform[node.0]
            .sticky
            .and_then(|c| c.shifting_containing_block)
            .map_or(PointF::new(0.0, 0.0), |n| {
                self.total_sticky_offset(n, offsets)
            });
        PointF::new(own.x + around.x, own.y + around.y)
    }

    /// What a wheel at `at` (the document's space, CSS px) is for, the
    /// containers at `offsets`: the main thread, if a region says so, else
    /// the innermost container whose border box, cut by its clips, is
    /// there. Those that are layers don't overlap but where one is in
    /// another, or a region covers the overlap.
    pub(crate) fn scroll_hit_test(&self, at: PointF, offsets: &ScrollOffsets) -> ScrollHitTest {
        let in_region = |region: &[RectF], at: PointF| region.iter().any(|rect| rect.contains(at));
        let mut main_thread = in_region(&self.main_thread_scroll_hit_test_region, at);
        let mut blocking = in_region(&self.wheel_event_handler_region, at);
        let mut innermost = None;
        for (index, node) in self.scroll.iter().enumerate() {
            let origin = self.position(node.transform, offsets);
            let mut picked_at = node.border_box.offset(origin.x, origin.y);
            if let Some(clip) = self.clip_rect(node.clip, offsets) {
                picked_at = picked_at.intersect(&clip);
            }
            if !picked_at.contains(at) {
                continue;
            }
            let content = self.position(node.scroll_translation, offsets);
            let inside = PointF::new(at.x - content.x, at.y - content.y);
            main_thread |= in_region(&node.main_thread_scroll_hit_test_region, inside);
            blocking |= in_region(&node.wheel_event_handler_region, inside);
            // Parents come first: the last is the innermost.
            innermost = Some(ScrollNodeId(index));
        }
        for sticky in &self.sticky_regions {
            let origin = self.position(sticky.transform, offsets);
            let inside = PointF::new(at.x - origin.x, at.y - origin.y);
            main_thread |= in_region(&sticky.main_thread_scroll_hit_test_region, inside);
            blocking |= in_region(&sticky.wheel_event_handler_region, inside);
        }
        if main_thread || blocking {
            return ScrollHitTest::MainThread { blocking };
        }
        ScrollHitTest::Scroll(innermost)
    }

    /// Which frame input at `at` (the document's space, CSS px) is for, the
    /// containers at `offsets` (cc's `FindClosestMatchingLayerForAttribution`
    /// over the attribution's rects): the frame of the rect there painted
    /// last; none can be told if another rect there is of a frame that
    /// isn't that one or one around it, or that one is a child frame's
    /// masked canvas. The main frame's where there is no rect. A rect in a
    /// container's space counts only where the container is picked, as for
    /// a wheel.
    pub(crate) fn frame_attribution(
        &self,
        at: PointF,
        offsets: &ScrollOffsets,
    ) -> InputAttribution {
        let attribution = &self.frame_attribution;
        if attribution.rects.is_empty() {
            return InputAttribution::TargetedFrame(None);
        }
        let inside = |space: AttributionSpace| -> Option<PointF> {
            match space {
                AttributionSpace::Document => Some(at),
                AttributionSpace::Scroller(id) => {
                    let node = &self.scroll[id.0];
                    let origin = self.position(node.transform, offsets);
                    let mut picked_at = node.border_box.offset(origin.x, origin.y);
                    if let Some(clip) = self.clip_rect(node.clip, offsets) {
                        picked_at = picked_at.intersect(&clip);
                    }
                    if !picked_at.contains(at) {
                        return None;
                    }
                    let content = self.position(node.scroll_translation, offsets);
                    Some(PointF::new(at.x - content.x, at.y - content.y))
                }
                AttributionSpace::Sticky(transform) => {
                    let origin = self.position(transform, offsets);
                    Some(PointF::new(at.x - origin.x, at.y - origin.y))
                }
            }
        };
        let hits: Vec<&AttributionRect> = attribution
            .rects
            .iter()
            .filter(|r| inside(r.space).is_some_and(|point| r.rect.contains(point)))
            .collect();
        let Some(top) = hits.iter().max_by_key(|r| r.order) else {
            return InputAttribution::TargetedFrame(None);
        };
        if top.masked {
            return InputAttribution::Unknown;
        }
        // The top one's frame and those around it.
        let mut path = vec![top.frame];
        while let Some(Some(frame)) = path.last().copied() {
            path.push(attribution.frames[frame].parent);
        }
        if hits.iter().any(|r| !path.contains(&r.frame)) {
            return InputAttribution::Unknown;
        }
        InputAttribution::TargetedFrame(top.frame.map(|f| attribution.frames[f].document))
    }

    /// The rounded one of `clip` and the clips around it, where it is in
    /// the document, CSS px; `None` if none is rounded. Paint makes only
    /// scroll containers under at most one rounded clip layers, so there
    /// is one at most (the innermost, if not).
    pub(crate) fn rounded_clip(
        &self,
        mut clip: ClipNodeId,
        offsets: &ScrollOffsets,
    ) -> Option<RRectF> {
        loop {
            let node = &self.clip[clip.0];
            if let Some(rect) = node.rect {
                let rrect = RRectF {
                    rect,
                    radii: node.radii,
                };
                if !rrect.is_rect() {
                    let at = self.position(node.transform, offsets);
                    return Some(rrect.offset(at.x, at.y));
                }
            }
            clip = node.parent?;
        }
    }

    /// What `clip` and the clips around it leave, in the document, CSS px;
    /// `None` if nothing clips.
    pub(crate) fn clip_rect(&self, mut clip: ClipNodeId, offsets: &ScrollOffsets) -> Option<RectF> {
        let mut out: Option<RectF> = None;
        loop {
            let node = &self.clip[clip.0];
            if let Some(rect) = node.rect {
                let at = self.position(node.transform, offsets);
                let rect = RectF::new(rect.x + at.x, rect.y + at.y, rect.width, rect.height);
                out = Some(out.map_or(rect, |o| o.intersect(&rect)));
            }
            match node.parent {
                Some(parent) => clip = parent,
                None => return out,
            }
        }
    }
}

impl StickyPositionConstraint {
    /// `by`, snapped with the box's position: the two land on the device
    /// pixel the main thread paints the box at.
    fn snapped(&self, by: PointF) -> PointF {
        let scale = self.device_scale_factor;
        let snap = |v: f32| (v * scale).round() / scale;
        let at = self.pixel_snap_offset;
        PointF::new(
            snap(at.x + by.x) - snap(at.x),
            snap(at.y + by.y) - snap(at.y),
        )
    }
}

/// One axis of cc's `StickyPositionOffset`: the box's, its containing
/// block's and the scrollport's start and end; the end inset first, then
/// the start one, which wins.
fn sticky_axis_offset(
    sticky_box: (f32, f32),
    containing: (f32, f32),
    content: (f32, f32),
    min: Option<f32>,
    max: Option<f32>,
) -> f32 {
    let mut delta = 0.0;
    if let Some(max) = max {
        let wanted = (content.1 - max - sticky_box.1).min(0.0);
        let available = (containing.0 - sticky_box.0).min(0.0);
        delta += wanted.max(available);
    }
    if let Some(min) = min {
        let wanted = (content.0 + min - sticky_box.0).max(0.0);
        let available = (containing.1 - sticky_box.1).max(0.0);
        delta += wanted.min(available);
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A container at (10, 20) whose content is its element 7's, clipped to
    /// its 100x50 padding box at (12, 22) in its space.
    fn container() -> PropertyTrees {
        let mut trees = PropertyTrees::default();
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(0)),
            offset: PointF::new(10.0, 20.0),
            scroll: None,
            sticky: None,
        });
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(1)),
            offset: PointF::new(0.0, 0.0),
            scroll: Some(ElementId(7)),
            sticky: None,
        });
        trees.clip.push(ClipNode {
            parent: Some(ClipNodeId(0)),
            transform: TransformNodeId(1),
            rect: Some(RectF::new(2.0, 2.0, 100.0, 50.0)),
            radii: Default::default(),
        });
        trees
    }

    #[test]
    fn a_scroll_translation_moves_by_its_elements_offset() {
        let trees = container();
        let mut offsets = ScrollOffsets::default();
        assert_eq!(
            trees.position(TransformNodeId(2), &offsets),
            PointF::new(10.0, 20.0)
        );
        offsets.insert(ElementId(7), PointF::new(0.0, 30.5));
        assert_eq!(
            trees.position(TransformNodeId(2), &offsets),
            PointF::new(10.0, -10.5)
        );
        assert_eq!(
            trees.position(TransformNodeId(1), &offsets),
            PointF::new(10.0, 20.0),
            "the container itself doesn't move"
        );
    }

    /// `container()` as a container that is a layer: its border box is
    /// its padding box grown by 2px, at (10, 20) in the root's space, under
    /// a clip of the root's from y 0 to 50.
    fn picked() -> PropertyTrees {
        let mut trees = container();
        trees.clip.push(ClipNode {
            parent: Some(ClipNodeId(0)),
            transform: TransformNodeId(0),
            rect: Some(RectF::new(0.0, 0.0, 500.0, 50.0)),
            radii: Default::default(),
        });
        trees.scroll.push(ScrollTreeNode {
            parent: None,
            element: ElementId(7),
            scroller: ScrollNode {
                container_bounds: gfx::SizeF::new(100.0, 50.0),
                bounds: gfx::SizeF::new(100.0, 300.0),
                user_scrollable_horizontal: true,
                user_scrollable_vertical: true,
            },
            transform: TransformNodeId(1),
            border_box: RectF::new(0.0, 0.0, 104.0, 54.0),
            clip: ClipNodeId(2),
            scroll_translation: TransformNodeId(2),
            main_thread_scroll_hit_test_region: Arc::from([]),
            wheel_event_handler_region: Arc::from([]),
        });
        trees
    }

    fn hit(trees: &PropertyTrees, x: f32, y: f32, offsets: &ScrollOffsets) -> ScrollHitTest {
        trees.scroll_hit_test(PointF::new(x, y), offsets)
    }

    const CONTAINER: ScrollHitTest = ScrollHitTest::Scroll(Some(ScrollNodeId(0)));
    const VIEWPORT: ScrollHitTest = ScrollHitTest::Scroll(None);

    #[test]
    fn a_wheel_picks_a_container_by_its_border_box_cut_by_its_clips() {
        let trees = picked();
        let none = ScrollOffsets::default();
        assert_eq!(hit(&trees, 10.0, 20.0, &none), CONTAINER, "its border");
        assert_eq!(hit(&trees, 113.0, 49.0, &none), CONTAINER);
        assert_eq!(hit(&trees, 114.0, 30.0, &none), VIEWPORT, "beside it");
        assert_eq!(hit(&trees, 9.0, 30.0, &none), VIEWPORT);
        assert_eq!(hit(&trees, 50.0, 50.0, &none), VIEWPORT, "clipped away");
        // Its own offset doesn't move it.
        let scrolled: ScrollOffsets = [(ElementId(7), PointF::new(0.0, 100.0))].into();
        assert_eq!(hit(&trees, 10.0, 20.0, &scrolled), CONTAINER);
    }

    #[test]
    fn the_regions_send_a_wheel_to_the_main_thread() {
        let mut trees = picked();
        trees.main_thread_scroll_hit_test_region = Arc::from([RectF::new(0.0, 40.0, 20.0, 5.0)]);
        // A container that isn't a layer, in this one's content at y 100.
        trees.scroll[0].main_thread_scroll_hit_test_region =
            Arc::from([RectF::new(0.0, 100.0, 50.0, 10.0)]);
        let none = ScrollOffsets::default();
        assert_eq!(
            hit(&trees, 12.0, 42.0, &none),
            ScrollHitTest::MainThread { blocking: false }
        );
        assert_eq!(hit(&trees, 12.0, 30.0, &none), CONTAINER);
        // The one in the content comes into view as this one scrolls:
        // y 100 in it is at 120 unscrolled, 30 scrolled by 90.
        let scrolled: ScrollOffsets = [(ElementId(7), PointF::new(0.0, 90.0))].into();
        assert_eq!(
            hit(&trees, 30.0, 30.0, &scrolled),
            ScrollHitTest::MainThread { blocking: false }
        );
        assert_eq!(hit(&trees, 30.0, 45.0, &scrolled), CONTAINER);
        assert_eq!(hit(&trees, 70.0, 30.0, &scrolled), CONTAINER);
        // Only where this one shows it: not below the clip, where it is
        // unscrolled.
        trees.scroll[0].main_thread_scroll_hit_test_region =
            Arc::from([RectF::new(0.0, 0.0, 50.0, 300.0)]);
        assert_eq!(hit(&trees, 30.0, 60.0, &none), VIEWPORT);
    }

    /// The wheel event handler regions send a wheel to the main thread too,
    /// blocking (a listener may cancel it): the document's, and one in the
    /// container's content, which moves with it. Over both regions, it is
    /// blocking.
    #[test]
    fn the_wheel_event_handler_regions_send_a_wheel_to_the_main_thread_blocking() {
        let mut trees = picked();
        trees.wheel_event_handler_region = Arc::from([RectF::new(0.0, 40.0, 20.0, 5.0)]);
        trees.scroll[0].wheel_event_handler_region =
            Arc::from([RectF::new(0.0, 100.0, 50.0, 10.0)]);
        let none = ScrollOffsets::default();
        let blocking = ScrollHitTest::MainThread { blocking: true };
        assert_eq!(hit(&trees, 12.0, 42.0, &none), blocking);
        assert_eq!(hit(&trees, 12.0, 30.0, &none), CONTAINER);
        let scrolled: ScrollOffsets = [(ElementId(7), PointF::new(0.0, 90.0))].into();
        assert_eq!(hit(&trees, 30.0, 30.0, &scrolled), blocking);
        assert_eq!(hit(&trees, 30.0, 45.0, &scrolled), CONTAINER);
        trees.main_thread_scroll_hit_test_region = Arc::from([RectF::new(0.0, 40.0, 20.0, 5.0)]);
        assert_eq!(hit(&trees, 12.0, 42.0, &none), blocking);
    }

    #[test]
    fn a_wheel_picks_the_innermost_container_where_it_is_now() {
        let mut trees = picked();
        // Another at y 100 in the first one's content, 100x40.
        trees.scroll.push(ScrollTreeNode {
            parent: Some(ScrollNodeId(0)),
            element: ElementId(8),
            transform: TransformNodeId(2),
            border_box: RectF::new(0.0, 100.0, 100.0, 40.0),
            clip: ClipNodeId(1),
            scroll_translation: TransformNodeId(2),
            ..trees.scroll[0].clone()
        });
        let inner = ScrollHitTest::Scroll(Some(ScrollNodeId(1)));
        let none = ScrollOffsets::default();
        assert_eq!(hit(&trees, 30.0, 30.0, &none), CONTAINER);
        let scrolled: ScrollOffsets = [(ElementId(7), PointF::new(0.0, 90.0))].into();
        assert_eq!(hit(&trees, 30.0, 30.0, &scrolled), inner);
        assert_eq!(hit(&trees, 30.0, 25.0, &scrolled), CONTAINER, "above it");
    }

    #[test]
    fn a_clip_is_where_its_space_is_and_doesnt_scroll() {
        let trees = container();
        let offsets: ScrollOffsets = [(ElementId(7), PointF::new(0.0, 30.0))].into();
        assert_eq!(trees.clip_rect(ClipNodeId(0), &offsets), None);
        assert_eq!(
            trees.clip_rect(ClipNodeId(1), &offsets),
            Some(RectF::new(12.0, 22.0, 100.0, 50.0))
        );
    }

    /// `trees` with a sticky box's translation under `parent`: laid out
    /// at `sticky_box` in a 100x100 scrollport of `scroller`, in
    /// `containing_block`, with `inset`.
    fn add_sticky(
        trees: &mut PropertyTrees,
        parent: TransformNodeId,
        scroller: ElementId,
        sticky_box: RectF,
        containing_block: RectF,
        inset: [Option<f32>; 4],
        around: Option<TransformNodeId>,
    ) -> TransformNodeId {
        trees.transform.push(TransformNode {
            parent: Some(parent),
            offset: PointF::new(0.0, 0.0),
            scroll: None,
            sticky: Some(StickyPositionConstraint {
                scroller,
                constraining_rect: RectF::new(0.0, 0.0, 100.0, 100.0),
                sticky_box,
                containing_block,
                inset,
                shifting_containing_block: around,
                pixel_snap_offset: PointF::new(sticky_box.x, sticky_box.y),
                device_scale_factor: 1.0,
            }),
        });
        TransformNodeId(trees.transform.len() - 1)
    }

    fn scrolled(scroller: ElementId, x: f32, y: f32) -> ScrollOffsets {
        ScrollOffsets::from([(scroller, PointF::new(x, y))])
    }

    /// Each inset keeps the box inside the scrollport as it scrolls, no
    /// further than its containing block lets it; the end one first, then
    /// the start one, which wins.
    #[test]
    fn a_sticky_box_sticks_by_its_insets_inside_its_containing_block() {
        let viewport = ElementId::VIEWPORT;
        let mut trees = PropertyTrees::default();
        let root = TransformNodeId(0);
        let cb = RectF::new(0.0, 0.0, 300.0, 300.0);
        let top = add_sticky(
            &mut trees,
            root,
            viewport,
            RectF::new(0.0, 50.0, 10.0, 20.0),
            cb,
            [Some(5.0), None, None, None],
            None,
        );
        let bottom = add_sticky(
            &mut trees,
            root,
            viewport,
            RectF::new(0.0, 200.0, 10.0, 20.0),
            cb,
            [None, None, Some(5.0), None],
            None,
        );
        let sides = add_sticky(
            &mut trees,
            root,
            viewport,
            RectF::new(150.0, 0.0, 20.0, 10.0),
            cb,
            [None, Some(5.0), None, Some(5.0)],
            None,
        );
        let at = |node, x, y| trees.sticky_offset(node, &scrolled(viewport, x, y));
        assert_eq!(at(top, 0.0, 0.0), PointF::new(0.0, 0.0));
        assert_eq!(at(top, 0.0, 100.0), PointF::new(0.0, 55.0));
        // No further than the containing block's end, 300.
        assert_eq!(at(top, 0.0, 500.0), PointF::new(0.0, 230.0));
        // Up into the scrollport, no higher than the containing block's
        // top.
        assert_eq!(at(bottom, 0.0, 0.0), PointF::new(0.0, -125.0));
        assert_eq!(at(bottom, 0.0, -500.0), PointF::new(0.0, -200.0));
        assert_eq!(at(bottom, 0.0, 200.0), PointF::new(0.0, 0.0));
        // Both sides: at 150 to 170, moved left to end at 95 (the
        // scrollport's end less 5); scrolled 200 across, right to 205,
        // which wins.
        assert_eq!(at(sides, 0.0, 0.0), PointF::new(-75.0, 0.0));
        assert_eq!(at(sides, 200.0, 0.0), PointF::new(55.0, 0.0));
        // The offsets of others don't move it.
        assert_eq!(
            trees.sticky_offset(top, &scrolled(ElementId(7), 0.0, 100.0)),
            PointF::new(0.0, 0.0)
        );
    }

    /// A sticky box in another moves with it: its containing block is
    /// moved by the outer box's offset, and the outer box's by those
    /// around it.
    #[test]
    fn a_sticky_box_moves_with_the_sticky_boxes_around_it() {
        let viewport = ElementId::VIEWPORT;
        let mut trees = PropertyTrees::default();
        let root = TransformNodeId(0);
        let body = RectF::new(0.0, 0.0, 100.0, 2700.0);
        let a = add_sticky(
            &mut trees,
            root,
            viewport,
            RectF::new(0.0, 100.0, 100.0, 600.0),
            body,
            [Some(0.0), None, None, None],
            None,
        );
        let b = add_sticky(
            &mut trees,
            a,
            viewport,
            RectF::new(0.0, 200.0, 100.0, 300.0),
            RectF::new(0.0, 100.0, 100.0, 600.0),
            [Some(0.0), None, None, None],
            Some(a),
        );
        let c = add_sticky(
            &mut trees,
            b,
            viewport,
            RectF::new(0.0, 300.0, 100.0, 50.0),
            RectF::new(0.0, 200.0, 100.0, 300.0),
            [Some(0.0), None, None, None],
            Some(b),
        );
        let offsets = |y| {
            let scroll = scrolled(viewport, 0.0, y);
            [a, b, c].map(|node| trees.sticky_offset(node, &scroll).y)
        };
        assert_eq!(offsets(400.0), [300.0, 0.0, 0.0]);
        assert_eq!(offsets(2500.0), [2000.0, 200.0, 0.0]);
        // Each translation by its own offset: `c` is where `a` and `b`
        // put it, at the top.
        let at = trees.position(c, &scrolled(viewport, 0.0, 2500.0));
        assert_eq!(at, PointF::new(0.0, 2200.0));
    }

    /// The offset lands where the main thread paints the box: snapped
    /// with the box's place, not alone.
    #[test]
    fn a_sticky_offset_is_snapped_with_its_box() {
        let mut trees = PropertyTrees::default();
        let node = add_sticky(
            &mut trees,
            TransformNodeId(0),
            ElementId(7),
            RectF::new(0.0, 10.5, 100.0, 50.0),
            RectF::new(0.0, 0.0, 100.0, 1000.0),
            [Some(0.0), None, None, None],
            None,
        );
        let scroll = scrolled(ElementId(7), 0.0, 20.0);
        assert_eq!(trees.sticky_offset(node, &scroll), PointF::new(0.0, 9.5));
        // The box at 10.5 is painted at 11: 11 + 9 is 20, where it sticks.
        assert_eq!(trees.position(node, &scroll), PointF::new(0.0, 9.0));
    }

    /// The regions in a sticky box's space move with it: a wheel over
    /// them goes to the main thread where the box is now.
    #[test]
    fn a_sticky_boxs_regions_move_with_it() {
        let mut trees = picked();
        let node = add_sticky(
            &mut trees,
            TransformNodeId(0),
            ElementId::VIEWPORT,
            RectF::new(200.0, 50.0, 100.0, 20.0),
            RectF::new(0.0, 0.0, 800.0, 1000.0),
            [Some(0.0), None, None, None],
            None,
        );
        trees.sticky_regions.push(StickyRegions {
            transform: node,
            main_thread_scroll_hit_test_region: Arc::from([RectF::new(200.0, 50.0, 100.0, 20.0)]),
            wheel_event_handler_region: Arc::from([RectF::new(250.0, 50.0, 10.0, 20.0)]),
        });
        let scroll = scrolled(ElementId::VIEWPORT, 0.0, 300.0);
        // Laid out at 50; stuck at 300 with the viewport scrolled 300.
        assert_eq!(
            hit(&trees, 210.0, 55.0, &Default::default()),
            ScrollHitTest::MainThread { blocking: false }
        );
        assert_eq!(
            hit(&trees, 210.0, 55.0, &scroll),
            ScrollHitTest::Scroll(None)
        );
        assert_eq!(
            hit(&trees, 210.0, 305.0, &scroll),
            ScrollHitTest::MainThread { blocking: false }
        );
        assert_eq!(
            hit(&trees, 255.0, 305.0, &scroll),
            ScrollHitTest::MainThread { blocking: true }
        );
    }

    /// A rect of `frame` (`None`: the main frame's) at `order`.
    fn of(frame: Option<usize>, order: u32, rect: RectF) -> AttributionRect {
        AttributionRect {
            rect,
            space: AttributionSpace::Document,
            order,
            frame,
            masked: false,
        }
    }

    /// Child frames 0 (in the main frame) and 1 (in 0), with `rects`.
    fn attributed(rects: Vec<AttributionRect>) -> PropertyTrees {
        PropertyTrees {
            frame_attribution: FrameAttributionRegion {
                frames: Arc::from([
                    AttributedFrame {
                        document: ElementId(100),
                        parent: None,
                    },
                    AttributedFrame {
                        document: ElementId(101),
                        parent: Some(0),
                    },
                ]),
                rects: rects.into(),
            },
            ..PropertyTrees::default()
        }
    }

    /// Input is for the frame of the rect at the point painted last, the
    /// main frame's where there is none; no frame is told where another
    /// rect there is of a frame that isn't that one or around it (cc's
    /// `FindClosestMatchingLayerForAttribution`), or a masked canvas is on
    /// top.
    #[test]
    fn input_is_for_the_frame_on_top() {
        let none = ScrollOffsets::default();
        let at = |trees: &PropertyTrees, x: f32, y: f32| {
            trees.frame_attribution(PointF::new(x, y), &none)
        };
        let main = InputAttribution::TargetedFrame(None);
        let child = InputAttribution::TargetedFrame(Some(ElementId(100)));
        let grandchild = InputAttribution::TargetedFrame(Some(ElementId(101)));
        assert_eq!(at(&PropertyTrees::default(), 5.0, 5.0), main);
        let canvas = RectF::new(0.0, 0.0, 100.0, 60.0);
        let trees = attributed(vec![
            of(Some(0), 1, canvas),
            of(Some(1), 2, RectF::new(0.0, 0.0, 50.0, 30.0)),
            // The child frame's own box over the grandchild: the
            // grandchild isn't around it.
            of(Some(0), 3, RectF::new(40.0, 20.0, 5.0, 5.0)),
            // The main frame's box over the child frame.
            of(None, 4, RectF::new(80.0, 40.0, 10.0, 10.0)),
            // And beside it.
            of(None, 5, RectF::new(200.0, 0.0, 10.0, 10.0)),
        ]);
        assert_eq!(at(&trees, 70.0, 50.0), child, "the child frame's canvas");
        assert_eq!(
            at(&trees, 10.0, 10.0),
            grandchild,
            "the main frame and the child around it"
        );
        assert_eq!(
            at(&trees, 42.0, 22.0),
            InputAttribution::Unknown,
            "over the grandchild"
        );
        assert_eq!(
            at(&trees, 85.0, 45.0),
            InputAttribution::Unknown,
            "over the child"
        );
        assert_eq!(at(&trees, 205.0, 5.0), main, "the main frame's alone");
        assert_eq!(at(&trees, 300.0, 300.0), main, "nothing there");
        let masked = attributed(vec![AttributionRect {
            masked: true,
            ..of(Some(0), 1, canvas)
        }]);
        assert_eq!(at(&masked, 10.0, 10.0), InputAttribution::Unknown);
    }

    /// A rect in a container's space moves with its scroll offset, and
    /// counts only where the container is picked (its border box, cut by
    /// its clips).
    #[test]
    fn a_rect_in_a_container_moves_with_it() {
        let mut trees = picked();
        trees.frame_attribution = attributed(vec![AttributionRect {
            space: AttributionSpace::Scroller(ScrollNodeId(0)),
            ..of(Some(0), 1, RectF::new(0.0, 0.0, 100.0, 40.0))
        }])
        .frame_attribution;
        let child = InputAttribution::TargetedFrame(Some(ElementId(100)));
        let main = InputAttribution::TargetedFrame(None);
        let mut offsets = ScrollOffsets::default();
        // The container is at (10, 20); its content's (0, 0) there.
        assert_eq!(
            trees.frame_attribution(PointF::new(15.0, 25.0), &offsets),
            child
        );
        // Past the root's clip (y 50) the container isn't picked.
        assert_eq!(
            trees.frame_attribution(PointF::new(15.0, 55.0), &offsets),
            main
        );
        // Scrolled 30px: (15, 25) is the content's (5, 35), in the rect;
        // (15, 45) its (5, 55), past it.
        offsets.insert(ElementId(7), PointF::new(0.0, 30.0));
        assert_eq!(
            trees.frame_attribution(PointF::new(15.0, 25.0), &offsets),
            child
        );
        assert_eq!(
            trees.frame_attribution(PointF::new(15.0, 45.0), &offsets),
            main
        );
        // One further down the content, at y 60 to 80: under (15, 35) only
        // once scrolled 50.
        trees.frame_attribution = attributed(vec![AttributionRect {
            space: AttributionSpace::Scroller(ScrollNodeId(0)),
            ..of(Some(0), 1, RectF::new(0.0, 60.0, 100.0, 20.0))
        }])
        .frame_attribution;
        offsets.insert(ElementId(7), PointF::new(0.0, 0.0));
        assert_eq!(
            trees.frame_attribution(PointF::new(15.0, 35.0), &offsets),
            main
        );
        offsets.insert(ElementId(7), PointF::new(0.0, 50.0));
        assert_eq!(
            trees.frame_attribution(PointF::new(15.0, 35.0), &offsets),
            child
        );
    }

    /// A rect in a sticky box's space moves as the box sticks.
    #[test]
    fn a_rect_in_a_sticky_box_moves_with_it() {
        let mut trees = PropertyTrees::default();
        let node = add_sticky(
            &mut trees,
            TransformNodeId(0),
            ElementId::VIEWPORT,
            RectF::new(200.0, 50.0, 100.0, 20.0),
            RectF::new(0.0, 0.0, 800.0, 1000.0),
            [Some(0.0), None, None, None],
            None,
        );
        trees.frame_attribution = attributed(vec![AttributionRect {
            space: AttributionSpace::Sticky(node),
            ..of(Some(0), 1, RectF::new(200.0, 50.0, 100.0, 20.0))
        }])
        .frame_attribution;
        let child = InputAttribution::TargetedFrame(Some(ElementId(100)));
        let main = InputAttribution::TargetedFrame(None);
        // Laid out at 50; stuck at 300 with the viewport scrolled 300.
        let scroll = scrolled(ElementId::VIEWPORT, 0.0, 300.0);
        assert_eq!(
            trees.frame_attribution(PointF::new(210.0, 55.0), &Default::default()),
            child
        );
        assert_eq!(
            trees.frame_attribution(PointF::new(210.0, 55.0), &scroll),
            main
        );
        assert_eq!(
            trees.frame_attribution(PointF::new(210.0, 305.0), &scroll),
            child
        );
    }
}
