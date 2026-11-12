//! What `bench.py calls` measures: a page's script (`MACHOPO_BENCH_SCRIPT`)
//! in a realm with the DOM's interface objects and `machopoDom` (or a
//! window's, with `MACHOPO_BENCH_WINDOW`), whose loop calls one member
//! some million times.

use crate::env::Env;

/// Runs `MACHOPO_BENCH_SCRIPT`; `bench.py calls` counts what each call costs.
#[test]
#[ignore = "bench.py calls runs it"]
fn bench_calls() {
    let source = std::env::var("MACHOPO_BENCH_SCRIPT").expect("MACHOPO_BENCH_SCRIPT");
    // A window's realm when asked for (`MACHOPO_BENCH_WINDOW`): what a
    // member needing one is measured in (a selection, an abort, a
    // traversal's filter).
    let mut env = match std::env::var_os("MACHOPO_BENCH_WINDOW") {
        Some(_) => Env::window("https://bench.test/"),
        None => Env::new(),
    };
    assert_eq!(env.eval(&source), "0");
}
