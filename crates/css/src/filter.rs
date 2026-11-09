//! `filter` (filter-effects 1, the filter functions; not `url()`):
//! parsing, computing and serializing.

use std::sync::Arc;

use computed_style::FilterFunction;
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::background::Angle;
use crate::shadow::SpecifiedShadow;
use crate::values::{ComputeContext, Length, ParseResult, write_number};

/// The functions that take an amount, a number or a percentage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountFunction {
    Brightness,
    Contrast,
    Grayscale,
    Invert,
    Opacity,
    Saturate,
    Sepia,
}

impl AmountFunction {
    fn name(self) -> &'static str {
        match self {
            Self::Brightness => "brightness",
            Self::Contrast => "contrast",
            Self::Grayscale => "grayscale",
            Self::Invert => "invert",
            Self::Opacity => "opacity",
            Self::Saturate => "saturate",
            Self::Sepia => "sepia",
        }
    }

    /// Whether more than 100% means 100% (filter-effects 1): the others
    /// go on brightening, contrasting, saturating.
    fn at_most_one(self) -> bool {
        matches!(
            self,
            Self::Grayscale | Self::Invert | Self::Opacity | Self::Sepia
        )
    }
}

/// An amount as written: a number, or a percentage (`50%` is 0.5).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Amount {
    value: f32,
    percent: bool,
}

/// One filter function as written; an argument left out is `None` (its
/// default: no blur, 1 for an amount, no turn).
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedFilter {
    Blur(Option<Length>),
    Amount(AmountFunction, Option<Amount>),
    HueRotate(Option<Angle>),
    DropShadow(SpecifiedShadow),
}

/// `none | <filter-function>+`; `none` is the empty list.
pub(crate) fn parse_filter<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[SpecifiedFilter]>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::new([]));
    }
    let mut functions = vec![parse_function(input)?];
    while let Ok(function) = input.try_parse(parse_function) {
        functions.push(function);
    }
    Ok(functions.into())
}

fn parse_function<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedFilter> {
    let location = input.current_source_location();
    let name = input.expect_function()?.clone();
    let amount = |name: &str| {
        Some(match_ignore_ascii_case! { name,
            "brightness" => AmountFunction::Brightness,
            "contrast" => AmountFunction::Contrast,
            "grayscale" => AmountFunction::Grayscale,
            "invert" => AmountFunction::Invert,
            "opacity" => AmountFunction::Opacity,
            "saturate" => AmountFunction::Saturate,
            "sepia" => AmountFunction::Sepia,
            _ => return None,
        })
    };
    input.parse_nested_block(|input| {
        if let Some(function) = amount(&name) {
            let value = input.try_parse(|i| parse_amount(i, function)).ok();
            return Ok(SpecifiedFilter::Amount(function, value));
        }
        match_ignore_ascii_case! { &name,
            "blur" => Ok(SpecifiedFilter::Blur(input.try_parse(|i| Length::parse(i, false)).ok())),
            "hue-rotate" => Ok(SpecifiedFilter::HueRotate(
                input.try_parse(Angle::parse_or_zero).ok(),
            )),
            "drop-shadow" => Ok(SpecifiedFilter::DropShadow(SpecifiedShadow::parse(input, true)?)),
            _ => Err(location.new_custom_error(())),
        }
    })
}

/// A non-negative number or percentage; one written past 100% is 100%
/// for the functions that stop there (a math function's result is
/// clamped as it is used).
fn parse_amount<'i>(
    input: &mut Parser<'i, '_>,
    function: AmountFunction,
) -> ParseResult<'i, Amount> {
    if let Ok(value) = input.try_parse(crate::calc::parse_number) {
        return Ok(Amount {
            value,
            percent: false,
        });
    }
    if let Ok(percent) = input.try_parse(crate::calc::parse_percentage) {
        return Ok(Amount {
            value: percent / 100.0,
            percent: true,
        });
    }
    let location = input.current_source_location();
    let (value, percent) = match *input.next()? {
        Token::Number { value, .. } => (value, false),
        Token::Percentage { unit_value, .. } => (unit_value, true),
        _ => return Err(location.new_custom_error(())),
    };
    if value < 0.0 {
        return Err(location.new_custom_error(()));
    }
    let value = if function.at_most_one() {
        value.min(1.0)
    } else {
        value
    };
    Ok(Amount { value, percent })
}

pub(crate) fn compute_filter(
    functions: &[SpecifiedFilter],
    cx: &ComputeContext,
) -> Arc<[FilterFunction]> {
    functions
        .iter()
        .map(|function| match function {
            SpecifiedFilter::Blur(length) => {
                FilterFunction::Blur(length.as_ref().map_or(0.0, |l| l.to_px(cx).max(0.0)))
            }
            SpecifiedFilter::HueRotate(angle) => {
                FilterFunction::HueRotate(angle.map_or(0.0, |angle| angle.degrees()))
            }
            SpecifiedFilter::DropShadow(shadow) => FilterFunction::DropShadow(shadow.compute(cx)),
            SpecifiedFilter::Amount(function, amount) => {
                let value = amount.map_or(1.0, |amount| amount.value).max(0.0);
                let value = if function.at_most_one() {
                    value.min(1.0)
                } else {
                    value
                };
                match function {
                    AmountFunction::Brightness => FilterFunction::Brightness(value),
                    AmountFunction::Contrast => FilterFunction::Contrast(value),
                    AmountFunction::Grayscale => FilterFunction::Grayscale(value),
                    AmountFunction::Invert => FilterFunction::Invert(value),
                    AmountFunction::Opacity => FilterFunction::Opacity(value),
                    AmountFunction::Saturate => FilterFunction::Saturate(value),
                    AmountFunction::Sepia => FilterFunction::Sepia(value),
                }
            }
        })
        .collect()
}

/// As written: an argument left out stays out (`blur()`).
pub(crate) fn filter_to_css(functions: &[SpecifiedFilter], dest: &mut String) {
    if functions.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, function) in functions.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        let name = match function {
            SpecifiedFilter::Blur(_) => "blur",
            SpecifiedFilter::Amount(function, _) => function.name(),
            SpecifiedFilter::HueRotate(_) => "hue-rotate",
            SpecifiedFilter::DropShadow(_) => "drop-shadow",
        };
        dest.push_str(name);
        dest.push('(');
        match function {
            SpecifiedFilter::Blur(length) => {
                if let Some(length) = length {
                    length.to_css(dest);
                }
            }
            SpecifiedFilter::Amount(_, amount) => {
                if let Some(amount) = amount {
                    if amount.percent {
                        write_number(dest, f64::from(amount.value) * 100.0);
                        dest.push('%');
                    } else {
                        write_number(dest, amount.value.into());
                    }
                }
            }
            SpecifiedFilter::HueRotate(angle) => {
                if let Some(angle) = angle {
                    angle.to_css(dest);
                }
            }
            SpecifiedFilter::DropShadow(shadow) => shadow.to_css(dest),
        }
        dest.push(')');
    }
}

/// The computed value: amounts as numbers, the blur and the turn in px
/// and degrees, the shadow's color resolved by `color`.
pub(crate) fn computed_filter_to_css(
    functions: &[FilterFunction],
    dest: &mut String,
    color: &dyn Fn(computed_style::StyleColor, &mut String),
) {
    if functions.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, function) in functions.iter().enumerate() {
        if i > 0 {
            dest.push(' ');
        }
        let (name, value) = match *function {
            FilterFunction::Blur(px) => {
                dest.push_str("blur(");
                write_number(dest, px.into());
                dest.push_str("px)");
                continue;
            }
            FilterFunction::HueRotate(degrees) => {
                dest.push_str("hue-rotate(");
                write_number(dest, degrees.into());
                dest.push_str("deg)");
                continue;
            }
            FilterFunction::DropShadow(shadow) => {
                dest.push_str("drop-shadow(");
                color(shadow.color, dest);
                for length in [shadow.x, shadow.y, shadow.blur] {
                    dest.push(' ');
                    write_number(dest, length.into());
                    dest.push_str("px");
                }
                dest.push(')');
                continue;
            }
            FilterFunction::Brightness(v) => ("brightness", v),
            FilterFunction::Contrast(v) => ("contrast", v),
            FilterFunction::Grayscale(v) => ("grayscale", v),
            FilterFunction::Invert(v) => ("invert", v),
            FilterFunction::Opacity(v) => ("opacity", v),
            FilterFunction::Saturate(v) => ("saturate", v),
            FilterFunction::Sepia(v) => ("sepia", v),
        };
        dest.push_str(name);
        dest.push('(');
        write_number(dest, value.into());
        dest.push(')');
    }
}

/// `isolation`.
pub(crate) fn isolation(keyword: &str) -> Option<computed_style::Isolation> {
    Some(match_ignore_ascii_case! { keyword,
        "auto" => computed_style::Isolation::Auto,
        "isolate" => computed_style::Isolation::Isolate,
        _ => return None,
    })
}

pub(crate) fn isolation_name(value: computed_style::Isolation) -> &'static str {
    match value {
        computed_style::Isolation::Auto => "auto",
        computed_style::Isolation::Isolate => "isolate",
    }
}
