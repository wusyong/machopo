//! Network (`//services/network`): loads `http:`, `https:`, `file:` and `data:`
//! URLs for the browser (navigations) and, through it, the renderer
//! (subresources); and the `blob:` URLs renderers register (`blob`).
//!
//! Everything runs on one thread, `NetworkService`, with a tokio runtime on
//! it, so neither the UI thread nor a renderer ever waits on the network.
//! [`NetworkContext`] is the handle callers keep; its [`fetch`] answers
//! through a callback that runs on that thread, and gives a [`LoadHandle`]
//! that can cancel the load.
//!
//! Two layers, as in Chromium's `//net`:
//! - [`url_loader`] (`URLRequest`): the scheme, following redirects, and
//!   turning the result into `protocol::network` types;
//! - [`http`] (`HttpStreamFactory` and the streams under it): sends one
//!   HTTP request and returns the response. reqwest lives only here; no
//!   reqwest type leaves the crate, so another transport can replace it
//!   without changing callers.
//!
//! No cookies and no HTTP cache.
//!
//! [`NetworkContextParams`] is for web tests only: host mapping rules and
//! extra trusted roots, so WPT's domains reach the local wptserve over
//! HTTPS. A browser window starts with the default, empty params.
//!
//! [`fetch`]: NetworkContext::fetch

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

mod blob;
mod content_encoding;
mod cors;
mod data;
mod file;
mod host_mapping;
mod http;
mod mime;
pub mod test_server;
mod url_loader;

use std::sync::{Arc, Mutex};

use protocol::network::{LoadResult, RedirectInfo, ResourceRequest};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

pub use file::{file_path, file_url};
pub use host_mapping::{HostMapping, HostMappingRules};

type Done = Box<dyn FnOnce(LoadResult) + Send>;
type OnRedirect = Box<dyn FnMut(RedirectInfo) + Send>;

struct Job {
    request: ResourceRequest,
    /// For a load that waits at each redirect.
    on_redirect: Option<OnRedirect>,
    done: Done,
    state: Arc<Mutex<LoadState>>,
}

/// What a load's handle and the `NetworkService` thread share.
#[derive(Default)]
struct LoadState {
    cancelled: bool,
    /// The load's task, once the `NetworkService` thread started it.
    task: Option<AbortHandle>,
    /// While the load waits at a redirect: what lets it go on.
    follow: Option<oneshot::Sender<()>>,
}

/// Where a load that waits at redirects tells of one.
struct Redirects {
    tell: OnRedirect,
    state: Arc<Mutex<LoadState>>,
}

impl Redirects {
    /// Tells of `redirect` and waits until the load's handle follows it.
    async fn wait_at(&mut self, redirect: RedirectInfo) {
        let (follow, followed) = oneshot::channel();
        // Before telling, so a follow that comes at once finds it.
        self.state.lock().expect("a load's state").follow = Some(follow);
        (self.tell)(redirect);
        // The state keeps the sender until the handle follows: this only
        // ends when it does (or the task is aborted).
        let _ = followed.await;
    }
}

/// A load in flight (Chromium's `URLLoader` remote): cancelling it drops
/// the load, which never calls back; dropping the handle without
/// cancelling leaves it running (or, at a redirect, waiting for good).
pub struct LoadHandle {
    state: Arc<Mutex<LoadState>>,
}

impl LoadHandle {
    /// Lets a load waiting at a redirect go on to where it leads
    /// (`URLLoader::FollowRedirect`). Nothing if it isn't waiting at one.
    pub fn follow_redirect(&self) {
        let follow = self.state.lock().expect("a load's state").follow.take();
        if let Some(follow) = follow {
            let _ = follow.send(());
        }
    }

    /// Stops the load (closing the `URLLoader` pipe): it stops at its next
    /// wait, without calling back; one that called back already is over.
    pub fn cancel(self) {
        let mut state = self.state.lock().expect("a load's state");
        state.cancelled = true;
        if let Some(task) = state.task.take() {
            task.abort();
        }
    }
}

/// `network::mojom::NetworkContextParams`, the part of it machopo has: the
/// user agent, and what only web tests set. The default sends no
/// `User-Agent` and changes nothing else.
#[derive(Debug, Clone, Default)]
pub struct NetworkContextParams {
    /// What requests send as `User-Agent`: the browser's, the one
    /// `navigator.userAgent` answers too. Empty: none.
    pub user_agent: String,
    /// Which hosts resolve to a fixed address, or not at all, instead of
    /// asking the system (`--host-resolver-rules`).
    pub host_resolver_rules: HostMappingRules,
    /// Root certificates (DER) trusted on top of the system's, for HTTPS
    /// (`CertVerifierCreationParams`' additional trust anchors).
    /// Certificates are still verified: their chain has to reach one of
    /// these or a system root, and their name has to match.
    pub additional_trust_anchors: Vec<Vec<u8>>,
}

impl NetworkContextParams {
    /// Adds every certificate of a PEM file to
    /// [`additional_trust_anchors`](Self::additional_trust_anchors); an
    /// error if it has none, or one doesn't parse.
    pub fn add_trust_anchors_pem(&mut self, pem: &[u8]) -> Result<(), String> {
        use rustls::pki_types::CertificateDer;
        use rustls::pki_types::pem::PemObject;
        let certificates = CertificateDer::pem_slice_iter(pem)
            .map(|c| c.map(|c| c.to_vec()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("bad PEM: {e}"))?;
        if certificates.is_empty() {
            return Err("no certificate in the PEM".into());
        }
        self.additional_trust_anchors.extend(certificates);
        Ok(())
    }
}

/// `network::mojom::NetworkContext`: where requests go. Cheap to clone;
/// the `NetworkService` thread exits once every clone is dropped (loads
/// still in flight are then abandoned, without calling back).
#[derive(Clone)]
pub struct NetworkContext {
    jobs: mpsc::UnboundedSender<Job>,
    blob_urls: blob::BlobUrlRegistry,
}

impl NetworkContext {
    /// Starts the `NetworkService` thread.
    pub fn start(params: NetworkContextParams) -> std::io::Result<Self> {
        let (jobs, mut rx) = mpsc::unbounded_channel::<Job>();
        std::thread::Builder::new()
            .name("NetworkService".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime for the network service");
                runtime.block_on(async move {
                    let transport = http::HttpTransport::new(&params);
                    while let Some(job) = rx.recv().await {
                        let transport = transport.clone();
                        let mut state = job.state.lock().expect("a load's state");
                        if state.cancelled {
                            continue;
                        }
                        let (request, done) = (job.request, job.done);
                        let redirects = job.on_redirect.map(|tell| Redirects {
                            tell,
                            state: job.state.clone(),
                        });
                        let task = tokio::spawn(async move {
                            let result = url_loader::load(&transport, &request, redirects).await;
                            if let Err(err) = &result {
                                tracing::debug!(url = %request.url, code = err.code.name(), detail = %err.detail, "load failed");
                            }
                            done(result);
                        });
                        state.task = Some(task.abort_handle());
                    }
                });
            })?;
        Ok(Self {
            jobs,
            blob_urls: blob::BlobUrlRegistry::default(),
        })
    }

    /// Loads `request` and calls `done` with the result, on the
    /// `NetworkService` thread, unless the handle cancels it first. Never
    /// blocks. It follows redirects by itself.
    pub fn fetch(
        &self,
        request: ResourceRequest,
        done: impl FnOnce(LoadResult) + Send + 'static,
    ) -> LoadHandle {
        self.start_load(request, None, Box::new(done))
    }

    /// Like [`fetch`](Self::fetch), but the load stops at each redirect it
    /// would follow: `on_redirect` hears of it (on the `NetworkService`
    /// thread), and the load waits there until the handle's
    /// [`follow_redirect`](LoadHandle::follow_redirect)
    /// (`URLLoaderClient::OnReceiveRedirect`).
    pub fn fetch_with_redirects(
        &self,
        request: ResourceRequest,
        on_redirect: impl FnMut(RedirectInfo) + Send + 'static,
        done: impl FnOnce(LoadResult) + Send + 'static,
    ) -> LoadHandle {
        self.start_load(request, Some(Box::new(on_redirect)), Box::new(done))
    }

    fn start_load(
        &self,
        request: ResourceRequest,
        on_redirect: Option<OnRedirect>,
        done: Done,
    ) -> LoadHandle {
        let state = Arc::new(Mutex::new(LoadState::default()));
        let job = Job {
            request,
            on_redirect,
            done,
            state: state.clone(),
        };
        if self.jobs.send(job).is_err() {
            tracing::warn!("network service is gone; dropping request");
        }
        LoadHandle { state }
    }

    /// `URL.createObjectURL`'s `url` stands for a blob of `bytes` and
    /// `mime_type`, registered by renderer `owner` (a number unique among
    /// the renderers this context serves).
    pub fn register_blob_url(&self, owner: u64, url: &str, mime_type: String, bytes: Arc<[u8]>) {
        self.blob_urls.register(owner, url, mime_type, bytes);
    }

    /// `url` no longer stands for its blob for loads, if renderer `owner`
    /// registered it: on the thread its loads come through, in order with
    /// them.
    pub fn revoke_blob_url(&self, owner: u64, url: &str) {
        self.blob_urls.revoke(owner, url);
    }

    /// `url` no longer stands for its blob for navigations either: on the
    /// thread its navigations are handled on, in order with them.
    pub fn forget_blob_url(&self, owner: u64, url: &str) {
        self.blob_urls.forget(owner, url);
    }

    /// Every blob URL renderer `owner` registered goes: it has gone.
    pub fn revoke_blob_urls_of(&self, owner: u64) {
        self.blob_urls.revoke_all_of(owner);
    }

    /// Loads the `blob:` URL `request` is for, now, on the calling thread:
    /// for renderer `requester` (a subresource: only a URL it registered),
    /// or for a navigation (`None`).
    pub fn load_blob_url(&self, request: &ResourceRequest, requester: Option<u64>) -> LoadResult {
        let start = ipc::TimeTicks::now();
        self.blob_urls.load(request, requester).map(|mut response| {
            response.timing = Some(url_loader::local_timing(start));
            response
        })
    }
}
