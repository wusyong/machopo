//! CSS transitions (css-transitions 1 and 2; Blink's
//! `CSSAnimations::CalculateTransitionUpdate` and `CSSTransition`): when a
//! style change starts, replaces, reverses or cancels a transition (an
//! [`Animation`] a transition owns), and an element's animations, which
//! give its style their values at a time.

use std::collections::HashSet;
use std::sync::Arc;

use computed_style::{
    ComputedStyle, EasingFunction, FillMode, PlaybackDirection, TransitionBehavior,
    TransitionProperty,
};
use css::interpolate::{AnimationType, interpolable, same_value};
use css::properties::{LonghandId, longhands_of};

use crate::animation::{Animation, Owner, ReplaceState, TransitionOwner};
use crate::css_animation::{CssAnimations, KeyframeEffect};
use crate::event::AnimationEvent;
use crate::playback::PlayState;
use crate::timing::{Timing, milliseconds};

/// A target's animations and its style as the cascade last made it, which
/// their values are applied to (Blink's `ElementAnimations` with its base
/// computed style): what a DOM keeps of an element or pseudo-element that
/// has animations.
#[derive(Debug, Clone)]
pub struct AnimatedStyle {
    pub animations: ElementAnimations,
    pub base: Arc<ComputedStyle>,
}

fn owner(animation: &Animation) -> &TransitionOwner {
    match &animation.owner {
        Owner::Transition(owner) => owner,
        _ => unreachable!("a transition"),
    }
}

/// An element's animations (Blink's `ElementAnimations`): its CSS
/// animations; its running transitions (those script canceled too: they
/// keep their owning element), and the end values of the completed ones,
/// which a style change with the same end value doesn't start again; and
/// the animations style no longer runs whose target it still is
/// (`others`: done transitions, and those style canceled or replaced,
/// which no longer have their owning element), which script may still
/// play: they give it their values too.
#[derive(Debug, Clone, Default)]
pub struct ElementAnimations {
    pub css_animations: CssAnimations,
    pub transitions: Vec<Animation>,
    pub others: Vec<Animation>,
    completed: Vec<(LonghandId, Arc<ComputedStyle>)>,
    /// All of them, once [`cancel_all`](Self::cancel_all) took them off
    /// it, for [`take_removed`](Self::take_removed).
    removed: Vec<Animation>,
}

/// The transition-property entry that matches each longhand, the last
/// one that names it (css-transitions 1): its index in the lists.
fn matching_transitions(style: &ComputedStyle) -> Vec<(LonghandId, usize)> {
    let mut matched: Vec<(LonghandId, usize)> = Vec::new();
    // Where each property is in `matched`: `all` lists every one.
    let mut at: Vec<Option<usize>> = Vec::new();
    let mut add = |id: LonghandId, i: usize| {
        let id = id.physical();
        if id.animation_type() == AnimationType::NotAnimatable {
            return;
        }
        if at.is_empty() {
            at.resize(LonghandId::COUNT, None);
        }
        match at[id.index()] {
            Some(entry) => matched[entry].1 = i,
            None => {
                at[id.index()] = Some(matched.len());
                matched.push((id, i));
            }
        }
    };
    for (i, property) in style.transitions.property.iter().enumerate() {
        match property {
            TransitionProperty::None => {}
            // A flow-relative longhand is its physical one's: listed once.
            TransitionProperty::All => {
                for id in LonghandId::ALL {
                    add(id, i);
                }
            }
            TransitionProperty::Name(name) => {
                for id in longhands_of(name).unwrap_or_default() {
                    add(id, i);
                }
            }
        }
    }
    matched
}

impl ElementAnimations {
    /// Whether there is anything to keep: a CSS animation, a transition
    /// running, one completed, or another animation.
    pub fn is_empty(&self) -> bool {
        self.css_animations.is_empty()
            && self.transitions.is_empty()
            && self.completed.is_empty()
            && self.others.is_empty()
    }

    /// Whether [`cancel_all`](Self::cancel_all) took animations off it,
    /// for [`take_removed`](Self::take_removed).
    pub fn has_removed(&self) -> bool {
        !self.removed.is_empty()
    }

    /// Whether one of its animations runs (one changed, is pending, or
    /// plays and hadn't ended): the document wants frames.
    pub fn is_running(&self) -> bool {
        self.transitions.iter().any(Animation::is_running)
            || self.css_animations.is_running()
            || self.others.iter().any(Animation::is_running)
    }

    /// Whether one of its animations animates `property`.
    pub fn animates(&self, property: LonghandId) -> bool {
        self.properties().any(|id| id == property)
    }

    /// The properties its animations animate.
    pub fn properties(&self) -> impl Iterator<Item = LonghandId> + '_ {
        self.transitions
            .iter()
            .map(|t| owner(t).property)
            .chain(self.css_animations.properties())
            .chain(self.others.iter().flat_map(|a| a.effect.properties()))
    }

    /// Its animations: the transitions, the CSS animations, then the
    /// others, each in their order.
    pub fn animations(&self) -> impl Iterator<Item = &Animation> + '_ {
        self.transitions
            .iter()
            .chain(&self.css_animations.animations)
            .chain(&self.others)
    }

    /// Its animations, to change.
    pub fn animations_mut(&mut self) -> impl Iterator<Item = &mut Animation> + '_ {
        self.transitions
            .iter_mut()
            .chain(&mut self.css_animations.animations)
            .chain(&mut self.others)
    }

    /// The animation numbered `id`, if it is one of its.
    pub fn animation_mut(&mut self, id: u64) -> Option<&mut Animation> {
        self.transitions
            .iter_mut()
            .chain(&mut self.css_animations.animations)
            .chain(&mut self.others)
            .find(|a| a.id == id)
    }

    /// A style change event (css-transitions 1, "Starting of
    /// transitions"): from `before`, the element's style as of the last
    /// one with its transitions at `now`, to `after`, its new style
    /// without them. Starts, replaces, reverses and cancels transitions;
    /// `order` numbers new ones. The events of the canceled ones go into
    /// `events`; a new one's come with its first tick.
    pub fn style_change(
        &mut self,
        before: &Arc<ComputedStyle>,
        after: &Arc<ComputedStyle>,
        now: f64,
        order: &mut u64,
        events: &mut Vec<AnimationEvent>,
    ) {
        let matched = matching_transitions(after);
        let generation = *order;
        // A running transition no entry matches any more is canceled; a
        // completed one forgotten.
        let mut canceled = Vec::new();
        let mut kept = Vec::new();
        for t in std::mem::take(&mut self.transitions) {
            if matched.iter().any(|(id, _)| *id == owner(&t).property) {
                kept.push(t);
            } else {
                canceled.push(t);
            }
        }
        self.transitions = kept;
        self.completed
            .retain(|(id, _)| matched.iter().any(|(other, _)| other == id));
        // Each listed transition's times, worked out once for all the
        // properties it has.
        let mut times: Vec<Option<(f64, f64)>> = Vec::new();
        for (property, i) in matched {
            let (duration, easing, delay, behavior) = after.transitions.timing(i);
            if times.len() <= i {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "i indexes the listed transitions, a Vec's"
                )]
                let len = i + 1;
                times.resize(len, None);
            }
            let (duration, delay) =
                *times[i].get_or_insert_with(|| (milliseconds(duration), milliseconds(delay)));
            let combined = duration.max(0.0) + delay;
            // css-transitions 2: a pair that doesn't interpolate (a
            // discrete property's, or `auto` and a length) transitions as
            // a step with `allow-discrete`.
            let transitionable = |from: &ComputedStyle, to: &ComputedStyle| {
                interpolable(property, from, to) || behavior == TransitionBehavior::AllowDiscrete
            };
            // A completed transition to another value is forgotten.
            self.completed
                .retain(|(id, end)| *id != property || same_value(property, end, after));
            let running = self
                .transitions
                .iter()
                .position(|t| owner(t).property == property);
            let Some(r) = running else {
                let completed_here = self.completed.iter().any(|(id, _)| *id == property);
                if !same_value(property, before, after)
                    && !completed_here
                    && combined > 0.0
                    && transitionable(before, after)
                {
                    self.start(
                        property, before, after, before, 1.0, delay, duration, easing, now, order,
                        generation, events,
                    );
                }
                continue;
            };
            if same_value(property, &owner(&self.transitions[r]).to, after) {
                continue;
            }
            // The current value is the before-change style's.
            let old = self.transitions.remove(r);
            let old_owner = owner(&old).clone();
            let progress = old.computed(now).progress.unwrap_or(0.0);
            canceled.push(old);
            if same_value(property, before, after)
                || !transitionable(before, after)
                || combined <= 0.0
            {
                continue;
            }
            if same_value(property, &old_owner.reversing_adjusted_start, after) {
                // Back to where it came from: shorter, by how far it got.
                let factor = (progress * old_owner.reversing_shortening_factor + 1.0
                    - old_owner.reversing_shortening_factor)
                    .abs()
                    .clamp(0.0, 1.0);
                let delay = if delay < 0.0 { delay * factor } else { delay };
                self.start(
                    property,
                    before,
                    after,
                    &old_owner.to,
                    factor,
                    delay,
                    duration * factor,
                    easing,
                    now,
                    order,
                    generation,
                    events,
                );
            } else {
                self.start(
                    property, before, after, before, 1.0, delay, duration, easing, now, order,
                    generation, events,
                );
            }
        }
        for mut t in canceled {
            t.cancel(now, events);
            t.disassociate();
            self.others.push(t);
        }
        self.transitions.sort_by_key(|t| t.id);
    }

    /// The CSS animations style canceled since (their `animation-name`
    /// no longer has them), among `others`.
    pub fn keep_canceled(&mut self) {
        for mut animation in self.css_animations.take_removed() {
            animation.disassociate();
            self.others.push(animation);
        }
    }

    /// What [`cancel_all`](Self::cancel_all) took off it.
    pub fn take_removed(&mut self) -> Vec<Animation> {
        std::mem::take(&mut self.removed)
    }

    /// Takes off it those of `others` `keep` doesn't keep, and those
    /// transitions script canceled that it doesn't (script can no longer
    /// reach them).
    pub fn forget(&mut self, mut keep: impl FnMut(&Animation) -> bool) {
        self.others.retain(&mut keep);
        self.transitions
            .retain(|t| t.playback.play_state(None, t.end()) != PlayState::Idle || keep(t));
    }

    #[allow(clippy::too_many_arguments)]
    fn start(
        &mut self,
        property: LonghandId,
        from: &Arc<ComputedStyle>,
        to: &Arc<ComputedStyle>,
        reversing_adjusted_start: &Arc<ComputedStyle>,
        reversing_shortening_factor: f64,
        delay: f64,
        duration: f64,
        easing: &EasingFunction,
        now: f64,
        order: &mut u64,
        generation: u64,
        events: &mut Vec<AnimationEvent>,
    ) {
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per animation made: a u64 doesn't run out"
        )]
        {
            *order += 1;
        }
        let timing = Timing {
            delay,
            end_delay: 0.0,
            fill: FillMode::Backwards,
            iteration_start: 0.0,
            iterations: 1.0,
            duration: duration.max(0.0),
            direction: PlaybackDirection::Normal,
            easing: easing.clone(),
        };
        let owner = Owner::Transition(TransitionOwner {
            property,
            to: to.clone(),
            reversing_adjusted_start: reversing_adjusted_start.clone(),
            reversing_shortening_factor,
            generation,
            ran: false,
        });
        let effect = Arc::new(KeyframeEffect::transition(
            property,
            from.clone(),
            to.clone(),
        ));
        self.transitions.push(Animation::new(
            *order, effect, timing, owner, false, now, events,
        ));
    }

    /// The pending transitions and CSS animations start (or pause) at
    /// `now`, the time of the frame whose rendering shows them first;
    /// `true` if one did.
    pub fn commit_pending(&mut self, now: f64, events: &mut Vec<AnimationEvent>) -> bool {
        let mut started = self.css_animations.commit_pending(now, events);
        for t in self.transitions.iter_mut().chain(&mut self.others) {
            started |= t.commit_pending(now, events);
        }
        started
    }

    /// The animations at `now` (the document timeline's time, at a
    /// frame): the events of each one's phase change, and the transitions
    /// finished completed (those script canceled just go).
    pub fn tick(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        self.css_animations.tick(now, events);
        for t in self.transitions.iter_mut().chain(&mut self.others) {
            t.tick(now, events);
        }
        let mut running = Vec::new();
        for t in std::mem::take(&mut self.transitions) {
            match t.play_state(now) {
                // Completed: no longer running, still its owning
                // element's (sought back, it sends its events again).
                PlayState::Finished => {
                    let owner = owner(&t);
                    self.completed.retain(|(id, _)| *id != owner.property);
                    self.completed.push((owner.property, owner.to.clone()));
                    self.others.push(t);
                }
                PlayState::Idle | PlayState::Running | PlayState::Paused => running.push(t),
            }
        }
        self.transitions = running;
    }

    /// Cancels every transition and CSS animation (the element left the
    /// document, or no longer renders: css-transitions 1, css-animations
    /// 1), with their events; script's, and those markup no longer makes,
    /// go on as they were (web-animations 1: nothing cancels them). All
    /// leave it.
    pub fn cancel_all(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        self.keep_canceled();
        let css = std::mem::take(&mut self.css_animations.animations);
        let transitions = std::mem::take(&mut self.transitions);
        let others = std::mem::take(&mut self.others);
        for mut animation in css.into_iter().chain(transitions).chain(others) {
            if !matches!(animation.owner, Owner::Script) && animation.owning {
                animation.cancel(now, events);
                animation.disassociate();
            }
            self.removed.push(animation);
        }
        self.completed.clear();
    }

    /// The animations markup makes canceled, as by
    /// [`cancel_all`](Self::cancel_all) (the element was removed and
    /// inserted again: a new element, to css-transitions and
    /// css-animations); script's, and those markup no longer makes, stay.
    pub fn cancel_markup(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        self.keep_canceled();
        let (kept, markup) = std::mem::take(&mut self.others)
            .into_iter()
            .partition(|a| matches!(a.owner, Owner::Script) || !a.owning);
        self.others = markup;
        self.cancel_all(now, events);
        self.others = kept;
    }

    /// `base` with its CSS animations' values at `now`: what transitions
    /// start from and to (css-transitions 1's before-change and
    /// after-change styles have them).
    pub fn apply_css_animations(&self, now: f64, base: &ComputedStyle) -> ComputedStyle {
        let mut style = base.clone();
        self.css_animations.apply(now, &mut style);
        // Then the others, script's in composite order: after the CSS
        // animations, by their places in the global animation list.
        for animation in self.others_in_order() {
            animation.apply(now, &mut style);
        }
        style
    }

    /// The others in composite order.
    fn others_in_order(&self) -> Vec<&Animation> {
        let mut others: Vec<&Animation> = self.others.iter().collect();
        others.sort_by_key(|a| a.global_order);
        others
    }

    /// `base` with the values at `now` of its animations in composite
    /// order up to `id`'s, and its (`commitStyles()`'s partial effect
    /// stack, endpoint-inclusive): the removed ones only if `id`'s was
    /// removed too.
    pub fn apply_up_to(&self, now: f64, base: &ComputedStyle, id: u64) -> ComputedStyle {
        let mut style = base.clone();
        // web-animations' composite order: transitions, CSS animations,
        // then the others.
        let stack: Vec<&Animation> = self
            .transitions
            .iter()
            .chain(&self.css_animations.animations)
            .chain(self.others_in_order())
            .collect();
        let Some(at) = stack.iter().position(|a| a.id == id) else {
            return style;
        };
        let with_removed = stack[at].replace_state == ReplaceState::Removed;
        for animation in &stack[..=at] {
            animation.commit(now, with_removed, &mut style);
        }
        style
    }

    /// web-animations 1's "remove replaced animations" on this target:
    /// from the last in composite order, a replaceable animation whose
    /// properties later replaceable ones all animate is removed (Blink's
    /// `AnimationTimeline::RemoveReplacedAnimations`); a persisted one is
    /// kept, though its properties count. Their `remove` events into
    /// `events`.
    pub fn remove_replaced(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        let mut order: Vec<usize> = (0..self.others.len())
            .filter(|&i| self.others[i].is_replaceable(now))
            .collect();
        order.sort_by_key(|&i| std::cmp::Reverse(self.others[i].global_order));
        let mut replaced: HashSet<LonghandId> = HashSet::new();
        for i in order {
            let animation = &mut self.others[i];
            let mut covered = true;
            for id in animation.effect.properties() {
                covered &= !replaced.insert(id);
            }
            if covered && animation.replace_state == ReplaceState::Active {
                animation.remove(now, events);
            }
        }
    }

    /// `base` with its CSS animations' values at `now`, then each running
    /// transition's over them, in their order (css-cascade's animation
    /// origin, below the transition one).
    pub fn apply(&self, now: f64, base: &ComputedStyle) -> ComputedStyle {
        let mut style = self.apply_css_animations(now, base);
        for t in &self.transitions {
            t.apply(now, &mut style);
        }
        style
    }
}
