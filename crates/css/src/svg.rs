//! The SVG properties' own values (css-fill-stroke 3, SVG 2's geometry
//! properties): paints, dash arrays, path data, and the `transform`
//! attribute's list. Path data and the attribute's transforms are SVG's
//! own syntax, parsed by `svgtypes`.

use std::sync::Arc;

use base::numerics::f32_from_f64;
use computed_style::{PathSegment, SvgPaint, SvgPaintFallback};
use computed_style::{absolute_path, segment_is_abs};
use cssparser::{Parser, Token};
use gfx::Path;

use crate::css_url_data::CssUrlData;
use crate::effects::TransformFunction;
use crate::values::{
    ComputeContext, LengthPercentage, ParseResult, SpecifiedColor, parse_style_color, write_number,
};

/// `fill` or `stroke` as written: `none`, a color, or a `url()` and
/// what to paint if it isn't to a paint server.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedPaint {
    None,
    Color(SpecifiedColor),
    Url(CssUrlData, SpecifiedFallback),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedFallback {
    Absent,
    None,
    Color(SpecifiedColor),
}

impl SpecifiedPaint {
    /// `none | <color> | <url> [none | <color>]?` (Chrome has
    /// `context-fill` and `context-stroke` too, for markers, which machopo
    /// doesn't draw).
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
            return Ok(Self::None);
        }
        let Ok(url) = input.try_parse(parse_url) else {
            return Ok(Self::Color(parse_style_color(input)?));
        };
        let fallback = if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
            SpecifiedFallback::None
        } else if let Ok(color) = input.try_parse(parse_style_color) {
            SpecifiedFallback::Color(color)
        } else {
            SpecifiedFallback::Absent
        };
        Ok(Self::Url(url, fallback))
    }

    /// A URL is what it resolved to, as a background image's is; a fragment
    /// alone isn't resolved: it stays as written, an element of the same
    /// document (Blink's `CSSUrlData` with `IsLocal`).
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> SvgPaint {
        match self {
            Self::None => SvgPaint::None,
            Self::Color(color) => SvgPaint::Color(color.compute(cx)),
            Self::Url(url, fallback) => {
                let url = url.computed();
                let fallback = match fallback {
                    SpecifiedFallback::Absent => SvgPaintFallback::Absent,
                    SpecifiedFallback::None => SvgPaintFallback::None,
                    SpecifiedFallback::Color(color) => SvgPaintFallback::Color(color.compute(cx)),
                };
                SvgPaint::Url(url, fallback)
            }
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::None => dest.push_str("none"),
            Self::Color(color) => color.to_css(dest),
            Self::Url(url, fallback) => {
                url_to_css(url.specified(), dest);
                match fallback {
                    SpecifiedFallback::Absent => {}
                    SpecifiedFallback::None => dest.push_str(" none"),
                    SpecifiedFallback::Color(color) => {
                        dest.push(' ');
                        color.to_css(dest);
                    }
                }
            }
        }
    }
}

/// `url(…)`, quoted or not.
pub(crate) fn parse_url<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, CssUrlData> {
    let location = input.current_source_location();
    match input.next()?.clone() {
        Token::UnquotedUrl(url) => Ok(CssUrlData::new(&url)),
        Token::Function(name) if name.eq_ignore_ascii_case("url") => {
            input.parse_nested_block(|i| Ok(CssUrlData::new(&i.expect_string()?.clone())))
        }
        _ => Err(location.new_custom_error(())),
    }
}

/// `url("…")`, as Chrome writes it.
pub(crate) fn url_to_css(url: &str, dest: &mut String) {
    dest.push_str("url(");
    cssparser::serialize_string(url, dest).expect("writing to a String");
    dest.push(')');
}

/// `stroke-dasharray`: `none` (`None`), or non-negative lengths,
/// percentages and numbers separated by commas, white space or both, as
/// Chrome takes them (css-fill-stroke 3 has commas only).
pub(crate) fn parse_dasharray<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Option<Arc<[LengthPercentage]>>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    let mut dashes = vec![LengthPercentage::parse_svg(input, false)?];
    loop {
        let comma = input.try_parse(|i| i.expect_comma()).is_ok();
        match input.try_parse(|i| LengthPercentage::parse_svg(i, false)) {
            Ok(dash) => dashes.push(dash),
            Err(_) if comma => return Err(input.new_custom_error(())),
            Err(_) => break,
        }
    }
    Ok(Some(dashes.into()))
}

/// Comma separated, as Chrome writes a dash array.
pub(crate) fn dasharray_to_css(dashes: Option<&[LengthPercentage]>, dest: &mut String) {
    let Some(dashes) = dashes else {
        dest.push_str("none");
        return;
    };
    for (i, dash) in dashes.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        dash.to_css(dest);
    }
}

/// `d` in CSS: `none` (`None`), or `path("…")` with a whole, valid path;
/// an empty one is `none`.
pub(crate) fn parse_d<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Option<Arc<[PathSegment]>>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(None);
    }
    input.expect_function_matching("path")?;
    let data = input.parse_nested_block(|i| Ok(i.expect_string()?.as_ref().to_owned()))?;
    match path_data(&data, true) {
        Some(segments) => Ok((!segments.is_empty()).then(|| segments.into())),
        None => Err(input.new_custom_error(())),
    }
}

/// The `d` attribute: path data as far as it is valid (SVG 2's "error
/// handling in path data": everything up to the first error is drawn);
/// `None` where nothing is.
pub fn parse_d_attribute(data: &str) -> Option<Arc<[PathSegment]>> {
    let segments = path_data(data, false)?;
    (!segments.is_empty()).then(|| segments.into())
}

/// The segments of path data; `None` if `strict` and some of it isn't
/// valid.
pub(crate) fn path_data(data: &str, strict: bool) -> Option<Vec<PathSegment>> {
    let mut segments = Vec::new();
    for segment in svgtypes::PathParser::from(data) {
        match segment {
            Ok(segment) => segments.push(path_segment(segment)),
            Err(_) if strict => return None,
            Err(_) => break,
        }
    }
    Some(segments)
}

fn path_segment(segment: svgtypes::PathSegment) -> PathSegment {
    use svgtypes::PathSegment as S;
    let f = f32_from_f64;
    match segment {
        S::MoveTo { abs, x, y } => PathSegment::MoveTo {
            abs,
            x: f(x),
            y: f(y),
        },
        S::LineTo { abs, x, y } => PathSegment::LineTo {
            abs,
            x: f(x),
            y: f(y),
        },
        S::HorizontalLineTo { abs, x } => PathSegment::HorizontalLineTo { abs, x: f(x) },
        S::VerticalLineTo { abs, y } => PathSegment::VerticalLineTo { abs, y: f(y) },
        S::CurveTo {
            abs,
            x1,
            y1,
            x2,
            y2,
            x,
            y,
        } => PathSegment::CurveTo {
            abs,
            x1: f(x1),
            y1: f(y1),
            x2: f(x2),
            y2: f(y2),
            x: f(x),
            y: f(y),
        },
        S::SmoothCurveTo { abs, x2, y2, x, y } => PathSegment::SmoothCurveTo {
            abs,
            x2: f(x2),
            y2: f(y2),
            x: f(x),
            y: f(y),
        },
        S::Quadratic { abs, x1, y1, x, y } => PathSegment::Quadratic {
            abs,
            x1: f(x1),
            y1: f(y1),
            x: f(x),
            y: f(y),
        },
        S::SmoothQuadratic { abs, x, y } => PathSegment::SmoothQuadratic {
            abs,
            x: f(x),
            y: f(y),
        },
        S::EllipticalArc {
            abs,
            rx,
            ry,
            x_axis_rotation,
            large_arc,
            sweep,
            x,
            y,
        } => PathSegment::EllipticalArc {
            abs,
            rx: f(rx),
            ry: f(ry),
            x_axis_rotation: f(x_axis_rotation),
            large_arc,
            sweep,
            x: f(x),
            y: f(y),
        },
        S::ClosePath { .. } => PathSegment::ClosePath,
    }
}

/// `none` or `path("…")`, each command's letter and numbers apart, as
/// Chrome writes path data (`BuildStringFromByteStream`).
pub(crate) fn d_to_css(segments: Option<&[PathSegment]>, dest: &mut String) {
    let Some(segments) = segments else {
        dest.push_str("none");
        return;
    };
    dest.push_str("path(");
    path_data_to_css(segments, dest);
    dest.push(')');
}

/// Path data as a CSS string, each command's letter and numbers spaced
/// (Blink's `BuildStringFromByteStream`): `path()`'s argument.
pub(crate) fn path_data_to_css(segments: &[PathSegment], dest: &mut String) {
    let mut data = String::new();
    for (i, &segment) in segments.iter().enumerate() {
        if i > 0 {
            data.push(' ');
        }
        let letter = |upper: char| {
            if segment_is_abs(segment) {
                upper
            } else {
                upper.to_ascii_lowercase()
            }
        };
        let (command, numbers): (char, Vec<f32>) = match segment {
            PathSegment::MoveTo { x, y, .. } => (letter('M'), vec![x, y]),
            PathSegment::LineTo { x, y, .. } => (letter('L'), vec![x, y]),
            PathSegment::HorizontalLineTo { x, .. } => (letter('H'), vec![x]),
            PathSegment::VerticalLineTo { y, .. } => (letter('V'), vec![y]),
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
                ..
            } => (letter('C'), vec![x1, y1, x2, y2, x, y]),
            PathSegment::SmoothCurveTo { x2, y2, x, y, .. } => (letter('S'), vec![x2, y2, x, y]),
            PathSegment::Quadratic { x1, y1, x, y, .. } => (letter('Q'), vec![x1, y1, x, y]),
            PathSegment::SmoothQuadratic { x, y, .. } => (letter('T'), vec![x, y]),
            PathSegment::EllipticalArc {
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
                ..
            } => (
                letter('A'),
                vec![
                    rx,
                    ry,
                    x_axis_rotation,
                    f32::from(u8::from(large_arc)),
                    f32::from(u8::from(sweep)),
                    x,
                    y,
                ],
            ),
            PathSegment::ClosePath => ('Z', vec![]),
        };
        data.push(command);
        for number in numbers {
            data.push(' ');
            write_number(&mut data, number.into());
        }
    }
    cssparser::serialize_string(&data, dest).expect("writing to a String");
}

/// The `transform` attribute (and `gradientTransform`,
/// `patternTransform`): SVG's transform list, which takes plain numbers
/// and `rotate(a cx cy)`, as one matrix (Blink's `SVGTransformList`
/// consolidated); none for an empty list. `None` if any of it isn't
/// valid, which drops the whole attribute, as Chrome does.
pub fn parse_transform_attribute(text: &str) -> Option<Arc<[TransformFunction]>> {
    use svgtypes::TransformListToken as T;
    let mut matrix: Option<[f64; 6]> = None;
    for token in svgtypes::TransformListParser::from(text) {
        let next = match token.ok()? {
            T::Matrix { a, b, c, d, e, f } => [a, b, c, d, e, f],
            T::Translate { tx, ty } => [1.0, 0.0, 0.0, 1.0, tx, ty],
            T::Scale { sx, sy } => [sx, 0.0, 0.0, sy, 0.0, 0.0],
            T::Rotate { angle } => {
                let (sin, cos) = gfx::sin_cos_degrees(angle);
                [cos, sin, -sin, cos, 0.0, 0.0]
            }
            T::SkewX { angle } => [1.0, 0.0, angle.to_radians().tan(), 1.0, 0.0, 0.0],
            T::SkewY { angle } => [1.0, angle.to_radians().tan(), 0.0, 1.0, 0.0, 0.0],
        };
        // Each function applies inside the ones before it.
        matrix = Some(match matrix {
            None => next,
            Some([a, b, c, d, e, f]) => {
                let [a2, b2, c2, d2, e2, f2] = next;
                [
                    a * a2 + c * b2,
                    b * a2 + d * b2,
                    a * c2 + c * d2,
                    b * c2 + d * d2,
                    a * e2 + c * f2 + e,
                    b * e2 + d * f2 + f,
                ]
            }
        });
    }
    Some(match matrix {
        None => Arc::new([]),
        Some(matrix) => Arc::new([TransformFunction::Matrix(matrix.map(f32_from_f64))]),
    })
}

/// Path data as a path (Blink's `BuildPathFromString`, which a `Path2D`
/// is built from as SVG's `d` is): what comes before the first error, if
/// any; empty for none.
pub fn path_from_string(data: &str) -> Path {
    let mut path = Path::new();
    if let Some(segments) = parse_d_attribute(data) {
        computed_style::append_path_data(&mut path, &absolute_path(&segments));
    }
    path
}

/// The id a reference to an element of the same document names (`<use>`'s
/// `href="#id"`, a paint's `url(#id)`): the fragment, percent-decoded (as
/// UTF-8, lossily), as Blink's `SVGURIReference::FragmentIdentifierFrom
/// IRIString` decodes it; `None` for one to another document.
pub fn fragment_id(url: &str) -> Option<String> {
    let fragment = url.trim().strip_prefix('#')?;
    let mut rest = fragment.as_bytes();
    let mut out = Vec::with_capacity(rest.len());
    while let Some((&first, after)) = rest.split_first() {
        let decoded = (first == b'%')
            .then(|| after.split_first_chunk::<2>())
            .flatten()
            .and_then(|(hex, after)| {
                let hex = std::str::from_utf8(hex).ok()?;
                Some((u8::from_str_radix(hex, 16).ok()?, after))
            });
        match decoded {
            Some((byte, after)) => {
                out.push(byte);
                rest = after;
            }
            None => {
                out.push(first);
                rest = after;
            }
        }
    }
    Some(String::from_utf8_lossy(&out).into_owned())
}
