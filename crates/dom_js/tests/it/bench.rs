//! What `bench.py calls` measures: a page's script (`MACHOPO_BENCH_SCRIPT`)
//! in a realm with the DOM's interface objects and `machopoDom`, whose
//! loop calls one member some million times.

use crate::env::Env;

/// Runs `MACHOPO_BENCH_SCRIPT`; `bench.py calls` counts what each call costs.
#[test]
#[ignore = "bench.py calls runs it"]
fn bench_calls() {
    let source = std::env::var("MACHOPO_BENCH_SCRIPT").expect("MACHOPO_BENCH_SCRIPT");
    let mut env = Env::new();
    assert_eq!(env.eval(&source), "0");
}
