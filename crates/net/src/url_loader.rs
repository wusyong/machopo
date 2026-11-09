//! `URLRequest`: one load from start to finish. Picks the scheme, follows
//! redirects (for a load that waits at them, once told to), fills in the
//! MIME type, and goes by CORS (`cors`) for a request with an initiator.

use ipc::TimeTicks;
use protocol::network::{
    LoadResult, LoadTiming, NetError, NetErrorCode, RedirectInfo, RedirectMode, RequestDestination,
    ResourceRequest, ResourceResponse, ResponseTainting, is_forbidden_method,
    is_forbidden_request_header,
};
use url::Url;

use crate::Redirects;
use crate::cors::Cors;
use crate::http::{HttpTransport, RawResponse};
use crate::{data, file, mime};

/// `kMaxRedirects` in Chromium's `URLRequest`.
const MAX_REDIRECTS: usize = 20;

/// Loads `request`; with `redirects`, telling of each redirect it would
/// follow and waiting there until it may.
pub(crate) async fn load(
    transport: &HttpTransport,
    request: &ResourceRequest,
    mut redirects: Option<Redirects>,
) -> LoadResult {
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
    let filter = |cors: &Option<Cors<'_>>, response: ResourceResponse| match cors {
        Some(cors) => cors.filter(response),
        None => response,
    };
    let head = |response: LoadResult| {
        response.map(|mut response| {
            if request.method == "HEAD" {
                response.body.clear();
            }
            filter(&cors, response)
        })
    };
    // A local scheme's response is timed whole: no connection, no head
    // before the body.
    let start = TimeTicks::now();
    let local = |response: LoadResult| {
        response.map(|mut response| {
            response.timing = Some(local_timing(start));
            response
        })
    };
    match url.scheme() {
        "file" => return head(local(file::load(url).await)),
        "data" => return head(local(data::load(&url))),
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
        if let Some(cors) = &cors {
            cors.add_origin(&method, &mut sent);
            // Fetch's "CORS-preflight fetch", for each hop that needs one
            // (no cache): the other origin has to allow the request first.
            if let Some(preflight) = cors.preflight(&method, &headers) {
                let answer = transport
                    .send("OPTIONS", &url, "*/*", &preflight, None, false)
                    .await?;
                cors.check_preflight(&url, &answer, &method, &headers)?;
            }
        }
        let raw = transport
            .send(&method, &url, accept, &sent, body.as_deref(), true)
            .await?;
        if let Some(cors) = &cors {
            cors.check(&url, &raw)?;
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
                RedirectMode::Manual => return Ok(filter(&cors, finish(url, raw, false))),
            }
        }
        match redirect_target(&url, &raw)? {
            Some(next) => {
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
                if let Some(redirects) = &mut redirects {
                    let mut response = finish(url.clone(), raw, hops > 0);
                    response.body.clear();
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
            None => return Ok(filter(&cors, finish(url, raw, hops > 0))),
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
        RequestDestination::Script | RequestDestination::Empty => "*/*",
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

fn finish(url: Url, mut raw: RawResponse, redirected: bool) -> ResourceResponse {
    // Cookies never go to a renderer, whatever the tainting (machopo keeps
    // none, and script may never read them).
    raw.headers
        .retain(|(name, _)| !matches!(name.as_str(), "set-cookie" | "set-cookie2"));
    let (mime_type, charset) = match raw
        .header("content-type")
        .and_then(mime::parse_content_type)
    {
        Some(parsed) => parsed,
        None => (mime::sniff(&raw.body).to_owned(), None),
    };
    ResourceResponse {
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
        encoded_body_length: raw.encoded_body_length,
        body: raw.body.into(),
    }
}
