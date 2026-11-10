//! Where glyphs put ink, from their outlines (skrifa's), made bold and
//! slanted as playback draws them.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "kurbo's operators on f64 points, vectors and matrices (its `Mul`, `Add`               and the like, which the lint takes for integers'): none overflows"
)]

use gfx::{GlyphRun, RectF};
use skrifa::bitmap::{BitmapData, BitmapFormat, BitmapGlyph, Origin};
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, OutlinePen};
use skrifa::raw::TableProvider;
use skrifa::{FontRef, GlyphId, MetadataProvider};
use vello_cpu::kurbo::{self, Affine, BezPath, PathEl, Point, Rect, Vec2};

/// How much wider a glyph made bold is on each side, in px at `size`:
/// Skia's fake bold, an outline wider by a 24th of the size at 9 px and a
/// 32nd at 36 px, half on each side.
pub(crate) fn embolden_side(size: f32) -> f64 {
    let t = ((size - 9.0) / (36.0 - 9.0)).clamp(0.0, 1.0);
    let extra = size * (1.0 / 24.0 + t * (1.0 / 32.0 - 1.0 / 24.0));
    f64::from(extra) * 0.5
}

/// How an outline is made bold: by `side` each way, with the joins Vello's
/// glyph runs have (`glifo::FontEmbolden`'s defaults).
const EMBOLDEN_JOIN: kurbo::Join = kurbo::Join::Miter;
const EMBOLDEN_MITER_LIMIT: f64 = 4.0;
const EMBOLDEN_TOLERANCE: f64 = 0.1;

/// The slant a run's `skew` (degrees) gives its glyphs, y down.
pub(crate) fn slant(skew: f32) -> Affine {
    Affine::skew(-f64::from(skew.to_radians().tan()), 0.0)
}

/// An outline as a path, y up as the font has it.
#[derive(Default)]
struct PathPen(BezPath);

impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to((f64::from(x), f64::from(y)));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to((f64::from(x), f64::from(y)));
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        self.0.quad_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.0.curve_to(
            (f64::from(cx0), f64::from(cy0)),
            (f64::from(cx1), f64::from(cy1)),
            (f64::from(x), f64::from(y)),
        );
    }

    fn close(&mut self) {
        self.0.close_path();
    }
}

/// The bounds of `path`'s points, control points too (Skia's
/// `SkPath::getBounds`, which Blink's `TextMetrics` are made from), each
/// through `transform`; `None` for a path with no points.
fn control_bounds(path: &BezPath, transform: Affine) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    let mut add = |point: Point| {
        let point = transform * point;
        let at = Rect::from_points(point, point);
        bounds = Some(bounds.map_or(at, |bounds| bounds.union(at)));
    };
    for element in path.elements() {
        match *element {
            PathEl::MoveTo(p) | PathEl::LineTo(p) => add(p),
            PathEl::QuadTo(c, p) => {
                add(c);
                add(p);
            }
            PathEl::CurveTo(c1, c2, p) => {
                add(c1);
                add(c2);
                add(p);
            }
            PathEl::ClosePath => {}
        }
    }
    bounds
}

/// What a run's glyphs are measured with: its font, at its size and axis
/// values.
struct Face<'a> {
    font: FontRef<'a>,
    size: f32,
    coords: Vec<skrifa::instance::NormalizedCoord>,
    /// The font's units per em.
    upem: f64,
    /// Bold made up: how much wider on each side.
    embolden: Option<f64>,
    /// The glyph's own transform, y down (its slant).
    transform: Affine,
    /// Whether the font is Apple Color Emoji, whose bitmaps Apple (and
    /// Vello) place by a rule of their own.
    apple_color_emoji: bool,
}

impl<'a> Face<'a> {
    fn new(run: &'a GlyphRun) -> Option<Self> {
        let font = FontRef::from_index((*run.font.bytes).as_ref(), run.font.index).ok()?;
        let settings = run
            .variations
            .iter()
            .map(|variation| (skrifa::Tag::new(&variation.tag), variation.value));
        let coords = font.axes().location(settings).coords().to_vec();
        let apple_color_emoji = font.bitmap_strikes().format() == Some(BitmapFormat::Sbix)
            && font
                .localized_strings(skrifa::string::StringId::POSTSCRIPT_NAME)
                .any(|name| name.chars().eq("AppleColorEmoji".chars()));
        let upem = f64::from(font.head().ok()?.units_per_em());
        if upem == 0.0 {
            return None;
        }
        Some(Self {
            upem,
            font,
            size: run.size,
            coords,
            embolden: run.embolden.then(|| embolden_side(run.size)),
            transform: slant(run.skew),
            apple_color_emoji,
        })
    }

    /// Where `glyph` at the origin puts ink, y down; `None` for none.
    /// What Vello draws of a glyph, in its order: a color glyph's layers
    /// (measured by its outline, if it has one), else its bitmap, else
    /// its outline.
    fn ink(&self, glyph: u16) -> Option<Rect> {
        let id = GlyphId::new(u32::from(glyph));
        if self.font.color_glyphs().get(id).is_none()
            && let Some(bitmap) = self
                .font
                .bitmap_strikes()
                .glyph_for_size(Size::new(self.size), id)
            && let Some(rect) = self.bitmap_ink(&bitmap)
        {
            return Some(rect);
        }
        self.outline_ink(id)
    }

    /// The bounds of `glyph`'s outline, made bold and slanted.
    fn outline_ink(&self, glyph: GlyphId) -> Option<Rect> {
        let outline = self.font.outline_glyphs().get(glyph)?;
        let mut pen = PathPen::default();
        // In the font's units, scaled here: skrifa scales an outline to
        // 64ths of a pixel, which at a canvas's small sizes is a good part
        // of a glyph. Not hinted, as it is drawn: layout places the glyphs.
        let settings = DrawSettings::unhinted(Size::unscaled(), LocationRef::new(&self.coords));
        outline.draw(settings, &mut pen).ok()?;
        let mut path = pen.0;
        path.apply_affine(Affine::scale(f64::from(self.size) / self.upem));
        if let Some(side) = self.embolden {
            // As the font has it, y up: which way is out goes by the
            // path's winding, as when it is drawn.
            path = kurbo::expand_path(
                &path,
                kurbo::Diagonal2::new(side, side),
                EMBOLDEN_JOIN,
                EMBOLDEN_MITER_LIMIT,
                EMBOLDEN_TOLERANCE,
            );
        }
        control_bounds(&path, self.transform * Affine::FLIP_Y)
    }

    /// The rect `bitmap` is drawn in, as Vello's glyph runs place one
    /// (`glifo`'s `calculate_bitmap_transform`): the strike's pixels scaled
    /// to the size, moved by its bearings. Only a PNG is drawn.
    fn bitmap_ink(&self, bitmap: &BitmapGlyph<'_>) -> Option<Rect> {
        let BitmapData::Png(png) = bitmap.data else {
            return None;
        };
        let pixels = crate::image_decode::image_size(png)?;
        let (width, height) = (f64::from(pixels.width), f64::from(pixels.height));
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a font's units per em, at most 16384"
        )]
        let to_size = self.size / self.upem as f32;
        let outer = if self.apple_color_emoji {
            Vec2::new(0.0, f64::from(self.size) / 8.0)
        } else {
            Vec2::new(
                f64::from(bitmap.bearing_x * to_size),
                -f64::from(bitmap.bearing_y * to_size),
            )
        };
        let origin = match bitmap.placement_origin {
            Origin::TopLeft => Vec2::ZERO,
            Origin::BottomLeft => Vec2::new(0.0, -height),
        };
        let transform = self.transform
            * Affine::translate(outer)
            * Affine::scale_non_uniform(
                f64::from(self.size / bitmap.ppem_x),
                f64::from(self.size / bitmap.ppem_y),
            )
            * Affine::translate((
                f64::from(bitmap.inner_bearing_x),
                -f64::from(bitmap.inner_bearing_y),
            ))
            * Affine::translate(origin);
        Some(transform.transform_rect_bbox(Rect::new(0.0, 0.0, width, height)))
    }
}

/// Where `runs` put ink, y down: each glyph's outline's bounds at its
/// position, made bold and slanted as it is drawn, all of them together;
/// glyphs with no ink (spaces) left out, and an empty rect at the origin
/// if none has any (Blink's `TextMetrics::MeasureRuns`). The bounds are
/// the outline's points', not rounded, as Blink's for a canvas's metrics
/// (`SkFontGetPreciseBoundsForGlyph`: the path's bounds). A glyph drawn
/// from a bitmap has the rect it is drawn in; a font that can't be read
/// has no ink.
pub fn ink_bounds(runs: &[GlyphRun]) -> RectF {
    let mut ink: Option<Rect> = None;
    for run in runs {
        let Some(face) = Face::new(run) else {
            tracing::warn!(font = ?run.font, "font data that can't be read");
            continue;
        };
        for (&glyph, position) in run.glyphs.iter().zip(&run.positions) {
            let Some(bounds) = face.ink(glyph) else {
                continue;
            };
            if bounds.width() <= 0.0 || bounds.height() <= 0.0 {
                continue;
            }
            let bounds = bounds + Vec2::new(f64::from(position.x), f64::from(position.y));
            ink = Some(ink.map_or(bounds, |ink| ink.union(bounds)));
        }
    }
    #[expect(
        clippy::cast_possible_truncation,
        reason = "glyph bounds in px, far inside an f32's range"
    )]
    ink.map_or_else(RectF::default, |ink| {
        RectF::new(
            ink.x0 as f32,
            ink.y0 as f32,
            ink.width() as f32,
            ink.height() as f32,
        )
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gfx::{FontData, PointF};

    use super::*;

    static AHEM: &[u8] = include_bytes!("../../../resources/test_fonts/Ahem.ttf");
    static LATO: &[u8] = include_bytes!("../../../resources/test_fonts/Lato-Medium.ttf");

    fn glyph(font: &[u8], c: char) -> u16 {
        let id = FontRef::new(font).unwrap().charmap().map(c).unwrap();
        u16::try_from(id.to_u32()).unwrap()
    }

    fn ahem(text: &str, x: f32, y: f32) -> GlyphRun {
        GlyphRun {
            font: FontData {
                bytes: Arc::new(AHEM.to_vec()),
                index: 0,
                id: 1,
            },
            size: 13.0,
            glyphs: text.chars().map(|c| glyph(AHEM, c)).collect(),
            positions: (0..text.len())
                .map(|i| PointF::new(x + i as f32 * 13.0, y))
                .collect(),
            embolden: false,
            skew: 0.0,
            variations: Arc::new([]),
        }
    }

    #[track_caller]
    fn close(a: RectF, b: RectF) {
        let d = [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height];
        assert!(d.iter().all(|d| d.abs() < 1e-4), "{a:?} vs {b:?}");
    }

    /// Ahem's "X"s are em squares, 0.8em above the baseline; its spaces
    /// have no ink. At 13px the squares' edges fall between pixels, and
    /// the bounds keep them there: not rounded out to whole pixels, nor
    /// to the 64ths of one a scaled outline has.
    #[test]
    fn ink_bounds_are_the_glyphs_outlines_together() {
        close(
            ink_bounds(&[ahem("X X", 0.5, 0.0)]),
            RectF::new(0.5, -10.4, 39.0, 13.0),
        );
        // Each run's glyphs where they are.
        close(
            ink_bounds(&[ahem("X", 0.0, 0.0), ahem("X", 100.0, 3.0)]),
            RectF::new(0.0, -10.4, 113.0, 16.0),
        );
        // No ink: an empty rect at the origin.
        assert_eq!(ink_bounds(&[ahem("  ", 5.0, 5.0)]), RectF::default());
        assert_eq!(ink_bounds(&[]), RectF::default());
        // At a size a canvas may have: 64ths of a pixel would be 1.5625
        // and 1.203125.
        close(
            ink_bounds(&[GlyphRun {
                size: 1.5,
                ..ahem("X", 0.0, 0.0)
            }]),
            RectF::new(0.0, -1.2, 1.5, 1.5),
        );
    }

    /// A curve's control points are in the bounds, as in Skia's of a
    /// path: past where the curve itself goes.
    #[test]
    fn bounds_are_the_points_control_points_too() {
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.quad_to((5.0, 10.0), (10.0, 0.0));
        path.curve_to((12.0, -3.0), (14.0, 8.0), (10.0, 2.0));
        path.close_path();
        assert_eq!(
            control_bounds(&path, Affine::IDENTITY),
            Some(Rect::new(0.0, -3.0, 14.0, 10.0))
        );
        assert_eq!(
            control_bounds(&path, Affine::FLIP_Y),
            Some(Rect::new(0.0, -10.0, 14.0, 3.0))
        );
        assert_eq!(control_bounds(&BezPath::new(), Affine::IDENTITY), None);
    }

    /// Made bold, the square is wider by the same on every side, as much
    /// as it is drawn wider; slanted, its top goes right and its bottom
    /// left, about the baseline.
    #[test]
    fn ink_bounds_are_made_bold_and_slanted() {
        let side = embolden_side(13.0) as f32;
        assert!((side - 0.25).abs() < 0.03, "{side}");
        close(
            ink_bounds(&[GlyphRun {
                embolden: true,
                ..ahem("X", 0.0, 0.0)
            }]),
            RectF::new(-side, -10.4 - side, 13.0 + 2.0 * side, 13.0 + 2.0 * side),
        );
        let tan = 20f32.to_radians().tan();
        close(
            ink_bounds(&[GlyphRun {
                skew: 20.0,
                ..ahem("X", 0.0, 0.0)
            }]),
            RectF::new(-2.6 * tan, -10.4, 13.0 + 13.0 * tan, 13.0),
        );
        // Both: made bold, then slanted.
        close(
            ink_bounds(&[GlyphRun {
                embolden: true,
                skew: 20.0,
                ..ahem("X", 0.0, 0.0)
            }]),
            RectF::new(
                -side - (2.6 + side) * tan,
                -10.4 - side,
                13.0 + 2.0 * side + (13.0 + 2.0 * side) * tan,
                13.0 + 2.0 * side,
            ),
        );
    }

    /// How much wider bold is goes by the size: a 24th of it up to 9 px,
    /// a 32nd from 36 px, half on each side.
    #[test]
    fn bold_is_wider_by_the_size() {
        let close = |a: f64, b: f64| assert!((a - b).abs() < 1e-6, "{a} {b}");
        close(embolden_side(6.0), 6.0 / 48.0);
        close(embolden_side(9.0), 9.0 / 48.0);
        close(embolden_side(36.0), 36.0 / 64.0);
        close(embolden_side(72.0), 72.0 / 64.0);
        close(embolden_side(22.5), 22.5 * (1.0 / 24.0 + 1.0 / 32.0) / 4.0);
    }

    /// A variable font's glyph is measured at the axis values it was
    /// shaped with: WPT's `RobotoExtremo` subset's "x" is wider at `wght`
    /// 900 than at its default, and wider there than at 100.
    #[test]
    fn a_variable_font_is_measured_at_its_axes() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../third_party/wpt/css/css-fonts/support/fonts/RobotoExtremo-VF.subset.ttf"
        );
        let bytes = std::fs::read(path).unwrap();
        let x = glyph(&bytes, 'x');
        let font = FontData {
            bytes: Arc::new(bytes),
            index: 0,
            id: 7,
        };
        let ink = |weight: Option<f32>| {
            ink_bounds(&[GlyphRun {
                font: font.clone(),
                size: 100.0,
                glyphs: vec![x],
                positions: vec![PointF::new(0.0, 0.0)],
                embolden: false,
                skew: 0.0,
                variations: weight
                    .map(|value| gfx::FontVariation {
                        tag: *b"wght",
                        value,
                    })
                    .into_iter()
                    .collect(),
            }])
        };
        let (thin, black, default) = (ink(Some(100.0)), ink(Some(900.0)), ink(None));
        assert!(black.width > default.width, "{black:?} {default:?}");
        assert!(default.width > thin.width, "{default:?} {thin:?}");
    }

    /// A face of a collection is measured as that face; a face it hasn't,
    /// and bytes that are no font, have no ink.
    #[test]
    fn a_collections_face_is_measured_and_no_font_has_no_ink() {
        let collection = Arc::new(crate::playback_tests::ttc(&[LATO, AHEM]));
        let run = |index: u32| GlyphRun {
            font: FontData {
                bytes: collection.clone(),
                index,
                id: 2,
            },
            ..ahem("X", 0.0, 0.0)
        };
        close(ink_bounds(&[run(1)]), RectF::new(0.0, -10.4, 13.0, 13.0));
        // Lato's glyph of that id is no em square.
        assert_ne!(ink_bounds(&[run(0)]), ink_bounds(&[run(1)]));
        assert_eq!(ink_bounds(&[run(2)]), RectF::default());
        let none = GlyphRun {
            font: FontData {
                bytes: Arc::new(b"not a font".to_vec()),
                index: 0,
                id: 3,
            },
            ..ahem("X", 0.0, 0.0)
        };
        assert_eq!(ink_bounds(&[none]), RectF::default());
    }

    /// A glyph that is a bitmap (a color emoji of WPT's Noto Color Emoji
    /// subset) has the rect the bitmap is drawn in: the strike's pixels
    /// scaled to the size, about as large as the size, most of it above
    /// the baseline.
    #[test]
    fn a_bitmap_glyph_has_the_rect_it_is_drawn_in() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../third_party/wpt/fonts/noto/color-emoji/NotoColorEmoji-subset.ttf"
        );
        let bytes = std::fs::read(path).unwrap();
        let font = FontRef::new(&bytes).unwrap();
        assert!(font.outline_glyphs().get(GlyphId::new(2)).is_none());
        let strike = font
            .bitmap_strikes()
            .glyph_for_size(Size::new(40.0), GlyphId::new(2))
            .unwrap();
        let run = |size: f32, skew: f32| GlyphRun {
            font: FontData {
                bytes: Arc::new(bytes.clone()),
                index: 0,
                id: 9,
            },
            size,
            glyphs: vec![2],
            positions: vec![PointF::new(10.0, 0.0)],
            embolden: true,
            skew,
            variations: Arc::new([]),
        };
        let ink = ink_bounds(&[run(40.0, 0.0)]);
        let BitmapData::Png(png) = strike.data else {
            panic!("a PNG");
        };
        let pixels = crate::image_decode::image_size(png).unwrap();
        let scale = 40.0 / strike.ppem_x;
        close(
            RectF::new(ink.x, ink.y, ink.width, ink.height),
            RectF::new(
                10.0 + strike.bearing_x * 40.0 / 2048.0 + strike.inner_bearing_x * scale,
                -strike.bearing_y * 40.0 / 2048.0 - strike.inner_bearing_y * scale,
                pixels.width as f32 * scale,
                pixels.height as f32 * scale,
            ),
        );
        assert!((40.0..56.0).contains(&ink.width), "{ink:?}");
        assert!(ink.y < -30.0 && ink.bottom() > 0.0, "{ink:?}");
        // Twice the size, twice the rect; slanted, wider.
        let twice = ink_bounds(&[run(80.0, 0.0)]);
        assert!((twice.width - 2.0 * ink.width).abs() < 1e-3, "{twice:?}");
        assert!(ink_bounds(&[run(40.0, 20.0)]).width > ink.width + 10.0);
    }
}
