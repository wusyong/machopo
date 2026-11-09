//! Recorded paint operations (`//cc/paint`).
//!
//! Blink's paint phase records [`PaintOp`]s into a [`DisplayItemList`]; the
//! compositor hands that list to raster, which plays it back onto a Skia
//! canvas. The ops intentionally mirror `cc::PaintOpBuffer` so the raster
//! side stays a thin 1:1 translation to Skia calls.
//!
//! This crate is separate from `compositor` for the same reason `cc/paint` is
//! its own component in Chromium: both `compositor` and `raster` depend on it.

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

mod rtree;

use std::sync::Arc;

use gfx::{Color, FontData, ImageData, Path, PointF, RRectF, RectF, Size, SizeF, Transform};
// A stroke's geometry, which hit testing shares.
pub use gfx::{LineCap, LineJoin};

/// Fill vs. stroke (`cc::PaintFlags::Style`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PaintStyle {
    Fill,
    Stroke { width: f32 },
}

/// `cc::PaintFlags` (the subset we use).
#[derive(Debug, Clone, PartialEq)]
pub struct PaintFlags {
    pub color: Color,
    pub style: PaintStyle,
    pub anti_alias: bool,
    /// Fills with this instead of `color` (`cc::PaintFlags::setShader`).
    pub shader: Option<Arc<PaintShader>>,
    /// The standard deviation of a Gaussian blur of what is drawn, in the
    /// current space; 0 for none (`cc::PaintFlags::setMaskFilter` with
    /// `SkMaskFilter::MakeBlur`).
    pub blur: f32,
    /// A stroke's dashes (`cc::PaintFlags::setPathEffect` with
    /// `SkDashPathEffect`), if it has any.
    pub dash: Option<Dash>,
    /// A stroke's ends (`cc::PaintFlags::setStrokeCap`): of its open
    /// subpaths and of each dash.
    pub cap: LineCap,
    /// Where a stroke's segments meet (`cc::PaintFlags::setStrokeJoin`).
    pub join: LineJoin,
    /// How far a miter join may reach, in stroke widths, before it is
    /// beveled instead (`cc::PaintFlags::setStrokeMiter`; Skia's default
    /// is 4, as SVG's `stroke-miterlimit`'s).
    pub miter_limit: f32,
    /// How what is drawn combines with what is under it
    /// (`cc::PaintFlags::setBlendMode`): a canvas's
    /// `globalCompositeOperation`.
    pub blend_mode: BlendMode,
}

/// How a draw combines with the pixels under it (`SkBlendMode`): the
/// modes a canvas's `globalCompositeOperation` names. The first twelve
/// are Porter-Duff's, the rest blend the colors (CSS's `mix-blend-mode`
/// values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    Clear,
    /// Replaces what is under it (a canvas's `copy`).
    Src,
    #[default]
    SrcOver,
    DstOver,
    SrcIn,
    DstIn,
    SrcOut,
    DstOut,
    SrcATop,
    DstATop,
    Xor,
    /// Adds (a canvas's `lighter`).
    Plus,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// An effect on what a layer drew, applied as the layer is composited
/// (`cc::PaintFilter`).
#[derive(Debug, Clone, PartialEq)]
pub enum ImageFilter {
    /// A shadow of what was drawn: its alpha blurred with `sigma` (both
    /// ways), moved by `offset` and filled with `color`; under what was
    /// drawn, or alone if `shadow_only` (`DropShadowPaintFilter`'s
    /// `kDrawShadowAndForeground` and `kDrawShadowOnly`).
    DropShadow {
        offset: (f32, f32),
        sigma: f32,
        color: Color,
        shadow_only: bool,
    },
    /// What was drawn blurred with `sigma` both ways (`BlurPaintFilter`).
    Blur { sigma: f32 },
    /// Each color (not premultiplied, 0 to 1) through a 4×5 matrix, row by
    /// row, the last column added (`ColorFilterPaintFilter` with a matrix
    /// color filter).
    ColorMatrix([f32; 20]),
    /// These one after the other, each on what the one before made
    /// (`ComposePaintFilter`).
    Chain(Arc<[ImageFilter]>),
}

/// How a layer is composited when it is restored (the `cc::PaintFlags` of
/// a `cc::SaveLayerOp`).
#[derive(Debug, Clone, PartialEq)]
pub struct LayerFlags {
    pub alpha: f32,
    pub blend_mode: BlendMode,
    pub image_filter: Option<ImageFilter>,
}

impl Default for LayerFlags {
    fn default() -> Self {
        Self {
            alpha: 1.0,
            blend_mode: BlendMode::SrcOver,
            image_filter: None,
        }
    }
}

/// A dashed stroke (`SkDashPathEffect`): its intervals drawn and not in
/// turn, over and over along each subpath, starting `phase` into them.
#[derive(Debug, Clone, PartialEq)]
pub struct Dash {
    intervals: Arc<[f32]>,
    phase: f32,
}

impl Dash {
    /// Dashes of `intervals`, an odd number of them repeated to make an
    /// even one (SVG's `stroke-dasharray` and canvas's `setLineDash` both
    /// do); `None` for a solid line: no intervals, a negative or infinite
    /// one, or all 0 (Skia's `SkDashPathEffect::Make` draws those solid).
    pub fn new(intervals: &[f32], phase: f32) -> Option<Self> {
        if intervals.iter().any(|i| !i.is_finite() || *i < 0.0)
            || intervals.iter().sum::<f32>() <= 0.0
            || !phase.is_finite()
        {
            return None;
        }
        let intervals: Arc<[f32]> = if intervals.len() % 2 == 1 {
            intervals.iter().chain(intervals).copied().collect()
        } else {
            intervals.into()
        };
        Some(Self { intervals, phase })
    }

    /// An even number of them, the drawn ones first.
    pub fn intervals(&self) -> &[f32] {
        &self.intervals
    }

    pub fn phase(&self) -> f32 {
        self.phase
    }
}

/// `cc::PaintShader`: what an op fills with in place of its color. Raster
/// makes it a Skia shader, so a display list holds the description, not
/// pixels, and stays sharp at any scale.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintShader {
    /// An image or a gradient drawn into a tile, the tile repeated: what
    /// a background layer fills its area with.
    Tiled(TiledShader),
    /// A gradient over the whole plane (cc's gradient shaders): SVG's
    /// paint servers, canvas's gradients.
    Gradient(GradientShader),
    /// Nothing at all (cc's `PaintShader::MakeEmpty`): a canvas's gradient
    /// with no length, or no stops.
    Empty,
}

impl From<TiledShader> for PaintShader {
    fn from(shader: TiledShader) -> Self {
        Self::Tiled(shader)
    }
}

impl From<GradientShader> for PaintShader {
    fn from(shader: GradientShader) -> Self {
        Self::Gradient(shader)
    }
}

/// An image or a gradient drawn into a tile and the tile repeated (cc's
/// image shader, or its paint record shader for a tile with gaps).
#[derive(Debug, Clone, PartialEq)]
pub struct TiledShader {
    pub image: ShaderImage,
    /// Where the first tile is, in the current space: the image is drawn
    /// scaled into it.
    pub tile: RectF,
    /// The gap after each tile, on each axis (`background-repeat: space`).
    pub spacing: SizeF,
    /// Whether the tile repeats on each axis, both ways.
    pub repeat_x: bool,
    pub repeat_y: bool,
    /// From the tiles' space to the current one (cc's local matrix: a
    /// canvas pattern's `setTransform`).
    pub transform: Transform,
    /// How the image is sampled.
    pub filter: FilterQuality,
    /// On an axis that doesn't repeat, whether to draw nothing past the
    /// tile (the op covers more than it: a rounded background), rather
    /// than extend its edge (the op's rect stops at the tile, and its edge
    /// pixels stay whole, as Chrome draws an image that doesn't repeat).
    pub decal: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShaderImage {
    /// An encoded image, decoded at raster time as `DrawImage`'s is.
    Image(ImageData),
    /// A gradient in the tile's space (its origin at the tile's).
    Gradient(GradientShader),
    /// A recorded list (an SVG image's paint), its space `size` big scaled
    /// to the tile: replayed as vectors.
    Record {
        record: std::sync::Arc<DisplayItemList>,
        size: SizeF,
    },
}

/// A gradient (cc's `MakeLinearGradient`, `MakeTwoPointConicalGradient`),
/// its stops ready for Skia: at least two, their offsets from 0 to 1 and
/// in order along the line (or between the circles) from start to end.
#[derive(Debug, Clone, PartialEq)]
pub struct GradientShader {
    pub kind: GradientShaderKind,
    pub stops: Vec<GradientStop>,
    /// What is past the ends (`SkTileMode`).
    pub tile_mode: TileMode,
    /// From the gradient's space to the current one (cc's local matrix:
    /// SVG's `gradientTransform`, a CSS ellipse's scale).
    pub transform: Transform,
    pub interpolation: ColorInterpolation,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum GradientShaderKind {
    Linear {
        start: PointF,
        end: PointF,
    },
    /// Circles from the one around `start` of `start_radius` (offset 0)
    /// to the one around `end` of `end_radius` (offset 1): a radial
    /// gradient when the centers are one, SVG's focal point and canvas's
    /// two circles when not.
    TwoPointConical {
        start: PointF,
        start_radius: f32,
        end: PointF,
        end_radius: f32,
    },
    /// Around `center`, from `start_angle` (offset 0) to `end_angle`
    /// (offset 1), in degrees clockwise from the x axis (cc's
    /// `MakeSweepGradient`): canvas's conic gradients.
    Sweep {
        center: PointF,
        start_angle: f32,
        end_angle: f32,
    },
}

/// What a gradient is past its ends (`SkTileMode`, but `kDecal`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileMode {
    /// The end colors extend (SVG's `pad`).
    Clamp,
    /// The stops repeat (`repeating-*`, SVG's `repeat`).
    Repeat,
    /// The stops repeat, every other time backwards (SVG's `reflect`).
    Mirror,
}

/// How a gradient's colors mix between stops (Blink's
/// `Gradient::ColorInterpolation`): premultiplied as css-images says for
/// CSS's gradients, unpremultiplied for SVG's and canvas's, as Chrome
/// does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorInterpolation {
    Premultiplied,
    Unpremultiplied,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GradientStop {
    pub offset: f32,
    pub color: Color,
}

impl PaintFlags {
    pub fn fill(color: Color) -> Self {
        Self {
            color,
            style: PaintStyle::Fill,
            anti_alias: true,
            shader: None,
            blur: 0.0,
            dash: None,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
            blend_mode: BlendMode::SrcOver,
        }
    }

    /// Fills with `shader`.
    pub fn shader(shader: impl Into<PaintShader>) -> Self {
        Self {
            shader: Some(Arc::new(shader.into())),
            ..Self::fill(Color::BLACK)
        }
    }

    pub fn stroke(color: Color, width: f32) -> Self {
        Self {
            style: PaintStyle::Stroke { width },
            ..Self::fill(color)
        }
    }
}

/// One recorded op. Coordinates are in the layer's space, CSS pixels.
#[derive(Debug, Clone, PartialEq)]
pub enum PaintOp {
    Save,
    Restore,
    Translate {
        dx: f32,
        dy: f32,
    },
    ClipRect {
        rect: RectF,
        anti_alias: bool,
    },
    /// A clip with rounded corners (`cc::ClipRRectOp`): `overflow` and a
    /// replaced element's content under `border-radius`.
    ClipRRect {
        rrect: RRectF,
        anti_alias: bool,
    },
    /// A clip to a path, filled by its fill rule (`cc::ClipPathOp`, with
    /// `SkClipOp::kIntersect`): canvas's `clip()`, SVG's `clipPath`.
    ClipPath {
        path: Path,
        anti_alias: bool,
    },
    /// A clip to the union of paths, each filled by its fill rule:
    /// `clip-path`'s `<clipPath>` of several shapes (Blink's
    /// `LayoutSVGResourceClipper::AsPath`, which joins them with
    /// `SkPathOps`). None clips everything away.
    ClipPaths {
        paths: Vec<Path>,
        anti_alias: bool,
    },
    /// Multiplies the current matrix by `transform` (applied first).
    Concat {
        transform: Transform,
    },
    /// Replaces the current matrix with `transform`, relative to the
    /// matrix playback started with (as `cc::SetMatrixOp` is), so a
    /// display list stays position-independent.
    SetMatrix {
        transform: Transform,
    },
    /// Starts a group composited with `alpha` at the matching `Restore`.
    /// `bounds` limits the group, in the current space; `None` is
    /// unbounded.
    SaveLayerAlpha {
        bounds: Option<RectF>,
        alpha: f32,
    },
    /// Starts a group composited by `flags` at the matching `Restore`
    /// (`cc::SaveLayerOp`): with its alpha, blend mode and image filter,
    /// the filter in the space current here. `bounds` as
    /// `SaveLayerAlpha`'s.
    SaveLayer {
        bounds: Option<RectF>,
        flags: LayerFlags,
    },
    /// Fills the clip with `color`, combined by `blend_mode`
    /// (`cc::DrawColorOp`; `Src` is `SkCanvas::clear`).
    DrawColor {
        color: Color,
        blend_mode: BlendMode,
    },
    DrawRect {
        rect: RectF,
        flags: PaintFlags,
    },
    /// Makes `rect` transparent black: a canvas's `clearRect` (Blink
    /// draws the rect with `SkBlendMode::kClear` flags; this needs none).
    ClearRect {
        rect: RectF,
    },
    DrawRRect {
        rrect: RRectF,
        flags: PaintFlags,
    },
    /// The area between two rounded rects, `inner` inside `outer`
    /// (`cc::DrawDRRectOp`): a rounded border. `clip`, if any, is a
    /// polygon it is drawn only inside: one side's part of a border whose
    /// sides differ (Blink's `BoxBorderPainter` clips each side to the
    /// trapezoid between the edges and the corners' diagonals).
    DrawDRRect {
        outer: RRectF,
        inner: RRectF,
        clip: Option<Arc<[PointF]>>,
        flags: PaintFlags,
    },
    DrawLine {
        p0: PointF,
        p1: PointF,
        flags: PaintFlags,
    },
    /// `op` drawn only inside `polygon` (a save, a clip and a restore around
    /// it, in cc): one side of a border whose sides differ, cut to its
    /// trapezoid between the edges and the corners' diagonals.
    DrawClipped {
        /// Shared by an item's copies, as `glyphs` is.
        polygon: Arc<[PointF]>,
        op: Box<PaintOp>,
    },
    /// A box shadow (Blink's `BoxPainterBase::PaintNormalBoxShadow` and
    /// `PaintInsetBoxShadow`, which cc records as a save, a clip, the
    /// blurred shape and a restore): `shape`, less `hole` if any, filled
    /// with `flags` (its color and blur), and drawn only outside `clip`
    /// (an outer shadow, which doesn't show under its box) or, if
    /// `inside`, only inside it (an inset one, in the padding box).
    DrawShadow {
        shape: RRectF,
        hole: Option<RRectF>,
        clip: RRectF,
        inside: bool,
        flags: PaintFlags,
    },
    /// A path filled or stroked (`cc::DrawPathOp`). Borders are polygons
    /// filled as one path, so polygons that share an edge leave no
    /// anti-aliasing seam between them (`BoxBorderPainter`).
    DrawPath {
        path: Path,
        flags: PaintFlags,
    },
    /// Glyphs of one font at the given positions (glyph origins on the
    /// baseline), filled or stroked with `flags` (`cc::DrawTextBlobOp`).
    /// `embolden` and `skew` synthesize bold and oblique when the font has
    /// no such face; `skew` is an angle in degrees.
    DrawGlyphs {
        font: FontData,
        size: f32,
        /// Shared by the copies of an item paint takes from the last
        /// paint, as an `SkTextBlob` is.
        glyphs: Arc<[u16]>,
        positions: Arc<[PointF]>,
        /// Each glyph's rotation about its origin, in degrees clockwise
        /// (a text blob's RSXforms: SVG's `rotate`); empty for none.
        rotations: Arc<[f32]>,
        flags: PaintFlags,
        embolden: bool,
        skew: f32,
        /// A variable font's axes as the glyphs were shaped with them
        /// (`SkFontArguments`' variation design position); empty for
        /// another font.
        variations: Arc<[gfx::FontVariation]>,
        /// Conservative bounds: the glyphs' ink (and a stroke's) may
        /// extend past their advances.
        bounds: RectF,
    },
    /// An image's `src` (its whole, for `None`), scaled into `dst`, sampled
    /// as `filter` says, never past `src`'s edges (cc's `DrawImageRectOp`
    /// with the strict constraint); drawn with `flags`' alpha if given (a
    /// canvas's `globalAlpha`). An encoded one is decoded at raster time.
    DrawImage {
        image: ImageData,
        src: Option<RectF>,
        dst: RectF,
        filter: FilterQuality,
        flags: Option<PaintFlags>,
    },
    /// `pixels` (premultiplied RGBA8, `size`, rows tightly packed) written
    /// over the canvas's at `(x, y)`, in its pixels, as they are: not
    /// through the matrix, the clip or blending (Skia's `writePixels`). Only
    /// in a canvas's bitmap's list (`putImageData` on a bitmap another
    /// thread draws, which gets it in order with the draws; Blink flushes
    /// and writes the pixels to the bitmap's resource), never in a list
    /// paint makes or bounds.
    WritePixels {
        x: u32,
        y: u32,
        size: Size,
        pixels: Arc<[u8]>,
    },
    /// A recorded list drawn into `dst`, its space `size` big scaled to
    /// fit and cut to it (cc's `DrawImageRectOp` of a `PaintImage` backed by
    /// a paint record, an SVG image's): the list replays as vectors, at
    /// whatever scale it is drawn.
    DrawRecord {
        record: std::sync::Arc<DisplayItemList>,
        size: SizeF,
        dst: RectF,
    },
}

/// How an image is sampled when it is scaled (`cc::PaintFlags::FilterQuality`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterQuality {
    /// The nearest pixel: a canvas's with `imageSmoothingEnabled` off.
    None,
    /// Bilinear, without mipmaps: a canvas's (Blink's
    /// `CanvasDefaultInterpolationQuality`, `kInterpolationLow`).
    Low,
    /// Bilinear, with mipmaps when scaling down: an `<img>`'s and a
    /// background's (CSS's `image-rendering: auto`).
    Medium,
    /// Cubic when scaling up, as `Medium` when scaling down: a canvas's
    /// `imageSmoothingQuality = "high"` (cc's `kHigh`).
    High,
}

/// What a display list is for (`cc::DisplayItemList::UsageHint`): a
/// layer's, rastered a tile at a time, or one only ever played whole.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UsageHint {
    /// A layer's (`kTopLevelDisplayItemList`): finalizing it builds the
    /// spatial index each tile is looked up in.
    #[default]
    TopLevel,
    /// Played whole, or drawn into another list (`kToBeReleasedAsPaintOpBuffer`):
    /// a canvas's recording, an SVG image's, a view's painting.
    Record,
}

/// Where an op may draw when that isn't known: everywhere, in effect.
const EVERYWHERE: RectF = RectF::new(-1e30, -1e30, 2e30, 2e30);

/// `cc::DisplayItemList`: an immutable-after-finalize list of ops plus the
/// union of the rects they touch, and each op's visual rect: a drawing
/// op's as given (or everywhere, if none was), and once finalized, a state
/// op's that of what it applies to. A layer's list then has an R-tree of
/// them, which tells raster the ops that draw in a tile.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DisplayItemList {
    ops: Vec<PaintOp>,
    /// One for each op, in the layer's space (CSS px).
    visual_rects: Vec<RectF>,
    bounds: RectF,
    usage: UsageHint,
    rtree: Option<rtree::RTree>,
    finalized: bool,
}

impl DisplayItemList {
    /// A layer's list.
    pub fn new() -> Self {
        Self::default()
    }

    /// A list only ever played whole, or drawn into another list: no index.
    pub fn record() -> Self {
        Self {
            usage: UsageHint::Record,
            ..Self::default()
        }
    }

    /// Appends an op. `visual_rect` is the op's conservative bounds (used for
    /// invalidation, to size the layer and to find the ops a tile needs);
    /// `None` for state ops. A drawing op without one may draw anywhere.
    pub fn push(&mut self, op: PaintOp, visual_rect: Option<RectF>) {
        assert!(!self.finalized, "DisplayItemList is finalized");
        if let Some(r) = visual_rect {
            self.push_bounds(r);
        }
        self.visual_rects.push(match visual_rect {
            Some(r) => r,
            None if is_state_op(&op) => RectF::default(),
            None => EVERYWHERE,
        });
        self.ops.push(op);
    }

    fn push_bounds(&mut self, r: RectF) {
        self.bounds = if self.bounds.is_empty() {
            r
        } else {
            let x = self.bounds.x.min(r.x);
            let y = self.bounds.y.min(r.y);
            RectF::new(
                x,
                y,
                self.bounds.right().max(r.right()) - x,
                self.bounds.bottom().max(r.bottom()) - y,
            )
        };
    }

    /// Convenience: record an op whose bounds are implied by its geometry.
    /// The geometry is taken as-is, so inside a `Translate` use [`push`]
    /// with an explicit rect instead.
    ///
    /// [`push`]: DisplayItemList::push
    pub fn push_op(&mut self, op: PaintOp) {
        let rect = op_bounds(&op);
        self.push(op, rect);
    }

    /// Appends `other`'s ops drawn `dx`, `dy` further (cc's `DrawRecordOp`
    /// under a translate), its bounds and visual rects moved with them: a
    /// view's painting into its parent's layer.
    pub fn push_list_at(&mut self, other: &DisplayItemList, dx: f32, dy: f32) {
        self.push(PaintOp::Save, None);
        self.push(PaintOp::Translate { dx, dy }, None);
        self.ops.extend(other.ops.iter().cloned());
        self.visual_rects
            .extend(other.visual_rects.iter().map(|r| r.offset(dx, dy)));
        self.push(PaintOp::Restore, None);
        if !other.bounds.is_empty() {
            self.push_bounds(other.bounds.offset(dx, dy));
        }
    }

    /// `DisplayItemList::Finalize`: no more ops may be added. Each state op
    /// gets the union of the visual rects of the drawing ops it applies to,
    /// so that whatever meets one of those meets it too, and a layer's list
    /// builds its R-tree.
    pub fn finalize(&mut self) {
        if self.finalized {
            return;
        }
        self.finalized = true;
        self.cover_state_ops();
        if self.usage == UsageHint::TopLevel {
            self.rtree = Some(rtree::RTree::new(self.visual_rects.iter().copied()));
        }
    }

    /// cc's paired begin and end: a `Save` (or a group) and its `Restore`
    /// get the union of everything between them; a transform or a clip,
    /// of everything after it until its scope's end. Every op in a group
    /// whose filter or blend mode may draw past its content may draw
    /// anywhere.
    fn cover_state_ops(&mut self) {
        // Forward: the groups that may draw past their content.
        let mut groups: Vec<bool> = Vec::new();
        let mut inside = 0;
        for (op, rect) in self.ops.iter().zip(&mut self.visual_rects) {
            let opens = matches!(
                op,
                PaintOp::Save | PaintOp::SaveLayer { .. } | PaintOp::SaveLayerAlpha { .. }
            );
            if opens {
                let reaching = reaches_past_its_content(op);
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "one per group in `groups`, which a Vec's length bounds"
                )]
                {
                    inside += usize::from(reaching);
                }
                groups.push(reaching);
            }
            if inside > 0 {
                *rect = EVERYWHERE;
            }
            if matches!(op, PaintOp::Restore) && groups.pop() == Some(true) {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "the group popped was counted in `inside` as it opened"
                )]
                {
                    inside -= 1;
                }
            }
        }
        // Backward: what each scope draws, from its end.
        let mut scopes: Vec<(Option<usize>, RectF)> = vec![(None, RectF::default())];
        for i in (0..self.ops.len()).rev() {
            let op = &self.ops[i];
            if !is_state_op(op) {
                let drawn = self.visual_rects[i];
                let scope = scopes.last_mut().expect("the outermost scope stays");
                scope.1 = scope.1.union(&drawn);
                continue;
            }
            match op {
                PaintOp::Restore => scopes.push((Some(i), RectF::default())),
                PaintOp::Save | PaintOp::SaveLayer { .. } | PaintOp::SaveLayerAlpha { .. }
                    if scopes.len() > 1 =>
                {
                    let (restore, drawn) = scopes.pop().expect("more than the outermost");
                    let drawn = drawn.union(&self.visual_rects[i]);
                    self.visual_rects[i] = drawn;
                    if let Some(restore) = restore {
                        self.visual_rects[restore] = drawn;
                    }
                    let outer = scopes.last_mut().expect("the outermost scope stays");
                    outer.1 = outer.1.union(&drawn);
                }
                // A transform, a clip, or a save never restored: it applies
                // to the rest of its scope.
                _ => {
                    let drawn = scopes.last().expect("the outermost scope stays").1;
                    self.visual_rects[i] = drawn.union(&self.visual_rects[i]);
                }
            }
        }
        // A restore with no save before it restores nothing: it keeps no
        // rect, and isn't played.
    }

    pub fn ops(&self) -> &[PaintOp] {
        &self.ops
    }

    /// Each op's visual rect (see [`DisplayItemList`]).
    pub fn visual_rects(&self) -> &[RectF] {
        &self.visual_rects
    }

    pub fn bounds(&self) -> RectF {
        self.bounds
    }

    /// The ops to play to draw `rect` (in the layer's space, CSS px), in
    /// order: those whose visual rect meets it (cc's `DisplayItemList::Raster`
    /// searching its R-tree). All of them for a list without an index.
    /// None means playing the list leaves `rect` as it was cleared (cc's
    /// `GetColorIfSolidInRect` finding no op there).
    pub fn ops_in(&self, rect: RectF) -> Vec<usize> {
        match &self.rtree {
            Some(rtree) => rtree.search(rect),
            None => (0..self.ops.len()).collect(),
        }
    }

    /// cc's `DisplayItemList::GetColorIfSolidInRect` (its `SolidColorAnalyzer`):
    /// the one color playing `ops` (`ops_in`'s, in order) leaves all of
    /// `rect` (the layer's device pixels at `scale`) when it was cleared to
    /// `clear`, as `TileDrawInfo` would read it back: opaque, or
    /// transparent. `None` when the ops can't tell, or the answer is
    /// neither. Ops whose geometry misses `rect` draw nothing in it; one
    /// opaque fill that covers it gives its color. Anything else (text,
    /// images, paths that reach `rect`, groups, a clip that isn't a rect,
    /// rotations) gives up at once: the tile is rastered.
    pub fn color_if_solid_in_rect(
        &self,
        ops: &[usize],
        rect: RectF,
        scale: f32,
        clear: Color,
    ) -> Option<Color> {
        #[derive(Clone, Copy)]
        enum Clip {
            None,
            Rect(RectF),
            // A rect, but under a matrix that turns it.
            Unknown,
        }
        let only_scale_and_translate = |t: Transform| {
            let [_, b, c, _, _, _] = t.0;
            (b == 0.0 && c == 0.0).then_some(t)
        };
        let to_device = Transform::scale(scale, scale);
        let mut color = match clear.a {
            255 => Some(clear),
            0 => Some(Color::TRANSPARENT),
            _ => None,
        };
        // In the layer's space, CSS px; `None` if it turns things.
        let mut matrix = Some(Transform::IDENTITY);
        let mut clip = Clip::None;
        let mut saved = Vec::new();
        for &i in ops {
            let op = &self.ops[i];
            let clip_to = |clip: Clip, r: RectF| match (clip, matrix) {
                (Clip::Unknown, _) | (_, None) => Clip::Unknown,
                (Clip::None, Some(m)) => Clip::Rect(to_device.then(&m).map_rect(r)),
                (Clip::Rect(c), Some(m)) => {
                    Clip::Rect(c.intersect(&to_device.then(&m).map_rect(r)))
                }
            };
            match op {
                PaintOp::Save => saved.push((matrix, clip)),
                PaintOp::Restore => {
                    if let Some((m, c)) = saved.pop() {
                        (matrix, clip) = (m, c);
                    }
                }
                PaintOp::Translate { dx, dy } => {
                    matrix = matrix.map(|m| m.then(&Transform::translate(*dx, *dy)));
                }
                PaintOp::Concat { transform } => {
                    matrix = matrix.and_then(|m| only_scale_and_translate(m.then(transform)));
                }
                PaintOp::SetMatrix { transform } => matrix = only_scale_and_translate(*transform),
                PaintOp::ClipRect { rect: r, .. } => clip = clip_to(clip, *r),
                PaintOp::ClipRRect { rrect, .. } if rrect.is_rect() => {
                    clip = clip_to(clip, rrect.rect);
                }
                PaintOp::ClipRRect { .. }
                | PaintOp::ClipPath { .. }
                | PaintOp::ClipPaths { .. }
                | PaintOp::SaveLayerAlpha { .. }
                | PaintOp::SaveLayer { .. }
                | PaintOp::WritePixels { .. } => return None,
                _ => {
                    let (Some(m), Clip::None | Clip::Rect(_)) = (matrix, clip) else {
                        return None;
                    };
                    let device = to_device.then(&m);
                    let clipped = |r: RectF| match clip {
                        Clip::Rect(c) => c.intersect(&r),
                        _ => r,
                    };
                    let drawn = match op {
                        PaintOp::DrawColor { .. } => EVERYWHERE,
                        _ => match op_bounds(op) {
                            // A blur may reach a little past its 3 sigmas.
                            Some(b) if matches!(op, PaintOp::DrawShadow { .. }) => {
                                device.map_rect(b).inset(-1.0)
                            }
                            Some(b) => device.map_rect(b),
                            // An empty path: nothing.
                            None => continue,
                        },
                    };
                    if !rtree::meets(&clipped(drawn), &rect) {
                        continue;
                    }
                    let fill = match op {
                        PaintOp::DrawRect { rect: r, flags }
                            if flags.style == PaintStyle::Fill
                                && flags.shader.is_none()
                                && flags.blur == 0.0
                                && flags.dash.is_none()
                                && matches!(
                                    flags.blend_mode,
                                    BlendMode::SrcOver | BlendMode::Src
                                ) =>
                        {
                            (device.map_rect(*r), flags.color)
                        }
                        PaintOp::DrawColor {
                            color,
                            blend_mode: BlendMode::SrcOver | BlendMode::Src,
                        } => (EVERYWHERE, *color),
                        _ => return None,
                    };
                    let area = clipped(fill.0);
                    let covers = area.x <= rect.x
                        && area.y <= rect.y
                        && area.right() >= rect.right()
                        && area.bottom() >= rect.bottom();
                    if fill.1.a != 255 || !covers {
                        return None;
                    }
                    color = Some(fill.1);
                }
            }
        }
        color
    }

    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// Whether `op` only changes the state ops after it draw with.
fn is_state_op(op: &PaintOp) -> bool {
    matches!(
        op,
        PaintOp::Save
            | PaintOp::Restore
            | PaintOp::Translate { .. }
            | PaintOp::ClipRect { .. }
            | PaintOp::ClipRRect { .. }
            | PaintOp::ClipPath { .. }
            | PaintOp::ClipPaths { .. }
            | PaintOp::Concat { .. }
            | PaintOp::SetMatrix { .. }
            | PaintOp::SaveLayerAlpha { .. }
            | PaintOp::SaveLayer { .. }
    )
}

/// Whether `op` starts a group whose filter (a blur's, a shadow's) or blend
/// mode (one that clears what is under transparent pixels) may draw past
/// what is drawn in it.
fn reaches_past_its_content(op: &PaintOp) -> bool {
    match op {
        PaintOp::SaveLayer { flags, .. } => {
            flags.image_filter.is_some() || flags.blend_mode != BlendMode::SrcOver
        }
        _ => false,
    }
}

/// What `op` draws on, as its geometry says; `None` for state ops.
fn op_bounds(op: &PaintOp) -> Option<RectF> {
    match op {
        PaintOp::DrawRect { rect, flags }
        | PaintOp::DrawRRect {
            rrect: RRectF { rect, .. },
            flags,
        } => Some(match flags.style {
            PaintStyle::Fill => *rect,
            PaintStyle::Stroke { width } => rect.inset(-width / 2.0),
        }),
        PaintOp::ClearRect { rect } => Some(*rect),
        PaintOp::DrawDRRect { outer, .. } => Some(outer.rect),
        PaintOp::DrawClipped { op, .. } => op_bounds(op),
        // A Gaussian blur reaches about three standard deviations.
        PaintOp::DrawShadow {
            shape,
            clip,
            inside,
            flags,
            ..
        } => Some(if *inside {
            clip.rect
        } else {
            shape.rect.inset(-3.0 * flags.blur)
        }),
        PaintOp::DrawLine { p0, p1, flags } => {
            let w = match flags.style {
                PaintStyle::Stroke { width } => width,
                PaintStyle::Fill => 1.0,
            };
            let x = p0.x.min(p1.x);
            let y = p0.y.min(p1.y);
            Some(RectF::new(x, y, (p0.x - p1.x).abs(), (p0.y - p1.y).abs()).inset(-w / 2.0))
        }
        PaintOp::DrawGlyphs { bounds, .. } => Some(*bounds),
        PaintOp::DrawImage { dst, .. } | PaintOp::DrawRecord { dst, .. } => Some(*dst),
        PaintOp::DrawPath { path, flags } => path.bounds().map(|bounds| match flags.style {
            PaintStyle::Fill => bounds,
            // Past the path by half the width, or as far as a miter or a
            // square cap reaches (Skia's `SkStrokeRec::GetInflationRadius`).
            PaintStyle::Stroke { width } => {
                let mut reach: f32 = 1.0;
                if flags.join == LineJoin::Miter {
                    reach = reach.max(flags.miter_limit);
                }
                if flags.cap == LineCap::Square {
                    reach = reach.max(std::f32::consts::SQRT_2);
                }
                bounds.inset(-width / 2.0 * reach)
            }
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_union() {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(10.0, 10.0, 10.0, 10.0),
            flags: PaintFlags::fill(Color::BLACK),
        });
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(50.0, 0.0, 10.0, 10.0),
            flags: PaintFlags::fill(Color::BLACK),
        });
        list.finalize();
        assert_eq!(list.bounds(), RectF::new(10.0, 0.0, 50.0, 20.0));
        assert_eq!(list.ops().len(), 3);
    }

    #[test]
    fn a_shadows_bounds_take_in_its_blur_or_stay_in_its_clip() {
        let shape = RRectF::from_rect(RectF::new(10.0, 10.0, 20.0, 20.0));
        let flags = PaintFlags {
            blur: 2.0,
            ..PaintFlags::fill(Color::BLACK)
        };
        let bounds = |inside| {
            let mut list = DisplayItemList::new();
            list.push_op(PaintOp::DrawShadow {
                shape,
                hole: None,
                clip: RRectF::from_rect(RectF::new(0.0, 0.0, 15.0, 15.0)),
                inside,
                flags: flags.clone(),
            });
            list.finalize();
            list.bounds()
        };
        // Three deviations past the shape, the blur's reach.
        assert_eq!(bounds(false), RectF::new(4.0, 4.0, 32.0, 32.0));
        assert_eq!(bounds(true), RectF::new(0.0, 0.0, 15.0, 15.0));
    }

    #[test]
    fn a_clipped_ops_bounds_are_its_ops() {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawClipped {
            polygon: vec![
                PointF::new(0.0, 0.0),
                PointF::new(1.0, 0.0),
                PointF::new(0.0, 1.0),
            ]
            .into(),
            op: Box::new(PaintOp::DrawRect {
                rect: RectF::new(10.0, 10.0, 5.0, 5.0),
                flags: PaintFlags::fill(Color::BLACK),
            }),
        });
        list.finalize();
        assert_eq!(list.bounds(), RectF::new(10.0, 10.0, 5.0, 5.0));
    }

    fn rect_op(rect: RectF) -> PaintOp {
        PaintOp::DrawRect {
            rect,
            flags: PaintFlags::fill(Color::BLACK),
        }
    }

    #[test]
    fn a_state_ops_visual_rect_is_what_it_applies_to() {
        let (a, b, c, d) = (
            RectF::new(0.0, 0.0, 10.0, 10.0),
            RectF::new(100.0, 0.0, 10.0, 10.0),
            RectF::new(0.0, 100.0, 10.0, 10.0),
            RectF::new(500.0, 500.0, 10.0, 10.0),
        );
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::ClipRect {
            rect: RectF::new(0.0, 0.0, 200.0, 200.0),
            anti_alias: true,
        });
        list.push_op(rect_op(a));
        list.push_op(PaintOp::SaveLayerAlpha {
            bounds: None,
            alpha: 0.5,
        });
        list.push_op(PaintOp::Translate { dx: 1.0, dy: 1.0 });
        list.push(rect_op(b), Some(b));
        list.push_op(PaintOp::Restore);
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::Restore);
        list.push_op(rect_op(c));
        list.push_op(PaintOp::Restore);
        // Never restored: to the end.
        list.push_op(PaintOp::SetMatrix {
            transform: Transform::translate(2.0, 2.0),
        });
        list.push_op(rect_op(d));
        list.finalize();
        let abc = a.union(&b).union(&c);
        let empty = RectF::default();
        assert_eq!(
            list.visual_rects(),
            [abc, abc, a, b, b, b, b, empty, empty, c, abc, d, d]
        );
        // What meets b: it, and the state it is drawn with.
        assert_eq!(list.ops_in(b.inset(2.0)), [0, 1, 3, 4, 5, 6, 10]);
        assert_eq!(
            list.ops_in(RectF::new(300.0, 300.0, 10.0, 10.0)),
            [0usize; 0]
        );
        // Without an index, all of them.
        let mut record = DisplayItemList::record();
        record.push_op(rect_op(a));
        record.push_op(rect_op(b));
        record.finalize();
        assert_eq!(record.ops_in(d), [0, 1]);
    }

    #[test]
    fn what_may_draw_anywhere_meets_every_rect() {
        let far = RectF::new(5000.0, -5000.0, 1.0, 1.0);
        let tested = |ops: Vec<(PaintOp, Option<RectF>)>| {
            let mut list = DisplayItemList::new();
            for (op, visual_rect) in ops {
                list.push(op, visual_rect);
            }
            list.finalize();
            list.ops_in(far)
        };
        let r = RectF::new(0.0, 0.0, 10.0, 10.0);
        let group = |flags| PaintOp::SaveLayer {
            bounds: None,
            flags,
        };
        let color = PaintOp::DrawColor {
            color: Color::BLACK,
            blend_mode: BlendMode::SrcOver,
        };
        // A drawing op without its rect, and the state it is drawn with.
        assert_eq!(
            tested(vec![
                (PaintOp::Save, None),
                (rect_op(r), Some(r)),
                (color.clone(), None),
                (PaintOp::Restore, None),
            ]),
            [0, 2, 3]
        );
        assert_eq!(tested(vec![(rect_op(r), None)]), [0]);
        // All of a group whose filter or blend mode may reach past it.
        let shadow = group(LayerFlags {
            image_filter: Some(ImageFilter::DropShadow {
                offset: (100.0, 0.0),
                sigma: 2.0,
                color: Color::BLACK,
                shadow_only: false,
            }),
            ..LayerFlags::default()
        });
        let blended = group(LayerFlags {
            blend_mode: BlendMode::Src,
            ..LayerFlags::default()
        });
        for reaching in [shadow, blended] {
            assert_eq!(
                tested(vec![
                    (rect_op(r), Some(r)),
                    (reaching, None),
                    (PaintOp::Save, None),
                    (rect_op(r), Some(r)),
                    (PaintOp::Restore, None),
                    (PaintOp::Restore, None),
                    (rect_op(r), Some(r)),
                ]),
                [1, 2, 3, 4, 5]
            );
        }
        // Not one whose alpha only fades what it holds.
        let faded = group(LayerFlags {
            alpha: 0.5,
            ..LayerFlags::default()
        });
        for group in [
            faded,
            PaintOp::SaveLayerAlpha {
                bounds: None,
                alpha: 0.5,
            },
        ] {
            assert_eq!(
                tested(vec![
                    (group, None),
                    (rect_op(r), Some(r)),
                    (PaintOp::Restore, None)
                ]),
                [0usize; 0]
            );
        }
        // Nor what is drawn after such a group.
        let mut inner = DisplayItemList::record();
        inner.push(color, None);
        inner.finalize();
        let mut list = DisplayItemList::new();
        list.push_list_at(&inner, 1.0, 1.0);
        list.push_op(rect_op(r));
        list.finalize();
        assert_eq!(list.ops_in(far), [0, 1, 2, 3]);
    }

    /// What `color_if_solid_in_rect` makes of `ops`, for the tile 64,64
    /// to 128,128 at 2x (32,32 to 64,64 in CSS px), cleared to white.
    fn solid(ops: Vec<(PaintOp, Option<RectF>)>, clear: Color) -> Option<Color> {
        let mut list = DisplayItemList::new();
        for (op, visual_rect) in ops {
            list.push(op, visual_rect);
        }
        list.finalize();
        let tile = RectF::new(64.0, 64.0, 64.0, 64.0);
        let found = list.ops_in(tile.inset(-1.0).scale(0.5));
        list.color_if_solid_in_rect(&found, tile, 2.0, clear)
    }

    #[test]
    fn a_tile_is_of_one_color_if_its_ops_say_so() {
        let red = Color::rgb(255, 0, 0);
        let fill = |rect: RectF, flags: PaintFlags| {
            let visual = op_bounds(&PaintOp::DrawRect {
                rect,
                flags: flags.clone(),
            });
            (PaintOp::DrawRect { rect, flags }, visual)
        };
        let over = RectF::new(32.0, 32.0, 32.0, 32.0);
        let white = Some(Color::WHITE);
        // Nothing drawn: the clear color, if it is opaque or transparent.
        assert_eq!(solid(vec![], Color::WHITE), white);
        assert_eq!(
            solid(vec![], Color::rgba(9, 9, 9, 0)),
            Some(Color::TRANSPARENT)
        );
        assert_eq!(solid(vec![], Color::rgba(255, 255, 255, 128)), None);
        // An opaque fill over the tile, edges on its edges.
        assert_eq!(
            solid(vec![fill(over, PaintFlags::fill(red))], Color::WHITE),
            Some(red)
        );
        let src = PaintFlags {
            blend_mode: BlendMode::Src,
            ..PaintFlags::fill(red)
        };
        assert_eq!(solid(vec![fill(over, src)], Color::WHITE), Some(red));
        // ... and over a translucent clear color.
        let translucent = Color::rgba(255, 255, 255, 128);
        assert_eq!(
            solid(vec![fill(over, PaintFlags::fill(red))], translucent),
            Some(red)
        );
        // Not one that leaves a sliver, nor a translucent or patterned one,
        // nor a stroke or a blurred one.
        let short = RectF::new(32.0, 32.0, 31.75, 32.0);
        assert_eq!(
            solid(vec![fill(short, PaintFlags::fill(red))], Color::WHITE),
            None
        );
        let faint = PaintFlags::fill(Color::rgba(255, 0, 0, 254));
        assert_eq!(solid(vec![fill(over, faint)], Color::WHITE), None);
        let shaded = PaintFlags {
            shader: Some(Arc::new(PaintShader::Empty)),
            ..PaintFlags::fill(red)
        };
        assert_eq!(solid(vec![fill(over, shaded)], Color::WHITE), None);
        let blurred = PaintFlags {
            blur: 1.0,
            ..PaintFlags::fill(red)
        };
        assert_eq!(solid(vec![fill(over, blurred)], Color::WHITE), None);
        assert_eq!(
            solid(
                vec![fill(over, PaintFlags::stroke(red, 200.0))],
                Color::WHITE
            ),
            None
        );
        let multiplied = PaintFlags {
            blend_mode: BlendMode::Multiply,
            ..PaintFlags::fill(red)
        };
        assert_eq!(solid(vec![fill(over, multiplied)], Color::WHITE), None);
        let dashed = PaintFlags {
            dash: Dash::new(&[4.0, 4.0], 0.0),
            ..PaintFlags::fill(red)
        };
        assert_eq!(solid(vec![fill(over, dashed)], Color::WHITE), None);
        // An empty path draws nothing.
        let empty = PaintOp::DrawPath {
            path: Path::new(),
            flags: PaintFlags::fill(Color::BLACK),
        };
        let ops = vec![(empty, Some(over)), fill(over, PaintFlags::fill(red))];
        assert_eq!(solid(ops, Color::WHITE), Some(red));
        // What only its visual rect brings near: a pixel off, nothing.
        let beside = RectF::new(20.0, 32.0, 12.0, 32.0);
        assert_eq!(
            solid(vec![fill(beside, PaintFlags::fill(red))], Color::WHITE),
            white
        );
        // Then whatever covers it last.
        let blue = Color::rgb(0, 0, 255);
        let ops = vec![
            fill(over, PaintFlags::fill(red)),
            fill(over.inset(-10.0), PaintFlags::fill(blue)),
        ];
        assert_eq!(solid(ops, Color::WHITE), Some(blue));
        // A DrawColor fills the clip.
        let color = |c: Color| PaintOp::DrawColor {
            color: c,
            blend_mode: BlendMode::SrcOver,
        };
        assert_eq!(solid(vec![(color(red), None)], Color::WHITE), Some(red));
        assert_eq!(
            solid(vec![(color(Color::rgba(255, 0, 0, 9)), None)], Color::WHITE),
            None
        );
        let screened = PaintOp::DrawColor {
            color: red,
            blend_mode: BlendMode::Screen,
        };
        assert_eq!(solid(vec![(screened, None)], Color::WHITE), None);
        let clip = |rect: RectF| PaintOp::ClipRect {
            rect,
            anti_alias: true,
        };
        let clipped = |rect, op: (PaintOp, Option<RectF>)| {
            vec![
                (PaintOp::Save, None),
                (clip(rect), None),
                op,
                (PaintOp::Restore, None),
            ]
        };
        assert_eq!(
            solid(clipped(over.inset(-1.0), (color(red), None)), Color::WHITE),
            Some(red)
        );
        assert_eq!(
            solid(clipped(over.inset(1.0), (color(red), None)), Color::WHITE),
            None
        );
        // A clip it is all outside of hides it.
        let away = RectF::new(0.0, 0.0, 30.0, 30.0);
        assert_eq!(
            solid(
                clipped(away, fill(over, PaintFlags::fill(red))),
                Color::WHITE
            ),
            white
        );
        // A clip a rect covers, and one cut short.
        let rounded = |radius| PaintOp::ClipRRect {
            rrect: RRectF::uniform(over.inset(-4.0), radius),
            anti_alias: true,
        };
        let in_rrect = |radius| {
            vec![
                (rounded(radius), None),
                fill(over.inset(-8.0), PaintFlags::fill(red)),
            ]
        };
        assert_eq!(solid(in_rrect(0.0), Color::WHITE), Some(red));
        assert_eq!(solid(in_rrect(2.0), Color::WHITE), None);
        // The restore ends the clip: the fill after it isn't cut.
        let mut ops = clipped(away, fill(over, PaintFlags::fill(red)));
        ops.push(fill(over, PaintFlags::fill(blue)));
        assert_eq!(solid(ops, Color::WHITE), Some(blue));
        // A scale and a translation are followed; a rotation gives up.
        let scaled = vec![
            (
                PaintOp::SetMatrix {
                    transform: Transform::scale(2.0, 2.0),
                },
                None,
            ),
            (PaintOp::Translate { dx: 4.0, dy: 4.0 }, None),
            (
                PaintOp::Concat {
                    transform: Transform::translate(-4.0, -4.0),
                },
                None,
            ),
            fill(RectF::new(16.0, 16.0, 16.0, 16.0), PaintFlags::fill(red)),
        ];
        let visual = Some(RectF::new(32.0, 32.0, 32.0, 32.0));
        let mut scaled = scaled;
        scaled[3].1 = visual;
        assert_eq!(solid(scaled.clone(), Color::WHITE), Some(red));
        scaled[3].0 = PaintOp::DrawRect {
            rect: RectF::new(16.0, 16.0, 15.0, 16.0),
            flags: PaintFlags::fill(red),
        };
        assert_eq!(solid(scaled, Color::WHITE), None);
        let turned = Transform([0.0, 1.0, -1.0, 0.0, 64.0, 0.0]);
        for transform in [
            PaintOp::SetMatrix { transform: turned },
            PaintOp::Concat { transform: turned },
        ] {
            let ops = vec![(transform, None), fill(over, PaintFlags::fill(red))];
            assert_eq!(solid(ops, Color::WHITE), None);
        }
        // Ops it doesn't know, where they reach the tile, give up; a group
        // too.
        let rounded_fill = PaintOp::DrawRRect {
            rrect: RRectF::uniform(over.inset(-8.0), 2.0),
            flags: PaintFlags::fill(red),
        };
        assert_eq!(
            solid(vec![(rounded_fill.clone(), Some(over))], Color::WHITE),
            None
        );
        let group = PaintOp::SaveLayerAlpha {
            bounds: None,
            alpha: 1.0,
        };
        let grouped = vec![
            (group, None),
            fill(over, PaintFlags::fill(red)),
            (PaintOp::Restore, None),
        ];
        assert_eq!(solid(grouped, Color::WHITE), None);
        // A shadow's blur may reach a pixel past its 3 sigmas.
        let shadow = |left: f32| {
            let shape = RRectF::from_rect(RectF::new(left, 32.0, 2.0, 32.0));
            let op = PaintOp::DrawShadow {
                shape,
                hole: None,
                clip: shape,
                inside: false,
                flags: PaintFlags {
                    blur: 1.0,
                    ..PaintFlags::fill(Color::BLACK)
                },
            };
            let visual = op_bounds(&op);
            vec![(op, visual)]
        };
        // 2 px wide and 3 sigmas past: to 29 CSS px, 58 device px; a pixel
        // more, 59; the tile starts at 64.
        assert_eq!(solid(shadow(24.0), Color::WHITE), white);
        // To 31.75, 63.5 device px: only the pixel more reaches the tile.
        assert_eq!(solid(shadow(26.75), Color::WHITE), None);
    }

    #[test]
    fn a_list_pushed_at_an_offset_is_drawn_there() {
        let mut inner = DisplayItemList::new();
        inner.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 10.0, 5.0),
            flags: PaintFlags::fill(Color::BLACK),
        });
        inner.finalize();
        let mut list = DisplayItemList::new();
        list.push_list_at(&inner, 20.0, 30.0);
        list.finalize();
        assert_eq!(list.bounds(), RectF::new(20.0, 30.0, 10.0, 5.0));
        // Its ops' visual rects too: found where they are drawn.
        assert_eq!(list.ops_in(RectF::new(25.0, 32.0, 1.0, 1.0)), [0, 1, 2, 3]);
        assert_eq!(list.ops_in(RectF::new(5.0, 2.0, 1.0, 1.0)), [0usize; 0]);
        assert!(matches!(
            list.ops(),
            [
                PaintOp::Save,
                PaintOp::Translate { dx: 20.0, dy: 30.0 },
                PaintOp::DrawRect { .. },
                PaintOp::Restore
            ]
        ));
    }

    #[test]
    fn a_stroked_paths_bounds_take_in_its_joins_and_caps() {
        let p = PointF::new;
        let mut path = Path::new();
        path.move_to(p(10.0, 10.0));
        path.cubic_to(p(30.0, 0.0), p(30.0, 40.0), p(50.0, 30.0));
        let bounds = |flags: PaintFlags| {
            let mut list = DisplayItemList::new();
            list.push_op(PaintOp::DrawPath {
                path: path.clone(),
                flags,
            });
            list.bounds()
        };
        // Filled: the control points' box.
        assert_eq!(
            bounds(PaintFlags::fill(Color::BLACK)),
            RectF::new(10.0, 0.0, 40.0, 40.0)
        );
        // Miter joins reach up to 4 half widths out, by default.
        let stroke = PaintFlags::stroke(Color::BLACK, 2.0);
        assert_eq!(bounds(stroke.clone()), RectF::new(6.0, -4.0, 48.0, 48.0));
        // Round joins, half the width; square caps, its diagonal.
        let round = PaintFlags {
            join: LineJoin::Round,
            ..stroke.clone()
        };
        assert_eq!(bounds(round.clone()), RectF::new(9.0, -1.0, 42.0, 42.0));
        let square = bounds(PaintFlags {
            cap: LineCap::Square,
            ..round
        });
        let r = std::f32::consts::SQRT_2;
        assert_eq!(square, RectF::new(10.0, 0.0, 40.0, 40.0).inset(-r));
    }

    #[test]
    fn dashes_repeat_an_odd_count_and_are_solid_when_they_draw_all_or_nothing() {
        let dash = Dash::new(&[5.0, 3.0, 1.0], -2.0).unwrap();
        assert_eq!(dash.intervals(), [5.0, 3.0, 1.0, 5.0, 3.0, 1.0]);
        assert_eq!(dash.phase(), -2.0);
        assert_eq!(Dash::new(&[4.0, 0.0], 0.0).unwrap().intervals(), [4.0, 0.0]);
        assert_eq!(Dash::new(&[0.0, 6.0], 0.0).unwrap().intervals(), [0.0, 6.0]);
        assert_eq!(Dash::new(&[], 0.0), None);
        assert_eq!(Dash::new(&[0.0, 0.0], 0.0), None);
        assert_eq!(Dash::new(&[4.0, -1.0], 0.0), None);
        assert_eq!(Dash::new(&[4.0, f32::INFINITY], 0.0), None);
        assert_eq!(Dash::new(&[4.0, 2.0], f32::NAN), None);
    }
}
