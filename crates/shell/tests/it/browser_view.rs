//! A browser window without the window: the `BrowserView` with a real tab,
//! its frames and the tab's drawn by Viz on the GPU into a texture read
//! back from, driven by the calls `window` makes for winit's events. Pages
//! come from a local test server.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use base::AtomicSequenceNumber;
use browser::{ContentsEvent, NetworkContext, NetworkContextParams, WebContents};
use crossbeam_channel::{Receiver, Sender, unbounded};
use gfx::{Color, PointF, Rect, RectF, Size, SizeF};
use net::test_server::{Route, TestServer};
use protocol::TabId;
use protocol::input::{MouseButton, ScrollDelta};
use protocol::renderer::{CursorType, RendererHostMsg};
use protocol::viz::{
    CompositorFrameSinkMsg, ContextLostObserver, CopyOutputRequest, CopyOutputResult, FrameSinkId,
    SharedImage, SharedImageInterface, VizMsg,
};
use shell::browser_view::{BROWSER_FRAME_SINK, BrowserView, TabFactory};
use shell::browser_view_layout::{BrowserViewLayout, Layout, tab_strip_height, toolbar_height};
use shell::omnibox::Clipboard;
use shell::sad_tab::SadTab;
use shell::tab_strip::TabStrip;
use shell::{tab_strip, toolbar};
use viz::{BeginFrameSource, Display, GpuSharedImages, WgpuRenderer};
use winit::keyboard::{Key, ModifiersState, NamedKey};

const TIMEOUT: Duration = Duration::from_secs(30);
const TOOLBAR: [u8; 4] = [241, 243, 244, 255];
const TAB_STRIP: [u8; 4] = [222, 225, 230, 255];
const HOVERED_TAB: [u8; 4] = [232, 234, 237, 255];
/// Where the toolbar starts, under the tab strip, CSS px.
const TOOLBAR_Y: f32 = tab_strip::HEIGHT;
/// Where the page starts, CSS px.
const TOP: f32 = tab_strip::HEIGHT + toolbar::HEIGHT;
const GREEN: [u8; 4] = [0, 128, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
/// The tab of a second web view.
const SECOND: TabId = TabId(1);

#[derive(Clone, Default)]
struct SharedClipboard(Rc<RefCell<Option<String>>>);

impl Clipboard for SharedClipboard {
    fn read(&mut self) -> Option<String> {
        self.0.borrow().clone()
    }
    fn write(&mut self, text: &str) {
        *self.0.borrow_mut() = Some(text.to_owned());
    }
}

/// Makes no shared images (the tiles keep their pixels, for the software
/// renderer), but counts the tiles that asked.
#[derive(Default)]
struct CountingGpu(AtomicUsize);

impl SharedImageInterface for CountingGpu {
    fn create_shared_image(&self, _: Size, _: &[u8]) -> Option<SharedImage> {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test's count: far from wrapping"
        )]
        self.0.fetch_add(1, Ordering::Relaxed);
        None
    }

    fn flush(&self) {}

    fn add_context_lost_observer(&self, _: Weak<dyn ContextLostObserver>) {}
}

struct Window {
    view: BrowserView,
    host: Receiver<(TabId, ContentsEvent)>,
    host_tx: Sender<(TabId, ContentsEvent)>,
    viz: Receiver<VizMsg>,
    viz_tx: Sender<VizMsg>,
    /// The renderers' `BeginFrame`s, sent back to back as Viz messages are
    /// read (not from another thread: a frame must be readable as soon as
    /// the message that follows it on the host channel is). Shared with the
    /// view's `TabFactory`.
    begin_frames: Rc<RefCell<BeginFrameSource>>,
    /// The tabs the view closed, in order.
    closed: Rc<RefCell<Vec<TabId>>>,
    frame_time: f64,
    display: Display<WgpuRenderer>,
    clipboard: SharedClipboard,
}

impl Window {
    /// `size` in physical px, at `scale`. The toolbar's tiles go onto the
    /// GPU device Viz draws with (shared images); the tabs' are pixels Viz
    /// uploads, as from a client with no shared image interface.
    fn open(size: Size, scale: f32) -> Self {
        let images = GpuSharedImages::new();
        Self::open_with_gpu(size, scale, Some(images.clone()), images)
    }

    /// [`open`](Self::open), the toolbar's tiles going through `gpu`, and
    /// Viz's device shared through `images`.
    fn open_with_gpu(
        size: Size,
        scale: f32,
        gpu: Option<Arc<dyn SharedImageInterface>>,
        images: Arc<GpuSharedImages>,
    ) -> Self {
        Self::open_with(size, scale, gpu, images)
    }

    fn open_with(
        size: Size,
        scale: f32,
        gpu: Option<Arc<dyn SharedImageInterface>>,
        images: Arc<GpuSharedImages>,
    ) -> Self {
        let (viz_tx, viz) = unbounded::<VizMsg>();
        let (host_tx, host) = unbounded();
        let begin_frames = Rc::new(RefCell::new(BeginFrameSource::new()));
        let web_contents = start_contents(TabId(0), &viz_tx, &host_tx, &begin_frames);
        let closed = Rc::default();
        let clipboard = SharedClipboard::default();
        let mut view = BrowserView::new(
            web_contents,
            viz_tx.clone(),
            gpu,
            true,
            Box::new(clipboard.clone()),
        );
        view.set_mac(true);
        view.set_tab_factory(Box::new(TestTabs {
            viz: viz_tx.clone(),
            host: host_tx.clone(),
            begin_frames: begin_frames.clone(),
            closed: Rc::clone(&closed),
        }));
        view.resize(size, scale);
        Self {
            view,
            host,
            host_tx,
            viz,
            viz_tx,
            begin_frames,
            closed,
            frame_time: 0.0,
            display: Display::new(
                WgpuRenderer::offscreen(size, images).expect("an adapter"),
                BROWSER_FRAME_SINK,
            ),
            clipboard,
        }
    }

    /// A window 400 x 300 CSS px at `scale`, laid out `SideBySide`: the
    /// tab showing `first`, and a second web view `second`, both loaded.
    fn side_by_side(scale: f32, overlap: bool, first: &str, second: &str) -> Self {
        let mut window = Self::open(window_size(scale), scale);
        window.view.set_layout(Box::new(SideBySide { overlap }));
        window.load(first);
        let contents = start_contents(
            SECOND,
            &window.viz_tx,
            &window.host_tx,
            &window.begin_frames,
        );
        window.view.add_web_view(contents);
        let second_contents = window.view.contents_mut(SECOND).unwrap();
        second_contents.navigate(second);
        let latest = second_contents.latest_navigation_id();
        window.until_from(|tab, msg| {
            tab == SECOND
                && matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == latest)
        });
        window
    }

    /// The bounds of the web views `SideBySide` gives this window.
    fn web_views(&self, overlap: bool) -> (Rect, Rect) {
        let scale = self.view.scale();
        let views = SideBySide { overlap }
            .layout(window_size(scale), scale, 2)
            .web_views;
        (views[0], views[1])
    }

    /// The text of `tab`'s `#out` now.
    fn out_of(&mut self, tab: TabId) -> String {
        static QUERIES: AtomicSequenceNumber<u64> = AtomicSequenceNumber::<u64>::new();
        let id = QUERIES.get_next();
        self.view
            .contents(tab)
            .unwrap()
            .query_element(id, &[], "#out");
        let mut text = String::new();
        self.until_from(|from, msg| match msg {
            RendererHostMsg::QueryElementResult { query_id, element }
                if from == tab && *query_id == id =>
            {
                text = element.as_ref().expect("#out").text.clone();
                true
            }
            _ => false,
        });
        text
    }

    /// Until `tab`'s `#out` says `expected`.
    fn until_out(&mut self, tab: TabId, expected: &str) {
        let start = Instant::now();
        loop {
            let text = self.out_of(tab);
            if text == expected {
                return;
            }
            assert!(
                start.elapsed() < TIMEOUT,
                "{tab:?} says {text:?}, not {expected:?}"
            );
        }
    }

    /// Until `tab` asks for a cursor.
    fn until_cursor_from(&mut self, tab: TabId) {
        self.until_from(|from, msg| from == tab && matches!(msg, RendererHostMsg::SetCursor(_)));
    }

    /// Until the window's pixel at `(x, y)` is `color`, drawing the frames
    /// that come meanwhile.
    fn until_pixel(&mut self, x: u32, y: u32, color: [u8; 4]) {
        let start = Instant::now();
        while self.pixel(x, y) != color {
            assert!(start.elapsed() < TIMEOUT, "({x}, {y}) turns {color:?}");
            self.send_begin_frames();
            if let Ok(msg) = self.viz.recv_timeout(Duration::from_millis(5)) {
                self.viz_message(msg);
            }
            while let Ok((tab, event)) = self.host.try_recv() {
                self.view.on_contents_event(tab, event);
            }
        }
    }

    /// Hands the renderer's messages to the view until `done` picks one,
    /// handling Viz's meanwhile (the renderer waits for `BeginFrame`s).
    fn until(&mut self, mut done: impl FnMut(&RendererHostMsg) -> bool) {
        self.until_from(|_, msg| done(msg));
    }

    /// [`until`](Self::until), knowing whose renderer each message is from.
    fn until_from(&mut self, mut done: impl FnMut(TabId, &RendererHostMsg) -> bool) {
        let start = Instant::now();
        loop {
            while let Ok(msg) = self.viz.try_recv() {
                self.viz_message(msg);
            }
            self.send_begin_frames();
            // Messages may keep coming: the wait as a whole is bounded.
            if let Ok((tab, event)) = self.host.recv_timeout(Duration::from_millis(5))
                && let Some(msg) = self.view.on_contents_event(tab, event)
                && done(tab, &msg)
            {
                return;
            }
            assert!(start.elapsed() < TIMEOUT, "the renderer answers");
        }
    }

    /// A Viz message: `BeginFrame` bookkeeping, then the frame if it is one
    /// (`submit`).
    fn viz_message(&mut self, msg: VizMsg) -> Option<FrameSinkId> {
        match &msg {
            VizMsg::FrameSink(sink, msg) => self.begin_frames.borrow_mut().did_receive(*sink, msg),
            // A renderer launched again after one has gone: its old
            // surface goes.
            VizMsg::InvalidateFrameSinkId(sink) => {
                self.begin_frames.borrow_mut().remove_client(*sink);
                self.display.remove_frame_sink(*sink);
            }
            VizMsg::RegisterFrameSinkId(sink, observer) => {
                let observer = observer.clone();
                self.begin_frames
                    .borrow_mut()
                    .add_client(*sink, Box::new(move |args| observer.on_begin_frame(args)));
            }
            _ => {}
        }
        let from = self.submit(msg);
        self.send_begin_frames();
        from
    }

    fn send_begin_frames(&mut self) {
        let mut begin_frames = self.begin_frames.borrow_mut();
        if begin_frames.wants_begin_frame() {
            begin_frames.issue(self.frame_time);
            self.frame_time += 1000.0 / 60.0;
        }
    }

    /// Until Viz hears `wanted` from a renderer, handling what comes before.
    fn until_viz(&mut self, mut wanted: impl FnMut(&VizMsg) -> bool) {
        let start = Instant::now();
        loop {
            // Messages may keep coming: the wait as a whole is bounded.
            assert!(start.elapsed() < TIMEOUT, "Viz hears it");
            self.send_begin_frames();
            let msg = self.viz.recv_timeout(TIMEOUT).expect("Viz hears it");
            let found = wanted(&msg);
            self.viz_message(msg);
            if found {
                return;
            }
            while let Ok((tab, event)) = self.host.try_recv() {
                self.view.on_contents_event(tab, event);
            }
        }
    }

    /// A key with Ctrl held.
    fn ctrl(&mut self, key: NamedKey, shift: bool) {
        let mut modifiers = ModifiersState::CONTROL;
        modifiers.set(ModifiersState::SHIFT, shift);
        self.view.set_modifiers(modifiers);
        press(&mut self.view, Key::Named(key), None);
        self.view.set_modifiers(ModifiersState::empty());
    }

    /// Until the latest navigation has loaded and drawn.
    fn until_loaded(&mut self) {
        let latest = self.view.web_contents().latest_navigation_id();
        self.until(|msg| {
            matches!(msg, RendererHostMsg::DidFinishLoad { navigation_id, .. } if *navigation_id == latest)
        });
    }

    fn load(&mut self, url: &str) {
        self.view.navigate(url);
        self.until_loaded();
    }

    /// Hands a frame to Viz; which sink it came from.
    fn submit(&mut self, msg: VizMsg) -> Option<FrameSinkId> {
        let VizMsg::FrameSink(
            sink,
            CompositorFrameSinkMsg::SubmitCompositorFrame {
                local_surface_id,
                frame,
            },
        ) = msg
        else {
            return None;
        };
        self.display
            .submit_compositor_frame(sink, local_surface_id, frame);
        Some(sink)
    }

    /// Viz draws every frame sent so far, the browser's and the tab's.
    fn draw(&mut self) {
        while let Ok(msg) = self.viz.try_recv() {
            self.viz_message(msg);
        }
        if self.display.needs_draw() {
            self.display.draw_and_swap();
        }
    }

    /// Waits for the next frame from `sink`, drawing it; the sinks of every
    /// frame on the way, it included.
    fn until_frame_from(&mut self, sink: FrameSinkId) -> Vec<FrameSinkId> {
        let start = Instant::now();
        let mut senders = Vec::new();
        loop {
            // Other sinks' frames may keep coming: the wait as a whole is
            // bounded.
            assert!(start.elapsed() < TIMEOUT, "a frame from {sink:?} comes");
            self.send_begin_frames();
            let msg = self.viz.recv_timeout(TIMEOUT).expect("a frame comes");
            if let Some(from) = self.viz_message(msg) {
                senders.push(from);
                if from == sink {
                    self.draw();
                    return senders;
                }
            }
        }
    }

    fn pixel(&mut self, x: u32, y: u32) -> [u8; 4] {
        self.draw();
        let copy = self.read_back();
        let i = ((y * copy.size.width + x) * 4) as usize;
        copy.pixels[i..i + 4].try_into().unwrap()
    }

    /// What Viz shows now, drawn again and read back.
    fn read_back(&mut self) -> CopyOutputResult {
        let copy = Arc::new(std::sync::Mutex::new(None));
        let into = copy.clone();
        self.display
            .request_copy_of_output(CopyOutputRequest::new(move |result| {
                *into.lock().unwrap() = result;
            }));
        assert!(self.display.draw_and_swap(), "a frame to draw");
        copy.lock().unwrap().take().expect("read back")
    }

    /// A primary-button click at `(x, y)`, CSS px of the window.
    fn click(&mut self, x: f32, y: f32) {
        self.view.pointer_moved(PointF::new(x, y));
        self.view.mouse_input(MouseButton::Left, true);
        self.view.mouse_input(MouseButton::Left, false);
    }

    fn key(&mut self, key: NamedKey) {
        press(&mut self.view, Key::Named(key), None);
    }

    fn type_text(&mut self, text: &str) {
        for c in text.chars() {
            let s = c.to_string();
            press(
                &mut self.view,
                Key::Character(s.as_str().into()),
                Some(s.clone()),
            );
        }
    }

    fn shortcut(&mut self, c: &str) {
        self.view.set_modifiers(ModifiersState::SUPER);
        press(&mut self.view, Key::Character(c.into()), Some(c.to_owned()));
        self.view.set_modifiers(ModifiersState::empty());
    }

    fn url(&self) -> Option<&str> {
        self.view.web_contents().url()
    }

    fn field_text(&self) -> String {
        self.view.toolbar().omnibox.text()
    }

    fn close(mut self) {
        self.view.shutdown();
    }
}

/// The window's tabs, as `BrowserWindow`'s are made, with the test's
/// `BeginFrameSource` for Viz.
struct TestTabs {
    viz: Sender<VizMsg>,
    host: Sender<(TabId, ContentsEvent)>,
    begin_frames: Rc<RefCell<BeginFrameSource>>,
    closed: Rc<RefCell<Vec<TabId>>>,
}

impl TabFactory for TestTabs {
    fn create(&mut self, tab: TabId) -> Result<WebContents, String> {
        Ok(start_contents(
            tab,
            &self.viz,
            &self.host,
            &self.begin_frames,
        ))
    }

    fn destroy(&mut self, mut contents: WebContents) {
        self.closed.borrow_mut().push(contents.tab());
        self.begin_frames
            .borrow_mut()
            .remove_client(contents.frame_sink_id());
        contents.shutdown();
    }
}

/// Starts `tab`'s renderer, its frames going to `viz` and its messages to
/// `host`, and has `begin_frames` send it `BeginFrame`s.
fn start_contents(
    tab: TabId,
    viz: &Sender<VizMsg>,
    host: &Sender<(TabId, ContentsEvent)>,
    begin_frames: &RefCell<BeginFrameSource>,
) -> WebContents {
    let host = host.clone();
    let contents = WebContents::new(
        tab,
        browser::RendererFlags {
            test_fonts: true,
            web_tests: false,
            wait_for_all_pipeline_stages_before_draw: true,
            // To read what pages write (`Window::until_text`).
            automation: true,
            user_agent: browser::user_agent(),
            system_colors: gfx::SystemColors::DEFAULT,
            num_raster_threads: None,
            caret_blink_interval: None,
        },
        shell::SCRIPT_ENGINES,
        viz.clone(),
        None,
        NetworkContext::start(NetworkContextParams {
            user_agent: browser::user_agent(),
            ..Default::default()
        })
        .expect("network service starts"),
        Box::new(move |tab, msg| {
            let _ = host.send((tab, msg));
        }),
    )
    .expect("renderer starts");
    begin_frames.borrow_mut().add_client(
        contents.frame_sink_id(),
        Box::new(contents.begin_frame_sender()),
    );
    contents
}

/// 400 x 300 CSS px at `scale`, physical px.
fn window_size(scale: f32) -> Size {
    Size::new(
        (400.0 * scale).round() as u32,
        (300.0 * scale).round() as u32,
    )
}

/// Two web views under the tab strip and the toolbar: the second 10px right
/// of the first,
/// and 31px lower, so that it starts on odd pixels, between two CSS px at
/// 1.5x and 2x. With `overlap`, it covers the first's right 50px instead.
struct SideBySide {
    overlap: bool,
}

impl BrowserViewLayout for SideBySide {
    fn layout(&self, size: Size, scale: f32, web_views: usize) -> Layout {
        let strip = tab_strip_height(scale);
        let chrome = strip + toolbar_height(scale);
        let half = size.width / 2;
        let first_width = if self.overlap { half + 50 } else { half - 9 };
        let top = chrome + 31;
        let mut views = vec![
            Rect::new(0, chrome as i32, first_width, size.height - chrome),
            Rect::new(
                half as i32 + 1,
                top as i32,
                size.width - half - 1,
                size.height - top,
            ),
        ];
        views.truncate(web_views);
        Layout {
            tab_strip: Rect::new(0, 0, size.width, strip),
            toolbar: Rect::new(0, strip as i32, size.width, chrome - strip),
            web_views: views,
        }
    }
}

/// Where in the window, CSS px, `(x, y)` of a web view at `bounds` is.
fn in_window(bounds: Rect, scale: f32, x: f32, y: f32) -> PointF {
    PointF::new(bounds.x as f32 / scale + x, bounds.y as f32 / scale + y)
}

/// The window's pixel `(x, y)` CSS px into a web view at `bounds`.
fn pixel_in(bounds: Rect, scale: f32, x: f32, y: f32) -> (u32, u32) {
    (
        (bounds.x as f32 + x * scale) as u32,
        (bounds.y as f32 + y * scale) as u32,
    )
}

fn server(routes: &[(&str, &str)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, html)| (path.to_string(), Route::ok("text/html", *html)))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

const GREEN_PAGE: &str =
    "<!DOCTYPE html><title>green</title><style>html { background-color: green }</style>";

/// Links as 50px-high blocks, one under another.
fn links(hrefs: &[&str]) -> String {
    let links: String = hrefs
        .iter()
        .map(|href| format!("<a href='{href}'></a>"))
        .collect();
    format!(
        "<!DOCTYPE html><style>html, body {{ margin: 0 }} a {{ display: block; height: 50px }}</style>{links}"
    )
}

/// A key down and up, as a window reports one.
fn press(view: &mut BrowserView, key: Key, text: Option<String>) {
    let input = shell::keyboard::KeyInput::press(key, text);
    view.key(&input);
    view.key(&input.release());
}

#[test]
fn the_toolbar_is_over_the_page() {
    let server = server(&[("/", GREEN_PAGE)]);
    for scale in [1.0, 1.5, 2.0] {
        let size = Size::new((400.0 * scale) as u32, (300.0 * scale) as u32);
        let mut window = Window::open(size, scale);
        window.load(&server.url("/"));
        let px = |css: f32| (css * scale) as u32;
        // The tab strip's background, the toolbar's, then the page right
        // under them, to the window's bottom.
        assert_eq!(window.pixel(px(1.0), px(1.0)), TAB_STRIP, "at {scale}x");
        assert_eq!(
            window.pixel(px(1.0), px(TOOLBAR_Y + 1.0)),
            TOOLBAR,
            "at {scale}x"
        );
        let top = px(TOP);
        assert_eq!(window.pixel(px(1.0), top), GREEN, "at {scale}x");
        assert_eq!(window.pixel(px(1.0), size.height - 1), GREEN, "at {scale}x");
        assert_ne!(window.pixel(px(1.0), top - 1), GREEN, "at {scale}x");
        assert_eq!(window.view.title(), "green");
        window.close();
    }
}

/// At another device scale factor, the page is painted again even at the
/// same size in CSS px: its edges snap to the new device pixels.
#[test]
fn a_new_scale_factor_paints_the_page_on_its_device_pixels() {
    let server = server(&[(
        "/",
        "<!DOCTYPE html><style>html, body { margin: 0 }</style>
         <div style='margin-left: 10.25px; width: 20.5px; height: 20px; background-color: green'></div>",
    )]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    let y = |scale: f32| ((TOP + 10.0) * scale) as u32;
    // 10.25 to 30.75: 10 to 31 at 1x.
    assert_eq!(window.pixel(10, y(1.0)), GREEN);
    assert_ne!(window.pixel(9, y(1.0)), GREEN);
    // 20.5 to 61.5 device px at 2x: 21 to 62.
    let size = Size::new(800, 600);
    window.view.resize(size, 2.0);
    window.display.resize(size);
    let tab = window.view.web_contents().frame_sink_id();
    window.until_frame_from(tab);
    assert_ne!(window.pixel(20, y(2.0)), GREEN, "snapped at 1x");
    assert_eq!(window.pixel(21, y(2.0)), GREEN);
    assert_eq!(window.pixel(61, y(2.0)), GREEN);
    assert_ne!(window.pixel(62, y(2.0)), GREEN);
    window.close();
}

/// A new device scale factor shows an `image-set()`'s image for it: the
/// one not loaded yet loads (after the first frame at that scale, it
/// paints anew as it comes), and the one loaded before shows at once when
/// the scale comes back.
#[test]
fn a_new_scale_factor_shows_an_image_sets_image_for_it() {
    let page = "<!DOCTYPE html><style>html, body { margin: 0 }
        div { width: 100px; height: 50px; background-repeat: no-repeat;
              background-image: image-set(url(/a.png) 1x, url(/b.png) 2x) }</style><div></div>";
    let server = TestServer::start(HashMap::from([
        ("/".to_owned(), Route::ok("text/html", page)),
        ("/a.png".to_owned(), Route::ok("image/png", WIDE_GREEN_PNG)),
        (
            "/b.png".to_owned(),
            Route::ok("image/png", TALL_GREEN_PNG).with_delay(Duration::from_millis(300)),
        ),
    ]))
    .expect("test server starts");
    let tab = |window: &Window| window.view.web_contents().frame_sink_id();
    // The image's device pixels shown, its corner's and those past it.
    let shows = |window: &mut Window, scale: f32, (width, height): (u32, u32)| {
        let top = (TOP * scale) as u32;
        window.pixel(width - 1, top + height - 1) == GREEN
            && window.pixel(width, top + height - 1) != GREEN
            && window.pixel(width - 1, top + height) != GREEN
    };
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    assert!(shows(&mut window, 1.0, (40, 30)), "a.png at 1x");
    let size = Size::new(800, 600);
    window.view.resize(size, 2.0);
    window.display.resize(size);
    // b.png, 20×40 at 2x: 10×20 CSS px.
    let start = Instant::now();
    while !shows(&mut window, 2.0, (20, 40)) {
        assert!(start.elapsed() < TIMEOUT, "b.png shows at 2x");
        let sink = tab(&window);
        window.until_frame_from(sink);
    }
    let size = Size::new(400, 300);
    window.view.resize(size, 1.0);
    window.display.resize(size);
    let sink = tab(&window);
    window.until_frame_from(sink);
    assert!(shows(&mut window, 1.0, (40, 30)), "a.png again at 1x");
    let images: Vec<String> = server
        .requests()
        .into_iter()
        .map(|request| request.target)
        .filter(|target| target.ends_with(".png"))
        .collect();
    assert_eq!(images, ["/a.png", "/b.png"], "each loaded once");
    window.close();
}

static WIDE_GREEN_PNG: &[u8] =
    include_bytes!("../../../../tests/reftests/images/resources/green-40x30.png");
static TALL_GREEN_PNG: &[u8] =
    include_bytes!("../../../../tests/reftests/images/resources/green-20x40.png");

#[test]
fn a_new_frame_from_the_tab_alone_is_drawn() {
    let server = server(&[(
        "/",
        "<!DOCTYPE html><style>html, body { margin: 0 }</style>
         <div style='height: 1000px; background-color: red'></div>
         <div style='height: 10px; background-color: green'></div>",
    )]);
    let mut window = Window::open(Size::new(200, 200), 1.0);
    window.load(&server.url("/"));
    assert_eq!(window.pixel(10, 199), [255, 0, 0, 255]);
    // Scrolling is a frame from the tab only; the browser sends none, and
    // Viz draws the tab's new one in the browser's.
    window.key(NamedKey::End);
    let tab = window.view.web_contents().frame_sink_id();
    assert_eq!(window.until_frame_from(tab), [tab]);
    // At the end: the green 10px at the window's bottom, so the page's
    // viewport is the window less the toolbar.
    assert_eq!(window.pixel(10, 199), GREEN);
    assert_eq!(window.pixel(10, 190), GREEN);
    assert_eq!(window.pixel(10, 185), [255, 0, 0, 255]);
    assert_eq!(window.pixel(10, TOOLBAR_Y as u32 + 1), TOOLBAR);
    window.close();
}

/// When the GPU is lost, the toolbar's tiles are rastered again and sent
/// in a frame of their own, drawn as before.
#[test]
fn the_toolbar_is_drawn_again_when_the_gpu_is_lost() {
    let server = server(&[("/", GREEN_PAGE)]);
    let gpu = Arc::new(CountingGpu::default());
    let mut window = Window::open_with_gpu(
        Size::new(400, 300),
        1.0,
        Some(gpu.clone()),
        GpuSharedImages::new(),
    );
    window.load(&server.url("/"));
    assert_eq!(window.pixel(1, TOOLBAR_Y as u32 + 1), TOOLBAR);
    let rastered = gpu.0.load(Ordering::Relaxed);
    assert!(rastered > 0);
    window.view.did_lose_context();
    assert!(gpu.0.load(Ordering::Relaxed) > rastered, "rastered again");
    assert_eq!(
        window.until_frame_from(BROWSER_FRAME_SINK),
        [BROWSER_FRAME_SINK]
    );
    assert_eq!(window.pixel(1, TOOLBAR_Y as u32 + 1), TOOLBAR);
    assert_eq!(window.pixel(1, 299), GREEN);
    window.close();
}

#[test]
fn clicks_in_the_page_are_below_the_toolbar() {
    let server = server(&[
        ("/", &links(&["/a", "/b"])),
        ("/a", GREEN_PAGE),
        ("/b", GREEN_PAGE),
    ]);
    for scale in [1.0, 1.5, 2.0] {
        let mut window = Window::open(window_size(scale), scale);
        window.load(&server.url("/"));
        // 70px down the window is 30px down the page: the first link.
        window.click(10.0, TOP + 30.0);
        window.until(|msg| matches!(msg, RendererHostMsg::DidCommitNavigation { .. }));
        assert_eq!(window.url(), Some(server.url("/a").as_str()), "at {scale}x");
        // The field shows where the tab went.
        assert_eq!(window.field_text(), server.url("/a"), "at {scale}x");
        window.close();
    }
}

/// A navigation through a redirect: the view hands its tab the redirect,
/// on the UI thread, and the tab goes on to where it leads.
#[test]
fn a_navigation_through_a_redirect_loads_where_it_leads() {
    let server = TestServer::start(HashMap::from([
        ("/old".to_owned(), Route::redirect(302, "/new")),
        ("/new".to_owned(), Route::ok("text/html", GREEN_PAGE)),
    ]))
    .expect("test server starts");
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/old"));
    assert_eq!(window.url(), Some(server.url("/new").as_str()));
    window.close();
}

#[test]
fn typing_in_the_address_field_navigates() {
    let server = server(&[("/", GREEN_PAGE), ("/next", "<title>next</title>")]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    // Cmd+L focuses and selects it all; typing replaces it. A host and
    // port with no scheme: `http://`, for an IP address.
    window.shortcut("l");
    assert!(window.view.ime_area().is_some());
    let typed = server.url("/next");
    window.type_text(typed.trim_start_matches("http://"));
    window.key(NamedKey::Enter);
    window.until(|msg| matches!(msg, RendererHostMsg::DidCommitNavigation { .. }));
    assert_eq!(window.url(), Some(typed.as_str()));
    assert!(!window.view.toolbar().omnibox.is_focused());
    assert_eq!(window.view.ime_area(), None);
    assert_eq!(window.field_text(), typed);
    window.close();
}

#[test]
fn the_field_gives_the_url_back() {
    let server = server(&[("/", GREEN_PAGE)]);
    let url = server.url("/");
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&url);
    let field = window.view.toolbar().field_rect();
    let in_field = (field.x + 20.0, TOOLBAR_Y + field.y + field.height / 2.0);
    // A click focuses it with everything selected; copy that.
    window.click(in_field.0, in_field.1);
    window.shortcut("c");
    assert_eq!(window.clipboard.0.borrow().as_deref(), Some(url.as_str()));
    // Typed over, then Escape: the URL is back.
    window.type_text("nonsense");
    assert_eq!(window.field_text(), "nonsense");
    window.key(NamedKey::Escape);
    assert_eq!(window.field_text(), url);
    // Typed over, then a click in the page: the same, and the page's keys
    // go to the page again.
    window.type_text("more");
    window.click(10.0, 200.0);
    assert_eq!(window.field_text(), url);
    assert!(!window.view.toolbar().omnibox.is_focused());
    // Paste and go.
    *window.clipboard.0.borrow_mut() = Some(format!("{url}#pasted"));
    window.click(in_field.0, in_field.1);
    window.shortcut("v");
    window.key(NamedKey::Enter);
    window.until(|msg| matches!(msg, RendererHostMsg::DidCommitNavigation { .. }));
    assert_eq!(window.url(), Some(format!("{url}#pasted").as_str()));
    window.close();
}

#[test]
fn the_back_and_forward_buttons() {
    let server = server(&[("/", &links(&["/two"])), ("/two", GREEN_PAGE)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    assert_eq!(window.view.toolbar().history(), (false, false));
    // The renderer asks for the link; once the navigation it starts has
    // committed, wait for that page.
    window.click(10.0, TOP + 10.0);
    window.until(|msg| matches!(msg, RendererHostMsg::DidCommitNavigation { .. }));
    window.until_loaded();
    assert_eq!(window.view.toolbar().history(), (true, false));
    // The back button: the first in the toolbar.
    let back = (6.0 + 14.0, TOOLBAR_Y + toolbar::HEIGHT / 2.0);
    let forward = (back.0 + 32.0, back.1);
    window.click(back.0, back.1);
    window.until_loaded();
    assert_eq!(window.url(), Some(server.url("/").as_str()));
    assert_eq!(window.view.toolbar().history(), (false, true));
    window.click(forward.0, forward.1);
    window.until_loaded();
    assert_eq!(window.url(), Some(server.url("/two").as_str()));
    window.close();
}

#[test]
fn the_cursor_follows_what_is_under_it() {
    let server = server(&[("/", &links(&["/a"]))]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    let field = window.view.toolbar().field_rect();
    window
        .view
        .pointer_moved(PointF::new(field.x + 20.0, TOOLBAR_Y + field.y + 10.0));
    assert_eq!(window.view.cursor(), CursorType::IBeam);
    window
        .view
        .pointer_moved(PointF::new(20.0, TOOLBAR_Y + 20.0));
    assert_eq!(window.view.cursor(), CursorType::Pointer);
    // Over the link the renderer asks for the hand.
    window.view.pointer_moved(PointF::new(10.0, TOP + 10.0));
    window.until(|msg| matches!(msg, RendererHostMsg::SetCursor(_)));
    assert_eq!(window.view.cursor(), CursorType::Hand);
    // Back over the toolbar it is the toolbar's.
    window
        .view
        .pointer_moved(PointF::new(20.0, TOOLBAR_Y + 20.0));
    assert_eq!(window.view.cursor(), CursorType::Pointer);
    window.close();
}

/// Where clicks are, page CSS px, in `#out`.
const CLICKS: &str = "<!DOCTYPE html><style>html, body { margin: 0; height: 100% }</style>
     <p id=out></p><script>onclick = e => document.getElementById('out').textContent +=
       Math.round(e.clientX) + ',' + Math.round(e.clientY) + ';'</script>";

/// Blue, its last column and row red: they show only if the renderer is
/// exactly as big as its web view.
const EDGES: &str = "<!DOCTYPE html><style>
     html { background-color: blue; height: 100%; box-sizing: border-box;
            border-right: 2px solid red; border-bottom: 2px solid red }
     body { margin: 0 }</style>";

#[test]
fn web_views_are_drawn_on_their_bounds_at_their_size() {
    let server = server(&[("/green", GREEN_PAGE), ("/edges", EDGES)]);
    for scale in [1.0, 1.5, 2.0] {
        let (first, second) = (server.url("/green"), server.url("/edges"));
        let mut window = Window::side_by_side(scale, false, &first, &second);
        // The toolbar follows the window's tab.
        assert_eq!(window.field_text(), first, "at {scale}x");
        let (a, b) = window.web_views(false);
        let right = |r: Rect| (r.x + r.width as i32) as u32;
        let bottom = |r: Rect| (r.y + r.height as i32) as u32;
        let (ax, ay, bx, by) = (a.x as u32, a.y as u32, b.x as u32, b.y as u32);
        // The first, to its last pixel; then the gap.
        assert_eq!(window.pixel(ax, ay), GREEN, "at {scale}x");
        assert_eq!(
            window.pixel(right(a) - 1, bottom(a) - 1),
            GREEN,
            "at {scale}x"
        );
        assert_ne!(window.pixel(right(a), by + 10), GREEN, "at {scale}x");
        // The second from its first pixel, which is no CSS px at 1.5x and
        // 2x; its renderer is its size: its borders on its last pixels.
        window.until_pixel(bx, by, BLUE);
        assert_ne!(window.pixel(bx - 1, by + 10), BLUE, "at {scale}x");
        assert_ne!(window.pixel(bx + 10, by - 1), BLUE, "at {scale}x");
        assert_eq!(window.pixel(right(b) - 1, by + 10), RED, "at {scale}x");
        assert_eq!(window.pixel(bx + 10, bottom(b) - 1), RED, "at {scale}x");
        let inside = (6.0 * scale) as u32;
        assert_eq!(
            window.pixel(right(b) - inside, bottom(b) - inside),
            BLUE,
            "at {scale}x"
        );
        window.close();
    }
}

#[test]
fn clicks_go_to_the_web_view_under_them_in_its_coordinates() {
    let server = server(&[("/", CLICKS)]);
    for scale in [1.0, 1.5, 2.0] {
        let url = server.url("/");
        let mut window = Window::side_by_side(scale, false, &url, &url);
        let (a, b) = window.web_views(false);
        let at = in_window(b, scale, 30.0, 20.0);
        // Where automation finds a page's point in the window too.
        let to_window = window
            .view
            .web_view_to_window(SECOND, RectF::new(30.0, 20.0, 1.0, 1.0))
            .unwrap();
        assert!(
            (to_window.x - at.x).abs() < 0.01 && (to_window.y - at.y).abs() < 0.01,
            "{to_window:?} at {scale}x"
        );
        window.click(at.x, at.y);
        window.until_out(SECOND, "30,20;");
        // Between the two: neither (each page has its clicks in order).
        let gap = (a.x + a.width as i32 + b.x) as f32 / 2.0 / scale;
        window.click(gap, at.y);
        let at = in_window(a, scale, 40.0, 25.0);
        window.click(at.x, at.y);
        window.until_out(TabId(0), "40,25;");
        let at = in_window(b, scale, 50.0, 60.0);
        window.click(at.x, at.y);
        window.until_out(SECOND, "30,20;50,60;");
        window.close();
    }
}

#[test]
fn a_later_web_view_is_over_an_earlier_one() {
    let server = server(&[
        ("/green", GREEN_PAGE),
        ("/edges", EDGES),
        ("/clicks", CLICKS),
    ]);
    let mut window = Window::side_by_side(1.0, true, &server.url("/green"), &server.url("/edges"));
    let (a, b) = window.web_views(true);
    let (x, y) = pixel_in(b, 1.0, 20.0, 20.0);
    assert!(x < (a.x + a.width as i32) as u32, "over the first");
    window.until_pixel(x, y, BLUE);
    window.close();

    let url = server.url("/clicks");
    let mut window = Window::side_by_side(1.0, true, &url, &url);
    let at = in_window(b, 1.0, 20.0, 20.0);
    window.click(at.x, at.y);
    window.until_out(SECOND, "20,20;");
    let at = in_window(a, 1.0, 30.0, 30.0);
    window.click(at.x, at.y);
    window.until_out(TabId(0), "30,30;");
    window.close();
}

/// A link far right of the page, out of a narrow viewport but under the
/// pointer when it has gone right of the page.
const FAR_LINK: &str = "<!DOCTYPE html><style>html, body { margin: 0; height: 100% }
     a { display: block; margin-left: 300px; width: 2000px; height: 2000px }</style>
     <p id=out></p><script>onclick = e => document.getElementById('out').textContent +=
       Math.round(e.clientX) + ',' + Math.round(e.clientY) + ';'</script>
     <a href='#far'></a>";

/// All link.
const ALL_LINK: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     a { display: block; height: 2000px }</style><a href='#all'></a>";

#[test]
fn the_web_view_a_drag_starts_in_has_it() {
    let server = server(&[("/far", FAR_LINK), ("/all", ALL_LINK)]);
    let scale = 1.5;
    let mut window = Window::side_by_side(scale, false, &server.url("/far"), &server.url("/all"));
    let (a, b) = window.web_views(false);
    let over_b = in_window(a, scale, 250.0, 110.0);
    assert!(over_b.x * scale > b.x as f32 && over_b.y * scale > b.y as f32);
    // Each web view has its renderer's cursor.
    window.view.pointer_moved(over_b);
    window.until_cursor_from(SECOND);
    assert_eq!(window.view.cursor(), CursorType::Hand);
    window.view.pointer_moved(in_window(a, scale, 20.0, 110.0));
    assert_eq!(window.view.cursor(), CursorType::Pointer);
    // Down in the first, then over the second: the first has the pointer,
    // its cursor and its moves, in its coordinates.
    window.view.mouse_input(MouseButton::Left, true);
    window.view.pointer_moved(over_b);
    assert_eq!(window.view.cursor(), CursorType::Pointer);
    let far = in_window(a, scale, 350.0, 110.0);
    window.view.pointer_moved(far);
    window.until_cursor_from(TabId(0));
    assert_eq!(window.view.cursor(), CursorType::Hand);
    // And the up: a click around both.
    window.view.mouse_input(MouseButton::Left, false);
    window.until_out(TabId(0), "350,110;");
    // Then it's free: over the address field, the field's.
    let field = window.view.toolbar().field_rect();
    window
        .view
        .pointer_moved(PointF::new(field.x + 20.0, TOOLBAR_Y + field.y + 10.0));
    assert_eq!(window.view.cursor(), CursorType::IBeam);
    window.close();
}

/// Logs the mouse's events into `#out`: each one's type, `buttons`,
/// `detail` and whether Shift is held.
const MOUSE_LOG: &str = "<!DOCTYPE html><style>html, body { margin: 0; height: 100% }</style>
     <p id=out></p><script>for (const type of ['mousedown', 'mousemove', 'mouseup', 'dblclick', 'wheel'])
       addEventListener(type, e => document.getElementById('out').textContent +=
         `${e.type}:${e.buttons}:${e.detail}:${e.shiftKey};`)</script>";

/// The page's mouse events (a wheel's too) have the buttons down, the keys
/// held and each button's click count (its release its press's), as the
/// window gives them.
#[test]
fn the_mouse_carries_its_buttons_keys_and_click_count() {
    let server = server(&[("/", MOUSE_LOG)]);
    let url = server.url("/");
    let mut window = Window::side_by_side(1.0, false, &url, &url);
    let (a, _) = window.web_views(false);
    let mut out = String::new();
    let mut expect = |window: &mut Window, events: &str| {
        out.push_str(events);
        window.until_out(TabId(0), &out);
    };
    window.view.pointer_moved(in_window(a, 1.0, 20.0, 50.0));
    expect(&mut window, "mousemove:0:0:false;");
    window
        .view
        .mouse_input_with_count(MouseButton::Right, true, 1);
    expect(&mut window, "mousedown:2:1:false;");
    window.view.set_modifiers(ModifiersState::SHIFT);
    window.view.pointer_moved(in_window(a, 1.0, 21.0, 50.0));
    expect(&mut window, "mousemove:2:0:true;");
    window.view.wheel(ScrollDelta::Pixels { x: 0.0, y: 10.0 });
    expect(&mut window, "wheel:2:0:true;");
    window
        .view
        .mouse_input_with_count(MouseButton::Left, true, 2);
    expect(&mut window, "mousedown:3:2:true;");
    window
        .view
        .mouse_input_with_count(MouseButton::Left, false, 0);
    expect(&mut window, "mouseup:2:2:true;dblclick:2:2:true;");
    window
        .view
        .mouse_input_with_count(MouseButton::Right, false, 0);
    expect(&mut window, "mouseup:0:1:true;");
    window.close();
}

/// A scroller at the top left, 100px square: red, green when scrolled.
const SCROLLER: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     #s { width: 100px; height: 100px; overflow: scroll }</style>
     <div id=s><div style='height: 100px; background-color: red'></div>
     <div style='height: 300px; background-color: green'></div></div>";

#[test]
fn the_wheel_goes_to_the_web_view_under_it() {
    let server = server(&[("/clicks", CLICKS), ("/scroller", SCROLLER)]);
    let scale = 1.5;
    let mut window = Window::side_by_side(
        scale,
        false,
        &server.url("/clicks"),
        &server.url("/scroller"),
    );
    let (a, b) = window.web_views(false);
    let (x, y) = pixel_in(b, scale, 5.0, 5.0);
    window.until_pixel(x, y, RED);
    // Even with the first holding the pointer, the wheel goes to the
    // second, at its scroller: where it is in the second.
    let down = in_window(a, scale, 20.0, 20.0);
    window.view.pointer_moved(down);
    window.view.mouse_input(MouseButton::Left, true);
    window.view.pointer_moved(in_window(b, scale, 10.0, 10.0));
    window.view.wheel(ScrollDelta::Pixels { x: 0.0, y: 100.0 });
    window.until_pixel(x, y, GREEN);
    window.view.mouse_input(MouseButton::Left, false);
    window.close();
}

/// Red at the top, a link; green under it.
const STRIPES: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     a { display: block; height: 100px; background-color: red }</style>
     <a href='#top'></a><div style='height: 3000px; background-color: green'></div>";

#[test]
fn keys_go_to_the_web_view_pressed_last() {
    let server = server(&[("/", STRIPES)]);
    let url = server.url("/");
    let mut window = Window::side_by_side(1.0, false, &url, &url);
    let (a, b) = window.web_views(false);
    let (ax, ay) = pixel_in(a, 1.0, 5.0, 5.0);
    let (bx, by) = pixel_in(b, 1.0, 5.0, 5.0);
    window.until_pixel(bx, by, RED);
    // At first, to the window's tab.
    window.key(NamedKey::End);
    window.until_pixel(ax, ay, GREEN);
    assert_eq!(window.pixel(bx, by), RED);
    // Any button gives a web view the keys.
    let in_b = in_window(b, 1.0, 50.0, 150.0);
    window.view.pointer_moved(in_b);
    window.view.mouse_input(MouseButton::Right, true);
    window.view.mouse_input(MouseButton::Right, false);
    window.key(NamedKey::End);
    window.until_pixel(bx, by, GREEN);
    // Gone while it has the keys and the pointer: both go back.
    window.view.mouse_input(MouseButton::Left, true);
    let mut gone = window.view.remove_web_view(SECOND).expect("shown");
    gone.shutdown();
    assert!(
        window.view.remove_web_view(TabId(0)).is_none(),
        "the tab stays"
    );
    let drawn = window.pixel(bx, by);
    assert!(drawn != GREEN && drawn != RED, "not drawn: {drawn:?}");
    window.key(NamedKey::Home);
    window.until_pixel(ax, ay, RED);
    // The button still down, the moves go to what is under the pointer.
    window.view.pointer_moved(in_window(a, 1.0, 10.0, 10.0));
    window.until_cursor_from(TabId(0));
    assert_eq!(window.view.cursor(), CursorType::Hand);
    window.view.mouse_input(MouseButton::Left, false);
    window.close();
}

const BLUE_PAGE: &str =
    "<!DOCTYPE html><title>blue</title><style>html { background-color: blue }</style>";

#[test]
fn tabs_open_switch_and_close_with_shortcuts() {
    let server = server(&[("/green", GREEN_PAGE), ("/blue", BLUE_PAGE)]);
    let (green, blue) = (server.url("/green"), server.url("/blue"));
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&green);
    // A new tab, last and shown, the address field taking the keys.
    window.shortcut("t");
    assert_eq!(window.view.tab_count(), 2);
    assert_eq!(window.view.active_tab_index(), 1);
    assert!(
        window.view.ime_area().is_some(),
        "the address field is focused"
    );
    window.type_text(blue.trim_start_matches("http://"));
    window.key(NamedKey::Enter);
    window.until_loaded();
    window.until_pixel(1, 299, BLUE);
    assert_eq!(window.view.title(), "blue");
    assert_eq!(window.field_text(), blue);
    // Ctrl+Tab: the first, as it was; the toolbar follows it.
    window.ctrl(NamedKey::Tab, false);
    assert_eq!(window.view.active_tab_index(), 0);
    window.until_pixel(1, 299, GREEN);
    assert_eq!(window.view.title(), "green");
    assert_eq!(window.field_text(), green);
    assert!(window.view.ime_area().is_none(), "the page has the keys");
    window.ctrl(NamedKey::Tab, true);
    assert_eq!(window.view.active_tab_index(), 1, "around the start");
    window.shortcut("1");
    assert_eq!(window.view.active_tab_index(), 0);
    window.shortcut("9");
    assert_eq!(window.view.active_tab_index(), 1);
    // Closing the shown tab shows the one left.
    window.shortcut("1");
    window.shortcut("w");
    assert_eq!(window.view.tab_count(), 1);
    assert_eq!(*window.closed.borrow(), [TabId(0)]);
    window.until_pixel(1, 299, BLUE);
    assert_eq!(window.view.title(), "blue");
    assert_eq!(window.field_text(), blue);
    // The last closes the window instead.
    assert!(!window.view.close_requested());
    window.shortcut("w");
    assert!(window.view.close_requested());
    assert_eq!(window.view.tab_count(), 1);
    window.close();
}

/// Asks for a frame after each one, changing it.
const ANIMATED: &str = "<!DOCTYPE html><style>.a { background-color: green }</style>
     <script>let n = 0;
       function frame() { document.documentElement.setAttribute('class', n++ % 2 ? 'a' : '');
                          requestAnimationFrame(frame); }
       requestAnimationFrame(frame);</script>";

#[test]
fn a_hidden_tab_asks_for_no_frames() {
    let server = server(&[("/", ANIMATED)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    let first = window.view.web_contents().frame_sink_id();
    window.until_frame_from(first);
    window.until_frame_from(first);
    // Hidden, it stops asking for BeginFrames, though it still wants
    // animation frames.
    window.shortcut("t");
    assert!(window.view.ime_area().is_some());
    window.until_viz(|msg| {
        matches!(msg, VizMsg::FrameSink(sink, CompositorFrameSinkMsg::SetNeedsBeginFrame(false)) if *sink == first)
    });
    // Shown again, it asks, and its frames come. The address field gives
    // the keys back, showing the tab's URL.
    window.ctrl(NamedKey::Tab, false);
    assert!(window.view.ime_area().is_none());
    assert_eq!(window.field_text(), server.url("/"));
    window.until_viz(|msg| {
        matches!(msg, VizMsg::FrameSink(sink, CompositorFrameSinkMsg::SetNeedsBeginFrame(true)) if *sink == first)
    });
    window.until_frame_from(first);
    window.until_frame_from(first);
    window.close();
}

/// A tab whose renderer has gone shows its sad tab, the last one too;
/// its button, or reloading, launches another renderer. A tab behind shows
/// its own once shown.
#[test]
fn a_tab_whose_renderer_has_gone_shows_its_sad_tab() {
    // A box, not the root's background (which fills a frame of any size):
    // drawn only by a renderer told its size.
    let server = server(&[(
        "/green",
        "<!DOCTYPE html><title>green</title><style>html, body { margin: 0 }
         div { height: 200px; background-color: green }</style><div></div>",
    )]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/green"));
    let page = (5, TOP as u32 + 5);
    let sad = Color::rgb(248, 249, 250).premultiplied();
    window.view.navigate("about:crash");
    window.until_pixel(page.0, page.1, sad);
    assert_eq!(window.view.tab_count(), 1);
    assert!(!window.view.close_requested());
    assert_eq!(window.view.title(), "green");
    // Its button: the page again, in a new renderer.
    let page_size = SizeF::new(400.0, 300.0 - TOP);
    let button = SadTab::reload_rect(page_size);
    window.click(
        button.x + button.width / 2.0,
        TOP + button.y + button.height / 2.0,
    );
    assert!(!window.view.web_contents().is_crashed());
    // Nothing until the new renderer draws: not the old one's last frame.
    assert_eq!(window.pixel(page.0, page.1), [255, 255, 255, 255]);
    window.until_loaded();
    window.until_pixel(page.0, page.1, GREEN);
    // Gone on its own; reloading brings it back too.
    window
        .view
        .on_renderer_message(TabId(0), &RendererHostMsg::RendererGone);
    window.until_pixel(page.0, page.1, sad);
    window.shortcut("r");
    window.until_loaded();
    window.until_pixel(page.0, page.1, GREEN);
    // Behind, seen once shown.
    window.shortcut("t");
    window.until_loaded();
    window
        .view
        .on_renderer_message(TabId(0), &RendererHostMsg::RendererGone);
    assert_eq!(window.pixel(page.0, page.1), [255, 255, 255, 255]);
    window.shortcut("1");
    window.until_pixel(page.0, page.1, sad);
    window.close();
}

#[test]
fn keys_and_the_pointer_go_to_the_shown_tab() {
    let server = server(&[("/stripes", STRIPES)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/stripes"));
    // Down in the first tab's page (under its link), which has the pointer.
    window.view.pointer_moved(PointF::new(10.0, TOP + 150.0));
    window.view.mouse_input(MouseButton::Left, true);
    window.shortcut("t");
    window.until_loaded();
    assert_eq!(window.url(), Some("about:blank"));
    window.load(&server.url("/stripes"));
    // Another tab shown, the pointer is free: its moves go to that one.
    window.view.pointer_moved(PointF::new(20.0, TOP + 50.0));
    window.until_cursor_from(TabId(1));
    assert_eq!(window.view.cursor(), CursorType::Hand);
    window.view.mouse_input(MouseButton::Left, false);
    // The keys are the shown tab's: the second's here, the first left as
    // it was.
    window.shortcut("1");
    window.shortcut("2");
    let top = (5, (TOP + 5.0) as u32);
    window.key(NamedKey::End);
    window.until_pixel(top.0, top.1, GREEN);
    window.shortcut("1");
    window.until_pixel(top.0, top.1, RED);
    window.key(NamedKey::End);
    window.until_pixel(top.0, top.1, GREEN);
    window.close();
}

#[test]
fn the_tab_strip_switches_closes_and_opens_tabs() {
    let server = server(&[("/green", GREEN_PAGE), ("/blue", BLUE_PAGE)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/green"));
    window.shortcut("t");
    window.until_loaded();
    // A page with no title: its URL.
    assert_eq!(window.view.tab_strip().titles(), ["green", "about:blank"]);
    window.load(&server.url("/blue"));
    assert_eq!(window.view.tab_strip().titles(), ["green", "blue"]);
    // Navigated as if from the address field, which shows where it went.
    assert!(window.view.ime_area().is_none());
    assert_eq!(window.field_text(), server.url("/blue"));
    let strip = |window: &Window, at: fn(&TabStrip) -> RectF| at(window.view.tab_strip());
    let middle = |rect: RectF| PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let first = middle(strip(&window, |s| s.tab_rect(0)));
    // The active tab is the toolbar's color, the other the strip's.
    let above_title = |at: PointF| (at.x as u32, at.y as u32 - 10);
    let (x, y) = above_title(middle(strip(&window, |s| s.tab_rect(1))));
    assert_eq!(window.pixel(x, y), TOOLBAR);
    let (x, y) = above_title(first);
    assert_eq!(window.pixel(x, y), TAB_STRIP);
    // Hovered, until the pointer leaves the strip.
    window.view.pointer_moved(first);
    assert_eq!(window.pixel(x, y), HOVERED_TAB);
    window.view.pointer_moved(PointF::new(10.0, TOP + 10.0));
    assert_eq!(window.pixel(x, y), TAB_STRIP);
    // A press on a tab shows it.
    window.click(first.x, first.y);
    assert_eq!(window.view.active_tab_index(), 0);
    window.until_pixel(1, TOP as u32 + 1, GREEN);
    assert_eq!(window.pixel(x, y), TOOLBAR);
    // Its close button closes the other.
    let close = middle(strip(&window, |s| s.close_rect(1)));
    window.click(close.x, close.y);
    assert_eq!(window.view.tab_count(), 1);
    assert_eq!(*window.closed.borrow(), [TabId(1)]);
    assert_eq!(window.view.tab_strip().titles(), ["green"]);
    // The new tab button opens one, the address field focused.
    let new_tab = middle(strip(&window, TabStrip::new_tab_rect));
    window.click(new_tab.x, new_tab.y);
    assert_eq!(window.view.tab_count(), 2);
    assert_eq!(window.view.active_tab_index(), 1);
    assert!(window.view.ime_area().is_some());
    // The middle button closes the tab it clicks.
    let second = middle(strip(&window, |s| s.tab_rect(1)));
    window.view.pointer_moved(second);
    window.view.mouse_input(MouseButton::Middle, true);
    window.view.mouse_input(MouseButton::Middle, false);
    assert_eq!(window.view.tab_count(), 1);
    window.close();
}

/// Three links, 50px high each: to a new tab (`target`), to green, and
/// one to blue whose middle click (`auxclick`) is canceled (there is no
/// `onauxclick` attribute yet).
const LINKS: &str =
    "<!DOCTYPE html><style>html, body { margin: 0 } a { display: block; height: 50px }</style>
     <a href='/blue' target='_blank'></a><a href='/green'></a>
     <a id=canceled href='/blue'></a>
     <script>document.getElementById('canceled')
       .addEventListener('auxclick', event => event.preventDefault())</script>";

#[test]
fn links_open_tabs_where_the_click_says() {
    let server = server(&[("/", LINKS), ("/green", GREEN_PAGE), ("/blue", BLUE_PAGE)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/"));
    let opened = |msg: &RendererHostMsg| matches!(msg, RendererHostMsg::OpenUrl { .. });
    let (blank, green, canceled) = (TOP + 25.0, TOP + 75.0, TOP + 125.0);
    // `target`: a new tab, shown.
    window.click(10.0, blank);
    window.until(opened);
    assert_eq!(window.view.tab_count(), 2);
    assert_eq!(window.view.active_tab_index(), 1);
    window.until_loaded();
    assert_eq!(window.url(), Some(server.url("/blue").as_str()));
    // Closed, back to the tab it was opened from.
    window.shortcut("w");
    assert_eq!(window.view.active_tab_index(), 0);
    // The middle button: a tab behind, right of this one.
    let middle = |window: &mut Window, y: f32| {
        window.view.pointer_moved(PointF::new(10.0, y));
        window.view.mouse_input(MouseButton::Middle, true);
        window.view.mouse_input(MouseButton::Middle, false);
    };
    middle(&mut window, green);
    window.until(opened);
    assert_eq!(window.view.tab_count(), 2);
    assert_eq!(window.view.active_tab_index(), 0);
    // Hidden, as big as the tab shown.
    let behind = window.view.contents(TabId(2)).expect("the tab behind");
    assert!(!behind.is_visible());
    assert_eq!(behind.viewport_size(), Size::new(400, 300 - TOP as u32));
    assert!(window.view.web_contents().is_visible());
    // The platform's key for a new tab: behind too, after the one before.
    let key = if cfg!(target_os = "macos") {
        ModifiersState::SUPER
    } else {
        ModifiersState::CONTROL
    };
    window.view.set_modifiers(key);
    window.click(10.0, green);
    window.until(opened);
    assert_eq!(window.view.tab_count(), 3);
    assert_eq!(window.view.active_tab_index(), 0);
    // With Shift: shown, after those two.
    window.view.set_modifiers(key | ModifiersState::SHIFT);
    window.click(10.0, green);
    window.view.set_modifiers(ModifiersState::empty());
    window.until(opened);
    assert_eq!(window.view.tab_count(), 4);
    assert_eq!(window.view.active_tab_index(), 3);
    window.until_loaded();
    // A new tab at the end, then back to the last one opened and closed:
    // to the tab it came from, not its neighbor.
    window.shortcut("t");
    window.shortcut("4");
    window.shortcut("w");
    assert_eq!(window.view.active_tab_index(), 0);
    assert_eq!(window.view.tab_count(), 4);
    // A canceled middle click opens nothing: the next one opens the first
    // tab after it.
    middle(&mut window, canceled);
    middle(&mut window, green);
    let mut first = None;
    window.until(|msg| match msg {
        RendererHostMsg::OpenUrl { url, .. } => {
            first = Some(url.clone());
            true
        }
        _ => false,
    });
    assert_eq!(first, Some(server.url("/green")));
    assert_eq!(window.view.tab_count(), 5);
    window.close();
}

/// Says its document's visibility state in its title, as it starts and
/// as it changes.
const VISIBILITY: &str = "<!DOCTYPE html><script>document.title = document.visibilityState;
     document.addEventListener('visibilitychange',
       () => { document.title = document.visibilityState; })</script>";

/// A tab opened behind (by a link's middle click, or by what drives the
/// browser) is hidden before its first document commits, so that document
/// starts hidden; shown, it becomes visible.
#[test]
fn a_tab_opened_behind_starts_hidden() {
    let server = server(&[("/", &links(&["/visibility"])), ("/visibility", VISIBILITY)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/visibility"));
    assert_eq!(window.view.web_contents().title(), "visible");
    window.load(&server.url("/"));
    window.view.pointer_moved(PointF::new(10.0, TOP + 25.0));
    window.view.mouse_input(MouseButton::Middle, true);
    window.view.mouse_input(MouseButton::Middle, false);
    let title = |of: TabId, wanted: &'static str| {
        move |tab: TabId, msg: &RendererHostMsg| {
            tab == of && matches!(msg, RendererHostMsg::UpdateTitle(title) if title == wanted)
        }
    };
    window.until_from(title(TabId(1), "hidden"));
    assert_eq!(window.view.active_tab_index(), 0);
    window.ctrl(NamedKey::Tab, false);
    window.until_from(title(TabId(1), "visible"));
    let behind = window
        .view
        .open_tab(&server.url("/visibility"), true)
        .expect("a tab");
    window.until_from(title(behind, "hidden"));
    assert!(!window.view.contents(behind).unwrap().is_visible());
    window.close();
}

/// A closed tab's id isn't given again: its renderer's last messages may
/// still come, and are no other tab's.
#[test]
fn a_closed_tabs_id_isnt_given_again() {
    let server = server(&[("/green", GREEN_PAGE)]);
    let mut window = Window::open(Size::new(400, 300), 1.0);
    window.load(&server.url("/green"));
    window.shortcut("t");
    assert_eq!(window.view.web_contents().tab(), TabId(1));
    window.shortcut("w");
    window.shortcut("t");
    assert_eq!(window.view.web_contents().tab(), TabId(2));
    // The closed one's renderer going: nothing to do with this one.
    window
        .view
        .on_renderer_message(TabId(1), &RendererHostMsg::RendererGone);
    assert_eq!(window.view.tab_count(), 2);
    window.close();
}

/// A select at the page's top left; its value in `#out`.
const SELECT: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     select { display: block; width: 100px; height: 20px }</style>
     <select id=s><option>one</option><option>two</option><option>three</option></select>
     <p id=out>one</p>
     <script>const s = document.getElementById('s');
     s.onchange = () => { document.getElementById('out').textContent = s.value; };</script>";

/// A select's dropdown: a click opens it below the select, drawn by the
/// browser over the page's surface; ↓ and Enter pick the next option,
/// which the page hears. A press on the toolbar closes it; one on the
/// select while it is shown closes it, and doesn't open it again; the tab
/// shown changing closes it.
#[test]
fn a_selects_dropdown_is_over_the_page() {
    a_selects_dropdown(Window::open(window_size(1.0), 1.0));
}

fn a_selects_dropdown(mut window: Window) {
    let server = server(&[("/", SELECT)]);
    window.load(&server.url("/"));
    let shown = |msg: &RendererHostMsg| matches!(msg, RendererHostMsg::ShowPopupMenu(_));
    window.click(10.0, TOP + 10.0);
    window.until(shown);
    let rect = window.view.popup_menu().expect("shown").rect();
    assert_eq!((rect.x, rect.y), (0.0, TOP + 20.0));
    // Its first row, highlighted (`SelectedItem`), over the page.
    window.until_pixel(5, (rect.y + 3.0) as u32, [25, 103, 210, 255]);
    window.key(NamedKey::ArrowDown);
    window.key(NamedKey::Enter);
    assert!(window.view.popup_menu().is_none());
    window.until_out(TabId(0), "two");

    window.click(10.0, TOP + 10.0);
    window.until(shown);
    window.click(390.0, TOOLBAR_Y + 5.0);
    assert!(window.view.popup_menu().is_none(), "the toolbar's press");

    window.click(10.0, TOP + 10.0);
    window.until(shown);
    window.click(10.0, TOP + 10.0);
    window.until(|msg| matches!(msg, RendererHostMsg::HidePopupMenu));
    // Everything the presses asked is in once `#out` is read.
    assert_eq!(window.out_of(TabId(0)), "two");
    assert!(window.view.popup_menu().is_none(), "not shown again");

    // A click on its third row picks it.
    window.click(10.0, TOP + 10.0);
    window.until(shown);
    let rect = window.view.popup_menu().expect("shown").rect();
    let row = (rect.height - 2.0) / 3.0;
    window.click(10.0, rect.y + 1.0 + 2.5 * row);
    assert!(window.view.popup_menu().is_none());
    window.until_out(TabId(0), "three");
    // A press elsewhere on the page closes it, by the renderer.
    window.click(10.0, TOP + 10.0);
    window.until(shown);
    window.click(300.0, TOP + 200.0);
    window.until(|msg| matches!(msg, RendererHostMsg::HidePopupMenu));
    assert!(window.view.popup_menu().is_none(), "a press on the page");
    // So does a wheel outside it.
    window.click(10.0, TOP + 10.0);
    window.until(shown);
    window.view.pointer_moved(PointF::new(300.0, TOP + 200.0));
    window.view.wheel(ScrollDelta::Pixels { x: 0.0, y: 10.0 });
    assert!(window.view.popup_menu().is_none(), "a wheel");
    assert_eq!(window.out_of(TabId(0)), "three");

    window.click(10.0, TOP + 10.0);
    window.until(shown);
    window.view.resize(Size::new(410, 300), 1.0);
    assert!(window.view.popup_menu().is_none(), "a resize closes it");
    window.close();
}

/// Only the tab shown has a dropdown: one asked for by another web view's
/// renderer isn't shown.
#[test]
fn only_the_tab_shown_has_a_dropdown() {
    let server = server(&[("/", SELECT)]);
    let url = server.url("/");
    let mut window = Window::side_by_side(1.0, false, &url, &url);
    let menu = protocol::renderer::PopupMenu {
        bounds: RectF::new(0.0, 0.0, 100.0, 20.0),
        font_size: 13.0,
        color: Color::BLACK,
        items: Vec::new(),
        selected: None,
        right_aligned: false,
    };
    window
        .view
        .on_renderer_message(SECOND, &RendererHostMsg::ShowPopupMenu(menu.clone()));
    assert!(window.view.popup_menu().is_none());
    window
        .view
        .on_renderer_message(TabId(0), &RendererHostMsg::ShowPopupMenu(menu));
    assert!(window.view.popup_menu().is_some());
    window.close();
}

/// A select, and an element whose presses don't move the focus.
const SELECT_AND_STILL: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     select { display: block; width: 100px; height: 20px } div { height: 50px }</style>
     <select id=s><option>one</option><option>two</option></select>
     <div onmousedown='event.preventDefault()'>still</div>";

/// A press on the page closes the list, by the renderer, though the select
/// keeps the focus (its `mousedown` canceled).
#[test]
fn a_press_that_keeps_the_focus_closes_the_dropdown() {
    let mut window = Window::open(window_size(1.0), 1.0);
    let server = server(&[("/", SELECT_AND_STILL)]);
    window.load(&server.url("/"));
    window.click(10.0, TOP + 10.0);
    window.until(|msg| matches!(msg, RendererHostMsg::ShowPopupMenu(_)));
    // Beside the list, on the element.
    window.click(300.0, TOP + 40.0);
    window.until(|msg| matches!(msg, RendererHostMsg::HidePopupMenu));
    assert!(window.view.popup_menu().is_none());
    window.close();
}

/// A list box of three rows and five options; whether it scrolled, and
/// what is selected, in `#out` at each change.
const LIST_BOX: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     select { display: block; width: 100px }</style>
     <select id=s size=3 multiple><option>a</option><option>b</option><option>c</option><option>d</option>
     <option>e</option></select><p id=out>none</p>
     <script>const s = document.getElementById('s');
     s.onchange = () => {
       const picked = [...s.selectedOptions].map(o => o.text).join(' ');
       document.getElementById('out').textContent = picked + (s.scrollTop > 0 ? ' scrolled' : ' top');
     };</script>";

/// A list box: a click selects the option under it, Shift+↓ extends the
/// range, and the option it goes to below the rows shown scrolls into view
/// in it (the page doesn't); Home selects the first alone and scrolls back.
#[test]
fn a_list_box_scrolls_to_its_selection() {
    let server = server(&[("/", LIST_BOX)]);
    let mut window = Window::open(window_size(1.0), 1.0);
    window.load(&server.url("/"));
    // The second row.
    window.click(10.0, TOP + 22.0);
    window.until_out(TabId(0), "b top");
    window.view.set_modifiers(ModifiersState::SHIFT);
    window.key(NamedKey::ArrowDown);
    window.key(NamedKey::ArrowDown);
    window.view.set_modifiers(ModifiersState::empty());
    window.until_out(TabId(0), "b c d scrolled");
    window.key(NamedKey::Home);
    window.until_out(TabId(0), "a top");
    window.close();
}

/// A select that a task removes after the click that opens its list.
const REMOVED_SELECT: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     select { display: block; width: 100px; height: 20px }</style>
     <select id=s><option>one</option><option>two</option></select>
     <script>const s = document.getElementById('s');
     s.addEventListener('mouseup', () => setTimeout(() => s.remove(), 0));</script>";

/// A select whose focus a task moves away after the click that opens its
/// list.
const BLURRED_SELECT: &str = "<!DOCTYPE html><style>html, body { margin: 0 }
     select { display: block; width: 100px; height: 20px }</style>
     <select id=s><option>one</option><option>two</option></select><input id=i>
     <script>const s = document.getElementById('s');
     s.addEventListener('mouseup', () => setTimeout(() => document.getElementById('i').focus(), 0));
     </script>";

/// The list of a select that loses the focus closes.
#[test]
fn a_blurred_selects_dropdown_closes() {
    a_blurred_selects_dropdown(Window::open(window_size(1.0), 1.0));
}

fn a_blurred_selects_dropdown(mut window: Window) {
    let server = server(&[("/", BLURRED_SELECT)]);
    window.load(&server.url("/"));
    window.click(10.0, TOP + 10.0);
    window.until(|msg| matches!(msg, RendererHostMsg::ShowPopupMenu(_)));
    window.until(|msg| matches!(msg, RendererHostMsg::HidePopupMenu));
    assert!(window.view.popup_menu().is_none());
    window.close();
}

/// The renderer closes the list of a select taken out of its document.
#[test]
fn a_removed_selects_dropdown_closes() {
    a_removed_selects_dropdown(Window::open(window_size(1.0), 1.0));
}

fn a_removed_selects_dropdown(mut window: Window) {
    let server = server(&[("/", REMOVED_SELECT)]);
    window.load(&server.url("/"));
    window.click(10.0, TOP + 10.0);
    window.until(|msg| matches!(msg, RendererHostMsg::ShowPopupMenu(_)));
    window.until(|msg| matches!(msg, RendererHostMsg::HidePopupMenu));
    assert!(window.view.popup_menu().is_none());
    window.close();
}
