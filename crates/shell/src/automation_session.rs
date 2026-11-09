//! What drives the browser (machopodriver through `machopo --automation-pipe`, and
//! the scenarios): carries out `protocol::automation`'s commands against a
//! [`BrowserWindow`] and tells of what happens in it, as DevTools'
//! protocol does (Chromium's `DevToolsSession` with the `Target`, `Page`
//! and `Browser` handlers of `content/browser/devtools/protocol`; what they
//! don't answer of a page's goes to its tab's renderer, whose V8 inspector
//! answers, as in Chromium's renderer), plus
//! machopo's `Machopo` domain: the window as a person uses it (Chromium's
//! interactive_ui_tests do that with `ui_controls` and `ui_test_utils`).
//!
//! Its clients are the driver (the pipe's, or the scenario runner's), which
//! a session in automation mode has and whose commands come through
//! [`AutomationSession::command`], and those that connect to
//! `--remote-debugging-port` ([`ClientEvent`]), which can be many at once,
//! as Chromium's `DevToolsAgentHost` has many clients: each has its own
//! page sessions and asks for its own events. The `Machopo` domain is
//! automation mode's, for any client: its window methods take it that no
//! one else uses the window.
//!
//! Each tab is a target, whose id is the one its `WebContents` drew at
//! random when it made the tab, as Chrome's are, so a page can't guess one
//! (`/json/close/<id>` is a GET a page's `<img>` can make), and told its
//! renderer; its main frame's id is the same. A loader is a navigation that loads a document: its id is
//! the tab's navigation id (`WebContents::latest_navigation_id`). The `Machopo`
//! domain acts on the tab shown, but for its WebDriver methods, a page's,
//! which go to the session's tab's renderer (`RendererMsg::WebDriver`),
//! for the frame they name: their values cross as WebDriver's JSON here,
//! the tab's window reference being its target id (machopodriver's window
//! handle), a child frame's its frame id, which the renderer gives.
//!
//! Commands and renderer messages both come on the UI thread. What the
//! session answers and tells goes to the client's outbox: at once, or later, once
//! what a command waits for has happened (a renderer's answer, an input's
//! acknowledgement, a frame drawn). Input to the window goes back to the
//! embedder ([`Input`]), which gives it as the platform would and then
//! answers its [`Reply`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use base::SequenceNumber;
use base::numerics::{clamp_round, f32_from_f64, saturated_cast};
use base64::Engine;
use browser::{ContentsEvent, WebContents};
use crossbeam_channel::Sender;
use gfx::{PointF, RectF};
use ipc::TimeTicks;
use protocol::TabId;
use protocol::automation::{
    self, CaptureResult, Command, DispatchKeyEvent, DispatchMouseEvent, Error, Event, EventKind,
    INVALID_PARAMS, METHOD_NOT_FOUND, Message, Method, NavigateResult, NavigationEntry,
    NavigationHistory, QueriedElement, QueryResult, Rect, Response, SERVER_ERROR,
    SESSION_NOT_FOUND, TargetInfo, WindowInput,
};
use protocol::devtools_network::{request_json, response_json, wall_time};
use protocol::input::{InputEvent, KeyboardEvent, Modifiers, MouseButton, ScrollDelta};
use protocol::network::{RequestDestination, ResourceRequest};
use protocol::renderer::RendererHostMsg;
use protocol::viz::{CopyOutputRequest, CopyOutputResult, DisplayMsg, VizMsg};
use protocol::webdriver::{ErrorKind, Locator, WebDriverCommand, WebDriverError, WebDriverValue};
use serde_json::{Value, json};
use url::Url;
use winit::keyboard::{Key, ModifiersState};

use crate::automation_pipe::Read;
use crate::browser_window::BrowserWindow;

/// machopo's one window (`Browser.getWindowForTarget`).
const WINDOW_ID: u64 = 1;

/// Input for the embedder to give the window, as the platform would; CSS
/// px of the window.
#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    /// The pointer to there.
    Move(PointF),
    /// The primary button down and up, the first click of a series
    /// (whenever the last click was).
    Click(PointF),
    /// Two clicks there, the second the double click.
    DoubleClick(PointF),
    Wheel {
        at: PointF,
        delta: ScrollDelta,
    },
    /// A key with these modifiers held; `text` is what it types.
    Key {
        key: Key,
        modifiers: ModifiersState,
        text: Option<String>,
    },
    /// The window's size, and where it is on the screen if `origin`.
    SetBounds {
        origin: Option<PointF>,
        width: f32,
        height: f32,
    },
}

/// Where a client's answers and events go: its transport's queue (the
/// pipe's writer, a websocket's connection, the scenario runner's channel).
/// `send` says whether the client is still there.
#[derive(Clone)]
pub struct Outbox(Arc<dyn Fn(Message) -> bool + Send + Sync>);

impl Outbox {
    pub fn new(send: impl Fn(Message) -> bool + Send + Sync + 'static) -> Self {
        Self(Arc::new(send))
    }

    pub fn send(&self, message: Message) -> bool {
        (self.0)(message)
    }
}

impl From<Sender<Message>> for Outbox {
    fn from(sender: Sender<Message>) -> Self {
        Self::new(move |message| sender.send(message).is_ok())
    }
}

impl std::fmt::Debug for Outbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Outbox")
    }
}

/// Where a command's answer goes, once it has one.
#[derive(Debug)]
pub struct Reply {
    id: i32,
    session_id: Option<String>,
    outbox: Outbox,
}

impl Reply {
    pub fn answer(self, result: Result<Value, Error>) {
        self.outbox.send(Message::Response(Response {
            id: self.id,
            session_id: self.session_id,
            result,
        }));
    }

    /// The command did what it asked: `{}`.
    pub fn done(self) {
        self.answer(Ok(json!({})));
    }
}

/// A page session (`Target.attachToTarget`): which tab, and which of its
/// events it asked for.
struct Attached {
    /// Whose session it is: only that client's commands use it.
    client: ClientId,
    tab: TabId,
    /// `Page.enable`.
    page: bool,
    /// `Page.setLifecycleEventsEnabled`, which the renderer answers: noted
    /// for a new renderer of the tab.
    lifecycle: bool,
    /// `Network.enable`: the session hears of the tab's navigations' requests.
    network: bool,
    /// `Log.enable`: the session hears of what it asked of the browser's
    /// `Network` that machopo can't do.
    log: bool,
}

/// The largest DevTools message a renderer may send: room for the largest
/// answers (`Debugger.getScriptSource` of a bundle of megabytes, a CPU
/// profile), the size Chromium's IPC takes in one message (mojo's 256 MB).
const MAX_RENDERER_MESSAGE_SIZE: usize = 256 << 20;

/// The methods a renderer takes while its script runs, as soon as it can,
/// rather than once script is done: those of Chromium's
/// `DevToolsSession::ShouldSendOnIO` that V8's inspector answers. None
/// runs page script.
const INTERRUPTING: &[&str] = &[
    "Debugger.getPossibleBreakpoints",
    "Debugger.getScriptSource",
    "Debugger.getStackTrace",
    "Debugger.pause",
    "Debugger.removeBreakpoint",
    "Debugger.resume",
    "Debugger.setBreakpoint",
    "Debugger.setBreakpointByUrl",
    "Debugger.setBreakpointsActive",
    "Runtime.terminateExecution",
];

/// A client of the session (Chromium's `DevToolsAgentHostClient`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ClientId(u64);

/// A client: where its messages go, and the events it asked for.
struct Client {
    outbox: Outbox,
    /// `Target.setDiscoverTargets`.
    discover: bool,
    /// The tabs as last told, for `targetCreated` and `targetDestroyed`.
    told: Vec<TabId>,
    /// The session of the tab the client connected straight to
    /// (`/devtools/page/<id>`): its commands without a session are that
    /// tab's, and what the session sends it goes without the session's id,
    /// as Chrome's per-target connection does.
    page_session: Option<String>,
}

/// What a remote debugging connection asks of the session, from the HTTP
/// server's thread (`devtools_http_handler`), on the UI thread.
pub enum ClientEvent {
    /// A websocket asks to connect: to the browser, or (`page`, a target
    /// id) straight to a tab. `accepted` is told the client's id, or `None`
    /// if there is no such tab.
    Connect {
        outbox: Outbox,
        page: Option<String>,
        accepted: Box<dyn FnOnce(Option<ClientId>) + Send>,
    },
    /// What the client sent: a command, or why it isn't one.
    Command(ClientId, Read),
    /// The connection has closed: its sessions go.
    Disconnect(ClientId),
    /// One of the HTTP endpoints (`/json/...`); `answer` is told.
    Json {
        request: JsonRequest,
        answer: Box<dyn FnOnce(JsonAnswer) + Send>,
    },
}

impl std::fmt::Debug for ClientEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ClientEvent::Connect { page, .. } => {
                f.debug_struct("Connect").field("page", page).finish()
            }
            ClientEvent::Command(client, read) => {
                f.debug_tuple("Command").field(client).field(read).finish()
            }
            ClientEvent::Disconnect(client) => f.debug_tuple("Disconnect").field(client).finish(),
            ClientEvent::Json { request, .. } => {
                f.debug_struct("Json").field("request", request).finish()
            }
        }
    }
}

/// The HTTP endpoints' requests, as Chrome's `DevToolsHttpHandler` has.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonRequest {
    /// `/json/version`.
    Version,
    /// `/json/list`: the tabs.
    List,
    /// `/json/new?<url>`: a new tab, shown.
    New(String),
    /// `/json/activate/<id>`: shows the tab.
    Activate(String),
    /// `/json/close/<id>`.
    Close(String),
}

/// What the session answers a [`JsonRequest`].
#[derive(Debug, Clone, PartialEq)]
pub enum JsonAnswer {
    /// `/json/version`'s: the browser's product and user agent.
    Version {
        product: String,
        user_agent: String,
    },
    Targets(Vec<TargetInfo>),
    Target(TargetInfo),
    Done,
    /// No tab has the id asked for.
    NoTarget,
    Failed(String),
}

/// A capture of a tab's page as a PNG, waiting for the frame that shows
/// what the page has done so far: all of its view (`Page.captureScreenshot`,
/// answered as `{data}`), or the part `clip` of it, its view's CSS px
/// (`Machopo.captureElementScreenshot`, answered as WebDriver's `{value}`).
struct PageCapture {
    tab: TabId,
    reply: Reply,
    clip: Option<RectF>,
}

/// What a command comes to.
enum Dispatched {
    Now(Result<Value, Error>),
    /// Answered once what it waits for happens.
    Later,
    /// Input for the embedder; answered once given.
    Input(Input),
}

pub struct AutomationSession {
    clients: HashMap<ClientId, Client>,
    /// From 1; the driver's is the first.
    client_ids: SequenceNumber<u64>,
    /// The pipe's or the scenario runner's, in automation mode.
    driver: Option<ClientId>,
    /// Automation mode: the `Machopo` domain is there.
    automation: bool,
    sessions: HashMap<String, Attached>,
    /// From 1.
    session_ids: SequenceNumber<u64>,
    /// Each tab's target id, from when the session first saw it.
    targets: HashMap<TabId, String>,
    /// The tabs as last seen.
    tabs: Vec<TabId>,
    /// Each tab's latest loader as last told, for `frameStartedNavigating`.
    loaders: HashMap<TabId, u64>,
    /// What started a tab's next loader, if the session did it
    /// (`frameStartedNavigating`'s `navigationType`).
    starting: HashMap<TabId, &'static str>,
    /// Each tab's URL as last committed, to tell a fragment from the
    /// history API in `navigatedWithinDocument`.
    committed: HashMap<TabId, String>,
    /// The tabs whose renderer has gone, as last told (`targetCrashed`).
    crashed: Vec<TabId>,
    /// Each tab's latest navigation that has loaded (`DidFinishLoad`).
    loaded: HashMap<TabId, u64>,
    /// `Machopo.waitForLoad`s, by tab and the navigation they wait for.
    loads: Vec<(TabId, u64, Reply)>,
    /// The commands the browser sent a new renderer itself, to restore a
    /// session as it was, by session and (negative) id: no client waits for
    /// their answers.
    restoring: HashSet<(String, i64)>,
    /// From 1.
    restore_ids: SequenceNumber<i64>,
    /// `Page.navigate`s, by tab and their navigation, answered as it is
    /// about to commit (Chromium's `PageHandler::Navigate`).
    navigates: Vec<(TabId, u64, Reply)>,
    /// From 1.
    query_ids: SequenceNumber<u64>,
    /// `Machopo.queryElement`s, by query id.
    queries: HashMap<u64, (TabId, Reply)>,
    /// From 1.
    visual_states: SequenceNumber<u64>,
    /// `Machopo.captureWindow`s waiting for their visual state.
    copies: HashMap<u64, (TabId, Reply)>,
    /// `Page.captureScreenshot`s and `Machopo.captureElementScreenshot`s
    /// waiting for theirs.
    page_captures: HashMap<u64, PageCapture>,
    /// The `Machopo.captureElementScreenshot`s among the WebDriver requests:
    /// the renderer's answer is where to capture.
    element_captures: HashSet<u64>,
    /// `Machopo.scrollIntoViewIfNeeded`s waiting for theirs.
    scrolls: HashMap<u64, (TabId, Reply)>,
    /// `Machopo.waitForInputHandled`s, by tab.
    input_handled: Vec<(TabId, Reply)>,
    /// From 1.
    webdriver_ids: SequenceNumber<u64>,
    /// The `Machopo` domain's WebDriver methods, by request.
    webdriver: HashMap<u64, (TabId, Reply)>,
    /// Commands handed to a renderer, by session and id, until it answers:
    /// answered that the renderer has gone if it goes first (Chromium's
    /// `DevToolsSession` keeps them too).
    forwarded: HashMap<(String, i32), Reply>,
    /// `Browser.close` was answered: the embedder ends the browser.
    close: bool,
    /// The browser has the platform's window (`in_platform_window`).
    windowed: bool,
}

/// The `Network` commands only the browser answers (Chromium's
/// `NetworkHandler`: cookies, the cache, network conditions), as machopo can:
/// what holds anyway succeeds (it keeps no cookies nor cache, and loads at
/// full speed), what it can't do is an error. `None` for the others, which
/// the renderer answers.
fn network_answer(name: &str, params: &Value) -> Option<Result<Value, Error>> {
    let cant = |what: &str| Err(Error::new(SERVER_ERROR, format!("machopo can't {what}")));
    Some(match name {
        "Network.getCookies" | "Network.getAllCookies" => Ok(json!({ "cookies": [] })),
        "Network.clearBrowserCache" | "Network.clearBrowserCookies" => Ok(json!({})),
        "Network.canClearBrowserCache" | "Network.canClearBrowserCookies" => {
            Ok(json!({ "result": true }))
        }
        "Network.canEmulateNetworkConditions" => Ok(json!({ "result": false })),
        "Network.emulateNetworkConditionsByRule" => {
            let offline = params.get("offline").and_then(Value::as_bool) == Some(true);
            let rules = params
                .get("matchedNetworkConditions")
                .and_then(Value::as_array)
                .is_some_and(|rules| !rules.is_empty());
            if offline || rules {
                cant("take the network offline or throttle it")
            } else {
                Ok(json!({ "ruleIds": [] }))
            }
        }
        "Network.emulateNetworkConditions" => {
            let number = |name: &str| params.get(name).and_then(Value::as_f64).unwrap_or(0.0);
            if params.get("offline").and_then(Value::as_bool) == Some(true)
                || number("latency") > 0.0
                || number("downloadThroughput") > 0.0
                || number("uploadThroughput") > 0.0
            {
                cant("take the network offline or throttle it")
            } else {
                Ok(json!({}))
            }
        }
        _ => return None,
    })
}

fn no_target() -> Error {
    Error::new(SERVER_ERROR, "No target with given id found")
}

fn no_session() -> Error {
    Error::new(SESSION_NOT_FOUND, "Session with given id not found.")
}

fn invalid(message: &str) -> Error {
    Error::new(INVALID_PARAMS, format!("Invalid parameters: {message}"))
}

/// The URL without its fragment.
fn without_fragment(url: &str) -> &str {
    url.split_once('#').map_or(url, |(before, _)| before)
}

fn rect(rect: RectF) -> Rect {
    Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
    }
}

/// `Machopo.dispatchWindowInput`'s input as the window takes it.
fn window_input(input: &WindowInput) -> Result<Input, Error> {
    let at = PointF::new(input.x, input.y);
    match input.kind.as_str() {
        "move" => Ok(Input::Move(at)),
        "click" => Ok(Input::Click(at)),
        "dblclick" => Ok(Input::DoubleClick(at)),
        "wheel" => Ok(Input::Wheel {
            at,
            delta: ScrollDelta::Pixels {
                x: input.delta_x,
                y: input.delta_y,
            },
        }),
        "key" => {
            let name = input
                .key
                .as_deref()
                .ok_or_else(|| invalid("a key needs `key`"))?;
            let (key, _) = crate::scenario::key_named(name).map_err(|e| invalid(&e))?;
            let mut modifiers = ModifiersState::empty();
            for (bit, state) in [
                (automation::ALT, ModifiersState::ALT),
                (automation::CTRL, ModifiersState::CONTROL),
                (automation::META, ModifiersState::SUPER),
                (automation::SHIFT, ModifiersState::SHIFT),
            ] {
                if input.modifiers & bit != 0 {
                    modifiers |= state;
                }
            }
            Ok(Input::Key {
                key,
                modifiers,
                text: input.text.clone(),
            })
        }
        other => Err(invalid(&format!("no input of type {other:?}"))),
    }
}

impl AutomationSession {
    /// A session in automation mode over `browser`'s tabs as they are now
    /// (those don't count as created), driven by the client whose answers
    /// and events go to `outbox` (the pipe's, the scenario runner's). The
    /// browser is a window without the platform's, unless
    /// [`Self::in_platform_window`].
    pub fn new(browser: &BrowserWindow, outbox: impl Into<Outbox>) -> Self {
        let mut session = Self::without_driver(browser);
        session.automation = true;
        session.driver = Some(session.add_client(outbox.into(), None));
        session
    }

    /// A session for remote debugging only (`machopo --remote-debugging-port`
    /// in a window a person uses): no driver, no `Machopo` domain.
    pub fn for_remote_debugging(browser: &BrowserWindow) -> Self {
        Self::without_driver(browser)
    }

    fn without_driver(browser: &BrowserWindow) -> Self {
        let mut session = Self {
            clients: HashMap::new(),
            client_ids: SequenceNumber::starting_at(1),
            driver: None,
            automation: false,
            sessions: HashMap::new(),
            session_ids: SequenceNumber::starting_at(1),
            targets: HashMap::new(),
            tabs: Vec::new(),
            loaders: HashMap::new(),
            starting: HashMap::new(),
            committed: HashMap::new(),
            crashed: Vec::new(),
            loaded: HashMap::new(),
            loads: Vec::new(),
            navigates: Vec::new(),
            restoring: HashSet::new(),
            restore_ids: SequenceNumber::starting_at(1),
            query_ids: SequenceNumber::starting_at(1),
            queries: HashMap::new(),
            visual_states: SequenceNumber::starting_at(1),
            copies: HashMap::new(),
            page_captures: HashMap::new(),
            element_captures: HashSet::new(),
            scrolls: HashMap::new(),
            input_handled: Vec::new(),
            webdriver_ids: SequenceNumber::starting_at(1),
            webdriver: HashMap::new(),
            forwarded: HashMap::new(),
            close: false,
            windowed: false,
        };
        // No client yet: nothing is told of the tabs there are.
        session.changes(browser);
        session
    }

    fn add_client(&mut self, outbox: Outbox, page_session: Option<String>) -> ClientId {
        let id = ClientId(self.client_ids.get_next());
        self.clients.insert(
            id,
            Client {
                outbox,
                discover: false,
                told: Vec::new(),
                page_session,
            },
        );
        id
    }

    /// A remote debugging client connects, to the browser, or straight to
    /// the tab `page` (a target id): its id, or `None` if there is no such
    /// tab.
    pub fn connect(
        &mut self,
        outbox: Outbox,
        page: Option<&str>,
        browser: &BrowserWindow,
    ) -> Option<ClientId> {
        self.changes(browser);
        let Some(target) = page else {
            return Some(self.add_client(outbox, None));
        };
        let tab = self.tab_of(target)?;
        let session_id = self.new_session_id();
        // What goes to the session goes without its id: the connection is
        // the tab's.
        let implicit = session_id.clone();
        let outbox = Outbox::new(move |mut message| {
            let session = match &mut message {
                Message::Response(response) => &mut response.session_id,
                Message::Event(event) => &mut event.session_id,
                Message::Raw { session_id, .. } => session_id,
            };
            if session.as_deref() == Some(implicit.as_str()) {
                *session = None;
            }
            outbox.send(message)
        });
        let client = self.add_client(outbox, Some(session_id.clone()));
        self.attach(browser, client, tab, session_id);
        Some(client)
    }

    /// `client`'s session `session`, of `tab`, goes, in the renderer too:
    /// `Target.detachedFromTarget`, and its commands' answers have no one
    /// to go to.
    fn detach(&mut self, browser: &BrowserWindow, client: ClientId, session: &str, tab: TabId) {
        self.sessions.remove(session);
        if let Some(contents) = browser.view().contents(tab) {
            contents.devtools_detach(session);
        }
        self.forwarded.retain(|(of, _), _| of != session);
        self.detached_from_target(client, session, tab);
    }

    /// Tells `client` its session `session` of `tab` has gone
    /// (`Target.detachedFromTarget`, on the browser's session).
    fn detached_from_target(&self, client: ClientId, session: &str, tab: TabId) {
        let mut object = serde_json::Map::new();
        object.insert("method".into(), "Target.detachedFromTarget".into());
        object.insert(
            "params".into(),
            json!({ "sessionId": session, "targetId": self.target_id(tab).unwrap_or_default() }),
        );
        self.send(
            client,
            Message::Raw {
                session_id: None,
                object,
            },
        );
    }

    /// The client has gone: its sessions with it, in the renderers too.
    pub fn disconnect(&mut self, client: ClientId, browser: &BrowserWindow) {
        self.clients.remove(&client);
        let view = browser.view();
        self.sessions.retain(|id, attached| {
            if attached.client != client {
                return true;
            }
            if let Some(contents) = view.contents(attached.tab) {
                contents.devtools_detach(id);
            }
            false
        });
        // Its commands' answers have no one to go to.
        let sessions = &self.sessions;
        self.forwarded
            .retain(|(session, _), _| sessions.contains_key(session));
    }

    /// Carries out what a remote debugging connection asks; input it hands
    /// back, as [`Self::command`] does.
    pub fn client_event(
        &mut self,
        event: ClientEvent,
        browser: &mut BrowserWindow,
    ) -> Option<(Input, Reply)> {
        match event {
            ClientEvent::Connect {
                outbox,
                page,
                accepted,
            } => accepted(self.connect(outbox, page.as_deref(), browser)),
            ClientEvent::Command(client, Ok(command)) => {
                return self.command_from(client, command, browser);
            }
            ClientEvent::Command(client, Err((id, error))) => self.refuse_from(client, id, error),
            ClientEvent::Disconnect(client) => self.disconnect(client, browser),
            ClientEvent::Json { request, answer } => answer(self.json(request, browser)),
        }
        None
    }

    /// Answers one of the HTTP endpoints.
    pub fn json(&mut self, request: JsonRequest, browser: &mut BrowserWindow) -> JsonAnswer {
        self.changes(browser);
        let answer = match request {
            JsonRequest::Version => JsonAnswer::Version {
                product: self.product(),
                user_agent: browser::user_agent(),
            },
            JsonRequest::List => JsonAnswer::Targets(
                browser
                    .view()
                    .tabs()
                    .map(|contents| self.target_info(contents))
                    .collect(),
            ),
            JsonRequest::New(url) => {
                // As Chrome's: what isn't a URL opens a blank page.
                let url = if Url::parse(&url).is_ok() {
                    url
                } else {
                    "about:blank".to_owned()
                };
                match browser.view_mut().open_tab(&url, false) {
                    Some(tab) => {
                        self.changes(browser);
                        match browser.view().contents(tab) {
                            Some(contents) => JsonAnswer::Target(self.target_info(contents)),
                            None => JsonAnswer::NoTarget,
                        }
                    }
                    None => JsonAnswer::Failed("Could not create new page".into()),
                }
            }
            JsonRequest::Activate(target) => match self.tab_of(&target) {
                Some(tab) if browser.view_mut().activate(tab) => JsonAnswer::Done,
                _ => JsonAnswer::NoTarget,
            },
            JsonRequest::Close(target) => match self.tab_of(&target) {
                Some(tab) if browser.view_mut().close(tab) => JsonAnswer::Done,
                _ => JsonAnswer::NoTarget,
            },
        };
        self.changes(browser);
        answer
    }

    /// `tab`'s target id, which is its main frame's id too.
    pub fn target_id(&self, tab: TabId) -> Option<String> {
        self.targets.get(&tab).cloned()
    }

    /// `tab`'s target id: the session sees every tab before a command or a
    /// message can name it ([`Self::changes`] comes first).
    fn target(&self, tab: TabId) -> String {
        self.target_id(tab)
            .expect("every tab the session works on has been seen")
    }

    fn target_info(&self, contents: &WebContents) -> TargetInfo {
        TargetInfo {
            target_id: self.target(contents.tab()),
            kind: "page".into(),
            title: contents.title().to_owned(),
            url: contents.url().unwrap_or("about:blank").to_owned(),
            attached: self.attached(contents.tab()),
            can_access_opener: false,
        }
    }

    /// `Browser.getVersion`'s product: without the platform's window, as
    /// Chrome's headless mode tells (`HeadlessChrome/…`).
    fn product(&self) -> String {
        let product = if self.windowed {
            "machopo"
        } else {
            "Headlessmachopo"
        };
        format!("{product}/{}", env!("CARGO_PKG_VERSION"))
    }

    fn new_session_id(&mut self) -> String {
        format!("{:032X}", self.session_ids.get_next())
    }

    /// A session of `client` for `tab`, attached to its renderer too: the
    /// methods the browser doesn't have go there.
    fn attach(
        &mut self,
        browser: &BrowserWindow,
        client: ClientId,
        tab: TabId,
        session_id: String,
    ) {
        if let Some(contents) = browser.view().contents(tab) {
            contents.devtools_attach(&session_id);
        }
        self.sessions.insert(
            session_id,
            Attached {
                client,
                tab,
                page: false,
                lifecycle: false,
                network: false,
                log: false,
            },
        );
    }

    /// `client`'s session `id`.
    fn session_of(&self, client: ClientId, id: &str) -> Option<&Attached> {
        self.sessions
            .get(id)
            .filter(|attached| attached.client == client)
    }

    /// The browser is the platform's window (`machopo --automation-pipe
    /// --window`, `machopo --automation`), which `Browser.getVersion` tells.
    pub fn in_platform_window(mut self) -> Self {
        self.windowed = true;
        self
    }

    /// `Browser.close` was answered.
    pub fn close_requested(&self) -> bool {
        self.close
    }

    /// Answers the driver's message that isn't a command with why (CDP's
    /// dispatcher does); one with no id to answer to is only logged, as
    /// Chromium does.
    pub fn refuse(&self, id: Option<i32>, error: Error) {
        if let Some(driver) = self.driver {
            self.refuse_from(driver, id, error);
        }
    }

    fn refuse_from(&self, client: ClientId, id: Option<i32>, error: Error) {
        let message = match id {
            Some(id) => Message::Response(Response {
                id,
                session_id: None,
                result: Err(error),
            }),
            // As Chrome: an error with no id.
            None => Message::error_without_id(error),
        };
        self.send(client, message);
    }

    fn send(&self, client: ClientId, message: Message) {
        if let Some(client) = self.clients.get(&client) {
            client.outbox.send(message);
        }
    }

    /// Where `client`'s `command` is answered; the command is `client`'s
    /// (`command_from` checked it is there).
    fn reply(&self, client: ClientId, command: &Command) -> Reply {
        Reply {
            id: command.id,
            session_id: command.session_id.clone(),
            outbox: self.clients[&client].outbox.clone(),
        }
    }

    /// Carries out the driver's `command`: what it tells (a navigation it
    /// started) first, then its answer, now or later. Input it hands back,
    /// for the embedder to give the window and then answer.
    pub fn command(
        &mut self,
        command: Command,
        browser: &mut BrowserWindow,
    ) -> Option<(Input, Reply)> {
        let driver = self
            .driver
            .expect("only a session in automation mode has a driver");
        self.command_from(driver, command, browser)
    }

    fn command_from(
        &mut self,
        client: ClientId,
        mut command: Command,
        browser: &mut BrowserWindow,
    ) -> Option<(Input, Reply)> {
        let known = self.clients.get(&client)?;
        // On a tab's own connection, a command without a session is the
        // tab's.
        if command.session_id.is_none() {
            command.session_id.clone_from(&known.page_session);
        }
        // A tab the window made since (a person opened it) has a target id.
        self.changes(browser);
        let dispatched = self.dispatch(client, &command, browser);
        self.changes(browser);
        // A traversal within the document started no loader.
        self.starting.clear();
        match dispatched {
            Dispatched::Now(result) => {
                self.reply(client, &command).answer(result);
                None
            }
            Dispatched::Later => None,
            Dispatched::Input(input) => Some((input, self.reply(client, &command))),
        }
    }

    /// A message from `tab`'s renderer, once the view has had it: what it
    /// tells, and the answers it was waited for.
    pub fn on_renderer_message(
        &mut self,
        tab: TabId,
        msg: &RendererHostMsg,
        browser: &BrowserWindow,
    ) {
        // A link followed, `location`: a navigation the page started.
        self.changes(browser);
        let frame_id = self.target_id(tab).unwrap_or_default();
        match msg {
            RendererHostMsg::DidCommitNavigation { url, .. } => {
                self.committed.insert(tab, url.clone());
            }
            RendererHostMsg::DidFinishLoad { navigation_id, .. } => {
                let loaded = self.loaded.entry(tab).or_default();
                *loaded = (*loaded).max(*navigation_id);
                let loaded = *loaded;
                let (done, waiting) = std::mem::take(&mut self.loads)
                    .into_iter()
                    .partition(|(of, id, _)| *of == tab && *id <= loaded);
                self.loads = waiting;
                for (_, _, reply) in done {
                    reply.done();
                }
            }
            RendererHostMsg::DidCommitSameDocumentNavigation {
                url, navigation_id, ..
            } => {
                let before = self.committed.insert(tab, url.clone()).unwrap_or_default();
                let navigation_type = if navigation_id.is_some() {
                    // Back or forward between the document's entries.
                    "other"
                } else if without_fragment(&before) == without_fragment(url) {
                    "fragment"
                } else {
                    "historyApi"
                };
                self.to_pages(
                    tab,
                    |attached| attached.page,
                    EventKind::NavigatedWithinDocument {
                        frame_id,
                        url: url.clone(),
                        navigation_type: navigation_type.into(),
                    },
                );
            }
            RendererHostMsg::QueryElementResult { query_id, element } => {
                let Some((_, reply)) = self.queries.remove(query_id) else {
                    return;
                };
                let view = browser.view();
                let result = QueryResult {
                    element: element.as_ref().map(|element| QueriedElement {
                        rect: element
                            .rect
                            .and_then(|r| view.web_view_to_window(tab, r))
                            .map(rect),
                        text: element.text.clone(),
                    }),
                    page: view.web_view_bounds(tab).map(rect),
                    scale: view.scale(),
                };
                reply.answer(Ok(serde_json::to_value(result).expect("serializes")));
            }
            // The view has counted it.
            RendererHostMsg::InputEventAck { .. } => {
                if browser
                    .view()
                    .contents(tab)
                    .is_some_and(|contents| contents.input_in_flight() == 0)
                {
                    let (done, waiting) = std::mem::take(&mut self.input_handled)
                        .into_iter()
                        .partition(|(of, _)| *of == tab);
                    self.input_handled = waiting;
                    for (_, reply) in done {
                        reply.done();
                    }
                }
            }
            // The frame showing it is ahead of this request in Viz's queue.
            RendererHostMsg::WebDriverReply { request_id, result } => {
                let Some((_, reply)) = self.webdriver.remove(request_id) else {
                    return;
                };
                if self.element_captures.remove(request_id) {
                    // Where the element is: captured as the next frame
                    // shows it, scrolled into view.
                    let clip = match result.as_ref().map(rect_of) {
                        Ok(Some(clip)) => clip,
                        Ok(None) => {
                            return reply.answer(Err(Error::new(SERVER_ERROR, "no element rect")));
                        }
                        Err(e) => return reply.answer(Err(e.clone().into())),
                    };
                    self.capture_page(browser, tab, reply, Some(clip));
                    return;
                }
                let window = self.target_id(tab).unwrap_or_default();
                reply.answer(
                    result
                        .clone()
                        .map(|value| json!({ "value": value.to_json(&window) }))
                        .map_err(Error::from),
                );
            }
            RendererHostMsg::VisualStateReady { request_id } => {
                if let Some(capture) = self.page_captures.remove(request_id) {
                    copy_page(browser, capture);
                } else if let Some((_, reply)) = self.copies.remove(request_id) {
                    copy_output(browser, reply);
                } else if let Some((_, reply)) = self.scrolls.remove(request_id) {
                    reply.done();
                }
            }
            RendererHostMsg::DevToolsMessage { session, message } => {
                self.dispatch_to_client(tab, session, message);
            }
            // Nothing more will come from it: what waits for it fails.
            RendererHostMsg::RendererGone => self.renderer_gone(tab),
            _ => {}
        }
    }

    /// An answer or event of `session` from `tab`'s renderer: to the
    /// session's client as it is, with the session's id (Chromium's
    /// `DevToolsSession::DispatchProtocolMessageToClient`). Only a renderer's
    /// own session's, as an object, and not past the limit (a renderer is
    /// not to be trusted: once it is a process of its own, one taken over
    /// could send anything); anything else is dropped.
    fn dispatch_to_client(&mut self, tab: TabId, session: &str, message: &str) {
        let Some(attached) = self
            .sessions
            .get(session)
            .filter(|attached| attached.tab == tab)
        else {
            tracing::warn!(session, "a DevTools message for another tab's session");
            return;
        };
        if message.len() > MAX_RENDERER_MESSAGE_SIZE {
            tracing::warn!(
                session,
                bytes = message.len(),
                "a DevTools message over the limit"
            );
            return;
        }
        let Ok(Value::Object(object)) = serde_json::from_str::<Value>(message) else {
            tracing::warn!(session, "a DevTools message that isn't a JSON object");
            return;
        };
        let client = attached.client;
        // An answer: no longer waited for; the browser's own, for no one.
        if let Some(id) = object.get("id").and_then(Value::as_i64) {
            if self.restoring.remove(&(session.to_owned(), id)) {
                return;
            }
            if let Ok(id) = i32::try_from(id) {
                self.forwarded.remove(&(session.to_owned(), id));
            }
        }
        self.send(
            client,
            Message::Raw {
                session_id: Some(session.to_owned()),
                object,
            },
        );
    }

    /// `tab`'s renderer has gone (it crashed, or `about:crash` ended it):
    /// what waits for it fails, as nothing more will come from it.
    fn renderer_gone(&mut self, tab: TabId) {
        let gone = || Err(Error::new(SERVER_ERROR, "the tab's renderer has gone"));
        let (queries, kept) = std::mem::take(&mut self.queries)
            .into_iter()
            .partition(|(_, (of, _))| *of == tab);
        self.queries = kept;
        let (copies, kept) = std::mem::take(&mut self.copies)
            .into_iter()
            .partition(|(_, (of, _))| *of == tab);
        self.copies = kept;
        let (scrolls, kept) = std::mem::take(&mut self.scrolls)
            .into_iter()
            .partition(|(_, (of, _))| *of == tab);
        self.scrolls = kept;
        let (captures, kept): (HashMap<_, _>, _) = std::mem::take(&mut self.page_captures)
            .into_iter()
            .partition(|(_, capture)| capture.tab == tab);
        self.page_captures = kept;
        let captures = captures
            .into_iter()
            .map(|(id, capture)| (id, (capture.tab, capture.reply)))
            .collect();
        let (webdriver, kept) = std::mem::take(&mut self.webdriver)
            .into_iter()
            .partition(|(_, (of, _))| *of == tab);
        self.webdriver = kept;
        self.element_captures
            .retain(|id| self.webdriver.contains_key(id));
        let (forwarded, kept): (HashMap<_, _>, _) = std::mem::take(&mut self.forwarded)
            .into_iter()
            .partition(|((session, _), _)| {
                self.sessions
                    .get(session)
                    .is_some_and(|attached| attached.tab == tab)
            });
        self.forwarded = kept;
        let gone_by_id: Vec<Reply> = [queries, copies, captures, scrolls, webdriver]
            .into_iter()
            .flat_map(|map: HashMap<u64, (TabId, Reply)>| map.into_values())
            .map(|(_, reply)| reply)
            .collect();
        let (loads, kept) = std::mem::take(&mut self.loads)
            .into_iter()
            .partition(|(of, _, _)| *of == tab);
        self.loads = kept;
        let (navigates, kept) = std::mem::take(&mut self.navigates)
            .into_iter()
            .partition(|(of, _, _)| *of == tab);
        self.navigates = kept;
        let (handled, kept) = std::mem::take(&mut self.input_handled)
            .into_iter()
            .partition(|(of, _)| *of == tab);
        self.input_handled = kept;
        self.loaded.remove(&tab);
        let waiting = gone_by_id
            .into_iter()
            .chain(loads.into_iter().map(|(_, _, reply): (_, _, Reply)| reply))
            .chain(
                navigates
                    .into_iter()
                    .map(|(_, _, reply): (_, _, Reply)| reply),
            )
            .chain(handled.into_iter().map(|(_, reply): (_, Reply)| reply))
            .chain(forwarded.into_values());
        for reply in waiting {
            reply.answer(gone());
        }
    }

    /// `Network`'s event `method`, `params`, to every session of `tab` that
    /// turned `Network` on, through its client.
    fn to_network(&self, tab: TabId, method: &str, params: Value) {
        let mut sessions: Vec<(&String, ClientId)> = self
            .sessions
            .iter()
            .filter(|(_, attached)| attached.tab == tab && attached.network)
            .map(|(id, attached)| (id, attached.client))
            .collect();
        sessions.sort();
        for (session_id, client) in sessions {
            let mut object = serde_json::Map::new();
            object.insert("method".into(), method.into());
            object.insert("params".into(), params.clone());
            self.send(
                client,
                Message::Raw {
                    session_id: Some(session_id.clone()),
                    object,
                },
            );
        }
    }

    /// Warns `command`'s session, if it turned `Log` on, that machopo can't do
    /// what it asked (`text`), as the renderer warns of what it refuses:
    /// DevTools' front end doesn't show the error it is answered.
    fn warn(&self, client: ClientId, command: &Command, text: &str) {
        let Some(session) = &command.session_id else {
            return;
        };
        if !self
            .sessions
            .get(session)
            .is_some_and(|attached| attached.client == client && attached.log)
        {
            return;
        }
        let mut object = serde_json::Map::new();
        object.insert("method".into(), "Log.entryAdded".into());
        object.insert(
            "params".into(),
            json!({
                "entry": {
                    "source": "other",
                    "level": "warning",
                    "text": text,
                    "timestamp": wall_time() * 1000.0,
                },
            }),
        );
        self.send(
            client,
            Message::Raw {
                session_id: Some(session.clone()),
                object,
            },
        );
    }

    /// A navigation of `tab`'s reached a redirect or ended, on the UI
    /// thread before the tab follows it or commits: its request's
    /// `Network` events (Chromium's `NetworkHandler`). The renderer's
    /// messages come to `on_renderer_message`.
    pub fn on_contents_event(
        &mut self,
        tab: TabId,
        event: &ContentsEvent,
        browser: &BrowserWindow,
    ) {
        // Its start first.
        self.changes(browser);
        let frame_id = self.target(tab);
        match event {
            ContentsEvent::Renderer(_) => {}
            ContentsEvent::NavigationRedirected {
                navigation_id,
                redirect,
            } => {
                let request = {
                    let mut request = ResourceRequest::new(
                        redirect.new_url.clone(),
                        RequestDestination::Document,
                    );
                    request.method.clone_from(&redirect.new_method);
                    request
                };
                self.to_network(
                    tab,
                    "Network.requestWillBeSent",
                    json!({
                        "requestId": navigation_id.to_string(),
                        "loaderId": navigation_id.to_string(),
                        "documentURL": redirect.new_url,
                        "request": request_json(&request, 0),
                        "timestamp": TimeTicks::now().as_secs_f64(),
                        "wallTime": wall_time(),
                        "initiator": { "type": "other" },
                        "redirectHasExtraInfo": false,
                        "redirectResponse": response_json(&redirect.response),
                        "type": "Document",
                        "frameId": frame_id,
                        "hasUserGesture": false,
                    }),
                );
            }
            ContentsEvent::NavigationCompleted {
                navigation_id,
                result: Ok(response),
            } => {
                let timestamp = response
                    .timing
                    .map_or_else(TimeTicks::now, |timing| timing.receive_headers_end);
                self.to_network(
                    tab,
                    "Network.responseReceived",
                    json!({
                        "requestId": navigation_id.to_string(),
                        "loaderId": navigation_id.to_string(),
                        "timestamp": timestamp.as_secs_f64(),
                        "type": "Document",
                        "response": response_json(response),
                        "hasExtraInfo": false,
                        "frameId": frame_id,
                    }),
                );
                // Its response came: ready to commit.
                self.navigation_ready(tab, *navigation_id, None);
            }
            ContentsEvent::NavigationCompleted {
                navigation_id,
                result: Err(error),
            } => {
                let error_text = format!("net::{}", error.code.name());
                self.to_network(
                    tab,
                    "Network.loadingFailed",
                    json!({
                        "requestId": navigation_id.to_string(),
                        "timestamp": TimeTicks::now().as_secs_f64(),
                        "type": "Document",
                        "errorText": error_text,
                        "canceled": false,
                    }),
                );
                // Its error page commits.
                self.navigation_ready(tab, *navigation_id, Some(error_text));
            }
        }
    }

    /// `Page.navigate`'s result for `tab`'s navigation `loader`.
    fn navigate_result(&self, tab: TabId, loader: u64, error_text: Option<String>) -> Value {
        let result = NavigateResult {
            frame_id: self.target(tab),
            loader_id: loader.to_string(),
            error_text,
            is_download: false,
        };
        serde_json::to_value(result).expect("serializes")
    }

    /// `tab`'s navigation `navigation_id` is about to commit (its document,
    /// or an error page for `error_text`): the `Page.navigate`s that
    /// started it are answered.
    fn navigation_ready(&mut self, tab: TabId, navigation_id: u64, error_text: Option<String>) {
        let (ready, waiting) = std::mem::take(&mut self.navigates)
            .into_iter()
            .partition(|(of, id, _)| *of == tab && *id == navigation_id);
        self.navigates = waiting;
        for (_, _, reply) in ready {
            reply.answer(Ok(self.navigate_result(
                tab,
                navigation_id,
                error_text.clone(),
            )));
        }
    }

    /// `event` to every session of `tab` that asked for it, through its
    /// client.
    fn to_pages(&self, tab: TabId, wants: impl Fn(&Attached) -> bool, event: EventKind) {
        let mut sessions: Vec<(&String, ClientId)> = self
            .sessions
            .iter()
            .filter(|(_, attached)| attached.tab == tab && wants(attached))
            .map(|(id, attached)| (id, attached.client))
            .collect();
        sessions.sort();
        for (session_id, client) in sessions {
            self.send(
                client,
                Message::Event(Event {
                    session_id: Some(session_id.clone()),
                    event: event.clone(),
                }),
            );
        }
    }

    /// Notes the target id of `contents`'s tab (`WebContents`'s, drawn when
    /// it was made). Ids stay after their tabs go: a late message may still
    /// name one.
    fn know_target(&mut self, contents: &WebContents) -> String {
        self.targets
            .entry(contents.tab())
            .or_insert_with(|| contents.devtools_frame_token().to_owned())
            .clone()
    }

    /// The clients that asked for targets' events
    /// (`Target.setDiscoverTargets`), in the order they connected.
    fn discovering(&self) -> Vec<ClientId> {
        let mut clients: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, client)| client.discover)
            .map(|(id, _)| *id)
            .collect();
        clients.sort();
        clients
    }

    /// Tells what changed in the tabs since last told: tabs made, crashed
    /// (whatever ended the renderer: it crashed, `about:crash`) or closed,
    /// loaders started.
    fn changes(&mut self, browser: &BrowserWindow) {
        let view = browser.view();
        let now: Vec<TabId> = view.tabs().map(WebContents::tab).collect();
        for contents in view.tabs() {
            self.know_target(contents);
        }
        let discovering = self.discovering();
        let mut newly_crashed = Vec::new();
        for contents in view.tabs() {
            let tab = contents.tab();
            for client in &discovering {
                if !self.clients[client].told.contains(&tab) {
                    self.send(
                        *client,
                        Message::Event(Event {
                            session_id: None,
                            event: EventKind::TargetCreated {
                                target_info: self.target_info(contents),
                            },
                        }),
                    );
                }
            }
            let crashed = contents.is_crashed();
            if crashed && !self.crashed.contains(&tab) {
                for client in &discovering {
                    self.send(
                        *client,
                        Message::Event(Event {
                            session_id: None,
                            event: EventKind::TargetCrashed {
                                target_id: self.target(tab),
                                status: "crashed".into(),
                                error_code: 0,
                            },
                        }),
                    );
                }
            }
            if crashed != self.crashed.contains(&tab) {
                if crashed {
                    self.crashed.push(tab);
                    // Ended by the browser (`about:crash`): no
                    // `RendererGone` comes for it.
                    newly_crashed.push(tab);
                } else {
                    self.crashed.retain(|known| *known != tab);
                    // A new renderer: the tab's sessions are its too, `Page`
                    // and its lifecycle events on as they were (what else
                    // they enabled in the last one, they enable again).
                    let mut restoring = Vec::new();
                    for (id, attached) in &self.sessions {
                        if attached.tab == tab {
                            contents.devtools_attach(id);
                            if attached.page {
                                restoring.push((id.clone(), json!({ "method": "Page.enable" })));
                            }
                            if attached.lifecycle {
                                restoring.push((
                                    id.clone(),
                                    json!({
                                        "method": "Page.setLifecycleEventsEnabled",
                                        "params": { "enabled": true },
                                    }),
                                ));
                            }
                        }
                    }
                    for (session, mut command) in restoring {
                        let id = self.restore_ids.get_next().saturating_neg();
                        command["id"] = id.into();
                        self.restoring.insert((session.clone(), id));
                        contents.devtools_message(&session, command.to_string(), false);
                    }
                }
            }
            let loader = contents.latest_navigation_id();
            let known = self.loaders.insert(tab, loader);
            // Those a newer navigation replaced stopped loading.
            let (aborted, waiting) = std::mem::take(&mut self.navigates)
                .into_iter()
                .partition(|(of, id, _)| *of == tab && *id < loader);
            self.navigates = waiting;
            for (_, id, reply) in aborted {
                let error_text = Some("net::ERR_ABORTED".to_owned());
                reply.answer(Ok(self.navigate_result(tab, id, error_text)));
            }
            if known.is_some_and(|known| known != loader) {
                let navigation_type = self.starting.remove(&tab).unwrap_or("differentDocument");
                let url = contents.url().unwrap_or_default().to_owned();
                self.to_pages(
                    tab,
                    |attached| attached.page,
                    EventKind::FrameStartedNavigating {
                        frame_id: self.target(tab),
                        url: url.clone(),
                        loader_id: loader.to_string(),
                        navigation_type: navigation_type.into(),
                    },
                );
                // Its request (`NetworkHandler::NavigationRequestWillBeSent`);
                // the browser's own pages make none.
                if !url.starts_with("about:") {
                    let request = ResourceRequest::new(url.clone(), RequestDestination::Document);
                    self.to_network(
                        tab,
                        "Network.requestWillBeSent",
                        json!({
                            "requestId": loader.to_string(),
                            "loaderId": loader.to_string(),
                            "documentURL": url,
                            "request": request_json(&request, 0),
                            "timestamp": TimeTicks::now().as_secs_f64(),
                            "wallTime": wall_time(),
                            "initiator": { "type": "other" },
                            "redirectHasExtraInfo": false,
                            "type": "Document",
                            "frameId": self.target(tab),
                            "hasUserGesture": false,
                        }),
                    );
                }
            }
        }
        for client in &discovering {
            let gone: Vec<TabId> = self.clients[client]
                .told
                .iter()
                .filter(|tab| !now.contains(tab))
                .copied()
                .collect();
            for tab in gone {
                self.send(
                    *client,
                    Message::Event(Event {
                        session_id: None,
                        event: EventKind::TargetDestroyed {
                            target_id: self.target(tab),
                        },
                    }),
                );
            }
        }
        for client in self.clients.values_mut() {
            client.told.clone_from(&now);
        }
        for gone in self.tabs.clone().iter().filter(|tab| !now.contains(tab)) {
            self.loaders.remove(gone);
            self.committed.remove(gone);
            self.loaded.remove(gone);
            self.crashed.retain(|known| known != gone);
        }
        // A tab's own connection ends with the tab, as Chrome's does.
        let detached: Vec<ClientId> =
            self.sessions
                .iter()
                .filter(|(id, attached)| {
                    !now.contains(&attached.tab)
                        && self.clients.get(&attached.client).is_some_and(|client| {
                            client.page_session.as_deref() == Some(id.as_str())
                        })
                })
                .map(|(_, attached)| attached.client)
                .collect();
        for client in detached {
            self.send(
                client,
                Message::Event(Event {
                    session_id: None,
                    event: EventKind::InspectorDetached {
                        reason: "target_closed".into(),
                    },
                }),
            );
        }
        // A flat session's client hears of it on the browser's session.
        let flat: Vec<(ClientId, String, TabId)> =
            self.sessions
                .iter()
                .filter(|(id, attached)| {
                    !now.contains(&attached.tab)
                        && self.clients.get(&attached.client).is_some_and(|client| {
                            client.page_session.as_deref() != Some(id.as_str())
                        })
                })
                .map(|(id, attached)| (attached.client, id.clone(), attached.tab))
                .collect();
        for (client, session, tab) in flat {
            self.detached_from_target(client, &session, tab);
        }
        self.sessions
            .retain(|_, attached| now.contains(&attached.tab));
        self.tabs = now;
        for tab in newly_crashed {
            self.renderer_gone(tab);
        }
    }

    fn attached(&self, tab: TabId) -> bool {
        self.sessions.values().any(|attached| attached.tab == tab)
    }

    /// The tab there is now whose target id is `target_id`.
    fn tab_of(&self, target_id: &str) -> Option<TabId> {
        self.tabs
            .iter()
            .copied()
            .find(|tab| self.targets.get(tab).is_some_and(|id| id == target_id))
    }

    /// The window's bounds (`Browser.Bounds`): where it is on the screen
    /// and its size, CSS px.
    fn bounds(browser: &BrowserWindow) -> Value {
        let view = browser.view();
        let (size, scale, origin) = (view.size(), view.scale(), view.origin());
        json!({
            "left": clamp_round::<i32>(origin.x),
            "top": clamp_round::<i32>(origin.y),
            "width": clamp_round::<u32>(size.width as f32 / scale),
            "height": clamp_round::<u32>(size.height as f32 / scale),
            "windowState": "normal",
        })
    }

    fn dispatch(
        &mut self,
        client: ClientId,
        command: &Command,
        browser: &mut BrowserWindow,
    ) -> Dispatched {
        let not_found = || {
            Dispatched::Now(Err(Error::new(
                METHOD_NOT_FOUND,
                format!("'{}' wasn't found", command.method.name()),
            )))
        };
        // Automation mode's only: the window as a person uses it, which no
        // one else does then.
        if !self.automation && command.method.name().starts_with("Machopo.") {
            return not_found();
        }
        // The browser's methods, which no session has.
        match &command.method {
            Method::BrowserGetVersion => {
                // Pages' user agent stays.
                return Dispatched::Now(Ok(json!({
                    "protocolVersion": "1.3",
                    "product": self.product(),
                    "revision": "",
                    "userAgent": browser::user_agent(),
                    "jsVersion": "",
                })));
            }
            Method::SystemInfoGetInfo => {
                // Chrome's shape: the GPU in `gpu.devices`, what else it
                // knows of it in `gpu.auxAttributes` (machopo's adapter type,
                // `Cpu` when it draws on the CPU, and its backend).
                let (devices, aux) = match viz::gpu_context::adapter_info() {
                    Some(adapter) => (
                        json!([{
                            "vendorId": adapter.vendor,
                            "deviceId": adapter.device,
                            "vendorString": "",
                            "deviceString": adapter.name,
                            "driverVendor": adapter.driver,
                            "driverVersion": adapter.driver_info,
                        }]),
                        json!({
                            "adapterType": format!("{:?}", adapter.device_type),
                            "backend": format!("{:?}", adapter.backend),
                        }),
                    ),
                    None => (json!([]), json!({})),
                };
                return Dispatched::Now(Ok(json!({
                    "gpu": {
                        "devices": devices,
                        "auxAttributes": aux,
                        "featureStatus": {},
                        "driverBugWorkarounds": [],
                    },
                    "modelName": "",
                    "modelVersion": "",
                    "commandLine": "",
                })));
            }
            Method::BrowserClose => {
                self.close = true;
                return Dispatched::Now(Ok(json!({})));
            }
            Method::BrowserGetWindowForTarget(params) => {
                if let Some(target) = &params.target_id
                    && self.tab_of(target).is_none()
                {
                    return Dispatched::Now(Err(no_target()));
                }
                return Dispatched::Now(Ok(json!({
                    "windowId": WINDOW_ID,
                    "bounds": Self::bounds(browser),
                })));
            }
            Method::BrowserSetWindowBounds(params) => {
                if params.window_id != WINDOW_ID {
                    return Dispatched::Now(Err(Error::new(
                        SERVER_ERROR,
                        "Browser window not found",
                    )));
                }
                let bounds = Self::bounds(browser);
                let width = params
                    .bounds
                    .width
                    .unwrap_or(saturated_cast(bounds["width"].as_u64().unwrap_or_default()));
                let height = params.bounds.height.unwrap_or(saturated_cast(
                    bounds["height"].as_u64().unwrap_or_default(),
                ));
                let (left, top) = (params.bounds.left, params.bounds.top);
                let origin = (left.is_some() || top.is_some()).then(|| {
                    let at = |given: Option<i32>, key: &str| {
                        given.unwrap_or(saturated_cast(bounds[key].as_i64().unwrap_or_default()))
                            as f32
                    };
                    PointF::new(at(left, "left"), at(top, "top"))
                });
                // Viz draws the window into one texture: no side past
                // `MAX_TEXTURE_SIZE` device pixels (Chrome takes any size).
                let largest =
                    (protocol::viz::MAX_TEXTURE_SIZE as f32 / browser.view().scale()).floor();
                return Dispatched::Input(Input::SetBounds {
                    origin,
                    width: (width as f32).min(largest),
                    height: (height as f32).min(largest),
                });
            }
            Method::TargetGetTargets => {
                let infos: Vec<TargetInfo> = browser
                    .view()
                    .tabs()
                    .map(|contents| self.target_info(contents))
                    .collect();
                return Dispatched::Now(Ok(json!({ "targetInfos": infos })));
            }
            Method::TargetGetTargetInfo(params) => {
                let tab = match (&params.target_id, &command.session_id) {
                    (Some(target), _) => self.tab_of(target).ok_or_else(no_target),
                    (None, Some(session)) => self
                        .session_of(client, session)
                        .map(|attached| attached.tab)
                        .ok_or_else(no_session),
                    (None, None) => Err(no_target()),
                };
                let info = tab.and_then(|tab| {
                    let contents = browser.view().contents(tab).ok_or_else(no_target)?;
                    Ok(json!({ "targetInfo": self.target_info(contents) }))
                });
                return Dispatched::Now(info);
            }
            Method::TargetAttachToTarget(params) => {
                if !params.flatten {
                    return Dispatched::Now(Err(invalid("only flat sessions (flatten: true)")));
                }
                let Some(tab) = self.tab_of(&params.target_id) else {
                    return Dispatched::Now(Err(no_target()));
                };
                let session_id = self.new_session_id();
                self.attach(browser, client, tab, session_id.clone());
                return Dispatched::Now(Ok(json!({ "sessionId": session_id })));
            }
            Method::TargetSetDiscoverTargets(params) => {
                let known = self.clients.get_mut(&client).expect("dispatched for");
                // Turned on, it tells of the targets there are.
                if params.discover && !known.discover {
                    known.told.clear();
                }
                known.discover = params.discover;
                return Dispatched::Now(Ok(json!({})));
            }
            Method::TargetCreateTarget(params) => {
                if Url::parse(&params.url).is_err() {
                    return Dispatched::Now(Err(Error::new(SERVER_ERROR, "Invalid URL")));
                }
                return Dispatched::Now(
                    match browser.view_mut().open_tab(&params.url, params.background) {
                        Some(tab) => match browser.view().contents(tab) {
                            Some(contents) => Ok(json!({ "targetId": self.know_target(contents) })),
                            None => Err(no_target()),
                        },
                        None => Err(Error::new(SERVER_ERROR, "Failed to open a new tab")),
                    },
                );
            }
            // Nothing attaches on its own: machopo's iframes are in the tab's
            // renderer, and it has no workers.
            Method::TargetSetAutoAttach(_) => return Dispatched::Now(Ok(json!({}))),
            Method::TargetDetachFromTarget(params) => {
                let session = match (&params.session_id, &params.target_id) {
                    (Some(session), _) => Some(session.clone()),
                    (None, Some(target)) => self.tab_of(target).and_then(|tab| {
                        self.sessions
                            .iter()
                            .find(|(_, attached)| attached.client == client && attached.tab == tab)
                            .map(|(id, _)| id.clone())
                    }),
                    (None, None) => None,
                };
                let Some((session, attached)) = session.and_then(|session| {
                    let attached = self.session_of(client, &session)?.tab;
                    Some((session, attached))
                }) else {
                    return Dispatched::Now(Err(Error::new(
                        SERVER_ERROR,
                        "No session with given id",
                    )));
                };
                self.detach(browser, client, &session, attached);
                return Dispatched::Now(Ok(json!({})));
            }
            Method::TargetCloseTarget(params) => {
                let closed = self
                    .tab_of(&params.target_id)
                    .is_some_and(|tab| browser.view_mut().close(tab));
                return Dispatched::Now(if closed {
                    Ok(json!({ "success": true }))
                } else {
                    Err(no_target())
                });
            }
            Method::TargetActivateTarget(params) => {
                let shown = self
                    .tab_of(&params.target_id)
                    .is_some_and(|tab| browser.view_mut().activate(tab));
                return Dispatched::Now(if shown {
                    Ok(json!({}))
                } else {
                    Err(no_target())
                });
            }
            _ => {}
        }
        if let Method::Other { name, params } = &command.method {
            if let Some(answer) = network_answer(name, params) {
                if let Err(error) = &answer {
                    self.warn(client, command, &format!("{name}: {}", error.message));
                }
                return Dispatched::Now(answer);
            }
            // The browser's page events stop too; the renderer answers.
            if name == "Page.disable"
                && let Some(session) = &command.session_id
                && let Some(attached) = self
                    .sessions
                    .get_mut(session)
                    .filter(|attached| attached.client == client)
            {
                attached.page = false;
            }
            // Noted for the navigations' requests, which the browser tells
            // of, and passed on: the renderer tells of the rest
            // (Chromium's `NetworkHandler` falls through).
            if matches!(name.as_str(), "Network.enable" | "Network.disable")
                && let Some(session) = &command.session_id
                && let Some(attached) = self
                    .sessions
                    .get_mut(session)
                    .filter(|attached| attached.client == client)
            {
                attached.network = name == "Network.enable";
            }
            // Noted for the warnings above, and passed on: the renderer
            // keeps the page's log.
            if matches!(name.as_str(), "Log.enable" | "Log.disable")
                && let Some(session) = &command.session_id
                && let Some(attached) = self
                    .sessions
                    .get_mut(session)
                    .filter(|attached| attached.client == client)
            {
                attached.log = name == "Log.enable";
            }
            return self.forward(client, command, name, params, browser);
        }
        if let Some(webdriver) = webdriver_command(&command.method) {
            return self.dispatch_webdriver(client, command, webdriver, browser);
        }
        if command.method.name().starts_with("Machopo.") {
            return self.dispatch_machopo(client, command, browser);
        }
        if matches!(
            command.method,
            Method::PageCaptureScreenshot(_)
                | Method::PageNavigate(_)
                | Method::InputDispatchMouseEvent(_)
                | Method::InputDispatchKeyEvent(_)
        ) {
            return self.dispatch_page_later(client, command, browser);
        }
        // The renderer's `Page` agent sends the lifecycle events, as Blink's.
        if let Method::PageSetLifecycleEventsEnabled(params) = &command.method {
            if let Some(attached) = command
                .session_id
                .as_ref()
                .and_then(|session| self.sessions.get_mut(session))
                .filter(|attached| attached.client == client)
            {
                attached.lifecycle = params.enabled;
            }
            let params = json!({ "enabled": params.enabled });
            return self.forward(
                client,
                command,
                "Page.setLifecycleEventsEnabled",
                &params,
                browser,
            );
        }
        if matches!(command.method, Method::PageEnable) {
            // Noted, and passed on, as Chromium's handlers fall through: the
            // renderer's `Page` agent answers, and tells of its frames.
            if let Err(error) = self.dispatch_page(client, command, browser) {
                return Dispatched::Now(Err(error));
            }
            return self.forward(client, command, "Page.enable", &json!({}), browser);
        }
        Dispatched::Now(self.dispatch_page(client, command, browser))
    }

    /// A method the browser doesn't have, of a page's session: to the tab's
    /// renderer as it is (without the session's id), whose answer comes
    /// back as `RendererHostMsg::DevToolsMessage`. Without a session, no one
    /// has it.
    fn forward(
        &mut self,
        client: ClientId,
        command: &Command,
        name: &str,
        params: &Value,
        browser: &BrowserWindow,
    ) -> Dispatched {
        let Some(session) = &command.session_id else {
            return Dispatched::Now(Err(Error::new(
                METHOD_NOT_FOUND,
                format!("'{name}' wasn't found"),
            )));
        };
        let Some(tab) = self
            .session_of(client, session)
            .map(|attached| attached.tab)
        else {
            return Dispatched::Now(Err(no_session()));
        };
        let Some(contents) = browser.view().contents(tab) else {
            return Dispatched::Now(Err(no_target()));
        };
        if contents.is_crashed() {
            return Dispatched::Now(Err(Error::new(SERVER_ERROR, "the tab's renderer has gone")));
        }
        let message = json!({ "id": command.id, "method": name, "params": params });
        contents.devtools_message(session, message.to_string(), INTERRUPTING.contains(&name));
        self.forwarded
            .insert((session.clone(), command.id), self.reply(client, command));
        Dispatched::Later
    }

    /// A page's that is answered later: once the frame showing the page is
    /// drawn, the renderer has handled the input, or the navigation is
    /// about to commit.
    fn dispatch_page_later(
        &mut self,
        client: ClientId,
        command: &Command,
        browser: &mut BrowserWindow,
    ) -> Dispatched {
        let Some(session) = &command.session_id else {
            return Dispatched::Now(Err(Error::new(
                METHOD_NOT_FOUND,
                format!("'{}' wasn't found", command.method.name()),
            )));
        };
        let Some(tab) = self
            .session_of(client, session)
            .map(|attached| attached.tab)
        else {
            return Dispatched::Now(Err(no_session()));
        };
        match &command.method {
            Method::PageCaptureScreenshot(params) => {
                if params
                    .format
                    .as_deref()
                    .is_some_and(|format| format != "png")
                {
                    return Dispatched::Now(Err(invalid("machopo captures only PNG")));
                }
                self.capture_page(browser, tab, self.reply(client, command), None);
                Dispatched::Later
            }
            Method::PageNavigate(params) => {
                if Url::parse(&params.url).is_err() {
                    return Dispatched::Now(Err(Error::new(
                        SERVER_ERROR,
                        "Cannot navigate to invalid URL",
                    )));
                }
                let Some((loader, loading)) = browser.view_mut().update_contents(tab, |contents| {
                    contents.navigate(params.url.clone());
                    let loader = contents.latest_navigation_id();
                    (loader, contents.is_navigating(loader))
                }) else {
                    return Dispatched::Now(Err(no_target()));
                };
                // A page of the browser's own has committed already.
                if !loading {
                    return Dispatched::Now(Ok(self.navigate_result(tab, loader, None)));
                }
                self.navigates
                    .push((tab, loader, self.reply(client, command)));
                Dispatched::Later
            }
            Method::InputDispatchMouseEvent(_) | Method::InputDispatchKeyEvent(_) => {
                let event = match &command.method {
                    Method::InputDispatchMouseEvent(params) => mouse_event(params),
                    Method::InputDispatchKeyEvent(params) => key_event(params),
                    _ => unreachable!(),
                };
                let event = match event {
                    Ok(event) => event,
                    Err(e) => return Dispatched::Now(Err(e)),
                };
                let Some(contents) = browser.view_mut().contents_mut(tab) else {
                    return Dispatched::Now(Err(no_target()));
                };
                if contents.is_crashed() {
                    return Dispatched::Now(Err(Error::new(
                        SERVER_ERROR,
                        "the tab's renderer has gone",
                    )));
                }
                contents.forward_input_event(event);
                if contents.input_in_flight() == 0 {
                    return Dispatched::Now(Ok(json!({})));
                }
                self.input_handled.push((tab, self.reply(client, command)));
                Dispatched::Later
            }
            other => unreachable!("answered at once: {}", other.name()),
        }
    }

    /// Captures `tab`'s page (the part `clip` of it) as the next frame
    /// that shows what it has done so far draws it, for `reply`; the tab
    /// must be shown.
    fn capture_page(
        &mut self,
        browser: &BrowserWindow,
        tab: TabId,
        reply: Reply,
        clip: Option<RectF>,
    ) {
        let view = browser.view();
        let (Some(contents), Some(_)) = (view.contents(tab), view.web_view_bounds(tab)) else {
            return reply.answer(Err(Error::new(SERVER_ERROR, "the tab isn't shown")));
        };
        let state = self.visual_states.get_next();
        contents.request_visual_state(state);
        self.page_captures
            .insert(state, PageCapture { tab, reply, clip });
    }

    /// The `Machopo` domain's: on the window and its tab shown.
    fn dispatch_machopo(
        &mut self,
        client: ClientId,
        command: &Command,
        browser: &mut BrowserWindow,
    ) -> Dispatched {
        let contents = browser.view().web_contents();
        let tab = contents.tab();
        match &command.method {
            Method::MachopoNavigateFromAddressBar(params) => {
                browser.view_mut().navigate(&params.url);
                Dispatched::Now(Ok(json!({})))
            }
            Method::MachopoWaitForLoad => {
                // A page session's tab (machopodriver's window), else the one
                // shown (the scenario runner's).
                let tab = command
                    .session_id
                    .as_ref()
                    .and_then(|session| self.sessions.get(session))
                    .filter(|attached| attached.client == client)
                    .map_or(tab, |attached| attached.tab);
                let Some(latest) = browser
                    .view()
                    .contents(tab)
                    .map(WebContents::latest_navigation_id)
                else {
                    return Dispatched::Now(Err(no_target()));
                };
                if self
                    .loaded
                    .get(&tab)
                    .is_some_and(|loaded| *loaded >= latest)
                {
                    return Dispatched::Now(Ok(json!({})));
                }
                self.loads.push((tab, latest, self.reply(client, command)));
                Dispatched::Later
            }
            Method::MachopoDispatchWindowInput(params) => match window_input(params) {
                Ok(input) => Dispatched::Input(input),
                Err(e) => Dispatched::Now(Err(e)),
            },
            Method::MachopoGetTitle => {
                Dispatched::Now(Ok(json!({ "title": browser.view().title() })))
            }
            Method::MachopoGetCursor => Dispatched::Now(Ok(
                json!({ "cursor": crate::browser_view::cursor_name(browser.view().cursor()) }),
            )),
            Method::MachopoWaitForInputHandled => {
                if contents.input_in_flight() == 0 {
                    return Dispatched::Now(Ok(json!({})));
                }
                self.input_handled.push((tab, self.reply(client, command)));
                Dispatched::Later
            }
            Method::MachopoQueryElement(params) => {
                let query = self.query_ids.get_next();
                self.queries
                    .insert(query, (tab, self.reply(client, command)));
                contents.query_element(query, &params.frames, &params.selector);
                Dispatched::Later
            }
            // The visual state asked for after the scroll comes with the
            // frame that shows it, if there is one to draw.
            Method::MachopoScrollIntoViewIfNeeded(params) => {
                let state = self.visual_states.get_next();
                contents.scroll_into_view_if_needed(&params.frames, &params.selector);
                self.scrolls
                    .insert(state, (tab, self.reply(client, command)));
                contents.request_visual_state(state);
                Dispatched::Later
            }
            Method::MachopoCaptureWindow(params) if params.now => {
                copy_output(browser, self.reply(client, command));
                Dispatched::Later
            }
            Method::MachopoCaptureWindow(_) => {
                let state = self.visual_states.get_next();
                self.copies
                    .insert(state, (tab, self.reply(client, command)));
                contents.request_visual_state(state);
                Dispatched::Later
            }
            other => unreachable!("not the Machopo domain's: {}", other.name()),
        }
    }

    /// A WebDriver method of the `Machopo` domain: to the session's tab's
    /// renderer, its references read here first.
    fn dispatch_webdriver(
        &mut self,
        client: ClientId,
        command: &Command,
        webdriver: Result<WebDriverRequest, WebDriverError>,
        browser: &BrowserWindow,
    ) -> Dispatched {
        let Some(session) = &command.session_id else {
            return Dispatched::Now(Err(Error::new(
                METHOD_NOT_FOUND,
                format!("'{}' wasn't found", command.method.name()),
            )));
        };
        let Some(tab) = self
            .session_of(client, session)
            .map(|attached| attached.tab)
        else {
            return Dispatched::Now(Err(no_session()));
        };
        let Some(contents) = browser.view().contents(tab) else {
            return Dispatched::Now(Err(no_target()));
        };
        let request = webdriver.and_then(|request| request.command(&self.target(tab)));
        let request = match request {
            Ok(request) => request,
            Err(e) => return Dispatched::Now(Err(e.into())),
        };
        let request_id = self.webdriver_ids.get_next();
        self.webdriver
            .insert(request_id, (tab, self.reply(client, command)));
        if matches!(command.method, Method::MachopoCaptureElementScreenshot(_)) {
            self.element_captures.insert(request_id);
        }
        contents.webdriver(request_id, webdriver_frame(&command.method), request);
        Dispatched::Later
    }

    /// A page's: through its session.
    fn dispatch_page(
        &mut self,
        client: ClientId,
        command: &Command,
        browser: &mut BrowserWindow,
    ) -> Result<Value, Error> {
        let Some(session) = &command.session_id else {
            return Err(Error::new(
                METHOD_NOT_FOUND,
                format!("'{}' wasn't found", command.method.name()),
            ));
        };
        let attached = self
            .sessions
            .get_mut(session)
            .filter(|attached| attached.client == client)
            .ok_or_else(no_session)?;
        let tab = attached.tab;
        let view = browser.view_mut();
        match &command.method {
            Method::PageEnable => {
                attached.page = true;
                Ok(json!({}))
            }
            Method::PageReload(params) => {
                if params
                    .script_to_evaluate_on_load
                    .as_ref()
                    .is_some_and(|script| !script.is_empty())
                {
                    return Err(Error::new(
                        SERVER_ERROR,
                        "machopo can't evaluate a script on load",
                    ));
                }
                let current = view
                    .contents(tab)
                    .map(|contents| contents.latest_navigation_id().to_string());
                if let Some(loader) = &params.loader_id
                    && current.as_ref() != Some(loader)
                {
                    return Err(Error::new(SERVER_ERROR, "Not the current loader"));
                }
                self.starting.insert(tab, "reload");
                view.update_contents(tab, WebContents::reload)
                    .ok_or_else(no_target)?;
                Ok(json!({}))
            }
            Method::PageGetNavigationHistory => {
                let contents = view.contents(tab).ok_or_else(no_target)?;
                let controller = contents.controller();
                let entries = controller
                    .entries()
                    .iter()
                    .enumerate()
                    .map(|(index, entry)| NavigationEntry {
                        id: index as u64,
                        url: entry.url.clone(),
                        user_typed_url: entry.url.clone(),
                        title: entry.title.clone(),
                        transition_type: "typed".into(),
                    })
                    .collect();
                let history = NavigationHistory {
                    current_index: controller.current_index().unwrap_or(0) as u64,
                    entries,
                };
                Ok(serde_json::to_value(history).expect("serializes"))
            }
            Method::PageNavigateToHistoryEntry(params) => {
                let contents = view.contents(tab).ok_or_else(no_target)?;
                let controller = contents.controller();
                let current = controller.current_index();
                let entry = usize::try_from(params.entry_id)
                    .ok()
                    .filter(|index| *index < controller.entries().len());
                let (Some(current), Some(entry)) = (current, entry) else {
                    return Err(Error::new(SERVER_ERROR, "No entry with passed id"));
                };
                self.starting.insert(tab, "historyDifferentDocument");
                #[expect(
                    clippy::cast_possible_wrap,
                    clippy::arithmetic_side_effects,
                    reason = "both index the history's entries, fewer than isize::MAX: the \
                              casts are exact and their difference fits"
                )]
                let offset = entry as isize - current as isize;
                view.update_contents(tab, |contents| contents.go_to_offset(offset))
                    .ok_or_else(no_target)?;
                Ok(json!({}))
            }
            other => unreachable!("answered above: {}", other.name()),
        }
    }
}

/// A `Machopo` WebDriver method's parameters, before its arguments' references
/// are read with the tab's window handle.
enum WebDriverRequest {
    Script {
        body: String,
        args: Vec<Value>,
        is_async: bool,
    },
    Ready(WebDriverCommand),
}

impl WebDriverRequest {
    /// The renderer's command, the tab's window being `window`.
    fn command(self, window: &str) -> Result<WebDriverCommand, WebDriverError> {
        Ok(match self {
            WebDriverRequest::Script {
                body,
                args,
                is_async,
            } => WebDriverCommand::ExecuteScript {
                body,
                args: args
                    .iter()
                    .map(|arg| WebDriverValue::from_json(arg, window))
                    .collect::<Result<_, _>>()?,
                is_async,
            },
            WebDriverRequest::Ready(command) => command,
        })
    }
}

/// The frame a `Machopo` WebDriver method is for (the top-level one if none).
fn webdriver_frame(method: &Method) -> Option<String> {
    match method {
        Method::MachopoExecuteScript(params) => params.frame.clone(),
        Method::MachopoFindElements(params) => params.frame.clone(),
        Method::MachopoGetElementRect(params) | Method::MachopoGetElementTagName(params) => {
            params.frame.clone()
        }
        Method::MachopoGetElementAttribute(params) | Method::MachopoGetElementProperty(params) => {
            params.frame.clone()
        }
        Method::MachopoSwitchToFrame(params) => params.frame.clone(),
        Method::MachopoGetElementClickPoint(params) => params.frame.clone(),
        Method::MachopoCaptureElementScreenshot(params) => params.frame.clone(),
        Method::MachopoGetFrameViewport(params) => params.frame.clone(),
        Method::MachopoPrepareSendKeys(params) => params.frame.clone(),
        Method::MachopoClearElement(params) => params.frame.clone(),
        _ => None,
    }
}

/// The renderer's WebDriver work `method` asks for, if it is a `Machopo`
/// WebDriver method; an error for a strategy it can't find with, or a
/// frame picked two ways.
fn webdriver_command(method: &Method) -> Option<Result<WebDriverRequest, WebDriverError>> {
    let ready = |command| Some(Ok(WebDriverRequest::Ready(command)));
    match method {
        Method::MachopoExecuteScript(params) => Some(Ok(WebDriverRequest::Script {
            body: params.script.clone(),
            args: params.args.clone(),
            is_async: params.is_async,
        })),
        Method::MachopoFindElements(params) => Some(match Locator::from_name(&params.using) {
            Ok(Some(using)) => Ok(WebDriverRequest::Ready(WebDriverCommand::FindElements {
                using,
                value: params.value.clone(),
                from: params.element.clone(),
            })),
            Ok(None) => Err(WebDriverError::new(
                ErrorKind::UnsupportedOperation,
                format!("machopo doesn't find elements by {}", params.using),
            )),
            Err(e) => Err(e),
        }),
        Method::MachopoGetElementRect(params) => ready(WebDriverCommand::ElementRect {
            element: params.element.clone(),
        }),
        Method::MachopoGetElementTagName(params) => ready(WebDriverCommand::ElementTagName {
            element: params.element.clone(),
        }),
        Method::MachopoGetElementAttribute(params) => ready(WebDriverCommand::ElementAttribute {
            element: params.element.clone(),
            name: params.name.clone(),
        }),
        Method::MachopoGetElementProperty(params) => ready(WebDriverCommand::ElementProperty {
            element: params.element.clone(),
            name: params.name.clone(),
        }),
        Method::MachopoSwitchToFrame(params)
            if params.index.is_some() && params.element.is_some() =>
        {
            Some(Err(WebDriverError::new(
                ErrorKind::InvalidArgument,
                "a frame by its index or by its element, not both",
            )))
        }
        Method::MachopoSwitchToFrame(params) => ready(WebDriverCommand::SwitchToFrame {
            index: params.index,
            element: params.element.clone(),
        }),
        Method::MachopoGetElementClickPoint(params) => ready(WebDriverCommand::ElementClickPoint {
            element: params.element.clone(),
            in_view_center_only: params.in_view_center_only,
        }),
        Method::MachopoGetFrameViewport(_) => ready(WebDriverCommand::FrameViewport),
        Method::MachopoPrepareSendKeys(params) => ready(WebDriverCommand::ElementSendKeys {
            element: params.element.clone(),
            text: params.text.clone(),
        }),
        Method::MachopoClearElement(params) => ready(WebDriverCommand::ElementClear {
            element: params.element.clone(),
        }),
        Method::MachopoCaptureElementScreenshot(params) => {
            ready(WebDriverCommand::ElementScreenshotRect {
                element: params.element.clone(),
            })
        }
        _ => None,
    }
}

/// CDP's modifier bits as the renderer takes them.
fn modifiers(bits: u8) -> Modifiers {
    Modifiers {
        shift: bits & automation::SHIFT != 0,
        ctrl: bits & automation::CTRL != 0,
        alt: bits & automation::ALT != 0,
        meta: bits & automation::META != 0,
    }
}

/// `Input.dispatchKeyEvent`'s event as the renderer takes it: CDP's
/// `keyDown` is Chromium's `kRawKeyDown` and its `kChar` in one, which
/// the renderer's key is; a `rawKeyDown` types nothing.
fn key_event(params: &DispatchKeyEvent) -> Result<InputEvent, Error> {
    let down = match params.kind.as_str() {
        "keyDown" | "rawKeyDown" => true,
        "keyUp" => false,
        other => return Err(invalid(&format!("no key event of type {other:?}"))),
    };
    if params.location > 3 {
        return Err(invalid("a key's location is 0 to 3"));
    }
    let text = if params.kind == "keyDown" {
        params.text.clone()
    } else {
        String::new()
    };
    Ok(InputEvent::Key(KeyboardEvent {
        down,
        key: params.key.clone(),
        code: params.code.clone(),
        key_code: params.windows_virtual_key_code,
        text,
        location: params.location,
        repeat: params.auto_repeat,
        modifiers: modifiers(params.modifiers),
    }))
}

/// `Input.dispatchMouseEvent`'s event as the renderer takes it.
fn mouse_event(params: &DispatchMouseEvent) -> Result<InputEvent, Error> {
    let modifiers = modifiers(params.modifiers);
    let button = match params.button.as_deref() {
        None | Some("none") => None,
        Some("left") => Some(MouseButton::Left),
        Some("middle") => Some(MouseButton::Middle),
        Some("right") => Some(MouseButton::Right),
        Some(other) => return Err(invalid(&format!("machopo has no button {other:?}"))),
    };
    let (x, y, buttons, click_count) = (params.x, params.y, params.buttons, params.click_count);
    let pressed = |button: Option<MouseButton>| {
        button.ok_or_else(|| invalid("a press or a release needs its button"))
    };
    match params.kind.as_str() {
        "mouseMoved" if button.is_none() => Ok(InputEvent::MouseMove {
            x,
            y,
            buttons,
            modifiers,
        }),
        "mouseMoved" => Err(invalid(
            "machopo moves the pointer with no button: the ones down are `buttons`",
        )),
        "mousePressed" => Ok(InputEvent::MouseDown {
            x,
            y,
            button: pressed(button)?,
            buttons,
            modifiers,
            click_count,
        }),
        "mouseReleased" => Ok(InputEvent::MouseUp {
            x,
            y,
            button: pressed(button)?,
            buttons,
            modifiers,
            click_count,
        }),
        "mouseWheel" => Ok(InputEvent::MouseWheel {
            x,
            y,
            delta: ScrollDelta::Pixels {
                x: params.delta_x,
                y: params.delta_y,
            },
            buttons,
            modifiers,
        }),
        other => Err(invalid(&format!("no mouse event of type {other:?}"))),
    }
}

/// The rect a WebDriver value `{x, y, width, height}` is.
fn rect_of(value: &WebDriverValue) -> Option<RectF> {
    let WebDriverValue::Object(properties) = value else {
        return None;
    };
    let get = |key: &str| {
        properties.iter().find_map(|(name, value)| match value {
            WebDriverValue::Number(n) if name == key => Some(f32_from_f64(*n)),
            _ => None,
        })
    };
    Some(RectF::new(
        get("x")?,
        get("y")?,
        get("width")?,
        get("height")?,
    ))
}

/// Asks Viz for a copy of the next frame it draws, for `capture`: its
/// tab's view, or the part `clip` of it, in physical px, as a PNG in
/// base64 (encoded on Viz's thread).
fn copy_page(browser: &BrowserWindow, capture: PageCapture) {
    let PageCapture { tab, reply, clip } = capture;
    let view = browser.view();
    let Some(bounds) = view.web_view_bounds(tab) else {
        return reply.answer(Err(Error::new(SERVER_ERROR, "the tab isn't shown")));
    };
    let element = clip.is_some();
    let area = match clip {
        Some(clip) => clip.offset(bounds.x, bounds.y).intersect(&bounds),
        None => bounds,
    };
    let scale = view.scale();
    // Negative is 0, as before an edge was clamped to the frame.
    let edge = |v: f32| clamp_round::<u32>(v * scale);
    let (left, top, right, bottom) = (
        edge(area.x),
        edge(area.y),
        edge(area.right()),
        edge(area.bottom()),
    );
    let unable = move |message: &str| {
        if element {
            WebDriverError::new(ErrorKind::UnableToCaptureScreen, message).into()
        } else {
            Error::new(SERVER_ERROR, message)
        }
    };
    if area.is_empty() || right <= left || bottom <= top {
        return reply.answer(Err(unable("nothing of the page to capture")));
    }
    let reply = std::sync::Mutex::new(Some(reply));
    let request = CopyOutputRequest::new(move |result: Option<CopyOutputResult>| {
        let Some(reply) = reply.lock().ok().and_then(|mut reply| reply.take()) else {
            return;
        };
        let Some(frame) = result else {
            return reply.answer(Err(unable("this window can't read back what it draws")));
        };
        let (right, bottom) = (right.min(frame.size.width), bottom.min(frame.size.height));
        if right <= left || bottom <= top {
            return reply.answer(Err(unable("the page is outside the frame drawn")));
        }
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "left < right and top < bottom (checked above); a row of the frame \
                      is in memory"
        )]
        let (size, stride, row_bytes) = (
            gfx::Size::new(right - left, bottom - top),
            frame.size.width as usize * 4,
            (right - left) as usize * 4,
        );
        let mut pixels = Vec::with_capacity(size.checked_byte_size().unwrap_or(0));
        for row in top..bottom {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "row < bottom <= the frame's height and left < right <= its width: \
                          within its pixels"
            )]
            let start = row as usize * stride + left as usize * 4;
            pixels.extend_from_slice(&frame.pixels[start..][..row_bytes]);
        }
        let mut png = Vec::new();
        if let Err(e) = crate::headless::encode_png(&mut png, size, &pixels) {
            return reply.answer(Err(unable(&format!("encoding the PNG: {e}"))));
        }
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        reply.answer(Ok(if element {
            json!({ "value": data })
        } else {
            json!({ "data": data })
        }));
    });
    let _ = browser
        .viz()
        .send(VizMsg::Display(DisplayMsg::RequestCopyOfOutput(request)));
}

/// Asks Viz for a copy of the next frame it draws, the whole window's, for
/// `reply` (`Machopo.captureWindow`'s result, encoded on Viz's thread).
fn copy_output(browser: &BrowserWindow, reply: Reply) {
    let reply = std::sync::Mutex::new(Some(reply));
    let request = CopyOutputRequest::new(move |result: Option<CopyOutputResult>| {
        let Some(reply) = reply.lock().ok().and_then(|mut reply| reply.take()) else {
            return;
        };
        reply.answer(match result {
            Some(frame) => Ok(serde_json::to_value(CaptureResult {
                width: frame.size.width,
                height: frame.size.height,
                data: base64::engine::general_purpose::STANDARD.encode(&frame.pixels),
            })
            .expect("serializes")),
            None => Err(Error::new(
                SERVER_ERROR,
                "this window can't read back what it draws",
            )),
        });
    });
    let _ = browser
        .viz()
        .send(VizMsg::Display(DisplayMsg::RequestCopyOfOutput(request)));
}
