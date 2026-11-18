use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use base::AtomicSequenceNumber;
use bytes::Bytes;
use crossbeam_channel::Sender;
use net::{LoadHandle, NetworkContext};
use protocol::input::InputEvent;
use protocol::network::{LoadResult, NetError, NetErrorCode, RequestDestination, ResourceRequest};
use protocol::renderer::{
    CompositorMsg, HistoryEntryParams, NavigationLoad, NavigationTimingType, RendererHostMsg,
    RendererMsg,
};
use protocol::viz::{
    BeginFrameArgs, CompositorFrameSinkSender, ContextLostObserver, FrameSinkId,
    SharedImageInterface, VizMsg,
};
use protocol::{FrameToken, TabId};
use renderer::{
    CompositorSender, RendererEndpoints, RendererFlags, RendererSender, ScriptEngineFactory,
};

use crate::bad_message::BadMessageReason;

/// `content::RenderProcessHost`. For now the "process" is a thread in this
/// process; launching a real child process would carry the same messages
/// over IPC and keep this interface.
pub struct RenderProcessHost {
    to_renderer: RendererSender,
    /// The renderer's compositor thread, for Viz's `BeginFrame`s and the
    /// browser's input.
    to_compositor: CompositorSender,
    file_access: Arc<AtomicBool>,
    /// Told by Viz when the GPU is lost, while this is alive; it tells the
    /// renderer.
    _context_lost: Option<Arc<dyn ContextLostObserver>>,
    renderer_thread: Option<JoinHandle<()>>,
    io_thread: Option<JoinHandle<()>>,
    /// Its number, which its blob URLs are registered under.
    renderer_id: u64,
    /// The IO thread passes the renderer's messages on: not once it was
    /// killed, so nothing it still sends is taken for its successor's.
    forwarding: Arc<AtomicBool>,
}

/// Called on a renderer's IO thread with each message it sends the UI
/// thread, by the renderer's id; or with what makes one bad (`Err`).
pub type RendererMessageCallback =
    Box<dyn Fn(TabId, RendererId, Result<RendererHostMsg, BadMessageReason>) + Send>;

/// Which renderer a message came from (Chromium's `ChildProcessHost` id):
/// a tab heeds its own renderer's alone (`WebContents::is_current_renderer`),
/// so what one it replaced or ended still sends counts for nothing, as a
/// gone process's messages reach no one in Chromium.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RendererId(u64);

/// The next renderer's number (Chromium's `ChildProcessHost` id), which
/// its blob URLs are registered under: unique among this browser's.
static RENDERER_IDS: AtomicSequenceNumber<u64> = AtomicSequenceNumber::<u64>::starting_at(1);

/// Passes on Viz's news that the GPU was lost to the renderer's compositor,
/// as a message (Chromium's renderer hears it through its GPU channel): the
/// renderer registers nothing with Viz.
struct ContextLostForwarder(CompositorSender);

impl ContextLostObserver for ContextLostForwarder {
    fn on_context_lost(&self) {
        let _ = self.0.send(CompositorMsg::ContextLost);
    }
}

impl RenderProcessHost {
    /// Launches the renderer, with `flags` and the script engine
    /// `script_engine` makes (which a renderer process of its own would
    /// pick itself). `on_message` runs on the IO thread for every message
    /// the renderer sends, with the renderer's id, except subresource
    /// requests, which the IO thread passes to `network` itself; or for
    /// one the IO thread found bad (`Err`), which the UI thread ends the
    /// renderer for.
    #[allow(clippy::too_many_arguments)]
    pub fn launch(
        tab: TabId,
        frame_sink_id: FrameSinkId,
        flags: RendererFlags,
        script_engine: ScriptEngineFactory,
        viz: Sender<VizMsg>,
        gpu: Option<Arc<dyn SharedImageInterface>>,
        network: NetworkContext,
        on_message: RendererMessageCallback,
    ) -> std::io::Result<Self> {
        let (to_renderer, to_compositor, from_browser) = renderer::channel();
        let (to_browser, from_renderer) = crossbeam_channel::unbounded();
        let file_access = Arc::new(AtomicBool::new(false));
        let context_lost = gpu.as_ref().map(|gpu| {
            let forwarder: Arc<dyn ContextLostObserver> =
                Arc::new(ContextLostForwarder(to_compositor.clone()));
            gpu.add_context_lost_observer(Arc::downgrade(&forwarder));
            forwarder
        });
        let renderer_thread = renderer::spawn_renderer_main_thread(
            RendererEndpoints {
                from_browser,
                to_browser,
                // Bound to the renderer's sink (`FrameSinkManager`): it
                // speaks for no other.
                viz: CompositorFrameSinkSender::new(frame_sink_id, viz),
                gpu,
            },
            flags,
            script_engine,
        )?;
        // `BrowserThread::IO`: where child-process IPC lands before being
        // posted to the UI thread. Subresource requests stop here, as a
        // renderer's `URLLoaderFactory` talks to the network service
        // without going through the UI thread. The renderer is gone once
        // its channel closes, every sender dropped (Chromium's
        // `OnChannelError`), whether it exited or crashed: nothing it sent
        // can come after that.
        let forwarding = Arc::new(AtomicBool::new(true));
        let renderer_id = RENDERER_IDS.get_next();
        let io_thread = {
            let to_renderer = to_renderer.clone();
            let file_access = file_access.clone();
            let forwarding = forwarding.clone();
            let forward = move |tab, msg: Result<RendererHostMsg, BadMessageReason>| {
                if forwarding.load(Ordering::Acquire) {
                    on_message(tab, RendererId(renderer_id), msg);
                }
            };
            std::thread::Builder::new()
                .name("Chrome_IOThread".into())
                .spawn(move || {
                    let on_message = |tab, msg| forward(tab, Ok(msg));
                    // The renderer's loads still loading.
                    let loads = Loads::default();
                    // Its dedicated workers, which may load (Chromium's
                    // `DedicatedWorkerHost`s, each a `URLLoaderFactory`),
                    // and the blob URLs each registered (its
                    // `BlobURLStore`'s), which go with it.
                    let mut workers = HashSet::new();
                    let mut worker_blob_urls: HashMap<u64, Vec<String>> = HashMap::new();
                    while let Ok(msg) = from_renderer.recv() {
                        let fetch = |load, request| {
                            fetch_for_renderer(
                                &network,
                                renderer_id,
                                &file_access,
                                &to_renderer,
                                &loads,
                                load,
                                request,
                            );
                        };
                        match msg {
                            RendererHostMsg::FetchResource {
                                request_id,
                                worker,
                                request,
                            } => {
                                let load = LoadKey::Resource { worker, request_id };
                                // The browser loads every frame's document,
                                // as a navigation: a renderer asking for one
                                // is bad (the network service's "frame
                                // destination requires kNavigate mode").
                                if matches!(
                                    request.destination,
                                    RequestDestination::Document | RequestDestination::Iframe
                                ) {
                                    let _ = to_renderer.send(load.answer(Err(NetError::new(
                                        NetErrorCode::AccessDenied,
                                        request.url,
                                    ))));
                                    forward(tab, Err(BadMessageReason::DocumentRequest));
                                } else if worker.is_some_and(|worker| !workers.contains(&worker)) {
                                    // One it didn't make, or gone.
                                    let _ = to_renderer.send(load.answer(Err(NetError::new(
                                        NetErrorCode::AccessDenied,
                                        request.url,
                                    ))));
                                } else {
                                    fetch(load, request);
                                }
                            }
                            RendererHostMsg::DedicatedWorkerCreated { worker, request } => {
                                if workers.insert(worker) {
                                    fetch(LoadKey::WorkerScript(worker), request);
                                }
                            }
                            // Its loads close with it.
                            RendererHostMsg::DedicatedWorkerGone { worker } => {
                                workers.remove(&worker);
                                // Revoked, as a revoke of each: for loads
                                // here, for navigations on the UI thread.
                                for url in worker_blob_urls.remove(&worker).unwrap_or_default() {
                                    network.revoke_blob_url(renderer_id, &url);
                                    on_message(tab, RendererHostMsg::RevokeBlobUrl { url });
                                }
                                let mut loads = loads.lock().expect("the loads");
                                let ended: Vec<LoadKey> = loads
                                    .keys()
                                    .copied()
                                    .filter(|load| load.worker() == Some(worker))
                                    .collect();
                                for load in ended {
                                    if let Some(load) = loads.remove(&load) {
                                        load.cancel();
                                    }
                                }
                            }
                            // Here, before any load that comes after it on
                            // the channel (Chromium's `BlobURLStore` is a
                            // pipe of its own, its messages kept in order
                            // with the loads by an associated interface).
                            RendererHostMsg::RegisterBlobUrl {
                                worker,
                                url,
                                mime_type,
                                bytes,
                            } => match worker {
                                None => {
                                    network.register_blob_url(renderer_id, &url, mime_type, bytes);
                                }
                                // One it didn't make, or gone: nothing.
                                Some(worker) if workers.contains(&worker) => {
                                    network.register_blob_url(renderer_id, &url, mime_type, bytes);
                                    worker_blob_urls.entry(worker).or_default().push(url);
                                }
                                Some(_) => {}
                            },
                            // For loads here; the UI thread forgets it after
                            // the navigations asked for before.
                            RendererHostMsg::RevokeBlobUrl { url } => {
                                for urls in worker_blob_urls.values_mut() {
                                    urls.retain(|registered| *registered != url);
                                }
                                network.revoke_blob_url(renderer_id, &url);
                                on_message(tab, RendererHostMsg::RevokeBlobUrl { url });
                            }
                            RendererHostMsg::FollowRedirect { request_id, worker } => {
                                if let Some(load) = loads
                                    .lock()
                                    .expect("the loads")
                                    .get(&LoadKey::Resource { worker, request_id })
                                {
                                    load.follow_redirect();
                                }
                            }
                            RendererHostMsg::CancelResource { request_id, worker } => {
                                let load = loads
                                    .lock()
                                    .expect("the loads")
                                    .remove(&LoadKey::Resource { worker, request_id });
                                if let Some(load) = load {
                                    load.cancel();
                                }
                            }
                            msg => on_message(tab, msg),
                        }
                    }
                    // Its loads go with it (its `URLLoader`s close), and
                    // its blob URLs.
                    for (_, load) in loads.lock().expect("the loads").drain() {
                        load.cancel();
                    }
                    network.revoke_blob_urls_of(renderer_id);
                    on_message(tab, RendererHostMsg::RendererGone);
                })?
        };
        Ok(Self {
            to_renderer,
            to_compositor,
            file_access,
            _context_lost: context_lost,
            renderer_thread: Some(renderer_thread),
            io_thread: Some(io_thread),
            renderer_id,
            forwarding,
        })
    }

    /// Its number, which its blob URLs are registered under.
    pub(crate) fn renderer_id(&self) -> u64 {
        self.renderer_id
    }

    /// Which renderer it is, as its messages say.
    pub(crate) fn id(&self) -> RendererId {
        RendererId(self.renderer_id)
    }

    /// Ends the renderer at once, as `chrome://kill` does, and passes on
    /// nothing more from it, its `RendererGone` included: the caller knows
    /// it has gone.
    pub fn kill(&self) {
        self.forwarding.store(false, Ordering::Release);
        self.send(RendererMsg::Shutdown);
    }

    /// Lets the renderer's threads end on their own: a renderer that has
    /// gone, replaced by another. Waiting for one that hangs would hang the
    /// browser.
    pub fn abandon(mut self) {
        self.forwarding.store(false, Ordering::Release);
        self.renderer_thread.take();
        self.io_thread.take();
    }

    pub fn send(&self, msg: RendererMsg) {
        if self.to_renderer.send(msg).is_err() {
            tracing::warn!("renderer is gone; dropping message");
        }
    }

    /// `WidgetInputHandler::DispatchEvent`: input goes to the renderer's
    /// compositor thread, which passes on what it doesn't handle.
    pub fn send_input_event(&self, event: InputEvent) {
        if self
            .to_compositor
            .send(CompositorMsg::HandleInputEvent(event))
            .is_err()
        {
            tracing::warn!("renderer is gone; dropping input");
        }
    }

    /// Delivers `BeginFrame`s to the renderer: for Viz, which sends them
    /// to the renderer's compositor thread, as Chromium's
    /// `CompositorFrameSinkClient` pipe is bound there, so a busy main
    /// thread doesn't hold them up.
    pub fn begin_frame_sender(&self) -> impl Fn(BeginFrameArgs) + Send + Sync + 'static {
        let to_compositor = self.to_compositor.clone();
        move |args| {
            let _ = to_compositor.send(CompositorMsg::BeginFrame(args));
        }
    }

    /// Whether the renderer may ask for `url`, to load or to navigate a new
    /// tab to (`ChildProcessSecurityPolicy::CanRequestURL`): a local file
    /// only while one of its frames shows one.
    pub(crate) fn can_request(&self, url: &str) -> bool {
        !is_file_url(url) || self.file_access.load(Ordering::Relaxed)
    }

    /// Whether the renderer may read local files: while it shows one
    /// (`ChildProcessSecurityPolicy::GrantRequestScheme`), as Chromium
    /// allows `file:` subresources only from `file:` documents.
    pub(crate) fn set_file_access(&self, may: bool) {
        self.file_access.store(may, Ordering::Relaxed);
    }

    /// What commits navigation `frame`'s document of a navigation (the
    /// renderer's `begin_id`, if it asked for it), to an entry with
    /// `history`.
    pub(crate) fn committer(
        &self,
        frame: FrameToken,
        begin_id: Option<u64>,
        history: HistoryEntryParams,
        navigation_start: ipc::TimeTicks,
        navigation_type: NavigationTimingType,
    ) -> Committer {
        Committer {
            to_renderer: self.to_renderer.clone(),
            frame,
            begin_id,
            history,
            navigation_start,
            navigation_type,
        }
    }

    /// Asks the renderer to exit and waits for it.
    pub fn shutdown(&mut self) {
        let _ = self.to_renderer.send(RendererMsg::Shutdown);
        for handle in [self.renderer_thread.take(), self.io_thread.take()]
            .into_iter()
            .flatten()
        {
            if handle.join().is_err() {
                tracing::error!("renderer-side thread panicked");
            }
        }
    }
}

impl Drop for RenderProcessHost {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Commits a frame's navigation.
pub(crate) struct Committer {
    to_renderer: RendererSender,
    frame: FrameToken,
    begin_id: Option<u64>,
    history: HistoryEntryParams,
    /// When the navigation started (`NavigationRequest`'s
    /// `navigation_start`).
    navigation_start: ipc::TimeTicks,
    navigation_type: NavigationTimingType,
}

/// A navigation's document as it commits.
pub(crate) struct CommittedDocument {
    pub(crate) url: String,
    pub(crate) mime_type: String,
    pub(crate) charset: Option<String>,
    pub(crate) content_language: Option<String>,
    pub(crate) body: Bytes,
    /// What its load took; none for a page of the browser's own or an
    /// error page.
    pub(crate) loaded: Option<NavigationLoad>,
    /// The browser's error page for `url`.
    pub(crate) error_page: bool,
}

impl CommittedDocument {
    /// A page the browser made (`about:`): HTML, no load.
    pub(crate) fn page(url: String, markup: String) -> Self {
        Self {
            url,
            mime_type: "text/html".into(),
            charset: None,
            content_language: None,
            body: markup.into(),
            loaded: None,
            error_page: false,
        }
    }

    /// The error page for `url`, which failed to load.
    pub(crate) fn error_page(url: String, markup: String) -> Self {
        Self {
            error_page: true,
            ..Self::page(url, markup)
        }
    }

    /// Its origin, as its URL's (an error page's, opaque), and whether it
    /// may open local files: a local file (not an error page for one).
    pub(crate) fn origin(&self) -> crate::frame_tree::CommittedOrigin {
        if self.error_page {
            return crate::frame_tree::CommittedOrigin::opaque();
        }
        crate::frame_tree::CommittedOrigin {
            origin: url::Url::parse(&self.url).map_or_else(
                |_| crate::frame_tree::CommittedOrigin::opaque().origin,
                |url| url.origin(),
            ),
            local: is_file_url(&self.url),
        }
    }
}

impl Committer {
    /// Where its entry is in the session history as it commits, and how
    /// many entries there are then (Chromium's
    /// `UpdateHistoryParamsInCommitNavigationParams`, as `CommitNavigation`
    /// is sent): what changed since it started counts.
    pub(crate) fn set_history_place(&mut self, (index, length): (u32, u32)) {
        self.history.index = index;
        self.history.length = length;
    }

    pub(crate) fn commit(self, navigation_id: u64, document: CommittedDocument) {
        let _ = self.to_renderer.send(RendererMsg::CommitNavigation {
            frame: self.frame,
            navigation_id,
            begin_id: self.begin_id,
            url: document.url,
            error_page: document.error_page,
            mime_type: document.mime_type,
            charset: document.charset,
            content_language: document.content_language,
            body: document.body,
            history: self.history,
            navigation_start: self.navigation_start,
            loaded: document.loaded.map(Box::new),
            navigation_type: self.navigation_type,
        });
    }
}

/// A load of a renderer's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum LoadKey {
    /// A `FetchResource` of its main thread's or of its dedicated worker
    /// `worker`'s, by its request id (unique to whichever asked).
    Resource {
        worker: Option<u64>,
        request_id: u64,
    },
    /// The main script of its dedicated worker (`DedicatedWorkerCreated`),
    /// its redirects followed here (Chromium's `WorkerScriptFetcher`).
    WorkerScript(u64),
}

impl LoadKey {
    /// The dedicated worker whose it is.
    fn worker(self) -> Option<u64> {
        match self {
            Self::Resource { worker, .. } => worker,
            Self::WorkerScript(worker) => Some(worker),
        }
    }

    /// What tells the renderer it is over.
    fn answer(self, result: LoadResult) -> RendererMsg {
        match self {
            Self::Resource { worker, request_id } => RendererMsg::ResourceResponse {
                request_id,
                worker,
                result,
            },
            Self::WorkerScript(worker) => RendererMsg::DedicatedWorkerScript { worker, result },
        }
    }
}

/// A renderer's loads in flight, which the IO thread and the
/// `NetworkService` thread (taking out one that is over) share.
type Loads = Arc<Mutex<HashMap<LoadKey, LoadHandle>>>;

#[allow(clippy::too_many_arguments)]
fn fetch_for_renderer(
    network: &NetworkContext,
    renderer_id: u64,
    file_access: &AtomicBool,
    to_renderer: &RendererSender,
    loads: &Loads,
    load: LoadKey,
    request: ResourceRequest,
) {
    if is_file_url(&request.url) && !file_access.load(Ordering::Relaxed) {
        tracing::warn!(url = %request.url, "renderer may not read local files");
        let _ = to_renderer
            .send(load.answer(Err(NetError::new(NetErrorCode::AccessDenied, request.url))));
        return;
    }
    // A blob URL's blob, as it is now: answered at once, so a revoke after
    // the request doesn't reach it.
    if is_blob_url(&request.url) {
        let result = network.load_blob_url(&request, Some(renderer_id));
        let _ = to_renderer.send(load.answer(result));
        return;
    }
    // Held while the load starts, so one that is over at once finds its
    // entry to take out.
    let mut held = loads.lock().expect("the loads");
    let over = loads.clone();
    let answered = to_renderer.clone();
    let done = move |result| {
        over.lock().expect("the loads").remove(&load);
        let _ = answered.send(load.answer(result));
    };
    let handle = match load {
        LoadKey::Resource { worker, request_id } => {
            let redirected = to_renderer.clone();
            network.fetch_with_redirects(
                request,
                move |redirect| {
                    let _ = redirected.send(RendererMsg::ReceivedRedirect {
                        request_id,
                        worker,
                        redirect: Box::new(redirect),
                    });
                },
                done,
            )
        }
        LoadKey::WorkerScript(_) => network.fetch(request, done),
    };
    held.insert(load, handle);
}

pub(crate) fn is_file_url(url: &str) -> bool {
    url.get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("file:"))
}

pub(crate) fn is_blob_url(url: &str) -> bool {
    url.get(..5)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("blob:"))
}
