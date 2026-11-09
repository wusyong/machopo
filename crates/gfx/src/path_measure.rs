//! Lengths along a [`Path`], and the point a length along it reaches, the
//! two ways Chrome measures them for SVG:
//!
//! - Skia's `SkContourMeasure` (Blink's `Path::length` and
//!   `PointAndNormalAtLength`, what SVG's shapes other than `<path>` use):
//!   each contour's conics cut in halves until each piece is within half a
//!   unit of its chord, the chords' lengths added up; a point is found by
//!   interpolating the curve's `t` between the pieces' ends, then
//!   evaluating the curve there. Ported with the same steps, in floats, so
//!   a `<circle>`'s length is Chrome's (62.427 for a radius of 10, not
//!   2πr).
//! - Blink's `PathTraversalState` (`SVGPathQuery`, what a `<path>` uses on
//!   its path data): each curve as a cubic, cut in halves until its
//!   control polygon is within 0.00001 of its chord, each piece's length
//!   between the two; a point is found along the last piece's chord.

use base::numerics::f32_from_f64;

use crate::{Path, PathElement, PointF};

/// Skia's `kMaxTValue`: a curve's `t` from 0 to 1 as an integer.
const MAX_T: u32 = 0x3FFF_FFFF;

/// Skia's `kMaxRecursionDepth`.
const MAX_DEPTH: u32 = 8;

/// Skia's `CHEAP_DIST_LIMIT`, at a `resScale` of 1 (as Blink measures).
const TOLERANCE: f32 = 0.5;

/// Blink's `kPathSegmentLengthTolerance` and `kCurveSplitDepthLimit`.
const TRAVERSAL_TOLERANCE: f32 = 0.00001;
const TRAVERSAL_DEPTH: u32 = 20;

fn distance(a: PointF, b: PointF) -> f32 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    (dx * dx + dy * dy).sqrt()
}

/// Blink's `Vector2dF::Length`: in doubles.
fn blink_distance(a: PointF, b: PointF) -> f32 {
    let (dx, dy) = (f64::from(b.x - a.x), f64::from(b.y - a.y));
    f32_from_f64((dx * dx + dy * dy).sqrt())
}

/// `a + (b - a) * t`, Skia's `interp`.
fn interp(a: PointF, b: PointF, t: f32) -> PointF {
    PointF::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
}

/// A curve of a contour, as Skia keeps its points: the lines and conics
/// SVG's shapes are made of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Curve {
    Line([PointF; 2]),
    Conic([PointF; 3], f32),
}

impl Curve {
    /// The point at `t` (`SkConic::evalAt`, by its coefficients).
    fn eval(&self, t: f32) -> PointF {
        match *self {
            Curve::Line([a, b]) => interp(a, b, t),
            Curve::Conic([p0, p1, p2], w) => {
                let numer = |p0: f32, p1: f32, p2: f32| {
                    let p1w = p1 * w;
                    let (a, b) = (p2 - 2.0 * p1w + p0, 2.0 * (p1w - p0));
                    (a * t + b) * t + p0
                };
                let b = 2.0 * (w - 1.0);
                let denom = (-b * t + b) * t + 1.0;
                PointF::new(
                    numer(p0.x, p1.x, p2.x) / denom,
                    numer(p0.y, p1.y, p2.y) / denom,
                )
            }
        }
    }
}

/// One piece of a curve: where it ends along the contour, and its curve's
/// `t` there.
#[derive(Debug, Clone, Copy)]
struct Segment {
    distance: f32,
    /// Which curve of the contour it is a piece of.
    curve: usize,
    t: u32,
}

impl Segment {
    fn scalar_t(&self) -> f32 {
        self.t as f32 * (1.0 / MAX_T as f32)
    }
}

/// A contour measured as `SkContourMeasureIter::buildSegments` does.
#[derive(Debug, Default)]
struct Contour {
    curves: Vec<Curve>,
    segments: Vec<Segment>,
    length: f32,
}

impl Contour {
    fn line(&mut self, a: PointF, b: PointF) {
        let before = self.length;
        self.length += distance(a, b);
        if self.length > before {
            let curve = self.curves.len();
            self.curves.push(Curve::Line([a, b]));
            self.segments.push(Segment {
                distance: self.length,
                curve,
                t: MAX_T,
            });
        }
    }

    /// Adds a conic's pieces (`compute_conic_segs`); keeps it only if it
    /// has length.
    fn conic(&mut self, points: [PointF; 3], weight: f32) {
        let curve = Curve::Conic(points, weight);
        let before = self.length;
        let index = self.curves.len();
        conic_segments(
            &curve,
            &mut self.length,
            (0, points[0]),
            (MAX_T, points[2]),
            index,
            0,
            &mut self.segments,
        );
        if self.length > before {
            self.curves.push(curve);
        }
    }

    /// The point `distance` along it (`getPosTan`, `distanceToSegment`).
    fn point_at(&self, distance: f32) -> PointF {
        let distance = distance.clamp(0.0, self.length);
        // The first piece ending at or past it (`SkTSearch`).
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "a contour with length has a piece"
        )]
        let index = self
            .segments
            .partition_point(|segment| segment.distance < distance)
            .min(self.segments.len() - 1);
        let segment = self.segments[index];
        let (mut start_t, mut start_d) = (0.0, 0.0);
        if let Some(&previous) = index.checked_sub(1).and_then(|i| self.segments.get(i)) {
            start_d = previous.distance;
            if previous.curve == segment.curve {
                start_t = previous.scalar_t();
            }
        }
        let t = start_t
            + (segment.scalar_t() - start_t) * (distance - start_d) / (segment.distance - start_d);
        self.curves[segment.curve].eval(t)
    }
}

/// Whether a piece of `t` this long is worth cutting (`tspan_big_enough`).
fn big_enough(span: u32) -> bool {
    span >> 10 != 0
}

fn push_piece(segments: &mut Vec<Segment>, length: &mut f32, d: f32, curve: usize, t: u32) {
    let before = *length;
    *length += d;
    if *length > before {
        segments.push(Segment {
            distance: *length,
            curve,
            t,
        });
    }
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "t is at most MAX_T (2^30 - 1), so two add up within a u32; min_t <= max_t; \
              depth < MAX_DEPTH (8)"
)]
fn conic_segments(
    conic: &Curve,
    length: &mut f32,
    (min_t, min_p): (u32, PointF),
    (max_t, max_p): (u32, PointF),
    curve: usize,
    depth: u32,
    segments: &mut Vec<Segment>,
) {
    let half = (min_t + max_t) >> 1;
    let half_p = conic.eval(half as f32 * (1.0 / MAX_T as f32));
    // A piece whose middle isn't finite adds nothing; the others go on.
    if !(half_p.x.is_finite() && half_p.y.is_finite()) {
        return;
    }
    // How far the curve's middle is from its chord's (`conic_too_curvy`).
    let mid = PointF::new((min_p.x + max_p.x) * 0.5, (min_p.y + max_p.y) * 0.5);
    let too_curvy = (half_p.x - mid.x).abs().max((half_p.y - mid.y).abs()) > TOLERANCE;
    if depth < MAX_DEPTH && big_enough(max_t - min_t) && too_curvy {
        conic_segments(
            conic,
            length,
            (min_t, min_p),
            (half, half_p),
            curve,
            depth + 1,
            segments,
        );
        conic_segments(
            conic,
            length,
            (half, half_p),
            (max_t, max_p),
            curve,
            depth + 1,
            segments,
        );
    } else {
        push_piece(segments, length, distance(min_p, max_p), curve, max_t);
    }
}

impl Path {
    /// Its contours with length, as `SkContourMeasureIter` gives them: a
    /// move starts one (Skia moves to the last start before a segment that
    /// follows a close, and to the origin before a first segment without
    /// a move), and a closed one ends with the line back to its start.
    fn contours(&self) -> Vec<Contour> {
        let mut contours = Vec::new();
        let mut contour = Contour::default();
        let (mut current, mut start) = (PointF::new(0.0, 0.0), PointF::new(0.0, 0.0));
        let (mut closed, mut needs_move) = (false, true);
        let mut finish = |contour: &mut Contour, closed: bool, current: PointF, start: PointF| {
            if closed {
                contour.line(current, start);
            }
            let contour = std::mem::take(contour);
            if !contour.segments.is_empty() && contour.length.is_finite() {
                contours.push(contour);
            }
        };
        for element in self.elements() {
            if needs_move && !matches!(element, PathElement::MoveTo(_) | PathElement::Close) {
                finish(&mut contour, closed, current, start);
                closed = false;
                current = start;
            }
            needs_move = false;
            match *element {
                PathElement::MoveTo(p) => {
                    finish(&mut contour, closed, current, start);
                    closed = false;
                    (current, start) = (p, p);
                }
                PathElement::LineTo(p) => {
                    contour.line(current, p);
                    current = p;
                }
                PathElement::ConicTo(c, p, w) => {
                    contour.conic([current, c, p], w);
                    current = p;
                }
                // SVG's shapes make none: a `<path>`'s curves are measured
                // by `traversal_length`.
                PathElement::QuadTo(_, p) | PathElement::CubicTo(_, _, p) => {
                    contour.line(current, p);
                    current = p;
                }
                PathElement::Close => {
                    closed = true;
                    needs_move = true;
                }
            }
        }
        finish(&mut contour, closed, current, start);
        contours
    }

    /// Its length as Skia's `SkPathMeasure` measures it (Blink's
    /// `Path::length`): its contours' together. For the lines and conics of
    /// SVG's shapes; a quadratic or cubic counts as its chord.
    pub fn length(&self) -> f32 {
        self.contours().iter().map(|contour| contour.length).sum()
    }

    /// The point `length` along it as Skia finds it (Blink's
    /// `Path::PointAtLength`): on the contour that length reaches into;
    /// past them all, or with none, its first point (the origin without
    /// one).
    pub fn point_at_length(&self, length: f32) -> PointF {
        let mut length = length;
        for contour in self.contours() {
            if length <= contour.length {
                return contour.point_at(length);
            }
            length -= contour.length;
        }
        match self.elements().first() {
            Some(PathElement::MoveTo(p)) => *p,
            _ => PointF::new(0.0, 0.0),
        }
    }

    /// Its length as Blink's `SVGPathQuery` measures a `<path>`'s path
    /// data (`PathTraversalState`).
    pub fn traversal_length(&self) -> f32 {
        Traversal::run(self, None).total
    }

    /// The point `length` along it as Blink's `SVGPathQuery` finds it: on
    /// the chord of the piece of a curve that reaches it.
    pub fn traversal_point_at_length(&self, length: f32) -> PointF {
        Traversal::run(self, Some(length)).current
    }
}

/// Blink's `PathTraversalState`, totaling lengths or looking for the
/// point at `desired`.
struct Traversal {
    desired: Option<f32>,
    total: f32,
    start: PointF,
    current: PointF,
    previous: PointF,
    success: bool,
}

/// A cubic as Blink's `PathTraversalState` cuts it.
#[derive(Clone, Copy)]
struct Cubic {
    p: [PointF; 4],
    depth: u32,
}

fn midpoint(a: PointF, b: PointF) -> PointF {
    PointF::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0)
}

impl Cubic {
    fn polygon(&self) -> f32 {
        let p = self.p;
        blink_distance(p[0], p[1]) + blink_distance(p[1], p[2]) + blink_distance(p[2], p[3])
    }

    fn split(&self) -> (Cubic, Cubic) {
        let [start, c1, c2, end] = self.p;
        let middle = midpoint(c1, c2);
        let left1 = midpoint(start, c1);
        let left2 = midpoint(left1, middle);
        let right2 = midpoint(c2, end);
        let right1 = midpoint(right2, middle);
        let half = midpoint(left2, right1);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "only a cubic shallower than TRAVERSAL_DEPTH (20) is split"
        )]
        let depth = self.depth + 1;
        (
            Cubic {
                p: [start, left1, left2, half],
                depth,
            },
            Cubic {
                p: [half, right1, right2, end],
                depth,
            },
        )
    }
}

impl Traversal {
    fn run(path: &Path, desired: Option<f32>) -> Traversal {
        let origin = PointF::new(0.0, 0.0);
        let mut state = Traversal {
            desired,
            total: 0.0,
            start: origin,
            current: origin,
            previous: origin,
            success: false,
        };
        for element in path.elements() {
            if state.success {
                break;
            }
            let length = match *element {
                PathElement::MoveTo(p) => {
                    (state.current, state.start) = (p, p);
                    0.0
                }
                PathElement::LineTo(p) => {
                    let d = blink_distance(state.current, p);
                    state.current = p;
                    d
                }
                // Path data's quadratics and arcs are cubics to Blink's
                // `SVGPathNormalizer`.
                PathElement::QuadTo(c, p) => {
                    let from = state.current;
                    let third =
                        |a: PointF| PointF::new((a.x + 2.0 * c.x) / 3.0, (a.y + 2.0 * c.y) / 3.0);
                    state.cubic([from, third(from), third(p), p])
                }
                PathElement::ConicTo(c, p, w) => {
                    let from = state.current;
                    let k = 4.0 * w / (3.0 * (1.0 + w));
                    state.cubic([from, interp(from, c, k), interp(p, c, k), p])
                }
                PathElement::CubicTo(c1, c2, p) => state.cubic([state.current, c1, c2, p]),
                PathElement::Close => {
                    let d = blink_distance(state.current, state.start);
                    state.current = state.start;
                    d
                }
            };
            state.total += length;
            state.process_segment();
        }
        state
    }

    /// A cubic's length (`CurveLength`); looking for a point, only up to
    /// the piece that reaches it, with `previous` and `current` that
    /// piece's ends.
    fn cubic(&mut self, p: [PointF; 4]) -> f32 {
        let mut stack = vec![Cubic { p, depth: 0 }];
        let mut total = 0.0;
        while let Some(&top) = stack.last() {
            let polygon = top.polygon();
            let chord = blink_distance(top.p[0], top.p[3]);
            if polygon - chord > TRAVERSAL_TOLERANCE && top.depth < TRAVERSAL_DEPTH {
                let (left, right) = top.split();
                *stack.last_mut().unwrap() = right;
                stack.push(left);
            } else {
                // Between the chord and the control polygon, which the
                // curve's length is between (what matches Chrome's to the
                // last digit it gives; the polygon alone is long by
                // 0.0005 in 200).
                total += (polygon + chord) / 2.0;
                if let Some(desired) = self.desired {
                    self.previous = top.p[0];
                    self.current = top.p[3];
                    if self.total + total > desired {
                        return total;
                    }
                }
                stack.pop();
            }
        }
        if self.desired.is_none() {
            self.current = p[3];
        }
        total
    }

    /// Once the length reaches the one looked for, the point that far,
    /// along the last chord (`ProcessSegment`).
    fn process_segment(&mut self) {
        if let Some(desired) = self.desired
            && !self.success
            && self.total >= desired
        {
            let slope = (self.current.y - self.previous.y).atan2(self.current.x - self.previous.x);
            let offset = desired - self.total;
            self.current = PointF::new(
                self.current.x + offset * slope.cos(),
                self.current.y + offset * slope.sin(),
            );
            self.success = true;
        }
        self.previous = self.current;
    }
}
