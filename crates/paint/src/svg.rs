//! An outermost `<svg>`'s content: pre-paint makes each SVG element's
//! transform, its opacity and a nested `<svg>`'s viewport clip property
//! nodes (Blink's `PaintPropertyTreeBuilder` for SVG children, and
//! `ReplacedContentTransform` for the `viewBox`), and paint draws each
//! shape's fill, then its stroke, as items of their own (`SVGShapePainter`)
//! in tree order, a gradient as a shader. Hit testing finds a shape by its geometry, as its
//! `pointer-events` says (`LayoutSVGShape::NodeAtPoint`), with `kurbo`
//! where Blink has Skia.

use base::numerics::clamp_round;
use compositor_paint::{
    ColorInterpolation, Dash, GradientShader, GradientShaderKind, GradientStop, LineCap, LineJoin,
    PaintFlags, PaintOp, PaintStyle, TileMode,
};
use computed_style::{
    ComputedStyle, PointerEvents, StrokeLinecap, StrokeLinejoin, SvgPaint, SvgPaintFallback,
};
use gfx::{Color, NodeId, Path, PointF, RRectF, RectF, SizeF, Transform};
use kurbo::Shape;
use layout::{
    BoxFragment, Spread, SvgGradient, SvgGradientKind, SvgNode, SvgNodeKind, SvgPaintServer,
    SvgShape, SvgText, SvgTextCell,
};

use std::sync::Arc;

use crate::hit_test::cell_bounds;
use crate::property_trees::{NodeKey, NodeOwner, NodeRole};
use crate::{DisplayItem, DisplayItemKind, Painter, PrePaint, PropertyTreeState, ScrollNodeId};

impl PrePaint {
    /// The property nodes of `fragment`'s SVG content (if it has any),
    /// under `contents`, its border box at `offset`: the content box's
    /// position and the `viewBox`, then each element's.
    pub(crate) fn walk_svg(
        &mut self,
        fragment: &BoxFragment,
        offset: PointF,
        contents: PropertyTreeState,
        frame: Option<NodeId>,
    ) {
        let Some(svg) = &fragment.svg else {
            return;
        };
        let (b, p) = (&fragment.border, &fragment.padding);
        let content_box =
            Transform::translate(offset.x + b.left + p.left, offset.y + b.top + p.top);
        let mut state = contents;
        let owner = fragment.node.map(|node| NodeOwner::Box {
            frame,
            node,
            pseudo: fragment.pseudo,
        });
        state.transform = self.transform_node(
            owner.map(|owner| NodeKey {
                owner,
                role: NodeRole::SvgContent,
            }),
            contents.transform,
            content_box.then(&svg.content_transform),
            false,
        );
        let mut index = 0;
        for node in &svg.children {
            self.walk_svg_node(
                node,
                state,
                fragment.node.map(|svg| (frame, svg)),
                &mut index,
            );
        }
    }

    /// `node` under `parent`, the `index`th element of the content of
    /// `svg` (its element, in its frame) in tree order.
    fn walk_svg_node(
        &mut self,
        node: &SvgNode,
        parent: PropertyTreeState,
        svg: Option<(Option<NodeId>, NodeId)>,
        index: &mut u32,
    ) {
        let owner = svg.map(|(frame, svg)| NodeOwner::Svg {
            frame,
            svg,
            index: *index,
        });
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per node of an SVG document: fewer than 2^32"
        )]
        {
            *index += 1;
        }
        let key = |role| owner.map(|owner| NodeKey { owner, role });
        let mut state = parent;
        if !node.transform.is_identity() {
            state.transform = self.transform_node(
                key(NodeRole::SvgTransform),
                parent.transform,
                node.transform,
                false,
            );
        }
        // Its `clip-path` cuts it and what is in it, in its own space.
        if let Some(clip_path) = &node.clip_path {
            for (i, paths) in clip_path.iter().enumerate() {
                state.clip = self.clip_paths_node(
                    key(NodeRole::ClipPath(i)),
                    state.clip,
                    state.transform,
                    paths,
                    Transform::IDENTITY,
                );
            }
        }
        // A group's opacity and filter apply to it painted whole (`<g
        // opacity>` over overlapping shapes), as an effect node does, what
        // the filter makes cut by the clip-path after it.
        let filter = crate::filter::image_filter(&node.style);
        if node.style.opacity < 1.0 || filter.is_some() {
            state.effect =
                self.effect_node(key(NodeRole::SvgEffect), state, node.style.opacity, filter);
        }
        Arc::make_mut(&mut self.svg_states).insert(node as *const SvgNode, state);
        let SvgNodeKind::Container(children) = &node.kind else {
            return;
        };
        let mut inner = state;
        if let Some(clip) = node.clip {
            inner.clip = self.clip_node(
                key(NodeRole::SvgClip),
                state.clip,
                state.transform,
                RRectF::from_rect(clip),
            );
        }
        if !node.content_transform.is_identity() {
            inner.transform = self.transform_node(
                key(NodeRole::SvgContentTransform),
                inner.transform,
                node.content_transform,
                false,
            );
        }
        for child in children {
            self.walk_svg_node(child, inner, svg, index);
        }
    }
}

impl Painter<'_> {
    /// `fragment`'s SVG content, each shape where pre-paint put it, in
    /// `scroll` (the innermost scroll container `fragment` is in).
    pub(crate) fn svg(&mut self, fragment: &BoxFragment, scroll: Option<ScrollNodeId>) {
        let Some(svg) = &fragment.svg else {
            return;
        };
        for node in &svg.children {
            self.svg_node(node, scroll);
        }
    }

    /// An SVG element, with its targets for hit testing: a shape's bounds,
    /// each character's cell, and a container, found by itself only with
    /// `pointer-events: bounding-box` (by its children's bounds, which the
    /// regions don't need: an empty rect), before its children.
    fn svg_node(&mut self, node: &SvgNode, scroll: Option<ScrollNodeId>) {
        let pre = self.pre;
        let state = || pre.svg_states[&(node as *const SvgNode)];
        // A character's too: the `<text>`'s.
        let wheel = node.style.inside_blocking_wheel_event_handler;
        match &node.kind {
            SvgNodeKind::Container(children) => {
                if node.style.pointer_events == PointerEvents::BoundingBox {
                    self.push_hit(RectF::default(), state(), scroll, None, wheel);
                }
                for child in children {
                    self.svg_node(child, scroll);
                }
            }
            SvgNodeKind::Shape(shape) => {
                self.push_hit(shape_bounds(node, shape), state(), scroll, None, wheel);
                self.svg_shape(node, shape);
            }
            SvgNodeKind::Text(text) => {
                for cell in &text.cells {
                    self.push_hit(cell_bounds(cell), state(), scroll, None, wheel);
                }
                self.svg_text(node, text);
            }
            SvgNodeKind::Hidden(_) => return,
        }
        // Its outline (a focus ring) after what it draws, around that, in
        // its own space: turned with it (`SVGModelObjectPainter::
        // PaintOutline`).
        if node.style.outline_style.is_drawn()
            && let Some(rect) = outline_rect(node)
        {
            let state = self.pre.svg_states[&(node as *const SvgNode)];
            self.outline(
                Some(node.node),
                &node.style,
                rect,
                [SizeF::default(); 4],
                state,
            );
        }
    }

    /// Each run's glyphs filled, then stroked (`SVGInlineTextBoxPainter`),
    /// as a shape's are.
    fn svg_text(&mut self, node: &SvgNode, text: &SvgText) {
        let state = self.pre.svg_states[&(node as *const SvgNode)];
        for run in &text.runs {
            let style = &run.style;
            if !style.is_visible() || run.glyphs.is_empty() {
                continue;
            }
            let svg = &style.svg;
            let glyphs = |flags: PaintFlags, visual_rect: RectF| DisplayItem {
                client: Some(run.node),
                kind: DisplayItemKind::SvgText,
                visual_rect,
                op: PaintOp::DrawGlyphs {
                    font: run.font.clone(),
                    size: run.font_size,
                    glyphs: Arc::from(&run.glyphs[..]),
                    positions: Arc::from(&run.positions[..]),
                    rotations: Arc::from(&run.rotations[..]),
                    flags,
                    embolden: run.embolden,
                    skew: run.skew,
                    variations: run.variations.clone(),
                    bounds: visual_rect,
                },
            };
            let ink = run.bounds.inset(-svg_glyph_reach(run.font_size));
            if let Some(flags) =
                paint_flags(style, &svg.fill, run.fill_server.as_ref(), svg.fill_opacity)
            {
                self.push(state, glyphs(flags, ink));
            }
            if run.stroke_width > 0.0
                && let Some(paint) = paint_flags(
                    style,
                    &svg.stroke,
                    run.stroke_server.as_ref(),
                    svg.stroke_opacity,
                )
            {
                let flags = PaintFlags {
                    cap: match svg.stroke_linecap {
                        StrokeLinecap::Butt => LineCap::Butt,
                        StrokeLinecap::Round => LineCap::Round,
                        StrokeLinecap::Square => LineCap::Square,
                    },
                    join: match svg.stroke_linejoin {
                        StrokeLinejoin::Miter => LineJoin::Miter,
                        StrokeLinejoin::Round => LineJoin::Round,
                        StrokeLinejoin::Bevel => LineJoin::Bevel,
                    },
                    miter_limit: svg.stroke_miterlimit,
                    dash: Dash::new(&run.dashes, run.dash_offset),
                    style: PaintStyle::Stroke {
                        width: run.stroke_width,
                    },
                    ..paint
                };
                let visual_rect = stroke_bounds(ink, &flags);
                self.push(state, glyphs(flags, visual_rect));
            }
        }
    }

    /// The fill, then the stroke (`paint-order`'s default).
    fn svg_shape(&mut self, node: &SvgNode, shape: &SvgShape) {
        let style = &node.style;
        let Some(path) = &shape.path else {
            return;
        };
        if !style.is_visible() {
            return;
        }
        let state = self.pre.svg_states[&(node as *const SvgNode)];
        let Some(bounds) = path.bounds() else {
            return;
        };
        let svg = &style.svg;
        if let Some(flags) = paint_flags(
            style,
            &svg.fill,
            shape.fill_server.as_ref(),
            svg.fill_opacity,
        ) {
            self.push(
                state,
                DisplayItem {
                    client: Some(node.node),
                    kind: DisplayItemKind::SvgShape,
                    visual_rect: bounds,
                    op: PaintOp::DrawPath {
                        path: path.clone(),
                        flags,
                    },
                },
            );
        }
        if shape.stroke_width > 0.0
            && let Some(paint) = paint_flags(
                style,
                &svg.stroke,
                shape.stroke_server.as_ref(),
                svg.stroke_opacity,
            )
        {
            let flags = PaintFlags {
                cap: match svg.stroke_linecap {
                    StrokeLinecap::Butt => LineCap::Butt,
                    StrokeLinecap::Round => LineCap::Round,
                    StrokeLinecap::Square => LineCap::Square,
                },
                join: match svg.stroke_linejoin {
                    StrokeLinejoin::Miter => LineJoin::Miter,
                    StrokeLinejoin::Round => LineJoin::Round,
                    StrokeLinejoin::Bevel => LineJoin::Bevel,
                },
                miter_limit: svg.stroke_miterlimit,
                dash: Dash::new(&shape.dashes, shape.dash_offset),
                style: PaintStyle::Stroke {
                    width: shape.stroke_width,
                },
                ..paint
            };
            let visual_rect = stroke_bounds(bounds, &flags);
            self.push(
                state,
                DisplayItem {
                    client: Some(node.node),
                    kind: DisplayItemKind::SvgShape,
                    visual_rect,
                    op: PaintOp::DrawPath {
                        path: path.clone(),
                        flags,
                    },
                },
            );
        }
    }
}

/// What an SVG element's outline goes around, in its space (its
/// `transform` aside): what it draws, a stroke's half width included, a
/// container's children's together (Blink's
/// `VisualRectInLocalSVGCoordinates`); `None` if it draws nothing.
fn outline_rect(node: &SvgNode) -> Option<RectF> {
    let rect = match &node.kind {
        SvgNodeKind::Shape(shape) => {
            let bbox = shape.path.as_ref().and(shape.bbox)?;
            let stroked = shape.stroke_width > 0.0 && node.style.svg.stroke != SvgPaint::None;
            if stroked {
                bbox.inset(-shape.stroke_width / 2.0)
            } else {
                bbox
            }
        }
        SvgNodeKind::Text(text) => text.bounds?,
        SvgNodeKind::Container(children) => children
            .iter()
            .filter_map(|child| Some(child.transform.map_rect(outline_rect(child)?)))
            .reduce(|a, b| a.union(&b))?,
        SvgNodeKind::Hidden(_) => return None,
    };
    Some(node.content_transform.map_rect(rect))
}

/// A shape's bounds in its space, its stroke's reach included: where hit
/// testing's regions take it to be.
pub(crate) fn shape_bounds(node: &SvgNode, shape: &SvgShape) -> RectF {
    let Some(bounds) = shape.path.as_ref().and_then(Path::bounds) else {
        return RectF::default();
    };
    let svg = &node.style.svg;
    let mut flags = PaintFlags::stroke(Color::BLACK, shape.stroke_width);
    flags.cap = match svg.stroke_linecap {
        StrokeLinecap::Square => LineCap::Square,
        _ => LineCap::Butt,
    };
    flags.join = match svg.stroke_linejoin {
        StrokeLinejoin::Miter => LineJoin::Miter,
        _ => LineJoin::Round,
    };
    flags.miter_limit = svg.stroke_miterlimit;
    stroke_bounds(bounds, &flags)
}

/// What `paint` paints with, its opacity applied, as fill flags; `None`
/// for nothing. A `url()` paints with what layout found for it
/// (`server`), or, if that isn't a paint server it can use, its
/// fallback, or nothing without one (SVG 2, "Specifying paint").
fn paint_flags(
    style: &ComputedStyle,
    paint: &SvgPaint,
    server: Option<&SvgPaintServer>,
    opacity: f32,
) -> Option<PaintFlags> {
    let color = match (paint, server) {
        (SvgPaint::None, _) => return None,
        (SvgPaint::Color(color), _) => style.resolve(*color),
        (SvgPaint::Url(..), Some(SvgPaintServer::None)) => return None,
        (SvgPaint::Url(..), Some(SvgPaintServer::Color(color))) => *color,
        (SvgPaint::Url(..), Some(SvgPaintServer::Gradient(gradient))) => {
            let alpha: u8 = clamp_round(255.0 * opacity);
            if alpha == 0 {
                return None;
            }
            return Some(PaintFlags {
                color: Color {
                    a: alpha,
                    ..Color::BLACK
                },
                ..PaintFlags::shader(gradient_shader(gradient))
            });
        }
        (SvgPaint::Url(_, fallback), None) => match fallback {
            SvgPaintFallback::Color(color) => style.resolve(*color),
            SvgPaintFallback::None | SvgPaintFallback::Absent => return None,
        },
    };
    let alpha: u8 = clamp_round(f32::from(color.a) * opacity);
    (alpha > 0).then(|| PaintFlags::fill(Color { a: alpha, ..color }))
}

/// An SVG gradient as a shader over the plane: from the focal circle to
/// the end circle for a radial one, its colors mixed unpremultiplied (as
/// Blink's `Gradient` does for SVG).
fn gradient_shader(gradient: &SvgGradient) -> GradientShader {
    GradientShader {
        kind: match gradient.kind {
            SvgGradientKind::Linear { start, end } => GradientShaderKind::Linear { start, end },
            SvgGradientKind::Radial {
                focal,
                focal_radius,
                center,
                radius,
            } => GradientShaderKind::TwoPointConical {
                start: focal,
                start_radius: focal_radius,
                end: center,
                end_radius: radius,
            },
        },
        stops: gradient
            .stops
            .iter()
            .map(|&(offset, color)| GradientStop { offset, color })
            .collect(),
        tile_mode: match gradient.spread {
            Spread::Pad => TileMode::Clamp,
            Spread::Reflect => TileMode::Mirror,
            Spread::Repeat => TileMode::Repeat,
        },
        transform: gradient.transform,
        interpolation: ColorInterpolation::Unpremultiplied,
    }
}

/// A stroked path's reach past its bounds, as `compositor_paint` takes it
/// for an op's bounds: half the width, or as far as a miter or a square
/// cap goes.
/// How far past its cells an SVG run's glyphs at `size` may draw, every
/// way: about the font size, what its items' visual rects allow for.
pub(crate) fn svg_glyph_reach(size: f32) -> f32 {
    size
}

fn stroke_bounds(bounds: RectF, flags: &PaintFlags) -> RectF {
    let PaintStyle::Stroke { width } = flags.style else {
        return bounds;
    };
    let mut reach: f32 = 1.0;
    if flags.join == LineJoin::Miter {
        reach = reach.max(flags.miter_limit);
    }
    if flags.cap == LineCap::Square {
        reach = reach.max(std::f32::consts::SQRT_2);
    }
    bounds.inset(-width / 2.0 * reach)
}

/// Whether `point`, in the shape's own space, is on the parts of it that
/// its `pointer-events` lets hit testing find: its fill area, its stroke
/// area (a shape whose fill or stroke is `none` isn't "painted" there),
/// or its bounding box; for the `visible*` values, and `auto`
/// (`visiblePainted`), only while it is `visibility: visible`.
pub(crate) fn shape_hit(node: &SvgNode, shape: &SvgShape, point: PointF) -> bool {
    let style = &node.style;
    let Some(path) = &shape.path else {
        return false;
    };
    let fill_painted = !matches!(style.svg.fill, SvgPaint::None);
    let stroke_painted = !matches!(style.svg.stroke, SvgPaint::None);
    let (visible_only, fill, stroke) = match style.pointer_events {
        PointerEvents::None => return false,
        PointerEvents::Auto | PointerEvents::VisiblePainted => (true, fill_painted, stroke_painted),
        PointerEvents::VisibleFill => (true, true, false),
        PointerEvents::VisibleStroke => (true, false, true),
        PointerEvents::Visible => (true, true, true),
        PointerEvents::Painted => (false, fill_painted, stroke_painted),
        PointerEvents::Fill => (false, true, false),
        PointerEvents::Stroke => (false, false, true),
        PointerEvents::All => (false, true, true),
        PointerEvents::BoundingBox => {
            let bounds = path.to_kurbo().bounding_box();
            return bounds.contains(kurbo::Point::new(point.x.into(), point.y.into()));
        }
    };
    if visible_only && !style.is_visible() {
        return false;
    }
    (fill && path.contains(point, style.svg.fill_rule))
        || (stroke && stroke_contains(style, shape, path, point))
}

/// Whether `point`, in the shape's own space, is in its fill area or its
/// stroke area (`stroke`), whatever its paint, visibility and
/// `pointer-events` (`SVGGeometryElement`'s `isPointInFill` and
/// `isPointInStroke`, Blink's `Path::Contains` and `StrokeContains`).
pub fn svg_shape_contains(node: &SvgNode, point: PointF, stroke: bool) -> bool {
    let SvgNodeKind::Shape(shape) = &node.kind else {
        return false;
    };
    let Some(path) = &shape.path else {
        return false;
    };
    if stroke {
        stroke_contains(&node.style, shape, path, point)
    } else {
        path.contains(point, node.style.svg.fill_rule)
    }
}

/// Inside the stroke's outline, or on its edge, as paint strokes it: its
/// width, ends, joins and dashes (`Dash::new`'s, or solid); nothing
/// without a width.
fn stroke_contains(style: &ComputedStyle, shape: &SvgShape, path: &Path, point: PointF) -> bool {
    let svg = &style.svg;
    let dash = Dash::new(&shape.dashes, shape.dash_offset);
    let stroke = gfx::StrokeStyle {
        width: shape.stroke_width,
        cap: match svg.stroke_linecap {
            StrokeLinecap::Butt => LineCap::Butt,
            StrokeLinecap::Round => LineCap::Round,
            StrokeLinecap::Square => LineCap::Square,
        },
        join: match svg.stroke_linejoin {
            StrokeLinejoin::Miter => LineJoin::Miter,
            StrokeLinejoin::Round => LineJoin::Round,
            StrokeLinejoin::Bevel => LineJoin::Bevel,
        },
        miter_limit: svg.stroke_miterlimit,
        dash: dash.as_ref().map(|dash| (dash.intervals(), dash.phase())),
    };
    path.stroke_contains(point, &stroke, 0.05)
}

/// Whether `point`, in the `<text>`'s space, is on a character's cell, as
/// its `pointer-events` lets hit testing find it: for text, the fill and
/// the stroke both are the cell (Blink's `SVGInlineTextBox`'s
/// `NodeAtPoint`), painted if its `fill` or `stroke` isn't `none`.
pub(crate) fn text_cell_hit(cell: &SvgTextCell, point: PointF) -> bool {
    let style = &cell.style;
    let painted =
        !matches!(style.svg.fill, SvgPaint::None) || !matches!(style.svg.stroke, SvgPaint::None);
    let (visible_only, counts) = match style.pointer_events {
        PointerEvents::None => return false,
        PointerEvents::Auto | PointerEvents::VisiblePainted => (true, painted),
        PointerEvents::VisibleFill | PointerEvents::VisibleStroke | PointerEvents::Visible => {
            (true, true)
        }
        PointerEvents::Painted => (false, painted),
        PointerEvents::Fill
        | PointerEvents::Stroke
        | PointerEvents::All
        | PointerEvents::BoundingBox => (false, true),
    };
    counts && !(visible_only && !style.is_visible()) && cell.contains(point)
}
