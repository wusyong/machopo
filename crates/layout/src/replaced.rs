//! Replaced elements: `<img>` (Blink's `LayoutImage`), `<iframe>`
//! (`LayoutIFrame`, whose content is its child frame's, laid out and
//! painted on its own), `<canvas>` (`LayoutHTMLCanvas`, as big as its
//! bitmap) and an outermost `<svg>` (`LayoutSVGRoot`, whose content is
//! laid out by [`crate::svg`]). Layout doesn't load images; the renderer
//! does, and
//! answers [`Images`] with what it has.

use computed_style::{
    BoxSizing, ComputedStyle, LengthPercentage, LengthPercentageOrAuto, LengthPercentageOrNone,
    ObjectFit,
};
use std::sync::Arc;

use gfx::NodeId;
use gfx::{ImageData, RectF, Size, SizeF};

use crate::svg::{SvgNaturalSize, SvgSource};

/// An image element's image as layout sees it (Blink's
/// `ImageResourceContent`, through `LayoutImage`).
#[derive(Debug, Clone, PartialEq)]
pub enum ImageState {
    /// Asked for, not here yet.
    Loading,
    Loaded {
        data: ImageData,
        size: Size,
    },
    /// An SVG image (Blink's `SVGImage`): the renderer's, by number, which
    /// paints it at whatever size it is laid out; its natural dimensions,
    /// each of which it may lack.
    Svg {
        image: u64,
        natural: SvgNaturalSize,
    },
    /// Couldn't be loaded or decoded.
    Failed,
}

/// Where layout finds each image element's image.
pub trait Images {
    /// The image of `element`, or `None` if it has none to load (no `src`).
    fn image(&self, element: NodeId) -> Option<ImageState>;
}

/// No images at all, for callers that load none.
impl Images for () {
    fn image(&self, _: NodeId) -> Option<ImageState> {
        None
    }
}

/// One replaced element's image, as the box tree keeps it.
#[derive(Debug, Clone)]
pub(crate) struct Replaced {
    /// The image and its natural size, once loaded.
    pub(crate) image: Option<(ImageData, Size)>,
    /// A canvas's natural size: its bitmap's, in CSS px.
    canvas: Option<SizeF>,
    /// What it is sized as where it has no natural size (CSS's default
    /// object size): an iframe's 300×150. An image has none: without one
    /// loaded it takes only the room its style gives.
    default_size: Option<SizeF>,
    /// An outermost `<svg>`'s content.
    pub(crate) svg: Option<Arc<SvgSource>>,
    /// An outermost `<svg>`'s natural dimensions, as an SVG image's.
    svg_natural: Option<SvgNaturalSize>,
    /// An SVG image: the renderer's number for it, and its natural
    /// dimensions, which may be one, a ratio, or none.
    pub(crate) svg_image: Option<(u64, SvgNaturalSize)>,
}

/// CSS's default object size (css-images 3, "default sizing algorithm").
const DEFAULT_OBJECT_SIZE: SizeF = SizeF::new(300.0, 150.0);

impl Replaced {
    pub(crate) fn new(state: Option<ImageState>) -> Self {
        let (image, svg_image) = match state {
            Some(ImageState::Loaded { data, size }) => (Some((data, size)), None),
            Some(ImageState::Svg { image, natural }) => (None, Some((image, natural))),
            _ => (None, None),
        };
        Self {
            image,
            canvas: None,
            // An SVG image lacking a natural dimension takes the default
            // object size's there.
            default_size: svg_image.map(|_| DEFAULT_OBJECT_SIZE),
            svg: None,
            svg_natural: None,
            svg_image,
        }
    }

    /// A canvas whose bitmap is `width` × `height`: that is its natural
    /// size, and so its ratio (HTML's "intrinsic dimensions" of a canvas;
    /// none for a 0 on either axis, as for an image).
    pub(crate) fn canvas((width, height): (u32, u32)) -> Self {
        Self {
            image: None,
            canvas: Some(SizeF::new(width as f32, height as f32)),
            default_size: None,
            svg: None,
            svg_natural: None,
            svg_image: None,
        }
    }

    /// An iframe: no natural size or ratio, so the default object size.
    pub(crate) fn iframe() -> Self {
        Self {
            image: None,
            canvas: None,
            default_size: Some(DEFAULT_OBJECT_SIZE),
            svg: None,
            svg_natural: None,
            svg_image: None,
        }
    }

    /// A form control's sizing (`crate::control`): `content` where its
    /// style gives no size, as a default object size, with no natural size
    /// or ratio (a width doesn't change its height).
    pub(crate) fn control(content: SizeF) -> Self {
        Self {
            image: None,
            canvas: None,
            default_size: Some(content),
            svg: None,
            svg_natural: None,
            svg_image: None,
        }
    }

    /// An outermost `<svg>`: its natural dimensions are those of its
    /// `width` and `height` attributes that are lengths, whatever its CSS
    /// says (`natural`, [`crate::svg::svg_natural_size`]: Blink's
    /// `LayoutSVGRoot::UnscaledNaturalSizingInfo` reads
    /// `SVGSVGElement::IntrinsicWidth`), its ratio theirs or its
    /// `viewBox`'s; the default object size where it has neither.
    pub(crate) fn svg(source: SvgSource, natural: SvgNaturalSize) -> Self {
        Self {
            image: None,
            canvas: None,
            default_size: Some(DEFAULT_OBJECT_SIZE),
            svg: Some(Arc::new(source)),
            svg_natural: Some(natural),
            svg_image: None,
        }
    }

    /// The natural dimensions of an SVG image or an outermost `<svg>`.
    fn svg_dimensions(&self) -> Option<SvgNaturalSize> {
        self.svg_image
            .map(|(_, natural)| natural)
            .or(self.svg_natural)
    }

    /// Whether it is a canvas.
    pub(crate) fn is_canvas(&self) -> bool {
        self.canvas.is_some()
    }

    /// Its natural size, where it has one on both axes.
    pub(crate) fn natural_size(&self) -> Option<SizeF> {
        let image = self
            .image
            .as_ref()
            .map(|(_, size)| SizeF::new(size.width as f32, size.height as f32))
            .or(self.canvas);
        let svg = self
            .svg_dimensions()
            .and_then(|n| Some(SizeF::new(n.width?, n.height?)));
        image.or(svg)
    }

    /// Its natural width and height, each where it has one.
    fn natural_dimensions(&self) -> (Option<f32>, Option<f32>) {
        match (self.natural_size(), self.svg_dimensions()) {
            (Some(size), _) => (Some(size.width), Some(size.height)),
            (None, Some(natural)) => (natural.width, natural.height),
            (None, None) => (None, None),
        }
    }

    /// Its size where nothing else gives one (css-images 3's "default
    /// sizing algorithm" with no specified size): each natural dimension,
    /// one missing through the other and the ratio, then the default
    /// object size's. `None` for an image not loaded, which has no default.
    fn sizing_size(&self) -> Option<SizeF> {
        let (width, height) = self.natural_dimensions();
        let ratio = self.ratio();
        let width = width.or_else(|| Some(height? * ratio?));
        let height = height.or_else(|| Some(width? / ratio?));
        match (width, height, self.default_size) {
            (Some(w), Some(h), _) => Some(SizeF::new(w, h)),
            (w, h, Some(default)) => Some(SizeF::new(
                w.unwrap_or(default.width),
                h.unwrap_or(default.height),
            )),
            _ => None,
        }
    }

    /// Where its content goes in `content`, its content box: sized by
    /// `object-fit` and placed by `object-position` (css-images 3, "sizing
    /// objects"; Blink's `LayoutReplaced::ComputeReplacedContentRect`).
    /// Without a natural ratio, `contain` and `cover` fill the box, as
    /// `fill` does.
    pub(crate) fn object_rect(&self, style: &ComputedStyle, content: RectF) -> RectF {
        let room = content.size();
        let ratio = self.ratio();
        // The largest size of the ratio inside the box, or the smallest
        // that covers it.
        let fit = |cover: bool| match ratio {
            Some(r) if room.width > 0.0 && room.height > 0.0 => {
                if (room.width / room.height > r) == cover {
                    SizeF::new(room.width, room.width / r)
                } else {
                    SizeF::new(room.height * r, room.height)
                }
            }
            _ => room,
        };
        // Its own size, the box standing for the default object size.
        let natural = || {
            let (width, height) = self.natural_dimensions();
            let width = width.or_else(|| Some(height? * ratio?));
            let height = height.or_else(|| Some(width? / ratio?));
            match (width, height) {
                (Some(w), Some(h)) => SizeF::new(w, h),
                (Some(w), None) => SizeF::new(w, room.height),
                (None, Some(h)) => SizeF::new(room.width, h),
                (None, None) => fit(false),
            }
        };
        let size = match style.rare.object_fit {
            ObjectFit::Fill => room,
            ObjectFit::Contain => fit(false),
            ObjectFit::Cover => fit(true),
            ObjectFit::None => natural(),
            ObjectFit::ScaleDown => {
                let (none, contain) = (natural(), fit(false));
                if none.width <= contain.width {
                    none
                } else {
                    contain
                }
            }
        };
        let position = style.rare.object_position;
        RectF::new(
            content.x + position.x.resolve(room.width - size.width),
            content.y + position.y.resolve(room.height - size.height),
            size.width,
            size.height,
        )
    }

    /// The natural aspect ratio, width over height.
    pub(crate) fn ratio(&self) -> Option<f32> {
        self.natural_size()
            .filter(|s| s.width > 0.0 && s.height > 0.0)
            .map(|s| s.width / s.height)
            .or_else(|| self.svg_dimensions()?.ratio)
    }

    /// A natural ratio without a natural dimension (an `<svg>`, or an SVG
    /// image, with only a `viewBox`): with no size given, it fills the
    /// containing block's width, its height through the ratio (CSS 2 §
    /// 10.3.2, as Chrome does).
    pub(crate) fn fills_width(&self, style: &ComputedStyle) -> bool {
        self.natural_dimensions() == (None, None)
            && self.ratio().is_some()
            && style.width.length_percentage().is_none()
            && style.height.length_percentage().is_none()
    }

    /// A replaced flex item's content size given what is known of it: the
    /// natural size, or the other axis through the natural ratio (or the
    /// natural size on that axis without one); 0×0 without an image. taffy
    /// resolves the style's sizes, min and max around it.
    pub(crate) fn measure(&self, width: Option<f32>, height: Option<f32>) -> SizeF {
        let natural = self.sizing_size().unwrap_or(SizeF::new(0.0, 0.0));
        let ratio = self.ratio();
        match (width, height) {
            (Some(w), Some(h)) => SizeF::new(w, h),
            (Some(w), None) => SizeF::new(w, ratio.map_or(natural.height, |r| w / r)),
            (None, Some(h)) => SizeF::new(ratio.map_or(natural.width, |r| h * r), h),
            (None, None) => natural,
        }
    }

    /// The used content-box size (CSS 2 § 10.3.2 and § 10.6.2), with
    /// percentages of `percent_base`, the containing block's width (a
    /// percentage height counts as `auto`, the containing block's height
    /// not being known here). With no definite width (`None`, a max-content
    /// size), a percentage width counts as `auto`, and percentages in the
    /// padding as 0 (css-sizing 3, "cyclic percentage"). A size given on one axis gives the other
    /// through the natural aspect ratio; with neither, the natural size.
    /// Without an image (loading, failed, no `src`) the natural size is
    /// 0×0, so only what the style gives takes room. With a natural ratio,
    /// `min-*` and `max-*` keep it (CSS 2 § 10.4): an `auto` axis follows
    /// the other as they leave it, and with both `auto` the table's
    /// constraints apply to both; without one, each axis on its own.
    ///
    /// With size containment it has no natural size (0×0) and no natural
    /// ratio; with inline-size containment, no natural width (css-contain
    /// 2, "size containment"; Blink's `LayoutReplaced` with
    /// `ShouldApplySizeContainment`).
    pub(crate) fn content_size(&self, style: &ComputedStyle, percent_base: Option<f32>) -> SizeF {
        let size = self.unclamped_content_size(style, percent_base);
        let extra = Extra::of(style, percent_base.unwrap_or(0.0));
        // In the content box, as the sizes are (`box-sizing`).
        let bounds = |min: LengthPercentageOrAuto,
                      max: LengthPercentageOrNone,
                      base,
                      to_content: &dyn Fn(f32) -> f32| {
            let min = specified(min, base).map_or(0.0, to_content);
            let max = match max {
                LengthPercentageOrNone::None => f32::INFINITY,
                LengthPercentageOrNone::LengthPercentage(v) => {
                    specified(LengthPercentageOrAuto::LengthPercentage(v), base)
                        .map_or(f32::INFINITY, to_content)
                }
            };
            // A `max-*` below the `min-*` is the `min-*` (§ 10.4).
            (min, max.max(min))
        };
        let (min_w, max_w) = bounds(style.min_width, style.max_width, percent_base, &|w| {
            extra.to_content_width(w)
        });
        let (min_h, max_h) = bounds(style.min_height, style.max_height, None, &|h| {
            extra.to_content_height(h)
        });
        let clamp_w = |w: f32| w.clamp(min_w, max_w);
        let clamp_h = |h: f32| h.clamp(min_h, max_h);
        let (w, h) = (size.width, size.height);
        let width_auto = specified(sized(style.width), percent_base).is_none();
        let height_auto = specified(sized(style.height), None).is_none();
        let ratio = self.sizing_ratio(style).filter(|_| w > 0.0 && h > 0.0);
        let (width, height) = match (width_auto, height_auto, ratio) {
            (false, true, Some(r)) => {
                let w = clamp_w(w);
                (w, clamp_h(w / r))
            }
            (true, false, Some(r)) => {
                let h = clamp_h(h);
                (clamp_w(h * r), h)
            }
            (true, true, Some(_)) => {
                // § 10.4's table, by which constraints the size breaks.
                let (over_w, under_w) = (w > max_w, w < min_w);
                let (over_h, under_h) = (h > max_h, h < min_h);
                if over_w && over_h {
                    if max_w / w <= max_h / h {
                        (max_w, (max_w * h / w).max(min_h))
                    } else {
                        ((max_h * w / h).max(min_w), max_h)
                    }
                } else if under_w && under_h {
                    if min_w / w <= min_h / h {
                        ((min_h * w / h).min(max_w), min_h)
                    } else {
                        (min_w, (min_w * h / w).min(max_h))
                    }
                } else if under_w && over_h {
                    (min_w, max_h)
                } else if over_w && under_h {
                    (max_w, min_h)
                } else if over_w {
                    (max_w, (max_w * h / w).max(min_h))
                } else if under_w {
                    (min_w, (min_w * h / w).min(max_h))
                } else if over_h {
                    ((max_h * w / h).max(min_w), max_h)
                } else if under_h {
                    ((min_h * w / h).min(max_w), min_h)
                } else {
                    (w, h)
                }
            }
            _ => (clamp_w(w), clamp_h(h)),
        };
        SizeF::new(width, height).max_zero()
    }

    /// The natural ratio sizing goes by: none with size containment, or
    /// inline-size containment (css-contain 2).
    fn sizing_ratio(&self, style: &ComputedStyle) -> Option<f32> {
        use computed_style::Contain;
        let contain = crate::applied_containment(style);
        if contain.contains(Contain::SIZE) || contain.contains(Contain::INLINE_SIZE) {
            None
        } else {
            self.ratio()
        }
    }

    /// [`Replaced::content_size`] before `min-*` and `max-*`: for taffy's
    /// block layout, which applies them itself, with percentages of the
    /// containing block it knows.
    pub(crate) fn unclamped_content_size(
        &self,
        style: &ComputedStyle,
        percent_base: Option<f32>,
    ) -> SizeF {
        use computed_style::Contain;
        let extra = Extra::of(style, percent_base.unwrap_or(0.0));
        let width = specified(sized(style.width), percent_base).map(|w| extra.to_content_width(w));
        let height = specified(sized(style.height), None).map(|h| extra.to_content_height(h));
        let contain = crate::applied_containment(style);
        let (natural, ratio) = if contain.contains(Contain::SIZE) {
            (Some(SizeF::new(0.0, 0.0)), None)
        } else if contain.contains(Contain::INLINE_SIZE) {
            (self.sizing_size().map(|n| SizeF::new(0.0, n.height)), None)
        } else {
            (self.sizing_size(), self.ratio())
        };
        let (width, height) = match (width, height) {
            (Some(w), Some(h)) => (w, h),
            (Some(w), None) => (
                w,
                ratio
                    .map(|r| w / r)
                    .or(natural.map(|n| n.height))
                    .unwrap_or(0.0),
            ),
            (None, Some(h)) => (
                ratio
                    .map(|r| h * r)
                    .or(natural.map(|n| n.width))
                    .unwrap_or(0.0),
                h,
            ),
            (None, None) => match ratio.filter(|_| self.fills_width(style)) {
                // With no containing block width (a max-content size), the
                // default object size's width.
                Some(ratio) => {
                    let width = percent_base.map_or(DEFAULT_OBJECT_SIZE.width, |base| {
                        let margin =
                            |m: LengthPercentageOrAuto| specified(m, Some(base)).unwrap_or(0.0);
                        (base
                            - extra.horizontal
                            - margin(style.margin.left)
                            - margin(style.margin.right))
                        .max(0.0)
                    });
                    (width, width / ratio)
                }
                None => natural.map_or((0.0, 0.0), |n| (n.width, n.height)),
            },
        };
        SizeF::new(width, height).max_zero()
    }
}

/// `size`, with the intrinsic size keywords as `auto`: a replaced
/// element's min-content and max-content sizes are its natural size.
fn sized(size: computed_style::Size) -> LengthPercentageOrAuto {
    size.length_percentage().map_or(
        LengthPercentageOrAuto::Auto,
        LengthPercentageOrAuto::LengthPercentage,
    )
}

/// A length or percentage of `base`; `None` for `auto`, or for a
/// percentage with no base.
fn specified(value: LengthPercentageOrAuto, base: Option<f32>) -> Option<f32> {
    match value {
        LengthPercentageOrAuto::Auto => None,
        LengthPercentageOrAuto::LengthPercentage(v) => match v.px() {
            Some(px) => Some(px),
            // A percentage, or a `calc()` with one, needs the base.
            None => base.map(|base| v.resolve(base)),
        },
    }
}

/// Padding and border, for `box-sizing: border-box` sizes.
struct Extra {
    horizontal: f32,
    vertical: f32,
    border_box: bool,
}

impl Extra {
    fn of(style: &ComputedStyle, percent_base: f32) -> Self {
        let lp = |v: LengthPercentage| v.resolve(percent_base);
        let (p, b) = (&style.padding, &style.border_width);
        Self {
            horizontal: lp(p.left) + lp(p.right) + b.left + b.right,
            vertical: lp(p.top) + lp(p.bottom) + b.top + b.bottom,
            border_box: style.box_sizing == BoxSizing::BorderBox,
        }
    }

    fn to_content_width(&self, width: f32) -> f32 {
        if self.border_box {
            (width - self.horizontal).max(0.0)
        } else {
            width
        }
    }

    fn to_content_height(&self, height: f32) -> f32 {
        if self.border_box {
            (height - self.vertical).max(0.0)
        } else {
            height
        }
    }
}

trait MaxZero {
    fn max_zero(self) -> Self;
}

impl MaxZero for SizeF {
    fn max_zero(self) -> Self {
        SizeF::new(self.width.max(0.0), self.height.max(0.0))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn with(width: LengthPercentageOrAuto, height: LengthPercentageOrAuto) -> Arc<ComputedStyle> {
        let size = |v| match v {
            LengthPercentageOrAuto::Auto => computed_style::Size::Auto,
            LengthPercentageOrAuto::LengthPercentage(v) => {
                computed_style::Size::LengthPercentage(v)
            }
        };
        Arc::new(ComputedStyle {
            width: size(width),
            height: size(height),
            ..ComputedStyle::initial()
        })
    }

    fn px(v: f32) -> LengthPercentageOrAuto {
        LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(v))
    }

    const AUTO: LengthPercentageOrAuto = LengthPercentageOrAuto::Auto;

    fn loaded(w: u32, h: u32) -> Replaced {
        Replaced {
            image: Some((ImageData::new(&b""[..]), Size::new(w, h))),
            canvas: None,
            default_size: None,
            svg: None,
            svg_natural: None,
            svg_image: None,
        }
    }

    fn svg_image(width: Option<f32>, height: Option<f32>, ratio: Option<f32>) -> Replaced {
        Replaced::new(Some(ImageState::Svg {
            image: 1,
            natural: SvgNaturalSize {
                width,
                height,
                ratio,
            },
        }))
    }

    /// An SVG image's natural dimensions, each it may lack (Chrome 152's
    /// boxes): one missing through the other and the ratio, else the
    /// default object size's; with a ratio alone, the containing block's
    /// width.
    #[test]
    fn svg_images_size_by_what_they_have() {
        let size = |image: &Replaced, w, h| image.content_size(&with(w, h), Some(400.0));
        let both = svg_image(Some(20.0), Some(10.0), Some(2.0));
        assert_eq!(size(&both, AUTO, AUTO), SizeF::new(20.0, 10.0));
        let width_and_ratio = svg_image(Some(40.0), None, Some(2.0));
        assert_eq!(size(&width_and_ratio, AUTO, AUTO), SizeF::new(40.0, 20.0));
        let height_and_ratio = svg_image(None, Some(40.0), Some(2.0));
        assert_eq!(size(&height_and_ratio, AUTO, AUTO), SizeF::new(80.0, 40.0));
        let width = svg_image(Some(40.0), None, None);
        assert_eq!(size(&width, AUTO, AUTO), SizeF::new(40.0, 150.0));
        // No ratio: a width given leaves the default height.
        assert_eq!(size(&width, px(100.0), AUTO), SizeF::new(100.0, 150.0));
        let ratio = svg_image(None, None, Some(2.0));
        assert_eq!(size(&ratio, AUTO, AUTO), SizeF::new(400.0, 200.0));
        assert_eq!(size(&ratio, AUTO, px(10.0)), SizeF::new(20.0, 10.0));
        let none = svg_image(None, None, None);
        assert_eq!(size(&none, AUTO, AUTO), SizeF::new(300.0, 150.0));
        assert_eq!(none.measure(None, Some(15.0)), SizeF::new(300.0, 15.0));
    }

    fn missing() -> Replaced {
        Replaced::new(Some(ImageState::Failed))
    }

    #[test]
    fn sizes_follow_the_natural_ratio() {
        let image = loaded(40, 30);
        let size = |w, h| image.content_size(&with(w, h), Some(200.0));
        assert_eq!(size(AUTO, AUTO), SizeF::new(40.0, 30.0));
        assert_eq!(size(px(80.0), AUTO), SizeF::new(80.0, 60.0));
        assert_eq!(size(AUTO, px(15.0)), SizeF::new(20.0, 15.0));
        assert_eq!(size(px(10.0), px(10.0)), SizeF::new(10.0, 10.0));
        // A percentage of the containing block's width.
        let half = LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Percent(50.0));
        assert_eq!(size(half, AUTO), SizeF::new(100.0, 75.0));
        // A percentage height has nothing to resolve against: auto.
        assert_eq!(size(AUTO, half), SizeF::new(40.0, 30.0));
        // With no definite width, a percentage width is `auto` too.
        assert_eq!(
            image.content_size(&with(half, AUTO), None),
            SizeF::new(40.0, 30.0)
        );
    }

    #[test]
    fn measure_fills_in_what_isnt_known() {
        let image = loaded(40, 30);
        assert_eq!(image.measure(None, None), SizeF::new(40.0, 30.0));
        assert_eq!(image.measure(Some(80.0), None), SizeF::new(80.0, 60.0));
        assert_eq!(image.measure(None, Some(15.0)), SizeF::new(20.0, 15.0));
        assert_eq!(image.measure(Some(1.0), Some(2.0)), SizeF::new(1.0, 2.0));
        // No ratio (a 0-high image): the natural size on the other axis.
        let flat = loaded(40, 0);
        assert_eq!(flat.measure(Some(80.0), None), SizeF::new(80.0, 0.0));
        assert_eq!(flat.measure(None, Some(15.0)), SizeF::new(40.0, 15.0));
        assert_eq!(missing().measure(None, None), SizeF::new(0.0, 0.0));
        assert_eq!(missing().measure(Some(5.0), None), SizeF::new(5.0, 0.0));
    }

    #[test]
    fn without_an_image_only_the_style_takes_room() {
        let image = missing();
        let size = |w, h| image.content_size(&with(w, h), Some(200.0));
        assert_eq!(size(AUTO, AUTO), SizeF::new(0.0, 0.0));
        assert_eq!(size(px(30.0), px(20.0)), SizeF::new(30.0, 20.0));
        assert_eq!(size(px(30.0), AUTO), SizeF::new(30.0, 0.0));
    }

    #[test]
    fn border_box_sizing_and_min_max() {
        let image = loaded(40, 30);
        let style = Arc::new(ComputedStyle {
            width: computed_style::Size::LengthPercentage(LengthPercentage::Px(60.0)),
            box_sizing: BoxSizing::BorderBox,
            padding: computed_style::Sides {
                left: LengthPercentage::Px(10.0),
                right: LengthPercentage::Px(10.0),
                ..ComputedStyle::initial().padding
            },
            ..ComputedStyle::initial()
        });
        // 60px border box, 20px of padding: 40px content, 30px high.
        assert_eq!(
            image.content_size(&style, Some(200.0)),
            SizeF::new(40.0, 30.0)
        );
        let style = Arc::new(ComputedStyle {
            max_width: LengthPercentageOrNone::LengthPercentage(LengthPercentage::Px(20.0)),
            ..ComputedStyle::initial()
        });
        assert_eq!(image.content_size(&style, Some(200.0)).width, 20.0);
    }

    /// `min-*` and `max-*` keep an image's ratio (CSS 2 § 10.4), as
    /// Chrome 152 sizes a 200×100 image: an `auto` height follows the
    /// width they leave, an `auto` width the height, and with both `auto`
    /// the table's cases.
    #[test]
    fn min_and_max_keep_the_ratio() {
        let image = loaded(200, 100);
        let max = |v: f32| LengthPercentageOrNone::LengthPercentage(LengthPercentage::Px(v));
        let none = LengthPercentageOrNone::None;
        let size = |width, height, min: [f32; 2], max: [LengthPercentageOrNone; 2]| {
            let style = ComputedStyle {
                min_width: px(min[0]),
                min_height: px(min[1]),
                max_width: max[0],
                max_height: max[1],
                ..(*with(width, height)).clone()
            };
            let size = image.content_size(&style, Some(500.0));
            (size.width, size.height)
        };
        let cases = [
            (AUTO, AUTO, [0.0, 0.0], [max(50.0), none], (50.0, 25.0)),
            (AUTO, AUTO, [400.0, 0.0], [none, none], (400.0, 200.0)),
            (AUTO, AUTO, [0.0, 0.0], [none, max(20.0)], (40.0, 20.0)),
            (AUTO, AUTO, [0.0, 0.0], [max(50.0), max(10.0)], (20.0, 10.0)),
            (
                px(100.0),
                AUTO,
                [0.0, 0.0],
                [none, max(20.0)],
                (100.0, 20.0),
            ),
            (AUTO, AUTO, [300.0, 0.0], [none, max(100.0)], (300.0, 100.0)),
            (AUTO, AUTO, [300.0, 200.0], [none, none], (400.0, 200.0)),
            (AUTO, AUTO, [0.0, 40.0], [max(50.0), none], (50.0, 40.0)),
            (AUTO, px(40.0), [0.0, 0.0], [max(50.0), none], (50.0, 40.0)),
            (
                AUTO,
                AUTO,
                [0.0, 0.0],
                [max(150.0), max(80.0)],
                (150.0, 75.0),
            ),
            (
                AUTO,
                px(300.0),
                [0.0, 0.0],
                [none, max(100.0)],
                (200.0, 100.0),
            ),
            (AUTO, AUTO, [0.0, 200.0], [none, none], (400.0, 200.0)),
            // A `max-width` below the `min-width` is the `min-width`.
            (AUTO, AUTO, [100.0, 0.0], [max(50.0), none], (100.0, 50.0)),
        ];
        for (width, height, min, max, expected) in cases {
            assert_eq!(size(width, height, min, max), expected, "{min:?} {max:?}");
        }
    }

    /// With `box-sizing: border-box`, `min-*` and `max-*` are of the border
    /// box, as the sizes are: a 200×100 image with 5px of padding and a
    /// `max-width` of 60px has a content box 50px wide (WPT's
    /// `box-sizing-replaced-001`).
    #[test]
    fn min_and_max_of_the_border_box() {
        let image = loaded(200, 100);
        let five = LengthPercentage::Px(5.0);
        let style = ComputedStyle {
            box_sizing: BoxSizing::BorderBox,
            padding: computed_style::Sides {
                top: five,
                right: five,
                bottom: five,
                left: five,
            },
            max_width: LengthPercentageOrNone::LengthPercentage(LengthPercentage::Px(60.0)),
            min_height: px(40.0),
            ..(*with(AUTO, AUTO)).clone()
        };
        let size = image.content_size(&style, Some(500.0));
        assert_eq!((size.width, size.height), (50.0, 30.0));
    }
}
