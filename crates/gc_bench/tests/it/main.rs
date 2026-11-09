//! `gc_bench`'s integration tests, one binary.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod workloads;

// V8's flags are the process's, set before the first isolate: the forced
// GC at the end of each workload needs `--expose-gc` on an isolate's heap.
// A pattern this small can end while V8 still compiles its functions on a
// worker (Sparkplug, Maglev or TurboFan), and the worker's allocation
// buffers (a page of code, trusted and old space each) count as used until
// it finishes, so a run here would read up to 768 KiB more after the forced
// GC than it keeps. The full-size runs finish compiling long before the
// end; `bench.py gc` keeps V8's defaults.
::bindings::initialize_v8_before_main!(
    "--expose-gc --no-concurrent-sparkplug --no-concurrent-recompilation --no-concurrent-osr"
);
