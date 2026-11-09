//! `filter` as the layer it applies to is composited with (Blink's
//! `FilterEffectBuilder` and cc's `RenderSurfaceFilters`): each function an
//! image filter, the color ones Skia's color matrices, in sRGB as Chrome
//! applies them.

use compositor_paint::ImageFilter;
use computed_style::{ComputedStyle, FilterFunction};

/// `style`'s `filter`, `None` for `none`.
pub(crate) fn image_filter(style: &ComputedStyle) -> Option<ImageFilter> {
    let filters: Vec<ImageFilter> = style
        .rare
        .filter
        .iter()
        .map(|function| one(function, style))
        .collect();
    match filters.len() {
        0 => None,
        1 => filters.into_iter().next(),
        _ => Some(ImageFilter::Chain(filters.into())),
    }
}

fn one(function: &FilterFunction, style: &ComputedStyle) -> ImageFilter {
    match *function {
        FilterFunction::Blur(sigma) => ImageFilter::Blur { sigma },
        // Its blur a radius, as a box shadow's: half of it is the sigma.
        FilterFunction::DropShadow(shadow) => ImageFilter::DropShadow {
            offset: (shadow.x, shadow.y),
            sigma: shadow.blur / 2.0,
            color: style.resolve(shadow.color),
            shadow_only: false,
        },
        FilterFunction::Brightness(a) => ImageFilter::ColorMatrix(scale_rgb(a, 0.0)),
        FilterFunction::Contrast(a) => ImageFilter::ColorMatrix(scale_rgb(a, 0.5 - 0.5 * a)),
        FilterFunction::Grayscale(a) => ImageFilter::ColorMatrix(grayscale(1.0 - a)),
        FilterFunction::Sepia(a) => ImageFilter::ColorMatrix(sepia(1.0 - a)),
        FilterFunction::Saturate(a) => ImageFilter::ColorMatrix(saturate(a)),
        FilterFunction::HueRotate(degrees) => ImageFilter::ColorMatrix(hue_rotate(degrees)),
        FilterFunction::Invert(a) => ImageFilter::ColorMatrix(invert(a)),
        FilterFunction::Opacity(a) => {
            let mut m = IDENTITY;
            m[18] = a;
            ImageFilter::ColorMatrix(m)
        }
    }
}

/// How far past what was drawn `filter` may draw, in its space: three
/// standard deviations of a blur, and a shadow's offset too.
pub(crate) fn reach(filter: &ImageFilter) -> f32 {
    match filter {
        ImageFilter::Blur { sigma } => 3.0 * sigma,
        ImageFilter::DropShadow { offset, sigma, .. } => {
            offset.0.abs().max(offset.1.abs()) + 3.0 * sigma
        }
        ImageFilter::ColorMatrix(_) => 0.0,
        ImageFilter::Chain(filters) => filters.iter().map(reach).sum(),
    }
}

const IDENTITY: [f32; 20] = [
    1.0, 0.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, 0.0,
];

/// Each color channel times `scale`, plus `add`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "row < 3: indices into the 4×5 matrix"
)]
fn scale_rgb(scale: f32, add: f32) -> [f32; 20] {
    let mut m = IDENTITY;
    for row in 0..3 {
        m[row * 5 + row] = scale;
        m[row * 5 + 4] = add;
    }
    m
}

/// The rows of a matrix for red, green and blue, alpha kept.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "row < 3: indices into the 4×5 matrix"
)]
fn rgb(rows: [[f32; 3]; 3]) -> [f32; 20] {
    let mut m = IDENTITY;
    for (row, values) in rows.iter().enumerate() {
        m[row * 5..row * 5 + 3].copy_from_slice(values);
    }
    m
}

/// cc's `GetGrayscaleMatrix`, `a` the share of the color kept.
fn grayscale(a: f32) -> [f32; 20] {
    rgb([
        [
            0.2126 + 0.7874 * a,
            0.7152 - 0.7152 * a,
            0.0722 - 0.0722 * a,
        ],
        [
            0.2126 - 0.2126 * a,
            0.7152 + 0.2848 * a,
            0.0722 - 0.0722 * a,
        ],
        [
            0.2126 - 0.2126 * a,
            0.7152 - 0.7152 * a,
            0.0722 + 0.9278 * a,
        ],
    ])
}

/// cc's `GetSepiaMatrix`, `a` the share of the color kept.
fn sepia(a: f32) -> [f32; 20] {
    rgb([
        [0.393 + 0.607 * a, 0.769 - 0.769 * a, 0.189 - 0.189 * a],
        [0.349 - 0.349 * a, 0.686 + 0.314 * a, 0.168 - 0.168 * a],
        [0.272 - 0.272 * a, 0.534 - 0.534 * a, 0.131 + 0.869 * a],
    ])
}

/// cc's `GetSaturateMatrix`.
fn saturate(s: f32) -> [f32; 20] {
    rgb([
        [0.213 + 0.787 * s, 0.715 - 0.715 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 + 0.285 * s, 0.072 - 0.072 * s],
        [0.213 - 0.213 * s, 0.715 - 0.715 * s, 0.072 + 0.928 * s],
    ])
}

/// cc's `GetHueRotateMatrix`.
fn hue_rotate(degrees: f32) -> [f32; 20] {
    let (sin, cos) = degrees.to_radians().sin_cos();
    rgb([
        [
            0.213 + cos * 0.787 - sin * 0.213,
            0.715 - cos * 0.715 - sin * 0.715,
            0.072 - cos * 0.072 + sin * 0.928,
        ],
        [
            0.213 - cos * 0.213 + sin * 0.143,
            0.715 + cos * 0.285 + sin * 0.140,
            0.072 - cos * 0.072 - sin * 0.283,
        ],
        [
            0.213 - cos * 0.213 - sin * 0.787,
            0.715 - cos * 0.715 + sin * 0.715,
            0.072 + cos * 0.928 + sin * 0.072,
        ],
    ])
}

/// Each channel `c` as `(1 − 2a) c + a`.
fn invert(a: f32) -> [f32; 20] {
    scale_rgb(1.0 - 2.0 * a, a)
}
