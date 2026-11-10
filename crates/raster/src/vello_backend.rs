//! PROTOTYPE (not for main): tiles and canvases drawn by vello_cpu instead
//! of Skia, with `MACHOPO_RASTER=vello`.
//!
//! Each `PaintOp` is drawn on a `RenderContext`, one per raster thread,
//! then rendered into the pixels. What Vello has no call for is made up
//! here from what it has: filters it doesn't run and filters at a tile's
//! edge (the layer drawn off to the side, in a context larger than the
//! tile), a clip to a union of paths (a mask), shadows and blurs of any
//! shape, patterns of records or with gaps.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "PROTOTYPE: kurbo's operators on f64 points, vectors and matrices, which \
              don't overflow; each to be looked at before this is for main"
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use base::numerics::{clamp_ceil, saturated_cast};
use compositor_paint::{
    BlendMode, ColorInterpolation, DisplayItemList, FilterQuality, GradientShader,
    GradientShaderKind, ImageFilter, LineCap, LineJoin, PaintFlags, PaintOp, PaintShader,
    PaintStyle, ShaderImage, TileMode, TiledShader,
};
use gfx::{Color, FillRule, FontData, ImageData, PathElement, PointF, RRectF, RectF, Size};
use vello_cpu::filter_effects::{EdgeMode, Filter, FilterPrimitive};
use vello_cpu::kurbo::{self, Affine, BezPath, Cap, Join, Point, Rect, Shape, Stroke};
use vello_cpu::peniko::{
    self, Compose, Extend, Fill, Gradient, ImageAlphaType, ImageQuality, ImageSampler,
    InterpolationAlphaSpace, Mix,
};
use vello_cpu::{
    Image, ImageSource, Mask, PixelMetadata, Pixmap, PixmapMut, RasterizerSettings, RenderContext,
    RenderMode, Resources, TargetInit,
};

use crate::PlaybackParams;

/// What the prototype keeps between tiles, beside Skia's in `RasterCache`:
/// fonts as Vello takes them and decoded images.
#[derive(Default)]
pub(crate) struct VelloCache {
    tables: Mutex<Tables>,
}

#[derive(Default)]
struct Tables {
    /// By font id and face index: Vello keys what it keeps for a font on
    /// the id of the blob it was handed, so each font is one blob.
    fonts: HashMap<(u64, u32), peniko::FontData>,
    /// By image id, as `RasterCache`'s: filled once, outside the lock.
    images: HashMap<u64, (ImageBytes, ImageSlot)>,
}

type ImageBytes = Weak<dyn AsRef<[u8]> + Send + Sync>;
type ImageSlot = Arc<OnceLock<Option<Arc<Pixmap>>>>;

impl VelloCache {
    fn tables(&self) -> std::sync::MutexGuard<'_, Tables> {
        self.tables
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn font(&self, font: &FontData) -> peniko::FontData {
        self.tables()
            .fonts
            .entry((font.id, font.index))
            .or_insert_with(|| {
                peniko::FontData::new(peniko::Blob::new(font.bytes.clone()), font.index)
            })
            .clone()
    }

    fn image(&self, data: &ImageData) -> Option<Arc<Pixmap>> {
        if !data.wait_ready() {
            return None;
        }
        let slot = {
            let mut tables = self.tables();
            match tables.images.get(&data.id) {
                Some((_, slot)) => slot.clone(),
                None => {
                    tables
                        .images
                        .retain(|_, (bytes, _)| bytes.strong_count() > 0);
                    let slot = ImageSlot::default();
                    tables
                        .images
                        .insert(data.id, (Arc::downgrade(&data.bytes), slot.clone()));
                    slot
                }
            }
        };
        slot.get_or_init(|| {
            let (size, pixels) = match data.decoded {
                Some(size) => (size, data.as_bytes().to_vec()),
                None => {
                    let decoded = crate::image_decode::decode(data.as_bytes())?;
                    (decoded.size, decoded.pixels)
                }
            };
            let width = u16::try_from(size.width).ok()?;
            let height = u16::try_from(size.height).ok()?;
            Some(Arc::new(premultiplied_pixmap(pixels, width, height)))
        })
        .clone()
    }
}

/// Premultiplied RGBA8 `pixels` as a pixmap.
fn premultiplied_pixmap(pixels: Vec<u8>, width: u16, height: u16) -> Pixmap {
    Pixmap::from_parts(
        pixels,
        width,
        height,
        PixelMetadata {
            may_have_transparency: true,
            alpha_type: ImageAlphaType::AlphaPremultiplied,
        },
    )
}

thread_local! {
    /// This raster thread's context, kept from tile to tile for its
    /// allocations.
    static CONTEXT: RefCell<Option<(RenderContext, Resources)>> = const { RefCell::new(None) };
    /// How many ops of each kind this thread left out.
    static SKIPPED: RefCell<HashMap<&'static str, usize>> = RefCell::new(HashMap::new());
    static PROFILE: RefCell<Profile> = RefCell::new(Profile::default());
}

/// An op (or a part of one) the prototype doesn't draw: counted, and
/// logged when the count doubles, so a page's log says what it lacked.
fn skipped(what: &'static str) {
    SKIPPED.with_borrow_mut(|counts| {
        let count = counts.entry(what).or_insert(0);
        *count = count.saturating_add(1);
        if count.is_power_of_two() {
            tracing::warn!(what, count = *count, "vello prototype: not drawn");
        }
    });
}

/// Whether pixels of `size` can be drawn here: a context's side is a `u16`.
pub(crate) fn can_draw(size: Size) -> bool {
    u16::try_from(size.width).is_ok() && u16::try_from(size.height).is_ok()
}

/// `skia_backend::playback`, with Vello.
pub(crate) fn playback(
    list: &DisplayItemList,
    ops: &[usize],
    params: &PlaybackParams,
    cache: &VelloCache,
    size: Size,
    pixels: &mut [u8],
) {
    let (Ok(width), Ok(height)) = (u16::try_from(size.width), u16::try_from(size.height)) else {
        return;
    };
    CONTEXT.with_borrow_mut(|slot| {
        let (context, resources) =
            slot.get_or_insert_with(|| (RenderContext::new(width, height), Resources::new()));
        let profile = switches().profile;
        let started = Instant::now();
        context.reset_and_resize(width, height);
        let scale = f64::from(params.contents_scale);
        let base = Affine::translate((
            -f64::from(params.content_rect.x),
            -f64::from(params.content_rect.y),
        )) * Affine::scale(scale);
        let mut player = Player::new(context, resources, cache, base);
        player.timings = profile.then(Vec::new);
        let all = list.ops();
        let ops: Vec<&PaintOp> = ops.iter().map(|&op| &all[op]).collect();
        player.play(&ops);
        player.finish();
        let glyphs = player.glyphs;
        let timings = player.timings.take();
        let Some(target) = PixmapMut::new(width, height, pixels) else {
            return;
        };
        context.flush();
        context.render_with(
            target,
            resources,
            settings(TargetInit::Clear(color(params.clear_color))),
        );
        if let Some(timings) = timings {
            Profile::add(started.elapsed(), timings, glyphs);
        }
    });
}

/// `skia_backend::draw_into`, with Vello: `list` over what `pixels` have.
pub(crate) fn draw_into(
    list: &DisplayItemList,
    cache: &VelloCache,
    size: Size,
    opaque: bool,
    pixels: &mut [u8],
) {
    let (Ok(width), Ok(height)) = (u16::try_from(size.width), u16::try_from(size.height)) else {
        return;
    };
    CONTEXT.with_borrow_mut(|slot| {
        let (context, resources) =
            slot.get_or_insert_with(|| (RenderContext::new(width, height), Resources::new()));
        context.reset_and_resize(width, height);
        let mut player = Player::new(context, resources, cache, Affine::IDENTITY);
        player.canvas = Some(pixels);
        player.seed_canvas();
        let ops: Vec<&PaintOp> = list.ops().iter().collect();
        player.play(&ops);
        player.finish();
    });
    if opaque {
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
    }
}

/// `draw` drawn on a context of its own of `width` × `height`, starting
/// with `transform` (what its `SetMatrix`es are relative to), and rendered
/// over transparent black.
fn render_pixmap(
    cache: &VelloCache,
    width: u16,
    height: u16,
    transform: Affine,
    draw: impl FnOnce(&mut Player<'_>),
) -> Pixmap {
    let mut context = RenderContext::new(width, height);
    let mut resources = Resources::new();
    let mut player = Player::new(&mut context, &mut resources, cache, transform);
    draw(&mut player);
    player.finish();
    let mut pixmap = Pixmap::new(width, height);
    context.flush();
    context.render_with(
        &mut pixmap,
        &mut resources,
        settings(TargetInit::Clear(peniko::Color::TRANSPARENT)),
    );
    pixmap
}

/// How a context is rendered: over `target_init`, with the pipeline the
/// environment picks.
fn settings(target_init: TargetInit) -> RasterizerSettings {
    RasterizerSettings {
        render_mode: if switches().quality {
            RenderMode::OptimizeQuality
        } else {
            RenderMode::OptimizeSpeed
        },
        target_init,
        ..RasterizerSettings::default()
    }
}

/// PROTOTYPE: where a thread's tiles' time went (`MACHOPO_VELLO_PROFILE=1`).
#[derive(Default)]
struct Profile {
    tiles: usize,
    total: Duration,
    /// Everything but handing the ops to the context: resetting it, popping
    /// what is left and rendering.
    render: Duration,
    /// By op: how many, and the time handing them to the context took.
    ops: HashMap<&'static str, (usize, Duration)>,
    /// Glyphs in the runs, and those handed to Vello.
    glyphs: (usize, usize),
}

impl Profile {
    fn add(total: Duration, timings: Vec<(&'static str, Duration)>, glyphs: (usize, usize)) {
        let drawing: Duration = timings.iter().map(|(_, time)| *time).sum();
        PROFILE.with_borrow_mut(|sums| {
            sums.tiles += 1;
            sums.total += total;
            sums.render += total.saturating_sub(drawing);
            sums.glyphs.0 += glyphs.0;
            sums.glyphs.1 += glyphs.1;
            for (kind, time) in timings {
                let entry = sums.ops.entry(kind).or_default();
                entry.0 += 1;
                entry.1 += time;
            }
            if !sums.tiles.is_power_of_two() || sums.tiles < 256 {
                return;
            }
            let per = |time: Duration| time.as_secs_f64() * 1e6 / sums.tiles as f64;
            let mut ops: Vec<_> = sums.ops.iter().collect();
            ops.sort_by_key(|(_, (_, time))| std::cmp::Reverse(*time));
            let ops: Vec<String> = ops
                .iter()
                .map(|(kind, (count, time))| {
                    format!(
                        "{kind} {:.0}us x{:.1}",
                        per(*time),
                        *count as f64 / sums.tiles as f64
                    )
                })
                .collect();
            tracing::warn!(
                "vello profile: {} tiles, {:.0}us a tile: render {:.0}, ops: {}; glyphs {:.0} drawn of {:.0} a tile",
                sums.tiles,
                per(sums.total),
                per(sums.render),
                ops.join(", "),
                sums.glyphs.1 as f64 / sums.tiles as f64,
                sums.glyphs.0 as f64 / sums.tiles as f64,
            );
        });
    }
}

fn op_name(op: &PaintOp) -> &'static str {
    match op {
        PaintOp::Save => "Save",
        PaintOp::Restore => "Restore",
        PaintOp::Translate { .. } | PaintOp::Concat { .. } | PaintOp::SetMatrix { .. } => "matrix",
        PaintOp::ClipRect { .. } => "ClipRect",
        PaintOp::ClipRRect { .. } => "ClipRRect",
        PaintOp::ClipPath { .. } | PaintOp::ClipPaths { .. } => "ClipPath",
        PaintOp::SaveLayerAlpha { .. } | PaintOp::SaveLayer { .. } => "SaveLayer",
        PaintOp::DrawColor { .. } => "DrawColor",
        PaintOp::DrawRect { .. } | PaintOp::ClearRect { .. } => "DrawRect",
        PaintOp::DrawRRect { .. } => "DrawRRect",
        PaintOp::DrawDRRect { .. } => "DrawDRRect",
        PaintOp::DrawLine { .. } => "DrawLine",
        PaintOp::DrawClipped { .. } => "DrawClipped",
        PaintOp::DrawShadow { .. } => "DrawShadow",
        PaintOp::DrawPath { .. } => "DrawPath",
        PaintOp::DrawGlyphs { .. } => "DrawGlyphs",
        PaintOp::DrawImage { .. } => "DrawImage",
        PaintOp::WritePixels { .. } => "WritePixels",
        PaintOp::DrawRecord { .. } => "DrawRecord",
    }
}

/// What a `Save` remembers, and what was pushed on the context since: a
/// `Restore` pops those, last first.
struct Save {
    transform: Affine,
    pushed: Vec<Pushed>,
}

enum Pushed {
    /// A clip wholly outside the context: not pushed on it, and nothing
    /// is drawn until it is popped.
    Hidden,
    /// A clip, with what it takes to push it again (a canvas's context is
    /// rendered and reset where pixels are written into it).
    Clip {
        path: BezPath,
        transform: Affine,
        fill: Fill,
        anti_alias: bool,
    },
    Layer,
}

struct Player<'a> {
    context: &'a mut RenderContext,
    resources: &'a mut Resources,
    cache: &'a VelloCache,
    /// What `SetMatrix` is relative to.
    base: Affine,
    /// The current matrix: Vello's layers and clips don't keep it.
    transform: Affine,
    saves: Vec<Save>,
    /// How many clips in force are wholly outside the context: while any
    /// is, nothing is drawn.
    hidden: usize,
    /// Whether the flags set for the op being drawn pushed a clip (a
    /// pattern that draws nothing past its tile), which resetting them
    /// pops.
    decal: bool,
    /// A canvas's pixels, which what was drawn is rendered over at the
    /// end, and where pixels are written into them.
    canvas: Option<&'a mut [u8]>,
    /// Glyphs in the runs drawn, and those handed to Vello.
    glyphs: (usize, usize),
    /// Each op's time, if they are timed.
    timings: Option<Vec<(&'static str, Duration)>>,
}

fn color(c: Color) -> peniko::Color {
    peniko::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

fn point(p: PointF) -> Point {
    Point::new(f64::from(p.x), f64::from(p.y))
}

fn rect(r: RectF) -> Rect {
    Rect::new(
        f64::from(r.x),
        f64::from(r.y),
        f64::from(r.x) + f64::from(r.width),
        f64::from(r.y) + f64::from(r.height),
    )
}

fn affine(t: &gfx::Transform) -> Affine {
    Affine::new(t.0.map(f64::from))
}

/// How much `transform` enlarges lengths, along the axis it enlarges most.
fn scale_of(transform: Affine) -> f64 {
    let [a, b, c, d, ..] = transform.as_coeffs();
    (a * a + b * b).sqrt().max((c * c + d * d).sqrt())
}

fn polygon_path(polygon: &[PointF]) -> BezPath {
    let mut path = BezPath::new();
    if let Some(first) = polygon.first() {
        path.move_to(point(*first));
        for p in &polygon[1..] {
            path.line_to(point(*p));
        }
        path.close_path();
    }
    path
}

/// A rounded rect's outline added to `path`: the one `gfx::Path` makes
/// (Skia's, each corner a conic), so that a rounded clip, a rounded fill
/// and a path of the same shape have the same edge.
fn rrect_path(rrect: &RRectF, path: &mut BezPath) {
    let mut outline = gfx::Path::new();
    outline.add_round_rect(rrect.rect, rrect.radii, true);
    path.extend(bez_path(&outline));
}

fn rrect(rrect: &RRectF) -> BezPath {
    let mut path = BezPath::new();
    rrect_path(rrect, &mut path);
    path
}

/// A `gfx::Path` as kurbo's: a segment after a close (or first) starts
/// where the last subpath did, as Skia has it; a conic is cut in sixteen
/// and each part drawn as a quadratic (four left an arc scaled up a
/// hundred times off by most of a pixel).
fn bez_path(path: &gfx::Path) -> BezPath {
    let mut out = BezPath::new();
    let mut start = Point::ZERO;
    let mut current = Point::ZERO;
    let mut open = false;
    for element in path.elements() {
        if !matches!(element, PathElement::MoveTo(_) | PathElement::Close) && !open {
            out.move_to(start);
            current = start;
            open = true;
        }
        match *element {
            PathElement::MoveTo(p) => {
                start = point(p);
                current = start;
                out.move_to(start);
                open = true;
            }
            PathElement::LineTo(p) => {
                current = point(p);
                out.line_to(current);
            }
            PathElement::QuadTo(c, p) => {
                current = point(p);
                out.quad_to(point(c), current);
            }
            PathElement::CubicTo(c1, c2, p) => {
                current = point(p);
                out.curve_to(point(c1), point(c2), current);
            }
            PathElement::ConicTo(c, p, weight) => {
                conic(&mut out, current, point(c), point(p), f64::from(weight), 4);
                current = point(p);
            }
            PathElement::Close => {
                if open {
                    out.close_path();
                }
                current = start;
                open = false;
            }
        }
    }
    out
}

/// A conic from `p0` by `p1` to `p2` of `weight`, halved `depth` times.
fn conic(out: &mut BezPath, p0: Point, p1: Point, p2: Point, weight: f64, depth: u32) {
    if depth == 0 || !weight.is_finite() || weight <= 0.0 {
        out.quad_to(p1, p2);
        return;
    }
    let scale = 1.0 / (1.0 + weight);
    let weighted = p1.to_vec2() * weight;
    let left = ((p0.to_vec2() + weighted) * scale).to_point();
    let right = ((weighted + p2.to_vec2()) * scale).to_point();
    let middle = ((p0.to_vec2() + weighted * 2.0 + p2.to_vec2()) * (scale * 0.5)).to_point();
    let half_weight = ((1.0 + weight) * 0.5).sqrt();
    conic(out, p0, left, middle, half_weight, depth.saturating_sub(1));
    conic(out, middle, right, p2, half_weight, depth.saturating_sub(1));
}

fn fill_rule(path: &gfx::Path) -> Fill {
    match path.fill_rule() {
        FillRule::NonZero => Fill::NonZero,
        FillRule::EvenOdd => Fill::EvenOdd,
    }
}

fn blend_mode(mode: BlendMode) -> peniko::BlendMode {
    let compose = |compose| peniko::BlendMode::new(Mix::Normal, compose);
    let mix = |mix| peniko::BlendMode::new(mix, Compose::SrcOver);
    match mode {
        BlendMode::Clear => compose(Compose::Clear),
        BlendMode::Src => compose(Compose::Copy),
        BlendMode::SrcOver => compose(Compose::SrcOver),
        BlendMode::DstOver => compose(Compose::DestOver),
        BlendMode::SrcIn => compose(Compose::SrcIn),
        BlendMode::DstIn => compose(Compose::DestIn),
        BlendMode::SrcOut => compose(Compose::SrcOut),
        BlendMode::DstOut => compose(Compose::DestOut),
        BlendMode::SrcATop => compose(Compose::SrcAtop),
        BlendMode::DstATop => compose(Compose::DestAtop),
        BlendMode::Xor => compose(Compose::Xor),
        BlendMode::Plus => compose(Compose::Plus),
        BlendMode::Multiply => mix(Mix::Multiply),
        BlendMode::Screen => mix(Mix::Screen),
        BlendMode::Overlay => mix(Mix::Overlay),
        BlendMode::Darken => mix(Mix::Darken),
        BlendMode::Lighten => mix(Mix::Lighten),
        BlendMode::ColorDodge => mix(Mix::ColorDodge),
        BlendMode::ColorBurn => mix(Mix::ColorBurn),
        BlendMode::HardLight => mix(Mix::HardLight),
        BlendMode::SoftLight => mix(Mix::SoftLight),
        BlendMode::Difference => mix(Mix::Difference),
        BlendMode::Exclusion => mix(Mix::Exclusion),
        BlendMode::Hue => mix(Mix::Hue),
        BlendMode::Saturation => mix(Mix::Saturation),
        BlendMode::Color => mix(Mix::Color),
        BlendMode::Luminosity => mix(Mix::Luminosity),
    }
}

fn gradient(shader: &GradientShader) -> Gradient {
    // A hard stop at the very end (the last two stops both at offset 1, of
    // two colors: `blue 0% 50px, green 50px`; one before the end Vello
    // has right): past the end Vello pads
    // with the color it has at offset 1, which is the first of the two,
    // where Skia pads with the last. So the gradient is made twice as
    // long and its offsets halved: the hard stop is then in the middle,
    // and the last color from there on. (Not one that repeats, whose
    // period that would change; not a sweep, which ends where it began.)
    let last_two = shader.stops.last_chunk::<2>();
    let stretch = shader.tile_mode == TileMode::Clamp
        && !matches!(shader.kind, GradientShaderKind::Sweep { .. })
        && last_two
            .is_some_and(|[a, b]| a.offset == b.offset && b.offset >= 1.0 && a.color != b.color);
    let twice = |from: PointF, to: PointF| {
        let (from, to) = (point(from), point(to));
        if stretch {
            from + (to - from) * 2.0
        } else {
            to
        }
    };
    let mut gradient = match shader.kind {
        GradientShaderKind::Linear { start, end } => {
            Gradient::new_linear(point(start), twice(start, end))
        }
        GradientShaderKind::TwoPointConical {
            start,
            start_radius,
            end,
            end_radius,
        } => Gradient::new_two_point_radial(
            point(start),
            start_radius,
            twice(start, end),
            if stretch {
                start_radius + (end_radius - start_radius) * 2.0
            } else {
                end_radius
            },
        ),
        GradientShaderKind::Sweep {
            center,
            start_angle,
            end_angle,
        } => Gradient::new_sweep(
            point(center),
            start_angle.to_radians(),
            end_angle.to_radians(),
        ),
    };
    gradient.extend = match shader.tile_mode {
        TileMode::Clamp => Extend::Pad,
        TileMode::Repeat => Extend::Repeat,
        TileMode::Mirror => Extend::Reflect,
    };
    gradient.interpolation_alpha_space = match shader.interpolation {
        ColorInterpolation::Premultiplied => InterpolationAlphaSpace::Premultiplied,
        ColorInterpolation::Unpremultiplied => InterpolationAlphaSpace::Unpremultiplied,
    };
    let mut stops: Vec<peniko::ColorStop> = shader
        .stops
        .iter()
        .map(|stop| {
            let offset = if stretch {
                stop.offset * 0.5
            } else {
                stop.offset
            };
            peniko::ColorStop::from((offset, color(stop.color)))
        })
        .collect();
    if let (true, Some(last)) = (stretch, shader.stops.last()) {
        stops.push(peniko::ColorStop::from((1.0, color(last.color))));
    }
    gradient.with_stops(&stops[..])
}

fn quality(filter: FilterQuality) -> ImageQuality {
    match filter {
        FilterQuality::None => ImageQuality::Low,
        // No mipmaps yet: scaled down, `Medium` is as `Low`.
        FilterQuality::Low | FilterQuality::Medium => ImageQuality::Medium,
        FilterQuality::High => ImageQuality::High,
    }
}

/// The pixels of `pixmap` that `src` covers (whole ones) as a pixmap of
/// their own, and `src` in it; `None` if it covers none.
fn cropped(pixmap: &Arc<Pixmap>, src: RectF) -> Option<(Arc<Pixmap>, RectF)> {
    let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
    let x0 = base::numerics::clamp_floor::<u32>(src.x).min(width);
    let y0 = base::numerics::clamp_floor::<u32>(src.y).min(height);
    let x1 = clamp_ceil::<u32>(src.x + src.width).min(width);
    let y1 = clamp_ceil::<u32>(src.y + src.height).min(height);
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    if (x0, y0, x1, y1) == (0, 0, width, height) {
        return Some((pixmap.clone(), src));
    }
    let data = pixmap.data_as_u8_slice();
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity((x1 - x0) as usize * (y1 - y0) as usize * 4);
    for y in y0..y1 {
        let row = y as usize * stride;
        out.extend_from_slice(&data[row + x0 as usize * 4..row + x1 as usize * 4]);
    }
    let cut = premultiplied_pixmap(
        out,
        saturated_cast::<u16, _>(x1 - x0),
        saturated_cast::<u16, _>(y1 - y0),
    );
    let within = RectF::new(src.x - x0 as f32, src.y - y0 as f32, src.width, src.height);
    Some((Arc::new(cut), within))
}

/// `pixmap` as a paint, each pixel where it is (not smoothed, not repeated).
fn pixmap_paint(pixmap: Pixmap, alpha: f32) -> Image {
    Image {
        image: ImageSource::Pixmap(Arc::new(pixmap)),
        sampler: ImageSampler {
            x_extend: Extend::Pad,
            y_extend: Extend::Pad,
            quality: ImageQuality::Low,
            alpha,
        },
    }
}

/// How far `filter` reaches past what it filters, in the space it is in
/// (cc's `MapRect`: three sigmas of a blur).
fn reach(filter: &ImageFilter) -> f32 {
    match filter {
        ImageFilter::Blur { sigma } => 3.0 * sigma,
        ImageFilter::DropShadow { offset, sigma, .. } => {
            offset.0.abs().max(offset.1.abs()) + 3.0 * sigma
        }
        ImageFilter::ColorMatrix(_) => 0.0,
        ImageFilter::Chain(filters) => filters.iter().map(reach).sum(),
    }
}

/// The pixels a context is enlarged by on each side for what reaches
/// `reach` device pixels past a tile; no more than a tile's side or so.
fn pad_for(reach: f64) -> u16 {
    clamp_ceil::<u16>(reach.max(0.0) + 1.0).min(1024)
}

/// `pixmap` through `filter`, which is in a space `to_device` maps to the
/// pixmap's: the filters Vello runs on a layer of its own, a color matrix
/// here, a chain one after the other.
fn filtered(cache: &VelloCache, pixmap: Pixmap, filter: &ImageFilter, to_device: Affine) -> Pixmap {
    let scale = scale_of(to_device);
    let sigma = |sigma: f32| f64::from(sigma) * scale;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a sigma or an offset in device pixels, far inside an f32's range"
    )]
    let primitive = match filter {
        ImageFilter::ColorMatrix(matrix) => {
            let (width, height) = (pixmap.width(), pixmap.height());
            let mut pixels = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
            color_matrix(&mut pixels, matrix);
            return premultiplied_pixmap(pixels, width, height);
        }
        ImageFilter::Chain(filters) => {
            return filters.iter().fold(pixmap, |pixmap, filter| {
                filtered(cache, pixmap, filter, to_device)
            });
        }
        ImageFilter::Blur { sigma: s } => {
            FilterPrimitive::gaussian_blur(sigma(*s) as f32, EdgeMode::None)
        }
        ImageFilter::DropShadow {
            offset,
            sigma: s,
            color: c,
            shadow_only,
        } => {
            // The offset is a vector: turned and scaled, not moved.
            let [a, b, c2, d, ..] = to_device.as_coeffs();
            let (x, y) = (f64::from(offset.0), f64::from(offset.1));
            let dx = (a * x + c2 * y) as f32;
            let dy = (b * x + d * y) as f32;
            let std_deviation = sigma(*s) as f32;
            if *shadow_only {
                FilterPrimitive::DropShadowOnly {
                    dx,
                    dy,
                    std_deviation,
                    color: color(*c),
                    edge_mode: EdgeMode::None,
                }
            } else {
                FilterPrimitive::DropShadow {
                    dx,
                    dy,
                    std_deviation,
                    color: color(*c),
                    edge_mode: EdgeMode::None,
                }
            }
        }
    };
    let (width, height) = (pixmap.width(), pixmap.height());
    render_pixmap(cache, width, height, Affine::IDENTITY, |player| {
        player
            .context
            .push_filter_layer(Filter::from_primitive(primitive));
        player.context.set_paint(pixmap_paint(pixmap, 1.0));
        player
            .context
            .fill_rect(&Rect::new(0.0, 0.0, f64::from(width), f64::from(height)));
        player.context.pop_layer();
    })
}

/// Premultiplied RGBA8 `pixels` through a 4 × 5 color matrix, as Skia's
/// matrix color filter: each color not premultiplied, from 0 to 1, the
/// last column added, the result kept in 0 to 1 and premultiplied again.
fn color_matrix(pixels: &mut [u8], m: &[f32; 20]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = f32::from(pixel[3]) / 255.0;
        let unpremultiplied = |channel: u8| {
            if alpha > 0.0 {
                (f32::from(channel) / 255.0 / alpha).min(1.0)
            } else {
                0.0
            }
        };
        let [r, g, b] = [pixel[0], pixel[1], pixel[2]].map(unpremultiplied);
        let row = |i: usize| {
            (m[i] * r + m[i + 1] * g + m[i + 2] * b + m[i + 3] * alpha + m[i + 4]).clamp(0.0, 1.0)
        };
        let out = [row(0), row(5), row(10), row(15)];
        let byte = |value: f32| saturated_cast::<u8, _>((value * 255.0).round());
        pixel[0] = byte(out[0] * out[3]);
        pixel[1] = byte(out[1] * out[3]);
        pixel[2] = byte(out[2] * out[3]);
        pixel[3] = byte(out[3]);
    }
}

/// The flags an op is drawn with, if it has any to blur it by.
fn flags_mut(op: &mut PaintOp) -> Option<&mut PaintFlags> {
    match op {
        PaintOp::DrawRect { flags, .. }
        | PaintOp::DrawRRect { flags, .. }
        | PaintOp::DrawDRRect { flags, .. }
        | PaintOp::DrawLine { flags, .. }
        | PaintOp::DrawPath { flags, .. }
        | PaintOp::DrawGlyphs { flags, .. } => Some(flags),
        PaintOp::DrawImage { flags, .. } => flags.as_mut(),
        _ => None,
    }
}

/// The blur an op's flags ask for (a mask blur), if any.
fn mask_blur(op: &PaintOp) -> Option<f32> {
    let flags = match op {
        PaintOp::DrawRect { flags, .. }
        | PaintOp::DrawRRect { flags, .. }
        | PaintOp::DrawDRRect { flags, .. }
        | PaintOp::DrawLine { flags, .. }
        | PaintOp::DrawPath { flags, .. }
        | PaintOp::DrawGlyphs { flags, .. } => flags,
        PaintOp::DrawImage { flags, .. } => flags.as_ref()?,
        _ => return None,
    };
    (flags.blur > 0.0).then_some(flags.blur)
}

/// How many of `ops` are inside the layer (or save) that the op before
/// them opened: up to its `Restore`, which is the next op if there is one
/// (a tile's ops may stop short of it).
fn inside(ops: &[&PaintOp]) -> usize {
    let mut depth = 0usize;
    for (index, op) in ops.iter().enumerate() {
        match op {
            PaintOp::Save | PaintOp::SaveLayer { .. } | PaintOp::SaveLayerAlpha { .. } => {
                depth = depth.saturating_add(1);
            }
            PaintOp::Restore => {
                if depth == 0 {
                    return index;
                }
                depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
    ops.len()
}

impl<'a> Player<'a> {
    fn new(
        context: &'a mut RenderContext,
        resources: &'a mut Resources,
        cache: &'a VelloCache,
        transform: Affine,
    ) -> Self {
        context.set_transform(transform);
        Self {
            context,
            resources,
            cache,
            base: transform,
            transform,
            saves: vec![Save {
                transform,
                pushed: Vec::new(),
            }],
            hidden: 0,
            decal: false,
            canvas: None,
            glyphs: (0, 0),
            timings: None,
        }
    }

    fn size(&self) -> (u16, u16) {
        (self.context.width(), self.context.height())
    }

    fn bounds(&self) -> Rect {
        let (width, height) = self.size();
        Rect::new(0.0, 0.0, f64::from(width), f64::from(height))
    }

    fn set_transform(&mut self, transform: Affine) {
        self.transform = transform;
        self.context.set_transform(transform);
    }

    fn top(&mut self) -> &mut Save {
        self.saves.last_mut().expect("the first save stays")
    }

    /// Pushes a clip on the context as the op that asked for it has it.
    fn push_clip_now(&mut self, path: &BezPath, fill: Fill, anti_alias: bool) {
        self.context.set_fill_rule(fill);
        self.context
            .set_aliasing_threshold((!anti_alias).then_some(128));
        self.context.push_clip_path(path);
        self.context.set_aliasing_threshold(None);
        self.context.set_fill_rule(Fill::NonZero);
    }

    /// Whether anything of `path`, in the current space, is inside the
    /// context. What isn't doesn't go to Vello: it shows nothing, and a
    /// clipped stroke that begins at the right edge makes Vello's clip
    /// intersection subtract past zero.
    fn shows(&self, path: &BezPath) -> bool {
        let seen = self.transform.transform_rect_bbox(path.bounding_box());
        let bounds = self.bounds();
        seen.x0 < bounds.x1 && seen.x1 > bounds.x0 && seen.y0 < bounds.y1 && seen.y1 > bounds.y0
    }

    /// A clip that stays until the save it is in is restored.
    fn push_clip(&mut self, path: BezPath, fill: Fill, anti_alias: bool) {
        if !self.shows(&path) {
            self.hidden += 1;
            self.top().pushed.push(Pushed::Hidden);
            return;
        }
        self.push_clip_now(&path, fill, anti_alias);
        let transform = self.transform;
        self.top().pushed.push(Pushed::Clip {
            path,
            transform,
            fill,
            anti_alias,
        });
    }

    fn pop(&mut self, pushed: &Pushed) {
        match pushed {
            Pushed::Hidden => self.hidden = self.hidden.saturating_sub(1),
            Pushed::Clip { .. } => self.context.pop_clip(),
            Pushed::Layer => self.context.pop_layer(),
        }
    }

    fn save(&mut self) {
        self.saves.push(Save {
            transform: self.transform,
            pushed: Vec::new(),
        });
    }

    fn restore(&mut self) {
        // The first save is the playback's own: a `Restore` without its
        // `Save` (a tile's ops are a part of the list) pops nothing.
        if self.saves.len() <= 1 {
            return;
        }
        let save = self.saves.pop().expect("more than one");
        for pushed in save.pushed.iter().rev() {
            self.pop(pushed);
        }
        self.set_transform(save.transform);
    }

    /// Pops what is left, and for a canvas renders over its pixels.
    fn finish(&mut self) {
        while let Some(save) = self.saves.pop() {
            for pushed in save.pushed.iter().rev() {
                self.pop(pushed);
            }
        }
        self.render_canvas();
    }

    /// Draws the canvas's pixels as they are, first thing on the context:
    /// what is drawn after blends with them inside the scene, as on
    /// Skia's canvas over the bitmap (a blend mode that takes away, like
    /// a clear, needs what it takes from to be there). A copy of them a
    /// time; Skia draws on the bitmap in place.
    fn seed_canvas(&mut self) {
        let (width, height) = self.size();
        let Some(pixels) = self.canvas.as_deref() else {
            return;
        };
        let pixmap = premultiplied_pixmap(pixels.to_vec(), width, height);
        let bounds = self.bounds();
        self.context.set_transform(Affine::IDENTITY);
        self.context.set_paint_transform(Affine::IDENTITY);
        self.context.set_paint(pixmap_paint(pixmap, 1.0));
        self.context.fill_rect(&bounds);
        self.context.set_transform(self.transform);
    }

    /// Renders what was drawn so far (the canvas's pixels first) as the
    /// canvas's pixels, if this draws on one. Nothing may be pushed on the
    /// context.
    fn render_canvas(&mut self) {
        let (width, height) = self.size();
        let Some(pixels) = self.canvas.as_deref_mut() else {
            return;
        };
        let Some(target) = PixmapMut::new(width, height, pixels) else {
            return;
        };
        self.context.flush();
        self.context.render_with(
            target,
            self.resources,
            settings(TargetInit::Clear(peniko::Color::TRANSPARENT)),
        );
    }

    /// `pixels` (`size`, premultiplied RGBA8) written over the canvas's
    /// at `(x, y)` as they are: what was drawn before is rendered first,
    /// and the clips in force put back for what comes after.
    fn write_pixels(&mut self, x: u32, y: u32, size: Size, pixels: &[u8]) {
        if self.canvas.is_none() {
            skipped("WritePixels: not on a canvas");
            return;
        }
        if self
            .saves
            .iter()
            .any(|save| save.pushed.iter().any(|p| matches!(p, Pushed::Layer)))
        {
            skipped("WritePixels: inside a layer");
            return;
        }
        let clips: usize = self
            .saves
            .iter()
            .flat_map(|save| &save.pushed)
            .filter(|pushed| matches!(pushed, Pushed::Clip { .. }))
            .count();
        for _ in 0..clips {
            self.context.pop_clip();
        }
        self.render_canvas();
        let (width, height) = self.size();
        if let Some(canvas) = self.canvas.as_deref_mut() {
            let stride = usize::from(width) * 4;
            let columns = (size.width as usize).min(usize::from(width).saturating_sub(x as usize));
            for row in 0..size.height as usize {
                let target_row = y as usize + row;
                if target_row >= usize::from(height) || columns == 0 {
                    break;
                }
                let source = row * size.width as usize * 4;
                let target = target_row * stride + x as usize * 4;
                canvas[target..target + columns * 4]
                    .copy_from_slice(&pixels[source..source + columns * 4]);
            }
        }
        self.context.reset();
        self.seed_canvas();
        let saves = std::mem::take(&mut self.saves);
        for save in &saves {
            for pushed in &save.pushed {
                if let Pushed::Clip {
                    path,
                    transform,
                    fill,
                    anti_alias,
                } = pushed
                {
                    self.context.set_transform(*transform);
                    self.push_clip_now(path, *fill, *anti_alias);
                }
            }
        }
        self.saves = saves;
        self.context.set_transform(self.transform);
    }

    /// `draw` drawn off to the side, in this context's space, on a context
    /// `pad` larger on every side (what reaches into the tile from past
    /// its edge is there to filter).
    fn offscreen(&mut self, pad: u16, draw: impl FnOnce(&mut Player<'_>)) -> Option<Pixmap> {
        let (width, height) = self.size();
        let grown = |side: u16| side.checked_add(pad.checked_mul(2)?);
        let shift = Affine::translate((f64::from(pad), f64::from(pad)));
        let base = shift * self.base;
        let transform = shift * self.transform;
        Some(render_pixmap(
            self.cache,
            grown(width)?,
            grown(height)?,
            base,
            |player| {
                player.set_transform(transform);
                draw(player);
            },
        ))
    }

    /// `pixmap`, which `offscreen` made with `pad`, drawn where it was
    /// drawn from, with `alpha` and `blend`.
    fn composite(&mut self, pixmap: Pixmap, pad: u16, alpha: f32, blend: peniko::BlendMode) {
        let transform = self.transform;
        let bounds = self.bounds();
        self.context.set_transform(Affine::IDENTITY);
        self.context.set_paint(pixmap_paint(pixmap, alpha));
        self.context
            .set_paint_transform(Affine::translate((-f64::from(pad), -f64::from(pad))));
        self.context.set_blend_mode(blend);
        self.context.fill_rect(&bounds);
        self.reset_flags();
        self.context.set_transform(transform);
    }

    /// Sets the context up to draw with `flags` (but their blur, which
    /// `draw` sees to); false if they draw nothing.
    fn set_flags(&mut self, flags: &PaintFlags) -> bool {
        self.context.set_paint_transform(Affine::IDENTITY);
        // A shader takes the color's place, but not its alpha's (a
        // canvas's `globalAlpha` over a gradient or a pattern).
        let alpha = f32::from(flags.color.a) / 255.0;
        match flags.shader.as_deref() {
            None => self.context.set_paint(color(flags.color)),
            Some(PaintShader::Empty) => return false,
            Some(PaintShader::Gradient(shader)) => {
                self.context
                    .set_paint(gradient(shader).multiply_alpha(alpha));
                self.context.set_paint_transform(affine(&shader.transform));
            }
            Some(PaintShader::Tiled(tiled)) => {
                if !self.set_tiled(tiled, alpha) {
                    return false;
                }
            }
        }
        if let PaintStyle::Stroke { width } = flags.style {
            let mut stroke = Stroke::new(f64::from(width))
                .with_caps(match flags.cap {
                    LineCap::Butt => Cap::Butt,
                    LineCap::Round => Cap::Round,
                    LineCap::Square => Cap::Square,
                })
                .with_join(match flags.join {
                    LineJoin::Miter => Join::Miter,
                    LineJoin::Round => Join::Round,
                    LineJoin::Bevel => Join::Bevel,
                })
                .with_miter_limit(f64::from(flags.miter_limit));
            if let Some(dash) = &flags.dash {
                stroke = stroke.with_dashes(
                    f64::from(dash.phase()),
                    // A dash of no length is a dot under a round or
                    // square cap (Skia's); kurbo drops it, so it gets a
                    // length too small to see.
                    dash.intervals().iter().map(|&i| f64::from(i).max(1e-3)),
                );
            }
            self.context.set_stroke(stroke);
        }
        self.context.set_blend_mode(blend_mode(flags.blend_mode));
        self.context
            .set_aliasing_threshold((!flags.anti_alias).then_some(128));
        true
    }

    /// A pattern as the paint. An image, or a gradient that isn't seen to
    /// repeat, is Vello's own; a record, a gradient that repeats and tiles
    /// with gaps are drawn into a pixmap of one tile (and its gap) at the
    /// size it is seen at, which is then the image.
    fn set_tiled(&mut self, tiled: &TiledShader, alpha: f32) -> bool {
        let tile = Affine::translate((f64::from(tiled.tile.x), f64::from(tiled.tile.y)));
        let local = affine(&tiled.transform);
        let extend = |repeat| if repeat { Extend::Repeat } else { Extend::Pad };
        let gaps = tiled.spacing.width != 0.0 || tiled.spacing.height != 0.0;
        match &tiled.image {
            ShaderImage::Image(data) if !gaps => {
                let Some(pixmap) = self.cache.image(data) else {
                    return false;
                };
                let scale = Affine::scale_non_uniform(
                    f64::from(tiled.tile.width) / f64::from(pixmap.width()),
                    f64::from(tiled.tile.height) / f64::from(pixmap.height()),
                );
                self.context.set_paint(Image {
                    image: ImageSource::Pixmap(pixmap),
                    sampler: ImageSampler {
                        x_extend: extend(tiled.repeat_x),
                        y_extend: extend(tiled.repeat_y),
                        quality: quality(tiled.filter),
                        alpha,
                    },
                });
                self.context.set_paint_transform(local * tile * scale);
            }
            ShaderImage::Gradient(shader) if !gaps && !tiled.repeat_x && !tiled.repeat_y => {
                self.context
                    .set_paint(gradient(shader).multiply_alpha(alpha));
                self.context
                    .set_paint_transform(local * tile * affine(&shader.transform));
            }
            image => {
                let cell = (
                    f64::from(tiled.tile.width) + f64::from(tiled.spacing.width),
                    f64::from(tiled.tile.height) + f64::from(tiled.spacing.height),
                );
                if !(cell.0 > 0.0 && cell.1 > 0.0) {
                    return false;
                }
                let [a, b, c, d, ..] = (self.transform * local).as_coeffs();
                let seen = ((a * a + b * b).sqrt(), (c * c + d * d).sqrt());
                let side = |length: f64| clamp_ceil::<u16>(length).clamp(1, 4096);
                let (width, height) = (side(cell.0 * seen.0), side(cell.1 * seen.1));
                let scale = (f64::from(width) / cell.0, f64::from(height) / cell.1);
                let content = RectF::new(0.0, 0.0, tiled.tile.width, tiled.tile.height);
                let filter = tiled.filter;
                let pixmap = render_pixmap(
                    self.cache,
                    width,
                    height,
                    Affine::scale_non_uniform(scale.0, scale.1),
                    |player| match image {
                        ShaderImage::Image(data) => player.draw(&PaintOp::DrawImage {
                            image: data.clone(),
                            src: None,
                            dst: content,
                            filter,
                            flags: None,
                        }),
                        ShaderImage::Gradient(shader) => {
                            player.context.set_paint(gradient(shader));
                            player
                                .context
                                .set_paint_transform(affine(&shader.transform));
                            player.context.fill_rect(&rect(content));
                            player.reset_flags();
                        }
                        ShaderImage::Record { record, size } => {
                            player.draw(&PaintOp::DrawRecord {
                                record: record.clone(),
                                size: *size,
                                dst: content,
                            });
                        }
                    },
                );
                self.context.set_paint(Image {
                    image: ImageSource::Pixmap(Arc::new(pixmap)),
                    sampler: ImageSampler {
                        x_extend: extend(tiled.repeat_x),
                        y_extend: extend(tiled.repeat_y),
                        quality: ImageQuality::Medium,
                        alpha,
                    },
                });
                self.context.set_paint_transform(
                    local * tile * Affine::scale_non_uniform(1.0 / scale.0, 1.0 / scale.1),
                );
            }
        }
        // Nothing past the tile on an axis it doesn't repeat on.
        if tiled.decal && (!tiled.repeat_x || !tiled.repeat_y) {
            const FAR: f64 = 1e7;
            let t = rect(tiled.tile);
            let seen = Rect::new(
                if tiled.repeat_x { -FAR } else { t.x0 },
                if tiled.repeat_y { -FAR } else { t.y0 },
                if tiled.repeat_x { FAR } else { t.x1 },
                if tiled.repeat_y { FAR } else { t.y1 },
            );
            self.context.push_clip_path(&(local * seen.to_path(0.1)));
            self.decal = true;
        }
        true
    }

    fn reset_flags(&mut self) {
        if std::mem::take(&mut self.decal) {
            self.context.pop_clip();
        }
        self.context.set_blend_mode(peniko::BlendMode::default());
        self.context.set_aliasing_threshold(None);
        self.context.set_paint_transform(Affine::IDENTITY);
        self.context.set_fill_rule(Fill::NonZero);
    }

    fn draw_path(&mut self, path: &BezPath, fill: Fill, flags: &PaintFlags) {
        if !self.set_flags(flags) {
            return;
        }
        match flags.style {
            PaintStyle::Fill => {
                self.context.set_fill_rule(fill);
                self.context.fill_path(path);
            }
            PaintStyle::Stroke { .. } => self.context.stroke_path(path),
        }
        self.reset_flags();
    }

    /// Plays `ops`, which may be a part of a list (a tile's): a layer with
    /// a filter takes the ops up to its `Restore` with it.
    fn play(&mut self, ops: &[&PaintOp]) {
        let mut index = 0;
        while let Some(&op) = ops.get(index) {
            let started = self.timings.is_some().then(Instant::now);
            index += 1;
            match op {
                PaintOp::SaveLayer { bounds, flags } if flags.image_filter.is_some() => {
                    let body = &ops[index..];
                    let body = &body[..inside(body)];
                    index += body.len() + 1;
                    let filter = flags.image_filter.as_ref().expect("checked");
                    self.filter_layer(*bounds, flags.alpha, flags.blend_mode, filter, body);
                }
                op => self.draw(op),
            }
            if let (Some(started), Some(timings)) = (started, self.timings.as_mut()) {
                timings.push((op_name(op), started.elapsed()));
            }
        }
    }

    /// A layer with a filter: its ops drawn off to the side with room for
    /// what the filter reaches, filtered, and drawn back.
    fn filter_layer(
        &mut self,
        bounds: Option<RectF>,
        alpha: f32,
        blend: BlendMode,
        filter: &ImageFilter,
        body: &[&PaintOp],
    ) {
        if self.hidden > 0 {
            return;
        }
        let to_device = self.transform;
        let pad = pad_for(f64::from(reach(filter)) * scale_of(to_device));
        let Some(pixmap) = self.offscreen(pad, |player| {
            if let Some(bounds) = bounds {
                player.push_clip(rect(bounds).to_path(0.1), Fill::NonZero, true);
            }
            player.play(body);
        }) else {
            skipped("SaveLayer: a filter's layer too large");
            return;
        };
        let pixmap = filtered(self.cache, pixmap, filter, to_device);
        self.composite(pixmap, pad, alpha, blend_mode(blend));
    }

    /// `op`, whose flags blur it by `sigma`: drawn sharp off to the side,
    /// blurred, and drawn back with its blend mode.
    fn draw_blurred(&mut self, op: &PaintOp, sigma: f32) {
        let mut sharp = op.clone();
        let Some(flags) = flags_mut(&mut sharp) else {
            return;
        };
        let blend = std::mem::replace(&mut flags.blend_mode, BlendMode::SrcOver);
        flags.blur = 0.0;
        let to_device = self.transform;
        let pad = pad_for(f64::from(sigma) * 3.0 * scale_of(to_device));
        let Some(pixmap) = self.offscreen(pad, |player| player.draw(&sharp)) else {
            skipped("a mask blur's layer too large");
            return;
        };
        let pixmap = filtered(self.cache, pixmap, &ImageFilter::Blur { sigma }, to_device);
        self.composite(pixmap, pad, 1.0, blend_mode(blend));
    }

    fn draw(&mut self, op: &PaintOp) {
        let state = matches!(
            op,
            PaintOp::Save
                | PaintOp::Restore
                | PaintOp::Translate { .. }
                | PaintOp::Concat { .. }
                | PaintOp::SetMatrix { .. }
                | PaintOp::ClipRect { .. }
                | PaintOp::ClipRRect { .. }
                | PaintOp::ClipPath { .. }
                | PaintOp::ClipPaths { .. }
                | PaintOp::SaveLayer { .. }
                | PaintOp::SaveLayerAlpha { .. }
        );
        if self.hidden > 0 && !state {
            return;
        }
        if let Some(sigma) = mask_blur(op) {
            self.draw_blurred(op, sigma);
            return;
        }
        match op {
            PaintOp::Save => self.save(),
            PaintOp::Restore => self.restore(),
            PaintOp::Translate { dx, dy } => {
                self.set_transform(
                    self.transform * Affine::translate((f64::from(*dx), f64::from(*dy))),
                );
            }
            PaintOp::Concat { transform } => {
                self.set_transform(self.transform * affine(transform));
            }
            PaintOp::SetMatrix { transform } => {
                self.set_transform(self.base * affine(transform));
            }
            PaintOp::ClipRect {
                rect: r,
                anti_alias,
            } => {
                self.push_clip(rect(*r).to_path(0.1), Fill::NonZero, *anti_alias);
            }
            PaintOp::ClipRRect {
                rrect: r,
                anti_alias,
            } => {
                self.push_clip(rrect(r), Fill::NonZero, *anti_alias);
            }
            PaintOp::ClipPath { path, anti_alias } => {
                self.push_clip(bez_path(path), fill_rule(path), *anti_alias);
            }
            // Vello has no union of paths: each is filled into a mask the
            // size of the context, and what follows is drawn through it.
            // None is an empty mask, which clips everything away.
            PaintOp::ClipPaths { paths, anti_alias } => {
                let (width, height) = self.size();
                let transform = self.transform;
                let anti_alias = *anti_alias;
                let mask = render_pixmap(self.cache, width, height, transform, |player| {
                    for path in paths {
                        player.context.set_paint(peniko::Color::WHITE);
                        player.context.set_fill_rule(fill_rule(path));
                        player
                            .context
                            .set_aliasing_threshold((!anti_alias).then_some(128));
                        player.context.fill_path(&bez_path(path));
                        player.reset_flags();
                    }
                });
                self.context.push_mask_layer(Mask::new_alpha(&mask));
                self.top().pushed.push(Pushed::Layer);
            }
            PaintOp::SaveLayerAlpha { bounds, alpha } => {
                self.save_layer(*bounds, *alpha, None);
            }
            // One with a filter is `play`'s; here (an op drawn alone) its
            // filter has no ops to take.
            PaintOp::SaveLayer { bounds, flags } => {
                if flags.image_filter.is_some() {
                    skipped("SaveLayer: a filter outside a list");
                }
                self.save_layer(*bounds, flags.alpha, Some(blend_mode(flags.blend_mode)));
            }
            PaintOp::DrawColor {
                color: c,
                blend_mode: mode,
            } => {
                let transform = self.transform;
                let bounds = self.bounds();
                self.context.set_transform(Affine::IDENTITY);
                self.context.set_paint_transform(Affine::IDENTITY);
                self.context.set_paint(color(*c));
                self.context.set_blend_mode(blend_mode(*mode));
                self.context.fill_rect(&bounds);
                self.reset_flags();
                self.context.set_transform(transform);
            }
            PaintOp::DrawRect { rect: r, flags } => {
                if !self.set_flags(flags) {
                    return;
                }
                match flags.style {
                    PaintStyle::Fill => self.context.fill_rect(&rect(*r)),
                    PaintStyle::Stroke { .. } => self.context.stroke_rect(&rect(*r)),
                }
                self.reset_flags();
            }
            PaintOp::ClearRect { rect: r } => {
                self.context.set_paint_transform(Affine::IDENTITY);
                self.context.set_paint(peniko::Color::BLACK);
                self.context
                    .set_blend_mode(peniko::BlendMode::new(Mix::Normal, Compose::Clear));
                self.context.fill_rect(&rect(*r));
                self.reset_flags();
            }
            PaintOp::DrawRRect { rrect: r, flags } => {
                self.draw_path(&rrect(r), Fill::NonZero, flags);
            }
            PaintOp::DrawDRRect {
                outer,
                inner,
                clip,
                flags,
            } => {
                let mut ring = BezPath::new();
                rrect_path(outer, &mut ring);
                rrect_path(inner, &mut ring);
                if let Some(polygon) = clip {
                    let clip = polygon_path(polygon);
                    if self.shows(&clip) {
                        self.context.push_clip_path(&clip);
                        self.draw_path(&ring, Fill::EvenOdd, flags);
                        self.context.pop_clip();
                    }
                } else {
                    self.draw_path(&ring, Fill::EvenOdd, flags);
                }
            }
            PaintOp::DrawLine { p0, p1, flags } => {
                let mut line = BezPath::new();
                line.move_to(point(*p0));
                line.line_to(point(*p1));
                if !self.set_flags(flags) {
                    return;
                }
                if matches!(flags.style, PaintStyle::Fill) {
                    // A line has no area: Skia draws a hairline.
                    self.context.set_stroke(Stroke::new(0.0));
                }
                self.context.stroke_path(&line);
                self.reset_flags();
            }
            PaintOp::DrawClipped { polygon, op } => {
                let clip = polygon_path(polygon);
                if self.shows(&clip) {
                    self.context.push_clip_path(&clip);
                    self.draw(op);
                    self.context.pop_clip();
                }
            }
            PaintOp::DrawShadow {
                shape,
                hole,
                clip,
                inside,
                flags,
            } => self.draw_shadow(shape, hole.as_ref(), clip, *inside, flags),
            PaintOp::DrawPath { path, flags } => {
                self.draw_path(&bez_path(path), fill_rule(path), flags);
            }
            PaintOp::DrawGlyphs {
                font,
                size,
                glyphs,
                positions,
                rotations,
                flags,
                embolden,
                skew,
                variations,
                bounds: _,
            } => {
                self.draw_glyphs(
                    font, *size, glyphs, positions, rotations, flags, *embolden, *skew, variations,
                );
            }
            PaintOp::DrawImage {
                image,
                src,
                dst,
                filter,
                flags,
            } => {
                let Some(pixmap) = self.cache.image(image) else {
                    return;
                };
                // A part of the image is cut out, so that smoothing takes
                // no pixel from outside it (Skia's strict constraint): the
                // whole pixels it covers, and `src` within those.
                let (pixmap, src) = match src {
                    Some(src) => match cropped(&pixmap, *src) {
                        Some(cut) => cut,
                        None => return,
                    },
                    None => {
                        let whole = RectF::new(
                            0.0,
                            0.0,
                            f32::from(pixmap.width()),
                            f32::from(pixmap.height()),
                        );
                        (pixmap, whole)
                    }
                };
                if src.width <= 0.0 || src.height <= 0.0 {
                    return;
                }
                let scale = (
                    f64::from(dst.width) / f64::from(src.width),
                    f64::from(dst.height) / f64::from(src.height),
                );
                self.context.set_paint(Image {
                    image: ImageSource::Pixmap(pixmap),
                    sampler: ImageSampler {
                        x_extend: Extend::Pad,
                        y_extend: Extend::Pad,
                        quality: quality(*filter),
                        alpha: flags
                            .as_ref()
                            .map_or(1.0, |flags| f32::from(flags.color.a) / 255.0),
                    },
                });
                self.context.set_paint_transform(
                    Affine::translate((
                        f64::from(dst.x) - f64::from(src.x) * scale.0,
                        f64::from(dst.y) - f64::from(src.y) * scale.1,
                    )) * Affine::scale_non_uniform(scale.0, scale.1),
                );
                if let Some(flags) = flags {
                    self.context.set_blend_mode(blend_mode(flags.blend_mode));
                }
                self.context.fill_rect(&rect(*dst));
                self.reset_flags();
            }
            PaintOp::WritePixels { x, y, size, pixels } => {
                self.write_pixels(*x, *y, *size, pixels);
            }
            // Its ops in its own space, mapped onto `dst` and cut to it;
            // its `SetMatrix`es are relative to that space.
            PaintOp::DrawRecord { record, size, dst } => {
                if size.width <= 0.0 || size.height <= 0.0 {
                    return;
                }
                self.save();
                self.push_clip(rect(*dst).to_path(0.1), Fill::NonZero, true);
                self.set_transform(
                    self.transform
                        * Affine::translate((f64::from(dst.x), f64::from(dst.y)))
                        * Affine::scale_non_uniform(
                            f64::from(dst.width) / f64::from(size.width),
                            f64::from(dst.height) / f64::from(size.height),
                        ),
                );
                let base = std::mem::replace(&mut self.base, self.transform);
                let depth = self.saves.len();
                let timings = self.timings.take();
                let ops: Vec<&PaintOp> = record.ops().iter().collect();
                self.play(&ops);
                self.timings = timings;
                while self.saves.len() > depth {
                    self.restore();
                }
                self.base = base;
                self.restore();
            }
        }
    }

    fn save_layer(&mut self, bounds: Option<RectF>, alpha: f32, blend: Option<peniko::BlendMode>) {
        self.save();
        let clip = bounds.map(|bounds| rect(bounds).to_path(0.1));
        self.context
            .push_layer(clip.as_ref(), blend, Some(alpha), None, None);
        self.top().pushed.push(Pushed::Layer);
    }

    /// A box shadow: `shape`, less `hole`, blurred by the flags' blur and
    /// drawn only outside `clip`, or only inside it. A rounded rect of one
    /// round radius, without a hole, is Vello's own blurred one; any other
    /// is drawn sharp off to the side and blurred.
    fn draw_shadow(
        &mut self,
        shape: &RRectF,
        hole: Option<&RRectF>,
        clip: &RRectF,
        inside: bool,
        flags: &PaintFlags,
    ) {
        let sigma = flags.blur.max(0.0);
        let mut region = BezPath::new();
        let fill = if inside {
            rrect_path(clip, &mut region);
            Fill::NonZero
        } else {
            // Everywhere the shadow may reach, less the box.
            let reach = f64::from(sigma) * 3.0 + 1.0;
            let around = rect(shape.rect)
                .union(rect(clip.rect))
                .inflate(reach, reach);
            region.extend(around.to_path(0.1));
            rrect_path(clip, &mut region);
            Fill::EvenOdd
        };
        self.push_clip_now(&region, fill, true);
        let first = shape.radii[0];
        let round = shape
            .radii
            .iter()
            .all(|radius| *radius == first && radius.width == radius.height);
        let blend = blend_mode(flags.blend_mode);
        if hole.is_none() && round && sigma > 0.0 {
            self.context.set_paint_transform(Affine::IDENTITY);
            self.context.set_paint(color(flags.color));
            self.context.set_blend_mode(blend);
            self.context
                .fill_blurred_rounded_rect(&rect(shape.rect), first.width, sigma);
            self.reset_flags();
        } else {
            let mut outline = rrect(shape);
            if let Some(hole) = hole {
                rrect_path(hole, &mut outline);
            }
            let to_device = self.transform;
            let pad = pad_for(f64::from(sigma) * 3.0 * scale_of(to_device));
            let paint = color(flags.color);
            match self.offscreen(pad, |player| {
                player.context.set_paint(paint);
                player.context.set_fill_rule(Fill::EvenOdd);
                player.context.fill_path(&outline);
                player.reset_flags();
            }) {
                Some(mut pixmap) => {
                    if sigma > 0.0 {
                        pixmap =
                            filtered(self.cache, pixmap, &ImageFilter::Blur { sigma }, to_device);
                    }
                    self.composite(pixmap, pad, 1.0, blend);
                }
                None => skipped("DrawShadow: its layer too large"),
            }
        }
        self.context.pop_clip();
    }

    #[expect(clippy::too_many_arguments, reason = "PROTOTYPE: an op's fields")]
    fn draw_glyphs(
        &mut self,
        font: &FontData,
        size: f32,
        glyphs: &[u16],
        positions: &[PointF],
        rotations: &[f32],
        flags: &PaintFlags,
        embolden: bool,
        skew: f32,
        variations: &[gfx::FontVariation],
    ) {
        // A run is drawn for every tile it touches, most of its glyphs
        // outside each: Vello flattens a glyph's outline before it finds
        // that out, so those too far from the tile to reach it (twice the
        // size, and the stroke's width) are left out here. Skia does as
        // much with its glyph bounds.
        let to_device = self.transform;
        let reach = {
            let stroke = match flags.style {
                PaintStyle::Fill => 0.0,
                PaintStyle::Stroke { width } => f64::from(width),
            };
            (f64::from(size) * 2.0 + stroke) * scale_of(to_device)
        };
        let visible = self.bounds().inflate(reach, reach);
        let cull = !switches().no_cull;
        let near = move |position: &PointF| !cull || visible.contains(to_device * point(*position));
        let kept = positions.iter().filter(|position| near(position)).count();
        self.glyphs.0 += positions.len();
        self.glyphs.1 += kept;
        if kept == 0 || !self.set_flags(flags) {
            return;
        }
        let slant = Affine::skew(-f64::from(skew.to_radians().tan()), 0.0);
        let font = self.cache.font(font);
        let coords = normalized_coords(&font, variations);
        let stroked = matches!(flags.style, PaintStyle::Stroke { .. });
        // Each glyph's origin moved to
        // the nearest of a few places a device pixel, as Skia's glyph
        // cache has it (a quarter along the line, whole pixels across it),
        // where the matrix only scales and moves. Vello draws a glyph
        // exactly where it is put, so the same text a hair apart (a test
        // and its reference) differs by a level at an edge.
        let snap = switches().snap;
        let [a, b, c, d, e, f] = to_device.as_coeffs();
        let aligned = b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a position in the layer's space, far inside an f32's range"
        )]
        let glyph = move |(&id, position): (&u16, &PointF)| {
            let (mut x, mut y) = (position.x, position.y);
            if snap > 0.0 && aligned {
                let device_x = f64::from(x) * a + e;
                let device_y = f64::from(y) * d + f;
                x = (((device_x * snap).round() / snap - e) / a) as f32;
                y = ((device_y.round() - f) / d) as f32;
            }
            vello_cpu::Glyph {
                id: u32::from(id),
                x,
                y,
            }
        };
        let pairs = glyphs.iter().zip(positions.iter());
        let mut failed = false;
        if rotations.is_empty() {
            let run = glyph_run(
                self.context,
                self.resources,
                &font,
                size,
                &coords,
                embolden,
                (skew != 0.0).then_some(slant),
            );
            let glyphs = pairs.filter(|(_, position)| near(position)).map(glyph);
            failed = if stroked {
                run.stroke_glyphs(glyphs).is_err()
            } else {
                run.fill_glyphs(glyphs).is_err()
            };
        } else {
            // Each glyph turned about its origin (SVG's `rotate`): a run
            // of its own, as a run has one transform for its glyphs.
            let turns = rotations.iter().chain(std::iter::repeat(&0.0));
            for (pair, degrees) in pairs.zip(turns).filter(|((_, p), _)| near(p)) {
                let turn = Affine::rotate(f64::from(degrees.to_radians())) * slant;
                let run = glyph_run(
                    self.context,
                    self.resources,
                    &font,
                    size,
                    &coords,
                    embolden,
                    Some(turn),
                );
                let one = std::iter::once(glyph(pair));
                failed |= if stroked {
                    run.stroke_glyphs(one).is_err()
                } else {
                    run.fill_glyphs(one).is_err()
                };
            }
        }
        if failed {
            skipped("DrawGlyphs: a glyph Vello can't draw");
        }
        self.reset_flags();
    }
}

/// A run of `font`'s glyphs at `size`, not hinted (their positions come
/// from layout: nothing moves them), each through `glyph_transform`.
fn glyph_run<'a>(
    context: &'a mut RenderContext,
    resources: &'a mut Resources,
    font: &peniko::FontData,
    size: f32,
    coords: &'a [i16],
    embolden: bool,
    glyph_transform: Option<Affine>,
) -> vello_cpu::GlyphRunBuilder<'a> {
    let mut run = context
        .glyph_run(resources, font)
        .font_size(size)
        .hint(false)
        .normalized_coords(coords);
    if let Some(transform) = glyph_transform {
        run = run.glyph_transform(transform);
    }
    if embolden {
        // Skia's fake bold: an outline wider by a 24th of the size at
        // 9 px, a 32nd at 36 px, half on each side.
        let t = ((size - 9.0) / (36.0 - 9.0)).clamp(0.0, 1.0);
        let extra = size * (1.0 / 24.0 + t * (1.0 / 32.0 - 1.0 / 24.0));
        let side = f64::from(extra) * 0.5;
        run = run.font_embolden(glifo::FontEmbolden::new(kurbo::Diagonal2::new(side, side)));
    }
    run
}

/// A variable font's axis values as the normalized coordinates Vello draws
/// with; none for a font that isn't one, or can't be read.
fn normalized_coords(font: &peniko::FontData, variations: &[gfx::FontVariation]) -> Vec<i16> {
    use skrifa::MetadataProvider;
    if variations.is_empty() {
        return Vec::new();
    }
    let Ok(font) = skrifa::FontRef::from_index(font.data.as_ref(), font.index) else {
        return Vec::new();
    };
    let settings = variations
        .iter()
        .map(|variation| (skrifa::Tag::new(&variation.tag), variation.value));
    font.axes()
        .location(settings)
        .coords()
        .iter()
        .map(|coord| coord.to_bits())
        .collect()
}

/// PROTOTYPE: what the environment asks of it, read once a thread (the
/// process's environment is one for every renderer in it, but this is
/// not for main).
#[derive(Clone, Copy)]
pub(crate) struct Switches {
    /// `MACHOPO_RASTER=vello`: tiles and canvases are drawn by Vello, not
    /// Skia.
    pub vello: bool,
    /// `MACHOPO_VELLO_NO_CULL=1`: every glyph of a run is handed to Vello,
    /// as the prototype first did; to measure what leaving out the far
    /// ones saves.
    no_cull: bool,
    /// `MACHOPO_VELLO_PROFILE=1`: where the tiles' time goes, in the log.
    profile: bool,
    /// `MACHOPO_VELLO_QUALITY=1`: Vello's f32 pipeline, slower and more
    /// exact, in place of its u8 one; to see what it changes.
    quality: bool,
    /// `MACHOPO_VELLO_SNAP=<n>`: glyphs' origins moved to the nearest of
    /// `n` places a device pixel along the line (and whole pixels across
    /// it); 4 unless set, as Skia's glyph cache has them; 0: where they
    /// are.
    snap: f64,
}

pub(crate) fn switches() -> Switches {
    thread_local! {
        static SWITCHES: std::cell::Cell<Option<Switches>> = const { std::cell::Cell::new(None) };
    }
    SWITCHES.with(|cell| {
        cell.get().unwrap_or_else(|| {
            let is = |name: &str, value: &str| std::env::var(name).is_ok_and(|v| v == value);
            let switches = Switches {
                vello: is("MACHOPO_RASTER", "vello"),
                no_cull: is("MACHOPO_VELLO_NO_CULL", "1"),
                profile: is("MACHOPO_VELLO_PROFILE", "1"),
                quality: is("MACHOPO_VELLO_QUALITY", "1"),
                snap: std::env::var("MACHOPO_VELLO_SNAP")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(4.0),
            };
            cell.set(Some(switches));
            switches
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stroke with butt caps that begins exactly at the context's right
    /// edge, clipped to a polygon that begins there too, as `draw` hands
    /// Vello nothing like (`shows`). `Some(true)` if Vello panicked.
    fn clipped_stroke_at_the_edge(width: u16, clipped_out: bool) -> bool {
        std::panic::catch_unwind(move || {
            let mut context = RenderContext::new(width, 256);
            let edge = f64::from(width);
            let mut clip = BezPath::new();
            clip.move_to((edge + 3.0, -3.0));
            clip.line_to((edge + 3.0, 11.0));
            clip.line_to((edge, 8.0));
            clip.line_to((edge, 0.0));
            clip.close_path();
            let mut line = BezPath::new();
            line.move_to((edge + 1.5, -3.0));
            line.line_to((edge + 1.5, 11.0));
            let mut player_context = RenderContext::new(width, 256);
            let mut resources = Resources::new();
            let cache = VelloCache::default();
            if clipped_out {
                // Through the player: the clip doesn't show, so nothing
                // is handed to Vello.
                let player = Player::new(
                    &mut player_context,
                    &mut resources,
                    &cache,
                    Affine::IDENTITY,
                );
                assert!(!player.shows(&clip));
                return;
            }
            context.push_clip_path(&clip);
            context.set_paint(peniko::Color::BLACK);
            context.set_stroke(Stroke::new(3.0).with_caps(Cap::Butt).with_join(Join::Miter));
            context.stroke_path(&line);
            context.pop_clip();
        })
        .is_err()
    }

    /// Upstream's bug, at the commit pinned: Vello's clip intersection
    /// subtracts past zero (`vello_common::clip`, `cur_strip_fill_area`)
    /// for a clipped stroke wholly past the right edge and beginning at
    /// it: a panic with overflow checks, a wrong width without. A border's
    /// side in the tile beside it is one. When this fails, upstream fixed
    /// it; `shows` keeps such clips from Vello either way.
    #[test]
    fn upstream_panics_on_a_clipped_stroke_at_the_right_edge() {
        assert!(clipped_stroke_at_the_edge(32, false));
        assert!(clipped_stroke_at_the_edge(256, false));
        assert!(!clipped_stroke_at_the_edge(32, true));
    }
}
