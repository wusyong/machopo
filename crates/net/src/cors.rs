//! CORS for a request with an initiator (`fetch()`'s, and a subresource's
//! that goes by CORS: a module script, `crossorigin`): Chromium's
//! `CorsURLLoader`, in the network service, as a renderer isn't trusted
//! to keep another origin's data from script. Fetch's parts of "main
//! fetch" (the response tainting, by the request's mode), "HTTP fetch"
//! (the CORS check, and the preflight: Chromium's `PreflightController`,
//! without its cache), "HTTP-redirect fetch" (credentials in a location,
//! the tainted origin) and the CORS-filtered and opaque responses.

use protocol::network::{
    CredentialsMode, NetError, NetErrorCode, RedirectMode, RequestDestination, RequestMode,
    ResourceRequest, ResourceResponse, ResponseTainting, cors_unsafe_request_header_names,
    is_cors_safelisted_method,
};
use url::Url;

use crate::http::RawResponse;

/// A request's CORS state across its redirects.
pub(crate) struct Cors<'r> {
    request: &'r ResourceRequest,
    /// The initiator's origin, serialized.
    origin: &'r str,
    /// Fetch's tainted origin flag: a redirect went from another origin to
    /// a third; the request's origin is `null` from then on.
    tainted: bool,
    pub(crate) tainting: ResponseTainting,
}

/// A network error of CORS's (Chromium's `net::ERR_FAILED` with a CORS
/// error status).
fn blocked(what: impl std::fmt::Display) -> NetError {
    NetError::new(NetErrorCode::Failed, format!("CORS: {what}"))
}

/// Whether `url` is of the origin serialized as `origin` (never, for an
/// opaque one).
fn same_origin(url: &Url, origin: &str) -> bool {
    url.origin().is_tuple() && url.origin().ascii_serialization() == origin
}

fn includes_credentials(url: &Url) -> bool {
    !url.username().is_empty() || url.password().is_some()
}

/// A response header's value (names lowercase), its values joined.
fn header(raw: &RawResponse, name: &str) -> Option<String> {
    let values: Vec<&str> = raw
        .headers
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

/// Fetch's "extract header list values" of `name` for a preflight: its
/// values split on commas, each trimmed and a token (`None` if one isn't);
/// none if it's absent.
fn header_list_values(raw: &RawResponse, name: &str) -> Option<Vec<String>> {
    let Some(value) = header(raw, name) else {
        return Some(Vec::new());
    };
    value
        .split(',')
        .map(|item| item.trim_matches(|c| matches!(c, ' ' | '\t')))
        .filter(|item| !item.is_empty())
        .map(|item| is_token(item).then(|| item.to_owned()))
        .collect()
}

/// HTTP's `token`.
fn is_token(text: &str) -> bool {
    text.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

impl<'r> Cors<'r> {
    /// The CORS state of `request`; `None` for one without an initiator
    /// (the browser's own, a subresource that doesn't go by CORS), which
    /// CORS doesn't apply to.
    pub(crate) fn new(request: &'r ResourceRequest) -> Option<Self> {
        Some(Self {
            request,
            origin: request.initiator.as_deref()?,
            tainted: false,
            tainting: ResponseTainting::Basic,
        })
    }

    /// The request's origin as `Origin` says it: `null` once tainted.
    fn request_origin(&self) -> &str {
        if self.tainted { "null" } else { self.origin }
    }

    /// Fetch's "main fetch", for the request going to `url` (first, or
    /// after a redirect): the response tainting, by the mode, or a network
    /// error.
    pub(crate) fn start(&mut self, url: &Url) -> Result<(), NetError> {
        let request = self.request;
        // The cases in Fetch's order: a `data:` URL is basic whatever the
        // mode.
        if (same_origin(url, self.origin)
            && !self.tainted
            && self.tainting == ResponseTainting::Basic)
            || url.scheme() == "data"
        {
            self.tainting = ResponseTainting::Basic;
            return Ok(());
        }
        match request.mode {
            RequestMode::SameOrigin => {
                return Err(blocked(format!(
                    "{url} isn't of {}, and the mode is same-origin",
                    self.request_origin()
                )));
            }
            RequestMode::NoCors => {
                if request.redirect != RedirectMode::Follow {
                    return Err(blocked("a no-cors request must follow its redirects"));
                }
                self.tainting = ResponseTainting::Opaque;
            }
            RequestMode::Cors => {
                if !matches!(url.scheme(), "http" | "https") {
                    return Err(blocked(format!("{url}: CORS is only for HTTP(S)")));
                }
                self.tainting = ResponseTainting::Cors;
            }
        }
        Ok(())
    }

    /// Fetch's "append a request `Origin` header", to `headers`: a CORS
    /// request's, or one other than a `GET` or `HEAD`.
    pub(crate) fn add_origin(&self, method: &str, headers: &mut Vec<(String, String)>) {
        if self.tainting == ResponseTainting::Cors || !matches!(method, "GET" | "HEAD") {
            headers.push(("Origin".to_owned(), self.request_origin().to_owned()));
        }
    }

    /// The headers of the preflight a CORS request of `method` with
    /// `headers` needs before it goes (Fetch's "CORS-preflight fetch"): its
    /// method isn't safelisted, or it has unsafe headers; `None` if it
    /// needs none. An `OPTIONS` asking for them (with `Origin`, which
    /// `add_origin` gives a CORS request, and `Accept: */*`), without
    /// script's headers or a body.
    pub(crate) fn preflight(
        &self,
        method: &str,
        headers: &[(String, String)],
    ) -> Option<Vec<(String, String)>> {
        if self.tainting != ResponseTainting::Cors {
            return None;
        }
        let unsafe_names = cors_unsafe_request_header_names(headers);
        if is_cors_safelisted_method(method) && unsafe_names.is_empty() {
            return None;
        }
        let mut preflight = vec![
            ("Accept".to_owned(), "*/*".to_owned()),
            (
                "Access-Control-Request-Method".to_owned(),
                method.to_owned(),
            ),
        ];
        if !unsafe_names.is_empty() {
            preflight.push((
                "Access-Control-Request-Headers".to_owned(),
                unsafe_names.join(","),
            ));
        }
        self.add_origin("OPTIONS", &mut preflight);
        Some(preflight)
    }

    /// Whether the preflight's response `raw` lets the request of `method`
    /// with `headers` go (Fetch's "CORS-preflight fetch"): it passes the
    /// CORS check with an ok status (a redirect doesn't), and allows the
    /// method (unless safelisted) and every unsafe header, by
    /// `Access-Control-Allow-Methods` and `-Headers` (`*`, without
    /// credentials, for any; `Authorization` only by name).
    pub(crate) fn check_preflight(
        &self,
        url: &Url,
        raw: &RawResponse,
        method: &str,
        headers: &[(String, String)],
    ) -> Result<(), NetError> {
        self.check(url, raw)?;
        if !(200..=299).contains(&raw.status) {
            return Err(blocked(format!("{url}: preflight status {}", raw.status)));
        }
        let methods = header_list_values(raw, "access-control-allow-methods").ok_or_else(|| {
            blocked(format!(
                "{url}: Access-Control-Allow-Methods isn't a list of methods"
            ))
        })?;
        let names = header_list_values(raw, "access-control-allow-headers").ok_or_else(|| {
            blocked(format!(
                "{url}: Access-Control-Allow-Headers isn't a list of names"
            ))
        })?;
        let include = self.request.credentials == CredentialsMode::Include;
        if !methods.iter().any(|allowed| allowed == method)
            && !is_cors_safelisted_method(method)
            && (include || !methods.iter().any(|allowed| allowed == "*"))
        {
            return Err(blocked(format!("{url} doesn't allow {method}")));
        }
        let named = |name: &str| {
            names
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(name))
        };
        if headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("authorization"))
            && !named("authorization")
        {
            return Err(blocked(format!("{url} doesn't allow Authorization")));
        }
        for name in cors_unsafe_request_header_names(headers) {
            if !named(&name) && (include || !named("*")) {
                return Err(blocked(format!("{url} doesn't allow the header {name}")));
            }
        }
        Ok(())
    }

    /// Fetch's "CORS check" of a CORS request's response (a redirect's
    /// too): the other origin allows this one (`Access-Control-Allow-Origin`
    /// and, with credentials, `Access-Control-Allow-Credentials`).
    pub(crate) fn check(&self, url: &Url, raw: &RawResponse) -> Result<(), NetError> {
        if self.tainting != ResponseTainting::Cors {
            return Ok(());
        }
        let include = self.request.credentials == CredentialsMode::Include;
        let allowed = match header(raw, "access-control-allow-origin") {
            None => false,
            Some(origin) if origin == "*" && !include => true,
            Some(origin) if origin != self.request_origin() => false,
            Some(_) if !include => true,
            Some(_) => header(raw, "access-control-allow-credentials").as_deref() == Some("true"),
        };
        if !allowed {
            return Err(blocked(format!(
                "{url} doesn't allow {}",
                self.request_origin()
            )));
        }
        Ok(())
    }

    /// Fetch's "HTTP-redirect fetch" for a redirect from `from` to `to`: a
    /// location with credentials is a network error for a CORS request
    /// (and a `cors` one to another origin); going from another origin to
    /// a third taints the request's origin.
    pub(crate) fn redirect(&mut self, from: &Url, to: &Url) -> Result<(), NetError> {
        if includes_credentials(to)
            && ((self.request.mode == RequestMode::Cors && !same_origin(to, self.origin))
                || self.tainting == ResponseTainting::Cors)
        {
            return Err(blocked(format!("{to}: credentials in a redirect")));
        }
        if from.origin() != to.origin() && !same_origin(from, self.origin) {
            self.tainted = true;
        }
        Ok(())
    }

    /// The response, its tainting set. Fetch's filtering of what script
    /// sees (a CORS-filtered response's headers, an opaque one's status
    /// and headers) is the renderer's, as Blink's `FetchResponseData` does
    /// it, so DevTools there sees the response as it came; but an opaque
    /// `fetch()`'s body stays here: no script may read it, and machopo has no
    /// opaque response blocking to keep a renderer from it otherwise.
    pub(crate) fn filter(&self, mut response: ResourceResponse) -> ResourceResponse {
        response.tainting = self.tainting;
        if self.tainting == ResponseTainting::Opaque
            && self.request.destination == RequestDestination::Empty
        {
            response.body.clear();
        }
        response
    }
}
