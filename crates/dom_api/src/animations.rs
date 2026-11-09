//! CSS transitions' and animations' events (css-transitions 1's
//! `TransitionEvent`, css-animations 1's `AnimationEvent`), and the
//! rendering update's "update animations and send events" for a document
//! (Blink's `DocumentAnimations::UpdateAnimationTimingForAnimationFrame`
//! and the events its animations queue): `dom` moves its timeline on
//! and keeps the events its animations sent, and style starts and stops
//! them (`style`); this dispatches the events, as trusted ones that
//! bubble and can be canceled, at their elements, in css-transitions 2's
//! and css-animations 2's order.

use dom::{
    Event, EventFlags, NodeIdHashMap, NodeKind, NodeRef, NodeRoot, PendingAnimationEvent,
    TargetRoot, names,
};
use elise::{Gc, Persistent};

use crate::interfaces::{AnimationEventInit, TransitionEventInit};
use crate::typed::{DomStr, ScriptHost};
use crate::{ApiCx, ApiError, OpFront};

/// `new TransitionEvent(type, eventInitDict)`: an event, as `new Event`'s,
/// with the init's members.
pub(crate) fn construct_transition_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &TransitionEventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = untrusted(
        op,
        cx,
        event_type,
        init.bubbles,
        init.cancelable,
        init.composed,
    );
    cx.set_animation_event_data(
        event,
        true,
        &init.propertyName,
        init.elapsedTime,
        &init.pseudoElement,
    );
    Ok(event)
}

/// `new AnimationEvent(type, eventInitDict)`.
pub(crate) fn construct_animation_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &AnimationEventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = untrusted(
        op,
        cx,
        event_type,
        init.bubbles,
        init.cancelable,
        init.composed,
    );
    cx.set_animation_event_data(
        event,
        false,
        &init.animationName,
        init.elapsedTime,
        &init.pseudoElement,
    );
    Ok(event)
}

fn untrusted<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    composed: bool,
) -> Gc<'gc, Event> {
    cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles,
            cancelable,
            composed,
            trusted: false,
        },
        op.current_window(cx),
    )
}

fn fields<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> dom::AnimationEventFields<'gc> {
    cx.animation_event_data(event)
        .expect("the front end's checks: a TransitionEvent or an AnimationEvent")
}

/// `TransitionEvent.propertyName` and `AnimationEvent.animationName`.
pub(crate) fn event_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(fields(cx, this).name.clone()))
}

pub(crate) fn elapsed_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<f64, ApiError> {
    Ok(fields(cx, this).elapsed_time)
}

pub(crate) fn pseudo_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(fields(cx, this).pseudo_element.clone()))
}

/// One of the events the update of animations sends: a CSS one at its
/// element, or a playback's at its animation's object.
enum Dispatch {
    Event(TargetRoot, Persistent<Event>),
    Playback(PendingAnimationEvent),
}

/// What a document's update of animations found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AnimationsUpdate {
    /// They moved on, or sent events: the rendering shows what changed.
    pub changed: bool,
    /// They still run, or events wait: the next frame moves them on.
    pub running: bool,
}

/// HTML's "update animations and send events" for `document`, at `now`
/// (its timeline's time, in milliseconds): its animations move on (those
/// of elements that left it canceled), then the events they and style
/// queued since are dispatched, in order, a microtask checkpoint after
/// each.
pub fn update_animations<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    now: f64,
) -> AnimationsUpdate {
    let (moved_on, events): (bool, Vec<Dispatch>) = host.mutate(|cx| {
        let document = document.get(cx.gc);
        let moved_on = cx.tick_animations(document, now);
        let mut pending = cx.take_animation_events(document);
        sort_events(cx, document, &mut pending);
        let events = pending
            .into_iter()
            .filter_map(|pending| {
                // A playback's: its promises settled first (below), then its
                // event at its object, whether or not the animation has an
                // element.
                if let animation::AnimationEvent::Playback(playback) = &pending.event {
                    if !playback.promises_settled
                        && let Some(object) = cx.existing_animation_object(document, playback.id)
                    {
                        cx.push_animation_notice(object, playback.notice);
                    }
                    return Some(Dispatch::Playback(pending.clone()));
                }
                let element = cx.node_by_id(pending.element?)?;
                let (name, transition, elapsed_time, property) = match &pending.event {
                    animation::AnimationEvent::Transition(event) => (
                        event.kind.name(),
                        true,
                        event.elapsed_time,
                        event.property.name().to_owned(),
                    ),
                    animation::AnimationEvent::Animation(event) => (
                        event.kind.name(),
                        false,
                        event.elapsed_time,
                        event.name.to_string(),
                    ),
                    animation::AnimationEvent::Playback(_) => return None,
                };
                // css-transitions 1 and css-animations 1: they bubble, and
                // can be canceled.
                let event = cx.create_event(
                    names::intern(name),
                    EventFlags {
                        bubbles: true,
                        cancelable: true,
                        composed: false,
                        trusted: true,
                    },
                    cx.window_of(document),
                );
                let pseudo_element = match pending.pseudo {
                    None => "",
                    Some(::computed_style::PseudoId::Before) => "::before",
                    Some(::computed_style::PseudoId::After) => "::after",
                    Some(::computed_style::PseudoId::Marker) => "::marker",
                };
                cx.set_animation_event_data(
                    event,
                    transition,
                    &property,
                    elapsed_time,
                    pseudo_element,
                );
                Some(Dispatch::Event(
                    TargetRoot::Node(cx.root(element)),
                    Persistent::new(cx.gc, event),
                ))
            })
            .collect();
        (moved_on, events)
    });
    let sent = !events.is_empty();
    // The playbacks' promises first, as their finish notification steps
    // ran when they happened (a microtask then), before this update sends
    // their events.
    crate::animation::settle_animation_notices(host);
    host.perform_microtask_checkpoint();
    // A microtask checkpoint after each (HTML's "clean up after running
    // script" once the stack is empty): what a listener's promise does runs
    // before the next event, as for the events the UA fires one by one.
    for dispatch in events {
        match dispatch {
            Dispatch::Event(target, event) => {
                crate::event_target::dispatch(host, &target, &event);
            }
            Dispatch::Playback(pending) => {
                crate::animation::playback_event(host, document, &pending)
            }
        }
        host.perform_microtask_checkpoint();
    }
    host.mutate(|cx| cx.prune_animations(document.get(cx.gc)));
    AnimationsUpdate {
        changed: moved_on || sent,
        running: host.mutate(|cx| cx.animations_want_frames(document.get(cx.gc))),
    }
}

/// The order the update of animations sends `document`'s events in
/// (css-transitions 2's and css-animations 2's "event dispatch"): by when
/// they were scheduled, transitions' before CSS animations', then by their
/// targets' composite order (tree order, an element before its
/// `::marker`, `::before`, `::after`), then by the animations' own.
fn sort_events<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    events: &mut [PendingAnimationEvent],
) {
    // One target's (most often): their own order.
    let places = if events
        .windows(2)
        .any(|pair| pair[0].element != pair[1].element)
    {
        tree_places(cx, document, true)
    } else {
        NodeIdHashMap::default()
    };
    let key = |pending: &PendingAnimationEvent| {
        let (time, class, order) = pending.event.order_key();
        let pseudo = match pending.pseudo {
            None => 0,
            Some(::computed_style::PseudoId::Marker) => 1,
            Some(::computed_style::PseudoId::Before) => 2,
            Some(::computed_style::PseudoId::After) => 3,
        };
        let place = pending
            .element
            .and_then(|element| places.get(&element).copied())
            .unwrap_or(usize::MAX);
        (time, class, (place, pseudo), order)
    };
    events.sort_by(|a, b| {
        let (a, b) = (key(a), key(b));
        a.0.total_cmp(&b.0)
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
}

/// The elements of `root`'s subtree (`root` too), by their places in tree
/// order; into shadow trees too if `shadow_including` (a shadow root after
/// its host, before the host's children).
pub(crate) fn tree_places<'gc>(
    cx: &ApiCx<'_, 'gc>,
    root: NodeRef<'gc>,
    shadow_including: bool,
) -> NodeIdHashMap<usize> {
    let mut places = NodeIdHashMap::default();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        let element = cx.kind(node) == NodeKind::Element;
        if element {
            let place = places.len();
            places.insert(cx.node_id(node), place);
        }
        let first = stack.len();
        let mut child = cx.first_child(node);
        while let Some(at) = child {
            stack.push(at);
            child = cx.next_sibling(at);
        }
        stack[first..].reverse();
        if element
            && shadow_including
            && let Some(shadow_root) = cx.shadow_root(node)
        {
            stack.push(shadow_root);
        }
    }
    places
}
