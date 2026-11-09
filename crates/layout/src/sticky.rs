//! `position: sticky` after layout (Blink's
//! `StickyPositionScrollingConstraints`): where each sticky box is in the
//! scrollport it sticks in, and how far it moves as that scrolls.
//!
//! The boxes are found under the fragments that say a sticky box is below
//! them (`BoxFragment::sticky_below`), each with the scroll container
//! nearest above it, or the viewport (Blink's `UpdateAllStickyConstraints`
//! after layout, from the sticky descendants a scroll container's fragment
//! took). Their rects are in that container's content space before it
//! scrolls, its padding box's origin there; transforms are ignored, as
//! Blink maps them with `kIgnoreTransforms`. A scroll changes only the
//! offsets, which [`StickyPositions::offsets`] works out from the rects
//! again (Blink's `ComputeStickyOffset`), with no walk of the fragments.

use computed_style::{LengthPercentageOrAuto, PseudoId};
use gfx::NodeId;
use gfx::{PointF, RectF, SizeF};
use rustc_hash::FxHashMap;

use crate::table::TablePart;
use crate::{BoxFragment, BoxRole, ScrollOffsets};

/// A box's element, and the pseudo-element of it that generated it.
pub type StickyKey = (NodeId, Option<PseudoId>);

/// How far each sticky box is moved, by its key.
pub type StickyOffsets = FxHashMap<StickyKey, PointF>;

/// How far each sticky box may move, by its key: the rect of the offsets
/// it can take, the boxes around it standing still.
pub type StickyReaches = FxHashMap<StickyKey, RectF>;

/// What paint takes of the sticky boxes: how far each is moved now, and
/// how far it may be as its scrollport scrolls.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StickyState {
    pub offsets: StickyOffsets,
    pub reaches: StickyReaches,
}

/// One sticky box's constraints (Blink's `StickyConstraintsData`, both
/// axes): every rect in its scroller's content space, unscrolled.
#[derive(Debug, Clone, PartialEq)]
pub struct StickyConstraints {
    pub key: StickyKey,
    /// The scroll container whose scrollport it sticks in; `None`: the
    /// frame's viewport.
    pub scroller: Option<NodeId>,
    /// In a box fixed to the viewport, with no scroll container between:
    /// the viewport's scrolling doesn't move it (Blink's
    /// `is_fixed_to_view`).
    pub fixed_to_view: bool,
    /// What it stays inside, before the scroll offset moves it: the
    /// scrollport's content box (`ComputeStickyConstrainingRect`).
    pub constraining_rect: RectF,
    /// Its border box.
    pub sticky_box: RectF,
    /// Its containing block's content box, less its own margins: as far
    /// as it may move (the scroll container's scrollable overflow if that
    /// is its containing block).
    pub containing_block: RectF,
    /// Its `top`, `right`, `bottom` and `left` (`None` for `auto`),
    /// percentages of the constraining rect's size; the end one less what
    /// both don't leave room for.
    pub inset: [Option<f32>; 4],
    /// The same as resolved, before either gives way: what
    /// `getComputedStyle` says (`min_inset_for_get_computed_style`).
    pub resolved_inset: [Option<f32>; 4],
    /// The sticky box nearest above it below its scroller, at or above its
    /// containing block, by its index: moving, it moves this one's
    /// containing block (Blink's `NearestStickyLayerShiftingContainingBlock`).
    pub shifting_containing_block: Option<usize>,
}

/// The sticky boxes of a fragment tree, each after the sticky boxes above
/// it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StickyPositions {
    pub boxes: Vec<StickyConstraints>,
}

/// Whether `fragment` sticks: sticky with an inset, and a box that can
/// be (not a table's row, row group or cell, whose containing blocks are
/// their table's parts).
pub fn sticks(fragment: &BoxFragment) -> bool {
    fragment.style.is_sticky_constrained()
        && !matches!(
            TablePart::of(fragment.style.display),
            Some(TablePart::RowGroup | TablePart::Row | TablePart::Cell)
        )
}

/// The scroller the walk is in.
struct Scroller {
    element: Option<NodeId>,
    constraining_rect: RectF,
    fixed_to_view: bool,
}

impl StickyPositions {
    /// The sticky boxes under `layout_view`, a frame's, whose viewport is
    /// `viewport` big; empty if none is.
    pub fn new(layout_view: &BoxFragment, viewport: SizeF) -> Self {
        let mut positions = Self::default();
        if layout_view.sticky_below {
            let scroller = Scroller {
                element: None,
                constraining_rect: RectF::new(0.0, 0.0, viewport.width, viewport.height),
                fixed_to_view: false,
            };
            let origin = PointF::new(0.0, 0.0);
            positions.walk(
                layout_view,
                origin,
                &scroller,
                (layout_view, origin, true),
                None,
            );
        }
        positions
    }

    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// How far each box may move: from its place in the flow to its
    /// containing block's edges, on the axes it has an inset on.
    pub fn reaches(&self) -> StickyReaches {
        self.boxes
            .iter()
            .map(|c| {
                let [top, right, bottom, left] = c.inset;
                let (b, cb) = (c.sticky_box, c.containing_block);
                let reach = |has: bool, by: f32| if has { by } else { 0.0 };
                let (x0, x1) = (
                    reach(right.is_some(), (cb.x - b.x).min(0.0)),
                    reach(left.is_some(), (cb.right() - b.right()).max(0.0)),
                );
                let (y0, y1) = (
                    reach(bottom.is_some(), (cb.y - b.y).min(0.0)),
                    reach(top.is_some(), (cb.bottom() - b.bottom()).max(0.0)),
                );
                (c.key, RectF::new(x0, y0, x1 - x0, y1 - y0))
            })
            .collect()
    }

    /// The constraints of the box of `key`, if it sticks.
    pub fn get(&self, key: StickyKey) -> Option<&StickyConstraints> {
        self.boxes.iter().find(|c| c.key == key)
    }

    /// `fragment`, at `at` in `scroller`'s content space, and what is
    /// below it; `containing_block` is its parent's nearest box that isn't
    /// anonymous, where that is, and whether it is the scroller's own box;
    /// `sticky` is the sticky box nearest above it there.
    fn walk<'a>(
        &mut self,
        fragment: &'a BoxFragment,
        at: PointF,
        scroller: &Scroller,
        containing_block: (&'a BoxFragment, PointF, bool),
        sticky: Option<usize>,
    ) {
        let mut sticky = sticky;
        if sticks(fragment)
            && let Some(node) = fragment.node
        {
            let constraints =
                self.constraints(fragment, node, at, scroller, containing_block, sticky);
            sticky = Some(self.boxes.len());
            self.boxes.push(constraints);
        }
        if !fragment.sticky_below {
            return;
        }
        // A scroll container's content is in its own space, from its
        // padding box's origin; the boxes in it stick in its scrollport.
        // The element whose `overflow` went to the viewport has `visible`
        // here: the layout view is the viewport's.
        let inner = match fragment.node {
            Some(element) if fragment.style.is_scroll_container() => {
                let (b, p) = (&fragment.border, &fragment.padding);
                Some(Scroller {
                    element: Some(element),
                    constraining_rect: RectF::new(
                        p.left,
                        p.top,
                        fragment.size.width - b.left - b.right - p.left - p.right,
                        fragment.size.height - b.top - b.bottom - p.top - p.bottom,
                    ),
                    fixed_to_view: false,
                })
            }
            _ => None,
        };
        let (scroller, origin, sticky, containing_block) = match &inner {
            Some(inner) => {
                let b = &fragment.border;
                let origin = PointF::new(-b.left, -b.top);
                (inner, origin, None, (fragment, origin, true))
            }
            // The initial containing block is the viewport's scroller's.
            None if fragment.role == BoxRole::LayoutView => {
                (scroller, at, sticky, (fragment, at, true))
            }
            // A box's anonymous blocks and table boxes aren't a
            // containing block: the one around them is (Blink's
            // `StickyContainer` skips them).
            None if matches!(
                fragment.role,
                BoxRole::AnonymousBlock | BoxRole::AnonymousTable
            ) =>
            {
                (scroller, at, sticky, containing_block)
            }
            None => (scroller, at, sticky, (fragment, at, false)),
        };
        for child in &fragment.children {
            let child_at = PointF::new(origin.x + child.offset.x, origin.y + child.offset.y);
            let child = &child.fragment;
            if !child.sticky_below && !sticks(child) {
                continue;
            }
            // Fixed to the viewport: laid out against it, which the
            // viewport's scrolling doesn't move.
            if child.fixed_to_viewport {
                let fixed = Scroller {
                    element: None,
                    constraining_rect: scroller.constraining_rect,
                    fixed_to_view: true,
                };
                self.walk(child, child_at, &fixed, containing_block, sticky);
            } else {
                self.walk(child, child_at, scroller, containing_block, sticky);
            }
        }
    }

    /// Blink's `ComputeStickyPositionConstraints`, both axes.
    fn constraints(
        &self,
        fragment: &BoxFragment,
        node: NodeId,
        at: PointF,
        scroller: &Scroller,
        containing_block: (&BoxFragment, PointF, bool),
        sticky: Option<usize>,
    ) -> StickyConstraints {
        let style = &fragment.style;
        let (block, block_at, scrolls) = containing_block;
        let (b, p) = (&block.border, &block.padding);
        // Its content box: the scroll container's scrollable overflow if
        // that is its containing block, else its padding box less its
        // padding.
        let padding_box = if scrolls {
            let overflow = crate::scrollable_overflow(block);
            RectF::new(0.0, 0.0, overflow.width, overflow.height)
        } else {
            RectF::new(
                block_at.x + b.left,
                block_at.y + b.top,
                block.size.width - b.left - b.right,
                block.size.height - b.top - b.bottom,
            )
        };
        let content_width = (block.size.width - b.left - b.right - p.left - p.right).max(0.0);
        // Its margins as written, `auto` as 0, percentages of its
        // containing block's width.
        let margin = |value: LengthPercentageOrAuto| match value {
            LengthPercentageOrAuto::Auto => 0.0,
            LengthPercentageOrAuto::LengthPercentage(v) => v.resolve(content_width),
        };
        let m = &style.margin;
        let (top, right, bottom, left) = (
            p.top + margin(m.top),
            p.right + margin(m.right),
            p.bottom + margin(m.bottom),
            p.left + margin(m.left),
        );
        let containing = RectF::new(
            padding_box.x + left,
            padding_box.y + top,
            padding_box.width - left - right,
            padding_box.height - top - bottom,
        );
        let constraining = scroller.constraining_rect;
        let inset = |value: LengthPercentageOrAuto, base: f32| match value {
            LengthPercentageOrAuto::Auto => None,
            LengthPercentageOrAuto::LengthPercentage(v) => Some(v.resolve(base)),
        };
        // Both insets of an axis where there isn't room for both: the end
        // one gives way.
        let axis = |min: Option<f32>, max: Option<f32>, available: f32, size: f32| match (min, max)
        {
            (Some(min), Some(max)) => {
                let free = available - size - min - max;
                (Some(min), Some(if free < 0.0 { max + free } else { max }))
            }
            other => other,
        };
        let resolved_inset = [
            inset(style.inset.top, constraining.height),
            inset(style.inset.right, constraining.width),
            inset(style.inset.bottom, constraining.height),
            inset(style.inset.left, constraining.width),
        ];
        let [top, right, bottom, left] = resolved_inset;
        let (top, bottom) = axis(top, bottom, constraining.height, fragment.size.height);
        let (left, right) = axis(left, right, constraining.width, fragment.size.width);
        StickyConstraints {
            key: (node, fragment.pseudo),
            scroller: scroller.element,
            fixed_to_view: scroller.fixed_to_view,
            constraining_rect: constraining,
            sticky_box: RectF::new(at.x, at.y, fragment.size.width, fragment.size.height),
            containing_block: containing,
            inset: [top, right, bottom, left],
            resolved_inset,
            shifting_containing_block: sticky,
        }
    }

    /// How far each box moves with the containers at `scroll` and the
    /// viewport at `viewport` (Blink's `ComputeStickyOffset`): into its
    /// scrollport by its insets, no further than its containing block,
    /// after the sticky box around that moved.
    pub fn offsets(&self, scroll: &ScrollOffsets, viewport: PointF) -> StickyOffsets {
        // Each box's own offset, and with the one moving its containing
        // block (`total_containing_block_sticky_offset`).
        let mut totals: Vec<PointF> = Vec::with_capacity(self.boxes.len());
        let mut offsets = StickyOffsets::default();
        for constraints in &self.boxes {
            let ancestor = constraints
                .shifting_containing_block
                .map_or(PointF::new(0.0, 0.0), |index| totals[index]);
            let scrolled = if constraints.fixed_to_view {
                PointF::new(0.0, 0.0)
            } else {
                match constraints.scroller {
                    None => viewport,
                    Some(element) => scroll.get(&element).copied().unwrap_or_default(),
                }
            };
            let c = constraints;
            let sticky_box = c.sticky_box.offset(ancestor.x, ancestor.y);
            let containing = c.containing_block.offset(ancestor.x, ancestor.y);
            let content = c.constraining_rect.offset(scrolled.x, scrolled.y);
            let [top, right, bottom, left] = c.inset;
            let x = axis_offset(
                (sticky_box.x, sticky_box.right()),
                (containing.x, containing.right()),
                (content.x, content.right()),
                left,
                right,
            );
            let y = axis_offset(
                (sticky_box.y, sticky_box.bottom()),
                (containing.y, containing.bottom()),
                (content.y, content.bottom()),
                top,
                bottom,
            );
            totals.push(PointF::new(ancestor.x + x, ancestor.y + y));
            offsets.insert(c.key, PointF::new(x, y));
        }
        offsets
    }
}

/// One axis of `PerAxisData::ComputeOffset`: the box's, its containing
/// block's and the scrollport's start and end; the end inset first, then
/// the start one, which wins.
fn axis_offset(
    sticky_box: (f32, f32),
    containing: (f32, f32),
    content: (f32, f32),
    min: Option<f32>,
    max: Option<f32>,
) -> f32 {
    let mut delta = 0.0;
    if let Some(max) = max {
        // Up (or left) to keep its end inside, no further than its
        // containing block's start.
        let limit = content.1 - max;
        let wanted = (limit - sticky_box.1).min(0.0);
        let available = (containing.0 - sticky_box.0).min(0.0);
        delta += wanted.max(available);
    }
    if let Some(min) = min {
        let limit = content.0 + min;
        let wanted = (limit - sticky_box.0).max(0.0);
        let available = (containing.1 - sticky_box.1).max(0.0);
        delta += wanted.min(available);
    }
    delta
}
