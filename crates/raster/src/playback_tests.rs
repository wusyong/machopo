//! Tests of playback: display lists drawn into pixels, by tile and whole,
//! and onto a canvas's bitmap.

use std::sync::Arc;

use compositor_paint::{
    BlendMode, ColorInterpolation, DisplayItemList, FilterQuality, GradientShader,
    GradientShaderKind, ImageFilter, LayerFlags, LineCap, LineJoin, PaintFlags, PaintOp,
    PaintShader, ShaderImage, TileMode, TiledShader,
};
use gfx::{Color, FillRule, FontData, ImageData, PointF, RectF, Size, Transform};

use crate::{PlaybackParams, RasterCache};

static AHEM: &[u8] = include_bytes!("../../../resources/test_fonts/Ahem.ttf");
static LATO: &[u8] = include_bytes!("../../../resources/test_fonts/Lato-Medium.ttf");

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
    assert_eq!(
        cache.vello.image_dimensions(&ImageData::new(turned)),
        Some((30, 40))
    );
}

/// Pixels with alpha are drawn premultiplied: half transparent red over
/// white is pink.
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
    assert!(cache.vello.has_image(id));
    // Another compositor's cache has none of it.
    let other = RasterCache::default();
    draw_with(&other, ImageData::new(GREEN_BLUE), rect, Size::new(20, 10));
    assert!(!other.vello.has_image(id));
    assert!(cache.vello.has_image(id));
    drop(data);
    // Cached entries whose data is gone go when the next image comes in.
    draw_with(&cache, ImageData::new(GREEN_BLUE), rect, Size::new(20, 10));
    assert!(!cache.vello.has_image(id));
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
    assert_eq!(cache.vello.image_count(), 1);
    assert_eq!(decodes(&cache), 1);
}

fn decodes(cache: &RasterCache) -> usize {
    cache.vello.decodes()
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
    let PaintShader::Gradient(mut gradient) = plane(linear, TileMode::Clamp, Transform::IDENTITY)
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

/// One Ahem "X" (a full em square) at 20px, its origin at `at`, drawn
/// with `flags` and turned by `rotation` degrees.
fn ahem_square(at: PointF, rotation: Option<f32>, flags: PaintFlags) -> crate::Bitmap {
    use skrifa::MetadataProvider;
    let x = skrifa::FontRef::new(AHEM)
        .unwrap()
        .charmap()
        .map('X')
        .unwrap();
    let mut list = DisplayItemList::new();
    list.push_op(PaintOp::DrawGlyphs {
        font: FontData {
            bytes: Arc::new(AHEM.to_vec()),
            index: 0,
            id: 1,
        },
        size: 20.0,
        glyphs: vec![u16::try_from(x.to_u32()).unwrap()].into(),
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

/// Glyph ids of `text` in `font`.
fn glyph_ids(font: &[u8], text: &str) -> Vec<u16> {
    use skrifa::MetadataProvider;
    let charmap = skrifa::FontRef::new(font).unwrap().charmap();
    text.chars()
        .map(|c| u16::try_from(charmap.map(c).unwrap().to_u32()).unwrap())
        .collect()
}

/// `text` in `font` (whose id is `id`) at `size`, its glyphs `advance`
/// apart from `at`.
fn text_run(
    (font, id): (&[u8], u64),
    text: &str,
    size: f32,
    at: PointF,
    advance: f32,
    flags: PaintFlags,
) -> PaintOp {
    let glyphs = glyph_ids(font, text);
    let positions: Vec<PointF> = (0..glyphs.len())
        .map(|i| PointF::new(at.x + advance * i as f32, at.y))
        .collect();
    PaintOp::DrawGlyphs {
        font: FontData {
            bytes: Arc::new(font.to_vec()),
            index: 0,
            id,
        },
        size,
        glyphs: glyphs.into(),
        positions: positions.into(),
        rotations: Vec::new().into(),
        flags,
        embolden: false,
        skew: 0.0,
        variations: Arc::new([]),
        bounds: RectF::new(-1000.0, -1000.0, 3000.0, 3000.0),
    }
}

fn list_of(ops: Vec<PaintOp>) -> DisplayItemList {
    let mut list = DisplayItemList::new();
    for op in ops {
        list.push_op(op);
    }
    list.finalize();
    list
}

fn layer(filter: ImageFilter, ops: Vec<PaintOp>) -> Vec<PaintOp> {
    let mut all = vec![PaintOp::SaveLayer {
        bounds: None,
        flags: LayerFlags {
            image_filter: Some(filter),
            ..LayerFlags::default()
        },
    }];
    all.extend(ops);
    all.push(PaintOp::Restore);
    all
}

/// The matrix that inverts the colors and keeps the alpha.
fn invert() -> [f32; 20] {
    let mut invert = [0.0; 20];
    for row in 0..3 {
        invert[row * 5 + row] = -1.0;
        invert[row * 5 + 4] = 1.0;
    }
    invert[18] = 1.0;
    invert
}

const BLEND_MODES: [BlendMode; 27] = [
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
];

/// One of everything a display list can hold: each op, each filter (and
/// chains of chains), each blend mode on a draw, a layer and a fill of the
/// clip, each kind of shader.
fn one_of_everything() -> Vec<PaintOp> {
    let p = PointF::new;
    let rect = RectF::new(4.0, 4.0, 40.0, 30.0);
    let rrect = gfx::RRectF::uniform(rect, 6.0);
    let green = PaintFlags::fill(Color::rgba(0, 128, 0, 200));
    let triangle = gfx::Path::polygons([vec![p(2.0, 2.0), p(50.0, 8.0), p(10.0, 44.0)]]);
    let blur = ImageFilter::Blur { sigma: 3.0 };
    let shadow = |shadow_only| ImageFilter::DropShadow {
        offset: (3.0, -2.0),
        sigma: 2.0,
        color: Color::rgba(0, 0, 0, 128),
        shadow_only,
    };
    let matrix = ImageFilter::ColorMatrix(invert());
    let chain = ImageFilter::Chain([blur.clone(), matrix.clone(), shadow(false)].into());
    let nested = ImageFilter::Chain([chain.clone(), shadow(true), chain.clone()].into());
    let record = Arc::new({
        let mut record = DisplayItemList::record();
        record.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 5.0, 5.0),
            flags: green.clone(),
        });
        record.push_op(text_run(
            (AHEM, 1),
            "X",
            8.0,
            p(1.0, 9.0),
            8.0,
            PaintFlags::fill(Color::BLACK),
        ));
        record.finalize();
        record
    });
    let gradient = |kind| GradientShader {
        kind,
        stops: [
            (0.0, Color::rgb(255, 0, 0)),
            (0.5, Color::rgba(0, 0, 255, 100)),
            (1.0, Color::rgb(0, 128, 0)),
            (1.0, Color::rgb(0, 0, 0)),
        ]
        .map(|(offset, color)| compositor_paint::GradientStop { offset, color })
        .into(),
        tile_mode: TileMode::Mirror,
        transform: Transform::translate(2.0, 1.0),
        interpolation: ColorInterpolation::Unpremultiplied,
    };
    let kinds = [
        GradientShaderKind::Linear {
            start: p(0.0, 0.0),
            end: p(20.0, 10.0),
        },
        GradientShaderKind::TwoPointConical {
            start: p(10.0, 10.0),
            start_radius: 2.0,
            end: p(14.0, 12.0),
            end_radius: 20.0,
        },
        GradientShaderKind::Sweep {
            center: p(20.0, 20.0),
            start_angle: 30.0,
            end_angle: 300.0,
        },
    ];
    let tiled = |image, decal| TiledShader {
        image,
        tile: RectF::new(3.0, 2.0, 12.0, 9.0),
        spacing: gfx::SizeF::new(2.0, 3.0),
        repeat_x: true,
        repeat_y: false,
        transform: Transform::translate(1.5, 0.5),
        filter: FilterQuality::Low,
        decal,
    };
    let mut shaders: Vec<PaintShader> = vec![PaintShader::Empty];
    for kind in kinds {
        shaders.push(gradient(kind).into());
        shaders.push(tiled(ShaderImage::Gradient(gradient(kind)), false).into());
    }
    shaders.push(tiled(ShaderImage::Image(ImageData::new(GREEN_BLUE)), true).into());
    shaders.push(
        tiled(
            ShaderImage::Record {
                record: record.clone(),
                size: gfx::SizeF::new(10.0, 10.0),
            },
            false,
        )
        .into(),
    );

    let mut ops = vec![
        PaintOp::Save,
        PaintOp::Translate { dx: 1.5, dy: 2.5 },
        PaintOp::ClipRect {
            rect: RectF::new(0.0, 0.0, 90.0, 90.0),
            anti_alias: false,
        },
        PaintOp::ClipRRect {
            rrect: gfx::RRectF::uniform(RectF::new(0.0, 0.0, 88.0, 88.0), 9.0),
            anti_alias: true,
        },
        PaintOp::Concat {
            transform: Transform::translate(1.0, 1.0),
        },
        PaintOp::DrawColor {
            color: Color::rgba(200, 200, 255, 255),
            blend_mode: BlendMode::SrcOver,
        },
        PaintOp::DrawRect {
            rect,
            flags: green.clone(),
        },
        PaintOp::DrawRRect {
            rrect,
            flags: PaintFlags::stroke(Color::rgb(255, 0, 0), 3.0),
        },
        PaintOp::DrawDRRect {
            outer: rrect,
            inner: rrect.inset(4.0, 5.0, 6.0, 7.0),
            clip: Some(vec![p(0.0, 0.0), p(50.0, 0.0), p(25.0, 25.0)].into()),
            flags: green.clone(),
        },
        PaintOp::DrawLine {
            p0: p(0.0, 50.0),
            p1: p(60.0, 52.0),
            flags: PaintFlags {
                dash: compositor_paint::Dash::new(&[4.0, 0.0, 2.0], 1.0),
                cap: LineCap::Round,
                join: LineJoin::Bevel,
                ..PaintFlags::stroke(Color::BLACK, 2.0)
            },
        },
        PaintOp::DrawClipped {
            polygon: vec![p(0.0, 0.0), p(30.0, 0.0), p(0.0, 30.0)].into(),
            op: Box::new(PaintOp::DrawRRect {
                rrect,
                flags: green.clone(),
            }),
        },
        PaintOp::DrawShadow {
            shape: rrect.offset(4.0, 4.0),
            hole: None,
            clip: rrect,
            inside: false,
            flags: PaintFlags {
                blur: 3.0,
                ..PaintFlags::fill(Color::BLACK)
            },
        },
        PaintOp::DrawShadow {
            shape: rrect,
            hole: Some(rrect.inset(3.0, 3.0, 3.0, 3.0).offset(2.0, 1.0)),
            clip: rrect,
            inside: true,
            flags: PaintFlags {
                blur: 2.0,
                ..PaintFlags::fill(Color::rgba(0, 0, 0, 128))
            },
        },
        PaintOp::DrawPath {
            path: triangle.clone().with_fill_rule(FillRule::EvenOdd),
            flags: PaintFlags {
                blur: 1.5,
                ..green.clone()
            },
        },
        PaintOp::DrawImage {
            image: ImageData::new(GREEN_BLUE),
            src: Some(RectF::new(2.0, 1.0, 10.0, 6.0)),
            dst: RectF::new(50.0, 4.0, 30.0, 20.0),
            filter: FilterQuality::High,
            flags: Some(PaintFlags {
                blend_mode: BlendMode::Multiply,
                ..PaintFlags::fill(Color::rgba(0, 0, 0, 128))
            }),
        },
        PaintOp::DrawRecord {
            record,
            size: gfx::SizeF::new(10.0, 10.0),
            dst: RectF::new(60.0, 60.0, 25.0, 15.0),
        },
        PaintOp::ClearRect {
            rect: RectF::new(70.0, 0.0, 5.0, 5.0),
        },
        PaintOp::WritePixels {
            x: 3,
            y: 2,
            size: Size::new(2, 2),
            pixels: vec![128; 16].into(),
        },
        PaintOp::Save,
        PaintOp::ClipPath {
            path: triangle.clone(),
            anti_alias: true,
        },
        PaintOp::ClipPaths {
            paths: vec![
                triangle.clone(),
                gfx::Path::polygons([vec![p(30.0, 30.0), p(80.0, 30.0), p(80.0, 80.0)]]),
            ],
            anti_alias: false,
        },
        PaintOp::DrawColor {
            color: Color::rgba(255, 128, 0, 128),
            blend_mode: BlendMode::SrcOver,
        },
        PaintOp::Restore,
        PaintOp::SaveLayerAlpha {
            bounds: Some(rect),
            alpha: 0.5,
        },
        PaintOp::SetMatrix {
            transform: Transform([0.8, 0.3, -0.3, 0.8, 20.0, 5.0]),
        },
        PaintOp::DrawRect {
            rect,
            flags: green.clone(),
        },
        PaintOp::Restore,
    ];
    for filter in [blur, shadow(false), shadow(true), matrix, chain, nested] {
        ops.extend(layer(
            filter,
            vec![
                PaintOp::DrawRRect {
                    rrect,
                    flags: green.clone(),
                },
                text_run((LATO, 2), "fig", 14.0, p(40.3, 60.6), 7.3, green.clone()),
            ],
        ));
    }
    for (index, blend_mode) in BLEND_MODES.into_iter().enumerate() {
        let x = 3.0 * index as f32;
        ops.push(filled(
            RectF::new(x, 70.0, 6.0, 8.0),
            Color::rgba(200, 40, 90, 180),
            blend_mode,
        ));
        ops.push(PaintOp::SaveLayer {
            bounds: Some(RectF::new(x, 78.0, 6.0, 8.0)),
            flags: LayerFlags {
                alpha: 0.7,
                blend_mode,
                image_filter: None,
            },
        });
        ops.push(PaintOp::DrawRect {
            rect: RectF::new(x, 78.0, 6.0, 8.0),
            flags: green.clone(),
        });
        ops.push(PaintOp::Restore);
        ops.push(PaintOp::Save);
        ops.push(PaintOp::ClipRect {
            rect: RectF::new(x, 86.0, 6.0, 4.0),
            anti_alias: true,
        });
        ops.push(PaintOp::DrawColor {
            color: Color::rgba(0, 90, 200, 150),
            blend_mode,
        });
        ops.push(PaintOp::Restore);
    }
    for (index, shader) in shaders.into_iter().enumerate() {
        let x = 9.0 * index as f32;
        let flags = PaintFlags {
            color: Color::rgba(0, 0, 0, 200),
            ..PaintFlags::shader(shader)
        };
        ops.push(PaintOp::DrawRect {
            rect: RectF::new(x, 40.0, 8.0, 20.0),
            flags: flags.clone(),
        });
        ops.push(PaintOp::DrawLine {
            p0: p(x, 62.0),
            p1: p(x + 8.0, 66.0),
            flags: PaintFlags {
                style: compositor_paint::PaintStyle::Stroke { width: 3.0 },
                ..flags.clone()
            },
        });
        ops.push(text_run((AHEM, 1), "X", 9.0, p(x, 30.0), 9.0, flags));
    }
    let lato = |flags: PaintFlags, embolden, skew, rotations: Vec<f32>| {
        let PaintOp::DrawGlyphs {
            font,
            size,
            glyphs,
            positions,
            variations,
            bounds,
            ..
        } = text_run((LATO, 2), "Raster", 13.0, p(5.2, 20.7), 7.4, flags.clone())
        else {
            unreachable!()
        };
        PaintOp::DrawGlyphs {
            font,
            size,
            glyphs,
            positions,
            rotations: rotations.into(),
            flags,
            embolden,
            skew,
            variations,
            bounds,
        }
    };
    ops.push(lato(green.clone(), false, 0.0, Vec::new()));
    ops.push(lato(green.clone(), true, 14.0, Vec::new()));
    ops.push(lato(
        PaintFlags::stroke(Color::BLACK, 1.0),
        false,
        0.0,
        vec![10.0, -30.0],
    ));
    ops.push(PaintOp::Restore);
    ops
}

/// Nothing a display list can hold makes playback panic: not on a tile
/// that begins inside the layer, scaled or not, nor on a canvas. (Vello
/// panics on what it doesn't do yet; those are drawn another way.)
#[test]
fn one_of_everything_is_drawn() {
    let list = list_of(one_of_everything());
    for (scale, x, y) in [(1.0, 0.0, 0.0), (1.0, 31.0, 17.0), (1.75, 40.5, 12.0)] {
        let bmp = crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(x, y, 64.0, 64.0),
                contents_scale: scale,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        );
        assert!(bmp.pixels.as_chunks::<4>().0.iter().any(|&p| p != WHITE));
    }
    let size = Size::new(96, 96);
    for opaque in [false, true] {
        let mut pixels = vec![0; 96 * 96 * 4];
        crate::draw_into(&list, &RasterCache::default(), size, opaque, &mut pixels);
        assert!(pixels.as_chunks::<4>().0.iter().any(|p| p[1] != 0));
    }
}

/// The greatest difference in a channel, and where, between `ops` played
/// in one piece `side` on a side and played as tiles `tile` on a side.
fn seam(ops: Vec<PaintOp>, side: u32, tile: u32, scale: f32) -> (u8, (u32, u32)) {
    let list = list_of(ops);
    let cache = RasterCache::default();
    let play = |x: u32, y: u32, size: u32| {
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(x as f32, y as f32, size as f32, size as f32),
                contents_scale: scale,
                clear_color: Color::WHITE,
            },
            &cache,
        )
    };
    let whole = play(0, 0, side);
    let mut worst = (0, (0, 0));
    for ty in (0..side).step_by(tile as usize) {
        for tx in (0..side).step_by(tile as usize) {
            let part = play(tx, ty, tile);
            for y in 0..tile {
                for x in 0..tile {
                    let (a, b) = (part.pixel(x, y), whole.pixel(tx + x, ty + y));
                    let diff = (0..4).map(|i| a[i].abs_diff(b[i])).max().unwrap();
                    if diff > worst.0 {
                        worst = (diff, (tx + x, ty + y));
                    }
                }
            }
        }
    }
    worst
}

/// A tile is the pixels of that part of the layer drawn whole: what
/// reaches in from outside it (a blur, a shadow, a glyph whose origin is
/// in the next tile) is drawn. Exactly the same where shapes and text are
/// at whole pixels' scale; within a level where a blur is drawn from a
/// context of another size, or where an edge that isn't on a pixel is
/// covered from another origin (the coverage is computed in floats, which
/// round otherwise 32 pixels along).
#[test]
fn tiles_have_the_pixels_of_the_whole() {
    let p = PointF::new;
    let rrect = gfx::RRectF::uniform(RectF::new(20.0, 20.0, 50.0, 40.0), 8.0);
    let green = PaintFlags::fill(Color::rgb(0, 128, 0));
    let shapes = vec![
        PaintOp::DrawRRect {
            rrect,
            flags: green.clone(),
        },
        PaintOp::DrawLine {
            p0: p(3.0, 90.0),
            p1: p(93.0, 61.0),
            flags: PaintFlags::stroke(Color::rgb(0, 0, 255), 5.0),
        },
        filled(
            RectF::new(30.5, 10.25, 40.0, 70.0),
            Color::rgba(255, 0, 0, 100),
            BlendMode::Multiply,
        ),
    ];
    let text = vec![
        text_run(
            (LATO, 2),
            "Seamless tiling",
            15.0,
            p(1.3, 34.6),
            6.7,
            green.clone(),
        ),
        text_run((AHEM, 1), "XXXXX", 21.0, p(-9.5, 70.2), 21.3, green.clone()),
        text_run(
            (LATO, 2),
            "Wide",
            40.0,
            p(2.0, 66.0),
            24.0,
            PaintFlags::stroke(Color::BLACK, 3.0),
        ),
    ];
    let shadows = vec![
        PaintOp::DrawShadow {
            shape: rrect.offset(5.0, 7.0),
            hole: None,
            clip: rrect,
            inside: false,
            flags: PaintFlags {
                blur: 6.0,
                ..PaintFlags::fill(Color::BLACK)
            },
        },
        PaintOp::DrawShadow {
            shape: rrect,
            hole: Some(rrect.inset(4.0, 4.0, 4.0, 4.0).offset(3.0, 3.0)),
            clip: rrect,
            inside: true,
            flags: PaintFlags {
                blur: 4.0,
                ..PaintFlags::fill(Color::rgb(0, 0, 255))
            },
        },
    ];
    let filters = [
        ImageFilter::Blur { sigma: 4.0 },
        ImageFilter::DropShadow {
            offset: (6.0, 9.0),
            sigma: 3.0,
            color: Color::rgba(0, 0, 0, 160),
            shadow_only: false,
        },
        ImageFilter::Chain(
            [
                ImageFilter::Blur { sigma: 2.0 },
                ImageFilter::ColorMatrix(invert()),
            ]
            .into(),
        ),
    ];
    let mut cases = vec![
        ("shapes", shapes.clone(), true),
        ("text", text, true),
        ("shadows", shadows, true),
    ];
    for (name, filter) in ["blur", "drop shadow", "chain"].into_iter().zip(filters) {
        cases.push((name, layer(filter, shapes.clone()), false));
    }
    let mut seams = Vec::new();
    for (name, ops, exact) in cases {
        for scale in [1.0, 1.5] {
            let allowed = if exact && scale == 1.0 { 0 } else { 1 };
            let (diff, at) = seam(ops.clone(), 96, 32, scale);
            if diff > allowed {
                seams.push(format!("{name} at {scale}: {diff} at {at:?}"));
            }
        }
    }
    assert!(seams.is_empty(), "{seams:#?}");
}

/// A glyph is drawn at the nearest quarter of a device pixel along the
/// line and the nearest whole one across it: the same text a hair apart
/// is the same pixels.
#[test]
fn glyphs_are_drawn_at_quarters_of_a_pixel() {
    let at = |x: f32, y: f32| ahem_square(PointF::new(x, y), None, PaintFlags::fill(Color::BLACK));
    let exact = at(2.0, 18.0);
    assert_eq!(at(2.1, 18.0).pixels, exact.pixels);
    assert_eq!(at(1.9, 18.4).pixels, exact.pixels);
    assert_eq!(at(2.0, 17.6).pixels, exact.pixels);
    // A quarter on: three quarters of the first column, a quarter of the
    // one past the last.
    let quarter = at(2.2, 18.0);
    assert_eq!(quarter.pixels, at(2.25, 18.0).pixels);
    assert_eq!(quarter.pixel(2, 10), [64, 64, 64, 255]);
    assert_eq!(quarter.pixel(22, 10), [191, 191, 191, 255]);
    // Across the line, a whole pixel or none.
    let down = at(2.0, 18.6);
    assert_eq!(down.pixel(10, 2), WHITE);
    assert_eq!(down.pixel(10, 3), [0, 0, 0, 255]);
    assert_eq!(down.pixel(10, 22), [0, 0, 0, 255]);
}

/// A glyph whose origin is outside what is drawn, less than twice its
/// size away, is drawn (it may reach in), with the others of its run,
/// far away or not.
#[test]
fn a_glyph_that_begins_outside_is_drawn() {
    let black = PaintFlags::fill(Color::BLACK);
    // The square goes right and up from its origin: from x = -15 to 5.
    let bmp = ahem_square(PointF::new(-15.0, 30.0), None, black.clone());
    assert_eq!(bmp.pixel(2, 20), [0, 0, 0, 255]);
    assert_eq!(bmp.pixel(6, 20), WHITE);
    // And down to 4 below it.
    let bmp = ahem_square(PointF::new(10.0, -3.0), None, black.clone());
    assert_eq!(bmp.pixel(20, 0), [0, 0, 0, 255]);
    assert_eq!(bmp.pixel(20, 2), WHITE);
    // A run from far left of the tile to far right of it.
    let run = text_run(
        (AHEM, 1),
        "XXXXXXXXXXXXXXXXXXXXX",
        20.0,
        PointF::new(-190.0, 18.0),
        20.0,
        black,
    );
    let bmp = play(vec![run], 40.0, 40.0);
    assert_eq!(bmp.pixel(0, 10), [0, 0, 0, 255]);
    assert_eq!(bmp.pixel(39, 10), [0, 0, 0, 255]);
    assert_eq!(bmp.pixel(20, 30), WHITE);
}

/// Inside a clip that is wholly outside what is drawn, nothing is drawn,
/// whatever else is clipped or saved inside it; after its restore,
/// everything is again.
#[test]
fn a_clip_outside_hides_what_is_in_it() {
    let p = PointF::new;
    let everywhere = RectF::new(-100.0, -100.0, 300.0, 300.0);
    let red = PaintFlags::fill(Color::rgb(255, 0, 0));
    let outside = [
        PaintOp::ClipRect {
            rect: RectF::new(20.0, 0.0, 10.0, 10.0),
            anti_alias: true,
        },
        PaintOp::ClipRRect {
            rrect: gfx::RRectF::uniform(RectF::new(0.0, 20.0, 10.0, 10.0), 2.0),
            anti_alias: true,
        },
        PaintOp::ClipPath {
            path: gfx::Path::polygons([vec![
                p(23.0, -3.0),
                p(23.0, 11.0),
                p(20.0, 8.0),
                p(20.0, 0.0),
            ]]),
            anti_alias: true,
        },
        PaintOp::ClipPaths {
            paths: vec![
                gfx::Path::polygons([vec![p(-9.0, 0.0), p(0.0, 0.0), p(0.0, 9.0)]]),
                gfx::Path::polygons([vec![p(0.0, -9.0), p(9.0, -9.0), p(9.0, 0.0)]]),
            ],
            anti_alias: true,
        },
    ];
    for clip in outside {
        let name = format!("{clip:?}");
        let bmp = play(
            vec![
                PaintOp::Save,
                clip,
                PaintOp::DrawRect {
                    rect: everywhere,
                    flags: red.clone(),
                },
                PaintOp::Save,
                PaintOp::ClipRect {
                    rect: RectF::new(0.0, 0.0, 10.0, 10.0),
                    anti_alias: true,
                },
                PaintOp::DrawLine {
                    p0: p(21.5, -3.0),
                    p1: p(21.5, 11.0),
                    flags: PaintFlags::stroke(Color::BLACK, 3.0),
                },
                PaintOp::SaveLayerAlpha {
                    bounds: None,
                    alpha: 0.5,
                },
                PaintOp::DrawColor {
                    color: Color::rgb(255, 0, 0),
                    blend_mode: BlendMode::Src,
                },
                PaintOp::Restore,
                PaintOp::Restore,
                PaintOp::DrawRect {
                    rect: everywhere,
                    flags: red.clone(),
                },
                PaintOp::Restore,
                PaintOp::DrawRect {
                    rect: RectF::new(0.0, 0.0, 10.0, 10.0),
                    flags: PaintFlags::fill(Color::rgb(0, 0, 255)),
                },
            ],
            20.0,
            20.0,
        );
        for (x, y) in [(5, 5), (15, 5), (5, 15), (15, 15), (19, 0), (0, 19)] {
            let expected = if x < 10 && y < 10 { BLUE } else { WHITE };
            assert_eq!(bmp.pixel(x, y), expected, "{name} at {x}, {y}");
        }
    }
}

/// A gradient that ends in a hard stop (two colors at offset 1) has the
/// second past its end, not the first.
#[test]
fn past_a_hard_stop_at_its_end_a_gradient_is_the_last_color() {
    let stops = |stops: &[(f32, Color)]| {
        fill_with(
            GradientShader {
                kind: GradientShaderKind::Linear {
                    start: PointF::new(0.0, 0.0),
                    end: PointF::new(10.0, 0.0),
                },
                stops: stops
                    .iter()
                    .map(|&(offset, color)| compositor_paint::GradientStop { offset, color })
                    .collect(),
                tile_mode: TileMode::Clamp,
                transform: Transform::IDENTITY,
                interpolation: ColorInterpolation::Premultiplied,
            },
            RectF::new(0.0, 0.0, 30.0, 4.0),
            Size::new(30, 4),
        )
    };
    let red = Color::rgb(255, 0, 0);
    let blue = Color::rgb(0, 0, 255);
    let bmp = stops(&[(0.0, red), (1.0, red), (1.0, blue)]);
    assert_eq!(bmp.pixel(5, 2), [255, 0, 0, 255]);
    assert_eq!(bmp.pixel(8, 2), [255, 0, 0, 255]);
    assert_eq!(bmp.pixel(11, 2), BLUE);
    assert_eq!(bmp.pixel(25, 2), BLUE);
    // One that fades to its hard stop keeps its line's length.
    let bmp = stops(&[(0.0, red), (1.0, Color::WHITE), (1.0, blue)]);
    assert!(
        close(bmp.pixel(5, 2), [255, 140, 140, 255]),
        "{:?}",
        bmp.pixel(5, 2)
    );
    assert_eq!(bmp.pixel(15, 2), BLUE);
    // Without one, the last color goes on.
    let bmp = stops(&[(0.0, red), (0.5, blue), (1.0, red)]);
    assert_eq!(bmp.pixel(25, 2), [255, 0, 0, 255]);
}

/// On a canvas, a draw whose blend mode takes away takes from what the
/// bitmap has, and only where it draws.
#[test]
fn on_a_canvas_a_blend_mode_takes_from_what_the_bitmap_has() {
    let size = Size::new(4, 1);
    let cache = RasterCache::default();
    let drawn = |blend_mode, color| {
        let mut pixels = [[200u8, 0, 0, 200]; 4].concat();
        let list = list_of(vec![filled(
            RectF::new(1.0, 0.0, 2.0, 1.0),
            color,
            blend_mode,
        )]);
        crate::draw_into(&list, &cache, size, false, &mut pixels);
        pixels
    };
    let kept = [200, 0, 0, 200];
    let pixels = drawn(BlendMode::DstOut, Color::BLACK);
    assert_eq!(pixels, [kept, [0; 4], [0; 4], kept].concat());
    let pixels = drawn(BlendMode::Clear, Color::BLACK);
    assert_eq!(pixels, [kept, [0; 4], [0; 4], kept].concat());
    let pixels = drawn(BlendMode::Src, Color::rgba(0, 0, 255, 0));
    assert_eq!(pixels, [kept, [0; 4], [0; 4], kept].concat());
    // Under what is there: blue shows through the 55 of 255 left.
    let pixels = drawn(BlendMode::DstOver, Color::rgb(0, 0, 255));
    assert_eq!(
        pixels,
        [kept, [200, 0, 55, 255], [200, 0, 55, 255], kept].concat()
    );
}

/// Pixels with a side Vello's context can't have (over 65535) are left as
/// they are: a canvas that large keeps what it had.
#[test]
fn pixels_too_wide_to_draw_on_are_left() {
    let size = Size::new(65536, 1);
    let mut pixels = vec![7; 65536 * 4];
    let list = list_of(vec![PaintOp::DrawColor {
        color: Color::rgb(255, 0, 0),
        blend_mode: BlendMode::Src,
    }]);
    crate::draw_into(&list, &RasterCache::default(), size, false, &mut pixels);
    assert!(pixels.iter().all(|&byte| byte == 7));
}

/// Packs single fonts into a TrueType collection: each font is copied
/// whole, with its table offsets moved to where it lands in the file.
pub(crate) fn ttc(fonts: &[&[u8]]) -> Vec<u8> {
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
        let records = u16::from_be_bytes(font[4..6].try_into().unwrap()) as usize;
        for r in 0..records {
            let at = base + 12 + 16 * r + 8;
            let offset = u32::from_be_bytes(out[at..at + 4].try_into().unwrap());
            out[at..at + 4].copy_from_slice(&(offset + base as u32).to_be_bytes());
        }
        out.resize(out.len().next_multiple_of(4), 0);
    }
    out
}

/// A face other than the first of a collection is drawn: Ahem's square,
/// the second face after Lato.
#[test]
fn draws_a_face_after_the_first_of_a_collection() {
    let font = FontData {
        bytes: Arc::new(ttc(&[LATO, AHEM])),
        index: 1,
        id: 2,
    };
    let list = list_of(vec![PaintOp::DrawGlyphs {
        font,
        size: 20.0,
        glyphs: glyph_ids(AHEM, "X").into(),
        positions: vec![PointF::new(2.0, 18.0)].into(),
        rotations: Vec::new().into(),
        flags: PaintFlags::fill(Color::rgb(0, 0, 0)),
        embolden: false,
        skew: 0.0,
        variations: Arc::new([]),
        bounds: RectF::new(2.0, 2.0, 20.0, 20.0),
    }]);
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

/// Where `run`, drawn on white 200 px a side with its origin at
/// (60, 120), leaves pixels that aren't white: left, top, right, bottom,
/// about that origin.
fn drawn_bounds(run: &gfx::GlyphRun) -> [f32; 4] {
    let at = PointF::new(60.0, 120.0);
    let list = list_of(vec![PaintOp::DrawGlyphs {
        font: run.font.clone(),
        size: run.size,
        glyphs: run.glyphs.clone().into(),
        positions: run
            .positions
            .iter()
            .map(|p| PointF::new(p.x + at.x, p.y + at.y))
            .collect(),
        rotations: Vec::new().into(),
        flags: PaintFlags::fill(Color::BLACK),
        embolden: run.embolden,
        skew: run.skew,
        variations: run.variations.clone(),
        bounds: RectF::new(0.0, 0.0, 200.0, 200.0),
    }]);
    let bmp = crate::playback(
        &list,
        &PlaybackParams {
            content_rect: RectF::new(0.0, 0.0, 200.0, 200.0),
            contents_scale: 1.0,
            clear_color: Color::WHITE,
        },
        &RasterCache::default(),
    );
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for y in 0..200 {
        for x in 0..200 {
            if bmp.pixel(x, y) != WHITE {
                (x0, y0) = (x0.min(x), y0.min(y));
                (x1, y1) = (x1.max(x + 1), y1.max(y + 1));
            }
        }
    }
    assert!(x0 < x1, "something was drawn");
    assert!(x0 > 0 && y0 > 0 && x1 < 200 && y1 < 200, "all of it");
    [
        x0 as f32 - at.x,
        y0 as f32 - at.y,
        x1 as f32 - at.x,
        y1 as f32 - at.y,
    ]
}

/// A bitmap glyph's ink is the rect its bitmap is drawn in: every pixel
/// drawn is in it, and the pixels reach its sides but for the bitmap's
/// own transparent margin.
#[test]
fn a_bitmap_glyphs_ink_is_where_it_is_drawn() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../third_party/wpt/fonts/noto/color-emoji/NotoColorEmoji-subset.ttf"
    );
    let bytes = Arc::new(std::fs::read(path).unwrap());
    for (size, skew) in [(40.0, 0.0), (64.0, 0.0), (23.5, 0.0), (40.0, 20.0)] {
        let run = gfx::GlyphRun {
            font: FontData {
                bytes: bytes.clone(),
                index: 0,
                id: 9,
            },
            size,
            glyphs: vec![2],
            positions: vec![PointF::new(0.0, 0.0)],
            embolden: false,
            skew,
            variations: Arc::new([]),
        };
        let ink = crate::ink_bounds(std::slice::from_ref(&run));
        let drawn = drawn_bounds(&run);
        let ink = [ink.x, ink.y, ink.right(), ink.bottom()];
        let name = format!("{size} {skew}: {ink:?} {drawn:?}");
        assert!(
            ink[0] <= drawn[0] + 1.0 && ink[1] <= drawn[1] + 1.0,
            "{name}"
        );
        assert!(
            ink[2] >= drawn[2] - 1.0 && ink[3] >= drawn[3] - 1.0,
            "{name}"
        );
        let margin = size * MARGIN + 1.0;
        // Slanted, the rect's corners are past the round picture's sides
        // by as much as the slant moves its top from its bottom.
        let lean = (ink[3] - ink[1]) * skew.to_radians().tan();
        assert!(
            ink[0] >= drawn[0] - margin - lean && ink[1] >= drawn[1] - margin,
            "{name}"
        );
        assert!(
            ink[2] <= drawn[2] + margin + lean && ink[3] <= drawn[3] + margin,
            "{name}"
        );
    }
}

/// How much of the size the emoji's bitmap is transparent at a side, at
/// most (3 px of it at 40 px).
const MARGIN: f32 = 0.08;

/// The ink bounds of glyphs are where they are drawn: around every pixel
/// drawn, and no more than a pixel and a half past them (the pixels are
/// whole; the bounds are the outline's points', which a curve's control
/// points are just outside of). Made bold and slanted too, by the same
/// amounts.
#[test]
fn ink_bounds_are_where_glyphs_are_drawn() {
    let lato = |text: &str, size: f32, embolden: bool, skew: f32| gfx::GlyphRun {
        font: FontData {
            bytes: Arc::new(LATO.to_vec()),
            index: 0,
            id: 2,
        },
        size,
        glyphs: glyph_ids(LATO, text),
        positions: (0..text.chars().count())
            .map(|i| PointF::new(i as f32 * size * 0.7, 0.0))
            .collect(),
        embolden,
        skew,
        variations: Arc::new([]),
    };
    for text in ["A", "g", "W", "Oj", "fy", "i", "-", "@"] {
        for (size, embolden, skew) in [
            (40.0, false, 0.0),
            (13.0, false, 0.0),
            (40.0, true, 0.0),
            (13.0, true, 0.0),
            (72.0, true, 0.0),
            (40.0, false, 20.0),
            (40.0, true, 14.0),
        ] {
            let run = lato(text, size, embolden, skew);
            let ink = crate::ink_bounds(std::slice::from_ref(&run));
            let drawn = drawn_bounds(&run);
            let ink = [ink.x, ink.y, ink.right(), ink.bottom()];
            let name = format!("{text} {size} {embolden} {skew}: {ink:?} {drawn:?}");
            // Outside the pixels drawn, or inside by less than the
            // coverage that leaves a pixel white.
            assert!(
                ink[0] <= drawn[0] + 1.0 && ink[1] <= drawn[1] + 1.0,
                "{name}"
            );
            assert!(
                ink[2] >= drawn[2] - 1.0 && ink[3] >= drawn[3] - 1.0,
                "{name}"
            );
            assert!(
                ink[0] >= drawn[0] - 1.5 && ink[1] >= drawn[1] - 1.5,
                "{name}"
            );
            assert!(
                ink[2] <= drawn[2] + 1.5 && ink[3] <= drawn[3] + 1.5,
                "{name}"
            );
        }
    }
}

/// A 16 by 16 checkerboard of black and white pixels, with a red 8 by 8
/// in its top left quarter's place if `corner`.
fn checkerboard(corner: bool) -> ImageData {
    let mut pixels = Vec::new();
    for y in 0..16 {
        for x in 0..16 {
            let value = if (x + y) % 2 == 0 { 0 } else { 255 };
            pixels.extend(if corner && x < 8 && y < 8 {
                [255, 0, 0, 255]
            } else {
                [value, value, value, 255]
            });
        }
    }
    ImageData::decoded(Size::new(16, 16), pixels)
}

/// An image drawn smaller than half its size with `Medium` or `High` is
/// drawn from its pixels averaged: a checkerboard of black and white is
/// gray all over, where four pixels of it sampled as they are (`Low`, a
/// canvas's) are whatever they happen to be.
#[test]
fn an_image_made_much_smaller_is_its_pixels_averaged() {
    let drawn = |filter: FilterQuality, scale: f32| {
        let list = list_of(vec![PaintOp::DrawImage {
            image: checkerboard(false),
            src: None,
            dst: RectF::new(0.0, 0.0, 16.0 * scale, 16.0 * scale),
            filter,
            flags: None,
        }]);
        crate::playback(
            &list,
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 4.0, 4.0),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        )
    };
    let gray = |pixel: [u8; 4]| (126..=129).contains(&pixel[0]) && pixel[0] == pixel[1];
    for filter in [FilterQuality::Medium, FilterQuality::High] {
        let bmp = drawn(filter, 0.25);
        for (x, y) in [(0, 0), (1, 2), (3, 3)] {
            assert!(gray(bmp.pixel(x, y)), "{filter:?}: {:?}", bmp.pixel(x, y));
        }
    }
    // Three pixels of the image to one: no four of them average it.
    let low = drawn(FilterQuality::Low, 1.0 / 3.0);
    assert!(!(0..4).all(|x| gray(low.pixel(x, 1))));
    let medium = drawn(FilterQuality::Medium, 1.0 / 3.0);
    assert!((0..4).all(|x| gray(medium.pixel(x, 1))));
    // By the scale it is seen at, not the one it is drawn at alone
    // (a third, where a quarter would sample between four pixels each
    // time, which is their average too).
    let list = list_of(vec![
        PaintOp::Concat {
            transform: Transform([1.0 / 3.0, 0.0, 0.0, 1.0 / 3.0, 0.0, 0.0]),
        },
        PaintOp::DrawImage {
            image: checkerboard(false),
            src: None,
            dst: RectF::new(0.0, 0.0, 16.0, 16.0),
            filter: FilterQuality::Medium,
            flags: None,
        },
    ]);
    let bmp = crate::playback(
        &list,
        &PlaybackParams {
            content_rect: RectF::new(0.0, 0.0, 4.0, 4.0),
            contents_scale: 1.0,
            clear_color: Color::WHITE,
        },
        &RasterCache::default(),
    );
    for x in 0..4 {
        assert!(gray(bmp.pixel(x, 1)), "{:?}", bmp.pixel(x, 1));
    }
}

/// A part of an image drawn much smaller is that part of the smaller
/// level, and a pattern of one is tiles of it: the red quarter of the
/// image stays the top left quarter of what is drawn, the rest gray.
#[test]
fn a_part_and_a_pattern_of_an_image_made_smaller_are_from_its_level() {
    let play = |op: PaintOp| {
        crate::playback(
            &list_of(vec![op]),
            &PlaybackParams {
                content_rect: RectF::new(0.0, 0.0, 8.0, 8.0),
                contents_scale: 1.0,
                clear_color: Color::WHITE,
            },
            &RasterCache::default(),
        )
    };
    let red = |pixel: [u8; 4]| pixel == [255, 0, 0, 255];
    let gray = |pixel: [u8; 4]| (126..=129).contains(&pixel[0]) && pixel[0] == pixel[1];
    // The right half of the image, 8 by 16, into 2 by 4: all gray. Its
    // left half the same way: red above, gray below.
    for (x, top_red) in [(8.0, false), (0.0, true)] {
        let bmp = play(PaintOp::DrawImage {
            image: checkerboard(true),
            src: Some(RectF::new(x, 0.0, 8.0, 16.0)),
            dst: RectF::new(1.0, 1.0, 2.0, 4.0),
            filter: FilterQuality::Medium,
            flags: None,
        });
        for px in 1..3 {
            let top = bmp.pixel(px, 1);
            assert!(if top_red { red(top) } else { gray(top) }, "{x}: {top:?}");
            assert!(gray(bmp.pixel(px, 4)), "{x}: {:?}", bmp.pixel(px, 4));
        }
        assert_eq!(bmp.pixel(0, 2), WHITE);
        assert_eq!(bmp.pixel(3, 2), WHITE);
        assert_eq!(bmp.pixel(1, 5), WHITE);
    }
    // Tiles of 5 by 5, from (0, 0): each red in its top left quarter
    // (5 to the image's 16 samples it between pixels, not where four
    // meet, which would be their average as it is).
    let bmp = play(PaintOp::DrawRect {
        rect: RectF::new(0.0, 0.0, 8.0, 8.0),
        flags: PaintFlags::shader(TiledShader {
            image: ShaderImage::Image(checkerboard(true)),
            tile: RectF::new(0.0, 0.0, 5.0, 5.0),
            spacing: gfx::SizeF::new(0.0, 0.0),
            repeat_x: true,
            repeat_y: true,
            transform: Transform::IDENTITY,
            filter: FilterQuality::Medium,
            decal: false,
        }),
    });
    for (tx, ty) in [(0, 0), (5, 0), (0, 5)] {
        assert!(red(bmp.pixel(tx, ty)), "{:?}", bmp.pixel(tx, ty));
        assert!(
            red(bmp.pixel(tx + 1, ty + 1)),
            "{:?}",
            bmp.pixel(tx + 1, ty + 1)
        );
    }
    for (x, y) in [(3, 0), (4, 1), (3, 3), (0, 3), (1, 4), (4, 4)] {
        assert!(gray(bmp.pixel(x, y)), "{x}, {y}: {:?}", bmp.pixel(x, y));
    }
}

/// A red rect over all of a 4 by 2 tile.
fn red_tile() -> DisplayItemList {
    list_of(vec![PaintOp::DrawRect {
        rect: RectF::new(0.0, 0.0, 4.0, 2.0),
        flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
    }])
}

fn tile_params(clear_color: Color) -> PlaybackParams {
    PlaybackParams {
        content_rect: RectF::new(0.0, 0.0, 4.0, 2.0),
        contents_scale: 1.0,
        clear_color,
    }
}

/// What Vello panics on, were it handed it (`shows` keeps it back): a
/// stroke clipped to a polygon, both beginning at the right edge of
/// pixels 20 wide.
fn vello_panics_on() -> Vec<PaintOp> {
    let p = PointF::new;
    vec![
        PaintOp::Save,
        PaintOp::ClipPath {
            path: gfx::Path::polygons([vec![
                p(23.0, -3.0),
                p(23.0, 11.0),
                p(20.0, 8.0),
                p(20.0, 0.0),
            ]]),
            anti_alias: true,
        },
        PaintOp::DrawLine {
            p0: p(21.5, -3.0),
            p1: p(21.5, 11.0),
            flags: PaintFlags::stroke(Color::BLACK, 3.0),
        },
        PaintOp::Restore,
    ]
}

/// Whether playbacks with `cache` hand Vello what `shows` keeps from it.
fn show_all(cache: &RasterCache, all: bool) {
    cache
        .vello
        .shows_all
        .store(all, std::sync::atomic::Ordering::Relaxed);
}

/// A playback that panics leaves the tile its clear color (not what the
/// buffer had from the tile before), is counted, and costs the thread its
/// context: the next playback draws with a new one.
#[test]
fn a_playback_that_panics_leaves_the_tile_its_clear_color() {
    let cache = RasterCache::default();
    let list = red_tile();
    let ops = [0];
    let mut pixels = Vec::new();
    let white = tile_params(Color::WHITE);
    crate::playback_into(&list, &ops, &white, &cache, &mut pixels);
    assert_eq!(pixels, [255, 0, 0, 255].repeat(8));
    assert!(crate::vello_backend::has_context());
    assert_eq!(cache.take_failure(), None);

    for (clear, left) in [
        (Color::rgb(10, 20, 30), [10, 20, 30, 255]),
        (Color::rgba(200, 100, 0, 128), [100, 50, 0, 128]),
    ] {
        cache.fail_next_for_testing(1);
        let size = crate::playback_into(&list, &ops, &tile_params(clear), &cache, &mut pixels);
        assert_eq!(size, Size::new(4, 2));
        assert_eq!(pixels, left.repeat(8));
        assert!(!crate::vello_backend::has_context());
        // The one after is drawn.
        crate::playback_into(&list, &ops, &white, &cache, &mut pixels);
        assert_eq!(pixels, [255, 0, 0, 255].repeat(8));
        assert!(crate::vello_backend::has_context());
    }
    assert_eq!(
        cache.take_failure(),
        Some(crate::RasterFailure {
            message: "a test asked this playback to fail".into(),
            count: 2,
        })
    );
    assert_eq!(cache.take_failure(), None);
}

/// A failure is one to tell the first time and each time the count
/// doubles; the last such one waits until it is taken.
#[test]
fn failures_are_told_when_their_count_doubles() {
    let cache = RasterCache::default();
    let list = red_tile();
    let params = tile_params(Color::WHITE);
    cache.fail_next_for_testing(9);
    let mut told = Vec::new();
    for _ in 0..9 {
        crate::playback_ops(&list, &[0], &params, &cache);
        told.push(cache.take_failure().map(|failure| failure.count));
    }
    assert_eq!(
        told,
        [
            Some(1),
            Some(2),
            None,
            Some(4),
            None,
            None,
            None,
            Some(8),
            None
        ]
    );
    // Not taken each time: the last one to tell.
    cache.fail_next_for_testing(8);
    for _ in 0..8 {
        crate::playback_ops(&list, &[0], &params, &cache);
    }
    assert_eq!(cache.take_failure().map(|failure| failure.count), Some(16));
    // No more asked for: drawn, and nothing to tell.
    let bmp = crate::playback_ops(&list, &[0], &params, &cache);
    assert_eq!(bmp.pixel(0, 0), [255, 0, 0, 255]);
    assert_eq!(cache.take_failure(), None);
}

/// A panic of Vello's own, in the middle of a playback, is caught as one
/// asked for is. (When this fails with the tile drawn, upstream fixed the
/// panic: see `upstream_panics_on_a_clipped_stroke_at_the_right_edge`.)
#[test]
fn a_panic_of_vellos_is_caught() {
    let cache = RasterCache::default();
    // Catching, as a release build.
    cache.fail_next_for_testing(0);
    let mut ops = vec![PaintOp::DrawRect {
        rect: RectF::new(0.0, 0.0, 20.0, 20.0),
        flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
    }];
    ops.extend(vello_panics_on());
    let list = list_of(ops);
    let all: Vec<usize> = (0..list.ops().len()).collect();
    let params = PlaybackParams {
        content_rect: RectF::new(0.0, 0.0, 20.0, 20.0),
        contents_scale: 1.0,
        clear_color: Color::WHITE,
    };
    let red = [255, 0, 0, 255];
    // As it is drawn: the clip isn't handed to Vello.
    let bmp = crate::playback_ops(&list, &all, &params, &cache);
    assert_eq!(bmp.pixel(10, 10), red);
    assert_eq!(cache.take_failure(), None);
    show_all(&cache, true);
    let bmp = crate::playback_ops(&list, &all, &params, &cache);
    assert_eq!(bmp.pixels, [255; 4].repeat(400));
    assert!(!crate::vello_backend::has_context());
    show_all(&cache, false);
    let failure = cache.take_failure().expect("the panic is told");
    assert_eq!(failure.count, 1);
    assert!(failure.message.contains("overflow"), "{}", failure.message);
    let bmp = crate::playback_ops(&list, &all, &params, &cache);
    assert_eq!(bmp.pixel(10, 10), red);
}

/// On a canvas, a playback that panics leaves the bitmap what it had and
/// what was written to it before the panic; an opaque one stays opaque.
#[test]
fn a_canvas_keeps_what_it_had_before_a_panic() {
    let size = Size::new(20, 20);
    let red = list_of(vec![PaintOp::DrawRect {
        rect: RectF::new(0.0, 0.0, 20.0, 20.0),
        flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
    }]);
    let cache = RasterCache::default();
    let mut pixels = [0, 0, 255, 255].repeat(400);
    // Asked to fail as it begins: nothing of the list.
    cache.fail_next_for_testing(1);
    crate::draw_into(&red, &cache, size, false, &mut pixels);
    assert_eq!(pixels, [0, 0, 255, 255].repeat(400));
    assert_eq!(cache.take_failure().map(|failure| failure.count), Some(1));
    crate::draw_into(&red, &cache, size, false, &mut pixels);
    assert_eq!(pixels, [255, 0, 0, 255].repeat(400));

    // Vello's own, after pixels were written: those are there, the red
    // under them too, and an opaque bitmap's alpha is 255 for all that
    // the pixels written had none.
    let mut ops = vec![PaintOp::WritePixels {
        x: 1,
        y: 0,
        size: Size::new(1, 1),
        pixels: vec![0, 0, 0, 0].into(),
    }];
    ops.extend(vello_panics_on());
    let list = list_of(ops);
    show_all(&cache, true);
    crate::draw_into(&list, &cache, size, true, &mut pixels);
    assert_eq!(cache.take_failure().map(|failure| failure.count), Some(2));
    assert!(!crate::vello_backend::has_context());
    assert_eq!(pixels[..12], [255, 0, 0, 255, 0, 0, 0, 255, 255, 0, 0, 255]);
}

/// Where debug assertions are on, a playback's panic isn't caught: a test
/// fails of it.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "a test asked this playback to fail")]
fn with_debug_assertions_a_playback_that_panics_panics() {
    let cache = RasterCache::default();
    cache
        .failures
        .fail_next
        .store(1, std::sync::atomic::Ordering::Relaxed);
    crate::playback_ops(&red_tile(), &[0], &tile_params(Color::WHITE), &cache);
}

/// A red square in a layer put back by `mode`, in `clip`, on a green
/// canvas of 10 by 10.
fn clearing_layer_in(clip: RectF, mode: BlendMode) -> Vec<u8> {
    let list = list_of(vec![
        PaintOp::Save,
        PaintOp::ClipRect {
            rect: clip,
            anti_alias: false,
        },
        PaintOp::SaveLayer {
            bounds: None,
            flags: LayerFlags {
                blend_mode: mode,
                ..LayerFlags::default()
            },
        },
        PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 3.0, 3.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        },
        PaintOp::Restore,
        PaintOp::Restore,
    ]);
    let mut pixels = [0, 255, 0, 255].repeat(100);
    let size = Size::new(10, 10);
    crate::draw_into(&list, &RasterCache::default(), size, false, &mut pixels);
    pixels
}

const CLEARING: [BlendMode; 4] = [
    BlendMode::SrcIn,
    BlendMode::SrcOut,
    BlendMode::DstIn,
    BlendMode::DstATop,
];

/// A layer put back with a blend mode that clears what it doesn't cover
/// changes nothing when the clip it is in is wholly outside the pixels:
/// the layer is hidden whole, not put back unclipped.
#[test]
fn a_clearing_layer_in_a_clip_outside_changes_nothing() {
    for mode in CLEARING {
        let pixels = clearing_layer_in(RectF::new(-20.0, -20.0, 10.0, 10.0), mode);
        assert_eq!(pixels, [0, 255, 0, 255].repeat(100), "{mode:?}");
    }
}

/// What upstream does, at the commit pinned: a layer's blend mode isn't
/// kept to the clip the layer is in, so one that clears what the layer
/// doesn't cover clears outside the clip too (a canvas's `source-in` after
/// `clip()`). When this fails, Vello keeps it to the clip.
#[test]
fn upstream_doesnt_keep_a_clearing_layer_to_its_clip() {
    for mode in CLEARING {
        let pixels = clearing_layer_in(RectF::new(2.0, 2.0, 4.0, 4.0), mode);
        // (8, 8) is outside the clip, and should be green.
        assert_eq!(pixels[(8 * 10 + 8) * 4..][..4], [0, 0, 0, 0], "{mode:?}");
    }
}
