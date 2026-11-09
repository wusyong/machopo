//! Runs a [`scenario`](crate::scenario) against a browser window: for the
//! scenario tests (a window without the platform's, `windowless`) and
//! `machopo --automation` (the real one), as Chromium's `interactive_ui_tests`
//! drive a browser.
//!
//! The runner is a client of `protocol::automation`, as machopodriver is:
//! it steps through the scenario on a thread of its own and sends
//! the `Machopo` domain's commands (the window and its tab shown, as a person
//! uses them), waiting, polling, for what it checks. The window's
//! `AutomationSession` answers them on its UI thread, in the same process:
//! the commands and answers go through channels, not serialized; input it
//! hands to the embedder, which gives it to the window as the platform
//! would.
//!
//! The renderer answers `Machopo.queryElement` on its main thread, so while
//! script keeps that busy a query waits; frames come from Viz, which
//! answers meanwhile.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use base::numerics::{clamp_ceil, clamp_floor, clamp_round};

use base64::Engine;
use crossbeam_channel::{Receiver, RecvTimeoutError};
use gfx::{PointF, RectF, Size};
use protocol::automation::{
    self, Bounds, CaptureResult, CaptureWindow, Command as AutomationCommand, Error, Message,
    Method, Navigate, QueryElement, QueryResult, SetWindowBounds, WindowInput,
};
use protocol::viz::CopyOutputResult;
use serde_json::Value;
use winit::keyboard::ModifiersState;

use crate::scenario::{Command, Platform, Step, Target};

/// How long a wait may take, until a scenario's `timeout` says otherwise.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10);

/// How often a wait asks again.
const POLL: Duration = Duration::from_millis(16);

/// How long a query may go unanswered before `wait-busy` takes the
/// renderer's main thread to be busy: far longer than it takes between
/// tasks (a frame is 16.7 ms).
const BUSY: Duration = Duration::from_millis(300);

/// machopo's one window (`Browser.getWindowForTarget`).
const WINDOW_ID: u64 = 1;

/// Why a frame asked for `now` didn't come.
const NO_FRAME: &str = "no frame was drawn: is the window hidden?";

/// An element, where the window shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Element {
    /// CSS px of the window; `None` if it has no box.
    pub rect: Option<RectF>,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QueryReply {
    pub element: Option<Element>,
    /// Where the page that answered is, CSS px of the window.
    pub page: Option<RectF>,
    /// The window's device pixels per CSS px, for frames' pixels.
    pub scale: f32,
}

fn rect(rect: automation::Rect) -> RectF {
    RectF::new(rect.x, rect.y, rect.width, rect.height)
}

/// The runner's end of the window's automation: sends commands (`false`
/// once the window is gone) and reads what the window sends back.
pub struct Client {
    send: Box<dyn Fn(AutomationCommand) -> bool + Send>,
    messages: Receiver<Message>,
    next_id: Cell<i32>,
}

/// Why a command got no result.
enum CallError {
    TimedOut,
    Gone,
    Failed(Error),
}

impl Client {
    pub fn new(
        send: Box<dyn Fn(AutomationCommand) -> bool + Send>,
        messages: Receiver<Message>,
    ) -> Self {
        Self {
            send,
            messages,
            next_id: Cell::new(0),
        }
    }

    /// Sends `method` and waits up to `timeout` for its answer. Answers to
    /// commands given up on, and events, are passed over.
    fn call(&self, method: Method, timeout: Duration) -> Result<Value, CallError> {
        let id = self
            .next_id
            .get()
            .checked_add(1)
            .expect("fewer than 2^31 commands: CDP's ids are ints");
        self.next_id.set(id);
        let command = AutomationCommand {
            id,
            session_id: None,
            method,
        };
        if !(self.send)(command) {
            return Err(CallError::Gone);
        }
        let deadline = deadline(timeout);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.messages.recv_timeout(left) {
                Ok(Message::Response(response)) if response.id == id => {
                    return response.result.map_err(CallError::Failed);
                }
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => return Err(CallError::TimedOut),
                Err(RecvTimeoutError::Disconnected) => return Err(CallError::Gone),
            }
        }
    }
}

/// Why a scenario failed: at which line, and what was seen.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    /// 0 when it isn't one step's.
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            0 => f.write_str(&self.message),
            line => write!(f, "line {line}: {}", self.message),
        }
    }
}

/// What a scenario runs with besides its window.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// What `navigate` resolves relative paths against: the scenario's
    /// directory.
    pub base: PathBuf,
    /// Where `screenshot` saves; `None` saves nothing.
    pub out: Option<PathBuf>,
    /// The window uses macOS's keyboard shortcuts: `primary` is Cmd, and
    /// `platform mac` blocks run (`platform other` ones otherwise).
    pub mac: bool,
}

/// The scenario's half: runs `steps`, through `client`. Blocks until done.
pub fn run(steps: &[Step], context: &Context, client: &Client) -> Result<(), Failure> {
    let mut runner = Runner {
        client,
        context,
        timeout: DEFAULT_TIMEOUT,
        captures: HashMap::new(),
        before_input: None,
        frames: Vec::new(),
    };
    // The steps that run here, with this platform's shortcuts.
    let mut platform = Platform::All;
    let runs: Vec<&Step> = steps
        .iter()
        .filter(|step| {
            if let Command::Platform(next) = step.command {
                platform = next;
                return false;
            }
            match platform {
                Platform::All => true,
                Platform::Mac => context.mac,
                Platform::Other => !context.mac,
            }
        })
        .collect();
    let nexts = runs.iter().skip(1).map(Some).chain([None]);
    for (step, next) in runs.iter().zip(nexts) {
        let failure = |message| Failure {
            line: step.line,
            message,
        };
        // A `wait-change` right after input waits for the element to differ
        // from before the input: once the input is handled, what it changed
        // has changed already.
        if gives_input(&step.command)
            && let Some(Command::WaitChange(selector)) = next.map(|next| &next.command)
        {
            runner.before_input = Some(runner.query(selector).map_err(failure)?.element);
        }
        runner.step(&step.command).map_err(failure)?;
    }
    Ok(())
}

/// Whether `command` gives the window input.
fn gives_input(command: &Command) -> bool {
    matches!(
        command,
        Command::Hover(_)
            | Command::Click(_)
            | Command::DoubleClick(_)
            | Command::Wheel { .. }
            | Command::Key { .. }
            | Command::Resize { .. }
    )
}

struct Runner<'a> {
    client: &'a Client,
    context: &'a Context,
    timeout: Duration,
    captures: HashMap<String, CopyOutputResult>,
    /// The element the `wait-change` right after the input just given looks
    /// at, as it was before the input.
    before_input: Option<Option<Element>>,
    /// The iframes, one in each child frame from the document shown, whose
    /// document selectors pick in (`frame`).
    frames: Vec<String>,
}

impl Runner<'_> {
    fn step(&mut self, command: &Command) -> Result<(), String> {
        match command {
            Command::Navigate(url) => {
                let url = self.resolve(url);
                self.call(
                    Method::MachopoNavigateFromAddressBar(Navigate { url }),
                    "navigate",
                )
                .map(drop)
            }
            Command::WaitLoaded => self
                .call(Method::MachopoWaitForLoad, "the page didn't finish loading")
                .map(drop),
            Command::Hover(target) => {
                let at = self.point(target)?;
                self.give(WindowInput {
                    kind: "move".into(),
                    x: at.x,
                    y: at.y,
                    ..Default::default()
                })
            }
            Command::WaitCursor(cursor) => {
                let mut last = String::new();
                self.poll(|runner| {
                    let answer = runner.call(
                        Method::MachopoGetCursor,
                        "no answer about the cursor: the window is gone",
                    )?;
                    last = answer["cursor"].as_str().unwrap_or_default().to_owned();
                    Ok(last == *cursor)
                })
                .map_err(|e| e.or(format!("the cursor never was {cursor}; it was {last}")))
            }
            Command::Click(target) => {
                let at = self.point(target)?;
                self.give(WindowInput {
                    kind: "click".into(),
                    x: at.x,
                    y: at.y,
                    ..Default::default()
                })
            }
            Command::DoubleClick(target) => {
                let at = self.point(target)?;
                self.give(WindowInput {
                    kind: "dblclick".into(),
                    x: at.x,
                    y: at.y,
                    ..Default::default()
                })
            }
            Command::Wheel { target, dx, dy } => {
                let at = self.point(target)?;
                self.give(WindowInput {
                    kind: "wheel".into(),
                    x: at.x,
                    y: at.y,
                    delta_x: *dx,
                    delta_y: *dy,
                    ..Default::default()
                })
            }
            Command::Key {
                key,
                modifiers,
                primary,
                text,
            } => {
                let mut modifiers = *modifiers;
                if *primary {
                    modifiers |= if self.context.mac {
                        ModifiersState::SUPER
                    } else {
                        ModifiersState::CONTROL
                    };
                }
                let mut bits = 0;
                for (state, bit) in [
                    (ModifiersState::ALT, automation::ALT),
                    (ModifiersState::CONTROL, automation::CTRL),
                    (ModifiersState::SUPER, automation::META),
                    (ModifiersState::SHIFT, automation::SHIFT),
                ] {
                    if modifiers.contains(state) {
                        bits |= bit;
                    }
                }
                let key = crate::scenario::key_name(key)
                    .ok_or_else(|| format!("no name for the key {key:?}"))?;
                self.give(WindowInput {
                    kind: "key".into(),
                    key: Some(key),
                    modifiers: bits,
                    text: text.clone(),
                    ..Default::default()
                })
            }
            // `run` follows them.
            Command::Platform(_) => Ok(()),
            Command::WaitText { selector, text } => {
                let mut last = None;
                self.poll(|runner| {
                    let element = runner.query(selector)?.element;
                    let done = element.as_ref().is_some_and(|e| e.text == *text);
                    last = Some(element.map(|e| e.text));
                    Ok(done)
                })
                .map_err(|e| match (e, last) {
                    (Waited::Error(e), _) => e,
                    (Waited::TimedOut, Some(Some(seen))) => {
                        format!("{selector} never had the text {text:?}; it had {seen:?}")
                    }
                    (Waited::TimedOut, _) => format!("nothing matched {selector}"),
                })
            }
            Command::WaitTitle(title) => {
                let mut last = String::new();
                self.poll(|runner| {
                    last = runner.title()?;
                    Ok(last == *title)
                })
                .map_err(|e| {
                    e.or(format!(
                        "the tab's title never was {title:?}; it was {last:?}"
                    ))
                })
            }
            Command::WaitChange(selector) => {
                let first = match self.before_input.take() {
                    Some(before) => before,
                    None => self.query(selector)?.element,
                };
                self.poll(|runner| Ok(runner.query(selector)?.element != first))
                    .map_err(|e| e.or(format!("{selector} didn't change from {first:?}")))
            }
            Command::ScrollTo(selector) => self.scroll_into_view(selector),
            Command::Frame(Some(selector)) => {
                self.frames.push(selector.clone());
                Ok(())
            }
            Command::Frame(None) => {
                self.frames.clear();
                Ok(())
            }
            Command::WaitBusy => self
                .poll(|runner| runner.busy())
                .map_err(|e| e.or("the page never got busy".into())),
            Command::Capture { name, now } => {
                let frame = self.copy_output(*now)?;
                self.captures.insert(name.clone(), frame);
                Ok(())
            }
            Command::ExpectDiffers { name, selector } => {
                let before = self
                    .captures
                    .get(name)
                    .ok_or_else(|| format!("nothing was captured as {name:?}"))?
                    .clone();
                let region = match selector {
                    None => None,
                    Some(selector) => Some(self.box_of(selector)?),
                };
                // Any frame: it waits for one that differs anyway, and works
                // while the page is busy. The first one on the default
                // timeout, whatever the scenario's, which is for the change
                // to show: a busy machine can take longer than a short one
                // to draw any frame, and the wait would fail for that
                // instead of for what it saw.
                let first = self
                    .capture_window(true, self.timeout.max(DEFAULT_TIMEOUT))?
                    .ok_or_else(|| NO_FRAME.to_owned())?;
                if differs(&before, &first, region) {
                    return Ok(());
                }
                self.poll(|runner| {
                    // One not drawn in time doesn't differ yet.
                    Ok(runner
                        .capture_window(true, runner.timeout)?
                        .is_some_and(|after| differs(&before, &after, region)))
                })
                .map_err(|e| {
                    let place = selector.as_deref().unwrap_or("the window");
                    e.or(format!("{place} still looks as it did at {name:?}"))
                })
            }
            Command::Screenshot { name, now } => {
                let frame = self.copy_output(*now)?;
                let Some(out) = &self.context.out else {
                    return Ok(());
                };
                std::fs::create_dir_all(out).map_err(|e| format!("{}: {e}", out.display()))?;
                let path = out.join(format!("{}.png", name.trim_end_matches(".png")));
                crate::headless::write_png(&path, frame.size, &frame.pixels)
                    .map_err(|e| format!("{}: {e}", path.display()))
            }
            Command::Resize { width, height } => {
                let bounds = Bounds {
                    width: Some(clamp_round(*width)),
                    height: Some(clamp_round(*height)),
                    ..Bounds::default()
                };
                let method = Method::BrowserSetWindowBounds(SetWindowBounds {
                    window_id: WINDOW_ID,
                    bounds,
                });
                self.call(method, "the window didn't take the input")
                    .map(drop)
            }
            Command::Timeout(timeout) => {
                self.timeout = *timeout;
                Ok(())
            }
        }
    }

    /// `method`'s result, within the scenario's timeout; `timed_out` if it
    /// takes longer.
    fn call(&self, method: Method, timed_out: &str) -> Result<Value, String> {
        self.call_within(method, self.timeout)?
            .ok_or_else(|| timed_out.to_owned())
    }

    /// `method`'s result, or `None` if it takes longer than `timeout`.
    fn call_within(&self, method: Method, timeout: Duration) -> Result<Option<Value>, String> {
        match self.client.call(method, timeout) {
            Ok(value) => Ok(Some(value)),
            Err(CallError::TimedOut) => Ok(None),
            Err(CallError::Gone) => Err("the window has gone".into()),
            Err(CallError::Failed(error)) => Err(error.message),
        }
    }

    /// Gives the window `input`, and waits until it has it.
    fn give(&self, input: WindowInput) -> Result<(), String> {
        self.call(
            Method::MachopoDispatchWindowInput(input),
            "the window didn't take the input",
        )
        .map(drop)
    }

    /// A URL as given, or a path relative to the scenario as a `file:` URL.
    fn resolve(&self, url: &str) -> String {
        if url.starts_with("about:") || url.contains("://") {
            return url.to_owned();
        }
        crate::to_url(&self.context.base.join(url).to_string_lossy())
    }

    /// Waits for the renderer to have handled the input given so far, so
    /// that what it says next (where an element is, the frame it draws)
    /// takes that input in: a click that moves something is laid out and
    /// drawn before the next element is looked for. `wait-busy`, input at a
    /// point and `capture now` don't wait: they are for a page whose main
    /// thread is still busy with the input.
    fn input_handled(&self) -> Result<(), String> {
        self.call(
            Method::MachopoWaitForInputHandled,
            "the page hasn't handled the input given it: is it busy?",
        )
        .map(drop)
    }

    fn query(&self, selector: &str) -> Result<QueryReply, String> {
        self.input_handled()?;
        let method = Method::MachopoQueryElement(QueryElement {
            selector: selector.to_owned(),
            frames: self.frames.clone(),
        });
        let value = self.call(
            method,
            &format!("no answer about {selector}: the page is busy, or gone"),
        )?;
        let result: QueryResult =
            serde_json::from_value(value).map_err(|e| format!("Machopo.queryElement: {e}"))?;
        Ok(QueryReply {
            element: result.element.map(|element| Element {
                rect: element.rect.map(rect),
                text: element.text,
            }),
            page: result.page.map(rect),
            scale: result.scale,
        })
    }

    /// Scrolls `selector`'s element into view if its center is out of it,
    /// once it has a box, and waits until that is drawn.
    fn scroll_into_view(&self, selector: &str) -> Result<(), String> {
        self.element_rect(selector)?;
        let method = Method::MachopoScrollIntoViewIfNeeded(QueryElement {
            selector: selector.to_owned(),
            frames: self.frames.clone(),
        });
        self.call(
            method,
            &format!("{selector} wasn't scrolled into view: the page is busy, or gone"),
        )
        .map(drop)
    }

    /// The active tab's title, as its tab shows it: the browser's, so it
    /// takes no input in (the renderer sends the title when it changes).
    fn title(&self) -> Result<String, String> {
        let title = self.call(
            Method::MachopoGetTitle,
            "no answer about the title: the window is gone",
        )?;
        Ok(title["title"].as_str().unwrap_or_default().to_owned())
    }

    /// Whether the renderer's main thread leaves a query unanswered for
    /// `BUSY`. The answer that comes later is passed over.
    fn busy(&self) -> Result<bool, String> {
        let method = Method::MachopoQueryElement(QueryElement {
            selector: "html".into(),
            frames: Vec::new(),
        });
        match self.client.call(method, BUSY) {
            Ok(_) => Ok(false),
            Err(CallError::TimedOut) => Ok(true),
            Err(CallError::Gone) => Err("the page has gone".into()),
            Err(CallError::Failed(error)) => Err(error.message),
        }
    }

    /// A frame as `Capture { now }` picks it.
    fn copy_output(&self, now: bool) -> Result<CopyOutputResult, String> {
        if !now {
            self.input_handled().map_err(|_| {
                "the page hasn't handled the input given it, so no frame shows it: is it busy \
                 (then add `now`)?"
                    .to_owned()
            })?;
        }
        self.capture_window(now, self.timeout)?.ok_or_else(|| {
            if now {
                NO_FRAME.to_owned()
            } else {
                "no frame showing the page was drawn: is the page busy (then add `now`), \
                 or the window hidden?"
                    .to_owned()
            }
        })
    }

    /// `Machopo.captureWindow`: a frame, or `None` if none came within `timeout`.
    fn capture_window(
        &self,
        now: bool,
        timeout: Duration,
    ) -> Result<Option<CopyOutputResult>, String> {
        let Some(value) =
            self.call_within(Method::MachopoCaptureWindow(CaptureWindow { now }), timeout)?
        else {
            return Ok(None);
        };
        let frame: CaptureResult =
            serde_json::from_value(value).map_err(|e| format!("Machopo.captureWindow: {e}"))?;
        let pixels = base64::engine::general_purpose::STANDARD
            .decode(&frame.data)
            .map_err(|e| format!("Machopo.captureWindow: {e}"))?;
        Ok(Some(CopyOutputResult {
            size: Size::new(frame.width, frame.height),
            pixels,
        }))
    }

    /// Where `target` is, once it has a box. An element is scrolled into
    /// view first if its center is out of it, as WebDriver's Element Click
    /// does. Its center has to be on the page as the window shows it: a
    /// person couldn't point at it otherwise, and the window gives a point
    /// off the page to nothing.
    fn point(&self, target: &Target) -> Result<PointF, String> {
        match target {
            Target::Point(point) => Ok(*point),
            Target::Element(selector) => {
                self.scroll_into_view(selector)?;
                let rect = self.element_rect(selector)?;
                let center = PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
                let page = self.query(selector)?.page;
                if !page.is_some_and(|page| page.contains(center)) {
                    return Err(format!(
                        "the center of {selector}, ({}, {}), is off the page in the window, \
                         even scrolled to: make the window bigger (`window size=`)",
                        center.x, center.y
                    ));
                }
                Ok(center)
            }
        }
    }

    /// The element's box in a frame's physical pixels.
    fn box_of(&self, selector: &str) -> Result<RectF, String> {
        let rect = self.element_rect(selector)?;
        let scale = self.query(selector)?.scale;
        Ok(RectF::new(
            rect.x * scale,
            rect.y * scale,
            rect.width * scale,
            rect.height * scale,
        ))
    }

    /// The element's box, CSS px of the window, waiting for it to have one.
    fn element_rect(&self, selector: &str) -> Result<RectF, String> {
        let mut rect = None;
        self.poll(|runner| {
            rect = runner.query(selector)?.element.and_then(|e| e.rect);
            Ok(rect.is_some())
        })
        .map_err(|e| e.or(format!("nothing matching {selector} has a box")))?;
        Ok(rect.expect("found"))
    }

    /// Calls `check` until it says done, or the timeout passes.
    fn poll(&self, mut check: impl FnMut(&Self) -> Result<bool, String>) -> Result<(), Waited> {
        let deadline = deadline(self.timeout);
        loop {
            if check(self).map_err(Waited::Error)? {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(Waited::TimedOut);
            }
            std::thread::sleep(POLL);
        }
    }
}

/// Why a wait ended without its condition.
enum Waited {
    TimedOut,
    Error(String),
}

impl Waited {
    /// The error, or `timed_out` for a timeout.
    fn or(self, timed_out: String) -> String {
        match self {
            Waited::TimedOut => timed_out,
            Waited::Error(e) => e,
        }
    }
}

/// The longest a wait waits: a century, which no clock's `Instant` is near
/// the end of. A scenario's `timeout` (or a test's) past it is as good as
/// none.
const LONGEST_WAIT: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

/// When a wait of `timeout` from now ends.
pub(crate) fn deadline(timeout: Duration) -> Instant {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "now plus a century at most: within any clock's range"
    )]
    let deadline = Instant::now() + timeout.min(LONGEST_WAIT);
    deadline
}

/// Whether two frames differ, within `region` (physical px) if given. A
/// frame of another size differs; within a region, only the part of it both
/// frames have counts (the window was resized: does the element look
/// different where it is now?).
fn differs(a: &CopyOutputResult, b: &CopyOutputResult, region: Option<RectF>) -> bool {
    if a.size != b.size && region.is_none() {
        return true;
    }
    let width = a.size.width.min(b.size.width);
    let height = a.size.height.min(b.size.height);
    let (x0, y0, x1, y1) = match region {
        None => (0, 0, width, height),
        Some(r) => (
            clamp_floor::<u32>(r.x).min(width),
            clamp_floor::<u32>(r.y).min(height),
            clamp_ceil::<u32>(r.right()).min(width),
            clamp_ceil::<u32>(r.bottom()).min(height),
        ),
    };
    /// Row `y`'s pixels from `x0` to `x1`, in `frame`'s own width.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "y < height and x0 <= x1 <= width (a box's right is not left of its \
                  left), both no more than `frame`'s: within its pixels"
    )]
    fn row(frame: &CopyOutputResult, y: u32, x0: u32, x1: u32) -> &[u8] {
        let start = (y as usize * frame.size.width as usize + x0 as usize) * 4;
        &frame.pixels[start..start + (x1 - x0) as usize * 4]
    }
    (y0..y1).any(|y| row(a, y, x0, x1) != row(b, y, x0, x1))
}

#[cfg(test)]
mod tests {
    use gfx::Size;

    use super::*;

    /// A 4×2 frame, `changed` pixels (x, y) red, the rest black.
    fn frame(changed: &[(u32, u32)]) -> CopyOutputResult {
        let mut pixels = [0, 0, 0, 255].repeat(8);
        for &(x, y) in changed {
            let i = (y as usize * 4 + x as usize) * 4;
            pixels[i] = 255;
        }
        CopyOutputResult {
            size: Size::new(4, 2),
            pixels,
        }
    }

    #[test]
    fn frames_differ_in_a_region() {
        let before = frame(&[]);
        assert!(!differs(&before, &frame(&[]), None));
        assert!(differs(&before, &frame(&[(3, 1)]), None));
        // Only within the region; its edges are whole pixels around it.
        let left = Some(RectF::new(0.0, 0.0, 2.0, 2.0));
        assert!(!differs(&before, &frame(&[(3, 1)]), left));
        assert!(differs(&before, &frame(&[(1, 1)]), left));
        assert!(differs(
            &before,
            &frame(&[(2, 0)]),
            Some(RectF::new(1.5, 0.0, 0.6, 0.5))
        ));
        // Past the frame: what is in it.
        assert!(differs(
            &before,
            &frame(&[(3, 1)]),
            Some(RectF::new(3.0, -5.0, 10.0, 10.0))
        ));
        // Another size.
        let other = CopyOutputResult {
            size: Size::new(2, 4),
            pixels: before.pixels.clone(),
        };
        assert!(differs(&before, &other, None));
    }

    /// Within a region, frames of different sizes are compared where both
    /// have pixels, row by row in each frame's own width.
    #[test]
    fn a_region_of_frames_of_different_sizes() {
        let before = frame(&[(1, 1)]);
        // 3x3, black, red at (1, 1) and (2, 2).
        let mut pixels = [0, 0, 0, 255].repeat(9);
        for (x, y) in [(1, 1), (2, 2)] {
            pixels[(y * 3 + x) * 4] = 255;
        }
        let bigger = CopyOutputResult {
            size: Size::new(3, 3),
            pixels,
        };
        let top_left = Some(RectF::new(0.0, 0.0, 2.0, 2.0));
        assert!(!differs(&before, &bigger, top_left));
        // (2, 1) is black in both; (2, 2) is outside the smaller frame.
        assert!(!differs(
            &before,
            &bigger,
            Some(RectF::new(2.0, 0.0, 9.0, 9.0))
        ));
        assert!(differs(&before, &frame(&[]), top_left));
    }

    /// In a child frame (`frame`), an element is scrolled into view and
    /// pointed at as in the document shown: its frame path goes with every
    /// query and scroll, and the click goes to the center of its box, which
    /// the queries give in the window. `frame top` drops the path. The
    /// browser's end is played here: each command is answered as the
    /// window's session would.
    #[test]
    fn a_child_frames_element_is_scrolled_to_and_clicked() {
        use crossbeam_channel::unbounded;
        use protocol::automation::Response;
        use serde_json::json;

        let step = |command| Step { line: 1, command };
        let (commands_tx, commands) = unbounded::<AutomationCommand>();
        let (answers, answers_rx) = unbounded();
        let browser = std::thread::spawn(move || {
            let mut asked = Vec::new();
            for command in commands {
                let result = match &command.method {
                    Method::MachopoWaitForInputHandled => json!({}),
                    Method::MachopoQueryElement(query) => {
                        asked.push(format!("query {:?}", query.frames));
                        json!({
                            "element": { "rect": { "x": 10.0, "y": 20.0, "width": 10.0, "height": 10.0 }, "text": "" },
                            "page": { "x": 0.0, "y": 0.0, "width": 800.0, "height": 600.0 },
                            "scale": 1.0,
                        })
                    }
                    Method::MachopoScrollIntoViewIfNeeded(query) => {
                        asked.push(format!("scroll {:?}", query.frames));
                        json!({})
                    }
                    Method::MachopoDispatchWindowInput(input) => {
                        asked.push(format!("{} {} {}", input.kind, input.x, input.y));
                        json!({})
                    }
                    other => panic!("not asked for: {other:?}"),
                };
                let _ = answers.send(Message::Response(Response {
                    id: command.id,
                    session_id: None,
                    result: Ok(result),
                }));
            }
            asked
        });
        let client = Client::new(
            Box::new(move |command| commands_tx.send(command).is_ok()),
            answers_rx,
        );
        let steps = [
            step(Command::Frame(Some("#a".into()))),
            step(Command::Frame(Some("#b".into()))),
            step(Command::Click(Target::Element("#c".into()))),
            step(Command::Frame(None)),
            step(Command::ScrollTo("#d".into())),
        ];
        run(&steps, &Context::default(), &client).expect("runs");
        drop(client);
        let path = r##"["#a", "#b"]"##;
        assert_eq!(
            browser.join().unwrap(),
            [
                format!("query {path}"),
                format!("scroll {path}"),
                format!("query {path}"),
                format!("query {path}"),
                "click 15 25".into(),
                "query []".into(),
                "scroll []".into(),
            ]
        );
    }

    /// A call's answer is the one with its id: an answer that came too late
    /// for an earlier call (`busy` gives up on its queries) and events are
    /// passed over.
    #[test]
    fn a_call_takes_its_own_answer() {
        use crossbeam_channel::unbounded;
        use protocol::automation::{Event, EventKind, Response};
        use serde_json::json;

        let (answers, answers_rx) = unbounded();
        let answer = |id, result| {
            Message::Response(Response {
                id,
                session_id: None,
                result: Ok(result),
            })
        };
        // The late answer to a call this client made before.
        answers.send(answer(1, json!("late"))).unwrap();
        let client = Client::new(
            Box::new(move |command| {
                let told = Message::Event(Event {
                    session_id: None,
                    event: EventKind::TargetDestroyed {
                        target_id: "0".into(),
                    },
                });
                answers.send(told).is_ok() && answers.send(answer(command.id, json!("own"))).is_ok()
            }),
            answers_rx,
        );
        client.next_id.set(1);
        let result = client.call(Method::MachopoWaitForLoad, Duration::from_secs(5));
        assert_eq!(result.ok(), Some(json!("own")));
    }

    /// `wait-title` waits until the tab's title is the one given, asking
    /// the window (whose title changes as the renderer sends it), and fails
    /// naming the last title seen.
    #[test]
    fn wait_title_waits_for_the_title() {
        use std::sync::{Arc, Mutex};

        use crossbeam_channel::unbounded;
        use protocol::automation::Response;
        use serde_json::json;

        let step = |command| Step { line: 1, command };
        let titles = Arc::new(Mutex::new(vec!["third", "second", "first"]));
        let client = {
            let titles = titles.clone();
            let (answers, answers_rx) = unbounded();
            Client::new(
                Box::new(move |command| {
                    let title = match command.method {
                        Method::MachopoGetTitle => {
                            let mut titles = titles.lock().unwrap();
                            if titles.len() > 1 {
                                titles.pop().unwrap()
                            } else {
                                titles[0]
                            }
                        }
                        other => panic!("not asked for: {other:?}"),
                    };
                    let answer = Message::Response(Response {
                        id: command.id,
                        session_id: None,
                        result: Ok(json!({ "title": title })),
                    });
                    answers.send(answer).is_ok()
                }),
                answers_rx,
            )
        };
        let steps = [step(Command::WaitTitle("second".into()))];
        run(&steps, &Context::default(), &client).expect("the title comes");
        assert_eq!(titles.lock().unwrap().len(), 1, "asked until it came");
        let steps = [
            step(Command::Timeout(Duration::from_millis(100))),
            step(Command::WaitTitle("never".into())),
        ];
        let failure = run(&steps, &Context::default(), &client).expect_err("never comes");
        assert!(
            failure.message.contains("\"never\"") && failure.message.contains("\"third\""),
            "{}",
            failure.message
        );
    }

    /// `expect-differs` gets its first frame however long the window takes
    /// to draw one (a busy machine), longer than the scenario's timeout,
    /// which is for the change to show: it fails because the window looked
    /// the same, not because no frame came in time.
    #[test]
    fn expect_differs_waits_out_a_slow_first_frame() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        use crossbeam_channel::unbounded;
        use protocol::automation::Response;
        use serde_json::json;

        let step = |command| Step { line: 1, command };
        // Each frame takes 300 ms; with `changes`, those after the first
        // differ from it.
        let slow = |changes: bool| {
            let (answers, answers_rx) = unbounded();
            let drawn = AtomicUsize::new(0);
            Client::new(
                Box::new(move |command| {
                    let Method::MachopoCaptureWindow(_) = command.method else {
                        panic!("not asked for: {:?}", command.method);
                    };
                    #[expect(
                        clippy::disallowed_methods,
                        reason = "a test's count: far from wrapping"
                    )]
                    let first = drawn.fetch_add(1, Ordering::SeqCst) == 0;
                    let frame = frame(if changes && !first { &[(0, 0)] } else { &[] });
                    let answers = answers.clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(Duration::from_millis(300));
                        let data = base64::engine::general_purpose::STANDARD.encode(&frame.pixels);
                        let _ = answers.send(Message::Response(Response {
                            id: command.id,
                            session_id: None,
                            result: Ok(json!({ "width": 4, "height": 2, "data": data })),
                        }));
                    });
                    true
                }),
                answers_rx,
            )
        };
        let steps = [
            step(Command::Capture {
                name: "a".into(),
                now: true,
            }),
            step(Command::Timeout(Duration::from_millis(100))),
            step(Command::ExpectDiffers {
                name: "a".into(),
                selector: None,
            }),
        ];
        let failure = run(&steps, &Context::default(), &slow(false)).expect_err("no change");
        assert_eq!(failure.message, "the window still looks as it did at \"a\"");
        run(&steps, &Context::default(), &slow(true)).expect("the change shows");
    }
}
