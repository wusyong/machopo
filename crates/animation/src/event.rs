//! The events animations send (css-transitions 2 and css-animations 2,
//! "Event dispatch"), and what their playback does to their promises and
//! `finish` and `cancel` events (web-animations 1), for the rendering
//! update's "update animations and send events" to dispatch.

use std::sync::Arc;

use css::properties::LonghandId;

use crate::playback::Notice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionEventKind {
    Run,
    Start,
    End,
    Cancel,
}

impl TransitionEventKind {
    /// The event's type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Run => "transitionrun",
            Self::Start => "transitionstart",
            Self::End => "transitionend",
            Self::Cancel => "transitioncancel",
        }
    }
}

/// A transition event to dispatch (css-transitions 2, "Transition
/// Events").
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionEvent {
    pub kind: TransitionEventKind,
    pub property: LonghandId,
    /// `elapsedTime`, in seconds.
    pub elapsed_time: f64,
    /// On the document timeline, in milliseconds.
    pub scheduled_time: f64,
    pub order: u64,
}

/// What an animation's playback did to its promises, or its `finish` or
/// `cancel` event (an `AnimationPlaybackEvent` at its script object).
#[derive(Debug, Clone, PartialEq)]
pub struct PlaybackEvent {
    /// The animation's.
    pub id: u64,
    pub notice: Notice,
    /// On the document timeline, in milliseconds: when it happened.
    pub scheduled_time: f64,
    /// Its promise was settled when it happened (script's call did it),
    /// so the update of animations only fires its event.
    pub promises_settled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CssAnimationEventKind {
    Start,
    Iteration,
    End,
    Cancel,
}

impl CssAnimationEventKind {
    /// The event's type.
    pub fn name(self) -> &'static str {
        match self {
            Self::Start => "animationstart",
            Self::Iteration => "animationiteration",
            Self::End => "animationend",
            Self::Cancel => "animationcancel",
        }
    }
}

/// A CSS animation's event.
#[derive(Debug, Clone, PartialEq)]
pub struct CssAnimationEvent {
    pub kind: CssAnimationEventKind,
    /// Its `animation-name`.
    pub name: Arc<str>,
    /// `elapsedTime`, in seconds.
    pub elapsed_time: f64,
    /// On the document timeline, in milliseconds.
    pub scheduled_time: f64,
    pub order: u64,
}

/// A transition's or a CSS animation's event: dispatched by when it was
/// scheduled, then in composite order (transitions before CSS animations,
/// each class by the order they were made in; web-animations 1's "update
/// animations and send events").
#[derive(Debug, Clone, PartialEq)]
pub enum AnimationEvent {
    Transition(TransitionEvent),
    Animation(CssAnimationEvent),
    Playback(PlaybackEvent),
}

impl AnimationEvent {
    /// Whether it is a playback's notice that only settles a promise
    /// (not its `finish` or `cancel` event).
    pub fn is_promise_notice(&self) -> bool {
        matches!(
            self,
            Self::Playback(event)
                if !matches!(
                    event.notice,
                    Notice::Finished { .. } | Notice::Canceled { .. } | Notice::Removed { .. }
                )
        )
    }

    /// The event's type; `None` for what isn't an event of the CSS
    /// specs (a playback's).
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Self::Transition(event) => Some(event.kind.name()),
            Self::Animation(event) => Some(event.kind.name()),
            Self::Playback(_) => None,
        }
    }

    /// Where it sorts among the events to dispatch.
    pub fn dispatch_order(&self, other: &Self) -> std::cmp::Ordering {
        let (a, b) = (self.order_key(), other.order_key());
        a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2))
    }

    /// What [`dispatch_order`](Self::dispatch_order) compares: when it was
    /// scheduled, its class (a transition's, a CSS animation's, a
    /// playback's) and its animation's order. The DOM puts its target's
    /// place in tree order between the last two.
    pub fn order_key(&self) -> (f64, u8, u64) {
        match self {
            Self::Transition(e) => (e.scheduled_time, 0, e.order),
            Self::Animation(e) => (e.scheduled_time, 1, e.order),
            Self::Playback(e) => (e.scheduled_time, 2, e.id),
        }
    }
}

impl From<TransitionEvent> for AnimationEvent {
    fn from(event: TransitionEvent) -> Self {
        Self::Transition(event)
    }
}
