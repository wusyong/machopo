//! Runs every scenario under `tests/scenarios/` (format in its README) in a
//! browser window without the platform's window (`shell::windowless`):
//!
//! ```text
//! python3 scripts/test.py scenarios [FILTER...]
//! MACHOPO_SLOW_TESTS=1 cargo test -p shell --test scenarios [-- FILTER...]
//! ```
//!
//! Without `MACHOPO_SLOW_TESTS` it does nothing: the scenarios wait in real
//! time, which a plain `cargo test --workspace` shouldn't.
//!
//! Only scenarios whose file name contains one of the filters run. One that
//! presses keys or has a `platform` block runs twice, with macOS's keyboard
//! shortcuts and with the others', on any system; the rest, which would do
//! the same both ways, once, with the others'. Its screenshots go to
//! `target/scenario-results/<name>/mac/` and `other/`. A few run at once,
//! each in a window of its own: most of their time is waiting, and the
//! longest (`demo-script`, which waits for its clock) then sets the run's
//! length. Not more: each checks what happens in real time, and too many
//! renderers beside it would slow it down.
//!
//! The run fails on a scenario that fails unless
//! `tests/scenarios/known-failures.txt` names it, and on one it names that
//! passes now (to be taken off the list).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Mutex;

use shell::scenario;
use shell::windowless::{self, Options};

/// How many scenarios run at once.
const WORKERS: usize = 4;

fn workspace() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn main() -> ExitCode {
    if std::env::var_os("MACHOPO_SLOW_TESTS").is_none() {
        println!(
            "scenarios: skipped without MACHOPO_SLOW_TESTS (`python3 scripts/test.py scenarios` runs them)"
        );
        return ExitCode::SUCCESS;
    }
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| !a.starts_with('-'))
        .collect();
    let dir = workspace().join("tests/scenarios");
    let mut scenarios: Vec<PathBuf> = match std::fs::read_dir(&dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == "txt"))
            // The list of those known to fail, not a scenario.
            .filter(|p| p.file_stem().is_none_or(|stem| stem != KNOWN_FAILURES))
            .collect(),
        Err(e) => {
            eprintln!("scenarios: {}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
    };
    scenarios.sort();
    let results = workspace().join("target/scenario-results");
    let options = Options::default();
    let mut runs = Vec::new();
    for path in &scenarios {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        if !filters.is_empty() && !filters.iter().any(|f| name.contains(f.as_str())) {
            continue;
        }
        let _ = std::fs::remove_dir_all(results.join(&name));
        // One that can't be read or parsed runs once, to say why.
        let twice = std::fs::read_to_string(path)
            .ok()
            .and_then(|text| scenario::parse(&text).ok())
            .is_some_and(|scenario| scenario.depends_on_shortcuts());
        let platforms: &[_] = if twice {
            &[("mac", true), ("other", false)]
        } else {
            &[("other", false)]
        };
        for &(platform, mac) in platforms {
            runs.push((name.clone(), path, platform, mac));
        }
    }
    let queue = Mutex::new((0..runs.len()).collect::<VecDeque<_>>());
    let outcomes = Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                loop {
                    // Its own statement: a `while let` would hold the lock
                    // (a temporary of the condition) through the body, and
                    // run the scenarios one at a time.
                    let next = queue.lock().unwrap().pop_front();
                    let Some(index) = next else {
                        break;
                    };
                    let (name, path, platform, mac) = &runs[index];
                    let out = results.join(name).join(platform);
                    let outcome = windowless::run_file(path, Some(&out), *mac, &options);
                    outcomes.lock().unwrap().push((index, outcome));
                }
            });
        }
    });
    let mut outcomes = outcomes.into_inner().unwrap();
    outcomes.sort_by_key(|(index, _)| *index);
    let mut failed = 0;
    for (index, outcome) in &outcomes {
        let (name, _, platform, _) = &runs[*index];
        match outcome {
            Ok(()) => println!("scenario {name} ({platform}) ... ok"),
            Err(failure) => {
                failed += 1;
                println!("scenario {name} ({platform}) ... FAILED: {failure}");
            }
        }
    }
    println!("\n{} runs, {failed} failed", outcomes.len());
    judge(&dir, &runs, &outcomes)
}

/// The scenarios known to fail (`known-failures.txt`, without `.txt`).
const KNOWN_FAILURES: &str = "known-failures";

/// The run judged by `known-failures.txt` (`scenario::judge_runs`).
fn judge(
    dir: &Path,
    runs: &[(String, &PathBuf, &str, bool)],
    outcomes: &[(usize, Result<(), shell::automation::Failure>)],
) -> ExitCode {
    let list = match std::fs::read_to_string(dir.join(format!("{KNOWN_FAILURES}.txt"))) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("scenarios: {KNOWN_FAILURES}.txt: {e}");
            return ExitCode::FAILURE;
        }
    };
    let known = scenario::known_failures(&list);
    let names: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            let stem = path.file_stem()?.to_string_lossy().into_owned();
            (path.extension()? == "txt").then_some(stem)
        })
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let ran: Vec<(&str, bool)> = outcomes
        .iter()
        .map(|(index, outcome)| (runs[*index].0.as_str(), outcome.is_ok()))
        .collect();
    let problems = scenario::judge_runs(&names, &ran, &known);
    for problem in &problems {
        println!("{problem}");
    }
    if problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
