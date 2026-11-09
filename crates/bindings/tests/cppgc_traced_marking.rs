//! `TracedSlot` writes interleaved with marking: `--stress-incremental-marking`
//! and no forced GCs, so V8 starts ordinary incremental cycles from
//! allocation, and the slots are set and cleared while they run. A value
//! stored into an already-traced holder is only kept if V8 marks it on
//! assignment.
//!
//! Run locally: `cargo test -p bindings --test cppgc_traced_marking`.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]
#![allow(unsafe_code, reason = "drives the slot's unsafe API directly")]

use std::ffi::CStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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

/// xorshift64*.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as usize % n
    }
}

fn seed(default: u64) -> u64 {
    let seed = std::env::var("MACHOPO_TEST_SEED")
        .ok()
        .and_then(|v| match v.strip_prefix("0x") {
            Some(hex) => u64::from_str_radix(hex, 16).ok(),
            None => v.parse().ok(),
        })
        .unwrap_or(default);
    eprintln!("MACHOPO_TEST_SEED={seed:#x}");
    seed
}

fn extra_v8_flag(flag: &str) -> bool {
    std::env::var("MACHOPO_V8_FLAGS").is_ok_and(|flags| flags.split_whitespace().any(|f| f == flag))
}

const HOLDERS: usize = 64;

/// Whether an incremental cycle is between its start and its final pause,
/// per V8's GC callbacks: an incremental cycle starts with the
/// `kGCTypeIncrementalMarking` callbacks and finalizes with the
/// `kGCTypeMarkSweepCompact` ones.
static MARKING: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn on_gc_prologue(
    _: v8::UnsafeRawIsolatePtr,
    gc_type: v8::GCType,
    _: v8::GCCallbackFlags,
    _: *mut std::ffi::c_void,
) {
    if gc_type == v8::GCType::kGCTypeMarkSweepCompact {
        MARKING.store(false, Ordering::SeqCst);
    }
}

unsafe extern "C" fn on_gc_epilogue(
    _: v8::UnsafeRawIsolatePtr,
    gc_type: v8::GCType,
    _: v8::GCCallbackFlags,
    _: *mut std::ffi::c_void,
) {
    if gc_type == v8::GCType::kGCTypeIncrementalMarking {
        MARKING.store(true, Ordering::SeqCst);
    }
}

#[test]
fn slot_writes_interleaved_with_marking() {
    bindings::initialize_with_flags("--stress-incremental-marking");
    let mut data = V8PerIsolateData::new();
    let all = v8::GCType::kGCTypeAll;
    data.isolate()
        .add_gc_prologue_callback(on_gc_prologue, std::ptr::null_mut(), all);
    data.isolate()
        .add_gc_epilogue_callback(on_gc_epilogue, std::ptr::null_mut(), all);
    let context = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Context::new(scope, Default::default());
        v8::Global::new(scope, context)
    };
    let drops = Arc::new(AtomicUsize::new(0));
    let new_holder = |heap: &v8::cppgc::Heap| {
        let holder = Holder {
            slot: TracedSlot::empty(),
            drops: drops.clone(),
        };
        // SAFETY: moved into a Persistent right away.
        Persistent::new(&unsafe { v8::cppgc::make_garbage_collected(heap, holder) })
    };
    let holders: Vec<Persistent<Holder>> = (0..HOLDERS)
        .map(|_| new_holder(data.isolate().get_cpp_heap().unwrap()))
        .collect();
    // The id of the object each holder's slot should point to.
    let mut expected: Vec<Option<i32>> = vec![None; HOLDERS];
    let mut rng = Rng(seed(0x7ace_d510) | 1);
    let mut sets_while_marking = 0;

    for round in 0..300 {
        let mut garbage = Vec::new();
        {
            let isolate = data.isolate();
            v8::scope_with_context!(let scope, isolate, &context);
            let key = v8::String::new(scope, "id").unwrap();
            for i in 0..40 {
                let index = rng.below(HOLDERS);
                let holder = holders[index].get().unwrap();
                if rng.below(8) == 0 {
                    // SAFETY: on the heap's thread; the holder is on the heap.
                    unsafe { holder.slot.set(scope, None) };
                    expected[index] = None;
                    continue;
                }
                let id = round * 100 + i;
                let object = v8::Object::new(scope);
                let value = v8::Integer::new(scope, id);
                object.set(scope, key.into(), value.into()).unwrap();
                // SAFETY: as above.
                unsafe { holder.slot.set(scope, Some(object)) };
                expected[index] = Some(id);
                sets_while_marking += usize::from(MARKING.load(Ordering::SeqCst));
                // Garbage on both heaps, to drive allocation and marking.
                let array = v8::Array::new(scope, 64);
                for j in 0..64 {
                    let element = v8::Object::new(scope);
                    array.set_index(scope, j, element.into()).unwrap();
                }
                let dead = new_holder(scope.get_cpp_heap().unwrap());
                // SAFETY: as above.
                unsafe { dead.get().unwrap().slot.set(scope, Some(array.into())) };
                garbage.push(dead);
            }
        }
        drop(garbage);
        // Top of the "event loop": let V8 run marking steps / finalization.
        data.pump_platform_tasks();

        let isolate = data.isolate();
        v8::scope_with_context!(let scope, isolate, &context);
        let key = v8::String::new(scope, "id").unwrap();
        for (holder, expected) in holders.iter().zip(&expected) {
            // SAFETY: on the heap's thread.
            let object = unsafe { holder.get().unwrap().slot.get(scope) };
            let id = object.map(|o| {
                o.get(scope, key.into())
                    .unwrap()
                    .int32_value(scope)
                    .unwrap()
            });
            assert_eq!(id, *expected, "round {round}");
        }
    }

    let finalized = drops.load(Ordering::SeqCst);
    if extra_v8_flag("--no-incremental-marking") {
        // Nothing starts a GC then: V8 only starts incremental cycles, and
        // this test forces none.
        eprintln!("--no-incremental-marking: no GC expected");
    } else {
        // ~10k garbage holders were created; cycles must have completed and
        // swept a good share of them without any forced GC.
        assert!(
            finalized > 1000,
            "only {finalized} holders finalized: incremental cycles did not complete"
        );
        eprintln!("finalized {finalized} holders without a forced GC");
        // The point of the test: stores into holders a running cycle may
        // already have traced.
        assert!(
            sets_while_marking > 1000,
            "only {sets_while_marking} slots set while marking"
        );
        eprintln!("{sets_while_marking} slots set while marking");
    }
    drop(holders);
    drop(context);
}
