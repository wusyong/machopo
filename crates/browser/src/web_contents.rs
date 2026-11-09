use std::sync::{Arc, Mutex};

use base::SequenceNumber;
use crossbeam_channel::Sender;
use gfx::{RectF, Size};
use ipc::TimeTicks;
use net::{LoadHandle, NetworkContext};
use protocol::TabId;
use protocol::network::{LoadResult, RedirectInfo, RequestDestination, ResourceRequest};
use protocol::renderer::{
    NavigationLoad, NavigationTimingType, RedirectHop, RendererHostMsg, RendererMsg,
    VisualProperties, WindowOpenDisposition,
};
use protocol::viz::{
    BeginFrameArgs, BeginFrameObserver, DisplayMsg, FrameSinkId, LocalSurfaceId,
    SharedImageInterface, SurfaceId, VizMsg,
};

use crate::debug_urls::{DebugUrl, debug_url};
use crate::navigation_controller::{NavigationController, NavigationType};
use crate::render_process_host::{CommittedDocument, Committer};
use crate::{RenderProcessHost, RendererFlags, ScriptEngineFactory};

/// What a tab's other threads have for its UI thread: the embedder posts
/// each to the UI thread and hands it to the tab there (the renderer's
/// messages to [`WebContents::on_renderer_message`], a navigation's
/// redirect to [`WebContents::on_navigation_redirected`] and its end to
/// [`WebContents::on_navigation_completed`]), as Chromium's
/// IPC and its `NavigationURLLoader` post to the UI thread. A navigation's
/// events aren't renderer messages: a renderer mustn't be able to send
/// them.
#[derive(Debug)]
pub enum ContentsEvent {
    /// From the tab's renderer, through its IO thread.
    Renderer(RendererHostMsg),
    /// Navigation `navigation_id` reached a redirect and waits there
    /// (`NavigationURLLoaderDelegate::OnRequestRedirected`), from the
    /// network service's thread.
    NavigationRedirected {
        navigation_id: u64,
        redirect: RedirectInfo,
    },
    /// Navigation `navigation_id` loaded, or failed
    /// (`NavigationURLLoaderDelegate::OnResponseStarted`,
    /// `OnRequestFailed`): it commits on the UI thread.
    NavigationCompleted {
        navigation_id: u64,
        result: LoadResult,
    },
}

/// Called on the thread an event comes from (the renderer's IO thread, the
/// network service's), to post it to the UI thread.
pub type ContentsEventCallback = Box<dyn Fn(TabId, ContentsEvent) + Send>;

/// `content::WebContents`: one tab.
pub struct WebContents {
    tab: TabId,
    /// What DevTools calls the tab and its main frame (its target id):
    /// random, so a page can't guess it (Chromium's frame tokens are
    /// `base::UnguessableToken`s); each of its renderers is told it.
    devtools_frame_token: String,
    frame_sink_id: FrameSinkId,
    process: RenderProcessHost,
    network: NetworkContext,
    /// For the debug URLs that act on Viz.
    viz: Sender<VizMsg>,
    visual_properties: VisualProperties,
    /// The browser window's rect on the screen, CSS px, as the renderer
    /// was last told (`RenderWidgetHostImpl::SendScreenRects`); none yet.
    window_screen_rect: Option<RectF>,
    /// Numbers the navigations started from 1: each gets a larger id.
    navigation_ids: SequenceNumber<u64>,
    /// The last of them that loads a document (not a traversal within
    /// the document shown).
    document_navigation_id: u64,
    /// That navigation, until it commits or the next one starts
    /// (`NavigationRequest`).
    navigation: Option<PendingNavigation>,
    controller: NavigationController,
    /// The URL to show: the latest navigation's while it loads, then the
    /// committed one (Chromium's visible URL).
    url: Option<String>,
    title: String,
    /// Links the renderer followed to new tabs, for the embedder to open
    /// (`WebContentsDelegate::OpenURLFromTab`).
    opened: Vec<OpenUrlParams>,
    /// Shown (`WebContentsImpl::GetVisibility`): the renderer was last told
    /// so.
    visible: bool,
    /// The page has the system's focus (`RenderWidgetHostImpl::is_focused_`):
    /// the renderer was told so.
    focused: bool,
    /// The renderer has gone (`WebContents::IsCrashed`): the next
    /// navigation launches another.
    crashed: bool,
    /// How to launch a renderer, again after one has gone.
    launch: Launch,
    /// Input events handed to the renderer that it (or its compositor) has
    /// yet to acknowledge (`RenderWidgetHostImpl::in_flight_event_count_`).
    input_in_flight: u32,
}

/// A navigation loading (`NavigationRequest`): its load (none for a
/// `blob:` URL's, answered at once), and how its document commits.
struct PendingNavigation {
    id: u64,
    load: Option<LoadHandle>,
    committer: Committer,
    url: String,
    /// The redirects it followed, for Navigation Timing.
    redirects: Vec<RedirectHop>,
}

/// What launches this tab's renderers.
struct Launch {
    flags: RendererFlags,
    script_engine: ScriptEngineFactory,
    gpu: Option<Arc<dyn SharedImageInterface>>,
    /// Every renderer's messages, and every navigation's events, go here.
    on_event: Arc<Mutex<ContentsEventCallback>>,
}

impl Launch {
    fn process(
        &self,
        tab: TabId,
        frame_sink_id: FrameSinkId,
        viz: &Sender<VizMsg>,
        network: &NetworkContext,
    ) -> std::io::Result<RenderProcessHost> {
        let on_event = self.on_event.clone();
        RenderProcessHost::launch(
            tab,
            frame_sink_id,
            self.flags.clone(),
            self.script_engine,
            viz.clone(),
            self.gpu.clone(),
            network.clone(),
            Box::new(move |tab, msg| {
                let on_event = on_event.lock().unwrap_or_else(|e| e.into_inner());
                on_event(tab, ContentsEvent::Renderer(msg));
            }),
        )
    }
}

/// A tab's id for DevTools: 128 bits of the operating system's random
/// numbers, as 32 hex digits, as long as Chrome's.
fn devtools_frame_token() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// `content::OpenURLParams`: a link to open in another tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenUrlParams {
    pub url: String,
    pub disposition: WindowOpenDisposition,
}

/// The longest title a tab takes, in characters (Chromium's
/// `kMaxTitleChars`, in UTF-16 code units there): a page can make its
/// title as long as it likes.
pub const MAX_TITLE_CHARS: usize = 4096;

/// `title`, cut to `MAX_TITLE_CHARS` characters.
fn truncated_title(title: &str) -> String {
    match title.char_indices().nth(MAX_TITLE_CHARS) {
        Some((end, _)) => title[..end].to_owned(),
        None => title.to_owned(),
    }
}

impl WebContents {
    /// `gpu` is where the renderer puts its tiles on the GPU Viz draws
    /// with; with `None` they go to Viz as pixels, which it uploads.
    pub fn new(
        tab: TabId,
        flags: RendererFlags,
        script_engine: ScriptEngineFactory,
        viz: Sender<VizMsg>,
        gpu: Option<Arc<dyn SharedImageInterface>>,
        network: NetworkContext,
        on_event: ContentsEventCallback,
    ) -> std::io::Result<Self> {
        // One renderer "process" per tab, one frame sink per renderer
        // (client 0 is the browser's own).
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "tab ids count the tabs opened: far fewer than u32::MAX"
        )]
        let client_id = tab.0 + 1;
        let frame_sink_id = FrameSinkId {
            client_id,
            sink_id: 1,
        };
        let launch = Launch {
            flags,
            script_engine,
            gpu,
            on_event: Arc::new(Mutex::new(on_event)),
        };
        let process = launch.process(tab, frame_sink_id, &viz, &network)?;
        let devtools_frame_token = devtools_frame_token();
        process.send(RendererMsg::DevToolsMainFrameToken(
            devtools_frame_token.clone(),
        ));
        Ok(Self {
            tab,
            devtools_frame_token,
            frame_sink_id,
            process,
            network,
            viz,
            navigation_ids: SequenceNumber::starting_at(1),
            document_navigation_id: 0,
            navigation: None,
            controller: NavigationController::default(),
            visual_properties: VisualProperties {
                viewport_size: Size::new(0, 0),
                device_scale_factor: 1.0,
                local_surface_id: LocalSurfaceId::default(),
            },
            window_screen_rect: None,
            url: None,
            title: String::new(),
            opened: Vec::new(),
            visible: true,
            focused: false,
            crashed: false,
            launch,
            input_in_flight: 0,
        })
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// What DevTools calls the tab and its main frame: its target id.
    pub fn devtools_frame_token(&self) -> &str {
        &self.devtools_frame_token
    }

    /// The frame sink this tab's renderer submits to.
    pub fn frame_sink_id(&self) -> FrameSinkId {
        self.frame_sink_id
    }

    /// Where Viz sends this tab's renderer its `BeginFrame`s: register it
    /// with the `BeginFrameSource` under [`frame_sink_id`](Self::frame_sink_id).
    pub fn begin_frame_sender(&self) -> impl Fn(BeginFrameArgs) + Send + Sync + 'static {
        self.process.begin_frame_sender()
    }

    /// The same, for registering with Viz while it runs
    /// (`VizMsg::RegisterFrameSinkId`).
    pub fn begin_frame_observer(&self) -> BeginFrameObserver {
        BeginFrameObserver::new(self.begin_frame_sender())
    }

    /// The surface the tab's frames go to, for the embedder to show
    /// (`RenderWidgetHostViewAura`'s `SurfaceId`, embedded by the browser's
    /// compositor with a `SurfaceLayer`).
    pub fn surface_id(&self) -> SurfaceId {
        SurfaceId {
            frame_sink_id: self.frame_sink_id,
            local_surface_id: self.visual_properties.local_surface_id,
        }
    }

    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    /// The session history.
    pub fn controller(&self) -> &NavigationController {
        &self.controller
    }

    /// The id of the latest navigation started that loads a document (0
    /// before the first); its `DidCommitNavigation` and `DidFinishLoad`
    /// carry it. A traversal within the document shown has neither.
    pub fn latest_navigation_id(&self) -> u64 {
        self.document_navigation_id
    }

    /// Whether navigation `navigation_id` is on its way: loading, not yet
    /// committed (a page of the browser's own commits as it starts).
    pub fn is_navigating(&self, navigation_id: u64) -> bool {
        self.navigation
            .as_ref()
            .is_some_and(|navigation| navigation.id == navigation_id)
    }

    /// `NavigationController::LoadURL`: starts a `NavigationRequest` for a
    /// new history entry. Returns at once; the response is committed in
    /// the renderer when it arrives, from the network thread. A failed
    /// load commits an error page (see [`loader`](crate::loader)).
    /// A debug URL (`about:gpucrash`) is handled instead, and nothing is
    /// navigated.
    pub fn navigate(&mut self, url: impl Into<String>) {
        let url = url.into();
        match debug_url(&url) {
            Some(DebugUrl::GpuCrash) => {
                tracing::warn!(url, "losing the GPU device");
                let _ = self.viz.send(VizMsg::Display(DisplayMsg::LoseContext));
            }
            Some(DebugUrl::Crash) => {
                tracing::warn!(url, "ending the renderer");
                self.process.kill();
                self.crashed = true;
            }
            None => self.begin_navigation(url, NavigationType::NewEntry),
        }
    }

    /// The tab's first document, `url`, as an initial entry: the first
    /// new entry takes its place (Chromium's `IsInitialEntry`), so there is
    /// no going back to it. What starts a tab that something drives (`machopo
    /// --automation-pipe`), as Chrome's new tab has its `about:blank`.
    pub fn navigate_initial(&mut self, url: impl Into<String>) {
        self.begin_navigation(url.into(), NavigationType::Initial);
    }

    /// `NavigationController::GoBack`. False if there is nothing back.
    pub fn go_back(&mut self) -> bool {
        self.go_to_offset(-1)
    }

    /// `NavigationController::GoForward`. False if there is nothing
    /// forward.
    pub fn go_forward(&mut self) -> bool {
        self.go_to_offset(1)
    }

    /// `NavigationController::Reload`: loads the entry shown again.
    pub fn reload(&mut self) {
        if let Some(current) = self.controller.current_index() {
            let url = self.controller.entries()[current].url.clone();
            self.begin_navigation(url, NavigationType::Reload);
        }
    }

    /// Goes to the entry `offset` steps away (`GoToOffset`): within the
    /// document shown if it is one of its entries (the renderer fires
    /// `popstate`), else loading it again, its state with it; there is no
    /// back-forward cache, and the scroll position isn't restored. False
    /// if there is no such entry.
    pub fn go_to_offset(&mut self, offset: isize) -> bool {
        let Some(index) = self.controller.index_for_offset(offset) else {
            return false;
        };
        let kind = NavigationType::HistoryIndex(index);
        let url = self.controller.entries()[index].url.clone();
        match self.controller.current_document() {
            Some(document) if !self.crashed && self.controller.is_same_document(index) => {
                let navigation_id = self.navigation_ids.get_next();
                let history = self.controller.entry_params(kind);
                self.controller.did_start(navigation_id, kind);
                tracing::info!(%url, navigation_id, "navigate within the document");
                self.process
                    .send(RendererMsg::CommitSameDocumentNavigation {
                        navigation_id,
                        document,
                        url,
                        history,
                    });
            }
            _ => self.begin_navigation(url, kind),
        }
        true
    }

    /// `RenderFrameHostImpl::OpenURL`: a link the renderer followed, here
    /// or to a new tab, which the embedder opens (`take_opened`). It may
    /// not open a local file unless it shows one, as it may not load one:
    /// in a new tab neither, as this tab's renderer asks.
    /// Only schemes the browser loads are navigated to; Chromium hands
    /// others (`mailto:`) to the system, which isn't done yet. A `data:`
    /// URL is loaded, but not for a page, here or in a new tab
    /// (Chromium's `DataUrlNavigationThrottle`: a page that makes its own
    /// URL is a phishing tool); child frames and the user's navigations
    /// may have one. A debug URL is navigated to like any other, not
    /// handled: only the user's own navigations handle them, so that no
    /// page can crash the GPU.
    fn open_url(&mut self, url: &str, disposition: WindowOpenDisposition, replace: bool) {
        let scheme = url.split(':').next().unwrap_or("").to_ascii_lowercase();
        match scheme.as_str() {
            "http" | "https" | "file" | "about" | "blob" => {}
            "data" => {
                tracing::warn!(%url, "a page may not navigate to a data: URL");
                return;
            }
            _ => {
                tracing::warn!(%url, "no handler for the link's scheme");
                return;
            }
        }
        if !self.process.can_request(url) {
            tracing::warn!(%url, "the renderer may not open local files");
            return;
        }
        if disposition != WindowOpenDisposition::CurrentTab {
            self.opened.push(OpenUrlParams {
                url: url.to_owned(),
                disposition,
            });
            return;
        }
        let kind = if replace {
            NavigationType::Replace
        } else {
            NavigationType::NewEntry
        };
        self.begin_navigation(url.to_owned(), kind);
    }

    /// The links followed to new tabs since last asked, in order.
    pub fn take_opened(&mut self) -> Vec<OpenUrlParams> {
        std::mem::take(&mut self.opened)
    }

    /// Whether the renderer has gone: its sad tab shows until the next
    /// navigation.
    pub fn is_crashed(&self) -> bool {
        self.crashed
    }

    /// `RenderFrameHostManager`'s new process for a navigation after a
    /// crash: the same tab and frame sink, the size and visibility it had.
    /// Viz forgets the sink and its surface first, so the old renderer's
    /// last frame doesn't show until the new one draws (Chromium's new
    /// process has a new sink), then sends `BeginFrame`s to the new one; a
    /// newer `LocalSurfaceId` keeps a late frame of the old one out.
    fn relaunch(&mut self) {
        let process =
            match self
                .launch
                .process(self.tab, self.frame_sink_id, &self.viz, &self.network)
            {
                Ok(process) => process,
                Err(e) => {
                    tracing::error!("failed to launch a renderer again: {e}");
                    return;
                }
            };
        std::mem::replace(&mut self.process, process).abandon();
        self.crashed = false;
        self.process.send(RendererMsg::DevToolsMainFrameToken(
            self.devtools_frame_token.clone(),
        ));
        let props = &mut self.visual_properties;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per resize or relaunch of the tab: far fewer than u32::MAX"
        )]
        {
            props.local_surface_id.parent_sequence_number += 1;
        }
        self.process
            .send(RendererMsg::UpdateVisualProperties(*props));
        if let Some(window_screen_rect) = self.window_screen_rect {
            self.process
                .send(RendererMsg::UpdateScreenRects { window_screen_rect });
        }
        if !self.visible {
            self.process.send(RendererMsg::WasHidden);
        }
        if self.focused {
            self.process.send(RendererMsg::SetFocus(true));
        }
        let _ = self
            .viz
            .send(VizMsg::InvalidateFrameSinkId(self.frame_sink_id));
        let _ = self.viz.send(VizMsg::RegisterFrameSinkId(
            self.frame_sink_id,
            self.begin_frame_observer(),
        ));
    }

    fn begin_navigation(&mut self, url: String, kind: NavigationType) {
        // The new document's time origin (HR-Time), whatever starts it.
        let navigation_start = ipc::TimeTicks::now();
        if self.crashed {
            self.relaunch();
        }
        let navigation_id = self.navigation_ids.get_next();
        self.document_navigation_id = navigation_id;
        // The navigation it replaces stops loading (Chromium resets its
        // `NavigationRequest`): left at a redirect, it would wait for good.
        if let Some(load) = self
            .navigation
            .take()
            .and_then(|navigation| navigation.load)
        {
            load.cancel();
        }
        let history = self.controller.entry_params(kind);
        self.controller.did_start(navigation_id, kind);
        tracing::info!(%url, navigation_id, ?kind, "navigate");
        self.url = Some(url.clone());
        let navigation_type = match kind {
            NavigationType::Reload => NavigationTimingType::Reload,
            NavigationType::HistoryIndex(_) => NavigationTimingType::BackForward,
            NavigationType::NewEntry | NavigationType::Replace | NavigationType::Initial => {
                NavigationTimingType::Navigate
            }
        };
        let committer = self
            .process
            .committer(history, navigation_start, navigation_type);
        if let Some(markup) = crate::loader::about_page(&url) {
            committer.commit(navigation_id, CommittedDocument::page(url, markup.into()));
            return;
        }
        let request = ResourceRequest::new(url.clone(), RequestDestination::Document);
        self.navigation = Some(PendingNavigation {
            id: navigation_id,
            load: None,
            committer,
            url,
            redirects: Vec::new(),
        });
        // Its redirects and its end, to the UI thread (`NavigationURLLoader`
        // tells its `NavigationRequest`).
        let post = {
            let on_event = self.launch.on_event.clone();
            let tab = self.tab;
            move |event| {
                let on_event = on_event.lock().unwrap_or_else(|e| e.into_inner());
                on_event(tab, event);
            }
        };
        let completed = {
            let post = post.clone();
            move |result| {
                post(ContentsEvent::NavigationCompleted {
                    navigation_id,
                    result,
                });
            }
        };
        // A blob URL's blob, as it is now (`BlobURLLoaderFactory`).
        if crate::render_process_host::is_blob_url(&request.url) {
            completed(self.network.load_blob_url(&request, None));
            return;
        }
        let load = self.network.fetch_with_redirects(
            request,
            move |redirect| {
                post(ContentsEvent::NavigationRedirected {
                    navigation_id,
                    redirect,
                });
            },
            completed,
        );
        if let Some(navigation) = &mut self.navigation {
            navigation.load = Some(load);
        }
    }

    /// Navigation `navigation_id` reached a redirect
    /// (`NavigationRequest::OnRequestRedirected`, on the UI thread): it
    /// goes on, unless another navigation has replaced it (its load was
    /// cancelled then).
    pub fn on_navigation_redirected(&mut self, navigation_id: u64, redirect: &RedirectInfo) {
        if let Some(navigation) = &mut self.navigation
            && navigation.id == navigation_id
            && let Some(load) = &navigation.load
        {
            navigation.redirects.push(RedirectHop::of(redirect));
            load.follow_redirect();
        }
    }

    /// Navigation `navigation_id` loaded, or failed
    /// (`NavigationRequest::OnResponseStarted`, `OnRequestFailed`, on the UI
    /// thread): its document, or the error page, commits, unless another
    /// navigation has replaced it.
    pub fn on_navigation_completed(&mut self, navigation_id: u64, result: LoadResult) {
        let Some(navigation) = self
            .navigation
            .take_if(|navigation| navigation.id == navigation_id)
        else {
            return;
        };
        let committer = navigation.committer;
        match result {
            Ok(response) => {
                let content_language = response
                    .headers
                    .iter()
                    .find(|(name, _)| name == "content-language")
                    .map(|(_, value)| value.clone());
                let content_encoding = response
                    .headers
                    .iter()
                    .find(|(name, _)| name == "content-encoding")
                    .map(|(_, value)| value.clone());
                let loaded = NavigationLoad {
                    status: response.status,
                    status_text: response.status_text.clone(),
                    encoded_headers_length: response.encoded_headers_length,
                    encoded_body_length: response.encoded_body_length,
                    response_end: response
                        .timing
                        .map_or_else(TimeTicks::now, |timing| timing.response_end),
                    timing: response.timing,
                    redirects: navigation.redirects,
                    protocol: response.protocol.clone(),
                    content_encoding,
                };
                committer.commit(
                    navigation_id,
                    CommittedDocument {
                        url: response.url,
                        mime_type: response.mime_type,
                        charset: response.charset,
                        content_language,
                        body: response.body,
                        loaded: Some(loaded),
                    },
                );
            }
            Err(error) => {
                let url = navigation.url;
                tracing::warn!(%url, code = error.code.name(), detail = %error.detail, "navigation failed");
                let page = crate::loader::error_page(&error);
                committer.commit(navigation_id, CommittedDocument::page(url, page));
            }
        }
    }

    /// `RenderWidgetHostImpl::SynchronizeVisualProperties`. Allocates a new
    /// `LocalSurfaceId` whenever size or scale actually changes.
    pub fn resize(&mut self, viewport_size: Size, device_scale_factor: f32) {
        let props = &mut self.visual_properties;
        if props.viewport_size == viewport_size && props.device_scale_factor == device_scale_factor
        {
            return;
        }
        props.viewport_size = viewport_size;
        props.device_scale_factor = device_scale_factor;
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per resize or relaunch of the tab: far fewer than u32::MAX"
        )]
        {
            props.local_surface_id.parent_sequence_number += 1;
        }
        self.process
            .send(RendererMsg::UpdateVisualProperties(*props));
    }

    /// `RenderWidgetHostImpl::SendScreenRects`: tells the renderer where
    /// the browser window is on the screen and how big (CSS px), if that
    /// changed.
    pub fn set_window_screen_rect(&mut self, window_screen_rect: RectF) {
        if self.window_screen_rect == Some(window_screen_rect) {
            return;
        }
        self.window_screen_rect = Some(window_screen_rect);
        self.process
            .send(RendererMsg::UpdateScreenRects { window_screen_rect });
    }

    /// `WebContentsImpl::WasHidden`: another tab is shown instead.
    pub fn was_hidden(&mut self) {
        self.visible = false;
        self.process.send(RendererMsg::WasHidden);
    }

    /// `WebContentsImpl::WasShown`.
    pub fn was_shown(&mut self) {
        self.visible = true;
        self.process.send(RendererMsg::WasShown);
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// `RenderWidgetHostImpl::SetFocus`: the page takes or loses the
    /// system's focus; the renderer hears of changes only.
    pub fn set_focus(&mut self, focused: bool) {
        if self.focused != focused {
            self.focused = focused;
            self.process.send(RendererMsg::SetFocus(focused));
        }
    }

    pub fn has_focus(&self) -> bool {
        self.focused
    }

    /// The dropdown the renderer asked for closed (Chromium's
    /// `PopupMenuHelper` answering the `PopupMenuClient`).
    pub fn popup_menu_closed(&mut self, result: protocol::renderer::PopupMenuResult) {
        self.process.send(RendererMsg::PopupMenuClosed(result));
    }

    /// `WebContentsImpl::FocusThroughTabTraversal`: Tab from the toolbar
    /// moved into the page: its first (last, `reverse`) focusable element
    /// takes the focus.
    pub fn set_initial_focus(&mut self, reverse: bool) {
        self.process.send(RendererMsg::SetInitialFocus { reverse });
    }

    /// The size the renderer was last told, physical px.
    pub fn viewport_size(&self) -> Size {
        self.visual_properties.viewport_size
    }

    /// `RenderWidgetHostImpl::ForwardWheelEvent` / `ForwardKeyboardEvent`:
    /// hands an input event to the renderer, whose compositor thread gets
    /// it first. In flight until acknowledged (`input_in_flight`).
    pub fn forward_input_event(&mut self, event: protocol::input::InputEvent) {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per event in flight, each acknowledged before long: far fewer than u32::MAX"
        )]
        {
            self.input_in_flight += 1;
        }
        self.process.send_input_event(event);
    }

    /// How many input events the renderer has yet to acknowledge. One that
    /// changed the page is acknowledged with the frame that shows the
    /// change, so at 0 what the renderer says about its last frame (where
    /// an element is) takes the input in.
    pub fn input_in_flight(&self) -> u32 {
        self.input_in_flight
    }

    /// Asks the renderer for the first element of the document shown that
    /// matches `selector`, which it answers with
    /// `RendererHostMsg::QueryElementResult` and the same `query_id`; only
    /// with `RendererFlags::automation`. With `frames`, of the child frame
    /// of the first iframe matching each in turn.
    pub fn query_element(&self, query_id: u64, frames: &[String], selector: &str) {
        self.process.send(RendererMsg::QueryElement {
            query_id,
            frames: frames.to_vec(),
            selector: selector.to_owned(),
        });
    }

    /// Hands the renderer WebDriver's `command`, in the frame `frame` (an
    /// id the renderer gave; the top-level one if none), which it answers
    /// with `RendererHostMsg::WebDriverReply` and the same `request_id`;
    /// only with `RendererFlags::automation`.
    pub fn webdriver(
        &self,
        request_id: u64,
        frame: Option<String>,
        command: protocol::webdriver::WebDriverCommand,
    ) {
        self.process.send(RendererMsg::WebDriver {
            request_id,
            frame,
            command,
        });
    }

    /// Attaches a DevTools session of the browser's to the page
    /// (`RendererMsg::DevToolsAttach`): the commands it can't answer go
    /// there.
    pub fn devtools_attach(&self, session: &str) {
        self.process.send(RendererMsg::DevToolsAttach {
            session: session.to_owned(),
        });
    }

    /// The session has gone.
    pub fn devtools_detach(&self, session: &str) {
        self.process.send(RendererMsg::DevToolsDetach {
            session: session.to_owned(),
        });
    }

    /// A DevTools command (JSON) for the page's `session`, which the
    /// renderer answers with `RendererHostMsg::DevToolsMessage`; with
    /// `interrupt`, while its script runs.
    pub fn devtools_message(&self, session: &str, message: String, interrupt: bool) {
        self.process.send(RendererMsg::DevToolsMessage {
            session: session.to_owned(),
            message,
            interrupt,
        });
    }

    /// Asks the renderer to scroll the first element of the document shown
    /// that matches `selector` into view if its center is out of it, at its
    /// next main frame (`DOM.scrollIntoViewIfNeeded`); a
    /// `request_visual_state` after it is answered once that is drawn. Only
    /// with `RendererFlags::automation`. With `frames`, of the child frame
    /// of the first iframe matching each in turn.
    pub fn scroll_into_view_if_needed(&self, frames: &[String], selector: &str) {
        self.process.send(RendererMsg::ScrollIntoViewIfNeeded {
            frames: frames.to_vec(),
            selector: selector.to_owned(),
        });
    }

    /// Asks the renderer to answer, with `RendererHostMsg::VisualStateReady`
    /// and the same `request_id`, once a frame showing what the document
    /// has changed so far has gone to Viz (`InsertVisualStateCallback`);
    /// only with `RendererFlags::automation`.
    pub fn request_visual_state(&self, request_id: u64) {
        self.process
            .send(RendererMsg::RequestVisualState { request_id });
    }

    /// Applies a renderer message to browser-side state. The embedder calls
    /// this on the UI thread after the IO thread forwarded it.
    pub fn on_renderer_message(&mut self, msg: &RendererHostMsg) {
        match msg {
            RendererHostMsg::UpdateTitle(title) => {
                let title = truncated_title(title);
                self.title = title.clone();
                self.controller.set_title(title);
            }
            RendererHostMsg::DidCommitNavigation { navigation_id, url } => {
                self.url = Some(url.clone());
                self.controller.did_commit(*navigation_id, url.clone());
                // The entries may have changed since the navigation
                // started (the old document's `pushState`).
                let (index, length) = self.controller.offset_and_length();
                self.process
                    .send(RendererMsg::SetHistoryOffsetAndLength { index, length });
            }
            RendererHostMsg::DidCommitSameDocumentNavigation {
                url,
                replace,
                state,
                navigation_id,
            } => {
                self.url = Some(url.clone());
                match navigation_id {
                    Some(id) => self
                        .controller
                        .did_commit_history_same_document(*id, url.clone()),
                    None => self.controller.did_commit_same_document(
                        url.clone(),
                        *replace,
                        state.clone(),
                    ),
                }
            }
            RendererHostMsg::SameDocumentNavigationFailed { navigation_id } => {
                if let Some(index) = self.controller.did_fail_same_document(*navigation_id) {
                    let url = self.controller.entries()[index].url.clone();
                    self.begin_navigation(url, NavigationType::HistoryIndex(index));
                }
            }
            RendererHostMsg::GoToEntryAtOffset(offset) => {
                self.go_to_offset(*offset as isize);
            }
            RendererHostMsg::UpdateState {
                manual_scroll_restoration,
            } => self
                .controller
                .set_manual_scroll_restoration(*manual_scroll_restoration),
            RendererHostMsg::OpenUrl {
                url,
                disposition,
                replace,
            } => self.open_url(url, *disposition, *replace),
            RendererHostMsg::Reload => self.reload(),
            // The embedder shows it.
            RendererHostMsg::SetCursor(_)
            | RendererHostMsg::TakeFocus { .. }
            | RendererHostMsg::ShowPopupMenu(_)
            | RendererHostMsg::HidePopupMenu => {}
            RendererHostMsg::InputEventAck { .. } => {
                self.input_in_flight = self.input_in_flight.saturating_sub(1);
            }
            // The IO thread answers these itself.
            RendererHostMsg::FetchResource { .. }
            | RendererHostMsg::CancelResource { .. }
            | RendererHostMsg::FollowRedirect { .. }
            | RendererHostMsg::RegisterBlobUrl { .. } => {}
            // Revoked for its loads already; the navigations asked for
            // before it, which came before it, have their blob.
            RendererHostMsg::RevokeBlobUrl { url } => {
                self.network
                    .forget_blob_url(self.process.renderer_id(), url);
            }
            RendererHostMsg::DidFinishLoad {
                failed_subresources,
                ..
            } => {
                for url in failed_subresources {
                    tracing::warn!(%url, "subresource failed to load");
                }
            }
            // For the test harness, and for automated checks.
            RendererHostMsg::ReftestWaitEnded { .. }
            | RendererHostMsg::TestharnessResults { .. }
            | RendererHostMsg::QueryElementResult { .. }
            | RendererHostMsg::VisualStateReady { .. }
            | RendererHostMsg::WebDriverReply { .. }
            | RendererHostMsg::DevToolsMessage { .. } => {}
            // What was in flight will never be acknowledged; the next
            // renderer starts with nothing in flight.
            RendererHostMsg::RendererGone => {
                self.crashed = true;
                self.input_in_flight = 0;
            }
            RendererHostMsg::RendererReady => {}
        }
    }

    pub fn shutdown(&mut self) {
        if let Some(load) = self
            .navigation
            .take()
            .and_then(|navigation| navigation.load)
        {
            load.cancel();
        }
        self.process.shutdown();
        // No renderer is launched again: the messages' way out goes, its
        // last message (`RendererGone`) delivered as the threads ended.
        *self
            .launch
            .on_event
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Box::new(|_, _| {});
    }
}
