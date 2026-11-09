//! `clip-path` as paths (Blink's `ClipPathClipper::PathBasedClip` and each
//! `BasicShape`'s `GetPath`): a shape laid on its reference box, a box's
//! own rounded shape, or a `<clipPath>`'s shapes (`svg::ClipSource`), in
//! the space of the element it clips.

use std::sync::Arc;

use computed_style::{BasicShape, ClipPath, ComputedStyle, GeometryBox, Position2D, ShapeRadius};
use gfx::{Path, PointF, RRectF, RectF, SizeF};

use crate::svg::ClipSource;
use computed_style::Sides;

/// What `clip-path` clips to: each a union of paths, filled by their
/// fill rules, all of them intersected: a shape or a box alone, or a
/// `<clipPath>`'s shapes, then those of the `<clipPath>` its own
/// `clip-path` names, and so on. A union of no paths clips everything
/// away.
pub type ClipPaths = Arc<[Arc<[Path]>]>;

/// `clip-path`'s paths for a CSS box `size` big with these edges, from its
/// border box's top left corner: `source` is the `<clipPath>` a `url()`
/// named, if it is one machopo can clip to (else nothing is clipped).
pub(crate) fn box_clip_paths(
    style: &ComputedStyle,
    size: SizeF,
    edges: [&Sides<f32>; 3],
    source: Option<&ClipSource>,
) -> Option<ClipPaths> {
    let reference = |which| css_reference_box(style, size, edges, which);
    let path = match &style.rare.clip_path {
        ClipPath::None => return None,
        ClipPath::Shape(shape, which) => basic_shape_path(shape, reference(*which).rect),
        ClipPath::Box(which) => rrect_path(reference(*which)),
        // Its user space the border box's, which is its object bounding
        // box too (Blink's `ClipPathClipper` for a CSS box).
        ClipPath::Url(_) => {
            let border_box = RectF::new(0.0, 0.0, size.width, size.height);
            return Some(source?.lay_out(size, border_box));
        }
    };
    Some(Arc::from([Arc::from([path])]))
}

/// The paths of an SVG element's `clip-path` other than a `url()`, in its
/// own space: its boxes are its object bounding box (`fill-box`, and the
/// CSS content and padding boxes), its stroke bounding box (`stroke-box`,
/// the border and margin boxes) and its viewport (`view-box`)
/// (css-masking 1).
pub(crate) fn svg_shape_clip_paths(
    clip_path: &ClipPath,
    fill_box: RectF,
    stroke_box: RectF,
    viewport: SizeF,
) -> Option<ClipPaths> {
    let reference = |which| match which {
        GeometryBox::FillBox | GeometryBox::ContentBox | GeometryBox::PaddingBox => fill_box,
        GeometryBox::StrokeBox | GeometryBox::BorderBox | GeometryBox::MarginBox => stroke_box,
        GeometryBox::ViewBox => RectF::new(0.0, 0.0, viewport.width, viewport.height),
    };
    let path = match clip_path {
        ClipPath::Shape(shape, which) => basic_shape_path(shape, reference(*which)),
        ClipPath::Box(which) => rrect_path(RRectF::from_rect(reference(*which))),
        ClipPath::None | ClipPath::Url(_) => return None,
    };
    Some(Arc::from([Arc::from([path])]))
}

fn rrect_path(rrect: RRectF) -> Path {
    let mut path = Path::new();
    path.add_round_rect(rrect.rect, rrect.radii, true);
    path
}

/// The box `reference` names on a CSS box, with its corners' curve. Its
/// `fill-box` is its content box, its `stroke-box` and `view-box` its
/// border box (css-masking 1).
fn css_reference_box(
    style: &ComputedStyle,
    size: SizeF,
    [border, padding, margin]: [&Sides<f32>; 3],
    reference: GeometryBox,
) -> RRectF {
    let border_box = RRectF::new(
        RectF::new(0.0, 0.0, size.width, size.height),
        border_radii(style, size),
    );
    let (b, p, m) = (border, padding, margin);
    match reference {
        GeometryBox::BorderBox | GeometryBox::StrokeBox | GeometryBox::ViewBox => border_box,
        GeometryBox::PaddingBox => border_box.inset(b.top, b.right, b.bottom, b.left),
        GeometryBox::ContentBox | GeometryBox::FillBox => border_box.inset(
            b.top + p.top,
            b.right + p.right,
            b.bottom + p.bottom,
            b.left + p.left,
        ),
        // Out by the margins, a rounded corner's radii with them, a
        // square one staying square.
        GeometryBox::MarginBox => {
            let r = border_box.rect;
            let rect = RectF::new(
                r.x - m.left,
                r.y - m.top,
                r.width + m.left + m.right,
                r.height + m.top + m.bottom,
            );
            let grow = |radius: SizeF, x: f32, y: f32| {
                if radius.width > 0.0 && radius.height > 0.0 {
                    SizeF::new(radius.width + x, radius.height + y)
                } else {
                    SizeF::new(0.0, 0.0)
                }
            };
            let [tl, tr, br, bl] = border_box.radii;
            RRectF::new(
                rect,
                [
                    grow(tl, m.left, m.top),
                    grow(tr, m.right, m.top),
                    grow(br, m.right, m.bottom),
                    grow(bl, m.left, m.bottom),
                ],
            )
        }
    }
}

/// `border-radius`'s corners for a box `size` big.
fn border_radii(style: &ComputedStyle, size: SizeF) -> [SizeF; 4] {
    let r = &style.rare.border_radius;
    [r.top_left, r.top_right, r.bottom_right, r.bottom_left]
        .map(|c| SizeF::new(c.x.resolve(size.width), c.y.resolve(size.height)))
}

/// `shape` laid on `reference`.
pub(crate) fn basic_shape_path(shape: &BasicShape, reference: RectF) -> Path {
    let (w, h) = (reference.width, reference.height);
    let at = |x: f32, y: f32| PointF::new(reference.x + x, reference.y + y);
    let mut path = Path::new();
    match shape {
        // Insets past each other leave nothing; the radii are of the
        // reference box's size (Blink's `BasicShapeInset::GetPath`).
        BasicShape::Inset { insets, radii } => {
            let [top, right, bottom, left] = [
                insets[0].resolve(h),
                insets[1].resolve(w),
                insets[2].resolve(h),
                insets[3].resolve(w),
            ];
            let rect = RectF::new(
                reference.x + left,
                reference.y + top,
                (w - left - right).max(0.0),
                (h - top - bottom).max(0.0),
            );
            let radii =
                radii.map(|(x, y)| SizeF::new(x.resolve(w).max(0.0), y.resolve(h).max(0.0)));
            let rrect = RRectF::new(rect, radii);
            path.add_round_rect(rrect.rect, rrect.radii, true);
        }
        BasicShape::Circle { radius, center } => {
            let center = center.unwrap_or(Position2D::CENTER);
            let (cx, cy) = (center.x.resolve(w), center.y.resolve(h));
            let r = match radius {
                ShapeRadius::Length(length) => length.resolve(((w * w + h * h) / 2.0).sqrt()),
                ShapeRadius::ClosestSide => cx
                    .abs()
                    .min((w - cx).abs())
                    .min(cy.abs())
                    .min((h - cy).abs()),
                ShapeRadius::FarthestSide => cx
                    .abs()
                    .max((w - cx).abs())
                    .max(cy.abs())
                    .max((h - cy).abs()),
            }
            .max(0.0);
            let c = at(cx, cy);
            path.add_oval(RectF::new(c.x - r, c.y - r, 2.0 * r, 2.0 * r), true, 1);
        }
        BasicShape::Ellipse { radii, center } => {
            let center = center.unwrap_or(Position2D::CENTER);
            let (cx, cy) = (center.x.resolve(w), center.y.resolve(h));
            let radius = |radius: ShapeRadius, c: f32, side: f32| {
                match radius {
                    ShapeRadius::Length(length) => length.resolve(side),
                    ShapeRadius::ClosestSide => c.abs().min((side - c).abs()),
                    ShapeRadius::FarthestSide => c.abs().max((side - c).abs()),
                }
                .max(0.0)
            };
            let (rx, ry) = (radius(radii.0, cx, w), radius(radii.1, cy, h));
            let c = at(cx, cy);
            path.add_oval(RectF::new(c.x - rx, c.y - ry, 2.0 * rx, 2.0 * ry), true, 1);
        }
        BasicShape::Polygon {
            fill_rule,
            round,
            points,
        } => {
            let points: Vec<PointF> = points
                .iter()
                .map(|(x, y)| at(x.resolve(w), y.resolve(h)))
                .collect();
            path = if *round > 0.0 && points.len() > 2 {
                rounded_polygon(&points, *round)
            } else {
                Path::polygons([points])
            }
            .with_fill_rule(*fill_rule);
        }
        BasicShape::Path {
            fill_rule,
            segments,
        } => {
            computed_style::append_path_data(&mut path, &computed_style::absolute_path(segments));
            path.transform(&gfx::Transform::translate(reference.x, reference.y));
            path = path.with_fill_rule(*fill_rule);
        }
    }
    path
}

/// A polygon whose corners are arcs of `radius` (css-shapes 2's
/// `polygon(round …)`): each a circle's arc tangent to the two edges at
/// the corner, as a conic from the corner, its tangent points no further
/// along an edge than half of it (so two corners' arcs never cross), the
/// radius smaller where that cuts them.
fn rounded_polygon(points: &[PointF], radius: f32) -> Path {
    let n = points.len();
    let mut path = Path::new();
    for i in 0..n {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "i < n: i + n - 1 and i + 1 are less than 2n, and n > 0"
        )]
        let (prev, corner, next) = (points[(i + n - 1) % n], points[i], points[(i + 1) % n]);
        let to = |p: PointF| {
            let (dx, dy) = (p.x - corner.x, p.y - corner.y);
            let len = (dx * dx + dy * dy).sqrt();
            (dx, dy, len)
        };
        let ((ux, uy, a), (vx, vy, b)) = (to(prev), to(next));
        // The angle between the edges; none at a corner on a straight
        // line or with an edge of no length, where the point is drawn as
        // it is.
        let cos = if a > 0.0 && b > 0.0 {
            ((ux * vx + uy * vy) / (a * b)).clamp(-1.0, 1.0)
        } else {
            -1.0
        };
        let angle = cos.acos();
        let tangent = if angle > 1e-4 && angle < std::f32::consts::PI - 1e-4 {
            (radius / (angle / 2.0).tan()).min(a / 2.0).min(b / 2.0)
        } else {
            0.0
        };
        let along = |dx: f32, dy: f32, len: f32| {
            PointF::new(corner.x + dx / len * tangent, corner.y + dy / len * tangent)
        };
        if tangent <= 0.0 {
            if i == 0 {
                path.move_to(corner);
            } else {
                path.line_to(corner);
            }
            continue;
        }
        let (start, end) = (along(ux, uy, a), along(vx, vy, b));
        if i == 0 {
            path.move_to(start);
        } else {
            path.line_to(start);
        }
        path.conic_to(corner, end, (angle / 2.0).sin());
    }
    path.close();
    path
}
