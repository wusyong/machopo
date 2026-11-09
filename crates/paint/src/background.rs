//! A background layer's geometry (Blink's `BackgroundImageGeometry`) and
//! its gradient as a shader (`CSSGradientValue`'s color stop resolution and
//! `Gradient`): where the image's first tile goes, how big it is, whether
//! it repeats, and what area the layer fills.

use std::sync::Arc;

use base::numerics::clamp_round;
use compositor_paint::{
    ColorInterpolation, DisplayItemList, FilterQuality, GradientShader, GradientShaderKind,
    GradientStop, ShaderImage, TileMode, TiledShader,
};
use computed_style::{
    BackgroundLayer, BackgroundRepeat, BackgroundSize, ComputedStyle, Gradient, GradientItem,
    GradientKind, HorizontalSide, LengthPercentage, LengthPercentageOrAuto, LinearDirection,
    RadialShape, RadialSize, VerticalSide,
};
use gfx::{Color, ImageData, PointF, RectF, Size, SizeF, Transform};
use layout::SvgNaturalSize;

/// The part of a text node's data drawn as selected (Blink's
/// `LayoutSelection` state of its `LayoutText`), UTF-16 (an end past the
/// data's: the selection goes on after it); and whether in the active
/// colors (its frame has the focus on a page that has, Blink's
/// `FrameSelection::FrameIsFocusedAndActive`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSelection {
    pub range: std::ops::Range<u32>,
    pub active: bool,
}

/// The colors selected text with no `::selection` style is drawn in, its
/// background and its text (Blink's `LayoutTheme::
/// ActiveSelectionBackgroundColor` and the like): the active ones while
/// the page has the focus, else the inactive.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SelectionColors {
    pub background: Color,
    pub text: Color,
}

impl SelectionColors {
    /// The active ones of `colors`.
    pub fn active(colors: &gfx::SystemColors) -> Self {
        Self {
            background: colors.highlight,
            text: colors.highlight_text,
        }
    }

    /// The inactive ones of `colors`.
    pub fn inactive(colors: &gfx::SystemColors) -> Self {
        Self {
            background: colors.inactive_highlight,
            text: colors.inactive_highlight_text,
        }
    }
}

/// Where the caret is drawn (Blink's `FrameCaret`, which paints it with
/// its block's foreground, `CaretDisplayItemClient`), while it is shown:
/// `block` is the element of the box whose lines it is on (the box's
/// parent's for an anonymous block's), or that holds it with none.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Caret {
    pub at: CaretPosition,
    pub block: gfx::NodeId,
}

/// Where a caret is (Blink's `PositionWithAffinity`, downstream).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CaretPosition {
    /// Before the UTF-16 code unit `offset` of text node `text`'s data
    /// (at its end: after it).
    Text { text: gfx::NodeId, offset: u32 },
    /// At a `<br>`, where its line's text ends.
    Break(gfx::NodeId),
    /// In an empty editable element, where its line would start (Blink's
    /// `LocalCaretRectForEmptyElement`): its first available font's
    /// ascent and descent, in a line `line_height` high.
    Empty {
        element: gfx::NodeId,
        ascent: f32,
        descent: f32,
        line_height: f32,
    },
}

/// Where paint finds the images backgrounds name, by URL (Blink's
/// `StyleImage`, which points at the document's `ImageResourceContent`):
/// the image and its natural size, once loaded. And what else it draws
/// from the document rather than the fragments: canvases' bitmaps, the
/// selection.
pub trait BackgroundImages {
    fn image(&self, url: &str) -> Option<(ImageData, Size)>;

    /// The snapshot of the canvas `element`'s bitmap (Blink's
    /// `HTMLCanvasElement::Snapshot`), decoded pixels; `None` if it shows
    /// nothing (no context, or nothing drawn on a transparent one). The
    /// same `ImageData` (by id) while the bitmap doesn't change. None by
    /// default: no canvas has anything.
    fn canvas(&self, element: gfx::NodeId) -> Option<ImageData> {
        let _ = element;
        None
    }

    /// The SVG image a background's URL shows, once loaded: the number
    /// layout knows it by, and its natural dimensions.
    fn svg_url(&self, url: &str) -> Option<(u64, SvgNaturalSize)> {
        let _ = url;
        None
    }

    /// SVG image `image` (layout's number for it) painted to be drawn into
    /// a box `size` big, CSS px (Blink's `SVGImageForContainer`): its paint,
    /// the one list, and the size of the space it is in, which the box
    /// scales to. `None` by default, and for a size not painted yet: the
    /// renderer paints it then, and `client`'s items again.
    fn svg_image(
        &self,
        image: u64,
        size: SizeF,
        client: Option<gfx::NodeId>,
    ) -> Option<(Arc<DisplayItemList>, SizeF)> {
        let _ = (image, size, client);
        None
    }

    /// What of text node `text` is drawn as selected, if any. None by
    /// default.
    fn selection(&self, text: gfx::NodeId) -> Option<TextSelection> {
        let _ = text;
        None
    }

    /// The caret, while it is shown. None by default.
    fn caret(&self) -> Option<Caret> {
        None
    }

    /// The colors selected text with no `::selection` style is drawn in,
    /// the active or the inactive ones: by default the default system
    /// colors'.
    fn selection_colors(&self, active: bool) -> SelectionColors {
        let colors = gfx::SystemColors::DEFAULT;
        if active {
            SelectionColors::active(&colors)
        } else {
            SelectionColors::inactive(&colors)
        }
    }
}

/// No images loaded: only gradients paint.
impl BackgroundImages for () {
    fn image(&self, _: &str) -> Option<(ImageData, Size)> {
        None
    }
}

/// What one layer draws: `rect` filled with `shader`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerPaint {
    pub(crate) rect: RectF,
    pub(crate) shader: TiledShader,
}

/// `length`, against `basis` for a percentage.
fn resolve(length: LengthPercentage, basis: f32) -> f32 {
    length.resolve(basis)
}

/// What a background image has of a natural size: each dimension, and a
/// ratio (width over height), any of which it may lack (an SVG image; a
/// gradient has none).
#[derive(Debug, Clone, Copy, Default)]
struct Natural {
    width: Option<f32>,
    height: Option<f32>,
    ratio: Option<f32>,
}

impl Natural {
    /// A bitmap's: none if it is empty on an axis, which then sizes as a
    /// gradient does.
    fn of_size(size: SizeF) -> Self {
        if size.width <= 0.0 || size.height <= 0.0 {
            return Self::default();
        }
        Self {
            width: Some(size.width),
            height: Some(size.height),
            ratio: Some(size.width / size.height),
        }
    }
}

/// The size of the layer's tile (css-backgrounds 3, "background-size",
/// with css-images 3's default sizing algorithm), before `round`: what
/// `background-size` gives, an axis it leaves `auto` through the natural
/// ratio, else the natural dimension, else the positioning area's; `cover`
/// and `contain` scale the ratio to the area, without one the area.
fn tile_size(size: BackgroundSize, natural: Natural, area: SizeF) -> SizeF {
    let ratio = natural.ratio.filter(|r| *r > 0.0 && r.is_finite());
    match size {
        BackgroundSize::Cover | BackgroundSize::Contain => {
            let Some(r) = ratio else {
                return area;
            };
            // The natural size scaled, or a box of the ratio.
            let n = match (natural.width, natural.height) {
                (Some(w), Some(h)) if w > 0.0 && h > 0.0 => SizeF::new(w, h),
                _ => SizeF::new(r, 1.0),
            };
            let (sx, sy) = (area.width / n.width, area.height / n.height);
            let scale = if size == BackgroundSize::Cover {
                sx.max(sy)
            } else {
                sx.min(sy)
            };
            SizeF::new(n.width * scale, n.height * scale)
        }
        BackgroundSize::Explicit(width, height) => {
            let length = |v: LengthPercentageOrAuto, basis: f32| match v {
                LengthPercentageOrAuto::Auto => None,
                LengthPercentageOrAuto::LengthPercentage(v) => Some(resolve(v, basis).max(0.0)),
            };
            match (length(width, area.width), length(height, area.height)) {
                (Some(w), Some(h)) => SizeF::new(w, h),
                (Some(w), None) => SizeF::new(
                    w,
                    ratio.map_or(natural.height.unwrap_or(area.height), |r| w / r),
                ),
                (None, Some(h)) => SizeF::new(
                    ratio.map_or(natural.width.unwrap_or(area.width), |r| h * r),
                    h,
                ),
                (None, None) => match (natural.width, natural.height, ratio) {
                    (Some(w), Some(h), _) => SizeF::new(w, h),
                    (Some(w), None, Some(r)) => SizeF::new(w, w / r),
                    (None, Some(h), Some(r)) => SizeF::new(h * r, h),
                    // A ratio alone: contained in the area.
                    (None, None, Some(_)) => tile_size(BackgroundSize::Contain, natural, area),
                    (w, h, None) => SizeF::new(w.unwrap_or(area.width), h.unwrap_or(area.height)),
                },
            }
        }
    }
}

/// How one axis tiles: the tile's size and start on it, the gap after
/// each tile, and whether it repeats.
struct Axis {
    size: f32,
    start: f32,
    spacing: f32,
    repeats: bool,
}

/// The tiles on one axis of the positioning area (`origin`, `extent`) for
/// a tile of `size`: `round` has already resized it; `space` places as
/// many as fit edge to edge, or one where `background-position` says if
/// fewer than two do.
fn axis(
    repeat: BackgroundRepeat,
    size: f32,
    origin: f32,
    extent: f32,
    position: computed_style::PositionComponent,
) -> Axis {
    let positioned = origin + position.resolve(extent - size);
    match repeat {
        BackgroundRepeat::Space => {
            let count = (extent / size).floor();
            if count >= 2.0 {
                return Axis {
                    size,
                    start: origin,
                    spacing: (extent - count * size) / (count - 1.0),
                    repeats: true,
                };
            }
            Axis {
                size,
                start: positioned,
                spacing: 0.0,
                repeats: false,
            }
        }
        BackgroundRepeat::NoRepeat => Axis {
            size,
            start: positioned,
            spacing: 0.0,
            repeats: false,
        },
        BackgroundRepeat::Repeat | BackgroundRepeat::Round => Axis {
            size,
            start: positioned,
            spacing: 0.0,
            repeats: true,
        },
    }
}

/// `round`: the tile resized to fit a whole number of times, at least
/// once (css-backgrounds 3, "background-repeat").
fn rounded(size: f32, extent: f32) -> f32 {
    if size <= 0.0 || extent <= 0.0 {
        return size;
    }
    extent / (extent / size).round().max(1.0)
}

/// What `layer` draws: its tile in `positioning` (the `background-origin`
/// box), filling `painting` (the `background-clip` box, or the canvas),
/// snapped to device pixels, `dsf` of them per CSS px. `None` if it draws
/// nothing: no image yet, an empty tile or area.
pub(crate) fn layer_paint(
    style: &ComputedStyle,
    layer: &BackgroundLayer,
    images: &dyn BackgroundImages,
    client: Option<gfx::NodeId>,
    painting: RectF,
    positioning: RectF,
    dsf: f32,
) -> Option<LayerPaint> {
    let snap = |v: f32| (v * dsf).round() / dsf;
    // An `image-set()`'s image for `dsf`, its natural size divided by its
    // resolution (Blink's `StyleImage::ImageScaleFactor`).
    let (shown, resolution) = layer.image.shown(dsf)?;
    let (image, natural) = match shown {
        computed_style::BackgroundImage::None | computed_style::BackgroundImage::Set(_) => {
            return None;
        }
        computed_style::BackgroundImage::Url(url) => match images.image(&url.url) {
            Some((data, size)) => (
                Source::Image(data),
                Natural::of_size(SizeF::new(size.width as f32, size.height as f32)),
            ),
            None => {
                let (image, natural) = images.svg_url(&url.url)?;
                let natural = Natural {
                    width: natural.width,
                    height: natural.height,
                    ratio: match (natural.width, natural.height) {
                        (Some(w), Some(h)) if w > 0.0 && h > 0.0 => Some(w / h),
                        _ => natural.ratio,
                    },
                };
                (Source::Svg(image), natural)
            }
        },
        computed_style::BackgroundImage::Gradient(gradient) => {
            (Source::Gradient(gradient), Natural::default())
        }
    };
    let natural = Natural {
        width: natural.width.map(|w| w / resolution),
        height: natural.height.map(|h| h / resolution),
        ratio: natural.ratio,
    };
    if painting.width <= 0.0 || painting.height <= 0.0 {
        return None;
    }
    let area = SizeF::new(positioning.width, positioning.height);
    let mut tile = tile_size(layer.size, natural, area);
    let (repeat_x, repeat_y) = layer.repeat;
    let auto = |v: LengthPercentageOrAuto| v == LengthPercentageOrAuto::Auto;
    let (auto_width, auto_height) = match layer.size {
        BackgroundSize::Explicit(w, h) => (auto(w), auto(h)),
        _ => (false, false),
    };
    if repeat_x == BackgroundRepeat::Round {
        let width = rounded(tile.width, area.width);
        // `round` on one axis only, with `auto` on the other: the other
        // keeps the ratio.
        if repeat_y != BackgroundRepeat::Round && auto_height && tile.width > 0.0 {
            tile.height *= width / tile.width;
        }
        tile.width = width;
    }
    if repeat_y == BackgroundRepeat::Round {
        let height = rounded(tile.height, area.height);
        if repeat_x != BackgroundRepeat::Round && auto_width && tile.height > 0.0 {
            tile.width *= height / tile.height;
        }
        tile.height = height;
    }
    // Blink sizes the tile in `LayoutUnit`s, 1/64 px: one smaller on an
    // axis is empty (an extreme ratio's, `contain`ed).
    if tile.width < 1.0 / 64.0 || tile.height < 1.0 / 64.0 {
        return None;
    }
    // Whole device pixels, at least one, as Blink snaps the tile size
    // (`BackgroundImageGeometry`): a fractional tile repeated would blend
    // its neighbours' edges into each other.
    let length = |v: f32| (v * dsf).round().max(1.0) / dsf;
    tile = SizeF::new(length(tile.width), length(tile.height));
    let x = axis(
        repeat_x,
        tile.width,
        positioning.x,
        area.width,
        layer.position_x,
    );
    let y = axis(
        repeat_y,
        tile.height,
        positioning.y,
        area.height,
        layer.position_y,
    );
    // The tile starts on a device pixel, as Blink snaps the phase.
    let tile_rect = RectF::new(snap(x.start), snap(y.start), x.size, y.size);
    // On an axis that doesn't repeat, only the tile's band is drawn.
    let mut rect = painting;
    if !x.repeats {
        rect = intersect_x(rect, tile_rect.x, tile_rect.right());
    }
    if !y.repeats {
        rect = intersect_y(rect, tile_rect.y, tile_rect.bottom());
    }
    if rect.width <= 0.0 || rect.height <= 0.0 {
        return None;
    }
    let image = match image {
        Source::Image(data) => ShaderImage::Image(data),
        // Painted for the tile's size, or the layer waits for it.
        Source::Svg(image) => {
            let (record, size) = images.svg_image(image, tile_rect.size(), client)?;
            ShaderImage::Record { record, size }
        }
        Source::Gradient(gradient) => ShaderImage::Gradient(gradient_shader(
            style,
            gradient,
            SizeF::new(tile_rect.width, tile_rect.height),
        )),
    };
    Some(LayerPaint {
        rect,
        shader: TiledShader {
            image,
            tile: tile_rect,
            spacing: SizeF::new(x.spacing, y.spacing),
            repeat_x: x.repeats,
            repeat_y: y.repeats,
            transform: Transform::IDENTITY,
            filter: FilterQuality::Medium,
            decal: false,
        },
    })
}

enum Source<'a> {
    Image(ImageData),
    /// An SVG image, by layout's number for it.
    Svg(u64),
    Gradient(&'a Gradient),
}

fn intersect_x(rect: RectF, x0: f32, x1: f32) -> RectF {
    let (a, b) = (rect.x.max(x0), rect.right().min(x1));
    RectF::new(a, rect.y, (b - a).max(0.0), rect.height)
}

fn intersect_y(rect: RectF, y0: f32, y1: f32) -> RectF {
    let (a, b) = (rect.y.max(y0), rect.bottom().min(y1));
    RectF::new(rect.x, a, rect.width, (b - a).max(0.0))
}

// --- gradients -------------------------------------------------------------

/// The gradient in a tile of `size` (css-images 3): its line or rays, and
/// its stops placed along them, ready for Skia.
pub(crate) fn gradient_shader(
    style: &ComputedStyle,
    gradient: &Gradient,
    size: SizeF,
) -> GradientShader {
    let (w, h) = (size.width, size.height);
    match gradient.kind {
        GradientKind::Linear(direction) => {
            // The direction as an angle, clockwise from up: a corner's is
            // perpendicular to the diagonal between the two corners next
            // to it, into its quadrant (Blink turns it into an angle too,
            // so a corner and its angle draw the same pixels).
            let degrees = match direction {
                LinearDirection::Angle(degrees) => f64::from(degrees),
                LinearDirection::To(x, y) => {
                    let sx = match x {
                        Some(HorizontalSide::Left) => -1.0,
                        Some(HorizontalSide::Right) => 1.0,
                        None => 0.0,
                    };
                    let sy = match y {
                        Some(VerticalSide::Top) => -1.0,
                        Some(VerticalSide::Bottom) => 1.0,
                        None => 0.0,
                    };
                    let (vx, vy) = if sx != 0.0 && sy != 0.0 {
                        (sx * f64::from(h), sy * f64::from(w))
                    } else {
                        (sx, sy)
                    };
                    vx.atan2(-vy).to_degrees()
                }
            };
            let radians = degrees.to_radians();
            #[expect(
                clippy::cast_possible_truncation,
                reason = "a sine and a cosine, within ±1: an f32 holds them"
            )]
            let (dx, dy) = (radians.sin() as f32, -radians.cos() as f32);
            // The line through the center, as long as it takes for the
            // perpendiculars at its ends to touch the corners.
            let length = (w * dx).abs() + (h * dy).abs();
            let center = PointF::new(w / 2.0, h / 2.0);
            let start = PointF::new(center.x - dx * length / 2.0, center.y - dy * length / 2.0);
            let (stops, from, to) = place_stops(style, gradient, length, false);
            let at = |t: f32| PointF::new(start.x + dx * t, start.y + dy * t);
            GradientShader {
                kind: GradientShaderKind::Linear {
                    start: at(from),
                    end: at(to),
                },
                stops,
                tile_mode: tile_mode(gradient),
                transform: Transform::IDENTITY,
                interpolation: ColorInterpolation::Premultiplied,
            }
        }
        GradientKind::Radial {
            shape,
            size: extent,
            position,
        } => {
            let center = PointF::new(position.x.resolve(w), position.y.resolve(h));
            let (rx, ry) = radii(shape, extent, center, size);
            // A degenerate shape is drawn as a tiny one: the last color
            // everywhere, as css-images asks.
            const TINY: f32 = 1e-3;
            let (rx, ry) = (rx.max(TINY), ry.max(TINY));
            let (stops, from, to) = place_stops(style, gradient, rx, true);
            // An ellipse: the circle scaled vertically about its center.
            let ellipse = Transform::translate(center.x, center.y)
                .then(&Transform::scale(1.0, ry / rx))
                .then(&Transform::translate(-center.x, -center.y));
            GradientShader {
                kind: GradientShaderKind::TwoPointConical {
                    start: center,
                    start_radius: from,
                    end: center,
                    end_radius: to,
                },
                stops,
                tile_mode: tile_mode(gradient),
                transform: ellipse,
                interpolation: ColorInterpolation::Premultiplied,
            }
        }
    }
}

/// `repeating-*`'s stops repeat; the others' end colors extend.
fn tile_mode(gradient: &Gradient) -> TileMode {
    if gradient.repeating {
        TileMode::Repeat
    } else {
        TileMode::Clamp
    }
}

/// A radial gradient's ending shape's radii (css-images 3, "radial-size").
fn radii(shape: RadialShape, size: RadialSize, center: PointF, area: SizeF) -> (f32, f32) {
    let (w, h) = (area.width, area.height);
    let (left, right, top, bottom) = (
        center.x.abs(),
        (w - center.x).abs(),
        center.y.abs(),
        (h - center.y).abs(),
    );
    let side = |closest: bool| {
        let pick = |a: f32, b: f32| if closest { a.min(b) } else { a.max(b) };
        (pick(left, right), pick(top, bottom))
    };
    match size {
        RadialSize::Circle(r) => (r, r),
        RadialSize::Ellipse(x, y) => (resolve(x, w), resolve(y, h)),
        RadialSize::ClosestSide | RadialSize::FarthestSide => {
            let (x, y) = side(size == RadialSize::ClosestSide);
            match shape {
                RadialShape::Circle => {
                    let r = if size == RadialSize::ClosestSide {
                        x.min(y)
                    } else {
                        x.max(y)
                    };
                    (r, r)
                }
                RadialShape::Ellipse => (x, y),
            }
        }
        RadialSize::ClosestCorner | RadialSize::FarthestCorner => {
            let closest = size == RadialSize::ClosestCorner;
            // The corner's offsets from the center: the nearest (or
            // farthest) side on each axis.
            let (dx, dy) = side(closest);
            match shape {
                RadialShape::Circle => {
                    let r = (dx * dx + dy * dy).sqrt();
                    (r, r)
                }
                // Through the corner, with the ratio the same keyword's
                // side would give.
                RadialShape::Ellipse => {
                    if dx == 0.0 || dy == 0.0 {
                        return (0.0, 0.0);
                    }
                    let scale = std::f32::consts::SQRT_2;
                    (dx * scale, dy * scale)
                }
            }
        }
    }
}

/// The stops placed along a line of `length` px (css-images 3, "color
/// stop fixup"), hints turned into stops, and offsets made 0 to 1 for
/// Skia: returns them with where on the line offsets 0 and 1 are. A
/// radial gradient (`radial`) can't start before its center: a
/// non-repeating one starts at the color there.
fn place_stops(
    style: &ComputedStyle,
    gradient: &Gradient,
    length: f32,
    radial: bool,
) -> (Vec<GradientStop>, f32, f32) {
    // Each stop's color and position, `None` until fixed up; each hint
    // after the stop before it.
    let mut stops: Vec<(Color, Option<f32>)> = Vec::new();
    let mut hints: Vec<(usize, f32)> = Vec::new();
    for item in gradient.items.iter() {
        match *item {
            GradientItem::Stop(color, position) => {
                stops.push((style.resolve(color), position.map(|p| resolve(p, length))));
            }
            GradientItem::Hint(position) => {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "a hint comes after a stop (the gradient's grammar)"
                )]
                let after = stops.len() - 1;
                hints.push((after, resolve(position, length)))
            }
        }
    }
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "a gradient has two stops at least (its grammar)"
    )]
    let last = stops.len() - 1;
    if stops[0].1.is_none() {
        stops[0].1 = Some(0.0);
    }
    if stops[last].1.is_none() {
        stops[last].1 = Some(length);
    }
    // Never before an earlier one.
    let mut max = f32::MIN;
    for stop in &mut stops {
        if let Some(p) = &mut stop.1 {
            *p = p.max(max);
            max = *p;
        }
    }
    spread_evenly(&mut stops);
    let mut placed: Vec<(f32, Color)> = Vec::new();
    for (i, &(color, position)) in stops.iter().enumerate() {
        placed.push((position.unwrap(), color));
        if let Some(&(_, hint)) = hints.iter().find(|(after, _)| *after == i) {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "a hint is between two stops (the gradient's grammar)"
            )]
            let next = i + 1;
            let (next_color, next) = (stops[next].0, stops[next].1.unwrap());
            placed.extend(hint_stops(position.unwrap(), color, next, next_color, hint));
        }
    }
    normalize(placed, length, gradient.repeating, radial)
}

/// The stops a color hint between `(p0, c0)` and `(p1, c1)` at `hint`
/// stands for: the curve css-images gives (the color halfway at the hint)
/// sampled at nine points, as Blink approximates it
/// (`CSSGradientValue::ReplaceColorHintsWithColorStops`).
fn hint_stops(p0: f32, c0: Color, p1: f32, c1: Color, hint: f32) -> Vec<(f32, Color)> {
    let span = p1 - p0;
    if span <= 0.0 {
        return Vec::new();
    }
    let h = ((hint - p0) / span).clamp(0.0, 1.0);
    if h <= 0.0 {
        // The next color from the start.
        return vec![(p0, c1)];
    }
    if h >= 1.0 {
        return vec![(p1, c0)];
    }
    if (h - 0.5).abs() < 1e-6 {
        return Vec::new();
    }
    let exponent = 0.5f32.ln() / h.ln();
    (1..8)
        .map(|k| {
            let x = k as f32 / 8.0;
            (p0 + x * span, mix(c0, c1, x.powf(exponent)))
        })
        .collect()
}

/// `a` to `b` at `t`, premultiplied, as the gradient itself interpolates.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let premul = |c: Color| {
        let alpha = c.a as f32 / 255.0;
        [
            c.r as f32 * alpha,
            c.g as f32 * alpha,
            c.b as f32 * alpha,
            c.a as f32,
        ]
    };
    let (pa, pb) = (premul(a), premul(b));
    let v: Vec<f32> = (0..4).map(|i| pa[i] + (pb[i] - pa[i]) * t).collect();
    let alpha = v[3] / 255.0;
    let channel = |x: f32| {
        if alpha > 0.0 {
            clamp_round::<u8>(x / alpha)
        } else {
            0
        }
    };
    Color::rgba(
        channel(v[0]),
        channel(v[1]),
        channel(v[2]),
        clamp_round::<u8>(v[3]),
    )
}

/// The stops with no position spread evenly between their neighbours: the
/// first and the last have one.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "indices into `stops`: the first and the last stops have positions, so a run \
              without one has a stop before it (start >= 0) and after it (end < len)"
)]
fn spread_evenly(stops: &mut [(Color, Option<f32>)]) {
    let mut i = 0;
    while i < stops.len() {
        if stops[i].1.is_some() {
            i += 1;
            continue;
        }
        let start = i - 1;
        let mut end = i;
        while stops[end].1.is_none() {
            end += 1;
        }
        let (a, b) = (stops[start].1.unwrap(), stops[end].1.unwrap());
        let steps = (end - start) as f32;
        for (k, stop) in stops[start + 1..end].iter_mut().enumerate() {
            stop.1 = Some(a + (b - a) * (k + 1) as f32 / steps);
        }
        i = end;
    }
}

/// Placed stops (in order) as Skia takes them: offsets 0 to 1 between the
/// first and the last, with where those are on the line.
fn normalize(
    mut placed: Vec<(f32, Color)>,
    length: f32,
    repeating: bool,
    radial: bool,
) -> (Vec<GradientStop>, f32, f32) {
    let first = placed[0].0;
    let (last, last_color) = *placed.last().expect("a gradient has stops");
    let span = last - first;
    if repeating && span <= 1e-4 {
        // Nothing to repeat: the last color everywhere (css-images 4,
        // "color stop fixup"; WPT's `normalization-*-degenerate`).
        let color = last_color;
        let stops = vec![
            GradientStop { offset: 0.0, color },
            GradientStop { offset: 1.0, color },
        ];
        return (stops, 0.0, 1.0);
    }
    if radial {
        if repeating {
            // Rings can't start inside the center: move the pattern out
            // by whole periods, which repeats it the same.
            if first < 0.0 {
                let shift = (-first / span).ceil() * span;
                for stop in &mut placed {
                    stop.0 += shift;
                }
            }
        } else if first < 0.0 {
            // What is before the center isn't seen: start at its color.
            let at_center = color_at(&placed, 0.0);
            placed.retain(|(p, _)| *p > 0.0);
            placed.insert(0, (0.0, at_center));
            if placed.len() == 1 {
                placed.push((0.0, at_center));
            }
        }
    }
    // A repeating gradient's line is its period, from the first stop to
    // the last. Another's is its own line, longer if the stops are
    // outside it: never shorter, as a line much shorter than a pixel (the
    // stops all at one place) makes the side of a sharp edge a pixel is
    // on a matter of rounding.
    let (mut first, mut last) = (placed[0].0, placed.last().expect("a gradient has stops").0);
    if !repeating {
        first = first.min(0.0);
        last = last.max(length);
    }
    let span = (last - first).max(1e-3);
    let stops = placed
        .iter()
        .map(|&(p, color)| GradientStop {
            offset: ((p - first) / span).clamp(0.0, 1.0),
            color,
        })
        .collect();
    (stops, first, first + span)
}

/// The color the placed stops give at `position`.
fn color_at(placed: &[(f32, Color)], position: f32) -> Color {
    let after = placed.iter().position(|(p, _)| *p >= position);
    match after {
        None => placed.last().expect("a gradient has stops").1,
        Some(0) => placed[0].1,
        Some(i) => {
            #[expect(clippy::arithmetic_side_effects, reason = "i > 0: matched above")]
            let before = i - 1;
            let (p0, c0) = placed[before];
            let (p1, c1) = placed[i];
            if p1 <= p0 {
                c1
            } else {
                mix(c0, c1, (position - p0) / (p1 - p0))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use computed_style::{PositionComponent, StyleColor};

    use super::*;

    fn px(v: f32) -> LengthPercentageOrAuto {
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v))
    }

    #[test]
    fn sizes_keep_the_ratio_unless_both_are_given() {
        let natural = Natural::of_size(SizeF::new(40.0, 20.0));
        let area = SizeF::new(100.0, 100.0);
        let auto = LengthPercentageOrAuto::Auto;
        let size = |w, h| tile_size(BackgroundSize::Explicit(w, h), natural, area);
        assert_eq!(size(auto, auto), SizeF::new(40.0, 20.0));
        assert_eq!(size(px(80.0), auto), SizeF::new(80.0, 40.0));
        assert_eq!(size(auto, px(10.0)), SizeF::new(20.0, 10.0));
        assert_eq!(size(px(10.0), px(30.0)), SizeF::new(10.0, 30.0));
        assert_eq!(
            tile_size(BackgroundSize::Cover, natural, area),
            SizeF::new(200.0, 100.0)
        );
        assert_eq!(
            tile_size(BackgroundSize::Contain, natural, area),
            SizeF::new(100.0, 50.0)
        );
        // A gradient has no natural size: the area's.
        assert_eq!(
            tile_size(
                BackgroundSize::Explicit(px(10.0), auto),
                Natural::default(),
                area
            ),
            SizeF::new(10.0, 100.0)
        );
        assert_eq!(
            tile_size(BackgroundSize::Cover, Natural::default(), area),
            area
        );
    }

    /// An SVG image's: one natural dimension, a ratio, or neither (css-images
    /// 3's default sizing algorithm, as Chrome 152 sizes them).
    #[test]
    fn sizes_from_what_an_svg_image_has() {
        let area = SizeF::new(60.0, 30.0);
        let auto = LengthPercentageOrAuto::Auto;
        let natural = |width, height, ratio| Natural {
            width,
            height,
            ratio,
        };
        let size = |n, w, h| tile_size(BackgroundSize::Explicit(w, h), n, area);
        // A ratio alone: contained.
        let square = natural(None, None, Some(1.0));
        assert_eq!(size(square, auto, auto), SizeF::new(30.0, 30.0));
        assert_eq!(size(square, px(10.0), auto), SizeF::new(10.0, 10.0));
        // Neither: the area's, or what is given of it.
        let none = natural(None, None, None);
        assert_eq!(size(none, auto, auto), area);
        assert_eq!(size(none, px(10.0), auto), SizeF::new(10.0, 30.0));
        // One dimension: the other through the ratio, else the area's.
        assert_eq!(
            size(natural(Some(20.0), None, Some(2.0)), auto, auto),
            SizeF::new(20.0, 10.0)
        );
        assert_eq!(
            size(natural(None, Some(20.0), Some(2.0)), auto, auto),
            SizeF::new(40.0, 20.0)
        );
        assert_eq!(
            size(natural(Some(20.0), None, None), auto, auto),
            SizeF::new(20.0, 30.0)
        );
        assert_eq!(
            tile_size(BackgroundSize::Cover, square, area),
            SizeF::new(60.0, 60.0)
        );
    }

    #[test]
    fn space_and_round_fit_whole_tiles() {
        let start = PositionComponent::START;
        let space = axis(BackgroundRepeat::Space, 30.0, 5.0, 100.0, start);
        // Three fit: 10px left over, shared by the two gaps.
        assert_eq!(
            (space.start, space.spacing, space.repeats),
            (5.0, 5.0, true)
        );
        let one = axis(
            BackgroundRepeat::Space,
            60.0,
            0.0,
            100.0,
            PositionComponent::CENTER,
        );
        assert_eq!((one.start, one.repeats), (20.0, false));
        assert_eq!(rounded(30.0, 100.0), 100.0 / 3.0);
        // 2.5 tiles: three, not two.
        assert_eq!(rounded(40.0, 100.0), 100.0 / 3.0);
        assert_eq!(rounded(80.0, 100.0), 100.0);
        assert_eq!(rounded(300.0, 100.0), 100.0);
    }

    /// Every image 20x10.
    struct Images;

    impl BackgroundImages for Images {
        fn image(&self, _: &str) -> Option<(ImageData, Size)> {
            Some((ImageData::new(&b"x"[..]), Size::new(20, 10)))
        }
    }

    fn image_layer(
        size: BackgroundSize,
        repeat: (BackgroundRepeat, BackgroundRepeat),
    ) -> BackgroundLayer {
        BackgroundLayer {
            image: computed_style::BackgroundImage::Url("i".into()),
            position_x: PositionComponent::START,
            position_y: PositionComponent::START,
            size,
            repeat,
            attachment: computed_style::BackgroundAttachment::Scroll,
            clip: computed_style::BackgroundBox::BorderBox,
            origin: computed_style::BackgroundBox::PaddingBox,
        }
    }

    #[test]
    fn tiles_are_whole_device_pixels() {
        let area = RectF::new(0.0, 0.0, 66.0, 10.0);
        let paint = |layer: &BackgroundLayer, dsf: f32| {
            layer_paint(&style(), layer, &Images, None, area, area, dsf).unwrap()
        };
        use BackgroundRepeat::*;
        // `round` across, the height kept in ratio: 22x11.
        let round = paint(&image_layer(BackgroundSize::AUTO, (Round, NoRepeat)), 1.0);
        assert_eq!(round.shader.tile, RectF::new(0.0, 0.0, 22.0, 11.0));
        // A fractional size: whole pixels, at least one.
        let thin = image_layer(
            BackgroundSize::Explicit(px(10.4), px(0.3)),
            (Repeat, Repeat),
        );
        assert_eq!(paint(&thin, 1.0).shader.tile.size(), SizeF::new(10.0, 1.0));
        // At 2x, half pixels.
        assert_eq!(paint(&thin, 2.0).shader.tile.size(), SizeF::new(10.5, 0.5));
    }

    fn style() -> ComputedStyle {
        ComputedStyle::initial()
    }

    fn gradient(kind: GradientKind, items: Vec<GradientItem>, repeating: bool) -> Gradient {
        Gradient {
            kind,
            repeating,
            items: items.into(),
        }
    }

    const RED: Color = Color::rgb(255, 0, 0);
    const BLUE: Color = Color::rgb(0, 0, 255);

    fn stop(color: Color, at: Option<LengthPercentage>) -> GradientItem {
        GradientItem::Stop(StyleColor::Rgba(color), at)
    }

    #[test]
    fn linear_gradients_reach_the_corners() {
        let to = |x, y| GradientKind::Linear(LinearDirection::To(x, y));
        let items = vec![stop(RED, None), stop(BLUE, None)];
        let size = SizeF::new(200.0, 100.0);
        let shader = gradient_shader(
            &style(),
            &gradient(to(None, Some(VerticalSide::Bottom)), items.clone(), false),
            size,
        );
        assert_eq!(
            shader.kind,
            GradientShaderKind::Linear {
                start: PointF::new(100.0, 0.0),
                end: PointF::new(100.0, 100.0)
            }
        );
        // CSS's gradients mix premultiplied, along their line as it is.
        assert_eq!(shader.interpolation, ColorInterpolation::Premultiplied);
        assert!(shader.transform.is_identity());
        assert_eq!(shader.tile_mode, TileMode::Clamp);
        let shader = gradient_shader(
            &style(),
            &gradient(
                GradientKind::Linear(LinearDirection::Angle(90.0)),
                items.clone(),
                false,
            ),
            size,
        );
        let GradientShaderKind::Linear { start, end } = shader.kind else {
            panic!()
        };
        assert!((start.x - 0.0).abs() < 1e-3 && (end.x - 200.0).abs() < 1e-3);
        // To a corner: the line is perpendicular to the other diagonal, so
        // the corner itself is where it ends.
        let shader = gradient_shader(
            &style(),
            &gradient(
                to(Some(HorizontalSide::Right), Some(VerticalSide::Bottom)),
                items,
                false,
            ),
            size,
        );
        let GradientShaderKind::Linear { start, end } = shader.kind else {
            panic!()
        };
        let (dx, dy) = (end.x - start.x, end.y - start.y);
        // Perpendicular to (200, -100).
        assert!((dx * 200.0 - dy * 100.0).abs() < 1e-2);
        // The corner (200, 100) projects onto the end.
        let along = |p: PointF| ((p.x - start.x) * dx + (p.y - start.y) * dy) / (dx * dx + dy * dy);
        assert!((along(PointF::new(200.0, 100.0)) - 1.0).abs() < 1e-4);
        assert!(along(PointF::new(0.0, 0.0)).abs() < 1e-4);
    }

    #[test]
    fn stops_are_fixed_up() {
        let pct = |v| Some(LengthPercentage::Percent(v));
        let green = Color::rgb(0, 128, 0);
        let g = gradient(
            GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom))),
            vec![
                stop(RED, pct(20.0)),
                stop(green, None),
                stop(green, None),
                // Before the one before it: moved up to it.
                stop(BLUE, pct(10.0)),
                stop(RED, None),
            ],
            false,
        );
        let (stops, from, to) = place_stops(&style(), &g, 100.0, false);
        let offsets: Vec<f32> = stops.iter().map(|s| s.offset).collect();
        // 20, 20, 20, 20, 100 px, on the gradient's own line.
        assert_eq!((from, to), (0.0, 100.0));
        assert_eq!(offsets, [0.2, 0.2, 0.2, 0.2, 1.0]);
        // Stops past the line's ends make it longer.
        let g = gradient(
            GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom))),
            vec![
                stop(RED, Some(LengthPercentage::Px(-50.0))),
                stop(BLUE, pct(150.0)),
            ],
            false,
        );
        let (stops, from, to) = place_stops(&style(), &g, 100.0, false);
        assert_eq!((from, to), (-50.0, 150.0));
        assert_eq!((stops[0].offset, stops[1].offset), (0.0, 1.0));
        let g = gradient(
            GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom))),
            vec![
                stop(RED, None),
                stop(Color::rgb(0, 0, 0), None),
                stop(BLUE, None),
            ],
            false,
        );
        let (stops, ..) = place_stops(&style(), &g, 100.0, false);
        assert_eq!(stops[1].offset, 0.5);
        // A hint off the middle becomes stops between its two.
        let g = gradient(
            GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom))),
            vec![
                stop(RED, None),
                GradientItem::Hint(LengthPercentage::Percent(25.0)),
                stop(BLUE, None),
            ],
            false,
        );
        let (stops, ..) = place_stops(&style(), &g, 100.0, false);
        assert_eq!(stops.len(), 9);
        // Halfway at the hint: the stop at 25% is half red, half blue.
        let quarter = stops
            .iter()
            .find(|s| (s.offset - 0.25).abs() < 1e-6)
            .unwrap();
        assert_eq!(quarter.color, Color::rgb(128, 0, 128));
    }

    #[test]
    fn repeating_gradients_repeat_their_stops() {
        let px = |v| Some(LengthPercentage::Px(v));
        let g = gradient(
            GradientKind::Radial {
                shape: RadialShape::Circle,
                size: RadialSize::Circle(50.0),
                position: computed_style::Position2D::CENTER,
            },
            vec![stop(RED, px(-10.0)), stop(BLUE, px(10.0))],
            true,
        );
        let shader = gradient_shader(&style(), &g, SizeF::new(100.0, 100.0));
        // Moved out by one period (20px).
        assert_eq!(
            shader.kind,
            GradientShaderKind::TwoPointConical {
                start: PointF::new(50.0, 50.0),
                start_radius: 10.0,
                end: PointF::new(50.0, 50.0),
                end_radius: 30.0,
            }
        );
        assert_eq!(shader.tile_mode, TileMode::Repeat);
        assert!(shader.transform.is_identity(), "a circle");
        // Stops all at one place: the last color.
        let g = gradient(
            GradientKind::Linear(LinearDirection::To(None, Some(VerticalSide::Bottom))),
            vec![stop(RED, px(10.0)), stop(BLUE, px(10.0))],
            true,
        );
        let (stops, ..) = place_stops(&style(), &g, 100.0, false);
        assert!(stops.iter().all(|s| s.color == BLUE));
    }

    #[test]
    fn radial_sizes() {
        let center = PointF::new(20.0, 30.0);
        let area = SizeF::new(100.0, 50.0);
        assert_eq!(
            radii(RadialShape::Ellipse, RadialSize::ClosestSide, center, area),
            (20.0, 20.0)
        );
        assert_eq!(
            radii(RadialShape::Circle, RadialSize::FarthestSide, center, area),
            (80.0, 80.0)
        );
        assert_eq!(
            radii(RadialShape::Circle, RadialSize::ClosestCorner, center, area),
            (800.0f32.sqrt(), 800.0f32.sqrt())
        );
        let (rx, ry) = radii(
            RadialShape::Ellipse,
            RadialSize::FarthestCorner,
            center,
            area,
        );
        assert!((rx - 80.0 * std::f32::consts::SQRT_2).abs() < 1e-3);
        assert!((ry - 30.0 * std::f32::consts::SQRT_2).abs() < 1e-3);
        assert_eq!(
            radii(
                RadialShape::Ellipse,
                RadialSize::Ellipse(LengthPercentage::Percent(50.0), LengthPercentage::Px(5.0)),
                center,
                area
            ),
            (50.0, 5.0)
        );
    }

    /// An ellipse is its circle (of the horizontal radius) scaled
    /// vertically about the center, by the gradient's transform.
    #[test]
    fn an_ellipse_is_a_scaled_circle() {
        let g = gradient(
            GradientKind::Radial {
                shape: RadialShape::Ellipse,
                size: RadialSize::FarthestSide,
                position: computed_style::Position2D::CENTER,
            },
            vec![stop(RED, None), stop(BLUE, None)],
            false,
        );
        let shader = gradient_shader(&style(), &g, SizeF::new(100.0, 50.0));
        let center = PointF::new(50.0, 25.0);
        assert_eq!(
            shader.kind,
            GradientShaderKind::TwoPointConical {
                start: center,
                start_radius: 0.0,
                end: center,
                end_radius: 50.0,
            }
        );
        assert_eq!(shader.transform.map_point(center), center);
        assert_eq!(
            shader.transform.map_point(PointF::new(50.0, 75.0)),
            PointF::new(50.0, 50.0)
        );
        assert_eq!(
            shader.transform.map_point(PointF::new(100.0, 25.0)),
            PointF::new(100.0, 25.0)
        );
        assert_eq!(shader.tile_mode, TileMode::Clamp);
        assert_eq!(shader.interpolation, ColorInterpolation::Premultiplied);
    }
}
