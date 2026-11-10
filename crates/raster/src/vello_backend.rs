//! The raster backend: tiles and canvases drawn by vello_cpu (Linebender's
//! CPU renderer; our fork's, which follows upstream's main).
//!
//! Each `PaintOp` is drawn on a `RenderContext`, one per raster thread,
//! then rendered into the pixels. What Vello has no call for is made up
//! here from what it has: filters it doesn't run and filters at a tile's
//! edge (the layer drawn off to the side, in a context larger than the
//! tile), a clip to a union of paths (a mask), shadows and blurs of any
//! shape, patterns of records or with gaps. Each such place has a comment
//! that begins `vello:`, saying what Vello lacks or does wrong.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "kurbo's operators on f64 points, vectors and matrices (its `Add`, `Mul` \
              and the like, which the lint takes for integers'), and f64 arithmetic: \
              none overflows"
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use base::numerics::{clamp_ceil, clamp_floor, clamp_round, f32_from_f64, saturated_cast};
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
    Resources, TargetInit,
};

use crate::PlaybackParams;

/// What playback keeps between tiles: fonts as Vello takes them, and
/// decoded images with the smaller versions of them that were drawn.
#[derive(Default)]
pub(crate) struct VelloCache {
    tables: Mutex<Tables>,
    /// How many images were decoded.
    #[cfg(test)]
    decodes: std::sync::atomic::AtomicUsize,
}

#[derive(Default)]
struct Tables {
    /// By font id and face index: Vello keys what it keeps for a font on
    /// the id of the blob it was handed, so each font is one blob.
    fonts: HashMap<(u64, u32), peniko::FontData>,
    /// By image id, each with a weak handle on its bytes: an entry goes
    /// when a new one is added after its data did. Filled once, outside
    /// the lock.
    images: HashMap<u64, (ImageBytes, ImageSlot)>,
}

type ImageBytes = Weak<dyn AsRef<[u8]> + Send + Sync>;
type ImageSlot = Arc<Decoded>;

/// An image's pixels, and those of it made smaller, by level (the first
/// is level 1, half the size): each made once, when first drawn, and
/// kept as long as the image (cc's `SoftwareImageDecodeCache` keeps an
/// entry of its own for each, within a budget; all the levels of an
/// image together are a third of it).
#[derive(Default)]
struct Decoded {
    pixels: OnceLock<Option<Arc<Pixmap>>>,
    /// A side is a `u16`: 16 halvings make it 1.
    levels: [OnceLock<Arc<Pixmap>>; 16],
}

/// A side of `side` pixels at `level`: halved that many times, rounded
/// up (cc's `MipMapUtil::GetSizeForLevel`).
fn level_side(side: u16, level: u32) -> u16 {
    let rounding = 1u32
        .checked_shl(level)
        .map_or(u32::MAX, |one| one.saturating_sub(1));
    let side = u32::from(side)
        .saturating_add(rounding)
        .checked_shr(level)
        .unwrap_or(0);
    saturated_cast::<u16, _>(side.max(1))
}

/// The level an image of `size` is drawn from to be `target` pixels: the
/// smallest that is no smaller than that either way, so that it is only
/// ever made smaller (cc's `MipMapUtil::GetLevelForSize`).
fn level_for(size: (u16, u16), target: (u16, u16)) -> u32 {
    let mut level = 0u32;
    loop {
        let next = level.saturating_add(1);
        let (width, height) = (level_side(size.0, next), level_side(size.1, next));
        if width < target.0 || height < target.1 {
            return level;
        }
        if (level_side(size.0, level), level_side(size.1, level)) == (1, 1) {
            return level;
        }
        level = next;
    }
}

/// For each of `to` pixels along a side of `from`, the first pixel it
/// covers and how much of it each of those is: `from / to` pixels'
/// worth, the ends in part.
fn coverage(from: u16, to: u16) -> Vec<(usize, Vec<f32>)> {
    let scale = f64::from(from) / f64::from(to);
    (0..to)
        .map(|index| {
            let low = f64::from(index) * scale;
            let high = (f64::from(index) + 1.0) * scale;
            let first = clamp_floor::<u16>(low).min(from.saturating_sub(1));
            let last = clamp_ceil::<u16>(high).clamp(first.saturating_add(1), from);
            let weights = (first..last)
                .map(|pixel| {
                    let pixel = f64::from(pixel);
                    let covered = (high.min(pixel + 1.0) - low.max(pixel)).max(0.0);
                    f32_from_f64(covered / scale)
                })
                .collect();
            (usize::from(first), weights)
        })
        .collect()
}

/// `pixmap` made `width` by `height`, each pixel the average of those it
/// covers (premultiplied, as they are).
///
/// vello: no mipmaps. A smaller image is sampled from four pixels of the
/// whole one, whatever it leaves out.
fn shrunk(pixmap: &Pixmap, width: u16, height: u16) -> Pixmap {
    let columns = coverage(pixmap.width(), width);
    let rows = coverage(pixmap.height(), height);
    let data = pixmap.data_as_u8_slice();
    let stride = usize::from(pixmap.width()) * 4;
    let mut out = Vec::with_capacity(usize::from(width) * usize::from(height) * 4);
    let mut sums = vec![0.0f32; usize::from(width) * 4];
    for (first_row, row_weights) in &rows {
        sums.fill(0.0);
        for (row, row_weight) in data[first_row * stride..]
            .chunks_exact(stride)
            .zip(row_weights)
        {
            for ((first, weights), sum) in columns.iter().zip(sums.as_chunks_mut::<4>().0) {
                let mut pixel_sum = [0.0f32; 4];
                for (pixel, weight) in row[first * 4..].as_chunks::<4>().0.iter().zip(weights) {
                    for (channel, value) in pixel_sum.iter_mut().zip(pixel) {
                        *channel += f32::from(*value) * weight;
                    }
                }
                for (channel, value) in sum.iter_mut().zip(pixel_sum) {
                    *channel += value * row_weight;
                }
            }
        }
        out.extend(sums.iter().map(|&sum| clamp_round::<u8>(sum)));
    }
    premultiplied_pixmap(out, width, height)
}

impl VelloCache {
    fn tables(&self) -> std::sync::MutexGuard<'_, Tables> {
        self.tables
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// How many fonts it holds.
    pub(crate) fn font_count(&self) -> usize {
        self.tables().fonts.len()
    }

    /// How many images it holds (decoded, or found not to be images).
    pub(crate) fn image_count(&self) -> usize {
        self.tables().images.len()
    }

    #[cfg(test)]
    pub(crate) fn has_image(&self, id: u64) -> bool {
        self.tables().images.contains_key(&id)
    }

    #[cfg(test)]
    pub(crate) fn decodes(&self) -> usize {
        self.decodes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// The size `data` is drawn from (decoding it if it isn't yet).
    #[cfg(test)]
    pub(crate) fn image_dimensions(&self, data: &ImageData) -> Option<(u16, u16)> {
        self.image(data)
            .map(|pixmap| (pixmap.width(), pixmap.height()))
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

    /// `data`'s pixels to draw it from with `filter`, `scale` device
    /// pixels to each of its own: the image, or what of it is the level
    /// that size takes if `filter` has one (cc's decode cache scales an
    /// image to its level and draws that with `kLow`); and how many of
    /// its pixels are one of the image's, each way.
    fn image_at(
        &self,
        data: &ImageData,
        filter: FilterQuality,
        scale: (f64, f64),
    ) -> Option<(Arc<Pixmap>, (f64, f64))> {
        let (slot, image) = self.decoded(data)?;
        let size = (image.width(), image.height());
        // A canvas's images (`Low`) are sampled as they are, and so is
        // one whose pixels are to show (`None`).
        let level = match filter {
            FilterQuality::None | FilterQuality::Low => 0,
            FilterQuality::Medium | FilterQuality::High => {
                let target = |side: u16, scale: f64| {
                    clamp_round::<u16>(f64::from(side) * scale.abs()).max(1)
                };
                level_for(size, (target(size.0, scale.0), target(size.1, scale.1)))
            }
        };
        let Some(made) = level
            .checked_sub(1)
            .and_then(|index| slot.levels.get(usize::try_from(index).ok()?))
        else {
            return Some((image, (1.0, 1.0)));
        };
        let (width, height) = (level_side(size.0, level), level_side(size.1, level));
        let small = made
            .get_or_init(|| Arc::new(shrunk(&image, width, height)))
            .clone();
        let ratio = (
            f64::from(width) / f64::from(size.0),
            f64::from(height) / f64::from(size.1),
        );
        Some((small, ratio))
    }

    fn image(&self, data: &ImageData) -> Option<Arc<Pixmap>> {
        self.decoded(data).map(|(_, image)| image)
    }

    fn decoded(&self, data: &ImageData) -> Option<(ImageSlot, Arc<Pixmap>)> {
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
        let image = slot
            .pixels
            .get_or_init(|| {
                let (size, pixels) = match data.decoded {
                    Some(size) => (size, data.as_bytes().to_vec()),
                    None => {
                        #[cfg(test)]
                        #[expect(
                            clippy::disallowed_methods,
                            reason = "a count a test reads: it never gets near wrapping"
                        )]
                        let _ = self
                            .decodes
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let decoded = crate::image_decode::decode(data.as_bytes())?;
                        (decoded.size, decoded.pixels)
                    }
                };
                let width = u16::try_from(size.width).ok()?;
                let height = u16::try_from(size.height).ok()?;
                Some(Arc::new(premultiplied_pixmap(pixels, width, height)))
            })
            .clone()?;
        Some((slot, image))
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
    /// How many times this thread left each thing out.
    static SKIPPED: RefCell<HashMap<&'static str, usize>> = RefCell::new(HashMap::new());
}

/// Something that isn't drawn (a layer too large to draw off to the side,
/// a glyph Vello can't draw): counted, and logged when the count doubles,
/// so that a page that keeps asking for it doesn't fill the log.
fn skipped(what: &'static str) {
    SKIPPED.with_borrow_mut(|counts| {
        let count = counts.entry(what).or_insert(0);
        *count = count.saturating_add(1);
        if count.is_power_of_two() {
            tracing::warn!(what, count = *count, "raster: not drawn");
        }
    });
}

/// Plays `ops` of `list` back into `pixels` (`size`, RGBA8 premultiplied),
/// cleared first, as `params` says.
pub(crate) fn playback(
    list: &DisplayItemList,
    ops: &[usize],
    params: &PlaybackParams,
    cache: &VelloCache,
    size: Size,
    pixels: &mut [u8],
) {
    // A context's side is a `u16`.
    let (Ok(width), Ok(height)) = (u16::try_from(size.width), u16::try_from(size.height)) else {
        skipped("pixels with a side over 65535");
        return;
    };
    CONTEXT.with_borrow_mut(|slot| {
        let (context, resources) =
            slot.get_or_insert_with(|| (RenderContext::new(width, height), Resources::new()));
        context.reset_and_resize(width, height);
        let scale = f64::from(params.contents_scale);
        let base = Affine::translate((
            -f64::from(params.content_rect.x),
            -f64::from(params.content_rect.y),
        )) * Affine::scale(scale);
        let mut player = Player::new(context, resources, cache, base);
        let all = list.ops();
        let ops: Vec<&PaintOp> = ops.iter().map(|&op| &all[op]).collect();
        player.play(&ops);
        player.finish();
        let Some(target) = PixmapMut::new(width, height, pixels) else {
            return;
        };
        context.flush();
        context.render_with(
            target,
            resources,
            settings(TargetInit::Clear(color(params.clear_color))),
        );
    });
}

/// Plays `list` back onto a canvas's `pixels` (`size`), over what they
/// have; an `opaque` one's alpha stays 255.
pub(crate) fn draw_into(
    list: &DisplayItemList,
    cache: &VelloCache,
    size: Size,
    opaque: bool,
    pixels: &mut [u8],
) {
    // A context's side is a `u16`.
    let (Ok(width), Ok(height)) = (u16::try_from(size.width), u16::try_from(size.height)) else {
        skipped("pixels with a side over 65535");
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

/// How a context is rendered: over `target_init`, with Vello's u8
/// pipeline (its f32 one made five of WPT's reftests pass that are off by
/// a level in a gradient or a blur, and took 1.3 to 2 times as long).
fn settings(target_init: TargetInit) -> RasterizerSettings {
    RasterizerSettings {
        target_init,
        ..RasterizerSettings::default()
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

/// How much `transform` enlarges lengths along x and along y.
fn axis_scales(transform: Affine) -> (f64, f64) {
    let [a, b, c, d, ..] = transform.as_coeffs();
    ((a * a + b * b).sqrt(), (c * c + d * d).sqrt())
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
    // vello: pads past a hard stop at the end with the color before it.
    // A hard stop at the very end (the last two stops both at offset 1, of
    // two colors: `blue 0% 50px, green 50px`; one before the end Vello
    // has right): past the end Vello pads with the color it has at offset
    // 1, which is the first of the two, where Skia pads with the last
    // (and CSS means the last). So the gradient is made twice as
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
        // `Medium` is drawn from a smaller level of the image
        // (`VelloCache::image_at`), and so is `High` made smaller.
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
        // vello: no color matrix filter (it panics on one); done on the
        // layer's pixels here.
        ImageFilter::ColorMatrix(matrix) => {
            let (width, height) = (pixmap.width(), pixmap.height());
            let mut pixels = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
            color_matrix(&mut pixels, matrix);
            return premultiplied_pixmap(pixels, width, height);
        }
        // vello: no filter of several primitives (it panics on one); each
        // is run on the last one's pixels.
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
    /// context. What isn't doesn't go to Vello: it shows nothing.
    ///
    /// vello: a clipped stroke that begins at the right edge makes its
    /// clip intersection subtract past zero (a panic, or a wrong width).
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

    /// vello: a scene's blend modes see what is in the scene, not what
    /// the pixels it is rendered onto have.
    ///
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
    ///
    /// vello: no pattern of a recording, none with gaps between its tiles
    /// or that stops after one tile along an axis.
    fn set_tiled(&mut self, tiled: &TiledShader, alpha: f32) -> bool {
        let tile = Affine::translate((f64::from(tiled.tile.x), f64::from(tiled.tile.y)));
        let local = affine(&tiled.transform);
        let extend = |repeat| if repeat { Extend::Repeat } else { Extend::Pad };
        let gaps = tiled.spacing.width != 0.0 || tiled.spacing.height != 0.0;
        match &tiled.image {
            ShaderImage::Image(data) if !gaps => {
                let Some(whole) = self.cache.image(data) else {
                    return false;
                };
                let seen = axis_scales(self.transform * local);
                let seen = (
                    seen.0 * f64::from(tiled.tile.width) / f64::from(whole.width()),
                    seen.1 * f64::from(tiled.tile.height) / f64::from(whole.height()),
                );
                let Some((pixmap, _)) = self.cache.image_at(data, tiled.filter, seen) else {
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
        }
    }

    /// A layer with a filter: its ops drawn off to the side with room for
    /// what the filter reaches, filtered, and drawn back.
    ///
    /// vello: a filter layer sees only what is inside the context, so a
    /// blur at a tile's edge would miss what the next tile has.
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
    ///
    /// vello: no blur on a paint (Skia's mask filter), only of a rounded
    /// rect of one radius.
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
            // vello: no union of paths. Each is filled into a mask the
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
                let Some(whole) = self.cache.image(image) else {
                    return;
                };
                let drawn = src.map_or(
                    (f32::from(whole.width()), f32::from(whole.height())),
                    |src| (src.width, src.height),
                );
                if drawn.0 <= 0.0 || drawn.1 <= 0.0 {
                    return;
                }
                let seen = axis_scales(self.transform);
                let seen = (
                    seen.0 * f64::from(dst.width) / f64::from(drawn.0),
                    seen.1 * f64::from(dst.height) / f64::from(drawn.1),
                );
                let Some((pixmap, ratio)) = self.cache.image_at(image, *filter, seen) else {
                    return;
                };
                // `src` is in the image's pixels: in the level's, it is
                // as much smaller as the level is.
                let src = src.map(|src| {
                    let (x, y) = (f32_from_f64(ratio.0), f32_from_f64(ratio.1));
                    RectF::new(src.x * x, src.y * y, src.width * x, src.height * y)
                });
                let src = &src;
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
                let ops: Vec<&PaintOp> = record.ops().iter().collect();
                self.play(&ops);
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
    ///
    /// vello: a blurred rounded rect has one radius for its four corners,
    /// both ways, and no hole.
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

    #[expect(clippy::too_many_arguments, reason = "an op's fields")]
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
        // vello: glyphs outside the context cost as much as those inside.
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
        let near = move |position: &PointF| visible.contains(to_device * point(*position));
        if !positions.iter().any(near) || !self.set_flags(flags) {
            return;
        }
        let slant = crate::ink::slant(skew);
        let font = self.cache.font(font);
        let coords = normalized_coords(&font, variations);
        let stroked = matches!(flags.style, PaintStyle::Stroke { .. });
        // vello: draws a glyph exactly where it is put.
        // Each glyph's origin moved to the nearest of a few places a
        // device pixel, as Skia's glyph cache has it (a quarter along the
        // line, whole pixels across it), where the matrix only scales and
        // moves. Vello draws a glyph exactly where it is put, so the same
        // text a hair apart (a reftest and its reference, whose positions
        // differ in the last bits of a float) would differ by a level at
        // an edge. (A sixty-fourth did worse: the more places, the likelier
        // a hair's difference straddles two.)
        const SNAP: f64 = 4.0;
        let [a, b, c, d, e, f] = to_device.as_coeffs();
        let aligned = b == 0.0 && c == 0.0 && a > 0.0 && d > 0.0;
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a position in the layer's space, far inside an f32's range"
        )]
        let glyph = move |(&id, position): (&u16, &PointF)| {
            let (mut x, mut y) = (position.x, position.y);
            if aligned {
                let device_x = f64::from(x) * a + e;
                let device_y = f64::from(y) * d + f;
                x = (((device_x * SNAP).round() / SNAP - e) / a) as f32;
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
        // With `FontEmbolden`'s own joins, which `ink_bounds` measures
        // with too.
        let side = crate::ink::embolden_side(size);
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

    /// The level an image is drawn from is cc's: the smallest whose sides
    /// are both no smaller than what is drawn, each side halved and
    /// rounded up a level.
    #[test]
    fn an_image_is_drawn_from_the_smallest_level_that_is_large_enough() {
        assert_eq!(level_side(100, 0), 100);
        assert_eq!(level_side(100, 1), 50);
        assert_eq!(level_side(101, 1), 51);
        assert_eq!(level_side(5, 2), 2);
        assert_eq!(level_side(5, 3), 1);
        assert_eq!(level_side(5, 9), 1);
        assert_eq!(level_side(65535, 16), 1);
        assert_eq!(level_side(65535, 40), 1);
        // As large, larger, or more than half: the image itself.
        assert_eq!(level_for((100, 100), (100, 100)), 0);
        assert_eq!(level_for((100, 100), (300, 300)), 0);
        assert_eq!(level_for((100, 100), (51, 51)), 0);
        // Half or less: the level that is half; a quarter: the next.
        assert_eq!(level_for((100, 100), (50, 50)), 1);
        assert_eq!(level_for((100, 100), (26, 26)), 1);
        assert_eq!(level_for((100, 100), (25, 25)), 2);
        // The side that is made smaller the least decides.
        assert_eq!(level_for((100, 100), (10, 50)), 1);
        assert_eq!(level_for((100, 100), (60, 10)), 0);
        // A side rounded up is large enough for one more pixel.
        assert_eq!(level_for((101, 101), (51, 51)), 1);
        // Down to one pixel, and no further.
        assert_eq!(level_for((8, 1), (1, 1)), 3);
        assert_eq!(level_for((1, 1), (1, 1)), 0);
        assert_eq!(level_for((65535, 65535), (1, 1)), 16);
    }

    /// Made smaller, a pixel is the average of those it covers, the ones
    /// at its ends by the part of them it covers: 3 pixels into 2 are the
    /// first and half the second, half the second and the third.
    #[test]
    fn a_smaller_level_averages_what_each_pixel_covers() {
        let image = |width: u16, height: u16, pixels: &[[u8; 4]]| {
            premultiplied_pixmap(pixels.concat(), width, height)
        };
        let gray = |v: u8| [v, v, v, 255];
        let bytes = |pixmap: Pixmap| pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
        let row = image(3, 1, &[gray(0), gray(90), gray(240)]);
        assert_eq!(bytes(shrunk(&row, 2, 1)), [gray(30), gray(190)].concat());
        let column = image(1, 3, &[gray(0), gray(90), gray(240)]);
        assert_eq!(bytes(shrunk(&column, 1, 2)), [gray(30), gray(190)].concat());
        // Both ways at once, each channel on its own, alpha too.
        let square = image(
            2,
            2,
            &[
                [200, 0, 0, 200],
                [0, 100, 0, 100],
                [0, 0, 40, 40],
                [0, 0, 0, 0],
            ],
        );
        assert_eq!(bytes(shrunk(&square, 1, 1)), [50, 25, 10, 85]);
        // A whole number of pixels each: no part of another.
        let four = image(4, 1, &[gray(10), gray(30), gray(100), gray(200)]);
        assert_eq!(bytes(shrunk(&four, 2, 1)), [gray(20), gray(150)].concat());
        // The same size: the same pixels.
        assert_eq!(bytes(shrunk(&four, 4, 1)), bytes(four));
    }

    /// A level is made once, when it is first drawn from, and only those
    /// drawn from are made; `Low` and `None` draw from the image itself.
    #[test]
    fn a_level_is_made_once_and_only_when_drawn_from() {
        let cache = VelloCache::default();
        let data = ImageData::decoded(Size::new(16, 16), vec![255; 16 * 16 * 4]);
        let made = |cache: &VelloCache| {
            let tables = cache.tables();
            let (_, slot) = &tables.images[&data.id];
            slot.levels
                .iter()
                .map(|level| level.get().map(|pixmap| pixmap.width()))
                .collect::<Vec<_>>()
        };
        let (whole, ratio) = cache
            .image_at(&data, FilterQuality::Medium, (1.0, 1.0))
            .unwrap();
        assert_eq!((whole.width(), ratio), (16, (1.0, 1.0)));
        assert!(made(&cache).iter().all(Option::is_none));
        for filter in [FilterQuality::None, FilterQuality::Low] {
            let (image, _) = cache.image_at(&data, filter, (0.1, 0.1)).unwrap();
            assert!(Arc::ptr_eq(&image, &whole));
        }
        assert!(made(&cache).iter().all(Option::is_none));
        // A quarter the size: level 2, and not level 1.
        let (quarter, ratio) = cache
            .image_at(&data, FilterQuality::Medium, (0.25, 0.25))
            .unwrap();
        assert_eq!(
            (quarter.width(), quarter.height(), ratio),
            (4, 4, (0.25, 0.25))
        );
        let (again, _) = cache
            .image_at(&data, FilterQuality::High, (0.2, 0.25))
            .unwrap();
        assert!(Arc::ptr_eq(&again, &quarter));
        let levels = made(&cache);
        assert_eq!(&levels[..3], [None, Some(4), None]);
        // Turned over (a negative scale) is as small.
        let (half, _) = cache
            .image_at(&data, FilterQuality::Medium, (-0.5, 0.5))
            .unwrap();
        assert_eq!(half.width(), 8);
        // With the image go its levels.
        drop(data);
        let other = ImageData::decoded(Size::new(1, 1), vec![0; 4]);
        let _ = cache.image(&other);
        assert_eq!(cache.image_count(), 1);
    }

    /// A rect on a layer with `filter`, rendered by Vello alone.
    fn render_filtered(filter: Filter) {
        let mut context = RenderContext::new(16, 16);
        let mut resources = Resources::new();
        context.push_filter_layer(filter);
        context.set_paint(peniko::Color::BLACK);
        context.fill_rect(&Rect::new(2.0, 2.0, 10.0, 10.0));
        context.pop_layer();
        let mut pixmap = Pixmap::new(16, 16);
        context.flush();
        context.render_with(
            &mut pixmap,
            &mut resources,
            settings(TargetInit::Clear(peniko::Color::TRANSPARENT)),
        );
    }

    /// What upstream lacks, at the commit pinned: a color matrix (CSS's
    /// `grayscale()`, `invert()` and the like), which `filtered` does on
    /// the layer's pixels. When this stops panicking, Vello has it.
    #[test]
    #[should_panic(expected = "Other filter primitives not yet implemented")]
    fn upstream_lacks_color_matrix_filters() {
        let mut matrix = [0.0; 20];
        for channel in 0..4 {
            matrix[channel * 6] = 1.0;
        }
        render_filtered(Filter::from_primitive(FilterPrimitive::ColorMatrix {
            matrix,
        }));
    }

    /// What upstream lacks, at the commit pinned: a filter of more than
    /// one primitive (`blur(2px) drop-shadow(...)`), which `filtered` runs
    /// one after the other, each from the last one's pixels. When this
    /// stops panicking, Vello has it.
    #[test]
    #[should_panic(expected = "Multi-primitive filter graphs are not yet supported")]
    fn upstream_lacks_filters_of_several_primitives() {
        use vello_cpu::filter_effects::FilterGraph;
        let mut graph = FilterGraph::new();
        let _ = graph.add(FilterPrimitive::gaussian_blur(2.0, EdgeMode::None), None);
        let last = graph.add(FilterPrimitive::Offset { dx: 1.0, dy: 1.0 }, None);
        graph.set_output(last);
        render_filtered(Filter {
            graph: Arc::new(graph),
        });
    }

    /// Upstream's bug, at the commit pinned: past the end of a gradient
    /// whose last two stops are both at offset 1, Vello pads with the
    /// first of the two (red here) where the second (blue) is meant.
    /// `gradient` makes such a gradient twice as long. When this fails,
    /// upstream fixed it.
    #[test]
    fn upstream_pads_a_gradient_past_a_hard_stop_with_the_color_before_it() {
        let (red, blue) = (
            peniko::Color::from_rgb8(255, 0, 0),
            peniko::Color::from_rgb8(0, 0, 255),
        );
        let stops = [
            peniko::ColorStop::from((0.0, red)),
            peniko::ColorStop::from((1.0, red)),
            peniko::ColorStop::from((1.0, blue)),
        ];
        let mut gradient = Gradient::new_linear((0.0, 0.0), (10.0, 0.0)).with_stops(&stops[..]);
        gradient.extend = Extend::Pad;
        let mut context = RenderContext::new(32, 4);
        let mut resources = Resources::new();
        context.set_paint(gradient);
        context.fill_rect(&Rect::new(0.0, 0.0, 32.0, 4.0));
        let mut pixmap = Pixmap::new(32, 4);
        context.flush();
        context.render_with(
            &mut pixmap,
            &mut resources,
            settings(TargetInit::Clear(peniko::Color::TRANSPARENT)),
        );
        let pixels = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
        assert_eq!(pixels[(32 + 25) * 4..][..4], [255, 0, 0, 255]);
    }
}
