//! Reading a keyframes argument (web-animations 1's "process a keyframes
//! argument", the part that reads script values; Blink's `EffectInput`):
//! an iterable's keyframe-like objects, or one property-indexed keyframe,
//! each read in WebIDL's order. `animation::process_keyframes` does the
//! rest.

use animation::{KeyframeLike, OneOrList, ScriptKeyframes};

use crate::templates;

/// `keyframes` read: an object with an `@@iterator` is a list of
/// keyframes (each `undefined`, `null` or an object), any other object one
/// property-indexed keyframe. `None` with an exception thrown.
pub(crate) fn read<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    keyframes: v8::Local<'s, v8::Value>,
) -> Option<ScriptKeyframes> {
    let Ok(object) = v8::Local::<v8::Object>::try_from(keyframes) else {
        crate::throw_type_error(scope, "keyframes: not an object");
        return None;
    };
    if has_iterator(scope, object)? {
        let mut list = Vec::new();
        crate::observable_array::for_each_item(scope, object.into(), |scope, item| {
            if !(item.is_null_or_undefined() || item.is_object()) {
                crate::throw_type_error(scope, "keyframes: a keyframe isn't an object");
                return None;
            }
            list.push(keyframe_like(scope, item, false)?);
            Some(())
        })?;
        return Some(ScriptKeyframes::List(list));
    }
    Some(ScriptKeyframes::PropertyIndexed(keyframe_like(
        scope,
        object.into(),
        true,
    )?))
}

/// Whether `object` has an `@@iterator` (getting it may throw).
fn has_iterator<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
) -> Option<bool> {
    let symbol = v8::Symbol::get_iterator(scope);
    let method = object.get(scope, symbol.into())?;
    Some(!method.is_null_or_undefined())
}

/// One keyframe-like object (`undefined` and `null` are empty ones): its
/// `composite`, `easing` and `offset` (a list each, if `lists` and given
/// one), then its animation properties' values by name.
fn keyframe_like<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    lists: bool,
) -> Option<KeyframeLike> {
    let mut keyframe = KeyframeLike::default();
    let Ok(object) = v8::Local::<v8::Object>::try_from(value) else {
        return Some(keyframe);
    };
    keyframe.composite = member(scope, object, "composite", lists, string)?;
    keyframe.easing = member(scope, object, "easing", lists, string)?;
    keyframe.offset = member(scope, object, "offset", lists, offset)?;
    let names = object.get_own_property_names(
        scope,
        v8::GetPropertyNamesArgs {
            mode: v8::KeyCollectionMode::OwnOnly,
            property_filter: v8::PropertyFilter::ONLY_ENUMERABLE | v8::PropertyFilter::SKIP_SYMBOLS,
            index_filter: v8::IndexFilter::SkipIndices,
            key_conversion: v8::KeyConversionMode::ConvertToString,
        },
    )?;
    let mut properties = Vec::new();
    for i in 0..names.length() {
        let name = names.get_index(scope, i)?.to_rust_string_lossy(scope);
        if animation::is_animation_property(&name) {
            properties.push(name);
        }
    }
    // In code point order (UTF-8 byte order is it).
    properties.sort();
    for name in properties {
        if let Some(value) = member(scope, object, &name, lists, string)? {
            keyframe.properties.push((name, value));
        }
    }
    Some(keyframe)
}

/// WebIDL's `DOMString` of `value` (`ToString`; a symbol throws).
fn string<'s>(scope: &mut v8::PinScope<'s, '_>, value: v8::Local<'s, v8::Value>) -> Option<String> {
    Some(value.to_string(scope)?.to_rust_string_lossy(scope))
}

/// WebIDL's `double?` of `value`: `null` none; not finite a `TypeError`.
fn offset<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<Option<f64>> {
    if value.is_null() {
        return Some(None);
    }
    let number = value.number_value(scope)?;
    if !number.is_finite() {
        crate::throw_type_error(scope, "keyframes: an offset must be a finite number");
        return None;
    }
    Some(Some(number))
}

/// `object[name]` converted by `convert`, or (if `lists`) a sequence of it
/// when it is an object with an `@@iterator` (WebIDL's union of the two):
/// `None` inside for `undefined`.
fn member<'s, T>(
    scope: &mut v8::PinScope<'s, '_>,
    object: v8::Local<'s, v8::Object>,
    name: &str,
    lists: bool,
    convert: impl Fn(&mut v8::PinScope<'s, '_>, v8::Local<'s, v8::Value>) -> Option<T>,
) -> Option<Option<OneOrList<T>>> {
    let key = templates::string(scope, name);
    let value = object.get(scope, key.into())?;
    if value.is_undefined() {
        return Some(None);
    }
    if lists
        && let Ok(list) = v8::Local::<v8::Object>::try_from(value)
        && has_iterator(scope, list)?
    {
        let mut values = Vec::new();
        crate::observable_array::for_each_item(scope, value, |scope, item| {
            values.push(convert(scope, item)?);
            Some(())
        })?;
        return Some(Some(OneOrList::List(values)));
    }
    Some(Some(OneOrList::One(convert(scope, value)?)))
}

/// `animate()`'s options read (`(unrestricted double or
/// KeyframeAnimationOptions)`): a number its duration; an object (or
/// `null`, `undefined`) the dictionary, its members in WebIDL's order
/// (`EffectTiming`'s, `KeyframeEffectOptions`', `KeyframeAnimationOptions`'),
/// its enumerations checked. `None` with an exception thrown.
pub(crate) fn read_options<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
    animation: bool,
) -> Option<dom_api::typed::AnimationOptions> {
    let mut options = dom_api::typed::AnimationOptions::default();
    if value.is_null_or_undefined() {
        return Some(options);
    }
    let Ok(dict) = v8::Local::<v8::Object>::try_from(value) else {
        let duration = value.number_value(scope)?;
        options.timing.duration = Some(animation::DurationInput::Number(duration));
        return Some(options);
    };
    read_timing(scope, dict, &mut options.timing)?;
    if let Some(composite) = enum_member(
        scope,
        dict,
        "composite",
        "CompositeOperation",
        &["replace", "add", "accumulate"],
    )? {
        options.composite = composite;
    }
    if let Some(pseudo) = get(scope, dict, "pseudoElement")? {
        options.pseudo_element = if pseudo.is_null() {
            None
        } else {
            Some(string(scope, pseudo)?)
        };
    }
    // `KeyframeAnimationOptions`' own, after `KeyframeEffectOptions`'.
    if !animation {
        return Some(options);
    }
    if let Some(id) = get(scope, dict, "id")? {
        options.id = string(scope, id)?;
    }
    if let Some(timeline) = get(scope, dict, "timeline")? {
        options.has_timeline = read_timeline(scope, timeline)?;
    }
    Some(options)
}

/// `updateTiming()`'s `OptionalEffectTiming`: the members given (`null`
/// and `undefined` none); not an object a `TypeError`. `None` with an
/// exception thrown.
pub(crate) fn read_optional_timing<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<animation::OptionalTiming> {
    let mut timing = animation::OptionalTiming::default();
    if value.is_null_or_undefined() {
        return Some(timing);
    }
    let Ok(dict) = v8::Local::<v8::Object>::try_from(value) else {
        crate::throw_type_error(
            scope,
            "The provided value is not of type 'OptionalEffectTiming'.",
        );
        return None;
    };
    read_timing(scope, dict, &mut timing)?;
    Some(timing)
}

/// `EffectTiming`'s members of `dict` given, in WebIDL's order, onto
/// `timing`; `None` with an exception thrown.
fn read_timing<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    dict: v8::Local<'s, v8::Object>,
    timing: &mut animation::OptionalTiming,
) -> Option<()> {
    timing.delay = finite_member(scope, dict, "delay")?.or(timing.delay);
    if let Some(direction) = enum_member(
        scope,
        dict,
        "direction",
        "PlaybackDirection",
        &["normal", "reverse", "alternate", "alternate-reverse"],
    )? {
        timing.direction = Some(match direction.as_str() {
            "reverse" => animation::PlaybackDirection::Reverse,
            "alternate" => animation::PlaybackDirection::Alternate,
            "alternate-reverse" => animation::PlaybackDirection::AlternateReverse,
            _ => animation::PlaybackDirection::Normal,
        });
    }
    if let Some(duration) = get(scope, dict, "duration")? {
        timing.duration = Some(if duration.is_number() {
            animation::DurationInput::Number(duration.number_value(scope)?)
        } else {
            animation::DurationInput::Text(string(scope, duration)?)
        });
    }
    if let Some(easing) = get(scope, dict, "easing")? {
        timing.easing = Some(string(scope, easing)?);
    }
    timing.end_delay = finite_member(scope, dict, "endDelay")?.or(timing.end_delay);
    if let Some(fill) = enum_member(
        scope,
        dict,
        "fill",
        "FillMode",
        &["none", "forwards", "backwards", "both", "auto"],
    )? {
        timing.fill = Some(match fill.as_str() {
            "none" => animation::FillInput::Mode(animation::FillMode::None),
            "forwards" => animation::FillInput::Mode(animation::FillMode::Forwards),
            "backwards" => animation::FillInput::Mode(animation::FillMode::Backwards),
            "both" => animation::FillInput::Mode(animation::FillMode::Both),
            _ => animation::FillInput::Auto,
        });
    }
    timing.iteration_start =
        finite_member(scope, dict, "iterationStart")?.or(timing.iteration_start);
    if let Some(iterations) = get(scope, dict, "iterations")? {
        timing.iterations = Some(iterations.number_value(scope)?);
    }
    Some(())
}

/// An `AnimationTimeline?` given no default: whether the animation has one
/// (missing or a `DocumentTimeline`: its document's; `null`: none);
/// anything else a `TypeError`. `None` with an exception thrown.
pub(crate) fn read_timeline<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    value: v8::Local<'s, v8::Value>,
) -> Option<bool> {
    if value.is_undefined() {
        return Some(true);
    }
    if value.is_null() {
        return Some(false);
    }
    if bindings::is_wrapper::<dom::DocumentTimeline>(scope, value) {
        return Some(true);
    }
    crate::throw_type_error(
        scope,
        "The provided value is not of type 'AnimationTimeline'.",
    );
    None
}

/// `dict[name]`, none for `undefined`; `None` if getting it threw.
fn get<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    dict: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<Option<v8::Local<'s, v8::Value>>> {
    let key = templates::string(scope, name);
    let value = dict.get(scope, key.into())?;
    Some((!value.is_undefined()).then_some(value))
}

/// A `double` member: a finite number, else a `TypeError`.
fn finite_member<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    dict: v8::Local<'s, v8::Object>,
    name: &str,
) -> Option<Option<f64>> {
    let Some(value) = get(scope, dict, name)? else {
        return Some(None);
    };
    let number = value.number_value(scope)?;
    if !number.is_finite() {
        crate::throw_type_error(
            scope,
            &format!("The provided double value for '{name}' is non-finite."),
        );
        return None;
    }
    Some(Some(number))
}

/// An enumeration member: one of `values`, else a `TypeError`.
fn enum_member<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    dict: v8::Local<'s, v8::Object>,
    name: &str,
    enumeration: &str,
    values: &[&str],
) -> Option<Option<String>> {
    let Some(value) = get(scope, dict, name)? else {
        return Some(None);
    };
    let text = string(scope, value)?;
    if !values.contains(&text.as_str()) {
        crate::throw_type_error(
            scope,
            &format!(
                "The provided value '{text}' is not a valid enum value of type {enumeration}."
            ),
        );
        return None;
    }
    Some(Some(text))
}
