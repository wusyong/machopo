//! The timing model (web-animations 1, "Timing model"): phases, the
//! active time, iterations and directions, and the before flag.

use animation::{FillMode, Phase, PlaybackDirection, Timing};
use computed_style::{EasingFunction, StepPosition};

fn timing(delay: f64, duration: f64, fill: FillMode) -> Timing {
    Timing {
        delay,
        end_delay: 0.0,
        fill,
        iteration_start: 0.0,
        iterations: 1.0,
        duration,
        direction: PlaybackDirection::Normal,
        easing: EasingFunction::Linear,
    }
}

#[test]
fn phases_and_the_active_time() {
    let t = timing(100.0, 1000.0, FillMode::None);
    assert_eq!(t.active_duration(), 1000.0);
    assert_eq!(t.end_time(), 1100.0);
    let at = |local: f64| t.compute(Some(local), false);
    assert_eq!(at(50.0).phase, Phase::Before);
    assert_eq!(at(50.0).active_time, None, "no fill");
    assert_eq!(
        at(100.0).phase,
        Phase::Active,
        "the interval starts at the delay"
    );
    assert_eq!(at(600.0).active_time, Some(500.0));
    assert_eq!(at(600.0).progress, Some(0.5));
    assert_eq!(at(1100.0).phase, Phase::After, "and ends at the end time");
    assert_eq!(at(1100.0).progress, None);
    // No current time: not in effect.
    assert_eq!(t.compute(None, false).progress, None);
    // Playing backwards, the boundaries go the other way.
    assert_eq!(t.compute(Some(100.0), true).phase, Phase::Before);
    assert_eq!(t.compute(Some(1100.0), true).phase, Phase::Active);
}

#[test]
fn fill_modes() {
    let backwards = timing(100.0, 1000.0, FillMode::Backwards);
    assert_eq!(backwards.compute(Some(0.0), false).progress, Some(0.0));
    assert_eq!(backwards.compute(Some(2000.0), false).progress, None);
    let forwards = timing(100.0, 1000.0, FillMode::Forwards);
    assert_eq!(forwards.compute(Some(0.0), false).progress, None);
    assert_eq!(forwards.compute(Some(2000.0), false).progress, Some(1.0));
    let both = timing(100.0, 1000.0, FillMode::Both);
    assert_eq!(both.compute(Some(0.0), false).progress, Some(0.0));
    assert_eq!(both.compute(Some(2000.0), false).progress, Some(1.0));
}

/// A negative delay starts partway through.
#[test]
fn negative_delays() {
    let t = timing(-500.0, 1000.0, FillMode::Backwards);
    assert_eq!(t.end_time(), 500.0);
    let now = t.compute(Some(0.0), false);
    assert_eq!(now.phase, Phase::Active);
    assert_eq!(now.progress, Some(0.5));
    // All of it before 0: done from the start.
    let t = timing(-2000.0, 1000.0, FillMode::Backwards);
    assert_eq!(t.end_time(), 0.0);
    assert_eq!(t.compute(Some(0.0), false).phase, Phase::After);
}

#[test]
fn iterations_and_directions() {
    let mut t = timing(0.0, 100.0, FillMode::Both);
    t.iterations = 3.0;
    let at = |t: &Timing, local: f64| t.compute(Some(local), false);
    assert_eq!(at(&t, 150.0).current_iteration, Some(1.0));
    assert_eq!(at(&t, 150.0).progress, Some(0.5));
    // The end of an iteration is its progress 1, not the next one's 0.
    assert_eq!(at(&t, 300.0).progress, Some(1.0));
    assert_eq!(at(&t, 300.0).current_iteration, Some(2.0));
    t.direction = PlaybackDirection::Reverse;
    assert_eq!(at(&t, 25.0).progress, Some(0.75));
    t.direction = PlaybackDirection::Alternate;
    assert_eq!(at(&t, 25.0).progress, Some(0.25));
    assert_eq!(
        at(&t, 125.0).progress,
        Some(0.75),
        "the second iteration backwards"
    );
    t.direction = PlaybackDirection::AlternateReverse;
    assert_eq!(at(&t, 25.0).progress, Some(0.75));
    assert_eq!(at(&t, 125.0).progress, Some(0.25));
    t.iteration_start = 0.5;
    t.direction = PlaybackDirection::Normal;
    assert_eq!(at(&t, 0.0).progress, Some(0.5));
    // Infinitely many: never after, before the end.
    t.iteration_start = 0.0;
    t.iterations = f64::INFINITY;
    assert_eq!(t.end_time(), f64::INFINITY);
    assert_eq!(at(&t, 1_000_050.0).phase, Phase::Active);
    assert_eq!(at(&t, 1_000_050.0).progress, Some(0.5));
}

/// A zero duration jumps to the end once active.
#[test]
fn zero_duration() {
    let t = timing(100.0, 0.0, FillMode::Both);
    assert_eq!(t.compute(Some(50.0), false).progress, Some(0.0));
    assert_eq!(t.compute(Some(100.0), false).phase, Phase::After);
    assert_eq!(t.compute(Some(100.0), false).progress, Some(1.0));
    // Before, it is at the start whatever the iterations.
    let mut t = timing(100.0, 0.0, FillMode::Both);
    t.iterations = 2.5;
    assert_eq!(t.compute(Some(50.0), false).progress, Some(0.0));
    assert_eq!(t.compute(Some(100.0), false).progress, Some(0.5));
    // Infinitely many of nothing is nothing.
    t.iterations = f64::INFINITY;
    assert_eq!(t.active_duration(), 0.0);
}

/// The easing function gets the before flag before the interval.
#[test]
fn the_easing_and_its_before_flag() {
    let mut t = timing(100.0, 1000.0, FillMode::Backwards);
    t.easing = EasingFunction::Steps(1, StepPosition::Start);
    assert_eq!(
        t.compute(Some(0.0), false).progress,
        Some(0.0),
        "before: the flag"
    );
    assert_eq!(
        t.compute(Some(100.0), false).progress,
        Some(1.0),
        "active: jumped"
    );
    t.easing = EasingFunction::EaseIn;
    let p = t.compute(Some(600.0), false).progress.unwrap();
    assert!((p - 0.315_357).abs() < 1e-4, "{p}");
}
