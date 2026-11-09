//! The `text-decoration` longhands machopo has beyond the line, and
//! `text-underline-position` (css-text-decor 3 and 4): parsing, computing
//! and serializing. `text-shadow` is a list of shadows (`shadow`).

use computed_style::{TextDecorationStyle, TextDecorationThickness, TextUnderlinePosition};
use cssparser::{Parser, match_ignore_ascii_case};

use crate::values::{ComputeContext, LengthPercentage, ParseResult};

/// `solid | double | dotted | dashed | wavy`.
pub(crate) fn text_decoration_style(keyword: &str) -> Option<TextDecorationStyle> {
    Some(match_ignore_ascii_case! { keyword,
        "solid" => TextDecorationStyle::Solid,
        "double" => TextDecorationStyle::Double,
        "dotted" => TextDecorationStyle::Dotted,
        "dashed" => TextDecorationStyle::Dashed,
        "wavy" => TextDecorationStyle::Wavy,
        _ => return None,
    })
}

pub(crate) fn text_decoration_style_name(style: TextDecorationStyle) -> &'static str {
    match style {
        TextDecorationStyle::Solid => "solid",
        TextDecorationStyle::Double => "double",
        TextDecorationStyle::Dotted => "dotted",
        TextDecorationStyle::Dashed => "dashed",
        TextDecorationStyle::Wavy => "wavy",
    }
}

/// `text-decoration-thickness` as written.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedThickness {
    Auto,
    FromFont,
    Length(LengthPercentage),
}

impl SpecifiedThickness {
    /// `auto | from-font | <length-percentage>`, negative ones too (used
    /// as at least 1px).
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
            return Ok(Self::Auto);
        }
        if input
            .try_parse(|i| i.expect_ident_matching("from-font"))
            .is_ok()
        {
            return Ok(Self::FromFont);
        }
        LengthPercentage::parse(input, true).map(Self::Length)
    }

    pub(crate) fn compute(&self, cx: &ComputeContext) -> TextDecorationThickness {
        match self {
            Self::Auto => TextDecorationThickness::Auto,
            Self::FromFont => TextDecorationThickness::FromFont,
            Self::Length(length) => TextDecorationThickness::Length(length.compute(cx)),
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Auto => dest.push_str("auto"),
            Self::FromFont => dest.push_str("from-font"),
            Self::Length(length) => length.to_css(dest),
        }
    }
}

pub(crate) fn computed_thickness_to_css(thickness: TextDecorationThickness, dest: &mut String) {
    match thickness {
        TextDecorationThickness::Auto => dest.push_str("auto"),
        TextDecorationThickness::FromFont => dest.push_str("from-font"),
        TextDecorationThickness::Length(length) => crate::computed::length_percentage(length, dest),
    }
}

/// `auto | [ from-font | under ] || [ left | right ]`.
pub(crate) fn parse_underline_position<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, TextUnderlinePosition> {
    let mut position = TextUnderlinePosition::default();
    if input.try_parse(|i| i.expect_ident_matching("auto")).is_ok() {
        return Ok(position);
    }
    let (mut vertical, mut side) = (false, false);
    loop {
        let result: ParseResult<'i, ()> = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?.clone();
            let (taken, flag) = match_ignore_ascii_case! { &ident,
                "from-font" => (&mut vertical, &mut position.from_font),
                "under" => (&mut vertical, &mut position.under),
                "left" => (&mut side, &mut position.left),
                "right" => (&mut side, &mut position.right),
                _ => return Err(location.new_custom_error(())),
            };
            if *taken {
                return Err(location.new_custom_error(()));
            }
            *taken = true;
            *flag = true;
            Ok(())
        });
        if result.is_err() {
            break;
        }
    }
    if !vertical && !side {
        return Err(input.new_custom_error(()));
    }
    Ok(position)
}

/// The vertical part, then the side, as Chrome writes it.
pub(crate) fn underline_position_to_css(position: TextUnderlinePosition, dest: &mut String) {
    let parts: Vec<&str> = [
        (position.from_font, "from-font"),
        (position.under, "under"),
        (position.left, "left"),
        (position.right, "right"),
    ]
    .into_iter()
    .filter_map(|(set, name)| set.then_some(name))
    .collect();
    if parts.is_empty() {
        dest.push_str("auto");
    } else {
        dest.push_str(&parts.join(" "));
    }
}
