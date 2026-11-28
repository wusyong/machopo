//! `URLRequest`: one load from start to finish. Picks the scheme, follows
//! redirects (for a load that waits at them, once told to), fills in the
//! MIME type, and goes by CORS (`cors`) for a request with an initiator.
//! It answers with the response's head; the body comes after, as it is
//! read.

use bytes::{Bytes, BytesMut};
use ipc::TimeTicks;
use protocol::network::{
    LoadTiming, NetError, NetErrorCode, RedirectInfo, RedirectMode, RequestDestination,
    ResourceRequest, ResourceResponse, ResponseHead, ResponseTainting, is_forbidden_method,
    is_forbidden_request_header,
};
use url::Url;

use crate::Redirects;
use crate::cors::Cors;
use crate::file::FileBody;
use crate::http::{HttpBody, HttpTransport, RawResponse};
use crate::{data, file, mime};

/// `kMaxRedirects` in Chromium's `URLRequest`.
const MAX_REDIRECTS: usize = 20;

/// How much of a body without a `Content-Type` is read before its head is
/// given, to sniff its type (`net::kMaxBytesToSniff`).
const SNIFF_LENGTH: usize = 1024;

/// A response as a load gives it: its head, and its body to come.
pub(crate) struct Response {
    pub(crate) head: ResponseHead,
    pub(crate) body: Body,
    /// A local scheme's: its head and its body come together, so a whole
    /// response is timed as one.
    pub(crate) local: bool,
}

impl Response {
    /// `response`, its body already all there.
    pub(crate) fn whole(response: ResourceResponse) -> Self {
        Self {
            head: response.head,
            body: Body::whole(response.body.into_bytes()),
            local: false,
        }
    }
}

/// A response's body, read a piece at a time (`URLRequest::Read`).
pub(crate) struct Body {
    /// Read already: the part sniffed, or all of a body that came whole.
    ahead: Option<Bytes>,
    source: Source,
    /// Read to its end, but none of it given (a `HEAD`'s, an opaque
    /// `fetch()`'s).
    discarded: bool,
}

enum Source {
    Over,
    /// Boxed: much bigger than the others, and one per load.
    Http(Box<HttpBody>),
    File(FileBody),
}

impl Body {
    pub(crate) fn whole(bytes: Bytes) -> Self {
        Self {
            ahead: (!bytes.is_empty()).then_some(bytes),
            source: Source::Over,
            discarded: false,
        }
    }

    pub(crate) fn file(file: FileBody) -> Self {
        Self {
            ahead: None,
            source: Source::File(file),
            discarded: false,
        }
    }

    fn http(body: HttpBody) -> Self {
        Self {
            ahead: None,
            source: Source::Http(Box::new(body)),
            discarded: false,
        }
    }

    /// Reads it to its end, but gives none of it.
    pub(crate) fn discard(&mut self) {
        self.ahead = None;
        self.discarded = true;
    }

    /// The next piece; `None` once it is over.
    pub(crate) async fn next(&mut self) -> Result<Option<Bytes>, NetError> {
        if self.discarded {
            while self.read().await?.is_some() {}
            return Ok(None);
        }
        match self.ahead.take() {
            Some(ahead) => Ok(Some(ahead)),
            None => self.read().await,
        }
    }

    /// The bytes it took on the wire so far, before `Content-Encoding` was
    /// undone: none but over the network.
    pub(crate) fn encoded_length(&self) -> u64 {
        match &self.source {
            Source::Http(body) => body.encoded_length(),
            Source::Over | Source::File(_) => 0,
        }
    }

    /// Its first `length` bytes, or all of it if it is shorter, read ahead:
    /// they still come from [`next`](Self::next).
    async fn read_ahead(&mut self, length: usize) -> Result<Bytes, NetError> {
        let mut pieces: Vec<Bytes> = self.ahead.take().into_iter().collect();
        let mut read = pieces.first().map_or(0, Bytes::len);
        while read < length {
            let Some(piece) = self.read().await? else {
                break;
            };
            read = read.saturating_add(piece.len());
            pieces.push(piece);
        }
        let ahead = concat(pieces);
        self.ahead = (!ahead.is_empty()).then(|| ahead.clone());
        Ok(ahead)
    }

    async fn read(&mut self) -> Result<Option<Bytes>, NetError> {
        match &mut self.source {
            Source::Over => Ok(None),
            Source::Http(body) => body.next().await,
            Source::File(body) => body.next().await,
        }
    }
}

/// `pieces` as one, copying only if there is more than one.
pub fn concat(mut pieces: Vec<Bytes>) -> Bytes {
    match pieces.len() {
        0 => Bytes::new(),
        1 => pieces.swap_remove(0),
        _ => {
            let length = pieces.iter().map(Bytes::len).fold(0, usize::saturating_add);
            let mut all = BytesMut::with_capacity(length);
            for piece in &pieces {
                all.extend_from_slice(piece);
            }
            all.freeze()
        }
    }
}

/// Loads `request` up to its response's head; with `redirects`, telling
/// of each redirect it would follow and waiting there until it may.
pub(crate) async fn load(
    transport: &HttpTransport,
    cookies: &crate::cookies::CookieJar,
    cache: &crate::http_cache::HttpCache,
    request: &ResourceRequest,
    mut redirects: Option<Redirects<'_>>,
) -> Result<Response, NetError> {
    let mut url = Url::parse(&request.url)
        .map_err(|e| NetError::new(NetErrorCode::InvalidUrl, format!("{}: {e}", request.url)))?;
    // The renderer checked these too; a renderer isn't trusted.
    if let Some((name, _)) = request
        .headers
        .iter()
        .find(|(name, value)| is_forbidden_request_header(name, value))
    {
        return Err(NetError::new(
            NetErrorCode::Failed,
            format!("{}: forbidden request header {name}", request.url),
        ));
    }
    if !is_token(&request.method) || is_forbidden_method(&request.method) {
        return Err(NetError::new(
            NetErrorCode::Failed,
            format!(
                "{}: method {:?} may not be sent",
                request.url, request.method
            ),
        ));
    }
    let mut cors = Cors::new(request);
    if let Some(cors) = &mut cors {
        cors.start(&url)?;
    }
    // Fetch's "main fetch": a `HEAD`'s response has no body, whatever the
    // scheme; and it is filtered for its tainting.
    let filter = |cors: &Option<Cors<'_>>, response: Response| match cors {
        Some(cors) => cors.filter(response),
        None => response,
    };
    let head = |response: Result<Response, NetError>| {
        response.map(|mut response| {
            if request.method == "HEAD" {
                response.body.discard();
            }
            filter(&cors, response)
        })
    };
    // A local scheme's response: no connection, its head as it starts.
    let start = TimeTicks::now();
    let local = |response: Result<Response, NetError>| {
        response.map(|mut response| {
            response.head.timing = Some(local_timing(start));
            response.local = true;
            response
        })
    };
    match url.scheme() {
        "file" => return head(local(file::load(url).await)),
        "data" => return head(local(data::load(&url).map(Response::whole))),
        "http" | "https" => {}
        _ => {
            return Err(NetError::new(
                NetErrorCode::UnknownUrlScheme,
                request.url.clone(),
            ));
        }
    }
    let accept = accept_for(request.destination);
    // What a redirect may change of the request.
    let mut method = request.method.clone();
    let mut headers = request.headers.clone();
    let mut body = request.body.clone();
    // Following redirects here, not in the transport, keeps every hop in
    // one place whatever the transport is.
    for hops in 0..=MAX_REDIRECTS {
        // Fetch's "append a request `Origin` header", afresh for each hop:
        // the tainting, and the origin, may change on a redirect.
        let mut sent = headers.clone();
        // Fetch's "include credentials" for this hop: `include`, or
        // `same-origin` while the response would be basic (of the
        // initiator's origin, or the browser's own request).
        let credentials = match request.credentials {
            protocol::network::CredentialsMode::Include => true,
            protocol::network::CredentialsMode::SameOrigin => cors
                .as_ref()
                .is_none_or(|cors| cors.tainting == ResponseTainting::Basic),
            protocol::network::CredentialsMode::Omit => false,
        };
        if credentials {
            // The profile's cookies first.
            cookies.wait_for_load().await;
        }
        if credentials
            && let Some(cookie) = cookies.cookie_header(&url, request.trusted.as_ref(), &method)
        {
            sent.push(("cookie".to_owned(), cookie));
        }
        if let Some(cors) = &cors {
            cors.add_origin(&method, &mut sent);
            // Fetch's "CORS-preflight fetch", for each hop that needs one
            // (no cache): the other origin has to allow the request first.
            if let Some(preflight) = cors.preflight(&method, &headers) {
                // Its body isn't read (Chromium's isn't either): one that
                // never ends mustn't stall the request.
                let answer = transport
                    .send("OPTIONS", &url, "*/*", &preflight, None)
                    .await?;
                cors.check_preflight(&url, &answer, &method, &headers)?;
            }
        }
        let mut raw = crate::http_cache::transaction::send(
            cache,
            transport,
            request,
            crate::http_cache::transaction::Hop {
                method: &method,
                url: &url,
                accept,
                headers: &sent,
                body: body.as_deref(),
            },
        )
        .await?;
        // A redirect's cookies are kept too, before it is followed (or
        // refused by CORS: the server set them all the same, as
        // Chromium's `URLRequestHttpJob` keeps them before its checks).
        if credentials {
            cookies.set_from_response(&url, request.trusted.as_ref(), &raw.headers);
        }
        if let Some(cors) = &cors {
            cors.check(&url, &raw)?;
        }
        // A renderer's `no-cors` request (one with an initiator: the
        // browser's own have none), each hop's response.
        if request.mode == protocol::network::RequestMode::NoCors
            && let Some(initiator) = request.initiator.as_deref()
            && crate::corp::blocks(
                initiator,
                &url,
                &raw.headers,
                crate::corp::EmbedderPolicy::default(),
            )
        {
            return Err(NetError::new(
                NetErrorCode::BlockedByResponse,
                format!("{url}: its Cross-Origin-Resource-Policy isn't for {initiator}"),
            ));
        }
        if is_redirect_status(raw.status) {
            match request.redirect {
                RedirectMode::Follow => {}
                RedirectMode::Error => {
                    return Err(NetError::new(
                        NetErrorCode::Failed,
                        format!("{url}: redirected, and the request says that's an error"),
                    ));
                }
                // The redirect itself, which the renderer filters.
                RedirectMode::Manual => {
                    return Ok(filter(&cors, respond(url, raw, false).await?));
                }
            }
        }
        match redirect_target(&url, &raw)? {
            Some(next) => {
                // Followed, so its body isn't read: the cache keeps it as it
                // is (`DoneReading`).
                raw.body.done_reading();
                if let Some(cors) = &mut cors {
                    cors.redirect(&url, &next)?;
                }
                // Fetch's "HTTP-redirect fetch": a 301 or 302 of a `POST`,
                // and a 303 of anything but `GET` or `HEAD`, go on as a
                // `GET` without the body and the headers about it.
                if (matches!(raw.status, 301 | 302) && method == "POST")
                    || (raw.status == 303 && !matches!(method.as_str(), "GET" | "HEAD"))
                {
                    method = "GET".to_owned();
                    body = None;
                    headers.retain(|(name, _)| !is_request_body_header_name(name));
                }
                // Credentials stay with their origin.
                if next.origin() != url.origin() {
                    headers.retain(|(name, _)| !name.eq_ignore_ascii_case("authorization"));
                }
                // Its body isn't read: dropping it closes the connection.
                if let Some(redirects) = &mut redirects {
                    let mime =
                        content_type(&raw).unwrap_or_else(|| (mime::sniff(b"").into(), None));
                    let (response, _body) = split(url.clone(), raw, hops > 0, mime);
                    redirects
                        .wait_at(RedirectInfo {
                            new_url: next.to_string(),
                            new_method: method.clone(),
                            response,
                        })
                        .await;
                }
                url = next;
                if let Some(cors) = &mut cors {
                    cors.start(&url)?;
                }
            }
            None => return Ok(filter(&cors, respond(url, raw, hops > 0).await?)),
        }
    }
    Err(NetError::new(
        NetErrorCode::TooManyRedirects,
        request.url.clone(),
    ))
}

/// Chromium's `Accept` header for each destination, minus the image
/// formats we can't decode.
fn accept_for(destination: RequestDestination) -> &'static str {
    match destination {
        RequestDestination::Document | RequestDestination::Iframe => {
            "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
        }
        RequestDestination::Style => "text/css,*/*;q=0.1",
        RequestDestination::Image => "image/png,image/jpeg,image/gif,image/*;q=0.8,*/*;q=0.5",
        RequestDestination::Script | RequestDestination::Worker | RequestDestination::Empty => {
            "*/*"
        }
    }
}

/// HTTP's `token` (a method).
fn is_token(text: &str) -> bool {
    !text.is_empty()
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

/// Fetch's "redirect status".
fn is_redirect_status(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// Fetch's "request-body-header name": what goes with a body that a
/// redirect drops.
fn is_request_body_header_name(name: &str) -> bool {
    [
        "content-encoding",
        "content-language",
        "content-location",
        "content-type",
    ]
    .iter()
    .any(|body_header| name.eq_ignore_ascii_case(body_header))
}

/// Where a redirect response points, if it is one. A 3xx without a usable
/// `Location` is an ordinary response and shown as is, as in Chromium.
fn redirect_target(url: &Url, raw: &RawResponse) -> Result<Option<Url>, NetError> {
    if !is_redirect_status(raw.status) {
        return Ok(None);
    }
    let Some(location) = raw.header("location") else {
        return Ok(None);
    };
    // The header's bytes (a character each), as UTF-8, as a URL is; bytes
    // that aren't stay bytes, percent-encoded (as in Chrome).
    let location: Vec<u8> = location.chars().map(|c| c as u8).collect();
    let mut decoded = String::new();
    for chunk in location.utf8_chunks() {
        decoded.push_str(chunk.valid());
        for byte in chunk.invalid() {
            decoded.push_str(&format!("%{byte:02X}"));
        }
    }
    let location = decoded.as_str();
    let mut next = url.join(location).map_err(|e| {
        NetError::new(
            NetErrorCode::InvalidRedirect,
            format!("{url} -> {location}: {e}"),
        )
    })?;
    // Web pages may not redirect to local files, or anything else that
    // isn't HTTP (`URLRequestHttpJob::IsSafeRedirect`).
    if !matches!(next.scheme(), "http" | "https") {
        return Err(NetError::new(
            NetErrorCode::UnsafeRedirect,
            format!("{url} -> {next}"),
        ));
    }
    // A redirect without a fragment keeps the original one (Fetch, step
    // "HTTP-redirect fetch").
    if next.fragment().is_none() {
        next.set_fragment(url.fragment());
    }
    Ok(Some(next))
}

/// The timing of a load that didn't go over the network, which started at
/// `start` and is over now.
pub(crate) fn local_timing(start: TimeTicks) -> LoadTiming {
    let end = TimeTicks::now();
    LoadTiming {
        request_start: start,
        dns: None,
        connect: None,
        receive_headers_end: end,
        response_end: end,
    }
}

/// `Content-Type`'s MIME type and charset, if it has a usable one (Fetch's
/// "extract a MIME type", Chromium's `GetMimeTypeAndCharset`): of every
/// value (each header's, split at the commas outside quotes), the last
/// usable one but `*/*`, with the charset of the last of the same type
/// that had one. A server that sends two (`application/json`, then
/// `text/html`) gets the last.
fn content_type(raw: &RawResponse) -> Option<(String, Option<String>)> {
    let values = raw
        .headers
        .iter()
        .filter(|(name, _)| name == "content-type")
        .flat_map(|(_, value)| mime::split_header_values(value));
    let mut found: Option<(String, Option<String>)> = None;
    for (essence, charset) in values.filter_map(mime::parse_content_type) {
        if essence == "*/*" {
            continue;
        }
        found = Some(match found {
            Some((previous, kept)) if previous == essence && charset.is_none() => (essence, kept),
            _ => (essence, charset),
        });
    }
    found
}

/// The final response: its head, once its type is known: from
/// `Content-Type`, or sniffed from the start of its body (Chromium's
/// `network::URLLoader` sniffs before it gives the head too).
async fn respond(url: Url, raw: RawResponse, redirected: bool) -> Result<Response, NetError> {
    let content_type = content_type(&raw);
    let (mut head, body) = split(url, raw, redirected, (String::new(), None));
    let mut body = Body::http(body);
    (head.mime_type, head.charset) = match content_type {
        Some(parsed) => parsed,
        None => {
            let start = body.read_ahead(SNIFF_LENGTH).await?;
            let sniffed = start.get(..SNIFF_LENGTH).unwrap_or(&start);
            (mime::sniff(sniffed).to_owned(), None)
        }
    };
    Ok(Response {
        head,
        body,
        local: false,
    })
}

/// `raw`'s head, of type `mime`, and its body.
fn split(
    url: Url,
    mut raw: RawResponse,
    redirected: bool,
    (mime_type, charset): (String, Option<String>),
) -> (ResponseHead, HttpBody) {
    // Cookies never go to a renderer, whatever the tainting (machopo keeps
    // none, and script may never read them).
    raw.headers
        .retain(|(name, _)| !matches!(name.as_str(), "set-cookie" | "set-cookie2"));
    let head = ResponseHead {
        url: url.into(),
        status: raw.status,
        status_text: raw.status_text,
        redirected,
        tainting: ResponseTainting::Basic,
        mime_type,
        charset,
        headers: raw.headers,
        timing: Some(raw.timing),
        remote_address: raw.remote_address,
        protocol: raw.protocol.to_owned(),
        encoded_headers_length: raw.encoded_headers_length,
        was_fetched_via_cache: raw.from_cache,
    };
    (head, raw.body)
}
