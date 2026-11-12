//! Fetch's body mixin (https://fetch.spec.whatwg.org/#body-mixin, Blink's
//! `core/fetch/body.cc`), which `Request` and `Response` share: a body is
//! whole bytes, read at most once (no streams). "Extract" a body of what script gave, and "consume" one: its
//! bytes as text, JSON (parsed by the front end), a buffer, a view or a
//! blob, the promise resolved at once.

use std::sync::Arc;

use dom::{DomString, Request, Response};
use elise::Gc;
use protocol::header_list;

use crate::typed::{BodyArg, OpFront};
use crate::{ApiCx, ApiError};

/// What has a body: a `Request` or a `Response`.
#[derive(Clone, Copy)]
pub(crate) enum Body<'gc> {
    Request(Gc<'gc, Request>),
    Response(Gc<'gc, Response>),
}

impl<'gc> Body<'gc> {
    /// Whether it has a body (not a null one), and whether that was read.
    fn state(self, cx: &ApiCx<'_, 'gc>) -> (bool, bool) {
        match self {
            Body::Request(request) => {
                cx.with_request_data(request, |data| (data.body.is_some(), data.body_used))
            }
            Body::Response(response) => {
                cx.with_response_data(response, |data| (data.body.is_some(), data.body_used))
            }
        }
    }

    /// Marks the body read and gives its bytes; none for a null body.
    fn take(self, cx: &ApiCx<'_, 'gc>) -> Option<Arc<[u8]>> {
        let take = |body: &mut Option<Arc<[u8]>>, used: &mut bool| {
            *used = body.is_some();
            body.clone()
        };
        match self {
            Body::Request(request) => {
                cx.with_request_data(request, |data| take(&mut data.body, &mut data.body_used))
            }
            Body::Response(response) => {
                cx.with_response_data(response, |data| take(&mut data.body, &mut data.body_used))
            }
        }
    }

    /// Its `Content-Type`, the values joined.
    fn content_type(self, cx: &ApiCx<'_, 'gc>) -> String {
        let headers = match self {
            Body::Request(request) => cx.request_headers(request),
            Body::Response(response) => cx.response_headers(response),
        };
        cx.with_header_list(headers, |list| header_list::get(list, "Content-Type"))
            .unwrap_or_default()
    }
}

/// Fetch's "extract" a body (the parts without a stream): its bytes, and
/// the `Content-Type` it implies, if any (a buffer's has none, nor has a
/// blob of no type).
pub(crate) fn extract<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    body: &BodyArg<O::Node>,
) -> (Arc<[u8]>, Option<String>) {
    match body {
        BodyArg::Blob(blob) => {
            let data = cx.blob_data(op.object::<dom::Blob>(cx, blob));
            let content_type = (!data.type_().is_empty()).then(|| data.type_().to_owned());
            (data.shared_bytes(), content_type)
        }
        BodyArg::Bytes(bytes) => (bytes.as_slice().into(), None),
        BodyArg::SearchParams(params) => {
            let params = op.object::<dom::UrlSearchParams>(cx, params);
            let text = cx.with_search_params(params, |list| crate::url::serialize(list));
            (
                text.into_bytes().into(),
                Some("application/x-www-form-urlencoded;charset=UTF-8".to_owned()),
            )
        }
        BodyArg::String(text) => (
            text.as_bytes().into(),
            Some("text/plain;charset=UTF-8".to_owned()),
        ),
    }
}

/// `bodyUsed`: its body was read (Fetch's "disturbed").
pub(crate) fn body_used<'gc>(cx: &ApiCx<'_, 'gc>, body: Body<'gc>) -> bool {
    let (has_body, used) = body.state(cx);
    has_body && used
}

/// Fetch's "consume body", to its bytes: a body read already is a
/// `TypeError`; a response's whose `fetch()` was aborted rejects with the
/// reason (its stream errored); a null body is an empty one.
fn consume<'gc>(cx: &ApiCx<'_, 'gc>, body: Body<'gc>, member: &str) -> Result<Arc<[u8]>, ApiError> {
    if body_used(cx, body) {
        let interface = match body {
            Body::Request(_) => "Request",
            Body::Response(_) => "Response",
        };
        return Err(ApiError::type_error(format!(
            "Failed to execute '{member}' on '{interface}': body stream already read"
        )));
    }
    if let Body::Response(response) = body
        && body.state(cx).0
        && let Some(signal) = cx
            .response_signal(response)
            .filter(|&signal| cx.abort_reason(signal).is_some())
    {
        return Err(ApiError::aborted(cx, signal));
    }
    Ok(body.take(cx).unwrap_or_else(|| Arc::from(&[][..])))
}

/// Encoding's "UTF-8 decode": a leading BOM goes, an invalid sequence is
/// U+FFFD.
fn utf8_decode(bytes: &[u8]) -> DomString {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    DomString::new(&String::from_utf8_lossy(bytes))
}

/// `text()`.
pub(crate) fn text<'gc>(cx: &ApiCx<'_, 'gc>, body: Body<'gc>) -> Result<DomString, ApiError> {
    Ok(utf8_decode(&consume(cx, body, "text")?))
}

/// `json()`: the text, which the front end parses.
pub(crate) fn json<'gc>(cx: &ApiCx<'_, 'gc>, body: Body<'gc>) -> Result<DomString, ApiError> {
    Ok(utf8_decode(&consume(cx, body, "json")?))
}

/// `arrayBuffer()` and `bytes()`: a copy of the bytes, the buffer's.
pub(crate) fn bytes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    body: Body<'gc>,
    member: &str,
) -> Result<Vec<u8>, ApiError> {
    Ok(consume(cx, body, member)?.to_vec())
}

/// `blob()`: a new blob of the bytes (shared, not copied), its type the
/// `Content-Type` as Chrome has it: the header's value, lowercased (not
/// Fetch's parsed and serialized MIME type).
pub(crate) fn blob<'gc>(
    cx: &ApiCx<'_, 'gc>,
    body: Body<'gc>,
) -> Result<Gc<'gc, dom::Blob>, ApiError> {
    let type_ = crate::blob::normalize_type(&body.content_type(cx));
    let bytes = consume(cx, body, "blob")?;
    Ok(cx.create_blob(dom::BlobData::from_shared(bytes, type_)))
}
