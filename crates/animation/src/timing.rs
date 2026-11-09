//! The timing model (web-animations 1, "Timing model"; Blink's `Timing`
//! and `TimingCalculations`): from an effect's local time to its phase,
//! active time and the progress its easing function makes of it. Pure
//! functions of the time; times are in milliseconds.

use computed_style::{EasingFunction, FillMode, PlaybackDirection};

/// A computed time in seconds (an `f32`) in milliseconds, as the decimal
/// it was written as: `0.1s` is 100 ms, not 100.0000015, as in Chrome,
/// which keeps times as doubles.
pub(crate) fn milliseconds(seconds: f32) -> f64 {
    seconds
        .to_string()
        .parse::<f64>()
        .unwrap_or(f64::from(seconds))
        * 1000.0
}

/// An effect's timing (web-animations 1's `EffectTiming`, resolved).
#[derive(Debug, Clone, PartialEq)]
pub struct Timing {
    pub delay: f64,
    pub end_delay: f64,
    pub fill: FillMode,
    pub iteration_start: f64,
    /// Not negative; may be infinite.
    pub iterations: f64,
    /// The iteration duration: not negative.
    pub duration: f64,
    pub direction: PlaybackDirection,
    pub easing: EasingFunction,
}

/// Which members of a CSS animation's timing script set
/// (`updateTiming()`, css-animations 2): style no longer changes those
/// (Blink's `Timing::AnimationTimingOverride`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimingOverrides {
    pub delay: bool,
    pub end_delay: bool,
    pub fill: bool,
    pub iteration_start: bool,
    pub iterations: bool,
    pub duration: bool,
    pub direction: bool,
    pub easing: bool,
}

impl TimingOverrides {
    /// Every member: script gave the animation another effect.
    pub const ALL: Self = Self {
        delay: true,
        end_delay: true,
        fill: true,
        iteration_start: true,
        iterations: true,
        duration: true,
        direction: true,
        easing: true,
    };

    /// The members either sets.
    pub fn union(self, other: Self) -> Self {
        Self {
            delay: self.delay || other.delay,
            end_delay: self.end_delay || other.end_delay,
            fill: self.fill || other.fill,
            iteration_start: self.iteration_start || other.iteration_start,
            iterations: self.iterations || other.iterations,
            duration: self.duration || other.duration,
            direction: self.direction || other.direction,
            easing: self.easing || other.easing,
        }
    }

    /// `style`'s timing, but for the members script set, which stay
    /// `current`'s.
    pub fn apply(&self, style: &Timing, current: &Timing) -> Timing {
        let pick = |overridden: bool| if overridden { current } else { style };
        Timing {
            delay: pick(self.delay).delay,
            end_delay: pick(self.end_delay).end_delay,
            fill: pick(self.fill).fill,
            iteration_start: pick(self.iteration_start).iteration_start,
            iterations: pick(self.iterations).iterations,
            duration: pick(self.duration).duration,
            direction: pick(self.direction).direction,
            easing: pick(self.easing).easing.clone(),
        }
    }
}

/// An effect's timing with nothing given (web-animations 1's defaults):
/// what an animation without an effect has.
impl Default for Timing {
    fn default() -> Self {
        Self {
            delay: 0.0,
            end_delay: 0.0,
            fill: FillMode::None,
            iteration_start: 0.0,
            iterations: 1.0,
            duration: 0.0,
            direction: PlaybackDirection::Normal,
            easing: EasingFunction::Linear,
        }
    }
}

/// Where the local time is against the active interval.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Before,
    Active,
    After,
}

/// What the timing comes to at one local time (web-animations 1's
/// `ComputedEffectTiming`, the part machopo uses).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ComputedTiming {
    pub phase: Phase,
    /// `None` when the effect isn't in effect (not active, and not filling).
    pub active_time: Option<f64>,
    /// The transformed progress: what the easing function made of the
    /// directed progress; `None` when not in effect.
    pub progress: Option<f64>,
    /// `None` when not in effect.
    pub current_iteration: Option<f64>,
    /// The progress comes from the start of the interval (web-animations
    /// 1's before flag: before it going forwards, after it going
    /// backwards), which a step easing at a jump goes by.
    pub before_flag: bool,
}

impl Timing {
    /// The iteration duration times the iterations (0 if either is).
    pub fn active_duration(&self) -> f64 {
        if self.duration == 0.0 || self.iterations == 0.0 {
            0.0
        } else {
            self.duration * self.iterations
        }
    }

    /// When the effect ends: after its delay, active duration and end
    /// delay, and not before 0.
    pub fn end_time(&self) -> f64 {
        (self.delay + self.active_duration() + self.end_delay).max(0.0)
    }

    /// The timing at `local_time` (`None`: the animation has no current
    /// time, and the effect isn't in effect). `backwards` is whether the
    /// animation plays backwards (a negative playback rate).
    pub fn compute(&self, local_time: Option<f64>, backwards: bool) -> ComputedTiming {
        self.compute_with(local_time, backwards, false)
    }

    /// The timing at `local_time` with web-animations 1's endpoint-inclusive
    /// phases (`commitStyles()`): at the end it plays towards, the effect
    /// is still active, so a finished one gives its last value.
    pub fn compute_inclusive(&self, local_time: Option<f64>, backwards: bool) -> ComputedTiming {
        self.compute_with(local_time, backwards, true)
    }

    fn compute_with(
        &self,
        local_time: Option<f64>,
        backwards: bool,
        inclusive: bool,
    ) -> ComputedTiming {
        let Some(local) = local_time else {
            return ComputedTiming {
                phase: Phase::Before,
                active_time: None,
                progress: None,
                current_iteration: None,
                before_flag: false,
            };
        };
        let active_duration = self.active_duration();
        let end = self.end_time();
        // "Animation effect phases and states".
        let before_active = self.delay.min(end).max(0.0);
        let active_after = (self.delay + active_duration).min(end).max(0.0);
        let phase = if local < before_active || (backwards && !inclusive && local == before_active)
        {
            Phase::Before
        } else if local > active_after || (!backwards && !inclusive && local == active_after) {
            Phase::After
        } else {
            Phase::Active
        };
        // "Calculating the active time".
        let active_time = match phase {
            Phase::Before => matches!(self.fill, FillMode::Backwards | FillMode::Both)
                .then(|| (local - self.delay).max(0.0)),
            Phase::Active => Some(local - self.delay),
            Phase::After => matches!(self.fill, FillMode::Forwards | FillMode::Both)
                .then(|| (local - self.delay).min(active_duration).max(0.0)),
        };
        let Some(active_time) = active_time else {
            return ComputedTiming {
                phase,
                active_time: None,
                progress: None,
                current_iteration: None,
                before_flag: false,
            };
        };
        // "Calculating the overall progress".
        let overall = if self.duration == 0.0 {
            match phase {
                Phase::Before => 0.0,
                _ => self.iterations,
            }
        } else {
            active_time / self.duration
        } + self.iteration_start;
        // "Calculating the simple iteration progress".
        let mut simple = if overall.is_infinite() {
            self.iteration_start % 1.0
        } else {
            overall % 1.0
        };
        if simple == 0.0
            && matches!(phase, Phase::Active | Phase::After)
            && active_time == active_duration
            && self.iterations != 0.0
        {
            simple = 1.0;
        }
        // "Calculating the current iteration".
        let current_iteration = if phase == Phase::After && self.iterations.is_infinite() {
            f64::INFINITY
        } else if simple == 1.0 {
            overall.floor() - 1.0
        } else {
            overall.floor()
        };
        // "Calculating the directed progress".
        let forwards = match self.direction {
            PlaybackDirection::Normal => true,
            PlaybackDirection::Reverse => false,
            PlaybackDirection::Alternate | PlaybackDirection::AlternateReverse => {
                let even = current_iteration.is_infinite() || current_iteration % 2.0 == 0.0;
                even == (self.direction == PlaybackDirection::Alternate)
            }
        };
        let directed = if forwards { simple } else { 1.0 - simple };
        // "Calculating the transformed progress": the before flag is set
        // when the progress comes from the start of the interval.
        let before_flag =
            (phase == Phase::Before && forwards) || (phase == Phase::After && !forwards);
        ComputedTiming {
            phase,
            active_time: Some(active_time),
            progress: Some(self.easing.evaluate(directed, before_flag)),
            current_iteration: Some(current_iteration),
            before_flag,
        }
    }
}
