//! The main frame's widget (Blink's `WebFrameWidgetImpl` and its
//! `WidgetBase`): the `LayerTreeHost` the frame's layers go to the
//! compositor through, what was last committed, and what waits for the
//! next commit. The frame's (`Frame` has it once the first document
//! commits), as Chromium's widget is the local root frame's, not the
//! render thread's: script paused at a breakpoint reaches it through the
//! engine, as it reaches the frame's layout, and draws while paused.

use std::sync::Arc;

use compositor::{LayerTreeHost, PictureLayer, ProxyMain, SwapPromise};

use crate::frame_view::PaintOutput;

pub(crate) struct FrameWidget {
    pub(crate) layer_tree_host: LayerTreeHost<ProxyMain>,
    /// The paint the layers are from: a frame that only scrolled keeps it,
    /// so its commit keeps the tiles.
    pub(crate) painted: Option<Arc<PaintOutput>>,
    /// DevTools' overlay's layer as last committed, and the count of what
    /// it showed then.
    pub(crate) overlay_layer: Option<PictureLayer>,
    pub(crate) overlay_version: u64,
    /// The DOM's tree version its last frame drew: script paused asks for
    /// a frame once it moves.
    pub(crate) rendered_tree_version: u64,
    /// The acks of input whose handling asked for a main frame: they go
    /// with the next commit, so the browser hears them once the frame that
    /// shows the change is drawn.
    pub(crate) pending_input_acks: Vec<SwapPromise>,
    /// `VisualStateReady`s waiting for the main frame they asked for: they
    /// go with the next commit, as input acks do.
    pub(crate) pending_visual_states: Vec<SwapPromise>,
}

impl FrameWidget {
    pub(crate) fn new(layer_tree_host: LayerTreeHost<ProxyMain>) -> Self {
        Self {
            layer_tree_host,
            painted: None,
            overlay_layer: None,
            overlay_version: 0,
            rendered_tree_version: 0,
            pending_input_acks: Vec::new(),
            pending_visual_states: Vec::new(),
        }
    }
}
