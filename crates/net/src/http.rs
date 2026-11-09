//! The HTTP transport: one request out, one response back, over TCP
//! (HTTP/1.1 or HTTP/2), timed. The only module that knows about reqwest.

use std::cell::RefCell;
use std::error::Error as _;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use ipc::TimeTicks;
use protocol::network::{LoadTiming, NetError, NetErrorCode, TimeSpan};
use url::Url;

use crate::NetworkContextParams;
use crate::content_encoding;
use crate::host_mapping::{HostMapping, HostMappingRules};

/// A response as the wire gave it, before `URLLoader` looks at it.
pub(crate) struct RawResponse {
    pub(crate) status: u16,
    /// The status's canonical reason phrase (reqwest keeps no other), or ""
    /// over HTTP/2.
    pub(crate) status_text: String,
    /// Names lowercase, in order.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    pub(crate) timing: LoadTiming,
    pub(crate) remote_address: Option<SocketAddr>,
    /// `http/1.1`, `h2`... (`ResourceResponse::protocol`).
    pub(crate) protocol: &'static str,
    /// The bytes the head and the body took on the wire (the body before
    /// `Content-Encoding` was undone).
    pub(crate) encoded_headers_length: u64,
    pub(crate) encoded_body_length: u64,
}

impl RawResponse {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Cheap to clone: reqwest's client is shared behind it, and so is its
/// connection pool.
#[derive(Clone)]
pub(crate) struct HttpTransport {
    client: Result<reqwest::Client, String>,
}

impl HttpTransport {
    pub(crate) fn new(params: &NetworkContextParams) -> Self {
        let mut builder = reqwest::Client::builder()
            // `URLLoader` follows redirects itself.
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(30))
            .read_timeout(Duration::from_secs(60));
        if !params.user_agent.is_empty() {
            builder = builder.user_agent(params.user_agent.as_str());
        }
        // machopo's own resolver and a layer around reqwest's connector, both
        // to time what opening a connection takes; the resolver also maps
        // hosts by the rules, if there are any. Without extra roots,
        // reqwest's own verifier, untouched.
        builder = builder
            .dns_resolver(Arc::new(MappedHostResolver {
                rules: params.host_resolver_rules.clone(),
            }))
            .connector_layer(ConnectTiming);
        if !params.additional_trust_anchors.is_empty() {
            builder = builder.tls_certs_merge(
                params
                    .additional_trust_anchors
                    .iter()
                    // Can't fail with rustls: a certificate that doesn't
                    // parse fails `build` instead.
                    .filter_map(|der| reqwest::Certificate::from_der(der).ok()),
            );
        }
        let client = builder.build().map_err(|e| {
            tracing::error!(error = %e, "can't set up the HTTP client");
            e.to_string()
        });
        Self { client }
    }

    /// A request of `url` by `method` (a token) with `headers` (script's),
    /// `accept` as `Accept` unless they have one, and `body`. A `POST` or
    /// `PUT` without a body says `Content-Length: 0` (Fetch's
    /// "HTTP-network-or-cache fetch"). The response's body is read unless
    /// `read_body` is false (a preflight's: it is left empty).
    pub(crate) async fn send(
        &self,
        method: &str,
        url: &Url,
        accept: &str,
        headers: &[(String, String)],
        body: Option<&[u8]>,
        read_body: bool,
    ) -> Result<RawResponse, NetError> {
        let client = self
            .client
            .as_ref()
            .map_err(|e| NetError::new(NetErrorCode::Failed, e.clone()))?;
        let method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| NetError::new(NetErrorCode::Failed, format!("{method}: {e}")))?;
        let empty_body = matches!(method, reqwest::Method::POST | reqwest::Method::PUT);
        let mut request = client.request(method, url.as_str());
        match body {
            Some(body) => request = request.body(body.to_vec()),
            // Hyper sends no length for an empty body.
            None if empty_body => {
                request = request
                    .header(reqwest::header::CONTENT_LENGTH, "0")
                    .body(Vec::new());
            }
            None => {}
        }
        if !headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("accept"))
        {
            request = request.header(reqwest::header::ACCEPT, accept);
        }
        // The codings the body may come in, which `net` undoes; none for a
        // range (`URLRequestHttpJob::AddExtraHeaders`: a server could
        // compress just the range).
        let ranged = headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("range"));
        request = request.header(
            reqwest::header::ACCEPT_ENCODING,
            if ranged {
                "identity"
            } else {
                content_encoding::ACCEPT_ENCODING
            },
        );
        for (name, value) in headers {
            // A `ByteString`: each character a byte.
            let value: Vec<u8> = value.chars().map(|c| c as u8).collect();
            request = request.header(name.as_str(), value);
        }
        let request_start = TimeTicks::now();
        let (response, connecting) = CONNECTING
            .scope(RefCell::new(Connecting::default()), async {
                let response = request.send().await;
                (response, CONNECTING.with(|connecting| *connecting.borrow()))
            })
            .await;
        let response = response.map_err(|e| net_error(url, &e))?;
        let receive_headers_end = TimeTicks::now();
        let remote_address = response.remote_addr();
        let protocol = protocol_name(response.version());
        let status = response.status().as_u16();
        // HTTP/2 and later have no reason phrase.
        let status_text = if response.version() >= reqwest::Version::HTTP_2 {
            String::new()
        } else {
            response
                .status()
                .canonical_reason()
                .unwrap_or_default()
                .to_owned()
        };
        // HTTP/1 with neither a length nor chunks: the body ends where the
        // connection does.
        let close_delimited = response.version() <= reqwest::Version::HTTP_11
            && !response
                .headers()
                .contains_key(reqwest::header::CONTENT_LENGTH)
            && !response
                .headers()
                .contains_key(reqwest::header::TRANSFER_ENCODING);
        // Fetch's "isomorphic decode": each byte a character, as script
        // reads a header's value (a `ByteString`).
        let headers: Vec<(String, String)> = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    value.as_bytes().iter().map(|&b| char::from(b)).collect(),
                )
            })
            .collect();
        let mut response = response;
        let mut body = Vec::new();
        // A preflight's body isn't read (Chromium doesn't either): one that
        // never ends mustn't stall the request. Dropping the response
        // closes the connection.
        if read_body {
            loop {
                match response.chunk().await {
                    Ok(Some(chunk)) => body.extend_from_slice(&chunk),
                    Ok(None) => break,
                    // Servers that end such a body by closing the connection
                    // without TLS's close_notify (Python's `ssl`, so wptserve)
                    // are common; Chromium takes the body as complete, and so
                    // do we. A body with a length still has to have all of it.
                    Err(e) if close_delimited && is_unexpected_eof(&e) => break,
                    Err(e) => return Err(net_error(url, &e)),
                }
            }
        }
        let encoded_body_length = u64::try_from(body.len()).unwrap_or(u64::MAX);
        let content_encoding = headers_value(&headers, "content-encoding");
        let body = content_encoding::decode(content_encoding.as_deref(), body).map_err(|e| {
            NetError::new(NetErrorCode::ContentDecodingFailed, format!("{url}: {e}"))
        })?;
        let encoded_headers_length = head_length(&status_text, &headers);
        let (dns, connect) = connection_timing(connecting);
        let timing = LoadTiming {
            request_start,
            dns,
            connect,
            receive_headers_end,
            response_end: TimeTicks::now(),
        };
        Ok(RawResponse {
            status,
            status_text,
            headers,
            body,
            timing,
            remote_address,
            protocol,
            encoded_headers_length,
            encoded_body_length,
        })
    }
}

/// The value of header `name` (lowercase), the values of every such header
/// joined with ", ".
fn headers_value(headers: &[(String, String)], name: &str) -> Option<String> {
    let values: Vec<&str> = headers
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, value)| value.as_str())
        .collect();
    (!values.is_empty()).then(|| values.join(", "))
}

/// How many bytes a response's head takes as HTTP/1 writes it: the status
/// line, each header's line and the blank line.
fn head_length(status_text: &str, headers: &[(String, String)]) -> u64 {
    // "HTTP/1.1 200 OK\r\n", each "name: value\r\n", and the final "\r\n".
    let status_line = "HTTP/1.1 200 \r\n".len().saturating_add(status_text.len());
    let lines = headers
        .iter()
        .map(|(name, value)| name.len().saturating_add(value.len()).saturating_add(4))
        .fold(status_line.saturating_add(2), usize::saturating_add);
    u64::try_from(lines).unwrap_or(u64::MAX)
}

/// What opening a connection took, for the request that opened it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Connecting {
    dns: Option<TimeSpan>,
    connect: Option<TimeSpan>,
}

tokio::task_local! {
    /// The `Connecting` of the request this task sends. hyper-util opens
    /// a new connection in the task of the request that asked for it, so
    /// the resolver and the connector layer record into that request's;
    /// a connection it finishes in the background (the request went out
    /// on one that came back idle first) records nowhere.
    static CONNECTING: RefCell<Connecting>;
}

/// Records into the `Connecting` of the request this task sends, if any.
fn record(f: impl FnOnce(&mut Connecting)) {
    let _ = CONNECTING.try_with(|connecting| f(&mut connecting.borrow_mut()));
}

/// What a request reports of opening its connection: nothing unless it
/// opened the one it went out on. A lookup without a connection was for a
/// connection it didn't use (one finished in the background), so it isn't
/// the request's either. The connection is timed from where resolving
/// ended: the connector resolves before it connects.
fn connection_timing(connecting: Connecting) -> (Option<TimeSpan>, Option<TimeSpan>) {
    let Some(connect) = connecting.connect else {
        return (None, None);
    };
    let start = connecting.dns.map_or(connect.start, |dns| dns.end);
    (
        connecting.dns,
        Some(TimeSpan {
            start,
            end: connect.end,
        }),
    )
}

/// `ResourceResponse::protocol` for an HTTP version, as Chromium's
/// `HttpConnectionInfoToString` names them.
fn protocol_name(version: reqwest::Version) -> &'static str {
    match version {
        reqwest::Version::HTTP_09 => "http/0.9",
        reqwest::Version::HTTP_10 => "http/1.0",
        reqwest::Version::HTTP_2 => "h2",
        reqwest::Version::HTTP_3 => "h3",
        _ => "http/1.1",
    }
}

/// A layer around reqwest's connector (which resolves, connects and does
/// TLS) that times each connection it opens.
#[derive(Clone)]
struct ConnectTiming;

impl<S> tower_layer::Layer<S> for ConnectTiming {
    type Service = TimedConnect<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimedConnect(inner)
    }
}

#[derive(Clone)]
struct TimedConnect<S>(S);

impl<S, R> tower_service::Service<R> for TimedConnect<S>
where
    S: tower_service::Service<R>,
    S::Future: Send + 'static,
    S::Response: Send,
    S::Error: Send,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), S::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let start = TimeTicks::now();
        let connecting = self.0.call(request);
        Box::pin(async move {
            let connection = connecting.await?;
            let end = TimeTicks::now();
            record(|connecting| connecting.connect = Some(TimeSpan { start, end }));
            Ok(connection)
        })
    }
}

/// Whether the connection ended without TLS's close_notify (rustls reports
/// it as an `UnexpectedEof`).
fn is_unexpected_eof(err: &reqwest::Error) -> bool {
    let mut source = err.source();
    while let Some(e) = source {
        if e.downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::UnexpectedEof)
        {
            return true;
        }
        source = next_source(e);
    }
    false
}

/// Chromium's `MappedHostResolver`: a host the rules map resolves to their
/// address, or fails; any other goes to the system, as reqwest's default
/// resolver would (`getaddrinfo` on a blocking thread). Each lookup is
/// timed for the request whose connection asked.
struct MappedHostResolver {
    rules: HostMappingRules,
}

impl reqwest::dns::Resolve for MappedHostResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let mapping = self.rules.map(name.as_str());
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let start = TimeTicks::now();
            // Port 0: the connector puts the URL's port on each address.
            let addrs: reqwest::dns::Addrs = match mapping {
                Some(HostMapping::Address(ip)) => Box::new(std::iter::once(SocketAddr::new(ip, 0))),
                Some(HostMapping::NotFound) => {
                    return Err(format!("{host}: not found (host mapping rules)").into());
                }
                None => {
                    let addrs = tokio::net::lookup_host((host.as_str(), 0)).await?;
                    Box::new(addrs.collect::<Vec<_>>().into_iter())
                }
            };
            let end = TimeTicks::now();
            record(|connecting| connecting.dns = Some(TimeSpan { start, end }));
            Ok(addrs)
        })
    }
}

/// Maps reqwest's error to Chromium's code by looking down its source
/// chain: I/O errors by kind, rustls' certificate errors by variant, and
/// DNS failures by hyper-util's message (it has no error type for them).
fn net_error(url: &Url, err: &reqwest::Error) -> NetError {
    let detail = error_chain(err);
    let code = if err.is_timeout() {
        NetErrorCode::TimedOut
    } else if let Some(code) = code_from_sources(err) {
        code
    } else if detail.contains("dns error") {
        NetErrorCode::NameNotResolved
    } else if err.is_connect() {
        NetErrorCode::ConnectionFailed
    } else {
        NetErrorCode::Failed
    };
    NetError::new(code, format!("{url}: {detail}"))
}

fn code_from_sources(err: &reqwest::Error) -> Option<NetErrorCode> {
    let mut source = err.source();
    while let Some(e) = source {
        if let Some(io) = e.downcast_ref::<std::io::Error>() {
            match io.kind() {
                std::io::ErrorKind::ConnectionRefused => {
                    return Some(NetErrorCode::ConnectionRefused);
                }
                std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => {
                    return Some(NetErrorCode::ConnectionReset);
                }
                std::io::ErrorKind::TimedOut => return Some(NetErrorCode::TimedOut),
                _ => {}
            }
        }
        if let Some(tls) = e.downcast_ref::<rustls::Error>() {
            return Some(tls_code(tls));
        }
        source = next_source(e);
    }
    None
}

/// The error under `err`. An `io::Error`'s `source()` skips the error it
/// wraps, and TLS errors come wrapped in two of them (hyper-util's around
/// tokio-rustls'), so for those it is the wrapped error itself.
fn next_source<'a>(
    err: &'a (dyn std::error::Error + 'static),
) -> Option<&'a (dyn std::error::Error + 'static)> {
    match err.downcast_ref::<std::io::Error>() {
        Some(io) => match io.get_ref() {
            Some(inner) => Some(inner),
            None => io.source(),
        },
        None => err.source(),
    }
}

fn tls_code(err: &rustls::Error) -> NetErrorCode {
    use rustls::CertificateError as C;
    match err {
        rustls::Error::InvalidCertificate(cert) => match cert {
            C::UnknownIssuer | C::BadSignature | C::UnhandledCriticalExtension => {
                NetErrorCode::CertAuthorityInvalid
            }
            C::Expired
            | C::ExpiredContext { .. }
            | C::NotValidYet
            | C::NotValidYetContext { .. } => NetErrorCode::CertDateInvalid,
            C::NotValidForName | C::NotValidForNameContext { .. } => {
                NetErrorCode::CertCommonNameInvalid
            }
            _ => NetErrorCode::CertInvalid,
        },
        _ => NetErrorCode::SslProtocolError,
    }
}

/// `a: b: c`, for logs and error pages.
fn error_chain(err: &reqwest::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(e) = source {
        let part = e.to_string();
        if !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        source = e.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `ms` after `origin`.
    fn at(origin: TimeTicks, ms: u64) -> TimeTicks {
        origin.checked_add(Duration::from_millis(ms)).unwrap()
    }

    fn span(origin: TimeTicks, start: u64, end: u64) -> TimeSpan {
        TimeSpan {
            start: at(origin, start),
            end: at(origin, end),
        }
    }

    #[test]
    fn a_connection_is_timed_from_where_resolving_ended() {
        let o = TimeTicks::now();
        let connecting = Connecting {
            dns: Some(span(o, 2, 5)),
            connect: Some(span(o, 1, 9)),
        };
        assert_eq!(
            connection_timing(connecting),
            (Some(span(o, 2, 5)), Some(span(o, 5, 9)))
        );
    }

    #[test]
    fn a_connection_without_a_lookup_is_timed_from_its_start() {
        let o = TimeTicks::now();
        let connecting = Connecting {
            dns: None,
            connect: Some(span(o, 1, 9)),
        };
        assert_eq!(connection_timing(connecting), (None, Some(span(o, 1, 9))));
    }

    /// The request went out on a connection that came back idle while
    /// its own was being opened: that lookup was for the other one.
    #[test]
    fn a_lookup_without_a_connection_is_not_the_requests() {
        let o = TimeTicks::now();
        let connecting = Connecting {
            dns: Some(span(o, 2, 5)),
            connect: None,
        };
        assert_eq!(connection_timing(connecting), (None, None));
    }

    #[test]
    fn protocols_are_named_as_chromium_names_them() {
        assert_eq!(protocol_name(reqwest::Version::HTTP_09), "http/0.9");
        assert_eq!(protocol_name(reqwest::Version::HTTP_10), "http/1.0");
        assert_eq!(protocol_name(reqwest::Version::HTTP_11), "http/1.1");
        assert_eq!(protocol_name(reqwest::Version::HTTP_2), "h2");
        assert_eq!(protocol_name(reqwest::Version::HTTP_3), "h3");
    }
}
