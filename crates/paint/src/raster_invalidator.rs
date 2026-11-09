//! `blink::RasterInvalidator`: what changed in a layer between two paints,
//! as the rects of its space to raster again.
//!
//! Chunks are matched first, by an id that stays the same across paints
//! (Blink's `PaintChunk::Id`): the node their first item paints and what it
//! paints, and which of the chunks starting so they are. A chunk whose
//! items paint copied, in order, from one old chunk (`PaintController`'s)
//! under the same transform, clips and opacities paints as it did, and is
//! taken whole, its items neither placed nor compared again. A chunk now
//! transformed, clipped or faded differently, one that came or went, or one
//! that now paints in another order among the others needs all its items
//! rastered again, where they were and where they are.
//!
//! In the other matched chunks, display items are matched by an id
//! (`blink::DisplayItem::Id`): the node they paint, what they paint, and
//! which of that node's items of that kind in the chunk they are. An item
//! that changed, moved, came or went needs its old and its new rect
//! rastered again; so does one that now paints in another order among the
//! others. An item paint copied unchanged needs no comparing what it
//! paints; the others are compared by it. An item gone to another chunk
//! went from one and came to the other.

use std::ops::Range;
use std::sync::Arc;

use compositor_paint::ImageFilter;
use gfx::{NodeId, Path, RRectF, RectF, Transform};
use rustc_hash::FxHashMap;

use crate::{DisplayItemKind, PaintArtifact, PendingLayer, PropertyTrees};

/// `blink::DisplayItem::Id`, and `blink::PaintChunk::Id` for a chunk's
/// first item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ItemId {
    client: Option<NodeId>,
    kind: DisplayItemKind,
    /// The client's items of this kind before it in its chunk; for a
    /// chunk, the chunks before it starting with the same.
    ordinal: u32,
}

/// How a chunk's items are drawn into the layer: the matrix into its
/// space, the clips between the layer's and the chunk's (each with its
/// matrix, its corners and its paths), and the effects around it (each
/// its opacity, its filter and the filter's space's matrix).
#[derive(Debug, Clone, PartialEq)]
struct ChunkState {
    matrix: Transform,
    clips: Vec<ChunkClip>,
    effects: Vec<(f32, Option<ImageFilter>, Transform)>,
}

impl ChunkState {
    /// How far past an item its effects' filters may draw, in the layer's
    /// space: each one's reach (Blink's `FilterOperations::MapRect`) as
    /// far as its matrix stretches it.
    fn filter_reach(&self) -> f32 {
        self.effects
            .iter()
            .filter_map(|(_, filter, matrix)| {
                let filter = filter.as_ref()?;
                let [a, b, c, d, ..] = matrix.0;
                let stretch = (a.abs() + c.abs()).max(b.abs() + d.abs());
                Some(crate::filter::reach(filter) * stretch)
            })
            .sum()
    }
}

/// A clip of a chunk's: its matrix, its corners and its paths.
type ChunkClip = (Transform, RRectF, Option<Arc<[Path]>>);

#[derive(Debug, Clone, Copy)]
struct PaintedItem {
    id: ItemId,
    /// Into the artifact's items.
    index: usize,
    /// What it may touch, in the layer's space, cut by its clips.
    rect: RectF,
}

#[derive(Debug, Clone)]
struct PaintedChunk {
    id: ItemId,
    state: ChunkState,
    /// Into the layer's items.
    items: Range<usize>,
    /// Into the artifact's items.
    artifact_items: Range<usize>,
    /// The chunk of the layer this was painted after, by its artifact,
    /// that it paints as: taken whole from it.
    taken_from: Option<usize>,
}

/// What one layer painted, kept until its next paint to compare with
/// (what Blink's `RasterInvalidator` keeps of the old paint chunks).
#[derive(Debug, Clone)]
pub struct PaintedLayer {
    artifact: Arc<PaintArtifact>,
    chunks: Vec<PaintedChunk>,
    items: Vec<PaintedItem>,
    /// The paint `taken_from` is of.
    after: Option<crate::PaintId>,
}

impl PaintedLayer {
    /// What `layer` of `artifact` paints, in its space.
    pub fn new(artifact: &Arc<PaintArtifact>, trees: &PropertyTrees, layer: &PendingLayer) -> Self {
        Self::painted(artifact, trees, layer, None)
    }

    /// What `layer` of `artifact` paints, painted after `old`, and the
    /// rects to raster again ([`Self::invalidation`]): the chunks it paints
    /// as they were are taken whole from `old`.
    pub fn after(
        artifact: &Arc<PaintArtifact>,
        trees: &PropertyTrees,
        layer: &PendingLayer,
        old: &PaintedLayer,
    ) -> (Self, Vec<RectF>) {
        let painted = Self::painted(artifact, trees, layer, Some(old));
        let invalidation = painted.invalidation(old);
        (painted, invalidation)
    }

    fn painted(
        artifact: &Arc<PaintArtifact>,
        trees: &PropertyTrees,
        layer: &PendingLayer,
        old: Option<&PaintedLayer>,
    ) -> Self {
        let space = layer.space(trees);
        let outside = trees.clip_chain(space.clip).len();
        // What `artifact` copied from `old`'s paint.
        let copied = old.and_then(|old| {
            artifact
                .cached
                .as_ref()
                .filter(|cached| cached.from == old.artifact.id)
                .map(|cached| (old, &cached.indices))
        });
        let mut chunk_ordinals: FxHashMap<(Option<NodeId>, DisplayItemKind), u32> =
            FxHashMap::default();
        let mut ordinals: FxHashMap<(Option<NodeId>, DisplayItemKind), u32> = FxHashMap::default();
        let mut chunks = Vec::new();
        let mut items = Vec::new();
        // Each space's matrix into the layer's, from the root's of only the
        // nodes the chunks are under, each once: many chunks share one.
        let mut to_root: FxHashMap<usize, Transform> = FxHashMap::default();
        let mut root_of = |transform: crate::TransformNodeId| {
            let mut chain = Vec::new();
            let mut at = Some(transform);
            let mut matrix = None;
            while let Some(node) = at {
                if let Some(&known) = to_root.get(&node.0) {
                    matrix = Some(known);
                    break;
                }
                chain.push(node);
                at = trees.transform[node.0].parent;
            }
            for node in chain.into_iter().rev() {
                let own = &trees.transform[node.0].matrix;
                let m = matrix.map_or(*own, |parent: Transform| parent.then(own));
                to_root.insert(node.0, m);
                matrix = Some(m);
            }
            matrix.expect("a node, so a matrix")
        };
        let from_root = root_of(space.transform)
            .inverse()
            .expect("a layer's space is a translation of the root's");
        let origin = Transform::translate(-space.origin.x, -space.origin.y).then(&from_root);
        let mut matrix_of = |transform: crate::TransformNodeId| origin.then(&root_of(transform));
        for chunk in &artifact.chunks[layer.chunks.clone()] {
            let matrix = matrix_of(chunk.state.transform);
            let clips: Vec<_> = trees.clip_chain(chunk.state.clip)[outside..]
                .iter()
                .map(|clip| {
                    let node = &trees.clip[clip.0];
                    let rrect = node.rrect().expect("only the root clip has no rect");
                    (matrix_of(node.transform), rrect, node.paths.clone())
                })
                .collect();
            let effects = trees
                .effect_chain(chunk.state.effect)
                .iter()
                .map(|effect| {
                    let node = &trees.effect[effect.0];
                    (node.opacity, node.filter.clone(), matrix_of(node.transform))
                })
                .collect();
            let state = ChunkState {
                matrix,
                clips,
                effects,
            };
            let first = &artifact.items[chunk.items.start];
            let ordinal = chunk_ordinals
                .entry((first.client, first.kind))
                .or_default();
            let id = ItemId {
                client: first.client,
                kind: first.kind,
                ordinal: *ordinal,
            };
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "one per chunk of a client: fewer than 2^32"
            )]
            {
                *ordinal += 1;
            }
            let start = items.len();
            // Its items copied, in order, from one old chunk, under the
            // same state: that one's, placed as they were.
            let taken_from = copied.and_then(|(old, indices)| {
                let from = indices[chunk.items.start]?;
                let at = old
                    .chunks
                    .binary_search_by_key(&from, |c| c.artifact_items.start)
                    .ok()?;
                let was = &old.chunks[at];
                let len = chunk.items.len();
                (was.artifact_items.len() == len
                    && was.state == state
                    && chunk
                        .items
                        .clone()
                        .zip(from..)
                        .all(|(index, from)| indices[index] == Some(from)))
                .then_some(at)
            });
            match taken_from {
                Some(at) => {
                    let (old, _) = copied.expect("taken from it");
                    let was = &old.chunks[at];
                    items.extend(
                        old.items[was.items.clone()]
                            .iter()
                            .zip(chunk.items.clone())
                            .map(|(item, index)| PaintedItem { index, ..*item }),
                    );
                }
                None => {
                    // Anti-aliasing reaches a pixel past what isn't drawn
                    // on whole pixels.
                    let bleed = if state.matrix.as_translation().is_some() {
                        0.0
                    } else {
                        1.0
                    };
                    let clip = state
                        .clips
                        .iter()
                        .map(|(matrix, rrect, _)| matrix.map_rect(rrect.rect))
                        .reduce(|a, b| a.intersect(&b));
                    let reach = state.filter_reach();
                    ordinals.clear();
                    for index in chunk.items.clone() {
                        let item = &artifact.items[index];
                        let ordinal = ordinals.entry((item.client, item.kind)).or_default();
                        let id = ItemId {
                            client: item.client,
                            kind: item.kind,
                            ordinal: *ordinal,
                        };
                        #[expect(
                            clippy::arithmetic_side_effects,
                            reason = "one per item of a client: fewer than 2^32"
                        )]
                        {
                            *ordinal += 1;
                        }
                        let mut rect = state.matrix.map_rect(item.visual_rect).inset(-bleed);
                        if let Some(clip) = clip {
                            rect = rect.intersect(&clip);
                        }
                        // What a filter makes of it reaches past it, and
                        // past the clips inside the filter.
                        if reach > 0.0 && !rect.is_empty() {
                            rect = rect.inset(-reach);
                        }
                        items.push(PaintedItem { id, index, rect });
                    }
                }
            }
            chunks.push(PaintedChunk {
                id,
                state,
                items: start..items.len(),
                artifact_items: chunk.items.clone(),
                taken_from,
            });
        }
        Self {
            artifact: artifact.clone(),
            chunks,
            items,
            after: copied.map(|(old, _)| old.artifact.id),
        }
    }

    /// How many of its chunks it took whole, and how many it has. For
    /// tests.
    pub fn chunks_taken_for_testing(&self) -> (usize, usize) {
        let taken = self
            .chunks
            .iter()
            .filter(|chunk| chunk.taken_from.is_some());
        (taken.count(), self.chunks.len())
    }

    /// The rects of the layer's space to raster again, this having been
    /// painted after `old`: each item that changed, moved, came or went,
    /// where it was and where it is.
    pub fn invalidation(&self, old: &PaintedLayer) -> Vec<RectF> {
        let mut rects: Vec<RectF> = Vec::new();
        let mut push = |rect: RectF| {
            if !rect.is_empty() && rects.last() != Some(&rect) {
                rects.push(rect);
            }
        };
        let taken = self.after == Some(old.artifact.id);
        let old_by_id: FxHashMap<ItemId, usize> = old
            .chunks
            .iter()
            .enumerate()
            .map(|(position, chunk)| (chunk.id, position))
            .collect();
        let mut matched = vec![false; old.chunks.len()];
        // The matched chunks, in the old order and in the new.
        let mut old_order = Vec::new();
        let mut new_order = Vec::new();
        for (position, chunk) in self.chunks.iter().enumerate() {
            let found = match chunk.taken_from.filter(|_| taken) {
                Some(at) => Some(at),
                None => old_by_id.get(&chunk.id).copied(),
            };
            let Some(old_position) = found.filter(|&at| !matched[at]) else {
                self.items[chunk.items.clone()]
                    .iter()
                    .for_each(|item| push(item.rect));
                continue;
            };
            matched[old_position] = true;
            old_order.push(old_position);
            new_order.push(position);
            let was = &old.chunks[old_position];
            if taken && chunk.taken_from == Some(old_position) {
                continue;
            }
            if chunk.state != was.state {
                old.items[was.items.clone()]
                    .iter()
                    .for_each(|item| push(item.rect));
                self.items[chunk.items.clone()]
                    .iter()
                    .for_each(|item| push(item.rect));
                continue;
            }
            self.chunk_invalidation(chunk, old, was, &mut push);
        }
        for (old_position, was) in old.chunks.iter().enumerate() {
            if !matched[old_position] {
                old.items[was.items.clone()]
                    .iter()
                    .for_each(|item| push(item.rect));
            }
        }
        // Painted in another order among the others: what overlaps comes
        // out differently.
        old_order.sort_unstable();
        for (&old_position, &position) in old_order.iter().zip(&new_order) {
            let (was, chunk) = (&old.chunks[old_position], &self.chunks[position]);
            if was.id != chunk.id {
                old.items[was.items.clone()]
                    .iter()
                    .for_each(|item| push(item.rect));
                self.items[chunk.items.clone()]
                    .iter()
                    .for_each(|item| push(item.rect));
            }
        }
        rects
    }

    /// What changed between the items of two matched chunks under the same
    /// state, `chunk` of this and `was` of `old`.
    fn chunk_invalidation(
        &self,
        chunk: &PaintedChunk,
        old: &PaintedLayer,
        was: &PaintedChunk,
        push: &mut impl FnMut(RectF),
    ) {
        let (items, old_items) = (
            &self.items[chunk.items.clone()],
            &old.items[was.items.clone()],
        );
        let old_by_id: FxHashMap<ItemId, usize> = old_items
            .iter()
            .enumerate()
            .map(|(position, item)| (item.id, position))
            .collect();
        let mut matched = vec![false; old_items.len()];
        let mut old_order = Vec::new();
        let mut new_order = Vec::new();
        for (position, item) in items.iter().enumerate() {
            let Some(&old_position) = old_by_id.get(&item.id) else {
                push(item.rect);
                continue;
            };
            matched[old_position] = true;
            old_order.push(old_position);
            new_order.push(position);
            let was = &old_items[old_position];
            // Its rect follows from the two, so it needs no comparing.
            let same = self
                .artifact
                .is_cached_from(item.index, &old.artifact, was.index)
                || self.artifact.items[item.index] == old.artifact.items[was.index];
            if !same {
                push(was.rect);
                push(item.rect);
            }
        }
        for (old_position, was) in old_items.iter().enumerate() {
            if !matched[old_position] {
                push(was.rect);
            }
        }
        old_order.sort_unstable();
        for (&old_position, &position) in old_order.iter().zip(&new_order) {
            let (was, item) = (&old_items[old_position], &items[position]);
            if was.id != item.id {
                push(was.rect);
                push(item.rect);
            }
        }
    }
}
