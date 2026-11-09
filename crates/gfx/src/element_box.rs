//! What script reads of a frame's layout (CSSOM View): a frame's viewport
//! and an element's box, the same for either DOM.

use crate::{PointF, RectF, SizeF};

/// A frame's viewport, as script sees it (`innerWidth`, `scrollX`,
/// `devicePixelRatio`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// CSS px: the window's, or a child frame's iframe's content box.
    pub size: SizeF,
    /// Device pixels per CSS px.
    pub device_scale_factor: f32,
    /// How far the document is scrolled, CSS px.
    pub scroll: PointF,
    /// The document's size, what can be scrolled to (Blink's
    /// `ContentsSize`).
    pub content_size: SizeF,
}

/// An element's box as `offset*`, `client*` and `scroll*` read it: in the
/// coordinates of its document's frame, before transforms and scrolling
/// (Blink's chain of `PhysicalLocation`s).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElementBox {
    /// The border box; for an inline element, the rect around its boxes
    /// on the lines.
    pub border_box: RectF,
    /// Where it starts: the border box's origin; for an inline element,
    /// its first box on a line (Blink's `FirstLineBoxTopLeft`).
    pub start: PointF,
    /// A block-level or atomic box's own geometry; `None` for an inline
    /// element, which has no client or scroll area.
    pub block: Option<BlockBox>,
}

/// What a box that isn't an inline element has besides its border box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlockBox {
    /// Its border widths: top, right, bottom, left.
    pub border: [f32; 4],
    /// Its padding, used values, in the same order: what the resolved
    /// value of `padding-*` is, and what `width`'s content box takes off
    /// the border box.
    pub padding: [f32; 4],
    /// Its margins, used values, in the same order (the resolved value of
    /// `margin-*`): `auto` resolved to what layout gave it.
    pub margin: [f32; 4],
    /// Its insets, used values, in the same order (the resolved value of
    /// `top` and its kin for a positioned box): where an absolutely
    /// positioned box sits in its containing block's padding box, or how
    /// far a relatively positioned one moved. Zero for a static box, whose
    /// resolved value is its computed `auto` instead.
    pub inset: [f32; 4],
    /// Its scrollable overflow, from its padding box's origin, at least the
    /// padding box (`scrollWidth`, `scrollHeight`).
    pub scroll_size: SizeF,
    /// How far it is scrolled, if it is a scroll container.
    pub scroll: Option<PointF>,
}

/// Where scrolling an element into view puts it on an axis
/// (CSSOM View's `ScrollLogicalPosition`, Blink's `ScrollAlignment`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAlignment {
    /// Its start edge at the scrolling box's.
    Start,
    /// Its center at the scrolling box's.
    Center,
    /// Its end edge at the scrolling box's.
    End,
    /// Not scrolled if it is all in view; else the edge nearer to it.
    Nearest,
    /// Not scrolled if it is all in view; the edge nearer to it if part
    /// of it is; else its center at the scrolling box's (Blink's
    /// `ScrollAlignment::CenterIfNeeded`: `focus()`'s).
    CenterIfNeeded,
}

/// What `scrollIntoView` asks of each scrolling box (Blink's
/// `ScrollIntoViewParams`): machopo's text is horizontal, so the block axis
/// is vertical and the inline axis horizontal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollIntoViewParams {
    pub block: ScrollAlignment,
    pub inline: ScrollAlignment,
    /// Only the nearest scrolling box (`container: "nearest"`), not every
    /// one around the element.
    pub nearest_only: bool,
}
