//! `BrowserViewLayout` (Chromium's, in `chrome/browser/ui/views/frame`):
//! where the tab strip, the toolbar and each web view go in a browser
//! window. Nothing
//! else in the window assumes where they are: the view draws, hit tests and
//! sizes the renderers from the bounds this gives.

use base::numerics::{clamp_round, saturated_cast};
use gfx::{Rect, Size};

use crate::{tab_strip, toolbar};

/// Where everything in a window is, physical px of the window. Bounds are
/// whole pixels: a page starting between two would be blurred, and what is
/// drawn, the size a renderer is told and where clicks land all agree.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Layout {
    pub tab_strip: Rect,
    pub toolbar: Rect,
    /// One per web view, in their order: a later one is over an earlier
    /// one. An empty one isn't shown.
    pub web_views: Vec<Rect>,
}

pub trait BrowserViewLayout {
    /// For a window `size` physical px at `scale`, showing `web_views`
    /// web views.
    fn layout(&self, size: Size, scale: f32, web_views: usize) -> Layout;
}

/// The tab strip across the top, the toolbar under it, and the first web
/// view under that, to the window's bottom. The others aren't shown.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultLayout;

impl BrowserViewLayout for DefaultLayout {
    fn layout(&self, size: Size, scale: f32, web_views: usize) -> Layout {
        // Each part no taller than what is left of the window: the parts
        // add up to its height at most.
        let strip = tab_strip_height(scale).min(size.height);
        let toolbar = toolbar_height(scale).min(size.height.saturating_sub(strip));
        let top = strip.saturating_add(toolbar);
        let mut views = vec![Rect::default(); web_views];
        if let Some(first) = views.first_mut() {
            *first = Rect::new(
                0,
                saturated_cast(top),
                size.width,
                size.height.saturating_sub(top),
            );
        }
        Layout {
            tab_strip: Rect::new(0, 0, size.width, strip),
            toolbar: Rect::new(0, saturated_cast(strip), size.width, toolbar),
            web_views: views,
        }
    }
}

/// The tab strip's height in physical px at `scale`, rounded to a whole
/// pixel.
pub fn tab_strip_height(scale: f32) -> u32 {
    clamp_round(tab_strip::HEIGHT * scale)
}

/// The toolbar's height in physical px at `scale`, rounded to a whole
/// pixel.
pub fn toolbar_height(scale: f32) -> u32 {
    clamp_round(toolbar::HEIGHT * scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_web_view_is_under_the_toolbar() {
        let layout = DefaultLayout.layout(Size::new(600, 450), 1.5, 2);
        assert_eq!(layout.tab_strip, Rect::new(0, 0, 600, 51));
        assert_eq!(layout.toolbar, Rect::new(0, 51, 600, 60));
        assert_eq!(
            layout.web_views,
            [Rect::new(0, 111, 600, 339), Rect::default()]
        );
        // At 1.33x, 45.22px and 53.2px: the page starts on a whole pixel.
        let layout = DefaultLayout.layout(Size::new(400, 300), 1.33, 1);
        assert_eq!(layout.toolbar, Rect::new(0, 45, 400, 53));
        assert_eq!(layout.web_views, [Rect::new(0, 98, 400, 202)]);
        // Shorter than the tab strip and the toolbar.
        let layout = DefaultLayout.layout(Size::new(400, 50), 1.0, 1);
        assert_eq!(layout.tab_strip, Rect::new(0, 0, 400, 34));
        assert_eq!(layout.toolbar, Rect::new(0, 34, 400, 16));
        assert!(layout.web_views[0].is_empty());
    }
}
