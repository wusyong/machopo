//! The wall-clock time cookies are kept by (Chromium's `base::Time` as
//! `//net/cookies` uses it): when one was made, when it expires. Not
//! `std::time::SystemTime`: an expiry is often far out of the system's range
//! (`Max-Age=99999999999999999999`) or before it (`Max-Age=0` is "the
//! earliest representable time"), and the arithmetic on it saturates there
//! as Chromium's does, instead of failing.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Microseconds in a second.
const MICROS_PER_SECOND: i64 = 1_000_000;
/// Seconds in a day.
const SECONDS_PER_DAY: i64 = 86_400;

/// A point in UTC, in microseconds since the Unix epoch (Chromium's
/// `base::Time` counts them since 1601, with `Time::Min` and `Time::Max` at
/// the ends of its range: [`Time::MIN`] and [`Time::MAX`] here).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Time(i64);

impl Time {
    /// The earliest representable time (`Time::Min`): what `Max-Age=0`
    /// expires at.
    pub const MIN: Self = Self(i64::MIN);
    /// The latest representable time (`Time::Max`).
    pub const MAX: Self = Self(i64::MAX);
    /// The Unix epoch.
    pub const UNIX_EPOCH: Self = Self(0);

    /// Now, by the system's clock (`Time::Now`).
    pub fn now() -> Self {
        Self::from_system_time(SystemTime::now())
    }

    /// `time`, saturated at the ends of the range.
    pub fn from_system_time(time: SystemTime) -> Self {
        let micros = |duration: Duration| i64::try_from(duration.as_micros()).unwrap_or(i64::MAX);
        match time.duration_since(UNIX_EPOCH) {
            Ok(after) => Self(micros(after)),
            Err(before) => Self(micros(before.duration()).saturating_neg()),
        }
    }

    /// `micros` microseconds after the epoch (before it if negative).
    pub const fn from_unix_micros(micros: i64) -> Self {
        Self(micros)
    }

    /// `seconds` seconds after the epoch, saturated.
    pub const fn from_unix_seconds(seconds: i64) -> Self {
        Self(seconds.saturating_mul(MICROS_PER_SECOND))
    }

    /// Microseconds since the epoch (negative before it).
    pub const fn unix_micros(self) -> i64 {
        self.0
    }

    /// This time `seconds` seconds later (earlier if negative), saturated
    /// (`Time + base::Seconds`).
    pub const fn saturating_add_seconds(self, seconds: i64) -> Self {
        self.saturating_add_micros(seconds.saturating_mul(MICROS_PER_SECOND))
    }

    /// This time `days` days later, saturated (`Time + base::Days`).
    pub const fn saturating_add_days(self, days: i64) -> Self {
        self.saturating_add_seconds(days.saturating_mul(SECONDS_PER_DAY))
    }

    /// This time `micros` microseconds later (earlier if negative),
    /// saturated.
    pub const fn saturating_add_micros(self, micros: i64) -> Self {
        Self(self.0.saturating_add(micros))
    }

    /// Microseconds from `earlier` to this time, saturated (`Time - Time`,
    /// a `TimeDelta`).
    pub const fn saturating_micros_since(self, earlier: Self) -> i64 {
        self.0.saturating_sub(earlier.0)
    }

    /// The time at a UTC date and time of day, if it is one: `month` 1 to
    /// 12, `day` in that month (the 29th of February only in a leap year),
    /// `hour` 0 to 23, `minute` and `second` 0 to 59 (no leap second).
    /// Years 0 to 99999, the ones a cookie's date can name, in the
    /// proleptic Gregorian calendar (`Time::FromUTCExploded`, which also
    /// refuses a day the month doesn't have, by exploding the result back).
    pub(crate) fn from_utc(
        year: u32,
        month: u32,
        day: u32,
        hour: u32,
        minute: u32,
        second: u32,
    ) -> Option<Self> {
        if year > 99_999
            || !(1..=12).contains(&month)
            || day < 1
            || day > days_in_month(year, month)
            || hour > 23
            || minute > 59
            || second > 59
        {
            return None;
        }
        let days = days_from_civil(i64::from(year), i64::from(month), i64::from(day));
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "|days| < 2^26 for years up to 99999, and the time of day < 86400 s"
        )]
        let seconds = days * SECONDS_PER_DAY
            + i64::from(hour) * 3600
            + i64::from(minute) * 60
            + i64::from(second);
        Some(Self::from_unix_seconds(seconds))
    }
}

fn is_leap_year(year: u32) -> bool {
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

/// The days of `month` (1 to 12) in `year`.
fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 if is_leap_year(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a date of the proleptic Gregorian calendar
/// (Howard Hinnant's `days_from_civil`), for `month` 1 to 12 and `day` 1 to
/// 31.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "year is at most 99999 and month, day small: every value stays under 2^27"
)]
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    // March is the year's first month, so a leap day is its last.
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(Time::from_utc(1970, 1, 1, 0, 0, 0), Some(Time::UNIX_EPOCH));
        assert_eq!(
            Time::from_utc(2038, 1, 19, 3, 14, 7),
            Some(Time::from_unix_seconds(2_147_483_647))
        );
        assert_eq!(
            Time::from_utc(2000, 2, 29, 0, 0, 0),
            Some(Time::from_unix_seconds(951_782_400))
        );
        assert_eq!(
            Time::from_utc(1601, 1, 1, 0, 0, 0),
            Some(Time::from_unix_seconds(-11_644_473_600))
        );
        assert_eq!(Time::from_utc(1900, 2, 29, 0, 0, 0), None);
        assert_eq!(Time::from_utc(2021, 4, 31, 0, 0, 0), None);
        assert_eq!(Time::from_utc(2021, 1, 1, 0, 0, 60), None);
        assert_eq!(Time::from_utc(2021, 1, 0, 0, 0, 0), None);
        assert!(Time::from_utc(99_999, 12, 31, 23, 59, 59).is_some());
    }

    #[test]
    fn saturates() {
        assert_eq!(Time::MAX.saturating_add_seconds(1), Time::MAX);
        assert_eq!(Time::MIN.saturating_add_seconds(-1), Time::MIN);
        assert_eq!(Time::UNIX_EPOCH.saturating_add_seconds(i64::MAX), Time::MAX);
        assert_eq!(Time::MAX.saturating_micros_since(Time::MIN), i64::MAX);
    }
}
