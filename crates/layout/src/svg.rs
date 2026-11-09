//! SVG's layout (Blink's `core/layout/svg`): an outermost `<svg>` is a
//! replaced box (`LayoutSVGRoot`), and its descendants are laid out by
//! SVG's own rules, not CSS's boxes: each container's viewport and
//! transform (`LayoutSVGContainer`, `LayoutSVGViewportContainer`), each
//! shape's path (`LayoutSVGShape`).
//!
//! The box tree keeps what the descendants are as an [`SvgSource`],
//! captured when the box is built, and builds it again whole when anything
//! in it changes; layout makes the [`SvgFragment`] from it once the
//! `<svg>`'s content box, its viewport, is known. Pre-paint makes the
//! transforms, clips and opacities property nodes, as Chrome does, and
//! paint draws each shape on its own.
//!
//! Arcs come out as Skia's conics, as Blink's paths make them (circles and
//! ellipses `SkPath::addOval`, rounded rects `addRRect`, path data's arcs
//! `arcTo`), so they land on the same pixels.

use std::sync::Arc;

use computed_style::{
    ClipPath, ComputedStyle, Display, LengthPercentage, LengthPercentageOrAuto, Overflow, Sides,
    Size, TransformBox, Visibility,
};
use gfx::NodeId;
pub use gfx::svg::SvgNaturalSize;
use gfx::{Path, PointF, RectF, SizeF, Transform};

use crate::clip_path::ClipPaths;
use crate::fragment::{BoxFragment, BoxRole, FragmentLink};
use crate::svg_dom::{SvgAttr, SvgElement, SvgName};
use crate::svg_gradient::{Capture, Lookups, SourceServer, SvgPaintServer};
use crate::svg_text::{SourceText, SvgText};

/// An outermost `<svg>`'s descendants, as the box tree keeps them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SvgSource {
    viewport: Viewport,
    children: Vec<SourceNode>,
}

/// How an `<svg>` maps its user space into its viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Viewport {
    /// `None` without a valid `viewBox` (one with no area included: SVG
    /// 2 disables the element's rendering then, which Chrome sizes as if
    /// it had none).
    view_box: Option<RectF>,
    aspect: svgtypes::AspectRatio,
    /// A `viewBox` with no width or height: nothing is drawn.
    disabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct SourceNode {
    node: NodeId,
    style: Arc<ComputedStyle>,
    kind: SourceKind,
    children: Vec<SourceNode>,
    /// A shape's `fill` and `stroke` paint servers, where they are
    /// `url()`s to gradients.
    servers: [Option<SourceServer>; 2],
    /// The `<clipPath>` its `clip-path: url()` names, if one machopo clips to.
    clip: Option<Arc<ClipSource>>,
}

#[derive(Debug, Clone, PartialEq)]
enum SourceKind {
    /// `<g>`, `<a>`, and a `<switch>`, whose one child drawn is in
    /// `children`.
    Group,
    /// A nested `<svg>`.
    Svg(Viewport),
    /// A `<use>`: its copy of what it references, if anything, is its one
    /// child, placed at its `x` and `y`, sized by its `width` and
    /// `height` if that is an `<svg>` or a `<symbol>` (attributes, not
    /// properties, as in Chrome; `None` for `auto`).
    Use(Option<LengthPercentage>, Option<LengthPercentage>),
    /// A `<symbol>` a `<use>` draws.
    Symbol(Viewport),
    /// `<defs>`, a `<symbol>` in the document (only `<use>`s draw one),
    /// and a `<g>` with `display: none`: not drawn, but what is in it has
    /// geometry (Blink's `LayoutSVGHiddenContainer`).
    Hidden,
    /// A `<text>` with characters, its content shaped.
    Text(Arc<SourceText>),
    Rect,
    Circle,
    Ellipse,
    /// `x1`, `y1`, `x2`, `y2`: attributes, not properties.
    Line([LengthPercentage; 4]),
    /// `points`; a polygon is closed.
    Poly {
        points: Arc<[PointF]>,
        closed: bool,
    },
    Path,
}

impl SvgSource {
    /// `svg`'s descendants that are drawn: containers, `<use>`s with their
    /// copies, shapes and paths, with the gradients the shapes paint with.
    /// The others, and what is in them, aren't drawn yet (`<text>`,
    /// `<image>`, `foreignObject`), aren't drawn on their own (the paint
    /// servers and other resources), or are elements SVG doesn't know.
    pub(crate) fn capture(svg: SvgElement<'_, '_>, text: &mut crate::text::TextContext) -> Self {
        Self::capture_noting(svg, text).0
    }

    /// [`capture`](Self::capture), with the ids it looked up.
    pub(crate) fn capture_noting(
        svg: SvgElement<'_, '_>,
        text: &mut crate::text::TextContext,
    ) -> (Self, Lookups) {
        let cx = Capture::new(svg, text);
        let source = Self {
            viewport: Viewport::of(svg),
            children: svg
                .children()
                .filter_map(|child| capture_node(&cx, child, None, false))
                .collect(),
        };
        (source, cx.into_lookups())
    }
}

/// `element` as it is drawn: as itself, or in a `<use>`'s copy, drawn as
/// that `<use>` (`shown_as`: what hit testing finds and paint records, as
/// Chrome retargets from a copy to the `<use>` in the document). `used`:
/// it is what a `<use>` references, the one way a `<symbol>` is drawn.
fn capture_node<'d, 'gc>(
    cx: &Capture<'_, 'd, 'gc>,
    element: SvgElement<'d, 'gc>,
    shown_as: Option<NodeId>,
    used: bool,
) -> Option<SourceNode> {
    if !element.is_element() {
        return None;
    }
    let style = element.computed_style()?;
    if !element.is_svg_element() || !conditions_hold(element) {
        return None;
    }
    let svg_name = element.svg_name();
    // A `<g>` with `display: none` is still there, hidden (Blink's
    // `SVGGElement::LayoutObjectIsNeeded`): what is in it has geometry.
    let hidden_group = svg_name == Some(SvgName::G) && style.display == Display::None;
    if style.display == Display::None && !hidden_group {
        return None;
    }
    let node = shown_as.unwrap_or(element.id());
    let clip = clip_of(cx, &style, 0);
    // Its element children as drawn.
    let children = |only_first: bool| -> Vec<SourceNode> {
        let mut out: Vec<SourceNode> = element
            .children()
            .filter_map(|child| capture_node(cx, child, shown_as, false))
            .collect();
        // A `<switch>` draws its first child whose conditions hold (those
        // that don't aren't captured at all).
        if only_first {
            out.truncate(1);
        }
        out
    };
    let kind = match svg_name {
        _ if hidden_group => SourceKind::Hidden,
        Some(SvgName::Defs) => SourceKind::Hidden,
        Some(SvgName::G | SvgName::A | SvgName::Switch) => SourceKind::Group,
        Some(SvgName::Svg) => SourceKind::Svg(Viewport::of(element)),
        // What it references, as copied: its copy as nodes, drawn as
        // itself.
        Some(SvgName::Use) => {
            let child = element.use_copy().and_then(|target| {
                let child = capture_node(cx, target, Some(node), true)?;
                Some(match child.kind {
                    // Only a `<use>` draws a `<symbol>`.
                    SourceKind::Group if target.svg_name() == Some(SvgName::Symbol) => SourceNode {
                        kind: SourceKind::Symbol(Viewport::of(target)),
                        ..child
                    },
                    _ => child,
                })
            });
            let size = |name| attribute_size(element, name, style.font.size);
            return Some(SourceNode {
                node,
                kind: SourceKind::Use(size(SvgAttr::Width), size(SvgAttr::Height)),
                style,
                children: child.into_iter().collect(),
                servers: [None, None],
                clip,
            });
        }
        // As a group here; a `<use>` makes it its viewport (above), and
        // nothing else draws one, not one in what a `<use>` copies.
        Some(SvgName::Symbol) if used => SourceKind::Group,
        Some(SvgName::Symbol) => SourceKind::Hidden,
        Some(SvgName::Rect) => SourceKind::Rect,
        Some(SvgName::Circle) => SourceKind::Circle,
        Some(SvgName::Ellipse) => SourceKind::Ellipse,
        Some(SvgName::Line) => SourceKind::Line(
            [SvgAttr::X1, SvgAttr::Y1, SvgAttr::X2, SvgAttr::Y2]
                .map(|attribute| attribute_length(element, attribute, style.font.size)),
        ),
        Some(SvgName::Polyline | SvgName::Polygon) => {
            let points = element
                .attribute(SvgAttr::Points)
                .map(|points| points_of(&points))
                .unwrap_or_default();
            SourceKind::Poly {
                points: points.into(),
                closed: svg_name == Some(SvgName::Polygon),
            }
        }
        Some(SvgName::Path) => SourceKind::Path,
        Some(SvgName::Text) => {
            match SourceText::capture(cx, element, shown_as, style.clone()) {
                Some(text) => SourceKind::Text(Arc::new(text)),
                // One with no characters draws nothing, but is there, as
                // Blink's `LayoutSVGText` is: no bounding box, a client rect
                // at its origin.
                None => {
                    return Some(SourceNode {
                        node,
                        style,
                        kind: SourceKind::Group,
                        children: Vec::new(),
                        servers: [None, None],
                        clip,
                    });
                }
            }
        }
        _ => return None,
    };
    let (children, servers) = match kind {
        SourceKind::Group | SourceKind::Svg(_) | SourceKind::Hidden => {
            (children(svg_name == Some(SvgName::Switch)), [None, None])
        }
        _ => (
            Vec::new(),
            [cx.server(&style.svg.fill), cx.server(&style.svg.stroke)],
        ),
    };
    Some(SourceNode {
        node,
        style,
        kind,
        children,
        servers,
        clip,
    })
}

/// The languages machopo prefers: `navigator.languages`' (fixed).
const LANGUAGES: [&str; 2] = ["en-US", "en"];

/// The extensions `requiredExtensions` may name that Chrome has: HTML and
/// MathML in `foreignObject`.
const EXTENSIONS: [&str; 2] = [
    "http://www.w3.org/1999/xhtml",
    "http://www.w3.org/1998/Math/MathML",
];

/// SVG's conditional processing attributes: `systemLanguage` holds when
/// one of its languages is one machopo prefers, or one of those followed by
/// `-` and more; `requiredExtensions` when it names only extensions machopo
/// has (empty, it doesn't). `requiredFeatures` is SVG 1.1's, ignored as
/// Chrome does.
pub(crate) fn conditions_hold(element: SvgElement<'_, '_>) -> bool {
    let languages = element
        .attribute(SvgAttr::SystemLanguage)
        .is_none_or(|value| {
            value.split(',').map(str::trim).any(|language| {
                LANGUAGES.iter().any(|preferred| {
                    language.eq_ignore_ascii_case(preferred)
                        || language.as_bytes().get(preferred.len()) == Some(&b'-')
                            && language
                                .get(..preferred.len())
                                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(preferred))
                })
            })
        });
    let extensions = element
        .attribute(SvgAttr::RequiredExtensions)
        .is_none_or(|value| {
            let mut names = value.split_ascii_whitespace().peekable();
            names.peek().is_some() && names.all(|name| EXTENSIONS.contains(&name))
        });
    languages && extensions
}

impl Viewport {
    fn of(svg: SvgElement<'_, '_>) -> Self {
        // Four numbers: a negative width or height is an error (no
        // `viewBox`), a 0 one disables rendering (SVG 2).
        let view_box = svg.attribute(SvgAttr::ViewBox).and_then(|text| {
            let numbers: Vec<f64> = svgtypes::NumberListParser::from(text.as_str())
                .collect::<Result<_, _>>()
                .ok()?;
            match numbers[..] {
                [x, y, w, h] if w >= 0.0 && h >= 0.0 => Some((x, y, w, h)),
                _ => None,
            }
        });
        let aspect = svg
            .attribute(SvgAttr::PreserveAspectRatio)
            .and_then(|text| text.parse().ok())
            .unwrap_or_default();
        let disabled = view_box.is_some_and(|(_, _, w, h)| w == 0.0 || h == 0.0);
        Self {
            view_box: view_box.filter(|_| !disabled).map(|(x, y, w, h)| {
                let f = base::numerics::f32_from_f64;
                RectF::new(f(x), f(y), f(w), f(h))
            }),
            aspect,
            disabled,
        }
    }

    /// From the user space the content is in to a viewport `size` big
    /// (SVG 2's "equivalent transform of an SVG viewport").
    fn transform(&self, size: SizeF) -> Transform {
        let Some(b) = self.view_box else {
            return Transform::IDENTITY;
        };
        let (sx, sy) = (size.width / b.width, size.height / b.height);
        use svgtypes::Align;
        let (ax, ay) = match self.aspect.align {
            Align::None => {
                return Transform([sx, 0.0, 0.0, sy, -b.x * sx, -b.y * sy]);
            }
            Align::XMinYMin => (0.0, 0.0),
            Align::XMidYMin => (0.5, 0.0),
            Align::XMaxYMin => (1.0, 0.0),
            Align::XMinYMid => (0.0, 0.5),
            Align::XMidYMid => (0.5, 0.5),
            Align::XMaxYMid => (1.0, 0.5),
            Align::XMinYMax => (0.0, 1.0),
            Align::XMidYMax => (0.5, 1.0),
            Align::XMaxYMax => (1.0, 1.0),
        };
        let s = if self.aspect.slice {
            sx.max(sy)
        } else {
            sx.min(sy)
        };
        let tx = -b.x * s + ax * (size.width - b.width * s);
        let ty = -b.y * s + ay * (size.height - b.height * s);
        Transform([s, 0.0, 0.0, s, tx, ty])
    }

    /// The size lengths in the content are percentages of: the
    /// `viewBox`'s, or the viewport's without one.
    fn reference(&self, size: SizeF) -> SizeF {
        self.view_box.map_or(size, |b| b.size())
    }
}

/// An attribute that is an SVG `<length>` (`<line>`'s `x1`): a number is
/// px; `em` and `ex` of the element's font size; invalid is 0.
fn attribute_length(
    element: SvgElement<'_, '_>,
    name: SvgAttr,
    font_size: f32,
) -> LengthPercentage {
    element
        .attribute(name)
        .and_then(|text| text.trim().parse::<svgtypes::Length>().ok())
        .map_or(LengthPercentage::Px(0.0), |length| {
            self::length(length, font_size)
        })
}

/// An SVG `<length>`: a number is px; `em` and `ex` of the element's font
/// size.
pub(crate) fn length(length: svgtypes::Length, font_size: f32) -> LengthPercentage {
    let n = base::numerics::f32_from_f64(length.number);
    use svgtypes::LengthUnit as U;
    LengthPercentage::Px(match length.unit {
        U::None | U::Px => n,
        U::Em => n * font_size,
        // Half the font size, as Blink takes `ex` without the font's
        // x-height.
        U::Ex => n * font_size / 2.0,
        U::In => n * 96.0,
        U::Cm => n * 96.0 / 2.54,
        U::Mm => n * 96.0 / 25.4,
        U::Pt => n * 96.0 / 72.0,
        U::Pc => n * 16.0,
        U::Percent => return LengthPercentage::Percent(n),
    })
}

/// `<use>`'s `width` or `height` attribute: a non-negative `<length>`, or
/// `None` for `auto`, none or invalid.
fn attribute_size(
    element: SvgElement<'_, '_>,
    name: SvgAttr,
    font_size: f32,
) -> Option<LengthPercentage> {
    let text = element.attribute(name)?;
    if text.trim().parse::<svgtypes::Length>().is_err() {
        return None;
    }
    match attribute_length(element, name, font_size) {
        LengthPercentage::Px(px) if px < 0.0 => None,
        LengthPercentage::Percent(p) if p < 0.0 => None,
        length => Some(length),
    }
}

pub fn svg_natural_size(root: SvgElement<'_, '_>) -> SvgNaturalSize {
    let font_size = root.computed_style().map_or(16.0, |style| style.font.size);
    let length = |name| match attribute_size(root, name, font_size) {
        Some(LengthPercentage::Px(px)) => Some(px),
        _ => None,
    };
    let (width, height) = (length(SvgAttr::Width), length(SvgAttr::Height));
    let ratio = Viewport::of(root)
        .view_box
        .filter(|b| b.width > 0.0 && b.height > 0.0)
        .map(|b| b.width / b.height);
    SvgNaturalSize {
        width,
        height,
        ratio,
    }
}

/// An SVG image's document laid out at `size` (Blink's `SVGImage` drawn
/// for a container that big): a layout view of that size holding the root
/// `<svg>`, which fills it whatever its own `width` and `height`, as an
/// `SVGImage` sizes its root to its container.
pub fn svg_image_fragment(
    root: SvgElement<'_, '_>,
    text: &mut crate::text::TextContext,
    size: SizeF,
) -> Arc<BoxFragment> {
    let source = SvgSource::capture(root, text);
    let none = Sides {
        top: 0.0,
        right: 0.0,
        bottom: 0.0,
        left: 0.0,
    };
    let initial = Arc::new(ComputedStyle::initial());
    let svg = BoxFragment {
        pseudo: None,
        node: Some(root.id()),
        role: BoxRole::RootElement,
        style: root.computed_style().unwrap_or_else(|| initial.clone()),
        floating: false,
        size,
        padding: none,
        border: none,
        margin: none,
        children: Vec::new(),
        out_of_flow: Vec::new(),
        lines: Vec::new(),
        image: None,
        canvas: false,
        svg: Some(Arc::new(source.lay_out(size))),
        svg_image: None,
        object_rect: None,
        columns: Vec::new(),
        hidden: false,
        fixed_to_viewport: false,
        sticky_below: false,
        clip_path: None,
        border_collapsed: false,
        background_inside_border: false,
        collapsed_borders: None,
        control_state: None,
    };
    Arc::new(BoxFragment {
        pseudo: None,
        node: None,
        role: BoxRole::LayoutView,
        style: initial,
        floating: false,
        size,
        padding: none,
        border: none,
        margin: none,
        children: vec![FragmentLink {
            offset: PointF::new(0.0, 0.0),
            fragment: Arc::new(svg),
        }],
        out_of_flow: Vec::new(),
        lines: Vec::new(),
        image: None,
        canvas: false,
        svg: None,
        svg_image: None,
        object_rect: None,
        columns: Vec::new(),
        hidden: false,
        fixed_to_viewport: false,
        sticky_below: false,
        clip_path: None,
        border_collapsed: false,
        background_inside_border: false,
        collapsed_borders: None,
        control_state: None,
    })
}

/// `points`: pairs of numbers, an odd one at the end left out; up to the
/// first error (SVG 2, "error handling").
fn points_of(text: &str) -> Vec<PointF> {
    svgtypes::PointsParser::from(text)
        .map(|(x, y)| {
            PointF::new(
                base::numerics::f32_from_f64(x),
                base::numerics::f32_from_f64(y),
            )
        })
        .collect()
}

/// An outermost `<svg>`'s content laid out in its content box.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgFragment {
    /// From the content's user space to the content box (its origin at
    /// the content box's): the `viewBox` (Blink's
    /// `LayoutSVGRoot::LocalToBorderBoxTransform`).
    pub content_transform: Transform,
    pub children: Vec<SvgNode>,
}

/// One drawn SVG element (a `LayoutSVGModelObject`).
#[derive(Debug, Clone, PartialEq)]
pub struct SvgNode {
    pub node: NodeId,
    pub style: Arc<ComputedStyle>,
    /// Its `transform`, from its space to its parent's (identity for
    /// none), about its `transform-origin` in its `transform-box` (by
    /// default its viewport's origin: the UA stylesheet's `0 0`).
    pub transform: Transform,
    /// A nested `<svg>`'s viewport, in its own space, if its `overflow`
    /// clips: what its content is cut to.
    pub clip: Option<RectF>,
    /// A nested `<svg>`'s position and `viewBox`: from its content's space
    /// to its own (identity for others).
    pub content_transform: Transform,
    /// What its `clip-path` clips it and what is in it to, in its own
    /// space; `None` for nothing.
    pub clip_path: Option<ClipPaths>,
    pub kind: SvgNodeKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SvgNodeKind {
    /// `<g>`, `<a>`, a nested `<svg>`: its children, in paint order.
    Container(Vec<SvgNode>),
    /// `<defs>`, a `<symbol>` in the document, a `<g>` with `display:
    /// none`: its children, laid out for their geometry (`getBBox()`,
    /// `getCTM()`), not drawn or hit.
    Hidden(Vec<SvgNode>),
    Shape(SvgShape),
    Text(SvgText),
}

/// A shape's geometry, its lengths resolved against its viewport.
#[derive(Debug, Clone, PartialEq)]
pub struct SvgShape {
    /// `None` where the shape renders nothing (a rect with no area, a
    /// path with no data).
    pub path: Option<Path>,
    /// Its object bounding box: the path's tight bounds, or, for a rect,
    /// circle or ellipse with no area, where its geometry puts it (`(x, y,
    /// 0, height)`), as Blink keeps it (`fill_bounding_box_`); `None` for
    /// an empty path.
    pub bbox: Option<RectF>,
    /// `stroke-width` in user units.
    pub stroke_width: f32,
    /// A rect, circle or ellipse: no corner of its stroke reaches past
    /// half the width (Blink's `LayoutSVGRect` and `LayoutSVGEllipse`
    /// stroke bounding boxes).
    pub simple_stroke: bool,
    /// `stroke-dasharray` in user units; empty for none.
    pub dashes: Vec<f32>,
    pub dash_offset: f32,
    /// What its `fill` and `stroke` `url()`s paint with: `None` for a
    /// paint that isn't one, or for one that isn't to a paint server it
    /// can use, which paints with the `url()`'s fallback.
    pub fill_server: Option<SvgPaintServer>,
    pub stroke_server: Option<SvgPaintServer>,
}

impl SvgSource {
    /// The content laid out in a content box `size` big.
    pub(crate) fn lay_out(&self, size: SizeF) -> SvgFragment {
        if self.viewport.disabled {
            return SvgFragment {
                content_transform: Transform::IDENTITY,
                children: Vec::new(),
            };
        }
        let reference = self.viewport.reference(size);
        SvgFragment {
            content_transform: self.viewport.transform(size),
            children: self
                .children
                .iter()
                .map(|child| child.lay_out(reference, None))
                .collect(),
        }
    }
}

impl SourceNode {
    /// Laid out in a viewport whose lengths' percentages are of
    /// `viewport`; an `<svg>` or `<symbol>` a `<use>` draws takes its
    /// `width` and `height` where they aren't `auto` (`sized_by`).
    fn lay_out(
        &self,
        viewport: SizeF,
        sized_by: Option<(Option<LengthPercentage>, Option<LengthPercentage>)>,
    ) -> SvgNode {
        let style = &self.style;
        let units = Units { viewport };
        let mut clip = None;
        let mut content_transform = Transform::IDENTITY;
        let kind = match &self.kind {
            SourceKind::Group => SvgNodeKind::Container(self.lay_out_children(viewport)),
            SourceKind::Hidden => SvgNodeKind::Hidden(self.lay_out_children(viewport)),
            // Placed at its `x` and `y`, after its own `transform`.
            SourceKind::Use(width, height) => {
                content_transform =
                    Transform::translate(units.x(style.svg.x), units.y(style.svg.y));
                SvgNodeKind::Container(
                    self.children
                        .iter()
                        .map(|child| child.lay_out(viewport, Some((*width, *height))))
                        .collect(),
                )
            }
            SourceKind::Svg(nested) | SourceKind::Symbol(nested) => {
                // `width` and `height` are `auto` as 100%; a `<use>`'s
                // win where they aren't.
                let size_of = |size: Size, used: Option<LengthPercentage>, base: f32| match used
                    .or(size.length_percentage())
                {
                    Some(lp) => units.resolve(lp, base).max(0.0),
                    None => base,
                };
                let rect = RectF::new(
                    units.x(style.svg.x),
                    units.y(style.svg.y),
                    size_of(style.width, sized_by.and_then(|(w, _)| w), viewport.width),
                    size_of(style.height, sized_by.and_then(|(_, h)| h), viewport.height),
                );
                if style.overflow_x != Overflow::Visible {
                    clip = Some(rect);
                }
                content_transform =
                    Transform::translate(rect.x, rect.y).then(&nested.transform(rect.size()));
                let children = if nested.disabled || rect.width <= 0.0 || rect.height <= 0.0 {
                    Vec::new()
                } else {
                    self.lay_out_children(nested.reference(rect.size()))
                };
                SvgNodeKind::Container(children)
            }
            SourceKind::Text(text) => SvgNodeKind::Text(text.lay_out(viewport)),
            kind => {
                let path = shape_path(kind, style, &units);
                let bbox = match &path {
                    Some(path) => path.tight_bounds(),
                    None => empty_shape_bbox(kind, style, &units),
                };
                // A gradient in its object bounding box's units, which
                // one that renders nothing has none of.
                let server = |server: &Option<SourceServer>| {
                    server
                        .as_ref()?
                        .lay_out(bbox.filter(|_| path.is_some()), viewport)
                };
                SvgNodeKind::Shape(SvgShape {
                    fill_server: server(&self.servers[0]),
                    stroke_server: server(&self.servers[1]),
                    simple_stroke: matches!(
                        kind,
                        SourceKind::Rect | SourceKind::Circle | SourceKind::Ellipse
                    ),
                    path,
                    bbox,
                    stroke_width: units.diagonal(style.svg.stroke_width).max(0.0),
                    dashes: style
                        .svg
                        .stroke_dasharray
                        .iter()
                        .map(|&dash| units.diagonal(dash))
                        .collect(),
                    dash_offset: units.diagonal(style.svg.stroke_dashoffset),
                })
            }
        };
        let mut node = SvgNode {
            node: self.node,
            style: style.clone(),
            transform: Transform::IDENTITY,
            clip,
            content_transform,
            clip_path: None,
            kind,
        };
        let is_use = matches!(self.kind, SourceKind::Use(..));
        node.transform = style
            .transform_about_origin(reference_box(&node, viewport, is_use))
            .unwrap_or(Transform::IDENTITY);
        let fill_box = || crate::svg_geometry::own_bbox(&node, is_use).unwrap_or_default();
        node.clip_path = match &style.rare.clip_path {
            ClipPath::Url(_) => self
                .clip
                .as_ref()
                .map(|clip| clip.lay_out(viewport, fill_box())),
            clip_path => crate::clip_path::svg_shape_clip_paths(
                clip_path,
                fill_box(),
                crate::svg_geometry::painted_stroke_bbox(&node, is_use).unwrap_or_default(),
                viewport,
            ),
        };
        node
    }

    fn lay_out_children(&self, viewport: SizeF) -> Vec<SvgNode> {
        self.children
            .iter()
            .map(|child| child.lay_out(viewport, None))
            .collect()
    }
}

/// A `<clipPath>` as `clip-path: url()` finds it (Blink's
/// `LayoutSVGResourceClipper`, as a path): its shapes, captured to be
/// laid out with what references it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ClipSource {
    /// `clipPathUnits="objectBoundingBox"`: its user space is the object
    /// bounding box of what references it, 0 to 1 across.
    object_bounding_box: bool,
    /// Its own style: its `transform`.
    style: Arc<ComputedStyle>,
    shapes: Vec<SourceNode>,
    /// What its own `clip-path: url()` names: what it clips to is cut by
    /// that too.
    clip: Option<Arc<ClipSource>>,
}

/// The `<clipPath>` the `clip-path: url()` of `element` (in `style`) names,
/// if it is one machopo clips to, with the ids it looked up (none for no
/// `url()`).
pub(crate) fn clip_source_noting(
    element: SvgElement<'_, '_>,
    style: &ComputedStyle,
    text: &mut crate::TextContext,
) -> (Option<Arc<ClipSource>>, Lookups) {
    if !matches!(style.rare.clip_path, ClipPath::Url(_)) {
        return (None, Vec::new());
    }
    let cx = Capture::new(element, text);
    let clip = clip_of(&cx, style, 0);
    (clip, cx.into_lookups())
}

/// What `style`'s `clip-path: url()` names: `None` for none, for a URL to
/// no element of the document or to one that isn't a `<clipPath>` (Blink's
/// `ClipPathClipper` clips nothing then), and for one with text or a
/// `<use>` in it, which machopo would have to make a mask of. One that names
/// itself, at `depth` 16 at the latest, is none either (a cycle: Blink's
/// `SVGResources` drops it).
fn clip_of<'d, 'gc>(
    cx: &Capture<'_, 'd, 'gc>,
    style: &ComputedStyle,
    depth: usize,
) -> Option<Arc<ClipSource>> {
    let ClipPath::Url(url) = &style.rare.clip_path else {
        return None;
    };
    if depth >= 16 {
        return None;
    }
    let element = cx.by_fragment(url)?;
    if element.svg_name() != Some(SvgName::ClipPath) {
        return None;
    }
    let own = element.computed_style()?;
    let mut shapes = Vec::new();
    for child in element.children() {
        // Not its children's own `clip-path`s (Chrome cuts each shape by
        // its own).
        let Some(mut shape) = capture_node(cx, child, None, false) else {
            continue;
        };
        match shape.kind {
            SourceKind::Text(_) | SourceKind::Use(..) => return None,
            SourceKind::Rect
            | SourceKind::Circle
            | SourceKind::Ellipse
            | SourceKind::Line(_)
            | SourceKind::Poly { .. }
            | SourceKind::Path
                if shape.style.visibility == Visibility::Visible =>
            {
                shape.clip = None;
                shapes.push(shape);
            }
            _ => {}
        }
    }
    let clip = match &own.rare.clip_path {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "depth < 16: `clip_of` checks it first"
        )]
        ClipPath::Url(_) => Some(clip_of(cx, &own, depth + 1)?),
        _ => None,
    };
    let object_bounding_box = element
        .attribute(SvgAttr::ClipPathUnits)
        .is_some_and(|units| units.trim() == "objectBoundingBox");
    Some(Arc::new(ClipSource {
        object_bounding_box,
        style: own,
        shapes,
        clip,
    }))
}

impl ClipSource {
    /// Laid out for what references it, whose lengths' percentages are of
    /// `viewport` and whose object bounding box is `bbox`: each union of
    /// paths in its space, this one's first, then those of the
    /// `<clipPath>` its own `clip-path` names.
    pub(crate) fn lay_out(&self, viewport: SizeF, bbox: RectF) -> ClipPaths {
        let mut out = Vec::new();
        self.lay_out_into(viewport, bbox, &mut out);
        out.into()
    }

    fn lay_out_into(&self, viewport: SizeF, bbox: RectF, out: &mut Vec<Arc<[Path]>>) {
        // In units of the bounding box: its lengths' percentages of it,
        // which is 1 by 1 then.
        let (units, to_user) = if self.object_bounding_box {
            (
                SizeF::new(1.0, 1.0),
                Transform::translate(bbox.x, bbox.y)
                    .then(&Transform::scale(bbox.width, bbox.height)),
            )
        } else {
            (viewport, Transform::IDENTITY)
        };
        let own = self
            .style
            .transform_about_origin(RectF::new(0.0, 0.0, units.width, units.height))
            .unwrap_or(Transform::IDENTITY);
        let to_user = to_user.then(&own);
        let paths: Vec<Path> = self
            .shapes
            .iter()
            .filter_map(|shape| {
                let node = shape.lay_out(units, None);
                let SvgNodeKind::Shape(laid) = &node.kind else {
                    return None;
                };
                let mut path = laid.path.clone()?;
                path.transform(&to_user.then(&node.transform));
                Some(path.with_fill_rule(node.style.svg.clip_rule))
            })
            .collect();
        out.push(paths.into());
        if let Some(clip) = &self.clip {
            clip.lay_out_into(viewport, bbox, out);
        }
    }
}

/// How lengths resolve in a viewport: a percentage of its width, its
/// height, or its normalized diagonal (SVG 2, "units").
struct Units {
    viewport: SizeF,
}

impl Units {
    fn resolve(&self, value: LengthPercentage, base: f32) -> f32 {
        value.resolve(base)
    }

    fn x(&self, value: LengthPercentage) -> f32 {
        self.resolve(value, self.viewport.width)
    }

    fn y(&self, value: LengthPercentage) -> f32 {
        self.resolve(value, self.viewport.height)
    }

    /// The other lengths (`r`, `stroke-width`): of √((w² + h²) / 2).
    fn diagonal(&self, value: LengthPercentage) -> f32 {
        let SizeF { width, height } = self.viewport;
        self.resolve(value, ((width * width + height * height) / 2.0).sqrt())
    }
}

/// The box an SVG element's `transform-origin` and `translate()`'s
/// percentages are of (`transform-box`; Blink's
/// `TransformHelper::ComputeReferenceBox`), in its own user space: its
/// object bounding box (`fill-box`, and `content-box` as it), its stroke
/// bounding box (`stroke-box`, and `border-box` as it), or its viewport
/// at its user space's origin (`view-box`). An empty one is at the
/// origin.
fn reference_box(node: &SvgNode, viewport: SizeF, is_use: bool) -> RectF {
    match node.style.rare.transform_box {
        TransformBox::FillBox | TransformBox::ContentBox => {
            crate::svg_geometry::own_bbox(node, is_use).unwrap_or_default()
        }
        TransformBox::StrokeBox | TransformBox::BorderBox => {
            crate::svg_geometry::own_stroke_bbox(node, is_use).unwrap_or_default()
        }
        TransformBox::ViewBox => RectF::new(0.0, 0.0, viewport.width, viewport.height),
    }
}

/// The path of a shape with no layout node (Blink's `AsPath` from its
/// style), its lengths' percentages of nothing, and its bounding box:
/// `None` where it renders nothing, or for an element that isn't a shape.
pub(crate) fn unrendered_shape_path(
    element: SvgElement<'_, '_>,
    style: &ComputedStyle,
) -> (Option<Path>, Option<RectF>) {
    let svg_name = element.svg_name();
    let kind = match svg_name {
        Some(SvgName::Rect) => SourceKind::Rect,
        Some(SvgName::Circle) => SourceKind::Circle,
        Some(SvgName::Ellipse) => SourceKind::Ellipse,
        Some(SvgName::Line) => SourceKind::Line(
            [SvgAttr::X1, SvgAttr::Y1, SvgAttr::X2, SvgAttr::Y2]
                .map(|attribute| attribute_length(element, attribute, style.font.size)),
        ),
        Some(SvgName::Polyline | SvgName::Polygon) => SourceKind::Poly {
            points: element
                .attribute(SvgAttr::Points)
                .map(|points| points_of(&points))
                .unwrap_or_default()
                .into(),
            closed: svg_name == Some(SvgName::Polygon),
        },
        Some(SvgName::Path) => SourceKind::Path,
        _ => return (None, None),
    };
    let units = Units {
        viewport: SizeF::default(),
    };
    let path = shape_path(&kind, style, &units);
    let bbox = match &path {
        Some(path) => path.tight_bounds(),
        None => empty_shape_bbox(&kind, style, &units),
    };
    (path, bbox)
}

/// Where a rect, circle or ellipse with no area is: its `x` and `y` and
/// its sides cut to 0, its center and its radii cut to 0 (Blink's
/// `LayoutSVGRect` and `LayoutSVGEllipse` keep these as their bounding
/// boxes, whatever they draw). `None` for the other shapes.
fn empty_shape_bbox(kind: &SourceKind, style: &ComputedStyle, units: &Units) -> Option<RectF> {
    let svg = &style.svg;
    let (rx, ry) = match kind {
        SourceKind::Rect => {
            let length = |size: Size, base: f32| match size {
                Size::LengthPercentage(lp) => units.resolve(lp, base).max(0.0),
                _ => 0.0,
            };
            return Some(RectF::new(
                units.x(svg.x),
                units.y(svg.y),
                length(style.width, units.viewport.width),
                length(style.height, units.viewport.height),
            ));
        }
        SourceKind::Circle => {
            let r = units.diagonal(svg.r).max(0.0);
            (r, r)
        }
        SourceKind::Ellipse => {
            let rx = auto_or(svg.rx, |v| units.x(v));
            let ry = auto_or(svg.ry, |v| units.y(v));
            match (rx, ry) {
                (Some(rx), Some(ry)) => (rx, ry),
                (Some(r), None) | (None, Some(r)) => (r, r),
                (None, None) => (0.0, 0.0),
            }
        }
        _ => return None,
    };
    let (rx, ry) = (rx.max(0.0), ry.max(0.0));
    let (cx, cy) = (units.x(svg.cx), units.y(svg.cy));
    Some(RectF::new(cx - rx, cy - ry, 2.0 * rx, 2.0 * ry))
}

/// A shape's path in its user space; `None` where it renders nothing.
fn shape_path(kind: &SourceKind, style: &ComputedStyle, units: &Units) -> Option<Path> {
    let svg = &style.svg;
    let mut path = Path::new();
    match kind {
        SourceKind::Rect => {
            let length = |size: Size, base: f32| match size {
                Size::LengthPercentage(lp) => units.resolve(lp, base),
                _ => 0.0,
            };
            let (x, y) = (units.x(svg.x), units.y(svg.y));
            let w = length(style.width, units.viewport.width);
            let h = length(style.height, units.viewport.height);
            if w <= 0.0 || h <= 0.0 {
                return None;
            }
            // An `auto` radius is the other's; both are at most half the
            // side (SVG 2, "the rect element").
            let rx = auto_or(svg.rx, |v| units.x(v));
            let ry = auto_or(svg.ry, |v| units.y(v));
            let (rx, ry) = match (rx, ry) {
                (None, None) => (0.0, 0.0),
                (Some(rx), None) => (rx, rx),
                (None, Some(ry)) => (ry, ry),
                (Some(rx), Some(ry)) => (rx, ry),
            };
            let (rx, ry) = (rx.clamp(0.0, w / 2.0), ry.clamp(0.0, h / 2.0));
            // As Blink's `Path::AddRoundedRect` and `AddRect` add them:
            // `addRRect` from just after the top left corner, `addRect`
            // from it, clockwise.
            if rx > 0.0 && ry > 0.0 {
                path.add_round_rect(RectF::new(x, y, w, h), [SizeF::new(rx, ry); 4], true);
            } else {
                path.add_rect(PointF::new(x, y), PointF::new(x + w, y + h));
            }
        }
        SourceKind::Circle => {
            let r = units.diagonal(svg.r);
            if r <= 0.0 {
                return None;
            }
            oval(
                &mut path,
                PointF::new(units.x(svg.cx), units.y(svg.cy)),
                r,
                r,
            );
        }
        SourceKind::Ellipse => {
            let rx = auto_or(svg.rx, |v| units.x(v));
            let ry = auto_or(svg.ry, |v| units.y(v));
            // An `auto` radius is the other's (SVG 2).
            let (rx, ry) = match (rx, ry) {
                (Some(rx), Some(ry)) => (rx, ry),
                (Some(r), None) | (None, Some(r)) => (r, r),
                (None, None) => return None,
            };
            if rx <= 0.0 || ry <= 0.0 {
                return None;
            }
            oval(
                &mut path,
                PointF::new(units.x(svg.cx), units.y(svg.cy)),
                rx,
                ry,
            );
        }
        SourceKind::Line([x1, y1, x2, y2]) => {
            path.move_to(PointF::new(units.x(*x1), units.y(*y1)));
            path.line_to(PointF::new(units.x(*x2), units.y(*y2)));
        }
        SourceKind::Poly { points, closed } => {
            let (first, rest) = points.split_first()?;
            path.move_to(*first);
            for &point in rest {
                path.line_to(point);
            }
            if *closed {
                path.close();
            }
        }
        SourceKind::Path => computed_style::append_path_data(&mut path, svg.d.as_deref()?),
        SourceKind::Group
        | SourceKind::Svg(_)
        | SourceKind::Use(..)
        | SourceKind::Symbol(_)
        | SourceKind::Hidden
        | SourceKind::Text(_) => {
            return None;
        }
    }
    Some(path.with_fill_rule(svg.fill_rule))
}

fn auto_or(
    value: LengthPercentageOrAuto,
    resolve: impl Fn(LengthPercentage) -> f32,
) -> Option<f32> {
    match value {
        LengthPercentageOrAuto::Auto => None,
        LengthPercentageOrAuto::LengthPercentage(v) => Some(resolve(v)),
    }
}

/// An ellipse as Blink's `Path::AddEllipse` adds it: `addOval` from its
/// right end (index 1), clockwise.
fn oval(path: &mut Path, center: PointF, rx: f32, ry: f32) {
    let bounds = RectF::new(center.x - rx, center.y - ry, 2.0 * rx, 2.0 * ry);
    path.add_oval(bounds, true, 1);
}
