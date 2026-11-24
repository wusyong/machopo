//! The renderer's requests to the browser's network (Blink's
//! `ResourceFetcher` handing loads to the frame's `URLLoaderFactory`): their
//! ids, unique in the renderer, so a response for a document that has gone
//! is recognised and dropped. And the blob URLs the renderer's realms
//! registered (`BlobUrls`). And which responses no classic script is, and
//! what a `fetch()` sends and gets, a window's or a worker's.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use base::SequenceNumber;
use dom_api::FrameId;
use protocol::control_channel::BrowserSender;
use protocol::network::{
    CredentialsMode, LoadFlags, LoadResult, RedirectMode, RequestDestination, RequestMode,
    ResourceRequest,
};
use protocol::renderer::RendererHostMsg;

/// What DevTools calls `document`'s loader (`loaderId`): the navigation
/// that brought it; for one none brought (a child frame's first
/// `about:blank`, a `srcdoc` or `javascript:` URL's), the document's
/// number.
pub(crate) fn devtools_loader_id(
    loader: Option<dom::DocumentLoader>,
    document: dom::NodeId,
) -> String {
    loader.and_then(|loader| loader.navigation).map_or_else(
        || document.get().to_string(),
        |navigation| navigation.to_string(),
    )
}

thread_local! {
    /// The renderer's next request id: each renderer's thread its own, so
    /// what the render thread sends and what the engine's load sink sends
    /// are numbered apart without sharing anything (Blink's
    /// `CreateUniqueIdentifier`, its process's). A `RefCell`: a
    /// `SequenceNumber` hands out the next by `&mut`, and no borrow of it
    /// outlives `next_request_id`.
    static NEXT_REQUEST_ID: RefCell<SequenceNumber<u64>> =
        const { RefCell::new(SequenceNumber::starting_at(1)) };
}

/// A new request id, unique in this renderer.
pub(crate) fn next_request_id() -> u64 {
    NEXT_REQUEST_ID.with_borrow_mut(SequenceNumber::get_next)
}

thread_local! {
    /// The renderer's number for its next navigation (`BeginNavigation`'s
    /// `id`): its thread's own, as `NEXT_REQUEST_ID` is, so what the render
    /// thread sends and what the engine's frame client sends are numbered
    /// apart without sharing anything.
    static NEXT_BEGIN_NAVIGATION_ID: RefCell<SequenceNumber<u64>> =
        const { RefCell::new(SequenceNumber::starting_at(1)) };
}

/// A new navigation number, unique in this renderer.
pub(crate) fn next_begin_navigation_id() -> u64 {
    NEXT_BEGIN_NAVIGATION_ID.with_borrow_mut(SequenceNumber::get_next)
}

/// The URL of each request the render thread sent, until it is answered,
/// which the page's log names a failed load by (a `fetch()`'s is the
/// DOM's, `Dom::sent_fetch`).
#[derive(Debug, Default)]
pub(crate) struct RequestUrls(HashMap<u64, String>);

impl RequestUrls {
    /// Request `request_id`, `request`, is being sent.
    pub(crate) fn sent(&mut self, request_id: u64, request: &ResourceRequest) {
        self.0.insert(request_id, request.url.clone());
    }

    /// Request `request_id` was answered: its URL, if it was sent.
    pub(crate) fn answered(&mut self, request_id: u64) -> Option<String> {
        self.0.remove(&request_id)
    }
}

/// The blob URLs this renderer's realms registered (Blink's
/// `PublicURLManager`), which the browser serves (`RegisterBlobUrl`). A
/// revoked one is revoked here at once, for the `fetch()`es after it; the
/// browser hears of it once the loads asked for before it went
/// (`flush_revoked`), so they get the blob (File API's URL parser resolves
/// a blob URL as it parses it).
#[derive(Debug, Clone)]
pub(crate) struct BlobUrls {
    to_browser: BrowserSender,
    state: Rc<RefCell<BlobUrlState>>,
}

#[derive(Debug, Default)]
struct BlobUrlState {
    /// The URLs registered and not revoked, by the frame whose realm
    /// registered each (its document at the time: the URL goes with the
    /// next).
    live: HashMap<String, FrameId>,
    /// Revoked since the browser last heard, in order.
    revoked: Vec<String>,
    /// Every one revoked.
    gone: HashSet<String>,
}

impl BlobUrls {
    pub(crate) fn new(to_browser: BrowserSender) -> Self {
        Self {
            to_browser,
            state: Rc::default(),
        }
    }

    /// Registers `url` for a blob of `mime_type` and `bytes`, which frame
    /// `owner`'s realm made: the browser hears now, before any load of it.
    pub(crate) fn register(
        &self,
        url: &str,
        owner: FrameId,
        mime_type: &str,
        bytes: std::sync::Arc<[u8]>,
    ) {
        self.state.borrow_mut().live.insert(url.to_owned(), owner);
        let _ = self.to_browser.send(RendererHostMsg::RegisterBlobUrl {
            worker: None,
            url: url.to_owned(),
            mime_type: mime_type.to_owned(),
            bytes: bytes.into(),
        });
    }

    /// Revokes `url`, if one of this renderer's realms registered it.
    pub(crate) fn revoke(&self, url: &str) {
        let mut state = self.state.borrow_mut();
        if state.live.remove(url).is_some() {
            state.revoked.push(url.to_owned());
            state.gone.insert(url.to_owned());
        }
    }

    /// Whether `url` (without its fragment) was one of this renderer's,
    /// revoked.
    pub(crate) fn is_revoked(&self, url: &url::Url) -> bool {
        let mut url = url.clone();
        url.set_fragment(None);
        self.state.borrow().gone.contains(url.as_str())
    }

    /// Revokes the URLs the DOM's `frame`'s realm made: its document
    /// went (another took its place, or the frame went).
    pub(crate) fn revoke_frame(&self, frame: FrameId) {
        let urls: Vec<String> = self
            .state
            .borrow()
            .live
            .iter()
            .filter(|(_, owner)| **owner == frame)
            .map(|(url, _)| url.clone())
            .collect();
        for url in urls {
            self.revoke(&url);
        }
    }

    /// Tells the browser of the URLs revoked since it last heard: after the
    /// loads the tasks that revoked them asked for.
    pub(crate) fn flush_revoked(&self) {
        let revoked = std::mem::take(&mut self.state.borrow_mut().revoked);
        for url in revoked {
            let _ = self.to_browser.send(RendererHostMsg::RevokeBlobUrl { url });
        }
    }
}

/// The network service's request for a `fetch()`'s.
pub(crate) fn fetch_resource_request(request: &dom::FetchRequest) -> ResourceRequest {
    let mut resource = ResourceRequest::new(&request.url, RequestDestination::Empty);
    resource.method.clone_from(&request.method);
    resource.headers.clone_from(&request.headers);
    resource.initiator = Some(request.initiator.clone());
    resource.body = request
        .body
        .as_ref()
        .map(|body| protocol::BigBuffer::copy_from_slice(body));
    resource.redirect = match request.redirect {
        dom::RequestRedirect::Follow => RedirectMode::Follow,
        dom::RequestRedirect::Error => RedirectMode::Error,
        dom::RequestRedirect::Manual => RedirectMode::Manual,
    };
    resource.mode = match request.mode {
        dom::RequestMode::SameOrigin => RequestMode::SameOrigin,
        dom::RequestMode::NoCors => RequestMode::NoCors,
        // A `Request` script makes is never `navigate`.
        dom::RequestMode::Cors | dom::RequestMode::Navigate => RequestMode::Cors,
    };
    resource.credentials = match request.credentials {
        dom::RequestCredentials::Omit => CredentialsMode::Omit,
        dom::RequestCredentials::SameOrigin => CredentialsMode::SameOrigin,
        dom::RequestCredentials::Include => CredentialsMode::Include,
    };
    resource.load_flags = cache_load_flags(request.cache);
    resource
}

/// The load flags of `request`, a subresource of a document whose load has
/// them use the HTTP cache as `mode` says (Blink's `ResourceFetcher` gives a
/// request of the default cache mode `FrameFetchContext::
/// ResourceRequestCachePolicy`): a request that says how keeps it; else past
/// the cache, or what is stored taken stale or not, as the document's load
/// does; else validated if it is conditional (its own `If-None-Match` and
/// the like).
pub(crate) fn subresource_load_flags(
    request: &ResourceRequest,
    mode: dom::SubresourceCacheMode,
) -> LoadFlags {
    if request.load_flags != LoadFlags::default() {
        return request.load_flags;
    }
    let mut flags = LoadFlags::default();
    match mode {
        dom::SubresourceCacheMode::Default => {
            flags.validate_cache = is_conditional(&request.headers)
        }
        dom::SubresourceCacheMode::BypassCache => flags.bypass_cache = true,
        dom::SubresourceCacheMode::ForceCache => flags.skip_cache_validation = true,
    }
    flags
}

/// `flags` with DevTools' cache disabled (Blink's `InspectorNetworkAgent::
/// PrepareRequest`): past the HTTP cache, whatever the request asked; one
/// that may only use the cache finds nothing there
/// (`kUnspecifiedForceCacheMiss`).
pub(crate) fn cache_disabled_load_flags(flags: LoadFlags) -> LoadFlags {
    LoadFlags {
        only_from_cache: flags.only_from_cache,
        bypass_cache: true,
        ..LoadFlags::default()
    }
}

/// Whether a request of `headers` validates a response of its own (Blink's
/// `ResourceRequest::IsConditional`).
fn is_conditional(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, _)| {
        [
            "if-unmodified-since",
            "if-modified-since",
            "if-none-match",
            "if-range",
            "if-match",
        ]
        .iter()
        .any(|conditional| name.eq_ignore_ascii_case(conditional))
    })
}

/// The HTTP cache's load flags of a request's cache mode (Blink's
/// `FetchCacheMode` of Fetch's mode, then `WebURLRequest::
/// GetLoadFlagsForWebURLRequest`): `no-store` neither reads nor keeps (and
/// says `no-cache`), `reload` doesn't read but keeps, `no-cache` validates,
/// `force-cache` uses a stale one, `only-if-cached` that and nothing else.
pub(crate) fn cache_load_flags(cache: dom::RequestCache) -> LoadFlags {
    let mut flags = LoadFlags::default();
    match cache {
        dom::RequestCache::Default => {}
        dom::RequestCache::NoStore => {
            flags.disable_cache = true;
            flags.bypass_cache = true;
        }
        dom::RequestCache::Reload => flags.bypass_cache = true,
        dom::RequestCache::NoCache => flags.validate_cache = true,
        dom::RequestCache::ForceCache => flags.skip_cache_validation = true,
        dom::RequestCache::OnlyIfCached => {
            flags.only_from_cache = true;
            flags.skip_cache_validation = true;
        }
    }
    flags
}

/// A `fetch()`'s response as the DOM takes it, or a network error, of
/// which script learns no more (Fetch's).
pub(crate) fn fetched_response(result: &LoadResult) -> Result<dom_api::FetchedResponse, ()> {
    match result {
        Ok(response) => Ok(dom_api::FetchedResponse {
            tainting: response.head.tainting,
            url: response.head.url.clone(),
            redirected: response.head.redirected,
            status: response.head.status,
            status_text: response.head.status_text.clone(),
            headers: response.head.headers.clone(),
            body: std::sync::Arc::from(&response.body[..]),
        }),
        Err(_) => Err(()),
    }
}

/// Whether `headers` (names lowercase) say `X-Content-Type-Options:
/// nosniff`: the first value's first item, without regard to ASCII case
/// (Fetch's "determine nosniff", Blink's `ParseContentTypeOptionsHeader`).
pub(crate) fn is_nosniff(headers: &[(String, String)]) -> bool {
    headers
        .iter()
        .find(|(name, _)| name == "x-content-type-options")
        .and_then(|(_, value)| value.split(',').next())
        .is_some_and(|first| first.trim().eq_ignore_ascii_case("nosniff"))
}

/// Whether a response of MIME type `essence` (lowercase) is blocked as a
/// classic script, a worker's or `importScripts()`'s too: `image/`,
/// `audio/`, `video/` and `text/csv`, which Fetch's "should response to
/// request be blocked due to its MIME type?" names; any other passes (Blink's
/// `AllowedByNosniff::MimeTypeAsScript`, lax for elements and workers).
pub(crate) fn is_blocked_as_script(essence: &str) -> bool {
    ["image/", "audio/", "video/"]
        .iter()
        .any(|prefix| essence.starts_with(prefix))
        || essence == "text/csv"
}

/// A module's response as HTML's "fetch a single module script" takes it:
/// none for a failed load (a network error, a CORS failure), a status
/// that isn't ok, or a MIME type that isn't JavaScript's; else its text,
/// UTF-8, and its URL after redirects (its base URL).
pub(crate) fn module_loaded(result: &LoadResult) -> Option<dom_api::module::FetchedModule> {
    let response = result.as_ref().ok()?;
    if !(200..300).contains(&response.head.status)
        || !dom_api::is_javascript_mime_type(&response.head.mime_type)
    {
        return None;
    }
    let (source, _) = encoding_rs::UTF_8.decode_with_bom_removal(&response.body);
    Some(dom_api::module::FetchedModule {
        url: url::Url::parse(&response.head.url).ok()?,
        source: source.into_owned(),
    })
}

/// The credentials mode of a script's request by CORS (a `crossorigin`
/// attribute's, a module worker's `credentials`).
pub(crate) fn script_credentials(cors: dom::ScriptCors) -> CredentialsMode {
    match cors {
        dom::ScriptCors::Anonymous => CredentialsMode::SameOrigin,
        dom::ScriptCors::UseCredentials => CredentialsMode::Include,
        dom::ScriptCors::Omit => CredentialsMode::Omit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_subresource_goes_by_its_documents_load_unless_it_says() {
        use dom::SubresourceCacheMode as Mode;
        let request = |headers: &[(&str, &str)]| {
            let mut request = ResourceRequest::new("https://a.test/i", RequestDestination::Image);
            request.headers = headers
                .iter()
                .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
                .collect();
            request
        };
        let plain = request(&[]);
        assert_eq!(
            subresource_load_flags(&plain, Mode::Default),
            LoadFlags::default()
        );
        let bypass = subresource_load_flags(&plain, Mode::BypassCache);
        assert!(bypass.bypass_cache && !bypass.skip_cache_validation);
        let force = subresource_load_flags(&plain, Mode::ForceCache);
        assert!(force.skip_cache_validation && !force.bypass_cache);
        // Its own validators: validated.
        for name in [
            "If-None-Match",
            "if-modified-since",
            "if-match",
            "if-range",
            "if-unmodified-since",
        ] {
            assert!(
                subresource_load_flags(&request(&[(name, "x")]), Mode::Default).validate_cache,
                "{name}"
            );
        }
        // One that says how keeps it.
        let mut no_store = plain.clone();
        no_store.load_flags = cache_load_flags(dom::RequestCache::NoStore);
        assert_eq!(
            subresource_load_flags(&no_store, Mode::ForceCache),
            no_store.load_flags
        );
    }

    #[test]
    fn devtools_cache_disabled_bypasses_it_or_misses() {
        let mut only = cache_load_flags(dom::RequestCache::OnlyIfCached);
        only = cache_disabled_load_flags(only);
        assert!(only.only_from_cache && only.bypass_cache && !only.skip_cache_validation);
        let force = cache_disabled_load_flags(cache_load_flags(dom::RequestCache::ForceCache));
        assert_eq!(
            force,
            LoadFlags {
                bypass_cache: true,
                ..LoadFlags::default()
            }
        );
    }

    #[test]
    fn a_fetchs_cache_mode_is_blinks_load_flags() {
        use dom::RequestCache;
        let flags = |set: fn(&mut LoadFlags)| {
            let mut flags = LoadFlags::default();
            set(&mut flags);
            flags
        };
        assert_eq!(
            cache_load_flags(RequestCache::Default),
            LoadFlags::default()
        );
        assert_eq!(
            cache_load_flags(RequestCache::NoStore),
            flags(|f| {
                f.disable_cache = true;
                f.bypass_cache = true;
            })
        );
        assert_eq!(
            cache_load_flags(RequestCache::Reload),
            flags(|f| f.bypass_cache = true)
        );
        assert_eq!(
            cache_load_flags(RequestCache::NoCache),
            flags(|f| f.validate_cache = true)
        );
        assert_eq!(
            cache_load_flags(RequestCache::ForceCache),
            flags(|f| f.skip_cache_validation = true)
        );
        assert_eq!(
            cache_load_flags(RequestCache::OnlyIfCached),
            flags(|f| {
                f.only_from_cache = true;
                f.skip_cache_validation = true;
            })
        );
    }

    /// A blob URL is registered with the browser at once, with the blob's
    /// type and bytes; a revoke is the renderer's at once (`is_revoked`,
    /// its fragment aside) and the browser's only once flushed; a URL not
    /// registered, or revoked already, isn't revoked again; a frame's URLs
    /// are revoked when its document goes, the other frames' kept.
    #[test]
    fn blob_urls_are_registered_and_revoked() {
        let (to_browser, from_renderer) = crossbeam_channel::unbounded();
        let urls = BlobUrls::new(to_browser.into());
        let bytes: std::sync::Arc<[u8]> = std::sync::Arc::from(&b"hi"[..]);
        urls.register("blob:null/1", FrameId(1), "a/b", bytes.clone());
        urls.register("blob:null/2", FrameId(2), "a/b", bytes.clone());
        let registered: Vec<(String, String, Vec<u8>)> = from_renderer
            .try_iter()
            .map(|msg| match msg {
                RendererHostMsg::RegisterBlobUrl {
                    worker: None,
                    url,
                    mime_type,
                    bytes,
                } => (url, mime_type, bytes.to_vec()),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            registered,
            [
                ("blob:null/1".into(), "a/b".into(), b"hi".to_vec()),
                ("blob:null/2".into(), "a/b".into(), b"hi".to_vec()),
            ]
        );
        let first: url::Url = "blob:null/1#x".parse().unwrap();
        assert!(!urls.is_revoked(&first));
        urls.revoke("blob:null/1");
        urls.revoke("blob:null/1");
        urls.revoke("blob:null/unknown");
        assert!(urls.is_revoked(&first));
        assert!(from_renderer.try_recv().is_err(), "not before the flush");
        urls.flush_revoked();
        let revoked = |rx: &crossbeam_channel::Receiver<RendererHostMsg>| -> Vec<String> {
            rx.try_iter()
                .map(|msg| match msg {
                    RendererHostMsg::RevokeBlobUrl { url } => url,
                    other => panic!("{other:?}"),
                })
                .collect()
        };
        assert_eq!(revoked(&from_renderer), ["blob:null/1"]);
        urls.register("blob:null/3", FrameId(1), "a/b", bytes.clone());
        urls.register("blob:null/4", FrameId(3), "a/b", bytes);
        let _ = from_renderer.try_iter().count();
        urls.revoke_frame(FrameId(3));
        urls.flush_revoked();
        assert_eq!(
            revoked(&from_renderer),
            ["blob:null/4"],
            "that frame's only"
        );
        assert!(urls.is_revoked(&"blob:null/4".parse().unwrap()));
        assert!(!urls.is_revoked(&"blob:null/2".parse().unwrap()));
    }
}
