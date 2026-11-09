//! The animation model (`blink/renderer/core/animation`): the timing
//! model, CSS animations and CSS transitions. It knows styles, not nodes:
//! `dom` keeps each element's [`ElementAnimations`] (off the GC heap, by
//! node id), `style` starts and samples them as it styles elements (and
//! computes CSS animations' keyframes), and the renderer ticks them at
//! each frame and dispatches their events.
//!
//! Times are in milliseconds on the document timeline, whose time is the
//! frame's (what `requestAnimationFrame` callbacks get).

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

mod animation;
mod css_animation;
mod event;
mod keyframes;
mod playback;
mod timing;
mod timing_input;
mod transition;

pub use computed_style::{FillMode, PlaybackDirection};

pub use crate::animation::{Animation, CssAnimationOwner, Owner, ReplaceState, TransitionOwner};
pub use crate::css_animation::{
    CssAnimations, DeclaredKeyframe, DeclaredKeyframes, EffectFor, Keyframe, KeyframeEffect,
    KeyframeSource, KeyframesInput,
};
pub use crate::event::{
    AnimationEvent, CssAnimationEvent, CssAnimationEventKind, PlaybackEvent, TransitionEvent,
    TransitionEventKind,
};
pub use crate::keyframes::{
    KeyframeLike, OneOrList, ScriptKeyframes, is_animation_property, process_keyframes,
};
pub use crate::playback::{Notice, PendingTask, PlayState, Playback, PlaybackError};
pub use crate::timing::{ComputedTiming, Phase, Timing, TimingOverrides};
pub use crate::timing_input::{
    DurationInput, FillInput, OptionalTiming, TimingInput, apply_timing, default_timing,
};
pub use crate::transition::{AnimatedStyle, ElementAnimations};
