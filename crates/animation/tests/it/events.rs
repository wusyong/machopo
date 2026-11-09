//! The order animation events are dispatched in (web-animations 1,
//! "update animations and send events"): by when they were scheduled,
//! then transitions' before CSS animations', each class in the order they
//! were made.

use std::sync::Arc;

use animation::{
    AnimationEvent, CssAnimationEvent, CssAnimationEventKind, TransitionEvent, TransitionEventKind,
};
use css::properties::LonghandId;

fn transition(at: f64, order: u64) -> AnimationEvent {
    AnimationEvent::Transition(TransitionEvent {
        kind: TransitionEventKind::End,
        property: LonghandId::Width,
        elapsed_time: 0.0,
        scheduled_time: at,
        order,
    })
}

fn animation(at: f64, order: u64) -> AnimationEvent {
    AnimationEvent::Animation(CssAnimationEvent {
        kind: CssAnimationEventKind::End,
        name: Arc::from("k"),
        elapsed_time: 0.0,
        scheduled_time: at,
        order,
    })
}

#[test]
fn by_time_then_class_then_order() {
    let mut events = vec![
        animation(10.0, 1),
        transition(10.0, 5),
        animation(5.0, 9),
        transition(10.0, 2),
        animation(10.0, 0),
    ];
    events.sort_by(AnimationEvent::dispatch_order);
    assert_eq!(
        events,
        [
            animation(5.0, 9),
            transition(10.0, 2),
            transition(10.0, 5),
            animation(10.0, 0),
            animation(10.0, 1),
        ]
    );
    assert_eq!(events[0].name(), Some("animationend"));
    assert_eq!(events[1].name(), Some("transitionend"));
}
