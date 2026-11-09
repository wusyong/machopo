//! Windowed mode: winit on the UI thread, Viz presenting through wgpu on its
//! own thread. This file only translates winit's events for the
//! [`BrowserView`](shell::browser_view::BrowserView) and applies what it
//! asks of the window (cursor, title, IME); the rest of the browser is a
//! [`BrowserWindow`].
//!
//! With `--automation`, a scenario drives the window (`shell::automation`),
//! and with `--automation-pipe --window`, what reads and writes machopo's
//! standard input and output does (`shell::automation_pipe`): both through
//! the window's `AutomationSession`, whose input goes in as winit's events
//! would. The platform's own input is ignored meanwhile, as
//! `interactive_ui_tests` own the window they drive. With
//! `--remote-debugging-port` the session has the server's clients too
//! (`shell::devtools_http_handler`), in a window a person may be using.

use std::sync::Arc;
use std::time::Instant;

use base::numerics::f32_from_f64;
use browser::{ContentsEvent, NetworkContextParams};
use crossbeam_channel::unbounded;
use gfx::{PointF, RectF, Size};
use protocol::TabId;
use protocol::automation::Message;
use protocol::input::{MouseButton, ScrollDelta};
use protocol::renderer::CursorType;
use protocol::viz::{ContextLostObserver, DisplayMsg, SharedImageInterface, VizMsg};
use shell::automation::{self, Client, Context, Failure};
use shell::automation_pipe::{self, Read};
use shell::automation_session::{AutomationSession, ClientEvent, Input, Reply};
use shell::browser_view::Command;
use shell::browser_window::{self, BrowserWindow};
use shell::devtools_http_handler::{self, DevToolsHttpHandler};
use shell::keyboard::KeyInput;
use shell::mouse::{ClickCounter, double_click_interval};
use shell::omnibox::Clipboard;
use shell::scenario::Scenario;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, Ime, MouseButton as WinitButton, MouseScrollDelta, TouchPhase,
    WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Window, WindowId, WindowLevel};

/// Events posted to the UI thread from other threads.
#[derive(Debug)]
enum ShellEvent {
    /// From a tab's renderer, or its navigation. Boxed: a navigation's
    /// response is much bigger than the other events.
    FromContents(TabId, Box<ContentsEvent>),
    /// The GPU was lost, and Viz has a new one: the toolbar's tiles go on
    /// it again.
    ContextLost,
    /// Viz asks for a new surface for the window (only the thread that
    /// made the window can make one).
    MakeSurface(crossbeam_channel::Sender<Result<wgpu::Surface<'static>, String>>),
    /// From the scenario's thread, or the pipe's reader.
    Automation(Read),
    /// The scenario has run, or the pipe's input has ended (`Ok`).
    AutomationDone(Result<(), Failure>),
    /// From the remote debugging server's thread.
    DevTools(ClientEvent),
}

/// A scenario for the window to run (`machopo --automation`).
pub struct Automation {
    pub scenario: Scenario,
    pub context: Context,
}

/// What drives the window.
pub enum Driver {
    /// `machopo --automation`.
    Scenario(Automation),
    /// `machopo --automation-pipe --window`, with the network settings given.
    Pipe(NetworkContextParams),
}

/// The window's half of a run that drives it.
struct Automating {
    /// Until the window is up and what drives it starts.
    pending: Option<Driver>,
    /// The pipe's writer, to wait for once the session has gone.
    writer: Option<std::thread::JoinHandle<()>>,
    /// Set while the window gives itself the driver's input, which goes
    /// through; the platform's doesn't.
    injecting: bool,
    result: Option<Result<(), Failure>>,
}

/// The platform's input, which the window ignores while a scenario drives
/// it.
fn is_input(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::CursorMoved { .. }
            | WindowEvent::CursorEntered { .. }
            | WindowEvent::CursorLeft { .. }
            | WindowEvent::MouseInput { .. }
            | WindowEvent::MouseWheel { .. }
            | WindowEvent::KeyboardInput { .. }
            | WindowEvent::ModifiersChanged(_)
            | WindowEvent::Ime(_)
            | WindowEvent::Touch(_)
    )
}

/// Tells the UI thread the GPU was lost (Chromium's `ui::Compositor` hears
/// it through its `LayerTreeFrameSink`).
struct ToolbarContextLost(EventLoopProxy<ShellEvent>);

impl ContextLostObserver for ToolbarContextLost {
    fn on_context_lost(&self) {
        let _ = self.0.send_event(ShellEvent::ContextLost);
    }
}

/// The OS clipboard through `arboard`; without one (no display server),
/// copying and pasting do nothing.
struct SystemClipboard(Option<arboard::Clipboard>);

impl Clipboard for SystemClipboard {
    fn read(&mut self) -> Option<String> {
        self.0.as_mut()?.get_text().ok()
    }

    fn write(&mut self, text: &str) {
        if let Some(clipboard) = &mut self.0
            && let Err(err) = clipboard.set_text(text)
        {
            tracing::warn!(%err, "couldn't copy to the clipboard");
        }
    }
}

/// State that exists once the window does.
struct Running {
    window: Arc<Window>,
    browser: BrowserWindow,
    /// Told while the window lives.
    _context_lost: Arc<dyn ContextLostObserver>,
    /// What was last applied to the window.
    cursor: CursorType,
    title: String,
    ime: Option<RectF>,
    /// The click count of the mouse's presses, and where the pointer is
    /// (logical px), which it goes by.
    clicks: ClickCounter,
    pointer: PointF,
}

impl Running {
    /// Brings the window's cursor, title and IME in line with the view.
    fn apply(&mut self) {
        let view = self.browser.view_mut();
        let (cursor, title, ime) = (view.cursor(), view.title(), view.ime_area());
        if cursor != self.cursor {
            // `none` hides the pointer; any other shows it again.
            match cursor_icon(cursor) {
                Some(icon) => {
                    if self.cursor == CursorType::None {
                        self.window.set_cursor_visible(true);
                    }
                    self.window.set_cursor(icon);
                }
                None => self.window.set_cursor_visible(false),
            }
            self.cursor = cursor;
        }
        if title != self.title {
            self.window.set_title(&title);
            self.title = title;
        }
        if ime != self.ime {
            match ime {
                Some(area) => {
                    if self.ime.is_none() {
                        self.window.set_ime_allowed(true);
                    }
                    self.window.set_ime_cursor_area(
                        LogicalPosition::new(area.x, area.y),
                        LogicalSize::new(area.width, area.height),
                    );
                }
                None => self.window.set_ime_allowed(false),
            }
            self.ime = ime;
        }
    }

    fn shutdown(&mut self) {
        self.browser.shutdown();
    }
}

struct App {
    initial_url: String,
    test_fonts: bool,
    num_raster_threads: Option<usize>,
    proxy: EventLoopProxy<ShellEvent>,
    running: Option<Running>,
    error: Option<String>,
    automating: Option<Automating>,
    /// `--remote-debugging-port`'s settings.
    remote_debugging: Option<devtools_http_handler::Options>,
    /// Once the window is up, if something drives it or it serves remote
    /// debugging.
    session: Option<AutomationSession>,
    /// The remote debugging server, while the window is up.
    devtools: Option<DevToolsHttpHandler>,
    /// Answered once the window has the size asked for.
    resized: Option<Reply>,
}

/// winit's icon for `cursor` (they are CSS's keywords), `None` to hide
/// the pointer (`ui::mojom::CursorType` to the platform's cursor, as
/// `ui::Cursor` does).
fn cursor_icon(cursor: CursorType) -> Option<CursorIcon> {
    use CursorType as C;
    Some(match cursor {
        C::Pointer => CursorIcon::Default,
        C::Cross => CursorIcon::Crosshair,
        C::Hand => CursorIcon::Pointer,
        C::IBeam => CursorIcon::Text,
        C::Wait => CursorIcon::Wait,
        C::Help => CursorIcon::Help,
        C::EastResize => CursorIcon::EResize,
        C::NorthResize => CursorIcon::NResize,
        C::NorthEastResize => CursorIcon::NeResize,
        C::NorthWestResize => CursorIcon::NwResize,
        C::SouthResize => CursorIcon::SResize,
        C::SouthEastResize => CursorIcon::SeResize,
        C::SouthWestResize => CursorIcon::SwResize,
        C::WestResize => CursorIcon::WResize,
        C::NorthSouthResize => CursorIcon::NsResize,
        C::EastWestResize => CursorIcon::EwResize,
        C::NorthEastSouthWestResize => CursorIcon::NeswResize,
        C::NorthWestSouthEastResize => CursorIcon::NwseResize,
        C::ColumnResize => CursorIcon::ColResize,
        C::RowResize => CursorIcon::RowResize,
        C::Move => CursorIcon::Move,
        C::VerticalText => CursorIcon::VerticalText,
        C::Cell => CursorIcon::Cell,
        C::ContextMenu => CursorIcon::ContextMenu,
        C::Alias => CursorIcon::Alias,
        C::Progress => CursorIcon::Progress,
        C::NoDrop => CursorIcon::NoDrop,
        C::Copy => CursorIcon::Copy,
        C::None => return None,
        C::NotAllowed => CursorIcon::NotAllowed,
        C::ZoomIn => CursorIcon::ZoomIn,
        C::ZoomOut => CursorIcon::ZoomOut,
        C::Grab => CursorIcon::Grab,
        C::Grabbing => CursorIcon::Grabbing,
    })
}

fn size_of(p: PhysicalSize<u32>) -> Size {
    Size::new(p.width, p.height)
}

impl App {
    fn start(&mut self, event_loop: &ActiveEventLoop) -> Result<Running, String> {
        let mut attrs = Window::default_attributes()
            .with_title("machopo")
            .with_inner_size(LogicalSize::new(1024.0, 768.0));
        // A scenario's caret blinks only if it says so (`window
        // caret-blink=`), as the windowless runner's.
        let mut caret_blink_interval =
            shell::system_colors::caret_blink_interval_for_window(self.test_fonts, false);
        let driver = self.automating.as_ref().and_then(|a| a.pending.as_ref());
        let network = match driver {
            Some(Driver::Pipe(network)) => network.clone(),
            _ => NetworkContextParams::default(),
        };
        let pipe = matches!(driver, Some(Driver::Pipe(_)));
        if driver.is_some() {
            // Nothing covers it: a window hidden has no frames to read back.
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        }
        if let Some(Driver::Scenario(automation)) = driver {
            let window = automation.scenario.window;
            if let Some((width, height)) = window.size {
                attrs = attrs.with_inner_size(LogicalSize::new(width, height));
            }
            if window.scale.is_some() {
                tracing::info!(
                    "automation: the window's scale is the display's; `window scale=` is ignored"
                );
            }
            caret_blink_interval = window.caret_blink;
        }
        let automation = driver.is_some();
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| format!("create_window: {e}"))?,
        );
        let size = size_of(window.inner_size());
        let scale = f32_from_f64(window.scale_factor());

        // The surface is created here (it needs the window) and then moved
        // to the Viz thread, which creates the device and does all drawing.
        // The process's one instance, made with the window's display.
        let display = event_loop.owned_display_handle();
        let instance = viz::gpu_context::instance_with(move || {
            wgpu::InstanceDescriptor::new_with_display_handle(Box::new(display))
        });
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| format!("create_surface: {e}"))?;
        // Another, for Viz on its own thread, made here: a lost device's
        // next one may need one (Chromium's GPU process gets its surfaces
        // from the browser, which owns the window).
        let make_surface = {
            let proxy = self.proxy.clone();
            move || {
                let gone = || "the window has gone".to_owned();
                let (reply, made) = crossbeam_channel::bounded(1);
                proxy
                    .send_event(ShellEvent::MakeSurface(reply))
                    .map_err(|_| gone())?;
                made.recv().map_err(|_| gone())?
            }
        };

        let proxy = self.proxy.clone();
        // Tiles go straight onto the GPU Viz draws with once its device is up
        // (Chromium's `SharedImageInterface` to the GPU process).
        let shared_images = viz::GpuSharedImages::new();
        let gpu: Arc<dyn SharedImageInterface> = shared_images.clone();
        let clipboard = arboard::Clipboard::new()
            .map_err(|err| tracing::warn!(%err, "no clipboard"))
            .ok();
        let mut browser = BrowserWindow::start(
            browser_window::Options {
                test_fonts: self.test_fonts,
                automation,
                network,
                num_raster_threads: self.num_raster_threads,
                caret_blink_interval: if pipe { None } else { caret_blink_interval },
            },
            size,
            scale,
            Some(gpu.clone()),
            move || {
                viz::WgpuRenderer::new(
                    surface,
                    Box::new(make_surface),
                    size,
                    shared_images,
                    automation,
                )
            },
            Box::new(SystemClipboard(clipboard)),
            Box::new(move |tab, msg| {
                let _ = proxy.send_event(ShellEvent::FromContents(tab, Box::new(msg)));
            }),
        )?;
        // A scenario navigates itself; through the pipe the tab starts with
        // a document, as Chrome's does.
        if !automation {
            browser.view_mut().navigate(&self.initial_url);
        } else if pipe {
            automation_pipe::start_blank(&mut browser);
        }
        let context_lost: Arc<dyn ContextLostObserver> =
            Arc::new(ToolbarContextLost(self.proxy.clone()));
        gpu.add_context_lost_observer(Arc::downgrade(&context_lost));

        let mut running = Running {
            window,
            browser,
            _context_lost: context_lost,
            cursor: CursorType::Pointer,
            title: String::new(),
            ime: None,
            clicks: ClickCounter::new(double_click_interval()),
            pointer: PointF::new(0.0, 0.0),
        };
        running.apply();
        Ok(running)
    }

    /// Starts what drives the window, now that it is up: the session, and
    /// the scenario on a thread of its own, or the pipe's reader and
    /// writer.
    fn start_automation(&mut self) {
        let (Some(running), Some(automating)) = (&self.running, &mut self.automating) else {
            return;
        };
        let Some(driver) = automating.pending.take() else {
            return;
        };
        let (outbox, messages) = unbounded::<Message>();
        self.session = Some(AutomationSession::new(&running.browser, outbox).in_platform_window());
        let proxy = self.proxy.clone();
        let send = move |read| proxy.send_event(ShellEvent::Automation(read)).is_ok();
        let done = self.proxy.clone();
        let spawned = match driver {
            Driver::Scenario(Automation { scenario, context }) => std::thread::Builder::new()
                .name("Automation".into())
                .spawn(move || {
                    let client = Client::new(Box::new(move |command| send(Ok(command))), messages);
                    let result = automation::run(&scenario.steps, &context, &client);
                    let _ = done.send_event(ShellEvent::AutomationDone(result));
                })
                .map(drop),
            Driver::Pipe(_) => automation_pipe::spawn_writer(messages)
                .and_then(|writer| {
                    automating.writer = Some(writer);
                    automation_pipe::spawn_reader(send, move || {
                        let _ = done.send_event(ShellEvent::AutomationDone(Ok(())));
                    })
                })
                .map(drop),
        };
        if let Err(e) = spawned {
            self.error = Some(format!("start what drives the window: {e}"));
        }
    }

    /// Starts the remote debugging server, now that the window is up, with
    /// a session of its own if nothing drives the window.
    fn start_remote_debugging(&mut self) -> Result<(), String> {
        let (Some(running), Some(options)) = (&self.running, &self.remote_debugging) else {
            return Ok(());
        };
        if self.session.is_none() {
            self.session = Some(
                AutomationSession::for_remote_debugging(&running.browser).in_platform_window(),
            );
        }
        let proxy = self.proxy.clone();
        let server = DevToolsHttpHandler::start(options, move |event| {
            proxy.send_event(ShellEvent::DevTools(event)).is_ok()
        })
        .map_err(|e| format!("--remote-debugging-port: {e}"))?;
        self.devtools = Some(server);
        Ok(())
    }

    /// Gives the window `input` as winit would: the pointer's moves,
    /// buttons and wheel as its events; keys straight to the view (winit's
    /// key events can't be made outside it). `done` is answered once the
    /// window has it.
    fn inject(&mut self, event_loop: &ActiveEventLoop, input: Input, done: Reply) {
        let Some(running) = &mut self.running else {
            return;
        };
        let scale = running.window.scale_factor();
        let device_id = DeviceId::dummy();
        let pointer = |at: gfx::PointF| WindowEvent::CursorMoved {
            device_id,
            position: LogicalPosition::new(at.x, at.y).to_physical::<f64>(scale),
        };
        let events = match input {
            Input::Move(at) => vec![pointer(at)],
            Input::Click(at) | Input::DoubleClick(at) => {
                // A `click` is the first of its series, however soon after
                // the last; a double click's two come at once.
                running.clicks.reset();
                let clicks = if matches!(input, Input::Click(_)) {
                    1
                } else {
                    2
                };
                let mut events = vec![pointer(at)];
                for _ in 0..clicks {
                    for state in [ElementState::Pressed, ElementState::Released] {
                        events.push(WindowEvent::MouseInput {
                            device_id,
                            state,
                            button: WinitButton::Left,
                        });
                    }
                }
                events
            }
            Input::Wheel { at, delta } => {
                // The other way from `window_event`: winit's deltas move the
                // content.
                let delta = match delta {
                    ScrollDelta::Pixels { x, y } => MouseScrollDelta::PixelDelta(
                        PhysicalPosition::new(-x as f64 * scale, -y as f64 * scale),
                    ),
                    ScrollDelta::Lines { x, y } => MouseScrollDelta::LineDelta(-x, -y),
                };
                vec![
                    pointer(at),
                    WindowEvent::MouseWheel {
                        device_id,
                        delta,
                        phase: TouchPhase::Moved,
                    },
                ]
            }
            Input::Key {
                key,
                modifiers,
                text,
            } => {
                let view = running.browser.view_mut();
                view.set_modifiers(modifiers);
                let press = KeyInput::press(key, text);
                view.key(&press);
                view.key(&press.release());
                view.set_modifiers(ModifiersState::empty());
                running.apply();
                done.done();
                return;
            }
            Input::SetBounds {
                origin,
                width,
                height,
            } => {
                // The view takes the `Moved` that follows as it takes the
                // platform's; it has the position at once meanwhile.
                if let Some(origin) = origin {
                    let position = LogicalPosition::new(origin.x, origin.y);
                    running.window.set_outer_position(position);
                    running.browser.view_mut().move_to(origin);
                }
                let size = LogicalSize::new(width, height);
                let wanted: PhysicalSize<u32> = size.to_physical(scale);
                let now = running.window.inner_size();
                if now == wanted {
                    // Already that size: no `Resized` comes (Windows sends
                    // none for the size the window has).
                    done.done();
                } else if let Some(applied) = running.window.request_inner_size(size) {
                    // Resized at once, and no `Resized` follows: a Wayland
                    // client picks its own size, with no round trip to the
                    // compositor. The window knows, but nothing else does
                    // yet, so tell the browser here, as the event would.
                    // (`applied` is what the window could do: the size it
                    // keeps while it is maximized or tiled, say.) macOS,
                    // Windows and X11 send the request to the system and
                    // answer `None`, and their `Resized` does this.
                    running
                        .browser
                        .resize(size_of(applied), f32_from_f64(scale));
                    running.apply();
                    done.done();
                } else {
                    self.resized = Some(done);
                }
                return;
            }
        };
        if let Some(automating) = &mut self.automating {
            automating.injecting = true;
        }
        for event in events {
            self.window_event(event_loop, WindowId::dummy(), event);
        }
        if let Some(automating) = &mut self.automating {
            automating.injecting = false;
        }
        done.done();
    }

    fn exit(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(mut running) = self.running.take() {
            running.shutdown();
        }
        // The pipe's writer and the server's websockets end with the
        // session, having sent all; the server waits for them as it stops.
        self.session = None;
        self.devtools = None;
        if let Some(automating) = &mut self.automating
            && let Some(writer) = automating.writer.take()
        {
            let _ = writer.join();
        }
        event_loop.exit();
    }
}

impl ApplicationHandler<ShellEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.running.is_some() {
            return;
        }
        match self.start(event_loop) {
            Ok(r) => {
                self.running = Some(r);
                self.start_automation();
                if let Err(e) = self.start_remote_debugging() {
                    self.error = Some(e);
                    self.exit(event_loop);
                }
            }
            Err(e) => {
                self.error = Some(e);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        if let Some(automating) = &mut self.automating
            && !automating.injecting
            && is_input(&event)
        {
            return;
        }
        let scale = running.window.scale_factor();
        if let WindowEvent::Resized(new_size) = event {
            running
                .browser
                .resize(size_of(new_size), f32_from_f64(scale));
            if let Some(done) = self.resized.take() {
                done.done();
            }
        }
        if let WindowEvent::RedrawRequested = event {
            let viz = running.browser.viz();
            let _ = viz.send(VizMsg::Display(DisplayMsg::ForceRedraw));
        }
        let view = running.browser.view_mut();
        match event {
            WindowEvent::CloseRequested => return self.exit(event_loop),
            // Above: they go to Viz too.
            WindowEvent::Resized(_) | WindowEvent::RedrawRequested => {}
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                // A `Resized` with the new physical size follows.
                view.resize(
                    size_of(running.window.inner_size()),
                    f32_from_f64(scale_factor),
                );
            }
            WindowEvent::CursorMoved { position, .. } => {
                let p = position.to_logical::<f32>(scale);
                running.pointer = PointF::new(p.x, p.y);
                view.pointer_moved(running.pointer);
            }
            WindowEvent::Moved(position) => {
                let p = position.to_logical::<f32>(scale);
                view.move_to(PointF::new(p.x, p.y));
            }
            WindowEvent::ModifiersChanged(modifiers) => view.set_modifiers(modifiers.state()),
            WindowEvent::Focused(focused) => view.set_window_focused(focused),
            WindowEvent::MouseInput { state, button, .. } => {
                let pressed = state == ElementState::Pressed;
                let page_button = match button {
                    WinitButton::Left => Some(MouseButton::Left),
                    WinitButton::Middle => Some(MouseButton::Middle),
                    WinitButton::Right => Some(MouseButton::Right),
                    _ => None,
                };
                if let Some(button) = page_button {
                    // Chromium's platform layer counts the clicks too.
                    let count = if pressed {
                        running
                            .clicks
                            .press(Instant::now(), running.pointer, button)
                    } else {
                        0
                    };
                    view.mouse_input_with_count(button, pressed, count);
                }
                match button {
                    // The mouse's own back and forward buttons go through
                    // history, as in Chrome.
                    WinitButton::Back if pressed => view.run(Command::Back),
                    WinitButton::Forward if pressed => view.run(Command::Forward),
                    _ => {}
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // winit's deltas move the content (positive: right and
                // down); ours move the scroll offset, the other way.
                view.wheel(match delta {
                    MouseScrollDelta::LineDelta(x, y) => ScrollDelta::Lines { x: -x, y: -y },
                    MouseScrollDelta::PixelDelta(p) => ScrollDelta::Pixels {
                        x: f32_from_f64(-p.x / scale),
                        y: f32_from_f64(-p.y / scale),
                    },
                });
            }
            WindowEvent::KeyboardInput { event, .. } => {
                view.key(&KeyInput {
                    logical: event.logical_key.clone(),
                    physical: event.physical_key,
                    location: event.location,
                    text: event.text.as_deref().map(str::to_owned),
                    pressed: event.state == ElementState::Pressed,
                    repeat: event.repeat,
                });
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => view.ime_preedit(&text, cursor),
            WindowEvent::Ime(Ime::Commit(text)) => view.ime_commit(&text),
            _ => {}
        }
        running.apply();
        if running.browser.view().close_requested() {
            self.exit(event_loop);
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ShellEvent) {
        let Some(running) = &mut self.running else {
            return;
        };
        let (tab, msg) = match event {
            ShellEvent::FromContents(tab, event) => {
                let event = *event;
                // A navigation's, to DevTools before the tab goes on with it.
                if !matches!(event, ContentsEvent::Renderer(_))
                    && let Some(session) = self.session.as_mut()
                {
                    session.on_contents_event(tab, &event, &running.browser);
                }
                let Some(msg) = running.browser.view_mut().on_contents_event(tab, event) else {
                    return;
                };
                (tab, msg)
            }
            ShellEvent::ContextLost => {
                running.browser.view_mut().did_lose_context();
                return;
            }
            ShellEvent::MakeSurface(reply) => {
                let surface = viz::gpu_context::instance()
                    .create_surface(running.window.clone())
                    .map_err(|e| format!("create_surface: {e}"));
                let _ = reply.send(surface);
                return;
            }
            ShellEvent::Automation(read) => {
                let Some(session) = self.session.as_mut() else {
                    return;
                };
                match read {
                    Ok(command) => {
                        let input = session.command(command, &mut running.browser);
                        let close = session.close_requested();
                        if let Some((input, done)) = input {
                            self.inject(event_loop, input, done);
                        }
                        if close {
                            if let Some(automating) = &mut self.automating {
                                automating.result = Some(Ok(()));
                            }
                            return self.exit(event_loop);
                        }
                    }
                    Err((id, error)) => session.refuse(id, error),
                }
                return;
            }
            ShellEvent::AutomationDone(result) => {
                if let Some(automating) = &mut self.automating {
                    automating.result = Some(result);
                }
                return self.exit(event_loop);
            }
            ShellEvent::DevTools(event) => {
                let Some(session) = self.session.as_mut() else {
                    return;
                };
                let input = session.client_event(event, &mut running.browser);
                let close = session.close_requested();
                if let Some((input, done)) = input {
                    self.inject(event_loop, input, done);
                }
                let Some(running) = &mut self.running else {
                    return;
                };
                if close || running.browser.view().close_requested() {
                    if let Some(automating) = &mut self.automating {
                        automating.result = Some(Ok(()));
                    }
                    return self.exit(event_loop);
                }
                running.apply();
                return;
            }
        };
        if let Some(session) = self.session.as_mut() {
            session.on_renderer_message(tab, &msg, &running.browser);
        }
        if running.browser.view().close_requested() {
            return self.exit(event_loop);
        }
        running.apply();
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(mut running) = self.running.take() {
            running.shutdown();
        }
    }
}

/// Opens a window at `url`, until it is closed; or, with `driver`, lets it
/// drive the window until it is done (the scenario has run, failing if it
/// does; the pipe's input has ended, or `Browser.close`).
pub fn run(
    url: String,
    test_fonts: bool,
    num_raster_threads: Option<usize>,
    driver: Option<Driver>,
    remote_debugging: Option<devtools_http_handler::Options>,
) -> Result<(), String> {
    let event_loop = EventLoop::<ShellEvent>::with_user_event()
        .build()
        .map_err(|e| format!("event loop: {e}"))?;
    let mut app = App {
        initial_url: url,
        test_fonts,
        num_raster_threads,
        proxy: event_loop.create_proxy(),
        running: None,
        error: None,
        automating: driver.map(|driver| Automating {
            pending: Some(driver),
            writer: None,
            injecting: false,
            result: None,
        }),
        remote_debugging,
        session: None,
        devtools: None,
        resized: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("event loop: {e}"))?;
    if let Some(e) = app.error {
        return Err(e);
    }
    match app.automating.map(|a| a.result) {
        None | Some(Some(Ok(()))) => Ok(()),
        Some(Some(Err(failure))) => Err(failure.to_string()),
        Some(None) => Err("the window closed before what drove it was done".into()),
    }
}
