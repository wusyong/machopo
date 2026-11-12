//! Fetch's `Response` (https://fetch.spec.whatwg.org/#response-class,
//! Blink's `core/fetch/response.cc`): its constructor,
//! `error()`, `redirect()`, `json()`, and its members.

use std::sync::Arc;

use dom::{DomString, HeadersGuard, Response, ResponseData, ResponseType, Window};
use elise::Gc;
use protocol::header_list;

use crate::body::{self, Body};
use crate::interfaces::ResponseInit;
use crate::typed::{BodyArg, DomStr, OpFront};
use crate::{ApiCx, ApiError};

/// Fetch's "null body status": a response of one has no body.
pub(crate) fn is_null_body_status(status: u16) -> bool {
    matches!(status, 101 | 103 | 204 | 205 | 304)
}

/// Fetch's "redirect status".
pub(crate) fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Whether `text` matches HTTP's `reason-phrase` (Fetch's check of
/// `statusText`): tabs, spaces, visible ASCII and `obs-text` (U+0080 to
/// U+00FF, as a `ByteString` has them).
fn is_reason_phrase(text: &str) -> bool {
    text.chars().all(|c| {
        matches!(c, '\t' | ' ') || c.is_ascii_graphic() || ('\u{80}'..='\u{ff}').contains(&c)
    })
}

/// Fetch's "initialize a response": the init's status, status text and
/// headers, then the body and the `Content-Type` it implies (if the
/// headers have none).
fn initialize<'gc>(
    cx: &ApiCx<'_, 'gc>,
    data: &mut ResponseData,
    headers: Gc<'gc, dom::Headers>,
    init: &ResponseInit,
    body: Option<(Arc<[u8]>, Option<String>)>,
    member: &str,
) -> Result<(), ApiError> {
    if !(200..=599).contains(&init.status) {
        return Err(ApiError::Range(format!(
            "{member}: The status provided ({}) is outside the range [200, 599].",
            init.status
        )));
    }
    if !is_reason_phrase(&init.statusText) {
        return Err(ApiError::type_error(format!(
            "{member}: Invalid statusText"
        )));
    }
    data.status = init.status;
    init.statusText.clone_into(&mut data.status_text);
    if let Some(init) = &init.headers {
        crate::headers::fill(cx, headers, &crate::headers::pairs_init(&init.0))?;
    }
    if let Some((bytes, content_type)) = body {
        if is_null_body_status(init.status) {
            return Err(ApiError::type_error(format!(
                "{member}: Response with null body status ({}) cannot have body",
                init.status
            )));
        }
        data.body = Some(bytes);
        if let Some(content_type) = content_type {
            cx.with_header_list(headers, |list| {
                if !header_list::contains(list, "Content-Type") {
                    header_list::append(list, "Content-Type", &content_type);
                }
            });
        }
    }
    Ok(())
}

/// `new Response(body, init)`: in the current realm.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    body: Option<&BodyArg>,
    init: &ResponseInit,
) -> Result<Gc<'gc, Response>, ApiError> {
    let window = crate::request::current_window(op, cx, "Response")?;
    let headers = cx.create_headers(HeadersGuard::Response);
    let body = body.map(|body| body::extract(op, cx, body));
    let mut data = ResponseData::default();
    initialize(
        cx,
        &mut data,
        headers,
        init,
        body,
        "Failed to construct 'Response'",
    )?;
    Ok(cx.create_response(window, data, headers))
}

/// `Response.error()`: a network error, with immutable headers.
pub(crate) fn error<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, Response>, ApiError> {
    let window = crate::request::current_window(op, cx, "Response")?;
    Ok(network_error(cx, window))
}

/// A network error of `window`'s realm.
pub(crate) fn network_error<'gc>(
    cx: &ApiCx<'_, 'gc>,
    window: Gc<'gc, Window>,
) -> Gc<'gc, Response> {
    let headers = cx.create_headers(HeadersGuard::Immutable);
    let data = ResponseData {
        response_type: ResponseType::Error,
        status: 0,
        ..ResponseData::default()
    };
    cx.create_response(window, data, headers)
}

/// `Response.redirect(url, status)`: `url` parsed against the current
/// realm's base URL, as `Location`; a status that isn't a redirect status
/// is a `RangeError`. Its headers are immutable.
pub(crate) fn redirect<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    url: &str,
    status: u16,
) -> Result<Gc<'gc, Response>, ApiError> {
    let window = crate::request::current_window(op, cx, "Response")?;
    let base = cx.document_base_url(cx.window_document(window));
    let parsed = url::Url::options()
        .base_url(base.as_ref())
        .parse(url)
        .map_err(|_| {
            ApiError::type_error(format!(
                "Failed to execute 'redirect' on 'Response': Failed to parse URL from {url}"
            ))
        })?;
    if !is_redirect_status(status) {
        return Err(ApiError::Range(format!(
            "Failed to execute 'redirect' on 'Response': Invalid status code ({status})"
        )));
    }
    let headers = cx.create_headers(HeadersGuard::Immutable);
    cx.with_header_list(headers, |list| {
        list.push(("Location".to_owned(), parsed.to_string()))
    });
    let data = ResponseData {
        status,
        ..ResponseData::default()
    };
    Ok(cx.create_response(window, data, headers))
}

/// `Response.json(data, init)`: `data` serialized as JSON (the front end
/// did, a `TypeError` for what JSON can't), its body, with
/// `Content-Type: application/json`.
pub(crate) fn json_static<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    data: &str,
    init: &ResponseInit,
) -> Result<Gc<'gc, Response>, ApiError> {
    let window = crate::request::current_window(op, cx, "Response")?;
    let headers = cx.create_headers(HeadersGuard::Response);
    let body = (
        Arc::from(data.as_bytes()),
        Some("application/json".to_owned()),
    );
    let mut response = ResponseData::default();
    initialize(
        cx,
        &mut response,
        headers,
        init,
        Some(body),
        "Failed to execute 'json' on 'Response'",
    )?;
    Ok(cx.create_response(window, response, headers))
}

fn string<'gc>(text: &str) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(text)))
}

pub(crate) fn response_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<DomStr<'gc>, ApiError> {
    string(
        match cx.with_response_data(this, |data| data.response_type) {
            ResponseType::Basic => "basic",
            ResponseType::Cors => "cors",
            ResponseType::Default => "default",
            ResponseType::Error => "error",
            ResponseType::Opaque => "opaque",
            ResponseType::OpaqueRedirect => "opaqueredirect",
        },
    )
}

/// `url`: its URL without its fragment; "" for one script made.
pub(crate) fn url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_response_data(this, |data| match &data.url {
        Some(url) => string(url.split_once('#').map_or(url.as_str(), |(url, _)| url)),
        None => string(""),
    })
}

pub(crate) fn redirected<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<bool, ApiError> {
    Ok(cx.with_response_data(this, |data| data.redirected))
}

pub(crate) fn status<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Response>) -> Result<u16, ApiError> {
    Ok(cx.with_response_data(this, |data| data.status))
}

/// `ok`: a status from 200 to 299.
pub(crate) fn ok<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Response>) -> Result<bool, ApiError> {
    Ok((200..=299).contains(&cx.with_response_data(this, |data| data.status)))
}

pub(crate) fn status_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_response_data(this, |data| string(&data.status_text))
}

pub(crate) fn headers<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<Gc<'gc, dom::Headers>, ApiError> {
    Ok(cx.response_headers(this))
}

pub(crate) fn body_used<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<bool, ApiError> {
    Ok(body::body_used(cx, Body::Response(this)))
}

/// `clone()`: a used body is a `TypeError`; else a new response of the
/// same data (the body's bytes shared) and headers (copied, the same
/// guard), and the same `fetch()`'s signal, in this one's realm.
pub(crate) fn clone<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<Gc<'gc, Response>, ApiError> {
    if body::body_used(cx, Body::Response(this)) {
        return Err(ApiError::type_error(
            "Failed to execute 'clone' on 'Response': Response body is already used",
        ));
    }
    let data = cx.with_response_data(this, |data| data.clone());
    let guard = cx.headers_guard(cx.response_headers(this));
    let headers = cx.create_headers(guard);
    let list = cx.with_header_list(cx.response_headers(this), |list| list.clone());
    cx.with_header_list(headers, |own| *own = list);
    let clone = cx.create_response(cx.response_window(this), data, headers);
    if let Some(signal) = cx.response_signal(this) {
        cx.set_response_signal(clone, signal);
    }
    Ok(clone)
}

pub(crate) fn text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<DomString, ApiError> {
    body::text(cx, Body::Response(this))
}

pub(crate) fn json<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<DomString, ApiError> {
    body::json(cx, Body::Response(this))
}

pub(crate) fn array_buffer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<Vec<u8>, ApiError> {
    body::bytes(cx, Body::Response(this), "arrayBuffer")
}

pub(crate) fn bytes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<Vec<u8>, ApiError> {
    body::bytes(cx, Body::Response(this), "bytes")
}

pub(crate) fn blob<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Response>,
) -> Result<Gc<'gc, dom::Blob>, ApiError> {
    body::blob(cx, Body::Response(this))
}
