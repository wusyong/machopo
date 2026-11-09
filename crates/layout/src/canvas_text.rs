//! A canvas's text laid out for its 2D context (`fillText`, `strokeText`,
//! `measureText`): one line, never wrapped, in the context's font, which
//! way the context says, with its spacings and kerning; and its first
//! font's metrics, which `textBaseline` and `TextMetrics` go by (Blink's
//! `PlainTextPainter` with the context's `Font`, and its `FontMetrics`).

use std::borrow::Cow;
use std::sync::Arc;

use computed_style::{CanvasTextStyle, Direction, FontStyle};
use gfx::{FontData, FontMetrics, GlyphRun, PointF, RectF, ShapedText};
use parley::{StyleProperty, TextWrapMode};
use read_fonts::types::Tag;
use read_fonts::{FontRef, TableProvider};

use crate::text::{TextContext, needs_synthetic_bold};

/// `text` (already made one line: the context's text preparation) laid
/// out in `style`. The metrics are the first font's, the one a space is
/// laid out in (Blink's primary font), which text with no glyphs has too.
pub fn shape_canvas_text(cx: &mut TextContext, text: &str, style: &CanvasTextStyle) -> ShapedText {
    let (runs, width) = shape(cx, text, style);
    let metrics = shape(cx, " ", style)
        .0
        .first()
        .map(|run| font_metrics(&run.font, run.size))
        .unwrap_or_default();
    ShapedText {
        runs,
        width,
        metrics,
        // Skia's, which the renderer adds (`raster::ink_bounds`).
        ink: RectF::default(),
    }
}

/// The runs and the line's width. No text is nothing (parley gives an
/// empty line a font size's width).
fn shape(cx: &mut TextContext, text: &str, style: &CanvasTextStyle) -> (Vec<GlyphRun>, f32) {
    if text.is_empty() {
        return (Vec::new(), 0.0);
    }
    let (fonts, layouts) = cx.contexts();
    let mut builder = layouts.ranged_builder(fonts, text, 1.0, true);
    builder.set_base_direction(match style.direction {
        Direction::Ltr => parley::BaseDirection::Ltr,
        Direction::Rtl => parley::BaseDirection::Rtl,
    });
    for property in properties(style) {
        builder.push_default(property);
    }
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    let mut runs = Vec::new();
    let mut width = 0.0;
    for line in layout.lines() {
        width += line.metrics().advance;
        for item in line.items() {
            let parley::PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let synthesis = run.synthesis();
            let baseline = glyph_run.baseline();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "an OpenType glyph id is 16 bits, which parley holds in a u32"
            )]
            let (glyphs, positions) = glyph_run
                .positioned_glyphs()
                .map(|g| (g.id as u16, PointF::new(g.x, g.y - baseline)))
                .unzip();
            runs.push(GlyphRun {
                font: FontData {
                    bytes: Arc::new(font.font.data.clone()),
                    index: font.font.index,
                    id: font.font.data.id(),
                },
                size: run.font_size(),
                glyphs,
                positions,
                embolden: synthesis.embolden()
                    || needs_synthetic_bold(style.font.weight, &font.font),
                skew: synthesis.skew().unwrap_or(0.0),
                variations: crate::font_features::run_variations(run, None),
            });
        }
    }
    (runs, width)
}

/// The parley properties for `style`: the font's family (with what Chrome
/// takes for each name, and the UA's standard font when none of it is
/// there, as for a page's text), size, weight, style and width, the caps
/// and kerning as OpenType features, the spacings.
fn properties(style: &CanvasTextStyle) -> Vec<StyleProperty<'static, u32>> {
    let font = &style.font;
    let families =
        crate::font_families::font_stack(&font.family, crate::font_families::Platform::CURRENT);
    // A page's text's, for the canvas's caps and kerning.
    let mut features = Vec::new();
    crate::font_features::caps_features(font.variant_caps, &mut features);
    crate::font_features::kerning_feature(style.kerning, &mut features);
    vec![
        StyleProperty::FontFamily(parley::FontFamily::List(Cow::Owned(families))),
        StyleProperty::FontSize(font.size),
        StyleProperty::FontWeight(parley::FontWeight::new(font.weight)),
        StyleProperty::FontStyle(match font.style {
            FontStyle::Normal => parley::FontStyle::Normal,
            FontStyle::Italic => parley::FontStyle::Italic,
            FontStyle::Oblique => parley::FontStyle::Oblique(None),
        }),
        StyleProperty::FontWidth(parley::FontWidth::from_ratio(
            font.stretch.percentage() / 100.0,
        )),
        StyleProperty::FontFeatures(parley::FontFeatures::List(Cow::Owned(features))),
        StyleProperty::LetterSpacing(style.letter_spacing),
        StyleProperty::WordSpacing(style.word_spacing),
        StyleProperty::TextWrapMode(TextWrapMode::NoWrap),
        StyleProperty::Brush(0),
    ]
}

/// `value` rounded to Blink's `LayoutUnit`, 1/64 px.
fn layout_unit(value: f32) -> f32 {
    (value * 64.0).round() / 64.0
}

/// The metrics of `font` at `size` px, from its tables (see
/// [`FontMetrics`]); none for a font that can't be read.
pub(crate) fn font_metrics(font: &FontData, size: f32) -> FontMetrics {
    let bytes: &[u8] = (*font.bytes).as_ref();
    let Ok(face) = FontRef::from_index(bytes, font.index) else {
        return FontMetrics::default();
    };
    let Ok(head) = face.head() else {
        return FontMetrics::default();
    };
    let scale = size / f32::from(head.units_per_em());
    let (ascent, descent) = face.hhea().map_or((0.0, 0.0), |hhea| {
        (
            f32::from(hhea.ascender().to_i16()) * scale,
            -f32::from(hhea.descender().to_i16()) * scale,
        )
    });
    // `TrySetNormalizedTypoAscentAndDescent`: OS/2's, else the font's
    // ascent and descent, in their ratio, to add up to 1em.
    let (typo_ascent, typo_descent) = face
        .os2()
        .ok()
        .map(|os2| {
            (
                f32::from(os2.s_typo_ascender()) * scale,
                -f32::from(os2.s_typo_descender()) * scale,
            )
        })
        .filter(|&(a, d)| a + d > 0.0 && a >= 0.0 && a <= a + d)
        .unwrap_or((ascent, descent));
    let height = typo_ascent + typo_descent;
    let (typo_ascent, typo_descent) = if height > 0.0 {
        (
            layout_unit(size * typo_ascent / height),
            layout_unit(size * typo_descent / height),
        )
    } else {
        (0.0, 0.0)
    };
    let base = |tag: &[u8; 4]| base_baseline(&face, Tag::new(tag)).map(|units| units * scale);
    FontMetrics {
        ascent,
        descent,
        typo_ascent,
        typo_descent,
        hanging: base(b"hang"),
        ideographic: base(b"ideo"),
        alphabetic: base(b"romn"),
    }
}

/// The BASE table's horizontal baseline `tag`, in font units, for the
/// default script (`DFLT`, else `latn`, else the first the font lists), as
/// HarfBuzz's `hb_ot_layout_get_baseline` reads it for Blink.
fn base_baseline(face: &FontRef<'_>, tag: Tag) -> Option<f32> {
    let axis = face.base().ok()?.horiz_axis()?.ok()?;
    let tags = axis.base_tag_list()?.ok()?;
    let index = tags.baseline_tags().iter().position(|t| t.get() == tag)?;
    let scripts = axis.base_script_list().ok()?;
    let records = scripts.base_script_records();
    let record = [Tag::new(b"DFLT"), Tag::new(b"latn")]
        .iter()
        .find_map(|script| records.iter().find(|r| r.base_script_tag() == *script))
        .or_else(|| records.first())?;
    let script = record.base_script(scripts.offset_data()).ok()?;
    let values = script.base_values()?.ok()?;
    let coord = values.base_coords().get(index).ok()?;
    Some(f32::from(coord.coordinate()))
}

#[cfg(test)]
mod tests {
    use computed_style::CanvasFont;

    use super::*;
    use computed_style::{FontFamily, FontKerning};

    /// `family` at `size` px, laid out left to right, no spacing.
    fn style(family: &str, size: f32) -> CanvasTextStyle {
        CanvasTextStyle {
            font: CanvasFont {
                family: Arc::new([FontFamily::Named(family.into())]),
                size,
                ..CanvasFont::default()
            },
            direction: Direction::Ltr,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            kerning: FontKerning::Auto,
        }
    }

    fn xs(text: &ShapedText) -> Vec<f32> {
        text.runs
            .iter()
            .flat_map(|run| run.positions.iter().map(|p| p.x))
            .collect()
    }

    #[test]
    fn text_is_laid_out_on_its_alphabetic_baseline_from_its_left_end() {
        let cx = &mut TextContext::new(true);
        let text = shape_canvas_text(cx, "XX", &style("Ahem", 20.0));
        assert_eq!(text.width, 40.0);
        assert_eq!(xs(&text), [0.0, 20.0]);
        assert!(text.runs[0].positions.iter().all(|p| p.y == 0.0));
        assert_eq!(text.runs[0].size, 20.0);
        // Ahem: 0.8em above the baseline, 0.2em below, no BASE.
        let m = text.metrics;
        assert_eq!((m.ascent, m.descent), (16.0, 4.0));
        assert_eq!((m.typo_ascent, m.typo_descent), (16.0, 4.0));
        assert_eq!((m.hanging, m.ideographic, m.alphabetic), (None, None, None));
        // No glyphs, the same metrics.
        let empty = shape_canvas_text(cx, "", &style("Ahem", 20.0));
        assert_eq!((empty.width, empty.runs.len()), (0.0, 0));
        assert_eq!(empty.metrics, m);
    }

    #[test]
    fn the_spacings_are_added() {
        let cx = &mut TextContext::new(true);
        let spaced = CanvasTextStyle {
            letter_spacing: 5.0,
            ..style("Ahem", 20.0)
        };
        let text = shape_canvas_text(cx, "XX", &spaced);
        assert_eq!(xs(&text), [0.0, 25.0]);
        assert_eq!(text.width, 50.0);
        let words = CanvasTextStyle {
            word_spacing: 7.0,
            ..style("Ahem", 10.0)
        };
        assert_eq!(shape_canvas_text(cx, "X X", &words).width, 37.0);
    }

    #[test]
    fn right_to_left_text_runs_from_the_right() {
        let cx = &mut TextContext::new(true);
        // An LTR word and a Hebrew letter: in an RTL paragraph the letter
        // comes first from the left.
        let mut glyphs = |direction| {
            let text = shape_canvas_text(
                cx,
                "XY\u{5d0}",
                &CanvasTextStyle {
                    direction,
                    ..style("Ahem", 10.0)
                },
            );
            let mut placed: Vec<(f32, u16)> = text
                .runs
                .iter()
                .flat_map(|run| {
                    run.positions
                        .iter()
                        .map(|p| p.x)
                        .zip(run.glyphs.iter().copied())
                })
                .collect();
            placed.sort_by(|a, b| a.0.total_cmp(&b.0));
            placed
                .into_iter()
                .map(|(_, glyph)| glyph)
                .collect::<Vec<_>>()
        };
        let ltr = glyphs(Direction::Ltr);
        let rtl = glyphs(Direction::Rtl);
        assert_eq!(rtl, [ltr[2], ltr[0], ltr[1]]);
    }

    #[test]
    fn kerning_can_be_turned_off() {
        let cx = &mut TextContext::new(true);
        // Lato (every generic family's test font) kerns "AV".
        let mut width = |kerning| {
            let style = CanvasTextStyle {
                kerning,
                font: CanvasFont {
                    size: 40.0,
                    ..CanvasFont::default()
                },
                ..style("Ahem", 40.0)
            };
            shape_canvas_text(cx, "AV", &style).width
        };
        let (kerned, normal, none) = (
            width(FontKerning::Auto),
            width(FontKerning::Normal),
            width(FontKerning::None),
        );
        assert_eq!(kerned, normal);
        assert!(none > kerned, "{none} vs {kerned}");
    }

    #[test]
    fn the_typo_lines_are_scaled_to_add_up_to_1em() {
        let cx = &mut TextContext::new(true);
        // Lato (every generic family's test font): OS/2's 1974 and 426 of
        // 2000 units, 1.2em in all; hhea's the same.
        let style = CanvasTextStyle {
            font: CanvasFont {
                size: 40.0,
                ..CanvasFont::default()
            },
            ..style("Ahem", 40.0)
        };
        let m = shape_canvas_text(cx, "A", &style).metrics;
        assert!((m.ascent - 39.48).abs() < 1e-4 && (m.descent - 8.52).abs() < 1e-4);
        // 40 × 1974 / 2400 = 32.9 and 7.1, each to 1/64 px.
        assert_eq!((m.typo_ascent, m.typo_descent), (32.90625, 7.09375));
    }

    #[test]
    fn the_base_tables_baselines_are_read() {
        // WPT's CanvasTest.ttf, whose BASE gives a hanging baseline at
        // 0.5em and an ideographic one at 0.125em, both above the
        // alphabetic one.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../third_party/wpt/fonts/CanvasTest.ttf"
        );
        let bytes = std::fs::read(path).expect("the WPT checkout's CanvasTest.ttf");
        let font = FontData {
            bytes: Arc::new(bytes),
            index: 0,
            id: 1,
        };
        let m = font_metrics(&font, 50.0);
        assert_eq!(m.hanging, Some(25.0));
        assert_eq!(m.ideographic, Some(6.25));
        assert_eq!(m.alphabetic, Some(0.0));
    }
}
