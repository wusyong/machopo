//! A elise heap V8 drives in V8's heap snapshots (`EmbedderHeap::snapshot`,
//! `HeapGraph::add_heap`): each object a native node named by its type,
//! merged with its wrapper (one node: the wrapper's properties are its
//! edges, the wrapper itself no node of its own), its edges to other
//! objects named by their fields, its JS edges to V8's values by theirs,
//! its detachedness as the heap tells it, and the heap's roots under a
//! synthetic root.
//!
//! These link the real V8 static library, so run them locally:
//! `cargo test -p bindings --test it heap_snapshot::`.

#![allow(
    unsafe_code,
    reason = "registers the heap, which must go before the isolate"
)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bindings::{Detachedness, EmbedderRegistration, HeapGraph, V8PerIsolateData};
use elise::{EphemeronPair, GcObject, Heap, HeapOptions, JsEdge, Member, Persistent, WeakMember};
use serde_json::Value;

#[derive(GcObject)]
struct Node {
    next: Member<Node>,
    wrapper: JsEdge,
    value: Cell<u32>,
}

/// An object holding a JS value strongly (a listener's callback), and
/// nodes weakly.
#[derive(GcObject)]
struct Holder {
    node: Member<Node>,
    weak: WeakMember<Node>,
    pair: EphemeronPair<Node, Node>,
    callback: JsEdge,
}

/// The heap as V8 drives it; in a snapshot, a node is detached if its
/// value is odd, attached if even, and anything else unknown.
struct Bridge(Rc<RefCell<Heap>>);

impl bindings::EmbedderHeap for Bridge {
    fn start(&self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
        Some(self.0.borrow_mut().start_marking(sink))
    }

    fn advance(&self, budget: Duration, atomic: bool) -> bool {
        self.0.borrow_mut().advance_marking(atomic, budget)
    }

    fn end(&self) -> usize {
        self.0.borrow_mut().finish_marking()
    }

    fn snapshot(&self, graph: &mut HeapGraph<'_, '_, '_, '_>) {
        self.0.borrow_mut().walk(|_, walk| {
            graph.add_heap(walk, |object| match object.get().downcast::<Node>() {
                Some(node) if node.value.get() % 2 == 1 => Detachedness::Detached,
                Some(_) => Detachedness::Attached,
                None => Detachedness::Unknown,
            });
        });
    }
}

/// A heap snapshot's nodes, each with its name, type, detachedness and
/// edges (name or index, and the node it goes to).
struct Snapshot {
    nodes: Vec<SnapshotNode>,
}

struct SnapshotNode {
    name: String,
    kind: String,
    detachedness: u64,
    edges: Vec<(String, String, usize)>,
}

impl Snapshot {
    fn parse(json: &[u8]) -> Self {
        let snapshot: Value = serde_json::from_slice(json).unwrap();
        let meta = &snapshot["snapshot"]["meta"];
        let field = |fields: &Value, name: &str| {
            fields
                .as_array()
                .unwrap()
                .iter()
                .position(|field| field == name)
                .unwrap()
        };
        let node_fields = &meta["node_fields"];
        let edge_fields = &meta["edge_fields"];
        let width = node_fields.as_array().unwrap().len();
        let edge_width = edge_fields.as_array().unwrap().len();
        let types: Vec<&str> = meta["node_types"][0]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| kind.as_str().unwrap())
            .collect();
        let edge_types: Vec<&str> = meta["edge_types"][0]
            .as_array()
            .unwrap()
            .iter()
            .map(|kind| kind.as_str().unwrap())
            .collect();
        let strings: Vec<&str> = snapshot["strings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|string| string.as_str().unwrap())
            .collect();
        let number = |value: &Value| value.as_u64().unwrap() as usize;
        let raw: Vec<&[Value]> = snapshot["nodes"]
            .as_array()
            .unwrap()
            .chunks(width)
            .collect();
        let edges = snapshot["edges"].as_array().unwrap();
        let mut at = 0;
        let mut nodes = Vec::new();
        for node in &raw {
            let count = number(&node[field(node_fields, "edge_count")]);
            let mut own = Vec::new();
            for edge in edges[at..at + count * edge_width].chunks(edge_width) {
                let kind = edge_types[number(&edge[field(edge_fields, "type")])];
                let name = &edge[field(edge_fields, "name_or_index")];
                let name = if matches!(kind, "element" | "hidden") {
                    name.to_string()
                } else {
                    strings[number(name)].to_owned()
                };
                own.push((
                    kind.to_owned(),
                    name,
                    number(&edge[field(edge_fields, "to_node")]) / width,
                ));
            }
            at += count * edge_width;
            nodes.push(SnapshotNode {
                name: strings[number(&node[field(node_fields, "name")])].to_owned(),
                kind: types[number(&node[field(node_fields, "type")])].to_owned(),
                detachedness: node[field(node_fields, "detachedness")].as_u64().unwrap(),
                edges: own,
            });
        }
        Self { nodes }
    }

    fn named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a SnapshotNode> + 'a {
        self.nodes.iter().filter(move |node| node.name == name)
    }

    /// The node `edge` of `node` goes to.
    fn to(&self, node: &SnapshotNode, edge: &str) -> &SnapshotNode {
        let (_, _, to) = node
            .edges
            .iter()
            .find(|(_, name, _)| name == edge)
            .unwrap_or_else(|| panic!("{} has no edge {edge}: {:?}", node.name, node.edges));
        &self.nodes[*to]
    }
}

fn take(data: &mut V8PerIsolateData) -> Snapshot {
    let mut json = Vec::new();
    data.isolate().take_heap_snapshot(|chunk| {
        json.extend_from_slice(chunk);
        true
    });
    Snapshot::parse(&json)
}

/// Script holds node 1's wrapper (detached: odd), which has a property;
/// node 1 points at node 2 (attached), which a holder (rooted) points at,
/// whose callback is a function script made, and which holds node 3
/// weakly and an ephemeron of node 2 to node 3 (node 2 holds node 3 then).
/// Node 3 (detached), whose wrapper script dropped, is rooted.
#[test]
fn a_snapshot_has_the_heaps_objects() {
    let mut data = V8PerIsolateData::new();
    let heap = Rc::new(RefCell::new(Heap::with_platform(
        HeapOptions {
            driven: true,
            verify: true,
            ..HeapOptions::default()
        },
        bindings::gc_platform(),
    )));
    // SAFETY: dropped below, before the isolate.
    let registration: EmbedderRegistration =
        unsafe { bindings::register_embedder_heap(data.isolate(), Box::new(Bridge(heap.clone()))) };
    let roots = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let template = v8::FunctionTemplate::new(
            scope,
            |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
        );
        let name = v8::String::new(scope, "TestWrapper").unwrap();
        template.set_class_name(name);
        let function = template.get_function(scope).unwrap();
        let first = function.new_instance(scope, &[]).unwrap();
        let third = function.new_instance(scope, &[]).unwrap();
        let key = v8::String::new(scope, "property").unwrap();
        let value = v8::String::new(scope, "the wrapper's own").unwrap();
        first.set(scope, key.into(), value.into()).unwrap();
        let global = context.global(scope);
        let kept = v8::String::new(scope, "kept").unwrap();
        global.set(scope, kept.into(), first.into()).unwrap();
        let source = v8::String::new(scope, "(function aCallback() {})").unwrap();
        let callback = v8::Script::compile(scope, source, None)
            .unwrap()
            .run(scope)
            .unwrap();
        heap.borrow().read(|cx| {
            let mut node = |value: u32, next, wrapper: Option<v8::Local<'_, v8::Object>>| {
                let node = cx.alloc(NodeInit {
                    next,
                    wrapper: (),
                    value: Cell::new(value),
                });
                if let Some(wrapper) = wrapper {
                    bindings::wrap(scope, cx, wrapper, node);
                    bindings::set_wrapper(scope, &node.wrapper, wrapper);
                }
                node
            };
            let second = node(2, None, None);
            node(1, Some(second), Some(first));
            let third = node(3, None, Some(third));
            let holder = cx.alloc(HolderInit {
                node: Some(second),
                weak: Some(third),
                pair: (Some(second), Some(third)),
                callback: (),
            });
            bindings::set_strong_value(scope, &holder.callback, callback);
            (Persistent::new(cx, holder), Persistent::new(cx, third))
        })
    };
    let snapshot = take(&mut data);

    // Every node, the wrapped ones merged with their wrappers (named after
    // the node).
    let nodes: Vec<_> = snapshot.named("Node").collect();
    assert_eq!(nodes.len(), 3);
    assert!(nodes.iter().all(|node| node.kind == "native"));
    // The wrappers (whose prototype is `TestWrapper.prototype`) are the
    // wrapped nodes 1 and 3; the one object left is V8's boilerplate of
    // the template's instances (its template instantiation cache).
    let constructor = snapshot
        .nodes
        .iter()
        .find(|node| node.kind == "closure" && node.name == "TestWrapper")
        .expect("the wrappers' constructor");
    let prototype = constructor
        .edges
        .iter()
        .find(|(_, name, _)| name == "prototype")
        .map(|(_, _, to)| *to)
        .unwrap();
    let instances = |kind: &str| {
        snapshot
            .nodes
            .iter()
            .filter(|node| {
                node.kind == kind
                    && node
                        .edges
                        .iter()
                        .any(|(_, name, to)| name == "__proto__" && *to == prototype)
            })
            .count()
    };
    assert_eq!(instances("object"), 1, "a wrapper is its node's");
    assert_eq!(instances("native"), 2);
    // The wrapped one script holds: its wrapper's property, and its edge
    // to node 2.
    let first = nodes
        .iter()
        .find(|node| node.edges.iter().any(|(_, name, _)| name == "property"))
        .expect("node 1, with its wrapper's property");
    assert_eq!(snapshot.to(first, "property").name, "the wrapper's own");
    let second = snapshot.to(first, "next");
    assert_eq!(second.name, "Node");
    // Detached (1) or attached (2), as the heap said.
    assert_eq!(first.detachedness, 2);
    assert_eq!(second.detachedness, 1);
    let holder = snapshot.named("Holder").next().expect("the holder");
    assert_eq!(holder.kind, "native");
    assert_eq!(holder.detachedness, 0);
    assert!(std::ptr::eq(snapshot.to(holder, "node"), second));
    assert_eq!(snapshot.to(holder, "callback").name, "aCallback");
    // The roots' node, synthetic, holds the holder and node 3.
    let root = snapshot
        .named("(Persistent roots)")
        .next()
        .expect("the roots");
    assert_eq!(root.kind, "synthetic");
    let held: HashMap<&str, usize> =
        root.edges
            .iter()
            .fold(HashMap::new(), |mut held, (_, _, to)| {
                *held.entry(snapshot.nodes[*to].name.as_str()).or_default() += 1;
                held
            });
    assert_eq!(held, HashMap::from([("Holder", 1), ("Node", 1)]));
    let third = root
        .edges
        .iter()
        .map(|(_, _, to)| &snapshot.nodes[*to])
        .find(|node| node.name == "Node")
        .unwrap();
    assert_eq!(third.detachedness, 2);
    // Weak edges: to node 3, and the ephemeron's key and value; node 2,
    // the key, holds the value.
    let edge = |from: &SnapshotNode, to: &SnapshotNode| -> Vec<(String, String)> {
        from.edges
            .iter()
            .filter(|(_, _, at)| std::ptr::eq(&snapshot.nodes[*at], to))
            .map(|(kind, name, _)| (kind.clone(), name.clone()))
            .collect()
    };
    let pair = |kind: &str, name: &str| (kind.to_owned(), name.to_owned());
    assert_eq!(
        edge(holder, third),
        [pair("weak", "weak"), pair("weak", "pair")]
    );
    assert_eq!(
        edge(holder, second),
        [pair("internal", "node"), pair("weak", "pair")]
    );
    assert_eq!(edge(second, third), [pair("internal", "pair")]);

    drop(roots);
    drop(registration);
}

/// A JS edge to another object's wrapper, of the same type, is an edge to
/// that object, not the holder's own wrapper: a wrapper is its object's by
/// the address it wraps, not by its tag alone.
#[test]
fn another_objects_wrapper_isnt_the_holders() {
    let mut data = V8PerIsolateData::new();
    let heap = Rc::new(RefCell::new(Heap::with_platform(
        HeapOptions {
            driven: true,
            verify: true,
            ..HeapOptions::default()
        },
        bindings::gc_platform(),
    )));
    // SAFETY: dropped below, before the isolate.
    let registration: EmbedderRegistration =
        unsafe { bindings::register_embedder_heap(data.isolate(), Box::new(Bridge(heap.clone()))) };
    let roots = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let scope = &mut v8::ContextScope::new(scope, context);
        let template = v8::FunctionTemplate::new(
            scope,
            |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
        );
        let wrapper = template
            .get_function(scope)
            .unwrap()
            .new_instance(scope, &[])
            .unwrap();
        // Held by script too: nothing drops it.
        let kept = v8::String::new(scope, "kept").unwrap();
        context
            .global(scope)
            .set(scope, kept.into(), wrapper.into())
            .unwrap();
        heap.borrow().read(|cx| {
            let wrapped = cx.alloc(NodeInit {
                next: None,
                wrapper: (),
                value: Cell::new(1),
            });
            bindings::wrap(scope, cx, wrapper, wrapped);
            bindings::set_wrapper(scope, &wrapped.wrapper, wrapper);
            let holder = cx.alloc(NodeInit {
                next: None,
                wrapper: (),
                value: Cell::new(2),
            });
            bindings::set_wrapper(scope, &holder.wrapper, wrapper);
            (Persistent::new(cx, wrapped), Persistent::new(cx, holder))
        })
    };
    let snapshot = take(&mut data);

    let nodes: Vec<_> = snapshot.named("Node").collect();
    assert_eq!(nodes.len(), 2);
    // Only the holder has an edge by the field's name: the wrapped one's
    // own wrapper is merged with it.
    let holders: Vec<_> = nodes
        .iter()
        .filter(|node| node.edges.iter().any(|(_, name, _)| name == "wrapper"))
        .collect();
    assert_eq!(holders.len(), 1, "one holds the other's wrapper");
    let wrapped = nodes
        .iter()
        .find(|node| !std::ptr::eq(**node, *holders[0]))
        .unwrap();
    assert!(std::ptr::eq(snapshot.to(holders[0], "wrapper"), *wrapped));

    drop(roots);
    drop(registration);
}

/// A heap that tells nothing (the default `snapshot`) adds no node.
#[test]
fn a_heap_that_tells_nothing_adds_nothing() {
    struct Silent(Rc<RefCell<Heap>>);

    impl bindings::EmbedderHeap for Silent {
        fn start(&self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
            Some(self.0.borrow_mut().start_marking(sink))
        }

        fn advance(&self, budget: Duration, atomic: bool) -> bool {
            self.0.borrow_mut().advance_marking(atomic, budget)
        }

        fn end(&self) -> usize {
            self.0.borrow_mut().finish_marking()
        }
    }

    let mut data = V8PerIsolateData::new();
    let heap = Rc::new(RefCell::new(Heap::with_platform(
        HeapOptions {
            driven: true,
            ..HeapOptions::default()
        },
        bindings::gc_platform(),
    )));
    // SAFETY: dropped below, before the isolate.
    let registration =
        unsafe { bindings::register_embedder_heap(data.isolate(), Box::new(Silent(heap.clone()))) };
    let root = heap.borrow_mut().mutate(|cx| {
        Persistent::new(
            cx,
            cx.alloc(NodeInit {
                next: None,
                wrapper: (),
                value: Cell::new(0),
            }),
        )
    });
    let snapshot = take(&mut data);
    assert_eq!(snapshot.named("Node").count(), 0);
    assert_eq!(snapshot.named("(Persistent roots)").count(), 0);
    drop(root);
    drop(registration);
}
