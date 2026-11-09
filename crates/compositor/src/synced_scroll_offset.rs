//! The viewport's scroll offset as both threads see it (cc's
//! `SyncedProperty` for scroll offsets): what the main thread committed,
//! plus what the compositor scrolled since, kept apart by which commit will
//! include it.
//!
//! What the compositor scrolls goes to the main thread two ways: with a
//! `BeginMainFrame` (as in Chromium), and with each input forwarded to it
//! (so the main thread hit tests where the user sees things). The main
//! thread works through its queue in order and commits only at the end of
//! a main frame, so a commit includes what went with its `BeginMainFrame`
//! and before, and none of what went with input after it.

use gfx::PointF;

use crate::ScrollNode;

/// Offsets in CSS px. The one drawn is the sum of the four parts.
#[derive(Debug, Default)]
pub(crate) struct SyncedScrollOffset {
    /// The main thread's last committed offset.
    base: PointF,
    /// Handed to the main thread with the last `BeginMainFrame` posted, or
    /// before it: that main frame's commit includes it
    /// (`reflected_delta_in_main_tree`).
    reflected: PointF,
    /// Handed over with each input forwarded since, one per input, in
    /// order: the main thread applies them after the commit in flight.
    after: Vec<PointF>,
    /// Scrolled here, not handed over yet.
    unsent: PointF,
}

impl SyncedScrollOffset {
    /// Where the viewport is now, as drawn.
    pub(crate) fn current(&self) -> PointF {
        self.after.iter().fold(
            add(add(self.base, self.reflected), self.unsent),
            |sum, a| add(sum, *a),
        )
    }

    /// The impl side scrolled `node` by as much of `rest` as fits, which
    /// is taken from `rest` (`ScrollNode::scroll_by`). Whether it moved.
    pub(crate) fn scroll_by(&mut self, node: &ScrollNode, rest: &mut PointF) -> bool {
        let before = self.current();
        let mut offset = before;
        if !node.scroll_by(&mut offset, rest) {
            return false;
        }
        self.unsent = add(
            self.unsent,
            PointF::new(offset.x - before.x, offset.y - before.y),
        );
        true
    }

    /// What an input forwarded to the main thread carries: all that is
    /// unsent, from now on one more part of `after`.
    pub(crate) fn take_for_input(&mut self) -> Option<PointF> {
        let delta = std::mem::take(&mut self.unsent);
        if is_zero(delta) {
            return None;
        }
        self.after.push(delta);
        Some(delta)
    }

    /// What a `BeginMainFrame` carries (`PullDeltaForMainThread`): all that
    /// is unsent. Its commit will include that and everything handed over
    /// before.
    pub(crate) fn take_for_main_frame(&mut self) -> Option<PointF> {
        let delta = std::mem::take(&mut self.unsent);
        for a in self.after.drain(..) {
            self.reflected = add(self.reflected, a);
        }
        self.reflected = add(self.reflected, delta);
        (!is_zero(delta)).then_some(delta)
    }

    /// Whether the main thread has something still to get, or to confirm
    /// with a commit. What it got with a `BeginMainFrame` that was aborted
    /// waits for the next commit it makes anyway.
    pub(crate) fn needs_main_frame(&self) -> bool {
        !is_zero(self.unsent) || !self.after.is_empty()
    }

    /// A commit (`PushMainToPending`): `base` is the main thread's offset,
    /// which includes everything reflected. What it hasn't applied yet is
    /// clamped to `node` in the order it will apply it, as it will clamp
    /// it: each input's part, then the unsent part. Only a part that leaves
    /// the range changes, so one that stays isn't nudged by rounding.
    pub(crate) fn push_from_main(&mut self, base: PointF, node: Option<&ScrollNode>) {
        self.base = base;
        self.reflected = PointF::new(0.0, 0.0);
        let Some(node) = node else {
            return;
        };
        let max = node.max_scroll_offset();
        let mut at = base;
        for part in self
            .after
            .iter_mut()
            .chain(std::iter::once(&mut self.unsent))
        {
            clamp_part(&mut at.x, &mut part.x, max.width);
            clamp_part(&mut at.y, &mut part.y, max.height);
        }
    }

    /// The main thread's offset, with nothing scrolled here: another
    /// document's, or a single-threaded host's.
    pub(crate) fn reset(&mut self, base: PointF) {
        *self = Self {
            base,
            ..Self::default()
        };
    }
}

/// Moves `at` by `part`, keeping it in `0..=max`, and shrinks `part` to
/// what was taken if it went out.
fn clamp_part(at: &mut f32, part: &mut f32, max: f32) {
    let next = *at + *part;
    if (0.0..=max).contains(&next) {
        *at = next;
    } else {
        let clamped = next.clamp(0.0, max);
        *part = clamped - *at;
        *at = clamped;
    }
}

fn add(a: PointF, b: PointF) -> PointF {
    PointF::new(a.x + b.x, a.y + b.y)
}

fn is_zero(p: PointF) -> bool {
    p.x == 0.0 && p.y == 0.0
}

#[cfg(test)]
mod tests {
    use gfx::SizeF;

    use super::*;

    /// A 100x100 viewport over a page `height` high, scrollable both ways.
    fn node(height: f32) -> ScrollNode {
        ScrollNode {
            container_bounds: SizeF::new(100.0, 100.0),
            bounds: SizeF::new(100.0, height),
            user_scrollable_horizontal: true,
            user_scrollable_vertical: true,
        }
    }

    fn down(y: f32) -> PointF {
        PointF::new(0.0, y)
    }

    #[test]
    fn a_commit_confirms_what_went_with_its_main_frame_not_input_after_it() {
        let n = node(1000.0);
        let mut s = SyncedScrollOffset::default();
        s.scroll_by(&n, &mut down(50.0));
        assert_eq!(s.take_for_main_frame(), Some(down(50.0)));
        s.scroll_by(&n, &mut down(30.0));
        assert_eq!(s.take_for_input(), Some(down(30.0)));
        // The commit of that main frame has the 50, not the 30 yet.
        s.push_from_main(down(50.0), Some(&n));
        assert_eq!(s.current(), down(80.0));
        assert!(s.needs_main_frame(), "the 30 waits for a commit");
        // The next main frame's commit has both.
        assert_eq!(s.take_for_main_frame(), None);
        s.push_from_main(down(80.0), Some(&n));
        assert_eq!(s.current(), down(80.0));
        assert!(!s.needs_main_frame());
    }

    #[test]
    fn what_is_scrolled_meanwhile_stays_on_top_of_the_commit() {
        let n = node(1000.0);
        let mut s = SyncedScrollOffset::default();
        s.scroll_by(&n, &mut down(30.0));
        s.take_for_main_frame();
        s.scroll_by(&n, &mut down(20.0));
        // The main thread moved it itself too (a `#fragment`).
        s.push_from_main(down(500.0), Some(&n));
        assert_eq!(s.current(), down(520.0));
        assert_eq!(s.take_for_main_frame(), Some(down(20.0)));
    }

    #[test]
    fn nothing_moves_at_an_abort_and_the_next_commit_has_it_all() {
        let n = node(1000.0);
        let mut s = SyncedScrollOffset::default();
        s.scroll_by(&n, &mut down(50.0));
        s.take_for_main_frame();
        // Aborted: the main thread applied the 50 but didn't commit.
        assert_eq!(s.current(), down(50.0));
        assert!(!s.needs_main_frame(), "it has it already");
        s.scroll_by(&n, &mut down(10.0));
        assert_eq!(s.take_for_main_frame(), Some(down(10.0)));
        s.push_from_main(down(60.0), Some(&n));
        assert_eq!(s.current(), down(60.0));
    }

    #[test]
    fn a_smaller_page_clamps_each_part_in_the_order_main_applies_them() {
        let mut s = SyncedScrollOffset::default();
        let tall = node(1000.0);
        s.scroll_by(&tall, &mut down(100.0));
        s.take_for_main_frame();
        s.scroll_by(&tall, &mut down(60.0));
        s.take_for_input();
        s.scroll_by(&tall, &mut down(-40.0));
        s.take_for_input();
        s.scroll_by(&tall, &mut down(5.0));
        // The page is now 220 high: 120 to scroll. The main thread will
        // apply +60 (to 120, clamped), -40 (to 80), then +5 (to 85).
        s.push_from_main(down(100.0), Some(&node(220.0)));
        assert_eq!(s.current(), down(85.0));
        assert_eq!(s.after, [down(20.0), down(-40.0)]);
        assert_eq!(s.unsent, down(5.0));
    }

    #[test]
    fn a_part_that_stays_in_range_is_left_as_it_is() {
        let mut s = SyncedScrollOffset::default();
        let n = ScrollNode {
            bounds: SizeF::new(1000.0, 1000.0),
            ..node(1000.0)
        };
        s.scroll_by(&n, &mut down(0.1));
        s.take_for_input();
        s.scroll_by(&n, &mut PointF::new(0.2, 0.0));
        s.push_from_main(down(0.3), Some(&n));
        assert_eq!(s.after, [down(0.1)]);
        assert_eq!(s.unsent, PointF::new(0.2, 0.0));
    }

    #[test]
    fn a_confirmed_commit_leaves_no_residue() {
        let mut s = SyncedScrollOffset::default();
        let n = node(1000.0);
        s.scroll_by(&n, &mut down(0.1));
        s.take_for_input();
        s.scroll_by(&n, &mut down(0.2));
        s.take_for_main_frame();
        s.push_from_main(down(0.1 + 0.2), Some(&n));
        assert!(!s.needs_main_frame());
        assert_eq!(s.take_for_main_frame(), None);
    }

    #[test]
    fn scrolling_past_the_end_moves_only_as_far_as_it_goes() {
        let mut s = SyncedScrollOffset::default();
        let n = node(150.0);
        let mut rest = down(80.0);
        assert!(s.scroll_by(&n, &mut rest));
        assert_eq!(s.current(), down(50.0));
        assert_eq!(rest, down(30.0), "for the scroller around it");
        assert!(!s.scroll_by(&n, &mut down(10.0)));
        assert_eq!(s.take_for_main_frame(), Some(down(50.0)));
    }

    #[test]
    fn reset_drops_everything_scrolled_here() {
        let mut s = SyncedScrollOffset::default();
        let n = node(1000.0);
        s.scroll_by(&n, &mut down(10.0));
        s.take_for_main_frame();
        s.scroll_by(&n, &mut down(10.0));
        s.take_for_input();
        s.scroll_by(&n, &mut down(10.0));
        s.reset(down(5.0));
        assert_eq!(s.current(), down(5.0));
        assert!(!s.needs_main_frame());
    }
}
