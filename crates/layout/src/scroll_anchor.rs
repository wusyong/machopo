//! Scroll anchoring's geometry (css-scroll-anchoring; Blink's
//! `ScrollAnchor::FindAnchor` and `ComputeRelativeOffset`): which box or
//! text in a scroller's content is its anchor, and how far its top is from
//! the top of what the scroller shows. The renderer keeps the anchors and
//! moves the scroll offsets; this only measures, in the root fragment's
//! space as `client_rects` maps it (the scroll containers' offsets taken
//! off, the viewport's not).
//!
//! machopo lays out horizontal text only: the block axis is the vertical one,
//! and the anchor's corner is its top.

use gfx::NodeId;
use gfx::{PointF, RectF, Transform};

use crate::geometry::{ScrollOffsets, box_transform, stuck};
use crate::{BoxFragment, StickyOffsets};

/// What a scroll anchor is: an element's box, or a text node's text
/// (Blink anchors a `LayoutText` as it does a box).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollAnchorNode {
    Box(NodeId),
    Text(NodeId),
}

impl ScrollAnchorNode {
    pub fn node(self) -> NodeId {
        match self {
            Self::Box(node) | Self::Text(node) => node,
        }
    }
}

/// `scroller`'s anchor (`None`: the viewport, which shows `viewport` of
/// the document) and how far its top is below the top of what the
/// scroller shows (Blink's `saved_relative_offset_`); `None` if nothing in
/// it can be one. As Blink's `FindAnchorRecursive` and `Examine`, the
/// first in tree order that is in view: a box or text all in view is it;
/// one partly in view is it unless something in it is all in view, and
/// what comes after it isn't looked at. Not looked at, nor what is in
/// them: `overflow-anchor: none`, fixed and sticky boxes, boxes with no
/// width or height, and what isn't in view at all. A box reaches down to
/// where what overflows it ends, unless it clips that. What is in another
/// scroll container is looked at where it has scrolled to, clipped or not
/// (Blink's search doesn't stop at one). An inline
/// element is gone through (Blink's `LayoutInline`), its text examined
/// (text whose element has `overflow-anchor: none` is skipped). The boxes
/// and text of an inline formatting context are taken line by line, then
/// left to right. `layout_view` may be from before the DOM and styles
/// changed: a node `skip` says isn't a candidate now, with no box or with
/// `overflow-anchor: none`, isn't looked at, nor what is in it (Blink
/// searches its layout tree before layout, its objects gone or with their
/// new styles, where they were).
///
/// First, the priority candidate (Blink's `FindAnchorInPriorityCandidates`):
/// the first of `priority` (the focused element in editable content and
/// the elements around it, in the flat tree; empty if there is none) that
/// has a box, if that is in the scroller, nothing from it up to the
/// scroller is `overflow-anchor: none`, fixed or sticky, and it is in
/// view: the search runs in it, and it is the anchor if nothing in it is.
pub fn find_scroll_anchor(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    scroller: Option<NodeId>,
    viewport: RectF,
    skip: &dyn Fn(NodeId) -> bool,
    priority: &[NodeId],
) -> Option<(ScrollAnchorNode, f32)> {
    let port = scrollport(layout_view, scroll, sticky, scroller, viewport)?;
    let mut found = None;
    let candidate = priority_candidate(layout_view, priority);
    examine_children(&port, scroll, sticky, skip, candidate, &mut found);
    found.map(|(anchor, rect): (ScrollAnchorNode, RectF)| (anchor, rect.y - port.visible.y))
}

/// How far `anchor`'s top is below the top of what `scroller` shows now
/// (Blink's `ComputeRelativeOffset`); `None` if the scroller or the anchor
/// has no box.
pub fn scroll_anchor_offset(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    scroller: Option<NodeId>,
    viewport: RectF,
    anchor: ScrollAnchorNode,
) -> Option<f32> {
    let port = scrollport(layout_view, scroll, sticky, scroller, viewport)?;
    let rects = match anchor {
        ScrollAnchorNode::Box(element) => {
            crate::geometry::client_rects(layout_view, scroll, sticky, element)
        }
        ScrollAnchorNode::Text(text) => {
            crate::geometry::text_rects(layout_view, scroll, sticky, text, 0, u32::MAX)
        }
    };
    let top = rects.iter().map(|rect| rect.y).reduce(f32::min)?;
    Some(top - port.visible.y)
}

/// A scroller as the search sees it: what it shows, in the root's space,
/// and its content: its fragment, where its border box is, and the
/// mapping of its content to the root.
struct Scrollport<'a> {
    visible: RectF,
    fragment: &'a BoxFragment,
    offset: PointF,
    contents: Transform,
}

fn scrollport<'a>(
    layout_view: &'a BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    scroller: Option<NodeId>,
    viewport: RectF,
) -> Option<Scrollport<'a>> {
    let Some(element) = scroller else {
        return Some(Scrollport {
            visible: viewport,
            fragment: layout_view,
            offset: PointF::new(0.0, 0.0),
            contents: Transform::IDENTITY,
        });
    };
    fn find<'a>(
        fragment: &'a BoxFragment,
        offset: PointF,
        to_root: Transform,
        scroll: &ScrollOffsets,
        sticky: &StickyOffsets,
        element: NodeId,
    ) -> Option<Scrollport<'a>> {
        let place = Place::of(fragment, offset, to_root, scroll, sticky);
        if fragment.node == Some(element)
            && fragment.pseudo.is_none()
            && fragment.style.is_scroll_container()
        {
            let (b, size) = (&fragment.border, fragment.size);
            let padding_box = RectF::new(
                place.offset.x + b.left,
                place.offset.y + b.top,
                size.width - b.left - b.right,
                size.height - b.top - b.bottom,
            );
            return Some(Scrollport {
                visible: place.to_root.map_rect(padding_box),
                fragment,
                offset: place.offset,
                contents: place.contents,
            });
        }
        fragment.children.iter().find_map(|child| {
            let at = PointF::new(
                place.offset.x + child.offset.x,
                place.offset.y + child.offset.y,
            );
            find(&child.fragment, at, place.contents, scroll, sticky, element)
        })
    }
    find(
        layout_view,
        PointF::new(0.0, 0.0),
        Transform::IDENTITY,
        scroll,
        sticky,
        element,
    )
}

/// Where a fragment is, as `geometry`'s walk places it: its border box's
/// offset in the space `to_root` maps from (after how far it sticks, and
/// at the origin under its own transform), and the mapping of its content.
struct Place {
    offset: PointF,
    to_root: Transform,
    contents: Transform,
}

impl Place {
    fn of(
        fragment: &BoxFragment,
        offset: PointF,
        to_root: Transform,
        scroll: &ScrollOffsets,
        sticky: &StickyOffsets,
    ) -> Self {
        let (mut to_root, mut offset) = (to_root, stuck(fragment, offset, sticky));
        if let Some(transform) = box_transform(fragment) {
            to_root = to_root
                .then(&Transform::translate(offset.x, offset.y))
                .then(&transform);
            offset = PointF::new(0.0, 0.0);
        }
        let contents = match fragment.node {
            Some(node) if fragment.style.is_scroll_container() => {
                let at = scroll.get(&node).copied().unwrap_or(PointF::new(0.0, 0.0));
                to_root.then(&Transform::translate(-at.x, -at.y))
            }
            _ => to_root,
        };
        Self {
            offset,
            to_root,
            contents,
        }
    }

    /// The box as a candidate sees it (Blink's `RelativeBounds`): its
    /// border box, down to where what overflows it ends unless it clips
    /// that (`extend_overflow` stops there).
    fn bounds(&self, fragment: &BoxFragment) -> RectF {
        let mut overflow = gfx::SizeF::new(0.0, 0.0);
        crate::extend_overflow(fragment, PointF::new(0.0, 0.0), &mut overflow);
        let height = fragment.size.height.max(overflow.height);
        self.to_root.map_rect(RectF::new(
            self.offset.x,
            self.offset.y,
            fragment.size.width,
            height,
        ))
    }
}

/// What `Examine` says of a candidate.
enum Examined {
    /// Not it, nor anything in it.
    Skip,
    /// All in view: it.
    Return,
    /// Partly in view: it, unless something in it is all in view.
    Constrain,
}

fn examine(rect: RectF, visible: RectF) -> Examined {
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return Examined::Skip;
    }
    let inside = rect.x >= visible.x
        && rect.y >= visible.y
        && rect.right() <= visible.right()
        && rect.bottom() <= visible.bottom();
    let overlaps = rect.x < visible.right()
        && visible.x < rect.right()
        && rect.y < visible.bottom()
        && visible.y < rect.bottom();
    if inside {
        Examined::Return
    } else if overlaps {
        Examined::Constrain
    } else {
        Examined::Skip
    }
}

/// A candidate in a box: a child box at its border box's offset, mapped
/// to the root by the transform, or a text node's text there.
enum Candidate<'a> {
    Box(&'a BoxFragment, PointF, Transform),
    Text(NodeId, RectF),
}

/// Where each out-of-flow box in the scroller is, as its containing
/// block's child: its border box's offset and the mapping of its
/// containing block's content. The search takes it up where its parent
/// is in tree order (`BoxFragment::out_of_flow`), as Blink's layout tree
/// has it; one whose containing block is outside the scroller isn't here,
/// and doesn't move with it (Blink's `CandidateMayMoveWithScroller`).
type OutOfFlow = std::collections::HashMap<*const BoxFragment, (PointF, Transform)>;

fn collect_out_of_flow(
    fragment: &BoxFragment,
    offset: PointF,
    contents: Transform,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    out: &mut OutOfFlow,
) {
    for link in &fragment.children {
        let at = PointF::new(offset.x + link.offset.x, offset.y + link.offset.y);
        if link.fragment.style.is_out_of_flow() {
            out.insert(std::sync::Arc::as_ptr(&link.fragment), (at, contents));
        }
        let place = Place::of(&link.fragment, at, contents, scroll, sticky);
        collect_out_of_flow(
            &link.fragment,
            place.offset,
            place.contents,
            scroll,
            sticky,
            out,
        );
    }
}

/// What the search carries.
struct Search<'a> {
    visible: RectF,
    scroll: &'a ScrollOffsets,
    sticky: &'a StickyOffsets,
    skip: &'a dyn Fn(NodeId) -> bool,
    out_of_flow: OutOfFlow,
    found: Option<(ScrollAnchorNode, RectF)>,
}

/// Searches the content of the scroller `port`.
fn examine_children(
    port: &Scrollport<'_>,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    skip: &dyn Fn(NodeId) -> bool,
    priority: Option<NodeId>,
    found: &mut Option<(ScrollAnchorNode, RectF)>,
) {
    let mut out_of_flow = OutOfFlow::new();
    collect_out_of_flow(
        port.fragment,
        port.offset,
        port.contents,
        scroll,
        sticky,
        &mut out_of_flow,
    );
    let mut search = Search {
        visible: port.visible,
        scroll,
        sticky,
        skip,
        out_of_flow,
        found: None,
    };
    let prioritized = priority.is_some_and(|candidate| {
        let Some((fragment, at, to_root, movable)) = locate(
            port.fragment,
            port.offset,
            port.contents,
            scroll,
            sticky,
            candidate,
        ) else {
            return false;
        };
        if !movable || skip(candidate) {
            return false;
        }
        // Out of view, it is skipped and nothing is found.
        let (examined, rect) = search.box_candidate(fragment, at, to_root);
        search.examine_candidate(examined, rect);
        search.found.is_some()
    });
    if !prioritized {
        search.content(port.fragment, port.offset, &port.contents);
    }
    *found = search.found;
}

/// The first of `priority` that has a box (Blink's
/// `PriorityCandidateFromNode`: an inline element has none of its own).
fn priority_candidate(layout_view: &BoxFragment, priority: &[NodeId]) -> Option<NodeId> {
    if priority.is_empty() {
        return None;
    }
    fn boxed(fragment: &BoxFragment, priority: &[NodeId], first: &mut usize) {
        if let Some(node) = fragment.node.filter(|_| fragment.pseudo.is_none())
            && let Some(at) = priority.iter().position(|&id| id == node)
        {
            *first = at.min(*first);
        }
        for link in &fragment.children {
            boxed(&link.fragment, priority, first);
        }
    }
    let mut first = priority.len();
    boxed(layout_view, priority, &mut first);
    priority.get(first).copied()
}

/// Where `target`'s box is in `fragment`'s content (its border box at
/// `offset`, its content mapped by `contents`): the box, its offset and
/// the mapping of its containing block's content, and whether it moves
/// with the scroller: nothing from it up is fixed or sticky (Blink's
/// `ExaminePriorityCandidate`; `overflow-anchor: none` on the way is the
/// caller's `skip`, which reads the styles as they are now).
fn locate<'a>(
    fragment: &'a BoxFragment,
    offset: PointF,
    contents: Transform,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    target: NodeId,
) -> Option<(&'a BoxFragment, PointF, Transform, bool)> {
    use computed_style::Position;
    fragment.children.iter().find_map(|link| {
        let child = &link.fragment;
        let at = PointF::new(offset.x + link.offset.x, offset.y + link.offset.y);
        let movable = !matches!(child.style.position, Position::Fixed | Position::Sticky)
            && !child.fixed_to_viewport;
        if child.node == Some(target) && child.pseudo.is_none() {
            return Some((&**child, at, contents, movable));
        }
        let place = Place::of(child, at, contents, scroll, sticky);
        locate(child, place.offset, place.contents, scroll, sticky, target)
            .map(|(found, at, to_root, inner)| (found, at, to_root, inner && movable))
    })
}

impl Search<'_> {
    /// `fragment`'s candidates, its border box at `offset`, its content
    /// mapped by `contents` (Blink's loop over a candidate's children in
    /// `FindAnchorRecursive`, then `FindAnchorInOOFs`): whether to stop,
    /// an anchor all in view, or one partly in view, found.
    fn content(&mut self, fragment: &BoxFragment, offset: PointF, contents: &Transform) -> bool {
        // Its children in tree order: the in-flow ones, and the out-of-flow
        // ones whose parent it is where they come among them.
        let mut candidates: Vec<(Candidate<'_>, RectF)> = Vec::new();
        let mut places = fragment.out_of_flow.iter().peekable();
        let in_flow = fragment
            .children
            .iter()
            .enumerate()
            .filter(|(_, link)| !link.fragment.style.is_out_of_flow());
        for link in in_flow.map(Some).chain(std::iter::once(None)) {
            while let Some(place) = places.next_if(|p| link.is_none_or(|(i, _)| p.before <= i)) {
                if let Some(&(at, to_root)) = self
                    .out_of_flow
                    .get(&std::sync::Arc::as_ptr(&place.fragment))
                {
                    candidates.push(self.box_candidate(&place.fragment, at, to_root));
                }
            }
            if let Some((_, link)) = link {
                let at = PointF::new(offset.x + link.offset.x, offset.y + link.offset.y);
                candidates.push(self.box_candidate(&link.fragment, at, *contents));
            }
        }
        if !fragment.lines.is_empty() {
            self.add_text(fragment, offset, contents, &mut candidates);
        }
        for (candidate, rect) in candidates {
            if let Some(stop) = self.examine_candidate(candidate, rect) {
                return stop;
            }
        }
        // The out-of-flow boxes it is the containing block of whose parent
        // isn't it, which their parents may not have reached (Blink's
        // `FindAnchorInOOFs`).
        for link in fragment.children.iter().filter(|link| {
            link.fragment.style.is_out_of_flow()
                && !fragment
                    .out_of_flow
                    .iter()
                    .any(|p| std::sync::Arc::ptr_eq(&p.fragment, &link.fragment))
        }) {
            let at = PointF::new(offset.x + link.offset.x, offset.y + link.offset.y);
            let (candidate, rect) = self.box_candidate(&link.fragment, at, *contents);
            if self.examine_candidate(candidate, rect) == Some(true) {
                return true;
            }
        }
        false
    }

    fn box_candidate<'f>(
        &self,
        fragment: &'f BoxFragment,
        at: PointF,
        to_root: Transform,
    ) -> (Candidate<'f>, RectF) {
        let place = Place::of(fragment, at, to_root, self.scroll, self.sticky);
        (
            Candidate::Box(fragment, at, to_root),
            place.bounds(fragment),
        )
    }

    /// Each text node's runs on `fragment`'s lines, as one box; then all
    /// the candidates there line by line, then left to right.
    fn add_text<'f>(
        &self,
        fragment: &'f BoxFragment,
        offset: PointF,
        contents: &Transform,
        candidates: &mut Vec<(Candidate<'f>, RectF)>,
    ) {
        let mut texts: Vec<(NodeId, RectF)> = Vec::new();
        for line in fragment.lines.iter().filter(|line| !line.hidden) {
            for run in &line.runs {
                let Some((node, _)) = &run.source else {
                    continue;
                };
                if run.style.rare.overflow_anchor == computed_style::OverflowAnchor::None {
                    continue;
                }
                let rect = contents.map_rect(RectF::new(
                    run.x + offset.x,
                    run.baseline - run.ascent + offset.y,
                    run.advance,
                    run.ascent + run.descent,
                ));
                match texts.iter_mut().find(|(n, _)| n == node) {
                    Some((_, union)) => *union = crate::geometry::bounding_rect(&[*union, rect]),
                    None => texts.push((*node, rect)),
                }
            }
        }
        candidates.extend(
            texts
                .into_iter()
                .map(|(node, rect)| (Candidate::Text(node, rect), rect)),
        );
        // The line each is on: the first that ends below its top (an
        // inline-block on the baseline may reach above the line's text).
        let line_ends: Vec<f32> = fragment
            .lines
            .iter()
            .filter(|line| !line.hidden)
            .map(|line| {
                contents
                    .map_rect(line.rect.offset(offset.x, offset.y))
                    .bottom()
            })
            .collect();
        let line_of = |rect: &RectF| line_ends.iter().take_while(|end| **end <= rect.y).count();
        candidates.sort_by(|a, b| {
            (line_of(&a.1), a.1.x)
                .partial_cmp(&(line_of(&b.1), b.1.x))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }

    /// Blink's `FindAnchorRecursive` of one candidate: `Some(true)` to
    /// stop, `None` to go on to the next.
    fn examine_candidate(&mut self, candidate: Candidate<'_>, rect: RectF) -> Option<bool> {
        match candidate {
            Candidate::Text(node, _) if (self.skip)(node) => None,
            Candidate::Text(node, rect) => match examine(rect, self.visible) {
                Examined::Skip => None,
                Examined::Return | Examined::Constrain => {
                    self.found = Some((ScrollAnchorNode::Text(node), rect));
                    Some(true)
                }
            },
            Candidate::Box(child, at, to_root) => {
                use computed_style::Position;
                let style = &child.style;
                if style.rare.overflow_anchor == computed_style::OverflowAnchor::None
                    || matches!(style.position, Position::Fixed | Position::Sticky)
                    || child.fixed_to_viewport
                {
                    return None;
                }
                let place = Place::of(child, at, to_root, self.scroll, self.sticky);
                // An anonymous box is gone through (Blink's anonymous
                // objects), as is a box with no node.
                let Some(node) = child.node.filter(|_| child.pseudo.is_none()) else {
                    return self
                        .content(child, place.offset, &place.contents)
                        .then_some(true);
                };
                if (self.skip)(node) {
                    return None;
                }
                match examine(rect, self.visible) {
                    Examined::Skip => None,
                    Examined::Return => {
                        self.found = Some((ScrollAnchorNode::Box(node), rect));
                        Some(true)
                    }
                    Examined::Constrain => {
                        self.found = Some((ScrollAnchorNode::Box(node), rect));
                        self.content(child, place.offset, &place.contents);
                        Some(true)
                    }
                }
            }
        }
    }
}
