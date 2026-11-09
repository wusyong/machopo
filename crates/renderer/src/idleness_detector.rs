//! When a document's network goes quiet (Blink's `IdlenessDetector`), for
//! DevTools' `networkAlmostIdle` and `networkIdle` lifecycle events, which
//! automation waits on (Puppeteer's `networkidle2` and `networkidle0`):
//! from `DOMContentLoaded` on, no more than 2 requests, then none, in
//! flight for 500 ms; each told once, at the moment the quiet began.
//!
//! Blink counts its document's fetcher's requests as they start and end;
//! the frame counts them as the render thread looks, after each task: a
//! request can't start and end within one.

use std::time::Duration;

use ipc::TimeTicks;

/// How long the network stays quiet to count (Blink's
/// `kNetworkQuietWindow`).
pub(crate) const QUIET_WINDOW: Duration = Duration::from_millis(500);

/// One document's.
#[derive(Debug, Default)]
pub(crate) struct IdlenessDetector {
    /// `DOMContentLoaded` fired: it watches.
    started: bool,
    /// Since when no more than 2 requests have been in flight, until
    /// `networkAlmostIdle` is told; `None` while more are.
    almost_quiet_since: Option<TimeTicks>,
    /// Since when none has, until `networkIdle` is told.
    quiet_since: Option<TimeTicks>,
    almost_idle: bool,
    idle: bool,
}

impl IdlenessDetector {
    /// `DOMContentLoaded` fired (Blink's `DomContentLoadedEventFired`).
    pub(crate) fn start(&mut self) {
        self.started = true;
    }

    /// `active` requests are in flight at `now`: the events due, each with
    /// when its quiet began.
    pub(crate) fn update(
        &mut self,
        active: usize,
        now: TimeTicks,
    ) -> Vec<(&'static str, TimeTicks)> {
        let mut due = Vec::new();
        if !self.started {
            return due;
        }
        if !self.almost_idle {
            let since = self.almost_quiet_since.filter(|_| active <= 2);
            self.almost_quiet_since = (active <= 2).then(|| since.unwrap_or(now));
            if let Some(since) = self.almost_quiet_since
                && now.saturating_since(since) >= QUIET_WINDOW
            {
                self.almost_idle = true;
                due.push(("networkAlmostIdle", since));
            }
        }
        if !self.idle {
            let since = self.quiet_since.filter(|_| active == 0);
            self.quiet_since = (active == 0).then(|| since.unwrap_or(now));
            if let Some(since) = self.quiet_since
                && now.saturating_since(since) >= QUIET_WINDOW
            {
                self.idle = true;
                due.push(("networkIdle", since));
            }
        }
        due
    }

    /// When the next event would be due, if the network stays as it is.
    pub(crate) fn next_due(&self) -> Option<TimeTicks> {
        let almost = self.almost_quiet_since.filter(|_| !self.almost_idle);
        let quiet = self.quiet_since.filter(|_| !self.idle);
        [almost, quiet]
            .into_iter()
            .flatten()
            .filter_map(|since| since.checked_add(QUIET_WINDOW))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(start: TimeTicks, ms: u64) -> TimeTicks {
        start
            .checked_add(Duration::from_millis(ms))
            .expect("in range")
    }

    /// Nothing before `DOMContentLoaded`; then each once its quiet has
    /// lasted the window, at when it began; a request in flight starts the
    /// quiet over.
    #[test]
    fn each_is_told_once_the_quiet_has_lasted() {
        let start = TimeTicks::now();
        let mut detector = IdlenessDetector::default();
        assert_eq!(detector.update(0, at(start, 0)), []);
        assert_eq!(detector.next_due(), None);
        detector.start();
        assert_eq!(detector.update(3, at(start, 0)), []);
        assert_eq!(detector.next_due(), None);
        assert_eq!(detector.update(1, at(start, 100)), []);
        assert_eq!(detector.next_due(), Some(at(start, 600)));
        assert_eq!(
            detector.update(1, at(start, 600)),
            [("networkAlmostIdle", at(start, 100))]
        );
        assert_eq!(detector.update(0, at(start, 700)), []);
        // A new request: the quiet starts over.
        assert_eq!(detector.update(1, at(start, 900)), []);
        assert_eq!(detector.update(0, at(start, 1000)), []);
        assert_eq!(detector.next_due(), Some(at(start, 1500)));
        assert_eq!(detector.update(0, at(start, 1499)), []);
        assert_eq!(
            detector.update(0, at(start, 1500)),
            [("networkIdle", at(start, 1000))]
        );
        assert_eq!(detector.next_due(), None);
        assert_eq!(detector.update(5, at(start, 2000)), []);
        assert_eq!(detector.update(0, at(start, 3000)), []);
    }

    /// Quiet from the start: both at once.
    #[test]
    fn a_quiet_network_is_both() {
        let start = TimeTicks::now();
        let mut detector = IdlenessDetector::default();
        detector.start();
        assert_eq!(detector.update(0, at(start, 0)), []);
        assert_eq!(
            detector.update(0, at(start, 500)),
            [
                ("networkAlmostIdle", at(start, 0)),
                ("networkIdle", at(start, 0))
            ]
        );
    }
}
