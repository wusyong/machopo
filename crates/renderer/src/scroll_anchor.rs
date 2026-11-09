//! Scroll anchoring (css-scroll-anchoring; Blink's `ScrollAnchor`): before
//! a layout each scrolled scroller without an anchor picks one; after it,
//! each scrolls by as much as its anchor moved, so what the user was
//! reading stays where it was. `layout::find_scroll_anchor` and
//! `layout::scroll_anchor_offset` measure; this keeps the anchors in
//! `ScrollState` and moves the offsets.

use dom::{Cx, LocalToken, NodeId, NodeIdHashSet, NodeRef};
use gfx::{PointF, RectF, SizeF};
use layout::BoxFragment;

use crate::scroll::{ScrollAnchor, ScrollState};

impl ScrollState {
    /// Before a layout of `document` (Blink's
    /// `ScrollAnchor::NotifyBeforeLayout`): a scroller at the top of its
    /// block axis, or a container with `overflow-anchor: none`, has no
    /// anchor; one scrolled without one picks it in `fragment`, the last
    /// layout, among what still has a box. One that has an anchor keeps
    /// it, and the offset it was picked at, until a scroll other than the
    /// clamp's and scroll anchoring's moves it, the anchor has no box
    /// (Blink's anchor is a weak pointer to its layout object, destroyed
    /// with its node, without a box, or built again: `changes.rebuilt`),
    /// or the `overflow-anchor` of something in the scroller changed
    /// (`changes.overflow_anchor`, Blink's `ClearAncestorScrollAnchors`).
    pub(crate) fn pick_scroll_anchors<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        token: &LocalToken,
        document: NodeRef<'gc>,
        fragment: &BoxFragment,
        viewport: SizeF,
        changes: &layout::ScrollAnchoringChanges,
    ) {
        for &changed in &changes.overflow_anchor {
            self.anchors.remove(&None);
            let mut node = cx
                .node_by_id(changed)
                .and_then(|node| cx.flat_tree_parent(node));
            while let Some(n) = node {
                self.anchors.remove(&Some(cx.node_id(n)));
                node = cx.flat_tree_parent(n);
            }
        }
        let rebuilt: NodeIdHashSet = changes.rebuilt.iter().copied().collect();
        let gone = |node: NodeId| !has_box(cx, document, node, &rebuilt);
        self.anchors.retain(|_, anchor| !gone(anchor.node.node()));
        // The focused element in editable content, and those around it:
        // the priority candidate is the first with a box.
        let focused = cx
            .focused_element(document)
            .filter(|&focused| cx.is_editable(focused));
        let priority: Vec<NodeId> = std::iter::successors(focused, |&n| cx.flat_tree_parent(n))
            .map(|n| cx.node_id(n))
            .collect();
        let mut scrollers: Vec<(Option<NodeId>, PointF)> = vec![(None, self.viewport)];
        scrollers.extend(self.containers.iter().map(|(e, o)| (Some(*e), *o)));
        for (scroller, offset) in scrollers {
            // A container with `overflow-anchor: none` has none (Blink's
            // `ShouldPerformScrollAnchoring`; the viewport's is the root's
            // box's, which the search skips with what is in it).
            let opted_out = scroller.is_some_and(|element| {
                cx.node_by_id(element)
                    .and_then(|element| cx.computed_style(element, token))
                    .is_some_and(|style| {
                        style.rare.overflow_anchor == computed_style::OverflowAnchor::None
                    })
            });
            if offset.y == 0.0 || opted_out {
                self.anchors.remove(&scroller);
                continue;
            }
            if self.anchors.contains_key(&scroller) {
                continue;
            }
            // The last layout's boxes have the styles they had: what can't
            // be an anchor now is skipped as its new style says.
            let skip = |node: NodeId| gone(node) || !anchors_now(cx, token, node, scroller);
            if let Some((node, offset)) = layout::find_scroll_anchor(
                fragment,
                &self.containers,
                &self.stuck.offsets,
                scroller,
                self.viewport_rect(viewport),
                &skip,
                &priority,
            ) {
                self.anchors.insert(scroller, ScrollAnchor { node, offset });
            }
        }
    }

    /// After the layout of `fragment` (Blink's `ScrollAnchor::Adjust`):
    /// each scroller with an anchor scrolls by how far the anchor moved
    /// from where it was when picked, in device pixels at
    /// `device_scale_factor`. An anchor with no box goes; so does one
    /// that moved if a box from it up to its scroller changed a property
    /// that moves what is in it (`disabled`, the box tree's
    /// `ScrollAnchoringChanges::disabled`), without the scroll: the change
    /// itself is what the page meant to show (Blink's
    /// `scroll_anchor_disabling_style_changed_`). The caller clamps after.
    /// Whether anything moved.
    pub(crate) fn adjust_for_scroll_anchors(
        &mut self,
        cx: &Cx<'_, '_>,
        fragment: &BoxFragment,
        viewport: SizeF,
        disabled: &[NodeId],
        device_scale_factor: f32,
    ) -> bool {
        // The viewport first, then the containers outside in (Blink's
        // `anchoring_adjustment_queue_`, in the order the scrollers came):
        // each measures its anchor before those inside it move.
        let mut order = vec![None];
        if self.anchors.len() > usize::from(self.anchors.contains_key(&None)) {
            containers_in_tree_order(fragment, &mut order);
        }
        let anchors: Vec<(Option<NodeId>, ScrollAnchor)> = order
            .into_iter()
            .filter_map(|scroller| Some((scroller, *self.anchors.get(&scroller)?)))
            .collect();
        let mut moved = false;
        for (scroller, anchor) in anchors {
            let Some(offset) = layout::scroll_anchor_offset(
                fragment,
                &self.containers,
                &self.stuck.offsets,
                scroller,
                self.viewport_rect(viewport),
                anchor.node,
            ) else {
                self.anchors.remove(&scroller);
                continue;
            };
            let snap = |y: f32| (y * device_scale_factor).round();
            let adjustment = (snap(offset) - snap(anchor.offset)) / device_scale_factor;
            if adjustment == 0.0 {
                continue;
            }
            if changed_on_the_way(cx, anchor.node.node(), scroller, disabled) {
                self.anchors.remove(&scroller);
                continue;
            }
            let current = match scroller {
                None => self.viewport,
                Some(element) => self.containers.get(&element).copied().unwrap_or_default(),
            };
            moved |= self.move_scroller(scroller, PointF::new(current.x, current.y + adjustment));
        }
        moved
    }

    /// What the viewport shows of the document, as `layout`'s geometry
    /// has it.
    fn viewport_rect(&self, viewport: SizeF) -> RectF {
        RectF::new(
            self.viewport.x,
            self.viewport.y,
            viewport.width,
            viewport.height,
        )
    }
}

/// The scroll containers in `fragment`, in tree order.
fn containers_in_tree_order(fragment: &BoxFragment, out: &mut Vec<Option<NodeId>>) {
    for link in &fragment.children {
        let child = &link.fragment;
        if let Some(node) = child.node
            && child.pseudo.is_none()
            && child.style.is_scroll_container()
        {
            out.push(Some(node));
        }
        containers_in_tree_order(child, out);
    }
}

/// Whether `node` is in `document` and has the box it had: whether it,
/// and each element around it in the flat tree, wasn't built again
/// (`rebuilt`: inserted, moved, `display: none` and other new `display`s;
/// Blink's layout tree no longer has such a node's object, or has a new
/// one never laid out, which the search skips).
fn has_box<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    node: NodeId,
    rebuilt: &NodeIdHashSet,
) -> bool {
    let Some(mut n) = cx.node_by_id(node) else {
        return false;
    };
    loop {
        if rebuilt.contains(&cx.node_id(n)) {
            return false;
        }
        match cx.flat_tree_parent(n) {
            Some(parent) => n = parent,
            None => return cx.node_id(n) == cx.node_id(document),
        }
    }
}

/// Whether the styles let `node` be `scroller`'s anchor now: neither it
/// (a text node's element: Blink's `LayoutText` has its parent's style) nor
/// an element around it below `scroller` (`None`: the viewport) has
/// `overflow-anchor: none`. The search skips a box with it and what is in
/// it, but goes through an inline element, which has no box of its own
/// (Blink skips a `LayoutInline` with it).
fn anchors_now(
    cx: &Cx<'_, '_>,
    token: &LocalToken,
    node: NodeId,
    scroller: Option<NodeId>,
) -> bool {
    let mut node = cx.node_by_id(node);
    while let Some(n) = node {
        if Some(cx.node_id(n)) == scroller {
            return true;
        }
        if cx
            .computed_style(n, token)
            .is_some_and(|style| style.rare.overflow_anchor == computed_style::OverflowAnchor::None)
        {
            return false;
        }
        node = cx.flat_tree_parent(n);
    }
    true
}

/// Whether `anchor` or an element around it, up to and including
/// `scroller` (`None`: the root), is in `disabled` (Blink's
/// `ComputeScrollAnchorDisablingStyleChanged`, which walks the anchor's
/// containing boxes the same way).
fn changed_on_the_way(
    cx: &Cx<'_, '_>,
    anchor: NodeId,
    scroller: Option<NodeId>,
    disabled: &[NodeId],
) -> bool {
    if disabled.is_empty() {
        return false;
    }
    let mut node = cx.node_by_id(anchor);
    while let Some(n) = node {
        let id = cx.node_id(n);
        if disabled.contains(&id) {
            return true;
        }
        if Some(id) == scroller {
            return false;
        }
        node = cx.flat_tree_parent(n);
    }
    false
}
