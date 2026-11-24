use std::sync::{Arc, Mutex};

use base::SequenceNumber;
use crossbeam_channel::Sender;
use gfx::{RectF, Size};
use ipc::TimeTicks;
use protocol::network::{
    CompletionStatus, LoadFlags, LoadResult, NetError, NetErrorCode, RedirectInfo,
    RequestDestination, ResourceRequest, ResponseHead,
};
use protocol::renderer::{
    BeginNavigationKind, FrameLoadType, HistoryEntryParams, NavigationLoad, RedirectHop,
    RendererHostMsg, RendererMsg, StorageKind, VisualProperties, WindowOpenDisposition,
};
use protocol::viz::{
    BeginFrameArgs, BeginFrameObserver, ContextProvider, DisplayMsg, FrameSinkId, LocalSurfaceId,
    SurfaceId, VizMsg,
};
use protocol::{FrameToken, TabId};

use crate::bad_message::BadMessageReason;
use crate::frame_tree::{CommittedOrigin, FrameTree};

use crate::debug_urls::{DebugUrl, debug_url};
use crate::navigation_controller::{HistoryLoad, NavigationController, NavigationType};
use crate::navigation_request::{
    DocumentLoad, HeldCommit, Loading, NavigationRequest, NavigationResponse,
};
use crate::render_process_host::{CommittedDocument, Committer, is_file_url};
use crate::{RenderProcessHost, RendererFlags, RendererId, RendererLauncher, StoragePartition};

/// What a tab's other threads have for its UI thread: the embedder posts
/// each to the UI thread and hands it to the tab there (the renderer's
/// messages to [`WebContents::on_renderer_message`], if
/// [`WebContents::is_current_renderer`] says they count; a navigation's
/// redirect to [`WebContents::on_navigation_redirected`], its response to
/// [`WebContents::on_navigation_response_started`], its body's data to
/// [`WebContents::on_navigation_body_readable`] and its end to
/// [`WebContents::on_navigation_completed`], a blob URL's whole to
/// [`WebContents::on_navigation_blob_loaded`]; a bad message to
/// [`WebContents::received_bad_message`]), as Chromium's IPC and its
/// `NavigationURLLoader` post to the UI thread. A navigation's events
/// aren't renderer messages: a renderer mustn't be able to send them.
#[derive(Debug)]
pub enum ContentsEvent {
    /// From one of the tab's renderers, through its IO thread: the one it
    /// has, or one it ended or replaced, whose messages count for nothing.
    Renderer {
        renderer: RendererId,
        msg: RendererHostMsg,
    },
    /// What one of the tab's renderers sent its IO thread that the browser
    /// can't take: the tab ends it, if it is the one the tab has.
    BadMessage {
        renderer: RendererId,
        reason: BadMessageReason,
    },
    /// A navigation started, on the UI thread, for those that watch
    /// (DevTools' `frameStartedNavigating` and its request, Chromium's
    /// `WebContentsObserver::DidStartNavigation`).
    NavigationStarted(NavigationStarted),
    /// Navigation `navigation_id` of `frame` reached a redirect and waits
    /// there (`NavigationURLLoaderDelegate::OnRequestRedirected`), from the
    /// network service's thread.
    NavigationRedirected {
        frame: FrameToken,
        navigation_id: u64,
        redirect: RedirectInfo,
    },
    /// Navigation `navigation_id` of `frame` has its response's head
    /// (`NavigationURLLoaderDelegate::OnResponseStarted`), and its body's
    /// pipe: it commits on the UI thread once it has the body's start.
    NavigationResponseStarted {
        frame: FrameToken,
        navigation_id: u64,
        head: ResponseHead,
        body: ipc::data_pipe::Consumer,
    },
    /// More of the body of navigation `navigation_id` of `frame` may be
    /// read, while it hasn't committed: its pipe's watcher's, from the
    /// network service's thread.
    NavigationBodyReadable {
        frame: FrameToken,
        navigation_id: u64,
    },
    /// The load of navigation `navigation_id` of `frame` ended
    /// (`URLLoaderClient::OnComplete`): without a response, it failed
    /// (`OnRequestFailed`), and its error page commits; else its body is
    /// all in its pipe, or stopped short.
    NavigationCompleted {
        frame: FrameToken,
        navigation_id: u64,
        status: CompletionStatus,
    },
    /// Navigation `navigation_id` of `frame` to a blob URL has its blob,
    /// whole, or failed (`BlobURLLoaderFactory`): it commits on the UI
    /// thread.
    NavigationBlobLoaded {
        frame: FrameToken,
        navigation_id: u64,
        result: LoadResult,
    },
}

/// A navigation's load's client (`NavigationURLLoaderImpl`), on the network
/// service's thread: what it hears, posted to the UI thread.
struct NavigationClient {
    frame: FrameToken,
    navigation_id: u64,
    post: Poster,
}

/// Posts an event to the UI thread, from any thread.
type Poster = Arc<dyn Fn(ContentsEvent) + Send + Sync>;

impl net::UrlLoaderClient for NavigationClient {
    fn on_receive_redirect(&mut self, redirect: RedirectInfo) {
        (self.post)(ContentsEvent::NavigationRedirected {
            frame: self.frame,
            navigation_id: self.navigation_id,
            redirect,
        });
    }

    fn on_receive_response(&mut self, head: ResponseHead, body: ipc::data_pipe::Consumer) {
        (self.post)(ContentsEvent::NavigationResponseStarted {
            frame: self.frame,
            navigation_id: self.navigation_id,
            head,
            body,
        });
    }

    fn on_complete(&mut self, status: CompletionStatus) {
        (self.post)(ContentsEvent::NavigationCompleted {
            frame: self.frame,
            navigation_id: self.navigation_id,
            status,
        });
    }
}

/// How many of a body's first bytes a navigation waits for before it
/// commits (all of a shorter one): what the renderer chooses the engine
/// and the encoding by, as `net::kMaxBytesToSniff`.
const BODY_START_LENGTH: usize = 1024;

/// A navigation as it started: of `frame` (the main frame's, if
/// `main_frame`), to `url`; who asked, as DevTools' `Network.Initiator`
/// (JSON) if the renderer said; whether it loads anything (not a page of
/// the browser's own).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationStarted {
    pub frame: FrameToken,
    pub main_frame: bool,
    pub navigation_id: u64,
    pub url: String,
    pub devtools_initiator: Option<String>,
    pub loads: bool,
}

/// Called on the thread an event comes from (the renderer's IO thread, the
/// network service's), to post it to the UI thread.
pub type ContentsEventCallback = Box<dyn Fn(TabId, ContentsEvent) + Send>;

/// `content::WebContents`: one tab.
pub struct WebContents {
    tab: TabId,
    /// The tab's frames, as its renderer told of them; the main frame's
    /// token (the root's) is what DevTools calls the tab and its main
    /// frame (its target id): random, so a page can't guess it (Chromium's
    /// frame tokens are `base::UnguessableToken`s), the same for each of
    /// its renderers, which are told it first.
    frame_tree: FrameTree,
    /// The frame sink its renderer submits to: a new one for each renderer,
    /// so what the one before still sends (it isn't waited for) goes to a
    /// sink Viz has forgotten.
    frame_sink_id: FrameSinkId,
    /// Numbers the renderers' frame sinks from 1.
    sink_ids: SequenceNumber<u32>,
    process: RenderProcessHost,
    /// The profile's storage: its network context and its `localStorage`.
    partition: StoragePartition,
    /// For the debug URLs that act on Viz.
    viz: Sender<VizMsg>,
    visual_properties: VisualProperties,
    /// The browser window's rect on the screen, CSS px, as the renderer
    /// was last told (`RenderWidgetHostImpl::SendScreenRects`); none yet.
    window_screen_rect: Option<RectF>,
    /// Numbers the navigations started from 1: each gets a larger id.
    navigation_ids: SequenceNumber<u64>,
    /// The last of them that loads a document in the main frame (not a
    /// traversal within the document shown, nor a child frame's).
    document_navigation_id: u64,
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
    /// DevTools has the HTTP cache disabled for the tab
    /// (`Network.setCacheDisabled`): its navigations go past it.
    cache_disabled: bool,
    /// How many of the renderer's own new entries it has heard of
    /// (`DidCommitSameDocumentNavigation`s neither `replace` nor a
    /// traversal), which every place it gives the renderer says
    /// (`entries_heard`): the renderer counts those it made after on top.
    /// A new renderer's start again.
    entries_heard: SequenceNumber<u64>,
    /// How to launch a renderer, again after one has gone.
    launch: Launch,
    /// Input events handed to the renderer that it (or its compositor) has
    /// yet to acknowledge (`RenderWidgetHostImpl::in_flight_event_count_`).
    input_in_flight: u32,
}

/// What the renderer said of a navigation it asked for: its number for
/// it, and who asked, for DevTools; nothing for the browser's own, but
/// whether its reload bypasses the HTTP cache.
#[derive(Debug, Default)]
struct Asked {
    begin_id: Option<u64>,
    /// The frame that asked: an `about:blank` it navigates to has its
    /// document's origin.
    initiator: Option<FrameToken>,
    devtools_initiator: Option<String>,
    /// A reload that bypasses the HTTP cache (Shift+Reload, DevTools'
    /// `ignoreCache`: Chromium's `RELOAD_BYPASSING_CACHE`).
    bypass_cache: bool,
    /// The renderer's say on how it uses the HTTP cache (a child frame's
    /// in a reload past it), added to the browser's own.
    load_flags: LoadFlags,
}

/// What launches this tab's renderers.
struct Launch {
    flags: RendererFlags,
    launcher: RendererLauncher,
    gpu: Option<Arc<dyn ContextProvider>>,
    /// Every renderer's messages, and every navigation's events, go here.
    on_event: Arc<Mutex<ContentsEventCallback>>,
}

impl Launch {
    fn process(
        &self,
        tab: TabId,
        frame_sink_id: FrameSinkId,
        viz: &Sender<VizMsg>,
        partition: &StoragePartition,
    ) -> std::io::Result<RenderProcessHost> {
        let on_event = self.on_event.clone();
        RenderProcessHost::launch(
            tab,
            frame_sink_id,
            self.flags.clone(),
            &self.launcher,
            viz.clone(),
            self.gpu.clone(),
            partition.clone(),
            Box::new(move |tab, renderer, msg| {
                let on_event = on_event.lock().unwrap_or_else(|e| e.into_inner());
                on_event(
                    tab,
                    match msg {
                        Ok(msg) => ContentsEvent::Renderer { renderer, msg },
                        Err(reason) => ContentsEvent::BadMessage { renderer, reason },
                    },
                );
            }),
        )
    }
}

/// A tab's main frame token, its id for DevTools: 128 bits of the
/// operating system's random numbers, as Chrome's.
fn devtools_frame_token() -> FrameToken {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
    FrameToken(bytes)
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
        launcher: RendererLauncher,
        viz: Sender<VizMsg>,
        gpu: Option<Arc<dyn ContextProvider>>,
        partition: StoragePartition,
        on_event: ContentsEventCallback,
    ) -> std::io::Result<Self> {
        // One renderer "process" at a time per tab, one frame sink per
        // renderer (client 0 is the browser's own).
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "tab ids count the tabs opened: far fewer than u32::MAX"
        )]
        let client_id = tab.0 + 1;
        let mut sink_ids = SequenceNumber::starting_at(1);
        let frame_sink_id = FrameSinkId {
            client_id,
            sink_id: sink_ids.get_next(),
        };
        let launch = Launch {
            flags,
            launcher,
            gpu,
            on_event: Arc::new(Mutex::new(on_event)),
        };
        let process = launch.process(tab, frame_sink_id, &viz, &partition)?;
        let devtools_frame_token = devtools_frame_token();
        process.send(RendererMsg::DevToolsMainFrameToken(devtools_frame_token));
        Ok(Self {
            tab,
            frame_tree: FrameTree::new(devtools_frame_token),
            frame_sink_id,
            sink_ids,
            process,
            partition,
            viz,
            navigation_ids: SequenceNumber::starting_at(1),
            document_navigation_id: 0,
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
            cache_disabled: false,
            entries_heard: SequenceNumber::new(),
            launch,
            input_in_flight: 0,
        })
    }

    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// The main frame's token: what DevTools calls the tab and its main
    /// frame (its target id, written as `FrameToken` writes it).
    pub fn devtools_frame_token(&self) -> FrameToken {
        self.frame_tree.root()
    }

    /// Whether a message from `renderer` counts: the tab's renderer's, while
    /// it is there. One it replaced, or ended (`about:crash`, a bad message),
    /// may still have sent some, which the embedder hasn't handed on yet:
    /// those count for nothing, nor does anything after it has gone.
    pub fn is_current_renderer(&self, renderer: RendererId) -> bool {
        renderer == self.process.id() && !self.crashed
    }

    /// The tab's frames: the main frame, and the child frames its renderer
    /// told of.
    pub fn frame_tree(&self) -> &FrameTree {
        &self.frame_tree
    }

    /// The frame sink this tab's renderer submits to: another once a
    /// renderer is launched again.
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
            .with_pixels_returned(self.process.pixels_returned_sender())
    }

    /// Viz has done with the pixels of the tab's renderer's shared image
    /// `mailbox`: for a Viz that isn't told through the observer.
    pub fn shared_image_pixels_returned(&self, mailbox: protocol::viz::Mailbox) {
        (self.process.pixels_returned_sender())(mailbox);
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

    /// The network context of the tab's profile (its storage partition's):
    /// what DevTools' and automation's cookie commands go through.
    pub fn network(&self) -> &net::NetworkContext {
        self.partition.network()
    }

    /// The URLs of the documents the tab's frames show, the main frame's
    /// first (DevTools' `ComputeCookieURLs`); none for a frame that has
    /// shown none of the browser's yet.
    pub fn frame_urls(&self) -> Vec<String> {
        let root = self.frame_tree.root();
        let others = self
            .frame_tree
            .frames()
            .into_iter()
            .filter(|&frame| frame != root);
        std::iter::once(root)
            .chain(others)
            .filter_map(|frame| self.frame_tree.url(frame).map(str::to_owned))
            .collect()
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
        self.frame_tree
            .navigation(self.frame_tree.root())
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
                // A renderer process crashes itself; a renderer that is
                // this process's threads is ended from here. Either way
                // it is gone by the next thing the tab is asked.
                if !self.process.crash() {
                    self.process.kill();
                }
                self.crashed = true;
                self.frame_tree.reset();
            }
            None => self.begin_navigation(url, NavigationType::NewEntry, Asked::default()),
        }
    }

    /// The tab's first document, `url`, as an initial entry: the first
    /// new entry takes its place (Chromium's `IsInitialEntry`), so there is
    /// no going back to it. What starts a tab that something drives (`machopo
    /// --automation-pipe`), as Chrome's new tab has its `about:blank`.
    pub fn navigate_initial(&mut self, url: impl Into<String>) {
        self.begin_navigation(url.into(), NavigationType::Initial, Asked::default());
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
        self.reload_as(Asked::default());
    }

    /// DevTools has the HTTP cache disabled for the tab, or not
    /// (`Network.setCacheDisabled`, as Chromium's `NetworkHandler` keeps it
    /// for the navigations): its navigations go past it while it does.
    pub fn set_cache_disabled(&mut self, disabled: bool) {
        self.cache_disabled = disabled;
    }

    /// `NavigationController::Reload` with `RELOAD_BYPASSING_CACHE`: loads
    /// the entry shown again past the HTTP cache, its subresources too until
    /// its `load`.
    pub fn reload_bypassing_cache(&mut self) {
        self.reload_as(Asked {
            bypass_cache: true,
            ..Asked::default()
        });
    }

    fn reload_as(&mut self, asked: Asked) {
        let current = self.controller.current_index();
        if let Some(entry) = current.and_then(|current| self.controller.entry(current)) {
            let url = entry.url().to_owned();
            self.begin_navigation(url, NavigationType::Reload, asked);
        }
    }

    /// Goes to the entry `offset` steps away (`GoToOffset`): each frame
    /// the entry has at another item than the one it shows
    /// (`frames_to_navigate`) traverses within its document if the item is
    /// of it (the renderer fires `popstate`), else loads the item again,
    /// its state with it; those within their documents first, as Chromium
    /// sends them. There is no back-forward cache, and the scroll position
    /// isn't restored. With no frame to move (those that differ gone), it is
    /// the current entry, nothing loaded, the renderer told its place. To
    /// the current entry itself (back, then forward, before the first
    /// commits), every frame stops loading and nothing else happens
    /// (Chromium's `NavigateToExistingPendingEntry` drops it so). After a
    /// crash, the main frame loads it. False if there is no such entry.
    pub fn go_to_offset(&mut self, offset: isize) -> bool {
        let Some(index) = self.controller.index_for_offset(offset) else {
            return false;
        };
        if !self.crashed && self.controller.current_index() == Some(index) {
            self.stop_loading();
            return true;
        }
        let root = self.frame_tree.root();
        let loads = if self.crashed {
            vec![(root, HistoryLoad::DifferentDocument)]
        } else {
            let tree = &self.frame_tree;
            self.controller
                .frames_to_navigate(tree, index, |frame| tree.history_place(frame))
        };
        if loads.is_empty() {
            self.controller.go_to_entry_in_place(index);
            self.send_history_offset_and_length();
            return true;
        }
        let (within, loading): (Vec<_>, Vec<_>) = loads
            .into_iter()
            .partition(|(_, load)| *load == HistoryLoad::SameDocument);
        for (frame, load) in within.into_iter().chain(loading) {
            let Some(item) = self.controller.item_at(&self.frame_tree, frame, index) else {
                continue;
            };
            let (url, document) = (item.url.clone(), item.document);
            if load == HistoryLoad::DifferentDocument {
                self.history_load(frame, index, url, Asked::default());
                continue;
            }
            let kind = NavigationType::HistoryIndex(index);
            let navigation_id = self.navigation_ids.get_next();
            let history = self.controller.entry_params(&self.frame_tree, frame, kind);
            self.controller.did_start(navigation_id, frame, kind);
            tracing::info!(%url, navigation_id, "navigate within the document");
            self.process
                .send(RendererMsg::CommitSameDocumentNavigation {
                    frame,
                    navigation_id,
                    document,
                    url,
                    history,
                });
        }
        true
    }

    /// `frame`'s navigation to its item in the entry at `index`, at `url`,
    /// loading its document again (a history navigation): the main frame's
    /// as `begin_navigation`'s.
    fn history_load(&mut self, frame: FrameToken, index: usize, url: String, asked: Asked) {
        let kind = NavigationType::HistoryIndex(index);
        if frame == self.frame_tree.root() {
            return self.begin_navigation(url, kind, asked);
        }
        let navigation_id = self.navigation_ids.get_next();
        let history = self.controller.entry_params(&self.frame_tree, frame, kind);
        self.controller.did_start(navigation_id, frame, kind);
        self.start_navigation(
            frame,
            navigation_id,
            url,
            asked,
            history,
            FrameLoadType::BackForward,
            TimeTicks::now(),
        );
    }

    /// `RenderFrameHostImpl::BeginNavigation`: frame `frame` of the page is
    /// to have another document, as frame `initiator` asked; `id` the
    /// renderer's number for it. A frame the tab doesn't have is a bad
    /// message. What a page may not navigate a frame to
    /// (`page_may_navigate_to`), and a child frame's reload before the
    /// browser loaded it anything, go nowhere: the renderer hears it is
    /// dropped (`NavigationDropped`).
    #[expect(
        clippy::too_many_arguments,
        reason = "the renderer's `BeginNavigation` has that many parts"
    )]
    fn begin_renderer_navigation(
        &mut self,
        frame: FrameToken,
        initiator: FrameToken,
        id: u64,
        kind: &BeginNavigationKind,
        devtools_initiator: Option<&str>,
        history_in_new_child_frame: bool,
        load_flags: LoadFlags,
    ) {
        if !self.frame_tree.contains(frame) {
            return self.received_bad_message(BadMessageReason::NavigationOfUnknownFrame);
        }
        if !self.frame_tree.contains(initiator) {
            return self.received_bad_message(BadMessageReason::NavigationByUnknownFrame);
        }
        let main_frame = frame == self.frame_tree.root();
        let asked = Asked {
            begin_id: Some(id),
            initiator: Some(initiator),
            devtools_initiator: devtools_initiator.map(String::from),
            // A page's `location.reload()` validates; only the browser
            // reloads past the cache.
            bypass_cache: false,
            load_flags,
        };
        // A new child frame of a document back or forward loaded: what the
        // session history has for it in the entry its parent's document is
        // of, if any (Chromium's `StartHistoryNavigationInNewSubframe`),
        // the history's own, not checked as what a page asks is.
        if history_in_new_child_frame && !self.frame_tree.has_committed_real_load(frame) {
            let restored = self
                .frame_tree
                .parent(frame)
                .and_then(|parent| self.frame_tree.history_place(parent))
                .and_then(|place| {
                    self.controller
                        .item_for_new_child_frame(&self.frame_tree, frame, place.entry)
                })
                .map(|(index, item)| (index, item.url.clone()));
            if let Some((index, url)) = restored {
                return self.history_load(frame, index, url, asked);
            }
        }
        let (url, navigation_type) = match kind {
            BeginNavigationKind::To { url, .. } => {
                let local = self.frame_tree.may_open_local_files(initiator);
                if let Err(why) = self.page_may_navigate_to(url, main_frame, local) {
                    tracing::warn!(%url, why, "a page's navigation goes nowhere");
                    return self.dropped(frame, id);
                }
                (url.clone(), FrameLoadType::Standard)
            }
            BeginNavigationKind::Reload if main_frame => return self.reload_as(asked),
            BeginNavigationKind::Reload => match self.frame_tree.url(frame) {
                Some(url) => (url.to_owned(), FrameLoadType::Reload),
                None => return self.dropped(frame, id),
            },
        };
        if main_frame {
            let kind = match kind {
                BeginNavigationKind::To { replace: true, .. } => NavigationType::Replace,
                _ => NavigationType::NewEntry,
            };
            return self.begin_navigation(url, kind, asked);
        }
        // A child frame's next document is a new entry, but in place of
        // the document it replaces if asked, and of its first: its initial
        // `about:blank`'s or another `about:blank`'s (Chromium's
        // `ShouldReplaceCurrentEntryForNavigationFromInitialEmptyDocumentOrEntry`,
        // by `has_committed_real_load` as it was), or one of the initial
        // entry.
        let kind = match kind {
            BeginNavigationKind::Reload => NavigationType::Reload,
            BeginNavigationKind::To { replace, .. }
                if *replace
                    || !self.frame_tree.has_committed_real_load(frame)
                    || self.controller.is_initial() =>
            {
                NavigationType::Replace
            }
            BeginNavigationKind::To { .. } => NavigationType::NewEntry,
        };
        let navigation_id = self.navigation_ids.get_next();
        let history = self.controller.entry_params(&self.frame_tree, frame, kind);
        self.controller.did_start(navigation_id, frame, kind);
        self.start_navigation(
            frame,
            navigation_id,
            url,
            asked,
            history,
            navigation_type,
            TimeTicks::now(),
        );
    }

    /// The tasks that asked for the renderer's navigations `ids` are over
    /// (`NavigationCancellationWindowEnded`): each still on its way may
    /// commit, and one ready commits now, as it was to.
    fn cancellation_windows_ended(&mut self, ids: &[u64]) {
        for &id in ids {
            let Some((frame, navigation_id)) = self.frame_tree.navigation_of_begin_id(id) else {
                continue;
            };
            let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id) else {
                continue;
            };
            navigation.cancellation_window = false;
            match navigation.held.take() {
                None => {}
                Some(HeldCommit::Response) => self.commit_response(frame, navigation_id),
                Some(HeldCommit::Failed) => {
                    if let Some(status) = navigation.end.take() {
                        self.on_navigation_completed(frame, navigation_id, status);
                    }
                }
                Some(HeldCommit::Blob(result)) => {
                    self.on_navigation_blob_loaded(frame, navigation_id, result);
                }
                Some(HeldCommit::Page(document, origin)) => {
                    if let Some(navigation) = self.frame_tree.take_navigation(frame, navigation_id)
                    {
                        self.commit(
                            frame,
                            navigation_id,
                            navigation.committer,
                            document,
                            Some(origin),
                        );
                    }
                }
            }
        }
    }

    /// The renderer's navigation `id` of `frame` won't commit.
    fn dropped(&self, frame: FrameToken, id: u64) {
        self.process
            .send(RendererMsg::NavigationDropped { frame, id });
    }

    /// `RenderFrameHostImpl::OpenURL`: a link the renderer followed to a new
    /// tab, which the embedder opens (`take_opened`), if a page may
    /// navigate a main frame there.
    fn open_url(&mut self, url: &str, disposition: WindowOpenDisposition) {
        let local = self.process.can_request(url);
        if let Err(why) = self.page_may_navigate_to(url, true, local) {
            tracing::warn!(%url, why, "a page's new tab goes nowhere");
            return;
        }
        self.opened.push(OpenUrlParams {
            url: url.to_owned(),
            disposition,
        });
    }

    /// Whether a page may navigate a frame to `url` (the main frame, or a
    /// new tab's, if `main_frame`), as a document that may open local files
    /// if `local` asked: the browser's checks of what any renderer asks,
    /// in one place. Only schemes the browser loads are navigated to;
    /// Chromium hands others (`mailto:`) to the system, which isn't done
    /// yet. A `data:` URL is loaded, but not for a page's main frame, here
    /// or in a new tab (Chromium's `DataUrlNavigationThrottle`: a page that
    /// makes its own URL is a phishing tool); child frames and the user's
    /// navigations may have one. Of `about:` URLs a page may navigate to
    /// `about:blank`, and a child frame to `about:srcdoc`: the browser's own
    /// pages and its debug URLs are the user's alone, so that no page can
    /// crash the GPU. A local file only a local file's document may open,
    /// as Blink's `SecurityOrigin::CanDisplay` has it.
    fn page_may_navigate_to(
        &self,
        url: &str,
        main_frame: bool,
        local: bool,
    ) -> Result<(), &'static str> {
        let scheme = url.split_once(':').map_or(url, |(scheme, _)| scheme);
        match scheme.to_ascii_lowercase().as_str() {
            "http" | "https" | "file" | "blob" => {}
            "data" if main_frame => return Err("a page may not navigate to a data: URL"),
            "data" => {}
            "about" => match crate::loader::about_path(url) {
                Some("blank") => {}
                Some("srcdoc") if !main_frame => {}
                _ => return Err("a page may not navigate to that about: URL"),
            },
            _ => return Err("no handler for the scheme"),
        }
        if is_file_url(url) && !local {
            return Err("only a local file's document may open local files");
        }
        Ok(())
    }

    /// The links followed to new tabs since last asked, in order.
    pub fn take_opened(&mut self) -> Vec<OpenUrlParams> {
        std::mem::take(&mut self.opened)
    }

    /// Whether the renderer has gone: its sad tab shows until the next
    /// navigation.
    /// Hands the tab's renderer's side a message, for a renderer with
    /// nothing at the other end (`RendererLauncher::Detached`): it goes
    /// the way a renderer's does, through the checks made where its
    /// channel is read, and what passes comes out as the tab's events do.
    pub fn deliver_from_renderer(&mut self, msg: RendererHostMsg) {
        self.process.deliver(msg);
    }

    /// The renderer with nothing at the other end goes, as one whose
    /// channel is over.
    pub fn deliver_renderer_gone(&mut self) {
        self.process.deliver_gone();
    }

    /// The process the tab's renderer is, if it is one of its own
    /// (Chromium's `RenderProcessHost::GetProcess`).
    pub fn renderer_pid(&self) -> Option<u32> {
        self.process.pid()
    }

    /// The shared memory the browser keeps for what it sends the tab's
    /// renderer process: its bytes, and how many of its regions are free.
    /// None for a renderer in this process.
    pub fn renderer_shared_memory(&self) -> Option<(usize, usize)> {
        self.process.shared_memory()
    }

    /// How the tab's renderer process ended, if it has: what the system
    /// says of it (an exit code, or the signal that ended it).
    pub fn renderer_exit(&self) -> Option<std::process::ExitStatus> {
        self.process.exit()
    }

    pub fn is_crashed(&self) -> bool {
        self.crashed
    }

    /// `RenderFrameHostManager`'s new process for a navigation after a
    /// crash: the same tab, the size and visibility it had, and a new frame
    /// sink, as Chromium's new process has. Viz forgets the old sink and its
    /// surface, so the old renderer's last frame doesn't show until the new
    /// one draws, and what the old renderer still sends (it isn't waited
    /// for: it may hang) is dropped: an answer to a `BeginFrame`, or that
    /// it wants none, would otherwise stand for the new one's. Viz hears of
    /// the new sink before the new renderer hears anything.
    fn relaunch(&mut self) {
        let frame_sink_id = FrameSinkId {
            client_id: self.frame_sink_id.client_id,
            sink_id: self.sink_ids.get_next(),
        };
        let process = match self
            .launch
            .process(self.tab, frame_sink_id, &self.viz, &self.partition)
        {
            Ok(process) => process,
            Err(e) => {
                tracing::error!("failed to launch a renderer again: {e}");
                return;
            }
        };
        // The frame tree is the main frame alone already: it was reset as
        // the renderer before went. The new one tells of its frames.
        std::mem::replace(&mut self.process, process).abandon();
        let _ = self
            .viz
            .send(VizMsg::InvalidateFrameSinkId(self.frame_sink_id));
        self.frame_sink_id = frame_sink_id;
        let _ = self.viz.send(VizMsg::RegisterFrameSinkId(
            self.frame_sink_id,
            self.begin_frame_observer(),
        ));
        self.crashed = false;
        self.entries_heard = SequenceNumber::new();
        self.process
            .send(RendererMsg::DevToolsMainFrameToken(self.frame_tree.root()));
        self.process
            .send(RendererMsg::UpdateVisualProperties(self.visual_properties));
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
    }

    /// The main frame's navigation to `url`, a new entry or another of
    /// the session history's (`kind`).
    fn begin_navigation(&mut self, url: String, kind: NavigationType, asked: Asked) {
        // The new document's time origin (HR-Time), whatever starts it.
        let navigation_start = ipc::TimeTicks::now();
        if self.crashed {
            self.relaunch();
        }
        let navigation_id = self.navigation_ids.get_next();
        self.document_navigation_id = navigation_id;
        let root = self.frame_tree.root();
        let history = self.controller.entry_params(&self.frame_tree, root, kind);
        self.controller.did_start(navigation_id, root, kind);
        tracing::info!(%url, navigation_id, ?kind, "navigate");
        self.url = Some(url.clone());
        let navigation_type = match kind {
            NavigationType::Reload if asked.bypass_cache => FrameLoadType::ReloadBypassingCache,
            NavigationType::Reload => FrameLoadType::Reload,
            NavigationType::HistoryIndex(_) => FrameLoadType::BackForward,
            NavigationType::NewEntry | NavigationType::Replace | NavigationType::Initial => {
                FrameLoadType::Standard
            }
        };
        self.start_navigation(
            self.frame_tree.root(),
            navigation_id,
            url,
            asked,
            history,
            navigation_type,
            navigation_start,
        );
    }

    /// `frame`'s `NavigationRequest`, navigation `navigation_id` to `url`:
    /// loaded, then committed (`on_navigation_completed`), a page of the
    /// browser's own at once. The frame's navigation before stops (Chromium
    /// resets its `NavigationRequest`), the renderer told if it asked for
    /// it.
    #[expect(
        clippy::too_many_arguments,
        reason = "a navigation has that many parts"
    )]
    fn start_navigation(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        url: String,
        asked: Asked,
        history: HistoryEntryParams,
        navigation_type: FrameLoadType,
        navigation_start: TimeTicks,
    ) {
        let main_frame = frame == self.frame_tree.root();
        let page = self.about_document(frame, &url, asked.initiator);
        self.post(ContentsEvent::NavigationStarted(NavigationStarted {
            frame,
            main_frame,
            navigation_id,
            url: url.clone(),
            devtools_initiator: asked.devtools_initiator,
            loads: page.is_none(),
        }));
        let committer = self.process.committer(
            frame,
            asked.begin_id,
            history,
            navigation_start,
            navigation_type,
        );
        if let Some((document, origin)) = page {
            // The renderer's: held till the task that asked is over.
            if asked.begin_id.is_some() {
                let navigation = NavigationRequest {
                    id: navigation_id,
                    begin_id: asked.begin_id,
                    url,
                    redirects: Vec::new(),
                    committer,
                    load: Loading::default(),
                    response: None,
                    end: None,
                    cancellation_window: true,
                    held: Some(HeldCommit::Page(document, origin)),
                };
                if let Ok(Some(before)) = self.frame_tree.set_navigation(frame, navigation) {
                    self.replaced(frame, before);
                }
                return;
            }
            if let Some(before) = self.frame_tree.take_any_navigation(frame) {
                self.replaced(frame, before);
            }
            self.commit(frame, navigation_id, committer, document, Some(origin));
            return;
        }
        let destination = if main_frame {
            RequestDestination::Document
        } else {
            RequestDestination::Iframe
        };
        let mut request = ResourceRequest::new(url.clone(), destination);
        request.trusted = Some(
            self.frame_tree
                .navigation_params(frame, &url, asked.initiator),
        );
        request.load_flags = navigation_load_flags(navigation_type).union(asked.load_flags);
        // DevTools' cache disabled: past it, whatever the navigation
        // (`ApplyNetworkRequestOverrides`).
        if self.cache_disabled {
            request.load_flags = LoadFlags {
                bypass_cache: true,
                ..LoadFlags::default()
            };
        }
        let navigation = NavigationRequest {
            id: navigation_id,
            begin_id: asked.begin_id,
            url,
            redirects: Vec::new(),
            committer,
            load: Loading::default(),
            response: None,
            end: None,
            cancellation_window: asked.begin_id.is_some(),
            held: None,
        };
        match self.frame_tree.set_navigation(frame, navigation) {
            Ok(Some(before)) => self.replaced(frame, before),
            Ok(None) => {}
            // Its frame went.
            Err(_) => return,
        }
        // A blob URL's blob, as it is now (`BlobURLLoaderFactory`), whole.
        if crate::render_process_host::is_blob_url(&request.url) {
            let result = self.partition.network().load_blob_url(&request, None);
            (self.poster())(ContentsEvent::NavigationBlobLoaded {
                frame,
                navigation_id,
                result,
            });
            return;
        }
        // Its redirects, its response and its end, to the UI thread
        // (`NavigationURLLoader` tells its `NavigationRequest`).
        let client = NavigationClient {
            frame,
            navigation_id,
            post: self.poster(),
        };
        let load = self
            .partition
            .network()
            .load(request, net::AtRedirects::Wait, client);
        if let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id) {
            navigation.load.set(load);
        }
    }

    /// What posts an event to the UI thread, as the network's are.
    fn poster(&self) -> Poster {
        let on_event = self.launch.on_event.clone();
        let tab = self.tab;
        Arc::new(move |event| {
            let on_event = on_event.lock().unwrap_or_else(|e| e.into_inner());
            on_event(tab, event);
        })
    }

    /// The document of `frame`'s navigation to `url`, as `initiator` asked,
    /// if it loads nothing (Chromium's `NeedsUrlLoader`), and its origin:
    /// `about:blank`'s empty, of the initiator's origin (an opaque one
    /// for one the browser starts); a child frame's `about:srcdoc`, its
    /// iframe's `srcdoc`, of its parent's origin, if it may have it
    /// (`may_commit_srcdoc`), else the error page; the main frame's other
    /// built-in pages, of no origin. `None` for any other URL.
    fn about_document(
        &self,
        frame: FrameToken,
        url: &str,
        initiator: Option<FrameToken>,
    ) -> Option<(CommittedDocument, CommittedOrigin)> {
        let main_frame = frame == self.frame_tree.root();
        let page = |markup: &str| CommittedDocument::page(url.to_owned(), markup.to_owned());
        match crate::loader::about_path(url)? {
            "blank" => {
                let origin = initiator
                    .and_then(|initiator| self.frame_tree.committed_origin(initiator))
                    .unwrap_or_else(CommittedOrigin::opaque);
                Some((page(""), origin))
            }
            "srcdoc" if !main_frame => {
                if initiator.is_some_and(|initiator| !self.may_commit_srcdoc(frame, initiator)) {
                    tracing::warn!(url, "a navigation to about:srcdoc the page may not make");
                    let error = NetError::new(NetErrorCode::InvalidUrl, url);
                    let page = crate::loader::error_page(&error);
                    let document = CommittedDocument::error_page(url.to_owned(), page);
                    return Some((document, CommittedOrigin::opaque()));
                }
                let parent = self.frame_tree.parent(frame)?;
                let origin = self.frame_tree.committed_origin(parent)?;
                Some((page(self.frame_tree.srcdoc(frame)?), origin))
            }
            _ if main_frame => {
                let markup = crate::loader::about_page(url)?;
                Some((page(markup), CommittedOrigin::opaque()))
            }
            _ => None,
        }
    }

    /// Whether `initiator` may have child frame `frame` commit its
    /// `about:srcdoc` (Chromium's `NavigationRequest::CheckAboutSrcDoc`):
    /// its content is its parent's, so an initiator of its parent's origin
    /// may, or one of its own origin while it shows one already (it
    /// reloads). Any other may not: HTML has no navigations to it.
    fn may_commit_srcdoc(&self, frame: FrameToken, initiator: FrameToken) -> bool {
        let tree = &self.frame_tree;
        let Some(by) = tree.origin(initiator) else {
            return false;
        };
        let reloads = tree
            .url(frame)
            .is_some_and(|url| crate::loader::about_path(url) == Some("srcdoc"))
            && tree.origin(frame) == Some(by);
        let parents = tree
            .parent(frame)
            .is_some_and(|parent| tree.origin(parent) == Some(by));
        reloads || parents
    }

    /// `before`, `frame`'s navigation, stops for another: its load goes with
    /// it, and the renderer hears that one it asked for won't commit.
    /// `FrameTree::StopLoading`: every frame's navigation on its way stops,
    /// the renderer told of those it asked for, and the session history
    /// forgets them; and every frame's document whose body still comes
    /// stops loading.
    fn stop_loading(&mut self) {
        for frame in self.frame_tree.frames() {
            if let Some(navigation) = self.frame_tree.take_any_navigation(frame) {
                self.controller.did_stop(navigation.id);
                self.replaced(frame, navigation);
            }
            // Its document's body, if it still comes, stops too: the
            // renderer hears its load was aborted (Chromium's main frame's
            // `RenderFrameHostManager::Stop`, which stops every frame's
            // loading in the renderer).
            drop(self.frame_tree.take_any_document_load(frame));
        }
    }

    fn replaced(&self, frame: FrameToken, before: NavigationRequest) {
        if let Some(id) = before.begin_id {
            self.dropped(frame, id);
        }
    }

    /// `event` to the UI thread, as the network's are.
    fn post(&self, event: ContentsEvent) {
        let on_event = self
            .launch
            .on_event
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        on_event(self.tab, event);
    }

    /// `frame` commits `document`, navigation `navigation_id`'s, through
    /// `committer`, which is the frame's as the renderer reports it
    /// committed: of `origin` if it has one from another document, else
    /// its URL's (an error page's, opaque). Whether the renderer may read
    /// local files goes with it (`ChildProcessSecurityPolicy::
    /// GrantCommitURL`), before the renderer has it: the main frame's new
    /// document decides, its child frames going with the one before; a
    /// child frame's local one lets it too.
    fn commit(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        committer: Committer,
        document: CommittedDocument,
        origin: Option<CommittedOrigin>,
    ) -> crate::render_process_host::LoadCompleter {
        let origin = origin.unwrap_or_else(|| document.origin());
        let local = origin.local;
        if frame == self.frame_tree.root() {
            self.process.set_file_access(local);
        } else if local {
            self.process.set_file_access(true);
        }
        // Its loader's params, by the frames it is in now (before its own
        // commit, which the renderer hasn't reported).
        let params = self.frame_tree.loader_params(frame, &origin.origin);
        self.frame_tree
            .committing(frame, navigation_id, &document.url, origin);
        let before = self.frame_tree.take_any_document_load(frame);
        let mut committer = committer;
        let place = self.controller.commit_place(navigation_id);
        committer.set_history_place(place, self.entries_heard.peek());
        let loader = self.process.register_loader(params);
        let completer = committer.commit(navigation_id, document, loader);
        // The document before is no more: its body stops, and the renderer
        // hears it was aborted after the commit that takes its place (it
        // would commit what it has of it, else).
        drop(before);
        completer
    }

    /// One of a navigation's events (its redirect, response, body's data,
    /// end, or a blob URL's whole), on the UI thread, to the method that
    /// takes it; any other is no navigation's, and is left.
    pub fn on_navigation_event(&mut self, event: ContentsEvent) {
        match event {
            ContentsEvent::NavigationRedirected {
                frame,
                navigation_id,
                redirect,
            } => self.on_navigation_redirected(frame, navigation_id, &redirect),
            ContentsEvent::NavigationResponseStarted {
                frame,
                navigation_id,
                head,
                body,
            } => self.on_navigation_response_started(frame, navigation_id, head, body),
            ContentsEvent::NavigationBodyReadable {
                frame,
                navigation_id,
            } => self.on_navigation_body_readable(frame, navigation_id),
            ContentsEvent::NavigationCompleted {
                frame,
                navigation_id,
                status,
            } => self.on_navigation_completed(frame, navigation_id, status),
            ContentsEvent::NavigationBlobLoaded {
                frame,
                navigation_id,
                result,
            } => self.on_navigation_blob_loaded(frame, navigation_id, result),
            ContentsEvent::Renderer { .. }
            | ContentsEvent::BadMessage { .. }
            | ContentsEvent::NavigationStarted(_) => {}
        }
    }

    /// Navigation `navigation_id` of `frame` reached a redirect
    /// (`NavigationRequest::OnRequestRedirected`, on the UI thread): it
    /// goes on, unless another navigation has replaced it (its load was
    /// cancelled then), or its frame went.
    pub fn on_navigation_redirected(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        redirect: &RedirectInfo,
    ) {
        if let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id) {
            navigation.redirects.push(RedirectHop::of(redirect));
            navigation.load.follow_redirect();
        }
    }

    /// Navigation `navigation_id` of `frame` has its response's head
    /// (`NavigationRequest::OnResponseStarted`, on the UI thread): it
    /// commits once it has the body's first bytes, or all of a shorter
    /// one, unless another navigation has replaced it (its load was
    /// cancelled then), or its frame went.
    pub fn on_navigation_response_started(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        head: ResponseHead,
        mut body: ipc::data_pipe::Consumer,
    ) {
        let post = self.poster();
        let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id) else {
            return;
        };
        body.watch(move || {
            post(ContentsEvent::NavigationBodyReadable {
                frame,
                navigation_id,
            });
        });
        navigation.response = Some(NavigationResponse {
            head,
            body,
            start: Vec::new(),
            start_length: 0,
            done: false,
        });
        self.read_body_start(frame, navigation_id);
    }

    /// More of the body of navigation `navigation_id` of `frame` may be
    /// read: it commits if it has its start now.
    pub fn on_navigation_body_readable(&mut self, frame: FrameToken, navigation_id: u64) {
        self.read_body_start(frame, navigation_id);
    }

    /// Reads the start of the body of navigation `navigation_id` of
    /// `frame`, if it is still on its way, and commits it once it is in.
    fn read_body_start(&mut self, frame: FrameToken, navigation_id: u64) {
        let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id) else {
            return;
        };
        let Some(response) = &mut navigation.response else {
            return;
        };
        while !response.done && response.start_length < BODY_START_LENGTH {
            match response
                .body
                .read(BODY_START_LENGTH.saturating_sub(response.start_length))
            {
                ipc::data_pipe::Read::Data(data) => {
                    response.start_length = response.start_length.saturating_add(data.len());
                    response.start.push(data);
                }
                ipc::data_pipe::Read::Empty => return,
                ipc::data_pipe::Read::Done => response.done = true,
            }
        }
        self.commit_response(frame, navigation_id);
    }

    /// The load of navigation `navigation_id` of `frame` ended: without a
    /// response, it failed (`NavigationRequest::OnRequestFailed`), and its
    /// error page commits; with one before it committed, it commits with
    /// what came; after, the renderer hears how it ended. Unless another
    /// navigation has replaced it, another document has committed in the
    /// frame since, or its frame went.
    pub fn on_navigation_completed(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        status: CompletionStatus,
    ) {
        if let Some(load) = self.frame_tree.take_document_load(frame, navigation_id) {
            load.complete(status);
            return;
        }
        let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id) else {
            return;
        };
        if navigation.response.is_some() {
            // The body is all in its pipe by now: read the rest of its
            // start.
            navigation.end = Some(status);
            self.read_body_start(frame, navigation_id);
            return;
        }
        if navigation.cancellation_window {
            navigation.end = Some(status);
            navigation.held = Some(HeldCommit::Failed);
            return;
        }
        let Some(navigation) = self.frame_tree.take_navigation(frame, navigation_id) else {
            return;
        };
        let error = status.error.unwrap_or_else(|| {
            NetError::new(NetErrorCode::Failed, "the load ended without a response")
        });
        tracing::warn!(url = %navigation.url, code = error.code.name(), detail = %error.detail, "navigation failed");
        let document = CommittedDocument::error_page(
            navigation.url.clone(),
            crate::loader::error_page(&error),
        );
        self.commit(frame, navigation_id, navigation.committer, document, None);
    }

    /// Navigation `navigation_id` of `frame`, which has its response and
    /// the start of its body, commits: its load, if it hasn't ended, kept
    /// until it does.
    fn commit_response(&mut self, frame: FrameToken, navigation_id: u64) {
        if let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id)
            && navigation.cancellation_window
        {
            navigation.held = Some(HeldCommit::Response);
            return;
        }
        let Some(navigation) = self.frame_tree.take_navigation(frame, navigation_id) else {
            return;
        };
        let NavigationRequest {
            committer,
            redirects,
            load,
            response,
            end,
            ..
        } = navigation;
        let Some(response) = response else {
            return;
        };
        let NavigationResponse {
            head,
            body,
            start,
            done,
            ..
        } = response;
        let header = |name: &str| {
            head.headers
                .iter()
                .find(|(header, _)| header == name)
                .map(|(_, value)| value.clone())
        };
        let content_language = header("content-language");
        let content_encoding = header("content-encoding");
        let ended = end.is_some();
        let loaded = NavigationLoad {
            status: head.status,
            status_text: head.status_text.clone(),
            encoded_headers_length: head.encoded_headers_length,
            timing: head.timing,
            redirects,
            protocol: head.protocol.clone(),
            content_encoding,
            end,
        };
        let document = CommittedDocument {
            url: head.url,
            mime_type: head.mime_type,
            charset: head.charset,
            content_language,
            body: net::concat(start),
            body_pipe: (!done).then_some(body),
            loaded: Some(loaded),
            error_page: false,
        };
        let completer = self.commit(frame, navigation_id, committer, document, None);
        if !ended {
            self.frame_tree
                .set_document_load(frame, DocumentLoad::new(navigation_id, load, completer));
        }
    }

    /// Navigation `navigation_id` of `frame` to a blob URL has its blob,
    /// whole, or failed (on the UI thread): it commits, unless another
    /// navigation has replaced it, or its frame went.
    pub fn on_navigation_blob_loaded(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        result: LoadResult,
    ) {
        if let Some(navigation) = self.frame_tree.navigation_mut(frame, navigation_id)
            && navigation.cancellation_window
        {
            navigation.held = Some(HeldCommit::Blob(result));
            return;
        }
        let Some(navigation) = self.frame_tree.take_navigation(frame, navigation_id) else {
            return;
        };
        let NavigationRequest {
            committer,
            url,
            redirects,
            ..
        } = navigation;
        let document = match result {
            Ok(response) => {
                let head = response.head;
                let content_language = head
                    .headers
                    .iter()
                    .find(|(name, _)| name == "content-language")
                    .map(|(_, value)| value.clone());
                let loaded = NavigationLoad {
                    status: head.status,
                    status_text: head.status_text.clone(),
                    encoded_headers_length: head.encoded_headers_length,
                    timing: head.timing,
                    redirects,
                    protocol: head.protocol.clone(),
                    content_encoding: None,
                    end: Some(CompletionStatus {
                        error: None,
                        encoded_body_length: response.encoded_body_length,
                        completion_time: head
                            .timing
                            .map_or_else(TimeTicks::now, |timing| timing.response_end),
                    }),
                };
                CommittedDocument {
                    url: head.url,
                    mime_type: head.mime_type,
                    charset: head.charset,
                    content_language,
                    body: response.body.into_bytes(),
                    body_pipe: None,
                    loaded: Some(loaded),
                    error_page: false,
                }
            }
            Err(error) => {
                tracing::warn!(%url, code = error.code.name(), detail = %error.detail, "navigation failed");
                CommittedDocument::error_page(url, crate::loader::error_page(&error))
            }
        };
        self.commit(frame, navigation_id, committer, document, None);
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

    /// Asks the renderer, with `RendererFlags::web_tests`, for a frame once
    /// it has no task and wants no frame, answered with
    /// `RendererHostMsg::IdleFrameReady` (the test harness's, after input).
    pub fn request_idle_frame(&self) {
        self.process.send(RendererMsg::RequestIdleFrame);
    }

    /// Tells the renderer where the current entry is and how many there
    /// are (`SetHistoryOffsetAndLength`), as of the new entries of its own
    /// it has heard of.
    fn send_history_offset_and_length(&self) {
        let (index, length) = self.controller.offset_and_length();
        self.process.send(RendererMsg::SetHistoryOffsetAndLength {
            index,
            length,
            entries_heard: self.entries_heard.peek(),
        });
    }

    /// The renderer's frame `frame` opens its document's `localStorage` or
    /// `sessionStorage` (`kind`; `RenderFrameHostImpl::BindDomStorage` with
    /// the frame's storage key, which the renderer doesn't say, and this
    /// tab's namespace): the area goes to the renderer as the answer
    /// `request_id` it waits for; none for a frame that has gone (its
    /// iframe removed as the message came), which is to have its answer
    /// all the same.
    fn open_storage_area(&self, frame: FrameToken, kind: StorageKind, request_id: u64) {
        let sender = self.process.sender();
        let to_renderer: crate::ToRenderer = Arc::new(move |msg| {
            let _ = sender.send(msg);
        });
        match self.frame_tree.storage_key(frame) {
            Some(key) => self.partition.dom_storage().open(
                self.process.renderer_id(),
                to_renderer,
                kind,
                self.tab,
                key.clone(),
                request_id,
            ),
            None => to_renderer(RendererMsg::StorageAreaOpened {
                request_id,
                area: None,
            }),
        }
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
            RendererHostMsg::DidCommitNavigation {
                frame,
                navigation_id,
                url,
            } => {
                if let Err(reason) = self.frame_tree.did_commit(*frame, *navigation_id, url) {
                    return self.received_bad_message(reason);
                }
                if *frame == self.frame_tree.root() {
                    self.url = Some(url.clone());
                }
                self.controller
                    .did_commit(&self.frame_tree, *frame, *navigation_id, url.clone());
                let place = self.controller.current_place(&self.frame_tree, *frame);
                self.frame_tree.set_history_place(*frame, place);
                // The entries may have changed since the commit was sent
                // (`pushState` as the renderer was busy with script when
                // it came): the renderer counts on from here those of its
                // own it made after the commit.
                self.send_history_offset_and_length();
            }
            RendererHostMsg::DidCommitSameDocumentNavigation {
                frame,
                url,
                replace,
                state,
                navigation_id,
            } => {
                if !replace && navigation_id.is_none() {
                    self.entries_heard.get_next();
                }
                if let Err(reason) = self.frame_tree.did_commit_same_document(*frame, url) {
                    return self.received_bad_message(reason);
                }
                if *frame == self.frame_tree.root() {
                    self.url = Some(url.clone());
                }
                match navigation_id {
                    Some(id) => {
                        self.controller.did_commit_history_same_document(
                            &self.frame_tree,
                            *frame,
                            *id,
                            url.clone(),
                        );
                        // The renderer took the place as the traversal
                        // started; the new entries of its own heard of
                        // since came before it.
                        self.send_history_offset_and_length();
                    }
                    None => self.controller.did_commit_same_document(
                        &self.frame_tree,
                        *frame,
                        url.clone(),
                        *replace,
                        state.clone(),
                    ),
                }
                let place = self.controller.current_place(&self.frame_tree, *frame);
                self.frame_tree.set_history_place(*frame, place);
            }
            RendererHostMsg::SameDocumentNavigationFailed { navigation_id } => {
                let failed = self.controller.did_fail_same_document(*navigation_id);
                let load = failed.and_then(|(frame, index)| {
                    let item = self.controller.item_at(&self.frame_tree, frame, index)?;
                    Some((frame, index, item.url.clone()))
                });
                if let Some((frame, index, url)) = load {
                    self.history_load(frame, index, url, Asked::default());
                }
            }
            RendererHostMsg::GoToEntryAtOffset(offset) => {
                self.go_to_offset(*offset as isize);
            }
            RendererHostMsg::NavigationCancellationWindowEnded { ids } => {
                self.cancellation_windows_ended(ids);
            }
            RendererHostMsg::UpdateState {
                frame,
                manual_scroll_restoration,
            } => {
                if !self.frame_tree.contains(*frame) {
                    return self.received_bad_message(BadMessageReason::StateOfUnknownFrame);
                }
                self.controller.set_manual_scroll_restoration(
                    &self.frame_tree,
                    *frame,
                    *manual_scroll_restoration,
                );
            }
            RendererHostMsg::BeginNavigation {
                frame,
                initiator,
                id,
                kind,
                devtools_initiator,
                history_in_new_child_frame,
                load_flags,
            } => self.begin_renderer_navigation(
                *frame,
                *initiator,
                *id,
                kind,
                devtools_initiator.as_deref(),
                *history_in_new_child_frame,
                *load_flags,
            ),
            RendererHostMsg::OpenUrl { url, disposition } => self.open_url(url, *disposition),
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
            | RendererHostMsg::DedicatedWorkerCreated { .. }
            | RendererHostMsg::DedicatedWorkerGone { .. }
            | RendererHostMsg::RegisterBlobUrl { .. }
            | RendererHostMsg::ChangeStorage { .. }
            | RendererHostMsg::ForgetStorageAreas
            | RendererHostMsg::GetCookies { .. }
            | RendererHostMsg::SetCookie { .. }
            | RendererHostMsg::CookiesEnabled { .. } => {}
            // Revoked for its loads already; the navigations asked for
            // before it, which came before it, have their blob.
            RendererHostMsg::RevokeBlobUrl { url } => {
                self.partition
                    .network()
                    .forget_blob_url(self.process.renderer_id(), url);
            }
            RendererHostMsg::OpenStorageArea {
                frame,
                kind,
                request_id,
            } => {
                self.open_storage_area(*frame, *kind, *request_id);
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
            | RendererHostMsg::IdleFrameReady { .. }
            | RendererHostMsg::WebDriverReply { .. }
            | RendererHostMsg::DevToolsMessage { .. } => {}
            // What was in flight will never be acknowledged; the next
            // renderer starts with nothing in flight.
            RendererHostMsg::RendererGone => {
                self.process.ended();
                self.crashed = true;
                self.input_in_flight = 0;
                self.frame_tree.reset();
            }
            RendererHostMsg::RendererReady => {}
            RendererHostMsg::DidCreateChildFrame {
                parent,
                frame,
                name,
                created_by_script,
            } => {
                if let Err(reason) =
                    self.frame_tree
                        .add_child(*parent, *frame, name, *created_by_script)
                {
                    self.received_bad_message(reason);
                }
            }
            RendererHostMsg::DidChangeSrcDoc { frame, srcdoc } => {
                if let Err(reason) = self.frame_tree.set_srcdoc(*frame, srcdoc) {
                    self.received_bad_message(reason);
                }
            }
            RendererHostMsg::DidDetachFrame { frame } => {
                if let Err(reason) = self.frame_tree.remove(*frame) {
                    self.received_bad_message(reason);
                }
            }
        }
    }

    /// The renderer sent what the browser can't take
    /// (`bad_message::ReceivedBadMessage`): it is ended, as `about:crash`
    /// ends it, the tab's sad tab showing; nothing more from it counts.
    pub fn received_bad_message(&mut self, reason: BadMessageReason) {
        tracing::error!(?reason, "the renderer sent a bad message; ending it");
        self.process.kill();
        self.crashed = true;
        self.input_in_flight = 0;
        self.frame_tree.reset();
    }

    pub fn shutdown(&mut self) {
        // Every frame's navigation stops.
        drop(self.frame_tree.take_any_navigation(self.frame_tree.root()));
        self.frame_tree.reset();
        self.process.shutdown();
        // The tab's `sessionStorage` goes with it.
        self.partition.dom_storage().forget_namespace(self.tab);
        // No renderer is launched again: the messages' way out goes, its
        // last message (`RendererGone`) delivered as the threads ended.
        *self
            .launch
            .on_event
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Box::new(|_, _| {});
    }
}

/// How a navigation of `load_type` uses the HTTP cache, the browser's
/// say, not the renderer's (`CalculateCacheLoadFlags`, for a `GET`: machopo's
/// navigations have no body): a reload validates, a reload bypassing it
/// doesn't read it, back and forward use it stale or not.
fn navigation_load_flags(load_type: FrameLoadType) -> LoadFlags {
    let mut flags = LoadFlags::default();
    match load_type {
        FrameLoadType::Standard => {}
        FrameLoadType::Reload => flags.validate_cache = true,
        FrameLoadType::ReloadBypassingCache => flags.bypass_cache = true,
        FrameLoadType::BackForward => flags.skip_cache_validation = true,
    }
    flags
}
