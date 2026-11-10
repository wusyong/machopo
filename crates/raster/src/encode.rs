//! A bitmap's pixels as an image file: a canvas's `toDataURL` and
//! `toBlob` (Blink's `ImageEncoder`, with its encoders and settings).

use gfx::Size;

use crate::image_decode::mul_div_255;

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
/// Blink encodes one (Skia's encoder, libjpeg-turbo): the pixels not
/// premultiplied and blended on black, 4:2:0 but at 100, 4:4:4; the
/// quantization tables libjpeg's, scaled by the quality as it scales
/// them. `None` if the size is empty or past JPEG's (65,500 a side, as
/// libjpeg), or the pixels don't fit it.
pub fn encode_jpeg(pixels: &[u8], size: Size, opaque: bool, quality: u8) -> Option<Vec<u8>> {
    /// libjpeg's `JPEG_MAX_DIMENSION`.
    const MAX_SIDE: u16 = 65_500;
    let rgba = unpremultiplied(pixels, size, opaque)?;
    let width = u16::try_from(size.width).ok().filter(|&w| w <= MAX_SIDE)?;
    let height = u16::try_from(size.height).ok().filter(|&h| h <= MAX_SIDE)?;
    // On black, a color is itself times its alpha: premultiplied again,
    // as Skia's encoder does to the pixels it was handed.
    let rgb: Vec<u8> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            if opaque {
                [r, g, b]
            } else {
                [mul_div_255(r, a), mul_div_255(g, a), mul_div_255(b, a)]
            }
        })
        .collect();
    let mut out = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut out, quality);
    encoder.set_sampling_factor(if quality == 100 {
        jpeg_encoder::SamplingFactor::R_4_4_4
    } else {
        jpeg_encoder::SamplingFactor::R_4_2_0
    });
    encoder
        .encode(&rgb, width, height, jpeg_encoder::ColorType::Rgb)
        .ok()?;
    Some(out)
}

/// `pixels` made not premultiplied as Skia does (`readPixels`, as Blink
/// reads a canvas's for its encoder): each color divided by its alpha in
/// `f32`, the way Skia's pipeline does it (the byte as a fraction of 255,
/// times the reciprocal of the alpha's, back to a byte, a half going to
/// the even one), so that the bytes are the ones Skia gives; transparent
/// pixels' colors are 0. `opaque` pixels are as they are. `None` if the
/// size is empty or the pixels don't fit it.
fn unpremultiplied(pixels: &[u8], size: Size, opaque: bool) -> Option<Vec<u8>> {
    if size.is_empty() || Some(pixels.len()) != size.checked_byte_size() {
        return None;
    }
    let mut out = pixels.to_vec();
    if opaque {
        return Some(out);
    }
    for pixel in out.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha == 255 {
            continue;
        }
        let reciprocal = 1.0 / (f32::from(alpha) * (1.0 / 255.0));
        let scale = if reciprocal.is_finite() {
            reciprocal
        } else {
            0.0
        };
        for channel in &mut pixel[..3] {
            let color = f32::from(*channel) * (1.0 / 255.0) * scale;
            // A color past its alpha (not a premultiplied one) stops at 255.
            *channel = base::numerics::saturated_cast((color * 255.0).round_ties_even());
        }
    }
    Some(out)
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

    /// A color over its alpha, as Skia rounds it: 1 / 6 of 255 is 42.5
    /// and comes out 42, 5 / 6 is 212.5 and comes out 212 (the nearest
    /// integer would be 43 and 213). A color past its alpha is 255;
    /// a transparent pixel's is 0; opaque pixels aren't touched.
    #[test]
    fn unpremultiplying_rounds_as_skia_does() {
        let size = Size::new(4, 1);
        let pixels = [1, 5, 3, 6, 10, 2, 12, 12, 200, 7, 0, 100, 9, 9, 9, 0];
        assert_eq!(
            unpremultiplied(&pixels, size, false).unwrap(),
            [
                42, 212, 128, 6, 212, 42, 255, 12, 255, 18, 0, 100, 0, 0, 0, 0
            ]
        );
        assert_eq!(unpremultiplied(&pixels, size, true).unwrap(), pixels);
    }

    const HASH_PREMUL: u64 = 0xcf8c_04a6_36d9_e851;
    const HASH_OPAQUE: u64 = 0x10cd_9ffd_6970_1b25;

    /// Every color at every alpha comes out the bytes Skia's `readPixels`
    /// gave (skia-safe 0.153, compared byte for byte while raster still
    /// had it): their FNV-1a hashes.
    #[test]
    fn unpremultiplying_is_skias_for_every_color_and_alpha() {
        let mut pixels = Vec::new();
        for a in 0..=255u8 {
            for c in 0..=255u8 {
                pixels.extend([c, c / 2, 255 - c, a]);
            }
        }
        let hash = |bytes: &[u8]| {
            bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |hash, byte| {
                (hash ^ u64::from(*byte)).wrapping_mul(0x0100_0000_01b3)
            })
        };
        let size = Size::new(256, 256);
        let hashes =
            [false, true].map(|opaque| hash(&unpremultiplied(&pixels, size, opaque).unwrap()));
        assert_eq!(hashes, [HASH_PREMUL, HASH_OPAQUE], "{hashes:x?}");
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
    fn jpeg_pixel(jpeg: &[u8], x: u32, y: u32) -> [u8; 4] {
        let image = crate::image_decode::decode(jpeg).unwrap();
        let at = ((y * image.size.width + x) * 4) as usize;
        image.pixels[at..at + 4].try_into().unwrap()
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

    /// The quantization tables are libjpeg's: at quality 50 the standard
    /// ones as they are (the luminance one begins 16, 11, 12, 14 in the
    /// file's zigzag order), at 25 twice that.
    #[test]
    fn a_jpegs_tables_are_libjpegs() {
        let gray = [128u8, 128, 128, 255].repeat(8 * 8);
        let table = |quality| {
            let jpeg = encode_jpeg(&gray, Size::new(8, 8), true, quality).unwrap();
            let dqt = jpeg.windows(2).position(|w| w == [0xff, 0xdb]).unwrap();
            // Marker, length (2), precision and id, then the table.
            jpeg[dqt + 5..dqt + 9].to_vec()
        };
        assert_eq!(table(50), [16, 11, 12, 14]);
        assert_eq!(table(25), [32, 22, 24, 28]);
    }

    #[test]
    fn nothing_for_no_pixels() {
        assert!(encode_png(&[], Size::new(0, 1), false).is_none());
        assert!(encode_png(&[0; 4], Size::new(2, 1), false).is_none());
        assert!(encode_jpeg(&[], Size::new(1, 0), false, 92).is_none());
        // Past what a JPEG's side may be.
        assert!(encode_jpeg(&vec![0; 65_501 * 4], Size::new(65_501, 1), true, 92).is_none());
        assert!(encode_jpeg(&vec![0; 65_500 * 4], Size::new(65_500, 1), true, 92).is_some());
    }
}
