//! `machopo --remote-debugging-port`: the DevTools protocol over HTTP and
//! websockets, beside the pipe. What a CDP client (Chrome's DevTools,
//! Puppeteer) relies on: where the server listens, `/json/*`, the browser's
//! and a tab's websockets; what keeps pages out (the `Host`, the `Origin`,
//! `PUT` for `/json/new`, ids they can't guess); and the clients kept
//! apart, each with its own sessions and events.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, Command as Process, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, unbounded};
use protocol::automation::Message;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::handshake::client::Response;
use tokio_tungstenite::tungstenite::protocol::frame::Frame as RawFrame;
use tokio_tungstenite::tungstenite::protocol::frame::coding::{Data, OpCode};
use tokio_tungstenite::tungstenite::{self, HandshakeError, Message as Frame, WebSocket};

const TIMEOUT: Duration = Duration::from_secs(30);

/// A `machopo --automation-pipe --remote-debugging-port=0`, its pipe and the
/// server's port. Other modules drive machopo with it too (`devtools`).
pub(crate) struct Machopo {
    child: Child,
    input: Option<ChildStdin>,
    pipe: Receiver<Message>,
    /// The pipe's events read while waiting for an answer.
    pipe_events: Vec<String>,
    next_id: i32,
    pub(crate) port: u16,
    /// `DevTools listening on <this>`.
    pub(crate) browser_url: String,
    /// Its standard error's lines after that one.
    errors: Receiver<String>,
}

impl Machopo {
    pub(crate) fn start() -> Self {
        Self::start_with(&[])
    }

    pub(crate) fn start_with(args: &[&str]) -> Self {
        let mut child = Process::new(env!("CARGO_BIN_EXE_machopo"))
            .args([
                "--automation-pipe",
                "--test-fonts",
                "--remote-debugging-port=0",
            ])
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("machopo starts");
        let input = child.stdin.take();
        let (tx, pipe) = unbounded();
        let output = child.stdout.take().expect("piped");
        std::thread::spawn(move || {
            let mut output = BufReader::new(output);
            let mut bytes = Vec::new();
            while output.read_until(0, &mut bytes).is_ok_and(|n| n > 0) {
                bytes.pop();
                let text = String::from_utf8(std::mem::take(&mut bytes)).expect("UTF-8");
                let message = Message::from_json(&text).unwrap_or_else(|| panic!("{text}"));
                if tx.send(message).is_err() {
                    break;
                }
            }
        });
        let (line_tx, errors) = unbounded();
        let stderr = child.stderr.take().expect("piped");
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if line_tx.send(line).is_err() {
                    break;
                }
            }
        });
        let deadline = Instant::now() + TIMEOUT;
        let browser_url = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = errors
                .recv_timeout(left)
                .expect("machopo says where it listens");
            if let Some(url) = line.strip_prefix("DevTools listening on ") {
                break url.to_owned();
            }
        };
        let port = browser_url
            .strip_prefix("ws://127.0.0.1:")
            .and_then(|rest| rest.split_once('/'))
            .and_then(|(port, _)| port.parse().ok())
            .unwrap_or_else(|| panic!("{browser_url}"));
        Self {
            child,
            input,
            pipe,
            pipe_events: Vec::new(),
            next_id: 0,
            port,
            browser_url,
            errors,
        }
    }

    /// The first line of standard error from now on (to come within
    /// [`TIMEOUT`]) that starts with `prefix`.
    pub(crate) fn error_line(&self, prefix: &str) -> String {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let line = self.errors.recv_timeout(left).expect("the line comes");
            if line.starts_with(prefix) {
                return line;
            }
        }
    }

    /// The pipe's command `method`, for `session`: its answer.
    pub(crate) fn pipe_call(
        &mut self,
        session: Option<&str>,
        method: &str,
        params: Value,
    ) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        let mut command = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            command["sessionId"] = session.into();
        }
        let input = self.input.as_mut().expect("open");
        input.write_all(command.to_string().as_bytes()).unwrap();
        input.write_all(&[0]).unwrap();
        input.flush().unwrap();
        loop {
            match self.pipe.recv_timeout(TIMEOUT).expect("machopo answers") {
                Message::Response(response) if response.id == id => {
                    return match response.result {
                        Ok(result) => json!({ "result": result }),
                        Err(error) => json!({ "error": { "code": error.code } }),
                    };
                }
                Message::Event(event) => self.pipe_events.push(event.event.name().to_owned()),
                Message::Response(_) | Message::Raw { .. } => {}
            }
        }
    }

    /// The pipe's events since they were last taken.
    pub(crate) fn take_pipe_events(&mut self) -> Vec<String> {
        let mut events = std::mem::take(&mut self.pipe_events);
        events.extend(self.pipe.try_iter().filter_map(|message| match message {
            Message::Event(event) => Some(event.event.name().to_owned()),
            Message::Response(_) | Message::Raw { .. } => None,
        }));
        events
    }

    /// `method path` over HTTP with `Host: host`: the status and the body.
    pub(crate) fn http_as(&self, method: &str, path: &str, host: &str) -> (u16, String) {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connects");
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        write!(
            stream,
            "{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let status = response
            .split(' ')
            .nth(1)
            .and_then(|status| status.parse().ok())
            .unwrap_or_else(|| panic!("{response}"));
        let body = response.split_once("\r\n\r\n").map_or("", |(_, body)| body);
        (status, body.to_owned())
    }

    pub(crate) fn http(&self, method: &str, path: &str) -> (u16, String) {
        self.http_as(method, path, &format!("127.0.0.1:{}", self.port))
    }

    /// `/json/list`.
    pub(crate) fn targets(&self) -> Vec<Value> {
        let (status, body) = self.http("GET", "/json/list");
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<Value>(&body)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }

    /// Connects a websocket to `url`, with `Origin: origin` if given.
    pub(crate) fn connect(&self, url: &str, origin: Option<&str>) -> Result<Client, u16> {
        let mut request = url.into_client_request().unwrap();
        if let Some(origin) = origin {
            request
                .headers_mut()
                .insert("Origin", origin.parse().unwrap());
        }
        let stream = TcpStream::connect(("127.0.0.1", self.port)).expect("connects");
        stream.set_read_timeout(Some(TIMEOUT)).unwrap();
        match tungstenite::client(request, stream) {
            Ok((socket, _)) => Ok(Client {
                socket,
                next_id: 0,
                events: Vec::new(),
                page_enabled: false,
            }),
            Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
                Err(status_of(&response))
            }
            Err(e) => panic!("{e}"),
        }
    }

    pub(crate) fn connect_browser(&self) -> Client {
        self.connect(&self.browser_url, None).expect("connects")
    }

    /// How machopo ended, waiting up to [`TIMEOUT`].
    pub(crate) fn ended(&mut self) -> ExitStatus {
        let deadline = Instant::now() + TIMEOUT;
        loop {
            if let Some(status) = self.child.try_wait().expect("waits") {
                return status;
            }
            assert!(Instant::now() < deadline, "machopo didn't end");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

fn status_of(response: &Response) -> u16 {
    response.status().as_u16()
}

impl Drop for Machopo {
    fn drop(&mut self) {
        // A failing test shows whether its machopo still ran and what it said:
        // its standard error is read into `errors`, which only the tests
        // that wait for a line look at.
        if std::thread::panicking() {
            eprintln!("machopo's exit status: {:?}", self.child.try_wait());
            for line in self.errors.try_iter() {
                eprintln!("machopo: {line}");
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A websocket client, and the events it was sent that weren't read yet.
pub(crate) struct Client {
    socket: WebSocket<TcpStream>,
    next_id: u64,
    events: Vec<Value>,
    /// The connection's own session turned `Page` on.
    page_enabled: bool,
}

impl Client {
    /// Whether the connection's own session turned `Page` on.
    pub(crate) fn page_enabled(&self) -> bool {
        self.page_enabled
    }

    /// Sends `method` for `session`: the whole answer, as sent.
    pub(crate) fn call(&mut self, session: Option<&str>, method: &str, params: Value) -> Value {
        let id = self.send(session, method, params);
        self.answer(id)
    }

    /// Sends `method` for `session`, not waiting for its answer: its id.
    pub(crate) fn send(&mut self, session: Option<&str>, method: &str, params: Value) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        if session.is_none() {
            match method {
                "Page.enable" => self.page_enabled = true,
                "Page.disable" => self.page_enabled = false,
                _ => {}
            }
        }
        let mut command = json!({ "id": id, "method": method, "params": params });
        if let Some(session) = session {
            command["sessionId"] = session.into();
        }
        self.socket.send(Frame::text(command.to_string())).unwrap();
        id
    }

    /// The answer to the command `id`, the events before it kept.
    pub(crate) fn answer(&mut self, id: u64) -> Value {
        if let Some(at) = self.events.iter().position(|message| message["id"] == id) {
            return self.events.remove(at);
        }
        loop {
            let message = self.read().expect("an answer");
            if message["id"] == id {
                return message;
            }
            self.events.push(message);
        }
    }

    /// The next message, or `None` once the connection has closed.
    pub(crate) fn read(&mut self) -> Option<Value> {
        loop {
            match self.socket.read() {
                Ok(Frame::Text(text)) => return Some(serde_json::from_str(&text).unwrap()),
                Ok(Frame::Close(_)) | Err(_) => return None,
                Ok(_) => {}
            }
        }
    }

    /// The first event named `method`, kept or to come; the others are
    /// kept.
    pub(crate) fn event(&mut self, method: &str) -> Value {
        if let Some(at) = self
            .events
            .iter()
            .position(|event| event["method"] == method)
        {
            return self.events.remove(at);
        }
        loop {
            let event = self
                .read()
                .unwrap_or_else(|| panic!("closed before {method}"));
            if event["method"] == method {
                return event;
            }
            self.events.push(event);
        }
    }

    /// The events that came already, unread, whose method starts with
    /// `prefix`, in order; the others stay.
    pub(crate) fn take_events(&mut self, prefix: &str) -> Vec<Value> {
        let (taken, kept) = std::mem::take(&mut self.events)
            .into_iter()
            .partition(|event| {
                event["method"]
                    .as_str()
                    .is_some_and(|method| method.starts_with(prefix))
            });
        self.events = kept;
        taken
    }

    /// Keeps `message`, read but not wanted yet, for later reads.
    pub(crate) fn keep(&mut self, message: Value) {
        self.events.push(message);
    }

    /// Whether an answer to the command `id` came already (and is still
    /// unread).
    pub(crate) fn answered(&self, id: u64) -> bool {
        self.events.iter().any(|message| message["id"] == id)
    }

    /// The answers that came already, unread: those to commands this
    /// client never sent among them.
    pub(crate) fn answers(&self) -> Vec<&Value> {
        self.events
            .iter()
            .filter(|message| message.get("id").is_some())
            .collect()
    }

    /// Whether an event `method` came already (and is still unread).
    pub(crate) fn came(&self, method: &str) -> bool {
        self.events.iter().any(|event| event["method"] == method)
    }

    /// Whether the connection closes within a few seconds, whatever else
    /// comes first.
    pub(crate) fn closes(&mut self) -> bool {
        let wait = Duration::from_secs(5);
        // macOS refuses a timeout on a socket the other end has reset.
        if self.socket.get_mut().set_read_timeout(Some(wait)).is_err() {
            return true;
        }
        loop {
            match self.socket.read() {
                Ok(Frame::Close(_)) => return true,
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    return false;
                }
                Err(_) => return true,
                Ok(_) => {}
            }
        }
    }
}

/// Where it listens goes to standard error as Chrome writes it; the
/// endpoints tell the browser and its tabs as Chrome's do.
#[test]
fn it_says_where_it_listens_and_lists_the_tabs() {
    let machopo = Machopo::start();
    let id = machopo
        .browser_url
        .rsplit('/')
        .next()
        .expect("the browser's id");
    assert_eq!(id.len(), 36, "a UUID: {id}");

    let (status, body) = machopo.http("GET", "/json/version");
    assert_eq!(status, 200);
    let version: Value = serde_json::from_str(&body).unwrap();
    assert!(
        version["Browser"]
            .as_str()
            .unwrap()
            .starts_with("Headlessmachopo/")
    );
    assert_eq!(version["Protocol-Version"], "1.3");
    assert_eq!(
        version["webSocketDebuggerUrl"],
        machopo.browser_url.as_str()
    );

    let targets = machopo.targets();
    assert_eq!(targets.len(), 1);
    let target = &targets[0];
    let target_id = target["id"].as_str().unwrap();
    assert_eq!(target["type"], "page");
    assert_eq!(target["url"], "about:blank");
    let page = format!("127.0.0.1:{}/devtools/page/{target_id}", machopo.port);
    assert_eq!(target["webSocketDebuggerUrl"], format!("ws://{page}"));
    assert_eq!(
        target["devtoolsFrontendUrl"],
        format!("devtools://devtools/bundled/inspector.html?ws={page}")
    );
    // `/json` is `/json/list`; anything else isn't there.
    assert_eq!(machopo.http("GET", "/json").0, 200);
    assert_eq!(machopo.http("GET", "/json/protocol").0, 404);
}

/// A tab's target id is drawn at random (a page can't guess one), and is
/// the same to every client.
#[test]
fn target_ids_are_random_and_the_same_to_every_client() {
    let mut machopo = Machopo::start();
    let listed = machopo.targets()[0]["id"].as_str().unwrap().to_owned();
    assert_eq!(listed.len(), 32);
    assert!(listed.chars().all(|c| c.is_ascii_hexdigit()));
    // Not the tab's number, as the ids once were.
    assert_ne!(listed, format!("{:032X}", 0));
    assert_ne!(listed, format!("{:032X}", 1));
    let piped = machopo.pipe_call(None, "Target.getTargets", json!({}));
    assert_eq!(
        piped["result"]["targetInfos"][0]["targetId"],
        listed.as_str()
    );

    let (status, _) = machopo.http("PUT", "/json/new?about:blank");
    assert_eq!(status, 200);
    let ids: Vec<String> = machopo
        .targets()
        .iter()
        .map(|target| target["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids[0], ids[1]);
    // Two browsers draw their own.
    let other = Machopo::start();
    assert_ne!(other.targets()[0]["id"], listed.as_str());
}

/// `/json/new` opens a tab only for a `PUT`, which a page's `<img>` or link
/// can't make; its URL is the query, unescaped.
#[test]
fn only_a_put_opens_a_tab() {
    let machopo = Machopo::start();
    let (status, body) = machopo.http("GET", "/json/new?about:blank");
    assert_eq!(status, 405, "{body}");
    assert!(body.contains("supports only PUT verb"), "{body}");
    assert_eq!(machopo.targets().len(), 1, "nothing was opened");

    let (status, body) = machopo.http("PUT", "/json/new?about%3Ablank%23here");
    assert_eq!(status, 200, "{body}");
    let opened: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(opened["type"], "page");
    let targets = machopo.targets();
    assert_eq!(targets.len(), 2);
    assert!(targets.iter().any(|target| target["id"] == opened["id"]));
    // What isn't a URL opens a blank page, as Chrome's does.
    let (status, body) = machopo.http("PUT", "/json/new?not%20a%20url");
    assert_eq!(status, 200, "{body}");
    let opened: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(opened["url"], "about:blank");
}

/// `/json/activate/<id>` shows the tab and `/json/close/<id>` closes it;
/// an id no tab has is a 404.
#[test]
fn tabs_are_shown_and_closed_by_id() {
    let machopo = Machopo::start();
    let (_, body) = machopo.http("PUT", "/json/new");
    let id = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let (status, body) = machopo.http("GET", &format!("/json/activate/{id}"));
    assert_eq!((status, body.as_str()), (200, "Target activated"));
    let (status, body) = machopo.http("GET", "/json/activate/NOPE");
    assert_eq!((status, body.as_str()), (404, "No such target id: NOPE"));
    let (status, body) = machopo.http("GET", &format!("/json/close/{id}"));
    assert_eq!((status, body.as_str()), (200, "Target is closing"));
    assert!(
        machopo
            .targets()
            .iter()
            .all(|target| target["id"] != id.as_str())
    );
    assert_eq!(machopo.http("GET", &format!("/json/close/{id}")).0, 404);
}

/// A request for the server by a name other than `localhost` is refused:
/// a page whose domain resolves to 127.0.0.1 is same-origin with it (DNS
/// rebinding). Its address and `localhost` are let in.
#[test]
fn only_its_address_and_localhost_are_its_host() {
    let machopo = Machopo::start();
    let port = machopo.port;
    let (status, body) = machopo.http_as("GET", "/json/list", &format!("evil.test:{port}"));
    assert_eq!(status, 500);
    assert!(body.contains("not an IP address or localhost"), "{body}");
    assert_eq!(
        machopo
            .http_as("GET", "/json/version", &format!("localhost:{port}"))
            .0,
        200
    );
    // The websocket's handshake too.
    let url = machopo.browser_url.replace("127.0.0.1", "evil.test");
    let mut request = url.into_client_request().unwrap();
    request
        .headers_mut()
        .insert("Host", format!("evil.test:{port}").parse().unwrap());
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    match tungstenite::client(request, stream) {
        Err(HandshakeError::Failure(tungstenite::Error::Http(response))) => {
            assert_eq!(status_of(&response), 500);
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

/// A page can open a websocket anywhere: one that comes with an `Origin`
/// is refused unless `--remote-allow-origins` lets it in. Tools send none;
/// Chrome's DevTools' own pages are let in.
#[test]
fn pages_connect_only_from_the_origins_allowed() {
    let machopo = Machopo::start();
    let url = machopo.browser_url.clone();
    assert_eq!(
        machopo.connect(&url, Some("http://evil.test")).err(),
        Some(403)
    );
    let page = format!(
        "ws://127.0.0.1:{}/devtools/page/{}",
        machopo.port,
        machopo.targets()[0]["id"].as_str().unwrap()
    );
    assert_eq!(
        machopo.connect(&page, Some("http://evil.test")).err(),
        Some(403)
    );
    assert!(machopo.connect(&url, None).is_ok());
    assert!(machopo.connect(&url, Some("devtools://devtools")).is_ok());

    let allowed = Machopo::start_with(&["--remote-allow-origins=http://evil.test"]);
    let url = allowed.browser_url.clone();
    assert!(allowed.connect(&url, Some("http://evil.test")).is_ok());
    assert_eq!(
        allowed.connect(&url, Some("http://other.test")).err(),
        Some(403)
    );
}

/// The browser's websocket speaks what the pipe does: the browser's
/// methods, flat sessions and their events. Another browser id is no
/// browser.
#[test]
fn the_browsers_websocket_speaks_the_pipes_protocol() {
    let machopo = Machopo::start();
    let wrong = format!("ws://127.0.0.1:{}/devtools/browser/nope", machopo.port);
    assert_eq!(machopo.connect(&wrong, None).err(), Some(404));

    let mut client = machopo.connect_browser();
    let version = client.call(None, "Browser.getVersion", json!({}));
    assert!(
        version["result"]["product"]
            .as_str()
            .unwrap()
            .starts_with("Headlessmachopo/")
    );
    let targets = client.call(None, "Target.getTargets", json!({}));
    let target = targets["result"]["targetInfos"][0]["targetId"].clone();
    let attached = client.call(
        None,
        "Target.attachToTarget",
        json!({ "targetId": target, "flatten": true }),
    );
    let session = attached["result"]["sessionId"].as_str().unwrap().to_owned();
    client.call(
        Some(&session),
        "Page.setLifecycleEventsEnabled",
        json!({ "enabled": true }),
    );
    let navigated = client.call(
        Some(&session),
        "Page.navigate",
        json!({ "url": "about:blank#a" }),
    );
    assert_eq!(navigated["sessionId"], session.as_str());
    let load = client.event("Page.lifecycleEvent");
    assert_eq!(load["sessionId"], session.as_str());
    // The method isn't there.
    let unknown = client.call(None, "Fly.away", json!({}));
    assert_eq!(unknown["error"]["code"], -32601);
}

/// A tab's own websocket is the tab's session: commands need no session,
/// and what it is sent carries none. It closes with the tab.
#[test]
fn a_tabs_websocket_is_its_session() {
    let machopo = Machopo::start();
    let nope = format!("ws://127.0.0.1:{}/devtools/page/NOPE", machopo.port);
    assert_eq!(machopo.connect(&nope, None).err(), Some(404));

    let id = machopo.targets()[0]["id"].as_str().unwrap().to_owned();
    let page = format!("ws://127.0.0.1:{}/devtools/page/{id}", machopo.port);
    let mut client = machopo.connect(&page, None).expect("connects");
    let info = client.call(None, "Target.getTargetInfo", json!({}));
    assert_eq!(info["result"]["targetInfo"]["targetId"], id.as_str());
    assert!(info.get("sessionId").is_none(), "{info}");
    client.call(
        None,
        "Page.setLifecycleEventsEnabled",
        json!({ "enabled": true }),
    );
    let navigated = client.call(None, "Page.navigate", json!({ "url": "about:blank#b" }));
    assert!(navigated["result"]["frameId"].is_string(), "{navigated}");
    assert!(navigated.get("sessionId").is_none());
    let load = client.event("Page.lifecycleEvent");
    assert!(load.get("sessionId").is_none(), "{load}");

    // A second tab, so closing this one doesn't end the browser.
    assert_eq!(machopo.http("PUT", "/json/new").0, 200);
    assert_eq!(machopo.http("GET", &format!("/json/close/{id}")).0, 200);
    let detached = client.event("Inspector.detached");
    assert_eq!(detached["params"]["reason"], "target_closed");
    assert!(client.closes());
}

/// Each client has its own sessions and asks for its own events; one going
/// leaves the others. In automation mode every client has the `Machopo` domain.
#[test]
fn clients_keep_their_own_sessions_and_events() {
    let mut machopo = Machopo::start();
    let target = machopo.targets()[0]["id"].clone();
    let attached = machopo.pipe_call(
        None,
        "Target.attachToTarget",
        json!({ "targetId": target, "flatten": true }),
    );
    let pipes = attached["result"]["sessionId"].as_str().unwrap().to_owned();

    let mut client = machopo.connect_browser();
    // The pipe's session isn't the websocket's.
    let refused = client.call(Some(&pipes), "Page.enable", json!({}));
    assert_eq!(refused["error"]["code"], -32001, "{refused}");
    let refused = client.call(Some(&pipes), "Target.getTargetInfo", json!({}));
    assert_eq!(refused["error"]["code"], -32001, "{refused}");
    // Only the client that asked hears of targets.
    client.call(
        None,
        "Target.setDiscoverTargets",
        json!({ "discover": true }),
    );
    let told = client.event("Target.targetCreated");
    assert_eq!(told["params"]["targetInfo"]["targetId"], target);
    assert_eq!(machopo.http("PUT", "/json/new").0, 200);
    client.event("Target.targetCreated");
    machopo.pipe_call(None, "Browser.getVersion", json!({}));
    assert!(
        !machopo
            .take_pipe_events()
            .iter()
            .any(|event| event == "Target.targetCreated"),
        "the pipe didn't ask"
    );
    // Automation mode's `Machopo` domain, to any client.
    let title = client.call(None, "Machopo.getTitle", json!({}));
    assert!(title["result"]["title"].is_string(), "{title}");

    // Another client, then the first goes: the pipe and the other stay.
    let mut other = machopo.connect_browser();
    drop(client);
    let still = machopo.pipe_call(Some(&pipes), "Page.enable", json!({}));
    assert!(still.get("result").is_some(), "{still}");
    let version = other.call(None, "Browser.getVersion", json!({}));
    assert!(version.get("result").is_some(), "{version}");
}

/// A frame over its limit (16 MiB), or a message over its own (64 MiB) in
/// frames under it, closes the websocket it came on; the browser and its
/// other clients go on.
#[test]
fn a_frame_or_a_message_over_the_limit_closes_its_connection() {
    let machopo = Machopo::start();
    let mut other = machopo.connect_browser();
    // One frame, of a message under the message's limit.
    let mut client = machopo.connect_browser();
    let frame = RawFrame::message(vec![b' '; (16 << 20) + 1], OpCode::Data(Data::Text), true);
    // The server may close before it has read it all.
    let _ = client.socket.send(Frame::Frame(frame));
    assert!(client.closes(), "a frame over 16 MiB");
    // Five frames of 15 MiB, one message.
    let mut client = machopo.connect_browser();
    for part in 0..5 {
        let code = if part == 0 {
            Data::Text
        } else {
            Data::Continue
        };
        let frame = RawFrame::message(vec![b' '; 15 << 20], OpCode::Data(code), part == 4);
        if client.socket.send(Frame::Frame(frame)).is_err() {
            break;
        }
    }
    assert!(client.closes(), "a message over 64 MiB");
    let version = other.call(None, "Browser.getVersion", json!({}));
    assert!(version.get("result").is_some(), "{version}");
}

/// A message over the limit on the pipe ends it, and so the browser, as
/// the pipe's input ending does.
#[test]
fn a_message_over_the_limit_ends_the_pipe() {
    let mut machopo = Machopo::start();
    let input = machopo.input.as_mut().expect("open");
    let chunk = vec![b' '; 1 << 20];
    for _ in 0..=64 {
        if input.write_all(&chunk).is_err() {
            break;
        }
    }
    let status = machopo.ended();
    assert!(status.success(), "{status}");
}

/// `Browser.close` from any client ends the browser.
#[test]
fn any_client_closes_the_browser() {
    let mut machopo = Machopo::start();
    let mut client = machopo.connect_browser();
    let closed = client.call(None, "Browser.close", json!({}));
    assert!(closed.get("result").is_some(), "{closed}");
    let status = machopo.ended();
    assert!(status.success(), "{status}");
}
