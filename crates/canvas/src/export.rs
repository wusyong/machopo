//! A canvas's pixels as an image file (`toDataURL`, `toBlob`): the file
//! type and quality script asks for, as the DOM reads them (Blink's
//! `ImageEncoderUtils`), and what the bitmap is encoded from. The encoders
//! are the renderer's (Blink's `ImageEncoder`).

use base::numerics::clamp_floor;

use crate::CanvasData;

/// What a canvas's pixels are encoded as (Blink's `ImageEncodingMimeType`
/// and its quality).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageEncoding {
    Png,
    /// Of `quality`, 0 to 100.
    Jpeg {
        quality: u8,
    },
}

impl ImageEncoding {
    /// Its file's MIME type.
    pub fn mime_type(self) -> &'static str {
        match self {
            Self::Png => "image/png",
            Self::Jpeg { .. } => "image/jpeg",
        }
    }
}

/// The encoding `mime_type` names (ASCII case-insensitively: `image/png`
/// or `image/jpeg`; anything else, `image/webp` too, is PNG), a JPEG's
/// quality from `quality`, the argument if it is a number (`jpeg_quality`):
/// Blink's `ToEncodingMimeType`.
pub fn image_encoding(mime_type: &str, quality: Option<f64>) -> ImageEncoding {
    if mime_type.eq_ignore_ascii_case("image/jpeg") {
        ImageEncoding::Jpeg {
            quality: jpeg_quality(quality),
        }
    } else {
        ImageEncoding::Png
    }
}

/// A JPEG's quality, 0 to 100, from the number given (Blink's
/// `ComputeJpegQuality`): rounded from 0 to 1 times 100; 92 for none, or
/// one outside that.
pub fn jpeg_quality(quality: Option<f64>) -> u8 {
    quality
        .filter(|q| (0.0..=1.0).contains(q))
        .map_or(92, |q| clamp_floor::<u8>(q * 100.0 + 0.5))
}

/// The `SecurityError`'s message for a canvas that isn't origin-clean,
/// Blink's.
pub const TAINTED_MESSAGE: &str = "Tainted canvases may not be exported.";

/// A bitmap to encode: its pixels (RGBA8, premultiplied; for a bitmap
/// another thread has, a snapshot it fills in later), its size, and
/// whether it is opaque (a context without alpha).
#[derive(Debug, Clone)]
pub struct ExportBitmap {
    pub pixels: gfx::ImageData,
    pub size: (u32, u32),
    pub opaque: bool,
}

/// A canvas's bitmap as it is now, to encode (`size` valid, what was
/// recorded drawn first or sent): its snapshot, which a bitmap another
/// thread has fills in later (`gfx::ImageData::wait_ready`), or transparent
/// black for a bitmap with nothing drawn (an opaque one's snapshot is its
/// black) or no context (`data` `None`). `None` without the memory for
/// them.
pub fn export_bitmap(
    data: Option<&mut CanvasData>,
    background: [u8; 4],
    size: (u32, u32),
) -> Option<ExportBitmap> {
    let opaque = data.is_some() && background[3] == 255;
    // `CanvasData::snapshot` gives another thread's bitmap's as it is.
    let pixels = match data.and_then(|data| data.snapshot(background)) {
        Some(pixels) => pixels,
        None => transparent(size)?,
    };
    Some(ExportBitmap {
        pixels,
        size,
        opaque,
    })
}

/// `size`'s pixels, transparent black: made for a valid size up to a GB
/// (Blink's `CreateTransparentImage`), so asked for rather than taken.
fn transparent(size: (u32, u32)) -> Option<gfx::ImageData> {
    let size = gfx::Size::new(size.0, size.1);
    let len = size.checked_byte_size()?;
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(len).ok()?;
    bytes.resize(len, 0);
    Some(gfx::ImageData::decoded(size, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qualities() {
        assert_eq!(jpeg_quality(None), 92);
        assert_eq!(jpeg_quality(Some(0.0)), 0);
        assert_eq!(jpeg_quality(Some(0.926)), 93);
        assert_eq!(jpeg_quality(Some(1.5)), 92);
        assert_eq!(
            image_encoding("IMAGE/JPEG", Some(1.0)),
            ImageEncoding::Jpeg { quality: 100 }
        );
        assert_eq!(image_encoding("image/webp", Some(1.0)), ImageEncoding::Png);
    }

    /// No context: transparent black, not opaque.
    #[test]
    fn a_canvas_with_no_context_is_transparent() {
        let bitmap = export_bitmap(None, [0, 0, 0, 255], (2, 1)).expect("pixels");
        assert_eq!(bitmap.pixels.as_bytes(), &[0; 8]);
        assert!(!bitmap.opaque);
    }
}
