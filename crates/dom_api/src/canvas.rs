//! `<canvas>`'s `HTMLCanvasElement` and its `CanvasRenderingContext2D`
//! (HTML's canvas), over `dom`'s context and the `canvas` crate's
//! data and steps (`canvas::context`, `canvas::path`): each member finds the context's data, converts its arguments,
//! and records that its canvas has ops to draw (`record_canvas_draw`); the
//! renderer draws them (`FrameLayout::flush_canvas`, and before a frame).

use canvas::context::{self, MAX_RECORDED_OPS};
use canvas::export::{self, ExportBitmap};
use canvas::image::{self, SourceImage};
use canvas::path::{CanvasPath, PathBuilder, PathError};
use canvas::text;
use canvas::{
    CanvasColorType, CanvasData, CanvasDrawingState, CanvasFlush, CanvasSettings, CanvasStyle,
    CanvasStyleTarget, PredefinedColorSpace,
};
use std::sync::Arc;

use canvas::CanvasColorStop;
use compositor_paint::{PaintFlags, PaintOp};
use dom::{
    CanvasGradient, CanvasPattern, CanvasRenderingContext2d, CanvasStyleObject, DomString,
    ImageData, ImageForCanvas, NodeRef, Path2d, names,
};
use elise::Gc;
use gfx::{Color, FillRule};

use crate::typed::{
    Callback, CanvasStyleParam, CanvasStyleRet, ImageDataWithPixels, OpFront, Path2dOrString,
    ScriptHost, Uint8ClampedArg,
};
use crate::{ApiCx, ApiError, DomException, DomStr, ExceptionName};

// --- HTMLCanvasElement ------------------------------------------------------

/// HTML's `getContext(contextId, options)`: for `"2d"`, the canvas's 2D
/// context, made with `options` the first time and the same one after
/// (later options are ignored); `null` for any other id (machopo has no
/// other context: WebGL, WebGPU, `bitmaprenderer`). The options are
/// converted whatever the id.
pub(crate) fn get_context<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    context_id: &str,
    options: &crate::interfaces::CanvasRenderingContext2DSettings,
) -> Result<Option<Gc<'gc, CanvasRenderingContext2d>>, ApiError> {
    let settings = settings(options)?;
    if context_id != "2d" {
        return Ok(None);
    }
    if let Some(context) = cx.canvas_context(this) {
        return Ok(Some(context));
    }
    Ok(Some(cx.create_canvas_context(this, settings)))
}

/// The options as a context keeps them: `colorSpace` and `colorType` one
/// of their enumerations' values (WebIDL: another is a `TypeError`).
fn settings(
    options: &crate::interfaces::CanvasRenderingContext2DSettings,
) -> Result<CanvasSettings, ApiError> {
    let invalid = |value: &str, name: &str| {
        ApiError::type_error(format!(
            "Failed to execute 'getContext' on 'HTMLCanvasElement': The provided value '{value}' is not a valid enum value of type {name}."
        ))
    };
    let color_space = match options.colorSpace.as_str() {
        "srgb" => PredefinedColorSpace::Srgb,
        "display-p3" => PredefinedColorSpace::DisplayP3,
        other => return Err(invalid(other, "PredefinedColorSpace")),
    };
    let color_type = match options.colorType.as_str() {
        "unorm8" => CanvasColorType::Unorm8,
        "float16" => CanvasColorType::Float16,
        other => return Err(invalid(other, "CanvasColorType")),
    };
    Ok(CanvasSettings {
        alpha: options.alpha,
        desynchronized: options.desynchronized,
        color_space,
        color_type,
        will_read_frequently: options.willReadFrequently,
    })
}

/// HTML's "serialize as a file" of `canvas`'s bitmap as it is now
/// (`canvas::export::export_bitmap`: what was recorded drawn first, or sent
/// to the thread that has the bitmap, its snapshot asked for and not waited
/// for): none if it has no pixels (a width or height of 0, or past what
/// can have them). A `SecurityError` for one that isn't origin-clean.
fn bitmap_to_export<'gc>(
    cx: &ApiCx<'_, 'gc>,
    canvas: NodeRef<'gc>,
) -> Result<Option<ExportBitmap>, ApiError> {
    let context = cx.canvas_context(canvas);
    if let Some(context) = context
        && !with_data(cx, context, |data| data.origin_clean)
    {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::Security,
            message: export::TAINTED_MESSAGE.into(),
        }));
    }
    let size = cx.canvas_size(canvas);
    if !canvas::is_valid_bitmap_size(size) {
        return Ok(None);
    }
    Ok(match context {
        Some(context) => {
            flush(cx, context, CanvasFlush::Read);
            let background = canvas::background(cx.canvas_context_settings(context));
            with_data(cx, context, |data| {
                export::export_bitmap(Some(data), background, size)
            })
        }
        None => export::export_bitmap(None, [0; 4], size),
    })
}

/// `toDataURL(type, quality)`: the bitmap (`bitmap_to_export`) as a
/// `data:` URL of the file `type` and `quality` say
/// (`canvas::export::image_encoding`), encoded now, on this thread, as
/// HTML says (a bitmap another thread has waited for: Blink's
/// `GetImage` of an accelerated canvas); `data:,` for one with no pixels,
/// or that can't be encoded (none came, no encoders).
pub(crate) fn to_data_url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    mime_type: Option<&str>,
    quality: Option<f64>,
) -> Result<DomStr<'gc>, ApiError> {
    let encoding = export::image_encoding(mime_type.unwrap_or("image/png"), quality);
    let file = bitmap_to_export(cx, this)?
        .and_then(|bitmap| cx.lend_frame_layout(|layout| layout.encode_image(&bitmap, encoding)))
        .flatten();
    let url = match file {
        Some(bytes) => {
            use base64::Engine;
            let base64 = base64::engine::general_purpose::STANDARD.encode(bytes);
            format!("data:{};base64,{base64}", encoding.mime_type())
        }
        None => "data:,".to_owned(),
    };
    Ok(DomStr::Owned(DomString::from(url)))
}

/// `toBlob(callback, type, quality)`: the bitmap as it is now
/// (`bitmap_to_export`), to encode as `type` and `quality` say, off this
/// thread (`Cx::request_canvas_blob`: the host's encoder, which waits for
/// a snapshot another thread fills in, where Blink encodes in idle tasks or
/// on a worker thread, `CanvasAsyncBlobCreator`); `callback` called in an
/// element task with a `Blob` of the file, or null for no pixels or no
/// file. Nothing for a canvas whose document has no window, as Blink
/// without an execution context.
pub(crate) fn to_blob<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    callback: Callback<O::Callback>,
    mime_type: Option<&str>,
    quality: Option<f64>,
) -> Result<(), ApiError> {
    let bitmap = bitmap_to_export(cx, this)?;
    if cx.window_of(cx.document_of(this)).is_none() {
        return Ok(());
    }
    let encoding = export::image_encoding(mime_type.unwrap_or("image/png"), quality);
    let edge = cx.request_canvas_blob(this, bitmap, encoding);
    op.store_callback(cx, edge, callback);
    Ok(())
}

/// A `toBlob` call's element task, of the call `id` names: its callback
/// called with its blob, or null (`ScriptHost::call_blob_callback`).
pub(crate) fn run_blob_task<H: ScriptHost>(host: &mut H, id: u64) {
    let callback = host.mutate(|cx| {
        cx.take_canvas_blob(id)
            .map(|callback| elise::Persistent::new(cx.gc, callback))
    });
    if let Some(callback) = callback {
        host.call_blob_callback(&callback);
    }
}

// --- CanvasRenderingContext2D -----------------------------------------------

/// The context's `canvas`: its element.
pub(crate) fn canvas<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.canvas_context_canvas(this))
}

fn with_data<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    f: impl FnOnce(&mut CanvasData) -> R,
) -> R {
    cx.with_canvas_data(this, f)
}

/// Records the draw `ops` (`context::record_ops_into`): as one draw,
/// composited and shadowed as the state says unless not `composited`
/// (`clearRect`'s); that the canvas has ops to draw; drawn now once there
/// are `MAX_RECORDED_OPS` (Blink's `FlushIfRecordingLimitExceeded`).
fn record_ops<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    ops: Vec<PaintOp>,
    composited: bool,
) {
    record_ops_with(cx, this, context::IDENTITY, ops, composited);
}

/// `record_ops` of composited `ops` under the `local` transform (a
/// squeezed text's scale); nothing for none (text with no glyphs).
fn record_ops_under<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    local: [f64; 6],
    ops: Vec<PaintOp>,
) {
    if !ops.is_empty() {
        record_ops_with(cx, this, local, ops, true);
    }
}

fn record_ops_with<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    local: [f64; 6],
    ops: Vec<PaintOp>,
    composited: bool,
) {
    let opaque = !cx.canvas_context_settings(this).alpha;
    let recorded = with_data(cx, this, |data| {
        context::record_ops_into(data, opaque, local, ops, composited)
    });
    let Some(count) = recorded else {
        return;
    };
    cx.record_canvas_draw(cx.canvas_context_canvas(this));
    if count >= MAX_RECORDED_OPS {
        flush(cx, this, CanvasFlush::Draw);
    }
}

/// Has the frame's renderer draw what the context recorded onto its
/// bitmap now, or send it to the thread that has the bitmap, as `flush`
/// says (`FrameLayout::flush_canvas`): any canvas's, shown or not. Nothing
/// without a frame.
fn flush<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, CanvasRenderingContext2d>, flush: CanvasFlush) {
    let id = cx.node_id(cx.canvas_context_canvas(this));
    cx.lend_frame_layout(|layout| layout.flush_canvas(cx, id, flush));
}

/// `f` with the bitmap's pixels as script reads them, what was recorded
/// drawn first: the context's own, or for a bitmap another thread has, its
/// snapshot's, which the main thread waits for (Blink's `ReadPixels` of an
/// accelerated canvas). `None` for no pixels yet (all the background), or
/// ones that never came.
fn with_read_pixels<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    f: impl FnOnce(&CanvasData, Option<&[u8]>) -> R,
) -> R {
    flush(cx, this, CanvasFlush::Read);
    // Waited for with the data put back: the wait borrows nothing.
    let snapshot = with_data(cx, this, |data| {
        data.resource.as_ref().and(data.snapshot.clone())
    })
    .filter(gfx::ImageData::wait_ready);
    with_data(cx, this, |data| match (&data.resource, &snapshot) {
        (Some(_), Some(snapshot)) => f(data, Some(snapshot.as_bytes())),
        (Some(_), None) => f(data, None),
        (None, _) => f(data, data.pixels.as_deref()),
    })
}

fn record<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, CanvasRenderingContext2d>, op: Option<PaintOp>) {
    if let Some(op) = op {
        record_ops(cx, this, vec![op], true);
    }
}

// CanvasState

pub(crate) fn save<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<(), ApiError> {
    cx.save_canvas_state(this);
    Ok(())
}

pub(crate) fn restore<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<(), ApiError> {
    cx.restore_canvas_state(this);
    Ok(())
}

/// `reset()`: HTML's "reset the rendering context to its default state":
/// the bitmap cleared, the state stack emptied and the state the default.
pub(crate) fn reset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<(), ApiError> {
    let size = with_data(cx, this, |data| data.size);
    cx.reset_canvas_context(this, size);
    cx.record_canvas_draw(cx.canvas_context_canvas(this));
    Ok(())
}

/// `isContextLost()`: machopo's contexts aren't lost (their bitmaps are in
/// memory, not on a GPU that can go).
pub(crate) fn is_context_lost<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<bool, ApiError> {
    Ok(false)
}

// CanvasTransform

pub(crate) fn scale<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::scale(data, x, y));
    Ok(())
}

pub(crate) fn rotate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    angle: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::rotate(data, angle));
    Ok(())
}

pub(crate) fn translate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::translate(data, x, y));
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "WebIDL's `transform(a, b, c, d, e, f)`"
)]
pub(crate) fn transform<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| {
        context::transform_by(data, [a, b, c, d, e, f])
    });
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "WebIDL's `setTransform(a, b, c, d, e, f)`"
)]
pub(crate) fn set_transform<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    a: f64,
    b: f64,
    c: f64,
    d: f64,
    e: f64,
    f: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| {
        context::set_transform(data, [a, b, c, d, e, f])
    });
    Ok(())
}

pub(crate) fn reset_transform<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<(), ApiError> {
    with_data(cx, this, context::reset_transform_of);
    Ok(())
}

// CanvasCompositing

pub(crate) fn global_alpha<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().global_alpha))
}

pub(crate) fn set_global_alpha<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::set_global_alpha(data, value));
    Ok(())
}

pub(crate) fn global_composite_operation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let mode = with_data(cx, this, |data| data.state().global_composite);
    Ok(DomStr::Owned(DomString::from(context::composite_name(
        mode,
    ))))
}

pub(crate) fn set_global_composite_operation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::set_composite(data, value));
    Ok(())
}

// CanvasFillStrokeStyles

fn style_of(state: &CanvasDrawingState, target: CanvasStyleTarget) -> CanvasStyle {
    match target {
        CanvasStyleTarget::Fill => state.fill_style,
        CanvasStyleTarget::Stroke => state.stroke_style,
    }
}

/// A style as script reads it back: a color serialized, or the gradient or
/// pattern.
fn style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    target: CanvasStyleTarget,
) -> Result<CanvasStyleRet<'gc>, ApiError> {
    let style = with_data(cx, this, |data| style_of(data.state(), target));
    Ok(match style {
        CanvasStyle::Color(color) => CanvasStyleRet::String(DomStr::Owned(DomString::from(
            css::canvas::serialize_canvas_color(color),
        ))),
        CanvasStyle::Gradient | CanvasStyle::Pattern => CanvasStyleRet::Object(
            cx.canvas_style_object(this, target)
                .expect("a gradient or pattern style's object, in its slot"),
        ),
    })
}

/// The system colors a color names, the frame's (the defaults without
/// one).
fn system_colors<'gc>(cx: &ApiCx<'_, 'gc>) -> gfx::SystemColors {
    cx.lend_frame_layout(|layout| layout.system_colors())
        .unwrap_or(gfx::SystemColors::DEFAULT)
}

/// `value` as a CSS color, parsed as it is set; `currentcolor` is the
/// canvas's inline style's `color` then, as in Blink (`GetCurrentColor`):
/// black if it has none, or isn't connected. `None` for what isn't a color.
fn parse_color<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Option<Color> {
    let system_colors = system_colors(cx);
    match css::canvas::parse_canvas_color(value, &system_colors)? {
        css::canvas::CanvasColor::Color(color) => Some(color),
        css::canvas::CanvasColor::CurrentColor => {
            let canvas = cx.canvas_context_canvas(this);
            let inline = cx
                .is_connected(canvas)
                .then(|| cx.attribute_value_by_id(canvas, names::local::STYLE))
                .flatten()
                .map(|value| value.borrow().to_str().into_owned());
            Some(css::canvas::canvas_current_color(
                inline.as_deref(),
                &system_colors,
            ))
        }
    }
}

/// Setting a style: a gradient or pattern as it is, a string if it is a
/// color (else ignored). A pattern of another origin's pixels makes the
/// canvas not origin-clean from now on (Blink taints as the style is set).
fn set_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    target: CanvasStyleTarget,
    value: CanvasStyleParam<'_, 'gc>,
) {
    match value {
        CanvasStyleParam::String(value) => {
            if let Some(color) = parse_color(cx, this, value) {
                cx.set_canvas_style(this, target, CanvasStyle::Color(color), None);
            }
        }
        CanvasStyleParam::Object(object) => {
            let style = match object {
                CanvasStyleObject::Gradient(_) => CanvasStyle::Gradient,
                CanvasStyleObject::Pattern(pattern) => {
                    if !cx.with_canvas_pattern(pattern, |pattern| pattern.origin_clean) {
                        with_data(cx, this, |data| data.origin_clean = false);
                    }
                    CanvasStyle::Pattern
                }
            };
            cx.set_canvas_style(this, target, style, Some(object));
        }
    }
}

pub(crate) fn fill_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<CanvasStyleRet<'gc>, ApiError> {
    style(cx, this, CanvasStyleTarget::Fill)
}

pub(crate) fn set_fill_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: CanvasStyleParam<'_, 'gc>,
) -> Result<(), ApiError> {
    set_style(cx, this, CanvasStyleTarget::Fill, value);
    Ok(())
}

pub(crate) fn stroke_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<CanvasStyleRet<'gc>, ApiError> {
    style(cx, this, CanvasStyleTarget::Stroke)
}

pub(crate) fn set_stroke_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: CanvasStyleParam<'_, 'gc>,
) -> Result<(), ApiError> {
    set_style(cx, this, CanvasStyleTarget::Stroke, value);
    Ok(())
}

/// The flags that paint with the state's `target` style (Blink's
/// `CanvasStyle::ApplyToFlags`): its color with `globalAlpha`, or its
/// gradient or pattern as a shader (a pattern sampled as the image
/// smoothing says), `globalAlpha` its alpha; a stroke's with the line
/// styles too (`canvas::context::paint_flags`).
fn flags<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    target: CanvasStyleTarget,
) -> PaintFlags {
    let style = with_data(cx, this, |data| style_of(data.state(), target));
    let (color, shader) = match style {
        CanvasStyle::Color(color) => (color, None),
        CanvasStyle::Gradient | CanvasStyle::Pattern => {
            let shader = match cx
                .canvas_style_object(this, target)
                .expect("a gradient or pattern style's object, in its slot")
            {
                CanvasStyleObject::Gradient(gradient) => cx
                    .with_canvas_gradient(gradient, |gradient| {
                        context::gradient_shader(gradient.kind, &gradient.stops)
                    }),
                CanvasStyleObject::Pattern(pattern) => {
                    let filter = with_data(cx, this, |data| data.state().image_filter());
                    cx.with_canvas_pattern(pattern, |pattern| {
                        image::pattern_shader(pattern, filter)
                    })
                }
            };
            (Color::BLACK, Some(Arc::new(shader)))
        }
    };
    with_data(cx, this, |data| {
        context::paint_flags(data.state(), target, color, shader)
    })
}

// CanvasShadowStyles

pub(crate) fn shadow_offset_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().shadow_offset.0))
}

/// Ignored unless finite.
pub(crate) fn set_shadow_offset_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| {
        context::set_shadow_offset(data, Some(value), None)
    });
    Ok(())
}

pub(crate) fn shadow_offset_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().shadow_offset.1))
}

/// Ignored unless finite.
pub(crate) fn set_shadow_offset_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| {
        context::set_shadow_offset(data, None, Some(value))
    });
    Ok(())
}

pub(crate) fn shadow_blur<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().shadow_blur))
}

/// Ignored unless finite and not negative.
pub(crate) fn set_shadow_blur<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::set_shadow_blur(data, value));
    Ok(())
}

/// Serialized as `fillStyle`'s colors are.
pub(crate) fn shadow_color<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let color = with_data(cx, this, |data| data.state().shadow_color);
    Ok(DomStr::Owned(DomString::from(
        css::canvas::serialize_canvas_color(color),
    )))
}

/// Parsed as `fillStyle`'s colors are (`currentcolor` too); ignored
/// unless a color.
pub(crate) fn set_shadow_color<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(color) = parse_color(cx, this, value) {
        with_data(cx, this, |data| data.state_mut().shadow_color = color);
    }
    Ok(())
}

// CanvasFillStrokeStyles' gradients

/// `createLinearGradient(x0, y0, x1, y1)`: from `(x0, y0)` to `(x1, y1)`,
/// in the space it is drawn in.
pub(crate) fn create_linear_gradient<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CanvasRenderingContext2d>,
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
) -> Result<Gc<'gc, CanvasGradient>, ApiError> {
    Ok(cx.create_canvas_gradient(context::linear_gradient(x0, y0, x1, y1)))
}

/// `createRadialGradient(x0, y0, r0, x1, y1, r1)`: from one circle to the
/// other; an `IndexSizeError` for a negative radius.
#[expect(clippy::too_many_arguments, reason = "WebIDL's `createRadialGradient`")]
pub(crate) fn create_radial_gradient<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CanvasRenderingContext2d>,
    x0: f64,
    y0: f64,
    r0: f64,
    x1: f64,
    y1: f64,
    r1: f64,
) -> Result<Gc<'gc, CanvasGradient>, ApiError> {
    let kind = context::radial_gradient(x0, y0, r0, x1, y1, r1).map_err(|name| {
        ApiError::Dom(DomException {
            name: ExceptionName::IndexSize,
            message: format!("The {name} provided is less than 0."),
        })
    })?;
    Ok(cx.create_canvas_gradient(kind))
}

/// `createConicGradient(startAngle, x, y)`: round `(x, y)`, clockwise
/// from `startAngle` radians off the x axis.
pub(crate) fn create_conic_gradient<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CanvasRenderingContext2d>,
    start_angle: f64,
    x: f64,
    y: f64,
) -> Result<Gc<'gc, CanvasGradient>, ApiError> {
    Ok(cx.create_canvas_gradient(context::conic_gradient(start_angle, x, y)))
}

/// `addColorStop(offset, color)`: a stop at `offset`, after those already
/// there; an `IndexSizeError` for an offset outside 0 to 1, a
/// `SyntaxError` for what isn't a color (`currentcolor` is black: a
/// gradient has no element, Blink's `ParseCanvasColorString`).
pub(crate) fn add_color_stop<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasGradient>,
    offset: f64,
    color: &str,
) -> Result<(), ApiError> {
    if !(0.0..=1.0).contains(&offset) {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::IndexSize,
            message: format!("The provided value ({offset}) is outside the range (0.0, 1.0)."),
        }));
    }
    let color = match css::canvas::parse_canvas_color(color, &system_colors(cx)) {
        Some(css::canvas::CanvasColor::Color(color)) => color,
        Some(css::canvas::CanvasColor::CurrentColor) => Color::BLACK,
        None => {
            return Err(ApiError::Dom(DomException {
                name: ExceptionName::Syntax,
                message: format!("The value provided ('{color}') could not be parsed as a color."),
            }));
        }
    };
    cx.with_canvas_gradient(this, |gradient| {
        gradient.stops.push(CanvasColorStop { offset, color });
    });
    Ok(())
}

// CanvasPathDrawingStyles

pub(crate) fn line_width<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().line_width))
}

/// Ignored unless finite and more than 0.
pub(crate) fn set_line_width<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    if let Some(value) = context::positive(value) {
        with_data(cx, this, |data| data.state_mut().line_width = value);
    }
    Ok(())
}

pub(crate) fn line_cap<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let cap = with_data(cx, this, |data| data.state().line_cap);
    let name = context::LINE_CAPS
        .iter()
        .find(|(_, c)| *c == cap)
        .map_or("butt", |(name, _)| name);
    Ok(DomStr::Owned(DomString::from(name)))
}

/// Ignored unless one of `CanvasLineCap`'s values (WebIDL's enumeration
/// attribute).
pub(crate) fn set_line_cap<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(&(_, cap)) = context::LINE_CAPS.iter().find(|(name, _)| *name == value) {
        with_data(cx, this, |data| data.state_mut().line_cap = cap);
    }
    Ok(())
}

pub(crate) fn line_join<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let join = with_data(cx, this, |data| data.state().line_join);
    let name = context::LINE_JOINS
        .iter()
        .find(|(_, j)| *j == join)
        .map_or("miter", |(name, _)| name);
    Ok(DomStr::Owned(DomString::from(name)))
}

/// Ignored unless one of `CanvasLineJoin`'s values.
pub(crate) fn set_line_join<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(&(_, join)) = context::LINE_JOINS.iter().find(|(name, _)| *name == value) {
        with_data(cx, this, |data| data.state_mut().line_join = join);
    }
    Ok(())
}

pub(crate) fn miter_limit<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().miter_limit))
}

/// Ignored unless finite and more than 0.
pub(crate) fn set_miter_limit<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    if let Some(value) = context::positive(value) {
        with_data(cx, this, |data| data.state_mut().miter_limit = value);
    }
    Ok(())
}

pub(crate) fn set_line_dash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    segments: Vec<f64>,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| context::set_line_dash(data, segments));
    Ok(())
}

/// `getLineDash()`: the list `setLineDash` kept, an odd one repeated.
pub(crate) fn get_line_dash<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<Vec<f64>, ApiError> {
    Ok(with_data(cx, this, |data| data.state().line_dash.clone()))
}

pub(crate) fn line_dash_offset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<f64, ApiError> {
    Ok(with_data(cx, this, |data| data.state().line_dash_offset))
}

/// Ignored unless finite.
pub(crate) fn set_line_dash_offset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: f64,
) -> Result<(), ApiError> {
    if value.is_finite() {
        with_data(cx, this, |data| data.state_mut().line_dash_offset = value);
    }
    Ok(())
}

// CanvasRect

pub(crate) fn fill_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), ApiError> {
    let flags = flags(cx, this, CanvasStyleTarget::Fill);
    record(cx, this, context::fill_rect_op(x, y, w, h, flags));
    Ok(())
}

pub(crate) fn stroke_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), ApiError> {
    let flags = flags(cx, this, CanvasStyleTarget::Stroke);
    record(cx, this, context::stroke_rect_op(x, y, w, h, flags));
    Ok(())
}

pub(crate) fn clear_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), ApiError> {
    let alpha = cx.canvas_context_settings(this).alpha;
    if let Some(op) = context::clear_rect_op(x, y, w, h, alpha) {
        record_ops(cx, this, vec![op], false);
    }
    Ok(())
}

// CanvasDrawPath

pub(crate) fn begin_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| data.path = gfx::Path::new());
    Ok(())
}

/// A `CanvasFillRule`, `nonzero` without one (WebIDL: another value is a
/// `TypeError`).
fn fill_rule(rule: Option<&str>, member: &str) -> Result<FillRule, ApiError> {
    match rule.unwrap_or("nonzero") {
        "nonzero" => Ok(FillRule::NonZero),
        "evenodd" => Ok(FillRule::EvenOdd),
        other => Err(ApiError::type_error(format!(
            "Failed to execute '{member}' on 'CanvasRenderingContext2D': The provided value '{other}' is not a valid enum value of type CanvasFillRule."
        ))),
    }
}

fn fill_with<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    path: gfx::Path,
    rule: FillRule,
) {
    let flags = flags(cx, this, CanvasStyleTarget::Fill);
    record(cx, this, context::fill_path_op(path, rule, flags));
}

/// `fill(fillRule)`: the current default path filled by the rule.
pub(crate) fn fill<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    rule: Option<&str>,
) -> Result<(), ApiError> {
    let rule = fill_rule(rule, "fill")?;
    let path = with_data(cx, this, |data| data.path.clone());
    fill_with(cx, this, path, rule);
    Ok(())
}

/// `fill(path, fillRule)`: the `Path2D`'s path as it is now, under the
/// current transform.
pub(crate) fn fill_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    path: Gc<'gc, Path2d>,
    rule: Option<&str>,
) -> Result<(), ApiError> {
    let rule = fill_rule(rule, "fill")?;
    let path = cx.with_path_2d(path, |path| path.clone());
    fill_with(cx, this, path, rule);
    Ok(())
}

fn stroke_with<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, CanvasRenderingContext2d>, path: gfx::Path) {
    let flags = flags(cx, this, CanvasStyleTarget::Stroke);
    record(cx, this, context::stroke_path_op(path, flags));
}

pub(crate) fn stroke<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<(), ApiError> {
    let path = with_data(cx, this, |data| data.path.clone());
    stroke_with(cx, this, path);
    Ok(())
}

/// `stroke(path)`: of the `Path2D`'s path as it is now.
pub(crate) fn stroke_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    path: Gc<'gc, Path2d>,
) -> Result<(), ApiError> {
    let path = cx.with_path_2d(path, |path| path.clone());
    stroke_with(cx, this, path);
    Ok(())
}

pub(crate) fn clip<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    rule: Option<&str>,
) -> Result<(), ApiError> {
    let rule = fill_rule(rule, "clip")?;
    with_data(cx, this, |data| {
        let path = data.path.clone();
        context::clip(data, path, rule);
    });
    Ok(())
}

/// `clip(path, fillRule)`: to the `Path2D`'s path as it is now.
pub(crate) fn clip_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    path: Gc<'gc, Path2d>,
    rule: Option<&str>,
) -> Result<(), ApiError> {
    let rule = fill_rule(rule, "clip")?;
    let path = cx.with_path_2d(path, |path| path.clone());
    with_data(cx, this, |data| context::clip(data, path, rule));
    Ok(())
}

/// `isPointInPath(x, y, fillRule)`: whether the point is in the current
/// default path's fill (`canvas::context::point_in_path`).
pub(crate) fn is_point_in_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
    rule: Option<&str>,
) -> Result<bool, ApiError> {
    let rule = fill_rule(rule, "isPointInPath")?;
    Ok(with_data(cx, this, |data| {
        context::point_in_path(data, &data.path, x, y, rule)
    }))
}

/// `isPointInPath(path, x, y, fillRule)`: of the `Path2D`'s path.
pub(crate) fn is_point_in_path_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    path: Gc<'gc, Path2d>,
    x: f64,
    y: f64,
    rule: Option<&str>,
) -> Result<bool, ApiError> {
    let rule = fill_rule(rule, "isPointInPath")?;
    let path = cx.with_path_2d(path, |path| path.clone());
    Ok(with_data(cx, this, |data| {
        context::point_in_path(data, &path, x, y, rule)
    }))
}

/// `isPointInStroke(x, y)`: whether the point is in the current default
/// path's stroke (`canvas::context::point_in_stroke`).
pub(crate) fn is_point_in_stroke<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    x: f64,
    y: f64,
) -> Result<bool, ApiError> {
    Ok(with_data(cx, this, |data| {
        context::point_in_stroke(data, &data.path, x, y)
    }))
}

/// `isPointInStroke(path, x, y)`: of the `Path2D`'s path.
pub(crate) fn is_point_in_stroke_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    path: Gc<'gc, Path2d>,
    x: f64,
    y: f64,
) -> Result<bool, ApiError> {
    let path = cx.with_path_2d(path, |path| path.clone());
    Ok(with_data(cx, this, |data| {
        context::point_in_stroke(data, &path, x, y)
    }))
}

// CanvasPath: `canvas::path`'s, of a context's current default path or a
// `Path2D`'s path, each member's function the same for both.

/// What the `CanvasPath` members build in: a context's data, or a
/// `Path2D`'s path (each taken out and put back for the member).
pub(crate) trait PathOwner<'gc>: Copy {
    fn with_path<R>(self, cx: &ApiCx<'_, 'gc>, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R;
}

impl<'gc> PathOwner<'gc> for Gc<'gc, CanvasRenderingContext2d> {
    fn with_path<R>(self, cx: &ApiCx<'_, 'gc>, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R {
        with_data(cx, self, |mut data| CanvasPath::with_path(&mut data, f))
    }
}

impl<'gc> PathOwner<'gc> for Gc<'gc, Path2d> {
    fn with_path<R>(self, cx: &ApiCx<'_, 'gc>, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R {
        cx.with_path_2d(self, |mut path| CanvasPath::with_path(&mut path, f))
    }
}

/// A `PathOwner` as `canvas::path` builds in it.
struct OwnerPath<'a, 'b, 'gc, T> {
    cx: &'a ApiCx<'b, 'gc>,
    this: T,
}

impl<'gc, T: PathOwner<'gc>> CanvasPath for OwnerPath<'_, '_, 'gc, T> {
    fn with_path<R>(&mut self, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R {
        self.this.with_path(self.cx, f)
    }
}

fn path_error(error: PathError) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::IndexSize,
        message: error.message(),
    })
}

fn on_path<'a, 'b, 'gc, T: PathOwner<'gc>>(
    cx: &'a ApiCx<'b, 'gc>,
    this: T,
    f: impl FnOnce(&mut OwnerPath<'a, 'b, 'gc, T>) -> Result<(), PathError>,
) -> Result<(), ApiError> {
    f(&mut OwnerPath { cx, this }).map_err(path_error)
}

pub(crate) fn close_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
) -> Result<(), ApiError> {
    on_path(cx, this, canvas::path::close_path)
}

pub(crate) fn move_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| canvas::path::move_to(path, x, y))
}

pub(crate) fn line_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| canvas::path::line_to(path, x, y))
}

pub(crate) fn quadratic_curve_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    cpx: f64,
    cpy: f64,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| {
        canvas::path::quadratic_curve_to(path, cpx, cpy, x, y)
    })
}

#[expect(clippy::too_many_arguments, reason = "WebIDL's `bezierCurveTo`")]
pub(crate) fn bezier_curve_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    cp1x: f64,
    cp1y: f64,
    cp2x: f64,
    cp2y: f64,
    x: f64,
    y: f64,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| {
        canvas::path::bezier_curve_to(path, cp1x, cp1y, cp2x, cp2y, x, y)
    })
}

pub(crate) fn arc_to<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    radius: f64,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| {
        canvas::path::arc_to(path, x1, y1, x2, y2, radius)
    })
}

pub(crate) fn rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| canvas::path::rect_path(path, x, y, w, h))
}

#[expect(clippy::too_many_arguments, reason = "WebIDL's `arc`")]
pub(crate) fn arc<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x: f64,
    y: f64,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
    counterclockwise: bool,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| {
        canvas::path::arc(path, x, y, radius, start_angle, end_angle, counterclockwise)
    })
}

#[expect(clippy::too_many_arguments, reason = "WebIDL's `ellipse`")]
pub(crate) fn ellipse<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x: f64,
    y: f64,
    radius_x: f64,
    radius_y: f64,
    rotation: f64,
    start_angle: f64,
    end_angle: f64,
    counterclockwise: bool,
) -> Result<(), ApiError> {
    on_path(cx, this, |path| {
        canvas::path::ellipse(
            path,
            x,
            y,
            radius_x,
            radius_y,
            rotation,
            start_angle,
            end_angle,
            counterclockwise,
        )
    })
}

/// `roundRect(x, y, w, h, radii)`: one to four radii checked
/// (`canvas::path::round_rect_radii`: a `RangeError` for another count, or
/// a negative one), then the path (`canvas::path::round_rect`).
pub(crate) fn round_rect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: impl PathOwner<'gc>,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radii: &[canvas::path::Radius],
) -> Result<(), ApiError> {
    let radii = canvas::path::round_rect_radii(radii).map_err(ApiError::Range)?;
    canvas::path::round_rect(&mut OwnerPath { cx, this }, x, y, w, h, &radii);
    Ok(())
}

// Path2D

/// `new Path2D(path)`: an empty path, a copy of another `Path2D`'s, or
/// path data, read as SVG's `d` is (what comes before an error).
pub(crate) fn construct_path_2d<'gc>(
    cx: &ApiCx<'_, 'gc>,
    path: Option<Path2dOrString<'_, 'gc>>,
) -> Result<Gc<'gc, Path2d>, ApiError> {
    let path = match path {
        None => gfx::Path::new(),
        Some(Path2dOrString::Path(other)) => cx.with_path_2d(other, |path| path.clone()),
        Some(Path2dOrString::String(data)) => css::path_from_string(data),
    };
    Ok(cx.create_path_2d(path))
}

/// `addPath(path, transform)`: `path`'s subpaths (as they are now, so a
/// path added to itself once) mapped by `transform` added after this
/// one's; a `TypeError` where two names of a value differ, nothing where
/// one isn't finite.
pub(crate) fn add_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Path2d>,
    path: Gc<'gc, Path2d>,
    transform: &crate::interfaces::DOMMatrix2DInit,
) -> Result<(), ApiError> {
    let matrix = matrix_2d(transform, "Path2D")?;
    if !matrix.iter().all(|n| n.is_finite()) {
        return Ok(());
    }
    let other = cx.with_path_2d(path, |path| path.clone());
    cx.with_path_2d(this, |path| path.add_path(&other, &context::to_gfx(matrix)));
    Ok(())
}

// CanvasTextDrawingStyles

/// `font`: the font as Chromium writes it back (`10px sans-serif` until
/// one is set).
pub(crate) fn font<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let font = with_data(cx, this, |data| {
        css::serialize_canvas_font(&data.state().font)
    });
    Ok(DomStr::Owned(DomString::from(font)))
}

/// Whether the frame shows `canvas`'s document (Blink's canvas has a
/// frame to resolve fonts in): a canvas in no document a frame shows sets
/// no font and draws no text.
fn in_frame<'gc>(cx: &ApiCx<'_, 'gc>, canvas: NodeRef<'gc>) -> bool {
    let document = cx.node_id(cx.document_of(canvas));
    cx.lend_frame_layout(|layout| layout.shows(document))
        .unwrap_or(false)
}

/// What a canvas's `font` computes against (Blink's `ResolveFont`): its
/// computed style, its document's brought up to date first, and the root
/// element's font size (for `rem`); 10px sans-serif and the root's last
/// size (16px with none) for a canvas that isn't connected, which has no
/// style.
fn font_parent<'gc>(
    cx: &ApiCx<'_, 'gc>,
    canvas: NodeRef<'gc>,
) -> (Arc<computed_style::ComputedStyle>, f32) {
    let document = cx.document_of(canvas);
    let root = std::iter::successors(cx.first_child(document), |&node| cx.next_sibling(node))
        .find(|&node| cx.kind(node) == dom::NodeKind::Element);
    let connected = cx.is_connected(canvas);
    let found = cx.lend_frame_layout(|layout| {
        if connected {
            layout.update_style_and_layout(cx);
        }
        let style = connected
            .then(|| layout.computed_style(cx, canvas))
            .flatten();
        let root = root.and_then(|root| layout.computed_style(cx, root));
        (style, root.map(|root| root.font.size))
    });
    let (style, root) = found.unwrap_or((None, None));
    (
        style.unwrap_or_else(|| Arc::new(css::canvas_font_parent())),
        root.unwrap_or(computed_style::ComputedStyle::initial().font.size),
    )
}

/// Setting `font`: parsed as the `font` shorthand (through the document's
/// cache, `Cx::parse_canvas_font`) and computed against the canvas's style
/// now (Blink's `setFont`, `ResolveFont`); ignored if it isn't a font, if
/// it is the string last set, or if no frame shows the canvas's document
/// (Blink's `WillSetFont`).
pub(crate) fn set_font<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    let canvas = cx.canvas_context_canvas(this);
    if !in_frame(cx, canvas) {
        return Ok(());
    }
    if with_data(cx, this, |data| {
        data.state().font_source.as_deref() == Some(value)
    }) {
        return Ok(());
    }
    let Some(parsed) = cx.parse_canvas_font(cx.document_of(canvas), value) else {
        return Ok(());
    };
    let (parent, root_font_size) = font_parent(cx, canvas);
    let font = parsed.compute(&parent, root_font_size);
    with_data(cx, this, |data| {
        let state = data.state_mut();
        state.font = font;
        state.font_source = Some(value.to_owned());
    });
    Ok(())
}

/// A keyword attribute's value: the name of what the state has.
fn keyword<T: PartialEq + Copy>(
    table: &[(&'static str, T)],
    value: T,
    default: &'static str,
) -> DomStr<'static> {
    let name = table
        .iter()
        .find(|(_, v)| *v == value)
        .map_or(default, |(name, _)| name);
    DomStr::Owned(DomString::from(name))
}

/// Setting a keyword attribute: what `value` names, if it is one of the
/// enumeration's values (WebIDL's enumeration attribute: else ignored).
fn set_keyword<'gc, T: Copy>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    table: &[(&'static str, T)],
    value: &str,
    set: impl FnOnce(&mut CanvasDrawingState, T),
) -> Result<(), ApiError> {
    if let Some(&(_, value)) = table.iter().find(|(name, _)| *name == value) {
        with_data(cx, this, |data| set(data.state_mut(), value));
    }
    Ok(())
}

pub(crate) fn text_align<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let align = with_data(cx, this, |data| data.state().text_align);
    Ok(keyword(&text::TEXT_ALIGNS, align, "start"))
}

pub(crate) fn set_text_align<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::TEXT_ALIGNS, value, |state, v| {
        state.text_align = v
    })
}

pub(crate) fn text_baseline<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let baseline = with_data(cx, this, |data| data.state().text_baseline);
    Ok(keyword(&text::TEXT_BASELINES, baseline, "alphabetic"))
}

pub(crate) fn set_text_baseline<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::TEXT_BASELINES, value, |state, v| {
        state.text_baseline = v
    })
}

/// `direction`: as it was set, `inherit` too.
pub(crate) fn direction<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let direction = with_data(cx, this, |data| data.state().direction);
    Ok(keyword(&text::DIRECTIONS, direction, "inherit"))
}

pub(crate) fn set_direction<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::DIRECTIONS, value, |state, v| {
        state.direction = v
    })
}

pub(crate) fn letter_spacing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let spacing = with_data(cx, this, |data| data.state().letter_spacing.clone());
    Ok(DomStr::Owned(DomString::from(spacing)))
}

/// Ignored unless one length with a unit (`css::parse_canvas_spacing`); it
/// reads back in that unit.
pub(crate) fn set_letter_spacing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(spacing) = css::parse_canvas_spacing(value) {
        with_data(cx, this, |data| {
            data.state_mut().letter_spacing = spacing.to_css()
        });
    }
    Ok(())
}

pub(crate) fn word_spacing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let spacing = with_data(cx, this, |data| data.state().word_spacing.clone());
    Ok(DomStr::Owned(DomString::from(spacing)))
}

/// As `letterSpacing`.
pub(crate) fn set_word_spacing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(spacing) = css::parse_canvas_spacing(value) {
        with_data(cx, this, |data| {
            data.state_mut().word_spacing = spacing.to_css()
        });
    }
    Ok(())
}

pub(crate) fn font_kerning<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let kerning = with_data(cx, this, |data| data.state().font_kerning);
    Ok(keyword(&text::FONT_KERNINGS, kerning, "auto"))
}

pub(crate) fn set_font_kerning<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::FONT_KERNINGS, value, |state, v| {
        state.font_kerning = v
    })
}

/// `fontStretch`: the font's (`font` sets it too).
pub(crate) fn font_stretch<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let stretch = with_data(cx, this, |data| data.state().font.stretch);
    Ok(keyword(&text::FONT_STRETCHES, stretch, "normal"))
}

pub(crate) fn set_font_stretch<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::FONT_STRETCHES, value, |state, v| {
        state.font.stretch = v
    })
}

/// `fontVariantCaps`: the font's (`font` sets it too, to `small-caps` or
/// `normal`).
pub(crate) fn font_variant_caps<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let caps = with_data(cx, this, |data| data.state().font.variant_caps);
    Ok(keyword(&text::FONT_VARIANT_CAPS, caps, "normal"))
}

pub(crate) fn set_font_variant_caps<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::FONT_VARIANT_CAPS, value, |state, v| {
        state.font.variant_caps = v
    })
}

pub(crate) fn text_rendering<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let rendering = with_data(cx, this, |data| data.state().text_rendering);
    Ok(keyword(&text::TEXT_RENDERINGS, rendering, "auto"))
}

pub(crate) fn set_text_rendering<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    set_keyword(cx, this, &text::TEXT_RENDERINGS, value, |state, v| {
        state.text_rendering = v
    })
}

// CanvasText

/// What the state lays text out in: its font, its direction (`inherit`
/// the canvas's directionality), its spacings in px (`em` against the
/// font) and its kerning.
fn text_style(
    state: &CanvasDrawingState,
    direction: computed_style::Direction,
) -> computed_style::CanvasTextStyle {
    let spacing = |text: &str| {
        css::parse_canvas_spacing(text).map_or(0.0, |spacing| spacing.to_px(state.font.size))
    };
    computed_style::CanvasTextStyle {
        font: state.font.clone(),
        direction,
        letter_spacing: spacing(&state.letter_spacing),
        word_spacing: spacing(&state.word_spacing),
        kerning: state.font_kerning,
    }
}

/// The state, and `text` prepared and laid out in it by the frame (with
/// its ink if `ink`), and the direction it ran; `None` where no frame
/// shows the canvas's document (Blink resolves no fonts there) or the
/// frame lays no text out.
fn lay_out_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    text: &str,
    ink: bool,
) -> Option<(
    CanvasDrawingState,
    computed_style::Direction,
    gfx::ShapedText,
)> {
    let canvas = cx.canvas_context_canvas(this);
    if !in_frame(cx, canvas) {
        return None;
    }
    let state = with_data(cx, this, |data| data.state().clone());
    let direction = state.direction.unwrap_or_else(|| cx.directionality(canvas));
    let style = text_style(&state, direction);
    let text = text::prepare_text(text);
    let shaped = cx
        .lend_frame_layout(|layout| layout.shape_canvas_text(&text, &style, ink))
        .flatten()?;
    Some((state, direction, shaped))
}

/// `fillText` and `strokeText` (`canvas::text::text_ops`): nothing where
/// no frame shows the document, for an `x` or `y` that isn't finite, or a
/// `maxWidth` that isn't finite or > 0.
fn draw_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    text: &str,
    (x, y, max_width): (f64, f64, Option<f64>),
    stroke: bool,
) {
    if !text::text_args_drawable(x, y, max_width) {
        return;
    }
    let Some((state, direction, shaped)) = lay_out_text(cx, this, text, false) else {
        return;
    };
    let target = if stroke {
        CanvasStyleTarget::Stroke
    } else {
        CanvasStyleTarget::Fill
    };
    let flags = flags(cx, this, target);
    let (scale, ops) = text::text_ops(&state, direction, shaped, (x, y, max_width), &flags, stroke);
    record_ops_under(cx, this, [scale, 0.0, 0.0, 1.0, 0.0, 0.0], ops);
}

/// `fillText(text, x, y, maxWidth)`: the text filled with `fillStyle`.
pub(crate) fn fill_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    text: &str,
    x: f64,
    y: f64,
    max_width: Option<f64>,
) -> Result<(), ApiError> {
    draw_text(cx, this, text, (x, y, max_width), false);
    Ok(())
}

/// `strokeText(text, x, y, maxWidth)`: the glyphs' outlines stroked with
/// `strokeStyle` and the line styles.
pub(crate) fn stroke_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    text: &str,
    x: f64,
    y: f64,
    max_width: Option<f64>,
) -> Result<(), ApiError> {
    draw_text(cx, this, text, (x, y, max_width), true);
    Ok(())
}

/// `measureText(text)` (`canvas::text::text_metrics`): all 0 where no
/// frame shows the document, as in Blink.
pub(crate) fn measure_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    text: &str,
) -> Result<Gc<'gc, dom::TextMetrics>, ApiError> {
    let values = lay_out_text(cx, this, text, true)
        .map(|(state, direction, shaped)| text::text_metrics(&state, direction, &shaped))
        .unwrap_or_default();
    Ok(cx.create_text_metrics(values))
}

macro_rules! text_metrics_getters {
    ($($name:ident),* $(,)?) => {
        $(
            pub(crate) fn $name<'gc>(
                cx: &ApiCx<'_, 'gc>,
                this: Gc<'gc, dom::TextMetrics>,
            ) -> Result<f64, ApiError> {
                Ok(cx.text_metrics(this).$name)
            }
        )*
    };
}

/// `TextMetrics`' attributes, each its number.
pub(crate) mod text_metrics {
    use super::{ApiCx, ApiError, Gc};

    text_metrics_getters!(
        width,
        actual_bounding_box_left,
        actual_bounding_box_right,
        font_bounding_box_ascent,
        font_bounding_box_descent,
        actual_bounding_box_ascent,
        actual_bounding_box_descent,
        hanging_baseline,
        alphabetic_baseline,
        ideographic_baseline,
    );
}

// CanvasDrawImage, CanvasImageSmoothing, patterns

fn invalid_state(message: impl Into<String>) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::InvalidState,
        message: message.into(),
    })
}

/// HTML's "check the usability of the image argument" and Blink's
/// `GetSourceImageForCanvas`: the image, `None` where there is nothing to
/// draw yet (an `<img>` not loaded, or of no size); an `InvalidStateError`
/// for a broken `<img>` or a canvas of no width or height. A canvas's
/// recording is drawn first, then its pixels taken as they are
/// (`canvas::image::canvas_source`); one whose size can't have pixels has
/// none.
fn source_image<'gc>(
    cx: &ApiCx<'_, 'gc>,
    source: NodeRef<'gc>,
) -> Result<Option<SourceImage>, ApiError> {
    if cx.is_img(source) {
        return match cx.image_for_canvas(source) {
            ImageForCanvas::Nothing => Ok(None),
            ImageForCanvas::Broken => Err(invalid_state(image::BROKEN_IMAGE_MESSAGE)),
            ImageForCanvas::Image {
                data,
                size,
                density,
                origin_clean,
            } => {
                let density = if density > 0.0 { density } else { 1.0 };
                Ok(Some(SourceImage {
                    data: Some(data),
                    size: (
                        f64::from(size.width) / density,
                        f64::from(size.height) / density,
                    ),
                    density,
                    origin_clean,
                }))
            }
        };
    }
    let size = cx.canvas_size(source);
    if size.0 == 0 || size.1 == 0 {
        return Err(invalid_state(image::empty_canvas_message(size.0 == 0)));
    }
    if !canvas::is_valid_bitmap_size(size) {
        return Ok(Some(image::invalid_canvas_source(size)));
    }
    Ok(Some(match cx.canvas_context(source) {
        Some(context) => {
            flush(cx, context, CanvasFlush::Read);
            let background = canvas::background(cx.canvas_context_settings(context));
            with_data(cx, context, |data| {
                image::canvas_source(Some(data), background, size)
            })
        }
        None => image::canvas_source(None, [0; 4], size),
    }))
}

/// `drawImage` (`canvas::image::draw_image_op`): nothing for a number that
/// isn't finite, or an image with nothing to draw; a canvas that draws an
/// image another origin's isn't origin-clean after.
fn draw_image<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    source: NodeRef<'gc>,
    src: Option<[f64; 4]>,
    dst: ([f64; 2], Option<[f64; 2]>),
) -> Result<(), ApiError> {
    if !image::draw_image_args_finite(src, dst) {
        return Ok(());
    }
    let Some(source) = source_image(cx, source)? else {
        return Ok(());
    };
    let op = with_data(cx, this, |data| {
        let op = image::draw_image_op(data.state(), &source, src, dst)?;
        if !source.origin_clean {
            data.origin_clean = false;
        }
        Some(op)
    });
    record(cx, this, op);
    Ok(())
}

/// `drawImage(image, dx, dy)`: the whole image at its size.
pub(crate) fn draw_image_at<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    image: NodeRef<'gc>,
    dx: f64,
    dy: f64,
) -> Result<(), ApiError> {
    draw_image(cx, this, image, None, ([dx, dy], None))
}

/// `drawImage(image, dx, dy, dw, dh)`: the whole image, scaled to the
/// rect.
pub(crate) fn draw_image_scaled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    image: NodeRef<'gc>,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
) -> Result<(), ApiError> {
    draw_image(cx, this, image, None, ([dx, dy], Some([dw, dh])))
}

/// `drawImage(image, sx, sy, sw, sh, dx, dy, dw, dh)`: part of the image,
/// scaled to the rect.
#[expect(clippy::too_many_arguments, reason = "the WebIDL operation's")]
pub(crate) fn draw_image_part<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    image: NodeRef<'gc>,
    sx: f64,
    sy: f64,
    sw: f64,
    sh: f64,
    dx: f64,
    dy: f64,
    dw: f64,
    dh: f64,
) -> Result<(), ApiError> {
    draw_image(
        cx,
        this,
        image,
        Some([sx, sy, sw, sh]),
        ([dx, dy], Some([dw, dh])),
    )
}

pub(crate) fn image_smoothing_enabled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<bool, ApiError> {
    Ok(with_data(cx, this, |data| {
        data.state().image_smoothing_enabled
    }))
}

pub(crate) fn set_image_smoothing_enabled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: bool,
) -> Result<(), ApiError> {
    with_data(cx, this, |data| {
        data.state_mut().image_smoothing_enabled = value
    });
    Ok(())
}

pub(crate) fn image_smoothing_quality<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<DomStr<'gc>, ApiError> {
    let quality = with_data(cx, this, |data| data.state().image_smoothing_quality);
    Ok(DomStr::Owned(DomString::from(
        image::smoothing_quality_name(quality),
    )))
}

/// `imageSmoothingQuality`'s setter: one of `ImageSmoothingQuality`'s
/// values, anything else ignored (WebIDL's enumeration attribute); kept
/// while smoothing is off.
pub(crate) fn set_image_smoothing_quality<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(&(_, quality)) = image::SMOOTHING_QUALITIES
        .iter()
        .find(|(name, _)| *name == value)
    {
        with_data(cx, this, |data| {
            data.state_mut().image_smoothing_quality = quality
        });
    }
    Ok(())
}

/// `createPattern(image, repetition)`: the image as it is now, tiled as
/// `repetition` says (`canvas::image::parse_repetition`: a `SyntaxError`
/// for what isn't one, checked first, as Blink does); `null` while the
/// image has nothing to draw.
pub(crate) fn create_pattern<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CanvasRenderingContext2d>,
    source: NodeRef<'gc>,
    repetition: &str,
) -> Result<Option<Gc<'gc, CanvasPattern>>, ApiError> {
    let repeat = image::parse_repetition(repetition).map_err(|message| {
        ApiError::Dom(DomException {
            name: ExceptionName::Syntax,
            message,
        })
    })?;
    let Some(source) = source_image(cx, source)? else {
        return Ok(None);
    };
    Ok(Some(cx.create_canvas_pattern(image::pattern_data(
        source, repeat,
    ))))
}

/// `CanvasPattern`'s `setTransform(transform)`: the pattern drawn through
/// `transform` from now on (any numbers, as Blink keeps them); a
/// `TypeError` where a value's two names differ.
pub(crate) fn set_pattern_transform<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasPattern>,
    transform: &crate::interfaces::DOMMatrix2DInit,
) -> Result<(), ApiError> {
    let matrix = matrix_2d(transform, "CanvasPattern")?;
    cx.with_canvas_pattern(this, |pattern| pattern.transform = matrix);
    Ok(())
}

// CanvasTransform's dictionary form, and the settings

/// The matrix a `DOMMatrix2DInit` makes (geometry's "validate and fixup
/// (2D)": a `TypeError` where a value's two names differ), for
/// `interface`'s `setTransform`.
fn matrix_2d(
    transform: &crate::interfaces::DOMMatrix2DInit,
    interface: &str,
) -> Result<[f64; 6], ApiError> {
    let t = transform;
    context::matrix_2d([
        (t.a, t.m11),
        (t.b, t.m12),
        (t.c, t.m21),
        (t.d, t.m22),
        (t.e, t.m41),
        (t.f, t.m42),
    ])
    .map_err(|(short, long)| {
        ApiError::type_error(format!(
            "Failed to execute 'setTransform' on '{interface}': The '{short}' property should equal the '{long}' property."
        ))
    })
}

/// `setTransform(transform)`: the transform a `DOMMatrix2DInit` makes
/// (geometry's "validate and fixup (2D)": a `TypeError` where a value's
/// two names differ), ignored unless every value is finite; with none
/// given, the identity.
pub(crate) fn set_transform_matrix<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    transform: &crate::interfaces::DOMMatrix2DInit,
) -> Result<(), ApiError> {
    let matrix = matrix_2d(transform, "CanvasRenderingContext2D")?;
    with_data(cx, this, |data| context::set_transform(data, matrix));
    Ok(())
}

fn color_space_name(space: PredefinedColorSpace) -> &'static str {
    match space {
        PredefinedColorSpace::Srgb => "srgb",
        PredefinedColorSpace::DisplayP3 => "display-p3",
    }
}

/// `getContextAttributes()`: the settings it was made with.
pub(crate) fn get_context_attributes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
) -> Result<crate::interfaces::CanvasRenderingContext2DSettings, ApiError> {
    let settings = cx.canvas_context_settings(this);
    Ok(crate::interfaces::CanvasRenderingContext2DSettings {
        alpha: settings.alpha,
        colorSpace: color_space_name(settings.color_space).to_owned(),
        colorType: match settings.color_type {
            CanvasColorType::Unorm8 => "unorm8",
            CanvasColorType::Float16 => "float16",
        }
        .to_owned(),
        desynchronized: settings.desynchronized,
        willReadFrequently: settings.will_read_frequently,
    })
}

// CanvasImageData and ImageData

/// The color space `settings` says (WebIDL: one of `PredefinedColorSpace`'s
/// values, else a `TypeError`), none if it says none.
fn settings_color_space(
    settings: &crate::interfaces::ImageDataSettings,
    member: &str,
) -> Result<Option<PredefinedColorSpace>, ApiError> {
    match settings.colorSpace.as_deref() {
        None => Ok(None),
        Some("srgb") => Ok(Some(PredefinedColorSpace::Srgb)),
        Some("display-p3") => Ok(Some(PredefinedColorSpace::DisplayP3)),
        Some(other) => Err(ApiError::type_error(format!(
            "Failed to execute '{member}': Failed to read the 'colorSpace' property from 'ImageDataSettings': The provided value '{other}' is not a valid enum value of type PredefinedColorSpace."
        ))),
    }
}

fn index_size(message: impl Into<String>) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::IndexSize,
        message: message.into(),
    })
}

fn out_of_memory() -> ApiError {
    ApiError::Range(canvas::pixels::OUT_OF_MEMORY.into())
}

/// An `IndexSizeError` for a width or height of 0.
fn check_size(width: u32, height: u32) -> Result<(), ApiError> {
    if width == 0 || height == 0 {
        let which = if width == 0 { "width" } else { "height" };
        return Err(index_size(format!("The source {which} is 0.")));
    }
    Ok(())
}

/// A new `ImageData` of `bytes`, its array made of them after the
/// operation.
fn new_image_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    (width, height): (u32, u32),
    space: PredefinedColorSpace,
    pixels: Vec<u8>,
) -> ImageDataWithPixels<'gc> {
    ImageDataWithPixels {
        image_data: cx.create_image_data(width, height, space),
        pixels,
    }
}

/// Zeroed pixels of an `ImageData` of `width` × `height`: a `RangeError`
/// where there can't be that many.
fn zeroed_pixels(width: u32, height: u32) -> Result<Vec<u8>, ApiError> {
    canvas::pixels::zeroed_pixels(width, height).ok_or_else(out_of_memory)
}

/// `new ImageData(sw, sh, settings)`: `sw` × `sh` transparent black pixels,
/// in the settings' color space (sRGB by default). An `IndexSizeError` for
/// a width or height of 0, or more bytes than 32 bits count (Blink's
/// `ValidateAndCreate`); a `RangeError` for more than there can be.
pub(crate) fn construct_image_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    sw: u32,
    sh: u32,
    settings: &crate::interfaces::ImageDataSettings,
) -> Result<ImageDataWithPixels<'gc>, ApiError> {
    let space = settings_color_space(settings, "ImageData")?;
    check_size(sw, sh)?;
    // No product of two u32s and 4 is past a u64 but past a u32 too.
    let bytes = u64::from(sw)
        .checked_mul(u64::from(sh))
        .and_then(|area| area.checked_mul(4));
    if bytes.is_none_or(|bytes| bytes > u64::from(u32::MAX)) {
        return Err(index_size(
            "The requested image size exceeds the supported range.",
        ));
    }
    let bytes = zeroed_pixels(sw, sh)?;
    Ok(new_image_data(
        cx,
        (sw, sh),
        space.unwrap_or(PredefinedColorSpace::Srgb),
        bytes,
    ))
}

/// `new ImageData(data, sw, sh, settings)`: an `ImageData` of `data`
/// itself (HTML's, Blink's `ImageData::Create` of an array): its length a
/// nonzero multiple of 4 (else an `InvalidStateError`) and of `sw` pixels
/// (else an `IndexSizeError`), its height the rest, `sh` if given (else an
/// `IndexSizeError`).
pub(crate) fn construct_image_data_of<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    data: Uint8ClampedArg,
    sw: u32,
    sh: Option<u32>,
    settings: &crate::interfaces::ImageDataSettings,
) -> Result<Gc<'gc, ImageData>, ApiError> {
    let space = settings_color_space(settings, "ImageData")?;
    if data.len == 0 || !data.len.is_multiple_of(4) {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: "The input data length is not a multiple of 4.".into(),
        }));
    }
    let pixels = data.len / 4;
    if sw == 0 {
        return Err(index_size("The source width is 0."));
    }
    let width = sw as usize;
    if !pixels.is_multiple_of(width) {
        return Err(index_size(
            "The input data length is not a multiple of (4 * width).",
        ));
    }
    // A u32 holds it: no more pixels than the bytes, a u32's.
    let height: u32 = base::numerics::saturated_cast(pixels.checked_div(width).unwrap_or(0));
    if sh.is_some_and(|sh| sh != height) {
        return Err(index_size(
            "The input data length is not equal to (4 * width * height).",
        ));
    }
    let image_data = cx.create_image_data(sw, height, space.unwrap_or(PredefinedColorSpace::Srgb));
    op.store_array(cx, cx.image_data_array_edge(image_data), data);
    Ok(image_data)
}

pub(crate) fn image_data_width<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ImageData>,
) -> Result<u32, ApiError> {
    Ok(cx.image_data_size(this).0)
}

pub(crate) fn image_data_height<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ImageData>,
) -> Result<u32, ApiError> {
    Ok(cx.image_data_size(this).1)
}

/// `data`: its pixels, the engine's array (the same one each time).
pub(crate) fn image_data_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ImageData>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx.image_data_array_edge(this))
}

pub(crate) fn image_data_color_space<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, ImageData>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(color_space_name(
        cx.image_data_color_space(this),
    ))))
}

/// `createImageData(sw, sh, settings)`: transparent black pixels, as many
/// as the sizes say whatever their signs; an `IndexSizeError` for a width
/// or height of 0. The color space is the settings', else the context's.
pub(crate) fn create_image_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    sw: i32,
    sh: i32,
    settings: &crate::interfaces::ImageDataSettings,
) -> Result<ImageDataWithPixels<'gc>, ApiError> {
    let space = settings_color_space(settings, "createImageData")?;
    let (width, height) = (sw.unsigned_abs(), sh.unsigned_abs());
    check_size(width, height)?;
    let bytes = zeroed_pixels(width, height)?;
    let space = space.unwrap_or(cx.canvas_context_settings(this).color_space);
    Ok(new_image_data(cx, (width, height), space, bytes))
}

/// `createImageData(imageData)`: transparent black pixels of its size and
/// color space.
pub(crate) fn create_image_data_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CanvasRenderingContext2d>,
    image_data: Gc<'gc, ImageData>,
) -> Result<ImageDataWithPixels<'gc>, ApiError> {
    let size = cx.image_data_size(image_data);
    let bytes = zeroed_pixels(size.0, size.1)?;
    let space = cx.image_data_color_space(image_data);
    Ok(new_image_data(cx, size, space, bytes))
}

/// `getImageData(sx, sy, sw, sh, settings)`: the pixels of that rect of
/// the bitmap (a negative size goes left or up), not premultiplied, what
/// was recorded drawn first; transparent black outside the bitmap. An
/// `IndexSizeError` for a width or height of 0, a `SecurityError` for a
/// canvas that isn't origin-clean. The color space is the settings', else
/// the context's (the pixels aren't converted).
#[expect(clippy::too_many_arguments, reason = "WebIDL's `getImageData`")]
pub(crate) fn get_image_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    sx: i32,
    sy: i32,
    sw: i32,
    sh: i32,
    settings: &crate::interfaces::ImageDataSettings,
) -> Result<ImageDataWithPixels<'gc>, ApiError> {
    let space = settings_color_space(settings, "getImageData")?;
    if sw == 0 || sh == 0 {
        let which = if sw == 0 { "width" } else { "height" };
        return Err(index_size(format!("The source {which} is 0.")));
    }
    if !with_data(cx, this, |data| data.origin_clean) {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::Security,
            message: "The canvas has been tainted by cross-origin data.".into(),
        }));
    }
    let (x, y, width, height) = canvas::pixels::source_rect(sx, sy, sw, sh);
    let mut bytes = zeroed_pixels(width, height)?;
    let context_settings = cx.canvas_context_settings(this);
    with_read_pixels(cx, this, |data, pixels| {
        canvas::pixels::read_rect(
            data,
            pixels,
            canvas::background(context_settings),
            (x, y),
            width,
            &mut bytes,
        );
    });
    // A bitmap read that often comes back to be drawn here.
    if let Some(snapshot) =
        with_data(cx, this, canvas::pixels::count_readback).filter(gfx::ImageData::wait_ready)
    {
        with_data(cx, this, |data| canvas::pixels::take_back(data, &snapshot));
    }
    let space = space.unwrap_or(context_settings.color_space);
    Ok(new_image_data(cx, (width, height), space, bytes))
}

/// `putImageData(imageData, dx, dy)`: all its pixels at `(dx, dy)`.
pub(crate) fn put_image_data<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    image_data: Gc<'gc, ImageData>,
    dx: i32,
    dy: i32,
) -> Result<(), ApiError> {
    let (width, height) = cx.image_data_size(image_data);
    let side = |n: u32| i32::try_from(n).unwrap_or(i32::MAX);
    put_image_data_part(
        op,
        cx,
        this,
        image_data,
        dx,
        dy,
        0,
        0,
        side(width),
        side(height),
    )
}

/// `putImageData(imageData, dx, dy, dirtyX, dirtyY, dirtyWidth,
/// dirtyHeight)`: the pixels of the dirty rect of it written over the
/// bitmap's at `(dx, dy)` past that rect's corner, as they are
/// (`canvas::pixels::put_pixels`), read from its array in place. What was
/// recorded is drawn first, or dropped if the pixels cover the whole
/// bitmap. An `InvalidStateError` for an array that was detached.
#[expect(clippy::too_many_arguments, reason = "WebIDL's `putImageData`")]
pub(crate) fn put_image_data_part<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CanvasRenderingContext2d>,
    image_data: Gc<'gc, ImageData>,
    dx: i32,
    dy: i32,
    dirty_x: i32,
    dirty_y: i32,
    dirty_width: i32,
    dirty_height: i32,
) -> Result<(), ApiError> {
    let (width, height) = cx.image_data_size(image_data);
    let bitmap = with_data(cx, this, |data| data.size);
    let region = canvas::pixels::put_region(
        (width, height),
        bitmap,
        (dx, dy),
        (dirty_x, dirty_y, dirty_width, dirty_height),
    );
    let expected = gfx::Size::new(width, height).checked_byte_size();
    let mut detached = false;
    let mut put = None;
    let edge = cx.image_data_array_edge(image_data);
    // What was recorded, first: drawn, or (the whole bitmap written over)
    // dropped. A bitmap another thread has gets the pixels after it.
    if let Some(region) = region {
        if region.whole {
            with_data(cx, this, |data| {
                data.ops.clear();
                data.matrix = None;
            });
        } else if with_data(cx, this, |data| data.resource.is_none()) {
            flush(cx, this, CanvasFlush::Draw);
        }
    }
    let settings = cx.canvas_context_settings(this);
    op.with_array_bytes(cx, edge, &mut |bytes| {
        let Some(bytes) = bytes.filter(|bytes| Some(bytes.len()) == expected) else {
            detached = true;
            return;
        };
        if let Some(region) = region {
            put = Some(with_data(cx, this, |data| {
                canvas::pixels::put_pixels(data, settings, (bytes, width), region, (dx, dy))
            }));
        }
    });
    if detached {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: "The source data has been detached.".into(),
        }));
    }
    match put {
        None | Some(canvas::pixels::Put::NoMemory) => {}
        Some(canvas::pixels::Put::Written) => cx.record_canvas_draw(cx.canvas_context_canvas(this)),
        Some(canvas::pixels::Put::Recorded(count)) => {
            cx.record_canvas_draw(cx.canvas_context_canvas(this));
            if count >= MAX_RECORDED_OPS {
                flush(cx, this, CanvasFlush::Draw);
            }
        }
    }
    Ok(())
}
