//! Browser <-> Renderer messages.

use std::sync::Arc;

use bytes::Bytes;
use gfx::{Color, RectF, Size};
use serde::{Deserialize, Serialize};

use crate::input::InputEvent;
use crate::network::{LoadResult, LoadTiming, RedirectInfo, ResourceRequest};

/// What a navigation's load took (Chromium's `URLResponseHead` and
/// `URLLoaderCompletionStatus`, the part DevTools' `Network.dataReceived`
/// and `loadingFinished` tell): the bytes its head and body took on the
/// wire, and when it ended; and its status, which the page's log tells of
/// when it is 400 or more. With what Navigation Timing reads of it
/// (Chromium's `NavigationTiming` in `CommitNavigationParams`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NavigationLoad {
    pub status: u16,
    pub status_text: String,
    pub encoded_headers_length: u64,
    pub encoded_body_length: u64,
    pub response_end: ipc::TimeTicks,
    /// When the parts of the last request happened (after the redirects).
    pub timing: Option<LoadTiming>,
    /// The redirects it followed, in order.
    pub redirects: Vec<RedirectHop>,
    /// How it came (`ResourceResponse::protocol`): `http/1.1`, `h2`...
    pub protocol: String,
    /// The response's `Content-Encoding`, as sent, if any.
    pub content_encoding: Option<String>,
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

/// How a navigation came about, as Navigation Timing tells it
/// (`PerformanceNavigationTiming.type`; Blink's `WebNavigationType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NavigationTimingType {
    Navigate,
    Reload,
    BackForward,
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
    pub state: Option<Vec<u8>>,
    /// Its scroll restoration mode is `"manual"`.
    pub manual_scroll_restoration: bool,
    /// The entry's index, and how many entries there are.
    pub index: u32,
    pub length: u32,
}

/// Browser -> Renderer (`content.mojom.Frame`, `blink.mojom.Widget`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RendererMsg {
    /// `CommitNavigation`: the browser has fetched the response and the
    /// renderer should show it. Chromium streams the body through a data
    /// pipe; here it arrives whole.
    CommitNavigation {
        /// Increases with every navigation the browser starts. Responses
        /// arrive in any order, so the renderer drops a commit older than
        /// the one it shows.
        navigation_id: u64,
        /// The final URL, after redirects.
        url: String,
        mime_type: String,
        /// The `charset` of the response's `Content-Type`, if any.
        charset: Option<String>,
        /// The response's `Content-Language`, as sent, if any (Chromium's
        /// `ResourceResponse::HttpContentLanguage`): the document's content
        /// language comes from it.
        content_language: Option<String>,
        body: Bytes,
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
        navigation_type: NavigationTimingType,
    },
    /// `CommitSameDocumentNavigation` of a history traversal: back or
    /// forward to an entry of the document shown, `document` (the
    /// `navigation_id` that committed it). The renderer changes the URL,
    /// the state and its place in the session history, and fires
    /// `popstate`, and `hashchange` if the fragment changed; answered with
    /// `DidCommitSameDocumentNavigation` carrying `navigation_id`, or
    /// `SameDocumentNavigationFailed` if it shows another document by then
    /// (Chromium's `CommitResult::RestartCrossDocument`).
    CommitSameDocumentNavigation {
        navigation_id: u64,
        document: u64,
        url: String,
        history: HistoryEntryParams,
    },
    /// `SetHistoryOffsetAndLength`: where the document's entry is in the
    /// session history now, and how many entries there are, once its
    /// commit is in.
    SetHistoryOffsetAndLength { index: u32, length: u32 },
    /// A `FetchResource`'s load reached a redirect and waits there
    /// (`URLLoaderClient::OnReceiveRedirect`) until the renderer answers
    /// `FollowRedirect`.
    ReceivedRedirect {
        request_id: u64,
        /// Boxed: a response's head, much bigger than the other messages.
        redirect: Box<RedirectInfo>,
    },
    /// The answer to a `FetchResource` (`URLLoaderClient::OnComplete`).
    ResourceResponse { request_id: u64, result: LoadResult },
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
    /// person uses has DevTools too.
    DevToolsAttach { session: String },
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
    DevToolsMainFrameToken(String),
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

/// `WindowOpenDisposition`: where a followed link opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowOpenDisposition {
    CurrentTab,
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

/// Renderer -> Browser (`content.mojom.FrameHost`, `blink.mojom.WidgetHost`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RendererHostMsg {
    /// Renderer main thread finished initialising (V8 is up).
    RendererReady,
    DidCommitNavigation {
        navigation_id: u64,
        url: String,
    },
    /// `DidCommitSameDocumentNavigation`: the document shown navigated
    /// within itself, without loading anything (to a fragment: a link,
    /// `location.hash`; `history.pushState`); its URL is now `url`.
    /// `replace`: the session history's current entry takes it, rather
    /// than a new one (`location.replace("#x")`, the same URL again,
    /// `replaceState`). `state`: the entry's (`None` is `null`).
    /// `navigation_id`: the browser's history traversal it was
    /// (`CommitSameDocumentNavigation`), if it was one.
    DidCommitSameDocumentNavigation {
        url: String,
        replace: bool,
        state: Option<Vec<u8>>,
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
    /// `FrameHost::UpdateState`: script changed the current entry's scroll
    /// restoration mode (`history.scrollRestoration`).
    UpdateState {
        manual_scroll_restoration: bool,
    },
    /// `FrameHost::OpenURL`: a link was followed, or script navigated
    /// (`location`), to another document, or, opening a new tab, to any.
    /// The browser decides whether to navigate there. `replace`: the new
    /// document takes the current entry's place in the session history
    /// (`location.replace`; Chromium's `should_replace_current_entry`).
    OpenUrl {
        url: String,
        disposition: WindowOpenDisposition,
        replace: bool,
    },
    /// `location.reload()`: the browser loads the current entry again, as
    /// its own reload does.
    Reload,
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
        request: ResourceRequest,
    },
    /// Closing the `URLLoader` of a `FetchResource` still loading (an
    /// aborted `fetch()`, a discarded document's): no answer comes. Handled
    /// on the IO thread too.
    CancelResource {
        request_id: u64,
    },
    /// `URLLoader::FollowRedirect`: the load of a `FetchResource` waiting
    /// at the redirect it told of (`RendererMsg::ReceivedRedirect`) goes
    /// on. Ignored for a load that isn't there (cancelled, or answered).
    /// Handled on the IO thread too.
    FollowRedirect {
        request_id: u64,
    },
    /// `BlobURLStore::Register` (`URL.createObjectURL`): `url` stands for
    /// a blob of `bytes` and `mime_type` (its type, maybe empty), for this
    /// renderer's subresources and for any navigation, until revoked or
    /// the renderer goes. Sent before any load of it, on the same channel.
    /// Handled on the IO thread.
    RegisterBlobUrl {
        url: String,
        mime_type: String,
        bytes: Arc<[u8]>,
    },
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
    /// The renderer has gone, exited or crashed. The renderer never sends
    /// it: the browser's IO thread reports it once the renderer's channel
    /// has closed (`RenderProcessHostImpl::OnChannelError`), so it is the
    /// last message from that renderer.
    RendererGone,
}
