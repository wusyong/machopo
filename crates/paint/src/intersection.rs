//! Intersection Observer's geometry (Blink's `IntersectionGeometry`, which
//! maps through `GeometryMapper` and paint's property trees): where a target
//! is, how much of it shows in its root, and the root's rect, from what
//! pre-paint placed, after the lifecycle. The same trees hit testing and
//! paint use, so transforms, scroll offsets and clips (scroll containers',
//! an `<svg>`'s, an iframe's content box) are where they are painted.
//!
//! All of it is worked out in the main frame's viewport (CSS px): the root
//! space less the viewport's scroll offset, but for what is fixed to it,
//! which pre-paint already places there. Each rect is then given in its
//! document's viewport.

use std::collections::HashSet;

use computed_style::SvgPaint;
use gfx::{MarginLength, Margins, NodeId, NodeIdHashMap, NodeIdHashSet, PointF, RectF, SizeF};
use layout::{BoxFragment, SvgNode, SvgNodeKind};

use crate::hit_test::VisualOverflow;
use crate::{ClipNodeId, PaintArtifact, PrePaint, PropertyTreeState, TransformNodeId};

/// An observer's root (the spec's "intersection root").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntersectionRoot {
    /// The top-level document's viewport: the main frame's.
    Implicit,
    /// An element: its padding box if it clips its overflow, else its
    /// border box. Only its descendants (in the fragment tree, which is the
    /// containing block chain) intersect it.
    Element(NodeId),
    /// A document: its viewport (the main frame's, or an iframe's content
    /// box). Only what is in it intersects it.
    Document(NodeId),
}

/// What finding out whether a target is visible takes besides the trees:
/// what was painted, and the DOM.
pub struct Occlusion<'a> {
    /// What was painted as the trees place it: how far each element's
    /// paint reaches.
    pub artifact: &'a PaintArtifact,
    /// Whether the second node is the first or in it, in the flat tree
    /// and through the frames (a child frame's document is in its iframe):
    /// what is in a target is the target as what covers it (Blink's hit
    /// test stops at it).
    pub contains: &'a dyn Fn(NodeId, NodeId) -> bool,
}

/// One target of one observer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IntersectionRequest {
    pub target: NodeId,
    pub root: IntersectionRoot,
    /// Grows (or shrinks) the root's rect; `None` where it doesn't apply (a
    /// target in a document not same origin-domain with the implicit
    /// root's).
    pub root_margin: Option<Margins>,
    /// Grows (or shrinks) each scroll container's clip between the target
    /// and the root.
    pub scroll_margin: Margins,
    /// Whether to find out if the target is visible (its observer's
    /// `trackVisibility`).
    pub visibility: bool,
}

/// What [`intersections`] found for a request.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct IntersectionGeometry {
    /// The target's bounding box, in its document's viewport; `None` if it
    /// has no box, or an explicit root's document doesn't have it under the
    /// root (the spec skips to an entry of zero rects then).
    pub target: Option<RectF>,
    /// The part of it in the root, cut by the clips between them, in its
    /// document's viewport; `None` if it doesn't intersect the root (edges
    /// touching, or no area, still intersect: the spec's edge-inclusive
    /// intersection).
    pub intersection: Option<RectF>,
    /// The root's rect, its margin applied, in the root's document's
    /// viewport; `None` if the root has no box.
    pub root: Option<RectF>,
    /// Whether the target is visible, if asked and it meets the root
    /// (Blink's `ComputeVisibilityInfo`): nothing distorts how it shows,
    /// and nothing else is on top anywhere on its rect (the whole of it,
    /// not only what shows in the root, as Blink tests); `false` otherwise,
    /// and with nothing painted yet.
    pub visible: bool,
}

/// Where an element was found in the walk.
#[derive(Default)]
struct Found {
    /// Its boxes, each with the state its rect is in.
    boxes: Vec<(RectF, PropertyTreeState)>,
    /// The iframe whose child frame it is in; `None` in the main frame.
    frame: Option<NodeId>,
    /// The roots asked for that are around it in the fragment tree.
    inside: Vec<NodeId>,
    /// As a root, from its first box: its rect (the padding box if it
    /// clips its overflow, else the border box) in its own space (outside
    /// its scroll offset), and the clip its content is under (its own
    /// overflow clip and those around it).
    as_root: Option<(RectF, TransformNodeId, ClipNodeId)>,
}

/// Each request's geometry, from the frame tree `layout_view` shows as
/// `pre` placed it; `document` is the main frame's, `viewport` its
/// viewport's size; `occlusion` what a request for visibility takes.
pub fn intersections(
    layout_view: &BoxFragment,
    pre: &PrePaint,
    document: NodeId,
    viewport: SizeF,
    requests: &[IntersectionRequest],
    occlusion: Option<&Occlusion<'_>>,
) -> Vec<IntersectionGeometry> {
    let _span = trace_event::trace_event!("IntersectionObserverController::ComputeIntersections");
    let roots: NodeIdHashSet = requests
        .iter()
        .filter_map(|r| match r.root {
            IntersectionRoot::Element(root) => Some(root),
            _ => None,
        })
        .collect();
    let wanted: NodeIdHashSet = requests
        .iter()
        .map(|r| r.target)
        .chain(roots.iter().copied())
        .collect();
    let mut walk = Walk {
        pre,
        wanted: &wanted,
        roots: &roots,
        found: NodeIdHashMap::default(),
        around: Vec::new(),
    };
    walk.fragment(layout_view, None);
    let mapper = Mapper::new(pre, document, viewport);
    // How far each element's paint reaches, once, if any request asks.
    let visibility = occlusion
        .filter(|_| requests.iter().any(|r| r.visibility))
        .map(|o| (o, layout_view, VisualOverflow::of(o.artifact, pre)));
    requests
        .iter()
        .map(|request| mapper.geometry(request, &walk.found, visibility.as_ref()))
        .collect()
}

/// The walk over the fragment tree, through the child frames, that finds
/// the elements asked for.
struct Walk<'a> {
    pre: &'a PrePaint,
    wanted: &'a NodeIdHashSet,
    roots: &'a NodeIdHashSet,
    found: NodeIdHashMap<Found>,
    /// The roots asked for around the fragment being walked.
    around: Vec<NodeId>,
}

impl Walk<'_> {
    /// Where `node` was found, made on its first box: the roots around
    /// it are those around that box (not itself: a root goes on `around`
    /// only after its own box is found).
    fn found(&mut self, node: NodeId, frame: Option<NodeId>) -> &mut Found {
        let inside = &self.around;
        self.found.entry(node).or_insert_with(|| Found {
            frame,
            inside: inside.clone(),
            ..Found::default()
        })
    }

    fn fragment(&mut self, fragment: &BoxFragment, frame: Option<NodeId>) {
        let properties = self.pre.properties(fragment);
        let origin = properties.offset;
        if let Some(node) = fragment.node.filter(|n| self.wanted.contains(n)) {
            let border_box = RectF::new(
                origin.x,
                origin.y,
                fragment.size.width,
                fragment.size.height,
            );
            let as_root = self.roots.contains(&node).then(|| {
                let rect = if layout::clips_content(&fragment.style) {
                    let b = &fragment.border;
                    RectF::new(
                        border_box.x + b.left,
                        border_box.y + b.top,
                        border_box.width - b.left - b.right,
                        border_box.height - b.top - b.bottom,
                    )
                } else {
                    border_box
                };
                (rect, properties.local.transform, properties.contents.clip)
            });
            let found = self.found(node, frame);
            found.boxes.push((border_box, properties.local));
            if found.as_root.is_none() {
                found.as_root = as_root;
            }
        }
        // An inline element's boxes, on its block's lines.
        for line in &fragment.lines {
            for b in &line.boxes {
                if self.wanted.contains(&b.node) {
                    let rect = b.rect.offset(origin.x, origin.y);
                    self.found(b.node, frame)
                        .boxes
                        .push((rect, properties.contents));
                }
            }
        }
        let is_root = fragment.node.is_some_and(|n| self.roots.contains(&n));
        if is_root {
            self.around.push(fragment.node.expect("a root's box"));
        }
        if let Some(svg) = &fragment.svg {
            for node in &svg.children {
                self.svg_node(node, frame);
            }
        }
        for child in &fragment.children {
            self.fragment(&child.fragment, frame);
        }
        // A child frame's document is another: no root around it here is
        // its elements' (the spec's roots intersect only what is in their
        // document).
        if let (Some(iframe), Some(child)) = (fragment.node, self.pre.child_frame(fragment)) {
            let layout_view = child.layout_view.clone();
            let around = std::mem::take(&mut self.around);
            self.fragment(&layout_view, Some(iframe));
            self.around = around;
        }
        if is_root {
            self.around.pop();
        }
    }

    /// An SVG element in an `<svg>`'s content: a shape by its bounding box
    /// grown by half its stroke's width, if it is stroked (Blink's target
    /// rect of an SVG element is its stroke bounding box; WPT's
    /// `svg-stroke-change.html`), text by its cells', a container by what
    /// is in it.
    fn svg_node(
        &mut self,
        node: &SvgNode,
        frame: Option<NodeId>,
    ) -> Vec<(RectF, PropertyTreeState)> {
        let Some(&state) = self.pre.svg_states.get(&(node as *const SvgNode)) else {
            return Vec::new();
        };
        let boxes = match &node.kind {
            SvgNodeKind::Shape(shape) => {
                let stroked = shape.stroke_width > 0.0 && node.style.svg.stroke != SvgPaint::None;
                let half = if stroked {
                    shape.stroke_width / 2.0
                } else {
                    0.0
                };
                shape
                    .bbox
                    .map(|b| {
                        (
                            RectF::new(
                                b.x - half,
                                b.y - half,
                                b.width + 2.0 * half,
                                b.height + 2.0 * half,
                            ),
                            state,
                        )
                    })
                    .into_iter()
                    .collect()
            }
            SvgNodeKind::Text(text) => text.bounds.map(|b| (b, state)).into_iter().collect(),
            SvgNodeKind::Container(children) => children
                .iter()
                .flat_map(|child| self.svg_node(child, frame))
                .collect(),
            SvgNodeKind::Hidden(_) => Vec::new(),
        };
        if self.wanted.contains(&node.node) {
            self.found(node.node, frame)
                .boxes
                .extend(boxes.iter().copied());
        }
        boxes
    }
}

/// Maps rects between the trees' spaces and the main frame's viewport.
struct Mapper<'a> {
    pre: &'a PrePaint,
    /// The main frame's document, and its viewport's size.
    document: NodeId,
    viewport: SizeF,
    to_root: Vec<gfx::Transform>,
    /// The clips that are scroll containers' (the viewports of child
    /// frames' too): what a scroll margin grows.
    scroll_clips: HashSet<ClipNodeId>,
}

impl<'a> Mapper<'a> {
    fn new(pre: &'a PrePaint, document: NodeId, viewport: SizeF) -> Self {
        Self {
            pre,
            document,
            viewport,
            to_root: pre.trees.to_root(),
            scroll_clips: pre
                .trees
                .scroll
                .iter()
                .map(|s| s.as_ref().map_or(ClipNodeId(0), |s| s.overflow_clip))
                .collect(),
        }
    }

    /// How far the main frame's viewport is scrolled for what is in
    /// `space`: nothing for what is fixed to it.
    fn scroll(&self, space: TransformNodeId) -> PointF {
        if self.pre.trees.is_fixed(space) {
            PointF::new(0.0, 0.0)
        } else {
            self.pre.viewport_scroll
        }
    }

    /// `rect` in `space`, in the main frame's document's coordinates (the
    /// root space, where hit testing goes).
    fn to_document(&self, rect: RectF, space: TransformNodeId) -> RectF {
        let scroll = self.pre.viewport_scroll;
        self.to_viewport(rect, space).offset(scroll.x, scroll.y)
    }

    /// `rect` in `space`, in the main frame's viewport.
    fn to_viewport(&self, rect: RectF, space: TransformNodeId) -> RectF {
        let scroll = self.scroll(space);
        self.to_root[space.0]
            .map_rect(rect)
            .offset(-scroll.x, -scroll.y)
    }

    /// `rect`, in the main frame's viewport, in `space`; `None` if a
    /// transform on the way can't be undone.
    fn to_space(&self, rect: RectF, space: TransformNodeId) -> Option<RectF> {
        let scroll = self.scroll(space);
        Some(
            self.to_root[space.0]
                .inverse()?
                .map_rect(rect.offset(scroll.x, scroll.y)),
        )
    }

    /// `rect`, in the main frame's viewport, in the viewport of the
    /// document of the child frame `iframe` shows (the main frame's for
    /// `None`); `None` if it can't be mapped.
    fn in_frame(&self, rect: RectF, iframe: Option<NodeId>) -> Option<RectF> {
        match iframe {
            None => Some(rect),
            Some(iframe) => {
                let frame = self.pre.child_frames.get(&iframe)?;
                // Its viewport's space: the iframe's content box, before
                // the frame's own scroll offset.
                let origin = self.pre.trees.scroll(frame.scroll).transform;
                self.to_space(rect, origin)
            }
        }
    }

    /// `rect` (in `space`) mapped into `to`'s space; `None` if a transform
    /// on the way can't be undone.
    fn map(&self, rect: RectF, space: TransformNodeId, to: TransformNodeId) -> Option<RectF> {
        if space == to {
            return Some(rect);
        }
        let matrix = self.to_root[to.0].inverse()?.then(&self.to_root[space.0]);
        Some(matrix.map_rect(rect))
    }

    fn geometry(
        &self,
        request: &IntersectionRequest,
        found: &NodeIdHashMap<Found>,
        visibility: Option<&(&Occlusion<'_>, &BoxFragment, VisualOverflow)>,
    ) -> IntersectionGeometry {
        // The root's rect in the main frame's viewport, the clips around it
        // (none of which are between it and the target), the frame it is
        // in, and whether the target can intersect it.
        let target = found.get(&request.target).filter(|t| !t.boxes.is_empty());
        let (root, outside, root_frame, descendant) = match request.root {
            IntersectionRoot::Implicit => (
                Some(RectF::from_size(self.viewport)),
                Vec::new(),
                None,
                true,
            ),
            IntersectionRoot::Document(document) if document == self.document => (
                Some(RectF::from_size(self.viewport)),
                Vec::new(),
                None,
                target.is_some_and(|t| t.frame.is_none()),
            ),
            IntersectionRoot::Document(document) => match self.frame_of_document(document) {
                None => (None, Vec::new(), None, false),
                Some((iframe, port, origin, clip)) => (
                    Some(self.to_viewport(port, origin)),
                    self.pre.trees.clip_chain(clip),
                    Some(iframe),
                    target.is_some_and(|t| t.frame == Some(iframe)),
                ),
            },
            IntersectionRoot::Element(element) => match found
                .get(&element)
                .and_then(|r| r.as_root.map(|a| (a, r.frame)))
            {
                Some(((rect, space, clip), frame)) => (
                    Some(self.to_viewport(rect, space)),
                    self.pre.trees.clip_chain(clip),
                    frame,
                    target.is_some_and(|t| t.inside.contains(&element)),
                ),
                None => (None, Vec::new(), None, false),
            },
        };
        let root = root.map(|rect| match request.root_margin {
            Some(margin) => grow(rect, &margin),
            None => rect,
        });
        let Some(target) = target.filter(|_| descendant) else {
            return IntersectionGeometry {
                target: None,
                intersection: None,
                root: root.and_then(|r| self.in_frame(r, root_frame)),
                visible: false,
            };
        };
        let (target_rect, state) = self.target_rect(target);
        let target_in_viewport = self.to_viewport(target_rect, state.transform);
        let intersection = root.and_then(|root| {
            let clipped = self.clip(target_rect, state, &outside, &request.scroll_margin)?;
            inclusive_intersect(clipped, root)
        });
        let visible = request.visibility
            && intersection.is_some()
            && !self.distorted(target.boxes[0].1)
            && visibility.is_some_and(|(occlusion, layout_view, overflow)| {
                // Undistorted, the target's rect maps to the document's
                // coordinates as a rect.
                let rect = self.to_document(target_rect, state.transform);
                !crate::hit_test::occluded(
                    layout_view,
                    self.pre,
                    overflow,
                    rect,
                    request.target,
                    occlusion.contains,
                )
            });
        IntersectionGeometry {
            target: self.in_frame(target_in_viewport, target.frame),
            intersection: intersection.and_then(|r| self.in_frame(r, target.frame)),
            root: root.and_then(|r| self.in_frame(r, root_frame)),
            visible,
        }
    }

    /// Whether what is painted in `state` may show otherwise than as it
    /// is (Blink's `LayoutObject::HasDistortingVisualEffects`): under an
    /// opacity below 1 or a filter, its own or around it, in any frame; or
    /// mapped to the main frame by more than a translation and a scale up
    /// by the same amount on both axes. (machopo's transforms are all 2D, so
    /// Blink's check that it is in the main frame's plane always holds.)
    fn distorted(&self, state: PropertyTreeState) -> bool {
        let trees = &self.pre.trees;
        let mut effect = Some(state.effect);
        while let Some(id) = effect {
            let node = &trees.effect[id.0];
            if node.opacity != 1.0 || node.filter.is_some() {
                return true;
            }
            effect = node.parent;
        }
        let [scale_x, skew_y, skew_x, scale_y, ..] = self.to_root[state.transform.0].0;
        !(skew_x == 0.0 && skew_y == 0.0 && scale_x >= 1.0 && scale_x == scale_y)
    }

    /// The target's rect, and the state it is in: its box's, or, for one
    /// with boxes in more than one state (an inline element across blocks,
    /// an SVG container), their union in the root space under the first
    /// one's clip.
    fn target_rect(&self, target: &Found) -> (RectF, PropertyTreeState) {
        let (first, state) = target.boxes[0];
        if target.boxes.iter().all(|(_, s)| *s == state) {
            let rect = target
                .boxes
                .iter()
                .fold(first, |all, (rect, _)| all.union(rect));
            return (rect, state);
        }
        let root = TransformNodeId(0);
        let rect = target
            .boxes
            .iter()
            .fold(RectF::default(), |all, (rect, s)| {
                all.union(&self.to_root[s.transform.0].map_rect(*rect))
            });
        (
            rect,
            PropertyTreeState {
                transform: root,
                ..state
            },
        )
    }

    /// `rect` (in `state`'s transform space) cut by each clip of `state`
    /// from the innermost out, but those in `outside` (around the root),
    /// in the main frame's viewport; a scroll container's clip grown by
    /// `scroll_margin`. `None` if a clip doesn't meet it, edges touching
    /// included, or a transform can't be undone (Blink's
    /// `GeometryMapper::LocalToAncestorVisualRect` with an inclusive
    /// intersection). A rounded clip cuts as its rect, as in Blink.
    fn clip(
        &self,
        rect: RectF,
        state: PropertyTreeState,
        outside: &[ClipNodeId],
        scroll_margin: &Margins,
    ) -> Option<RectF> {
        let mut rect = rect;
        let mut space = state.transform;
        for clip in self.pre.trees.clip_chain(state.clip).into_iter().rev() {
            if outside.contains(&clip) {
                continue;
            }
            let node = &self.pre.trees.clip[clip.0];
            let Some(mut clip_rect) = node.rect else {
                continue;
            };
            if self.scroll_clips.contains(&clip) {
                clip_rect = grow(clip_rect, scroll_margin);
            }
            rect = self.map(rect, space, node.transform)?;
            space = node.transform;
            rect = inclusive_intersect(rect, clip_rect)?;
        }
        Some(self.to_viewport(rect, space))
    }

    /// The child frame showing `document`: its iframe, its viewport's rect
    /// and the space that is in, and the clip its document is under.
    fn frame_of_document(
        &self,
        document: NodeId,
    ) -> Option<(NodeId, RectF, TransformNodeId, ClipNodeId)> {
        self.pre.child_frames.iter().find_map(|(&iframe, frame)| {
            let viewport = &self.pre.trees.scroll(frame.scroll);
            (viewport.element == document).then_some((
                iframe,
                viewport.container,
                viewport.transform,
                frame.state.clip,
            ))
        })
    }
}

/// `rect` grown by `margin` (top, right, bottom, left), a percentage of
/// its height for the top and bottom, of its width for the sides.
fn grow(rect: RectF, margin: &Margins) -> RectF {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "a margin as script gave it, as Blink's float margins round it"
    )]
    let side = |m: &MarginLength, of: f32| match *m {
        MarginLength::Px(px) => px as f32,
        MarginLength::Percent(percent) => (percent / 100.0) as f32 * of,
    };
    let top = side(&margin[0], rect.height);
    let right = side(&margin[1], rect.width);
    let bottom = side(&margin[2], rect.height);
    let left = side(&margin[3], rect.width);
    RectF::new(
        rect.x - left,
        rect.y - top,
        rect.width + left + right,
        rect.height + top + bottom,
    )
}

/// Where `a` and `b` overlap, edges touching included (an empty rect there
/// then); `None` if they don't meet.
fn inclusive_intersect(a: RectF, b: RectF) -> Option<RectF> {
    let (x, y) = (a.x.max(b.x), a.y.max(b.y));
    let (right, bottom) = (a.right().min(b.right()), a.bottom().min(b.bottom()));
    (right >= x && bottom >= y).then(|| RectF::new(x, y, right - x, bottom - y))
}
