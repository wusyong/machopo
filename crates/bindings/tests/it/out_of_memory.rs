//! A script that runs V8 out of memory ends its process at once, with
//! `bindings::OUT_OF_MEMORY_EXIT` and a line on stderr saying so: what the
//! browser tells a renderer that ran out by. The test binary runs again as
//! the process that runs out, under a small heap.

use std::io::Write as _;
use std::process::Command;

use bindings::{ScriptState, V8PerIsolateData};

/// Set in the test run that is to run out.
const RUNS_OUT: &str = "MACHOPO_TEST_RUNS_OUT_OF_MEMORY";

/// What the side that runs out leaves in stdout's buffer (no newline):
/// written out only if something of the process's ran as it ended.
const BUFFERED: &str = "still in stdout's buffer";

/// The side that runs out, when `RUNS_OUT` says so; without it (the test
/// run itself) it does nothing.
#[test]
fn child() {
    if std::env::var_os(RUNS_OUT).is_none() {
        return;
    }
    let _ = std::io::stdout().write_all(BUFFERED.as_bytes());
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    let _ = state.evaluate_classic_script(
        &mut data,
        "const held = []; for (;;) held.push(new Array(1 << 14).fill(held.length));",
    );
    // V8 let it go on: not what this is for.
    std::process::exit(1);
}

#[test]
fn a_script_that_runs_v8_out_of_its_heap_ends_its_process_saying_so() {
    let output = Command::new(std::env::current_exe().expect("the test's own path"))
        .args(["out_of_memory::child", "--exact", "--nocapture"])
        .env(RUNS_OUT, "1")
        // Out soon; and none of a stress run's flags.
        .env("MACHOPO_V8_FLAGS", "--max-old-space-size=16")
        .output()
        .expect("the test runs again");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.code(),
        Some(i32::from(bindings::OUT_OF_MEMORY_EXIT)),
        "{stderr}"
    );
    // What ran out, and where V8 says it did.
    let line = stderr
        .lines()
        .find_map(|line| line.strip_prefix("V8 ran out of memory: its heap's limit ("))
        .unwrap_or_else(|| panic!("{stderr}"));
    let location = line.strip_suffix(')').unwrap_or_else(|| panic!("{stderr}"));
    assert!(!location.is_empty(), "{stderr}");
    // At once: nothing buffered is written out, and the test harness's own
    // line after the test never comes.
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains(BUFFERED), "{stdout}");
    assert!(!stdout.contains("test result"), "{stdout}");
}
