//! What a canvas's 2D context parses as CSS: its colors (`fillStyle`,
//! `strokeStyle`), which HTML serializes its own way, its `font`, computed
//! against the canvas's style as Chromium does, and its `letterSpacing`
//! and `wordSpacing`.

use base::numerics::saturated_cast;
use computed_style::{CanvasFont, ComputedStyle, FontStyle, FontVariantCaps};
use cssparser::{Parser, ParserInput, Token};
use gfx::Color;

use crate::font::{FontShorthand, canvas_font_family_to_css, parse_font_shorthand};
use crate::values::{
    ComputeContext, Length, LengthUnit, SpecifiedColor, parse_style_color, write_number,
};

/// A canvas color as it is set: a color, or `currentcolor`, which the
/// context resolves against its canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasColor {
    Color(Color),
    CurrentColor,
}

/// `text` as a CSS `<color>` or `currentcolor`, white space around it
/// allowed, a system color the platform's in `system_colors`; `None` if it
/// is anything else (the context keeps the color it has).
pub fn parse_canvas_color(text: &str, system_colors: &gfx::SystemColors) -> Option<CanvasColor> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let color = parse_style_color(&mut parser).ok()?;
    parser.expect_exhausted().ok()?;
    Some(match color {
        SpecifiedColor::CurrentColor => CanvasColor::CurrentColor,
        SpecifiedColor::Named(_, color) | SpecifiedColor::Rgba(color) => CanvasColor::Color(color),
        SpecifiedColor::System(system) => CanvasColor::Color(system.color(system_colors)),
    })
}

/// What `currentcolor` is for a canvas whose `style` attribute is
/// `style_attribute` (Blink's `CanvasRenderingContext2D::GetCurrentColor`):
/// the color its inline style's `color` declares, black if that isn't one
/// (none, or `currentcolor` itself). Blink reads no other style: a color
/// the canvas inherits or a stylesheet gives it doesn't count.
pub fn canvas_current_color(
    style_attribute: Option<&str>,
    system_colors: &gfx::SystemColors,
) -> Color {
    let declared = style_attribute
        .map(|css| crate::CssPropertyValueSet::parse(css).property_value("color"))
        .unwrap_or_default();
    match parse_canvas_color(&declared, system_colors) {
        Some(CanvasColor::Color(color)) => color,
        _ => Color::BLACK,
    }
}

/// HTML's "serialization of a color" for the 2D context: `#rrggbb` in
/// lowercase when opaque, otherwise `rgba(r, g, b, a)`, the alpha the
/// shortest number that comes back to the same byte.
pub fn serialize_canvas_color(color: Color) -> String {
    let Color { r, g, b, a } = color;
    if a == u8::MAX {
        return format!("#{r:02x}{g:02x}{b:02x}");
    }
    format!("rgba({r}, {g}, {b}, {})", crate::computed::alpha_to_css(a))
}

/// A canvas's `font` as it is set (`CSSParser::ParseFont`): the `font`
/// shorthand alone, with a canvas's `small-caps` and stretch keywords;
/// computed against the canvas's style.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedCanvasFont(FontShorthand);

/// `text` as a canvas's `font`; `None` if it isn't one (the context keeps
/// its font): a CSS-wide keyword, `var()` or a system font isn't.
pub fn parse_canvas_font(text: &str) -> Option<SpecifiedCanvasFont> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let font = parse_font_shorthand(&mut parser).ok()?;
    parser.expect_exhausted().ok()?;
    Some(SpecifiedCanvasFont(font))
}

impl SpecifiedCanvasFont {
    /// The font, its relative values (`em`, `%`, `larger`, `bolder`)
    /// against `parent` (the canvas's computed style, or 10px sans-serif
    /// for one that has none: `canvas_font_parent`), `rem` against
    /// `root_font_size` (Blink's `StyleResolver::ComputeFont`).
    pub fn compute(&self, parent: &ComputedStyle, root_font_size: f32) -> CanvasFont {
        let font = &self.0;
        let cx = ComputeContext {
            font_size: parent.font.size,
            parent_font_size: parent.font.size,
            root_font_size,
            viewport: None,
            system_colors: &gfx::SystemColors::DEFAULT,
            link_colors: crate::values::TextLinkColors::DEFAULT,
        };
        CanvasFont {
            family: font.family.clone(),
            size: font.size.compute(&cx),
            weight: font
                .weight
                .map_or(400.0, |weight| weight.compute(parent.font.weight)),
            style: font.style.unwrap_or(FontStyle::Normal),
            stretch: font.stretch.unwrap_or_default(),
            variant_caps: if font.small_caps {
                FontVariantCaps::SmallCaps
            } else {
                FontVariantCaps::Normal
            },
        }
    }
}

/// What a canvas with no computed style (not in a document a frame
/// renders) computes its `font` against: 10px sans-serif (Blink's
/// `CanvasFontCache::CreateDefaultFontStyle`).
pub fn canvas_font_parent() -> ComputedStyle {
    let default = CanvasFont::default();
    let mut style = ComputedStyle::initial();
    let font = style.font_mut();
    font.family = default.family;
    font.size = default.size;
    style
}

/// A canvas's `font` as its getter gives it (Blink's
/// `BaseRenderingContext2D::font`): `italic` (not `oblique`), `bold` or the
/// weight's number but for 400, `small-caps` (not the other caps), the
/// size in px, the families (a name quoted unless it is one identifier);
/// no line height or stretch.
pub fn serialize_canvas_font(font: &CanvasFont) -> String {
    let mut out = String::new();
    if font.style == FontStyle::Italic {
        out.push_str("italic ");
    }
    if font.weight == 700.0 {
        out.push_str("bold ");
    } else if font.weight != 400.0 {
        out.push_str(&format!("{} ", saturated_cast::<i32, _>(font.weight)));
    }
    if font.variant_caps == FontVariantCaps::SmallCaps {
        out.push_str("small-caps ");
    }
    write_number(&mut out, font.size.into());
    out.push_str("px ");
    canvas_font_family_to_css(&font.family, &mut out);
    out
}

/// A canvas's `letterSpacing` or `wordSpacing` as it is set: one length,
/// in its own unit.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasSpacing(Length);

/// `text` as a canvas's `letterSpacing` or `wordSpacing`: exactly one
/// length with a unit (Blink's `StringToNumWithUnit`: not `0`, a
/// percentage, `calc()` or white space around it); `None` if it isn't one
/// (the context keeps what it has). The units machopo's CSS has: `ex`, `ch`
/// and the viewport's are not.
pub fn parse_canvas_spacing(text: &str) -> Option<CanvasSpacing> {
    let mut input = ParserInput::new(text);
    let mut parser = Parser::new(&mut input);
    let length = match *parser.next_including_whitespace().ok()? {
        Token::Dimension {
            value, ref unit, ..
        } => match LengthUnit::from_name(unit)? {
            // No viewport here.
            LengthUnit::Viewport(..) => return None,
            unit => Length::Dimension { value, unit },
        },
        _ => return None,
    };
    // Nothing after it, white space neither.
    if parser.next_including_whitespace().is_ok() {
        return None;
    }
    Some(CanvasSpacing(length))
}

impl CanvasSpacing {
    /// As it was written, in its unit (`1.5em`).
    pub fn to_css(&self) -> String {
        let mut out = String::new();
        self.0.to_css(&mut out);
        out
    }

    /// In px, `em` and `rem` against the context's font size (as Blink's
    /// conversion with no element does).
    pub fn to_px(&self, font_size: f32) -> f32 {
        self.0.to_px(&ComputeContext {
            font_size,
            parent_font_size: font_size,
            root_font_size: font_size,
            viewport: None,
            system_colors: &gfx::SystemColors::DEFAULT,
            link_colors: crate::values::TextLinkColors::DEFAULT,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_canvas_color_is_a_whole_css_color() {
        assert_eq!(
            parse_canvas_color(" #0F0 ", &gfx::SystemColors::DEFAULT),
            Some(CanvasColor::Color(Color::rgb(0, 255, 0)))
        );
        assert_eq!(
            parse_canvas_color("rgba(1, 2, 3, 0.5)", &gfx::SystemColors::DEFAULT),
            Some(CanvasColor::Color(Color::rgba(1, 2, 3, 128)))
        );
        assert_eq!(
            parse_canvas_color("CurrentColor", &gfx::SystemColors::DEFAULT),
            Some(CanvasColor::CurrentColor)
        );
        assert_eq!(
            parse_canvas_color("transparent", &gfx::SystemColors::DEFAULT),
            Some(CanvasColor::Color(Color::TRANSPARENT))
        );
        assert_eq!(
            parse_canvas_color("red blue", &gfx::SystemColors::DEFAULT),
            None
        );
        assert_eq!(parse_canvas_color("", &gfx::SystemColors::DEFAULT), None);
        assert_eq!(
            parse_canvas_color("nonsense", &gfx::SystemColors::DEFAULT),
            None
        );
    }

    #[test]
    fn current_color_is_the_inline_styles_color_or_black() {
        assert_eq!(
            canvas_current_color(Some("color: #0f0; width: 1px"), &gfx::SystemColors::DEFAULT),
            Color::rgb(0, 255, 0)
        );
        assert_eq!(
            canvas_current_color(Some("width: 1px"), &gfx::SystemColors::DEFAULT),
            Color::BLACK
        );
        assert_eq!(
            canvas_current_color(Some("color: currentcolor"), &gfx::SystemColors::DEFAULT),
            Color::BLACK
        );
        assert_eq!(
            canvas_current_color(None, &gfx::SystemColors::DEFAULT),
            Color::BLACK
        );
    }

    #[test]
    fn opaque_colors_serialize_as_hex() {
        assert_eq!(serialize_canvas_color(Color::rgb(255, 0, 170)), "#ff00aa");
        assert_eq!(
            serialize_canvas_color(Color::rgba(1, 2, 3, 128)),
            "rgba(1, 2, 3, 0.5)"
        );
        assert_eq!(
            serialize_canvas_color(Color::TRANSPARENT),
            "rgba(0, 0, 0, 0)"
        );
    }

    /// `text` set on a canvas whose style is 10px sans-serif, read back.
    fn font(text: &str) -> Option<String> {
        parse_canvas_font(text)
            .map(|font| serialize_canvas_font(&font.compute(&canvas_font_parent(), 16.0)))
    }

    #[test]
    fn a_canvas_font_reads_back_as_chromes() {
        assert_eq!(font("12px serif").as_deref(), Some("12px serif"));
        // Points become px; style before weight; `oblique` isn't written,
        // nor the line height.
        assert_eq!(
            font("bold italic 12pt Arial, sans-serif").as_deref(),
            Some("italic bold 16px Arial, sans-serif")
        );
        assert_eq!(font("oblique 300 10px/2 x").as_deref(), Some("300 10px x"));
        assert_eq!(
            font("10px 'Times New Roman'").as_deref(),
            Some("10px \"Times New Roman\"")
        );
        // Relative to the canvas's 10px, 400: `em`, `%`, `larger`,
        // `bolder`; `rem` to the root's.
        assert_eq!(font("2em a").as_deref(), Some("20px a"));
        assert_eq!(font("150% a").as_deref(), Some("15px a"));
        assert_eq!(font("larger a").as_deref(), Some("12px a"));
        assert_eq!(font("bolder 1rem a").as_deref(), Some("bold 16px a"));
    }

    #[test]
    fn a_canvas_font_takes_small_caps_and_stretch() {
        let parsed = parse_canvas_font("small-caps condensed normal 10px a").unwrap();
        let computed = parsed.compute(&canvas_font_parent(), 16.0);
        assert_eq!(computed.variant_caps, FontVariantCaps::SmallCaps);
        assert_eq!(computed.stretch, computed_style::FontStretch::Condensed);
        // The stretch isn't written back.
        assert_eq!(serialize_canvas_font(&computed), "small-caps 10px a");
        // A page's `font` takes them too, into its longhands.
        let page = crate::CssPropertyValueSet::parse("font: small-caps condensed 10px a");
        assert_eq!(page.property_value("font-size"), "10px");
        assert_eq!(page.property_value("font-variant-caps"), "small-caps");
        assert_eq!(page.property_value("font-stretch"), "condensed");
    }

    #[test]
    fn a_canvas_font_is_the_font_shorthand_alone() {
        for text in [
            "inherit",
            "initial",
            "12px",
            "serif",
            "var(--f)",
            "caption",
            "12px serif garbage,",
            "bold bold 12px a",
        ] {
            assert_eq!(font(text), None, "{text}");
        }
    }

    #[test]
    fn a_canvas_spacing_is_one_length_with_a_unit() {
        let spacing = |text: &str| parse_canvas_spacing(text);
        let em = spacing("1.5em").unwrap();
        assert_eq!(em.to_css(), "1.5em");
        assert_eq!(em.to_px(10.0), 15.0);
        assert_eq!(spacing("1rem").unwrap().to_px(10.0), 10.0);
        assert_eq!(spacing("-2px").unwrap().to_px(10.0), -2.0);
        assert_eq!(spacing("3PT").unwrap().to_css(), "3pt");
        for text in [
            "0",
            "1",
            "5%",
            " 1px",
            "1px ",
            "1px 2px",
            "calc(1px)",
            "1ex",
            "1vw",
            "",
        ] {
            assert_eq!(spacing(text), None, "{text:?}");
        }
    }
}
