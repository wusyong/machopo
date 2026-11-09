//! DevTools' `Network` domain as the renderer answers it (Blink's
//! `InspectorNetworkAgent`): for each session that turned `Network` on, the
//! requests the renderer sends (subresources, `fetch()`, iframes'
//! documents) as they go, redirect and end, and their bodies, kept for
//! `Network.getResponseBody`. The browser tells of the main frame's
//! navigations (the renderer, of their end); it answers the commands that
//! are only its own (cookies, the cache, network conditions) and passes the
//! rest on.
//!
//! Each session keeps its own: its limits, the bodies it holds (the bytes
//! are shared, not copied) and whether `Network` is on, as each of
//! Chrome's sessions has its own agent.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

use bytes::Bytes;
use crossbeam_channel::Sender;
use ipc::TimeTicks;
use protocol::devtools_network::{priority, request_json, resource_type, response_json, wall_time};
use protocol::network::{LoadResult, NetError, RedirectInfo, RequestDestination, ResourceRequest};
use protocol::renderer::{NavigationLoad, RendererHostMsg};
use serde_json::{Value, json};

use super::log_agent::LogAgent;
use crate::initiator::Initiator;

/// JSON-RPC's "invalid params".
const INVALID_PARAMS: i64 = -32602;
/// CDP's error for what the command can't do.
const SERVER_ERROR: i64 = -32000;

/// How much of the bodies a session keeps, at most, unless `Network.enable`
/// says otherwise (Blink's `kDefaultTotalBufferSize`).
const DEFAULT_TOTAL_BUFFER_SIZE: usize = 200_000_000;
/// How big a body a session keeps, at most (`kDefaultResourceBufferSize`).
const DEFAULT_RESOURCE_BUFFER_SIZE: usize = 20_000_000;

/// Where a request was sent from, as DevTools names it.
pub(crate) struct RequestFrame {
    pub(crate) frame_id: String,
    /// The document's loader (for an iframe's document's own request, the
    /// request's: `NetworkAgent::request_id`).
    pub(crate) loader_id: String,
    pub(crate) document_url: String,
}

/// A request DevTools was told of, until it ends.
struct Request {
    id: String,
    loader_id: String,
    frame_id: String,
    document_url: String,
    kind: &'static str,
    /// Who asked for it (`Network.Initiator`), told again at each redirect.
    initiator: Value,
    /// The sessions told of it.
    sessions: Vec<String>,
}

/// A session that turned `Network` on.
struct Session {
    max_total_buffer_size: usize,
    max_resource_buffer_size: usize,
    max_post_data_size: usize,
    /// `Network.setAttachDebugStack`: the requests script asks for are
    /// told with its stack.
    attach_debug_stack: bool,
    /// The bodies kept, oldest first; `None` for one not kept (too big, or
    /// dropped for newer ones).
    bodies: VecDeque<(String, Option<KeptBody>)>,
    /// What the kept bodies take.
    kept: usize,
    /// The requests' bodies, for `Network.getRequestPostData`.
    post_data: HashMap<String, Vec<u8>>,
}

impl Session {
    fn new(params: &Value) -> Self {
        let size = |name: &str, default: usize| {
            params
                .get(name)
                .and_then(Value::as_u64)
                .and_then(|size| usize::try_from(size).ok())
                .unwrap_or(default)
        };
        Self {
            max_total_buffer_size: size("maxTotalBufferSize", DEFAULT_TOTAL_BUFFER_SIZE),
            max_resource_buffer_size: size("maxResourceBufferSize", DEFAULT_RESOURCE_BUFFER_SIZE),
            max_post_data_size: size("maxPostDataSize", 0),
            attach_debug_stack: false,
            bodies: VecDeque::new(),
            kept: 0,
            post_data: HashMap::new(),
        }
    }

    /// Keeps `body` as request `id`'s, unless it is too big; drops the
    /// oldest kept until they fit (Blink's `NetworkResourcesData`).
    fn keep(&mut self, id: &str, body: KeptBody) {
        let size = body.bytes.len();
        if size > self.max_resource_buffer_size || size > self.max_total_buffer_size {
            self.bodies.push_back((id.to_owned(), None));
            return;
        }
        while self.kept.saturating_add(size) > self.max_total_buffer_size {
            let Some(oldest) = self.bodies.iter_mut().find(|(_, body)| body.is_some()) else {
                break;
            };
            if let Some(dropped) = oldest.1.take() {
                self.kept = self.kept.saturating_sub(dropped.bytes.len());
            }
        }
        self.kept = self.kept.saturating_add(size);
        self.bodies.push_back((id.to_owned(), Some(body)));
    }
}

/// A response's body as kept: its bytes, and whether they read as text;
/// for a subresource, its frame's id and URL (without its fragment), by
/// which `Page.getResourceContent` asks for it.
struct KeptBody {
    bytes: Bytes,
    text: bool,
    resource: Option<(String, String)>,
}

impl KeptBody {
    /// As DevTools takes a body: text, or base64.
    fn to_json(&self) -> Value {
        if self.text {
            json!({
                "body": String::from_utf8_lossy(&self.bytes),
                "base64Encoded": false,
            })
        } else {
            json!({ "body": base64(&self.bytes), "base64Encoded": true })
        }
    }
}

/// Blink's `InspectorNetworkAgent`, for every session of the page.
pub(crate) struct NetworkAgent {
    to_browser: Sender<RendererHostMsg>,
    /// This renderer's, in its requests' ids (its frames' too).
    prefix: u64,
    sessions: RefCell<HashMap<String, Session>>,
    /// The requests told of, by the renderer's id, until they end.
    requests: RefCell<HashMap<u64, Request>>,
    /// Where a session hears of what machopo can't do.
    log_agent: Rc<LogAgent>,
}

impl NetworkAgent {
    pub(crate) fn new(
        to_browser: Sender<RendererHostMsg>,
        prefix: u64,
        log_agent: Rc<LogAgent>,
    ) -> Self {
        Self {
            to_browser,
            prefix,
            sessions: RefCell::new(HashMap::new()),
            requests: RefCell::new(HashMap::new()),
            log_agent,
        }
    }

    /// Whether any session has `Network` on: the requests sent are worth
    /// telling of (none is looked at otherwise).
    pub(crate) fn enabled(&self) -> bool {
        !self.sessions.borrow().is_empty()
    }

    /// Whether a session wants the requests' stacks
    /// (`Network.setAttachDebugStack`).
    pub(crate) fn wants_stacks(&self) -> bool {
        self.sessions
            .borrow()
            .values()
            .any(|state| state.attach_debug_stack)
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
    }

    /// `method`'s event, `params`, to `sessions` that still have `Network`
    /// on.
    fn tell(&self, sessions: &[String], method: &str, params: &Value) {
        let message = json!({ "method": method, "params": params });
        let on = self.sessions.borrow();
        for session in sessions.iter().filter(|session| on.contains_key(*session)) {
            self.send(session, &message);
        }
    }

    /// What DevTools calls the renderer's request `request_id` (also the
    /// loader of the iframe document it brings).
    pub(crate) fn request_id(&self, request_id: u64) -> String {
        format!("{:016x}.{request_id}", self.prefix)
    }

    /// Request `request_id`, `request`, goes now, from `frame`, asked for
    /// by `initiator` (`Network.requestWillBeSent`, to each session with
    /// `Network` on; each keeps its body).
    pub(crate) fn will_send(
        &self,
        request_id: u64,
        request: &ResourceRequest,
        frame: RequestFrame,
        initiator: Option<&Initiator>,
    ) {
        let sessions: Vec<String> = self.sessions.borrow().keys().cloned().collect();
        if sessions.is_empty() {
            return;
        }
        let id = self.request_id(request_id);
        let kind = resource_type(request.destination);
        let loader_id = if request.destination == RequestDestination::Iframe {
            // An iframe's document: its request is its loader.
            id.clone()
        } else {
            frame.loader_id
        };
        let initiator = Initiator::to_json(initiator);
        let (timestamp, wall_time) = (TimeTicks::now().as_secs_f64(), wall_time());
        let mut sessions_told = Vec::new();
        for session in &sessions {
            let max_post_data_size = {
                let mut on = self.sessions.borrow_mut();
                let Some(state) = on.get_mut(session) else {
                    continue;
                };
                if let Some(body) = &request.body {
                    state.post_data.insert(id.clone(), body.clone());
                }
                state.max_post_data_size
            };
            let params = json!({
                "requestId": id,
                "loaderId": loader_id,
                "documentURL": frame.document_url,
                "request": request_json(request, max_post_data_size),
                "timestamp": timestamp,
                "wallTime": wall_time,
                "initiator": initiator,
                "redirectHasExtraInfo": false,
                "type": kind,
                "frameId": frame.frame_id,
                "hasUserGesture": false,
            });
            self.send(
                session,
                &json!({ "method": "Network.requestWillBeSent", "params": params }),
            );
            sessions_told.push(session.clone());
        }
        self.requests.borrow_mut().insert(
            request_id,
            Request {
                id,
                loader_id,
                frame_id: frame.frame_id,
                document_url: frame.document_url,
                kind,
                initiator,
                sessions: sessions_told,
            },
        );
    }

    /// Request `request_id` reached `redirect`: the request it leads to
    /// goes under the same id, with the redirect's response
    /// (`Network.requestWillBeSent` with `redirectResponse`).
    pub(crate) fn did_redirect(&self, request_id: u64, redirect: &RedirectInfo) {
        let requests = self.requests.borrow();
        let Some(known) = requests.get(&request_id) else {
            return;
        };
        let next = json!({
            "url": redirect.new_url,
            "method": redirect.new_method,
            "headers": {},
            "initialPriority": priority(known.kind),
            "referrerPolicy": "strict-origin-when-cross-origin",
            "mixedContentType": "none",
        });
        let params = json!({
            "requestId": known.id,
            "loaderId": known.loader_id,
            "documentURL": known.document_url,
            "request": next,
            "timestamp": TimeTicks::now().as_secs_f64(),
            "wallTime": wall_time(),
            "initiator": known.initiator,
            "redirectHasExtraInfo": false,
            "redirectResponse": response_json(&redirect.response),
            "type": known.kind,
            "frameId": known.frame_id,
            "hasUserGesture": false,
        });
        self.tell(&known.sessions, "Network.requestWillBeSent", &params);
    }

    /// Request `request_id` ended with `result` (`responseReceived`,
    /// `dataReceived` and `loadingFinished`, or `loadingFailed`); each
    /// session told of it keeps its body.
    pub(crate) fn did_finish(&self, request_id: u64, result: &LoadResult) {
        let Some(known) = self.requests.borrow_mut().remove(&request_id) else {
            return;
        };
        let response = match result {
            Ok(response) => response,
            Err(error) => {
                self.failed(&known, Some(error));
                return;
            }
        };
        let end = response
            .timing
            .map_or_else(TimeTicks::now, |timing| timing.response_end);
        self.tell(
            &known.sessions,
            "Network.responseReceived",
            &json!({
                "requestId": known.id,
                "loaderId": known.loader_id,
                "timestamp": response
                    .timing
                    .map_or(end, |timing| timing.receive_headers_end)
                    .as_secs_f64(),
                "type": known.kind,
                "response": response_json(response),
                "hasExtraInfo": false,
                "frameId": known.frame_id,
            }),
        );
        let body_length = response.body.len();
        if body_length > 0 {
            self.tell(
                &known.sessions,
                "Network.dataReceived",
                &json!({
                    "requestId": known.id,
                    "timestamp": end.as_secs_f64(),
                    "dataLength": body_length,
                    "encodedDataLength": response.encoded_body_length,
                }),
            );
        }
        self.tell(
            &known.sessions,
            "Network.loadingFinished",
            &json!({
                "requestId": known.id,
                "timestamp": end.as_secs_f64(),
                "encodedDataLength": response
                    .encoded_headers_length
                    .saturating_add(response.encoded_body_length),
            }),
        );
        let text = is_text(&response.mime_type) && std::str::from_utf8(&response.body).is_ok();
        let url = response.url.split('#').next().unwrap_or_default();
        let mut sessions = self.sessions.borrow_mut();
        for session in &known.sessions {
            if let Some(state) = sessions.get_mut(session) {
                state.keep(
                    &known.id,
                    KeptBody {
                        bytes: response.body.clone(),
                        text,
                        resource: Some((known.frame_id.clone(), url.to_owned())),
                    },
                );
            }
        }
    }

    /// The renderer cancelled request `request_id`
    /// (`Network.loadingFailed`, `canceled`).
    pub(crate) fn did_cancel(&self, request_id: u64) {
        let known = self.requests.borrow_mut().remove(&request_id);
        if let Some(known) = known {
            self.failed(&known, None);
        }
    }

    /// `known` failed with `error`, or was cancelled.
    fn failed(&self, known: &Request, error: Option<&NetError>) {
        let error_text = error.map_or_else(
            || "net::ERR_ABORTED".to_owned(),
            |error| format!("net::{}", error.code.name()),
        );
        self.tell(
            &known.sessions,
            "Network.loadingFailed",
            &json!({
                "requestId": known.id,
                "timestamp": TimeTicks::now().as_secs_f64(),
                "type": known.kind,
                "errorText": error_text,
                "canceled": error.is_none(),
            }),
        );
    }

    /// The main frame's navigation `navigation_id` committed its document,
    /// `body`, of `mime_type`, its load as `loaded`: the rest of the request
    /// the browser told of (`Network.dataReceived`, `loadingFinished`; its
    /// id is the navigation's), and each session keeps the body.
    pub(crate) fn did_commit_navigation(
        &self,
        navigation_id: u64,
        body: &Bytes,
        mime_type: &str,
        loaded: &NavigationLoad,
    ) {
        let sessions: Vec<String> = self.sessions.borrow().keys().cloned().collect();
        if sessions.is_empty() {
            return;
        }
        let id = navigation_id.to_string();
        let end = loaded.response_end.as_secs_f64();
        if !body.is_empty() {
            self.tell(
                &sessions,
                "Network.dataReceived",
                &json!({
                    "requestId": id,
                    "timestamp": end,
                    "dataLength": body.len(),
                    "encodedDataLength": loaded.encoded_body_length,
                }),
            );
        }
        self.tell(
            &sessions,
            "Network.loadingFinished",
            &json!({
                "requestId": id,
                "timestamp": end,
                "encodedDataLength": loaded
                    .encoded_headers_length
                    .saturating_add(loaded.encoded_body_length),
            }),
        );
        let text = is_text(mime_type) && std::str::from_utf8(body).is_ok();
        let mut on = self.sessions.borrow_mut();
        for state in on.values_mut() {
            state.keep(
                &id,
                KeptBody {
                    bytes: body.clone(),
                    text,
                    // The document's own, which `Page` has.
                    resource: None,
                },
            );
        }
    }

    /// The main frame has a new document: the bodies kept go (Blink's
    /// agent clears its resources' data for the new document).
    pub(crate) fn main_frame_navigated(&self) {
        for state in self.sessions.borrow_mut().values_mut() {
            state.bodies.clear();
            state.kept = 0;
            state.post_data.clear();
        }
    }

    /// `session` has gone.
    pub(crate) fn detach(&self, session: &str) {
        self.sessions.borrow_mut().remove(session);
    }

    /// Answers `message`, a command of `session`'s, if it is one of the
    /// agent's: whether it was.
    pub(crate) fn handle(&self, session: &str, message: &str) -> bool {
        let Ok(command) = serde_json::from_str::<Value>(message) else {
            return false;
        };
        let Some(method) = command.get("method").and_then(Value::as_str) else {
            return false;
        };
        let params = command.get("params").cloned().unwrap_or(Value::Null);
        let result = match method {
            "Network.enable" => {
                self.sessions
                    .borrow_mut()
                    .insert(session.to_owned(), Session::new(&params));
                Ok(json!({}))
            }
            "Network.disable" => {
                self.sessions.borrow_mut().remove(session);
                Ok(json!({}))
            }
            "Network.getResponseBody" => self.response_body(session, &params),
            "Network.getRequestPostData" => self.post_data(session, &params),
            "Network.setAttachDebugStack" => match self.sessions.borrow_mut().get_mut(session) {
                Some(state) => {
                    state.attach_debug_stack = params
                        .get("enabled")
                        .and_then(Value::as_bool)
                        .unwrap_or(false);
                    Ok(json!({}))
                }
                None => Err((SERVER_ERROR, "Domain must be enabled".to_owned())),
            },
            // What machopo does anyway: no HTTP cache, no service workers.
            "Network.setCacheDisabled" | "Network.setBypassServiceWorker" => Ok(json!({})),
            "Network.setBlockedURLs" => {
                let blocks = params
                    .get("urlPatterns")
                    .and_then(Value::as_array)
                    .is_some_and(|patterns| {
                        patterns.iter().any(|pattern| {
                            pattern.get("block").and_then(Value::as_bool) != Some(false)
                        })
                    })
                    || params
                        .get("urls")
                        .and_then(Value::as_array)
                        .is_some_and(|urls| !urls.is_empty());
                if blocks {
                    self.cant(session, method, "block requests")
                } else {
                    Ok(json!({}))
                }
            }
            "Network.overrideNetworkState" => {
                if is_unthrottled(&params) {
                    Ok(json!({}))
                } else {
                    self.cant(session, method, "take the network offline or throttle it")
                }
            }
            "Network.setExtraHTTPHeaders" => {
                let none = params
                    .get("headers")
                    .and_then(Value::as_object)
                    .is_none_or(serde_json::Map::is_empty);
                if none {
                    Ok(json!({}))
                } else {
                    self.cant(session, method, "add headers to requests")
                }
            }
            _ => return false,
        };
        let id = command.get("id").cloned().unwrap_or(Value::Null);
        let answer = match result {
            Ok(result) => json!({ "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "id": id, "error": { "code": code, "message": message } })
            }
        };
        self.send(session, &answer);
        true
    }

    /// `method`'s answer when it asks for what machopo can't do, `what`: an
    /// error, and a warning in `session`'s log, as DevTools' front end
    /// doesn't show the error (Chrome does all of these).
    fn cant(&self, session: &str, method: &str, what: &str) -> Result<Value, (i64, String)> {
        let message = format!("machopo can't {what}");
        self.log_agent.warn(session, format!("{method}: {message}"));
        Err((SERVER_ERROR, message))
    }

    /// `Network.getResponseBody`, of the bodies `session` kept.
    fn response_body(&self, session: &str, params: &Value) -> Result<Value, (i64, String)> {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return Err((INVALID_PARAMS, "requestId is needed".to_owned()));
        };
        let sessions = self.sessions.borrow();
        let found = sessions
            .get(session)
            .and_then(|state| state.bodies.iter().rev().find(|(kept, _)| kept == id));
        match found {
            None => Err((
                SERVER_ERROR,
                "No resource with given identifier found".to_owned(),
            )),
            Some((_, None)) => Err((
                SERVER_ERROR,
                "Request content was evicted from inspector cache".to_owned(),
            )),
            Some((_, Some(body))) => Ok(body.to_json()),
        }
    }

    /// The newest body `session` kept of frame `frame_id`'s subresource
    /// `url` (without its fragment), as `Page.getResourceContent` answers.
    pub(crate) fn kept_content(&self, session: &str, frame_id: &str, url: &str) -> Option<Value> {
        let sessions = self.sessions.borrow();
        let body = sessions
            .get(session)?
            .bodies
            .iter()
            .rev()
            .find_map(|(_, body)| {
                body.as_ref().filter(|body| {
                    body.resource
                        .as_ref()
                        .is_some_and(|(frame, kept)| frame == frame_id && kept == url)
                })
            })?;
        let mut content = body.to_json();
        let object = content.as_object_mut()?;
        let text = object.remove("body")?;
        object.insert("content".into(), text);
        Some(content)
    }

    /// `Network.getRequestPostData`, of the requests `session` was told of.
    fn post_data(&self, session: &str, params: &Value) -> Result<Value, (i64, String)> {
        let Some(id) = params.get("requestId").and_then(Value::as_str) else {
            return Err((INVALID_PARAMS, "requestId is needed".to_owned()));
        };
        self.sessions
            .borrow()
            .get(session)
            .and_then(|state| state.post_data.get(id))
            .map(|body| json!({ "postData": String::from_utf8_lossy(body) }))
            .ok_or_else(|| {
                (
                    SERVER_ERROR,
                    "No post data available for the request".to_owned(),
                )
            })
    }
}

/// Whether `Network.overrideNetworkState` (or `emulateNetworkConditions`'
/// params) asks for the network as it is: online, no latency, no limit.
fn is_unthrottled(params: &Value) -> bool {
    let number = |name: &str| params.get(name).and_then(Value::as_f64).unwrap_or(0.0);
    params.get("offline").and_then(Value::as_bool) != Some(true)
        && number("latency") <= 0.0
        && number("downloadThroughput") <= 0.0
        && number("uploadThroughput") <= 0.0
}

/// Whether a body of MIME type `mime_type` reads as text (Blink's
/// `IsTextBasedMIMEType`, roughly).
fn is_text(mime_type: &str) -> bool {
    mime_type.starts_with("text/")
        || mime_type.ends_with("+json")
        || mime_type.ends_with("+xml")
        || matches!(
            mime_type,
            "application/json"
                | "application/javascript"
                | "application/ecmascript"
                | "application/xml"
                | "application/x-javascript"
                | "image/svg+xml"
        )
}

/// `bytes` in standard base64.
fn base64(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

#[cfg(test)]
mod tests {
    use protocol::network::{NetErrorCode, ResponseTainting};

    use super::*;

    /// What was sent: each message's session and JSON.
    fn sent(from: &crossbeam_channel::Receiver<RendererHostMsg>) -> Vec<(String, Value)> {
        from.try_iter()
            .map(|msg| match msg {
                RendererHostMsg::DevToolsMessage { session, message } => {
                    (session, serde_json::from_str(&message).unwrap())
                }
                other => panic!("{other:?}"),
            })
            .collect()
    }

    /// The methods of the events `session` was sent, in order.
    fn methods(sent: &[(String, Value)], session: &str) -> Vec<String> {
        sent.iter()
            .filter(|(to, message)| to == session && message.get("method").is_some())
            .map(|(_, message)| message["method"].as_str().unwrap().to_owned())
            .collect()
    }

    fn agent() -> (NetworkAgent, crossbeam_channel::Receiver<RendererHostMsg>) {
        let (to_browser, from) = crossbeam_channel::unbounded();
        let log = Rc::new(LogAgent::new(to_browser.clone()));
        (NetworkAgent::new(to_browser, 0xab, log), from)
    }

    fn command(method: &str, params: Value) -> String {
        json!({ "id": 1, "method": method, "params": params }).to_string()
    }

    fn frame() -> RequestFrame {
        RequestFrame {
            frame_id: "F".into(),
            loader_id: "L".into(),
            document_url: "https://a.test/".into(),
        }
    }

    fn response(url: &str, mime_type: &str, body: &[u8]) -> protocol::network::ResourceResponse {
        protocol::network::ResourceResponse {
            url: url.into(),
            status: 200,
            status_text: "OK".into(),
            redirected: false,
            tainting: ResponseTainting::Basic,
            mime_type: mime_type.into(),
            charset: None,
            headers: Vec::new(),
            timing: None,
            remote_address: None,
            protocol: "http/1.1".into(),
            encoded_headers_length: 10,
            encoded_body_length: 3,
            body: Bytes::copy_from_slice(body),
        }
    }

    /// Nothing is told, or kept, before a session turns `Network` on: a
    /// request sent then ends untold.
    #[test]
    fn nothing_is_told_without_a_session() {
        let (agent, from) = agent();
        assert!(!agent.enabled());
        let request = ResourceRequest::new("https://a.test/i.png", RequestDestination::Image);
        agent.will_send(1, &request, frame(), None);
        agent.did_finish(1, &Ok(response("https://a.test/i.png", "image/png", b"x")));
        assert!(sent(&from).is_empty());
        assert!(agent.handle("s", &command("Network.enable", json!({}))));
        assert!(agent.enabled());
        agent.did_finish(1, &Ok(response("https://a.test/i.png", "image/png", b"x")));
        assert_eq!(methods(&sent(&from), "s"), Vec::<String>::new());
    }

    /// A request is told from its frame and document, with who asked,
    /// under the renderer's id; its redirect under the same, its end with
    /// its response, its data and its length; its body kept, as text or
    /// base64, and for `Page.getResourceContent` by its frame and URL. An
    /// iframe's document's request is its own loader.
    #[test]
    fn a_request_is_told_as_it_goes_and_ends() {
        let (agent, from) = agent();
        agent.handle("s", &command("Network.enable", json!({})));
        sent(&from);
        let request = ResourceRequest::new("https://a.test/s.css", RequestDestination::Style);
        let parser = Initiator::Parser {
            url: "https://a.test/".into(),
            line: 1,
        };
        agent.will_send(7, &request, frame(), Some(&parser));
        let redirect = RedirectInfo {
            new_url: "https://a.test/t.css".into(),
            new_method: "GET".into(),
            response: response("https://a.test/s.css", "text/html", b""),
        };
        agent.did_redirect(7, &redirect);
        agent.did_finish(
            7,
            &Ok(response("https://a.test/t.css#x", "text/css", b"p{}")),
        );
        let told = sent(&from);
        assert_eq!(
            methods(&told, "s"),
            [
                "Network.requestWillBeSent",
                "Network.requestWillBeSent",
                "Network.responseReceived",
                "Network.dataReceived",
                "Network.loadingFinished"
            ]
        );
        let first = &told[0].1["params"];
        assert_eq!(first["requestId"], "00000000000000ab.7");
        assert_eq!(
            (&first["frameId"], &first["loaderId"], &first["documentURL"]),
            (&json!("F"), &json!("L"), &json!("https://a.test/"))
        );
        assert_eq!(first["type"], "Stylesheet");
        assert_eq!(
            first["initiator"],
            json!({ "type": "parser", "url": "https://a.test/", "lineNumber": 1 })
        );
        let redirected = &told[1].1["params"];
        assert_eq!(redirected["requestId"], first["requestId"]);
        assert_eq!(redirected["request"]["url"], "https://a.test/t.css");
        assert_eq!(redirected["initiator"], first["initiator"], "told again");
        assert!(redirected.get("redirectResponse").is_some());
        assert_eq!(told[3].1["params"]["dataLength"], 3);
        assert_eq!(told[4].1["params"]["encodedDataLength"], 13);
        agent.handle(
            "s",
            &command(
                "Network.getResponseBody",
                json!({ "requestId": "00000000000000ab.7" }),
            ),
        );
        assert_eq!(
            sent(&from)[0].1["result"],
            json!({ "body": "p{}", "base64Encoded": false })
        );
        assert_eq!(
            agent.kept_content("s", "F", "https://a.test/t.css"),
            Some(json!({ "content": "p{}", "base64Encoded": false }))
        );
        assert_eq!(agent.kept_content("s", "G", "https://a.test/t.css"), None);
        assert_eq!(agent.kept_content("t", "F", "https://a.test/t.css"), None);
        // Ended: a late answer for it is no one's.
        agent.did_finish(7, &Ok(response("https://a.test/t.css", "text/css", b"")));
        assert!(sent(&from).is_empty());

        let image = ResourceRequest::new("https://a.test/i.png", RequestDestination::Image);
        agent.will_send(8, &image, frame(), None);
        agent.did_finish(
            8,
            &Ok(response(
                "https://a.test/i.png",
                "image/png",
                &[0xff, 0, 0x10],
            )),
        );
        assert_eq!(
            sent(&from)[0].1["params"]["initiator"],
            json!({ "type": "other" })
        );
        assert_eq!(
            agent.kept_content("s", "F", "https://a.test/i.png"),
            Some(json!({ "content": "/wAQ", "base64Encoded": true }))
        );

        let child = ResourceRequest::new("https://a.test/f.html", RequestDestination::Iframe);
        agent.will_send(9, &child, frame(), None);
        let params = sent(&from)[0].1["params"].clone();
        assert_eq!(
            (&params["type"], &params["loaderId"]),
            (&json!("Document"), &params["requestId"])
        );
    }

    /// A failure and a cancel end in `loadingFailed`, told apart; a
    /// request's body is told within the session's limit and kept for
    /// `getRequestPostData`; each session keeps what it was told, and one
    /// that turned `Network` off is told no more.
    #[test]
    fn failures_bodies_and_sessions() {
        let (agent, from) = agent();
        agent.handle(
            "s",
            &command("Network.enable", json!({ "maxPostDataSize": 3 })),
        );
        agent.handle(
            "t",
            &command("Network.enable", json!({ "maxPostDataSize": 10 })),
        );
        sent(&from);
        let mut post = ResourceRequest::new("https://a.test/post", RequestDestination::Empty);
        post.method = "POST".into();
        post.body = Some(b"x=12345".to_vec());
        agent.will_send(1, &post, frame(), None);
        let told = sent(&from);
        let of = |session: &str| {
            told.iter()
                .find(|(to, _)| to == session)
                .map(|(_, message)| message["params"]["request"].clone())
                .unwrap()
        };
        assert!(of("s").get("postData").is_none(), "longer than 3");
        assert_eq!(of("t")["postData"], "x=12345");
        agent.handle(
            "s",
            &command(
                "Network.getRequestPostData",
                json!({ "requestId": "00000000000000ab.1" }),
            ),
        );
        assert_eq!(sent(&from)[0].1["result"]["postData"], "x=12345");

        agent.handle("t", &command("Network.disable", json!({})));
        sent(&from);
        let refused = Err(NetError::new(NetErrorCode::ConnectionRefused, ""));
        agent.did_finish(1, &refused);
        let told = sent(&from);
        assert_eq!(methods(&told, "s"), ["Network.loadingFailed"]);
        assert_eq!(methods(&told, "t"), Vec::<String>::new());
        assert_eq!(
            (
                &told[0].1["params"]["errorText"],
                &told[0].1["params"]["canceled"]
            ),
            (&json!("net::ERR_CONNECTION_REFUSED"), &json!(false))
        );
        agent.will_send(2, &post, frame(), None);
        sent(&from);
        agent.did_cancel(2);
        agent.did_cancel(2);
        let told = sent(&from);
        assert_eq!(told.len(), 1, "once");
        assert_eq!(
            (
                &told[0].1["params"]["errorText"],
                &told[0].1["params"]["canceled"]
            ),
            (&json!("net::ERR_ABORTED"), &json!(true))
        );
        // A new document: the bodies go.
        agent.main_frame_navigated();
        agent.handle(
            "s",
            &command(
                "Network.getRequestPostData",
                json!({ "requestId": "00000000000000ab.1" }),
            ),
        );
        assert!(sent(&from)[0].1.get("error").is_some());
    }

    /// Stacks are wanted once a session asks (`Network.setAttachDebugStack`),
    /// which it can't before turning `Network` on, and no more once it
    /// goes.
    #[test]
    fn stacks_are_wanted_while_a_session_asks() {
        let (agent, from) = agent();
        let attach = command("Network.setAttachDebugStack", json!({ "enabled": true }));
        agent.handle("s", &attach);
        assert!(sent(&from)[0].1.get("error").is_some());
        assert!(!agent.wants_stacks());
        agent.handle("s", &command("Network.enable", json!({})));
        agent.handle("s", &attach);
        assert!(agent.wants_stacks());
        agent.handle(
            "s",
            &command("Network.setAttachDebugStack", json!({ "enabled": false })),
        );
        assert!(!agent.wants_stacks());
        agent.handle("s", &attach);
        agent.detach("s");
        assert!(!agent.wants_stacks() && !agent.enabled());
    }
}
