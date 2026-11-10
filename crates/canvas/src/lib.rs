//! A `<canvas>` 2D context's data apart from any DOM (the DOM's GC
//! objects keep it): its settings, its drawing state stack (Blink's
//! `CanvasRenderingContext2DState`), the paint ops recorded and not drawn
//! yet, and its bitmap ([`CanvasData`]); a pattern's and a gradient's data,
//! `TextMetrics`' numbers, and the attribute parsing of a canvas's size.
//! HTML's algorithms are each DOM's API's, drawing the ops (`raster`) the
//! renderer's.

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]
pub mod context;
pub mod export;
pub mod image;
pub mod path;
pub mod pixels;
pub mod text;

use std::sync::Arc;

use compositor_paint::{BlendMode, FilterQuality, LineCap, LineJoin, PaintOp};
use computed_style::{CanvasFont, Direction, FontKerning, TextAlign, TextBaseline, TextRendering};
use gfx::{Color, Path, Size, Transform};

/// HTML's `PredefinedColorSpace`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PredefinedColorSpace {
    #[default]
    Srgb,
    DisplayP3,
}

/// HTML's `CanvasColorType`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CanvasColorType {
    #[default]
    Unorm8,
    Float16,
}

/// What a 2D context was made with (HTML's `CanvasRenderingContext2DSettings`,
/// kept as its `getContextAttributes()` gives them back).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CanvasSettings {
    /// Whether the bitmap has an alpha channel: without one it is opaque,
    /// black where nothing is drawn.
    pub alpha: bool,
    pub desynchronized: bool,
    pub color_space: PredefinedColorSpace,
    pub color_type: CanvasColorType,
    pub will_read_frequently: bool,
}

impl Default for CanvasSettings {
    fn default() -> Self {
        Self {
            alpha: true,
            desynchronized: false,
            color_space: PredefinedColorSpace::Srgb,
            color_type: CanvasColorType::Unorm8,
            will_read_frequently: false,
        }
    }
}

/// The pixel a bitmap is all of before anything is drawn on it: opaque
/// black without alpha, transparent black with.
pub fn background(settings: CanvasSettings) -> [u8; 4] {
    if settings.alpha {
        [0, 0, 0, 0]
    } else {
        [0, 0, 0, 255]
    }
}

/// Why a canvas's recording is drawn before a frame needs it (Blink's
/// `FlushReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasFlush {
    /// Too much was recorded, or pixels are about to be written over what
    /// was: drawn wherever the bitmap is, nobody waiting for it.
    Draw,
    /// Script reads the pixels next (`getImageData`, `toDataURL`, the
    /// canvas drawn as an image): drawn, and the canvas's snapshot made,
    /// which a bitmap another thread has fills in once there.
    Read,
}

/// One level of a 2D context's state (HTML's "drawing state"), what
/// `save()` pushes and `restore()` pops (Blink's
/// `CanvasRenderingContext2DState`).
/// A fill or stroke style as a state level keeps it (Blink's
/// `CanvasStyle`): a color, or a gradient or pattern, which the context
/// holds in the level's slot for it (`DomReadScope::canvas_style`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasStyle {
    Color(Color),
    Gradient,
    Pattern,
}

/// Which of a state's styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasStyleTarget {
    Fill,
    Stroke,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CanvasDrawingState {
    /// The current transformation matrix, from the context's coordinates
    /// to the bitmap's pixels: CSS's `matrix(a, b, c, d, e, f)`, in
    /// doubles as Blink's `AffineTransform` is (a recorded op takes it as
    /// `f32`).
    pub transform: [f64; 6],
    pub global_alpha: f64,
    /// `globalCompositeOperation`'s mode.
    pub global_composite: BlendMode,
    /// The shadow: its offset and blur (a radius, twice the Gaussian's
    /// standard deviation) in the bitmap's pixels, which the transform
    /// doesn't change, and its color.
    pub shadow_offset: (f64, f64),
    pub shadow_blur: f64,
    pub shadow_color: Color,
    pub fill_style: CanvasStyle,
    pub stroke_style: CanvasStyle,
    pub line_width: f64,
    pub line_cap: LineCap,
    pub line_join: LineJoin,
    pub miter_limit: f64,
    /// `setLineDash`'s list, an odd one repeated: an even number of
    /// finite lengths, none negative; empty for a solid line.
    pub line_dash: Vec<f64>,
    pub line_dash_offset: f64,
    /// The clips `clip()` added at this level, not those of the levels
    /// under it (Blink's `ClipList`, which `save()` doesn't copy): each a
    /// path in the bitmap's pixels, filled by its fill rule. A recording
    /// starts with every level's (`CanvasData::ops`).
    pub clips: Vec<Path>,
    /// The font `font` computed (its stretch and caps are `fontStretch`
    /// and `fontVariantCaps`).
    pub font: CanvasFont,
    /// The string `font` was last set to, if it was: setting it again
    /// changes nothing (Blink's check against `UnparsedFont`).
    pub font_source: Option<String>,
    pub text_align: TextAlign,
    pub text_baseline: TextBaseline,
    /// `None` for `inherit`: the canvas's directionality.
    pub direction: Option<Direction>,
    /// `letterSpacing` and `wordSpacing` as they read back, in their units
    /// (`1.5em`): made px when text is laid out, against the font then.
    pub letter_spacing: String,
    pub word_spacing: String,
    pub font_kerning: FontKerning,
    pub text_rendering: TextRendering,
    /// How images (`drawImage`, patterns) are sampled when scaled:
    /// `imageSmoothingQuality`'s (`Low`, `Medium`, `High`), or `None` with
    /// `imageSmoothingEnabled` off. The quality is kept while smoothing is
    /// off (`image_smoothing_quality`).
    pub image_smoothing_enabled: bool,
    pub image_smoothing_quality: FilterQuality,
}

impl CanvasDrawingState {
    /// Whether draws cast a shadow: one that shows, a color not wholly
    /// transparent, blurred or moved (Blink's `ShouldDrawShadows`).
    pub fn draws_shadows(&self) -> bool {
        self.shadow_color.a != 0 && (self.shadow_blur != 0.0 || self.shadow_offset != (0.0, 0.0))
    }

    /// What images are sampled with now (Blink's `UpdateFilterQuality`).
    pub fn image_filter(&self) -> FilterQuality {
        if self.image_smoothing_enabled {
            self.image_smoothing_quality
        } else {
            FilterQuality::None
        }
    }
}

impl Default for CanvasDrawingState {
    fn default() -> Self {
        Self {
            transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            global_alpha: 1.0,
            global_composite: BlendMode::SrcOver,
            shadow_offset: (0.0, 0.0),
            shadow_blur: 0.0,
            shadow_color: Color::TRANSPARENT,
            fill_style: CanvasStyle::Color(Color::BLACK),
            stroke_style: CanvasStyle::Color(Color::BLACK),
            line_width: 1.0,
            line_cap: LineCap::Butt,
            line_join: LineJoin::Miter,
            miter_limit: 10.0,
            line_dash: Vec::new(),
            line_dash_offset: 0.0,
            clips: Vec::new(),
            font: CanvasFont::default(),
            font_source: None,
            text_align: TextAlign::Start,
            text_baseline: TextBaseline::Alphabetic,
            direction: None,
            letter_spacing: "0px".into(),
            word_spacing: "0px".into(),
            font_kerning: FontKerning::Auto,
            text_rendering: TextRendering::Auto,
            image_smoothing_enabled: true,
            image_smoothing_quality: FilterQuality::Low,
        }
    }
}

/// What a 2D context keeps besides its settings: its state stack, the ops
/// recorded and not drawn yet (Blink's `MemoryManagedPaintRecorder`), and
/// its bitmap.
#[derive(Debug)]
pub struct CanvasData {
    /// The drawing states, the current one last: never empty.
    pub states: Vec<CanvasDrawingState>,
    /// What was recorded since the bitmap was last drawn onto, in order.
    /// Each draw is recorded with the matrix it is drawn under, and a
    /// recording starts with a `Save` for each level of the state stack
    /// and that level's clips (Blink's `RestoreMatrixClipStack`), so a
    /// batch doesn't depend on the batches before it.
    pub ops: Vec<PaintOp>,
    /// The matrix the last `SetMatrix` in `ops` set, if any, and no
    /// `Restore` since: a draw under the same one records no other.
    pub matrix: Option<Transform>,
    /// The current default path (HTML's "current default path"; Blink's
    /// `CanvasPath`): in the current transform's space (each change of
    /// transform maps it into the new one), or in the bitmap's pixels
    /// while the transform can't be inverted. Not part of the drawing
    /// state: `save()` and `restore()` keep it.
    pub path: Path,
    /// The bitmap's size, in pixels: the canvas's when the bitmap was last
    /// reset.
    pub size: (u32, u32),
    /// Its pixels (RGBA8, premultiplied, rows tightly packed), once
    /// something was drawn or read; `None` is all transparent black, or
    /// opaque black for an opaque canvas. Always `None` once another
    /// thread has the bitmap (`resource`).
    pub pixels: Option<Vec<u8>>,
    /// A copy of the pixels as an image, for paint, until they change
    /// (`snapshot`). For a bitmap another thread has, the one it made
    /// last, filled in when it gets there; `None` while ops were sent
    /// after it.
    pub snapshot: Option<gfx::ImageData>,
    /// The bitmap kept and drawn on by another thread (the renderer's),
    /// once it was moved there: then `ops` are sent there, not drawn here.
    pub resource: Option<Arc<CanvasResource>>,
    /// How many times script read the pixels back (`getImageData`) from a
    /// bitmap another thread had (Blink's `read_count_`), up to the count
    /// that takes it back.
    pub readbacks: u8,
    /// Whether the bitmap is drawn on the main thread from now on, script
    /// having read it back that often (Blink's `DisableAcceleration`):
    /// then `resource` is `None`. A context's `reset()` keeps it, and the
    /// count, as Blink keeps both.
    pub drawn_here: bool,
    /// HTML's origin-clean flag: false once another origin's pixels may be
    /// in the bitmap (an image without CORS, a pattern of one, a canvas
    /// that has them), and then the pixels can't be read. A context's
    /// `reset()` keeps it, as Blink does; a new width or height makes it
    /// clean again (`dom_api`'s).
    pub origin_clean: bool,
}

impl CanvasData {
    /// A context's data for a bitmap of `size`: the default state, nothing
    /// recorded or drawn.
    pub fn new(size: (u32, u32)) -> Self {
        Self {
            states: vec![CanvasDrawingState::default()],
            ops: Vec::new(),
            matrix: None,
            path: Path::new(),
            size,
            pixels: None,
            snapshot: None,
            resource: None,
            readbacks: 0,
            drawn_here: false,
            origin_clean: true,
        }
    }

    /// The current drawing state.
    pub fn state(&self) -> &CanvasDrawingState {
        self.states.last().expect("a context has a state")
    }

    pub fn state_mut(&mut self) -> &mut CanvasDrawingState {
        self.states.last_mut().expect("a context has a state")
    }

    /// `save()`'s: a copy of the current state pushed, without its clips,
    /// which stay with the level that added them (Blink's
    /// `kDontCopyClipList`).
    pub fn push_state(&mut self) {
        let state = CanvasDrawingState {
            clips: Vec::new(),
            ..self.state().clone()
        };
        self.states.push(state);
    }

    /// `restore()`'s: the current state popped; `false` (and nothing) with
    /// only the first.
    pub fn pop_state(&mut self) -> bool {
        if self.states.len() <= 1 {
            return false;
        }
        self.states.pop();
        true
    }

    /// The current state's level: how many states are below it.
    pub fn level(&self) -> usize {
        self.states
            .len()
            .checked_sub(1)
            .expect("a context has a state")
    }

    /// HTML's "reset the rendering context to its default state", for a
    /// bitmap of `size`: the default state, nothing recorded, the bitmap
    /// cleared (here again: another thread's lets go of it). Where it is
    /// drawn stays.
    pub fn reset(&mut self, size: (u32, u32)) {
        let Self {
            origin_clean,
            readbacks,
            drawn_here,
            ..
        } = *self;
        *self = Self {
            origin_clean,
            readbacks,
            drawn_here,
            ..Self::new(size)
        };
    }

    /// Whether the bitmap can have pixels (`is_valid_bitmap_size`). A
    /// canvas outside that draws nothing, and reads back as transparent
    /// black.
    pub fn has_valid_size(&self) -> bool {
        is_valid_bitmap_size(self.size)
    }

    /// The bitmap's pixels to draw on, made the first time (each pixel
    /// `background`, premultiplied): `None` if the size isn't valid, or
    /// there isn't the memory for them (a failed allocation, which would
    /// otherwise abort). The snapshot is made again after.
    ///
    /// # Panics
    ///
    /// If another thread has the bitmap (`resource`): it is drawn there.
    pub fn pixels_mut(&mut self, background: [u8; 4]) -> Option<&mut Vec<u8>> {
        assert!(
            self.resource.is_none(),
            "another thread draws on the bitmap"
        );
        self.snapshot = None;
        if self.pixels.is_none() {
            if !self.has_valid_size() {
                return None;
            }
            let (width, height) = self.size;
            let len = gfx::Size::new(width, height).checked_byte_size()?;
            let mut pixels = Vec::new();
            pixels.try_reserve_exact(len).ok()?;
            if background == [0; 4] {
                pixels.resize(len, 0);
            } else {
                pixels.extend(std::iter::repeat_n(background, len / 4).flatten());
            }
            self.pixels = Some(pixels);
        }
        self.pixels.as_mut()
    }

    /// The bitmap as an image (Blink's `HTMLCanvasElement::Snapshot`): a
    /// copy of the pixels, the same one until they change. `None` for a
    /// bitmap with nothing to show (no pixels yet on a transparent one,
    /// whose `background` is transparent), one whose size can have none, or
    /// no memory for the copy. An opaque one with no pixels yet is all
    /// `background` (made now). For a bitmap another thread has, the
    /// snapshot it was last asked for, `None` if ops were sent after it.
    pub fn snapshot(&mut self, background: [u8; 4]) -> Option<gfx::ImageData> {
        if let Some(snapshot) = &self.snapshot {
            return Some(snapshot.clone());
        }
        if self.resource.is_some() {
            return None;
        }
        if self.pixels.is_none() && background[3] == 0 {
            return None;
        }
        let (width, height) = self.size;
        let pixels = self.pixels_mut(background)?;
        let mut copy = Vec::new();
        copy.try_reserve_exact(pixels.len()).ok()?;
        copy.extend_from_slice(pixels);
        let snapshot = gfx::ImageData::decoded(Size::new(width, height), copy);
        self.snapshot = Some(snapshot.clone());
        Some(snapshot)
    }
}

/// A canvas's bitmap kept by the thread that draws it, off the main thread
/// (Blink's `CanvasResourceProvider` of an accelerated canvas: its shared
/// image, in the GPU process). What made it (the renderer) knows the
/// bitmap by `id`, and lets it go once nothing holds this: dropping it
/// tells nobody, as it frees memory only.
#[derive(Debug, PartialEq, Eq)]
pub struct CanvasResource {
    id: u64,
}

impl CanvasResource {
    pub fn new(id: u64) -> Self {
        Self { id }
    }

    pub fn id(&self) -> u64 {
        self.id
    }
}

/// A pattern's image and how it tiles.
#[derive(Debug, Clone, PartialEq)]
pub struct CanvasPatternData {
    pub image: gfx::ImageData,
    /// The image's size in the space it is drawn in, a tile's (an
    /// `<img>`'s density-corrected natural size; a canvas's bitmap size).
    pub size: (f32, f32),
    pub repeat_x: bool,
    pub repeat_y: bool,
    /// `setTransform`'s, CSS's `matrix(a, b, c, d, e, f)`.
    pub transform: [f64; 6],
    /// Whether the image was origin-clean: a canvas whose style it is
    /// isn't, otherwise.
    pub origin_clean: bool,
}

/// Whether a canvas's bitmap of `size` can have pixels (Chromium's
/// `IsValidImageSize`): neither side 0, neither over 65535, and at most
/// 32768 × 8192 in all.
pub fn is_valid_bitmap_size((width, height): (u32, u32)) -> bool {
    const MAX_AREA: u64 = 32768 * 8192;
    const MAX_SIDE: u32 = 65535;
    width > 0
        && height > 0
        && width <= MAX_SIDE
        && height <= MAX_SIDE
        && u64::from(width)
            .checked_mul(u64::from(height))
            .is_some_and(|area| area <= MAX_AREA)
}

/// A gradient's geometry, in the space it is drawn in (the context's at
/// the time).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CanvasGradientKind {
    /// From `(x0, y0)` (offset 0) to `(x1, y1)`.
    Linear { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// From the circle around `(x0, y0)` of `r0` to the one around
    /// `(x1, y1)` of `r1`.
    Radial {
        x0: f32,
        y0: f32,
        r0: f32,
        x1: f32,
        y1: f32,
        r1: f32,
    },
    /// Around `(x, y)`, clockwise from `start_angle` radians off the x
    /// axis.
    Conic { start_angle: f32, x: f32, y: f32 },
}

/// One of a gradient's stops, in the order added.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasColorStop {
    pub offset: f64,
    pub color: Color,
}

/// A `TextMetrics`'s numbers, in CSS px: across from the alignment point
/// (left and right positive away from it), up and down from the
/// `textBaseline`'s line (ascents up, descents down; the baselines up).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TextMetricsValues {
    pub width: f64,
    pub actual_bounding_box_left: f64,
    pub actual_bounding_box_right: f64,
    pub font_bounding_box_ascent: f64,
    pub font_bounding_box_descent: f64,
    pub actual_bounding_box_ascent: f64,
    pub actual_bounding_box_descent: f64,
    pub hanging_baseline: f64,
    pub alphabetic_baseline: f64,
    pub ideographic_baseline: f64,
}

/// A canvas's bitmap size where its attribute is missing or isn't a
/// non-negative integer.
pub const DEFAULT_CANVAS_WIDTH: u32 = 300;
pub const DEFAULT_CANVAS_HEIGHT: u32 = 150;

/// HTML's "rules for parsing non-negative integers": leading ASCII
/// whitespace, an optional `+`, digits; whatever follows is ignored. A
/// value past what a `long` holds is an error, as it is for Blink.
pub fn parse_non_negative_integer(text: &str) -> Option<u32> {
    let text = text.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (negative, digits) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let end = digits
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(digits.len());
    let value: u64 = digits[..end].parse().ok()?;
    // `-0` is 0.
    if negative && value != 0 {
        return None;
    }
    u32::try_from(value)
        .ok()
        .filter(|value| *value <= i32::MAX as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapshot_is_the_same_until_the_pixels_change() {
        let mut data = CanvasData::new((1, 1));
        // Nothing drawn on a transparent bitmap: nothing to show.
        assert_eq!(data.snapshot([0; 4]), None);
        assert!(data.pixels.is_none());
        data.pixels_mut([0; 4]).unwrap()[0] = 9;
        let first = data.snapshot([0; 4]).unwrap();
        assert_eq!(first.as_bytes(), [9, 0, 0, 0]);
        assert_eq!(first.decoded, Some(Size::new(1, 1)));
        assert_eq!(data.snapshot([0; 4]), Some(first.clone()));
        // Drawn on: a new one, of the new pixels.
        data.pixels_mut([0; 4]).unwrap()[0] = 7;
        let second = data.snapshot([0; 4]).unwrap();
        assert_ne!(second, first);
        assert_eq!(second.as_bytes(), [7, 0, 0, 0]);
        // Reset: nothing again.
        data.reset((1, 1));
        assert_eq!(data.snapshot([0; 4]), None);
        // An opaque bitmap shows its black from the start.
        let mut opaque = CanvasData::new((2, 1));
        let black = opaque.snapshot([0, 0, 0, 255]).unwrap();
        assert_eq!(black.as_bytes(), [0, 0, 0, 255, 0, 0, 0, 255]);
        // A size with no pixels: nothing.
        assert_eq!(CanvasData::new((0, 1)).snapshot([0, 0, 0, 255]), None);
    }

    /// A bitmap another thread has: its snapshot is the one that thread
    /// was last asked for, none while ops were sent after it; never made
    /// here.
    #[test]
    fn a_bitmap_another_thread_has_shows_the_snapshot_asked_for() {
        let mut data = CanvasData::new((2, 1));
        data.resource = Some(Arc::new(CanvasResource::new(1)));
        assert_eq!(data.snapshot([0, 0, 0, 255]), None);
        assert!(data.pixels.is_none());
        let (pending, _filler) = gfx::ImageData::pending(Size::new(2, 1));
        data.snapshot = Some(pending.clone());
        assert_eq!(data.snapshot([0, 0, 0, 255]), Some(pending));
        // Reset: here again.
        data.reset((2, 1));
        assert_eq!(data.resource, None);
        assert!(data.snapshot([0, 0, 0, 255]).is_some());
    }

    /// A reset clears the bitmap, not where it is drawn: one taken back
    /// from another thread stays here.
    #[test]
    fn a_reset_keeps_where_the_bitmap_is_drawn() {
        let mut data = CanvasData::new((2, 1));
        data.readbacks = 2;
        data.drawn_here = true;
        data.pixels = Some(vec![1; 8]);
        data.reset((3, 1));
        assert_eq!((data.readbacks, data.drawn_here), (2, true));
        assert_eq!((data.size, data.pixels), ((3, 1), None));
    }

    #[test]
    #[should_panic(expected = "another thread draws on the bitmap")]
    fn a_bitmap_another_thread_has_is_not_drawn_here() {
        let mut data = CanvasData::new((1, 1));
        data.resource = Some(Arc::new(CanvasResource::new(1)));
        data.pixels_mut([0; 4]);
    }

    #[test]
    fn dimensions_parse_as_non_negative_integers() {
        assert_eq!(parse_non_negative_integer("  +12px"), Some(12));
        assert_eq!(parse_non_negative_integer("007"), Some(7));
        assert_eq!(parse_non_negative_integer("-1"), None);
        assert_eq!(parse_non_negative_integer("-0"), Some(0));
        assert_eq!(parse_non_negative_integer(""), None);
        assert_eq!(parse_non_negative_integer("x1"), None);
        assert_eq!(parse_non_negative_integer("2147483647"), Some(2147483647));
        assert_eq!(parse_non_negative_integer("2147483648"), None);
        assert_eq!(parse_non_negative_integer("99999999999999999999"), None);
    }
}
