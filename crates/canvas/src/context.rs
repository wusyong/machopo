//! What a 2D context's members do to its data, apart from any DOM: the
//! transform's arithmetic (in doubles, as Blink's `AffineTransform`), and
//! recording a draw (Blink's `Canvas2DRecorderContext::Draw`): the
//! recording started with the state stack replayed, each op under its
//! matrix, composited and shadowed as the state says. Each DOM's API
//! finds the context's data, converts the arguments, and records that its
//! canvas has ops to draw.

use base::numerics::{clamp_round, f32_from_f64};
use std::sync::Arc;

use compositor_paint::{
    BlendMode, ColorInterpolation, Dash, GradientShader, GradientShaderKind, GradientStop,
    ImageFilter, LayerFlags, PaintFlags, PaintOp, PaintShader, TileMode,
};
use gfx::{Color, FillRule, Path, PointF, RectF, Transform};

use crate::{
    CanvasColorStop, CanvasData, CanvasDrawingState, CanvasGradientKind, CanvasStyleTarget,
};

/// How many ops a context records before they are drawn without waiting
/// for the next frame (Blink's `FlushIfRecordingLimitExceeded`), so a
/// script drawing without end doesn't grow the recording without end.
pub const MAX_RECORDED_OPS: usize = 10_000;

/// `a` after `b`: maps a point through `b`, then `a` (CSS's `matrix()`
/// order, in doubles).
pub fn multiply(a: [f64; 6], b: [f64; 6]) -> [f64; 6] {
    let [a1, b1, c1, d1, e1, f1] = a;
    let [a2, b2, c2, d2, e2, f2] = b;
    [
        a1 * a2 + c1 * b2,
        b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2,
        b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1,
        b1 * e2 + d1 * f2 + f1,
    ]
}

pub const IDENTITY: [f64; 6] = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

/// Whether `m` can be undone (Blink's `AffineTransform::IsInvertible`).
/// Under one that can't, nothing is drawn and the path is kept in the
/// bitmap's pixels.
pub fn invertible(m: [f64; 6]) -> bool {
    let [a, b, c, d, _, _] = m;
    let det = a * d - b * c;
    det.is_finite() && det != 0.0
}

/// `m`'s inverse, if it has one (Blink's `AffineTransform::Inverse`).
pub fn inverse(m: [f64; 6]) -> Option<[f64; 6]> {
    if !invertible(m) {
        return None;
    }
    let [a, b, c, d, e, f] = m;
    let det = a * d - b * c;
    Some([
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ])
}

/// `m` as a recorded op (and Skia) takes it, in floats.
pub fn to_gfx(m: [f64; 6]) -> Transform {
    Transform(m.map(f32_from_f64))
}

/// The transform becomes `new` (the current one times `change`, applied
/// first); the path, kept in the current transform's space, is mapped
/// into the new one's by `undo` (`change`'s inverse) unless the new one
/// can't be inverted (Blink's `Canvas2DRecorderContext::transform` and
/// the like). Nothing if `new` is the same.
pub fn change_transform(data: &mut CanvasData, change: [f64; 6], undo: impl FnOnce() -> [f64; 6]) {
    let current = data.state().transform;
    let new = multiply(current, change);
    if new == current {
        return;
    }
    data.state_mut().transform = new;
    if invertible(new) {
        data.path.transform(&to_gfx(undo()));
    }
}

/// The current transform times `matrix` (applied first), if every number
/// is finite: Blink and HTML ignore a call with one that isn't.
pub fn transform_by(data: &mut CanvasData, matrix: [f64; 6]) {
    if matrix.iter().all(|n| n.is_finite()) {
        // Only used if the new transform can be inverted, so this one
        // can.
        change_transform(data, matrix, || inverse(matrix).unwrap_or(IDENTITY));
    }
}

/// The transform back to the identity; the path mapped into it, unless
/// the transform couldn't be inverted: then the path is in the bitmap's
/// pixels already (the identity's), as it was before (Blink's
/// `resetTransform`).
pub fn reset_transform_of(data: &mut CanvasData) {
    let current = data.state().transform;
    if current == IDENTITY {
        return;
    }
    data.state_mut().transform = IDENTITY;
    if invertible(current) {
        data.path.transform(&to_gfx(current));
    }
}

/// The names `globalCompositeOperation` takes and their modes: HTML's
/// compositing operators, then the blend modes, whose `normal` is
/// `source-over`'s mode (Blink's `kCanvasCompositeOperatorNames` and
/// `kCanvasBlendModeNames`).
pub const COMPOSITE_OPERATIONS: [(&str, BlendMode); 28] = [
    ("clear", BlendMode::Clear),
    ("copy", BlendMode::Src),
    ("source-over", BlendMode::SrcOver),
    ("source-in", BlendMode::SrcIn),
    ("source-out", BlendMode::SrcOut),
    ("source-atop", BlendMode::SrcATop),
    ("destination-over", BlendMode::DstOver),
    ("destination-in", BlendMode::DstIn),
    ("destination-out", BlendMode::DstOut),
    ("destination-atop", BlendMode::DstATop),
    ("xor", BlendMode::Xor),
    ("lighter", BlendMode::Plus),
    ("normal", BlendMode::SrcOver),
    ("multiply", BlendMode::Multiply),
    ("screen", BlendMode::Screen),
    ("overlay", BlendMode::Overlay),
    ("darken", BlendMode::Darken),
    ("lighten", BlendMode::Lighten),
    ("color-dodge", BlendMode::ColorDodge),
    ("color-burn", BlendMode::ColorBurn),
    ("hard-light", BlendMode::HardLight),
    ("soft-light", BlendMode::SoftLight),
    ("difference", BlendMode::Difference),
    ("exclusion", BlendMode::Exclusion),
    ("hue", BlendMode::Hue),
    ("saturation", BlendMode::Saturation),
    ("color", BlendMode::Color),
    ("luminosity", BlendMode::Luminosity),
];

/// `n` as a float, the largest one past it (Blink's `ClampTo<float>`).
pub fn clamp_f32(n: f64) -> f32 {
    f32_from_f64(n.clamp(f64::from(f32::MIN), f64::from(f32::MAX)))
}

/// The rect from `(x, y)` `w` wide and `h` high, either way (a negative
/// size goes left or up); `None` unless every number is finite, which
/// Blink's `ValidateRectForCanvas` and HTML ignore.
pub fn rect(x: f64, y: f64, w: f64, h: f64) -> Option<RectF> {
    [x, y, w, h].iter().all(|n| n.is_finite()).then(|| {
        let (x0, x1) = (x.min(x + w), x.max(x + w));
        let (y0, y1) = (y.min(y + h), y.max(y + h));
        RectF::new(
            f32_from_f64(x0),
            f32_from_f64(y0),
            f32_from_f64(x1 - x0),
            f32_from_f64(y1 - y0),
        )
    })
}

/// `color` as the current state draws it: its alpha times `globalAlpha`.
pub fn with_global_alpha(color: Color, global_alpha: f64) -> Color {
    let alpha = clamp_round::<u8>(f64::from(color.a) * global_alpha);
    Color { a: alpha, ..color }
}

/// Starts a recording if none is under way: what the state stack holds
/// the canvas to, replayed (Blink's `RestoreMatrixClipStack`): each level
/// above the first a `Save`, and each level's clips, in the bitmap's
/// pixels. `restore()` then pops what its `save()` pushed, as when the
/// recording saw the `save()`.
pub fn begin_recording(data: &mut CanvasData) {
    if !data.ops.is_empty() {
        return;
    }
    let mut ops = Vec::new();
    for (level, state) in data.states.iter().enumerate() {
        if level > 0 {
            ops.push(PaintOp::Save);
        }
        if !state.clips.is_empty() && data.matrix != Some(Transform::IDENTITY) {
            ops.push(PaintOp::SetMatrix {
                transform: Transform::IDENTITY,
            });
            data.matrix = Some(Transform::IDENTITY);
        }
        ops.extend(state.clips.iter().map(|clip| PaintOp::ClipPath {
            path: clip.clone(),
            anti_alias: true,
        }));
    }
    data.ops = ops;
}

/// Records `op` under the current transform (a `SetMatrix` first unless
/// the last one set the same), starting a recording if none is under way.
pub fn record_op(data: &mut CanvasData, op: PaintOp) {
    let matrix = data.state().transform;
    record_op_under(data, matrix, op);
}

/// Records `op` under `matrix` (see `record_op`).
pub fn record_op_under(data: &mut CanvasData, matrix: [f64; 6], op: PaintOp) {
    begin_recording(data);
    let matrix = to_gfx(matrix);
    if data.matrix != Some(matrix) {
        data.ops.push(PaintOp::SetMatrix { transform: matrix });
        data.matrix = Some(matrix);
    }
    data.ops.push(op);
}

/// Records the draw `ops` under `matrix` as `globalCompositeOperation`
/// and the shadow say (Blink's `DrawInternal` and `CompositedDraw`):
/// - `copy`: the clip cleared (to black if `opaque`), then the ops drawn
///   replacing what is there; no shadow, which they would cover.
/// - A shadow with `source-over`, `source-atop` or `destination-out`: one
///   layer with the shadow under the ops, composited by the mode
///   (Blink's `BlendModeSupportsShadowFilter`).
/// - `source-in`, `source-out`, `destination-in`, `destination-atop`,
///   which change the whole clip, not only where the ops draw (Blink's
///   `IsFullCanvasCompositeMode`), or a shadow with another mode: the
///   shadow alone in one layer, the ops in another, each composited by
///   the mode.
/// - Otherwise the ops drawn by the mode.
///
/// A layer is in the bitmap's pixels, so the shadow's offset and blur are
/// too; the ops in it are drawn under `matrix`. An `opaque` canvas's alpha
/// is put back after a mode that could lower it (Blink's
/// `ResetAlphaIfNeeded`).
pub fn composite(data: &mut CanvasData, matrix: [f64; 6], ops: Vec<PaintOp>, opaque: bool) {
    let state = data.state();
    let mode = state.global_composite;
    let shadow = state.draws_shadows().then(|| {
        (
            (
                clamp_f32(state.shadow_offset.0),
                clamp_f32(state.shadow_offset.1),
            ),
            clamp_f32(state.shadow_blur / 2.0),
            state.shadow_color,
        )
    });
    let drop_shadow = |shadow_only| {
        shadow.map(|(offset, sigma, color)| ImageFilter::DropShadow {
            offset,
            sigma,
            color,
            shadow_only,
        })
    };
    let whole = matches!(
        mode,
        BlendMode::SrcIn | BlendMode::SrcOut | BlendMode::DstIn | BlendMode::DstATop
    );
    if mode == BlendMode::Src {
        begin_recording(data);
        data.ops.push(PaintOp::DrawColor {
            color: if opaque {
                Color::BLACK
            } else {
                Color::TRANSPARENT
            },
            blend_mode: BlendMode::Src,
        });
        for op in ops {
            record_op_under(data, matrix, blended(op, mode));
        }
    } else if shadow.is_some()
        && matches!(
            mode,
            BlendMode::SrcOver | BlendMode::SrcATop | BlendMode::DstOut
        )
    {
        record_layer(data, matrix, ops, mode, drop_shadow(false));
    } else if whole || shadow.is_some() {
        if shadow.is_some() {
            record_layer(data, matrix, ops.clone(), mode, drop_shadow(true));
        }
        record_layer(data, matrix, ops, mode, None);
    } else {
        for op in ops {
            record_op_under(data, matrix, blended(op, mode));
        }
    }
    // Blink's `BlendModeDoesntPreserveOpaqueDestinationAlpha`.
    let lowers_alpha = matches!(
        mode,
        BlendMode::Src
            | BlendMode::SrcIn
            | BlendMode::DstIn
            | BlendMode::SrcOut
            | BlendMode::DstOut
            | BlendMode::SrcATop
            | BlendMode::DstATop
            | BlendMode::Xor
    );
    if opaque && lowers_alpha {
        data.ops.push(PaintOp::DrawColor {
            color: Color::BLACK,
            blend_mode: BlendMode::DstOver,
        });
    }
}

/// `op` drawn by `mode`.
pub fn blended(mut op: PaintOp, mode: BlendMode) -> PaintOp {
    match &mut op {
        PaintOp::DrawRect { flags, .. }
        | PaintOp::DrawPath { flags, .. }
        | PaintOp::DrawGlyphs { flags, .. }
        | PaintOp::DrawImage {
            flags: Some(flags), ..
        } => flags.blend_mode = mode,
        _ => {}
    }
    op
}

/// Records `ops` under `matrix` in a layer in the bitmap's pixels,
/// composited by `mode` through `filter`, if any.
pub fn record_layer(
    data: &mut CanvasData,
    matrix: [f64; 6],
    ops: Vec<PaintOp>,
    mode: BlendMode,
    filter: Option<ImageFilter>,
) {
    record_op_under(
        data,
        IDENTITY,
        PaintOp::SaveLayer {
            bounds: None,
            flags: LayerFlags {
                blend_mode: mode,
                image_filter: filter,
                ..LayerFlags::default()
            },
        },
    );
    for op in ops {
        record_op_under(data, matrix, op);
    }
    data.ops.push(PaintOp::Restore);
    // The matrix is the one the layer was saved under.
    data.matrix = None;
}

pub fn has_nan(rect: &RectF) -> bool {
    [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .any(|n| n.is_nan())
}

/// Records the draw `ops` under the current transform times `local`
/// (applied first): composited and with a shadow as the state says
/// (`composite`), or (not `composited`: `clearRect`'s) as they are. The
/// count of ops recorded since the bitmap was last drawn onto, which at
/// `MAX_RECORDED_OPS` has them drawn now; `None` (nothing recorded) for no
/// ops, a canvas whose size can have no pixels, or a transform that can't
/// be inverted, which draw nothing (Blink's `Draw`). An `opaque` canvas
/// (no alpha) gets its alpha back after a mode that lowers it.
pub fn record_ops_into(
    data: &mut CanvasData,
    opaque: bool,
    local: [f64; 6],
    ops: Vec<PaintOp>,
    composited: bool,
) -> Option<usize> {
    if ops.is_empty() {
        return None;
    }
    let transform = data.state().transform;
    if !data.has_valid_size() || !invertible(transform) {
        return None;
    }
    let matrix = multiply(transform, local);
    if composited {
        composite(data, matrix, ops, opaque);
    } else {
        for op in ops {
            record_op_under(data, matrix, op);
        }
    }
    Some(data.ops.len())
}

/// How the state draws with its `target` style: `color` (a gradient's or
/// pattern's black, under its `shader`) times `globalAlpha`; a stroke with
/// the line styles.
pub fn paint_flags(
    state: &CanvasDrawingState,
    target: CanvasStyleTarget,
    color: Color,
    shader: Option<Arc<PaintShader>>,
) -> PaintFlags {
    let color = with_global_alpha(color, state.global_alpha);
    let flags = match target {
        CanvasStyleTarget::Fill => PaintFlags::fill(color),
        CanvasStyleTarget::Stroke => {
            let dash: Vec<f32> = state.line_dash.iter().map(|&s| f32_from_f64(s)).collect();
            PaintFlags {
                cap: state.line_cap,
                join: state.line_join,
                miter_limit: f32_from_f64(state.miter_limit),
                dash: Dash::new(&dash, f32_from_f64(state.line_dash_offset)),
                ..PaintFlags::stroke(color, f32_from_f64(state.line_width))
            }
        }
    };
    PaintFlags { shader, ..flags }
}

// The members' steps on the data, which each DOM's API calls with its
// arguments converted.

/// `save()`: a copy of the current state pushed (`CanvasData::push_state`),
/// a `Save` recorded if a recording is under way (one that starts later
/// replays the stack).
pub fn save(data: &mut CanvasData) {
    data.push_state();
    if !data.ops.is_empty() {
        data.ops.push(PaintOp::Save);
    }
}

/// `restore()`: the state `save()` pushed last popped, its clips with it
/// (a `Restore` recorded); the path mapped into the restored transform's
/// space (Blink's `PopStateStack`). `false` (and nothing) with none
/// pushed.
pub fn restore(data: &mut CanvasData) -> bool {
    let from = data.state().transform;
    if !data.pop_state() {
        return false;
    }
    if invertible(from) && from != IDENTITY {
        data.path.transform(&to_gfx(from));
    }
    let to = data.state().transform;
    if to != IDENTITY
        && let Some(inverse) = inverse(to)
    {
        data.path.transform(&to_gfx(inverse));
    }
    if !data.ops.is_empty() {
        data.ops.push(PaintOp::Restore);
        // The matrix is the one before the matching `Save`.
        data.matrix = None;
    }
    true
}

/// `scale(x, y)`; ignored unless both are finite.
pub fn scale(data: &mut CanvasData, x: f64, y: f64) {
    if x.is_finite() && y.is_finite() {
        change_transform(data, [x, 0.0, 0.0, y, 0.0, 0.0], || {
            [1.0 / x, 0.0, 0.0, 1.0 / y, 0.0, 0.0]
        });
    }
}

/// `rotate(angle)`: clockwise by `angle` radians (y points down); ignored
/// unless finite.
pub fn rotate(data: &mut CanvasData, angle: f64) {
    if !angle.is_finite() {
        return;
    }
    let rotation = |angle: f64| {
        let (sin, cos) = angle.sin_cos();
        [cos, sin, -sin, cos, 0.0, 0.0]
    };
    change_transform(data, rotation(angle), || rotation(-angle));
}

/// `translate(x, y)`; ignored unless both are finite, and (as Blink does)
/// while the transform can't be inverted.
pub fn translate(data: &mut CanvasData, x: f64, y: f64) {
    if x.is_finite() && y.is_finite() && invertible(data.state().transform) {
        change_transform(data, [1.0, 0.0, 0.0, 1.0, x, y], || {
            [1.0, 0.0, 0.0, 1.0, -x, -y]
        });
    }
}

/// `setTransform(a, b, c, d, e, f)`: the transform becomes this matrix
/// (ignored unless every number is finite): a reset, then the matrix
/// applied (as Blink does, which maps the path the same way).
pub fn set_transform(data: &mut CanvasData, matrix: [f64; 6]) {
    if matrix.iter().all(|n| n.is_finite()) {
        reset_transform_of(data);
        transform_by(data, matrix);
    }
}

/// `globalAlpha`'s setter: ignored unless from 0 to 1 (NaN isn't).
pub fn set_global_alpha(data: &mut CanvasData, value: f64) {
    if (0.0..=1.0).contains(&value) {
        data.state_mut().global_alpha = value;
    }
}

/// `globalCompositeOperation`: the mode's first name (`source-over`, not
/// `normal`).
pub fn composite_name(mode: BlendMode) -> &'static str {
    COMPOSITE_OPERATIONS
        .iter()
        .find(|(_, m)| *m == mode)
        .map_or("source-over", |(name, _)| name)
}

/// `globalCompositeOperation`'s setter: ignored unless one of the names,
/// as written.
pub fn set_composite(data: &mut CanvasData, name: &str) {
    if let Some((_, mode)) = COMPOSITE_OPERATIONS.iter().find(|(n, _)| *n == name) {
        data.state_mut().global_composite = *mode;
    }
}

/// `lineWidth`'s and `miterLimit`'s setters: ignored unless finite and
/// more than 0.
pub fn positive(value: f64) -> Option<f64> {
    (value.is_finite() && value > 0.0).then_some(value)
}

/// `setLineDash(segments)`: ignored if any is negative or not finite; an
/// odd number of them is repeated to make an even one (HTML; Blink).
pub fn set_line_dash(data: &mut CanvasData, segments: Vec<f64>) {
    if segments.iter().any(|s| !s.is_finite() || *s < 0.0) {
        return;
    }
    let dash = if segments.len() % 2 == 1 {
        segments.repeat(2)
    } else {
        segments
    };
    data.state_mut().line_dash = dash;
}

/// The names of `CanvasLineCap` and `CanvasLineJoin`, and their values.
pub const LINE_CAPS: [(&str, compositor_paint::LineCap); 3] = [
    ("butt", compositor_paint::LineCap::Butt),
    ("round", compositor_paint::LineCap::Round),
    ("square", compositor_paint::LineCap::Square),
];
pub const LINE_JOINS: [(&str, compositor_paint::LineJoin); 3] = [
    ("round", compositor_paint::LineJoin::Round),
    ("bevel", compositor_paint::LineJoin::Bevel),
    ("miter", compositor_paint::LineJoin::Miter),
];

/// `fillRect(x, y, w, h)`'s op, filled by `flags`: none for numbers that
/// aren't finite or an empty rect.
pub fn fill_rect_op(x: f64, y: f64, w: f64, h: f64, flags: PaintFlags) -> Option<PaintOp> {
    let rect = rect(x, y, w, h)?;
    (!rect.is_empty()).then_some(PaintOp::DrawRect { rect, flags })
}

/// `strokeRect(x, y, w, h)`'s op, stroked by `flags`: the rect's outline;
/// with no width or no height a line there and back, closed (no caps:
/// Blink's `StrokeRectOnCanvas`); with neither nothing (HTML; Blink skips
/// the zero-length segments).
pub fn stroke_rect_op(x: f64, y: f64, w: f64, h: f64, flags: PaintFlags) -> Option<PaintOp> {
    let rect = rect(x, y, w, h)?;
    match (rect.width == 0.0, rect.height == 0.0) {
        (true, true) => None,
        (true, false) | (false, true) => {
            let mut path = gfx::Path::new();
            path.move_to(gfx::PointF::new(rect.x, rect.y));
            path.line_to(gfx::PointF::new(rect.right(), rect.bottom()));
            path.close();
            Some(PaintOp::DrawPath { path, flags })
        }
        (false, false) => Some(PaintOp::DrawRect { rect, flags }),
    }
}

/// `clearRect(x, y, w, h)`'s op: the rect made transparent black, or
/// opaque black on a canvas without `alpha` (Blink's `GetClearFlags`),
/// whatever `globalAlpha` is; recorded as it is (not composited).
pub fn clear_rect_op(x: f64, y: f64, w: f64, h: f64, alpha: bool) -> Option<PaintOp> {
    let rect = rect(x, y, w, h)?;
    if rect.is_empty() {
        return None;
    }
    Some(if alpha {
        PaintOp::ClearRect { rect }
    } else {
        PaintOp::DrawRect {
            rect,
            flags: PaintFlags::fill(Color::BLACK),
        }
    })
}

/// `fill(fillRule)`'s op of `path`, filled by `flags` by the rule (open
/// subpaths as if closed): none for an empty path (Blink's
/// `DrawPathInternal`).
pub fn fill_path_op(path: gfx::Path, rule: gfx::FillRule, flags: PaintFlags) -> Option<PaintOp> {
    path.bounds().filter(|b| !has_nan(b))?;
    Some(PaintOp::DrawPath {
        path: path.with_fill_rule(rule),
        flags,
    })
}

/// `stroke()`'s op of `path`, stroked by `flags`: none for a path whose
/// points are all one (Blink's `DrawPathInternal`).
pub fn stroke_path_op(path: gfx::Path, flags: PaintFlags) -> Option<PaintOp> {
    let bounds = path.bounds().filter(|b| !has_nan(b))?;
    (bounds.width != 0.0 || bounds.height != 0.0).then_some(PaintOp::DrawPath { path, flags })
}

/// `clip(fillRule)` to `path`: what is drawn from now on is cut to it,
/// filled by the rule, until the `restore()` of this level: kept in the
/// bitmap's pixels with the level (for the next recordings), and recorded
/// if a recording is under way. An empty path clips everything out.
/// Nothing under a transform that can't be inverted (Blink's
/// `ClipInternal`).
pub fn clip(data: &mut CanvasData, path: gfx::Path, rule: gfx::FillRule) {
    let transform = data.state().transform;
    if !invertible(transform) {
        return;
    }
    let path = path.with_fill_rule(rule);
    let mut in_pixels = path.clone();
    in_pixels.transform(&to_gfx(transform));
    data.state_mut().clips.push(in_pixels);
    if !data.ops.is_empty() {
        record_op(
            data,
            PaintOp::ClipPath {
                path,
                anti_alias: true,
            },
        );
    }
}

// CanvasShadowStyles

/// `shadowOffsetX`'s and `shadowOffsetY`'s setters: ignored unless finite.
pub fn set_shadow_offset(data: &mut CanvasData, x: Option<f64>, y: Option<f64>) {
    let offset = &mut data.state_mut().shadow_offset;
    if let Some(x) = x.filter(|x| x.is_finite()) {
        offset.0 = x;
    }
    if let Some(y) = y.filter(|y| y.is_finite()) {
        offset.1 = y;
    }
}

/// `shadowBlur`'s setter: ignored unless finite and not negative.
pub fn set_shadow_blur(data: &mut CanvasData, value: f64) {
    if value.is_finite() && value >= 0.0 {
        data.state_mut().shadow_blur = value;
    }
}

// CanvasFillStrokeStyles' gradients

/// `createLinearGradient(x0, y0, x1, y1)`'s: from `(x0, y0)` to `(x1, y1)`,
/// in the space it is drawn in.
pub fn linear_gradient(x0: f64, y0: f64, x1: f64, y1: f64) -> CanvasGradientKind {
    CanvasGradientKind::Linear {
        x0: clamp_f32(x0),
        y0: clamp_f32(y0),
        x1: clamp_f32(x1),
        y1: clamp_f32(y1),
    }
}

/// `createRadialGradient(x0, y0, r0, x1, y1, r1)`'s: from one circle to the
/// other; the name of a negative radius (an `IndexSizeError`).
pub fn radial_gradient(
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
) -> Result<CanvasGradientKind, &'static str> {
    for (name, radius) in [("r0", r0), ("r1", r1)] {
        if radius < 0.0 {
            return Err(name);
        }
    }
    Ok(CanvasGradientKind::Radial {
        x0: clamp_f32(x0),
        y0: clamp_f32(y0),
        r0: clamp_f32(r0),
        x1: clamp_f32(x1),
        y1: clamp_f32(y1),
        r1: clamp_f32(r1),
    })
}

/// `createConicGradient(startAngle, x, y)`'s: round `(x, y)`, clockwise
/// from `startAngle` radians off the x axis.
pub fn conic_gradient(start_angle: f64, x: f64, y: f64) -> CanvasGradientKind {
    CanvasGradientKind::Conic {
        start_angle: clamp_f32(start_angle),
        x: clamp_f32(x),
        y: clamp_f32(y),
    }
}

/// A gradient as a shader (Blink's `Gradient::CreateShader`): its stops
/// in order of offset (those at one offset as added), the first copied to
/// 0 and the last to 1 if they aren't there, transparent black for none;
/// nothing at all for a line of no length, or two circles that are one;
/// the ends extended, the colors mixed unpremultiplied.
pub fn gradient_shader(kind: CanvasGradientKind, stops: &[CanvasColorStop]) -> PaintShader {
    let point = PointF::new;
    let (kind, transform) = match kind {
        CanvasGradientKind::Linear { x0, y0, x1, y1 } => {
            if (x0, y0) == (x1, y1) {
                return PaintShader::Empty;
            }
            let kind = GradientShaderKind::Linear {
                start: point(x0, y0),
                end: point(x1, y1),
            };
            (kind, Transform::IDENTITY)
        }
        CanvasGradientKind::Radial {
            x0,
            y0,
            r0,
            x1,
            y1,
            r1,
        } => {
            if (x0, y0, r0) == (x1, y1, r1) {
                return PaintShader::Empty;
            }
            let kind = GradientShaderKind::TwoPointConical {
                start: point(x0, y0),
                start_radius: r0,
                end: point(x1, y1),
                end_radius: r1,
            };
            (kind, Transform::IDENTITY)
        }
        // Turned about the center by the start angle (Blink's
        // `ConicGradient` turns Skia's sweep, which starts on the x axis).
        CanvasGradientKind::Conic { start_angle, x, y } => {
            let kind = GradientShaderKind::Sweep {
                center: point(x, y),
                start_angle: 0.0,
                end_angle: 360.0,
            };
            let (sin, cos) = start_angle.sin_cos();
            let turn = Transform([
                cos,
                sin,
                -sin,
                cos,
                x - x * cos + y * sin,
                y - x * sin - y * cos,
            ]);
            (kind, turn)
        }
    };
    let mut stops = stops.to_vec();
    stops.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let stop = |offset: f64, color| GradientStop {
        offset: f32_from_f64(offset),
        color,
    };
    let mut shader_stops = Vec::with_capacity(stops.len().saturating_add(2));
    match stops.first() {
        None => shader_stops.push(stop(0.0, Color::TRANSPARENT)),
        Some(first) if first.offset > 0.0 => shader_stops.push(stop(0.0, first.color)),
        Some(_) => {}
    }
    shader_stops.extend(stops.iter().map(|s| stop(s.offset, s.color)));
    let last = *shader_stops.last().expect("a stop at least");
    if last.offset < 1.0 {
        shader_stops.push(GradientStop {
            offset: 1.0,
            ..last
        });
    }
    PaintShader::Gradient(GradientShader {
        kind,
        stops: shader_stops,
        tile_mode: TileMode::Clamp,
        transform,
        interpolation: ColorInterpolation::Unpremultiplied,
    })
}

// CanvasTransform's `setTransform(transform)`

/// Geometry's "validate and fixup (2D)" of a `DOMMatrix2DInit`, its values
/// as CSS's `matrix(a, b, c, d, e, f)`: each pair (`a` and `m11`, `b` and
/// `m12`, `c` and `m21`, `d` and `m22`, `e` and `m41`, `f` and `m42`) its
/// `m11`-like name's value if given, else its `a`-like name's, else the
/// identity's; the names of a pair both given and not the same number
/// (`SameValueZero`: `NaN` is itself, `-0` is `0`), a `TypeError`.
pub fn matrix_2d(
    pairs: [(Option<f64>, Option<f64>); 6],
) -> Result<[f64; 6], (&'static str, &'static str)> {
    const NAMES: [(&str, &str); 6] = [
        ("a", "m11"),
        ("b", "m12"),
        ("c", "m21"),
        ("d", "m22"),
        ("e", "m41"),
        ("f", "m42"),
    ];
    let mut matrix = IDENTITY;
    for ((value, (short, long)), names) in matrix.iter_mut().zip(pairs).zip(NAMES) {
        if let (Some(x), Some(y)) = (short, long)
            && !(x == y || (x.is_nan() && y.is_nan()))
        {
            return Err(names);
        }
        *value = long.or(short).unwrap_or(*value);
    }
    Ok(matrix)
}

// Hit testing.

/// `(x, y)` in the space the current transform maps from, where a path
/// is drawn from (Blink maps it by the inverse); `None` if either isn't
/// finite, or the transform can't be inverted, which hit nothing.
fn hit_point(data: &CanvasData, x: f64, y: f64) -> Option<PointF> {
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    let [a, b, c, d, e, f] = inverse(data.state().transform)?;
    let (x, y) = (f64::from(clamp_f32(x)), f64::from(clamp_f32(y)));
    Some(PointF::new(
        clamp_f32(a * x + c * y + e),
        clamp_f32(b * x + d * y + f),
    ))
}

/// `isPointInPath`: whether `(x, y)` is in `path`'s fill by `rule`, the
/// point mapped by the current transform's inverse (Blink's
/// `IsPointInPathInternal`).
pub fn point_in_path(data: &CanvasData, path: &Path, x: f64, y: f64, rule: FillRule) -> bool {
    hit_point(data, x, y).is_some_and(|at| path.contains(at, rule))
}

/// `isPointInStroke`: whether `(x, y)` is in the area `path`'s stroke
/// covers with the current line styles (Blink's
/// `IsPointInStrokeInternal`): its outline as fine as a pixel's twentieth
/// wherever the transform scales it to.
pub fn point_in_stroke(data: &CanvasData, path: &Path, x: f64, y: f64) -> bool {
    let Some(at) = hit_point(data, x, y) else {
        return false;
    };
    let state = data.state();
    let [a, b, c, d, ..] = state.transform;
    let scale = (a * a + b * b).max(c * c + d * d).sqrt();
    let intervals: Vec<f32> = state.line_dash.iter().map(|&s| f32_from_f64(s)).collect();
    let dash = Dash::new(&intervals, f32_from_f64(state.line_dash_offset));
    let style = gfx::StrokeStyle {
        width: f32_from_f64(state.line_width),
        cap: state.line_cap,
        join: state.line_join,
        miter_limit: f32_from_f64(state.miter_limit),
        dash: dash.as_ref().map(|dash| (dash.intervals(), dash.phase())),
    };
    path.stroke_contains(at, &style, 0.05 / scale)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point is mapped by the transform's inverse; one that isn't
    /// finite, or a transform that can't be inverted, hits nothing; a
    /// stroke is as wide as the line.
    #[test]
    fn points_hit_through_the_transform() {
        let mut data = CanvasData::new((10, 10));
        let mut path = Path::new();
        path.add_rect(PointF::new(0.0, 0.0), PointF::new(2.0, 2.0));
        assert!(point_in_path(&data, &path, 1.0, 1.0, FillRule::NonZero));
        data.state_mut().transform = [1.0, 0.0, 0.0, 1.0, 5.0, 0.0];
        assert!(!point_in_path(&data, &path, 1.0, 1.0, FillRule::NonZero));
        assert!(point_in_path(&data, &path, 6.0, 1.0, FillRule::NonZero));
        assert!(!point_in_path(
            &data,
            &path,
            f64::NAN,
            1.0,
            FillRule::NonZero
        ));
        assert!(point_in_stroke(&data, &path, 5.0, 1.0));
        assert!(!point_in_stroke(&data, &path, 6.0, 1.0));
        data.state_mut().line_width = 4.0;
        assert!(point_in_stroke(&data, &path, 6.0, 1.0));
        data.state_mut().transform = [0.0; 6];
        assert!(!point_in_path(&data, &path, 0.0, 0.0, FillRule::NonZero));
        assert!(!point_in_stroke(&data, &path, 0.0, 0.0));
    }

    #[test]
    fn a_rect_goes_either_way_and_must_be_finite() {
        assert_eq!(
            rect(10.0, 10.0, -4.0, 2.0),
            Some(RectF::new(6.0, 10.0, 4.0, 2.0))
        );
        assert_eq!(rect(0.0, f64::NAN, 1.0, 1.0), None);
        assert_eq!(rect(0.0, 0.0, f64::INFINITY, 1.0), None);
    }

    #[test]
    fn matrices_multiply_in_css_order() {
        // Translate, then scale: a point is scaled first.
        let m = multiply(
            [1.0, 0.0, 0.0, 1.0, 10.0, 0.0],
            [2.0, 0.0, 0.0, 2.0, 0.0, 0.0],
        );
        assert_eq!(m, [2.0, 0.0, 0.0, 2.0, 10.0, 0.0]);
        let m = multiply(
            [2.0, 0.0, 0.0, 2.0, 0.0, 0.0],
            [1.0, 0.0, 0.0, 1.0, 10.0, 0.0],
        );
        assert_eq!(m, [2.0, 0.0, 0.0, 2.0, 20.0, 0.0]);
    }
}
