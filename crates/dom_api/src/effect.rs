//! Web Animations' `KeyframeEffect` (web-animations 1; Blink's
//! `KeyframeEffect`, `EffectInput`, `TimingInput`): its
//! constructor, its timing (`getTiming()`, `getComputedTiming()`,
//! `updateTiming()`), its target and keyframes.
//!
//! An effect's object holds its number: the effect is in its animation (on
//! its target's element, or off any), or, without one, on the object
//! itself (`dom::FreeEffect`), which alone reaches it. Script's
//! keyframes are kept as declarations, which style computes on the
//! target, as it does a CSS animation's.

use std::sync::Arc;

use animation::{
    KeyframeEffect, OptionalTiming, Owner, ScriptKeyframes, Timing, TimingInput, TimingOverrides,
};
use computed_style::PseudoId;
use dom::{DomString, FreeEffect, KeyframeEffectObject, NodeRef};
use elise::Gc;

use crate::typed::{AnimationOptions, DictValue, DomStr};
use crate::{ApiCx, ApiError, DomException, ExceptionName, OpFront};

/// Where an effect is.
enum Place {
    /// On its object: no animation has it.
    Free,
    /// In the animation numbered so.
    Animation(u64),
}

/// An effect as it is now: its keyframes, timing (and how script wrote
/// it) and target, and its animation's number, if it has one.
pub(crate) struct EffectState<'gc> {
    pub(crate) effect: Arc<KeyframeEffect>,
    pub(crate) timing: Timing,
    pub(crate) input: TimingInput,
    pub(crate) target: Option<(NodeRef<'gc>, Option<PseudoId>)>,
    pub(crate) animation: Option<u64>,
}

fn place<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, KeyframeEffectObject>) -> Option<Place> {
    if cx.free_effect(this).is_some() {
        return Some(Place::Free);
    }
    let (id, document) = cx.effect_of_object(this);
    let (animation, _) = cx.find_effect(document, id)?;
    Some(Place::Animation(animation))
}

/// `this`'s effect as it is now; `None` if it is gone.
pub(crate) fn state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
) -> Option<EffectState<'gc>> {
    let (id, document) = cx.effect_of_object(this);
    match place(cx, this)? {
        Place::Free => {
            let (free, element) = cx.free_effect(this)?;
            let target = element.map(|element| (element, free.input.pseudo));
            Some(EffectState {
                effect: free.effect,
                timing: free.timing,
                input: free.input,
                target,
                animation: None,
            })
        }
        Place::Animation(animation) => {
            let (_, target) = cx.find_effect(document, id)?;
            cx.read_animation(
                document,
                animation,
                target.map(|(element, _)| element),
                |a, _| EffectState {
                    effect: a.effect.clone(),
                    timing: a.timing.clone(),
                    input: a.timing_input.clone(),
                    target,
                    animation: Some(animation),
                },
            )
        }
    }
}

/// What script changes of an effect: its keyframes, these members of its
/// timing, or neither (its pseudo-element).
enum Change {
    Keyframes,
    Timing(TimingOverrides),
    Neither,
}

/// Changes `this`'s effect in place (and its CSS animation's, which
/// style then no longer changes); its target is styled again. `None` if
/// it is gone.
fn change<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
    change: Change,
    f: impl FnOnce(&mut Arc<KeyframeEffect>, &mut Timing, &mut TimingInput) -> R,
) -> Option<R> {
    let (id, document) = cx.effect_of_object(this);
    match place(cx, this)? {
        Place::Free => cx.with_free_effect(this, |free: &mut FreeEffect| {
            f(&mut free.effect, &mut free.timing, &mut free.input)
        }),
        Place::Animation(animation) => {
            let (_, target) = cx.find_effect(document, id)?;
            cx.with_animation(
                document,
                animation,
                target.map(|(element, _)| element),
                |a, _| {
                    let result = f(&mut a.effect, &mut a.timing, &mut a.timing_input);
                    if let Owner::CssAnimation(owner) = &mut a.owner {
                        match change {
                            Change::Keyframes => owner.keyframes_overridden = true,
                            Change::Timing(members) => {
                                owner.timing_overrides = owner.timing_overrides.union(members);
                            }
                            Change::Neither => {}
                        }
                    }
                    result
                },
            )
        }
    }
}

/// Style brought up to date first if `this` is a CSS animation's or
/// transition's effect, as their members do; script's effects never cause
/// a style change event (web-animations 1's model liveness).
fn update_style_for<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, KeyframeEffectObject>) {
    let (_, document) = cx.effect_of_object(this);
    let Some(Place::Animation(animation)) = place(cx, this) else {
        return;
    };
    let css = cx
        .read_animation(document, animation, None, |a, _| {
            !matches!(a.owner, Owner::Script)
        })
        .unwrap_or(false);
    if css {
        cx.with_style(document, |_| ());
    }
}

fn gone() -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::InvalidState,
        message: "the effect is gone".into(),
    })
}

// --- `KeyframeEffect`

/// `new KeyframeEffect(target, keyframes, options)`: of the realm's
/// document, without an animation (on its object).
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    target: Option<NodeRef<'gc>>,
    keyframes: Option<&ScriptKeyframes>,
    options: &AnimationOptions,
) -> Result<Gc<'gc, KeyframeEffectObject>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("no window"))?;
    let document = cx.window_document(window);
    let mut timing = animation::default_timing();
    let mut input = TimingInput::default();
    animation::apply_timing(&mut timing, &mut input, &options.timing)
        .map_err(ApiError::type_error)?;
    input.pseudo = crate::animation::parse_pseudo(options.pseudo_element.as_deref())?;
    let effect = crate::animation::declared_effect(cx, document, keyframes)?;
    let id = cx.next_animation_number();
    let object = cx.effect_object(document, id);
    cx.put_free_effect(
        object,
        FreeEffect {
            effect,
            timing,
            input,
        },
        target,
    );
    Ok(object)
}

/// The `EffectTiming` of `timing`, `input` saying how it was written, as
/// a dictionary's members in order.
fn effect_timing(timing: &Timing, input: &TimingInput) -> Vec<(&'static str, DictValue)> {
    let direction = match timing.direction {
        animation::PlaybackDirection::Normal => "normal",
        animation::PlaybackDirection::Reverse => "reverse",
        animation::PlaybackDirection::Alternate => "alternate",
        animation::PlaybackDirection::AlternateReverse => "alternate-reverse",
    };
    let fill = if input.fill_auto {
        "auto"
    } else {
        fill_text(timing.fill)
    };
    vec![
        ("delay", DictValue::Number(timing.delay)),
        ("direction", DictValue::String(direction.into())),
        (
            "duration",
            if input.duration_auto {
                DictValue::String("auto".into())
            } else {
                DictValue::Number(timing.duration)
            },
        ),
        (
            "easing",
            DictValue::String(
                input
                    .easing
                    .clone()
                    .unwrap_or_else(|| css::animation::easing_text(&timing.easing)),
            ),
        ),
        ("endDelay", DictValue::Number(timing.end_delay)),
        ("fill", DictValue::String(fill.into())),
        ("iterationStart", DictValue::Number(timing.iteration_start)),
        ("iterations", DictValue::Number(timing.iterations)),
    ]
}

fn fill_text(fill: animation::FillMode) -> &'static str {
    match fill {
        animation::FillMode::None => "none",
        animation::FillMode::Forwards => "forwards",
        animation::FillMode::Backwards => "backwards",
        animation::FillMode::Both => "both",
    }
}

/// `getTiming()`.
pub(crate) fn get_timing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
) -> Result<Vec<(&'static str, DictValue)>, ApiError> {
    update_style_for(cx, this);
    let state = state(cx, this).ok_or_else(gone)?;
    Ok(effect_timing(&state.timing, &state.input))
}

/// `getComputedTiming()`: its timing at its animation's current time
/// (none without an animation), `fill: "auto"` resolved, its duration a
/// number.
pub(crate) fn get_computed_timing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
) -> Result<Vec<(&'static str, DictValue)>, ApiError> {
    update_style_for(cx, this);
    let state = state(cx, this).ok_or_else(gone)?;
    let (_, document) = cx.effect_of_object(this);
    let now = cx.animation_time(document);
    let (local_time, backwards) = match state.animation {
        Some(animation) => cx
            .read_animation(
                document,
                animation,
                state.target.map(|(element, _)| element),
                |a, _| (a.current_time(now), a.playback.playback_rate < 0.0),
            )
            .ok_or_else(gone)?,
        None => (None, false),
    };
    let computed = state.timing.compute(local_time, backwards);
    let number_or_null = |value: Option<f64>| value.map_or(DictValue::Null, DictValue::Number);
    let mut members = effect_timing(&state.timing, &state.input);
    for (name, value) in &mut members {
        match *name {
            "duration" => *value = DictValue::Number(state.timing.duration),
            "fill" => *value = DictValue::String(fill_text(state.timing.fill).into()),
            _ => {}
        }
    }
    members.extend([
        (
            "activeDuration",
            DictValue::Number(state.timing.active_duration()),
        ),
        (
            "currentIteration",
            number_or_null(computed.current_iteration),
        ),
        ("endTime", DictValue::Number(state.timing.end_time())),
        ("localTime", number_or_null(local_time)),
        ("progress", number_or_null(computed.progress)),
        ("startTime", DictValue::Number(0.0)),
    ]);
    Ok(members)
}

/// `updateTiming(timing)`: the members given (checked first: nothing
/// changes if one fails); its animation's finished state follows.
pub(crate) fn update_timing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
    timing: &OptionalTiming,
) -> Result<(), ApiError> {
    update_style_for(cx, this);
    let state = state(cx, this).ok_or_else(gone)?;
    let (mut new, mut input) = (state.timing.clone(), state.input.clone());
    animation::apply_timing(&mut new, &mut input, timing).map_err(ApiError::type_error)?;
    let members = TimingOverrides {
        delay: timing.delay.is_some(),
        end_delay: timing.end_delay.is_some(),
        fill: timing.fill.is_some(),
        iteration_start: timing.iteration_start.is_some(),
        iterations: timing.iterations.is_some(),
        duration: timing.duration.is_some(),
        direction: timing.direction.is_some(),
        easing: timing.easing.is_some(),
    };
    change(cx, this, Change::Timing(members), |_, out, out_input| {
        *out = new;
        *out_input = input;
    });
    if let Some(animation) = state.animation {
        let (_, document) = cx.effect_of_object(this);
        crate::animation::update_finished_state(cx, document, animation);
    }
    Ok(())
}

/// `target`: the element it animates (a pseudo-element's element).
pub(crate) fn target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(state(cx, this)
        .and_then(|state| state.target)
        .map(|(element, _)| element))
}

/// Points `this` at `target`: its animation moves to the target's element
/// (a CSS transition's or animation's stays its owning element's), and
/// both are styled again.
fn retarget<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
    target: Option<(NodeRef<'gc>, Option<PseudoId>)>,
) -> Result<(), ApiError> {
    let (_, document) = cx.effect_of_object(this);
    let state = state(cx, this).ok_or_else(gone)?;
    match state.animation {
        None => cx.set_free_effect_target(this, target.map(|(element, _)| element)),
        Some(animation) => {
            let hint = state.target.map(|(element, _)| element);
            let scripts = cx
                .read_animation(document, animation, hint, |a, _| {
                    matches!(a.owner, Owner::Script) || !a.owning
                })
                .unwrap_or(false);
            if scripts && let Some(moved) = cx.take_animation(document, animation, hint) {
                cx.put_animation(
                    document,
                    target,
                    moved,
                    crate::animation::base_style_fn(cx, target),
                );
                if let Some(object) = cx.existing_animation_object(document, animation) {
                    cx.set_animation_object_target(object, target.map(|(element, _)| element));
                }
            }
        }
    }
    for (element, _) in [state.target, target].into_iter().flatten() {
        cx.animations_changed(element);
    }
    Ok(())
}

pub(crate) fn set_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
    value: Option<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    let pseudo = state(cx, this).ok_or_else(gone)?.input.pseudo;
    retarget(cx, this, value.map(|element| (element, pseudo)))
}

fn pseudo_text(pseudo: PseudoId) -> &'static str {
    match pseudo {
        PseudoId::Before => "::before",
        PseudoId::After => "::after",
        PseudoId::Marker => "::marker",
    }
}

/// `pseudoElement`.
pub(crate) fn pseudo_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
) -> Result<Option<DomString>, ApiError> {
    let pseudo = state(cx, this).and_then(|state| match state.target {
        Some((_, pseudo)) => pseudo,
        None => state.input.pseudo,
    });
    Ok(pseudo.map(|pseudo| DomString::new(pseudo_text(pseudo))))
}

pub(crate) fn set_pseudo_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
    value: Option<&str>,
) -> Result<(), ApiError> {
    let pseudo = crate::animation::parse_pseudo(value)?;
    change(cx, this, Change::Neither, |_, _, input| {
        input.pseudo = pseudo
    });
    if let Some((element, _)) = state(cx, this).and_then(|state| state.target) {
        retarget(cx, this, Some((element, pseudo)))?;
    }
    Ok(())
}

/// `composite`: every effect composites as `replace` in machopo.
pub(crate) fn composite<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, KeyframeEffectObject>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh("replace"))
}

/// Setting `composite`: nothing (an enumeration's value it doesn't know
/// is ignored too).
pub(crate) fn set_composite<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, KeyframeEffectObject>,
    _: &str,
) -> Result<(), ApiError> {
    Ok(())
}

/// A property's script name (CSSOM's camel-cased attribute name).
fn camel_case(name: &str) -> String {
    if name == "float" {
        return "cssFloat".into();
    }
    let mut out = String::with_capacity(name.len());
    let mut upper = false;
    for c in name.chars() {
        match c {
            '-' => upper = true,
            c if upper => {
                out.push(c.to_ascii_uppercase());
                upper = false;
            }
            c => out.push(c),
        }
    }
    out
}

/// `getKeyframes()`: script's as given (with their computed offsets); a
/// CSS animation's or transition's as computed on its element.
pub(crate) fn get_keyframes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
) -> Result<Vec<Vec<(String, DictValue)>>, ApiError> {
    update_style_for(cx, this);
    let state = state(cx, this).ok_or_else(gone)?;
    if let Some(declared) = &state.effect.declared {
        let offsets = declared.computed_offsets();
        return Ok(declared
            .keyframes
            .iter()
            .zip(offsets)
            .map(|(keyframe, computed)| {
                let mut entries = vec![
                    (
                        "offset".into(),
                        keyframe.offset.map_or(DictValue::Null, DictValue::Number),
                    ),
                    ("computedOffset".into(), DictValue::Number(computed)),
                    (
                        "easing".into(),
                        DictValue::String(keyframe.easing_text.clone()),
                    ),
                    (
                        "composite".into(),
                        DictValue::String(keyframe.composite.clone()),
                    ),
                ];
                entries.extend(
                    keyframe
                        .values
                        .iter()
                        .map(|(name, value)| (camel_case(name), DictValue::String(value.clone()))),
                );
                entries
            })
            .collect());
    }
    Ok(state
        .effect
        .computed_keyframes()
        .into_iter()
        .map(|(offset, easing, values)| {
            let mut entries = vec![
                ("offset".into(), DictValue::Number(offset)),
                ("computedOffset".into(), DictValue::Number(offset)),
                (
                    "easing".into(),
                    DictValue::String(css::animation::easing_text(&easing)),
                ),
                ("composite".into(), DictValue::String("auto".into())),
            ];
            entries.extend(values.into_iter().map(|(id, style)| {
                let value = style_resolver::computed_value(&style, id.name()).unwrap_or_default();
                (camel_case(id.name()), DictValue::String(value))
            }));
            entries
        })
        .collect())
}

/// `setKeyframes(keyframes)`: style computes them on the target.
pub(crate) fn set_keyframes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, KeyframeEffectObject>,
    keyframes: Option<&ScriptKeyframes>,
) -> Result<(), ApiError> {
    let (_, document) = cx.effect_of_object(this);
    let effect = crate::animation::declared_effect(cx, document, keyframes)?;
    change(cx, this, Change::Keyframes, |out, _, _| *out = effect).ok_or_else(gone)
}
