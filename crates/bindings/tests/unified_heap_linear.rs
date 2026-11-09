//! A elise heap V8 drives (`register_embedder_heap`), in V8's linear
//! closure: with `--ephemeron-fixpoint-iterations=0` V8's pause always takes
//! it (else only after ten rounds of ephemerons), and its one pass with the
//! heap stops once V8 and the heap say they are done. An object V8 hands
//! its main thread's local in that pass (fewer than a segment's worth: the
//! local publishes full ones itself) is marked only if the local counts as
//! work: objects rooted in the heap, behind their wrappers' properties'
//! wrappers, live through an atomic full GC. How much of that V8 finds
//! only in the linear pass depends on how far its parallel markers got
//! before it (with `--single-threaded-gc`, its main thread finds it all in
//! earlier passes), so the test makes new ones fifty times, collecting each three times.
//!
//! Run locally: `cargo test -p bindings --test unified_heap_linear`.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]
#![allow(
    unsafe_code,
    reason = "registers the heap, which must go before the isolate"
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bindings::V8PerIsolateData;
use elise::{GcObject, Heap, HeapOptions, JsEdge, Persistent, WeakPersistent};

const FLAGS: &str = "--expose-gc --ephemeron-fixpoint-iterations=0";

#[derive(GcObject)]
struct Node {
    wrapper: JsEdge,
    value: Cell<u32>,
}

/// The heap as V8 drives it.
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
}

/// Rooted objects A, each referring to its wrapper WA, whose property
/// refers to wrapper WC, which wraps C, which nothing else holds: V8 finds
/// each WC only from what the heap gave it in its pause, and hands C to its
/// main thread's local then.
#[test]
fn objects_behind_wrappers_behind_objects_live() {
    const PAIRS: u32 = 20;
    bindings::initialize_with_flags(FLAGS);
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
    let registration =
        unsafe { bindings::register_embedder_heap(data.isolate(), Box::new(Bridge(heap.clone()))) };
    for round in 0..50 {
        let ends: Vec<(Persistent<Node>, WeakPersistent<Node>)> = {
            let isolate = data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let template = v8::FunctionTemplate::new(
                scope,
                |_: &mut v8::PinScope<'_, '_>,
                 _: v8::FunctionCallbackArguments,
                 _: v8::ReturnValue| {},
            );
            let function = template.get_function(scope).unwrap();
            (0..PAIRS)
                .map(|i| {
                    let wa = function.new_instance(scope, &[]).unwrap();
                    let wc = function.new_instance(scope, &[]).unwrap();
                    let key = v8::String::new(scope, "c").unwrap();
                    wa.set(scope, key.into(), wc.into()).unwrap();
                    heap.borrow_mut().mutate(|cx| {
                        let mut node = |wrapper| {
                            let node = cx.alloc(NodeInit {
                                wrapper: (),
                                value: Cell::new(i),
                            });
                            bindings::wrap(scope, cx, wrapper, node);
                            bindings::set_wrapper(scope, &node.wrapper, wrapper);
                            node
                        };
                        let (a, c) = (node(wa), node(wc));
                        (Persistent::new(cx, a), WeakPersistent::new(cx, c))
                    })
                })
                .collect()
        };
        // The second and later GCs find them in another order (marked in
        // the GC before).
        for _ in 0..3 {
            data.isolate()
                .request_garbage_collection_for_testing_with_stack_state(
                    v8::GarbageCollectionType::Full,
                    v8::cppgc::EmbedderStackState::NoHeapPointers,
                );
        }
        let lost = heap
            .borrow()
            .read(|cx| ends.iter().filter(|(_, c)| c.get(cx).is_none()).count());
        assert_eq!(
            lost, 0,
            "round {round}: objects behind the wrappers' properties lost"
        );
    }
    drop(registration);
}
