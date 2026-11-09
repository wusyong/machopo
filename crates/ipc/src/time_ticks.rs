//! The operating system's monotonic clock, which every process of the
//! machine reads alike (Chromium's `base::TimeTicks`).

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// A reading of the operating system's monotonic clock (Chromium's
/// `base::TimeTicks`). Unlike an `Instant`, whose value no process can
/// see, it crosses processes: readings taken in different processes of
/// the machine compare. It counts from no particular moment (on most
/// systems the boot), so only the time between two readings means
/// anything; DevTools' `MonotonicTime` is a reading in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct TimeTicks(Duration);

impl TimeTicks {
    /// The clock now.
    pub fn now() -> Self {
        Self(platform::now())
    }

    /// How long after `earlier` this reading is; zero if it isn't after
    /// it.
    pub fn saturating_since(self, earlier: TimeTicks) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// `duration` after this reading, if the clock goes that far.
    pub fn checked_add(self, duration: Duration) -> Option<Self> {
        self.0.checked_add(duration).map(Self)
    }

    /// The reading in seconds, as DevTools' `MonotonicTime`.
    pub fn as_secs_f64(self) -> f64 {
        self.0.as_secs_f64()
    }
}

/// `CLOCK_MONOTONIC`, which every process reads alike.
#[cfg(unix)]
mod platform {
    use std::time::Duration;

    pub(super) fn now() -> Duration {
        let now = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
        // A monotonic clock's seconds aren't negative, nor its nanoseconds
        // a second or more.
        Duration::new(
            u64::try_from(now.tv_sec).unwrap_or(0),
            u32::try_from(now.tv_nsec).unwrap_or(0),
        )
    }
}

/// `QueryPerformanceCounter`, which every process reads alike (Chromium's
/// `TimeTicks` on Windows).
#[cfg(windows)]
mod platform {
    use std::sync::OnceLock;
    use std::time::Duration;

    use windows_sys::Win32::System::Performance::{
        QueryPerformanceCounter, QueryPerformanceFrequency,
    };

    const NANOS_PER_SEC: u128 = 1_000_000_000;

    /// The counter's ticks a second, fixed at boot.
    fn frequency() -> u64 {
        static FREQUENCY: OnceLock<u64> = OnceLock::new();
        *FREQUENCY.get_or_init(|| {
            let mut frequency: i64 = 0;
            // SAFETY: the pointer is to a live `i64`, which the call only
            // writes. It can't fail on Windows XP and later.
            unsafe { QueryPerformanceFrequency(&mut frequency) };
            // Never zero, so the division below can't be by zero.
            u64::try_from(frequency).unwrap_or(1).max(1)
        })
    }

    pub(super) fn now() -> Duration {
        let frequency = frequency();
        let mut count: i64 = 0;
        // SAFETY: the pointer is to a live `i64`, which the call only
        // writes. It can't fail on Windows XP and later.
        unsafe { QueryPerformanceCounter(&mut count) };
        let count = u64::try_from(count).unwrap_or(0);
        let secs = count.checked_div(frequency).unwrap_or(0);
        let ticks = count.checked_rem(frequency).unwrap_or(0);
        // `ticks` is below `frequency`, so this is below a second.
        let nanos = u128::from(ticks)
            .checked_mul(NANOS_PER_SEC)
            .and_then(|n| n.checked_div(u128::from(frequency)))
            .unwrap_or(0);
        Duration::new(secs, u32::try_from(nanos).unwrap_or(0))
    }
}
