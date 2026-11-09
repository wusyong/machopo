//! Drawing images (`drawImage`) and patterns (`createPattern`): what a
//! source image is once the DOM found it (`SourceImage`), the rects
//! `drawImage` draws from and to, a pattern's tiling and its shader, and
//! the image smoothing's names. Both DOMs find the source (an `<img>`'s
//! image, a canvas's snapshot); these steps are the same for both.

use base::numerics::f32_from_f64;
use compositor_paint::{FilterQuality, PaintFlags, PaintOp, PaintShader, ShaderImage, TiledShader};
use gfx::{Color, RectF, SizeF};

use crate::context::{IDENTITY, clamp_f32, to_gfx, with_global_alpha};
use crate::{CanvasData, CanvasDrawingState, CanvasPatternData};

/// What a canvas draws an image source as: the image, its size in the
/// space it is drawn in (an `<img>`'s density-corrected natural size, a
/// canvas's bitmap size), how many of its pixels make one unit of that,
/// and whether a canvas that draws it stays origin-clean.
pub struct SourceImage {
    /// `None` for a canvas whose bitmap can't have pixels (Blink's
    /// `kInvalidSourceImageStatus`), or that there is no memory for: it
    /// draws nothing, and a pattern of it paints nothing.
    pub data: Option<gfx::ImageData>,
    pub size: (f64, f64),
    pub density: f64,
    pub origin_clean: bool,
}

/// The message of the `InvalidStateError` for a canvas source of no width
/// (`width`) or height, Blink's.
pub fn empty_canvas_message(width: bool) -> &'static str {
    if width {
        "The image argument is a canvas element with a width of 0."
    } else {
        "The image argument is a canvas element with a height of 0."
    }
}

/// The message of the `InvalidStateError` for a broken `<img>`, Blink's.
pub const BROKEN_IMAGE_MESSAGE: &str = "The HTMLImageElement provided is in the 'broken' state.";

/// A canvas's pixels as a source image (`size` valid, its recording drawn
/// first): its snapshot (`CanvasData::snapshot`, which a bitmap another
/// thread has fills in later: drawing it records it, and waits for
/// nothing, as Blink's `AcceleratedStaticBitmapImage`), and whether it is
/// origin-clean. One with nothing drawn, or no context (`data` `None`), is
/// transparent, as Blink's `CreateTransparentImage`: made for a valid size
/// up to a GB, so asked for rather than taken. `None` without the memory
/// for it (an opaque canvas's snapshot is its black: without one there was
/// none).
pub fn canvas_source(
    data: Option<&mut CanvasData>,
    background: [u8; 4],
    size: (u32, u32),
) -> SourceImage {
    let (snapshot, origin_clean) = match data {
        Some(data) => (data.snapshot(background), data.origin_clean),
        None => (None, true),
    };
    let data = snapshot.or_else(|| {
        if background[3] != 0 {
            return None;
        }
        let size = gfx::Size::new(size.0, size.1);
        let len = size.checked_byte_size()?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).ok()?;
        pixels.resize(len, 0);
        Some(gfx::ImageData::decoded(size, pixels))
    });
    SourceImage {
        data,
        size: (f64::from(size.0), f64::from(size.1)),
        density: 1.0,
        origin_clean,
    }
}

/// A canvas source of a size that can't have pixels: it draws nothing.
pub fn invalid_canvas_source(size: (u32, u32)) -> SourceImage {
    SourceImage {
        data: None,
        size: (f64::from(size.0), f64::from(size.1)),
        density: 1.0,
        origin_clean: true,
    }
}

/// `rect` with a negative width or height made positive, going back
/// from its corner (Blink's `AdjustRectForCanvas`).
fn normalized([x, y, w, h]: [f64; 4]) -> [f64; 4] {
    let (x, w) = if w < 0.0 { (x + w, -w) } else { (x, w) };
    let (y, h) = if h < 0.0 { (y + h, -h) } else { (y, h) };
    [x, y, w, h]
}

/// `src` cut to the image's rect, and `dst` cut as much in proportion
/// (Blink's `ClipRectsToImageRect`).
fn clip_to_image(image: [f64; 2], src: [f64; 4], dst: [f64; 4]) -> ([f64; 4], [f64; 4]) {
    let [sx, sy, sw, sh] = src;
    let [dx, dy, dw, dh] = dst;
    if sx >= 0.0 && sy >= 0.0 && sx + sw <= image[0] && sy + sh <= image[1] {
        return (src, dst);
    }
    let (scale_x, scale_y) = (dw / sw, dh / sh);
    let (offset_x, offset_y) = (dx - sx * scale_x, dy - sy * scale_y);
    let (x0, y0) = (sx.max(0.0), sy.max(0.0));
    let (x1, y1) = ((sx + sw).min(image[0]), (sy + sh).min(image[1]));
    let src = [x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0)];
    let dst = [
        src[0] * scale_x + offset_x,
        src[1] * scale_y + offset_y,
        src[2] * scale_x,
        src[3] * scale_y,
    ];
    (src, dst)
}

/// Whether `drawImage`'s numbers are all finite: it draws nothing
/// otherwise, before it looks at the image.
pub fn draw_image_args_finite(src: Option<[f64; 4]>, dst: ([f64; 2], Option<[f64; 2]>)) -> bool {
    src.into_iter()
        .flatten()
        .chain(dst.0)
        .chain(dst.1.into_iter().flatten())
        .all(f64::is_finite)
}

/// `drawImage`'s op (Blink's `drawImage`): `src` of the image (its whole
/// for `None`) into `dst` (its size for `None`), sampled as the image
/// smoothing says, with `globalAlpha`. `None` for an image with nothing to
/// draw, or a rect of no size. A canvas that draws an image another
/// origin's isn't origin-clean after: the caller's.
pub fn draw_image_op(
    state: &CanvasDrawingState,
    image: &SourceImage,
    src: Option<[f64; 4]>,
    dst: ([f64; 2], Option<[f64; 2]>),
) -> Option<PaintOp> {
    let data = image.data.clone()?;
    let (width, height) = image.size;
    let src = normalized(src.unwrap_or([0.0, 0.0, width, height]));
    let [dx, dy] = dst.0;
    let [dw, dh] = dst.1.unwrap_or([src[2], src[3]]);
    let dst = normalized([dx, dy, dw, dh]);
    if src[2] == 0.0 || src[3] == 0.0 || dst[2] == 0.0 || dst[3] == 0.0 {
        return None;
    }
    let (src, dst) = clip_to_image([width, height], src, dst);
    if src[2] <= 0.0 || src[3] <= 0.0 {
        return None;
    }
    let rect = |[x, y, w, h]: [f64; 4], scale: f64| {
        RectF::new(
            clamp_f32(x * scale),
            clamp_f32(y * scale),
            clamp_f32(w * scale),
            clamp_f32(h * scale),
        )
    };
    Some(PaintOp::DrawImage {
        image: data,
        src: Some(rect(src, image.density)),
        dst: rect(dst, 1.0),
        filter: state.image_filter(),
        flags: Some(PaintFlags::fill(with_global_alpha(
            Color::BLACK,
            state.global_alpha,
        ))),
    })
}

/// `createPattern`'s `repetition`, which axes it repeats on: `repeat` (the
/// empty string's too), `repeat-x`, `repeat-y`, `no-repeat`; the
/// `SyntaxError`'s message for anything else (checked first, as Blink
/// does).
pub fn parse_repetition(repetition: &str) -> Result<(bool, bool), String> {
    Ok(match repetition {
        "repeat" | "" => (true, true),
        "repeat-x" => (true, false),
        "repeat-y" => (false, true),
        "no-repeat" => (false, false),
        other => {
            return Err(format!(
                "The provided type ('{other}') is not one of 'repeat', 'no-repeat', 'repeat-x', or 'repeat-y'."
            ));
        }
    })
}

/// A pattern of `source` as it is now, tiled as `repeat` says, with no
/// transform. A canvas of no pixels paints nothing: as a transparent pixel
/// would.
pub fn pattern_data(source: SourceImage, (repeat_x, repeat_y): (bool, bool)) -> CanvasPatternData {
    let (image, size) = match source.data {
        Some(image) => (
            image,
            (f32_from_f64(source.size.0), f32_from_f64(source.size.1)),
        ),
        None => (
            gfx::ImageData::decoded(gfx::Size::new(1, 1), vec![0; 4]),
            (1.0, 1.0),
        ),
    };
    CanvasPatternData {
        image,
        size,
        repeat_x,
        repeat_y,
        transform: IDENTITY,
        origin_clean: source.origin_clean,
    }
}

/// A pattern as a shader: its image in a tile of its size at the origin,
/// repeated on the axes it repeats on and nothing past it on the others
/// (Skia's decal), through its transform, sampled as the image smoothing
/// says.
pub fn pattern_shader(pattern: &CanvasPatternData, filter: FilterQuality) -> PaintShader {
    PaintShader::Tiled(TiledShader {
        image: ShaderImage::Image(pattern.image.clone()),
        tile: RectF::new(0.0, 0.0, pattern.size.0, pattern.size.1),
        spacing: SizeF::new(0.0, 0.0),
        repeat_x: pattern.repeat_x,
        repeat_y: pattern.repeat_y,
        transform: to_gfx(pattern.transform),
        filter,
        decal: true,
    })
}

/// `imageSmoothingQuality`'s values and the filters they sample with.
pub const SMOOTHING_QUALITIES: [(&str, FilterQuality); 3] = [
    ("low", FilterQuality::Low),
    ("medium", FilterQuality::Medium),
    ("high", FilterQuality::High),
];

/// `imageSmoothingQuality` as it reads back.
pub fn smoothing_quality_name(quality: FilterQuality) -> &'static str {
    match quality {
        FilterQuality::Medium => "medium",
        FilterQuality::High => "high",
        FilterQuality::Low | FilterQuality::None => "low",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_source_rect_past_the_image_cuts_the_destination_as_much() {
        // Half of the source is past the image's right edge: half the
        // destination goes with it.
        let (src, dst) =
            clip_to_image([10.0, 10.0], [5.0, 0.0, 10.0, 10.0], [0.0, 0.0, 20.0, 20.0]);
        assert_eq!(src, [5.0, 0.0, 5.0, 10.0]);
        assert_eq!(dst, [0.0, 0.0, 10.0, 20.0]);
        assert_eq!(normalized([10.0, 10.0, -4.0, -6.0]), [6.0, 4.0, 4.0, 6.0]);
    }

    /// The op draws with `globalAlpha`; a source with no pixels draws
    /// nothing.
    #[test]
    fn an_image_is_drawn_with_the_global_alpha() {
        let state = CanvasDrawingState {
            global_alpha: 0.5,
            ..CanvasDrawingState::default()
        };
        let mut image = invalid_canvas_source((2, 2));
        assert!(draw_image_op(&state, &image, None, ([0.0, 0.0], None)).is_none());
        image.data = Some(gfx::ImageData::decoded(gfx::Size::new(2, 2), vec![0; 16]));
        let Some(PaintOp::DrawImage {
            flags: Some(flags), ..
        }) = draw_image_op(&state, &image, None, ([0.0, 0.0], None))
        else {
            panic!("an image op with flags");
        };
        assert_eq!(flags.color.a, 128);
    }

    #[test]
    fn repetitions() {
        assert_eq!(parse_repetition(""), Ok((true, true)));
        assert_eq!(parse_repetition("repeat-y"), Ok((false, true)));
        assert!(parse_repetition("Repeat").is_err());
    }
}
