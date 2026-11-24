//! One hop of a load through the cache (Chromium's `HttpCache::
//! Transaction`): it answers from a stored response, validates one, or
//! sends the request on and keeps what comes back.
//!
//! - Only `GET` and `HEAD` are answered from it. A `POST`, `PUT`, `DELETE`
//!   or `PATCH` that succeeds (a 2xx or 3xx) drops the entry of its URL.
//! - Its request's load flags say how it uses the cache, with what its
//!   headers add (Chromium's effective load flags): past it, in and out
//!   (`disable_cache`; `If-Match` and the like, an empty `If-None-Match`);
//!   fetched again and stored (`bypass_cache`; `Cache-Control: no-cache`,
//!   `Pragma: no-cache`); validated even if fresh (`validate_cache`;
//!   `Cache-Control: max-age=0`); used even if stale (`skip_cache_
//!   validation`, unless its `Vary` differs); from the cache or not at all
//!   (`only_from_cache`: `ERR_CACHE_MISS` when it has nothing it may use
//!   as it is). The request's own flags also send their headers, under the
//!   cache.
//! - One with its own `If-None-Match` or `If-Modified-Since` is never
//!   answered from it (`UPDATE` mode): it goes out as it is; if the stored
//!   `200`'s validators are the ones it sends, a `304` updates the entry and
//!   a new response replaces it.
//! - A `HEAD` only uses a `GET`'s entry: a fresh one answers it without its
//!   body; one validated with a `304` is updated, one answered with a
//!   `200` dropped (that answer has no body to keep). It never makes one.
//! - A stored response is used as it is while fresh, else validated with
//!   its `ETag` and `Last-Modified` (a `304` updates it and it answers),
//!   else fetched again. `stale-while-revalidate` isn't used: a stale one
//!   is validated first (Chromium uses it only for loads that ask).
//! - While a load goes to the network for a key, a later one for the same
//!   key waits until it is over, then looks again: what it stored, if it
//!   could store it.

use protocol::network::{LoadFlags, NetError, NetErrorCode, ResourceRequest};
use url::Url;

use super::entry::StoredResponse;
use super::headers::{self, Validation};
use super::key::cache_key;
use super::request_headers::{ExternalValidation, HeaderFlag, header_flag};
use super::vary::VaryData;
use super::{HttpCache, Recorder, Slot};
use crate::cookies::Time;
use crate::http::{HttpBody, HttpTransport, RawResponse, headers_value};
use crate::url_loader::local_timing;

/// What a hop sends: its request's method, URL, `Accept` (unless the
/// headers have one), headers and body.
pub(crate) struct Hop<'a> {
    pub(crate) method: &'a str,
    pub(crate) url: &'a Url,
    pub(crate) accept: &'a str,
    pub(crate) headers: &'a [(String, String)],
    pub(crate) body: Option<&'a [u8]>,
}

/// Sends `hop` of `request` through `cache` over `transport`: the response,
/// from the network or from a stored one.
pub(crate) async fn send(
    cache: &HttpCache,
    transport: &HttpTransport,
    request: &ResourceRequest,
    hop: Hop<'_>,
) -> Result<RawResponse, NetError> {
    let asked = request.load_flags;
    let mut flags = asked;
    match header_flag(hop.headers) {
        Some(HeaderFlag::DisableCache) => flags.disable_cache = true,
        Some(HeaderFlag::BypassCache) => flags.bypass_cache = true,
        Some(HeaderFlag::ValidateCache) => flags.validate_cache = true,
        None => {}
    }
    let external = ExternalValidation::of(hop.headers).unwrap_or_else(|_| {
        // An empty validator: past the cache.
        flags.disable_cache = true;
        None
    });
    let miss = || {
        Err(NetError::new(
            NetErrorCode::CacheMiss,
            format!("{}: not in the HTTP cache", hop.url),
        ))
    };
    let key = (!flags.disable_cache)
        .then(|| cache_key(request, hop.url))
        .flatten();
    let Some(key) = key else {
        return if flags.only_from_cache {
            miss()
        } else {
            network(transport, &hop, hop.headers, asked).await
        };
    };
    let head = match hop.method {
        "GET" => false,
        "HEAD" => true,
        _ if flags.only_from_cache => return miss(),
        // A change to the resource: what was kept of it is gone, if it
        // worked (`NonErrorResponse`).
        "POST" | "PUT" | "DELETE" | "PATCH" => {
            let response = network(transport, &hop, hop.headers, asked).await?;
            if (200..400).contains(&response.status) {
                cache.remove(&key);
            }
            return Ok(response);
        }
        _ => return network(transport, &hop, hop.headers, asked).await,
    };
    // The caller's own validation is never answered from the cache.
    if external.is_some() && flags.only_from_cache {
        return miss();
    }
    // What the cache sees of the request: its headers, with the `Accept`
    // the transport adds (one a `Vary` may name).
    let mut seen = hop.headers.to_vec();
    if !seen.iter().any(|(name, _)| name.eq_ignore_ascii_case("accept")) {
        seen.push(("accept".to_owned(), hop.accept.to_owned()));
    }
    let slot = loop {
        match cache.begin(&key) {
            Ok(slot) => break slot,
            // Another load is getting it: once it is over, look again.
            Err(told) => {
                let _ = told.await;
            }
        }
    };
    let stored = cache.get(&key);
    if let Some(external) = external {
        let validating = stored
            .as_ref()
            .is_some_and(|stored| stored.status == 200 && external.matches(&stored.headers));
        if !validating {
            return network(transport, &hop, hop.headers, asked).await;
        }
        let request_time = Time::now();
        let response = network(transport, &hop, hop.headers, asked).await?;
        let response_time = Time::now();
        if response.status == 304 {
            if let Some(stored) = stored {
                update(cache, &key, stored, &response, &seen, request_time, response_time);
            }
            return Ok(response);
        }
        return Ok(keep(slot, response, head, &seen, request_time, response_time));
    }
    let stored = match stored {
        Some(stored) if !flags.bypass_cache => stored,
        // Nothing, or from the cache only and not from it (nonsense, in
        // Chromium's words): a miss either way.
        _ if flags.only_from_cache => return miss(),
        // Nothing to use: a `HEAD` makes no entry.
        _ if head => return network(transport, &hop, hop.headers, asked).await,
        _ => {
            let request_time = Time::now();
            let response = network(transport, &hop, hop.headers, asked).await?;
            let response_time = Time::now();
            return Ok(keep(slot, response, false, &seen, request_time, response_time));
        }
    };
    let vary_mismatch = stored
        .vary
        .as_ref()
        .is_some_and(|vary| !vary.matches(&seen, &stored.headers));
    let validation = if vary_mismatch {
        Validation::Synchronous
    } else if flags.skip_cache_validation {
        Validation::None
    } else if flags.validate_cache {
        Validation::Synchronous
    } else {
        headers::requires_validation(
            stored.status,
            &stored.headers,
            stored.request_time,
            stored.response_time,
            Time::now(),
        )
    };
    if validation == Validation::None {
        return Ok(answer(stored, hop.url, head));
    }
    // From the cache only, but it has to be validated first.
    if flags.only_from_cache {
        return miss();
    }
    let validators =
        headers::validation_headers(stored.status, stored.protocol, &stored.headers, vary_mismatch);
    let mut sent = hop.headers.to_vec();
    if let Some(validators) = &validators {
        sent.extend(validators.iter().cloned());
    }
    let request_time = Time::now();
    let response = network(transport, &hop, &sent, asked).await?;
    let response_time = Time::now();
    if validators.is_some() && response.status == 304 {
        let updated = update(cache, &key, stored, &response, &seen, request_time, response_time);
        return Ok(answer(updated, hop.url, head));
    }
    Ok(keep(slot, response, head, &seen, request_time, response_time))
}

/// `hop` over the network, with `headers`, and the headers its request's
/// own `flags` send (`HttpNetworkTransaction::BuildRequestHeaders`, under
/// the cache, which doesn't see them): `Pragma: no-cache` and
/// `Cache-Control: no-cache` when it bypasses the cache, `Cache-Control:
/// max-age=0` when it validates.
async fn network(
    transport: &HttpTransport,
    hop: &Hop<'_>,
    headers: &[(String, String)],
    flags: LoadFlags,
) -> Result<RawResponse, NetError> {
    let added: &[(&str, &str)] = if flags.bypass_cache {
        &[("pragma", "no-cache"), ("cache-control", "no-cache")]
    } else if flags.validate_cache {
        &[("cache-control", "max-age=0")]
    } else {
        &[]
    };
    if added.is_empty() {
        return transport
            .send(hop.method, hop.url, hop.accept, headers, hop.body)
            .await;
    }
    // Each in place of the request's own (`SetHeader`).
    let mut sent: Vec<(String, String)> = headers
        .iter()
        .filter(|(name, _)| !added.iter().any(|(added, _)| name.eq_ignore_ascii_case(added)))
        .cloned()
        .collect();
    sent.extend(added.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())));
    transport
        .send(hop.method, hop.url, hop.accept, &sent, hop.body)
        .await
}

/// `response`, from the network, kept under `slot`'s key as its body comes
/// (or, a `HEAD`'s, in place of nothing: the entry goes). One that may not
/// be stored drops what was kept, and `slot` goes at once, letting the
/// loads waiting for it go to the network themselves.
fn keep(
    slot: Slot,
    mut response: RawResponse,
    head: bool,
    seen: &[(String, String)],
    request_time: Time,
    response_time: Time,
) -> RawResponse {
    if head || !headers::is_storable(&response.headers) {
        slot.cache.remove(&slot.key);
        return response;
    }
    let stored = StoredResponse {
        status: response.status,
        status_text: response.status_text.clone(),
        protocol: response.protocol,
        headers: without_cookies(&response.headers),
        request_time,
        response_time,
        vary: VaryData::new(seen, &response.headers),
        body: Vec::new(),
    };
    response.body.record(Recorder::new(slot, stored));
    response
}

/// The stored response updated by a `304` to its validation
/// (`DoUpdateCachedResponse`): its headers merged, its times the
/// validation's, its `Vary` again; kept, unless it now says `no-store`.
fn update(
    cache: &HttpCache,
    key: &str,
    mut stored: StoredResponse,
    response: &RawResponse,
    seen: &[(String, String)],
    request_time: Time,
    response_time: Time,
) -> StoredResponse {
    stored.headers = without_cookies(&headers::update(&stored.headers, &response.headers));
    stored.request_time = request_time;
    stored.response_time = response_time;
    stored.vary = VaryData::new(seen, &stored.headers);
    if headers::is_storable(&stored.headers) {
        cache.put(key, stored.clone());
    } else {
        cache.remove(key);
    }
    stored
}

/// `headers` without the cookies they set: the cache never sets one again
/// (`PERSIST_SANS_COOKIES`).
fn without_cookies(headers: &[(String, String)]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter(|(name, _)| !name.eq_ignore_ascii_case("set-cookie") && !name.eq_ignore_ascii_case("set-cookie2"))
        .cloned()
        .collect()
}

/// The response `stored` answers with, to `url`; a `HEAD`'s without the
/// body (over HTTP, only the server leaves a `HEAD`'s out).
fn answer(stored: StoredResponse, url: &Url, head: bool) -> RawResponse {
    let content_encoding = headers_value(&stored.headers, "content-encoding");
    let body = if head { Vec::new() } else { stored.body };
    RawResponse {
        status: stored.status,
        status_text: stored.status_text,
        body: HttpBody::stored(url.clone(), body, content_encoding.as_deref()),
        headers: stored.headers,
        timing: local_timing(ipc::TimeTicks::now()),
        remote_address: None,
        protocol: stored.protocol,
        encoded_headers_length: 0,
        from_cache: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn not_modified(headers: &[(&str, &str)]) -> RawResponse {
        let url = Url::parse("http://a.test/").expect("a URL");
        RawResponse {
            status: 304,
            status_text: String::new(),
            headers: lines(headers),
            body: HttpBody::stored(url, Vec::new(), None),
            timing: local_timing(ipc::TimeTicks::now()),
            remote_address: None,
            protocol: "http/1.1",
            encoded_headers_length: 0,
            from_cache: false,
        }
    }

    fn stored() -> StoredResponse {
        StoredResponse {
            status: 200,
            status_text: "OK".to_owned(),
            protocol: "http/1.1",
            headers: lines(&[("cache-control", "no-cache"), ("vary", "x-mode")]),
            request_time: Time::UNIX_EPOCH,
            response_time: Time::UNIX_EPOCH,
            vary: Some(VaryData::Values("old\n".to_owned())),
            body: vec![bytes::Bytes::from_static(b"body")],
        }
    }

    #[test]
    fn a_304_updates_the_headers_times_and_vary() {
        let cache = HttpCache::default();
        let (asked, came) = (
            Time::UNIX_EPOCH.saturating_add_seconds(100),
            Time::UNIX_EPOCH.saturating_add_seconds(101),
        );
        let seen = lines(&[("x-mode", "new")]);
        let updated = update(
            &cache,
            "k",
            stored(),
            &not_modified(&[("cache-control", "max-age=60"), ("set-cookie", "a=1")]),
            &seen,
            asked,
            came,
        );
        assert_eq!(
            updated.headers,
            lines(&[("vary", "x-mode"), ("cache-control", "max-age=60")])
        );
        assert_eq!((updated.request_time, updated.response_time), (asked, came));
        assert_eq!(updated.vary, Some(VaryData::Values("new\n".to_owned())));
        assert_eq!(updated.body, stored().body);
        assert_eq!(cache.get("k"), Some(updated));
    }

    #[test]
    fn a_304_that_says_no_store_drops_it() {
        let cache = HttpCache::default();
        cache.put("k", stored());
        let updated = update(
            &cache,
            "k",
            stored(),
            &not_modified(&[("cache-control", "no-store")]),
            &[],
            Time::UNIX_EPOCH,
            Time::UNIX_EPOCH,
        );
        // It still answers this time.
        assert_eq!(updated.body, stored().body);
        assert_eq!(cache.get("k"), None);
    }

    #[test]
    fn cookies_arent_kept() {
        let headers: Vec<(String, String)> = [
            ("set-cookie", "a=1"),
            ("etag", "\"v1\""),
            ("Set-Cookie2", "b=2"),
        ]
        .iter()
        .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
        .collect();
        assert_eq!(
            without_cookies(&headers),
            [("etag".to_owned(), "\"v1\"".to_owned())]
        );
    }
}

