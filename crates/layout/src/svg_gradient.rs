//! SVG's gradients, the paint servers a `fill` or `stroke` names by
//! `url()` (Blink's `LayoutSVGResourceLinearGradient` and
//! `LayoutSVGResourceRadialGradient`, with `SVGGradientElement`'s
//! attribute collection): captured with the shape, their attributes
//! taken through the `href` chain, their stops' colors from the stops'
//! styles; laid out with the shape, once its viewport and its object
//! bounding box are known.

use std::cell::{OnceCell, RefCell};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use computed_style::{ComputedStyle, LengthPercentage, SvgPaint, TransformOperation};
use gfx::NodeId;
use gfx::{Color, PointF, RectF, SizeF, Transform};

use crate::svg_dom::{SvgAttr, SvgElement, SvgName};
use crate::text::TextContext;

/// The ids a capture looked up, each with the element it found: what it
/// depends on besides what it captured.
pub(crate) type Lookups = Vec<(String, Option<NodeId>)>;

/// What capturing an `<svg>`'s content looks things up in: the elements
/// of its document by id, found the first time a `url()` asks.
pub(crate) struct Capture<'t, 'd, 'gc> {
    document: SvgElement<'d, 'gc>,
    ids: OnceCell<HashMap<String, NodeId>>,
    lookups: RefCell<Lookups>,
    /// What `<text>` is shaped with.
    text: RefCell<&'t mut TextContext>,
}

impl<'t, 'd, 'gc> Capture<'t, 'd, 'gc> {
    /// For capturing what is under `element`.
    pub(crate) fn new(element: SvgElement<'d, 'gc>, text: &'t mut TextContext) -> Self {
        let mut document = element;
        while let Some(parent) = document.parent() {
            document = parent;
        }
        Self {
            document,
            ids: OnceCell::new(),
            lookups: RefCell::new(Vec::new()),
            text: RefCell::new(text),
        }
    }

    /// The ids it looked up, each once.
    pub(crate) fn into_lookups(self) -> Lookups {
        self.lookups.into_inner()
    }

    pub(crate) fn text(&self) -> std::cell::RefMut<'_, &'t mut TextContext> {
        self.text.borrow_mut()
    }

    /// The element `#id` names (the first with the id in tree order, as
    /// `getElementById`); `None` for a URL to another document.
    pub(crate) fn by_fragment(&self, url: &str) -> Option<SvgElement<'d, 'gc>> {
        let id = css::svg::fragment_id(url)?;
        let ids = self.ids.get_or_init(|| {
            let mut ids = HashMap::new();
            let mut stack: Vec<SvgElement<'d, 'gc>> = self.document.children().collect();
            stack.reverse();
            while let Some(node) = stack.pop() {
                if node.is_element()
                    && let Some(id) = node.attribute(SvgAttr::Id)
                {
                    ids.entry(id).or_insert(node.id());
                }
                let first = stack.len();
                stack.extend(node.children());
                stack[first..].reverse();
            }
            ids
        });
        let found = ids.get(&id).copied();
        let mut lookups = self.lookups.borrow_mut();
        if !lookups.iter().any(|(looked_up, _)| *looked_up == id) {
            lookups.push((id, found));
        }
        self.document.node_by_id(found?)
    }

    /// What `paint` (a shape's `fill` or `stroke`) names, if it is a
    /// `url()` to a gradient; `None` for any other paint, and for a
    /// `url()` to anything else, which paints with its fallback.
    pub(crate) fn server(&self, paint: &SvgPaint) -> Option<SourceServer> {
        let SvgPaint::Url(url, _) = paint else {
            return None;
        };
        let element = self.by_fragment(url)?;
        if !matches!(
            element.svg_name(),
            Some(SvgName::LinearGradient | SvgName::RadialGradient)
        ) {
            return None;
        }
        Some(self.gradient(element))
    }

    /// `element`'s gradient: each attribute from the first gradient on its
    /// `href` chain that has it (the geometry only from those of its own
    /// kind), the stops from the first that has any.
    fn gradient(&self, element: SvgElement<'d, 'gc>) -> SourceServer {
        let mut chain = vec![element];
        let mut seen = HashSet::from([element.id()]);
        while let Some(next) = reference(*chain.last().unwrap())
            .and_then(|url| self.by_fragment(&url))
            .filter(|next| {
                matches!(
                    next.svg_name(),
                    Some(SvgName::LinearGradient | SvgName::RadialGradient)
                )
            })
            .filter(|next| seen.insert(next.id()))
        {
            chain.push(next);
        }
        let linear = element.svg_name() == Some(SvgName::LinearGradient);
        let same_kind = |e: &SvgElement<'d, 'gc>| e.svg_name() == element.svg_name();
        let length = |name: SvgAttr| {
            chain
                .iter()
                .filter(|e| same_kind(e))
                .find_map(|e| attribute_length(*e, name))
        };
        let geometry = if linear {
            let percent = LengthPercentage::Percent;
            Geometry::Linear {
                x1: length(SvgAttr::X1).unwrap_or(percent(0.0)),
                y1: length(SvgAttr::Y1).unwrap_or(percent(0.0)),
                x2: length(SvgAttr::X2).unwrap_or(percent(100.0)),
                y2: length(SvgAttr::Y2).unwrap_or(percent(0.0)),
            }
        } else {
            let half = LengthPercentage::Percent(50.0);
            let (cx, cy) = (
                length(SvgAttr::Cx).unwrap_or(half),
                length(SvgAttr::Cy).unwrap_or(half),
            );
            Geometry::Radial {
                cx,
                cy,
                r: length(SvgAttr::R).unwrap_or(half),
                fx: length(SvgAttr::Fx).unwrap_or(cx),
                fy: length(SvgAttr::Fy).unwrap_or(cy),
                fr: length(SvgAttr::Fr).unwrap_or(LengthPercentage::Percent(0.0)),
            }
        };
        let object_bounding_box = chain
            .iter()
            .find_map(|e| match e.attribute(SvgAttr::GradientUnits).as_deref() {
                Some("userSpaceOnUse") => Some(false),
                Some("objectBoundingBox") => Some(true),
                _ => None,
            })
            .unwrap_or(true);
        let spread = chain
            .iter()
            .find_map(|e| match e.attribute(SvgAttr::SpreadMethod).as_deref() {
                Some("pad") => Some(Spread::Pad),
                Some("reflect") => Some(Spread::Reflect),
                Some("repeat") => Some(Spread::Repeat),
                _ => None,
            })
            .unwrap_or(Spread::Pad);
        // `gradientTransform` is the gradient's `transform` presentation
        // attribute (as Chrome maps it), so a stylesheet's counts too.
        let transform = chain
            .iter()
            .find_map(|e| {
                let style = e.computed_style()?;
                (!style.transform.is_empty() || e.attribute(SvgAttr::GradientTransform).is_some())
                    .then(|| style.transform.clone())
            })
            .unwrap_or_default();
        let stops = chain
            .iter()
            .map(|e| stops(*e))
            .find(|stops| !stops.is_empty())
            .unwrap_or_default();
        if stops.is_empty() {
            return SourceServer::NoStops;
        }
        SourceServer::Gradient(Arc::new(SourceGradient {
            geometry,
            object_bounding_box,
            transform,
            spread,
            stops,
        }))
    }
}

/// A gradient's `href`, or SVG 1.1's `xlink:href` without one.
fn reference(element: SvgElement<'_, '_>) -> Option<String> {
    element
        .attribute(SvgAttr::Href)
        .or_else(|| element.attribute(SvgAttr::XlinkHref))
        .map(|url| url.trim().to_owned())
}

/// A gradient's `<length>` attribute (`x1`, `r`); `None` when it is
/// absent or invalid, as if it weren't there.
fn attribute_length(element: SvgElement<'_, '_>, name: SvgAttr) -> Option<LengthPercentage> {
    let length = element
        .attribute(name)?
        .trim()
        .parse::<svgtypes::Length>()
        .ok()?;
    let font_size = element
        .computed_style()
        .map_or(16.0, |style| style.font.size);
    Some(crate::svg::length(length, font_size))
}

/// A gradient's `<stop>` children: each offset (a number, or a
/// percentage of 1) clamped to 0 to 1 and to no less than the one before,
/// each color with its `stop-opacity`.
fn stops(gradient: SvgElement<'_, '_>) -> Vec<(f32, Color)> {
    let mut last = 0.0f32;
    gradient
        .children()
        .filter(|child| child.svg_name() == Some(SvgName::Stop))
        .map(|stop| {
            let offset = stop
                .attribute(SvgAttr::Offset)
                .and_then(|text| {
                    let text = text.trim();
                    match text.strip_suffix('%') {
                        Some(percent) => percent.parse::<f32>().ok().map(|p| p / 100.0),
                        None => text.parse::<f32>().ok(),
                    }
                })
                .filter(|offset| offset.is_finite())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0)
                .max(last);
            last = offset;
            let style = stop
                .computed_style()
                .unwrap_or_else(|| Arc::new(ComputedStyle::initial()));
            let color = style.resolve(style.svg.stop_color);
            let alpha = base::numerics::clamp_round(f32::from(color.a) * style.svg.stop_opacity);
            (offset, Color { a: alpha, ..color })
        })
        .collect()
}

/// What a shape's `url()` paint names, as captured.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SourceServer {
    Gradient(Arc<SourceGradient>),
    /// A gradient without stops: nothing is painted, not the fallback.
    NoStops,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SourceGradient {
    geometry: Geometry,
    /// `gradientUnits="objectBoundingBox"` (the default): the geometry is
    /// in fractions of the shape's bounding box.
    object_bounding_box: bool,
    /// `gradientTransform`.
    transform: Arc<[TransformOperation]>,
    spread: Spread,
    /// At least one.
    stops: Vec<(f32, Color)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Geometry {
    Linear {
        x1: LengthPercentage,
        y1: LengthPercentage,
        x2: LengthPercentage,
        y2: LengthPercentage,
    },
    Radial {
        cx: LengthPercentage,
        cy: LengthPercentage,
        r: LengthPercentage,
        fx: LengthPercentage,
        fy: LengthPercentage,
        fr: LengthPercentage,
    },
}

/// `spreadMethod`: what is past the gradient's ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spread {
    Pad,
    Reflect,
    Repeat,
}

/// What a shape's `url()` paint paints with, laid out (Blink's
/// `SVGPaintServer`).
#[derive(Debug, Clone, PartialEq)]
pub enum SvgPaintServer {
    Gradient(SvgGradient),
    /// One color: a gradient with one stop, or with no length (its line's
    /// ends, or its radius, at one place: the last stop's color).
    Color(Color),
    /// Nothing: a gradient without stops.
    None,
}

/// A gradient in a shape's user space.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgGradient {
    pub kind: SvgGradientKind,
    /// At least two, their offsets from 0 to 1, in order.
    pub stops: Vec<(f32, Color)>,
    pub spread: Spread,
    /// From the gradient's space to the shape's user space: the object
    /// bounding box's, then `gradientTransform`.
    pub transform: Transform,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SvgGradientKind {
    Linear {
        start: PointF,
        end: PointF,
    },
    /// From the focal circle (offset 0) to the end circle (offset 1).
    Radial {
        focal: PointF,
        focal_radius: f32,
        center: PointF,
        radius: f32,
    },
}

impl SourceServer {
    /// Laid out for a shape whose object bounding box is `bbox`, in a
    /// viewport `viewport` big; `None` where the gradient can't be used
    /// (an object bounding box with no width or height), which paints
    /// with the `url()`'s fallback, as Chrome does.
    pub(crate) fn lay_out(&self, bbox: Option<RectF>, viewport: SizeF) -> Option<SvgPaintServer> {
        let gradient = match self {
            SourceServer::NoStops => return Some(SvgPaintServer::None),
            SourceServer::Gradient(gradient) => gradient,
        };
        let (to_user, units) = if gradient.object_bounding_box {
            let bbox = bbox.filter(|b| b.width > 0.0 && b.height > 0.0)?;
            let to_user = Transform::translate(bbox.x, bbox.y)
                .then(&Transform::scale(bbox.width, bbox.height));
            // Fractions of the box: a percentage is of 1.
            (to_user, SizeF::new(1.0, 1.0))
        } else {
            (Transform::IDENTITY, viewport)
        };
        let resolve = |value: LengthPercentage, base: f32| value.resolve(base);
        let diagonal = ((units.width * units.width + units.height * units.height) / 2.0).sqrt();
        let last = gradient.stops.last().unwrap().1;
        let kind = match gradient.geometry {
            Geometry::Linear { x1, y1, x2, y2 } => {
                let start = PointF::new(resolve(x1, units.width), resolve(y1, units.height));
                let end = PointF::new(resolve(x2, units.width), resolve(y2, units.height));
                if start == end {
                    return Some(SvgPaintServer::Color(last));
                }
                SvgGradientKind::Linear { start, end }
            }
            Geometry::Radial {
                cx,
                cy,
                r,
                fx,
                fy,
                fr,
            } => {
                let radius = resolve(r, diagonal);
                if radius <= 0.0 {
                    return Some(SvgPaintServer::Color(last));
                }
                SvgGradientKind::Radial {
                    focal: PointF::new(resolve(fx, units.width), resolve(fy, units.height)),
                    focal_radius: resolve(fr, diagonal).max(0.0),
                    center: PointF::new(resolve(cx, units.width), resolve(cy, units.height)),
                    radius,
                }
            }
        };
        if let [(_, color)] = gradient.stops[..] {
            return Some(SvgPaintServer::Color(color));
        }
        let transform = to_user.then(&gradient_transform(&gradient.transform, units));
        Some(SvgPaintServer::Gradient(SvgGradient {
            kind,
            stops: gradient.stops.clone(),
            spread: gradient.spread,
            transform,
        }))
    }
}

/// `gradientTransform`, its percentages of the gradient's units.
fn gradient_transform(operations: &[TransformOperation], units: SizeF) -> Transform {
    computed_style::transform_list_matrix(operations, units)
}
