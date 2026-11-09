//! `machopo --run-web-tests`: testharness.js tests run one after another for
//! a driver (`shell`'s tests drive it), as content_shell's
//! `--run-web-tests` runs them for Chromium's blinkpy. Each line of the
//! input is a test, its timeout in milliseconds and its URL; each is loaded
//! headless in a tab of its own (a renderer of its own), and its record is
//! a line of JSON on the output: its URL, `OK` with what the page reported
//! (`testRunner.reportHarnessResults`: JSON if it parses, else the text),
//! `TIMEOUT` if nothing came by the timeout, or `ERROR` with why (the
//! renderer went).
//!
//! A line `screenshot TIMEOUT URL PATH` is a reftest's page instead: its
//! screenshot (`headless::screenshot`, `reftest-wait` waited for up to the
//! timeout) written to `PATH` as a PNG, and `OK`, `TIMEOUT`
//! (`reftest-wait` still on) or `ERROR`.

use std::io::{BufRead, Write};
use std::time::Duration;

use browser::NetworkContextParams;

use crate::headless;

/// Runs the tests `input` lists, each record written to `output` as it
/// ends (flushed: the driver reads them as they come). A line that isn't
/// a timeout and a URL is an `ERROR` record of its own.
pub fn run(
    network: &NetworkContextParams,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(screenshot) = line.strip_prefix("screenshot ") {
            let record = match screenshot_line(screenshot) {
                Some((timeout, url, path)) => {
                    run_screenshot(url, Duration::from_millis(timeout), path, network)
                }
                None => serde_json::json!({
                    "url": line,
                    "status": "ERROR",
                    "message": "a screenshot is a line: `screenshot`, its timeout in milliseconds, its URL, the PNG's path",
                }),
            };
            writeln!(output, "{record}")?;
            output.flush()?;
            continue;
        }
        let record = match line
            .split_once(' ')
            .and_then(|(timeout, url)| Some((timeout.parse::<u64>().ok()?, url.trim())))
        {
            Some((timeout, url)) => run_one(url, Duration::from_millis(timeout), network),
            None => serde_json::json!({
                "url": line,
                "status": "ERROR",
                "message": "a test is a line: its timeout in milliseconds, a space, its URL",
            }),
        };
        writeln!(output, "{record}")?;
        output.flush()?;
    }
    Ok(())
}

fn run_one(url: &str, timeout: Duration, network: &NetworkContextParams) -> serde_json::Value {
    match headless::run_testharness(url, timeout, network.clone()) {
        Ok(text) => {
            let results = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
            serde_json::json!({ "url": url, "status": "OK", "results": results })
        }
        Err(error) if error == headless::NO_TESTHARNESS_RESULTS => {
            serde_json::json!({ "url": url, "status": "TIMEOUT" })
        }
        Err(error) => serde_json::json!({ "url": url, "status": "ERROR", "message": error }),
    }
}

/// A screenshot line's timeout, URL and path (which may have spaces).
fn screenshot_line(line: &str) -> Option<(u64, &str, &str)> {
    let (timeout, rest) = line.split_once(' ')?;
    let (url, path) = rest.trim().split_once(' ')?;
    Some((timeout.parse().ok()?, url, path.trim()))
}

fn run_screenshot(
    url: &str,
    timeout: Duration,
    path: &str,
    network: &NetworkContextParams,
) -> serde_json::Value {
    match headless::screenshot(url, network.clone(), timeout) {
        Ok(frame) => {
            match headless::write_png(std::path::Path::new(path), frame.size, &frame.pixels) {
                Ok(()) => serde_json::json!({ "url": url, "status": "OK" }),
                Err(error) => {
                    serde_json::json!({ "url": url, "status": "ERROR", "message": error.to_string() })
                }
            }
        }
        Err(error) if error.starts_with("reftest-wait was still on") => {
            serde_json::json!({ "url": url, "status": "TIMEOUT" })
        }
        Err(error) => serde_json::json!({ "url": url, "status": "ERROR", "message": error }),
    }
}
