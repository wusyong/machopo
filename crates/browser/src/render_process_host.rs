use std::collections::HashMap;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread::JoinHandle;

use base::AtomicSequenceNumber;
use bytes::Bytes;
use crossbeam_channel::Sender;
use ipc::core::channel::Gone;
use ipc::core::frame::BadMessage;
use ipc::endpoint::{self, Handler, IoLoop, LoopHandle, Received};
use ipc::platform::Channel;
use net::{LoadHandle, NetworkContext};
use protocol::control_channel::{self, ChannelLink, send_over, to_browser, to_renderer};
use protocol::input::InputEvent;
use protocol::network::{
    LoadResult, NetError, NetErrorCode, RequestDestination, ResourceRequest, TrustedParams,
};
use protocol::renderer::{
    CompositorMsg, FrameLoadType, HistoryEntryParams, NavigationLoad, RendererHostMsg, RendererMsg,
};
use protocol::viz::{
    BeginFrameArgs, CompositorFrameSinkMsg, CompositorFrameSinkSender, ContextLostObserver,
    ContextProvider, FrameSinkId, VizMsg,
};
use protocol::{FrameToken, TabId};
use renderer::{
    ChildInit, CompositorSender, RendererEndpoints, RendererFlags, RendererSender,
    ScriptEngineFactory,
};

use crate::StoragePartition;
use crate::bad_message::BadMessageReason;
use crate::font_data_service::{FontDataService, FontRequest, SERVES_SYSTEM_FONTS};

/// A renderer process to start: its command, and what its starter gives
/// it beside the command where the operating system has the starter give
/// it (Windows): the sandbox it is started in, and files it is to write
/// and couldn't open inside one (each told to it on the command, by its
/// handle's value). Elsewhere a renderer enters its own sandbox and opens
/// its own files before, and both are left empty.
pub struct RendererProcess {
    pub command: Command,
    pub sandbox: Option<ipc::platform::Sandbox>,
    pub files: Vec<std::fs::File>,
    /// The process has only the variables `command` sets, none of the
    /// browser's: `command`'s `env_clear`, which the standard library
    /// doesn't let be read back, and the process isn't started by it on
    /// Windows. Elsewhere `env_clear` is followed as it is.
    #[cfg(windows)]
    pub clear_environment: bool,
    /// Called with the process's id once it has started.
    pub started: Option<Box<dyn FnOnce(u32) + Send>>,
}

impl RendererProcess {
    /// `command`, with no sandbox of its starter's and no files.
    pub fn new(command: Command) -> Self {
        Self {
            command,
            sandbox: None,
            files: Vec::new(),
            #[cfg(windows)]
            clear_environment: false,
            started: None,
        }
    }
}

/// How a renderer is run (Chromium's `--single-process`, or not).
#[derive(Clone)]
pub enum RendererLauncher {
    /// As threads of this process, with the script engines given: its
    /// messages are moved, not serialized.
    InProcess(ScriptEngineFactory),
    /// As a process of its own. The function makes the command that
    /// starts one: the embedder's executable, told to be a renderer (it
    /// takes its end with `ipc::platform::inherited` and runs
    /// `renderer::run_child_process`, with the engines it picks itself).
    Process(Arc<dyn Fn() -> RendererProcess + Send + Sync>),
    /// With nothing at the other end, for tests and fuzzing: what the
    /// browser sends the renderer waits in queues nobody reads, and the
    /// renderer's messages are whatever the caller hands in
    /// (`WebContents::deliver_from_renderer`), each handled before the
    /// call returns, by the same object that handles a real renderer's.
    Detached,
}

impl std::fmt::Debug for RendererLauncher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InProcess(_) => "RendererLauncher::InProcess",
            Self::Process(_) => "RendererLauncher::Process",
            Self::Detached => "RendererLauncher::Detached",
        })
    }
}

/// The browser's IO thread for its renderer processes (`BrowserThread::IO`):
/// one for the process, which waits on every renderer's channel, started
/// with the first renderer process and never stopped. `None` when the
/// system gives no way to wait, or no thread.
fn io_thread() -> Option<&'static LoopHandle<Channel>> {
    static IO: OnceLock<Option<LoopHandle<Channel>>> = OnceLock::new();
    IO.get_or_init(|| {
        let (io, handle) = IoLoop::new()?;
        std::thread::Builder::new()
            .name("Chrome_IOThread".into())
            .spawn(move || io.run())
            .ok()?;
        Some(handle)
    })
    .as_ref()
}

/// The IO thread's side of a renderer process's channel: what it sends
/// the browser goes to the object that checks it, and what it sends Viz
/// goes there as from the sink the browser bound the renderer to,
/// whatever the renderer would say.
struct ChannelHost {
    /// None once the channel is over.
    host: Option<IoThreadHost>,
    frame_sink_id: FrameSinkId,
    viz: Sender<VizMsg>,
}

impl Handler<Channel> for ChannelHost {
    fn message(&mut self, mut message: Received<'_, Channel>) -> Result<(), BadMessage> {
        // Every renderer's messages wait behind this one: what it costs
        // (its decoding, and whatever of it is copied) is what the
        // browser's IO thread is busy for.
        let _span = trace_event::trace_event!("ChannelHost::OnMessage");
        match message.destination() {
            to_browser::HOST => {
                let msg = message.decode::<RendererHostMsg>()?;
                if let Some(host) = &mut self.host {
                    host.on_message(msg);
                }
            }
            to_browser::FRAME_SINK => {
                let msg = message.decode::<CompositorFrameSinkMsg>()?;
                // Checked here, as it is read, as Chromium's mojo traits
                // check a frame: what can't be right is the renderer's bad
                // message, and Viz never has it. A renderer's compositor
                // embeds no other's surface.
                if let Err(bad) = msg.check(false) {
                    tracing::warn!(?bad, "a renderer's message to Viz can't be right");
                    if let Some(host) = &self.host {
                        (host.forward)(Err(BadMessageReason::FrameSink(bad)));
                    }
                    return Ok(());
                }
                // Viz gone: the browser is ending.
                let _ = self.viz.send(VizMsg::FrameSink(self.frame_sink_id, msg));
            }
            _ => return Err(BadMessage::Rejected),
        }
        Ok(())
    }

    fn gone(&mut self, gone: Gone) {
        tracing::debug!(?gone, "a renderer's channel is over");
        if let Some(host) = self.host.take() {
            // Over for what the renderer sent: a bad message as any
            // other, which the tab ends the renderer for (it may well be
            // running still).
            if matches!(gone, Gone::BadMessage(_) | Gone::QueueFull) {
                (host.forward)(Err(BadMessageReason::Channel));
            }
            host.on_channel_closed();
        }
    }
}

/// `content::RenderProcessHost`: a renderer, in this process (its threads)
/// or in one of its own (a child process and the channel to it). The
/// messages and the checks are the same either way.
pub struct RenderProcessHost {
    to_renderer: RendererSender,
    /// The renderer's compositor thread, for Viz's `BeginFrame`s and the
    /// browser's input.
    to_compositor: CompositorSender,
    file_access: Arc<AtomicBool>,
    /// Told by Viz when the GPU is lost, while this is alive; it tells the
    /// renderer.
    _context_lost: Option<Arc<dyn ContextLostObserver>>,
    /// A renderer in this process: its main thread, and the thread that
    /// takes its messages.
    renderer_thread: Option<JoinHandle<()>>,
    io_thread: Option<JoinHandle<()>>,
    /// What stops its main thread's script, as machopo ends: a thread
    /// can't be killed.
    terminator: Option<renderer::MainThreadTerminator>,
    /// A renderer in a process of its own: the process, and this end of
    /// its channel.
    child: Option<(ipc::platform::Child, ChannelLink)>,
    /// How the process ended, once it has.
    exit: Option<std::process::ExitStatus>,
    /// A renderer with nothing at the other end: what handles the messages
    /// handed in for it, and the queues the browser's messages wait in.
    detached: Option<(IoThreadHost, renderer::RendererReceiver)>,
    /// Its number, which its blob URLs are registered under.
    renderer_id: u64,
    /// The IO thread passes the renderer's messages on: not once it was
    /// killed, so nothing it still sends is taken for its successor's.
    forwarding: Arc<AtomicBool>,
    /// The loaders the browser gave the renderer's documents, which the IO
    /// thread checks their requests by (`register_loader`).
    loaders: Arc<Mutex<Loaders>>,
}

/// The params of a loader the browser gives a document as it commits
/// (Chromium's `URLLoaderFactoryParams`): the origin its requests'
/// initiator is to be (`request_initiator_origin_lock`), and what its
/// requests' cookies go by (`IsolationInfo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LoaderParams {
    pub(crate) origin: url::Origin,
    pub(crate) trusted: TrustedParams,
}

/// A renderer's loaders, by number, from 1. Kept while the renderer is:
/// a document gone may have requests on their way still (Chromium's
/// factory lives until the renderer lets its pipe go).
#[derive(Default)]
pub(crate) struct Loaders {
    params: HashMap<u64, LoaderParams>,
    last: u64,
}

/// What the IO thread knows of a dedicated worker of the renderer's: the
/// loader it was made with, its creator's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Worker {
    loader: u64,
}

/// A request's trusted params, its loader's, if the renderer may send it
/// through `loader` (worker `worker`'s, if a worker's): of a loader it
/// was given, the worker's for a worker's, its initiator none, opaque (an
/// opaque origin of the loader's, as Chromium lets its precursor) or the
/// loader's origin, with no trusted params of its own. `Ok(None)`: a
/// worker the renderer has no more (gone as the request came), refused
/// without blame.
fn check_request(
    loaders: &Loaders,
    workers: &HashMap<u64, Worker>,
    worker: Option<u64>,
    loader: u64,
    request: &ResourceRequest,
) -> Result<Option<TrustedParams>, BadMessageReason> {
    if request.trusted.is_some() {
        return Err(BadMessageReason::TrustedParamsFromRenderer);
    }
    let params = loaders
        .params
        .get(&loader)
        .ok_or(BadMessageReason::UnknownLoader)?;
    if let Some(worker) = worker {
        let Some(known) = workers.get(&worker) else {
            return Ok(None);
        };
        if known.loader != loader {
            return Err(BadMessageReason::WorkerLoaderMismatch);
        }
    }
    let lock = match &params.origin {
        origin @ url::Origin::Tuple(..) => origin.ascii_serialization(),
        url::Origin::Opaque(_) => "null".to_owned(),
    };
    match request.initiator.as_deref() {
        None | Some("null") => {}
        Some(initiator) if initiator == lock => {}
        Some(_) => return Err(BadMessageReason::InitiatorMismatch),
    }
    Ok(Some(params.trusted.clone()))
}

/// Where script's cookies of `url` (a document's cookie URL) are, through
/// `loader` (`RestrictedCookieManager::ValidateAccessToCookiesAt`, with the
/// loader as the frame's bound origin and context): the URL and the
/// loader's trusted params, if the renderer was given the loader, its
/// origin isn't opaque, and `url` is of it.
fn check_cookie_access(
    loaders: &Loaders,
    loader: u64,
    url: &str,
) -> Result<(url::Url, TrustedParams), BadMessageReason> {
    let params = loaders
        .params
        .get(&loader)
        .ok_or(BadMessageReason::UnknownLoader)?;
    if !params.origin.is_tuple() {
        return Err(BadMessageReason::CookiesOfOpaqueOrigin);
    }
    let url = url::Url::parse(url).map_err(|_| BadMessageReason::CookieUrlMismatch)?;
    if url.origin() != params.origin {
        return Err(BadMessageReason::CookieUrlMismatch);
    }
    Ok((url, params.trusted.clone()))
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
    /// Launches the renderer as `launcher` says, with `flags`.
    /// `on_message` runs on the IO thread for every message
    /// the renderer sends, with the renderer's id, except subresource
    /// requests, which the IO thread passes to `network` itself; or for
    /// one the IO thread found bad (`Err`), which the UI thread ends the
    /// renderer for.
    #[allow(clippy::too_many_arguments)]
    pub fn launch(
        tab: TabId,
        frame_sink_id: FrameSinkId,
        flags: RendererFlags,
        launcher: &RendererLauncher,
        viz: Sender<VizMsg>,
        gpu: Option<Arc<dyn ContextProvider>>,
        partition: StoragePartition,
        on_message: RendererMessageCallback,
    ) -> std::io::Result<Self> {
        let network = partition.network().clone();
        let dom_storage = partition.dom_storage().clone();
        let file_access = Arc::new(AtomicBool::new(false));
        let forwarding = Arc::new(AtomicBool::new(true));
        let renderer_id = RENDERER_IDS.get_next();
        let loaders = Arc::new(Mutex::new(Loaders::default()));
        // The system's fonts, which the renderer's text asks the browser
        // for, where it does: none of it finds them itself.
        let fonts = (SERVES_SYSTEM_FONTS && !flags.test_fonts)
            .then(FontDataService::get)
            .flatten();
        // `BrowserThread::IO`: where a renderer's messages land before
        // being posted to the UI thread. Subresource requests stop here,
        // as a renderer's `URLLoaderFactory` talks to the network service
        // without going through the UI thread. The renderer is gone once
        // its channel is over (Chromium's `OnChannelError`), whether it
        // exited or crashed: nothing it sent can come after that.
        let host = |to_renderer: RendererSender| {
            let forwarding = forwarding.clone();
            IoThreadHost {
                renderer_id,
                network,
                dom_storage,
                to_renderer,
                fonts,
                file_access: file_access.clone(),
                loaders: loaders.clone(),
                forward: Box::new(move |msg| {
                    if forwarding.load(Ordering::Acquire) {
                        on_message(tab, RendererId(renderer_id), msg);
                    }
                }),
                loads: Loads::default(),
                workers: HashMap::new(),
                worker_blob_urls: HashMap::new(),
            }
        };
        let mut detached = None;
        let (to_renderer, to_compositor, renderer_thread, io_thread, terminator, child) =
            match launcher {
                RendererLauncher::InProcess(script_engine) => {
                    let (to_renderer, to_compositor, from_browser) = renderer::channel();
                    let (to_browser, from_renderer) = crossbeam_channel::unbounded();
                    let terminator = renderer::MainThreadTerminator::default();
                    let renderer_thread = renderer::spawn_renderer_main_thread(
                        RendererEndpoints {
                            from_browser,
                            to_browser: to_browser.into(),
                            // Bound to the renderer's sink (`FrameSinkManager`):
                            // it speaks for no other.
                            viz: CompositorFrameSinkSender::new(frame_sink_id, viz),
                            gpu: gpu.is_some(),
                        },
                        flags,
                        *script_engine,
                        Some(terminator.clone()),
                        match fonts {
                            Some(fonts) => {
                                renderer::SystemFontSource::Browser(fonts.names().clone())
                            }
                            // Found by the renderer's main thread: a thread
                            // of this process has the file system.
                            None => renderer::SystemFontSource::FindHere,
                        },
                    )?;
                    // The channel closes once every sender is dropped.
                    let mut host = host(to_renderer.clone());
                    let io_thread = std::thread::Builder::new()
                        .name("Chrome_IOThread".into())
                        .spawn(move || {
                            while let Ok(msg) = from_renderer.recv() {
                                host.on_message(msg);
                            }
                            host.on_channel_closed();
                        })?;
                    (
                        to_renderer,
                        to_compositor,
                        Some(renderer_thread),
                        Some(io_thread),
                        Some(terminator),
                        None,
                    )
                }
                RendererLauncher::Process(command) => {
                    let io = io_thread()
                        .ok_or_else(|| std::io::Error::other("the browser has no IO thread"))?;
                    let mut process = command();
                    #[cfg(windows)]
                    let (child, channel) = ipc::platform::spawn_handing(
                        &mut process.command,
                        process.sandbox.as_ref(),
                        &process.files,
                        process.clear_environment,
                    )?;
                    #[cfg(not(windows))]
                    let (child, channel) =
                        ipc::platform::spawn_in(&mut process.command, process.sandbox.as_ref())?;
                    // The child has its own; these were the browser's.
                    drop(std::mem::take(&mut process.files));
                    if let Some(started) = process.started.take() {
                        started(child.id());
                    }
                    let (link, endpoint) =
                        endpoint::endpoint(channel, control_channel::config(true));
                    // What the renderer starts with, before anything else is
                    // sent it: its own first message.
                    let init = ChildInit {
                        flags,
                        gpu: gpu.is_some(),
                        fonts: fonts.map(|fonts| fonts.names().clone()),
                    };
                    let _ = send_over(&link, to_renderer::MAIN, &init);
                    // Shared memory for the tiles it draws, so their pixels
                    // cross without being copied: enough for a frame's worth
                    // of new tiles, and it asks for more when it has none
                    // free.
                    if gpu.is_some() {
                        for _ in 0..TILE_REGIONS {
                            link.grant_to_peer(TILE_REGION_LEN);
                        }
                    }
                    let to_renderer = RendererSender::over(link.clone());
                    io.serve(
                        endpoint,
                        ChannelHost {
                            host: Some(host(to_renderer.clone())),
                            frame_sink_id,
                            viz,
                        },
                    );
                    let to_compositor = CompositorSender::over(link.clone());
                    (
                        to_renderer,
                        to_compositor,
                        None,
                        None,
                        None,
                        Some((child, link)),
                    )
                }
                RendererLauncher::Detached => {
                    let (to_renderer, to_compositor, from_browser) = renderer::channel();
                    detached = Some((host(to_renderer.clone()), from_browser));
                    (to_renderer, to_compositor, None, None, None, None)
                }
            };
        let context_lost = gpu.as_ref().map(|gpu| {
            let forwarder: Arc<dyn ContextLostObserver> =
                Arc::new(ContextLostForwarder(to_compositor.clone()));
            gpu.add_context_lost_observer(Arc::downgrade(&forwarder));
            forwarder
        });
        Ok(Self {
            to_renderer,
            to_compositor,
            file_access,
            _context_lost: context_lost,
            renderer_thread,
            io_thread,
            terminator,
            child,
            exit: None,
            detached,
            renderer_id,
            forwarding,
            loaders,
        })
    }

    /// A loader for a document about to commit, with `params`: its number,
    /// which the commit carries (`CommitNavigation`'s `loader`).
    pub(crate) fn register_loader(&self, params: LoaderParams) -> u64 {
        let mut loaders = self.loaders.lock().unwrap_or_else(PoisonError::into_inner);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one a document committed: a u64 lasts centuries"
        )]
        {
            loaders.last += 1;
        }
        let id = loaders.last;
        loaders.params.insert(id, params);
        id
    }

    /// Its number, which its blob URLs are registered under.
    pub(crate) fn renderer_id(&self) -> u64 {
        self.renderer_id
    }

    /// Its way to the renderer's main thread, for what answers it from
    /// another thread.
    pub(crate) fn sender(&self) -> RendererSender {
        self.to_renderer.clone()
    }

    /// A message from a renderer with nothing at the other end
    /// ([`RendererLauncher::Detached`]), handled as one off a channel is,
    /// before this returns. Nothing, for any other renderer.
    pub(crate) fn deliver(&mut self, msg: RendererHostMsg) {
        if let Some((host, _)) = &mut self.detached {
            host.on_message(msg);
        }
    }

    /// The channel of a renderer with nothing at the other end is over,
    /// as when a renderer goes: what it had is let go, and the tab is
    /// told (`RendererGone`).
    pub(crate) fn deliver_gone(&mut self) {
        if let Some((host, _)) = self.detached.take() {
            host.on_channel_closed();
        }
    }

    /// The renderer's process, if it has one of its own.
    pub(crate) fn pid(&self) -> Option<u32> {
        self.child.as_ref().map(|(child, _)| child.id())
    }

    /// The shared memory this end keeps for what it sends a renderer
    /// process (the regions its large data goes by): their bytes, and how
    /// many of them are free. None for a renderer in this process.
    pub(crate) fn shared_memory(&self) -> Option<(usize, usize)> {
        self.child.as_ref().map(|(_, link)| link.regions())
    }

    /// Which renderer it is, as its messages say.
    pub(crate) fn id(&self) -> RendererId {
        RendererId(self.renderer_id)
    }

    /// Ends the renderer at once, as `chrome://kill` does, and passes on
    /// nothing more from it, its `RendererGone` included: the caller knows
    /// it has gone. A renderer's threads are asked to end, and do when
    /// they get to it; a renderer process isn't asked: it is killed, and
    /// is no more when this returns.
    pub fn kill(&mut self) {
        self.forwarding.store(false, Ordering::Release);
        match self.child.take() {
            Some((child, link)) => self.exit = end_child(child, &link, None).status,
            None => self.send(RendererMsg::Shutdown),
        }
    }

    /// The renderer's channel is over (`RendererGone`): its process, if it
    /// has one of its own, is seen to its end. One that crashed has
    /// exited, or is about to; one whose channel went for another reason
    /// (it closed it, the channel refused what it sent) may be running,
    /// and is killed.
    pub(crate) fn ended(&mut self) {
        if let Some((child, link)) = self.child.take() {
            self.exit = end_child(child, &link, Some(GONE_EXIT_WAIT)).status;
        }
    }

    /// Has the renderer crash (`chrome://crash`), if it is a process of its
    /// own: `false` for one that is this process's threads, which can't.
    /// The process ends itself, and is no more when this returns, so
    /// whatever the tab is asked next finds a renderer that is gone, as
    /// after a kill (a real crash comes whenever it does; this one is
    /// asked for). One that doesn't crash in time is killed. Nothing more
    /// from it is passed on.
    pub(crate) fn crash(&mut self) -> bool {
        let Some((child, link)) = self.child.take() else {
            return false;
        };
        self.forwarding.store(false, Ordering::Release);
        self.send(RendererMsg::Crash);
        self.exit = end_child(child, &link, Some(CRASH_WAIT)).status;
        true
    }

    /// How the renderer's process ended, if it has (Chromium's
    /// `ChildProcessTerminationInfo`): what the system says of it.
    pub(crate) fn exit(&self) -> Option<std::process::ExitStatus> {
        self.exit
    }

    /// Lets the renderer's threads end on their own: a renderer that has
    /// gone, replaced by another. Waiting for one that hangs would hang the
    /// browser.
    pub fn abandon(mut self) {
        self.forwarding.store(false, Ordering::Release);
        self.renderer_thread.take();
        self.io_thread.take();
        // A process is ended, not left: nothing waits for one that is
        // killed.
        if let Some((child, link)) = self.child.take() {
            end_child(child, &link, None);
        }
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

    /// Tells the renderer's compositor that Viz has done with the pixels
    /// of its shared image `mailbox`: for Viz, as `begin_frame_sender`.
    pub fn pixels_returned_sender(
        &self,
    ) -> impl Fn(protocol::viz::Mailbox) + Send + Sync + 'static {
        let to_compositor = self.to_compositor.clone();
        move |mailbox| {
            let _ = to_compositor.send(CompositorMsg::SharedImagePixelsReturned { mailbox });
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
        navigation_type: FrameLoadType,
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

    /// Asks the renderer to exit and waits for it (a tab closed), a
    /// process for [`CHILD_EXIT_WAIT`] at most.
    pub fn shutdown(&mut self) {
        self.ask_to_shut_down();
        self.join_threads();
        if let Some((child, link)) = self.child.take() {
            end_child(child, &link, Some(CHILD_EXIT_WAIT));
        }
    }

    /// Asks the renderer to exit, without waiting: machopo, ending, asks
    /// every renderer first, then waits for them all with one deadline
    /// ([`wait_for_shutdown`](Self::wait_for_shutdown)), so that hung ones
    /// take that long however many they are.
    pub fn ask_to_shut_down(&self) {
        let _ = self.to_renderer.send(RendererMsg::Shutdown);
    }

    /// Waits for the renderer [`ask_to_shut_down`](Self::ask_to_shut_down)
    /// asked to exit until `deadline`: what was given up on then, if
    /// anything. A process still running is killed (its trace not
    /// written; Chromium kills a child it can't wait for,
    /// `base::EnsureProcessTerminated`). A renderer of this process has its
    /// script stopped, again and again till its thread ends (the script of
    /// the tasks queued before the message to exit too): one whose thread
    /// doesn't end even so (in native code) is left running, machopo to
    /// end without it. (A tab closed, [`shutdown`](Self::shutdown), waits
    /// for its threads as long as it takes.)
    pub fn wait_for_shutdown(&mut self, deadline: std::time::Instant) -> Option<GaveUp> {
        if let (Some(terminator), Some(thread)) = (&self.terminator, &self.renderer_thread) {
            while !thread.is_finished() {
                if std::time::Instant::now() >= deadline {
                    // Its IO thread ends with it: both are left.
                    self.renderer_thread.take();
                    self.io_thread.take();
                    return Some(GaveUp::LeftRunning);
                }
                terminator.terminate();
                std::thread::sleep(SCRIPT_STOP_INTERVAL);
            }
        }
        self.join_threads();
        let (child, link) = self.child.take()?;
        let pid = child.id();
        let wait = deadline.saturating_duration_since(std::time::Instant::now());
        end_child(child, &link, Some(wait))
            .killed
            .then_some(GaveUp::Killed { pid })
    }
}

impl RenderProcessHost {
    /// The renderer's threads in this process, ended.
    fn join_threads(&mut self) {
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

/// What [`RenderProcessHost::wait_for_shutdown`] gave up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GaveUp {
    /// The renderer's process, killed.
    Killed { pid: u32 },
    /// The renderer's threads, in this process, left running.
    LeftRunning,
}

/// How often the script of a renderer of this process is stopped while
/// machopo waits for it to end.
const SCRIPT_STOP_INTERVAL: std::time::Duration = std::time::Duration::from_millis(10);

/// How long a renderer process asked to exit is given before it is ended.
const CHILD_EXIT_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// How long a renderer process told to crash is given to have done it.
const CRASH_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// The shared memory a renderer process that draws for a GPU starts
/// with: how many regions, each as long as a tile's pixels (256 by 256,
/// four bytes each).
const TILE_REGIONS: usize = 16;
const TILE_REGION_LEN: usize = 256 << 10;

/// How long a renderer process whose channel is over is given to have
/// exited by itself (the channel of one that crashed is over a moment
/// before the system says the process is) before it is killed.
const GONE_EXIT_WAIT: std::time::Duration = std::time::Duration::from_millis(200);

/// How [`end_child`] ended a process.
struct ChildEnd {
    /// What the system says of it.
    status: Option<std::process::ExitStatus>,
    /// It didn't exit by itself in time, and was killed.
    killed: bool,
}

/// Ends a renderer process: waits `wait` for it to exit by itself (it was
/// asked to), then kills it; closes this end of its channel, and tells
/// the platform it has gone (a child that never took its end).
fn end_child(
    mut child: ipc::platform::Child,
    link: &ChannelLink,
    wait: Option<std::time::Duration>,
) -> ChildEnd {
    let asked = std::time::Instant::now();
    let exited = wait.is_some_and(|wait| {
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break true,
                Ok(None) if asked.elapsed() < wait => {
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                _ => break false,
            }
        }
    });
    let status = if exited {
        child.try_wait().ok().flatten()
    } else {
        let _ = child.kill();
        child.wait().ok()
    };
    tracing::info!(
        pid = child.id(),
        ?status,
        killed = !exited,
        "a renderer process ended"
    );
    link.close();
    ipc::platform::child_gone(child.id());
    ChildEnd {
        status,
        killed: !exited,
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
    navigation_type: FrameLoadType,
}

/// A navigation's document as it commits.
pub(crate) struct CommittedDocument {
    pub(crate) url: String,
    pub(crate) mime_type: String,
    pub(crate) charset: Option<String>,
    pub(crate) content_language: Option<String>,
    /// The body's first bytes, or all of it without a `body_pipe`.
    pub(crate) body: Bytes,
    /// The rest of the body, for one that came over the network.
    pub(crate) body_pipe: Option<ipc::data_pipe::Consumer>,
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
            body_pipe: None,
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
    /// is sent): what changed since it started counts, if it is pending
    /// (`place`). `entries_heard`: of the renderer's own new entries, how
    /// many the browser has heard of by then.
    pub(crate) fn set_history_place(&mut self, place: Option<(u32, u32)>, entries_heard: u64) {
        if let Some((index, length)) = place {
            self.history.index = index;
            self.history.length = length;
        }
        self.history.entries_heard = entries_heard;
    }

    /// Sends the commit, its subresources through loader `loader`; what
    /// tells the renderer how its load ended, if it hasn't yet, comes back.
    pub(crate) fn commit(
        self,
        navigation_id: u64,
        document: CommittedDocument,
        loader: u64,
    ) -> LoadCompleter {
        let completer = LoadCompleter {
            to_renderer: self.to_renderer.clone(),
            frame: self.frame,
            navigation_id,
        };
        let _ = self.to_renderer.send(RendererMsg::CommitNavigation {
            frame: self.frame,
            loader,
            navigation_id,
            begin_id: self.begin_id,
            url: document.url,
            error_page: document.error_page,
            mime_type: document.mime_type,
            charset: document.charset,
            content_language: document.content_language,
            body: document.body.into(),
            body_pipe: document.body_pipe,
            history: self.history,
            navigation_start: self.navigation_start,
            loaded: document.loaded.map(Box::new),
            navigation_type: self.navigation_type,
        });
        completer
    }
}

/// Tells the renderer how the load of a document it was sent to commit
/// ended (`URLLoaderClient::OnComplete`), through the renderer it went to.
pub(crate) struct LoadCompleter {
    to_renderer: RendererSender,
    frame: FrameToken,
    navigation_id: u64,
}

impl LoadCompleter {
    pub(crate) fn complete(self, status: protocol::network::CompletionStatus) {
        let _ = self.to_renderer.send(RendererMsg::NavigationLoadCompleted {
            frame: self.frame,
            navigation_id: self.navigation_id,
            status,
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

/// What the browser does with one renderer's messages before any reaches
/// the UI thread (Chromium's `RenderProcessHostImpl::IOThreadHostImpl` and
/// the interfaces bound on the IO thread: the renderer's
/// `URLLoaderFactory`, its `BlobURLStore`, its frames'
/// `RestrictedCookieManager`s): it checks each against what the browser
/// gave the renderer, sends subresource requests, script's cookies and
/// storage changes straight to their services, and passes the rest on.
/// One way in, [`on_message`](Self::on_message): whatever reads the
/// renderer's channel (a thread's queue now, a process's pipe later) hands
/// each message here, and the checks are the same either way.
struct IoThreadHost {
    renderer_id: u64,
    network: NetworkContext,
    dom_storage: crate::dom_storage::DomStorageContext,
    to_renderer: RendererSender,
    /// What answers the renderer's questions of the system's fonts, if it
    /// was given them.
    fonts: Option<&'static FontDataService>,
    file_access: Arc<AtomicBool>,
    /// The loaders the browser gave the renderer's documents.
    loaders: Arc<Mutex<Loaders>>,
    /// To the UI thread: a message, or why one was bad.
    forward: Box<dyn Fn(Result<RendererHostMsg, BadMessageReason>) + Send>,
    /// The renderer's loads still loading.
    loads: Loads,
    /// Its dedicated workers, which may load (Chromium's
    /// `DedicatedWorkerHost`s, each a `URLLoaderFactory`), and the blob
    /// URLs each registered (its `BlobURLStore`'s), which go with it.
    workers: HashMap<u64, Worker>,
    worker_blob_urls: HashMap<u64, Vec<String>>,
}

impl IoThreadHost {
    fn forward_on(&self, msg: RendererHostMsg) {
        (self.forward)(Ok(msg));
    }

    /// A question of the system's fonts, to the font thread, which
    /// answers the renderer; a bad one answered with nothing all the same,
    /// so the main thread waits no more.
    fn ask_fonts(&self, request: FontRequest) {
        let asked = match self.fonts {
            Some(fonts) => fonts.ask(&self.to_renderer, request),
            None => {
                request.refuse(&self.to_renderer);
                Err(BadMessageReason::FontRequest)
            }
        };
        if let Err(reason) = asked {
            (self.forward)(Err(reason));
        }
    }

    fn fetch(&self, load: LoadKey, request: ResourceRequest) {
        fetch_for_renderer(
            &self.network,
            self.renderer_id,
            &self.file_access,
            &self.to_renderer,
            &self.loads,
            load,
            request,
        );
    }

    /// A message from the renderer.
    fn on_message(&mut self, msg: RendererHostMsg) {
        match msg {
            RendererHostMsg::FetchResource {
                request_id,
                worker,
                loader,
                mut request,
            } => {
                let load = LoadKey::Resource { worker, request_id };
                let checked = check_request(
                    &self.loaders.lock().unwrap_or_else(PoisonError::into_inner),
                    &self.workers,
                    worker,
                    loader,
                    &request,
                );
                let trusted = match checked {
                    Ok(trusted) => trusted,
                    Err(reason) => {
                        let _ =
                            self.to_renderer.send(load.answer(Err(NetError::new(
                                NetErrorCode::AccessDenied,
                                request.url,
                            ))));
                        (self.forward)(Err(reason));
                        return;
                    }
                };
                request.trusted = trusted;
                // The browser loads every frame's document,
                // as a navigation: a renderer asking for one
                // is bad (the network service's "frame
                // destination requires kNavigate mode").
                if matches!(
                    request.destination,
                    RequestDestination::Document | RequestDestination::Iframe
                ) {
                    let _ = self.to_renderer.send(
                        load.answer(Err(NetError::new(NetErrorCode::AccessDenied, request.url))),
                    );
                    (self.forward)(Err(BadMessageReason::DocumentRequest));
                } else if request.trusted.is_none() {
                    // One it didn't make, or gone.
                    let _ = self.to_renderer.send(
                        load.answer(Err(NetError::new(NetErrorCode::AccessDenied, request.url))),
                    );
                } else {
                    self.fetch(load, request);
                }
            }
            RendererHostMsg::DedicatedWorkerCreated {
                worker,
                loader,
                mut request,
            } => {
                if self.workers.contains_key(&worker) {
                    return;
                }
                let checked = check_request(
                    &self.loaders.lock().unwrap_or_else(PoisonError::into_inner),
                    &self.workers,
                    None,
                    loader,
                    &request,
                );
                match checked {
                    Ok(trusted) => {
                        request.trusted = trusted;
                        self.workers.insert(worker, Worker { loader });
                        self.fetch(LoadKey::WorkerScript(worker), request);
                    }
                    Err(reason) => (self.forward)(Err(reason)),
                }
            }
            // Its loads close with it.
            RendererHostMsg::DedicatedWorkerGone { worker } => {
                self.workers.remove(&worker);
                // Revoked, as a revoke of each: for loads
                // here, for navigations on the UI thread.
                for url in self.worker_blob_urls.remove(&worker).unwrap_or_default() {
                    self.network.revoke_blob_url(self.renderer_id, &url);
                    self.forward_on(RendererHostMsg::RevokeBlobUrl { url });
                }
                let mut loads = self.loads.lock().unwrap_or_else(|e| e.into_inner());
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
                    self.network.register_blob_url(
                        self.renderer_id,
                        &url,
                        mime_type,
                        bytes.into_bytes(),
                    );
                }
                // One it didn't make, or gone: nothing.
                Some(worker) if self.workers.contains_key(&worker) => {
                    self.network.register_blob_url(
                        self.renderer_id,
                        &url,
                        mime_type,
                        bytes.into_bytes(),
                    );
                    self.worker_blob_urls.entry(worker).or_default().push(url);
                }
                Some(_) => {}
            },
            // For loads here; the UI thread forgets it after
            // the navigations asked for before.
            RendererHostMsg::RevokeBlobUrl { url } => {
                for urls in self.worker_blob_urls.values_mut() {
                    urls.retain(|registered| *registered != url);
                }
                self.network.revoke_blob_url(self.renderer_id, &url);
                self.forward_on(RendererHostMsg::RevokeBlobUrl { url });
            }
            // To the storage thread at once: after the
            // answer that gave the renderer the area (the
            // UI thread's), before any open after it.
            RendererHostMsg::ChangeStorage {
                area,
                change,
                source,
                url,
            } => {
                self.dom_storage
                    .change(self.renderer_id, area, change, source, url);
            }
            RendererHostMsg::ForgetStorageAreas => self.dom_storage.forget(self.renderer_id),
            // Script's cookies, to the network service at
            // once, in order (the frame's
            // `RestrictedCookieManager`); a bad one answered
            // all the same, so the main thread waits no more.
            RendererHostMsg::GetCookies {
                loader,
                url,
                request_id,
            } => {
                let checked = check_cookie_access(
                    &self.loaders.lock().unwrap_or_else(PoisonError::into_inner),
                    loader,
                    &url,
                );
                let answer = self.to_renderer.clone();
                let done = move |cookies| {
                    let _ = answer.send(RendererMsg::Cookies {
                        request_id,
                        cookies,
                    });
                };
                match checked {
                    Ok((url, trusted)) => {
                        self.network.get_cookies_for_script(url, trusted, done);
                    }
                    Err(reason) => {
                        done(String::new());
                        (self.forward)(Err(reason));
                    }
                }
            }
            RendererHostMsg::SetCookie { loader, url, line } => {
                let checked = check_cookie_access(
                    &self.loaders.lock().unwrap_or_else(PoisonError::into_inner),
                    loader,
                    &url,
                );
                match checked {
                    Ok((url, trusted)) => {
                        self.network.set_cookie_from_script(url, trusted, line);
                    }
                    Err(reason) => (self.forward)(Err(reason)),
                }
            }
            RendererHostMsg::CookiesEnabled {
                loader,
                url,
                request_id,
            } => {
                let checked = check_cookie_access(
                    &self.loaders.lock().unwrap_or_else(PoisonError::into_inner),
                    loader,
                    &url,
                );
                let answer = self.to_renderer.clone();
                let done = move |enabled| {
                    let _ = answer.send(RendererMsg::CookiesEnabled {
                        request_id,
                        enabled,
                    });
                };
                match checked {
                    Ok((url, trusted)) => {
                        self.network.cookies_enabled_for(url, trusted, done);
                    }
                    Err(reason) => {
                        done(false);
                        (self.forward)(Err(reason));
                    }
                }
            }
            RendererHostMsg::GetFontFamily { family, request_id } => {
                self.ask_fonts(FontRequest::Family { family, request_id });
            }
            RendererHostMsg::GetFontFallback { script, request_id } => {
                self.ask_fonts(FontRequest::Fallback { script, request_id });
            }
            RendererHostMsg::GetFontFiles { files, request_id } => {
                self.ask_fonts(FontRequest::Files { files, request_id });
            }
            RendererHostMsg::FollowRedirect { request_id, worker } => {
                if let Some(load) = self
                    .loads
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&LoadKey::Resource { worker, request_id })
                {
                    load.follow_redirect();
                }
            }
            RendererHostMsg::CancelResource { request_id, worker } => {
                let load = self
                    .loads
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&LoadKey::Resource { worker, request_id });
                if let Some(load) = load {
                    load.cancel();
                }
            }
            msg => self.forward_on(msg),
        }
    }

    /// The renderer's channel closed (Chromium's `OnChannelError`): its
    /// loads go with it (its `URLLoader`s close), and its blob URLs.
    fn on_channel_closed(self) {
        for (_, load) in self.loads.lock().unwrap_or_else(|e| e.into_inner()).drain() {
            load.cancel();
        }
        self.network.revoke_blob_urls_of(self.renderer_id);
        self.dom_storage.forget(self.renderer_id);
        self.forward_on(RendererHostMsg::RendererGone);
    }
}

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
    let mut held = loads.lock().unwrap_or_else(|e| e.into_inner());
    let over = loads.clone();
    let answered = to_renderer.clone();
    let done = move |result| {
        over.lock().unwrap_or_else(|e| e.into_inner()).remove(&load);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn loaders() -> Loaders {
        let mut loaders = Loaders::default();
        loaders.params.insert(
            1,
            LoaderParams {
                origin: url::Url::parse("https://a.example/").unwrap().origin(),
                trusted: TrustedParams {
                    site_for_cookies: Some("https://a.example".to_owned()),
                    top_frame_site: Some("https://a.example".to_owned()),
                    ..TrustedParams::default()
                },
            },
        );
        loaders.params.insert(
            2,
            LoaderParams {
                origin: url::Url::parse("data:,").unwrap().origin(),
                trusted: TrustedParams::default(),
            },
        );
        loaders
    }

    fn request(initiator: Option<&str>) -> ResourceRequest {
        let mut request = ResourceRequest::new("https://b.example/x", RequestDestination::Image);
        request.initiator = initiator.map(str::to_owned);
        request
    }

    /// A request through a loader the renderer was given takes the
    /// loader's trusted params: its initiator none, opaque or the loader's
    /// origin; another origin, a loader it wasn't given, or trusted params
    /// of its own are bad.
    #[test]
    fn a_request_takes_its_loaders_params() {
        let (loaders, workers) = (loaders(), HashMap::new());
        let check = |loader, request: &ResourceRequest| {
            check_request(&loaders, &workers, None, loader, request)
        };
        let trusted = loaders.params[&1].trusted.clone();
        for initiator in [None, Some("null"), Some("https://a.example")] {
            assert_eq!(check(1, &request(initiator)), Ok(Some(trusted.clone())));
        }
        assert_eq!(
            check(1, &request(Some("https://b.example"))),
            Err(BadMessageReason::InitiatorMismatch)
        );
        assert_eq!(
            check(1, &request(Some("https://a.example:444"))),
            Err(BadMessageReason::InitiatorMismatch)
        );
        // An opaque loader's: "null" alone.
        assert_eq!(
            check(2, &request(Some("null"))),
            Ok(Some(TrustedParams::default()))
        );
        assert_eq!(
            check(2, &request(Some("https://a.example"))),
            Err(BadMessageReason::InitiatorMismatch)
        );
        assert_eq!(
            check(3, &request(None)),
            Err(BadMessageReason::UnknownLoader)
        );
        assert_eq!(
            check(0, &request(None)),
            Err(BadMessageReason::UnknownLoader)
        );
        let mut claimed = request(None);
        claimed.trusted = Some(trusted);
        assert_eq!(
            check(1, &claimed),
            Err(BadMessageReason::TrustedParamsFromRenderer)
        );
    }

    /// A worker's request goes through the loader the worker was made
    /// with; through another it is bad; of a worker the renderer has no
    /// more, refused without blame.
    #[test]
    fn a_workers_request_goes_through_its_loader() {
        let loaders = loaders();
        let workers = HashMap::from([(7, Worker { loader: 1 })]);
        let ok = request(Some("https://a.example"));
        assert_eq!(
            check_request(&loaders, &workers, Some(7), 1, &ok),
            Ok(Some(loaders.params[&1].trusted.clone()))
        );
        assert_eq!(
            check_request(&loaders, &workers, Some(7), 2, &ok),
            Err(BadMessageReason::WorkerLoaderMismatch)
        );
        assert_eq!(check_request(&loaders, &workers, Some(8), 1, &ok), Ok(None));
    }

    /// Script's cookies through a loader: of a URL of the loader's origin,
    /// by its trusted params; of another origin (another port too), a URL
    /// that isn't one, an opaque origin's loader or one not given are bad.
    #[test]
    fn script_cookies_are_of_the_loaders_origin() {
        let loaders = loaders();
        let checked = check_cookie_access(&loaders, 1, "https://a.example/p?q");
        assert_eq!(
            checked,
            Ok((
                url::Url::parse("https://a.example/p?q").unwrap(),
                loaders.params[&1].trusted.clone()
            ))
        );
        for url in [
            "https://b.example/",
            "https://a.example:444/",
            "http://a.example/",
            "a",
        ] {
            assert_eq!(
                check_cookie_access(&loaders, 1, url),
                Err(BadMessageReason::CookieUrlMismatch),
                "{url}"
            );
        }
        assert_eq!(
            check_cookie_access(&loaders, 2, "https://a.example/"),
            Err(BadMessageReason::CookiesOfOpaqueOrigin)
        );
        assert_eq!(
            check_cookie_access(&loaders, 3, "https://a.example/"),
            Err(BadMessageReason::UnknownLoader)
        );
    }

    /// An `IoThreadHost` with no renderer and no thread: what it passes
    /// on, and the renderer's end of its queue (kept, so sends reach it).
    fn io_thread_host() -> (
        IoThreadHost,
        crossbeam_channel::Receiver<Result<RendererHostMsg, BadMessageReason>>,
        renderer::RendererReceiver,
    ) {
        let (to_renderer, _to_compositor, from_browser) = renderer::channel();
        let (forward, forwarded) = crossbeam_channel::unbounded();
        let host = IoThreadHost {
            renderer_id: 7,
            network: NetworkContext::start(net::NetworkContextParams::default()).unwrap(),
            dom_storage: crate::dom_storage::DomStorageContext::new(&crate::Profile::in_memory())
                .unwrap(),
            to_renderer,
            fonts: None,
            file_access: Arc::new(AtomicBool::new(false)),
            loaders: Arc::new(Mutex::new(loaders())),
            forward: Box::new(move |msg| {
                let _ = forward.send(msg);
            }),
            loads: Loads::default(),
            workers: HashMap::new(),
            worker_blob_urls: HashMap::new(),
        };
        (host, forwarded, from_browser)
    }

    fn blob_loads(host: &IoThreadHost, url: &str) -> bool {
        let request = ResourceRequest::new(url, RequestDestination::Empty);
        host.network
            .load_blob_url(&request, Some(host.renderer_id))
            .is_ok()
    }

    fn register_blob(worker: Option<u64>, url: &str) -> RendererHostMsg {
        RendererHostMsg::RegisterBlobUrl {
            worker,
            url: url.to_owned(),
            mime_type: "text/plain".to_owned(),
            bytes: b"blob".into(),
        }
    }

    /// The host takes each message by its one way in, with no thread or
    /// channel of the renderer's behind it. What isn't its to handle goes
    /// on to the UI thread as it came; a request through a loader the
    /// renderer wasn't given, one for a document, and a cookie read through
    /// such a loader go on as the reasons they are bad.
    #[test]
    fn the_io_thread_host_passes_on_and_refuses() {
        let (mut host, forwarded, _renderer) = io_thread_host();
        host.on_message(RendererHostMsg::RendererReady);
        assert!(matches!(
            forwarded.try_recv(),
            Ok(Ok(RendererHostMsg::RendererReady))
        ));

        host.on_message(RendererHostMsg::FetchResource {
            request_id: 1,
            worker: None,
            loader: 9,
            request: request(None),
        });
        assert_eq!(
            forwarded.try_recv().unwrap().err(),
            Some(BadMessageReason::UnknownLoader)
        );

        let document = ResourceRequest::new("https://a.example/", RequestDestination::Document);
        host.on_message(RendererHostMsg::FetchResource {
            request_id: 2,
            worker: None,
            loader: 1,
            request: document,
        });
        assert_eq!(
            forwarded.try_recv().unwrap().err(),
            Some(BadMessageReason::DocumentRequest)
        );

        host.on_message(RendererHostMsg::GetCookies {
            loader: 9,
            url: "https://a.example/".to_owned(),
            request_id: 3,
        });
        assert_eq!(
            forwarded.try_recv().unwrap().err(),
            Some(BadMessageReason::UnknownLoader)
        );
        // None of them left a load behind.
        assert!(host.loads.lock().unwrap().is_empty());
        assert!(forwarded.try_recv().is_err());
    }

    /// A worker's blob URLs go with it, each told to the UI thread; one
    /// registered for a worker the renderer doesn't have registers nothing.
    /// When the renderer's channel closes its blob URLs go, and the UI
    /// thread is told the renderer has gone, last.
    #[test]
    fn the_io_thread_hosts_blob_urls() {
        let (mut host, forwarded, _renderer) = io_thread_host();
        let script = ResourceRequest::new("data:text/javascript,1", RequestDestination::Worker);
        host.on_message(RendererHostMsg::DedicatedWorkerCreated {
            worker: 1,
            loader: 1,
            request: script,
        });
        host.on_message(register_blob(Some(1), "blob:https://a.example/workers"));
        host.on_message(register_blob(Some(5), "blob:https://a.example/no-ones"));
        host.on_message(register_blob(None, "blob:https://a.example/documents"));
        assert!(blob_loads(&host, "blob:https://a.example/workers"));
        assert!(!blob_loads(&host, "blob:https://a.example/no-ones"));
        assert!(blob_loads(&host, "blob:https://a.example/documents"));

        host.on_message(RendererHostMsg::DedicatedWorkerGone { worker: 1 });
        assert!(!blob_loads(&host, "blob:https://a.example/workers"));
        assert!(blob_loads(&host, "blob:https://a.example/documents"));
        assert!(matches!(
            forwarded.try_recv(),
            Ok(Ok(RendererHostMsg::RevokeBlobUrl { url })) if url == "blob:https://a.example/workers"
        ));
        assert!(forwarded.try_recv().is_err());

        let network = host.network.clone();
        host.on_channel_closed();
        let request = ResourceRequest::new(
            "blob:https://a.example/documents",
            RequestDestination::Empty,
        );
        assert!(network.load_blob_url(&request, Some(7)).is_err());
        assert!(matches!(
            forwarded.try_recv(),
            Ok(Ok(RendererHostMsg::RendererGone))
        ));
        assert!(forwarded.try_recv().is_err());
    }

    /// A font thread of the test's own, the test fonts its system's.
    fn test_font_service() -> &'static FontDataService {
        let service = FontDataService::start(|| {
            let fonts = std::path::Path::new(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../resources/test_fonts"
            ));
            renderer::SystemFontManager::for_testing(&[fonts], &[("serif", "Lato")])
        })
        .expect("a font thread");
        Box::leak(Box::new(service))
    }

    /// The answer to call `request_id` that reaches `renderer`, waited
    /// for no longer than a test would.
    fn reply(renderer: &'static renderer::RendererReceiver, request_id: u64) -> RendererMsg {
        let (got, reply) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = got.send(renderer.reply_for_testing(request_id));
        });
        reply
            .recv_timeout(std::time::Duration::from_secs(30))
            .expect("an answer")
            .expect("the renderer's queue is there")
    }

    fn family_named(fonts: &FontDataService, name: &str) -> u32 {
        fonts
            .names()
            .names
            .iter()
            .find(|(each, _)| each == name)
            .expect("the family")
            .1
    }

    /// A renderer's questions of the system's fonts go to the font thread,
    /// which answers the renderer itself: a family's faces, each with its
    /// file's number, the file of that number open to read, a script's
    /// fallback.
    #[test]
    fn the_font_thread_answers_what_the_io_thread_passes_on() {
        let (mut host, forwarded, renderer) = io_thread_host();
        let fonts = test_font_service();
        host.fonts = Some(fonts);
        let renderer = &*Box::leak(Box::new(renderer));
        host.on_message(RendererHostMsg::GetFontFamily {
            family: family_named(fonts, "Ahem"),
            request_id: 1,
        });
        let RendererMsg::FontFamily {
            faces: Some(faces), ..
        } = reply(renderer, 1)
        else {
            panic!("no faces");
        };
        assert_eq!(faces.name, "Ahem");
        assert_eq!(faces.faces.len(), 1);
        assert_eq!(faces.faces[0].index, 0);
        host.on_message(RendererHostMsg::GetFontFiles {
            files: vec![faces.faces[0].file],
            request_id: 2,
        });
        let RendererMsg::FontFiles { files, .. } = reply(renderer, 2) else {
            panic!("no files");
        };
        let [Some(file)] = <[_; 1]>::try_from(files).expect("one file") else {
            panic!("the file wasn't opened");
        };
        let ahem = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../resources/test_fonts/Ahem.ttf"
        ))
        .unwrap();
        assert_eq!(&file.map().expect("mapped")[..], &ahem[..]);
        host.on_message(RendererHostMsg::GetFontFallback {
            script: *b"Latn",
            request_id: 3,
        });
        assert!(matches!(
            reply(renderer, 3),
            RendererMsg::FontFallback { families, .. } if families.is_empty()
        ));
        assert!(forwarded.try_recv().is_err());
    }

    /// What the browser gave no number, more files than go with a message,
    /// no file, a script that is no tag, and any question from a renderer
    /// it doesn't serve the fonts are bad: answered with nothing, so the
    /// renderer waits no more, and told to the UI thread as the reason.
    #[test]
    fn the_io_thread_refuses_font_questions_of_nothing_it_gave() {
        let (mut host, forwarded, renderer) = io_thread_host();
        let fonts = test_font_service();
        host.fonts = Some(fonts);
        let renderer = &*Box::leak(Box::new(renderer));
        let refused = |request_id| {
            assert_eq!(
                forwarded.try_recv().unwrap().err(),
                Some(BadMessageReason::FontRequest),
                "{request_id}"
            );
            match reply(renderer, request_id) {
                RendererMsg::FontFamily { faces, .. } => assert!(faces.is_none()),
                RendererMsg::FontFallback { families, .. } => assert!(families.is_empty()),
                RendererMsg::FontFiles { files, .. } => assert!(files.is_empty()),
                other => panic!("{other:?}"),
            }
        };
        host.on_message(RendererHostMsg::GetFontFamily {
            family: fonts.names().families,
            request_id: 1,
        });
        refused(1);
        // No family answered yet: no file numbered.
        host.on_message(RendererHostMsg::GetFontFiles {
            files: vec![0],
            request_id: 2,
        });
        refused(2);
        host.on_message(RendererHostMsg::GetFontFamily {
            family: family_named(fonts, "Lato"),
            request_id: 3,
        });
        let RendererMsg::FontFamily {
            faces: Some(faces), ..
        } = reply(renderer, 3)
        else {
            panic!("no faces");
        };
        let file = faces.faces[0].file;
        host.on_message(RendererHostMsg::GetFontFiles {
            files: vec![file.checked_add(1).unwrap()],
            request_id: 4,
        });
        refused(4);
        host.on_message(RendererHostMsg::GetFontFiles {
            files: vec![file; 17],
            request_id: 5,
        });
        refused(5);
        host.on_message(RendererHostMsg::GetFontFiles {
            files: Vec::new(),
            request_id: 6,
        });
        refused(6);
        host.on_message(RendererHostMsg::GetFontFallback {
            script: *b"La1n",
            request_id: 7,
        });
        refused(7);
        // Sixteen of it is one message's worth.
        host.on_message(RendererHostMsg::GetFontFiles {
            files: vec![file; 16],
            request_id: 8,
        });
        assert!(matches!(
            reply(renderer, 8),
            RendererMsg::FontFiles { files, .. } if files.len() == 16 && files.iter().all(Option::is_some)
        ));
        host.fonts = None;
        host.on_message(RendererHostMsg::GetFontFamily {
            family: 0,
            request_id: 9,
        });
        refused(9);
        assert!(forwarded.try_recv().is_err());
    }
}
