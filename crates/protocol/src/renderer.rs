//! Browser <-> Renderer messages.

use crate::BigBuffer;
use gfx::{Color, RectF, Size};
use serde::{Deserialize, Serialize};

use crate::input::InputEvent;
use crate::network::{CompletionStatus, LoadResult, LoadTiming, RedirectInfo, ResourceRequest};

/// What a navigation's load took, as far as its head (Chromium's
/// `URLResponseHead`, the part DevTools' `Network.responseReceived` tells):
/// the bytes its head took on the wire, and its status, which the page's
/// log tells of when it is 400 or more. With what Navigation Timing reads
/// of it (Chromium's `NavigationTiming` in `CommitNavigationParams`). The
/// bytes its body took and when it ended come once it has
/// ([`CompletionStatus`], `URLLoaderCompletionStatus`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationLoad {
    pub status: u16,
    pub status_text: String,
    pub encoded_headers_length: u64,
    /// When the parts of the last request happened (after the redirects).
    pub timing: Option<LoadTiming>,
    /// The redirects it followed, in order.
    pub redirects: Vec<RedirectHop>,
    /// How it came (`ResourceResponse::protocol`): `http/1.1`, `h2`...
    pub protocol: String,
    /// The response's `Content-Encoding`, as sent, if any.
    pub content_encoding: Option<String>,
    /// How it ended, if it had by the time it committed; else
    /// [`RendererMsg::NavigationLoadCompleted`] tells later.
    pub end: Option<CompletionStatus>,
}

/// One redirect of a navigation: the URL it was the response to, when the
/// parts of that request happened, and the redirect's
/// `Timing-Allow-Origin`, as sent (Resource Timing's TAO check reads it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectHop {
    pub url: String,
    pub timing: Option<LoadTiming>,
    pub timing_allow_origin: Option<String>,
}

impl RedirectHop {
    /// `redirect`'s hop.
    pub fn of(redirect: &RedirectInfo) -> Self {
        Self {
            url: redirect.response.url.clone(),
            timing: redirect.response.timing,
            timing_allow_origin: redirect
                .response
                .headers
                .iter()
                .find(|(name, _)| name == "timing-allow-origin")
                .map(|(_, value)| value.clone()),
        }
    }
}

/// How a navigation came about (Blink's `WebFrameLoadType`): what
/// Navigation Timing tells (`PerformanceNavigationTiming.type`: both reloads
/// are `reload`), and how the load and the document's subresources use the
/// HTTP cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrameLoadType {
    /// A new document: a link, a URL typed, `location`.
    Standard,
    /// Back or forward: what is in the cache is used, stale or not.
    BackForward,
    /// The document loaded again: validated with the server.
    Reload,
    /// Loaded again past the HTTP cache (Shift+Reload, DevTools'
    /// `ignoreCache`): its subresources too, until its `load`.
    ReloadBypassingCache,
}
use crate::viz::{BeginFrameArgs, LocalSurfaceId};
use crate::webdriver::{WebDriverCommand, WebDriverError, WebDriverValue};

/// Widget geometry pushed from the browser (`blink::VisualProperties`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct VisualProperties {
    /// Viewport size in physical pixels.
    pub viewport_size: Size,
    pub device_scale_factor: f32,
    /// Allocated by the browser (the embedder) for every geometry change;
    /// the renderer tags the frames it produces for this geometry with it.
    pub local_surface_id: LocalSurfaceId,
}

impl Default for VisualProperties {
    fn default() -> Self {
        Self {
            viewport_size: Size::new(800, 600),
            device_scale_factor: 1.0,
            local_surface_id: LocalSurfaceId::default(),
        }
    }
}

/// What the session history has of the entry a commit shows, and where it
/// is (Chromium's `CommitNavigationParams`: the entry's `PageState`,
/// `current_history_list_offset` and `current_history_list_length`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntryParams {
    /// The classic history API's state, as the renderer's engine serialized
    /// it; `None` is `null`.
    pub state: Option<BigBuffer>,
    /// Its scroll restoration mode is `"manual"`.
    pub manual_scroll_restoration: bool,
    /// The entry's index, and how many entries there are.
    pub index: u32,
    pub length: u32,
    /// A commit's (`CommitNavigation`): how many of the renderer's own new
    /// entries the browser had heard of as it worked `index` and `length`
    /// out, as `SetHistoryOffsetAndLength` says. A traversal within the
    /// document takes its place as it is: the new entries the browser hears
    /// of before it is in come before it, and the browser says where things
    /// are once it is.
    pub entries_heard: u64,
}

/// What a DevTools session has on in a renderer, as the renderer wrote it
/// (Chromium's `blink::mojom::DevToolsSessionState`, the session's state
/// cookie): the browser keeps it without reading it, and gives it to the
/// next renderer the session is attached to, which goes on from there.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DevToolsSessionState {
    /// The renderer's own agents' (`Page`, `Network`, `Log`, `DOM`, `CSS`,
    /// `Overlay`), each by its domain, as JSON.
    pub agents: Vec<(String, String)>,
    /// The script engine's inspector's, as the engine wrote it (V8's
    /// session state).
    pub engine: Option<BigBuffer>,
}

impl DevToolsSessionState {
    /// The most bytes a renderer may give for one: far more than its
    /// agents' switches and settings and V8's take.
    pub const MAX_BYTES: usize = 1 << 20;

    /// The bytes it takes, as counted against [`Self::MAX_BYTES`].
    pub fn bytes(&self) -> usize {
        self.agents
            .iter()
            .map(|(domain, state)| domain.len().saturating_add(state.len()))
            .fold(0, usize::saturating_add)
            .saturating_add(self.engine.as_ref().map_or(0, |engine| engine.len()))
    }
}

/// A storage area as the browser opens it for a renderer: its number
/// (each renderer's own, one for each storage key it has opened), and its
/// items, if the renderer didn't have the area already.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenedStorageArea {
    pub area: u64,
    pub items: Option<Vec<(String, String)>>,
}

/// How many bytes an area's items may take (`kPerStorageAreaQuota`),
/// counted as `storage_quota_for` counts them: what the renderer allows
/// script.
pub const PER_STORAGE_AREA_QUOTA: usize = 10 * 1024 * 1024;

/// How many bytes more the browser allows an area
/// (`kPerStorageAreaOverQuotaAllowance`): renderers that wrote at once, each
/// within the quota as it saw the area.
pub const PER_STORAGE_AREA_OVER_QUOTA_ALLOWANCE: usize = 100 * 1024;

/// What an item of `key` and `value` takes of its area's quota: 2 bytes a
/// UTF-16 code unit of each (Blink's `QuotaForString`).
pub fn storage_quota_for(key: &str, value: &str) -> usize {
    key.encode_utf16()
        .count()
        .saturating_add(value.encode_utf16().count())
        .saturating_mul(2)
}

/// Which storage an area is of: `localStorage`'s (the profile's) or
/// `sessionStorage`'s (the tab's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum StorageKind {
    Local,
    Session,
}

/// A change a renderer made to a storage area (`StorageArea::Put`,
/// `Delete`, `DeleteAll`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageChange {
    Set { key: String, value: String },
    Remove { key: String },
    Clear,
}

/// The URL an error page's document has (Chromium's
/// `kUnreachableWebDataURL`): a scheme no one loads, so its origin is
/// opaque, and no other document is of it. The browser's address bar and
/// session history keep the URL that failed.
pub const ERROR_PAGE_URL: &str = "chrome-error://chromewebdata/";

/// Browser -> Renderer (`content.mojom.Frame`, `blink.mojom.Widget`).
/// Not `Clone`: a commit carries its body's data pipe.
#[derive(Debug, Serialize, Deserialize)]
pub enum RendererMsg {
    /// `CommitNavigation`: the browser has the response's head and the
    /// start of its body, and frame `frame` should show it.
    CommitNavigation {
        frame: crate::FrameToken,
        /// The loader the document's subresources go through (Chromium's
        /// `subresource_loader_factories`, bound with the browser's params
        /// for the document): its number, for each `FetchResource` of it.
        loader: u64,
        /// Increases with every navigation the browser starts. Responses
        /// arrive in any order, so the renderer drops a commit older than
        /// the one it shows.
        navigation_id: u64,
        /// The renderer's own number for it (`BeginNavigation`'s), if the
        /// renderer asked for it: a frame that asked again since takes
        /// only the later one's.
        begin_id: Option<u64>,
        /// The final URL, after redirects; a failed navigation's URL, for
        /// an error page.
        url: String,
        /// The document is the browser's error page for `url`: it commits
        /// at `ERROR_PAGE_URL` (an opaque origin, so that no other document
        /// may read why the load failed), and tells the browser of `url`.
        error_page: bool,
        mime_type: String,
        /// The `charset` of the response's `Content-Type`, if any.
        charset: Option<String>,
        /// The response's `Content-Language`, as sent, if any (Chromium's
        /// `ResourceResponse::HttpContentLanguage`): the document's content
        /// language comes from it.
        content_language: Option<String>,
        /// The body's first bytes (1024, or all of it if it is shorter),
        /// or the whole body if it has no `body_pipe`.
        body: BigBuffer,
        /// The rest of the body, as it comes (Chromium's `response_body`,
        /// a data pipe's consumer handle): a document that came over the
        /// network has one, which may be over already; a page of the
        /// browser's own, an error page or a blob's comes whole. To a
        /// renderer in a process of its own the pipe crosses with the
        /// message: its bytes go as messages of the channel's, into a pipe
        /// of that process, whose consumer is what is read here.
        body_pipe: Option<ipc::data_pipe::Consumer>,
        /// The entry it commits to, as the navigation started.
        history: HistoryEntryParams,
        /// When the navigation started (`CommonNavigationParams::
        /// navigation_start`), on the clock both processes read, as the load's
        /// times are: the document's time origin. (How long ago when the
        /// message was sent would put it later by as long as the commit waits
        /// in the renderer's queue.)
        navigation_start: ipc::TimeTicks,
        /// What its load took, for DevTools and Navigation Timing, if the
        /// document came over the network (not a page of the browser's
        /// own, or an error page).
        /// Boxed: most messages are far smaller.
        loaded: Option<Box<NavigationLoad>>,
        /// How the navigation came about.
        navigation_type: FrameLoadType,
    },
    /// `URLLoaderClient::OnComplete` of the load of the document navigation
    /// `navigation_id` committed in `frame`, which hadn't ended when it
    /// committed: its body is all in its pipe, or stopped short with an
    /// error. Chromium's network service tells the renderer directly; here
    /// the browser, which keeps the load, passes it on.
    NavigationLoadCompleted {
        frame: crate::FrameToken,
        navigation_id: u64,
        status: CompletionStatus,
    },
    /// `NavigationClient`'s disconnection, as Chromium's renderer hears it
    /// (`RenderFrameImpl::OnDroppedNavigation`): the navigation of `frame`
    /// the renderer asked for as `id` (`BeginNavigation`'s) won't commit,
    /// the browser didn't take it. The frame no longer waits for it.
    NavigationDropped { frame: crate::FrameToken, id: u64 },
    /// `CommitSameDocumentNavigation` of a history traversal: back or
    /// forward to an entry of the document `frame` shows, `document` (the
    /// `navigation_id` that committed it). The renderer changes the URL,
    /// the state and its place in the session history, and fires
    /// `popstate`, and `hashchange` if the fragment changed; answered with
    /// `DidCommitSameDocumentNavigation` carrying `navigation_id`, or
    /// `SameDocumentNavigationFailed` if it shows another document by then
    /// (Chromium's `CommitResult::RestartCrossDocument`).
    CommitSameDocumentNavigation {
        frame: crate::FrameToken,
        navigation_id: u64,
        document: u64,
        url: String,
        history: HistoryEntryParams,
    },
    /// `SetHistoryOffsetAndLength`: where the document's entry is in the
    /// session history now, and how many entries there are, once its
    /// commit is in. `entries_heard`: how many of the renderer's own new
    /// entries (each a `DidCommitSameDocumentNavigation` neither `replace`
    /// nor a traversal: `pushState`, a fragment's navigation) the browser
    /// had heard of then; those it hadn't come after, and the renderer
    /// counts them on (a place it takes as it is would drop them, those
    /// script made as the place was on its way).
    SetHistoryOffsetAndLength {
        index: u32,
        length: u32,
        entries_heard: u64,
    },
    /// A `FetchResource`'s load reached a redirect and waits there
    /// (`URLLoaderClient::OnReceiveRedirect`) until the renderer answers
    /// `FollowRedirect`.
    ReceivedRedirect {
        request_id: u64,
        /// The dedicated worker whose request it is (`FetchResource`'s);
        /// none: the main thread's.
        worker: Option<u64>,
        /// Boxed: a response's head, much bigger than the other messages.
        redirect: Box<RedirectInfo>,
    },
    /// The answer to a `FetchResource` (`URLLoaderClient::OnComplete`).
    ResourceResponse {
        request_id: u64,
        /// As `ReceivedRedirect`'s.
        worker: Option<u64>,
        result: LoadResult,
    },
    /// The main script of the dedicated worker `worker`
    /// (`DedicatedWorkerCreated`'s), loaded, or why it wasn't (Chromium's
    /// `DedicatedWorkerHost`'s `WorkerScriptFetcher`).
    DedicatedWorkerScript { worker: u64, result: LoadResult },
    /// The answer to `RendererHostMsg::OpenStorageArea` (`StorageArea`'s
    /// binding and its `GetAll`): the frame's area, or none if its
    /// document may have none (an opaque origin's) or the frame has gone.
    /// Not a task of the main thread's: it waits for it alone, every other
    /// task left queued in order (a `[Sync]` call's reply).
    StorageAreaOpened {
        request_id: u64,
        area: Option<OpenedStorageArea>,
    },
    /// The answer to `RendererHostMsg::GetCookies` (`[Sync]
    /// GetCookiesString`): the `Cookie` string script sees, empty for none.
    /// Not a task: the main thread waits for it alone, as for
    /// `StorageAreaOpened`.
    Cookies { request_id: u64, cookies: String },
    /// The answer to `RendererHostMsg::CookiesEnabled` (`[Sync]
    /// CookiesEnabledFor`). Not a task, as `Cookies`.
    CookiesEnabled { request_id: u64, enabled: bool },
    /// The answer to `RendererHostMsg::GetFontFamily`: the family's faces,
    /// none if it has none the browser could give. Not a task, as
    /// `Cookies`.
    FontFamily {
        request_id: u64,
        faces: Option<crate::font::FontFamilyFaces>,
    },
    /// The answer to `RendererHostMsg::GetFontFallback`: the families, by
    /// number. Not a task, as `Cookies`.
    FontFallback { request_id: u64, families: Vec<u32> },
    /// The answer to `RendererHostMsg::GetFontFiles`: each file asked for,
    /// open to read, in order; none for one the browser couldn't open. Not
    /// a task, as `Cookies`.
    FontFiles {
        request_id: u64,
        files: Vec<Option<ipc::file::ReadOnlyFile>>,
    },
    /// A change to the `localStorage` area `area`, which this renderer
    /// opened (`StorageAreaObserver::KeyChanged`, `KeyDeleted`,
    /// `AllDeleted`), in the order the browser made them: the value before
    /// (none for `Clear`, or a key that had none), whether it changed
    /// anything, and the URL of the document that made it. `source`: the
    /// renderer's own number for the `Storage` that made it, if this
    /// renderer did (`ChangeStorage`'s, which it has made in its copy
    /// already).
    StorageAreaChanged {
        area: u64,
        change: StorageChange,
        old_value: Option<String>,
        changed: bool,
        url: String,
        source: Option<u64>,
    },
    /// The browser refused this renderer's change to the `localStorage`
    /// area `area` (`ChangeStorage`'s, the first it hasn't told back): the
    /// item would have grown past the area's quota and allowance (another
    /// renderer filled it meanwhile). `key` is `value` in the browser's
    /// area (`StorageAreaObserver::KeyChangeFailed`).
    StorageAreaChangeFailed {
        area: u64,
        key: String,
        value: Option<String>,
    },
    /// `UpdateVisualProperties`.
    UpdateVisualProperties(VisualProperties),
    /// `blink.mojom.Widget::UpdateScreenRects`: where the browser window
    /// is on the screen and how big, CSS px (Chromium's
    /// `window_screen_rect`, in DIPs), which `outerWidth` and `outerHeight`
    /// answer for every frame of the tab.
    UpdateScreenRects { window_screen_rect: RectF },
    /// With `RendererFlags::automation`: the first element of the document
    /// shown that matches `selector`, answered with `QueryElementResult`
    /// (DevTools' `DOM.querySelector` and `DOM.getBoxModel`). Ignored
    /// otherwise, so no page can learn of it. With `frames`, of the child
    /// frame of the first iframe matching each in turn (DevTools'
    /// `DOM.describeNode` of an iframe's `contentDocument`); its box is
    /// still in the viewport of the document shown.
    QueryElement {
        query_id: u64,
        frames: Vec<String>,
        selector: String,
    },
    /// With `RendererFlags::automation`: if the first element matching
    /// `selector` has a box whose center is out of the viewport, scroll it
    /// into view at the next main frame, as a link to `#fragment` does
    /// (DevTools' `DOM.scrollIntoViewIfNeeded`, which ChromeDriver uses
    /// before a click). Not answered: a `RequestVisualState` after it is
    /// answered once the frame showing the scroll has gone to Viz. With
    /// `frames`, of a child frame as `QueryElement`'s: it scrolls in its
    /// frame, and so does each iframe around it, out to the document shown.
    ScrollIntoViewIfNeeded {
        frames: Vec<String>,
        selector: String,
    },
    /// With `RendererFlags::automation`: answer with `VisualStateReady`
    /// once a frame showing what the document has changed so far has gone
    /// to Viz (Chromium's `VisualStateRequest`). The renderer asks for a
    /// main frame and answers once it has swapped, even with nothing new to
    /// draw: the last commit may not have reached Viz yet.
    RequestVisualState { request_id: u64 },
    /// With `RendererFlags::web_tests`: once the main thread has no task
    /// and wants no frame (what the input before ran, and the tasks it
    /// left), answer with `IdleFrameReady` once a main frame asked for
    /// then has gone to Viz. A frame that shows what input led to, for the
    /// test harness: its ack's frame may come before tasks the input
    /// queued (a link's `hashchange`), as a main frame is asked for as a
    /// task ends.
    RequestIdleFrame,
    /// With `RendererFlags::automation`: WebDriver's work in a frame of the
    /// tab (`frame`, a frame id `WebDriverCommand::SwitchToFrame` gave; the
    /// top-level one if none), answered with `WebDriverReply` (a script
    /// once it settles). Ignored otherwise, as `QueryElement` is.
    WebDriver {
        request_id: u64,
        frame: Option<String>,
        command: WebDriverCommand,
    },
    /// A DevTools session of the browser's for the page (Blink's
    /// `DevToolsAgent::AttachDevToolsSession`): its commands go to the
    /// script engine's inspector (V8's), which answers through
    /// `RendererHostMsg::DevToolsMessage`. Whatever the flags: a window a
    /// person uses has DevTools too. With `state`, the session goes on
    /// where it was in the renderer before (the tab has another: Blink's
    /// `reattach_session_state`), as that one told it.
    DevToolsAttach {
        session: String,
        state: Option<DevToolsSessionState>,
    },
    /// The browser's `session` has gone.
    DevToolsDetach { session: String },
    /// A DevTools command, as JSON, for the browser's `session`: one the
    /// browser doesn't answer itself (`Runtime.evaluate`...). With
    /// `interrupt`, the renderer takes it while script runs, as soon as it
    /// can (`Debugger.pause`; Chromium's `DevToolsSession::ShouldSendOnIO`);
    /// else once script is done, or paused.
    DevToolsMessage {
        session: String,
        message: String,
        interrupt: bool,
    },
    /// The id DevTools knows the tab's main frame by, which is its target
    /// id: drawn by the browser when it made the tab (Chromium's
    /// `devtools_main_frame_token`, in the view's creation parameters), and
    /// sent first to every renderer the tab has, so it stays the same.
    DevToolsMainFrameToken(crate::FrameToken),
    /// `blink.mojom.Widget::WasHidden`: the tab isn't shown (another tab is
    /// the current one). The compositor asks for no `BeginFrame`s, so there
    /// are no main frames and no animation frame callbacks.
    WasHidden,
    /// `blink.mojom.Widget::WasShown`: the tab is shown again; it draws.
    WasShown,
    /// `blink.mojom.WidgetInputHandler::SetFocus`: the page took (`true`)
    /// or lost the system's focus: its window became the active one or
    /// not, its tab was selected or not, the address field let go of the
    /// keys or took them. The renderer starts without it.
    SetFocus(bool),
    /// `blink.mojom.LocalFrame::SetInitialFocus`: Tab from the browser's
    /// toolbar moved into the page: its first focusable element takes the
    /// focus (its last, with Shift+Tab, `reverse`).
    SetInitialFocus { reverse: bool },
    /// `blink.mojom.PopupMenuClient`'s `DidAcceptIndices` and `DidCancel`:
    /// the dropdown the renderer asked for (`RendererHostMsg::
    /// ShowPopupMenu`) closed, with its result.
    PopupMenuClosed(PopupMenuResult),
    /// Orderly shutdown of the renderer main thread.
    Shutdown,
    /// The renderer is to crash (`chrome://crash`, which has the renderer
    /// crash itself): only to a renderer in a process of its own, which
    /// ends itself abnormally as soon as it reads this. One that is the
    /// browser's threads can't, and isn't sent it.
    Crash,
}

/// A `<select>`'s dropdown, as the renderer asks the browser to show it
/// (Blink's `ExternalPopupMenu`, `PopupMenuHelper::ShowPopupMenu`'s
/// arguments): the select's list items and where the select is. The
/// browser draws the list itself, in the select's font size and color.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PopupMenu {
    /// The select's border box, CSS px of the page's viewport: the list
    /// goes below it, or above.
    pub bounds: RectF,
    pub font_size: f32,
    /// The select's text color.
    pub color: Color,
    pub items: Vec<MenuItem>,
    /// The item of the option selected now, if any.
    pub selected: Option<u32>,
    /// The select's direction is right to left: the list is aligned to
    /// its right edge.
    pub right_aligned: bool,
}

/// One of a dropdown's items (`blink.mojom.MenuItem`): an option, an
/// optgroup's label, or an `<hr>`'s line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    pub label: String,
    pub kind: MenuItemKind,
    /// An option that can be picked (one disabled, or in a disabled
    /// optgroup, can't).
    pub enabled: bool,
    /// Its `direction` is right to left (`blink.mojom.MenuItem`'s
    /// `text_direction`): its label goes from the right, as a paragraph of
    /// that direction.
    pub rtl: bool,
}

/// `blink.mojom.MenuItem::Type`, the kinds a select's list has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MenuItemKind {
    Option,
    /// An optgroup's label: the options after it, up to the next group,
    /// are in it.
    Group,
    Separator,
}

/// How a dropdown closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PopupMenuResult {
    /// The user picked the item at `index`, with a key (Enter, Tab) or
    /// the mouse.
    Accepted { index: u32, by_keyboard: bool },
    /// It closed without a pick: Escape, a wheel, the window or the tab
    /// changing, or a press outside it (`by_press`), which goes on to the
    /// page and doesn't open the same list again (Blink's
    /// `WebViewImpl::HandleMouseDown`).
    Canceled { by_press: bool },
}

/// An element found by `RendererMsg::QueryElement`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QueriedElement {
    /// The union of its border boxes as last laid out, in the viewport's
    /// CSS px (as `getBoundingClientRect`); `None` if it has no box.
    pub rect: Option<RectF>,
    /// Its `textContent` now.
    pub text: String,
}

/// What a renderer's navigation is (`BeginNavigation`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BeginNavigationKind {
    /// To `url` (a link followed, `location`), its entry taking the
    /// current one's place in the session history if `replace`
    /// (`location.replace`; Chromium's `should_replace_current_entry`).
    To { url: String, replace: bool },
    /// The frame's document loaded again (`location.reload()`), as the
    /// browser's own reload does.
    Reload,
}

/// `WindowOpenDisposition`: where a followed link opens, out of its tab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowOpenDisposition {
    /// A new tab, shown.
    NewForegroundTab,
    /// A new tab, behind the one shown.
    NewBackgroundTab,
}

/// Viz and the browser -> the renderer's compositor thread. Chromium binds
/// these interfaces' pipes on the compositor thread, so they reach it while
/// the main thread is busy: `viz.mojom.CompositorFrameSinkClient` (here) and
/// `blink.mojom.WidgetInputHandler` (here). One channel carries both.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompositorMsg {
    /// Viz has put the pixels of the shared image `mailbox` on the GPU and
    /// holds their buffer no more (the client's `CreateSharedImage`): the
    /// compositor draws another tile into it.
    SharedImagePixelsReturned { mailbox: crate::viz::Mailbox },
    /// `CompositorFrameSinkClient::OnBeginFrame`: a frame is due, if the
    /// renderer asked for one. The compositor answers through its frame
    /// sink, with a frame or `DidNotProduceFrame`, after starting a main
    /// frame if the main thread wants one.
    BeginFrame(BeginFrameArgs),
    /// `WidgetInputHandler::DispatchEvent`. The compositor handles what it
    /// can and hands the rest to the main thread; either way the renderer
    /// answers with `RendererHostMsg::InputEventAck`.
    HandleInputEvent(InputEvent),
    /// The GPU the compositor's tiles were put on (`SharedImageInterface`)
    /// was lost and Viz has a new one: every shared image made before is
    /// gone, and the compositor makes new ones (cc's
    /// `DidLoseLayerTreeFrameSink`, as the GPU channel reports it to the
    /// renderer). The browser hears it from Viz and passes it on.
    ContextLost,
}

/// `ui::mojom::CursorType`: what the pointer looks like, as CSS `cursor`
/// asks (Blink's `EventHandler::SelectCursor`), but for custom images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CursorType {
    /// The arrow (Chromium's name for it): `default`.
    Pointer,
    Cross,
    /// The pointing hand: `pointer`, and over a link.
    Hand,
    /// `text`, and over text that can be selected.
    IBeam,
    Wait,
    Help,
    EastResize,
    NorthResize,
    NorthEastResize,
    NorthWestResize,
    SouthResize,
    SouthEastResize,
    SouthWestResize,
    WestResize,
    NorthSouthResize,
    EastWestResize,
    NorthEastSouthWestResize,
    NorthWestSouthEastResize,
    ColumnResize,
    RowResize,
    /// `move` and `all-scroll`.
    Move,
    VerticalText,
    Cell,
    ContextMenu,
    Alias,
    Progress,
    NoDrop,
    Copy,
    /// No pointer shown.
    None,
    NotAllowed,
    ZoomIn,
    ZoomOut,
    Grab,
    Grabbing,
}

/// The navigation id a document the renderer committed itself (a child
/// frame's `javascript:` `src`'s) is told by (`DidCommitNavigation`): none
/// of the browser's, which count from 1.
pub const RENDERER_COMMIT: u64 = 0;

/// Renderer -> Browser (`content.mojom.FrameHost`, `blink.mojom.WidgetHost`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RendererHostMsg {
    /// Renderer main thread finished initialising (V8 is up).
    RendererReady,
    /// `DidCommitNavigation`: frame `frame` shows the document of the
    /// browser's navigation `navigation_id` (`CommitNavigation`'s), at
    /// `url` (a failed one's, for an error page), or of its own
    /// (`RENDERER_COMMIT`).
    DidCommitNavigation {
        frame: crate::FrameToken,
        navigation_id: u64,
        url: String,
    },
    /// `DidCommitSameDocumentNavigation`: the document frame `frame` shows
    /// navigated within itself, without loading anything (to a fragment: a
    /// link, `location.hash`; `history.pushState`); its URL is now `url`.
    /// `replace`: the session history's current entry takes it, rather
    /// than a new one (`location.replace("#x")`, the same URL again,
    /// `replaceState`). `state`: the entry's (`None` is `null`).
    /// `navigation_id`: the browser's history traversal it was
    /// (`CommitSameDocumentNavigation`), if it was one. A child frame's
    /// isn't in the session history: the browser has its URL. One neither
    /// `replace` nor a traversal is a new entry the renderer counted
    /// itself, which the browser counts too (`entries_heard`).
    DidCommitSameDocumentNavigation {
        frame: crate::FrameToken,
        url: String,
        replace: bool,
        state: Option<BigBuffer>,
        navigation_id: Option<u64>,
    },
    /// The renderer shows another document than the one the browser's
    /// `CommitSameDocumentNavigation` was for: the browser loads the entry
    /// instead.
    SameDocumentNavigationFailed {
        navigation_id: u64,
    },
    /// `FrameHost::GoToEntryAtOffset`: `history.go(offset)` (and `back`,
    /// `forward`), within range as the renderer knew it.
    GoToEntryAtOffset(i32),
    /// `NavigationClient`'s `NotifyNavigationCancellationWindowEnded`: the
    /// task that asked for the renderer's navigations `ids`
    /// (`BeginNavigation`'s) is over, and what it asked of the browser
    /// before them has gone (the loads, the blob URLs it revoked): the
    /// browser may commit them now, which it holds till then (Chromium's
    /// `RendererCancellationThrottle`), so that one the same task replaced
    /// never commits.
    NavigationCancellationWindowEnded {
        ids: Vec<u64>,
    },
    /// `FrameHost::UpdateState`: script in `frame` changed its item's
    /// scroll restoration mode in the current entry
    /// (`history.scrollRestoration`).
    UpdateState {
        frame: crate::FrameToken,
        manual_scroll_restoration: bool,
    },
    /// `FrameHost::BeginNavigation`: a frame of the page is to have
    /// another document, as script or a link in frame `initiator` asked
    /// (HTML's source document's; `frame` itself for a reload). The
    /// browser decides whether, loads it and has the frame commit it; `id`
    /// is the renderer's own number for the request. `devtools_initiator`:
    /// who asked, as DevTools' `Network.Initiator` (JSON), while DevTools
    /// has `Network` on in the renderer (`BeginNavigationParams::
    /// devtools_initiator`); the browser tells DevTools of the request.
    BeginNavigation {
        frame: crate::FrameToken,
        initiator: crate::FrameToken,
        id: u64,
        kind: BeginNavigationKind,
        devtools_initiator: Option<String>,
        /// A new child frame's first navigation, by its iframe, while its
        /// parent's document back or forward loaded is loading (Blink's
        /// `is_history_navigation_in_new_child_frame`): the browser loads
        /// what the session history has for the frame there, if it has
        /// any, instead.
        history_in_new_child_frame: bool,
        /// How the renderer has it use the HTTP cache (Blink's, Chromium's
        /// `BeginNavigationParams::load_flags`): a child frame's by its
        /// parent's document's load (past the cache in a reload that
        /// bypasses it, stale or not back or forward), until that one's
        /// `load`. The browser adds its own by the kind of navigation
        /// (Chromium takes these cache flags of the renderer's,
        /// `kAllowedBeginParamsLoadFlags`: they only touch the frame's own
        /// entries).
        load_flags: crate::network::LoadFlags,
    },
    /// `FrameHost::OpenURL`: a link followed to a new tab, which the
    /// browser decides whether to open.
    OpenUrl {
        url: String,
        disposition: WindowOpenDisposition,
    },
    /// `FrameHost::CreateChildFrame`: the renderer made child frame `frame`
    /// of `parent` (an iframe inserted), at once and on its own, as script
    /// reads its window next. The browser's frame tree takes it. `name`:
    /// its iframe's `name` then (its name from now on; empty without one);
    /// `created_by_script`: script running inserted it (not the parser).
    /// What its place in the session history goes by comes of them.
    DidCreateChildFrame {
        parent: crate::FrameToken,
        frame: crate::FrameToken,
        name: String,
        created_by_script: bool,
    },
    /// `FrameHost::DidChangeSrcDoc`: child frame `frame`'s iframe's `srcdoc`
    /// is `srcdoc` now (empty without one), as the frame was made and as
    /// the attribute changes: the browser keeps it, for the frame's
    /// `about:srcdoc` documents to commit.
    DidChangeSrcDoc {
        frame: crate::FrameToken,
        srcdoc: String,
    },
    /// `FrameHost::Detach`: child frame `frame` has gone, and the frames in
    /// it with it: its iframe was removed, or the document it was in was
    /// navigated from (once for each frame that document had). The only
    /// way the browser's frame tree loses a frame, but for a renderer that
    /// has gone.
    DidDetachFrame {
        frame: crate::FrameToken,
    },
    /// `WidgetHost::SetCursor`: what the pointer looks like over the page.
    SetCursor(CursorType),
    /// `ChromeClient::OpenPopupMenu` with an `ExternalPopupMenu`: a
    /// collapsed select's list, which the browser shows over the page
    /// until it is closed (`RendererMsg::PopupMenuClosed`) or hidden. A
    /// new one replaces the one shown.
    ShowPopupMenu(PopupMenu),
    /// The select whose list is shown went away (lost the focus, was
    /// removed, its document went): the browser closes the list, and
    /// sends no result.
    HidePopupMenu,
    /// `LocalFrameHost::TakeFocus` (`WebContentsDelegate::TakeFocus`): Tab
    /// went past the page's last focusable element (Shift+Tab, `reverse`,
    /// past its first): the browser takes the focus back.
    TakeFocus {
        reverse: bool,
    },
    /// `HandleInputEvent`'s callback: the renderer has handled an input
    /// event. What it sent because of the event (a same-document
    /// navigation, a link to open) came before. `frame_token` is the first
    /// frame that shows what the event did, sent before this ack; `None`
    /// if the event changed nothing to draw (or was dropped, aimed at a
    /// document no longer shown). Unlike Chromium's ack, which comes as
    /// soon as the event is handled, this one waits for that frame, so a
    /// caller knows which frame to look at.
    InputEventAck {
        frame_token: Option<u64>,
    },
    UpdateTitle(String),
    /// A subresource load (Blink's `ResourceFetcher` through a
    /// `URLLoaderFactory`). Answered with `RendererMsg::ResourceResponse`,
    /// after a `RendererMsg::ReceivedRedirect` for each redirect.
    /// Handled on the IO thread; it never reaches the UI thread.
    FetchResource {
        request_id: u64,
        /// The dedicated worker that asks (its thread sends it, its answer
        /// goes to it: Chromium gives each worker a `URLLoaderFactory` of
        /// its own); none: the main thread. One the browser doesn't know as
        /// the renderer's (`DedicatedWorkerCreated`) is refused.
        worker: Option<u64>,
        /// The loader it goes through: its document's (`CommitNavigation`'s
        /// `loader`), or the worker's (`DedicatedWorkerCreated`'s). Its
        /// request's `initiator` is to be the loader's origin, and its
        /// cookies are the loader's site's: one the browser doesn't know as
        /// the renderer's, or that isn't the worker's, is a bad message.
        loader: u64,
        request: ResourceRequest,
    },
    /// Closing the `URLLoader` of a `FetchResource` still loading (an
    /// aborted `fetch()`, a discarded document's): no answer comes. Handled
    /// on the IO thread too.
    CancelResource {
        request_id: u64,
        worker: Option<u64>,
    },
    /// `URLLoader::FollowRedirect`: the load of a `FetchResource` waiting
    /// at the redirect it told of (`RendererMsg::ReceivedRedirect`) goes
    /// on. Ignored for a load that isn't there (cancelled, or answered).
    /// Handled on the IO thread too.
    FollowRedirect {
        request_id: u64,
        worker: Option<u64>,
    },
    /// A dedicated worker the renderer made (`new Worker()`, Chromium's
    /// `DedicatedWorkerHostFactory::CreateWorkerHost`): the browser knows it
    /// as the renderer's from now on (its requests are let through), and
    /// loads its main script, `request`, answering
    /// `RendererMsg::DedicatedWorkerScript`. Handled on the IO thread.
    DedicatedWorkerCreated {
        worker: u64,
        /// The loader of the document (or worker) that made it, which its
        /// loads go through (Chromium's `DedicatedWorkerHost` takes its
        /// creator's).
        loader: u64,
        request: ResourceRequest,
    },
    /// The dedicated worker `worker` is no more (terminated, closed, its
    /// document gone): its loads closed, its requests refused. Handled on
    /// the IO thread.
    DedicatedWorkerGone {
        worker: u64,
    },
    /// `BlobURLStore::Register` (`URL.createObjectURL`): `url` stands for
    /// a blob of `bytes` and `mime_type` (its type, maybe empty), for this
    /// renderer's subresources and for any navigation, until revoked or
    /// the renderer goes, or the dedicated worker that registered it
    /// (`worker`, its thread sending it; none: the main thread's) does
    /// (`DedicatedWorkerGone`; one the browser doesn't know as the
    /// renderer's registers nothing). Sent before any load of it, on the
    /// same channel. Handled on the IO thread.
    RegisterBlobUrl {
        worker: Option<u64>,
        url: String,
        mime_type: String,
        bytes: BigBuffer,
    },
    /// `localStorage` or `sessionStorage` (`kind`) first used in frame
    /// `frame`'s document (Chromium's `StorageArea` bound for the frame's
    /// storage key, and its tab's namespace for `sessionStorage`, then
    /// `GetAll`): the browser works out the key from the frame, its
    /// document committed before this on the channel, and answers
    /// `StorageAreaOpened` with the same `request_id`, which the main
    /// thread waits for. Handled on the UI thread, which has the frames.
    OpenStorageArea {
        frame: crate::FrameToken,
        kind: StorageKind,
        request_id: u64,
    },
    /// `document.cookie`'s getter, through the document's loader
    /// (`RestrictedCookieManager::GetCookiesString`, the frame's): the
    /// cookies of `url`, the document's cookie URL, that script sees, by
    /// the loader's trusted params. Answered with `Cookies` of the same
    /// `request_id`, which the main thread waits for. Handled on the IO
    /// thread: `url` of another origin than the loader's is a bad message.
    GetCookies {
        loader: u64,
        url: String,
        request_id: u64,
    },
    /// `document.cookie`'s setter (`SetCookieFromString`): `line` set as
    /// script sets it, at `url`, through `loader`, as `GetCookies`; not
    /// answered. A later `GetCookies` sees it: they go the same way.
    SetCookie {
        loader: u64,
        url: String,
        line: String,
    },
    /// `navigator.cookieEnabled` (`CookiesEnabledFor`): whether the cookie
    /// policy lets the document at `url` have unpartitioned cookies,
    /// through `loader`, as `GetCookies`. Answered with `CookiesEnabled`.
    CookiesEnabled {
        loader: u64,
        url: String,
        request_id: u64,
    },
    /// The faces of the system's font family `family` (a number of the
    /// `FontNames` the renderer started with), the first time its text
    /// needs the family (`FontDataService::MatchFamilyName`). Answered with
    /// `FontFamily` of the same `request_id`, which the main thread waits
    /// for. Checked on the IO thread (a family the renderer wasn't told
    /// of is a bad message), answered by the browser's font thread.
    GetFontFamily {
        family: u32,
        request_id: u64,
    },
    /// The families text of `script` (an ISO 15924 tag) falls back to, the
    /// first time the renderer's text has a character of it
    /// (`FontDataService::MatchFamilyNameCharacter`, by script). Answered
    /// with `FontFallback`, as `GetFontFamily`.
    GetFontFallback {
        script: [u8; 4],
        request_id: u64,
    },
    /// The font files `files` (numbers of `FontFamily`'s faces, at most
    /// `font::FONT_FILES_PER_CALL`), each the first time a face in it is
    /// registered (Chromium's `MatchFamilyName` answers with the file).
    /// Answered with `FontFiles`, as `GetFontFamily`: a number no answer
    /// has given (to any renderer: every file is one any family's answer
    /// could give it), or more files, is a bad message.
    GetFontFiles {
        files: Vec<u32>,
        request_id: u64,
    },
    /// A change to the area `area` (`LocalStorageOpened`'s), which this
    /// renderer made to its own copy first (`StorageArea::Put`, `Delete`,
    /// `DeleteAll`): sent after the answer that gave it the area, with the
    /// renderer's number for the `Storage` that made it and its document's
    /// URL (`StorageAreaSource`), for the `storage` events. Handled on the
    /// IO thread; one of an area that isn't the renderer's changes nothing.
    ChangeStorage {
        area: u64,
        change: StorageChange,
        source: u64,
        url: String,
    },
    /// The renderer has none of its areas any more (its DOM replaced by
    /// another engine's): the next `OpenLocalStorage` of a frame gives the
    /// items again. Handled on the IO thread, before any `OpenLocalStorage`
    /// after it.
    ForgetStorageAreas,
    /// `BlobURLStore::Revoke`: `url`, which this renderer registered, no
    /// longer stands for its blob. Sent after the loads asked for before
    /// it. Handled on the IO thread.
    RevokeBlobUrl {
        url: String,
    },
    /// `DidFinishLoad`: the document and every subresource it asked for
    /// have loaded or failed. Sent once the frame showing that is
    /// submitted, with the frame's token (`None` if none could be drawn,
    /// e.g. for an empty viewport), so a caller can wait for the pixels.
    DidFinishLoad {
        navigation_id: u64,
        frame_token: Option<u64>,
        /// The subresources that failed (URLs, or the `href` / `src` as
        /// written when it didn't resolve). For logs and tests.
        failed_subresources: Vec<String>,
        /// With `RendererFlags::web_tests`: the root element still has the
        /// `reftest-wait` class, so the test isn't ready yet;
        /// `ReftestWaitEnded` follows once script removes it.
        reftest_wait: bool,
    },
    /// With `RendererFlags::web_tests`: the `reftest-wait` class is gone
    /// from the root element, and this is the frame (`None` if none could
    /// be drawn) that shows the document since. Once per navigation.
    ReftestWaitEnded {
        navigation_id: u64,
        frame_token: Option<u64>,
    },
    /// With `RendererFlags::web_tests`: what a test page reported
    /// (`testRunner.reportHarnessResults`), the text the end-to-end test
    /// that loaded it checks.
    TestharnessResults {
        navigation_id: u64,
        results: String,
    },
    /// The answer to `RendererMsg::QueryElement`: `None` if no element
    /// matched, there is no document, or `selector` isn't a valid selector
    /// list.
    QueryElementResult {
        query_id: u64,
        element: Option<QueriedElement>,
    },
    /// The answer to `RendererMsg::RequestIdleFrame`: the frame that went
    /// to Viz, if one did (its token, as an input's ack names its frame).
    IdleFrameReady {
        frame_token: Option<u64>,
    },
    /// The answer to `RendererMsg::RequestVisualState`: the frame showing
    /// it has gone to Viz before this.
    VisualStateReady {
        request_id: u64,
    },
    /// The answer to `RendererMsg::WebDriver`.
    WebDriverReply {
        request_id: u64,
        result: Result<WebDriverValue, WebDriverError>,
    },
    /// An answer or event of the browser's DevTools `session`, as JSON:
    /// the browser passes it on to the session's client as it is (it
    /// doesn't read a renderer's answers), once it checked the renderer has
    /// that session.
    DevToolsMessage {
        session: String,
        message: String,
    },
    /// What the browser's DevTools `session` has on in the renderer now, as
    /// a command changed it: the browser keeps it, for the next renderer
    /// of the tab (Blink's session state updates, sent with its answers).
    DevToolsSessionState {
        session: String,
        state: DevToolsSessionState,
    },
    /// The renderer has gone, exited or crashed. The renderer never sends
    /// it: the browser's IO thread reports it once the renderer's channel
    /// has closed (`RenderProcessHostImpl::OnChannelError`), so it is the
    /// last message from that renderer.
    RendererGone,
}
