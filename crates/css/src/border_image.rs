//! `border-image` and its longhands (css-backgrounds 3, "Border Images"):
//! parsing, computing and serializing them. machopo doesn't draw a border
//! image: they are parsed only.

use computed_style::{
    BorderImageOutset, BorderImageRepeat, BorderImageSlice, BorderImageWidth, ComputedStyle,
    NumberOrPercentage, Sides,
};
use cssparser::{Parser, match_ignore_ascii_case};

use crate::background::{SpecifiedImage, image_to_css, parse_image};
use crate::properties::Value;
use crate::values::{
    ComputeContext, Length, LengthPercentage, ParseResult, parse_number_at_least, write_number,
};

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

/// A `border-image-width` side as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedBorderImageWidth {
    Number(f32),
    LengthPercentage(LengthPercentage),
    Auto,
}

/// A `border-image-outset` side as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedBorderImageOutset {
    Number(f32),
    Length(Length),
}

/// One to four values, in `margin` order, for each of the four sides.
fn parse_sides<'i, T: Clone>(
    input: &mut Parser<'i, '_>,
    mut parse: impl FnMut(&mut Parser<'i, '_>) -> ParseResult<'i, T>,
) -> ParseResult<'i, [T; 4]> {
    let mut values = vec![parse(input)?];
    while values.len() < 4 {
        match input.try_parse(&mut parse) {
            Ok(value) => values.push(value),
            Err(_) => break,
        }
    }
    let top = values[0].clone();
    let right = values.get(1).unwrap_or(&top).clone();
    let bottom = values.get(2).unwrap_or(&top).clone();
    let left = values.get(3).unwrap_or(&right).clone();
    Ok([top, right, bottom, left])
}

fn sides<T>([top, right, bottom, left]: [T; 4]) -> Sides<T> {
    Sides {
        top,
        right,
        bottom,
        left,
    }
}

/// A non-negative `<percentage>` (a math function's clamped to 0).
fn parse_percentage<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    if let Ok(percent) = input.try_parse(crate::calc::parse_percentage) {
        return Ok(percent.max(0.0));
    }
    let percent = input.expect_percentage()? * 100.0;
    if percent < 0.0 {
        return invalid(input);
    }
    Ok(percent)
}

/// `[<number [0,∞]> | <percentage [0,∞]>]{1,4} && fill?`.
pub(crate) fn parse_slice<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Value> {
    let mut fill = input.try_parse(|i| i.expect_ident_matching("fill")).is_ok();
    let values = parse_sides(input, |input| {
        if let Ok(number) = input.try_parse(|i| parse_number_at_least(i, 0.0)) {
            return Ok(NumberOrPercentage::Number(number));
        }
        Ok(NumberOrPercentage::Percent(parse_percentage(input)?))
    })?;
    if !fill {
        fill = input.try_parse(|i| i.expect_ident_matching("fill")).is_ok();
    }
    Ok(Value::BorderImageSlice(BorderImageSlice {
        sides: sides(values),
        fill,
    }))
}

/// `[<length-percentage [0,∞]> | <number [0,∞]> | auto]{1,4}`: a number
/// before a length (`0` is a number).
pub(crate) fn parse_width<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Value> {
    let values = parse_sides(input, |input| {
        if let Ok(number) = input.try_parse(|i| parse_number_at_least(i, 0.0)) {
            return Ok(SpecifiedBorderImageWidth::Number(number));
        }
        if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
            return Ok(SpecifiedBorderImageWidth::Auto);
        }
        Ok(SpecifiedBorderImageWidth::LengthPercentage(
            LengthPercentage::parse(input, false)?,
        ))
    })?;
    Ok(Value::BorderImageWidth(values))
}

/// `[<length [0,∞]> | <number [0,∞]>]{1,4}`.
pub(crate) fn parse_outset<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Value> {
    let values = parse_sides(input, |input| {
        if let Ok(number) = input.try_parse(|i| parse_number_at_least(i, 0.0)) {
            return Ok(SpecifiedBorderImageOutset::Number(number));
        }
        Ok(SpecifiedBorderImageOutset::Length(Length::parse(
            input, false,
        )?))
    })?;
    Ok(Value::BorderImageOutset(values))
}

fn parse_one_repeat<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, BorderImageRepeat> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "stretch" => BorderImageRepeat::Stretch,
        "repeat" => BorderImageRepeat::Repeat,
        "round" => BorderImageRepeat::Round,
        "space" => BorderImageRepeat::Space,
        _ => return Err(location.new_custom_error(())),
    })
}

/// `[stretch | repeat | round | space]{1,2}`: horizontal, then vertical
/// (the horizontal one again if left out).
pub(crate) fn parse_repeat<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Value> {
    let horizontal = parse_one_repeat(input)?;
    let vertical = input.try_parse(parse_one_repeat).unwrap_or(horizontal);
    Ok(Value::BorderImageRepeat(horizontal, vertical))
}

/// The longhands' initial values, in the shorthand's order: source,
/// slice, width, outset, repeat.
pub(crate) fn initial_values() -> [Value; 5] {
    [
        Value::BorderImageSource(SpecifiedImage::None),
        Value::BorderImageSlice(BorderImageSlice {
            sides: Sides::all(NumberOrPercentage::Percent(100.0)),
            fill: false,
        }),
        Value::BorderImageWidth([const { SpecifiedBorderImageWidth::Number(1.0) }; 4]),
        Value::BorderImageOutset([const { SpecifiedBorderImageOutset::Number(0.0) }; 4]),
        Value::BorderImageRepeat(BorderImageRepeat::Stretch, BorderImageRepeat::Stretch),
    ]
}

/// `border-image`: `<'border-image-source'> || <'border-image-slice'> [ /
/// <'border-image-width'> | / <'border-image-width'>? / <'border-image-
/// outset'> ]? || <'border-image-repeat'>`, each longhand left out its
/// initial value (implicit).
pub(crate) fn parse_border_image<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Vec<(Value, bool)>> {
    let mut values: [Option<Value>; 5] = Default::default();
    loop {
        if values[0].is_none()
            && let Ok(image) = input.try_parse(parse_image)
        {
            values[0] = Some(Value::BorderImageSource(image));
            continue;
        }
        if values[1].is_none()
            && let Ok(slice) = input.try_parse(parse_slice)
        {
            values[1] = Some(slice);
            if input.try_parse(|i| i.expect_delim('/')).is_ok() {
                values[2] = input.try_parse(parse_width).ok();
                if input.try_parse(|i| i.expect_delim('/')).is_ok() {
                    values[3] = Some(parse_outset(input)?);
                } else if values[2].is_none() {
                    return invalid(input);
                }
            }
            continue;
        }
        if values[4].is_none()
            && let Ok(repeat) = input.try_parse(parse_repeat)
        {
            values[4] = Some(repeat);
            continue;
        }
        break;
    }
    if values.iter().all(Option::is_none) {
        return invalid(input);
    }
    Ok(values
        .into_iter()
        .zip(initial_values())
        .map(|(value, initial)| match value {
            Some(value) => (value, false),
            None => (initial, true),
        })
        .collect())
}

/// `border-image` from its longhands' values (Blink's
/// `StylePropertySerializer::BorderImagePropertyValue`): the source alone
/// while the others are initial, else all five.
pub(crate) fn border_image_to_css(values: &[(&Value, bool)]) -> String {
    let text = |value: &Value| {
        let mut text = String::new();
        crate::serialize::value_to_css(value, &mut text);
        text
    };
    let initial = initial_values();
    if values[1..]
        .iter()
        .zip(&initial[1..])
        .all(|((value, _), initial)| *value == initial)
    {
        return text(values[0].0);
    }
    format!(
        "{} {} / {} / {} {}",
        text(values[0].0),
        text(values[1].0),
        text(values[2].0),
        text(values[3].0),
        text(values[4].0)
    )
}

/// Each side written, as few as `margin`'s shorthand writes.
fn sides_to_css<T>(values: [&T; 4], dest: &mut String, write: impl Fn(&T, &mut String)) {
    let texts = values.map(|value| {
        let mut text = String::new();
        write(value, &mut text);
        text
    });
    dest.push_str(&crate::serialize::sides_to_css(&texts));
}

fn number_or_percentage_to_css(value: &NumberOrPercentage, dest: &mut String) {
    match value {
        NumberOrPercentage::Number(number) => write_number(dest, (*number).into()),
        NumberOrPercentage::Percent(percent) => crate::serialize::percent_to_css(*percent, dest),
    }
}

fn side_refs<T>(sides: &Sides<T>) -> [&T; 4] {
    [&sides.top, &sides.right, &sides.bottom, &sides.left]
}

pub(crate) fn slice_to_css(slice: &BorderImageSlice, dest: &mut String) {
    sides_to_css(side_refs(&slice.sides), dest, number_or_percentage_to_css);
    if slice.fill {
        dest.push_str(" fill");
    }
}

pub(crate) fn specified_width_to_css(values: &[SpecifiedBorderImageWidth; 4], dest: &mut String) {
    let [top, right, bottom, left] = values;
    sides_to_css(
        [top, right, bottom, left],
        dest,
        |value, dest| match value {
            SpecifiedBorderImageWidth::Number(number) => write_number(dest, (*number).into()),
            SpecifiedBorderImageWidth::LengthPercentage(v) => v.to_css(dest),
            SpecifiedBorderImageWidth::Auto => dest.push_str("auto"),
        },
    );
}

pub(crate) fn specified_outset_to_css(values: &[SpecifiedBorderImageOutset; 4], dest: &mut String) {
    let [top, right, bottom, left] = values;
    sides_to_css(
        [top, right, bottom, left],
        dest,
        |value, dest| match value {
            SpecifiedBorderImageOutset::Number(number) => write_number(dest, (*number).into()),
            SpecifiedBorderImageOutset::Length(v) => v.to_css(dest),
        },
    );
}

pub(crate) fn repeat_to_css(
    horizontal: BorderImageRepeat,
    vertical: BorderImageRepeat,
    dest: &mut String,
) {
    let name = |repeat| match repeat {
        BorderImageRepeat::Stretch => "stretch",
        BorderImageRepeat::Repeat => "repeat",
        BorderImageRepeat::Round => "round",
        BorderImageRepeat::Space => "space",
    };
    dest.push_str(name(horizontal));
    if vertical != horizontal {
        dest.push(' ');
        dest.push_str(name(vertical));
    }
}

pub(crate) fn compute_width(
    values: &[SpecifiedBorderImageWidth; 4],
    cx: &ComputeContext,
) -> Sides<BorderImageWidth> {
    sides(values.clone().map(|value| match value {
        SpecifiedBorderImageWidth::Number(number) => BorderImageWidth::Number(number),
        SpecifiedBorderImageWidth::LengthPercentage(v) => {
            BorderImageWidth::LengthPercentage(v.compute(cx))
        }
        SpecifiedBorderImageWidth::Auto => BorderImageWidth::Auto,
    }))
}

pub(crate) fn compute_outset(
    values: &[SpecifiedBorderImageOutset; 4],
    cx: &ComputeContext,
) -> Sides<BorderImageOutset> {
    sides(values.clone().map(|value| match value {
        SpecifiedBorderImageOutset::Number(number) => BorderImageOutset::Number(number),
        SpecifiedBorderImageOutset::Length(v) => BorderImageOutset::Px(v.to_px(cx)),
    }))
}

/// The computed values, as `getComputedStyle` writes each longhand.
pub(crate) fn source_to_css(style: &ComputedStyle, dest: &mut String) {
    image_to_css(Some(style), &style.rare.border_image.source, dest);
}

pub(crate) fn width_to_css(width: &Sides<BorderImageWidth>, dest: &mut String) {
    sides_to_css(side_refs(width), dest, |value, dest| match value {
        BorderImageWidth::Number(number) => write_number(dest, (*number).into()),
        BorderImageWidth::LengthPercentage(v) => crate::computed::length_percentage(*v, dest),
        BorderImageWidth::Auto => dest.push_str("auto"),
    });
}

pub(crate) fn outset_to_css(outset: &Sides<BorderImageOutset>, dest: &mut String) {
    sides_to_css(side_refs(outset), dest, |value, dest| match value {
        BorderImageOutset::Number(number) => write_number(dest, (*number).into()),
        BorderImageOutset::Px(px) => crate::computed::px_to_css(*px, dest),
    });
}
