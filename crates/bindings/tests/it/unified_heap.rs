//! A elise heap V8's major GCs mark (`register_embedder_heap`): its objects
//! wrapped by V8's API wrappers (`wrap`, `unwrap`), and their references to
//! their wrappers (`set_wrapper`, `wrapper`). V8's full GCs keep what script
//! holds through a wrapper and what the heap's roots hold through an
//! object's reference to its wrapper, and collect cycles that go through
//! both heaps; V8's scavenge drops an unmodified wrapper nothing holds and
//! empties the reference, but not a modified one.
//!
//! These link the real V8 static library, so run them locally:
//! `cargo test -p bindings --test it unified_heap::`.

#![allow(
    unsafe_code,
    reason = "registers the heap, which must go before the isolate"
)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use bindings::{EmbedderRegistration, V8PerIsolateData};
use elise::{GcObject, Heap, HeapOptions, JsEdge, Member, Persistent, WeakPersistent};

#[derive(GcObject)]
struct Node {
    next: Member<Node>,
    wrapper: JsEdge,
    value: Cell<u32>,
}

/// Another type with a JS edge: its wrappers have another tag.
#[derive(GcObject)]
struct Other {
    wrapper: JsEdge,
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

struct Env {
    registration: Option<EmbedderRegistration>,
    heap: Rc<RefCell<Heap>>,
    context: v8::Global<v8::Context>,
    template: v8::Global<v8::FunctionTemplate>,
    data: V8PerIsolateData,
}

fn setup() -> Env {
    let mut data = V8PerIsolateData::new();
    let heap = Rc::new(RefCell::new(Heap::with_platform(
        HeapOptions {
            driven: true,
            verify: true,
            ..HeapOptions::default()
        },
        bindings::gc_platform(),
    )));
    // SAFETY: `Env`'s drop drops it first, before the isolate.
    let registration =
        unsafe { bindings::register_embedder_heap(data.isolate(), Box::new(Bridge(heap.clone()))) };
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

/// What a test made: the object (weakly, to see it go) and a root of it if
/// the heap holds it.
struct Made<T: elise::HeapType> {
    weak: WeakPersistent<T>,
    root: Option<Persistent<T>>,
}

impl Env {
    /// Runs `f` in a context scope.
    fn run<R>(&mut self, f: impl FnOnce(&mut v8::PinScope<'_, '_>, &Self2<'_>) -> R) -> R {
        let env = Self2 {
            heap: &self.heap,
            template: &self.template,
        };
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        f(scope, &env)
    }

    /// A full GC that doesn't scan the stack (as `cppgc_traced`'s): what the
    /// tests keep alive is rooted by a global or a `Persistent`.
    fn full_gc(&mut self) {
        self.data
            .isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Full,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }

    fn minor_gc(&mut self) {
        self.data
            .isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Minor,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }

    fn alive<T: elise::HeapType>(&self, made: &Made<T>) -> bool {
        self.heap.borrow().read(|cx| made.weak.get(cx).is_some())
    }

    fn eval(&mut self, source: &str) -> String {
        self.run(|scope, _| {
            let source = v8::String::new(scope, source).unwrap();
            let script = v8::Script::compile(scope, source, None).unwrap();
            script.run(scope).unwrap().to_rust_string_lossy(scope)
        })
    }
}

/// `Env`'s parts a scope's closure uses.
struct Self2<'a> {
    heap: &'a Rc<RefCell<Heap>>,
    template: &'a v8::Global<v8::FunctionTemplate>,
}

impl Self2<'_> {
    /// A new API wrapper (an object of the template's), wrapping nothing.
    fn new_wrapper<'s>(&self, scope: &mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object> {
        let template = v8::Local::new(scope, self.template);
        template
            .get_function(scope)
            .unwrap()
            .new_instance(scope, &[])
            .unwrap()
    }

    /// A node of `value` wrapped by `wrapper`, which its JS edge refers to;
    /// rooted if `rooted`; pointing at `next`'s node.
    fn wrap_node(
        &self,
        scope: &mut v8::PinScope<'_, '_>,
        wrapper: v8::Local<'_, v8::Object>,
        value: u32,
        next: Option<&Made<Node>>,
        rooted: bool,
    ) -> Made<Node> {
        self.heap.borrow().read(|cx| {
            let next = next.map(|next| next.weak.get(cx).expect("alive"));
            let node = cx.alloc(NodeInit {
                next,
                wrapper: (),
                value: Cell::new(value),
            });
            bindings::wrap(scope, cx, wrapper, node);
            bindings::set_wrapper(scope, &node.wrapper, wrapper);
            Made {
                weak: WeakPersistent::new(cx, node),
                root: rooted.then(|| Persistent::new(cx, node)),
            }
        })
    }
}

fn set_global(scope: &mut v8::PinScope<'_, '_>, name: &str, value: v8::Local<'_, v8::Value>) {
    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, name).unwrap();
    global.set(scope, key.into(), value).unwrap();
}

fn global<'s>(scope: &mut v8::PinScope<'s, '_>, name: &str) -> v8::Local<'s, v8::Value> {
    let global = scope.get_current_context().global(scope);
    let key = v8::String::new(scope, name).unwrap();
    global.get(scope, key.into()).unwrap()
}

#[test]
fn a_wrapped_object_lives_while_script_holds_its_wrapper() {
    let mut env = setup();
    let made = env.run(|scope, env| {
        let wrapper = env.new_wrapper(scope);
        set_global(scope, "kept", wrapper.into());
        env.wrap_node(scope, wrapper, 7, None, false)
    });
    for _ in 0..3 {
        env.full_gc();
    }
    assert!(env.alive(&made));
    let value = env.run(|scope, env| {
        let kept = global(scope, "kept");
        let heap = env.heap.borrow();
        heap.read(|cx| {
            let node = bindings::unwrap::<Node>(scope, cx, kept).expect("a node's wrapper");
            node.value.get()
        })
    });
    assert_eq!(value, 7);
    env.eval("kept = undefined");
    env.full_gc();
    assert!(!env.alive(&made));
}

#[test]
fn a_rooted_objects_wrapper_lives_with_its_properties() {
    let mut env = setup();
    let made = env.run(|scope, env| {
        let wrapper = env.new_wrapper(scope);
        let made = env.wrap_node(scope, wrapper, 1, None, true);
        // A property makes the wrapper modified: the scavenge keeps it.
        let key = v8::String::new(scope, "x").unwrap();
        let value = v8::Integer::new(scope, 42);
        wrapper.set(scope, key.into(), value.into()).unwrap();
        made
    });
    env.minor_gc();
    for _ in 0..3 {
        env.full_gc();
    }
    let x = env.run(|scope, env| {
        let heap = env.heap.borrow();
        let wrapper = heap.read(|cx| {
            let node = made.root.as_ref().unwrap().get(cx);
            bindings::wrapper(scope, &node.wrapper)
        });
        let wrapper = wrapper.expect("the node's wrapper lives");
        let key = v8::String::new(scope, "x").unwrap();
        wrapper
            .get(scope, key.into())
            .unwrap()
            .int32_value(scope)
            .unwrap()
    });
    assert_eq!(x, 42);
}

#[test]
fn the_scavenge_drops_an_unmodified_wrapper_nothing_holds() {
    if crate::v8_gcs_at_its_own_times() {
        eprintln!(
            "skipped: V8 GCs at its own times, and its scavenge keeps what a marking reached"
        );
        return;
    }
    let mut env = setup();
    let made = env.run(|scope, env| {
        let wrapper = env.new_wrapper(scope);
        env.wrap_node(scope, wrapper, 1, None, true)
    });
    env.minor_gc();
    let has_wrapper = env.run(|scope, env| {
        let heap = env.heap.borrow();
        heap.read(|cx| {
            let node = made.root.as_ref().unwrap().get(cx);
            bindings::wrapper(scope, &node.wrapper).is_some()
        })
    });
    assert!(!has_wrapper, "the reference is emptied with the wrapper");
    assert!(env.alive(&made), "the node is the heap's root's");
}

/// Object A refers to wrapper WA (its JS edge), whose property refers to
/// wrapper WB, which wraps B, which points at A: both heaps, twice.
#[test]
fn a_cycle_through_both_heaps_lives_while_held_and_then_goes() {
    let mut env = setup();
    let (a, b) = env.run(|scope, env| {
        let wa = env.new_wrapper(scope);
        let a = env.wrap_node(scope, wa, 1, None, false);
        let wb = env.new_wrapper(scope);
        let b = env.wrap_node(scope, wb, 2, Some(&a), false);
        let key = v8::String::new(scope, "b").unwrap();
        wa.set(scope, key.into(), wb.into()).unwrap();
        set_global(scope, "kept", wb.into());
        (a, b)
    });
    for _ in 0..3 {
        env.full_gc();
    }
    assert!(env.alive(&a) && env.alive(&b));
    env.eval("kept = undefined");
    env.full_gc();
    assert!(!env.alive(&a) && !env.alive(&b));
}

/// Rooted objects A, each referring to its wrapper WA, whose property
/// refers to wrapper WC, which wraps C, which nothing else holds: V8 finds
/// each WC only from what the heap gave it, and hands C over then, on its
/// parallel markers too (which publish to the heap's worklist, not the main
/// thread's local): V8's pause waits for the heap to mark them.
#[test]
fn objects_behind_wrappers_behind_objects_live() {
    const PAIRS: u32 = 2000;
    let mut env = setup();
    let ends: Vec<(Made<Node>, Made<Node>)> = env.run(|scope, env| {
        (0..PAIRS)
            .map(|i| {
                let wa = env.new_wrapper(scope);
                let a = env.wrap_node(scope, wa, i, None, true);
                let wc = env.new_wrapper(scope);
                let c = env.wrap_node(scope, wc, i, None, false);
                let key = v8::String::new(scope, "c").unwrap();
                wa.set(scope, key.into(), wc.into()).unwrap();
                (a, c)
            })
            .collect()
    });
    for _ in 0..3 {
        env.full_gc();
    }
    let lost = ends.iter().filter(|(_, c)| !env.alive(c)).count();
    assert_eq!(lost, 0, "objects behind the wrappers' properties lost");
    drop(ends);
    env.full_gc();
}

#[test]
fn unwrap_reads_its_types_wrappers_only() {
    let mut env = setup();
    env.run(|scope, env| {
        let node_wrapper = env.new_wrapper(scope);
        let _node = env.wrap_node(scope, node_wrapper, 3, None, false);
        let other_wrapper = env.new_wrapper(scope);
        let unwrapped = {
            let heap = env.heap.borrow();
            heap.read(|cx| {
                let other = cx.alloc(OtherInit { wrapper: () });
                bindings::wrap(scope, cx, other_wrapper, other);
                let plain = v8::Object::new(scope);
                let number = v8::Integer::new(scope, 1);
                (
                    bindings::unwrap::<Node>(scope, cx, node_wrapper.into()).map(|n| n.value.get()),
                    bindings::unwrap::<Node>(scope, cx, other_wrapper.into()).is_some(),
                    bindings::unwrap::<Other>(scope, cx, other_wrapper.into()).is_some(),
                    bindings::unwrap::<Node>(scope, cx, plain.into()).is_some(),
                    bindings::unwrap::<Node>(scope, cx, number.into()).is_some(),
                )
            })
        };
        assert_eq!(unwrapped, (Some(3), false, true, false, false));
        assert!(bindings::is_wrapper::<Node>(scope, node_wrapper.into()));
        assert!(!bindings::is_wrapper::<Node>(scope, other_wrapper.into()));
        let number = v8::Integer::new(scope, 1);
        assert!(!bindings::is_wrapper::<Node>(scope, number.into()));
    });
}

#[test]
#[should_panic(expected = "wrapping nothing yet")]
fn a_wrapper_wraps_one_object() {
    let mut env = setup();
    env.run(|scope, env| {
        let wrapper = env.new_wrapper(scope);
        let _first = env.wrap_node(scope, wrapper, 1, None, false);
        let _second = env.wrap_node(scope, wrapper, 2, None, false);
    });
}

/// A heap that is the isolate's state goes with it, V8's GC reaching it
/// through the slot; a second state (of any type) panics before it
/// registers, so that the first stays the one V8 marks.
#[test]
fn a_heap_registered_with_the_state_stays_the_isolates() {
    struct Counting {
        heap: Heap,
        ends: Rc<std::cell::Cell<u32>>,
    }
    impl bindings::EmbedderHeapState for Counting {
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
    struct Other(Counting);
    impl bindings::EmbedderHeapState for Other {
        fn start(&mut self, sink: Arc<dyn elise::JsEdgeSink>) -> Option<elise::WrapperMarker> {
            self.0.start(sink)
        }
        fn advance(&mut self, budget: Duration, atomic: bool) -> bool {
            self.0.advance(budget, atomic)
        }
        fn end(&mut self) -> usize {
            self.0.end()
        }
    }
    let counting = |ends: &Rc<std::cell::Cell<u32>>| Counting {
        heap: Heap::with_platform(
            HeapOptions {
                driven: true,
                ..HeapOptions::default()
            },
            Arc::new(elise::InlinePlatform),
        ),
        ends: ends.clone(),
    };
    let full_gc = |data: &mut V8PerIsolateData| {
        data.isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Full,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    };
    let mut data = V8PerIsolateData::new();
    let ends = Rc::new(std::cell::Cell::new(0));
    bindings::set_isolate_state_with_embedder_heap(data.isolate(), counting(&ends));
    full_gc(&mut data);
    assert_eq!(ends.get(), 1, "V8 marked the heap");
    assert_eq!(
        bindings::isolate_state_mut::<Counting>(data.isolate()).map(|state| state.ends.get()),
        Some(1),
        "the state is the heap"
    );
    let others = Rc::new(std::cell::Cell::new(0));
    // A state of the same type, and of another.
    let second = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bindings::set_isolate_state_with_embedder_heap(data.isolate(), counting(&others));
    }));
    assert!(second.is_err(), "a second state");
    let third = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        bindings::set_isolate_state_with_embedder_heap(data.isolate(), Other(counting(&others)));
    }));
    assert!(third.is_err(), "a second state");
    full_gc(&mut data);
    assert_eq!(
        (ends.get(), others.get()),
        (2, 0),
        "V8 marks the first heap still"
    );
    drop(data);
}
