//! A tiny HTTP/1.1 server for tests (Chromium's `EmbeddedTestServer`), so
//! network tests never leave the machine. Standard library only, but
//! rustls for HTTPS; one thread per connection; every response closes the
//! connection, unless it keeps connections alive
//! ([`start_keep_alive`](TestServer::start_keep_alive)). It serves fixed
//! routes.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// What the server answers for one path.
#[derive(Debug, Clone)]
pub struct Route {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// How long to wait before answering.
    pub delay: Duration,
    /// How long to wait between the head and the body.
    pub body_delay: Duration,
    /// How far into the body to stop for a while, and for how long.
    pub body_pause: Option<(usize, Duration)>,
    /// No `Content-Length`: the body ends where the connection does,
    /// closed (over HTTPS) without TLS's close_notify, as Python's `ssl`
    /// (wptserve) does.
    pub close_delimited: bool,
    /// Over HTTPS, bytes that aren't TLS after the body: the client's read
    /// fails, but not because the connection ended.
    pub corrupt_after_body: bool,
    /// Over HTTP, when the server reads the request and closes the
    /// connection without answering.
    pub unanswered: Unanswered,
    /// Answers a request whose `If-None-Match` is the route's `ETag`, or
    /// whose `If-Modified-Since` is its `Last-Modified`, with a `304`: its
    /// headers, no body.
    pub revalidates: bool,
    /// What its `304` says in place of the route's headers of the same
    /// names (a server telling a new freshness, say).
    pub not_modified_headers: Vec<(String, String)>,
}

/// When a route's request is read and its connection closed without an
/// answer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Unanswered {
    #[default]
    Never,
    /// On a connection kept alive that answered a request before: a server
    /// that closed it as the request came (wptserve closes one after a
    /// response without a length, and doesn't say so).
    OnReusedConnection,
    /// Like `OnReusedConnection`, the request left unread: the connection
    /// is reset, not closed (closing a socket with data yet to be read
    /// resets it), as when a request reaches a connection the server has
    /// closed. The request isn't recorded.
    ResetOnReusedConnection,
    Always,
}

impl Route {
    /// `status` with no headers.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body: body.into(),
            delay: Duration::ZERO,
            body_delay: Duration::ZERO,
            body_pause: None,
            close_delimited: false,
            corrupt_after_body: false,
            unanswered: Unanswered::Never,
            revalidates: false,
            not_modified_headers: Vec::new(),
        }
    }

    /// 200 with a `Content-Type`.
    pub fn ok(content_type: &str, body: impl Into<Vec<u8>>) -> Self {
        Self::new(200, body).with_header("Content-Type", content_type)
    }

    /// A redirect to `location` (absolute or relative).
    pub fn redirect(status: u16, location: &str) -> Self {
        Self::new(status, Vec::new()).with_header("Location", location)
    }

    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// See [`body_delay`](Self::body_delay).
    pub fn with_body_delay(mut self, delay: Duration) -> Self {
        self.body_delay = delay;
        self
    }

    /// See [`body_pause`](Self::body_pause).
    pub fn with_pause_in_body(mut self, at: usize, pause: Duration) -> Self {
        self.body_pause = Some((at, pause));
        self
    }

    /// See [`close_delimited`](Self::close_delimited).
    pub fn close_delimited(mut self) -> Self {
        self.close_delimited = true;
        self
    }

    /// See [`corrupt_after_body`](Self::corrupt_after_body).
    pub fn corrupt_after_body(mut self) -> Self {
        self.corrupt_after_body = true;
        self
    }

    /// See [`unanswered`](Self::unanswered).
    pub fn unanswered(mut self, when: Unanswered) -> Self {
        self.unanswered = when;
        self
    }

    /// See [`revalidates`](Self::revalidates).
    pub fn revalidated(mut self) -> Self {
        self.revalidates = true;
        self
    }

    /// Revalidated (see [`revalidated`](Self::revalidated)), its `304`
    /// saying `name: value` in place of the route's `name`.
    pub fn with_304_header(mut self, name: &str, value: &str) -> Self {
        self.revalidates = true;
        self.not_modified_headers.push((name.into(), value.into()));
        self
    }

    /// Whether a request of `headers` (names lowercase) is answered with a
    /// `304`, as [`revalidates`](Self::revalidates) says.
    fn not_modified(&self, headers: &[(String, String)]) -> bool {
        let header = |list: &[(String, String)], name: &str| {
            list.iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(name))
                .map(|(_, v)| v.clone())
        };
        let same = |asked: &str, ours: &str| {
            header(headers, asked).is_some_and(|value| header(&self.headers, ours) == Some(value))
        };
        self.revalidates
            && (same("if-none-match", "etag") || same("if-modified-since", "last-modified"))
    }
}

/// A request as the server saw it.
#[derive(Debug, Clone)]
pub struct Request {
    pub method: String,
    /// The path and query, as sent (`/a/b?c`).
    pub target: String,
    /// Names lowercase.
    pub headers: Vec<(String, String)>,
    /// As long as its `Content-Length` says (none without one).
    pub body: Vec<u8>,
    /// Over HTTP, the client closed the connection before the server
    /// answered (once the route's delay was over): it gave the request up.
    pub abandoned: bool,
    /// Which connection it came on, numbered from 1 as they were accepted.
    pub connection: u64,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

/// What a server answers with.
struct Site {
    routes: HashMap<String, Route>,
    /// Serves HTTPS with this, or HTTP without.
    tls: Option<Arc<rustls::ServerConfig>>,
    /// Over HTTP, a connection stays open for the next request after a
    /// response with a length.
    keep_alive: bool,
}

/// Serves `routes` on `127.0.0.1` until dropped. Unknown paths get a 404.
pub struct TestServer {
    addr: SocketAddr,
    https: bool,
    requests: Arc<Mutex<Vec<Request>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl TestServer {
    /// `routes` maps a path (without the query) to its response, or a
    /// method and a path (`OPTIONS /a`) to its response for that method.
    pub fn start(routes: HashMap<String, Route>) -> std::io::Result<Self> {
        Self::serve_site(Site {
            routes,
            tls: None,
            keep_alive: false,
        })
    }

    /// Like [`start`](Self::start), keeping each connection open for more
    /// requests (`Connection: keep-alive`) unless a route's body ends with
    /// the connection, so a client can reuse it.
    pub fn start_keep_alive(routes: HashMap<String, Route>) -> std::io::Result<Self> {
        Self::serve_site(Site {
            routes,
            tls: None,
            keep_alive: true,
        })
    }

    /// Like [`start`](Self::start), over HTTPS: `certificate_chain` and
    /// `private_key` are PEM, the server's certificate first.
    pub fn start_https(
        routes: HashMap<String, Route>,
        certificate_chain: &[u8],
        private_key: &[u8],
    ) -> std::io::Result<Self> {
        use rustls::pki_types::pem::PemObject;
        use rustls::pki_types::{CertificateDer, PrivateKeyDer};
        let invalid = |e: &dyn std::fmt::Display| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
        };
        let chain = CertificateDer::pem_slice_iter(certificate_chain)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| invalid(&e))?;
        let key = PrivateKeyDer::from_pem_slice(private_key).map_err(|e| invalid(&e))?;
        let config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::aws_lc_rs::default_provider(),
        ))
        .with_safe_default_protocol_versions()
        .map_err(|e| invalid(&e))?
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map_err(|e| invalid(&e))?;
        Self::serve_site(Site {
            routes,
            tls: Some(Arc::new(config)),
            keep_alive: false,
        })
    }

    fn serve_site(site: Site) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let https = site.tls.is_some();
        let routes = Arc::new(site);
        let thread = {
            let requests = requests.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("TestServer".into())
                .spawn(move || {
                    let mut connection: u64 = 0;
                    for stream in listener.incoming() {
                        if stop.load(Ordering::Relaxed) {
                            break;
                        }
                        let Ok(stream) = stream else { continue };
                        connection = connection.saturating_add(1);
                        let routes = routes.clone();
                        let requests = requests.clone();
                        std::thread::spawn(move || accept(stream, &routes, &requests, connection));
                    }
                })?
        };
        Ok(Self {
            addr,
            https,
            requests,
            stop,
            thread: Some(thread),
        })
    }

    /// `http://127.0.0.1:PORT` + `path` (`https:` for
    /// [`start_https`](Self::start_https)).
    pub fn url(&self, path: &str) -> String {
        self.url_for_host(&self.addr.ip().to_string(), path)
    }

    /// Like [`url`](Self::url), with `host` for `127.0.0.1`: a name that
    /// resolves here (host mapping rules), or that a certificate names.
    pub fn url_for_host(&self, host: &str, path: &str) -> String {
        let scheme = if self.https { "https" } else { "http" };
        format!("{scheme}://{host}:{}{path}", self.addr.port())
    }

    /// Every request so far, in arrival order.
    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    /// Whether a request of `target` was abandoned, waiting up to
    /// `timeout` for it to be (the server finds out once its route's delay
    /// is over).
    pub fn wait_for_abandoned(&self, target: &str, timeout: Duration) -> bool {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "a test's timeout from now: seconds, within any clock's range"
        )]
        let deadline = std::time::Instant::now() + timeout;
        loop {
            if self
                .requests()
                .iter()
                .any(|r| r.target == target && r.abandoned)
            {
                return true;
            }
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the accept loop so it sees `stop`.
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A port of this machine that no server listens on, nor can while it is
/// kept: bound and not listening, so a connection to it is refused at once
/// (Chromium's `EmbeddedTestServer` holds its port the same way before it
/// listens). Kept for as long as the test that wants a refused connection
/// runs: a port let go is another test's server's to take, as Linux hands
/// out a free port from a random place in its range.
///
/// On macOS it isn't kept: its TCP (BSD's) drops a connection's first
/// segment to a socket that is bound and not listening, without a reset,
/// so the connection would wait for its timeout instead of being refused.
/// There a port let go is safe enough: macOS hands out free ports in turn
/// (from 49152 up), so one comes back only once the range has gone round.
pub struct RefusedPort {
    // Bound, never listening: what keeps the port (none on macOS).
    _socket: Option<tokio::net::TcpSocket>,
    port: u16,
}

impl RefusedPort {
    pub fn port(&self) -> u16 {
        self.port
    }
}

/// A port connections to are refused, kept till the guard goes.
pub fn refused_port() -> RefusedPort {
    let socket = tokio::net::TcpSocket::new_v4().expect("a socket");
    socket
        .bind(std::net::SocketAddr::from(([127, 0, 0, 1], 0)))
        .expect("bind a free port");
    let port = socket.local_addr().expect("local address").port();
    // macOS: let go, to be refused (above).
    let socket = (!cfg!(target_os = "macos")).then_some(socket);
    RefusedPort {
        _socket: socket,
        port,
    }
}

fn accept(mut stream: TcpStream, site: &Site, requests: &Mutex<Vec<Request>>, connection: u64) {
    let Some(tls) = &site.tls else {
        let probe = stream.try_clone().ok();
        // The client closed its end: a read finds the end at once.
        let closed = move || {
            let Some(probe) = &probe else {
                return false;
            };
            if probe.set_nonblocking(true).is_err() {
                return false;
            }
            let closed = matches!(probe.peek(&mut [0]), Ok(0));
            let _ = probe.set_nonblocking(false);
            closed
        };
        let mut reused = false;
        while let Some(route) = serve(&mut stream, site, requests, connection, reused, &closed) {
            if !site.keep_alive || route.close_delimited || resets_next(&stream, site) {
                break;
            }
            reused = true;
        }
        return;
    };
    let Ok(tls_connection) = rustls::ServerConnection::new(tls.clone()) else {
        return;
    };
    let mut stream = rustls::StreamOwned::new(tls_connection, stream);
    // A handshake the client gives up on (a certificate it doesn't trust)
    // fails the first read, and nothing is recorded.
    match serve(&mut stream, site, requests, connection, false, &|| false) {
        Some(route) if route.corrupt_after_body => {
            // A whole application data record that doesn't decrypt, after
            // the client has had the head and the body: it fails reading
            // on, not reading the head.
            std::thread::sleep(Duration::from_millis(200));
            let mut record = vec![0x17, 0x03, 0x03, 0x00, 0x20];
            record.extend([0xab; 0x20]);
            let _ = stream.sock.write_all(&record);
            let _ = stream.sock.flush();
        }
        Some(route) if route.close_delimited => {}
        _ => {
            stream.conn.send_close_notify();
            let _ = stream.flush();
        }
    }
}

/// Whether the next request on `stream`, which answered one before, is of
/// a route that resets it: looked at without reading it, so that it stays
/// unread as the connection is dropped. Waits for it to come.
fn resets_next(stream: &TcpStream, site: &Site) -> bool {
    let mut head = [0; 4096];
    loop {
        let Ok(length) = stream.peek(&mut head) else {
            return false;
        };
        let peeked = &head[..length];
        if length == 0 {
            return false;
        }
        let Some(end) = peeked.iter().position(|&b| b == b'\n') else {
            if length == head.len() {
                return false;
            }
            // The rest of its line is still to come.
            std::thread::sleep(Duration::from_millis(1));
            continue;
        };
        let line = String::from_utf8_lossy(&peeked[..end]);
        let mut words = line.split_whitespace();
        let method = words.next().unwrap_or("GET");
        let path = words.next().unwrap_or("/").split('?').next().unwrap_or("/");
        let route = site
            .routes
            .get(&format!("{method} {path}"))
            .or_else(|| site.routes.get(path));
        return route.is_some_and(|route| route.unanswered == Unanswered::ResetOnReusedConnection);
    }
}

/// Answers one request, which came on `connection` (`reused`: it answered
/// one before), with its route, which it returns (`None` if no request
/// came, or the route leaves it unanswered): how the connection ends
/// depends on it. Unless `closed` says the client gave up meanwhile: the
/// request is then abandoned.
fn serve(
    stream: &mut (impl Read + Write),
    site: &Site,
    requests: &Mutex<Vec<Request>>,
    connection: u64,
    reused: bool,
    closed: &dyn Fn() -> bool,
) -> Option<Route> {
    // A request's body is as long as its `Content-Length` says; once it is
    // read, the rest of the stream is only written.
    let mut reader = BufReader::new(stream);
    // A line of the head, each byte a character (HTTP's header values are
    // bytes, which `ByteString`s give as Latin-1).
    let mut read_line = || {
        let mut bytes = Vec::new();
        match reader.read_until(b'\n', &mut bytes) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(bytes.iter().map(|&b| char::from(b)).collect::<String>()),
        }
    };
    let line = read_line()?;
    let mut words = line.split_whitespace();
    let method = words.next().unwrap_or("GET").to_owned();
    let target = words.next().unwrap_or("/").to_owned();
    let mut headers = Vec::new();
    while let Some(line) = read_line() {
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    let path = target.split('?').next().unwrap_or("/").to_owned();
    let head_only = method == "HEAD";
    let asked = headers.clone();
    // A route for the method (`OPTIONS /a`) before the path's.
    let route_key = format!("{method} {path}");
    let index = {
        let mut requests = requests.lock().unwrap();
        let index = requests.len();
        requests.push(Request {
            method,
            target,
            headers,
            body,
            abandoned: false,
            connection,
        });
        index
    };

    let not_found = Route::new(404, "not found").with_header("Content-Type", "text/plain");
    let route = site
        .routes
        .get(&route_key)
        .or_else(|| site.routes.get(&path))
        .unwrap_or(&not_found);
    match route.unanswered {
        Unanswered::Always => return None,
        Unanswered::OnReusedConnection if reused => return None,
        _ => {}
    }
    std::thread::sleep(route.delay);
    if closed() {
        requests.lock().unwrap()[index].abandoned = true;
        return None;
    }
    let not_modified = route.not_modified(&asked);
    let status = if not_modified { 304 } else { route.status };
    let head_only = head_only || not_modified;
    let keep_alive = site.keep_alive && !route.close_delimited;
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let mut head = format!("HTTP/1.1 {status} X\r\nConnection: {connection}\r\n");
    if !route.close_delimited && !not_modified {
        head.push_str(&format!("Content-Length: {}\r\n", route.body.len()));
    }
    let replaced = |name: &str| {
        not_modified
            && route
                .not_modified_headers
                .iter()
                .any(|(other, _)| other.eq_ignore_ascii_case(name))
    };
    for (name, value) in route.headers.iter().filter(|(name, _)| !replaced(name)) {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    if not_modified {
        for (name, value) in &route.not_modified_headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    let stream = reader.get_mut();
    let _ = stream.write_all(head.as_bytes());
    if !route.body_delay.is_zero() {
        let _ = stream.flush();
        std::thread::sleep(route.body_delay);
    }
    if !head_only {
        let (first, rest) = match route.body_pause {
            Some((at, _)) => route.body.split_at(at.min(route.body.len())),
            None => (&route.body[..], &[][..]),
        };
        let _ = stream.write_all(first);
        if let Some((_, pause)) = route.body_pause {
            let _ = stream.flush();
            std::thread::sleep(pause);
        }
        let _ = stream.write_all(rest);
    }
    let _ = stream.flush();
    Some(route.clone())
}
