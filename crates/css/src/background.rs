//! The background properties (css-backgrounds 3) and the images they take
//! (css-images 3: `url()`, `linear-gradient()`, `radial-gradient()` and
//! their `repeating-` forms; css-images 4's `image-set()`): parsing,
//! computing and serializing. Each
//! longhand is a comma-separated list, one item per layer (Blink's
//! `FillLayer`).

use std::sync::Arc;

use computed_style::{
    BackgroundAttachment, BackgroundBox, BackgroundImage, BackgroundRepeat, BackgroundSize,
    ComputedStyle, Gradient, GradientItem, GradientKind, HorizontalSide, ImageSetOption, ImageUrl,
    LengthPercentage as ComputedLengthPercentage, LengthPercentageOrAuto, LinearDirection,
    Position2D, PositionComponent, RadialShape, RadialSize, VerticalSide,
};
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::css_url_data::CssUrlData;
use crate::values::{
    ComputeContext, Length, LengthPercentage, ParseResult, SpecifiedColor, parse_style_color,
    write_number, write_rgba,
};

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

/// Comma-separated items of one layer each.
pub(crate) fn parse_list<'i, T>(
    input: &mut Parser<'i, '_>,
    item: impl for<'t> FnMut(&mut Parser<'i, 't>) -> ParseResult<'i, T>,
) -> ParseResult<'i, Arc<[T]>> {
    input.parse_comma_separated(item).map(Into::into)
}

fn list_to_css<T>(list: &[T], dest: &mut String, mut item: impl FnMut(&T, &mut String)) {
    for (i, value) in list.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        item(value, dest);
    }
}

// --- <image> --------------------------------------------------------------

/// One layer's `background-image` as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedImage {
    None,
    Url(CssUrlData),
    Gradient(Arc<SpecifiedGradient>),
    Set(Arc<[SpecifiedImageSetOption]>),
}

/// One option of an `image-set()` as written.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedImageSetOption {
    /// A `Url` (a plain string is one) or a `Gradient`.
    image: SpecifiedImage,
    resolution: Resolution,
    mime_type: Option<Arc<str>>,
}

/// A `<resolution>` as written: `calc()`'s (and the other math
/// functions') in dppx, which CSSOM writes as `calc(<n>dppx)` (Chrome 152
/// writes `calc(1x + 1x)` as `calc(2dppx)`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resolution {
    value: f32,
    unit: ResolutionUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolutionUnit {
    X,
    Dppx,
    Dpi,
    Dpcm,
    Calc,
}

impl Resolution {
    /// Omitted, an option's resolution is `1x`.
    const ONE: Self = Self {
        value: 1.0,
        unit: ResolutionUnit::X,
    };

    /// Not negative.
    fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if let Ok(value) = input.try_parse(crate::calc::parse_resolution) {
            return Ok(Self {
                value,
                unit: ResolutionUnit::Calc,
            });
        }
        let location = input.current_source_location();
        let Token::Dimension { value, unit, .. } = input.next()? else {
            return Err(location.new_custom_error(()));
        };
        let unit = match_ignore_ascii_case! { unit,
            "x" => ResolutionUnit::X,
            "dppx" => ResolutionUnit::Dppx,
            "dpi" => ResolutionUnit::Dpi,
            "dpcm" => ResolutionUnit::Dpcm,
            _ => return Err(location.new_custom_error(())),
        };
        if *value < 0.0 {
            return Err(location.new_custom_error(()));
        }
        Ok(Self {
            value: *value,
            unit,
        })
    }

    /// In dppx; a math function's negative one is 0 (Chrome 152 writes
    /// `calc(-1x)` as `calc(-1dppx)` and computes it to `0dppx`).
    fn dppx(self) -> f32 {
        let unit = match self.unit {
            ResolutionUnit::X | ResolutionUnit::Calc => "x",
            ResolutionUnit::Dppx => "dppx",
            ResolutionUnit::Dpi => "dpi",
            ResolutionUnit::Dpcm => "dpcm",
        };
        crate::calc::resolution_dppx(self.value, unit)
            .expect("a resolution unit")
            .max(0.0)
    }

    fn to_css(self, dest: &mut String) {
        let unit = match self.unit {
            ResolutionUnit::X => "x",
            ResolutionUnit::Dppx => "dppx",
            ResolutionUnit::Dpi => "dpi",
            ResolutionUnit::Dpcm => "dpcm",
            ResolutionUnit::Calc => {
                dest.push_str("calc(");
                write_number(dest, self.value.into());
                dest.push_str("dppx)");
                return;
            }
        };
        write_number(dest, self.value.into());
        dest.push_str(unit);
    }
}

/// `none | <image>`: `url()` (not a plain string), a gradient or an
/// `image-set()`.
pub(crate) fn parse_image<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedImage> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(SpecifiedImage::None);
    }
    parse_image_value(input, true)
}

/// An `<image>`: an `image-set()` only if `set` (one can't hold another).
pub(crate) fn parse_image_value<'i>(
    input: &mut Parser<'i, '_>,
    set: bool,
) -> ParseResult<'i, SpecifiedImage> {
    let location = input.current_source_location();
    let token = input.next()?.clone();
    match token {
        Token::UnquotedUrl(url) => Ok(SpecifiedImage::Url(CssUrlData::new(&url))),
        Token::Function(name) if name.eq_ignore_ascii_case("url") => input
            .parse_nested_block(|i| Ok(CssUrlData::new(&i.expect_string()?.clone())))
            .map(SpecifiedImage::Url),
        // `-webkit-image-set()` is the same, as in Chrome.
        Token::Function(name)
            if set
                && (name.eq_ignore_ascii_case("image-set")
                    || name.eq_ignore_ascii_case("-webkit-image-set")) =>
        {
            input
                .parse_nested_block(|i| i.parse_comma_separated(parse_image_set_option))
                .map(|options| SpecifiedImage::Set(options.into()))
        }
        Token::Function(name) => {
            let (repeating, radial) = match_ignore_ascii_case! { &name,
                "linear-gradient" => (false, false),
                "repeating-linear-gradient" => (true, false),
                "radial-gradient" => (false, true),
                "repeating-radial-gradient" => (true, true),
                _ => return Err(location.new_custom_error::<(), ()>(())),
            };
            input
                .parse_nested_block(|i| SpecifiedGradient::parse(i, repeating, radial))
                .map(|gradient| SpecifiedImage::Gradient(Arc::new(gradient)))
        }
        _ => Err(location.new_custom_error::<(), ()>(())),
    }
}

/// `<image> | <string>`, then a resolution and `type(<string>)`, each at
/// most once, in either order.
fn parse_image_set_option<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, SpecifiedImageSetOption> {
    let image = match input.try_parse(|i| i.expect_string().cloned()) {
        Ok(url) => SpecifiedImage::Url(CssUrlData::new(&url)),
        Err(_) => parse_image_value(input, false)?,
    };
    let mut resolution = None;
    let mut mime_type = None;
    while !input.is_exhausted() {
        if resolution.is_none()
            && let Ok(r) = input.try_parse(Resolution::parse)
        {
            resolution = Some(r);
            continue;
        }
        if mime_type.is_none()
            && let Ok(t) = input.try_parse(|i| -> ParseResult<'i, Arc<str>> {
                i.expect_function_matching("type")?;
                i.parse_nested_block(|i| Ok(i.expect_string()?.as_ref().into()))
            })
        {
            mime_type = Some(t);
            continue;
        }
        return invalid(input);
    }
    Ok(SpecifiedImageSetOption {
        image,
        resolution: resolution.unwrap_or(Resolution::ONE),
        mime_type,
    })
}

impl SpecifiedImage {
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> BackgroundImage {
        match self {
            Self::None => BackgroundImage::None,
            Self::Url(url) => BackgroundImage::Url(ImageUrl {
                url: url.computed(),
                referrer: url.referrer().cloned(),
            }),
            Self::Gradient(gradient) => BackgroundImage::Gradient(Arc::new(gradient.compute(cx))),
            Self::Set(options) => BackgroundImage::Set(
                options
                    .iter()
                    .map(|option| ImageSetOption {
                        image: option.image.compute(cx),
                        resolution: option.resolution.dppx(),
                        mime_type: option.mime_type.clone(),
                    })
                    .collect(),
            ),
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::None => dest.push_str("none"),
            Self::Url(url) => url_to_css(url.specified(), dest),
            Self::Gradient(gradient) => gradient.to_css(dest),
            Self::Set(options) => {
                dest.push_str("image-set(");
                list_to_css(options, dest, |option, dest| {
                    option.image.to_css(dest);
                    dest.push(' ');
                    option.resolution.to_css(dest);
                    if let Some(mime_type) = &option.mime_type {
                        type_to_css(mime_type, dest);
                    }
                });
                dest.push(')');
            }
        }
    }

    /// Its `url()`s, an `image-set()`'s too.
    pub(crate) fn urls_mut(&mut self) -> Vec<&mut CssUrlData> {
        match self {
            Self::Url(url) => vec![url],
            Self::Set(options) => Arc::make_mut(options)
                .iter_mut()
                .flat_map(|option| option.image.urls_mut())
                .collect(),
            Self::None | Self::Gradient(_) => Vec::new(),
        }
    }

    /// Whether it has a `url()` not resolved yet.
    pub(crate) fn has_unresolved_urls(&self) -> bool {
        match self {
            Self::Url(url) => !url.is_resolved(),
            Self::Set(options) => options.iter().any(|o| o.image.has_unresolved_urls()),
            Self::None | Self::Gradient(_) => false,
        }
    }

    /// Whether it is a `url()`, or an `image-set()` of them only (what
    /// `cursor` takes).
    pub(crate) fn is_urls(&self) -> bool {
        match self {
            Self::Url(_) => true,
            Self::Set(options) => options
                .iter()
                .all(|option| matches!(option.image, Self::Url(_))),
            Self::None | Self::Gradient(_) => false,
        }
    }

    /// An image of `url()`s only ([`SpecifiedImage::is_urls`]) computed,
    /// each URL by `url`.
    pub(crate) fn compute_urls(&self, url: &impl Fn(&CssUrlData) -> Arc<str>) -> BackgroundImage {
        match self {
            Self::Url(data) => BackgroundImage::Url(ImageUrl {
                url: url(data),
                referrer: data.referrer().cloned(),
            }),
            Self::Set(options) => BackgroundImage::Set(
                options
                    .iter()
                    .map(|option| ImageSetOption {
                        image: option.image.compute_urls(url),
                        resolution: option.resolution.dppx(),
                        mime_type: option.mime_type.clone(),
                    })
                    .collect(),
            ),
            Self::None | Self::Gradient(_) => unreachable!("an image of URLs"),
        }
    }

    /// Itself with its `url()`s as if not resolved yet.
    pub(crate) fn as_written(&self) -> Self {
        let mut image = self.clone();
        for url in image.urls_mut() {
            *url = url.as_written();
        }
        image
    }
}

/// ` type("…")`.
fn type_to_css(mime_type: &str, dest: &mut String) {
    dest.push_str(" type(");
    cssparser::serialize_string(mime_type, dest).expect("writing to a String");
    dest.push(')');
}

/// `url("…")`, as Chrome writes it.
pub(crate) fn url_to_css(url: &str, dest: &mut String) {
    dest.push_str("url(");
    cssparser::serialize_string(url, dest).expect("writing to a String");
    dest.push(')');
}

/// An `<angle>` as written.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Angle {
    value: f32,
    unit: AngleUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AngleUnit {
    Deg,
    Grad,
    Rad,
    Turn,
}

impl Angle {
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        // A math function's angle is in degrees.
        if let Ok(value) = input.try_parse(crate::calc::parse_angle) {
            return Ok(Self {
                value,
                unit: AngleUnit::Deg,
            });
        }
        let location = input.current_source_location();
        match *input.next()? {
            Token::Dimension {
                value, ref unit, ..
            } => {
                let unit = match_ignore_ascii_case! { unit,
                    "deg" => AngleUnit::Deg,
                    "grad" => AngleUnit::Grad,
                    "rad" => AngleUnit::Rad,
                    "turn" => AngleUnit::Turn,
                    _ => return Err(location.new_custom_error::<(), ()>(())),
                };
                Ok(Self { value, unit })
            }
            _ => Err(location.new_custom_error::<(), ()>(())),
        }
    }

    /// An `<angle>` or a unitless `0`, which the `transform` functions
    /// take (css-transforms 1's `<zero>`), as degrees.
    pub(crate) fn parse_or_zero<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if input
            .try_parse(|i| match *i.next()? {
                Token::Number { value: 0.0, .. } => Ok(()),
                _ => Err(i.new_custom_error::<(), ()>(())),
            })
            .is_ok()
        {
            return Ok(Self {
                value: 0.0,
                unit: AngleUnit::Deg,
            });
        }
        Self::parse(input)
    }

    pub(crate) fn degrees(self) -> f32 {
        match self.unit {
            AngleUnit::Deg => self.value,
            AngleUnit::Grad => self.value * 0.9,
            AngleUnit::Rad => self.value.to_degrees(),
            AngleUnit::Turn => self.value * 360.0,
        }
    }

    pub(crate) fn to_css(self, dest: &mut String) {
        write_number(dest, self.value.into());
        dest.push_str(match self.unit {
            AngleUnit::Deg => "deg",
            AngleUnit::Grad => "grad",
            AngleUnit::Rad => "rad",
            AngleUnit::Turn => "turn",
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SpecifiedDirection {
    Angle(Angle),
    To(Option<HorizontalSide>, Option<VerticalSide>),
}

/// A radial gradient's size as written.
#[derive(Debug, Clone, PartialEq)]
enum SpecifiedRadialSize {
    /// An extent keyword: the computed `RadialSize` of the same name.
    Extent(RadialSize),
    Circle(Length),
    Ellipse(LengthPercentage, LengthPercentage),
}

#[derive(Debug, Clone, PartialEq)]
enum SpecifiedGradientKind {
    Linear(Option<SpecifiedDirection>),
    Radial {
        shape: Option<RadialShape>,
        size: Option<SpecifiedRadialSize>,
        position: Option<SpecifiedPosition>,
    },
}

#[derive(Debug, Clone, PartialEq)]
enum SpecifiedGradientItem {
    Stop(SpecifiedColor, Option<LengthPercentage>),
    Hint(LengthPercentage),
}

/// A gradient function as written.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedGradient {
    kind: SpecifiedGradientKind,
    repeating: bool,
    items: Vec<SpecifiedGradientItem>,
}

impl SpecifiedGradient {
    /// The inside of the function.
    fn parse<'i>(
        input: &mut Parser<'i, '_>,
        repeating: bool,
        radial: bool,
    ) -> ParseResult<'i, Self> {
        let kind = if radial {
            let (mut shape, mut size) = (None, None);
            loop {
                if shape.is_none()
                    && let Ok(s) = input.try_parse(|i| {
                        let location = i.current_source_location();
                        let ident = i.expect_ident()?;
                        Ok(match_ignore_ascii_case! { ident,
                            "circle" => RadialShape::Circle,
                            "ellipse" => RadialShape::Ellipse,
                            _ => return Err(location.new_custom_error::<(), ()>(())),
                        })
                    })
                {
                    shape = Some(s);
                    continue;
                }
                if size.is_none()
                    && let Ok(s) = input.try_parse(parse_radial_size)
                {
                    size = Some(s);
                    continue;
                }
                break;
            }
            // An explicit size goes with its shape: one length is a
            // circle's, two an ellipse's.
            match (shape, &size) {
                (Some(RadialShape::Circle), Some(SpecifiedRadialSize::Ellipse(..)))
                | (Some(RadialShape::Ellipse), Some(SpecifiedRadialSize::Circle(_))) => {
                    return invalid(input);
                }
                _ => {}
            }
            let position = if input.try_parse(|i| i.expect_ident_matching("at")).is_ok() {
                Some(SpecifiedPosition::parse(input)?)
            } else {
                None
            };
            if shape.is_some() || size.is_some() || position.is_some() {
                input.expect_comma()?;
            }
            SpecifiedGradientKind::Radial {
                shape,
                size,
                position,
            }
        } else {
            let direction = if let Ok(angle) = input.try_parse(Angle::parse) {
                Some(SpecifiedDirection::Angle(angle))
            } else if input.try_parse(|i| i.expect_ident_matching("to")).is_ok() {
                Some(parse_side_or_corner(input)?)
            } else {
                None
            };
            if direction.is_some() {
                input.expect_comma()?;
            }
            SpecifiedGradientKind::Linear(direction)
        };
        let items = parse_color_stop_list(input)?;
        Ok(Self {
            kind,
            repeating,
            items,
        })
    }

    fn compute(&self, cx: &ComputeContext<'_>) -> Gradient {
        let kind = match &self.kind {
            SpecifiedGradientKind::Linear(direction) => GradientKind::Linear(match direction {
                None => LinearDirection::To(None, Some(VerticalSide::Bottom)),
                Some(SpecifiedDirection::Angle(angle)) => LinearDirection::Angle(angle.degrees()),
                Some(SpecifiedDirection::To(x, y)) => LinearDirection::To(*x, *y),
            }),
            SpecifiedGradientKind::Radial {
                shape,
                size,
                position,
            } => {
                let size = match size {
                    None => RadialSize::FarthestCorner,
                    Some(SpecifiedRadialSize::Extent(extent)) => *extent,
                    Some(SpecifiedRadialSize::Circle(length)) => {
                        RadialSize::Circle(length.to_px(cx))
                    }
                    Some(SpecifiedRadialSize::Ellipse(x, y)) => {
                        RadialSize::Ellipse(x.compute(cx), y.compute(cx))
                    }
                };
                let shape = shape.unwrap_or(match size {
                    RadialSize::Circle(_) => RadialShape::Circle,
                    _ => RadialShape::Ellipse,
                });
                GradientKind::Radial {
                    shape,
                    size,
                    position: position
                        .as_ref()
                        .map_or(Position2D::CENTER, |position| position.compute(cx)),
                }
            }
        };
        Gradient {
            kind,
            repeating: self.repeating,
            items: self
                .items
                .iter()
                .map(|item| match item {
                    SpecifiedGradientItem::Stop(color, position) => GradientItem::Stop(
                        color.compute(cx),
                        position.as_ref().map(|p| p.compute(cx)),
                    ),
                    SpecifiedGradientItem::Hint(position) => {
                        GradientItem::Hint(position.compute(cx))
                    }
                })
                .collect(),
        }
    }

    fn to_css(&self, dest: &mut String) {
        let radial = matches!(self.kind, SpecifiedGradientKind::Radial { .. });
        dest.push_str(gradient_name(self.repeating, radial));
        dest.push('(');
        let mut prelude = Vec::new();
        match &self.kind {
            SpecifiedGradientKind::Linear(None) => {}
            SpecifiedGradientKind::Linear(Some(SpecifiedDirection::Angle(angle))) => {
                let mut text = String::new();
                angle.to_css(&mut text);
                prelude.push(text);
            }
            SpecifiedGradientKind::Linear(Some(SpecifiedDirection::To(x, y))) => {
                prelude.push(side_or_corner_to_css(*x, *y));
            }
            SpecifiedGradientKind::Radial {
                shape,
                size,
                position,
            } => {
                if let Some(shape) = shape {
                    prelude.push(shape_name(*shape).to_owned());
                }
                match size {
                    None => {}
                    Some(SpecifiedRadialSize::Extent(extent)) => {
                        prelude.push(extent_name(*extent).to_owned())
                    }
                    Some(SpecifiedRadialSize::Circle(length)) => {
                        let mut text = String::new();
                        length.to_css(&mut text);
                        prelude.push(text);
                    }
                    Some(SpecifiedRadialSize::Ellipse(x, y)) => {
                        let mut text = String::new();
                        x.to_css(&mut text);
                        text.push(' ');
                        y.to_css(&mut text);
                        prelude.push(text);
                    }
                }
                if let Some(position) = position {
                    let mut text = String::from("at ");
                    position.to_css(&mut text);
                    prelude.push(text);
                }
            }
        }
        if !prelude.is_empty() {
            dest.push_str(&prelude.join(" "));
            dest.push_str(", ");
        }
        list_to_css(&self.items, dest, |item, dest| match item {
            SpecifiedGradientItem::Stop(color, position) => {
                color.to_css(dest);
                if let Some(position) = position {
                    dest.push(' ');
                    position.to_css(dest);
                }
            }
            SpecifiedGradientItem::Hint(position) => position.to_css(dest),
        });
        dest.push(')');
    }
}

fn gradient_name(repeating: bool, radial: bool) -> &'static str {
    match (repeating, radial) {
        (false, false) => "linear-gradient",
        (true, false) => "repeating-linear-gradient",
        (false, true) => "radial-gradient",
        (true, true) => "repeating-radial-gradient",
    }
}

fn shape_name(shape: RadialShape) -> &'static str {
    match shape {
        RadialShape::Circle => "circle",
        RadialShape::Ellipse => "ellipse",
    }
}

fn extent_name(size: RadialSize) -> &'static str {
    match size {
        RadialSize::ClosestSide => "closest-side",
        RadialSize::FarthestSide => "farthest-side",
        RadialSize::ClosestCorner => "closest-corner",
        RadialSize::FarthestCorner => "farthest-corner",
        RadialSize::Circle(_) | RadialSize::Ellipse(..) => unreachable!("not a keyword"),
    }
}

/// `to` and its sides: the horizontal one first, as Chrome writes it.
fn side_or_corner_to_css(x: Option<HorizontalSide>, y: Option<VerticalSide>) -> String {
    let mut parts = vec!["to"];
    match x {
        Some(HorizontalSide::Left) => parts.push("left"),
        Some(HorizontalSide::Right) => parts.push("right"),
        None => {}
    }
    match y {
        Some(VerticalSide::Top) => parts.push("top"),
        Some(VerticalSide::Bottom) => parts.push("bottom"),
        None => {}
    }
    parts.join(" ")
}

/// `<side-or-corner>` after `to`: a side, or one of each in either order.
fn parse_side_or_corner<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedDirection> {
    let side = |i: &mut Parser<'i, '_>| {
        let location = i.current_source_location();
        let ident = i.expect_ident()?;
        Ok(match_ignore_ascii_case! { ident,
            "left" => (Some(HorizontalSide::Left), None),
            "right" => (Some(HorizontalSide::Right), None),
            "top" => (None, Some(VerticalSide::Top)),
            "bottom" => (None, Some(VerticalSide::Bottom)),
            _ => return Err(location.new_custom_error::<(), ()>(())),
        })
    };
    let (x, y) = side(input)?;
    match input.try_parse(side) {
        Ok((x2, y2)) if x.is_some() != x2.is_some() => {
            Ok(SpecifiedDirection::To(x.or(x2), y.or(y2)))
        }
        Ok(_) => invalid(input),
        Err(_) => Ok(SpecifiedDirection::To(x, y)),
    }
}

/// An extent keyword, or a circle's length, or an ellipse's two
/// lengths or percentages, none negative.
fn parse_radial_size<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedRadialSize> {
    if let Ok(extent) = input.try_parse(|i| {
        let location = i.current_source_location();
        let ident = i.expect_ident()?;
        Ok(match_ignore_ascii_case! { ident,
            "closest-side" => RadialSize::ClosestSide,
            "farthest-side" => RadialSize::FarthestSide,
            "closest-corner" => RadialSize::ClosestCorner,
            "farthest-corner" => RadialSize::FarthestCorner,
            _ => return Err(location.new_custom_error::<(), ()>(())),
        })
    }) {
        return Ok(SpecifiedRadialSize::Extent(extent));
    }
    let x = LengthPercentage::parse(input, false)?;
    if let Ok(y) = input.try_parse(|i| LengthPercentage::parse(i, false)) {
        return Ok(SpecifiedRadialSize::Ellipse(x, y));
    }
    match x {
        LengthPercentage::Length(length) => Ok(SpecifiedRadialSize::Circle(length)),
        LengthPercentage::Percent(_) | LengthPercentage::Calc(_) => invalid(input),
    }
}

/// `<color-stop-list>`: stops, each a color and up to two positions (two
/// are two stops of the color), with optional hints between them; at
/// least two stops.
fn parse_color_stop_list<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Vec<SpecifiedGradientItem>> {
    let mut items = Vec::new();
    loop {
        if let Ok(hint) = input.try_parse(|i| {
            let hint = LengthPercentage::parse(i, true)?;
            i.expect_comma()?;
            Ok::<_, cssparser::ParseError<'i, ()>>(hint)
        }) {
            // Only between two stops.
            if !matches!(items.last(), Some(SpecifiedGradientItem::Stop(..))) {
                return invalid(input);
            }
            items.push(SpecifiedGradientItem::Hint(hint));
        }
        let color = parse_style_color(input)?;
        let first = input.try_parse(|i| LengthPercentage::parse(i, true)).ok();
        let second = first
            .as_ref()
            .and_then(|_| input.try_parse(|i| LengthPercentage::parse(i, true)).ok());
        items.push(SpecifiedGradientItem::Stop(color.clone(), first));
        if let Some(second) = second {
            items.push(SpecifiedGradientItem::Stop(color, Some(second)));
        }
        if input.is_exhausted() {
            break;
        }
        input.expect_comma()?;
    }
    let stops = items
        .iter()
        .filter(|item| matches!(item, SpecifiedGradientItem::Stop(..)))
        .count();
    if stops < 2 {
        return invalid(input);
    }
    Ok(items)
}

/// A gradient's computed value as Chrome writes it: the default direction,
/// shape, size and position left out.
fn gradient_to_css(style: &ComputedStyle, gradient: &Gradient, dest: &mut String) {
    let radial = matches!(gradient.kind, GradientKind::Radial { .. });
    dest.push_str(gradient_name(gradient.repeating, radial));
    dest.push('(');
    let mut prelude = Vec::new();
    match gradient.kind {
        GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom))) => {}
        GradientKind::Linear(LinearDirection::Angle(degrees)) => {
            let mut text = String::new();
            write_number(&mut text, degrees.into());
            text.push_str("deg");
            prelude.push(text);
        }
        GradientKind::Linear(LinearDirection::To(x, y)) => {
            prelude.push(side_or_corner_to_css(x, y));
        }
        GradientKind::Radial {
            shape,
            size,
            position,
        } => {
            match size {
                RadialSize::FarthestCorner => {
                    if shape == RadialShape::Circle {
                        prelude.push("circle".to_owned());
                    }
                }
                RadialSize::Circle(px) => {
                    let mut text = String::new();
                    write_number(&mut text, px.into());
                    text.push_str("px");
                    prelude.push(text);
                }
                RadialSize::Ellipse(x, y) => {
                    let mut text = String::new();
                    length_percentage_to_css(x, &mut text);
                    text.push(' ');
                    length_percentage_to_css(y, &mut text);
                    prelude.push(text);
                }
                extent => {
                    if shape == RadialShape::Circle {
                        prelude.push("circle".to_owned());
                    }
                    prelude.push(extent_name(extent).to_owned());
                }
            }
            if position != Position2D::CENTER {
                let mut text = String::from("at ");
                position_component_to_css(position.x, &mut text);
                text.push(' ');
                position_component_to_css(position.y, &mut text);
                prelude.push(text);
            }
        }
    }
    if !prelude.is_empty() {
        dest.push_str(&prelude.join(" "));
        dest.push_str(", ");
    }
    list_to_css(&gradient.items, dest, |item, dest| match *item {
        GradientItem::Stop(color, position) => {
            write_rgba(dest, style.resolve(color));
            if let Some(position) = position {
                dest.push(' ');
                length_percentage_to_css(position, dest);
            }
        }
        GradientItem::Hint(position) => length_percentage_to_css(position, dest),
    });
    dest.push(')');
}

fn length_percentage_to_css(value: ComputedLengthPercentage, dest: &mut String) {
    match value {
        ComputedLengthPercentage::Px(px) => {
            write_number(dest, px.into());
            dest.push_str("px");
        }
        ComputedLengthPercentage::Percent(percent) => {
            write_number(dest, percent.into());
            dest.push('%');
        }
        ComputedLengthPercentage::Calc { px, percent, .. } => {
            crate::calc::write_computed(px, percent, dest);
        }
    }
}

/// `background-image`'s computed value: absolute URLs, gradients with
/// their colors resolved.
pub(crate) fn images_to_css(style: &ComputedStyle, dest: &mut String) {
    list_to_css(&style.backgrounds.background_image, dest, |image, dest| {
        image_to_css(Some(style), image, dest)
    });
}

/// An image's computed value (`style`'s, for the gradient's colors;
/// none for an image of URLs only, a cursor's).
pub(crate) fn image_to_css(
    style: Option<&ComputedStyle>,
    image: &BackgroundImage,
    dest: &mut String,
) {
    match image {
        BackgroundImage::None => dest.push_str("none"),
        BackgroundImage::Url(url) => url_to_css(&url.url, dest),
        BackgroundImage::Gradient(gradient) => {
            gradient_to_css(style.expect("a gradient's style"), gradient, dest)
        }
        // The resolution in dppx (Chrome 152 writes `192dpi` as `2dppx`).
        BackgroundImage::Set(options) => {
            dest.push_str("image-set(");
            list_to_css(options, dest, |option, dest| {
                image_to_css(style, &option.image, dest);
                dest.push(' ');
                write_number(dest, option.resolution.into());
                dest.push_str("dppx");
                if let Some(mime_type) = &option.mime_type {
                    type_to_css(mime_type, dest);
                }
            });
            dest.push(')');
        }
    }
}

// --- <position> -----------------------------------------------------------

/// One axis of a `<position>` as written: `center`, a side with an
/// optional offset from it, or an offset from the start side.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedPositionComponent {
    Center,
    /// `left` / `top` (`false`) or `right` / `bottom` (`true`).
    Side(bool, Option<LengthPercentage>),
    Offset(LengthPercentage),
}

impl SpecifiedPositionComponent {
    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> PositionComponent {
        let start = |offset| PositionComponent {
            from_end: false,
            offset,
        };
        match self {
            Self::Center => PositionComponent::CENTER,
            Self::Offset(offset) | Self::Side(false, Some(offset)) => start(offset.compute(cx)),
            Self::Side(false, None) => PositionComponent::START,
            Self::Side(true, None) => start(ComputedLengthPercentage::Percent(100.0)),
            Self::Side(true, Some(offset)) => match offset.compute(cx) {
                ComputedLengthPercentage::Percent(percent) => {
                    start(ComputedLengthPercentage::Percent(100.0 - percent))
                }
                // From the start: 100% less it (Blink's computed value).
                ComputedLengthPercentage::Calc { px, percent, .. } => {
                    start(ComputedLengthPercentage::Calc {
                        px: -px,
                        percent: 100.0 - percent,
                        non_negative: false,
                    })
                }
                px => PositionComponent {
                    from_end: true,
                    offset: px,
                },
            },
        }
    }

    /// As written, with the keywords for `axis` (0 for x, 1 for y).
    pub(crate) fn to_css(&self, axis: usize, dest: &mut String) {
        let side = |end: bool| match (axis, end) {
            (0, false) => "left",
            (0, true) => "right",
            (_, false) => "top",
            (_, true) => "bottom",
        };
        match self {
            Self::Center => dest.push_str("center"),
            Self::Side(end, offset) => {
                dest.push_str(side(*end));
                if let Some(offset) = offset {
                    dest.push(' ');
                    offset.to_css(dest);
                }
            }
            Self::Offset(offset) => offset.to_css(dest),
        }
    }

    /// `background-position-x` / `-y`: `center`, the axis's side with an
    /// optional offset, or an offset.
    pub(crate) fn parse_axis<'i>(input: &mut Parser<'i, '_>, axis: usize) -> ParseResult<'i, Self> {
        if let Ok(offset) = input.try_parse(|i| LengthPercentage::parse(i, true)) {
            return Ok(Self::Offset(offset));
        }
        let location = input.current_source_location();
        let ident = input.expect_ident()?.clone();
        let end = match (axis, ident.to_ascii_lowercase().as_str()) {
            (_, "center") => return Ok(Self::Center),
            (0, "left") | (1, "top") => false,
            (0, "right") | (1, "bottom") => true,
            _ => return Err(location.new_custom_error::<(), ()>(())),
        };
        let offset = input.try_parse(|i| LengthPercentage::parse(i, true)).ok();
        Ok(Self::Side(end, offset))
    }
}

/// A computed `<position>` component, as Chrome writes it: an offset from
/// the end side as `calc(100% - …)`.
pub(crate) fn position_component_to_css(value: PositionComponent, dest: &mut String) {
    match (value.from_end, value.offset) {
        (false, offset) => length_percentage_to_css(offset, dest),
        (true, ComputedLengthPercentage::Px(0.0)) => dest.push_str("100%"),
        (true, ComputedLengthPercentage::Px(px)) => {
            let sign = if px < 0.0 { '+' } else { '-' };
            dest.push_str(&format!("calc(100% {sign} "));
            write_number(dest, px.abs().into());
            dest.push_str("px)");
        }
        (true, ComputedLengthPercentage::Percent(percent)) => {
            write_number(dest, (100.0 - percent).into());
            dest.push('%');
        }
        (true, ComputedLengthPercentage::Calc { px, percent, .. }) => {
            crate::calc::write_computed(-px, 100.0 - percent, dest);
        }
    }
}

/// A `<bg-position>` / `<position>` as written: both axes.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedPosition {
    pub(crate) x: SpecifiedPositionComponent,
    pub(crate) y: SpecifiedPositionComponent,
}

/// A keyword of `<bg-position>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionKeyword {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

impl PositionKeyword {
    fn is_horizontal(&self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    fn is_vertical(&self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }

    fn component(&self, offset: Option<LengthPercentage>) -> SpecifiedPositionComponent {
        match self {
            Self::Center => SpecifiedPositionComponent::Center,
            Self::Left | Self::Top => SpecifiedPositionComponent::Side(false, offset),
            Self::Right | Self::Bottom => SpecifiedPositionComponent::Side(true, offset),
        }
    }
}

#[derive(Debug, Clone)]
enum PositionPart {
    Keyword(PositionKeyword),
    Offset(LengthPercentage),
}

impl SpecifiedPosition {
    /// `<bg-position>` (css-backgrounds 3): one to four values.
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        Self::parse_parts(input, true)
    }

    /// `<position>` (css-values 4, `object-position`): as `<bg-position>`,
    /// but not three values.
    pub(crate) fn parse_position<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        Self::parse_parts(input, false)
    }

    fn parse_parts<'i>(input: &mut Parser<'i, '_>, three: bool) -> ParseResult<'i, Self> {
        let part = |i: &mut Parser<'i, '_>| {
            if let Ok(offset) = i.try_parse(|i| LengthPercentage::parse(i, true)) {
                return Ok(PositionPart::Offset(offset));
            }
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            Ok(PositionPart::Keyword(match_ignore_ascii_case! { ident,
                "left" => PositionKeyword::Left,
                "right" => PositionKeyword::Right,
                "top" => PositionKeyword::Top,
                "bottom" => PositionKeyword::Bottom,
                "center" => PositionKeyword::Center,
                _ => return Err(location.new_custom_error::<(), ()>(())),
            }))
        };
        let mut parts = vec![part(input)?];
        while parts.len() < 4 {
            match input.try_parse(part) {
                Ok(p) => parts.push(p),
                Err(_) => break,
            }
        }
        if parts.len() == 3 && !three {
            return Err(input.new_custom_error(()));
        }
        // Nothing else in the `background` shorthand is a keyword or a
        // length here, so taking all that parse loses nothing.
        Self::from_parts(&parts).ok_or_else(|| input.new_custom_error(()))
    }

    fn from_parts(parts: &[PositionPart]) -> Option<Self> {
        use PositionKeyword::*;
        use PositionPart::{Keyword as K, Offset as O};
        let component = |k: PositionKeyword| k.component(None);
        Some(match parts {
            [O(x)] => Self {
                x: SpecifiedPositionComponent::Offset(x.clone()),
                y: SpecifiedPositionComponent::Center,
            },
            [K(k)] => {
                let k = *k;
                if k.is_vertical() {
                    Self {
                        x: SpecifiedPositionComponent::Center,
                        y: component(k),
                    }
                } else {
                    Self {
                        x: component(k),
                        y: SpecifiedPositionComponent::Center,
                    }
                }
            }
            [K(a), K(b)] => {
                let (a, b) = (*a, *b);
                // Either order: `top left`, `center left`.
                let (h, v) = if a.is_vertical() || b.is_horizontal() {
                    (b, a)
                } else {
                    (a, b)
                };
                if h.is_vertical() || v.is_horizontal() {
                    return None;
                }
                Self {
                    x: component(h),
                    y: component(v),
                }
            }
            [first, second] => {
                let x = match first {
                    O(x) => SpecifiedPositionComponent::Offset(x.clone()),
                    K(k) if !k.is_vertical() => component(*k),
                    K(_) => return None,
                };
                let y = match second {
                    O(y) => SpecifiedPositionComponent::Offset(y.clone()),
                    K(k) if !k.is_horizontal() => component(*k),
                    K(_) => return None,
                };
                Self { x, y }
            }
            // Three or four: two keywords, each but `center` with an
            // optional offset after it, at least one with one.
            _ => {
                let mut groups: Vec<(PositionKeyword, Option<LengthPercentage>)> = Vec::new();
                for part in parts {
                    match (part, groups.last_mut()) {
                        (K(k), _) => groups.push((*k, None)),
                        (O(offset), Some((k, slot @ None))) if *k != Center => {
                            *slot = Some(offset.clone())
                        }
                        _ => return None,
                    }
                }
                let [(a, a_offset), (b, b_offset)] = <[_; 2]>::try_from(groups).ok()?;
                let (h, h_offset, v, v_offset) = if a.is_vertical() || b.is_horizontal() {
                    (b, b_offset, a, a_offset)
                } else {
                    (a, a_offset, b, b_offset)
                };
                if h.is_vertical() || v.is_horizontal() {
                    return None;
                }
                Self {
                    x: h.component(h_offset),
                    y: v.component(v_offset),
                }
            }
        })
    }

    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> Position2D {
        Position2D {
            x: self.x.compute(cx),
            y: self.y.compute(cx),
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        self.x.to_css(0, dest);
        dest.push(' ');
        self.y.to_css(1, dest);
    }
}

// --- the other longhands --------------------------------------------------

/// One layer's `background-size` as written: a single length leaves the
/// height out, which is `auto`.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedBackgroundSize {
    Cover,
    Contain,
    /// `None` is `auto`.
    Explicit(Option<LengthPercentage>, Option<Option<LengthPercentage>>),
}

impl SpecifiedBackgroundSize {
    pub(crate) const AUTO: Self = Self::Explicit(None, None);

    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        let auto_or = |i: &mut Parser<'i, '_>| {
            if i.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                return Ok(None);
            }
            LengthPercentage::parse(i, false).map(Some)
        };
        if let Ok(keyword) = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            Ok(match_ignore_ascii_case! { ident,
                "cover" => Self::Cover,
                "contain" => Self::Contain,
                _ => return Err(location.new_custom_error::<(), ()>(())),
            })
        }) {
            return Ok(keyword);
        }
        let width = auto_or(input)?;
        let height = input.try_parse(auto_or).ok();
        Ok(Self::Explicit(width, height))
    }

    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> BackgroundSize {
        let auto_or = |v: Option<&LengthPercentage>| match v {
            None => LengthPercentageOrAuto::Auto,
            Some(v) => LengthPercentageOrAuto::LengthPercentage(v.compute(cx)),
        };
        match self {
            Self::Cover => BackgroundSize::Cover,
            Self::Contain => BackgroundSize::Contain,
            Self::Explicit(width, height) => BackgroundSize::Explicit(
                auto_or(width.as_ref()),
                auto_or(height.as_ref().and_then(Option::as_ref)),
            ),
        }
    }

    /// Both sizes unless both are `auto`, as Chrome writes them.
    pub(crate) fn to_css(&self, dest: &mut String) {
        let auto_or = |v: Option<&LengthPercentage>, dest: &mut String| match v {
            None => dest.push_str("auto"),
            Some(v) => v.to_css(dest),
        };
        match self {
            Self::Cover => dest.push_str("cover"),
            Self::Contain => dest.push_str("contain"),
            Self::Explicit(None, None | Some(None)) => dest.push_str("auto"),
            Self::Explicit(width, height) => {
                auto_or(width.as_ref(), dest);
                dest.push(' ');
                auto_or(height.as_ref().and_then(Option::as_ref), dest);
            }
        }
    }
}

pub(crate) fn sizes_to_css(sizes: &[BackgroundSize], dest: &mut String) {
    let auto_or = |v: LengthPercentageOrAuto, dest: &mut String| match v {
        LengthPercentageOrAuto::Auto => dest.push_str("auto"),
        LengthPercentageOrAuto::LengthPercentage(v) => length_percentage_to_css(v, dest),
    };
    list_to_css(sizes, dest, |size, dest| match *size {
        BackgroundSize::Cover => dest.push_str("cover"),
        BackgroundSize::Contain => dest.push_str("contain"),
        BackgroundSize::Explicit(LengthPercentageOrAuto::Auto, LengthPercentageOrAuto::Auto) => {
            dest.push_str("auto")
        }
        BackgroundSize::Explicit(width, height) => {
            auto_or(width, dest);
            dest.push(' ');
            auto_or(height, dest);
        }
    });
}

/// `<repeat-style>`: `repeat-x`, `repeat-y`, or one or two of the others
/// (one for both axes).
pub(crate) fn parse_repeat<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, (BackgroundRepeat, BackgroundRepeat)> {
    use BackgroundRepeat::*;
    let location = input.current_source_location();
    let ident = input.expect_ident()?.clone();
    let keyword = |name: &str| {
        Some(match_ignore_ascii_case! { name,
            "repeat" => Repeat,
            "space" => Space,
            "round" => Round,
            "no-repeat" => NoRepeat,
            _ => return None,
        })
    };
    match_ignore_ascii_case! { &ident,
        "repeat-x" => return Ok((Repeat, NoRepeat)),
        "repeat-y" => return Ok((NoRepeat, Repeat)),
        _ => {}
    }
    let x = keyword(&ident).ok_or_else(|| location.new_custom_error::<(), ()>(()))?;
    let y = input
        .try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            keyword(ident).ok_or_else(|| location.new_custom_error::<(), ()>(()))
        })
        .unwrap_or(x);
    Ok((x, y))
}

fn repeat_name(repeat: BackgroundRepeat) -> &'static str {
    match repeat {
        BackgroundRepeat::Repeat => "repeat",
        BackgroundRepeat::Space => "space",
        BackgroundRepeat::Round => "round",
        BackgroundRepeat::NoRepeat => "no-repeat",
    }
}

/// `repeat-x` / `repeat-y` where they say it, one keyword for both axes.
pub(crate) fn repeats_to_css(repeats: &[(BackgroundRepeat, BackgroundRepeat)], dest: &mut String) {
    use BackgroundRepeat::*;
    list_to_css(repeats, dest, |&(x, y), dest| match (x, y) {
        (Repeat, NoRepeat) => dest.push_str("repeat-x"),
        (NoRepeat, Repeat) => dest.push_str("repeat-y"),
        (x, y) if x == y => dest.push_str(repeat_name(x)),
        (x, y) => {
            dest.push_str(repeat_name(x));
            dest.push(' ');
            dest.push_str(repeat_name(y));
        }
    });
}

pub(crate) fn parse_attachment<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, BackgroundAttachment> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "scroll" => BackgroundAttachment::Scroll,
        "fixed" => BackgroundAttachment::Fixed,
        "local" => BackgroundAttachment::Local,
        _ => return Err(location.new_custom_error::<(), ()>(())),
    })
}

pub(crate) fn attachments_to_css(attachments: &[BackgroundAttachment], dest: &mut String) {
    list_to_css(attachments, dest, |attachment, dest| {
        dest.push_str(match attachment {
            BackgroundAttachment::Scroll => "scroll",
            BackgroundAttachment::Fixed => "fixed",
            BackgroundAttachment::Local => "local",
        })
    });
}

/// `<visual-box>`: `border-box`, `padding-box` or `content-box`.
pub(crate) fn parse_box<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, BackgroundBox> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "border-box" => BackgroundBox::BorderBox,
        "padding-box" => BackgroundBox::PaddingBox,
        "content-box" => BackgroundBox::ContentBox,
        _ => return Err(location.new_custom_error::<(), ()>(())),
    })
}

pub(crate) fn box_name(value: BackgroundBox) -> &'static str {
    match value {
        BackgroundBox::BorderBox => "border-box",
        BackgroundBox::PaddingBox => "padding-box",
        BackgroundBox::ContentBox => "content-box",
    }
}

pub(crate) fn boxes_to_css(boxes: &[BackgroundBox], dest: &mut String) {
    list_to_css(boxes, dest, |value, dest| dest.push_str(box_name(*value)));
}

pub(crate) fn position_components_to_css(components: &[PositionComponent], dest: &mut String) {
    list_to_css(components, dest, |component, dest| {
        position_component_to_css(*component, dest)
    });
}

/// Specified lists, for serialization in `serialize.rs`.
pub(crate) fn specified_list_to_css<T>(
    list: &[T],
    dest: &mut String,
    item: impl FnMut(&T, &mut String),
) {
    list_to_css(list, dest, item)
}

#[cfg(test)]
mod tests {
    use cssparser::ParserInput;

    use super::*;

    fn parse<T>(
        css: &str,
        f: impl for<'i, 't> FnOnce(&mut Parser<'i, 't>) -> ParseResult<'i, T>,
    ) -> Option<T> {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        parser.parse_entirely(|p| f(p)).ok()
    }

    fn position(css: &str) -> Option<String> {
        let position = parse(css, SpecifiedPosition::parse)?;
        let mut text = String::new();
        position.to_css(&mut text);
        Some(text)
    }

    #[test]
    fn positions_take_one_to_four_values() {
        for (css, expected) in [
            ("1px", "1px center"),
            ("center left", "left center"),
            ("bottom", "center bottom"),
            ("top left", "left top"),
            ("center right 7%", "right 7% center"),
            ("top 17px right 18px", "right 18px top 17px"),
            ("bottom 16% left", "left bottom 16%"),
            ("left 10px center", "left 10px center"),
            ("-2% -3%", "-2% -3%"),
        ] {
            assert_eq!(position(css).as_deref(), Some(expected), "{css}");
        }
        for css in [
            "left right",
            "top bottom",
            "10px left",
            "top 10px",
            "center 10px left",
            "left 1px 2px",
            "left top 1px right",
        ] {
            assert_eq!(position(css), None, "{css}");
        }
    }

    fn image(css: &str) -> Option<String> {
        let image = parse(css, parse_image)?;
        let mut text = String::new();
        image.to_css(&mut text);
        Some(text)
    }

    #[test]
    fn images_serialize_as_written() {
        for (css, expected) in [
            ("url(a.png)", r#"url("a.png")"#),
            (r#"url( "a b.png" )"#, r#"url("a b.png")"#),
            ("none", "none"),
            ("linear-gradient(red, blue)", "linear-gradient(red, blue)"),
            (
                "linear-gradient(to bottom left, red 10%, 30%, blue 20px 40px)",
                "linear-gradient(to left bottom, red 10%, 30%, blue 20px, blue 40px)",
            ),
            (
                "repeating-linear-gradient(0.25turn, red, blue)",
                "repeating-linear-gradient(0.25turn, red, blue)",
            ),
            (
                "radial-gradient(circle 10px at left top, red, blue)",
                "radial-gradient(circle 10px at left top, red, blue)",
            ),
            (
                "radial-gradient(farthest-side ellipse, red, blue)",
                "radial-gradient(ellipse farthest-side, red, blue)",
            ),
        ] {
            assert_eq!(image(css).as_deref(), Some(expected), "{css}");
        }
        for css in [
            "\"a.png\"",
            "linear-gradient(red)",
            "linear-gradient(to left right, red, blue)",
            "linear-gradient(red, 10%)",
            "linear-gradient(10%, red, blue)",
            "linear-gradient(red, 10%, 20%, blue)",
            "radial-gradient(circle 10% 10%, red, blue)",
            "radial-gradient(ellipse 10px, red, blue)",
            "radial-gradient(circle 10%, red, blue)",
            "conic-gradient(red, blue)",
        ] {
            assert_eq!(image(css), None, "{css}");
        }
    }
}
