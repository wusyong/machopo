//! Web Animations' script API (web-animations 1; Blink's `core/animation`
//! bindings): `document.timeline`,
//! `getAnimations()`, `element.animate()`, an `Animation`'s members (and
//! `CSSTransition`'s, `CSSAnimation`'s), `AnimationPlaybackEvent`, and
//! what its playback's notices do to its promises and `finish` and
//! `cancel` events.
//!
//! An animation's object holds its number (`dom`'s
//! `animation_objects`): each member finds the animation on its element
//! (or off any) and changes it there; the element is styled again with it
//! at the next style update. A CSS animation's or transition's members
//! bring style up to date first, as Blink's `CSSAnimation` and
//! `CSSTransition` do: their state is style's output. What a member's
//! playback does to the promises is queued on the object, in order, and
//! the engine settles it before the call returns (`dom::Microtasks`'
//! animations bit, `settle_animation_notices`), or before it gives a
//! promise.

use std::sync::Arc;
use std::time::Instant;

use animation::{
    AnimationEvent, KeyframeEffect, Notice, Owner, PlayState, PlaybackError, ScriptKeyframes,
};
use computed_style::{ComputedStyle, PseudoId};
use dom::{
    AnimationClass, AnimationPlace, DocumentTimeline, Event, EventFlags, EventTargetImpl,
    KeyframeEffectObject, NodeIdHashMap, NodeRef, PendingAnimationEvent, TargetRoot, names,
};
use elise::{Gc, Persistent};

use crate::interfaces::{AnimationPlaybackEventInit, GetAnimationsOptions};
use crate::typed::{AnimationOptions, AnimationPromise, DomStr, ScriptHost};
use crate::{ApiCx, ApiError, DomException, ExceptionName, OpFront};

/// What `this`, an animation's object, stands for: its number, its class
/// and its document, whose timeline is started.
fn of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> (u64, AnimationClass, NodeRef<'gc>) {
    let (id, class, document) = cx
        .animation_of_object(this)
        .expect("the table only calls an animation's member on its object");
    start_timeline(cx, document);
    (id, class, document)
}

/// `document`'s timeline's time, if it has one: a frame's, or before the
/// first frame that updates its animations, its window's clock now (kept
/// till then). `None` for a document with no window.
fn start_timeline<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>) -> Option<f64> {
    if let Some(time) = cx.timeline_time(document) {
        return Some(time);
    }
    let window = cx.window_of(document)?;
    Some(cx.start_timeline(document, cx.time_at(window, Instant::now())))
}

/// `f` on `this`'s animation at its timeline's time: its playback's
/// notices go to its promises and its events to the next update of
/// animations (`notify`), its element styled again; `None` if it is gone
/// (script dropped it and it was let go of). A disassociated one run again
/// goes last in the global animation list.
fn with_animation<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    f: impl FnOnce(&mut animation::Animation, f64, &mut Vec<AnimationEvent>) -> R,
) -> Option<R> {
    let (id, _, document) = of(cx, this);
    let now = cx.animation_time(document);
    let hint = cx.animation_object_target(this);
    let mut sent = Vec::new();
    let (result, place, renumber) = cx.with_animation(document, id, hint, |animation, place| {
        let result = f(animation, now, &mut sent);
        let renumber = animation.needs_global_order && animation.play_state(now) != PlayState::Idle;
        (result, place, renumber)
    })?;
    if renumber {
        let order = cx.next_animation_number();
        cx.with_animation(document, id, hint, |animation, _| {
            animation.global_order = order;
            animation.needs_global_order = false;
        });
    }
    notify(cx, this, document, place, sent);
    Some(result)
}

/// Reads `this`'s animation at its timeline's time; `None` if it is gone.
fn read_animation<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    f: impl FnOnce(&animation::Animation, f64) -> R,
) -> Option<R> {
    let (id, _, document) = of(cx, this);
    let now = cx.animation_time(document);
    cx.read_animation(
        document,
        id,
        cx.animation_object_target(this),
        |animation, _| f(animation, now),
    )
}

/// What a member did: its playback's notices queued for its promises (in
/// order, settled before the call returns), its events queued for the
/// next update of animations (a `finish` or `cancel` at its object, its
/// promises already settled), a CSS animation's in its place in
/// `animation-name`, as those of its ticks.
fn notify<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    document: NodeRef<'gc>,
    place: AnimationPlace<'gc>,
    sent: Vec<AnimationEvent>,
) {
    let (element, pseudo) = match place {
        AnimationPlace::Target(element, pseudo) => {
            cx.set_animation_object_target(this, Some(element));
            (Some(element), pseudo)
        }
        AnimationPlace::Detached => {
            cx.set_animation_object_target(this, None);
            (None, None)
        }
    };
    let (id, _, _) = of(cx, this);
    let position = element.and_then(|element| {
        cx.read_animations(element, |targets| {
            targets[slot(pseudo)]
                .as_ref()?
                .animations
                .css_animations
                .animations
                .iter()
                .position(|animation| animation.id == id)
        })
        .flatten()
    });
    let mut queued = Vec::new();
    for mut event in sent {
        if let (AnimationEvent::Animation(event), Some(position)) = (&mut event, position) {
            event.order = u64::try_from(position).unwrap_or(u64::MAX);
        }
        if let AnimationEvent::Playback(playback) = &mut event {
            cx.push_animation_notice(this, playback.notice);
            if !matches!(
                playback.notice,
                Notice::Finished { .. } | Notice::Canceled { .. }
            ) {
                continue;
            }
            playback.promises_settled = true;
        }
        queued.push(event);
    }
    match element {
        Some(element) => cx.queue_animation_events(element, pseudo, queued),
        None => cx.queue_detached_animation_events(document, queued),
    }
}

fn slot(pseudo: Option<PseudoId>) -> usize {
    match pseudo {
        None => 0,
        Some(PseudoId::Before) => 1,
        Some(PseudoId::After) => 2,
        Some(PseudoId::Marker) => 3,
    }
}

fn abort_error() -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::Abort,
        message: "the animation was canceled".into(),
    })
}

/// The script error a playback procedure threw.
fn playback_error(error: PlaybackError) -> ApiError {
    match error {
        PlaybackError::InvalidState => ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: "the animation can't do that in its state".into(),
        }),
    }
}

/// What a member does first: a CSS animation's or transition's
/// document's style brought up to date, then the notices its animation's
/// playback sent since that wait for the next update of animations queued
/// for its promises now (style's: it started, paused or canceled the
/// animation), in order, so that the member sees them as they are
/// (`ready` after style replaced it is the new one). Their `finish` and
/// `cancel` events still wait.
fn prepare<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, EventTargetImpl>) {
    let (id, class, document) = of(cx, this);
    if class != AnimationClass::Script {
        cx.with_style(document, |_| ());
    }
    for notice in cx.take_unsettled_playback_notices(document, id) {
        cx.push_animation_notice(this, notice);
    }
}

/// Runs a playback procedure on `this`'s animation; nothing if it is gone.
fn control<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    step: impl FnOnce(
        &mut animation::Playback,
        Option<f64>,
        f64,
        &mut Vec<Notice>,
    ) -> Result<(), PlaybackError>,
) -> Result<(), ApiError> {
    with_animation(cx, this, |animation, now, sent| {
        animation.control(now, sent, step)
    })
    .unwrap_or(Ok(()))
    .map_err(playback_error)
}

fn is_paused<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, EventTargetImpl>) -> bool {
    read_animation(cx, this, |a, now| a.play_state(now) == PlayState::Paused).unwrap_or(false)
}

/// After a `reverse()` or a new `startTime` that took a CSS animation into
/// or out of the paused state (`was_paused` before it),
/// `animation-play-state` no longer plays or pauses it (css-animations 2).
fn override_if_pause_changed<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    was_paused: bool,
) {
    if is_paused(cx, this) != was_paused {
        override_play_state(cx, this);
    }
}

/// Script played or paused a CSS animation: `animation-play-state` no
/// longer does (css-animations 2: after a successful `play()` or
/// `pause()`).
fn override_play_state<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, EventTargetImpl>) {
    with_animation(cx, this, |animation, _, _| {
        if let Owner::CssAnimation(owner) = &mut animation.owner {
            owner.play_state_overridden = true;
        }
    });
}

// --- `DocumentTimeline`

/// `document.timeline` (`[SameObject]`).
pub(crate) fn document_timeline<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DocumentTimeline>, ApiError> {
    Ok(cx.document_timeline(this))
}

/// `currentTime`: its document's frame's animation time, in milliseconds.
pub(crate) fn timeline_current_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DocumentTimeline>,
) -> Result<Option<f64>, ApiError> {
    Ok(start_timeline(cx, cx.timeline_document(this)))
}

// --- `getAnimations()`

/// Whether an animation is relevant at `now` (web-animations 1): its
/// effect is current (before or in its active interval, as it plays) or in
/// effect (filling).
fn is_relevant(animation: &animation::Animation, now: f64) -> bool {
    if animation.current_time(now).is_none()
        || animation.replace_state == animation::ReplaceState::Removed
    {
        return false;
    }
    let timing = animation.computed(now);
    let rate = animation.playback.effective_playback_rate();
    let current = match timing.phase {
        animation::Phase::Active => true,
        animation::Phase::Before => rate > 0.0,
        animation::Phase::After => rate < 0.0,
    };
    current || timing.active_time.is_some()
}

/// The relevant animations of the elements `places` lists (and of their
/// pseudo-elements, if `pseudo_elements`), of `document`, as objects, in
/// composite order: transitions, then CSS animations, each by element;
/// then a transition by its generation (the style change that started
/// it) and its property's name, a CSS animation by its place in
/// `animation-name`; then script's and disassociated ones by their places
/// in the global animation list.
fn animations_of<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    places: &NodeIdHashMap<usize>,
    pseudo_elements: bool,
) -> Vec<Gc<'gc, EventTargetImpl>> {
    start_timeline(cx, document);
    cx.with_style(document, |_| ());
    let now = cx.animation_time(document);
    let mut found = Vec::new();
    for element in cx.elements_with_animations(document) {
        let Some(&place) = places.get(&cx.node_id(element)) else {
            continue;
        };
        cx.read_animations(element, |targets| {
            for (index, target) in targets.iter().enumerate() {
                let Some(target) = target else {
                    continue;
                };
                if index > 0 && !pseudo_elements {
                    continue;
                }
                let pseudo_order: u8 = match index {
                    0 => 0,
                    3 => 1,
                    1 => 2,
                    _ => 3,
                };
                let order = (place, pseudo_order);
                let animations = &target.animations;
                for (position, animation) in animations.css_animations.animations.iter().enumerate()
                {
                    if is_relevant(animation, now) {
                        let position = u64::try_from(position).unwrap_or(u64::MAX);
                        found.push((
                            1,
                            order,
                            position,
                            "",
                            animation.id,
                            AnimationClass::CssAnimation,
                            element,
                        ));
                    }
                }
                for animation in &animations.others {
                    if !is_relevant(animation, now) {
                        continue;
                    }
                    match &animation.owner {
                        // Done, still its owning element's.
                        Owner::Transition(owner) if animation.owning => found.push((
                            0,
                            order,
                            owner.generation,
                            owner.property.name(),
                            animation.id,
                            AnimationClass::CssTransition,
                            element,
                        )),
                        // Script's, or disassociated: by their places in
                        // the global animation list, whatever their
                        // targets.
                        _ => found.push((
                            2,
                            (0, 0),
                            animation.global_order,
                            "",
                            animation.id,
                            AnimationClass::of(animation),
                            element,
                        )),
                    }
                }
                for animation in &animations.transitions {
                    let Owner::Transition(owner) = &animation.owner else {
                        continue;
                    };
                    if is_relevant(animation, now) {
                        found.push((
                            0,
                            order,
                            owner.generation,
                            owner.property.name(),
                            animation.id,
                            AnimationClass::CssTransition,
                            element,
                        ));
                    }
                }
            }
        });
    }
    found.sort_by_key(|&(class, order, place, name, id, _, _)| (class, order, place, name, id));
    found
        .into_iter()
        .map(|(_, _, _, _, id, class, element)| {
            cx.animation_object(document, id, class, Some(element))
        })
        .collect()
}

/// `element.getAnimations(options)`: its own, or with `subtree` its
/// pseudo-elements' and its descendants' (and theirs) too, not those in
/// shadow trees.
pub(crate) fn element_get_animations<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &GetAnimationsOptions,
) -> Result<Vec<Gc<'gc, EventTargetImpl>>, ApiError> {
    let places = if options.subtree {
        crate::animations::tree_places(cx, this, false)
    } else {
        NodeIdHashMap::from_iter([(cx.node_id(this), 0)])
    };
    Ok(animations_of(
        cx,
        cx.document_of(this),
        &places,
        options.subtree,
    ))
}

/// `document.getAnimations()` and a shadow root's: those of the elements
/// of its tree (not of shadow trees in it, as Blink's
/// `GetAnimationsForTreeScope`).
pub(crate) fn tree_get_animations<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Vec<Gc<'gc, EventTargetImpl>>, ApiError> {
    let places = crate::animations::tree_places(cx, this, false);
    Ok(animations_of(cx, cx.document_of(this), &places, true))
}

// --- `Animation`

/// `new Animation(effect, timeline)`: script's, idle, of the realm's
/// document; the document's timeline unless `timeline` is `null`. (An
/// effect's object comes from another animation's `effect`: it moves.)
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    effect: Option<Gc<'gc, KeyframeEffectObject>>,
    has_timeline: bool,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("no window"))?;
    let document = cx.window_document(window);
    Ok(make_animation(cx, document, effect, has_timeline))
}

/// An effect taken for an animation: its number, keyframes, timing (and
/// how script wrote it) and target.
pub(crate) struct TakenEffect<'gc> {
    id: u64,
    keyframes: Arc<KeyframeEffect>,
    timing: animation::Timing,
    input: animation::TimingInput,
    target: Option<(NodeRef<'gc>, Option<PseudoId>)>,
}

/// Takes the effect `effect` stands for from where it is: its object (no
/// animation has it), or the animation that has it (left without one: a
/// script's, or one style no longer runs, then has no target).
fn take_effect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    effect: Gc<'gc, KeyframeEffectObject>,
) -> Option<TakenEffect<'gc>> {
    let (effect_id, _) = cx.effect_of_object(effect);
    if let Some((free, element)) = cx.take_free_effect(effect) {
        let target = element.map(|element| (element, free.input.pseudo));
        return Some(TakenEffect {
            id: effect_id,
            keyframes: free.effect,
            timing: free.timing,
            input: free.input,
            target,
        });
    }
    let (animation, target) = cx.find_effect(document, effect_id)?;
    let hint = target.map(|(element, _)| element);
    let (keyframes, timing, input, moves) =
        cx.with_animation(document, animation, hint, |animation, _| {
            let effect =
                std::mem::replace(&mut animation.effect, Arc::new(KeyframeEffect::empty()));
            let timing = std::mem::take(&mut animation.timing);
            let input = std::mem::take(&mut animation.timing_input);
            animation.effect_id = None;
            let moves = matches!(animation.owner, Owner::Script) || !animation.owning;
            (effect, timing, input, moves)
        })?;
    // Without an effect it has no target.
    if moves && let Some(moved) = cx.take_animation(document, animation, hint) {
        cx.put_animation(document, None, moved, base_style_fn(cx, None));
        if let Some(object) = cx.existing_animation_object(document, animation) {
            cx.set_animation_object_target(object, None);
        }
    }
    update_finished_state(cx, document, animation);
    Some(TakenEffect {
        id: effect_id,
        keyframes,
        timing,
        input,
        target,
    })
}

/// The animation of `document` numbered `id`'s finished state updated
/// now (its effect's timing changed): its notices go to its object's
/// promises, its events to the next update of animations.
pub(crate) fn update_finished_state<'gc>(cx: &ApiCx<'_, 'gc>, document: NodeRef<'gc>, id: u64) {
    let now = cx.animation_time(document);
    let mut sent = Vec::new();
    let Some(place) = cx.with_animation(document, id, None, |animation, place| {
        let _ = animation.control(now, &mut sent, |playback, at, end, notices| {
            playback.update_finished_state(false, false, at, end, notices);
            Ok(())
        });
        place
    }) else {
        return;
    };
    if let Some(object) = cx.existing_animation_object(document, id) {
        notify(cx, object, document, place, sent);
    }
}

/// The style an animation put on `target` starts from: its element's (or
/// pseudo-element's) style as last computed, the initial one if none.
pub(crate) fn base_style_fn<'gc, 'a>(
    cx: &'a ApiCx<'_, 'gc>,
    target: Option<(NodeRef<'gc>, Option<PseudoId>)>,
) -> impl FnOnce() -> Arc<ComputedStyle> + 'a {
    move || {
        target
            .and_then(|(element, pseudo)| {
                cx.peek_layout(element, |layout| match pseudo {
                    None => layout.computed_style(cx, element),
                    Some(pseudo) => layout.pseudo_style(cx, element, pseudo),
                })
                .flatten()
            })
            .unwrap_or_else(|| Arc::new(ComputedStyle::initial()))
    }
}

fn make_animation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    effect: Option<Gc<'gc, KeyframeEffectObject>>,
    has_timeline: bool,
) -> Gc<'gc, EventTargetImpl> {
    start_timeline(cx, document);
    let taken = effect.and_then(|effect| take_effect(cx, document, effect));
    let id = cx.next_animation_number();
    let (mut animation, target) = match taken {
        Some(taken) => {
            let mut animation = animation::Animation::idle(
                id,
                Some(taken.id),
                taken.keyframes,
                taken.timing,
                Owner::Script,
            );
            animation.timing_input = taken.input;
            (animation, taken.target)
        }
        None => (
            animation::Animation::idle(
                id,
                None,
                Arc::new(KeyframeEffect::empty()),
                animation::Timing::default(),
                Owner::Script,
            ),
            None,
        ),
    };
    animation.has_timeline = has_timeline;
    cx.put_animation(document, target, animation, base_style_fn(cx, target));
    cx.animation_object(
        document,
        id,
        AnimationClass::Script,
        target.map(|(element, _)| element),
    )
}

/// `element.animate(keyframes, options)`: a new effect on it (its
/// keyframes processed: `animation::process_keyframes`, their `url()`s
/// resolved against its document's URL now), a new animation of it (its
/// `id`, its `timeline` the document's unless given `null`), played.
pub(crate) fn animate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    keyframes: Option<&ScriptKeyframes>,
    options: &AnimationOptions,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let document = cx.document_of(this);
    let mut timing = animation::default_timing();
    let mut input = animation::TimingInput::default();
    animation::apply_timing(&mut timing, &mut input, &options.timing)
        .map_err(ApiError::type_error)?;
    let pseudo = parse_pseudo(options.pseudo_element.as_deref())?;
    input.pseudo = pseudo;
    let effect = declared_effect(cx, document, keyframes)?;
    start_timeline(cx, document);
    let id = cx.next_animation_number();
    let mut animation = animation::Animation::idle(id, Some(id), effect, timing, Owner::Script);
    animation.has_timeline = options.has_timeline;
    animation.timing_input = input;
    let target = Some((this, pseudo));
    cx.put_animation(document, target, animation, base_style_fn(cx, target));
    let object = cx.animation_object(document, id, AnimationClass::Script, Some(this));
    if !options.id.is_empty() {
        cx.set_animation_object_name(object, &options.id);
    }
    play(cx, object)?;
    Ok(object)
}

/// An effect of script's `keyframes` (processed:
/// `animation::process_keyframes`), their `url()`s resolved against
/// `document`'s URL now (Chrome parses them with it).
pub(crate) fn declared_effect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    keyframes: Option<&ScriptKeyframes>,
) -> Result<Arc<KeyframeEffect>, ApiError> {
    let mut declared = match keyframes {
        Some(keyframes) => {
            animation::process_keyframes(keyframes.clone()).map_err(ApiError::type_error)?
        }
        None => Vec::new(),
    };
    if declared
        .iter()
        .any(|keyframe| keyframe.block.has_unresolved_urls())
        && let Some(href) = cx.document_url(document)
        && let Some(url) = cx.document_base_url(document)
    {
        let base = css::UrlContext::new(url, &href.to_str());
        for keyframe in &mut declared {
            Arc::make_mut(&mut keyframe.block).resolve_urls(&base);
        }
    }
    Ok(Arc::new(KeyframeEffect::declared_only(Arc::new(
        animation::DeclaredKeyframes {
            keyframes: declared,
        },
    ))))
}

/// `pseudoElement`'s value as a pseudo-element: none, `::marker`,
/// `::before`, `::after` (and CSS 2's `:before`, `:after`); anything else
/// a `SyntaxError`, those machopo doesn't have too.
pub(crate) fn parse_pseudo(value: Option<&str>) -> Result<Option<PseudoId>, ApiError> {
    let Some(value) = value else {
        return Ok(None);
    };
    Ok(match value {
        "::before" | ":before" => Some(PseudoId::Before),
        "::after" | ":after" => Some(PseudoId::After),
        "::marker" => Some(PseudoId::Marker),
        _ => {
            return Err(ApiError::Dom(DomException {
                name: ExceptionName::Syntax,
                message: "not a pseudo-element".into(),
            }));
        }
    })
}

pub(crate) fn id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.animation_object_name(this)))
}

pub(crate) fn set_id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_animation_object_name(this, value);
    Ok(())
}

/// `effect`: its effect's object, `null` without one.
pub(crate) fn effect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Option<Gc<'gc, KeyframeEffectObject>>, ApiError> {
    let (_, _, document) = of(cx, this);
    let effect = read_animation(cx, this, |a, _| a.effect_id).flatten();
    Ok(effect.map(|id| cx.effect_object(document, id)))
}

/// `timeline`: its document's, `null` without one.
pub(crate) fn timeline<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Option<Gc<'gc, DocumentTimeline>>, ApiError> {
    let (_, _, document) = of(cx, this);
    if read_animation(cx, this, |a, _| a.has_timeline) == Some(false) {
        return Ok(None);
    }
    Ok(Some(cx.document_timeline(document)))
}

/// Setting `effect` (web-animations 1's "set the associated effect"): the
/// one it had goes on without an animation (on its object); the new one
/// leaves where it was; script's animation moves to its target.
pub(crate) fn set_effect<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    value: Option<Gc<'gc, KeyframeEffectObject>>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    let (id, _, document) = of(cx, this);
    let new = value.map(|effect| cx.effect_of_object(effect).0);
    let old = read_animation(cx, this, |a, _| a.effect_id).flatten();
    if new == old {
        return Ok(());
    }
    if let Some(old) = old {
        let object = cx.effect_object(document, old);
        if let Some(taken) = take_effect(cx, document, object) {
            cx.put_free_effect(
                object,
                dom::FreeEffect {
                    effect: taken.keyframes,
                    timing: taken.timing,
                    input: taken.input,
                },
                taken.target.map(|(element, _)| element),
            );
        }
    }
    let taken = value.and_then(|effect| take_effect(cx, document, effect));
    let target = taken.as_ref().and_then(|taken| taken.target);
    with_animation(cx, this, |animation, _, _| {
        animation.effect_id = new;
        match taken {
            Some(taken) => {
                animation.effect = taken.keyframes;
                animation.timing = taken.timing;
                animation.timing_input = taken.input;
            }
            None => {
                animation.effect = Arc::new(KeyframeEffect::empty());
                animation.timing = animation::Timing::default();
                animation.timing_input = animation::TimingInput::default();
            }
        }
        // A CSS animation's effect script replaced is script's: style no
        // longer changes its keyframes or timing (css-animations 2).
        if let Owner::CssAnimation(owner) = &mut animation.owner {
            owner.keyframes_overridden = true;
            owner.timing_overrides = animation::TimingOverrides::ALL;
        }
    });
    let scripts = read_animation(cx, this, |a, _| {
        matches!(a.owner, Owner::Script) || !a.owning
    })
    .unwrap_or(false);
    if scripts
        && let Some(moved) = cx.take_animation(document, id, cx.animation_object_target(this))
    {
        cx.put_animation(document, target, moved, base_style_fn(cx, target));
        cx.set_animation_object_target(this, target.map(|(element, _)| element));
    }
    update_finished_state(cx, document, id);
    Ok(())
}

/// Setting `timeline`: the document's, or none.
pub(crate) fn set_timeline<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    value: Option<Gc<'gc, DocumentTimeline>>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    let has = value.is_some();
    with_animation(cx, this, |animation, now, sent| {
        animation.set_timeline(has, now, sent)
    });
    Ok(())
}

/// `replaceState`.
pub(crate) fn replace_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    prepare(cx, this);
    let state = read_animation(cx, this, |a, _| a.replace_state).unwrap_or_default();
    Ok(DomStr::fresh(match state {
        animation::ReplaceState::Active => "active",
        animation::ReplaceState::Removed => "removed",
        animation::ReplaceState::Persisted => "persisted",
    }))
}

/// `persist()`: never removed; one removed already gives its target values
/// again.
pub(crate) fn persist<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    with_animation(cx, this, |animation, _, _| {
        animation.replace_state = animation::ReplaceState::Persisted;
    });
    Ok(())
}

/// `commitStyles()` (web-animations 1's "commit computed styles"): the
/// values its effect's properties have now, in composite order up to it
/// (the removed animations too, if it was removed), written to its
/// target's inline style as computed values. A pseudo-element or an
/// element of neither HTML nor SVG is a `NoModificationAllowedError`, one
/// not rendered an `InvalidStateError`.
pub(crate) fn commit_styles<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    let (id, _, document) = of(cx, this);
    let Some(AnimationPlace::Target(element, pseudo)) = cx.read_animation(
        document,
        id,
        cx.animation_object_target(this),
        |_, place| place,
    ) else {
        return Ok(());
    };
    let not_styled = || {
        ApiError::Dom(DomException {
            name: ExceptionName::NoModificationAllowed,
            message: "the effect's target has no style attribute".into(),
        })
    };
    if pseudo.is_some() {
        return Err(not_styled());
    }
    let namespace = cx.qualified_name(element).map(|name| name.namespace());
    if namespace != Some(names::HTML_NAMESPACE) && namespace != Some(names::SVG_NAMESPACE) {
        return Err(not_styled());
    }
    let rendered = cx.is_connected(element)
        && cx
            .with_style(document, |layout| {
                let mut at = Some(element);
                while let Some(node) = at {
                    if cx.kind(node) == dom::NodeKind::Element
                        && layout
                            .computed_style(cx, node)
                            .is_none_or(|style| style.display == computed_style::Display::None)
                    {
                        return false;
                    }
                    at = cx.flat_tree_parent(node);
                }
                true
            })
            .unwrap_or(false);
    if !rendered {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: "the effect's target isn't rendered".into(),
        }));
    }
    let now = cx.animation_time(document);
    let values: Vec<(&'static str, String)> = cx
        .read_animations(element, |targets| {
            let target = targets[0].as_ref()?;
            let animation = target.animations.animations().find(|a| a.id == id)?;
            let style = target.animations.apply_up_to(now, &target.base, id);
            Some(
                animation
                    .effect
                    .properties()
                    .map(|property| {
                        let name = property.name();
                        (
                            name,
                            style_resolver::computed_value(&style, name).unwrap_or_default(),
                        )
                    })
                    .collect(),
            )
        })
        .flatten()
        .unwrap_or_default();
    if values.is_empty() {
        return Ok(());
    }
    let mut block = cx.take_inline_style(element);
    let mut changed = false;
    for (name, value) in values {
        changed |= Arc::make_mut(&mut block).set_property(name, &value, "");
    }
    if changed {
        cx.set_inline_style(element, block);
    }
    Ok(())
}

pub(crate) fn start_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Option<f64>, ApiError> {
    prepare(cx, this);
    Ok(read_animation(cx, this, |a, _| a.playback.start_time).flatten())
}

pub(crate) fn set_start_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    value: Option<f64>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    let was_paused = is_paused(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.set_start_time(value, now, end, notices);
        Ok(())
    })?;
    override_if_pause_changed(cx, this, was_paused);
    Ok(())
}

pub(crate) fn current_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Option<f64>, ApiError> {
    prepare(cx, this);
    Ok(read_animation(cx, this, |a, now| a.current_time(now)).flatten())
}

/// Setting `currentTime`: `null` only while it has none (a `TypeError`
/// otherwise).
pub(crate) fn set_current_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    value: Option<f64>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    let Some(seek) = value else {
        if read_animation(cx, this, |a, now| a.current_time(now).is_some()) == Some(true) {
            return Err(ApiError::type_error(
                "an animation with a current time can't be set to none",
            ));
        }
        return Ok(());
    };
    control(cx, this, |playback, now, end, notices| {
        playback.set_current_time(seek, now, end, notices);
        Ok(())
    })
}

pub(crate) fn playback_rate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<f64, ApiError> {
    Ok(read_animation(cx, this, |a, _| a.playback.playback_rate).unwrap_or(1.0))
}

pub(crate) fn set_playback_rate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    value: f64,
) -> Result<(), ApiError> {
    prepare(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.set_playback_rate(value, now, end, notices);
        Ok(())
    })
}

/// `playState`.
pub(crate) fn play_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    prepare(cx, this);
    let state = read_animation(cx, this, |a, now| a.play_state(now)).unwrap_or(PlayState::Idle);
    Ok(DomStr::fresh(match state {
        PlayState::Idle => "idle",
        PlayState::Running => "running",
        PlayState::Paused => "paused",
        PlayState::Finished => "finished",
    }))
}

/// `pending`: a play or pause task waits.
pub(crate) fn pending<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<bool, ApiError> {
    prepare(cx, this);
    Ok(read_animation(cx, this, |a, _| a.playback.is_pending()).unwrap_or(false))
}

/// `ready` or `finished`: the same promise each time until its animation
/// replaces it (its notices settled first), made the first time asked,
/// resolved then if its animation's is.
fn promise<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    ready: bool,
) -> Result<AnimationPromise<'gc>, ApiError> {
    prepare(cx, this);
    let resolved = read_animation(cx, this, |a, _| {
        if ready {
            !a.playback.is_ready_pending()
        } else {
            a.playback.is_finished_resolved()
        }
    })
    .unwrap_or(ready);
    Ok(AnimationPromise {
        object: this,
        ready,
        resolved,
    })
}

pub(crate) fn ready<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<AnimationPromise<'gc>, ApiError> {
    promise(cx, this, true)
}

pub(crate) fn finished<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<AnimationPromise<'gc>, ApiError> {
    promise(cx, this, false)
}

/// `play()`: its object kept again (it can be reached).
pub(crate) fn play<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.play(true, now, end, notices)
    })?;
    override_play_state(cx, this);
    let (_, _, document) = of(cx, this);
    cx.keep_animation_object(document, this);
    Ok(())
}

pub(crate) fn pause<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.pause(now, end, notices)
    })?;
    override_play_state(cx, this);
    let (_, _, document) = of(cx, this);
    cx.keep_animation_object(document, this);
    Ok(())
}

pub(crate) fn finish<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.finish(now, end, notices)
    })
}

/// `cancel()`: a CSS transition's or animation's events too (a transition
/// then leaves its element); canceled, it no longer answers to
/// `animation-play-state` (Blink's `CSSAnimation::cancel`).
pub(crate) fn cancel<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    with_animation(cx, this, |animation, now, sent| animation.cancel(now, sent));
    override_play_state(cx, this);
    Ok(())
}

pub(crate) fn reverse<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    prepare(cx, this);
    let was_paused = is_paused(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.reverse(now, end, notices)
    })?;
    override_if_pause_changed(cx, this, was_paused);
    let (_, _, document) = of(cx, this);
    cx.keep_animation_object(document, this);
    Ok(())
}

pub(crate) fn update_playback_rate<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    rate: f64,
) -> Result<(), ApiError> {
    prepare(cx, this);
    control(cx, this, |playback, now, end, notices| {
        playback.update_playback_rate(rate, now, end, notices);
        Ok(())
    })
}

/// `CSSAnimation.animationName`.
pub(crate) fn animation_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh(
        read_animation(cx, this, |a, _| match &a.owner {
            Owner::CssAnimation(owner) => owner.name.to_string(),
            _ => String::new(),
        })
        .unwrap_or_default(),
    ))
}

/// `CSSTransition.transitionProperty`.
pub(crate) fn transition_property<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh(
        read_animation(cx, this, |a, _| match &a.owner {
            Owner::Transition(owner) => owner.property.name(),
            _ => "",
        })
        .unwrap_or(""),
    ))
}

// --- `AnimationPlaybackEvent`

/// `new AnimationPlaybackEvent(type, eventInitDict)`.
pub(crate) fn construct_playback_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &AnimationPlaybackEventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_window(cx),
    );
    cx.set_animation_playback_event_data(event, init.currentTime, init.timelineTime);
    Ok(event)
}

fn playback_data<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> (Option<f64>, Option<f64>) {
    cx.animation_playback_event_data(event)
        .expect("the front end's checks: an AnimationPlaybackEvent")
}

pub(crate) fn event_current_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<f64>, ApiError> {
    Ok(playback_data(cx, this).0)
}

pub(crate) fn event_timeline_time<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<f64>, ApiError> {
    Ok(playback_data(cx, this).1)
}

// --- The rendering update's and the engine's steps

/// The animations' objects notices were queued for (members' playback,
/// before the call returns: `dom::Microtasks`' animations bit), their
/// promises settled in order by the engine.
pub fn settle_animation_notices<H: ScriptHost>(host: &mut H) {
    let objects: Vec<Persistent<EventTargetImpl>> = host.mutate(|cx| {
        cx.take_objects_with_animation_notices()
            .into_iter()
            .map(|object| Persistent::new(cx.gc, object))
            .collect()
    });
    for object in objects {
        host.settle_animation_notices(&object);
    }
}

/// The notices of `document`'s animations the frame just committed
/// started (or paused) applied to their promises now, as Blink's
/// `Animation::NotifyReady` resolves `ready` there; their events wait for
/// the next update of animations.
pub fn settle_started_animations<H: ScriptHost>(host: &mut H, document: &dom::NodeRoot) {
    host.mutate(|cx| {
        let document = document.get(cx.gc);
        for (id, notice) in cx.take_all_unsettled_playback_notices(document) {
            if let Some(object) = cx.existing_animation_object(document, id) {
                cx.push_animation_notice(object, notice);
            }
        }
    });
    settle_animation_notices(host);
}

/// What one notice does to `object`'s promises, as the engine applies it
/// (web-animations 1's finish notification steps, a pending task's): to
/// `ready` (`true`) or `finished`, resolved (with the object) or rejected
/// (an `AbortError`), and whether the next one asked for is new.
pub fn notice_steps(notice: Notice) -> Option<(bool, Option<Option<ApiError>>, bool)> {
    Some(match notice {
        Notice::NewReady => (true, None, true),
        Notice::ReadyResolved => (true, Some(None), false),
        Notice::ReadyAborted => (true, Some(Some(abort_error())), true),
        Notice::Finished { .. } => (false, Some(None), false),
        Notice::NewFinished => (false, None, true),
        Notice::Canceled { .. } => (false, Some(Some(abort_error())), true),
        Notice::Removed { .. } => return None,
    })
}

/// The update of animations' step for `document`'s playback events (in
/// order with the others, their promises settled before any): a `finish`
/// or `cancel` event at its object, if script has one.
pub(crate) fn playback_event<H: ScriptHost>(
    host: &mut H,
    document: &dom::NodeRoot,
    pending: &PendingAnimationEvent,
) {
    let AnimationEvent::Playback(playback) = &pending.event else {
        return;
    };
    let object: Option<Persistent<EventTargetImpl>> = host.mutate(|cx| {
        let object = cx.existing_animation_object(document.get(cx.gc), playback.id)?;
        Some(Persistent::new(cx.gc, object))
    });
    let Some(object) = object else {
        return;
    };
    let (event_type, current_time, timeline_time) = match playback.notice {
        Notice::Finished {
            current_time,
            timeline_time,
        } => (names::event::FINISH, current_time, timeline_time),
        Notice::Canceled { timeline_time } => (names::event::CANCEL, None, timeline_time),
        Notice::Removed {
            current_time,
            timeline_time,
        } => (names::event::REMOVE, current_time, timeline_time),
        _ => return,
    };
    let event = host.mutate(|cx| {
        let event = cx.create_event(
            event_type,
            EventFlags {
                trusted: true,
                ..EventFlags::default()
            },
            cx.window_of(document.get(cx.gc)),
        );
        cx.set_animation_playback_event_data(event, current_time, timeline_time);
        Persistent::new(cx.gc, event)
    });
    crate::event_target::dispatch(host, &TargetRoot::EventTarget(object), &event);
}
