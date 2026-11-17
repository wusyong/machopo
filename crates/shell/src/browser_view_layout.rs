//! How many physical px the horizontal layout's parts are at a scale: the
//! tab strip and the toolbar, each rounded to a whole pixel (Chromium's
//! `BrowserViewLayout` has them in DIPs, its layers snapped to pixels).
//! Where everything goes in a window is the UI's description (`chrome`):
//! the web views are where it lays its web view slots out.

use base::numerics::clamp_round;

use crate::{tab_strip, toolbar};

/// The tab strip's height in physical px at `scale`, rounded to a whole
/// pixel: the toolbar, and so the page, start on one.
pub fn tab_strip_height(scale: f32) -> u32 {
    clamp_round(tab_strip::HEIGHT * scale)
}

/// The toolbar's height in physical px at `scale`, rounded to a whole
/// pixel.
pub fn toolbar_height(scale: f32) -> u32 {
    clamp_round(toolbar::HEIGHT * scale)
}
