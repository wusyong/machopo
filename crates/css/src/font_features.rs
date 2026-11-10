//! The font longhands beside the ones that pick and size the font
//! (css-fonts 4): `font-stretch`, the `font-variant-*` longhands and the
//! `font-variant` shorthand, `font-feature-settings`,
//! `font-variation-settings`, `font-kerning`, `font-optical-sizing`,
//! `font-size-adjust`, `font-language-override`, the `font-synthesis-*`
//! longhands and the `font-synthesis` shorthand: parsing, computing and
//! serializing them.

use std::sync::Arc;

use computed_style::{
    AlternatesFunction, EastAsianVariant, EastAsianWidth, FontFeatureSetting, FontKerning,
    FontSizeAdjust, FontSizeAdjustMetric, FontStretch, FontVariantAlternates, FontVariantCaps,
    FontVariantEastAsian, FontVariantEmoji, FontVariantLigatures, FontVariantNumeric,
    FontVariantPosition, FontVariationSetting, NumericFigure, NumericFraction, NumericSpacing,
};
use cssparser::{Parser, match_ignore_ascii_case};

use crate::properties::{LonghandId, Value};
use crate::values::{ParseResult, parse_number, write_number};

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

/// Writes `parts`, one space apart; `empty` if there are none.
fn join(parts: &[&str], empty: &str, dest: &mut String) {
    if parts.is_empty() {
        dest.push_str(empty);
    } else {
        dest.push_str(&parts.join(" "));
    }
}

// --- font-stretch ----------------------------------------------------------

/// `font-stretch` as written: a keyword stays one (Chrome's).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpecifiedFontStretch {
    Keyword(FontStretch),
    /// Not negative; a math function's, as computed.
    Percent(f32),
}

impl SpecifiedFontStretch {
    pub(crate) fn percentage(self) -> f32 {
        match self {
            Self::Keyword(keyword) => keyword.percentage(),
            Self::Percent(percent) => percent,
        }
    }
}

pub(crate) fn stretch_name(stretch: FontStretch) -> &'static str {
    match stretch {
        FontStretch::UltraCondensed => "ultra-condensed",
        FontStretch::ExtraCondensed => "extra-condensed",
        FontStretch::Condensed => "condensed",
        FontStretch::SemiCondensed => "semi-condensed",
        FontStretch::Normal => "normal",
        FontStretch::SemiExpanded => "semi-expanded",
        FontStretch::Expanded => "expanded",
        FontStretch::ExtraExpanded => "extra-expanded",
        FontStretch::UltraExpanded => "ultra-expanded",
    }
}

/// `normal | <percentage [0,∞]> | <font-stretch-css3>`.
pub(crate) fn parse_stretch<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Value> {
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(Value::FontStretch(SpecifiedFontStretch::Keyword(
            FontStretch::Normal,
        )));
    }
    if let Ok(keyword) = input.try_parse(crate::font::parse_font_stretch) {
        return Ok(Value::FontStretch(SpecifiedFontStretch::Keyword(keyword)));
    }
    let percent = match input.try_parse(crate::calc::parse_percentage) {
        Ok(percent) => percent.max(0.0),
        Err(_) => {
            let percent = input.expect_percentage()? * 100.0;
            if percent < 0.0 {
                return invalid(input);
            }
            percent
        }
    };
    Ok(Value::FontStretch(SpecifiedFontStretch::Percent(percent)))
}

pub(crate) fn stretch_to_css(stretch: SpecifiedFontStretch, dest: &mut String) {
    match stretch {
        SpecifiedFontStretch::Keyword(keyword) => dest.push_str(stretch_name(keyword)),
        SpecifiedFontStretch::Percent(percent) => crate::serialize::percent_to_css(percent, dest),
    }
}

/// The keyword a percentage is, if one is (the `font` shorthand writes
/// only keywords).
pub(crate) fn stretch_keyword(percent: f32) -> Option<FontStretch> {
    use FontStretch::*;
    [
        UltraCondensed,
        ExtraCondensed,
        Condensed,
        SemiCondensed,
        Normal,
        SemiExpanded,
        Expanded,
        ExtraExpanded,
        UltraExpanded,
    ]
    .into_iter()
    .find(|keyword| keyword.percentage() == percent)
}

// --- font-variant-caps -----------------------------------------------------

fn caps_keyword(ident: &str) -> Option<FontVariantCaps> {
    Some(match_ignore_ascii_case! { ident,
        "normal" => FontVariantCaps::Normal,
        "small-caps" => FontVariantCaps::SmallCaps,
        "all-small-caps" => FontVariantCaps::AllSmallCaps,
        "petite-caps" => FontVariantCaps::PetiteCaps,
        "all-petite-caps" => FontVariantCaps::AllPetiteCaps,
        "unicase" => FontVariantCaps::Unicase,
        "titling-caps" => FontVariantCaps::TitlingCaps,
        _ => return None,
    })
}

pub(crate) fn parse_caps<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontVariantCaps> {
    let location = input.current_source_location();
    caps_keyword(input.expect_ident()?).ok_or_else(|| location.new_custom_error(()))
}

pub(crate) fn caps_name(caps: FontVariantCaps) -> &'static str {
    match caps {
        FontVariantCaps::Normal => "normal",
        FontVariantCaps::SmallCaps => "small-caps",
        FontVariantCaps::AllSmallCaps => "all-small-caps",
        FontVariantCaps::PetiteCaps => "petite-caps",
        FontVariantCaps::AllPetiteCaps => "all-petite-caps",
        FontVariantCaps::Unicase => "unicase",
        FontVariantCaps::TitlingCaps => "titling-caps",
    }
}

// --- font-variant-ligatures ------------------------------------------------

/// One ligature keyword into `ligatures`; `false` if its kind is set.
fn ligature_keyword(ligatures: &mut FontVariantLigatures, ident: &str) -> Option<bool> {
    let (slot, on) = match_ignore_ascii_case! { ident,
        "common-ligatures" => (&mut ligatures.common, true),
        "no-common-ligatures" => (&mut ligatures.common, false),
        "discretionary-ligatures" => (&mut ligatures.discretionary, true),
        "no-discretionary-ligatures" => (&mut ligatures.discretionary, false),
        "historical-ligatures" => (&mut ligatures.historical, true),
        "no-historical-ligatures" => (&mut ligatures.historical, false),
        "contextual" => (&mut ligatures.contextual, true),
        "no-contextual" => (&mut ligatures.contextual, false),
        _ => return None,
    };
    if slot.is_some() {
        return Some(false);
    }
    *slot = Some(on);
    Some(true)
}

/// `normal | none | [ <common-lig-values> || <discretionary-lig-values> ||
/// <historical-lig-values> || <contextual-alt-values> ]`.
pub(crate) fn parse_ligatures<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, FontVariantLigatures> {
    let mut ligatures = FontVariantLigatures::default();
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(ligatures);
    }
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        ligatures.none = true;
        return Ok(ligatures);
    }
    let mut any = false;
    while let Ok(()) = input.try_parse(|i| {
        let ident = i.expect_ident()?.clone();
        match ligature_keyword(&mut ligatures, &ident) {
            Some(true) => Ok(()),
            _ => invalid(i),
        }
    }) {
        any = true;
    }
    if !any {
        return invalid(input);
    }
    Ok(ligatures)
}

pub(crate) fn ligatures_to_css(ligatures: FontVariantLigatures, dest: &mut String) {
    if ligatures.none {
        dest.push_str("none");
        return;
    }
    let mut parts = Vec::new();
    for (value, on, off) in [
        (ligatures.common, "common-ligatures", "no-common-ligatures"),
        (
            ligatures.discretionary,
            "discretionary-ligatures",
            "no-discretionary-ligatures",
        ),
        (
            ligatures.historical,
            "historical-ligatures",
            "no-historical-ligatures",
        ),
        (ligatures.contextual, "contextual", "no-contextual"),
    ] {
        if let Some(value) = value {
            parts.push(if value { on } else { off });
        }
    }
    join(&parts, "normal", dest);
}

// --- font-variant-numeric --------------------------------------------------

fn numeric_keyword(numeric: &mut FontVariantNumeric, ident: &str) -> Option<bool> {
    fn set<T>(slot: &mut Option<T>, value: T) -> bool {
        slot.is_none() && {
            *slot = Some(value);
            true
        }
    }
    fn flag(slot: &mut bool) -> bool {
        !*slot && {
            *slot = true;
            true
        }
    }
    Some(match_ignore_ascii_case! { ident,
        "lining-nums" => set(&mut numeric.figure, NumericFigure::Lining),
        "oldstyle-nums" => set(&mut numeric.figure, NumericFigure::Oldstyle),
        "proportional-nums" => set(&mut numeric.spacing, NumericSpacing::Proportional),
        "tabular-nums" => set(&mut numeric.spacing, NumericSpacing::Tabular),
        "diagonal-fractions" => set(&mut numeric.fraction, NumericFraction::Diagonal),
        "stacked-fractions" => set(&mut numeric.fraction, NumericFraction::Stacked),
        "ordinal" => flag(&mut numeric.ordinal),
        "slashed-zero" => flag(&mut numeric.slashed_zero),
        _ => return None,
    })
}

/// `normal | [ <numeric-figure-values> || <numeric-spacing-values> ||
/// <numeric-fraction-values> || ordinal || slashed-zero ]`.
pub(crate) fn parse_numeric<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontVariantNumeric> {
    let mut numeric = FontVariantNumeric::default();
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(numeric);
    }
    let mut any = false;
    while let Ok(()) = input.try_parse(|i| {
        let ident = i.expect_ident()?.clone();
        match numeric_keyword(&mut numeric, &ident) {
            Some(true) => Ok(()),
            _ => invalid(i),
        }
    }) {
        any = true;
    }
    if !any {
        return invalid(input);
    }
    Ok(numeric)
}

pub(crate) fn numeric_to_css(numeric: FontVariantNumeric, dest: &mut String) {
    let mut parts = Vec::new();
    match numeric.figure {
        Some(NumericFigure::Lining) => parts.push("lining-nums"),
        Some(NumericFigure::Oldstyle) => parts.push("oldstyle-nums"),
        None => {}
    }
    match numeric.spacing {
        Some(NumericSpacing::Proportional) => parts.push("proportional-nums"),
        Some(NumericSpacing::Tabular) => parts.push("tabular-nums"),
        None => {}
    }
    match numeric.fraction {
        Some(NumericFraction::Diagonal) => parts.push("diagonal-fractions"),
        Some(NumericFraction::Stacked) => parts.push("stacked-fractions"),
        None => {}
    }
    if numeric.ordinal {
        parts.push("ordinal");
    }
    if numeric.slashed_zero {
        parts.push("slashed-zero");
    }
    join(&parts, "normal", dest);
}

// --- font-variant-east-asian -----------------------------------------------

fn east_asian_keyword(east_asian: &mut FontVariantEastAsian, ident: &str) -> Option<bool> {
    fn set<T>(slot: &mut Option<T>, value: T) -> bool {
        slot.is_none() && {
            *slot = Some(value);
            true
        }
    }
    Some(match_ignore_ascii_case! { ident,
        "jis78" => set(&mut east_asian.variant, EastAsianVariant::Jis78),
        "jis83" => set(&mut east_asian.variant, EastAsianVariant::Jis83),
        "jis90" => set(&mut east_asian.variant, EastAsianVariant::Jis90),
        "jis04" => set(&mut east_asian.variant, EastAsianVariant::Jis04),
        "simplified" => set(&mut east_asian.variant, EastAsianVariant::Simplified),
        "traditional" => set(&mut east_asian.variant, EastAsianVariant::Traditional),
        "full-width" => set(&mut east_asian.width, EastAsianWidth::Full),
        "proportional-width" => set(&mut east_asian.width, EastAsianWidth::Proportional),
        "ruby" => !east_asian.ruby && {
            east_asian.ruby = true;
            true
        },
        _ => return None,
    })
}

/// `normal | [ <east-asian-variant-values> || <east-asian-width-values> ||
/// ruby ]`.
pub(crate) fn parse_east_asian<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, FontVariantEastAsian> {
    let mut east_asian = FontVariantEastAsian::default();
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(east_asian);
    }
    let mut any = false;
    while let Ok(()) = input.try_parse(|i| {
        let ident = i.expect_ident()?.clone();
        match east_asian_keyword(&mut east_asian, &ident) {
            Some(true) => Ok(()),
            _ => invalid(i),
        }
    }) {
        any = true;
    }
    if !any {
        return invalid(input);
    }
    Ok(east_asian)
}

pub(crate) fn east_asian_to_css(east_asian: FontVariantEastAsian, dest: &mut String) {
    let mut parts = Vec::new();
    if let Some(variant) = east_asian.variant {
        parts.push(match variant {
            EastAsianVariant::Jis78 => "jis78",
            EastAsianVariant::Jis83 => "jis83",
            EastAsianVariant::Jis90 => "jis90",
            EastAsianVariant::Jis04 => "jis04",
            EastAsianVariant::Simplified => "simplified",
            EastAsianVariant::Traditional => "traditional",
        });
    }
    match east_asian.width {
        Some(EastAsianWidth::Full) => parts.push("full-width"),
        Some(EastAsianWidth::Proportional) => parts.push("proportional-width"),
        None => {}
    }
    if east_asian.ruby {
        parts.push("ruby");
    }
    join(&parts, "normal", dest);
}

// --- font-variant-alternates -----------------------------------------------

fn alternates_function_name(function: AlternatesFunction) -> &'static str {
    match function {
        AlternatesFunction::Stylistic => "stylistic",
        AlternatesFunction::Swash => "swash",
        AlternatesFunction::Ornaments => "ornaments",
        AlternatesFunction::Annotation => "annotation",
        AlternatesFunction::Styleset => "styleset",
        AlternatesFunction::CharacterVariant => "character-variant",
    }
}

/// `normal | [ stylistic(<ident>) || historical-forms || styleset(<ident>#)
/// || character-variant(<ident>#) || swash(<ident>) || ornaments(<ident>)
/// || annotation(<ident>) ]`.
pub(crate) fn parse_alternates<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, FontVariantAlternates> {
    let mut alternates = FontVariantAlternates::default();
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(alternates);
    }
    let mut functions: Vec<(AlternatesFunction, Arc<[Box<str>]>)> = Vec::new();
    let mut any = false;
    loop {
        if !alternates.historical_forms
            && input
                .try_parse(|i| i.expect_ident_matching("historical-forms"))
                .is_ok()
        {
            alternates.historical_forms = true;
            any = true;
            continue;
        }
        let Ok(function) = input.try_parse(|i| {
            let location = i.current_source_location();
            let name = i.expect_function()?.clone();
            let function = match_ignore_ascii_case! { &name,
                "stylistic" => AlternatesFunction::Stylistic,
                "swash" => AlternatesFunction::Swash,
                "ornaments" => AlternatesFunction::Ornaments,
                "annotation" => AlternatesFunction::Annotation,
                "styleset" => AlternatesFunction::Styleset,
                "character-variant" => AlternatesFunction::CharacterVariant,
                _ => return Err(location.new_custom_error(())),
            };
            let several = matches!(
                function,
                AlternatesFunction::Styleset | AlternatesFunction::CharacterVariant
            );
            let names = i.parse_nested_block(|i| {
                let names: Vec<Box<str>> = if several {
                    i.parse_comma_separated(|i| Ok(i.expect_ident()?.as_ref().into()))?
                } else {
                    vec![i.expect_ident()?.as_ref().into()]
                };
                Ok(names)
            })?;
            Ok::<_, cssparser::ParseError<'i, ()>>((function, names))
        }) else {
            break;
        };
        if functions.iter().any(|(f, _)| *f == function.0) {
            return invalid(input);
        }
        functions.push((function.0, function.1.into()));
        any = true;
    }
    if !any {
        return invalid(input);
    }
    functions.sort_by_key(|(function, _)| *function);
    alternates.functions = functions.into();
    Ok(alternates)
}

pub(crate) fn alternates_to_css(alternates: &FontVariantAlternates, dest: &mut String) {
    let function = |(function, names): &(AlternatesFunction, Arc<[Box<str>]>)| {
        let mut text = alternates_function_name(*function).to_owned();
        text.push('(');
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                text.push_str(", ");
            }
            cssparser::serialize_identifier(name, &mut text).expect("writing to a String");
        }
        text.push(')');
        text
    };
    // css-fonts 4's order: `stylistic()`, `historical-forms`, then the
    // rest.
    let (stylistic, rest): (Vec<_>, Vec<_>) = alternates
        .functions
        .iter()
        .partition(|(f, _)| *f == AlternatesFunction::Stylistic);
    let mut parts: Vec<String> = stylistic.into_iter().map(function).collect();
    if alternates.historical_forms {
        parts.push("historical-forms".to_owned());
    }
    parts.extend(rest.into_iter().map(function));
    if parts.is_empty() {
        dest.push_str("normal");
    } else {
        dest.push_str(&parts.join(" "));
    }
}

// --- font-variant-position, -emoji, font-kerning, font-optical-sizing ------

fn position_keyword(ident: &str) -> Option<FontVariantPosition> {
    Some(match_ignore_ascii_case! { ident,
        "normal" => FontVariantPosition::Normal,
        "sub" => FontVariantPosition::Sub,
        "super" => FontVariantPosition::Super,
        _ => return None,
    })
}

pub(crate) fn parse_position<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, FontVariantPosition> {
    let location = input.current_source_location();
    position_keyword(input.expect_ident()?).ok_or_else(|| location.new_custom_error(()))
}

pub(crate) fn position_name(position: FontVariantPosition) -> &'static str {
    match position {
        FontVariantPosition::Normal => "normal",
        FontVariantPosition::Sub => "sub",
        FontVariantPosition::Super => "super",
    }
}

fn emoji_keyword(ident: &str) -> Option<FontVariantEmoji> {
    Some(match_ignore_ascii_case! { ident,
        "normal" => FontVariantEmoji::Normal,
        "text" => FontVariantEmoji::Text,
        "emoji" => FontVariantEmoji::Emoji,
        "unicode" => FontVariantEmoji::Unicode,
        _ => return None,
    })
}

pub(crate) fn parse_emoji<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontVariantEmoji> {
    let location = input.current_source_location();
    emoji_keyword(input.expect_ident()?).ok_or_else(|| location.new_custom_error(()))
}

pub(crate) fn emoji_name(emoji: FontVariantEmoji) -> &'static str {
    match emoji {
        FontVariantEmoji::Normal => "normal",
        FontVariantEmoji::Text => "text",
        FontVariantEmoji::Emoji => "emoji",
        FontVariantEmoji::Unicode => "unicode",
    }
}

pub(crate) fn parse_kerning<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontKerning> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "auto" => FontKerning::Auto,
        "normal" => FontKerning::Normal,
        "none" => FontKerning::None,
        _ => return Err(location.new_custom_error(())),
    })
}

pub(crate) fn kerning_name(kerning: FontKerning) -> &'static str {
    match kerning {
        FontKerning::Auto => "auto",
        FontKerning::Normal => "normal",
        FontKerning::None => "none",
    }
}

/// `auto | none`, as whether it is `auto`.
pub(crate) fn parse_optical_sizing<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, bool> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "auto" => true,
        "none" => false,
        _ => return Err(location.new_custom_error(())),
    })
}

// --- font-feature-settings, font-variation-settings ------------------------

/// An OpenType tag: a string of four characters from U+20 to U+7E.
fn parse_tag<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, [u8; 4]> {
    let location = input.current_source_location();
    let string = input.expect_string()?;
    let bytes = string.as_bytes();
    if bytes.len() != 4 || !bytes.iter().all(|b| (0x20..=0x7E).contains(b)) {
        return Err(location.new_custom_error(()));
    }
    Ok([bytes[0], bytes[1], bytes[2], bytes[3]])
}

fn tag_to_css(tag: [u8; 4], dest: &mut String) {
    let text = std::str::from_utf8(&tag).expect("printable ASCII");
    cssparser::serialize_string(text, dest).expect("writing to a String");
}

/// `normal | <feature-tag-value>#`: a tag and `<integer [0,∞]> | on |
/// off`, 1 if left out; as written (`normal` is empty).
pub(crate) fn parse_feature_settings<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[FontFeatureSetting]>> {
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(Arc::new([]));
    }
    let settings = input.parse_comma_separated(|input| {
        let tag = parse_tag(input)?;
        let value = if let Ok(value) = input.try_parse(crate::values::parse_integer) {
            // Not negative.
            let Ok(value) = u32::try_from(value) else {
                return invalid(input);
            };
            value
        } else if input.try_parse(|i| i.expect_ident_matching("on")).is_ok() {
            1
        } else if input.try_parse(|i| i.expect_ident_matching("off")).is_ok() {
            0
        } else {
            1
        };
        Ok(FontFeatureSetting { tag, value })
    })?;
    Ok(settings.into())
}

/// Each tag, and its value unless it is 1; `normal` for none.
pub(crate) fn feature_settings_to_css(settings: &[FontFeatureSetting], dest: &mut String) {
    if settings.is_empty() {
        dest.push_str("normal");
        return;
    }
    for (i, setting) in settings.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        tag_to_css(setting.tag, dest);
        if setting.value != 1 {
            dest.push(' ');
            write_number(dest, setting.value.into());
        }
    }
}

/// `normal | [ <opentype-tag> <number> ]#`, as written.
pub(crate) fn parse_variation_settings<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[FontVariationSetting]>> {
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(Arc::new([]));
    }
    let settings = input.parse_comma_separated(|input| {
        let tag = parse_tag(input)?;
        let value = parse_number(input)?;
        Ok(FontVariationSetting { tag, value })
    })?;
    Ok(settings.into())
}

pub(crate) fn variation_settings_to_css(settings: &[FontVariationSetting], dest: &mut String) {
    if settings.is_empty() {
        dest.push_str("normal");
        return;
    }
    for (i, setting) in settings.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        tag_to_css(setting.tag, dest);
        dest.push(' ');
        write_number(dest, setting.value.into());
    }
}

/// The computed value of a list of settings: one per tag, the last
/// written, sorted by tag (Chrome's).
pub(crate) fn computed_settings<T: Copy>(settings: &[T], tag: impl Fn(&T) -> [u8; 4]) -> Arc<[T]> {
    let mut out: Vec<T> = Vec::with_capacity(settings.len());
    for setting in settings.iter().rev() {
        if !out.iter().any(|kept| tag(kept) == tag(setting)) {
            out.push(*setting);
        }
    }
    out.sort_by_key(&tag);
    out.into()
}

// --- font-size-adjust ------------------------------------------------------

/// `none | [ ex-height | cap-height | ch-width | ic-width | ic-height ]? [
/// from-font | <number [0,∞]> ]`.
pub(crate) fn parse_size_adjust<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontSizeAdjust> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(FontSizeAdjust::None);
    }
    let metric = input
        .try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            Ok::<_, cssparser::ParseError<'i, ()>>(match_ignore_ascii_case! { ident,
                "ex-height" => FontSizeAdjustMetric::ExHeight,
                "cap-height" => FontSizeAdjustMetric::CapHeight,
                "ch-width" => FontSizeAdjustMetric::ChWidth,
                "ic-width" => FontSizeAdjustMetric::IcWidth,
                "ic-height" => FontSizeAdjustMetric::IcHeight,
                _ => return Err(location.new_custom_error(())),
            })
        })
        .unwrap_or(FontSizeAdjustMetric::ExHeight);
    if input
        .try_parse(|i| i.expect_ident_matching("from-font"))
        .is_ok()
    {
        return Ok(FontSizeAdjust::Adjust(metric, None));
    }
    let value = crate::values::parse_number_at_least(input, 0.0)?;
    Ok(FontSizeAdjust::Adjust(metric, Some(value)))
}

/// The metric left out while it is `ex-height` (Chrome's).
pub(crate) fn size_adjust_to_css(adjust: FontSizeAdjust, dest: &mut String) {
    let FontSizeAdjust::Adjust(metric, value) = adjust else {
        dest.push_str("none");
        return;
    };
    let metric = match metric {
        FontSizeAdjustMetric::ExHeight => None,
        FontSizeAdjustMetric::CapHeight => Some("cap-height"),
        FontSizeAdjustMetric::ChWidth => Some("ch-width"),
        FontSizeAdjustMetric::IcWidth => Some("ic-width"),
        FontSizeAdjustMetric::IcHeight => Some("ic-height"),
    };
    if let Some(metric) = metric {
        dest.push_str(metric);
        dest.push(' ');
    }
    match value {
        Some(value) => write_number(dest, value.into()),
        None => dest.push_str("from-font"),
    }
}

// --- font-language-override ------------------------------------------------

/// `normal | <string>`: one to four characters from U+20 to U+7E, the
/// trailing spaces dropped (a tag's padding).
pub(crate) fn parse_language_override<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Option<Arc<str>>> {
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(None);
    }
    let location = input.current_source_location();
    let string = input.expect_string()?;
    let trimmed = string.trim_end_matches(' ');
    if string.is_empty()
        || string.len() > 4
        || trimmed.is_empty()
        || !string.bytes().all(|b| (0x20..=0x7E).contains(&b))
    {
        return Err(location.new_custom_error(()));
    }
    Ok(Some(trimmed.into()))
}

pub(crate) fn language_override_to_css(language: Option<&str>, dest: &mut String) {
    match language {
        None => dest.push_str("normal"),
        Some(language) => cssparser::serialize_string(language, dest).expect("writing to a String"),
    }
}

// --- font-synthesis --------------------------------------------------------

/// `auto | none`, as whether it is `auto`.
pub(crate) fn parse_synthesis_longhand<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, bool> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "auto" => true,
        "none" => false,
        _ => return Err(location.new_custom_error(())),
    })
}

pub(crate) fn auto_or_none(auto: bool) -> &'static str {
    if auto { "auto" } else { "none" }
}

/// `none | [ weight || style || small-caps ]`: weight, style, small caps.
pub(crate) fn parse_synthesis<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, [bool; 3]> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok([false; 3]);
    }
    let mut set = [false; 3];
    let mut any = false;
    while let Ok(index) = input.try_parse(|i| {
        let location = i.current_source_location();
        let ident = i.expect_ident()?;
        Ok::<_, cssparser::ParseError<'i, ()>>(match_ignore_ascii_case! { ident,
            "weight" => 0,
            "style" => 1,
            "small-caps" => 2,
            _ => return Err(location.new_custom_error(())),
        })
    }) {
        if set[index] {
            return invalid(input);
        }
        set[index] = true;
        any = true;
    }
    if !any {
        return invalid(input);
    }
    Ok(set)
}

pub(crate) fn synthesis_to_css(set: [bool; 3], dest: &mut String) {
    let parts: Vec<&str> = ["weight", "style", "small-caps"]
        .into_iter()
        .zip(set)
        .filter_map(|(name, on)| on.then_some(name))
        .collect();
    join(&parts, "none", dest);
}

// --- font-variant ----------------------------------------------------------

/// `font-variant`'s values, in its longhands' order (ligatures, numeric,
/// east-asian, caps, alternates, position, emoji).
pub(crate) fn parse_font_variant<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Vec<Value>> {
    let initial = || {
        (
            FontVariantLigatures::default(),
            FontVariantNumeric::default(),
            FontVariantEastAsian::default(),
            FontVariantCaps::Normal,
            FontVariantAlternates::default(),
            FontVariantPosition::Normal,
            FontVariantEmoji::Normal,
        )
    };
    let values = |(ligatures, numeric, east_asian, caps, alternates, position, emoji): (
        FontVariantLigatures,
        FontVariantNumeric,
        FontVariantEastAsian,
        FontVariantCaps,
        FontVariantAlternates,
        FontVariantPosition,
        FontVariantEmoji,
    )| {
        vec![
            Value::FontVariantLigatures(ligatures),
            Value::FontVariantNumeric(numeric),
            Value::FontVariantEastAsian(east_asian),
            Value::FontVariantCaps(caps),
            Value::FontVariantAlternates(alternates),
            Value::FontVariantPosition(position),
            Value::FontVariantEmoji(emoji),
        ]
    };
    let mut variant = initial();
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        return Ok(values(variant));
    }
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        variant.0.none = true;
        return Ok(values(variant));
    }
    let (mut caps, mut position, mut emoji) = (None, None, None);
    let mut alternates = None;
    let mut any = false;
    loop {
        if alternates.is_none()
            && let Ok(v) = input.try_parse(parse_alternates_part)
        {
            alternates = Some(v);
            any = true;
            continue;
        }
        let Ok(()) = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?.clone();
            let ok = if let Some(ok) = ligature_keyword(&mut variant.0, &ident) {
                ok
            } else if let Some(ok) = numeric_keyword(&mut variant.1, &ident) {
                ok
            } else if let Some(ok) = east_asian_keyword(&mut variant.2, &ident) {
                ok
            } else if let Some(v) = caps_keyword(&ident)
                && v != FontVariantCaps::Normal
            {
                caps.replace(v).is_none()
            } else if let Some(v) = position_keyword(&ident)
                && v != FontVariantPosition::Normal
            {
                position.replace(v).is_none()
            } else if let Some(v) = emoji_keyword(&ident)
                && v != FontVariantEmoji::Normal
            {
                emoji.replace(v).is_none()
            } else {
                false
            };
            if ok {
                Ok(())
            } else {
                Err(location.new_custom_error::<(), ()>(()))
            }
        }) else {
            break;
        };
        any = true;
    }
    if !any {
        return invalid(input);
    }
    variant.3 = caps.unwrap_or(FontVariantCaps::Normal);
    variant.4 = alternates.unwrap_or_default();
    variant.5 = position.unwrap_or(FontVariantPosition::Normal);
    variant.6 = emoji.unwrap_or(FontVariantEmoji::Normal);
    Ok(values(variant))
}

/// `font-variant-alternates`' values but `normal`, together (they are
/// next to each other in `font-variant`, as css-fonts 4's grammar goes).
fn parse_alternates_part<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontVariantAlternates> {
    let start = input.state();
    if input
        .try_parse(|i| i.expect_ident_matching("normal"))
        .is_ok()
    {
        input.reset(&start);
        return invalid(input);
    }
    parse_alternates(input)
}

/// `font-variant` from its longhands' values: `normal` while they all
/// are, `none` while only the ligatures are (`none`), else what isn't
/// normal in css-fonts 4's order; `None` for `none` with anything else.
/// A longhand that a system font left pending counts as normal (Chrome's:
/// `font: menu` reads back `font-variant: normal`).
pub(crate) fn font_variant_to_css(values: &[(&Value, bool)]) -> Option<String> {
    let initial = variant_initial_values();
    let values: Vec<&Value> = values
        .iter()
        .zip(&initial)
        .map(|((value, _), initial)| match value {
            Value::SystemFont(_) => initial,
            _ => *value,
        })
        .collect();
    let [
        Value::FontVariantLigatures(ligatures),
        Value::FontVariantNumeric(numeric),
        Value::FontVariantEastAsian(east_asian),
        Value::FontVariantCaps(caps),
        Value::FontVariantAlternates(alternates),
        Value::FontVariantPosition(position),
        Value::FontVariantEmoji(emoji),
    ] = values[..]
    else {
        return None;
    };
    let others_normal = *numeric == FontVariantNumeric::default()
        && *east_asian == FontVariantEastAsian::default()
        && *caps == FontVariantCaps::Normal
        && *alternates == FontVariantAlternates::default()
        && *position == FontVariantPosition::Normal
        && *emoji == FontVariantEmoji::Normal;
    if ligatures.none {
        return others_normal.then(|| "none".to_owned());
    }
    let mut parts = Vec::new();
    let mut part = |write: &dyn Fn(&mut String), normal: bool| {
        if !normal {
            let mut text = String::new();
            write(&mut text);
            parts.push(text);
        }
    };
    part(
        &|dest| ligatures_to_css(*ligatures, dest),
        *ligatures == FontVariantLigatures::default(),
    );
    part(
        &|dest| dest.push_str(caps_name(*caps)),
        *caps == FontVariantCaps::Normal,
    );
    part(
        &|dest| alternates_to_css(alternates, dest),
        *alternates == FontVariantAlternates::default(),
    );
    part(
        &|dest| numeric_to_css(*numeric, dest),
        *numeric == FontVariantNumeric::default(),
    );
    part(
        &|dest| east_asian_to_css(*east_asian, dest),
        *east_asian == FontVariantEastAsian::default(),
    );
    part(
        &|dest| dest.push_str(position_name(*position)),
        *position == FontVariantPosition::Normal,
    );
    part(
        &|dest| dest.push_str(emoji_name(*emoji)),
        *emoji == FontVariantEmoji::Normal,
    );
    Some(if parts.is_empty() {
        "normal".to_owned()
    } else {
        parts.join(" ")
    })
}

/// `font-variant`'s longhands' initial values, in its order.
pub(crate) fn variant_initial_values() -> [Value; 7] {
    crate::properties::FONT_VARIANT
        .map(|id| font_reset_value(id).unwrap_or_else(|| unreachable!("{id:?} is font-variant's")))
}

/// What the `font` shorthand resets longhand `id` to, the initial value:
/// for `font-variant`'s longhands (but `small-caps`, which it can set) and
/// the others it has no syntax for; `None` for one it sets.
pub(crate) fn font_reset_value(id: LonghandId) -> Option<Value> {
    Some(match id {
        LonghandId::FontVariantLigatures => {
            Value::FontVariantLigatures(FontVariantLigatures::default())
        }
        LonghandId::FontVariantNumeric => Value::FontVariantNumeric(FontVariantNumeric::default()),
        LonghandId::FontVariantEastAsian => {
            Value::FontVariantEastAsian(FontVariantEastAsian::default())
        }
        LonghandId::FontVariantCaps => Value::FontVariantCaps(FontVariantCaps::Normal),
        LonghandId::FontVariantAlternates => {
            Value::FontVariantAlternates(FontVariantAlternates::default())
        }
        LonghandId::FontVariantPosition => Value::FontVariantPosition(FontVariantPosition::Normal),
        LonghandId::FontVariantEmoji => Value::FontVariantEmoji(FontVariantEmoji::Normal),
        LonghandId::FontSizeAdjust => Value::FontSizeAdjust(FontSizeAdjust::None),
        LonghandId::FontLanguageOverride => Value::FontLanguageOverride(None),
        LonghandId::FontKerning => Value::FontKerning(FontKerning::Auto),
        LonghandId::FontOpticalSizing => Value::FontOpticalSizing(true),
        LonghandId::FontFeatureSettings => Value::FontFeatureSettings(Arc::new([])),
        LonghandId::FontVariationSettings => Value::FontVariationSettings(Arc::new([])),
        _ => return None,
    })
}
