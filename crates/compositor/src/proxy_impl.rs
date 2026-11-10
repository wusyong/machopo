//! `cc::ProxyImpl`, with the part of `cc::Scheduler` machopo has so far: the impl
//! side of a threaded `LayerTreeHost`, which lives on the compositor thread.

use gfx::PointF;
use protocol::input::InputEvent;
use protocol::viz::BeginFrameArgs;

use crossbeam_channel::Sender;
use protocol::renderer::RendererHostMsg;

use crate::DocumentId;
use crate::input_handler::{self, EventDisposition};
use crate::layer_tree_host_impl::{CommitState, LayerTreeHostImpl};
use crate::{ElementId, LayerTreeFrameSink, LayerTreeSettings, SwapPromise};

/// What the compositor thread asks of the main thread. A main frame and
/// an input event each carry what the compositor scrolled since it last
/// handed that over: the main thread applies it first thing, before
/// anything else in the task.
#[derive(Debug, Clone, PartialEq)]
pub enum ToMainThread {
    /// Run a main frame for this `BeginFrame` (`ProxyMain::BeginMainFrame`),
    /// and end it with a commit or an abort.
    BeginMainFrame {
        args: BeginFrameArgs,
        scroll: Option<CompositorScrollDelta>,
    },
    /// An input event the compositor didn't handle itself (Blink's
    /// `InputHandlerProxy` answering `DID_NOT_HANDLE`, which hands it to the
    /// main thread's event queue). `document` is the document of the last
    /// frame drawn, the one the user aimed at; `None` before any.
    /// `attribution` is the frame it is for, as the compositor tells.
    Input {
        event: InputEvent,
        attribution: InputAttribution,
        document: Option<DocumentId>,
        scroll: Option<CompositorScrollDelta>,
        dispatch: InputDispatch,
    },
    /// A tile's raster panicked and the tile was left blank, the `count`th
    /// time for this compositor: for the page's log (as cc tells its
    /// client of a lost context, which Blink writes to the console). Sent
    /// the first time and when the count doubles.
    RasterFailed { message: String, count: u64 },
}

/// Which frame an input event the compositor hands on is for (Blink's
/// `WebInputEventAttribution`, which `InputHandlerProxy::
/// PerformEventAttribution` makes): what `navigator.scheduling.
/// isInputPending()` tells pages of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputAttribution {
    /// It can't be told (`kUnknown`).
    Unknown,
    /// The frame with the focus (`kFocusedFrame`): a key's.
    FocusedFrame,
    /// The frame whose document is the element (`kTargetedFrame`): a
    /// pointer's, the frame under it; `None` for the drawn document's.
    TargetedFrame(Option<ElementId>),
}

impl InputAttribution {
    /// The attribution of `event` (Blink's `PerformEventAttribution`): a
    /// key's is the focused frame's, a mouse's and a wheel's the frame
    /// under them, as `host_impl` finds it.
    pub(crate) fn of(event: &InputEvent, host_impl: &LayerTreeHostImpl) -> Self {
        match *event {
            InputEvent::Key(_) => Self::FocusedFrame,
            InputEvent::MouseMove { x, y, .. }
            | InputEvent::MouseDown { x, y, .. }
            | InputEvent::MouseUp { x, y, .. }
            | InputEvent::MouseWheel { x, y, .. } => host_impl.frame_attribution(PointF::new(x, y)),
        }
    }
}

/// How the main thread takes an input the compositor hands it (Blink's
/// `WebInputEvent::DispatchType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputDispatch {
    /// The main thread handles it and acks it (`kBlocking`); a wheel's
    /// `wheel` event is cancelable only if `cancelable` (it was in the wheel
    /// event handler region), and the main thread scrolls unless it was
    /// canceled.
    Blocking { cancelable: bool },
    /// The compositor handled the wheel (scrolled, acked): the main thread
    /// only dispatches its `wheel` event, not cancelable (cc's
    /// `DID_HANDLE_NON_BLOCKING`, `kListenersNonBlockingPassive`).
    NonBlocking,
}

/// How far the compositor scrolled the viewport and each container, CSS
/// px, in which document (the drawn one: the compositor scrolls no other;
/// cc's `CompositorCommitData`). The main thread applies it only to that
/// document, each part clamped to what it can scroll; a later commit
/// confirms it.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositorScrollDelta {
    pub document: DocumentId,
    pub viewport: PointF,
    /// The containers that moved, by element, in element order.
    pub containers: Vec<(ElementId, PointF)>,
}

/// Posts a task to the main thread (Chromium's `ProxyImpl` posts to the
/// main thread's task runner). The compositor can't name the main thread's
/// task type, so whoever starts the compositor thread hands this in.
pub(crate) type PostToMainThread = Box<dyn FnMut(ToMainThread) + Send>;

/// `cc::ProxyImpl`: owns the impl side of a threaded `LayerTreeHost` and
/// decides what each of Viz's `BeginFrame`s does.
///
/// With `wait_for_all_pipeline_stages_before_draw` (cc's setting) it waits
/// for the main thread: a `BeginFrame` that starts a main frame is answered
/// once the main frame's commit, or its abort, comes back, with the frame
/// drawn from it. Without, it answers every `BeginFrame` at once from the
/// active tree, and a commit is drawn at the next. Raster runs inline, so a
/// commit is activated as soon as it arrives.
///
/// Plain methods, no thread: the compositor thread calls one for each task,
/// and tests drive them step by step.
pub(crate) struct ProxyImpl {
    host_impl: LayerTreeHostImpl,
    post_to_main_thread: PostToMainThread,
    /// For the acks of the input handled here (`WidgetInputHandler`'s
    /// callback, which Blink runs from the compositor thread for what it
    /// handled there), so they don't wait for a busy main thread.
    to_browser: Sender<RendererHostMsg>,
    /// The main thread wants main frames (it has animation frame callbacks,
    /// or something changed). The main thread sets and clears it.
    needs_begin_main_frame: bool,
    /// A `BeginMainFrame` went out and neither its commit nor its abort has
    /// come back.
    main_frame_in_flight: bool,
    /// The `BeginFrame` that started the main frame in flight is still
    /// unanswered; Viz sends no other until it is.
    begin_frame_pending: bool,
    /// Whether a `BeginFrame` that starts a main frame waits for its commit
    /// before drawing (`LayerTreeSettings`).
    wait_for_all_pipeline_stages_before_draw: bool,
    /// Something was activated, or scrolled, since the last draw.
    needs_redraw: bool,
    /// What the frame sink last told Viz (`SetNeedsBeginFrame`).
    needs_begin_frame: bool,
    /// The tab is shown (the scheduler's visibility).
    visible: bool,
}

impl ProxyImpl {
    pub(crate) fn new(
        settings: LayerTreeSettings,
        frame_sink: Box<dyn LayerTreeFrameSink>,
        post_to_main_thread: PostToMainThread,
        to_browser: Sender<RendererHostMsg>,
    ) -> Self {
        Self {
            wait_for_all_pipeline_stages_before_draw: settings
                .wait_for_all_pipeline_stages_before_draw,
            host_impl: LayerTreeHostImpl::new(settings, frame_sink),
            post_to_main_thread,
            to_browser,
            needs_begin_main_frame: false,
            main_frame_in_flight: false,
            begin_frame_pending: false,
            needs_redraw: false,
            needs_begin_frame: false,
            visible: true,
        }
    }

    /// `DidLoseLayerTreeFrameSink`: the GPU was lost and Viz has a new one.
    /// The tiles on the old one are rastered again and drawn at the next
    /// `BeginFrame`.
    pub(crate) fn did_lose_layer_tree_frame_sink(&mut self) {
        self.host_impl.did_lose_layer_tree_frame_sink();
        self.needs_redraw = true;
        self.update_needs_begin_frame();
    }

    /// `Scheduler::SetVisible`: hidden, it asks for no `BeginFrame`s, and
    /// the input acks waiting for a frame are answered without one; shown
    /// again, it draws what it has at the next one.
    pub(crate) fn set_visible(&mut self, visible: bool) {
        if visible == self.visible {
            return;
        }
        self.visible = visible;
        if visible {
            self.needs_redraw = true;
        }
        self.break_input_acks_if_hidden();
        self.update_needs_begin_frame();
    }

    /// A hidden tab draws no frame for an input ack to wait for.
    fn break_input_acks_if_hidden(&mut self) {
        if !self.visible {
            self.host_impl.break_input_acks();
        }
    }

    /// From the main thread: whether it wants main frames.
    pub(crate) fn set_needs_begin_main_frame(&mut self, needs: bool) {
        self.needs_begin_main_frame = needs;
        self.update_needs_begin_frame();
    }

    /// `OnBeginFrame`: a frame is due. Starts a main frame if the main
    /// thread wants one, or has scrolling to get or confirm, and isn't
    /// still on the last. Waiting for the main thread, answers Viz once that
    /// comes back; otherwise, and when no main frame starts, draws what is
    /// new now, or answers that nothing is.
    pub(crate) fn begin_frame(&mut self, args: BeginFrameArgs) {
        debug_assert!(
            !self.begin_frame_pending,
            "Viz sends no BeginFrame before the last is answered"
        );
        let wants_main_frame =
            self.needs_begin_main_frame || self.host_impl.scroll_needs_main_frame();
        let starts_main_frame = wants_main_frame && !self.main_frame_in_flight;
        if starts_main_frame {
            self.send_begin_main_frame(args);
        }
        if starts_main_frame && self.wait_for_all_pipeline_stages_before_draw {
            self.begin_frame_pending = true;
        } else {
            self.draw_or_answer();
        }
        self.update_needs_begin_frame();
    }

    /// `ScheduledActionSendBeginMainFrame`: asks the main thread for a main
    /// frame, handing it what was scrolled here. The commit or abort that
    /// ends it answers this.
    fn send_begin_main_frame(&mut self, args: BeginFrameArgs) {
        self.main_frame_in_flight = true;
        let scroll = self.host_impl.take_scroll_for_main_frame();
        (self.post_to_main_thread)(ToMainThread::BeginMainFrame { args, scroll });
    }

    /// An input event from the browser. What the compositor handles (the
    /// viewport's scrolling) it acknowledges itself: with the next frame,
    /// which shows the scroll, or at once if nothing moved. The rest goes to
    /// the main thread, tagged with the document of the last frame drawn
    /// (the main thread drops one aimed at a document it no longer shows),
    /// taking along what was scrolled here, so the main thread hit tests
    /// where the user sees things; a commit will have to confirm that.
    pub(crate) fn handle_input_event(&mut self, event: InputEvent) {
        let disposition = input_handler::handle_input_event(&mut self.host_impl, &event);
        // A wheel the compositor scrolled or dropped still goes to the page's
        // listeners, if it may have some, which can't cancel it.
        let non_blocking = matches!(event, InputEvent::MouseWheel { .. })
            && !matches!(disposition, EventDisposition::DidNotHandle { .. })
            && self.host_impl.wheel_event_listeners();
        let forwarded = non_blocking.then(|| event.clone());
        match disposition {
            EventDisposition::DidHandle => {
                self.did_scroll();
                let to_browser = self.to_browser.clone();
                self.host_impl
                    .queue_swap_promise(SwapPromise::input_ack(move |frame_token| {
                        let _ = to_browser.send(RendererHostMsg::InputEventAck { frame_token });
                    }));
                self.break_input_acks_if_hidden();
            }
            EventDisposition::DropEvent => {
                let _ = self
                    .to_browser
                    .send(RendererHostMsg::InputEventAck { frame_token: None });
            }
            EventDisposition::DidNotHandle { cancelable } => {
                self.post_input(event, InputDispatch::Blocking { cancelable });
            }
        }
        if let Some(event) = forwarded {
            self.post_input(event, InputDispatch::NonBlocking);
        }
    }

    /// Hands `event` to the main thread, aimed at the drawn document, with
    /// the scrolling the main thread hasn't had yet (where the user sees
    /// things). Whether a main frame is wanted doesn't change: what was
    /// unsent now waits for a commit to confirm it, which wants one too.
    fn post_input(&mut self, event: InputEvent, dispatch: InputDispatch) {
        let document = self.host_impl.drawn_document();
        let scroll = self.host_impl.take_scroll_for_input();
        let attribution = InputAttribution::of(&event, &self.host_impl);
        (self.post_to_main_thread)(ToMainThread::Input {
            event,
            attribution,
            document,
            scroll,
            dispatch,
        });
    }

    /// The impl side scrolled the viewport: drawn at the next `BeginFrame`,
    /// and handed to the main thread with the next main frame or forwarded
    /// input.
    fn did_scroll(&mut self) {
        self.needs_redraw = true;
        self.update_needs_begin_frame();
    }

    /// Scrolls the viewport by `delta` (CSS px) as a wheel outside any
    /// container would, without its ack. Whether it moved.
    #[cfg(test)]
    fn scroll_viewport(&mut self, delta: PointF) -> bool {
        if !self.host_impl.scroll_viewport(delta) {
            return false;
        }
        self.did_scroll();
        true
    }

    /// Scrolls from the container that is `element`'s by `delta` (CSS
    /// px), as a wheel over it would, without its ack. Whether anything
    /// moved.
    #[cfg(test)]
    fn scroll_container(&mut self, element: ElementId, delta: PointF) -> bool {
        let Some(node) = self.host_impl.scroll_node_of(element) else {
            return false;
        };
        if !self.host_impl.scroll_from(Some(node), delta) {
            return false;
        }
        self.did_scroll();
        true
    }

    /// A main frame's commit: built into the pending tree, rastered and
    /// activated right away, then drawn for the `BeginFrame` it was for. A
    /// commit no `BeginFrame` waits for is drawn at the next.
    pub(crate) fn commit(&mut self, state: CommitState) {
        // What it confirms of the scrolling handed over relies on this.
        debug_assert!(self.main_frame_in_flight, "a commit ends a main frame");
        self.main_frame_in_flight = false;
        self.host_impl.commit(state);
        // A main frame that started before the tab was hidden.
        self.break_input_acks_if_hidden();
        self.host_impl.prepare_tiles();
        self.host_impl.activate();
        self.needs_redraw = true;
        self.finish_begin_frame();
        self.update_needs_begin_frame();
    }

    /// `BeginMainFrameAborted`: the main frame had nothing to commit.
    pub(crate) fn begin_main_frame_aborted(&mut self) {
        debug_assert!(self.main_frame_in_flight, "an abort ends a main frame");
        self.main_frame_in_flight = false;
        self.finish_begin_frame();
        self.update_needs_begin_frame();
    }

    /// Tells the main thread of the tile whose raster failed, if it is
    /// one to tell of: before a frame is drawn, whether the tile was
    /// rastered for it just now or for the commit it shows.
    fn tell_raster_failure(&mut self) {
        if let Some(raster::RasterFailure { message, count }) = self.host_impl.take_raster_failure()
        {
            (self.post_to_main_thread)(ToMainThread::RasterFailed { message, count });
        }
    }

    /// Answers the `BeginFrame` the main frame was for, if it waits.
    fn finish_begin_frame(&mut self) {
        if std::mem::take(&mut self.begin_frame_pending) {
            self.draw_or_answer();
        }
    }

    /// Answers a `BeginFrame`: with a frame if something new was activated
    /// and there is anything to draw, else `DidNotProduceFrame`.
    fn draw_or_answer(&mut self) {
        if std::mem::take(&mut self.needs_redraw) {
            self.host_impl.prepare_tiles_if_scrolled();
            self.tell_raster_failure();
            if self.host_impl.draw_and_submit().is_some() {
                return;
            }
        }
        self.host_impl.frame_sink().did_not_produce_frame();
    }

    /// Asks Viz for `BeginFrame`s while shown and the main thread wants
    /// main frames, the main thread has scrolling to get or confirm, or
    /// something waits to be drawn. Sent only when it changes.
    fn update_needs_begin_frame(&mut self) {
        let needs = self.visible
            && (self.needs_begin_main_frame
                || self.host_impl.scroll_needs_main_frame()
                || self.needs_redraw);
        if needs != self.needs_begin_frame {
            self.needs_begin_frame = needs;
            self.host_impl.frame_sink().set_needs_begin_frame(needs);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Step by step, as cc's `LayerTreeTest` does: each test plays the main
    //! thread and Viz, calling the methods the compositor thread's tasks
    //! would, and checks what reached Viz and the main thread.

    use std::sync::{Arc, Mutex};

    use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
    use gfx::{Color, RectF, Size, SizeF};
    use protocol::viz::{CompositorFrame, LocalSurfaceId};

    use super::*;
    use crate::{
        ClipNode, ClipNodeId, LayerId, LayerProperties, LayerTreeHost, PictureLayer, PropertyTrees,
        ScrollNode, ScrollOffsets, ScrollTreeNode, SwapPromise, TransformNode, TransformNodeId,
    };

    /// What the impl side told Viz.
    #[derive(Debug, Clone, PartialEq)]
    enum ToViz {
        /// `top`: where the topmost tile quad starts, physical px. With
        /// the setup's 64px tiles, a scroll of less than a tile is `-top`.
        /// `clipped_top`: the same for the clipped quads, a container's.
        Frame {
            token: u64,
            resources: Vec<u64>,
            top: f32,
            clipped_top: f32,
        },
        NeedsBeginFrame(bool),
        DidNotProduceFrame,
    }

    #[derive(Clone, Default)]
    struct Viz(Arc<Mutex<Vec<ToViz>>>);

    impl Viz {
        fn take(&self) -> Vec<ToViz> {
            std::mem::take(&mut *self.0.lock().unwrap())
        }
    }

    impl LayerTreeFrameSink for Viz {
        fn submit_compositor_frame(&mut self, _: LocalSurfaceId, frame: CompositorFrame) {
            let resources = frame.resource_list.iter().map(|r| r.id.0).collect();
            let top = |clipped: bool| {
                frame
                    .root_pass()
                    .map(|pass| {
                        pass.quad_list
                            .iter()
                            .filter(|q| !clipped || q.shared_quad_state != 0)
                            .map(|q| q.rect.y)
                            .fold(f32::INFINITY, f32::min)
                    })
                    .unwrap_or(f32::INFINITY)
            };
            self.0.lock().unwrap().push(ToViz::Frame {
                token: frame.metadata.frame_token,
                resources,
                top: top(false),
                clipped_top: top(true),
            });
        }

        fn set_needs_begin_frame(&mut self, needs: bool) {
            self.0.lock().unwrap().push(ToViz::NeedsBeginFrame(needs));
        }

        fn did_not_produce_frame(&mut self) {
            self.0.lock().unwrap().push(ToViz::DidNotProduceFrame);
        }
    }

    /// The impl side, Viz's view of it, what it posted to the main thread,
    /// and a main-thread host whose commits feed it: a 100x100 viewport
    /// over a 100x300 layer of 64px tiles.
    struct Setup {
        proxy: ProxyImpl,
        viz: Viz,
        to_main: Arc<Mutex<Vec<ToMainThread>>>,
        /// What the impl side told the browser: its own acks.
        to_browser: crossbeam_channel::Receiver<RendererHostMsg>,
        host: LayerTreeHost<()>,
    }

    fn setup() -> Setup {
        setup_drawing(true)
    }

    /// `setup()`, drawing each `BeginFrame` once its main frame commits
    /// (`wait`), or at once.
    fn setup_drawing(wait: bool) -> Setup {
        let viz = Viz::default();
        let to_main = Arc::new(Mutex::new(Vec::new()));
        let posted = to_main.clone();
        let (acks, to_browser) = crossbeam_channel::unbounded();
        let proxy = ProxyImpl::new(
            LayerTreeSettings {
                default_tile_size: 64,
                wait_for_all_pipeline_stages_before_draw: wait,
                ..LayerTreeSettings::default()
            },
            Box::new(viz.clone()),
            Box::new(move |task| posted.lock().unwrap().push(task)),
            acks,
        );
        let mut host = LayerTreeHost::with_proxy(());
        host.set_viewport_rect_and_scale(Size::new(100, 100), 1.0, LocalSurfaceId::default());
        host.set_root_layer(layer(display_list(Color::rgb(255, 0, 0))));
        Setup {
            proxy,
            viz,
            to_main,
            to_browser,
            host,
        }
    }

    /// 100x300 of `color`, with a dot a pixel into every 32 px so no tile
    /// is of one color: tiles are rastered and kept as resources.
    fn display_list(color: Color) -> Arc<DisplayItemList> {
        let mut list = DisplayItemList::new();
        list.push_op(PaintOp::DrawRect {
            rect: RectF::new(0.0, 0.0, 100.0, 300.0),
            flags: PaintFlags::fill(color),
        });
        for y in (0..10).map(|i| i as f32 * 32.0 + 1.0) {
            for x in (0..4).map(|i| i as f32 * 32.0 + 1.0) {
                list.push_op(PaintOp::DrawRect {
                    rect: RectF::new(x, y, 1.0, 1.0),
                    flags: PaintFlags::fill(Color::BLACK),
                });
            }
        }
        list.finalize();
        Arc::new(list)
    }

    fn layer(display_list: Arc<DisplayItemList>) -> PictureLayer {
        PictureLayer {
            id: LayerId(1),
            bounds: SizeF::new(100.0, 300.0),
            display_list,
            contents_opaque: true,
            background_color: Color::WHITE,
            invalidation: None,
            properties: Default::default(),
        }
    }

    fn args(sequence: u64) -> BeginFrameArgs {
        BeginFrameArgs {
            sequence,
            frame_time: sequence as f64 * 16.0,
        }
    }

    fn main_frame(sequence: u64) -> ToMainThread {
        ToMainThread::BeginMainFrame {
            args: args(sequence),
            scroll: None,
        }
    }

    /// A main frame no `BeginFrame` waits for, and its commit, as the mode
    /// that doesn't wait for the main thread will have them: nothing draws
    /// it until the next `BeginFrame`.
    fn commit_unwaited(s: &mut Setup, promises: Vec<SwapPromise>) {
        s.proxy.send_begin_main_frame(args(0));
        s.proxy.commit(commit(&mut s.host, promises));
    }

    /// The host's next commit, carrying `promises`.
    fn commit(host: &mut LayerTreeHost<()>, promises: Vec<SwapPromise>) -> CommitState {
        let mut state = host.take_commit_state().expect("something to commit");
        state.swap_promises = promises;
        state
    }

    /// A promise, and what it was called with.
    fn promise() -> (SwapPromise, Arc<Mutex<Vec<Option<u64>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let record = calls.clone();
        let promise = SwapPromise::new(move |token| record.lock().unwrap().push(token));
        (promise, calls)
    }

    /// An input ack's promise, and what it was called with.
    fn input_ack_promise() -> (SwapPromise, Arc<Mutex<Vec<Option<u64>>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let record = calls.clone();
        let promise = SwapPromise::input_ack(move |token| record.lock().unwrap().push(token));
        (promise, calls)
    }

    fn frame_resources(sent: &[ToViz]) -> Vec<u64> {
        match sent.iter().find(|s| matches!(s, ToViz::Frame { .. })) {
            Some(ToViz::Frame { resources, .. }) => resources.clone(),
            _ => panic!("no frame in {sent:?}"),
        }
    }

    #[test]
    fn a_begin_frame_with_nothing_to_do_is_answered_at_once() {
        let mut s = setup();
        s.proxy.begin_frame(args(0));
        assert_eq!(s.viz.take(), [ToViz::DidNotProduceFrame]);
        assert!(s.to_main.lock().unwrap().is_empty());
    }

    #[test]
    fn a_main_frame_is_drawn_for_its_begin_frame_once_it_commits() {
        let mut s = setup();
        s.proxy.set_needs_begin_main_frame(true);
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);

        s.proxy.begin_frame(args(1));
        assert_eq!(*s.to_main.lock().unwrap(), [main_frame(1)]);
        assert_eq!(s.viz.take(), [], "Viz waits for the main frame");

        let (promise, calls) = promise();
        s.proxy.commit(commit(&mut s.host, vec![promise]));
        let sent = s.viz.take();
        let [
            ToViz::Frame {
                token, resources, ..
            },
        ] = &sent[..]
        else {
            panic!("expected one frame, got {sent:?}");
        };
        assert!(!resources.is_empty(), "the visible tiles were rastered");
        assert_eq!(*calls.lock().unwrap(), [Some(*token)]);
    }

    /// Losing the GPU asks Viz for a `BeginFrame`, which draws the tiles
    /// rastered again, as new resources, with no main frame.
    #[test]
    fn losing_the_gpu_draws_the_tiles_again_at_the_next_begin_frame() {
        let mut s = setup();
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        let resources_of = |sent: Vec<ToViz>| {
            sent.into_iter()
                .find_map(|to_viz| match to_viz {
                    ToViz::Frame { resources, .. } => Some(resources),
                    _ => None,
                })
                .expect("a frame")
        };
        let before = resources_of(s.viz.take());

        s.proxy.did_lose_layer_tree_frame_sink();
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.proxy.begin_frame(args(2));
        let after = resources_of(s.viz.take());
        assert_eq!(after.len(), before.len());
        assert!(
            after.iter().all(|id| !before.contains(id)),
            "{before:?} {after:?}"
        );
        assert_eq!(*s.to_main.lock().unwrap(), [main_frame(1)]);
    }

    /// A hidden tab's compositor asks for no `BeginFrame`s, whatever the
    /// main thread wants; shown again, it draws what it has.
    #[test]
    fn a_hidden_compositor_asks_for_no_begin_frames() {
        let mut s = drawn();
        s.proxy.set_visible(false);
        s.proxy.set_needs_begin_main_frame(true);
        assert_eq!(s.viz.take(), [], "hidden");
        s.proxy.set_needs_begin_main_frame(false);
        s.proxy.set_visible(true);
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)], "shown");
        s.proxy.begin_frame(args(2));
        assert!(
            matches!(s.viz.take().first(), Some(ToViz::Frame { .. })),
            "drawn again"
        );
        // Hidden while it asks: it stops.
        s.proxy.set_needs_begin_main_frame(true);
        s.viz.take();
        s.proxy.set_visible(false);
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(false)]);
    }

    /// Hiding a tab answers the input acks waiting for a frame at once,
    /// without one: a hidden tab draws none. The other promises wait for
    /// the frame drawn once it is shown again.
    #[test]
    fn hiding_answers_only_the_input_acks_waiting_for_a_frame() {
        let mut s = drawn();
        let (input, input_calls) = input_ack_promise();
        let (other, other_calls) = promise();
        s.host.set_needs_commit();
        commit_unwaited(&mut s, vec![input, other]);
        s.proxy.set_visible(false);
        assert_eq!(*input_calls.lock().unwrap(), [None]);
        assert_eq!(*other_calls.lock().unwrap(), [], "waits for a frame");

        s.proxy.set_visible(true);
        s.viz.take();
        s.proxy.begin_frame(args(2));
        let sent = s.viz.take();
        let Some(ToViz::Frame { token, .. }) = sent.first() else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(*other_calls.lock().unwrap(), [Some(*token)]);
        assert_eq!(*input_calls.lock().unwrap(), [None], "answered once");
    }

    /// A main frame that started before the tab was hidden commits after:
    /// its input acks are answered at once too.
    #[test]
    fn input_acks_committed_while_hidden_are_answered_at_once() {
        let mut s = drawn();
        let (input, input_calls) = input_ack_promise();
        let (other, other_calls) = promise();
        s.host.set_needs_commit();
        s.proxy.send_begin_main_frame(args(2));
        s.proxy.set_visible(false);
        s.proxy.commit(commit(&mut s.host, vec![input, other]));
        assert_eq!(*input_calls.lock().unwrap(), [None]);
        assert_eq!(*other_calls.lock().unwrap(), []);
    }

    #[test]
    fn the_next_begin_frame_starts_the_next_main_frame() {
        let mut s = setup();
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.begin_frame(args(2));
        assert_eq!(*s.to_main.lock().unwrap(), [main_frame(1), main_frame(2)]);
    }

    #[test]
    fn an_aborted_main_frame_answers_with_no_frame() {
        let mut s = setup();
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.viz.take();
        s.proxy.begin_main_frame_aborted();
        assert_eq!(s.viz.take(), [ToViz::DidNotProduceFrame]);
    }

    #[test]
    fn nothing_to_draw_answers_with_no_frame_and_tells_the_promises() {
        let mut s = setup();
        s.host
            .set_viewport_rect_and_scale(Size::new(0, 0), 1.0, LocalSurfaceId::default());
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.viz.take();
        let (promise, calls) = promise();
        s.proxy.commit(commit(&mut s.host, vec![promise]));
        assert_eq!(s.viz.take(), [ToViz::DidNotProduceFrame]);
        assert_eq!(*calls.lock().unwrap(), [None]);
    }

    #[test]
    fn a_commit_overtaken_before_it_is_drawn_hands_its_promises_on() {
        let mut s = setup();
        // Two commits with no BeginFrame waiting: neither is drawn yet.
        let (first, first_calls) = promise();
        commit_unwaited(&mut s, vec![first]);
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.host
            .set_root_layer(layer(display_list(Color::rgb(0, 0, 255))));
        let (second, second_calls) = promise();
        commit_unwaited(&mut s, vec![second]);
        assert_eq!(s.viz.take(), []);
        assert!(first_calls.lock().unwrap().is_empty());

        // The next frame shows both.
        s.proxy.begin_frame(args(1));
        let sent = s.viz.take();
        let Some(ToViz::Frame { token, .. }) = sent.first() else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(*first_calls.lock().unwrap(), [Some(*token)]);
        assert_eq!(*second_calls.lock().unwrap(), [Some(*token)]);
        assert_eq!(sent[1..], [ToViz::NeedsBeginFrame(false)]);
    }

    /// What rides on a commit is answered in the order the commits came,
    /// within a frame and across frames. The renderer's `DidFinishLoad`
    /// for one navigation and the next one's both ride on commits, so an
    /// old navigation's never reaches the browser after a newer one's.
    #[test]
    fn promises_are_answered_in_the_order_their_commits_came() {
        let mut s = setup();
        let order = Arc::new(Mutex::new(Vec::new()));
        let named = |name: &'static str| {
            let order = order.clone();
            SwapPromise::new(move |_| order.lock().unwrap().push(name))
        };
        // Two commits drawn by one frame.
        commit_unwaited(&mut s, vec![named("first")]);
        s.host.set_needs_commit();
        commit_unwaited(&mut s, vec![named("second")]);
        s.proxy.begin_frame(args(1));
        // A third, in the next.
        s.host.set_needs_commit();
        commit_unwaited(&mut s, vec![named("third")]);
        s.proxy.begin_frame(args(2));
        assert_eq!(*order.lock().unwrap(), ["first", "second", "third"]);
    }

    #[test]
    fn an_unchanged_display_list_keeps_its_tiles() {
        let mut s = setup();
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(1));
        let first = frame_resources(&s.viz.take());

        // Only the viewport moved: the same display list, the same tiles.
        s.host.set_root_scroll_offset(gfx::PointF::new(0.0, 10.0));
        s.host.set_needs_commit();
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(2));
        let scrolled = frame_resources(&s.viz.take());
        assert!(
            scrolled.iter().all(|id| first.contains(id)),
            "{first:?} {scrolled:?}"
        );

        // A new display list is rastered again.
        s.host
            .set_root_layer(layer(display_list(Color::rgb(0, 0, 255))));
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(3));
        let repainted = frame_resources(&s.viz.take());
        assert!(
            repainted.iter().all(|id| !first.contains(id)),
            "{first:?} {repainted:?}"
        );
    }

    #[test]
    fn the_same_display_list_at_another_scale_is_rastered_again() {
        let mut s = setup();
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(1));
        let first = frame_resources(&s.viz.take());

        s.host
            .set_viewport_rect_and_scale(Size::new(200, 200), 2.0, LocalSurfaceId::default());
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(2));
        let scaled = frame_resources(&s.viz.take());
        assert!(
            scaled.iter().all(|id| !first.contains(id)),
            "{first:?} {scaled:?}"
        );
    }

    #[test]
    fn the_same_display_list_rastered_differently_is_rastered_again() {
        let mut s = setup();
        let list = display_list(Color::rgb(255, 0, 0));
        s.host.set_root_layer(layer(list.clone()));
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(1));
        let mut last = frame_resources(&s.viz.take());

        let other_background = PictureLayer {
            background_color: Color::BLACK,
            ..layer(list.clone())
        };
        // Only the bounds differ from the layer before it.
        let other_bounds = PictureLayer {
            bounds: SizeF::new(100.0, 400.0),
            ..other_background.clone()
        };
        for (sequence, changed) in [(2, other_background), (3, other_bounds)] {
            s.host.set_root_layer(changed);
            commit_unwaited(&mut s, Vec::new());
            s.proxy.begin_frame(args(sequence));
            let now = frame_resources(&s.viz.take());
            assert!(now.iter().all(|id| !last.contains(id)), "{last:?} {now:?}");
            last = now;
        }
    }

    #[test]
    fn input_is_aimed_at_the_document_last_drawn() {
        let mut s = setup();
        let click = || {
            InputEvent::mouse_down(
                1.0,
                2.0,
                protocol::input::MouseButton::Left,
                protocol::input::Modifiers::NONE,
            )
        };
        let input = |document| ToMainThread::Input {
            event: click(),
            attribution: InputAttribution::TargetedFrame(None),
            document,
            scroll: None,
            dispatch: InputDispatch::Blocking { cancelable: true },
        };

        s.proxy.handle_input_event(click());
        assert_eq!(
            s.to_main.lock().unwrap().pop(),
            Some(input(None)),
            "nothing drawn yet"
        );

        s.host.set_document_id(DocumentId(1));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.to_main.lock().unwrap().clear();
        s.proxy.handle_input_event(click());
        assert_eq!(
            s.to_main.lock().unwrap().pop(),
            Some(input(Some(DocumentId(1))))
        );

        // The next document is committed and activated, but no frame shows
        // it yet: the user still sees the first.
        s.host.set_document_id(DocumentId(2));
        commit_unwaited(&mut s, Vec::new());
        s.proxy.handle_input_event(click());
        assert_eq!(
            s.to_main.lock().unwrap().pop(),
            Some(input(Some(DocumentId(1))))
        );

        s.proxy.begin_frame(args(2));
        let sent = s.viz.take();
        assert!(
            sent.iter().any(|t| matches!(t, ToViz::Frame { .. })),
            "the next document is drawn: {sent:?}"
        );
        s.proxy.handle_input_event(click());
        assert_eq!(
            s.to_main.lock().unwrap().pop(),
            Some(input(Some(DocumentId(2))))
        );
    }

    #[test]
    fn begin_frames_are_asked_for_while_main_frames_or_a_draw_are_wanted() {
        let mut s = setup();
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.set_needs_begin_main_frame(false);
        assert_eq!(
            s.viz.take(),
            [ToViz::NeedsBeginFrame(true), ToViz::NeedsBeginFrame(false)],
            "sent only when it changes"
        );

        // A commit drawn at no BeginFrame yet asks for one, and stops
        // asking once drawn.
        commit_unwaited(&mut s, Vec::new());
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.proxy.begin_frame(args(1));
        let sent = s.viz.take();
        assert!(matches!(sent[0], ToViz::Frame { .. }), "{sent:?}");
        assert_eq!(sent[1..], [ToViz::NeedsBeginFrame(false)]);
    }

    // Scrolling on the impl side, step by step, as cc's
    // `LayerTreeHostScrollTest` does: the main thread's end is played by
    // the test, which applies what a main frame hands it and commits.

    /// The setup's page as a scroller: 100x100 over 100x300, so 200 to
    /// scroll down.
    fn viewport_node() -> ScrollNode {
        ScrollNode {
            container_bounds: SizeF::new(100.0, 100.0),
            bounds: SizeF::new(100.0, 300.0),
            user_scrollable_horizontal: true,
            user_scrollable_vertical: true,
        }
    }

    /// `setup()` with the first frame drawn: document 1, scrollable, at the
    /// top; what reached Viz and the main thread so far cleared.
    fn drawn() -> Setup {
        let mut s = setup();
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.viz.take();
        s.to_main.lock().unwrap().clear();
        s
    }

    fn down(y: f32) -> gfx::PointF {
        gfx::PointF::new(0.0, y)
    }

    /// What the main thread is handed: `y` scrolled in document 1.
    fn handed(y: f32) -> Option<CompositorScrollDelta> {
        Some(CompositorScrollDelta {
            document: DocumentId(1),
            viewport: down(y),
            containers: Vec::new(),
        })
    }

    fn last_to_main(s: &Setup) -> ToMainThread {
        s.to_main.lock().unwrap().last().cloned().expect("a task")
    }

    /// The main thread ends its main frame at `y` (what it got applied,
    /// and whatever it scrolled itself), committing even if nothing else
    /// changed, as it does whenever it was handed scrolling.
    fn main_commits_at(s: &mut Setup, y: f32) {
        s.host.set_root_scroll_offset(down(y));
        s.host.set_needs_commit();
        s.proxy.commit(commit(&mut s.host, Vec::new()));
    }

    /// The viewport's offset on the impl side now, CSS px.
    fn offset(s: &Setup) -> f32 {
        s.proxy.host_impl.scroll_offset().y
    }

    fn frame_top(sent: &[ToViz]) -> f32 {
        match sent.iter().find(|t| matches!(t, ToViz::Frame { .. })) {
            Some(ToViz::Frame { top, .. }) => *top,
            _ => panic!("no frame in {sent:?}"),
        }
    }

    #[test]
    fn an_impl_scroll_is_handed_to_the_main_thread_and_drawn_with_its_commit() {
        let mut s = drawn();
        assert!(s.proxy.scroll_viewport(down(50.0)));
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.proxy.begin_frame(args(2));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(2),
                scroll: handed(50.0),
            }
        );
        assert_eq!(s.viz.take(), [], "Viz waits for the main frame");
        main_commits_at(&mut s, 50.0);
        let sent = s.viz.take();
        assert_eq!(frame_top(&sent), -50.0);
        assert_eq!(
            sent.last(),
            Some(&ToViz::NeedsBeginFrame(false)),
            "nothing left to hand over"
        );
    }

    #[test]
    fn what_is_scrolled_before_the_commit_comes_stays_on_top_of_it() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(30.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_viewport(down(20.0));
        main_commits_at(&mut s, 30.0);
        assert_eq!(frame_top(&s.viz.take()), -50.0);
        assert!(
            s.proxy.needs_begin_frame,
            "drawn, but the 20 is still to hand over"
        );
        s.proxy.begin_frame(args(3));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(3),
                scroll: handed(20.0),
            }
        );
    }

    /// The main thread works through its queue in order, so input
    /// forwarded after a `BeginMainFrame` is handled after that main
    /// frame's commit: the commit doesn't have what the input took along.
    #[test]
    fn a_commit_doesnt_take_back_what_input_after_its_main_frame_took_along() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(50.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_viewport(down(30.0));
        let click = || {
            InputEvent::mouse_down(
                1.0,
                2.0,
                protocol::input::MouseButton::Left,
                protocol::input::Modifiers::NONE,
            )
        };
        s.proxy.handle_input_event(click());
        assert_eq!(
            last_to_main(&s),
            ToMainThread::Input {
                event: click(),
                attribution: InputAttribution::TargetedFrame(None),
                document: Some(DocumentId(1)),
                scroll: handed(30.0),
                dispatch: InputDispatch::Blocking { cancelable: true },
            }
        );
        main_commits_at(&mut s, 50.0);
        assert_eq!(frame_top(&s.viz.take()), -16.0, "80: 64 + 16, no jump back");
        assert_eq!(offset(&s), 80.0);
        assert!(
            s.proxy.needs_begin_frame,
            "drawn, but a main frame still has to confirm the 30"
        );
        // The next main frame confirms the 30, though it hands nothing.
        s.proxy.begin_frame(args(3));
        assert_eq!(last_to_main(&s), main_frame(3));
        main_commits_at(&mut s, 80.0);
        assert_eq!(offset(&s), 80.0);
        assert!(!s.proxy.needs_begin_frame, "nothing left to hand over");
    }

    #[test]
    fn an_aborted_main_frame_draws_the_scroll_and_its_next_commit_confirms_it() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(50.0));
        s.proxy.begin_frame(args(2));
        // Rendering is blocked: the main thread applied the 50, didn't
        // commit.
        s.proxy.begin_main_frame_aborted();
        let sent = s.viz.take();
        assert_eq!(frame_top(&sent), -50.0);
        assert_eq!(
            sent.last(),
            Some(&ToViz::NeedsBeginFrame(false)),
            "the main thread has it: no more main frames for it"
        );
        // Its next main frame commits it, once.
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(3));
        main_commits_at(&mut s, 50.0);
        assert_eq!(offset(&s), 50.0);
    }

    #[test]
    fn the_main_threads_own_scroll_and_the_impl_sides_add_up() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(20.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_viewport(down(10.0));
        // A `#fragment` moved the viewport to 150 on the main thread.
        main_commits_at(&mut s, 150.0);
        assert_eq!(offset(&s), 160.0);
    }

    #[test]
    fn a_commit_clamps_what_main_hasnt_applied_to_its_smaller_page() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(100.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_viewport(down(60.0));
        s.proxy.handle_input_event(InputEvent::mouse_move(0.0, 0.0));
        // The page shrank to 220 high: 120 to scroll.
        s.host.set_viewport_scroll_node(Some(ScrollNode {
            bounds: SizeF::new(100.0, 220.0),
            ..viewport_node()
        }));
        main_commits_at(&mut s, 100.0);
        assert_eq!(offset(&s), 120.0);
        let resources = frame_resources(&s.viz.take());
        assert!(!resources.is_empty());
    }

    #[test]
    fn another_document_drops_what_was_scrolled_in_the_last() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(50.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_viewport(down(30.0));
        // The main thread went on to document 2: it ignored the 50.
        s.host.set_document_id(DocumentId(2));
        main_commits_at(&mut s, 0.0);
        assert_eq!(offset(&s), 0.0);
        assert_eq!(s.viz.take().last(), Some(&ToViz::NeedsBeginFrame(false)));
    }

    #[test]
    fn only_the_document_the_user_sees_scrolls() {
        let mut s = setup();
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        assert!(!s.proxy.scroll_viewport(down(10.0)), "nothing committed");
        // Committed with nothing to draw it in.
        s.host
            .set_viewport_rect_and_scale(Size::new(0, 0), 1.0, LocalSurfaceId::default());
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        assert!(!s.proxy.scroll_viewport(down(10.0)), "nothing drawn");

        let mut s = drawn();
        s.host.set_document_id(DocumentId(2));
        commit_unwaited(&mut s, Vec::new());
        assert!(
            !s.proxy.scroll_viewport(down(10.0)),
            "document 2 is active, document 1 still shows"
        );
        s.proxy.begin_frame(args(2));
        assert!(s.proxy.scroll_viewport(down(10.0)));
    }

    #[test]
    fn a_viewport_the_user_cant_scroll_doesnt_move() {
        let mut s = setup();
        s.host.set_viewport_scroll_node(Some(ScrollNode {
            user_scrollable_vertical: false,
            ..viewport_node()
        }));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.viz.take();
        assert!(!s.proxy.scroll_viewport(down(10.0)));
        assert_eq!(s.viz.take(), [], "no BeginFrame for it");
    }

    #[test]
    fn scrolling_asks_for_main_frames_whatever_main_wants() {
        let mut s = drawn();
        s.proxy.scroll_viewport(down(10.0));
        s.proxy.set_needs_begin_main_frame(false);
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.proxy.begin_frame(args(2));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(2),
                scroll: handed(10.0),
            }
        );
    }

    #[test]
    fn what_an_impl_scroll_brings_into_view_is_rastered_before_it_is_drawn() {
        let mut s = drawn();
        // To the end, 200: the tile from 256 down was outside the interest
        // rect at the top. Drawn after an abort, with no commit to prepare
        // tiles for it.
        s.proxy.scroll_viewport(down(200.0));
        s.proxy.begin_frame(args(2));
        s.proxy.begin_main_frame_aborted();
        assert_eq!(
            frame_resources(&s.viz.take()).len(),
            4,
            "the rows at 192 and 256, two tiles each"
        );
    }

    /// Tiles whose raster panics are left blank and drawn so; the main
    /// thread is told the first time and when the count doubles, whether
    /// the tiles were rastered for a commit or for a scroll.
    #[test]
    fn the_main_thread_is_told_of_tiles_whose_raster_failed() {
        let mut s = drawn();
        let failed = |count| ToMainThread::RasterFailed {
            message: "a test asked this playback to fail".into(),
            count,
        };
        // The scroll has the row of two tiles at 256 rastered: both fail.
        s.proxy.host_impl.fail_next_rasters(2);
        s.proxy.scroll_viewport(down(200.0));
        s.proxy.begin_frame(args(2));
        s.proxy.begin_main_frame_aborted();
        assert_eq!(
            frame_resources(&s.viz.take()).len(),
            2,
            "the row at 192; the one at 256 is blank"
        );
        let told = |s: &Setup| -> Vec<ToMainThread> {
            let mut to_main = s.to_main.lock().unwrap();
            let told = to_main
                .iter()
                .filter(|task| matches!(task, ToMainThread::RasterFailed { .. }))
                .cloned()
                .collect();
            to_main.clear();
            told
        };
        assert_eq!(told(&s), [failed(2)]);
        // A commit's: the third, not one to tell of, and the fourth.
        s.proxy.host_impl.fail_next_rasters(2);
        s.host
            .set_root_layer(layer(display_list(Color::rgb(0, 0, 255))));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(3));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        assert_eq!(told(&s), [failed(4)]);
        // None fails: nothing told.
        s.host
            .set_root_layer(layer(display_list(Color::rgb(0, 255, 0))));
        s.proxy.begin_frame(args(4));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        assert_eq!(told(&s), []);
    }

    #[test]
    fn the_offset_is_drawn_rounded_to_physical_pixels() {
        let mut s = setup();
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        s.host
            .set_viewport_rect_and_scale(Size::new(200, 200), 2.0, LocalSurfaceId::default());
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.viz.take();
        s.proxy.scroll_viewport(down(10.25));
        s.proxy.begin_frame(args(2));
        s.proxy.begin_main_frame_aborted();
        assert_eq!(frame_top(&s.viz.take()), -21.0, "20.5 physical, rounded");
        assert_eq!(offset(&s), 10.25, "kept unrounded");
    }

    // A container that is a layer scrolled on the impl side: handed over
    // and confirmed as the viewport's offset is, each container on its own.

    const CONTAINER: ElementId = ElementId(7);

    /// Over the setup's page, element 7's 100x300 of content in a 100x50
    /// port at (0, 20), a layer of its own, which can scroll down
    /// `height - 50`.
    fn with_a_container_layer(host: &mut LayerTreeHost<()>, height: f32) {
        let mut trees = PropertyTrees::default();
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(0)),
            offset: gfx::PointF::new(0.0, 0.0),
            scroll: Some(CONTAINER),
            sticky: None,
        });
        trees.clip.push(ClipNode {
            parent: Some(ClipNodeId(0)),
            transform: TransformNodeId(0),
            rect: Some(RectF::new(0.0, 20.0, 100.0, 50.0)),
            radii: Default::default(),
        });
        trees.scroll.push(ScrollTreeNode {
            parent: None,
            element: CONTAINER,
            scroller: ScrollNode {
                container_bounds: SizeF::new(100.0, 50.0),
                bounds: SizeF::new(100.0, height),
                user_scrollable_horizontal: true,
                user_scrollable_vertical: true,
            },
            transform: TransformNodeId(0),
            border_box: RectF::new(0.0, 20.0, 100.0, 50.0),
            clip: ClipNodeId(0),
            scroll_translation: TransformNodeId(1),
            main_thread_scroll_hit_test_region: Arc::from([]),
            wheel_event_handler_region: Arc::from([]),
        });
        let container = PictureLayer {
            id: LayerId(2),
            contents_opaque: false,
            properties: LayerProperties {
                transform: TransformNodeId(1),
                clip: ClipNodeId(1),
                origin: gfx::PointF::new(0.0, 20.0),
                fixed: false,
                over_surfaces: false,
            },
            ..layer(display_list(Color::rgb(0, 255, 0)))
        };
        host.set_layers(
            vec![layer(display_list(Color::rgb(255, 0, 0))), container],
            trees,
        );
    }

    /// `drawn()` with the container layer, and the first frame's resources.
    fn drawn_with_a_container_layer() -> (Setup, Vec<u64>) {
        let mut s = setup();
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        with_a_container_layer(&mut s.host, 300.0);
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        let first = frame_resources(&s.viz.take());
        s.to_main.lock().unwrap().clear();
        (s, first)
    }

    /// What the main thread is handed: the container scrolled `y` in
    /// document 1.
    fn handed_container(y: f32) -> Option<CompositorScrollDelta> {
        Some(CompositorScrollDelta {
            document: DocumentId(1),
            viewport: down(0.0),
            containers: vec![(CONTAINER, down(y))],
        })
    }

    /// The main thread ends its main frame with the container at `y`.
    fn main_commits_container_at(s: &mut Setup, y: f32) {
        s.host
            .set_scroll_offsets(ScrollOffsets::from([(CONTAINER, down(y))]));
        s.host.set_needs_commit();
        s.proxy.commit(commit(&mut s.host, Vec::new()));
    }

    /// The container's offset on the impl side now, CSS px.
    fn container_offset(s: &Setup) -> Option<f32> {
        let offsets = s.proxy.host_impl.scroll_offsets();
        offsets.get(&CONTAINER).map(|offset| offset.y)
    }

    fn clipped_top(sent: &[ToViz]) -> f32 {
        match sent.iter().find(|t| matches!(t, ToViz::Frame { .. })) {
            Some(ToViz::Frame { clipped_top, .. }) => *clipped_top,
            _ => panic!("no frame in {sent:?}"),
        }
    }

    #[test]
    fn a_container_scrolled_here_is_handed_over_and_drawn_from_its_tiles() {
        let (mut s, first) = drawn_with_a_container_layer();
        assert!(s.proxy.scroll_container(CONTAINER, down(30.0)));
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.proxy.begin_frame(args(2));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(2),
                scroll: handed_container(30.0),
            }
        );
        main_commits_container_at(&mut s, 30.0);
        let sent = s.viz.take();
        assert_eq!(clipped_top(&sent), -10.0, "at 20, up by 30");
        assert_eq!(frame_top(&sent), -10.0, "the page stays");
        assert!(
            frame_resources(&sent).iter().all(|id| first.contains(id)),
            "rastered again"
        );
        assert_eq!(
            sent.last(),
            Some(&ToViz::NeedsBeginFrame(false)),
            "nothing left to hand over"
        );
    }

    #[test]
    fn a_commit_doesnt_take_back_what_input_after_its_main_frame_took_along_in_a_container() {
        let (mut s, _) = drawn_with_a_container_layer();
        s.proxy.scroll_container(CONTAINER, down(30.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_container(CONTAINER, down(20.0));
        let moved = InputEvent::mouse_move(1.0, 2.0);
        s.proxy.handle_input_event(moved.clone());
        assert_eq!(
            last_to_main(&s),
            ToMainThread::Input {
                event: moved,
                attribution: InputAttribution::TargetedFrame(None),
                document: Some(DocumentId(1)),
                scroll: handed_container(20.0),
                dispatch: InputDispatch::Blocking { cancelable: true },
            }
        );
        main_commits_container_at(&mut s, 30.0);
        assert_eq!(container_offset(&s), Some(50.0), "no jump back");
        assert!(
            s.proxy.needs_begin_frame,
            "a main frame still has to confirm the 20"
        );
        s.proxy.begin_frame(args(3));
        assert_eq!(last_to_main(&s), main_frame(3), "handing nothing");
        main_commits_container_at(&mut s, 50.0);
        assert_eq!(container_offset(&s), Some(50.0));
        assert!(!s.proxy.needs_begin_frame, "nothing left to hand over");
    }

    #[test]
    fn a_commit_clamps_what_main_hasnt_applied_to_the_containers_new_size() {
        let (mut s, _) = drawn_with_a_container_layer();
        s.proxy.scroll_container(CONTAINER, down(200.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_container(CONTAINER, down(40.0));
        s.proxy.handle_input_event(InputEvent::mouse_move(0.0, 0.0));
        // Its content shrank to 220 high: 170 to scroll, where the main
        // thread clamped the 200 it got.
        with_a_container_layer(&mut s.host, 220.0);
        main_commits_container_at(&mut s, 170.0);
        assert_eq!(container_offset(&s), Some(170.0));
    }

    #[test]
    fn another_document_drops_what_was_scrolled_in_the_last_ones_containers() {
        let (mut s, _) = drawn_with_a_container_layer();
        s.proxy.scroll_container(CONTAINER, down(30.0));
        s.proxy.begin_frame(args(2));
        s.proxy.scroll_container(CONTAINER, down(20.0));
        // Document 2 has a container of the same element id, at the top.
        s.host.set_document_id(DocumentId(2));
        main_commits_container_at(&mut s, 0.0);
        assert_eq!(container_offset(&s), Some(0.0));
        assert_eq!(s.viz.take().last(), Some(&ToViz::NeedsBeginFrame(false)));
    }

    /// The main thread paints a container that is no longer a layer where
    /// it has it: what wasn't handed over yet is gone with the layer.
    #[test]
    fn a_container_no_longer_a_layer_drops_what_wasnt_handed_over() {
        let (mut s, _) = drawn_with_a_container_layer();
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(2));
        s.proxy.set_needs_begin_main_frame(false);
        s.proxy.scroll_container(CONTAINER, down(20.0));
        s.host
            .set_root_layer(layer(display_list(Color::rgb(0, 0, 255))));
        main_commits_container_at(&mut s, 0.0);
        assert_eq!(container_offset(&s), None);
        assert_eq!(s.viz.take().last(), Some(&ToViz::NeedsBeginFrame(false)));
    }

    #[test]
    fn only_the_containers_of_the_document_the_user_sees_scroll() {
        let mut s = setup();
        s.host.set_document_id(DocumentId(1));
        with_a_container_layer(&mut s.host, 300.0);
        s.proxy.send_begin_main_frame(args(0));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        assert!(
            !s.proxy.scroll_container(CONTAINER, down(10.0)),
            "nothing drawn"
        );

        let (mut s, _) = drawn_with_a_container_layer();
        assert!(
            !s.proxy.scroll_container(ElementId(8), down(10.0)),
            "no such container"
        );
        s.host.set_document_id(DocumentId(2));
        commit_unwaited(&mut s, Vec::new());
        assert!(
            !s.proxy.scroll_container(CONTAINER, down(10.0)),
            "document 2 is active, document 1 still shows"
        );
        s.proxy.begin_frame(args(2));
        assert!(s.proxy.scroll_container(CONTAINER, down(10.0)));
    }

    #[test]
    fn what_a_container_scroll_brings_into_view_is_rastered_before_it_is_drawn() {
        let (mut s, _) = drawn_with_a_container_layer();
        // To 200: the container's tiles from 192 down were outside the
        // interest rect. Drawn after an abort, with no commit to prepare
        // tiles for it.
        s.proxy.scroll_container(CONTAINER, down(200.0));
        s.proxy.begin_frame(args(2));
        s.proxy.begin_main_frame_aborted();
        let sent = s.viz.take();
        assert_eq!(clipped_top(&sent), -52.0, "the row at 128, from -180");
        assert_eq!(
            frame_resources(&sent).len(),
            4 + 6,
            "the page's rows at 0 and 64, the container's at 128, 192 and 256, two tiles each"
        );
    }

    // The input handler: what each event is for, and its ack.

    fn wheel_at(x: f32, y: f32, dy: f32) -> InputEvent {
        InputEvent::wheel(x, y, protocol::input::ScrollDelta::Pixels { x: 0.0, y: dy })
    }

    fn key(key: &str) -> InputEvent {
        InputEvent::Key(protocol::input::KeyboardEvent {
            down: true,
            key: key.to_owned(),
            code: key.to_owned(),
            key_code: 0,
            text: String::new(),
            location: 0,
            repeat: false,
            modifiers: protocol::input::Modifiers::NONE,
        })
    }

    /// The acks the impl side sent the browser so far.
    fn acks(s: &Setup) -> Vec<Option<u64>> {
        s.to_browser
            .try_iter()
            .map(|msg| match msg {
                RendererHostMsg::InputEventAck { frame_token } => frame_token,
                other => panic!("not an ack: {other:?}"),
            })
            .collect()
    }

    /// `drawn()`, with a scroll container that isn't a layer at document y
    /// 200 to 250.
    fn drawn_with_a_container() -> Setup {
        let mut s = setup();
        s.host.set_layers(
            vec![layer(display_list(Color::rgb(255, 0, 0)))],
            PropertyTrees {
                main_thread_scroll_hit_test_region: Arc::from([RectF::new(
                    0.0, 200.0, 100.0, 50.0,
                )]),
                ..PropertyTrees::default()
            },
        );
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.viz.take();
        s.to_main.lock().unwrap().clear();
        s
    }

    /// Input handed to the main thread is for the frame the drawn tree has
    /// under the pointer, where it is in the document as the viewport is
    /// scrolled here: a child frame's document at document (0, 0) to
    /// (100, 60), the drawn document's elsewhere, and a key the focused
    /// frame's.
    #[test]
    fn input_is_for_the_frame_under_the_pointer() {
        let mut s = setup();
        s.host.set_layers(
            vec![layer(display_list(Color::rgb(255, 0, 0)))],
            PropertyTrees {
                frame_attribution: crate::FrameAttributionRegion {
                    frames: Arc::from([crate::AttributedFrame {
                        document: ElementId(42),
                        parent: None,
                    }]),
                    rects: Arc::from([crate::AttributionRect {
                        rect: RectF::new(0.0, 0.0, 100.0, 60.0),
                        space: crate::AttributionSpace::Document,
                        order: 0,
                        frame: Some(0),
                        masked: false,
                    }]),
                },
                ..PropertyTrees::default()
            },
        );
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.viz.take();
        s.to_main.lock().unwrap().clear();
        let attribution = |s: &mut Setup, event: InputEvent| {
            s.proxy.handle_input_event(event);
            match s.to_main.lock().unwrap().pop() {
                Some(ToMainThread::Input { attribution, .. }) => attribution,
                other => panic!("{other:?}"),
            }
        };
        let press = |x, y| {
            InputEvent::mouse_down(
                x,
                y,
                protocol::input::MouseButton::Left,
                protocol::input::Modifiers::NONE,
            )
        };
        assert_eq!(
            attribution(&mut s, press(10.0, 10.0)),
            InputAttribution::TargetedFrame(Some(ElementId(42)))
        );
        assert_eq!(
            attribution(&mut s, InputEvent::mouse_move(50.0, 100.0)),
            InputAttribution::TargetedFrame(None)
        );
        assert_eq!(
            attribution(&mut s, key("a")),
            InputAttribution::FocusedFrame
        );
        // Scrolled 50 here: the pointer's y 5 is the document's 55, its 10
        // the document's 60, below the child frame.
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 50.0));
        assert_eq!(
            attribution(&mut s, press(10.0, 5.0)),
            InputAttribution::TargetedFrame(Some(ElementId(42)))
        );
        assert_eq!(
            attribution(&mut s, press(10.0, 10.0)),
            InputAttribution::TargetedFrame(None)
        );
    }

    /// `drawn()`, the page with `wheel` listeners, a blocking one's region
    /// at document y 300 to 350 (below the viewport's first 200px).
    fn drawn_with_wheel_listeners(listeners: bool) -> Setup {
        let mut s = setup();
        s.host.set_layers(
            vec![layer(display_list(Color::rgb(255, 0, 0)))],
            PropertyTrees {
                wheel_event_handler_region: Arc::from([RectF::new(0.0, 300.0, 100.0, 50.0)]),
                wheel_event_listeners: listeners,
                ..PropertyTrees::default()
            },
        );
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.set_needs_begin_main_frame(false);
        s.viz.take();
        s.to_main.lock().unwrap().clear();
        s
    }

    /// In the wheel event handler region a wheel waits for the main thread,
    /// which may cancel it; elsewhere the compositor scrolls it, and hands
    /// it on for the listeners, which can't (it acks it itself), even one
    /// that moved nothing; with no listeners, it doesn't.
    #[test]
    fn a_wheel_waits_for_the_main_thread_only_in_the_wheel_event_handler_region() {
        let mut s = drawn_with_wheel_listeners(true);
        // Scrolled 200: y 110 is the document's 310.
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 200.0));
        assert_eq!(offset(&s), 200.0, "scrolled here");
        assert_eq!(
            last_to_main(&s),
            ToMainThread::Input {
                event: wheel_at(10.0, 10.0, 200.0),
                attribution: InputAttribution::TargetedFrame(None),
                document: Some(DocumentId(1)),
                scroll: handed(200.0),
                dispatch: InputDispatch::NonBlocking,
            },
            "with what it scrolled, for the listeners"
        );
        assert_eq!(acks(&s), [], "the ack waits for the frame");
        let over = wheel_at(10.0, 110.0, -10.0);
        s.proxy.handle_input_event(over.clone());
        assert_eq!(offset(&s), 200.0, "not here");
        assert_eq!(
            last_to_main(&s),
            ToMainThread::Input {
                event: over,
                attribution: InputAttribution::TargetedFrame(None),
                document: Some(DocumentId(1)),
                // Handed over with the one before.
                scroll: None,
                dispatch: InputDispatch::Blocking { cancelable: true },
            }
        );
        // Up past the top: nothing moves, still the listeners'.
        s.to_main.lock().unwrap().clear();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, -1000.0));
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, -10.0));
        assert_eq!(s.to_main.lock().unwrap().len(), 2);
        assert_eq!(acks(&s), [None], "nothing to draw for the last");

        let mut s = drawn_with_wheel_listeners(false);
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 50.0));
        assert!(s.to_main.lock().unwrap().is_empty(), "no listeners");
    }

    #[test]
    fn a_wheel_outside_containers_is_scrolled_here_and_acked_with_the_frame() {
        let mut s = drawn_with_a_container();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 50.0));
        assert!(
            s.to_main.lock().unwrap().is_empty(),
            "not the main thread's"
        );
        assert_eq!(acks(&s), [], "the ack waits for the frame");
        assert_eq!(s.viz.take(), [ToViz::NeedsBeginFrame(true)]);
        s.proxy.begin_frame(args(2));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(2),
                scroll: handed(50.0),
            }
        );
        main_commits_at(&mut s, 50.0);
        let sent = s.viz.take();
        let Some(ToViz::Frame { token, top, .. }) = sent.first() else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(*top, -50.0);
        assert_eq!(acks(&s), [Some(*token)]);
    }

    /// A hidden tab draws no frame for a wheel's ack to wait for.
    #[test]
    fn a_wheel_scrolled_here_while_hidden_is_acked_at_once() {
        let mut s = drawn_with_a_container();
        s.proxy.set_visible(false);
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 50.0));
        assert_eq!(acks(&s), [None]);
    }

    /// Hidden after a wheel was scrolled here, its ack goes then.
    #[test]
    fn hiding_acks_a_wheel_scrolled_here() {
        let mut s = drawn_with_a_container();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 50.0));
        assert_eq!(acks(&s), [], "the ack waits for the frame");
        s.proxy.set_visible(false);
        assert_eq!(acks(&s), [None]);
    }

    /// The region is the document's: the pointer is placed where the
    /// viewport is now, scrolling not yet handed over included.
    #[test]
    fn a_wheel_over_a_container_goes_to_the_main_thread_with_the_scrolling() {
        let mut s = drawn_with_a_container();
        // At y 40: the document's 40, above the container.
        s.proxy.handle_input_event(wheel_at(10.0, 40.0, 170.0));
        assert!(s.to_main.lock().unwrap().is_empty());
        // Scrolled 170: y 40 is the document's 210, the container.
        let over = wheel_at(10.0, 40.0, 10.0);
        s.proxy.handle_input_event(over.clone());
        assert_eq!(
            last_to_main(&s),
            ToMainThread::Input {
                event: over,
                attribution: InputAttribution::TargetedFrame(None),
                document: Some(DocumentId(1)),
                scroll: handed(170.0),
                // No listener may cancel it: the main thread picks the
                // scroller.
                dispatch: InputDispatch::Blocking { cancelable: false },
            }
        );
        assert_eq!(
            acks(&s),
            [],
            "the main thread acks it; the first waits for its frame"
        );
        // Beside the container, the viewport.
        s.proxy.handle_input_event(wheel_at(150.0, 40.0, 10.0));
        assert_eq!(s.to_main.lock().unwrap().len(), 1);
        assert_eq!(offset(&s), 180.0);
    }

    #[test]
    fn what_doesnt_move_is_acked_at_once_and_not_forwarded() {
        let mut s = drawn();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, -10.0));
        assert_eq!(acks(&s), [None], "at the top already");
        assert!(s.to_main.lock().unwrap().is_empty());
        assert!(!s.proxy.needs_begin_frame, "no frame for them");

        let mut s = setup();
        s.host.set_viewport_scroll_node(Some(ScrollNode {
            user_scrollable_vertical: false,
            ..viewport_node()
        }));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 10.0));
        assert_eq!(acks(&s), [None], "overflow: hidden");
        assert!(
            s.to_main
                .lock()
                .unwrap()
                .iter()
                .all(|t| matches!(t, ToMainThread::BeginMainFrame { .. }))
        );
    }

    /// Before a frame shows the active document, wheels are dropped, even
    /// over a container: the user can't have aimed at it.
    #[test]
    fn wheels_for_a_document_not_drawn_are_dropped() {
        let mut s = setup();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 10.0));
        assert_eq!(acks(&s), [None], "nothing committed");

        let mut s = drawn_with_a_container();
        s.host.set_document_id(DocumentId(2));
        commit_unwaited(&mut s, Vec::new());
        s.to_main.lock().unwrap().clear();
        s.proxy.handle_input_event(wheel_at(10.0, 210.0, 10.0));
        assert_eq!(acks(&s), [None], "document 2 is active, document 1 shows");
        assert!(
            s.to_main.lock().unwrap().is_empty(),
            "the container's wheel too"
        );
    }

    /// Keys go to the main thread, which scrolls if their `keydown` isn't
    /// canceled; it acks them. They are for the focused frame.
    #[test]
    fn keys_go_to_the_main_thread() {
        let mut s = drawn();
        s.proxy.handle_input_event(key("PageDown"));
        assert_eq!(offset(&s), 0.0, "not scrolled here");
        assert_eq!(acks(&s), [], "the main thread acks it");
        assert!(matches!(
            s.to_main.lock().unwrap().last(),
            Some(ToMainThread::Input {
                event: InputEvent::Key(_),
                attribution: InputAttribution::FocusedFrame,
                ..
            })
        ));
    }

    /// `drawn_with_a_container_layer()`, with another container that is
    /// a layer in the first one's content at y 100, 100x40, over 200 of
    /// content. Both are below the setup's 100x100 viewport's top 20px.
    fn drawn_with_nested_container_layers() -> Setup {
        let (mut s, _) = drawn_with_a_container_layer();
        let mut trees = s.proxy.host_impl.active_property_trees();
        trees.transform.push(TransformNode {
            parent: Some(TransformNodeId(1)),
            offset: gfx::PointF::new(0.0, 0.0),
            scroll: Some(INNER),
            sticky: None,
        });
        trees.clip.push(ClipNode {
            parent: Some(ClipNodeId(1)),
            transform: TransformNodeId(1),
            rect: Some(RectF::new(0.0, 100.0, 100.0, 40.0)),
            radii: Default::default(),
        });
        trees.scroll.push(ScrollTreeNode {
            parent: Some(crate::ScrollNodeId(0)),
            element: INNER,
            scroller: ScrollNode {
                container_bounds: SizeF::new(100.0, 40.0),
                bounds: SizeF::new(100.0, 200.0),
                user_scrollable_horizontal: true,
                user_scrollable_vertical: true,
            },
            transform: TransformNodeId(1),
            border_box: RectF::new(0.0, 100.0, 100.0, 40.0),
            clip: ClipNodeId(1),
            scroll_translation: TransformNodeId(2),
            main_thread_scroll_hit_test_region: Arc::from([]),
            wheel_event_handler_region: Arc::from([]),
        });
        s.host
            .set_layers(vec![layer(display_list(Color::rgb(255, 0, 0)))], trees);
        commit_unwaited(&mut s, Vec::new());
        s.proxy.begin_frame(args(2));
        s.viz.take();
        s.to_main.lock().unwrap().clear();
        s
    }

    const INNER: ElementId = ElementId(8);

    fn offset_of(s: &Setup, element: ElementId) -> f32 {
        s.proxy.host_impl.scroll_offsets()[&element].y
    }

    #[test]
    fn a_wheel_over_a_container_layer_scrolls_it_here_and_is_acked_with_the_frame() {
        let (mut s, _) = drawn_with_a_container_layer();
        s.proxy.handle_input_event(wheel_at(10.0, 30.0, 30.0));
        assert!(
            s.to_main.lock().unwrap().is_empty(),
            "not the main thread's"
        );
        assert_eq!((offset(&s), container_offset(&s)), (0.0, Some(30.0)));
        s.proxy.begin_frame(args(2));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(2),
                scroll: handed_container(30.0),
            }
        );
        main_commits_container_at(&mut s, 30.0);
        let sent = s.viz.take();
        let Some(ToViz::Frame {
            token, clipped_top, ..
        }) = sent.iter().find(|t| matches!(t, ToViz::Frame { .. }))
        else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(*clipped_top, -10.0);
        assert_eq!(acks(&s), [Some(*token)]);
        // Beside it, the viewport.
        s.proxy.handle_input_event(wheel_at(10.0, 80.0, 10.0));
        assert_eq!((offset(&s), container_offset(&s)), (10.0, Some(30.0)));
    }

    #[test]
    fn what_a_container_layer_cant_take_goes_to_the_viewport() {
        let (mut s, _) = drawn_with_a_container_layer();
        // 250 to scroll in it, 200 in the viewport.
        s.proxy.handle_input_event(wheel_at(10.0, 30.0, 300.0));
        assert_eq!((container_offset(&s), offset(&s)), (Some(250.0), 50.0));
        s.proxy.begin_frame(args(2));
        assert_eq!(
            last_to_main(&s),
            ToMainThread::BeginMainFrame {
                args: args(2),
                scroll: Some(CompositorScrollDelta {
                    document: DocumentId(1),
                    viewport: down(50.0),
                    containers: vec![(CONTAINER, down(250.0))],
                }),
            }
        );
        // The rest of this one to the viewport, which then is at its end
        // too: the next moves nothing, and is acked at once.
        s.proxy.handle_input_event(wheel_at(10.0, 30.0, 500.0));
        assert_eq!(offset(&s), 200.0);
        assert_eq!(acks(&s), []);
        s.proxy.handle_input_event(wheel_at(10.0, 30.0, 10.0));
        assert_eq!(acks(&s), [None]);
        assert_eq!(s.to_main.lock().unwrap().len(), 1, "just the main frame");
    }

    #[test]
    fn a_wheel_scrolls_the_innermost_container_layer_then_those_around_it() {
        let mut s = drawn_with_nested_container_layers();
        // The inner one is at 120, below the outer one's port (20 to 70).
        s.proxy.handle_input_event(wheel_at(10.0, 30.0, 100.0));
        assert_eq!(
            (offset_of(&s, CONTAINER), offset_of(&s, INNER)),
            (100.0, 0.0)
        );
        // Now at 20: where it is drawn, unsent scrolling included.
        s.proxy.handle_input_event(wheel_at(10.0, 30.0, 1000.0));
        assert_eq!(
            (offset_of(&s, INNER), offset_of(&s, CONTAINER), offset(&s)),
            (160.0, 250.0, 200.0),
            "each to its end, the inner one first"
        );
        assert!(s.to_main.lock().unwrap().is_empty());
    }

    #[test]
    fn a_handled_input_is_acked_by_the_next_frame_whatever_it_shows() {
        // Drawn after an abort: rendering blocked, the scroll still shows.
        let mut s = drawn();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 30.0));
        s.proxy.begin_frame(args(2));
        s.proxy.begin_main_frame_aborted();
        let sent = s.viz.take();
        let Some(ToViz::Frame { token, .. }) =
            sent.iter().find(|t| matches!(t, ToViz::Frame { .. }))
        else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(acks(&s), [Some(*token)]);

        // The next frame draws nothing: the ack says so.
        let mut s = drawn();
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 30.0));
        s.proxy.begin_frame(args(2));
        s.host
            .set_viewport_rect_and_scale(Size::new(0, 0), 1.0, LocalSurfaceId::default());
        main_commits_at(&mut s, 30.0);
        assert_eq!(acks(&s), [None]);
    }

    // Not waiting for the main thread: what the window does.

    #[test]
    fn not_waiting_a_begin_frame_is_answered_at_once_and_a_commit_drawn_at_the_next() {
        let mut s = setup_drawing(false);
        s.proxy.set_needs_begin_main_frame(true);
        s.viz.take();
        s.proxy.begin_frame(args(1));
        assert_eq!(*s.to_main.lock().unwrap(), [main_frame(1)]);
        assert_eq!(
            s.viz.take(),
            [ToViz::DidNotProduceFrame],
            "nothing to draw yet, and no waiting"
        );
        let (promise, calls) = promise();
        s.proxy.commit(commit(&mut s.host, vec![promise]));
        assert_eq!(s.viz.take(), [], "drawn at the next BeginFrame");
        assert!(calls.lock().unwrap().is_empty());
        s.proxy.begin_frame(args(2));
        let sent = s.viz.take();
        let Some(ToViz::Frame { token, .. }) = sent.first() else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(*calls.lock().unwrap(), [Some(*token)]);
        assert_eq!(last_to_main(&s), main_frame(2), "and the next main frame");
    }

    /// The point of not waiting: while the main thread is busy with a main
    /// frame, what the impl side scrolls is drawn, and acked, all the same.
    #[test]
    fn not_waiting_a_scroll_is_drawn_while_the_main_thread_is_busy() {
        let mut s = setup_drawing(false);
        s.host.set_document_id(DocumentId(1));
        s.host.set_viewport_scroll_node(Some(viewport_node()));
        s.proxy.set_needs_begin_main_frame(true);
        s.proxy.begin_frame(args(1));
        s.proxy.commit(commit(&mut s.host, Vec::new()));
        s.proxy.begin_frame(args(2));
        s.viz.take();
        // The main frame of BeginFrame 2 doesn't come back: script runs.
        s.proxy.handle_input_event(wheel_at(10.0, 10.0, 50.0));
        s.proxy.begin_frame(args(3));
        let sent = s.viz.take();
        let Some(ToViz::Frame { token, top, .. }) = sent.first() else {
            panic!("expected a frame, got {sent:?}");
        };
        assert_eq!(*top, -50.0);
        assert_eq!(acks(&s), [Some(*token)]);
        assert_eq!(
            s.to_main.lock().unwrap().len(),
            2,
            "no other main frame while one is out"
        );
    }

    #[test]
    fn the_impl_side_can_move_to_the_compositor_thread() {
        fn assert_send<T: Send>() {}
        assert_send::<ProxyImpl>();
        assert_send::<CommitState>();
    }
}
