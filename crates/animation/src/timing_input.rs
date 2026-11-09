//! web-animations 1's "update the timing properties of an animation
//! effect" as script writes them (`animate()`'s options, `updateTiming()`),
//! the DOM's: the members given, checked, onto an effect's [`Timing`],
//! and how they were written ([`TimingInput`], which `getTiming()` gives
//! back).

use computed_style::{EasingFunction, FillMode, PlaybackDirection, PseudoId};

use crate::timing::Timing;

/// How script wrote an effect's timing: `fill: "auto"` (no fill, for a
/// keyframe effect), `duration: "auto"` (0), and its easing as written;
/// and its `pseudoElement`, which it keeps without a target.
#[derive(Debug, Clone, Default)]
pub struct TimingInput {
    pub fill_auto: bool,
    pub duration_auto: bool,
    pub easing: Option<String>,
    pub pseudo: Option<PseudoId>,
}

/// `duration` as written: a number of milliseconds, or a string (`"auto"`
/// the only one taken).
#[derive(Debug, Clone, PartialEq)]
pub enum DurationInput {
    Number(f64),
    Text(String),
}

/// `fill` as written: `FillMode`'s or `"auto"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillInput {
    Mode(FillMode),
    Auto,
}

/// `OptionalEffectTiming`: the members given.
#[derive(Debug, Clone, Default)]
pub struct OptionalTiming {
    pub delay: Option<f64>,
    pub direction: Option<PlaybackDirection>,
    pub duration: Option<DurationInput>,
    pub easing: Option<String>,
    pub end_delay: Option<f64>,
    pub fill: Option<FillInput>,
    pub iteration_start: Option<f64>,
    pub iterations: Option<f64>,
}

/// The timing a keyframe effect starts with: no delay, one iteration of 0
/// ms, linear, no fill.
pub fn default_timing() -> Timing {
    Timing {
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

/// `timing`'s members that are given, onto `out` and `input`; every check
/// first (a negative `iterationStart`, `iterations` or `duration`, a
/// `NaN`, a `duration` string but `"auto"`, an easing that isn't one: the
/// `TypeError`'s message), so nothing changes if one fails.
pub fn apply_timing(
    out: &mut Timing,
    input: &mut TimingInput,
    timing: &OptionalTiming,
) -> Result<(), &'static str> {
    if timing.iteration_start.is_some_and(|start| start < 0.0) {
        return Err("iterationStart can't be negative");
    }
    if timing.iterations.is_some_and(|n| n.is_nan() || n < 0.0) {
        return Err("iterations must be a number not below 0");
    }
    let duration = match &timing.duration {
        None => None,
        Some(DurationInput::Number(d)) if d.is_nan() || *d < 0.0 => {
            return Err("duration must be a number not below 0");
        }
        Some(DurationInput::Number(d)) => Some(Some(*d)),
        Some(DurationInput::Text(s)) if s == "auto" => Some(None),
        Some(DurationInput::Text(_)) => return Err("duration must be a number or auto"),
    };
    let easing = match &timing.easing {
        None => None,
        Some(text) => {
            Some(css::animation::parse_easing_text(text).ok_or("not an easing function")?)
        }
    };
    if let Some(delay) = timing.delay {
        out.delay = delay;
    }
    if let Some(end_delay) = timing.end_delay {
        out.end_delay = end_delay;
    }
    if let Some(fill) = timing.fill {
        input.fill_auto = fill == FillInput::Auto;
        out.fill = match fill {
            FillInput::Mode(mode) => mode,
            FillInput::Auto => FillMode::None,
        };
    }
    if let Some(start) = timing.iteration_start {
        out.iteration_start = start;
    }
    if let Some(iterations) = timing.iterations {
        out.iterations = iterations;
    }
    if let Some(duration) = duration {
        input.duration_auto = duration.is_none();
        out.duration = duration.unwrap_or(0.0);
    }
    if let Some(direction) = timing.direction {
        out.direction = direction;
    }
    if let Some((easing, text)) = easing {
        out.easing = easing;
        input.easing = Some(text);
    }
    Ok(())
}
