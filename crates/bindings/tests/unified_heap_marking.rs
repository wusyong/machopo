//! A elise heap V8 drives (`register_embedder_heap`), under V8's incremental
//! markings: `--stress-incremental-marking` starts them from allocation,
//! and with `--single-threaded-gc` V8's main thread does all of its
//! marking and shares none of its work, so what it hands the heap stays in
//! its own local unless it publishes it before each of the heap's steps
//! (else V8's pause waits for that local to empty, forever). A chain of
//! wrapped objects made while markings run (each wrapper set on its object
//! mid-marking) lives through them while script holds its end, then goes;
//! a registration dropped while a marking runs finishes it first, and so
//! does the isolate's state that is its embedder heap.
//!
//! Run locally: `cargo test -p bindings --test unified_heap_marking`.

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

use bindings::{EmbedderRegistration, V8PerIsolateData};
use elise::{GcObject, Heap, HeapOptions, JsEdge, Member, WeakPersistent};

const FLAGS: &str = "--expose-gc --single-threaded-gc --stress-incremental-marking";

#[derive(GcObject)]
struct Node {
    next: Member<Node>,
    wrapper: JsEdge,
    value: Cell<u32>,
}

/// The heap as V8 drives it, counting the markings V8 ran of it.
struct Bridge(Rc<RefCell<Heap>>, Rc<Cell<u32>>);

impl bindings::EmbedderHeap for Bridge {
    fn start(&self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
        Some(self.0.borrow_mut().start_marking(sink))
    }

    fn advance(&self, budget: Duration, atomic: bool) -> bool {
        self.0.borrow_mut().advance_marking(atomic, budget)
    }

    fn end(&self) -> usize {
        self.1.set(self.1.get() + 1);
        self.0.borrow_mut().finish_marking()
    }
}

struct Env {
    registration: Option<EmbedderRegistration>,
    heap: Rc<RefCell<Heap>>,
    markings: Rc<Cell<u32>>,
    context: v8::Global<v8::Context>,
    template: v8::Global<v8::FunctionTemplate>,
    data: V8PerIsolateData,
}

/// Whether this run's V8 marks incrementally: `MACHOPO_V8_FLAGS` (as
/// `test.py`'s fuzzer sets it) may turn it off, and then no marking of the
/// heap runs alongside script for these tests to see.
fn v8_marks_incrementally() -> bool {
    !std::env::var("MACHOPO_V8_FLAGS").is_ok_and(|flags| {
        flags
            .split_whitespace()
            .any(|flag| flag == "--no-incremental-marking")
    })
}

fn setup() -> Env {
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
    let markings = Rc::new(Cell::new(0));
    // SAFETY: `Env`'s drop drops it first, before the isolate.
    let registration = unsafe {
        bindings::register_embedder_heap(
            data.isolate(),
            Box::new(Bridge(heap.clone(), markings.clone())),
        )
    };
    let (context, template) = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        let template = v8::FunctionTemplate::new(
            scope,
            |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
        );
        (
            v8::Global::new(scope, context),
            v8::Global::new(scope, template),
        )
    };
    Env {
        registration: Some(registration),
        heap,
        markings,
        context,
        template,
        data,
    }
}

impl Drop for Env {
    fn drop(&mut self) {
        self.registration = None;
    }
}

impl Env {
    /// Garbage on V8's heap, whose allocation advances V8's markings (and
    /// so the heap's), and the heap's growth told to V8.
    fn churn(&mut self) {
        self.eval("for (let i = 0; i < 20; i++) { const a = []; for (let j = 0; j < 500; j++) a.push({ j }); }");
        let used = self.heap.borrow().used_bytes();
        bindings::report_embedder_used_size(self.data.isolate(), used);
    }

    fn eval(&mut self, source: &str) -> String {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let source = v8::String::new(scope, source).unwrap();
        let script = v8::Script::compile(scope, source, None).unwrap();
        script.run(scope).unwrap().to_rust_string_lossy(scope)
    }

    /// A node of `value` after `next`, wrapped by a new wrapper whose
    /// `index` is `value` (modified: the scavenge keeps it), which becomes
    /// `globalThis.kept`.
    fn push(&mut self, value: u32, next: Option<&WeakPersistent<Node>>) -> WeakPersistent<Node> {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let template = v8::Local::new(scope, &self.template);
        let wrapper = template
            .get_function(scope)
            .unwrap()
            .new_instance(scope, &[])
            .unwrap();
        let key = v8::String::new(scope, "index").unwrap();
        let index = v8::Integer::new_from_unsigned(scope, value);
        wrapper.set(scope, key.into(), index.into()).unwrap();
        let global = context.global(scope);
        let key = v8::String::new(scope, "kept").unwrap();
        global.set(scope, key.into(), wrapper.into()).unwrap();
        self.heap.borrow_mut().mutate(|cx| {
            let next = next.map(|next| next.get(cx).expect("alive"));
            let node = cx.alloc(NodeInit {
                next,
                wrapper: (),
                value: Cell::new(value),
            });
            bindings::wrap(scope, cx, wrapper, node);
            bindings::set_wrapper(scope, &node.wrapper, wrapper);
            WeakPersistent::new(cx, node)
        })
    }

    /// The sum of the chain's wrappers' `index`, from `kept`'s node on.
    fn sum_of_indices(&mut self) -> u64 {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let global = context.global(scope);
        let key = v8::String::new(scope, "kept").unwrap();
        let kept = global.get(scope, key.into()).unwrap();
        let wrappers = self.heap.borrow().read(|cx| {
            let mut node = bindings::unwrap::<Node>(scope, cx, kept);
            let mut wrappers = Vec::new();
            while let Some(n) = node {
                wrappers
                    .push(bindings::wrapper(scope, &n.wrapper).expect("a modified wrapper lives"));
                node = n.next.get(cx);
            }
            wrappers
        });
        let key = v8::String::new(scope, "index").unwrap();
        wrappers
            .into_iter()
            .map(|w| {
                w.get(scope, key.into())
                    .unwrap()
                    .integer_value(scope)
                    .unwrap() as u64
            })
            .sum()
    }

    fn alive(&self, node: &WeakPersistent<Node>) -> bool {
        self.heap.borrow().read(|cx| node.get(cx).is_some())
    }
}

#[test]
fn a_chain_of_wrappers_through_incremental_markings() {
    if !v8_marks_incrementally() {
        eprintln!("skipped: V8 doesn't mark incrementally in this run");
        return;
    }
    const NODES: u32 = 3000;
    let mut env = setup();
    let first = env.push(0, None);
    let mut last = env.push(1, Some(&first));
    for value in 2..NODES {
        last = env.push(value, Some(&last));
        if value % 50 == 0 {
            env.churn();
        }
    }
    assert!(
        env.markings.get() > 0,
        "V8 ran markings of the heap meanwhile"
    );
    // V8 ends a marking when its schedule says, not after so many
    // allocations: bounded generously, so only a marking that never ends
    // fails.
    let markings = env.markings.get();
    for _ in 0..200 {
        if env.markings.get() > markings {
            break;
        }
        env.churn();
    }
    assert!(env.markings.get() > markings, "and afterwards");
    let expected = u64::from(NODES) * u64::from(NODES - 1) / 2;
    assert_eq!(env.sum_of_indices(), expected);
    assert!(env.alive(&first));
    env.eval("kept = undefined");
    // Asked only after two whole markings: a weak read while one runs
    // marks what it reads (as `unwrap`'s), which would keep the chain
    // alive; once a marking ended without it, it reads as gone.
    let markings = env.markings.get();
    for _ in 0..50 {
        if env.markings.get() >= markings + 2 {
            break;
        }
        env.churn();
    }
    // V8 schedules its markings by its own clock and the heaps' growth:
    // with its debug build's pace, often not twice in so many rounds. What
    // it hasn't, full GCs finish (each ends a marking of the heap).
    for _ in 0..4 {
        if env.markings.get() >= markings + 2 {
            break;
        }
        env.data
            .isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Full,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }
    assert!(
        env.markings.get() >= markings + 2,
        "V8 marked the heap twice more"
    );
    // Then a full GC that doesn't scan the stack: in a pause that may find
    // heap pointers there (the incremental markings', from allocation), V8
    // scans it conservatively, and a stale word pointing to one of the
    // chain's `TracedReference`s keeps that wrapper, and the chain from its
    // node on, a GC longer (as Blink's on-stack references).
    env.data
        .isolate()
        .request_garbage_collection_for_testing_with_stack_state(
            v8::GarbageCollectionType::Full,
            v8::cppgc::EmbedderStackState::NoHeapPointers,
        );
    // What a failure needs to be told apart: which end lived, after how
    // many markings, and what the last one marked.
    let last_gc = env.heap.borrow().stats().last_gc;
    let ends = (env.alive(&first), env.alive(&last));
    assert_eq!(
        ends,
        (false, false),
        "the chain goes with its wrappers: (first, last) alive after markings {markings} to {}; the last marked {last_gc:?}",
        env.markings.get()
    );
}

#[test]
fn dropping_the_registration_finishes_the_marking_that_runs() {
    if !v8_marks_incrementally() {
        eprintln!("skipped: V8 doesn't mark incrementally in this run");
        return;
    }
    let mut env = setup();
    let node = env.push(1, None);
    for _ in 0..200 {
        if env.heap.borrow().is_marking() {
            break;
        }
        env.churn();
    }
    assert!(
        env.heap.borrow().is_marking(),
        "V8 started a marking of the heap"
    );
    let markings = env.markings.get();
    env.registration = None;
    assert!(!env.heap.borrow().is_marking());
    assert_eq!(
        env.markings.get(),
        markings + 1,
        "V8 ended it with the heap"
    );
    // V8 goes on without the heap.
    for _ in 0..20 {
        env.churn();
    }
    assert_eq!(env.markings.get(), markings + 1);
    assert!(env.alive(&node));
}

/// The heap as the isolate's state (`set_isolate_state_with_embedder_heap`),
/// counting the markings V8 ended of it.
struct StateHeap {
    heap: Heap,
    ends: Rc<Cell<u32>>,
}

impl bindings::EmbedderHeapState for StateHeap {
    fn start(&mut self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
        Some(self.heap.start_marking(sink))
    }

    fn advance(&mut self, budget: Duration, atomic: bool) -> bool {
        self.heap.advance_marking(atomic, budget)
    }

    fn end(&mut self) -> usize {
        self.ends.set(self.ends.get() + 1);
        self.heap.finish_marking()
    }
}

/// The state dropped while a marking of it runs: its registration goes
/// first, V8 ending the marking with the state whole (through its slot),
/// then the state; V8 goes on without it.
#[test]
fn dropping_the_state_finishes_the_marking_that_runs() {
    if !v8_marks_incrementally() {
        eprintln!("skipped: V8 doesn't mark incrementally in this run");
        return;
    }
    bindings::initialize_with_flags(FLAGS);
    let mut data = V8PerIsolateData::new();
    let ends = Rc::new(Cell::new(0));
    bindings::set_isolate_state_with_embedder_heap(
        data.isolate(),
        StateHeap {
            heap: Heap::with_platform(
                HeapOptions {
                    driven: true,
                    ..HeapOptions::default()
                },
                bindings::gc_platform(),
            ),
            ends: ends.clone(),
        },
    );
    let context = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        v8::Global::new(scope, context)
    };
    let churn = |data: &mut V8PerIsolateData| {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let source = v8::String::new(
            scope,
            "for (let i = 0; i < 20; i++) { const a = []; for (let j = 0; j < 500; j++) a.push({ j }); }",
        )
        .unwrap();
        v8::Script::compile(scope, source, None)
            .unwrap()
            .run(scope)
            .unwrap();
    };
    let marking = |data: &mut V8PerIsolateData| {
        bindings::isolate_state_mut::<StateHeap>(data.isolate())
            .is_some_and(|state| state.heap.is_marking())
    };
    for _ in 0..200 {
        if marking(&mut data) {
            break;
        }
        churn(&mut data);
    }
    assert!(marking(&mut data), "V8 started a marking of the heap");
    let ended = ends.get();
    // SAFETY: no callback of the isolate runs after (the test makes none).
    unsafe { bindings::drop_isolate_state(data.isolate()) };
    assert_eq!(ends.get(), ended + 1, "V8 ended it with the state");
    for _ in 0..20 {
        churn(&mut data);
    }
    assert_eq!(ends.get(), ended + 1);
}
