//! A 2D context's pixels as script reads and writes them (HTML's
//! `getImageData` and `putImageData`, Blink's `GetImageDataInternal` and
//! `PutByteArray`): the steps on the data, apart from the DOM that finds
//! it, has its recording drawn first and makes the `ImageData`.

use base::numerics::{clamp_round, saturated_cast};
use compositor_paint::PaintOp;

use crate::context::begin_recording;
use crate::{CanvasData, CanvasSettings};

/// What a `RangeError` for too many pixels says (Blink's).
pub const OUT_OF_MEMORY: &str = "Out of memory at ImageData creation";

/// `width` × `height` pixels' bytes, if there can be that many (an
/// `ImageData` whose array a `u32` can index).
pub fn pixel_bytes(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .filter(|&n| n <= isize::MAX as usize && u32::try_from(n).is_ok())
}

/// Zeroed bytes for `width` × `height` pixels (transparent black); none
/// where there can't be that many, or there isn't the memory.
pub fn zeroed_pixels(width: u32, height: u32) -> Option<Vec<u8>> {
    let len = pixel_bytes(width, height)?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    Some(bytes)
}

/// A pixel not premultiplied, premultiplied as Skia rounds it
/// (`SkMulDiv255Round`).
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "c, a <= 255: c * a + 128 <= 65153, plus its high byte <= 65407, \
              which shifted down is a byte"
)]
pub fn premultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    let times = |c: u8| {
        let product = u32::from(c) * u32::from(a) + 128;
        ((product + (product >> 8)) >> 8) as u8
    };
    [times(r), times(g), times(b), a]
}

/// A premultiplied pixel's color as it is, the channels rounded to the
/// nearest byte (Skia's conversion to unpremultiplied).
pub fn unpremultiply([r, g, b, a]: [u8; 4]) -> [u8; 4] {
    if a == 0 {
        return [0, 0, 0, 0];
    }
    let channel = |c: u8| clamp_round::<u8>(f32::from(c) * 255.0 / f32::from(a));
    [channel(r), channel(g), channel(b), a]
}

/// `getImageData`'s rect of `(sx, sy, sw, sh)`: its left and top, width
/// and height, a negative size going left or up; the size a `u32`'s.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "sums and negations of i32s, in i64"
)]
pub fn source_rect(sx: i32, sy: i32, sw: i32, sh: i32) -> (i64, i64, u32, u32) {
    let side = |start: i32, size: i32| {
        if size < 0 {
            (i64::from(start) + i64::from(size), -i64::from(size))
        } else {
            (i64::from(start), i64::from(size))
        }
    };
    let (x, w) = side(sx, sw);
    let (y, h) = side(sy, sh);
    // An i32's magnitude: a u32 holds it.
    (x, y, saturated_cast(w), saturated_cast(h))
}

/// Writes the pixels of the bitmap's rect at `(x, y)` of `out`'s size
/// (`width` wide) into `out`, not premultiplied; transparent black outside
/// the bitmap. `pixels` are the bitmap's as script reads them (none: all
/// `background`, as before anything is drawn).
pub fn read_rect(
    data: &CanvasData,
    pixels: Option<&[u8]>,
    background: [u8; 4],
    (x, y): (i64, i64),
    width: u32,
    out: &mut [u8],
) {
    if !data.has_valid_size() || width == 0 {
        return;
    }
    let (bw, bh) = (i64::from(data.size.0), i64::from(data.size.1));
    let w = i64::from(width);
    for (row, line) in (0_i64..).zip(out.chunks_exact_mut(width as usize * 4)) {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "an i32 plus less than a u32, in i64"
        )]
        let by = y + row;
        if !(0..bh).contains(&by) {
            continue;
        }
        for (column, pixel) in (0..w).zip(line.as_chunks_mut::<4>().0) {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "an i32 plus less than a u32, in i64"
            )]
            let bx = x + column;
            if !(0..bw).contains(&bx) {
                continue;
            }
            let premultiplied = match pixels {
                Some(pixels) => {
                    #[expect(
                        clippy::arithmetic_side_effects,
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "0 <= bx < bw and 0 <= by < bh: a pixel of the bitmap"
                    )]
                    let i = ((by * bw + bx) * 4) as usize;
                    let source = &pixels[i..][..4];
                    [source[0], source[1], source[2], source[3]]
                }
                None => background,
            };
            pixel.copy_from_slice(&unpremultiply(premultiplied));
        }
    }
}

/// How many `getImageData`s take a bitmap back from the thread that has
/// it, to be drawn on the main thread from then on (Blink's
/// `kFallbackToCPUAfterReadbacks`): each one waits for that thread.
pub const READBACKS_TO_DRAW_HERE: u8 = 2;

/// Counts a `getImageData` of a bitmap another thread has: the snapshot to
/// take it back with (`take_back`, once it is ready) when read that often.
pub fn count_readback(data: &mut CanvasData) -> Option<gfx::ImageData> {
    data.resource.as_ref()?;
    data.readbacks = data.readbacks.saturating_add(1);
    (data.readbacks >= READBACKS_TO_DRAW_HERE)
        .then(|| data.snapshot.clone())
        .flatten()
}

/// The bitmap taken back from the thread that has it: its pixels the
/// ready `snapshot`'s, the thread letting go of its own at the next frame.
/// Kept there if there isn't the memory for a copy.
pub fn take_back(data: &mut CanvasData, snapshot: &gfx::ImageData) {
    let bytes = snapshot.as_bytes();
    let mut pixels = Vec::new();
    if pixels.try_reserve_exact(bytes.len()).is_err() {
        return;
    }
    pixels.extend_from_slice(bytes);
    data.pixels = Some(pixels);
    data.resource = None;
    data.drawn_here = true;
}

/// Where `putImageData` writes, in the bitmap: from `(x0, y0)` to `(x1,
/// y1)`, and whether that is all of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutRegion {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
    pub whole: bool,
}

/// The region `putImageData` of an image of `image` size writes at `(dx,
/// dy)` from its dirty rect (a negative size going left or up from it,
/// Blink's way: from 0 if it starts left of it) into a bitmap of `bitmap`
/// size; none for a dirty rect too big for a `long`'s area, or that misses
/// the image or the bitmap.
pub fn put_region(
    image: (u32, u32),
    bitmap: (u32, u32),
    (dx, dy): (i32, i32),
    (dirty_x, dirty_y, dirty_width, dirty_height): (i32, i32, i32, i32),
) -> Option<PutRegion> {
    dirty_width.checked_mul(dirty_height)?;
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "sums and negations of i32s, in i64"
    )]
    let normalized = |start: i32, size: i32| -> (i64, i64) {
        let (start, size) = (i64::from(start), i64::from(size));
        match size {
            size if size >= 0 => (start, size),
            _ if start < 0 => (0, 0),
            size => (start + size, -size),
        }
    };
    let (x, w) = normalized(dirty_x, dirty_width);
    let (y, h) = normalized(dirty_y, dirty_height);
    let (dx, dy) = (i64::from(dx), i64::from(dy));
    let x0 = x.max(0).saturating_add(dx).max(0);
    let y0 = y.max(0).saturating_add(dy).max(0);
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "x and w are i32s', normalized: their sum fits an i64"
    )]
    let x1 = (x + w)
        .min(i64::from(image.0))
        .saturating_add(dx)
        .min(i64::from(bitmap.0));
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "y and h are i32s', normalized: their sum fits an i64"
    )]
    let y1 = (y + h)
        .min(i64::from(image.1))
        .saturating_add(dy)
        .min(i64::from(bitmap.1));
    if x0 >= x1 || y0 >= y1 {
        return None;
    }
    let whole = x0 == 0 && y0 == 0 && x1 == i64::from(bitmap.0) && y1 == i64::from(bitmap.1);
    Some(PutRegion {
        x0,
        y0,
        x1,
        y1,
        whole,
    })
}

/// How `put_pixels` wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Put {
    /// Into the bitmap's pixels here.
    Written,
    /// Recorded as an op (the bitmap is another thread's), this many
    /// recorded now.
    Recorded(usize),
    /// Not at all: no memory for the pixels.
    NoMemory,
}

/// Writes `bytes` (an image `width` wide, not premultiplied) over the
/// bitmap's `region`, the image's pixel `(bx - dx, by - dy)` at `(bx,
/// by)`, as they are: premultiplied (an opaque canvas takes the colors as
/// they are, alpha 255), not through the transform, the clip,
/// `globalAlpha`, the compositing or the shadow. A bitmap another thread
/// has gets them after what was recorded, in order, as an op (Blink
/// flushes, then has the GPU process write them,
/// `CanvasResourceProvider::WritePixels`, waiting for neither); the caller
/// draws what was recorded first for one here, or drops it for the whole
/// bitmap.
pub fn put_pixels(
    data: &mut CanvasData,
    settings: CanvasSettings,
    (bytes, width): (&[u8], u32),
    region: PutRegion,
    (dx, dy): (i32, i32),
) -> Put {
    let PutRegion { x0, y0, x1, y1, .. } = region;
    let (dx, dy) = (i64::from(dx), i64::from(dy));
    let background = crate::background(settings);
    // The image's pixel that goes at (bx, by) in the bitmap, premultiplied.
    let pixel = |bx: i64, by: i64| {
        #[expect(
            clippy::arithmetic_side_effects,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "(bx, by) less (dx, dy) is in the image (the region is cut to it): a \
                      pixel of `bytes`"
        )]
        let source = (((by - dy) * i64::from(width) + (bx - dx)) * 4) as usize;
        let source = &bytes[source..][..4];
        let [r, g, b, a] = [source[0], source[1], source[2], source[3]];
        if settings.alpha {
            premultiply([r, g, b, a])
        } else {
            [r, g, b, 255]
        }
    };
    if data.resource.is_some() {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "x0 < x1 and y0 < y1, all in the bitmap: u32s, in i64"
        )]
        let size = gfx::Size::new(saturated_cast(x1 - x0), saturated_cast(y1 - y0));
        let mut pixels = Vec::new();
        let reserved = size
            .checked_byte_size()
            .is_some_and(|len| pixels.try_reserve_exact(len).is_ok());
        if !reserved {
            return Put::NoMemory;
        }
        for by in y0..y1 {
            for bx in x0..x1 {
                pixels.extend_from_slice(&pixel(bx, by));
            }
        }
        begin_recording(data);
        data.ops.push(PaintOp::WritePixels {
            // In the bitmap: u32s.
            x: saturated_cast(x0),
            y: saturated_cast(y0),
            size,
            pixels: pixels.into(),
        });
        return Put::Recorded(data.ops.len());
    }
    let bitmap_width = data.size.0;
    let Some(target_pixels) = data.pixels_mut(background) else {
        return Put::NoMemory;
    };
    for by in y0..y1 {
        for bx in x0..x1 {
            #[expect(
                clippy::arithmetic_side_effects,
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "0 <= bx < x1 <= the bitmap's width, and the same for by: a pixel \
                          of the bitmap"
            )]
            let target = ((by * i64::from(bitmap_width) + bx) * 4) as usize;
            target_pixels[target..][..4].copy_from_slice(&pixel(bx, by));
        }
    }
    Put::Written
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplying_rounds_as_skia_does() {
        assert_eq!(premultiply([255, 255, 255, 255]), [255, 255, 255, 255]);
        assert_eq!(premultiply([255, 128, 0, 128]), [128, 64, 0, 128]);
        assert_eq!(premultiply([9, 9, 9, 0]), [0, 0, 0, 0]);
    }

    #[test]
    fn unpremultiplying_rounds_to_the_nearest() {
        assert_eq!(unpremultiply([0, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(unpremultiply([128, 0, 64, 128]), [255, 0, 128, 128]);
        assert_eq!(unpremultiply([10, 20, 30, 255]), [10, 20, 30, 255]);
        // 1 of 3 is 85.
        assert_eq!(unpremultiply([1, 0, 0, 3]), [85, 0, 0, 3]);
    }

    #[test]
    fn a_source_rect_goes_either_way() {
        assert_eq!(source_rect(10, 10, -4, 2), (6, 10, 4, 2));
        assert_eq!(
            source_rect(0, 0, i32::MIN, 1),
            (i64::from(i32::MIN), 0, 1 << 31, 1)
        );
    }

    #[test]
    fn a_put_region_is_cut_to_the_image_and_the_bitmap() {
        // The whole of a 4×4 image at (1, 1) of a 3×3 bitmap.
        let region = put_region((4, 4), (3, 3), (1, 1), (0, 0, 4, 4)).unwrap();
        assert_eq!(
            (region.x0, region.y0, region.x1, region.y1, region.whole),
            (1, 1, 3, 3, false)
        );
        // A negative dirty size goes left; from before 0, nothing.
        let region = put_region((4, 4), (8, 8), (0, 0), (3, 0, -2, 1)).unwrap();
        assert_eq!((region.x0, region.x1), (1, 3));
        assert_eq!(put_region((4, 4), (8, 8), (0, 0), (-1, 0, -2, 1)), None);
        assert!(
            put_region((2, 2), (2, 2), (0, 0), (0, 0, 2, 2))
                .unwrap()
                .whole
        );
        assert_eq!(put_region((4, 4), (8, 8), (9, 0), (0, 0, 4, 4)), None);
        assert_eq!(
            put_region((4, 4), (8, 8), (0, 0), (0, 0, i32::MAX, 2)),
            None
        );
    }
}
