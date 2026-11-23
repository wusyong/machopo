//! Where an element's boxes are, for script (Blink's
//! `LayoutObject::AbsoluteQuads` / `LocalToAbsolute`): its border box, or
//! an inline element's box on each line, mapped from the fragment tree
//! through transforms and the scroll containers' offsets, and to the
//! viewport through the page's scroll, which moves all but what is fixed
//! to it.
//!
//! Pre-paint builds the same mapping into its property trees for paint and
//! hit testing (`paint::pre_paint`, which takes `box_transform` from here);
//! script reads geometry right after a layout it forced, which, as in
//! Chrome, goes no further than layout.
//!
//! [`BoxTree::geometry`] finds an element from its boxes and goes down only
//! the way to them, as Blink climbs from the `LayoutObject`; the free
//! functions walk the tree from the root to find it, which is what it is
//! checked against, and what queries of a pseudo-element still do.

use std::cell::Cell;
use std::sync::Arc;

use computed_style::{LengthPercentageOrAuto, TransformBox};
use gfx::{BlockBox, ElementBox, NodeId, NodeIdHashMap};
use gfx::{PointF, RectF, SizeF, Transform};

use crate::{BoxFragment, BoxTree, FragmentLink, LAYOUT_VIEW, LineFragment, StickyOffsets};

/// Each scroll container's scroll offset, by element: how far its content
/// is moved up and left. Containers not in it are at the origin.
pub type ScrollOffsets = NodeIdHashMap<PointF>;

/// From a space in the fragment tree to the viewport (Blink's
/// `TransformState`, as `MapLocalToAncestor` leaves it): through `to_root`
/// to the root fragment's space, then less `scrolled`, how far the page's
/// scroll moves what is in it. That isn't composed into the transform: a
/// rect's size would be rounded otherwise the further the page scrolls.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Mapping {
    pub(crate) to_root: Transform,
    pub(crate) scrolled: PointF,
}

impl Mapping {
    fn then(&self, transform: &Transform) -> Self {
        Self {
            to_root: self.to_root.then(transform),
            scrolled: self.scrolled,
        }
    }

    fn map_point(&self, point: PointF) -> PointF {
        let at = self.to_root.map_point(point);
        PointF::new(at.x - self.scrolled.x, at.y - self.scrolled.y)
    }

    fn map_rect(&self, rect: RectF) -> RectF {
        let rect = self.to_root.map_rect(rect);
        rect.offset(-self.scrolled.x, -self.scrolled.y)
    }
}

/// `fragment`'s `transform`, from its border box's origin, about its
/// `transform-origin` in its reference box (`transform-box`: the content
/// box for `content-box` and `fill-box`, else the border box; Blink's
/// `TransformHelper::ComputeReferenceBox` for a CSS box); `None` without
/// one.
pub fn box_transform(fragment: &BoxFragment) -> Option<Transform> {
    let size = fragment.size;
    let reference = match fragment.style.rare.transform_box {
        TransformBox::ContentBox | TransformBox::FillBox => {
            let (border, padding) = (&fragment.border, &fragment.padding);
            let (left, top) = (border.left + padding.left, border.top + padding.top);
            RectF::new(
                left,
                top,
                (size.width - left - border.right - padding.right).max(0.0),
                (size.height - top - border.bottom - padding.bottom).max(0.0),
            )
        }
        TransformBox::BorderBox | TransformBox::StrokeBox | TransformBox::ViewBox => {
            RectF::new(0.0, 0.0, size.width, size.height)
        }
    };
    fragment.style.transform_about_origin(reference)
}

/// `element`'s boxes in the viewport, the page scrolled to
/// `viewport_scroll` (zero gives the document's coordinates), in tree
/// order: its border box, or an inline element's box on each line (CSSOM
/// View's `getClientRects`), each the bounding rect of the box as
/// transformed, moved by the offsets of the scroll containers around it
/// (`scroll`) and by the page's unless it is in a box fixed to the
/// viewport, not cut by clips. Empty if it has no box.
pub fn client_rects(
    layout_view: &BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Vec<RectF> {
    let mut rects = Vec::new();
    walk(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        &mut |fragment, offset, mapping, contents| {
            let lines = &fragment.lines;
            client_rects_in(
                fragment, lines, offset, mapping, contents, element, &mut rects,
            );
            true
        },
    );
    rects
}

/// `client_rects`' of `element` that `fragment` has, at `offset` (as `walk`
/// gives them): its border box if it is the element's box, and the
/// element's boxes on `lines` (its lines, or those of them the element is
/// on).
fn client_rects_in(
    fragment: &BoxFragment,
    lines: &[LineFragment],
    offset: PointF,
    mapping: &Mapping,
    contents: &Mapping,
    element: NodeId,
    rects: &mut Vec<RectF>,
) {
    if fragment.node == Some(element) && fragment.pseudo.is_none() {
        let border_box = RectF::new(
            offset.x,
            offset.y,
            fragment.size.width,
            fragment.size.height,
        );
        rects.push(mapping.map_rect(border_box));
    }
    for line in lines {
        for inline_box in line
            .boxes
            .iter()
            .filter(|b| b.node == element && b.pseudo.is_none())
        {
            rects.push(contents.map_rect(inline_box.rect.offset(offset.x, offset.y)));
        }
    }
}

/// A box's four corners, clockwise from the top left as laid out, wherever
/// transforms took them (Blink's `gfx::QuadF`).
pub type Quad = [PointF; 4];

/// An element's content, padding, border and margin boxes as quads, for
/// DevTools' box model and highlight (Blink's
/// `InspectorHighlightBase::BuildNodeQuads`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxQuads {
    pub content: Quad,
    pub padding: Quad,
    pub border: Quad,
    pub margin: Quad,
}

fn quad(rect: RectF, to: &Mapping) -> Quad {
    [
        to.map_point(PointF::new(rect.x, rect.y)),
        to.map_point(PointF::new(rect.x + rect.width, rect.y)),
        to.map_point(PointF::new(rect.x + rect.width, rect.y + rect.height)),
        to.map_point(PointF::new(rect.x, rect.y + rect.height)),
    ]
}

/// `rect` less `sides`, never of a negative size.
fn contract(rect: RectF, sides: &computed_style::Sides<f32>) -> RectF {
    RectF::new(
        rect.x + sides.left,
        rect.y + sides.top,
        (rect.width - sides.left - sides.right).max(0.0),
        (rect.height - sides.top - sides.bottom).max(0.0),
    )
}

/// `element`'s box model in the viewport (as `client_rects` has it),
/// through transforms and the scroll offsets (Blink's
/// `BuildNodeQuads`): a box's content, padding, border and margin boxes
/// (the margins clamped to no negative size); an inline element's around
/// its boxes on the lines (Blink's `PhysicalLinesBoundingBox`), its border
/// and padding its first box's at the start and top and its last's at the
/// end and bottom, its margin box its border box (machopo's inline boxes
/// keep no margins; Blink's has the horizontal ones). None without a box.
pub fn box_quads(
    layout_view: &BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Option<BoxQuads> {
    pseudo_box_quads(layout_view, viewport_scroll, scroll, sticky, element, None)
}

/// `box_quads` of `element`'s pseudo-element `pseudo` (its own, if none):
/// its boxes are those of the element's marked as its (DevTools'
/// `getBoxModel` of a `::before`).
pub fn pseudo_box_quads(
    layout_view: &BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
    pseudo: Option<computed_style::PseudoId>,
) -> Option<BoxQuads> {
    let found = box_of(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        element,
        pseudo,
    );
    let mut boxes = Vec::new();
    if found.is_none() {
        walk(
            layout_view,
            viewport_scroll,
            scroll,
            sticky,
            &mut |fragment, offset, _, contents| {
                let lines = &fragment.lines;
                inline_boxes_in(lines, offset, contents, element, pseudo, &mut boxes);
                true
            },
        );
    }
    quads_of(found, &boxes)
}

/// An inline element's box on a line, as `box_quads` adds them up: its
/// rect in the root's space, its border and padding, and how far the
/// page's scroll moves it.
type InlineQuadBox = (
    RectF,
    computed_style::Sides<f32>,
    computed_style::Sides<f32>,
    PointF,
);

/// `element`'s (or its `pseudo`'s) boxes on `fragment`'s lines, at `offset`
/// in the space `contents` maps from.
fn inline_boxes_in(
    lines: &[LineFragment],
    offset: PointF,
    contents: &Mapping,
    element: NodeId,
    pseudo: Option<computed_style::PseudoId>,
    boxes: &mut Vec<InlineQuadBox>,
) {
    for line in lines {
        for inline_box in line
            .boxes
            .iter()
            .filter(|b| b.node == element && b.pseudo == pseudo)
        {
            // Added up in the root's space, scrolled once added.
            let rect = inline_box.rect.offset(offset.x, offset.y);
            boxes.push((
                contents.to_root.map_rect(rect),
                inline_box.border,
                inline_box.padding,
                contents.scrolled,
            ));
        }
    }
}

/// `box_quads` of a box (`found`, as `box_of` gives it), or else of an
/// inline element's boxes on the lines, in tree order.
fn quads_of(found: Option<(&BoxFragment, Mapping)>, boxes: &[InlineQuadBox]) -> Option<BoxQuads> {
    if let Some((fragment, mapping)) = found {
        let border = RectF::new(0.0, 0.0, fragment.size.width, fragment.size.height);
        let padding = contract(border, &fragment.border);
        let content = contract(padding, &fragment.padding);
        let margins = &fragment.margin;
        let margin = RectF::new(
            -margins.left,
            -margins.top,
            (border.width + margins.left + margins.right).max(0.0),
            (border.height + margins.top + margins.bottom).max(0.0),
        );
        return Some(BoxQuads {
            content: quad(content, &mapping),
            padding: quad(padding, &mapping),
            border: quad(border, &mapping),
            margin: quad(margin, &mapping),
        });
    }
    let (first, last) = (boxes.first()?, boxes.last()?);
    let border = bounding_rect(&boxes.iter().map(|(rect, ..)| *rect).collect::<Vec<_>>());
    let border_sides = computed_style::Sides {
        top: first.1.top,
        right: last.1.right,
        bottom: last.1.bottom,
        left: first.1.left,
    };
    let padding_sides = computed_style::Sides {
        top: first.2.top,
        right: last.2.right,
        bottom: last.2.bottom,
        left: first.2.left,
    };
    let padding = contract(border, &border_sides);
    let content = contract(padding, &padding_sides);
    let from_root = Mapping {
        to_root: Transform::IDENTITY,
        scrolled: first.3,
    };
    Some(BoxQuads {
        content: quad(content, &from_root),
        padding: quad(padding, &from_root),
        border: quad(border, &from_root),
        margin: quad(border, &from_root),
    })
}

/// `element`'s border boxes as quads in the viewport, in tree order, as
/// `client_rects` finds them but not made rects: a box's border
/// box through its transforms, an inline element's box on each line
/// (DevTools' `getContentQuads`; Blink's `CollectQuads` goes into an
/// inline element's children instead). Empty without a box.
pub fn border_quads(
    layout_view: &BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Vec<Quad> {
    let mut quads = Vec::new();
    walk(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        &mut |fragment, offset, mapping, contents| {
            let lines = &fragment.lines;
            border_quads_in(
                fragment, lines, offset, mapping, contents, element, &mut quads,
            );
            true
        },
    );
    quads
}

/// `border_quads`' of `element` that `fragment` has, as `client_rects_in`.
fn border_quads_in(
    fragment: &BoxFragment,
    lines: &[LineFragment],
    offset: PointF,
    mapping: &Mapping,
    contents: &Mapping,
    element: NodeId,
    quads: &mut Vec<Quad>,
) {
    if fragment.node == Some(element) && fragment.pseudo.is_none() {
        let border = RectF::new(
            offset.x,
            offset.y,
            fragment.size.width,
            fragment.size.height,
        );
        quads.push(quad(border, mapping));
    }
    for line in lines {
        for inline_box in line
            .boxes
            .iter()
            .filter(|b| b.node == element && b.pseudo.is_none())
        {
            quads.push(quad(inline_box.rect.offset(offset.x, offset.y), contents));
        }
    }
}

/// The boxes of text node `text`'s data from `start` to `end` (UTF-16 code
/// units), in the space and order of `client_rects`'s (Blink's
/// `LayoutText::AbsoluteQuadsForRange`): one for each of its runs on a
/// line that the range is in, from where the range starts in the run to
/// where it ends, as tall as the run's font (its ascent and descent around
/// the baseline, not the line). A point inside a cluster (in a surrogate
/// pair, a grapheme, a ligature) takes all of it, as Chrome's; one in
/// white space that collapsed away is where the next character is. A
/// collapsed range has one box, of no width, in the run it is in, else
/// (at the end of the data) the last. Empty if the text has no box.
pub fn text_rects(
    layout_view: &BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    text: NodeId,
    start: u32,
    end: u32,
) -> Vec<RectF> {
    let mut rects = Vec::new();
    // A collapsed range at the end of a run: the last such, if no run
    // holds it.
    let mut at_end = None;
    walk(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        &mut |fragment, offset, _, contents| {
            for run in fragment.lines.iter().flat_map(|line| &line.runs) {
                let Some((node, source)) = &run.source else {
                    continue;
                };
                if *node != text {
                    continue;
                }
                let ends_here = start == end && start == source.end;
                let holds = if start == end {
                    source.start <= start && start < source.end
                } else {
                    start < source.end && source.start < end
                };
                if !holds && !ends_here {
                    continue;
                }
                let (left, right) = run.x_range(start, end);
                let rect = RectF::new(
                    left + offset.x,
                    run.baseline - run.ascent + offset.y,
                    right - left,
                    run.ascent + run.descent,
                );
                let rect = contents.map_rect(rect);
                if !holds {
                    at_end = Some(rect);
                    continue;
                }
                rects.push(rect);
                if start == end {
                    return false;
                }
            }
            true
        },
    );
    if rects.is_empty() {
        rects.extend(at_end);
    }
    rects
}

/// Pre-paint's walk, mapping instead of recording: `visit` sees each
/// fragment under the `LayoutView` with where its border box is in the
/// space the first mapping maps from, and the mapping of its contents (its
/// lines and children, moved by its scroll offset); it returns whether to
/// go on.
///
/// The page's scroll, `viewport_scroll`, is the root's step: its children
/// are moved by it but the ones fixed to the viewport, which are all among
/// them, and what is in a child goes with it (Blink's
/// `OffsetFromScrollableContainer`: nothing from the `LayoutView` for a
/// fixed box).
fn walk<'a>(
    layout_view: &'a BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    visit: &mut dyn FnMut(&'a BoxFragment, PointF, &Mapping, &Mapping) -> bool,
) {
    fn go<'a>(
        fragment: &'a BoxFragment,
        offset: PointF,
        mapping: Mapping,
        scroll: &ScrollOffsets,
        sticky: &StickyOffsets,
        visit: &mut dyn FnMut(&'a BoxFragment, PointF, &Mapping, &Mapping) -> bool,
    ) -> bool {
        let (offset, mapping, contents) = enter(fragment, offset, mapping, scroll, sticky);
        if !visit(fragment, offset, &mapping, &contents) {
            return false;
        }
        for child in &fragment.children {
            let at = PointF::new(offset.x + child.offset.x, offset.y + child.offset.y);
            if !go(&child.fragment, at, contents, scroll, sticky, visit) {
                return false;
            }
        }
        true
    }
    // Not the `LayoutView` itself, which is no node's and has no lines.
    for child in &layout_view.children {
        let mapping = from_root(child, viewport_scroll);
        if !go(
            &child.fragment,
            child.offset,
            mapping,
            scroll,
            sticky,
            visit,
        ) {
            return;
        }
    }
}

/// The mapping the `LayoutView`'s `child` starts from: the page's scroll,
/// unless it is fixed to the viewport.
fn from_root(child: &FragmentLink, viewport_scroll: PointF) -> Mapping {
    let scrolled = if child.fragment.fixed_to_viewport {
        PointF::new(0.0, 0.0)
    } else {
        viewport_scroll
    };
    Mapping {
        to_root: Transform::IDENTITY,
        scrolled,
    }
}

/// The step into `fragment`, at `offset` in the space `mapping` maps from,
/// that the walk and the way down to one box both take, so that they add
/// up the same, to the bit: where its border box is (moved by how far it
/// sticks), its mapping (through its transform), and its contents' (moved
/// by its scroll offset).
fn enter(
    fragment: &BoxFragment,
    offset: PointF,
    mapping: Mapping,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
) -> (PointF, Mapping, Mapping) {
    let (mut mapping, mut offset) = (mapping, stuck(fragment, offset, sticky));
    if let Some(transform) = box_transform(fragment) {
        mapping = mapping
            .then(&Transform::translate(offset.x, offset.y))
            .then(&transform);
        offset = PointF::new(0.0, 0.0);
    }
    // A scroll container's content (its lines and children) moves by its
    // offset. The element whose `overflow` went to the viewport has
    // `visible` here: the viewport's offset is the root's.
    let contents = match fragment.node {
        Some(node) if fragment.style.is_scroll_container() => {
            let at = scroll.get(&node).copied().unwrap_or(PointF::new(0.0, 0.0));
            mapping.then(&Transform::translate(-at.x, -at.y))
        }
        _ => mapping,
    };
    (offset, mapping, contents)
}

/// `offset`, where `fragment` is, moved by how far it sticks (Blink's
/// `OffsetFromContainer` adds `StickyPositionOffset`).
pub(crate) fn stuck(fragment: &BoxFragment, offset: PointF, sticky: &StickyOffsets) -> PointF {
    match fragment.node {
        Some(node) if fragment.style.position == computed_style::Position::Sticky => {
            let by = sticky
                .get(&(node, fragment.pseudo))
                .copied()
                .unwrap_or_default();
            PointF::new(offset.x + by.x, offset.y + by.y)
        }
        _ => offset,
    }
}

/// `element`'s box (the first, if it has several), and the mapping from
/// its border box's space (its origin at the border box's) to the
/// viewport, as `client_rects` maps it.
pub(crate) fn box_to_viewport<'a>(
    layout_view: &'a BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Option<(&'a BoxFragment, Mapping)> {
    box_of(layout_view, viewport_scroll, scroll, sticky, element, None)
}

/// `box_to_viewport` of `element`'s pseudo-element `pseudo`'s box (its
/// own, if none).
fn box_of<'a>(
    layout_view: &'a BoxFragment,
    viewport_scroll: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
    pseudo: Option<computed_style::PseudoId>,
) -> Option<(&'a BoxFragment, Mapping)> {
    let mut found = None;
    walk(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        &mut |fragment, offset, mapping, _| {
            if fragment.node == Some(element) && fragment.pseudo == pseudo {
                let to_box = Transform::translate(offset.x, offset.y);
                found = Some((fragment, mapping.then(&to_box)));
                return false;
            }
            true
        },
    );
    found
}

/// `element`'s box for `offset*`, `client*` and `scroll*`, in the root
/// fragment's space before transforms and scroll offsets: positions add
/// up each box's offset in its parent, as Blink's `PhysicalLocation`s do.
/// A scroll container's offset is `scroll`'s. `None` if it has no box.
pub fn element_box(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Option<ElementBox> {
    let mut inline = Vec::new();
    let found = find_box(
        layout_view,
        PointF::new(0.0, 0.0),
        scroll,
        sticky,
        element,
        &mut inline,
        None,
    );
    if found.is_some() {
        return found;
    }
    inline_element_box(&inline)
}

/// The box of `element` if it is `fragment` or below it, `fragment` being
/// at `offset`; an inline element's boxes on the lines go into `inline`.
fn find_box(
    fragment: &BoxFragment,
    offset: PointF,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
    inline: &mut Vec<RectF>,
    parent: Option<(&BoxFragment, PointF)>,
) -> Option<ElementBox> {
    let offset = stuck(fragment, offset, sticky);
    let lines = &fragment.lines;
    if let Some(found) = box_in(fragment, lines, offset, scroll, element, inline, parent) {
        return Some(found);
    }
    fragment.children.iter().find_map(|child| {
        let at = PointF::new(offset.x + child.offset.x, offset.y + child.offset.y);
        find_box(
            &child.fragment,
            at,
            scroll,
            sticky,
            element,
            inline,
            Some((fragment, child.offset)),
        )
    })
}

/// `element`'s box if it is `fragment`, at `offset` (moved by how far it
/// sticks), as `element_box` has it; otherwise the element's boxes on
/// `lines` (as `client_rects_in`'s) go into `inline`.
fn box_in(
    fragment: &BoxFragment,
    lines: &[LineFragment],
    offset: PointF,
    scroll: &ScrollOffsets,
    element: NodeId,
    inline: &mut Vec<RectF>,
    parent: Option<(&BoxFragment, PointF)>,
) -> Option<ElementBox> {
    if fragment.node == Some(element) && fragment.pseudo.is_none() {
        let (b, p, m) = (&fragment.border, &fragment.padding, &fragment.margin);
        let border_box = RectF::new(
            offset.x,
            offset.y,
            fragment.size.width,
            fragment.size.height,
        );
        let scroll = fragment.style.is_scroll_container().then(|| {
            scroll
                .get(&element)
                .copied()
                .unwrap_or(PointF::new(0.0, 0.0))
        });
        return Some(ElementBox {
            border_box,
            start: offset,
            block: Some(BlockBox {
                border: [b.top, b.right, b.bottom, b.left],
                padding: [p.top, p.right, p.bottom, p.left],
                margin: [m.top, m.right, m.bottom, m.left],
                inset: used_inset(fragment, parent),
                scroll,
            }),
        });
    }
    for line in lines {
        for inline_box in line
            .boxes
            .iter()
            .filter(|b| b.node == element && b.pseudo.is_none())
        {
            inline.push(inline_box.rect.offset(offset.x, offset.y));
        }
    }
    None
}

/// An element box from an inline element's boxes on the lines, in tree
/// order; none without one.
fn inline_element_box(inline: &[RectF]) -> Option<ElementBox> {
    let first = *inline.first()?;
    Some(ElementBox {
        border_box: inline[1..].iter().fold(first, |a, b| a.union(b)),
        start: PointF::new(first.x, first.y),
        block: None,
    })
}

/// `element`'s scrollable overflow, from its padding box's origin, at least
/// the padding box (`scrollWidth`, `scrollHeight`; Blink's
/// `LayoutBox::ScrollWidth`); `None` without a box of its own. Worked out
/// when asked: `element_box` leaves it out, as Blink's `OffsetWidth()`
/// doesn't touch it.
pub fn scroll_size(layout_view: &BoxFragment, element: NodeId) -> Option<SizeF> {
    fn find(fragment: &BoxFragment, element: NodeId) -> Option<&BoxFragment> {
        if fragment.node == Some(element) && fragment.pseudo.is_none() {
            return Some(fragment);
        }
        fragment
            .children
            .iter()
            .find_map(|child| find(&child.fragment, element))
    }
    let fragment = find(layout_view, element)?;
    Some(crate::scrollable_overflow(fragment).size())
}

/// The used value of each of `fragment`'s insets (top, right, bottom,
/// left), what CSSOM resolves `top` and its kin to for a positioned box;
/// `parent` is the fragment it is a child of, and where in it it sits. An
/// absolutely positioned box's is where it sits in its containing block's
/// padding box — the box it is a child of is that containing block (RFC
/// 0013's decision 11) — and a relatively positioned one's is how far it
/// moved. Zero for a static box, whose resolved value is `auto` instead.
fn used_inset(fragment: &BoxFragment, parent: Option<(&BoxFragment, PointF)>) -> [f32; 4] {
    let Some((parent, at)) = parent else {
        return [0.0; 4];
    };
    let style = &fragment.style;
    if style.is_out_of_flow() {
        let (b, m) = (&parent.border, &fragment.margin);
        // Its containing block is the padding box.
        let width = parent.size.width - b.left - b.right;
        let height = parent.size.height - b.top - b.bottom;
        // An inset it was given keeps its computed value, the percentage
        // of the containing block resolved; only an `auto` one takes the
        // value layout worked out, which is where the box ended up
        // (CSSOM's resolved value, and what Chrome writes).
        let computed = |value, basis| match value {
            LengthPercentageOrAuto::Auto => None,
            LengthPercentageOrAuto::LengthPercentage(v) => Some(v.resolve(basis)),
        };
        let (left, top) = (at.x - m.left - b.left, at.y - m.top - b.top);
        return [
            computed(style.inset.top, height).unwrap_or(top),
            computed(style.inset.right, width).unwrap_or(width - left - fragment.size.width),
            computed(style.inset.bottom, height).unwrap_or(height - top - fragment.size.height),
            computed(style.inset.left, width).unwrap_or(left),
        ];
    }
    if style.position == computed_style::Position::Relative {
        // Percentages are of the containing block: its parent's content box.
        let (b, p) = (&parent.border, &parent.padding);
        let width = parent.size.width - b.left - b.right - p.left - p.right;
        let height = parent.size.height - b.top - b.bottom - p.top - p.bottom;
        let computed = |value, basis| match value {
            LengthPercentageOrAuto::Auto => None,
            LengthPercentageOrAuto::LengthPercentage(v) => Some(v.resolve(basis)),
        };
        // As above: an inset it was given keeps its computed value; an
        // `auto` one is the other side's, negated (CSS 2 § 9.4.3), or 0.
        let (top, bottom) = (
            computed(style.inset.top, height),
            computed(style.inset.bottom, height),
        );
        let (left, right) = (
            computed(style.inset.left, width),
            computed(style.inset.right, width),
        );
        return [
            top.unwrap_or(-bottom.unwrap_or(0.0)),
            right.unwrap_or(-left.unwrap_or(0.0)),
            bottom.unwrap_or(-top.unwrap_or(0.0)),
            left.unwrap_or(-right.unwrap_or(0.0)),
        ];
    }
    if style.position == computed_style::Position::Sticky {
        // Percentages are of its scrollport, which the sticky boxes'
        // constraints have (the caller's); its parent's content box
        // until then. An `auto` one stays `auto`.
        let (b, p) = (&parent.border, &parent.padding);
        let width = parent.size.width - b.left - b.right - p.left - p.right;
        let height = parent.size.height - b.top - b.bottom - p.top - p.bottom;
        let computed = |value, basis| match value {
            LengthPercentageOrAuto::Auto => 0.0,
            LengthPercentageOrAuto::LengthPercentage(v) => v.resolve(basis),
        };
        return [
            computed(style.inset.top, height),
            computed(style.inset.right, width),
            computed(style.inset.bottom, height),
            computed(style.inset.left, width),
        ];
    }
    [0.0; 4]
}

/// CSSOM View's `getBoundingClientRect` from the element's `rects`: none
/// is an empty rect at the origin; if none has both a width and a height,
/// the first; otherwise the union of those that have.
pub fn bounding_rect(rects: &[RectF]) -> RectF {
    let Some(first) = rects.first() else {
        return RectF::new(0.0, 0.0, 0.0, 0.0);
    };
    rects
        .iter()
        .filter(|r| r.width != 0.0 && r.height != 0.0)
        .copied()
        .reduce(|a, b| a.union(&b))
        .unwrap_or(*first)
}

impl BoxTree {
    /// The geometry of `layout_view`, this tree's last layout, the page
    /// scrolled to `viewport_scroll` (as the free functions take it): each
    /// query finds the element's boxes from the tree's indexes and goes
    /// down only the way to them (Blink's `LayoutObject` climbing its
    /// containers), not through the whole tree. The tree mustn't have
    /// changed since that layout, which debug builds check.
    pub fn geometry<'a>(
        &'a self,
        layout_view: &'a BoxFragment,
        viewport_scroll: PointF,
        scroll: &'a ScrollOffsets,
        sticky: &'a StickyOffsets,
    ) -> Geometry<'a> {
        debug_assert!(
            self.boxes[LAYOUT_VIEW]
                .fragment
                .as_deref()
                .is_some_and(|last| std::ptr::eq(last, layout_view)),
            "geometry of a layout not this tree's last"
        );
        Geometry {
            tree: self,
            layout_view,
            viewport_scroll,
            scroll,
            sticky,
            entered: Cell::new(0),
            lines: Cell::new(0),
        }
    }

    /// Where box `index`'s fragment is under `layout_view`: each child's
    /// index on the way down, from the boxes' links. `None` if it isn't
    /// there: the box has no fragment, or a link on the way is stale (its
    /// fragment isn't the child there), which is what a box no longer laid
    /// out, or taken out, has.
    fn path(&self, layout_view: &BoxFragment, index: usize) -> Option<Vec<u32>> {
        let mut up = Vec::new();
        let mut at = index;
        while at != LAYOUT_VIEW {
            let (parent, child) = self.boxes.get(at)?.link?;
            up.push((at, child));
            // Each box is on the way once: a longer climb is a cycle of
            // stale links.
            if up.len() > self.boxes.len() {
                return None;
            }
            at = usize::try_from(parent).ok()?;
        }
        let mut fragment = layout_view;
        let mut path = Vec::with_capacity(up.len());
        for (at, child) in up.into_iter().rev() {
            let link = fragment.children.get(usize::try_from(child).ok()?)?;
            // The pointers alone, not the counts.
            if !Arc::ptr_eq(&link.fragment, self.boxes[at].fragment.as_ref()?) {
                return None;
            }
            fragment = &link.fragment;
            path.push(child);
        }
        Some(path)
    }

    /// The paths of the boxes that have a box of element `node`: its own,
    /// or those its inline boxes are in (an element has one or the other:
    /// an inline element is in its block's inline content instead), each
    /// once. In tree order, which is the paths' order: the walk's.
    fn paths(&self, layout_view: &BoxFragment, node: NodeId) -> Vec<Vec<u32>> {
        let mut paths: Vec<Vec<u32>> = self
            .elements
            .get(&node)
            .copied()
            .into_iter()
            .chain(
                self.inlines
                    .range((node, 0)..=(node, usize::MAX))
                    .map(|&(_, index)| index),
            )
            .filter_map(|index| self.path(layout_view, index))
            // The `LayoutView` is no node's.
            .filter(|path| !path.is_empty())
            .collect();
        paths.sort_unstable();
        paths
    }

    /// The fragments the links of the boxes in the tree find under
    /// `layout_view`, for tests: each of its fragments but itself once, if
    /// the links are right.
    pub fn linked_fragments_for_testing<'a>(
        &self,
        layout_view: &'a BoxFragment,
    ) -> Vec<&'a BoxFragment> {
        let unused: std::collections::HashSet<usize> = self.unused.iter().copied().collect();
        (0..self.boxes.len())
            .filter(|index| *index != LAYOUT_VIEW && !unused.contains(index))
            .filter_map(|index| {
                let path = self.path(layout_view, index)?;
                path.iter().try_fold(layout_view, |fragment, &child| {
                    let child = fragment.children.get(usize::try_from(child).ok()?)?;
                    Some(&*child.fragment)
                })
            })
            .collect()
    }
}

/// A box tree's geometry queries of its last layout
/// ([`BoxTree::geometry`]): the free functions' answers, each the same to
/// the bit, found from the element.
pub struct Geometry<'a> {
    tree: &'a BoxTree,
    layout_view: &'a BoxFragment,
    viewport_scroll: PointF,
    scroll: &'a ScrollOffsets,
    sticky: &'a StickyOffsets,
    /// How many fragments the queries went into, and how many lines they
    /// looked at, for tests.
    entered: Cell<usize>,
    lines: Cell<usize>,
}

impl<'a> Geometry<'a> {
    /// `element_box`.
    pub fn element_box(&self, element: NodeId) -> Option<ElementBox> {
        let mut inline = Vec::new();
        for path in self.tree.paths(self.layout_view, element) {
            // As `find_box` goes down, which `walk` doesn't.
            let (mut fragment, mut offset, mut parent) =
                (self.layout_view, PointF::new(0.0, 0.0), None);
            for &child in &path {
                let from = stuck(fragment, offset, self.sticky);
                let Some(child) = usize::try_from(child)
                    .ok()
                    .and_then(|child| fragment.children.get(child))
                else {
                    break;
                };
                self.count_entered();
                parent = Some((fragment, child.offset));
                offset = PointF::new(from.x + child.offset.x, from.y + child.offset.y);
                fragment = &child.fragment;
            }
            let offset = stuck(fragment, offset, self.sticky);
            let lines = self.lines_of(fragment, element);
            if let Some(found) = box_in(
                fragment,
                lines,
                offset,
                self.scroll,
                element,
                &mut inline,
                parent,
            ) {
                return Some(found);
            }
        }
        inline_element_box(&inline)
    }

    /// `client_rects`.
    pub fn client_rects(&self, element: NodeId) -> Vec<RectF> {
        let mut rects = Vec::new();
        for path in self.tree.paths(self.layout_view, element) {
            if let Some((fragment, offset, mapping, contents)) = self.mapped(&path) {
                let lines = self.lines_of(fragment, element);
                client_rects_in(
                    fragment, lines, offset, &mapping, &contents, element, &mut rects,
                );
            }
        }
        rects
    }

    /// `border_quads`.
    pub fn border_quads(&self, element: NodeId) -> Vec<Quad> {
        let mut quads = Vec::new();
        for path in self.tree.paths(self.layout_view, element) {
            if let Some((fragment, offset, mapping, contents)) = self.mapped(&path) {
                let lines = self.lines_of(fragment, element);
                border_quads_in(
                    fragment, lines, offset, &mapping, &contents, element, &mut quads,
                );
            }
        }
        quads
    }

    /// `box_quads`.
    pub fn box_quads(&self, element: NodeId) -> Option<BoxQuads> {
        let mut boxes = Vec::new();
        for path in self.tree.paths(self.layout_view, element) {
            let Some((fragment, offset, mapping, contents)) = self.mapped(&path) else {
                continue;
            };
            if fragment.node == Some(element) && fragment.pseudo.is_none() {
                let to_box = Transform::translate(offset.x, offset.y);
                return quads_of(Some((fragment, mapping.then(&to_box))), &[]);
            }
            let lines = self.lines_of(fragment, element);
            inline_boxes_in(lines, offset, &contents, element, None, &mut boxes);
        }
        quads_of(None, &boxes)
    }

    /// `scroll_size`.
    pub fn scroll_size(&self, element: NodeId) -> Option<SizeF> {
        self.tree
            .paths(self.layout_view, element)
            .iter()
            .find_map(|path| {
                let fragment = path.iter().try_fold(self.layout_view, |fragment, &child| {
                    let child = fragment.children.get(usize::try_from(child).ok()?)?;
                    self.count_entered();
                    Some(&*child.fragment)
                })?;
                (fragment.node == Some(element) && fragment.pseudo.is_none())
                    .then(|| crate::scrollable_overflow(fragment).size())
            })
    }

    /// How many fragments the queries so far went into, for tests: how
    /// deep the boxes are, not how many there are.
    pub fn entered_for_testing(&self) -> usize {
        self.entered.get()
    }

    fn count_entered(&self) {
        self.entered.set(self.entered.get().saturating_add(1));
    }

    /// How many lines the queries so far looked at, for tests: those of
    /// the elements, not of the paragraphs they are in.
    pub fn lines_for_testing(&self) -> usize {
        self.lines.get()
    }

    /// `fragment`'s lines `element` has boxes on.
    fn lines_of(&self, fragment: &'a BoxFragment, element: NodeId) -> &'a [LineFragment] {
        let lines = fragment.inline_lines.of(&fragment.lines, element);
        self.lines.set(self.lines.get().saturating_add(lines.len()));
        lines
    }

    /// The fragment at the end of `path`, as `walk` visits it: where its
    /// border box is in the space its mapping maps from, the mapping, and
    /// its contents'.
    fn mapped(&self, path: &[u32]) -> Option<(&'a BoxFragment, PointF, Mapping, Mapping)> {
        let (&first, rest) = path.split_first()?;
        let child = self
            .layout_view
            .children
            .get(usize::try_from(first).ok()?)?;
        let (mut fragment, mut offset) = (&*child.fragment, child.offset);
        let mut mapping = from_root(child, self.viewport_scroll);
        let mut rest = rest.iter();
        loop {
            self.count_entered();
            let (at, own, contents) = enter(fragment, offset, mapping, self.scroll, self.sticky);
            let Some(&next) = rest.next() else {
                return Some((fragment, at, own, contents));
            };
            let child = fragment.children.get(usize::try_from(next).ok()?)?;
            offset = PointF::new(at.x + child.offset.x, at.y + child.offset.y);
            mapping = contents;
            fragment = &child.fragment;
        }
    }
}
