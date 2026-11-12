//! Fetch's `Request` (https://fetch.spec.whatwg.org/#request-class, Blink's
//! `core/fetch/request.cc`): its constructor, which
//! `fetch()` runs too, and its members. machopo keeps `referrer`,
//! `referrerPolicy`, `cache`, `integrity`, `keepalive` and `priority` as
//! script gave them, checked, but acts on none of them: it sends no
//! `Referer`, and has no HTTP cache or SRI.

use dom::{
    DomString, EventTargetImpl, HeadersGuard, Referrer, Request, RequestCache, RequestCredentials,
    RequestData, RequestMode, RequestPriority, RequestRedirect, Window,
};
use elise::Gc;
use protocol::{header_list, network};
use url::Url;

use crate::body::{self, Body};
use crate::interfaces::RequestInit;
use crate::typed::{BodyMember, DomStr, OpFront, RequestInput, SignalMember};
use crate::{ApiCx, ApiError, DomException, ExceptionName};

fn construct_error(message: impl std::fmt::Display) -> ApiError {
    ApiError::type_error(format!("Failed to construct 'Request': {message}"))
}

/// The enum's value of `value`, or WebIDL's `TypeError` (`name` the enum's).
fn enum_value<T: Copy>(value: &str, name: &str, values: &[(&str, T)]) -> Result<T, ApiError> {
    values
        .iter()
        .find(|(text, _)| *text == value)
        .map(|&(_, value)| value)
        .ok_or_else(|| {
            construct_error(format!(
                "The provided value '{value}' is not a valid enum value of type {name}."
            ))
        })
}

const MODES: &[(&str, RequestMode)] = &[
    ("navigate", RequestMode::Navigate),
    ("same-origin", RequestMode::SameOrigin),
    ("no-cors", RequestMode::NoCors),
    ("cors", RequestMode::Cors),
];
const CREDENTIALS: &[(&str, RequestCredentials)] = &[
    ("omit", RequestCredentials::Omit),
    ("same-origin", RequestCredentials::SameOrigin),
    ("include", RequestCredentials::Include),
];
const CACHES: &[(&str, RequestCache)] = &[
    ("default", RequestCache::Default),
    ("no-store", RequestCache::NoStore),
    ("reload", RequestCache::Reload),
    ("no-cache", RequestCache::NoCache),
    ("force-cache", RequestCache::ForceCache),
    ("only-if-cached", RequestCache::OnlyIfCached),
];
const REDIRECTS: &[(&str, RequestRedirect)] = &[
    ("follow", RequestRedirect::Follow),
    ("error", RequestRedirect::Error),
    ("manual", RequestRedirect::Manual),
];
const PRIORITIES: &[(&str, RequestPriority)] = &[
    ("high", RequestPriority::High),
    ("low", RequestPriority::Low),
    ("auto", RequestPriority::Auto),
];
/// Referrer Policy's values, and "" (none).
const REFERRER_POLICIES: &[(&str, ())] = &[
    ("", ()),
    ("no-referrer", ()),
    ("no-referrer-when-downgrade", ()),
    ("same-origin", ()),
    ("origin", ()),
    ("strict-origin", ()),
    ("origin-when-cross-origin", ()),
    ("strict-origin-when-cross-origin", ()),
    ("unsafe-url", ()),
];

fn text_of<T: PartialEq + Copy>(values: &[(&'static str, T)], value: T) -> &'static str {
    values
        .iter()
        .find(|(_, other)| *other == value)
        .map_or("", |(text, _)| text)
}

/// Fetch's "normalize" a method: the six it knows, uppercase.
fn normalize_method(method: &str) -> String {
    const KNOWN: &[&str] = &["DELETE", "GET", "HEAD", "OPTIONS", "POST", "PUT"];
    KNOWN
        .iter()
        .find(|known| known.eq_ignore_ascii_case(method))
        .map_or_else(|| method.to_owned(), |known| (*known).to_owned())
}

pub(crate) fn current_window<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    what: &str,
) -> Result<Gc<'gc, Window>, ApiError> {
    op.current_window(cx).ok_or_else(|| {
        ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: format!("no realm to make a {what} in"),
        })
    })
}

/// `new Request(input, init)`: in the current realm.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    input: RequestInput<'_, 'gc>,
    init: &RequestInit<O::Node>,
) -> Result<Gc<'gc, Request>, ApiError> {
    let window = current_window(op, cx, "Request")?;
    new_request(op, cx, window, input, init)
}

/// Fetch's `Request` constructor steps, in `window`'s realm (its document's
/// base URL and origin), which `fetch()` runs too.
pub(crate) fn new_request<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    window: Gc<'gc, Window>,
    input: RequestInput<'_, 'gc>,
    init: &RequestInit<O::Node>,
) -> Result<Gc<'gc, Request>, ApiError> {
    let document = cx.window_document(window);
    let base = cx.document_base_url(document);
    let origin = crate::frame::origin(cx, document);
    // The request it starts from, its signal, and the mode if init has none.
    let (mut data, input_request, mut signal, fallback_mode) = match input {
        RequestInput::Url(url) => {
            let parsed = Url::options()
                .base_url(base.as_ref())
                .parse(url)
                .map_err(|_| construct_error(format!("Failed to parse URL from {url}")))?;
            if !parsed.username().is_empty() || parsed.password().is_some() {
                return Err(construct_error(format!(
                    "Request cannot be constructed from a URL that includes credentials: {url}"
                )));
            }
            let data = RequestData {
                url: parsed.into(),
                ..RequestData::default()
            };
            (data, None, None, Some(RequestMode::Cors))
        }
        RequestInput::Request(request) => {
            let data = cx.with_request_data(request, |data| data.clone());
            (data, Some(request), Some(cx.request_signal(request)), None)
        }
    };
    let init_given = init.method.is_some()
        || init.headers.is_some()
        || init.body.is_some()
        || init.referrer.is_some()
        || init.referrerPolicy.is_some()
        || init.mode.is_some()
        || init.credentials.is_some()
        || init.cache.is_some()
        || init.redirect.is_some()
        || init.integrity.is_some()
        || init.keepalive.is_some()
        || init.signal.is_some()
        || init.duplex.is_some()
        || init.priority.is_some()
        || init.window.is_some();
    if init.window.is_some_and(|window| !window.0) {
        return Err(construct_error("'window' must be null"));
    }
    if init_given {
        if data.mode == RequestMode::Navigate {
            data.mode = RequestMode::SameOrigin;
        }
        data.referrer = Referrer::Client;
        data.referrer_policy = String::new();
    }
    if let Some(referrer) = &init.referrer {
        data.referrer = if referrer.is_empty() {
            Referrer::NoReferrer
        } else {
            let parsed = Url::options()
                .base_url(base.as_ref())
                .parse(referrer)
                .map_err(|_| {
                    construct_error(format!("Referrer '{referrer}' is not a valid URL."))
                })?;
            let same_origin = origin
                .as_ref()
                .is_some_and(|origin| origin.is_tuple() && parsed.origin() == *origin);
            if (parsed.scheme() == "about" && parsed.path() == "client") || !same_origin {
                Referrer::Client
            } else {
                Referrer::Url(parsed.into())
            }
        };
    }
    if let Some(policy) = &init.referrerPolicy {
        enum_value(policy, "ReferrerPolicy", REFERRER_POLICIES)?;
        policy.clone_into(&mut data.referrer_policy);
    }
    let mode = match &init.mode {
        Some(mode) => Some(enum_value(mode, "RequestMode", MODES)?),
        None => fallback_mode,
    };
    if mode == Some(RequestMode::Navigate) {
        return Err(construct_error(
            "Cannot construct a Request with a RequestInit whose mode member is set as 'navigate'.",
        ));
    }
    if let Some(mode) = mode {
        data.mode = mode;
    }
    if let Some(credentials) = &init.credentials {
        data.credentials = enum_value(credentials, "RequestCredentials", CREDENTIALS)?;
    }
    if let Some(cache) = &init.cache {
        data.cache = enum_value(cache, "RequestCache", CACHES)?;
    }
    if data.cache == RequestCache::OnlyIfCached && data.mode != RequestMode::SameOrigin {
        return Err(construct_error(
            "'only-if-cached' can be set only with 'same-origin' mode",
        ));
    }
    if let Some(redirect) = &init.redirect {
        data.redirect = enum_value(redirect, "RequestRedirect", REDIRECTS)?;
    }
    if let Some(integrity) = &init.integrity {
        integrity.clone_into(&mut data.integrity);
    }
    if let Some(keepalive) = init.keepalive {
        data.keepalive = keepalive;
    }
    if let Some(method) = &init.method {
        if method.chars().any(|c| u32::from(c) > 0xFF) || !header_list::is_header_name(method) {
            return Err(construct_error(format!(
                "'{method}' is not a valid HTTP method."
            )));
        }
        if network::is_forbidden_method(method) {
            return Err(construct_error(format!(
                "'{method}' HTTP method is unsupported."
            )));
        }
        data.method = normalize_method(method);
    }
    match init.signal {
        Some(SignalMember::Null) => signal = None,
        Some(SignalMember::Signal(ref object)) => {
            let given = op.object::<EventTargetImpl>(cx, object);
            if !cx.is_abort_signal(given) {
                return Err(construct_error(
                    "Failed to read the 'signal' property from 'RequestInit': Failed to convert value to 'AbortSignal'.",
                ));
            }
            signal = Some(given);
        }
        None => {}
    }
    if let Some(duplex) = &init.duplex {
        enum_value(duplex, "RequestDuplex", &[("half", ())])?;
    }
    if let Some(priority) = &init.priority {
        data.priority = enum_value(priority, "RequestPriority", PRIORITIES)?;
    }
    // A dependent of the signal it was given, if any (so aborting one
    // aborts the other, but not back).
    let sources: Vec<Gc<'gc, EventTargetImpl>> = signal.into_iter().collect();
    let signal = cx.create_dependent_abort_signal(window, &sources);
    let headers = cx.create_headers(HeadersGuard::Request);
    if let Some(input) = input_request {
        let list = cx.with_header_list(cx.request_headers(input), |list| list.clone());
        cx.with_header_list(headers, |own| *own = list);
    }
    if data.mode == RequestMode::NoCors {
        if !network::is_cors_safelisted_method(&data.method) {
            return Err(construct_error(format!(
                "'{}' is unsupported in no-cors mode.",
                data.method
            )));
        }
        cx.set_headers_guard(headers, HeadersGuard::RequestNoCors);
    }
    if init_given {
        let list = cx.with_header_list(headers, std::mem::take);
        match &init.headers {
            Some(init) => crate::headers::fill(cx, headers, &crate::headers::pairs_init(&init.0))?,
            None => {
                for (name, value) in list {
                    crate::headers::append_to(cx, headers, &name, &value)?;
                }
            }
        }
    }
    let input_body = data.body.take();
    let init_body = match &init.body {
        Some(BodyMember::Body(body)) => Some(body),
        Some(BodyMember::Null) | None => None,
    };
    if (init_body.is_some() || input_body.is_some())
        && matches!(data.method.as_str(), "GET" | "HEAD")
    {
        return Err(construct_error(
            "Request with GET/HEAD method cannot have body.",
        ));
    }
    if let Some(init_body) = init_body {
        let (bytes, content_type) = body::extract(op, cx, init_body);
        data.body = Some(bytes);
        if let Some(content_type) = content_type {
            cx.with_header_list(headers, |list| {
                if !header_list::contains(list, "Content-Type") {
                    header_list::append(list, "Content-Type", &content_type);
                }
            });
        }
    } else if let Some(input_body) = input_body {
        let input = input_request.expect("a body came from a request");
        if body::body_used(cx, Body::Request(input)) {
            return Err(construct_error(
                "Cannot construct a Request with a Request object that has already been used.",
            ));
        }
        // The input's body moves to the new request: the input is used.
        cx.with_request_data(input, |data| data.body_used = true);
        data.body = Some(input_body);
    }
    data.body_used = false;
    Ok(cx.create_request(window, data, headers, signal))
}

fn string<'gc>(text: &str) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(text)))
}

pub(crate) fn method<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_request_data(this, |data| string(&data.method))
}

pub(crate) fn url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_request_data(this, |data| string(&data.url))
}

pub(crate) fn headers<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<Gc<'gc, dom::Headers>, ApiError> {
    Ok(cx.request_headers(this))
}

/// `destination`: a `Request` script makes has none.
pub(crate) fn destination<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    string("")
}

/// `referrer`: `about:client`, "" for none, or the URL.
pub(crate) fn referrer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_request_data(this, |data| match &data.referrer {
        Referrer::NoReferrer => string(""),
        Referrer::Client => string("about:client"),
        Referrer::Url(url) => string(url),
    })
}

pub(crate) fn referrer_policy<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_request_data(this, |data| string(&data.referrer_policy))
}

pub(crate) fn mode<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    string(text_of(MODES, cx.with_request_data(this, |data| data.mode)))
}

pub(crate) fn credentials<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    string(text_of(
        CREDENTIALS,
        cx.with_request_data(this, |data| data.credentials),
    ))
}

pub(crate) fn cache<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    string(text_of(
        CACHES,
        cx.with_request_data(this, |data| data.cache),
    ))
}

pub(crate) fn redirect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    string(text_of(
        REDIRECTS,
        cx.with_request_data(this, |data| data.redirect),
    ))
}

pub(crate) fn integrity<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    cx.with_request_data(this, |data| string(&data.integrity))
}

pub(crate) fn keepalive<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<bool, ApiError> {
    Ok(cx.with_request_data(this, |data| data.keepalive))
}

/// `isReloadNavigation`, `isHistoryNavigation`: no request script makes is
/// one.
pub(crate) fn not_navigation<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Request>,
) -> Result<bool, ApiError> {
    Ok(false)
}

pub(crate) fn signal<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    Ok(cx.request_signal(this))
}

/// `duplex`: always `half`.
pub(crate) fn duplex<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, Request>,
) -> Result<DomStr<'gc>, ApiError> {
    string("half")
}

/// `clone()`: a used body is a `TypeError`; else a new request of the
/// same data (the body's bytes shared), headers copied, a signal that
/// follows this one's, in this one's realm.
pub(crate) fn clone<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<Gc<'gc, Request>, ApiError> {
    if body::body_used(cx, Body::Request(this)) {
        return Err(ApiError::type_error(
            "Failed to execute 'clone' on 'Request': Request body is already used",
        ));
    }
    let window = cx.request_window(this);
    let data = cx.with_request_data(this, |data| data.clone());
    let guard = cx.headers_guard(cx.request_headers(this));
    let headers = cx.create_headers(guard);
    let list = cx.with_header_list(cx.request_headers(this), |list| list.clone());
    cx.with_header_list(headers, |own| *own = list);
    let signal = cx.create_dependent_abort_signal(window, &[cx.request_signal(this)]);
    Ok(cx.create_request(window, data, headers, signal))
}

pub(crate) fn body_used<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<bool, ApiError> {
    Ok(body::body_used(cx, Body::Request(this)))
}

pub(crate) fn text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomString, ApiError> {
    body::text(cx, Body::Request(this))
}

pub(crate) fn json<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<DomString, ApiError> {
    body::json(cx, Body::Request(this))
}

pub(crate) fn array_buffer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<Vec<u8>, ApiError> {
    body::bytes(cx, Body::Request(this), "arrayBuffer")
}

pub(crate) fn bytes<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Request>) -> Result<Vec<u8>, ApiError> {
    body::bytes(cx, Body::Request(this), "bytes")
}

pub(crate) fn blob<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Request>,
) -> Result<Gc<'gc, dom::Blob>, ApiError> {
    body::blob(cx, Body::Request(this))
}
