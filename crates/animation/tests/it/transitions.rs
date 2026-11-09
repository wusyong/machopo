//! CSS transitions (css-transitions 1, "Starting of transitions", and 2,
//! "Transition Events"): what a style change starts, replaces, reverses
//! and cancels, the values they give, and their events.

use std::sync::Arc;

use animation::{AnimationEvent, ElementAnimations, Owner, TransitionEventKind};
use computed_style::{
    ComputedStyle, EasingFunction, LengthPercentage, Position, Size, TransitionBehavior,
    TransitionProperty, Transitions,
};
use css::properties::LonghandId;

/// A style with `width: {width}px` and `transition: {property}
/// {duration}s linear {delay}s`.
fn styled(width: f32, property: &str, duration: f32, delay: f32) -> Arc<ComputedStyle> {
    let mut style = ComputedStyle::initial();
    style.width = Size::LengthPercentage(LengthPercentage::Px(width));
    style.transitions = transitions(&[property], duration, delay);
    Arc::new(style)
}

fn transitions(properties: &[&str], duration: f32, delay: f32) -> Transitions {
    Transitions {
        property: properties
            .iter()
            .map(|&name| match name {
                "all" => TransitionProperty::All,
                "none" => TransitionProperty::None,
                name => TransitionProperty::Name(name.into()),
            })
            .collect(),
        duration: Arc::new([duration]),
        timing_function: Arc::new([EasingFunction::Linear]),
        delay: Arc::new([delay]),
        behavior: Arc::new([TransitionBehavior::Normal]),
    }
}

fn width(style: &ComputedStyle) -> f32 {
    match style.width {
        Size::LengthPercentage(LengthPercentage::Px(px)) => px,
        other => panic!("{other:?}"),
    }
}

/// An element's transitions and what they send, as a frame loop would
/// drive them.
struct Element {
    animations: ElementAnimations,
    /// Its style with its transitions.
    style: Arc<ComputedStyle>,
    /// Its style without them.
    base: Arc<ComputedStyle>,
    order: u64,
    events: Vec<AnimationEvent>,
}

impl Element {
    fn new(style: Arc<ComputedStyle>) -> Self {
        Self {
            animations: ElementAnimations::default(),
            base: style.clone(),
            style,
            order: 0,
            events: Vec::new(),
        }
    }

    /// A style change at `now` to `after`, in the lifecycle of the frame at
    /// `now`, which starts the new transitions: the style is then the new
    /// one with its transitions.
    fn change(&mut self, after: Arc<ComputedStyle>, now: f64) {
        self.animations
            .style_change(&self.style, &after, now, &mut self.order, &mut self.events);
        self.style = Arc::new(self.animations.apply(now, &after));
        self.base = after;
        self.animations.commit_pending(now, &mut self.events);
    }

    fn tick(&mut self, now: f64) {
        self.animations.tick(now, &mut self.events);
        self.style = Arc::new(self.animations.apply(now, &self.base));
    }

    /// The events since the last call: their kind and elapsed time.
    fn take_events(&mut self) -> Vec<(TransitionEventKind, f64)> {
        std::mem::take(&mut self.events)
            .into_iter()
            .filter_map(|e| match e {
                AnimationEvent::Transition(e) => Some((e.kind, e.elapsed_time)),
                AnimationEvent::Animation(e) => panic!("no CSS animations here: {e:?}"),
                AnimationEvent::Playback(_) => None,
            })
            .collect()
    }
}

use TransitionEventKind::*;

#[test]
fn a_change_starts_a_transition() {
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 1000.0);
    assert!(e.animations.is_running());
    assert_eq!(width(&e.style), 0.0, "the start value at its start");
    e.tick(1250.0);
    assert_eq!(width(&e.style), 25.0);
    assert_eq!(e.take_events(), [(Run, 0.0), (Start, 0.0)]);
    e.tick(2000.0);
    assert_eq!(width(&e.style), 100.0, "done: the base value");
    assert_eq!(e.take_events(), [(End, 1.0)]);
    assert!(!e.animations.is_running());
    // Done, the same end value again starts nothing.
    e.change(styled(100.0, "width", 1.0, 0.0), 2500.0);
    assert!(!e.animations.is_running());
    // Another one starts another, from the done one's end: nothing to
    // cancel, it left.
    e.change(styled(200.0, "width", 1.0, 0.0), 3000.0);
    assert!(e.animations.is_running());
    e.tick(3000.0);
    assert_eq!(e.take_events(), [(Run, 0.0), (Start, 0.0)]);
}

/// A transition that completed isn't started again from a value the
/// style kept from before it completed (its last frame's).
#[test]
fn a_completed_transition_isnt_started_again() {
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(990.0);
    assert_eq!(width(&e.style), 99.0);
    // Completed, with no new style made.
    e.animations.tick(1000.0, &mut e.events);
    e.change(styled(100.0, "width", 1.0, 0.0), 1000.0);
    assert!(!e.animations.is_running());
    // Canceling forgets it.
    assert!(!e.animations.is_empty());
    e.animations.cancel_all(1000.0, &mut e.events);
    assert!(e.animations.is_empty());
}

#[test]
fn what_starts_nothing() {
    let start = |before: Arc<ComputedStyle>, after: Arc<ComputedStyle>| {
        let mut e = Element::new(before);
        e.change(after, 0.0);
        e.animations.is_running()
    };
    // The same value.
    assert!(!start(
        styled(0.0, "width", 1.0, 0.0),
        styled(0.0, "width", 1.0, 0.0)
    ));
    // No time: the duration and delay add up to 0 or less.
    assert!(!start(
        styled(0.0, "width", 0.0, 0.0),
        styled(9.0, "width", 0.0, 0.0)
    ));
    assert!(!start(
        styled(0.0, "width", 1.0, -1.0),
        styled(9.0, "width", 1.0, -1.0)
    ));
    assert!(start(
        styled(0.0, "width", 0.0, 1.0),
        styled(9.0, "width", 0.0, 1.0)
    ));
    // Another property.
    assert!(!start(
        styled(0.0, "height", 1.0, 0.0),
        styled(9.0, "height", 1.0, 0.0)
    ));
    assert!(!start(
        styled(0.0, "none", 1.0, 0.0),
        styled(9.0, "none", 1.0, 0.0)
    ));
    // The after-change style's `transition` counts, not the before's.
    assert!(start(
        styled(0.0, "none", 1.0, 0.0),
        styled(9.0, "width", 1.0, 0.0)
    ));
    assert!(!start(
        styled(0.0, "width", 1.0, 0.0),
        styled(9.0, "none", 1.0, 0.0)
    ));
    // `auto` against a length doesn't interpolate.
    let mut auto = (*styled(0.0, "width", 1.0, 0.0)).clone();
    auto.width = Size::Auto;
    assert!(!start(Arc::new(auto), styled(9.0, "width", 1.0, 0.0)));
}

/// A discrete property transitions with `allow-discrete` only, as a
/// step halfway.
#[test]
fn allow_discrete() {
    let positioned = |position: Position, behavior: TransitionBehavior| {
        let mut style = ComputedStyle::initial();
        style.position = position;
        style.transitions = transitions(&["position"], 1.0, 0.0);
        style.transitions.behavior = Arc::new([behavior]);
        Arc::new(style)
    };
    let mut e = Element::new(positioned(Position::Static, TransitionBehavior::Normal));
    e.change(
        positioned(Position::Absolute, TransitionBehavior::Normal),
        0.0,
    );
    assert!(!e.animations.is_running());
    let mut e = Element::new(positioned(
        Position::Static,
        TransitionBehavior::AllowDiscrete,
    ));
    e.change(
        positioned(Position::Absolute, TransitionBehavior::AllowDiscrete),
        0.0,
    );
    assert!(e.animations.is_running());
    e.tick(400.0);
    assert_eq!(e.style.position, Position::Static);
    e.tick(600.0);
    assert_eq!(e.style.position, Position::Absolute);
    // `auto` against a length too: a step of `width`.
    let mut auto = (*styled(0.0, "width", 1.0, 0.0)).clone();
    auto.width = Size::Auto;
    auto.transitions.behavior = Arc::new([TransitionBehavior::AllowDiscrete]);
    let mut wide = (*styled(100.0, "width", 1.0, 0.0)).clone();
    wide.transitions.behavior = Arc::new([TransitionBehavior::AllowDiscrete]);
    let mut e = Element::new(Arc::new(auto));
    e.change(Arc::new(wide), 0.0);
    assert!(e.animations.is_running());
    e.tick(400.0);
    assert_eq!(e.style.width, Size::Auto);
    e.tick(600.0);
    assert_eq!(width(&e.style), 100.0);
}

/// A new transition is pending until a frame shows it: at its start
/// value, sending only `transitionrun` (and `transitioncancel` if canceled
/// then); then its time counts from that frame's, however late (script
/// started it between frames, a while after the last).
#[test]
fn pending_until_a_frame() {
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.animations.style_change(
        &e.style.clone(),
        &styled(100.0, "width", 1.0, 0.0),
        0.0,
        &mut e.order,
        &mut e.events,
    );
    e.base = styled(100.0, "width", 1.0, 0.0);
    assert_eq!(e.animations.transitions[0].playback.start_time, None);
    assert_eq!(
        width(&e.animations.apply(5000.0, &e.base)),
        0.0,
        "at its start"
    );
    e.animations.tick(5000.0, &mut e.events);
    assert_eq!(e.take_events(), [(Run, 0.0)], "idle to pending");
    e.animations.tick(5100.0, &mut e.events);
    assert_eq!(e.take_events(), [], "it waits");
    assert!(e.animations.commit_pending(5000.0, &mut e.events));
    assert!(
        !e.animations.commit_pending(6000.0, &mut e.events),
        "started already"
    );
    e.tick(5250.0);
    assert_eq!(width(&e.style), 25.0);
    assert_eq!(e.take_events(), [(Start, 0.0)]);
    // Canceled while pending, once it ran.
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    let after = styled(100.0, "width", 1.0, 0.0);
    e.animations
        .style_change(&e.style.clone(), &after, 0.0, &mut e.order, &mut e.events);
    e.animations.tick(16.0, &mut e.events);
    e.animations.cancel_all(16.0, &mut e.events);
    assert_eq!(e.take_events(), [(Run, 0.0), (Cancel, 0.0)]);
}

/// Before its delay it holds the start value (it fills backwards).
#[test]
fn delays() {
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.5));
    e.change(styled(100.0, "width", 1.0, 0.5), 0.0);
    e.tick(250.0);
    assert_eq!(width(&e.style), 0.0);
    assert_eq!(e.take_events(), [(Run, 0.0)]);
    e.tick(1000.0);
    assert_eq!(width(&e.style), 50.0);
    assert_eq!(e.take_events(), [(Start, 0.0)]);
    // A negative delay starts partway through: the events' elapsed time
    // too.
    let mut e = Element::new(styled(0.0, "width", 1.0, -0.25));
    e.change(styled(100.0, "width", 1.0, -0.25), 0.0);
    assert_eq!(width(&e.style), 25.0);
    e.tick(0.0);
    assert_eq!(e.take_events(), [(Run, 0.25), (Start, 0.25)]);
    e.tick(750.0);
    assert_eq!(e.take_events(), [(End, 1.0)]);
}

/// A new end value while running: canceled, and a new one from the
/// current value.
#[test]
fn a_new_end_value_replaces_the_transition() {
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(500.0);
    e.take_events();
    e.change(styled(200.0, "width", 1.0, 0.0), 500.0);
    assert_eq!(e.take_events(), [(Cancel, 0.5)]);
    assert_eq!(width(&e.style), 50.0, "from where it was");
    e.tick(1000.0);
    assert_eq!(width(&e.style), 125.0);
    assert_eq!(e.animations.transitions.len(), 1);
    // The same end value leaves it running.
    e.change(styled(200.0, "width", 1.0, 0.0), 1000.0);
    assert!(e.take_events().iter().all(|(kind, _)| *kind != Cancel));
    assert_eq!(e.animations.transitions[0].playback.start_time, Some(500.0));
}

/// Back to the start value while running: as far back as it got, in as
/// much of the duration (the reversing shortening factor).
#[test]
fn reversing_is_shorter() {
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(250.0);
    e.change(styled(0.0, "width", 1.0, 0.0), 250.0);
    let t = &e.animations.transitions[0];
    assert_eq!(t.timing.duration, 250.0, "a quarter of the way back");
    e.tick(375.0);
    assert_eq!(width(&e.style), 12.5);
    e.tick(500.0);
    assert_eq!(width(&e.style), 0.0);
    assert!(!e.animations.is_running());
    // Reversing a reversal: by the product of how far each got.
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(500.0);
    e.change(styled(0.0, "width", 1.0, 0.0), 500.0);
    assert_eq!(e.animations.transitions[0].timing.duration, 500.0);
    e.tick(750.0);
    e.change(styled(100.0, "width", 1.0, 0.0), 750.0);
    // Halfway through the 0.5 back: 0.5 * 0.5 + 1 - 0.5.
    assert_eq!(e.animations.transitions[0].timing.duration, 750.0);
}

#[test]
fn cancels() {
    // No longer listed.
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(200.0);
    e.take_events();
    e.change(styled(100.0, "none", 1.0, 0.0), 300.0);
    assert!(!e.animations.is_running());
    assert_eq!(e.take_events(), [(Cancel, 0.3)]);
    // Back to its current value.
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(500.0);
    e.take_events();
    e.change(styled(50.0, "width", 1.0, 0.0), 500.0);
    assert!(!e.animations.is_running());
    assert_eq!(e.take_events(), [(Cancel, 0.5)]);
    // A duration of 0 now.
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(500.0);
    e.take_events();
    e.change(styled(70.0, "width", 0.0, 0.0), 500.0);
    assert!(!e.animations.is_running());
    // Everything at once; one never ticked sends nothing.
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.animations.cancel_all(0.0, &mut e.events);
    assert_eq!(e.take_events(), []);
    let mut e = Element::new(styled(0.0, "width", 1.0, 0.0));
    e.change(styled(100.0, "width", 1.0, 0.0), 0.0);
    e.tick(100.0);
    e.take_events();
    e.animations.cancel_all(600.0, &mut e.events);
    assert_eq!(e.take_events(), [(Cancel, 0.6)]);
    assert!(e.animations.is_empty());
}

/// `all`, a shorthand's longhands, a flow-relative longhand's physical
/// one, and the last entry naming a property: which transitions match.
#[test]
fn matching_names() {
    let margins = |px: f32, properties: &[&str], durations: &[f32]| {
        let mut style = ComputedStyle::initial();
        style.margin.left =
            computed_style::LengthPercentageOrAuto::LengthPercentage(LengthPercentage::Px(px));
        style.margin.top = style.margin.left;
        style.transitions = transitions(properties, 1.0, 0.0);
        style.transitions.duration = durations.into();
        Arc::new(style)
    };
    let started = |properties: &[&str], durations: &[f32]| {
        let mut e = Element::new(margins(0.0, properties, durations));
        e.change(margins(10.0, properties, durations), 0.0);
        let mut found: Vec<(LonghandId, f64)> = e
            .animations
            .transitions
            .iter()
            .map(|t| {
                let Owner::Transition(owner) = &t.owner else {
                    panic!("a transition");
                };
                (owner.property, t.timing.duration)
            })
            .collect();
        found.sort_by_key(|(id, _)| id.index());
        found
    };
    use LonghandId::{MarginLeft, MarginTop};
    assert_eq!(
        started(&["all"], &[1.0]),
        [(MarginTop, 1000.0), (MarginLeft, 1000.0)]
    );
    assert_eq!(
        started(&["margin"], &[1.0]),
        [(MarginTop, 1000.0), (MarginLeft, 1000.0)]
    );
    assert_eq!(
        started(&["margin-inline-start"], &[1.0]),
        [(MarginLeft, 1000.0)]
    );
    assert_eq!(
        started(&["MARGIN-LEFT"], &[1.0]),
        [(MarginLeft, 1000.0)],
        "any case"
    );
    assert_eq!(
        started(&["bogus", "margin-top"], &[1.0]),
        [(MarginTop, 1000.0)]
    );
    // The lists repeat; the last entry naming a property wins.
    assert_eq!(
        started(&["margin", "margin-left"], &[1.0, 2.0]),
        [(MarginTop, 1000.0), (MarginLeft, 2000.0)]
    );
    assert_eq!(
        started(&["margin-left", "margin", "margin-top"], &[1.0, 2.0]),
        [(MarginTop, 1000.0), (MarginLeft, 2000.0)]
    );
}

/// Transitions apply in the order they started; the events of a frame
/// come in the order they were scheduled.
#[test]
fn events_go_in_order() {
    let mut a = ComputedStyle::initial();
    a.transitions = transitions(&["opacity", "width"], 1.0, 0.0);
    a.transitions.delay = Arc::new([0.5, 0.0]);
    a.width = Size::LengthPercentage(LengthPercentage::Px(0.0));
    let mut b = a.clone();
    b.opacity = 0.0;
    b.width = Size::LengthPercentage(LengthPercentage::Px(10.0));
    let mut e = Element::new(Arc::new(a));
    e.change(Arc::new(b), 0.0);
    e.tick(2000.0);
    let mut events = std::mem::take(&mut e.events);
    events.sort_by(AnimationEvent::dispatch_order);
    let seen: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            AnimationEvent::Transition(e) => Some((e.kind, e.property)),
            AnimationEvent::Animation(e) => panic!("no CSS animations here: {e:?}"),
            AnimationEvent::Playback(_) => None,
        })
        .collect();
    use LonghandId::{Opacity, Width};
    assert_eq!(
        seen,
        [
            (Run, Opacity),
            (Run, Width),
            (Start, Width),
            (Start, Opacity),
            (End, Width),
            (End, Opacity),
        ]
    );
    assert_eq!(TransitionEventKind::End.name(), "transitionend");
}
