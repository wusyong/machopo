//! DevTools' `DOM` domain as the renderer answers it (Blink's
//! `InspectorDOMAgent`): the top-level frame's document and the frames'
//! below it, as `DOM.Node`s, pushed to each session as it asks
//! (`getDocument`, `requestChildNodes`) and kept up to date with the DOM's
//! changes.
//!
//! Each session numbers the nodes it was sent (`nodeId`, Blink's `Bind`),
//! from 1, never twice. A node in a document's tree lives with it; one
//! sent that isn't in one (a node script holds, asked for by its object)
//! is kept by the session, as Blink's `Member<Node>` keeps it, until its
//! numbers go: at `getDocument`, a new document, `disable`. A node taken
//! out of its tree loses its number, and those below it theirs, as Blink's
//! `Unbind`. `backendNodeId` is the DOM's own id of the node (Blink's
//! `DOMNodeIds`): weak, the renderer's, the same for every session.
//!
//! The DOM notes its trees' changes while a session has `DOM` on
//! (`Dom::set_dom_changes_wanted`); the agent takes them after each task,
//! before it answers a command, and as script pauses, and tells each
//! session of those it has the nodes of, as Blink's agent tells them as
//! they happen: a node is sent as it is when the agent takes the change.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crossbeam_channel::Sender;
use dom::{Cx, Dom, DomChange, NodeId, NodeIdHashMap, NodeKind, NodeRef, NodeRoot, QuirksMode};
use dom_api::DevToolsPage;

use crate::frame_view::Frame;
use protocol::renderer::RendererHostMsg;
use serde_json::{Value, json};

/// DevTools' `DOM.NodeId`.
pub(crate) type Id = i64;

/// The longest text a node's `nodeValue` is sent with (Blink's
/// `kMaxTextSize`), what is past it cut and an ellipsis put in its place.
const MAX_TEXT_SIZE: usize = 10_000;

/// A frame as the `DOM` agent knows it: what DevTools calls it, its
/// document, and its iframe (none for the top-level one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DomFrame {
    pub(crate) id: String,
    pub(crate) document: NodeId,
    pub(crate) owner: Option<NodeId>,
}

/// A session's `DOM`: whether it is on, and the nodes it was sent.
#[derive(Default)]
struct Session {
    enabled: bool,
    /// `DOM.enable`'s `includeWhitespace` was `all`: text of white space
    /// alone is sent too.
    include_whitespace: bool,
    /// The last number given.
    last_id: Id,
    ids: NodeIdHashMap<Id>,
    nodes: HashMap<Id, NodeId>,
    /// The nodes whose children were sent (Blink's `children_requested_`).
    children_sent: HashSet<Id>,
    /// How many children each node sent without them has, as the session
    /// was told (Blink's `cached_child_count_`).
    child_counts: HashMap<Id, u32>,
    /// The roots of the subtrees sent that aren't in a document's tree,
    /// kept while their numbers are (Blink's dangling maps).
    kept: Vec<NodeRoot>,
    /// `performSearch`'s results, by search, until discarded or the numbers
    /// go; the last search's number.
    searches: HashMap<String, Vec<NodeId>>,
    last_search: u64,
}

impl Session {
    fn id(&self, node: NodeId) -> Option<Id> {
        self.ids.get(&node).copied()
    }

    /// The number of `node`, given now if it has none.
    fn bind(&mut self, node: NodeId) -> Id {
        if let Some(id) = self.id(node) {
            return id;
        }
        // Sessions number fewer than 2^63 nodes.
        self.last_id = self.last_id.saturating_add(1);
        self.ids.insert(node, self.last_id);
        self.nodes.insert(self.last_id, node);
        self.last_id
    }

    /// Every number goes (Blink's `DiscardFrontendBindings`).
    fn discard(&mut self) {
        self.ids.clear();
        self.nodes.clear();
        self.children_sent.clear();
        self.child_counts.clear();
        self.kept.clear();
        self.searches.clear();
    }
}

/// Blink's `InspectorDOMAgent`, for every session of the page.
pub(crate) struct DomAgent {
    to_browser: Sender<RendererHostMsg>,
    sessions: RefCell<HashMap<String, Session>>,
    /// The top-level frame's document (Blink's `document_`).
    document: Cell<Option<NodeId>>,
    /// The frames, the top-level one first (`set_frames`).
    frames: RefCell<Vec<DomFrame>>,
}

impl DomAgent {
    pub(crate) fn new(to_browser: Sender<RendererHostMsg>) -> Self {
        Self {
            to_browser,
            sessions: RefCell::new(HashMap::new()),
            document: Cell::new(None),
            frames: RefCell::new(Vec::new()),
        }
    }

    fn send(&self, session: &str, message: &Value) {
        let _ = self.to_browser.send(RendererHostMsg::DevToolsMessage {
            session: session.to_owned(),
            message: message.to_string(),
        });
    }

    fn tell(&self, session: &str, method: &str, params: Value) {
        self.send(session, &json!({ "method": method, "params": params }));
    }

    /// Whether `session` has `DOM` on.
    pub(crate) fn enabled(&self, session: &str) -> bool {
        self.sessions
            .borrow()
            .get(session)
            .is_some_and(|session| session.enabled)
    }

    /// `node`'s number in `session`, the nodes above it it hasn't sent
    /// first (`setChildNodes`, Blink's `PushNodePathToFrontend`); none if
    /// the session hasn't asked for the document.
    pub(crate) fn push_node(&self, name: &str, node: NodeId, dom: &Dom) -> Option<Id> {
        let mut sessions = self.sessions.borrow_mut();
        let session = sessions.get_mut(name).filter(|session| session.enabled)?;
        let frames = self.frames.borrow();
        let document = self.document.get()?;
        let mut events = Vec::new();
        let id = dom.read(|cx| {
            let node = cx.node_by_id(node)?;
            let document = cx.node_by_id(document)?;
            let mut tree = Tree {
                cx,
                session,
                frames: &frames,
                layout: None,
            };
            Some(tree.push_path(node, document, &mut events))
        })?;
        for (method, params) in events {
            self.tell(name, method, params);
        }
        (id != 0).then_some(id)
    }

    /// Whether a session has `DOM` on: the DOM notes its changes while
    /// one has.
    pub(crate) fn any_enabled(&self) -> bool {
        self.sessions
            .borrow()
            .values()
            .any(|session| session.enabled)
    }

    /// `session` has gone: what it was sent goes with it.
    pub(crate) fn detach(&self, session: &str) {
        self.sessions.borrow_mut().remove(session);
    }

    /// The DOM is going (its engine with it): every node a session kept
    /// goes before its heap, and every number.
    pub(crate) fn dom_gone(&self) {
        for session in self.sessions.borrow_mut().values_mut() {
            session.discard();
        }
        self.document.set(None);
        self.frames.borrow_mut().clear();
    }

    /// The top-level frame committed `document`, still to be parsed: the
    /// numbers go, and the sessions hear of it once it is parsed (Blink's
    /// `SetDocument`, which tells of a document only once it isn't
    /// parsing).
    pub(crate) fn document_committed(&self, document: NodeId) {
        self.document.set(Some(document));
        for session in self.sessions.borrow_mut().values_mut() {
            session.discard();
        }
    }

    /// The top-level document's `DOMContentLoaded`: the numbers go, and
    /// each session with `DOM` on is told to ask for the document again
    /// (Blink's `DomContentLoadedEventFired`).
    pub(crate) fn dom_content_loaded(&self) {
        let mut sessions = self.sessions.borrow_mut();
        for (name, session) in sessions.iter_mut() {
            session.discard();
            if session.enabled {
                self.tell(name, "DOM.documentUpdated", json!({}));
            }
        }
    }

    /// The frames now, the top-level one first. A child frame whose iframe
    /// has another document than before is told as its iframe taken out
    /// and put back, with its new document (Blink's
    /// `InvalidateFrameOwnerElement`).
    /// The frames as last told (`set_frames`), the top-level one first.
    pub(crate) fn frames(&self) -> Vec<DomFrame> {
        self.frames.borrow().clone()
    }

    pub(crate) fn set_frames(&self, dom: &Dom, frames: Vec<DomFrame>) {
        let changed: Vec<NodeId> = {
            let known = self.frames.borrow();
            frames
                .iter()
                .filter_map(|frame| {
                    let owner = frame.owner?;
                    let before = known.iter().find(|known| known.owner == Some(owner));
                    before
                        .is_some_and(|before| before.document != frame.document)
                        .then_some(owner)
                })
                .collect()
        };
        *self.frames.borrow_mut() = frames;
        if changed.is_empty() {
            return;
        }
        let frames = self.frames.borrow();
        let mut sessions = self.sessions.borrow_mut();
        for (name, session) in sessions.iter_mut() {
            if !session.enabled {
                continue;
            }
            for &owner in &changed {
                dom.read(|cx| {
                    let Some(owner) = cx.node_by_id(owner) else {
                        return;
                    };
                    let mut tree = Tree {
                        cx,
                        session,
                        frames: &frames,
                        layout: None,
                    };
                    for (method, params) in tree.frame_owner_changed(owner) {
                        self.tell(name, method, params);
                    }
                });
            }
        }
    }

    /// Tells each session with `DOM` on of the DOM's changes since they
    /// were last taken.
    pub(crate) fn take_changes(&self, dom: &Dom) {
        let changes = dom.take_dom_changes();
        if changes.is_empty() {
            return;
        }
        let frames = self.frames.borrow();
        let mut sessions = self.sessions.borrow_mut();
        for (name, session) in sessions.iter_mut() {
            if !session.enabled {
                continue;
            }
            let events = dom.read(|cx| {
                let mut tree = Tree {
                    cx,
                    session,
                    frames: &frames,
                    layout: None,
                };
                tree.changed(&changes)
            });
            for (method, params) in events {
                self.tell(name, method, params);
            }
        }
    }

    /// Answers `message`, a command of `session`'s, if it is one of the
    /// agent's: whether it was. The DOM's changes so far are told first,
    /// so what it answers and what it told agree.
    pub(crate) fn handle(&self, session: &str, message: &str, page: &mut dyn DevToolsPage) -> bool {
        let Ok(command) = serde_json::from_str::<Value>(message) else {
            return false;
        };
        let Some(method) = command.get("method").and_then(Value::as_str) else {
            return false;
        };
        if !COMMANDS.contains(&method) {
            return false;
        }
        self.take_changes(page.dom());
        let id = command.get("id").cloned().unwrap_or(Value::Null);
        let params = command.get("params").cloned().unwrap_or_else(|| json!({}));
        let answer = self.answer(session, method, params, page);
        let message = match answer {
            Ok(result) => json!({ "id": id, "result": result }),
            Err(error) => json!({ "id": id, "error": { "code": -32000, "message": error } }),
        };
        self.send(session, &message);
        page.dom().set_dom_changes_wanted(self.any_enabled());
        true
    }

    fn answer(
        &self,
        name: &str,
        method: &str,
        mut params: Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<Value, String> {
        {
            let mut sessions = self.sessions.borrow_mut();
            let session = sessions.entry(name.to_owned()).or_default();
            match method {
                "DOM.enable" => {
                    if !session.enabled {
                        session.enabled = true;
                        session.discard();
                        session.include_whitespace =
                            params["includeWhitespace"].as_str() == Some("all");
                    }
                    return Ok(json!({}));
                }
                "DOM.disable" => {
                    if !session.enabled {
                        return Err("DOM agent hasn't been enabled".to_owned());
                    }
                    session.enabled = false;
                    session.include_whitespace = false;
                    session.discard();
                    return Ok(json!({}));
                }
                _ => {}
            }
        }
        // What the engine's inspector answers, with no borrow of the DOM
        // or the sessions held: script may run (a getter, a handler
        // compiled).
        match method {
            "DOM.resolveNode" => {
                let node = self.node_to_resolve(name, &params, page.dom())?;
                let group = params["objectGroup"].as_str().unwrap_or_default();
                let object = page
                    .objects()
                    .resolve_node(name, node, group)
                    .and_then(|object| serde_json::from_str::<Value>(&object).ok())
                    .ok_or("Node with given id does not belong to the document")?;
                return Ok(json!({ "object": object }));
            }
            "DOM.setInspectedNode" => {
                let id = params["nodeId"].as_i64().unwrap_or(0);
                let node = self
                    .sessions
                    .borrow()
                    .get(name)
                    .and_then(|session| session.nodes.get(&id).copied())
                    .filter(|node| page.dom().read(|cx| cx.node_by_id(*node).is_some()))
                    .ok_or("Could not find node with given id")?;
                page.objects().inspect_node(name, node);
                return Ok(json!({}));
            }
            "DOMDebugger.getEventListeners" => {
                let object = params["objectId"].as_str().unwrap_or_default();
                let depth = params["depth"].as_i64().unwrap_or(1);
                let pierce = params["pierce"].as_bool().unwrap_or(false);
                let include_whitespace = self
                    .sessions
                    .borrow()
                    .get(name)
                    .is_some_and(|session| session.include_whitespace);
                let listeners = page.objects().event_listeners(
                    name,
                    object,
                    depth,
                    pierce,
                    include_whitespace,
                )?;
                let listeners: Value =
                    serde_json::from_str(&listeners).unwrap_or_else(|_| json!([]));
                return Ok(json!({ "listeners": listeners }));
            }
            "DOM.getBoxModel" => {
                let node = self.node_param(name, &params, page)?;
                return box_model(page, node)
                    .ok_or_else(|| "Could not compute box model.".to_owned());
            }
            "DOM.getContentQuads" => {
                let node = self.node_param(name, &params, page)?;
                return content_quads(page, node)
                    .ok_or_else(|| "Could not compute content quads.".to_owned());
            }
            "DOM.getNodeForLocation" => {
                let point = gfx::PointF::new(
                    base::numerics::f32_from_f64(params["x"].as_f64().unwrap_or(0.0)),
                    base::numerics::f32_from_f64(params["y"].as_f64().unwrap_or(0.0)),
                );
                let user_agent = params["includeUserAgentShadowDOM"]
                    .as_bool()
                    .unwrap_or(false);
                let node = node_at(page, point, true, user_agent)
                    .ok_or("No node found at given location")?;
                params["backendNodeId"] = node.get().into();
            }
            "DOM.requestNode" => {
                let object = params["objectId"].as_str().unwrap_or_default();
                let node = page.objects().node_of_object(name, object)?;
                params["backendNodeId"] = node.get().into();
            }
            "DOM.describeNode" | "DOM.getOuterHTML"
                if params.get("nodeId").is_none()
                    && params.get("backendNodeId").is_none()
                    && let Some(object) = params["objectId"].as_str() =>
            {
                let node = page.objects().node_of_object(name, object)?;
                params["backendNodeId"] = node.get().into();
            }
            _ => {}
        }
        let mut sessions = self.sessions.borrow_mut();
        let session = sessions.entry(name.to_owned()).or_default();
        let frames = self.frames.borrow();
        let document = self.document.get();
        let mut events = Vec::new();
        let (dom, layout) = page.dom_and_layout();
        let layout = layout
            .and_then(|layout| layout.as_any_mut().downcast_mut::<Frame>())
            .map(|frame| &*frame);
        let result = dom.read(|cx| {
            let mut tree = Tree {
                cx,
                session,
                frames: &frames,
                layout,
            };
            let document = document.and_then(|document| cx.node_by_id(document));
            let method = method
                .strip_prefix("DOM.")
                .expect("the inspector's commands answered above");
            tree.command(method, &params, document, &mut events)
        });
        // Before the answer, as Blink's agent sends them while answering
        // (`setChildNodes` above all: the front end reads the children
        // once `requestChildNodes` is answered).
        for (method, params) in events {
            self.tell(name, method, params);
        }
        result
    }

    /// The node a command names by its `nodeId`, `backendNodeId` or
    /// `objectId` (Blink's `AssertNode`).
    pub(crate) fn node_param(
        &self,
        name: &str,
        params: &Value,
        page: &mut dyn DevToolsPage,
    ) -> Result<NodeId, String> {
        let node = if let Some(id) = params["nodeId"].as_i64() {
            self.sessions
                .borrow()
                .get(name)
                .and_then(|session| session.nodes.get(&id).copied())
                .ok_or("Could not find node with given id")?
        } else if let Some(backend) = params["backendNodeId"].as_u64() {
            u32::try_from(backend)
                .ok()
                .map(NodeId::from_raw)
                .ok_or("No node found for given backend id")?
        } else if let Some(object) = params["objectId"].as_str() {
            page.objects().node_of_object(name, object)?
        } else {
            return Err("Either nodeId, backendNodeId or objectId must be specified".to_owned());
        };
        if page.dom().read(|cx| cx.node_by_id(node).is_none()) {
            return Err(if params["nodeId"].is_i64() {
                "Could not find node with given id".to_owned()
            } else {
                "No node found for given backend id".to_owned()
            });
        }
        Ok(node)
    }

    /// The node `resolveNode` names, by its number or its backend id
    /// (Blink's `resolveNode`).
    fn node_to_resolve(&self, name: &str, params: &Value, dom: &Dom) -> Result<NodeId, String> {
        let node = match (params["nodeId"].as_i64(), params["backendNodeId"].as_u64()) {
            (Some(id), None) => self
                .sessions
                .borrow()
                .get(name)
                .and_then(|session| session.nodes.get(&id).copied()),
            (None, Some(backend)) => u32::try_from(backend).ok().map(NodeId::from_raw),
            _ => return Err("Either nodeId or backendNodeId must be specified.".to_owned()),
        };
        node.filter(|node| dom.read(|cx| cx.node_by_id(*node).is_some()))
            .ok_or_else(|| "No node with given id found".to_owned())
    }
}

/// The commands the agent answers: `DOM`'s and the one of `DOMDebugger`
/// the Elements panel's event listeners ask (Blink's DOM debugger agent's);
/// another of theirs is the inspector's (V8 has none), which answers it
/// isn't there.
const COMMANDS: &[&str] = &[
    "DOM.enable",
    "DOM.disable",
    "DOM.getDocument",
    "DOM.requestChildNodes",
    "DOM.describeNode",
    "DOM.pushNodesByBackendIdsToFrontend",
    "DOM.pushNodeByPathToFrontend",
    "DOM.querySelector",
    "DOM.querySelectorAll",
    "DOM.getAttributes",
    "DOM.getOuterHTML",
    "DOM.getFrameOwner",
    "DOM.getTopLayerElements",
    "DOM.collectClassNamesFromSubtree",
    "DOM.resolveNode",
    "DOM.requestNode",
    "DOM.setInspectedNode",
    "DOM.performSearch",
    "DOM.getSearchResults",
    "DOM.discardSearchResults",
    "DOM.getBoxModel",
    "DOM.getContentQuads",
    "DOM.getNodeForLocation",
    "DOMDebugger.getEventListeners",
];

/// What a command or a change sends before its answer, or as it is told:
/// methods and their params.
type Events = Vec<(&'static str, Value)>;

/// A session's nodes over the DOM, for a command or the changes.
struct Tree<'a, 'cx, 'gc> {
    cx: &'a Cx<'cx, 'gc>,
    session: &'a mut Session,
    frames: &'a [DomFrame],
    /// The layout as last laid out, if lent: what a node's `isScrollable`
    /// reads (none for the changes' events: Blink's reads its layout
    /// object as it is then too).
    layout: Option<&'a Frame>,
}

impl<'gc> Tree<'_, '_, 'gc> {
    fn command(
        &mut self,
        method: &str,
        params: &Value,
        document: Option<NodeRef<'gc>>,
        events: &mut Events,
    ) -> Result<Value, String> {
        match method {
            "getDocument" => {
                // Asking for it turns `DOM` on, as Blink's agent does.
                self.session.enabled = true;
                let document = document.ok_or("Document is not available")?;
                self.session.discard();
                let depth = depth(params, 2);
                let pierce = params["pierce"].as_bool().unwrap_or(false);
                let root = self.build(document, depth, pierce, true);
                Ok(json!({ "root": root }))
            }
            "requestChildNodes" => {
                let depth = match params["depth"].as_i64().unwrap_or(1) {
                    -1 => i64::MAX,
                    depth if depth > 0 => depth,
                    _ => {
                        return Err(
                            "Please provide a positive integer as a depth or -1 for entire subtree"
                                .to_owned(),
                        );
                    }
                };
                let pierce = params["pierce"].as_bool().unwrap_or(false);
                if let Some(id) = params["nodeId"].as_i64() {
                    self.push_children(id, depth, pierce, events);
                }
                Ok(json!({}))
            }
            "describeNode" => {
                let node = self.node_of(params)?;
                let depth = depth(params, 0);
                let pierce = params["pierce"].as_bool().unwrap_or(false);
                Ok(json!({ "node": self.build(node, depth, pierce, false) }))
            }
            "pushNodesByBackendIdsToFrontend" => {
                let document = document
                    .filter(|document| self.bound(*document).is_some())
                    .ok_or("Document needs to be requested first")?;
                let ids: Vec<Value> = params["backendNodeIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|backend| {
                        let node = backend
                            .as_u64()
                            .and_then(|backend| u32::try_from(backend).ok())
                            .and_then(|backend| self.cx.node_by_id(NodeId::from_raw(backend)))
                            .filter(|node| self.in_frames(*node));
                        node.map_or(0, |node| self.push_path(node, document, events))
                            .into()
                    })
                    .collect();
                Ok(json!({ "nodeIds": ids }))
            }
            "pushNodeByPathToFrontend" => {
                if !self.session.enabled {
                    return Err("DOM agent is not enabled".to_owned());
                }
                let document = document.ok_or("No node with given path found")?;
                let path = params["path"].as_str().unwrap_or_default();
                let node = self
                    .node_for_path(document, path)
                    .ok_or("No node with given path found")?;
                Ok(json!({ "nodeId": self.push_path(node, document, events) }))
            }
            "querySelector" | "querySelectorAll" => {
                let id = params["nodeId"].as_i64().unwrap_or(0);
                let node = self.assert_node(id)?;
                if !matches!(
                    self.cx.kind(node),
                    NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
                ) {
                    return Err("Not a container node".to_owned());
                }
                let selectors = params["selector"].as_str().unwrap_or_default();
                let query =
                    style::SelectorQuery::parse(selectors).ok_or("DOM Error while querying")?;
                let document = document.ok_or("DOM Error while querying")?;
                if method == "querySelector" {
                    let found = query
                        .query_first(self.cx, node)
                        .map_or(0, |found| self.push_path(found, document, events));
                    Ok(json!({ "nodeId": found }))
                } else {
                    let found: Vec<Value> = query
                        .query_all(self.cx, node)
                        .into_iter()
                        .map(|found| self.push_path(found, document, events).into())
                        .collect();
                    Ok(json!({ "nodeIds": found }))
                }
            }
            "getAttributes" => {
                let id = params["nodeId"].as_i64().unwrap_or(0);
                let node = self.assert_node(id)?;
                if self.cx.kind(node) != NodeKind::Element {
                    return Err("Node is not an Element".to_owned());
                }
                Ok(json!({ "attributes": self.attributes(node) }))
            }
            "getOuterHTML" => {
                let node = self.node_of(params)?;
                let html = match self.cx.kind(node) {
                    NodeKind::Document | NodeKind::DocumentFragment => {
                        html::parser::serialize_children(self.cx, node, true)
                    }
                    _ => html::parser::serialize_node(self.cx, node, true),
                };
                Ok(json!({ "outerHTML": html }))
            }
            "getFrameOwner" => {
                let frame = params["frameId"].as_str().unwrap_or_default();
                let found = self
                    .frames
                    .iter()
                    .find(|known| known.id == frame)
                    .ok_or("Frame with the given id was not found.")?;
                let owner = found
                    .owner
                    .and_then(|owner| self.cx.node_by_id(owner))
                    .ok_or("Frame with the given id does not belong to the target.")?;
                let mut result = json!({ "backendNodeId": self.cx.node_id(owner).get() });
                if self.session.enabled
                    && let Some(document) =
                        document.filter(|document| self.bound(*document).is_some())
                {
                    result["nodeId"] = self.push_path(owner, document, events).into();
                }
                Ok(result)
            }
            "getNodeForLocation" => {
                let node = params["backendNodeId"]
                    .as_u64()
                    .and_then(|backend| u32::try_from(backend).ok())
                    .and_then(|backend| self.cx.node_by_id(NodeId::from_raw(backend)))
                    .ok_or("No node found at given location")?;
                let mut result = json!({ "backendNodeId": self.cx.node_id(node).get() });
                if let Some(frame) = self.frame_of_document(self.cx.document_of(node)) {
                    result["frameId"] = frame.into();
                }
                if self.session.enabled
                    && let Some(document) =
                        document.filter(|document| self.bound(*document).is_some())
                {
                    result["nodeId"] = self.push_path(node, document, events).into();
                }
                Ok(result)
            }
            "requestNode" => {
                let node = params["backendNodeId"]
                    .as_u64()
                    .and_then(|backend| u32::try_from(backend).ok())
                    .and_then(|backend| self.cx.node_by_id(NodeId::from_raw(backend)))
                    .ok_or("Object id doesn't reference a Node")?;
                let document = document.ok_or("Document is not available")?;
                Ok(json!({ "nodeId": self.push_path(node, document, events) }))
            }
            "performSearch" => {
                if !self.session.enabled {
                    return Err("DOM agent is not enabled".to_owned());
                }
                let query = params["query"].as_str().unwrap_or_default();
                let user_agent = params["includeUserAgentShadowDOM"]
                    .as_bool()
                    .unwrap_or(false);
                let found = self.search(query, user_agent);
                self.session.last_search = self.session.last_search.saturating_add(1);
                let id = self.session.last_search.to_string();
                let count = found.len();
                self.session.searches.insert(id.clone(), found);
                Ok(json!({ "searchId": id, "resultCount": count }))
            }
            "getSearchResults" => {
                let id = params["searchId"].as_str().unwrap_or_default();
                let found = self
                    .session
                    .searches
                    .get(id)
                    .cloned()
                    .ok_or("No search session with given id found")?;
                let from = params["fromIndex"].as_i64().unwrap_or(0);
                let to = params["toIndex"].as_i64().unwrap_or(0);
                let (Ok(from), Ok(to)) = (usize::try_from(from), usize::try_from(to)) else {
                    return Err("Invalid search result range".to_owned());
                };
                if to > found.len() || from >= to {
                    return Err("Invalid search result range".to_owned());
                }
                let document = document.ok_or("Document is not available")?;
                let ids: Vec<Value> = found[from..to]
                    .iter()
                    .map(|node| match self.cx.node_by_id(*node) {
                        Some(node) => self.push_path(node, document, events).into(),
                        None => 0.into(),
                    })
                    .collect();
                Ok(json!({ "nodeIds": ids }))
            }
            "discardSearchResults" => {
                let id = params["searchId"].as_str().unwrap_or_default();
                self.session.searches.remove(id);
                Ok(json!({}))
            }
            "getTopLayerElements" => {
                if document.is_none() {
                    return Err("DOM agent hasn't been enabled".to_owned());
                }
                // machopo has no top layer (no modal dialog, no popover).
                Ok(json!({ "nodeIds": [] }))
            }
            "collectClassNamesFromSubtree" => {
                let id = params["nodeId"].as_i64().unwrap_or(0);
                let node = self
                    .node(id)
                    .filter(|node| {
                        matches!(
                            self.cx.kind(*node),
                            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
                        )
                    })
                    .ok_or("No suitable node with given id found")?;
                Ok(json!({ "classNames": self.class_names(node) }))
            }
            _ => Err(format!("'DOM.{method}' wasn't found")),
        }
    }

    fn bound(&self, node: NodeRef<'gc>) -> Option<Id> {
        self.session.id(self.cx.node_id(node))
    }

    fn node(&self, id: Id) -> Option<NodeRef<'gc>> {
        let node = *self.session.nodes.get(&id)?;
        self.cx.node_by_id(node)
    }

    /// Blink's `AssertNode` of a node id.
    fn assert_node(&self, id: Id) -> Result<NodeRef<'gc>, String> {
        self.node(id)
            .ok_or_else(|| "Could not find node with given id".to_owned())
    }

    /// The node a command names, by its `nodeId` or `backendNodeId`
    /// (Blink's `AssertNode`).
    fn node_of(&self, params: &Value) -> Result<NodeRef<'gc>, String> {
        if let Some(id) = params["nodeId"].as_i64() {
            return self.assert_node(id);
        }
        if let Some(backend) = params["backendNodeId"].as_u64() {
            return u32::try_from(backend)
                .ok()
                .and_then(|backend| self.cx.node_by_id(NodeId::from_raw(backend)))
                .ok_or_else(|| "No node found for given backend id".to_owned());
        }
        Err("Either nodeId, backendNodeId or objectId must be specified".to_owned())
    }

    /// Whether `node` is of a document of the page's frames.
    fn in_frames(&self, node: NodeRef<'gc>) -> bool {
        let document = self.cx.node_id(self.cx.document_of(node));
        self.frames.iter().any(|frame| frame.document == document)
    }

    /// Whether `node` isn't sent: text of white space alone, unless the
    /// session asked for it (Blink's `ShouldSkipNode`).
    fn skipped(&self, node: NodeRef<'gc>) -> bool {
        !self.session.include_whitespace
            && self.cx.kind(node) == NodeKind::Text
            && self
                .cx
                .text_data(node)
                .is_some_and(|data| data.borrow().to_str().trim().is_empty())
    }

    fn first_child(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut child = self.cx.first_child(node);
        while let Some(at) = child
            && self.skipped(at)
        {
            child = self.cx.next_sibling(at);
        }
        child
    }

    fn next_sibling(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut sibling = self.cx.next_sibling(node);
        while let Some(at) = sibling
            && self.skipped(at)
        {
            sibling = self.cx.next_sibling(at);
        }
        sibling
    }

    fn children(&self, node: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let mut children = Vec::new();
        let mut child = self.first_child(node);
        while let Some(at) = child {
            children.push(at);
            child = self.next_sibling(at);
        }
        children
    }

    /// The node above `node` as DevTools has it: a document's iframe, a
    /// shadow root's host, else its parent (Blink's `InnerParentNode`).
    fn parent(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        match self.cx.kind(node) {
            NodeKind::Document => self.cx.frame_owner(node),
            _ => self.cx.parent(node).or_else(|| self.cx.shadow_host(node)),
        }
    }

    fn is_container(&self, node: NodeRef<'gc>) -> bool {
        matches!(
            self.cx.kind(node),
            NodeKind::Document | NodeKind::DocumentFragment | NodeKind::Element
        )
    }

    /// `node` as a `DOM.Node` (Blink's `BuildObjectForNode`), with its
    /// children `depth` levels down; numbered, and those it is sent with,
    /// if `bind`, else with its number if it has one (`describeNode`).
    fn build(&mut self, node: NodeRef<'gc>, depth: i64, pierce: bool, bind: bool) -> Value {
        let cx = self.cx;
        let backend = cx.node_id(node);
        let id = if bind {
            self.session.bind(backend)
        } else {
            self.session.id(backend).unwrap_or(0)
        };
        let kind = cx.kind(node);
        let node_value = match kind {
            NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction => cx
                .data(node)
                .map(|data| truncated(&data.to_str()))
                .unwrap_or_default(),
            _ => String::new(),
        };
        let local_name = match kind {
            NodeKind::Element => cx
                .element_name(node)
                .map(|name| dom::names::string(name.local).to_str().into_owned())
                .unwrap_or_default(),
            _ => String::new(),
        };
        let mut value = json!({
            "nodeId": id,
            "backendNodeId": backend.get(),
            "nodeType": cx.node_type(node),
            "nodeName": cx.node_name(node).to_str(),
            "localName": local_name,
            "nodeValue": node_value,
        });
        let mut depth = depth;
        let mut force_children = false;
        match kind {
            NodeKind::Element => {
                let name = cx.element_name(node).expect("an element's name");
                if name.namespace == dom::names::SVG_NAMESPACE {
                    value["isSVG"] = true.into();
                }
                value["attributes"] = self.attributes(node).into();
                if let Some(frame) = self.frame_of_owner(node) {
                    value["frameId"] = frame.into();
                }
                if let Some(document) = cx.content_document(node) {
                    let depth = if pierce { depth } else { 0 };
                    value["contentDocument"] = self.build(document, depth, pierce, bind);
                }
                if cx
                    .parent(node)
                    .is_some_and(|parent| cx.kind(parent) == NodeKind::Document)
                    && let Some(frame) = self.frame_of_document(cx.document_of(node))
                {
                    value["frameId"] = frame.into();
                }
                if let Some(root) = cx.shadow_root(node) {
                    let depth = if pierce { depth } else { 0 };
                    value["shadowRoots"] = json!([self.build(root, depth, pierce, bind)]);
                    force_children = true;
                }
                if name.namespace == dom::names::HTML_NAMESPACE
                    && name.local == dom::names::local::LINK
                {
                    force_children = true;
                }
                if let Some(content) = cx.template_content(node) {
                    value["templateContent"] = self.build(content, 0, pierce, bind);
                    force_children = true;
                }
                if let Some(slot) = cx.assigned_slot(node) {
                    value["assignedSlot"] = json!({
                        "nodeType": cx.node_type(slot),
                        "nodeName": cx.node_name(slot).to_str(),
                        "backendNodeId": cx.node_id(slot).get(),
                    });
                }
            }
            NodeKind::Document => {
                value["documentURL"] = cx
                    .document_url(node)
                    .map_or_else(String::new, |url| url.to_str().into_owned())
                    .into();
                value["baseURL"] = cx
                    .document_base_url(node)
                    .map_or_else(String::new, |url| url.to_string())
                    .into();
                value["xmlVersion"] = "".into();
                value["compatibilityMode"] = match cx.quirks_mode(node) {
                    QuirksMode::NoQuirks => "NoQuirksMode",
                    QuirksMode::LimitedQuirks => "LimitedQuirksMode",
                    QuirksMode::Quirks => "QuirksMode",
                }
                .into();
            }
            NodeKind::DocumentType => {
                if let Some(doctype) = cx.doctype(node) {
                    value["publicId"] = doctype.public_id.to_str().into();
                    value["systemId"] = doctype.system_id.to_str().into();
                }
            }
            NodeKind::DocumentFragment => {
                if let Some(init) = cx.shadow_root_init(node) {
                    value["shadowRootType"] = match init.mode {
                        dom::ShadowRootMode::Open => "open",
                        dom::ShadowRootMode::Closed => "closed",
                        dom::ShadowRootMode::UserAgent => "user-agent",
                    }
                    .into();
                }
            }
            _ => {}
        }
        if self.is_container(node) {
            let count = u32::try_from(self.children(node).len()).unwrap_or(u32::MAX);
            value["childNodeCount"] = count.into();
            if bind {
                self.session.child_counts.insert(id, count);
            }
            if bind && force_children && depth == 0 {
                depth = 1;
            }
            let children = self.build_children(node, depth, pierce, bind);
            if !children.is_empty() || depth != 0 {
                value["children"] = children.into();
            }
        }
        if kind == NodeKind::Element
            && let Some(layout) = self.layout
            && layout.is_user_scrollable(cx.node_id(cx.document_of(node)), backend)
        {
            value["isScrollable"] = true.into();
        }
        value
    }

    /// `container`'s children as `DOM.Node`s, `depth` levels down (Blink's
    /// `BuildArrayForContainerChildren`): at 0 none, but a lone text child,
    /// as if they had been asked for.
    fn build_children(
        &mut self,
        container: NodeRef<'gc>,
        depth: i64,
        pierce: bool,
        bind: bool,
    ) -> Vec<Value> {
        let cx = self.cx;
        if depth == 0 {
            if !bind {
                return Vec::new();
            }
            let Some(child) = cx.first_child(container).filter(|child| {
                cx.kind(*child) == NodeKind::Text && cx.next_sibling(*child).is_none()
            }) else {
                return Vec::new();
            };
            let parent = self.session.bind(cx.node_id(container));
            let mut built = self.build(child, 0, pierce, bind);
            built["parentId"] = parent.into();
            self.session.children_sent.insert(parent);
            return vec![built];
        }
        let parent = bind.then(|| self.session.bind(cx.node_id(container)));
        if let Some(parent) = parent {
            self.session.children_sent.insert(parent);
        }
        let children = self.children(container);
        children
            .into_iter()
            .map(|child| {
                let mut built = self.build(child, depth.saturating_sub(1), pierce, bind);
                built["parentId"] = parent
                    .unwrap_or_else(|| self.session.id(cx.node_id(container)).unwrap_or(0))
                    .into();
                built
            })
            .collect()
    }

    fn attributes(&self, element: NodeRef<'gc>) -> Vec<Value> {
        let cx = self.cx;
        let mut attributes = Vec::new();
        for index in 0..cx.attribute_count(element) {
            if let Some(attribute) = cx.attribute_at(element, index) {
                attributes.push(qualified(attribute.name).into());
                attributes.push(attribute.value.to_str().into());
            }
        }
        attributes
    }

    fn frame_of_owner(&self, owner: NodeRef<'gc>) -> Option<String> {
        let owner = self.cx.node_id(owner);
        self.frames
            .iter()
            .find(|frame| frame.owner == Some(owner))
            .map(|frame| frame.id.clone())
    }

    fn frame_of_document(&self, document: NodeRef<'gc>) -> Option<String> {
        let document = self.cx.node_id(document);
        self.frames
            .iter()
            .find(|frame| frame.document == document)
            .map(|frame| frame.id.clone())
    }

    /// Sends the children of the node numbered `id`, `depth` levels down,
    /// as `setChildNodes`; of those sent already, theirs below (Blink's
    /// `PushChildNodesToFrontend`).
    fn push_children(&mut self, id: Id, depth: i64, pierce: bool, events: &mut Events) {
        let Some(node) = self.node(id).filter(|node| self.is_container(*node)) else {
            return;
        };
        if self.session.children_sent.contains(&id) {
            if depth <= 1 {
                return;
            }
            for child in self.children(node) {
                if let Some(child) = self.bound(child) {
                    self.push_children(child, depth.saturating_sub(1), pierce, events);
                }
            }
            return;
        }
        let children = self.build_children(node, depth, pierce, true);
        events.push((
            "DOM.setChildNodes",
            json!({ "parentId": id, "nodes": children }),
        ));
    }

    /// `node`'s number, sending the nodes above it the session hasn't yet,
    /// from the document down (Blink's `PushNodePathToFrontend`). A node of
    /// no document's tree is sent with its subtree's root, as a child of
    /// none, and kept while the session has it.
    fn push_path(&mut self, node: NodeRef<'gc>, document: NodeRef<'gc>, events: &mut Events) -> Id {
        if let Some(id) = self.bound(node) {
            return id;
        }
        if self.bound(document).is_none() {
            return 0;
        }
        let mut path = Vec::new();
        let mut at = node;
        let found = loop {
            let Some(parent) = self.parent(at) else {
                break false;
            };
            path.push(parent);
            if self.bound(parent).is_some() {
                break true;
            }
            at = parent;
        };
        if !found {
            // Detached: its subtree's root, as a child of none.
            let root = path.last().copied().unwrap_or(node);
            self.session.kept.push(self.cx.root(root));
            let built = self.build(root, 0, false, true);
            events.push((
                "DOM.setChildNodes",
                json!({ "parentId": 0, "nodes": [built] }),
            ));
        }
        for parent in path.into_iter().rev() {
            if let Some(id) = self.bound(parent) {
                self.push_children(id, 1, false, events);
            }
        }
        self.bound(node).unwrap_or(0)
    }

    /// The node `path` names from `document` (Blink's `NodeForPath`): index
    /// and `nodeName` in turn, the index `a` or `u` for a host's shadow
    /// root, `d` for an iframe's document.
    fn node_for_path(&self, document: NodeRef<'gc>, path: &str) -> Option<NodeRef<'gc>> {
        let tokens: Vec<&str> = path.split(',').filter(|token| !token.is_empty()).collect();
        if tokens.is_empty() {
            return None;
        }
        let mut node = document;
        for [index, name] in tokens.as_chunks::<2>().0 {
            let child = match index.parse::<usize>() {
                Ok(index) => self.children(node).get(index).copied(),
                Err(_) => match *index {
                    "d" => self.cx.content_document(node),
                    "a" | "u" => self.cx.shadow_root(node).filter(|root| {
                        let user_agent = self
                            .cx
                            .shadow_root_init(*root)
                            .is_some_and(|init| init.mode == dom::ShadowRootMode::UserAgent);
                        user_agent == (*index == "u")
                    }),
                    _ => None,
                },
            }?;
            if self.cx.node_name(child).to_str() != *name {
                return None;
            }
            node = child;
        }
        Some(node)
    }

    /// The nodes of the frames' documents `query` finds (Blink's
    /// `performSearch`), each once, in turn: those the query as a selector
    /// matches, in each document and each shadow root of it (a
    /// user-agent one if `user_agent`); then, in tree order through the
    /// shadow roots, text and comments with the query in them, elements
    /// whose name matches it (`<div` its start, `div>` its end, `<div>`
    /// the whole name, as Blink's), and elements with an attribute whose
    /// name has it or whose value has it (`"value"` the whole value); all
    /// ignoring case. machopo has no XPath: Blink's third pass isn't.
    fn search(&self, query: &str, user_agent: bool) -> Vec<NodeId> {
        let cx = self.cx;
        let documents: Vec<NodeRef<'gc>> = self
            .frames
            .iter()
            .filter_map(|frame| cx.node_by_id(frame.document))
            .collect();
        let mut found: Vec<NodeId> = Vec::new();
        let mut seen = HashSet::new();
        let mut add = |node: NodeRef<'gc>, found: &mut Vec<NodeId>| {
            let id = cx.node_id(node);
            if seen.insert(id) {
                found.push(id);
            }
        };
        if let Some(selector) = style::SelectorQuery::parse(query) {
            for &document in &documents {
                for node in selector.query_all(cx, document) {
                    add(node, &mut found);
                }
                let mut roots = Vec::new();
                if let Some(element) = document_element(cx, document) {
                    self.walk(element, user_agent, &mut |node| {
                        if cx.is_shadow_root(node) {
                            roots.push(node);
                        }
                    });
                }
                for root in roots {
                    for node in selector.query_all(cx, root) {
                        add(node, &mut found);
                    }
                }
            }
        }
        let lower = query.to_lowercase();
        let (tag, start, end) = {
            let (start, tag) = match lower.strip_prefix("</").or_else(|| lower.strip_prefix('<')) {
                Some(tag) => (true, tag),
                None => (false, lower.as_str()),
            };
            match tag.strip_suffix('>') {
                Some(tag) => (tag.to_owned(), start, true),
                None => (tag.to_owned(), start, false),
            }
        };
        let name_matches = |name: &str| {
            let name = name.to_lowercase();
            match (start, end) {
                (false, false) => name.contains(&tag),
                (true, true) => name == tag,
                (true, false) => name.starts_with(&tag),
                (false, true) => name.ends_with(&tag),
            }
        };
        let (value, exact) = match lower
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
        {
            Some(value) => (value.to_owned(), true),
            None => (
                lower
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .to_owned(),
                false,
            ),
        };
        let value_matches = |text: &str| {
            let text = text.to_lowercase();
            if exact {
                text == value
            } else {
                text.contains(&value)
            }
        };
        for &document in &documents {
            let Some(element) = document_element(cx, document) else {
                continue;
            };
            let mut matched = Vec::new();
            self.walk(element, user_agent, &mut |node| match cx.kind(node) {
                NodeKind::Text | NodeKind::Comment => {
                    if cx
                        .data(node)
                        .is_some_and(|data| data.to_str().to_lowercase().contains(&lower))
                    {
                        matched.push(node);
                    }
                }
                NodeKind::Element => {
                    if name_matches(&cx.node_name(node).to_str()) {
                        matched.push(node);
                        return;
                    }
                    let by_attribute = (0..cx.attribute_count(node)).any(|index| {
                        cx.attribute_at(node, index).is_some_and(|attribute| {
                            dom::names::string(attribute.name.local)
                                .to_str()
                                .to_lowercase()
                                .contains(&lower)
                                || value_matches(&attribute.value.to_str())
                        })
                    });
                    if by_attribute {
                        matched.push(node);
                    }
                }
                _ => {}
            });
            for node in matched {
                add(node, &mut found);
            }
        }
        found
    }

    /// `node` and what is below it, a host's shadow tree before its
    /// children (Blink's `NextNodeWithShadowDOMInMind`), a user-agent one
    /// only if `user_agent`.
    fn walk(&self, node: NodeRef<'gc>, user_agent: bool, visit: &mut dyn FnMut(NodeRef<'gc>)) {
        let cx = self.cx;
        visit(node);
        if let Some(root) = cx.shadow_root(node)
            && (user_agent
                || cx
                    .shadow_root_init(root)
                    .is_some_and(|init| init.mode != dom::ShadowRootMode::UserAgent))
        {
            self.walk(root, user_agent, visit);
        }
        let mut child = cx.first_child(node);
        while let Some(at) = child {
            self.walk(at, user_agent, visit);
            child = cx.next_sibling(at);
        }
    }

    fn class_names(&self, root: NodeRef<'gc>) -> Vec<String> {
        let cx = self.cx;
        let mut names = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if cx.kind(node) == NodeKind::Element
                && let Some(classes) = cx.attribute_value_by_id(node, dom::names::CLASS)
            {
                for class in classes.borrow().to_str().split_ascii_whitespace() {
                    if seen.insert(class.to_owned()) {
                        names.push(class.to_owned());
                    }
                }
            }
            if let Some(root) = cx.shadow_root(node) {
                stack.push(root);
            }
            let mut child = cx.last_child(node);
            while let Some(at) = child {
                stack.push(at);
                child = cx.previous_sibling(at);
            }
        }
        names
    }

    /// Forgets the numbers of `node` and of what was sent below it (Blink's
    /// `Unbind`).
    fn unbind(&mut self, node: NodeRef<'gc>) {
        let Some(id) = self.bound(node) else {
            return;
        };
        let cx = self.cx;
        self.session.ids.remove(&cx.node_id(node));
        self.session.nodes.remove(&id);
        if let Some(document) = cx.content_document(node) {
            self.unbind(document);
        }
        if let Some(root) = cx.shadow_root(node) {
            self.unbind(root);
        }
        if let Some(content) = cx.template_content(node) {
            self.unbind(content);
        }
        if self.session.children_sent.remove(&id) {
            for child in self.children(node) {
                self.unbind(child);
            }
        }
        self.session.child_counts.remove(&id);
    }

    /// The events of the DOM's `changes`, in order, for the nodes the
    /// session has.
    fn changed(&mut self, changes: &[DomChange]) -> Events {
        let cx = self.cx;
        let mut events = Vec::new();
        let mut inline_styles = Vec::new();
        // The nodes sent without their children whose children changed:
        // told their counts once the batch is through.
        let mut recount = Vec::new();
        for change in changes {
            match *change {
                DomChange::Inserted { parent, node, .. } => {
                    let (Some(parent), Some(node)) = (cx.node_by_id(parent), cx.node_by_id(node))
                    else {
                        continue;
                    };
                    self.inserted(parent, node, &mut recount, &mut events);
                }
                DomChange::Removed { parent, node } => {
                    let Some(parent_id) = self.session.id(parent) else {
                        continue;
                    };
                    // White space the session wasn't sent, or a node gone
                    // it never had.
                    match cx.node_by_id(node) {
                        Some(removed) if self.skipped(removed) => continue,
                        None if self.session.id(node).is_none() => continue,
                        _ => {}
                    }
                    self.removed(parent_id, node, &mut recount, &mut events);
                }
                DomChange::Attribute { element, name } => {
                    let Some(element) = cx.node_by_id(element) else {
                        continue;
                    };
                    let Some(id) = self.bound(element) else {
                        continue;
                    };
                    let value = (0..cx.attribute_count(element)).find_map(|index| {
                        (cx.attribute_name_at(element, index) == Some(name))
                            .then(|| cx.attribute_at(element, index))
                            .flatten()
                    });
                    let name = qualified(name.ids());
                    events.push(match value {
                        Some(attribute) => (
                            "DOM.attributeModified",
                            json!({ "nodeId": id, "name": name, "value": attribute.value.to_str() }),
                        ),
                        None => ("DOM.attributeRemoved", json!({ "nodeId": id, "name": name })),
                    });
                }
                DomChange::CharacterData { node } => {
                    let Some(node) = cx.node_by_id(node) else {
                        continue;
                    };
                    match self.bound(node) {
                        Some(_) if self.skipped(node) => {
                            match cx.parent(node).and_then(|parent| self.bound(parent)) {
                                Some(parent) => {
                                    self.removed(
                                        parent,
                                        cx.node_id(node),
                                        &mut recount,
                                        &mut events,
                                    );
                                }
                                None => self.unbind(node),
                            }
                        }
                        Some(id) => {
                            let data = cx.data(node).map(|data| data.to_str().into_owned());
                            events.push((
                                "DOM.characterDataModified",
                                json!({ "nodeId": id, "characterData": data.unwrap_or_default() }),
                            ));
                        }
                        None => {
                            if let Some(parent) = cx.parent(node) {
                                self.inserted(parent, node, &mut recount, &mut events);
                            }
                        }
                    }
                }
                DomChange::InlineStyle { element } => {
                    if let Some(id) = self.session.id(element)
                        && !inline_styles.contains(&id)
                    {
                        inline_styles.push(id);
                    }
                }
                DomChange::ShadowAttached { host, root } => {
                    let (Some(host), Some(root)) = (cx.node_by_id(host), cx.node_by_id(root))
                    else {
                        continue;
                    };
                    let Some(host_id) = self.bound(host) else {
                        continue;
                    };
                    self.push_children(host_id, 1, false, &mut events);
                    let root = self.build(root, 0, false, true);
                    events.push((
                        "DOM.shadowRootPushed",
                        json!({ "hostId": host_id, "root": root }),
                    ));
                }
            }
        }
        for id in recount {
            if self.session.children_sent.contains(&id) {
                continue;
            }
            let Some(node) = self.node(id) else {
                continue;
            };
            let count = u32::try_from(self.children(node).len()).unwrap_or(u32::MAX);
            if self.session.child_counts.insert(id, count) != Some(count) {
                events.push((
                    "DOM.childNodeCountUpdated",
                    json!({ "nodeId": id, "childNodeCount": count }),
                ));
            }
        }
        if !inline_styles.is_empty() {
            events.push((
                "DOM.inlineStyleInvalidated",
                json!({ "nodeIds": inline_styles }),
            ));
        }
        events
    }

    /// `node` was put in `parent`'s children (Blink's `DidInsertDOMNode`):
    /// told if the session has `parent`, as a count if it wasn't sent
    /// `parent`'s children (`recount`, once the batch is through), else as
    /// the node itself, after the closest sibling before it the session has
    /// (one put in later in the batch is told after it, before it). One
    /// taken out or moved since in the batch is told as its later changes
    /// say.
    fn inserted(
        &mut self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        recount: &mut Vec<Id>,
        events: &mut Events,
    ) {
        let cx = self.cx;
        if self.skipped(node)
            || cx.parent(node).map(|at| cx.node_id(at)) != Some(cx.node_id(parent))
        {
            return;
        }
        self.unbind(node);
        let Some(parent_id) = self.bound(parent) else {
            return;
        };
        if !self.session.children_sent.contains(&parent_id) {
            if !recount.contains(&parent_id) {
                recount.push(parent_id);
            }
            return;
        }
        let mut previous = cx.previous_sibling(node);
        let previous_id = loop {
            let Some(at) = previous else {
                break 0;
            };
            if !self.skipped(at)
                && let Some(id) = self.bound(at)
            {
                break id;
            }
            previous = cx.previous_sibling(at);
        };
        let built = self.build(node, 0, false, true);
        events.push((
            "DOM.childNodeInserted",
            json!({ "parentNodeId": parent_id, "previousNodeId": previous_id, "node": built }),
        ));
    }

    /// `node` was taken out of the children of the node numbered
    /// `parent_id` (Blink's `DOMNodeRemoved`): as a count if the session
    /// wasn't sent them (`recount`).
    fn removed(&mut self, parent_id: Id, node: NodeId, recount: &mut Vec<Id>, events: &mut Events) {
        if !self.session.children_sent.contains(&parent_id) {
            if !recount.contains(&parent_id) {
                recount.push(parent_id);
            }
            return;
        }
        let Some(id) = self.session.id(node) else {
            return;
        };
        events.push((
            "DOM.childNodeRemoved",
            json!({ "parentNodeId": parent_id, "nodeId": id }),
        ));
        match self.cx.node_by_id(node) {
            Some(node) => self.unbind(node),
            None => {
                self.session.ids.remove(&node);
                self.session.nodes.remove(&id);
                self.session.children_sent.remove(&id);
                self.session.child_counts.remove(&id);
            }
        }
    }

    /// An iframe has a new document: told as taken out and put back with
    /// it, if the session has it (Blink's `InvalidateFrameOwnerElement`).
    fn frame_owner_changed(&mut self, owner: NodeRef<'gc>) -> Events {
        let mut events = Vec::new();
        let Some(owner_id) = self.bound(owner) else {
            return events;
        };
        let parent_id = self
            .parent(owner)
            .and_then(|parent| self.bound(parent))
            .unwrap_or(0);
        events.push((
            "DOM.childNodeRemoved",
            json!({ "parentNodeId": parent_id, "nodeId": owner_id }),
        ));
        self.unbind(owner);
        let built = self.build(owner, 0, false, true);
        let mut previous = self.cx.previous_sibling(owner);
        let previous_id = loop {
            let Some(at) = previous else {
                break 0;
            };
            if !self.skipped(at) {
                break self.bound(at).unwrap_or(0);
            }
            previous = self.cx.previous_sibling(at);
        };
        events.push((
            "DOM.childNodeInserted",
            json!({ "parentNodeId": parent_id, "previousNodeId": previous_id, "node": built }),
        ));
        events
    }
}

/// `quad` as DevTools has it: x and y of each corner in turn.
pub(crate) fn quad_json(quad: &layout::Quad) -> Value {
    quad.iter()
        .flat_map(|point| [f64::from(point.x), f64::from(point.y)])
        .collect()
}

/// `f` with the DOM and the page's frame tree (its layout brought up to
/// date first), and `node` with its document's id; none if the page has no
/// layout lent, or `node` is gone.
pub(crate) fn with_layout<R>(
    page: &mut dyn DevToolsPage,
    node: NodeId,
    f: impl for<'gc> FnOnce(&Cx<'_, 'gc>, &mut Frame, NodeRef<'gc>, NodeId) -> Option<R>,
) -> Option<R> {
    let (dom, layout) = page.dom_and_layout();
    let layout = layout?;
    dom.read(|cx| {
        layout.update_style_and_layout(cx);
        let frame = layout.as_any_mut().downcast_mut::<Frame>()?;
        let element = cx.node_by_id(node)?;
        let document = cx.node_id(cx.document_of(element));
        f(cx, frame, element, document)
    })
}

/// `DOM.getBoxModel`'s model of `node` (Blink's
/// `InspectorHighlight::GetBoxModel`): its quads in the top-level frame's
/// viewport, its border box's size as `offsetWidth` and `offsetHeight`
/// have it. None for a node with no box.
fn box_model(page: &mut dyn DevToolsPage, node: NodeId) -> Option<Value> {
    with_layout(page, node, |_, frame, _, document| {
        let quads = frame.box_quads_in_top(document, node)?;
        let size = frame.view.view_of(document)?.element_box(node)?.border_box;
        Some(json!({
            "model": {
                "content": quad_json(&quads.content),
                "padding": quad_json(&quads.padding),
                "border": quad_json(&quads.border),
                "margin": quad_json(&quads.margin),
                "width": size.width.round(),
                "height": size.height.round(),
            }
        }))
    })
}

/// `DOM.getContentQuads`' quads of `node`: its border boxes, empty with
/// none (Blink's `GetContentQuads`).
fn content_quads(page: &mut dyn DevToolsPage, node: NodeId) -> Option<Value> {
    with_layout(page, node, |_, frame, _, document| {
        let quads: Vec<Value> = frame
            .border_quads_in_top(document, node)
            .iter()
            .map(quad_json)
            .collect();
        Some(json!({ "quads": quads }))
    })
}

/// The node at `point`, CSS px of the top-level frame's document if
/// `document_point` (`getNodeForLocation`), else of its viewport, through
/// the frames (Blink's hit test for the inspector): a run of text's
/// element; one in a user-agent shadow tree its host's unless
/// `user_agent`. None outside the viewport, or what is laid out.
pub(crate) fn node_at(
    page: &mut dyn DevToolsPage,
    point: gfx::PointF,
    document_point: bool,
    user_agent: bool,
) -> Option<NodeId> {
    let (dom, layout) = page.dom_and_layout();
    let layout = layout?;
    dom.read(|cx| {
        layout.update_style_and_layout(cx);
        let frame = layout.as_any_mut().downcast_mut::<Frame>()?;
        let point = if document_point {
            let scroll = frame.top_viewport_scroll();
            gfx::PointF::new(point.x - scroll.x, point.y - scroll.y)
        } else {
            point
        };
        // Outside the viewport, nothing is hit (Blink hit tests the frame's
        // visible rect).
        let viewport = frame.viewport_size();
        if !((0.0..viewport.width).contains(&point.x) && (0.0..viewport.height).contains(&point.y))
        {
            return None;
        }
        let hit = frame.hit_test_in_top(point);
        let mut node = match hit.text.and_then(|text| cx.node_by_id(text)) {
            Some(text) => cx.parent(text)?,
            None => cx.node_by_id(hit.element?)?,
        };
        while !user_agent && let Some(root) = cx.containing_shadow_root(node) {
            let user_agent_root = cx
                .shadow_root_init(root)
                .is_some_and(|init| init.mode == dom::ShadowRootMode::UserAgent);
            if !user_agent_root {
                break;
            }
            node = cx.shadow_host(root)?;
        }
        Some(cx.node_id(node))
    })
}

/// `document`'s element child.
fn document_element<'gc>(cx: &Cx<'_, 'gc>, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let mut child = cx.first_child(document);
    while let Some(at) = child {
        if cx.kind(at) == NodeKind::Element {
            return Some(at);
        }
        child = cx.next_sibling(at);
    }
    None
}

/// A command's `depth`: `default` if none, -1 the whole subtree.
fn depth(params: &Value, default: i64) -> i64 {
    match params["depth"].as_i64().unwrap_or(default) {
        -1 => i64::MAX,
        depth => depth.max(0),
    }
}

/// A name as its qualified name: `prefix:local`, or `local`.
fn qualified(name: dom::NameIds) -> String {
    let local = dom::names::string(name.local);
    if name.prefix == dom::names::NONE {
        return local.to_str().into_owned();
    }
    format!(
        "{}:{}",
        dom::names::string(name.prefix).to_str(),
        local.to_str()
    )
}

/// `text`, cut at `MAX_TEXT_SIZE` characters with an ellipsis after
/// (Blink's `BuildObjectForNode`).
fn truncated(text: &str) -> String {
    match text.char_indices().nth(MAX_TEXT_SIZE) {
        Some((at, _)) => format!("{}\u{2026}", &text[..at]),
        None => text.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document with a `<body>`, the agent's top-level one.
    fn page(agent: &DomAgent) -> (Dom, NodeRoot) {
        let mut dom = Dom::new();
        let document = dom.mutate(|cx| {
            let document = cx.create_document(true);
            let html = cx.create_element(document, dom::QualName::html("html"));
            cx.append_child(document, html).unwrap();
            let body = cx.create_element(document, dom::QualName::html("body"));
            cx.append_child(html, body).unwrap();
            cx.root(document)
        });
        let id = dom.read(|cx| cx.node_id(document.get(cx.gc)));
        agent.document_committed(id);
        (dom, document)
    }

    fn command(method: &str) -> String {
        json!({ "id": 1, "method": method, "params": {} }).to_string()
    }

    /// A DOM lent as a page with no engine: no layout, no objects.
    struct Lent<'a>(&'a mut Dom);

    impl DevToolsPage for Lent<'_> {
        fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn dom_api::FrameLayout>) {
            (self.0, None)
        }

        fn objects(&mut self) -> &mut dyn dom_api::DevToolsObjects {
            unreachable!("no command of the test asks the engine")
        }
    }

    /// The DOM notes its changes while a session has `DOM` on, and not once
    /// none has: its last one turned it off, or went.
    #[test]
    fn the_dom_notes_changes_while_a_session_has_dom_on() {
        let (to_browser, _from) = crossbeam_channel::unbounded();
        let agent = DomAgent::new(to_browser);
        let (mut dom, document) = page(&agent);
        let page = &mut Lent(&mut dom);
        assert!(agent.handle("a", &command("DOM.enable"), page));
        assert!(agent.handle("b", &command("DOM.enable"), page));
        assert!(agent.any_enabled());
        assert!(agent.handle("a", &command("DOM.disable"), page));
        assert!(agent.any_enabled(), "b's is on");
        agent.detach("b");
        assert!(!agent.any_enabled(), "b's went with it");
        assert!(agent.sessions.borrow().get("b").is_none());
        // Another domain's command isn't the agent's.
        assert!(!agent.handle("a", &command("CSS.enable"), page));
        drop(document);
    }
}
