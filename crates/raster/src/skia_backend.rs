//! What Skia still does: glyphs' ink bounds, with its typefaces. (Tiles
//! and canvases are `vello_backend`'s.)

use std::collections::HashMap;
use std::sync::Mutex;

#[cfg(test)]
use gfx::{FillRule, PathElement, Transform};
use gfx::{FontData, GlyphRun, RectF};
use read_fonts::FileRef;
use skia_safe as sk;

/// A path as Skia's, element for element: Skia injects the moves a
/// segment after a close (or first) leaves out, as `gfx::Path` says.
#[cfg(test)]
fn sk_path(path: &gfx::Path) -> sk::Path {
    let mut builder = sk::PathBuilder::new();
    builder.set_fill_type(match path.fill_rule() {
        FillRule::NonZero => sk::PathFillType::Winding,
        FillRule::EvenOdd => sk::PathFillType::EvenOdd,
    });
    let pt = |p: gfx::PointF| sk::Point::new(p.x, p.y);
    for element in path.elements() {
        match *element {
            PathElement::MoveTo(p) => {
                builder.move_to(pt(p));
            }
            PathElement::LineTo(p) => {
                builder.line_to(pt(p));
            }
            PathElement::QuadTo(c, p) => {
                builder.quad_to(pt(c), pt(p));
            }
            PathElement::CubicTo(c1, c2, p) => {
                builder.cubic_to(pt(c1), pt(c2), pt(p));
            }
            PathElement::ConicTo(c, p, weight) => {
                builder.conic_to(pt(c), pt(p), weight);
            }
            PathElement::Close => {
                builder.close();
            }
        }
    }
    builder.detach()
}

#[cfg(test)]
fn sk_matrix(t: &Transform) -> sk::Matrix {
    let [a, b, c, d, e, f] = t.0;
    sk::Matrix::new_all(a, c, e, b, d, f, 0.0, 0.0, 1.0)
}

/// What raster keeps between playbacks: fonts and decoded images
/// (`vello`), and Skia's typefaces, for ink bounds. One per compositor
/// (cc's `ImageDecodeCache`, which a `LayerTreeHostImpl` owns; Skia's
/// font cache is one per renderer process): renderers share none of it, as
/// they won't share a process. The compositor's raster workers share it: a
/// lock around its tables, held only to look up or add an entry, not while
/// decoding or drawing.
#[derive(Default)]
pub struct RasterCache {
    tables: Mutex<CacheTables>,
    /// What Vello draws with.
    pub(crate) vello: crate::vello_backend::VelloCache,
}

/// A variable font's instance: its font id, face index and the axes'
/// values (their bits).
type InstanceKey = (u64, u32, Box<[([u8; 4], u32)]>);

#[derive(Default)]
struct CacheTables {
    /// By font id and face index. Kept as long as the compositor.
    typefaces: HashMap<(u64, u32), sk::Typeface>,
    /// A variable font's instances, by font id, face index and the axes'
    /// values (their bits). Kept as long as the compositor.
    instances: HashMap<InstanceKey, sk::Typeface>,
}

impl std::fmt::Debug for RasterCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tables = self.tables();
        f.debug_struct("RasterCache")
            .field("typefaces", &tables.typefaces.len())
            .field("images", &self.vello.image_count())
            .finish()
    }
}

impl RasterCache {
    fn tables(&self) -> std::sync::MutexGuard<'_, CacheTables> {
        // A worker that panicked mid-insert leaves whole entries or none.
        self.tables
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn typeface(&self, font: &FontData) -> Option<sk::Typeface> {
        let key = (font.id, font.index);
        let mut tables = self.tables();
        if let Some(typeface) = tables.typefaces.get(&key) {
            return Some(typeface.clone());
        }
        let bytes = (*font.bytes).as_ref();
        let mgr = sk::FontMgr::new();
        let typeface = mgr
            .new_from_data(sk::Data::new_copy(bytes), font.index)
            .or_else(|| {
                // Skia's CoreText font manager only loads face 0 of a collection
                // (`SkTypeface_mac_ct.cpp`: "TODO: Use
                // CTFontManagerCreateFontDescriptorsFromData"); FreeType and
                // DirectWrite load any face. Hand it the face on its own.
                if font.index == 0 {
                    return None;
                }
                let face = collection_face(bytes, font.index)?;
                mgr.new_from_data(sk::Data::new_copy(&face), 0)
            })?;
        tables.typefaces.insert(key, typeface.clone());
        Some(typeface)
    }

    /// `font`'s typeface at `variations`' axis values (Blink's
    /// `FontPlatformData` makes its `SkTypeface` with them); the font's own
    /// for none.
    fn instance(&self, font: &FontData, variations: &[gfx::FontVariation]) -> Option<sk::Typeface> {
        if variations.is_empty() {
            return self.typeface(font);
        }
        let key = (
            font.id,
            font.index,
            variations
                .iter()
                .map(|v| (v.tag, v.value.to_bits()))
                .collect::<Box<[_]>>(),
        );
        if let Some(typeface) = self.tables().instances.get(&key) {
            return Some(typeface.clone());
        }
        let typeface = self.typeface(font)?;
        let coordinates: Vec<sk::font_arguments::variation_position::Coordinate> = variations
            .iter()
            .map(|v| sk::font_arguments::variation_position::Coordinate {
                axis: sk::FourByteTag::from_chars(
                    v.tag[0] as char,
                    v.tag[1] as char,
                    v.tag[2] as char,
                    v.tag[3] as char,
                ),
                value: v.value,
            })
            .collect();
        let arguments = sk::FontArguments::new().set_variation_design_position(
            sk::font_arguments::VariationPosition {
                coordinates: &coordinates,
            },
        );
        let instance = typeface
            .clone_with_arguments(&arguments)
            .unwrap_or(typeface);
        self.tables().instances.insert(key, instance.clone());
        Some(instance)
    }
}

/// The Skia font glyphs are drawn and measured with; `None` (and a
/// warning) for font data Skia can't load.
fn sk_font(
    cache: &RasterCache,
    font: &FontData,
    size: f32,
    embolden: bool,
    skew: f32,
    variations: &[gfx::FontVariation],
) -> Option<sk::Font> {
    let Some(typeface) = cache.instance(font, variations) else {
        tracing::warn!(?font, "font data Skia can't load");
        return None;
    };
    let mut sk_font = sk::Font::from_typeface(typeface, size);
    // Glyph positions come from layout: no hinting or snapping that would
    // move them.
    sk_font
        .set_edging(sk::font::Edging::AntiAlias)
        .set_hinting(sk::FontHinting::None)
        .set_subpixel(true)
        .set_linear_metrics(true)
        .set_baseline_snap(false)
        .set_embolden(embolden)
        .set_skew_x(-skew.to_radians().tan());
    Some(sk_font)
}

/// Where `runs` put ink, y down: each glyph's outline's bounds at its
/// position, made bold and slanted as it is drawn, all of them together;
/// glyphs with no ink (spaces) left out, and an empty rect at the origin
/// if none has any (Blink's `TextMetrics::MeasureRuns`). The outlines'
/// bounds are exact, as Blink's on macOS; elsewhere Blink takes Skia's
/// glyph bounds, whole pixels. A glyph with no outline (a bitmap one) has
/// those.
pub fn ink_bounds(cache: &RasterCache, runs: &[GlyphRun]) -> RectF {
    let mut ink = RectF::default();
    for run in runs {
        let Some(font) = sk_font(
            cache,
            &run.font,
            run.size,
            run.embolden,
            run.skew,
            &run.variations,
        ) else {
            continue;
        };
        for (&glyph, position) in run.glyphs.iter().zip(&run.positions) {
            let bounds = match font.get_path(glyph) {
                Some(path) => *path.bounds(),
                None => {
                    let mut bounds = [sk::Rect::default()];
                    font.get_bounds(&[glyph], &mut bounds, None);
                    bounds[0]
                }
            };
            let bounds = RectF::new(
                position.x + bounds.left,
                position.y + bounds.top,
                bounds.width(),
                bounds.height(),
            );
            if !bounds.is_empty() {
                ink = ink.union(&bounds);
            }
        }
    }
    ink
}

/// Face `index` of the font collection `ttc` as a standalone font: its table
/// directory with the offsets rewritten, followed by its tables. The tables
/// are copied unchanged, so glyph ids stay the ones layout shaped with.
fn collection_face(ttc: &[u8], index: u32) -> Option<Vec<u8>> {
    let FileRef::Collection(collection) = FileRef::new(ttc).ok()? else {
        return None;
    };
    let face = collection.get(index).ok()?;
    let dir = face.table_directory();
    let records = dir.table_records();

    let mut out = Vec::new();
    out.extend(dir.sfnt_version().to_be_bytes());
    out.extend(dir.num_tables().to_be_bytes());
    out.extend(dir.search_range().to_be_bytes());
    out.extend(dir.entry_selector().to_be_bytes());
    out.extend(dir.range_shift().to_be_bytes());
    // Tables keep their directory order and start on 4-byte boundaries.
    let mut tables = Vec::with_capacity(records.len());
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "records.len() is the directory's `num_tables`, a u16"
    )]
    let mut offset = 12 + 16 * records.len();
    for record in records {
        let start = record.offset() as usize;
        let data = ttc.get(start..start.checked_add(record.length() as usize)?)?;
        out.extend(record.tag().to_be_bytes());
        out.extend(record.checksum().to_be_bytes());
        out.extend(u32::try_from(offset).ok()?.to_be_bytes());
        out.extend(record.length().to_be_bytes());
        offset = offset.checked_add(data.len().next_multiple_of(4))?;
        tables.push(data);
    }
    for data in tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
    use gfx::{Color, PointF, Size};
    use read_fonts::{FontRef, TableProvider};

    use super::*;
    use crate::PlaybackParams;

    static AHEM: &[u8] = include_bytes!("../../../resources/test_fonts/Ahem.ttf");
    static LATO: &[u8] = include_bytes!("../../../resources/test_fonts/Lato-Medium.ttf");

    /// Packs single fonts into a TrueType collection: each font is copied
    /// whole, with its table offsets moved to where it lands in the file.
    fn ttc(fonts: &[&[u8]]) -> Vec<u8> {
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
            let records = FontRef::new(font)
                .unwrap()
                .table_directory()
                .table_records()
                .len();
            for r in 0..records {
                let at = base + 12 + 16 * r + 8;
                let offset = u32::from_be_bytes(out[at..at + 4].try_into().unwrap());
                out[at..at + 4].copy_from_slice(&(offset + base as u32).to_be_bytes());
            }
            out.resize(out.len().next_multiple_of(4), 0);
        }
        out
    }

    #[test]
    fn collection_face_is_a_standalone_font_with_the_same_tables() {
        let collection = ttc(&[LATO, AHEM]);
        for (index, original) in [LATO, AHEM].into_iter().enumerate() {
            let face = collection_face(&collection, index as u32).unwrap();
            let face = FontRef::new(&face).unwrap();
            let original = FontRef::new(original).unwrap();
            let records = original.table_directory().table_records();
            assert_eq!(face.table_directory().table_records().len(), records.len());
            for record in records {
                assert_eq!(
                    face.table_data(record.tag()).unwrap().as_bytes(),
                    original.table_data(record.tag()).unwrap().as_bytes(),
                    "face {index}, table {}",
                    record.tag()
                );
            }
            assert_eq!(
                face.maxp().unwrap().num_glyphs(),
                original.maxp().unwrap().num_glyphs()
            );
        }
        assert!(collection_face(&collection, 2).is_none());
        // Not a collection.
        assert!(collection_face(AHEM, 0).is_none());
    }

    /// A face other than the first of a collection loads on every platform,
    /// including macOS, where Skia's CoreText font manager refuses it.
    #[test]
    fn draws_a_face_after_the_first_of_a_collection() {
        let font = FontData {
            bytes: Arc::new(ttc(&[LATO, AHEM])),
            index: 1,
            id: 2,
        };
        let typeface = RasterCache::default()
            .typeface(&font)
            .expect("face 1 loads");
        assert_eq!(typeface.family_name(), "Ahem");
        let glyph = typeface.unichar_to_glyph('X' as i32);
        let mut list = DisplayItemList::new();
        // Same square as `ahem_glyph_is_a_crisp_square`: x, y in [2, 22).
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

    #[test]
    fn paths_become_skias_element_for_element() {
        use sk::PathVerb as V;
        let p = PointF::new;
        let mut path = gfx::Path::new();
        path.move_to(p(1.0, 1.0));
        path.line_to(p(9.0, 1.0));
        path.quad_to(p(9.0, 9.0), p(5.0, 9.0));
        path.cubic_to(p(4.0, 9.0), p(3.0, 8.0), p(2.0, 7.0));
        path.conic_to(p(1.0, 7.0), p(1.0, 5.0), 0.5);
        path.close();
        // No move after the close: Skia starts at the subpath's start.
        path.line_to(p(5.0, 5.0));
        let sk = sk_path(&path.with_fill_rule(FillRule::EvenOdd));
        assert_eq!(sk.fill_type(), sk::PathFillType::EvenOdd);
        assert_eq!(
            sk.verbs(),
            [
                V::Move,
                V::Line,
                V::Quad,
                V::Cubic,
                V::Conic,
                V::Close,
                V::Move,
                V::Line
            ]
        );
        let points: Vec<(f32, f32)> = sk.points().iter().map(|p| (p.x, p.y)).collect();
        assert_eq!(
            points,
            [
                (1.0, 1.0),
                (9.0, 1.0),
                (9.0, 9.0),
                (5.0, 9.0),
                (4.0, 9.0),
                (3.0, 8.0),
                (2.0, 7.0),
                (1.0, 7.0),
                (1.0, 5.0),
                (1.0, 1.0),
                (5.0, 5.0)
            ]
        );
        assert_eq!(sk.conic_weights(), [0.5]);
        let sk = sk_path(&gfx::Path::new());
        assert_eq!(sk.fill_type(), sk::PathFillType::Winding);
    }

    /// A variable font's glyph is drawn at the axis values it was shaped
    /// with: WPT's `RobotoExtremo` subset's "x" heavier at `wght` 900 than at
    /// 100, and its instance kept (the same typeface for the same values).
    #[test]
    fn a_variable_font_is_drawn_at_its_axes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../third_party/wpt/css/css-fonts/support/fonts/RobotoExtremo-VF.subset.ttf"
        );
        let bytes = std::fs::read(path).unwrap();
        let typeface = sk::FontMgr::new()
            .new_from_data(sk::Data::new_copy(&bytes), 0)
            .unwrap();
        let font = FontData {
            bytes: Arc::new(bytes),
            index: 0,
            id: 7,
        };
        let glyph = typeface.unichar_to_glyph('x' as i32);
        let cache = RasterCache::default();
        let ink = |weight: Option<f32>| {
            let variations: Arc<[gfx::FontVariation]> = weight
                .map(|value| gfx::FontVariation {
                    tag: *b"wght",
                    value,
                })
                .into_iter()
                .collect();
            crate::ink_bounds(
                &cache,
                &[GlyphRun {
                    font: font.clone(),
                    size: 100.0,
                    glyphs: vec![glyph],
                    positions: vec![PointF::new(0.0, 0.0)],
                    embolden: false,
                    skew: 0.0,
                    variations,
                }],
            )
        };
        let (thin, black, default) = (ink(Some(100.0)), ink(Some(900.0)), ink(None));
        assert!(black.width > default.width, "{black:?} {default:?}");
        assert!(default.width > thin.width, "{default:?} {thin:?}");
        assert_eq!(ink(Some(900.0)), black);
        assert_eq!(cache.tables().instances.len(), 2);
    }

    /// Ahem's "X"s are em squares, 0.8em above the baseline; its spaces
    /// have no ink. At 13px the squares' edges fall between pixels: their
    /// outlines' bounds keep them there (Skia's glyph bounds would round
    /// them out to whole pixels).
    #[test]
    fn ink_bounds_are_the_glyphs_outlines_together() {
        let typeface = sk::FontMgr::new()
            .new_from_data(sk::Data::new_copy(AHEM), 0)
            .unwrap();
        let run = |text: &str, x: f32, y: f32| GlyphRun {
            font: FontData {
                bytes: Arc::new(AHEM.to_vec()),
                index: 0,
                id: 1,
            },
            size: 13.0,
            glyphs: text
                .chars()
                .map(|c| typeface.unichar_to_glyph(c as i32))
                .collect(),
            positions: (0..text.len())
                .map(|i| PointF::new(x + i as f32 * 13.0, y))
                .collect(),
            embolden: false,
            skew: 0.0,
            variations: Arc::new([]),
        };
        // Within a thousandth of a pixel: FreeType (Skia's font backend on
        // Linux) loads the outlines in fixed point, 0.0006px off. Rounding
        // out to whole pixels would be 0.4px off or more.
        let close = |a: RectF, b: RectF| {
            let d = [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height];
            assert!(d.iter().all(|d| d.abs() < 1e-3), "{a:?} vs {b:?}");
        };
        let cache = &mut RasterCache::default();
        close(
            ink_bounds(cache, &[run("X X", 0.5, 0.0)]),
            RectF::new(0.5, -10.4, 39.0, 13.0),
        );
        // Each run's glyphs where they are.
        close(
            ink_bounds(cache, &[run("X", 0.0, 0.0), run("X", 100.0, 3.0)]),
            RectF::new(0.0, -10.4, 113.0, 16.0),
        );
        // No ink: an empty rect at the origin.
        assert_eq!(ink_bounds(cache, &[run("  ", 5.0, 5.0)]), RectF::default());
        assert_eq!(ink_bounds(cache, &[]), RectF::default());
        // Made bold and slanted as drawn: past the square.
        let bold = ink_bounds(
            cache,
            &[GlyphRun {
                embolden: true,
                ..run("X", 0.0, 0.0)
            }],
        );
        assert!(bold.x < 0.0 && bold.width > 13.0, "{bold:?}");
        let slanted = ink_bounds(
            cache,
            &[GlyphRun {
                skew: 20.0,
                ..run("X", 0.0, 0.0)
            }],
        );
        assert!(slanted.right() > 16.0, "{slanted:?}");
    }
}

#[cfg(test)]
mod path_shape_tests {
    use gfx::{Path, PointF, SizeF};

    use super::*;

    fn p(x: f32, y: f32) -> PointF {
        PointF::new(x, y)
    }

    fn sk_point(p: PointF) -> sk::Point {
        sk::Point::new(p.x, p.y)
    }

    /// `ours`, built by Skia as raster draws it, is `skia`.
    #[track_caller]
    fn assert_same(ours: &Path, skia: &sk::Path) {
        let ours = sk_path(ours);
        assert_eq!(ours.verbs(), skia.verbs(), "verbs");
        let bits = |points: &[sk::Point]| {
            points
                .iter()
                .map(|p| (p.x.to_bits(), p.y.to_bits()))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            bits(ours.points()),
            bits(skia.points()),
            "{:?} vs {:?}",
            ours.points(),
            skia.points()
        );
        let weights = |path: &sk::Path| {
            path.conic_weights()
                .iter()
                .map(|w| w.to_bits())
                .collect::<Vec<_>>()
        };
        assert_eq!(weights(&ours), weights(skia), "weights");
    }

    /// A path that starts at `start` (a move) if any, as both build it.
    fn both(start: Option<PointF>) -> (Path, sk::PathBuilder) {
        let mut ours = Path::new();
        let mut skia = sk::PathBuilder::new();
        if let Some(start) = start {
            ours.move_to(start);
            skia.move_to(sk_point(start));
        }
        (ours, skia)
    }

    #[test]
    fn arcs_of_an_oval_are_skias_conics() {
        let (tl, br) = (p(10.5, 20.25), p(110.0, 80.75));
        let oval = sk::Rect::new(tl.x, tl.y, br.x, br.y);
        for (start, sweep) in [
            (0.0, 90.0),
            (30.0, 200.0),
            (-45.0, -120.0),
            (370.0, 45.0),
            (12.0, 359.5),
            (12.0, -359.5),
            // So near a whole turn that its ends round to one direction.
            (0.0, 359.9999),
            (90.0, 180.0),
            (0.0, 0.0),
            (45.0, 0.0),
            (10.0, 0.001),
            (10.0, 360.0),
            (10.0, 400.0),
        ] {
            for force_move_to in [false, true] {
                for start_at in [None, Some(p(0.0, 0.0)), Some(p(110.0, 50.5))] {
                    let (mut ours, mut skia) = both(start_at);
                    ours.arc_to(tl, br, start, sweep, force_move_to);
                    skia.arc_to(oval, start, sweep, force_move_to);
                    assert_same(&ours, &skia.snapshot());
                }
            }
        }
        // Two arcs of one oval, the second from where the first ended:
        // no line between them.
        let (mut ours, mut skia) = both(None);
        for (start, sweep) in [(15.0, 180.0), (195.0, 180.0)] {
            ours.arc_to(tl, br, start, sweep, false);
            skia.arc_to(oval, start, sweep, false);
        }
        assert_same(&ours, &skia.snapshot());
        // An oval of no size is a point; one turned inside out, nothing.
        for (tl, br) in [(p(5.0, 5.0), p(5.0, 5.0)), (p(5.0, 5.0), p(1.0, 9.0))] {
            let (mut ours, mut skia) = both(Some(p(1.0, 1.0)));
            ours.arc_to(tl, br, 30.0, 90.0, false);
            skia.arc_to(sk::Rect::new(tl.x, tl.y, br.x, br.y), 30.0, 90.0, false);
            assert_same(&ours, &skia.snapshot());
        }
    }

    #[test]
    fn tangent_arcs_are_skias() {
        let cases: [(Option<PointF>, PointF, PointF, f32); 7] = [
            (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, 70.0), 20.0),
            (Some(p(10.0, 10.0)), p(90.0, 15.0), p(30.0, 60.0), 7.5),
            // Turning the other way.
            (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, -50.0), 20.0),
            // On one line, a radius of 0, the same point twice.
            (Some(p(10.0, 10.0)), p(50.0, 10.0), p(90.0, 10.0), 20.0),
            (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, 70.0), 0.0),
            (Some(p(10.0, 10.0)), p(10.0, 10.0), p(90.0, 70.0), 5.0),
            // From the origin.
            (None, p(90.0, 10.0), p(90.0, 70.0), 20.0),
        ];
        for (start, p1, p2, radius) in cases {
            let (mut ours, mut skia) = both(start);
            ours.tangent_arc_to(p1, p2, radius);
            skia.arc_to_tangent(sk_point(p1), sk_point(p2), radius);
            assert_same(&ours, &skia.snapshot());
        }
        // After a close: from the start of the subpath it closed.
        let (mut ours, mut skia) = both(Some(p(10.0, 10.0)));
        ours.line_to(p(50.0, 40.0));
        ours.close();
        skia.line_to((50.0, 40.0));
        skia.close();
        ours.tangent_arc_to(p(90.0, 10.0), p(90.0, 70.0), 15.0);
        skia.arc_to_tangent((90.0, 10.0), (90.0, 70.0), 15.0);
        assert_same(&ours, &skia.snapshot());
    }

    #[test]
    fn rects_and_rounded_rects_are_skias() {
        // A rect from one corner to the other, either way round.
        for (p0, p1) in [
            (p(10.0, 20.0), p(50.0, 70.0)),
            (p(50.0, 20.0), p(10.0, 70.0)),
        ] {
            let (mut ours, mut skia) = both(None);
            ours.add_rect(p0, p1);
            skia.add_rect(
                sk::Rect::new(p0.x, p0.y, p1.x, p1.y),
                sk::PathDirection::CW,
                0,
            );
            assert_same(&ours, &skia.snapshot());
        }
        let s = SizeF::new;
        let rect = gfx::RectF::new(10.25, 20.0, 80.0, 50.5);
        for radii in [
            [s(5.0, 5.0); 4],
            [s(5.0, 3.0), s(10.0, 12.0), s(0.0, 4.0), s(7.0, 7.0)],
            // Too big: scaled to fit, as CSS does.
            [s(60.0, 30.0), s(50.0, 40.0), s(10.0, 35.0), s(45.0, 20.0)],
            // Every corner square: a rect; halfway at every corner: an oval.
            [s(0.0, 9.0), s(0.0, 0.0), s(3.0, 0.0), s(0.0, 0.0)],
            [s(40.0, 25.25); 4],
            [s(100.0, 100.0); 4],
            // Too small to matter beside the other on its side.
            [s(1e-9, 5.0), s(40.0, 5.0), s(5.0, 5.0), s(5.0, 5.0)],
            [s(40.0, 5.0), s(1e-9, 5.0), s(5.0, 5.0), s(5.0, 5.0)],
        ] {
            for clockwise in [true, false] {
                let (mut ours, mut skia) = both(Some(p(1.0, 2.0)));
                ours.line_to(p(3.0, 4.0));
                skia.line_to((3.0, 4.0));
                ours.add_round_rect(rect, radii, clockwise);
                let sk_rect = sk::Rect::from_xywh(rect.x, rect.y, rect.width, rect.height);
                let sk_radii = radii.map(|r| sk::Vector::new(r.width, r.height));
                let direction = if clockwise {
                    sk::PathDirection::CW
                } else {
                    sk::PathDirection::CCW
                };
                skia.add_rrect(sk::RRect::new_rect_radii(sk_rect, &sk_radii), direction, 0);
                assert_same(&ours, &skia.snapshot());
            }
        }
    }

    #[test]
    fn a_transformed_path_is_skias() {
        let mut ours = Path::new();
        ours.line_to(p(10.0, 20.0));
        ours.quad_to(p(30.5, 1.0), p(40.0, 40.0));
        ours.close();
        ours.cubic_to(p(1.0, 2.0), p(3.0, 4.0), p(5.0, 6.0));
        ours.conic_to(p(7.0, 8.0), p(9.0, 1.0), 0.5);
        let skia = sk_path(&ours);
        let t = Transform([0.8, 0.6, -0.6, 0.8, 12.5, -3.0]);
        ours.transform(&t);
        assert_same(&ours, &skia.make_transform(&sk_matrix(&t)));
    }
}
