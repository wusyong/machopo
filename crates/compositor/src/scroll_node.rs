//! What scrolling one scroller needs to know (`cc::ScrollNode`), and the
//! math on it that cc keeps on `cc::ScrollTree`. The main thread and the
//! compositor scroll with the same math, so a scroll moves the same way
//! whichever thread handles it.

use gfx::{PointF, SizeF};

/// `cc::ScrollNode`: a scroller's port and content, in CSS px. Like cc's,
/// it holds no offset: whoever scrolls keeps that.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollNode {
    /// What shows the content (`container_bounds`): the viewport, or a
    /// scroll container's padding box.
    pub container_bounds: SizeF,
    /// The content's size (`bounds`): the scrollable overflow.
    pub bounds: SizeF,
    /// Whether the user can scroll each axis (wheel, keys). Script and
    /// `#fragment`s scroll either way.
    pub user_scrollable_horizontal: bool,
    pub user_scrollable_vertical: bool,
}

impl ScrollNode {
    /// `ScrollTree::MaxScrollOffset`: how far the content can scroll; zero
    /// on an axis where it fits.
    pub fn max_scroll_offset(&self) -> SizeF {
        SizeF::new(
            (self.bounds.width - self.container_bounds.width).max(0.0),
            (self.bounds.height - self.container_bounds.height).max(0.0),
        )
    }

    /// `ScrollTree::ClampScrollOffsetToLimits`: `offset` within
    /// `0..=max_scroll_offset()`, whatever the user may scroll.
    pub fn clamp_scroll_offset(&self, offset: PointF) -> PointF {
        let max = self.max_scroll_offset();
        PointF::new(
            offset.x.clamp(0.0, max.width),
            offset.y.clamp(0.0, max.height),
        )
    }

    /// `ScrollTree::ScrollBy`: moves `offset` by as much of `rest` as fits,
    /// on the axes the user can scroll, and takes that from `rest`; what is
    /// left is for the scroller around this one. Whether it moved.
    pub fn scroll_by(&self, offset: &mut PointF, rest: &mut PointF) -> bool {
        let max = self.max_scroll_offset();
        let mut moved = false;
        if self.user_scrollable_horizontal {
            moved |= take(&mut offset.x, &mut rest.x, max.width);
        }
        if self.user_scrollable_vertical {
            moved |= take(&mut offset.y, &mut rest.y, max.height);
        }
        moved
    }
}

/// Moves `offset` by as much of `rest` as fits in `0..=max`, taking that
/// from `rest`. Whether it moved.
fn take(offset: &mut f32, rest: &mut f32, max: f32) -> bool {
    let next = (*offset + *rest).clamp(0.0, max);
    let moved = next - *offset;
    *offset = next;
    *rest -= moved;
    moved != 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 100x50 port over 300x250 of content, scrollable both ways.
    fn node() -> ScrollNode {
        ScrollNode {
            container_bounds: SizeF::new(100.0, 50.0),
            bounds: SizeF::new(300.0, 250.0),
            user_scrollable_horizontal: true,
            user_scrollable_vertical: true,
        }
    }

    #[test]
    fn the_max_offset_is_what_overflows_the_port() {
        assert_eq!(node().max_scroll_offset(), SizeF::new(200.0, 200.0));
        let fits = ScrollNode {
            bounds: SizeF::new(80.0, 20.0),
            ..node()
        };
        assert_eq!(fits.max_scroll_offset(), SizeF::new(0.0, 0.0));
    }

    #[test]
    fn clamping_keeps_both_ends_whatever_the_user_may_scroll() {
        let hidden = ScrollNode {
            user_scrollable_horizontal: false,
            user_scrollable_vertical: false,
            ..node()
        };
        for n in [node(), hidden] {
            assert_eq!(
                n.clamp_scroll_offset(PointF::new(-5.0, 500.0)),
                PointF::new(0.0, 200.0)
            );
            assert_eq!(
                n.clamp_scroll_offset(PointF::new(500.0, -5.0)),
                PointF::new(200.0, 0.0)
            );
            assert_eq!(
                n.clamp_scroll_offset(PointF::new(30.0, 40.0)),
                PointF::new(30.0, 40.0)
            );
        }
    }

    #[test]
    fn scroll_by_takes_what_fits_and_leaves_the_rest() {
        let n = node();
        let mut offset = PointF::new(0.0, 150.0);
        let mut rest = PointF::new(30.0, 80.0);
        assert!(n.scroll_by(&mut offset, &mut rest));
        assert_eq!(offset, PointF::new(30.0, 200.0));
        assert_eq!(rest, PointF::new(0.0, 30.0), "50 of the 80 fit");

        // At the end: nothing moves, all of it is left.
        let mut rest = PointF::new(0.0, 10.0);
        assert!(!n.scroll_by(&mut offset, &mut rest));
        assert_eq!(rest, PointF::new(0.0, 10.0));

        // Back up, past the start.
        let mut rest = PointF::new(-50.0, -300.0);
        assert!(n.scroll_by(&mut offset, &mut rest));
        assert_eq!(offset, PointF::new(0.0, 0.0));
        assert_eq!(rest, PointF::new(-20.0, -100.0));
    }

    #[test]
    fn scroll_by_leaves_an_axis_the_user_cant_scroll() {
        let n = ScrollNode {
            user_scrollable_vertical: false,
            ..node()
        };
        let mut offset = PointF::new(0.0, 0.0);
        let mut rest = PointF::new(10.0, 10.0);
        assert!(n.scroll_by(&mut offset, &mut rest));
        assert_eq!(offset, PointF::new(10.0, 0.0));
        assert_eq!(rest, PointF::new(0.0, 10.0));

        let n = ScrollNode {
            user_scrollable_horizontal: false,
            user_scrollable_vertical: false,
            ..node()
        };
        let mut rest = PointF::new(10.0, 10.0);
        assert!(!n.scroll_by(&mut offset, &mut rest));
        assert_eq!(
            (offset, rest),
            (PointF::new(10.0, 0.0), PointF::new(10.0, 10.0))
        );
    }

    #[test]
    fn scroll_by_says_whether_either_axis_moved() {
        let n = node();
        // Only y can move: still moved.
        let mut offset = PointF::new(200.0, 0.0);
        let mut rest = PointF::new(10.0, 10.0);
        assert!(n.scroll_by(&mut offset, &mut rest));
        // Only x can move.
        let mut offset = PointF::new(0.0, 200.0);
        let mut rest = PointF::new(10.0, 10.0);
        assert!(n.scroll_by(&mut offset, &mut rest));
    }
}
