//! `PaintArtifactCompositor`: which paint chunks go into which compositor
//! layer.
//!
//! Only scroll containers and canvases become layers of their own: a
//! container, so the compositor moves its content by the scroll offset
//! without painting or rastering it again; a canvas (Blink's foreign layer
//! of a composited canvas), so a draw on it changes only its texture. Each
//! one only where its transforms up to the root only move things, under no
//! opacity (the compositor and Viz only translate layers) and at most one
//! rounded clip; a container also has to have something to scroll.
//! Transforms and opacity stay painted into their layer: without a reason
//! such as an animation, Blink doesn't composite them either.
//!
//! The chunks go into layers in paint order, a new layer each time a chunk
//! is under a different composited container than the one before, and
//! around a canvas's chunk: what is painted before a container (or canvas),
//! its content, and what is painted after it.
//! Later content that overlaps the container is thus above it; no overlap
//! testing merges layers back (Blink's `PendingLayer` merging).
//!
//! A layer is as big as what it has: a container's, what scrolls in it;
//! the document's own (the first), the whole document, as its background
//! is the page's; the others, the union of their chunks, out to whole
//! tiles (Blink's `PendingLayer` bounds are the exact union).

use std::ops::Range;

use compositor_paint::{DisplayItemList, PaintOp};
use gfx::{NodeId, RectF, SizeF, Transform};

/// The grid, in device pixels, that the bounds of a layer of what is
/// painted after a container snap out to: the compositor's tile size
/// (`LayerTreeSettings::default_tile_size`). Content that grows or shrinks
/// in it without crossing a line keeps the layer's origin, and so its
/// tiles.
pub const LAYER_GRID: f32 = 256.0;

use crate::{
    ClipNodeId, DisplayItemKind, EffectNodeId, LayerSpace, PaintArtifact, PropertyTreeState,
    PropertyTrees, ScrollNodeId, StickyScroll, TransformNodeId, chunks_to_display_list,
};

/// A run of consecutive paint chunks for one compositor layer
/// (`blink::PendingLayer`).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingLayer {
    /// The composited scroll container whose content this is; `None` for
    /// the document's own.
    pub scroller: Option<ScrollNodeId>,
    /// The sticky box whose content this is, if the compositor moves it
    /// ([`composited_sticky`]) and it is in `scroller`: the layer is in
    /// its translation's space, which the compositor moves as the box
    /// sticks.
    pub sticky: Option<TransformNodeId>,
    /// Whether its content is fixed to the main frame's viewport (RFC
    /// 0013's decision 16): the compositor keeps it where it is drawn
    /// when it scrolls the viewport, so it is a layer of its own.
    pub fixed: bool,
    /// Indices into the artifact's chunks.
    pub chunks: Range<usize>,
    /// Where the layer is in its space (see [`layerize`]), snapped to device
    /// pixels like what is painted in it, so its content lands on the
    /// pixel grid in the layer as in the document.
    pub bounds: RectF,
    /// A canvas's layer: its one chunk is the canvas's item, which the
    /// compositor draws from the canvas's bitmap (a texture layer), not
    /// from the item.
    pub canvas: Option<CanvasLayer>,
}

/// What a canvas's layer is of: the element, and where its item is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasLayer {
    pub element: NodeId,
    /// Its chunk's: the layer's space and clip are its own.
    pub state: PropertyTreeState,
}

/// Whether content under `transform`, `effect` and `clip` can be a layer of
/// its own: only translations from its space up to the root, no opacity on
/// it or around it, at most one rounded clip (Viz cuts a layer's quads to
/// one, Chromium's fast rounded corners; more would need a render surface
/// with a mask, which machopo doesn't have) and no path clip (Blink makes a
/// mask layer for one, `SynthesizedClip`).
pub(crate) fn can_be_layer(
    trees: &PropertyTrees,
    transform: TransformNodeId,
    effect: EffectNodeId,
    clip: ClipNodeId,
) -> bool {
    let mut node = Some(transform);
    while let Some(t) = node {
        let t = &trees.transform[t.0];
        if t.matrix.as_translation().is_none() {
            return false;
        }
        node = t.parent;
    }
    let clips = trees.clip_chain(clip);
    let rounded = clips
        .iter()
        .filter(|clip| trees.clip[clip.0].is_rounded())
        .count();
    // The compositor's clips are rounded rects: what a path clips is
    // painted into the layer it is in, its clip with it.
    let paths = clips.iter().any(|clip| trees.clip[clip.0].paths.is_some());
    trees.effect_chain(effect).is_empty() && rounded <= 1 && !paths
}

/// For each scroll node, whether its container becomes a layer: something
/// to scroll, and it can be one ([`can_be_layer`]; the rounded clip on its
/// content: its own `border-radius` or one around it).
pub fn composited_scrollers(trees: &PropertyTrees) -> Vec<bool> {
    trees
        .scroll
        .iter()
        .map(|node| {
            node.as_ref().is_some_and(|node| {
                let overflows = node.contents.width > node.container.width
                    || node.contents.height > node.container.height;
                overflows && can_be_layer(trees, node.transform, node.effect, node.overflow_clip)
            })
        })
        .collect()
}

/// The canvas whose layer `chunk` is, if it is a canvas's chunk that can be
/// a layer: its bitmap no wider or taller than `max_texture_size`. One
/// bigger is drawn in its chunk's layer's tiles, as Blink draws a canvas it
/// has no shared image for (`Canvas2DResourceProvider` past
/// `max_texture_size`): decided here, as the chunk goes into a layer.
fn canvas_layer(
    artifact: &PaintArtifact,
    trees: &PropertyTrees,
    chunk: usize,
    max_texture_size: u32,
) -> Option<CanvasLayer> {
    let chunk = &artifact.chunks[chunk];
    let item = &artifact.items[chunk.items.start];
    let state = chunk.state;
    let fits = match &item.op {
        PaintOp::DrawImage { image, .. } => image
            .decoded
            .is_some_and(|size| size.width <= max_texture_size && size.height <= max_texture_size),
        _ => false,
    };
    (item.kind == DisplayItemKind::Canvas
        && fits
        && can_be_layer(trees, state.transform, state.effect, state.clip))
    .then_some(CanvasLayer {
        element: item.client?,
        state,
    })
}

/// For each transform node, whether it is a sticky box's translation that
/// the compositor moves as the box's scrollport scrolls (Blink composites
/// a sticky translation with a composited scroll ancestor): its scrollport
/// is the main frame's viewport or a container that is a layer
/// (`composited`), and what is in it can be a layer of its own
/// ([`can_be_layer`]). Any other is painted where it sticks and painted
/// again as it moves. A sticky box in another, in the same scrollport, is
/// under all the other is under: the compositor moves it only if it moves
/// the other.
pub fn composited_sticky(trees: &PropertyTrees, composited: &[bool]) -> Vec<bool> {
    trees
        .transform
        .iter()
        .enumerate()
        .map(|(index, node)| {
            node.sticky.is_some_and(|sticky| {
                let scrolls = match sticky.scroll {
                    StickyScroll::Viewport => true,
                    StickyScroll::Scroller(scroller) => composited[scroller.0],
                    StickyScroll::Fixed => false,
                };
                scrolls && can_be_layer(trees, TransformNodeId(index), sticky.effect, sticky.clip)
            })
        })
        .collect()
}

/// For each transform node, the innermost container that is a layer
/// (`composited`) whose content it is in: what moves it when the
/// compositor scrolls.
pub(crate) fn composited_scroller_of(
    trees: &PropertyTrees,
    composited: &[bool],
) -> Vec<Option<ScrollNodeId>> {
    // The container whose scroll translation each node is, of those that
    // are layers.
    let mut own: Vec<Option<ScrollNodeId>> = vec![None; trees.transform.len()];
    for (id, node) in trees.scroll_nodes() {
        if composited[id.0] {
            own[node.scroll_translation.0] = Some(id);
        }
    }
    let mut scroller_of: Vec<Option<ScrollNodeId>> = Vec::with_capacity(trees.transform.len());
    for (index, node) in trees.transform.iter().enumerate() {
        let scroller = own[index].or_else(|| node.parent.and_then(|p| scroller_of[p.0]));
        scroller_of.push(scroller);
    }
    scroller_of
}

/// For each transform node, the innermost sticky box's translation that
/// the compositor moves ([`composited_sticky`]) whose content it is in,
/// inside the innermost composited container around it (`scroller_of`):
/// a container inside the box moves what is in it instead.
pub(crate) fn composited_sticky_of(
    trees: &PropertyTrees,
    composited: &[bool],
    scroller_of: &[Option<ScrollNodeId>],
) -> Vec<Option<TransformNodeId>> {
    let sticky = composited_sticky(trees, composited);
    let mut out: Vec<Option<TransformNodeId>> = Vec::with_capacity(trees.transform.len());
    for (index, node) in trees.transform.iter().enumerate() {
        let inherited = node.parent.and_then(|p| out[p.0]).filter(|&s| {
            // Not past a composited container's scroll translation: the
            // sticky box is around the container, which moves it.
            scroller_of[index] == scroller_of[s.0]
        });
        out.push(if sticky[index] {
            Some(TransformNodeId(index))
        } else {
            inherited
        });
    }
    out
}

/// The artifact's chunks as layers, in paint order. The first is always
/// the document's own, empty if nothing is painted outside the containers:
/// the opaque layer everything else is over, as big as the `document`. A
/// container's is its padding box, as big as what scrolls in it; another
/// is the union of what its chunks paint, in the document and out to
/// [`LAYER_GRID`]. A canvas is a layer of its own if its bitmap fits a
/// texture of `max_texture_size` ([`canvas_layer`]).
pub fn layerize(
    artifact: &PaintArtifact,
    trees: &PropertyTrees,
    document: SizeF,
    max_texture_size: u32,
) -> Vec<PendingLayer> {
    let composited = composited_scrollers(trees);
    let scroller_of = composited_scroller_of(trees, &composited);
    let sticky_of = composited_sticky_of(trees, &composited, &scroller_of);
    let mut layers = vec![PendingLayer {
        scroller: None,
        sticky: None,
        fixed: false,
        chunks: 0..0,
        bounds: RectF::default(),
        canvas: None,
    }];
    for (index, chunk) in artifact.chunks.iter().enumerate() {
        let scroller = scroller_of[chunk.state.transform.0];
        let sticky = sticky_of[chunk.state.transform.0];
        let fixed = trees.is_fixed(chunk.state.transform);
        let canvas = canvas_layer(artifact, trees, index, max_texture_size);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "index < artifact.chunks.len()"
        )]
        let next = index + 1;
        match layers.last_mut() {
            Some(last)
                if last.scroller == scroller
                    && last.sticky == sticky
                    && last.fixed == fixed
                    && last.canvas.is_none()
                    && canvas.is_none() =>
            {
                if last.chunks.is_empty() {
                    last.chunks.start = index;
                }
                last.chunks.end = next;
            }
            _ => layers.push(PendingLayer {
                scroller,
                sticky,
                fixed,
                chunks: index..next,
                bounds: RectF::default(),
                canvas,
            }),
        }
    }
    let dsf = artifact.device_scale_factor;
    let root = TransformNodeId(0);
    for (index, layer) in layers.iter_mut().enumerate() {
        // A canvas's: its item's rect, in its own space (snapped already).
        if layer.canvas.is_some() {
            layer.bounds =
                artifact.items[artifact.chunks[layer.chunks.start].items.start].visual_rect;
            continue;
        }
        // A sticky box's: as big as what it paints, in its space, where
        // the compositor moves it.
        if let Some(sticky) = layer.sticky {
            let around = layer.clip(trees);
            let painted = painted_rect(artifact, trees, layer.chunks.clone(), sticky, around);
            layer.bounds = crate::snap(snap_out(painted, LAYER_GRID / dsf), dsf);
            continue;
        }
        let bounds = match layer.scroller {
            Some(s) => {
                let node = &trees.scroll(s);
                RectF::new(
                    node.container.x,
                    node.container.y,
                    node.contents.width.max(node.container.width),
                    node.contents.height.max(node.container.height),
                )
            }
            // Fixed content: as big as what it paints, wherever that is
            // in the viewport (the compositor keeps it there).
            None if layer.fixed => {
                let painted =
                    painted_rect(artifact, trees, layer.chunks.clone(), root, ClipNodeId(0));
                snap_out(painted, LAYER_GRID / dsf)
            }
            None if index == 0 => RectF::new(0.0, 0.0, document.width, document.height),
            None => {
                let painted =
                    painted_rect(artifact, trees, layer.chunks.clone(), root, ClipNodeId(0))
                        .intersect(&RectF::new(0.0, 0.0, document.width, document.height));
                snap_out(painted, LAYER_GRID / dsf)
            }
        };
        layer.bounds = crate::snap(bounds, dsf);
    }
    layers
}

/// What `chunks` (of a layer in `space`, only translated from the root's)
/// paint, there: each item's visual rect through its chunk's transform,
/// cut by its clips inside `around` (the layer's, which the compositor
/// applies where the layer is), a pixel more where anti-aliasing reaches
/// past it.
fn painted_rect(
    artifact: &PaintArtifact,
    trees: &PropertyTrees,
    chunks: Range<usize>,
    space: TransformNodeId,
    around: ClipNodeId,
) -> RectF {
    let outside = trees.clip_chain(around);
    let to_space = trees.to_root();
    let origin = to_space[space.0]
        .as_translation()
        .expect("a layer's space is only translated");
    let to_root: Vec<Transform> = to_space
        .iter()
        .map(|m| Transform::translate(-origin.x, -origin.y).then(m))
        .collect();
    let mut painted: Option<RectF> = None;
    for chunk in &artifact.chunks[chunks] {
        let matrix = &to_root[chunk.state.transform.0];
        let bleed = if matrix.as_translation().is_some() {
            0.0
        } else {
            1.0
        };
        let clip = trees
            .clip_chain(chunk.state.clip)
            .iter()
            .filter(|clip| !outside.contains(clip))
            .filter_map(|clip| {
                let node = &trees.clip[clip.0];
                Some(to_root[node.transform.0].map_rect(node.rect?))
            })
            .reduce(|a, b| a.intersect(&b));
        for item in &artifact.items[chunk.items.clone()] {
            let mut rect = matrix.map_rect(item.visual_rect).inset(-bleed);
            if let Some(clip) = clip {
                rect = rect.intersect(&clip);
            }
            if rect.is_empty() {
                continue;
            }
            painted = Some(match painted {
                Some(p) => union(p, rect),
                None => rect,
            });
        }
    }
    painted.unwrap_or_default()
}

fn union(a: RectF, b: RectF) -> RectF {
    let (x0, y0) = (a.x.min(b.x), a.y.min(b.y));
    let (x1, y1) = (a.right().max(b.right()), a.bottom().max(b.bottom()));
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

/// `rect` grown out to multiples of `grid`. Nothing painted is an empty
/// rect at the origin, which stays one.
fn snap_out(rect: RectF, grid: f32) -> RectF {
    let (x0, y0) = (
        (rect.x / grid).floor() * grid,
        (rect.y / grid).floor() * grid,
    );
    let (x1, y1) = (
        (rect.right() / grid).ceil() * grid,
        (rect.bottom() / grid).ceil() * grid,
    );
    RectF::new(x0, y0, x1 - x0, y1 - y0)
}

impl PendingLayer {
    /// The space the layer's content is in: a canvas's own, its sticky
    /// box's translation, its container's scroll translation, or the
    /// root's.
    pub fn transform(&self, trees: &PropertyTrees) -> TransformNodeId {
        if let Some(canvas) = self.canvas {
            return canvas.state.transform;
        }
        if let Some(sticky) = self.sticky {
            return sticky;
        }
        self.scroller
            .map_or(TransformNodeId(0), |s| trees.scroll(s).scroll_translation)
    }

    /// The clip the compositor applies to the whole layer: a canvas's own
    /// (and so all around it), the one its sticky box is under, its
    /// container's overflow clip, or none.
    pub fn clip(&self, trees: &PropertyTrees) -> ClipNodeId {
        if let Some(canvas) = self.canvas {
            return canvas.state.clip;
        }
        if let Some(sticky) = self.sticky {
            return trees.transform[sticky.0]
                .sticky
                .expect("a sticky box's translation")
                .clip;
        }
        self.scroller
            .map_or(ClipNodeId(0), |s| trees.scroll(s).overflow_clip)
    }

    /// The layer's chunks as a display list in its own space, its origin at
    /// the top left of its bounds.
    pub fn display_list(&self, artifact: &PaintArtifact, trees: &PropertyTrees) -> DisplayItemList {
        chunks_to_display_list(
            artifact,
            trees,
            &artifact.chunks[self.chunks.clone()],
            self.space(trees),
            DisplayItemList::new(),
        )
    }

    /// The layer's space: its transform node, the clip around it, and its
    /// origin there.
    pub(crate) fn space(&self, trees: &PropertyTrees) -> LayerSpace {
        LayerSpace {
            transform: self.transform(trees),
            clip: self.clip(trees),
            origin: gfx::PointF::new(self.bounds.x, self.bounds.y),
        }
    }
}
