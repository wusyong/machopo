//! Text in the browser's own UI, laid out with parley apart from Blink's
//! (Chromium's `gfx::RenderText`, which Views uses): the fonts, a label on
//! one line, and glyph runs as paint ops. The address field and the tab
//! strip share it.
//!
//! Transitional, with the rest of the native UI (see `toolbar`).

use std::borrow::Cow;
use std::sync::Arc;

use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, FontData, PointF, RectF};
use parley::fontique::{Blob, Collection, CollectionOptions, SourceCache};
use parley::{FontContext, Layout, LayoutContext, PositionedLayoutItem, StyleProperty};

static LATO: &[u8] = include_bytes!("../../../resources/test_fonts/Lato-Medium.ttf");

/// The UI's fonts. `test_fonts`: Lato for every family, as the renderer's
/// test fonts, so tests measure the same everywhere. Otherwise the
/// system's fonts.
pub fn font_context(test_fonts: bool) -> FontContext {
    if !test_fonts {
        return FontContext::new();
    }
    let mut collection = Collection::new(CollectionOptions {
        shared: false,
        system_fonts: false,
    });
    let lato = collection.register_fonts(Blob::new(Arc::new(LATO)), None);
    let lato = lato.first().expect("Lato registers one family").0;
    for &generic in parley::GenericFamily::all() {
        collection.set_generic_families(generic, std::iter::once(lato));
    }
    FontContext {
        collection,
        source_cache: SourceCache::default(),
    }
}

/// The UI's font family: the system's UI font.
pub fn font_family() -> StyleProperty<'static, ()> {
    StyleProperty::FontFamily(parley::FontFamily::List(Cow::Owned(vec![
        parley::FontFamilyName::Generic(parley::GenericFamily::SystemUi),
        parley::FontFamilyName::Generic(parley::GenericFamily::SansSerif),
    ])))
}

/// `text` on one line, `size` CSS px.
pub fn label(
    fonts: &mut FontContext,
    layouts: &mut LayoutContext<()>,
    text: &str,
    size: f32,
) -> Layout<()> {
    let mut builder = layouts.ranged_builder(fonts, text, 1.0, true);
    builder.push_default(font_family());
    builder.push_default(StyleProperty::FontSize(size));
    let mut layout = builder.build(text);
    layout.break_all_lines(None);
    layout
}

/// Draws `layout`'s glyphs with its top left at `origin`, in `color`.
pub fn paint_glyphs(list: &mut DisplayItemList, layout: &Layout<()>, origin: PointF, color: Color) {
    for line in layout.lines() {
        for item in line.items() {
            let PositionedLayoutItem::GlyphRun(glyph_run) = item else {
                continue;
            };
            let run = glyph_run.run();
            let font = run.font();
            let metrics = run.font_metrics();
            let size = run.font_size();
            let synthesis = run.synthesis();
            let baseline = origin.y + glyph_run.baseline();
            let x = origin.x + glyph_run.offset();
            let bounds = RectF::new(
                x - size,
                baseline - metrics.ascent - size / 2.0,
                glyph_run.advance() + 2.0 * size,
                metrics.ascent + metrics.descent + size,
            );
            let (glyphs, positions): (Vec<u16>, Vec<PointF>) = glyph_run
                .positioned_glyphs()
                .map(|g| (glyph_id(g.id), PointF::new(origin.x + g.x, origin.y + g.y)))
                .unzip();
            list.push(
                PaintOp::DrawGlyphs {
                    font: FontData {
                        bytes: Arc::new(font.font.data.clone()),
                        index: font.font.index,
                        id: font.font.data.id(),
                    },
                    size,
                    glyphs: glyphs.into(),
                    positions: positions.into(),
                    rotations: Arc::default(),
                    flags: PaintFlags::fill(color),
                    embolden: synthesis.embolden(),
                    skew: synthesis.skew().unwrap_or(0.0),
                    // A variable font (the system's UI font) at the weight
                    // fontique picked on its axis, as it was shaped.
                    variations: if run.normalized_coords().is_empty() {
                        Arc::new([])
                    } else {
                        synthesis
                            .variation_settings()
                            .iter()
                            .map(|(tag, value)| gfx::FontVariation {
                                tag: tag.to_be_bytes(),
                                value: *value,
                            })
                            .collect()
                    },
                    bounds,
                },
                Some(bounds),
            );
        }
    }
}

/// A glyph's id as a font has it: parley's are u32s of an OpenType font's
/// 16-bit ids.
#[expect(
    clippy::cast_possible_truncation,
    reason = "an OpenType glyph id is 16 bits"
)]
fn glyph_id(id: u32) -> u16 {
    id as u16
}
