//! A 2D context's text (Blink's `BaseRenderingContext2D`'s text members
//! and `TextMetrics`), apart from any DOM: the text drawing styles' names,
//! what is drawn of a string, where its glyphs go for `fillText` and
//! `strokeText` once the renderer laid it out, and `measureText`'s
//! numbers. Each DOM finds the font and the direction, and has the text
//! laid out.

use base::numerics::f32_from_f64;
use compositor_paint::{PaintFlags, PaintOp};
use computed_style::{
    Direction, FontKerning, FontStretch, FontVariantCaps, TextAlign, TextBaseline, TextRendering,
};
use gfx::{FontMetrics, PointF, RectF, ShapedText};

use crate::{CanvasDrawingState, TextMetricsValues};

/// `textAlign`'s values (`CanvasTextAlign`).
pub const TEXT_ALIGNS: [(&str, TextAlign); 5] = [
    ("start", TextAlign::Start),
    ("end", TextAlign::End),
    ("left", TextAlign::Left),
    ("right", TextAlign::Right),
    ("center", TextAlign::Center),
];

/// `textBaseline`'s values (`CanvasTextBaseline`).
pub const TEXT_BASELINES: [(&str, TextBaseline); 6] = [
    ("top", TextBaseline::Top),
    ("hanging", TextBaseline::Hanging),
    ("middle", TextBaseline::Middle),
    ("alphabetic", TextBaseline::Alphabetic),
    ("ideographic", TextBaseline::Ideographic),
    ("bottom", TextBaseline::Bottom),
];

/// `direction`'s values (`CanvasDirection`): `inherit` is none, kept as it
/// is set (Blink's `CanvasTextDirectionReflect`).
pub const DIRECTIONS: [(&str, Option<Direction>); 3] = [
    ("ltr", Some(Direction::Ltr)),
    ("rtl", Some(Direction::Rtl)),
    ("inherit", None),
];

/// `fontKerning`'s values (`CanvasFontKerning`).
pub const FONT_KERNINGS: [(&str, FontKerning); 3] = [
    ("auto", FontKerning::Auto),
    ("normal", FontKerning::Normal),
    ("none", FontKerning::None),
];

/// `fontStretch`'s values (`CanvasFontStretch`): the font's stretch.
pub const FONT_STRETCHES: [(&str, FontStretch); 9] = [
    ("ultra-condensed", FontStretch::UltraCondensed),
    ("extra-condensed", FontStretch::ExtraCondensed),
    ("condensed", FontStretch::Condensed),
    ("semi-condensed", FontStretch::SemiCondensed),
    ("normal", FontStretch::Normal),
    ("semi-expanded", FontStretch::SemiExpanded),
    ("expanded", FontStretch::Expanded),
    ("extra-expanded", FontStretch::ExtraExpanded),
    ("ultra-expanded", FontStretch::UltraExpanded),
];

/// `fontVariantCaps`'s values (`CanvasFontVariantCaps`): the font's caps.
pub const FONT_VARIANT_CAPS: [(&str, FontVariantCaps); 7] = [
    ("normal", FontVariantCaps::Normal),
    ("small-caps", FontVariantCaps::SmallCaps),
    ("all-small-caps", FontVariantCaps::AllSmallCaps),
    ("petite-caps", FontVariantCaps::PetiteCaps),
    ("all-petite-caps", FontVariantCaps::AllPetiteCaps),
    ("unicase", FontVariantCaps::Unicase),
    ("titling-caps", FontVariantCaps::TitlingCaps),
];

/// `textRendering`'s values (`CanvasTextRendering`): only kept and read
/// back (Chromium picks hinting with it, and machopo's raster has none).
pub const TEXT_RENDERINGS: [(&str, TextRendering); 4] = [
    ("auto", TextRendering::Auto),
    ("optimizeSpeed", TextRendering::OptimizeSpeed),
    ("optimizeLegibility", TextRendering::OptimizeLegibility),
    ("geometricPrecision", TextRendering::GeometricPrecision),
];

/// What a context draws of `text`: ASCII white space each a space, none
/// collapsed (HTML's text preparation; Blink's `TextRun::SetNormalizeSpace`).
pub fn prepare_text(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' | '\n' | '\u{c}' | '\r' => ' ',
            c => c,
        })
        .collect()
}

/// The baseline's height above the alphabetic one in a font with
/// `metrics` (Blink's `GetFontBaseline`): `top` and `bottom` the 1em box's
/// edges, `middle` halfway; `hanging`, `ideographic` and `alphabetic` the
/// font's BASE table's, else 80% of the ascent, the descent and 0.
pub fn baseline_offset(baseline: TextBaseline, metrics: &FontMetrics) -> f32 {
    match baseline {
        TextBaseline::Top => metrics.typo_ascent,
        TextBaseline::Bottom => -metrics.typo_descent,
        TextBaseline::Middle => (metrics.typo_ascent - metrics.typo_descent) / 2.0,
        TextBaseline::Hanging => metrics.hanging.unwrap_or(metrics.ascent * 0.8),
        TextBaseline::Ideographic => metrics.ideographic.unwrap_or(-metrics.descent),
        TextBaseline::Alphabetic => metrics.alphabetic.unwrap_or(0.0),
    }
}

/// How far left of the alignment point text `width` wide starts (Blink's
/// `text_align_dx_`): none for `left`, half for `center`, all for `right`;
/// `start` and `end` are left or right by the direction.
pub fn align_offset(align: TextAlign, direction: Direction, width: f64) -> f64 {
    let rtl = direction == Direction::Rtl;
    match (align, rtl) {
        (TextAlign::Center, _) => width / 2.0,
        (TextAlign::Right, _) | (TextAlign::Start, true) | (TextAlign::End, false) => width,
        (TextAlign::Left, _) | (TextAlign::Start, false) | (TextAlign::End, true) => 0.0,
    }
}

/// Whether `fillText`'s and `strokeText`'s numbers draw anything: `x` and
/// `y` finite, a `maxWidth` finite and above 0.
pub fn text_args_drawable(x: f64, y: f64, max_width: Option<f64>) -> bool {
    x.is_finite() && y.is_finite() && max_width.is_none_or(|width| width.is_finite() && width > 0.0)
}

/// `fillText`'s and `strokeText`'s ops (Blink's `DrawTextInternal`), of
/// text `shaped` on one line in `direction`: its baseline at `y` as
/// `textBaseline` says, at `x` as `textAlign` says (`start` and `end` by
/// the direction); squeezed across to `maxWidth` if it is wider. The
/// horizontal scale to record them under (1 unless squeezed), and the
/// glyphs drawn with `flags`.
pub fn text_ops(
    state: &CanvasDrawingState,
    direction: Direction,
    shaped: ShapedText,
    (x, y, max_width): (f64, f64, Option<f64>),
    flags: &PaintFlags,
    stroke: bool,
) -> (f64, Vec<PaintOp>) {
    let font_width = f64::from(shaped.width);
    let squeeze = max_width.filter(|&width| width < font_width);
    let width = squeeze.unwrap_or(font_width);
    let left = x - align_offset(state.text_align, direction, width);
    let baseline = f32_from_f64(y) + baseline_offset(state.text_baseline, &shaped.metrics);
    // Squeezed, the glyphs are laid out in the scaled space, where the
    // left end is `left` scaled back.
    let scale = squeeze.map_or(1.0, |width| width / font_width);
    let origin = PointF::new(f32_from_f64(left / scale), baseline);
    // As a page's text's: the ink may go past the font's lines and the
    // advances by up to about the size; a stroke past that by as far as
    // its joins reach.
    let metrics = &shaped.metrics;
    let size = state.font.size;
    let mut bounds = RectF::new(
        origin.x - size,
        baseline - metrics.ascent - size / 2.0,
        shaped.width + 2.0 * size,
        metrics.ascent + metrics.descent + size,
    );
    if stroke {
        let reach = f32_from_f64(state.miter_limit).max(std::f32::consts::SQRT_2);
        bounds = bounds.inset(-f32_from_f64(state.line_width) / 2.0 * reach);
    }
    // Blink draws no glyphs too, for the composite operations that clear
    // what isn't drawn over (which machopo doesn't have yet).
    let ops = shaped
        .runs
        .into_iter()
        .map(|run| PaintOp::DrawGlyphs {
            font: run.font,
            size: run.size,
            glyphs: run.glyphs.into(),
            positions: run
                .positions
                .iter()
                .map(|p| PointF::new(origin.x + p.x, origin.y + p.y))
                .collect(),
            rotations: Vec::new().into(),
            flags: flags.clone(),
            embolden: run.embolden,
            skew: run.skew,
            variations: run.variations,
            bounds,
        })
        .collect();
    (scale, ops)
}

/// `measureText`'s numbers (Blink's `TextMetrics::Update`) of text
/// `shaped` (with its ink) in `direction`, laid out as `fillText` would:
/// its width; its ink's edges (`actualBoundingBox`) from the alignment
/// point across and the `textBaseline`'s line up and down; the first
/// font's ascent and descent (`fontBoundingBox`) and baselines from that
/// line too.
pub fn text_metrics(
    state: &CanvasDrawingState,
    direction: Direction,
    shaped: &ShapedText,
) -> TextMetricsValues {
    let width = f64::from(shaped.width);
    let dx = align_offset(state.text_align, direction, width);
    let metrics = &shaped.metrics;
    // How far the alphabetic baseline is below the `textBaseline`'s line
    // (Blink's `baseline_y`), and each baseline's height above that line.
    let below = f64::from(baseline_offset(state.text_baseline, metrics));
    let line = |baseline| f64::from(baseline_offset(baseline, metrics)) - below;
    let ink = shaped.ink;
    TextMetricsValues {
        width,
        actual_bounding_box_left: dx - f64::from(ink.x),
        actual_bounding_box_right: f64::from(ink.right()) - dx,
        font_bounding_box_ascent: f64::from(metrics.ascent) - below,
        font_bounding_box_descent: f64::from(metrics.descent) + below,
        actual_bounding_box_ascent: -f64::from(ink.y) - below,
        actual_bounding_box_descent: f64::from(ink.bottom()) + below,
        hanging_baseline: line(TextBaseline::Hanging),
        alphabetic_baseline: line(TextBaseline::Alphabetic),
        ideographic_baseline: line(TextBaseline::Ideographic),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_space_is_each_a_space() {
        assert_eq!(prepare_text("a\tb\n\r\u{c}c  d"), "a b   c  d");
    }

    #[test]
    fn start_and_end_go_by_the_direction() {
        assert_eq!(align_offset(TextAlign::Start, Direction::Rtl, 10.0), 10.0);
        assert_eq!(align_offset(TextAlign::End, Direction::Rtl, 10.0), 0.0);
        assert_eq!(align_offset(TextAlign::Center, Direction::Ltr, 10.0), 5.0);
    }

    #[test]
    fn numbers_that_draw_nothing() {
        assert!(text_args_drawable(0.0, 0.0, None));
        assert!(!text_args_drawable(f64::NAN, 0.0, None));
        assert!(!text_args_drawable(0.0, 0.0, Some(0.0)));
        assert!(!text_args_drawable(0.0, 0.0, Some(f64::INFINITY)));
    }
}
