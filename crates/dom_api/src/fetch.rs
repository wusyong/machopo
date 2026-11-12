//! Fetch's `fetch()` from a window (https://fetch.spec.whatwg.org/#fetch-method,
//! Blink's `global_fetch.cc` and `FetchManager`): a new `Request` of its
//! arguments, then a `FetchRecord` in the window, and its request queued
//! for the host (`FetchHostRequest::Start`), which the engine hands it as a
//! frame request once the task is over (so the request goes as the task
//! ends, not as `fetch()` is called, as Chrome's does). The response comes
//! back in a task of its own (`did_receive_response`). Another origin's
//! resource goes by CORS, in the network service (which a renderer can't
//! skip): the response comes filtered for its tainting.
//!
//! The request's signal aborts it (`abort_fetch`): the promise rejects
//! with the signal's reason and the host cancels the request, or, once the
//! response came, its body can't be read.

use std::sync::Arc;

use dom::{
    EventTargetImpl, FetchHostRequest, FetchRecord, FetchRequest, HeadersGuard, ObjectRef,
    RequestCredentials, RequestRedirect, ResponseData, ResponseType, Window,
};
use elise::{Gc, Persistent};
use protocol::header_list;
use protocol::network::{CredentialsMode, ResponseTainting, cors_filtered_headers};

use crate::interfaces::RequestInit;
use crate::typed::{OpFront, RequestInput, ScriptHost};
use crate::{ApiCx, ApiError};

/// A response from the network, for `did_receive_response`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedResponse {
    /// What it is to the request's origin, the network service having
    /// filtered it for that (an opaque one has nothing of its own).
    pub tainting: ResponseTainting,
    /// The final URL, after redirects.
    pub url: String,
    pub redirected: bool,
    pub status: u16,
    pub status_text: String,
    /// In order, as sent.
    pub headers: Vec<(String, String)>,
    pub body: Arc<[u8]>,
}

/// The `TypeError` a network error rejects with (Chrome's message).
fn failed_to_fetch() -> ApiError {
    ApiError::type_error("Failed to fetch")
}

/// `window.fetch(input, init)`: a new `Request` of them (its errors
/// reject), then the request queued for the host, if machopo may send it; the
/// promise's number, pending.
pub(crate) fn fetch<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    input: RequestInput<'_, 'gc>,
    init: &RequestInit,
) -> Result<u64, ApiError> {
    let request = crate::request::new_request(op, cx, this, input, init)?;
    let signal = cx.request_signal(request);
    // Aborted already: the promise rejects with the reason, nothing goes.
    if cx.abort_reason(signal).is_some() {
        return Err(ApiError::aborted(cx, signal));
    }
    let data = cx.with_request_data(request, |data| data.clone());
    let url = url::Url::parse(&data.url).map_err(|_| failed_to_fetch())?;
    match url.scheme() {
        "data" | "http" | "https" | "blob" => {}
        "file" => {
            return Err(ApiError::type_error(format!(
                "Failed to fetch: URL scheme \"file\" is not supported ({url})"
            )));
        }
        // A scheme fetch doesn't load.
        _ => return Err(failed_to_fetch()),
    }
    // Headers of one name go as one, their values joined, in the name's
    // first case (Blink's `FetchHeaderList` does the same for the network).
    let headers = cx.with_header_list(cx.request_headers(request), |list| {
        let mut headers: Vec<(String, String)> = Vec::new();
        for (name, _) in list.iter() {
            if !header_list::contains(&headers, name) {
                let value = header_list::get(list, name).expect("a name of the list");
                headers.push((name.clone(), value));
            }
        }
        headers
    });
    let document = cx.window_document(this);
    let initiator = match crate::frame::origin(cx, document) {
        Some(origin) if origin.is_tuple() => origin.ascii_serialization(),
        _ => "null".to_owned(),
    };
    let promise = cx.new_promise_number();
    let record = cx.add_fetch(this, signal, promise, data.redirect, data.credentials);
    // (Making the `Request` moved the input's body: script can't reach
    // this one.)
    cx.request_fetch(FetchHostRequest::Start(FetchRequest {
        id: cx.fetch_state(record).id,
        document: cx.node_id(document),
        url: data.url,
        method: data.method,
        headers,
        body: data.body,
        mode: data.mode,
        credentials: data.credentials,
        redirect: data.redirect,
        initiator,
    }));
    Ok(promise)
}

/// A `fetch()`'s signal aborted (Fetch's "abort the `fetch()` call"): if
/// its request still waits for the response, the host cancels it and the
/// promise rejects with the signal's reason. Once the response came, its
/// body can't be read instead (the body mixin checks the response's
/// signal).
pub(crate) fn abort_fetch<H: ScriptHost>(
    host: &mut H,
    record: &Persistent<FetchRecord>,
    signal: &Persistent<EventTargetImpl>,
) {
    let promise = host.mutate(|cx| {
        let record = record.get(cx.gc);
        if !cx.finish_fetch(record) {
            return None;
        }
        let state = cx.fetch_state(record);
        cx.request_fetch(FetchHostRequest::Cancel(state.id));
        Some(state.promise)
    });
    if let Some(promise) = promise {
        host.reject_promise_with_reason(promise, signal);
    }
}

/// The response to `window`'s fetch numbered `id` came (Fetch's "fetch
/// response handover"; `Err` a network error), in a task of its own in the
/// window's realm: its promise resolves with a new `Response` of its
/// tainting (a basic or CORS one: `Set-Cookie` filtered out, its headers
/// immutable, a null body for a null body status; a CORS one keeping the
/// headers the server exposes to a request of its credentials mode
/// (Blink's `FetchResponseData::CreateCorsFilteredResponse`); an opaque one
/// with nothing of its own; for a `manual` request's redirect, an
/// opaque-redirect one, with nothing of it but its URL), or a network
/// error rejects it with a `TypeError`. Nothing if it was aborted.
pub fn did_receive_response<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    id: u64,
    response: Result<FetchedResponse, ()>,
) {
    let settled = host.mutate(|cx| {
        let window = window.get(cx.gc);
        let record = cx.window_fetch(window, id)?;
        cx.finish_fetch(record);
        let state = cx.fetch_state(record);
        let signal = cx.fetch_signal(record);
        let Ok(response) = response else {
            return Some((state.promise, None));
        };
        let response = make_response(cx, window, state.redirect, state.credentials, response);
        if cx.with_response_data(response, |data| {
            matches!(data.response_type, ResponseType::Basic | ResponseType::Cors)
        }) {
            cx.set_response_signal(response, signal);
        }
        Some((
            state.promise,
            Some(cx.root_object(ObjectRef::Response(response))),
        ))
    });
    match settled {
        None => {}
        Some((promise, Some(response))) => host.resolve_promise_with(promise, &response),
        Some((promise, None)) => host.settle_promises(&[promise], Some(failed_to_fetch())),
    }
}

fn make_response<'gc>(
    cx: &ApiCx<'_, 'gc>,
    window: Gc<'gc, Window>,
    redirect: RequestRedirect,
    credentials: RequestCredentials,
    response: FetchedResponse,
) -> Gc<'gc, dom::Response> {
    let immutable = cx.create_headers(HeadersGuard::Immutable);
    if redirect == RequestRedirect::Manual && crate::response::is_redirect_status(response.status) {
        let data = ResponseData {
            response_type: ResponseType::OpaqueRedirect,
            url: Some(response.url),
            status: 0,
            ..ResponseData::default()
        };
        return cx.create_response(window, data, immutable);
    }
    // An opaque response: nothing of it but its type (no URL either).
    if response.tainting == ResponseTainting::Opaque {
        let data = ResponseData {
            response_type: ResponseType::Opaque,
            status: 0,
            ..ResponseData::default()
        };
        return cx.create_response(window, data, immutable);
    }
    let headers: Vec<(String, String)> = response
        .headers
        .into_iter()
        .filter(|(name, _)| !header_list::is_forbidden_response_header_name(name))
        .collect();
    let headers = match response.tainting {
        ResponseTainting::Cors => cors_filtered_headers(
            headers,
            match credentials {
                RequestCredentials::Omit => CredentialsMode::Omit,
                RequestCredentials::SameOrigin => CredentialsMode::SameOrigin,
                RequestCredentials::Include => CredentialsMode::Include,
            },
        ),
        _ => headers,
    };
    cx.with_header_list(immutable, |list| *list = headers);
    let data = ResponseData {
        response_type: match response.tainting {
            ResponseTainting::Cors => ResponseType::Cors,
            _ => ResponseType::Basic,
        },
        url: Some(response.url),
        redirected: response.redirected,
        status: response.status,
        status_text: response.status_text,
        body: (!crate::response::is_null_body_status(response.status)).then_some(response.body),
        body_used: false,
    };
    cx.create_response(window, data, immutable)
}
