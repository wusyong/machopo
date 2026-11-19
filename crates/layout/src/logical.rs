//! Layout's logical geometry: sizes, offsets, rects and box edges in a
//! box's inline and block directions, and the one conversion to and from
//! the physical ones (Blink's `core/layout/geometry/`: `LogicalSize`,
//! `LogicalOffset`, `LogicalRect`, `BoxStrut`, `WritingModeConverter`).
//!
//! machopo's own algorithms place in these and turn the result physical
//! once the box's size is known, writing it into taffy's `Layout`; the
//! fragment tree, paint and hit testing see only physical geometry. With
//! `horizontal-tb` and `ltr` the conversion is the identity.

#![cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "the algorithms that place in logical coordinates take the rest up one by one"
    )
)]

use computed_style::{Sides, WritingDirectionMode, WritingMode};
use gfx::{PointF, RectF, SizeF};

/// A size along the inline and block directions (Blink's `LogicalSize`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct LogicalSize {
    pub(crate) inline_size: f32,
    pub(crate) block_size: f32,
}

impl LogicalSize {
    pub(crate) const fn new(inline_size: f32, block_size: f32) -> Self {
        Self {
            inline_size,
            block_size,
        }
    }

    pub(crate) fn from_physical(size: SizeF, writing_direction: WritingDirectionMode) -> Self {
        if writing_direction.is_horizontal() {
            Self::new(size.width, size.height)
        } else {
            Self::new(size.height, size.width)
        }
    }

    pub(crate) fn to_physical(self, writing_direction: WritingDirectionMode) -> SizeF {
        if writing_direction.is_horizontal() {
            SizeF::new(self.inline_size, self.block_size)
        } else {
            SizeF::new(self.block_size, self.inline_size)
        }
    }
}

/// An offset from a box's inline-start, block-start corner (Blink's
/// `LogicalOffset`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct LogicalOffset {
    pub(crate) inline_offset: f32,
    pub(crate) block_offset: f32,
}

impl LogicalOffset {
    pub(crate) const fn new(inline_offset: f32, block_offset: f32) -> Self {
        Self {
            inline_offset,
            block_offset,
        }
    }
}

/// A rect in a box's logical coordinates (Blink's `LogicalRect`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct LogicalRect {
    pub(crate) offset: LogicalOffset,
    pub(crate) size: LogicalSize,
}

impl LogicalRect {
    pub(crate) const fn new(offset: LogicalOffset, size: LogicalSize) -> Self {
        Self { offset, size }
    }
}

/// A value per logical side: margins, borders, paddings along the inline
/// and block directions (Blink's `BoxStrut`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct BoxStrut {
    pub(crate) inline_start: f32,
    pub(crate) inline_end: f32,
    pub(crate) block_start: f32,
    pub(crate) block_end: f32,
}

impl BoxStrut {
    pub(crate) fn from_physical(
        sides: Sides<f32>,
        writing_direction: WritingDirectionMode,
    ) -> Self {
        let side = |side: PhysicalSide| match side {
            PhysicalSide::Top => sides.top,
            PhysicalSide::Right => sides.right,
            PhysicalSide::Bottom => sides.bottom,
            PhysicalSide::Left => sides.left,
        };
        let (inline_start, block_start) = start_sides(writing_direction);
        Self {
            inline_start: side(inline_start),
            inline_end: side(inline_start.opposite()),
            block_start: side(block_start),
            block_end: side(block_start.opposite()),
        }
    }

    pub(crate) fn to_physical(self, writing_direction: WritingDirectionMode) -> Sides<f32> {
        let (inline_start, block_start) = start_sides(writing_direction);
        let on = |side: PhysicalSide| {
            if side == inline_start {
                self.inline_start
            } else if side == inline_start.opposite() {
                self.inline_end
            } else if side == block_start {
                self.block_start
            } else {
                self.block_end
            }
        };
        Sides {
            top: on(PhysicalSide::Top),
            right: on(PhysicalSide::Right),
            bottom: on(PhysicalSide::Bottom),
            left: on(PhysicalSide::Left),
        }
    }

    pub(crate) fn inline_sum(self) -> f32 {
        self.inline_start + self.inline_end
    }

    pub(crate) fn block_sum(self) -> f32 {
        self.block_start + self.block_end
    }
}

/// A side of a box on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PhysicalSide {
    Top,
    Right,
    Bottom,
    Left,
}

impl PhysicalSide {
    pub(crate) fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Right => Self::Left,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
        }
    }
}

/// The physical sides the inline-start and block-start sides are on
/// (CSS Writing Modes 4 § 6.4's table).
pub(crate) fn start_sides(writing_direction: WritingDirectionMode) -> (PhysicalSide, PhysicalSide) {
    let ltr = writing_direction.is_ltr();
    match writing_direction.writing_mode {
        WritingMode::HorizontalTb if ltr => (PhysicalSide::Left, PhysicalSide::Top),
        WritingMode::HorizontalTb => (PhysicalSide::Right, PhysicalSide::Top),
        WritingMode::VerticalRl | WritingMode::SidewaysRl if ltr => {
            (PhysicalSide::Top, PhysicalSide::Right)
        }
        WritingMode::VerticalRl | WritingMode::SidewaysRl => {
            (PhysicalSide::Bottom, PhysicalSide::Right)
        }
        WritingMode::VerticalLr if ltr => (PhysicalSide::Top, PhysicalSide::Left),
        WritingMode::VerticalLr => (PhysicalSide::Bottom, PhysicalSide::Left),
        WritingMode::SidewaysLr if ltr => (PhysicalSide::Bottom, PhysicalSide::Left),
        WritingMode::SidewaysLr => (PhysicalSide::Top, PhysicalSide::Left),
    }
}

/// Turns a box's logical coordinates into physical ones and back (Blink's
/// `WritingModeConverter`): offsets of what is inside it, from its
/// inline-start, block-start corner to (physically) its top left. Turning
/// a child's offset needs the child's size, as where its top left is
/// depends on which of its corners the offset is to; and the box's own
/// size, which is why a box's children are turned physical only once its
/// size is known.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WritingModeConverter {
    writing_direction: WritingDirectionMode,
    /// The box's physical size.
    outer_size: SizeF,
}

impl WritingModeConverter {
    pub(crate) const fn new(writing_direction: WritingDirectionMode, outer_size: SizeF) -> Self {
        Self {
            writing_direction,
            outer_size,
        }
    }

    /// Where something of physical size `inner_size` at `offset` has its
    /// top left.
    pub(crate) fn to_physical_offset(&self, offset: LogicalOffset, inner_size: SizeF) -> PointF {
        let outer = self.outer_size;
        // From the far edge, for the axes that run right to left or bottom
        // to top.
        let from_right = |at: f32| outer.width - at - inner_size.width;
        let from_bottom = |at: f32| outer.height - at - inner_size.height;
        let (inline, block) = (offset.inline_offset, offset.block_offset);
        let ltr = self.writing_direction.is_ltr();
        match self.writing_direction.writing_mode {
            WritingMode::HorizontalTb if ltr => PointF::new(inline, block),
            WritingMode::HorizontalTb => PointF::new(from_right(inline), block),
            WritingMode::VerticalRl | WritingMode::SidewaysRl if ltr => {
                PointF::new(from_right(block), inline)
            }
            WritingMode::VerticalRl | WritingMode::SidewaysRl => {
                PointF::new(from_right(block), from_bottom(inline))
            }
            WritingMode::VerticalLr if ltr => PointF::new(block, inline),
            WritingMode::VerticalLr => PointF::new(block, from_bottom(inline)),
            WritingMode::SidewaysLr if ltr => PointF::new(block, from_bottom(inline)),
            WritingMode::SidewaysLr => PointF::new(block, inline),
        }
    }

    /// The logical offset of something of physical size `inner_size`
    /// whose top left is at `offset`.
    pub(crate) fn to_logical_offset(&self, offset: PointF, inner_size: SizeF) -> LogicalOffset {
        // Every case of `to_physical_offset` is its own inverse once the
        // axes are swapped back: measuring from the far edge twice is
        // measuring from the near one.
        let swapped = if self.writing_direction.is_horizontal() {
            LogicalOffset::new(offset.x, offset.y)
        } else {
            LogicalOffset::new(offset.y, offset.x)
        };
        let back = self.to_physical_offset(swapped, inner_size);
        if self.writing_direction.is_horizontal() {
            LogicalOffset::new(back.x, back.y)
        } else {
            LogicalOffset::new(back.y, back.x)
        }
    }

    pub(crate) fn to_physical_size(&self, size: LogicalSize) -> SizeF {
        size.to_physical(self.writing_direction)
    }

    pub(crate) fn to_logical_size(&self, size: SizeF) -> LogicalSize {
        LogicalSize::from_physical(size, self.writing_direction)
    }

    pub(crate) fn to_physical_rect(&self, rect: LogicalRect) -> RectF {
        let size = self.to_physical_size(rect.size);
        let at = self.to_physical_offset(rect.offset, size);
        RectF::new(at.x, at.y, size.width, size.height)
    }

    pub(crate) fn to_logical_rect(&self, rect: RectF) -> LogicalRect {
        let size = rect.size();
        LogicalRect::new(
            self.to_logical_offset(PointF::new(rect.x, rect.y), size),
            self.to_logical_size(size),
        )
    }
}

/// How far something moved by `by` along the inline and block directions
/// moves on the screen: a distance, not a place, so no box's size comes
/// in.
pub(crate) fn to_physical_delta(
    writing_direction: WritingDirectionMode,
    by: LogicalOffset,
) -> PointF {
    let along = |side: PhysicalSide, v: f32| match side {
        PhysicalSide::Left => PointF::new(v, 0.0),
        PhysicalSide::Right => PointF::new(-v, 0.0),
        PhysicalSide::Top => PointF::new(0.0, v),
        PhysicalSide::Bottom => PointF::new(0.0, -v),
    };
    let (inline_start, block_start) = start_sides(writing_direction);
    let (inline, block) = (
        along(inline_start, by.inline_offset),
        along(block_start, by.block_offset),
    );
    PointF::new(inline.x + block.x, inline.y + block.y)
}

/// Where a box's content box has its top left, from its border box's: its
/// border and padding (`inset`, per physical side) in from its
/// inline-start and block-start sides, and `block_shift` further along
/// the block direction (content moved down as a whole, as a button's
/// centered), turned physical by its writing direction.
pub(crate) fn content_box_origin(
    writing_direction: WritingDirectionMode,
    border_box: SizeF,
    inset: Sides<f32>,
    block_shift: f32,
) -> PointF {
    let inset = BoxStrut::from_physical(inset, writing_direction);
    let size = LogicalSize::from_physical(border_box, writing_direction);
    let content = LogicalRect::new(
        LogicalOffset::new(inset.inline_start, inset.block_start + block_shift),
        LogicalSize::new(
            (size.inline_size - inset.inline_sum()).max(0.0),
            (size.block_size - inset.block_sum() - block_shift).max(0.0),
        ),
    );
    let rect = WritingModeConverter::new(writing_direction, border_box).to_physical_rect(content);
    PointF::new(rect.x, rect.y)
}

#[cfg(test)]
mod tests {
    use computed_style::Direction;

    use super::*;

    const ALL: [(WritingMode, Direction); 10] = [
        (WritingMode::HorizontalTb, Direction::Ltr),
        (WritingMode::HorizontalTb, Direction::Rtl),
        (WritingMode::VerticalRl, Direction::Ltr),
        (WritingMode::VerticalRl, Direction::Rtl),
        (WritingMode::VerticalLr, Direction::Ltr),
        (WritingMode::VerticalLr, Direction::Rtl),
        (WritingMode::SidewaysRl, Direction::Ltr),
        (WritingMode::SidewaysRl, Direction::Rtl),
        (WritingMode::SidewaysLr, Direction::Ltr),
        (WritingMode::SidewaysLr, Direction::Rtl),
    ];

    fn mode(writing_mode: WritingMode, direction: Direction) -> WritingDirectionMode {
        WritingDirectionMode::new(writing_mode, direction)
    }

    /// A 10x20 box 3 along the inline direction and 5 along the block in a
    /// 100x200 one lands where Blink's `WritingModeConverter` puts it.
    #[test]
    fn offsets_turn_physical_as_in_each_mode() {
        let outer = SizeF::new(100.0, 200.0);
        let inner = SizeF::new(10.0, 20.0);
        let offset = LogicalOffset::new(3.0, 5.0);
        for ((writing_mode, direction), expected) in ALL.into_iter().zip([
            (3.0, 5.0),
            (87.0, 5.0),
            (85.0, 3.0),
            (85.0, 177.0),
            (5.0, 3.0),
            (5.0, 177.0),
            (85.0, 3.0),
            (85.0, 177.0),
            (5.0, 177.0),
            (5.0, 3.0),
        ]) {
            let converter = WritingModeConverter::new(mode(writing_mode, direction), outer);
            assert_eq!(
                converter.to_physical_offset(offset, inner),
                PointF::new(expected.0, expected.1),
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                converter.to_logical_offset(PointF::new(expected.0, expected.1), inner),
                offset,
                "{writing_mode:?} {direction:?}"
            );
        }
    }

    #[test]
    fn sizes_swap_in_the_vertical_modes() {
        let size = LogicalSize::new(10.0, 20.0);
        for (writing_mode, direction) in ALL {
            let writing_direction = mode(writing_mode, direction);
            let physical = size.to_physical(writing_direction);
            let expected = if writing_mode == WritingMode::HorizontalTb {
                SizeF::new(10.0, 20.0)
            } else {
                SizeF::new(20.0, 10.0)
            };
            assert_eq!(physical, expected, "{writing_mode:?} {direction:?}");
            assert_eq!(
                LogicalSize::from_physical(physical, writing_direction),
                size
            );
        }
    }

    /// A logical rect's physical one: its size swapped as the mode's, its
    /// offset turned with that size.
    #[test]
    fn rects_turn_physical_with_their_own_size() {
        let outer = SizeF::new(100.0, 200.0);
        let rect = LogicalRect::new(LogicalOffset::new(3.0, 5.0), LogicalSize::new(20.0, 10.0));
        for ((writing_mode, direction), expected) in ALL.into_iter().zip([
            RectF::new(3.0, 5.0, 20.0, 10.0),
            RectF::new(77.0, 5.0, 20.0, 10.0),
            RectF::new(85.0, 3.0, 10.0, 20.0),
            RectF::new(85.0, 177.0, 10.0, 20.0),
            RectF::new(5.0, 3.0, 10.0, 20.0),
            RectF::new(5.0, 177.0, 10.0, 20.0),
            RectF::new(85.0, 3.0, 10.0, 20.0),
            RectF::new(85.0, 177.0, 10.0, 20.0),
            RectF::new(5.0, 177.0, 10.0, 20.0),
            RectF::new(5.0, 3.0, 10.0, 20.0),
        ]) {
            let converter = WritingModeConverter::new(mode(writing_mode, direction), outer);
            assert_eq!(
                converter.to_physical_rect(rect),
                expected,
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(
                converter.to_logical_rect(expected),
                rect,
                "{writing_mode:?} {direction:?}"
            );
        }
    }

    /// Top 1, right 2, bottom 3, left 4, read as inline-start, inline-end,
    /// block-start, block-end (CSS Writing Modes 4 § 6.4).
    #[test]
    fn struts_map_sides_as_in_each_mode() {
        let sides = Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        };
        for ((writing_mode, direction), [inline_start, inline_end, block_start, block_end]) in
            ALL.into_iter().zip([
                [4.0, 2.0, 1.0, 3.0],
                [2.0, 4.0, 1.0, 3.0],
                [1.0, 3.0, 2.0, 4.0],
                [3.0, 1.0, 2.0, 4.0],
                [1.0, 3.0, 4.0, 2.0],
                [3.0, 1.0, 4.0, 2.0],
                [1.0, 3.0, 2.0, 4.0],
                [3.0, 1.0, 2.0, 4.0],
                [3.0, 1.0, 4.0, 2.0],
                [1.0, 3.0, 4.0, 2.0],
            ])
        {
            let writing_direction = mode(writing_mode, direction);
            let strut = BoxStrut::from_physical(sides, writing_direction);
            assert_eq!(
                strut,
                BoxStrut {
                    inline_start,
                    inline_end,
                    block_start,
                    block_end,
                },
                "{writing_mode:?} {direction:?}"
            );
            assert_eq!(strut.to_physical(writing_direction), sides);
            assert_eq!(strut.inline_sum(), inline_start + inline_end);
            assert_eq!(strut.block_sum(), block_start + block_end);
        }
    }

    /// Border and padding of 1, 2, 3, 4 on the top, right, bottom and left
    /// of a 100x200 box, content moved 5 along the block direction: the
    /// content box's top left is in by its sides, shifted only when its
    /// block-start side is on the left or top.
    #[test]
    fn content_boxes_start_in_from_their_start_sides() {
        let inset = Sides {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 4.0,
        };
        let border_box = SizeF::new(100.0, 200.0);
        for ((writing_mode, direction), expected) in ALL.into_iter().zip([
            (4.0, 6.0),
            (4.0, 6.0),
            (4.0, 1.0),
            (4.0, 1.0),
            (9.0, 1.0),
            (9.0, 1.0),
            (4.0, 1.0),
            (4.0, 1.0),
            (9.0, 1.0),
            (9.0, 1.0),
        ]) {
            assert_eq!(
                content_box_origin(mode(writing_mode, direction), border_box, inset, 5.0),
                PointF::new(expected.0, expected.1),
                "{writing_mode:?} {direction:?}"
            );
        }
    }

    /// 3 along the inline direction and 5 along the block: toward each
    /// mode's inline-end and block-end sides.
    #[test]
    fn deltas_point_toward_the_end_sides() {
        for ((writing_mode, direction), expected) in ALL.into_iter().zip([
            (3.0, 5.0),
            (-3.0, 5.0),
            (-5.0, 3.0),
            (-5.0, -3.0),
            (5.0, 3.0),
            (5.0, -3.0),
            (-5.0, 3.0),
            (-5.0, -3.0),
            (5.0, -3.0),
            (5.0, 3.0),
        ]) {
            assert_eq!(
                to_physical_delta(mode(writing_mode, direction), LogicalOffset::new(3.0, 5.0)),
                PointF::new(expected.0, expected.1),
                "{writing_mode:?} {direction:?}"
            );
        }
    }
}
