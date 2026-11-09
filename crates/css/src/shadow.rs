//! `box-shadow` (css-backgrounds 3, "drop shadows") and `text-shadow`
//! (css-text-decor 3, the same with no spread and no `inset`): parsing,
//! computing and serializing the list of shadows.

use std::sync::Arc;

use computed_style::{BoxShadow, StyleColor};
use cssparser::Parser;

use crate::values::{ComputeContext, Length, ParseResult, SpecifiedColor, parse_style_color};

/// One shadow as written (Blink's `CSSShadowValue`): what was left out
/// stays out when serialized.
#[derive(Debug, Clone, PartialEq)]
pub struct SpecifiedShadow {
    pub(crate) x: Length,
    pub(crate) y: Length,
    pub(crate) blur: Option<Length>,
    pub(crate) spread: Option<Length>,
    pub(crate) color: Option<SpecifiedColor>,
    pub(crate) inset: bool,
}

impl SpecifiedShadow {
    /// `inset? && <length>{2,4} && <color>?`: the parts in any order, the
    /// lengths together; the blur not negative. A `text` shadow's are
    /// `<length>{2,3} && <color>?`.
    pub(crate) fn parse<'i>(input: &mut Parser<'i, '_>, text: bool) -> ParseResult<'i, Self> {
        let (mut lengths, mut color, mut inset) = (None, None, false);
        loop {
            if !inset
                && !text
                && input
                    .try_parse(|i| i.expect_ident_matching("inset"))
                    .is_ok()
            {
                inset = true;
                continue;
            }
            if lengths.is_none()
                && let Ok(x) = input.try_parse(|i| Length::parse(i, true))
            {
                let y = Length::parse(input, true)?;
                let blur = input.try_parse(|i| Length::parse(i, false)).ok();
                let spread = blur
                    .as_ref()
                    .filter(|_| !text)
                    .and_then(|_| input.try_parse(|i| Length::parse(i, true)).ok());
                lengths = Some((x, y, blur, spread));
                continue;
            }
            if color.is_none()
                && let Ok(c) = input.try_parse(parse_style_color)
            {
                color = Some(c);
                continue;
            }
            break;
        }
        let Some((x, y, blur, spread)) = lengths else {
            return Err(input.new_custom_error(()));
        };
        Ok(Self {
            x,
            y,
            blur,
            spread,
            color,
            inset,
        })
    }

    pub(crate) fn compute(&self, cx: &ComputeContext) -> BoxShadow {
        let px = |length: &Option<Length>| length.as_ref().map_or(0.0, |l| l.to_px(cx));
        BoxShadow {
            x: self.x.to_px(cx),
            y: self.y.to_px(cx),
            blur: px(&self.blur),
            spread: px(&self.spread),
            // An omitted color is `currentcolor`.
            color: self
                .color
                .as_ref()
                .map_or(StyleColor::CurrentColor, |color| color.compute(cx)),
            inset: self.inset,
        }
    }

    /// As Blink writes it (`CSSShadowValue::CustomCSSText`): the color,
    /// the lengths, then `inset`, each part only if it was written.
    pub(crate) fn to_css(&self, dest: &mut String) {
        let mut parts = Vec::new();
        if let Some(color) = &self.color {
            let mut text = String::new();
            color.to_css(&mut text);
            parts.push(text);
        }
        for length in [
            Some(&self.x),
            Some(&self.y),
            self.blur.as_ref(),
            self.spread.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let mut text = String::new();
            length.to_css(&mut text);
            parts.push(text);
        }
        if self.inset {
            parts.push("inset".to_owned());
        }
        dest.push_str(&parts.join(" "));
    }
}

/// `none | <shadow>#`; `none` is the empty list.
pub(crate) fn parse_shadows<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[SpecifiedShadow]>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::new([]));
    }
    Ok(input
        .parse_comma_separated(|i| SpecifiedShadow::parse(i, false))?
        .into())
}

/// `text-shadow`'s `none | <shadow>#`.
pub(crate) fn parse_text_shadows<'i>(
    input: &mut Parser<'i, '_>,
) -> ParseResult<'i, Arc<[SpecifiedShadow]>> {
    if input.try_parse(|i| i.expect_ident_matching("none")).is_ok() {
        return Ok(Arc::new([]));
    }
    Ok(input
        .parse_comma_separated(|i| SpecifiedShadow::parse(i, true))?
        .into())
}

pub(crate) fn compute_shadows(
    shadows: &[SpecifiedShadow],
    cx: &ComputeContext,
) -> Arc<[BoxShadow]> {
    shadows.iter().map(|shadow| shadow.compute(cx)).collect()
}

pub(crate) fn shadows_to_css(shadows: &[SpecifiedShadow], dest: &mut String) {
    if shadows.is_empty() {
        dest.push_str("none");
        return;
    }
    for (i, shadow) in shadows.iter().enumerate() {
        if i > 0 {
            dest.push_str(", ");
        }
        shadow.to_css(dest);
    }
}
