//! Network service messages (`services/network/public/mojom`): what a load
//! asks for and what comes back. The browser uses them for navigations,
//! and the renderer's subresource requests carry them over IPC.

use std::net::SocketAddr;

use bytes::Bytes;
use ipc::TimeTicks;
use serde::{Deserialize, Serialize};

/// What a request is for (`network::mojom::RequestDestination`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestDestination {
    Document,
    /// An iframe's document, which the renderer loads itself.
    Iframe,
    Style,
    Image,
    Script,
    /// `fetch()`'s (Fetch's empty destination).
    Empty,
}

/// What a redirect does to a request (`network::mojom::RedirectMode`,
/// Fetch's request redirect mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RedirectMode {
    /// Followed, up to 20.
    Follow,
    /// A network error.
    Error,
    /// Not followed: the redirect response is the response, which the
    /// renderer makes an opaque-redirect one.
    Manual,
}

/// A request's mode (`network::mojom::RequestMode`, Fetch's request mode):
/// what it may do across origins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RequestMode {
    /// Only to its initiator's origin (or a `data:` URL).
    SameOrigin,
    /// Anywhere, but another origin's response is opaque.
    NoCors,
    /// Anywhere the other origin allows (CORS).
    Cors,
}

/// A request's credentials mode (`network::mojom::CredentialsMode`). machopo
/// has no cookies: it only changes what CORS asks of the other origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CredentialsMode {
    Omit,
    SameOrigin,
    Include,
}

/// What a response is to its request's initiator
/// (`network::mojom::FetchResponseType`, Fetch's response tainting), which
/// the network service decides; the renderer filters what script sees of
/// the response for it (an opaque `fetch()`'s body the network service
/// keeps back itself).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResponseTainting {
    /// Its own origin's, a `data:` URL's, or the browser's own request's:
    /// as it came.
    Basic,
    /// Another origin's, that origin allowing it: only the headers it
    /// exposes.
    Cors,
    /// Another origin's, without CORS: script sees nothing of it (a
    /// `fetch()`'s has no status, headers or body); a subresource's keeps
    /// its body for the renderer to use.
    Opaque,
}

/// `network::ResourceRequest`, trimmed to what we use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceRequest {
    pub url: String,
    /// Normalized (`GET`, `POST`...), a token, not a forbidden method.
    pub method: String,
    pub destination: RequestDestination,
    /// The headers script gave (`fetch()`'s), in order, none of them
    /// forbidden; the transport adds its own (`Accept` unless given,
    /// `User-Agent`).
    pub headers: Vec<(String, String)>,
    /// The origin of the document that asked (Chromium's
    /// `request_initiator`), serialized (`null` for an opaque one); `None`
    /// for one the browser makes itself, or a subresource that doesn't go
    /// by CORS. A request with one (`fetch()`'s, a module script's, one
    /// with `crossorigin`) goes by its `mode` and `credentials`.
    pub initiator: Option<String>,
    pub mode: RequestMode,
    pub credentials: CredentialsMode,
    /// Its body, whole; `None` for a null one.
    pub body: Option<Vec<u8>>,
    pub redirect: RedirectMode,
}

impl ResourceRequest {
    /// A `GET` of `url` for `destination`, with no headers of script's, no
    /// initiator and no body, following redirects.
    pub fn new(url: impl Into<String>, destination: RequestDestination) -> Self {
        Self {
            url: url.into(),
            method: "GET".to_owned(),
            destination,
            headers: Vec::new(),
            initiator: None,
            mode: RequestMode::NoCors,
            credentials: CredentialsMode::Include,
            body: None,
            redirect: RedirectMode::Follow,
        }
    }
}

/// Fetch's "forbidden method": `CONNECT`, `TRACE`, `TRACK`, without regard
/// to ASCII case.
pub fn is_forbidden_method(method: &str) -> bool {
    ["CONNECT", "TRACE", "TRACK"]
        .iter()
        .any(|forbidden| forbidden.eq_ignore_ascii_case(method))
}

/// Fetch's "forbidden request-header" (`name`, `value`): one script may not
/// set on a request (Chromium's `cors::IsForbiddenRequestHeader`, which the
/// renderer and the network service both check): the browser's own
/// (`Host`, `Cookie`, `Origin`...), anything `Proxy-` or `Sec-`, and a
/// method override naming a forbidden method. Names without regard to
/// ASCII case.
pub fn is_forbidden_request_header(name: &str, value: &str) -> bool {
    const NAMES: &[&str] = &[
        "accept-charset",
        "accept-encoding",
        "access-control-request-headers",
        "access-control-request-method",
        "connection",
        "content-length",
        "cookie",
        "cookie2",
        "date",
        "dnt",
        "expect",
        "host",
        "keep-alive",
        "origin",
        "referer",
        "set-cookie",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "via",
    ];
    let lower = name.to_ascii_lowercase();
    if NAMES.contains(&lower.as_str()) || lower.starts_with("proxy-") || lower.starts_with("sec-") {
        return true;
    }
    let is_http_whitespace = |c: char| matches!(c, '\t' | '\n' | '\r' | ' ');
    matches!(
        lower.as_str(),
        "x-http-method" | "x-http-method-override" | "x-method-override"
    ) && value
        .split(',')
        .any(|method| is_forbidden_method(method.trim_matches(is_http_whitespace)))
}

/// Fetch's "CORS-unsafe request-header byte": a control character but a
/// tab, or one of `"():<>?@[\\]{}` and DEL.
fn is_cors_unsafe_request_header_byte(c: char) -> bool {
    (c < '\u{20}' && c != '\t') || "\"():<>?@[\\]{}\u{7f}".contains(c)
}

/// Fetch's "parse a single range header value", without whitespace: the
/// start and end of `bytes=start-end`, either one possibly empty but not
/// both, and the start no greater than the end.
pub fn parse_single_range(value: &str) -> Option<(Option<u64>, Option<u64>)> {
    let (unit, range) = value.split_once('=')?;
    if !unit.eq_ignore_ascii_case("bytes") {
        return None;
    }
    let (start, end) = range.split_once('-')?;
    let number = |digits: &str| -> Option<Option<u64>> {
        if digits.is_empty() {
            return Some(None);
        }
        if !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(Some)
    };
    let (start, end) = (number(start)?, number(end)?);
    match (start, end) {
        (None, None) => None,
        (Some(start), Some(end)) if start > end => None,
        range => Some(range),
    }
}

/// Fetch's "CORS-safelisted request-header" (`name`, `value`): one a
/// cross-origin request may carry without a preflight (Chromium's
/// `cors::IsCorsSafelistedHeader`). Each character of `value` is a byte.
pub fn is_cors_safelisted_request_header(name: &str, value: &str) -> bool {
    if value.chars().count() > 128 {
        return false;
    }
    match name.to_ascii_lowercase().as_str() {
        "accept" => !value.contains(is_cors_unsafe_request_header_byte),
        "accept-language" | "content-language" => value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || " *,-.;=".contains(c)),
        "content-type" => {
            !value.contains(is_cors_unsafe_request_header_byte)
                && value.parse::<data_url::mime::Mime>().is_ok_and(|mime| {
                    matches!(
                        format!("{}/{}", mime.type_, mime.subtype).as_str(),
                        "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
                    )
                })
        }
        // Not a suffix range (`bytes=-500`).
        "range" => parse_single_range(value).is_some_and(|(start, _)| start.is_some()),
        _ => false,
    }
}

/// Fetch's "CORS-unsafe request-header names" of `headers`: those a
/// preflight has to ask the other origin for, lowercase, sorted, once
/// each. Every name counts once the safelisted values add up to more than
/// 1024 bytes.
pub fn cors_unsafe_request_header_names(headers: &[(String, String)]) -> Vec<String> {
    let mut unsafe_names = Vec::new();
    let mut potentially_unsafe = Vec::new();
    let mut safelisted_size: usize = 0;
    for (name, value) in headers {
        if is_cors_safelisted_request_header(name, value) {
            potentially_unsafe.push(name.to_ascii_lowercase());
            // Only compared with 1024: saturating doesn't change that.
            safelisted_size = safelisted_size.saturating_add(value.chars().count());
        } else {
            unsafe_names.push(name.to_ascii_lowercase());
        }
    }
    if safelisted_size > 1024 {
        unsafe_names.extend(potentially_unsafe);
    }
    unsafe_names.sort_unstable();
    unsafe_names.dedup();
    unsafe_names
}

/// Fetch's "no-CORS-safelisted request-header" (`name`, `value`): one
/// script may set on a `no-cors` request's headers.
pub fn is_no_cors_safelisted_request_header(name: &str, value: &str) -> bool {
    is_no_cors_safelisted_request_header_name(name)
        && is_cors_safelisted_request_header(name, value)
}

/// Fetch's "no-CORS-safelisted request-header name": `Accept`,
/// `Accept-Language`, `Content-Language`, `Content-Type`.
pub fn is_no_cors_safelisted_request_header_name(name: &str) -> bool {
    [
        "accept",
        "accept-language",
        "content-language",
        "content-type",
    ]
    .iter()
    .any(|safelisted| name.eq_ignore_ascii_case(safelisted))
}

/// Fetch's "privileged no-CORS request-header name": `Range`, which only
/// the browser may set on a `no-cors` request.
pub fn is_privileged_no_cors_request_header_name(name: &str) -> bool {
    name.eq_ignore_ascii_case("range")
}

/// Fetch's "CORS-safelisted response-header name": what a CORS-filtered
/// response keeps, besides the names the server exposes.
pub fn is_cors_safelisted_response_header_name(name: &str) -> bool {
    [
        "cache-control",
        "content-language",
        "content-length",
        "content-type",
        "expires",
        "last-modified",
        "pragma",
    ]
    .iter()
    .any(|safelisted| name.eq_ignore_ascii_case(safelisted))
}

/// Fetch's "CORS-filtered response"'s headers: the safelisted ones and
/// those `Access-Control-Expose-Headers` names (all, for `*`, unless the
/// request's credentials mode is `include`).
pub fn cors_filtered_headers(
    headers: Vec<(String, String)>,
    credentials: CredentialsMode,
) -> Vec<(String, String)> {
    let exposed: Vec<String> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("access-control-expose-headers"))
        .flat_map(|(_, value)| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect();
    let all = exposed.iter().any(|name| name == "*") && credentials != CredentialsMode::Include;
    headers
        .into_iter()
        .filter(|(name, _)| {
            is_cors_safelisted_response_header_name(name)
                || all
                || exposed
                    .iter()
                    .any(|exposed| exposed.eq_ignore_ascii_case(name))
        })
        .collect()
}

/// Fetch's "CORS-safelisted method": `GET`, `HEAD` or `POST`.
pub fn is_cors_safelisted_method(method: &str) -> bool {
    matches!(method, "GET" | "HEAD" | "POST")
}

/// `network::mojom::URLResponseHead` plus the whole body. Chromium streams
/// the body through a data pipe; here it arrives in one piece.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceResponse {
    /// The final URL, after redirects.
    pub url: String,
    /// The HTTP status; 200 for `file:`.
    pub status: u16,
    /// The status line's reason phrase (Fetch's status message): the
    /// status's canonical one for HTTP, "" for `file:` and `data:`.
    pub status_text: String,
    /// Whether it followed a redirect to get here (its URL list has more
    /// than one entry).
    pub redirected: bool,
    /// What it is to the initiator; its headers (and, for an opaque
    /// `fetch()`'s, everything) filtered for it.
    pub tainting: ResponseTainting,
    /// The `Content-Type` essence, lowercase (`text/html`); from the file
    /// extension for `file:`, sniffed when the server sent none.
    pub mime_type: String,
    /// The `charset` parameter of `Content-Type`, as sent.
    pub charset: Option<String>,
    /// Every response header in order, names lowercase; each byte of a
    /// value a character (Fetch's isomorphic decode).
    pub headers: Vec<(String, String)>,
    /// When the parts of the request it answers happened (the last one,
    /// after redirects); none for a response the renderer made up itself.
    pub timing: Option<LoadTiming>,
    /// The server's address, for HTTP (`URLResponseHead::remote_endpoint`).
    pub remote_address: Option<SocketAddr>,
    /// How it came, as DevTools names it (`connection_info`): `http/1.1`,
    /// `h2`, or the scheme (`file`, `data`, `blob`); empty for a response
    /// the renderer made up itself.
    pub protocol: String,
    /// The bytes its head took on the wire (`URLResponseHead`'s
    /// `encoded_data_length`), as HTTP/1 writes it (HTTP/2's compressed
    /// headers aren't counted as such); 0 when it didn't come over the
    /// network.
    pub encoded_headers_length: u64,
    /// The bytes its body took on the wire, before `Content-Encoding` was
    /// undone (`encoded_body_length`); 0 when it didn't come over the
    /// network.
    pub encoded_body_length: u64,
    /// Shared, not copied, with whoever else keeps it (DevTools' record of
    /// the response).
    pub body: Bytes,
}

/// A redirect a load reached and waits at until told to follow it
/// (`net::RedirectInfo`, with the redirect's `URLResponseHead`): the
/// response that redirects, without its body, and the request it leads to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RedirectInfo {
    /// Where the next request goes.
    pub new_url: String,
    /// The next request's method: a `POST` may become a `GET`.
    pub new_method: String,
    /// The redirect itself, as the server sent it (not filtered for the
    /// initiator), with an empty body.
    pub response: ResourceResponse,
}

/// When the parts of one request happened (`net::LoadTimingInfo`), on the
/// clock every process reads alike (`TimeTicks`), so the renderer and the
/// browser's DevTools session line them up with their own events.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadTiming {
    /// When the request started (DevTools' `requestTime`).
    pub request_start: TimeTicks,
    /// Resolving the server's name, if this request did: not for a
    /// connection it reused, or an IP address.
    pub dns: Option<TimeSpan>,
    /// Opening the connection, TLS included, if this request did (after
    /// resolving, if it resolved).
    pub connect: Option<TimeSpan>,
    /// When the response's head had come.
    pub receive_headers_end: TimeTicks,
    /// When the whole body had come.
    pub response_end: TimeTicks,
}

/// A stretch of time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeSpan {
    pub start: TimeTicks,
    pub end: TimeTicks,
}

/// A load that produced no response. The codes are Chromium's
/// (`net/base/net_error_list.h`), so error pages read the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetErrorCode {
    Failed,
    FileNotFound,
    AccessDenied,
    InvalidUrl,
    UnknownUrlScheme,
    UnsafeRedirect,
    TooManyRedirects,
    InvalidRedirect,
    NameNotResolved,
    ConnectionRefused,
    ConnectionReset,
    ConnectionFailed,
    TimedOut,
    SslProtocolError,
    CertAuthorityInvalid,
    CertDateInvalid,
    CertCommonNameInvalid,
    CertInvalid,
    /// The body's `Content-Encoding` didn't decode.
    ContentDecodingFailed,
}

impl NetErrorCode {
    /// The name Chromium shows, e.g. `ERR_CONNECTION_REFUSED`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Failed => "ERR_FAILED",
            Self::FileNotFound => "ERR_FILE_NOT_FOUND",
            Self::AccessDenied => "ERR_ACCESS_DENIED",
            Self::InvalidUrl => "ERR_INVALID_URL",
            Self::UnknownUrlScheme => "ERR_UNKNOWN_URL_SCHEME",
            Self::UnsafeRedirect => "ERR_UNSAFE_REDIRECT",
            Self::TooManyRedirects => "ERR_TOO_MANY_REDIRECTS",
            Self::InvalidRedirect => "ERR_INVALID_REDIRECT",
            Self::NameNotResolved => "ERR_NAME_NOT_RESOLVED",
            Self::ConnectionRefused => "ERR_CONNECTION_REFUSED",
            Self::ConnectionReset => "ERR_CONNECTION_RESET",
            Self::ConnectionFailed => "ERR_CONNECTION_FAILED",
            Self::TimedOut => "ERR_TIMED_OUT",
            Self::SslProtocolError => "ERR_SSL_PROTOCOL_ERROR",
            Self::CertAuthorityInvalid => "ERR_CERT_AUTHORITY_INVALID",
            Self::CertDateInvalid => "ERR_CERT_DATE_INVALID",
            Self::CertCommonNameInvalid => "ERR_CERT_COMMON_NAME_INVALID",
            Self::CertInvalid => "ERR_CERT_INVALID",
            Self::ContentDecodingFailed => "ERR_CONTENT_DECODING_FAILED",
        }
    }
}

/// A failed load: the code, plus a detail for logs and error pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetError {
    pub code: NetErrorCode,
    pub detail: String,
}

impl NetError {
    pub fn new(code: NetErrorCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

pub type LoadResult = Result<ResourceResponse, NetError>;
