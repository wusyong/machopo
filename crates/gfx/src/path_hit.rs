//! Whether a point is in a path's fill or in its stroke (Blink's
//! `Path::Contains` and `StrokeContains`, which Skia answers), with kurbo:
//! SVG's hit testing, `isPointInFill` and `isPointInStroke`, and a
//! canvas's `isPointInPath` and `isPointInStroke`.

use kurbo::Shape;

use crate::{FillRule, Path, PointF};

/// The shape of a stroke's ends (`SkPaint::Cap`; SVG's `stroke-linecap`,
/// canvas's `lineCap`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    /// Square, at the end.
    #[default]
    Butt,
    /// A half circle past the end, which makes a dash of length 0 a dot
    /// as wide as the stroke.
    Round,
    /// A half square past the end.
    Square,
}

/// The shape where a stroke's segments meet (`SkPaint::Join`; SVG's
/// `stroke-linejoin`, canvas's `lineJoin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineJoin {
    #[default]
    Miter,
    Round,
    Bevel,
}

/// How a path is stroked, for the area its stroke covers (Blink's
/// `StrokeData`).
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeStyle<'a> {
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
    /// The dashes' intervals (an even number of them, the drawn ones
    /// first) and phase; `None` for a solid line.
    pub dash: Option<(&'a [f32], f32)>,
}

impl Path {
    /// Whether `point` is inside by `rule`, or on the outline (Skia's
    /// `SkPath::contains` counts the edges); never for a point that isn't
    /// finite.
    pub fn contains(&self, point: PointF, rule: FillRule) -> bool {
        let Some(at) = kurbo_point(point) else {
            return false;
        };
        let path = self.to_kurbo();
        let winding = path.winding(at);
        let inside = match rule {
            FillRule::NonZero => winding != 0,
            FillRule::EvenOdd => winding % 2 != 0,
        };
        inside || on_outline(&path, at)
    }

    /// Whether `point` is in the area the path's stroke covers, or on its
    /// edge: its width, ends, joins and dashes, the outline within
    /// `tolerance` of the true one; never without a width.
    pub fn stroke_contains(&self, point: PointF, style: &StrokeStyle<'_>, tolerance: f64) -> bool {
        let Some(at) = kurbo_point(point) else {
            return false;
        };
        style.width > 0.0 && {
            let outline = stroke_outline(&self.to_kurbo(), style, tolerance);
            outline.winding(at) != 0 || on_outline(&outline, at)
        }
    }
}

fn kurbo_point(point: PointF) -> Option<kurbo::Point> {
    (point.x.is_finite() && point.y.is_finite())
        .then(|| kurbo::Point::new(point.x.into(), point.y.into()))
}

/// Whether `at` is on `path`'s outline: inside, as Skia's
/// `SkPath::contains` counts it (a winding number leaves out the right and
/// bottom edges).
fn on_outline(path: &kurbo::BezPath, at: kurbo::Point) -> bool {
    use kurbo::ParamCurveNearest;
    path.segments()
        .any(|segment| segment.nearest(at, 1e-6).distance_sq < 1e-6)
}

/// The stroke as an area, to fill non-zero.
fn stroke_outline(
    path: &kurbo::BezPath,
    style: &StrokeStyle<'_>,
    tolerance: f64,
) -> kurbo::BezPath {
    let cap = match style.cap {
        LineCap::Butt => kurbo::Cap::Butt,
        LineCap::Round => kurbo::Cap::Round,
        LineCap::Square => kurbo::Cap::Square,
    };
    let join = match style.join {
        LineJoin::Miter => kurbo::Join::Miter,
        LineJoin::Round => kurbo::Join::Round,
        LineJoin::Bevel => kurbo::Join::Bevel,
    };
    let mut stroke = kurbo::Stroke::new(style.width.into())
        .with_caps(cap)
        .with_join(join)
        .with_miter_limit(style.miter_limit.into());
    if let Some((intervals, phase)) = style.dash {
        let pattern: Vec<f64> = intervals.iter().map(|&i| f64::from(i)).collect();
        stroke = stroke.with_dashes(phase.into(), pattern);
    }
    kurbo::stroke(
        path.iter(),
        &stroke,
        &kurbo::StrokeOpts::default(),
        tolerance,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transform;

    fn square(x: f32, y: f32, size: f32) -> Path {
        let mut path = Path::new();
        path.add_rect(PointF::new(x, y), PointF::new(x + size, y + size));
        path
    }

    fn solid(width: f32) -> StrokeStyle<'static> {
        StrokeStyle {
            width,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 10.0,
            dash: None,
        }
    }

    /// Inside by the rule, the edges (all four) too, never a point not
    /// finite.
    #[test]
    fn contains_by_the_rule_and_on_the_edges() {
        let mut nested = square(0.0, 0.0, 10.0);
        nested.add_path(&square(2.0, 2.0, 6.0), &Transform::IDENTITY);
        let at = |x, y| PointF::new(x, y);
        assert!(nested.contains(at(5.0, 5.0), FillRule::NonZero));
        assert!(!nested.contains(at(5.0, 5.0), FillRule::EvenOdd));
        assert!(nested.contains(at(1.0, 5.0), FillRule::EvenOdd));
        assert!(!nested.contains(at(11.0, 5.0), FillRule::NonZero));
        for edge in [at(0.0, 5.0), at(10.0, 5.0), at(5.0, 0.0), at(5.0, 10.0)] {
            assert!(nested.contains(edge, FillRule::NonZero), "{edge:?}");
        }
        assert!(!nested.contains(at(f32::NAN, 5.0), FillRule::NonZero));
    }

    /// The stroke's area: half the width either side, its caps, its
    /// dashes; nothing without a width.
    #[test]
    fn stroke_contains_takes_the_style() {
        let mut line = Path::new();
        line.move_to(PointF::new(10.0, 10.0));
        line.line_to(PointF::new(30.0, 10.0));
        let hit =
            |style: &StrokeStyle<'_>, x, y| line.stroke_contains(PointF::new(x, y), style, 0.05);
        assert!(hit(&solid(2.0), 20.0, 10.9));
        assert!(!hit(&solid(2.0), 20.0, 11.1));
        assert!(!hit(&solid(2.0), 9.5, 10.0));
        assert!(hit(
            &StrokeStyle {
                cap: LineCap::Square,
                ..solid(2.0)
            },
            9.5,
            10.0
        ));
        assert!(!hit(&solid(0.0), 20.0, 10.0));
        let dashes = [4.0, 4.0];
        let dashed = StrokeStyle {
            dash: Some((&dashes, 0.0)),
            ..solid(2.0)
        };
        assert!(hit(&dashed, 12.0, 10.0));
        assert!(!hit(&dashed, 16.0, 10.0));
        // The phase moves them along.
        let moved = StrokeStyle {
            dash: Some((&dashes, 4.0)),
            ..solid(2.0)
        };
        assert!(hit(&moved, 16.0, 10.0));
        assert!(!hit(&solid(2.0), f32::INFINITY, 10.0));
    }

    /// A sharp corner (about 19 degrees): a miter reaches about six
    /// widths along the bisector, within a limit of 10 but past 4; a bevel
    /// doesn't.
    #[test]
    fn stroke_contains_takes_the_join_and_its_limit() {
        let mut corner = Path::new();
        corner.move_to(PointF::new(0.0, 0.0));
        corner.line_to(PointF::new(20.0, 0.0));
        corner.line_to(PointF::new(0.0, 7.0));
        let tip = PointF::new(23.94, -0.67);
        assert!(corner.stroke_contains(tip, &solid(2.0), 0.05));
        let bevel = StrokeStyle {
            join: LineJoin::Bevel,
            ..solid(2.0)
        };
        assert!(!corner.stroke_contains(tip, &bevel, 0.05));
        let limited = StrokeStyle {
            miter_limit: 4.0,
            ..solid(2.0)
        };
        assert!(!corner.stroke_contains(tip, &limited, 0.05));
    }

    /// `add_path` appends the other's subpaths through the transform.
    #[test]
    fn add_path_appends_through_the_transform() {
        let mut path = square(0.0, 0.0, 1.0);
        path.add_path(&square(0.0, 0.0, 1.0), &Transform::translate(5.0, 0.0));
        assert!(path.contains(PointF::new(5.5, 0.5), FillRule::NonZero));
        assert!(!path.contains(PointF::new(3.0, 0.5), FillRule::NonZero));
        assert_eq!(path.elements().len(), 10);
    }
}
