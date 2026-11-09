//! The dispatcher: WebDriver's commands, carried out one at a time against
//! the session's machopo (chromedriver's `Session` and its commands). One
//! session at a time; each is a machopo of its own, which ends with it.
//!
//! Each window is a tab: its handle is the tab's target id, and the
//! driver has a page session for it (`Target.attachToTarget`) once it has
//! been the session's window. New Window opens a tab behind the one shown
//! (machopo has one platform window: a tab whatever the type asked); Switch To
//! Window shows the tab it switches to (`Target.activateTarget`), as
//! chromedriver does, since a tab not shown draws nothing. The window's
//! rect is machopo's one window's (`Browser.getWindowForTarget`).
//!
//! The current browsing context is the window's top-level one or a child
//! frame in it: the driver keeps the frames switched to from the top
//! (chromedriver's frame stack), by the ids machopo's renderer gives
//! (`Machopo.switchToFrame`), and the `Machopo` domain's WebDriver methods carry
//! the last. Navigating, and switching windows, go back to the top.
//!
//! Navigations wait as the page load strategy says, on machopo's
//! `Page.lifecycleEvent`s for the navigation's loader and, for `normal`,
//! on `Machopo.waitForLoad` (the page drawn). Element Click asks
//! machopo where the pointer goes (`Machopo.getElementClickPoint`, the element
//! scrolled into view), then moves it there and clicks with
//! `Input.dispatchMouseEvent`, straight to the tab's renderer, as
//! chromedriver does; a navigation the click started is waited for too.
//! Perform Actions sends keys and the mouse to the tab's renderer as Element
//! Click does, with an input state for each window (`actions`); so does
//! Element Send Keys, once machopo has given the element the focus
//! (`Machopo.prepareSendKeys`). Element Clear is machopo's (`Machopo.clearElement`).
//! Screenshots are machopo's: `Page.captureScreenshot` of the page's view,
//! `Machopo.captureElementScreenshot` of an element's part of it. Script, finding
//! elements and reading them are the `Machopo` domain's WebDriver methods,
//! which machopo answers with WebDriver's JSON and errors (the CDP error's
//! `data`); the driver keeps the timeouts: the script's, and the implicit
//! wait, finding again until something is found.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use base::SequenceNumber;
use base::numerics::{f32_from_f64, saturated_cast};
use crossbeam_channel::Receiver;
use protocol::automation::{
    self, AttachToTarget, Bounds, CaptureScreenshot, CreateTarget, DispatchKeyEvent,
    DispatchMouseEvent, ElementClickPointParams, ElementParams, EventKind, ExecuteScript,
    FindElements, FrameParams, GetTargetInfo, Method, NamedElementParams, Navigate,
    NavigateToHistoryEntry, Reload, SendKeysParams, SetDiscoverTargets, SetLifecycleEventsEnabled,
    SetWindowBounds, SwitchToFrame, TargetParams,
};
use serde_json::{Map, Value, json};
use webdriver::capabilities::{BrowserCapabilities, Capabilities, CapabilitiesMatching};
use webdriver::command::{
    ActionsParameters, JavascriptCommandParameters, LocatorParameters, NewSessionParameters,
    TimeoutsParameters, WebDriverCommand, WebDriverMessage, WindowRectParameters,
};
use webdriver::common::FrameId;
use webdriver::error::{ErrorStatus, WebDriverResult};
use webdriver::response::{
    CloseWindowResponse, NewSessionResponse, NewWindowResponse, TimeoutsResponse, ValueResponse,
    WebDriverResponse, WindowRectResponse,
};

use crate::Options;
use crate::actions::{self, Input, InputState, Viewport};
use crate::error::{Failure, Result};
use crate::http::Call;
use crate::machopo::Machopo;

/// WebDriver's default timeouts, in ms.
const DEFAULT_SCRIPT_TIMEOUT: u64 = 30_000;
const DEFAULT_PAGE_LOAD_TIMEOUT: u64 = 300_000;
/// How often finding is tried again during the implicit wait.
const FIND_INTERVAL: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PageLoadStrategy {
    None,
    Eager,
    Normal,
}

impl PageLoadStrategy {
    fn name(self) -> &'static str {
        match self {
            PageLoadStrategy::None => "none",
            PageLoadStrategy::Eager => "eager",
            PageLoadStrategy::Normal => "normal",
        }
    }
}

/// WebDriver's timeouts, in ms; `None` (`null`): none.
struct Timeouts {
    script: Option<u64>,
    page_load: Option<u64>,
    implicit: Option<u64>,
}

struct Session {
    id: String,
    machopo: Machopo,
    /// The window's handle: its tab's target id.
    window: String,
    /// The page session of each window that has been the session's, by
    /// handle.
    pages: HashMap<String, String>,
    /// The child frames switched to from the window's top-level browsing
    /// context, outermost first: the current browsing context is the last
    /// (the top-level one if none), its parent the one before.
    frames: Vec<String>,
    timeouts: Timeouts,
    page_load_strategy: PageLoadStrategy,
    /// Each window's input state, by handle, made by its first Perform
    /// Actions.
    inputs: HashMap<String, InputState>,
}

/// Carries out the commands `calls` brings, in order, until the server
/// goes.
pub fn dispatch(calls: Receiver<Call>, options: Options) {
    let mut dispatcher = Dispatcher {
        options,
        session: None,
        sessions: SequenceNumber::starting_at(1),
    };
    for call in calls {
        let result = dispatcher
            .handle(call.message)
            .map(|response| serde_json::to_string(&response).expect("serializes"));
        let _ = call.reply.send(result);
    }
    if let Some(session) = dispatcher.session.take() {
        session.machopo.close();
    }
}

struct Dispatcher {
    options: Options,
    session: Option<Session>,
    /// Numbers the sessions made, from 1, for their ids.
    sessions: SequenceNumber<u64>,
}

impl Dispatcher {
    fn handle(&mut self, message: WebDriverMessage) -> Result<WebDriverResponse> {
        match message.command {
            WebDriverCommand::Status => {
                let ready = self.session.is_none();
                return Ok(WebDriverResponse::Generic(ValueResponse(json!({
                    "ready": ready,
                    "message": if ready { "" } else { "a session is running" },
                }))));
            }
            WebDriverCommand::NewSession(params) => return self.new_session(params),
            _ => {}
        }
        let session = match (&self.session, &message.session_id) {
            (Some(session), Some(id)) if session.id == *id => {
                self.session.as_mut().expect("just matched")
            }
            _ => {
                return Err(Failure::new(
                    ErrorStatus::InvalidSessionId,
                    "no such session",
                ));
            }
        };
        match message.command {
            WebDriverCommand::DeleteSession => {
                let session = self.session.take().expect("checked");
                session.machopo.close();
                Ok(WebDriverResponse::DeleteSession)
            }
            WebDriverCommand::CloseWindow => {
                let handles = session.close_window()?;
                // The last window closed: so did machopo, and the session ends.
                if handles.is_empty() {
                    let session = self.session.take().expect("checked");
                    session.machopo.close();
                }
                Ok(WebDriverResponse::CloseWindow(CloseWindowResponse(handles)))
            }
            WebDriverCommand::GetTimeouts => {
                Ok(WebDriverResponse::Timeouts(TimeoutsResponse::new(
                    session.timeouts.script,
                    session.timeouts.page_load,
                    session.timeouts.implicit,
                )))
            }
            WebDriverCommand::SetTimeouts(params) => {
                session.set_timeouts(params)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::Get(params) => {
                session.navigate(&params.url)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::GetCurrentUrl => {
                let info = session.live_window_info()?;
                Ok(value(info["url"].clone()))
            }
            WebDriverCommand::GetTitle => {
                let info = session.live_window_info()?;
                Ok(value(info["title"].clone()))
            }
            WebDriverCommand::GoBack => {
                session.traverse(-1)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::GoForward => {
                session.traverse(1)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::Refresh => {
                session.refresh()?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::GetWindowHandle => {
                session.live_window_info()?;
                Ok(value(json!(session.window)))
            }
            WebDriverCommand::GetWindowHandles => Ok(value(json!(session.window_handles()?))),
            WebDriverCommand::NewWindow(_) => session.new_window().map(|handle| {
                WebDriverResponse::NewWindow(NewWindowResponse {
                    handle,
                    typ: "tab".into(),
                })
            }),
            WebDriverCommand::SwitchToWindow(params) => {
                session.switch_to_window(&params.handle)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::SwitchToFrame(params) => {
                session.switch_to_frame(params.id)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::SwitchToParentFrame => {
                session.switch_to_parent_frame()?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::GetWindowRect => {
                session.window_rect().map(WebDriverResponse::WindowRect)
            }
            WebDriverCommand::SetWindowRect(params) => session
                .set_window_rect(params)
                .map(WebDriverResponse::WindowRect),
            WebDriverCommand::ExecuteScript(params) => session.execute(params, false).map(value),
            WebDriverCommand::ExecuteAsyncScript(params) => {
                session.execute(params, true).map(value)
            }
            WebDriverCommand::FindElement(params) => session.find(params, None, true).map(value),
            WebDriverCommand::FindElements(params) => session.find(params, None, false).map(value),
            WebDriverCommand::FindElementElement(element, params) => {
                session.find(params, Some(element.0), true).map(value)
            }
            WebDriverCommand::FindElementElements(element, params) => {
                session.find(params, Some(element.0), false).map(value)
            }
            WebDriverCommand::ElementClick(element) => {
                session.click(element.0)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::TakeScreenshot => session.screenshot().map(value),
            WebDriverCommand::TakeElementScreenshot(element) => {
                let frame = session.frame();
                session
                    .webdriver(Method::MachopoCaptureElementScreenshot(ElementParams {
                        frame,
                        element: element.0,
                    }))
                    .map(value)
            }
            WebDriverCommand::ElementSendKeys(element, params) => {
                session.send_keys(element.0, params.text)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::ElementClear(element) => {
                let frame = session.frame();
                session.webdriver_interactable(Method::MachopoClearElement(ElementParams {
                    frame,
                    element: element.0,
                }))?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::PerformActions(params) => {
                session.perform_actions(params)?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::ReleaseActions => {
                session.release_actions()?;
                Ok(WebDriverResponse::Void)
            }
            WebDriverCommand::GetElementRect(element) => {
                let frame = session.frame();
                session
                    .webdriver(Method::MachopoGetElementRect(ElementParams {
                        frame,
                        element: element.0,
                    }))
                    .map(value)
            }
            WebDriverCommand::GetElementTagName(element) => {
                let frame = session.frame();
                session
                    .webdriver(Method::MachopoGetElementTagName(ElementParams {
                        frame,
                        element: element.0,
                    }))
                    .map(value)
            }
            WebDriverCommand::GetElementAttribute(element, name) => {
                let frame = session.frame();
                session
                    .webdriver(Method::MachopoGetElementAttribute(NamedElementParams {
                        frame,
                        element: element.0,
                        name,
                    }))
                    .map(value)
            }
            WebDriverCommand::GetElementProperty(element, name) => {
                let frame = session.frame();
                session
                    .webdriver(Method::MachopoGetElementProperty(NamedElementParams {
                        frame,
                        element: element.0,
                        name,
                    }))
                    .map(value)
            }
            other => Err(Failure::new(
                ErrorStatus::UnknownCommand,
                format!("machopodriver doesn't do {other:?} yet"),
            )),
        }
    }

    fn new_session(&mut self, params: NewSessionParameters) -> Result<WebDriverResponse> {
        if self.session.is_some() {
            return Err(Failure::new(
                ErrorStatus::SessionNotCreated,
                "Session is already started",
            ));
        }
        let matched = params
            .capabilities
            .match_browser(&mut MachopoCapabilities)?
            .ok_or_else(|| {
                Failure::new(
                    ErrorStatus::SessionNotCreated,
                    "Unable to find a matching set of capabilities",
                )
            })?;
        let page_load_strategy = match matched.get("pageLoadStrategy").and_then(Value::as_str) {
            Some("none") => PageLoadStrategy::None,
            Some("eager") => PageLoadStrategy::Eager,
            _ => PageLoadStrategy::Normal,
        };
        // The crate checked them: `null`, or a whole number of ms (`2.0`
        // too).
        let requested = matched.get("timeouts");
        let timeout = |name: &str, default: u64| match requested.and_then(|t| t.get(name)) {
            None => Some(default),
            Some(value) => value
                .as_u64()
                .or_else(|| value.as_f64().map(saturated_cast::<u64, _>)),
        };
        let timeouts = Timeouts {
            script: timeout("script", DEFAULT_SCRIPT_TIMEOUT),
            page_load: timeout("pageLoad", DEFAULT_PAGE_LOAD_TIMEOUT),
            implicit: timeout("implicit", 0),
        };
        let not_created = |e: String| Failure::new(ErrorStatus::SessionNotCreated, e);
        let mut machopo = Machopo::start(&self.options).map_err(not_created)?;
        let started = (|| {
            let version = machopo.call_ok(None, Method::BrowserGetVersion)?;
            machopo.call_ok(
                None,
                Method::TargetSetDiscoverTargets(SetDiscoverTargets { discover: true }),
            )?;
            let targets = machopo.call_ok(None, Method::TargetGetTargets)?;
            let window = targets["targetInfos"][0]["targetId"]
                .as_str()
                .ok_or_else(|| not_created("machopo has no tab".into()))?
                .to_owned();
            let page = attach(&mut machopo, &window)?;
            Ok((version, window, page))
        })();
        let (version, window, page) = started.map_err(|e: Failure| not_created(e.message))?;
        let id = session_id(self.sessions.get_next());
        let mut capabilities = matched;
        capabilities.insert("browserName".into(), json!("machopo"));
        capabilities.insert("browserVersion".into(), json!(env!("CARGO_PKG_VERSION")));
        capabilities.insert("platformName".into(), json!(platform_name()));
        capabilities
            .entry("acceptInsecureCerts")
            .or_insert(json!(false));
        capabilities.insert("pageLoadStrategy".into(), json!(page_load_strategy.name()));
        capabilities.insert("proxy".into(), json!({}));
        capabilities.insert("setWindowRect".into(), json!(true));
        capabilities
            .entry("strictFileInteractability")
            .or_insert(json!(false));
        capabilities.insert(
            "timeouts".into(),
            json!({
                "script": timeouts.script,
                "pageLoad": timeouts.page_load,
                "implicit": timeouts.implicit,
            }),
        );
        capabilities
            .entry("unhandledPromptBehavior")
            .or_insert(json!("dismiss and notify"));
        capabilities.insert("userAgent".into(), version["userAgent"].clone());
        self.session = Some(Session {
            id: id.clone(),
            machopo,
            pages: HashMap::from([(window.clone(), page)]),
            window,
            frames: Vec::new(),
            timeouts,
            page_load_strategy,
            inputs: HashMap::new(),
        });
        Ok(WebDriverResponse::NewSession(NewSessionResponse::new(
            id,
            Value::Object(capabilities),
        )))
    }
}

fn value(value: Value) -> WebDriverResponse {
    WebDriverResponse::Generic(ValueResponse(value))
}

/// A page session for the tab `window`, which gets the events navigations
/// wait for.
fn attach(machopo: &mut Machopo, window: &str) -> Result<String> {
    let attached = machopo.call_ok(
        None,
        Method::TargetAttachToTarget(AttachToTarget {
            target_id: window.to_owned(),
            flatten: true,
        }),
    )?;
    let page = attached["sessionId"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    machopo.call_ok(Some(&page), Method::PageEnable)?;
    machopo.call_ok(
        Some(&page),
        Method::PageSetLifecycleEventsEnabled(SetLifecycleEventsEnabled { enabled: true }),
    )?;
    Ok(page)
}

/// machopo's one window (`Browser.getWindowForTarget`).
const WINDOW_ID: u64 = 1;

fn no_such_window() -> Failure {
    Failure::new(ErrorStatus::NoSuchWindow, "the window is closed")
}

/// `ms` from now: a WebDriver timeout's deadline. One past any clock (a
/// timeout is at most JavaScript's largest safe integer of ms, which every
/// clock machopo runs on holds) is none, as no timeout is.
fn after_ms(ms: u64) -> Option<Instant> {
    Instant::now().checked_add(Duration::from_millis(ms))
}

/// A session id: a UUID's form, from the session's number and the
/// process's hasher seed (unguessable enough for a server only this
/// machine reaches).
fn session_id(number: u64) -> String {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    let high = state.hash_one((number, std::process::id(), Instant::now()));
    let low = state.hash_one((high, number));
    let hex = format!("{high:016x}{low:016x}");
    format!(
        "{}-{}-4{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// WebDriver's `platformName` for the system machopo runs on.
fn platform_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "mac",
        other => other,
    }
}

impl Session {
    /// Those given; `null` is no timeout.
    fn set_timeouts(&mut self, params: TimeoutsParameters) -> Result<()> {
        if let Some(implicit) = params.implicit {
            self.timeouts.implicit = implicit;
        }
        if let Some(page_load) = params.page_load {
            self.timeouts.page_load = page_load;
        }
        if let Some(script) = params.script {
            self.timeouts.script = script;
        }
        Ok(())
    }

    /// The window's page session.
    fn page(&self) -> String {
        self.pages.get(&self.window).cloned().unwrap_or_default()
    }

    /// The current browsing context, if it is a child frame: its id.
    fn frame(&self) -> Option<String> {
        self.frames.last().cloned()
    }

    /// The window's target info; `no such window` if its tab is gone.
    fn window_info(&mut self) -> Result<Value> {
        let info = self.machopo.call(
            None,
            Method::TargetGetTargetInfo(GetTargetInfo {
                target_id: Some(self.window.clone()),
            }),
        )?;
        let info = info.map_err(|_| no_such_window())?;
        Ok(info["targetInfo"].clone())
    }

    /// [`window_info`](Self::window_info), `tab crashed` if the window's
    /// renderer has gone (as chromedriver answers). A navigation goes
    /// ahead: it launches another renderer.
    fn live_window_info(&mut self) -> Result<Value> {
        let info = self.window_info()?;
        if self.machopo.has_crashed(&self.window) {
            return Err(Failure::tab_crashed());
        }
        Ok(info)
    }

    fn window_handles(&mut self) -> Result<Vec<String>> {
        let targets = self.machopo.call_ok(None, Method::TargetGetTargets)?;
        Ok(targets["targetInfos"]
            .as_array()
            .map(|infos| {
                infos
                    .iter()
                    .filter_map(|info| info["targetId"].as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default())
    }

    /// New Window: a tab after the others, behind the one shown, showing
    /// `about:blank`; its handle. The session's window stays.
    fn new_window(&mut self) -> Result<String> {
        self.window_info()?;
        let created = self.machopo.call_ok(
            None,
            Method::TargetCreateTarget(CreateTarget {
                url: "about:blank".into(),
                background: true,
            }),
        )?;
        Ok(created["targetId"].as_str().unwrap_or_default().to_owned())
    }

    /// Close Window: closes the window's tab; the handles left. With none
    /// left, machopo has ended.
    fn close_window(&mut self) -> Result<Vec<String>> {
        self.window_info()?;
        // Asked first: machopo ends as its last tab closes.
        let handles = self.window_handles()?;
        self.machopo.call_ok(
            None,
            Method::TargetCloseTarget(TargetParams {
                target_id: self.window.clone(),
            }),
        )?;
        self.pages.remove(&self.window);
        self.inputs.remove(&self.window);
        self.frames.clear();
        Ok(handles
            .into_iter()
            .filter(|handle| *handle != self.window)
            .collect())
    }

    /// Switch To Window: the window `handle` becomes the session's, at its
    /// top-level browsing context, and is shown.
    fn switch_to_window(&mut self, handle: &str) -> Result<()> {
        if !self.window_handles()?.iter().any(|known| known == handle) {
            return Err(no_such_window());
        }
        if !self.pages.contains_key(handle) {
            let page = attach(&mut self.machopo, handle)?;
            self.pages.insert(handle.to_owned(), page);
        }
        self.machopo.call_ok(
            None,
            Method::TargetActivateTarget(TargetParams {
                target_id: handle.to_owned(),
            }),
        )?;
        self.window = handle.to_owned();
        self.frames.clear();
        Ok(())
    }

    /// Switch To Frame: to the window's top-level browsing context
    /// (`null`), or to a child frame of the current one, by its index in
    /// `window`'s or by its iframe.
    fn switch_to_frame(&mut self, id: FrameId) -> Result<()> {
        let (index, element) = match id {
            FrameId::Top => {
                self.live_window_info()?;
                self.frames.clear();
                return Ok(());
            }
            FrameId::Short(index) => (Some(index), None),
            FrameId::Element(element) => (None, Some(element.0)),
        };
        let frame = self.frame();
        let switched = self.webdriver(Method::MachopoSwitchToFrame(SwitchToFrame {
            frame,
            index,
            element,
        }))?;
        let id = switched
            .as_str()
            .ok_or_else(|| Failure::new(ErrorStatus::UnknownError, "machopo gave no frame"))?;
        self.frames.push(id.to_owned());
        Ok(())
    }

    /// Switch To Parent Frame: to the parent of the current browsing
    /// context, if it has one; `no such window` if the parent has gone.
    fn switch_to_parent_frame(&mut self) -> Result<()> {
        let Some(current) = self.frames.pop() else {
            self.live_window_info()?;
            return Ok(());
        };
        // The parent is still there: machopo gives back its id (`null` for the
        // top-level one).
        let checked = self.webdriver(Method::MachopoSwitchToFrame(SwitchToFrame {
            frame: self.frame(),
            ..SwitchToFrame::default()
        }));
        if let Err(failure) = checked {
            self.frames.push(current);
            return Err(failure);
        }
        Ok(())
    }

    /// The current browsing context is still there: `no such window` if
    /// not (its tab closed, or the child frame gone).
    fn check_context(&mut self) -> Result<()> {
        self.live_window_info()?;
        if self.frames.is_empty() {
            return Ok(());
        }
        let frame = self.frame();
        self.webdriver(Method::MachopoSwitchToFrame(SwitchToFrame {
            frame,
            ..SwitchToFrame::default()
        }))
        .map(drop)
    }

    /// Element Click: the pointer moved to the element's in-view center
    /// point (machopo scrolls it into view and checks nothing else would get
    /// the click), the primary button pressed and released there; then a
    /// navigation that started, waited for as the page load strategy says.
    fn click(&mut self, element: String) -> Result<()> {
        let frame = self.frame();
        let point = self.webdriver(Method::MachopoGetElementClickPoint(
            ElementClickPointParams {
                frame,
                element,
                in_view_center_only: false,
            },
        ))?;
        let at = |key: &str| f32_from_f64(point[key].as_f64().unwrap_or_default());
        let (x, y) = (at("x"), at("y"));
        let deadline = self.deadline();
        self.machopo.forget_events();
        let page = self.page();
        // The primary button's bit while it is down, and the first press of
        // a series, as chromedriver sends them.
        for (kind, button, buttons, click_count) in [
            ("mouseMoved", None, 0, 0),
            ("mousePressed", Some("left"), 1, 1),
            ("mouseReleased", Some("left"), 0, 1),
        ] {
            let event = DispatchMouseEvent {
                kind: kind.into(),
                x,
                y,
                button: button.map(str::to_owned),
                buttons,
                click_count,
                ..DispatchMouseEvent::default()
            };
            self.machopo
                .call_ok(Some(&page), Method::InputDispatchMouseEvent(event))?;
        }
        // The renderer handled the click before it answered: what the
        // click started has been told by now.
        let window = self.window.clone();
        let started = self.machopo.already(|event| match &event.event {
            EventKind::FrameStartedNavigating {
                frame_id,
                loader_id,
                ..
            } if *frame_id == window => Some(loader_id.clone()),
            _ => None,
        });
        match started {
            Some(loader) => self.wait_for_load(&loader, deadline),
            None => Ok(()),
        }
    }

    /// Perform Actions, with the window's input state (made if it has
    /// none).
    fn perform_actions(&mut self, params: ActionsParameters) -> Result<()> {
        self.check_context()?;
        let mut state = self.inputs.remove(&self.window).unwrap_or_default();
        let done = actions::process(params.actions, &mut state).and_then(|ticks| {
            actions::perform(&ticks, &mut state, &mut WindowInput { session: self })
        });
        self.inputs.insert(self.window.clone(), state);
        done
    }

    /// Element Send Keys: machopo gets the element ready (the focus, or a
    /// non-typeable control's value set), then the text's keys go to the
    /// window, from a key input source of their own in its input state.
    fn send_keys(&mut self, element: String, text: String) -> Result<()> {
        let frame = self.frame();
        let typing =
            self.webdriver_interactable(Method::MachopoPrepareSendKeys(SendKeysParams {
                frame,
                element,
                text: text.clone(),
            }))?;
        if typing != Value::Bool(true) {
            return Ok(());
        }
        let mut state = self.inputs.remove(&self.window).unwrap_or_default();
        let typed = actions::send_keys(&text, &mut state, &mut WindowInput { session: self });
        self.inputs.insert(self.window.clone(), state);
        typed
    }

    /// A `Machopo` WebDriver method that answers "element not interactable"
    /// until the element is: asked again until the implicit wait is over.
    fn webdriver_interactable(&mut self, method: Method) -> Result<Value> {
        let deadline = self.timeouts.implicit.and_then(after_ms);
        loop {
            match self.webdriver(method.clone()) {
                Err(failure)
                    if failure.error == "element not interactable"
                        && deadline.is_some_and(|deadline| Instant::now() < deadline) =>
                {
                    std::thread::sleep(FIND_INTERVAL);
                }
                answer => return answer,
            }
        }
    }

    /// Release Actions: what the window's input state has pressed, let go
    /// of, last first; then the state is dropped.
    fn release_actions(&mut self) -> Result<()> {
        self.check_context()?;
        let Some(mut state) = self.inputs.remove(&self.window) else {
            return Ok(());
        };
        actions::release(&mut state, &mut WindowInput { session: self })
    }

    /// Take Screenshot: the page's view, as the next frame that shows what
    /// it has done so far draws it; a PNG in base64.
    fn screenshot(&mut self) -> Result<Value> {
        self.live_window_info()?;
        let page = self.page();
        let captured = self.machopo.call(
            Some(&page),
            Method::PageCaptureScreenshot(CaptureScreenshot::default()),
        )?;
        let captured =
            captured.map_err(|e| Failure::new(ErrorStatus::UnableToCaptureScreen, e.message))?;
        Ok(captured["data"].clone())
    }

    /// Get Window Rect: machopo's window's position and size, CSS px.
    fn window_rect(&mut self) -> Result<WindowRectResponse> {
        let found = self.machopo.call(
            None,
            Method::BrowserGetWindowForTarget(GetTargetInfo {
                target_id: Some(self.window.clone()),
            }),
        )?;
        let bounds = found.map_err(|_| no_such_window())?["bounds"].clone();
        // machopo's window bounds, i32s (and sizes within 8192).
        let at = |key: &str| saturated_cast::<i32, _>(bounds[key].as_i64().unwrap_or_default());
        Ok(WindowRectResponse {
            x: at("left"),
            y: at("top"),
            width: at("width"),
            height: at("height"),
        })
    }

    /// Set Window Rect: what is given of the position and the size (the
    /// rest stays, as wdspec's `test_partial_input` has it); the rect then.
    fn set_window_rect(&mut self, params: WindowRectParameters) -> Result<WindowRectResponse> {
        self.window_rect()?;
        let bounds = Bounds {
            left: params.x,
            top: params.y,
            // The crate took only non-negative ones.
            width: params.width.map(i32::unsigned_abs),
            height: params.height.map(i32::unsigned_abs),
        };
        if bounds != Bounds::default() {
            self.machopo.call_ok(
                None,
                Method::BrowserSetWindowBounds(SetWindowBounds {
                    window_id: WINDOW_ID,
                    bounds,
                }),
            )?;
        }
        self.window_rect()
    }

    /// A `Machopo` WebDriver method in the window's tab: its value.
    fn webdriver(&mut self, method: Method) -> Result<Value> {
        Ok(self
            .webdriver_until(method, None)?
            .expect("no deadline to pass"))
    }

    /// [`webdriver`](Self::webdriver), giving up at `deadline`: `None`.
    fn webdriver_until(
        &mut self,
        method: Method,
        deadline: Option<Instant>,
    ) -> Result<Option<Value>> {
        self.live_window_info()?;
        let page = self.page();
        let Some(answer) = self.machopo.call_until(Some(&page), method, deadline)? else {
            return Ok(None);
        };
        match answer {
            Ok(result) => Ok(Some(result["value"].clone())),
            Err(error) => Err(self.failure(error)),
        }
    }

    /// Machopo's error as WebDriver's: the one its `data` names; with none, the
    /// tab's renderer went meanwhile, or something else went wrong.
    fn failure(&mut self, error: automation::Error) -> Failure {
        match error.data {
            Some(code) => Failure::new(ErrorStatus::from(code), error.message),
            None if self.machopo.has_crashed(&self.window) => Failure::tab_crashed(),
            None => Failure::new(ErrorStatus::UnknownError, error.message),
        }
    }

    /// Execute Script, or Execute Async Script: its value, once it
    /// settles; `script timeout` if that is after the script timeout.
    fn execute(&mut self, params: JavascriptCommandParameters, is_async: bool) -> Result<Value> {
        let deadline = self.timeouts.script.and_then(after_ms);
        let method = Method::MachopoExecuteScript(ExecuteScript {
            frame: self.frame(),
            script: params.script,
            args: params.args.unwrap_or_default(),
            is_async,
        });
        self.webdriver_until(method, deadline)?.ok_or_else(|| {
            Failure::new(
                ErrorStatus::ScriptTimeout,
                format!(
                    "the script didn't settle in {} ms",
                    self.timeouts.script.unwrap_or_default()
                ),
            )
        })
    }

    /// Find Element(s) (`first`: one), from the document or `element`:
    /// tried again until something is found or the implicit wait is over.
    fn find(
        &mut self,
        params: LocatorParameters,
        element: Option<String>,
        first: bool,
    ) -> Result<Value> {
        let using = serde_json::to_value(params.using).expect("a strategy serializes");
        let method = Method::MachopoFindElements(FindElements {
            frame: self.frame(),
            using: using.as_str().unwrap_or_default().to_owned(),
            value: params.value,
            element,
        });
        let deadline = self.timeouts.implicit.and_then(after_ms);
        loop {
            let found = self.webdriver(method.clone())?;
            let empty = found.as_array().is_none_or(Vec::is_empty);
            if !empty || deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                if !first {
                    return Ok(found);
                }
                return found
                    .get(0)
                    .cloned()
                    .ok_or_else(|| Failure::new(ErrorStatus::NoSuchElement, "nothing was found"));
            }
            std::thread::sleep(FIND_INTERVAL);
        }
    }

    /// When a navigation started now gives up, if it does.
    fn deadline(&self) -> Option<Instant> {
        self.timeouts.page_load.and_then(after_ms)
    }

    fn timed_out(&self) -> Failure {
        Failure::new(
            ErrorStatus::Timeout,
            format!(
                "navigation timed out after {} ms",
                self.timeouts.page_load.unwrap_or_default()
            ),
        )
    }

    /// WebDriver's Navigate To.
    fn navigate(&mut self, url: &str) -> Result<()> {
        // The window first: wdspec's `test_no_top_browsing_context`.
        self.window_info()?;
        if url::Url::parse(url).is_err() {
            return Err(Failure::new(
                ErrorStatus::InvalidArgument,
                format!("{url} isn't an absolute URL"),
            ));
        }
        self.frames.clear();
        let deadline = self.deadline();
        self.machopo.forget_events();
        let result = self.machopo.call_ok(
            Some(&self.page()),
            Method::PageNavigate(Navigate {
                url: url.to_owned(),
            }),
        )?;
        // A load that failed (an error page commits): an error, as
        // chromedriver answers.
        if let Some(error) = result["errorText"].as_str() {
            return Err(Failure::new(ErrorStatus::UnknownError, error));
        }
        let loader = result["loaderId"].as_str().unwrap_or_default().to_owned();
        self.wait_for_load(&loader, deadline)
    }

    /// Back (`delta` -1) or forward (1) in the window's history, waiting
    /// for the document it shows; nothing if there is no such entry.
    fn traverse(&mut self, delta: i64) -> Result<()> {
        self.window_info()?;
        self.frames.clear();
        let history = self
            .machopo
            .call_ok(Some(&self.page()), Method::PageGetNavigationHistory)?;
        let current = history["currentIndex"].as_i64().unwrap_or(0);
        let index = current
            .checked_add(delta)
            .and_then(|index| usize::try_from(index).ok());
        let Some(entry) = index.and_then(|index| history["entries"].get(index)) else {
            return Ok(());
        };
        let entry_id = entry["id"].as_u64().unwrap_or_default();
        let deadline = self.deadline();
        self.machopo.forget_events();
        self.machopo.call_ok(
            Some(&self.page()),
            Method::PageNavigateToHistoryEntry(NavigateToHistoryEntry { entry_id }),
        )?;
        self.wait_for_navigation(deadline)
    }

    fn refresh(&mut self) -> Result<()> {
        self.window_info()?;
        self.frames.clear();
        let deadline = self.deadline();
        self.machopo.forget_events();
        self.machopo
            .call_ok(Some(&self.page()), Method::PageReload(Reload::default()))?;
        self.wait_for_navigation(deadline)
    }

    /// After a command that may load a document or navigate within the
    /// one shown: waits for whichever it did.
    fn wait_for_navigation(&mut self, deadline: Option<Instant>) -> Result<()> {
        if self.page_load_strategy == PageLoadStrategy::None {
            return Ok(());
        }
        let window = self.window.clone();
        let started = self
            .machopo
            .wait_for(deadline, |event| match &event.event {
                EventKind::FrameStartedNavigating {
                    frame_id,
                    loader_id,
                    ..
                } if *frame_id == window => Some(Ok(Some(loader_id.clone()))),
                EventKind::NavigatedWithinDocument { frame_id, .. } if *frame_id == window => {
                    Some(Ok(None))
                }
                EventKind::TargetCrashed { target_id, .. } if *target_id == window => {
                    Some(Err(Failure::tab_crashed()))
                }
                _ => None,
            })?;
        match started {
            None => Err(self.timed_out()),
            Some(Err(failure)) => Err(failure),
            Some(Ok(None)) => Ok(()),
            Some(Ok(Some(loader))) => self.wait_for_load(&loader, deadline),
        }
    }

    /// Waits for `loader`'s document as far as the page load strategy
    /// says: `DOMContentLoaded` (`eager`), or `load` and the frame that
    /// shows it drawn (`normal`), so what is asked next (a screenshot) sees
    /// it.
    fn wait_for_load(&mut self, loader: &str, deadline: Option<Instant>) -> Result<()> {
        let wanted = match self.page_load_strategy {
            PageLoadStrategy::None => return Ok(()),
            PageLoadStrategy::Eager => ["DOMContentLoaded", "load"].as_slice(),
            PageLoadStrategy::Normal => ["load"].as_slice(),
        };
        let window = self.window.clone();
        let reached = self
            .machopo
            .wait_for(deadline, |event| match &event.event {
                // Loader ids are each tab's own.
                EventKind::LifecycleEvent {
                    frame_id,
                    loader_id,
                    name,
                    ..
                } if *frame_id == window
                    && loader_id == loader
                    && wanted.contains(&name.as_str()) =>
                {
                    Some(Ok(()))
                }
                EventKind::TargetCrashed { target_id, .. } if *target_id == window => {
                    Some(Err(Failure::tab_crashed()))
                }
                _ => None,
            })?;
        match reached {
            None => Err(self.timed_out()),
            Some(Err(failure)) => Err(failure),
            Some(Ok(())) if self.page_load_strategy == PageLoadStrategy::Normal => {
                let page = self.page();
                self.machopo
                    .call_ok(Some(&page), Method::MachopoWaitForLoad)
                    .map(drop)
            }
            Some(Ok(())) => Ok(()),
        }
    }
}

/// The session's window, for its actions.
struct WindowInput<'a> {
    session: &'a mut Session,
}

impl Input for WindowInput<'_> {
    fn key(&mut self, event: DispatchKeyEvent) -> Result<()> {
        let page = self.session.page();
        self.session
            .machopo
            .call_ok(Some(&page), Method::InputDispatchKeyEvent(event))
            .map(drop)
    }

    fn mouse(&mut self, event: DispatchMouseEvent) -> Result<()> {
        let page = self.session.page();
        self.session
            .machopo
            .call_ok(Some(&page), Method::InputDispatchMouseEvent(event))
            .map(drop)
    }

    fn element_point(&mut self, element: &str) -> Result<(f64, f64)> {
        let frame = self.session.frame();
        let point = self.session.webdriver(Method::MachopoGetElementClickPoint(
            ElementClickPointParams {
                frame,
                element: element.to_owned(),
                in_view_center_only: true,
            },
        ))?;
        let at = |key: &str| point[key].as_f64().unwrap_or_default();
        Ok((at("x"), at("y")))
    }

    fn viewport(&mut self) -> Result<Viewport> {
        let frame = self.session.frame();
        let port = self
            .session
            .webdriver(Method::MachopoGetFrameViewport(FrameParams { frame }))?;
        let at = |key: &str| port[key].as_f64().unwrap_or_default();
        Ok(Viewport {
            x: at("x"),
            y: at("y"),
            width: at("width"),
            height: at("height"),
        })
    }

    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// What machopo can do, for matching a new session's capabilities.
struct MachopoCapabilities;

impl BrowserCapabilities for MachopoCapabilities {
    fn init(&mut self, _: &Capabilities) {}

    fn browser_name(&mut self, _: &Capabilities) -> WebDriverResult<Option<String>> {
        Ok(Some("machopo".into()))
    }

    fn browser_version(&mut self, _: &Capabilities) -> WebDriverResult<Option<String>> {
        Ok(Some(env!("CARGO_PKG_VERSION").into()))
    }

    fn compare_browser_version(
        &mut self,
        version: &str,
        comparison: &str,
    ) -> WebDriverResult<bool> {
        Ok(version == comparison)
    }

    fn platform_name(&mut self, _: &Capabilities) -> WebDriverResult<Option<String>> {
        Ok(Some(platform_name().into()))
    }

    /// machopo can't be told to accept a certificate it doesn't trust.
    fn accept_insecure_certs(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn set_window_rect(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(true)
    }

    fn strict_file_interactability(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(true)
    }

    /// No WebDriver BiDi.
    fn web_socket_url(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn webauthn_virtual_authenticators(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn webauthn_extension_uvm(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn webauthn_extension_prf(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn webauthn_extension_large_blob(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn webauthn_extension_cred_blob(&mut self, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    /// machopo has no proxies.
    fn accept_proxy(&mut self, _: &Map<String, Value>, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(false)
    }

    fn validate_custom(&mut self, _: &str, _: &Value) -> WebDriverResult<()> {
        Ok(())
    }

    /// Other browsers' (`goog:chromeOptions`...) are no concern of machopo's.
    fn accept_custom(&mut self, _: &str, _: &Value, _: &Capabilities) -> WebDriverResult<bool> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_look_like_uuids_and_differ() {
        let (a, b) = (session_id(1), session_id(1));
        assert_ne!(a, b);
        let parts: Vec<usize> = a.split('-').map(str::len).collect();
        assert_eq!(parts, [8, 4, 4, 4, 12]);
        assert_eq!(a.as_bytes()[14], b'4');
    }
}
