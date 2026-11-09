//! A browser window without the platform's window, for the scenario tests
//! and `machopo --automation-pipe`: a [`BrowserWindow`] whose Viz thread draws
//! into a texture of its own on the process's GPU device ([`start`]), with
//! input given straight to its `BrowserView` (what
//! `window` does with winit's events), driven by `protocol::automation`'s
//! commands ([`drive`]). Everything else is as in the window: the
//! renderer's flags, Viz's own `BeginFrame`s every 16.7 ms, the compositor
//! drawing without waiting for the main thread. So time is real, and checks
//! wait for what they expect.

use std::path::Path;
use std::sync::Arc;

use base::numerics::clamp_round;
use browser::{ContentsEvent, ContentsEventCallback};
use crossbeam_channel::{Receiver, Sender, select, unbounded};
use gfx::Size;
use protocol::TabId;
use protocol::automation::Message;
use protocol::input::MouseButton;
use protocol::viz::{ContextLostObserver, SharedImageInterface};
use winit::keyboard::ModifiersState;

use crate::automation::{self, Client, Context, Failure};
use crate::automation_pipe::Read;
use crate::automation_session::{AutomationSession, ClientEvent, Input};
use crate::browser_window::{self, BrowserWindow};
use crate::keyboard::KeyInput;
use crate::omnibox::Clipboard;
use crate::scenario::{self, Scenario};

/// The window a scenario runs in, unless its `window` lines say otherwise.
#[derive(Debug, Clone)]
pub struct Options {
    /// CSS px.
    pub width: f32,
    pub height: f32,
    pub scale: f32,
}

impl Default for Options {
    /// 800×600 at 1x, as WPT's window.
    fn default() -> Self {
        Self {
            width: 800.0,
            height: 600.0,
            scale: 1.0,
        }
    }
}

/// Runs the scenario in the file at `path` in a new window, with macOS's
/// keyboard shortcuts if `mac`, whatever the system; screenshots go to
/// `out`, if given.
pub fn run_file(
    path: &Path,
    out: Option<&Path>,
    mac: bool,
    options: &Options,
) -> Result<(), Failure> {
    let failure = |message: String| Failure { line: 0, message };
    let text =
        std::fs::read_to_string(path).map_err(|e| failure(format!("{}: {e}", path.display())))?;
    let scenario = scenario::parse(&text).map_err(|e| Failure {
        line: e.line,
        message: e.message,
    })?;
    let context = Context {
        base: path.parent().unwrap_or(Path::new(".")).to_owned(),
        out: out.map(Path::to_owned),
        mac,
    };
    run(&scenario, &context, options)
}

/// Hears that the GPU device was lost and Viz has a new one: the toolbar's
/// tiles go on it again (the window's `ContextLost` event).
pub struct ContextLost {
    pub receiver: Receiver<()>,
    _observer: Arc<dyn ContextLostObserver>,
}

struct SendContextLost(Sender<()>);

impl ContextLostObserver for SendContextLost {
    fn on_context_lost(&self) {
        let _ = self.0.send(());
    }
}

/// A browser window without the platform's window, `size` physical px:
/// Viz draws into a texture of its own, which frames are read back from,
/// on the process's GPU device, and the tiles go straight onto it as in the
/// window (Chromium's shared images).
pub fn start(
    options: browser_window::Options,
    size: Size,
    scale: f32,
    clipboard: Box<dyn Clipboard>,
    on_message: ContentsEventCallback,
) -> Result<(BrowserWindow, ContextLost), String> {
    let shared_images = viz::GpuSharedImages::new();
    let gpu: Arc<dyn SharedImageInterface> = shared_images.clone();
    let browser = BrowserWindow::start(
        options,
        size,
        scale,
        Some(gpu.clone()),
        move || viz::WgpuRenderer::offscreen(size, shared_images),
        clipboard,
        on_message,
    )?;
    let (lost, receiver) = unbounded();
    let observer: Arc<dyn ContextLostObserver> = Arc::new(SendContextLost(lost));
    gpu.add_context_lost_observer(Arc::downgrade(&observer));
    Ok((
        browser,
        ContextLost {
            receiver,
            _observer: observer,
        },
    ))
}

/// The browser window a scenario's `window` settings ask for: the test
/// fonts, automated.
fn browser_options(window: &scenario::WindowSettings) -> browser_window::Options {
    browser_window::Options {
        test_fonts: true,
        automation: true,
        caret_blink_interval: window.caret_blink,
        ..Default::default()
    }
}

/// Runs `scenario` in a new window, which is closed after.
pub fn run(scenario: &Scenario, context: &Context, options: &Options) -> Result<(), Failure> {
    let failure = |message: String| Failure { line: 0, message };
    let window = scenario.window;
    let options = Options {
        width: window.size.map_or(options.width, |(width, _)| width),
        height: window.size.map_or(options.height, |(_, height)| height),
        scale: window.scale.unwrap_or(options.scale),
    };
    let size = physical(options.width, options.height, options.scale);
    let (host_tx, host_rx) = unbounded();
    let (mut browser, context_lost) = start(
        browser_options(&window),
        size,
        options.scale,
        Box::new(MemoryClipboard::default()),
        Box::new(move |tab, msg| {
            let _ = host_tx.send((tab, msg));
        }),
    )
    .map_err(failure)?;
    browser.view_mut().set_mac(context.mac);

    // The scenario's runner is the session's client, in this process.
    let (outbox, messages) = unbounded::<Message>();
    let mut session = AutomationSession::new(&browser, outbox);
    let (command_tx, commands) = unbounded::<Read>();
    let (done_tx, done) = unbounded();
    let ended = std::thread::scope(|scope| {
        scope.spawn(move || {
            let client = Client::new(
                Box::new(move |command| command_tx.send(Ok(command)).is_ok()),
                messages,
            );
            let _ = done_tx.send(automation::run(&scenario.steps, context, &client));
        });
        let ended = drive(
            &mut browser,
            &mut session,
            &host_rx,
            &context_lost,
            &commands,
            &crossbeam_channel::never(),
            &done,
            options.scale,
        );
        // The runner's waits end with the session's answers.
        drop(session);
        ended
    });
    browser.shutdown();
    match ended {
        Ended::Done(Some(result)) => result,
        Ended::Done(None) => Err(failure("the scenario panicked".into())),
        Ended::Closed => Err(failure(
            "the window closed before the scenario ended".into(),
        )),
        Ended::RendererGone => Err(failure("the renderer has gone".into())),
    }
}

/// Why [`drive`] stopped.
pub enum Ended<T> {
    /// What `done` brought; `None` if a channel closed (the pipe's input
    /// ended; the scenario's thread panicked).
    Done(Option<T>),
    /// The window closed (its last tab), or `Browser.close`.
    Closed,
    /// The renderers' messages stopped.
    RendererGone,
}

/// Drives `browser` with the driver's commands `commands` brings, and what
/// remote debugging's clients ask (`clients`), through `session`, until
/// `done` brings something, the commands stop, or the window closes: the
/// renderers' messages go to the view and then to the session, and the
/// input the session hands back is given to the view as the platform's
/// would be, at `scale`.
#[allow(clippy::too_many_arguments)]
pub fn drive<T>(
    browser: &mut BrowserWindow,
    session: &mut AutomationSession,
    host_rx: &Receiver<(TabId, ContentsEvent)>,
    context_lost: &ContextLost,
    commands: &Receiver<Read>,
    clients: &Receiver<ClientEvent>,
    done: &Receiver<T>,
    scale: f32,
) -> Ended<T> {
    loop {
        select! {
            recv(host_rx) -> msg => {
                let Ok((tab, event)) = msg else {
                    return Ended::RendererGone;
                };
                // A navigation's, to DevTools before the tab goes on with it.
                if !matches!(event, ContentsEvent::Renderer(_)) {
                    session.on_contents_event(tab, &event, browser);
                }
                let Some(msg) = browser.view_mut().on_contents_event(tab, event) else {
                    continue;
                };
                session.on_renderer_message(tab, &msg, browser);
                if browser.view().close_requested() {
                    return Ended::Closed;
                }
            }
            recv(context_lost.receiver) -> _ => {
                browser.view_mut().did_lose_context();
            }
            recv(commands) -> read => match read {
                Ok(Ok(command)) => {
                    if let Some((input, reply)) = session.command(command, browser) {
                        give(browser, input, scale);
                        reply.done();
                    }
                    if session.close_requested() || browser.view().close_requested() {
                        return Ended::Closed;
                    }
                }
                Ok(Err((id, error))) => session.refuse(id, error),
                // A runner sends what it came to before its commands'
                // channel closes, but `select!` may see the close first.
                Err(_) => return Ended::Done(done.try_recv().ok()),
            },
            recv(clients) -> event => {
                // Whoever posts them keeps a sender while the loop runs.
                let Ok(event) = event else { continue };
                if let Some((input, reply)) = session.client_event(event, browser) {
                    give(browser, input, scale);
                    reply.done();
                }
                if session.close_requested() || browser.view().close_requested() {
                    return Ended::Closed;
                }
            }
            recv(done) -> result => {
                return Ended::Done(result.ok());
            }
        }
    }
}

pub(crate) fn physical(width: f32, height: f32, scale: f32) -> Size {
    Size::new(clamp_round(width * scale), clamp_round(height * scale))
}

/// `input` as `window` gives winit's events to the view.
fn give(browser: &mut BrowserWindow, input: Input, scale: f32) {
    match input {
        Input::Move(at) => browser.view_mut().pointer_moved(at),
        Input::Click(at) | Input::DoubleClick(at) => {
            let clicks = if matches!(input, Input::Click(_)) {
                1
            } else {
                2
            };
            let view = browser.view_mut();
            view.pointer_moved(at);
            for count in 1..=clicks {
                view.mouse_input_with_count(MouseButton::Left, true, count);
                view.mouse_input_with_count(MouseButton::Left, false, count);
            }
        }
        Input::Wheel { at, delta } => {
            let view = browser.view_mut();
            view.pointer_moved(at);
            view.wheel(delta);
        }
        Input::Key {
            key,
            modifiers,
            text,
        } => {
            let view = browser.view_mut();
            view.set_modifiers(modifiers);
            let press = KeyInput::press(key, text);
            view.key(&press);
            view.key(&press.release());
            view.set_modifiers(ModifiersState::empty());
        }
        Input::SetBounds {
            origin,
            width,
            height,
        } => {
            // No platform window to move: where it is is only kept.
            if let Some(origin) = origin {
                browser.view_mut().move_to(origin);
            }
            browser.resize(physical(width, height, scale), scale);
        }
    }
}

/// A clipboard of the window's own, so tests don't touch the system's.
#[derive(Default)]
pub(crate) struct MemoryClipboard(Option<String>);

impl Clipboard for MemoryClipboard {
    fn read(&mut self) -> Option<String> {
        self.0.clone()
    }

    fn write(&mut self, text: &str) {
        self.0 = Some(text.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// A scenario's window is on the DOM the options name, with its
    /// script language and caret.
    #[test]
    fn the_browser_window_a_scenario_asks_for() {
        let window = scenario::WindowSettings {
            caret_blink: Some(Duration::from_millis(500)),
            ..Default::default()
        };
        let browser = browser_options(&window);
        assert!(browser.test_fonts && browser.automation);
        assert_eq!(
            browser.caret_blink_interval,
            Some(Duration::from_millis(500))
        );
    }
}
