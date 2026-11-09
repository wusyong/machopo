use std::sync::Arc;

use gfx::{Color, PointF, Size};
use protocol::viz::{LocalSurfaceId, SharedImageInterface};

use crate::layer_tree_host_impl::CommitState;
use crate::{Layer, PictureLayer, PropertyTrees, ScrollNode, ScrollOffsets, SurfaceLayer};

/// `cc::LayerTreeSettings` (the knobs we have so far).
#[derive(Clone)]
pub struct LayerTreeSettings {
    /// Square tile edge in physical pixels (Chromium's default is 256).
    pub default_tile_size: u32,
    /// A threaded host draws a `BeginFrame` only once the main frame it
    /// started has committed, so each frame has the main thread's update
    /// (headless and web tests, whose output must come out the same every
    /// run). Off, it draws the active tree right away and the main frame's
    /// commit at the next `BeginFrame`: a busy main thread doesn't stop the
    /// compositor drawing what it scrolls (the window). On by default.
    pub wait_for_all_pipeline_stages_before_draw: bool,
    /// Where tiles go once rastered, keeping no pixels here (cc's
    /// `RasterContextProvider` and its `SharedImageInterface`); without one,
    /// or while it has no GPU, tiles keep their pixels.
    pub shared_image_interface: Option<Arc<dyn SharedImageInterface>>,
    /// Threads that raster tiles at once (cc's `--num-raster-threads`): a
    /// pool of its own, made when there are two tiles to raster; 1 (the
    /// default) rasters them on the compositor thread, without one. More
    /// raster a page of many tiles (resizing it) several times as fast, but
    /// take CPU and memory from the page while it runs (Speedometer's
    /// suites, a few % more memory), so they are asked for. Chromium's
    /// renderer has 1 too, rastering on the GPU.
    pub num_raster_threads: usize,
}

impl std::fmt::Debug for LayerTreeSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerTreeSettings")
            .field("default_tile_size", &self.default_tile_size)
            .field(
                "wait_for_all_pipeline_stages_before_draw",
                &self.wait_for_all_pipeline_stages_before_draw,
            )
            .field(
                "shared_image_interface",
                &self.shared_image_interface.is_some(),
            )
            .field("num_raster_threads", &self.num_raster_threads)
            .finish()
    }
}

impl Default for LayerTreeSettings {
    fn default() -> Self {
        Self {
            default_tile_size: 256,
            wait_for_all_pipeline_stages_before_draw: true,
            shared_image_interface: None,
            num_raster_threads: 1,
        }
    }
}

/// Which document a tree shows, as the renderer numbers its documents
/// (Blink's `DocumentToken`; cc itself knows no documents). A new one means
/// another document: what was scrolled or drawn of the old one doesn't
/// carry over. Not a navigation's id: a document need not come from one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct DocumentId(pub u64);

/// `cc::LayerTreeHost`: the main-thread half of the compositor.
///
/// Blink (the `paint` crate) pushes layers here. `P` is the proxy
/// to the impl side, Chromium's `cc::Proxy`, but picked at compile time:
/// what only makes sense in one mode exists only on that mode's
/// `LayerTreeHost`. [`SingleThreadProxy`](crate::SingleThreadProxy) runs
/// the impl side right here, for the browser's UI.
pub struct LayerTreeHost<P> {
    /// The document's layers, in paint order (the last on top).
    layers: Vec<Layer>,
    /// Where the layers are and what clips them.
    property_trees: PropertyTrees,
    /// The offsets of the scroll containers that are layers: moved by the
    /// compositor, not painted.
    scroll_offsets: ScrollOffsets,
    /// Drawn over the picture layers, in order (the last on top).
    surface_layers: Vec<SurfaceLayer>,
    viewport_size: Size,
    device_scale_factor: f32,
    background_color: Color,
    local_surface_id: LocalSurfaceId,
    /// Which document the layers show (the renderer's navigation id). The
    /// impl side tags what it hands back with the document of the frame it
    /// last drew, so the main thread can tell what the user was looking at.
    document_id: DocumentId,
    /// The viewport's scroll offset, CSS px.
    pub(crate) root_scroll_offset: PointF,
    /// The viewport as a scroller, for the impl side to scroll it.
    viewport_scroll_node: Option<ScrollNode>,
    needs_commit: bool,
    source_frame_number: u64,
    pub(crate) proxy: P,
}

impl<P> LayerTreeHost<P> {
    pub(crate) fn with_proxy(proxy: P) -> Self {
        Self {
            layers: Vec::new(),
            property_trees: PropertyTrees::default(),
            scroll_offsets: ScrollOffsets::default(),
            surface_layers: Vec::new(),
            viewport_size: Size::new(0, 0),
            device_scale_factor: 1.0,
            background_color: Color::WHITE,
            local_surface_id: LocalSurfaceId::default(),
            document_id: DocumentId::default(),
            root_scroll_offset: PointF::new(0.0, 0.0),
            viewport_scroll_node: None,
            needs_commit: false,
            source_frame_number: 0,
            proxy,
        }
    }

    /// One layer, at the document's origin: all the browser's UI has.
    pub fn set_root_layer(&mut self, layer: PictureLayer) {
        self.layers = vec![layer.into()];
        self.property_trees = PropertyTrees::default();
        self.set_needs_commit();
    }

    /// The document's layers, in paint order, and the trees they are in
    /// (`PaintArtifactCompositor`'s output).
    pub fn set_layers(
        &mut self,
        layers: impl IntoIterator<Item = impl Into<Layer>>,
        property_trees: PropertyTrees,
    ) {
        self.layers = layers.into_iter().map(Into::into).collect();
        self.property_trees = property_trees;
        self.set_needs_commit();
    }

    /// The scroll containers' offsets, CSS px. A change commits: the impl
    /// side moves those that are layers, from the tiles it has.
    pub fn set_scroll_offsets(&mut self, offsets: ScrollOffsets) {
        if offsets != self.scroll_offsets {
            self.scroll_offsets = offsets;
            self.set_needs_commit();
        }
    }

    /// The surface layers over the root layer. Commits only if they
    /// changed: a new frame in a surface needs nothing here, Viz draws it.
    pub fn set_surface_layers(&mut self, layers: Vec<SurfaceLayer>) {
        if layers != self.surface_layers {
            self.surface_layers = layers;
            self.set_needs_commit();
        }
    }

    /// `SetViewportRectAndScale`. `viewport_size` is in physical pixels.
    pub fn set_viewport_rect_and_scale(
        &mut self,
        viewport_size: Size,
        device_scale_factor: f32,
        local_surface_id: LocalSurfaceId,
    ) {
        self.viewport_size = viewport_size;
        self.device_scale_factor = device_scale_factor;
        self.local_surface_id = local_surface_id;
        self.set_needs_commit();
    }

    /// The viewport's scroll offset, in CSS px: how far the root layer is
    /// scrolled. Needs no commit; the next frame draws with it, from the
    /// tiles the impl side has.
    pub fn set_root_scroll_offset(&mut self, offset: PointF) {
        self.root_scroll_offset = offset;
    }

    /// What the impl side may scroll the viewport over. A change commits:
    /// the impl side clamps what it scrolled to the node of each commit,
    /// as the main thread will clamp it.
    pub fn set_viewport_scroll_node(&mut self, node: Option<ScrollNode>) {
        if node != self.viewport_scroll_node {
            self.viewport_scroll_node = node;
            self.set_needs_commit();
        }
    }

    /// Which document the layers show from now on.
    pub fn set_document_id(&mut self, document_id: DocumentId) {
        if document_id != self.document_id {
            self.document_id = document_id;
            self.set_needs_commit();
        }
    }

    pub fn set_background_color(&mut self, color: Color) {
        if color != self.background_color {
            self.background_color = color;
            self.set_needs_commit();
        }
    }

    pub fn set_needs_commit(&mut self) {
        self.needs_commit = true;
    }

    pub fn needs_commit(&self) -> bool {
        self.needs_commit
    }

    pub fn source_frame_number(&self) -> u64 {
        self.source_frame_number
    }

    /// What a commit hands the impl side, if one is needed
    /// (`LayerTreeHost::FinishCommitOnImplThread`'s input). Taking it is the
    /// commit, as far as the main side goes: it needs no other until
    /// something changes, and the next gets the next frame number.
    pub(crate) fn take_commit_state(&mut self) -> Option<CommitState> {
        if !self.needs_commit {
            return None;
        }
        let state = CommitState {
            layers: self.layers.clone(),
            property_trees: self.property_trees.clone(),
            scroll_offsets: self.scroll_offsets.clone(),
            surface_layers: self.surface_layers.clone(),
            viewport_size: self.viewport_size,
            device_scale_factor: self.device_scale_factor,
            background_color: self.background_color,
            local_surface_id: self.local_surface_id,
            document_id: self.document_id,
            root_scroll_offset: self.root_scroll_offset,
            viewport_scroll_node: self.viewport_scroll_node,
            source_frame_number: self.source_frame_number,
            swap_promises: Vec::new(),
        };
        self.needs_commit = false;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one a commit: a u64 never runs out"
        )]
        let next = self.source_frame_number + 1;
        self.source_frame_number = next;
        Some(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unless asked for more, tiles are rastered on the compositor thread.
    #[test]
    fn tiles_are_rastered_on_the_compositor_thread_unless_asked() {
        assert_eq!(LayerTreeSettings::default().num_raster_threads, 1);
    }
}
