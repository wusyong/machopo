//! WebDriver's HTTP API: HTTP/1.1 with hyper, the routes and commands of
//! the `webdriver` crate (`standard_routes`, `WebDriverMessage::from_http`),
//! and the answers' JSON. The requests a page in a browser could send are
//! refused, as the crate's own server does: a `Host` other than this
//! server (a domain that isn't `localhost`, or another port), any
//! `Origin`, and a POST with a content type a form can send.

use std::convert::Infallible;
use std::net::{SocketAddr, TcpListener};
use std::sync::Arc;
use std::thread::JoinHandle;

use base::numerics::saturated_cast;
use crossbeam_channel::Sender;
use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::header::{CACHE_CONTROL, CONTENT_TYPE, HOST, ORIGIN};
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use tokio::sync::oneshot;
use url::{Host, Url};
use webdriver::Parameters;
use webdriver::command::WebDriverMessage;
use webdriver::error::ErrorStatus;
use webdriver::httpapi::{Route, VoidWebDriverExtensionRoute, standard_routes};

use crate::error::Failure;

/// A command for the dispatcher, and where its answer (the success's
/// body, or the failure) goes.
pub struct Call {
    pub message: WebDriverMessage,
    pub reply: oneshot::Sender<Result<String, Failure>>,
}

/// A route's path, split.
enum Segment {
    Literal(&'static str),
    /// `{name}`: any segment, as the parameter `name`.
    Parameter(&'static str),
}

struct Routes(Vec<(Method, Vec<Segment>, Route<VoidWebDriverExtensionRoute>)>);

impl Routes {
    fn new() -> Self {
        Self(
            standard_routes::<VoidWebDriverExtensionRoute>()
                .into_iter()
                .map(|(method, path, route)| {
                    let segments = path
                        .split('/')
                        .filter(|part| !part.is_empty())
                        .map(|part| match part.strip_prefix('{') {
                            Some(name) => Segment::Parameter(name.trim_end_matches('}')),
                            None => Segment::Literal(part),
                        })
                        .collect();
                    (method, segments, route)
                })
                .collect(),
        )
    }

    /// The route `method` and `path` are, with the path's parameters: an
    /// unknown path is `unknown command`, an unknown method on a known one
    /// `unknown method`.
    fn find(
        &self,
        method: &Method,
        path: &str,
    ) -> Result<(Route<VoidWebDriverExtensionRoute>, Parameters), Failure> {
        let parts: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
        let mut known_path = false;
        for (route_method, segments, route) in &self.0 {
            let Some(params) = matches(segments, &parts) else {
                continue;
            };
            known_path = true;
            if route_method == method {
                return Ok((route.clone(), params));
            }
        }
        Err(if known_path {
            Failure::new(ErrorStatus::UnknownMethod, format!("{method} {path}"))
        } else {
            Failure::new(ErrorStatus::UnknownCommand, format!("{method} {path}"))
        })
    }
}

fn matches(segments: &[Segment], parts: &[&str]) -> Option<Parameters> {
    if segments.len() != parts.len() {
        return None;
    }
    let mut params = Parameters::new();
    for (segment, part) in segments.iter().zip(parts) {
        match segment {
            Segment::Literal(literal) if literal == part => {}
            Segment::Literal(_) => return None,
            Segment::Parameter(name) => {
                params.insert((*name).to_owned(), (*part).to_owned());
            }
        }
    }
    Some(params)
}

/// Whether `host` (a `Host` header) is this server: an IP address or
/// `localhost`, at its port.
fn is_this_server(server: SocketAddr, host: &str) -> bool {
    let Ok(url) = Url::parse(&format!("http://{host}")) else {
        return false;
    };
    let host_ok = match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(_) | Host::Ipv6(_)) => true,
        None => false,
    };
    host_ok && url.port_or_known_default() == Some(server.port())
}

/// A content type a page can POST without asking first (Fetch's
/// CORS-safelisted ones).
fn is_form_content_type(content_type: &str) -> bool {
    let essence = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    matches!(
        essence.as_str(),
        "application/x-www-form-urlencoded" | "multipart/form-data" | "text/plain"
    )
}

async fn handle(
    request: Request<Incoming>,
    server: SocketAddr,
    routes: Arc<Routes>,
    commands: Sender<Call>,
) -> Result<String, Failure> {
    let refused = |message: &str| Failure::new(ErrorStatus::UnknownError, message);
    let header = |name| {
        request
            .headers()
            .get(name)
            .map(|value: &hyper::header::HeaderValue| {
                value.to_str().unwrap_or("\u{FFFD}").to_owned()
            })
    };
    match header(HOST) {
        Some(host) if is_this_server(server, &host) => {}
        Some(host) => return Err(refused(&format!("Invalid Host header {host}"))),
        None => return Err(refused("Missing Host header")),
    }
    if let Some(origin) = header(ORIGIN) {
        return Err(refused(&format!("Invalid Origin header {origin}")));
    }
    let method = request.method().clone();
    if method == Method::POST && header(CONTENT_TYPE).is_some_and(|ty| is_form_content_type(&ty)) {
        return Err(refused("Invalid Content-Type"));
    }
    let path = request.uri().path().to_owned();
    let (route, params) = routes.find(&method, &path)?;
    let body = request
        .into_body()
        .collect()
        .await
        .map_err(|e| refused(&format!("reading the body: {e}")))?
        .to_bytes();
    let body =
        String::from_utf8(body.to_vec()).map_err(|_| refused("Request body wasn't valid UTF-8"))?;
    tracing::debug!("-> {method} {path} {body}");
    let body = match route {
        Route::PerformActions => integral_numbers(body),
        _ => body,
    };
    let message = WebDriverMessage::from_http(route, &params, &body, method == Method::POST)?;
    let (reply, answer) = oneshot::channel();
    commands
        .send(Call { message, reply })
        .map_err(|_| refused("the dispatcher has gone"))?;
    answer
        .await
        .map_err(|_| refused("the dispatcher has gone"))?
}

/// `body` with its numbers that are whole (`100.0`) written as integers:
/// the spec's integers are Numbers that are whole, which the `webdriver`
/// crate's integer fields (an action's `duration`, a scroll's deltas)
/// don't take as floats; wptrunner's durations are times a float
/// multiplier. Not JSON: as it is, for the crate to refuse.
fn integral_numbers(body: String) -> String {
    fn whole(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64().filter(|f| n.is_f64() && f.fract() == 0.0)
                    && f.abs() < 2f64.powi(53)
                {
                    // Exactly: an i64 holds every integer under 2^53.
                    *value = serde_json::Value::from(saturated_cast::<i64, _>(f));
                }
            }
            serde_json::Value::Array(items) => items.iter_mut().for_each(whole),
            serde_json::Value::Object(members) => members.values_mut().for_each(whole),
            _ => {}
        }
    }
    match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(mut value) => {
            whole(&mut value);
            value.to_string()
        }
        Err(_) => body,
    }
}

/// Serves HTTP on `listener`, on a thread of its own.
pub fn serve(
    listener: TcpListener,
    server: SocketAddr,
    commands: Sender<Call>,
) -> std::io::Result<JoinHandle<()>> {
    listener.set_nonblocking(true)?;
    let routes = Arc::new(Routes::new());
    std::thread::Builder::new()
        .name("WebDriverServer".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .build()
                .expect("a tokio runtime");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(listener).expect("a nonblocking listener");
                loop {
                    let Ok((stream, _)) = listener.accept().await else {
                        continue;
                    };
                    let (routes, commands) = (routes.clone(), commands.clone());
                    tokio::spawn(async move {
                        let service = service_fn(move |request: Request<Incoming>| {
                            let (routes, commands) = (routes.clone(), commands.clone());
                            async move {
                                let head = request.method() == Method::HEAD;
                                let (status, body) =
                                    match handle(request, server, routes, commands).await {
                                        Ok(body) => (StatusCode::OK, body),
                                        Err(failure) => (failure.status, failure.body()),
                                    };
                                tracing::debug!("<- {status} {body}");
                                let body = if head { String::new() } else { body };
                                let response = Response::builder()
                                    .status(status)
                                    .header(CONTENT_TYPE, "application/json; charset=utf-8")
                                    .header(CACHE_CONTROL, "no-cache")
                                    .body(Full::new(Bytes::from(body)))
                                    .expect("a response");
                                Ok::<_, Infallible>(response)
                            }
                        });
                        let _ = http1::Builder::new()
                            .serve_connection(TokioIo::new(stream), service)
                            .await;
                    });
                }
            });
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn find(method: Method, path: &str) -> Result<(String, Parameters), Failure> {
        Routes::new()
            .find(&method, path)
            .map(|(route, params)| (format!("{route:?}"), params))
    }

    #[test]
    fn routes_take_their_parameters() {
        let (route, params) = find(Method::POST, "/session/abc/url").unwrap();
        assert_eq!(route, "Get");
        assert_eq!(params["sessionId"], "abc");
        let (route, params) = find(Method::GET, "/session/abc/url").unwrap();
        assert_eq!(route, "GetCurrentUrl");
        assert_eq!(params.len(), 1);
        assert_eq!(find(Method::POST, "/session").unwrap().0, "NewSession");
        // A trailing slash is the same path.
        assert_eq!(find(Method::GET, "/status/").unwrap().0, "Status");
    }

    #[test]
    fn unknown_paths_and_methods_are_told_apart() {
        let unknown = find(Method::GET, "/fly").unwrap_err();
        assert_eq!(
            (unknown.status, unknown.error.as_ref()),
            (StatusCode::NOT_FOUND, "unknown command")
        );
        let wrong = find(Method::PUT, "/session/abc/url").unwrap_err();
        assert_eq!(
            (wrong.status, wrong.error.as_ref()),
            (StatusCode::METHOD_NOT_ALLOWED, "unknown method")
        );
    }

    #[test]
    fn only_this_server_is_its_host() {
        let server: SocketAddr = "127.0.0.1:4444".parse().unwrap();
        assert!(is_this_server(server, "127.0.0.1:4444"));
        assert!(is_this_server(server, "localhost:4444"));
        assert!(is_this_server(server, "[::1]:4444"));
        assert!(!is_this_server(server, "127.0.0.1:4445"));
        assert!(!is_this_server(server, "127.0.0.1"));
        assert!(!is_this_server(server, "example.com:4444"));
        assert!(!is_this_server(server, "not a host"));
    }

    #[test]
    fn form_content_types_are_refused() {
        assert!(is_form_content_type("text/plain"));
        assert!(is_form_content_type("Text/Plain; charset=utf-8"));
        assert!(is_form_content_type("multipart/form-data; boundary=x"));
        assert!(is_form_content_type("application/x-www-form-urlencoded"));
        assert!(!is_form_content_type("application/json"));
        assert!(!is_form_content_type("application/json; charset=utf-8"));
    }
}
