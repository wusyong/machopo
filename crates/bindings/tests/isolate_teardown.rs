//! Dropping an isolate whose contexts still hold slots, under a GC flag that
//! makes V8 collect while it tears the isolate down. Its own binary: V8's
//! flags are per process.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

use bindings::{ModuleRecords, ScriptState, V8PerIsolateData};

/// rusty_v8 frees a context's slots, and the weak handle that watches the
/// context, before V8's own teardown; that GC then called the weak handle's
/// callback with the freed handle's data.
#[test]
fn an_isolate_drops_with_its_contexts_slots() {
    bindings::initialize_with_flags("--stress-compaction");
    for _ in 0..3 {
        let mut data = V8PerIsolateData::new();
        let state = ScriptState::new(&mut data);
        // The module table is the realm's slot.
        let records = {
            let isolate = data.isolate();
            v8::scope_with_context!(let scope, isolate, state.context());
            ModuleRecords::new(scope)
        };
        drop(records);
        drop(state);
        drop(data);
    }
}
