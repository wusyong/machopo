//! Inline SVG's layout: an outermost `<svg>` is a replaced box, sized as
//! Chrome 152 sizes it, and its content is laid out by SVG's rules into
//! the fragment's `svg`.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use gfx::{FillRule, NodeId, Path, PathElement, PointF, RectF, SizeF, Transform};
use layout::{BoxFragment, SvgFragment, SvgNode, SvgNodeKind};

use crate::page::{Page, VIEWPORT, has_fragment, r};

/// A page laid out, its SVG content found by the elements' `id`s.
struct Laid {
    view: Arc<BoxFragment>,
    ids: BTreeMap<NodeId, String>,
}

fn lay_out(html: &str) -> Laid {
    let laid = crate::page::lay_out(html);
    Laid {
        view: laid.view,
        ids: laid.ids,
    }
}

impl Laid {
    /// The border box of the element with `id`, in the `LayoutView`'s
    /// space; `None` if it has no box.
    fn rect(&self, id: &str) -> Option<RectF> {
        fn find(f: &BoxFragment, at: PointF, want: NodeId) -> Option<RectF> {
            if f.node == Some(want) {
                return Some(RectF::new(at.x, at.y, f.size.width, f.size.height));
            }
            f.children.iter().find_map(|c| {
                find(
                    &c.fragment,
                    PointF::new(at.x + c.offset.x, at.y + c.offset.y),
                    want,
                )
            })
        }
        find(&self.view, PointF::new(0.0, 0.0), self.node(id))
    }

    fn node(&self, id: &str) -> NodeId {
        *self
            .ids
            .iter()
            .find(|(_, name)| *name == id)
            .unwrap_or_else(|| panic!("no #{id}"))
            .0
    }

    /// The content of the `<svg>` with `id`.
    fn svg(&self, id: &str) -> Arc<SvgFragment> {
        fn find(f: &BoxFragment, want: NodeId) -> Option<Arc<SvgFragment>> {
            if f.node == Some(want) {
                return f.svg.clone();
            }
            f.children.iter().find_map(|c| find(&c.fragment, want))
        }
        find(&self.view, self.node(id)).expect("an <svg> box with content")
    }

    /// The node of the SVG element with `id` in the content of `svg`.
    fn svg_node(&self, svg: &str, id: &str) -> SvgNode {
        fn find(nodes: &[SvgNode], want: NodeId) -> Option<SvgNode> {
            nodes.iter().find_map(|n| {
                if n.node == want {
                    return Some(n.clone());
                }
                match &n.kind {
                    SvgNodeKind::Container(children) | SvgNodeKind::Hidden(children) => {
                        find(children, want)
                    }
                    SvgNodeKind::Shape(_) | SvgNodeKind::Text(_) => None,
                }
            })
        }
        find(&self.svg(svg).children, self.node(id)).unwrap_or_else(|| panic!("#{id} not drawn"))
    }

    /// The fragment of the element with `id`.
    fn fragment(&self, id: &str) -> Arc<BoxFragment> {
        fn find(f: &Arc<BoxFragment>, want: NodeId) -> Option<Arc<BoxFragment>> {
            if f.node == Some(want) {
                return Some(f.clone());
            }
            f.children.iter().find_map(|c| find(&c.fragment, want))
        }
        find(&self.view, self.node(id)).unwrap_or_else(|| panic!("no box for #{id}"))
    }

    fn path(&self, svg: &str, id: &str) -> Option<Path> {
        match self.svg_node(svg, id).kind {
            SvgNodeKind::Shape(shape) => shape.path,
            SvgNodeKind::Container(_) | SvgNodeKind::Hidden(_) | SvgNodeKind::Text(_) => {
                panic!("#{id} isn't a shape")
            }
        }
    }
}

fn p(x: f32, y: f32) -> PointF {
    PointF::new(x, y)
}

const RESET: &str = "<style>html, body { margin: 0 } svg { display: block }</style>";

#[test]
fn an_outermost_svg_is_sized_as_chrome_sizes_it() {
    let laid = lay_out(&format!(
        "{RESET}<div style='width: 400px'>\
         <svg id=a></svg>\
         <svg id=b width=100></svg>\
         <svg id=c height=50></svg>\
         <svg id=d viewBox='0 0 200 100'></svg>\
         <svg id=e viewBox='0 0 200 100' width=100></svg>\
         <svg id=f viewBox='0 0 200 100' height=30></svg>\
         <svg id=g width='50%' height=20></svg>\
         <svg id=h viewBox='0 0 0 100'></svg>\
         <svg id=i style='width: 120px' width=10 height=10></svg>\
         <svg id=j width=100 height=50 style='padding: 5px; border: 2px solid'></svg>\
         <svg id=k viewBox='0 0 200 100' style='margin: 5px 10%; padding: 3px; border: 2px solid'></svg>\
         </div>"
    ));
    let size = |id: &str| laid.rect(id).expect("a box").size();
    assert_eq!(
        size("a"),
        SizeF::new(300.0, 150.0),
        "the default object size"
    );
    assert_eq!(size("b"), SizeF::new(100.0, 150.0));
    assert_eq!(size("c"), SizeF::new(300.0, 50.0));
    assert_eq!(
        size("d"),
        SizeF::new(400.0, 200.0),
        "a ratio fills the width"
    );
    assert_eq!(size("e"), SizeF::new(100.0, 50.0));
    assert_eq!(size("f"), SizeF::new(60.0, 30.0));
    assert_eq!(size("g"), SizeF::new(200.0, 20.0));
    assert_eq!(
        size("h"),
        SizeF::new(300.0, 150.0),
        "an empty viewBox is none"
    );
    assert_eq!(
        size("i"),
        SizeF::new(120.0, 10.0),
        "CSS beats the attribute"
    );
    assert_eq!(size("j"), SizeF::new(114.0, 64.0));
    assert_eq!(
        size("k"),
        SizeF::new(320.0, 165.0),
        "a ratio fills the width its margins leave, with its borders and padding"
    );
    // Inline, the ratio fills the line too.
    let laid = lay_out(
        "<style>html, body { margin: 0 }</style>\
         <div style='width: 300px'><svg id=s viewBox='0 0 3 1'></svg></div>",
    );
    assert_eq!(laid.rect("s").unwrap().size(), SizeF::new(300.0, 100.0));
}

/// An outermost `<svg>`'s natural size is its `width` and `height`
/// attributes where they are lengths, whatever its CSS (Blink's
/// `SVGSVGElement::IntrinsicWidth`), its ratio theirs, or its `viewBox`'s
/// with only one; `min-*` and `max-*` keep the ratio, in a block and
/// inline. Chrome 152's sizes.
#[test]
fn an_outermost_svgs_natural_size_is_its_attributes() {
    let laid = lay_out(&format!(
        "{RESET}<div style='width: 400px'>\
         <svg id=k width=2000 height=1000 viewBox='0 0 2000 1000' style='height: auto; max-width: 100%'></svg>\
         <svg id=l width=2000 height=1000 viewBox='0 0 2000 1000' style='max-width: 100%'></svg>\
         <svg id=n width=469 height=64 viewBox='0 0 469 64' style='width: 100%; height: 100%'></svg>\
         <svg id=o width=100 height=50 style='height: auto; max-width: 60px'></svg>\
         <svg id=p width=100 height=50 viewBox='0 0 100 100' style='height: auto; max-width: 60px'></svg>\
         <svg id=q width=1000 height=500 style='height: auto; max-width: 100%; padding: 5px; border: 2px solid'></svg>\
         <svg id=r width='2em' height=10 style='height: auto; width: auto; font-size: 20px'></svg>\
         <svg id=s width=100 height=50 style='height: auto; min-width: 200px'></svg>\
         <span><svg id=t width=2000 height=1000 viewBox='0 0 2000 1000' style='display: inline; height: auto; max-width: 100%'></svg></span>\
         </div>"
    ));
    let size = |id: &str| laid.rect(id).expect("a box").size();
    assert_eq!(
        size("k"),
        SizeF::new(400.0, 200.0),
        "max-width keeps the ratio"
    );
    assert_eq!(size("l"), SizeF::new(400.0, 1000.0), "the height attribute");
    assert_eq!(
        size("n"),
        SizeF::new(400.0, 400.0 * 64.0 / 469.0),
        "a percentage height is auto"
    );
    assert_eq!(size("o"), SizeF::new(60.0, 30.0), "the attributes' ratio");
    assert_eq!(
        size("p"),
        SizeF::new(60.0, 30.0),
        "the attributes' ratio before the viewBox's"
    );
    assert_eq!(size("q"), SizeF::new(414.0, 214.0));
    assert_eq!(size("r"), SizeF::new(40.0, 10.0), "em of its font size");
    assert_eq!(size("s"), SizeF::new(200.0, 100.0));
    assert_eq!(size("t"), SizeF::new(400.0, 200.0), "inline too");
}

/// A percentage `width` or `max-width` makes an `<svg>` compressible:
/// its min-content contribution has no width (css-sizing 3), so `1fr`
/// columns and a flex container shrink to the room there is, and it with
/// them, however deep in flex boxes. Chrome 152's sizes.
#[test]
fn a_percentage_width_svg_shrinks_with_grid_columns_and_flex_boxes() {
    let svg = |id: &str, style: &str| {
        format!("<svg id={id} width=1280 height=720 viewBox='0 0 1280 720' style='{style}'></svg>")
    };
    let grid = "display: grid; grid-template-columns: 1fr 1fr; gap: 10px; width: 400px";
    let column = "display: flex; flex-direction: column";
    let laid = lay_out(&format!(
        "{RESET}<div style='{grid}'>{}{}</div>\
         <div style='{grid}'><div style='{column}'><div style='{column}'>{}</div></div>\
         <div>{}</div></div>\
         <div style='display: flex; width: 400px'><a style='display: flex'>\
         <svg id=m width=469 height=64 viewBox='0 0 469 64' style='width: 100%; height: 100%'></svg>\
         </a></div>",
        svg("g1", "width: 100%; height: auto"),
        svg("g2", "max-width: 100%; height: auto"),
        svg("h1", "width: 100%; height: auto"),
        svg("h2", "width: 100%; height: auto"),
    ));
    let size = |id: &str| laid.rect(id).expect("a box").size();
    for id in ["g1", "g2", "h1", "h2"] {
        assert_eq!(size(id), SizeF::new(195.0, 195.0 * 720.0 / 1280.0), "#{id}");
    }
    assert_eq!(size("m"), SizeF::new(400.0, 400.0 * 64.0 / 469.0));
    // With only a percentage `max-width`, the width its height and ratio
    // give (1280px).
    let laid = lay_out(&format!(
        "{RESET}<div style='{grid}'>\
         <svg id=a height=720 viewBox='0 0 1280 720' style='max-width: 100%'></svg>\
         <svg id=b height=720 viewBox='0 0 1280 720' style='max-width: 100%'></svg></div>"
    ));
    for id in ["a", "b"] {
        assert_eq!(
            laid.rect(id).unwrap().size(),
            SizeF::new(195.0, 720.0),
            "#{id}"
        );
    }
    let laid = lay_out(
        "<style>html, body { margin: 0 }</style>\
         <div style='display: flex; width: 400px'><a style='display: flex'>\
         <svg id=f1 height=64 viewBox='0 0 469 64' style='max-width: 100%'></svg></a></div>\
         <div style='display: flex; width: 400px'><a style='display: flex; flex-direction: column'>\
         <svg id=f2 height=64 viewBox='0 0 469 64' style='max-width: 100%'></svg></a></div>",
    );
    for id in ["f1", "f2"] {
        assert_eq!(
            laid.rect(id).unwrap().size(),
            SizeF::new(400.0, 64.0),
            "#{id}"
        );
    }
}

#[test]
fn svg_content_makes_no_boxes_and_only_what_is_drawn_is_laid_out() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=100 height=100>\
         <g id=g><rect id=r width=10 height=10 /><text id=t>text</text></g>\
         <rect id=none style='display: none' width=10 height=10 />\
         <unknown id=u><rect id=inside width=10 height=10 /></unknown>\
         <foreignObject id=fo><div id=div>html</div></foreignObject>\
         <circle id=c r=5 /></svg>"
    ));
    for id in ["g", "r", "t", "none", "u", "inside", "fo", "div", "c"] {
        assert_eq!(laid.rect(id), None, "#{id} has no box");
    }
    let svg = laid.svg("s");
    let drawn: Vec<NodeId> = svg.children.iter().map(|n| n.node).collect();
    assert_eq!(drawn, [laid.node("g"), laid.node("c")]);
    let SvgNodeKind::Container(children) = &svg.children[0].kind else {
        panic!("a group is a container");
    };
    assert_eq!(children.len(), 2, "the rect and the text");
    assert!(matches!(children[1].kind, SvgNodeKind::Text(_)));
}

#[test]
fn rects_and_their_corners() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100>\
         <rect id=plain x=10 y=20 width=30 height=40 />\
         <rect id=percent x='10%' y='10%' width='50%' height='50%' />\
         <rect id=empty width=0 height=10 />\
         <rect id=round width=40 height=20 rx=30 />\
         <rect id=elliptic width=40 height=20 rx=4 ry=8 />\
         <rect id=fill-rule width=10 height=10 fill-rule=evenodd /></svg>"
    ));
    assert_eq!(
        laid.path("s", "plain").unwrap(),
        Path::polygons([[p(10.0, 20.0), p(40.0, 20.0), p(40.0, 60.0), p(10.0, 60.0)]])
    );
    assert_eq!(
        laid.path("s", "percent").unwrap().bounds(),
        Some(r(20.0, 10.0, 100.0, 50.0))
    );
    assert_eq!(laid.path("s", "empty"), None);
    // `ry` is `auto`: `rx`, both cut to half the sides (20 and 10). Radii
    // of half the sides make an oval, which Skia's `addRRect` adds as
    // `addOval` from its top: no straight sides between the corners.
    let round = laid.path("s", "round").unwrap();
    assert_eq!(round.elements()[0], PathElement::MoveTo(p(20.0, 0.0)));
    assert_eq!(round.elements().len(), 6, "{:?}", round.elements());
    let PathElement::ConicTo(control, end, weight) = round.elements()[1] else {
        panic!("a corner is a conic: {:?}", round.elements());
    };
    assert!(
        (control.x - 40.0).abs() < 1e-4 && control.y.abs() < 1e-4,
        "{control:?}"
    );
    assert!(
        (end.x - 40.0).abs() < 1e-4 && (end.y - 10.0).abs() < 1e-4,
        "{end:?}"
    );
    assert!((weight - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
    let b = round.bounds().unwrap();
    assert!(
        b.x.abs() < 1e-4 && (b.width - 40.0).abs() < 1e-4 && b.height == 20.0,
        "{b:?}"
    );
    let elliptic = laid.path("s", "elliptic").unwrap();
    assert_eq!(elliptic.elements()[0], PathElement::MoveTo(p(4.0, 0.0)));
    assert_eq!(elliptic.elements()[1], PathElement::LineTo(p(36.0, 0.0)));
    assert_eq!(
        laid.path("s", "fill-rule").unwrap().fill_rule(),
        FillRule::EvenOdd
    );
}

#[test]
fn circles_ellipses_lines_and_polys() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=300 height=400>\
         <circle id=c cx=50 cy=60 r=10 />\
         <circle id=pc r='10%' />\
         <circle id=zero r=0 />\
         <ellipse id=e cx=10 cy=10 rx=20 />\
         <ellipse id=no-radius cx=10 cy=10 />\
         <line id=l x1=1 y1='50%' x2='1em' y2=4 />\
         <polyline id=pl points='0,0 10,0 10,10 5' />\
         <polygon id=pg points='0 0 10 0 10 10' /></svg>"
    ));
    let c = laid.path("s", "c").unwrap();
    // From the right end, clockwise (down first), as Blink's `AddEllipse`.
    assert_eq!(c.elements()[0], PathElement::MoveTo(p(60.0, 60.0)));
    assert!(
        matches!(c.elements()[1], PathElement::ConicTo(_, end, _) if end == p(50.0, 70.0)),
        "{:?}",
        c.elements()
    );
    assert_eq!(
        c.elements().len(),
        6,
        "a move, four quarter conics, a close"
    );
    assert_eq!(c.elements()[5], PathElement::Close);
    let b = c.bounds().unwrap();
    assert!(
        (b.x - 40.0).abs() < 1e-3 && (b.width - 20.0).abs() < 1e-3,
        "{b:?}"
    );
    // A percentage `r` is of the normalized diagonal: √((300² + 400²)/2).
    let pc = laid.path("s", "pc").unwrap();
    let expected = 0.1 * ((300.0f32 * 300.0 + 400.0 * 400.0) / 2.0).sqrt();
    let PathElement::MoveTo(start) = pc.elements()[0] else {
        unreachable!()
    };
    assert!((start.x - expected).abs() < 1e-3, "{start:?}");
    assert_eq!(laid.path("s", "zero"), None);
    // `ry` is `auto`: `rx`.
    let e = laid.path("s", "e").unwrap().bounds().unwrap();
    assert!((e.height - 40.0).abs() < 1e-3, "{e:?}");
    assert_eq!(laid.path("s", "no-radius"), None);
    let mut line = Path::new();
    line.move_to(p(1.0, 200.0));
    line.line_to(p(16.0, 4.0));
    assert_eq!(laid.path("s", "l").unwrap(), line);
    let mut polyline = Path::new();
    polyline.move_to(p(0.0, 0.0));
    polyline.line_to(p(10.0, 0.0));
    polyline.line_to(p(10.0, 10.0));
    assert_eq!(
        laid.path("s", "pl").unwrap(),
        polyline,
        "the odd number left out"
    );
    polyline.close();
    assert_eq!(laid.path("s", "pg").unwrap(), polyline, "a polygon closes");
}

#[test]
fn path_data_reflects_and_draws_arcs_as_conics() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=100 height=100>\
         <path id=curves d='M0 0 C 0 10 10 10 10 0 S 20 -10 20 0 Q 25 10 30 0 T 40 0' />\
         <path id=arc d='M0 0 A 10 10 0 0 1 20 0' />\
         <path id=flat d='M0 0 A 0 10 0 0 1 20 0 A 5 5 0 0 1 20 0' />\
         <path id=small d='M0 0 A 1 1 0 0 1 20 0' />\
         <path id=closed d='M5 5 L 10 5 Z l 1 1' />\
         <path id=none /></svg>"
    ));
    use PathElement::*;
    assert_eq!(
        laid.path("s", "curves").unwrap().elements(),
        [
            MoveTo(p(0.0, 0.0)),
            CubicTo(p(0.0, 10.0), p(10.0, 10.0), p(10.0, 0.0)),
            // The first control point is the last one reflected.
            CubicTo(p(10.0, -10.0), p(20.0, -10.0), p(20.0, 0.0)),
            QuadTo(p(25.0, 10.0), p(30.0, 0.0)),
            QuadTo(p(35.0, -10.0), p(40.0, 0.0)),
        ]
    );
    // A half circle: two quarter conics; the sweep flag turns the
    // positive way, clockwise with y down, so over the top.
    let arc = laid.path("s", "arc").unwrap();
    assert_eq!(arc.elements().len(), 3);
    let ConicTo(_, mid, _) = arc.elements()[1] else {
        panic!("{:?}", arc.elements());
    };
    assert!(
        (mid.x - 10.0).abs() < 1e-3 && (mid.y + 10.0).abs() < 1e-3,
        "{mid:?}"
    );
    // A radius of 0 is a line; an arc to where it is, nothing.
    assert_eq!(
        laid.path("s", "flat").unwrap().elements(),
        [MoveTo(p(0.0, 0.0)), LineTo(p(20.0, 0.0))]
    );
    // Radii too small are scaled up: still a half circle, 10 across.
    let small = laid.path("s", "small").unwrap().bounds().unwrap();
    assert!((small.height - 10.0).abs() < 1e-2, "{small:?}");
    // After a close, from the subpath's start: style made it absolute.
    assert_eq!(
        laid.path("s", "closed").unwrap().elements()[3],
        LineTo(p(6.0, 6.0))
    );
    assert_eq!(laid.path("s", "none"), None);
}

#[test]
fn view_boxes_and_aspect_ratios() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=meet width=200 height=100 viewBox='0 0 10 10'><rect id=r1 width='50%' height=10 /></svg>\
         <svg id=slice width=200 height=100 viewBox='0 0 10 10' preserveAspectRatio='xMaxYMid slice'></svg>\
         <svg id=none width=200 height=100 viewBox='10 20 10 10' preserveAspectRatio='none'></svg>\
         <svg id=plain width=200 height=100></svg>\
         <svg id=disabled width=200 height=100 viewBox='0 0 0 10'><rect width=10 height=10 /></svg>\
         <svg id=negative width=200 height=100 viewBox='0 0 -1 10'><rect width=10 height=10 /></svg>"
    ));
    // Meet, centered: scale 10, 50 in from the left.
    assert_eq!(
        laid.svg("meet").content_transform,
        Transform([10.0, 0.0, 0.0, 10.0, 50.0, 0.0])
    );
    // Percentages are of the viewBox.
    assert_eq!(
        laid.path("meet", "r1").unwrap().bounds(),
        Some(r(0.0, 0.0, 5.0, 10.0))
    );
    // Slice, at the right and the middle: scale 20, 50 up.
    assert_eq!(
        laid.svg("slice").content_transform,
        Transform([20.0, 0.0, 0.0, 20.0, 0.0, -50.0])
    );
    assert_eq!(
        laid.svg("none").content_transform,
        Transform([20.0, 0.0, 0.0, 10.0, -200.0, -200.0])
    );
    assert!(laid.svg("plain").content_transform.is_identity());
    assert!(laid.svg("disabled").children.is_empty(), "nothing drawn");
    // A negative width is an error: as if there were no viewBox.
    let negative = laid.svg("negative");
    assert!(negative.content_transform.is_identity());
    assert_eq!(negative.children.len(), 1);
}

#[test]
fn nested_svgs_place_and_clip_their_content() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100>\
         <svg id=n x=10 y='10%' width='50%' height=40 viewBox='0 0 50 20'>\
         <rect id=r width='10%' height='10%' /></svg>\
         <svg id=v x=5 style='overflow: visible'></svg>\
         <svg id=t transform='translate(10 0)' x=1 /></svg>"
    ));
    let n = laid.svg_node("s", "n");
    assert_eq!(n.clip, Some(r(10.0, 10.0, 100.0, 40.0)));
    // Placed, then its viewBox: scale 2 to fit 100 wide, centered in 40.
    assert_eq!(
        n.content_transform,
        Transform([2.0, 0.0, 0.0, 2.0, 10.0, 10.0])
    );
    // Its content's percentages are of its viewBox.
    assert_eq!(
        laid.path("s", "r").unwrap().bounds(),
        Some(r(0.0, 0.0, 5.0, 2.0))
    );
    // `overflow: visible` doesn't clip; `auto` sizes are 100%.
    let v = laid.svg_node("s", "v");
    assert_eq!(v.clip, None);
    assert_eq!(v.content_transform, Transform::translate(5.0, 0.0));
    // Its transform is outside its placement.
    let t = laid.svg_node("s", "t");
    assert_eq!(t.transform, Transform::translate(10.0, 0.0));
    assert_eq!(t.clip, Some(r(1.0, 0.0, 200.0, 100.0)));
}

#[test]
fn transforms_and_strokes_resolve_against_the_viewport() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=300 height=400>\
         <rect id=r width=10 height=10 style='transform: translate(10%, 50%) scale(2)' \
          stroke-width='10%' stroke-dasharray='1 10%' stroke-dashoffset='2%' />\
         <g id=g transform='matrix(1 0 0 1 3 4)' /></svg>"
    ));
    let rect = laid.svg_node("s", "r");
    assert_eq!(
        rect.transform,
        Transform([2.0, 0.0, 0.0, 2.0, 30.0, 200.0]),
        "translated by the viewport's 10% and 50%, then scaled"
    );
    let diagonal = ((300.0f32 * 300.0 + 400.0 * 400.0) / 2.0).sqrt();
    let SvgNodeKind::Shape(shape) = rect.kind else {
        unreachable!()
    };
    assert!((shape.stroke_width - diagonal / 10.0).abs() < 1e-3);
    assert_eq!(shape.dashes.len(), 2);
    assert!((shape.dashes[1] - diagonal / 10.0).abs() < 1e-3);
    assert!((shape.dash_offset - diagonal / 50.0).abs() < 1e-3);
    assert_eq!(
        laid.svg_node("s", "g").transform,
        Transform([1.0, 0.0, 0.0, 1.0, 3.0, 4.0])
    );
}

/// The node `use` draws (its copy of what it references, drawn as the
/// `<use>`), which is the `<use>`'s one child.
fn drawn_by_use(use_node: &SvgNode) -> &SvgNode {
    let SvgNodeKind::Container(children) = &use_node.kind else {
        panic!("a <use> is a container");
    };
    assert_eq!(children.len(), 1, "a <use> draws one copy");
    &children[0]
}

fn children(node: &SvgNode) -> &[SvgNode] {
    match &node.kind {
        SvgNodeKind::Container(children) | SvgNodeKind::Hidden(children) => children,
        SvgNodeKind::Shape(_) | SvgNodeKind::Text(_) => panic!("not a container"),
    }
}

#[test]
fn a_use_places_its_copy_and_draws_it_as_itself() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100>\
         <defs><rect id=r width=10 height=20 /><g id=g><circle r=1 /><rect width=1 height=1 /></g></defs>\
         <use id=u href=#r x=5 y='10%' width=50 height=50 transform='translate(1 0)' />\
         <use id=group href=#g /><use id=nothing href=#none /></svg>"
    ));
    // `<defs>` is there, hidden, for what is in it to have geometry.
    let top = &laid.svg("s").children;
    assert!(matches!(top[0].kind, SvgNodeKind::Hidden(_)));
    let drawn: Vec<NodeId> = top[1..].iter().map(|n| n.node).collect();
    assert_eq!(
        drawn,
        [laid.node("u"), laid.node("group"), laid.node("nothing")],
        "not what's in <defs>"
    );
    let u = laid.svg_node("s", "u");
    assert_eq!(u.transform, Transform::translate(1.0, 0.0));
    assert_eq!(u.content_transform, Transform::translate(5.0, 10.0));
    let copy = drawn_by_use(&u);
    assert_eq!(copy.node, laid.node("u"), "drawn as the <use>");
    let SvgNodeKind::Shape(shape) = &copy.kind else {
        panic!("a shape");
    };
    assert_eq!(
        shape.path.as_ref().unwrap().bounds(),
        Some(r(0.0, 0.0, 10.0, 20.0)),
        "a rect isn't sized by the <use>"
    );
    let group = laid.svg_node("s", "group");
    let copy = drawn_by_use(&group);
    assert_eq!(children(copy).len(), 2);
    assert!(children(copy).iter().all(|n| n.node == laid.node("group")));
    assert_eq!(children(&laid.svg_node("s", "nothing")).len(), 0);
}

/// A `<symbol>` is drawn only by a `<use>`, as an `<svg>` sized by the
/// `<use>`'s `width` and `height`; so is an `<svg>` it references.
#[test]
fn a_use_sizes_the_symbol_or_svg_it_draws() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100>\
         <symbol id=sym viewBox='0 0 10 10'><rect width=10 height=10 />\
         <symbol id=nested><rect width=1 height=1 /></symbol></symbol>\
         <use id=u href=#sym width=50 height=20 />\
         <use id=auto href=#sym width=auto />\
         <use id=negative href=#sym width=-5 height=abc />\
         <svg id=inner width=10 height=10 viewBox='0 0 1 1' x=3 />\
         <use id=wide href=#inner width=40 /></svg>"
    ));
    // The `<symbol>` in the document is hidden; only `<use>`s draw it.
    let top = &laid.svg("s").children;
    assert_eq!(top[0].node, laid.node("sym"));
    assert!(matches!(top[0].kind, SvgNodeKind::Hidden(_)));
    let drawn: Vec<NodeId> = top[1..].iter().map(|n| n.node).collect();
    assert_eq!(
        drawn,
        [
            laid.node("u"),
            laid.node("auto"),
            laid.node("negative"),
            laid.node("inner"),
            laid.node("wide")
        ]
    );
    let symbol = drawn_by_use(&laid.svg_node("s", "u")).clone();
    assert_eq!(symbol.clip, Some(r(0.0, 0.0, 50.0, 20.0)));
    // Scaled 2 to fit 20 high, centered in 50.
    assert_eq!(
        symbol.content_transform,
        Transform([2.0, 0.0, 0.0, 2.0, 15.0, 0.0])
    );
    assert_eq!(children(&symbol).len(), 2);
    assert!(
        matches!(children(&symbol)[1].kind, SvgNodeKind::Hidden(_)),
        "a <symbol> in it isn't drawn"
    );
    // `auto` is 100%.
    let symbol = drawn_by_use(&laid.svg_node("s", "auto")).clone();
    assert_eq!(symbol.clip, Some(r(0.0, 0.0, 200.0, 100.0)));
    // So are a negative length and one that doesn't parse.
    let symbol = drawn_by_use(&laid.svg_node("s", "negative")).clone();
    assert_eq!(symbol.clip, Some(r(0.0, 0.0, 200.0, 100.0)));
    // The `<use>`'s width wins; its height is `auto`, so the `<svg>`'s.
    let svg = drawn_by_use(&laid.svg_node("s", "wide")).clone();
    assert_eq!(svg.clip, Some(r(3.0, 0.0, 40.0, 10.0)));
}

#[test]
fn a_switch_draws_its_first_child_whose_conditions_hold() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100>\
         <switch id=sw>\
           <rect id=fr systemLanguage=fr width=1 height=1 />\
           <rect id=none requiredExtensions='' width=1 height=1 />\
           <rect id=unknown requiredExtensions='http://example.com' width=1 height=1 />\
           <rect id=gb systemLanguage='fr, EN-gb' width=1 height=1 />\
           <rect id=last width=1 height=1 />\
         </switch>\
         <switch id=first><rect id=a width=1 height=1 /><rect id=b width=1 height=1 /></switch>\
         <rect id=english systemLanguage=en width=1 height=1 />\
         <rect id=enx systemLanguage=enx width=1 height=1 />\
         <rect id=html requiredExtensions='http://www.w3.org/1999/xhtml' width=1 height=1 />\
         <rect id=feature requiredFeatures='http://www.w3.org/TR/SVG11/feature#Shape' width=1 height=1 />\
         </svg>"
    ));
    let ids = |nodes: &[SvgNode]| -> Vec<String> {
        nodes.iter().map(|n| laid.ids[&n.node].clone()).collect()
    };
    assert_eq!(ids(children(&laid.svg_node("s", "sw"))), ["gb"]);
    assert_eq!(ids(children(&laid.svg_node("s", "first"))), ["a"]);
    // The conditions hold outside a `<switch>` too.
    assert_eq!(
        ids(&laid.svg("s").children),
        ["sw", "first", "english", "html", "feature"]
    );
}

/// A `<use>` in a copy draws its own copy, drawn as the outer `<use>`.
#[test]
fn nested_uses_draw_as_the_outer_use() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100><defs>\
         <rect id=r width=10 height=10 />\
         <g id=g><use id=inner href=#r x=7 /></g></defs>\
         <use id=u href=#g /></svg>"
    ));
    let g = drawn_by_use(&laid.svg_node("s", "u")).clone();
    let inner = &children(&g)[0];
    assert_eq!(inner.node, laid.node("u"));
    assert_eq!(inner.content_transform, Transform::translate(7.0, 0.0));
    assert_eq!(drawn_by_use(inner).node, laid.node("u"));
}

fn shape(node: SvgNode) -> layout::SvgShape {
    match node.kind {
        SvgNodeKind::Shape(shape) => shape,
        SvgNodeKind::Container(_) | SvgNodeKind::Hidden(_) | SvgNodeKind::Text(_) => {
            panic!("not a shape")
        }
    }
}

fn gradient(server: Option<layout::SvgPaintServer>) -> layout::SvgGradient {
    match server {
        Some(layout::SvgPaintServer::Gradient(gradient)) => gradient,
        other => panic!("not a gradient: {other:?}"),
    }
}

const RED: gfx::Color = gfx::Color::rgb(255, 0, 0);
const BLUE: gfx::Color = gfx::Color::rgb(0, 0, 255);
const LIME: gfx::Color = gfx::Color::rgb(0, 255, 0);

/// A linear gradient's line from 0% to 100% across the shape's object
/// bounding box (around its curves, not their control points) by default;
/// in user space with percentages of the viewport for `userSpaceOnUse`.
#[test]
fn linear_gradients_in_the_box_or_in_user_space() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100><defs>\
         <linearGradient id=box><stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient>\
         <linearGradient id=user href=#box gradientUnits=userSpaceOnUse x1=10 y1='10%' x2='50%' y2=30 />\
         </defs>\
         <rect id=r x=10 y=20 width=40 height=30 fill='url(#box)' />\
         <circle id=c cx=100 cy=50 r=20 fill='url(#box)' />\
         <rect id=u width=10 height=10 stroke='url(#user)' />\
         <path id=q d='M0 0 Q10 20 20 0' fill='url(#box)' />\
         </svg>"
    ));
    let r = gradient(shape(laid.svg_node("s", "r")).fill_server);
    assert_eq!(
        r.kind,
        layout::SvgGradientKind::Linear {
            start: p(0.0, 0.0),
            end: p(1.0, 0.0)
        }
    );
    assert_eq!(
        r.transform,
        Transform([40.0, 0.0, 0.0, 30.0, 10.0, 20.0]),
        "the box's"
    );
    assert_eq!(r.stops, [(0.0, RED), (1.0, BLUE)]);
    assert_eq!(r.spread, layout::Spread::Pad);
    let c = gradient(shape(laid.svg_node("s", "c")).fill_server);
    let t = c.transform.0;
    let close = |a: f32, b: f32| (a - b).abs() < 1e-3;
    assert!(
        close(t[0], 40.0) && close(t[3], 40.0) && close(t[4], 80.0) && close(t[5], 30.0),
        "{t:?}"
    );
    let u = shape(laid.svg_node("s", "u"));
    assert_eq!(u.fill_server, None, "a color");
    let u = gradient(u.stroke_server);
    assert_eq!(
        u.kind,
        layout::SvgGradientKind::Linear {
            start: p(10.0, 10.0),
            end: p(100.0, 30.0)
        }
    );
    assert!(u.transform.is_identity());
    // The curve reaches half way to its control point.
    let q = gradient(shape(laid.svg_node("s", "q")).fill_server);
    assert_eq!(q.transform, Transform([20.0, 0.0, 0.0, 10.0, 0.0, 0.0]));
}

/// Each attribute comes from the first gradient on the `href` chain that
/// has it, the geometry from those of the same kind only, the stops from
/// the first with any; a cycle ends the chain.
#[test]
fn gradients_take_what_they_lack_through_href() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100><defs>\
         <linearGradient id=base x1=0.2 spreadMethod=reflect gradientTransform='scale(2)'>\
           <stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient>\
         <radialGradient id=radial href=#base cx=0.3 r=0.4><stop stop-color=lime /></radialGradient>\
         <radialGradient id=more xlink:href=#radial fr=0.1 />\
         <linearGradient id=cycle href=#loop x2=0.5 />\
         <linearGradient id=loop href=#cycle><stop stop-color=red /><stop offset=1 stop-color=lime /></linearGradient>\
         </defs>\
         <rect id=a width=10 height=10 fill='url(#more)' />\
         <rect id=b width=10 height=10 fill='url(#cycle)' />\
         </svg>"
    ));
    // `radial`'s one stop: one color, whatever the rest.
    let a = shape(laid.svg_node("s", "a"));
    assert_eq!(a.fill_server, Some(layout::SvgPaintServer::Color(LIME)));
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100><defs>\
         <linearGradient id=base x1=0.2 cy=0.9 spreadMethod=reflect gradientTransform='translate(1 0) scale(2)'>\
           <stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient>\
         <radialGradient id=radial href=#base cx=0.3 r=0.4 />\
         <radialGradient id=more xlink:href=#radial fr=0.1 fy=0.6 />\
         <linearGradient id=cycle href=#loop x2=0.5 />\
         <linearGradient id=loop href=#cycle><stop stop-color=red /><stop offset=1 stop-color=lime /></linearGradient>\
         </defs>\
         <rect id=a width=10 height=10 fill='url(#more)' />\
         <rect id=b width=10 height=10 fill='url(#cycle)' />\
         </svg>"
    ));
    let a = gradient(shape(laid.svg_node("s", "a")).fill_server);
    assert_eq!(
        a.kind,
        layout::SvgGradientKind::Radial {
            // `fx` is `cx` without one; `cy` is 50% (the linear one's isn't
            // a radial's to take), and so is `x1`.
            focal: p(0.3, 0.6),
            focal_radius: 0.1,
            center: p(0.3, 0.5),
            radius: 0.4,
        }
    );
    assert_eq!(a.spread, layout::Spread::Reflect);
    assert_eq!(a.stops, [(0.0, RED), (1.0, BLUE)]);
    // The box's, then `gradientTransform` in it.
    assert_eq!(a.transform, Transform([20.0, 0.0, 0.0, 20.0, 10.0, 0.0]));
    let b = gradient(shape(laid.svg_node("s", "b")).fill_server);
    assert_eq!(
        b.kind,
        layout::SvgGradientKind::Linear {
            start: p(0.0, 0.0),
            end: p(0.5, 0.0)
        }
    );
    assert_eq!(b.stops, [(0.0, RED), (1.0, LIME)]);
}

/// Offsets: numbers or percentages, clamped to 0 to 1 and to no less
/// than the one before; colors with their `stop-opacity`.
#[test]
fn stops_are_clamped_in_order() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100><defs>\
         <linearGradient id=g>\
           <stop offset=-1 stop-color=red /><stop offset='40%' stop-color=blue stop-opacity=0.5 />\
           <stop offset=0.2 style='stop-color: lime' /><stop offset=2 /><stop offset=x />\
         </linearGradient></defs>\
         <rect id=r width=10 height=10 fill='url(#g)' /></svg>"
    ));
    let g = gradient(shape(laid.svg_node("s", "r")).fill_server);
    let half_blue = gfx::Color { a: 128, ..BLUE };
    assert_eq!(
        g.stops,
        [
            (0.0, RED),
            (0.4, half_blue),
            (0.4, LIME),
            (1.0, gfx::Color::BLACK),
            (1.0, gfx::Color::BLACK)
        ]
    );
}

/// What isn't a usable gradient paints with the fallback (`None` here);
/// one without stops paints nothing; one with no length is its last
/// stop's color.
#[test]
fn gradients_that_cant_be_used() {
    let laid = lay_out(&format!(
        "{RESET}<svg id=s width=200 height=100><defs>\
         <linearGradient id=empty />\
         <linearGradient id=flat x2=0><stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient>\
         <radialGradient id=dot r=0><stop stop-color=red /><stop offset=1 stop-color=blue /></radialGradient>\
         <linearGradient id=g><stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient>\
         <linearGradient id=user gradientUnits=userSpaceOnUse href=#g />\
         <rect id=notone /></defs>\
         <rect id=missing width=10 height=10 fill='url(#nothing) red' />\
         <rect id=other width=10 height=10 fill='url(#notone)' />\
         <rect id=elsewhere width=10 height=10 fill='url(other.svg#g)' />\
         <rect id=nostops width=10 height=10 fill='url(#empty) red' />\
         <rect id=zero width=10 height=10 fill='url(#flat)' />\
         <rect id=point width=10 height=10 fill='url(#dot)' />\
         <line id=flatbox x2=10 stroke='url(#g)' />\
         <line id=userline x2=10 stroke='url(#user)' />\
         <rect id=encoded width=10 height=10 fill='url(#%67)' />\
         </svg>"
    ));
    let fill = |id: &str| shape(laid.svg_node("s", id)).fill_server;
    let stroke = |id: &str| shape(laid.svg_node("s", id)).stroke_server;
    assert_eq!(fill("missing"), None);
    assert_eq!(fill("other"), None);
    assert_eq!(fill("elsewhere"), None);
    assert_eq!(fill("nostops"), Some(layout::SvgPaintServer::None));
    assert_eq!(fill("zero"), Some(layout::SvgPaintServer::Color(BLUE)));
    assert_eq!(fill("point"), Some(layout::SvgPaintServer::Color(BLUE)));
    assert_eq!(stroke("flatbox"), None, "a box with no height");
    assert!(matches!(
        stroke("userline"),
        Some(layout::SvgPaintServer::Gradient(_))
    ));
    assert!(
        matches!(fill("encoded"), Some(layout::SvgPaintServer::Gradient(_))),
        "percent-decoded"
    );
}

/// Each cell of a `<text>`'s characters: its origin and turn, and the
/// element found there (`""` for the `<text>`).
fn cells(laid: &Laid, svg: &str, id: &str) -> Vec<(PointF, f32, String)> {
    let SvgNodeKind::Text(text) = laid.svg_node(svg, id).kind else {
        panic!("#{id} isn't text");
    };
    text.cells
        .iter()
        .map(|cell| {
            let found = if cell.node == laid.node(id) {
                String::new()
            } else {
                laid.ids.get(&cell.node).cloned().unwrap_or("?".into())
            };
            (cell.origin, cell.rotation, found)
        })
        .collect()
}

const AHEM: &str = "<style>text { font: 10px Ahem }</style>";

/// White space collapses across elements, none at the start or end;
/// `xml:space="preserve"` and `white-space: pre` keep it; a `<tspan>` that
/// isn't displayed, and what isn't text content, has no characters; a
/// `<text>` with none draws nothing, but is laid out.
#[test]
fn svg_text_processes_white_space() {
    let laid = lay_out(&format!(
        "{RESET}{AHEM}<svg id=s width=200 height=100>\
         <text id=a y=10>  X <tspan id=t> X </tspan>\nX  </text>\
         <text id=p y=20 xml:space=preserve> X\tX </text>\
         <text id=c y=30 style='white-space: pre'> X</text>\
         <text id=n y=40>X<tspan style='display: none'>X</tspan><foo>X</foo><desc>X</desc><rect />X</text>\
         <text id=empty y=50>   </text></svg>"
    ));
    let x = |id| -> Vec<f32> { cells(&laid, "s", id).iter().map(|c| c.0.x).collect() };
    assert_eq!(x("a"), [0.0, 10.0, 20.0, 30.0, 40.0], "X X X");
    let found: Vec<String> = cells(&laid, "s", "a").into_iter().map(|c| c.2).collect();
    // The space before the tspan is the text's; the one in it, its own.
    assert_eq!(found, ["", "", "t", "t", ""]);
    assert_eq!(
        x("p"),
        [0.0, 10.0, 20.0, 30.0, 40.0],
        "' X X ', the tab a space"
    );
    assert_eq!(x("c").len(), 2);
    assert_eq!(x("n"), [0.0, 10.0]);
    // It is there, for its geometry, with nothing to draw.
    let empty = laid.svg_node("s", "empty");
    assert!(matches!(&empty.kind, SvgNodeKind::Container(children) if children.is_empty()));
}

/// `x`, `y`, `dx`, `dy` per character from the innermost element with
/// one; `dy` carries on; `rotate`'s last value goes on to the end.
#[test]
fn svg_text_places_each_character() {
    let laid = lay_out(&format!(
        "{RESET}{AHEM}<svg id=s width=200 height=100>\
         <text id=t x='10 30 50' y='10%' dy='0 5' rotate='0 90'>XXXX</text>\
         <text id=d x=0 y=50 dx='5 5'><tspan dx=7>X</tspan>X<tspan x='50%' dy=-1em>X</tspan></text>\
         </svg>"
    ));
    let t = cells(&laid, "s", "t");
    let at: Vec<(PointF, f32)> = t.iter().map(|c| (c.0, c.1)).collect();
    assert_eq!(
        at,
        [
            (p(10.0, 10.0), 0.0),
            (p(30.0, 15.0), 90.0),
            (p(50.0, 15.0), 90.0),
            (p(60.0, 15.0), 90.0)
        ]
    );
    let d: Vec<PointF> = cells(&laid, "s", "d").iter().map(|c| c.0).collect();
    assert_eq!(
        d,
        [p(7.0, 50.0), p(22.0, 50.0), p(100.0, 40.0)],
        "the tspan's own dx wins"
    );
}

/// Each chunk (from an `x` or a `y`) is placed by its first character's
/// `text-anchor`, from where it was before any was moved (the values are
/// Chrome 152's).
#[test]
fn svg_text_anchors_each_chunk() {
    let laid = lay_out(&format!(
        "{RESET}{AHEM}<svg id=s width=400 height=100>\
         <text id=m x='100 200' y=10 text-anchor=middle>XX XX</text>\
         <text id=e x=300 y=20 text-anchor=end>XX<tspan x=350>XX</tspan></text>\
         <text id=g x=0 y=30 text-anchor=end dx='0 10'>XX</text>\
         <text id=y x=100 y=40 text-anchor=middle>X<tspan y=50>XX</tspan></text>\
         <text id=dy x=100 y=60 text-anchor=middle>XX<tspan dy=10>XX</tspan></text></svg>"
    ));
    let x = |id| -> Vec<f32> { cells(&laid, "s", id).iter().map(|c| c.0.x).collect() };
    assert_eq!(x("m"), [95.0, 180.0, 190.0, 200.0, 210.0]);
    assert_eq!(x("e"), [280.0, 290.0, 330.0, 340.0]);
    // The chunk's extent takes `dx` in.
    assert_eq!(x("g"), [-30.0, -10.0]);
    // A `y` starts one too; a `dy` doesn't.
    assert_eq!(x("y"), [95.0, 100.0, 110.0]);
    assert_eq!(x("dy"), [80.0, 90.0, 100.0, 110.0]);
}

/// A `<use>`'s copy of a `<text>`: its characters, styled as copied, all
/// found as the `<use>`.
#[test]
fn a_copy_of_text_is_drawn_as_its_use() {
    let laid = lay_out(&format!(
        "{RESET}{AHEM}<svg id=s width=200 height=100><defs>\
         <text id=t y=10>X<tspan id=in dx=5>X</tspan></text></defs>\
         <use id=u href=#t x=20 /></svg>"
    ));
    let u = laid.svg_node("s", "u");
    let SvgNodeKind::Container(copy) = u.kind else {
        panic!("the <use>");
    };
    let SvgNodeKind::Text(text) = &copy[0].kind else {
        panic!("the copy is text: {:?}", copy[0].kind);
    };
    let origins: Vec<PointF> = text.cells.iter().map(|c| c.origin).collect();
    assert_eq!(origins, [p(0.0, 10.0), p(15.0, 10.0)]);
    assert!(text.cells.iter().all(|c| c.node == laid.node("u")));
    assert!(text.runs.iter().all(|r| r.node == laid.node("u")));
}

/// `clip-path: url()` lays the `<clipPath>`'s shapes out for what names it:
/// each shape's path in its space (the shape's `transform` and the
/// `<clipPath>`'s, `objectBoundingBox` units in its bounding box), filled
/// by the shape's `clip-rule`, together a union, the `<clipPath>`'s own
/// `clip-path` another to intersect. A URL to nothing, to anything but a
/// `<clipPath>`, to one with text or to one that names itself is no clip;
/// one with no shape to draw clips everything away.
#[test]
fn clip_paths_lay_out_clip_path_elements() {
    let laid = lay_out(&format!(
        "{RESET}<svg width=0 height=0>
           <clipPath id=two><rect width=10 height=20 /><rect x=30 width=10 height=10 transform='translate(5 0)' clip-rule=evenodd /></clipPath>
           <clipPath id=bbox clipPathUnits=objectBoundingBox transform='scale(0.5)'><rect width=1 height=1 /></clipPath>
           <clipPath id=nested clip-path='url(#two)'><rect width=100 height=100 /></clipPath>
           <clipPath id=text><text>A</text></clipPath>
           <clipPath id=empty></clipPath>
           <clipPath id=hidden><rect width=5 height=5 visibility=hidden /><rect width=5 height=5 display=none /></clipPath>
           <rect id=other />
           <clipPath id=cycle clip-path='url(#cycle)'><rect width=5 height=5 /></clipPath>
         </svg>
         <div id=a style='width: 100px; height: 40px; clip-path: url(#two)'></div>
         <div id=b style='width: 100px; height: 40px; clip-path: url(#bbox)'></div>
         <div id=c style='width: 100px; height: 40px; clip-path: url(#nested)'></div>
         <div id=d style='width: 100px; height: 40px; clip-path: url(#text)'></div>
         <div id=e style='width: 100px; height: 40px; clip-path: url(#missing)'></div>
         <div id=f style='width: 100px; height: 40px; clip-path: url(#other)'></div>
         <div id=g style='width: 100px; height: 40px; clip-path: url(#empty)'></div>
         <div id=h style='width: 100px; height: 40px; clip-path: url(#hidden)'></div>
         <div id=i style='width: 100px; height: 40px; clip-path: url(#cycle)'></div>
         <svg id=s width=100 height=100>
           <g id=group clip-path='url(#bbox)'><rect x=10 y=20 width=40 height=60 /></g>
           <rect id=unstroked x=10 y=10 width=20 height=20 style='clip-path: inset(0) stroke-box' />
           <rect id=stroked x=10 y=10 width=20 height=20 stroke=black stroke-width=4 style='clip-path: inset(0) stroke-box' />
         </svg>"
    ));
    let clip = |id: &str| laid.fragment(id).clip_path.clone();
    let shapes = |paths: &[Path]| -> Vec<(Option<RectF>, FillRule)> {
        paths
            .iter()
            .map(|path| (path.bounds(), path.fill_rule()))
            .collect()
    };
    let a = clip("a").expect("a clip");
    assert_eq!(a.len(), 1);
    assert_eq!(
        shapes(&a[0]),
        [
            (Some(r(0.0, 0.0, 10.0, 20.0)), FillRule::NonZero),
            (Some(r(35.0, 0.0, 10.0, 10.0)), FillRule::EvenOdd)
        ]
    );
    // Half its 100 × 40 border box.
    let b = clip("b").expect("a clip");
    assert_eq!(
        shapes(&b[0]),
        [(Some(r(0.0, 0.0, 50.0, 20.0)), FillRule::NonZero)]
    );
    let c = clip("c").expect("a clip");
    assert_eq!(c.len(), 2);
    assert_eq!(
        shapes(&c[0]),
        [(Some(r(0.0, 0.0, 100.0, 100.0)), FillRule::NonZero)]
    );
    assert_eq!(c[1].len(), 2);
    // `#i`'s names itself: a cycle, no clip.
    for id in ["d", "e", "f", "i"] {
        assert!(clip(id).is_none(), "#{id}");
    }
    for id in ["g", "h"] {
        assert_eq!(clip(id).expect("a clip")[0].len(), 0, "#{id}");
    }
    // An SVG element's, in its own space: the group's bounding box is its
    // rect's.
    let group = laid.svg_node("s", "group").clip_path.expect("a clip");
    assert_eq!(
        shapes(&group[0]),
        [(Some(r(10.0, 20.0, 20.0, 30.0)), FillRule::NonZero)]
    );
    // Its `stroke-box` is its fill's without a stroke.
    let bounds = |id: &str| laid.svg_node("s", id).clip_path.expect("a clip")[0][0].bounds();
    assert_eq!(bounds("unstroked"), Some(r(10.0, 10.0, 20.0, 20.0)));
    assert_eq!(bounds("stroked"), Some(r(8.0, 8.0, 24.0, 24.0)));
}

/// An outermost `<svg>` is one replaced box, what is in it its content,
/// not boxes; an `<svg>` in an `<svg>` isn't an outermost one.
#[test]
fn an_svg_is_one_replaced_box() {
    let laid = crate::page::lay_out(
        "<!doctype html><div id=d><svg id=s width=10 height=10><g id=g>\
         <text id=t>x</text><svg id=inner></svg></g></svg><span id=after>y</span></div>",
    );
    let (view, ids) = (&laid.view, by_id(&laid));
    assert!(has_fragment(view, ids["d"]));
    assert!(has_fragment(view, ids["s"]), "{}", view.dump());
    assert!(has_fragment(view, ids["after"]), "{}", view.dump());
    for id in ["g", "t", "inner"] {
        assert!(!has_fragment(view, ids[id]), "#{id}:\n{}", view.dump());
    }
}

/// The elements with an `id`, by it.
fn by_id(laid: &crate::page::Laid) -> HashMap<String, NodeId> {
    laid.ids
        .iter()
        .map(|(node, id)| (id.clone(), *node))
        .collect()
}

/// The SVG queries' values: `getCTM()` stops at the nearest viewport, past
/// a `<g>` outside it; a `<tspan>` is laid out with its `<text>`; a
/// `<use>`'s `x` and `y` place its copy, not its matrix; a rect with no
/// height has a path, the line around it.
#[test]
fn the_svg_queries() {
    let html = "<!doctype html><style>body { margin: 0 } svg { display: block }</style>\
        <svg width=200 height=100><g transform='translate(5 0)'>\
        <svg x=10 y=10 width=50 height=50><rect id=inner x=1 y=2 width=3 height=4 /></svg></g>\
        <text x=0 y=50>a<tspan id=ts>b</tspan></text>\
        <defs><rect id=r width=10 height=10 /></defs><use id=u href='#r' x=7 y=8 />\
        <rect id=line width=10 height=0 /></svg>";
    let mut page = Page::build(html);
    let view = page.tree.layout(VIEWPORT);
    let Page { dom, token, .. } = &page;
    dom.read(|cx| {
        let document = page.document.get(cx.gc);
        let element = |id| {
            layout::svg_element(
                cx,
                token,
                cx.element_by_id(document, id).expect("the element"),
            )
        };
        let geometry = |id| {
            layout::svg_geometry(
                &view,
                &layout::ScrollOffsets::default(),
                &layout::StickyOffsets::default(),
                element(id),
            )
            .expect("laid out")
        };
        let inner = geometry("inner");
        assert_eq!(
            (inner.ctm.0[4], inner.ctm.0[5]),
            (10.0, 10.0),
            "to the inner <svg>"
        );
        assert_eq!((inner.screen_ctm.0[4], inner.screen_ctm.0[5]), (15.0, 10.0));
        assert!(geometry("ts").bbox.width > 0.0, "laid out with its <text>");
        let used = geometry("u");
        assert_eq!(
            (used.ctm.0[4], used.bbox.x),
            (0.0, 7.0),
            "x places the copy"
        );
        let line = layout::svg_shape_path(&view, element("line")).expect("a path");
        assert!(!line.elements().is_empty(), "the line around it");
    });
}
