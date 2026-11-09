//! DevTools' `Page` domain as the renderer answers it (Blink's
//! `InspectorPageAgent`): the frame tree (`Page.getResourceTree`,
//! `getFrameTree`) with the subresources each document loaded, a
//! document's markup or a subresource's body `Network` kept
//! (`getResourceContent`), and, for a
//! session that turned `Page` on, the frames attached, navigated and
//! detached. The browser answers the rest of `Page` (navigating, the
//! lifecycle, screenshots) and passes `Page.enable` and `disable` on after
//! noting them, as Chromium's handlers fall through.
//!
//! The agent keeps the frames as last told (`update`), and answers with
//! the tree as of then.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crossbeam_channel::Sender;
use ipc::TimeTicks;
use protocol::network::LoadResult;
use protocol::renderer::RendererHostMsg;
use serde_json::{Value, json};

use super::network_agent::NetworkAgent;

/// JSON-RPC's "invalid params".
const INVALID_PARAMS: i64 = -32602;
/// CDP's error for what the command can't do.
const SERVER_ERROR: i64 = -32000;

/// A subresource a document loaded, as `Page.getResourceTree` lists it
/// (`Page.FrameResource`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameResource {
    /// Without its fragment.
    pub(crate) url: String,
    /// `Network.ResourceType`.
    pub(crate) kind: &'static str,
    pub(crate) mime_type: String,
    /// Its body's length, decoded.
    pub(crate) content_size: usize,
    pub(crate) failed: bool,
}

/// The subresources a document loaded, in order, one per URL (Blink's
/// document's `ResourceFetcher`, whose resources DevTools'
/// `Page.getResourceTree` lists): kept whether or not DevTools is there,
/// as they are little.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct FrameResources {
    list: Vec<FrameResource>,
    by_url: HashMap<String, usize>,
}

impl FrameResources {
    /// `resource` loaded: in place of what was loaded from its URL before.
    pub(crate) fn add(&mut self, resource: FrameResource) {
        match self.by_url.get(&resource.url) {
            Some(&at) => self.list[at] = resource,
            None => {
                self.by_url.insert(resource.url.clone(), self.list.len());
                self.list.push(resource);
            }
        }
    }

    /// A load of `kind` ended with `result`; `url`, the request's, names
    /// one that failed (none: it isn't listed).
    pub(crate) fn add_result(
        &mut self,
        kind: &'static str,
        url: Option<&str>,
        result: &LoadResult,
    ) {
        let without_fragment = |url: &str| url.split('#').next().unwrap_or(url).to_owned();
        let resource = match result {
            Ok(response) => FrameResource {
                url: without_fragment(&response.url),
                kind,
                mime_type: response.mime_type.clone(),
                content_size: response.body.len(),
                failed: false,
            },
            Err(_) => {
                let Some(url) = url else {
                    return;
                };
                FrameResource {
                    url: without_fragment(url),
                    kind,
                    mime_type: String::new(),
                    content_size: 0,
                    failed: true,
                }
            }
        };
        self.add(resource);
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = &FrameResource> {
        self.list.iter()
    }
}

/// A frame, as DevTools' `Page.Frame` tells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FrameInfo {
    /// What DevTools calls it: the target id for the main frame.
    pub(crate) id: String,
    pub(crate) parent_id: Option<String>,
    /// Its document's: the browser's navigation id for the main frame's
    /// (as `Page.lifecycleEvent` has it), the document's number for a
    /// child frame's (whose navigations the renderer makes).
    pub(crate) loader_id: String,
    /// Which document it is, to tell a new one from the same.
    pub(crate) document: u64,
    /// Its iframe's `name` (a child frame's, if none, empty).
    pub(crate) name: Option<String>,
    /// Its document is its initial `about:blank`, which no navigation
    /// committed: not told as navigated to.
    pub(crate) initial: bool,
    /// A navigation of the frame is on its way (another document comes).
    pub(crate) navigating: bool,
    pub(crate) url: String,
    /// As Blink's `SecurityOrigin::ToRawString` (an opaque one, "://").
    pub(crate) origin: String,
    pub(crate) mime_type: String,
    /// `Page.SecureContextType`.
    pub(crate) secure_context: &'static str,
    /// The document's markup as it came.
    pub(crate) source: Rc<str>,
    /// The subresources its document loaded, as of when it was told (the
    /// document's, shared until it loads another).
    pub(crate) resources: Rc<FrameResources>,
    /// The stack of the script whose iframe made it, if DevTools wanted it
    /// then (`Page.frameAttached`'s).
    pub(crate) creation_stack: Option<Rc<str>>,
}

impl FrameInfo {
    fn to_json(&self) -> Value {
        let (url, fragment) = match self.url.split_once('#') {
            Some((url, fragment)) => (url, Some(format!("#{fragment}"))),
            None => (self.url.as_str(), None),
        };
        let mut frame = json!({
            "id": self.id,
            "loaderId": self.loader_id,
            "url": url,
            // machopo has no public suffix list: its registrable domain isn't
            // known (Chrome's is empty for an IP address too).
            "domainAndRegistry": "",
            "securityOrigin": self.origin,
            "mimeType": self.mime_type,
            "secureContextType": self.secure_context,
            "crossOriginIsolatedContextType": "NotIsolated",
            "gatedAPIFeatures": [],
            "securityOriginDetails": { "isLocalhost": is_localhost(&self.origin) },
        });
        // machopo tells no ads apart.
        frame["adFrameStatus"] = if self.parent_id.is_some() {
            json!({ "adFrameType": "none", "explanations": [] })
        } else {
            json!({ "adFrameType": "none" })
        };
        if let Some(parent) = &self.parent_id {
            frame["parentId"] = parent.as_str().into();
        }
        if let Some(name) = &self.name {
            frame["name"] = name.as_str().into();
        }
        if let Some(fragment) = fragment {
            frame["urlFragment"] = fragment.into();
        }
        frame
    }
}

/// A frame's document's lifecycle so far (Blink's `InspectorPageAgent::
/// LifecycleEvent`s since its `init`), kept whether or not a session
/// listens, for one that turns the events on later.
#[derive(Debug, Default)]
struct Lifecycle {
    /// Which document (`FrameInfo::document`).
    document: u64,
    /// The events, with their times.
    events: Vec<(&'static str, f64)>,
}

/// Blink's `InspectorPageAgent`, for every session of the page.
pub(crate) struct PageAgent {
    to_browser: Sender<RendererHostMsg>,
    /// What has the subresources' bodies, for `Page.getResourceContent`.
    network_agent: Rc<NetworkAgent>,
    /// The sessions that turned `Page` on.
    enabled: RefCell<Vec<String>>,
    /// Those that turned the lifecycle events on too
    /// (`Page.setLifecycleEventsEnabled`).
    lifecycle: RefCell<Vec<String>>,
    /// Each frame's document's lifecycle so far, by frame id.
    lifecycles: RefCell<HashMap<String, Lifecycle>>,
    /// The frames as last told, parents first.
    frames: RefCell<Vec<FrameInfo>>,
}

impl PageAgent {
    pub(crate) fn new(
        to_browser: Sender<RendererHostMsg>,
        network_agent: Rc<NetworkAgent>,
    ) -> Self {
        Self {
            to_browser,
            network_agent,
            enabled: RefCell::new(Vec::new()),
            lifecycle: RefCell::new(Vec::new()),
            lifecycles: RefCell::new(HashMap::new()),
            frames: RefCell::new(Vec::new()),
        }
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
    }

    /// `method`'s event, `params`, to every session that turned `Page` on.
    fn tell(&self, method: &str, params: Value) {
        let message = json!({ "method": method, "params": params });
        for session in self.enabled.borrow().iter() {
            self.send(session, &message);
        }
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
            "Page.enable" => {
                let mut enabled = self.enabled.borrow_mut();
                if !enabled.iter().any(|known| known == session) {
                    enabled.push(session.to_owned());
                }
                Ok(json!({}))
            }
            "Page.disable" => {
                self.enabled.borrow_mut().retain(|known| known != session);
                self.lifecycle.borrow_mut().retain(|known| known != session);
                Ok(json!({}))
            }
            "Page.setLifecycleEventsEnabled" => {
                let on = params
                    .get("enabled")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let was = self.lifecycle.borrow().iter().any(|known| known == session);
                if on {
                    if !was {
                        self.lifecycle.borrow_mut().push(session.to_owned());
                    }
                    // Each time, `Page` on or not, as Blink's agent does.
                    self.replay_lifecycle(session);
                } else {
                    self.lifecycle.borrow_mut().retain(|known| known != session);
                }
                Ok(json!({}))
            }
            "Page.getFrameTree" => Ok(json!({ "frameTree": self.tree(false) })),
            "Page.getResourceTree" => Ok(json!({ "frameTree": self.tree(true) })),
            "Page.getResourceContent" => {
                if self.enabled.borrow().iter().any(|known| known == session) {
                    self.resource_content(session, &params)
                } else {
                    Err((SERVER_ERROR, "Agent is not enabled.".into()))
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

    /// The main frame's document fired `DOMContentLoaded`
    /// (`Page.domContentEventFired`, which DevTools' Network panel draws).
    pub(crate) fn dom_content_loaded(&self) {
        self.tell(
            "Page.domContentEventFired",
            json!({ "timestamp": TimeTicks::now().as_secs_f64() }),
        );
    }

    /// The main frame's document fired `load` (`Page.loadEventFired`).
    pub(crate) fn load_fired(&self) {
        self.tell(
            "Page.loadEventFired",
            json!({ "timestamp": TimeTicks::now().as_secs_f64() }),
        );
    }

    /// Whether any session has `Page` on: a frame script makes is told with
    /// the stack that made it.
    pub(crate) fn any_enabled(&self) -> bool {
        !self.enabled.borrow().is_empty()
    }

    /// Frame `frame_id`'s document `document` (`FrameInfo::document`),
    /// loaded by `loader_id`, reached `name` at `at` (`Page.lifecycleEvent`,
    /// to the sessions that turned `Page` and the events on): kept as its
    /// lifecycle, which another document of the frame starts afresh.
    pub(crate) fn lifecycle_event(
        &self,
        frame_id: &str,
        document: u64,
        loader_id: &str,
        name: &'static str,
        at: TimeTicks,
    ) {
        let timestamp = at.as_secs_f64();
        {
            let mut lifecycles = self.lifecycles.borrow_mut();
            let lifecycle = lifecycles.entry(frame_id.to_owned()).or_default();
            if lifecycle.document != document || name == "init" {
                *lifecycle = Lifecycle {
                    document,
                    events: Vec::new(),
                };
            }
            lifecycle.events.push((name, timestamp));
        }
        let message = lifecycle_message(frame_id, loader_id, name, timestamp);
        let enabled = self.enabled.borrow();
        for session in self
            .lifecycle
            .borrow()
            .iter()
            .filter(|session| enabled.contains(session))
        {
            self.send(session, &message);
        }
    }

    /// `session` turned the lifecycle events on: each frame's document's
    /// so far, after a `commit` for it (as Blink's agent tells them as they
    /// are turned on). The events to come go only to a session that turned
    /// `Page` on too, as Blink's.
    fn replay_lifecycle(&self, session: &str) {
        let now = TimeTicks::now().as_secs_f64();
        let lifecycles = self.lifecycles.borrow();
        for frame in self.frames.borrow().iter() {
            self.send(
                session,
                &lifecycle_message(&frame.id, &frame.loader_id, "commit", now),
            );
            let Some(lifecycle) = lifecycles
                .get(&frame.id)
                .filter(|lifecycle| lifecycle.document == frame.document)
            else {
                continue;
            };
            for &(name, timestamp) in &lifecycle.events {
                if name != "init" {
                    self.send(
                        session,
                        &lifecycle_message(&frame.id, &frame.loader_id, name, timestamp),
                    );
                }
            }
        }
    }

    /// Frame `frame_id`'s document has loaded (`Page.frameStoppedLoading`).
    pub(crate) fn stopped_loading(&self, frame_id: &str) {
        self.tell("Page.frameStoppedLoading", json!({ "frameId": frame_id }));
    }

    /// `session` has gone.
    pub(crate) fn detach(&self, session: &str) {
        self.lifecycle.borrow_mut().retain(|known| known != session);
        self.enabled.borrow_mut().retain(|known| known != session);
    }

    /// The tree from the main frame down (`Page.FrameTree`, or with
    /// `resources`, `Page.FrameResourceTree`).
    fn tree(&self, resources: bool) -> Value {
        let frames = self.frames.borrow();
        let Some(main) = frames.iter().find(|frame| frame.parent_id.is_none()) else {
            return Value::Null;
        };
        subtree(&frames, main, resources)
    }

    /// `Page.getResourceContent`: a frame's document, as it came, or a
    /// subresource's body as `session`'s `Network` kept it (Blink's takes
    /// it from the memory cache; machopo has none, so one loaded before
    /// `Network.enable` isn't there).
    fn resource_content(&self, session: &str, params: &Value) -> Result<Value, (i64, String)> {
        let (Some(frame_id), Some(url)) = (
            params.get("frameId").and_then(Value::as_str),
            params.get("url").and_then(Value::as_str),
        ) else {
            return Err((INVALID_PARAMS, "frameId and url are needed".into()));
        };
        let frames = self.frames.borrow();
        let Some(frame) = frames.iter().find(|frame| frame.id == frame_id) else {
            return Err((SERVER_ERROR, "No frame for given id found".into()));
        };
        let without_fragment = |url: &str| url.split('#').next().unwrap_or(url).to_owned();
        let url = without_fragment(url);
        if url == without_fragment(&frame.url) {
            return Ok(json!({ "content": &*frame.source, "base64Encoded": false }));
        }
        self.network_agent
            .kept_content(session, frame_id, &url)
            .ok_or_else(|| (SERVER_ERROR, "No resource with given URL found".into()))
    }

    /// The frames now, parents first: what changed since last told goes
    /// to the sessions that turned `Page` on (frames gone, then new and
    /// navigated ones). The main frame's new document is told before its
    /// realm is (as Chrome tells `Page.frameNavigated` before
    /// `Runtime.executionContextCreated`): its old document's frames go
    /// with it.
    pub(crate) fn update(&self, frames: Vec<FrameInfo>) {
        let old = std::mem::replace(&mut *self.frames.borrow_mut(), frames);
        let new = self.frames.borrow().clone();
        self.lifecycles
            .borrow_mut()
            .retain(|id, _| new.iter().any(|frame| frame.id == *id));
        if self.enabled.borrow().is_empty() {
            return;
        }
        // Children before their parents.
        for gone in old.iter().rev() {
            if !new.iter().any(|frame| frame.id == gone.id) {
                self.tell(
                    "Page.frameDetached",
                    json!({ "frameId": gone.id, "reason": "remove" }),
                );
            }
        }
        for frame in &new {
            match old.iter().find(|known| known.id == frame.id) {
                None => {
                    if let Some(parent) = &frame.parent_id {
                        let mut attached = json!({ "frameId": frame.id, "parentFrameId": parent });
                        if let Some(stack) = frame
                            .creation_stack
                            .as_deref()
                            .and_then(|stack| serde_json::from_str::<Value>(stack).ok())
                        {
                            attached["stack"] = stack;
                        }
                        self.tell("Page.frameAttached", attached);
                    }
                    if frame.initial {
                        self.initial_document(frame);
                    } else {
                        self.navigated(frame);
                    }
                }
                Some(known) if known.document != frame.document => self.navigated(frame),
                Some(_) => {}
            }
        }
    }

    /// `frame`'s initial `about:blank` starts, as Blink's; one that stays
    /// (an iframe with no `src`) has loaded as it starts, and its network
    /// is quiet: its whole lifecycle, as Chrome tells it (Puppeteer waits
    /// for every frame's).
    fn initial_document(&self, frame: &FrameInfo) {
        let now = TimeTicks::now();
        let event = |name| {
            self.lifecycle_event(&frame.id, frame.document, &frame.loader_id, name, now);
        };
        event("init");
        if frame.navigating {
            return;
        }
        self.tell("Page.frameStartedLoading", json!({ "frameId": frame.id }));
        event("DOMContentLoaded");
        event("load");
        self.tell("Page.frameStoppedLoading", json!({ "frameId": frame.id }));
        event("networkAlmostIdle");
        event("networkIdle");
    }

    /// `frame` has a new document: it started loading, and the document's
    /// lifecycle starts (`init`, before `frameNavigated`, as in Chrome).
    fn navigated(&self, frame: &FrameInfo) {
        self.tell("Page.frameStartedLoading", json!({ "frameId": frame.id }));
        self.lifecycle_event(
            &frame.id,
            frame.document,
            &frame.loader_id,
            "init",
            TimeTicks::now(),
        );
        self.tell(
            "Page.frameNavigated",
            json!({ "frame": frame.to_json(), "type": "Navigation" }),
        );
    }
}

/// `Page.lifecycleEvent`.
fn lifecycle_message(frame_id: &str, loader_id: &str, name: &str, timestamp: f64) -> Value {
    json!({
        "method": "Page.lifecycleEvent",
        "params": {
            "frameId": frame_id,
            "loaderId": loader_id,
            "name": name,
            "timestamp": timestamp,
        },
    })
}

/// `frame` and the frames below it, as `Page.FrameTree` (with
/// `resources`, `FrameResourceTree`).
fn subtree(frames: &[FrameInfo], frame: &FrameInfo, resources: bool) -> Value {
    let mut tree = json!({ "frame": frame.to_json() });
    if resources {
        let listed: Vec<Value> = frame
            .resources
            .iter()
            .map(|resource| {
                let mut listed = json!({
                    "url": resource.url,
                    "type": resource.kind,
                    "mimeType": resource.mime_type,
                    "contentSize": resource.content_size,
                });
                if resource.failed {
                    listed["failed"] = true.into();
                }
                listed
            })
            .collect();
        tree["resources"] = listed.into();
    }
    let children: Vec<Value> = frames
        .iter()
        .filter(|child| child.parent_id.as_deref() == Some(frame.id.as_str()))
        .map(|child| subtree(frames, child, resources))
        .collect();
    if !children.is_empty() {
        tree["childFrames"] = children.into();
    }
    tree
}

/// Whether `origin` is the machine itself's (`localhost`, a loopback
/// address), as Chrome's `securityOriginDetails` tells it.
fn is_localhost(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    match url.host() {
        Some(url::Host::Domain(host)) => host == "localhost" || host.ends_with(".localhost"),
        Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
        Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A URL loaded again takes its first load's place, as Blink's
    /// fetcher keeps one resource per URL; a failure is listed by the
    /// request's URL, without its fragment, and one with no URL isn't.
    #[test]
    fn a_documents_resources_are_one_per_url() {
        let resource = |url: &str, failed: bool| FrameResource {
            url: url.to_owned(),
            kind: "Image",
            mime_type: String::new(),
            content_size: 0,
            failed,
        };
        let mut resources = FrameResources::default();
        resources.add(resource("https://a.test/1", true));
        resources.add(resource("https://a.test/2", false));
        resources.add(resource("https://a.test/1", false));
        let failed = Err(protocol::network::NetError::new(
            protocol::network::NetErrorCode::ConnectionRefused,
            "",
        ));
        resources.add_result("Script", Some("https://a.test/3#x"), &failed);
        resources.add_result("Script", None, &failed);
        let listed: Vec<_> = resources
            .iter()
            .map(|resource| (resource.url.as_str(), resource.failed))
            .collect();
        assert_eq!(
            listed,
            [
                ("https://a.test/1", false),
                ("https://a.test/2", false),
                ("https://a.test/3", true)
            ]
        );
    }
}
