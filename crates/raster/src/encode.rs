//! A bitmap's pixels as an image file: a canvas's `toDataURL` and
//! `toBlob` (Blink's `ImageEncoder`, with its encoders and settings).

use gfx::Size;
use skia_safe as sk;

/// `pixels` (RGBA8, premultiplied, rows tightly packed, `size`; `opaque`
/// if every alpha is 255) as a PNG, as Blink encodes one: Skia's Rust PNG
/// encoder (the `png` crate) at its low compression level, deflate level
/// 1 and the Up filter (`SkPngRustEncoder::CompressionLevel::kLow`), the
/// pixels not premultiplied, without alpha when opaque. `None` if the
/// size is empty or the pixels don't fit it.
pub fn encode_png(pixels: &[u8], size: Size, opaque: bool) -> Option<Vec<u8>> {
    let rgba = unpremultiplied(pixels, size, opaque)?;
    let (color, data) = if opaque {
        let rgb: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        (png::ColorType::Rgb, rgb)
    } else {
        (png::ColorType::Rgba, rgba)
    };
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, size.width, size.height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_deflate_compression(png::DeflateCompression::Level(1));
        encoder.set_filter(png::Filter::Up);
        let mut writer = encoder.write_header().ok()?;
        writer.write_image_data(&data).ok()?;
        writer.finish().ok()?;
    }
    Some(out)
}

/// `pixels` (as `encode_png`'s) as a JPEG of `quality` (0 to 100), as
/// Blink encodes one: Skia's encoder, the pixels not premultiplied and
/// blended on black, 4:2:0 but at 100, 4:4:4. `None` if the size is empty
/// or past JPEG's (65,500 a side), or the pixels don't fit it.
pub fn encode_jpeg(pixels: &[u8], size: Size, opaque: bool, quality: u8) -> Option<Vec<u8>> {
    let mut rgba = unpremultiplied(pixels, size, opaque)?;
    let info = image_info(size, sk::AlphaType::Unpremul);
    let pixmap = sk::Pixmap::new(&info, &mut rgba, info.min_row_bytes())?;
    let options = sk::jpeg_encoder::Options {
        quality: u32::from(quality),
        downsample: if quality == 100 {
            sk::jpeg_encoder::Downsample::No
        } else {
            sk::jpeg_encoder::Downsample::BothDirections
        },
        alpha_option: sk::jpeg_encoder::AlphaOption::BlendOnBlack,
        ..Default::default()
    };
    let mut out = Vec::new();
    sk::jpeg_encoder::encode(&pixmap, &mut out, &options).then_some(out)
}

/// The info of an image of `size`, which its callers have checked fits
/// Skia's `i32`s (`unpremultiplied`).
fn image_info(size: Size, alpha: sk::AlphaType) -> sk::ImageInfo {
    use base::numerics::saturated_cast;
    sk::ImageInfo::new(
        (saturated_cast(size.width), saturated_cast(size.height)),
        sk::ColorType::RGBA8888,
        alpha,
        None,
    )
}

/// `pixels` made not premultiplied as Skia does (`readPixels`, as Blink
/// reads a canvas's for its encoder).
fn unpremultiplied(pixels: &[u8], size: Size, opaque: bool) -> Option<Vec<u8>> {
    let width = i32::try_from(size.width).ok()?;
    let height = i32::try_from(size.height).ok()?;
    if width == 0 || height == 0 || Some(pixels.len()) != size.checked_byte_size() {
        return None;
    }
    let alpha = if opaque {
        sk::AlphaType::Opaque
    } else {
        sk::AlphaType::Premul
    };
    let source_info = image_info(size, alpha);
    let row_bytes = source_info.min_row_bytes();
    // Skia's pixmap borrows its pixels mutably, though reading only.
    let mut source_pixels = pixels.to_vec();
    let source = sk::Pixmap::new(&source_info, &mut source_pixels, row_bytes)?;
    let target_info = image_info(size, sk::AlphaType::Unpremul);
    let mut out = vec![0; pixels.len()];
    source
        .read_pixels(&target_info, &mut out, row_bytes, (0, 0))
        .then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2 × 1: opaque red, then half transparent blue (premultiplied).
    fn pixels() -> Vec<u8> {
        vec![255, 0, 0, 255, 0, 0, 128, 128]
    }

    fn decoded(bytes: &[u8]) -> (png::OutputInfo, Vec<u8>) {
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
            .read_info()
            .unwrap();
        let mut out = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut out).unwrap();
        out.truncate(info.buffer_size());
        (info, out)
    }

    /// The pixels not premultiplied, with alpha; without when opaque.
    #[test]
    fn a_png_is_the_pixels_not_premultiplied() {
        let png = encode_png(&pixels(), Size::new(2, 1), false).unwrap();
        let (info, out) = decoded(&png);
        assert_eq!(info.color_type, png::ColorType::Rgba);
        assert_eq!(out, [255, 0, 0, 255, 0, 0, 255, 128]);
        let opaque = encode_png(&[1, 2, 3, 255, 4, 5, 6, 255], Size::new(2, 1), true).unwrap();
        let (info, out) = decoded(&opaque);
        assert_eq!(info.color_type, png::ColorType::Rgb);
        assert_eq!(out, [1, 2, 3, 4, 5, 6]);
    }

    /// Blink's settings: deflate level 1 (the zlib header's FLEVEL, its
    /// second byte's top two bits, is 0: the fastest) and each row
    /// filtered Up (its first byte 2).
    #[test]
    fn a_png_has_blinks_settings() {
        let png = encode_png(&[7; 4 * 4 * 3], Size::new(4, 3), false).unwrap();
        let idat = png.windows(4).position(|w| w == b"IDAT").unwrap() + 4;
        let length = u32::from_be_bytes(png[idat - 8..idat - 4].try_into().unwrap()) as usize;
        let zlib = &png[idat..idat + length];
        assert_eq!(zlib[1] >> 6, 0, "{:x?}", &zlib[..2]);
        let raw = fdeflate::decompress_to_vec(zlib).unwrap();
        assert_eq!(raw.len(), 3 * (1 + 4 * 4));
        for row in raw.chunks(1 + 4 * 4) {
            assert_eq!(row[0], 2);
        }
    }

    /// A JPEG's pixel at `(x, y)`.
    fn jpeg_pixel(jpeg: &[u8], x: i32, y: i32) -> [u8; 4] {
        let image = sk::Image::from_encoded(sk::Data::new_copy(jpeg)).unwrap();
        let info = image_info(
            Size::new(image.width() as u32, image.height() as u32),
            sk::AlphaType::Unpremul,
        );
        let mut out = [0u8; 4];
        let one = info.with_dimensions((1, 1));
        assert!(image.read_pixels(&one, &mut out, 4, (x, y), sk::image::CachingHint::Allow));
        out
    }

    /// The sampling factors of a JPEG's first (luma) component: 0x22 for
    /// 4:2:0, 0x11 for 4:4:4.
    fn luma_sampling(jpeg: &[u8]) -> u8 {
        let sof = jpeg.windows(2).position(|w| w == [0xff, 0xc0]).unwrap();
        // Marker, length (2), precision, height (2), width (2), count,
        // then the first component's id and sampling factors.
        jpeg[sof + 2 + 2 + 1 + 2 + 2 + 1 + 1]
    }

    /// Half transparent blue, blended on black: half as bright.
    #[test]
    fn a_jpeg_is_blended_on_black() {
        let blue: Vec<u8> = [0, 0, 128, 128].repeat(16 * 16);
        let jpeg = encode_jpeg(&blue, Size::new(16, 16), false, 100).unwrap();
        assert_eq!(&jpeg[..2], [0xff, 0xd8]);
        let [r, g, b, a] = jpeg_pixel(&jpeg, 8, 8);
        assert!(
            r < 8 && g < 8 && (120..=136).contains(&b) && a == 255,
            "{:?}",
            [r, g, b, a]
        );
    }

    /// Its quality: a lower one smaller; 4:4:4 only at 100.
    #[test]
    fn a_jpegs_quality() {
        let gradient: Vec<u8> = (0..32 * 32)
            .flat_map(|i| [(i % 32 * 8) as u8, (i / 32 * 8) as u8, 128, 255])
            .collect();
        let size = Size::new(32, 32);
        let low = encode_jpeg(&gradient, size, true, 10).unwrap();
        let high = encode_jpeg(&gradient, size, true, 92).unwrap();
        let best = encode_jpeg(&gradient, size, true, 100).unwrap();
        assert!(low.len() < high.len() && high.len() < best.len());
        assert_eq!(luma_sampling(&high), 0x22);
        assert_eq!(luma_sampling(&best), 0x11);
    }

    #[test]
    fn nothing_for_no_pixels() {
        assert!(encode_png(&[], Size::new(0, 1), false).is_none());
        assert!(encode_png(&[0; 4], Size::new(2, 1), false).is_none());
        assert!(encode_jpeg(&[], Size::new(1, 0), false, 92).is_none());
    }
}
