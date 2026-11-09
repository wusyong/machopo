use std::collections::HashSet;

use gfx::Size;
use protocol::viz::{
    CompositorFrame, CopyOutputRequest, CopyOutputResult, FrameSinkId, LocalSurfaceId,
    MAX_TEXTURE_SIZE,
};

use crate::surface_aggregator::{AggregatedFrame, SurfaceAggregator, SurfaceManager};

/// `viz::DirectRenderer`: draws an aggregated frame to some output.
pub trait DirectRenderer {
    /// The output surface changed size (physical pixels).
    fn resize(&mut self, size: Size);
    /// Draws `frame` and presents / swaps.
    fn draw_frame(&mut self, frame: &AggregatedFrame<'_>);
    /// Draws `frame` as [`draw_frame`](Self::draw_frame) does, and returns
    /// a copy of the output as drawn (for `CopyOutputRequest`s).
    fn draw_frame_and_copy(
        &mut self,
        frame: &AggregatedFrame<'_>,
    ) -> Result<CopyOutputResult, CopyFailure> {
        self.draw_frame(frame);
        Err(CopyFailure::Unsupported)
    }
    /// Loses the GPU device as a driver reset would (`about:gpucrash`);
    /// the next draw makes a new one.
    fn lose_context(&mut self) {}
}

/// Why a renderer has no copy of the frame it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyFailure {
    /// Its output can't be read back: the copy requests get `None`.
    Unsupported,
    /// It drew nothing this time (a window hidden or minimised has nothing
    /// to draw into): the copy requests wait for a frame that is drawn.
    NotDrawn,
}

/// `viz::Display`: owns the surfaces, the aggregator and the renderer for one
/// output (one window).
pub struct Display<R: DirectRenderer> {
    root_frame_sink: FrameSinkId,
    surfaces: SurfaceManager,
    aggregator: SurfaceAggregator,
    /// The sinks the last frame drawn embedded: a new frame from one of
    /// them is drawn too.
    embedded: HashSet<FrameSinkId>,
    renderer: R,
    needs_draw: bool,
    /// Answered by the next draw.
    copy_requests: Vec<CopyOutputRequest>,
}

impl<R: DirectRenderer> Display<R> {
    /// `root_frame_sink` is the client whose surface fills the output: in a
    /// window the browser's own compositor (Chromium's `ui::Compositor`),
    /// which embeds the tab; in headless mode the tab itself.
    pub fn new(renderer: R, root_frame_sink: FrameSinkId) -> Self {
        Self {
            root_frame_sink,
            surfaces: SurfaceManager::default(),
            aggregator: SurfaceAggregator::default(),
            embedded: HashSet::new(),
            renderer,
            needs_draw: false,
            copy_requests: Vec::new(),
        }
    }

    /// `sink`'s new frame, once checked: a frame whose resources aren't
    /// what they say is dropped, and the last one stays (Chromium reports
    /// the client's bad message and ends its process; here a client is a
    /// thread of the same process, so such a frame is machopo's own bug).
    pub fn submit_compositor_frame(
        &mut self,
        sink: FrameSinkId,
        local_surface_id: LocalSurfaceId,
        frame: CompositorFrame,
    ) {
        if let Some(bad) = bad_resource(&frame) {
            tracing::error!(?sink, bad, "viz: dropping a frame");
            return;
        }
        if self.surfaces.submit(sink, local_surface_id, frame)
            && (sink == self.root_frame_sink || self.embedded.contains(&sink))
        {
            self.needs_draw = true;
        }
    }

    pub fn resize(&mut self, size: Size) {
        self.renderer.resize(size);
        self.needs_draw = true;
    }

    /// Loses the renderer's GPU device; the next draw, which comes at once,
    /// makes a new one.
    pub fn lose_context(&mut self) {
        self.renderer.lose_context();
        self.needs_draw = true;
    }

    /// Hands a copy of the next frame drawn to `request`: one is drawn even
    /// if nothing changed.
    pub fn request_copy_of_output(&mut self, request: CopyOutputRequest) {
        self.copy_requests.push(request);
        self.needs_draw = true;
    }

    /// `sink` has gone: its surface goes, and the next draw shows what is
    /// left (`SurfaceManager::InvalidateFrameSinkId`).
    pub fn remove_frame_sink(&mut self, sink: FrameSinkId) {
        self.surfaces.remove(sink);
        self.embedded.remove(&sink);
        self.needs_draw = true;
    }

    pub fn set_needs_draw(&mut self) {
        self.needs_draw = true;
    }

    pub fn needs_draw(&self) -> bool {
        self.needs_draw
    }

    /// `Display::DrawAndSwap`. Returns false if there was nothing to draw.
    pub fn draw_and_swap(&mut self) -> bool {
        self.needs_draw = false;
        let _span = trace_event::trace_event!("Display::DrawAndSwap");
        match self
            .aggregator
            .aggregate(&self.surfaces, self.root_frame_sink)
        {
            Some(frame) => {
                if self.copy_requests.is_empty() {
                    self.renderer.draw_frame(&frame);
                } else {
                    let copy = match self.renderer.draw_frame_and_copy(&frame) {
                        Ok(copy) => Some(copy),
                        Err(CopyFailure::Unsupported) => None,
                        Err(CopyFailure::NotDrawn) => {
                            self.embedded = frame.embedded;
                            return true;
                        }
                    };
                    for request in self.copy_requests.drain(..) {
                        request.send_result(copy.clone());
                    }
                }
                self.embedded = frame.embedded;
                true
            }
            None => false,
        }
    }

    pub fn renderer(&self) -> &R {
        &self.renderer
    }

    pub fn renderer_mut(&mut self) -> &mut R {
        &mut self.renderer
    }
}

/// Whether `pixels` bytes are the pixels of an RGBA8 texture of `size`
/// Viz can make: no side past [`MAX_TEXTURE_SIZE`], and as many as the
/// size has (Chromium's `CheckedNumeric` size of a shared bitmap).
pub(crate) fn pixels_fit(size: Size, pixels: usize) -> bool {
    size.width <= MAX_TEXTURE_SIZE
        && size.height <= MAX_TEXTURE_SIZE
        && size.checked_byte_size() == Some(pixels)
}

/// What is wrong with `frame`'s resources, if anything: one of pixels that
/// don't fit its size ([`pixels_fit`]). One on a shared image was checked
/// as it was made (`SharedImageInterface::create_shared_image`).
fn bad_resource(frame: &CompositorFrame) -> Option<String> {
    frame
        .resource_list
        .iter()
        .find(|resource| {
            resource.shared_image.is_none() && !pixels_fit(resource.size, resource.pixels.len())
        })
        .map(|resource| {
            format!(
                "resource {:?}: {} bytes for {:?}",
                resource.id,
                resource.pixels.len(),
                resource.size
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::{Arc, Mutex};

    use gfx::{Color, RectF};
    use protocol::viz::{
        CompositorFrameMetadata, CompositorRenderPass, DrawQuad, Material, RenderPassId,
        SharedQuadState, SurfaceId,
    };

    #[derive(Default)]
    struct Renderer {
        lost: u32,
        drawn: u32,
    }

    impl DirectRenderer for Renderer {
        fn resize(&mut self, _: Size) {}
        fn draw_frame(&mut self, _: &AggregatedFrame<'_>) {
            self.drawn += 1;
        }
        fn lose_context(&mut self) {
            self.lost += 1;
        }
    }

    /// Reads back a 2×1 output of the frame's background colour: what a
    /// `Display` does with copies, not how a frame is drawn
    /// (`WgpuRenderer`'s own tests).
    struct Copying;

    impl DirectRenderer for Copying {
        fn resize(&mut self, _: Size) {}
        fn draw_frame(&mut self, _: &AggregatedFrame<'_>) {}
        fn draw_frame_and_copy(
            &mut self,
            frame: &AggregatedFrame<'_>,
        ) -> Result<CopyOutputResult, CopyFailure> {
            Ok(copy_of(frame.background_color.premultiplied()).unwrap())
        }
    }

    const ROOT: FrameSinkId = FrameSinkId {
        client_id: 0,
        sink_id: 1,
    };

    /// A 2×1 frame, all `color`.
    fn frame(color: Color) -> CompositorFrame {
        let rect = RectF::new(0.0, 0.0, 2.0, 1.0);
        CompositorFrame {
            metadata: CompositorFrameMetadata {
                device_scale_factor: 1.0,
                frame_token: 1,
                root_background_color: color,
            },
            resource_list: Vec::new(),
            render_pass_list: vec![CompositorRenderPass {
                id: RenderPassId(1),
                output_rect: rect,
                damage_rect: rect,
                shared_quad_state_list: vec![SharedQuadState::default()],
                quad_list: vec![DrawQuad {
                    rect,
                    shared_quad_state: 0,
                    material: Material::SolidColor { color },
                }],
            }],
        }
    }

    /// What each copy request got, in order.
    type Copies = Arc<Mutex<Vec<Option<CopyOutputResult>>>>;

    fn request(copies: &Copies) -> CopyOutputRequest {
        let copies = copies.clone();
        CopyOutputRequest::new(move |result| copies.lock().unwrap().push(result))
    }

    fn copy_of(color: [u8; 4]) -> Option<CopyOutputResult> {
        Some(CopyOutputResult {
            size: Size::new(2, 1),
            pixels: color.repeat(2),
        })
    }

    /// A closed tab's surface goes, and what is left is drawn again.
    #[test]
    fn a_removed_frame_sink_takes_its_surface() {
        let tab = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        let mut display = Display::new(Renderer::default(), ROOT);
        display.submit_compositor_frame(tab, LocalSurfaceId::default(), frame(Color::WHITE));
        assert!(display.surfaces.latest(tab).is_some());
        display.remove_frame_sink(tab);
        assert!(display.surfaces.latest(tab).is_none());
        assert!(display.needs_draw());
    }

    /// A frame whose pixels don't fit their resource's size, or a size
    /// past a texture's, is dropped: the frame before stays, and nothing
    /// is drawn again.
    #[test]
    fn a_frame_of_bad_resources_is_dropped() {
        use protocol::viz::{ResourceFormat, ResourceId, TransferableResource};
        let resource = |size: Size, bytes: usize| TransferableResource {
            id: ResourceId(1),
            size,
            format: ResourceFormat::Rgba8Premul,
            pixels: vec![0; bytes].into(),
            shared_image: None,
        };
        let with = |resource| CompositorFrame {
            resource_list: vec![resource],
            ..frame(Color::BLACK)
        };
        let mut display = Display::new(Renderer::default(), ROOT);
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::WHITE));
        display.draw_and_swap();
        for bad in [
            resource(Size::new(2, 2), 15),
            resource(Size::new(2, 2), 17),
            resource(
                Size::new(MAX_TEXTURE_SIZE + 1, 1),
                (MAX_TEXTURE_SIZE as usize + 1) * 4,
            ),
            resource(
                Size::new(1, MAX_TEXTURE_SIZE + 1),
                (MAX_TEXTURE_SIZE as usize + 1) * 4,
            ),
        ] {
            display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), with(bad));
            let latest = display.surfaces.latest(ROOT).unwrap();
            assert_eq!(latest.metadata.root_background_color, Color::WHITE);
            assert!(!display.needs_draw());
        }
        let good = [
            resource(Size::new(2, 2), 16),
            resource(
                Size::new(MAX_TEXTURE_SIZE, 1),
                MAX_TEXTURE_SIZE as usize * 4,
            ),
            resource(Size::new(0, 3), 0),
        ];
        for good in good {
            display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), with(good));
            let latest = display.surfaces.latest(ROOT).unwrap();
            assert_eq!(latest.metadata.root_background_color, Color::BLACK);
        }
    }

    /// `about:gpucrash` reaches the renderer, and a draw follows at once,
    /// which makes the new device.
    #[test]
    fn losing_the_context_loses_the_renderers_and_draws() {
        let root = FrameSinkId {
            client_id: 0,
            sink_id: 1,
        };
        let mut display = Display::new(Renderer::default(), root);
        assert!(!display.needs_draw());
        display.lose_context();
        assert_eq!(display.renderer().lost, 1);
        assert!(display.needs_draw());
    }

    /// A copy request draws again though nothing changed, and every request
    /// made before the draw gets that frame; later draws don't answer them
    /// again.
    #[test]
    fn copy_requests_get_the_next_frame_drawn() {
        let mut display = Display::new(Copying, ROOT);
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::WHITE));
        assert!(display.draw_and_swap());
        assert!(!display.needs_draw());

        let copies = Copies::default();
        display.request_copy_of_output(request(&copies));
        display.request_copy_of_output(request(&copies));
        assert!(display.needs_draw());
        assert!(copies.lock().unwrap().is_empty());
        assert!(display.draw_and_swap());
        let white = copy_of([255; 4]);
        assert_eq!(*copies.lock().unwrap(), [white.clone(), white]);

        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::BLACK));
        assert!(display.draw_and_swap());
        assert_eq!(copies.lock().unwrap().len(), 2);
    }

    /// Before anything can be drawn, a request waits for the first frame.
    #[test]
    fn a_copy_request_waits_for_a_frame() {
        let mut display = Display::new(Copying, ROOT);
        let copies = Copies::default();
        display.request_copy_of_output(request(&copies));
        assert!(!display.draw_and_swap());
        assert!(copies.lock().unwrap().is_empty());
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::BLACK));
        assert!(display.draw_and_swap());
        assert_eq!(*copies.lock().unwrap(), [copy_of([0, 0, 0, 255])]);
    }

    /// A frame drawn into nothing (a hidden window) answers no request:
    /// they wait for the next frame that is drawn.
    #[test]
    fn copy_requests_wait_for_a_frame_that_is_drawn() {
        /// Draws nothing until told it may.
        #[derive(Default)]
        struct Hidden {
            shown: bool,
        }
        impl DirectRenderer for Hidden {
            fn resize(&mut self, _: Size) {}
            fn draw_frame(&mut self, _: &AggregatedFrame<'_>) {}
            fn draw_frame_and_copy(
                &mut self,
                _: &AggregatedFrame<'_>,
            ) -> Result<CopyOutputResult, CopyFailure> {
                if self.shown {
                    Ok(copy_of([1, 2, 3, 255]).unwrap())
                } else {
                    Err(CopyFailure::NotDrawn)
                }
            }
        }
        let mut display = Display::new(Hidden::default(), ROOT);
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::WHITE));
        let copies = Copies::default();
        display.request_copy_of_output(request(&copies));
        assert!(display.draw_and_swap());
        assert!(copies.lock().unwrap().is_empty());
        display.renderer_mut().shown = true;
        display.set_needs_draw();
        assert!(display.draw_and_swap());
        assert_eq!(*copies.lock().unwrap(), [copy_of([1, 2, 3, 255])]);
    }

    /// A renderer that can't read its output back still draws, and answers
    /// `None`.
    #[test]
    fn a_renderer_that_cant_copy_answers_none() {
        let mut display = Display::new(Renderer::default(), ROOT);
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::WHITE));
        let copies = Copies::default();
        display.request_copy_of_output(request(&copies));
        assert!(display.draw_and_swap());
        assert_eq!(display.renderer().drawn, 1);
        assert_eq!(*copies.lock().unwrap(), [None]);
    }

    /// The root's frame embeds a tab's surface: a new frame from the tab is
    /// drawn too, though the root's hasn't changed.
    #[test]
    fn a_new_frame_from_an_embedded_surface_is_drawn() {
        let tab = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        let mut display = Display::new(Renderer::default(), ROOT);
        let mut root = frame(Color::WHITE);
        root.render_pass_list[0].quad_list.push(DrawQuad {
            rect: RectF::new(0.0, 0.0, 2.0, 1.0),
            shared_quad_state: 0,
            material: Material::Surface {
                surface_id: SurfaceId {
                    frame_sink_id: tab,
                    local_surface_id: LocalSurfaceId::default(),
                },
                default_background_color: Color::WHITE,
            },
        });
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), root);
        assert!(display.draw_and_swap());
        assert!(!display.needs_draw());
        display.submit_compositor_frame(tab, LocalSurfaceId::default(), frame(Color::BLACK));
        assert!(display.needs_draw());
        assert!(display.draw_and_swap());
        assert_eq!(display.renderer().drawn, 2);
    }

    /// A frame from a sink the root doesn't embed draws nothing, nor does
    /// one older than the root's surface.
    #[test]
    fn other_sinks_and_stale_surfaces_draw_nothing() {
        let mut display = Display::new(Renderer::default(), ROOT);
        let other = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        display.submit_compositor_frame(other, LocalSurfaceId::default(), frame(Color::WHITE));
        assert!(!display.needs_draw());
        assert!(!display.draw_and_swap());

        let newer = LocalSurfaceId {
            parent_sequence_number: 2,
            child_sequence_number: 1,
        };
        display.submit_compositor_frame(ROOT, newer, frame(Color::WHITE));
        assert!(display.draw_and_swap());
        display.submit_compositor_frame(ROOT, LocalSurfaceId::default(), frame(Color::WHITE));
        assert!(!display.needs_draw());
    }
}
