//! CSS animations (css-animations 1; Blink's
//! `CSSAnimations::CalculateAnimationUpdate`, `CSSAnimation` and its
//! `StringKeyframeEffectModel`): which animations a style change keeps,
//! starts, updates and cancels, and the values keyframes give a style at
//! a time (a transition's effect is two of them).
//!
//! An animation's keyframes come computed for its element (`style` cascades
//! each keyframe's declarations on it), each a whole style: a frame only
//! interpolates between the two around the animation's progress, for each
//! property a keyframe declares.

use std::collections::HashMap;
use std::sync::Arc;

use computed_style::{
    AnimationName, AnimationPlayState, AnimationTiming, ComputedStyle, EasingFunction,
};
use css::interpolate::{AnimationType, interpolate};
use css::properties::LonghandId;
use css::{CssPropertyValueSet, KeyframesRule};

use crate::animation::{Animation, CssAnimationOwner, Owner};
use crate::event::AnimationEvent;
use crate::timing::{Timing, TimingOverrides, milliseconds};

/// One keyframe as computed for an element, at one offset (a keyframe
/// with several selectors is one of these for each).
#[derive(Debug, Clone)]
pub struct Keyframe {
    /// From 0 to 1.
    pub offset: f64,
    /// The element's style with the keyframe's declarations.
    pub style: Arc<ComputedStyle>,
    /// The longhands it declares, as written (logical ones too).
    pub properties: Vec<LonghandId>,
    /// Its `animation-timing-function`, if it declares one: the easing
    /// from it to the next keyframe.
    pub easing: Option<EasingFunction>,
}

/// A keyframe script gave (web-animations 1's processed keyframe): its
/// offset (`None`: spaced between its neighbors), its easing, and its
/// declarations; with what was written, for `getKeyframes()`.
#[derive(Debug, Clone)]
pub struct DeclaredKeyframe {
    pub offset: Option<f64>,
    pub easing: EasingFunction,
    /// The easing as written.
    pub easing_text: String,
    /// Its `composite` as written (`auto`, or an operation: machopo composites
    /// each as `replace`).
    pub composite: String,
    pub block: Arc<CssPropertyValueSet>,
    /// The properties (their CSS names) and their values, serialized.
    pub values: Vec<(String, String)>,
}

/// An effect's keyframes as script gave them (Blink's
/// `StringKeyframeEffectModel` before it is resolved).
#[derive(Debug, Clone, Default)]
pub struct DeclaredKeyframes {
    pub keyframes: Vec<DeclaredKeyframe>,
}

/// A keyframe as computed (`KeyframeEffect::computed_keyframes`): its
/// offset, the easing from there, and its properties with the styles
/// their values are in.
pub type ComputedKeyframe = (f64, EasingFunction, Vec<(LonghandId, Arc<ComputedStyle>)>);

impl DeclaredKeyframes {
    /// Each keyframe's computed offset (web-animations 1, "compute missing
    /// keyframe offsets"): as given, the first 0 and the last 1 if not
    /// given (a single one at 1), the others spaced evenly between the
    /// ones around them that have one.
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "indices below n, which is above 0 past the first check; i < j < n"
    )]
    pub fn computed_offsets(&self) -> Vec<f64> {
        let n = self.keyframes.len();
        let mut offsets: Vec<Option<f64>> = self.keyframes.iter().map(|k| k.offset).collect();
        if n == 0 {
            return Vec::new();
        }
        if n > 1 && offsets[0].is_none() {
            offsets[0] = Some(0.0);
        }
        if offsets[n - 1].is_none() {
            offsets[n - 1] = Some(1.0);
        }
        let mut i = 0;
        while i < n {
            let Some(from) = offsets[i] else {
                i += 1;
                continue;
            };
            let next = (i + 1..n).find(|&j| offsets[j].is_some());
            if let Some(j) = next {
                let to = offsets[j].expect("found");
                for (k, offset) in offsets.iter_mut().enumerate().take(j).skip(i + 1) {
                    *offset = Some(from + (to - from) * (k - i) as f64 / (j - i) as f64);
                }
                i = j;
            } else {
                i += 1;
            }
        }
        offsets.into_iter().map(|o| o.unwrap_or(0.0)).collect()
    }
}

/// What an effect's keyframes are of: a `@keyframes` rule's, or script's.
#[derive(Clone)]
pub enum KeyframesInput {
    Rule(Arc<KeyframesRule>),
    Declared(Arc<DeclaredKeyframes>),
}

impl KeyframesInput {
    fn is(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Rule(a), Self::Rule(b)) => Arc::ptr_eq(a, b),
            (Self::Declared(a), Self::Declared(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

/// What an effect's keyframes were computed from: while all of it is the
/// same, they are too, and a style change keeps them.
#[derive(Clone)]
pub struct KeyframeSource {
    pub input: KeyframesInput,
    /// The element's style without animations, which the missing 0% and
    /// 100% keyframes are.
    pub base: Arc<ComputedStyle>,
    /// What the keyframes inherit from.
    pub parent: Arc<ComputedStyle>,
    /// What `rem` and the viewport units are of.
    pub root_font_size: f32,
    pub viewport: (f32, f32),
}

impl std::fmt::Debug for KeyframeSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match &self.input {
            KeyframesInput::Rule(rule) => rule.name.to_string(),
            KeyframesInput::Declared(_) => "(script's)".to_owned(),
        };
        f.debug_struct("KeyframeSource")
            .field("input", &name)
            .finish_non_exhaustive()
    }
}

impl KeyframeSource {
    /// Whether keyframes computed from `self` are those of the rest.
    pub fn is(
        &self,
        input: &KeyframesInput,
        base: &ComputedStyle,
        parent: &ComputedStyle,
        root_font_size: f32,
        viewport: (f32, f32),
    ) -> bool {
        self.input.is(input)
            && self.root_font_size == root_font_size
            && self.viewport == viewport
            && *self.base == *base
            && *self.parent == *parent
    }
}

/// A property's keyframe: where, the style its value is in, and the easing
/// to the next one.
#[derive(Debug, Clone)]
struct PropertyKeyframe {
    offset: f64,
    style: Arc<ComputedStyle>,
    easing: EasingFunction,
}

/// An animation's keyframes for one element (Blink's
/// `StringKeyframeEffectModel`, its values computed): for each property a
/// keyframe declares, its keyframes by offset, from 0 to 1.
#[derive(Debug, Clone)]
pub struct KeyframeEffect {
    /// What its keyframes were computed from; `None` for a transition's,
    /// and for script's not yet computed.
    pub source: Option<KeyframeSource>,
    /// Script's keyframes, which style computes on the target
    /// (`KeyframeEffect::declared`).
    pub declared: Option<Arc<DeclaredKeyframes>>,
    properties: Vec<(LonghandId, Vec<PropertyKeyframe>)>,
    /// A keyframe uses a viewport unit: the element's style does too.
    uses_viewport: bool,
}

impl KeyframeEffect {
    /// The effect of `keyframes`, in the order written. For each property,
    /// a later keyframe at the same offset wins (keyframes cascade,
    /// css-animations 1); without one at 0% or 100%, the base style is
    /// there, with `easing` (the animation's `animation-timing-function`),
    /// as is a keyframe's that declares none. A property that doesn't
    /// animate is left out.
    pub fn new(source: KeyframeSource, keyframes: &[Keyframe], easing: &EasingFunction) -> Self {
        let mut properties: Vec<(LonghandId, Vec<PropertyKeyframe>)> = Vec::new();
        for keyframe in keyframes {
            for id in &keyframe.properties {
                let id = id.physical();
                if id.animation_type() == AnimationType::NotAnimatable {
                    continue;
                }
                let frame = PropertyKeyframe {
                    offset: keyframe.offset,
                    style: keyframe.style.clone(),
                    easing: keyframe.easing.clone().unwrap_or_else(|| easing.clone()),
                };
                let frames = match properties.iter_mut().find(|(other, _)| *other == id) {
                    Some((_, frames)) => frames,
                    None => {
                        properties.push((id, Vec::new()));
                        &mut properties.last_mut().expect("just pushed").1
                    }
                };
                match frames.iter_mut().find(|f| f.offset == keyframe.offset) {
                    Some(same) => *same = frame,
                    None => frames.push(frame),
                }
            }
        }
        for (_, frames) in &mut properties {
            frames.sort_by(|a, b| a.offset.total_cmp(&b.offset));
            let neutral = |offset| PropertyKeyframe {
                offset,
                style: source.base.clone(),
                easing: easing.clone(),
            };
            if frames.first().is_none_or(|f| f.offset != 0.0) {
                frames.insert(0, neutral(0.0));
            }
            if frames.last().is_none_or(|f| f.offset != 1.0) {
                frames.push(neutral(1.0));
            }
        }
        let uses_viewport = keyframes.iter().any(|k| k.style.has_viewport_units);
        Self {
            declared: match &source.input {
                KeyframesInput::Declared(declared) => Some(declared.clone()),
                KeyframesInput::Rule(_) => None,
            },
            source: Some(source),
            properties,
            uses_viewport,
        }
    }

    /// A transition's: `property` from its value in `from` to its value in
    /// `to`, linearly (its timing's easing eases it).
    pub fn transition(
        property: LonghandId,
        from: Arc<ComputedStyle>,
        to: Arc<ComputedStyle>,
    ) -> Self {
        let frame = |offset, style| PropertyKeyframe {
            offset,
            style,
            easing: EasingFunction::Linear,
        };
        Self {
            source: None,
            declared: None,
            properties: vec![(property, vec![frame(0.0, from), frame(1.0, to)])],
            uses_viewport: false,
        }
    }

    /// Script's keyframes, not yet computed on an element: it animates
    /// nothing until style computes them (`declared` keeps them).
    pub fn declared_only(keyframes: Arc<DeclaredKeyframes>) -> Self {
        Self {
            source: None,
            declared: Some(keyframes),
            properties: Vec::new(),
            uses_viewport: false,
        }
    }

    /// An effect of no keyframes (an animation without an effect).
    pub fn empty() -> Self {
        Self::declared_only(Arc::default())
    }

    /// Whether style has to compute script's keyframes on an element whose
    /// style without animations is `base`: not yet, or for another style.
    pub fn needs_computing(&self, base: &Arc<ComputedStyle>) -> bool {
        self.declared.is_some()
            && self
                .source
                .as_ref()
                .is_none_or(|source| !Arc::ptr_eq(&source.base, base) && *source.base != **base)
    }

    /// The style a property's last keyframe is in (a transition's end
    /// value's).
    pub fn last_style(&self, property: LonghandId) -> Option<&Arc<ComputedStyle>> {
        let (_, frames) = self.properties.iter().find(|(id, _)| *id == property)?;
        Some(&frames.last()?.style)
    }

    /// The properties it animates.
    pub fn properties(&self) -> impl Iterator<Item = LonghandId> + '_ {
        self.properties.iter().map(|(id, _)| *id)
    }

    /// Its keyframes as computed, by offset (`getKeyframes()` of a CSS
    /// animation's or transition's effect): each offset's properties with
    /// the styles their values are in, and the easing from there.
    pub fn computed_keyframes(&self) -> Vec<ComputedKeyframe> {
        let mut keyframes: Vec<ComputedKeyframe> = Vec::new();
        for (id, frames) in &self.properties {
            for frame in frames {
                match keyframes
                    .iter_mut()
                    .find(|(offset, _, _)| *offset == frame.offset)
                {
                    Some((_, _, values)) => values.push((*id, frame.style.clone())),
                    None => keyframes.push((
                        frame.offset,
                        frame.easing.clone(),
                        vec![(*id, frame.style.clone())],
                    )),
                }
            }
        }
        keyframes.sort_by(|a, b| a.0.total_cmp(&b.0));
        keyframes
    }

    /// Its values at the iteration progress `progress`, written into
    /// `style`: for each property, between the keyframes around it, eased
    /// as the first of the two says (web-animations 1, "The effect value
    /// of a keyframe effect"). `before`: the animation is in its before
    /// phase, for a step easing.
    pub(crate) fn apply(&self, progress: f64, before: bool, style: &mut ComputedStyle) {
        for (id, frames) in &self.properties {
            // The last keyframe at or before it, but the last but one at
            // 1 (the first is at 0).
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "a property has two keyframes at least (at 0 and at 1), and i <= len - 2"
            )]
            let (from, to) = {
                let i = frames
                    .iter()
                    .rposition(|f| f.offset <= progress)
                    .unwrap_or(0)
                    .min(frames.len() - 2);
                (&frames[i], &frames[i + 1])
            };
            let local = (progress - from.offset) / (to.offset - from.offset);
            let eased = from.easing.evaluate(local, before);
            interpolate(*id, &from.style, &to.style, eased, style);
        }
    }
}

/// The `i`th animation's timing (css-animations 1: its easing is each
/// keyframe's, the effect's own linear).
fn timing_of(timing: &AnimationTiming<'_>) -> Timing {
    Timing {
        delay: milliseconds(timing.delay),
        end_delay: 0.0,
        fill: timing.fill_mode,
        iteration_start: 0.0,
        iterations: f64::from(timing.iteration_count),
        duration: milliseconds(timing.duration),
        direction: timing.direction,
        easing: EasingFunction::Linear,
    }
}

/// Gives the keyframes of an animation-name's `i`th name, given the ones
/// its animation had (`None`: no `@keyframes` of it).
pub type EffectFor<'a> =
    dyn FnMut(usize, &str, Option<&Arc<KeyframeEffect>>) -> Option<Arc<KeyframeEffect>> + 'a;

/// An element's CSS animations, in the order of its `animation-name`
/// (their composite order).
#[derive(Debug, Clone, Default)]
pub struct CssAnimations {
    pub animations: Vec<Animation>,
    /// Those canceled since the last [`take_removed`](Self::take_removed):
    /// script may still hold them.
    pub(crate) removed: Vec<Animation>,
}

fn owner(animation: &Animation) -> &CssAnimationOwner {
    match &animation.owner {
        Owner::CssAnimation(owner) => owner,
        _ => unreachable!("a CSS animation"),
    }
}

/// The CSS animation events among `events` ordered as the animation at
/// `place` in `animation-name` (css-animations 2's composite order).
fn in_place(events: &mut [AnimationEvent], place: usize) {
    for event in events {
        if let AnimationEvent::Animation(event) = event {
            event.order = place as u64;
        }
    }
}

impl CssAnimations {
    /// A style change (css-animations 1): `style`'s `animation-name`
    /// keeps the animations of the names it still has (the last of a name
    /// for its last, and so on), with its timing and play state
    /// (unless script played or paused it), starts one for each other name
    /// with keyframes, and cancels the rest, as one whose keyframes are
    /// gone (their events into `events`). `effect` gives the `i`th name's
    /// keyframes (`None`: no `@keyframes` of it), given the ones it had.
    /// `order` numbers new ones.
    pub fn style_change(
        &mut self,
        style: &ComputedStyle,
        now: f64,
        order: &mut u64,
        effect: &mut EffectFor<'_>,
        events: &mut Vec<AnimationEvent>,
    ) {
        let mut old = std::mem::take(&mut self.animations);
        // Where each was in `animation-name`: a canceled one's events go
        // in that order (css-animations 2's composite order then).
        let places: HashMap<u64, usize> = old.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        let mut canceled = Vec::new();
        // The animation each name keeps: from the ends of both lists, the
        // last of a name keeps the last of that name, and so on
        // (css-animations 1, `animation-name`).
        let names = &style.rare.animations.name;
        let mut kept: Vec<Option<Animation>> = names.iter().map(|_| None).collect();
        for (i, name) in names.iter().enumerate().rev() {
            if let AnimationName::Name(name) = name
                && let Some(at) = old.iter().rposition(|a| owner(a).name == *name)
            {
                kept[i] = Some(old.remove(at));
            }
        }
        for (i, name) in names.iter().enumerate() {
            let AnimationName::Name(name) = name else {
                continue;
            };
            let existing = kept[i].take();
            let Some(keyframes) = effect(i, name, existing.as_ref().map(|a| &a.effect)) else {
                canceled.extend(existing);
                continue;
            };
            let timing = style.rare.animations.timing(i);
            let paused = timing.play_state == AnimationPlayState::Paused;
            let timing = timing_of(&timing);
            match existing {
                Some(mut animation) => {
                    let owner = match &animation.owner {
                        Owner::CssAnimation(owner) => owner.clone(),
                        _ => unreachable!("a CSS animation"),
                    };
                    // What script set (css-animations 2), style no
                    // longer does.
                    if !owner.keyframes_overridden {
                        animation.effect = keyframes;
                    }
                    let timing = owner.timing_overrides.apply(&timing, &animation.timing);
                    if animation.timing != timing {
                        animation.timing = timing;
                        let _ = animation.control(now, events, |playback, at, end, notices| {
                            playback.update_finished_state(false, false, at, end, notices);
                            Ok(())
                        });
                    }
                    let Owner::CssAnimation(owner) = &mut animation.owner else {
                        unreachable!("a CSS animation");
                    };
                    let play_state_changed = owner.style_paused != paused;
                    owner.style_paused = paused;
                    if play_state_changed && !owner.play_state_overridden {
                        let _ = animation.control(now, events, |playback, at, end, notices| {
                            if paused {
                                playback.pause(at, end, notices)
                            } else {
                                playback.play(false, at, end, notices)
                            }
                        });
                    }
                    self.animations.push(animation);
                }
                None => {
                    #[expect(
                        clippy::arithmetic_side_effects,
                        reason = "one per animation made: a u64 doesn't run out"
                    )]
                    {
                        *order += 1;
                    }
                    let owner = Owner::CssAnimation(CssAnimationOwner {
                        name: name.clone(),
                        style_paused: paused,
                        play_state_overridden: false,
                        keyframes_overridden: false,
                        timing_overrides: TimingOverrides::default(),
                    });
                    self.animations.push(Animation::new(
                        *order, keyframes, timing, owner, paused, now, events,
                    ));
                }
            }
        }
        canceled.extend(old);
        for mut animation in canceled {
            let from = events.len();
            animation.cancel(now, events);
            in_place(&mut events[from..], places[&animation.id]);
            self.removed.push(animation);
        }
    }

    /// The animations canceled since the last call.
    pub fn take_removed(&mut self) -> Vec<Animation> {
        std::mem::take(&mut self.removed)
    }

    pub fn is_empty(&self) -> bool {
        self.animations.is_empty()
    }

    pub fn is_running(&self) -> bool {
        self.animations.iter().any(Animation::is_running)
    }

    /// Whether one animates `property`.
    pub fn animates(&self, property: LonghandId) -> bool {
        self.properties().any(|id| id == property)
    }

    /// The properties they animate.
    pub fn properties(&self) -> impl Iterator<Item = LonghandId> + '_ {
        self.animations.iter().flat_map(|a| a.effect.properties())
    }

    /// Whether a keyframe uses a viewport unit.
    pub fn uses_viewport(&self) -> bool {
        self.animations.iter().any(|a| a.effect.uses_viewport)
    }

    pub fn commit_pending(&mut self, now: f64, events: &mut Vec<AnimationEvent>) -> bool {
        let mut started = false;
        for animation in &mut self.animations {
            started |= animation.commit_pending(now, events);
        }
        started
    }

    pub fn tick(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        for (place, animation) in self.animations.iter_mut().enumerate() {
            let from = events.len();
            animation.tick(now, events);
            in_place(&mut events[from..], place);
        }
    }

    /// Their values at `now`, written into `style`, the later names' over
    /// the earlier's.
    pub fn apply(&self, now: f64, style: &mut ComputedStyle) {
        for animation in &self.animations {
            animation.apply(now, style);
        }
    }
}
