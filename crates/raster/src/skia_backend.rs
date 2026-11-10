//! skia-safe backend: each `PaintOp` maps to one `SkCanvas` call, exactly as
//! `cc::PaintOpBuffer::Playback` does in Chromium.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use base::numerics::saturated_cast;
use compositor_paint::{
    BlendMode, ColorInterpolation, DisplayItemList, FilterQuality, GradientShader,
    GradientShaderKind, ImageFilter, LayerFlags, LineCap, LineJoin, PaintFlags, PaintOp,
    PaintShader, PaintStyle, ShaderImage, TileMode, TiledShader,
};
use gfx::{Color, FillRule, FontData, GlyphRun, ImageData, PathElement, RectF, Size, Transform};
use read_fonts::FileRef;
use skia_safe as sk;

use crate::PlaybackParams;

pub const NAME: &str = "skia";

fn sk_color(c: Color) -> sk::Color {
    sk::Color::from_argb(c.a, c.r, c.g, c.b)
}

fn sk_rect(r: RectF) -> sk::Rect {
    sk::Rect::from_xywh(r.x, r.y, r.width, r.height)
}

fn sk_rrect(r: &gfx::RRectF) -> sk::RRect {
    let radii = r
        .radii
        .map(|radius| sk::Vector::new(radius.width, radius.height));
    sk::RRect::new_rect_radii(sk_rect(r.rect), &radii)
}

/// A closed polygon.
fn polygon_path(polygon: &[gfx::PointF]) -> sk::Path {
    let mut builder = sk::PathBuilder::new();
    if let Some(first) = polygon.first() {
        builder.move_to((first.x, first.y));
        for p in &polygon[1..] {
            builder.line_to((p.x, p.y));
        }
        builder.close();
    }
    builder.detach()
}

/// A path as Skia's, element for element: Skia injects the moves a
/// segment after a close (or first) leaves out, as `gfx::Path` says.
fn sk_path(path: &gfx::Path) -> sk::Path {
    let mut builder = sk::PathBuilder::new();
    builder.set_fill_type(match path.fill_rule() {
        FillRule::NonZero => sk::PathFillType::Winding,
        FillRule::EvenOdd => sk::PathFillType::EvenOdd,
    });
    let pt = |p: gfx::PointF| sk::Point::new(p.x, p.y);
    for element in path.elements() {
        match *element {
            PathElement::MoveTo(p) => {
                builder.move_to(pt(p));
            }
            PathElement::LineTo(p) => {
                builder.line_to(pt(p));
            }
            PathElement::QuadTo(c, p) => {
                builder.quad_to(pt(c), pt(p));
            }
            PathElement::CubicTo(c1, c2, p) => {
                builder.cubic_to(pt(c1), pt(c2), pt(p));
            }
            PathElement::ConicTo(c, p, weight) => {
                builder.conic_to(pt(c), pt(p), weight);
            }
            PathElement::Close => {
                builder.close();
            }
        }
    }
    builder.detach()
}

fn sk_matrix(t: &Transform) -> sk::Matrix {
    let [a, b, c, d, e, f] = t.0;
    sk::Matrix::new_all(a, c, e, b, d, f, 0.0, 0.0, 1.0)
}

fn sk_paint(flags: &PaintFlags, cache: &RasterCache) -> sk::Paint {
    let mut paint = sk::Paint::default();
    paint.set_color(sk_color(flags.color));
    paint.set_anti_alias(flags.anti_alias);
    if let Some(shader) = &flags.shader {
        match sk_shader(shader, cache) {
            Some(shader) => {
                paint.set_shader(shader);
            }
            // An image that won't decode draws nothing, as `DrawImage`'s.
            None => {
                paint.set_color(sk::Color::TRANSPARENT);
            }
        }
    }
    match flags.style {
        PaintStyle::Fill => {
            paint.set_style(sk::PaintStyle::Fill);
        }
        PaintStyle::Stroke { width } => {
            paint.set_style(sk::PaintStyle::Stroke);
            paint.set_stroke_width(width);
        }
    }
    paint.set_stroke_cap(match flags.cap {
        LineCap::Butt => sk::PaintCap::Butt,
        LineCap::Round => sk::PaintCap::Round,
        LineCap::Square => sk::PaintCap::Square,
    });
    paint.set_stroke_join(match flags.join {
        LineJoin::Miter => sk::PaintJoin::Miter,
        LineJoin::Round => sk::PaintJoin::Round,
        LineJoin::Bevel => sk::PaintJoin::Bevel,
    });
    paint.set_stroke_miter(flags.miter_limit);
    // In the current space, so it scales with the content (cc's blur with
    // `respectCTM`).
    if let Some(dash) = &flags.dash {
        paint.set_path_effect(sk::PathEffect::dash(dash.intervals(), dash.phase()));
    }
    if flags.blur > 0.0 {
        paint.set_mask_filter(sk::MaskFilter::blur(
            sk::BlurStyle::Normal,
            flags.blur,
            true,
        ));
    }
    paint.set_blend_mode(sk_blend_mode(flags.blend_mode));
    paint
}

fn sk_blend_mode(mode: BlendMode) -> sk::BlendMode {
    match mode {
        BlendMode::Clear => sk::BlendMode::Clear,
        BlendMode::Src => sk::BlendMode::Src,
        BlendMode::SrcOver => sk::BlendMode::SrcOver,
        BlendMode::DstOver => sk::BlendMode::DstOver,
        BlendMode::SrcIn => sk::BlendMode::SrcIn,
        BlendMode::DstIn => sk::BlendMode::DstIn,
        BlendMode::SrcOut => sk::BlendMode::SrcOut,
        BlendMode::DstOut => sk::BlendMode::DstOut,
        BlendMode::SrcATop => sk::BlendMode::SrcATop,
        BlendMode::DstATop => sk::BlendMode::DstATop,
        BlendMode::Xor => sk::BlendMode::Xor,
        BlendMode::Plus => sk::BlendMode::Plus,
        BlendMode::Multiply => sk::BlendMode::Multiply,
        BlendMode::Screen => sk::BlendMode::Screen,
        BlendMode::Overlay => sk::BlendMode::Overlay,
        BlendMode::Darken => sk::BlendMode::Darken,
        BlendMode::Lighten => sk::BlendMode::Lighten,
        BlendMode::ColorDodge => sk::BlendMode::ColorDodge,
        BlendMode::ColorBurn => sk::BlendMode::ColorBurn,
        BlendMode::HardLight => sk::BlendMode::HardLight,
        BlendMode::SoftLight => sk::BlendMode::SoftLight,
        BlendMode::Difference => sk::BlendMode::Difference,
        BlendMode::Exclusion => sk::BlendMode::Exclusion,
        BlendMode::Hue => sk::BlendMode::Hue,
        BlendMode::Saturation => sk::BlendMode::Saturation,
        BlendMode::Color => sk::BlendMode::Color,
        BlendMode::Luminosity => sk::BlendMode::Luminosity,
    }
}

/// `filter` as Skia's (cc's `DropShadowPaintFilter` is
/// `SkImageFilters::DropShadow` or `DropShadowOnly`).
fn sk_image_filter(filter: &ImageFilter) -> Option<sk::ImageFilter> {
    sk_image_filter_on(filter, None)
}

/// `filter` on what `input` makes (on what was drawn if `None`).
fn sk_image_filter_on(
    filter: &ImageFilter,
    input: Option<sk::ImageFilter>,
) -> Option<sk::ImageFilter> {
    match filter {
        &ImageFilter::DropShadow {
            offset,
            sigma,
            color,
            shadow_only,
        } => {
            let color = sk::Color4f::from(sk_color(color));
            if shadow_only {
                sk::image_filters::drop_shadow_only(
                    offset,
                    (sigma, sigma),
                    color,
                    None,
                    input,
                    None,
                )
            } else {
                sk::image_filters::drop_shadow(offset, (sigma, sigma), color, None, input, None)
            }
        }
        &ImageFilter::Blur { sigma } => sk::image_filters::blur((sigma, sigma), None, input, None),
        ImageFilter::ColorMatrix(matrix) => sk::image_filters::color_filter(
            sk::color_filters::matrix_row_major(matrix, None),
            input,
            None,
        ),
        ImageFilter::Chain(filters) => filters
            .iter()
            .try_fold(input, |input, filter| {
                sk_image_filter_on(filter, input).map(Some)
            })
            .flatten(),
    }
}

/// The paint a layer is composited with when it is restored.
fn layer_paint(flags: &LayerFlags) -> sk::Paint {
    let mut paint = sk::Paint::default();
    paint.set_alpha_f(flags.alpha);
    paint.set_blend_mode(sk_blend_mode(flags.blend_mode));
    if let Some(filter) = &flags.image_filter {
        paint.set_image_filter(sk_image_filter(filter));
    }
    paint
}

/// Chromium's sampling for `filter`: bilinear, with mipmaps when scaling
/// down for an `<img>` (`image-rendering: auto`), without for a canvas.
fn image_sampling(filter: FilterQuality, upscale: bool) -> sk::SamplingOptions {
    match filter {
        FilterQuality::None => {
            sk::SamplingOptions::new(sk::FilterMode::Nearest, sk::MipmapMode::None)
        }
        FilterQuality::Low => {
            sk::SamplingOptions::new(sk::FilterMode::Linear, sk::MipmapMode::None)
        }
        // cc's `kHigh` scaling up (`ScalingOperation::kUpscale`).
        FilterQuality::High if upscale => sk::SamplingOptions::from(sk::CubicResampler::mitchell()),
        FilterQuality::Medium | FilterQuality::High => {
            sk::SamplingOptions::new(sk::FilterMode::Linear, sk::MipmapMode::Linear)
        }
    }
}

/// A `PaintShader` as Skia's (cc's `PaintShader::GetSkShader`).
fn sk_shader(shader: &PaintShader, cache: &RasterCache) -> Option<sk::Shader> {
    match shader {
        PaintShader::Tiled(tiled) => tiled_shader(tiled, cache),
        PaintShader::Gradient(gradient) => gradient_shader(gradient, None),
        PaintShader::Empty => Some(sk::shaders::empty()),
    }
}

/// A tiled shader: the image or gradient drawn into its tile, repeated
/// with Skia's tile modes on the axes that repeat; on those that don't,
/// its edge extended (the op's rect stops at the tile) or, for `decal`,
/// nothing past it (a rounded background draws its whole area with it). A
/// tile with gaps is a picture of the tile and its gaps, repeated (cc's
/// `MakePaintRecord` for `ImageTilingInfo` with spacing); so is a repeated
/// or `decal` gradient, whose own tile mode goes along its line, not the
/// tile's axes.
fn tiled_shader(shader: &TiledShader, cache: &RasterCache) -> Option<sk::Shader> {
    let tile = shader.tile;
    let mode = |repeats: bool| {
        if repeats {
            sk::TileMode::Repeat
        } else if shader.decal {
            sk::TileMode::Decal
        } else {
            sk::TileMode::Clamp
        }
    };
    let modes = (mode(shader.repeat_x), mode(shader.repeat_y));
    let at_tile = sk::Matrix::concat(
        &sk_matrix(&shader.transform),
        &sk::Matrix::translate((tile.x, tile.y)),
    );
    let sampling = image_sampling(shader.filter, false);
    let spaced = shader.spacing.width > 0.0 || shader.spacing.height > 0.0;
    let image = match &shader.image {
        ShaderImage::Image(data) => Some(cache.image(data)?),
        ShaderImage::Gradient(_) | ShaderImage::Record { .. } => None,
    };
    // The image scaled into the tile, the tile's origin at 0.
    let into_tile = |image: &sk::Image| {
        sk::Matrix::scale((
            tile.width / image.width() as f32,
            tile.height / image.height() as f32,
        ))
    };
    match (&shader.image, &image) {
        (ShaderImage::Image(_), Some(image)) if !spaced => {
            let matrix = sk::Matrix::concat(&at_tile, &into_tile(image));
            image.to_shader(modes, sampling, &matrix)
        }
        (ShaderImage::Gradient(gradient), _)
            if !spaced && !shader.repeat_x && !shader.repeat_y && !shader.decal =>
        {
            gradient_shader(gradient, Some(&at_tile))
        }
        _ => {
            // One tile and its gaps, as a picture.
            let bounds = sk::Rect::from_wh(
                tile.width + shader.spacing.width,
                tile.height + shader.spacing.height,
            );
            let mut recorder = sk::PictureRecorder::new();
            let canvas = recorder.begin_recording(bounds, false);
            let rect = sk::Rect::from_wh(tile.width, tile.height);
            match (&shader.image, &image) {
                (ShaderImage::Image(_), Some(image)) => {
                    canvas.draw_image_rect_with_sampling_options(
                        image,
                        None,
                        rect,
                        sampling,
                        &sk::Paint::default(),
                    );
                }
                (ShaderImage::Gradient(gradient), _) => {
                    let mut paint = sk::Paint::default();
                    paint.set_shader(gradient_shader(gradient, None)?);
                    canvas.draw_rect(rect, &paint);
                }
                // As `DrawRecord` draws one into the tile.
                (ShaderImage::Record { record, size }, _) => {
                    if size.width <= 0.0 || size.height <= 0.0 {
                        return None;
                    }
                    canvas.clip_rect(rect, sk::ClipOp::Intersect, true);
                    canvas.scale((tile.width / size.width, tile.height / size.height));
                    let base = canvas.local_to_device_as_3x3();
                    for op in record.ops() {
                        draw_op(canvas, op, &base, cache);
                    }
                }
                _ => return None,
            }
            let picture = recorder.finish_recording_as_picture(Some(&bounds))?;
            Some(picture.to_shader(modes, sk::FilterMode::Linear, &at_tile, &bounds))
        }
    }
}

/// A gradient as Skia's: its transform after `outer` (the tile's place).
fn gradient_shader(gradient: &GradientShader, outer: Option<&sk::Matrix>) -> Option<sk::Shader> {
    use sk::gradient_shader::{Gradient, GradientColors, Interpolation, interpolation};
    let colors: Vec<sk::Color4f> = gradient
        .stops
        .iter()
        .map(|stop| sk::Color4f::from(sk_color(stop.color)))
        .collect();
    let offsets: Vec<f32> = gradient.stops.iter().map(|stop| stop.offset).collect();
    let mode = match gradient.tile_mode {
        TileMode::Clamp => sk::TileMode::Clamp,
        TileMode::Repeat => sk::TileMode::Repeat,
        TileMode::Mirror => sk::TileMode::Mirror,
    };
    let interpolation = Interpolation {
        in_premul: match gradient.interpolation {
            ColorInterpolation::Premultiplied => interpolation::InPremul::Yes,
            ColorInterpolation::Unpremultiplied => interpolation::InPremul::No,
        },
        color_space: interpolation::ColorSpace::Destination,
        hue_method: interpolation::HueMethod::Shorter,
    };
    let gradient_colors = GradientColors::new(&colors, Some(offsets.as_slice()), mode, None);
    let spec = Gradient::new(gradient_colors, interpolation);
    let own = sk_matrix(&gradient.transform);
    let local = match outer {
        Some(outer) => sk::Matrix::concat(outer, &own),
        None => own,
    };
    match gradient.kind {
        GradientShaderKind::Linear { start, end } => sk::gradient::shaders::linear_gradient(
            ((start.x, start.y), (end.x, end.y)),
            &spec,
            &local,
        ),
        GradientShaderKind::TwoPointConical {
            start,
            start_radius,
            end,
            end_radius,
        } => sk::gradient::shaders::two_point_conical_gradient(
            ((start.x, start.y), start_radius),
            ((end.x, end.y), end_radius),
            &spec,
            &local,
        ),
        GradientShaderKind::Sweep {
            center,
            start_angle,
            end_angle,
        } => sk::gradient::shaders::sweep_gradient(
            (center.x, center.y),
            (start_angle, end_angle),
            &spec,
            &local,
        ),
    }
}

/// What raster keeps between playbacks: Skia's typefaces and decoded
/// images. One per compositor (cc's `ImageDecodeCache`, which a
/// `LayerTreeHostImpl` owns; Skia's font cache is one per renderer
/// process): renderers share none of it, as they won't share a process.
/// The compositor's raster workers share it: a lock around its tables,
/// held only to look up or add an entry, not while decoding or drawing
/// (Skia's typefaces and images are thread-safe); an image's entry is
/// filled once, by the first worker to draw it, the others waiting for it,
/// so each image is decoded once.
#[derive(Default)]
pub struct RasterCache {
    tables: Mutex<CacheTables>,
    /// How many images were decoded.
    #[cfg(test)]
    decodes: std::sync::atomic::AtomicUsize,
    /// PROTOTYPE: what Vello keeps, when it draws the tiles.
    pub(crate) vello: crate::vello_backend::VelloCache,
}

/// A variable font's instance: its font id, face index and the axes'
/// values (their bits).
type InstanceKey = (u64, u32, Box<[([u8; 4], u32)]>);

#[derive(Default)]
struct CacheTables {
    /// By font id and face index. Kept as long as the compositor.
    typefaces: HashMap<(u64, u32), sk::Typeface>,
    /// A variable font's instances, by font id, face index and the axes'
    /// values (their bits). Kept as long as the compositor.
    instances: HashMap<InstanceKey, sk::Typeface>,
    /// By image id, each with a weak handle on its bytes: an entry goes
    /// once no `ImageData` for it is left (its document and display lists
    /// are gone). Filled on first draw (cc's `ImageDecodeCache` decodes
    /// for the raster that needs the image), with `None` for bytes that
    /// aren't an image; it keeps the pixels.
    images: HashMap<u64, (ImageBytes, ImageSlot)>,
}

/// An image's place in the cache: empty until the first draw fills it,
/// outside the tables' lock.
type ImageSlot = Arc<OnceLock<Option<sk::Image>>>;

/// A weak handle on an image's bytes, encoded or decoded (`gfx::ImageData`'s).
type ImageBytes = Weak<dyn AsRef<[u8]> + Send + Sync>;

impl std::fmt::Debug for RasterCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tables = self.tables();
        f.debug_struct("RasterCache")
            .field("typefaces", &tables.typefaces.len())
            .field("images", &tables.images.len())
            .finish()
    }
}

impl RasterCache {
    fn tables(&self) -> std::sync::MutexGuard<'_, CacheTables> {
        // A worker that panicked mid-insert leaves whole entries or none.
        self.tables
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn image(&self, data: &ImageData) -> Option<sk::Image> {
        // A canvas's snapshot another thread draws: waited for before the
        // lock, which that thread takes too. None if its pixels never came.
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
        // Decoding takes long: not under the lock, which every worker's
        // every image goes through. Workers after the same image wait here.
        slot.get_or_init(|| self.make_image(data)).clone()
    }

    /// `data` as a Skia image of its pixels: a canvas's snapshot's as they
    /// are, a file's decoded. `None` for a file that isn't an image.
    fn make_image(&self, data: &ImageData) -> Option<sk::Image> {
        let (size, pixels) = match data.decoded {
            // A canvas's snapshot: already pixels.
            Some(size) => (size, sk::Data::new_copy(data.as_bytes())),
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
                // Skia keeps its own copy; ours goes at the end of this.
                (decoded.size, sk::Data::new_copy(&decoded.pixels))
            }
        };
        let info = sk::ImageInfo::new(
            (
                i32::try_from(size.width).ok()?,
                i32::try_from(size.height).ok()?,
            ),
            sk::ColorType::RGBA8888,
            sk::AlphaType::Premul,
            None,
        );
        sk::images::raster_from_data(&info, pixels, info.min_row_bytes())
    }

    fn typeface(&self, font: &FontData) -> Option<sk::Typeface> {
        let key = (font.id, font.index);
        let mut tables = self.tables();
        if let Some(typeface) = tables.typefaces.get(&key) {
            return Some(typeface.clone());
        }
        let bytes = (*font.bytes).as_ref();
        let mgr = sk::FontMgr::new();
        let typeface = mgr
            .new_from_data(sk::Data::new_copy(bytes), font.index)
            .or_else(|| {
                // Skia's CoreText font manager only loads face 0 of a collection
                // (`SkTypeface_mac_ct.cpp`: "TODO: Use
                // CTFontManagerCreateFontDescriptorsFromData"); FreeType and
                // DirectWrite load any face. Hand it the face on its own.
                if font.index == 0 {
                    return None;
                }
                let face = collection_face(bytes, font.index)?;
                mgr.new_from_data(sk::Data::new_copy(&face), 0)
            })?;
        tables.typefaces.insert(key, typeface.clone());
        Some(typeface)
    }

    /// `font`'s typeface at `variations`' axis values (Blink's
    /// `FontPlatformData` makes its `SkTypeface` with them); the font's own
    /// for none.
    fn instance(&self, font: &FontData, variations: &[gfx::FontVariation]) -> Option<sk::Typeface> {
        if variations.is_empty() {
            return self.typeface(font);
        }
        let key = (
            font.id,
            font.index,
            variations
                .iter()
                .map(|v| (v.tag, v.value.to_bits()))
                .collect::<Box<[_]>>(),
        );
        if let Some(typeface) = self.tables().instances.get(&key) {
            return Some(typeface.clone());
        }
        let typeface = self.typeface(font)?;
        let coordinates: Vec<sk::font_arguments::variation_position::Coordinate> = variations
            .iter()
            .map(|v| sk::font_arguments::variation_position::Coordinate {
                axis: sk::FourByteTag::from_chars(
                    v.tag[0] as char,
                    v.tag[1] as char,
                    v.tag[2] as char,
                    v.tag[3] as char,
                ),
                value: v.value,
            })
            .collect();
        let arguments = sk::FontArguments::new().set_variation_design_position(
            sk::font_arguments::VariationPosition {
                coordinates: &coordinates,
            },
        );
        let instance = typeface
            .clone_with_arguments(&arguments)
            .unwrap_or(typeface);
        self.tables().instances.insert(key, instance.clone());
        Some(instance)
    }
}

/// The Skia font glyphs are drawn and measured with; `None` (and a
/// warning) for font data Skia can't load.
fn sk_font(
    cache: &RasterCache,
    font: &FontData,
    size: f32,
    embolden: bool,
    skew: f32,
    variations: &[gfx::FontVariation],
) -> Option<sk::Font> {
    let Some(typeface) = cache.instance(font, variations) else {
        tracing::warn!(?font, "font data Skia can't load");
        return None;
    };
    let mut sk_font = sk::Font::from_typeface(typeface, size);
    // Glyph positions come from layout: no hinting or snapping that would
    // move them.
    sk_font
        .set_edging(sk::font::Edging::AntiAlias)
        .set_hinting(sk::FontHinting::None)
        .set_subpixel(true)
        .set_linear_metrics(true)
        .set_baseline_snap(false)
        .set_embolden(embolden)
        .set_skew_x(-skew.to_radians().tan());
    Some(sk_font)
}

/// Where `runs` put ink, y down: each glyph's outline's bounds at its
/// position, made bold and slanted as it is drawn, all of them together;
/// glyphs with no ink (spaces) left out, and an empty rect at the origin
/// if none has any (Blink's `TextMetrics::MeasureRuns`). The outlines'
/// bounds are exact, as Blink's on macOS; elsewhere Blink takes Skia's
/// glyph bounds, whole pixels. A glyph with no outline (a bitmap one) has
/// those.
pub fn ink_bounds(cache: &RasterCache, runs: &[GlyphRun]) -> RectF {
    let mut ink = RectF::default();
    for run in runs {
        let Some(font) = sk_font(
            cache,
            &run.font,
            run.size,
            run.embolden,
            run.skew,
            &run.variations,
        ) else {
            continue;
        };
        for (&glyph, position) in run.glyphs.iter().zip(&run.positions) {
            let bounds = match font.get_path(glyph) {
                Some(path) => *path.bounds(),
                None => {
                    let mut bounds = [sk::Rect::default()];
                    font.get_bounds(&[glyph], &mut bounds, None);
                    bounds[0]
                }
            };
            let bounds = RectF::new(
                position.x + bounds.left,
                position.y + bounds.top,
                bounds.width(),
                bounds.height(),
            );
            if !bounds.is_empty() {
                ink = ink.union(&bounds);
            }
        }
    }
    ink
}

/// Face `index` of the font collection `ttc` as a standalone font: its table
/// directory with the offsets rewritten, followed by its tables. The tables
/// are copied unchanged, so glyph ids stay the ones layout shaped with.
fn collection_face(ttc: &[u8], index: u32) -> Option<Vec<u8>> {
    let FileRef::Collection(collection) = FileRef::new(ttc).ok()? else {
        return None;
    };
    let face = collection.get(index).ok()?;
    let dir = face.table_directory();
    let records = dir.table_records();

    let mut out = Vec::new();
    out.extend(dir.sfnt_version().to_be_bytes());
    out.extend(dir.num_tables().to_be_bytes());
    out.extend(dir.search_range().to_be_bytes());
    out.extend(dir.entry_selector().to_be_bytes());
    out.extend(dir.range_shift().to_be_bytes());
    // Tables keep their directory order and start on 4-byte boundaries.
    let mut tables = Vec::with_capacity(records.len());
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "records.len() is the directory's `num_tables`, a u16"
    )]
    let mut offset = 12 + 16 * records.len();
    for record in records {
        let start = record.offset() as usize;
        let data = ttc.get(start..start.checked_add(record.length() as usize)?)?;
        out.extend(record.tag().to_be_bytes());
        out.extend(record.checksum().to_be_bytes());
        out.extend(u32::try_from(offset).ok()?.to_be_bytes());
        out.extend(record.length().to_be_bytes());
        offset = offset.checked_add(data.len().next_multiple_of(4))?;
        tables.push(data);
    }
    for data in tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

pub fn playback(
    list: &DisplayItemList,
    ops: &[usize],
    params: &PlaybackParams,
    cache: &RasterCache,
    size: Size,
    pixels: &mut [u8],
) {
    // A tile is at most 256 px a side, a canvas 65535: past `i32::MAX`
    // the canvas below couldn't be made either way.
    let info = sk::ImageInfo::new(
        (saturated_cast(size.width), saturated_cast(size.height)),
        sk::ColorType::RGBA8888,
        sk::AlphaType::Premul,
        None,
    );
    let row_bytes = size.width as usize * 4;
    let canvas = sk::Canvas::from_raster_direct(&info, pixels, row_bytes, None)
        .expect("valid raster canvas");

    canvas.clear(sk_color(params.clear_color));
    canvas.translate((-params.content_rect.x, -params.content_rect.y));
    canvas.scale((params.contents_scale, params.contents_scale));
    // What `SetMatrix` is relative to.
    let base = canvas.local_to_device_as_3x3();

    let all = list.ops();
    for &op in ops {
        draw_op(&canvas, &all[op], &base, cache);
    }
    // Dropping the OwnedCanvas flushes into `pixels`.
    drop(canvas);
}

/// Plays `list` back onto `pixels` (`size`, RGBA8 premultiplied, tightly
/// packed), over what they have, with no transform: a canvas's bitmap,
/// which its 2D context's ops draw onto (Blink's `CanvasResourceProvider`
/// draws on a Skia surface over the canvas's pixels). An `opaque` one's
/// alpha stays 255.
pub fn draw_into(
    list: &DisplayItemList,
    cache: &RasterCache,
    size: Size,
    opaque: bool,
    pixels: &mut [u8],
) {
    let alpha = if opaque {
        sk::AlphaType::Opaque
    } else {
        sk::AlphaType::Premul
    };
    // A tile is at most 256 px a side, a canvas 65535: past `i32::MAX`
    // the canvas below couldn't be made either way.
    let info = sk::ImageInfo::new(
        (saturated_cast(size.width), saturated_cast(size.height)),
        sk::ColorType::RGBA8888,
        alpha,
        None,
    );
    let row_bytes = size.width as usize * 4;
    let canvas = sk::Canvas::from_raster_direct(&info, pixels, row_bytes, None)
        .expect("valid raster canvas");
    let base = canvas.local_to_device_as_3x3();
    for op in list.ops() {
        draw_op(&canvas, op, &base, cache);
    }
    drop(canvas);
}

/// Plays one op back on `canvas`; `base` is what `SetMatrix` is relative
/// to.
fn draw_op(canvas: &sk::Canvas, op: &PaintOp, base: &sk::Matrix, cache: &RasterCache) {
    match op {
        PaintOp::Save => {
            canvas.save();
        }
        PaintOp::Restore => {
            canvas.restore();
        }
        PaintOp::Translate { dx, dy } => {
            canvas.translate((*dx, *dy));
        }
        PaintOp::ClipRect { rect, anti_alias } => {
            canvas.clip_rect(sk_rect(*rect), sk::ClipOp::Intersect, *anti_alias);
        }
        PaintOp::ClipRRect { rrect, anti_alias } => {
            canvas.clip_rrect(sk_rrect(rrect), sk::ClipOp::Intersect, *anti_alias);
        }
        PaintOp::ClipPath { path, anti_alias } => {
            canvas.clip_path(&sk_path(path), sk::ClipOp::Intersect, *anti_alias);
        }
        PaintOp::ClipPaths { paths, anti_alias } => {
            // Joined into one, as Blink's `AsPath` does; none is an empty
            // path, which clips everything away.
            let union = paths
                .iter()
                .map(sk_path)
                .reduce(|a, b| sk::op(&a, &b, sk::PathOp::Union).unwrap_or(a))
                .unwrap_or_default();
            canvas.clip_path(&union, sk::ClipOp::Intersect, *anti_alias);
        }
        PaintOp::Concat { transform } => {
            canvas.concat(&sk_matrix(transform));
        }
        PaintOp::SetMatrix { transform } => {
            let matrix = sk::Matrix::concat(base, &sk_matrix(transform));
            canvas.set_matrix(&sk::M44::from(matrix));
        }
        PaintOp::SaveLayerAlpha { bounds, alpha } => {
            canvas.save_layer_alpha_f(bounds.map(sk_rect), *alpha);
        }
        PaintOp::SaveLayer { bounds, flags } => {
            let paint = layer_paint(flags);
            let bounds = bounds.map(sk_rect);
            let mut rec = sk::canvas::SaveLayerRec::default().paint(&paint);
            if let Some(bounds) = &bounds {
                rec = rec.bounds(bounds);
            }
            canvas.save_layer(&rec);
        }
        PaintOp::DrawColor { color, blend_mode } => {
            canvas.draw_color(sk_color(*color), sk_blend_mode(*blend_mode));
        }
        PaintOp::DrawRect { rect, flags } => {
            canvas.draw_rect(sk_rect(*rect), &sk_paint(flags, cache));
        }
        PaintOp::ClearRect { rect } => {
            let mut paint = sk::Paint::default();
            paint.set_anti_alias(true);
            paint.set_blend_mode(sk::BlendMode::Clear);
            canvas.draw_rect(sk_rect(*rect), &paint);
        }
        PaintOp::DrawRRect { rrect, flags } => {
            canvas.draw_rrect(sk_rrect(rrect), &sk_paint(flags, cache));
        }
        PaintOp::DrawDRRect {
            outer,
            inner,
            clip,
            flags,
        } => {
            let paint = sk_paint(flags, cache);
            if let Some(polygon) = clip {
                canvas.save();
                canvas.clip_path(&polygon_path(polygon), sk::ClipOp::Intersect, true);
                canvas.draw_drrect(sk_rrect(outer), sk_rrect(inner), &paint);
                canvas.restore();
            } else {
                canvas.draw_drrect(sk_rrect(outer), sk_rrect(inner), &paint);
            }
        }
        PaintOp::DrawLine { p0, p1, flags } => {
            canvas.draw_line((p0.x, p0.y), (p1.x, p1.y), &sk_paint(flags, cache));
        }
        PaintOp::DrawClipped { polygon, op } => {
            canvas.save();
            canvas.clip_path(&polygon_path(polygon), sk::ClipOp::Intersect, true);
            draw_op(canvas, op, base, cache);
            canvas.restore();
        }
        PaintOp::DrawShadow {
            shape,
            hole,
            clip,
            inside,
            flags,
        } => {
            let paint = sk_paint(flags, cache);
            let op = if *inside {
                sk::ClipOp::Intersect
            } else {
                sk::ClipOp::Difference
            };
            canvas.save();
            canvas.clip_rrect(sk_rrect(clip), op, true);
            match hole {
                Some(hole) => {
                    canvas.draw_drrect(sk_rrect(shape), sk_rrect(hole), &paint);
                }
                None => {
                    canvas.draw_rrect(sk_rrect(shape), &paint);
                }
            }
            canvas.restore();
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
            let Some(sk_font) = sk_font(cache, font, *size, *embolden, *skew, variations) else {
                return;
            };
            let paint = sk_paint(flags, cache);
            if rotations.is_empty() {
                let positions: Vec<sk::Point> =
                    positions.iter().map(|p| sk::Point::new(p.x, p.y)).collect();
                canvas.draw_glyphs_at(glyphs, &positions[..], (0.0, 0.0), &sk_font, &paint);
            } else {
                // Each glyph turned about its origin: a text blob's RSXforms
                // (Blink's `SVGInlineTextBoxPainter` builds them for
                // `rotate`).
                let xforms: Vec<sk::RSXform> = positions
                    .iter()
                    .zip(rotations.iter().chain(std::iter::repeat(&0.0)))
                    .map(|(p, degrees)| {
                        let (sin, cos) = degrees.to_radians().sin_cos();
                        sk::RSXform::new(cos, sin, (p.x, p.y))
                    })
                    .collect();
                if let Some(blob) = sk::TextBlob::from_rsxform(&glyphs[..], &xforms, &sk_font) {
                    canvas.draw_text_blob(&blob, (0.0, 0.0), &paint);
                }
            }
        }
        PaintOp::DrawPath { path, flags } => {
            canvas.draw_path(&sk_path(path), &sk_paint(flags, cache));
        }
        PaintOp::DrawImage {
            image: data,
            src,
            dst,
            filter,
            flags,
        } => {
            let Some(image) = cache.image(data) else {
                tracing::warn!(?data, "image Skia can't decode, or pixels that never came");
                return;
            };
            // A given `src` strictly (a canvas's `drawImage`, as Blink's
            // `kClampImageToSourceRect`); the whole image as it is, which
            // keeps the mipmaps that strict sampling goes without.
            let (src, constraint) = match src {
                Some(src) => (sk_rect(*src), sk::canvas::SrcRectConstraint::Strict),
                None => (
                    sk::Rect::from_wh(image.width() as f32, image.height() as f32),
                    sk::canvas::SrcRectConstraint::Fast,
                ),
            };
            // Scaling up on both axes, as the device sees it.
            let to_device = canvas.local_to_device_as_3x3();
            let scale = |along: (f32, f32)| {
                let v = to_device.map_vector(along);
                (v.x * v.x + v.y * v.y).sqrt()
            };
            let upscale =
                scale((dst.width, 0.0)) >= src.width() && scale((0.0, dst.height)) >= src.height();
            let paint = flags
                .as_ref()
                .map_or_else(sk::Paint::default, |flags| sk_paint(flags, cache));
            canvas.draw_image_rect_with_sampling_options(
                &image,
                Some((&src, constraint)),
                sk_rect(*dst),
                image_sampling(*filter, upscale),
                &paint,
            );
        }
        PaintOp::WritePixels { x, y, size, pixels } => {
            // A canvas is at most 65535 a side: its pixels' too.
            let info = sk::ImageInfo::new(
                (saturated_cast(size.width), saturated_cast(size.height)),
                sk::ColorType::RGBA8888,
                sk::AlphaType::Premul,
                None,
            );
            let row_bytes = size.width as usize * 4;
            // False only for pixels outside the bitmap, which
            // `putImageData` cuts them to.
            let _ = canvas.write_pixels(
                &info,
                pixels,
                row_bytes,
                (saturated_cast::<i32, _>(*x), saturated_cast::<i32, _>(*y)),
            );
        }
        // Its ops in its own space, mapped onto `dst` and cut to it; its
        // `SetMatrix`es are relative to that space.
        PaintOp::DrawRecord { record, size, dst } => {
            if size.width <= 0.0 || size.height <= 0.0 {
                return;
            }
            let count = canvas.save();
            canvas.clip_rect(sk_rect(*dst), sk::ClipOp::Intersect, true);
            canvas.translate((dst.x, dst.y));
            canvas.scale((dst.width / size.width, dst.height / size.height));
            let base = canvas.local_to_device_as_3x3();
            for op in record.ops() {
                draw_op(canvas, op, &base, cache);
            }
            canvas.restore_to_count(count);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use compositor_paint::DisplayItemList;
    use gfx::{PointF, Size};
    use read_fonts::{FontRef, TableProvider};

    use super::*;

    static AHEM: &[u8] = include_bytes!("../../../resources/test_fonts/Ahem.ttf");
    static LATO: &[u8] = include_bytes!("../../../resources/test_fonts/Lato-Medium.ttf");

    /// Packs single fonts into a TrueType collection: each font is copied
    /// whole, with its table offsets moved to where it lands in the file.
    fn ttc(fonts: &[&[u8]]) -> Vec<u8> {
        let header = 12 + 4 * fonts.len();
        let mut out = Vec::new();
        out.extend(b"ttcf");
        out.extend(0x0001_0000u32.to_be_bytes());
        out.extend((fonts.len() as u32).to_be_bytes());
        out.resize(header, 0);
        for (i, font) in fonts.iter().enumerate() {
            let base = out.len();
            out[12 + 4 * i..16 + 4 * i].copy_from_slice(&(base as u32).to_be_bytes());
            out.extend_from_slice(font);
            let records = FontRef::new(font)
                .unwrap()
                .table_directory()
                .table_records()
                .len();
            for r in 0..records {
                let at = base + 12 + 16 * r + 8;
                let offset = u32::from_be_bytes(out[at..at + 4].try_into().unwrap());
                out[at..at + 4].copy_from_slice(&(offset + base as u32).to_be_bytes());
            }
            out.resize(out.len().next_multiple_of(4), 0);
        }
        out
    }

    #[test]
    fn collection_face_is_a_standalone_font_with_the_same_tables() {
        let collection = ttc(&[LATO, AHEM]);
        for (index, original) in [LATO, AHEM].into_iter().enumerate() {
            let face = collection_face(&collection, index as u32).unwrap();
            let face = FontRef::new(&face).unwrap();
            let original = FontRef::new(original).unwrap();
            let records = original.table_directory().table_records();
            assert_eq!(face.table_directory().table_records().len(), records.len());
            for record in records {
                assert_eq!(
                    face.table_data(record.tag()).unwrap().as_bytes(),
                    original.table_data(record.tag()).unwrap().as_bytes(),
                    "face {index}, table {}",
                    record.tag()
                );
            }
            assert_eq!(
                face.maxp().unwrap().num_glyphs(),
                original.maxp().unwrap().num_glyphs()
            );
        }
        assert!(collection_face(&collection, 2).is_none());
        // Not a collection.
        assert!(collection_face(AHEM, 0).is_none());
    }

    /// A face other than the first of a collection loads on every platform,
    /// including macOS, where Skia's CoreText font manager refuses it.
    #[test]
    fn draws_a_face_after_the_first_of_a_collection() {
        let font = FontData {
            bytes: Arc::new(ttc(&[LATO, AHEM])),
            index: 1,
            id: 2,
        };
        let typeface = RasterCache::default()
            .typeface(&font)
            .expect("face 1 loads");
        assert_eq!(typeface.family_name(), "Ahem");
        let glyph = typeface.unichar_to_glyph('X' as i32);
        let mut list = DisplayItemList::new();
        // Same square as `ahem_glyph_is_a_crisp_square`: x, y in [2, 22).
        list.push_op(PaintOp::DrawGlyphs {
            font,
            size: 20.0,
            glyphs: vec![glyph].into(),
            positions: vec![PointF::new(2.0, 18.0)].into(),
            rotations: Vec::new().into(),
            flags: PaintFlags::fill(Color::rgb(0, 0, 0)),
            embolden: false,
            skew: 0.0,
            variations: Arc::new([]),
            bounds: RectF::new(2.0, 2.0, 20.0, 20.0),
        });
        list.finalize();
        let bmp = crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 24.0, 24.0),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        );
        assert_eq!(bmp.size, Size::new(24, 24));
        assert_eq!(bmp.pixel(12, 12), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(1, 1), [255, 255, 255, 255]);
    }

    static GREEN_BLUE: &[u8] =
        include_bytes!("../../../tests/reftests/images/resources/green-blue-20x10.png");

    fn draw(image: ImageData, dst: RectF, size: Size) -> crate::Bitmap {
        draw_with(&RasterCache::default(), image, dst, size)
    }

    fn draw_with(cache: &RasterCache, image: ImageData, dst: RectF, size: Size) -> crate::Bitmap {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawImage {
            image,
            src: None,
            dst,
            filter: compositor_paint::FilterQuality::Medium,
            flags: None,
        });
        list.finalize();
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, size.width as f32, size.height as f32),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            cache,
        )
    }

    /// A 2 × 1 decoded image, red then blue, drawn on transparent: `src`
    /// of it into `dst`, sampled with `filter`, at `alpha`.
    fn red_blue(src: Option<RectF>, dst: RectF, filter: FilterQuality, alpha: u8) -> crate::Bitmap {
        let image = ImageData::decoded(Size::new(2, 1), vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawImage {
            image,
            src,
            dst,
            filter,
            flags: Some(PaintFlags::fill(Color::rgba(0, 0, 0, alpha))),
        });
        list.finalize();
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 8.0, 4.0),
                contents_scale: 1.0,
                clear_color: Color::TRANSPARENT,
            },
            &RasterCache::default(),
        )
    }

    /// A canvas's `drawImage`: its source rect never sampled past
    /// (strictly), its alpha, and each filter: the nearest pixel,
    /// bilinear, and cubic scaling up.
    #[test]
    fn an_images_source_rect_alpha_and_filter() {
        // The blue half, scaled up: no red bleeds in at its edge.
        let blue = red_blue(
            Some(RectF::new(1.0, 0.0, 1.0, 1.0)),
            RectF::new(0.0, 0.0, 4.0, 4.0),
            FilterQuality::Low,
            255,
        );
        assert_eq!(blue.pixel(0, 0), [0, 0, 255, 255]);
        // Half transparent.
        let faint = red_blue(
            None,
            RectF::new(0.0, 0.0, 2.0, 1.0),
            FilterQuality::Low,
            128,
        );
        let [r, _, _, a] = faint.pixel(0, 0);
        assert!(
            (126..=130).contains(&a) && (126..=130).contains(&r),
            "{:?}",
            faint.pixel(0, 0)
        );
        // Eight times wider: nearest keeps red to the middle, bilinear
        // mixes there, cubic mixes otherwise.
        let wide = |filter| red_blue(None, RectF::new(0.0, 0.0, 8.0, 1.0), filter, 255);
        let nearest = wide(FilterQuality::None);
        assert_eq!(nearest.pixel(3, 0), [255, 0, 0, 255]);
        assert_eq!(nearest.pixel(4, 0), [0, 0, 255, 255]);
        let bilinear = wide(FilterQuality::Low).pixel(3, 0);
        assert!(bilinear[0] < 250 && bilinear[2] > 5, "{bilinear:?}");
        let cubic = wide(FilterQuality::High).pixel(3, 0);
        assert_ne!(cubic, bilinear);
    }

    /// A tiled shader's local matrix: the tile scaled, nothing past it
    /// with `decal`; its filter samples the image.
    #[test]
    fn a_tiled_shader_goes_through_its_transform() {
        let image = ImageData::decoded(Size::new(2, 1), vec![255, 0, 0, 255, 0, 0, 255, 255]);
        let shader = |transform, filter| TiledShader {
            image: ShaderImage::Image(image.clone()),
            tile: RectF::new(0.0, 0.0, 2.0, 1.0),
            spacing: gfx::SizeF::new(0.0, 0.0),
            repeat_x: false,
            repeat_y: false,
            transform,
            filter,
            decal: true,
        };
        let area = RectF::new(0.0, 0.0, 16.0, 8.0);
        let bmp = fill_with(
            shader(
                Transform([4.0, 0.0, 0.0, 4.0, 0.0, 0.0]),
                FilterQuality::None,
            ),
            area,
            Size::new(16, 8),
        );
        assert_eq!(bmp.pixel(3, 3), [255, 0, 0, 255]);
        assert_eq!(bmp.pixel(4, 3), [0, 0, 255, 255]);
        assert_eq!(bmp.pixel(9, 3), WHITE);
        assert_eq!(bmp.pixel(3, 5), WHITE);
        // Bilinear mixes where nearest doesn't.
        let mixed = fill_with(
            shader(
                Transform([4.0, 0.0, 0.0, 4.0, 0.0, 0.0]),
                FilterQuality::Low,
            ),
            area,
            Size::new(16, 8),
        );
        assert_ne!(mixed.pixel(3, 1), [255, 0, 0, 255]);
    }

    /// `ops` played on a transparent bitmap `width` × `height`.
    fn played(ops: Vec<PaintOp>, width: f32, height: f32) -> crate::Bitmap {
        let mut list = DisplayItemList::new();
        for op in ops {
            list.push_op(op);
        }
        list.finalize();
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, width, height),
                contents_scale: 1.0,
                clear_color: Color::TRANSPARENT,
            },
            &RasterCache::default(),
        )
    }

    fn filled(rect: RectF, color: Color, blend_mode: BlendMode) -> PaintOp {
        PaintOp::DrawRect {
            rect,
            flags: PaintFlags {
                blend_mode,
                ..PaintFlags::fill(color)
            },
        }
    }

    const RED_COLOR: Color = Color::rgb(255, 0, 0);
    const BLUE_COLOR: Color = Color::rgb(0, 0, 255);

    /// A draw combines with what is under it by its blend mode.
    #[test]
    fn a_draw_is_combined_by_its_blend_mode() {
        let cut = played(
            vec![
                filled(
                    RectF::new(0.0, 0.0, 4.0, 2.0),
                    RED_COLOR,
                    BlendMode::SrcOver,
                ),
                filled(
                    RectF::new(2.0, 0.0, 2.0, 2.0),
                    BLUE_COLOR,
                    BlendMode::DstOut,
                ),
            ],
            4.0,
            2.0,
        );
        assert_eq!(cut.pixel(1, 1), [255, 0, 0, 255]);
        assert_eq!(cut.pixel(3, 1), [0, 0, 0, 0]);
        let multiplied = played(
            vec![
                filled(
                    RectF::new(0.0, 0.0, 1.0, 1.0),
                    Color::rgb(255, 255, 0),
                    BlendMode::SrcOver,
                ),
                filled(
                    RectF::new(0.0, 0.0, 1.0, 1.0),
                    Color::rgb(0, 255, 255),
                    BlendMode::Multiply,
                ),
            ],
            1.0,
            1.0,
        );
        assert_eq!(multiplied.pixel(0, 0), [0, 255, 0, 255]);
    }

    /// A layer is composited by its flags' mode as a whole, through its
    /// filter: a shadow under what it drew, or alone, moved and blurred in
    /// the space it was saved in.
    #[test]
    fn a_layer_is_composited_by_its_flags() {
        let layer = |flags: LayerFlags| PaintOp::SaveLayer {
            bounds: None,
            flags,
        };
        // `source-in`: the red outside the blue square goes too.
        let kept = played(
            vec![
                filled(
                    RectF::new(0.0, 0.0, 4.0, 2.0),
                    RED_COLOR,
                    BlendMode::SrcOver,
                ),
                layer(LayerFlags {
                    blend_mode: BlendMode::SrcIn,
                    ..LayerFlags::default()
                }),
                filled(
                    RectF::new(2.0, 0.0, 2.0, 2.0),
                    BLUE_COLOR,
                    BlendMode::SrcOver,
                ),
                PaintOp::Restore,
            ],
            4.0,
            2.0,
        );
        assert_eq!(kept.pixel(1, 1), [0, 0, 0, 0]);
        assert_eq!(kept.pixel(3, 1), [0, 0, 255, 255]);
        let shadowed = |sigma, shadow_only| {
            played(
                vec![
                    layer(LayerFlags {
                        image_filter: Some(ImageFilter::DropShadow {
                            offset: (4.0, 0.0),
                            sigma,
                            color: Color::rgb(0, 255, 0),
                            shadow_only,
                        }),
                        ..LayerFlags::default()
                    }),
                    // Scaled: the shadow's offset isn't.
                    PaintOp::SetMatrix {
                        transform: Transform([2.0, 0.0, 0.0, 2.0, 0.0, 0.0]),
                    },
                    filled(
                        RectF::new(0.0, 0.0, 2.0, 1.0),
                        RED_COLOR,
                        BlendMode::SrcOver,
                    ),
                    PaintOp::Restore,
                ],
                16.0,
                2.0,
            )
        };
        let both = shadowed(0.0, false);
        assert_eq!(both.pixel(3, 1), [255, 0, 0, 255]);
        assert_eq!(both.pixel(5, 1), [0, 255, 0, 255]);
        assert_eq!(both.pixel(9, 1), [0, 0, 0, 0]);
        let alone = shadowed(0.0, true);
        assert_eq!(alone.pixel(3, 1), [0, 0, 0, 0]);
        assert_eq!(alone.pixel(5, 1), [0, 255, 0, 255]);
        // Blurred: partly past its edge.
        let blurred = shadowed(1.0, true);
        let [_, _, _, edge] = blurred.pixel(8, 1);
        assert!(edge > 0 && edge < 255, "{edge}");
    }

    /// `DrawColor` fills the clip by its mode: `Src` clears it, `DstOver`
    /// fills only what is transparent.
    #[test]
    fn draw_color_fills_the_clip_by_its_mode() {
        let clip = |ops: Vec<PaintOp>| {
            let mut all = vec![
                filled(
                    RectF::new(0.0, 0.0, 2.0, 2.0),
                    RED_COLOR,
                    BlendMode::SrcOver,
                ),
                PaintOp::Save,
                PaintOp::ClipRect {
                    rect: RectF::new(1.0, 0.0, 2.0, 2.0),
                    anti_alias: false,
                },
            ];
            all.extend(ops);
            all.push(PaintOp::Restore);
            played(all, 4.0, 2.0)
        };
        let cleared = clip(vec![PaintOp::DrawColor {
            color: Color::TRANSPARENT,
            blend_mode: BlendMode::Src,
        }]);
        assert_eq!(cleared.pixel(0, 1), [255, 0, 0, 255]);
        assert_eq!(cleared.pixel(1, 1), [0, 0, 0, 0]);
        let under = clip(vec![PaintOp::DrawColor {
            color: Color::BLACK,
            blend_mode: BlendMode::DstOver,
        }]);
        assert_eq!(under.pixel(1, 1), [255, 0, 0, 255]);
        assert_eq!(under.pixel(2, 1), [0, 0, 0, 255]);
        assert_eq!(under.pixel(3, 1), [0, 0, 0, 0]);
    }

    /// A layer's alpha, and its bounds, which nothing drawn in it passes.
    #[test]
    fn a_layer_has_its_alpha_and_bounds() {
        let bmp = played(
            vec![
                PaintOp::SaveLayer {
                    bounds: Some(RectF::new(0.0, 0.0, 2.0, 1.0)),
                    flags: LayerFlags {
                        alpha: 0.5,
                        ..LayerFlags::default()
                    },
                },
                filled(
                    RectF::new(0.0, 0.0, 4.0, 1.0),
                    RED_COLOR,
                    BlendMode::SrcOver,
                ),
                PaintOp::Restore,
            ],
            4.0,
            1.0,
        );
        let [r, _, _, a] = bmp.pixel(1, 0);
        assert!((127..=128).contains(&a) && r > 0, "{:?}", bmp.pixel(1, 0));
        assert_eq!(bmp.pixel(3, 0), [0, 0, 0, 0]);
    }

    /// Each mode is Skia's of the same name.
    #[test]
    fn each_blend_mode_is_skias() {
        for mode in [
            BlendMode::Clear,
            BlendMode::Src,
            BlendMode::SrcOver,
            BlendMode::DstOver,
            BlendMode::SrcIn,
            BlendMode::DstIn,
            BlendMode::SrcOut,
            BlendMode::DstOut,
            BlendMode::SrcATop,
            BlendMode::DstATop,
            BlendMode::Xor,
            BlendMode::Plus,
            BlendMode::Multiply,
            BlendMode::Screen,
            BlendMode::Overlay,
            BlendMode::Darken,
            BlendMode::Lighten,
            BlendMode::ColorDodge,
            BlendMode::ColorBurn,
            BlendMode::HardLight,
            BlendMode::SoftLight,
            BlendMode::Difference,
            BlendMode::Exclusion,
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ] {
            assert_eq!(format!("{:?}", sk_blend_mode(mode)), format!("{mode:?}"));
        }
    }

    #[test]
    fn draws_an_image_scaled_into_its_rect() {
        // 20x10 (green left, blue right) drawn at 2x, 5px in.
        let bmp = draw(
            ImageData::new(GREEN_BLUE),
            RectF::new(5.0, 5.0, 40.0, 20.0),
            Size::new(50, 30),
        );
        assert_eq!(bmp.pixel(2, 2), [255, 255, 255, 255]);
        assert_eq!(bmp.pixel(10, 15), [0, 128, 0, 255]);
        assert_eq!(bmp.pixel(40, 15), [0, 0, 255, 255]);
        assert_eq!(bmp.pixel(47, 27), [255, 255, 255, 255]);
    }

    /// Each format a page may give is drawn: here a BMP and a JPEG, a PNG
    /// above.
    #[test]
    fn draws_each_format() {
        use crate::image_decode::tests::{JPEG, bmp};
        let rect = RectF::new(0.0, 0.0, 40.0, 30.0);
        let drawn = draw(
            ImageData::new(bmp(40, 30, [0, 128, 0])),
            rect,
            Size::new(40, 30),
        );
        assert_eq!(drawn.pixel(20, 15), [0, 128, 0, 255]);
        let drawn = draw(ImageData::new(JPEG), rect, Size::new(40, 30));
        let [r, g, b, a] = drawn.pixel(20, 15);
        assert!(r <= 3 && (125..=131).contains(&g) && b <= 3 && a == 255);
    }

    /// A JPEG whose EXIF orientation turns it a quarter is drawn turned,
    /// the size its header gave (`image_size`) the turned one: drawn in a
    /// rect of that size, nothing is stretched.
    #[test]
    fn draws_a_jpeg_turned_as_its_orientation_says() {
        use crate::image_decode::tests::{JPEG, with_orientation};
        let turned = with_orientation(JPEG, 6);
        assert_eq!(crate::image_size(&turned), Some(Size::new(30, 40)));
        let cache = RasterCache::default();
        let image = cache.image(&ImageData::new(turned)).unwrap();
        assert_eq!((image.width(), image.height()), (30, 40));
    }

    /// Pixels with alpha reach Skia premultiplied: half transparent red
    /// over white is pink.
    #[test]
    fn draws_a_translucent_image_over_what_is_there() {
        let png = crate::image_decode::tests::png_of(1, 1, &[255, 0, 0, 128]);
        let drawn = draw(
            ImageData::new(png),
            RectF::new(0.0, 0.0, 4.0, 4.0),
            Size::new(4, 4),
        );
        assert_eq!(drawn.pixel(2, 2), [255, 127, 127, 255]);
    }

    #[test]
    fn a_decoded_image_is_dropped_with_its_data() {
        let cache = RasterCache::default();
        let data = ImageData::new(GREEN_BLUE);
        let id = data.id;
        let rect = RectF::new(0.0, 0.0, 20.0, 10.0);
        draw_with(&cache, data.clone(), rect, Size::new(20, 10));
        assert!(cache.tables().images.contains_key(&id));
        // Another compositor's cache has none of it.
        let other = RasterCache::default();
        draw_with(&other, ImageData::new(GREEN_BLUE), rect, Size::new(20, 10));
        assert!(!other.tables().images.contains_key(&id));
        assert!(cache.tables().images.contains_key(&id));
        drop(data);
        // Cached entries whose data is gone go when the next image comes in.
        draw_with(&cache, ImageData::new(GREEN_BLUE), rect, Size::new(20, 10));
        assert!(!cache.tables().images.contains_key(&id));
        // Undecodable bytes draw nothing.
        let bmp = draw(
            ImageData::new(&b"junk"[..]),
            RectF::new(0.0, 0.0, 4.0, 4.0),
            Size::new(4, 4),
        );
        assert_eq!(bmp.pixel(1, 1), [255, 255, 255, 255]);
    }

    /// Threads rastering with one cache at once draw the same, and share
    /// its entries: the image is decoded once.
    #[test]
    fn threads_share_a_cache() {
        let cache = RasterCache::default();
        let data = ImageData::new(GREEN_BLUE);
        let rect = RectF::new(0.0, 0.0, 20.0, 10.0);
        let alone = draw_with(
            &RasterCache::default(),
            data.clone(),
            rect,
            Size::new(20, 10),
        );
        std::thread::scope(|scope| {
            let drawn: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| draw_with(&cache, data.clone(), rect, Size::new(20, 10))))
                .collect();
            for bitmap in drawn {
                assert!(bitmap.join().unwrap().pixels == alone.pixels);
            }
        });
        assert_eq!(cache.tables().images.len(), 1);
        assert_eq!(decodes(&cache), 1);
    }

    fn decodes(cache: &RasterCache) -> usize {
        cache.decodes.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// An image is decoded when it is first drawn, once however often it
    /// is drawn after; bytes that aren't an image are tried once too.
    #[test]
    fn an_image_is_decoded_once() {
        let cache = RasterCache::default();
        let data = ImageData::new(GREEN_BLUE);
        assert_eq!(decodes(&cache), 0);
        let rect = RectF::new(0.0, 0.0, 20.0, 10.0);
        for _ in 0..3 {
            draw_with(&cache, data.clone(), rect, Size::new(20, 10));
        }
        assert_eq!(decodes(&cache), 1);
        let junk = ImageData::new(&b"junk"[..]);
        for _ in 0..3 {
            draw_with(&cache, junk.clone(), rect, Size::new(20, 10));
        }
        assert_eq!(decodes(&cache), 2);
        // A canvas's snapshot is pixels already.
        let snapshot = ImageData::decoded(Size::new(1, 1), vec![0, 128, 0, 255]);
        let drawn = draw_with(&cache, snapshot, rect, Size::new(20, 10));
        assert_eq!(drawn.pixel(10, 5), [0, 128, 0, 255]);
        assert_eq!(decodes(&cache), 2);
    }

    fn fill_with(shader: impl Into<PaintShader>, rect: RectF, size: Size) -> crate::Bitmap {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect,
            flags: PaintFlags::shader(shader),
        });
        list.finalize();
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, size.width as f32, size.height as f32),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        )
    }

    fn image_shader(tile: RectF, spacing: gfx::SizeF, repeat: bool) -> TiledShader {
        TiledShader {
            image: ShaderImage::Image(ImageData::new(GREEN_BLUE)),
            tile,
            spacing,
            repeat_x: repeat,
            repeat_y: repeat,
            transform: Transform::IDENTITY,
            filter: FilterQuality::Medium,
            decal: false,
        }
    }

    const GREEN: [u8; 4] = [0, 128, 0, 255];
    const BLUE: [u8; 4] = [0, 0, 255, 255];
    const WHITE: [u8; 4] = [255, 255, 255, 255];

    #[test]
    fn an_image_shader_tiles_both_ways_from_its_tile() {
        // 20x10, green left and blue right, its first tile 5px in.
        let bmp = fill_with(
            image_shader(
                RectF::new(5.0, 5.0, 20.0, 10.0),
                gfx::SizeF::new(0.0, 0.0),
                true,
            ),
            RectF::new(0.0, 0.0, 50.0, 30.0),
            Size::new(50, 30),
        );
        // Left of and above the first tile: the tile before it.
        assert_eq!(bmp.pixel(2, 2), BLUE);
        assert_eq!(bmp.pixel(8, 7), GREEN);
        assert_eq!(bmp.pixel(20, 7), BLUE);
        assert_eq!(bmp.pixel(28, 17), GREEN);
        assert_eq!(bmp.pixel(48, 27), GREEN);
        // Scaled into the tile: 10x10, green the left 5px.
        let bmp = fill_with(
            image_shader(
                RectF::new(0.0, 0.0, 10.0, 10.0),
                gfx::SizeF::new(0.0, 0.0),
                true,
            ),
            RectF::new(0.0, 0.0, 20.0, 10.0),
            Size::new(20, 10),
        );
        assert_eq!(bmp.pixel(2, 5), GREEN);
        assert_eq!(bmp.pixel(7, 5), BLUE);
        assert_eq!(bmp.pixel(12, 5), GREEN);
    }

    #[test]
    fn spaced_tiles_leave_gaps() {
        let bmp = fill_with(
            image_shader(
                RectF::new(0.0, 0.0, 20.0, 10.0),
                gfx::SizeF::new(10.0, 5.0),
                true,
            ),
            RectF::new(0.0, 0.0, 60.0, 30.0),
            Size::new(60, 30),
        );
        assert_eq!(bmp.pixel(5, 5), GREEN);
        assert_eq!(bmp.pixel(25, 5), WHITE);
        assert_eq!(bmp.pixel(35, 5), GREEN);
        assert_eq!(bmp.pixel(45, 5), BLUE);
        assert_eq!(bmp.pixel(5, 12), WHITE);
        assert_eq!(bmp.pixel(5, 17), GREEN);
    }

    #[test]
    fn a_shadow_is_blurred_and_clipped_to_its_side_of_the_box() {
        let draw = |op: PaintOp| {
            let mut list = DisplayItemList::new();
            list.push_op(op);
            list.finalize();
            crate::playback(
                &list,
                &PlaybackParams {
                    content_rect: RectF::new(0.0, 0.0, 60.0, 60.0),
                    contents_scale: 1.0,
                    clear_color: Color::WHITE,
                },
                &RasterCache::default(),
            )
        };
        let shadow = |blur: f32| PaintFlags {
            blur,
            ..PaintFlags::fill(Color::BLACK)
        };
        let box_rect = gfx::RRectF::from_rect(RectF::new(20.0, 20.0, 20.0, 20.0));
        // An outer shadow 10 to the right, unblurred: nothing under the box.
        let bmp = draw(PaintOp::DrawShadow {
            shape: box_rect.offset(10.0, 0.0),
            hole: None,
            clip: box_rect,
            inside: false,
            flags: shadow(0.0),
        });
        assert_eq!(bmp.pixel(30, 30), WHITE);
        assert_eq!(bmp.pixel(45, 30), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(55, 30), WHITE);
        // Blurred: half dark at the shape's edge, fading out past it.
        let bmp = draw(PaintOp::DrawShadow {
            shape: box_rect.offset(10.0, 0.0),
            hole: None,
            clip: box_rect,
            inside: false,
            flags: shadow(2.0),
        });
        let gray = |x: u32| bmp.pixel(x, 30)[0];
        assert!(gray(45) < 10, "{}", gray(45));
        assert!((100..170).contains(&gray(50)), "{}", gray(50));
        assert!(gray(53) > 240, "{}", gray(53));
        // Inset: the ring between the clip and the hole, only inside.
        let bmp = draw(PaintOp::DrawShadow {
            shape: gfx::RRectF::from_rect(RectF::new(0.0, 0.0, 60.0, 60.0)),
            hole: Some(box_rect.inset(5.0, 5.0, 5.0, 5.0)),
            clip: box_rect,
            inside: true,
            flags: shadow(0.0),
        });
        assert_eq!(bmp.pixel(22, 30), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(30, 30), WHITE);
        assert_eq!(bmp.pixel(15, 30), WHITE);
    }

    #[test]
    fn dashes_leave_gaps_and_dots_are_round() {
        let line = |dash, cap| {
            let mut list = DisplayItemList::new();
            list.push_op(PaintOp::DrawLine {
                p0: gfx::PointF::new(0.0, 5.0),
                p1: gfx::PointF::new(40.0, 5.0),
                flags: PaintFlags {
                    dash: Some(dash),
                    cap,
                    ..PaintFlags::stroke(Color::BLACK, 4.0)
                },
            });
            list.finalize();
            crate::playback(
                &list,
                &PlaybackParams {
                    content_rect: RectF::new(0.0, 0.0, 40.0, 10.0),
                    contents_scale: 1.0,
                    clear_color: Color::WHITE,
                },
                &RasterCache::default(),
            )
        };
        let black = [0, 0, 0, 255];
        // 8 on, 4 off.
        let bmp = line(
            compositor_paint::Dash::new(&[8.0, 4.0], 0.0).unwrap(),
            LineCap::Butt,
        );
        assert_eq!(bmp.pixel(4, 5), black);
        assert_eq!(bmp.pixel(10, 5), WHITE);
        assert_eq!(bmp.pixel(14, 5), black);
        // Dots 4 wide every 10: round, so a dot's corner square is white.
        let bmp = line(
            compositor_paint::Dash::new(&[0.0, 10.0], 0.0).unwrap(),
            LineCap::Round,
        );
        assert_eq!(bmp.pixel(10, 5), black);
        assert_eq!(bmp.pixel(5, 5), WHITE);
        assert_ne!(bmp.pixel(11, 3), black);
        assert_eq!(bmp.pixel(11, 3)[3], 255);
        assert!(bmp.pixel(10, 6)[0] < 64, "{:?}", bmp.pixel(10, 6));
    }

    /// `ops` played back onto a white `width` by `height` bitmap.
    fn play(ops: Vec<PaintOp>, width: f32, height: f32) -> crate::Bitmap {
        let mut list = DisplayItemList::new();
        for op in ops {
            list.push_op(op);
        }
        list.finalize();
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, width, height),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        )
    }

    #[test]
    fn paths_become_skias_element_for_element() {
        use sk::PathVerb as V;
        let p = PointF::new;
        let mut path = gfx::Path::new();
        path.move_to(p(1.0, 1.0));
        path.line_to(p(9.0, 1.0));
        path.quad_to(p(9.0, 9.0), p(5.0, 9.0));
        path.cubic_to(p(4.0, 9.0), p(3.0, 8.0), p(2.0, 7.0));
        path.conic_to(p(1.0, 7.0), p(1.0, 5.0), 0.5);
        path.close();
        // No move after the close: Skia starts at the subpath's start.
        path.line_to(p(5.0, 5.0));
        let sk = sk_path(&path.with_fill_rule(FillRule::EvenOdd));
        assert_eq!(sk.fill_type(), sk::PathFillType::EvenOdd);
        assert_eq!(
            sk.verbs(),
            [
                V::Move,
                V::Line,
                V::Quad,
                V::Cubic,
                V::Conic,
                V::Close,
                V::Move,
                V::Line
            ]
        );
        let points: Vec<(f32, f32)> = sk.points().iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(
            points,
            [
                (1.0, 1.0),
                (9.0, 1.0),
                (9.0, 9.0),
                (5.0, 9.0),
                (4.0, 9.0),
                (3.0, 8.0),
                (2.0, 7.0),
                (1.0, 7.0),
                (1.0, 5.0),
                (1.0, 1.0),
                (5.0, 5.0)
            ]
        );
        assert_eq!(sk.conic_weights(), [0.5]);
        let sk = sk_path(&gfx::Path::new());
        assert_eq!(sk.fill_type(), sk::PathFillType::Winding);
    }

    #[test]
    fn a_path_fills_by_its_fill_rule() {
        let p = PointF::new;
        // A 20px square round a 10px one, both clockwise.
        let squares = gfx::Path::polygons([
            vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0), p(0.0, 20.0)],
            vec![p(5.0, 5.0), p(15.0, 5.0), p(15.0, 15.0), p(5.0, 15.0)],
        ]);
        let fill = |rule| {
            play(
                vec![PaintOp::DrawPath {
                    path: squares.clone().with_fill_rule(rule),
                    flags: PaintFlags::fill(Color::BLACK),
                }],
                20.0,
                20.0,
            )
        };
        let black = [0, 0, 0, 255];
        let nonzero = fill(FillRule::NonZero);
        assert_eq!(nonzero.pixel(2, 2), black);
        assert_eq!(nonzero.pixel(10, 10), black);
        let evenodd = fill(FillRule::EvenOdd);
        assert_eq!(evenodd.pixel(2, 2), black);
        assert_eq!(evenodd.pixel(10, 10), WHITE);
    }

    /// A clip to several paths keeps what any of them covers, each by its
    /// own fill rule; none keeps nothing.
    /// A layer's filters: a color matrix's last column 0 to 1 (an inverted
    /// red is cyan, the white around it black), a blur reaching past what
    /// it blurs, and a chain applied in order (inverted, then made half
    /// transparent, over the white page; inverted, then its color taken
    /// away, black, which the other way round is white).
    #[test]
    fn layer_filters_invert_blur_and_chain() {
        let layer = |filter: ImageFilter| {
            play(
                vec![
                    PaintOp::SaveLayer {
                        bounds: None,
                        flags: LayerFlags {
                            image_filter: Some(filter),
                            ..LayerFlags::default()
                        },
                    },
                    PaintOp::DrawRect {
                        rect: RectF::new(10.0, 0.0, 10.0, 10.0),
                        flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
                    },
                    PaintOp::Restore,
                ],
                30.0,
                10.0,
            )
        };
        let mut invert = [0.0; 20];
        for row in 0..3 {
            invert[row * 5 + row] = -1.0;
            invert[row * 5 + 4] = 1.0;
        }
        invert[18] = 1.0;
        let bmp = layer(ImageFilter::ColorMatrix(invert));
        assert_eq!(bmp.pixel(15, 5), [0, 255, 255, 255]);
        // Transparent there: inverted, still transparent over the page.
        assert_eq!(bmp.pixel(5, 5), WHITE);
        let bmp = layer(ImageFilter::Blur { sigma: 2.0 });
        assert_ne!(bmp.pixel(8, 5), WHITE);
        assert_eq!(bmp.pixel(1, 5), WHITE);
        let mut half = [0.0; 20];
        for row in 0..4 {
            half[row * 5 + row] = if row == 3 { 0.5 } else { 1.0 };
        }
        let bmp = layer(ImageFilter::Chain(
            [
                ImageFilter::ColorMatrix(invert),
                ImageFilter::ColorMatrix(half),
            ]
            .into(),
        ));
        // Cyan at half over white.
        let [r, g, b, _] = bmp.pixel(15, 5);
        assert!(
            (126..=129).contains(&r) && g == 255 && b == 255,
            "{r} {g} {b}"
        );
        let mut black = [0.0; 20];
        black[18] = 1.0;
        let bmp = layer(ImageFilter::Chain(
            [
                ImageFilter::ColorMatrix(invert),
                ImageFilter::ColorMatrix(black),
            ]
            .into(),
        ));
        assert_eq!(bmp.pixel(15, 5), [0, 0, 0, 255]);
    }

    #[test]
    fn a_clip_to_paths_keeps_their_union() {
        let p = PointF::new;
        let square = |x: f32| {
            gfx::Path::polygons([vec![
                p(x, 0.0),
                p(x + 10.0, 0.0),
                p(x + 10.0, 10.0),
                p(x, 10.0),
            ]])
        };
        // Two squares wound the same way, the second's center left out by
        // an even-odd fill of it twice.
        let mut ring = square(10.0);
        for &element in
            gfx::Path::polygons([vec![p(12.0, 2.0), p(18.0, 2.0), p(18.0, 8.0), p(12.0, 8.0)]])
                .elements()
        {
            match element {
                gfx::PathElement::MoveTo(at) => ring.move_to(at),
                gfx::PathElement::LineTo(at) => ring.line_to(at),
                gfx::PathElement::Close => ring.close(),
                _ => {}
            }
        }
        let ring = ring.with_fill_rule(gfx::FillRule::EvenOdd);
        let fill = |paths: Vec<gfx::Path>| {
            play(
                vec![
                    PaintOp::ClipPaths {
                        paths,
                        anti_alias: true,
                    },
                    PaintOp::DrawRect {
                        rect: RectF::new(0.0, 0.0, 30.0, 10.0),
                        flags: PaintFlags::fill(Color::BLACK),
                    },
                ],
                30.0,
                10.0,
            )
        };
        let bmp = fill(vec![square(0.0), ring]);
        assert_eq!(bmp.pixel(5, 5), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(11, 1), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(15, 5), WHITE);
        assert_eq!(bmp.pixel(25, 5), WHITE);
        assert_eq!(fill(Vec::new()).pixel(5, 5), WHITE);
    }

    #[test]
    fn a_path_clip_keeps_what_its_fill_covers() {
        let p = PointF::new;
        let triangle = gfx::Path::polygons([vec![p(0.0, 0.0), p(20.0, 0.0), p(0.0, 20.0)]]);
        let bmp = play(
            vec![
                PaintOp::ClipPath {
                    path: triangle,
                    anti_alias: true,
                },
                PaintOp::DrawRect {
                    rect: RectF::new(0.0, 0.0, 20.0, 20.0),
                    flags: PaintFlags::fill(Color::BLACK),
                },
            ],
            20.0,
            20.0,
        );
        assert_eq!(bmp.pixel(3, 3), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(16, 16), WHITE);
    }

    #[test]
    fn strokes_end_and_join_as_their_flags_say() {
        let p = PointF::new;
        let black = [0, 0, 0, 255];
        // A line 4 wide from x = 10 to 30.
        let mut line = gfx::Path::new();
        line.move_to(p(10.0, 10.0));
        line.line_to(p(30.0, 10.0));
        let end = |cap| {
            play(
                vec![PaintOp::DrawPath {
                    path: line.clone(),
                    flags: PaintFlags {
                        cap,
                        ..PaintFlags::stroke(Color::BLACK, 4.0)
                    },
                }],
                40.0,
                20.0,
            )
        };
        let butt = end(LineCap::Butt);
        assert_eq!(butt.pixel(10, 10), black);
        assert_eq!(butt.pixel(9, 10), WHITE);
        // Square: 2px past the end, corners and all.
        let square = end(LineCap::Square);
        assert_eq!(square.pixel(8, 8), black);
        // Round: past the end, but not into the corner.
        let round = end(LineCap::Round);
        assert_eq!(round.pixel(9, 10), black);
        assert_ne!(round.pixel(8, 8), black);
        assert_ne!(round.pixel(8, 8), WHITE);
        // A right angle 6 wide, its outer corner at (33, 37).
        let mut corner = gfx::Path::new();
        corner.move_to(p(10.0, 40.0));
        corner.line_to(p(30.0, 40.0));
        corner.line_to(p(30.0, 60.0));
        let join = |join, miter_limit| {
            play(
                vec![PaintOp::DrawPath {
                    path: corner.clone(),
                    flags: PaintFlags {
                        join,
                        miter_limit,
                        ..PaintFlags::stroke(Color::BLACK, 6.0)
                    },
                }],
                40.0,
                64.0,
            )
        };
        assert_eq!(join(LineJoin::Miter, 4.0).pixel(32, 37), black);
        assert_eq!(join(LineJoin::Bevel, 4.0).pixel(32, 37), WHITE);
        let round = join(LineJoin::Round, 4.0).pixel(32, 37);
        assert!(round != black && round != WHITE, "{round:?}");
        // A right angle's miter is 1.41 widths long: past a limit of 1.
        assert_eq!(join(LineJoin::Miter, 1.0).pixel(32, 37), WHITE);
    }

    #[test]
    fn dashes_go_through_their_intervals_from_the_phase() {
        let black = [0, 0, 0, 255];
        let mut line = gfx::Path::new();
        line.move_to(PointF::new(0.0, 2.0));
        line.line_to(PointF::new(20.0, 2.0));
        // 4 on, 2 off, 1 on, 3 off, starting 2 in: on to 2, off to 4, on
        // to 5, off to 8, on to 12.
        let bmp = play(
            vec![PaintOp::DrawPath {
                path: line,
                flags: PaintFlags {
                    dash: compositor_paint::Dash::new(&[4.0, 2.0, 1.0, 3.0], 2.0),
                    ..PaintFlags::stroke(Color::BLACK, 2.0)
                },
            }],
            20.0,
            4.0,
        );
        let row: Vec<bool> = (0..12).map(|x| bmp.pixel(x, 2) == black).collect();
        assert_eq!(
            row,
            [
                true, true, false, false, true, false, false, false, true, true, true, true
            ]
        );
    }

    #[test]
    fn a_clipped_op_draws_only_inside_its_polygon() {
        let p = gfx::PointF::new;
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawClipped {
            polygon: vec![p(0.0, 0.0), p(20.0, 0.0), p(0.0, 20.0)].into(),
            op: Box::new(PaintOp::DrawRect {
                rect: RectF::new(0.0, 0.0, 20.0, 20.0),
                flags: PaintFlags::fill(Color::BLACK),
            }),
        });
        list.finalize();
        let bmp = crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 20.0, 20.0),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        );
        assert_eq!(bmp.pixel(3, 3), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(16, 16), WHITE);
    }

    #[test]
    fn a_ring_with_a_clip_draws_only_inside_the_polygon() {
        // A rounded border's top side: the ring cut to the trapezoid
        // between the edges and the corners' diagonals.
        let outer = gfx::RRectF::uniform(RectF::new(0.0, 0.0, 40.0, 40.0), 10.0);
        let inner = outer.inset(10.0, 10.0, 10.0, 10.0);
        let p = gfx::PointF::new;
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawDRRect {
            outer,
            inner,
            clip: Some(vec![p(0.0, 0.0), p(40.0, 0.0), p(30.0, 10.0), p(10.0, 10.0)].into()),
            flags: PaintFlags::fill(Color::rgb(0, 128, 0)),
        });
        list.finalize();
        let bmp = crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 40.0, 40.0),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        );
        assert_eq!(bmp.pixel(20, 5), GREEN);
        // The left and bottom sides, and the hole.
        assert_eq!(bmp.pixel(5, 20), WHITE);
        assert_eq!(bmp.pixel(20, 35), WHITE);
        assert_eq!(bmp.pixel(20, 20), WHITE);
    }

    #[test]
    fn a_decal_shader_draws_nothing_past_its_tile() {
        // A rounded background draws its whole area: past the tile, on an
        // axis that doesn't repeat, nothing (not the edge's color
        // extended), for an image and for a gradient.
        let linear = GradientShaderKind::Linear {
            start: gfx::PointF::new(0.0, 0.0),
            end: gfx::PointF::new(20.0, 0.0),
        };
        for image in [
            ShaderImage::Image(ImageData::new(GREEN_BLUE)),
            red_to_blue(linear, false),
        ] {
            let shader = |decal| TiledShader {
                image: image.clone(),
                tile: RectF::new(5.0, 5.0, 20.0, 10.0),
                spacing: gfx::SizeF::new(0.0, 0.0),
                repeat_x: false,
                repeat_y: false,
                transform: Transform::IDENTITY,
                filter: FilterQuality::Medium,
                decal,
            };
            let area = RectF::new(0.0, 0.0, 40.0, 20.0);
            let bmp = fill_with(shader(true), area, Size::new(40, 20));
            assert_ne!(bmp.pixel(15, 10), WHITE);
            for (x, y) in [(2, 10), (35, 10), (15, 2), (15, 18)] {
                assert_eq!(bmp.pixel(x, y), WHITE, "({x}, {y})");
            }
            let bmp = fill_with(shader(false), area, Size::new(40, 20));
            assert_ne!(bmp.pixel(35, 10), WHITE);
        }
    }

    fn red_to_blue(kind: GradientShaderKind, repeating: bool) -> ShaderImage {
        ShaderImage::Gradient(GradientShader {
            kind,
            stops: vec![
                compositor_paint::GradientStop {
                    offset: 0.0,
                    color: Color::rgb(255, 0, 0),
                },
                compositor_paint::GradientStop {
                    offset: 1.0,
                    color: Color::rgb(0, 0, 255),
                },
            ],
            tile_mode: if repeating {
                TileMode::Repeat
            } else {
                TileMode::Clamp
            },
            transform: Transform::IDENTITY,
            interpolation: ColorInterpolation::Premultiplied,
        })
    }

    fn close(a: [u8; 4], b: [u8; 4]) -> bool {
        a.iter()
            .zip(b)
            .all(|(x, y)| (*x as i32 - y as i32).abs() <= 3)
    }

    #[test]
    fn a_linear_gradient_goes_along_its_line_in_its_tile() {
        let linear = GradientShaderKind::Linear {
            start: gfx::PointF::new(0.0, 0.0),
            end: gfx::PointF::new(100.0, 0.0),
        };
        let shader = |repeat| TiledShader {
            image: red_to_blue(linear, false),
            tile: RectF::new(10.0, 0.0, 100.0, 10.0),
            spacing: gfx::SizeF::new(0.0, 0.0),
            repeat_x: repeat,
            repeat_y: repeat,
            transform: Transform::IDENTITY,
            filter: FilterQuality::Medium,
            decal: false,
        };
        let bmp = fill_with(
            shader(false),
            RectF::new(10.0, 0.0, 100.0, 10.0),
            Size::new(120, 10),
        );
        assert!(
            close(bmp.pixel(10, 5), [253, 0, 2, 255]),
            "{:?}",
            bmp.pixel(10, 5)
        );
        assert!(
            close(bmp.pixel(60, 5), [126, 0, 129, 255]),
            "{:?}",
            bmp.pixel(60, 5)
        );
        assert!(
            close(bmp.pixel(109, 5), [2, 0, 253, 255]),
            "{:?}",
            bmp.pixel(109, 5)
        );
        // Repeated as a tile: red again right after the first one's blue.
        let bmp = fill_with(
            shader(true),
            RectF::new(0.0, 0.0, 120.0, 10.0),
            Size::new(120, 10),
        );
        assert!(
            close(bmp.pixel(5, 5), [12, 0, 243, 255]),
            "{:?}",
            bmp.pixel(5, 5)
        );
        assert!(
            close(bmp.pixel(110, 5), [253, 0, 2, 255]),
            "{:?}",
            bmp.pixel(110, 5)
        );
    }

    #[test]
    fn gradients_interpolate_premultiplied() {
        // From transparent black to red: halfway is half-transparent red,
        // not half-transparent dark red.
        let shader = TiledShader {
            image: ShaderImage::Gradient(GradientShader {
                kind: GradientShaderKind::Linear {
                    start: gfx::PointF::new(0.0, 0.0),
                    end: gfx::PointF::new(100.0, 0.0),
                },
                stops: vec![
                    compositor_paint::GradientStop {
                        offset: 0.0,
                        color: Color::TRANSPARENT,
                    },
                    compositor_paint::GradientStop {
                        offset: 1.0,
                        color: Color::rgb(255, 0, 0),
                    },
                ],
                tile_mode: TileMode::Clamp,
                transform: Transform::IDENTITY,
                interpolation: ColorInterpolation::Premultiplied,
            }),
            tile: RectF::new(0.0, 0.0, 100.0, 10.0),
            spacing: gfx::SizeF::new(0.0, 0.0),
            repeat_x: false,
            repeat_y: false,
            transform: Transform::IDENTITY,
            filter: FilterQuality::Medium,
            decal: false,
        };
        let bmp = fill_with(
            shader,
            RectF::new(0.0, 0.0, 100.0, 10.0),
            Size::new(100, 10),
        );
        // Over white.
        assert!(
            close(bmp.pixel(50, 5), [255, 126, 126, 255]),
            "{:?}",
            bmp.pixel(50, 5)
        );
    }

    #[test]
    fn a_gradients_transform_is_after_its_tiles_place() {
        let center = gfx::PointF::new(50.0, 25.0);
        let radial = GradientShaderKind::TwoPointConical {
            start: center,
            start_radius: 0.0,
            end: center,
            end_radius: 50.0,
        };
        let ShaderImage::Gradient(mut gradient) = red_to_blue(radial, false) else {
            unreachable!()
        };
        gradient.transform = Transform::translate(50.0, 25.0)
            .then(&Transform::scale(1.0, 0.5))
            .then(&Transform::translate(-50.0, -25.0));
        // The tile 50px down: the ellipse's center at (50, 75), scaled in
        // the tile's space (not about (50, 25) in the current one).
        let shader = TiledShader {
            image: ShaderImage::Gradient(gradient),
            tile: RectF::new(0.0, 50.0, 100.0, 50.0),
            spacing: gfx::SizeF::new(0.0, 0.0),
            repeat_x: false,
            repeat_y: false,
            transform: Transform::IDENTITY,
            filter: FilterQuality::Medium,
            decal: false,
        };
        let bmp = fill_with(
            shader,
            RectF::new(0.0, 50.0, 100.0, 50.0),
            Size::new(100, 100),
        );
        // As far along at 25px across as at 12.5px down.
        let across = bmp.pixel(75, 75);
        let down = bmp.pixel(50, 87);
        assert!(close(across, down), "{across:?} {down:?}");
        // At the center (the pixel's is half a pixel off it): red.
        assert!(bmp.pixel(50, 75)[0] > 240, "{:?}", bmp.pixel(50, 75));
        // Past the ring: the last color.
        assert!(close(bmp.pixel(1, 51), [0, 0, 255, 255]));
    }

    /// A gradient that isn't tiled: over the whole plane, through its
    /// transform, its tile mode past the ends.
    fn plane(kind: GradientShaderKind, tile_mode: TileMode, transform: Transform) -> PaintShader {
        let ShaderImage::Gradient(mut gradient) = red_to_blue(kind, false) else {
            unreachable!()
        };
        gradient.tile_mode = tile_mode;
        gradient.transform = transform;
        gradient.interpolation = ColorInterpolation::Unpremultiplied;
        PaintShader::Gradient(gradient)
    }

    /// A sweep goes clockwise (y down) round its center from its start
    /// angle, through its transform; the empty shader draws nothing.
    #[test]
    fn a_sweep_goes_round_its_center_and_empty_draws_nothing() {
        let sweep = GradientShaderKind::Sweep {
            center: gfx::PointF::new(20.0, 20.0),
            start_angle: 0.0,
            end_angle: 360.0,
        };
        let fill = |shader| fill_with(shader, RectF::new(0.0, 0.0, 40.0, 40.0), Size::new(40, 40));
        let red = |[r, _, b, _]: [u8; 4]| r > 230 && b < 25;
        let blue = |[r, _, b, _]: [u8; 4]| r < 25 && b > 230;
        // Just past the start, under the x axis on the right; just before
        // the end, over it; a quarter of the way round, under the center.
        let bmp = fill(plane(sweep, TileMode::Clamp, Transform::IDENTITY));
        assert!(red(bmp.pixel(38, 21)), "{:?}", bmp.pixel(38, 21));
        assert!(blue(bmp.pixel(38, 18)), "{:?}", bmp.pixel(38, 18));
        let [r, _, b, _] = bmp.pixel(20, 38);
        assert!(
            (185..200).contains(&r) && (55..72).contains(&b),
            "{:?}",
            bmp.pixel(20, 38)
        );
        // Turned a quarter round about the center: it starts downwards.
        let turned = Transform([0.0, 1.0, -1.0, 0.0, 40.0, 0.0]);
        let bmp = fill(plane(sweep, TileMode::Clamp, turned));
        assert!(red(bmp.pixel(18, 38)), "{:?}", bmp.pixel(18, 38));
        assert!(blue(bmp.pixel(21, 38)), "{:?}", bmp.pixel(21, 38));
        assert_eq!(fill(PaintShader::Empty).pixel(20, 20), WHITE);
    }

    #[test]
    fn a_gradient_covers_the_plane_through_its_transform() {
        let linear = GradientShaderKind::Linear {
            start: gfx::PointF::new(0.0, 0.0),
            end: gfx::PointF::new(100.0, 0.0),
        };
        let shader = plane(linear, TileMode::Clamp, Transform::translate(50.0, 0.0));
        let bmp = fill_with(
            shader,
            RectF::new(0.0, 0.0, 200.0, 10.0),
            Size::new(200, 10),
        );
        assert!(
            close(bmp.pixel(10, 5), [255, 0, 0, 255]),
            "{:?}",
            bmp.pixel(10, 5)
        );
        assert!(
            close(bmp.pixel(100, 5), [127, 0, 128, 255]),
            "{:?}",
            bmp.pixel(100, 5)
        );
        assert!(
            close(bmp.pixel(190, 5), [0, 0, 255, 255]),
            "{:?}",
            bmp.pixel(190, 5)
        );
    }

    #[test]
    fn past_its_ends_a_gradient_extends_repeats_or_mirrors() {
        let linear = GradientShaderKind::Linear {
            start: gfx::PointF::new(0.0, 0.0),
            end: gfx::PointF::new(50.0, 0.0),
        };
        let at_60 = |mode| {
            let shader = plane(linear, mode, Transform::IDENTITY);
            fill_with(
                shader,
                RectF::new(0.0, 0.0, 100.0, 10.0),
                Size::new(100, 10),
            )
            .pixel(60, 5)
        };
        // A fifth of the way into the second length.
        assert!(
            close(at_60(TileMode::Clamp), [0, 0, 255, 255]),
            "{:?}",
            at_60(TileMode::Clamp)
        );
        assert!(
            close(at_60(TileMode::Repeat), [202, 0, 53, 255]),
            "{:?}",
            at_60(TileMode::Repeat)
        );
        assert!(
            close(at_60(TileMode::Mirror), [53, 0, 202, 255]),
            "{:?}",
            at_60(TileMode::Mirror)
        );
    }

    /// Two circles with centers apart: SVG's focal point, inside the end
    /// circle off its center.
    #[test]
    fn a_two_point_conical_gradient_goes_from_one_circle_to_the_other() {
        let conical = GradientShaderKind::TwoPointConical {
            start: gfx::PointF::new(30.0, 50.0),
            start_radius: 0.0,
            end: gfx::PointF::new(50.0, 50.0),
            end_radius: 50.0,
        };
        let shader = plane(conical, TileMode::Clamp, Transform::IDENTITY);
        let bmp = fill_with(
            shader,
            RectF::new(0.0, 0.0, 100.0, 100.0),
            Size::new(100, 100),
        );
        assert!(bmp.pixel(30, 50)[0] > 240, "{:?}", bmp.pixel(30, 50));
        // As far from the focal point each way, but nearer the end circle
        // to the left: further along.
        let (left, right) = (bmp.pixel(20, 50), bmp.pixel(40, 50));
        assert!(left[2] > right[2] + 30, "{left:?} {right:?}");
        // Just inside the end circle: blue.
        assert!(bmp.pixel(50, 1)[2] > 230, "{:?}", bmp.pixel(50, 1));
    }

    #[test]
    fn unpremultiplied_gradients_mix_the_colors_alone() {
        // From transparent black to red: halfway is half-transparent dark
        // red (premultiplied, it would be half-transparent red).
        let linear = GradientShaderKind::Linear {
            start: gfx::PointF::new(0.0, 0.0),
            end: gfx::PointF::new(100.0, 0.0),
        };
        let PaintShader::Gradient(mut gradient) =
            plane(linear, TileMode::Clamp, Transform::IDENTITY)
        else {
            unreachable!()
        };
        gradient.stops[0].color = Color::TRANSPARENT;
        gradient.stops[1].color = Color::rgb(255, 0, 0);
        let bmp = fill_with(
            gradient,
            RectF::new(0.0, 0.0, 100.0, 10.0),
            Size::new(100, 10),
        );
        // Over white.
        assert!(
            close(bmp.pixel(50, 5), [191, 127, 127, 255]),
            "{:?}",
            bmp.pixel(50, 5)
        );
    }

    /// A variable font's glyph is drawn at the axis values it was shaped
    /// with: WPT's `RobotoExtremo` subset's "x" heavier at `wght` 900 than at
    /// 100, and its instance kept (the same typeface for the same values).
    #[test]
    fn a_variable_font_is_drawn_at_its_axes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../third_party/wpt/css/css-fonts/support/fonts/RobotoExtremo-VF.subset.ttf"
        );
        let bytes = std::fs::read(path).unwrap();
        let typeface = sk::FontMgr::new()
            .new_from_data(sk::Data::new_copy(&bytes), 0)
            .unwrap();
        let font = FontData {
            bytes: Arc::new(bytes),
            index: 0,
            id: 7,
        };
        let glyph = typeface.unichar_to_glyph('x' as i32);
        let cache = RasterCache::default();
        let ink = |weight: Option<f32>| {
            let variations: Arc<[gfx::FontVariation]> = weight
                .map(|value| gfx::FontVariation {
                    tag: *b"wght",
                    value,
                })
                .into_iter()
                .collect();
            crate::ink_bounds(
                &cache,
                &[GlyphRun {
                    font: font.clone(),
                    size: 100.0,
                    glyphs: vec![glyph],
                    positions: vec![PointF::new(0.0, 0.0)],
                    embolden: false,
                    skew: 0.0,
                    variations,
                }],
            )
        };
        let (thin, black, default) = (ink(Some(100.0)), ink(Some(900.0)), ink(None));
        assert!(black.width > default.width, "{black:?} {default:?}");
        assert!(default.width > thin.width, "{default:?} {thin:?}");
        assert_eq!(ink(Some(900.0)), black);
        assert_eq!(cache.tables().instances.len(), 2);
    }

    /// One Ahem "X" (a full em square) at 20px, its origin at `at`, drawn
    /// with `flags` and turned by `rotation` degrees.
    fn ahem_square(at: PointF, rotation: Option<f32>, flags: PaintFlags) -> crate::Bitmap {
        let typeface = sk::FontMgr::new()
            .new_from_data(sk::Data::new_copy(AHEM), 0)
            .unwrap();
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawGlyphs {
            font: FontData {
                bytes: Arc::new(AHEM.to_vec()),
                index: 0,
                id: 1,
            },
            size: 20.0,
            glyphs: vec![typeface.unichar_to_glyph('X' as i32)].into(),
            positions: vec![at].into(),
            rotations: rotation.into_iter().collect(),
            flags,
            embolden: false,
            skew: 0.0,
            variations: Arc::new([]),
            bounds: RectF::new(0.0, 0.0, 40.0, 40.0),
        });
        list.finalize();
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 40.0, 40.0),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        )
    }

    /// Ahem's "X"s are em squares, 0.8em above the baseline; its spaces
    /// have no ink. At 13px the squares' edges fall between pixels: their
    /// outlines' bounds keep them there (Skia's glyph bounds would round
    /// them out to whole pixels).
    #[test]
    fn ink_bounds_are_the_glyphs_outlines_together() {
        let typeface = sk::FontMgr::new()
            .new_from_data(sk::Data::new_copy(AHEM), 0)
            .unwrap();
        let run = |text: &str, x: f32, y: f32| GlyphRun {
            font: FontData {
                bytes: Arc::new(AHEM.to_vec()),
                index: 0,
                id: 1,
            },
            size: 13.0,
            glyphs: text
                .chars()
                .map(|c| typeface.unichar_to_glyph(c as i32))
                .collect(),
            positions: (0..text.len())
                .map(|i| PointF::new(x + i as f32 * 13.0, y))
                .collect(),
            embolden: false,
            skew: 0.0,
            variations: Arc::new([]),
        };
        // Within a thousandth of a pixel: FreeType (Skia's font backend on
        // Linux) loads the outlines in fixed point, 0.0006px off. Rounding
        // out to whole pixels would be 0.4px off or more.
        let close = |a: RectF, b: RectF| {
            let d = [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height];
            assert!(d.iter().all(|d| d.abs() < 1e-3), "{a:?} vs {b:?}");
        };
        let cache = &mut RasterCache::default();
        close(
            ink_bounds(cache, &[run("X X", 0.5, 0.0)]),
            RectF::new(0.5, -10.4, 39.0, 13.0),
        );
        // Each run's glyphs where they are.
        close(
            ink_bounds(cache, &[run("X", 0.0, 0.0), run("X", 100.0, 3.0)]),
            RectF::new(0.0, -10.4, 113.0, 16.0),
        );
        // No ink: an empty rect at the origin.
        assert_eq!(ink_bounds(cache, &[run("  ", 5.0, 5.0)]), RectF::default());
        assert_eq!(ink_bounds(cache, &[]), RectF::default());
        // Made bold and slanted as drawn: past the square.
        let bold = ink_bounds(
            cache,
            &[GlyphRun {
                embolden: true,
                ..run("X", 0.0, 0.0)
            }],
        );
        assert!(bold.x < 0.0 && bold.width > 13.0, "{bold:?}");
        let slanted = ink_bounds(
            cache,
            &[GlyphRun {
                skew: 20.0,
                ..run("X", 0.0, 0.0)
            }],
        );
        assert!(slanted.right() > 16.0, "{slanted:?}");
    }

    /// Glyphs take their flags: stroked, a shader, an alpha.
    #[test]
    fn glyphs_are_filled_or_stroked_with_their_flags() {
        let at = PointF::new(2.0, 18.0);
        // The square covers x and y in [2, 22): stroked, its middle is empty.
        let bmp = ahem_square(at, None, PaintFlags::stroke(Color::BLACK, 4.0));
        assert_eq!(bmp.pixel(2, 12), [0, 0, 0, 255]);
        assert_eq!(bmp.pixel(12, 12), WHITE);
        let linear = GradientShaderKind::Linear {
            start: PointF::new(2.0, 0.0),
            end: PointF::new(22.0, 0.0),
        };
        let ShaderImage::Gradient(gradient) = red_to_blue(linear, false) else {
            unreachable!()
        };
        let bmp = ahem_square(at, None, PaintFlags::shader(gradient));
        assert!(bmp.pixel(3, 12)[0] > 200, "{:?}", bmp.pixel(3, 12));
        assert!(bmp.pixel(21, 12)[2] > 200, "{:?}", bmp.pixel(21, 12));
        let bmp = ahem_square(at, None, PaintFlags::fill(Color::rgba(0, 0, 0, 128)));
        assert!(
            close(bmp.pixel(12, 12), [127, 127, 127, 255]),
            "{:?}",
            bmp.pixel(12, 12)
        );
    }

    /// A rotation turns a glyph about its origin, clockwise: the square
    /// that would go right and up from (12, 12) goes down and left.
    #[test]
    fn glyphs_turn_about_their_origins() {
        let at = PointF::new(12.0, 12.0);
        let black = PaintFlags::fill(Color::BLACK);
        let upright = ahem_square(at, None, black.clone());
        assert_eq!(upright.pixel(20, 5), [0, 0, 0, 255]);
        assert_eq!(upright.pixel(20, 25), WHITE);
        let turned = ahem_square(at, Some(90.0), black);
        assert_eq!(turned.pixel(20, 5), WHITE);
        assert_eq!(turned.pixel(20, 25), [0, 0, 0, 255]);
        // The glyph's y, from -16 to 4, turned to x from 8 to 28.
        assert_eq!(turned.pixel(5, 25), WHITE);
    }
}

/// `gfx::Path`'s shapes (ported from Skia's `SkPathBuilder`) against
/// Skia's own: the same verbs, points and conic weights, bit for bit.
#[cfg(test)]
mod path_shape_tests {
    use gfx::{Path, PointF, SizeF};

    use super::*;

    fn p(x: f32, y: f32) -> PointF {
        PointF::new(x, y)
    }

    fn sk_point(p: PointF) -> sk::Point {
        sk::Point::new(p.x, p.y)
    }

    /// `ours`, built by Skia as raster draws it, is `skia`.
    #[track_caller]
    fn assert_same(ours: &Path, skia: &sk::Path) {
        let ours = sk_path(ours);
        assert_eq!(ours.verbs(), skia.verbs(), "verbs");
        let bits = |points: &[sk::Point]| {
            points
                .iter()
                .map(|p| (p.x.to_bits(), p.y.to_bits()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            bits(ours.points()),
            bits(skia.points()),
            "{:?} vs {:?}",
            ours.points(),
            skia.points()
        );
        let weights = |path: &sk::Path| {
            path.conic_weights()
                .iter()
                .map(|w| w.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(weights(&ours), weights(skia), "weights");
    }

    /// A path that starts at `start` (a move) if any, as both build it.
    fn both(start: Option<PointF>) -> (Path, sk::PathBuilder) {
        let mut ours = Path::new();
        let mut skia = sk::PathBuilder::new();
        if let Some(start) = start {
            ours.move_to(start);
            skia.move_to(sk_point(start));
        }
        (ours, skia)
    }

    #[test]
    fn arcs_of_an_oval_are_skias_conics() {
        let (tl, br) = (p(10.5, 20.25), p(110.0, 80.75));
        let oval = sk::Rect::new(tl.x, tl.y, br.x, br.y);
        for (start, sweep) in [
            (0.0, 90.0),
            (30.0, 200.0),
            (-45.0, -120.0),
            (370.0, 45.0),
            (12.0, 359.5),
            (12.0, -359.5),
            // So near a whole turn that its ends round to one direction.
            (0.0, 359.9999),
            (90.0, 180.0),
            (0.0, 0.0),
            (45.0, 0.0),
            (10.0, 0.001),
            (10.0, 360.0),
            (10.0, 400.0),
        ] {
            for force_move_to in [false, true] {
                for start_at in [None, Some(p(0.0, 0.0)), Some(p(110.0, 50.5))] {
                    let (mut ours, mut skia) = both(start_at);
                    ours.arc_to(tl, br, start, sweep, force_move_to);
                    skia.arc_to(oval, start, sweep, force_move_to);
                    assert_same(&ours, &skia.snapshot());
                }
            }
        }
        // Two arcs of one oval, the second from where the first ended:
        // no line between them.
        let (mut ours, mut skia) = both(None);
        for (start, sweep) in [(15.0, 180.0), (195.0, 180.0)] {
            ours.arc_to(tl, br, start, sweep, false);
            skia.arc_to(oval, start, sweep, false);
        }
        assert_same(&ours, &skia.snapshot());
        // An oval of no size is a point; one turned inside out, nothing.
        for (tl, br) in [(p(5.0, 5.0), p(5.0, 5.0)), (p(5.0, 5.0), p(1.0, 9.0))] {
            let (mut ours, mut skia) = both(Some(p(1.0, 1.0)));
            ours.arc_to(tl, br, 30.0, 90.0, false);
            skia.arc_to(sk::Rect::new(tl.x, tl.y, br.x, br.y), 30.0, 90.0, false);
            assert_same(&ours, &skia.snapshot());
        }
    }

    #[test]
    fn tangent_arcs_are_skias() {
        let cases: [(Option<PointF>, PointF, PointF, f32); 7] = [
            (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, 70.0), 20.0),
            (Some(p(10.0, 10.0)), p(90.0, 15.0), p(30.0, 60.0), 7.5),
            // Turning the other way.
            (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, -50.0), 20.0),
            // On one line, a radius of 0, the same point twice.
            (Some(p(10.0, 10.0)), p(50.0, 10.0), p(90.0, 10.0), 20.0),
            (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, 70.0), 0.0),
            (Some(p(10.0, 10.0)), p(10.0, 10.0), p(90.0, 70.0), 5.0),
            // From the origin.
            (None, p(90.0, 10.0), p(90.0, 70.0), 20.0),
        ];
        for (start, p1, p2, radius) in cases {
            let (mut ours, mut skia) = both(start);
            ours.tangent_arc_to(p1, p2, radius);
            skia.arc_to_tangent(sk_point(p1), sk_point(p2), radius);
            assert_same(&ours, &skia.snapshot());
        }
        // After a close: from the start of the subpath it closed.
        let (mut ours, mut skia) = both(Some(p(10.0, 10.0)));
        ours.line_to(p(50.0, 40.0));
        ours.close();
        skia.line_to((50.0, 40.0));
        skia.close();
        ours.tangent_arc_to(p(90.0, 10.0), p(90.0, 70.0), 15.0);
        skia.arc_to_tangent((90.0, 10.0), (90.0, 70.0), 15.0);
        assert_same(&ours, &skia.snapshot());
    }

    #[test]
    fn rects_and_rounded_rects_are_skias() {
        // A rect from one corner to the other, either way round.
        for (p0, p1) in [
            (p(10.0, 20.0), p(50.0, 70.0)),
            (p(50.0, 20.0), p(10.0, 70.0)),
        ] {
            let (mut ours, mut skia) = both(None);
            ours.add_rect(p0, p1);
            skia.add_rect(
                sk::Rect::new(p0.x, p0.y, p1.x, p1.y),
                sk::PathDirection::CW,
                0,
            );
            assert_same(&ours, &skia.snapshot());
        }
        let s = SizeF::new;
        let rect = gfx::RectF::new(10.25, 20.0, 80.0, 50.5);
        for radii in [
            [s(5.0, 5.0); 4],
            [s(5.0, 3.0), s(10.0, 12.0), s(0.0, 4.0), s(7.0, 7.0)],
            // Too big: scaled to fit, as CSS does.
            [s(60.0, 30.0), s(50.0, 40.0), s(10.0, 35.0), s(45.0, 20.0)],
            // Every corner square: a rect; halfway at every corner: an oval.
            [s(0.0, 9.0), s(0.0, 0.0), s(3.0, 0.0), s(0.0, 0.0)],
            [s(40.0, 25.25); 4],
            [s(100.0, 100.0); 4],
            // Too small to matter beside the other on its side.
            [s(1e-9, 5.0), s(40.0, 5.0), s(5.0, 5.0), s(5.0, 5.0)],
            [s(40.0, 5.0), s(1e-9, 5.0), s(5.0, 5.0), s(5.0, 5.0)],
        ] {
            for clockwise in [true, false] {
                let (mut ours, mut skia) = both(Some(p(1.0, 2.0)));
                ours.line_to(p(3.0, 4.0));
                skia.line_to((3.0, 4.0));
                ours.add_round_rect(rect, radii, clockwise);
                let sk_rect = sk::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                let sk_radii = radii.map(|r| sk::Vector::new(r.width, r.height));
                let direction = if clockwise {
                    sk::PathDirection::CW
                } else {
                    sk::PathDirection::CCW
                };
                skia.add_rrect(sk::RRect::new_rect_radii(sk_rect, &sk_radii), direction, 0);
                assert_same(&ours, &skia.snapshot());
            }
        }
    }

    #[test]
    fn a_transformed_path_is_skias() {
        let mut ours = Path::new();
        ours.line_to(p(10.0, 20.0));
        ours.quad_to(p(30.5, 1.0), p(40.0, 40.0));
        ours.close();
        ours.cubic_to(p(1.0, 2.0), p(3.0, 4.0), p(5.0, 6.0));
        ours.conic_to(p(7.0, 8.0), p(9.0, 1.0), 0.5);
        let skia = sk_path(&ours);
        let t = Transform([0.8, 0.6, -0.6, 0.8, 12.5, -3.0]);
        ours.transform(&t);
        assert_same(&ours, &skia.make_transform(&sk_matrix(&t)));
    }
}
