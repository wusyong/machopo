//! What script reads of an SVG element's geometry: `getBBox()`,
//! `getCTM()`, `getScreenCTM()` and its client rect, from the layout of the
//! outermost `<svg>` it is in, as Blink reads its layout objects after a
//! layout (`SVGGraphicsElement::GetBBox`, `SVGElement::ComputeCTM`,
//! `LayoutObject::AbsoluteQuads`), not pre-paint's property trees.
//!
//! An element with no layout node (`display: none`, a `<switch>`'s child
//! not drawn, one with no outermost `<svg>` drawn around it) has no
//! bounding box and no client rect, and its own transform counts for
//! nothing, but its ancestors' do, as in Blink, which walks the elements
//! and takes each one's transform from its layout object if it has one.

use gfx::{NodeId, Path, PointF, RectF, SvgGeometry, SvgTextChar, Transform};

use crate::StickyOffsets;
use crate::geometry::{ScrollOffsets, box_to_root};
use crate::svg_dom::{SvgElement, SvgName};
use crate::{BoxFragment, SvgFragment, SvgNode, SvgNodeKind, SvgText};

/// Where an SVG element is in the layout of the outermost `<svg>` around
/// it, and the transforms on the way down.
struct Found<'a, 'd, 'gc> {
    svg: &'a SvgFragment,
    /// From the `<svg>`'s content box to the root fragment's space.
    content_box: Transform,
    /// The SVG elements from the outermost `<svg>` down to the element.
    path: Vec<SvgElement<'d, 'gc>>,
    /// Each one's own transform, what `getCTM()` multiplies.
    locals: Vec<Transform>,
    /// How far the layout goes down the path.
    at: Laid<'a>,
    /// Whether it is in a hidden container.
    hidden: bool,
}

/// `element`'s place in the layout; `None` if no outermost `<svg>` around
/// it has a box.
fn find<'a, 'd, 'gc>(
    layout_view: &'a BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: SvgElement<'d, 'gc>,
) -> Option<Found<'a, 'd, 'gc>> {
    // The SVG elements from the outermost down to `element`.
    let mut path = vec![element];
    let mut innermost = element;
    while let Some(parent) = innermost.parent().filter(|parent| parent.is_svg_element()) {
        path.push(parent);
        innermost = parent;
    }
    path.reverse();
    let top = path[0];
    let found = (top.svg_name() == Some(SvgName::Svg))
        .then(|| box_to_root(layout_view, scroll, sticky, top.id()))
        .flatten()
        .and_then(|(fragment, to_root)| Some((fragment, fragment.svg.as_deref()?, to_root)));
    let (fragment, svg, to_root) = found?;
    let (b, p) = (&fragment.border, &fragment.padding);
    let content_box = to_root.then(&Transform::translate(b.left + p.left, b.top + p.top));

    let mut locals = vec![svg.content_transform];
    let mut at = Laid::Root(&svg.children);
    let mut hidden = false;
    for element in &path[1..] {
        let children = match at {
            Laid::Root(children) => Some(children),
            Laid::Node(SvgNode {
                kind: SvgNodeKind::Container(children) | SvgNodeKind::Hidden(children),
                ..
            }) => Some(&children[..]),
            _ => None,
        };
        at = match (
            at,
            children.and_then(|c| c.iter().find(|n| n.node == element.id())),
        ) {
            (_, Some(node)) => {
                hidden |= matches!(node.kind, SvgNodeKind::Hidden(_));
                Laid::Node(node)
            }
            // What is in a `<text>` lays out with it.
            (
                Laid::Node(SvgNode {
                    kind: SvgNodeKind::Text(text),
                    ..
                })
                | Laid::InText(text),
                None,
            ) if in_text(*element) => Laid::InText(text),
            _ => Laid::None,
        };
        locals.push(match at {
            // A `<use>`'s `x` and `y` place its copy, not itself.
            Laid::Node(node) if element.svg_name() == Some(SvgName::Use) => node.transform,
            Laid::Node(node) => node.transform.then(&node.content_transform),
            _ => Transform::IDENTITY,
        });
    }

    Some(Found {
        svg,
        content_box,
        path,
        locals,
        at,
        hidden,
    })
}

/// `element`'s geometry, the frame laid out as `layout_view` with its
/// scroll containers at `scroll` and its sticky boxes moved by `sticky`: its screen matrix and client rect in the
/// root fragment's space (the document's coordinates, the viewport's scroll
/// offset not taken off). `element` is an SVG element; the outermost
/// `<svg>`'s own client rects are its CSS box's, not this one. `None` if
/// no outermost `<svg>` around it has a box: then nothing around it is
/// laid out, and its geometry is the default (no box, identity matrices).
pub fn svg_geometry(
    layout_view: &BoxFragment,
    scroll: &ScrollOffsets,
    sticky: &StickyOffsets,
    element: SvgElement<'_, '_>,
) -> Option<SvgGeometry> {
    let Found {
        svg,
        content_box,
        path,
        locals,
        at,
        hidden,
    } = find(layout_view, scroll, sticky, element)?;

    // To the nearest viewport: up to and including the first `<svg>` or
    // `<symbol>` around it.
    let mut ctm = Transform::IDENTITY;
    // The outermost, the path's last, is the `<svg>` in the HTML: not a
    // viewport to stop at.
    let outermost = path.len().saturating_sub(1);
    for (i, (element, local)) in path.iter().zip(&locals).enumerate().rev() {
        ctm = local.then(&ctm);
        if i < outermost && is_viewport(*element) {
            break;
        }
    }
    let screen_ctm = locals
        .iter()
        .fold(content_box, |matrix, local| matrix.then(local));

    let is_use = element.svg_name() == Some(SvgName::Use);
    let bbox = match at {
        _ if path.len() == 1 => union_in(&svg.children),
        Laid::Root(_) | Laid::None => None,
        Laid::Node(node) => own_bbox(node, is_use),
        Laid::InText(text) => text_bbox(text, element),
    };
    let rendered = path.len() > 1 && !matches!(at, Laid::None);
    Some(SvgGeometry {
        bbox: bbox.unwrap_or_default(),
        ctm,
        screen_ctm,
        client_rect: (rendered && !hidden).then(|| screen_ctm.map_rect(bbox.unwrap_or_default())),
    })
}

/// How far down the path to an element the layout goes.
#[derive(Clone, Copy)]
enum Laid<'a> {
    /// The outermost `<svg>`, its content.
    Root(&'a [SvgNode]),
    Node(&'a SvgNode),
    /// A `<tspan>` or an `<a>` in a `<text>`, which lays them out.
    InText(&'a SvgText),
    /// An element with no layout node, or one in it.
    None,
}

/// An element that makes a viewport, where `getCTM()` stops (Blink's
/// `IsViewportElement`, of what machopo lays out).
fn is_viewport(element: SvgElement<'_, '_>) -> bool {
    matches!(element.svg_name(), Some(SvgName::Svg | SvgName::Symbol))
}

/// Whether an element in a `<text>` is laid out with it: a `<tspan>` or
/// an `<a>`, not `display: none`.
fn in_text(element: SvgElement<'_, '_>) -> bool {
    matches!(element.svg_name(), Some(SvgName::Tspan | SvgName::A))
        && element
            .computed_style()
            .is_some_and(|style| style.display != computed_style::Display::None)
}

/// `node`'s object bounding box in its own user space (`getBBox()`): a
/// shape's, even one with no area; a container's children's; a `<use>`'s
/// copy's, placed at its `x` and `y`.
pub(crate) fn own_bbox(node: &SvgNode, is_use: bool) -> Option<RectF> {
    match &node.kind {
        SvgNodeKind::Shape(shape) => shape.bbox,
        SvgNodeKind::Text(text) => text.bounds,
        SvgNodeKind::Container(children) | SvgNodeKind::Hidden(children) => {
            let bbox = union_in(children)?;
            Some(if is_use {
                node.content_transform.map_rect(bbox)
            } else {
                bbox
            })
        }
    }
}

/// The union of `children`'s bounding boxes, in their parent's space, as
/// Blink unites them (`SVGLayoutSupport::ComputeContainerBoundingBoxes`):
/// a shape with no area, an empty container or text, and a hidden container
/// don't count; one with no width or height does, and visibility doesn't
/// matter.
fn union_in(children: &[SvgNode]) -> Option<RectF> {
    children
        .iter()
        .filter_map(|child| {
            let bbox = match &child.kind {
                SvgNodeKind::Shape(shape) => shape.path.as_ref().and(shape.bbox),
                SvgNodeKind::Text(text) => text.bounds,
                SvgNodeKind::Container(children) => union_in(children),
                SvgNodeKind::Hidden(_) => None,
            }?;
            Some(
                child
                    .transform
                    .then(&child.content_transform)
                    .map_rect(bbox),
            )
        })
        .reduce(union_even_if_empty)
}

/// `node`'s stroke bounding box in its own user space (css-masking 1,
/// "compute stroke bounding box"): its object bounding box grown by
/// half its `stroke-width`, for a path as far as a miter or a square cap
/// reaches, whether or not the stroke is `none` (Blink's approximate
/// `LayoutSVGShape::StrokeBoundingBox`); a container's children's.
pub(crate) fn own_stroke_bbox(node: &SvgNode, is_use: bool) -> Option<RectF> {
    stroke_bbox(node, is_use, false)
}

/// [`own_stroke_bbox`] where only a stroke that is painted counts (one
/// that is `none` is the object bounding box), as a `clip-path`'s
/// `stroke-box` is (Blink's `StrokeBoundingBox` of a shape without a
/// stroke is its fill bounding box).
pub(crate) fn painted_stroke_bbox(node: &SvgNode, is_use: bool) -> Option<RectF> {
    stroke_bbox(node, is_use, true)
}

fn stroke_bbox(node: &SvgNode, is_use: bool, painted: bool) -> Option<RectF> {
    let unstroked = painted && node.style.svg.stroke == computed_style::SvgPaint::None;
    match &node.kind {
        SvgNodeKind::Shape(shape) if unstroked => shape.bbox,
        SvgNodeKind::Text(text) if unstroked => text.bounds,
        SvgNodeKind::Shape(shape) => {
            let reach = if shape.simple_stroke {
                1.0
            } else {
                stroke_reach(&node.style)
            };
            Some(shape.bbox?.inset(-shape.stroke_width / 2.0 * reach))
        }
        SvgNodeKind::Text(text) => {
            let width = node.style.svg.stroke_width.resolve(0.0).max(0.0);
            Some(text.bounds?.inset(-width / 2.0 * stroke_reach(&node.style)))
        }
        SvgNodeKind::Container(children) | SvgNodeKind::Hidden(children) => {
            let bbox = children
                .iter()
                .filter(|child| !matches!(child.kind, SvgNodeKind::Hidden(_)))
                .filter_map(|child| {
                    let bbox = stroke_bbox(child, false, painted)?;
                    Some(
                        child
                            .transform
                            .then(&child.content_transform)
                            .map_rect(bbox),
                    )
                })
                .reduce(union_even_if_empty)?;
            Some(if is_use {
                node.content_transform.map_rect(bbox)
            } else {
                bbox
            })
        }
    }
}

/// How far past half the stroke width its corners reach.
fn stroke_reach(style: &computed_style::ComputedStyle) -> f32 {
    let mut reach: f32 = 1.0;
    if style.svg.stroke_linejoin == computed_style::StrokeLinejoin::Miter {
        reach = reach.max(style.svg.stroke_miterlimit);
    }
    if style.svg.stroke_linecap == computed_style::StrokeLinecap::Square {
        reach = reach.max(std::f32::consts::SQRT_2);
    }
    reach
}

/// The bounds of a `<text>`'s characters in `element` (it, or a
/// `<tspan>` or `<a>` in it), in the `<text>`'s space.
fn text_bbox(text: &SvgText, element: SvgElement<'_, '_>) -> Option<RectF> {
    text.cells
        .iter()
        .filter(|cell| is_within(cell.node, element))
        .map(|cell| {
            let corners = cell.corners();
            corners[1..]
                .iter()
                .fold(RectF::new(corners[0].x, corners[0].y, 0.0, 0.0), |r, p| {
                    union_even_if_empty(r, RectF::new(p.x, p.y, 0.0, 0.0))
                })
        })
        .reduce(union_even_if_empty)
}

/// Whether `node` is `element` or in it.
fn is_within(node: NodeId, element: SvgElement<'_, '_>) -> bool {
    let mut at = element.node_by_id(node);
    while let Some(node) = at {
        if node.id() == element.id() {
            return true;
        }
        at = node.parent();
    }
    false
}

/// The layout node of an SVG element, in a hidden container or not;
/// `None` for one with none, or laid out with a `<text>`.
pub fn svg_node<'f>(
    layout_view: &'f BoxFragment,
    element: SvgElement<'_, '_>,
) -> Option<&'f SvgNode> {
    match find(
        layout_view,
        &ScrollOffsets::default(),
        &StickyOffsets::default(),
        element,
    )?
    .at
    {
        Laid::Node(node) => Some(node),
        _ => None,
    }
}

/// An SVG shape's path in its user space, for its length (Blink's
/// `SVGGeometryElement::AsPath`): as laid out, empty where it renders
/// nothing; for one with no layout node, from its style, its lengths'
/// percentages of nothing (as Chrome 152 resolves them). `None` without a
/// style.
pub fn svg_shape_path(layout_view: &BoxFragment, element: SvgElement<'_, '_>) -> Option<Path> {
    let is_rect = element.svg_name() == Some(SvgName::Rect);
    let (path, bbox) = match svg_node(layout_view, element) {
        Some(node) => match &node.kind {
            SvgNodeKind::Shape(shape) => (shape.path.clone(), shape.bbox),
            _ => (None, None),
        },
        None => {
            let style = element.computed_style()?;
            crate::svg::unrendered_shape_path(element, &style)
        }
    };
    // A rect with no width or no height draws nothing, but still has a
    // path, the line around it (Blink's `SVGRectElement::AsPath` is empty
    // only with neither).
    let line = bbox
        .filter(|b| is_rect && (b.width > 0.0 || b.height > 0.0))
        .map(|b| {
            let mut path = Path::new();
            path.add_rect(PointF::new(b.x, b.y), PointF::new(b.right(), b.bottom()));
            path
        });
    Some(path.or(line).unwrap_or_else(Path::new))
}

/// The characters of a `<text>`, or of a `<tspan>` or `<a>` in one, as
/// laid out, in logical order (Blink's `SVGTextQuery`); none for one not
/// rendered.
pub fn svg_text_chars(layout_view: &BoxFragment, element: SvgElement<'_, '_>) -> Vec<SvgTextChar> {
    let Some(found) = find(
        layout_view,
        &ScrollOffsets::default(),
        &StickyOffsets::default(),
        element,
    ) else {
        return Vec::new();
    };
    let text = match found.at {
        Laid::Node(SvgNode {
            kind: SvgNodeKind::Text(text),
            ..
        })
        | Laid::InText(text) => text,
        _ => return Vec::new(),
    };
    text.cells
        .iter()
        .filter(|cell| is_within(cell.node, element))
        .map(|cell| {
            let (sin, cos) = cell.rotation.to_radians().sin_cos();
            SvgTextChar {
                units: cell.units,
                start: cell.origin,
                end: PointF::new(
                    cell.origin.x + cell.advance * cos,
                    cell.origin.y + cell.advance * sin,
                ),
                corners: cell.corners(),
                rotation: cell.rotation,
                advance: cell.advance,
            }
        })
        .collect()
}

/// The rect around both, one with no width or height included (Blink's
/// `UnionEvenIfEmpty`).
fn union_even_if_empty(a: RectF, b: RectF) -> RectF {
    let (x, y) = (a.x.min(b.x), a.y.min(b.y));
    let (r, bottom) = (a.right().max(b.right()), a.bottom().max(b.bottom()));
    RectF::new(x, y, r - x, bottom - y)
}
