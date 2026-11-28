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

use ipc::TimeTicks;
use protocol::control_channel::BrowserSender;
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
                url: without_fragment(&response.head.url),
                kind,
                mime_type: response.head.mime_type.clone(),
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
            "domainAndRegistry": domain_and_registry(url),
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
    to_browser: BrowserSender,
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
    /// The frames told attached as they were made (`frame_attached`) that
    /// `frames` doesn't have yet, by id with their parent's: those a frame
    /// detached at once takes with it.
    attached: RefCell<Vec<(String, String)>>,
    /// A child frame's document committing (`child_committing`), told as
    /// the engine makes it (`child_document_created`).
    committing: RefCell<Option<Committing>>,
}

/// A child frame's document as it commits, before the engine has made it.
struct Committing {
    /// All but its number (`FrameInfo::document`), which the engine gives.
    frame: FrameInfo,
    /// Its loader is named by the document's number: none of the
    /// browser's navigations brought it (the renderer committed it).
    loader_is_document: bool,
}

impl PageAgent {
    pub(crate) fn new(to_browser: BrowserSender, network_agent: Rc<NetworkAgent>) -> Self {
        Self {
            to_browser,
            network_agent,
            enabled: RefCell::new(Vec::new()),
            lifecycle: RefCell::new(Vec::new()),
            lifecycles: RefCell::new(HashMap::new()),
            frames: RefCell::new(Vec::new()),
            attached: RefCell::new(Vec::new()),
            committing: RefCell::new(None),
        }
    }

    /// Child frame `frame.id`'s document commits: told once the engine has
    /// made it (`child_document_created`), its number then known, and its
    /// loader's if `loader_is_document`.
    pub(crate) fn child_committing(&self, frame: FrameInfo, loader_is_document: bool) {
        *self.committing.borrow_mut() = Some(Committing {
            frame,
            loader_is_document,
        });
    }

    /// The engine made child frame `id`'s document committing, numbered
    /// `document`, before its realm: told navigated now (before
    /// `Runtime.executionContextCreated` and the document's script, as
    /// Chrome tells `Page.frameNavigated` as the navigation commits), and
    /// the frame's from now on; `update` finds it the same document.
    pub(crate) fn child_document_created(&self, id: &str, document: u64) {
        let Some(Committing {
            mut frame,
            loader_is_document,
        }) = self
            .committing
            .borrow_mut()
            .take_if(|committing| committing.frame.id == id)
        else {
            return;
        };
        frame.document = document;
        if loader_is_document {
            frame.loader_id = document.to_string();
        }
        {
            let mut frames = self.frames.borrow_mut();
            match frames.iter_mut().find(|known| known.id == frame.id) {
                Some(known) => *known = frame.clone(),
                None => frames.push(frame.clone()),
            }
        }
        self.attached
            .borrow_mut()
            .retain(|(known, _)| *known != frame.id);
        self.navigated(&frame);
    }

    /// Child frame `id` made in frame `parent`, by script whose stack (as
    /// `Runtime.StackTrace`'s JSON) is `stack`: told at once, as Blink's
    /// `InspectorPageAgent::FrameAttachedToParent` tells it in the frame's
    /// constructor (before the frame's context, and before the browser
    /// tells of its navigation). Its documents are told as `update` finds
    /// them.
    pub(crate) fn frame_attached(&self, id: String, parent: String, stack: Option<&str>) {
        if self.enabled.borrow().is_empty() {
            return;
        }
        let mut attached = json!({ "frameId": id, "parentFrameId": parent });
        if let Some(stack) = stack.and_then(|stack| serde_json::from_str::<Value>(stack).ok()) {
            attached["stack"] = stack;
        }
        self.tell("Page.frameAttached", attached);
        self.attached.borrow_mut().push((id, parent));
    }

    /// Child frame `id` gone, and the frames in it: each told detached at
    /// once, those in it first (Blink's `FrameDetachedFromParent`, in each
    /// frame's `DetachImpl`), and no longer the agent's.
    pub(crate) fn frame_detached(&self, id: &str) {
        let mut frames = self.frames.borrow_mut();
        let mut attached = self.attached.borrow_mut();
        // Parents before the frames in them.
        let known: Vec<(String, Option<String>)> = frames
            .iter()
            .map(|frame| (frame.id.clone(), frame.parent_id.clone()))
            .chain(
                attached
                    .iter()
                    .map(|(frame, parent)| (frame.clone(), Some(parent.clone()))),
            )
            .collect();
        let mut gone = vec![id.to_owned()];
        for (frame, parent) in &known {
            if parent.as_ref().is_some_and(|parent| gone.contains(parent)) && !gone.contains(frame)
            {
                gone.push(frame.clone());
            }
        }
        frames.retain(|frame| !gone.contains(&frame.id));
        attached.retain(|(frame, _)| !gone.contains(frame));
        drop((frames, attached));
        self.lifecycles
            .borrow_mut()
            .retain(|frame, _| !gone.contains(frame));
        if self.enabled.borrow().is_empty() {
            return;
        }
        for frame in gone.iter().rev() {
            self.tell(
                "Page.frameDetached",
                json!({ "frameId": frame, "reason": "remove" }),
            );
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
    /// agent's: whether it was ([`Self::dispatch`], its answer sent).
    #[cfg(test)]
    pub(crate) fn handle(&self, session: &str, message: &str) -> bool {
        self.dispatch(session, message)
            .map(|answer| self.send(session, &answer))
            .is_some()
    }

    /// `message`, a command of `session`'s, taken if it is one of the
    /// agent's: its answer, for the render thread to send once the
    /// browser has what the command turned on.
    pub(crate) fn dispatch(&self, session: &str, message: &str) -> Option<Value> {
        let command = serde_json::from_str::<Value>(message).ok()?;
        let method = command.get("method").and_then(Value::as_str)?;
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
            _ => return None,
        };
        let id = command.get("id").cloned().unwrap_or(Value::Null);
        let answer = match result {
            Ok(result) => json!({ "id": id, "result": result }),
            Err((code, message)) => {
                json!({ "id": id, "error": { "code": code, "message": message } })
            }
        };
        Some(answer)
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

    /// What `session` has on, for the tab's next renderer to go on from
    /// (Blink's `InspectorAgentState`): none if nothing.
    pub(crate) fn state(&self, session: &str) -> Option<Value> {
        let enabled = self.enabled.borrow().iter().any(|known| known == session);
        let lifecycle = self.lifecycle.borrow().iter().any(|known| known == session);
        (enabled || lifecycle).then(|| json!({ "enabled": enabled, "lifecycle": lifecycle }))
    }

    /// `session` goes on from `state` (`state`'s), as the renderer before
    /// had it: the lifecycle events so far come again, as they do when it
    /// turns them on.
    pub(crate) fn restore(&self, session: &str, state: &Value) {
        if state["enabled"].as_bool() == Some(true) {
            self.enabled.borrow_mut().push(session.to_owned());
        }
        if state["lifecycle"].as_bool() == Some(true) {
            self.lifecycle.borrow_mut().push(session.to_owned());
            self.replay_lifecycle(session);
        }
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

    /// The markup of the document the frame `id` has, as it came.
    pub(crate) fn source_of(&self, id: &str) -> Option<Rc<str>> {
        self.frames
            .borrow()
            .iter()
            .find(|frame| frame.id == id)
            .map(|frame| frame.source.clone())
    }

    /// The frames now, parents first: what changed since last told goes
    /// to the sessions that turned `Page` on (frames gone, then new and
    /// navigated ones). The main frame's new document is told before its
    /// realm is (as Chrome tells `Page.frameNavigated` before
    /// `Runtime.executionContextCreated`): its old document's frames go
    /// with it. A child frame's is told as the engine makes it
    /// (`child_document_created`), and found the same here.
    pub(crate) fn update(&self, frames: Vec<FrameInfo>) {
        let old = std::mem::replace(&mut *self.frames.borrow_mut(), frames);
        let new = self.frames.borrow().clone();
        self.lifecycles
            .borrow_mut()
            .retain(|id, _| new.iter().any(|frame| frame.id == *id));
        self.attached
            .borrow_mut()
            .retain(|(id, _)| !new.iter().any(|frame| frame.id == *id));
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
                // Told attached as it was made (`frame_attached`).
                None => {
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

    /// Child frame `id`'s initial `about:blank`, document `document`, is
    /// navigated from (its first navigation is asked for): its `init`
    /// now, before the browser tells of the navigation (Chrome tells the
    /// initial empty document's as the frame is made), unless it was told;
    /// `update` doesn't tell it again. Its loader is named by its number,
    /// as no navigation brought it.
    pub(crate) fn initial_document_navigated_from(&self, id: &str, document: u64) {
        let told = self
            .lifecycles
            .borrow()
            .get(id)
            .is_some_and(|lifecycle| lifecycle.document == document);
        if !told {
            let loader = document.to_string();
            self.lifecycle_event(id, document, &loader, "init", TimeTicks::now());
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
        // Told as its navigation was asked for, if it was
        // (`initial_document_navigated_from`).
        let told = self
            .lifecycles
            .borrow()
            .get(&frame.id)
            .is_some_and(|lifecycle| lifecycle.document == frame.document);
        if !told {
            event("init");
        }
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
/// The registrable domain of `url`'s host, private suffixes included, as
/// Blink's `InspectorPageAgent` tells a frame's: empty for an address, a
/// public suffix itself, or a URL with no host.
fn domain_and_registry(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return String::new();
    };
    let Some(url::Host::Domain(name)) = parsed.host() else {
        return String::new();
    };
    site::registrable_domain(&url::Host::Domain(name))
        .unwrap_or_default()
        .to_owned()
}

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

    /// A frame's `domainAndRegistry` is its URL's host's registrable
    /// domain, as Chrome tells it: empty for an address, a public suffix
    /// itself, a single label or no host.
    #[test]
    fn a_frames_domain_and_registry_is_its_registrable_domain() {
        for (url, expected) in [
            ("https://www.example.co.uk/a#b", "example.co.uk"),
            ("https://x.alice.github.io/", "alice.github.io"),
            ("http://www1.web-platform.test:8000/", "web-platform.test"),
            ("http://127.0.0.1:8000/", ""),
            ("http://[::1]/", ""),
            ("http://localhost:3000/", ""),
            ("https://github.io/", ""),
            ("about:blank", ""),
            ("file:///tmp/a.html", ""),
            ("not a URL", ""),
        ] {
            assert_eq!(domain_and_registry(url), expected, "{url}");
        }
    }

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

    /// What a session has on goes to another renderer's agent as it was:
    /// `Page` and its lifecycle events.
    #[test]
    fn a_sessions_page_goes_on_in_another_agent() {
        let (to_browser, _from) = crossbeam_channel::unbounded();
        let to_browser: BrowserSender = to_browser.into();
        let network = || {
            Rc::new(NetworkAgent::new(
                to_browser.clone(),
                0,
                Rc::new(super::super::log_agent::LogAgent::new(to_browser.clone())),
            ))
        };
        let agent = PageAgent::new(to_browser.clone(), network());
        assert_eq!(agent.state("a"), None);
        let command = |method: &str, params: Value| {
            json!({ "id": 1, "method": method, "params": params }).to_string()
        };
        assert!(agent.handle("a", &command("Page.enable", json!({}))));
        assert!(agent.handle(
            "a",
            &command("Page.setLifecycleEventsEnabled", json!({ "enabled": true }))
        ));
        let state = agent.state("a").expect("on");
        let next = PageAgent::new(to_browser.clone(), network());
        next.restore("a", &state);
        assert!(next.any_enabled());
        assert_eq!(next.state("a"), Some(state));
        assert!(agent.handle("a", &command("Page.disable", json!({}))));
        assert_eq!(agent.state("a"), None);
    }
}
