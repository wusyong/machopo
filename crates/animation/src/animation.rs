//! An animation (web-animations 1's `Animation` with its `KeyframeEffect`;
//! Blink's `Animation`, `KeyframeEffect`, and its `CSSTransition` and
//! `CSSAnimation`): its keyframes and timing, its playback, and who made
//! it, which says which rules and events are its. CSS transitions and CSS
//! animations are two of its owners, with their rules in `transition` and
//! `css_animation`; script is the third (`Element.animate()`,
//! `new Animation()`), with no CSS rules or events.

use std::sync::Arc;

use computed_style::ComputedStyle;
use css::properties::LonghandId;

use crate::css_animation::KeyframeEffect;
use crate::event::{
    AnimationEvent, CssAnimationEvent, CssAnimationEventKind, PlaybackEvent, TransitionEvent,
    TransitionEventKind,
};
use crate::playback::{Notice, PlayState, Playback, PlaybackError};
use crate::timing::TimingOverrides;
use crate::timing::{ComputedTiming, Phase, Timing};

/// A CSS transition's own data (css-transitions 1).
#[derive(Debug, Clone)]
pub struct TransitionOwner {
    /// A physical longhand.
    pub property: LonghandId,
    /// The style the end value is in (the after-change style).
    pub to: Arc<ComputedStyle>,
    /// What a transition back to the start reverses (the
    /// reversing-adjusted start value), and by how much it is shorter.
    pub reversing_adjusted_start: Arc<ComputedStyle>,
    pub reversing_shortening_factor: f64,
    /// Its transition generation (css-transitions 2): the style change
    /// that started it, by the order of the last animation made before;
    /// those of one generation sort by their property's name.
    pub generation: u64,
    /// It sent `transitionrun`: css-transitions 2's idle to pending, at
    /// its first tick, started or not.
    pub(crate) ran: bool,
}

/// A CSS animation's own data (css-animations 1).
#[derive(Debug, Clone)]
pub struct CssAnimationOwner {
    /// Its `animation-name`.
    pub name: Arc<str>,
    /// `animation-play-state` is `paused`, as last styled.
    pub style_paused: bool,
    /// Script played or paused it: `animation-play-state` no longer does
    /// (css-animations 2, "The CSSAnimation interface").
    pub play_state_overridden: bool,
    /// Script gave its effect keyframes: style's no longer replace them.
    pub keyframes_overridden: bool,
    /// The members of its timing script set: style no longer changes
    /// them.
    pub timing_overrides: TimingOverrides,
}

/// An animation's replace state (web-animations 1, "Replacing
/// animations"): removed, it no longer gives its target values (but
/// `commitStyles()` still can); persisted, it is never removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReplaceState {
    #[default]
    Active,
    Removed,
    Persisted,
}

/// Who made an animation.
#[derive(Debug, Clone)]
pub enum Owner {
    Transition(TransitionOwner),
    CssAnimation(CssAnimationOwner),
    /// Script: `Element.animate()`, `new Animation()`.
    Script,
}

/// An animation: what it animates (its effect, with its timing) and where
/// it is in time (its playback).
#[derive(Debug, Clone)]
pub struct Animation {
    /// Unique among the isolate's animations: what its script object
    /// stands for, and its order among the animations of its class (its
    /// composite order, and its events' at the same time).
    pub id: u64,
    /// Its effect's number (what the effect's script object stands for;
    /// it may move to another animation); `None` without one (`effect` is
    /// `null`: `effect` is then empty, its timing 0 long).
    pub effect_id: Option<u64>,
    pub effect: Arc<KeyframeEffect>,
    pub timing: Timing,
    /// How script wrote its effect's timing (what `getTiming()` gives
    /// back): with the effect as it moves from one animation to another.
    pub timing_input: crate::TimingInput,
    /// It has a timeline (the document's): without one it doesn't move,
    /// its current time only what it holds (web-animations 1's inactive
    /// timeline).
    pub has_timeline: bool,
    pub playback: Playback,
    pub owner: Owner,
    /// It still has its owning element (css-transitions 2 and
    /// css-animations 2): it sends CSS events. Style canceling or
    /// replacing it disassociates it; script's `cancel()` doesn't.
    pub owning: bool,
    /// Its place in the global animation list (web-animations 1's
    /// composite order of script's animations, which a disassociated one
    /// is): when it was made; a disassociated one's when it next leaves
    /// idle (Gecko's `mNeedsNewAnimationIndexWhenRun`).
    pub global_order: u64,
    /// Disassociated, and not run since: leaving idle it takes the next
    /// place.
    pub needs_global_order: bool,
    pub replace_state: ReplaceState,
    /// The phase it was in when last ticked, and the iteration, for its
    /// events; `None` until then (idle).
    last_phase: Option<Phase>,
    last_iteration: Option<f64>,
    /// Its timing or playback changed since it was last ticked: it runs
    /// until it is, whatever its phase was.
    pub(crate) changed: bool,
}

impl Animation {
    /// A new animation of `owner`, played at `now` (pending: it starts at
    /// the frame that shows it), and paused at once if `paused`.
    pub(crate) fn new(
        id: u64,
        effect: Arc<KeyframeEffect>,
        timing: Timing,
        owner: Owner,
        paused: bool,
        now: f64,
        events: &mut Vec<AnimationEvent>,
    ) -> Self {
        let mut animation = Self::idle(id, Some(id), effect, timing, owner);
        let mut notices = Vec::new();
        // A finite effect forwards never fails.
        let _ = animation
            .playback
            .play(true, Some(now), animation.end(), &mut notices);
        if paused {
            let _ = animation
                .playback
                .pause(Some(now), animation.end(), &mut notices);
        }
        animation.notify(notices, now, events);
        animation
    }

    /// A new animation of `owner`, of the effect numbered `effect_id`
    /// (`None`: no effect), idle (`new Animation()`): it has the
    /// document's timeline.
    pub fn idle(
        id: u64,
        effect_id: Option<u64>,
        effect: Arc<KeyframeEffect>,
        timing: Timing,
        owner: Owner,
    ) -> Self {
        Self {
            id,
            effect_id,
            effect,
            timing,
            timing_input: crate::TimingInput::default(),
            has_timeline: true,
            playback: Playback::default(),
            owner,
            owning: true,
            global_order: id,
            needs_global_order: false,
            replace_state: ReplaceState::Active,
            last_phase: None,
            last_iteration: None,
            changed: true,
        }
    }

    /// The timeline's time, `now` on the document's; `None` without a
    /// timeline.
    pub fn at(&self, now: f64) -> Option<f64> {
        self.has_timeline.then_some(now)
    }

    /// Its effect's end time (the associated effect end).
    pub fn end(&self) -> f64 {
        self.timing.end_time()
    }

    pub fn current_time(&self, now: f64) -> Option<f64> {
        self.playback.current_time(self.at(now))
    }

    pub fn computed(&self, now: f64) -> ComputedTiming {
        self.timing
            .compute(self.current_time(now), self.playback.playback_rate < 0.0)
    }

    pub fn play_state(&self, now: f64) -> PlayState {
        self.playback.play_state(self.at(now), self.end())
    }

    /// Whether it needs frames: it changed since it was last ticked, or it
    /// is pending, or it plays and hadn't ended when last ticked.
    pub fn is_running(&self) -> bool {
        let done = if self.playback.playback_rate < 0.0 {
            Phase::Before
        } else {
            Phase::After
        };
        self.changed
            || self.playback.is_pending()
            || (self.playback.start_time.is_some() && self.last_phase != Some(done))
    }

    /// Its values at `now`, written into `style`.
    pub(crate) fn apply(&self, now: f64, style: &mut ComputedStyle) {
        if self.replace_state != ReplaceState::Removed {
            self.apply_effect(now, style);
        }
    }

    /// Its effect's values at `now` onto `style` as `commitStyles()`
    /// computes them (endpoint-inclusive), unless it was removed and
    /// `with_removed` isn't set.
    pub(crate) fn commit(&self, now: f64, with_removed: bool, style: &mut ComputedStyle) {
        if self.replace_state == ReplaceState::Removed && !with_removed {
            return;
        }
        let timing = self
            .timing
            .compute_inclusive(self.current_time(now), self.playback.playback_rate < 0.0);
        if let Some(progress) = timing.progress {
            self.effect.apply(progress, timing.before_flag, style);
        }
    }

    fn apply_effect(&self, now: f64, style: &mut ComputedStyle) {
        let timing = self.computed(now);
        if let Some(progress) = timing.progress {
            self.effect.apply(progress, timing.before_flag, style);
        }
    }

    /// Its pending task done at `now`, the time of the frame whose
    /// rendering first shows it; `true` if it had one. Without a timeline
    /// it waits (web-animations 1: it isn't ready).
    pub(crate) fn commit_pending(&mut self, now: f64, events: &mut Vec<AnimationEvent>) -> bool {
        if !self.has_timeline {
            return false;
        }
        let mut notices = Vec::new();
        let done = self.playback.commit_pending(now, self.end(), &mut notices);
        self.notify(notices, now, events);
        if done {
            self.changed = true;
        }
        done
    }

    /// Its playback's notices as events for the DOM.
    pub(crate) fn notify(&self, notices: Vec<Notice>, now: f64, events: &mut Vec<AnimationEvent>) {
        events.extend(notices.into_iter().map(|notice| {
            // A `finish` event goes at the time its effect ended (before a
            // `remove` of the same update, at the timeline's time), never
            // after now: computed back from its start time, it may round
            // past now, after a `cancel()` queued next.
            let scheduled_time = match notice {
                Notice::Finished { .. } => self.finish_time().map_or(now, |time| time.min(now)),
                _ => now,
            };
            AnimationEvent::Playback(PlaybackEvent {
                id: self.id,
                notice,
                scheduled_time,
                promises_settled: false,
            })
        }));
    }

    /// The timeline's time at which it reached the end it plays towards
    /// (web-animations 1's finish notification: its associated effect end
    /// converted to timeline time); `None` without a start time or rate.
    fn finish_time(&self) -> Option<f64> {
        let start = self.playback.start_time?;
        let rate = self.playback.playback_rate;
        if rate == 0.0 {
            return None;
        }
        let at = if rate > 0.0 { self.end() } else { 0.0 };
        Some(start + at / rate)
    }

    /// Runs `step` on its playback at `now`: what script does to it. The
    /// step gets its timeline's time (`None` without one) and its end.
    pub fn control(
        &mut self,
        now: f64,
        events: &mut Vec<AnimationEvent>,
        step: impl FnOnce(
            &mut Playback,
            Option<f64>,
            f64,
            &mut Vec<Notice>,
        ) -> Result<(), PlaybackError>,
    ) -> Result<(), PlaybackError> {
        let mut notices = Vec::new();
        let end = self.end();
        let timeline = self.at(now);
        let result = step(&mut self.playback, timeline, end, &mut notices);
        self.changed = true;
        self.notify(notices, now, events);
        result
    }

    /// The interval's start and end, as the events' elapsed time goes
    /// (css-transitions 2 and css-animations 2, "Event dispatch").
    fn interval(&self) -> (f64, f64) {
        let active = self.timing.active_duration();
        let start = (-self.timing.delay).min(active).max(0.0);
        let end = (self.timing.end_time() - self.timing.delay)
            .min(active)
            .max(0.0);
        (start, end)
    }

    fn transition_event(&self, kind: TransitionEventKind, elapsed: f64, at: f64) -> AnimationEvent {
        let Owner::Transition(owner) = &self.owner else {
            unreachable!("a transition's event");
        };
        AnimationEvent::Transition(TransitionEvent {
            kind,
            property: owner.property,
            elapsed_time: elapsed / 1000.0,
            scheduled_time: at,
            order: self.id,
        })
    }

    fn animation_event(
        &self,
        kind: CssAnimationEventKind,
        elapsed: f64,
        at: f64,
    ) -> AnimationEvent {
        let Owner::CssAnimation(owner) = &self.owner else {
            unreachable!("a CSS animation's event");
        };
        AnimationEvent::Animation(CssAnimationEvent {
            kind,
            name: owner.name.clone(),
            elapsed_time: elapsed / 1000.0,
            scheduled_time: at,
            order: self.id,
        })
    }

    /// Where it is at `now` (the document timeline's time, at a frame):
    /// its finished state and finish notification, and the events of its
    /// owner as its phase changes (css-transitions 2's and css-animations
    /// 2's tables). A move with its playback's direction happens when its
    /// times say; one against it (a seek, a new timing) and one while it
    /// holds still happen now, the interval's ends swapped.
    pub(crate) fn tick(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        use Phase::*;
        if !self.owning {
            self.tick_detached(now, events);
            return;
        }
        self.tick_playback(now, events);
        // Idle: in no phase, as after a cancel.
        if self.current_time(now).is_none() {
            self.last_phase = None;
            self.last_iteration = None;
            self.changed = false;
            return;
        }

        let timing = self.computed(now);
        let (start, end) = self.interval();
        let rate = self.playback.playback_rate;
        // A finished one holds its end, but still has its start time; a
        // paused or pending one has none.
        let at = |elapsed: f64| match self.playback.start_time {
            Some(origin) if rate != 0.0 => origin + (self.timing.delay + elapsed) / rate,
            _ => now,
        };
        // Its playback's direction, and the times of the interval's ends
        // as a move with it, or against it, comes to them.
        let forwards = rate >= 0.0;
        let (at_start, at_end) = (at(start), at(end));
        let with = |forward_move: bool, time: f64| {
            if forward_move == forwards { time } else { now }
        };
        let was = self.last_phase;
        let mut changes = Vec::new();
        match (was, timing.phase) {
            (None | Some(Before), Active) => changes.push((true, start, with(true, at_start))),
            (Some(After), Active) => changes.push((true, end, with(false, at_end))),
            (None | Some(Before), After) => {
                changes.push((true, start, with(true, at_start)));
                changes.push((false, end, with(true, at_end)));
            }
            (Some(After), Before) => {
                changes.push((true, end, with(false, at_end)));
                changes.push((false, start, with(false, at_start)));
            }
            (Some(Active), Before) => changes.push((false, start, with(false, at_start))),
            (Some(Active), After) => changes.push((false, end, with(true, at_end))),
            _ => {}
        }
        match &mut self.owner {
            Owner::Transition(owner) => {
                let first = !owner.ran;
                owner.ran = true;
                if first {
                    let run_at = self.playback.start_time.unwrap_or(now);
                    events.push(self.transition_event(TransitionEventKind::Run, start, run_at));
                }
                // Pending, it waits: only `transitionrun` (css-transitions
                // 2's idle to pending).
                if self.playback.pending.is_some() && self.playback.start_time.is_none() {
                    self.changed = false;
                    return;
                }
                for (is_start, elapsed, at) in changes {
                    let kind = if is_start {
                        TransitionEventKind::Start
                    } else {
                        TransitionEventKind::End
                    };
                    events.push(self.transition_event(kind, elapsed, at));
                }
            }
            Owner::Script => {}
            Owner::CssAnimation(_) => {
                for (is_start, elapsed, at) in changes {
                    let kind = if is_start {
                        CssAnimationEventKind::Start
                    } else {
                        CssAnimationEventKind::End
                    };
                    events.push(self.animation_event(kind, elapsed, at));
                }
                if (was, timing.phase) == (Some(Active), Active)
                    && let (Some(iteration), Some(last)) =
                        (timing.current_iteration, self.last_iteration)
                    && iteration != last
                {
                    // Back into an earlier one, at its end.
                    let reached = if iteration < last {
                        iteration + 1.0
                    } else {
                        iteration
                    };
                    let elapsed = (reached - self.timing.iteration_start) * self.timing.duration;
                    events.push(self.animation_event(
                        CssAnimationEventKind::Iteration,
                        elapsed,
                        at(elapsed),
                    ));
                }
            }
        }
        self.last_phase = Some(timing.phase);
        self.last_iteration = timing.current_iteration;
        self.changed = false;
    }

    /// Canceled by style (or replaced): it no longer has its owning
    /// element, and sends no CSS events (css-transitions 2 and
    /// css-animations 2); its playback's go on.
    pub(crate) fn disassociate(&mut self) {
        self.owning = false;
        self.needs_global_order = true;
    }

    /// Whether script can still reach it without its object: it has a
    /// current time, and isn't done or still fills (`getAnimations()`
    /// lists it), or may still send `finish`.
    pub fn is_reachable(&self, now: f64) -> bool {
        if self.replace_state == ReplaceState::Removed {
            return false;
        }
        match self.play_state(now) {
            PlayState::Idle => false,
            PlayState::Finished => self.computed(now).active_time.is_some(),
            PlayState::Running | PlayState::Paused => true,
        }
    }

    /// Its finished state and finish notification at `now`.
    fn tick_playback(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        let mut notices = Vec::new();
        self.playback
            .update_finished_state(false, false, self.at(now), self.end(), &mut notices);
        self.playback
            .run_finish_notification(self.at(now), self.end(), &mut notices);
        self.notify(notices, now, events);
    }

    /// Where one that left its element (canceled, replaced, done) is at
    /// `now`: only its playback moves, as script plays it; it has no owner
    /// to send CSS events any more (css-transitions 2 and css-animations
    /// 2: canceling clears the owning element).
    pub fn tick_detached(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        self.tick_playback(now, events);
        self.last_phase = Some(self.computed(now).phase);
        self.changed = false;
    }

    /// Its pending task done at `now`, the time of the frame whose
    /// rendering first shows it, as [`commit_pending`](Self::commit_pending)
    /// for one that left its element.
    pub fn commit_pending_detached(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        self.commit_pending(now, events);
    }

    /// Canceled at `now` (by style, script, or because its element went):
    /// its owner's cancel event if it had started and not ended (from any
    /// phase but idle and after; a transition's once it sent
    /// `transitionrun`),
    /// its active time then the elapsed time; and its playback's.
    pub fn cancel(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        let active = self.computed(now).active_time.unwrap_or(0.0);
        self.cancel_event(active, now, events);
        let mut notices = Vec::new();
        self.playback.cancel(self.at(now), self.end(), &mut notices);
        self.notify(notices, now, events);
        self.went_idle();
    }

    /// Whether another animation can replace it (web-animations 1's
    /// replaceable animation): markup doesn't make it (script's, or one
    /// that lost its owning element), it is finished but still in effect,
    /// with an effect, on the document's timeline, and not removed yet.
    pub fn is_replaceable(&self, now: f64) -> bool {
        (matches!(self.owner, Owner::Script) || !self.owning)
            && self.replace_state != ReplaceState::Removed
            && self.has_timeline
            && self.effect_id.is_some()
            && self.play_state(now) == PlayState::Finished
            && self.computed(now).progress.is_some()
    }

    /// Removed at `now`, replaced: its `remove` event.
    pub(crate) fn remove(&mut self, now: f64, events: &mut Vec<AnimationEvent>) {
        self.replace_state = ReplaceState::Removed;
        let notice = Notice::Removed {
            current_time: self.current_time(now),
            timeline_time: self.at(now),
        };
        self.notify(vec![notice], now, events);
    }

    /// Its timeline given (`has`) or taken away (web-animations 1's "set
    /// the timeline of an animation"): a start time it had stays, its hold
    /// time goes (it isn't finished just for having been). Without one, a
    /// start time gives no current time: idle, which its owner's cancel
    /// event says as for [`cancel`](Self::cancel) (css-transitions 2's and
    /// css-animations 2's tables: any phase to idle).
    pub fn set_timeline(&mut self, has: bool, now: f64, events: &mut Vec<AnimationEvent>) {
        if self.has_timeline == has {
            return;
        }
        let active = self.computed(now).active_time.unwrap_or(0.0);
        self.has_timeline = has;
        let _ = self.control(now, events, |playback, at, end, notices| {
            if playback.start_time.is_some() {
                playback.hold_time = None;
            }
            playback.update_finished_state(false, false, at, end, notices);
            Ok(())
        });
        if self.current_time(now).is_none() {
            self.cancel_event(active, now, events);
            self.went_idle();
        }
    }

    /// Its owner's cancel event, if it had started and not ended, with
    /// `active` its elapsed time.
    fn cancel_event(&self, active: f64, now: f64, events: &mut Vec<AnimationEvent>) {
        match &self.owner {
            _ if !self.owning => {}
            Owner::Transition(owner) if owner.ran && self.last_phase != Some(Phase::After) => {
                events.push(self.transition_event(TransitionEventKind::Cancel, active, now));
            }
            Owner::CssAnimation(_) if !matches!(self.last_phase, None | Some(Phase::After)) => {
                events.push(self.animation_event(CssAnimationEventKind::Cancel, active, now));
            }
            _ => {}
        }
    }

    /// Idle: canceled again it says nothing, and played again its events
    /// start over (`transitionrun`, `animationstart`).
    fn went_idle(&mut self) {
        if let Owner::Transition(owner) = &mut self.owner {
            owner.ran = false;
        }
        self.last_phase = None;
        self.last_iteration = None;
    }
}
