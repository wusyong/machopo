//! The click count of a mouse button's presses (`WebMouseEvent::
//! click_count`, which Chromium's platform layer fills in): presses of the
//! same button, each within the system's double-click interval of the one
//! before and near it, count up; `dblclick` is the second.

use std::time::{Duration, Instant};

use gfx::PointF;
use protocol::input::MouseButton;

/// How far a press may be from the one before, each way, and still count
/// as the next of the series: Chromium's `kDoubleClickWidth` (4 px) halved
/// (`ui::MouseEvent::IsRepeatedClickEvent`).
const DOUBLE_CLICK_SLOP: f32 = 2.0;

/// The system's double-click interval: what the user set on macOS
/// (`NSEvent.doubleClickInterval`); elsewhere Chromium's
/// `kDoubleClickTimeMs`, as on Linux without GTK (Windows'
/// `GetDoubleClickTime` would need `unsafe` here).
pub fn double_click_interval() -> Duration {
    #[cfg(target_os = "macos")]
    {
        let seconds = objc2_app_kit::NSEvent::doubleClickInterval();
        if seconds.is_finite() && seconds > 0.0 {
            return Duration::from_secs_f64(seconds);
        }
    }
    Duration::from_millis(500)
}

/// Counts the presses of a series (Chromium's `ui::MouseEvent::
/// GetRepeatCount`).
#[derive(Debug)]
pub struct ClickCounter {
    interval: Duration,
    last: Option<Press>,
}

#[derive(Debug, Clone, Copy)]
struct Press {
    at: Instant,
    point: PointF,
    button: MouseButton,
    count: u32,
}

impl ClickCounter {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
        }
    }

    /// `button` went down at `point` (the window's logical px) at `at`: the
    /// how-manyth press of its series it is.
    pub fn press(&mut self, at: Instant, point: PointF, button: MouseButton) -> u32 {
        let count = match self.last {
            Some(last)
                if last.button == button
                    && at.saturating_duration_since(last.at) <= self.interval
                    && (point.x - last.point.x).abs() <= DOUBLE_CLICK_SLOP
                    && (point.y - last.point.y).abs() <= DOUBLE_CLICK_SLOP =>
            {
                // A press of a series a click's `detail` counts: four
                // billion of them in a row stay at the most there is.
                last.count.saturating_add(1)
            }
            _ => 1,
        };
        self.last = Some(Press {
            at,
            point,
            button,
            count,
        });
        count
    }

    /// The next press starts a series, whenever it comes.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_millis(500);

    /// Presses of the same button close in time and place count up; a
    /// late one, a far one or another button's starts over.
    #[test]
    fn presses_count_up_in_a_series() {
        let start = Instant::now();
        let ms = |n| start + Duration::from_millis(n);
        let mut clicks = ClickCounter::new(INTERVAL);
        let at = PointF::new(10.0, 10.0);
        assert_eq!(clicks.press(ms(0), at, MouseButton::Left), 1);
        assert_eq!(
            clicks.press(ms(400), PointF::new(12.0, 8.0), MouseButton::Left),
            2
        );
        assert_eq!(clicks.press(ms(900), at, MouseButton::Left), 3);
        // Too late.
        assert_eq!(clicks.press(ms(1401), at, MouseButton::Left), 1);
        // Too far, either way.
        assert_eq!(
            clicks.press(ms(1500), PointF::new(12.5, 10.0), MouseButton::Left),
            1
        );
        assert_eq!(
            clicks.press(ms(1600), PointF::new(12.5, 12.6), MouseButton::Left),
            1
        );
        // Another button.
        assert_eq!(
            clicks.press(ms(1700), PointF::new(12.5, 12.6), MouseButton::Right),
            1
        );
        assert_eq!(
            clicks.press(ms(1800), PointF::new(12.5, 12.6), MouseButton::Right),
            2
        );
        clicks.reset();
        assert_eq!(
            clicks.press(ms(1900), PointF::new(12.5, 12.6), MouseButton::Right),
            1
        );
    }

    #[test]
    fn the_interval_is_the_systems() {
        let interval = double_click_interval();
        assert!(interval > Duration::ZERO && interval < Duration::from_secs(10));
    }
}
