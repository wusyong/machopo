//! Font and text values: `font-family`, `font-size`, `font-weight`,
//! `line-height`, and the `font` shorthand's parts.

use std::sync::Arc;

use computed_style::{FontFamily, FontStretch, FontStyle, GenericFamily, LineHeight};
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::values::{
    ComputeContext, Length, LengthPercentage, LengthUnit, ParseResult, write_number,
};

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

/// `font-family`: a comma-separated list of names (quoted, or unquoted
/// identifiers joined by single spaces) and generic families. An unquoted
/// name can't hold a word that is a keyword of its own (a generic family, a
/// CSS-wide keyword, `default`): `cursive serif` and `inherit` are invalid
/// there (css-fonts 4, "font-family").
pub(crate) fn parse_font_family<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[FontFamily]>> {
    let families = input.parse_comma_separated(|input| {
        if let Ok(name) = input.try_parse(|i| i.expect_string().map(|s| s.as_ref().to_owned())) {
            return Ok(FontFamily::Named(name.into()));
        }
        let first = input.expect_ident()?.as_ref().to_owned();
        let mut words = vec![first];
        while let Ok(word) = input.try_parse(|i| i.expect_ident().map(|s| s.as_ref().to_owned())) {
            words.push(word);
        }
        if let [word] = &words[..]
            && let Some(generic) = generic_family(word)
        {
            return Ok(FontFamily::Generic(generic));
        }
        if words.iter().any(|word| is_reserved_family_word(word)) {
            return invalid(input);
        }
        Ok(FontFamily::Named(words.join(" ").into()))
    })?;
    Ok(families.into())
}

/// `font-family`'s value: generic families as keywords; a name unquoted if
/// it reads back as the same name (identifiers separated by single spaces,
/// none of them a keyword the property takes on its own), quoted otherwise.
pub(crate) fn font_family_to_css(families: &[FontFamily], dest: &mut String) {
    for (i, family) in families.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        match family {
            FontFamily::Generic(generic) => dest.push_str(generic_name(*generic)),
            FontFamily::Named(name) if is_unquoted_family_name(name) => dest.push_str(name),
            FontFamily::Named(name) => {
                cssparser::serialize_string(name, dest).expect("writing to a String");
            }
        }
    }
}

fn generic_family(word: &str) -> Option<GenericFamily> {
    match_ignore_ascii_case! { word,
        "serif" => Some(GenericFamily::Serif),
        "sans-serif" => Some(GenericFamily::SansSerif),
        "monospace" => Some(GenericFamily::Monospace),
        "cursive" => Some(GenericFamily::Cursive),
        "fantasy" => Some(GenericFamily::Fantasy),
        "system-ui" => Some(GenericFamily::SystemUi),
        _ => None,
    }
}

/// A word an unquoted family name can't hold: a generic family's, a
/// CSS-wide keyword, `default`.
fn is_reserved_family_word(word: &str) -> bool {
    generic_family(word).is_some()
        || match_ignore_ascii_case! { word,
            "initial" | "inherit" | "unset" | "revert" | "revert-layer" | "revert-rule"
                | "default" => true,
            _ => false,
        }
}

fn generic_name(generic: GenericFamily) -> &'static str {
    match generic {
        GenericFamily::Serif => "serif",
        GenericFamily::SansSerif => "sans-serif",
        GenericFamily::Monospace => "monospace",
        GenericFamily::Cursive => "cursive",
        GenericFamily::Fantasy => "fantasy",
        GenericFamily::SystemUi => "system-ui",
    }
}

/// A canvas's families as Chromium's `font` getter writes them
/// (`SerializeFontFamily`): a name unquoted only if it is one identifier,
/// quoted otherwise.
pub(crate) fn canvas_font_family_to_css(families: &[FontFamily], dest: &mut String) {
    for (i, family) in families.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        match family {
            FontFamily::Generic(generic) => dest.push_str(generic_name(*generic)),
            FontFamily::Named(name) if is_identifier(name) => dest.push_str(name),
            FontFamily::Named(name) => {
                cssparser::serialize_string(name, dest).expect("writing to a String");
            }
        }
    }
}

fn is_unquoted_family_name(name: &str) -> bool {
    name.split(' ')
        .all(|word| is_identifier(word) && !is_reserved_family_word(word))
}

/// Whether `word` is an `<ident-token>` written without escapes.
fn is_identifier(word: &str) -> bool {
    let start = |c: char| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii();
    let mut chars = word.chars();
    let valid_start = match chars.next() {
        Some('-') => match chars.next() {
            Some(c) => start(c) || c == '-',
            None => false,
        },
        Some(c) => start(c),
        None => false,
    };
    valid_start && chars.all(|c| start(c) || c.is_ascii_digit() || c == '-')
}

/// `font-size`, specified.
#[derive(Debug, Clone, PartialEq)]
pub enum FontSize {
    /// An absolute keyword (`medium`, ...): its index in
    /// `FONT_SIZE_KEYWORDS`.
    Keyword(usize),
    Larger,
    Smaller,
    LengthPercentage(LengthPercentage),
}

/// Chromium's sizes for the absolute keywords at a 16px default.
const FONT_SIZE_KEYWORDS: [(&str, f32); 8] = [
    ("xx-small", 9.0),
    ("x-small", 10.0),
    ("small", 13.0),
    ("medium", 16.0),
    ("large", 18.0),
    ("x-large", 24.0),
    ("xx-large", 32.0),
    ("xxx-large", 48.0),
];

/// The ratio between adjacent sizes for `larger` / `smaller`.
const FONT_SIZE_RATIO: f32 = 1.2;

impl FontSize {
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        let keyword: ParseResult<'i, Self> = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            if ident.eq_ignore_ascii_case("larger") {
                return Ok(Self::Larger);
            }
            if ident.eq_ignore_ascii_case("smaller") {
                return Ok(Self::Smaller);
            }
            FONT_SIZE_KEYWORDS
                .iter()
                .position(|(name, _)| ident.eq_ignore_ascii_case(name))
                .map(Self::Keyword)
                .ok_or_else(|| location.new_custom_error(()))
        });
        if let Ok(size) = keyword {
            return Ok(size);
        }
        LengthPercentage::parse(input, false).map(Self::LengthPercentage)
    }

    /// `em` and `%` are relative to the parent's font size here. From 0
    /// to [`computed_style::MAX_FONT_SIZE`], NaN 0 (Blink's
    /// `FontBuilder::SetSize`): a size taken from the parent's grows no
    /// further.
    pub(crate) fn compute(&self, cx: &ComputeContext) -> f32 {
        let size = self.unclamped(cx);
        if size.is_nan() {
            0.0
        } else {
            size.clamp(0.0, computed_style::MAX_FONT_SIZE)
        }
    }

    fn unclamped(&self, cx: &ComputeContext) -> f32 {
        match self {
            Self::Keyword(i) => FONT_SIZE_KEYWORDS[*i].1,
            Self::Larger => cx.parent_font_size * FONT_SIZE_RATIO,
            Self::Smaller => cx.parent_font_size / FONT_SIZE_RATIO,
            Self::LengthPercentage(LengthPercentage::Length(Length::Dimension {
                value: em,
                unit: LengthUnit::Em,
            })) => em * cx.parent_font_size,
            Self::LengthPercentage(LengthPercentage::Length(length)) => length.to_px(cx),
            Self::LengthPercentage(LengthPercentage::Percent(p)) => p / 100.0 * cx.parent_font_size,
            Self::LengthPercentage(v @ LengthPercentage::Calc(_)) => {
                v.compute(cx).resolve(cx.parent_font_size)
            }
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Keyword(i) => dest.push_str(FONT_SIZE_KEYWORDS[*i].0),
            Self::Larger => dest.push_str("larger"),
            Self::Smaller => dest.push_str("smaller"),
            Self::LengthPercentage(v) => v.to_css(dest),
        }
    }
}

/// `font-weight`, specified.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FontWeight {
    /// 400, serialized as the keyword.
    Normal,
    /// 700, serialized as the keyword.
    Bold,
    Absolute(f32),
    Bolder,
    Lighter,
}

impl FontWeight {
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        let location = input.current_source_location();
        if let Ok(value) = input.try_parse(crate::calc::parse_number) {
            return Ok(Self::Absolute(value.clamp(1.0, 1000.0)));
        }
        match *input.next()? {
            Token::Number { value, .. } if (1.0..=1000.0).contains(&value) => {
                Ok(Self::Absolute(value))
            }
            Token::Ident(ref ident) => match_ignore_ascii_case! { ident,
                "normal" => Ok(Self::Normal),
                "bold" => Ok(Self::Bold),
                "bolder" => Ok(Self::Bolder),
                "lighter" => Ok(Self::Lighter),
                _ => Err(location.new_custom_error(())),
            },
            _ => Err(location.new_custom_error(())),
        }
    }

    /// css-fonts' table for relative weights.
    pub(crate) fn compute(self, parent: f32) -> f32 {
        match self {
            Self::Normal => 400.0,
            Self::Bold => 700.0,
            Self::Absolute(weight) => weight,
            Self::Bolder if parent < 350.0 => 400.0,
            Self::Bolder if parent < 550.0 => 700.0,
            Self::Bolder if parent < 900.0 => 900.0,
            Self::Bolder => parent,
            Self::Lighter if parent < 100.0 => parent,
            Self::Lighter if parent < 550.0 => 100.0,
            Self::Lighter if parent < 750.0 => 400.0,
            Self::Lighter => 700.0,
        }
    }

    pub(crate) fn to_css(self, dest: &mut String) {
        match self {
            Self::Normal => dest.push_str("normal"),
            Self::Bold => dest.push_str("bold"),
            Self::Absolute(weight) => write_number(dest, weight.into()),
            Self::Bolder => dest.push_str("bolder"),
            Self::Lighter => dest.push_str("lighter"),
        }
    }
}

/// `line-height`, specified.
#[derive(Debug, Clone, PartialEq)]
pub enum LineHeightValue {
    Normal,
    Number(f32),
    LengthPercentage(LengthPercentage),
}

impl LineHeightValue {
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        if input
            .try_parse(|i| i.expect_ident_matching("normal"))
            .is_ok()
        {
            return Ok(Self::Normal);
        }
        if let Ok(number) = input.try_parse(|i| crate::values::parse_number_at_least(i, 0.0)) {
            return Ok(Self::Number(number));
        }
        LengthPercentage::parse(input, false).map(Self::LengthPercentage)
    }

    /// Relative to the element's own font size.
    pub(crate) fn compute(&self, cx: &ComputeContext) -> LineHeight {
        match self {
            Self::Normal => LineHeight::Normal,
            Self::Number(n) => LineHeight::Number(*n),
            Self::LengthPercentage(v) => LineHeight::Px(v.compute(cx).resolve(cx.font_size)),
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Normal => dest.push_str("normal"),
            Self::Number(n) => write_number(dest, (*n).into()),
            Self::LengthPercentage(v) => v.to_css(dest),
        }
    }
}

/// `font-style`'s keywords.
pub(crate) fn parse_font_style<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontStyle> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "normal" => FontStyle::Normal,
        "italic" => FontStyle::Italic,
        "oblique" => FontStyle::Oblique,
        _ => return Err(location.new_custom_error(())),
    })
}

/// CSS Fonts 3's `font-stretch` keywords.
pub(crate) fn parse_font_stretch<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, FontStretch> {
    let location = input.current_source_location();
    let ident = input.expect_ident()?;
    Ok(match_ignore_ascii_case! { ident,
        "ultra-condensed" => FontStretch::UltraCondensed,
        "extra-condensed" => FontStretch::ExtraCondensed,
        "condensed" => FontStretch::Condensed,
        "semi-condensed" => FontStretch::SemiCondensed,
        "semi-expanded" => FontStretch::SemiExpanded,
        "expanded" => FontStretch::Expanded,
        "extra-expanded" => FontStretch::ExtraExpanded,
        "ultra-expanded" => FontStretch::UltraExpanded,
        _ => return Err(location.new_custom_error(())),
    })
}

/// A system font keyword, which only the `font` shorthand takes: CSS 2's
/// and Chrome's `-webkit-*-control`, the form controls' (Blink's
/// `LayoutThemeFontProvider::SystemFont`). Its longhands are pending till
/// they compute (Blink's `CSSPendingSystemFontValue`): they read back as
/// nothing, the shorthand as the keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFont {
    Caption,
    Icon,
    Menu,
    MessageBox,
    SmallCaption,
    StatusBar,
    WebkitMiniControl,
    WebkitSmallControl,
    WebkitControl,
}

impl SystemFont {
    /// The keyword, lowercase.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Caption => "caption",
            Self::Icon => "icon",
            Self::Menu => "menu",
            Self::MessageBox => "message-box",
            Self::SmallCaption => "small-caption",
            Self::StatusBar => "status-bar",
            Self::WebkitMiniControl => "-webkit-mini-control",
            Self::WebkitSmallControl => "-webkit-small-control",
            Self::WebkitControl => "-webkit-control",
        }
    }

    fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        let location = input.current_source_location();
        let ident = input.expect_ident()?;
        Ok(match_ignore_ascii_case! { ident,
            "caption" => Self::Caption,
            "icon" => Self::Icon,
            "menu" => Self::Menu,
            "message-box" => Self::MessageBox,
            "small-caption" => Self::SmallCaption,
            "status-bar" => Self::StatusBar,
            "-webkit-mini-control" => Self::WebkitMiniControl,
            "-webkit-small-control" => Self::WebkitSmallControl,
            "-webkit-control" => Self::WebkitControl,
            _ => return Err(location.new_custom_error(())),
        })
    }

    /// Its size in px, Chrome's on Windows (152): the controls' 13.333px
    /// (10pt), the system's menu and status bar fonts' 12px, and the
    /// default font size for the rest. Its family is `system-ui`, its
    /// style, weight and line height the initial ones.
    pub(crate) fn size(self) -> f32 {
        match self {
            Self::WebkitMiniControl | Self::WebkitSmallControl | Self::WebkitControl => 40.0 / 3.0,
            Self::Menu | Self::SmallCaption | Self::StatusBar => 12.0,
            Self::Caption | Self::Icon | Self::MessageBox => 16.0,
        }
    }
}

/// `font` as written: a system font, or its parts.
pub(crate) fn parse_system_font<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SystemFont> {
    let font = SystemFont::parse(input)?;
    input.expect_exhausted()?;
    Ok(font)
}

/// The `font` shorthand's parts as written; those left out are `None`
/// (their initial values).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontShorthand {
    pub(crate) style: Option<FontStyle>,
    pub(crate) weight: Option<FontWeight>,
    pub(crate) small_caps: bool,
    pub(crate) stretch: Option<FontStretch>,
    pub(crate) size: FontSize,
    pub(crate) line_height: LineHeightValue,
    pub(crate) family: Arc<[FontFamily]>,
}

/// The `font` shorthand: `[style || small-caps || weight || stretch]? size
/// [/ line-height]? family` (CSS Fonts 4's, but only `small-caps` of
/// `font-variant-caps` and only the keywords of `font-stretch`), `normal`
/// standing for any part before the size. A system font (`caption`,
/// `menu`...) is [`parse_system_font`]'s.
pub(crate) fn parse_font_shorthand<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, FontShorthand> {
    let (mut style, mut weight, mut small_caps, mut stretch) = (None, None, false, None);
    for _ in 0..4 {
        if input
            .try_parse(|i| i.expect_ident_matching("normal"))
            .is_ok()
        {
            continue;
        }
        if style.is_none()
            && let Ok(v) = input.try_parse(parse_font_style)
        {
            style = Some(v);
            continue;
        }
        if weight.is_none()
            && let Ok(v) = input.try_parse(FontWeight::parse)
        {
            weight = Some(v);
            continue;
        }
        if !small_caps
            && input
                .try_parse(|i| i.expect_ident_matching("small-caps"))
                .is_ok()
        {
            small_caps = true;
            continue;
        }
        if stretch.is_none()
            && let Ok(v) = input.try_parse(parse_font_stretch)
        {
            stretch = Some(v);
            continue;
        }
        break;
    }
    let size = FontSize::parse(input)?;
    let line_height = if input.try_parse(|i| i.expect_delim('/')).is_ok() {
        LineHeightValue::parse(input)?
    } else {
        LineHeightValue::Normal
    };
    let family = parse_font_family(input)?;
    Ok(FontShorthand {
        style,
        weight,
        small_caps,
        stretch,
        size,
        line_height,
        family,
    })
}
