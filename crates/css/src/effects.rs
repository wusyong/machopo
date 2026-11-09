//! `transform` functions (2D only: `translate`, `scale`, `rotate`, `skew`,
//! `matrix` and their single-axis forms), `transform-origin` and
//! `transform-box`.

use std::sync::Arc;

use computed_style::TransformOperation;
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::background::Angle;
use crate::values::parse_number;
use crate::values::{ComputeContext, Length, LengthPercentage, ParseResult, write_number};

/// One specified transform function as written: lengths may still be in
/// `em`, and the single-axis forms and omitted arguments are kept for
/// serialization.
#[derive(Debug, Clone, PartialEq)]
pub enum TransformFunction {
    /// `translate(x)` (y is 0) or `translate(x, y)`.
    Translate(LengthPercentage, Option<LengthPercentage>),
    TranslateX(LengthPercentage),
    TranslateY(LengthPercentage),
    /// `scale(s)` (both axes) or `scale(x, y)`.
    Scale(f32, Option<f32>),
    ScaleX(f32),
    ScaleY(f32),
    /// `rotate(a)`, or `rotateZ(a)` (`true`), which is the same rotation.
    Rotate(Angle, bool),
    /// `skew(x)` (y is 0) or `skew(x, y)`.
    Skew(Angle, Option<Angle>),
    SkewX(Angle),
    SkewY(Angle),
    Matrix([f32; 6]),
}

const ZERO: LengthPercentage = LengthPercentage::Percent(0.0);

/// `none | <transform-function>+`.
pub(crate) fn parse_transform<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[TransformFunction]>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::new([]));
    }
    let mut functions = vec![parse_function(input)?];
    while let Ok(function) = input.try_parse(parse_function) {
        functions.push(function);
    }
    Ok(functions.into())
}

fn parse_function<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, TransformFunction> {
    let location = input.current_source_location();
    let name = match input.next()? {
        Token::Function(name) => name.clone(),
        _ => return Err(location.new_custom_error(())),
    };
    input.parse_nested_block(|input| {
        let lp = |i: &mut Parser<'i, '_>| LengthPercentage::parse(i, true);
        let comma_then = |i: &mut Parser<'i, '_>| i.try_parse(|i| i.expect_comma()).is_ok();
        match_ignore_ascii_case! { &name,
            "translate" => {
                let x = lp(input)?;
                let y = if comma_then(input) { Some(lp(input)?) } else { None };
                Ok(TransformFunction::Translate(x, y))
            },
            "translatex" => Ok(TransformFunction::TranslateX(lp(input)?)),
            "translatey" => Ok(TransformFunction::TranslateY(lp(input)?)),
            "scale" => {
                let x = parse_number(input)?;
                let y = if comma_then(input) { Some(parse_number(input)?) } else { None };
                Ok(TransformFunction::Scale(x, y))
            },
            "scalex" => Ok(TransformFunction::ScaleX(parse_number(input)?)),
            "scaley" => Ok(TransformFunction::ScaleY(parse_number(input)?)),
            "rotate" => Ok(TransformFunction::Rotate(Angle::parse_or_zero(input)?, false)),
            "rotatez" => Ok(TransformFunction::Rotate(Angle::parse_or_zero(input)?, true)),
            "skew" => {
                let x = Angle::parse_or_zero(input)?;
                let y = if comma_then(input) { Some(Angle::parse_or_zero(input)?) } else { None };
                Ok(TransformFunction::Skew(x, y))
            },
            "skewx" => Ok(TransformFunction::SkewX(Angle::parse_or_zero(input)?)),
            "skewy" => Ok(TransformFunction::SkewY(Angle::parse_or_zero(input)?)),
            "matrix" => {
                let mut m = [0.0; 6];
                for (i, value) in m.iter_mut().enumerate() {
                    if i > 0 {
                        input.expect_comma()?;
                    }
                    *value = parse_number(input)?;
                }
                Ok(TransformFunction::Matrix(m))
            },
            _ => Err(input.new_custom_error(())),
        }
    })
}

pub(crate) fn compute_transform(
    functions: &[TransformFunction],
    cx: &ComputeContext,
) -> Arc<[TransformOperation]> {
    functions
        .iter()
        .map(|f| {
            let translate = |x: &LengthPercentage, y: &LengthPercentage| {
                TransformOperation::Translate(x.compute(cx), y.compute(cx))
            };
            match f {
                TransformFunction::Translate(x, y) => translate(x, y.as_ref().unwrap_or(&ZERO)),
                TransformFunction::TranslateX(x) => translate(x, &ZERO),
                TransformFunction::TranslateY(y) => translate(&ZERO, y),
                TransformFunction::Scale(x, y) => TransformOperation::Scale(*x, y.unwrap_or(*x)),
                TransformFunction::ScaleX(x) => TransformOperation::Scale(*x, 1.0),
                TransformFunction::ScaleY(y) => TransformOperation::Scale(1.0, *y),
                TransformFunction::Rotate(a, _) => TransformOperation::Rotate(a.degrees()),
                TransformFunction::Skew(x, y) => {
                    TransformOperation::Skew(x.degrees(), y.map_or(0.0, Angle::degrees))
                }
                TransformFunction::SkewX(x) => TransformOperation::SkewX(x.degrees()),
                TransformFunction::SkewY(y) => TransformOperation::SkewY(y.degrees()),
                TransformFunction::Matrix(m) => TransformOperation::Matrix(*m),
            }
        })
        .collect()
}

/// `none`, or the functions separated by spaces, with Chrome's names
/// (`translateX`).
pub(crate) fn transform_to_css(functions: &[TransformFunction], dest: &mut String) {
    if functions.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, function) in functions.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        let number = |dest: &mut String, v: f32| write_number(dest, v.into());
        match function {
            TransformFunction::Translate(x, y) => {
                dest.push_str("translate(");
                x.to_css(dest);
                if let Some(y) = y {
                    dest.push_str(", ");
                    y.to_css(dest);
                }
            }
            TransformFunction::TranslateX(x) => {
                dest.push_str("translateX(");
                x.to_css(dest);
            }
            TransformFunction::TranslateY(y) => {
                dest.push_str("translateY(");
                y.to_css(dest);
            }
            TransformFunction::Scale(x, y) => {
                dest.push_str("scale(");
                number(dest, *x);
                if let Some(y) = y {
                    dest.push_str(", ");
                    number(dest, *y);
                }
            }
            TransformFunction::ScaleX(x) => {
                dest.push_str("scaleX(");
                number(dest, *x);
            }
            TransformFunction::ScaleY(y) => {
                dest.push_str("scaleY(");
                number(dest, *y);
            }
            TransformFunction::Rotate(a, z) => {
                dest.push_str(if *z { "rotateZ(" } else { "rotate(" });
                a.to_css(dest);
            }
            TransformFunction::Skew(x, y) => {
                dest.push_str("skew(");
                x.to_css(dest);
                if let Some(y) = y {
                    dest.push_str(", ");
                    y.to_css(dest);
                }
            }
            TransformFunction::SkewX(x) => {
                dest.push_str("skewX(");
                x.to_css(dest);
            }
            TransformFunction::SkewY(y) => {
                dest.push_str("skewY(");
                y.to_css(dest);
            }
            TransformFunction::Matrix(m) => {
                dest.push_str("matrix(");
                for (i, v) in m.iter().copied().enumerate() {
                    if i > 0 {
                        dest.push_str(", ");
                    }
                    number(dest, v);
                }
            }
        }
        dest.push(')');
    }
}

/// One axis of `transform-origin` as written: a keyword or a length.
#[derive(Debug, Clone, PartialEq)]
pub enum OriginComponent {
    Left,
    Center,
    Right,
    Top,
    Bottom,
    Length(LengthPercentage),
}

impl OriginComponent {
    fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if let Ok(length) = input.try_parse(|i| LengthPercentage::parse(i, true)) {
            return Ok(Self::Length(length));
        }
        let location = input.current_source_location();
        let ident = input.expect_ident()?;
        Ok(match_ignore_ascii_case! { ident,
            "left" => Self::Left,
            "center" => Self::Center,
            "right" => Self::Right,
            "top" => Self::Top,
            "bottom" => Self::Bottom,
            _ => return Err(location.new_custom_error(())),
        })
    }

    fn is_vertical_keyword(&self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    fn is_horizontal_keyword(&self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    fn compute(&self, cx: &ComputeContext) -> computed_style::LengthPercentage {
        use computed_style::LengthPercentage::Percent;
        match self {
            Self::Left | Self::Top => Percent(0.0),
            Self::Center => Percent(50.0),
            Self::Right | Self::Bottom => Percent(100.0),
            Self::Length(length) => length.compute(cx),
        }
    }

    fn to_css(&self, dest: &mut String) {
        match self {
            Self::Left => dest.push_str("left"),
            Self::Center => dest.push_str("center"),
            Self::Right => dest.push_str("right"),
            Self::Top => dest.push_str("top"),
            Self::Bottom => dest.push_str("bottom"),
            Self::Length(length) => length.to_css(dest),
        }
    }
}

/// `transform-origin` as written, the keywords put on their axes (`top
/// left` is `x: left, y: top`; a single value's other axis is `center`).
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedTransformOrigin {
    pub x: OriginComponent,
    pub y: OriginComponent,
    pub z: Option<Length>,
}

/// css-transforms 1's grammar: one value; or a horizontal value and a
/// vertical one, the keywords in either order but a length only where
/// its axis is clear, and then a depth.
pub(crate) fn parse_transform_origin<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, SpecifiedTransformOrigin> {
    let first = OriginComponent::parse(input)?;
    let Ok(second) = input.try_parse(OriginComponent::parse) else {
        let (x, y) = if first.is_vertical_keyword() {
            (OriginComponent::Center, first)
        } else {
            (first, OriginComponent::Center)
        };
        return Ok(SpecifiedTransformOrigin { x, y, z: None });
    };
    let location = input.current_source_location();
    let keyword = |c: &OriginComponent| !matches!(c, OriginComponent::Length(_));
    let (x, y) = if first.is_vertical_keyword()
        || (first == OriginComponent::Center && second.is_horizontal_keyword())
    {
        // `top left`, `center left`: the second is the horizontal one,
        // and only a keyword may follow a vertical keyword.
        if !keyword(&second) || second.is_vertical_keyword() {
            return Err(location.new_custom_error(()));
        }
        (second, first)
    } else {
        if second.is_horizontal_keyword() {
            return Err(location.new_custom_error(()));
        }
        (first, second)
    };
    let z = input.try_parse(|i| Length::parse(i, true)).ok();
    Ok(SpecifiedTransformOrigin { x, y, z })
}

pub(crate) fn compute_transform_origin(
    origin: &SpecifiedTransformOrigin,
    cx: &ComputeContext,
) -> computed_style::TransformOrigin {
    computed_style::TransformOrigin {
        x: origin.x.compute(cx),
        y: origin.y.compute(cx),
        z: origin.z.as_ref().map_or(0.0, |z| z.to_px(cx)),
    }
}

/// The horizontal value, the vertical one, and the depth if written, as
/// Chrome writes them.
pub(crate) fn transform_origin_to_css(origin: &SpecifiedTransformOrigin, dest: &mut String) {
    origin.x.to_css(dest);
    dest.push(' ');
    origin.y.to_css(dest);
    if let Some(z) = &origin.z {
        dest.push(' ');
        z.to_css(dest);
    }
}

pub(crate) fn parse_transform_box<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, computed_style::TransformBox> {
    use computed_style::TransformBox;
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "content-box" => TransformBox::ContentBox,
        "border-box" => TransformBox::BorderBox,
        "fill-box" => TransformBox::FillBox,
        "stroke-box" => TransformBox::StrokeBox,
        "view-box" => TransformBox::ViewBox,
        _ => return Err(location.new_custom_error(())),
    })
}

pub(crate) fn transform_box_name(value: computed_style::TransformBox) -> &'static str {
    use computed_style::TransformBox;
    match value {
        TransformBox::ContentBox => "content-box",
        TransformBox::BorderBox => "border-box",
        TransformBox::FillBox => "fill-box",
        TransformBox::StrokeBox => "stroke-box",
        TransformBox::ViewBox => "view-box",
    }
}
