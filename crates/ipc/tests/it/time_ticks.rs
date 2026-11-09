//! `TimeTicks`: the clock goes forward with `Instant`, and another process
//! reads the same clock.

use std::process::Command;
use std::time::{Duration, Instant};

use ipc::TimeTicks;

/// Set for the child process `another_process_reads_the_same_clock`
/// starts: it prints its reading instead of testing.
const PRINT_TICKS: &str = "MACHOPO_IPC_PRINT_TICKS";

#[test]
fn the_clock_goes_forward_as_time_does() {
    let (ticks, instant) = (TimeTicks::now(), Instant::now());
    std::thread::sleep(Duration::from_millis(50));
    let (later, elapsed) = (TimeTicks::now(), instant.elapsed());
    let measured = later.saturating_since(ticks);
    assert!(measured >= Duration::from_millis(50), "{measured:?}");
    // Read around the `Instant`s: within a few milliseconds of them.
    assert!(
        measured.abs_diff(elapsed) < Duration::from_millis(5),
        "{measured:?} vs {elapsed:?}"
    );
    assert_eq!(ticks.saturating_since(later), Duration::ZERO);
    assert!(
        (later.as_secs_f64() - ticks.as_secs_f64() - measured.as_secs_f64()).abs() < 1e-6,
        "seconds"
    );
}

#[test]
fn a_reading_moves_by_a_duration() {
    let ticks = TimeTicks::now();
    let later = ticks.checked_add(Duration::from_millis(7)).unwrap();
    assert_eq!(later.saturating_since(ticks), Duration::from_millis(7));
}

#[test]
fn a_reading_crosses_as_it_is() {
    let ticks = TimeTicks::now();
    let json = serde_json::to_string(&ticks).unwrap();
    assert_eq!(serde_json::from_str::<TimeTicks>(&json).unwrap(), ticks);
}

/// A child process's reading falls between two of this process's: the
/// processes read one clock.
#[test]
fn another_process_reads_the_same_clock() {
    if std::env::var_os(PRINT_TICKS).is_some() {
        println!(
            "ticks={}",
            serde_json::to_string(&TimeTicks::now()).unwrap()
        );
        return;
    }
    let before = TimeTicks::now();
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "time_ticks::another_process_reads_the_same_clock",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(PRINT_TICKS, "1")
        .output()
        .unwrap();
    let after = TimeTicks::now();
    let stdout = String::from_utf8(output.stdout).unwrap();
    // The harness prints the test's name on the same line.
    let json = stdout
        .split_once("ticks=")
        .and_then(|(_, rest)| rest.lines().next())
        .unwrap_or_else(|| panic!("the child prints its reading: {stdout}"));
    let child: TimeTicks = serde_json::from_str(json).unwrap();
    assert!(
        before <= child && child <= after,
        "{before:?} <= {child:?} <= {after:?}"
    );
}
