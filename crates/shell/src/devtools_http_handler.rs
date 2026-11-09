//! `machopo --remote-debugging-port`: the DevTools protocol over HTTP and
//! websockets, as Chromium's `DevToolsHttpHandler` serves it, on a thread
//! of its own (`DevToolsHandlerThread`, Chromium's
//! `Chrome_DevToolsHandlerThread`). Chrome's DevTools (`chrome://inspect`)
//! and other CDP clients find the tabs at `/json/list` and connect to the
//! browser (`/devtools/browser/<id>`, flat sessions, as the pipe) or
//! straight to a tab (`/devtools/page/<target id>`).
//!
//! What the endpoints and the connections ask goes to the UI thread's
//! [`AutomationSession`](crate::automation_session::AutomationSession) as
//! [`ClientEvent`]s; this thread only speaks HTTP and websockets.
//!
//! The port is the whole browser's control, so, as Chrome: it listens on
//! 127.0.0.1 only; a request whose `Host` is a name other than `localhost`
//! is refused (a page whose domain resolves to 127.0.0.1 would otherwise be
//! same-origin with this server: DNS rebinding); a websocket from a page
//! (it has an `Origin`) is refused unless `--remote-allow-origins` lets
//! that origin in (a page can open a websocket anywhere); `/json/new`,
//! which opens a tab, takes only `PUT` (a page's `<img>` can make a GET).
//! The target ids, which `/json/close/<id>` takes from a GET, are random.

use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use http_body_util::Full;
use hyper::body::{Bytes, Incoming};
use hyper::header::{
    CONNECTION, CONTENT_TYPE, HOST, HeaderValue, ORIGIN, SEC_WEBSOCKET_ACCEPT, SEC_WEBSOCKET_KEY,
    SEC_WEBSOCKET_VERSION, UPGRADE,
};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use protocol::automation::{Event, EventKind, Message, TargetInfo};
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message as Frame;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};

use crate::automation_pipe::{self, MAX_MESSAGE_SIZE};
use crate::automation_session::{ClientEvent, ClientId, JsonAnswer, JsonRequest, Outbox};

/// The largest websocket frame a client may send; a message may be
/// several (tungstenite's default, as the message's limit is).
const MAX_FRAME_SIZE: usize = 16 << 20;

/// The origins whose pages may open a websocket (`--remote-allow-origins`).
/// Clients that aren't pages send no `Origin` and are always let in.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum AllowedOrigins {
    /// Only DevTools' own pages (`devtools://devtools`).
    #[default]
    None,
    /// `*`: any.
    Any,
    /// These, as `--remote-allow-origins` lists them, comma-separated.
    List(Vec<String>),
}

impl AllowedOrigins {
    /// `--remote-allow-origins`'s value: `*`, or origins separated by
    /// commas.
    pub fn parse(value: &str) -> Self {
        let origins: Vec<String> = value
            .split(',')
            .map(|origin| origin.trim().trim_end_matches('/').to_ascii_lowercase())
            .filter(|origin| !origin.is_empty())
            .collect();
        if origins.iter().any(|origin| origin == "*") {
            Self::Any
        } else {
            Self::List(origins)
        }
    }

    /// Whether a websocket that came with `origin` may connect. Chrome's
    /// DevTools, opened from a `devtools://` page, always may: no web page
    /// can claim that origin.
    pub fn allows(&self, origin: Option<&str>) -> bool {
        let Some(origin) = origin else {
            return true;
        };
        let origin = origin.trim_end_matches('/').to_ascii_lowercase();
        if origin == "devtools://devtools" {
            return true;
        }
        match self {
            Self::None => false,
            Self::Any => true,
            Self::List(origins) => origins.contains(&origin),
        }
    }
}

/// Whether a request that came with the `Host` header `host` is for this
/// server by its address or `localhost`, not by some other name (Chromium's
/// `DevToolsHttpHandler` lets only these in).
pub fn is_allowed_host(host: &str) -> bool {
    let name = if let Some(rest) = host.strip_prefix('[') {
        // An IPv6 literal, maybe with a port after the bracket.
        match rest.split_once(']') {
            Some((address, port)) if port.is_empty() || port.starts_with(':') => address,
            _ => return false,
        }
    } else {
        host.rsplit_once(':').map_or(host, |(name, _)| name)
    };
    name.parse::<IpAddr>().is_ok() || name.eq_ignore_ascii_case("localhost")
}

/// The HTTP server's settings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Options {
    /// 0: one the system picks.
    pub port: u16,
    pub allowed_origins: AllowedOrigins,
}

/// The server, until it drops. Drop it once the session has gone: its
/// websockets then have all they will be sent, and it waits (a while) for
/// them to send it.
pub struct DevToolsHttpHandler {
    port: u16,
    browser_id: String,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

/// How long the server waits, as it stops, for its websockets to send what
/// they were given (the answer to `Browser.close`, say).
const FLUSH_TIMEOUT: Duration = Duration::from_secs(5);

/// What every request needs.
struct Context {
    port: u16,
    browser_id: String,
    allowed_origins: AllowedOrigins,
    post: Box<dyn Fn(ClientEvent) -> bool + Send + Sync>,
    /// The websockets' tasks, waited for as the server stops.
    connections: Mutex<JoinSet<()>>,
}

impl DevToolsHttpHandler {
    /// Listens on 127.0.0.1 at `options.port`, telling standard error where
    /// as Chrome does (Puppeteer and Playwright read that line); what the
    /// endpoints and connections ask goes to `post`, which says whether the
    /// browser is still there.
    pub fn start(
        options: &Options,
        post: impl Fn(ClientEvent) -> bool + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        let listener = TcpListener::bind(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            options.port,
        ))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let browser_id = browser_id();
        let context = Arc::new(Context {
            port,
            browser_id: browser_id.clone(),
            allowed_origins: options.allowed_origins.clone(),
            post: Box::new(post),
            connections: Mutex::new(JoinSet::new()),
        });
        let (stop, stopped) = oneshot::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("DevToolsHandlerThread".into())
            .spawn(move || serve(listener, context, stopped))?;
        let handler = Self {
            port,
            browser_id,
            stop: Some(stop),
            thread: Some(thread),
        };
        eprintln!("\nDevTools listening on {}\n", handler.browser_url());
        Ok(handler)
    }

    /// The port it listens on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Where a client connects to the browser.
    pub fn browser_url(&self) -> String {
        format!(
            "ws://127.0.0.1:{}/devtools/browser/{}",
            self.port, self.browser_id
        )
    }
}

impl Drop for DevToolsHttpHandler {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The browser's id in its websocket's URL: random, as Chrome's is, in the
/// form of a UUID.
fn browser_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).expect("the operating system gives random numbers");
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}

/// Accepts connections until `stopped`, then lets the websockets send what
/// they have left.
fn serve(listener: TcpListener, context: Arc<Context>, mut stopped: oneshot::Receiver<()>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .expect("a tokio runtime");
    runtime.block_on(async move {
        let listener = tokio::net::TcpListener::from_std(listener).expect("a nonblocking listener");
        loop {
            let stream = tokio::select! {
                _ = &mut stopped => break,
                accepted = listener.accept() => match accepted {
                    Ok((stream, _)) => stream,
                    Err(_) => continue,
                },
            };
            let context = context.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request: Request<Incoming>| {
                    let context = context.clone();
                    async move { Ok::<_, Infallible>(handle(request, context).await) }
                });
                let _ = http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .with_upgrades()
                    .await;
            });
        }
        // A websocket ends once its outbox has gone with the session,
        // having sent what was in it.
        let mut connections = std::mem::take(
            &mut *context
                .connections
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        let _ = tokio::time::timeout(FLUSH_TIMEOUT, async {
            while connections.join_next().await.is_some() {}
        })
        .await;
    });
}

fn respond(status: StatusCode, content_type: &str, body: String) -> Response<Full<Bytes>> {
    Response::builder()
        .status(status)
        .header(CONTENT_TYPE, content_type)
        .body(Full::new(Bytes::from(body)))
        .expect("a response")
}

fn text(status: StatusCode, body: impl Into<String>) -> Response<Full<Bytes>> {
    respond(status, "text/plain; charset=UTF-8", body.into())
}

fn json_response(value: &Value) -> Response<Full<Bytes>> {
    respond(
        StatusCode::OK,
        "application/json; charset=UTF-8",
        value.to_string(),
    )
}

fn header(request: &Request<Incoming>, name: hyper::header::HeaderName) -> Option<&str> {
    request
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
}

async fn handle(mut request: Request<Incoming>, context: Arc<Context>) -> Response<Full<Bytes>> {
    if let Some(host) = header(&request, HOST)
        && !is_allowed_host(host)
    {
        return text(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Host header is specified and is not an IP address or localhost.",
        );
    }
    let path = request.uri().path().trim_end_matches('/').to_owned();
    let query = request.uri().query().unwrap_or_default().to_owned();
    if let Some(id) = path.strip_prefix("/devtools/browser/") {
        if id != context.browser_id {
            return text(StatusCode::NOT_FOUND, format!("No such target id: {id}"));
        }
        return upgrade(&mut request, &context, None).await;
    }
    if let Some(id) = path.strip_prefix("/devtools/page/") {
        let id = id.to_owned();
        return upgrade(&mut request, &context, Some(id)).await;
    }
    let request = match path.as_str() {
        "/json/version" => JsonRequest::Version,
        "/json" | "/json/list" => JsonRequest::List,
        "/json/new" => {
            if request.method() != Method::PUT {
                return text(
                    StatusCode::METHOD_NOT_ALLOWED,
                    format!(
                        "Using unsafe HTTP verb {} to invoke /json/new. This action supports \
                         only PUT verb.",
                        request.method()
                    ),
                );
            }
            let url = percent_encoding::percent_decode_str(&query)
                .decode_utf8_lossy()
                .into_owned();
            JsonRequest::New(if url.is_empty() {
                "about:blank".into()
            } else {
                url
            })
        }
        _ => {
            if let Some(id) = path.strip_prefix("/json/activate/") {
                JsonRequest::Activate(id.to_owned())
            } else if let Some(id) = path.strip_prefix("/json/close/") {
                JsonRequest::Close(id.to_owned())
            } else {
                return text(StatusCode::NOT_FOUND, format!("Unknown command: {path}"));
            }
        }
    };
    let target = match &request {
        JsonRequest::Activate(id) | JsonRequest::Close(id) => id.clone(),
        _ => String::new(),
    };
    let closing = matches!(request, JsonRequest::Close(_));
    let Some(answer) = ask(&context, request).await else {
        return text(StatusCode::SERVICE_UNAVAILABLE, "The browser has gone");
    };
    match answer {
        JsonAnswer::Version {
            product,
            user_agent,
        } => json_response(&json!({
            "Browser": product,
            "Protocol-Version": "1.3",
            "User-Agent": user_agent,
            "webSocketDebuggerUrl": format!(
                "ws://127.0.0.1:{}/devtools/browser/{}",
                context.port, context.browser_id
            ),
        })),
        JsonAnswer::Targets(targets) => json_response(&Value::Array(
            targets
                .iter()
                .map(|target| target_json(&context, target))
                .collect(),
        )),
        JsonAnswer::Target(target) => json_response(&target_json(&context, &target)),
        JsonAnswer::Done if closing => text(StatusCode::OK, "Target is closing"),
        JsonAnswer::Done => text(StatusCode::OK, "Target activated"),
        JsonAnswer::NoTarget => text(
            StatusCode::NOT_FOUND,
            format!("No such target id: {target}"),
        ),
        JsonAnswer::Failed(why) => text(StatusCode::INTERNAL_SERVER_ERROR, why),
    }
}

/// A tab as `/json/list` lists it, as Chrome's `DevToolsHttpHandler` does.
/// Its DevTools are Chrome's own (`devtools://`): machopo has none to serve.
fn target_json(context: &Context, target: &TargetInfo) -> Value {
    let page = format!(
        "127.0.0.1:{}/devtools/page/{}",
        context.port, target.target_id
    );
    json!({
        "description": "",
        "devtoolsFrontendUrl": format!("devtools://devtools/bundled/inspector.html?ws={page}"),
        "id": target.target_id,
        "title": target.title,
        "type": target.kind,
        "url": target.url,
        "webSocketDebuggerUrl": format!("ws://{page}"),
    })
}

/// Asks the UI thread; `None` if the browser has gone.
async fn ask(context: &Context, request: JsonRequest) -> Option<JsonAnswer> {
    let (answer, answered) = oneshot::channel();
    let posted = (context.post)(ClientEvent::Json {
        request,
        answer: Box::new(move |result| {
            let _ = answer.send(result);
        }),
    });
    if !posted {
        return None;
    }
    answered.await.ok()
}

/// Whether `request` asks to become a websocket.
fn is_websocket(request: &Request<Incoming>) -> bool {
    let has_token = |name, token: &str| {
        header(request, name).is_some_and(|value| {
            value
                .split(',')
                .any(|item| item.trim().eq_ignore_ascii_case(token))
        })
    };
    request.method() == Method::GET
        && has_token(UPGRADE, "websocket")
        && has_token(CONNECTION, "upgrade")
}

/// Makes `request` a websocket of a new client: to the browser, or straight
/// to the tab `page` (a target id).
async fn upgrade(
    request: &mut Request<Incoming>,
    context: &Arc<Context>,
    page: Option<String>,
) -> Response<Full<Bytes>> {
    if !is_websocket(request) {
        return text(StatusCode::BAD_REQUEST, "Not a WebSocket handshake");
    }
    let origin = header(request, ORIGIN);
    if !context.allowed_origins.allows(origin) {
        let origin = origin.unwrap_or_default();
        return text(
            StatusCode::FORBIDDEN,
            format!(
                "Rejected an incoming WebSocket connection from the {origin} origin. Use the \
                 command line flag --remote-allow-origins={origin} to allow connections from \
                 this origin or --remote-allow-origins=* to allow all origins."
            ),
        );
    }
    let (Some(key), Some("13")) = (
        header(request, SEC_WEBSOCKET_KEY),
        header(request, SEC_WEBSOCKET_VERSION),
    ) else {
        return text(
            StatusCode::BAD_REQUEST,
            "Not a WebSocket version 13 handshake",
        );
    };
    let accept = derive_accept_key(key.as_bytes());
    let target = page.clone().unwrap_or_default();
    let (outgoing, messages) = mpsc::unbounded_channel::<Message>();
    let outbox = Outbox::new(move |message| outgoing.send(message).is_ok());
    let (accepted, client) = oneshot::channel();
    let posted = (context.post)(ClientEvent::Connect {
        outbox,
        page,
        accepted: Box::new(move |client| {
            let _ = accepted.send(client);
        }),
    });
    if !posted {
        return text(StatusCode::SERVICE_UNAVAILABLE, "The browser has gone");
    }
    let Ok(Some(client)) = client.await else {
        return text(
            StatusCode::NOT_FOUND,
            format!("No such target id: {target}"),
        );
    };
    let upgrading = hyper::upgrade::on(request);
    let task_context = context.clone();
    context
        .connections
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .spawn(async move {
            let context = task_context;
            match upgrading.await {
                Ok(upgraded) => {
                    connection(TokioIo::new(upgraded), client, &context, messages).await;
                }
                Err(_) => {
                    (context.post)(ClientEvent::Disconnect(client));
                }
            }
        });
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header(UPGRADE, HeaderValue::from_static("websocket"))
        .header(CONNECTION, HeaderValue::from_static("Upgrade"))
        .header(SEC_WEBSOCKET_ACCEPT, accept)
        .body(Full::default())
        .expect("a response")
}

/// A client's websocket, until either end closes it: its messages to the
/// UI thread as commands, the session's to it. A message or frame over the
/// limits closes it.
async fn connection<S>(
    socket: S,
    client: ClientId,
    context: &Context,
    mut messages: mpsc::UnboundedReceiver<Message>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_MESSAGE_SIZE))
        .max_frame_size(Some(MAX_FRAME_SIZE));
    let websocket = WebSocketStream::from_raw_socket(socket, Role::Server, Some(config)).await;
    let (mut sink, mut stream) = websocket.split();
    loop {
        tokio::select! {
            incoming = stream.next() => {
                let read = match incoming {
                    Some(Ok(Frame::Text(text))) => automation_pipe::read(text.as_bytes()),
                    Some(Ok(Frame::Binary(bytes))) => automation_pipe::read(&bytes),
                    // tungstenite answers a ping itself.
                    Some(Ok(Frame::Ping(_) | Frame::Pong(_) | Frame::Frame(_))) => continue,
                    Some(Ok(Frame::Close(_)) | Err(_)) | None => break,
                };
                if !(context.post)(ClientEvent::Command(client, read)) {
                    break;
                }
            }
            outgoing = messages.recv() => {
                let Some(message) = outgoing else {
                    break;
                };
                // The tab the connection was for has gone: so has it.
                let detached = matches!(
                    &message,
                    Message::Event(Event { event: EventKind::InspectorDetached { .. }, .. })
                );
                if sink.send(Frame::text(message.to_json())).await.is_err() {
                    break;
                }
                if detached {
                    let _ = sink.close().await;
                    break;
                }
            }
        }
    }
    (context.post)(ClientEvent::Disconnect(client));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_addresses_and_localhost_are_hosts() {
        for host in [
            "127.0.0.1:9222",
            "127.0.0.1",
            "localhost:9222",
            "LOCALHOST",
            "[::1]:9222",
            "[::1]",
            "192.168.1.2:80",
        ] {
            assert!(is_allowed_host(host), "{host}");
        }
        for host in [
            "example.com:9222",
            "evil.localhost:9222",
            "localhost.example.com",
            "[::1",
            "[::1]x",
            "",
        ] {
            assert!(!is_allowed_host(host), "{host}");
        }
    }

    #[test]
    fn origins_are_let_in_as_listed() {
        let none = AllowedOrigins::default();
        assert!(none.allows(None), "no origin: not a page");
        assert!(none.allows(Some("devtools://devtools")));
        assert!(!none.allows(Some("http://localhost:8000")));
        let listed = AllowedOrigins::parse("http://localhost:8000, https://Example.com/");
        assert!(listed.allows(Some("http://localhost:8000")));
        assert!(listed.allows(Some("https://example.com")));
        assert!(!listed.allows(Some("http://localhost:8001")));
        assert_eq!(
            AllowedOrigins::parse("http://a.test,*"),
            AllowedOrigins::Any
        );
        assert!(AllowedOrigins::Any.allows(Some("http://anything.test")));
    }

    #[test]
    fn the_browsers_id_is_a_random_uuid() {
        let (a, b) = (browser_id(), browser_id());
        assert_ne!(a, b);
        let groups: Vec<usize> = a.split('-').map(str::len).collect();
        assert_eq!(groups, [8, 4, 4, 4, 12]);
    }
}
