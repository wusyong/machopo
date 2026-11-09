use protocol::viz::{CompositorFrame, LocalSurfaceId};

/// `cc::LayerTreeFrameSink`: where the impl side sends finished frames.
///
/// The production implementation is
/// [`crate::mojo_embedder::ChannelLayerTreeFrameSink`]; tests can collect
/// frames directly. `Send`: a threaded host's impl side, frame sink
/// included, lives on the compositor thread.
pub trait LayerTreeFrameSink: Send {
    fn submit_compositor_frame(&mut self, local_surface_id: LocalSurfaceId, frame: CompositorFrame);

    /// Asks Viz for `BeginFrame`s, or stops asking. A sink that submits
    /// frames on its own (the browser's UI) never asks.
    fn set_needs_begin_frame(&mut self, _needs: bool) {}

    /// Answers a `BeginFrame` that produced no frame.
    fn did_not_produce_frame(&mut self) {}
}

impl<F: FnMut(LocalSurfaceId, CompositorFrame) + Send> LayerTreeFrameSink for F {
    fn submit_compositor_frame(
        &mut self,
        local_surface_id: LocalSurfaceId,
        frame: CompositorFrame,
    ) {
        self(local_surface_id, frame)
    }
}
