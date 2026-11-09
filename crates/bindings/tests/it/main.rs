//! `bindings`'s integration tests, one binary (each file was one): a change in
//! `bindings` or below links this once, not once per file.
//! `cppgc_traced_marking` and `isolate_teardown` have V8 flags of their own,
//! so they are binaries of their own.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod cppgc_traced;
mod cpu_profiles;
mod devtools_session;
mod gc_platform;
mod heap_snapshot;
mod isolate_state;
mod modules;
mod promise_resolvers;
mod stack_traces;
mod unified_heap;
mod v8_smoke;
mod v8_text;

// `bindings` initialises V8 itself, without flags, for a `V8PerIsolateData`:
// whichever test came first would decide them.
::bindings::initialize_v8_before_main!("--expose-gc");

/// Whether this run's V8 starts GCs at its own times (one of the stress
/// flags in `MACHOPO_V8_FLAGS` that `test.py`'s matrix and fuzzer set: a
/// marking almost all the time, compaction at every GC, GCs at allocation
/// counts): a scavenge then often runs while a marking does and keeps the
/// wrappers it reached, unmodified or not, and a test of the scavenge
/// dropping one has nothing to test.
pub fn v8_gcs_at_its_own_times() -> bool {
    std::env::var("MACHOPO_V8_FLAGS").is_ok_and(|flags| {
        [
            "--stress-incremental-marking",
            "--stress-compaction",
            "--gc-interval",
            "--random-gc-interval",
        ]
        .iter()
        .any(|flag| {
            flags
                .split_whitespace()
                .any(|given| given.starts_with(flag))
        })
    })
}
