//! The animation properties' computed values: easing functions
//! (css-easing 2), the `transition-*` lists (css-transitions 1) and the
//! `animation-*` lists (css-animations 1), with how an easing function
//! maps an input progress to an output one.

use std::sync::Arc;

/// `<easing-function>`, computed. The keywords stay keywords, as Chrome
/// writes them back (`ease`, not its `cubic-bezier()`).
#[derive(Debug, Clone, PartialEq)]
pub enum EasingFunction {
    Linear,
    Ease,
    EaseIn,
    EaseOut,
    EaseInOut,
    /// `cubic-bezier(x1, y1, x2, y2)`, `x1` and `x2` in 0 to 1.
    CubicBezier([f64; 4]),
    /// `steps(n, position)`: `n` is at least 1 (2 for `jump-none`).
    Steps(u32, StepPosition),
    /// `linear()`'s control points, each input filled in (css-easing 2,
    /// "create a linear easing function"): ascending, the first and the
    /// last at least two.
    LinearPoints(Arc<[LinearStop]>),
}

/// Where `steps()` jumps. `start` and `end` are `jump-start` and
/// `jump-end`, kept apart because Chrome writes each as it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepPosition {
    JumpStart,
    JumpEnd,
    JumpNone,
    JumpBoth,
    Start,
    End,
}

/// A `linear()` control point: the output progress at an input progress
/// (a fraction: `50%` is 0.5). In `f64`, as the progress it is compared
/// with: `20%` in `f32` is past a progress of 0.2 (Blink keeps doubles).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearStop {
    pub output: f64,
    pub input: f64,
}

impl EasingFunction {
    /// The output progress for `input` (css-easing 2). `before` is the
    /// before flag: the input comes from before the effect's active
    /// interval, which only matters to `steps()` at a jump.
    pub fn evaluate(&self, input: f64, before: bool) -> f64 {
        match self {
            Self::Linear => input,
            Self::Ease => cubic_bezier([0.25, 0.1, 0.25, 1.0], input),
            Self::EaseIn => cubic_bezier([0.42, 0.0, 1.0, 1.0], input),
            Self::EaseOut => cubic_bezier([0.0, 0.0, 0.58, 1.0], input),
            Self::EaseInOut => cubic_bezier([0.42, 0.0, 0.58, 1.0], input),
            Self::CubicBezier(points) => cubic_bezier(*points, input),
            Self::Steps(steps, position) => step(*steps, *position, input, before),
            Self::LinearPoints(points) => linear(points, input),
        }
    }
}

/// css-easing 2, "the step easing functions".
fn step(steps: u32, position: StepPosition, input: f64, before: bool) -> f64 {
    use StepPosition::*;
    let steps = f64::from(steps);
    let mut current = (input * steps).floor();
    if matches!(position, JumpStart | Start | JumpBoth) {
        current += 1.0;
    }
    if before && (input * steps).fract() == 0.0 {
        current -= 1.0;
    }
    if input >= 0.0 && current < 0.0 {
        current = 0.0;
    }
    let jumps = match position {
        JumpBoth => steps + 1.0,
        JumpNone => steps - 1.0,
        _ => steps,
    };
    if input <= 1.0 && current > jumps {
        current = jumps;
    }
    current / jumps
}

/// css-easing 2, "the linear easing function": between the two points
/// around `input`, the first or the last two extended outside them.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "`linear()` has two points at least, and start <= last - 1"
)]
fn linear(points: &[LinearStop], input: f64) -> f64 {
    let at = |i: usize| (points[i].input, points[i].output);
    let last = points.len() - 1;
    // The last point whose input is at most `input` starts the segment;
    // past the ends, the first or the last segment goes on.
    let start = points
        .iter()
        .rposition(|p| p.input <= input)
        .unwrap_or(0)
        .min(last - 1);
    let (x0, y0) = at(start);
    let (x1, y1) = at(start + 1);
    if x1 == x0 {
        return y1;
    }
    y0 + (y1 - y0) * (input - x0) / (x1 - x0)
}

/// The y of a cubic Bézier from (0, 0) to (1, 1) at the x `input`
/// (Chromium's `gfx::CubicBezier`): `t` found by Newton's method, then by
/// bisection; outside 0 to 1, the tangent at the nearer end goes on.
fn cubic_bezier([x1, y1, x2, y2]: [f64; 4], input: f64) -> f64 {
    let curve = |a: f64, b: f64, t: f64| {
        // B(t) = 3(1-t)²t·a + 3(1-t)t²·b + t³
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let slope_x = |t: f64| {
        let u = 1.0 - t;
        3.0 * u * u * x1 + 6.0 * u * t * (x2 - x1) + 3.0 * t * t * (1.0 - x2)
    };
    if input < 0.0 {
        // The tangent at the start: from the first control point, or the
        // second if the first is at the start; both there, a straight line
        // (Blink's `CubicBezier::InitGradients`).
        let slope = if x1 > 0.0 {
            y1 / x1
        } else if y1 == 0.0 && x2 > 0.0 {
            y2 / x2
        } else if y1 == 0.0 && y2 == 0.0 {
            1.0
        } else {
            0.0
        };
        return slope * input;
    }
    if input > 1.0 {
        let slope = if x2 < 1.0 {
            (y2 - 1.0) / (x2 - 1.0)
        } else if y2 == 1.0 && x1 < 1.0 {
            (y1 - 1.0) / (x1 - 1.0)
        } else if y2 == 1.0 && y1 == 1.0 {
            1.0
        } else {
            0.0
        };
        return 1.0 + slope * (input - 1.0);
    }
    const EPSILON: f64 = 1e-7;
    let mut t = input;
    for _ in 0..8 {
        let error = curve(x1, x2, t) - input;
        if error.abs() < EPSILON {
            return curve(y1, y2, t);
        }
        let slope = slope_x(t);
        if slope.abs() < 1e-6 {
            break;
        }
        t -= error / slope;
    }
    let (mut low, mut high) = (0.0, 1.0);
    t = input;
    while low < high {
        let x = curve(x1, x2, t);
        if (x - input).abs() < EPSILON {
            break;
        }
        if input > x {
            low = t;
        } else {
            high = t;
        }
        t = (high - low) / 2.0 + low;
        if high - low < EPSILON {
            break;
        }
    }
    curve(y1, y2, t)
}

/// One item of `transition-property`.
#[derive(Debug, Clone, PartialEq)]
pub enum TransitionProperty {
    /// `none`, only ever the whole list.
    None,
    All,
    /// A property's name as written: one machopo doesn't have, or none at all,
    /// is kept (and serialized), and transitions nothing.
    Name(Arc<str>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionBehavior {
    Normal,
    AllowDiscrete,
}

/// The `transition-*` lists, each as declared (at least one item): there
/// are as many transitions as `transition-property` has names, the other
/// lists repeated or cut to match (css-transitions 1, Blink's
/// `CSSTransitionData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Transitions {
    pub property: Arc<[TransitionProperty]>,
    /// In seconds, not negative.
    pub duration: Arc<[f32]>,
    pub timing_function: Arc<[EasingFunction]>,
    /// In seconds.
    pub delay: Arc<[f32]>,
    pub behavior: Arc<[TransitionBehavior]>,
}

impl Transitions {
    pub fn initial() -> Self {
        Self {
            property: Arc::new([TransitionProperty::All]),
            duration: Arc::new([0.0]),
            timing_function: Arc::new([EasingFunction::Ease]),
            delay: Arc::new([0.0]),
            behavior: Arc::new([TransitionBehavior::Normal]),
        }
    }

    /// The `i`th transition's values but its property: each list repeated
    /// to reach `i`.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "each list has an item at least"
    )]
    pub fn timing(&self, i: usize) -> (f32, &EasingFunction, f32, TransitionBehavior) {
        (
            self.duration[i % self.duration.len()],
            &self.timing_function[i % self.timing_function.len()],
            self.delay[i % self.delay.len()],
            self.behavior[i % self.behavior.len()],
        )
    }
}

/// One item of `animation-name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnimationName {
    /// `none`: the animation runs no keyframes (its other values still
    /// line up with the other lists).
    None,
    /// A `<keyframes-name>`, an identifier or a string: `"none"` is a
    /// name.
    Name(Arc<str>),
}

/// `fill-mode`, `animation-fill-mode`: whether the effect applies before
/// and after its active interval (Blink's `Timing::FillMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillMode {
    None,
    Forwards,
    Backwards,
    Both,
}

/// `direction`, `animation-direction`: which way each iteration plays
/// (Blink's `Timing::PlaybackDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackDirection {
    Normal,
    Reverse,
    Alternate,
    AlternateReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationPlayState {
    Running,
    Paused,
}

/// The `animation-*` lists, each as declared (at least one item): there
/// are as many animations as `animation-name` has items, the other lists
/// repeated or cut to match (css-animations 1, Blink's
/// `CSSAnimationData`).
#[derive(Debug, Clone, PartialEq)]
pub struct Animations {
    pub name: Arc<[AnimationName]>,
    /// In seconds, not negative; `auto` is 0 (all machopo's animations are on
    /// the document timeline).
    pub duration: Arc<[f32]>,
    pub timing_function: Arc<[EasingFunction]>,
    /// In seconds.
    pub delay: Arc<[f32]>,
    /// Not negative; `infinite` is infinity.
    pub iteration_count: Arc<[f32]>,
    pub direction: Arc<[PlaybackDirection]>,
    pub fill_mode: Arc<[FillMode]>,
    pub play_state: Arc<[AnimationPlayState]>,
    /// Parsed only, with the value machopo's animations follow (`auto`, the
    /// document's timeline): machopo has no scroll-driven animations.
    pub timeline: Arc<[AnimationTimeline]>,
    pub range_start: Arc<[AnimationRangeBoundary]>,
    pub range_end: Arc<[AnimationRangeBoundary]>,
}

/// `animation-timeline`'s values machopo has: `auto`, the document's timeline
/// (`none`, a named or a scroll or view timeline it hasn't).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationTimeline {
    Auto,
}

/// `animation-range-start`'s and `-end`'s values machopo has: `normal`, the
/// whole timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnimationRangeBoundary {
    Normal,
}

/// The `i`th animation's values but its name, each list repeated to reach
/// it.
#[derive(Debug, Clone, PartialEq)]
pub struct AnimationTiming<'a> {
    pub duration: f32,
    pub timing_function: &'a EasingFunction,
    pub delay: f32,
    pub iteration_count: f32,
    pub direction: PlaybackDirection,
    pub fill_mode: FillMode,
    pub play_state: AnimationPlayState,
}

impl Animations {
    pub fn initial() -> Self {
        Self {
            name: Arc::new([AnimationName::None]),
            duration: Arc::new([0.0]),
            timing_function: Arc::new([EasingFunction::Ease]),
            delay: Arc::new([0.0]),
            iteration_count: Arc::new([1.0]),
            direction: Arc::new([PlaybackDirection::Normal]),
            fill_mode: Arc::new([FillMode::None]),
            play_state: Arc::new([AnimationPlayState::Running]),
            timeline: Arc::new([AnimationTimeline::Auto]),
            range_start: Arc::new([AnimationRangeBoundary::Normal]),
            range_end: Arc::new([AnimationRangeBoundary::Normal]),
        }
    }

    pub fn timing(&self, i: usize) -> AnimationTiming<'_> {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "each list has an item at least"
        )]
        fn at<T>(list: &[T], i: usize) -> &T {
            &list[i % list.len()]
        }
        AnimationTiming {
            duration: *at(&self.duration, i),
            timing_function: at(&self.timing_function, i),
            delay: *at(&self.delay, i),
            iteration_count: *at(&self.iteration_count, i),
            direction: *at(&self.direction, i),
            fill_mode: *at(&self.fill_mode, i),
            play_state: *at(&self.play_state, i),
        }
    }
}
