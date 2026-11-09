//! Automation <-> Browser: what drives a browser from outside it (machopo's
//! WebDriver driver, `machopodriver`, through `machopo --automation-pipe`), shaped
//! as the Chrome DevTools Protocol (Chromium's `browser_protocol.pdl`).
//!
//! The envelope is CDP's: a command is `{id, method, params, sessionId}`,
//! answered by `{id, result}` or `{id, error: {code, message, data}}`
//! (`data` only when there is one: a WebDriver error's code); an event
//! is `{method, params, sessionId}`. A method CDP has keeps CDP's name,
//! parameters and meaning; a parameter machopo doesn't take is
//! `INVALID_PARAMS`, never ignored. What CDP has no method for is in machopo's
//! own `Machopo` domain. On the pipe each message is followed by a NUL, as on
//! Chromium's `--remote-debugging-pipe`.
//!
//! A page's methods go to a session (`sessionId`), which
//! `Target.attachToTarget` makes for a tab (CDP's flat sessions); the
//! browser's own (`Browser.*`, `Target.*`) have none, nor do the `Machopo`
//! domain's that act on the window and its tab shown, as a person would
//! (the scenarios'; Chromium's interactive_ui_tests drive the window with
//! `ui_controls` rather than CDP). The `Machopo` domain's WebDriver methods
//! (`Machopo.executeScript`, `Machopo.findElements`, `Machopo.getElement*`,
//! `Machopo.switchToFrame`, `Machopo.captureElementScreenshot`,
//! `Machopo.getFrameViewport`, `Machopo.prepareSendKeys`, `Machopo.clearElement`) are a
//! page's: chromedriver's
//! `Runtime.callFunctionOn` goes to a session too. Each may name a child
//! frame of the page to work in, by the id `Machopo.switchToFrame` gave.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::webdriver::WebDriverError;

/// JSON-RPC's "invalid request": a message that isn't a command (no `id`,
/// no `method`, a property a command doesn't have).
pub const INVALID_REQUEST: i64 = -32600;
/// JSON-RPC's "method not found": a method machopo doesn't have.
pub const METHOD_NOT_FOUND: i64 = -32601;
/// JSON-RPC's "invalid params": a parameter missing, of the wrong type, or
/// one machopo doesn't take.
pub const INVALID_PARAMS: i64 = -32602;
/// CDP's "server error": the method failed (no such target, history
/// entry...).
pub const SERVER_ERROR: i64 = -32000;
/// CDP's "session with given id not found".
pub const SESSION_NOT_FOUND: i64 = -32001;

/// A command's error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error {
    pub code: i64,
    pub message: String,
    /// CDP's `data`: a WebDriver error's code (`stale element reference`),
    /// for the `Machopo` domain's WebDriver methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
}

impl Error {
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl From<WebDriverError> for Error {
    /// A server error, with the WebDriver error's code as its `data`.
    fn from(error: WebDriverError) -> Self {
        Self {
            code: SERVER_ERROR,
            message: error.message,
            data: Some(error.kind.code().to_owned()),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message, self.code)
    }
}

impl std::error::Error for Error {}

/// A command: a method, for a session (`None`: the browser's). Its `id`
/// is an `int`, as Chrome's dispatcher (`crdtp`) takes it, and so V8's
/// inspector, which answers a page's methods, does.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub id: i32,
    pub session_id: Option<String>,
    pub method: Method,
}

/// The methods machopo has, with their parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum Method {
    /// `Browser.getVersion`.
    BrowserGetVersion,
    /// `SystemInfo.getInfo`: the GPU the browser draws with.
    SystemInfoGetInfo,
    /// `Browser.close`: answered, then the browser ends.
    BrowserClose,
    /// `Target.getTargets`: the tabs.
    TargetGetTargets,
    /// `Target.getTargetInfo`: a tab's, or with no `targetId` the
    /// session's.
    TargetGetTargetInfo(GetTargetInfo),
    /// `Target.attachToTarget`: a session for a tab. Only flat sessions.
    TargetAttachToTarget(AttachToTarget),
    /// `Target.setDiscoverTargets`: whether to send `Target.targetCreated`,
    /// `targetDestroyed` and `targetCrashed`.
    TargetSetDiscoverTargets(SetDiscoverTargets),
    /// `Target.createTarget`: a new tab after the others, in machopo's one
    /// window; `{targetId}`.
    TargetCreateTarget(CreateTarget),
    /// `Target.closeTarget`: closes the tab (the last one closes the
    /// window, and the browser ends); `{success: true}`.
    TargetCloseTarget(TargetParams),
    /// `Target.activateTarget`: shows the tab.
    TargetActivateTarget(TargetParams),
    /// `Target.setAutoAttach`: answered; machopo has no target to attach to on
    /// its own (no out-of-process iframes, no workers).
    TargetSetAutoAttach(SetAutoAttach),
    /// `Target.detachFromTarget`: the client's session goes
    /// (`Target.detachedFromTarget`).
    TargetDetachFromTarget(DetachFromTarget),
    /// `Page.enable`: the session gets `Page.frameStartedNavigating` and
    /// `Page.navigatedWithinDocument`.
    PageEnable,
    /// `Page.setLifecycleEventsEnabled`: the session gets
    /// `Page.lifecycleEvent`.
    PageSetLifecycleEventsEnabled(SetLifecycleEventsEnabled),
    /// `Page.navigate`: starts a navigation; answered at once, with its
    /// loader.
    PageNavigate(Navigate),
    /// `Page.reload`.
    PageReload(Reload),
    /// `Page.getNavigationHistory`.
    PageGetNavigationHistory,
    /// `Page.navigateToHistoryEntry`.
    PageNavigateToHistoryEntry(NavigateToHistoryEntry),
    /// `Page.captureScreenshot`: the page's view as the next frame that
    /// shows it draws it, physical px; `{data}`, a PNG in base64. Only
    /// `format: "png"`; the tab must be shown.
    PageCaptureScreenshot(CaptureScreenshot),
    /// `Input.dispatchMouseEvent`: the pointer's move, a button's press or
    /// release, or a wheel, at `x`, `y` in the page's CSS px, straight to the tab's
    /// renderer (DevTools' `InputHandler`, not through the window);
    /// answered once the renderer has handled it.
    InputDispatchMouseEvent(DispatchMouseEvent),
    /// `Input.dispatchKeyEvent`: a key's press or release, straight to the
    /// tab's renderer, as `Input.dispatchMouseEvent`; answered once the
    /// renderer has handled it.
    InputDispatchKeyEvent(DispatchKeyEvent),
    /// `Browser.getWindowForTarget`: the window (machopo has one) and its
    /// bounds; `width` and `height` are its inner size, CSS px.
    BrowserGetWindowForTarget(GetTargetInfo),
    /// `Browser.setWindowBounds`: the window's position and size, CSS px;
    /// answered once the window has them.
    BrowserSetWindowBounds(SetWindowBounds),
    /// `Machopo.navigateFromAddressBar`: as if typed in the address field, in
    /// the tab shown.
    MachopoNavigateFromAddressBar(Navigate),
    /// `Machopo.waitForLoad`: answered once the latest navigation that loads a
    /// document, of the session's tab (without one, the tab shown), has
    /// loaded and the frame showing it is drawn (`DidFinishLoad`).
    MachopoWaitForLoad,
    /// `Machopo.dispatchWindowInput`: input to the window as the platform
    /// gives it (the tab strip, the toolbar, the page: wherever it lands),
    /// answered once the window has it.
    MachopoDispatchWindowInput(WindowInput),
    /// `Machopo.queryElement`: the first element matching `selector` in the tab
    /// shown (in the child frames `frames` pick, iframe by iframe); its box
    /// in the window's CSS px and its `textContent`.
    MachopoQueryElement(QueryElement),
    /// `Machopo.scrollIntoViewIfNeeded`: scrolls the element into view if its
    /// center is out of it; answered once a frame showing that is drawn.
    MachopoScrollIntoViewIfNeeded(QueryElement),
    /// `Machopo.getTitle`: the tab shown's title, as its tab shows it;
    /// `{title}`.
    MachopoGetTitle,
    /// `Machopo.getCursor`: what the pointer looks like where it is over the
    /// window, as the CSS keyword for it (`default` for the arrow,
    /// `pointer` for the hand, `text` for the I-beam); `{cursor}`.
    MachopoGetCursor,
    /// `Machopo.waitForInputHandled`: answered once the tab shown has handled
    /// every input event given it (with the frame showing what each did).
    MachopoWaitForInputHandled,
    /// `Machopo.captureWindow`: the whole window as Viz draws it next (`now`),
    /// or once it shows what the tab shown has changed so far.
    MachopoCaptureWindow(CaptureWindow),
    /// `Machopo.executeScript`: WebDriver's Execute (Async) Script in the
    /// session's tab; `{value}` once the script settles.
    MachopoExecuteScript(ExecuteScript),
    /// `Machopo.findElements`: WebDriver's Find Elements (From Element);
    /// `{value: [references]}`.
    MachopoFindElements(FindElements),
    /// `Machopo.getElementRect`: `{value: {x, y, width, height}}`.
    MachopoGetElementRect(ElementParams),
    /// `Machopo.getElementTagName`: `{value: tagName}`.
    MachopoGetElementTagName(ElementParams),
    /// `Machopo.getElementAttribute`: `{value: string or null}`.
    MachopoGetElementAttribute(NamedElementParams),
    /// `Machopo.getElementProperty`: `{value}`.
    MachopoGetElementProperty(NamedElementParams),
    /// `Machopo.getElementClickPoint`: Element Click's work before the click
    /// (the element scrolled into view); `{value: {x, y}}`, where the
    /// pointer goes, in the page's CSS px. With `inViewCenterOnly`,
    /// Perform Actions' element origin: the point only, nothing scrolled
    /// or checked.
    MachopoGetElementClickPoint(ElementClickPointParams),
    /// `Machopo.captureElementScreenshot`: Take Element Screenshot (the
    /// element scrolled into view, then the part of it in view as the next
    /// frame draws it); `{value: data}`, a PNG in base64.
    MachopoCaptureElementScreenshot(ElementParams),
    /// `Machopo.getFrameViewport`: Perform Actions' viewport, `frame`'s (the
    /// top-level one if none); `{value: {x, y, width, height}}`, where it
    /// is in the top-level viewport and its size, CSS px.
    MachopoGetFrameViewport(FrameParams),
    /// `Machopo.prepareSendKeys`: Element Send Keys' work in the page before
    /// the keys (the element focused, or a non-typeable control's value
    /// set); `{value: bool}`, whether the driver types the keys.
    MachopoPrepareSendKeys(SendKeysParams),
    /// `Machopo.clearElement`: Element Clear.
    MachopoClearElement(ElementParams),
    /// `Machopo.switchToFrame`: WebDriver's Switch To Frame by `index` or by
    /// `element` (an iframe's reference) in `frame`; with neither, `frame`
    /// itself, if it is still there. `{value: id}`: the frame's id, `null`
    /// for the top-level one.
    MachopoSwitchToFrame(SwitchToFrame),
    /// A method the browser doesn't have (`Runtime.evaluate`, a page's
    /// methods V8's inspector answers): passed on as it is to the tab of
    /// the command's session, whose renderer answers it, as Chromium's
    /// browser-side handlers let a method fall through to the renderer.
    Other { name: String, params: Value },
}

impl Method {
    /// CDP's name.
    pub fn name(&self) -> &str {
        match self {
            Method::Other { name, .. } => name,
            Method::BrowserGetVersion => "Browser.getVersion",
            Method::SystemInfoGetInfo => "SystemInfo.getInfo",
            Method::BrowserClose => "Browser.close",
            Method::TargetGetTargets => "Target.getTargets",
            Method::TargetGetTargetInfo(_) => "Target.getTargetInfo",
            Method::TargetAttachToTarget(_) => "Target.attachToTarget",
            Method::TargetSetDiscoverTargets(_) => "Target.setDiscoverTargets",
            Method::TargetCreateTarget(_) => "Target.createTarget",
            Method::TargetCloseTarget(_) => "Target.closeTarget",
            Method::TargetActivateTarget(_) => "Target.activateTarget",
            Method::TargetSetAutoAttach(_) => "Target.setAutoAttach",
            Method::TargetDetachFromTarget(_) => "Target.detachFromTarget",
            Method::PageEnable => "Page.enable",
            Method::PageSetLifecycleEventsEnabled(_) => "Page.setLifecycleEventsEnabled",
            Method::PageNavigate(_) => "Page.navigate",
            Method::PageReload(_) => "Page.reload",
            Method::PageGetNavigationHistory => "Page.getNavigationHistory",
            Method::PageNavigateToHistoryEntry(_) => "Page.navigateToHistoryEntry",
            Method::PageCaptureScreenshot(_) => "Page.captureScreenshot",
            Method::InputDispatchMouseEvent(_) => "Input.dispatchMouseEvent",
            Method::InputDispatchKeyEvent(_) => "Input.dispatchKeyEvent",
            Method::BrowserGetWindowForTarget(_) => "Browser.getWindowForTarget",
            Method::BrowserSetWindowBounds(_) => "Browser.setWindowBounds",
            Method::MachopoNavigateFromAddressBar(_) => "Machopo.navigateFromAddressBar",
            Method::MachopoWaitForLoad => "Machopo.waitForLoad",
            Method::MachopoDispatchWindowInput(_) => "Machopo.dispatchWindowInput",
            Method::MachopoQueryElement(_) => "Machopo.queryElement",
            Method::MachopoScrollIntoViewIfNeeded(_) => "Machopo.scrollIntoViewIfNeeded",
            Method::MachopoWaitForInputHandled => "Machopo.waitForInputHandled",
            Method::MachopoGetTitle => "Machopo.getTitle",
            Method::MachopoGetCursor => "Machopo.getCursor",
            Method::MachopoCaptureWindow(_) => "Machopo.captureWindow",
            Method::MachopoExecuteScript(_) => "Machopo.executeScript",
            Method::MachopoFindElements(_) => "Machopo.findElements",
            Method::MachopoGetElementRect(_) => "Machopo.getElementRect",
            Method::MachopoGetElementTagName(_) => "Machopo.getElementTagName",
            Method::MachopoGetElementAttribute(_) => "Machopo.getElementAttribute",
            Method::MachopoGetElementProperty(_) => "Machopo.getElementProperty",
            Method::MachopoSwitchToFrame(_) => "Machopo.switchToFrame",
            Method::MachopoGetElementClickPoint(_) => "Machopo.getElementClickPoint",
            Method::MachopoCaptureElementScreenshot(_) => "Machopo.captureElementScreenshot",
            Method::MachopoGetFrameViewport(_) => "Machopo.getFrameViewport",
            Method::MachopoPrepareSendKeys(_) => "Machopo.prepareSendKeys",
            Method::MachopoClearElement(_) => "Machopo.clearElement",
        }
    }

    fn params(&self) -> Value {
        let value = match self {
            Method::TargetGetTargetInfo(p) => serde_json::to_value(p),
            Method::TargetAttachToTarget(p) => serde_json::to_value(p),
            Method::TargetSetDiscoverTargets(p) => serde_json::to_value(p),
            Method::TargetCreateTarget(p) => serde_json::to_value(p),
            Method::TargetCloseTarget(p) | Method::TargetActivateTarget(p) => {
                serde_json::to_value(p)
            }
            Method::TargetSetAutoAttach(p) => serde_json::to_value(p),
            Method::TargetDetachFromTarget(p) => serde_json::to_value(p),
            Method::PageSetLifecycleEventsEnabled(p) => serde_json::to_value(p),
            Method::PageNavigate(p) => serde_json::to_value(p),
            Method::PageReload(p) => serde_json::to_value(p),
            Method::PageNavigateToHistoryEntry(p) => serde_json::to_value(p),
            Method::PageCaptureScreenshot(p) => serde_json::to_value(p),
            Method::InputDispatchMouseEvent(p) => serde_json::to_value(p),
            Method::InputDispatchKeyEvent(p) => serde_json::to_value(p),
            Method::MachopoGetElementClickPoint(p) => serde_json::to_value(p),
            Method::MachopoGetFrameViewport(p) => serde_json::to_value(p),
            Method::MachopoPrepareSendKeys(p) => serde_json::to_value(p),
            Method::MachopoClearElement(p) => serde_json::to_value(p),
            Method::BrowserGetWindowForTarget(p) => serde_json::to_value(p),
            Method::BrowserSetWindowBounds(p) => serde_json::to_value(p),
            Method::MachopoNavigateFromAddressBar(p) => serde_json::to_value(p),
            Method::MachopoDispatchWindowInput(p) => serde_json::to_value(p),
            Method::MachopoQueryElement(p) | Method::MachopoScrollIntoViewIfNeeded(p) => {
                serde_json::to_value(p)
            }
            Method::MachopoCaptureWindow(p) => serde_json::to_value(p),
            Method::MachopoExecuteScript(p) => serde_json::to_value(p),
            Method::MachopoFindElements(p) => serde_json::to_value(p),
            Method::MachopoGetElementRect(p)
            | Method::MachopoGetElementTagName(p)
            | Method::MachopoCaptureElementScreenshot(p) => serde_json::to_value(p),
            Method::MachopoGetElementAttribute(p) | Method::MachopoGetElementProperty(p) => {
                serde_json::to_value(p)
            }
            Method::MachopoSwitchToFrame(p) => serde_json::to_value(p),
            Method::Other { params, .. } => return params.clone(),
            Method::BrowserGetVersion
            | Method::SystemInfoGetInfo
            | Method::BrowserClose
            | Method::TargetGetTargets
            | Method::PageEnable
            | Method::PageGetNavigationHistory
            | Method::MachopoWaitForLoad
            | Method::MachopoWaitForInputHandled
            | Method::MachopoGetTitle
            | Method::MachopoGetCursor => return Value::Object(Map::new()),
        };
        value.expect("parameters serialize")
    }

    /// The method `name` with `params` (`{}` if the command had none).
    pub fn from_name(name: &str, params: Value) -> Result<Method, Error> {
        Ok(match name {
            "Browser.getVersion" => none(params, Method::BrowserGetVersion)?,
            "SystemInfo.getInfo" => none(params, Method::SystemInfoGetInfo)?,
            "Browser.close" => none(params, Method::BrowserClose)?,
            "Target.getTargets" => none(params, Method::TargetGetTargets)?,
            "Target.getTargetInfo" => Method::TargetGetTargetInfo(typed(params)?),
            "Target.attachToTarget" => Method::TargetAttachToTarget(typed(params)?),
            "Target.setDiscoverTargets" => Method::TargetSetDiscoverTargets(typed(params)?),
            "Target.createTarget" => Method::TargetCreateTarget(typed(params)?),
            "Target.closeTarget" => Method::TargetCloseTarget(typed(params)?),
            "Target.activateTarget" => Method::TargetActivateTarget(typed(params)?),
            "Target.setAutoAttach" => Method::TargetSetAutoAttach(typed(params)?),
            "Target.detachFromTarget" => Method::TargetDetachFromTarget(typed(params)?),
            "Page.enable" => none(params, Method::PageEnable)?,
            "Page.setLifecycleEventsEnabled" => {
                Method::PageSetLifecycleEventsEnabled(typed(params)?)
            }
            "Page.navigate" => Method::PageNavigate(typed(params)?),
            "Page.reload" => Method::PageReload(typed(params)?),
            "Page.getNavigationHistory" => none(params, Method::PageGetNavigationHistory)?,
            "Page.navigateToHistoryEntry" => Method::PageNavigateToHistoryEntry(typed(params)?),
            "Page.captureScreenshot" => Method::PageCaptureScreenshot(typed(params)?),
            "Input.dispatchMouseEvent" => Method::InputDispatchMouseEvent(typed(params)?),
            "Input.dispatchKeyEvent" => Method::InputDispatchKeyEvent(typed(params)?),
            "Browser.getWindowForTarget" => Method::BrowserGetWindowForTarget(typed(params)?),
            "Browser.setWindowBounds" => Method::BrowserSetWindowBounds(typed(params)?),
            "Machopo.navigateFromAddressBar" => {
                Method::MachopoNavigateFromAddressBar(typed(params)?)
            }
            "Machopo.waitForLoad" => none(params, Method::MachopoWaitForLoad)?,
            "Machopo.dispatchWindowInput" => Method::MachopoDispatchWindowInput(typed(params)?),
            "Machopo.queryElement" => Method::MachopoQueryElement(typed(params)?),
            "Machopo.scrollIntoViewIfNeeded" => {
                Method::MachopoScrollIntoViewIfNeeded(typed(params)?)
            }
            "Machopo.waitForInputHandled" => none(params, Method::MachopoWaitForInputHandled)?,
            "Machopo.getTitle" => none(params, Method::MachopoGetTitle)?,
            "Machopo.getCursor" => none(params, Method::MachopoGetCursor)?,
            "Machopo.captureWindow" => Method::MachopoCaptureWindow(typed(params)?),
            "Machopo.executeScript" => Method::MachopoExecuteScript(typed(params)?),
            "Machopo.findElements" => Method::MachopoFindElements(typed(params)?),
            "Machopo.getElementRect" => Method::MachopoGetElementRect(typed(params)?),
            "Machopo.getElementTagName" => Method::MachopoGetElementTagName(typed(params)?),
            "Machopo.getElementAttribute" => Method::MachopoGetElementAttribute(typed(params)?),
            "Machopo.getElementProperty" => Method::MachopoGetElementProperty(typed(params)?),
            "Machopo.switchToFrame" => Method::MachopoSwitchToFrame(typed(params)?),
            "Machopo.getElementClickPoint" => Method::MachopoGetElementClickPoint(typed(params)?),
            "Machopo.captureElementScreenshot" => {
                Method::MachopoCaptureElementScreenshot(typed(params)?)
            }
            "Machopo.getFrameViewport" => Method::MachopoGetFrameViewport(typed(params)?),
            "Machopo.prepareSendKeys" => Method::MachopoPrepareSendKeys(typed(params)?),
            "Machopo.clearElement" => Method::MachopoClearElement(typed(params)?),
            // machopo's own: no one else has one.
            _ if name.starts_with("Machopo.") => {
                return Err(Error::new(
                    METHOD_NOT_FOUND,
                    format!("'{name}' wasn't found"),
                ));
            }
            _ => Method::Other {
                name: name.to_owned(),
                params,
            },
        })
    }
}

/// `params` as `T`: what CDP's dispatcher checks, plus that it has no
/// parameter machopo doesn't take.
fn typed<T: DeserializeOwned>(params: Value) -> Result<T, Error> {
    serde_json::from_value(params)
        .map_err(|e| Error::new(INVALID_PARAMS, format!("Invalid parameters: {e}")))
}

/// A method without parameters: `params` is empty.
fn none(params: Value, method: Method) -> Result<Method, Error> {
    match params {
        Value::Object(map) if map.is_empty() => Ok(method),
        _ => Err(Error::new(
            INVALID_PARAMS,
            format!("Invalid parameters: {} takes none", method.name()),
        )),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GetTargetInfo {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttachToTarget {
    pub target_id: String,
    /// Only `true`: machopo has flat sessions only.
    #[serde(default)]
    pub flatten: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetDiscoverTargets {
    pub discover: bool,
}

/// `Target.createTarget`'s: the URL the tab shows, and whether it opens
/// behind the tab shown. machopo has one window: no `newWindow`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateTarget {
    pub url: String,
    #[serde(default)]
    pub background: bool,
}

/// A target's: its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetParams {
    pub target_id: String,
}

/// `Target.setAutoAttach`'s (its `filter` and the rest, which change
/// nothing in machopo, are taken as they come).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetAutoAttach {
    pub auto_attach: bool,
    pub wait_for_debugger_on_start: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flatten: Option<bool>,
}

/// `Target.detachFromTarget`'s: the session, or (deprecated) its target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetachFromTarget {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetLifecycleEventsEnabled {
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Navigate {
    pub url: String,
}

/// `Page.reload`'s, all optional: DevTools' reload sends `ignoreCache`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Reload {
    /// Whether to skip the cache: machopo has none, so either way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ignore_cache: Option<bool>,
    /// Script to run in the new document before its own: machopo can't (an
    /// error unless empty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_to_evaluate_on_load: Option<String>,
    /// The document to reload, if it is still the one shown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loader_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NavigateToHistoryEntry {
    pub entry_id: u64,
}

/// `Page.captureScreenshot`'s: only PNG (the default).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureScreenshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

/// `Input.dispatchMouseEvent`'s: `type` is `mouseMoved`, `mousePressed`,
/// `mouseReleased` or `mouseWheel`; `button` `left`, `middle` or `right`
/// (`none` only to move); `buttons` the DOM's bits of those down (left 1,
/// right 2, middle 4), `clickCount` the press's count in a row, both 0
/// unless given, as CDP has them; `modifiers` CDP's bits (`ALT`...); a
/// wheel's `deltaX`, `deltaY` in CSS px.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchMouseEvent {
    #[serde(rename = "type")]
    pub kind: String,
    pub x: f32,
    pub y: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub button: Option<String>,
    #[serde(default)]
    pub buttons: u16,
    #[serde(default)]
    pub click_count: u32,
    #[serde(default)]
    pub modifiers: u8,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delta_x: f32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delta_y: f32,
}

fn is_zero(value: &f32) -> bool {
    *value == 0.0
}

/// `Input.dispatchKeyEvent`'s: `type` is `keyDown` (with `text`, what the
/// key types), `rawKeyDown` (types nothing) or `keyUp`; `key` and `code`
/// the DOM's, `windowsVirtualKeyCode` the legacy `keyCode`, `location` the
/// DOM's (0 standard, 1 left, 2 right, 3 numpad), `autoRepeat` a held key's
/// repeat; `modifiers` CDP's bits (`ALT`...), as CDP has them. machopo has no
/// `char` events.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DispatchKeyEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub modifiers: u8,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(default)]
    pub windows_virtual_key_code: u16,
    #[serde(default)]
    pub location: u8,
    #[serde(default)]
    pub auto_repeat: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SetWindowBounds {
    pub window_id: u64,
    pub bounds: Bounds,
}

/// `Browser.Bounds`: the window's, CSS px; what isn't given stays. machopo's
/// window has no state of its own to set (`windowState`): that is
/// `INVALID_PARAMS` when given.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Bounds {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
}

/// `Machopo.dispatchWindowInput`'s: `type` is `move` (the pointer to `x`,
/// `y`), `click` (the primary button down and up there, the first click of
/// a series), `dblclick` (two clicks there at once, the second the double
/// click), `wheel` (there, scrolling by `deltaX`, `deltaY` CSS px) or
/// `key`; CSS px of the window. A key is named as
/// winit's `NamedKey` (`Enter`, `ArrowUp`, `F5`...) or is a character;
/// `modifiers` is CDP's bit field (Alt 1, Ctrl 2, Meta 4, Shift 8); `text`
/// is what it types, if anything.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WindowInput {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
    #[serde(default)]
    pub delta_x: f32,
    #[serde(default)]
    pub delta_y: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default)]
    pub modifiers: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

/// CDP's modifier bits.
pub const ALT: u8 = 1;
pub const CTRL: u8 = 2;
pub const META: u8 = 4;
pub const SHIFT: u8 = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryElement {
    pub selector: String,
    /// The iframes, one in each child frame from the document shown,
    /// whose document `selector` picks in.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frames: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CaptureWindow {
    #[serde(default)]
    pub now: bool,
}

/// `Machopo.executeScript`'s: WebDriver's `script` and `args` (its JSON, with
/// references), and whether it is Execute Async Script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecuteScript {
    /// The frame it runs in (`Machopo.switchToFrame`'s id); the top-level one
    /// if none. So for the `Machopo` domain's other WebDriver methods.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    pub script: String,
    #[serde(default)]
    pub args: Vec<Value>,
    #[serde(default, rename = "async")]
    pub is_async: bool,
}

/// `Machopo.findElements`'s: WebDriver's location strategy (`using`) and
/// selector (`value`), from the document or from `element`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FindElements {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    pub using: String,
    pub value: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
}

/// An element's: its reference's id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    pub element: String,
}

/// A frame's: its id (the top-level one if none).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FrameParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
}

/// `Machopo.getElementClickPoint`'s: the element, and whether only its in-view
/// center point is wanted (Perform Actions' element origin).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ElementClickPointParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    pub element: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub in_view_center_only: bool,
}

/// An element's attribute's or property's: its reference's id, and the
/// name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NamedElementParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    pub element: String,
    pub name: String,
}

/// `Machopo.prepareSendKeys`'s: the element, and the text to send it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SendKeysParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    pub element: String,
    pub text: String,
}

/// `Machopo.switchToFrame`'s: in `frame` (the top-level one if none), the
/// child frame `window[index]` is, or the one of the iframe `element`
/// references; at most one of them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SwitchToFrame {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub element: Option<String>,
}

/// A rectangle, CSS px of the window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// `Machopo.queryElement`'s element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueriedElement {
    /// `None` while it has no box.
    pub rect: Option<Rect>,
    pub text: String,
}

/// `Machopo.queryElement`'s result: the element (`None` if nothing matched),
/// where the page is in the window, and the window's scale.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueryResult {
    pub element: Option<QueriedElement>,
    pub page: Option<Rect>,
    pub scale: f32,
}

/// `Machopo.captureWindow`'s result: the frame's size in physical px and its
/// pixels, RGBA, premultiplied, row by row, in base64 (CDP's binary data
/// is base64 strings).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureResult {
    pub width: u32,
    pub height: u32,
    pub data: String,
}

/// `Target.TargetInfo`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetInfo {
    pub target_id: String,
    /// Always `"page"`: a tab.
    #[serde(rename = "type")]
    pub kind: String,
    pub title: String,
    pub url: String,
    /// Whether a session is attached to it.
    pub attached: bool,
    pub can_access_opener: bool,
}

/// `Page.navigate`'s result, as the navigation is about to commit:
/// `error_text` (`net::ERR_…`) if it failed (an error page commits).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateResult {
    pub frame_id: String,
    pub loader_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_text: Option<String>,
    /// Always false: machopo downloads nothing.
    #[serde(default)]
    pub is_download: bool,
}

/// `Page.NavigationEntry`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEntry {
    pub id: u64,
    pub url: String,
    #[serde(rename = "userTypedURL")]
    pub user_typed_url: String,
    pub title: String,
    pub transition_type: String,
}

/// `Page.getNavigationHistory`'s result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationHistory {
    pub current_index: u64,
    pub entries: Vec<NavigationEntry>,
}

/// An answer to a command.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub id: i32,
    pub session_id: Option<String>,
    pub result: Result<Value, Error>,
}

/// What the browser sends of itself, to a session or (`None`) to the
/// browser's.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub session_id: Option<String>,
    pub event: EventKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EventKind {
    /// `Target.targetCreated`.
    TargetCreated { target_info: TargetInfo },
    /// `Target.targetDestroyed`.
    TargetDestroyed { target_id: String },
    /// `Target.targetCrashed`: the tab's renderer has gone (the tab stays,
    /// showing the sad tab).
    TargetCrashed {
        target_id: String,
        status: String,
        error_code: i64,
    },
    /// `Page.frameStartedNavigating`: a navigation that loads a document
    /// started, whatever started it; `loader_id` is its.
    FrameStartedNavigating {
        frame_id: String,
        url: String,
        loader_id: String,
        navigation_type: String,
    },
    /// `Page.navigatedWithinDocument`.
    NavigatedWithinDocument {
        frame_id: String,
        url: String,
        navigation_type: String,
    },
    /// `Page.lifecycleEvent`: `name` is the step the document reached (`init`, `DOMContentLoaded`, `load`, `networkIdle`…);
    /// `timestamp` in seconds on a monotonic clock.
    LifecycleEvent {
        frame_id: String,
        loader_id: String,
        name: String,
        timestamp: f64,
    },
    /// `Inspector.detached`: the target a client connected straight to
    /// (DevTools' `/devtools/page/<id>`) has gone; `reason` is
    /// `target_closed`. The connection closes after it.
    InspectorDetached { reason: String },
}

impl EventKind {
    pub fn name(&self) -> &'static str {
        match self {
            EventKind::TargetCreated { .. } => "Target.targetCreated",
            EventKind::TargetDestroyed { .. } => "Target.targetDestroyed",
            EventKind::TargetCrashed { .. } => "Target.targetCrashed",
            EventKind::FrameStartedNavigating { .. } => "Page.frameStartedNavigating",
            EventKind::NavigatedWithinDocument { .. } => "Page.navigatedWithinDocument",
            EventKind::LifecycleEvent { .. } => "Page.lifecycleEvent",
            EventKind::InspectorDetached { .. } => "Inspector.detached",
        }
    }

    fn params(&self) -> Value {
        match self {
            EventKind::TargetCreated { target_info } => json!({ "targetInfo": target_info }),
            EventKind::TargetDestroyed { target_id } => json!({ "targetId": target_id }),
            EventKind::TargetCrashed {
                target_id,
                status,
                error_code,
            } => json!({ "targetId": target_id, "status": status, "errorCode": error_code }),
            EventKind::FrameStartedNavigating {
                frame_id,
                url,
                loader_id,
                navigation_type,
            } => json!({
                "frameId": frame_id,
                "url": url,
                "loaderId": loader_id,
                "navigationType": navigation_type,
            }),
            EventKind::NavigatedWithinDocument {
                frame_id,
                url,
                navigation_type,
            } => json!({ "frameId": frame_id, "url": url, "navigationType": navigation_type }),
            EventKind::LifecycleEvent {
                frame_id,
                loader_id,
                name,
                timestamp,
            } => json!({
                "frameId": frame_id,
                "loaderId": loader_id,
                "name": name,
                "timestamp": timestamp,
            }),
            EventKind::InspectorDetached { reason } => json!({ "reason": reason }),
        }
    }

    fn from_name(name: &str, params: &Value) -> Option<EventKind> {
        let s = |key: &str| params.get(key)?.as_str().map(str::to_owned);
        Some(match name {
            "Target.targetCreated" => EventKind::TargetCreated {
                target_info: serde_json::from_value(params.get("targetInfo")?.clone()).ok()?,
            },
            "Target.targetDestroyed" => EventKind::TargetDestroyed {
                target_id: s("targetId")?,
            },
            "Target.targetCrashed" => EventKind::TargetCrashed {
                target_id: s("targetId")?,
                status: s("status")?,
                error_code: params.get("errorCode")?.as_i64()?,
            },
            "Page.frameStartedNavigating" => EventKind::FrameStartedNavigating {
                frame_id: s("frameId")?,
                url: s("url")?,
                loader_id: s("loaderId")?,
                navigation_type: s("navigationType")?,
            },
            "Page.navigatedWithinDocument" => EventKind::NavigatedWithinDocument {
                frame_id: s("frameId")?,
                url: s("url")?,
                navigation_type: s("navigationType")?,
            },
            "Page.lifecycleEvent" => EventKind::LifecycleEvent {
                frame_id: s("frameId")?,
                loader_id: s("loaderId")?,
                name: s("name")?,
                timestamp: params.get("timestamp")?.as_f64()?,
            },
            "Inspector.detached" => EventKind::InspectorDetached {
                reason: s("reason")?,
            },
            _ => return None,
        })
    }
}

/// What comes from the browser: an answer or an event.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Response(Response),
    Event(Event),
    /// One the browser passes on as it is, with `session_id` added: a
    /// page's answer or event from its renderer (V8's inspector's), or an
    /// error with no command to answer (`{"error": …}`, as Chrome sends one
    /// for a message whose `id` isn't an `int`).
    Raw {
        session_id: Option<String>,
        object: Map<String, Value>,
    },
}

/// A command's `id` as Chrome's dispatcher reads it: an integer in `int`'s
/// range, negative ones and `1e3` too; `2147483648` and `1.5` aren't one.
fn command_id(value: &Value) -> Option<i32> {
    let Value::Number(number) = value else {
        return None;
    };
    if let Some(id) = number.as_i64() {
        return i32::try_from(id).ok();
    }
    let id = number.as_f64()?;
    let in_range = id.fract() == 0.0 && (f64::from(i32::MIN)..=f64::from(i32::MAX)).contains(&id);
    in_range.then(|| base::numerics::clamp_round::<i32>(id))
}

fn with_session(mut object: Map<String, Value>, session_id: &Option<String>) -> Value {
    if let Some(session_id) = session_id {
        object.insert("sessionId".into(), Value::String(session_id.clone()));
    }
    Value::Object(object)
}

impl Command {
    /// The command as CDP's JSON.
    pub fn to_json(&self) -> String {
        let mut object = Map::new();
        object.insert("id".into(), self.id.into());
        object.insert("method".into(), self.method.name().into());
        object.insert("params".into(), self.method.params());
        with_session(object, &self.session_id).to_string()
    }

    /// A command from CDP's JSON, checked as CDP's dispatcher (`crdtp`)
    /// does. The error carries the command's id when it has one, for the
    /// answer.
    pub fn from_json(text: &str) -> Result<Command, (Option<i32>, Error)> {
        let value: Value = serde_json::from_str(text).map_err(|e| {
            (
                None,
                Error::new(INVALID_REQUEST, format!("Message must be JSON: {e}")),
            )
        })?;
        let Value::Object(object) = value else {
            return Err((
                None,
                Error::new(INVALID_REQUEST, "Message must be an object"),
            ));
        };
        let id = object.get("id").and_then(command_id);
        let fail = |message: &str| (id, Error::new(INVALID_REQUEST, message));
        let Some(id) = id else {
            return Err(fail("Message must have integer 'id' property"));
        };
        let Some(name) = object.get("method").and_then(Value::as_str) else {
            return Err(fail("Message must have string 'method' property"));
        };
        if object
            .keys()
            .any(|key| !matches!(key.as_str(), "id" | "method" | "params" | "sessionId"))
        {
            return Err(fail(
                "Message has property other than 'id', 'method', 'sessionId', 'params'",
            ));
        }
        let session_id = match object.get("sessionId") {
            None => None,
            Some(Value::String(s)) => Some(s.clone()),
            Some(_) => {
                return Err(fail(
                    "Message has property 'sessionId', but it's not a string",
                ));
            }
        };
        let params = match object.get("params") {
            None => Value::Object(Map::new()),
            Some(params @ Value::Object(_)) => params.clone(),
            Some(_) => {
                return Err((
                    Some(id),
                    Error::new(
                        INVALID_PARAMS,
                        "Message has property 'params', but it's not an object",
                    ),
                ));
            }
        };
        let method = Method::from_name(name, params).map_err(|e| (Some(id), e))?;
        Ok(Command {
            id,
            session_id,
            method,
        })
    }
}

impl Message {
    /// The message as CDP's JSON.
    pub fn to_json(&self) -> String {
        let mut object = Map::new();
        match self {
            Message::Response(response) => {
                object.insert("id".into(), response.id.into());
                match &response.result {
                    Ok(result) => object.insert("result".into(), result.clone()),
                    Err(error) => object.insert(
                        "error".into(),
                        serde_json::to_value(error).expect("an error serializes"),
                    ),
                };
                with_session(object, &response.session_id).to_string()
            }
            Message::Event(event) => {
                object.insert("method".into(), event.event.name().into());
                object.insert("params".into(), event.event.params());
                with_session(object, &event.session_id).to_string()
            }
            Message::Raw { session_id, object } => {
                with_session(object.clone(), session_id).to_string()
            }
        }
    }

    /// An error with no command to answer: Chrome's answer to a message
    /// that isn't a command it can tell the id of.
    pub fn error_without_id(error: Error) -> Self {
        let mut object = Map::new();
        object.insert(
            "error".into(),
            serde_json::to_value(error).expect("an error serializes"),
        );
        Message::Raw {
            session_id: None,
            object,
        }
    }

    /// A message from CDP's JSON: an answer, an event machopo knows, or any
    /// other object as it is (a page's event from V8's inspector, an error
    /// with no id); `None` if it isn't a JSON object.
    pub fn from_json(text: &str) -> Option<Message> {
        let value: Value = serde_json::from_str(text).ok()?;
        let session_id = value
            .get("sessionId")
            .and_then(Value::as_str)
            .map(str::to_owned);
        if let Some(id) = value.get("id").and_then(command_id) {
            let result = match (value.get("result"), value.get("error")) {
                (Some(result), None) => Ok(result.clone()),
                (None, Some(error)) => Err(serde_json::from_value(error.clone()).ok()?),
                _ => return None,
            };
            return Some(Message::Response(Response {
                id,
                session_id,
                result,
            }));
        }
        let params = value.get("params").cloned().unwrap_or(Value::Null);
        let known = value
            .get("method")
            .and_then(Value::as_str)
            .and_then(|name| EventKind::from_name(name, &params));
        if let Some(event) = known {
            return Some(Message::Event(Event { session_id, event }));
        }
        let Value::Object(mut object) = value else {
            return None;
        };
        object.remove("sessionId");
        Some(Message::Raw { session_id, object })
    }
}
