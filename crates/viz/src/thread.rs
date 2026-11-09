//! The Viz compositor thread ("VizCompositorThread" in Chromium).

use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, TryRecvError};
use protocol::viz::{CompositorFrameSinkMsg, DisplayMsg, FrameSinkId, VizMsg};

use crate::{BeginFrameSource, DirectRenderer, Display};

/// How often `BeginFrame`s go out: 60 per second, the display rate we
/// assume (there is no vsync signal to follow yet).
const BEGIN_FRAME_INTERVAL: Duration = Duration::from_micros(16_667);

/// How long a `BeginFrame` may go unanswered before it is logged.
const UNANSWERED: Duration = Duration::from_secs(1);

/// `by` after `instant`, an instant of now or just before: a second or a
/// frame later is well within the clock's range.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "an instant near now plus a second at most: within any clock's range"
)]
fn after(instant: Instant, by: Duration) -> Instant {
    instant + by
}

/// Spawns the Viz thread. `make_renderer` runs *on* that thread, so GPU
/// device creation and every draw happen there.
///
/// The loop blocks for a message, drains everything already queued (so a
/// burst of frames or resizes collapses into one draw), then draws once.
/// While a client of `begin_frames` asks for them, it also wakes every
/// `BEGIN_FRAME_INTERVAL` to send it a `BeginFrame` (their `frame_time` is
/// milliseconds since the thread started).
pub fn spawn_viz_compositor_thread<R, F>(
    rx: Receiver<VizMsg>,
    root_frame_sink: FrameSinkId,
    mut begin_frames: BeginFrameSource,
    make_renderer: F,
) -> std::io::Result<JoinHandle<()>>
where
    R: DirectRenderer,
    F: FnOnce() -> Result<R, String> + Send + 'static,
{
    std::thread::Builder::new()
        .name("VizCompositorThread".into())
        .spawn(move || {
            let renderer = match make_renderer() {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!("viz: failed to create renderer: {e}");
                    return;
                }
            };
            let mut display = Display::new(renderer, root_frame_sink);
            let start = Instant::now();
            let mut next_begin_frame = start;
            // When the last `BeginFrame` went out, and whether it has been
            // reported as unanswered: a client that never answers stops
            // every frame after it.
            let mut sent = start;
            let mut reported = false;
            'run: loop {
                let wait = if begin_frames.wants_begin_frame() {
                    Some(next_begin_frame.saturating_duration_since(Instant::now()))
                } else if begin_frames.is_pending() && !reported {
                    Some(after(sent, UNANSWERED).saturating_duration_since(Instant::now()))
                } else {
                    None
                };
                let first = if let Some(wait) = wait {
                    match rx.recv_timeout(wait) {
                        Ok(msg) => Some(msg),
                        Err(RecvTimeoutError::Timeout) => None,
                        Err(RecvTimeoutError::Disconnected) => break 'run,
                    }
                } else {
                    match rx.recv() {
                        Ok(msg) => Some(msg),
                        Err(_) => break 'run,
                    }
                };
                let mut next = first;
                while let Some(msg) = next.take() {
                    if let VizMsg::FrameSink(sink, msg) = &msg {
                        begin_frames.did_receive(*sink, msg);
                    }
                    match msg {
                        VizMsg::FrameSink(
                            sink,
                            CompositorFrameSinkMsg::SubmitCompositorFrame {
                                local_surface_id,
                                frame,
                            },
                        ) => display.submit_compositor_frame(sink, local_surface_id, frame),
                        VizMsg::FrameSink(_, CompositorFrameSinkMsg::SetNeedsBeginFrame(_))
                        | VizMsg::FrameSink(_, CompositorFrameSinkMsg::DidNotProduceFrame) => {}
                        VizMsg::Display(DisplayMsg::Resize(size)) => display.resize(size),
                        VizMsg::Display(DisplayMsg::ForceRedraw) => display.set_needs_draw(),
                        VizMsg::Display(DisplayMsg::LoseContext) => display.lose_context(),
                        VizMsg::Display(DisplayMsg::RequestCopyOfOutput(request)) => {
                            display.request_copy_of_output(request)
                        }
                        VizMsg::Display(DisplayMsg::Shutdown) => break 'run,
                        VizMsg::RegisterFrameSinkId(sink, observer) => begin_frames
                            .add_client(sink, Box::new(move |args| observer.on_begin_frame(args))),
                        VizMsg::InvalidateFrameSinkId(sink) => {
                            begin_frames.remove_client(sink);
                            display.remove_frame_sink(sink);
                        }
                    }
                    next = match rx.try_recv() {
                        Ok(m) => Some(m),
                        Err(TryRecvError::Empty) => None,
                        Err(TryRecvError::Disconnected) => break 'run,
                    };
                }
                if display.needs_draw() {
                    display.draw_and_swap();
                }
                let now = Instant::now();
                if begin_frames.wants_begin_frame() && now >= next_begin_frame {
                    begin_frames.issue(now.saturating_duration_since(start).as_secs_f64() * 1000.0);
                    next_begin_frame = after(now, BEGIN_FRAME_INTERVAL);
                    sent = now;
                    reported = false;
                }
                if begin_frames.is_pending() && !reported && now >= after(sent, UNANSWERED) {
                    tracing::warn!(
                        waited = ?now.saturating_duration_since(sent),
                        "viz: a BeginFrame is still unanswered; no other goes out until it is"
                    );
                    reported = true;
                }
            }
            tracing::debug!("viz: thread exiting");
        })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    use crossbeam_channel::unbounded;
    use gfx::Size;

    use gfx::{Color, RectF};
    use protocol::viz::{
        BeginFrameObserver, CompositorFrame, CompositorFrameMetadata, CompositorRenderPass,
        CopyOutputRequest, CopyOutputResult, LocalSurfaceId, RenderPassId, SharedQuadState,
    };

    use super::*;
    use crate::AggregatedFrame;

    /// Counts the times its device is lost.
    struct Renderer(Arc<AtomicU32>);

    impl DirectRenderer for Renderer {
        fn resize(&mut self, _: Size) {}
        fn draw_frame(&mut self, _: &AggregatedFrame<'_>) {}
        fn lose_context(&mut self) {
            #[expect(
                clippy::disallowed_methods,
                reason = "a test's count: far from wrapping"
            )]
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Reads back a 1×1 output of the frame's background colour: what the
    /// thread does with copies, not how a frame is drawn.
    struct Copying;

    impl DirectRenderer for Copying {
        fn resize(&mut self, _: Size) {}
        fn draw_frame(&mut self, _: &AggregatedFrame<'_>) {}
        fn draw_frame_and_copy(
            &mut self,
            frame: &AggregatedFrame<'_>,
        ) -> Result<CopyOutputResult, crate::CopyFailure> {
            Ok(CopyOutputResult {
                size: Size::new(1, 1),
                pixels: frame.background_color.premultiplied().to_vec(),
            })
        }
    }

    #[test]
    fn losing_the_context_reaches_the_renderer() {
        let lost = Arc::new(AtomicU32::new(0));
        let (tx, rx) = unbounded();
        let renderer = Renderer(lost.clone());
        let root = FrameSinkId {
            client_id: 0,
            sink_id: 1,
        };
        let thread =
            spawn_viz_compositor_thread(rx, root, BeginFrameSource::new(), move || Ok(renderer))
                .unwrap();
        tx.send(VizMsg::Display(DisplayMsg::LoseContext)).unwrap();
        tx.send(VizMsg::Display(DisplayMsg::Shutdown)).unwrap();
        thread.join().unwrap();
        assert_eq!(lost.load(Ordering::Relaxed), 1);
    }

    /// A tab registered while Viz runs gets `BeginFrame`s once it asks for
    /// them; invalidated, it gets no more, though it would answer.
    #[test]
    fn frame_sinks_come_and_go_while_viz_runs() {
        let (tx, rx) = unbounded();
        let root = FrameSinkId {
            client_id: 0,
            sink_id: 1,
        };
        let thread =
            spawn_viz_compositor_thread(rx, root, BeginFrameSource::new(), || Ok(Copying)).unwrap();
        let tab = FrameSinkId {
            client_id: 2,
            sink_id: 1,
        };
        // It answers each at once, so another follows every interval.
        let (frames_tx, frames) = unbounded();
        let answer = tx.clone();
        let observer = BeginFrameObserver::new(move |args| {
            let _ = frames_tx.send(args);
            let _ = answer.send(VizMsg::FrameSink(
                tab,
                CompositorFrameSinkMsg::DidNotProduceFrame,
            ));
        });
        tx.send(VizMsg::RegisterFrameSinkId(tab, observer)).unwrap();
        tx.send(VizMsg::FrameSink(
            tab,
            CompositorFrameSinkMsg::SetNeedsBeginFrame(true),
        ))
        .unwrap();
        for _ in 0..3 {
            frames
                .recv_timeout(Duration::from_secs(10))
                .expect("BeginFrames come");
        }
        tx.send(VizMsg::InvalidateFrameSinkId(tab)).unwrap();
        // One may have gone out before Viz had the message.
        std::thread::sleep(BEGIN_FRAME_INTERVAL * 3);
        while frames.try_recv().is_ok() {}
        std::thread::sleep(BEGIN_FRAME_INTERVAL * 6);
        assert!(frames.try_recv().is_err(), "no BeginFrame after it went");
        tx.send(VizMsg::Display(DisplayMsg::Shutdown)).unwrap();
        thread.join().unwrap();
    }

    /// A copy request reaches the display, which draws for it.
    #[test]
    fn a_copy_request_is_answered_with_the_next_frame() {
        let (tx, rx) = unbounded();
        let root = FrameSinkId {
            client_id: 0,
            sink_id: 1,
        };
        let thread =
            spawn_viz_compositor_thread(rx, root, BeginFrameSource::new(), || Ok(Copying)).unwrap();
        let rect = RectF::new(0.0, 0.0, 1.0, 1.0);
        let frame = CompositorFrame {
            metadata: CompositorFrameMetadata {
                device_scale_factor: 1.0,
                frame_token: 1,
                root_background_color: Color::BLACK,
            },
            resource_list: Vec::new(),
            render_pass_list: vec![CompositorRenderPass {
                id: RenderPassId(1),
                output_rect: rect,
                damage_rect: rect,
                shared_quad_state_list: vec![SharedQuadState::default()],
                quad_list: Vec::new(),
            }],
        };
        tx.send(VizMsg::FrameSink(
            root,
            CompositorFrameSinkMsg::SubmitCompositorFrame {
                local_surface_id: LocalSurfaceId::default(),
                frame,
            },
        ))
        .unwrap();
        let (copy_tx, copy_rx) = unbounded();
        let request = CopyOutputRequest::new(move |result| {
            let _ = copy_tx.send(result);
        });
        tx.send(VizMsg::Display(DisplayMsg::RequestCopyOfOutput(request)))
            .unwrap();
        let copy = copy_rx
            .recv_timeout(Duration::from_secs(10))
            .expect("the copy comes")
            .expect("read back");
        assert_eq!(copy.pixels, [0, 0, 0, 255]);
        tx.send(VizMsg::Display(DisplayMsg::Shutdown)).unwrap();
        thread.join().unwrap();
    }
}
