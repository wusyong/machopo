//! How far an input scrolls: wheel lines and keys turned into a delta in
//! CSS px (Blink's `ScrollGranularity`, which `ScrollableArea` resolves to
//! pixels). Shared by the main thread and the compositor.

use gfx::PointF;
use protocol::input::ScrollDelta;

use crate::ScrollNode;

/// How far a wheel line, or an arrow key, scrolls (Chromium's
/// `kPixelsPerLineStep`).
const PIXELS_PER_LINE: f32 = 40.0;

/// How much of the port a page scrolls (Chromium's
/// `kMinFractionToStepWhenPaging`).
const PAGE_FRACTION: f32 = 0.875;

/// A wheel's delta in CSS px: a touchpad's already is; a wheel's lines
/// are `PIXELS_PER_LINE` each.
pub fn wheel_scroll_delta(delta: ScrollDelta) -> PointF {
    match delta {
        ScrollDelta::Pixels { x, y } => PointF::new(x, y),
        ScrollDelta::Lines { x, y } => PointF::new(x * PIXELS_PER_LINE, y * PIXELS_PER_LINE),
    }
}

/// A key that scrolls, when nothing else takes it (Blink's
/// `KeyboardEventManager::DefaultKeyboardEventHandler`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    PageUp,
    PageDown,
    Space,
    Home,
    End,
}

impl Key {
    /// The scrolling key of a DOM `key`, if it is one.
    pub fn from_dom_key(key: &str) -> Option<Self> {
        Some(match key {
            "ArrowUp" => Self::ArrowUp,
            "ArrowDown" => Self::ArrowDown,
            "ArrowLeft" => Self::ArrowLeft,
            "ArrowRight" => Self::ArrowRight,
            "PageUp" => Self::PageUp,
            "PageDown" => Self::PageDown,
            " " => Self::Space,
            "Home" => Self::Home,
            "End" => Self::End,
            _ => return None,
        })
    }
}

/// How far `key` scrolls `node`: a line for the arrows, a page for Page
/// Up / Down and the space bar (up with shift), the whole content for Home
/// and End. It doesn't depend on where `node` is: `ScrollNode::scroll_by`
/// stops it at the ends.
pub fn key_scroll_delta(key: Key, shift: bool, node: &ScrollNode) -> PointF {
    let page = node.container_bounds.height * PAGE_FRACTION;
    let document = node.bounds.height;
    match key {
        Key::ArrowUp => PointF::new(0.0, -PIXELS_PER_LINE),
        Key::ArrowDown => PointF::new(0.0, PIXELS_PER_LINE),
        Key::ArrowLeft => PointF::new(-PIXELS_PER_LINE, 0.0),
        Key::ArrowRight => PointF::new(PIXELS_PER_LINE, 0.0),
        Key::PageUp => PointF::new(0.0, -page),
        Key::PageDown => PointF::new(0.0, page),
        Key::Space => PointF::new(0.0, if shift { -page } else { page }),
        Key::Home => PointF::new(0.0, -document),
        Key::End => PointF::new(0.0, document),
    }
}

#[cfg(test)]
mod tests {
    use gfx::SizeF;

    use super::*;

    /// A 200x100 viewport over a 1000px-high page.
    const VIEWPORT: ScrollNode = ScrollNode {
        container_bounds: SizeF::new(200.0, 100.0),
        bounds: SizeF::new(200.0, 1000.0),
        user_scrollable_horizontal: true,
        user_scrollable_vertical: true,
    };

    #[test]
    fn wheel_lines_are_forty_pixels() {
        assert_eq!(
            wheel_scroll_delta(ScrollDelta::Lines { x: 1.0, y: -3.0 }),
            PointF::new(40.0, -120.0)
        );
        assert_eq!(
            wheel_scroll_delta(ScrollDelta::Pixels { x: 1.5, y: -3.0 }),
            PointF::new(1.5, -3.0)
        );
    }

    #[test]
    fn each_key_scrolls_its_granularity() {
        let delta = |key, shift| key_scroll_delta(key, shift, &VIEWPORT);
        assert_eq!(delta(Key::ArrowUp, false), PointF::new(0.0, -40.0));
        assert_eq!(delta(Key::ArrowDown, false), PointF::new(0.0, 40.0));
        assert_eq!(delta(Key::ArrowLeft, false), PointF::new(-40.0, 0.0));
        assert_eq!(delta(Key::ArrowRight, false), PointF::new(40.0, 0.0));
        assert_eq!(delta(Key::PageUp, false), PointF::new(0.0, -87.5));
        assert_eq!(delta(Key::PageDown, false), PointF::new(0.0, 87.5));
        assert_eq!(delta(Key::Space, false), PointF::new(0.0, 87.5));
        assert_eq!(delta(Key::Space, true), PointF::new(0.0, -87.5));
        assert_eq!(delta(Key::Home, false), PointF::new(0.0, -1000.0));
        assert_eq!(delta(Key::End, false), PointF::new(0.0, 1000.0));
    }

    #[test]
    fn scrolling_keys_by_their_dom_key() {
        assert_eq!(Key::from_dom_key(" "), Some(Key::Space));
        assert_eq!(Key::from_dom_key("PageDown"), Some(Key::PageDown));
        assert_eq!(Key::from_dom_key("Enter"), None);
        assert_eq!(Key::from_dom_key("a"), None);
    }

    #[test]
    fn home_and_end_reach_the_ends_from_anywhere() {
        for start in [0.0, 333.3, 900.0] {
            let mut offset = PointF::new(0.0, start);
            let mut delta = key_scroll_delta(Key::End, false, &VIEWPORT);
            VIEWPORT.scroll_by(&mut offset, &mut delta);
            assert_eq!(offset.y, 900.0);
            let mut delta = key_scroll_delta(Key::Home, false, &VIEWPORT);
            VIEWPORT.scroll_by(&mut offset, &mut delta);
            assert_eq!(offset.y, 0.0);
        }
    }
}
