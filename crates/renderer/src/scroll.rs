//! Scrolling (Blink's `ScrollableArea`s, and the scroll part of
//! `EventHandler`): the viewport's offset, which the compositor applies,
//! and each scroll container's, which pre-paint turns into a scroll
//! translation. Wheels over a container (which the compositor passes on)
//! scroll the innermost one under the pointer and pass what it can't take
//! to the ones around it, then the viewport (scroll chaining, without
//! latching). A scrolling key scrolls one box, from the focused element
//! out (`keyboard`). The compositor scrolls the viewport itself, by wheels
//! elsewhere, and so the containers that are layers, and hands that over
//! (`apply_compositor_delta`).
//!
//! How far each scroller moves is the compositor's math
//! (`compositor::ScrollNode`), which the compositor scrolls with too: this
//! module finds the scrollers and turns them into its nodes, as Blink's
//! `PropertyTreeManager` turns paint's scroll nodes into cc's.

use compositor::{ElementId, ScrollKey, ScrollNode, key_scroll_delta};
use computed_style::Overflow;
use gfx::{NodeId, NodeIdHashMap, PointF, RectF, ScrollAlignment, ScrollIntoViewParams, SizeF};
use layout::{BoxFragment, StickyPositions, StickyState};
use paint::{PrePaint, ScrollOffsets};

use crate::compositing;

/// How following a link to `#target` shows it (Blink's `FragmentAnchor`):
/// its top at the top, and sideways only if it isn't in view.
pub(crate) const FRAGMENT_ANCHOR: ScrollIntoViewParams = ScrollIntoViewParams {
    block: ScrollAlignment::Start,
    inline: ScrollAlignment::Nearest,
    nearest_only: false,
};

/// How an element is scrolled into view in its scroll container alone (a
/// list box's option: Blink's `ScrollAlignment::ToEdgeIfNeeded` on both
/// axes, nothing around the list box scrolled).
pub(crate) const IN_CONTAINER: ScrollIntoViewParams = ScrollIntoViewParams {
    block: ScrollAlignment::Nearest,
    inline: ScrollAlignment::Nearest,
    nearest_only: true,
};

/// The offsets change only through [`ScrollState::set_viewport`] and
/// [`ScrollState::set_container`], which note what moved for its `scroll`
/// event and drop the scroller's scroll anchor (Blink's
/// `PaintLayerScrollableArea::UpdateScrollOffset` clears it for an
/// explicit scroll), and the clamp and scroll anchoring's own moves, which
/// keep it.
#[derive(Debug, Default)]
pub(crate) struct ScrollState {
    /// The viewport's offset, CSS px.
    pub(crate) viewport: PointF,
    /// Each scroll container's, by element.
    pub(crate) containers: ScrollOffsets,
    /// The viewport as a scroller, from the last layout; `None` before the
    /// first, when nothing can scroll.
    pub(crate) viewport_node: Option<ScrollNode>,
    /// What moved since the last scroll steps, in the order it first did,
    /// each once: the viewport (`None`) or a container (Blink's
    /// `Document::EnqueueScrollEventForNode`, its per-frame queue).
    scrolled: Vec<Option<NodeId>>,
    /// Something moved since the last pre-paint (Blink's
    /// `SetNeedsPaintPropertyUpdate` on a scroll).
    pub(crate) needs_pre_paint: bool,
    /// The sticky boxes of the last layout (Blink's sticky constraints,
    /// which `ExecutePendingStickyUpdates` works out after layout).
    pub(crate) sticky: StickyPositions,
    /// How far each moves with the offsets now: worked out again from
    /// `sticky` whenever something scrolls (Blink's
    /// `InvalidatePaintForStickyDescendants`); and how far each may.
    pub(crate) stuck: StickyState,
    /// Each scroller's scroll anchor (`crate::scroll_anchor`): the
    /// viewport's (`None`, first) and containers', by element.
    pub(crate) anchors: std::collections::BTreeMap<Option<NodeId>, ScrollAnchor>,
}

/// A scroller's scroll anchor (Blink's `ScrollAnchor`): the box or text,
/// and how far its top was below the top of what the scroller showed when
/// it was picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ScrollAnchor {
    pub(crate) node: layout::ScrollAnchorNode,
    pub(crate) offset: f32,
}

/// The viewport's scroller: `viewport` over the `document`, with the
/// viewport's `overflow` (the box tree's, from the root element or
/// `<body>`). Unlike a scroll container's, only `hidden` stops the user
/// scrolling it: a `visible` one scrolls.
pub(crate) fn viewport_scroll_node(
    viewport: SizeF,
    document: SizeF,
    overflow: (Overflow, Overflow),
) -> ScrollNode {
    ScrollNode {
        container_bounds: viewport,
        bounds: document,
        user_scrollable_horizontal: overflow.0 != Overflow::Hidden,
        user_scrollable_vertical: overflow.1 != Overflow::Hidden,
    }
}

/// What a scroll changed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scrolled {
    /// The viewport moved: the compositor draws again from its tiles.
    pub(crate) viewport: bool,
    /// A container moved: pre-paint and paint run again.
    pub(crate) containers: bool,
}

impl ScrollState {
    /// Moves the viewport to `offset`, a scroll by the user, script or the
    /// compositor: its anchor goes. Whether it moved.
    pub(crate) fn set_viewport(&mut self, offset: PointF) -> bool {
        let moved = self.move_viewport(offset);
        if moved {
            self.anchors.remove(&None);
        }
        moved
    }

    /// Moves container `element` to `offset`, as `set_viewport` the
    /// viewport.
    pub(crate) fn set_container(&mut self, element: NodeId, offset: PointF) -> bool {
        let moved = self.move_container(element, offset);
        if moved {
            self.anchors.remove(&Some(element));
        }
        moved
    }

    /// Moves `scroller` (`None`: the viewport) to `offset` keeping its
    /// anchor: scroll anchoring's adjustment.
    pub(crate) fn move_scroller(&mut self, scroller: Option<NodeId>, offset: PointF) -> bool {
        match scroller {
            None => self.move_viewport(offset),
            Some(element) => self.move_container(element, offset),
        }
    }

    fn move_viewport(&mut self, offset: PointF) -> bool {
        if offset == self.viewport {
            return false;
        }
        self.viewport = offset;
        self.note_scrolled(None);
        true
    }

    fn move_container(&mut self, element: NodeId, offset: PointF) -> bool {
        let current = self.containers.get(&element).copied().unwrap_or_default();
        if offset == current {
            return false;
        }
        self.containers.insert(element, offset);
        self.note_scrolled(Some(element));
        true
    }

    /// The sticky boxes of a new layout.
    pub(crate) fn set_sticky(&mut self, sticky: StickyPositions) {
        self.sticky = sticky;
        self.stuck = StickyState {
            offsets: self.sticky.offsets(&self.containers, self.viewport),
            reaches: self.sticky.reaches(),
        };
    }

    fn note_scrolled(&mut self, target: Option<NodeId>) {
        self.needs_pre_paint = true;
        if !self.sticky.is_empty() {
            self.stuck.offsets = self.sticky.offsets(&self.containers, self.viewport);
        }
        if !self.scrolled.contains(&target) {
            self.scrolled.push(target);
        }
    }

    /// What moved since last asked, for the scroll steps: the viewport
    /// (`None`) or containers, in order.
    pub(crate) fn take_scrolled(&mut self) -> Vec<Option<NodeId>> {
        std::mem::take(&mut self.scrolled)
    }

    /// Whether something moved that the scroll steps haven't seen.
    pub(crate) fn has_scrolled(&self) -> bool {
        !self.scrolled.is_empty()
    }

    /// `scroller` scrolls `target` (the viewport, `None`, or a container)
    /// by as much of `rest` as it can (`ScrollNode::scroll_by`), taking it
    /// from `rest`. Whether it moved.
    pub(crate) fn scroll_by(
        &mut self,
        target: Option<NodeId>,
        scroller: &ScrollNode,
        rest: &mut PointF,
    ) -> bool {
        let mut offset = match target {
            None => self.viewport,
            Some(element) => self.containers.get(&element).copied().unwrap_or_default(),
        };
        if !scroller.scroll_by(&mut offset, rest) {
            return false;
        }
        match target {
            None => self.set_viewport(offset),
            Some(element) => self.set_container(element, offset),
        };
        true
    }

    /// Keeps every offset within its range, after a layout changed what
    /// can be scrolled (`viewport_node` first): one clamped moved, and
    /// gets its `scroll` event (Blink's `kClamping` scroll). A container
    /// that is gone is dropped.
    pub(crate) fn clamp(&mut self, view: &BoxFragment) {
        if let Some(node) = self.viewport_node {
            self.move_viewport(node.clamp_scroll_offset(self.viewport));
        }
        let mut containers = NodeIdHashMap::default();
        container_nodes(view, &mut containers);
        self.containers
            .retain(|element, _| containers.contains_key(element));
        let offsets: Vec<(NodeId, PointF)> =
            self.containers.iter().map(|(e, o)| (*e, *o)).collect();
        for (element, offset) in offsets {
            self.move_container(element, containers[&element].clamp_scroll_offset(offset));
        }
    }

    /// Script scrolls `target` (`None`: the viewport) to `offset`, clamped
    /// to what its scroller can scroll now, whatever the user may (Blink's
    /// `ScrollableArea::SetProgrammaticScrollOffset`). Nothing for an
    /// element that isn't a scroll container, or before the first layout.
    /// Whether it moved.
    pub(crate) fn scroll_to(
        &mut self,
        view: &BoxFragment,
        target: Option<NodeId>,
        offset: PointF,
    ) -> bool {
        match target {
            None => {
                let Some(node) = self.viewport_node else {
                    return false;
                };
                self.set_viewport(node.clamp_scroll_offset(offset))
            }
            Some(element) => {
                let mut containers = NodeIdHashMap::default();
                container_nodes(view, &mut containers);
                let Some(&node) = containers.get(&element) else {
                    return false;
                };
                self.set_container(element, node.clamp_scroll_offset(offset))
            }
        }
    }

    /// A wheel at `point` (viewport CSS px) scrolling by `delta`: the
    /// innermost scroll container under it that can move that way takes
    /// what it can, the ones around it the rest, then the viewport. The
    /// main frame's; a container in a child frame (or its viewport) is
    /// `child_frame`'s to scroll, by its iframe and element, taking what
    /// it can of the rest; whether it moved.
    pub(crate) fn wheel(
        &mut self,
        pre: &PrePaint,
        view: &BoxFragment,
        point: PointF,
        delta: PointF,
        child_frame: &mut dyn FnMut(NodeId, NodeId, &ScrollNode, &mut PointF) -> bool,
    ) -> Scrolled {
        let mut scrolled = Scrolled::default();
        let mut rest = delta;
        // Hit testing is in the document's space: add the viewport's
        // offset to the point. What is hit there, as painted (clipped,
        // transformed, scrolled), picks the container.
        let at = PointF::new(point.x + self.viewport.x, point.y + self.viewport.y);
        let mut node = paint::hit_test(view, pre, at).scroll.map(|id| id.0);
        while let Some(index) = node {
            let n = pre.trees.scroll(paint::ScrollNodeId(index));
            let scroller = compositing::scroll_node(n);
            scrolled.containers |= match n.frame {
                None => self.scroll_by(Some(n.element), &scroller, &mut rest),
                Some(iframe) => child_frame(iframe, n.element, &scroller, &mut rest),
            };
            node = n.parent.map(|p| p.0);
        }
        if let Some(viewport) = self.viewport_node {
            scrolled.viewport |= self.scroll_by(None, &viewport, &mut rest);
        }
        scrolled
    }

    /// A scrolling key (Blink's `ScrollManager::LogicalScroll`): from the
    /// innermost scroll container around the first of `chain` that has one
    /// (the focused element and its ancestors, out through the frames),
    /// the first that can move that way moves by the key's step for it,
    /// else the viewport. Not split among them, as a wheel's delta is: a
    /// key scrolls one box. Containers in child frames (and their
    /// viewports) are `child_frame`'s, as for `wheel`.
    pub(crate) fn keyboard(
        &mut self,
        pre: &PrePaint,
        chain: &[NodeId],
        key: ScrollKey,
        shift: bool,
        child_frame: &mut dyn FnMut(NodeId, NodeId, &ScrollNode, &mut PointF) -> bool,
    ) -> Scrolled {
        let mut scrolled = Scrolled::default();
        let mut node = chain.iter().find_map(|id| {
            pre.trees
                .scroll
                .iter()
                .position(|node| node.as_ref().is_some_and(|node| node.element == *id))
        });
        while let Some(index) = node {
            let n = pre.trees.scroll(paint::ScrollNodeId(index));
            let scroller = compositing::scroll_node(n);
            let mut step = key_scroll_delta(key, shift, &scroller);
            let moved = match n.frame {
                None => self.scroll_by(Some(n.element), &scroller, &mut step),
                Some(iframe) => child_frame(iframe, n.element, &scroller, &mut step),
            };
            if moved {
                scrolled.containers = true;
                return scrolled;
            }
            node = n.parent.map(|p| p.0);
        }
        if let Some(viewport) = self.viewport_node {
            let mut step = key_scroll_delta(key, shift, &viewport);
            scrolled.viewport = self.scroll_by(None, &viewport, &mut step);
        }
        scrolled
    }

    /// What the compositor scrolled the viewport and the containers that
    /// are layers by, applied (Blink setting the offsets with
    /// `ScrollType::kCompositor`): each clamped to what its scroller can
    /// scroll now, whatever the user may, as the compositor clamps what it
    /// hands over. A container `view` no longer has is left.
    pub(crate) fn apply_compositor_delta(
        &mut self,
        view: &BoxFragment,
        viewport: PointF,
        containers: &[(ElementId, PointF)],
    ) {
        if let Some(node) = self.viewport_node {
            self.set_viewport(node.clamp_scroll_offset(add(self.viewport, viewport)));
        }
        if containers.is_empty() {
            return;
        }
        let mut nodes = NodeIdHashMap::default();
        container_nodes(view, &mut nodes);
        for (element, node) in nodes {
            let Some((_, delta)) = containers
                .iter()
                .find(|(id, _)| *id == compositing::element_id(element))
            else {
                continue;
            };
            let offset = self.containers.get(&element).copied().unwrap_or_default();
            self.set_container(element, node.clamp_scroll_offset(add(offset, *delta)));
        }
    }

    /// Scrolls `target` into view (Blink's `ScrollRectToVisible` in one
    /// frame): every scroll container around it, innermost first, then the
    /// viewport, each to show it as `params` says, as far as each can
    /// scroll; only the first with `nearest_only`. `inner`: a rect in the
    /// target's content box to show instead of its border box (a child
    /// frame's part of an iframe). Scripts' and fragments' scrolling isn't
    /// limited to what the user may scroll: `overflow: hidden` moves too.
    /// Where the target then is in the viewport; `None` if it has no box.
    pub(crate) fn scroll_into_view(
        &mut self,
        view: &BoxFragment,
        target: NodeId,
        inner: Option<RectF>,
        params: ScrollIntoViewParams,
    ) -> Option<RectF> {
        let mut scrolls = true;
        let rect = self.bring_into_view(view, target, inner, params, &mut scrolls)?;
        if scrolls && let Some(viewport) = self.viewport_node {
            self.set_viewport(expose(self.viewport, rect, &viewport, params));
        }
        Some(rect.offset(-self.viewport.x, -self.viewport.y))
    }

    /// Finds `target` in `fragment`, scrolls the containers inside and
    /// including `fragment` for it while `scrolls`, and returns where it
    /// then is, from `fragment`'s border box.
    fn bring_into_view(
        &mut self,
        fragment: &BoxFragment,
        target: NodeId,
        inner: Option<RectF>,
        params: ScrollIntoViewParams,
        scrolls: &mut bool,
    ) -> Option<RectF> {
        let found = if fragment.node == Some(target) {
            // The target itself: not scrolled for its own sake.
            let (b, p) = (&fragment.border, &fragment.padding);
            return Some(match inner {
                Some(rect) => rect.offset(b.left + p.left, b.top + p.top),
                None => RectF::new(0.0, 0.0, fragment.size.width, fragment.size.height),
            });
        } else if let Some(rect) = in_lines(fragment, target) {
            rect
        } else {
            fragment.children.iter().find_map(|child| {
                self.bring_into_view(&child.fragment, target, inner, params, scrolls)
                    .map(|r| r.offset(child.offset.x, child.offset.y))
            })?
        };
        // A fixed box moves with the viewport: scrolling what is around it
        // brings nothing nearer, so nothing past it scrolls (Blink's
        // scroll into view stops at a box fixed to the view). Its own
        // overflow still scrolls.
        let fixed = fragment.style.position == computed_style::Position::Fixed;
        let Some((element, node)) = scroll_container(fragment) else {
            if fixed {
                *scrolls = false;
            }
            return Some(found);
        };
        let current = self
            .containers
            .get(&element)
            .copied()
            .unwrap_or(PointF::new(0.0, 0.0));
        let offset = if *scrolls {
            // From the padding box, unscrolled.
            let in_content = found.offset(-fragment.border.left, -fragment.border.top);
            let offset = expose(current, in_content, &node, params);
            self.set_container(element, offset);
            *scrolls = !params.nearest_only;
            offset
        } else {
            current
        };
        if fixed {
            *scrolls = false;
        }
        Some(found.offset(-offset.x, -offset.y))
    }
}

fn add(a: PointF, b: PointF) -> PointF {
    PointF::new(a.x + b.x, a.y + b.y)
}

/// The offset that shows `rect` (in the scroller's content, unscrolled)
/// as `params` asks (Blink's `GetScrollOffsetToExpose`), clamped to what
/// `node` can scroll, whatever the user may.
fn expose(current: PointF, rect: RectF, node: &ScrollNode, params: ScrollIntoViewParams) -> PointF {
    let port = node.container_bounds;
    node.clamp_scroll_offset(PointF::new(
        expose_axis(current.x, port.width, rect.x, rect.width, params.inline),
        expose_axis(current.y, port.height, rect.y, rect.height, params.block),
    ))
}

/// One axis of [`expose`]: the offset of a port `port` long, now at
/// `current`, that puts the span from `start`, `size` long, where `align`
/// says.
fn expose_axis(current: f32, port: f32, start: f32, size: f32, align: ScrollAlignment) -> f32 {
    let end = start + size;
    match align {
        ScrollAlignment::Start => start,
        ScrollAlignment::Center => (start + end - port) / 2.0,
        ScrollAlignment::End => end - port,
        ScrollAlignment::Nearest | ScrollAlignment::CenterIfNeeded => {
            // By the edges, not by how much is shown: a difference of
            // `f32`s far down a page is off in its last bits, and a span
            // just in view would count as partly out.
            let port_end = current + port;
            if (start >= current && end <= port_end) || (start <= current && end >= port_end) {
                // All of it in view, or it fills the view: as it is.
                current
            } else if align == ScrollAlignment::CenterIfNeeded
                && (end <= current || start >= port_end)
            {
                // None of it in view (touching the edge isn't).
                (start + end - port) / 2.0
            } else if (end > current + port && size < port) || (end < current + port && size > port)
            {
                // The closest edge is the end: past the end and smaller
                // than the port, or before it and larger.
                end - port
            } else {
                start
            }
        }
    }
}

/// The rect around `target`'s boxes on `fragment`'s lines, from its
/// border box (Blink's `AbsoluteBoundingBoxRectForScrollIntoView`). An
/// atomic inline is one of its children.
fn in_lines(fragment: &BoxFragment, target: NodeId) -> Option<RectF> {
    fragment
        .lines
        .iter()
        .flat_map(|line| line.boxes.iter().filter(|b| b.node == target))
        .map(|b| b.rect)
        .reduce(|a, b| a.union(&b))
}

/// The element and its scroller if `fragment` is a scroll container: its
/// padding box over its scrollable overflow. The one whose `overflow` went
/// to the viewport isn't: layout made its own `visible`.
fn scroll_container(fragment: &BoxFragment) -> Option<(NodeId, ScrollNode)> {
    if !fragment.style.is_scroll_container() {
        return None;
    }
    let b = &fragment.border;
    let port = SizeF::new(
        fragment.size.width - b.left - b.right,
        fragment.size.height - b.top - b.bottom,
    );
    let node = ScrollNode {
        container_bounds: port,
        bounds: layout::scrollable_overflow(fragment),
        user_scrollable_horizontal: fragment.style.overflow_x.user_scrollable(),
        user_scrollable_vertical: fragment.style.overflow_y.user_scrollable(),
    };
    Some((fragment.node?, node))
}

/// Each scroll container's scroller.
fn container_nodes(fragment: &BoxFragment, out: &mut NodeIdHashMap<ScrollNode>) {
    if let Some((element, node)) = scroll_container(fragment) {
        out.insert(element, node);
    }
    for child in &fragment.children {
        container_nodes(&child.fragment, out);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use super::*;

    const VIEWPORT: SizeF = SizeF::new(200.0, 100.0);

    /// Lays out `html` at 200x100 and returns the fragment tree, the ids'
    /// elements, the document's size and the viewport's `overflow`.
    fn lay_out(
        html: &str,
    ) -> (
        Arc<BoxFragment>,
        HashMap<String, NodeId>,
        SizeF,
        (Overflow, Overflow),
    ) {
        let (mut tree, ids) = crate::testing::box_tree(html);
        let view = tree.layout(VIEWPORT);
        let size = layout::document_size(&view);
        (view, ids, size, tree.viewport_overflow())
    }

    /// A 1000px-high page with a 100x50 scroll container at the top whose
    /// content is 250px high.
    const PAGE: &str = "<style>html, body { margin: 0 }</style>
        <div id=box style='width: 100px; height: 50px; overflow: auto'><div style='height: 250px'></div></div>
        <div style='height: 950px'></div>";

    /// What `FrameView` keeps after laying out a document of `size` with
    /// the viewport's `overflow`: nothing scrolled yet.
    fn laid_out(size: SizeF, overflow: (Overflow, Overflow)) -> ScrollState {
        ScrollState {
            viewport_node: Some(viewport_scroll_node(VIEWPORT, size, overflow)),
            ..Default::default()
        }
    }

    fn wheel(state: &mut ScrollState, view: &BoxFragment, at: (f32, f32), dy: f32) -> Scrolled {
        let pre = paint::pre_paint(view, &state.containers, 1.0);
        state.wheel(
            &pre,
            view,
            PointF::new(at.0, at.1),
            PointF::new(0.0, dy),
            &mut |_, _, _, _| unreachable!("no child frames"),
        )
    }

    /// What moved is noted for its `scroll` event, once each until the
    /// scroll steps take them, in the order it first moved: by script, by
    /// the compositor, by layout's clamping; nothing that didn't move.
    #[test]
    fn what_moved_is_noted_for_its_scroll_event() {
        let (view, ids, size, overflow) = lay_out(PAGE);
        let element = ids["box"];
        let mut state = laid_out(size, overflow);
        assert!(!state.has_scrolled());
        state.scroll_to(&view, Some(element), PointF::new(0.0, 10.0));
        state.scroll_to(&view, None, PointF::new(0.0, 20.0));
        state.scroll_to(&view, Some(element), PointF::new(0.0, 30.0));
        // Where it is already: nothing.
        state.scroll_to(&view, None, PointF::new(0.0, 20.0));
        assert!(state.has_scrolled());
        assert_eq!(state.take_scrolled(), [Some(element), None]);
        assert!(!state.has_scrolled());

        state.apply_compositor_delta(
            &view,
            PointF::new(0.0, 5.0),
            &[(compositing::element_id(element), PointF::new(0.0, 0.0))],
        );
        assert_eq!(state.take_scrolled(), [None]);

        // A layout where the container can scroll less: clamped, moved.
        state.set_container(element, PointF::new(0.0, 900.0));
        state.take_scrolled();
        state.clamp(&view);
        assert_eq!(state.containers[&element], PointF::new(0.0, 200.0));
        assert_eq!(state.take_scrolled(), [Some(element)]);
        state.clamp(&view);
        assert!(!state.has_scrolled(), "nothing to clamp");
    }

    /// Script scrolls a container or the viewport to an offset, clamped to
    /// what it can scroll; `overflow: hidden` keeps the user out, not
    /// script. An element that isn't a scroll container, and a viewport
    /// never laid out, don't move.
    #[test]
    fn script_scrolls_to_an_offset_clamped() {
        let (view, ids, size, overflow) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div id=box style='width: 100px; height: 50px; overflow: auto'><div style='height: 250px'></div></div>
             <div id=hidden style='width: 100px; height: 50px; overflow: hidden'><div style='height: 80px'></div></div>
             <div id=plain style='height: 950px'></div>",
        );
        let mut state = laid_out(size, overflow);
        let to = |state: &mut ScrollState, target: Option<&str>, x: f32, y: f32| {
            state.scroll_to(&view, target.map(|t| ids[t]), PointF::new(x, y))
        };
        assert!(to(&mut state, Some("box"), 5.0, 120.0));
        assert_eq!(state.containers[&ids["box"]], PointF::new(0.0, 120.0));
        // Past the end, and before the start: to the end, to 0.
        assert!(to(&mut state, Some("box"), 0.0, 1000.0));
        assert_eq!(state.containers[&ids["box"]], PointF::new(0.0, 200.0));
        assert!(!to(&mut state, Some("box"), -3.0, 1000.0), "no move");
        assert!(to(&mut state, Some("hidden"), 0.0, 99.0));
        assert_eq!(state.containers[&ids["hidden"]], PointF::new(0.0, 30.0));
        assert!(!to(&mut state, Some("plain"), 0.0, 10.0));
        assert!(!state.containers.contains_key(&ids["plain"]));
        // The viewport: the document is 1050px high in 100.
        assert!(to(&mut state, None, 0.0, 2000.0));
        assert_eq!(state.viewport, PointF::new(0.0, 950.0));
        let mut fresh = ScrollState::default();
        assert!(!fresh.scroll_to(&view, None, PointF::new(0.0, 10.0)));
    }

    #[test]
    fn a_wheel_scrolls_the_container_under_it_then_the_viewport() {
        let (view, ids, size, _) = lay_out(PAGE);
        let box_ = ids["box"];
        let mut state = laid_out(size, Default::default());
        // Over the container: it takes the scroll.
        let s = wheel(&mut state, &view, (10.0, 10.0), 150.0);
        assert_eq!(
            s,
            Scrolled {
                viewport: false,
                containers: true
            }
        );
        assert_eq!(state.containers[&box_].y, 150.0);
        // Past its end (200): the rest goes to the viewport.
        let s = wheel(&mut state, &view, (10.0, 10.0), 100.0);
        assert_eq!(
            s,
            Scrolled {
                viewport: true,
                containers: true
            }
        );
        assert_eq!((state.containers[&box_].y, state.viewport.y), (200.0, 50.0));
        // Outside it: only the viewport, up to the document's end (900).
        let s = wheel(&mut state, &view, (150.0, 10.0), 5000.0);
        assert_eq!(
            s,
            Scrolled {
                viewport: true,
                containers: false
            }
        );
        assert_eq!(state.viewport.y, 900.0);
        // Nothing left to move.
        assert_eq!(
            wheel(&mut state, &view, (150.0, 10.0), 10.0),
            Scrolled::default()
        );
    }

    #[test]
    fn a_wheel_goes_from_an_inner_container_to_the_outer_one() {
        // A 100px-high outer container (300 of content) holding a 50px
        // inner one (150 of content) at its top.
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div id=outer style='width: 100px; height: 100px; overflow: auto'>
               <div id=inner style='height: 50px; overflow: auto'><div style='height: 150px'></div></div>
               <div style='height: 250px'></div>
             </div>",
        );
        let mut state = laid_out(size, Default::default());
        // The inner one can take 100; the outer the other 30.
        wheel(&mut state, &view, (10.0, 10.0), 130.0);
        assert_eq!(
            (
                state.containers[&ids["inner"]].y,
                state.containers[&ids["outer"]].y
            ),
            (100.0, 30.0)
        );
        assert_eq!(state.viewport.y, 0.0);
    }

    #[test]
    fn a_wheel_over_a_clipped_away_container_scrolls_what_shows_there() {
        // The inner container sticks out of the outer one, which clips it:
        // below the outer one, in its margin, what shows is the page.
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div id=outer style='width: 100px; height: 50px; margin-bottom: 100px; overflow: hidden'>
               <div id=inner style='height: 100px; overflow: auto'><div style='height: 300px'></div></div>
             </div>
             <div style='height: 950px'></div>",
        );
        let mut state = laid_out(size, Default::default());
        wheel(&mut state, &view, (10.0, 70.0), 30.0);
        assert_eq!(state.viewport.y, 30.0);
        assert!(!state.containers.contains_key(&ids["inner"]));
        // Inside the outer one, the inner one takes it.
        wheel(&mut state, &view, (10.0, 10.0), 30.0);
        assert_eq!(state.containers[&ids["inner"]].y, 30.0);
    }

    /// Blink's `GetScrollOffsetToExpose`, on one axis of a 100px port at
    /// 0: `start`, `center` and `end` always move; `nearest` doesn't when
    /// the span is in view or fills it, and else takes the nearer edge,
    /// which for a span larger than the port is the far one; `center if
    /// needed` centers one out of view.
    #[test]
    fn each_alignment_exposes_a_span_as_blink_does() {
        use ScrollAlignment::*;
        let at = |current: f32, start: f32, size: f32, align| {
            expose_axis(current, 100.0, start, size, align)
        };
        assert_eq!(at(0.0, 150.0, 20.0, Start), 150.0);
        assert_eq!(at(0.0, 150.0, 20.0, Center), 110.0);
        assert_eq!(at(0.0, 150.0, 20.0, End), 70.0);
        // In view, or over all of it: as it is.
        assert_eq!(at(0.0, 10.0, 20.0, Nearest), 0.0);
        assert_eq!(at(0.0, -10.0, 300.0, Nearest), 0.0);
        // Below, or partly: its end at the end.
        assert_eq!(at(0.0, 150.0, 20.0, Nearest), 70.0);
        assert_eq!(at(0.0, 90.0, 20.0, Nearest), 10.0);
        // Above: its start at the start.
        assert_eq!(at(200.0, 150.0, 20.0, Nearest), 150.0);
        // Larger than the port, past its end: its start; before its
        // start: its end.
        assert_eq!(at(0.0, 50.0, 250.0, Nearest), 50.0);
        assert_eq!(at(200.0, 0.0, 250.0, Nearest), 150.0);
        // `center if needed`: in view, or over all of it, as it is;
        // partly in view, as `nearest`; out of view (touching the edge
        // too), centered, a span larger than the port too.
        assert_eq!(at(0.0, 10.0, 20.0, CenterIfNeeded), 0.0);
        assert_eq!(at(0.0, -10.0, 300.0, CenterIfNeeded), 0.0);
        assert_eq!(at(0.0, 90.0, 20.0, CenterIfNeeded), 10.0);
        assert_eq!(at(200.0, 190.0, 20.0, CenterIfNeeded), 190.0);
        assert_eq!(at(0.0, 50.0, 250.0, CenterIfNeeded), 50.0);
        assert_eq!(at(0.0, 150.0, 20.0, CenterIfNeeded), 110.0);
        assert_eq!(at(0.0, 100.0, 20.0, CenterIfNeeded), 60.0);
        assert_eq!(at(300.0, 150.0, 20.0, CenterIfNeeded), 110.0);
        assert_eq!(at(0.0, 150.0, 250.0, CenterIfNeeded), 225.0);
    }

    /// `scrollIntoView` in one frame: the container, then the viewport,
    /// each as the alignments say (centered here; sideways it is in view,
    /// so `nearest` leaves both); where the target ends up in the viewport.
    /// With `nearest_only`, the container alone. A target with no box
    /// scrolls nothing.
    #[test]
    fn script_scrolls_into_view_as_asked() {
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div style='height: 300px'></div>
             <div id=box style='width: 100px; height: 50px; overflow: auto'>
               <div style='height: 200px'></div><div id=target style='width: 10px; height: 10px'></div>
               <div style='height: 100px'></div>
             </div>
             <div style='height: 950px'></div><div id=none style='display: none'></div>",
        );
        let center = ScrollIntoViewParams {
            block: ScrollAlignment::Center,
            inline: ScrollAlignment::Nearest,
            nearest_only: false,
        };
        let mut state = laid_out(size, Default::default());
        let rect = state.scroll_into_view(&view, ids["target"], None, center);
        // In the container: 200 + 5 - 25. The container, 300 to 350, then
        // centered in the 100px viewport: 325 - 50.
        assert_eq!(state.containers[&ids["box"]], PointF::new(0.0, 180.0));
        assert_eq!(state.viewport, PointF::new(0.0, 275.0));
        assert_eq!(rect, Some(RectF::new(0.0, 45.0, 10.0, 10.0)));
        assert_eq!(state.take_scrolled(), [Some(ids["box"]), None]);

        let mut state = laid_out(size, Default::default());
        let nearest_only = ScrollIntoViewParams {
            nearest_only: true,
            ..center
        };
        let rect = state.scroll_into_view(&view, ids["target"], None, nearest_only);
        assert_eq!(state.containers[&ids["box"]], PointF::new(0.0, 180.0));
        assert_eq!(state.viewport, PointF::new(0.0, 0.0));
        assert_eq!(rect, Some(RectF::new(0.0, 320.0, 10.0, 10.0)));

        assert_eq!(
            state.scroll_into_view(&view, ids["none"], None, center),
            None
        );
    }

    /// Inside a fixed box, the viewport doesn't scroll (the box moves with
    /// it); a scroll container that is the fixed box still does.
    #[test]
    fn nothing_past_a_fixed_box_scrolls_into_view() {
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div style='height: 3000px'></div>
             <div style='position: fixed; top: 0; width: 50px; height: 50px'>
               <div id=in-fixed style='margin-top: 2000px; height: 10px'></div>
             </div>
             <div id=scroller style='position: fixed; top: 0; width: 50px; height: 50px;
                                     overflow: auto'>
               <div style='height: 200px'></div><div id=in-scroller style='height: 10px'></div>
             </div>",
        );
        let start = ScrollIntoViewParams {
            block: ScrollAlignment::Start,
            inline: ScrollAlignment::Nearest,
            nearest_only: false,
        };
        let mut state = laid_out(size, Default::default());
        state.scroll_into_view(&view, ids["in-fixed"], None, start);
        assert_eq!(state.viewport, PointF::new(0.0, 0.0));
        state.scroll_into_view(&view, ids["in-scroller"], None, start);
        // At its end: 210 of content in 50.
        assert_eq!(state.containers[&ids["scroller"]], PointF::new(0.0, 160.0));
        assert_eq!(state.viewport, PointF::new(0.0, 0.0));
    }

    #[test]
    fn overflow_hidden_passes_the_wheel_on_but_a_fragment_scrolls_it() {
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div id=box style='width: 100px; height: 50px; overflow: hidden'>
               <div style='height: 200px'></div><div id=target style='height: 10px'></div><div style='height: 100px'></div>
             </div>
             <div style='height: 950px'></div>",
        );
        let mut state = laid_out(size, Default::default());
        let s = wheel(&mut state, &view, (10.0, 10.0), 30.0);
        assert_eq!(
            s,
            Scrolled {
                viewport: true,
                containers: false
            }
        );
        assert_eq!(state.viewport.y, 30.0);
        // The target's top at the container's top, and the container's at
        // the viewport's.
        assert!(
            state
                .scroll_into_view(&view, ids["target"], None, FRAGMENT_ANCHOR)
                .is_some()
        );
        assert_eq!(
            (state.containers[&ids["box"]].y, state.viewport.y),
            (200.0, 0.0)
        );
    }

    #[test]
    fn a_container_scrolls_only_the_axes_the_user_may() {
        // Only vertically: a sideways wheel goes on to the viewport, on a
        // page wide enough to take it.
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div id=box style='width: 100px; height: 50px; overflow-x: hidden; overflow-y: auto'>
               <div style='width: 300px; height: 250px'></div>
             </div>
             <div style='width: 400px; height: 950px'></div>",
        );
        let box_ = ids["box"];
        let mut state = laid_out(size, Default::default());
        wheel(&mut state, &view, (10.0, 10.0), 30.0);
        assert_eq!(state.containers[&box_], PointF::new(0.0, 30.0));
        let pre = paint::pre_paint(&view, &state.containers, 1.0);
        state.wheel(
            &pre,
            &view,
            PointF::new(10.0, 10.0),
            PointF::new(30.0, 0.0),
            &mut |_, _, _, _| unreachable!("no child frames"),
        );
        assert_eq!(state.containers[&box_], PointF::new(0.0, 30.0));
        assert_eq!(state.viewport, PointF::new(30.0, 0.0));
    }

    #[test]
    fn a_bordered_container_scrolls_its_padding_box() {
        // A 50px-high padding box inside 10px borders, over 250px of
        // content: 200 to scroll, both for the wheel (paint's scroll node)
        // and for clamping after layout (the fragment's).
        let (view, ids, size, _) = lay_out(
            "<style>html, body { margin: 0 }</style>
             <div id=box style='width: 100px; height: 50px; border: 10px solid; overflow: auto'>
               <div style='height: 250px'></div>
             </div>
             <div style='height: 950px'></div>",
        );
        let box_ = ids["box"];
        let mut state = laid_out(size, Default::default());
        wheel(&mut state, &view, (20.0, 20.0), 5000.0);
        assert_eq!(state.containers[&box_].y, 200.0);
        state.containers.insert(box_, PointF::new(0.0, 5000.0));
        state.clamp(&view);
        assert_eq!(state.containers[&box_].y, 200.0);
    }

    #[test]
    fn the_compositors_scrolling_moves_each_scroller_clamped_to_its_own_range() {
        let (view, ids, size, _) = lay_out(PAGE);
        let box_ = ids["box"];
        let element = compositing::element_id(box_);
        let mut state = laid_out(size, Default::default());
        state.containers.insert(box_, PointF::new(0.0, 30.0));
        let down = |y| PointF::new(0.0, y);
        state.apply_compositor_delta(&view, down(100.0), &[]);
        assert_eq!(state.viewport.y, 100.0);
        assert_eq!(state.containers[&box_].y, 30.0, "only the viewport");
        state.apply_compositor_delta(&view, down(5000.0), &[(element, down(50.0))]);
        assert_eq!(state.viewport.y, 900.0, "the document's end");
        assert_eq!(state.containers[&box_].y, 80.0);
        state.apply_compositor_delta(&view, down(0.0), &[(element, down(5000.0))]);
        assert_eq!(state.containers[&box_].y, 200.0, "the container's end");
        state.apply_compositor_delta(&view, down(-5000.0), &[(element, down(-5000.0))]);
        assert_eq!((state.viewport.y, state.containers[&box_].y), (0.0, 0.0));
        // A container no longer there is left.
        state.apply_compositor_delta(&view, down(0.0), &[(ElementId(u64::MAX), down(10.0))]);
        assert_eq!(state.containers.len(), 1);
        // Before the first layout there is nothing to scroll.
        let mut state = ScrollState::default();
        state.apply_compositor_delta(&view, down(100.0), &[]);
        assert_eq!(state.viewport.y, 0.0);
    }

    /// A container the compositor scrolled that main hasn't scrolled yet
    /// starts from 0, 0.
    #[test]
    fn the_compositors_scrolling_starts_a_container_main_never_scrolled() {
        let (view, ids, size, _) = lay_out(PAGE);
        let mut state = laid_out(size, Default::default());
        let element = compositing::element_id(ids["box"]);
        state.apply_compositor_delta(
            &view,
            PointF::new(0.0, 0.0),
            &[(element, PointF::new(0.0, 40.0))],
        );
        assert_eq!(state.containers[&ids["box"]], PointF::new(0.0, 40.0));
    }

    /// `clip` on the viewport is `hidden` (css-overflow 3): it stops the
    /// user scrolling it too.
    #[test]
    fn body_overflow_clip_is_hidden_on_the_viewport() {
        let (_, _, size, overflow) = lay_out(
            "<style>html, body { margin: 0 } body { overflow: clip }</style>
             <div style='height: 1000px'></div>",
        );
        assert_eq!(overflow, (Overflow::Hidden, Overflow::Hidden));
        let state = laid_out(size, overflow);
        let viewport = state.viewport_node.expect("laid out");
        assert!(!viewport.user_scrollable_vertical && !viewport.user_scrollable_horizontal);
    }

    #[test]
    fn body_overflow_hidden_stops_the_user_scrolling_the_viewport() {
        let (view, ids, size, overflow) = lay_out(
            "<style>html, body { margin: 0 } body { overflow: hidden }</style>
             <div style='height: 1000px'></div><div id=end style='height: 10px'></div>",
        );
        assert_eq!(overflow, (Overflow::Hidden, Overflow::Hidden));
        let mut state = laid_out(size, overflow);
        let viewport = state.viewport_node.expect("laid out");
        assert!(!viewport.user_scrollable_vertical && !viewport.user_scrollable_horizontal);
        assert_eq!(
            wheel(&mut state, &view, (10.0, 10.0), 50.0),
            Scrolled::default()
        );
        // `<body>`, whose `overflow` went to the viewport, is no scroll
        // container and doesn't clip: the document is as tall as its
        // content, and a `#fragment` still scrolls the viewport there.
        assert!(
            paint::pre_paint(&view, &state.containers, 1.0)
                .trees
                .scroll
                .is_empty()
        );
        assert_eq!(size.height, 1010.0);
        assert!(
            state
                .scroll_into_view(&view, ids["end"], None, FRAGMENT_ANCHOR)
                .is_some()
        );
        assert_eq!(state.viewport.y, 910.0);
        // The root's `overflow` wins when it isn't visible, and `<body>`
        // keeps its own: a scroll container.
        let (view, _, _, overflow) =
            lay_out("<style>html { overflow: auto } body { overflow: hidden }</style><div></div>");
        assert_eq!(overflow, (Overflow::Auto, Overflow::Auto));
        assert_eq!(
            paint::pre_paint(&view, &Default::default(), 1.0)
                .trees
                .scroll
                .len(),
            1
        );
    }

    #[test]
    fn offsets_are_clamped_when_what_can_scroll_shrinks() {
        let (view, ids, size, _) = lay_out(PAGE);
        let mut state = ScrollState {
            viewport: PointF::new(0.0, 5000.0),
            containers: [(ids["box"], PointF::new(0.0, 5000.0))]
                .into_iter()
                .collect(),
            ..laid_out(size, Default::default())
        };
        state.clamp(&view);
        assert_eq!(
            (state.viewport.y, state.containers[&ids["box"]].y),
            (900.0, 200.0)
        );
    }
}
