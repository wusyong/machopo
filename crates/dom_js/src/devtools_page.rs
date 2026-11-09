//! The page as the host's DevTools agents read it (`dom_api::DevToolsPage`),
//! as a command is answered or as script pauses: the DOM and the layout in
//! the isolate's state, borrowed through the isolate (script is at a
//! statement or not running: no one else borrows it), and what V8's
//! inspector knows of script's objects (Blink's `ResolveNode`,
//! `NodeForRemoteObjectId`, `InspectableNode`, and the DOM debugger agent's
//! `EventListenersInfoForTarget`). And what DevTools calls a node
//! (`subtype`: Blink's `ThreadDebuggerCommonImpl::valueSubtype` and
//! `descriptionForValueSubtype`).

use dom::{
    Dom, EventTargetImpl, Node, NodeId, NodeKind, NodeRef, ObjectRef, TargetRef, TargetRoot,
    Window, names,
};
use dom_api::{DevToolsObjects, DevToolsPage, FrameLayout};
use serde_json::{Value, json};

use crate::listeners::{have, wrap};
use crate::state::{self, Scope, State};

/// The page, borrowed through `isolate`; the layout taken from the state as
/// an operation takes it (`ApiCx::with_layout_cell`), put back as the page
/// goes.
pub(crate) struct Page<'a> {
    isolate: &'a mut v8::Isolate,
    layout: Option<Box<dyn FrameLayout>>,
    taken: bool,
}

impl<'a> Page<'a> {
    pub(crate) fn new(isolate: &'a mut v8::Isolate) -> Self {
        Self {
            isolate,
            layout: None,
            taken: false,
        }
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        if self.taken
            && let Some(state) = bindings::isolate_state_mut::<State>(self.isolate)
        {
            state.layout.set(self.layout.take());
        }
    }
}

impl DevToolsPage for Page<'_> {
    fn dom_and_layout(&mut self) -> (&mut Dom, Option<&mut dyn FrameLayout>) {
        let Self {
            isolate,
            layout,
            taken,
        } = self;
        let state = bindings::isolate_state_mut::<State>(isolate).expect("the engine's DOM");
        if !*taken {
            *layout = state.layout.take();
            *taken = true;
        }
        (&mut state.dom, layout.as_deref_mut())
    }

    fn objects(&mut self) -> &mut dyn DevToolsObjects {
        self
    }
}

/// The wrapper of the node `id` names, made if it has none.
fn node_wrapper<'s>(
    scope: &mut Scope<'_, 's, '_>,
    id: NodeId,
) -> Option<v8::Local<'s, v8::Object>> {
    let have = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            cx.node_by_id(id)
                .map(|node| have(&mut no_gc, cx, ObjectRef::Node(node)))
        })?
    };
    Some(wrap(scope, have))
}

/// The realm of the frame showing `node`'s document, if one does (Blink's
/// `ResolveNode` takes the frame's main world).
fn realm_of(isolate: &mut v8::Isolate, node: NodeId) -> Option<v8::Global<v8::Context>> {
    let state = bindings::isolate_state_mut::<State>(isolate)?;
    let document = state.dom.read(|cx| {
        cx.node_by_id(node)
            .map(|node| cx.node_id(cx.document_of(node)))
    })?;
    Some(state.frames.of_document(document)?.context.clone())
}

/// The top-level frame's realm: what an object is unwrapped in (the
/// inspector gives its own context with it).
fn top_realm(isolate: &mut v8::Isolate) -> Option<v8::Global<v8::Context>> {
    let state = bindings::isolate_state_mut::<State>(isolate)?;
    Some(state.frames.get(crate::frames::TOP)?.context.clone())
}

impl DevToolsObjects for Page<'_> {
    fn resolve_node(&mut self, session: &str, node: NodeId, group: &str) -> Option<String> {
        self.put_back_layout();
        let realm = realm_of(self.isolate, node)?;
        v8::scope!(let v8_scope, &mut *self.isolate);
        let context = v8::Local::new(v8_scope, realm);
        let v8_scope = &mut v8::ContextScope::new(v8_scope, context);
        let scope = &mut state::scope(v8_scope)?;
        let wrapper = node_wrapper(scope, node)?;
        bindings::devtools::wrap_object(scope.scope(), session, context, wrapper.into(), group)
    }

    fn node_of_object(&mut self, session: &str, object_id: &str) -> Result<NodeId, String> {
        self.put_back_layout();
        let not_a_node = || "Object id doesn't reference a Node".to_owned();
        let realm = top_realm(self.isolate).ok_or_else(not_a_node)?;
        v8::scope!(let v8_scope, &mut *self.isolate);
        let top = v8::Local::new(v8_scope, realm);
        let v8_scope = &mut v8::ContextScope::new(v8_scope, top);
        let (value, _, _) = bindings::devtools::unwrap_object(v8_scope, session, object_id)?;
        let object = v8::Local::<v8::Object>::try_from(value).map_err(|_| not_a_node())?;
        let scope = &mut state::scope(v8_scope).ok_or_else(not_a_node)?;
        let (state, mut no_gc) = scope.state();
        state
            .dom
            .read(|cx| {
                no_gc
                    .unwrap::<Node>(cx.gc, object)
                    .map(|node| cx.node_id(node))
            })
            .ok_or_else(not_a_node)
    }

    fn inspect_node(&mut self, session: &str, node: NodeId) {
        self.put_back_layout();
        bindings::devtools::add_inspected_object(self.isolate, session, Box::new(Inspected(node)));
    }

    fn event_listeners(
        &mut self,
        session: &str,
        object_id: &str,
        depth: i64,
        pierce: bool,
        include_whitespace: bool,
    ) -> Result<String, String> {
        self.put_back_layout();
        let Some(realm) = top_realm(self.isolate) else {
            return Ok("[]".to_owned());
        };
        v8::scope!(let v8_scope, &mut *self.isolate);
        let top = v8::Local::new(v8_scope, realm);
        let v8_scope = &mut v8::ContextScope::new(v8_scope, top);
        let (value, context, group) =
            bindings::devtools::unwrap_object(v8_scope, session, object_id)?;
        let v8_scope = &mut v8::ContextScope::new(v8_scope, context);
        let Some(scope) = &mut state::scope(v8_scope) else {
            return Ok("[]".to_owned());
        };
        let targets = match v8::Local::<v8::Object>::try_from(value) {
            Ok(object) => targets_of(scope, object, depth, pierce, include_whitespace),
            Err(_) => Vec::new(),
        };
        let mut listeners = Vec::new();
        for (target, backend) in targets {
            listeners.extend(listeners_of(scope, &target, backend));
        }
        // Those that capture first: they come first (Blink's
        // `BuildObjectsForEventListeners`).
        let (capturing, bubbling): (Vec<_>, Vec<_>) =
            listeners.into_iter().partition(|listener| listener.capture);
        let built: Vec<Value> = capturing
            .into_iter()
            .chain(bubbling)
            .map(|listener| listener.build(scope, session, context, &group))
            .collect();
        Ok(Value::Array(built).to_string())
    }
}

impl Page<'_> {
    /// The layout back in the state before the inspector runs: what it
    /// does may reach the DOM (a getter, a handler compiled), whose
    /// operations borrow the layout from there.
    fn put_back_layout(&mut self) {
        if self.taken
            && let Some(state) = bindings::isolate_state_mut::<State>(self.isolate)
        {
            state.layout.set(self.layout.take());
            self.taken = false;
        }
    }
}

/// `$0`: the node, as its frame's realm wraps it; undefined once gone.
struct Inspected(NodeId);

impl v8::inspector::InspectableImpl for Inspected {
    fn get<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        _context: v8::Local<'s, v8::Context>,
    ) -> v8::Local<'s, v8::Value> {
        let wrapper = state::scope(scope).and_then(|mut scope| {
            let wrapper = node_wrapper(&mut scope, self.0)?;
            // As the scope's own handle: the state's scope ends here.
            Some(v8::Global::new(scope.scope(), wrapper))
        });
        match wrapper {
            Some(wrapper) => v8::Local::new(scope, wrapper).into(),
            None => v8::undefined(scope).into(),
        }
    }
}

/// The targets whose listeners DevTools lists for `object`: a node and,
/// `depth` levels down, those below it that have listeners (Blink's
/// `CollectNodes` with `FilterNodesWithListeners`), each with its backend
/// id; else the window or other target it is.
fn targets_of(
    scope: &mut Scope<'_, '_, '_>,
    object: v8::Local<'_, v8::Object>,
    depth: i64,
    pierce: bool,
    include_whitespace: bool,
) -> Vec<(TargetRoot, Option<u32>)> {
    let (state, mut no_gc) = scope.state();
    state.dom.read(|cx| {
        if let Some(node) = no_gc.unwrap::<Node>(cx.gc, object) {
            let depth = if depth < 0 { i64::MAX } else { depth };
            let mut found = Vec::new();
            collect_nodes(cx, node, depth, pierce, include_whitespace, &mut found);
            return found
                .into_iter()
                .map(|node| {
                    (
                        cx.root_target(TargetRef::Node(node)),
                        Some(cx.node_id(node).get()),
                    )
                })
                .collect();
        }
        if let Some(window) = no_gc.unwrap::<Window>(cx.gc, object) {
            return vec![(cx.root_target(TargetRef::Window(window)), None)];
        }
        if let Some(target) = no_gc.unwrap::<EventTargetImpl>(cx.gc, object) {
            return vec![(cx.root_target(TargetRef::EventTarget(target)), None)];
        }
        Vec::new()
    })
}

/// Blink's `InspectorDOMAgent::CollectNodes`: `node` if it has listeners,
/// and those below it, `depth` levels down, an iframe's document and a
/// shadow root's too if `pierce`.
fn collect_nodes<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    node: NodeRef<'gc>,
    depth: i64,
    pierce: bool,
    include_whitespace: bool,
    found: &mut Vec<NodeRef<'gc>>,
) {
    let has_listeners = cx
        .listeners(TargetRef::Node(node))
        .is_some_and(|listeners| {
            (0..listeners.len()).any(|index| {
                listeners
                    .get(cx.gc, index)
                    .is_some_and(|listener| !cx.listener_removed(listener))
            })
        });
    if has_listeners {
        found.push(node);
    }
    let depth = depth.saturating_sub(1);
    if depth <= 0 {
        return;
    }
    if pierce && cx.kind(node) == NodeKind::Element {
        if let Some(document) = cx.content_document(node) {
            collect_nodes(cx, document, depth, pierce, include_whitespace, found);
        }
        if let Some(root) = cx.shadow_root(node) {
            collect_nodes(cx, root, depth, pierce, include_whitespace, found);
        }
    }
    let mut child = cx.first_child(node);
    while let Some(at) = child {
        let white_space = cx.kind(at) == NodeKind::Text
            && cx
                .text_data(at)
                .is_some_and(|data| data.borrow().to_str().trim().is_empty());
        if include_whitespace || !white_space {
            collect_nodes(cx, at, depth, pierce, include_whitespace, found);
        }
        child = cx.next_sibling(at);
    }
}

/// A listener, as DevTools lists it.
struct Listener<'s> {
    event_type: String,
    capture: bool,
    passive: bool,
    once: bool,
    /// What was added: a function, or an object with `handleEvent`.
    handler: v8::Local<'s, v8::Object>,
    /// What is called: the function, or the object's `handleEvent`.
    function: v8::Local<'s, v8::Function>,
    backend: Option<u32>,
}

impl<'s> Listener<'s> {
    /// As a `DOMDebugger.EventListener` (Blink's
    /// `BuildObjectForEventListener`): its handlers wrapped in the object
    /// `group` the target's object was in, if it was in one.
    fn build(
        self,
        scope: &mut Scope<'_, 's, '_>,
        session: &str,
        context: v8::Local<'s, v8::Context>,
        group: &str,
    ) -> Value {
        let mut built = json!({
            "type": self.event_type,
            "useCapture": self.capture,
            "passive": self.passive,
            "once": self.once,
            "scriptId": self.function.script_id().to_string(),
            "lineNumber": self.function.get_script_line_number().unwrap_or(0),
            "columnNumber": self.function.get_script_column_number().unwrap_or(0),
        });
        if !group.is_empty() {
            let v8_scope = scope.scope();
            let function = bindings::devtools::wrap_object(
                v8_scope,
                session,
                context,
                self.function.into(),
                group,
            );
            if let Some(function) =
                function.and_then(|json| serde_json::from_str::<Value>(&json).ok())
            {
                built["handler"] = function;
            }
            let handler = bindings::devtools::wrap_object(
                v8_scope,
                session,
                context,
                self.handler.into(),
                group,
            );
            if let Some(handler) =
                handler.and_then(|json| serde_json::from_str::<Value>(&json).ok())
            {
                built["originalHandler"] = handler;
            }
        }
        if let Some(backend) = self.backend {
            built["backendNodeId"] = backend.into();
        }
        built
    }
}

/// `target`'s listeners not removed, by type in the order the types were
/// first added, each type's in order (Blink's `CollectEventListeners`); an
/// event handler of a content attribute compiled first (Blink's
/// `GetListenerObject`), one that doesn't compile left out.
fn listeners_of<'s>(
    scope: &mut Scope<'_, 's, '_>,
    target: &TargetRoot,
    backend: Option<u32>,
) -> Vec<Listener<'s>> {
    let uncompiled: Vec<names::NameId> = {
        let (state, _) = scope.state();
        state.dom.read(|cx| {
            let Some(listeners) = cx.listeners(target.get(cx.gc)) else {
                return Vec::new();
            };
            (0..listeners.len())
                .filter_map(|index| listeners.get(cx.gc, index))
                .filter(|listener| {
                    !cx.listener_removed(*listener) && cx.uncompiled_handler(*listener).is_some()
                })
                .map(|listener| cx.listener_type(listener))
                .collect()
        })
    };
    for event_type in uncompiled {
        crate::handlers::compile_event_handler(scope, target, event_type);
    }
    let found: Vec<(names::NameId, dom::ListenerFlags, v8::Local<'s, v8::Object>)> = {
        let (state, mut no_gc) = scope.state();
        state.dom.read(|cx| {
            let Some(listeners) = cx.listeners(target.get(cx.gc)) else {
                return Vec::new();
            };
            (0..listeners.len())
                .filter_map(|index| listeners.get(cx.gc, index))
                .filter(|listener| !cx.listener_removed(*listener))
                .filter_map(|listener| {
                    let callback = no_gc.wrapper(cx.callback_edge(listener))?;
                    Some((
                        cx.listener_type(listener),
                        cx.listener_flags(listener),
                        callback,
                    ))
                })
                .collect()
        })
    };
    let mut types: Vec<names::NameId> = Vec::new();
    for (event_type, ..) in &found {
        if !types.contains(event_type) {
            types.push(*event_type);
        }
    }
    let mut listeners = Vec::new();
    for event_type in types {
        for (_, flags, handler) in found.iter().filter(|(at, ..)| *at == event_type) {
            let Some(function) = effective_function(scope, *handler) else {
                continue;
            };
            listeners.push(Listener {
                event_type: names::string(event_type).to_str().into_owned(),
                capture: flags.capture,
                passive: flags.passive,
                once: flags.once,
                handler: *handler,
                function,
                backend,
            });
        }
    }
    listeners
}

/// What a listener calls (Blink's `GetEffectiveFunction`): the function it
/// is, or its object's `handleEvent` (a getter it has runs).
fn effective_function<'s>(
    scope: &mut Scope<'_, 's, '_>,
    handler: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Function>> {
    if let Ok(function) = v8::Local::<v8::Function>::try_from(handler) {
        return Some(function);
    }
    let v8_scope = scope.scope();
    v8::tc_scope!(let tc, v8_scope);
    let key = v8::String::new(tc, "handleEvent")?;
    let value = handler.get(tc, key.into())?;
    v8::Local::<v8::Function>::try_from(value).ok()
}

/// What DevTools calls `value`, if it is a node: its subtype `node`, and
/// its tag with its id and classes (`div#app.main`), a doctype's
/// `<!DOCTYPE html>`, another node's name (Blink's
/// `ThreadDebuggerCommonImpl::descriptionForValueSubtype`).
pub(crate) fn subtype<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<bindings::devtools::Subtype> {
    let object = v8::Local::<v8::Object>::try_from(value).ok()?;
    let mut scope = state::scope(scope)?;
    let (state, mut no_gc) = scope.state();
    let description = state.dom.read(|cx| {
        let node = no_gc.unwrap::<Node>(cx.gc, object)?;
        Some(match cx.kind(node) {
            NodeKind::Element => {
                let name = cx.element_name(node)?;
                let mut description = names::string(name.local).to_str().into_owned();
                if name.prefix != names::NONE {
                    description = format!("{}:{description}", names::string(name.prefix).to_str());
                }
                if let Some(id) = cx.attribute_value_by_id(node, names::ID) {
                    let id = id.borrow().to_str().into_owned();
                    if !id.is_empty() {
                        description.push('#');
                        description.push_str(&id);
                    }
                }
                if let Some(classes) = cx.attribute_value_by_id(node, names::CLASS) {
                    for class in classes.borrow().to_str().split_ascii_whitespace() {
                        description.push('.');
                        description.push_str(class);
                    }
                }
                description
            }
            NodeKind::DocumentType => format!("<!DOCTYPE {}>", cx.node_name(node).to_str()),
            _ => cx.node_name(node).to_str().into_owned(),
        })
    })?;
    Some(bindings::devtools::Subtype {
        subtype: "node",
        description,
    })
}
