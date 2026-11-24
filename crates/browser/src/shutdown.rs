//! How machopo ends: within what limits, and what it says when something
//! doesn't wind down within them. Each step of the end waits with a limit;
//! one that is passed lets what it waited for go (killed, or left to the
//! process's end) and says so, a line on standard error in one form and an
//! event in the trace, so that a log shows which step it was. An end that
//! winds down as it should says nothing.

use std::time::Duration;

/// How long machopo takes at most to end once told to (its window closed,
/// its pipe's input ended, `Browser.close`, a signal), however many tabs
/// and whatever their pages do: its renderers, then its own threads.
/// Chromium kills a child it can't wait for after as long
/// (`base::EnsureProcessTerminated`).
pub const LIMIT: Duration = Duration::from_secs(2);

/// How long the profile's databases have after [`LIMIT`] to write what
/// they were told and close: a slow disk's few writes, not a hung one's.
pub const PROFILE_LIMIT: Duration = Duration::from_secs(10);

/// What each line [`gave_up`] writes begins with.
pub const GAVE_UP: &str = "machopo: shutdown gave up on ";

/// The end let `what` go at `step`, after `waited`: a line on standard
/// error (not the log's, whose filter may leave it out), and an event in
/// the trace.
pub fn gave_up(what: &str, step: &str, waited: Duration) {
    let waited_ms = waited.as_millis();
    let _event = trace_event::info_event!("ShutdownGaveUp", what, step, waited_ms);
    eprintln!("{GAVE_UP}{what}, {step}, after {waited_ms} ms");
}
