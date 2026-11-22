//! Runs every scenario under `tests/scenarios/` (format in its README) in a
//! browser window without the platform's window (`shell::windowless`), or
//! with `--window` in the real one:
//!
//! ```text
//! python3 scripts/test.py scenarios [--window] [FILTER...]
//! MACHOPO_SLOW_TESTS=1 cargo test -p shell --test scenarios [-- [--window] FILTER...]
//! ```
//!
//! Without `MACHOPO_SLOW_TESTS` it does nothing: the scenarios wait in real
//! time, which a plain `cargo test --workspace` shouldn't.
//!
//! With `--window`, each scenario runs in a `machopo --automation-pipe
//! --window` of its own (winit opens one event loop a process), driven
//! through the pipe as machopodriver drives it, one at a time (the window
//! stays on top), with the system's keyboard shortcuts only. Its
//! screenshots, and machopo's log, go to `target/automation-results/<name>/`.
//! The window's scale is the display's, so `window scale=` is ignored; a
//! scenario with `window caret-blink=` is skipped, since the pipe has no
//! way to make the window's caret blink.
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
//! Without `--window`, the run fails on a scenario that fails unless
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
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossbeam_channel::{RecvTimeoutError, unbounded};
use protocol::automation::{Command, Message, Method};
use shell::automation::{self, Client, Context, Failure};
use shell::scenario::{self, Step};
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
    let window = std::env::args().skip(1).any(|a| a == "--window");
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
    if !filters.is_empty() {
        scenarios.retain(|path| {
            let name = path.file_stem().unwrap().to_string_lossy();
            filters.iter().any(|f| name.contains(f.as_str()))
        });
    }
    if window {
        return run_in_windows(&dir, &scenarios);
    }
    let results = workspace().join("target/scenario-results");
    let options = Options::default();
    let mut runs = Vec::new();
    for path in &scenarios {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
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
    outcomes: &[(usize, Result<(), Failure>)],
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

/// Runs `scenarios` one at a time, each in a real window of its own
/// ([`run_in_window`]), and judges the run as the windowless one is.
fn run_in_windows(dir: &Path, scenarios: &[PathBuf]) -> ExitCode {
    let results = workspace().join("target/automation-results");
    let mut runs = Vec::new();
    let mut outcomes = Vec::new();
    for path in scenarios {
        let name = path.file_stem().unwrap().to_string_lossy().into_owned();
        let out = results.join(&name);
        let _ = std::fs::remove_dir_all(&out);
        let start = Instant::now();
        let outcome = match read(path) {
            Ok(scenario) if scenario.window.caret_blink.is_some() => {
                println!("scenario {name} (window) ... skipped: it makes the caret blink");
                continue;
            }
            Ok(scenario) => run_in_window(&scenario, path, &out),
            Err(failure) => Err(failure),
        };
        let took = start.elapsed().as_secs();
        match &outcome {
            Ok(()) => println!("scenario {name} (window) ... ok ({took}s)"),
            Err(failure) => println!("scenario {name} (window) ... FAILED ({took}s): {failure}"),
        }
        outcomes.push((runs.len(), outcome));
        runs.push((name, path, "window", cfg!(target_os = "macos")));
    }
    let failed = outcomes.iter().filter(|(_, o)| o.is_err()).count();
    println!(
        "\n{} runs, {failed} failed; screenshots and logs in {}",
        outcomes.len(),
        results.display()
    );
    judge(dir, &runs, &outcomes)
}

/// The scenario in the file at `path`.
fn read(path: &Path) -> Result<scenario::Scenario, Failure> {
    let text = std::fs::read_to_string(path).map_err(|e| Failure {
        line: 0,
        message: format!("{}: {e}", path.display()),
    })?;
    scenario::parse(&text).map_err(|e| Failure {
        line: e.line,
        message: e.message,
    })
}

/// How long the window has to come up and answer, before the scenario's
/// own waits start.
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

/// How long machopo has to end once its input has, before it is killed.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

/// Runs `scenario` (the file at `path`) in `machopo --automation-pipe
/// --window`, as a client of its pipe: `protocol::automation`'s messages,
/// each followed by a NUL. Its screenshots and machopo's log go to `out`.
fn run_in_window(scenario: &scenario::Scenario, path: &Path, out: &Path) -> Result<(), Failure> {
    let failure = |message: String| Failure { line: 0, message };
    std::fs::create_dir_all(out).map_err(|e| failure(format!("{}: {e}", out.display())))?;
    let log = File::create(out.join("machopo.log"))
        .map_err(|e| failure(format!("{}: {e}", out.display())))?;
    let mut child = Process::new(env!("CARGO_BIN_EXE_machopo"))
        .args(["--automation-pipe", "--window"])
        // Each tab's renderer in a process of its own, when the run asks
        // (`test.py scenarios --window --process-per-tab`).
        .args(std::env::var_os("MACHOPO_PROCESS_PER_TAB").map(|_| "--process-per-tab"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(log)
        .spawn()
        .map_err(|e| failure(format!("starting machopo: {e}")))?;
    let mut input = child.stdin.take().expect("piped");
    let output = child.stdout.take().expect("piped");
    // The writer and the reader are threads of their own: the runner's
    // `Client` sends from wherever it runs, and waits on a channel.
    let (commands, to_write) = unbounded::<Command>();
    let writer = std::thread::spawn(move || {
        for command in to_write {
            let mut bytes = command.to_json().into_bytes();
            bytes.push(0);
            if input
                .write_all(&bytes)
                .and_then(|()| input.flush())
                .is_err()
            {
                break;
            }
        }
        // `input` drops here: machopo's input ends, and so does machopo.
    });
    let (sent, messages) = unbounded::<Message>();
    std::thread::spawn(move || {
        let mut output = BufReader::new(output);
        let mut bytes = Vec::new();
        loop {
            bytes.clear();
            match output.read_until(0, &mut bytes) {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
            if bytes.last() == Some(&0) {
                bytes.pop();
            }
            // Messages the runner doesn't know (a page's own, from V8's
            // inspector) are passed over, as the runner passes over events.
            if let Some(message) = Message::from_json(&String::from_utf8_lossy(&bytes))
                && sent.send(message).is_err()
            {
                break;
            }
        }
    });
    // Once the window is up, machopo answers: the scenario's waits count
    // from then, as they do in a window that is up when they start.
    let up = commands.send(Command {
        id: 0,
        session_id: None,
        method: Method::BrowserGetVersion,
    });
    let mut result = up
        .map_err(|_| failure("machopo has gone".into()))
        .and_then(|()| {
            let deadline = Instant::now() + STARTUP_TIMEOUT;
            loop {
                match messages.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                    Ok(Message::Response(response)) if response.id == 0 => break Ok(()),
                    Ok(_) => {}
                    Err(RecvTimeoutError::Timeout) => {
                        break Err(failure("the window didn't come up".into()));
                    }
                    Err(RecvTimeoutError::Disconnected) => {
                        break Err(failure("machopo ended before its window came up".into()));
                    }
                }
            }
        });
    if result.is_ok() {
        // The window's size, as the windowless runner makes its window;
        // its scale is the display's.
        let mut steps = Vec::new();
        if let Some((width, height)) = scenario.window.size {
            steps.push(Step {
                line: 0,
                command: scenario::Command::Resize { width, height },
            });
        }
        steps.extend(scenario.steps.iter().cloned());
        let context = Context {
            base: path.parent().unwrap_or(Path::new(".")).to_owned(),
            out: Some(out.to_owned()),
            mac: cfg!(target_os = "macos"),
        };
        let client = Client::new(
            Box::new(move |command| commands.send(command).is_ok()),
            messages,
        );
        result = automation::run(&steps, &context, &client);
    } else {
        drop(commands);
    }
    // The client, and with it the writer's channel, has gone: machopo's
    // input ends.
    let _ = writer.join();
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() || result.is_err() => break,
            Ok(Some(status)) => {
                result = Err(failure(format!("machopo ended with {status}")));
                break;
            }
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                if result.is_ok() {
                    result = Err(failure("machopo didn't end once its input had".into()));
                }
                break;
            }
        }
    }
    result
}
