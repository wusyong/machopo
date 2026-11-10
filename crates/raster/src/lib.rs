//! Raster: plays a [`DisplayItemList`] back into pixels.
//!
//! This is the `RasterSource::PlaybackToCanvas` step of Chromium's raster
//! path. cc decides *what* to raster (which tile, at which scale) and calls
//! [`playback`]; this crate only knows how to turn paint ops into pixels.
//!
//! Output is always tightly-packed RGBA8, premultiplied — the format Viz
//! expects for `protocol::viz::ResourceFormat::Rgba8Premul` resources.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

use compositor_paint::DisplayItemList;
use gfx::{Color, RectF, Size};

mod encode;
mod image_decode;
mod skia_backend;

use skia_backend as backend;

/// Name of the raster backend, for logs.
pub const BACKEND: &str = backend::NAME;

pub use backend::{RasterCache, ink_bounds};
pub use encode::{encode_jpeg, encode_png};
pub use image_decode::image_size;

/// A CPU bitmap, RGBA8 premultiplied, `stride == width * 4`.
#[derive(Clone, PartialEq, Eq)]
pub struct Bitmap {
    pub size: Size,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for Bitmap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Bitmap")
            .field("size", &self.size)
            .finish_non_exhaustive()
    }
}

impl Bitmap {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            pixels: vec![
                0;
                size.checked_byte_size()
                    .expect("a bitmap's bytes fit a usize")
            ],
        }
    }

    /// Premultiplied RGBA at `(x, y)`.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "(x, y) is in the bitmap (else the indexing below panics): the index \
                      is below the pixels' length"
        )]
        let i = (y as usize * self.size.width as usize + x as usize) * 4;
        *self.pixels[i..]
            .first_chunk()
            .expect("(x, y) is in the bitmap")
    }
}

/// What to raster: a window onto the layer's content at a given scale
/// (`raster_transform` in Chromium, restricted to scale + translate).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlaybackParams {
    /// Region of the *scaled* content to produce, in physical pixels. Its size
    /// is the output bitmap size; its origin is where the bitmap sits in the
    /// layer (a tile's position).
    pub content_rect: RectF,
    /// CSS px -> physical px (device scale factor x page zoom).
    pub contents_scale: f32,
    /// Cleared before playback. `Color::TRANSPARENT` for non-opaque layers.
    pub clear_color: Color,
}

/// Rasters `list` according to `params`, with what earlier playbacks left
/// in `cache`: [`playback_ops`] of [`ops_for`].
pub fn playback(list: &DisplayItemList, params: &PlaybackParams, cache: &RasterCache) -> Bitmap {
    playback_ops(list, &ops_for(list, params), params, cache)
}

/// The ops of `list` that draw in what `params` rasters: those whose visual
/// rect meets its content rect (in the layer's space, a device pixel more
/// for anti-aliasing at its edges), with the state they are drawn with
/// (cc's `DisplayItemList::Raster` searching its R-tree for the playback
/// rect). None: what `params` rasters stays the clear color.
pub fn ops_for(list: &DisplayItemList, params: &PlaybackParams) -> Vec<usize> {
    let scale = params.contents_scale;
    list.ops_in(params.content_rect.inset(-1.0).scale(1.0 / scale))
}

/// Rasters `ops` of `list` (indices into its ops, in order: [`ops_for`]'s)
/// according to `params`.
pub fn playback_ops(
    list: &DisplayItemList,
    ops: &[usize],
    params: &PlaybackParams,
    cache: &RasterCache,
) -> Bitmap {
    let mut pixels = Vec::new();
    let size = playback_into(list, ops, params, cache, &mut pixels);
    Bitmap { size, pixels }
}

/// [`playback_ops`] into `pixels`, made the size's (RGBA8 premultiplied,
/// `stride == width * 4`): a buffer a raster thread draws tile after tile
/// into, keeping its memory. Returns the size.
pub fn playback_into(
    list: &DisplayItemList,
    ops: &[usize],
    params: &PlaybackParams,
    cache: &RasterCache,
    pixels: &mut Vec<u8>,
) -> Size {
    // Rounded up, a negative or NaN one empty.
    let size = Size::new(
        base::numerics::clamp_ceil(params.content_rect.width),
        base::numerics::clamp_ceil(params.content_rect.height),
    );
    let _span = trace_event::trace_event!(
        "raster::playback",
        w = size.width,
        h = size.height,
        ops = ops.len(),
        of = list.ops().len()
    );
    // Every pixel is cleared before anything is drawn: what was there
    // doesn't show.
    let len = size
        .checked_byte_size()
        .expect("a tile's pixels fit a usize");
    pixels.resize(len, 0);
    if !size.is_empty() {
        backend::playback(list, ops, params, cache, size, pixels);
    }
    size
}

/// Plays `list` back onto a canvas's bitmap, `pixels` of `size` (RGBA8
/// premultiplied, `stride == width * 4`), over what is drawn there
/// already, one unit a pixel: what a 2D context recorded since its last
/// flush (Blink's `CanvasResourceProvider` playing the recording). An
/// `opaque` bitmap (`alpha: false`) stays opaque. Nothing for an empty
/// size.
///
/// # Panics
///
/// If `pixels` isn't `size`'s.
pub fn draw_into(
    list: &DisplayItemList,
    cache: &RasterCache,
    size: Size,
    opaque: bool,
    pixels: &mut [u8],
) {
    assert_eq!(
        Some(pixels.len()),
        size.checked_byte_size(),
        "the pixels are the size's"
    );
    if size.is_empty() {
        return;
    }
    let _span = trace_event::trace_event!("raster::draw_into", w = size.width, h = size.height);
    if cfg!(debug_assertions) {
        DRAWS_INTO.set(DRAWS_INTO.get().wrapping_add(1));
    }
    backend::draw_into(list, cache, size, opaque, pixels);
}

thread_local! {
    /// How many times a canvas's bitmap was drawn on (`draw_into`), on
    /// this thread. Counted in debug builds only.
    static DRAWS_INTO: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How many times, on this thread, a canvas's bitmap was drawn on: what
/// tells which thread canvases are drawn on. `None` in release builds. For
/// tests.
pub fn draws_into_for_testing() -> Option<usize> {
    cfg!(debug_assertions).then(|| DRAWS_INTO.get())
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor_paint::{PaintFlags, PaintOp};

    #[test]
    fn drawing_into_a_bitmap_keeps_what_it_has() {
        let size = Size::new(4, 1);
        let mut pixels = vec![0; 16];
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 2.0, 1.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        draw_into(&list, &RasterCache::default(), size, false, &mut pixels);
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(1.0, 0.0, 2.0, 1.0),
            flags: PaintFlags::fill(Color::rgba(0, 0, 255, 128)),
        });
        // Cleared: transparent black, whatever was under it.
        list.push_op(PaintOp::ClearRect {
            rect: RectF::new(0.0, 0.0, 1.0, 1.0),
        });
        draw_into(&list, &RasterCache::default(), size, false, &mut pixels);
        assert_eq!(&pixels[0..4], [0, 0, 0, 0]);
        // Half blue over red, premultiplied.
        assert_eq!(&pixels[4..8], [127, 0, 128, 255]);
        assert_eq!(&pixels[8..12], [0, 0, 128, 128]);
        assert_eq!(&pixels[12..16], [0, 0, 0, 0]);
    }

    /// `image`, 8×1, drawn into one pixel with `filter`.
    fn shrunk(filter: compositor_paint::FilterQuality) -> [u8; 4] {
        // Six red pixels, then two blue.
        let mut pixels = Vec::new();
        for i in 0..8 {
            pixels.extend(if i < 6 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            });
        }
        let image = gfx::ImageData::decoded(Size::new(8, 1), pixels);
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawImage {
            image,
            src: None,
            dst: RectF::new(0.0, 0.0, 1.0, 1.0),
            filter,
            flags: None,
        });
        list.finalize();
        playback(&list, &params(1.0, 1.0, 1.0), &RasterCache::default()).pixel(0, 0)
    }

    /// A canvas's snapshot that another thread fills in is waited for (a
    /// tile's raster waiting for the canvas raster thread); one whose
    /// pixels never come draws nothing.
    #[test]
    fn a_pending_snapshot_is_drawn_once_filled() {
        let drawn = |image: gfx::ImageData| {
            let mut list = DisplayItemList::new();
            list.push_op(PaintOp::DrawImage {
                image,
                src: None,
                dst: RectF::new(0.0, 0.0, 1.0, 1.0),
                filter: compositor_paint::FilterQuality::None,
                flags: None,
            });
            list.finalize();
            playback(&list, &params(1.0, 1.0, 1.0), &RasterCache::default()).pixel(0, 0)
        };
        let (image, filler) = gfx::ImageData::pending(Size::new(1, 1));
        let fill = std::thread::spawn(move || {
            // Late enough that the raster is waiting by then.
            std::thread::sleep(std::time::Duration::from_millis(50));
            filler.fill(std::sync::Arc::new(vec![255, 0, 0, 255]));
        });
        assert_eq!(drawn(image), [255, 0, 0, 255]);
        fill.join().unwrap();
        let (never, filler) = gfx::ImageData::pending(Size::new(1, 1));
        drop(filler);
        assert_eq!(drawn(never), [255, 255, 255, 255], "the clear color");
    }

    #[test]
    fn a_decoded_image_is_drawn_as_its_pixels_sampled_as_its_filter_says() {
        // Bilinear without mipmaps samples round the middle: red.
        let [r, _, b, _] = shrunk(compositor_paint::FilterQuality::Low);
        assert!(r > 250 && b < 5, "{r} {b}");
        // With mipmaps, the average: three quarters red.
        let [r, _, b, _] = shrunk(compositor_paint::FilterQuality::Medium);
        assert!(
            (180..=200).contains(&r) && (55..=75).contains(&b),
            "{r} {b}"
        );
    }

    #[test]
    fn an_opaque_bitmap_stays_opaque() {
        let size = Size::new(1, 1);
        let mut pixels = vec![0, 0, 0, 255];
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 1.0, 1.0),
            flags: PaintFlags::fill(Color::rgba(255, 0, 0, 128)),
        });
        draw_into(&list, &RasterCache::default(), size, true, &mut pixels);
        assert_eq!(pixels, [128, 0, 0, 255]);
    }

    fn params(w: f32, h: f32, scale: f32) -> PlaybackParams {
        PlaybackParams {
            content_rect: RectF::new(0.0, 0.0, w, h),
            contents_scale: scale,
            clear_color: Color::WHITE,
        }
    }

    #[test]
    fn fills_rect() {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(2.0, 2.0, 4.0, 4.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        list.finalize();
        let bmp = playback(&list, &params(8.0, 8.0, 1.0), &RasterCache::default());
        assert_eq!(bmp.size, Size::new(8, 8));
        assert_eq!(bmp.pixel(0, 0), [255, 255, 255, 255]);
        assert_eq!(bmp.pixel(3, 3), [255, 0, 0, 255]);
        assert_eq!(bmp.pixel(6, 6), [255, 255, 255, 255]);
    }

    #[test]
    fn scale_and_tile_offset() {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(10.0, 0.0, 10.0, 10.0),
            flags: PaintFlags::fill(Color::rgb(0, 0, 255)),
        });
        list.finalize();
        // At 2x the rect covers physical x in [20, 40). Raster the tile that
        // starts at x = 16.
        let p = PlaybackParams {
            content_rect: RectF::new(16.0, 0.0, 16.0, 16.0),
            ..params(0.0, 0.0, 2.0)
        };
        let bmp = playback(&list, &p, &RasterCache::default());
        assert_eq!(bmp.pixel(2, 2), [255, 255, 255, 255]); // x = 18
        assert_eq!(bmp.pixel(5, 2), [0, 0, 255, 255]); // x = 21
    }

    #[test]
    fn clip_and_translate() {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::Translate { dx: 4.0, dy: 0.0 });
        list.push_op(PaintOp::ClipRect {
            rect: RectF::new(0.0, 0.0, 2.0, 8.0),
            anti_alias: false,
        });
        list.push_op(PaintOp::DrawColor {
            color: Color::rgb(0, 255, 0),
            blend_mode: compositor_paint::BlendMode::SrcOver,
        });
        list.push_op(PaintOp::Restore);
        list.finalize();
        let bmp = playback(&list, &params(8.0, 8.0, 1.0), &RasterCache::default());
        assert_eq!(bmp.pixel(3, 3), [255, 255, 255, 255]);
        assert_eq!(bmp.pixel(4, 3), [0, 255, 0, 255]);
        assert_eq!(bmp.pixel(5, 3), [0, 255, 0, 255]);
        assert_eq!(bmp.pixel(6, 3), [255, 255, 255, 255]);
    }
    #[test]
    fn ahem_glyph_is_a_crisp_square() {
        use std::sync::Arc;

        use gfx::{FontData, PointF};
        static AHEM: &[u8] = include_bytes!("../../../resources/test_fonts/Ahem.ttf");
        let font = FontData {
            bytes: Arc::new(AHEM),
            index: 0,
            id: 1,
        };
        let typeface = skia_safe::FontMgr::new()
            .new_from_data(skia_safe::Data::new_copy(AHEM), 0)
            .unwrap();
        let glyph = typeface.unichar_to_glyph('X' as i32);
        let mut list = DisplayItemList::new();
        // Ahem: ascent 0.8em, descent 0.2em. At 20px on a baseline at y = 18
        // the glyph covers x in [2, 22), y in [2, 22).
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
        let bmp = playback(&list, &params(24.0, 24.0, 1.0), &RasterCache::default());
        for y in 0..24 {
            for x in 0..24 {
                let inside = (2..22).contains(&x) && (2..22).contains(&y);
                let expected = if inside {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                assert_eq!(bmp.pixel(x, y), expected, "({x}, {y})");
            }
        }
    }

    #[test]
    fn set_matrix_is_relative_to_the_tile() {
        use gfx::Transform;
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::Translate { dx: 100.0, dy: 0.0 });
        // Replaces the translate, but keeps the tile offset and scale.
        list.push_op(PaintOp::SetMatrix {
            transform: Transform::translate(20.0, 0.0),
        });
        // Its visual rect in the layer's space.
        list.push(
            PaintOp::DrawRect {
                rect: RectF::new(0.0, 0.0, 2.0, 2.0),
                flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
            },
            Some(RectF::new(20.0, 0.0, 2.0, 2.0)),
        );
        list.push_op(PaintOp::Restore);
        list.finalize();
        // A 2x tile starting at x = 16 (layer space): the rect is at layer
        // x 20..22, so tile pixels (20 - 16) * 2 .. = 8..12.
        let p = PlaybackParams {
            content_rect: RectF::new(32.0, 0.0, 16.0, 16.0),
            ..params(0.0, 0.0, 2.0)
        };
        let bmp = playback(&list, &p, &RasterCache::default());
        assert_eq!(bmp.pixel(7, 1), [255, 255, 255, 255]);
        assert_eq!(bmp.pixel(8, 1), [255, 0, 0, 255]);
        assert_eq!(bmp.pixel(11, 1), [255, 0, 0, 255]);
        assert_eq!(bmp.pixel(12, 1), [255, 255, 255, 255]);
    }

    #[test]
    fn save_layer_alpha_blends_the_group_once() {
        use gfx::Transform;
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::SaveLayerAlpha {
            bounds: None,
            alpha: 0.5,
        });
        // Two overlapping opaque rects: inside the group the overlap is
        // just black, so the whole group is 50% black.
        for x in [0.0, 2.0] {
            list.push_op(PaintOp::DrawRect {
                rect: RectF::new(x, 0.0, 4.0, 4.0),
                flags: PaintFlags::fill(Color::BLACK),
            });
        }
        list.push_op(PaintOp::Restore);
        list.push_op(PaintOp::Concat {
            transform: Transform::translate(0.0, 4.0),
        });
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 1.0, 1.0),
            flags: PaintFlags::fill(Color::BLACK),
        });
        list.finalize();
        let bmp = playback(&list, &params(8.0, 8.0, 1.0), &RasterCache::default());
        let [r, _, _, a] = bmp.pixel(1, 1);
        assert_eq!(bmp.pixel(3, 1), [r, r, r, a], "the overlap is the same");
        assert!((126..=129).contains(&r), "{r}");
        // `Concat` applies after the layer: the last rect is at y = 4.
        assert_eq!(bmp.pixel(0, 4), [0, 0, 0, 255]);
    }

    #[test]
    fn path_polygons_share_edges_without_seams() {
        use gfx::PointF;
        let p = PointF::new;
        let mut list = DisplayItemList::new();
        // An 8x8 square as two triangles meeting on the diagonal.
        list.push_op(PaintOp::DrawPath {
            path: gfx::Path::polygons([
                vec![p(0.0, 0.0), p(8.0, 0.0), p(8.0, 8.0)],
                vec![p(0.0, 0.0), p(8.0, 8.0), p(0.0, 8.0)],
            ]),
            flags: PaintFlags::fill(Color::rgb(0, 0, 255)),
        });
        list.finalize();
        assert_eq!(list.bounds(), RectF::new(0.0, 0.0, 8.0, 8.0));
        let bmp = playback(&list, &params(8.0, 8.0, 1.0), &RasterCache::default());
        // Every pixel, including those the diagonal crosses, is fully blue.
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(bmp.pixel(x, y), [0, 0, 255, 255], "({x}, {y})");
            }
        }
    }

    /// A buffer drawn into tile after tile: each comes out as drawn alone,
    /// none of the last one's pixels left, at its own size.
    #[test]
    fn a_buffer_reused_holds_only_the_last_tile() {
        let mut red = DisplayItemList::new();
        red.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 8.0, 8.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        red.finalize();
        let mut empty = DisplayItemList::new();
        empty.finalize();
        let cache = RasterCache::default();
        let mut buffer = Vec::new();
        let size = playback_into(&red, &[0], &params(8.0, 8.0, 1.0), &cache, &mut buffer);
        assert_eq!(size, Size::new(8, 8));
        assert_eq!(&buffer[..4], &[255, 0, 0, 255]);
        let transparent = PlaybackParams {
            clear_color: Color::TRANSPARENT,
            ..params(4.0, 2.0, 1.0)
        };
        let size = playback_into(&empty, &[], &transparent, &cache, &mut buffer);
        assert_eq!(size, Size::new(4, 2));
        assert_eq!(buffer, vec![0; 4 * 2 * 4]);
    }

    #[test]
    fn only_the_ops_given_are_played() {
        let mut list = DisplayItemList::new();
        for x in [0.0, 4.0] {
            list.push_op(PaintOp::DrawRect {
                rect: RectF::new(x, 0.0, 4.0, 4.0),
                flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
            });
        }
        list.finalize();
        let bmp = playback_ops(&list, &[1], &params(8.0, 4.0, 1.0), &RasterCache::default());
        assert_eq!(bmp.pixel(1, 1), [255, 255, 255, 255]);
        assert_eq!(bmp.pixel(5, 1), [255, 0, 0, 255]);
    }

    /// xorshift64*: the same lists for the same seed, no dependencies.
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 >> 12;
            self.0 ^= self.0 << 25;
            self.0 ^= self.0 >> 27;
            self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() >> 33) as usize % n
        }

        fn between(&mut self, low: f32, high: f32) -> f32 {
            low + (self.next() >> 40) as f32 / (1u64 << 24) as f32 * (high - low)
        }

        fn rect(&mut self) -> RectF {
            RectF::new(
                self.between(-20.0, 300.0),
                self.between(-20.0, 300.0),
                self.between(0.0, 120.0),
                self.between(0.0, 120.0),
            )
        }
    }

    /// A display list of nested saves, groups (some with a shadow's filter),
    /// clips, transforms (rotations among them) and drawing ops, each with
    /// its visual rect in the layer's space, as paint gives them; some saves
    /// left open, and now and then a `DrawColor`, which has none.
    fn random_list(rng: &mut Rng) -> DisplayItemList {
        use compositor_paint::{ImageFilter, LayerFlags};
        use gfx::{PointF, RRectF, Transform};
        let mut list = DisplayItemList::new();
        let mut matrix = Transform::IDENTITY;
        let mut saved = Vec::new();
        let color = |rng: &mut Rng| {
            let c = [
                Color::rgb(255, 0, 0),
                Color::rgb(0, 0, 255),
                Color::rgba(0, 128, 0, 160),
            ];
            c[rng.below(3)]
        };
        for _ in 0..1 + rng.below(60) {
            match rng.below(16) {
                0 | 1 => {
                    list.push_op(PaintOp::Save);
                    saved.push(matrix);
                }
                2 | 3 if !saved.is_empty() => {
                    list.push_op(PaintOp::Restore);
                    matrix = saved.pop().unwrap();
                }
                4 => {
                    list.push_op(PaintOp::SaveLayerAlpha {
                        bounds: None,
                        alpha: 0.5,
                    });
                    saved.push(matrix);
                }
                5 if rng.below(3) == 0 => {
                    let filter = ImageFilter::DropShadow {
                        offset: (rng.between(-60.0, 60.0), rng.between(-60.0, 60.0)),
                        sigma: 2.0,
                        color: Color::BLACK,
                        shadow_only: false,
                    };
                    list.push_op(PaintOp::SaveLayer {
                        bounds: None,
                        flags: LayerFlags {
                            image_filter: Some(filter),
                            ..LayerFlags::default()
                        },
                    });
                    saved.push(matrix);
                }
                6 => list.push_op(PaintOp::ClipRect {
                    rect: rng.rect(),
                    anti_alias: true,
                }),
                7 => list.push_op(PaintOp::ClipRRect {
                    rrect: RRectF::uniform(rng.rect(), 8.0),
                    anti_alias: true,
                }),
                8 => {
                    let transform = if rng.below(2) == 0 {
                        let s = rng.between(0.5, 2.0);
                        Transform([
                            s,
                            0.0,
                            0.0,
                            s,
                            rng.between(-50.0, 50.0),
                            rng.between(-50.0, 50.0),
                        ])
                    } else {
                        let (sin, cos) = rng.between(0.0, 6.3).sin_cos();
                        Transform([
                            cos,
                            sin,
                            -sin,
                            cos,
                            rng.between(0.0, 200.0),
                            rng.between(0.0, 200.0),
                        ])
                    };
                    list.push_op(PaintOp::SetMatrix { transform });
                    matrix = transform;
                }
                9 => {
                    let (dx, dy) = (rng.between(-40.0, 40.0), rng.between(-40.0, 40.0));
                    list.push_op(PaintOp::Translate { dx, dy });
                    matrix = matrix.then(&Transform::translate(dx, dy));
                }
                10 | 11 => {
                    // Now and then a big one, as a page's background.
                    let rect = if rng.below(3) == 0 {
                        let (x, y) = (rng.between(-50.0, 100.0), rng.between(-50.0, 100.0));
                        RectF::new(x, y, rng.between(200.0, 900.0), rng.between(200.0, 900.0))
                    } else {
                        rng.rect()
                    };
                    let flags = PaintFlags::fill(color(rng));
                    list.push(
                        PaintOp::DrawRect { rect, flags },
                        Some(matrix.map_rect(rect)),
                    );
                }
                12 => {
                    let (rect, width) = (rng.rect(), rng.between(1.0, 9.0));
                    let flags = PaintFlags::stroke(color(rng), width);
                    let visual = matrix.map_rect(rect.inset(-width / 2.0));
                    list.push(PaintOp::DrawRect { rect, flags }, Some(visual));
                }
                13 => {
                    let rrect = RRectF::uniform(rng.rect(), rng.between(0.0, 30.0));
                    let visual = matrix.map_rect(rrect.rect);
                    let flags = PaintFlags::fill(color(rng));
                    list.push(PaintOp::DrawRRect { rrect, flags }, Some(visual));
                }
                14 => {
                    // A blurred shadow, reaching 3 sigmas past its shape.
                    let shape = RRectF::uniform(rng.rect(), 4.0);
                    let blur = rng.between(0.5, 6.0);
                    let visual = matrix.map_rect(shape.rect.inset(-3.0 * blur));
                    let op = PaintOp::DrawShadow {
                        shape,
                        hole: None,
                        clip: shape,
                        inside: false,
                        flags: PaintFlags {
                            blur,
                            ..PaintFlags::fill(Color::BLACK)
                        },
                    };
                    list.push(op, Some(visual));
                }
                15 if rng.below(8) == 0 => list.push(
                    PaintOp::DrawColor {
                        color: Color::rgba(255, 255, 0, 40),
                        blend_mode: compositor_paint::BlendMode::SrcOver,
                    },
                    None,
                ),
                _ => {
                    let (p0, p1) = (
                        PointF::new(rng.between(0.0, 300.0), rng.between(0.0, 300.0)),
                        PointF::new(rng.between(0.0, 300.0), rng.between(0.0, 300.0)),
                    );
                    let width = rng.between(1.0, 6.0);
                    let local = RectF::new(
                        p0.x.min(p1.x),
                        p0.y.min(p1.y),
                        (p0.x - p1.x).abs(),
                        (p0.y - p1.y).abs(),
                    );
                    let visual = matrix.map_rect(local.inset(-width / 2.0));
                    let flags = PaintFlags::stroke(color(rng), width);
                    list.push(PaintOp::DrawLine { p0, p1, flags }, Some(visual));
                }
            }
        }
        // Some saves left open.
        for _ in 0..rng.below(saved.len() + 1) {
            list.push_op(PaintOp::Restore);
        }
        list.finalize();
        list
    }

    /// Playing only the ops a tile's rect finds draws it as playing them
    /// all does, pixel for pixel, at any scale; and the index leaves ops
    /// out. Where the ops say the tile is of one color, every pixel is.
    #[test]
    fn the_ops_a_tile_finds_draw_it_as_all_of_them() {
        let mut rng = Rng(0x5eed_0057);
        let cache = RasterCache::default();
        let (mut played, mut all) = (0, 0);
        let (mut blank, mut covered) = (0, 0);
        for case in 0..1000 {
            let list = random_list(&mut rng);
            let every: Vec<usize> = (0..list.ops().len()).collect();
            for _ in 0..6 {
                let scale = [1.0, 1.5, 2.0][rng.below(3)];
                let params = PlaybackParams {
                    content_rect: RectF::new(
                        (rng.between(-40.0, 700.0)).floor(),
                        (rng.between(-40.0, 700.0)).floor(),
                        64.0,
                        64.0,
                    ),
                    contents_scale: scale,
                    clear_color: [Color::TRANSPARENT, Color::WHITE][rng.below(2)],
                };
                let ops = ops_for(&list, &params);
                played += ops.len();
                all += every.len();
                let found = playback_ops(&list, &ops, &params, &cache);
                let whole = playback_ops(&list, &every, &params, &cache);
                assert!(
                    found.pixels == whole.pixels,
                    "case {case}: {params:?}, {} of {} ops",
                    ops.len(),
                    every.len()
                );
                let solid = list.color_if_solid_in_rect(
                    &ops,
                    params.content_rect,
                    scale,
                    params.clear_color,
                );
                if let Some(color) = solid {
                    let pixel = [color.r, color.g, color.b, color.a];
                    let pixel = if color.a == 0 { [0; 4] } else { pixel };
                    assert!(
                        whole.pixels.chunks(4).all(|p| p == pixel),
                        "case {case}: {params:?} isn't all {color:?}"
                    );
                    if color == params.clear_color || color.a == 0 {
                        blank += 1;
                    } else {
                        covered += 1;
                    }
                }
            }
        }
        assert!(played * 2 < all, "{played} of {all} ops played");
        assert!(
            blank > 100 && covered > 20,
            "{blank} blank, {covered} covered"
        );
    }
}
