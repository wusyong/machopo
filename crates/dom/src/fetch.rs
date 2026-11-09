//! Fetch's `Request` (Blink's `core/fetch/request.h`, with its
//! `FetchRequestData`) and `Response` (`response.h`, with its
//! `FetchResponseData`): what the DOM keeps of them, and a `fetch()`
//! in flight (`FetchRecord`, which its window keeps until its response
//! comes or it is aborted; Blink's `FetchManager::Loader`). A body is whole
//! bytes, shared (`Arc<[u8]>`, as a `Blob`'s), read at most once (no
//! streams). Fetch's algorithms are `dom_api`'s.

use std::cell::Cell;
use std::sync::Arc;

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member};

use crate::{Cx, EventTargetImpl, Headers, Window, sites};

/// Fetch's response type (`Response.type`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResponseType {
    Basic,
    Cors,
    #[default]
    Default,
    Error,
    Opaque,
    OpaqueRedirect,
}

/// A response's data (Fetch's response, the parts `Response` exposes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseData {
    pub response_type: ResponseType,
    /// Its URL list's last entry, serialized; none for a response made by
    /// script (an empty URL list).
    pub url: Option<String>,
    /// Whether its URL list has more than one entry.
    pub redirected: bool,
    pub status: u16,
    pub status_text: String,
    /// Its body's bytes; none for a null body.
    pub body: Option<Arc<[u8]>>,
    /// Whether its body was read (Fetch's "disturbed").
    pub body_used: bool,
}

impl Default for ResponseData {
    /// Fetch's new response: status 200, a null body.
    fn default() -> Self {
        Self {
            response_type: ResponseType::Default,
            url: None,
            redirected: false,
            status: 200,
            status_text: String::new(),
            body: None,
            body_used: false,
        }
    }
}

/// Fetch's request mode (`Request.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestMode {
    Navigate,
    SameOrigin,
    #[default]
    NoCors,
    Cors,
}

/// Fetch's request credentials mode (`Request.credentials`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestCredentials {
    Omit,
    #[default]
    SameOrigin,
    Include,
}

/// Fetch's request cache mode (`Request.cache`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestCache {
    #[default]
    Default,
    NoStore,
    Reload,
    NoCache,
    ForceCache,
    OnlyIfCached,
}

/// Fetch's request redirect mode (`Request.redirect`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestRedirect {
    #[default]
    Follow,
    Error,
    Manual,
}

/// Fetch's request priority (`RequestInit.priority`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RequestPriority {
    High,
    Low,
    #[default]
    Auto,
}

/// Fetch's request referrer.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Referrer {
    NoReferrer,
    /// The client's (`about:client`): its document's URL, when sent.
    #[default]
    Client,
    Url(String),
}

/// A request's data (Fetch's request, the parts `Request` exposes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestData {
    /// Normalized, a `ByteString`.
    pub method: String,
    /// Its current URL, serialized.
    pub url: String,
    pub referrer: Referrer,
    /// Referrer Policy's value, as script gave it ("" for none).
    pub referrer_policy: String,
    pub mode: RequestMode,
    pub credentials: RequestCredentials,
    pub cache: RequestCache,
    pub redirect: RequestRedirect,
    pub integrity: String,
    pub keepalive: bool,
    pub priority: RequestPriority,
    /// Its body's bytes; none for a null body.
    pub body: Option<Arc<[u8]>>,
    /// Whether its body was read (Fetch's "disturbed").
    pub body_used: bool,
}

impl Default for RequestData {
    /// A `GET` of no URL: what a `Request` holds while it is read.
    fn default() -> Self {
        Self {
            method: "GET".to_owned(),
            url: String::new(),
            referrer: Referrer::Client,
            referrer_policy: String::new(),
            mode: RequestMode::NoCors,
            credentials: RequestCredentials::SameOrigin,
            cache: RequestCache::Default,
            redirect: RequestRedirect::Follow,
            integrity: String::new(),
            keepalive: false,
            priority: RequestPriority::Auto,
            body: None,
            body_used: false,
        }
    }
}

#[derive(GcObject)]
pub struct Request {
    /// Its relevant global: the window of the realm that made it.
    window: Member<Window>,
    /// Its `headers` and `signal`: made with it, never changed.
    headers: Member<Headers>,
    signal: Member<EventTargetImpl>,
    /// Taken out while it is read or changed, and put back.
    data: LocalExternal<Cell<RequestData>>,
    wrapper: JsEdge,
}

#[derive(GcObject)]
pub struct Response {
    window: Member<Window>,
    headers: Member<Headers>,
    /// The signal of the `fetch()` it came from, if it did: once that
    /// aborts, its body can't be read (Blink errors the body's stream).
    signal: Member<EventTargetImpl>,
    data: LocalExternal<Cell<ResponseData>>,
    wrapper: JsEdge,
}

/// A `fetch()` in flight: its window (its request's realm's), its number
/// (what the host's request and its response go by), its promise's number,
/// its request's signal, and what its response is made with (its redirect
/// mode and credentials mode).
#[derive(GcObject)]
pub struct FetchRecord {
    window: Member<Window>,
    signal: Member<EventTargetImpl>,
    id: Cell<u64>,
    promise: Cell<u64>,
    redirect: Cell<u8>,
    credentials: Cell<u8>,
}

/// A `fetch()`'s request for the host to send (Fetch's "fetch", the parts
/// the network takes): its number, its document's (the one whose window
/// fetches), and the request; the body as the request had it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    pub id: u64,
    pub document: crate::NodeId,
    pub url: String,
    pub method: String,
    /// One per name, the values joined (Blink's `FetchHeaderList` sends
    /// them so).
    pub headers: Vec<(String, String)>,
    pub body: Option<Arc<[u8]>>,
    pub mode: RequestMode,
    pub credentials: RequestCredentials,
    pub redirect: RequestRedirect,
    /// The document's origin, serialized (`null` for an opaque one).
    pub initiator: String,
}

/// What the host is to do with `fetch()`'s requests, in order: the DOM's
/// queue, which the engine takes (`Cx::take_fetch_requests`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchHostRequest {
    Start(FetchRequest),
    /// The request of the fetch the number names, aborted: cancelled, if
    /// it went.
    Cancel(u64),
}

/// What a `FetchRecord` says of its fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchState {
    pub id: u64,
    pub promise: u64,
    pub redirect: RequestRedirect,
    pub credentials: RequestCredentials,
}

const REDIRECTS: [RequestRedirect; 3] = [
    RequestRedirect::Follow,
    RequestRedirect::Error,
    RequestRedirect::Manual,
];
const CREDENTIALS: [RequestCredentials; 3] = [
    RequestCredentials::Omit,
    RequestCredentials::SameOrigin,
    RequestCredentials::Include,
];

impl<'gc> Cx<'_, 'gc> {
    /// A new `Request` of `window`'s realm.
    pub fn create_request(
        &self,
        window: Gc<'gc, Window>,
        data: RequestData,
        headers: Gc<'gc, Headers>,
        signal: Gc<'gc, EventTargetImpl>,
    ) -> Gc<'gc, Request> {
        self.gc.alloc_in(
            &sites::OTHER,
            RequestInit {
                window: Some(window),
                headers: Some(headers),
                signal: Some(signal),
                data: LocalExternal::new(Cell::new(data)),
                wrapper: (),
            },
        )
    }

    /// `f` with `request`'s data, which it may change.
    pub fn with_request_data<R>(
        &self,
        request: Gc<'gc, Request>,
        f: impl FnOnce(&mut RequestData) -> R,
    ) -> R {
        let cell = &*request.into_ref().data;
        let mut data = cell.take();
        let result = f(&mut data);
        cell.set(data);
        result
    }

    pub fn request_headers(&self, request: Gc<'gc, Request>) -> Gc<'gc, Headers> {
        request.headers.get(self.gc).expect("a request's headers")
    }

    pub fn request_signal(&self, request: Gc<'gc, Request>) -> Gc<'gc, EventTargetImpl> {
        request.signal.get(self.gc).expect("a request's signal")
    }

    pub fn request_window(&self, request: Gc<'gc, Request>) -> Gc<'gc, Window> {
        request.window.get(self.gc).expect("a request's window")
    }

    /// A new `Response` of `window`'s realm, of no `fetch()`.
    pub fn create_response(
        &self,
        window: Gc<'gc, Window>,
        data: ResponseData,
        headers: Gc<'gc, Headers>,
    ) -> Gc<'gc, Response> {
        self.gc.alloc_in(
            &sites::OTHER,
            ResponseInit {
                window: Some(window),
                headers: Some(headers),
                signal: None,
                data: LocalExternal::new(Cell::new(data)),
                wrapper: (),
            },
        )
    }

    /// `f` with `response`'s data, which it may change.
    pub fn with_response_data<R>(
        &self,
        response: Gc<'gc, Response>,
        f: impl FnOnce(&mut ResponseData) -> R,
    ) -> R {
        let cell = &*response.into_ref().data;
        let mut data = cell.take();
        let result = f(&mut data);
        cell.set(data);
        result
    }

    pub fn response_headers(&self, response: Gc<'gc, Response>) -> Gc<'gc, Headers> {
        response.headers.get(self.gc).expect("a response's headers")
    }

    pub fn response_window(&self, response: Gc<'gc, Response>) -> Gc<'gc, Window> {
        response.window.get(self.gc).expect("a response's window")
    }

    /// The signal of the `fetch()` `response` came from, if it did.
    pub fn response_signal(&self, response: Gc<'gc, Response>) -> Option<Gc<'gc, EventTargetImpl>> {
        response.signal.get(self.gc)
    }

    pub fn set_response_signal(
        &self,
        response: Gc<'gc, Response>,
        signal: Gc<'gc, EventTargetImpl>,
    ) {
        response.signal.set(self.gc, Some(signal));
    }

    /// A new `fetch()` of `window`'s in flight, its request's signal
    /// `signal`, its promise `promise`: its window keeps it, and the
    /// signal's abort algorithms have it. Its number is the DOM's next.
    pub fn add_fetch(
        &self,
        window: Gc<'gc, Window>,
        signal: Gc<'gc, EventTargetImpl>,
        promise: u64,
        redirect: RequestRedirect,
        credentials: RequestCredentials,
    ) -> Gc<'gc, FetchRecord> {
        let id = self.new_promise_number();
        let record = self.gc.alloc_in(
            &sites::OTHER,
            FetchRecordInit {
                window: Some(window),
                signal: Some(signal),
                id: Cell::new(id),
                promise: Cell::new(promise),
                redirect: Cell::new(index_of(&REDIRECTS, redirect)),
                credentials: Cell::new(index_of(&CREDENTIALS, credentials)),
            },
        );
        let fetches = match window.fetches.get(self.gc) {
            Some(fetches) => fetches,
            None => {
                let fetches = HeapVector::new(self.gc, 1);
                window.fetches.set(self.gc, Some(fetches));
                fetches
            }
        };
        fetches.push(self.gc, record);
        self.add_fetch_abort_algorithm(signal, record);
        record
    }

    pub fn fetch_state(&self, record: Gc<'gc, FetchRecord>) -> FetchState {
        FetchState {
            id: record.id.get(),
            promise: record.promise.get(),
            redirect: REDIRECTS[usize::from(record.redirect.get())],
            credentials: CREDENTIALS[usize::from(record.credentials.get())],
        }
    }

    pub fn fetch_window(&self, record: Gc<'gc, FetchRecord>) -> Gc<'gc, Window> {
        record.window.get(self.gc).expect("a fetch's window")
    }

    pub fn fetch_signal(&self, record: Gc<'gc, FetchRecord>) -> Gc<'gc, EventTargetImpl> {
        record.signal.get(self.gc).expect("a fetch's signal")
    }

    /// `window`'s fetch in flight numbered `id`, if it has it.
    pub fn window_fetch(&self, window: Gc<'gc, Window>, id: u64) -> Option<Gc<'gc, FetchRecord>> {
        let fetches = window.fetches.get(self.gc)?;
        (0..fetches.len())
            .filter_map(|index| fetches.get(self.gc, index))
            .find(|record| record.id.get() == id)
    }

    /// `record` no longer in flight (its response came, or it was
    /// aborted): its window lets it go, its signal's algorithms too.
    /// Whether it was in flight.
    pub fn finish_fetch(&self, record: Gc<'gc, FetchRecord>) -> bool {
        let window = self.fetch_window(record);
        let Some(fetches) = window.fetches.get(self.gc) else {
            return false;
        };
        let Some(index) = (0..fetches.len()).find(|&index| {
            fetches
                .get(self.gc, index)
                .is_some_and(|other| Gc::ptr_eq(other, record))
        }) else {
            return false;
        };
        fetches.remove(self.gc, index);
        self.remove_fetch_abort_algorithm(self.fetch_signal(record), record);
        true
    }

    /// A request for the host (`FetchHostRequest`), queued.
    pub fn request_fetch(&self, request: FetchHostRequest) {
        if let FetchHostRequest::Start(start) = &request {
            self.asked_for_load(crate::StackAsker::Fetch(start.id));
        }
        let state = &self.changes.frames.fetches;
        let mut requests = state.take();
        requests.push(request);
        state.set(requests);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | crate::Microtasks::FETCHES);
    }

    /// The `fetch()` requests queued since the last time, in order, taken.
    pub fn take_fetch_requests(&self) -> Vec<FetchHostRequest> {
        self.changes.frames.fetches.take()
    }

    pub(crate) fn request_wrapper_edge(&self, request: Gc<'gc, Request>) -> &'gc JsEdge {
        &request.into_ref().wrapper
    }

    pub(crate) fn response_wrapper_edge(&self, response: Gc<'gc, Response>) -> &'gc JsEdge {
        &response.into_ref().wrapper
    }
}

fn index_of<T: PartialEq>(all: &[T], value: T) -> u8 {
    let index = all
        .iter()
        .position(|other| *other == value)
        .expect("one of them");
    u8::try_from(index).expect("a few")
}
