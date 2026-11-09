//! Geometry and color primitives shared by every stage of the pipeline.
//!
//! Mirrors the small subset of `//ui/gfx` that the MVP needs. Everything here
//! is plain data (`Copy` + serde) so it can cross thread and, later, process
//! boundaries unchanged, except [`FontData`], which shares font bytes in
//! memory.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

mod element_box;
mod node_id;
mod path_hit;
mod path_measure;
mod path_shapes;
pub mod svg;

use std::fmt;
use std::sync::Arc;

use base::AtomicSequenceNumber;
use base::numerics::{clamp_ceil, f32_from_f64, saturated_cast};
use serde::{Deserialize, Serialize};

pub use crate::element_box::{
    BlockBox, ElementBox, ScrollAlignment, ScrollIntoViewParams, Viewport,
};
pub use crate::node_id::{NodeId, NodeIdHashMap, NodeIdHashSet};
pub use crate::path_hit::{LineCap, LineJoin, StrokeStyle};
pub use crate::svg::{SvgGeometry, SvgTextChar};

/// Integer size in physical pixels (`gfx::Size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// Its pixels, width × height, or `None` if that overflows a `usize`
    /// (`gfx::Size::GetCheckedArea`).
    pub fn checked_area(self) -> Option<usize> {
        usize::try_from(self.width)
            .ok()?
            .checked_mul(usize::try_from(self.height).ok()?)
    }

    /// The bytes of its RGBA8 pixels, rows tightly packed (Skia's
    /// `SkImageInfo::computeByteSize`), or `None` if that overflows a
    /// `usize`.
    pub fn checked_byte_size(self) -> Option<usize> {
        self.checked_area()?.checked_mul(4)
    }

    pub fn to_f32(self) -> SizeF {
        SizeF::new(self.width as f32, self.height as f32)
    }

    /// `gfx::ScaleToCeiledSize`.
    pub fn scale_to_ceiled(self, scale: f32) -> Size {
        Size::new(
            clamp_ceil(self.width as f32 * scale),
            clamp_ceil(self.height as f32 * scale),
        )
    }
}

/// Floating point size (`gfx::SizeF`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct SizeF {
    pub width: f32,
    pub height: f32,
}

impl SizeF {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// Floating point point (`gfx::PointF`).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct PointF {
    pub x: f32,
    pub y: f32,
}

impl PointF {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// Integer rectangle in physical pixels (`gfx::Rect`): where something sits
/// whole pixels apart, so its edges never fall between two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    pub const fn is_empty(&self) -> bool {
        self.size().is_empty()
    }

    pub fn to_f32(&self) -> RectF {
        RectF::new(
            self.x as f32,
            self.y as f32,
            self.width as f32,
            self.height as f32,
        )
    }
}

/// Floating point rectangle (`gfx::RectF`). Origin is top-left, y grows down.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RectF {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl RectF {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn from_size(size: SizeF) -> Self {
        Self::new(0.0, 0.0, size.width, size.height)
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    pub fn size(&self) -> SizeF {
        SizeF::new(self.width, self.height)
    }

    pub fn contains(&self, p: PointF) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// Returns the intersection, or an empty rect at the origin if disjoint.
    pub fn intersect(&self, other: &RectF) -> RectF {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r <= x || b <= y {
            RectF::default()
        } else {
            RectF::new(x, y, r - x, b - y)
        }
    }

    pub fn inset(&self, d: f32) -> RectF {
        RectF::new(
            self.x + d,
            self.y + d,
            (self.width - 2.0 * d).max(0.0),
            (self.height - 2.0 * d).max(0.0),
        )
    }

    pub fn scale(&self, s: f32) -> RectF {
        RectF::new(self.x * s, self.y * s, self.width * s, self.height * s)
    }

    pub fn offset(&self, dx: f32, dy: f32) -> RectF {
        RectF::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    /// The smallest rect around both; an empty one adds nothing.
    pub fn union(&self, other: &RectF) -> RectF {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let (x, y) = (self.x.min(other.x), self.y.min(other.y));
        let (r, b) = (
            self.right().max(other.right()),
            self.bottom().max(other.bottom()),
        );
        RectF::new(x, y, r - x, b - y)
    }

    /// The smallest rect of whole units around this one.
    pub fn enclosing(&self) -> RectF {
        let (x, y) = (self.x.floor(), self.y.floor());
        RectF::new(x, y, self.right().ceil() - x, self.bottom().ceil() - y)
    }
}

/// A rect with an elliptical radius per corner (`gfx::RRectF`, Blink's
/// `FloatRoundedRect`): top left, top right, bottom right, bottom left.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct RRectF {
    pub rect: RectF,
    pub radii: [SizeF; 4],
}

impl RRectF {
    /// `rect` with these radii, made to fit as css-backgrounds 3 says
    /// ("corner overlap"): if two corners' radii on a side add up to more
    /// than the side, every radius is scaled down by the same factor.
    pub fn new(rect: RectF, radii: [SizeF; 4]) -> Self {
        let radii = radii.map(|r| {
            // A corner with either radius 0 is square.
            if r.width <= 0.0 || r.height <= 0.0 {
                SizeF::new(0.0, 0.0)
            } else {
                r
            }
        });
        let [tl, tr, br, bl] = radii;
        let mut f = 1.0f32;
        for (length, sum) in [
            (rect.width, tl.width + tr.width),
            (rect.width, bl.width + br.width),
            (rect.height, tl.height + bl.height),
            (rect.height, tr.height + br.height),
        ] {
            if sum > 0.0 {
                f = f.min(length.max(0.0) / sum);
            }
        }
        let radii = if f < 1.0 {
            radii.map(|r| SizeF::new(r.width * f, r.height * f))
        } else {
            radii
        };
        Self { rect, radii }
    }

    /// The same radius at every corner.
    pub fn uniform(rect: RectF, radius: f32) -> Self {
        Self::new(rect, [SizeF::new(radius, radius); 4])
    }

    /// A plain rect.
    pub fn from_rect(rect: RectF) -> Self {
        Self {
            rect,
            radii: [SizeF::new(0.0, 0.0); 4],
        }
    }

    /// Whether no corner is rounded.
    pub fn is_rect(&self) -> bool {
        self.radii.iter().all(|r| r.width <= 0.0 || r.height <= 0.0)
    }

    pub fn offset(&self, dx: f32, dy: f32) -> RRectF {
        Self {
            rect: self.rect.offset(dx, dy),
            radii: self.radii,
        }
    }

    /// The rect inside by these widths (top, right, bottom, left), each
    /// corner's radii less the widths beside it, no less than 0: the
    /// padding edge's curve inside a border (css-backgrounds 3, "corner
    /// shaping").
    pub fn inset(&self, top: f32, right: f32, bottom: f32, left: f32) -> RRectF {
        let r = self.rect;
        let rect = RectF::new(
            r.x + left,
            r.y + top,
            (r.width - left - right).max(0.0),
            (r.height - top - bottom).max(0.0),
        );
        let less = |radius: SizeF, x: f32, y: f32| {
            SizeF::new((radius.width - x).max(0.0), (radius.height - y).max(0.0))
        };
        let [tl, tr, br, bl] = self.radii;
        Self::new(
            rect,
            [
                less(tl, left, top),
                less(tr, right, top),
                less(br, right, bottom),
                less(bl, left, bottom),
            ],
        )
    }

    /// The rect grown by `d` on every side (shrunk if negative), its
    /// corners with it, as css-backgrounds 3 says for a shadow's spread:
    /// shrinking takes `d` off each radius, no less than 0; growing adds
    /// `d` to each radius at least `d`, and less to a smaller one, nothing
    /// to a square corner (`r + d * (1 + (r / d - 1)^3)`), so a small
    /// radius doesn't round a big shadow (Blink's
    /// `FloatRoundedRect::OutsetForMarginOrShadow`).
    pub fn spread(&self, d: f32) -> RRectF {
        let r = self.rect;
        let rect = RectF::new(
            r.x - d,
            r.y - d,
            (r.width + 2.0 * d).max(0.0),
            (r.height + 2.0 * d).max(0.0),
        );
        let grow = |radius: f32| {
            if d < 0.0 {
                (radius + d).max(0.0)
            } else if radius <= 0.0 {
                0.0
            } else if radius >= d {
                radius + d
            } else {
                let t = radius / d - 1.0;
                radius + d * (1.0 + t * t * t)
            }
        };
        Self::new(
            rect,
            self.radii
                .map(|radius| SizeF::new(grow(radius.width), grow(radius.height))),
        )
    }

    /// Whether `p` is inside: in the rect, and inside each corner's curve.
    pub fn contains(&self, p: PointF) -> bool {
        let r = self.rect;
        if !r.contains(p) {
            return false;
        }
        let [tl, tr, br, bl] = self.radii;
        // Each corner's ellipse center, its radii, and whether `p` is in
        // that corner's box.
        let corners = [
            (
                r.x + tl.width,
                r.y + tl.height,
                tl,
                p.x < r.x + tl.width && p.y < r.y + tl.height,
            ),
            (
                r.right() - tr.width,
                r.y + tr.height,
                tr,
                p.x > r.right() - tr.width && p.y < r.y + tr.height,
            ),
            (
                r.right() - br.width,
                r.bottom() - br.height,
                br,
                p.x > r.right() - br.width && p.y > r.bottom() - br.height,
            ),
            (
                r.x + bl.width,
                r.bottom() - bl.height,
                bl,
                p.x < r.x + bl.width && p.y > r.bottom() - bl.height,
            ),
        ];
        corners.iter().all(|&(cx, cy, radius, in_box)| {
            if !in_box || radius.width <= 0.0 || radius.height <= 0.0 {
                return true;
            }
            let (dx, dy) = ((p.x - cx) / radius.width, (p.y - cy) / radius.height);
            dx * dx + dy * dy <= 1.0
        })
    }
}

/// Non-premultiplied sRGB color, 8 bits per channel (`SkColor`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    pub const BLACK: Color = Color::rgb(0, 0, 0);
    pub const WHITE: Color = Color::rgb(255, 255, 255);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self::rgba(r, g, b, 255)
    }

    /// `0xRRGGBB`, opaque.
    pub const fn from_hex(rgb: u32) -> Self {
        // The top byte isn't a channel.
        let [_, r, g, b] = rgb.to_be_bytes();
        Self::rgb(r, g, b)
    }

    pub const fn with_alpha(self, a: u8) -> Self {
        Self::rgba(self.r, self.g, self.b, a)
    }

    pub const fn is_opaque(self) -> bool {
        self.a == 255
    }

    /// Premultiplied RGBA8, the layout every bitmap in the pipeline uses.
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        reason = "c, a <= 255: c * a + 127 <= 65152, and that over 255 is a byte"
    )]
    pub fn premultiplied(self) -> [u8; 4] {
        let a = self.a as u32;
        let mul = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        [mul(self.r), mul(self.g), mul(self.b), self.a]
    }

    /// Premultiplied, normalised to 0..=1.
    pub fn premultiplied_f32(self) -> [f32; 4] {
        let a = self.a as f32 / 255.0;
        [
            self.r as f32 / 255.0 * a,
            self.g as f32 / 255.0 * a,
            self.b as f32 / 255.0 * a,
            a,
        ]
    }
}

/// The CSS system colors that follow the platform: what Chromium's
/// `ui::ColorProvider` gives Blink (`kColorCssSystem*`, read by
/// `LayoutTheme::SystemColorFromColorProvider`), which the browser makes
/// from the system's colors (on Windows, `GetSysColor`'s in the light
/// mode) and hands every renderer. The rest of the system colors are
/// fixed (`LayoutTheme::DefaultSystemColor`). The default is the table a
/// web test gets: Blink's `DefaultSystemColor`, which has no provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemColors {
    /// `ButtonFace`, and `ButtonHighlight`, `ButtonShadow`, `ThreeDFace`.
    pub button_face: Color,
    /// `ButtonText`.
    pub button_text: Color,
    /// `ButtonBorder`, and the deprecated borders and 3D shadows.
    pub button_border: Color,
    /// `Canvas`, and the deprecated window and menu backgrounds.
    pub canvas: Color,
    /// `CanvasText`, and the deprecated window and menu text.
    pub canvas_text: Color,
    pub field: Color,
    pub field_text: Color,
    pub gray_text: Color,
    /// `Highlight`: the selection's background while the page has the
    /// focus, and `HighlightText` its text.
    pub highlight: Color,
    pub highlight_text: Color,
    /// The selection's background and text while the page doesn't have
    /// the focus (Blink's `LayoutTheme::InactiveSelectionBackgroundColor`
    /// and `InactiveSelectionForegroundColor`): no CSS keyword names them.
    pub inactive_highlight: Color,
    pub inactive_highlight_text: Color,
    pub link_text: Color,
    pub visited_text: Color,
    pub active_text: Color,
    /// `SelectedItem`: a selected item's background, as a list box's.
    pub selected_item: Color,
    pub selected_item_text: Color,
}

impl SystemColors {
    /// Blink's `DefaultSystemColor`: what a web test (no provider), and
    /// where there is no document (a canvas outside one), gets.
    pub const DEFAULT: Self = Self {
        button_face: Color::from_hex(0xefefef),
        button_text: Color::BLACK,
        button_border: Color::from_hex(0x767676),
        canvas: Color::WHITE,
        canvas_text: Color::BLACK,
        field: Color::WHITE,
        field_text: Color::BLACK,
        gray_text: Color::from_hex(0x808080),
        // The selection's colors, as Chrome 152 has them on Linux: a
        // translucent blue (`rgb(51, 103, 209)` over white), and gray
        // while the page doesn't have the focus.
        highlight: Color::rgba(0, 65, 198, 204),
        highlight_text: Color::WHITE,
        inactive_highlight: Color::rgb(200, 200, 200),
        inactive_highlight_text: Color::rgb(50, 50, 50),
        link_text: Color::from_hex(0x0000ee),
        visited_text: Color::from_hex(0x551a8b),
        active_text: Color::from_hex(0xff0000),
        // A list box's selection, as Chrome's (Blink's
        // `LayoutTheme::PlatformActiveListBoxSelectionBackgroundColor`).
        selected_item: Color::from_hex(0x1967d2),
        selected_item_text: Color::WHITE,
    };
}

impl Default for SystemColors {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A 2D affine transform (`gfx::Transform`, 2D only): the matrix
/// `[a c e; b d f; 0 0 1]`, stored as CSS's `matrix(a, b, c, d, e, f)`.
/// A point maps to `(a x + c y + e, b x + d y + f)`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Transform(pub [f32; 6]);

impl Transform {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    pub const fn translate(x: f32, y: f32) -> Self {
        Self([1.0, 0.0, 0.0, 1.0, x, y])
    }

    pub const fn scale(x: f32, y: f32) -> Self {
        Self([x, 0.0, 0.0, y, 0.0, 0.0])
    }

    /// A rotation by `degrees`, clockwise on screen (y points down), exact
    /// at quarter turns (Blink's `gfx::Transform::Rotate`, whose
    /// `SinCosDegrees` keeps `rotate(90deg)` at `matrix(0, 1, -1, 0, 0, 0)`
    /// rather than 6e-17 off).
    pub fn rotate(degrees: f32) -> Self {
        let (sin, cos) = sin_cos_degrees(degrees.into());
        let (sin, cos) = (f32_from_f64(sin), f32_from_f64(cos));
        Self([cos, sin, -sin, cos, 0.0, 0.0])
    }

    /// A skew by `x` degrees along the x axis and `y` along the y axis
    /// (css-transforms' `skew(x, y)`: `matrix(1, tan(y), tan(x), 1, 0, 0)`).
    pub fn skew(x: f32, y: f32) -> Self {
        let tan = |degrees: f32| f32_from_f64(f64::from(degrees).to_radians().tan());
        Self([1.0, tan(y), tan(x), 1.0, 0.0, 0.0])
    }

    pub fn is_identity(&self) -> bool {
        *self == Self::IDENTITY
    }

    /// `from` at `progress` (0 to 1) of the way to `to`, each decomposed
    /// and the parts interpolated (css-transforms 1, "Interpolation of
    /// Matrices"; Blink's `gfx::Transform::Blend`): `None` if either
    /// can't be inverted, where the caller steps from one to the other.
    pub fn blend(from: &Self, to: &Self, progress: f64) -> Option<Self> {
        let (from, to) = (Decomposed::of(from.0)?, Decomposed::of(to.0)?);
        Some(Self(from.interpolate(&to, progress).compose()))
    }

    /// The offset, if this only moves things (no scale, rotation or skew).
    pub fn as_translation(&self) -> Option<PointF> {
        let [a, b, c, d, e, f] = self.0;
        (a == 1.0 && b == 0.0 && c == 0.0 && d == 1.0).then(|| PointF::new(e, f))
    }

    /// `self` after `inner`: maps a point through `inner`, then `self`.
    /// What overflows an f32 (a huge scale about an origin away from 0:
    /// `scale(calc(infinity))`) is worked out in f64 and kept to the
    /// biggest f32 instead (Blink's matrices are doubles, made floats
    /// clamped): an infinity would be NaN times a 0 in the next product.
    pub fn then(&self, inner: &Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [a2, b2, c2, d2, e2, f2] = inner.0;
        let product = Self([
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ]);
        if product.0.iter().all(|v| v.is_finite())
            || self.0.iter().chain(&inner.0).any(|v| !v.is_finite())
        {
            return product;
        }
        let [a, b, c, d, e, f] = self.0.map(f64::from);
        let [a2, b2, c2, d2, e2, f2] = inner.0.map(f64::from);
        let max = f64::from(f32::MAX);
        Self(
            [
                a * a2 + c * b2,
                b * a2 + d * b2,
                a * c2 + c * d2,
                b * c2 + d * d2,
                a * e2 + c * f2 + e,
                b * e2 + d * f2 + f,
            ]
            .map(|v| f32_from_f64(v.clamp(-max, max))),
        )
    }

    /// The transform that undoes this one; `None` if it flattens the
    /// plane (`scale(0)`), which nothing can undo.
    pub fn inverse(&self) -> Option<Self> {
        let [a, b, c, d, e, f] = self.0;
        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        let (ia, ib, ic, id) = (d / det, -b / det, -c / det, a / det);
        Some(Self([
            ia,
            ib,
            ic,
            id,
            -(ia * e + ic * f),
            -(ib * e + id * f),
        ]))
    }

    pub fn map_point(&self, p: PointF) -> PointF {
        let [a, b, c, d, e, f] = self.0;
        PointF::new(a * p.x + c * p.y + e, b * p.x + d * p.y + f)
    }

    /// The bounding box of `rect` mapped.
    pub fn map_rect(&self, rect: RectF) -> RectF {
        let corners = [
            self.map_point(PointF::new(rect.x, rect.y)),
            self.map_point(PointF::new(rect.right(), rect.y)),
            self.map_point(PointF::new(rect.x, rect.bottom())),
            self.map_point(PointF::new(rect.right(), rect.bottom())),
        ];
        let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
        let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in corners {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        RectF::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// A 2D matrix as css-transforms 1 decomposes one: a translation, a
/// rotation (degrees), a 2×2 matrix left after them (a skew), and a
/// scale per axis. `matrix(a, b, c, d, e, f)` maps `(x, y)` to
/// `(a·x + c·y + e, b·x + d·y + f)`.
#[derive(Debug, Clone, Copy)]
struct Decomposed {
    translate: [f64; 2],
    scale: [f64; 2],
    angle: f64,
    /// `m11`, `m12`, `m21`, `m22`.
    skew: [f64; 4],
}

impl Decomposed {
    /// css-transforms 1, "Decomposing a 2D matrix": `None` for one that
    /// can't be inverted.
    fn of(m: [f32; 6]) -> Option<Self> {
        let [a, b, c, d, e, f] = m.map(f64::from);
        if a * d - b * c == 0.0 {
            return None;
        }
        let (mut row0, mut row1) = ([a, b], [c, d]);
        let mut scale = [row0[0].hypot(row0[1]), row1[0].hypot(row1[1])];
        // A flip is a negative scale along one axis.
        if a * d - b * c < 0.0 {
            if a < d {
                scale[0] = -scale[0];
            } else {
                scale[1] = -scale[1];
            }
        }
        if scale[0] != 0.0 {
            row0 = row0.map(|v| v / scale[0]);
        }
        if scale[1] != 0.0 {
            row1 = row1.map(|v| v / scale[1]);
        }
        let angle = row0[1].atan2(row0[0]);
        if angle != 0.0 {
            // Rotate the rows back by the angle.
            let (sn, cs) = (-row0[1], row0[0]);
            let [m11, m12] = row0;
            let [m21, m22] = row1;
            row0 = [cs * m11 + sn * m21, cs * m12 + sn * m22];
            row1 = [-sn * m11 + cs * m21, -sn * m12 + cs * m22];
        }
        Some(Self {
            translate: [e, f],
            scale,
            angle: angle.to_degrees(),
            skew: [row0[0], row0[1], row1[0], row1[1]],
        })
    }

    /// css-transforms 1, "Interpolation of decomposed 2D matrix values":
    /// a flip on both axes is a rotation, and the rotation the short way
    /// round.
    fn interpolate(&self, other: &Self, progress: f64) -> Self {
        let (mut a, mut b) = (*self, *other);
        if (a.scale[0] < 0.0 && b.scale[1] < 0.0) || (a.scale[1] < 0.0 && b.scale[0] < 0.0) {
            a.scale = a.scale.map(|s| -s);
            a.angle += if a.angle < 0.0 { 180.0 } else { -180.0 };
        }
        if a.angle == 0.0 {
            a.angle = 360.0;
        }
        if b.angle == 0.0 {
            b.angle = 360.0;
        }
        if (a.angle - b.angle).abs() > 180.0 {
            if a.angle > b.angle {
                a.angle -= 360.0;
            } else {
                b.angle -= 360.0;
            }
        }
        let lerp = |a: f64, b: f64, t: f64| a + (b - a) * t;
        let mix2 = |x: [f64; 2], y: [f64; 2]| [0, 1].map(|i| lerp(x[i], y[i], progress));
        Self {
            translate: mix2(a.translate, b.translate),
            scale: mix2(a.scale, b.scale),
            angle: lerp(a.angle, b.angle, progress),
            skew: [0, 1, 2, 3].map(|i| lerp(a.skew[i], b.skew[i], progress)),
        }
    }

    /// css-transforms 1, "Recomposing to a 2D matrix".
    fn compose(&self) -> [f32; 6] {
        // A quarter turn's sine and cosine exactly: 180° would leave a
        // sine of 1e-16, written as such.
        let snap = |v: f64| if v.abs() < 1e-12 { 0.0 } else { v };
        let (sn, cs) = self.angle.to_radians().sin_cos();
        let (sn, cs) = (snap(sn), snap(cs));
        let [m11, m12, m21, m22] = self.skew;
        // By rows, the scale times the rotation times the skew: the
        // decomposition undone.
        let row0 = [cs * m11 + sn * m21, cs * m12 + sn * m22];
        let row1 = [-sn * m11 + cs * m21, -sn * m12 + cs * m22];
        let row0 = row0.map(|v| v * self.scale[0]);
        let row1 = row1.map(|v| v * self.scale[1]);
        [
            row0[0],
            row0[1],
            row1[0],
            row1[1],
            self.translate[0],
            self.translate[1],
        ]
        .map(f32_from_f64)
    }
}

/// The sine and cosine of `degrees`, exact at quarter turns (Blink's
/// `SinCosDegrees`): `rotate(90deg)` is `matrix(0, 1, -1, 0, 0, 0)`.
pub fn sin_cos_degrees(degrees: f64) -> (f64, f64) {
    let quarter = degrees / 90.0;
    if quarter.fract() == 0.0 {
        return match saturated_cast::<u8, _>(quarter.rem_euclid(4.0)) {
            0 => (0.0, 1.0),
            1 => (1.0, 0.0),
            2 => (0.0, -1.0),
            _ => (-1.0, 0.0),
        };
    }
    degrees.to_radians().sin_cos()
}

/// Which points a path's fill covers where its subpaths cross or nest
/// (`SkPathFillType`; SVG's `fill-rule`, canvas's `CanvasFillRule`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FillRule {
    /// Those the path winds round a nonzero number of times.
    #[default]
    NonZero,
    /// Those inside an odd number of times.
    EvenOdd,
}

/// One step of a [`Path`]: an `SkPath` verb with its points.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathElement {
    /// Starts a subpath.
    MoveTo(PointF),
    LineTo(PointF),
    /// A quadratic Bézier curve: its control point, then its end.
    QuadTo(PointF, PointF),
    /// A cubic Bézier curve: its two control points, then its end.
    CubicTo(PointF, PointF, PointF),
    /// A conic: a quadratic curve pulled toward its control point by a
    /// weight (1 is the quadratic; less than 1, an arc of an ellipse), as
    /// Skia draws arcs, so canvas's `arc()` lands on Chrome's pixels.
    ConicTo(PointF, PointF, f32),
    /// A line back to the subpath's start, joined to it.
    Close,
}

/// Lines and curves, in subpaths each started by a `MoveTo`, and how to
/// fill them (`SkPath`, which `cc::DrawPathOp` holds): what SVG's shapes
/// and canvas's paths are drawn as. Arcs come as curves already.
///
/// A segment with no `MoveTo` before it (after a `Close`, or first)
/// starts where the last subpath did, or at the origin, as
/// `SkPathBuilder` injects a move there.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Path {
    elements: Vec<PathElement>,
    fill_rule: FillRule,
}

impl Path {
    pub fn new() -> Self {
        Self::default()
    }

    /// Closed polygons as one path, filled non-zero.
    pub fn polygons<P: AsRef<[PointF]>>(polygons: impl IntoIterator<Item = P>) -> Self {
        let mut path = Self::new();
        for polygon in polygons {
            let Some((first, rest)) = polygon.as_ref().split_first() else {
                continue;
            };
            path.move_to(*first);
            for &p in rest {
                path.line_to(p);
            }
            path.close();
        }
        path
    }

    pub fn with_fill_rule(mut self, fill_rule: FillRule) -> Self {
        self.fill_rule = fill_rule;
        self
    }

    pub fn move_to(&mut self, p: PointF) {
        self.elements.push(PathElement::MoveTo(p));
    }

    pub fn line_to(&mut self, p: PointF) {
        self.elements.push(PathElement::LineTo(p));
    }

    pub fn quad_to(&mut self, control: PointF, p: PointF) {
        self.elements.push(PathElement::QuadTo(control, p));
    }

    pub fn cubic_to(&mut self, control1: PointF, control2: PointF, p: PointF) {
        self.elements
            .push(PathElement::CubicTo(control1, control2, p));
    }

    pub fn conic_to(&mut self, control: PointF, p: PointF, weight: f32) {
        self.elements.push(PathElement::ConicTo(control, p, weight));
    }

    pub fn close(&mut self) {
        self.elements.push(PathElement::Close);
    }

    pub fn elements(&self) -> &[PathElement] {
        &self.elements
    }

    pub fn fill_rule(&self) -> FillRule {
        self.fill_rule
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// The bounding box of every point, control points and injected moves
    /// too (`SkPath::getBounds`): it holds the curves, if not tightly.
    /// `None` without points.
    pub fn bounds(&self) -> Option<RectF> {
        // A segment after a close starts at its subpath's start, a point
        // already counted; one before any move, at the origin.
        let first_segment = self
            .elements
            .iter()
            .find(|element| !matches!(element, PathElement::Close));
        let origin = match first_segment {
            None | Some(PathElement::MoveTo(_)) => None,
            Some(_) => Some(PointF::default()),
        };
        let mut points = origin
            .into_iter()
            .chain(self.elements.iter().flat_map(|element| match *element {
                PathElement::MoveTo(p) | PathElement::LineTo(p) => vec![p],
                PathElement::QuadTo(c, p) | PathElement::ConicTo(c, p, _) => vec![c, p],
                PathElement::CubicTo(c1, c2, p) => vec![c1, c2, p],
                PathElement::Close => vec![],
            }));
        let first = points.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in points {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        Some(RectF::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// The bounding box of the curves themselves (`SkPath::computeTight
    /// Bounds`): SVG's object bounding box. A move's point counts too, as
    /// Skia keeps it (a path of a move alone is at its point), unless
    /// another move right after it replaces it. `None` without points.
    pub fn tight_bounds(&self) -> Option<RectF> {
        use kurbo::{PathEl, Shape};
        let path = self.to_kurbo();
        let elements = path.elements();
        let next = elements.iter().skip(1).map(Some).chain([None]);
        let moves = elements
            .iter()
            .zip(next)
            .filter_map(|(element, next)| match element {
                PathEl::MoveTo(p) if !matches!(next, Some(PathEl::MoveTo(_))) => {
                    Some(kurbo::Rect::from_points(*p, *p))
                }
                _ => None,
            });
        let curves = path
            .segments()
            .next()
            .is_some()
            .then(|| path.bounding_box());
        let r = moves.chain(curves).reduce(|a, b| a.union(b))?;
        Some(RectF::new(
            f32_from_f64(r.x0),
            f32_from_f64(r.y0),
            f32_from_f64(r.x1 - r.x0),
            f32_from_f64(r.y1 - r.y0),
        ))
    }

    /// The path as `kurbo`'s, for geometry Skia would do: each conic as a
    /// cubic close to it (its control point pulled in by 4w / 3(1 + w),
    /// exact at a quarter circle's ends and within a hair between), and
    /// the moves Skia injects (to the origin before a first segment
    /// without one, to the subpath's start after a close) made explicit.
    pub fn to_kurbo(&self) -> kurbo::BezPath {
        let pt = |p: PointF| kurbo::Point::new(p.x.into(), p.y.into());
        let mut out = kurbo::BezPath::new();
        let mut current = kurbo::Point::ZERO;
        let mut start = kurbo::Point::ZERO;
        let mut needs_move = true;
        for element in &self.elements {
            if needs_move && !matches!(element, PathElement::MoveTo(_) | PathElement::Close) {
                out.move_to(current);
                start = current;
            }
            needs_move = false;
            match *element {
                PathElement::MoveTo(p) => {
                    current = pt(p);
                    start = current;
                    out.move_to(current);
                }
                PathElement::LineTo(p) => {
                    current = pt(p);
                    out.line_to(current);
                }
                PathElement::QuadTo(c, p) => {
                    current = pt(p);
                    out.quad_to(pt(c), current);
                }
                PathElement::CubicTo(c1, c2, p) => {
                    current = pt(p);
                    out.curve_to(pt(c1), pt(c2), current);
                }
                PathElement::ConicTo(c, p, weight) => {
                    let (c, end) = (pt(c), pt(p));
                    let w = f64::from(weight);
                    let k = 4.0 * w / (3.0 * (1.0 + w));
                    out.curve_to(current.lerp(c, k), end.lerp(c, k), end);
                    current = end;
                }
                PathElement::Close => {
                    if !out.elements().is_empty() {
                        out.close_path();
                    }
                    current = start;
                    needs_move = true;
                }
            }
        }
        out
    }
}

/// A font file shared between layout and raster: the bytes, the face
/// index within them, and an id that is the same for the same bytes, so
/// raster can cache what it builds from them.
///
/// One axis of a variable font and its value, in the axis's units (a
/// design coordinate: `wght` 700).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontVariation {
    pub tag: [u8; 4],
    pub value: f32,
}

/// Glyphs of one font, laid out (a run of shaped text): where each is
/// drawn, in the space the text was laid out in.
#[derive(Debug, Clone, PartialEq)]
pub struct GlyphRun {
    pub font: FontData,
    /// In px.
    pub size: f32,
    pub glyphs: Vec<u16>,
    pub positions: Vec<PointF>,
    /// Bold and oblique the font lacks, made up (as `PaintOp::DrawGlyphs`
    /// takes them).
    pub embolden: bool,
    /// In degrees.
    pub skew: f32,
    /// A variable font's axes as the text was shaped with them; empty for
    /// another font.
    pub variations: Arc<[FontVariation]>,
}

/// A font's lines, in px above the alphabetic baseline (below is
/// negative but for the descents, which are below it): what a canvas's
/// `textBaseline` and `TextMetrics` are measured with (Blink's
/// `FontMetrics`, and the OpenType BASE baselines).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FontMetrics {
    /// hhea's ascent and descent.
    pub ascent: f32,
    pub descent: f32,
    /// OS/2's typo ascent and descent (hhea's without OS/2) scaled to add
    /// up to 1em, each rounded to 1/64 px (Blink's
    /// `NormalizedTypoAscentAndDescent`).
    pub typo_ascent: f32,
    pub typo_descent: f32,
    /// The BASE table's hanging, ideographic (under) and alphabetic
    /// (roman) baselines, if the font has them.
    pub hanging: Option<f32>,
    pub ideographic: Option<f32>,
    pub alphabetic: Option<f32>,
}

/// One line of text laid out, as a canvas's 2D context draws and measures
/// it: its runs in visual order, from x 0 at its left end, y 0 on its
/// alphabetic baseline; its width; its first font's metrics; and where its
/// glyphs put ink (y down), if that was asked for, else an empty rect at
/// the origin, as for no ink.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ShapedText {
    pub runs: Vec<GlyphRun>,
    pub width: f32,
    pub metrics: FontMetrics,
    pub ink: RectF,
}

/// Not serializable: once the renderer is its own process, fonts have
/// to be sent the way Chromium serializes typefaces.
#[derive(Clone)]
pub struct FontData {
    pub bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
    pub index: u32,
    pub id: u64,
}

impl PartialEq for FontData {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.index == other.index
    }
}

impl fmt::Debug for FontData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FontData")
            .field("id", &self.id)
            .field("index", &self.index)
            .field("len", &(*self.bytes).as_ref().len())
            .finish()
    }
}

/// An encoded image (PNG, JPEG, GIF) shared between the renderer and
/// raster: the file's bytes, and an id unique to this image so raster can
/// cache what it decodes. Raster keeps that only while some `ImageData` for
/// the image is alive. Decoding happens at raster time, as Chromium's
/// `PaintImage` defers it to cc.
///
/// A canvas's snapshot is decoded pixels, which the thread drawing the
/// canvas may fill in later (`pending`): what reads them waits for them
/// first (`wait_ready`), as Chromium waits on a shared image's sync token.
///
/// Not serializable, like [`FontData`]: a renderer process would need
/// images sent across another way.
#[derive(Clone)]
pub struct ImageData {
    /// The file's bytes, or the pixels of a decoded one. A pending
    /// snapshot's can't be read before `wait_ready`.
    pub bytes: Arc<dyn AsRef<[u8]> + Send + Sync>,
    /// For decoded pixels (a canvas's snapshot: premultiplied RGBA8, rows
    /// tightly packed), their size; `None` for an encoded file, which
    /// raster decodes.
    pub decoded: Option<Size>,
    pub id: u64,
    /// A pending snapshot's pixels, the same as `bytes`.
    pending: Option<Arc<PendingPixels>>,
}

static IMAGE_IDS: AtomicSequenceNumber<u64> = AtomicSequenceNumber::<u64>::starting_at(1);

impl ImageData {
    /// An encoded file (PNG, JPEG, GIF...).
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Self {
        let bytes: Arc<[u8]> = bytes.into();
        Self {
            bytes: Arc::new(bytes),
            decoded: None,
            id: IMAGE_IDS.get_next(),
            pending: None,
        }
    }

    /// An encoded file whose bytes stay where they are, shared rather
    /// than copied (a response body).
    pub fn shared(bytes: impl AsRef<[u8]> + Send + Sync + 'static) -> Self {
        Self {
            bytes: Arc::new(bytes),
            decoded: None,
            id: IMAGE_IDS.get_next(),
            pending: None,
        }
    }

    /// Decoded pixels: `pixels` are `size`'s, premultiplied RGBA8, rows
    /// tightly packed (cc's `PaintImage` of an `SkImage`: a canvas's
    /// snapshot).
    ///
    /// # Panics
    ///
    /// If `pixels` isn't `size`'s.
    pub fn decoded(size: Size, pixels: Vec<u8>) -> Self {
        assert_eq!(
            Some(pixels.len()),
            size.checked_byte_size(),
            "the pixels are the size's"
        );
        Self {
            bytes: Arc::new(pixels),
            decoded: Some(size),
            id: IMAGE_IDS.get_next(),
            pending: None,
        }
    }

    /// Decoded pixels of `size` that another thread fills in later, through
    /// the filler (Blink's snapshot of a canvas the GPU process draws: a
    /// shared image and the sync token it is ready at).
    pub fn pending(size: Size) -> (Self, PixelsFiller) {
        let pending = Arc::new(PendingPixels::default());
        let image = Self {
            bytes: pending.clone(),
            decoded: Some(size),
            id: IMAGE_IDS.get_next(),
            pending: Some(pending.clone()),
        };
        let filler = PixelsFiller {
            pending,
            len: size.checked_byte_size(),
        };
        (image, filler)
    }

    /// Waits until the pixels can be read, if they are pending: whether
    /// there are any (`false` for a snapshot whose pixels never came: the
    /// thread drawing it had no memory for them, or is gone). Anything
    /// that reads a snapshot's pixels calls it first.
    pub fn wait_ready(&self) -> bool {
        match &self.pending {
            Some(pending) => pending.wait().is_some(),
            None => true,
        }
    }

    /// The bytes: the file's, or the decoded pixels.
    ///
    /// # Panics
    ///
    /// If they are pending and `wait_ready` didn't say they came.
    pub fn as_bytes(&self) -> &[u8] {
        (*self.bytes).as_ref()
    }
}

impl PartialEq for ImageData {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl fmt::Debug for ImageData {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Not the bytes' length: a pending snapshot's can't be read yet.
        f.debug_struct("ImageData")
            .field("id", &self.id)
            .field("decoded", &self.decoded)
            .field("pending", &self.pending.is_some())
            .finish()
    }
}

/// A pending snapshot's pixels: `None` in the cell when they never come.
#[derive(Default)]
struct PendingPixels {
    pixels: std::sync::OnceLock<Option<Arc<Vec<u8>>>>,
    lock: std::sync::Mutex<()>,
    filled: std::sync::Condvar,
}

impl PendingPixels {
    fn lock(&self) -> std::sync::MutexGuard<'_, ()> {
        // Holds nothing: a thread that panicked holding it left no state.
        self.lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Fills the cell, once (later ones are dropped), and wakes the waiters.
    fn set(&self, pixels: Option<Arc<Vec<u8>>>) {
        // Set under the lock, so a waiter that found it empty is waiting
        // before the wake-up.
        let _guard = self.lock();
        let _ = self.pixels.set(pixels);
        self.filled.notify_all();
    }

    fn wait(&self) -> Option<&Arc<Vec<u8>>> {
        if let Some(pixels) = self.pixels.get() {
            return pixels.as_ref();
        }
        let mut guard = self.lock();
        loop {
            if let Some(pixels) = self.pixels.get() {
                return pixels.as_ref();
            }
            guard = self
                .filled
                .wait(guard)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

impl AsRef<[u8]> for PendingPixels {
    fn as_ref(&self) -> &[u8] {
        match self.pixels.get() {
            Some(Some(pixels)) => pixels.as_slice(),
            // Never came: nothing to read, which `wait_ready` said.
            Some(None) => &[],
            None => panic!("a pending snapshot's pixels are read before `wait_ready`"),
        }
    }
}

/// What fills a pending snapshot's pixels in ([`ImageData::pending`]): the
/// thread drawing the canvas, which has it. Dropped without filling them,
/// it tells the waiters they never come.
pub struct PixelsFiller {
    pending: Arc<PendingPixels>,
    /// The size's bytes, which the pixels are (`None`: too many to be).
    len: Option<usize>,
}

impl PixelsFiller {
    /// Fills the pixels in, shared with whoever has them (a canvas's
    /// bitmap, copied before it is drawn on again while they are).
    ///
    /// # Panics
    ///
    /// If `pixels` aren't the size's.
    pub fn fill(self, pixels: Arc<Vec<u8>>) {
        assert_eq!(Some(pixels.len()), self.len, "the pixels are the size's");
        self.pending.set(Some(pixels));
    }
}

impl Drop for PixelsFiller {
    fn drop(&mut self) {
        // After `fill`, a no-op.
        self.pending.set(None);
    }
}

impl fmt::Debug for PixelsFiller {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PixelsFiller")
            .field("len", &self.len)
            .finish()
    }
}

/// One side of an intersection observer's `rootMargin` or `scrollMargin`:
/// CSS px, or a percentage of the root's (or the scroll container's) width
/// or height. Both DOMs' (their observers keep it, paint applies it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MarginLength {
    Px(f64),
    Percent(f64),
}

/// The four sides of a margin: top, right, bottom, left.
pub type Margins = [MarginLength; 4];

#[cfg(test)]
mod tests {
    use super::*;

    /// A pending snapshot's pixels are read once filled in, by another
    /// thread, which a waiter waits for; its size and id are known before.
    #[test]
    fn a_pending_snapshot_is_read_once_filled() {
        let (image, filler) = ImageData::pending(Size::new(1, 1));
        assert_eq!(image.decoded, Some(Size::new(1, 1)));
        let copy = image.clone();
        let waiter = std::thread::spawn(move || {
            assert!(copy.wait_ready());
            copy.as_bytes().to_vec()
        });
        std::thread::spawn(move || {
            // Late enough that the waiter is waiting by then.
            std::thread::sleep(std::time::Duration::from_millis(50));
            filler.fill(Arc::new(vec![1, 2, 3, 4]));
        })
        .join()
        .unwrap();
        assert_eq!(waiter.join().unwrap(), [1, 2, 3, 4]);
        assert!(image.wait_ready());
        assert_eq!(image.as_bytes(), [1, 2, 3, 4]);
    }

    /// Reading a pending snapshot's pixels without waiting panics, rather
    /// than waiting where nobody sees it.
    #[test]
    #[should_panic(expected = "before `wait_ready`")]
    fn a_pending_snapshot_is_not_read_before_waiting() {
        let (image, _filler) = ImageData::pending(Size::new(1, 1));
        let _ = image.as_bytes();
    }

    /// A filler dropped without filling (its thread gone) says the pixels
    /// never come: nobody waits for ever.
    #[test]
    fn a_dropped_filler_says_the_pixels_never_come() {
        let (image, filler) = ImageData::pending(Size::new(1, 1));
        drop(filler);
        assert!(!image.wait_ready());
        assert!(image.as_bytes().is_empty());
        // Images that aren't pending are ready, and are what they hold.
        let decoded = ImageData::decoded(Size::new(1, 1), vec![5, 6, 7, 8]);
        assert!(decoded.wait_ready());
        assert_eq!(decoded.as_bytes(), [5, 6, 7, 8]);
    }

    #[test]
    #[should_panic(expected = "the pixels are the size's")]
    fn a_snapshot_is_filled_with_its_sizes_pixels() {
        let (_image, filler) = ImageData::pending(Size::new(2, 1));
        filler.fill(Arc::new(vec![0; 4]));
    }

    /// The area and the bytes of a size, and `None` where the bytes don't
    /// fit a `usize` (the area of two `u32`s always does on 64 bits).
    #[test]
    fn checked_area_and_byte_size() {
        let size = Size::new(300, 150);
        assert_eq!(size.checked_area(), Some(45_000));
        assert_eq!(size.checked_byte_size(), Some(180_000));
        assert_eq!(Size::new(0, 7).checked_byte_size(), Some(0));
        let largest = Size::new(u32::MAX, u32::MAX);
        if usize::BITS == 64 {
            assert_eq!(largest.checked_area(), Some(18_446_744_065_119_617_025));
            assert_eq!(largest.checked_byte_size(), None);
            // 2^62 pixels are 2^64 bytes, one past `usize::MAX`.
            assert_eq!(Size::new(1 << 31, 1 << 31).checked_byte_size(), None);
            assert_eq!(
                Size::new(1 << 31, (1 << 31) - 1).checked_byte_size(),
                Some(usize::MAX - (1 << 33) + 1)
            );
        } else {
            assert_eq!(largest.checked_area(), None);
        }
    }

    /// A turn by a whole quarter has exact sines and cosines, as Blink's
    /// `SinCosDegrees` gives (a stray 6e-17 would be written out); other
    /// turns are the trigonometry.
    #[test]
    fn quarter_turns_are_exact() {
        assert_eq!(Transform::rotate(90.0).0, [0.0, 1.0, -1.0, 0.0, 0.0, 0.0]);
        assert_eq!(Transform::rotate(180.0).0, [-1.0, 0.0, 0.0, -1.0, 0.0, 0.0]);
        assert_eq!(Transform::rotate(-90.0).0, [0.0, -1.0, 1.0, 0.0, 0.0, 0.0]);
        assert_eq!(Transform::rotate(720.0).0, Transform::IDENTITY.0);
        let [a, b, ..] = Transform::rotate(30.0).0;
        assert!((a - 0.866_025_4).abs() < 1e-6 && (b - 0.5).abs() < 1e-6);
    }

    #[test]
    fn decoded_pixels_keep_their_size_and_each_image_its_own_id() {
        let image = ImageData::decoded(Size::new(2, 1), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(image.decoded, Some(Size::new(2, 1)));
        assert_eq!(image.as_bytes(), [1, 2, 3, 4, 5, 6, 7, 8]);
        let again = ImageData::decoded(Size::new(2, 1), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_ne!(image, again);
        assert_eq!(image, image.clone());
        let file = ImageData::new(&b"png"[..]);
        assert_eq!((file.decoded, file.as_bytes()), (None, &b"png"[..]));
    }

    #[test]
    #[should_panic(expected = "the pixels are the size's")]
    fn decoded_pixels_are_their_sizes() {
        ImageData::decoded(Size::new(2, 2), vec![0; 4]);
    }

    #[test]
    fn transform_then_applies_the_inner_first() {
        let t = Transform::translate(10.0, 0.0).then(&Transform::scale(2.0, 3.0));
        assert_eq!(t.map_point(PointF::new(1.0, 1.0)), PointF::new(12.0, 3.0));
        let r = Transform::scale(2.0, 2.0).map_rect(RectF::new(1.0, 1.0, 2.0, 3.0));
        assert_eq!(r, RectF::new(2.0, 2.0, 4.0, 6.0));
        assert!(Transform::translate(0.0, 0.0).is_identity());
    }

    /// A huge scale about an origin away from 0 doesn't overflow to an
    /// infinity, which the next product would make NaN: its parts are the
    /// biggest f32s; finite ones are worked out as they are.
    #[test]
    fn a_product_stays_finite() {
        let big = Transform::scale(-f32::MAX, -f32::MAX);
        let about = Transform::translate(392.0, 10.0)
            .then(&big)
            .then(&Transform::translate(-392.0, -10.0));
        let placed = Transform::translate(8.0, 8.0).then(&about);
        assert!(placed.0.iter().all(|v| v.is_finite()), "{placed:?}");
        assert_eq!(placed.0[4], f32::MAX);
        // An infinity it is given stays one.
        let infinite = Transform::translate(f32::INFINITY, 0.0).then(&Transform::scale(2.0, 2.0));
        assert_eq!(infinite.0[4], f32::INFINITY);
        assert_eq!(
            Transform::translate(10.0, 0.0).then(&Transform::scale(2.0, 3.0)),
            Transform([2.0, 0.0, 0.0, 3.0, 10.0, 0.0])
        );
    }

    #[test]
    fn inverse_undoes_the_transform() {
        let t = Transform::translate(10.0, 5.0)
            .then(&Transform([1.0, 0.5, -0.5, 2.0, 0.0, 0.0]))
            .then(&Transform::scale(2.0, 4.0));
        let inverse = t.inverse().expect("invertible");
        let p = PointF::new(3.0, -7.0);
        let back = inverse.map_point(t.map_point(p));
        assert!(
            (back.x - p.x).abs() < 1e-4 && (back.y - p.y).abs() < 1e-4,
            "{back:?}"
        );
        assert_eq!(
            Transform::translate(10.0, 5.0).inverse(),
            Some(Transform::translate(-10.0, -5.0))
        );
        assert_eq!(Transform::scale(0.0, 1.0).inverse(), None);
    }

    #[test]
    fn intersect() {
        let a = RectF::new(0.0, 0.0, 10.0, 10.0);
        let b = RectF::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), RectF::new(5.0, 5.0, 5.0, 5.0));
        assert!(a.intersect(&RectF::new(20.0, 0.0, 1.0, 1.0)).is_empty());
    }

    #[test]
    fn premultiply() {
        assert_eq!(
            Color::rgba(255, 128, 0, 128).premultiplied(),
            [128, 64, 0, 128]
        );
        assert_eq!(Color::WHITE.premultiplied(), [255, 255, 255, 255]);
    }

    #[test]
    fn rounded_rects_fit_their_corners_and_contain_points_inside_the_curves() {
        let rect = RectF::new(0.0, 0.0, 100.0, 50.0);
        // 60 + 60 across 100: everything scaled by 100 / 120.
        let r = RRectF::new(rect, [SizeF::new(60.0, 10.0); 4]);
        assert!((r.radii[0].width - 50.0).abs() < 1e-4);
        assert!((r.radii[0].height - 50.0 / 6.0).abs() < 1e-4);
        let r = RRectF::uniform(rect, 20.0);
        assert!(r.contains(PointF::new(50.0, 25.0)));
        assert!(r.contains(PointF::new(20.0, 1.0)));
        // Past the top left curve, but inside the rect.
        assert!(!r.contains(PointF::new(2.0, 2.0)));
        assert!(!r.contains(PointF::new(98.0, 48.0)));
        assert!(r.contains(PointF::new(10.0, 10.0)));
        assert!(!r.contains(PointF::new(101.0, 25.0)));
        // A corner with one radius 0 is square.
        let r = RRectF::new(rect, [SizeF::new(20.0, 0.0); 4]);
        assert!(r.is_rect());
        // Inside a border: the radii less the widths, no less than 0.
        let inner = RRectF::uniform(rect, 20.0).inset(5.0, 30.0, 5.0, 5.0);
        assert_eq!(inner.rect, RectF::new(5.0, 5.0, 65.0, 40.0));
        assert_eq!(inner.radii[0], SizeF::new(15.0, 15.0));
        assert_eq!(inner.radii[1], SizeF::new(0.0, 0.0));
        // A spread: a radius at least as big grows by it, a smaller one
        // by less, a square corner not at all; shrinking takes it off.
        let r = RRectF::new(
            rect,
            [
                SizeF::new(10.0, 10.0),
                SizeF::new(2.0, 2.0),
                SizeF::new(0.0, 0.0),
                SizeF::new(10.0, 10.0),
            ],
        );
        let grown = r.spread(4.0);
        assert_eq!(grown.rect, RectF::new(-4.0, -4.0, 108.0, 58.0));
        assert_eq!(grown.radii[0], SizeF::new(14.0, 14.0));
        // 2 + 4 * (1 + (0.5 - 1)^3) = 5.5.
        assert_eq!(grown.radii[1], SizeF::new(5.5, 5.5));
        assert_eq!(grown.radii[2], SizeF::new(0.0, 0.0));
        let shrunk = r.spread(-4.0);
        assert_eq!(shrunk.rect, RectF::new(4.0, 4.0, 92.0, 42.0));
        assert_eq!(shrunk.radii[0], SizeF::new(6.0, 6.0));
        assert_eq!(shrunk.radii[1], SizeF::new(0.0, 0.0));
    }

    #[test]
    fn a_paths_bounds_hold_its_control_points() {
        let p = PointF::new;
        assert_eq!(Path::new().bounds(), None);
        let mut path = Path::new();
        path.move_to(p(10.0, 10.0));
        path.quad_to(p(20.0, -5.0), p(30.0, 10.0));
        path.cubic_to(p(40.0, 20.0), p(5.0, 40.0), p(10.0, 30.0));
        path.close();
        assert_eq!(path.bounds(), Some(RectF::new(5.0, -5.0, 35.0, 45.0)));
        // After the close, a curve from the subpath's start.
        path.conic_to(p(45.0, 10.0), p(40.0, 50.0), 0.5);
        assert_eq!(path.bounds(), Some(RectF::new(5.0, -5.0, 40.0, 55.0)));
        // With no move at all, from the origin.
        let mut path = Path::new();
        path.line_to(p(10.0, 20.0));
        assert_eq!(path.bounds(), Some(RectF::new(0.0, 0.0, 10.0, 20.0)));
        assert_eq!(path.fill_rule(), FillRule::NonZero);
        assert_eq!(
            path.with_fill_rule(FillRule::EvenOdd).fill_rule(),
            FillRule::EvenOdd
        );
    }

    /// The curves' own extent, not their control points': a quarter
    /// circle as a conic reaches its ends, a quadratic halfway to its
    /// control point; the moves Skia injects count as points.
    #[test]
    fn a_paths_tight_bounds_hold_its_curves() {
        let p = PointF::new;
        assert_eq!(Path::new().tight_bounds(), None);
        let mut path = Path::new();
        path.move_to(p(10.0, 0.0));
        path.conic_to(p(10.0, 10.0), p(0.0, 10.0), std::f32::consts::FRAC_1_SQRT_2);
        let bounds = path.tight_bounds().unwrap();
        let close = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(
            close(bounds.x, 0.0)
                && close(bounds.y, 0.0)
                && close(bounds.width, 10.0)
                && close(bounds.height, 10.0),
            "{bounds:?}"
        );
        let mut path = Path::new();
        path.move_to(p(0.0, 0.0));
        path.quad_to(p(10.0, 20.0), p(20.0, 0.0));
        assert_eq!(path.tight_bounds(), Some(RectF::new(0.0, 0.0, 20.0, 10.0)));
        assert_eq!(path.bounds(), Some(RectF::new(0.0, 0.0, 20.0, 20.0)));
        // From the origin without a move; after a close, from the start.
        let mut path = Path::new();
        path.line_to(p(10.0, 20.0));
        assert_eq!(path.tight_bounds(), Some(RectF::new(0.0, 0.0, 10.0, 20.0)));
        let mut path = Path::new();
        path.move_to(p(5.0, 5.0));
        path.line_to(p(10.0, 5.0));
        path.close();
        path.line_to(p(5.0, 15.0));
        assert_eq!(path.tight_bounds(), Some(RectF::new(5.0, 5.0, 5.0, 10.0)));
        // A move alone is at its point; one right before another isn't
        // kept; a last one after the curves counts.
        let mut moves = Path::new();
        moves.move_to(p(10.0, 10.0));
        assert_eq!(moves.tight_bounds(), Some(RectF::new(10.0, 10.0, 0.0, 0.0)));
        moves.move_to(p(30.0, 20.0));
        assert_eq!(moves.tight_bounds(), Some(RectF::new(30.0, 20.0, 0.0, 0.0)));
        moves.line_to(p(40.0, 20.0));
        moves.move_to(p(50.0, 30.0));
        assert_eq!(
            moves.tight_bounds(),
            Some(RectF::new(30.0, 20.0, 20.0, 10.0))
        );
        let kurbo = path.to_kurbo();
        assert_eq!(
            kurbo.elements()[3],
            kurbo::PathEl::MoveTo(kurbo::Point::new(5.0, 5.0)),
            "the move Skia injects after a close"
        );
    }

    #[test]
    fn polygons_are_closed_subpaths() {
        let p = PointF::new;
        let path = Path::polygons([vec![p(0.0, 0.0), p(4.0, 0.0), p(0.0, 4.0)], vec![]]);
        assert_eq!(
            path.elements(),
            [
                PathElement::MoveTo(p(0.0, 0.0)),
                PathElement::LineTo(p(4.0, 0.0)),
                PathElement::LineTo(p(0.0, 4.0)),
                PathElement::Close,
            ]
        );
    }

    /// Lengths and points along paths, as Chrome 152 gives them for SVG's
    /// shapes (Skia's `SkContourMeasure`: a circle is shorter than 2πr) and
    /// for a `<path>`'s data (Blink's traversal: nearly exact).
    #[test]
    fn lengths_along_paths_are_chromes() {
        let p = PointF::new;
        let near = |a: PointF, b: PointF| (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3;
        let check = |path: &Path, skia: bool, length: f32, points: [PointF; 4]| {
            let total = if skia {
                path.length()
            } else {
                path.traversal_length()
            };
            // Within Chrome's last digit (an arc's cubics differ a hair:
            // Blink makes them its own way).
            assert!((total - length).abs() < 2e-4, "{total} for {length}");
            for (fraction, want) in [0.1, 0.25, 0.5, 0.77].into_iter().zip(points) {
                let at = if skia {
                    path.point_at_length(total * fraction)
                } else {
                    path.traversal_point_at_length(total * fraction)
                };
                assert!(near(at, want), "{at:?} at {fraction} for {want:?}");
            }
        };
        // `<circle cx=50 cy=50 r=10>` and `<ellipse rx=30 ry=10>`: from
        // the right end, clockwise.
        let mut circle = Path::new();
        circle.add_oval(RectF::new(40.0, 40.0, 20.0, 20.0), true, 1);
        check(
            &circle,
            true,
            62.427,
            [
                p(58.1017, 55.8619),
                p(50.0, 60.0),
                p(40.0, 50.0),
                p(51.2031, 40.0726),
            ],
        );
        let mut ellipse = Path::new();
        ellipse.add_oval(RectF::new(20.0, 40.0, 60.0, 20.0), true, 1);
        check(
            &ellipse,
            true,
            133.3761,
            [
                p(69.9374, 57.4722),
                p(50.0, 60.0),
                p(20.0, 50.0),
                p(52.598, 40.0376),
            ],
        );
        // `<rect width=40 height=20 rx=5>`.
        let mut rect = Path::new();
        rect.add_round_rect(
            RectF::new(0.0, 0.0, 40.0, 20.0),
            [SizeF::new(5.0, 5.0); 4],
            true,
        );
        check(
            &rect,
            true,
            110.6147,
            [
                p(16.0615, 0.0),
                p(32.6537, 0.0),
                p(35.0, 20.0),
                p(5.134, 20.0),
            ],
        );
        // `<path d="M60 50 A10 10 0 1 1 40 50 A10 10 0 1 1 60 50">`: the
        // arcs as conics, which are cubics to the traversal.
        let w = std::f32::consts::FRAC_1_SQRT_2;
        let mut arcs = Path::new();
        arcs.move_to(p(60.0, 50.0));
        for (c, end) in [
            (p(60.0, 60.0), p(50.0, 60.0)),
            (p(40.0, 60.0), p(40.0, 50.0)),
            (p(40.0, 40.0), p(50.0, 40.0)),
            (p(60.0, 40.0), p(60.0, 50.0)),
        ] {
            arcs.conic_to(c, end, w);
        }
        check(
            &arcs,
            false,
            62.8408,
            [
                p(58.0908, 55.8781),
                p(50.0, 60.0),
                p(40.0, 50.0),
                p(51.2536, 40.0779),
            ],
        );
        let mut quad = Path::new();
        quad.move_to(p(0.0, 0.0));
        quad.quad_to(p(50.0, 100.0), p(100.0, 0.0));
        check(
            &quad,
            false,
            147.8943,
            [
                p(7.0023, 13.0239),
                p(19.4632, 31.3499),
                p(50.0, 50.0),
                p(82.3772, 29.0342),
            ],
        );
        let mut cubic = Path::new();
        cubic.move_to(p(0.0, 0.0));
        cubic.cubic_to(p(0.0, 100.0), p(100.0, 100.0), p(100.0, 0.0));
        check(
            &cubic,
            false,
            200.0001,
            [
                p(1.4622, 19.9268),
                p(10.5893, 48.3524),
                p(50.0, 75.0),
                p(91.2126, 44.7819),
            ],
        );
        // A close, then a new subpath: the close counts.
        let mut closed = Path::new();
        closed.move_to(p(0.0, 0.0));
        closed.line_to(p(10.0, 0.0));
        closed.close();
        closed.move_to(p(20.0, 0.0));
        closed.line_to(p(20.0, 10.0));
        check(
            &closed,
            false,
            30.0,
            [p(3.0, 0.0), p(7.5, 0.0), p(5.0, 0.0), p(20.0, 3.1)],
        );
        // After a close, a segment goes on from the subpath's start.
        let mut reopened = Path::new();
        reopened.move_to(p(0.0, 0.0));
        reopened.line_to(p(10.0, 0.0));
        reopened.close();
        reopened.line_to(p(0.0, 10.0));
        check(
            &reopened,
            false,
            30.0,
            [p(3.0, 0.0), p(7.5, 0.0), p(5.0, 0.0), p(0.0, 3.1)],
        );
        // Skia's contours one after another.
        let mut two = Path::new();
        two.move_to(p(0.0, 0.0));
        two.line_to(p(10.0, 0.0));
        two.move_to(p(20.0, 0.0));
        two.line_to(p(20.0, 10.0));
        assert_eq!(two.length(), 20.0);
        assert_eq!(two.point_at_length(15.0), p(20.0, 5.0));
        // A move alone: no length, its point; Skia's past the end is the
        // first point.
        let mut moves = Path::new();
        moves.move_to(p(5.0, 5.0));
        assert_eq!(moves.traversal_length(), 0.0);
        assert_eq!(moves.traversal_point_at_length(0.0), p(5.0, 5.0));
        assert_eq!(moves.length(), 0.0);
        assert_eq!(moves.point_at_length(3.0), p(5.0, 5.0));
        // `<polygon points="0,0 10,0 10,10">`: closed, so back to the start.
        let polygon = Path::polygons([[p(0.0, 0.0), p(10.0, 0.0), p(10.0, 10.0)]]);
        check(
            &polygon,
            true,
            34.1421,
            [
                p(3.4142, 0.0),
                p(8.5355, 0.0),
                p(10.0, 7.0711),
                p(5.5527, 5.5527),
            ],
        );
    }
}
