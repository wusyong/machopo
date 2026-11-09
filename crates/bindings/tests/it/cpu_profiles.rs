//! V8's `CpuProfiler` through the inspector's `Profiler`, as DevTools
//! uses it: a profile of what ran between `start` and `stop`, sampled
//! every interval asked for, telling the thread's waiting for its next
//! task from its being in one (`V8PerIsolateData::set_idle`).

use std::collections::HashMap;

use bindings::{ScriptState, V8PerIsolateData};
use serde_json::{Value, json};

use crate::devtools_session::Session;

/// Runs `name`, a function busy for `ms` ms, while profiling every
/// `interval_us` µs: the profile.
fn profile(
    data: &mut V8PerIsolateData,
    session: &mut Session,
    state: &ScriptState,
    name: &str,
    ms: u32,
    interval_us: u32,
) -> Value {
    session.call(
        data,
        "Profiler.setSamplingInterval",
        json!({ "interval": interval_us }),
    );
    session.call(data, "Profiler.start", json!({}));
    let script = format!(
        "function {name}() {{ const end = Date.now() + {ms}; let n = 0; while (Date.now() < end) n++; return n; }} {name}() > 0"
    );
    assert_eq!(
        state.evaluate_classic_script(data, &script).unwrap(),
        "true"
    );
    session.call(data, "Profiler.stop", json!({}))["profile"].clone()
}

fn samples(profile: &Value) -> usize {
    let samples = profile["samples"].as_array().unwrap().len();
    assert_eq!(profile["timeDeltas"].as_array().unwrap().len(), samples);
    samples
}

#[test]
fn samples_come_every_interval_with_the_functions_that_ran() {
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    let mut session = Session::attach(&mut data);
    session.call(&mut data, "Profiler.enable", json!({}));
    // 200 ms: about 10 samples 20 ms apart, about 400 0.5 ms apart; the
    // bounds leave room for a busy machine.
    let coarse = profile(&mut data, &mut session, &state, "coarse", 200, 20_000);
    let fine = profile(&mut data, &mut session, &state, "fine", 200, 500);
    for (profile, name) in [(&coarse, "coarse"), (&fine, "fine")] {
        assert!(
            profile["nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["callFrame"]["functionName"] == name),
            "{profile}"
        );
    }
    let (coarse, fine) = (samples(&coarse), samples(&fine));
    assert!(coarse <= 25, "{coarse} samples 20 ms apart in 200 ms");
    assert!(fine >= 100, "{fine} samples 0.5 ms apart in 200 ms");
}

/// The isolate goes, though the profile wasn't stopped.
#[test]
fn an_isolate_goes_while_profiling() {
    let mut data = V8PerIsolateData::new();
    let state = ScriptState::new(&mut data);
    let mut session = Session::attach(&mut data);
    session.call(&mut data, "Profiler.enable", json!({}));
    session.call(&mut data, "Profiler.start", json!({}));
    state
        .evaluate_classic_script(&mut data, "for (let i = 0; i < 1e6; i++) {} 0")
        .unwrap();
    drop(state);
    drop(data);
}

/// The thread marked idle is `(idle)` in the profile, not idle but out of
/// script `(program)`, as Blink marks it around each task.
#[test]
fn idle_and_busy_out_of_script() {
    let mut data = V8PerIsolateData::new();
    let mut session = Session::attach(&mut data);
    session.call(&mut data, "Profiler.enable", json!({}));
    session.call(
        &mut data,
        "Profiler.setSamplingInterval",
        json!({ "interval": 1000 }),
    );
    session.call(&mut data, "Profiler.start", json!({}));
    data.set_idle(false);
    std::thread::sleep(std::time::Duration::from_millis(100));
    data.set_idle(true);
    std::thread::sleep(std::time::Duration::from_millis(100));
    data.set_idle(false);
    std::thread::sleep(std::time::Duration::from_millis(100));
    let profile = session.call(&mut data, "Profiler.stop", json!({}))["profile"].clone();
    let names: HashMap<u64, &str> = profile["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|node| {
            (
                node["id"].as_u64().unwrap(),
                node["callFrame"]["functionName"].as_str().unwrap(),
            )
        })
        .collect();
    // The samples in time order, a run of the same state as one: about
    // 100 in each, 1 ms apart; a few others (the start's) aren't counted.
    let mut runs: Vec<(&str, usize)> = Vec::new();
    for sample in profile["samples"].as_array().unwrap() {
        let name = names[&sample.as_u64().unwrap()];
        match runs.last_mut() {
            Some((last, count)) if *last == name => *count += 1,
            _ => runs.push((name, 1)),
        }
    }
    let long: Vec<&str> = runs
        .iter()
        .filter(|(_, count)| *count >= 30)
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(long, ["(program)", "(idle)", "(program)"], "{runs:?}");
}
