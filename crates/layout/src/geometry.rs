//! Where an element's boxes are, for script (Blink's
//! `LayoutObject::AbsoluteQuads` / `LocalToAbsolute`): its border box, or
//! an inline element's box on each line, mapped from the fragment tree
//! through transforms and the scroll containers' offsets.
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

/// `element`'s boxes in the root fragment's space (the document's
/// coordinates: the viewport's scroll offset isn't taken off), in tree
/// order: its border box, or an inline element's box on each line (CSSOM
/// View's `getClientRects`), each the bounding rect of the box as
/// transformed, moved by the offsets of the scroll containers around it
/// (`scroll`), not cut by clips. Empty if it has no box.
pub fn client_rects(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Vec<RectF> {
    let mut rects = Vec::new();
    walk(
        layout_view,
        scroll,
        sticky,
        &mut |fragment, offset, to_root, contents| {
            if fragment.node == Some(element) && fragment.pseudo.is_none() {
                let border_box = RectF::new(
                    offset.x,
                    offset.y,
                    fragment.size.width,
                    fragment.size.height,
                );
                rects.push(to_root.map_rect(border_box));
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
/// fragment with where its border box is in the space `to_root` maps from,
/// and the transform for its contents (its lines and children, moved by
/// its scroll offset); it returns whether to go on.
fn walk(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    visit: &mut dyn FnMut(&BoxFragment, PointF, &Transform, &Transform) -> bool,
) {
    fn go(
        fragment: &BoxFragment,
        offset: PointF,
        to_root: Transform,
        scroll: &ScrollOffsets,
        sticky: &StickyOffsets,
        visit: &mut dyn FnMut(&BoxFragment, PointF, &Transform, &Transform) -> bool,
    ) -> bool {
        let (mut to_root, mut offset) = (to_root, stuck(fragment, offset, sticky));
        if let Some(transform) = box_transform(fragment) {
            to_root = to_root
                .then(&Transform::translate(offset.x, offset.y))
                .then(&transform);
            offset = PointF::new(0.0, 0.0);
        }
        // A scroll container's content (its lines and children) moves by
        // its offset. The element whose `overflow` went to the viewport has
        // `visible` here: the viewport's offset is the caller's.
        let contents = match fragment.node {
            Some(node) if fragment.style.is_scroll_container() => {
                let at = scroll.get(&node).copied().unwrap_or(PointF::new(0.0, 0.0));
                to_root.then(&Transform::translate(-at.x, -at.y))
            }
            _ => to_root,
        };
        if !visit(fragment, offset, &to_root, &contents) {
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
    go(
        layout_view,
        PointF::new(0.0, 0.0),
        Transform::IDENTITY,
        scroll,
        sticky,
        visit,
    );
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

/// `element`'s box (the first, if it has several), and the transform from
/// its border box's space (its origin at the border box's) to the root
/// fragment's, through transforms and the scroll containers' offsets, as
/// `client_rects` maps it.
pub(crate) fn box_to_root<'a>(
    layout_view: &'a BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: NodeId,
) -> Option<(&'a BoxFragment, Transform)> {
    fn find<'a>(
        fragment: &'a BoxFragment,
        offset: PointF,
        to_root: Transform,
        scroll: &ScrollOffsets,
        sticky: &StickyOffsets,
        element: NodeId,
    ) -> Option<(&'a BoxFragment, Transform)> {
        let (mut to_root, mut offset) = (to_root, stuck(fragment, offset, sticky));
        if let Some(transform) = box_transform(fragment) {
            to_root = to_root
                .then(&Transform::translate(offset.x, offset.y))
                .then(&transform);
            offset = PointF::new(0.0, 0.0);
        }
        if fragment.node == Some(element) && fragment.pseudo.is_none() {
            return Some((
                fragment,
                to_root.then(&Transform::translate(offset.x, offset.y)),
            ));
        }
        let contents = match fragment.node {
            Some(node) if fragment.style.is_scroll_container() => {
                let at = scroll.get(&node).copied().unwrap_or(PointF::new(0.0, 0.0));
                to_root.then(&Transform::translate(-at.x, -at.y))
            }
            _ => to_root,
        };
        fragment.children.iter().find_map(|child| {
            let at = PointF::new(offset.x + child.offset.x, offset.y + child.offset.y);
            find(&child.fragment, at, contents, scroll, sticky, element)
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
                scroll_size: crate::scrollable_overflow(fragment),
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

/// Whether `element`'s box is fixed to the viewport (`position: fixed`,
/// and no box around it its containing block): it is laid out against
/// the viewport, so where layout put it is where it shows, however far the
/// document is scrolled.
pub fn is_fixed(layout_view: &BoxFragment, element: NodeId) -> bool {
    fn find(fragment: &BoxFragment, element: NodeId) -> bool {
        if fragment.node == Some(element) && fragment.pseudo.is_none() {
            return fragment.fixed_to_viewport;
        }
        fragment
            .children
            .iter()
            .any(|child| find(&child.fragment, element))
    }
    find(layout_view, element)
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
