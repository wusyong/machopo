//! Paint's output into the compositor's layers (the end of Blink's
//! `PaintArtifactCompositor`, and its `PropertyTreeManager`): each pending
//! layer becomes a `PictureLayer`, or a canvas's a `TextureLayer` of its
//! bitmap, and the paint property tree nodes the layers are under become
//! the compositor's. A layer keeps its id from one
//! paint to the next, with what it painted, so the compositor rasters again
//! only what changed (Blink's `RasterInvalidator`).

use std::collections::HashMap;
use std::sync::Arc;

use compositor::{
    ClipNode, ClipNodeId, ElementId, Layer, LayerId, LayerProperties, PictureLayer, PropertyTrees,
    ScrollNode, ScrollNodeId, ScrollTreeNode, StickyPositionConstraint, StickyRegions,
    TextureLayer, TransformNode, TransformNodeId,
};
use compositor_paint::DisplayItemList;
use gfx::{Color, SizeF};
use layout::{StickyConstraints, StickyKey};
use paint::{PaintArtifact, PaintedLayer};

/// Which layer is which from one paint to the next: what moves it (its
/// container and sticky box, or none for the document's own) and how many
/// layers with the same came before it.
type LayerKey = (Option<gfx::NodeId>, Option<StickyKey>, usize);

/// The sticky boxes of a frame tree as layout has them, for the
/// compositor's nodes of those it moves.
#[derive(Default)]
pub(crate) struct StickyBoxes {
    /// Each box's constraints, and the box moving its containing block.
    pub(crate) constraints: HashMap<StickyKey, (StickyConstraints, Option<StickyKey>)>,
    /// How far each sticks with the main thread's offsets, and those
    /// offsets as the compositor keeps them (the main frame's viewport's
    /// as `ElementId::VIEWPORT`): a debug build checks the compositor's
    /// math against layout's with them.
    pub(crate) offsets: layout::StickyOffsets,
    pub(crate) scroll: compositor::ScrollOffsets,
}

/// `blink::PaintArtifactCompositor`, across paints of a document: each
/// layer's id, what it last painted, and the display list made of it.
pub(crate) struct PaintArtifactCompositor {
    layers: HashMap<LayerKey, (LayerId, PaintedLayer, Arc<DisplayItemList>)>,
    /// The canvases that are layers, by element: each keeps its layer's id
    /// while it is one.
    canvases: HashMap<gfx::NodeId, LayerId>,
    /// The biggest canvas that is a layer of its own: the texture Viz
    /// takes (smaller in tests).
    max_texture_size: u32,
}

impl Default for PaintArtifactCompositor {
    fn default() -> Self {
        Self {
            layers: HashMap::new(),
            canvases: HashMap::new(),
            max_texture_size: protocol::viz::MAX_TEXTURE_SIZE,
        }
    }
}

/// A canvas's bitmap as a layer shows it: its snapshot, and whether it is
/// opaque (`alpha: false`); `None` if it shows nothing.
pub(crate) type CanvasSnapshot<'a> = &'a dyn Fn(gfx::NodeId) -> Option<(gfx::ImageData, bool)>;

impl PaintArtifactCompositor {
    /// The layers of `artifact` (painted with the composited containers
    /// unscrolled), in paint order, and the trees they are in, with the
    /// containers that are layers in the scroll tree, the main thread's
    /// scroll hit test `region` and the frames' `attribution` for input.
    /// The first layer is the document's own, opaque. A layer that was
    /// there last time keeps its id, and says what to raster again; a new
    /// one is rastered whole. A canvas's layer
    /// shows its bitmap as `canvases` has it now (not its display item's,
    /// which may be one kept from an earlier paint); one that shows
    /// nothing is left out.
    #[expect(
        clippy::too_many_arguments,
        reason = "each is a part of what paint made that the compositor's trees take"
    )]
    pub(crate) fn update(
        &mut self,
        artifact: &Arc<PaintArtifact>,
        trees: &paint::PropertyTrees,
        document: SizeF,
        region: &paint::ScrollHitTestRegion,
        attribution: &paint::FrameAttributionRegion,
        sticky: &StickyBoxes,
        canvases: CanvasSnapshot<'_>,
    ) -> (Vec<Layer>, PropertyTrees) {
        let pending = paint::layerize(artifact, trees, document, self.max_texture_size);
        let mut manager = PropertyTreeManager::new(trees, sticky, artifact.device_scale_factor);
        manager.scroll_tree(region);
        manager.frame_attribution(attribution);
        let mut seen: HashMap<(Option<gfx::NodeId>, Option<StickyKey>), usize> = HashMap::new();
        let mut last = std::mem::take(&mut self.layers);
        let mut last_canvases = std::mem::take(&mut self.canvases);
        let layers = pending
            .iter()
            .enumerate()
            .filter_map(|(index, layer)| {
                let bounds = layer.bounds;
                let properties = LayerProperties {
                    transform: manager.transform(layer.transform(trees)),
                    clip: manager.clip(layer.clip(trees)),
                    origin: gfx::PointF::new(bounds.x, bounds.y),
                    fixed: layer.fixed,
                    over_surfaces: false,
                };
                if let Some(canvas) = layer.canvas {
                    let (snapshot, opaque) = canvases(canvas.element)?;
                    let id = last_canvases
                        .remove(&canvas.element)
                        .unwrap_or_else(LayerId::next);
                    self.canvases.insert(canvas.element, id);
                    return Some(Layer::Texture(TextureLayer {
                        id,
                        bounds: SizeF::new(bounds.width, bounds.height),
                        snapshot,
                        contents_opaque: opaque,
                        properties,
                    }));
                }
                let scroller = layer.scroller.map(|s| trees.scroll(s).element);
                let stuck = layer
                    .sticky
                    .and_then(|s| trees.transform[s.0].sticky)
                    .map(|s| s.key);
                let ordinal = seen.entry((scroller, stuck)).or_default();
                let key = (scroller, stuck, *ordinal);
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "one per layer of a container: fewer than usize::MAX"
                )]
                {
                    *ordinal += 1;
                }
                let (painted, id, invalidation, display_list) = match last.remove(&key) {
                    Some((id, old, display_list)) => {
                        let (painted, invalidation) =
                            PaintedLayer::after(artifact, trees, layer, &old);
                        // Nothing to raster again: it paints what it did,
                        // the same way.
                        let kept = invalidation.is_empty().then_some(display_list);
                        (painted, id, Some(invalidation.into()), kept)
                    }
                    None => (
                        PaintedLayer::new(artifact, trees, layer),
                        LayerId::next(),
                        None,
                        None,
                    ),
                };
                let display_list =
                    display_list.unwrap_or_else(|| Arc::new(layer.display_list(artifact, trees)));
                self.layers.insert(key, (id, painted, display_list.clone()));
                let first = index == 0;
                Some(Layer::Picture(PictureLayer {
                    id,
                    invalidation,
                    bounds: SizeF::new(bounds.width, bounds.height),
                    display_list,
                    // Only the bottom layer has the document's background;
                    // the others show what is under them.
                    contents_opaque: first,
                    background_color: Color::WHITE,
                    properties,
                }))
            })
            .collect();
        #[cfg(debug_assertions)]
        manager.check_sticky_offsets();
        (layers, manager.out)
    }

    /// The layer the canvas `element` is, if the last update made it one.
    pub(crate) fn canvas_layer(&self, element: gfx::NodeId) -> Option<LayerId> {
        self.canvases.get(&element).copied()
    }
}

/// The element a composited container's scroll offset is kept under.
pub(crate) fn element_id(element: gfx::NodeId) -> ElementId {
    ElementId(u64::from(element.get()))
}

/// A scroll container from paint's scroll tree, as the compositor's math
/// takes it (`PropertyTreeManager` building cc's scroll node from Blink's
/// `ScrollPaintPropertyNode`).
pub(crate) fn scroll_node(node: &paint::ScrollNode) -> ScrollNode {
    ScrollNode {
        container_bounds: node.container.size(),
        bounds: node.contents,
        user_scrollable_horizontal: node.user_scrollable_x,
        user_scrollable_vertical: node.user_scrollable_y,
    }
}

/// `PropertyTreeManager`: paint's nodes into the compositor's, only those
/// the layers and the scroll tree are under, each once.
struct PropertyTreeManager<'a> {
    paint: &'a paint::PropertyTrees,
    /// By paint's scroll node.
    composited: Vec<bool>,
    out: PropertyTrees,
    transforms: HashMap<paint::TransformNodeId, TransformNodeId>,
    clips: HashMap<paint::ClipNodeId, ClipNodeId>,
    /// The scroll translations of the containers that are layers, and
    /// their elements.
    scrolls: HashMap<paint::TransformNodeId, ElementId>,
    /// By paint's transform node, whether it is a sticky box's
    /// translation the compositor moves; and each sticky box's, by its
    /// key.
    composited_sticky: Vec<bool>,
    sticky_nodes: HashMap<StickyKey, paint::TransformNodeId>,
    sticky: &'a StickyBoxes,
    device_scale_factor: f32,
    /// The scroll tree's node of each container that is a layer, by
    /// paint's.
    scroll_ids: HashMap<paint::ScrollNodeId, ScrollNodeId>,
}

impl<'a> PropertyTreeManager<'a> {
    fn new(
        paint: &'a paint::PropertyTrees,
        sticky: &'a StickyBoxes,
        device_scale_factor: f32,
    ) -> Self {
        let composited = paint::composited_scrollers(paint);
        let scrolls = paint
            .scroll_nodes()
            .filter(|(id, _)| composited[id.0])
            .map(|(_, node)| (node.scroll_translation, element_id(node.element)))
            .collect();
        let composited_sticky = paint::composited_sticky(paint, &composited);
        let sticky_nodes = paint
            .transform
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((node.sticky?.key, paint::TransformNodeId(index))))
            .collect();
        Self {
            paint,
            composited,
            out: PropertyTrees::default(),
            transforms: [(paint::TransformNodeId(0), TransformNodeId(0))].into(),
            clips: [(paint::ClipNodeId(0), ClipNodeId(0))].into(),
            scrolls,
            composited_sticky,
            sticky_nodes,
            sticky,
            device_scale_factor,
            scroll_ids: HashMap::new(),
        }
    }

    /// What moves the sticky box of paint's translation `id` (Blink's
    /// `CompositorStickyConstraint`): layout's constraints, its scroller's
    /// element, and the compositor's node of the box moving its containing
    /// block; `None` unless the compositor moves it.
    fn sticky_constraint(
        &mut self,
        id: paint::TransformNodeId,
    ) -> Option<StickyPositionConstraint> {
        if !self.composited_sticky[id.0] {
            return None;
        }
        let node = self.paint.transform[id.0].sticky?;
        let (c, around) = self.sticky.constraints.get(&node.key)?;
        let scroller = match node.scroll {
            paint::StickyScroll::Viewport => ElementId::VIEWPORT,
            paint::StickyScroll::Scroller(s) => element_id(self.paint.scroll(s).element),
            paint::StickyScroll::Fixed => return None,
        };
        // Around it in the same scrollport: the compositor moves it too.
        let around = around
            .and_then(|key| self.sticky_nodes.get(&key).copied())
            .filter(|&n| self.composited_sticky[n.0])
            .map(|n| self.transform(n));
        Some(StickyPositionConstraint {
            scroller,
            constraining_rect: c.constraining_rect,
            sticky_box: c.sticky_box,
            containing_block: c.containing_block,
            inset: c.inset,
            shifting_containing_block: around,
            pixel_snap_offset: node.at,
            device_scale_factor: self.device_scale_factor,
        })
    }

    /// Checks that the compositor puts each sticky box it moves where
    /// layout does, with the main thread's offsets.
    #[cfg(debug_assertions)]
    fn check_sticky_offsets(&self) {
        for (&key, &node) in &self.sticky_nodes {
            let Some(&out) = self.transforms.get(&node) else {
                continue;
            };
            if self.out.transform[out.0].sticky.is_none() {
                continue;
            }
            let theirs = self.out.sticky_offset(out, &self.sticky.scroll);
            let ours = self.sticky.offsets.get(&key).copied().unwrap_or_default();
            assert!(
                (theirs.x - ours.x).abs() < 1e-3 && (theirs.y - ours.y).abs() < 1e-3,
                "the compositor moves {key:?} by {theirs:?}, layout by {ours:?}"
            );
        }
    }

    /// A scroll node for each container that is a layer, its parent the
    /// nearest such around it, with the region: what is in it in its node,
    /// the rest in the trees'.
    fn scroll_tree(&mut self, region: &paint::ScrollHitTestRegion) {
        let mut ids: HashMap<paint::ScrollNodeId, ScrollNodeId> = HashMap::new();
        // Each after its parent.
        for (id, node) in self.paint.scroll_nodes() {
            let index = id.0;
            if !self.composited[index] {
                continue;
            }
            let mut parent = node.parent;
            while let Some(p) = parent.filter(|p| !self.composited[p.0]) {
                parent = self.paint.scroll(p).parent;
            }
            let scroll_node = ScrollTreeNode {
                parent: parent.map(|p| ids[&p]),
                element: element_id(node.element),
                scroller: scroll_node(node),
                transform: self.transform(node.transform),
                border_box: node.border_box,
                clip: self.clip(node.clip),
                scroll_translation: self.transform(node.scroll_translation),
                main_thread_scroll_hit_test_region: region.scrollers[index].as_slice().into(),
                wheel_event_handler_region: Arc::from([]),
            };
            ids.insert(
                paint::ScrollNodeId(index),
                ScrollNodeId(self.out.scroll.len()),
            );
            self.out.scroll.push(scroll_node);
        }
        self.scroll_ids = ids;
        self.out.main_thread_scroll_hit_test_region = region.root.as_slice().into();
        // One for each sticky box the compositor moves, in order.
        let sticky: Vec<paint::TransformNodeId> = composited_sticky_nodes(&self.composited_sticky);
        self.out.sticky_regions = sticky
            .into_iter()
            .map(|node| StickyRegions {
                transform: self.transform(node),
                main_thread_scroll_hit_test_region: sticky_region(region, node).into(),
                wheel_event_handler_region: Arc::from([]),
            })
            .collect();
    }

    /// Which frame input at a point is for, `region` in the compositor's
    /// spaces: a container's that is a layer by its scroll tree node, a
    /// sticky box's by its translation node.
    fn frame_attribution(&mut self, region: &paint::FrameAttributionRegion) {
        let frames = region
            .frames
            .iter()
            .map(|frame| compositor::AttributedFrame {
                document: element_id(frame.document),
                parent: frame.parent,
            })
            .collect();
        let rects = region
            .rects
            .iter()
            .map(|rect| compositor::AttributionRect {
                rect: rect.rect,
                space: match rect.space {
                    paint::AttributionSpace::Document => compositor::AttributionSpace::Document,
                    paint::AttributionSpace::Scroller(scroller) => {
                        compositor::AttributionSpace::Scroller(self.scroll_ids[&scroller])
                    }
                    paint::AttributionSpace::Sticky(node) => {
                        compositor::AttributionSpace::Sticky(self.transform(node))
                    }
                },
                order: rect.order,
                frame: rect.frame,
                masked: rect.masked,
            })
            .collect();
        self.out.frame_attribution = compositor::FrameAttributionRegion { frames, rects };
    }

    fn transform(&mut self, id: paint::TransformNodeId) -> TransformNodeId {
        if let Some(&out) = self.transforms.get(&id) {
            return out;
        }
        let node = &self.paint.transform[id.0];
        let parent = self.transform(node.parent.expect("only the root has no parent"));
        // A layer's nodes, and a composited container's, only move things:
        // only such containers become layers. A composited container's scroll translation was painted
        // unscrolled; the compositor moves it by the element's offset. So
        // is a sticky box's translation the compositor moves as it sticks.
        let offset = node
            .matrix
            .as_translation()
            .expect("a layer is under translations only");
        let sticky = self.sticky_constraint(id);
        let out = TransformNodeId(self.out.transform.len());
        self.out.transform.push(TransformNode {
            parent: Some(parent),
            offset,
            scroll: self.scrolls.get(&id).copied(),
            sticky,
        });
        self.transforms.insert(id, out);
        out
    }

    fn clip(&mut self, id: paint::ClipNodeId) -> ClipNodeId {
        if let Some(&out) = self.clips.get(&id) {
            return out;
        }
        let node = &self.paint.clip[id.0];
        debug_assert!(node.paths.is_none(), "nothing under a path clip is a layer");
        let parent = self.clip(node.parent.expect("only the root has no parent"));
        let transform = self.transform(node.transform);
        let out = ClipNodeId(self.out.clip.len());
        self.out.clip.push(ClipNode {
            parent: Some(parent),
            transform,
            rect: node.rect,
            radii: node.radii,
        });
        self.clips.insert(id, out);
        out
    }
}

/// Puts the wheel event handler region (`paint::wheel_event_handler_region`
/// of the same pre-paint, `composited` the containers that are layers) in
/// `trees`, made by `update` from it, and whether the page may have `wheel`
/// listeners: with the layers, or alone when only the listeners changed.
pub(crate) fn set_wheel_event_handler_region(
    trees: &mut PropertyTrees,
    composited: &[bool],
    composited_sticky: &[bool],
    region: &paint::ScrollHitTestRegion,
    listeners: bool,
) {
    trees.wheel_event_handler_region = region.root.as_slice().into();
    trees.wheel_event_listeners = listeners;
    // The scroll tree has the containers that are layers, in order.
    let composited = composited.iter().enumerate().filter(|(_, c)| **c);
    for (node, (index, _)) in trees.scroll.iter_mut().zip(composited) {
        node.wheel_event_handler_region = region.scrollers[index].as_slice().into();
    }
    // And the regions, the sticky boxes the compositor moves.
    let sticky = composited_sticky_nodes(composited_sticky);
    for (regions, node) in trees.sticky_regions.iter_mut().zip(sticky) {
        regions.wheel_event_handler_region = sticky_region(region, node).into();
    }
}

/// Paint's translations of the sticky boxes the compositor moves
/// (`composited_sticky`), in order.
fn composited_sticky_nodes(composited_sticky: &[bool]) -> Vec<paint::TransformNodeId> {
    composited_sticky
        .iter()
        .enumerate()
        .filter(|(_, sticky)| **sticky)
        .map(|(index, _)| paint::TransformNodeId(index))
        .collect()
}

/// What `region` has in the space of the sticky box of translation `node`.
fn sticky_region(
    region: &paint::ScrollHitTestRegion,
    node: paint::TransformNodeId,
) -> &[gfx::RectF] {
    region
        .sticky
        .iter()
        .find(|(n, _)| *n == node)
        .map_or(&[], |(_, rects)| rects.as_slice())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lays out and paints `html` at 400x300, unscrolled, and turns it into
    /// the compositor's trees, as `FrameView` does.
    fn compositor_trees(html: &str) -> PropertyTrees {
        compose(&mut PaintArtifactCompositor::default(), html).1
    }

    /// `html` laid out, painted and turned into layers by `compositor`.
    fn compose(
        compositor: &mut PaintArtifactCompositor,
        html: &str,
    ) -> (Vec<PictureLayer>, PropertyTrees) {
        let (layers, trees) = compose_with(compositor, html, &|_| None, &|_| None);
        let pictures = layers
            .into_iter()
            .map(|layer| match layer {
                Layer::Picture(layer) => layer,
                Layer::Texture(_) => panic!("no canvas shows anything here"),
            })
            .collect();
        (pictures, trees)
    }

    /// The canvases' snapshots as paint asks for them.
    struct Painted<'a>(CanvasSnapshot<'a>);

    impl paint::BackgroundImages for Painted<'_> {
        fn image(&self, _: &str) -> Option<(gfx::ImageData, gfx::Size)> {
            None
        }

        fn canvas(&self, element: gfx::NodeId) -> Option<gfx::ImageData> {
            (self.0)(element).map(|(snapshot, _)| snapshot)
        }
    }

    /// [`compose`], the canvases painted with what `painted` says and
    /// shown in their layers with what `canvases` says.
    fn compose_with(
        compositor: &mut PaintArtifactCompositor,
        html: &str,
        painted: CanvasSnapshot<'_>,
        canvases: CanvasSnapshot<'_>,
    ) -> (Vec<Layer>, PropertyTrees) {
        let (mut tree, _) = crate::testing::box_tree(html);
        let viewport = SizeF::new(400.0, 300.0);
        let view = tree.layout(viewport);
        let size = layout::document_size(&view);
        // The sticky boxes where they were laid out, as `FrameView`
        // hands them over.
        let positions = layout::StickyPositions::new(&view, viewport);
        let stuck = layout::StickyState {
            offsets: positions.offsets(&Default::default(), gfx::PointF::new(0.0, 0.0)),
            reaches: positions.reaches(),
        };
        let sticky = StickyBoxes {
            constraints: positions
                .boxes
                .iter()
                .map(|c| {
                    let around = c
                        .shifting_containing_block
                        .map(|index| positions.boxes[index].key);
                    (c.key, (c.clone(), around))
                })
                .collect(),
            offsets: stuck.offsets.clone(),
            scroll: compositor::ScrollOffsets::from([(
                ElementId::VIEWPORT,
                gfx::PointF::new(0.0, 0.0),
            )]),
        };
        let view = Arc::new(view);
        let pre = paint::pre_paint_frame_tree(
            &view,
            &Default::default(),
            &stuck,
            &Default::default(),
            1.0,
            gfx::PointF::new(0.0, 0.0),
            None,
        );
        let composited = paint::composited_scrollers(&pre.trees);
        let artifact = Arc::new(paint::paint(&view, &pre, size, &Painted(painted)));
        let region = paint::main_thread_scroll_hit_test_region(&artifact, &pre, &composited);
        let attribution = paint::frame_attribution_region(&artifact, &pre, &composited);
        compositor.update(
            &artifact,
            &pre.trees,
            size,
            &region,
            &attribution,
            &sticky,
            canvases,
        )
    }

    /// A layer that was there last time keeps its id and says what changed
    /// in it; a new one gets a new id and is rastered whole.
    /// Layers' bounds snap to whole tiles, so content changing size in one
    /// keeps its tiles.
    #[test]
    fn layers_snap_to_the_compositors_tiles() {
        assert_eq!(
            paint::LAYER_GRID,
            compositor::LayerTreeSettings::default().default_tile_size as f32
        );
    }

    #[test]
    fn a_layer_keeps_its_id_and_says_what_to_raster_again() {
        let page = |first: &str, scroller: &str| {
            format!(
                "<style>html, body {{ margin: 0 }} div {{ width: 100px }}</style>
                 {first}
                 <div style='height: 50px; background-color: COLOR'></div>
                 <div style='height: 100px; overflow: auto'><div style='height: 300px; background-color: green'></div></div>
                 <div style='height: 50px; background-color: blue'></div>{scroller}"
            )
        };
        let mut compositor = PaintArtifactCompositor::default();
        let (first, _) = compose(&mut compositor, &page("", "").replace("COLOR", "green"));
        assert_eq!(first.len(), 3, "before, the scroller, after");
        assert!(first.iter().all(|layer| layer.invalidation.is_none()));

        // The first box turned red: the document's layer rasters it again,
        // the others nothing.
        let (second, _) = compose(&mut compositor, &page("", "").replace("COLOR", "red"));
        let ids = |layers: &[PictureLayer]| layers.iter().map(|l| l.id).collect::<Vec<_>>();
        assert_eq!(ids(&second), ids(&first));
        let invalidation = |layer: &PictureLayer| layer.invalidation.as_deref().map(<[_]>::to_vec);
        assert_eq!(
            invalidation(&second[0]),
            Some(vec![gfx::RectF::new(0.0, 0.0, 100.0, 50.0)])
        );
        assert_eq!(invalidation(&second[1]), Some(vec![]));
        assert_eq!(invalidation(&second[2]), Some(vec![]));
        // Those with nothing to raster again keep their display lists.
        let same_list =
            |a: &PictureLayer, b: &PictureLayer| Arc::ptr_eq(&a.display_list, &b.display_list);
        assert!(!same_list(&second[0], &first[0]));
        assert!(same_list(&second[1], &first[1]) && same_list(&second[2], &first[2]));

        // Another scroller after the rest: the layers before it are the
        // same ones; it and the document's layer after it are new.
        let more = "<div style='height: 100px; overflow: auto'><div style='height: 300px; background-color: green'></div></div>
                    <div style='height: 50px; background-color: blue'></div>";
        let (third, _) = compose(&mut compositor, &page("", more).replace("COLOR", "red"));
        assert_eq!(third.len(), 5, "and the new scroller, and what is after it");
        assert_eq!(ids(&third)[..3], ids(&first)[..]);
        assert_ne!(third[3].id, third[4].id);
        assert!(
            third[3..]
                .iter()
                .all(|l| l.invalidation.is_none() && !ids(&first).contains(&l.id))
        );
    }

    /// Each container that is a layer has a scroll node, its parent the
    /// nearest such around it (past one that isn't, with nothing to
    /// scroll), and the region goes where what it covers moves: in the
    /// outer one's node, or the trees'.
    #[test]
    fn the_scroll_tree_has_the_containers_that_are_layers_and_their_regions() {
        let trees = compositor_trees(
            "<style>html, body { margin: 0 } .layer { width: 200px; height: 100px; overflow: auto }</style>
             <div style='width: 100px; height: 50px; overflow: auto; opacity: 0.9'><div style='height: 300px'></div></div>
             <div class=layer id=outer>
               <div style='overflow: auto'>
                 <div class=layer id=inner><div style='height: 300px'></div></div>
               </div>
               <div style='width: 100px; height: 50px; overflow: auto; opacity: 0.9'><div style='height: 300px'></div></div>
               <div style='height: 300px'></div>
             </div>",
        );
        let parents: Vec<_> = trees.scroll.iter().map(|node| node.parent).collect();
        assert_eq!(parents, [None, Some(ScrollNodeId(0))], "outer, then inner");
        assert_eq!(
            trees.main_thread_scroll_hit_test_region.len(),
            1,
            "the first box"
        );
        assert_eq!(
            trees.scroll[0].main_thread_scroll_hit_test_region.len(),
            1,
            "the box in the outer one"
        );
        assert!(
            trees.scroll[1]
                .main_thread_scroll_hit_test_region
                .is_empty()
        );
    }

    /// A page with a canvas, `style` around it, between two boxes.
    fn canvas_page(style: &str) -> String {
        format!(
            "<style>html, body {{ margin: 0 }}</style>
             <div style='height: 10px; background: red'></div>
             <div style='{style}'><canvas width=20 height=10 style='display: block; margin: 5px; border: 1px solid'></canvas></div>
             <div>after</div>"
        )
    }

    /// A canvas that can be a layer is a `TextureLayer` of its bitmap as it
    /// is now, over its content box, between the layers of what is painted
    /// before and after it. It keeps its id while it is one; under opacity
    /// it is painted into the page, and is no layer.
    #[test]
    fn a_canvas_is_a_texture_layer_of_its_snapshot_that_keeps_its_id() {
        let element = std::cell::Cell::new(None);
        let old = gfx::ImageData::decoded(gfx::Size::new(1, 1), vec![0, 0, 0, 255]);
        let new = gfx::ImageData::decoded(gfx::Size::new(1, 1), vec![0, 0, 255, 255]);
        let painted = |canvas| {
            element.set(Some(canvas));
            Some((old.clone(), false))
        };
        let mut compositor = PaintArtifactCompositor::default();
        let (layers, _) = compose_with(&mut compositor, &canvas_page(""), &painted, &|_| {
            Some((new.clone(), true))
        });
        let element = element.get().expect("the canvas was painted");
        assert_eq!(layers.len(), 3, "{layers:?}");
        assert!(matches!(layers[0], Layer::Picture(_)));
        assert!(matches!(layers[2], Layer::Picture(_)));
        let Layer::Texture(texture) = &layers[1] else {
            panic!("{:?}", layers[1]);
        };
        // The bitmap now, not the one painted.
        assert_eq!(texture.snapshot.id, new.id);
        assert!(texture.contents_opaque);
        assert_eq!(texture.bounds, SizeF::new(20.0, 10.0));
        assert_eq!(texture.properties.origin, gfx::PointF::new(6.0, 16.0));
        assert_eq!(compositor.canvas_layer(element), Some(texture.id));
        let id = texture.id;

        let (again, _) = compose_with(&mut compositor, &canvas_page(""), &painted, &|_| {
            Some((old.clone(), false))
        });
        assert_eq!(again[1].id(), id);
        assert_eq!(compositor.canvas_layer(element), Some(id));

        let (faded, _) = compose_with(
            &mut compositor,
            &canvas_page("opacity: 0.5"),
            &painted,
            &|_| Some((old.clone(), false)),
        );
        assert!(faded.iter().all(|layer| matches!(layer, Layer::Picture(_))));
        assert_eq!(compositor.canvas_layer(element), None);
    }

    /// A canvas whose bitmap is wider or taller than a texture Viz takes is
    /// no layer: its bitmap is drawn in the page's layer, as painted. One
    /// as big as the texture is a layer.
    #[test]
    fn a_canvas_too_big_for_a_texture_is_painted_into_the_page() {
        for (size, layer) in [
            (gfx::Size::new(4, 4), true),
            (gfx::Size::new(5, 1), false),
            (gfx::Size::new(1, 5), false),
        ] {
            let pixels = vec![0; gfx::Size::checked_byte_size(size).unwrap()];
            let snapshot = gfx::ImageData::decoded(size, pixels);
            let mut compositor = PaintArtifactCompositor {
                max_texture_size: 4,
                ..Default::default()
            };
            let (layers, _) = compose_with(
                &mut compositor,
                &canvas_page(""),
                &|_| Some((snapshot.clone(), false)),
                &|_| Some((snapshot.clone(), false)),
            );
            let textures = layers
                .iter()
                .filter(|layer| matches!(layer, Layer::Texture(_)))
                .count();
            assert_eq!(textures, usize::from(layer), "{size:?}: {layers:?}");
            let drawn = layers.iter().any(|layer| {
                let Layer::Picture(picture) = layer else {
                    return false;
                };
                picture.display_list.ops().iter().any(|op| {
                    matches!(op, compositor_paint::PaintOp::DrawImage { image, .. } if image.id == snapshot.id)
                })
            });
            assert_eq!(drawn, !layer, "{size:?}");
        }
    }

    /// A canvas that shows nothing by the time its layer is made (reset
    /// after it was painted) has no layer.
    #[test]
    fn a_canvas_that_shows_nothing_has_no_layer() {
        let snapshot = gfx::ImageData::decoded(gfx::Size::new(1, 1), vec![0, 0, 0, 255]);
        let element = std::cell::Cell::new(None);
        let mut compositor = PaintArtifactCompositor::default();
        let (layers, _) = compose_with(
            &mut compositor,
            &canvas_page(""),
            &|canvas| {
                element.set(Some(canvas));
                Some((snapshot.clone(), false))
            },
            &|_| None,
        );
        assert!(
            layers
                .iter()
                .all(|layer| matches!(layer, Layer::Picture(_)))
        );
        assert_eq!(compositor.canvas_layer(element.get().unwrap()), None);
    }

    /// A sticky box the compositor moves: its translation node has layout's
    /// constraints, its scroller the viewport, and the sticky box moving
    /// its containing block the node of that box; the regions in it are in
    /// its space.
    #[test]
    fn a_sticky_boxs_node_has_its_constraints() {
        let (layers, trees) = compose(
            &mut PaintArtifactCompositor::default(),
            "<style>html, body { margin: 0 }</style>
             <div style='height: 50px'></div>
             <div style='position: sticky; top: 0; height: 100px; background: green'>
               <div style='height: 20px'></div>
               <div style='position: sticky; top: 5px; height: 20px; background: blue'></div>
               <div style='width: 50px; height: 40px; overflow: auto; opacity: 0.5'>
                 <div style='height: 100px'></div>
               </div>
             </div>
             <div style='height: 2000px'></div>",
        );
        let sticky: Vec<(TransformNodeId, StickyPositionConstraint)> = trees
            .transform
            .iter()
            .enumerate()
            .filter_map(|(index, node)| Some((TransformNodeId(index), node.sticky?)))
            .collect();
        let [(outer, o), (_, inner)] = sticky[..] else {
            panic!("two sticky boxes the compositor moves: {sticky:?}");
        };
        assert_eq!(o.scroller, ElementId::VIEWPORT);
        assert_eq!(o.sticky_box, gfx::RectF::new(0.0, 50.0, 400.0, 100.0));
        assert_eq!(o.inset, [Some(0.0), None, None, None]);
        assert_eq!(o.shifting_containing_block, None);
        assert_eq!(o.pixel_snap_offset, gfx::PointF::new(0.0, 50.0));
        assert_eq!(inner.shifting_containing_block, Some(outer));
        assert_eq!(inner.inset, [Some(5.0), None, None, None]);
        // The two boxes' own layers.
        let in_sticky = layers
            .iter()
            .filter(|layer| {
                trees.transform[layer.properties.transform.0]
                    .sticky
                    .is_some()
            })
            .count();
        assert_eq!(in_sticky, 2);
        // Regions for each, in order: the container that isn't a layer, at
        // 40 in the outer box (laid out at 50), 1px grown; none in the
        // inner one.
        let [regions, inner_regions] = &trees.sticky_regions[..] else {
            panic!("two sticky boxes' regions: {:?}", trees.sticky_regions);
        };
        assert_eq!(regions.transform, outer);
        assert_eq!(
            regions.main_thread_scroll_hit_test_region[..],
            [gfx::RectF::new(-1.0, 89.0, 52.0, 42.0)]
        );
        assert!(inner_regions.main_thread_scroll_hit_test_region.is_empty());
    }
}
