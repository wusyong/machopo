//! Frame sink backed by a channel to the Viz thread
//! (`cc/mojo_embedder/async_layer_tree_frame_sink`).

use protocol::viz::{
    CompositorFrame, CompositorFrameSinkMsg, CompositorFrameSinkSender, LocalSurfaceId,
};

use crate::LayerTreeFrameSink;

/// Sends to Viz through `sink`, which the browser bound to this
/// compositor's `FrameSinkId`.
pub struct ChannelLayerTreeFrameSink {
    sink: CompositorFrameSinkSender,
}

impl ChannelLayerTreeFrameSink {
    pub fn new(sink: CompositorFrameSinkSender) -> Self {
        Self { sink }
    }
}

impl LayerTreeFrameSink for ChannelLayerTreeFrameSink {
    fn submit_compositor_frame(
        &mut self,
        local_surface_id: LocalSurfaceId,
        frame: CompositorFrame,
    ) {
        let msg = CompositorFrameSinkMsg::SubmitCompositorFrame {
            local_surface_id,
            frame,
        };
        if self.sink.send(msg).is_err() {
            tracing::debug!("viz is gone; dropping frame");
        }
    }

    fn set_needs_begin_frame(&mut self, needs: bool) {
        let _ = self
            .sink
            .send(CompositorFrameSinkMsg::SetNeedsBeginFrame(needs));
    }

    fn did_not_produce_frame(&mut self) {
        let _ = self.sink.send(CompositorFrameSinkMsg::DidNotProduceFrame);
    }
}
