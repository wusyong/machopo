use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use base::SequenceNumber;
use base::numerics::{clamp_ceil, clamp_floor};
use gfx::{Color, PointF, RRectF, RectF, Size, SizeF};
use protocol::viz::{
    CompositorFrame, CompositorFrameMetadata, CompositorRenderPass, DrawQuad, LocalSurfaceId,
    Material, RenderPassId, ResourceFormat, ResourceId, SharedQuadState, TransferableResource,
};
use raster::PlaybackParams;

use crate::DocumentId;
use crate::property_trees::ScrollHitTest;
use crate::proxy_impl::CompositorScrollDelta;
use crate::synced_scroll_offset::SyncedScrollOffset;
use crate::{
    ElementId, Layer, LayerProperties, LayerTreeFrameSink, LayerTreeSettings, PictureLayer,
    PropertyTrees, ScrollNode, ScrollNodeId, ScrollOffsets, SurfaceLayer, SwapPromise,
    TextureLayer,
};

/// Everything the main thread hands over at commit
/// (`LayerTreeHost::FinishCommitOnImplThread`'s input).
pub(crate) struct CommitState {
    /// In paint order, the last on top.
    pub layers: Vec<Layer>,
    pub property_trees: PropertyTrees,
    /// The offsets of the containers in the scroll tree, CSS px; one
    /// missing is at 0, 0.
    pub scroll_offsets: ScrollOffsets,
    pub surface_layers: Vec<SurfaceLayer>,
    pub viewport_size: Size,
    pub device_scale_factor: f32,
    pub background_color: Color,
    pub local_surface_id: LocalSurfaceId,
    /// Which document the layers show.
    pub document_id: DocumentId,
    /// The viewport's scroll offset, CSS px.
    pub root_scroll_offset: PointF,
    /// The viewport as a scroller; `None` until the main thread has laid
    /// a document out, and for hosts that don't scroll.
    pub viewport_scroll_node: Option<ScrollNode>,
    pub source_frame_number: u64,
    /// Kept until a frame shows this commit.
    pub swap_promises: Vec<SwapPromise>,
}

/// What changed since the last frame drawn (the damage cc tracks for a
/// frame's `damage_rect`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Damage {
    None,
    /// In the document's physical pixels.
    Rect(RectF),
    /// All of the frame.
    Full,
}

impl Damage {
    fn add(self, other: Damage) -> Damage {
        match (self, other) {
            (Damage::Full, _) | (_, Damage::Full) => Damage::Full,
            (Damage::None, damage) | (damage, Damage::None) => damage,
            (Damage::Rect(a), Damage::Rect(b)) => Damage::Rect(a.union(&b)),
        }
    }
}

/// `cc::Tile`: one rectangle of a layer's scaled content.
#[derive(Clone)]
struct Tile {
    /// In the layer's content space, physical pixels.
    content_rect: RectF,
    /// What it is drawn with, once rastered.
    draw_info: Option<TileDrawInfo>,
}

/// What one `prepare_tiles` did: counts of tiles, each in memory, so
/// adding to them saturates as it never gets to.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TilesPrepared {
    pub(crate) rastered: usize,
    /// Of those rastered, of one color: no pixels kept.
    pub(crate) solid: usize,
    /// Not rastered: of one color, as their ops tell (none draws in them,
    /// or an opaque fill covers them).
    pub(crate) skipped: usize,
    pub(crate) kept: usize,
}

/// `cc::TileDrawInfo`: a tile's raster, or the one color it came out as.
/// Raster is checked rather than predicted (cc's `SolidColorAnalyzer`
/// looks at the paint ops before rastering), but for a tile no op draws in,
/// which is the color its layer is cleared to: a tile of one opaque color,
/// or nothing at all, keeps no pixels, here or in Viz.
#[derive(Clone)]
enum TileDrawInfo {
    Resource(TransferableResource),
    /// Drawn as a `SolidColor` quad; not at all if transparent.
    SolidColor(Color),
}

impl TileDrawInfo {
    /// What `pixels` (RGBA, premultiplied) come to: one opaque color, or
    /// transparent everywhere, as a color; anything else, `None`. A
    /// translucent color isn't one: unpremultiplied, it might not draw
    /// back the same.
    fn solid_color(pixels: &[u8]) -> Option<Color> {
        let (first, rest) = pixels.split_first_chunk::<4>()?;
        let (rest, _) = rest.as_chunks::<4>();
        if !rest.iter().all(|pixel| pixel == first) {
            return None;
        }
        let [r, g, b, a] = *first;
        match a {
            255 => Some(Color::rgba(r, g, b, a)),
            0 => Some(Color::TRANSPARENT),
            _ => None,
        }
    }
}

/// `cc::PictureLayerTiling` at a single scale, over `cc::TilingData`: the
/// layer's content cut into a grid of tiles, of which only those near the
/// viewport exist (`prepare_tiles` makes them, and drops the others), so a
/// layer of any size costs only what is near. The grid is worked out in
/// whole numbers: a content size past f32's integers, where adding a tile's
/// width no longer moves on, still has a last tile.
#[derive(Clone)]
struct PictureLayerTiling {
    contents_scale: f32,
    /// The content's size, physical pixels, rounded up.
    content_size: (f64, f64),
    tile_size: u32,
    /// The tiles there are, by row and column: in row order, as drawn.
    tiles: BTreeMap<(u64, u64), Tile>,
}

impl PictureLayerTiling {
    fn new(content_size: SizeF, contents_scale: f32, tile_size: u32) -> Self {
        let size = |v: f32| f64::from(v).ceil().max(0.0);
        Self {
            contents_scale,
            content_size: (size(content_size.width), size(content_size.height)),
            tile_size,
            tiles: BTreeMap::new(),
        }
    }

    /// The tile at `row`, `column`: in the layer's content space, the
    /// last row's and column's cut off at its edge.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "worked out in f64, the rect is an f32's: the nearest one is wanted"
    )]
    fn tile_rect(&self, (row, column): (u64, u64)) -> RectF {
        let ts = f64::from(self.tile_size);
        let (x, y) = (column as f64 * ts, row as f64 * ts);
        let (width, height) = self.content_size;
        RectF::new(
            x as f32,
            y as f32,
            ts.min(width - x) as f32,
            ts.min(height - y) as f32,
        )
    }

    /// The rows and columns of the tiles `rect` (the layer's content
    /// space) touches, half-open (`TilingData::TileXIndexFromSrcCoord`).
    fn tiles_in(&self, rect: RectF) -> (std::ops::Range<u64>, std::ops::Range<u64>) {
        let ts = f64::from(self.tile_size);
        let (width, height) = self.content_size;
        let span = |start: f32, length: f32, size: f64| {
            let (start, end) = (f64::from(start), f64::from(start) + f64::from(length));
            let (start, end) = (start.max(0.0), end.min(size));
            if end <= start {
                return 0..0;
            }
            clamp_floor::<u64>(start / ts)..clamp_ceil::<u64>(end / ts)
        };
        (
            span(rect.y, rect.height, height),
            span(rect.x, rect.width, width),
        )
    }

    /// A tiling for `content_size` that keeps the tiles of `self` (a layer
    /// at the same scale, with another recording) that are where they were
    /// and that `invalidation` (the layer's space, CSS px) doesn't touch
    /// (cc's `PictureLayerTiling` taking over the active tree's tiles but
    /// the invalidated ones). A tile at the edge that grew or shrank is
    /// rastered again.
    fn keeping(&self, content_size: SizeF, tile_size: u32, invalidation: &[RectF]) -> Self {
        let scale = self.contents_scale;
        let invalidated: Vec<RectF> = invalidation.iter().map(|rect| rect.scale(scale)).collect();
        let mut tiling = Self::new(content_size, scale, tile_size);
        if tile_size != self.tile_size {
            return tiling;
        }
        for (&index, tile) in &self.tiles {
            let rect = tiling.tile_rect(index);
            let kept = tile.content_rect == rect
                && !invalidated
                    .iter()
                    .any(|invalid| !invalid.intersect(&rect).is_empty());
            if kept && let Some(draw_info) = &tile.draw_info {
                tiling.tiles.insert(
                    index,
                    Tile {
                        content_rect: rect,
                        draw_info: Some(draw_info.clone()),
                    },
                );
            }
        }
        tiling
    }
}

/// `cc::PictureLayerImpl`.
struct PictureLayerImpl {
    layer: PictureLayer,
    tiling: PictureLayerTiling,
}

impl PictureLayerImpl {
    /// Whether `layer`'s tiles would come out the same as this one's: the
    /// same recording (the same `Arc`, as paint hands out a new display list
    /// whenever anything in it changes), size, scale and tile clearing.
    fn rasters_like(&self, layer: &PictureLayer, contents_scale: f32) -> bool {
        Arc::ptr_eq(&self.layer.display_list, &layer.display_list)
            && self.layer.bounds == layer.bounds
            && self.clears_like(layer, contents_scale)
    }

    /// Whether `layer`'s tiles are rastered at the same scale onto the same
    /// clear color as this one's: those its invalidation doesn't touch come
    /// out the same.
    fn clears_like(&self, layer: &PictureLayer, contents_scale: f32) -> bool {
        self.tiling.contents_scale == contents_scale
            && self.layer.contents_opaque == layer.contents_opaque
            && self.layer.background_color == layer.background_color
    }
}

/// `cc::TextureLayerImpl`: a texture layer, and its snapshot as a resource
/// once it is made (on the GPU, if there is one).
struct TextureLayerImpl {
    layer: TextureLayer,
    resource: Option<TransferableResource>,
}

/// `cc::LayerImpl`'s kinds.
enum LayerImpl {
    Picture(PictureLayerImpl),
    Texture(TextureLayerImpl),
}

impl LayerImpl {
    fn id(&self) -> crate::LayerId {
        match self {
            Self::Picture(layer) => layer.layer.id,
            Self::Texture(layer) => layer.layer.id,
        }
    }

    fn bounds(&self) -> SizeF {
        match self {
            Self::Picture(layer) => layer.layer.bounds,
            Self::Texture(layer) => layer.layer.bounds,
        }
    }

    fn properties(&self) -> LayerProperties {
        match self {
            Self::Picture(layer) => layer.layer.properties,
            Self::Texture(layer) => layer.layer.properties,
        }
    }
}

/// `cc::LayerTreeImpl`: the pending tree a commit builds, or the active tree
/// frames are drawn from.
struct LayerTreeImpl {
    /// In paint order, the last on top.
    layers: Vec<LayerImpl>,
    property_trees: PropertyTrees,
    surface_layers: Vec<SurfaceLayer>,
    viewport_size: Size,
    device_scale_factor: f32,
    background_color: Color,
    local_surface_id: LocalSurfaceId,
    document_id: DocumentId,
    viewport_scroll_node: Option<ScrollNode>,
    #[allow(dead_code)]
    source_frame_number: u64,
}

/// `cc::LayerTreeHostImpl`: the impl-side half. Owns tiles and resources and
/// produces compositor frames.
pub struct LayerTreeHostImpl {
    settings: LayerTreeSettings,
    frame_sink: Box<dyn LayerTreeFrameSink>,
    /// The last commit, until its tiles are rastered and it is activated.
    /// Raster runs inline, so activation follows in the same task.
    pending_tree: Option<LayerTreeImpl>,
    active_tree: Option<LayerTreeImpl>,
    /// The promises of every commit activated since the last draw: the
    /// next frame shows them all.
    swap_promises: Vec<SwapPromise>,
    /// The document of the last frame submitted: what the user sees.
    drawn_document: Option<DocumentId>,
    /// The viewport's scroll offset: the main thread's, and what the impl
    /// side scrolled since. Scrolling here needs no commit, and the tiles
    /// stay.
    scroll: SyncedScrollOffset,
    /// The same for each container in the scroll tree, by element.
    container_scrolls: HashMap<ElementId, SyncedScrollOffset>,
    /// The viewport's offset, in physical pixels, and the containers', the
    /// tiles were last prepared for: a scroll since may need others.
    tiles_prepared_for: Option<(PointF, ScrollOffsets)>,
    /// What the last `prepare_tiles` did: tiles rastered (of which of one
    /// color), and kept from before.
    tiles_prepared: TilesPrepared,
    /// What commits changed since the last frame drawn.
    damage: Damage,
    /// Where the viewport (physical pixels) and the containers were in the
    /// last frame drawn: moved since, all of the next is damaged.
    drawn_at: Option<(PointF, ScrollOffsets)>,
    /// From 1, never handed out twice: Viz keys what it uploaded by it.
    resource_ids: SequenceNumber<u64>,
    /// From 1, in order: what the browser and headless compare.
    frame_tokens: SequenceNumber<u64>,
    /// This compositor's own: no other renderer shares it. Its raster
    /// workers do.
    raster_cache: raster::RasterCache,
    /// `settings.num_raster_threads` threads (cc's `CategorizedWorkerPool`,
    /// one a renderer), made the first time there are tiles for more than
    /// one.
    raster_pool: Option<rayon::ThreadPool>,
}

/// A tile to raster: which, how, and with which of its list's ops. With
/// `expect`, it is of one color already, as its ops told; a debug build
/// rasters it to check.
struct RasterJob {
    layer: usize,
    tile: (u64, u64),
    list: Arc<compositor_paint::DisplayItemList>,
    params: PlaybackParams,
    ops: Vec<usize>,
    expect: Option<Color>,
}

thread_local! {
    /// What each thread that rasters tiles draws them into, tile after tile:
    /// a tile's pixels go to the GPU from here, or are copied out to be
    /// kept, so drawing a tile allocates nothing more (cc's raster buffers
    /// are reused too, by its `ResourcePool`).
    static TILE_PIXELS: std::cell::RefCell<Vec<u8>> = std::cell::RefCell::default();
}

/// What a tile came out as: of one color, or its pixels, put on the GPU
/// (then not kept) if there is one, by the thread that rastered it.
enum Rastered {
    Solid(Color),
    Pixels {
        size: Size,
        pixels: Arc<[u8]>,
        shared_image: Option<protocol::viz::SharedImage>,
    },
}

impl LayerTreeHostImpl {
    pub(crate) fn new(
        settings: LayerTreeSettings,
        frame_sink: Box<dyn LayerTreeFrameSink>,
    ) -> Self {
        Self {
            settings,
            frame_sink,
            pending_tree: None,
            active_tree: None,
            swap_promises: Vec::new(),
            drawn_document: None,
            scroll: SyncedScrollOffset::default(),
            container_scrolls: HashMap::new(),
            tiles_prepared_for: None,
            tiles_prepared: TilesPrepared::default(),
            damage: Damage::None,
            drawn_at: None,
            resource_ids: SequenceNumber::starting_at(1),
            frame_tokens: SequenceNumber::starting_at(1),
            raster_cache: raster::RasterCache::default(),
            raster_pool: None,
        }
    }

    /// Builds the pending tree from a commit. Each layer keeps the tiles its
    /// last commit's (the same id) has when they would come out the same:
    /// all of them with the same display list (a commit that only moved the
    /// viewport, or scrolled a container that is a layer, rasters nothing),
    /// else those its invalidation doesn't touch. Another document's layer
    /// keeps nothing.
    pub(crate) fn commit(&mut self, mut state: CommitState) {
        let _span = trace_event::trace_event!("LayerTreeHostImpl::commit");
        let scale = state.device_scale_factor;
        let previous = self.pending_tree.as_ref().or(self.active_tree.as_ref());
        let previous_document = previous.map(|tree| tree.document_id);
        let same_document = previous_document == Some(state.document_id);
        let tile_size = self.settings.default_tile_size;
        let layers: Vec<LayerImpl> = std::mem::take(&mut state.layers)
            .into_iter()
            .map(|layer| {
                let last = previous
                    .filter(|_| same_document)
                    .and_then(|tree| tree.layers.iter().find(|l| l.id() == layer.id()));
                match layer {
                    Layer::Picture(layer) => {
                        let size =
                            SizeF::new(layer.bounds.width * scale, layer.bounds.height * scale);
                        let last = match last {
                            Some(LayerImpl::Picture(last)) => Some(last),
                            _ => None,
                        };
                        let tiling = match (last, &layer.invalidation) {
                            (Some(last), _) if last.rasters_like(&layer, scale) => {
                                last.tiling.clone()
                            }
                            (Some(last), Some(invalidation)) if last.clears_like(&layer, scale) => {
                                last.tiling.keeping(size, tile_size, invalidation)
                            }
                            _ => PictureLayerTiling::new(size, scale, tile_size),
                        };
                        LayerImpl::Picture(PictureLayerImpl { layer, tiling })
                    }
                    // The same snapshot keeps its resource: it is put on the
                    // GPU once (`TextureLayer::SetNeedsDisplay` gives a new
                    // one).
                    Layer::Texture(layer) => {
                        let resource = match last {
                            Some(LayerImpl::Texture(last))
                                if last.layer.snapshot == layer.snapshot =>
                            {
                                last.resource.clone()
                            }
                            _ => None,
                        };
                        LayerImpl::Texture(TextureLayerImpl { layer, resource })
                    }
                }
            })
            .collect();
        let damage = match previous {
            Some(previous) if same_document => self.damage_of_commit(previous, &layers, &state),
            _ => Damage::Full,
        };
        self.damage = self.damage.add(damage);
        // What was scrolled here belongs to the document it was scrolled
        // in: another's starts where its main thread put it.
        if previous_document.is_some_and(|document| document != state.document_id) {
            self.scroll.reset(state.root_scroll_offset);
            self.container_scrolls.clear();
        } else {
            self.scroll.push_from_main(
                state.root_scroll_offset,
                state.viewport_scroll_node.as_ref(),
            );
        }
        // Each container in the scroll tree likewise. One that left it (no
        // longer a layer, or gone) takes along what was scrolled here and
        // not handed over yet: the main thread paints it where it has it.
        let mut container_scrolls = HashMap::new();
        for node in &state.property_trees.scroll {
            let base = state
                .scroll_offsets
                .get(&node.element)
                .copied()
                .unwrap_or_default();
            let mut scroll = self
                .container_scrolls
                .remove(&node.element)
                .unwrap_or_default();
            scroll.push_from_main(base, Some(&node.scroller));
            container_scrolls.insert(node.element, scroll);
        }
        self.container_scrolls = container_scrolls;
        self.swap_promises.extend(state.swap_promises);
        self.pending_tree = Some(LayerTreeImpl {
            layers,
            property_trees: state.property_trees,
            surface_layers: state.surface_layers,
            viewport_size: state.viewport_size,
            device_scale_factor: state.device_scale_factor,
            background_color: state.background_color,
            local_surface_id: state.local_surface_id,
            document_id: state.document_id,
            viewport_scroll_node: state.viewport_scroll_node,
            source_frame_number: state.source_frame_number,
        });
    }

    /// What a commit to the same document changes of what is drawn, in the
    /// document's physical pixels: each layer with a new display list, its
    /// invalidation where the layer is, cut by its clip. Anything else that
    /// changed (the layers or where they are, the viewport, the surface
    /// layers, the background) changes all of it.
    fn damage_of_commit(
        &self,
        previous: &LayerTreeImpl,
        layers: &[LayerImpl],
        state: &CommitState,
    ) -> Damage {
        let same_frame = previous.viewport_size == state.viewport_size
            && previous.device_scale_factor == state.device_scale_factor
            && previous.background_color == state.background_color
            && previous.surface_layers == state.surface_layers
            && previous.property_trees == state.property_trees
            && previous.layers.len() == layers.len()
            && previous.layers.iter().zip(layers).all(|(was, is)| {
                was.id() == is.id()
                    && was.bounds() == is.bounds()
                    && was.properties() == is.properties()
            });
        if !same_frame {
            return Damage::Full;
        }
        let scale = state.device_scale_factor;
        let offsets = self.drawn_scroll_offsets(scale);
        let drawn_scroll = self.scroll_offset_in_pixels(previous);
        let mut damage = Damage::None;
        for (was, is) in previous.layers.iter().zip(layers) {
            let all = [RectF::from_size(is.bounds())];
            let rects = match (was, is) {
                (LayerImpl::Picture(was), LayerImpl::Picture(is)) => {
                    if Arc::ptr_eq(&was.layer.display_list, &is.layer.display_list) {
                        continue;
                    }
                    is.layer.invalidation.as_deref().unwrap_or(&all)
                }
                // A new snapshot: all of it.
                (LayerImpl::Texture(was), LayerImpl::Texture(is)) => {
                    if was.layer.snapshot == is.layer.snapshot {
                        continue;
                    }
                    &all
                }
                _ => return Damage::Full,
            };
            // The trees are the same as before: where the layer was is
            // where it is.
            let properties = is.properties();
            let origin =
                Self::layer_origin_in_pixels(previous, &offsets, &properties, drawn_scroll);
            let clip = Self::layer_clip_in_pixels(previous, &offsets, &properties);
            for rect in rects {
                let mut rect = rect.scale(scale).offset(origin.x, origin.y);
                if let Some(clip) = clip {
                    rect = rect.intersect(&clip);
                }
                if !rect.is_empty() {
                    damage = damage.add(Damage::Rect(rect));
                }
            }
        }
        damage
    }

    /// `ActivateSyncTree`: the pending tree becomes the one frames are
    /// drawn from. Call after `prepare_tiles`, which rasters what it shows.
    /// Whether there was one.
    pub(crate) fn activate(&mut self) -> bool {
        let Some(pending) = self.pending_tree.take() else {
            return false;
        };
        self.active_tree = Some(pending);
        true
    }

    /// `DidLoseLayerTreeFrameSink`, and what cc does on getting a new one
    /// (`RecreateTileResources`, `SetFullViewportDamage`): the GPU the
    /// tiles' shared images were on was lost. Every tile with a resource is
    /// rastered again at the next `prepare_tiles`, the ones of one color
    /// (no resource) stay, and all of the next frame is damaged.
    pub(crate) fn did_lose_layer_tree_frame_sink(&mut self) {
        for tree in [&mut self.pending_tree, &mut self.active_tree]
            .into_iter()
            .flatten()
        {
            for layer in &mut tree.layers {
                match layer {
                    LayerImpl::Picture(layer) => {
                        layer.tiling.tiles.retain(|_, tile| {
                            !matches!(tile.draw_info, Some(TileDrawInfo::Resource(_)))
                        });
                    }
                    // Its snapshot is put on the new GPU at the next
                    // `prepare_tiles`.
                    LayerImpl::Texture(layer) => layer.resource = None,
                }
            }
        }
        self.tiles_prepared_for = None;
        self.drawn_at = None;
    }

    /// What the last `prepare_tiles` did.
    #[cfg(test)]
    pub(crate) fn tiles_prepared(&self) -> TilesPrepared {
        self.tiles_prepared
    }

    /// How many of the active tree's tiles have been rastered and kept.
    #[cfg(test)]
    pub(crate) fn resident_tiles(&self) -> usize {
        self.active_tree.as_ref().map_or(0, |tree| {
            tree.layers
                .iter()
                .filter_map(|layer| match layer {
                    LayerImpl::Picture(layer) => Some(layer),
                    LayerImpl::Texture(_) => None,
                })
                .flat_map(|layer| layer.tiling.tiles.values())
                .filter(|t| t.draw_info.is_some())
                .count()
        })
    }

    /// The viewport's offset as the main thread has it, with nothing
    /// scrolled here: a single-threaded host's, which the impl side never
    /// scrolls.
    pub(crate) fn reset_scroll_offset(&mut self, offset: PointF) {
        self.scroll.reset(offset);
    }

    /// The active tree, if the impl side may scroll in it: only the
    /// document the user sees scrolls, none before a frame shows the active
    /// tree, so whatever is scrolled belongs to the drawn document.
    fn scrollable_tree(&self) -> Option<&LayerTreeImpl> {
        let tree = self.active_tree.as_ref()?;
        (self.drawn_document == Some(tree.document_id)).then_some(tree)
    }

    /// The viewport as a scroller, if the impl side may scroll it.
    pub(crate) fn scrollable_viewport(&self) -> Option<ScrollNode> {
        self.scrollable_tree()?.viewport_scroll_node
    }

    /// The impl side scrolls by `delta` (CSS px), as far as it goes
    /// (`LayerTreeHostImpl::ScrollUpdate`): from the container `from`, each
    /// takes what it can and the one around it the rest, then the viewport
    /// (scroll chaining, as the main thread does). Whether anything moved.
    pub(crate) fn scroll_from(&mut self, from: Option<ScrollNodeId>, delta: PointF) -> bool {
        let Some(tree) = self.scrollable_tree() else {
            return false;
        };
        let mut chain = Vec::new();
        let mut next = from;
        while let Some(id) = next {
            let node = &tree.property_trees.scroll[id.0];
            chain.push((node.element, node.scroller));
            next = node.parent;
        }
        let viewport = tree.viewport_scroll_node;
        let mut rest = delta;
        let mut moved = false;
        for (element, scroller) in chain {
            if let Some(scroll) = self.container_scrolls.get_mut(&element) {
                moved |= scroll.scroll_by(&scroller, &mut rest);
            }
        }
        if let Some(viewport) = viewport {
            moved |= self.scroll.scroll_by(&viewport, &mut rest);
        }
        moved
    }

    /// Scrolls the viewport alone, as keys do.
    #[cfg(test)]
    pub(crate) fn scroll_viewport(&mut self, delta: PointF) -> bool {
        self.scroll_from(None, delta)
    }

    #[cfg(test)]
    pub(crate) fn active_property_trees(&self) -> PropertyTrees {
        self.active_tree
            .as_ref()
            .expect("a tree")
            .property_trees
            .clone()
    }

    /// The container that is `element`'s, in the active tree.
    #[cfg(test)]
    pub(crate) fn scroll_node_of(&self, element: ElementId) -> Option<ScrollNodeId> {
        let tree = self.active_tree.as_ref()?;
        let index = tree
            .property_trees
            .scroll
            .iter()
            .position(|node| node.element == element)?;
        Some(ScrollNodeId(index))
    }

    /// What a wheel at `point` (viewport CSS px) is for, in the active
    /// tree, with everything where it is drawn now: the viewport unrounded,
    /// the containers as drawn, unsent scrolling included (where the main
    /// thread's will be once it has it all).
    pub(crate) fn scroll_hit_test(&self, point: PointF) -> ScrollHitTest {
        let Some(tree) = self.active_tree.as_ref() else {
            return ScrollHitTest::MainThread { blocking: true };
        };
        let offset = self.scroll.current();
        let at = PointF::new(point.x + offset.x, point.y + offset.y);
        tree.property_trees
            .scroll_hit_test(at, &self.drawn_scroll_offsets(tree.device_scale_factor))
    }

    /// Which frame input at `point` (viewport CSS px) is for, in the active
    /// tree, with everything where it is drawn now, as for a wheel; the
    /// drawn document's before any tree.
    pub(crate) fn frame_attribution(&self, point: PointF) -> crate::InputAttribution {
        let Some(tree) = self.active_tree.as_ref() else {
            return crate::InputAttribution::TargetedFrame(None);
        };
        let offset = self.scroll.current();
        let at = PointF::new(point.x + offset.x, point.y + offset.y);
        tree.property_trees
            .frame_attribution(at, &self.drawn_scroll_offsets(tree.device_scale_factor))
    }

    /// Whether the active tree's page may have `wheel` listeners: the
    /// wheels the compositor handles go to the main thread too.
    pub(crate) fn wheel_event_listeners(&self) -> bool {
        self.active_tree
            .as_ref()
            .is_some_and(|tree| tree.property_trees.wheel_event_listeners)
    }

    /// `QueuePinnedSwapPromise`: answered by the next frame drawn, whatever
    /// commits before it.
    pub(crate) fn queue_swap_promise(&mut self, promise: SwapPromise) {
        self.swap_promises.push(promise);
    }

    /// Answers the input acks waiting for a frame with `None`: the tab is
    /// hidden and draws none. The other promises keep waiting.
    pub(crate) fn break_input_acks(&mut self) {
        let (acks, rest) = std::mem::take(&mut self.swap_promises)
            .into_iter()
            .partition::<Vec<_>, _>(SwapPromise::is_input_ack);
        self.swap_promises = rest;
        for ack in acks {
            ack.did_not_swap();
        }
    }

    /// What a `BeginMainFrame` hands the main thread: what was scrolled
    /// here since the last, in the drawn document.
    pub(crate) fn take_scroll_for_main_frame(&mut self) -> Option<CompositorScrollDelta> {
        let viewport = self.scroll.take_for_main_frame();
        let containers = self
            .container_scrolls
            .iter_mut()
            .filter_map(|(element, scroll)| Some((*element, scroll.take_for_main_frame()?)))
            .collect();
        self.scroll_delta(viewport, containers)
    }

    /// What an input forwarded to the main thread hands it: what was
    /// scrolled here since the last handed over.
    pub(crate) fn take_scroll_for_input(&mut self) -> Option<CompositorScrollDelta> {
        let viewport = self.scroll.take_for_input();
        let containers = self
            .container_scrolls
            .iter_mut()
            .filter_map(|(element, scroll)| Some((*element, scroll.take_for_input()?)))
            .collect();
        self.scroll_delta(viewport, containers)
    }

    fn scroll_delta(
        &self,
        viewport: Option<PointF>,
        mut containers: Vec<(ElementId, PointF)>,
    ) -> Option<CompositorScrollDelta> {
        if viewport.is_none() && containers.is_empty() {
            return None;
        }
        containers.sort_by_key(|(element, _)| *element);
        Some(CompositorScrollDelta {
            document: self
                .drawn_document
                .expect("only a drawn document is scrolled"),
            viewport: viewport.unwrap_or_default(),
            containers,
        })
    }

    /// Whether the main thread has scrolling still to get, or to confirm
    /// with a commit: a reason for a main frame of its own.
    pub(crate) fn scroll_needs_main_frame(&self) -> bool {
        self.scroll.needs_main_frame()
            || self
                .container_scrolls
                .values()
                .any(SyncedScrollOffset::needs_main_frame)
    }

    #[cfg(test)]
    pub(crate) fn scroll_offset(&self) -> PointF {
        self.scroll.current()
    }

    /// Each container's offset where it is drawn, snapped to `scale`'s
    /// device pixels as paint snaps the ones it paints, so a layer lands
    /// where it would have been painted; and the viewport's, as drawn, for
    /// the sticky boxes that stick in it ([`ElementId::VIEWPORT`]).
    fn drawn_scroll_offsets(&self, scale: f32) -> ScrollOffsets {
        let snap = |v: f32| (v * scale).round() / scale;
        let viewport = self.scroll.current();
        self.scroll_offsets()
            .into_iter()
            .chain([(ElementId::VIEWPORT, viewport)])
            .map(|(element, offset)| (element, PointF::new(snap(offset.x), snap(offset.y))))
            .collect()
    }

    /// Each container's offset: the main thread's, and what the impl side
    /// scrolled since.
    pub(crate) fn scroll_offsets(&self) -> ScrollOffsets {
        self.container_scrolls
            .iter()
            .map(|(element, scroll)| (*element, scroll.current()))
            .collect()
    }

    /// The viewport's offset in `tree`'s physical pixels, snapped to whole
    /// ones so tiles land on the pixel grid.
    fn scroll_offset_in_pixels(&self, tree: &LayerTreeImpl) -> PointF {
        let offset = self.scroll.current();
        let scale = tree.device_scale_factor;
        PointF::new((offset.x * scale).round(), (offset.y * scale).round())
    }

    /// The visible part of the document, in its physical pixels.
    fn visible_rect(&self, tree: &LayerTreeImpl) -> RectF {
        let size = tree.viewport_size.to_f32();
        let offset = self.scroll_offset_in_pixels(tree);
        RectF::new(offset.x, offset.y, size.width, size.height)
    }

    /// Where `layer`'s content starts in the document, with the containers
    /// at `offsets`, in `tree`'s physical pixels, snapped to whole ones so
    /// its tiles land on the pixel grid.
    fn layer_origin_in_pixels(
        tree: &LayerTreeImpl,
        offsets: &ScrollOffsets,
        properties: &LayerProperties,
        scroll: PointF,
    ) -> PointF {
        let properties = *properties;
        let at = tree.property_trees.position(properties.transform, offsets);
        let scale = tree.device_scale_factor;
        // A layer fixed to the viewport stays where it is drawn: the
        // scroll offset the draw takes off again goes back in here (RFC
        // 0013's decision 16).
        let fixed = if properties.fixed {
            scroll
        } else {
            PointF::new(0.0, 0.0)
        };
        PointF::new(
            ((at.x + properties.origin.x) * scale).round() + fixed.x,
            ((at.y + properties.origin.y) * scale).round() + fixed.y,
        )
    }

    /// What clips `layer`, with the containers at `offsets`, in the
    /// document's physical pixels; `None` if nothing does.
    fn layer_clip_in_pixels(
        tree: &LayerTreeImpl,
        offsets: &ScrollOffsets,
        properties: &LayerProperties,
    ) -> Option<RectF> {
        tree.property_trees
            .clip_rect(properties.clip, offsets)
            .map(|clip| clip.scale(tree.device_scale_factor))
    }

    /// `TileManager::PrepareTiles`: raster each tile near the viewport that
    /// has no resource yet, and let go of the ones far from it. "Near" is
    /// the interest rect: the visible rect grown by a viewport on each
    /// side, so a scroll finds its tiles ready. The pending tree's, if
    /// there is one, so it can be activated; else the active tree's. Runs
    /// inline, on whichever thread the impl side is on.
    pub(crate) fn prepare_tiles(&mut self) {
        let Some(tree) = self.pending_tree.as_ref().or(self.active_tree.as_ref()) else {
            return;
        };
        let visible = self.visible_rect(tree);
        let offsets = self.drawn_scroll_offsets(tree.device_scale_factor);
        let scroll = self.scroll_offset_in_pixels(tree);
        self.tiles_prepared_for = Some((PointF::new(visible.x, visible.y), self.scroll_offsets()));
        let interest = visible.inset(-visible.width.max(visible.height));
        let Some(tree) = self.pending_tree.as_mut().or(self.active_tree.as_mut()) else {
            return;
        };
        let _span = trace_event::trace_event!("TileManager::PrepareTiles");
        let origins: Vec<PointF> = tree
            .layers
            .iter()
            .map(|layer| Self::layer_origin_in_pixels(tree, &offsets, &layer.properties(), scroll))
            .collect();
        let mut prepared = TilesPrepared::default();
        // The tiles to raster, in the order they are drawn: their resources
        // are numbered in that order, however many threads raster them.
        let mut jobs = Vec::new();
        for (index, (layer, origin)) in tree.layers.iter_mut().zip(origins).enumerate() {
            let layer = match layer {
                LayerImpl::Picture(layer) => layer,
                // A snapshot not on the GPU yet: put there (once for each),
                // or kept as pixels where there is no GPU. A canvas drawn on
                // another thread is waited for (Viz waits on the resource's
                // sync token); one whose pixels never came has no resource,
                // and isn't drawn.
                LayerImpl::Texture(layer) => {
                    if layer.resource.is_none() && layer.layer.snapshot.wait_ready() {
                        let snapshot = &layer.layer.snapshot;
                        let size = snapshot
                            .decoded
                            .expect("a texture layer's snapshot is decoded");
                        let id = ResourceId(self.resource_ids.get_next());
                        let shared_image = self
                            .settings
                            .shared_image_interface
                            .as_ref()
                            .and_then(|gpu| gpu.create_shared_image(size, snapshot.as_bytes()));
                        let pixels: Arc<[u8]> = match shared_image {
                            Some(_) => Arc::from([]),
                            None => Arc::from(snapshot.as_bytes()),
                        };
                        layer.resource = Some(TransferableResource {
                            id,
                            size,
                            format: ResourceFormat::Rgba8Premul,
                            pixels,
                            shared_image,
                        });
                    }
                    continue;
                }
            };
            // The interest rect in the layer's own space.
            let interest = interest.offset(-origin.x, -origin.y);
            let clear_color = if layer.layer.contents_opaque {
                layer.layer.background_color
            } else {
                Color::TRANSPARENT
            };
            let contents_scale = layer.tiling.contents_scale;
            // Only the tiles it touches exist: the others go, and those
            // missing are made.
            let (rows, columns) = layer.tiling.tiles_in(interest);
            layer
                .tiling
                .tiles
                .retain(|(row, column), _| rows.contains(row) && columns.contains(column));
            for row in rows {
                for column in columns.clone() {
                    let content_rect = layer.tiling.tile_rect((row, column));
                    layer.tiling.tiles.entry((row, column)).or_insert(Tile {
                        content_rect,
                        draw_info: None,
                    });
                }
            }
            for (&key, tile) in layer.tiling.tiles.iter_mut() {
                if tile.draw_info.is_some() {
                    prepared.kept = prepared.kept.saturating_add(1);
                    continue;
                }
                let params = PlaybackParams {
                    content_rect: tile.content_rect,
                    contents_scale,
                    clear_color,
                };
                let list = &layer.layer.display_list;
                let ops = raster::ops_for(list, &params);
                // Of one color, if its ops tell (none drawing in it, or an
                // opaque fill over it): what rastering it would come to.
                let predicted = list.color_if_solid_in_rect(
                    &ops,
                    tile.content_rect,
                    contents_scale,
                    clear_color,
                );
                if let Some(color) = predicted {
                    prepared.skipped = prepared.skipped.saturating_add(1);
                    tile.draw_info = Some(TileDrawInfo::SolidColor(color));
                    // Debug builds raster it anyway, to check.
                    if !cfg!(debug_assertions) {
                        continue;
                    }
                }
                jobs.push(RasterJob {
                    layer: index,
                    tile: key,
                    list: list.clone(),
                    params,
                    ops,
                    expect: predicted,
                });
            }
        }

        // Raster, on the pool's threads if there are tiles for more than
        // one; reading each tile's pixels for one color there too.
        let threads = self.settings.num_raster_threads;
        if jobs.len() > 1 && threads > 1 && self.raster_pool.is_none() {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "a thread's index, below the number of threads asked for"
            )]
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(threads)
                .thread_name(|i| format!("CompositorTileWorker{}", i + 1))
                .build();
            match pool {
                Ok(pool) => self.raster_pool = Some(pool),
                Err(e) => {
                    tracing::warn!("no raster threads, rastering on the compositor's: {e}");
                    self.settings.num_raster_threads = 1;
                }
            }
        }
        let cache = &self.raster_cache;
        let gpu = self.settings.shared_image_interface.as_deref();
        // Done with each tile where it was rastered: its pixels go to the
        // GPU, or into what is kept, right away, not all of a frame's at
        // once on this thread.
        let raster = |job: &RasterJob| {
            TILE_PIXELS.with_borrow_mut(|buffer| {
                let size = raster::playback_into(&job.list, &job.ops, &job.params, cache, buffer);
                let drawn = &buffer[..];
                if let Some(color) = TileDrawInfo::solid_color(drawn) {
                    return Rastered::Solid(color);
                }
                // On the GPU, if there is one to put it on; else the pixels
                // are kept, copied out of the buffer.
                let shared_image = gpu.and_then(|gpu| gpu.create_shared_image(size, drawn));
                let pixels: Arc<[u8]> = match shared_image {
                    Some(_) => Arc::from([]),
                    None => Arc::from(drawn),
                };
                Rastered::Pixels {
                    size,
                    pixels,
                    shared_image,
                }
            })
        };
        let rastered: Vec<Rastered> = match &self.raster_pool {
            Some(pool) if jobs.len() > 1 => {
                use rayon::prelude::*;
                pool.install(|| jobs.par_iter().map(raster).collect())
            }
            _ => jobs.iter().map(raster).collect(),
        };

        let Some(tree) = self.pending_tree.as_mut().or(self.active_tree.as_mut()) else {
            return;
        };
        for (job, result) in jobs.into_iter().zip(rastered) {
            if let Some(color) = job.expect {
                let came = match &result {
                    Rastered::Solid(color) => Some(*color),
                    Rastered::Pixels { .. } => None,
                };
                debug_assert_eq!(
                    came,
                    Some(color),
                    "the color predicted for the tile at {:?}",
                    job.params.content_rect
                );
                continue;
            }
            let LayerImpl::Picture(layer) = &mut tree.layers[job.layer] else {
                unreachable!("only picture layers' tiles are rastered");
            };
            let tile = layer
                .tiling
                .tiles
                .get_mut(&job.tile)
                .expect("a tile rastered is there");
            prepared.rastered = prepared.rastered.saturating_add(1);
            let (size, pixels, shared_image) = match result {
                Rastered::Solid(color) => {
                    prepared.solid = prepared.solid.saturating_add(1);
                    tile.draw_info = Some(TileDrawInfo::SolidColor(color));
                    continue;
                }
                Rastered::Pixels {
                    size,
                    pixels,
                    shared_image,
                } => (size, pixels, shared_image),
            };
            // Numbered here, in the order tiles are drawn.
            let id = ResourceId(self.resource_ids.get_next());
            tile.draw_info = Some(TileDrawInfo::Resource(TransferableResource {
                id,
                size,
                format: ResourceFormat::Rgba8Premul,
                pixels,
                shared_image,
            }));
        }
        if prepared.rastered > 0 || prepared.skipped > 0 {
            tracing::debug!(
                rastered = prepared.rastered,
                solid = prepared.solid,
                skipped = prepared.skipped,
                kept = prepared.kept,
                "prepared tiles"
            );
        }
        self.tiles_prepared = prepared;
    }

    /// Prepares the active tree's tiles again if the impl side scrolled
    /// the viewport or a container since they were last prepared. Unlike cc, which prepares after
    /// drawing and draws a checkerboard where nothing is rastered yet,
    /// raster is synchronous here: do it before drawing, so what scrolled
    /// into view is there.
    pub(crate) fn prepare_tiles_if_scrolled(&mut self) {
        let Some(tree) = &self.active_tree else {
            return;
        };
        let now = (self.scroll_offset_in_pixels(tree), self.scroll_offsets());
        if self.tiles_prepared_for.as_ref() != Some(&now) {
            self.prepare_tiles();
        }
    }

    /// The document of the last frame submitted, if any was.
    pub(crate) fn drawn_document(&self) -> Option<DocumentId> {
        self.drawn_document
    }

    pub(crate) fn frame_sink(&mut self) -> &mut dyn LayerTreeFrameSink {
        self.frame_sink.as_mut()
    }

    /// `LayerTreeHostImpl::DrawLayers` + `SubmitCompositorFrame`, from the
    /// active tree. Returns the submitted frame's token, or `None` if there
    /// was nothing to draw (no tree, or an empty viewport). Either way the
    /// swap promises waiting for a frame get that answer.
    pub(crate) fn draw_and_submit(&mut self) -> Option<u64> {
        let token = self.generate_compositor_frame().map(|frame| {
            let token = frame.metadata.frame_token;
            let tree = self
                .active_tree
                .as_ref()
                .expect("a frame comes from the active tree");
            let local_surface_id = tree.local_surface_id;
            self.drawn_document = Some(tree.document_id);
            self.frame_sink
                .submit_compositor_frame(local_surface_id, frame);
            token
        });
        for promise in self.swap_promises.drain(..) {
            promise.fulfill(token);
        }
        token
    }

    fn generate_compositor_frame(&mut self) -> Option<CompositorFrame> {
        let tree = self.active_tree.as_ref()?;
        if tree.viewport_size.is_empty() {
            return None;
        }
        let _span = trace_event::trace_event!("LayerTreeHostImpl::DrawLayers");
        let viewport = RectF::from_size(tree.viewport_size.to_f32());
        let visible = self.visible_rect(tree);
        let scroll = self.scroll_offset_in_pixels(tree);
        let offsets = self.drawn_scroll_offsets(tree.device_scale_factor);
        let mut quads = Vec::new();
        let mut resources = Vec::new();
        // The picture layers drawn over the surface layers, front to back.
        let mut over = Vec::new();
        // `SurfaceLayerImpl::AppendQuads`: front to back, so the last
        // surface layer first, all of them over the root layer.
        let scale = tree.device_scale_factor;
        for layer in tree.surface_layers.iter().rev() {
            quads.push(DrawQuad {
                rect: RectF::new(
                    (layer.position.x * scale).round(),
                    (layer.position.y * scale).round(),
                    (layer.bounds.width * scale).round(),
                    (layer.bounds.height * scale).round(),
                ),
                shared_quad_state: 0,
                material: Material::Surface {
                    surface_id: layer.surface_id,
                    default_background_color: layer.background_color,
                },
            });
        }
        // `PictureLayerImpl::AppendQuads`, front to back too: the last
        // layer first. Each layer's content space is the target space moved
        // to where the layer is, less the viewport's scroll offset; the
        // clips around a layer are its quads' shared state.
        let mut shared_quad_states = vec![SharedQuadState::default()];
        for layer in tree.layers.iter().rev() {
            let properties = layer.properties();
            let quads = if properties.over_surfaces {
                &mut over
            } else {
                &mut quads
            };
            let origin = Self::layer_origin_in_pixels(tree, &offsets, &properties, scroll);
            let at = PointF::new(origin.x - scroll.x, origin.y - scroll.y);
            let state = match Self::layer_clip_in_pixels(tree, &offsets, &properties) {
                Some(clip) => {
                    // A rounded one among the clips: Viz cuts the quads to
                    // its curves too.
                    let rounded = tree
                        .property_trees
                        .rounded_clip(properties.clip, &offsets)
                        .map(|r| {
                            let s = tree.device_scale_factor;
                            RRectF {
                                rect: r.rect.scale(s).offset(-scroll.x, -scroll.y),
                                radii: r
                                    .radii
                                    .map(|radius| SizeF::new(radius.width * s, radius.height * s)),
                            }
                        });
                    let index = shared_quad_states.len();
                    shared_quad_states.push(SharedQuadState {
                        clip_rect: Some(clip.offset(-scroll.x, -scroll.y)),
                        rounded_clip: rounded,
                        ..SharedQuadState::default()
                    });
                    index
                }
                None => 0,
            };
            let visible = visible.offset(-origin.x, -origin.y);
            let layer = match layer {
                LayerImpl::Picture(layer) => layer,
                // `TextureLayerImpl::AppendQuads`: one quad, the snapshot
                // scaled bilinearly into the layer's bounds.
                LayerImpl::Texture(layer) => {
                    let Some(resource) = &layer.resource else {
                        continue;
                    };
                    let bounds = layer.layer.bounds;
                    let rect = RectF::new(
                        at.x,
                        at.y,
                        (bounds.width * scale).round(),
                        (bounds.height * scale).round(),
                    );
                    if rect.offset(-at.x, -at.y).intersect(&visible).is_empty() {
                        continue;
                    }
                    resources.push(resource.clone());
                    quads.push(DrawQuad {
                        rect,
                        shared_quad_state: state,
                        material: Material::Texture {
                            resource_id: resource.id,
                            tex_coord_rect: RectF::from_size(resource.size.to_f32()),
                            nearest_neighbor: false,
                        },
                    });
                    continue;
                }
            };
            for tile in layer.tiling.tiles.values() {
                let Some(draw_info) = &tile.draw_info else {
                    continue;
                };
                if tile.content_rect.intersect(&visible).is_empty() {
                    continue;
                }
                let r = tile.content_rect;
                let material = match draw_info {
                    TileDrawInfo::Resource(resource) => {
                        resources.push(resource.clone());
                        // Drawn 1:1: nearest keeps them pixel exact.
                        Material::Texture {
                            resource_id: resource.id,
                            tex_coord_rect: RectF::from_size(resource.size.to_f32()),
                            nearest_neighbor: true,
                        }
                    }
                    // Nothing to draw.
                    TileDrawInfo::SolidColor(color) if color.a == 0 => continue,
                    TileDrawInfo::SolidColor(color) => Material::SolidColor { color: *color },
                };
                quads.push(DrawQuad {
                    rect: RectF::new(r.x + at.x, r.y + at.y, r.width, r.height),
                    shared_quad_state: state,
                    material,
                });
            }
        }
        over.append(&mut quads);
        let quads = over;
        // What changed since the last frame drawn, where it is drawn now;
        // everything if anything moved.
        let drawn_at = (scroll, offsets);
        let damage = if self.drawn_at.as_ref() == Some(&drawn_at) {
            self.damage
        } else {
            Damage::Full
        };
        let damage_rect = match damage {
            Damage::None => RectF::default(),
            Damage::Rect(rect) => rect
                .offset(-scroll.x, -scroll.y)
                .enclosing()
                .intersect(&viewport),
            Damage::Full => viewport,
        };
        self.damage = Damage::None;
        self.drawn_at = Some(drawn_at);
        let token = self.frame_tokens.get_next();
        Some(CompositorFrame {
            metadata: CompositorFrameMetadata {
                device_scale_factor: tree.device_scale_factor,
                frame_token: token,
                root_background_color: tree.background_color,
            },
            resource_list: resources,
            render_pass_list: vec![CompositorRenderPass {
                id: RenderPassId(1),
                output_rect: viewport,
                damage_rect,
                shared_quad_state_list: shared_quad_states,
                quad_list: quads,
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::DocumentId;
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
    use gfx::{Color, RectF, Size, SizeF};
    use protocol::viz::{CompositorFrame, LocalSurfaceId, Material, SharedImageInterface};

    use crate::{
        ClipNode, ClipNodeId, ElementId, LayerId, LayerProperties, LayerTreeHost,
        LayerTreeSettings, PictureLayer, PropertyTrees, ScrollNode, ScrollOffsets, ScrollTreeNode,
        SingleThreadProxy, TransformNode, TransformNodeId,
    };

    use super::{LayerImpl, TileDrawInfo};
    use crate::fake_gpu::FakeGpu;

    fn host() -> (
        LayerTreeHost<SingleThreadProxy>,
        Arc<Mutex<Vec<CompositorFrame>>>,
    ) {
        host_with(None)
    }

    fn host_with(
        gpu: Option<Arc<dyn SharedImageInterface>>,
    ) -> (
        LayerTreeHost<SingleThreadProxy>,
        Arc<Mutex<Vec<CompositorFrame>>>,
    ) {
        host_with_threads(gpu, 1)
    }

    fn host_with_threads(
        gpu: Option<Arc<dyn SharedImageInterface>>,
        num_raster_threads: usize,
    ) -> (
        LayerTreeHost<SingleThreadProxy>,
        Arc<Mutex<Vec<CompositorFrame>>>,
    ) {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let sink = frames.clone();
        let host = LayerTreeHost::new_single_threaded(
            LayerTreeSettings {
                default_tile_size: 64,
                shared_image_interface: gpu,
                num_raster_threads,
                ..Default::default()
            },
            Box::new(move |_id: LocalSurfaceId, f: CompositorFrame| sink.lock().unwrap().push(f)),
        );
        (host, frames)
    }

    #[test]
    fn tiles_cover_visible_layer() {
        let (mut host, frames) = host();
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 50.0, 50.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        dots(&mut list, 100.0, 300.0);
        list.finalize();
        host.set_root_layer(PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 300.0),
            display_list: Arc::new(list),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        });
        // 2x scale: layer is 200x600 physical; viewport 150x100 physical.
        host.set_viewport_rect_and_scale(Size::new(150, 100), 2.0, LocalSurfaceId::default());
        assert_eq!(host.composite(), Some(1));

        let frames = frames.lock().unwrap();
        let frame = &frames[0];
        let pass = frame.root_pass().unwrap();
        // Visible tiles: columns 0..3 (0,64,128) x rows 0..2 (0,64) = 6.
        assert_eq!(pass.quad_list.len(), 6);
        assert_eq!(frame.resource_list.len(), 6);
        assert_eq!(frame.size_in_pixels(), Size::new(150, 100));
        // Tiles are full-size except at the layer edge (x = 192, not visible).
        let last_col = pass.quad_list.iter().find(|q| q.rect.x == 128.0).unwrap();
        assert_eq!(last_col.rect.width, 64.0);
        let Material::Texture { resource_id, .. } = pass.quad_list[0].material else {
            panic!("expected texture quad");
        };
        let res = frame
            .resource_list
            .iter()
            .find(|r| r.id == resource_id)
            .unwrap();
        // Top-left tile: red rect (0..100 physical) covers it entirely.
        assert_eq!(&res.pixels[0..4], &[255, 0, 0, 255]);
    }

    #[test]
    fn a_tile_is_one_color_if_every_pixel_is_opaque_and_the_same_or_none_is_there() {
        let solid = |pixels: &[[u8; 4]]| TileDrawInfo::solid_color(pixels.as_flattened());
        let red = [255, 0, 0, 255];
        assert_eq!(solid(&[red, red, red]), Some(Color::rgb(255, 0, 0)));
        assert_eq!(solid(&[[0; 4], [0; 4]]), Some(Color::TRANSPARENT));
        assert_eq!(solid(&[red, red, [0, 0, 255, 255]]), None, "two colors");
        assert_eq!(solid(&[red, [0; 4]]), None, "partly transparent");
        // Translucent: unpremultiplied, it might not draw back the same.
        let translucent = [64, 0, 0, 128];
        assert_eq!(solid(&[translucent, translucent]), None);
        assert_eq!(solid(&[]), None);
    }

    /// Tiles of one color keep no pixels and are drawn as solid color
    /// quads; transparent ones aren't drawn; the others are rastered.
    #[test]
    fn tiles_of_one_color_are_drawn_as_solid_color_quads() {
        let (mut host, frames) = host();
        // 64px tiles at 1x: the red rect fills the first column, the dot
        // makes the tile at 64,0 not of one color, and the rest of the
        // translucent layer is empty.
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 64.0, 128.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(70.0, 10.0, 1.0, 1.0),
            flags: PaintFlags::fill(Color::BLACK),
        });
        list.finalize();
        host.set_root_layer(PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(128.0, 128.0),
            display_list: Arc::new(list),
            contents_opaque: false,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        });
        host.set_viewport_rect_and_scale(Size::new(128, 128), 1.0, LocalSurfaceId::default());
        host.composite();
        let frames = frames.lock().unwrap();
        let pass = frames[0].root_pass().unwrap();
        let materials: Vec<(f32, f32, Material)> = pass
            .quad_list
            .iter()
            .map(|q| (q.rect.x, q.rect.y, q.material))
            .collect();
        let red = Material::SolidColor {
            color: Color::rgb(255, 0, 0),
        };
        assert!(materials.contains(&(0.0, 0.0, red)), "{materials:?}");
        assert!(materials.contains(&(0.0, 64.0, red)), "{materials:?}");
        // The dotted tile, rastered; the empty one at 64,64 not drawn.
        assert_eq!(materials.len(), 3, "{materials:?}");
        assert_eq!(frames[0].resource_list.len(), 1);
        assert!(matches!(
            materials.iter().find(|m| (m.0, m.1) == (64.0, 0.0)),
            Some((_, _, Material::Texture { .. }))
        ));
        assert_eq!(host.host_impl().resident_tiles(), 4, "all four kept");
    }

    /// A tile of one color is kept like another, until something it is in
    /// changes.
    #[test]
    fn a_tile_of_one_color_is_kept_until_it_changes() {
        let (mut host, _frames) = host();
        host.set_viewport_rect_and_scale(Size::new(128, 128), 1.0, LocalSurfaceId::default());
        let layer = |invalidation: Option<&[RectF]>| {
            let mut list = DisplayItemList::new();
            // A rounded rect with no rounding: of one color once rastered,
            // which its ops don't tell.
            list.push_op(PaintOp::DrawRRect {
                rrect: gfx::RRectF::uniform(RectF::new(0.0, 0.0, 128.0, 128.0), 0.0),
                flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
            });
            list.finalize();
            PictureLayer {
                id: LayerId(1),
                bounds: SizeF::new(128.0, 128.0),
                display_list: Arc::new(list),
                contents_opaque: true,
                background_color: Color::WHITE,
                invalidation: invalidation.map(Arc::from),
                properties: Default::default(),
            }
        };
        draw(&mut host, layer(None));
        let prepared = host.host_impl().tiles_prepared();
        assert_eq!((prepared.rastered, prepared.solid), (4, 4));
        // Another display list, changed in the first tile only.
        draw(&mut host, layer(Some(&[RectF::new(10.0, 10.0, 5.0, 5.0)])));
        let prepared = host.host_impl().tiles_prepared();
        assert_eq!((prepared.rastered, prepared.kept), (1, 3));
    }

    /// A tile whose ops tell its color isn't rastered: one no op draws in
    /// comes to the color its layer is cleared to, one an opaque fill
    /// covers to the fill's, as rastering it would have; not if the clear
    /// color is translucent, or the ops don't tell.
    #[test]
    fn a_tile_outside_the_content_is_not_rastered() {
        // A 128x128 layer at 1.5x: 3x3 tiles of 64px. A red rect from
        // 120,15 to 127.5,150: in the middle column, and within a pixel of
        // the last one; the first column blank.
        let column = [RectF::new(80.0, 10.0, 5.0, 90.0)];
        let layer = |rects: &[RectF], contents_opaque, background_color, analyzed: bool| {
            let mut list = DisplayItemList::new();
            if !analyzed {
                // A clip to a path, which tells the ops nothing: the tiles
                // that find ops are rastered. It is all around the layer,
                // so it cuts nothing.
                list.push_op(PaintOp::Save);
                let around = [
                    gfx::PointF::new(-1000.0, -1000.0),
                    gfx::PointF::new(1000.0, -1000.0),
                    gfx::PointF::new(1000.0, 1000.0),
                    gfx::PointF::new(-1000.0, 1000.0),
                ];
                list.push_op(PaintOp::ClipPath {
                    path: gfx::Path::polygons([around]),
                    anti_alias: true,
                });
            }
            for &rect in rects {
                list.push_op(PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
                });
            }
            if !analyzed {
                list.push_op(PaintOp::Restore);
            }
            list.finalize();
            PictureLayer {
                id: LayerId(1),
                bounds: SizeF::new(128.0, 128.0),
                display_list: Arc::new(list),
                contents_opaque,
                background_color,
                invalidation: None,
                properties: Default::default(),
            }
        };
        // What was prepared, and what each quad drew: its color, or its
        // tile's pixels.
        let run = |layer: PictureLayer| {
            let (mut host, frames) = host();
            host.set_viewport_rect_and_scale(Size::new(192, 192), 1.5, LocalSurfaceId::default());
            draw(&mut host, layer);
            let prepared = host.host_impl().tiles_prepared();
            let frames = frames.lock().unwrap();
            let frame = &frames[0];
            let mut drawn: Vec<(f32, f32, Result<Color, Vec<u8>>)> = frame
                .root_pass()
                .unwrap()
                .quad_list
                .iter()
                .map(|quad| {
                    let what = match quad.material {
                        Material::SolidColor { color } => Ok(color),
                        Material::Texture { resource_id, .. } => Err(frame
                            .resource_list
                            .iter()
                            .find(|r| r.id == resource_id)
                            .unwrap()
                            .pixels
                            .to_vec()),
                        other => panic!("{other:?}"),
                    };
                    (quad.rect.x, quad.rect.y, what)
                })
                .collect();
            drawn.sort_by(|a, b| (a.1, a.0).partial_cmp(&(b.1, b.0)).unwrap());
            (prepared, drawn)
        };
        let translucent = Color::rgba(255, 255, 255, 128);
        for (contents_opaque, background) in [
            (false, Color::WHITE),
            (true, Color::WHITE),
            (true, translucent),
        ] {
            let (prepared, drawn) = run(layer(&column, contents_opaque, background, true));
            let (all, rastered) = run(layer(&column, contents_opaque, background, false));
            // There, the two columns that find the rect (the last within a
            // pixel of it) are rastered.
            let found = if background == translucent {
                (9, 0)
            } else {
                (6, 3)
            };
            assert_eq!((all.rastered, all.skipped), found);
            assert_eq!(
                drawn, rastered,
                "as if rastered: {contents_opaque} {background:?}"
            );
            if background == translucent {
                assert_eq!((prepared.rastered, prepared.skipped), (9, 0));
                continue;
            }
            // The rect's own edges miss the last column.
            assert_eq!(
                (prepared.rastered, prepared.skipped),
                (3, 6),
                "{contents_opaque} {background:?}"
            );
            // The first column, and the last: not drawn if transparent,
            // white if not.
            let blank = drawn
                .iter()
                .filter(|(_, _, what)| what == &Ok(Color::WHITE))
                .count();
            assert_eq!(blank, if contents_opaque { 6 } else { 0 });
        }
        // Two rects, in the tiles at 0,0 and 128,128: the tiles between
        // them, though inside the content's bounds, aren't rastered either.
        let corners = [
            RectF::new(10.0, 10.0, 10.0, 10.0),
            RectF::new(100.0, 100.0, 10.0, 10.0),
        ];
        let (prepared, drawn) = run(layer(&corners, true, Color::WHITE, true));
        assert_eq!((prepared.rastered, prepared.skipped), (2, 7));
        assert_eq!(drawn, run(layer(&corners, true, Color::WHITE, false)).1);
        // An opaque rect over all of them: each of its color, unrastered.
        let all = [RectF::new(0.0, 0.0, 128.0, 128.0)];
        let (prepared, drawn) = run(layer(&all, true, Color::WHITE, true));
        assert_eq!((prepared.rastered, prepared.skipped), (0, 9));
        let red = Ok(Color::rgb(255, 0, 0));
        assert!(drawn.iter().all(|(_, _, what)| what == &red), "{drawn:?}");
        let (forced, rastered) = run(layer(&all, true, Color::WHITE, false));
        assert_eq!((forced.rastered, forced.skipped), (9, 0));
        assert_eq!(drawn, rastered);
    }

    /// Tiles rastered on raster threads come out as on the compositor's:
    /// the same quads, numbered alike, of the same pixels; each tile has
    /// what it is drawn with. The pool is made only when there are tiles
    /// for more than one thread.
    #[test]
    fn raster_threads_raster_as_the_compositor_thread_does() {
        // 320x320 at 1x: 5x5 tiles of 64px. A dot in every other tile,
        // a band of one color rastered (a rounded rect's), the rest blank.
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRRect {
            rrect: gfx::RRectF::uniform(RectF::new(0.0, 128.0, 320.0, 64.0), 0.0),
            flags: PaintFlags::fill(Color::rgb(0, 0, 255)),
        });
        for i in (0..25).step_by(2) {
            let (x, y) = ((i % 5) as f32 * 64.0, (i / 5) as f32 * 64.0);
            list.push_op(PaintOp::DrawRect {
                rect: RectF::new(x + 7.0, y + 9.0, 3.0, 3.0),
                flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
            });
        }
        list.finalize();
        let list = Arc::new(list);
        let run = |threads| {
            let (mut host, frames) = host_with_threads(None, threads);
            host.set_viewport_rect_and_scale(Size::new(320, 320), 1.0, LocalSurfaceId::default());
            draw(
                &mut host,
                PictureLayer {
                    id: LayerId(1),
                    bounds: SizeF::new(320.0, 320.0),
                    display_list: list.clone(),
                    contents_opaque: true,
                    background_color: Color::WHITE,
                    invalidation: None,
                    properties: Default::default(),
                },
            );
            let prepared = host.host_impl().tiles_prepared();
            let pooled = host.host_impl().raster_pool.is_some();
            let tiles = host
                .host_impl()
                .active_tree
                .as_ref()
                .unwrap()
                .layers
                .iter()
                .map(|layer| match layer {
                    LayerImpl::Picture(layer) => layer
                        .tiling
                        .tiles
                        .values()
                        .filter(|t| t.draw_info.is_none())
                        .count(),
                    LayerImpl::Texture(_) => 0,
                })
                .sum::<usize>();
            assert_eq!(tiles, 0, "every tile has what it is drawn with");
            let frames = frames.lock().unwrap();
            let frame = &frames[0];
            let quads: Vec<String> = frame
                .root_pass()
                .unwrap()
                .quad_list
                .iter()
                .map(|q| format!("{:?} {:?}", q.rect, q.material))
                .collect();
            let resources: Vec<(protocol::viz::ResourceId, Vec<u8>)> = frame
                .resource_list
                .iter()
                .map(|r| (r.id, r.pixels.to_vec()))
                .collect();
            (prepared, pooled, quads, resources)
        };
        let (one, pooled, quads, resources) = run(1);
        assert!(!pooled);
        assert_eq!((one.rastered, one.solid), (15, 2), "{one:?}");
        let (four, pooled, quads4, resources4) = run(4);
        assert!(pooled);
        assert_eq!(one, four);
        assert_eq!(quads, quads4);
        assert_eq!(resources, resources4);
        assert_eq!(resources.len(), 13);
    }

    /// `width` x `height` filled with `color`, with dots on it.
    fn solid(width: f32, height: f32, color: Color) -> Arc<DisplayItemList> {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, width, height),
            flags: PaintFlags::fill(color),
        });
        dots(&mut list, width, height);
        list.finalize();
        Arc::new(list)
    }

    /// A dot a pixel into every 32 CSS px over `width` x `height`: tiles
    /// start on those lines, so none of what is filled under it is of one
    /// color (its top left pixel still is the fill): tiles are rastered and
    /// kept as resources, not drawn as solid color quads.
    fn dots(list: &mut DisplayItemList, width: f32, height: f32) {
        for y in (0..(height / 32.0).ceil() as usize).map(|i| i as f32 * 32.0 + 1.0) {
            for x in (0..(width / 32.0).ceil() as usize).map(|i| i as f32 * 32.0 + 1.0) {
                list.push_op(PaintOp::DrawRect {
                    rect: RectF::new(x, y, 1.0, 1.0),
                    flags: PaintFlags::fill(Color::BLACK),
                });
            }
        }
    }

    /// A 200x400 document, and over it a scroll container's layer: element
    /// 7's 100x300 of content, in a 100x100 padding box at (0, 50).
    fn with_a_container_layer(host: &mut LayerTreeHost<SingleThreadProxy>) {
        with_a_rounded_container_layer(host, Default::default(), gfx::PointF::new(0.0, 0.0));
    }

    /// The same, the container's clip's corners rounded by `radii`, its
    /// clip node in a space at `clip_at` (its rect's origin is still the
    /// document's).
    fn with_a_rounded_container_layer(
        host: &mut LayerTreeHost<SingleThreadProxy>,
        radii: [SizeF; 4],
        clip_at: gfx::PointF,
    ) {
        let mut trees = PropertyTrees::default();
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(0)),
            offset: gfx::PointF::new(0.0, 0.0),
            scroll: Some(ElementId(7)),
            sticky: None,
        });
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(0)),
            offset: clip_at,
            scroll: None,
            sticky: None,
        });
        trees.clip.push(ClipNode {
            parent: Some(ClipNodeId(0)),
            transform: TransformNodeId(2),
            rect: Some(RectF::new(0.0, 50.0, 100.0, 100.0)),
            radii,
        });
        trees.scroll.push(ScrollTreeNode {
            parent: None,
            element: ElementId(7),
            scroller: ScrollNode {
                container_bounds: SizeF::new(100.0, 100.0),
                bounds: SizeF::new(100.0, 300.0),
                user_scrollable_horizontal: true,
                user_scrollable_vertical: true,
            },
            transform: TransformNodeId(0),
            border_box: RectF::new(0.0, 50.0, 100.0, 100.0),
            clip: ClipNodeId(0),
            scroll_translation: TransformNodeId(1),
            main_thread_scroll_hit_test_region: Arc::from([]),
            wheel_event_handler_region: Arc::from([]),
        });
        let document = PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(200.0, 400.0),
            display_list: solid(200.0, 400.0, Color::rgb(255, 0, 0)),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        };
        let container = PictureLayer {
            id: LayerId(2),
            bounds: SizeF::new(100.0, 300.0),
            display_list: solid(100.0, 300.0, Color::rgb(0, 255, 0)),
            contents_opaque: false,
            background_color: Color::WHITE,
            invalidation: None,
            properties: LayerProperties {
                transform: TransformNodeId(1),
                clip: ClipNodeId(1),
                origin: gfx::PointF::new(0.0, 50.0),
                fixed: false,
                over_surfaces: false,
            },
        };
        host.set_layers(vec![document, container], trees);
        host.set_viewport_rect_and_scale(Size::new(200, 200), 1.0, LocalSurfaceId::default());
    }

    /// The container's quads (the clipped ones) and their clip.
    fn container_quads(frame: &CompositorFrame) -> (Vec<RectF>, Option<RectF>) {
        let pass = frame.root_pass().unwrap();
        let quads: Vec<_> = pass
            .quad_list
            .iter()
            .filter(|q| q.shared_quad_state != 0)
            .collect();
        let clip = quads
            .first()
            .and_then(|q| pass.shared_quad_state_list[q.shared_quad_state].clip_rect);
        (quads.iter().map(|q| q.rect).collect(), clip)
    }

    #[test]
    fn a_container_layer_is_drawn_over_the_document_where_its_nodes_put_it() {
        let (mut host, frames) = host();
        with_a_container_layer(&mut host);
        host.composite();
        let frames = frames.lock().unwrap();
        let pass = frames[0].root_pass().unwrap();
        // Front to back: the container's quads, then the document's.
        let states: Vec<_> = pass
            .quad_list
            .iter()
            .map(|q| q.shared_quad_state != 0)
            .collect();
        let first_document = states.iter().position(|clipped| !clipped).unwrap();
        assert!(first_document > 0, "the container first: {states:?}");
        assert!(
            states[first_document..].iter().all(|clipped| !clipped),
            "{states:?}"
        );
        let (rects, clip) = container_quads(&frames[0]);
        assert_eq!(rects[0], RectF::new(0.0, 50.0, 64.0, 64.0));
        assert_eq!(clip, Some(RectF::new(0.0, 50.0, 100.0, 100.0)));
    }

    /// A container whose clip has rounded corners: its quads carry them for
    /// Viz to cut to, and keep them as it scrolls; a square one's don't.
    #[test]
    fn a_rounded_container_layers_quads_carry_its_corners() {
        let rounded_clip = |frame: &CompositorFrame| {
            let pass = frame.root_pass().unwrap();
            let quad = pass
                .quad_list
                .iter()
                .find(|q| q.shared_quad_state != 0)
                .unwrap();
            pass.shared_quad_state_list[quad.shared_quad_state].rounded_clip
        };
        let (mut rounded_host, frames) = host();
        let radii = [SizeF::new(10.0, 20.0); 4];
        with_a_rounded_container_layer(&mut rounded_host, radii, gfx::PointF::new(0.0, 0.0));
        rounded_host.composite();
        rounded_host.set_scroll_offsets(ScrollOffsets::from([(
            ElementId(7),
            gfx::PointF::new(0.0, 30.0),
        )]));
        rounded_host.composite();
        // The viewport scrolled: the corners move up with the page.
        rounded_host.set_root_scroll_offset(gfx::PointF::new(0.0, 10.0));
        rounded_host.composite();
        let frames = frames.lock().unwrap();
        let at = |x, y| {
            Some(gfx::RRectF {
                rect: RectF::new(x, y, 100.0, 100.0),
                radii,
            })
        };
        assert_eq!(rounded_clip(&frames[0]), at(0.0, 50.0));
        assert_eq!(rounded_clip(&frames[1]), at(0.0, 50.0));
        assert_eq!(rounded_clip(&frames[2]), at(0.0, 40.0));
        drop(frames);
        // Where its clip node is.
        let (mut moved_host, frames) = host();
        with_a_rounded_container_layer(&mut moved_host, radii, gfx::PointF::new(10.0, 5.0));
        moved_host.composite();
        assert_eq!(rounded_clip(&frames.lock().unwrap()[0]), at(10.0, 55.0));
        let (mut square_host, frames) = host();
        with_a_container_layer(&mut square_host);
        square_host.composite();
        assert_eq!(rounded_clip(&frames.lock().unwrap()[0]), None);
    }

    /// The point of a container being a layer: scrolled, it moves, clipped
    /// where it was, and nothing is rastered again.
    #[test]
    fn scrolling_a_container_layer_moves_it_and_rasters_nothing() {
        let (mut host, frames) = host();
        with_a_container_layer(&mut host);
        host.composite();
        host.set_scroll_offsets(ScrollOffsets::from([(
            ElementId(7),
            gfx::PointF::new(0.0, 30.0),
        )]));
        host.composite();
        let frames = frames.lock().unwrap();
        let before: std::collections::HashSet<_> =
            frames[0].resource_list.iter().map(|r| r.id).collect();
        assert!(
            frames[1]
                .resource_list
                .iter()
                .all(|r| before.contains(&r.id)),
            "rastered again"
        );
        let (rects, clip) = container_quads(&frames[1]);
        assert_eq!(rects[0], RectF::new(0.0, 20.0, 64.0, 64.0), "up by 30");
        assert_eq!(
            clip,
            Some(RectF::new(0.0, 50.0, 100.0, 100.0)),
            "clipped where it was"
        );
    }

    /// A container's offset is drawn snapped to device pixels, as paint
    /// snaps it when it paints the container: whole CSS px at 1x, half ones
    /// at 2x.
    #[test]
    fn a_container_layer_moves_by_its_offset_snapped_to_device_pixels() {
        for (scale, top) in [(1.0, 40.0), (2.0, 79.0)] {
            let (mut host, frames) = host();
            with_a_container_layer(&mut host);
            host.set_viewport_rect_and_scale(
                Size::new((200.0 * scale) as u32, (200.0 * scale) as u32),
                scale,
                LocalSurfaceId::default(),
            );
            host.set_scroll_offsets(ScrollOffsets::from([(
                ElementId(7),
                gfx::PointF::new(0.0, 10.25),
            )]));
            host.composite();
            let frames = frames.lock().unwrap();
            let (rects, _) = container_quads(&frames[0]);
            // At 50 less the offset: 39.75, snapped.
            assert_eq!(rects[0].y, top, "at {scale}x");
        }
    }

    // Raster invalidation: a new display list rasters again only the tiles
    // its invalidation touches.

    /// A `width`x`height` document layer of one color, at 1x in a 200x200
    /// viewport, with 64px tiles: rows and columns at 0, 64, 128 and 192.
    fn document_layer(
        id: u32,
        height: f32,
        color: Color,
        invalidation: Option<&[RectF]>,
    ) -> PictureLayer {
        PictureLayer {
            id: LayerId(id),
            bounds: SizeF::new(200.0, height),
            display_list: solid(200.0, height, color),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: invalidation.map(Arc::from),
            properties: Default::default(),
        }
    }

    /// The resources of each frame drawn.
    fn resources(frames: &[CompositorFrame]) -> Vec<Vec<u64>> {
        frames
            .iter()
            .map(|f| f.resource_list.iter().map(|r| r.id.0).collect())
            .collect()
    }

    /// How many of `frame`'s resources `before` doesn't have: rastered for
    /// it.
    fn rastered(before: &[u64], frame: &[u64]) -> usize {
        frame.iter().filter(|id| !before.contains(id)).count()
    }

    fn draw(host: &mut LayerTreeHost<SingleThreadProxy>, layer: PictureLayer) {
        host.set_root_layer(layer);
        host.composite();
    }

    fn viewport(host: &mut LayerTreeHost<SingleThreadProxy>) {
        host.set_viewport_rect_and_scale(Size::new(200, 200), 1.0, LocalSurfaceId::default());
    }

    #[test]
    fn a_new_display_list_rasters_only_the_tiles_its_invalidation_touches() {
        let (mut host, frames) = host();
        viewport(&mut host);
        let green = Color::rgb(0, 255, 0);
        draw(&mut host, document_layer(1, 200.0, green, None));
        // A change in the top left tile, and one across it and the three
        // around the corner at 64, 64.
        let changed = [
            RectF::new(10.0, 10.0, 20.0, 20.0),
            RectF::new(60.0, 60.0, 10.0, 10.0),
        ];
        draw(
            &mut host,
            document_layer(1, 200.0, Color::rgb(255, 0, 0), Some(&changed)),
        );
        // All of it.
        draw(&mut host, document_layer(1, 200.0, green, None));
        let frames = resources(&frames.lock().unwrap());
        assert_eq!(frames[0].len(), 16);
        assert_eq!(rastered(&frames[0], &frames[1]), 4);
        assert_eq!(rastered(&frames[1], &frames[2]), 16);
    }

    /// A layer that grew keeps the tiles that are where they were and as
    /// big; the ones at its old edge are rastered again.
    #[test]
    fn a_layer_that_grew_keeps_its_tiles_but_the_ones_at_its_old_edge() {
        let (mut host, frames) = host();
        viewport(&mut host);
        let green = Color::rgb(0, 255, 0);
        // 160 high: the row at 128 is 32 high.
        draw(&mut host, document_layer(1, 160.0, green, None));
        draw(&mut host, document_layer(1, 300.0, green, Some(&[])));
        let frames = resources(&frames.lock().unwrap());
        // The row at 128 is now 64 high, and the one at 192 is new.
        assert_eq!(rastered(&frames[0], &frames[1]), 4 + 4);
        assert_eq!(frames[1].len(), 16);
    }

    /// What isn't the same layer, or the same document, keeps nothing,
    /// whatever it says changed.
    #[test]
    fn another_layer_or_document_keeps_no_tiles() {
        let green = Color::rgb(0, 255, 0);
        let (mut host, frames) = host();
        viewport(&mut host);
        draw(&mut host, document_layer(1, 200.0, green, None));
        draw(&mut host, document_layer(2, 200.0, green, Some(&[])));
        host.set_document_id(DocumentId(7));
        draw(&mut host, document_layer(2, 200.0, green, Some(&[])));
        let frames = resources(&frames.lock().unwrap());
        assert_eq!(rastered(&frames[0], &frames[1]), 16, "another layer");
        assert_eq!(rastered(&frames[1], &frames[2]), 16, "another document");
    }

    fn damage(frame: &CompositorFrame) -> RectF {
        frame.root_pass().unwrap().damage_rect
    }

    /// The damage rect: what a commit changed, where it is drawn; all of
    /// the frame when anything else changed or moved.
    #[test]
    fn a_frame_is_damaged_where_what_changed_is() {
        let (mut host, frames) = host();
        host.set_viewport_rect_and_scale(Size::new(400, 400), 2.0, LocalSurfaceId::default());
        let green = Color::rgb(0, 255, 0);
        draw(&mut host, document_layer(1, 300.0, green, None));
        let changed = [
            RectF::new(10.25, 10.0, 20.0, 20.0),
            RectF::new(100.0, 50.0, 10.0, 10.0),
        ];
        draw(
            &mut host,
            document_layer(1, 300.0, Color::rgb(255, 0, 0), Some(&changed)),
        );
        // The viewport moved: all of it.
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 10.0));
        host.composite();
        // Nothing changed since.
        host.composite();
        let frames = frames.lock().unwrap();
        assert_eq!(
            damage(&frames[0]),
            RectF::new(0.0, 0.0, 400.0, 400.0),
            "the first"
        );
        assert_eq!(
            damage(&frames[1]),
            RectF::new(20.0, 20.0, 200.0, 100.0),
            "both rects, at 2x, whole pixels around them"
        );
        assert_eq!(damage(&frames[2]), RectF::new(0.0, 0.0, 400.0, 400.0));
        assert_eq!(damage(&frames[3]), RectF::default());
    }

    /// At 2x, the invalidation (CSS px) is scaled to the tiles' physical
    /// pixels: a 20px change at 70 is at 140 to 180, in the tiles at 128.
    #[test]
    fn the_invalidation_is_scaled_to_the_tiles() {
        let (mut host, frames) = host();
        host.set_viewport_rect_and_scale(Size::new(400, 400), 2.0, LocalSurfaceId::default());
        let green = Color::rgb(0, 255, 0);
        draw(&mut host, document_layer(1, 200.0, green, None));
        let changed = [RectF::new(70.0, 70.0, 20.0, 20.0)];
        draw(
            &mut host,
            document_layer(1, 200.0, Color::rgb(255, 0, 0), Some(&changed)),
        );
        let frames = frames.lock().unwrap();
        let ids = resources(&frames);
        assert_eq!(ids[0].len(), 49, "7 by 7 tiles of 64 over 400");
        // Where the one tile rastered again is drawn.
        let pass = frames[1].root_pass().unwrap();
        let new: Vec<RectF> = pass
            .quad_list
            .iter()
            .filter_map(|quad| match quad.material {
                Material::Texture { resource_id, .. } if !ids[0].contains(&resource_id.0) => {
                    Some(quad.rect)
                }
                _ => None,
            })
            .collect();
        assert_eq!(new, [RectF::new(128.0, 128.0, 64.0, 64.0)]);
    }

    /// The document's layer, and over it a 100x100 layer moved by its own
    /// transform node to `x`, of `color`.
    fn two_layers(
        host: &mut LayerTreeHost<SingleThreadProxy>,
        x: f32,
        color: Color,
        invalidation: Option<&[RectF]>,
        document: &Arc<DisplayItemList>,
    ) {
        let mut trees = PropertyTrees::default();
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(0)),
            offset: gfx::PointF::new(x, 0.0),
            scroll: None,
            sticky: None,
        });
        // The same display list each time, with what an earlier paint said
        // changed: a layer the main thread didn't paint again keeps that,
        // and it is no longer to be read.
        let document = PictureLayer {
            display_list: document.clone(),
            invalidation: Some(Arc::from([RectF::new(0.0, 0.0, 200.0, 200.0)])),
            ..document_layer(1, 200.0, Color::WHITE, None)
        };
        let over = PictureLayer {
            id: LayerId(2),
            bounds: SizeF::new(100.0, 100.0),
            display_list: solid(100.0, 100.0, color),
            contents_opaque: false,
            invalidation: invalidation.map(Arc::from),
            properties: LayerProperties {
                transform: TransformNodeId(1),
                ..Default::default()
            },
            ..document_layer(2, 100.0, color, None)
        };
        host.set_layers(vec![document, over], trees);
        host.composite();
    }

    /// Only the layer whose display list changed is damaged, where it is;
    /// a layer moved by its transform node damages all of the frame.
    #[test]
    fn only_what_changed_or_moved_is_damaged() {
        let (mut host, frames) = host();
        viewport(&mut host);
        let document = solid(200.0, 200.0, Color::rgb(0, 0, 255));
        let green = Color::rgb(0, 255, 0);
        two_layers(&mut host, 50.0, green, None, &document);
        let changed = [RectF::new(10.0, 10.0, 5.0, 5.0)];
        two_layers(
            &mut host,
            50.0,
            Color::rgb(255, 0, 0),
            Some(&changed),
            &document,
        );
        two_layers(&mut host, 60.0, Color::rgb(255, 0, 0), Some(&[]), &document);
        let frames = frames.lock().unwrap();
        assert_eq!(damage(&frames[1]), RectF::new(60.0, 10.0, 5.0, 5.0));
        assert_eq!(damage(&frames[2]), RectF::new(0.0, 0.0, 200.0, 200.0));
    }

    fn tall_layer() -> (
        LayerTreeHost<SingleThreadProxy>,
        Arc<Mutex<Vec<CompositorFrame>>>,
    ) {
        tall_layer_with(None)
    }

    fn tall_layer_with(
        gpu: Option<Arc<dyn SharedImageInterface>>,
    ) -> (
        LayerTreeHost<SingleThreadProxy>,
        Arc<Mutex<Vec<CompositorFrame>>>,
    ) {
        let (mut host, frames) = host_with(gpu);
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 100.0, 2000.0),
            flags: PaintFlags::fill(Color::rgb(0, 0, 255)),
        });
        dots(&mut list, 100.0, 2000.0);
        list.finalize();
        host.set_root_layer(PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 2000.0),
            display_list: Arc::new(list),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        });
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        (host, frames)
    }

    fn resource_ids(frame: &CompositorFrame) -> Vec<u64> {
        frame.resource_list.iter().map(|r| r.id.0).collect()
    }

    #[test]
    fn scrolling_moves_the_tiles_without_a_commit() {
        let (mut host, frames) = tall_layer();
        host.composite();
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 250.0));
        assert!(!host.needs_commit());
        host.composite();

        let frames = frames.lock().unwrap();
        let pass = frames[1].root_pass().unwrap();
        // Visible: 250..350, the tile rows at 192, 256 and 320, drawn
        // moved up by 250.
        let mut ys: Vec<f32> = pass.quad_list.iter().map(|q| q.rect.y).collect();
        ys.sort_by(f32::total_cmp);
        ys.dedup();
        assert_eq!(ys, [-58.0, 6.0, 70.0]);
    }

    /// A layer fixed to the viewport stays where it is drawn while the
    /// rest scrolls, with no commit and no raster of its own.
    #[test]
    fn a_fixed_layer_stays_where_it_is_while_the_rest_scrolls() {
        let (mut host, frames) = tall_layer();
        let tall = PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 2000.0),
            display_list: solid(100.0, 2000.0, Color::rgb(0, 0, 255)),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        };
        let bar = PictureLayer {
            id: LayerId(2),
            bounds: SizeF::new(100.0, 20.0),
            display_list: solid(100.0, 20.0, Color::rgb(0, 255, 0)),
            contents_opaque: true,
            background_color: Color::rgb(0, 255, 0),
            invalidation: None,
            properties: LayerProperties {
                fixed: true,
                ..Default::default()
            },
        };
        host.set_layers(vec![tall, bar], PropertyTrees::default());
        host.composite();
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 250.0));
        assert!(!host.needs_commit(), "the compositor scrolls on its own");
        host.composite();

        let frames = frames.lock().unwrap();
        // The fixed layer's quads are at the top of the viewport before
        // and after the scroll; the rest moved up by 250.
        let fixed_quads = |frame: &CompositorFrame| {
            let pass = frame.root_pass().unwrap();
            let mut ys: Vec<f32> = pass
                .quad_list
                .iter()
                .filter(|q| q.rect.height <= 20.0)
                .map(|q| q.rect.y)
                .collect();
            ys.sort_by(f32::total_cmp);
            ys.dedup();
            ys
        };
        assert_eq!(fixed_quads(&frames[0]), fixed_quads(&frames[1]));
        assert_eq!(fixed_quads(&frames[0]), [0.0]);
    }

    /// A sticky box's layer moves as the compositor scrolls: its
    /// translation node works out how far it sticks, with no commit.
    #[test]
    fn a_sticky_layer_sticks_as_the_compositor_scrolls() {
        let (mut host, frames) = tall_layer();
        let tall = PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 2000.0),
            display_list: solid(100.0, 2000.0, Color::rgb(0, 0, 255)),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        };
        // A 20px bar laid out at 50, `top: 0`, in a 2000px block.
        let mut trees = PropertyTrees::default();
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(0)),
            offset: gfx::PointF::new(0.0, 0.0),
            scroll: None,
            sticky: Some(crate::StickyPositionConstraint {
                scroller: ElementId::VIEWPORT,
                constraining_rect: RectF::new(0.0, 0.0, 100.0, 100.0),
                sticky_box: RectF::new(0.0, 50.0, 100.0, 20.0),
                containing_block: RectF::new(0.0, 0.0, 100.0, 2000.0),
                inset: [Some(0.0), None, None, None],
                shifting_containing_block: None,
                pixel_snap_offset: gfx::PointF::new(0.0, 50.0),
                device_scale_factor: 1.0,
            }),
        });
        let bar = PictureLayer {
            id: LayerId(2),
            bounds: SizeF::new(100.0, 20.0),
            display_list: solid(100.0, 20.0, Color::rgb(0, 255, 0)),
            contents_opaque: true,
            background_color: Color::rgb(0, 255, 0),
            invalidation: None,
            properties: LayerProperties {
                transform: TransformNodeId(1),
                origin: gfx::PointF::new(0.0, 50.0),
                ..Default::default()
            },
        };
        host.set_layers(vec![tall, bar], trees);
        host.composite();
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 10.0));
        host.composite();
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 250.0));
        assert!(!host.needs_commit(), "the compositor scrolls on its own");
        host.composite();

        let frames = frames.lock().unwrap();
        let bar_at = |frame: &CompositorFrame| {
            let pass = frame.root_pass().unwrap();
            let mut ys: Vec<f32> = pass
                .quad_list
                .iter()
                .filter(|q| q.rect.height <= 20.0)
                .map(|q| q.rect.y)
                .collect();
            ys.dedup();
            ys
        };
        // Where it was laid out, scrolled 10 short of sticking, then
        // stuck at the top.
        assert_eq!(bar_at(&frames[0]), [50.0]);
        assert_eq!(bar_at(&frames[1]), [40.0]);
        assert_eq!(bar_at(&frames[2]), [0.0]);
    }

    #[test]
    fn scrolling_reuses_the_tiles_it_has() {
        let (mut host, frames) = tall_layer();
        host.composite();
        // A small scroll: the rows at 0 and 64 are still visible, and a
        // row at 128 comes in, rastered already as part of the interest
        // rect.
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 30.0));
        host.composite();
        host.composite();
        let frames = frames.lock().unwrap();
        let (first, second, third) = (
            resource_ids(&frames[0]),
            resource_ids(&frames[1]),
            resource_ids(&frames[2]),
        );
        assert!(
            first.iter().all(|id| second.contains(id)),
            "{first:?} {second:?}"
        );
        assert_eq!(second, third);
    }

    #[test]
    fn tiles_around_the_viewport_are_rastered_ahead() {
        let (mut host, frames) = tall_layer();
        host.composite();
        // Visible: rows 0 and 64, two tiles each. The interest rect reaches
        // 100px further down, to the rows at 128 and 192, so they are
        // ready before they show.
        assert_eq!(frames.lock().unwrap()[0].resource_list.len(), 4);
        assert_eq!(host.host_impl().resident_tiles(), 8);
    }

    #[test]
    fn the_same_background_needs_no_commit() {
        let (mut host, _frames) = tall_layer();
        host.composite();
        host.set_background_color(Color::WHITE);
        assert!(!host.needs_commit());
        host.set_background_color(Color::BLACK);
        assert!(host.needs_commit());
    }

    /// A layer billions of pixels wide and tall has tiles only near the
    /// viewport, and its last ones where they are: past f32's integers,
    /// adding a tile's width to the last one's left edge no longer moves.
    #[test]
    fn a_huge_layer_has_tiles_only_near_the_viewport() {
        let (mut host, frames) = host();
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 50.0, 50.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        list.finalize();
        host.set_root_layer(PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(4.0e9, 4.0e9),
            display_list: Arc::new(list),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        });
        host.set_viewport_rect_and_scale(Size::new(150, 100), 1.0, LocalSurfaceId::default());
        assert_eq!(host.composite(), Some(1));
        // The interest rect: the viewport grown by 150px on each side,
        // -150..300 by -150..250: columns 0..5 by rows 0..4 of 64px tiles.
        assert_eq!(host.host_impl().resident_tiles(), 20);
        assert_eq!(
            frames.lock().unwrap()[0]
                .root_pass()
                .unwrap()
                .quad_list
                .len(),
            6
        );
        let tiling = super::PictureLayerTiling::new(SizeF::new(4.0e9, 4.0e9), 1.0, 256);
        let end = 4.0e9_f32;
        let (rows, columns) = tiling.tiles_in(RectF::new(end - 300.0, end - 300.0, 1000.0, 1000.0));
        assert_eq!((rows.end, columns.end), (15_625_000, 15_625_000));
        let last = tiling.tile_rect((rows.end - 1, columns.end - 1));
        assert_eq!(last.x + last.width, end);
        // A last tile cut at the content's edge.
        let small = super::PictureLayerTiling::new(SizeF::new(300.0, 100.0), 1.0, 256);
        assert_eq!(small.tile_rect((0, 1)), RectF::new(256.0, 0.0, 44.0, 100.0));
    }

    #[test]
    fn tiles_far_from_the_viewport_are_released() {
        let (mut host, frames) = tall_layer();
        host.composite();
        let first = resource_ids(&frames.lock().unwrap()[0]);
        // 1500 is far past the interest rect around 0..100 (-100..200).
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 1500.0));
        host.composite();
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 0.0));
        host.composite();
        let frames = frames.lock().unwrap();
        // Back at the top, the tiles were rastered again: new resources.
        let again = resource_ids(&frames[2]);
        assert!(
            again.iter().all(|id| !first.contains(id)),
            "{first:?} {again:?}"
        );
    }

    /// With a shared image interface, a tile's pixels go into a shared
    /// image and the tile keeps none: the frame hands Viz the image.
    #[test]
    fn a_tile_on_the_gpu_keeps_no_pixels() {
        let gpu = Arc::new(FakeGpu::default());
        let (mut host, frames) = tall_layer_with(Some(gpu.clone()));
        host.composite();
        // Every rastered tile, visible or ahead of the viewport.
        assert_eq!(gpu.alive(), host.host_impl().resident_tiles());
        let frames = frames.lock().unwrap();
        let resources = &frames[0].resource_list;
        assert_eq!(resources.len(), 4);
        for resource in resources {
            assert!(resource.pixels.is_empty(), "{resource:?}");
            let image = resource.shared_image.as_ref().expect("a shared image");
            let pixels = image.backing::<Vec<u8>>().unwrap();
            assert_eq!(
                pixels.len(),
                (resource.size.width * resource.size.height * 4) as usize
            );
            // The top left pixel of every tile is the blue fill.
            assert_eq!(pixels[..4], [0, 0, 255, 255]);
        }
    }

    /// A tile the interface makes no shared image for keeps its pixels.
    #[test]
    fn a_tile_with_no_shared_image_keeps_its_pixels() {
        let gpu = Arc::new(FakeGpu::default());
        gpu.full.store(true, Ordering::Relaxed);
        let (mut host, frames) = tall_layer_with(Some(gpu.clone()));
        host.composite();
        let frames = frames.lock().unwrap();
        let resources = &frames[0].resource_list;
        assert_eq!(resources.len(), 4);
        for resource in resources {
            assert!(resource.shared_image.is_none(), "{resource:?}");
            assert_eq!(
                resource.pixels.len(),
                (resource.size.width * resource.size.height * 4) as usize
            );
        }
    }

    /// A released tile lets its shared image go, once Viz has too.
    #[test]
    fn a_released_tile_lets_its_shared_image_go() {
        let gpu = Arc::new(FakeGpu::default());
        let (mut host, frames) = tall_layer_with(Some(gpu.clone()));
        host.composite();
        let first = gpu.alive();
        host.set_root_scroll_offset(gfx::PointF::new(0.0, 1500.0));
        host.composite();
        // Viz is done with the frames that held them.
        frames.lock().unwrap().clear();
        // Only the tiles around 1500 are left.
        let made = gpu.made.lock().unwrap();
        assert!(made[..first].iter().all(|image| image.strong_count() == 0));
        drop(made);
        assert_eq!(gpu.alive(), host.host_impl().resident_tiles());
    }

    /// When the GPU is lost, the tiles on it are rastered again into new
    /// shared images and the old ones let go; the tiles of one color stay;
    /// all of the next frame is damaged.
    #[test]
    fn losing_the_gpu_rasters_again_the_tiles_that_were_on_it() {
        let gpu = Arc::new(FakeGpu::default());
        let (mut host, frames) = host_with(Some(gpu.clone()));
        // As in `tiles_of_one_color_are_drawn_as_solid_color_quads`: the
        // first column red, the tile at 64,0 dotted, the one at 64,64
        // empty.
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 64.0, 128.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(70.0, 10.0, 1.0, 1.0),
            flags: PaintFlags::fill(Color::BLACK),
        });
        list.finalize();
        host.set_root_layer(PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(128.0, 128.0),
            display_list: Arc::new(list),
            contents_opaque: false,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        });
        host.set_viewport_rect_and_scale(Size::new(128, 128), 1.0, LocalSurfaceId::default());
        host.composite();
        host.composite();
        assert_eq!(gpu.made.lock().unwrap().len(), 1);

        host.did_lose_layer_tree_frame_sink();
        host.composite();
        let prepared = host.host_impl().tiles_prepared();
        assert_eq!((prepared.rastered, prepared.kept), (1, 3));
        assert_eq!(gpu.made.lock().unwrap().len(), 2);
        let mut frames = frames.lock().unwrap();
        assert_eq!(damage(&frames[1]), RectF::default(), "nothing changed");
        assert_eq!(damage(&frames[2]), RectF::new(0.0, 0.0, 128.0, 128.0));
        assert_ne!(resource_ids(&frames[2]), resource_ids(&frames[1]));
        assert_eq!(frames[2].root_pass().unwrap().quad_list.len(), 3);
        // Viz is done with the frames before.
        frames.drain(..2);
        assert_eq!(gpu.alive(), 1, "the tile holds only the new one");
    }

    /// A layer drawn over the surface layers (a browser's popup) comes
    /// before their quads, front to back; the others after.
    #[test]
    fn a_layer_over_the_surfaces_is_drawn_in_front_of_them() {
        use protocol::viz::{FrameSinkId, SurfaceId};

        use crate::SurfaceLayer;

        let (mut host, frames) = host();
        let mut page = DisplayItemList::new();
        dots(&mut page, 100.0, 100.0);
        page.finalize();
        let root = PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 100.0),
            display_list: Arc::new(page),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        };
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 40.0, 40.0),
            flags: PaintFlags::fill(Color::rgb(255, 0, 0)),
        });
        list.finalize();
        let popup = PictureLayer {
            id: LayerId(3),
            bounds: SizeF::new(40.0, 40.0),
            display_list: Arc::new(list),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: LayerProperties {
                origin: gfx::PointF::new(10.0, 10.0),
                over_surfaces: true,
                ..Default::default()
            },
        };
        host.set_layers([root, popup], PropertyTrees::default());
        let surface_id = SurfaceId {
            frame_sink_id: FrameSinkId {
                client_id: 1,
                sink_id: 1,
            },
            local_surface_id: LocalSurfaceId::default(),
        };
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        host.set_surface_layers(vec![SurfaceLayer {
            id: LayerId(2),
            position: gfx::PointF::new(0.0, 0.0),
            bounds: SizeF::new(100.0, 100.0),
            surface_id,
            background_color: Color::WHITE,
        }]);
        host.composite();
        let frames = frames.lock().unwrap();
        let quads = &frames[0].root_pass().unwrap().quad_list;
        assert_eq!(
            quads[0].material,
            Material::SolidColor {
                color: Color::rgb(255, 0, 0)
            }
        );
        assert_eq!(quads[0].rect, RectF::new(10.0, 10.0, 40.0, 40.0));
        assert!(matches!(quads[1].material, Material::Surface { .. }));
        assert!(
            quads[2..]
                .iter()
                .all(|q| matches!(q.material, Material::Texture { .. }))
        );
    }

    #[test]
    fn a_surface_layer_is_one_surface_quad_over_the_root() {
        use protocol::viz::{FrameSinkId, SurfaceId};

        use crate::SurfaceLayer;

        let (mut host, frames) = tall_layer();
        let surface_id = SurfaceId {
            frame_sink_id: FrameSinkId {
                client_id: 1,
                sink_id: 1,
            },
            local_surface_id: LocalSurfaceId::default(),
        };
        let layer = SurfaceLayer {
            id: LayerId(2),
            position: gfx::PointF::new(0.0, 20.0),
            bounds: SizeF::new(100.0, 30.0),
            surface_id,
            background_color: Color::WHITE,
        };
        host.set_viewport_rect_and_scale(Size::new(100, 100), 2.0, LocalSurfaceId::default());
        host.set_surface_layers(vec![layer.clone()]);
        host.composite();
        // The same layers again: nothing to commit.
        host.set_surface_layers(vec![layer]);
        assert!(!host.needs_commit());
        let frames = frames.lock().unwrap();
        let quads = &frames[0].root_pass().unwrap().quad_list;
        // In front of the tiles, at twice the CSS px.
        assert_eq!(quads[0].rect, RectF::new(0.0, 40.0, 200.0, 60.0));
        assert_eq!(
            quads[0].material,
            Material::Surface {
                surface_id,
                default_background_color: Color::WHITE
            }
        );
        assert!(
            quads[1..]
                .iter()
                .all(|q| matches!(q.material, Material::Texture { .. }))
        );
    }

    #[test]
    fn empty_viewport_draws_nothing() {
        let (mut host, frames) = host();
        host.set_viewport_rect_and_scale(Size::new(0, 0), 1.0, LocalSurfaceId::default());
        assert_eq!(host.composite(), None);
        assert!(frames.lock().unwrap().is_empty());
    }

    /// A canvas's layer of `snapshot`, at (10, 20), 4×2 CSS px.
    fn texture_layer(snapshot: gfx::ImageData) -> crate::TextureLayer {
        crate::TextureLayer {
            id: LayerId(7),
            bounds: SizeF::new(4.0, 2.0),
            snapshot,
            contents_opaque: false,
            properties: LayerProperties {
                origin: gfx::PointF::new(10.0, 20.0),
                ..Default::default()
            },
        }
    }

    fn snapshot(pixel: [u8; 4]) -> gfx::ImageData {
        gfx::ImageData::decoded(Size::new(2, 1), [pixel, pixel].concat())
    }

    /// The document, white, then `texture` over it. The document's tiles
    /// are rastered once: the display list is the same each time.
    fn with_texture(host: &mut LayerTreeHost<SingleThreadProxy>, texture: crate::TextureLayer) {
        let document = PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 100.0),
            display_list: solid(100.0, 100.0, Color::WHITE),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: Some(Arc::from([])),
            properties: Default::default(),
        };
        host.set_layers(
            [crate::Layer::from(document), crate::Layer::from(texture)],
            PropertyTrees::default(),
        );
    }

    /// The texture quad of `frame` (the front one), and its resource.
    fn texture_quad(
        frame: &CompositorFrame,
    ) -> (protocol::viz::DrawQuad, protocol::viz::TransferableResource) {
        let pass = frame.root_pass().unwrap();
        let quad = pass.quad_list[0];
        let Material::Texture { resource_id, .. } = quad.material else {
            panic!("not a texture: {:?}", quad.material);
        };
        let resource = frame
            .resource_list
            .iter()
            .find(|r| r.id == resource_id)
            .unwrap()
            .clone();
        (quad, resource)
    }

    /// A texture layer is one quad over what is under it, its snapshot
    /// scaled bilinearly into its bounds in device pixels.
    #[test]
    fn a_texture_layer_is_one_bilinear_quad_of_its_snapshot_over_the_layers_before() {
        let (mut host, frames) = host();
        with_texture(&mut host, texture_layer(snapshot([0, 0, 255, 255])));
        host.set_viewport_rect_and_scale(Size::new(100, 100), 2.0, LocalSurfaceId::default());
        assert_eq!(host.composite(), Some(1));
        let frames = frames.lock().unwrap();
        let (quad, resource) = texture_quad(&frames[0]);
        assert_eq!(quad.rect, RectF::new(20.0, 40.0, 8.0, 4.0));
        assert_eq!(
            quad.material,
            Material::Texture {
                resource_id: resource.id,
                tex_coord_rect: RectF::new(0.0, 0.0, 2.0, 1.0),
                nearest_neighbor: false,
            }
        );
        assert_eq!(resource.size, Size::new(2, 1));
        assert_eq!(&*resource.pixels, &[0, 0, 255, 255, 0, 0, 255, 255]);
        // The document's tiles are behind it, drawn 1:1.
        let pass = frames[0].root_pass().unwrap();
        assert!(pass.quad_list[1..].iter().all(|q| matches!(
            q.material,
            Material::Texture {
                nearest_neighbor: true,
                ..
            } | Material::SolidColor { .. }
        )));
    }

    /// A canvas drawn on another thread: its snapshot is waited for before
    /// it is put on the GPU (Viz waiting on the sync token); one whose
    /// pixels never come has no quad.
    #[test]
    fn a_texture_layer_waits_for_its_snapshots_pixels() {
        let (mut host, frames) = host();
        let (pending, filler) = gfx::ImageData::pending(Size::new(2, 1));
        let fill = std::thread::spawn(move || {
            // Late enough that the upload is waiting by then.
            std::thread::sleep(std::time::Duration::from_millis(50));
            filler.fill(Arc::new([0, 0, 255, 255].repeat(2)));
        });
        with_texture(&mut host, texture_layer(pending));
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        assert_eq!(host.composite(), Some(1));
        fill.join().unwrap();
        let (never, filler) = gfx::ImageData::pending(Size::new(2, 1));
        drop(filler);
        with_texture(&mut host, texture_layer(never));
        host.composite();
        let frames = frames.lock().unwrap();
        let (_, resource) = texture_quad(&frames[0]);
        assert_eq!(&*resource.pixels, &[0, 0, 255, 255, 0, 0, 255, 255]);
        let last = frames.last().unwrap().root_pass().unwrap();
        assert!(
            last.quad_list.iter().all(|q| !matches!(
                q.material,
                Material::Texture {
                    nearest_neighbor: false,
                    ..
                }
            )),
            "{:?}",
            last.quad_list
        );
    }

    /// The same snapshot keeps its resource, and rasters nothing; a new one
    /// gets a new resource and damages only the layer, leaving the
    /// document's tiles as they are.
    #[test]
    fn a_texture_layer_puts_each_snapshot_on_the_gpu_once() {
        let gpu = Arc::new(FakeGpu::default());
        let (mut host, frames) = host_with(Some(gpu.clone()));
        let blue = snapshot([0, 0, 255, 255]);
        with_texture(&mut host, texture_layer(blue.clone()));
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        host.composite();
        let made = gpu.made.lock().unwrap().len();
        // Committed again with the same snapshot: nothing made.
        with_texture(&mut host, texture_layer(blue));
        host.composite();
        assert_eq!(gpu.made.lock().unwrap().len(), made);
        // A new snapshot: one new shared image, the layer's.
        with_texture(&mut host, texture_layer(snapshot([255, 0, 0, 255])));
        host.composite();
        assert_eq!(gpu.made.lock().unwrap().len(), made + 1);
        let frames = frames.lock().unwrap();
        let (_, first) = texture_quad(&frames[0]);
        let (_, kept) = texture_quad(&frames[1]);
        let (_, new) = texture_quad(&frames[2]);
        assert_eq!(kept.id, first.id);
        assert_ne!(new.id, first.id);
        assert!(new.shared_image.is_some() && new.pixels.is_empty());
        // The second frame changed nothing; the third only the layer.
        assert_eq!(damage(&frames[1]), RectF::default());
        assert_eq!(damage(&frames[2]), RectF::new(10.0, 20.0, 4.0, 2.0));
        // The document's tile resources are the same in all three.
        let tiles = |frame: &CompositorFrame| {
            frame.root_pass().unwrap().quad_list[1..]
                .iter()
                .filter_map(|q| match q.material {
                    Material::Texture { resource_id, .. } => Some(resource_id),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(tiles(&frames[2]), tiles(&frames[0]));
    }

    /// The GPU lost, a texture layer's snapshot is put on the new one.
    #[test]
    fn a_texture_layer_is_put_on_a_new_gpu() {
        let gpu = Arc::new(FakeGpu::default());
        let (mut host, frames) = host_with(Some(gpu.clone()));
        with_texture(&mut host, texture_layer(snapshot([0, 0, 255, 255])));
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        host.composite();
        let made = gpu.made.lock().unwrap().len();
        host.did_lose_layer_tree_frame_sink();
        host.composite();
        assert!(gpu.made.lock().unwrap().len() > made);
        let frames = frames.lock().unwrap();
        let (_, before) = texture_quad(&frames[0]);
        let (_, after) = texture_quad(frames.last().unwrap());
        assert_ne!(after.id, before.id);
    }
}
