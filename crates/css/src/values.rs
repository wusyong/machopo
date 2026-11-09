//! Specified values: what a declaration says, before the cascade turns it
//! into a computed value.

use std::cell::Cell;
use std::sync::Arc;

use base::numerics::{clamp_floor, clamp_round, f32_from_f64};
use computed_style::{LengthPercentage as ComputedLengthPercentage, StyleColor};
use cssparser::{Parser, Token, match_ignore_ascii_case};

use crate::calc::{Calc, Kind as CalcKind};
use gfx::Color;

/// Any parse failure: the declaration is invalid and dropped.
pub type ParseResult<'i, T> = Result<T, cssparser::ParseError<'i, ()>>;

fn invalid<'i, T>(input: &Parser<'i, '_>) -> ParseResult<'i, T> {
    Err(input.new_custom_error(()))
}

/// A non-negative `<length>` (a math function of lengths too) in px,
/// outside an element: `em` and `rem` are `font_size`, viewport units of
/// `viewport`. HTML's `sizes` attribute's lengths.
pub fn parse_length_px<'i>(
    input: &mut Parser<'i, '_>,
    font_size: f32,
    viewport: gfx::SizeF,
) -> ParseResult<'i, f32> {
    let length = Length::parse(input, false)?;
    let used = Cell::new(false);
    let cx = ComputeContext {
        font_size,
        parent_font_size: font_size,
        root_font_size: font_size,
        viewport: Some((viewport, &used)),
        system_colors: &gfx::SystemColors::DEFAULT,
        link_colors: TextLinkColors::DEFAULT,
    };
    // A math function that works out to NaN or an infinity is no size
    // (`calc(0px / 0)`): a source size of it is skipped, as WPT's
    // `parse-a-sizes-attribute` has it, rather than clamped.
    if let Length::Calc(calc) = &length
        && !calc.evaluate(&cx).px.is_finite()
    {
        return Err(input.new_custom_error(()));
    }
    Ok(length.to_px(&cx))
}

/// `<number>`, or a math function of numbers.
pub(crate) fn parse_number<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
    if let Ok(value) = input.try_parse(crate::calc::parse_number) {
        return Ok(value);
    }
    Ok(input.expect_number()?)
}

/// `<number>` at least `min`: a number below it is invalid, a math
/// function's result is clamped to it (css-values-4).
pub(crate) fn parse_number_at_least<'i>(
    input: &mut Parser<'i, '_>,
    min: f32,
) -> ParseResult<'i, f32> {
    if let Ok(value) = input.try_parse(crate::calc::parse_number) {
        return Ok(value.max(min));
    }
    let value = input.expect_number()?;
    if value < min {
        return invalid(input);
    }
    Ok(value)
}

/// `<integer>`, or a math function of numbers, rounded to the nearest
/// integer (halves up).
pub(crate) fn parse_integer<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, i32> {
    if let Ok(value) = input.try_parse(crate::calc::parse_number) {
        return Ok(clamp_floor(value + 0.5));
    }
    Ok(input.expect_integer()?)
}

/// What a computed value depends on besides the declaration itself.
pub struct ComputeContext<'a> {
    /// The element's own computed font size: what `em` means everywhere
    /// but in `font-size` itself.
    pub font_size: f32,
    /// What `em` means in `font-size`.
    pub parent_font_size: f32,
    /// What `rem` means: the root element's font size (for the root
    /// itself, the initial one).
    pub root_font_size: f32,
    /// What viewport units (`vw`) are of: the frame's viewport in px, and
    /// what to set when one is used (the style then depends on it, Blink's
    /// `ComputedStyle::HasViewportUnits`). `None` outside an element
    /// (canvas), where they are 0.
    pub viewport: Option<(gfx::SizeF, &'a Cell<bool>)>,
    /// What the system colors (`ButtonFace`...) are: the platform's, from
    /// the browser; outside a document, [`gfx::SystemColors::DEFAULT`].
    pub system_colors: &'a gfx::SystemColors,
    /// What `-webkit-link` and `-webkit-activelink` are: the document's.
    pub link_colors: TextLinkColors,
}

/// A document's link colors, which `-webkit-link` and `-webkit-activelink`
/// (the UA stylesheet's links') compute to: its `<body>`'s `link` and
/// `alink`, else the defaults (Blink's `TextLinkColors`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextLinkColors {
    pub link: Color,
    pub active_link: Color,
}

impl TextLinkColors {
    /// Outside a document, and where `<body>` sets none.
    pub const DEFAULT: Self = Self {
        link: Color::from_hex(0x0000ee),
        active_link: Color::from_hex(0xff0000),
    };
}

/// A length unit as written: serialization keeps it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthUnit {
    /// `vw`, `vh`, `vmin`, `vmax` and their `s`, `l` and `d` forms (small,
    /// large and dynamic viewports, which are the same on a desktop).
    Viewport(ViewportAxis, &'static str),
    Px,
    Em,
    Rem,
    In,
    Cm,
    Mm,
    Q,
    Pt,
    Pc,
    /// None: an SVG length written as a plain number, in user units, which
    /// are px (Blink's `CSSPrimitiveValue::UnitType::kUserUnits`).
    Number,
}

/// Which side of the viewport a viewport unit is a hundredth of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewportAxis {
    Width,
    Height,
    Min,
    Max,
}

/// The viewport units: each name and its axis.
const VIEWPORT_UNITS: [(&str, ViewportAxis); 16] = {
    use ViewportAxis::*;
    [
        ("vw", Width),
        ("svw", Width),
        ("lvw", Width),
        ("dvw", Width),
        ("vh", Height),
        ("svh", Height),
        ("lvh", Height),
        ("dvh", Height),
        ("vmin", Min),
        ("svmin", Min),
        ("lvmin", Min),
        ("dvmin", Min),
        ("vmax", Max),
        ("svmax", Max),
        ("lvmax", Max),
        ("dvmax", Max),
    ]
};

impl LengthUnit {
    pub fn from_name(unit: &str) -> Option<Self> {
        if let Some(&(name, axis)) = VIEWPORT_UNITS
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(unit))
        {
            return Some(Self::Viewport(axis, name));
        }
        Some(match_ignore_ascii_case! { unit,
            "px" => Self::Px,
            "em" => Self::Em,
            "rem" => Self::Rem,
            "in" => Self::In,
            "cm" => Self::Cm,
            "mm" => Self::Mm,
            "q" => Self::Q,
            "pt" => Self::Pt,
            "pc" => Self::Pc,
            _ => return None,
        })
    }

    /// How many px one of it is, if it is absolute (not relative to a
    /// font).
    pub fn absolute_px(self) -> Option<f32> {
        Some(match self {
            Self::Px | Self::Number => 1.0,
            Self::In => 96.0,
            Self::Cm => 96.0 / 2.54,
            Self::Mm => 96.0 / 25.4,
            Self::Q => 96.0 / 101.6,
            Self::Pt => 96.0 / 72.0,
            Self::Pc => 16.0,
            Self::Em | Self::Rem | Self::Viewport(..) => return None,
        })
    }

    /// The canonical name, lowercase as Chrome serializes it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Px => "px",
            Self::Em => "em",
            Self::Rem => "rem",
            Self::In => "in",
            Self::Cm => "cm",
            Self::Mm => "mm",
            Self::Q => "q",
            Self::Pt => "pt",
            Self::Pc => "pc",
            Self::Number => "",
            Self::Viewport(_, name) => name,
        }
    }
}

/// A length as written (Blink's `CSSNumericLiteralValue` with a length
/// unit): absolute units become px only when computed. Or a math function
/// of lengths (Blink's `CSSMathFunctionValue`).
#[derive(Debug, Clone, PartialEq)]
pub enum Length {
    Dimension { value: f32, unit: LengthUnit },
    Calc(Arc<Calc>),
}

impl Length {
    pub const fn px(value: f32) -> Self {
        Self::Dimension {
            value,
            unit: LengthUnit::Px,
        }
    }

    /// In px, as a computed value holds it: within
    /// [`computed_style::clamp_css_length`]'s range (Blink's
    /// `ClampToCSSLengthRange`, as it computes a length).
    pub(crate) fn to_px(&self, cx: &ComputeContext) -> f32 {
        computed_style::clamp_css_length(self.unclamped_px(cx))
    }

    /// In px, before it is clamped: a math function's leaf, which only
    /// the function's result is clamped after.
    pub(crate) fn unclamped_px(&self, cx: &ComputeContext) -> f32 {
        match self {
            Self::Dimension { value: v, unit } => match unit {
                LengthUnit::Em => v * cx.font_size,
                LengthUnit::Rem => v * cx.root_font_size,
                LengthUnit::Viewport(axis, _) => {
                    let Some((size, used)) = cx.viewport else {
                        return 0.0;
                    };
                    used.set(true);
                    let side = match axis {
                        ViewportAxis::Width => size.width,
                        ViewportAxis::Height => size.height,
                        ViewportAxis::Min => size.width.min(size.height),
                        ViewportAxis::Max => size.width.max(size.height),
                    };
                    v * side / 100.0
                }
                unit => v * unit.absolute_px().expect("an absolute unit"),
            },
            Self::Calc(calc) => calc.compute(cx).px,
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Dimension { value, unit } => {
                write_number(dest, (*value).into());
                dest.push_str(unit.name());
            }
            Self::Calc(calc) => calc.to_css(dest),
        }
    }

    /// `<length>`, negative only if `negative`. A unitless zero is `0px`.
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>, negative: bool) -> ParseResult<'i, Self> {
        match LengthPercentage::parse(input, negative)? {
            LengthPercentage::Length(length) => Ok(length),
            LengthPercentage::Percent(_) | LengthPercentage::Calc(_) => invalid(input),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum LengthPercentage {
    Length(Length),
    Percent(f32),
    /// A math function with a percentage in it (one of lengths alone is a
    /// `Length`).
    Calc(Arc<Calc>),
}

impl LengthPercentage {
    /// Computed: the length and the percentage each in
    /// [`computed_style::clamp_css_length`]'s range.
    pub(crate) fn compute(&self, cx: &ComputeContext) -> ComputedLengthPercentage {
        use computed_style::clamp_css_length as clamp;
        match self {
            Self::Length(length) => ComputedLengthPercentage::Px(length.to_px(cx)),
            Self::Percent(percent) => ComputedLengthPercentage::Percent(clamp(*percent)),
            Self::Calc(calc) => {
                let value = calc.compute(cx);
                match calc.kind {
                    CalcKind::Percent => ComputedLengthPercentage::Percent(clamp(value.percent)),
                    _ => ComputedLengthPercentage::Calc {
                        px: clamp(value.px),
                        percent: clamp(value.percent),
                        non_negative: calc.non_negative,
                    },
                }
            }
        }
    }

    fn is_negative(&self) -> bool {
        match self {
            Self::Length(Length::Dimension { value: v, .. }) | Self::Percent(v) => *v < 0.0,
            // Clamped when computed instead.
            Self::Length(Length::Calc(_)) | Self::Calc(_) => false,
        }
    }

    /// `<length-percentage>`. A unitless zero is a length (`0px`).
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>, negative: bool) -> ParseResult<'i, Self> {
        if let Ok(calc) = input.try_parse(|input| {
            let name = input.expect_function()?.clone();
            Calc::parse_function(
                &name,
                input,
                &[CalcKind::Length, CalcKind::Percent, CalcKind::LengthPercent],
                !negative,
            )
        }) {
            return Ok(match calc.kind {
                CalcKind::Length => Self::Length(Length::Calc(Arc::new(calc))),
                _ => Self::Calc(Arc::new(calc)),
            });
        }
        let value = match *input.next()? {
            Token::Dimension {
                value, ref unit, ..
            } => match LengthUnit::from_name(unit) {
                Some(unit) => Self::Length(Length::Dimension {
                    value: finite(value),
                    unit,
                }),
                None => return invalid(input),
            },
            Token::Percentage { unit_value, .. } => Self::Percent(finite(unit_value * 100.0)),
            Token::Number { value: 0.0, .. } => Self::Length(Length::px(0.0)),
            _ => return invalid(input),
        };
        if !negative && value.is_negative() {
            return invalid(input);
        }
        Ok(value)
    }

    /// An SVG property's `<length-percentage> | <number>`: a plain number
    /// is a length in user units (px).
    pub(crate) fn parse_svg<'i>(
        input: &mut Parser<'i, '_>,
        negative: bool,
    ) -> ParseResult<'i, Self> {
        if let Ok(value) = input.try_parse(|i| i.expect_number()) {
            if !negative && value < 0.0 {
                return invalid(input);
            }
            return Ok(Self::Length(Length::Dimension {
                value,
                unit: LengthUnit::Number,
            }));
        }
        Self::parse(input, negative)
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Length(length) => length.to_css(dest),
            Self::Percent(percent) => {
                write_number(dest, (*percent).into());
                dest.push('%');
            }
            Self::Calc(calc) => calc.to_css(dest),
        }
    }
}

/// `<line-width>` as written: the keywords are serialized as such.
#[derive(Debug, Clone, PartialEq)]
pub enum LineWidth {
    Thin,
    Medium,
    Thick,
    Length(Length),
}

impl LineWidth {
    pub(crate) fn to_px(&self, cx: &ComputeContext) -> f32 {
        match self {
            Self::Thin => 1.0,
            Self::Medium => 3.0,
            Self::Thick => 5.0,
            Self::Length(length) => length.to_px(cx),
        }
    }

    /// `thin` / `medium` / `thick` or a non-negative length.
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Self> {
        let keyword: ParseResult<'i, Self> = input.try_parse(|i| {
            let location = i.current_source_location();
            let ident = i.expect_ident()?;
            Ok(match_ignore_ascii_case! { ident,
                "thin" => Self::Thin,
                "medium" => Self::Medium,
                "thick" => Self::Thick,
                _ => return Err(location.new_custom_error(())),
            })
        });
        if keyword.is_ok() {
            return keyword;
        }
        match LengthPercentage::parse(input, false)? {
            LengthPercentage::Length(length) => Ok(Self::Length(length)),
            LengthPercentage::Percent(_) | LengthPercentage::Calc(_) => invalid(input),
        }
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::Thin => dest.push_str("thin"),
            Self::Medium => dest.push_str("medium"),
            Self::Thick => dest.push_str("thick"),
            Self::Length(length) => length.to_css(dest),
        }
    }
}

/// A `<color>` or `currentcolor` as written: Chrome keeps a named color's
/// name, and writes the others as `rgb()` / `rgba()`.
#[derive(Debug, Clone, PartialEq)]
pub enum SpecifiedColor {
    CurrentColor,
    /// A named color or `transparent`, lowercase.
    Named(Arc<str>, Color),
    /// Hex, `rgb()` or `rgba()`.
    Rgba(Color),
    /// A system color, which computes to the platform's (Blink's
    /// `StyleColor` keeps the keyword till style resolution, so the same
    /// sheet follows each document's colors).
    System(SystemColor),
}

impl SpecifiedColor {
    pub(crate) const TRANSPARENT: &str = "transparent";
    pub(crate) const FOCUS_RING: &str = "-webkit-focus-ring-color";
    pub(crate) const FOCUS_RING_COLOR: Color = Color::rgb(16, 16, 16);

    pub(crate) fn compute(&self, cx: &ComputeContext<'_>) -> StyleColor {
        match *self {
            Self::CurrentColor => StyleColor::CurrentColor,
            Self::Named(_, color) | Self::Rgba(color) => StyleColor::Rgba(color),
            Self::System(SystemColor::WebkitLink) => StyleColor::Rgba(cx.link_colors.link),
            Self::System(SystemColor::WebkitActiveLink) => {
                StyleColor::Rgba(cx.link_colors.active_link)
            }
            Self::System(system) => StyleColor::Rgba(system.color(cx.system_colors)),
        }
    }

    /// `transparent`, the initial `background-color`.
    pub(crate) fn transparent() -> Self {
        Self::Named(Self::TRANSPARENT.into(), Color::TRANSPARENT)
    }

    pub(crate) fn to_css(&self, dest: &mut String) {
        match self {
            Self::CurrentColor => dest.push_str("currentcolor"),
            Self::Named(name, _) => dest.push_str(name),
            Self::Rgba(color) => write_rgba(dest, *color),
            Self::System(system) => dest.push_str(system.name()),
        }
    }
}

macro_rules! system_colors {
    ($($variant:ident = $name:literal,)*) => {
        /// A CSS Color 4 system color keyword, the deprecated ones among
        /// them, and the `-webkit-` ones Chrome keeps (Blink's
        /// `StyleColor::IsSystemColor`).
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum SystemColor {
            $($variant,)*
        }

        impl SystemColor {
            /// The keyword in lowercase, as Chrome writes it.
            pub(crate) fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }

            /// The keyword `name` (lowercase).
            fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($name => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

system_colors! {
    AccentColor = "accentcolor",
    AccentColorText = "accentcolortext",
    ActiveText = "activetext",
    ButtonBorder = "buttonborder",
    ButtonFace = "buttonface",
    ButtonText = "buttontext",
    Canvas = "canvas",
    CanvasText = "canvastext",
    Field = "field",
    FieldText = "fieldtext",
    GrayText = "graytext",
    Highlight = "highlight",
    HighlightText = "highlighttext",
    LinkText = "linktext",
    Mark = "mark",
    MarkText = "marktext",
    SelectedItem = "selecteditem",
    SelectedItemText = "selecteditemtext",
    VisitedText = "visitedtext",
    ActiveBorder = "activeborder",
    ActiveCaption = "activecaption",
    AppWorkspace = "appworkspace",
    Background = "background",
    ButtonHighlight = "buttonhighlight",
    ButtonShadow = "buttonshadow",
    CaptionText = "captiontext",
    InactiveBorder = "inactiveborder",
    InactiveCaption = "inactivecaption",
    InactiveCaptionText = "inactivecaptiontext",
    InfoBackground = "infobackground",
    InfoText = "infotext",
    Menu = "menu",
    MenuText = "menutext",
    Scrollbar = "scrollbar",
    ThreeDDarkShadow = "threeddarkshadow",
    ThreeDFace = "threedface",
    ThreeDHighlight = "threedhighlight",
    ThreeDLightShadow = "threedlightshadow",
    ThreeDShadow = "threedshadow",
    Window = "window",
    WindowFrame = "windowframe",
    WindowText = "windowtext",
    WebkitLink = "-webkit-link",
    WebkitActiveLink = "-webkit-activelink",
}

impl SystemColor {
    /// The color it is with `colors`, the platform's: the deprecated ones
    /// as CSS Color 4 maps them (Blink's
    /// `LayoutTheme::SystemColorFromColorProvider`), the rest fixed
    /// (`LayoutTheme::DefaultSystemColor`).
    pub fn color(self, colors: &gfx::SystemColors) -> Color {
        use SystemColor::*;
        match self {
            // The platform's accent color only when a page may see it,
            // which is never yet: Chrome's default.
            AccentColor => Color::from_hex(0x0075ff),
            AccentColorText => Color::WHITE,
            ActiveText => colors.active_text,
            ButtonFace | ButtonHighlight | ButtonShadow | ThreeDFace => colors.button_face,
            ButtonText => colors.button_text,
            ButtonBorder | ActiveBorder | InactiveBorder | ThreeDDarkShadow | ThreeDHighlight
            | ThreeDLightShadow | ThreeDShadow | WindowFrame => colors.button_border,
            Canvas | ActiveCaption | AppWorkspace | Background | InactiveCaption
            | InfoBackground | Menu | Scrollbar | Window => colors.canvas,
            CanvasText | CaptionText | InfoText | MenuText | WindowText => colors.canvas_text,
            Field => colors.field,
            FieldText => colors.field_text,
            GrayText => colors.gray_text,
            Highlight => colors.highlight,
            HighlightText => colors.highlight_text,
            LinkText => colors.link_text,
            VisitedText => colors.visited_text,
            SelectedItem => colors.selected_item,
            SelectedItemText => colors.selected_item_text,
            Mark => Color::from_hex(0xffff00),
            MarkText => Color::BLACK,
            // Not one the provider gives: the fixed `GrayText`.
            InactiveCaptionText => Color::from_hex(0x808080),
            // A document's, outside one (`SpecifiedColor::compute` takes
            // the document's).
            WebkitLink => TextLinkColors::DEFAULT.link,
            WebkitActiveLink => TextLinkColors::DEFAULT.active_link,
        }
    }
}

/// `<color>` or `currentcolor`.
pub(crate) fn parse_style_color<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedColor> {
    if input
        .try_parse(|i| i.expect_ident_matching("currentcolor"))
        .is_ok()
    {
        return Ok(SpecifiedColor::CurrentColor);
    }
    parse_color(input)
}

/// `<color>`: named colors, `transparent`, system colors, hex, `rgb()` /
/// `rgba()` (`currentcolor` is [`parse_style_color`]'s).
pub(crate) fn parse_color<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, SpecifiedColor> {
    let location = input.current_source_location();
    let token = input.next()?.clone();
    let invalid = || Err(location.new_custom_error(()));
    match token {
        Token::Hash(ref hex) | Token::IDHash(ref hex) => {
            match cssparser::color::parse_hash_color(hex.as_bytes()) {
                Ok((r, g, b, a)) => Ok(SpecifiedColor::Rgba(Color::rgba(r, g, b, alpha(a)))),
                Err(()) => invalid(),
            }
        }
        Token::Ident(ref name) => {
            let name = name.to_ascii_lowercase();
            if name == SpecifiedColor::TRANSPARENT {
                return Ok(SpecifiedColor::transparent());
            }
            // The platform's focus ring color, as Chrome's on Windows and
            // Linux (`LayoutTheme::FocusRingColor`; macOS's follows the
            // system's accent color).
            if name == SpecifiedColor::FOCUS_RING {
                return Ok(SpecifiedColor::Named(
                    name.into(),
                    SpecifiedColor::FOCUS_RING_COLOR,
                ));
            }
            if let Some(system) = SystemColor::from_name(&name) {
                return Ok(SpecifiedColor::System(system));
            }
            match cssparser::color::parse_named_color(&name) {
                Ok((r, g, b)) => Ok(SpecifiedColor::Named(name.into(), Color::rgb(r, g, b))),
                Err(()) => invalid(),
            }
        }
        Token::Function(ref name)
            if name.eq_ignore_ascii_case("rgb") || name.eq_ignore_ascii_case("rgba") =>
        {
            input
                .parse_nested_block(parse_rgb)
                .map(SpecifiedColor::Rgba)
        }
        _ => invalid(),
    }
}

/// HTML's rules for parsing a legacy colour value (`bgcolor`, `<font
/// color>`; Blink's `HTMLElement::ParseColorWithLegacyRules`): a named
/// color, `#rgb`, or anything else read as hex digits, three parts of it
/// (`chucknorris` is `#c00000`). `None` for the empty string and
/// `transparent`.
pub fn parse_legacy_color(value: &str) -> Option<Color> {
    let value = value.trim_matches(|c| matches!(c, ' ' | '\t' | '\n' | '\x0c' | '\r'));
    if value.is_empty() || value.eq_ignore_ascii_case(SpecifiedColor::TRANSPARENT) {
        return None;
    }
    if let Ok((r, g, b)) = cssparser::color::parse_named_color(&value.to_ascii_lowercase()) {
        return Some(Color::rgb(r, g, b));
    }
    let bytes = value.as_bytes();
    if bytes.len() == 4 && bytes[0] == b'#' && bytes[1..].iter().all(u8::is_ascii_hexdigit) {
        #[expect(
            clippy::arithmetic_side_effects,
            clippy::cast_possible_truncation,
            reason = "a hex digit is at most 15, and 15 * 17 is 255"
        )]
        let digit = |i: usize| (bytes[i] as char).to_digit(16).expect("a hex digit") as u8 * 17;
        return Some(Color::rgb(digit(1), digit(2), digit(3)));
    }
    // Outside the BMP, two zeros (a UTF-16 string's two code units), at
    // most 128 of them, without a leading `#`; what isn't a hex digit, a
    // zero; padded to three equal parts.
    let mut digits: Vec<u8> = Vec::new();
    for c in value.chars() {
        if c as u32 > 0xffff {
            digits.extend(b"00");
        } else {
            digits.push(if c.is_ascii_hexdigit() { c as u8 } else { b'0' });
        }
    }
    digits.truncate(128);
    if value.starts_with('#') {
        digits.remove(0);
    }
    while digits.is_empty() || !digits.len().is_multiple_of(3) {
        digits.push(b'0');
    }
    let mut length = digits.len() / 3;
    let mut parts: Vec<&[u8]> = digits.chunks(length).collect();
    // The last eight digits of each, then leading zeros all three have
    // dropped while longer than two, then the first two.
    if let Some(skip) = length.checked_sub(8).filter(|&skip| skip > 0) {
        for part in &mut parts {
            *part = &part[skip..];
        }
        length = 8;
    }
    while length > 2 && parts.iter().all(|part| part[0] == b'0') {
        for part in &mut parts {
            *part = &part[1..];
        }
        #[expect(clippy::arithmetic_side_effects, reason = "length > 2")]
        {
            length -= 1;
        }
    }
    let component = |part: &[u8]| {
        let part = &part[..part.len().min(2)];
        u8::from_str_radix(std::str::from_utf8(part).expect("ASCII"), 16).expect("hex digits")
    };
    Some(Color::rgb(
        component(parts[0]),
        component(parts[1]),
        component(parts[2]),
    ))
}

fn alpha(a: f32) -> u8 {
    clamp_round(a.clamp(0.0, 1.0) * 255.0)
}

/// The inside of `rgb()` / `rgba()`, in either syntax:
/// `rgb(255, 0, 0[, 0.5])` or `rgb(255 0 0[ / 50%])`. In the legacy
/// (comma) syntax the three channels must all be numbers or all be
/// percentages.
fn parse_rgb<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, Color> {
    // A channel as 0-255, and whether it was a percentage.
    fn channel<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, (f32, bool)> {
        match *input.next()? {
            Token::Number { value, .. } => Ok((value, false)),
            Token::Percentage { unit_value, .. } => Ok((unit_value * 255.0, true)),
            _ => invalid(input),
        }
    }
    fn alpha_value<'i>(input: &mut Parser<'i, '_>) -> ParseResult<'i, f32> {
        match *input.next()? {
            Token::Number { value, .. } => Ok(value),
            Token::Percentage { unit_value, .. } => Ok(unit_value),
            _ => invalid(input),
        }
    }
    let to_u8 = |v: f32| clamp_round::<u8>(v.clamp(0.0, 255.0));

    let (r, r_percent) = channel(input)?;
    let legacy = input.try_parse(|i| i.expect_comma()).is_ok();
    let (g, g_percent) = channel(input)?;
    if legacy {
        input.expect_comma()?;
    }
    let (b, b_percent) = channel(input)?;
    if legacy && !(r_percent == g_percent && g_percent == b_percent) {
        return invalid(input);
    }
    let a = if input.is_exhausted() {
        1.0
    } else {
        if legacy {
            input.expect_comma()?;
        } else {
            input.expect_delim('/')?;
        }
        alpha_value(input)?
    };
    input.expect_exhausted()?;
    Ok(Color::rgba(to_u8(r), to_u8(g), to_u8(b), alpha(a)))
}

/// Writes a number as Chrome does (`String::Number`, like C's `%g`): six
/// significant digits, without trailing zeros, in exponent form below
/// 1e-4 and from 1e6.
/// A number as written, kept finite: one too big for an f32 (which the
/// tokenizer makes infinite) is the biggest there is, as Blink keeps it
/// (`3.40282e+38`); a computed value clamps it further.
pub(crate) fn finite(value: f32) -> f32 {
    value.clamp(-f32::MAX, f32::MAX)
}

pub(crate) fn write_number(dest: &mut String, value: f64) {
    use std::fmt::Write;

    if value == 0.0 || !value.is_finite() {
        // -0 too; there are no infinities among parsed values.
        dest.push('0');
        return;
    }
    // An integer below 1e6 (most of CSS's numbers: `20px`, `700`) has six
    // significant digits at most: itself, with no rounding or exponent to
    // work out (two exact formattings, the slow path's, each a `String`).
    if value.fract() == 0.0 && value.abs() < 1e6 {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "an integer below 1e6 in magnitude: exact as an i32"
        )]
        let integer = value as i32;
        write!(dest, "{integer}").unwrap();
        return;
    }
    let rounded = format!("{value:.5e}");
    let (mantissa, exponent) = rounded.split_once('e').expect("exponent form");
    let exponent: i32 = exponent.parse().expect("an integer exponent");
    if !(-4..6).contains(&exponent) {
        dest.push_str(trim_fraction(mantissa));
        let sign = if exponent < 0 { '-' } else { '+' };
        write!(dest, "e{sign}{:02}", exponent.abs()).unwrap();
        return;
    }
    #[expect(
        clippy::arithmetic_side_effects,
        clippy::cast_sign_loss,
        reason = "-4 <= exponent < 6: 5 - exponent is from 0 to 9"
    )]
    let decimals = (5 - exponent) as usize;
    dest.push_str(trim_fraction(&format!("{value:.decimals$}")));
}

/// `1.500` is `1.5`, `2.000` is `2`.
fn trim_fraction(number: &str) -> &str {
    if number.contains('.') {
        number.trim_end_matches('0').trim_end_matches('.')
    } else {
        number
    }
}

/// `rgb(r, g, b)`, or `rgba(r, g, b, a)` if not opaque (css-color 4,
/// "serializing sRGB values").
pub(crate) fn write_rgba(dest: &mut String, color: Color) {
    use std::fmt::Write;

    let Color { r, g, b, a } = color;
    if a == 255 {
        write!(dest, "rgb({r}, {g}, {b})").unwrap();
        return;
    }
    write!(dest, "rgba({r}, {g}, {b}, ").unwrap();
    // The fewest decimals, two or three, that give back the same 8-bit
    // alpha.
    let a = f64::from(a) / 255.0;
    let two = (a * 100.0).round() / 100.0;
    let alpha = if alpha(f32_from_f64(two)) == color.a {
        two
    } else {
        (a * 1000.0).round() / 1000.0
    };
    write_number(dest, alpha);
    dest.push(')');
}

#[cfg(test)]
mod tests {
    use cssparser::ParserInput;

    use super::*;

    /// Numbers as Chrome writes them (`%g`'s six significant digits): an
    /// integer below 1e6 whole, one from 1e6 in exponent form, fractions
    /// rounded.
    #[test]
    fn numbers_are_written_as_chrome_writes_them() {
        let written = |value: f64| {
            let mut dest = String::new();
            write_number(&mut dest, value);
            dest
        };
        assert_eq!(written(20.0), "20");
        assert_eq!(written(-700.0), "-700");
        assert_eq!(written(999_999.0), "999999");
        assert_eq!(written(1_000_000.0), "1e+06");
        assert_eq!(written(1_234_567.0), "1.23457e+06");
        assert_eq!(written(1.5), "1.5");
        assert_eq!(written(0.000_01), "1e-05");
        assert_eq!(written(0.0), "0");
    }

    fn parse<T>(
        css: &str,
        f: impl for<'i, 't> FnOnce(&mut Parser<'i, 't>) -> ParseResult<'i, T>,
    ) -> Option<T> {
        let mut input = ParserInput::new(css);
        let mut parser = Parser::new(&mut input);
        parser.parse_entirely(|p| f(p)).ok()
    }

    fn color_with(css: &str, system_colors: &gfx::SystemColors) -> Option<Color> {
        let cx = ComputeContext {
            font_size: 16.0,
            parent_font_size: 16.0,
            root_font_size: 16.0,
            viewport: None,
            system_colors,
            link_colors: TextLinkColors::DEFAULT,
        };
        match parse(css, parse_color)?.compute(&cx) {
            StyleColor::Rgba(color) => Some(color),
            StyleColor::CurrentColor => unreachable!("not a <color>"),
        }
    }

    fn color(css: &str) -> Option<Color> {
        color_with(css, &gfx::SystemColors::DEFAULT)
    }

    #[test]
    fn system_colors() {
        // Written as the keyword (lowercase), computed to the platform's.
        let written = |css| {
            let mut out = String::new();
            parse(css, parse_color)?.to_css(&mut out);
            Some(out)
        };
        assert_eq!(written("ButtonFace").as_deref(), Some("buttonface"));
        assert_eq!(written("-WEBKIT-LINK").as_deref(), Some("-webkit-link"));
        // With no provider (a web test's), Blink's defaults.
        assert_eq!(color("buttonface"), Some(Color::from_hex(0xefefef)));
        assert_eq!(color("buttonborder"), Some(Color::from_hex(0x767676)));
        assert_eq!(color("graytext"), Some(Color::from_hex(0x808080)));
        assert_eq!(color("linktext"), Some(Color::from_hex(0x0000ee)));
        // A platform's, each one apart from the others, so that a keyword
        // that takes the wrong one shows.
        let windows = gfx::SystemColors {
            button_face: Color::rgb(240, 240, 240),
            button_text: Color::rgb(1, 1, 1),
            button_border: Color::rgb(1, 1, 1),
            canvas: Color::rgb(254, 254, 254),
            canvas_text: Color::rgb(2, 2, 2),
            field: Color::rgb(253, 253, 253),
            field_text: Color::rgb(3, 3, 3),
            gray_text: Color::rgb(109, 109, 109),
            highlight: Color::rgb(0, 120, 212),
            highlight_text: Color::rgb(252, 252, 252),
            inactive_highlight: Color::rgb(4, 4, 4),
            inactive_highlight_text: Color::rgb(5, 5, 5),
            link_text: Color::rgb(0, 102, 204),
            visited_text: Color::rgb(0, 102, 205),
            active_text: Color::rgb(0, 102, 206),
            selected_item: Color::rgb(25, 103, 210),
            selected_item_text: Color::rgb(251, 251, 251),
        };
        let platform = |css| color_with(css, &windows);
        for (css, want) in [
            ("buttonface", windows.button_face),
            ("buttontext", windows.button_text),
            ("buttonborder", windows.button_border),
            ("canvas", windows.canvas),
            ("canvastext", windows.canvas_text),
            ("field", windows.field),
            ("fieldtext", windows.field_text),
            ("graytext", windows.gray_text),
            ("highlight", windows.highlight),
            ("highlighttext", windows.highlight_text),
            ("linktext", windows.link_text),
            ("visitedtext", windows.visited_text),
            ("activetext", windows.active_text),
            ("selecteditem", windows.selected_item),
            ("selecteditemtext", windows.selected_item_text),
            // The deprecated ones, as CSS Color 4 maps them.
            ("buttonhighlight", windows.button_face),
            ("buttonshadow", windows.button_face),
            ("threedface", windows.button_face),
            ("activeborder", windows.button_border),
            ("inactiveborder", windows.button_border),
            ("threeddarkshadow", windows.button_border),
            ("threedhighlight", windows.button_border),
            ("threedlightshadow", windows.button_border),
            ("threedshadow", windows.button_border),
            ("windowframe", windows.button_border),
            ("activecaption", windows.canvas),
            ("appworkspace", windows.canvas),
            ("background", windows.canvas),
            ("inactivecaption", windows.canvas),
            ("infobackground", windows.canvas),
            ("menu", windows.canvas),
            ("scrollbar", windows.canvas),
            ("window", windows.canvas),
            ("captiontext", windows.canvas_text),
            ("infotext", windows.canvas_text),
            ("menutext", windows.canvas_text),
            ("windowtext", windows.canvas_text),
            // Fixed whatever the platform: a page never sees its accent
            // color.
            ("accentcolor", Color::from_hex(0x0075ff)),
            ("accentcolortext", Color::WHITE),
            ("mark", Color::from_hex(0xffff00)),
            ("marktext", Color::BLACK),
            ("inactivecaptiontext", Color::from_hex(0x808080)),
            ("-webkit-link", Color::from_hex(0x0000ee)),
            ("-webkit-activelink", Color::from_hex(0xff0000)),
        ] {
            assert_eq!(platform(css), Some(want), "{css}");
        }
    }

    #[test]
    fn colors() {
        let red = Some(Color::rgb(255, 0, 0));
        for css in [
            "red",
            "RED",
            "#f00",
            "#ff0000",
            "rgb(255, 0, 0)",
            "rgb(255 0 0)",
            "rgba(100%, 0%, 0%)",
            "rgb(255 0% 0)",
        ] {
            assert_eq!(color(css), red, "{css}");
        }
        assert_eq!(color("transparent"), Some(Color::TRANSPARENT));
        assert_eq!(color("#0f08"), Some(Color::rgba(0, 255, 0, 136)));
        assert_eq!(color("#00ff0080"), Some(Color::rgba(0, 255, 0, 128)));
        assert_eq!(
            color("rgba(0, 0, 255, 0.5)"),
            Some(Color::rgba(0, 0, 255, 128))
        );
        assert_eq!(
            color("rgb(0 0 255 / 50%)"),
            Some(Color::rgba(0, 0, 255, 128))
        );
        assert_eq!(color("rgb(300, -5, 0)"), Some(Color::rgb(255, 0, 0)));
        for css in [
            "#ff",
            "#12345",
            "notacolor",
            "rgb(255, 0%, 0)",
            "rgb(1, 2)",
            "rgb(1 2 3, 4)",
            "currentcolor",
            "hsl(0, 0%, 0%)",
        ] {
            assert_eq!(color(css), None, "{css}");
        }
    }

    #[test]
    fn lengths() {
        let lp = |css, negative| parse(css, |p| LengthPercentage::parse(p, negative));
        let cx = ComputeContext {
            font_size: 20.0,
            parent_font_size: 10.0,
            root_font_size: 16.0,
            viewport: None,
            system_colors: &gfx::SystemColors::DEFAULT,
            link_colors: TextLinkColors::DEFAULT,
        };
        let px = |css| lp(css, true).map(|v| v.compute(&cx));
        assert_eq!(px("10px"), Some(ComputedLengthPercentage::Px(10.0)));
        assert_eq!(px("1in"), Some(ComputedLengthPercentage::Px(96.0)));
        assert_eq!(px("2.54cm"), Some(ComputedLengthPercentage::Px(96.0)));
        assert_eq!(px("25.4mm"), Some(ComputedLengthPercentage::Px(96.0)));
        assert_eq!(px("101.6q"), Some(ComputedLengthPercentage::Px(96.0)));
        assert_eq!(px("72pt"), Some(ComputedLengthPercentage::Px(96.0)));
        assert_eq!(px("6PC"), Some(ComputedLengthPercentage::Px(96.0)));
        assert_eq!(px("2em"), Some(ComputedLengthPercentage::Px(40.0)));
        assert_eq!(px("2rem"), Some(ComputedLengthPercentage::Px(32.0)));
        assert_eq!(px("0"), Some(ComputedLengthPercentage::Px(0.0)));
        assert_eq!(px("50%"), Some(ComputedLengthPercentage::Percent(50.0)));
        assert_eq!(px("-3px"), Some(ComputedLengthPercentage::Px(-3.0)));
        assert_eq!(lp("-3px", false), None);
        assert_eq!(lp("-1%", false), None);
        for css in ["10", "10vx", "auto"] {
            assert_eq!(lp(css, true), None, "{css}");
        }
        // Math functions: lengths alone compute to px, with a percentage
        // to both; a negative one where none is allowed is clamped.
        assert_eq!(px("calc(1px)"), Some(ComputedLengthPercentage::Px(1.0)));
        assert_eq!(
            px("calc(10px + 2em)"),
            Some(ComputedLengthPercentage::Px(50.0))
        );
        assert_eq!(
            px("calc(50% - 10px)"),
            Some(ComputedLengthPercentage::Calc {
                px: -10.0,
                percent: 50.0,
                non_negative: false,
            })
        );
        assert_eq!(
            lp("calc(-3px)", false).map(|v| v.compute(&cx)),
            Some(ComputedLengthPercentage::Px(0.0))
        );
    }
}
