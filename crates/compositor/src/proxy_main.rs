//! `cc::ProxyMain`: the main thread's side of a threaded `LayerTreeHost`,
//! and the compositor thread's queue and loop.

use crossbeam_channel::{Receiver, Sender};
use gfx::PointF;
use protocol::renderer::{CompositorMsg, RendererHostMsg};

use crate::layer_tree_host_impl::CommitState;
use crate::proxy_impl::{ProxyImpl, ToMainThread};
use crate::{LayerTreeFrameSink, LayerTreeHost, LayerTreeSettings, SwapPromise};

/// One task for the compositor thread. Everything it does arrives here, in
/// the order it was posted, whichever thread posted it.
pub(crate) enum CompositorTask {
    /// From Viz or the browser.
    External(CompositorMsg),
    /// `ProxyImpl::InitializeImplOnImpl`: the impl side, made on the main
    /// thread with its frame sink and handed over.
    Initialize(Box<ProxyImpl>),
    /// A main frame's commit.
    Commit(Box<CommitState>),
    /// A main frame with nothing to commit.
    BeginMainFrameAborted,
    /// Whether the main thread wants main frames.
    SetNeedsBeginMainFrame(bool),
    /// Whether the tab is shown.
    SetVisible(bool),
    /// The main side has gone (`ProxyMain::Stop`).
    Shutdown,
}

/// A new compositor thread's queue: the sender for Viz and the browser, and
/// the compositor thread's end.
pub fn channel() -> (CompositorSender, CompositorReceiver) {
    let (tx, rx) = crossbeam_channel::unbounded();
    (CompositorSender(tx.clone()), CompositorReceiver { rx, tx })
}

/// How Viz and the browser reach a renderer's compositor thread (the
/// `mojo::Remote`s bound there). Each message becomes a task on its queue,
/// posted from the thread that sends it.
#[derive(Clone)]
pub struct CompositorSender(Sender<CompositorTask>);

impl CompositorSender {
    /// Fails once the compositor thread has gone; the message is dropped.
    pub fn send(&self, msg: CompositorMsg) -> Result<(), CompositorGone> {
        self.0
            .send(CompositorTask::External(msg))
            .map_err(|_| CompositorGone)
    }
}

/// The compositor thread has gone, and its queue with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompositorGone;

/// The compositor thread's end of its queue. Before the thread takes it,
/// the main thread's `LayerTreeHost` gets its own way in from it
/// ([`LayerTreeHost::new_threaded`]).
pub struct CompositorReceiver {
    rx: Receiver<CompositorTask>,
    /// For the main side's `ProxyMain`.
    tx: Sender<CompositorTask>,
}

impl CompositorReceiver {
    /// The compositor thread's loop: runs the tasks in order until the main
    /// side shuts it down (its `LayerTreeHost` dropped). Viz may still hold
    /// a sender then; what it sends after is dropped.
    pub fn run(self) {
        let CompositorReceiver { rx, tx } = self;
        drop(tx);
        let mut proxy: Option<ProxyImpl> = None;
        while let Ok(task) = rx.recv() {
            let task = match task {
                CompositorTask::Initialize(impl_side) => {
                    proxy = Some(*impl_side);
                    continue;
                }
                CompositorTask::Shutdown => break,
                task => task,
            };
            let Some(proxy) = proxy.as_mut() else {
                tracing::warn!("compositor task before the impl side was initialized");
                continue;
            };
            match task {
                CompositorTask::External(CompositorMsg::BeginFrame(args)) => {
                    proxy.begin_frame(args)
                }
                CompositorTask::External(CompositorMsg::HandleInputEvent(event)) => {
                    proxy.handle_input_event(event)
                }
                CompositorTask::External(CompositorMsg::ContextLost) => {
                    proxy.did_lose_layer_tree_frame_sink()
                }
                CompositorTask::Commit(state) => proxy.commit(*state),
                CompositorTask::BeginMainFrameAborted => proxy.begin_main_frame_aborted(),
                CompositorTask::SetNeedsBeginMainFrame(needs) => {
                    proxy.set_needs_begin_main_frame(needs)
                }
                CompositorTask::SetVisible(visible) => proxy.set_visible(visible),
                CompositorTask::Initialize(_) | CompositorTask::Shutdown => {
                    unreachable!("handled above")
                }
            }
        }
        tracing::debug!("compositor thread exiting");
    }
}

/// `cc::ProxyMain`: the main thread's side of a threaded `LayerTreeHost`.
/// Everything it does is a task posted to the compositor thread; it never
/// waits for one. Dropping it shuts the compositor thread down.
pub struct ProxyMain {
    to_impl: Sender<CompositorTask>,
    /// What the impl side was last told.
    needs_begin_main_frame: bool,
    /// The viewport's offset the last commit carried, CSS px. Moving it
    /// takes a commit, which keeps the tiles.
    committed_scroll_offset: PointF,
    /// The tab is shown (`LayerTreeHost::IsVisible`).
    visible: bool,
}

impl ProxyMain {
    fn post(&self, task: CompositorTask) {
        if self.to_impl.send(task).is_err() {
            tracing::debug!("compositor thread is gone; dropping task");
        }
    }
}

impl Drop for ProxyMain {
    fn drop(&mut self) {
        self.post(CompositorTask::Shutdown);
    }
}

impl LayerTreeHost<ProxyMain> {
    /// `LayerTreeHost::CreateThreaded`: the impl side, with `frame_sink`,
    /// goes to whichever thread runs `compositor` ([`CompositorReceiver::run`]).
    /// `post_to_main_thread` is how the compositor asks things of the main
    /// thread, from the compositor thread; `to_browser` takes the acks of
    /// the input it handles itself.
    pub fn new_threaded(
        settings: LayerTreeSettings,
        frame_sink: Box<dyn LayerTreeFrameSink>,
        compositor: &CompositorReceiver,
        post_to_main_thread: impl FnMut(ToMainThread) + Send + 'static,
        to_browser: Sender<RendererHostMsg>,
    ) -> Self {
        let proxy = ProxyMain {
            to_impl: compositor.tx.clone(),
            needs_begin_main_frame: false,
            committed_scroll_offset: PointF::new(0.0, 0.0),
            visible: true,
        };
        let impl_side = ProxyImpl::new(
            settings,
            frame_sink,
            Box::new(post_to_main_thread),
            to_browser,
        );
        proxy.post(CompositorTask::Initialize(Box::new(impl_side)));
        Self::with_proxy(proxy)
    }

    /// Whether the main thread wants main frames. Posted only when it
    /// changes.
    pub fn set_needs_begin_main_frame(&mut self, needs: bool) {
        if needs != self.proxy.needs_begin_main_frame {
            self.proxy.needs_begin_main_frame = needs;
            self.proxy
                .post(CompositorTask::SetNeedsBeginMainFrame(needs));
        }
    }

    /// `LayerTreeHost::SetVisible`: a hidden tab's compositor asks for no
    /// `BeginFrame`s, so there are no main frames either; shown again, it
    /// draws.
    pub fn set_visible(&mut self, visible: bool) {
        self.proxy.visible = visible;
        self.proxy.post(CompositorTask::SetVisible(visible));
    }

    /// `LayerTreeHost::IsVisible`.
    pub fn is_visible(&self) -> bool {
        self.proxy.visible
    }

    /// Ends the main frame the compositor asked for. Commits what changed,
    /// a moved viewport included, with `promises`; promises commit even if
    /// nothing changed, so that a frame answers them. With nothing to
    /// commit, tells the compositor so (`BeginMainFrameAborted`).
    pub fn commit(&mut self, promises: Vec<SwapPromise>) {
        if self.root_scroll_offset != self.proxy.committed_scroll_offset || !promises.is_empty() {
            self.set_needs_commit();
        }
        match self.take_commit_state() {
            Some(mut state) => {
                self.proxy.committed_scroll_offset = state.root_scroll_offset;
                state.swap_promises = promises;
                self.proxy.post(CompositorTask::Commit(Box::new(state)));
            }
            None => self.proxy.post(CompositorTask::BeginMainFrameAborted),
        }
    }

    /// Ends the main frame the compositor asked for without updating
    /// anything (nothing asked for an update, no document yet, or
    /// rendering is blocked).
    pub fn abort_main_frame(&mut self) {
        self.proxy.post(CompositorTask::BeginMainFrameAborted);
    }
}

#[cfg(test)]
mod tests {
    use crate::{DocumentId, InputDispatch};
    use std::sync::Arc;
    use std::time::Duration;

    use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
    use crossbeam_channel::unbounded;
    use gfx::{Color, RectF, Size, SizeF};
    use protocol::input::{InputEvent, MouseButton};
    use protocol::viz::{BeginFrameArgs, CompositorFrame, LocalSurfaceId};

    use super::*;
    use crate::fake_gpu::FakeGpu;
    use crate::{LayerId, PictureLayer, ScrollNode};

    const TIMEOUT: Duration = Duration::from_secs(10);

    fn layer(color: Color) -> PictureLayer {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 100.0, 100.0),
            flags: PaintFlags::fill(color),
        });
        list.finalize();
        PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 100.0),
            display_list: Arc::new(list),
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        }
    }

    /// A threaded host whose compositor thread doesn't run: the tests read
    /// what the main side posts.
    fn host() -> (LayerTreeHost<ProxyMain>, Receiver<CompositorTask>) {
        let (_viz, compositor) = channel();
        let host = LayerTreeHost::new_threaded(
            LayerTreeSettings::default(),
            Box::new(|_: LocalSurfaceId, _: CompositorFrame| {}),
            &compositor,
            |_| {},
            unbounded().0,
        );
        let rx = compositor.rx.clone();
        assert!(matches!(rx.try_recv(), Ok(CompositorTask::Initialize(_))));
        (host, rx)
    }

    fn posted(rx: &Receiver<CompositorTask>) -> Vec<&'static str> {
        rx.try_iter()
            .map(|task| match task {
                CompositorTask::External(_) => "External",
                CompositorTask::Initialize(_) => "Initialize",
                CompositorTask::Commit(_) => "Commit",
                CompositorTask::BeginMainFrameAborted => "BeginMainFrameAborted",
                CompositorTask::SetNeedsBeginMainFrame(true) => "SetNeedsBeginMainFrame(true)",
                CompositorTask::SetNeedsBeginMainFrame(false) => "SetNeedsBeginMainFrame(false)",
                CompositorTask::SetVisible(true) => "SetVisible(true)",
                CompositorTask::SetVisible(false) => "SetVisible(false)",
                CompositorTask::Shutdown => "Shutdown",
            })
            .collect()
    }

    #[test]
    fn a_main_frame_commits_what_changed_or_aborts() {
        let (mut host, rx) = host();
        host.set_root_layer(layer(Color::WHITE));
        host.commit(Vec::new());
        assert_eq!(posted(&rx), ["Commit"]);
        host.commit(Vec::new());
        assert_eq!(posted(&rx), ["BeginMainFrameAborted"], "nothing changed");
        host.abort_main_frame();
        assert_eq!(posted(&rx), ["BeginMainFrameAborted"]);
    }

    #[test]
    fn a_moved_viewport_commits_with_its_offset() {
        let (mut host, rx) = host();
        host.set_root_layer(layer(Color::WHITE));
        host.commit(Vec::new());
        rx.try_iter().for_each(drop);

        host.set_root_scroll_offset(PointF::new(0.0, 40.0));
        host.commit(Vec::new());
        match rx.try_recv() {
            Ok(CompositorTask::Commit(state)) => {
                assert_eq!(state.root_scroll_offset, PointF::new(0.0, 40.0))
            }
            _ => panic!("expected a commit"),
        }
        host.commit(Vec::new());
        assert_eq!(posted(&rx), ["BeginMainFrameAborted"], "it moved once");
    }

    #[test]
    fn a_new_viewport_scroll_node_commits() {
        let (mut host, rx) = host();
        host.set_root_layer(layer(Color::WHITE));
        host.commit(Vec::new());
        rx.try_iter().for_each(drop);

        let node = ScrollNode {
            container_bounds: SizeF::new(100.0, 100.0),
            bounds: SizeF::new(100.0, 300.0),
            user_scrollable_horizontal: true,
            user_scrollable_vertical: true,
        };
        host.set_viewport_scroll_node(Some(node));
        host.commit(Vec::new());
        match rx.try_recv() {
            Ok(CompositorTask::Commit(state)) => assert_eq!(state.viewport_scroll_node, Some(node)),
            _ => panic!("expected a commit"),
        }
        host.set_viewport_scroll_node(Some(node));
        host.commit(Vec::new());
        assert_eq!(posted(&rx), ["BeginMainFrameAborted"], "the same node");
    }

    #[test]
    fn promises_commit_even_with_nothing_changed() {
        let (mut host, rx) = host();
        host.set_root_layer(layer(Color::WHITE));
        host.commit(Vec::new());
        rx.try_iter().for_each(drop);

        host.commit(vec![SwapPromise::new(|_| {})]);
        match rx.try_recv() {
            Ok(CompositorTask::Commit(state)) => assert_eq!(state.swap_promises.len(), 1),
            _ => panic!("expected a commit"),
        }
    }

    #[test]
    fn needing_main_frames_is_posted_when_it_changes() {
        let (mut host, rx) = host();
        host.set_needs_begin_main_frame(true);
        host.set_needs_begin_main_frame(true);
        host.set_needs_begin_main_frame(false);
        assert_eq!(
            posted(&rx),
            [
                "SetNeedsBeginMainFrame(true)",
                "SetNeedsBeginMainFrame(false)"
            ]
        );
    }

    #[test]
    fn dropping_the_host_shuts_the_compositor_down() {
        let (host, rx) = host();
        drop(host);
        assert_eq!(posted(&rx), ["Shutdown"]);
    }

    /// The compositor doesn't watch the GPU itself: the browser tells it,
    /// with a message, once Viz has lost it (the renderer reaches nothing
    /// of Viz's but through messages).
    #[test]
    fn the_compositor_doesnt_watch_the_gpu() {
        let gpu = Arc::new(FakeGpu::default());
        let (_viz, compositor) = channel();
        let _host = LayerTreeHost::new_threaded(
            LayerTreeSettings {
                shared_image_interface: Some(gpu.clone()),
                ..LayerTreeSettings::default()
            },
            Box::new(|_: LocalSurfaceId, _: CompositorFrame| {}),
            &compositor,
            |_| {},
            unbounded().0,
        );
        assert_eq!(gpu.lose(), 0, "no observer");
    }

    /// What reached Viz from the compositor thread.
    #[derive(Debug, PartialEq)]
    enum ToViz {
        Frame(u64),
        NeedsBeginFrame(bool),
        DidNotProduceFrame,
    }

    struct ChannelSink(Sender<ToViz>);

    impl LayerTreeFrameSink for ChannelSink {
        fn submit_compositor_frame(&mut self, _: LocalSurfaceId, frame: CompositorFrame) {
            let _ = self.0.send(ToViz::Frame(frame.metadata.frame_token));
        }
        fn set_needs_begin_frame(&mut self, needs: bool) {
            let _ = self.0.send(ToViz::NeedsBeginFrame(needs));
        }
        fn did_not_produce_frame(&mut self) {
            let _ = self.0.send(ToViz::DidNotProduceFrame);
        }
    }

    #[test]
    fn a_main_frame_round_trip_through_the_compositor_thread() {
        let (viz_tx, viz) = unbounded();
        let (main_tx, main) = unbounded();
        let (viz_sender, compositor) = channel();
        let gpu = Arc::new(FakeGpu::default());
        let mut host = LayerTreeHost::new_threaded(
            LayerTreeSettings {
                shared_image_interface: Some(gpu.clone()),
                ..LayerTreeSettings::default()
            },
            Box::new(ChannelSink(viz_tx)),
            &compositor,
            move |task| main_tx.send(task).unwrap(),
            unbounded().0,
        );
        let thread = std::thread::Builder::new()
            .name("Compositor".into())
            .spawn(move || compositor.run())
            .unwrap();
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        host.set_root_layer(layer(Color::rgb(255, 0, 0)));
        host.set_document_id(DocumentId(3));

        host.set_needs_begin_main_frame(true);
        assert_eq!(viz.recv_timeout(TIMEOUT), Ok(ToViz::NeedsBeginFrame(true)));
        let args = BeginFrameArgs {
            sequence: 0,
            frame_time: 0.0,
        };
        viz_sender.send(CompositorMsg::BeginFrame(args)).unwrap();
        assert_eq!(
            main.recv_timeout(TIMEOUT),
            Ok(ToMainThread::BeginMainFrame { args, scroll: None }),
            "a main frame is asked for"
        );

        // As the renderer does: commit, then (at the end of its loop) stop
        // wanting main frames.
        let (token_tx, token) = unbounded();
        host.commit(vec![SwapPromise::new(move |t| token_tx.send(t).unwrap())]);
        host.set_needs_begin_main_frame(false);
        let Ok(ToViz::Frame(frame_token)) = viz.recv_timeout(TIMEOUT) else {
            panic!("expected a frame");
        };
        assert_eq!(token.recv_timeout(TIMEOUT), Ok(Some(frame_token)));
        assert_eq!(viz.recv_timeout(TIMEOUT), Ok(ToViz::NeedsBeginFrame(false)));

        let click = || {
            InputEvent::mouse_down(
                1.0,
                2.0,
                MouseButton::Left,
                protocol::input::Modifiers::NONE,
            )
        };
        viz_sender
            .send(CompositorMsg::HandleInputEvent(click()))
            .unwrap();
        assert_eq!(
            main.recv_timeout(TIMEOUT),
            Ok(ToMainThread::Input {
                event: click(),
                attribution: crate::proxy_impl::InputAttribution::TargetedFrame(None),
                document: Some(DocumentId(3)),
                scroll: None,
                dispatch: InputDispatch::Blocking { cancelable: true },
            }),
            "input goes on to the main thread, aimed at the drawn document"
        );

        // The GPU lost, as the browser tells it: the compositor thread asks
        // for a `BeginFrame` and draws. (The layer is of one color: no tile
        // was on the GPU.)
        viz_sender.send(CompositorMsg::ContextLost).unwrap();
        assert_eq!(viz.recv_timeout(TIMEOUT), Ok(ToViz::NeedsBeginFrame(true)));
        viz_sender
            .send(CompositorMsg::BeginFrame(BeginFrameArgs {
                sequence: 1,
                frame_time: 16.0,
            }))
            .unwrap();
        assert!(matches!(viz.recv_timeout(TIMEOUT), Ok(ToViz::Frame(_))));

        drop(host);
        thread.join().unwrap();
        assert!(
            viz_sender
                .send(CompositorMsg::BeginFrame(args))
                .is_err_and(|e| e == CompositorGone),
            "the compositor thread has gone"
        );
    }
}
