//! SIGTERM, SIGINT and SIGHUP end machopo as closing it does (Chromium's
//! `ShutdownDetector`, which posts the close of every browser window to
//! the UI thread): what it winds down within its limit, the profile and
//! the trace written. A second one ends it at once, as Chromium's handler
//! gives the signal its default action again after the first.

use std::cell::Cell;
use std::time::Instant;

/// Calls `end` (on a thread of its own) at the first of the signals from
/// now on, which is to end machopo as closing it does; at the second,
/// says so and ends the process at once, with the code a shell gives a
/// process a signal ended (128 and the signal's number).
pub fn listen(end: impl Fn() + Send + 'static) {
    // Called on the one thread the signals are read on.
    let first = Cell::new(None::<Instant>);
    let listening = ipc::platform::on_shutdown_signals(move |signal| match first.get() {
        None => {
            first.set(Some(Instant::now()));
            end();
        }
        Some(at) => {
            browser::shutdown::gave_up(
                "winding down",
                &format!("a second signal ({signal}), ending at once"),
                at.elapsed(),
            );
            ipc::platform::terminate_current_process_immediately(128_i32.saturating_add(signal));
        }
    });
    if let Err(error) = listening {
        tracing::warn!(%error, "SIGTERM, SIGINT and SIGHUP end machopo at once");
    }
}
