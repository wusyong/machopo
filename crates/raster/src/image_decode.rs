//! Image files into pixels (Blink's `ImageDecoder`s): PNG, JPEG, GIF (its
//! first frame), BMP and ICO, told by their content.
//!
//! The decoders are the `image` crate's, which says of itself that many of
//! them panic on malicious input: every call into it is under
//! `catch_unwind`, a panic one more way of not being an image. That needs
//! the workspace to unwind on panics.

use std::io::Cursor;

use gfx::Size;
use image::metadata::Orientation;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};

/// An image's pixels: RGBA8, premultiplied, rows tightly packed, turned
/// as its EXIF orientation says.
pub(crate) struct Decoded {
    pub size: Size,
    pub pixels: Vec<u8>,
}

/// The most pixels an image may have: its RGBA8 bytes stay below 2^31
/// (Blink's `ImageDecoder::SizeCalculationMayOverflow`). A header that
/// claims more isn't an image, so nothing is allocated for it.
const MAX_PIXELS: u64 = (1 << 29) - 1;

/// What the decoders may allocate for one image, on top of its pixels'
/// own limit: twice its largest RGBA8 (a decoder's own rows and tables,
/// then the pixels).
const MAX_ALLOC: u64 = MAX_PIXELS * 4 * 2;

/// An encoded image's size in pixels, from its header: no decoding. As it
/// is shown: a JPEG whose EXIF orientation turns it a quarter has its
/// width and height swapped (CSS's `image-orientation: from-image`).
/// `None` if it isn't a format read here, or claims more pixels than an
/// image may have.
pub fn image_size(bytes: &[u8]) -> Option<Size> {
    std::panic::catch_unwind(|| {
        let mut decoder = decoder(bytes)?;
        let size = stored_size(&decoder)?;
        Some(oriented(size, orientation(&mut decoder)))
    })
    .ok()
    .flatten()
}

/// `bytes` decoded; `None` if they aren't an image read here, are broken,
/// or the decoder panicked on them.
pub(crate) fn decode(bytes: &[u8]) -> Option<Decoded> {
    std::panic::catch_unwind(|| {
        let mut decoder = decoder(bytes)?;
        stored_size(&decoder)?;
        let orientation = orientation(&mut decoder);
        let mut image = DynamicImage::from_decoder(decoder).ok()?;
        image.apply_orientation(orientation);
        let image = image.into_rgba8();
        let size = Size::new(image.width(), image.height());
        let mut pixels = image.into_raw();
        premultiply(&mut pixels);
        Some(Decoded { size, pixels })
    })
    .ok()
    .flatten()
}

/// The decoder for `bytes`' format, their header read.
fn decoder(bytes: &[u8]) -> Option<impl ImageDecoder + '_> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader.into_decoder().ok()
}

/// The size of the image as the file stores it; `None` for one past
/// `MAX_PIXELS`. (The decoders take no header of an empty image.)
fn stored_size(decoder: &impl ImageDecoder) -> Option<Size> {
    let (width, height) = decoder.dimensions();
    let pixels = u64::from(width).checked_mul(u64::from(height))?;
    (pixels <= MAX_PIXELS).then_some(Size::new(width, height))
}

/// How the image is turned to be shown; as it is for metadata that can't
/// be read.
fn orientation(decoder: &mut impl ImageDecoder) -> Orientation {
    decoder.orientation().unwrap_or(Orientation::NoTransforms)
}

/// `size` once turned by `orientation`: swapped by a quarter turn.
fn oriented(size: Size, orientation: Orientation) -> Size {
    match orientation {
        Orientation::Rotate90
        | Orientation::Rotate270
        | Orientation::Rotate90FlipH
        | Orientation::Rotate270FlipH => Size::new(size.height, size.width),
        Orientation::NoTransforms
        | Orientation::Rotate180
        | Orientation::FlipHorizontal
        | Orientation::FlipVertical => size,
    }
}

/// RGBA8 pixels premultiplied in place, each channel rounded as Skia
/// rounds it.
fn premultiply(pixels: &mut [u8]) {
    for pixel in pixels.as_chunks_mut::<4>().0 {
        let alpha = pixel[3];
        if alpha == 255 {
            continue;
        }
        for channel in &mut pixel[..3] {
            *channel = mul_div_255(*channel, alpha);
        }
    }
}

/// `c * a / 255`, rounded (Skia's `SkMulDiv255Round`).
#[expect(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    reason = "c, a <= 255: c * a + 128 <= 65153, plus its high byte <= 65407, \
              which shifted down is a byte"
)]
pub(crate) fn mul_div_255(c: u8, a: u8) -> u8 {
    let product = u32::from(c) * u32::from(a) + 128;
    ((product + (product >> 8)) >> 8) as u8
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const PNG: &[u8] =
        include_bytes!("../../../tests/reftests/images/resources/green-40x30.png");
    pub(crate) const JPEG: &[u8] =
        include_bytes!("../../../tests/reftests/images/resources/green-40x30.jpg");
    pub(crate) const GIF: &[u8] =
        include_bytes!("../../../tests/reftests/images/resources/green-40x30.gif");
    const GREEN_BLUE: &[u8] =
        include_bytes!("../../../tests/reftests/images/resources/green-blue-20x10.png");

    /// A 24-bit BMP of `width` × `height`, every pixel `rgb`.
    pub(crate) fn bmp(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
        let row = (width * 3).next_multiple_of(4);
        let mut out = vec![b'B', b'M'];
        out.extend((54 + row * height).to_le_bytes());
        out.extend([0; 4]);
        out.extend(54u32.to_le_bytes());
        out.extend(40u32.to_le_bytes());
        out.extend(width.to_le_bytes());
        out.extend(height.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(24u16.to_le_bytes());
        out.extend([0; 24]);
        for _ in 0..height {
            for _ in 0..width {
                out.extend([rgb[2], rgb[1], rgb[0]]);
            }
            out.extend(std::iter::repeat_n(0, (row - width * 3) as usize));
        }
        out
    }

    /// An ICO of these PNG files, each entry saying `(width, height, bits
    /// per pixel)`.
    fn ico(entries: &[(u8, u8, u16, &[u8])]) -> Vec<u8> {
        let mut out = vec![0, 0, 1, 0];
        out.extend((entries.len() as u16).to_le_bytes());
        let mut offset = 6 + 16 * entries.len() as u32;
        for (width, height, bits, png) in entries {
            out.extend([*width, *height, 0, 0, 1, 0]);
            out.extend(bits.to_le_bytes());
            out.extend((png.len() as u32).to_le_bytes());
            out.extend(offset.to_le_bytes());
            offset += png.len() as u32;
        }
        for (.., png) in entries {
            out.extend_from_slice(png);
        }
        out
    }

    /// A PNG of `width` × `height` RGBA8 `pixels` (not premultiplied).
    pub(crate) fn png_of(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut encoder = png::Encoder::new(&mut out, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(pixels).unwrap();
        writer.finish().unwrap();
        out
    }

    /// `jpeg` with an EXIF segment after its SOI that says `orientation`
    /// (1 to 8), any APP1 it had taken out.
    pub(crate) fn with_orientation(jpeg: &[u8], orientation: u8) -> Vec<u8> {
        assert_eq!(jpeg[..2], [0xff, 0xd8]);
        let mut out = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 34];
        out.extend(b"Exif\0\0");
        // A little-endian TIFF: one IFD of one entry, tag 0x0112
        // (orientation), a SHORT.
        out.extend([0x49, 0x49, 0x2a, 0x00, 8, 0, 0, 0]);
        out.extend([1, 0]);
        out.extend([0x12, 0x01, 3, 0, 1, 0, 0, 0, orientation, 0, 0, 0]);
        out.extend([0, 0, 0, 0]);
        // The rest, segment by segment up to the scan, without its APP1s.
        let mut at = 2;
        while jpeg[at] == 0xff && jpeg[at + 1] != 0xda {
            let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
            if jpeg[at + 1] != 0xe1 {
                out.extend_from_slice(&jpeg[at..at + 2 + length]);
            }
            at += 2 + length;
        }
        out.extend_from_slice(&jpeg[at..]);
        out
    }

    fn pixel(image: &Decoded, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * image.size.width + x) * 4) as usize;
        image.pixels[at..at + 4].try_into().unwrap()
    }

    /// Each format's size is read and its pixels decoded; JPEG's within
    /// what its compression loses.
    #[test]
    fn every_format_is_measured_and_decoded() {
        let bmp = bmp(40, 30, [0, 128, 0]);
        for (name, bytes, exact) in [
            ("png", PNG, true),
            ("jpeg", JPEG, false),
            ("gif", GIF, true),
            ("bmp", &bmp[..], true),
        ] {
            assert_eq!(image_size(bytes), Some(Size::new(40, 30)), "{name}");
            let image = decode(bytes).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(image.size, Size::new(40, 30), "{name}");
            assert_eq!(image.pixels.len(), 40 * 30 * 4, "{name}");
            for (x, y) in [(0, 0), (39, 0), (20, 15), (0, 29), (39, 29)] {
                let [r, g, b, a] = pixel(&image, x, y);
                if exact {
                    assert_eq!([r, g, b, a], [0, 128, 0, 255], "{name} at {x}, {y}");
                } else {
                    assert!(
                        r <= 3 && (125..=131).contains(&g) && b <= 3 && a == 255,
                        "{name} at {x}, {y}: {:?}",
                        [r, g, b, a]
                    );
                }
            }
        }
    }

    /// A PNG's pixels come out where they are in the file: left green,
    /// right blue.
    #[test]
    fn pixels_keep_their_places() {
        let image = decode(GREEN_BLUE).unwrap();
        assert_eq!(image.size, Size::new(20, 10));
        assert_eq!(pixel(&image, 2, 5), [0, 128, 0, 255]);
        assert_eq!(pixel(&image, 17, 5), [0, 0, 255, 255]);
    }

    /// Pixels with alpha are premultiplied, rounded as Skia rounds.
    #[test]
    fn decoded_pixels_are_premultiplied() {
        let png = png_of(3, 1, &[255, 100, 0, 128, 10, 20, 30, 0, 1, 2, 3, 255]);
        let image = decode(&png).unwrap();
        // 255 * 128 / 255 = 128; 100 * 128 / 255 = 50.2.
        assert_eq!(pixel(&image, 0, 0), [128, 50, 0, 128]);
        assert_eq!(pixel(&image, 1, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(&image, 2, 0), [1, 2, 3, 255]);
        // Halves round up: 1 * 128 / 255 = 0.502, 3 * 85 / 255 = 1.
        let mut pixels = [1, 3, 127, 128, 3, 2, 1, 85];
        premultiply(&mut pixels);
        assert_eq!(pixels, [1, 2, 64, 128, 1, 1, 0, 85]);
    }

    /// An ICO is its entry of the most bits a pixel, then the largest
    /// (Blink's `ICOImageDecoder` sorts by size first; they differ only
    /// for a file whose larger icon has fewer colors). An entry that is a
    /// PNG has to be an RGBA one, as the format says; Skia read any.
    #[test]
    fn an_ico_is_its_best_entry() {
        let green = png_of(2, 4, &[0, 128, 0, 255].repeat(8));
        let blue = png_of(4, 3, &[0, 0, 255, 255].repeat(12));
        let one = ico(&[(2, 4, 32, &green)]);
        assert_eq!(image_size(&one), Some(Size::new(2, 4)));
        assert_eq!(pixel(&decode(&one).unwrap(), 1, 2), [0, 128, 0, 255]);
        for two in [
            ico(&[(2, 4, 32, &green), (4, 3, 32, &blue)]),
            ico(&[(4, 3, 32, &blue), (2, 4, 32, &green)]),
        ] {
            assert_eq!(image_size(&two), Some(Size::new(4, 3)));
            let image = decode(&two).unwrap();
            assert_eq!(image.size, Size::new(4, 3));
            assert_eq!(pixel(&image, 1, 1), [0, 0, 255, 255]);
        }
        let fewer_colors = ico(&[(2, 4, 32, &green), (4, 3, 8, &blue)]);
        assert_eq!(image_size(&fewer_colors), Some(Size::new(2, 4)));
        // A PNG without alpha in an ICO isn't read.
        let rgb = ico(&[(40, 30, 32, PNG)]);
        assert!(decode(&rgb).is_none());
    }

    /// A JPEG of 16 × 8: its left half red, its right half blue.
    fn red_blue_jpeg() -> Vec<u8> {
        let mut rgb = Vec::new();
        for _ in 0..8 {
            for x in 0..16 {
                rgb.extend(if x < 8 { [255, 0, 0] } else { [0, 0, 255] });
            }
        }
        let mut out = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut out, 100);
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
        encoder
            .encode(&rgb, 16, 8, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        out
    }

    /// A JPEG's EXIF orientation turns its size and its pixels alike, all
    /// eight of them: where the red half (stored on the left) ends up.
    #[test]
    fn a_jpegs_orientation_turns_its_size_and_pixels() {
        let jpeg = red_blue_jpeg();
        assert_eq!(image_size(&jpeg), Some(Size::new(16, 8)));
        // (orientation, the size, a point in the red half, one in the blue).
        let wide = Size::new(16, 8);
        let tall = Size::new(8, 16);
        let cases = [
            (1, wide, (3, 4), (12, 4)),
            (2, wide, (12, 4), (3, 4)),
            (3, wide, (12, 4), (3, 4)),
            (4, wide, (3, 4), (12, 4)),
            (5, tall, (4, 3), (4, 12)),
            (6, tall, (4, 3), (4, 12)),
            (7, tall, (4, 12), (4, 3)),
            (8, tall, (4, 12), (4, 3)),
        ];
        for (orientation, size, red, blue) in cases {
            let jpeg = with_orientation(&jpeg, orientation);
            assert_eq!(image_size(&jpeg), Some(size), "orientation {orientation}");
            let image = decode(&jpeg).unwrap();
            assert_eq!(image.size, size, "orientation {orientation}");
            let [r, _, b, _] = pixel(&image, red.0, red.1);
            assert!(r > 200 && b < 50, "orientation {orientation}: red");
            let [r, _, b, _] = pixel(&image, blue.0, blue.1);
            assert!(r < 50 && b > 200, "orientation {orientation}: blue");
        }
    }

    /// What isn't an image, or is a broken one, is none: no size, no
    /// pixels, no panic.
    #[test]
    fn broken_files_are_no_image() {
        assert_eq!(image_size(b""), None);
        assert_eq!(image_size(b"not an image"), None);
        assert!(decode(b"not an image").is_none());
        // A format `image` knows the signature of but that isn't read here.
        let webp = b"RIFF\x1a\0\0\0WEBPVP8 \x0e\0\0\0";
        assert_eq!(image_size(webp), None);
        assert!(decode(webp).is_none());
        for (name, bytes) in [("png", PNG), ("jpeg", JPEG), ("gif", GIF)] {
            // Cut in its header: no size.
            assert_eq!(image_size(&bytes[..10]), None, "{name}");
            assert!(decode(&bytes[..10]).is_none(), "{name}");
            // Every byte after the header a different one: whatever comes
            // of it, nothing panics out of here.
            let mut garbled = bytes.to_vec();
            for byte in &mut garbled[24..] {
                *byte = byte.wrapping_mul(31).wrapping_add(7);
            }
            let _ = image_size(&garbled);
            let _ = decode(&garbled);
        }
    }

    /// `jpeg` with its frame header saying `width` × `height`.
    fn claiming(jpeg: &[u8], width: u16, height: u16) -> Vec<u8> {
        let mut out = jpeg.to_vec();
        let sof = out
            .windows(2)
            .position(|w| w == [0xff, 0xc0] || w == [0xff, 0xc2])
            .unwrap();
        // Marker, length (2), precision, then height and width.
        out[sof + 5..sof + 7].copy_from_slice(&height.to_be_bytes());
        out[sof + 7..sof + 9].copy_from_slice(&width.to_be_bytes());
        out
    }

    /// A header claiming more pixels than an image may have isn't one:
    /// it has no size, and nothing is allocated to decode it. The most an
    /// image may have, 2^29 - 1, is between 23,170 and 23,171 squared.
    #[test]
    fn a_header_claiming_too_many_pixels_is_no_image() {
        let huge = claiming(JPEG, 60_000, 60_000);
        assert_eq!(image_size(&huge), None);
        assert!(decode(&huge).is_none());
        let over = claiming(JPEG, 23_171, 23_171);
        assert_eq!(image_size(&over), None);
        assert!(decode(&over).is_none());
        // Just below it is measured (decoding it would take 2 GB: not
        // here).
        let under = claiming(JPEG, 23_170, 23_170);
        assert_eq!(image_size(&under), Some(Size::new(23_170, 23_170)));
        // An image of no pixels isn't one either: the decoders say so.
        assert_eq!(image_size(&claiming(JPEG, 0, 30)), None);
        assert_eq!(image_size(&bmp(0, 30, [0, 0, 0])), None);
    }
}
