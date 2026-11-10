//! Drawing canvases' 2D contexts: what a context recorded (the DOM records
//! each call as paint ops, kept with the context) is played onto its
//! bitmap with Skia (`raster`), before a frame is painted, when script
//! reads the pixels, or when it recorded too much (Blink's
//! `CanvasResourceProvider` flushing the context's
//! `MemoryManagedPaintRecorder`). The bitmap is moved to the canvas raster
//! thread (`canvas_raster`) the first time it is drawn on, as Chromium's
//! accelerated canvas is in the GPU process; one made with
//! `willReadFrequently` stays on the main thread, drawn there (Blink's
//! `kPreferCPU`).

use canvas::{CanvasData, CanvasFlush};
use compositor_paint::DisplayItemList;
use gfx::{ImageData, Size};

use crate::canvas_raster::{CanvasRaster, Item};

/// Draws what a context made with `settings` recorded in its `data` onto
/// its bitmap, or adds it to `items` for the thread that has the bitmap,
/// the recording gone after; for `Read`, with a snapshot of what it draws,
/// which that thread fills in. A bitmap whose size can have no pixels (or
/// that can't get the memory for them) draws nothing.
pub(crate) fn flush_data(
    data: &mut CanvasData,
    settings: canvas::CanvasSettings,
    flush: CanvasFlush,
    raster: &mut CanvasRaster,
    items: &mut Vec<Item>,
) {
    let data = &mut *data;
    let background = canvas::background(settings);
    let ops = std::mem::take(&mut data.ops);
    data.matrix = None;
    // Read often, as script says or as it did: drawn here, where reading
    // waits for nothing (Blink's CPU canvas).
    if settings.will_read_frequently || data.drawn_here {
        draw_here(data, ops, !settings.alpha, background, raster);
        return;
    }
    let id = match &data.resource {
        Some(resource) => resource.id(),
        None if ops.is_empty() || !data.has_valid_size() => return,
        // Moved to the thread, with the pixels it had here, if any.
        None => {
            let resource = raster.new_resource();
            let id = resource.id();
            items.push(Item::Create {
                id,
                size: size(data),
                opaque: !settings.alpha,
                background,
                pixels: data.pixels.take(),
            });
            data.resource = Some(resource);
            data.snapshot = None;
            id
        }
    };
    if !ops.is_empty() {
        data.snapshot = None;
        items.push(Item::Draw { id, ops });
    }
    if flush == CanvasFlush::Read && data.snapshot.is_none() {
        let (snapshot, filler) = ImageData::pending(size(data));
        items.push(Item::Snapshot { id, filler });
        data.snapshot = Some(snapshot);
    }
}

fn size(data: &CanvasData) -> Size {
    let (width, height) = data.size;
    Size::new(width, height)
}

/// Draws `ops` onto the bitmap on this thread, the pixels made the first
/// time.
fn draw_here(
    data: &mut CanvasData,
    ops: Vec<compositor_paint::PaintOp>,
    opaque: bool,
    background: [u8; 4],
    raster: &CanvasRaster,
) {
    if ops.is_empty() {
        return;
    }
    let size = size(data);
    let Some(pixels) = data.pixels_mut(background) else {
        return;
    };
    let mut list = DisplayItemList::record();
    for op in ops {
        list.push(op, None);
    }
    list.finalize();
    raster::draw_into(&list, raster.cache(), size, opaque, pixels);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use canvas::CanvasSettings;
    use compositor_paint::{PaintFlags, PaintOp};
    use computed_style::{CanvasFont, CanvasTextStyle, Direction, FontFamily, FontKerning};
    use gfx::{Color, RectF, Transform};

    use super::*;

    /// A 2×1 canvas's context's data.
    fn data() -> CanvasData {
        CanvasData::new((2, 1))
    }

    /// Records a red fill of the left pixel, as the DOM would.
    fn record_red(data: &mut CanvasData) {
        data.ops.push(PaintOp::SetMatrix {
            transform: Transform::IDENTITY,
        });
        data.ops.push(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 1.0, 1.0),
            flags: PaintFlags::fill(Color::rgba(255, 0, 0, 128)),
        });
        data.matrix = Some(Transform::IDENTITY);
    }

    /// `flush_data`, what is for the thread sent at once.
    fn flush(
        data: &mut CanvasData,
        settings: CanvasSettings,
        flush: CanvasFlush,
        raster: &mut CanvasRaster,
    ) {
        let mut items = Vec::new();
        flush_data(data, settings, flush, raster, &mut items);
        raster.send(items, false);
    }

    /// The bitmap as script reads it: a `Read` flush, and its snapshot
    /// once the thread filled it in.
    fn read_back(
        data: &mut CanvasData,
        settings: CanvasSettings,
        raster: &mut CanvasRaster,
    ) -> Vec<u8> {
        flush(data, settings, CanvasFlush::Read, raster);
        let snapshot = data.snapshot.clone().expect("a read makes a snapshot");
        assert!(snapshot.wait_ready());
        snapshot.as_bytes().to_vec()
    }

    #[test]
    fn a_flush_sends_the_recording_to_the_thread_and_forgets_it() {
        let draws = raster::draws_into_for_testing();
        let mut raster = CanvasRaster::default();
        let settings = CanvasSettings::default();
        let mut data = data();
        record_red(&mut data);
        flush(&mut data, settings, CanvasFlush::Draw, &mut raster);
        assert!(data.ops.is_empty() && data.matrix.is_none());
        assert!(data.resource.is_some() && data.pixels.is_none());
        // Nothing read: no snapshot made.
        assert!(data.snapshot.is_none());
        assert_eq!(raster.batches_sent_for_testing(), 1);
        assert_eq!(
            read_back(&mut data, settings, &mut raster),
            [128, 0, 0, 128, 0, 0, 0, 0]
        );
        // Drawn again over what is there: 128 and 127 of 255 of the 128
        // under it, 191.75, which Vello's 8-bit blending rounds down.
        record_red(&mut data);
        assert_eq!(
            read_back(&mut data, settings, &mut raster),
            [191, 0, 0, 191, 0, 0, 0, 0]
        );
        assert_eq!(raster::draws_into_for_testing(), draws, "not drawn here");
        // Read again with nothing recorded: the same snapshot, nothing sent.
        let snapshot = data.snapshot.clone();
        assert_eq!(raster.batches_sent_for_testing(), 3);
        flush(&mut data, settings, CanvasFlush::Read, &mut raster);
        assert_eq!(raster.batches_sent_for_testing(), 3);
        assert_eq!(data.snapshot, snapshot);
    }

    /// `willReadFrequently`, or read back often: drawn on the main thread,
    /// which reads it at once (Blink's `kPreferCPU`, and its canvas that
    /// disabled acceleration).
    #[test]
    fn a_canvas_read_frequently_is_drawn_here() {
        for (will_read_frequently, drawn_here) in [(true, false), (false, true)] {
            let settings = CanvasSettings {
                will_read_frequently,
                ..CanvasSettings::default()
            };
            let draws = raster::draws_into_for_testing().map(|n| n + 1);
            let mut raster = CanvasRaster::default();
            let mut data = data();
            data.drawn_here = drawn_here;
            record_red(&mut data);
            flush(&mut data, settings, CanvasFlush::Draw, &mut raster);
            assert_eq!(data.pixels, Some(vec![128, 0, 0, 128, 0, 0, 0, 0]));
            assert_eq!(raster::draws_into_for_testing(), draws, "drawn here");
            assert!(data.resource.is_none());
            assert_eq!(raster.batches_sent_for_testing(), 0);
        }
    }

    /// An opaque bitmap starts black, and the pixels it had on the main
    /// thread move with it.
    #[test]
    fn an_opaque_bitmap_starts_black() {
        let opaque = CanvasSettings {
            alpha: false,
            ..CanvasSettings::default()
        };
        let mut raster = CanvasRaster::default();
        let mut data = data();
        record_red(&mut data);
        assert_eq!(
            read_back(&mut data, opaque, &mut raster),
            [128, 0, 0, 255, 0, 0, 0, 255]
        );
        let mut moved = self::data();
        moved.pixels = Some(vec![0, 0, 0, 0, 0, 0, 9, 9]);
        record_red(&mut moved);
        assert_eq!(
            read_back(&mut moved, CanvasSettings::default(), &mut raster),
            [128, 0, 0, 128, 0, 0, 9, 9]
        );
    }

    #[test]
    fn a_bitmap_that_can_have_no_pixels_drops_the_recording() {
        let mut raster = CanvasRaster::default();
        let mut data = data();
        record_red(&mut data);
        data.size = (0, 1);
        flush(
            &mut data,
            CanvasSettings::default(),
            CanvasFlush::Read,
            &mut raster,
        );
        assert!(data.ops.is_empty() && data.resource.is_none() && data.snapshot.is_none());
        assert_eq!(raster.batches_sent_for_testing(), 0);
    }

    #[test]
    fn the_typefaces_drawn_with_stay_cached_for_the_next_flush() {
        let mut data = data();
        let style = CanvasTextStyle {
            font: CanvasFont {
                family: Arc::new([FontFamily::Named("Ahem".into())]),
                ..CanvasFont::default()
            },
            direction: Direction::Ltr,
            letter_spacing: 0.0,
            word_spacing: 0.0,
            kerning: FontKerning::Auto,
        };
        let text = layout::shape_canvas_text(&mut layout::TextContext::new(true), "X", &style);
        let run = text.runs[0].clone();
        data.ops.push(PaintOp::SetMatrix {
            transform: Transform::IDENTITY,
        });
        data.ops.push(PaintOp::DrawGlyphs {
            font: run.font,
            size: run.size,
            glyphs: run.glyphs.into(),
            positions: run.positions.into(),
            rotations: Arc::default(),
            flags: PaintFlags::fill(Color::rgba(0, 0, 0, 255)),
            embolden: false,
            skew: 0.0,
            variations: run.variations,
            bounds: RectF::new(0.0, -10.0, 20.0, 20.0),
        });
        let mut raster = CanvasRaster::default();
        flush(
            &mut data,
            CanvasSettings::default(),
            CanvasFlush::Draw,
            &mut raster,
        );
        raster.wait_for_idle_for_testing();
        assert_eq!(
            format!("{:?}", raster.cache()),
            "RasterCache { fonts: 1, images: 0 }"
        );
    }
}
