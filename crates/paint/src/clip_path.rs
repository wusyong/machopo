//! `clip`'s rect as its clip node takes it (`clip-path`'s paths are
//! layout's, `layout::ClipPaths`).

use computed_style::{ComputedStyle, Position};
use gfx::{PointF, RectF, SizeF};

/// `clip`'s rect for the box at `offset`, if it has one: an absolutely
/// positioned box's, its `auto` edges the border box's (Blink's
/// `LayoutBox::ClipRect`).
pub(crate) fn css_clip(style: &ComputedStyle, offset: PointF, size: SizeF) -> Option<RectF> {
    if !matches!(style.position, Position::Absolute | Position::Fixed) {
        return None;
    }
    let [top, right, bottom, left] = style.rare.clip?;
    let (left, top) = (left.unwrap_or(0.0), top.unwrap_or(0.0));
    let (right, bottom) = (right.unwrap_or(size.width), bottom.unwrap_or(size.height));
    Some(RectF::new(
        offset.x + left,
        offset.y + top,
        (right - left).max(0.0),
        (bottom - top).max(0.0),
    ))
}
