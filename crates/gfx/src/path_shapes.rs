//! The shapes a [`Path`] adds the way Skia's `SkPathBuilder` adds them:
//! arcs of an ellipse as conics (`arcTo(oval, startAngle, sweepAngle,
//! forceMoveTo)` and `SkConic::BuildUnitArc`), the arc between two
//! tangents (`arcTo(p1, p2, radius)`), rects, ovals and rounded rects
//! (`addRect`, `addOval`, `addRRect`, with `SkRRect::setRectRadii` fitting
//! the radii), and the whole path mapped by a transform. Ported with the
//! same steps and tolerances, so a canvas's paths have Chrome's points.

use base::numerics::f32_from_f64;

use crate::{Path, PathElement, PointF, RectF, SizeF, Transform};

/// Skia's `SK_ScalarNearlyZero`, the tolerance of `SkScalarNearlyEqual`.
const NEARLY_ZERO: f32 = 1.0 / 4096.0;

/// A quarter circle's conic weight (`SK_ScalarRoot2Over2`).
const QUARTER_WEIGHT: f32 = std::f32::consts::FRAC_1_SQRT_2;

fn nearly_equal(a: PointF, b: PointF) -> bool {
    (a.x - b.x).abs() <= NEARLY_ZERO && (a.y - b.y).abs() <= NEARLY_ZERO
}

/// `sk_float_midpoint`: halfway, in doubles.
fn midpoint(a: f32, b: f32) -> f32 {
    f32_from_f64((f64::from(a) + f64::from(b)) * 0.5)
}

/// `sin` and `cos` snapped to 0 when nearly 0 (`SkScalarSinSnapToZero`).
fn unit_vector(radians: f32) -> PointF {
    let snap = |v: f32| if v.abs() <= NEARLY_ZERO { 0.0 } else { v };
    PointF::new(snap(radians.cos()), snap(radians.sin()))
}

fn degrees_to_radians(degrees: f32) -> f32 {
    degrees * (std::f32::consts::PI / 180.0)
}

/// `v` made `length` long, in doubles (`SkPoint::setLength`); 0 if it
/// can't be.
fn with_length(v: PointF, length: f32) -> PointF {
    let (x, y) = (f64::from(v.x), f64::from(v.y));
    let scale = f64::from(length) / (x * x + y * y).sqrt();
    let scaled = PointF::new(f32_from_f64(x * scale), f32_from_f64(y * scale));
    if !scaled.x.is_finite() || !scaled.y.is_finite() || (scaled.x == 0.0 && scaled.y == 0.0) {
        return PointF::new(0.0, 0.0);
    }
    scaled
}

/// The corners of an `SkRect` as Skia walks a closed shape round it
/// (`SkPath_PointIterator`): from `start`, forward if `clockwise`, else
/// back.
struct Walk<const N: usize> {
    points: [PointF; N],
    current: usize,
    advance: usize,
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "N > 0 (a walk has points); current and advance are below N, so their sum is \
              below 2N"
)]
impl<const N: usize> Walk<N> {
    fn new(points: [PointF; N], clockwise: bool, start: usize) -> Self {
        Self {
            points,
            current: start % N,
            advance: if clockwise { 1 } else { N - 1 },
        }
    }

    fn current(&self) -> PointF {
        self.points[self.current]
    }

    fn next(&mut self) -> PointF {
        self.current = (self.current + self.advance) % N;
        self.current()
    }
}

/// `SkPath_RectPointIterator`: the corners from the top left, clockwise.
fn rect_corners(left: f32, top: f32, right: f32, bottom: f32) -> [PointF; 4] {
    [
        PointF::new(left, top),
        PointF::new(right, top),
        PointF::new(right, bottom),
        PointF::new(left, bottom),
    ]
}

/// An affine map as `SkMatrix` keeps one, `[a c e; b d f]`, applied the
/// way `SkMatrix::mapPoints` does.
fn map(m: [f32; 6], p: PointF) -> PointF {
    Transform(m).map_point(p)
}

/// What `SkRRect::setRectRadii` made of a rect and its radii.
enum RoundRect {
    Rect,
    Oval,
    /// Rounded, with these radii (top left, top right, bottom right,
    /// bottom left).
    Rounded([SizeF; 4]),
}

/// A corner with a radius of 0 (or less) is square: both radii 0
/// (`clamp_to_zero`); whether every corner is.
fn clamp_to_zero(radii: &mut [SizeF; 4]) -> bool {
    let mut square = true;
    for r in radii.iter_mut() {
        if r.width <= 0.0 || r.height <= 0.0 {
            *r = SizeF::new(0.0, 0.0);
        } else {
            square = false;
        }
    }
    square
}

/// `flush_to_zero`: a radius too small to change the sum of the two on a
/// side is 0.
fn flush_to_zero(a: &mut f32, b: &mut f32) {
    if *a + *b == *a {
        *b = 0.0;
    } else if *a + *b == *b {
        *a = 0.0;
    }
}

/// `SkScaleToSides::AdjustRadii`: the two radii on a side scaled, and the
/// larger made to fit exactly if rounding left them over the side.
fn adjust_radii(limit: f64, scale: f64, a: &mut f32, b: &mut f32) {
    *a = f32_from_f64(f64::from(*a) * scale);
    *b = f32_from_f64(f64::from(*b) * scale);
    if f64::from(*a + *b) > limit {
        let (min, max) = if *a > *b { (b, a) } else { (a, b) };
        let mut new_max = f32_from_f64(limit - f64::from(*min));
        while f64::from(new_max + *min) > limit {
            // The f32 just below it (`nextafter` towards 0).
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "min is the smaller radius of two scaled to about the limit, so \
                          new_max + min > limit means new_max > 0: its bits are at least 1"
            )]
            let below = new_max.to_bits() - 1;
            new_max = f32::from_bits(below);
        }
        *max = new_max;
    }
}

/// `SkRRect::setRectRadii` for a sorted, finite, non-empty `rect`: the
/// radii scaled to fit as CSS scales them (computed in doubles), then
/// what the shape turned out to be.
fn fit_radii(width: f32, height: f32, mut radii: [SizeF; 4]) -> RoundRect {
    if radii
        .iter()
        .any(|r| !r.width.is_finite() || !r.height.is_finite())
        || clamp_to_zero(&mut radii)
    {
        return RoundRect::Rect;
    }
    let (width, height) = (f64::from(width), f64::from(height));
    let min_scale = |a: f32, b: f32, limit: f64, current: f64| {
        let sum = f64::from(a) + f64::from(b);
        if sum > limit {
            current.min(limit / sum)
        } else {
            current
        }
    };
    let [tl, tr, br, bl] = &mut radii;
    let mut scale = 1.0;
    scale = min_scale(tl.width, tr.width, width, scale);
    scale = min_scale(tr.height, br.height, height, scale);
    scale = min_scale(br.width, bl.width, width, scale);
    scale = min_scale(bl.height, tl.height, height, scale);
    flush_to_zero(&mut tl.width, &mut tr.width);
    flush_to_zero(&mut tr.height, &mut br.height);
    flush_to_zero(&mut br.width, &mut bl.width);
    flush_to_zero(&mut bl.height, &mut tl.height);
    if scale < 1.0 {
        adjust_radii(width, scale, &mut tl.width, &mut tr.width);
        adjust_radii(height, scale, &mut tr.height, &mut br.height);
        adjust_radii(width, scale, &mut br.width, &mut bl.width);
        adjust_radii(height, scale, &mut bl.height, &mut tl.height);
    }
    if clamp_to_zero(&mut radii) {
        return RoundRect::Rect;
    }
    // `computeType`: the same radii at every corner, reaching halfway
    // or more, are an oval.
    let first = radii[0];
    if radii.iter().all(|r| *r == first)
        && first.width >= f32_from_f64(width) / 2.0
        && first.height >= f32_from_f64(height) / 2.0
    {
        return RoundRect::Oval;
    }
    RoundRect::Rounded(radii)
}

impl Path {
    /// The last point of the last move or segment (`SkPathBuilder::
    /// getLastPt`): after a close, the last point of the closed subpath,
    /// not its start. `None` with no points.
    pub fn last_point(&self) -> Option<PointF> {
        self.elements
            .iter()
            .rev()
            .find_map(|element| match *element {
                PathElement::MoveTo(p)
                | PathElement::LineTo(p)
                | PathElement::QuadTo(_, p)
                | PathElement::CubicTo(_, _, p)
                | PathElement::ConicTo(_, p, _) => Some(p),
                PathElement::Close => None,
            })
    }

    /// Where a segment added now would start (`SkPathBuilder::
    /// ensureMove`): the last point; after a close, the start of the
    /// subpath it closed; the origin in an empty path.
    fn segment_start(&self) -> PointF {
        match self.elements.last() {
            None => PointF::default(),
            Some(PathElement::Close) => self
                .elements
                .iter()
                .rev()
                .find_map(|element| match *element {
                    PathElement::MoveTo(p) => Some(p),
                    _ => None,
                })
                .unwrap_or_default(),
            Some(_) => self.last_point().unwrap_or_default(),
        }
    }

    /// `other`'s subpaths mapped by `transform`, after this path's
    /// (`SkPath::addPath`, appending): a canvas's `Path2D.addPath`.
    pub fn add_path(&mut self, other: &Path, transform: &Transform) {
        let mut other = other.clone();
        other.transform(transform);
        self.elements.extend(other.elements);
    }

    /// Every point mapped by `transform` (`SkPathBuilder::transform`; an
    /// affine map leaves conic weights as they are). A segment before
    /// any move starts at the origin, which is mapped too.
    pub fn transform(&mut self, transform: &Transform) {
        let first = self
            .elements
            .iter()
            .find(|e| !matches!(e, PathElement::Close));
        if first.is_some_and(|e| !matches!(e, PathElement::MoveTo(_))) {
            self.elements
                .insert(0, PathElement::MoveTo(PointF::default()));
        }
        let m = |p: PointF| transform.map_point(p);
        for element in &mut self.elements {
            *element = match *element {
                PathElement::MoveTo(p) => PathElement::MoveTo(m(p)),
                PathElement::LineTo(p) => PathElement::LineTo(m(p)),
                PathElement::QuadTo(c, p) => PathElement::QuadTo(m(c), m(p)),
                PathElement::CubicTo(c1, c2, p) => PathElement::CubicTo(m(c1), m(c2), m(p)),
                PathElement::ConicTo(c, p, w) => PathElement::ConicTo(m(c), m(p), w),
                PathElement::Close => PathElement::Close,
            };
        }
    }

    /// An arc of the ellipse in the oval from `top_left` to
    /// `bottom_right` (an `SkRect`'s corners), from `start` degrees
    /// clockwise from the x axis (y points down), `sweep` degrees on
    /// (back if negative), as conics: `SkPathBuilder::arcTo(oval,
    /// startAngle, sweepAngle, forceMoveTo)`. It starts a subpath if
    /// `force_move_to` or the path is empty; otherwise a line joins it,
    /// unless the path already ends (nearly) where it starts. A sweep of
    /// 360 degrees or more draws nothing but that point (Skia's; callers
    /// split a whole ellipse in two).
    pub fn arc_to(
        &mut self,
        top_left: PointF,
        bottom_right: PointF,
        start: f32,
        sweep: f32,
        force_move_to: bool,
    ) {
        let (left, top, right, bottom) = (top_left.x, top_left.y, bottom_right.x, bottom_right.y);
        let (width, height) = (right - left, bottom - top);
        if width < 0.0 || height < 0.0 {
            return;
        }
        let start = start % 360.0;
        let force_move_to = force_move_to || self.elements.is_empty();

        // `arc_is_lone_point`: Chrome moves into and out of ovals this way.
        let lone = if sweep == 0.0 && (start == 0.0 || start == 360.0) {
            Some(PointF::new(right, midpoint(top, bottom)))
        } else if width == 0.0 && height == 0.0 {
            Some(PointF::new(right, top))
        } else {
            None
        };
        if let Some(p) = lone {
            if force_move_to {
                self.move_to(p);
            } else {
                self.line_to(p);
            }
            return;
        }

        // `angles_to_unit_vectors`.
        let start_v = unit_vector(degrees_to_radians(start));
        let mut stop_radians = degrees_to_radians(start + sweep);
        let mut stop_v = unit_vector(stop_radians);
        // Nearly (not quite) 360 degrees can round to the same vectors,
        // which would draw nothing instead of nearly an ellipse.
        if start_v == stop_v {
            let magnitude = sweep.abs();
            if magnitude < 360.0 && magnitude > 359.0 {
                let delta = (1.0f32 / 512.0).copysign(sweep);
                while start_v == stop_v {
                    stop_radians -= delta;
                    stop_v = unit_vector(stop_radians);
                }
            }
        }
        let clockwise = sweep > 0.0;

        // A move to `p`, or a line unless the path is (nearly) there:
        // contiguous arcs of one oval add no lines between them.
        let add_point = |path: &mut Path, p: PointF| {
            if force_move_to {
                path.move_to(p);
            } else if !path.last_point().is_some_and(|last| nearly_equal(last, p)) {
                path.line_to(p);
            }
        };

        let (center_x, center_y) = (midpoint(left, right), midpoint(top, bottom));
        // Too small a sweep for the vectors: the point at its end.
        if start_v == stop_v {
            let end = degrees_to_radians(start + sweep);
            let p = PointF::new(
                center_x + width / 2.0 * end.cos(),
                center_y + height / 2.0 * end.sin(),
            );
            add_point(self, p);
            return;
        }

        // `build_arc_conics`: the unit arc scaled to the oval.
        let oval = [width / 2.0, 0.0, 0.0, height / 2.0, center_x, center_y];
        let conics = unit_arc(start_v, stop_v, clockwise, oval);
        match conics.first() {
            Some(first) => {
                add_point(self, first.points[0]);
                for Conic {
                    points: [_, control, end],
                    weight,
                } in conics
                {
                    self.conic_to(control, end, weight);
                }
            }
            None => add_point(self, map(oval, stop_v)),
        }
    }

    /// The arc of radius `radius` tangent to the line from the path's
    /// last point to `p1` and to the line from `p1` to `p2`, after a line
    /// to where it starts (`SkPathBuilder::arcTo(p1, p2, radius)`): a line
    /// to `p1` if the radius is 0 or the lines are one.
    pub fn tangent_arc_to(&mut self, p1: PointF, p2: PointF, radius: f32) {
        let start = self.segment_start();
        if radius == 0.0 {
            self.line_to(p1);
            return;
        }
        let normalize = |from: PointF, to: PointF| {
            let (x, y) = (
                f64::from(to.x) - f64::from(from.x),
                f64::from(to.y) - f64::from(from.y),
            );
            let length = (x * x + y * y).sqrt();
            (x / length, y / length)
        };
        let before = normalize(start, p1);
        let after = normalize(p1, p2);
        let cosh = before.0 * after.0 + before.1 * after.1;
        let sinh = before.0 * after.1 - before.1 * after.0;
        let finite = |(x, y): (f64, f64)| x.is_finite() && y.is_finite();
        if !finite(before) || !finite(after) || f32_from_f64(sinh).abs() <= NEARLY_ZERO {
            self.line_to(p1);
            return;
        }
        let to_f32 = base::numerics::f32_from_f64;
        let dist = to_f32(f64::from(radius) * (1.0 - cosh) / sinh).abs();
        let tangent = PointF::new(
            to_f32(f64::from(p1.x) - f64::from(dist) * before.0),
            to_f32(f64::from(p1.y) - f64::from(dist) * before.1),
        );
        let after = with_length(PointF::new(to_f32(after.0), to_f32(after.1)), dist);
        self.line_to(tangent);
        let weight = to_f32(0.5 + cosh * 0.5).sqrt();
        self.conic_to(p1, PointF::new(p1.x + after.x, p1.y + after.y), weight);
    }

    /// A closed subpath round the rect from `p0` to its opposite corner
    /// `p1`, from `p0` toward `(p1.x, p0.y)` (`addRect` of
    /// `SkRect::MakeLTRB`, unsorted, clockwise from index 0).
    pub fn add_rect(&mut self, p0: PointF, p1: PointF) {
        self.add_rect_from(rect_corners(p0.x, p0.y, p1.x, p1.y), true, 0);
    }

    fn add_rect_from(&mut self, corners: [PointF; 4], clockwise: bool, start: usize) {
        let mut walk = Walk::new(corners, clockwise, start);
        self.move_to(walk.current());
        for _ in 0..3 {
            let p = walk.next();
            self.line_to(p);
        }
        self.close();
    }

    /// The ellipse in `rect`, as a closed subpath of four quarter conics
    /// (`SkPathBuilder::addOval(oval, dir, startIndex)`): from the middle
    /// of its top (`start` 0), right (1, Blink's `Path::AddEllipse`, SVG's
    /// `<circle>` and `<ellipse>`), bottom (2) or left (3), clockwise or
    /// not.
    pub fn add_oval(&mut self, rect: RectF, clockwise: bool, start: usize) {
        self.add_oval_from(
            rect.x,
            rect.y,
            rect.x + rect.width,
            rect.y + rect.height,
            clockwise,
            start,
        );
    }

    fn add_oval_from(
        &mut self,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
        clockwise: bool,
        start: usize,
    ) {
        let (cx, cy) = (midpoint(left, right), midpoint(top, bottom));
        let mut oval = Walk::new(
            [
                PointF::new(cx, top),
                PointF::new(right, cy),
                PointF::new(cx, bottom),
                PointF::new(left, cy),
            ],
            clockwise,
            start,
        );
        // The corners start one further on counterclockwise; a walk takes
        // its start round the four, so only `start`'s remainder matters.
        #[expect(clippy::arithmetic_side_effects, reason = "start % 4 + 1 <= 4")]
        let first_corner = start % 4 + usize::from(!clockwise);
        let mut corners = Walk::new(
            rect_corners(left, top, right, bottom),
            clockwise,
            first_corner,
        );
        self.move_to(oval.current());
        for _ in 0..4 {
            let control = corners.next();
            let end = oval.next();
            self.conic_to(control, end, QUARTER_WEIGHT);
        }
        self.close();
    }

    /// `rect` with these radii (top left, top right, bottom right, bottom
    /// left), fitted as `SkRRect::setRectRadii` fits them, as a closed
    /// subpath: `addRRect` from index 0 (just after the top left corner),
    /// clockwise or not; a plain rect or an oval when it is one.
    pub fn add_round_rect(&mut self, rect: RectF, radii: [SizeF; 4], clockwise: bool) {
        let (mut left, mut top, mut right, mut bottom) =
            (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
        if left > right {
            std::mem::swap(&mut left, &mut right);
        }
        if top > bottom {
            std::mem::swap(&mut top, &mut bottom);
        }
        let empty = !(left < right && top < bottom);
        let shape = if empty {
            RoundRect::Rect
        } else {
            fit_radii(right - left, bottom - top, radii)
        };
        let radii = match shape {
            RoundRect::Rect => {
                return self.add_rect_from(rect_corners(left, top, right, bottom), clockwise, 0);
            }
            RoundRect::Oval => return self.add_oval_from(left, top, right, bottom, clockwise, 0),
            RoundRect::Rounded(radii) => radii,
        };
        // `SkPath_RRectPointIterator`: where each corner's curve meets the
        // sides, from just after the top left corner.
        let [tl, tr, br, bl] = radii;
        let mut ends = Walk::new(
            [
                PointF::new(left + tl.width, top),
                PointF::new(right - tr.width, top),
                PointF::new(right, top + tr.height),
                PointF::new(right, bottom - br.height),
                PointF::new(right - br.width, bottom),
                PointF::new(left + bl.width, bottom),
                PointF::new(left, bottom - bl.height),
                PointF::new(left, top + tl.height),
            ],
            clockwise,
            0,
        );
        let mut corners = Walk::new(
            rect_corners(left, top, right, bottom),
            clockwise,
            usize::from(!clockwise),
        );
        self.move_to(ends.current());
        if clockwise {
            // A side, then a corner, four times.
            for _ in 0..4 {
                let p = ends.next();
                self.line_to(p);
                let control = corners.next();
                let end = ends.next();
                self.conic_to(control, end, QUARTER_WEIGHT);
            }
        } else {
            // A corner, then a side; the close draws the last side.
            for i in 0..4 {
                let control = corners.next();
                let end = ends.next();
                self.conic_to(control, end, QUARTER_WEIGHT);
                if i < 3 {
                    let p = ends.next();
                    self.line_to(p);
                }
            }
        }
        self.close();
    }
}

/// A conic (`SkConic`): its start, control point and end, and its weight.
struct Conic {
    points: [PointF; 3],
    weight: f32,
}

/// `SkConic::BuildUnitArc`: the arc of the unit circle from `start` to
/// `stop` (unit vectors), one conic per whole quadrant and one for the
/// rest, mapped by `oval`. Empty if the two are (nearly) the same
/// direction.
fn unit_arc(start: PointF, stop: PointF, clockwise: bool, oval: [f32; 6]) -> Vec<Conic> {
    // Rotated so that `start` is (1, 0).
    let x = start.x * stop.x + start.y * stop.y;
    let mut y = start.x * stop.y - start.y * stop.x;
    let abs_y = y.abs();
    if abs_y <= NEARLY_ZERO && x > 0.0 && ((y >= 0.0 && clockwise) || (y <= 0.0 && !clockwise)) {
        return Vec::new();
    }
    if !clockwise {
        y = -y;
    }
    // The quadrant (x, y) is in: [0, 90), [90, 180), [180, 270) or
    // [270, 360) degrees.
    let quadrant = if y == 0.0 {
        2
    } else if x == 0.0 {
        if y > 0.0 { 1 } else { 3 }
    } else {
        match (x < 0.0, y < 0.0) {
            (false, false) => 0,
            (true, false) => 1,
            (true, true) => 2,
            (false, true) => 3,
        }
    };
    let p = PointF::new;
    let quadrant_points = [
        p(1.0, 0.0),
        p(1.0, 1.0),
        p(0.0, 1.0),
        p(-1.0, 1.0),
        p(-1.0, 0.0),
        p(-1.0, -1.0),
        p(0.0, -1.0),
        p(1.0, -1.0),
    ];
    // A whole quadrant from each even point to the next.
    let mut conics: Vec<Conic> = quadrant_points
        .windows(3)
        .step_by(2)
        .take(quadrant)
        .map(|points| Conic {
            points: [points[0], points[1], points[2]],
            weight: QUARTER_WEIGHT,
        })
        .collect();
    // What is left, less than 90 degrees.
    #[expect(clippy::arithmetic_side_effects, reason = "quadrant <= 3")]
    let last = quadrant_points[quadrant * 2];
    let end = p(x, y);
    let dot = last.x * end.x + last.y * end.y;
    if dot.is_nan() {
        return Vec::new();
    }
    if dot < 1.0 {
        // The bisector, as long as the control point is far: its length
        // is 1 / cos(θ / 2), and cos(θ / 2) is the weight too.
        let cos_half = ((1.0 + dot) / 2.0).sqrt();
        let control = with_length(p(last.x + x, last.y + y), 1.0 / cos_half);
        if last != control {
            conics.push(Conic {
                points: [last, control, end],
                weight: cos_half,
            });
        }
    }
    // Rotated to `start` (`setSinCos`), flipped for counterclockwise,
    // then onto the oval.
    let (sin, cos) = (start.y, start.x);
    let rotation = if clockwise {
        [cos, sin, -sin, cos, 0.0, 0.0]
    } else {
        [cos, sin, sin, -cos, 0.0, 0.0]
    };
    let [sx, _, _, sy, tx, ty] = oval;
    let [a, b, c, d, _, _] = rotation;
    let m = [sx * a, sy * b, sx * c, sy * d, tx, ty];
    for conic in &mut conics {
        conic.points = conic.points.map(|p| map(m, p));
    }
    conics
}
