//! What the font properties ask the shaper for: OpenType features from
//! `font-variant-*`, `font-kerning`, `text-rendering` and
//! `font-feature-settings`, variations from `font-variation-settings`
//! (Blink's `FontFeatures::Initialize` and
//! `FontDescription::UpdateTypesettingFeatures`), and whether a bold or an
//! italic may be synthesized (`font-synthesis-*`).

use computed_style::{
    EastAsianVariant, EastAsianWidth, FontDescription, FontKerning, FontVariantCaps,
    FontVariantPosition, NumericFigure, NumericFraction, NumericSpacing, TextRendering,
};
use std::sync::Arc;

use parley::setting::Tag;
use parley::{FontFeature, FontVariation};

fn feature(tag: &[u8; 4], value: u16) -> FontFeature {
    FontFeature::new(Tag::new(tag), value)
}

/// `font-variant-caps`' features (no small caps synthesized where the font
/// lacks them, unlike Blink).
pub(crate) fn caps_features(caps: FontVariantCaps, out: &mut Vec<FontFeature>) {
    match caps {
        FontVariantCaps::Normal => {}
        FontVariantCaps::SmallCaps => out.push(feature(b"smcp", 1)),
        FontVariantCaps::AllSmallCaps => {
            out.extend([feature(b"smcp", 1), feature(b"c2sc", 1)]);
        }
        FontVariantCaps::PetiteCaps => out.push(feature(b"pcap", 1)),
        FontVariantCaps::AllPetiteCaps => {
            out.extend([feature(b"pcap", 1), feature(b"c2pc", 1)]);
        }
        FontVariantCaps::Unicase => out.push(feature(b"unic", 1)),
        FontVariantCaps::TitlingCaps => out.push(feature(b"titl", 1)),
    }
}

/// `font-kerning`'s: the shaper kerns by default.
pub(crate) fn kerning_feature(kerning: FontKerning, out: &mut Vec<FontFeature>) {
    match kerning {
        FontKerning::Auto => {}
        FontKerning::Normal => out.push(feature(b"kern", 1)),
        FontKerning::None => out.push(feature(b"kern", 0)),
    }
}

/// The features a page's text is shaped with, in Blink's order, later ones
/// winning: kerning, ligatures, caps, East Asian forms, numerals,
/// position, historical forms, then `font-feature-settings`. Empty for
/// the initial values: the shaper's defaults.
pub(crate) fn features(font: &FontDescription) -> Vec<FontFeature> {
    let mut out = Vec::new();
    // `optimizeSpeed` turns off kerning and the common ligatures, but where
    // `font-kerning` or `font-variant-ligatures` asks for them.
    let speed = font.text_rendering == TextRendering::OptimizeSpeed;
    match font.kerning {
        FontKerning::Auto if speed => out.push(feature(b"kern", 0)),
        kerning => kerning_feature(kerning, &mut out),
    }
    let ligatures = font.variant_ligatures;
    if ligatures.none {
        out.extend(
            [b"liga", b"clig", b"dlig", b"hlig", b"calt"]
                .into_iter()
                .map(|tag| feature(tag, 0)),
        );
    } else {
        match ligatures.common {
            Some(on) => out.extend([feature(b"liga", on.into()), feature(b"clig", on.into())]),
            None if speed => out.extend([feature(b"liga", 0), feature(b"clig", 0)]),
            None => {}
        }
        for (value, tag) in [
            (ligatures.discretionary, b"dlig"),
            (ligatures.historical, b"hlig"),
            (ligatures.contextual, b"calt"),
        ] {
            if let Some(on) = value {
                out.push(feature(tag, on.into()));
            }
        }
    }
    caps_features(font.variant_caps, &mut out);
    let east_asian = font.variant_east_asian;
    if let Some(variant) = east_asian.variant {
        out.push(feature(
            match variant {
                EastAsianVariant::Jis78 => b"jp78",
                EastAsianVariant::Jis83 => b"jp83",
                EastAsianVariant::Jis90 => b"jp90",
                EastAsianVariant::Jis04 => b"jp04",
                EastAsianVariant::Simplified => b"smpl",
                EastAsianVariant::Traditional => b"trad",
            },
            1,
        ));
    }
    match east_asian.width {
        Some(EastAsianWidth::Full) => out.push(feature(b"fwid", 1)),
        Some(EastAsianWidth::Proportional) => out.push(feature(b"pwid", 1)),
        None => {}
    }
    if east_asian.ruby {
        out.push(feature(b"ruby", 1));
    }
    let numeric = font.variant_numeric;
    match numeric.figure {
        Some(NumericFigure::Lining) => out.push(feature(b"lnum", 1)),
        Some(NumericFigure::Oldstyle) => out.push(feature(b"onum", 1)),
        None => {}
    }
    match numeric.spacing {
        Some(NumericSpacing::Proportional) => out.push(feature(b"pnum", 1)),
        Some(NumericSpacing::Tabular) => out.push(feature(b"tnum", 1)),
        None => {}
    }
    match numeric.fraction {
        Some(NumericFraction::Diagonal) => out.push(feature(b"frac", 1)),
        Some(NumericFraction::Stacked) => out.push(feature(b"afrc", 1)),
        None => {}
    }
    if numeric.ordinal {
        out.push(feature(b"ordn", 1));
    }
    if numeric.slashed_zero {
        out.push(feature(b"zero", 1));
    }
    // The font's sub- and superscript glyphs: none synthesized where it
    // lacks them (Blink makes them smaller and moves them).
    match font.variant_position {
        FontVariantPosition::Normal => {}
        FontVariantPosition::Sub => out.push(feature(b"subs", 1)),
        FontVariantPosition::Super => out.push(feature(b"sups", 1)),
    }
    if font.variant_alternates.historical_forms {
        out.push(feature(b"hist", 1));
    }
    out.extend(font.feature_settings.iter().map(|setting| {
        // A value past `u16` is a lookup's alternate index no font has.
        feature(&setting.tag, base::numerics::saturated_cast(setting.value))
    }));
    out
}

/// The axes text is shaped with: `opsz` at the font size for
/// `font-optical-sizing: auto` (CSS Fonts 4, as Blink's
/// `FontPlatformData` adds it to the variations it hands the shaper),
/// then `font-variation-settings`', which win where they set it. A font
/// without the axis ignores it.
pub(crate) fn variations(font: &FontDescription) -> Vec<FontVariation> {
    let settings = font
        .variation_settings
        .iter()
        .map(|setting| FontVariation::new(Tag::new(&setting.tag), setting.value));
    let optical = font.optical_sizing && !font.variation_settings.iter().any(|v| v.tag == *b"opsz");
    optical
        .then(|| FontVariation::new(Tag::new(b"opsz"), font.size))
        .into_iter()
        .chain(settings)
        .collect()
}

/// The axes a run of a variable font is drawn with, as the shaper set
/// them (later ones winning): fontique's for the weight, width and slant it
/// asked for, `opsz` at the font size for `font-optical-sizing: auto`, then
/// `font-variation-settings`. Empty for a font with no axes (the shaper
/// gave the run no coordinates). `font`: the text's, `None` for a canvas's
/// (no optical sizing or variation settings).
pub(crate) fn run_variations(
    run: &parley::Run<'_, u32>,
    font: Option<&FontDescription>,
) -> Arc<[gfx::FontVariation]> {
    if run.normalized_coords().is_empty() {
        return Arc::new([]);
    }
    let mut out: Vec<gfx::FontVariation> = Vec::new();
    let mut set = |tag: [u8; 4], value: f32| match out.iter_mut().find(|v| v.tag == tag) {
        Some(v) => v.value = value,
        None => out.push(gfx::FontVariation { tag, value }),
    };
    for (tag, value) in run.synthesis().variation_settings() {
        set(tag.to_be_bytes(), *value);
    }
    if let Some(font) = font {
        if font.optical_sizing && !font.variation_settings.iter().any(|v| v.tag == *b"opsz") {
            set(*b"opsz", run.font_size());
        }
        for setting in font.variation_settings.iter() {
            set(setting.tag, setting.value);
        }
    }
    out.into()
}

/// Whether a run is drawn bold though its face isn't (`face_bold`: fontique
/// or machopo found it should be), as `font-synthesis-weight` allows.
pub(crate) fn embolden(font: &FontDescription, face_bold: bool) -> bool {
    font.synthesis_weight && face_bold
}

/// The skew a run is drawn with (fontique's, for an italic or oblique face
/// it hasn't), as `font-synthesis-style` allows.
pub(crate) fn skew(font: &FontDescription, skew: Option<f32>) -> f32 {
    if font.synthesis_style {
        skew.unwrap_or(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use computed_style::{
        FontFeatureSetting, FontVariantEastAsian, FontVariantLigatures, FontVariantNumeric,
    };

    fn tags(font: &FontDescription) -> Vec<(String, u16)> {
        features(font)
            .iter()
            .map(|f| {
                (
                    String::from_utf8(f.tag.to_bytes().to_vec()).unwrap(),
                    f.value,
                )
            })
            .collect()
    }

    fn initial() -> FontDescription {
        (*computed_style::ComputedStyle::initial().font).clone()
    }

    #[track_caller]
    fn assert_tags(font: FontDescription, expected: &[(&str, u16)]) {
        let expected: Vec<(String, u16)> =
            expected.iter().map(|&(t, v)| (t.to_owned(), v)).collect();
        assert_eq!(tags(&font), expected);
    }

    /// Each value's OpenType features, in Blink's order, the settings last;
    /// none for the initial values.
    #[test]
    fn each_value_asks_for_its_features() {
        assert_tags(initial(), &[]);
        let mut font = initial();
        font.kerning = FontKerning::None;
        font.variant_ligatures = FontVariantLigatures {
            common: Some(false),
            discretionary: Some(true),
            historical: Some(true),
            contextual: Some(false),
            ..Default::default()
        };
        font.variant_caps = FontVariantCaps::AllPetiteCaps;
        font.variant_east_asian = FontVariantEastAsian {
            variant: Some(EastAsianVariant::Jis90),
            width: Some(EastAsianWidth::Proportional),
            ruby: true,
        };
        font.variant_numeric = FontVariantNumeric {
            figure: Some(NumericFigure::Oldstyle),
            spacing: Some(NumericSpacing::Tabular),
            fraction: Some(NumericFraction::Stacked),
            ordinal: true,
            slashed_zero: true,
        };
        font.variant_position = FontVariantPosition::Sub;
        font.variant_alternates.historical_forms = true;
        font.feature_settings = std::sync::Arc::new([FontFeatureSetting {
            tag: *b"liga",
            value: 1,
        }]);
        assert_tags(
            font,
            &[
                ("kern", 0),
                ("liga", 0),
                ("clig", 0),
                ("dlig", 1),
                ("hlig", 1),
                ("calt", 0),
                ("pcap", 1),
                ("c2pc", 1),
                ("jp90", 1),
                ("pwid", 1),
                ("ruby", 1),
                ("onum", 1),
                ("tnum", 1),
                ("afrc", 1),
                ("ordn", 1),
                ("zero", 1),
                ("subs", 1),
                ("hist", 1),
                ("liga", 1),
            ],
        );
        let mut font = initial();
        font.variant_ligatures.none = true;
        font.variant_east_asian.variant = Some(EastAsianVariant::Traditional);
        font.variant_east_asian.width = Some(EastAsianWidth::Full);
        font.variant_numeric.figure = Some(NumericFigure::Lining);
        font.variant_numeric.spacing = Some(NumericSpacing::Proportional);
        font.variant_numeric.fraction = Some(NumericFraction::Diagonal);
        font.variant_position = FontVariantPosition::Super;
        assert_tags(
            font,
            &[
                ("liga", 0),
                ("clig", 0),
                ("dlig", 0),
                ("hlig", 0),
                ("calt", 0),
                ("trad", 1),
                ("fwid", 1),
                ("lnum", 1),
                ("pnum", 1),
                ("frac", 1),
                ("sups", 1),
            ],
        );
        for (caps, expected) in [
            (FontVariantCaps::SmallCaps, vec![("smcp", 1)]),
            (
                FontVariantCaps::AllSmallCaps,
                vec![("smcp", 1), ("c2sc", 1)],
            ),
            (FontVariantCaps::PetiteCaps, vec![("pcap", 1)]),
            (FontVariantCaps::Unicase, vec![("unic", 1)]),
            (FontVariantCaps::TitlingCaps, vec![("titl", 1)]),
        ] {
            let mut font = initial();
            font.variant_caps = caps;
            assert_tags(font, &expected);
        }
        for (variant, tag) in [
            (EastAsianVariant::Jis78, "jp78"),
            (EastAsianVariant::Jis83, "jp83"),
            (EastAsianVariant::Jis04, "jp04"),
            (EastAsianVariant::Simplified, "smpl"),
        ] {
            let mut font = initial();
            font.variant_east_asian.variant = Some(variant);
            assert_tags(font, &[(tag, 1)]);
        }
        // `optimizeSpeed`: kerning and the common ligatures off, but where
        // asked for.
        let mut font = initial();
        font.text_rendering = TextRendering::OptimizeSpeed;
        assert_tags(font.clone(), &[("kern", 0), ("liga", 0), ("clig", 0)]);
        font.kerning = FontKerning::Normal;
        font.variant_ligatures.common = Some(true);
        assert_tags(font, &[("kern", 1), ("liga", 1), ("clig", 1)]);
    }
}
