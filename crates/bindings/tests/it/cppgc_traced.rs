//! The `v8` fork's `TracedSlot` on the isolate's CppHeap: a GC
//! object's pointer to a JS value, traced across the two heaps in one GC.
//!
//! These link the real V8 static library, so run them locally:
//! `cargo test -p bindings --test it cppgc_traced::`.

#![allow(unsafe_code, reason = "drives the slot's unsafe API directly")]

use std::ffi::CStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use bindings::V8PerIsolateData;
use v8::cppgc::TracedSlot;
use v8::cppgc::{GarbageCollected, Persistent, Visitor};

struct Holder {
    slot: TracedSlot<v8::Object>,
    drops: Arc<AtomicUsize>,
}

// SAFETY: traces its one slot.
unsafe impl GarbageCollected for Holder {
    fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: the slot is a field of this object, which only lives on the
        // isolate's heap.
        unsafe { self.slot.trace(visitor) }
    }

    fn get_name(&self) -> &'static CStr {
        c"Holder"
    }
}

impl Drop for Holder {
    fn drop(&mut self) {
        #[expect(
            clippy::disallowed_methods,
            reason = "a test's count: far from wrapping"
        )]
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}

struct Env {
    data: V8PerIsolateData,
    context: v8::Global<v8::Context>,
    drops: Arc<AtomicUsize>,
}

fn setup() -> Env {
    let mut data = V8PerIsolateData::new();
    let context = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        v8::Global::new(scope, context)
    };
    Env {
        data,
        context,
        drops: Arc::default(),
    }
}

impl Env {
    fn drops(&self) -> usize {
        self.drops.load(Ordering::SeqCst)
    }

    /// A full GC that doesn't scan the stack: everything these tests keep
    /// alive is rooted by a `Persistent` or a global, and a stale pointer a
    /// returned call left on the stack would otherwise keep garbage alive
    /// (it did with the Windows x64 dev profile).
    fn gc(&mut self) {
        self.data
            .isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Full,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }
}

/// A new `Holder` with an empty slot, rooted by the returned `Persistent`.
#[inline(never)]
fn new_holder(heap: &v8::cppgc::Heap, drops: &Arc<AtomicUsize>) -> Persistent<Holder> {
    let holder = Holder {
        slot: TracedSlot::empty(),
        drops: drops.clone(),
    };
    // SAFETY: moved into a Persistent right away.
    Persistent::new(&unsafe { v8::cppgc::make_garbage_collected(heap, holder) })
}

/// `{ id }`.
fn new_object<'s>(scope: &v8::PinScope<'s, '_>, id: i32) -> v8::Local<'s, v8::Object> {
    let object = v8::Object::new(scope);
    let key = v8::String::new(scope, "id").unwrap();
    let value = v8::Integer::new(scope, id);
    object.set(scope, key.into(), value.into()).unwrap();
    object
}

fn id_of(scope: &v8::PinScope<'_, '_>, object: v8::Local<'_, v8::Object>) -> i32 {
    let key = v8::String::new(scope, "id").unwrap();
    let value = object.get(scope, key.into()).unwrap();
    value.int32_value(scope).unwrap()
}

/// Points `holder`'s slot at a new `{ id }` and returns a weak handle to it,
/// so the test can see whether the GC kept it.
#[inline(never)]
fn set_new_object(env: &mut Env, holder: &Persistent<Holder>, id: i32) -> v8::Weak<v8::Object> {
    let isolate = env.data.isolate();
    v8::scope_with_context!(let scope, isolate, &env.context);
    let object = new_object(scope, id);
    // SAFETY: on the heap's thread; the holder is on the heap.
    unsafe { holder.get().unwrap().slot.set(scope, Some(object)) };
    v8::Weak::new(scope, object)
}

/// The `id` of the object in `holder`'s slot, or `None` if it's empty.
fn slot_id(env: &mut Env, holder: &Persistent<Holder>) -> Option<i32> {
    let isolate = env.data.isolate();
    v8::scope_with_context!(let scope, isolate, &env.context);
    // SAFETY: on the heap's thread.
    let object = unsafe { holder.get().unwrap().slot.get(scope) }?;
    Some(id_of(scope, object))
}

fn clear_slot(env: &mut Env, holder: &Persistent<Holder>) {
    let isolate = env.data.isolate();
    v8::scope_with_context!(let scope, isolate, &env.context);
    // SAFETY: on the heap's thread.
    unsafe { holder.get().unwrap().slot.set(scope, None) };
}

#[test]
fn set_get_and_clear() {
    let mut env = setup();
    let holder = new_holder(env.data.isolate().get_cpp_heap().unwrap(), &env.drops);
    assert_eq!(slot_id(&mut env, &holder), None);
    let first = set_new_object(&mut env, &holder, 1);
    assert_eq!(slot_id(&mut env, &holder), Some(1));
    let second = set_new_object(&mut env, &holder, 2);
    assert_eq!(slot_id(&mut env, &holder), Some(2));
    clear_slot(&mut env, &holder);
    assert_eq!(slot_id(&mut env, &holder), None);
    drop((first, second, holder));
}

#[test]
fn keeps_the_value_alive_while_the_owner_lives() {
    let mut env = setup();
    let holder = new_holder(env.data.isolate().get_cpp_heap().unwrap(), &env.drops);
    let replaced = set_new_object(&mut env, &holder, 1);
    let kept = set_new_object(&mut env, &holder, 2);
    env.gc();
    // Only the slot points to `kept`; `replaced` is no longer in it.
    assert!(!kept.is_empty(), "the slot didn't keep its value alive");
    assert!(replaced.is_empty(), "the replaced value was kept");
    assert_eq!(slot_id(&mut env, &holder), Some(2));

    clear_slot(&mut env, &holder);
    env.gc();
    assert!(kept.is_empty(), "a cleared slot kept its old value");

    let last = set_new_object(&mut env, &holder, 3);
    drop(holder);
    env.gc();
    assert_eq!(env.drops(), 1);
    assert!(last.is_empty(), "a dead owner kept its value alive");
    drop((replaced, kept, last));
}

const TAG: u16 = 1;

/// A JS object wrapping a new `Holder` whose slot points back to the
/// wrapper: a cycle through both heaps, rooted by nothing.
#[inline(never)]
fn new_wrapper_cycle(env: &mut Env) -> v8::Weak<v8::Object> {
    let isolate = env.data.isolate();
    v8::scope_with_context!(let scope, isolate, &env.context);
    let template = v8::FunctionTemplate::new(
        scope,
        |_: &mut v8::PinScope<'_, '_>, _: v8::FunctionCallbackArguments, _: v8::ReturnValue| {},
    );
    let wrapper = template
        .get_function(scope)
        .unwrap()
        .new_instance(scope, &[])
        .unwrap();
    let holder = Holder {
        slot: TracedSlot::empty(),
        drops: env.drops.clone(),
    };
    // SAFETY: the pointer stays on the stack until the wrapper holds it; the
    // holder is on the heap when its slot is set.
    unsafe {
        let holder = v8::cppgc::make_garbage_collected(scope.get_cpp_heap().unwrap(), holder);
        v8::Object::wrap::<TAG, Holder>(scope, wrapper, &holder);
        holder.as_ref().slot.set(scope, Some(wrapper));
    }
    v8::Weak::new(scope, wrapper)
}

#[test]
fn collects_cycles_through_both_heaps() {
    let mut env = setup();
    let kept = new_wrapper_cycle(&mut env);
    {
        let isolate = env.data.isolate();
        v8::scope_with_context!(let scope, isolate, &env.context);
        let wrapper = kept.to_local(scope).unwrap();
        let key = v8::String::new(scope, "kept").unwrap();
        let global = scope.get_current_context().global(scope);
        global.set(scope, key.into(), wrapper.into()).unwrap();
    }
    let garbage = new_wrapper_cycle(&mut env);
    env.gc();
    assert_eq!(env.drops(), 1, "the unreachable cycle wasn't collected");
    assert!(garbage.is_empty());
    assert!(
        !kept.is_empty(),
        "the cycle rooted by a global was collected"
    );
    drop((kept, garbage));
}

/// Tearing the isolate down finalizes every holder, including ones whose
/// slot is still set: rooted, in a live cycle, and garbage not yet swept.
#[test]
fn teardown_with_live_slots() {
    let mut env = setup();
    let rooted = new_holder(env.data.isolate().get_cpp_heap().unwrap(), &env.drops);
    let rooted_value = set_new_object(&mut env, &rooted, 1);
    let unrooted = new_holder(env.data.isolate().get_cpp_heap().unwrap(), &env.drops);
    let unrooted_value = set_new_object(&mut env, &unrooted, 2);
    drop(unrooted);
    let cycle = new_wrapper_cycle(&mut env);
    {
        let isolate = env.data.isolate();
        v8::scope_with_context!(let scope, isolate, &env.context);
        let wrapper = cycle.to_local(scope).unwrap();
        let key = v8::String::new(scope, "kept").unwrap();
        let global = scope.get_current_context().global(scope);
        global.set(scope, key.into(), wrapper.into()).unwrap();
    }
    drop((rooted_value, unrooted_value, cycle));
    // A Persistent must not outlive its heap.
    drop(rooted);
    let Env {
        data,
        context,
        drops,
    } = env;
    drop(context);
    drop(data);
    assert_eq!(drops.load(Ordering::SeqCst), 3);
}
