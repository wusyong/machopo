//! A text run's decorations as Blink draws them (`TextDecorationInfo`,
//! `TextDecorationOffset` and `DecorationLinePainter`), and its shadows
//! (`TextPainter`'s `cc::DrawLooper`): each line's place, thickness and
//! style from the run's font, worked out in device pixels as Blink, which
//! lays out in them, rounds there. Blink places an ancestor's decoration
//! with the decorating box's font where it can (`InlinePaintContext`);
//! machopo with each run's own.

use compositor_paint::{Dash, LineCap, PaintFlags, PaintOp};
use computed_style::{
    AppliedTextDecoration, BoxShadow, StyleColor, TextDecorationStyle, TextDecorationThickness,
};
use gfx::{Color, Path, PointF, RectF};
use layout::TextFragment;

/// Which of a decoration's lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Line {
    Under,
    Over,
    Through,
}

/// `decoration`'s `line` on `run`, `width` long from `left`, the top of the
/// run's text (its ascent above the baseline) at `top`: what to draw, each
/// with what it may touch. In CSS px; `dsf` device pixels to one.
pub(crate) fn line_ops(
    run: &TextFragment,
    decoration: &AppliedTextDecoration,
    line: Line,
    (left, top, width): (f32, f32, f32),
    color: Color,
    dsf: f32,
) -> Vec<(PaintOp, RectF)> {
    // Device pixels, as Blink's are.
    let font_size = run.font_size * dsf;
    let ascent = run.ascent * dsf;
    let thickness = match decoration.thickness {
        TextDecorationThickness::Auto => font_size / 10.0,
        TextDecorationThickness::FromFont if run.underline_size > 0.0 => run.underline_size * dsf,
        TextDecorationThickness::FromFont => font_size / 10.0,
        TextDecorationThickness::Length(length) => (length.resolve(run.font_size) * dsf).round(),
    }
    .max(1.0);
    // From the top of the text: `TextDecorationOffset`.
    let offset = match line {
        Line::Under => underline_offset(run, decoration, thickness, dsf),
        // Above the text, its bottom edge at its top.
        Line::Over => -thickness.floor(),
        Line::Through => 2.0 * ascent / 3.0 - thickness / 2.0,
    };
    let rect = RectF::new(left * dsf, top * dsf + offset, width * dsf, thickness);
    if rect.width <= 0.0 {
        return Vec::new();
    }
    // The second line of `double`, and the wave's middle: below an
    // underline, above an overline (`ComputeLineData`).
    let apart = thickness + 1.0;
    let (double, wavy) = match line {
        Line::Under => (apart, apart),
        Line::Over => (-apart, -apart),
        Line::Through => (apart.floor(), 0.0),
    };
    let css = 1.0 / dsf;
    match decoration.style {
        TextDecorationStyle::Solid => vec![solid(rect, color, css)],
        TextDecorationStyle::Double => vec![
            solid(rect, color, css),
            solid(
                RectF::new(rect.x, rect.y + double, rect.width, rect.height),
                color,
                css,
            ),
        ],
        TextDecorationStyle::Dotted | TextDecorationStyle::Dashed => {
            vec![stroked(rect, decoration.style, color, css)]
        }
        TextDecorationStyle::Wavy => vec![wave(rect, wavy, color, css)],
    }
}

/// An underline's top below the text's top (device px): the font's
/// ascent and a gap of half the thickness (at least 1, as the thickness
/// is) for `auto`, the
/// font's underline position for `from-font`, the em box's bottom for
/// `under`; moved by `text-underline-offset` (`ComputeUnderlineOffset`).
fn underline_offset(
    run: &TextFragment,
    decoration: &AppliedTextDecoration,
    thickness: f32,
    dsf: f32,
) -> f32 {
    let ascent = run.ascent * dsf;
    let moved = decoration
        .underline_offset
        .map_or(0.0, |offset| offset.resolve(run.font_size) * dsf);
    let auto = || {
        let gap = if decoration.underline_offset.is_none() {
            (thickness / 2.0).ceil()
        } else {
            0.0
        };
        ascent.round() + gap + moved.round()
    };
    let position = run.style.rare_inherited.text_underline_position;
    if position.under {
        // The em box's share of the descent (Blink's `EmHeightDescent`
        // normalizes the font's ascent and descent to the em).
        let em_descent = run.font_size * dsf * run.descent / (run.ascent + run.descent).max(1e-3);
        return (ascent + em_descent + moved).floor() + 1.0;
    }
    if position.from_font && run.underline_size > 0.0 {
        return (ascent - run.underline_offset * dsf + moved).round();
    }
    auto()
}

/// A solid line: its top at a whole device pixel, its thickness whole
/// (Blink's `SnapYAxis`), so it isn't blurred over two; its ends too, as
/// a background's (Blink lays out in device pixels, the text where they
/// meet). `rect` in device px, what is drawn in px `css` of them.
fn solid(rect: RectF, color: Color, css: f32) -> (PaintOp, RectF) {
    let (left, right) = (rect.x.round(), (rect.x + rect.width).round());
    let rect = scale_rect(
        RectF::new(
            left,
            (rect.y + 0.5).floor(),
            right - left,
            rect.height.floor().max(1.0),
        ),
        css,
    );
    let op = PaintOp::DrawRect {
        rect,
        flags: PaintFlags::fill(color),
    };
    (op, rect)
}

/// A dotted or dashed line, stroked along its middle (Blink's
/// `DrawLineAsStroke`): whole pixels across, its dashes as
/// `StyledStrokeData` sizes them; dots inset by half a dot, so the
/// round caps stay inside.
fn stroked(rect: RectF, style: TextDecorationStyle, color: Color, css: f32) -> (PaintOp, RectF) {
    let middle = (rect.y + (rect.height / 2.0).max(0.5)).floor();
    let (start, end) = (rect.x.trunc(), (rect.x + rect.width).trunc());
    let thickness = rect.height.round();
    let y = if thickness % 2.0 == 1.0 {
        middle + 0.5
    } else {
        middle
    };
    let dashed = style == TextDecorationStyle::Dashed || thickness <= 3.0;
    let (mut x0, mut x1) = (start, end);
    if !dashed {
        x0 += thickness / 2.0;
        x1 -= thickness / 2.0;
    }
    let mut flags = PaintFlags::stroke(color, rect.height * css);
    if let Some((intervals, cap)) = dashes(style, thickness, end - start) {
        flags.dash = Dash::new(&intervals.map(|i| i * css), 0.0);
        flags.cap = cap;
    }
    let bounds = RectF::new(start, middle - thickness / 2.0, end - start, thickness);
    let op = PaintOp::DrawLine {
        p0: PointF::new(x0 * css, y * css),
        p1: PointF::new(x1 * css, y * css),
        flags,
    };
    (op, scale_rect(bounds, css))
}

/// Blink's `DashEffectFromStrokeStyle` for a line `length` long: dashes
/// two widths long a width apart (three and two below 3), the gap fitted
/// to the length; dots of 3 and less as dashes as long as they are wide,
/// wider ones round, about two widths apart. `None` for a solid line.
fn dashes(style: TextDecorationStyle, width: f32, length: f32) -> Option<([f32; 2], LineCap)> {
    let dashed = style == TextDecorationStyle::Dashed || width <= 3.0;
    if dashed {
        let (mut dash, mut gap) = (width, width);
        if style == TextDecorationStyle::Dashed {
            let (dash_ratio, gap_ratio) = if width >= 3.0 { (2.0, 1.0) } else { (3.0, 2.0) };
            dash *= dash_ratio;
            gap *= gap_ratio;
        }
        if length <= 2.0 * dash {
            return None;
        }
        let two = 2.0 * dash + gap;
        if length <= two {
            let scale = length / two;
            return Some(([dash * scale, gap * scale], LineCap::Butt));
        }
        if style == TextDecorationStyle::Dashed {
            gap = best_dash_gap(length, dash, gap);
        }
        return Some(([dash, gap], LineCap::Butt));
    }
    let per_dot = 2.0 * width;
    if length < per_dot {
        return Some(([0.0, per_dot], LineCap::Round));
    }
    let gap = best_dash_gap(length, width, width);
    // A hair short, so the last dot isn't lost to rounding.
    Some(([0.0, gap + width - 1e-2], LineCap::Round))
}

/// Blink's `SelectBestDashGap` for an open line.
fn best_dash_gap(length: f32, dash: f32, gap: f32) -> f32 {
    let fewer = ((length + gap) / (dash + gap)).floor();
    let more = fewer + 1.0;
    let fewer_gap = (length - fewer * dash) / (fewer - 1.0);
    let more_gap = (length - more * dash) / (more - 1.0);
    if more_gap <= 0.0 || (fewer_gap - gap).abs() < (more_gap - gap).abs() {
        fewer_gap
    } else {
        more_gap
    }
}

/// A wavy line: cubic curves along the line moved by `offset`, a
/// wavelength and height from its thickness, stroked and cut to the
/// line's length (Blink's `MakeWave` and `WavyGeometry`, which tiles the
/// same curve).
fn wave(rect: RectF, offset: f32, color: Color, css: f32) -> (PaintOp, RectF) {
    let thickness = rect.height;
    let t = thickness.max(1.0);
    let wavelength = 1.0 + 2.0 * (2.0 * t + 0.5).round();
    let control = 0.5 + (3.0 * t + 0.5).round();
    let middle = rect.y + offset + 0.5;
    let mut path = Path::new();
    let mut x = rect.x - wavelength;
    path.move_to(PointF::new(x, middle));
    while x < rect.x + rect.width + wavelength {
        let half = x + wavelength / 2.0;
        path.cubic_to(
            PointF::new(half, middle + control),
            PointF::new(half, middle - control),
            PointF::new(x + wavelength, middle),
        );
        x += wavelength;
    }
    // How far the curve reaches either way: 3c·t(1 − t)(1 − 2t) peaks at
    // t = (3 − √3) / 6.
    let reach = 3.0 * control * 0.096_225 + thickness / 2.0;
    let top = (rect.y + offset + 0.5 - reach).floor();
    let bottom = (rect.y + offset + 0.5 + reach).ceil();
    let bounds = scale_rect(RectF::new(rect.x, top, rect.width, bottom - top), css);
    path.transform(&gfx::Transform::scale(css, css));
    let flags = PaintFlags::stroke(color, thickness * css);
    let op = PaintOp::DrawClipped {
        polygon: polygon(bounds),
        op: Box::new(PaintOp::DrawPath { path, flags }),
    };
    (op, bounds)
}

fn polygon(rect: RectF) -> std::sync::Arc<[PointF]> {
    let (r, b) = (rect.x + rect.width, rect.y + rect.height);
    [
        PointF::new(rect.x, rect.y),
        PointF::new(r, rect.y),
        PointF::new(r, b),
        PointF::new(rect.x, b),
    ]
    .into()
}

fn scale_rect(rect: RectF, by: f32) -> RectF {
    RectF::new(rect.x * by, rect.y * by, rect.width * by, rect.height * by)
}

/// `op` as the shadow `shadow` of text in `current_color` casts it: moved
/// by its offset, filled with its color (whatever the text's alpha:
/// Blink's `kShadowIgnoresAlpha`), blurred by half its blur radius; and
/// what it may touch, `bounds` moved and spread by the blur.
pub(crate) fn shadow_of(
    op: &PaintOp,
    bounds: RectF,
    shadow: &BoxShadow,
    current_color: Color,
) -> (PaintOp, RectF) {
    let (dx, dy) = (shadow.x, shadow.y);
    let sigma = shadow.blur / 2.0;
    let color = match shadow.color {
        StyleColor::CurrentColor => current_color,
        StyleColor::Rgba(color) => color,
    };
    let flags = |flags: &PaintFlags| PaintFlags {
        color,
        shader: None,
        blur: sigma,
        ..flags.clone()
    };
    fn shift(op: &PaintOp, dx: f32, dy: f32, flags: &dyn Fn(&PaintFlags) -> PaintFlags) -> PaintOp {
        let moved = |p: PointF| PointF::new(p.x + dx, p.y + dy);
        match op {
            PaintOp::DrawRect { rect, flags: f } => PaintOp::DrawRect {
                rect: RectF::new(rect.x + dx, rect.y + dy, rect.width, rect.height),
                flags: flags(f),
            },
            PaintOp::DrawLine { p0, p1, flags: f } => PaintOp::DrawLine {
                p0: moved(*p0),
                p1: moved(*p1),
                flags: flags(f),
            },
            PaintOp::DrawClipped { polygon, op } => PaintOp::DrawClipped {
                polygon: polygon.iter().map(|&p| moved(p)).collect(),
                op: Box::new(shift(op, dx, dy, flags)),
            },
            PaintOp::DrawPath { path, flags: f } => {
                let mut path = path.clone();
                path.transform(&gfx::Transform::translate(dx, dy));
                PaintOp::DrawPath {
                    path,
                    flags: flags(f),
                }
            }
            PaintOp::DrawGlyphs {
                font,
                size,
                glyphs,
                positions,
                rotations,
                flags: f,
                embolden,
                skew,
                variations,
                bounds,
            } => PaintOp::DrawGlyphs {
                font: font.clone(),
                size: *size,
                glyphs: glyphs.clone(),
                positions: positions.iter().map(|&p| moved(p)).collect(),
                rotations: rotations.clone(),
                flags: flags(f),
                embolden: *embolden,
                skew: *skew,
                variations: variations.clone(),
                bounds: RectF::new(bounds.x + dx, bounds.y + dy, bounds.width, bounds.height),
            },
            op => unreachable!("not a text's: {op:?}"),
        }
    }
    let op = shift(op, dx, dy, &flags);
    let reach = 3.0 * sigma;
    let bounds = RectF::new(
        bounds.x + dx - reach,
        bounds.y + dy - reach,
        bounds.width + 2.0 * reach,
        bounds.height + 2.0 * reach,
    );
    (op, bounds)
}
