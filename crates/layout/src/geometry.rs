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

use computed_style::{LengthPercentageOrAuto, TransformBox};
use gfx::{BlockBox, ElementBox, NodeId, NodeIdHashMap};
use gfx::{PointF, RectF, Transform};

use crate::{BoxFragment, StickyOffsets};

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
            if fragment.node == Some(element) && fragment.pseudo.is_none() {
                let border_box = RectF::new(
                    offset.x,
                    offset.y,
                    fragment.size.width,
                    fragment.size.height,
                );
                rects.push(mapping.map_rect(border_box));
            }
            for line in &fragment.lines {
                for inline_box in line
                    .boxes
                    .iter()
                    .filter(|b| b.node == element && b.pseudo.is_none())
                {
                    rects.push(contents.map_rect(inline_box.rect.offset(offset.x, offset.y)));
                }
            }
            true
        },
    );
    rects
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
    if let Some((fragment, mapping)) = box_of(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        element,
        pseudo,
    ) {
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
    let mut boxes = Vec::new();
    walk(
        layout_view,
        viewport_scroll,
        scroll,
        sticky,
        &mut |fragment, offset, _, contents| {
            for line in &fragment.lines {
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
            true
        },
    );
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
            if fragment.node == Some(element) && fragment.pseudo.is_none() {
                let border = RectF::new(
                    offset.x,
                    offset.y,
                    fragment.size.width,
                    fragment.size.height,
                );
                quads.push(quad(border, mapping));
            }
            for line in &fragment.lines {
                for inline_box in line
                    .boxes
                    .iter()
                    .filter(|b| b.node == element && b.pseudo.is_none())
                {
                    quads.push(quad(inline_box.rect.offset(offset.x, offset.y), contents));
                }
            }
            true
        },
    );
    quads
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
        let (mut mapping, mut offset) = (mapping, stuck(fragment, offset, sticky));
        if let Some(transform) = box_transform(fragment) {
            mapping = mapping
                .then(&Transform::translate(offset.x, offset.y))
                .then(&transform);
            offset = PointF::new(0.0, 0.0);
        }
        // A scroll container's content (its lines and children) moves by
        // its offset. The element whose `overflow` went to the viewport has
        // `visible` here: the viewport's offset is the root's.
        let contents = match fragment.node {
            Some(node) if fragment.style.is_scroll_container() => {
                let at = scroll.get(&node).copied().unwrap_or(PointF::new(0.0, 0.0));
                mapping.then(&Transform::translate(-at.x, -at.y))
            }
            _ => mapping,
        };
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
        let scrolled = if child.fragment.fixed_to_viewport {
            PointF::new(0.0, 0.0)
        } else {
            viewport_scroll
        };
        let mapping = Mapping {
            to_root: Transform::IDENTITY,
            scrolled,
        };
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
    let first = *inline.first()?;
    Some(ElementBox {
        border_box: inline[1..].iter().fold(first, |a, b| a.union(b)),
        start: PointF::new(first.x, first.y),
        block: None,
    })
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
                scroll_size: crate::scrollable_overflow(fragment).size(),
                scroll,
            }),
        });
    }
    for line in &fragment.lines {
        for inline_box in line
            .boxes
            .iter()
            .filter(|b| b.node == element && b.pseudo.is_none())
        {
            inline.push(inline_box.rect.offset(offset.x, offset.y));
        }
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
