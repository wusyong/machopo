//! A 2D context's and a `Path2D`'s `CanvasPath` methods (Blink's
//! `CanvasPath`), apart from any DOM: what each adds to the path it is
//! given (`CanvasPath`: a context's current default path, a `Path2D`'s),
//! as Blink and HTML say.

use base::numerics::f32_from_f64;
use gfx::{Path, PointF, RectF, SizeF, Transform};

use crate::CanvasData;
use crate::context::{IDENTITY, inverse, invertible, to_gfx};

// CanvasPath. Each takes its numbers as floats (Blink's
// `saturated_cast<float>`: one too big for a float is infinite) and does
// nothing with one that isn't finite.

/// What the `CanvasPath` methods build a path in (Blink's `CanvasPath`):
/// a context's current default path, or a `Path2D`'s, as each DOM keeps
/// them.
pub trait CanvasPath {
    /// Runs `f` on the path.
    fn with_path<R>(&mut self, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R;
}

/// A path being built, and the transform the points given are mapped by
/// if it can't be inverted: a context's, whose path is kept in the
/// bitmap's pixels then (shapes add nothing).
pub struct PathBuilder<'a> {
    pub path: &'a mut Path,
    pub singular: Option<Transform>,
}

impl PathBuilder<'_> {
    /// `p` where the path keeps it.
    fn point(&self, p: PointF) -> PointF {
        match self.singular {
            Some(transform) => transform.map_point(p),
            None => p,
        }
    }
}

/// A context's data as the path its `CanvasPath` methods build: its
/// current default path, kept in the bitmap's pixels while the transform
/// can't be inverted.
impl CanvasPath for &mut CanvasData {
    fn with_path<R>(&mut self, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R {
        let transform = self.state().transform;
        let singular = (!invertible(transform)).then(|| to_gfx(transform));
        f(&mut PathBuilder {
            path: &mut self.path,
            singular,
        })
    }
}

/// A `Path2D`'s path.
impl CanvasPath for &mut Path {
    fn with_path<R>(&mut self, f: impl FnOnce(&mut PathBuilder<'_>) -> R) -> R {
        f(&mut PathBuilder {
            path: self,
            singular: None,
        })
    }
}

/// Why a `CanvasPath` method throws (an `IndexSizeError` in each DOM's API,
/// with `message`).
#[derive(Debug, Clone, PartialEq)]
pub enum PathError {
    /// `arcTo`'s or `arc`'s radius is negative.
    NegativeRadius(f32),
    /// `ellipse`'s radius on the axis is negative.
    NegativeAxis { axis: &'static str, radius: f32 },
}

impl PathError {
    /// Blink's message.
    pub fn message(&self) -> String {
        match self {
            Self::NegativeRadius(radius) => format!("The radius provided ({radius}) is negative."),
            Self::NegativeAxis { axis, radius } => {
                format!("The {axis}-axis radius provided ({radius}) is negative.")
            }
        }
    }
}

/// `(x, y)` as floats, if both are finite.
fn point(x: f64, y: f64) -> Option<PointF> {
    let p = PointF::new(f32_from_f64(x), f32_from_f64(y));
    (p.x.is_finite() && p.y.is_finite()).then_some(p)
}

/// The path is one line, as Blink's `LineBuilder` holds it: moves, then a
/// move and a line.
fn only_line(path: &Path) -> Option<(PointF, PointF)> {
    use gfx::PathElement::{LineTo, MoveTo};
    let elements = path.elements();
    let (last, moves) = elements.split_last()?;
    let (&MoveTo(start), moves) = moves.split_last()? else {
        return None;
    };
    let LineTo(end) = *last else {
        return None;
    };
    moves
        .iter()
        .all(|e| matches!(e, MoveTo(_)))
        .then_some((start, end))
}

/// `closePath()`: a line back to the subpath's start, joined; nothing for
/// an empty path. A path of one line of no length is emptied (Blink's).
pub fn close_path(this: &mut impl CanvasPath) -> Result<(), PathError> {
    this.with_path(|b| {
        if b.path.is_empty() {
            return;
        }
        if only_line(b.path).is_some_and(|(start, end)| start == end) {
            *b.path = Path::new();
            return;
        }
        b.path.close();
    });
    Ok(())
}

pub fn move_to(this: &mut impl CanvasPath, x: f64, y: f64) -> Result<(), PathError> {
    if let Some(p) = point(x, y) {
        this.with_path(|b| {
            let p = b.point(p);
            b.path.move_to(p);
        });
    }
    Ok(())
}

/// A line to `p` (as given, not mapped yet); on an empty path a move
/// there first, so a line of no length (Blink's `LineBuilder`; HTML only
/// moves there).
fn line_to_point(b: &mut PathBuilder<'_>, p: PointF) {
    let p = b.point(p);
    if b.path.is_empty() {
        b.path.move_to(p);
    }
    b.path.line_to(p);
}

pub fn line_to(this: &mut impl CanvasPath, x: f64, y: f64) -> Result<(), PathError> {
    if let Some(p) = point(x, y) {
        this.with_path(|b| line_to_point(b, p));
    }
    Ok(())
}

/// With no point in the path yet, a curve starts at its (first) control
/// point, which Blink takes as given even under a transform that can't be
/// inverted.
pub fn quadratic_curve_to(
    this: &mut impl CanvasPath,
    cpx: f64,
    cpy: f64,
    x: f64,
    y: f64,
) -> Result<(), PathError> {
    let (Some(control), Some(end)) = (point(cpx, cpy), point(x, y)) else {
        return Ok(());
    };
    this.with_path(|b| {
        if b.path.last_point().is_none() {
            b.path.move_to(control);
        }
        let (control, end) = (b.point(control), b.point(end));
        b.path.quad_to(control, end);
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn bezier_curve_to(
    this: &mut impl CanvasPath,
    cp1x: f64,
    cp1y: f64,
    cp2x: f64,
    cp2y: f64,
    x: f64,
    y: f64,
) -> Result<(), PathError> {
    let (Some(c1), Some(c2), Some(end)) = (point(cp1x, cp1y), point(cp2x, cp2y), point(x, y))
    else {
        return Ok(());
    };
    this.with_path(|b| {
        if b.path.last_point().is_none() {
            b.path.move_to(c1);
        }
        let (c1, c2, end) = (b.point(c1), b.point(c2), b.point(end));
        b.path.cubic_to(c1, c2, end);
    });
    Ok(())
}

fn negative_radius(radius: f32) -> PathError {
    PathError::NegativeRadius(radius)
}

/// `arcTo(x1, y1, x2, y2, radius)`: a line to where an arc of `radius`
/// meets the line from the last point to `(x1, y1)`, then the arc, to
/// where it meets the line from there to `(x2, y2)`. With no point yet a
/// move to `(x1, y1)`; a line to it if it is the last point or `(x2,
/// y2)`, or the radius is 0. An `IndexSizeError` for a negative radius.
#[allow(clippy::too_many_arguments)]
pub fn arc_to(
    this: &mut impl CanvasPath,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    radius: f64,
) -> Result<(), PathError> {
    let radius = f32_from_f64(radius);
    let (Some(p1), Some(p2)) = (point(x1, y1), point(x2, y2)) else {
        return Ok(());
    };
    if !radius.is_finite() {
        return Ok(());
    }
    if radius < 0.0 {
        return Err(negative_radius(radius));
    }
    this.with_path(|b| {
        let (q1, q2) = (b.point(p1), b.point(p2));
        match b.path.last_point() {
            None => b.path.move_to(q1),
            Some(last) if q1 == last || q1 == q2 || radius == 0.0 => line_to_point(b, p1),
            Some(_) => b.path.tangent_arc_to(q1, q2, radius),
        }
    });
    Ok(())
}

const TWO_PI: f32 = 2.0 * std::f32::consts::PI;

/// The start angle made one of `[0, 2π)`, the end moved with it (Blink's
/// `CanonicalizeAngle`).
fn canonicalize_angles(start: f32, end: f32) -> (f32, f32) {
    let mut new_start = start % TWO_PI;
    if new_start < 0.0 {
        new_start += TWO_PI;
        if new_start >= TWO_PI {
            new_start -= TWO_PI;
        }
    }
    (new_start, end + (new_start - start))
}

/// The end angle for a sweep of at most a whole turn, the way the arc
/// goes (Blink's `AdjustEndAngle`): a whole turn from `start` when the
/// two are a turn or more apart that way; otherwise the end, the way the
/// arc goes.
fn adjust_end_angle(start: f32, end: f32, counterclockwise: bool) -> f32 {
    if !counterclockwise && end - start >= TWO_PI {
        start + TWO_PI
    } else if counterclockwise && start - end >= TWO_PI {
        start - TWO_PI
    } else if !counterclockwise && start > end {
        start + (TWO_PI - (start - end) % TWO_PI)
    } else if counterclockwise && start < end {
        start - (TWO_PI - (end - start) % TWO_PI)
    } else {
        end
    }
}

/// `base::RadToDeg`.
fn degrees(radians: f32) -> f32 {
    radians * 180.0 / std::f32::consts::PI
}

/// The arc of the ellipse round `center` from `start` to `end` radians
/// (adjusted, `start` in `[0, 2π)`), rotated by `rotation` (Blink's
/// `PathBuilder::AddEllipse`): Skia's arcs of the oval; a whole ellipse in
/// two halves, since one arc of 360 degrees draws nothing. A rotated one
/// is added in its own space: the path is mapped there and back.
fn add_ellipse(
    path: &mut Path,
    center: PointF,
    radii: (f32, f32),
    rotation: f32,
    start: f32,
    end: f32,
) {
    if rotation != 0.0 {
        let (sin, cos) = f64::from(rotation).sin_cos();
        let (x, y) = (f64::from(center.x), f64::from(center.y));
        let ellipse = [cos, sin, -sin, cos, x, y];
        let back = inverse(ellipse).unwrap_or(IDENTITY);
        path.transform(&to_gfx(back));
        add_ellipse(path, PointF::default(), radii, 0.0, start, end);
        path.transform(&to_gfx(ellipse));
        return;
    }
    let (rx, ry) = radii;
    let top_left = PointF::new(center.x - rx, center.y - ry);
    let bottom_right = PointF::new(center.x + rx, center.y + ry);
    let (start_degrees, sweep) = (degrees(start), degrees(end - start));
    // Skia's `SkScalarNearlyEqual`, as Blink's `WebCoreFloatNearlyEqual`.
    if (sweep.abs() - 360.0).abs() <= 1.0 / 4096.0 {
        let half = 180f32.copysign(sweep);
        path.arc_to(top_left, bottom_right, start_degrees, half, false);
        path.arc_to(top_left, bottom_right, start_degrees + half, half, false);
    } else {
        path.arc_to(top_left, bottom_right, start_degrees, sweep, false);
    }
}

/// `arc(x, y, radius, startAngle, endAngle, counterclockwise)`: a line to
/// the arc's start, then the arc of the circle; with a radius of 0, or
/// the same angles, only the line. An `IndexSizeError` for a negative
/// radius; nothing under a transform that can't be inverted.
#[allow(clippy::too_many_arguments)]
pub fn arc(
    this: &mut impl CanvasPath,
    x: f64,
    y: f64,
    radius: f64,
    start: f64,
    end: f64,
    counterclockwise: bool,
) -> Result<(), PathError> {
    let (radius, start, end) = (f32_from_f64(radius), f32_from_f64(start), f32_from_f64(end));
    let Some(center) = point(x, y) else {
        return Ok(());
    };
    if ![radius, start, end].iter().all(|n| n.is_finite()) {
        return Ok(());
    }
    if radius < 0.0 {
        return Err(negative_radius(radius));
    }
    this.with_path(|b| {
        if b.singular.is_some() {
            return;
        }
        if radius == 0.0 || start == end {
            let p = PointF::new(
                center.x + radius * start.cos(),
                center.y + radius * start.sin(),
            );
            line_to_point(b, p);
            return;
        }
        let (start, end) = canonicalize_angles(start, end);
        let end = adjust_end_angle(start, end, counterclockwise);
        add_ellipse(b.path, center, (radius, radius), 0.0, start, end);
    });
    Ok(())
}

/// Lines through a flat ellipse's points at `start`, every quarter turn
/// between, and `end` (Blink's `DegenerateEllipse`): the ellipse is a
/// line, or a point.
#[allow(clippy::too_many_arguments)]
fn degenerate_ellipse(
    b: &mut PathBuilder<'_>,
    center: PointF,
    rx: f32,
    ry: f32,
    rotation: f32,
    start: f32,
    end: f32,
    counterclockwise: bool,
) {
    let (sin, cos) = f64::from(rotation).sin_cos();
    let at = |angle: f32| {
        let (x, y) = (f64::from(rx * angle.cos()), f64::from(ry * angle.sin()));
        let (x, y) = (
            f32_from_f64(cos * x - sin * y),
            f32_from_f64(sin * x + cos * y),
        );
        PointF::new(center.x + x, center.y + y)
    };
    line_to_point(b, at(start));
    if (rx == 0.0 && ry == 0.0) || start == end {
        return;
    }
    const QUARTER: f32 = std::f32::consts::FRAC_PI_2;
    if !counterclockwise {
        let mut angle = start - start % QUARTER + QUARTER;
        while angle < end {
            line_to_point(b, at(angle));
            angle += QUARTER;
        }
    } else {
        let mut angle = start - start % QUARTER;
        while angle > end {
            line_to_point(b, at(angle));
            angle -= QUARTER;
        }
    }
    line_to_point(b, at(end));
}

/// `ellipse(x, y, radiusX, radiusY, rotation, startAngle, endAngle,
/// counterclockwise)`: as `arc`, of an ellipse rotated by `rotation`; one
/// with a radius of 0 is lines. An `IndexSizeError` for a negative
/// radius; nothing under a transform that can't be inverted.
#[allow(clippy::too_many_arguments)]
pub fn ellipse(
    this: &mut impl CanvasPath,
    x: f64,
    y: f64,
    radius_x: f64,
    radius_y: f64,
    rotation: f64,
    start: f64,
    end: f64,
    counterclockwise: bool,
) -> Result<(), PathError> {
    let (rx, ry, rotation, start, end) = (
        f32_from_f64(radius_x),
        f32_from_f64(radius_y),
        f32_from_f64(rotation),
        f32_from_f64(start),
        f32_from_f64(end),
    );
    let Some(center) = point(x, y) else {
        return Ok(());
    };
    if ![rx, ry, rotation, start, end].iter().all(|n| n.is_finite()) {
        return Ok(());
    }
    for (radius, axis) in [(rx, "major"), (ry, "minor")] {
        if radius < 0.0 {
            return Err(PathError::NegativeAxis { axis, radius });
        }
    }
    this.with_path(|b| {
        if b.singular.is_some() {
            return;
        }
        let (start, end) = canonicalize_angles(start, end);
        let end = adjust_end_angle(start, end, counterclockwise);
        if rx == 0.0 || ry == 0.0 || start == end {
            degenerate_ellipse(b, center, rx, ry, rotation, start, end, counterclockwise);
            return;
        }
        add_ellipse(b.path, center, (rx, ry), rotation, start, end);
    });
    Ok(())
}

/// The rect from `(x, y)` `w` wide and `h` high, either way, as `[x, y,
/// w, h]` in floats, if all four are finite.
fn path_rect(x: f64, y: f64, w: f64, h: f64) -> Option<[f32; 4]> {
    let rect = [x, y, w, h].map(f32_from_f64);
    rect.iter().all(|n| n.is_finite()).then_some(rect)
}

/// `rect(x, y, w, h)`: a closed subpath from `(x, y)` along the top (the
/// way `w` goes) and round; with neither width nor height, a move there.
/// Nothing under a transform that can't be inverted.
pub fn rect_path(
    this: &mut impl CanvasPath,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
) -> Result<(), PathError> {
    let Some([x, y, w, h]) = path_rect(x, y, w, h) else {
        return Ok(());
    };
    this.with_path(|b| {
        if b.singular.is_some() {
            return;
        }
        if w == 0.0 && h == 0.0 {
            b.path.move_to(PointF::new(x, y));
            return;
        }
        b.path
            .add_rect(PointF::new(x, y), PointF::new(x + w, y + h));
    });
    Ok(())
}

/// One of `roundRect`'s radii as script gives it (WebIDL's `(unrestricted
/// double or DOMPointInit)`): one number for both axes, or a point's `x`
/// and `y`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Radius {
    Both(f64),
    Point { x: f64, y: f64 },
}

/// `roundRect`'s radii as Blink checks them: a `RangeError`'s message for
/// fewer than one or more than four, or a negative one (after the count);
/// each one's radii as floats, `None` where one isn't finite (which makes
/// the call do nothing).
pub fn round_rect_radii(radii: &[Radius]) -> Result<Vec<Option<SizeF>>, String> {
    if radii.is_empty() || radii.len() > 4 {
        return Err(format!(
            "{} radii provided. Between one and four radii are necessary.",
            radii.len()
        ));
    }
    let negative = |which: &str, value: f32| format!("{which}adius value {value} is negative.");
    radii
        .iter()
        .map(|radius| match *radius {
            Radius::Point { x, y } => {
                let (x, y) = (f32_from_f64(x), f32_from_f64(y));
                if !x.is_finite() || !y.is_finite() {
                    return Ok(None);
                }
                if x < 0.0 {
                    return Err(negative("X-r", x));
                }
                if y < 0.0 {
                    return Err(negative("Y-r", y));
                }
                Ok(Some(SizeF::new(x, y)))
            }
            Radius::Both(r) => {
                let r = f32_from_f64(r);
                if !r.is_finite() {
                    return Ok(None);
                }
                if r < 0.0 {
                    return Err(negative("R", r));
                }
                Ok(Some(SizeF::new(r, r)))
            }
        })
        .collect()
}

/// `roundRect(x, y, w, h, radii)`'s path, its one to four radii given
/// (each `None` where one isn't finite: then nothing): a rect with no
/// width or no height, else the rounded rect (negative sizes going the
/// other way round, the corners mirrored), then a move to its start.
pub fn round_rect(
    this: &mut impl CanvasPath,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    radii: &[Option<SizeF>],
) {
    if this.with_path(|b| b.singular.is_some()) {
        return;
    }
    let Some([mut x, mut y, mut w, mut h]) = path_rect(x, y, w, h) else {
        return;
    };
    let Some(r) = radii.iter().copied().collect::<Option<Vec<SizeF>>>() else {
        return;
    };
    this.with_path(|b| {
        if w == 0.0 || h == 0.0 {
            b.path
                .add_rect(PointF::new(x, y), PointF::new(x + w, y + h));
            return;
        }
        // Top left, top right, bottom left, bottom right (Blink's order).
        let mut corners = match r[..] {
            [all] => [all; 4],
            [a, b] => [a, b, b, a],
            [a, b, c] => [a, b, b, c],
            [a, b, c, d] => [a, b, d, c],
            _ => unreachable!("one to four radii"),
        };
        // A negative size goes the other way round, the corners mirrored.
        let mut clockwise = true;
        if w < 0.0 {
            clockwise = false;
            x += w;
            w = -w;
            corners.swap(0, 1);
            corners.swap(2, 3);
        }
        if h < 0.0 {
            clockwise = !clockwise;
            y += h;
            h = -h;
            corners.swap(0, 2);
            corners.swap(1, 3);
        }
        let [tl, tr, bl, br] = corners;
        b.path
            .add_round_rect(RectF::new(x, y, w, h), [tl, tr, br, bl], clockwise);
        b.path.move_to(PointF::new(x, y));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One to four, the count before the numbers; a negative one a
    /// `RangeError`, one that isn't finite none.
    #[test]
    fn radii() {
        let both = Radius::Both;
        assert!(round_rect_radii(&[]).is_err());
        assert_eq!(
            round_rect_radii(&[both(-1.0); 5]),
            Err("5 radii provided. Between one and four radii are necessary.".to_owned())
        );
        assert_eq!(
            round_rect_radii(&[both(1.0), Radius::Point { x: 2.0, y: 3.0 }]),
            Ok(vec![Some(SizeF::new(1.0, 1.0)), Some(SizeF::new(2.0, 3.0))])
        );
        assert_eq!(round_rect_radii(&[both(f64::NAN)]), Ok(vec![None]));
        assert_eq!(
            round_rect_radii(&[Radius::Point { x: -1.0, y: 2.0 }]),
            Err("X-radius value -1 is negative.".to_owned())
        );
        assert_eq!(
            round_rect_radii(&[Radius::Point { x: 1.0, y: -2.0 }]),
            Err("Y-radius value -2 is negative.".to_owned())
        );
        assert_eq!(
            round_rect_radii(&[both(-1.0)]),
            Err("Radius value -1 is negative.".to_owned())
        );
    }
}
