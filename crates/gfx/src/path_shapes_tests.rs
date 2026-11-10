//! The shapes a path is given (arcs, rounded rects, a transform) are
//! Skia's, bit for bit: `path_shapes.golden` is what these cases gave when
//! each was last compared with `SkPathBuilder`'s own (skia-safe 0.153, when
//! raster still drew with Skia). A case whose output changes no longer has
//! Skia's points; a new case has nothing of Skia's to be compared with.

use std::fmt::Write;

use crate::{Path, PathElement, PointF, RectF, SizeF, Transform};

fn p(x: f32, y: f32) -> PointF {
    PointF::new(x, y)
}

/// A path that starts at `start` (a move), if any.
fn from(start: Option<PointF>) -> Path {
    let mut path = Path::new();
    if let Some(start) = start {
        path.move_to(start);
    }
    path
}

/// `path`'s elements, one a word and its numbers' bits: no rounding.
fn dump(path: &Path) -> String {
    let mut out = String::new();
    let mut put = |name: &str, points: &[PointF], weight: Option<f32>| {
        let _ = write!(out, " {name}");
        for point in points {
            let _ = write!(out, " {:08x},{:08x}", point.x.to_bits(), point.y.to_bits());
        }
        if let Some(weight) = weight {
            let _ = write!(out, " w{:08x}", weight.to_bits());
        }
    };
    for element in path.elements() {
        match *element {
            PathElement::MoveTo(at) => put("M", &[at], None),
            PathElement::LineTo(at) => put("L", &[at], None),
            PathElement::QuadTo(c, at) => put("Q", &[c, at], None),
            PathElement::CubicTo(c1, c2, at) => put("C", &[c1, c2, at], None),
            PathElement::ConicTo(c, at, weight) => put("K", &[c, at], Some(weight)),
            PathElement::Close => put("Z", &[], None),
        }
    }
    out
}

/// Every case, named, with what it builds.
fn cases() -> Vec<(String, Path)> {
    let mut cases = Vec::new();

    // Arcs of an oval.
    let (tl, br) = (p(10.5, 20.25), p(110.0, 80.75));
    for (start, sweep) in [
        (0.0, 90.0),
        (30.0, 200.0),
        (-45.0, -120.0),
        (370.0, 45.0),
        (12.0, 359.5),
        (12.0, -359.5),
        // So near a whole turn that its ends round to one direction.
        (0.0, 359.9999),
        (90.0, 180.0),
        (0.0, 0.0),
        (45.0, 0.0),
        (10.0, 0.001),
        (10.0, 360.0),
        (10.0, 400.0),
    ] {
        for force_move_to in [false, true] {
            for start_at in [None, Some(p(0.0, 0.0)), Some(p(110.0, 50.5))] {
                let mut path = from(start_at);
                path.arc_to(tl, br, start, sweep, force_move_to);
                cases.push((
                    format!("arc {start} {sweep} move={force_move_to} from={start_at:?}"),
                    path,
                ));
            }
        }
    }
    // Two arcs of one oval, the second from where the first ended: no
    // line between them.
    let mut path = Path::new();
    for (start, sweep) in [(15.0, 180.0), (195.0, 180.0)] {
        path.arc_to(tl, br, start, sweep, false);
    }
    cases.push(("two arcs of one oval".into(), path));
    // An oval of no size is a point; one turned inside out, nothing.
    for (tl, br) in [(p(5.0, 5.0), p(5.0, 5.0)), (p(5.0, 5.0), p(1.0, 9.0))] {
        let mut path = from(Some(p(1.0, 1.0)));
        path.arc_to(tl, br, 30.0, 90.0, false);
        cases.push((format!("arc of the oval {tl:?} {br:?}"), path));
    }

    // Arcs tangent to two lines.
    let tangents: [(Option<PointF>, PointF, PointF, f32); 7] = [
        (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, 70.0), 20.0),
        (Some(p(10.0, 10.0)), p(90.0, 15.0), p(30.0, 60.0), 7.5),
        // Turning the other way.
        (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, -50.0), 20.0),
        // On one line, a radius of 0, the same point twice.
        (Some(p(10.0, 10.0)), p(50.0, 10.0), p(90.0, 10.0), 20.0),
        (Some(p(10.0, 10.0)), p(90.0, 10.0), p(90.0, 70.0), 0.0),
        (Some(p(10.0, 10.0)), p(10.0, 10.0), p(90.0, 70.0), 5.0),
        // From the origin.
        (None, p(90.0, 10.0), p(90.0, 70.0), 20.0),
    ];
    for (start, p1, p2, radius) in tangents {
        let mut path = from(start);
        path.tangent_arc_to(p1, p2, radius);
        cases.push((format!("tangent {start:?} {p1:?} {p2:?} {radius}"), path));
    }
    // After a close: from the start of the subpath it closed.
    let mut path = from(Some(p(10.0, 10.0)));
    path.line_to(p(50.0, 40.0));
    path.close();
    path.tangent_arc_to(p(90.0, 10.0), p(90.0, 70.0), 15.0);
    cases.push(("tangent after a close".into(), path));

    // A rect from one corner to the other, either way round.
    for (p0, p1) in [
        (p(10.0, 20.0), p(50.0, 70.0)),
        (p(50.0, 20.0), p(10.0, 70.0)),
    ] {
        let mut path = Path::new();
        path.add_rect(p0, p1);
        cases.push((format!("rect {p0:?} {p1:?}"), path));
    }
    let s = SizeF::new;
    let rect = RectF::new(10.25, 20.0, 80.0, 50.5);
    for radii in [
        [s(5.0, 5.0); 4],
        [s(5.0, 3.0), s(10.0, 12.0), s(0.0, 4.0), s(7.0, 7.0)],
        // Too big: scaled to fit, as CSS does.
        [s(60.0, 30.0), s(50.0, 40.0), s(10.0, 35.0), s(45.0, 20.0)],
        // Every corner square: a rect; halfway at every corner: an oval.
        [s(0.0, 9.0), s(0.0, 0.0), s(3.0, 0.0), s(0.0, 0.0)],
        [s(40.0, 25.25); 4],
        [s(100.0, 100.0); 4],
        // Too small to matter beside the other on its side.
        [s(1e-9, 5.0), s(40.0, 5.0), s(5.0, 5.0), s(5.0, 5.0)],
        [s(40.0, 5.0), s(1e-9, 5.0), s(5.0, 5.0), s(5.0, 5.0)],
    ] {
        for clockwise in [true, false] {
            let mut path = from(Some(p(1.0, 2.0)));
            path.line_to(p(3.0, 4.0));
            path.add_round_rect(rect, radii, clockwise);
            cases.push((format!("round rect {radii:?} clockwise={clockwise}"), path));
        }
    }

    // A path through a matrix.
    let mut path = Path::new();
    path.line_to(p(10.0, 20.0));
    path.quad_to(p(30.5, 1.0), p(40.0, 40.0));
    path.close();
    path.cubic_to(p(1.0, 2.0), p(3.0, 4.0), p(5.0, 6.0));
    path.conic_to(p(7.0, 8.0), p(9.0, 1.0), 0.5);
    path.transform(&Transform([0.8, 0.6, -0.6, 0.8, 12.5, -3.0]));
    cases.push(("transformed".into(), path));

    cases
}

#[test]
fn shapes_are_skias_bit_for_bit() {
    let golden = include_str!("path_shapes.golden");
    let cases = cases();
    let lines: Vec<&str> = golden.lines().collect();
    assert_eq!(lines.len(), cases.len(), "a case for each line");
    for ((name, path), line) in cases.iter().zip(lines) {
        assert_eq!(format!("{name}:{}", dump(path)), line);
    }
}
