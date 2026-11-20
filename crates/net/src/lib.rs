//! Network (`//services/network`): loads `http:`, `https:`, `file:` and `data:`
//! URLs for the browser (navigations) and, through it, the renderer
//! (subresources); and the `blob:` URLs renderers register (`blob`).
//!
//! Everything runs on one thread, `NetworkService`, with a tokio runtime on
//! it, so neither the UI thread nor a renderer ever waits on the network.
//! [`NetworkContext`] is the handle callers keep. Its [`load`] tells a
//! [`UrlLoaderClient`] of the load as it goes (Chromium's
//! `URLLoaderClient`): the response's head with a data pipe its body comes
//! through, then how it ended. Its [`fetch`] answers once, with the whole
//! body, for those who take it in one piece. Both call back on that
//! thread, and give a [`LoadHandle`] that can cancel the load.
//!
//! Two layers, as in Chromium's `//net`:
//! - [`url_loader`] (`URLRequest`): the scheme, following redirects, and
//!   turning the result into `protocol::network` types;
//! - [`http`] (`HttpStreamFactory` and the streams under it): sends one
//!   HTTP request and returns the response. reqwest lives only here; no
//!   reqwest type leaves the crate, so another transport can replace it
//!   without changing callers.
//!
//! [`cookies`] (`//net/cookies`) is the cookie jar and the rules it keeps,
//! not yet used by loads. No HTTP cache.
//!
//! [`NetworkContextParams`] is for web tests only: host mapping rules and
//! extra trusted roots, so WPT's domains reach the local wptserve over
//! HTTPS. A browser window starts with the default, empty params.
//!
//! [`load`]: NetworkContext::load
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
pub mod cookies;
mod cors;
mod data;
mod file;
mod host_mapping;
mod http;
mod mime;
pub mod test_server;
mod url_loader;

use std::sync::{Arc, Mutex};

use ipc::TimeTicks;
use ipc::data_pipe;
use protocol::network::{
    CompletionStatus, LoadResult, NetError, NetErrorCode, RedirectInfo, ResourceRequest,
    ResourceResponse, ResponseHead,
};
use tokio::sync::{mpsc, oneshot};
use tokio::task::AbortHandle;

pub use cookies::{
    CanonicalCookie, CookieInclusionStatus, CookieMonster, CookieOptions, CookieSameSite,
    CookieSourceType, ExclusionReason, ParsedCookie, SameSiteCookieContext,
};
pub use file::{file_path, file_url};
pub use host_mapping::{HostMapping, HostMappingRules};
pub use url_loader::concat;

/// The most of a response's body written and not yet read: Chromium's
/// default (`network::features::GetDataPipeDefaultAllocationSize`).
pub const BODY_PIPE_CAPACITY: usize = 512 * 1024;

type Done = Box<dyn FnOnce(LoadResult) + Send>;
type OnRedirect = Box<dyn FnMut(RedirectInfo) + Send>;

/// `network::mojom::URLLoaderClient`: what hears of a load started with
/// [`NetworkContext::load`], on the `NetworkService` thread, in order: the
/// redirects it waits at (if it does), then the response's head and the
/// body's data pipe, then how it ended; or, for a load that produced no
/// response, only how it ended. A cancelled load tells nothing more.
pub trait UrlLoaderClient: Send {
    /// A redirect the load waits at (`OnReceiveRedirect`), for a load that
    /// waits at redirects ([`AtRedirects::Wait`]), until its handle's
    /// [`follow_redirect`](LoadHandle::follow_redirect).
    fn on_receive_redirect(&mut self, redirect: RedirectInfo);
    /// The response's head (`OnReceiveResponse`), filtered for its
    /// initiator, and the data pipe its body comes through: the body is
    /// over when the pipe is ([`data_pipe::Read::Done`]). Dropping `body`
    /// stops the load.
    fn on_receive_response(&mut self, head: ResponseHead, body: data_pipe::Consumer);
    /// How it ended (`OnComplete`): after the body's pipe was closed, with
    /// an error if the body didn't all come; or instead of a response.
    fn on_complete(&mut self, status: CompletionStatus);
}

/// What a load does at a redirect it would follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtRedirects {
    /// Follows it, telling no one.
    Follow,
    /// Tells of it and waits until its handle follows it.
    Wait,
}

/// Who hears of a load.
enum Listener {
    /// [`NetworkContext::fetch`]'s and `fetch_with_redirects`': once, with
    /// the whole body.
    Whole {
        on_redirect: Option<OnRedirect>,
        done: Done,
    },
    /// [`NetworkContext::load`]'s.
    Client {
        client: Box<dyn UrlLoaderClient>,
        at_redirects: AtRedirects,
    },
}

struct Job {
    request: ResourceRequest,
    listener: Listener,
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
struct Redirects<'a> {
    tell: &'a mut (dyn FnMut(RedirectInfo) + Send),
    state: Arc<Mutex<LoadState>>,
}

impl Redirects<'_> {
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
    /// Chromium's cookie policy, its defaults (`--allow-third-party-cookies`):
    /// a third party's cookies are sent and kept, and a cookie without
    /// `SameSite` goes with a top-level navigation of any method in its
    /// first two minutes (Lax-allowing-unsafe). Off: machopo's own, a third
    /// party's are neither, and `Lax` is `Lax`.
    pub allow_third_party_cookies: bool,
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
                    // The context's cookies, which its loads share.
                    let cookies = cookies::CookieJar::new(params.allow_third_party_cookies);
                    while let Some(job) = rx.recv().await {
                        let transport = transport.clone();
                        let cookies = cookies.clone();
                        let mut state = job.state.lock().expect("a load's state");
                        if state.cancelled {
                            continue;
                        }
                        let task = tokio::spawn(run(
                            transport,
                            cookies,
                            job.request,
                            job.listener,
                            job.state.clone(),
                        ));
                        state.task = Some(task.abort_handle());
                    }
                });
            })?;
        Ok(Self {
            jobs,
            blob_urls: blob::BlobUrlRegistry::default(),
        })
    }

    /// Loads `request`, telling `client` of it as it goes (Chromium's
    /// `URLLoaderFactory::CreateLoaderAndStart`), on the `NetworkService`
    /// thread, unless the handle cancels it first. Never blocks.
    pub fn load(
        &self,
        request: ResourceRequest,
        at_redirects: AtRedirects,
        client: impl UrlLoaderClient + 'static,
    ) -> LoadHandle {
        self.start_load(
            request,
            Listener::Client {
                client: Box::new(client),
                at_redirects,
            },
        )
    }

    /// Loads `request` and calls `done` with the result, on the
    /// `NetworkService` thread, unless the handle cancels it first. Never
    /// blocks. It follows redirects by itself.
    pub fn fetch(
        &self,
        request: ResourceRequest,
        done: impl FnOnce(LoadResult) + Send + 'static,
    ) -> LoadHandle {
        self.start_load(
            request,
            Listener::Whole {
                on_redirect: None,
                done: Box::new(done),
            },
        )
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
        self.start_load(
            request,
            Listener::Whole {
                on_redirect: Some(Box::new(on_redirect)),
                done: Box::new(done),
            },
        )
    }

    fn start_load(&self, request: ResourceRequest, listener: Listener) -> LoadHandle {
        let state = Arc::new(Mutex::new(LoadState::default()));
        let job = Job {
            request,
            listener,
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
            response.head.timing = Some(url_loader::local_timing(start));
            response
        })
    }
}

/// One load's task on the `NetworkService` thread: loads `request` and tells
/// its listener.
async fn run(
    transport: http::HttpTransport,
    cookies: cookies::CookieJar,
    request: ResourceRequest,
    listener: Listener,
    state: Arc<Mutex<LoadState>>,
) {
    match listener {
        Listener::Whole {
            mut on_redirect,
            done,
        } => {
            let redirects = on_redirect.as_mut().map(|tell| Redirects {
                tell: &mut **tell,
                state,
            });
            let result = match url_loader::load(&transport, &cookies, &request, redirects).await {
                Ok(response) => collect(response).await,
                Err(err) => Err(err),
            };
            if let Err(err) = &result {
                log_failure(&request, err);
            }
            done(result);
        }
        Listener::Client {
            mut client,
            at_redirects,
        } => {
            let result = {
                let mut tell = |redirect| client.on_receive_redirect(redirect);
                let redirects = (at_redirects == AtRedirects::Wait).then(|| Redirects {
                    tell: &mut tell,
                    state,
                });
                url_loader::load(&transport, &cookies, &request, redirects).await
            };
            let status = match result {
                Ok(response) => stream_body(response, &mut *client).await,
                Err(err) => CompletionStatus {
                    error: Some(err),
                    encoded_body_length: 0,
                    completion_time: TimeTicks::now(),
                },
            };
            if let Some(err) = &status.error {
                log_failure(&request, err);
            }
            client.on_complete(status);
        }
    }
}

/// `response` with all of its body, read into one buffer.
async fn collect(response: url_loader::Response) -> LoadResult {
    let url_loader::Response {
        mut head,
        mut body,
        local,
    } = response;
    let mut pieces = Vec::new();
    while let Some(piece) = body.next().await? {
        pieces.push(piece);
    }
    let end = TimeTicks::now();
    if let Some(timing) = &mut head.timing {
        // A local scheme's response is timed whole: no head before the body.
        if local {
            timing.receive_headers_end = end;
        }
        timing.response_end = end;
    }
    Ok(ResourceResponse {
        head,
        encoded_body_length: body.encoded_length(),
        body: url_loader::concat(pieces),
    })
}

/// Tells `client` of `response`'s head and writes its body into the pipe
/// it reads, as it comes; how that ended.
async fn stream_body(
    response: url_loader::Response,
    client: &mut dyn UrlLoaderClient,
) -> CompletionStatus {
    let url_loader::Response { head, mut body, .. } = response;
    let url = head.url.clone();
    let (mut producer, consumer) = data_pipe::create(BODY_PIPE_CAPACITY);
    client.on_receive_response(head, consumer);
    let error = loop {
        let piece = match body.next().await {
            Ok(Some(piece)) => piece,
            Ok(None) => break None,
            Err(err) => break Some(err),
        };
        // A reader that went stops the load (Chromium's `URLLoader` fails it
        // as the body's consumer closes).
        if let Err(data_pipe::Closed) = producer.write(piece).await {
            break Some(NetError::new(
                NetErrorCode::Failed,
                format!("{url}: the body's reader went"),
            ));
        }
    };
    // The body is over: the reader finds the pipe's end before the end is
    // told.
    drop(producer);
    CompletionStatus {
        error,
        encoded_body_length: body.encoded_length(),
        completion_time: TimeTicks::now(),
    }
}

fn log_failure(request: &ResourceRequest, err: &NetError) {
    tracing::debug!(url = %request.url, code = err.code.name(), detail = %err.detail, "load failed");
}
