//! What SVG's geometry members compute, for the DOM's bindings: an
//! element's geometry and its text's characters as laid out
//! ([`SvgGeometry`], [`SvgTextChar`]), `SVGMatrix`'s operations on its six
//! numbers (Blink's `SVGMatrixTearOff` over `AffineTransform`: a matrix
//! multiplied on the right), the text methods' character queries (Blink's
//! `SVGTextQuery`), and a shape's length along its path
//! (`SVGGeometryElement`). Numbers script gives are WebIDL `float`s,
//! rounded here.

use base::numerics::{clamp_round, f32_from_f64};

use crate::{Path, PointF, RectF, Transform};

/// What script reads of an SVG element's geometry, as laid out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgGeometry {
    /// Its object bounding box, in its user space (`getBBox()`): empty at
    /// the origin if it has none or isn't rendered.
    pub bbox: RectF,
    /// From its user space to its nearest viewport's (`getCTM()`).
    pub ctm: Transform,
    /// From its user space to the viewport of the frame showing it, CSS px
    /// (`getScreenCTM()`).
    pub screen_ctm: Transform,
    /// Its bounding box there, its one client rect (`getClientRects()`);
    /// `None` if it has none: not rendered, or in a hidden container
    /// (`<defs>`). Also what makes it a rendered one that can take the
    /// focus.
    pub client_rect: Option<RectF>,
}

impl Default for SvgGeometry {
    fn default() -> Self {
        Self {
            bbox: RectF::new(0.0, 0.0, 0.0, 0.0),
            ctm: Transform::IDENTITY,
            screen_ctm: Transform::IDENTITY,
            client_rect: None,
        }
    }
}

/// One character's cell of SVG text, in the `<text>`'s user space (Blink's
/// `SVGTextQuery`'s per-character metrics).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgTextChar {
    /// How many character numbers it takes: its UTF-16 code units.
    pub units: usize,
    /// Its origin on the baseline (`getStartPositionOfChar`).
    pub start: PointF,
    /// Its advance on from there, turned with it (`getEndPositionOfChar`).
    pub end: PointF,
    /// Its cell's corners: the advance by the font's ascent and descent,
    /// turned (`getExtentOfChar` is their bounds).
    pub corners: [PointF; 4],
    /// Degrees, clockwise (`getRotationOfChar`).
    pub rotation: f32,
    pub advance: f32,
}

/// An SVG image's natural dimensions (Blink's `LayoutSVGRoot::
/// ComputeIntrinsicSizingInfo` for an `SVGImage`): its root `<svg>`'s
/// `width` and `height` attributes, where they are lengths (`em` of its
/// font size), not percentages, and not its CSS; its ratio from its
/// `viewBox` (with both a width and a height, those give the ratio, before
/// it).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SvgNaturalSize {
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// Its `viewBox`'s width over its height.
    pub ratio: Option<f32>,
}

impl SvgNaturalSize {
    /// What `naturalWidth` and `naturalHeight` say of an SVG image, as
    /// HTML and Chrome do: each natural dimension, one missing through the
    /// other and the ratio; else the default object size's (300×150), with
    /// a ratio alone its width's and the height through the ratio. And
    /// which of them the image gave (directly or through the ratio): a
    /// density corrects only those, not the default's.
    pub fn dom_size(self) -> (crate::Size, [bool; 2]) {
        let width = self.width.or_else(|| Some(self.height? * self.ratio?));
        let height = self.height.or_else(|| Some(self.width? / self.ratio?));
        let default_height = match (width, self.ratio) {
            (None, Some(ratio)) => 300.0 / ratio,
            _ => 150.0,
        };
        (
            crate::Size::new(
                clamp_round(width.unwrap_or(300.0)),
                clamp_round(height.unwrap_or(default_height)),
            ),
            [width.is_some(), height.is_some()],
        )
    }
}

/// An `SVGMatrix`'s `a` to `f`: the 2D matrix [a c e; b d f; 0 0 1].
pub type MatrixValues = [f64; 6];

/// A WebIDL `float`'s value: a double rounded to a float.
pub fn webidl_float(value: f64) -> f64 {
    f64::from(f32_from_f64(value))
}

/// `SVGMatrix`'s operations: each a new matrix, the matrix it is called on
/// times another (the transform it names).
pub mod matrix {
    use super::{MatrixValues, webidl_float as float};

    pub const IDENTITY: MatrixValues = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

    /// `values` × `other`.
    pub fn multiply(
        [a, b, c, d, e, f]: MatrixValues,
        [a2, b2, c2, d2, e2, f2]: MatrixValues,
    ) -> MatrixValues {
        [
            a * a2 + c * b2,
            b * a2 + d * b2,
            a * c2 + c * d2,
            b * c2 + d * d2,
            a * e2 + c * f2 + e,
            b * e2 + d * f2 + f,
        ]
    }

    /// `inverse()`; `None` for one not invertible (an `InvalidStateError`).
    pub fn inverse([a, b, c, d, e, f]: MatrixValues) -> Option<MatrixValues> {
        let det = a * d - b * c;
        if det == 0.0 || !det.is_finite() {
            return None;
        }
        Some([
            d / det,
            -b / det,
            -c / det,
            a / det,
            (c * f - d * e) / det,
            (b * e - a * f) / det,
        ])
    }

    pub fn translate(x: f64, y: f64) -> MatrixValues {
        [1.0, 0.0, 0.0, 1.0, float(x), float(y)]
    }

    pub fn scale(factor: f64) -> MatrixValues {
        let factor = float(factor);
        [factor, 0.0, 0.0, factor, 0.0, 0.0]
    }

    pub fn scale_non_uniform(x: f64, y: f64) -> MatrixValues {
        [float(x), 0.0, 0.0, float(y), 0.0, 0.0]
    }

    /// A turn by `degrees`, clockwise in SVG's y-down space.
    fn rotation(degrees: f64) -> MatrixValues {
        let (sin, cos) = degrees.to_radians().sin_cos();
        [cos, sin, -sin, cos, 0.0, 0.0]
    }

    pub fn rotate(angle: f64) -> MatrixValues {
        rotation(float(angle))
    }

    /// `rotateFromVector()`: the turn to (`x`, `y`); `None` if either is 0
    /// (an `InvalidAccessError`).
    pub fn rotate_from_vector(x: f64, y: f64) -> Option<MatrixValues> {
        let (x, y) = (float(x), float(y));
        if x == 0.0 || y == 0.0 {
            return None;
        }
        Some(rotation(y.atan2(x).to_degrees()))
    }

    pub const FLIP_X: MatrixValues = [-1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    pub const FLIP_Y: MatrixValues = [1.0, 0.0, 0.0, -1.0, 0.0, 0.0];

    pub fn skew_x(angle: f64) -> MatrixValues {
        [1.0, 0.0, float(angle).to_radians().tan(), 1.0, 0.0, 0.0]
    }

    pub fn skew_y(angle: f64) -> MatrixValues {
        [1.0, float(angle).to_radians().tan(), 0.0, 1.0, 0.0, 0.0]
    }

    /// `SVGPoint`'s `matrixTransform()`: (`x`, `y`) through `matrix`, as
    /// floats.
    pub fn transform_point([a, b, c, d, e, f]: MatrixValues, [x, y]: [f64; 2]) -> [f64; 2] {
        [float(a * x + c * y + e), float(b * x + d * y + f)]
    }
}

/// `SVGTextContentElement`'s queries of a text's characters, as laid out.
/// Character numbers are UTF-16 code units of the text, white space
/// processed, as in Chrome: a cell (a grapheme cluster, an emoji's
/// surrogate pair) answers for each of its code units, and its width
/// counts once in a length.
pub mod text {
    use super::SvgTextChar;
    use crate::{PointF, RectF};

    /// `getNumberOfChars()`.
    pub fn count(chars: &[SvgTextChar]) -> usize {
        chars.iter().map(|c| c.units).sum()
    }

    /// Each character with the range of character numbers it answers for.
    fn numbered(
        chars: &[SvgTextChar],
    ) -> impl Iterator<Item = (std::ops::Range<usize>, &SvgTextChar)> {
        chars.iter().scan(0, |start, c| {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "the units of a text in memory: their sum fits a usize"
            )]
            let range = *start..*start + c.units;
            *start = range.end;
            Some((range, c))
        })
    }

    /// The character `charnum` is a code unit of; `None` past the end (an
    /// `IndexSizeError`).
    pub fn char_at(chars: &[SvgTextChar], charnum: usize) -> Option<SvgTextChar> {
        numbered(chars)
            .find(|(range, _)| range.contains(&charnum))
            .map(|(_, c)| *c)
    }

    /// `getComputedTextLength()`: the characters' advances together.
    pub fn computed_length(chars: &[SvgTextChar]) -> f32 {
        chars.iter().map(|c| c.advance).sum()
    }

    /// `getSubStringLength()`: the advances of the characters with any of
    /// the numbers from `charnum`, `nchars` of them (cut at the end);
    /// `None` for a `charnum` past the end.
    pub fn sub_string_length(chars: &[SvgTextChar], charnum: usize, nchars: usize) -> Option<f32> {
        if charnum >= count(chars) {
            return None;
        }
        // Past the end is to the end.
        let end = charnum.saturating_add(nchars);
        Some(
            numbered(chars)
                .filter(|(range, _)| range.start < end && charnum < range.end)
                .map(|(_, c)| c.advance)
                .sum(),
        )
    }

    /// `getExtentOfChar()`: the bounds of its cell, turned with it.
    pub fn extent(c: &SvgTextChar) -> RectF {
        let [first, rest @ ..] = c.corners;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in rest {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        RectF::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// `getCharNumAtPosition()`: the number of the first character whose
    /// cell holds `point`, in the `<text>`'s user space.
    pub fn char_num_at(chars: &[SvgTextChar], point: PointF) -> Option<usize> {
        numbered(chars)
            .find(|(_, c)| in_quad(&c.corners, point))
            .map(|(range, _)| range.start)
    }

    /// Whether `p` is in a cell, its edges included: on the inner side of
    /// each of its edges, its corners going around clockwise (a turn keeps
    /// them so).
    fn in_quad(corners: &[PointF; 4], p: PointF) -> bool {
        let side = |a: PointF, b: PointF| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
        let next = corners.iter().cycle().skip(1);
        corners.iter().zip(next).all(|(&a, &b)| side(a, b) >= 0.0)
    }
}

/// `getTotalLength()`: a shape's length along its path, a `<path>`'s
/// (`is_path`) along its path data as Blink's `SVGPathQuery` measures it,
/// another's as Skia's `SkContourMeasure` does (not scaled by
/// `pathLength`, as in Chrome).
pub fn shape_length(path: &Path, is_path: bool) -> f32 {
    if is_path {
        path.traversal_length()
    } else {
        path.length()
    }
}

/// `getPointAtLength()`: the point `distance` (a WebIDL `float`) along a
/// shape's path, measured as [`shape_length`], the distance clamped to it;
/// `None` for an empty path (an `InvalidStateError`).
pub fn point_at_length(path: &Path, is_path: bool, distance: f64) -> Option<PointF> {
    if path.elements().is_empty() {
        return None;
    }
    let distance = f32_from_f64(distance).clamp(0.0, shape_length(path, is_path));
    Some(if is_path {
        path.traversal_point_at_length(distance)
    } else {
        path.point_at_length(distance)
    })
}
