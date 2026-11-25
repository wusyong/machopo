//! Windowed mode: winit on the UI thread, Viz presenting through wgpu on its
//! own thread. This file only translates winit's events for the
//! [`BrowserView`](shell::browser_view::BrowserView) and applies what it
//! asks of the window (cursor, title, IME); the rest of the browser is a
//! [`BrowserWindow`].
//!
//! With `--automation-pipe --window`, what reads and writes machopo's
//! standard input and output drives the window (`shell::automation_pipe`:
//! machopodriver, or the scenario tests' runner), through the window's
//! `AutomationSession`, whose input goes in as winit's events would. The
//! platform's own input is ignored meanwhile, as `interactive_ui_tests` own
//! the window they drive. With
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
use protocol::viz::{ContextLostObserver, ContextProvider, DisplayMsg, VizMsg};
use shell::automation_pipe::{self, Read};
use shell::automation_session::{AutomationSession, ClientEvent, Input, Reply};
use shell::browser_view::Command;
use shell::browser_window::{self, BrowserWindow};
use shell::chrome::UiLayout;
use shell::devtools_http_handler::{self, DevToolsHttpHandler};
use shell::keyboard::KeyInput;
use shell::mouse::{ClickCounter, double_click_interval};
use shell::omnibox::Clipboard;
use shell::theme::ColorMode;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, Ime, MouseButton as WinitButton, MouseScrollDelta, TouchPhase,
    WindowEvent,
};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{CursorIcon, Theme, Window, WindowId, WindowLevel};

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
    /// From the pipe's reader.
    Automation(Read),
    /// The pipe's input has ended.
    AutomationDone,
    /// From the remote debugging server's thread.
    DevTools(ClientEvent),
    /// SIGTERM, SIGINT or SIGHUP: machopo ends as when the window closes.
    Signal,
    /// Viz has no GPU device that draws: machopo ends as when the window
    /// closes, with an error.
    GpuFailed(String),
}

/// The window's half of a run that drives it (`machopo --automation-pipe
/// --window`).
struct Automating {
    /// The network settings given, until the window is up and the pipe
    /// starts.
    pending: Option<NetworkContextParams>,
    /// The pipe's writer, to wait for once the session has gone.
    writer: Option<std::thread::JoinHandle<()>>,
    /// Set while the window gives itself the driver's input, which goes
    /// through; the platform's doesn't.
    injecting: bool,
    /// The pipe's input has ended, or `Browser.close` was answered.
    done: bool,
}

/// The platform's input, which the window ignores while the pipe drives
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
    /// `--scale`: the device scale factor in place of the screen's.
    forced_scale: Option<f64>,
}

impl Running {
    /// The device scale factor: `--scale`'s, or the screen's.
    fn scale(&self) -> f64 {
        self.forced_scale
            .unwrap_or_else(|| self.window.scale_factor())
    }

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
                    let scale = self.scale();
                    self.window.set_ime_cursor_area(
                        LogicalPosition::new(area.x, area.y).to_physical::<f64>(scale),
                        LogicalSize::new(area.width, area.height).to_physical::<f64>(scale),
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
    /// How the UI is laid out at first (`--ui-layout`).
    ui_layout: UiLayout,
    /// Light or dark (`--ui-theme`); `None`: as the system is, and as it
    /// changes.
    ui_theme: Option<ColorMode>,
    test_fonts: bool,
    num_raster_threads: Option<usize>,
    process_per_tab: bool,
    /// The window's profile (`--user-data-dir`, or the user's own).
    profile: browser::Profile,
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
    /// `--scale`: the device scale factor in place of the screen's.
    forced_scale: Option<f64>,
    /// The size what drives the window last asked for, and when: kept
    /// against a compositor's overriding it (`ASKED_SIZE_KEPT`).
    asked_size: Option<(PhysicalSize<u32>, std::time::Instant)>,
}

/// How long after what drives the window asks for a size another size
/// from the system is taken for its overriding it: a Wayland compositor
/// configures a new window to the size it was made with, after a size asked
/// for before that (a scenario's `window size=`), and the window asks
/// again, once. The system's own resizes come later than this.
const ASKED_SIZE_KEPT: std::time::Duration = std::time::Duration::from_secs(1);

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
        // In the screen's logical px, with `--scale` too: a window as big
        // in the forced scale's px may not fit the screen (at 2 on a
        // laptop's), and the compositor would maximize it, keeping it so
        // whatever size it is asked for after.
        let mut attrs = Window::default_attributes()
            .with_title("machopo")
            .with_inner_size(LogicalSize::new(1024.0, 768.0));
        let pipe = self.automating.as_ref().and_then(|a| a.pending.as_ref());
        let network = pipe.cloned().unwrap_or_default();
        let automation = pipe.is_some();
        if automation {
            // Nothing covers it: a window hidden has no frames to read back.
            attrs = attrs.with_window_level(WindowLevel::AlwaysOnTop);
        } else if let Some(mode) = self.ui_theme {
            // The title bar too, where the system draws it.
            attrs = attrs.with_theme(Some(match mode {
                ColorMode::Light => Theme::Light,
                ColorMode::Dark => Theme::Dark,
            }));
        }
        // Driven through the pipe, the caret doesn't blink, so each frame
        // comes out the same; a person's blinks as the system's does.
        let caret_blink_interval = if automation {
            None
        } else {
            shell::system_colors::caret_blink_interval_for_window(self.test_fonts, false)
        };
        let window = Arc::new(
            event_loop
                .create_window(attrs)
                .map_err(|e| format!("create_window: {e}"))?,
        );
        let size = size_of(window.inner_size());
        let scale = f32_from_f64(self.forced_scale.unwrap_or_else(|| window.scale_factor()));

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
        // (Chromium's `ContextProvider` to the GPU process).
        let shared_images = viz::GpuSharedImages::new();
        let failed = self.proxy.clone();
        let gpu: Arc<dyn ContextProvider> = shared_images.clone();
        let clipboard = arboard::Clipboard::new()
            .map_err(|err| tracing::warn!(%err, "no clipboard"))
            .ok();
        let mut browser = BrowserWindow::start(
            browser_window::Options {
                test_fonts: self.test_fonts,
                automation,
                network,
                num_raster_threads: self.num_raster_threads,
                process_per_tab: self.process_per_tab,
                caret_blink_interval,
                profile: self.profile.clone(),
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
                .map(|renderer| {
                    renderer.on_failure(move |error| {
                        let _ = failed.send_event(ShellEvent::GpuFailed(error));
                    })
                })
            },
            Box::new(SystemClipboard(clipboard)),
            Box::new(move |tab, msg| {
                let _ = proxy.send_event(ShellEvent::FromContents(tab, Box::new(msg)));
            }),
        )?;
        // Through the pipe the tab starts with a document, as Chrome's
        // does, and the pipe navigates.
        if automation {
            automation_pipe::start_blank(&mut browser);
        } else {
            browser.view_mut().set_ui_layout(self.ui_layout);
            // Driven through the pipe the UI stays light, whatever the
            // system: the screenshots don't change with the machine.
            browser
                .view_mut()
                .set_color_mode(window.theme().map_or(ColorMode::Light, color_mode));
            browser.view_mut().navigate(&self.initial_url);
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
            forced_scale: self.forced_scale,
        };
        running.apply();
        Ok(running)
    }

    /// Starts what drives the window, now that it is up: the session, and
    /// the pipe's reader and writer.
    fn start_automation(&mut self) {
        let (Some(running), Some(automating)) = (&self.running, &mut self.automating) else {
            return;
        };
        if automating.pending.take().is_none() {
            return;
        }
        let (outbox, messages) = unbounded::<Message>();
        self.session = Some(AutomationSession::new(&running.browser, outbox).in_platform_window());
        let proxy = self.proxy.clone();
        let send = move |read| proxy.send_event(ShellEvent::Automation(read)).is_ok();
        let done = self.proxy.clone();
        let spawned = automation_pipe::spawn_writer(messages)
            .and_then(|writer| {
                automating.writer = Some(writer);
                automation_pipe::spawn_reader(send, move || {
                    let _ = done.send_event(ShellEvent::AutomationDone);
                })
            })
            .map(drop);
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
        let scale = running.scale();
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
                    running
                        .window
                        .set_outer_position(position.to_physical::<i32>(scale));
                    running.browser.view_mut().move_to(origin);
                }
                let size = LogicalSize::new(width, height);
                let wanted: PhysicalSize<u32> = size.to_physical(scale);
                let now = running.window.inner_size();
                self.asked_size = Some((wanted, std::time::Instant::now()));
                if now == wanted {
                    // Already that size: no `Resized` comes (Windows sends
                    // none for the size the window has).
                    done.done();
                } else if let Some(applied) = running.window.request_inner_size(wanted) {
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
        let ending = std::time::Instant::now();
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
            automation_pipe::wait_for_writer(writer, ending);
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
        let scale = running.scale();
        if let WindowEvent::Resized(mut new_size) = event {
            // Overridden as soon as asked for: asked for again (and on
            // Wayland, which answers at once, taken at once).
            if let Some((asked, at)) = self.asked_size.take()
                && new_size != asked
                && at.elapsed() < ASKED_SIZE_KEPT
                && let Some(applied) = running.window.request_inner_size(asked)
            {
                new_size = applied;
            }
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
            // A `Resized` with the new physical size follows. A forced
            // scale stays.
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                view.resize(
                    size_of(running.window.inner_size()),
                    f32_from_f64(running.forced_scale.unwrap_or(scale_factor)),
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
            WindowEvent::CursorLeft { .. } => view.pointer_left_window(),
            // The system's changed: the UI follows, unless `--ui-theme`
            // chose, or the pipe drives it.
            WindowEvent::ThemeChanged(theme)
                if self.automating.is_none() && self.ui_theme.is_none() =>
            {
                view.set_color_mode(color_mode(theme));
            }
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
        // Asked to end: what drives the window is done with it too.
        if let ShellEvent::Signal = event {
            if let Some(automating) = &mut self.automating {
                automating.done = true;
            }
            return self.exit(event_loop);
        }
        if let ShellEvent::GpuFailed(error) = event {
            self.error = Some(automation_pipe::gpu_failed(&error));
            return self.exit(event_loop);
        }
        let Some(running) = &mut self.running else {
            return;
        };
        let (tab, msg) = match event {
            ShellEvent::FromContents(tab, event) => {
                let event = *event;
                // A navigation's, to DevTools before the tab goes on with it.
                if !matches!(event, ContentsEvent::Renderer { .. })
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
                                automating.done = true;
                            }
                            return self.exit(event_loop);
                        }
                    }
                    Err((id, error)) => session.refuse(id, error),
                }
                return;
            }
            ShellEvent::AutomationDone => {
                if let Some(automating) = &mut self.automating {
                    automating.done = true;
                }
                return self.exit(event_loop);
            }
            ShellEvent::Signal | ShellEvent::GpuFailed(_) => return,
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
                        automating.done = true;
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

/// What `run` opens a window with.
pub struct Options {
    pub url: String,
    /// `--ui-layout`.
    pub ui_layout: UiLayout,
    /// `--ui-theme`; `None`: the system's.
    pub ui_theme: Option<ColorMode>,
    pub test_fonts: bool,
    pub num_raster_threads: Option<usize>,
    /// Each tab's renderer is a process of its own (`--process-per-tab`).
    pub process_per_tab: bool,
    /// The network settings `--automation-pipe --window` gives: the pipe
    /// drives the window.
    pub pipe: Option<NetworkContextParams>,
    pub remote_debugging: Option<devtools_http_handler::Options>,
    pub profile: browser::Profile,
    /// `--scale`: the device scale factor in place of the screen's (as
    /// Chromium's `--force-device-scale-factor`), the window that many
    /// physical px a CSS px. The real window's scenarios run with 1, as
    /// the windowless ones: on a screen scaled by a fraction, their points
    /// and sizes would be rounded to physical px and come back off.
    pub scale: Option<f32>,
}

/// Opens a window at `options.url`, until it is closed; or, with
/// `options.pipe`, lets the pipe drive the window until its input ends, or
/// `Browser.close`.
pub fn run(options: Options) -> Result<(), String> {
    let Options {
        url,
        ui_layout,
        ui_theme,
        test_fonts,
        num_raster_threads,
        process_per_tab,
        pipe,
        remote_debugging,
        profile,
        scale,
    } = options;
    let event_loop = EventLoop::<ShellEvent>::with_user_event()
        .build()
        .map_err(|e| format!("event loop: {e}"))?;
    let signalled = event_loop.create_proxy();
    shell::shutdown_signals::listen(move || {
        let _ = signalled.send_event(ShellEvent::Signal);
    });
    let mut app = App {
        initial_url: url,
        ui_layout,
        ui_theme,
        test_fonts,
        num_raster_threads,
        process_per_tab,
        profile,
        proxy: event_loop.create_proxy(),
        running: None,
        error: None,
        automating: pipe.map(|network| Automating {
            pending: Some(network),
            writer: None,
            injecting: false,
            done: false,
        }),
        remote_debugging,
        session: None,
        devtools: None,
        resized: None,
        forced_scale: scale.map(f64::from),
        asked_size: None,
    };
    event_loop
        .run_app(&mut app)
        .map_err(|e| format!("event loop: {e}"))?;
    if let Some(e) = app.error {
        return Err(e);
    }
    match app.automating {
        Some(Automating { done: false, .. }) => {
            Err("the window closed before what drove it was done".into())
        }
        _ => Ok(()),
    }
}

fn color_mode(theme: Theme) -> ColorMode {
    match theme {
        Theme::Light => ColorMode::Light,
        Theme::Dark => ColorMode::Dark,
    }
}
