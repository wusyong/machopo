//! `cc::SingleThreadProxy`: the impl side on the main thread itself.

use crate::layer_tree_host_impl::LayerTreeHostImpl;
use crate::{LayerTreeFrameSink, LayerTreeHost, LayerTreeSettings};

/// `cc::SingleThreadProxy`: owns the impl side and runs it synchronously,
/// on the thread the `LayerTreeHost` is on.
///
/// Unlike Chromium's, it has no scheduler. Chromium's waits for a
/// `BeginFrame` and calls its user back to update and draw; this one leaves
/// when to draw to its user (`composite`), who also talks to Viz itself
/// (`set_needs_begin_frame`, `did_not_produce_frame`).
pub struct SingleThreadProxy {
    host_impl: LayerTreeHostImpl,
    /// What the frame sink last told Viz (`set_needs_begin_frame`).
    needs_begin_frame: bool,
}

impl LayerTreeHost<SingleThreadProxy> {
    /// `LayerTreeHost::CreateSingleThreaded`.
    pub fn new_single_threaded(
        settings: LayerTreeSettings,
        frame_sink: Box<dyn LayerTreeFrameSink>,
    ) -> Self {
        Self::with_proxy(SingleThreadProxy {
            host_impl: LayerTreeHostImpl::new(settings, frame_sink),
            needs_begin_frame: false,
        })
    }

    /// Asks Viz for `BeginFrame`s, or stops asking (`SetNeedsBeginFrame`).
    /// Sent only when it changes.
    pub fn set_needs_begin_frame(&mut self, needs: bool) {
        let proxy = &mut self.proxy;
        if needs != proxy.needs_begin_frame {
            proxy.needs_begin_frame = needs;
            proxy.host_impl.frame_sink().set_needs_begin_frame(needs);
        }
    }

    /// `DidLoseLayerTreeFrameSink`: the GPU was lost and Viz has a new one.
    /// The next `composite` rasters again the tiles that were on the old
    /// one. Its user hears of the loss itself, on its own thread, and
    /// composites (Chromium's `SingleThreadProxy` schedules it).
    pub fn did_lose_layer_tree_frame_sink(&mut self) {
        self.proxy.host_impl.did_lose_layer_tree_frame_sink();
    }

    /// Answers a `BeginFrame` that drew nothing (`DidNotProduceFrame`).
    pub fn did_not_produce_frame(&mut self) {
        self.proxy.host_impl.frame_sink().did_not_produce_frame();
    }

    #[cfg(test)]
    pub(crate) fn host_impl(&self) -> &LayerTreeHostImpl {
        &self.proxy.host_impl
    }

    /// Commit + prepare tiles + draw + submit, right now
    /// (`SingleThreadProxy::CompositeImmediately`). Returns the submitted
    /// frame token, or `None` if there was nothing to draw.
    pub fn composite(&mut self) -> Option<u64> {
        let _span = trace_event::trace_event!("LayerTreeHost::composite");
        if let Some(state) = self.take_commit_state() {
            self.proxy.host_impl.commit(state);
        }
        let offset = self.root_scroll_offset;
        let host_impl = &mut self.proxy.host_impl;
        host_impl.reset_scroll_offset(offset);
        host_impl.prepare_tiles();
        host_impl.activate();
        host_impl.draw_and_submit()
    }
}
