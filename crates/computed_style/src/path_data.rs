//! Path data as a path: SVG's `d`, which layout builds its shapes from,
//! and a canvas's `Path2D` from a string (Blink's `SVGPathBuilder`, which
//! `BuildPathFromString` drives for both).

use std::f64::consts::FRAC_PI_2;
use std::sync::Arc;

use base::numerics::{f32_from_f64, saturated_cast};
use gfx::{Path, PointF};

use crate::PathSegment;

fn pt(x: f64, y: f64) -> PointF {
    PointF::new(f32_from_f64(x), f32_from_f64(y))
}

/// An elliptical arc from angle `start` through `sweep` (radians, y
/// down), its ellipse turned by `rotation`, as conics of at most a quarter
/// turn each: for a unit circle's arc of angle θ, the control point where
/// the end tangents meet and the weight cos(θ / 2), which an affine map
/// keeps.
fn arc_conics(
    path: &mut Path,
    center: (f64, f64),
    radii: (f64, f64),
    rotation: f64,
    start: f64,
    sweep: f64,
) {
    let segments = (sweep.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0);
    let step = sweep / segments;
    let (sin_r, cos_r) = rotation.sin_cos();
    let map = |x: f64, y: f64| {
        let (x, y) = (x * radii.0, y * radii.1);
        pt(
            center.0 + x * cos_r - y * sin_r,
            center.1 + x * sin_r + y * cos_r,
        )
    };
    let weight = (step / 2.0).cos();
    let mut angle = start;
    for _ in 0..saturated_cast::<usize, _>(segments) {
        let mid = angle + step / 2.0;
        let end = angle + step;
        let control = map(mid.cos() / weight, mid.sin() / weight);
        path.conic_to(control, map(end.cos(), end.sin()), f32_from_f64(weight));
        angle = end;
    }
}

/// Path data added to `path`, every command absolute already
/// (`absolute_path`; style computes `d` so): the shorthand curves'
/// reflected control points, and arcs as conics (Blink's
/// `SVGPathBuilder`, with `SkPath::arcTo`'s handling of radii too small
/// and of none).
pub fn append_path_data(path: &mut Path, segments: &[PathSegment]) {
    let mut current = (0.0f64, 0.0f64);
    let mut start = current;
    // The last curve's second control point, for `S` (cubic) or `T`
    // (quadratic), if the last command was of that kind.
    let mut last_cubic: Option<(f64, f64)> = None;
    let mut last_quad: Option<(f64, f64)> = None;
    let f = |v: f32| f64::from(v);
    for &segment in segments {
        let (mut cubic, mut quad) = (None, None);
        match segment {
            PathSegment::MoveTo { x, y, .. } => {
                current = (f(x), f(y));
                start = current;
                path.move_to(pt(current.0, current.1));
            }
            PathSegment::LineTo { x, y, .. } => {
                current = (f(x), f(y));
                path.line_to(pt(current.0, current.1));
            }
            PathSegment::HorizontalLineTo { x, .. } => {
                current.0 = f(x);
                path.line_to(pt(current.0, current.1));
            }
            PathSegment::VerticalLineTo { y, .. } => {
                current.1 = f(y);
                path.line_to(pt(current.0, current.1));
            }
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
                ..
            } => {
                path.cubic_to(pt(f(x1), f(y1)), pt(f(x2), f(y2)), pt(f(x), f(y)));
                cubic = Some((f(x2), f(y2)));
                current = (f(x), f(y));
            }
            PathSegment::SmoothCurveTo { x2, y2, x, y, .. } => {
                let c1 = reflect(last_cubic, current);
                path.cubic_to(pt(c1.0, c1.1), pt(f(x2), f(y2)), pt(f(x), f(y)));
                cubic = Some((f(x2), f(y2)));
                current = (f(x), f(y));
            }
            PathSegment::Quadratic { x1, y1, x, y, .. } => {
                path.quad_to(pt(f(x1), f(y1)), pt(f(x), f(y)));
                quad = Some((f(x1), f(y1)));
                current = (f(x), f(y));
            }
            PathSegment::SmoothQuadratic { x, y, .. } => {
                let c = reflect(last_quad, current);
                path.quad_to(pt(c.0, c.1), pt(f(x), f(y)));
                quad = Some(c);
                current = (f(x), f(y));
            }
            PathSegment::EllipticalArc {
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
                ..
            } => {
                let to = (f(x), f(y));
                svg_arc(
                    path,
                    current,
                    to,
                    (f(rx), f(ry)),
                    f(x_axis_rotation),
                    large_arc,
                    sweep,
                );
                current = to;
            }
            PathSegment::ClosePath => {
                path.close();
                current = start;
            }
        }
        last_cubic = cubic;
        last_quad = quad;
    }
}

/// The control point reflected about `current`, or `current` itself when
/// the last command wasn't a curve of the kind.
fn reflect(control: Option<(f64, f64)>, current: (f64, f64)) -> (f64, f64) {
    control.map_or(current, |(x, y)| (2.0 * current.0 - x, 2.0 * current.1 - y))
}

/// An SVG arc from `from` to `to` (SVG 2's "out-of-range parameters": a
/// radius of 0 is a line, radii too small are scaled up), as conics.
fn svg_arc(
    path: &mut Path,
    from: (f64, f64),
    to: (f64, f64),
    radii: (f64, f64),
    rotation_degrees: f64,
    large_arc: bool,
    sweep: bool,
) {
    let arc = kurbo::SvgArc {
        from: kurbo::Point::new(from.0, from.1),
        to: kurbo::Point::new(to.0, to.1),
        radii: kurbo::Vec2::new(radii.0, radii.1),
        x_rotation: rotation_degrees.to_radians(),
        large_arc,
        sweep,
    };
    match kurbo::Arc::from_svg_arc(&arc) {
        Some(arc) => arc_conics(
            path,
            (arc.center.x, arc.center.y),
            (arc.radii.x, arc.radii.y),
            arc.x_rotation,
            arc.start_angle,
            arc.sweep_angle,
        ),
        // The same point, or a radius of 0: a line (none for the same
        // point).
        None if from != to => path.line_to(pt(to.0, to.1)),
        None => {}
    }
}

/// Every command made absolute, as Blink computes `d`: a relative one's
/// points offset from the current point, which a close puts back at the
/// subpath's start.
pub fn absolute_path(segments: &[PathSegment]) -> Arc<[PathSegment]> {
    let (mut cx, mut cy) = (0.0, 0.0);
    let (mut sx, mut sy) = (0.0, 0.0);
    let mut out = Vec::with_capacity(segments.len());
    for &segment in segments {
        let (dx, dy) = if segment_is_abs(segment) {
            (0.0, 0.0)
        } else {
            (cx, cy)
        };
        let absolute = match segment {
            PathSegment::MoveTo { x, y, .. } => {
                (sx, sy) = (x + dx, y + dy);
                PathSegment::MoveTo {
                    abs: true,
                    x: sx,
                    y: sy,
                }
            }
            PathSegment::LineTo { x, y, .. } => PathSegment::LineTo {
                abs: true,
                x: x + dx,
                y: y + dy,
            },
            PathSegment::HorizontalLineTo { x, .. } => PathSegment::HorizontalLineTo {
                abs: true,
                x: x + dx,
            },
            PathSegment::VerticalLineTo { y, .. } => PathSegment::VerticalLineTo {
                abs: true,
                y: y + dy,
            },
            PathSegment::CurveTo {
                x1,
                y1,
                x2,
                y2,
                x,
                y,
                ..
            } => PathSegment::CurveTo {
                abs: true,
                x1: x1 + dx,
                y1: y1 + dy,
                x2: x2 + dx,
                y2: y2 + dy,
                x: x + dx,
                y: y + dy,
            },
            PathSegment::SmoothCurveTo { x2, y2, x, y, .. } => PathSegment::SmoothCurveTo {
                abs: true,
                x2: x2 + dx,
                y2: y2 + dy,
                x: x + dx,
                y: y + dy,
            },
            PathSegment::Quadratic { x1, y1, x, y, .. } => PathSegment::Quadratic {
                abs: true,
                x1: x1 + dx,
                y1: y1 + dy,
                x: x + dx,
                y: y + dy,
            },
            PathSegment::SmoothQuadratic { x, y, .. } => PathSegment::SmoothQuadratic {
                abs: true,
                x: x + dx,
                y: y + dy,
            },
            PathSegment::EllipticalArc {
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x,
                y,
                ..
            } => PathSegment::EllipticalArc {
                abs: true,
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                x: x + dx,
                y: y + dy,
            },
            PathSegment::ClosePath => PathSegment::ClosePath,
        };
        (cx, cy) = match absolute {
            PathSegment::MoveTo { x, y, .. }
            | PathSegment::LineTo { x, y, .. }
            | PathSegment::CurveTo { x, y, .. }
            | PathSegment::SmoothCurveTo { x, y, .. }
            | PathSegment::Quadratic { x, y, .. }
            | PathSegment::SmoothQuadratic { x, y, .. }
            | PathSegment::EllipticalArc { x, y, .. } => (x, y),
            PathSegment::HorizontalLineTo { x, .. } => (x, cy),
            PathSegment::VerticalLineTo { y, .. } => (cx, y),
            PathSegment::ClosePath => (sx, sy),
        };
        out.push(absolute);
    }
    out.into()
}

/// Whether a command's points are absolute (a close's are).
pub fn segment_is_abs(segment: PathSegment) -> bool {
    match segment {
        PathSegment::MoveTo { abs, .. }
        | PathSegment::LineTo { abs, .. }
        | PathSegment::HorizontalLineTo { abs, .. }
        | PathSegment::VerticalLineTo { abs, .. }
        | PathSegment::CurveTo { abs, .. }
        | PathSegment::SmoothCurveTo { abs, .. }
        | PathSegment::Quadratic { abs, .. }
        | PathSegment::SmoothQuadratic { abs, .. }
        | PathSegment::EllipticalArc { abs, .. } => abs,
        PathSegment::ClosePath => true,
    }
}
