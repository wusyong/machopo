//! An outermost `<svg>`'s content: its property nodes, as Chrome makes
//! them, and each shape's fill and stroke.

use std::collections::HashMap;

use compositor_paint::{LineCap, LineJoin, PaintOp, PaintStyle};
use gfx::{Color, NodeId, RectF, SizeF, Transform};
use paint::{DisplayItemKind, PaintArtifact, PrePaint};

use crate::page;

const VIEWPORT: SizeF = SizeF::new(800.0, 600.0);

fn paint_html(html: &str) -> (PrePaint, PaintArtifact, HashMap<String, NodeId>) {
    let (view, ids) = page::lay_out(html, VIEWPORT);
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let artifact = paint::paint(&view, &pre, layout::document_size(&view), &());
    (pre, artifact, ids)
}

const RESET: &str = "<style>html, body { margin: 0 } svg { display: block }</style>";

/// The SVG shapes' items: their element, op and chunk's state.
fn shapes(artifact: &PaintArtifact) -> Vec<(NodeId, &PaintOp, paint::PropertyTreeState)> {
    artifact
        .chunks
        .iter()
        .flat_map(|chunk| {
            artifact.items[chunk.items.clone()]
                .iter()
                .filter(|item| item.kind == DisplayItemKind::SvgShape)
                .map(move |item| (item.client.unwrap(), &item.op, chunk.state))
        })
        .collect()
}

#[test]
fn an_svgs_content_is_under_its_own_property_nodes() {
    let (pre, artifact, ids) = paint_html(&format!(
        "{RESET}<div style='height: 10px'></div>\
         <svg id=s width=200 height=100 viewBox='0 0 100 50' style='padding: 5px'>\
         <g id=g transform='translate(10 0)' opacity='0.5'>\
         <rect id=a width=10 height=10 fill=lime /></g>\
         <svg x=20 y=0 width=30 height=30><rect id=b width=10 height=10 fill=lime /></svg>\
         <rect id=c width=10 height=10 fill=lime /></svg>"
    ));
    let trees = &pre.trees;
    let found = shapes(&artifact);
    let state = |id: &str| {
        found
            .iter()
            .find(|(node, ..)| *node == ids[id])
            .unwrap_or_else(|| panic!("#{id} painted"))
            .2
    };
    let to_root = trees.to_root();
    // The content box's position, then the viewBox's scale of 2.
    assert_eq!(
        to_root[state("c").transform.0],
        Transform([2.0, 0.0, 0.0, 2.0, 5.0, 15.0])
    );
    // The group's transform under it, and its opacity an effect.
    assert_eq!(
        to_root[state("a").transform.0],
        Transform([2.0, 0.0, 0.0, 2.0, 25.0, 15.0])
    );
    assert_eq!(trees.effect[state("a").effect.0].opacity, 0.5);
    assert_eq!(state("c").effect, paint::PropertyTreeState::ROOT.effect);
    // The nested svg's viewport, a clip in its parent's space, then its
    // position.
    let clip = &trees.clip[state("b").clip.0];
    assert_eq!(clip.rect, Some(RectF::new(20.0, 0.0, 30.0, 30.0)));
    assert_eq!(to_root[clip.transform.0], to_root[state("c").transform.0]);
    assert_eq!(
        to_root[state("b").transform.0],
        Transform([2.0, 0.0, 0.0, 2.0, 45.0, 15.0])
    );
    // The outermost svg clips to its content box and doesn't scroll.
    let outer = &trees.clip[state("c").clip.0];
    assert_eq!(outer.rect, Some(RectF::new(5.0, 15.0, 200.0, 100.0)));
    assert!(trees.scroll.is_empty(), "not a scroll container");
}

#[test]
fn fills_and_strokes() {
    let (_, artifact, ids) = paint_html(&format!(
        "{RESET}<svg width=100 height=100 style='color: blue'>\
         <rect id=plain width=10 height=10 />\
         <rect id=both width=10 height=10 fill=currentColor fill-opacity=0.5 stroke=red \
          stroke-width=4 stroke-linecap=round stroke-linejoin=bevel stroke-miterlimit=2 \
          stroke-dasharray='1 2 3' stroke-dashoffset=1 stroke-opacity=0.4 />\
         <rect id=fallback width=10 height=10 fill='url(#none) lime' stroke='url(#none)' />\
         <rect id=hidden width=10 height=10 visibility=hidden />\
         <rect id=thin width=10 height=10 fill=none stroke=red stroke-width=0 />\
         <rect id=clear width=10 height=10 fill=red fill-opacity=0 /></svg>"
    ));
    let found = shapes(&artifact);
    let ops = |id: &str| -> Vec<&PaintOp> {
        found
            .iter()
            .filter(|(node, ..)| *node == ids[id])
            .map(|(_, op, _)| *op)
            .collect()
    };
    let flags = |op: &PaintOp| match op {
        PaintOp::DrawPath { flags, .. } => flags.clone(),
        op => panic!("{op:?}"),
    };
    // Black fill, no stroke: the initial values.
    let plain = ops("plain");
    assert_eq!(plain.len(), 1);
    assert_eq!(flags(plain[0]).color, Color::BLACK);
    assert_eq!(flags(plain[0]).style, PaintStyle::Fill);
    // The fill first, then the stroke.
    let both = ops("both");
    assert_eq!(both.len(), 2);
    assert_eq!(flags(both[0]).color, Color::rgba(0, 0, 255, 128));
    let stroke = flags(both[1]);
    assert_eq!(stroke.style, PaintStyle::Stroke { width: 4.0 });
    assert_eq!(stroke.color, Color::rgba(255, 0, 0, 102));
    assert_eq!(stroke.cap, LineCap::Round);
    assert_eq!(stroke.join, LineJoin::Bevel);
    assert_eq!(stroke.miter_limit, 2.0);
    let dash = stroke.dash.expect("dashed");
    assert_eq!(dash.intervals(), [1.0, 2.0, 3.0, 1.0, 2.0, 3.0]);
    assert_eq!(dash.phase(), 1.0);
    // A `url()` to nothing: its fallback, or nothing.
    let fallback = ops("fallback");
    assert_eq!(fallback.len(), 1);
    assert_eq!(flags(fallback[0]).color, Color::rgb(0, 255, 0));
    assert!(ops("hidden").is_empty());
    assert!(ops("thin").is_empty());
    assert!(ops("clear").is_empty());
}

/// A gradient paints as a shader over the plane: a radial one from its
/// focal circle to its end circle, `reflect` mirrored, colors mixed
/// unpremultiplied; the opacity is the flags' alpha. One without stops
/// paints nothing, not the fallback; one with no length, one color.
#[test]
fn gradients_paint_as_shaders() {
    use compositor_paint::{
        ColorInterpolation, GradientShaderKind, GradientStop, PaintShader, TileMode,
    };
    let (_, artifact, ids) = paint_html(&format!(
        "{RESET}<svg width=100 height=100><defs>\
         <radialGradient id=g fx=0.2 fr=0.1 spreadMethod=reflect>\
           <stop stop-color=red /><stop offset=1 stop-color=blue /></radialGradient>\
         <linearGradient id=r href=#g spreadMethod=repeat />\
         <linearGradient id=empty /><linearGradient id=flat x2=0 href=#g /></defs>\
         <rect id=radial width=10 height=20 fill='url(#g)' fill-opacity=0.5 />\
         <rect id=repeat width=10 height=20 fill=none stroke='url(#r)' />\
         <rect id=nothing width=10 height=10 fill='url(#empty) red' />\
         <rect id=flat width=10 height=10 fill='url(#flat)' fill-opacity=0.5 /></svg>"
    ));
    let found = shapes(&artifact);
    let flags = |id: &str| -> Vec<compositor_paint::PaintFlags> {
        found
            .iter()
            .filter(|(node, ..)| *node == ids[id])
            .map(|(_, op, _)| match op {
                PaintOp::DrawPath { flags, .. } => flags.clone(),
                op => panic!("{op:?}"),
            })
            .collect()
    };
    let radial = flags("radial");
    assert_eq!(radial.len(), 1);
    assert_eq!(radial[0].color.a, 128);
    let Some(PaintShader::Gradient(gradient)) = radial[0].shader.as_deref() else {
        panic!("a gradient over the plane: {:?}", radial[0].shader);
    };
    assert_eq!(
        gradient.kind,
        GradientShaderKind::TwoPointConical {
            start: gfx::PointF::new(0.2, 0.5),
            start_radius: 0.1,
            end: gfx::PointF::new(0.5, 0.5),
            end_radius: 0.5,
        }
    );
    assert_eq!(gradient.tile_mode, TileMode::Mirror);
    assert_eq!(gradient.interpolation, ColorInterpolation::Unpremultiplied);
    assert_eq!(
        gradient.transform,
        gfx::Transform([10.0, 0.0, 0.0, 20.0, 0.0, 0.0])
    );
    assert_eq!(
        gradient.stops,
        [
            GradientStop {
                offset: 0.0,
                color: Color::rgb(255, 0, 0)
            },
            GradientStop {
                offset: 1.0,
                color: Color::rgb(0, 0, 255)
            },
        ]
    );
    let repeat = flags("repeat");
    assert_eq!(repeat.len(), 1);
    assert_eq!(repeat[0].style, PaintStyle::Stroke { width: 1.0 });
    assert_eq!(repeat[0].color.a, 255);
    let Some(PaintShader::Gradient(gradient)) = repeat[0].shader.as_deref() else {
        panic!("a gradient");
    };
    assert_eq!(gradient.tile_mode, TileMode::Repeat);
    assert!(matches!(gradient.kind, GradientShaderKind::Linear { .. }));
    assert!(flags("nothing").is_empty());
    let flat = flags("flat");
    assert_eq!(flat.len(), 1);
    assert_eq!(flat[0].shader, None);
    assert_eq!(flat[0].color, Color::rgba(0, 0, 255, 128));
}

/// Painted again with nothing changed, an `<svg>`'s shapes are still each
/// under its own properties: its content isn't taken from the last paint,
/// whose items a box puts back under its own contents' state.
#[test]
fn an_svgs_content_paints_anew() {
    let (view, _) = page::lay_out(
        &format!(
            "{RESET}<svg width=100 height=100><g transform='translate(10 0)'>\
             <rect width=10 height=10 /></g></svg>"
        ),
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let mut controller = paint::PaintController::default();
    let first = controller.paint(&view, &pre, layout::document_size(&view), &());
    let second = controller.paint(&view, &pre, layout::document_size(&view), &());
    let states = |artifact: &PaintArtifact| -> Vec<paint::PropertyTreeState> {
        shapes(artifact)
            .into_iter()
            .map(|(_, _, state)| state)
            .collect()
    };
    assert_eq!(states(&first).len(), 1);
    assert_eq!(states(&second), states(&first));
}

/// An `<svg>`'s targets for hit testing, as paint records them: its own
/// box, then each shape by its bounds (with its stroke's reach, a 1px
/// stroke's miter here) and each container found by itself (`bounding-
/// box`; by an empty rect), before what is in it, under the shape's or
/// container's own state.
#[test]
fn an_svgs_targets_are_its_shapes_and_bounding_box_containers() {
    let (pre, artifact, _) = paint_html(&format!(
        "{RESET}<svg width=100 height=100><g><rect width=10 height=10 /></g>\
         <g pointer-events=bounding-box transform='translate(20 0)'><rect width=5 height=5 /></g></svg>"
    ));
    let data = &artifact.hit_test;
    let targets: Vec<_> = data
        .chunks
        .iter()
        .flat_map(|chunk| {
            data.rects[chunk.rects.clone()]
                .iter()
                .map(|&r| (r, chunk.state))
        })
        .collect();
    let svg = targets
        .iter()
        .position(|&(rect, _)| rect == RectF::new(0.0, 0.0, 100.0, 100.0))
        .expect("the <svg>'s box");
    let after = &targets[svg + 1..];
    let to_root = pre.trees.to_root();
    let at = |&(rect, state): &(RectF, paint::PropertyTreeState)| {
        to_root[state.transform.0].map_rect(rect)
    };
    assert_eq!(
        after.iter().map(at).collect::<Vec<_>>(),
        [
            RectF::new(-2.0, -2.0, 14.0, 14.0),
            RectF::new(20.0, 0.0, 0.0, 0.0),
            RectF::new(18.0, -2.0, 9.0, 9.0),
        ]
    );
}

/// Under `border-radius`, an `<svg>`'s content is cut to its content box's
/// curve, as an image's is.
#[test]
fn an_svgs_content_is_cut_to_its_corners() {
    let (pre, artifact, _) = paint_html(&format!(
        "{RESET}<svg width=100 height=100 style='border-radius: 10px; overflow: visible'>\
         <rect width=100 height=100 /></svg>"
    ));
    let state = shapes(&artifact)[0].2;
    let rounded = pre
        .trees
        .clip_chain(state.clip)
        .into_iter()
        .any(|clip| pre.trees.clip[clip.0].is_rounded());
    assert!(rounded);
}

/// What is at each point, by `id` (`None` where nothing with one is).
fn hits(html: &str, points: &[(f32, f32)]) -> Vec<Option<String>> {
    let (view, ids) = page::lay_out(html, VIEWPORT);
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    points
        .iter()
        .map(|&(x, y)| {
            let node = paint::hit_test(&view, &pre, gfx::PointF::new(x, y)).node?;
            ids.iter()
                .find(|(_, id)| **id == node)
                .map(|(name, _)| name.clone())
        })
        .collect()
}

fn ids(names: &[&str]) -> Vec<Option<String>> {
    names
        .iter()
        .map(|name| (!name.is_empty()).then(|| name.to_string()))
        .collect()
}

#[test]
fn shapes_are_hit_by_their_fill_and_stroke() {
    let html = format!(
        "{RESET}<svg id=s width=200 height=100>\
         <rect id=filled x=10 y=10 width=30 height=30 />\
         <rect id=outline x=60 y=10 width=30 height=30 fill=none stroke=red stroke-width=6 />\
         <circle id=circle cx=130 cy=25 r=15 />\
         <path id=hole d='M150 50 h40 v40 h-40 z M160 60 h20 v20 h-20 z' fill-rule=evenodd />\
         <line id=dashed x1=10 y1=80 x2=100 y2=80 stroke=red stroke-width=10 stroke-dasharray='10 10' />\
         </svg>"
    );
    #[rustfmt::skip]
    let expected = ids(&[
        "filled",
        // Inside a stroke-only rect: its <svg>.
        "s",
        "outline",
        "circle",
        // Outside the circle's curve, inside its bounds.
        "s",
        "hole",
        "s",
        "dashed",
        // In a dash's gap.
        "s",
        "s",
        // On the right edge, as Skia counts it: inside.
        "filled",
        // Inside the circle's curve, outside its ends' diamond.
        "circle",
        // On the stroke's inner edge.
        "outline",
    ]);
    assert_eq!(
        hits(
            &html,
            &[
                (20.0, 20.0),
                (75.0, 25.0),
                (60.0, 25.0),
                (130.0, 25.0),
                (117.0, 12.0),
                (155.0, 55.0),
                (170.0, 70.0),
                (15.0, 82.0),
                (25.0, 82.0),
                (5.0, 5.0),
                (40.0, 20.0),
                (140.0, 15.0),
                (63.0, 25.0),
            ]
        ),
        expected
    );
}

#[test]
fn hit_testing_goes_through_transforms_clips_and_paint_order() {
    let html = format!(
        "{RESET}<svg id=s width=200 height=100 viewBox='0 0 100 50'>\
         <g transform='translate(10 0)'><rect id=moved width=10 height=10 /></g>\
         <svg x=40 y=0 width=10 height=10><rect id=clipped width=50 height=50 /></svg>\
         <rect id=under x=70 width=20 height=20 /><rect id=over x=80 width=20 height=20 />\
         </svg>"
    );
    assert_eq!(
        hits(
            &html,
            &[
                (25.0, 5.0),
                (5.0, 5.0),
                (85.0, 5.0),
                (85.0, 30.0),
                (150.0, 10.0),
                (170.0, 10.0)
            ]
        ),
        ids(&["moved", "s", "clipped", "s", "under", "over"])
    );
}

#[test]
fn pointer_events_say_what_of_a_shape_counts() {
    let html = format!(
        "{RESET}<svg id=s width=400 height=40>\
         <rect id=none x=0 width=30 height=30 pointer-events=none />\
         <rect id=vfill x=40 width=30 height=30 fill=none pointer-events=visibleFill />\
         <rect id=vstroke x=80 width=30 height=30 stroke=red stroke-width=6 pointer-events=visibleStroke />\
         <rect id=hidden x=120 width=30 height=30 visibility=hidden />\
         <rect id=painted x=160 width=30 height=30 visibility=hidden pointer-events=painted />\
         <rect id=fill x=200 width=30 height=30 fill=none visibility=hidden pointer-events=fill />\
         <circle id=bbox cx=255 cy=15 r=15 fill=none pointer-events=bounding-box />\
         <rect id=all x=280 width=30 height=30 fill=none visibility=hidden pointer-events=all />\
         <g pointer-events=none><rect id=inherits x=320 width=30 height=30 /></g>\
         </svg>"
    );
    #[rustfmt::skip]
    let expected = ids(&[
        "s",
        "vfill",
        // Inside the stroke, which is all that counts.
        "s",
        "vstroke",
        "s",
        "painted",
        "fill",
        // A corner of its bounding box.
        "bbox",
        "all",
        "s",
    ]);
    assert_eq!(
        hits(
            &html,
            &[
                (15.0, 15.0),
                (55.0, 15.0),
                (95.0, 15.0),
                (80.0, 15.0),
                (135.0, 15.0),
                (175.0, 15.0),
                (215.0, 15.0),
                (242.0, 2.0),
                (295.0, 15.0),
                (335.0, 15.0),
            ]
        ),
        expected
    );
}

/// `elementsFromPoint` lists a shape, then the `<svg>`, not the containers
/// around it, as Chrome 152 does (`elementsFromPoint-svg.html` asks for
/// them); a container whose `pointer-events` is `bounding-box` is listed
/// where something in it is.
#[test]
fn every_element_at_a_point_lists_a_shape_and_its_svg() {
    let (view, ids) = page::lay_out(
        "<html id=html><body id=body style='margin: 0'><style>svg { display: block }</style>\
         <svg id=s width=100 height=100><g id=g><rect id=r width=10 height=10 /></g>\
         <rect id=other x=50 width=10 height=10 />\
         <svg id=bb y=50 pointer-events=bounding-box><a id=a href=#x><rect id=b width=10 height=10 /></a></svg></svg>",
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let list = |x: f32, y: f32| -> Vec<String> {
        paint::hit_test_all(&view, &pre, gfx::PointF::new(x, y))
            .into_iter()
            .map(|node| ids.iter().find(|(_, id)| **id == node).unwrap().0.clone())
            .collect()
    };
    assert_eq!(list(5.0, 5.0), ["r", "s", "body", "html"]);
    // `bounding-box` is inherited: the `<a>` is listed too.
    assert_eq!(list(5.0, 55.0), ["b", "a", "bb", "s", "body", "html"]);
}

/// A `<use>`'s copy is hit as the `<use>` (Chrome retargets from its
/// shadow tree), each part where it is drawn: copies of one element have
/// property nodes of their own.
#[test]
fn a_use_is_hit_where_its_copy_is() {
    let html = format!(
        "{RESET}<svg id=s width=200 height=100>\
         <defs><g id=g><rect id=a width=10 height=10 transform='translate(20 0)' />\
         <rect id=b width=10 height=10 transform='translate(40 0)' /></g>\
         <symbol id=sym viewBox='0 0 10 10'><rect width=100 height=10 /></symbol></defs>\
         <use id=u href=#g /><use id=v href=#g y=50 />\
         <use id=w href=#sym x=100 width=20 height=20 />\
         <use id=none href=#g x=100 y=50 pointer-events=none /></svg>"
    );
    #[rustfmt::skip]
    let expected = ids(&[
        "u",
        "u",
        // Where the original would be: nothing drawn there.
        "s",
        "v",
        "v",
        "w",
        // Outside the symbol's viewport.
        "s",
        "s",
    ]);
    assert_eq!(
        hits(
            &html,
            &[
                (25.0, 5.0),
                (45.0, 5.0),
                (5.0, 5.0),
                (25.0, 55.0),
                (45.0, 55.0),
                (110.0, 10.0),
                (125.0, 10.0),
                (125.0, 55.0),
            ]
        ),
        expected
    );
}

/// SVG text's glyphs, each run filled then stroked with its element's
/// paint, turned glyphs with their angles; a preserved tab is a space's
/// glyph.
#[test]
fn svg_text_paints_its_glyphs() {
    use compositor_paint::PaintShader;
    let (_, artifact, ids) = paint_html(&format!(
        "{RESET}<style>text {{ font: 10px Ahem }}</style><svg width=100 height=100><defs>\
         <linearGradient id=g><stop stop-color=red /><stop offset=1 stop-color=blue /></linearGradient></defs>\
         <text id=t y=10 fill='url(#g)' stroke=lime stroke-width=2>XX<tspan id=s fill=blue stroke=none rotate=90>X</tspan></text>\
         <text id=tab y=30 xml:space=preserve>X&#9;X</text><text id=space y=40>X X</text>\
         </svg>"
    ));
    let glyph_ops: Vec<(NodeId, &PaintOp)> = artifact
        .items
        .iter()
        .filter(|item| item.kind == DisplayItemKind::SvgText)
        .map(|item| (item.client.unwrap(), &item.op))
        .collect();
    let glyphs_of = |id: &str| match glyph_ops
        .iter()
        .find(|(node, _)| *node == ids[id])
        .unwrap()
        .1
    {
        PaintOp::DrawGlyphs { glyphs, .. } => glyphs.clone(),
        op => panic!("{op:?}"),
    };
    assert_eq!(glyphs_of("tab"), glyphs_of("space"));
    let glyph_ops = &glyph_ops[..3];
    assert_eq!(
        glyph_ops.len(),
        3,
        "the text's fill and stroke, the tspan's fill"
    );
    let PaintOp::DrawGlyphs {
        glyphs,
        positions,
        flags,
        rotations,
        ..
    } = glyph_ops[0].1
    else {
        panic!("{:?}", glyph_ops[0].1);
    };
    assert_eq!(glyph_ops[0].0, ids["t"]);
    assert_eq!(glyphs.len(), 2);
    assert_eq!(
        &positions[..],
        &[gfx::PointF::new(0.0, 10.0), gfx::PointF::new(10.0, 10.0)]
    );
    assert!(rotations.is_empty());
    let Some(PaintShader::Gradient(gradient)) = flags.shader.as_deref() else {
        panic!("a gradient");
    };
    // Over the text's bounding box: its cells, the turned one too (from x
    // = 18 to 28, y = 10 to 20).
    assert_eq!(
        gradient.transform,
        gfx::Transform([28.0, 0.0, 0.0, 18.0, 0.0, 2.0])
    );
    let PaintOp::DrawGlyphs { flags, .. } = glyph_ops[1].1 else {
        panic!();
    };
    assert_eq!(flags.style, PaintStyle::Stroke { width: 2.0 });
    assert_eq!(flags.color, Color::rgb(0, 255, 0));
    let PaintOp::DrawGlyphs {
        flags, rotations, ..
    } = glyph_ops[2].1
    else {
        panic!();
    };
    assert_eq!(glyph_ops[2].0, ids["s"]);
    assert_eq!(flags.color, Color::rgb(0, 0, 255));
    assert_eq!(&rotations[..], &[90.0]);
}

/// A character's cell is found: its innermost element; between chunks,
/// above the ascent or past the cell (a stroke too), nothing; a turned
/// cell where it turned to (about (200, 20): x from 198 to 208, y from 20
/// to 30).
#[test]
fn svg_text_is_hit_by_its_cells() {
    let html = format!(
        "{RESET}<style>text {{ font: 10px Ahem }}</style><svg id=s width=300 height=100>\
         <text id=t x='0 100' y=10 stroke=black stroke-width=10>X<a id=a href=#x><tspan id=in>X</tspan>X</a></text>\
         <text id=r x=200 y=20 rotate=90>X</text>\
         <text id=n x=0 y=50 pointer-events=none>X</text>\
         <text id=u x=0 y=70 fill=none>X</text></svg>"
    );
    assert_eq!(
        hits(
            &html,
            &[
                (5.0, 5.0),
                (105.0, 5.0),
                (115.0, 5.0),
                (50.0, 5.0),
                (5.0, 1.0),
                (5.0, 11.0),
                (13.0, 5.0),
                (203.0, 25.0),
                (205.0, 15.0),
                (5.0, 45.0),
                (5.0, 65.0),
            ]
        ),
        // Neither filled nor stroked: not found.
        ids(&["t", "in", "a", "s", "s", "t", "s", "r", "s", "s", "s"])
    );
}

/// `elementsFromPoint` on a character lists the element it is in,
/// innermost, not the `<text>` or `<a>` around it (Chrome 152 lists an
/// element for its own text only), then what is around the `<text>`.
#[test]
fn every_element_at_a_character_lists_the_elements_around_it() {
    let (view, ids) = page::lay_out(
        "<html id=html><body id=body style='margin: 0'><style>svg { display: block } text { font: 10px Ahem }</style>\
         <svg id=s width=100 height=100><text id=t y=10><a id=a href=#x><tspan id=in>X</tspan></a></text></svg>",
        VIEWPORT,
    );
    let pre = paint::pre_paint(&view, &Default::default(), 1.0);
    let names: Vec<String> = paint::hit_test_all(&view, &pre, gfx::PointF::new(5.0, 5.0))
        .into_iter()
        .map(|node| ids.iter().find(|(_, id)| **id == node).unwrap().0.clone())
        .collect();
    assert_eq!(names, ["in", "s", "body", "html"]);
}
