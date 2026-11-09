//! The event loop's clock (HR-Time's monotonic clock in a renderer):
//! milliseconds since the renderer started, which its idle periods and the
//! documents' navigation timing count on.

use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Clock {
    start: Instant,
}

impl Clock {
    pub(crate) fn new() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// The time now, in milliseconds on this clock.
    pub(crate) fn now(&self) -> f64 {
        self.start.elapsed().as_secs_f64() * 1000.0
    }

    /// The time `ago` before now, on this clock: negative before its start
    /// (a navigation the browser began before this renderer was).
    pub(crate) fn before_now(&self, ago: Duration) -> f64 {
        self.now() - ago.as_secs_f64() * 1000.0
    }

    /// `instant` on this clock, negative before its start.
    pub(crate) fn at(&self, instant: Instant) -> f64 {
        match instant.checked_duration_since(self.start) {
            Some(after) => after.as_secs_f64() * 1000.0,
            None => -self.start.saturating_duration_since(instant).as_secs_f64() * 1000.0,
        }
    }

    /// The instant at `time` on this clock, before its start for a
    /// negative one; none if `time` isn't a number or an `Instant` can't
    /// hold it.
    pub(crate) fn instant_at(&self, time: f64) -> Option<Instant> {
        let since = Duration::try_from_secs_f64(time.abs() / 1000.0).ok()?;
        if time < 0.0 {
            self.start.checked_sub(since)
        } else {
            self.start.checked_add(since)
        }
    }

    /// When it started: its time is the milliseconds since.
    pub(crate) fn start(&self) -> Instant {
        self.start
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_before_and_after_the_start() {
        let clock = Clock::new();
        assert!((clock.at(clock.start) - 0.0).abs() < f64::EPSILON);
        let later = clock.start + Duration::from_millis(5);
        assert!((clock.at(later) - 5.0).abs() < 1e-6);
        if let Some(earlier) = clock.start.checked_sub(Duration::from_millis(7)) {
            assert!((clock.at(earlier) + 7.0).abs() < 1e-6);
        }
        let ago = clock.before_now(Duration::from_secs(1));
        assert!(ago < clock.now() - 999.0);
        assert_eq!(clock.instant_at(5.0), Some(later));
        if let Some(earlier) = clock.start.checked_sub(Duration::from_millis(7)) {
            assert_eq!(clock.instant_at(-7.0), Some(earlier));
        }
        assert_eq!(clock.instant_at(f64::NAN), None);
    }
}
