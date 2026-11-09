//! Does a tokio task-local around one request's `send` tell which DNS and
//! connect timings belong to that request? reqwest's resolver
//! (`dns_resolver`) and a connector layer (`connector_layer`) record into
//! the task-local of whatever task polls them; hyper-util polls a new
//! connection in the task of the request that asked for it, unless an idle
//! connection turns up first, when the new one is finished in a background
//! task. A request's timing counts only if its connect was recorded: a DNS
//! lookup alone is from a connection it didn't end up using.
//!
//! The servers say which connection served a request (`x-conn`) and how
//! many requests that connection had served (`x-nth`), so the run checks:
//! a request with a timing was the first on its connection, and every
//! connection's timing went to at most one request.

use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::Infallible;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

tokio::task_local! {
    static TIMING: RefCell<Timing>;
}

#[derive(Debug, Default, Clone, Copy)]
struct Timing {
    dns: Option<(Instant, Instant)>,
    connect: Option<(Instant, Instant)>,
}

fn record(f: impl FnOnce(&mut Timing)) {
    // Not in a request's task (a connection finished in the background):
    // nobody's.
    let _ = TIMING.try_with(|timing| f(&mut timing.borrow_mut()));
}

struct TimedResolver;

impl reqwest::dns::Resolve for TimedResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let start = Instant::now();
            // WPT's certificate is for web-platform.test.
            let host = if host == "web-platform.test" { "localhost".to_owned() } else { host };
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .collect();
            record(|timing| timing.dns = Some((start, Instant::now())));
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

/// Times each connection made; `delay` makes connecting slow, for the race.
#[derive(Clone)]
struct TimingLayer {
    delay: Duration,
}

impl<S> tower_layer::Layer<S> for TimingLayer {
    type Service = TimedConnect<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TimedConnect {
            inner,
            delay: self.delay,
        }
    }
}

#[derive(Clone)]
struct TimedConnect<S> {
    inner: S,
    delay: Duration,
}

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
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, request: R) -> Self::Future {
        let start = Instant::now();
        let delay = self.delay;
        let connecting = self.inner.call(request);
        Box::pin(async move {
            let connection = connecting.await?;
            tokio::time::sleep(delay).await;
            record(|timing| timing.connect = Some((start, Instant::now())));
            Ok(connection)
        })
    }
}

/// Serves on 127.0.0.1, HTTP/2 (cleartext) or HTTP/1.1 with keep-alive; or
/// with `tls`, HTTPS with whichever ALPN picks (`h2` or `http/1.1`).
/// `?delay=ms` waits before answering. Returns the port and the number of
/// connections accepted so far.
async fn server(h2: bool) -> (u16, Arc<AtomicU64>) {
    serve(h2, None).await
}

fn wpt_cert(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../third_party/wpt/tools/certs")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn tls_config() -> Arc<rustls::ServerConfig> {
    use rustls::pki_types::pem::PemObject;
    use rustls::pki_types::{CertificateDer, PrivateKeyDer};
    let chain = CertificateDer::pem_slice_iter(&wpt_cert("web-platform.test.pem"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(&wpt_cert("web-platform.test.key")).unwrap();
    let mut config =
        rustls::ServerConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Arc::new(config)
}

async fn serve(h2: bool, tls: Option<Arc<rustls::ServerConfig>>) -> (u16, Arc<AtomicU64>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let accepted = Arc::new(AtomicU64::new(0));
    let count = accepted.clone();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            let conn = count.fetch_add(1, Ordering::SeqCst) + 1;
            let served = Arc::new(AtomicU64::new(0));
            let tls = tls.clone();
            tokio::spawn(async move {
                let (io, h2) = match tls {
                    None => (Io::Plain(stream), h2),
                    Some(config) => {
                        let stream = tokio_rustls::TlsAcceptor::from(config)
                            .accept(stream)
                            .await
                            .unwrap();
                        let h2 = stream.get_ref().1.alpn_protocol() == Some(b"h2");
                        (Io::Tls(Box::new(stream)), h2)
                    }
                };
                let io = hyper_util::rt::TokioIo::new(io);
                let service = hyper::service::service_fn(move |request: hyper::Request<_>| {
                    let nth = served.fetch_add(1, Ordering::SeqCst) + 1;
                    let delay = request
                        .uri()
                        .query()
                        .and_then(|q| q.strip_prefix("delay="))
                        .and_then(|ms| ms.parse().ok())
                        .unwrap_or(0);
                    let _: &hyper::Request<hyper::body::Incoming> = &request;
                    async move {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                        Ok::<_, Infallible>(
                            hyper::Response::builder()
                                .header("x-conn", conn.to_string())
                                .header("x-nth", nth.to_string())
                                .body(http_body_util::Full::new(hyper::body::Bytes::from_static(
                                    b"ok",
                                )))
                                .unwrap(),
                        )
                    }
                });
                let _ = if h2 {
                    hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                        .serve_connection(io, service)
                        .await
                } else {
                    hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service)
                        .await
                };
            });
        }
    });
    (port, accepted)
}

/// A server connection, with or without TLS.
enum Io {
    Plain(tokio::net::TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<tokio::net::TcpStream>>),
}

impl tokio::io::AsyncRead for Io {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl tokio::io::AsyncWrite for Io {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<std::io::Result<usize>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_flush(cx),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        match self.get_mut() {
            Io::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Io::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// HTTPS as machopo would send it: ALPN offers `h2` and `http/1.1`.
fn https_client(connect_delay: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .no_proxy()
        .tls_certs_merge(reqwest::Certificate::from_pem(&wpt_cert("cacert.pem")))
        .dns_resolver(Arc::new(TimedResolver))
        .connector_layer(TimingLayer {
            delay: connect_delay,
        })
        .build()
        .unwrap()
}

/// As machopo's `HttpTransport` builds it, plus the timing hooks.
fn client(h2: bool, connect_delay: Duration) -> reqwest::Client {
    let builder = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .no_proxy()
        .dns_resolver(Arc::new(TimedResolver))
        .connector_layer(TimingLayer {
            delay: connect_delay,
        });
    let builder = if h2 {
        builder.http2_prior_knowledge()
    } else {
        builder.http1_only()
    };
    builder.build().unwrap()
}

#[derive(Debug)]
struct Outcome {
    name: &'static str,
    conn: u64,
    nth: u64,
    timing: Timing,
}

impl Outcome {
    /// What machopo would report: a timing only with a connect.
    fn counted(&self) -> bool {
        self.timing.connect.is_some()
    }
}

/// One request, in its own task-local (`localhost`, so the resolver runs).
async fn get(name: &'static str, client: reqwest::Client, url: String) -> Outcome {
    TIMING
        .scope(RefCell::new(Timing::default()), async move {
            let response = client.get(&url).send().await.unwrap();
            let header = |name: &str| -> u64 {
                response.headers()[name].to_str().unwrap().parse().unwrap()
            };
            let (conn, nth) = (header("x-conn"), header("x-nth"));
            response.bytes().await.unwrap();
            Outcome {
                name,
                conn,
                nth,
                timing: TIMING.with(|timing| *timing.borrow()),
            }
        })
        .await
}

fn show(scenario: &str, outcomes: &[Outcome], connections: u64) {
    println!("{scenario}: {connections} connection(s)");
    for o in outcomes {
        let span = |s: Option<(Instant, Instant)>| {
            s.map_or("-".to_owned(), |(a, b)| format!("{:?}", b - a))
        };
        println!(
            "  {:<3} conn {} nth {} dns {:<12} connect {:<12} counted {}",
            o.name,
            o.conn,
            o.nth,
            span(o.timing.dns),
            span(o.timing.connect),
            o.counted()
        );
    }
}

/// The checks that hold whatever the interleaving: a counted request was
/// the first on its connection, no connection is counted twice, and its
/// DNS lookup, if any, lies inside its connect.
fn check(scenario: &str, outcomes: &[Outcome]) -> bool {
    let mut ok = true;
    let mut counted_per_conn: HashMap<u64, u32> = HashMap::new();
    for o in outcomes.iter().filter(|o| o.counted()) {
        if o.nth != 1 {
            println!("  FAIL {scenario}: {} counted but was request {} on conn {}", o.name, o.nth, o.conn);
            ok = false;
        }
        *counted_per_conn.entry(o.conn).or_default() += 1;
        if let (Some((ds, de)), Some((cs, ce))) = (o.timing.dns, o.timing.connect) {
            if ds < cs || de > ce {
                println!("  FAIL {scenario}: {}'s DNS isn't inside its connect", o.name);
                ok = false;
            }
        }
    }
    for (conn, n) in counted_per_conn {
        if n > 1 {
            println!("  FAIL {scenario}: conn {conn} counted {n} times");
            ok = false;
        }
    }
    ok
}

fn expect(scenario: &str, what: &str, holds: bool) -> bool {
    if !holds {
        println!("  FAIL {scenario}: {what}");
    }
    holds
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut ok = true;

    // HTTP/1.1, one after another: the first opens the connection, the
    // others reuse it.
    {
        let (port, accepted) = server(false).await;
        let url = format!("http://localhost:{port}/");
        let c = client(false, Duration::ZERO);
        let mut outcomes = Vec::new();
        for name in ["a", "b", "c"] {
            outcomes.push(get(name, c.clone(), url.clone()).await);
        }
        let n = accepted.load(Ordering::SeqCst);
        show("h1 sequential", &outcomes, n);
        ok &= check("h1 sequential", &outcomes);
        ok &= expect("h1 sequential", "one connection", n == 1);
        ok &= expect(
            "h1 sequential",
            "only the first counted",
            outcomes.iter().map(Outcome::counted).collect::<Vec<_>>() == [true, false, false],
        );
    }

    // HTTP/1.1, five at once on slow answers: five connections, each
    // request counts its own.
    {
        let (port, accepted) = server(false).await;
        let url = format!("http://localhost:{port}/?delay=100");
        let c = client(false, Duration::ZERO);
        let names = ["a", "b", "c", "d", "e"];
        let tasks: Vec<_> = names
            .iter()
            .map(|&name| tokio::spawn(get(name, c.clone(), url.clone())))
            .collect();
        let mut outcomes = Vec::new();
        for task in tasks {
            outcomes.push(task.await.unwrap());
        }
        let n = accepted.load(Ordering::SeqCst);
        show("h1 concurrent", &outcomes, n);
        ok &= check("h1 concurrent", &outcomes);
        ok &= expect("h1 concurrent", "five connections", n == 5);
        ok &= expect("h1 concurrent", "all counted", outcomes.iter().all(Outcome::counted));
    }

    // HTTP/1.1, the race: connecting takes 300 ms. `a` opens conn 1 and is
    // answered at once; `b` starts 150 ms later and opens its own, but conn
    // 1 comes back idle first, so `b` goes out on it and its own connection
    // is finished in the background: `b` must not count (its DNS lookup
    // was recorded, its connect wasn't).
    {
        let (port, accepted) = server(false).await;
        let url = format!("http://localhost:{port}/");
        let c = client(false, Duration::from_millis(300));
        let a = tokio::spawn(get("a", c.clone(), url.clone()));
        tokio::time::sleep(Duration::from_millis(150)).await;
        let b = tokio::spawn(get("b", c.clone(), url.clone()));
        let outcomes = vec![a.await.unwrap(), b.await.unwrap()];
        // Let the background connection finish, then use both.
        tokio::time::sleep(Duration::from_millis(400)).await;
        let n = accepted.load(Ordering::SeqCst);
        show("h1 race", &outcomes, n);
        ok &= check("h1 race", &outcomes);
        ok &= expect("h1 race", "a counted", outcomes[0].counted());
        ok &= expect(
            "h1 race",
            "b reused conn 1 and isn't counted",
            outcomes[1].conn == outcomes[0].conn && !outcomes[1].counted(),
        );
        ok &= expect(
            "h1 race",
            "b's DNS lookup was recorded, so dropping it matters",
            outcomes[1].timing.dns.is_some(),
        );
        ok &= expect("h1 race", "two connections", n == 2);
    }

    // HTTP/2: five at once share one connection; exactly one counts it.
    // Then one more reuses it.
    {
        let (port, accepted) = server(true).await;
        let url = format!("http://localhost:{port}/?delay=100");
        let c = client(true, Duration::from_millis(50));
        let names = ["a", "b", "c", "d", "e"];
        let tasks: Vec<_> = names
            .iter()
            .map(|&name| tokio::spawn(get(name, c.clone(), url.clone())))
            .collect();
        let mut outcomes = Vec::new();
        for task in tasks {
            outcomes.push(task.await.unwrap());
        }
        outcomes.push(get("f", c.clone(), url.clone()).await);
        let n = accepted.load(Ordering::SeqCst);
        show("h2", &outcomes, n);
        ok &= expect("h2", "one connection", n == 1);
        ok &= expect(
            "h2",
            "exactly one counted",
            outcomes.iter().filter(|o| o.counted()).count() == 1,
        );
        ok &= expect("h2", "the later one isn't counted", !outcomes[5].counted());
        // Over HTTP/2 the counted request needn't be the first stream to
        // arrive, so only the per-connection check applies.
        let mut counted = HashMap::new();
        for o in outcomes.iter().filter(|o| o.counted()) {
            *counted.entry(o.conn).or_insert(0) += 1;
        }
        ok &= expect("h2", "no connection counted twice", counted.values().all(|&n| n == 1));
    }

    // HTTPS: ALPN picks HTTP/2, which the pool learns only once a
    // connection is up, so each of the five at once may open its own;
    // whoever ends up on which, no connection is counted twice, and a
    // counted request went out on a connection it opened. Then one more
    // reuses one.
    {
        let (port, accepted) = serve(true, Some(tls_config())).await;
        let url = format!("https://web-platform.test:{port}/?delay=100");
        let c = https_client(Duration::from_millis(50));
        let names = ["a", "b", "c", "d", "e"];
        let tasks: Vec<_> = names
            .iter()
            .map(|&name| tokio::spawn(get(name, c.clone(), url.clone())))
            .collect();
        let mut outcomes = Vec::new();
        for task in tasks {
            outcomes.push(task.await.unwrap());
        }
        outcomes.push(get("f", c.clone(), url.clone()).await);
        let n = accepted.load(Ordering::SeqCst);
        show("h2 over TLS (ALPN)", &outcomes, n);
        let mut counted = HashMap::new();
        for o in outcomes.iter().filter(|o| o.counted()) {
            *counted.entry(o.conn).or_insert(0) += 1;
        }
        ok &= expect("h2 tls", "no connection counted twice", counted.values().all(|&n| n == 1));
        ok &= expect(
            "h2 tls",
            "at most as many counted as connections",
            outcomes.iter().filter(|o| o.counted()).count() as u64 <= n,
        );
        ok &= expect("h2 tls", "something counted", !counted.is_empty());
        ok &= expect("h2 tls", "the later one isn't counted", !outcomes[5].counted());
    }

    println!("{}", if ok { "ALL OK" } else { "SOME FAILED" });
    std::process::exit(if ok { 0 } else { 1 });
}
