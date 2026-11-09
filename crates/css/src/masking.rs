//! `clip` (css-masking 1, deprecated) and `clip-path` with the
//! `<basic-shape>`s (css-shapes 1) and `<geometry-box>`es it takes.

use std::sync::Arc;

use computed_style::{
    BasicShape, ClipPath, ClipRect, GeometryBox, LengthPercentage as ComputedLengthPercentage,
    PathSegment, ShapeRadius,
};
use cssparser::{Parser, match_ignore_ascii_case};
use gfx::FillRule;

use crate::background::{SpecifiedPosition, position_component_to_css};
use crate::computed::length_percentage as computed_length_percentage_to_css;
use crate::css_url_data::CssUrlData;
use crate::values::{ComputeContext, Length, LengthPercentage, ParseResult, write_number};

// --- clip -----------------------------------------------------------------

/// `clip` as written: `None` is `auto`; each edge of `rect()` (top, right,
/// bottom, left) `None` for `auto`.
pub type SpecifiedClip = Option<[Option<Length>; 4]>;

/// `auto | rect(<top>, <right>, <bottom>, <left>)`, each edge a length or
/// `auto`, separated by commas or (all of them) by spaces.
pub(crate) fn parse_clip<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedClip> {
    if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
        return Ok(None);
    }
    input.expect_function_matching("rect")?;
    input.parse_nested_block(|i| {
        let edge = |i: &mut Parser<'i, '_>| -> ParseResult<'i, Option<Length>> {
            if i.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                return Ok(None);
            }
            Ok(Some(Length::parse(i, true)?))
        };
        let mut edges = [edge(i)?, None, None, None];
        let commas = i.try_parse(|i| i.expect_comma()).is_ok();
        for (k, slot) in edges.iter_mut().enumerate().skip(1) {
            if commas && k > 1 {
                i.expect_comma()?;
            }
            *slot = edge(i)?;
        }
        Ok(Some(edges))
    })
}

pub(crate) fn compute_clip(clip: &SpecifiedClip, cx: &ComputeContext<'_>) -> Option<ClipRect> {
    clip.as_ref().map(|edges| {
        edges
            .each_ref()
            .map(|edge| edge.as_ref().map(|l| l.to_px(cx)))
    })
}

/// `rect(…)`'s edges, comma separated (Blink's `CSSQuadValue` for
/// `clip`).
fn clip_to_css<T>(
    edges: Option<&[Option<T>; 4]>,
    dest: &mut String,
    edge: impl Fn(&T, &mut String),
) {
    let Some(edges) = edges else {
        dest.push_str("auto");
        return;
    };
    dest.push_str("rect(");
    for (i, e) in edges.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        match e {
            Some(e) => edge(e, dest),
            None => dest.push_str("auto"),
        }
    }
    dest.push(')');
}

pub(crate) fn specified_clip_to_css(clip: &SpecifiedClip, dest: &mut String) {
    clip_to_css(clip.as_ref(), dest, Length::to_css);
}

pub(crate) fn computed_clip_to_css(clip: Option<&ClipRect>, dest: &mut String) {
    clip_to_css(clip, dest, |px, dest| {
        write_number(dest, (*px).into());
        dest.push_str("px");
    });
}

// --- <geometry-box> -------------------------------------------------------

fn parse_geometry_box<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, GeometryBox> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "border-box" => GeometryBox::BorderBox,
        "padding-box" => GeometryBox::PaddingBox,
        "content-box" => GeometryBox::ContentBox,
        "margin-box" => GeometryBox::MarginBox,
        "fill-box" => GeometryBox::FillBox,
        "stroke-box" => GeometryBox::StrokeBox,
        "view-box" => GeometryBox::ViewBox,
        _ => return Err(location.new_custom_error(())),
    })
}

pub(crate) fn geometry_box_name(value: GeometryBox) -> &'static str {
    match value {
        GeometryBox::BorderBox => "border-box",
        GeometryBox::PaddingBox => "padding-box",
        GeometryBox::ContentBox => "content-box",
        GeometryBox::MarginBox => "margin-box",
        GeometryBox::FillBox => "fill-box",
        GeometryBox::StrokeBox => "stroke-box",
        GeometryBox::ViewBox => "view-box",
    }
}

// --- <basic-shape> --------------------------------------------------------

/// A `<shape-radius>` as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedShapeRadius {
    Length(LengthPercentage),
    ClosestSide,
    FarthestSide,
}

impl SpecifiedShapeRadius {
    fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if let Ok(length) = input.try_parse(|i| LengthPercentage::parse(i, false)) {
            return Ok(Self::Length(length));
        }
        let location = input.current_source_location();
        let ident = input.expect_ident()?;
        Ok(match_ignore_ascii_case! { ident,
            "closest-side" => Self::ClosestSide,
            "farthest-side" => Self::FarthestSide,
            _ => return Err(location.new_custom_error(())),
        })
    }

    fn compute(&self, cx: &ComputeContext<'_>) -> ShapeRadius {
        match self {
            Self::Length(length) => ShapeRadius::Length(length.compute(cx)),
            Self::ClosestSide => ShapeRadius::ClosestSide,
            Self::FarthestSide => ShapeRadius::FarthestSide,
        }
    }

    fn to_css(&self, dest: &mut String) {
        match self {
            Self::Length(length) => length.to_css(dest),
            Self::ClosestSide => dest.push_str("closest-side"),
            Self::FarthestSide => dest.push_str("farthest-side"),
        }
    }
}

/// `round <'border-radius'>`'s corners from the top left round, each
/// (horizontal, vertical); `None` without it.
pub type SpecifiedRadii = Option<[(LengthPercentage, LengthPercentage); 4]>;

/// One to four values for four sides or corners, as `margin` and
/// `border-radius` take them.
fn four<T: Clone>(values: &[T]) -> [T; 4] {
    match values {
        [a] => [a.clone(), a.clone(), a.clone(), a.clone()],
        [a, b] => [a.clone(), b.clone(), a.clone(), b.clone()],
        [a, b, c] => [a.clone(), b.clone(), c.clone(), b.clone()],
        [a, b, c, d] => [a.clone(), b.clone(), c.clone(), d.clone()],
        _ => unreachable!("one to four values"),
    }
}

/// Four sides or corners written as `margin` writes them: each the one
/// opposite repeats left out.
fn four_to_css(values: &[String; 4]) -> String {
    let [top, right, bottom, left] = values;
    let count = if left != right {
        4
    } else if bottom != top {
        3
    } else if right != top {
        2
    } else {
        1
    };
    values[..count].join(" ")
}

/// One to four non-negative `<length-percentage>`s.
fn parse_one_to_four<'i>(
    input: &mut Parser<'i, '_>,
    negative: bool,
) -> ParseResult<'i, Vec<LengthPercentage>> {
    let mut values = vec![LengthPercentage::parse(input, negative)?];
    while values.len() < 4 {
        match input.try_parse(|i| LengthPercentage::parse(i, negative)) {
            Ok(value) => values.push(value),
            Err(_) => break,
        }
    }
    Ok(values)
}

/// An optional `round <'border-radius'>`.
fn parse_round<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedRadii> {
    if input
        .try_parse(|i| i.expect_ident_matching("round"))
        .is_err()
    {
        return Ok(None);
    }
    let horizontal = four(&parse_one_to_four(input, false)?);
    let vertical = if input.try_parse(|i| i.expect_delim('/')).is_ok() {
        four(&parse_one_to_four(input, false)?)
    } else {
        horizontal.clone()
    };
    Ok(Some(
        [0, 1, 2, 3].map(|i| (horizontal[i].clone(), vertical[i].clone())),
    ))
}

/// ` round …`, left out when every radius is 0.
fn radii_to_css<T>(
    radii: &[(T, T); 4],
    is_zero: impl Fn(&T) -> bool,
    write: impl Fn(&T) -> String,
) -> String {
    if radii.iter().all(|(x, y)| is_zero(x) && is_zero(y)) {
        return String::new();
    }
    let horizontal = four_to_css(&radii.each_ref().map(|(x, _)| write(x)));
    let vertical = four_to_css(&radii.each_ref().map(|(_, y)| write(y)));
    if horizontal == vertical {
        format!(" round {horizontal}")
    } else {
        format!(" round {horizontal} / {vertical}")
    }
}

fn specified_is_zero(value: &LengthPercentage) -> bool {
    matches!(
        value,
        LengthPercentage::Length(Length::Dimension { value: 0.0, .. })
            | LengthPercentage::Percent(0.0)
    )
}

fn computed_is_zero(value: &ComputedLengthPercentage) -> bool {
    matches!(
        value,
        ComputedLengthPercentage::Px(0.0) | ComputedLengthPercentage::Percent(0.0)
    )
}

fn specified_text(value: &LengthPercentage) -> String {
    let mut text = String::new();
    value.to_css(&mut text);
    text
}

fn computed_text(value: &ComputedLengthPercentage) -> String {
    let mut text = String::new();
    computed_length_percentage_to_css(*value, &mut text);
    text
}

/// An optional `<fill-rule>`.
fn parse_fill_rule<'i>(input: &mut Parser<'i, '_>) -> Option<FillRule> {
    input
        .try_parse(|i| -> ParseResult<'i, FillRule> {
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            Ok(match_ignore_ascii_case! { ident,
                "nonzero" => FillRule::NonZero,
                "evenodd" => FillRule::EvenOdd,
                _ => return Err(location.new_custom_error(())),
            })
        })
        .ok()
}

/// A `<basic-shape>` as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedBasicShape {
    /// Top, right, bottom, left.
    Inset {
        insets: [LengthPercentage; 4],
        radii: SpecifiedRadii,
    },
    /// `rect()`: the top, right, bottom and left edges from the reference
    /// box's top left corner, each `None` for `auto` (its own edge).
    Rect {
        edges: [Option<LengthPercentage>; 4],
        radii: SpecifiedRadii,
    },
    /// `xywh()`: x, y, width, height.
    Xywh {
        values: [LengthPercentage; 4],
        radii: SpecifiedRadii,
    },
    /// `None` for `closest-side` (written or not) and the center.
    Circle {
        radius: Option<SpecifiedShapeRadius>,
        center: Option<SpecifiedPosition>,
    },
    /// `None` for both `closest-side`.
    Ellipse {
        radii: Option<(SpecifiedShapeRadius, SpecifiedShapeRadius)>,
        center: Option<SpecifiedPosition>,
    },
    Polygon {
        fill_rule: FillRule,
        /// `None` without `round` or with `round 0`.
        round: Option<Length>,
        points: Vec<(LengthPercentage, LengthPercentage)>,
    },
    Path {
        fill_rule: FillRule,
        segments: Arc<[PathSegment]>,
    },
}

impl SpecifiedBasicShape {
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        let location = input.current_source_location();
        let name = input.expect_function()?.clone();
        match_ignore_ascii_case! { &name,
            "inset" => input.parse_nested_block(|i| {
                let insets = four(&parse_one_to_four(i, true)?);
                Ok(Self::Inset { insets, radii: parse_round(i)? })
            }),
            "rect" => input.parse_nested_block(|i| {
                let edge = |i: &mut Parser<'i, '_>| -> ParseResult<'i, Option<LengthPercentage>> {
                    if i.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
                        return Ok(None);
                    }
                    Ok(Some(LengthPercentage::parse(i, true)?))
                };
                let edges = [edge(i)?, edge(i)?, edge(i)?, edge(i)?];
                Ok(Self::Rect { edges, radii: parse_round(i)? })
            }),
            "xywh" => input.parse_nested_block(|i| {
                let values = [
                    LengthPercentage::parse(i, true)?,
                    LengthPercentage::parse(i, true)?,
                    LengthPercentage::parse(i, false)?,
                    LengthPercentage::parse(i, false)?,
                ];
                Ok(Self::Xywh { values, radii: parse_round(i)? })
            }),
            "circle" => input.parse_nested_block(|i| {
                let radius = i
                    .try_parse(SpecifiedShapeRadius::parse)
                    .ok()
                    .filter(|r| *r != SpecifiedShapeRadius::ClosestSide);
                Ok(Self::Circle { radius, center: parse_at(i)? })
            }),
            "ellipse" => input.parse_nested_block(|i| {
                let radii = i
                    .try_parse(|i| Ok::<_, cssparser::ParseError<'i, ()>>((
                        SpecifiedShapeRadius::parse(i)?,
                        SpecifiedShapeRadius::parse(i)?,
                    )))
                    .ok()
                    .filter(|(x, y)| {
                        *x != SpecifiedShapeRadius::ClosestSide
                            || *y != SpecifiedShapeRadius::ClosestSide
                    });
                Ok(Self::Ellipse { radii, center: parse_at(i)? })
            }),
            "polygon" => input.parse_nested_block(|i| {
                let fill_rule = parse_fill_rule(i);
                let round = if i.try_parse(|i| i.expect_ident_matching("round")).is_ok() {
                    Some(Length::parse(i, false)?)
                } else {
                    None
                };
                if fill_rule.is_some() || round.is_some() {
                    i.expect_comma()?;
                }
                let points = i.parse_comma_separated(|i| {
                    Ok((LengthPercentage::parse(i, true)?, LengthPercentage::parse(i, true)?))
                })?;
                let round = round.filter(|r| !matches!(r, Length::Dimension { value: 0.0, .. }));
                Ok(Self::Polygon { fill_rule: fill_rule.unwrap_or_default(), round, points })
            }),
            "path" => input.parse_nested_block(|i| {
                let fill_rule = parse_fill_rule(i);
                if fill_rule.is_some() {
                    i.expect_comma()?;
                }
                let data = i.expect_string()?.as_ref().to_owned();
                match crate::svg::path_data(&data, true) {
                    Some(segments) if !segments.is_empty() => Ok(Self::Path {
                        fill_rule: fill_rule.unwrap_or_default(),
                        segments: segments.into(),
                    }),
                    _ => Err(i.new_custom_error(())),
                }
            }),
            _ => Err(location.new_custom_error(())),
        }
    }

    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> BasicShape {
        let radii = |radii: &SpecifiedRadii| match radii {
            Some(radii) => radii
                .each_ref()
                .map(|(x, y)| (x.compute(cx), y.compute(cx))),
            None => {
                [(
                    ComputedLengthPercentage::ZERO,
                    ComputedLengthPercentage::ZERO,
                ); 4]
            }
        };
        let percent = ComputedLengthPercentage::Percent;
        match self {
            Self::Inset { insets, radii: r } => BasicShape::Inset {
                insets: insets.each_ref().map(|inset| inset.compute(cx)),
                radii: radii(r),
            },
            // Insets from the edges: `inset(t calc(100% - r) calc(100% -
            // b) l)` (css-shapes 1, "computed values of basic shapes").
            Self::Rect { edges, radii: r } => {
                let edge = |i: usize, auto: f32| {
                    edges[i].as_ref().map_or(percent(auto), |e| e.compute(cx))
                };
                BasicShape::Inset {
                    insets: [
                        edge(0, 0.0),
                        from_end(edge(1, 100.0)),
                        from_end(edge(2, 100.0)),
                        edge(3, 0.0),
                    ],
                    radii: radii(r),
                }
            }
            // `inset(y calc(100% - x - w) calc(100% - y - h) x)`.
            Self::Xywh { values, radii: r } => {
                let [x, y, w, h] = values.each_ref().map(|v| v.compute(cx));
                BasicShape::Inset {
                    insets: [y, from_end(add(x, w)), from_end(add(y, h)), x],
                    radii: radii(r),
                }
            }
            Self::Circle { radius, center } => BasicShape::Circle {
                radius: radius
                    .as_ref()
                    .map_or(ShapeRadius::ClosestSide, |r| r.compute(cx)),
                center: center.as_ref().map(|c| c.compute(cx)),
            },
            Self::Ellipse { radii, center } => BasicShape::Ellipse {
                radii: radii.as_ref().map_or(
                    (ShapeRadius::ClosestSide, ShapeRadius::ClosestSide),
                    |(x, y)| (x.compute(cx), y.compute(cx)),
                ),
                center: center.as_ref().map(|c| c.compute(cx)),
            },
            Self::Polygon {
                fill_rule,
                round,
                points,
            } => BasicShape::Polygon {
                fill_rule: *fill_rule,
                // Non-negative as parsed (a `calc()` clamped).
                round: round.as_ref().map_or(0.0, |r| r.to_px(cx)),
                points: points
                    .iter()
                    .map(|(x, y)| (x.compute(cx), y.compute(cx)))
                    .collect(),
            },
            Self::Path {
                fill_rule,
                segments,
            } => BasicShape::Path {
                fill_rule: *fill_rule,
                segments: segments.clone(),
            },
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Inset { insets, radii } => {
                let insets = four_to_css(&insets.each_ref().map(specified_text));
                dest.push_str(&format!("inset({insets}"));
                specified_radii_to_css(radii, dest);
                dest.push(')');
            }
            Self::Rect { edges, radii } => {
                let edges = edges
                    .each_ref()
                    .map(|e| e.as_ref().map_or("auto".to_owned(), specified_text));
                dest.push_str(&format!("rect({}", edges.join(" ")));
                specified_radii_to_css(radii, dest);
                dest.push(')');
            }
            Self::Xywh { values, radii } => {
                let values = values.each_ref().map(specified_text);
                dest.push_str(&format!("xywh({}", values.join(" ")));
                specified_radii_to_css(radii, dest);
                dest.push(')');
            }
            Self::Circle { radius, center } => {
                dest.push_str("circle(");
                if let Some(radius) = radius {
                    radius.to_css(dest);
                }
                specified_at_to_css(radius.is_some(), center.as_ref(), dest);
                dest.push(')');
            }
            Self::Ellipse { radii, center } => {
                dest.push_str("ellipse(");
                if let Some((x, y)) = radii {
                    x.to_css(dest);
                    dest.push(' ');
                    y.to_css(dest);
                }
                specified_at_to_css(radii.is_some(), center.as_ref(), dest);
                dest.push(')');
            }
            Self::Polygon {
                fill_rule,
                round,
                points,
            } => {
                let round = round.as_ref().map(|r| {
                    let mut text = String::new();
                    r.to_css(&mut text);
                    text
                });
                let points: Vec<String> = points
                    .iter()
                    .map(|(x, y)| format!("{} {}", specified_text(x), specified_text(y)))
                    .collect();
                polygon_to_css(*fill_rule, round.as_deref(), &points, dest);
            }
            Self::Path {
                fill_rule,
                segments,
            } => path_to_css(*fill_rule, segments, dest),
        }
    }
}

/// An optional `at <position>`.
fn parse_at<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Option<SpecifiedPosition>> {
    if input.try_parse(|i| i.expect_ident_matching("at")).is_err() {
        return Ok(None);
    }
    Ok(Some(SpecifiedPosition::parse_position(input)?))
}

fn specified_at_to_css(after: bool, center: Option<&SpecifiedPosition>, dest: &mut String) {
    if let Some(center) = center {
        if after {
            dest.push(' ');
        }
        dest.push_str("at ");
        center.to_css(dest);
    }
}

fn specified_radii_to_css(radii: &SpecifiedRadii, dest: &mut String) {
    if let Some(radii) = radii {
        dest.push_str(&radii_to_css(radii, specified_is_zero, specified_text));
    }
}

fn polygon_to_css(fill_rule: FillRule, round: Option<&str>, points: &[String], dest: &mut String) {
    dest.push_str("polygon(");
    let mut prefix = Vec::new();
    if fill_rule == FillRule::EvenOdd {
        prefix.push("evenodd".to_owned());
    }
    if let Some(round) = round {
        prefix.push(format!("round {round}"));
    }
    if !prefix.is_empty() {
        dest.push_str(&prefix.join(" "));
        dest.push_str(", ");
    }
    dest.push_str(&points.join(", "));
    dest.push(')');
}

fn path_to_css(fill_rule: FillRule, segments: &[PathSegment], dest: &mut String) {
    dest.push_str("path(");
    if fill_rule == FillRule::EvenOdd {
        dest.push_str("evenodd, ");
    }
    crate::svg::path_data_to_css(segments, dest);
    dest.push(')');
}

/// `a + b`, computed, as it only goes into [`from_end`].
fn add(a: ComputedLengthPercentage, b: ComputedLengthPercentage) -> ComputedLengthPercentage {
    calc(parts(a).0 + parts(b).0, parts(a).1 + parts(b).1)
}

/// `100% - v`, computed.
fn from_end(v: ComputedLengthPercentage) -> ComputedLengthPercentage {
    calc(-parts(v).0, 100.0 - parts(v).1)
}

/// Its px and its percentage.
fn parts(v: ComputedLengthPercentage) -> (f32, f32) {
    match v {
        ComputedLengthPercentage::Px(px) => (px, 0.0),
        ComputedLengthPercentage::Percent(percent) => (0.0, percent),
        ComputedLengthPercentage::Calc { px, percent, .. } => (px, percent),
    }
}

/// `calc(percent% + px)`, a percentage alone if no px is left.
fn calc(px: f32, percent: f32) -> ComputedLengthPercentage {
    if px == 0.0 {
        ComputedLengthPercentage::Percent(percent)
    } else {
        ComputedLengthPercentage::Calc {
            px,
            percent,
            non_negative: false,
        }
    }
}

fn computed_radius_to_css(radius: ShapeRadius, dest: &mut String) {
    match radius {
        ShapeRadius::Length(length) => computed_length_percentage_to_css(length, dest),
        ShapeRadius::ClosestSide => dest.push_str("closest-side"),
        ShapeRadius::FarthestSide => dest.push_str("farthest-side"),
    }
}

/// A computed `<basic-shape>`, as Blink's `ValueForBasicShape` writes it:
/// `closest-side` and a center not given left out.
pub(crate) fn computed_shape_to_css(shape: &BasicShape, dest: &mut String) {
    // ` at x y` after what is before it, if there is a center.
    let center_to_css = |center: Option<computed_style::Position2D>, dest: &mut String| {
        let Some(center) = center else {
            return;
        };
        if !dest.ends_with('(') {
            dest.push(' ');
        }
        dest.push_str("at ");
        position_component_to_css(center.x, dest);
        dest.push(' ');
        position_component_to_css(center.y, dest);
    };
    match shape {
        BasicShape::Inset { insets, radii } => {
            let insets = four_to_css(&insets.each_ref().map(computed_text));
            dest.push_str(&format!("inset({insets}"));
            dest.push_str(&radii_to_css(radii, computed_is_zero, computed_text));
            dest.push(')');
        }
        BasicShape::Circle { radius, center } => {
            dest.push_str("circle(");
            if *radius != ShapeRadius::ClosestSide {
                computed_radius_to_css(*radius, dest);
            }
            center_to_css(*center, dest);
            dest.push(')');
        }
        BasicShape::Ellipse { radii, center } => {
            dest.push_str("ellipse(");
            if *radii != (ShapeRadius::ClosestSide, ShapeRadius::ClosestSide) {
                computed_radius_to_css(radii.0, dest);
                dest.push(' ');
                computed_radius_to_css(radii.1, dest);
            }
            center_to_css(*center, dest);
            dest.push(')');
        }
        BasicShape::Polygon {
            fill_rule,
            round,
            points,
        } => {
            let round = (*round > 0.0).then(|| {
                let mut text = String::new();
                write_number(&mut text, (*round).into());
                text.push_str("px");
                text
            });
            let points: Vec<String> = points
                .iter()
                .map(|(x, y)| format!("{} {}", computed_text(x), computed_text(y)))
                .collect();
            polygon_to_css(*fill_rule, round.as_deref(), &points, dest);
        }
        BasicShape::Path {
            fill_rule,
            segments,
        } => path_to_css(*fill_rule, segments, dest),
    }
}

// --- clip-path ------------------------------------------------------------

/// `clip-path` as written; the box `border-box` when only the shape is.
/// The shape boxed: it is most of the size of every declaration's
/// `Value` otherwise.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedClipPath {
    None,
    Shape(Box<SpecifiedBasicShape>, GeometryBox),
    Box(GeometryBox),
    Url(CssUrlData),
}

/// `none | <url> | <basic-shape> || <geometry-box>`.
pub(crate) fn parse_clip_path<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, SpecifiedClipPath> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(SpecifiedClipPath::None);
    }
    if let Ok(url) = input.try_parse(crate::svg::parse_url) {
        return Ok(SpecifiedClipPath::Url(url));
    }
    if let Ok(shape) = input.try_parse(SpecifiedBasicShape::parse) {
        let reference = input
            .try_parse(parse_geometry_box)
            .unwrap_or(GeometryBox::BorderBox);
        return Ok(SpecifiedClipPath::Shape(Box::new(shape), reference));
    }
    let reference = parse_geometry_box(input)?;
    Ok(match input.try_parse(SpecifiedBasicShape::parse) {
        Ok(shape) => SpecifiedClipPath::Shape(Box::new(shape), reference),
        Err(_) => SpecifiedClipPath::Box(reference),
    })
}

pub(crate) fn compute_clip_path(value: &SpecifiedClipPath, cx: &ComputeContext<'_>) -> ClipPath {
    match value {
        SpecifiedClipPath::None => ClipPath::None,
        SpecifiedClipPath::Shape(shape, reference) => {
            ClipPath::Shape(Arc::new(shape.compute(cx)), *reference)
        }
        SpecifiedClipPath::Box(reference) => ClipPath::Box(*reference),
        // As a `fill`'s: resolved, a fragment alone as written.
        SpecifiedClipPath::Url(url) => ClipPath::Url(url.computed()),
    }
}

/// The shape, then its box unless `border-box` (Blink's
/// `ValueForBasicShapeOrNone`).
fn clip_path_to_css<S>(
    value: Option<(Option<&S>, GeometryBox)>,
    dest: &mut String,
    shape_to_css: impl Fn(&S, &mut String),
) {
    let Some((shape, reference)) = value else {
        dest.push_str("none");
        return;
    };
    match shape {
        Some(shape) => {
            shape_to_css(shape, dest);
            if reference != GeometryBox::BorderBox {
                dest.push(' ');
                dest.push_str(geometry_box_name(reference));
            }
        }
        None => dest.push_str(geometry_box_name(reference)),
    }
}

pub(crate) fn specified_clip_path_to_css(value: &SpecifiedClipPath, dest: &mut String) {
    let value = match value {
        SpecifiedClipPath::Url(url) => return crate::svg::url_to_css(url.specified(), dest),
        SpecifiedClipPath::None => None,
        SpecifiedClipPath::Shape(shape, reference) => Some((Some(&**shape), *reference)),
        SpecifiedClipPath::Box(reference) => Some((None, *reference)),
    };
    clip_path_to_css(value, dest, SpecifiedBasicShape::to_css);
}

pub(crate) fn computed_clip_path_to_css(value: &ClipPath, dest: &mut String) {
    let value = match value {
        ClipPath::Url(url) => return crate::svg::url_to_css(url, dest),
        ClipPath::None => None,
        ClipPath::Shape(shape, reference) => Some((Some(&**shape), *reference)),
        ClipPath::Box(reference) => Some((None, *reference)),
    };
    clip_path_to_css(value, dest, computed_shape_to_css);
}
