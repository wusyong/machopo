//! Animating computed values (web-animations 1, "Animation types";
//! css-values 4, "Combining Values"; Blink's `CSSInterpolationType`s):
//! each longhand's animation type, whether two styles have the same value
//! of it, and the value at a progress between them.
//!
//! Values are interpolated as computed: lengths in px, percentages kept
//! (a mix of both is a `calc()`), `currentcolor` resolved against each
//! style's own `color`. A pair that can't be interpolated (`auto` and a
//! length, say) is animated as a discrete step: the first value before
//! 50%, the second from then on.

use std::sync::Arc;

use base::numerics::{clamp_floor, clamp_round, f32_from_f64};
use computed_style::{
    BackgroundSize, BoxShadow, ComputedStyle, CornerRadius, FlexBasis, LengthPercentage,
    LengthPercentageOrAuto, LengthPercentageOrNone, LineHeight, PositionComponent, Size,
    StyleColor, SvgPaint, TransformOperation, VerticalAlign, Visibility,
};
use gfx::Color;

use crate::properties::LonghandId;

/// How a longhand animates (web-animations 1, "Animation types").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationType {
    /// Never animated: `display`, and the animation properties themselves.
    NotAnimatable,
    /// A discrete step at 50%: transitions only run with
    /// `transition-behavior: allow-discrete`.
    Discrete,
    /// Interpolated as its computed value, when the two values allow it.
    ByComputedValue,
}

impl LonghandId {
    /// Its animation type: as its physical longhand's for a flow-relative
    /// one. machopo's `display` isn't animated (css-display 4 animates it
    /// specially, with `@starting-style`), nor are the grid track lists,
    /// `d` and the background images, which Chrome interpolates in only
    /// some cases.
    pub fn animation_type(self) -> AnimationType {
        use LonghandId::*;
        match self.physical() {
            Display
            | TransitionProperty
            | TransitionDuration
            | TransitionTimingFunction
            | TransitionDelay
            | TransitionBehavior
            | AnimationDuration
            | AnimationTimingFunction
            | AnimationDelay
            | AnimationIterationCount
            | AnimationDirection
            | AnimationFillMode
            | AnimationPlayState
            | AnimationName
            | AnimationTimeline
            | AnimationRangeStart
            | AnimationRangeEnd
            // css-contain 2: not animatable, so keyframes leave it out.
            | Contain => AnimationType::NotAnimatable,
            Top
            | Right
            | Bottom
            | Left
            | Width
            | Height
            | MinWidth
            | MinHeight
            | MaxWidth
            | MaxHeight
            | MarginTop
            | MarginRight
            | MarginBottom
            | MarginLeft
            | PaddingTop
            | PaddingRight
            | PaddingBottom
            | PaddingLeft
            | BorderTopWidth
            | BorderRightWidth
            | BorderBottomWidth
            | BorderLeftWidth
            | BorderTopColor
            | BorderRightColor
            | BorderBottomColor
            | BorderLeftColor
            | BorderTopLeftRadius
            | BorderTopRightRadius
            | BorderBottomRightRadius
            | BorderBottomLeftRadius
            | BoxShadow
            | TextDecorationColor
            | TextDecorationThickness
            | TextUnderlineOffset
            | TextShadow
            | WebkitTextFillColor
            | Filter
            | OutlineColor
            | OutlineWidth
            | OutlineOffset
            | BackgroundColor
            | BackgroundPositionX
            | BackgroundPositionY
            | BackgroundSize
            | Opacity
            | Transform
            | TransformOrigin
            | ObjectPosition
            | Clip
            | ClipPath
            | FlexGrow
            | FlexShrink
            | FlexBasis
            | Order
            | RowGap
            | ColumnGap
            | ZIndex
            | Color
            | FontSize
            | FontWeight
            | LineHeight
            | VerticalAlign
            | LetterSpacing
            | WordSpacing
            | TabSize
            | TextIndent
            | Fill
            | FillOpacity
            | Stroke
            | StrokeOpacity
            | StrokeWidth
            | StrokeMiterlimit
            | StrokeDasharray
            | StrokeDashoffset
            | StopColor
            | StopOpacity
            | Cx
            | Cy
            | R
            | Rx
            | Ry
            | X
            | Y
            | Visibility
            | WebkitBorderHorizontalSpacing
            | WebkitBorderVerticalSpacing
            | WebkitTapHighlightColor
            | CaretColor
            | TextSizeAdjust
            | FontStretch
            | FontVariationSettings
            | FontSizeAdjust
            | BorderImageSlice
            | BorderImageWidth
            | BorderImageOutset => AnimationType::ByComputedValue,
            _ => AnimationType::Discrete,
        }
    }
}

/// What [`visit`] does with a longhand's two values.
enum Op<'a> {
    /// Whether they are the same.
    Same,
    /// Whether they interpolate (rather than step).
    Interpolable,
    /// Writes the value at a progress into the style.
    Write(f64, &'a mut ComputedStyle),
}

/// Whether `a` and `b` have the same computed value of `id`.
pub fn same_value(id: LonghandId, a: &ComputedStyle, b: &ComputedStyle) -> bool {
    visit(id, a, b, Op::Same)
}

/// Whether `id`'s values in `from` and `to` interpolate: `false` for a
/// discrete step (one not animated by computed value, or a pair that
/// can't be mixed).
pub fn interpolable(id: LonghandId, from: &ComputedStyle, to: &ComputedStyle) -> bool {
    visit(id, from, to, Op::Interpolable)
}

/// Sets `id` in `out` to its value at `progress` from `from` to `to`
/// (outside 0 to 1 too, as an easing function's output can be), clamped
/// to the values the property takes; a discrete step if they don't
/// interpolate.
pub fn interpolate(
    id: LonghandId,
    from: &ComputedStyle,
    to: &ComputedStyle,
    progress: f64,
    out: &mut ComputedStyle,
) {
    visit(id, from, to, Op::Write(progress, out));
}

/// The colors `currentcolor` stands for in each style.
#[derive(Clone, Copy)]
struct Context {
    from: Color,
    to: Color,
}

/// A computed value that interpolates with another: `None` if the pair
/// can't (it then steps).
trait Animate: Sized {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self>;
}

/// A value that is only ever a discrete step.
trait Discrete {}

impl<T: Discrete> Animate for T {
    fn animate(&self, _: &Self, _: f64, _: Context) -> Option<Self> {
        None
    }
}

macro_rules! discrete {
    ($($ty:ty),* $(,)?) => { $(impl Discrete for $ty {})* };
}

discrete!(
    computed_style::Cursor,
    computed_style::BoxSizing,
    computed_style::Position,
    computed_style::Float,
    computed_style::Clear,
    computed_style::Content,
    Arc<[computed_style::CounterReset]>,
    Arc<[computed_style::CounterChange]>,
    computed_style::ListStyleType,
    computed_style::ListStylePosition,
    computed_style::BorderStyle,
    computed_style::OutlineStyle,
    Arc<[computed_style::BackgroundImage]>,
    Arc<
        [(
            computed_style::BackgroundRepeat,
            computed_style::BackgroundRepeat
        )],
    >,
    Arc<[computed_style::BackgroundAttachment]>,
    Arc<[computed_style::BackgroundBox]>,
    computed_style::TextDecorationLine,
    computed_style::TextDecorationStyle,
    computed_style::TextUnderlinePosition,
    computed_style::Isolation,
    computed_style::Overflow,
    computed_style::FlexDirection,
    computed_style::FlexWrap,
    computed_style::ContentAlignment,
    computed_style::ItemAlignment,
    Option<computed_style::ItemAlignment>,
    computed_style::TrackList,
    Option<Arc<computed_style::GridTemplateAreas>>,
    Arc<[computed_style::TrackSize]>,
    computed_style::GridAutoFlow,
    computed_style::GridLine,
    Arc<[computed_style::FontFamily]>,
    computed_style::FontStyle,
    computed_style::TextAlign,
    computed_style::WhiteSpaceCollapse,
    computed_style::TextWrapMode,
    computed_style::TextTransform,
    computed_style::TextOverflow,
    computed_style::Appearance,
    computed_style::TextSecurity,
    computed_style::WordBreak,
    computed_style::OverflowWrap,
    gfx::FillRule,
    computed_style::StrokeLinecap,
    computed_style::StrokeLinejoin,
    computed_style::TextAnchor,
    Option<Arc<[computed_style::PathSegment]>>,
    computed_style::PointerEvents,
    computed_style::Display,
    computed_style::BorderCollapse,
    computed_style::TableLayout,
    computed_style::CaptionSide,
    computed_style::EmptyCells,
    computed_style::UserSelect,
    computed_style::UserModify,
    computed_style::Hyphens,
    computed_style::ForcedColorAdjust,
    computed_style::TextRendering,
    computed_style::FontSmoothing,
    computed_style::Resize,
    computed_style::ScrollBehavior,
    computed_style::OverflowAnchor,
    computed_style::FontVariantLigatures,
    computed_style::FontVariantNumeric,
    computed_style::FontVariantEastAsian,
    computed_style::FontVariantCaps,
    computed_style::FontVariantAlternates,
    computed_style::FontVariantPosition,
    computed_style::FontVariantEmoji,
    computed_style::FontKerning,
    Option<Arc<str>>,
    Arc<[computed_style::FontFeatureSetting]>,
    bool,
    computed_style::BackgroundImage,
    (
        computed_style::BorderImageRepeat,
        computed_style::BorderImageRepeat
    ),
    computed_style::TransformBox,
    computed_style::ObjectFit,
    computed_style::BoxOrient,
    computed_style::BoxDirection,
    computed_style::BoxPack,
    computed_style::BoxAlign,
    u32,
    Option<u32>,
    computed_style::Contain,
);

fn lerp(a: f64, b: f64, progress: f64) -> f64 {
    a + (b - a) * progress
}

impl Animate for f32 {
    fn animate(&self, other: &Self, progress: f64, _: Context) -> Option<Self> {
        Some(f32_from_f64(lerp(
            (*self).into(),
            (*other).into(),
            progress,
        )))
    }
}

/// css-values 4: integers interpolate as numbers, rounded to the nearest
/// (halves toward positive infinity).
impl Animate for i32 {
    fn animate(&self, other: &Self, progress: f64, _: Context) -> Option<Self> {
        Some(clamp_floor(
            lerp((*self).into(), (*other).into(), progress) + 0.5,
        ))
    }
}

/// `z-index`: `auto` doesn't interpolate.
impl Animate for Option<i32> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Some(a), Some(b)) => Some(a.animate(b, progress, cx)),
            _ => None,
        }
    }
}

/// `letter-spacing`: `normal` is 0 (css-text 3), but an end stays as
/// it is (Chrome's: `normal` at 0).
impl Animate for Option<f32> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        if self.is_none() && other.is_none() {
            return Some(None);
        }
        if progress == 0.0 {
            return Some(*self);
        }
        if progress == 1.0 {
            return Some(*other);
        }
        let (a, b) = (self.unwrap_or(0.0), other.unwrap_or(0.0));
        Some(a.animate(&b, progress, cx))
    }
}

/// `filter` (Blink's `FilterInterpolationFunctions`): function by
/// function, of the same kind in the same order, the shorter list (`none`
/// the empty one) padded with the longer's functions at their initial
/// values (filter-effects 1, "Interpolation of Filters"); each amount at
/// least 0, at most 1 where it stops there.
impl Animate for Arc<[computed_style::FilterFunction]> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::FilterFunction as F;
        fn initial(function: &F) -> F {
            match function {
                F::Blur(_) => F::Blur(0.0),
                F::Brightness(_) => F::Brightness(1.0),
                F::Contrast(_) => F::Contrast(1.0),
                F::DropShadow(shadow) => F::DropShadow(computed_style::BoxShadow {
                    x: 0.0,
                    y: 0.0,
                    blur: 0.0,
                    color: StyleColor::Rgba(Color::TRANSPARENT),
                    ..*shadow
                }),
                F::Grayscale(_) => F::Grayscale(0.0),
                F::HueRotate(_) => F::HueRotate(0.0),
                F::Invert(_) => F::Invert(0.0),
                F::Opacity(_) => F::Opacity(1.0),
                F::Saturate(_) => F::Saturate(1.0),
                F::Sepia(_) => F::Sepia(0.0),
            }
        }
        let n = self.len().max(other.len());
        (0..n)
            .map(|i| {
                let (a, b) = match (self.get(i), other.get(i)) {
                    (Some(a), Some(b)) => (*a, *b),
                    (Some(a), None) => (*a, initial(a)),
                    (None, Some(b)) => (initial(b), *b),
                    (None, None) => unreachable!("within the longer list"),
                };
                let amount =
                    |a: f32, b: f32, max: f32| Some((a.animate(&b, progress, cx)?).clamp(0.0, max));
                Some(match (a, b) {
                    (F::Blur(a), F::Blur(b)) => F::Blur(amount(a, b, f32::INFINITY)?),
                    (F::Brightness(a), F::Brightness(b)) => {
                        F::Brightness(amount(a, b, f32::INFINITY)?)
                    }
                    (F::Contrast(a), F::Contrast(b)) => F::Contrast(amount(a, b, f32::INFINITY)?),
                    (F::Saturate(a), F::Saturate(b)) => F::Saturate(amount(a, b, f32::INFINITY)?),
                    (F::Grayscale(a), F::Grayscale(b)) => F::Grayscale(amount(a, b, 1.0)?),
                    (F::Invert(a), F::Invert(b)) => F::Invert(amount(a, b, 1.0)?),
                    (F::Opacity(a), F::Opacity(b)) => F::Opacity(amount(a, b, 1.0)?),
                    (F::Sepia(a), F::Sepia(b)) => F::Sepia(amount(a, b, 1.0)?),
                    (F::HueRotate(a), F::HueRotate(b)) => {
                        F::HueRotate(a.animate(&b, progress, cx)?)
                    }
                    (F::DropShadow(a), F::DropShadow(b)) => {
                        let (a, b): (Arc<[computed_style::BoxShadow]>, Arc<[_]>) =
                            (Arc::from([a]), Arc::from([b]));
                        let shadows = a.animate(&b, progress, cx)?;
                        F::DropShadow(shadows[0])
                    }
                    _ => return None,
                })
            })
            .collect()
    }
}

/// `text-underline-offset`: `auto` doesn't interpolate.
impl Animate for Option<LengthPercentage> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Some(a), Some(b)) => Some(Some(a.animate(b, progress, cx)?)),
            (None, None) => Some(None),
            _ => None,
        }
    }
}

/// `text-decoration-thickness`: lengths only (Blink's
/// `CSSTextDecorationThicknessInterpolationType` takes `auto` and
/// `from-font` as steps).
impl Animate for computed_style::TextDecorationThickness {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::TextDecorationThickness::Length;
        match (self, other) {
            (Length(a), Length(b)) => Some(Length(a.animate(b, progress, cx)?)),
            _ => None,
        }
    }
}

/// px plus a percentage: the mix of a length and a percentage is a
/// `calc()` of both (`0%` and `20px` make `calc(0% + 20px)`).
impl Animate for LengthPercentage {
    fn animate(&self, other: &Self, progress: f64, _: Context) -> Option<Self> {
        Some(animate_length_percentage(self, other, progress, false))
    }
}

/// `a` to `b` at `progress`; with `zero_is_length`, a zero percentage is a
/// length (`translate(10px)`'s y is `0%`, and stays a length next to one).
fn animate_length_percentage(
    a: &LengthPercentage,
    b: &LengthPercentage,
    progress: f64,
    zero_is_length: bool,
) -> LengthPercentage {
    type Lp = LengthPercentage;
    // Each as px and a percentage, and whether it is a length alone.
    let parts = |v: &Lp| match *v {
        Lp::Px(px) => (px, 0.0, true),
        Lp::Percent(percent) => (0.0, percent, zero_is_length && percent == 0.0),
        Lp::Calc { px, percent, .. } => (px, percent, false),
    };
    let (a_px, a_percent, a_is_px) = parts(a);
    let (b_px, b_percent, b_is_px) = parts(b);
    let px = f32_from_f64(lerp(a_px.into(), b_px.into(), progress));
    let percent = f32_from_f64(lerp(a_percent.into(), b_percent.into(), progress));
    // Lengths make a length. A mix whose length comes to 0 is its
    // percentage alone, as Chrome simplifies it (`40%`, not `calc(40% +
    // 0px)`; a zero percentage stays: `calc(0% + 40px)`), which two
    // percentages always are.
    if a_is_px && b_is_px {
        Lp::Px(px)
    } else if px == 0.0 {
        Lp::Percent(percent)
    } else {
        Lp::Calc {
            px,
            percent,
            non_negative: false,
        }
    }
}

impl Animate for LengthPercentageOrAuto {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::LengthPercentage(a), Self::LengthPercentage(b)) => {
                Some(Self::LengthPercentage(a.animate(b, progress, cx)?))
            }
            _ => None,
        }
    }
}

impl Animate for LengthPercentageOrNone {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::LengthPercentage(a), Self::LengthPercentage(b)) => {
                Some(Self::LengthPercentage(a.animate(b, progress, cx)?))
            }
            _ => None,
        }
    }
}

impl Animate for Size {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::LengthPercentage(a), Self::LengthPercentage(b)) => {
                Some(Self::LengthPercentage(a.animate(b, progress, cx)?))
            }
            _ => None,
        }
    }
}

impl Animate for FlexBasis {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::Size(a), Self::Size(b)) => Some(Self::Size(a.animate(b, progress, cx)?)),
            _ => None,
        }
    }
}

impl Animate for CornerRadius {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        Some(Self {
            x: self.x.animate(&other.x, progress, cx)?,
            y: self.y.animate(&other.y, progress, cx)?,
        })
    }
}

/// Each side, if every side interpolates.
fn animate_sides<T: Copy>(
    a: &computed_style::Sides<T>,
    b: &computed_style::Sides<T>,
    side: impl Fn(&T, &T) -> Option<T>,
) -> Option<computed_style::Sides<T>> {
    Some(computed_style::Sides {
        top: side(&a.top, &b.top)?,
        right: side(&a.right, &b.right)?,
        bottom: side(&a.bottom, &b.bottom)?,
        left: side(&a.left, &b.left)?,
    })
}

/// `border-image-slice`: numbers with numbers and percentages with
/// percentages, side by side, the same `fill` (Blink's
/// `CSSBorderImageLengthBoxInterpolationType` and its kin).
impl Animate for computed_style::BorderImageSlice {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::NumberOrPercentage::*;
        if self.fill != other.fill {
            return None;
        }
        Some(Self {
            sides: animate_sides(&self.sides, &other.sides, |a, b| match (a, b) {
                (Number(a), Number(b)) => Some(Number(a.animate(b, progress, cx)?)),
                (Percent(a), Percent(b)) => Some(Percent(a.animate(b, progress, cx)?)),
                _ => None,
            })?,
            fill: self.fill,
        })
    }
}

/// `border-image-width`: numbers with numbers, lengths and percentages
/// with lengths and percentages, side by side; `auto` only with `auto`.
impl Animate for computed_style::Sides<computed_style::BorderImageWidth> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::BorderImageWidth::*;
        animate_sides(self, other, |a, b| match (a, b) {
            (Number(a), Number(b)) => Some(Number(a.animate(b, progress, cx)?)),
            (LengthPercentage(a), LengthPercentage(b)) => {
                Some(LengthPercentage(a.animate(b, progress, cx)?))
            }
            (Auto, Auto) => Some(Auto),
            _ => None,
        })
    }
}

/// `border-image-outset`: numbers with numbers, lengths with lengths.
impl Animate for computed_style::Sides<computed_style::BorderImageOutset> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::BorderImageOutset::*;
        animate_sides(self, other, |a, b| match (a, b) {
            (Number(a), Number(b)) => Some(Number(a.animate(b, progress, cx)?)),
            (Px(a), Px(b)) => Some(Px(a.animate(b, progress, cx)?)),
            _ => None,
        })
    }
}

/// `font-variation-settings`: the same axes, in the same order (Chrome's),
/// each value interpolated; `normal` only steps.
impl Animate for Arc<[computed_style::FontVariationSetting]> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        if self.len() != other.len()
            || self.is_empty()
            || self.iter().zip(other.iter()).any(|(a, b)| a.tag != b.tag)
        {
            return None;
        }
        self.iter()
            .zip(other.iter())
            .map(|(a, b)| {
                Some(computed_style::FontVariationSetting {
                    tag: a.tag,
                    value: a.value.animate(&b.value, progress, cx)?,
                })
            })
            .collect()
    }
}

/// `font-size-adjust`: numbers for the same metric; `none` and
/// `from-font` only step.
impl Animate for computed_style::FontSizeAdjust {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::FontSizeAdjust::Adjust;
        match (self, other) {
            (Adjust(a_metric, Some(a)), Adjust(b_metric, Some(b))) if a_metric == b_metric => {
                Some(Adjust(*a_metric, Some(a.animate(b, progress, cx)?)))
            }
            _ => None,
        }
    }
}

/// `text-size-adjust`: percentages with percentages; `auto` only steps.
impl Animate for computed_style::TextSizeAdjust {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::TextSizeAdjust::Percent;
        match (self, other) {
            (Percent(a), Percent(b)) => Some(Percent(a.animate(b, progress, cx)?)),
            _ => None,
        }
    }
}

/// `line-height`: numbers with numbers, lengths with lengths.
impl Animate for LineHeight {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => Some(Self::Number(a.animate(b, progress, cx)?)),
            (Self::Px(a), Self::Px(b)) => Some(Self::Px(a.animate(b, progress, cx)?)),
            _ => None,
        }
    }
}

/// `vertical-align`: lengths and percentages, `baseline` among them as
/// `0px` (Blink's `CSSLengthInterpolationType` for it).
impl Animate for VerticalAlign {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        let length = |v: &Self| match v {
            Self::Length(length) => Some(*length),
            Self::Baseline => Some(LengthPercentage::ZERO),
            _ => None,
        };
        let (a, b) = (length(self)?, length(other)?);
        Some(Self::Length(a.animate(&b, progress, cx)?))
    }
}

/// Colors mix with premultiplied alpha, in sRGB as Chrome does for the
/// legacy color syntaxes.
fn mix(a: Color, b: Color, progress: f64) -> Color {
    let premultiplied = |c: Color| {
        let alpha = f64::from(c.a) / 255.0;
        [
            f64::from(c.r) * alpha,
            f64::from(c.g) * alpha,
            f64::from(c.b) * alpha,
            alpha,
        ]
    };
    let (a, b) = (premultiplied(a), premultiplied(b));
    let alpha = lerp(a[3], b[3], progress).clamp(0.0, 1.0);
    if alpha == 0.0 {
        return Color::TRANSPARENT;
    }
    let channel = |i: usize| clamp_round::<u8>(lerp(a[i], b[i], progress) / alpha);
    Color {
        r: channel(0),
        g: channel(1),
        b: channel(2),
        a: clamp_round(alpha * 255.0),
    }
}

impl Animate for Color {
    fn animate(&self, other: &Self, progress: f64, _: Context) -> Option<Self> {
        Some(mix(*self, *other, progress))
    }
}

/// `currentcolor` is its style's `color`; two of it stay the keyword
/// (and so follow `color` as it animates).
impl Animate for StyleColor {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        let resolve = |c: &Self, current: Color| match c {
            Self::CurrentColor => current,
            Self::Rgba(color) => *color,
        };
        if *self == Self::CurrentColor && *other == Self::CurrentColor {
            return Some(Self::CurrentColor);
        }
        Some(Self::Rgba(mix(
            resolve(self, cx.from),
            resolve(other, cx.to),
            progress,
        )))
    }
}

/// `caret-color`: colors interpolate as colors; `auto` steps (css-ui 4).
impl Animate for computed_style::CaretColor {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::Color(a), Self::Color(b)) => Some(Self::Color(a.animate(b, progress, cx)?)),
            _ => None,
        }
    }
}

/// `fill`, `stroke`: colors interpolate, the other paints step.
impl Animate for SvgPaint {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::Color(a), Self::Color(b)) => Some(Self::Color(a.animate(b, progress, cx)?)),
            _ => None,
        }
    }
}

/// `visibility`: `visible` all the way between 0 and 1 if either end is
/// (web-animations 1), else a step.
impl Animate for Visibility {
    fn animate(&self, other: &Self, progress: f64, _: Context) -> Option<Self> {
        if *self != Self::Visible && *other != Self::Visible {
            return None;
        }
        Some(if progress <= 0.0 {
            *self
        } else if progress >= 1.0 {
            *other
        } else {
            Self::Visible
        })
    }
}

/// `box-shadow`: shadow by shadow, the shorter list padded with
/// transparent zero shadows; one `inset` against one not steps.
impl Animate for Arc<[BoxShadow]> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        let n = self.len().max(other.len());
        let none = |like: &BoxShadow| BoxShadow {
            x: 0.0,
            y: 0.0,
            blur: 0.0,
            spread: 0.0,
            color: StyleColor::Rgba(Color::TRANSPARENT),
            inset: like.inset,
        };
        (0..n)
            .map(|i| {
                let (a, b) = match (self.get(i), other.get(i)) {
                    (Some(a), Some(b)) => (*a, *b),
                    (Some(a), None) => (*a, none(a)),
                    (None, Some(b)) => (none(b), *b),
                    (None, None) => unreachable!("within the longer list"),
                };
                if a.inset != b.inset {
                    return None;
                }
                Some(BoxShadow {
                    x: a.x.animate(&b.x, progress, cx)?,
                    y: a.y.animate(&b.y, progress, cx)?,
                    blur: a.blur.animate(&b.blur, progress, cx)?.max(0.0),
                    spread: a.spread.animate(&b.spread, progress, cx)?,
                    color: a.color.animate(&b.color, progress, cx)?,
                    inset: a.inset,
                })
            })
            .collect()
    }
}

/// Two lists item by item, each repeated to their least common multiple
/// in length (web-animations 1, "repeatable list"); an empty one doesn't
/// interpolate unless both are.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "both lists have items (checked first), so their gcd isn't 0; their least \
              common multiple is at most the product of two lengths of lists in memory, \
              which a 64-bit usize holds"
)]
fn repeatable_list<T: Animate>(a: &[T], b: &[T], progress: f64, cx: Context) -> Option<Arc<[T]>> {
    if a.is_empty() || b.is_empty() {
        return (a.is_empty() && b.is_empty()).then(|| Arc::new([]) as Arc<[T]>);
    }
    let gcd = |mut x: usize, mut y: usize| {
        while y != 0 {
            (x, y) = (y, x % y);
        }
        x
    };
    let n = a.len() / gcd(a.len(), b.len()) * b.len();
    (0..n)
        .map(|i| a[i % a.len()].animate(&b[i % b.len()], progress, cx))
        .collect()
}

/// A `background-position` component, from the end or not: as an
/// offset from the start (`right 10px` is `calc(100% - 10px)`).
impl Animate for PositionComponent {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        let from_start = |c: &Self| {
            if !c.from_end {
                return c.offset;
            }
            match c.offset {
                LengthPercentage::Px(px) => LengthPercentage::Calc {
                    px: -px,
                    percent: 100.0,
                    non_negative: false,
                },
                LengthPercentage::Percent(percent) => LengthPercentage::Percent(100.0 - percent),
                LengthPercentage::Calc { px, percent, .. } => LengthPercentage::Calc {
                    px: -px,
                    percent: 100.0 - percent,
                    non_negative: false,
                },
            }
        };
        Some(Self {
            from_end: false,
            offset: from_start(self).animate(&from_start(other), progress, cx)?,
        })
    }
}

impl Animate for BackgroundSize {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        match (self, other) {
            (Self::Explicit(aw, ah), Self::Explicit(bw, bh)) => {
                let axis = |a: &LengthPercentageOrAuto, b: &LengthPercentageOrAuto| match (a, b) {
                    (LengthPercentageOrAuto::Auto, LengthPercentageOrAuto::Auto) => {
                        Some(LengthPercentageOrAuto::Auto)
                    }
                    _ => a.animate(b, progress, cx),
                };
                Some(Self::Explicit(axis(aw, bw)?, axis(ah, bh)?))
            }
            // The same keyword (`contain`, `cover`) on both sides stays:
            // the rest of the list interpolates.
            _ if self == other => Some(*self),
            _ => None,
        }
    }
}

macro_rules! repeatable {
    ($($ty:ty),*) => {$(
        impl Animate for Arc<[$ty]> {
            fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
                repeatable_list(self, other, progress, cx)
            }
        }
    )*};
}

repeatable!(PositionComponent, BackgroundSize, LengthPercentage);

/// `transform` (css-transforms 2, "Interpolation of Transforms"; Blink's
/// `TransformOperations::Blend`): function by function over the longest
/// prefix where both lists have the same primitive (`none`, or the end of
/// the shorter list, stands for the identity of the other's), the rest as
/// one matrix each, decomposed and the parts interpolated. A rest that
/// needs the box for its matrix (a percentage) is kept as
/// `Interpolated`, resolved when the box is known.
impl Animate for Arc<[TransformOperation]> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        if self.is_empty() && other.is_empty() {
            return Some(self.clone());
        }
        let prefix = matching_prefix(self, other);
        // Only room asked for: the prefix and the interpolated rest.
        let mut out = Vec::with_capacity(prefix.saturating_add(1));
        for i in 0..prefix {
            let (a, b) = match (self.get(i), other.get(i)) {
                (Some(a), Some(b)) => (a.clone(), b.clone()),
                (Some(a), None) => (a.clone(), identity_of(a)?),
                (None, Some(b)) => (identity_of(b)?, b.clone()),
                (None, None) => unreachable!("the prefix is within the longer list"),
            };
            out.push(a.animate(&b, progress, cx)?);
        }
        let (from, to) = (rest(self, prefix), rest(other, prefix));
        if !from.is_empty() || !to.is_empty() {
            if from.iter().chain(to.iter()).any(depends_on_box) {
                out.push(TransformOperation::Interpolated {
                    from: from.into(),
                    to: to.into(),
                    progress: f32_from_f64(progress),
                });
            } else {
                let none = gfx::SizeF::default();
                let (a, b) = (
                    computed_style::transform_list_matrix(from, none),
                    computed_style::transform_list_matrix(to, none),
                );
                out.push(TransformOperation::Matrix(
                    gfx::Transform::blend(&a, &b, progress)?.0,
                ));
            }
        }
        Some(out.into())
    }
}

/// How many functions from the start interpolate one by one: all of the
/// longer list's if every pair of the shorter list's length has the same
/// primitive (the shorter list's end is then identities), else up to the
/// first pair that doesn't (Blink's `MatchingPrefixLength`).
fn matching_prefix(a: &[TransformOperation], b: &[TransformOperation]) -> usize {
    let shorter = a.len().min(b.len());
    match (0..shorter).find(|&i| !same_primitive(&a[i], &b[i])) {
        Some(i) => i,
        None if a
            .iter()
            .chain(b.iter())
            .skip(shorter)
            .all(|op| identity_of(op).is_some()) =>
        {
            a.len().max(b.len())
        }
        None => shorter,
    }
}

fn rest(list: &[TransformOperation], from: usize) -> &[TransformOperation] {
    list.get(from..).unwrap_or(&[])
}

/// Whether two functions interpolate one by one: the same primitive
/// (`rotate` and `rotateZ` are both `Rotate`; `translateX` a
/// `Translate`), but `skew`, `skewX` and `skewY` only each with its own
/// kind, as Chrome. An `Interpolated` only ever goes through a matrix.
fn same_primitive(a: &TransformOperation, b: &TransformOperation) -> bool {
    !matches!(a, TransformOperation::Interpolated { .. })
        && std::mem::discriminant(a) == std::mem::discriminant(b)
}

/// The identity function of `op`'s primitive, which stands for it where
/// the other list has none; `None` for an `Interpolated`.
fn identity_of(op: &TransformOperation) -> Option<TransformOperation> {
    Some(match op {
        TransformOperation::Translate(..) => {
            TransformOperation::Translate(LengthPercentage::ZERO, LengthPercentage::ZERO)
        }
        TransformOperation::Scale(..) => TransformOperation::Scale(1.0, 1.0),
        TransformOperation::Rotate(_) => TransformOperation::Rotate(0.0),
        TransformOperation::Skew(..) => TransformOperation::Skew(0.0, 0.0),
        TransformOperation::SkewX(_) => TransformOperation::SkewX(0.0),
        TransformOperation::SkewY(_) => TransformOperation::SkewY(0.0),
        TransformOperation::Matrix(_) => TransformOperation::Matrix(gfx::Transform::IDENTITY.0),
        TransformOperation::Interpolated { .. } => return None,
    })
}

/// Whether the function's matrix needs the box (a percentage).
fn depends_on_box(op: &TransformOperation) -> bool {
    let percent = |v: &LengthPercentage| match *v {
        LengthPercentage::Px(_) => false,
        LengthPercentage::Percent(p) | LengthPercentage::Calc { percent: p, .. } => p != 0.0,
    };
    match op {
        TransformOperation::Translate(x, y) => percent(x) || percent(y),
        TransformOperation::Interpolated { from, to, .. } => {
            from.iter().chain(to.iter()).any(depends_on_box)
        }
        _ => false,
    }
}

impl Animate for TransformOperation {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        Some(match (self, other) {
            (Self::Translate(ax, ay), Self::Translate(bx, by)) => Self::Translate(
                animate_length_percentage(ax, bx, progress, true),
                animate_length_percentage(ay, by, progress, true),
            ),
            (Self::Scale(ax, ay), Self::Scale(bx, by)) => {
                Self::Scale(ax.animate(bx, progress, cx)?, ay.animate(by, progress, cx)?)
            }
            // Angles as angles: a turn is a turn, not the same matrix.
            (Self::Rotate(a), Self::Rotate(b)) => Self::Rotate(a.animate(b, progress, cx)?),
            (Self::Skew(ax, ay), Self::Skew(bx, by)) => {
                Self::Skew(ax.animate(bx, progress, cx)?, ay.animate(by, progress, cx)?)
            }
            (Self::SkewX(a), Self::SkewX(b)) => Self::SkewX(a.animate(b, progress, cx)?),
            (Self::SkewY(a), Self::SkewY(b)) => Self::SkewY(a.animate(b, progress, cx)?),
            (Self::Matrix(a), Self::Matrix(b)) => Self::Matrix(
                gfx::Transform::blend(&gfx::Transform(*a), &gfx::Transform(*b), progress)?.0,
            ),
            _ => return None,
        })
    }
}

/// `tab-size`: numbers with numbers, lengths with lengths.
impl Animate for computed_style::TabSize {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::TabSize::{Px, Spaces};
        match (self, other) {
            (Spaces(a), Spaces(b)) => Some(Spaces(a.animate(b, progress, cx)?.max(0.0))),
            (Px(a), Px(b)) => Some(Px(a.animate(b, progress, cx)?.max(0.0))),
            _ => None,
        }
    }
}

/// `object-position`: each axis.
impl Animate for computed_style::Position2D {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        Some(Self {
            x: self.x.animate(&other.x, progress, cx)?,
            y: self.y.animate(&other.y, progress, cx)?,
        })
    }
}

/// `clip`: each edge, where both are lengths or both `auto` (Blink's
/// `CSSClipInterpolationType`).
impl Animate for Option<computed_style::ClipRect> {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        let (Some(a), Some(b)) = (self, other) else {
            return (self.is_none() && other.is_none()).then_some(None);
        };
        let mut edges = [None; 4];
        for (edge, (a, b)) in edges.iter_mut().zip(a.iter().zip(b)) {
            *edge = match (a, b) {
                (Some(a), Some(b)) => Some(a.animate(b, progress, cx)?),
                (None, None) => None,
                _ => return None,
            };
        }
        Some(Some(edges))
    }
}

/// `clip-path`: a shape with one of its kind on the same box (Blink's
/// `BasicShapeInterpolationFunctions`); anything else steps.
impl Animate for computed_style::ClipPath {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::ClipPath::*;
        match (self, other) {
            (None, None) => Some(None),
            (Shape(a, a_box), Shape(b, b_box)) if a_box == b_box => {
                Some(Shape(Arc::new(a.animate(b, progress, cx)?), *a_box))
            }
            _ => Option::None,
        }
    }
}

impl Animate for computed_style::BasicShape {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        use computed_style::BasicShape::*;
        let lp = |a: &LengthPercentage, b: &LengthPercentage| {
            animate_length_percentage(a, b, progress, false)
        };
        Some(match (self, other) {
            (
                Inset {
                    insets: a,
                    radii: ar,
                },
                Inset {
                    insets: b,
                    radii: br,
                },
            ) => Inset {
                insets: [0, 1, 2, 3].map(|i| lp(&a[i], &b[i])),
                radii: [0, 1, 2, 3].map(|i| {
                    (
                        non_negative(lp(&ar[i].0, &br[i].0)),
                        non_negative(lp(&ar[i].1, &br[i].1)),
                    )
                }),
            },
            (
                Circle {
                    radius: ar,
                    center: ac,
                },
                Circle {
                    radius: br,
                    center: bc,
                },
            ) => Circle {
                radius: animate_shape_radius(*ar, *br, progress)?,
                center: animate_center(*ac, *bc, progress, cx)?,
            },
            (
                Ellipse {
                    radii: ar,
                    center: ac,
                },
                Ellipse {
                    radii: br,
                    center: bc,
                },
            ) => Ellipse {
                radii: (
                    animate_shape_radius(ar.0, br.0, progress)?,
                    animate_shape_radius(ar.1, br.1, progress)?,
                ),
                center: animate_center(*ac, *bc, progress, cx)?,
            },
            (
                Polygon {
                    fill_rule: af,
                    round: around,
                    points: ap,
                },
                Polygon {
                    fill_rule: bf,
                    round: bround,
                    points: bp,
                },
            ) if af == bf && ap.len() == bp.len() => Polygon {
                fill_rule: *af,
                round: around.animate(bround, progress, cx)?.max(0.0),
                points: ap
                    .iter()
                    .zip(bp.iter())
                    .map(|((ax, ay), (bx, by))| (lp(ax, bx), lp(ay, by)))
                    .collect(),
            },
            (
                Path {
                    fill_rule: af,
                    segments: a,
                },
                Path {
                    fill_rule: bf,
                    segments: b,
                },
            ) if af == bf => Path {
                fill_rule: *af,
                segments: animate_path_data(a, b, progress)?,
            },
            _ => return Option::None,
        })
    }
}

/// Path data of the same commands, each absolute or relative as the
/// other, an arc's flags as the other's: each number interpolated (Blink's
/// `PathInterpolationFunctions`, whose paths of other shapes step).
fn animate_path_data(
    a: &[computed_style::PathSegment],
    b: &[computed_style::PathSegment],
    progress: f64,
) -> Option<Arc<[computed_style::PathSegment]>> {
    use computed_style::PathSegment::*;
    if a.len() != b.len() {
        return None;
    }
    let l = |a: f32, b: f32| f32_from_f64(lerp(a.into(), b.into(), progress));
    a.iter()
        .zip(b)
        .map(|(a, b)| {
            Some(match (*a, *b) {
                (
                    MoveTo { abs, x, y },
                    MoveTo {
                        abs: b_abs,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs => MoveTo {
                    abs,
                    x: l(x, bx),
                    y: l(y, by),
                },
                (
                    LineTo { abs, x, y },
                    LineTo {
                        abs: b_abs,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs => LineTo {
                    abs,
                    x: l(x, bx),
                    y: l(y, by),
                },
                (HorizontalLineTo { abs, x }, HorizontalLineTo { abs: b_abs, x: bx })
                    if abs == b_abs =>
                {
                    HorizontalLineTo { abs, x: l(x, bx) }
                }
                (VerticalLineTo { abs, y }, VerticalLineTo { abs: b_abs, y: by })
                    if abs == b_abs =>
                {
                    VerticalLineTo { abs, y: l(y, by) }
                }
                (
                    CurveTo {
                        abs,
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    },
                    CurveTo {
                        abs: b_abs,
                        x1: bx1,
                        y1: by1,
                        x2: bx2,
                        y2: by2,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs => CurveTo {
                    abs,
                    x1: l(x1, bx1),
                    y1: l(y1, by1),
                    x2: l(x2, bx2),
                    y2: l(y2, by2),
                    x: l(x, bx),
                    y: l(y, by),
                },
                (
                    SmoothCurveTo { abs, x2, y2, x, y },
                    SmoothCurveTo {
                        abs: b_abs,
                        x2: bx2,
                        y2: by2,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs => SmoothCurveTo {
                    abs,
                    x2: l(x2, bx2),
                    y2: l(y2, by2),
                    x: l(x, bx),
                    y: l(y, by),
                },
                (
                    Quadratic { abs, x1, y1, x, y },
                    Quadratic {
                        abs: b_abs,
                        x1: bx1,
                        y1: by1,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs => Quadratic {
                    abs,
                    x1: l(x1, bx1),
                    y1: l(y1, by1),
                    x: l(x, bx),
                    y: l(y, by),
                },
                (
                    SmoothQuadratic { abs, x, y },
                    SmoothQuadratic {
                        abs: b_abs,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs => SmoothQuadratic {
                    abs,
                    x: l(x, bx),
                    y: l(y, by),
                },
                (
                    EllipticalArc {
                        abs,
                        rx,
                        ry,
                        x_axis_rotation,
                        large_arc,
                        sweep,
                        x,
                        y,
                    },
                    EllipticalArc {
                        abs: b_abs,
                        rx: brx,
                        ry: bry,
                        x_axis_rotation: brot,
                        large_arc: b_large,
                        sweep: b_sweep,
                        x: bx,
                        y: by,
                    },
                ) if abs == b_abs && large_arc == b_large && sweep == b_sweep => EllipticalArc {
                    abs,
                    rx: l(rx, brx),
                    ry: l(ry, bry),
                    x_axis_rotation: l(x_axis_rotation, brot),
                    large_arc,
                    sweep,
                    x: l(x, bx),
                    y: l(y, by),
                },
                (ClosePath, ClosePath) => ClosePath,
                _ => return None,
            })
        })
        .collect()
}

/// Two centers, one not given the reference box's.
fn animate_center(
    a: Option<computed_style::Position2D>,
    b: Option<computed_style::Position2D>,
    progress: f64,
    cx: Context,
) -> Option<Option<computed_style::Position2D>> {
    if a.is_none() && b.is_none() {
        return Some(None);
    }
    let center = computed_style::Position2D::CENTER;
    Some(Some(a.unwrap_or(center).animate(
        &b.unwrap_or(center),
        progress,
        cx,
    )?))
}

/// Two `<shape-radius>`es: lengths interpolate; a keyword steps, even
/// with itself (Blink's `BasicShapeInterpolationFunctions` takes only
/// lengths).
fn animate_shape_radius(
    a: computed_style::ShapeRadius,
    b: computed_style::ShapeRadius,
    progress: f64,
) -> Option<computed_style::ShapeRadius> {
    use computed_style::ShapeRadius::Length;
    match (a, b) {
        (Length(a), Length(b)) => Some(Length(non_negative(animate_length_percentage(
            &a, &b, progress, false,
        )))),
        _ => None,
    }
}

/// `v`, never below 0 (an easing that overshoots can take a radius
/// there).
fn non_negative(v: LengthPercentage) -> LengthPercentage {
    match v {
        LengthPercentage::Px(px) => LengthPercentage::Px(px.max(0.0)),
        LengthPercentage::Percent(percent) => LengthPercentage::Percent(percent.max(0.0)),
        LengthPercentage::Calc { px, percent, .. } => LengthPercentage::Calc {
            px,
            percent,
            non_negative: true,
        },
    }
}

/// `transform-origin`: each length.
impl Animate for computed_style::TransformOrigin {
    fn animate(&self, other: &Self, progress: f64, cx: Context) -> Option<Self> {
        Some(Self {
            x: animate_length_percentage(&self.x, &other.x, progress, true),
            y: animate_length_percentage(&self.y, &other.y, progress, true),
            z: self.z.animate(&other.z, progress, cx)?,
        })
    }
}

/// The non-negative properties' values below 0 are 0 (an easing function
/// that overshoots can take them there); `opacity` and the like stay in
/// 0 to 1, `font-weight` in 1 to 1000.
fn clamp(id: LonghandId, style: &mut ComputedStyle) {
    use LonghandId::*;
    let lp = |v: &mut LengthPercentage| match v {
        LengthPercentage::Px(px) => *px = px.max(0.0),
        LengthPercentage::Percent(percent) => *percent = percent.max(0.0),
        LengthPercentage::Calc { non_negative, .. } => *non_negative = true,
    };
    let lp_or_auto = |v: &mut LengthPercentageOrAuto| {
        if let LengthPercentageOrAuto::LengthPercentage(v) = v {
            lp(v)
        }
    };
    match id {
        Width => {
            if let Size::LengthPercentage(v) = &mut style.width {
                lp(v)
            }
        }
        Height => {
            if let Size::LengthPercentage(v) = &mut style.height {
                lp(v)
            }
        }
        FlexBasis => {
            if let computed_style::FlexBasis::Size(Size::LengthPercentage(v)) =
                &mut style.flex_basis
            {
                lp(v)
            }
        }
        MinWidth => lp_or_auto(&mut style.min_width),
        MinHeight => lp_or_auto(&mut style.min_height),
        MaxWidth => {
            if let LengthPercentageOrNone::LengthPercentage(v) = &mut style.max_width {
                lp(v)
            }
        }
        MaxHeight => {
            if let LengthPercentageOrNone::LengthPercentage(v) = &mut style.max_height {
                lp(v)
            }
        }
        PaddingTop => lp(&mut style.padding.top),
        PaddingRight => lp(&mut style.padding.right),
        PaddingBottom => lp(&mut style.padding.bottom),
        PaddingLeft => lp(&mut style.padding.left),
        BorderTopWidth => style.border_width.top = style.border_width.top.max(0.0),
        BorderRightWidth => style.border_width.right = style.border_width.right.max(0.0),
        BorderBottomWidth => style.border_width.bottom = style.border_width.bottom.max(0.0),
        BorderLeftWidth => style.border_width.left = style.border_width.left.max(0.0),
        BorderTopLeftRadius
        | BorderTopRightRadius
        | BorderBottomRightRadius
        | BorderBottomLeftRadius => {
            let r = &mut style.rare_mut().border_radius;
            for corner in [
                &mut r.top_left,
                &mut r.top_right,
                &mut r.bottom_right,
                &mut r.bottom_left,
            ] {
                lp(&mut corner.x);
                lp(&mut corner.y);
            }
        }
        OutlineWidth => style.outline_width = style.outline_width.max(0.0),
        WebkitBorderHorizontalSpacing => {
            let spacing = &mut style.rare_inherited_mut().border_spacing;
            spacing.horizontal = spacing.horizontal.max(0.0);
        }
        WebkitBorderVerticalSpacing => {
            let spacing = &mut style.rare_inherited_mut().border_spacing;
            spacing.vertical = spacing.vertical.max(0.0);
        }
        BorderImageSlice | BorderImageWidth | BorderImageOutset => {
            use computed_style::{BorderImageOutset, BorderImageWidth, NumberOrPercentage};
            let image = &mut style.rare_mut().border_image;
            for side in image.slice.sides.iter_mut() {
                match side {
                    NumberOrPercentage::Number(v) | NumberOrPercentage::Percent(v) => {
                        *v = v.max(0.0)
                    }
                }
            }
            for side in image.width.iter_mut() {
                match side {
                    BorderImageWidth::Number(v) => *v = v.max(0.0),
                    BorderImageWidth::LengthPercentage(v) => lp(v),
                    BorderImageWidth::Auto => {}
                }
            }
            for side in image.outset.iter_mut() {
                match side {
                    BorderImageOutset::Number(v) | BorderImageOutset::Px(v) => *v = v.max(0.0),
                }
            }
        }
        TextSizeAdjust => {
            if let computed_style::TextSizeAdjust::Percent(percent) =
                &mut style.rare_inherited_mut().text_size_adjust
            {
                *percent = percent.max(0.0);
            }
        }
        Opacity => style.opacity = style.opacity.clamp(0.0, 1.0),
        FlexGrow => style.flex_grow = style.flex_grow.max(0.0),
        FlexShrink => style.flex_shrink = style.flex_shrink.max(0.0),
        RowGap => lp(&mut style.row_gap),
        ColumnGap => lp(&mut style.column_gap),
        FontSize => style.font_mut().size = style.font.size.max(0.0),
        FontStretch => style.font_mut().stretch = style.font.stretch.max(0.0),
        FontSizeAdjust => {
            if let computed_style::FontSizeAdjust::Adjust(_, Some(value)) =
                &mut style.font_mut().size_adjust
            {
                *value = value.max(0.0);
            }
        }
        FontWeight => style.font_mut().weight = style.font.weight.clamp(1.0, 1000.0),
        LineHeight => match &mut style.line_height {
            computed_style::LineHeight::Number(v) | computed_style::LineHeight::Px(v) => {
                *v = v.max(0.0)
            }
            computed_style::LineHeight::Normal => {}
        },
        FillOpacity => style.svg_mut().fill_opacity = style.svg.fill_opacity.clamp(0.0, 1.0),
        StrokeOpacity => style.svg_mut().stroke_opacity = style.svg.stroke_opacity.clamp(0.0, 1.0),
        StopOpacity => style.svg_mut().stop_opacity = style.svg.stop_opacity.clamp(0.0, 1.0),
        StrokeWidth => lp(&mut style.svg_mut().stroke_width),
        StrokeMiterlimit => {
            style.svg_mut().stroke_miterlimit = style.svg.stroke_miterlimit.max(1.0)
        }
        StrokeDasharray => {
            let mut dashes = style.svg.stroke_dasharray.to_vec();
            dashes.iter_mut().for_each(lp);
            style.svg_mut().stroke_dasharray = dashes.into();
        }
        R => lp(&mut style.svg_mut().r),
        Rx => lp_or_auto(&mut style.svg_mut().rx),
        Ry => lp_or_auto(&mut style.svg_mut().ry),
        BackgroundSize => {
            let mut sizes = style.backgrounds.background_size.to_vec();
            for size in &mut sizes {
                if let computed_style::BackgroundSize::Explicit(w, h) = size {
                    lp_or_auto(w);
                    lp_or_auto(h);
                }
            }
            style.backgrounds_mut().background_size = sizes.into();
        }
        _ => {}
    }
}

/// The two values of `id` in `from` and `to`, as `op` says. A
/// flow-relative longhand is its physical one's.
fn visit(id: LonghandId, from: &ComputedStyle, to: &ComputedStyle, op: Op<'_>) -> bool {
    use LonghandId::*;
    let cx = Context {
        from: from.color,
        to: to.color,
    };
    let physical = id.physical();
    // A field in one of the style's shared groups is written through its
    // own copy of the group (`rare_mut` and the like).
    macro_rules! field {
        (@ [$($read:tt)+] [$($write:tt)+]) => {{
            let (a, b) = (&from.$($read)+, &to.$($read)+);
            match op {
                Op::Same => a == b,
                Op::Interpolable => Animate::animate(a, b, 0.5, cx).is_some(),
                Op::Write(progress, out) => {
                    let interpolated = match Animate::animate(a, b, progress, cx) {
                        Some(value) => {
                            out.$($write)+ = value;
                            true
                        }
                        None => {
                            out.$($write)+ = if progress < 0.5 { a.clone() } else { b.clone() };
                            false
                        }
                    };
                    clamp(physical, out);
                    interpolated
                }
            }
        }};
        // A value that only steps, whatever its type.
        (@step [$($read:tt)+] [$($write:tt)+]) => {{
            let (a, b) = (&from.$($read)+, &to.$($read)+);
            match op {
                Op::Same => a == b,
                Op::Interpolable => false,
                Op::Write(progress, out) => {
                    out.$($write)+ = if progress < 0.5 { *a } else { *b };
                    false
                }
            }
        }};
        (rare . $($f:tt)+) => { field!(@ [rare . $($f)+] [rare_mut() . $($f)+]) };
        (font . $($f:tt)+) => { field!(@ [font . $($f)+] [font_mut() . $($f)+]) };
        (rare_inherited . $($f:tt)+) => {
            field!(@ [rare_inherited . $($f)+] [rare_inherited_mut() . $($f)+])
        };
        (backgrounds . $($f:tt)+) => {
            field!(@ [backgrounds . $($f)+] [backgrounds_mut() . $($f)+])
        };
        (svg . $($f:tt)+) => { field!(@ [svg . $($f)+] [svg_mut() . $($f)+]) };
        ($($f:tt)+) => { field!(@ [$($f)+] [$($f)+]) };
    }
    match physical {
        crate::properties::logical_longhands!() => unreachable!("mapped to a physical longhand"),
        Display => field!(display),
        BoxSizing => field!(box_sizing),
        Position => field!(position),
        Float => field!(float),
        Clear => field!(clear),
        Content => field!(rare.content),
        CounterReset => field!(rare.counter_reset),
        CounterIncrement => field!(rare.counter_increment),
        CounterSet => field!(rare.counter_set),
        Top => field!(inset.top),
        Right => field!(inset.right),
        Bottom => field!(inset.bottom),
        Left => field!(inset.left),
        Width => field!(width),
        Height => field!(height),
        MinWidth => field!(min_width),
        MinHeight => field!(min_height),
        MaxWidth => field!(max_width),
        MaxHeight => field!(max_height),
        MarginTop => field!(margin.top),
        MarginRight => field!(margin.right),
        MarginBottom => field!(margin.bottom),
        MarginLeft => field!(margin.left),
        PaddingTop => field!(padding.top),
        PaddingRight => field!(padding.right),
        PaddingBottom => field!(padding.bottom),
        PaddingLeft => field!(padding.left),
        BorderTopWidth => field!(border_width.top),
        BorderRightWidth => field!(border_width.right),
        BorderBottomWidth => field!(border_width.bottom),
        BorderLeftWidth => field!(border_width.left),
        BorderTopStyle => field!(border_style.top),
        BorderRightStyle => field!(border_style.right),
        BorderBottomStyle => field!(border_style.bottom),
        BorderLeftStyle => field!(border_style.left),
        BorderTopColor => field!(border_color.top),
        BorderRightColor => field!(border_color.right),
        BorderBottomColor => field!(border_color.bottom),
        BorderLeftColor => field!(border_color.left),
        BorderTopLeftRadius => field!(rare.border_radius.top_left),
        BorderTopRightRadius => field!(rare.border_radius.top_right),
        BorderBottomRightRadius => field!(rare.border_radius.bottom_right),
        BorderBottomLeftRadius => field!(rare.border_radius.bottom_left),
        BoxShadow => field!(box_shadow),
        OutlineColor => field!(outline_color),
        OutlineStyle => field!(outline_style),
        OutlineWidth => field!(outline_width),
        OutlineOffset => field!(outline_offset),
        BackgroundColor => field!(background_color),
        BackgroundImage => field!(backgrounds.background_image),
        BackgroundPositionX => field!(backgrounds.background_position_x),
        BackgroundPositionY => field!(backgrounds.background_position_y),
        BackgroundSize => field!(backgrounds.background_size),
        BackgroundRepeat => field!(backgrounds.background_repeat),
        BackgroundAttachment => field!(backgrounds.background_attachment),
        BackgroundClip => field!(backgrounds.background_clip),
        BackgroundOrigin => field!(backgrounds.background_origin),
        TextDecorationLine => field!(text_decoration_line),
        TextDecorationStyle => field!(rare.text_decoration_style),
        TextDecorationColor => field!(rare.text_decoration_color),
        TextDecorationThickness => field!(rare.text_decoration_thickness),
        TextUnderlineOffset => field!(rare_inherited.text_underline_offset),
        TextUnderlinePosition => field!(rare_inherited.text_underline_position),
        TextShadow => field!(rare_inherited.text_shadow),
        WebkitTextFillColor => field!(rare_inherited.text_fill_color),
        Filter => field!(rare.filter),
        Isolation => field!(rare.isolation),
        Opacity => field!(opacity),
        OverflowX => field!(overflow_x),
        OverflowY => field!(overflow_y),
        Transform => field!(transform),
        TransformOrigin => field!(rare.transform_origin),
        TransformBox => field!(rare.transform_box),
        ObjectFit => field!(rare.object_fit),
        ObjectPosition => field!(rare.object_position),
        Clip => field!(rare.clip),
        ClipPath => field!(rare.clip_path),
        WebkitBoxOrient => field!(rare.box_orient),
        WebkitBoxDirection => field!(rare.box_direction),
        WebkitBoxPack => field!(rare.box_pack),
        WebkitBoxAlign => field!(rare.box_align),
        // A number, but not animatable (Blink's): a discrete step.
        WebkitBoxFlex => field!(@step [rare.box_flex] [rare_mut().box_flex]),
        WebkitBoxOrdinalGroup => field!(rare.box_ordinal_group),
        WebkitLineClamp => field!(rare.line_clamp),
        Contain => field!(rare.contain),
        FlexDirection => field!(flex_direction),
        FlexWrap => field!(flex_wrap),
        FlexGrow => field!(flex_grow),
        FlexShrink => field!(flex_shrink),
        FlexBasis => field!(flex_basis),
        Order => field!(order),
        JustifyContent => field!(justify_content),
        AlignContent => field!(align_content),
        AlignItems => field!(align_items),
        AlignSelf => field!(align_self),
        RowGap => field!(row_gap),
        ColumnGap => field!(column_gap),
        ZIndex => field!(z_index),
        GridTemplateColumns => field!(rare.grid_template_columns),
        GridTemplateRows => field!(rare.grid_template_rows),
        GridAutoColumns => field!(rare.grid_auto_columns),
        GridAutoRows => field!(rare.grid_auto_rows),
        GridAutoFlow => field!(rare.grid_auto_flow),
        GridTemplateAreas => field!(rare.grid_template_areas),
        GridRowStart => field!(rare.grid_row_start),
        GridRowEnd => field!(rare.grid_row_end),
        GridColumnStart => field!(rare.grid_column_start),
        GridColumnEnd => field!(rare.grid_column_end),
        JustifyItems => field!(justify_items),
        JustifySelf => field!(justify_self),
        Color => field!(color),
        FontFamily => field!(font.family),
        FontSize => field!(font.size),
        FontWeight => field!(font.weight),
        FontStyle => field!(font.style),
        FontStretch => field!(font.stretch),
        FontVariantLigatures => field!(font.variant_ligatures),
        FontVariantNumeric => field!(font.variant_numeric),
        FontVariantEastAsian => field!(font.variant_east_asian),
        FontVariantCaps => field!(font.variant_caps),
        FontVariantAlternates => field!(font.variant_alternates),
        FontVariantPosition => field!(font.variant_position),
        FontVariantEmoji => field!(font.variant_emoji),
        FontSizeAdjust => field!(font.size_adjust),
        FontLanguageOverride => field!(font.language_override),
        FontKerning => field!(font.kerning),
        FontOpticalSizing => field!(font.optical_sizing),
        FontFeatureSettings => field!(font.feature_settings),
        FontVariationSettings => field!(font.variation_settings),
        FontSynthesisWeight => field!(font.synthesis_weight),
        FontSynthesisStyle => field!(font.synthesis_style),
        FontSynthesisSmallCaps => field!(font.synthesis_small_caps),
        LineHeight => field!(line_height),
        TextAlign => field!(text_align),
        VerticalAlign => field!(vertical_align),
        WhiteSpaceCollapse => field!(white_space_collapse),
        TextWrapMode => field!(text_wrap_mode),
        TextTransform => field!(text_transform),
        ListStyleType => field!(rare_inherited.list_style_type),
        ListStylePosition => field!(list_style_position),
        // Only `none`.
        ListStyleImage => matches!(op, Op::Same),
        TextOverflow => field!(text_overflow),
        Appearance => field!(appearance),
        LetterSpacing => field!(rare_inherited.letter_spacing),
        WordSpacing => field!(rare_inherited.word_spacing),
        TabSize => field!(rare_inherited.tab_size),
        TextIndent => field!(rare_inherited.text_indent),
        WordBreak => field!(rare_inherited.word_break),
        OverflowWrap => field!(rare_inherited.overflow_wrap),
        Fill => field!(svg.fill),
        FillOpacity => field!(svg.fill_opacity),
        FillRule => field!(svg.fill_rule),
        ClipRule => field!(svg.clip_rule),
        Stroke => field!(svg.stroke),
        StrokeOpacity => field!(svg.stroke_opacity),
        StrokeWidth => field!(svg.stroke_width),
        StrokeLinecap => field!(svg.stroke_linecap),
        StrokeLinejoin => field!(svg.stroke_linejoin),
        StrokeMiterlimit => field!(svg.stroke_miterlimit),
        StrokeDasharray => field!(svg.stroke_dasharray),
        StrokeDashoffset => field!(svg.stroke_dashoffset),
        TextAnchor => field!(svg.text_anchor),
        StopColor => field!(svg.stop_color),
        StopOpacity => field!(svg.stop_opacity),
        Cx => field!(svg.cx),
        Cy => field!(svg.cy),
        R => field!(svg.r),
        Rx => field!(svg.rx),
        Ry => field!(svg.ry),
        X => field!(svg.x),
        Y => field!(svg.y),
        D => field!(svg.d),
        Visibility => field!(visibility),
        WebkitTextSecurity => field!(rare_inherited.text_security),
        PointerEvents => field!(pointer_events),
        BorderCollapse => field!(border_collapse),
        WebkitBorderHorizontalSpacing => field!(rare_inherited.border_spacing.horizontal),
        WebkitBorderVerticalSpacing => field!(rare_inherited.border_spacing.vertical),
        TableLayout => field!(table_layout),
        CaptionSide => field!(caption_side),
        EmptyCells => field!(empty_cells),
        UserSelect => field!(rare_inherited.user_select),
        WebkitUserModify => field!(rare_inherited.user_modify),
        Hyphens => field!(rare_inherited.hyphens),
        ForcedColorAdjust => field!(rare_inherited.forced_color_adjust),
        WebkitTapHighlightColor => field!(rare_inherited.tap_highlight_color),
        CaretColor => field!(rare_inherited.caret_color),
        TextSizeAdjust => field!(rare_inherited.text_size_adjust),
        TextRendering => field!(font.text_rendering),
        WebkitFontSmoothing => field!(font.smoothing),
        Resize => field!(rare.resize),
        ScrollBehavior => field!(rare.scroll_behavior),
        OverflowAnchor => field!(rare.overflow_anchor),
        Cursor => field!(rare_inherited.cursor),
        BorderImageSource => field!(rare.border_image.source),
        BorderImageSlice => field!(rare.border_image.slice),
        BorderImageWidth => field!(rare.border_image.width),
        BorderImageOutset => field!(rare.border_image.outset),
        BorderImageRepeat => field!(rare.border_image.repeat),
        TransitionProperty => field!(transitions.property),
        TransitionDuration => field!(transitions.duration),
        TransitionTimingFunction => field!(transitions.timing_function),
        TransitionDelay => field!(transitions.delay),
        TransitionBehavior => field!(transitions.behavior),
        AnimationDuration => field!(rare.animations.duration),
        AnimationTimingFunction => field!(rare.animations.timing_function),
        AnimationDelay => field!(rare.animations.delay),
        AnimationIterationCount => field!(rare.animations.iteration_count),
        AnimationDirection => field!(rare.animations.direction),
        AnimationFillMode => field!(rare.animations.fill_mode),
        AnimationPlayState => field!(rare.animations.play_state),
        AnimationName => field!(rare.animations.name),
        AnimationTimeline => field!(rare.animations.timeline),
        AnimationRangeStart => field!(rare.animations.range_start),
        AnimationRangeEnd => field!(rare.animations.range_end),
    }
}

discrete!(
    Arc<[computed_style::TransitionProperty]>,
    Arc<[computed_style::EasingFunction]>,
    Arc<[computed_style::TransitionBehavior]>,
    Arc<[computed_style::PlaybackDirection]>,
    Arc<[computed_style::FillMode]>,
    Arc<[computed_style::AnimationPlayState]>,
    Arc<[computed_style::AnimationName]>,
    Arc<[computed_style::AnimationTimeline]>,
    Arc<[computed_style::AnimationRangeBoundary]>,
);

/// The `transition-*` and `animation-*` times and counts are never
/// animated.
impl Animate for Arc<[f32]> {
    fn animate(&self, _: &Self, _: f64, _: Context) -> Option<Self> {
        None
    }
}
