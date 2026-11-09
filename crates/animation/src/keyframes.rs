//! web-animations 1's "process a keyframes argument", once an engine read
//! the script object ([`ScriptKeyframes`]): the keyframes as declarations,
//! their offsets checked, their easings parsed, a value machopo can't parse
//! left out. Both DOMs' `animate()`, `new KeyframeEffect()` and
//! `setKeyframes()` (Blink's `EffectInput`).

use std::sync::Arc;

use css::CssPropertyValueSet;

use crate::css_animation::DeclaredKeyframe;

/// A keyframe member's value: one, or (a property-indexed keyframe's) a
/// list.
#[derive(Debug, Clone, PartialEq)]
pub enum OneOrList<T> {
    One(T),
    List(Vec<T>),
}

/// A keyframe-like object as an engine read it: each member `None` where
/// it was `undefined`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KeyframeLike {
    pub composite: Option<OneOrList<String>>,
    pub easing: Option<OneOrList<String>>,
    /// `null` is `None` inside.
    pub offset: Option<OneOrList<Option<f64>>>,
    /// The animation properties' values, by their script names, in code
    /// point order.
    pub properties: Vec<(String, OneOrList<String>)>,
}

/// A keyframes argument as read: an iterable's keyframes, or one
/// property-indexed keyframe.
#[derive(Debug, Clone, PartialEq)]
pub enum ScriptKeyframes {
    List(Vec<KeyframeLike>),
    PropertyIndexed(KeyframeLike),
}

/// Whether a keyframe object's property named `name` is an animation
/// property (web-animations 1): the attribute name of a property machopo
/// animates (`cssFloat` for `float`). What an engine reads of the object.
pub fn is_animation_property(name: &str) -> bool {
    // The IDL attribute's name only: `fontSize`, never `font-size`.
    if name == "float" || name.contains('-') {
        return false;
    }
    css::property_set::css_property_for_attribute(name).is_some_and(|css| {
        css::properties::longhands_of(css).is_some_and(|longhands| {
            longhands
                .iter()
                .any(|id| id.animation_type() != css::interpolate::AnimationType::NotAnimatable)
        })
    })
}

/// The keyframes `read` gives, their offsets checked (out of 0 to 1, or
/// not in order) and their easings and composites parsed: the message of
/// the `TypeError` otherwise. Their `url()`s are left for the caller to
/// resolve against its document.
pub fn process_keyframes(read: ScriptKeyframes) -> Result<Vec<DeclaredKeyframe>, &'static str> {
    let keyframes = match read {
        ScriptKeyframes::List(list) => list
            .into_iter()
            .map(keyframe_of)
            .collect::<Result<Vec<_>, _>>()?,
        ScriptKeyframes::PropertyIndexed(object) => property_indexed(object)?,
    };
    let mut last = 0.0;
    for keyframe in &keyframes {
        if let Some(offset) = keyframe.offset {
            if !(0.0..=1.0).contains(&offset) {
                return Err("a keyframe's offset must be from 0 to 1");
            }
            if offset < last {
                return Err("keyframes' offsets must be in order");
            }
            last = offset;
        }
    }
    Ok(keyframes)
}

fn one<T: Clone>(value: &Option<OneOrList<T>>) -> Result<Option<T>, &'static str> {
    match value {
        None => Ok(None),
        Some(OneOrList::One(value)) => Ok(Some(value.clone())),
        Some(OneOrList::List(_)) => Err("a keyframe's member can't be a list"),
    }
}

fn list<T: Clone>(value: &Option<OneOrList<T>>) -> Vec<T> {
    match value {
        None => Vec::new(),
        Some(OneOrList::One(value)) => vec![value.clone()],
        Some(OneOrList::List(values)) => values.clone(),
    }
}

/// A keyframe of the keyframes' values (`values`: script names and
/// values), with its offset, easing and composite as given.
fn declared(
    offset: Option<f64>,
    easing: Option<String>,
    composite: Option<String>,
    values: &[(String, String)],
) -> Result<DeclaredKeyframe, &'static str> {
    let easing_text = easing.unwrap_or_else(|| "linear".into());
    let (easing, easing_text) =
        css::animation::parse_easing_text(&easing_text).ok_or("not an easing function")?;
    let composite = composite.unwrap_or_else(|| "auto".into());
    if !is_composite(&composite) {
        return Err("not a composite operation");
    }
    let mut declarations = Vec::new();
    let mut written = Vec::new();
    for (name, value) in values {
        let Some(css) = css::property_set::css_property_for_attribute(name) else {
            continue;
        };
        let Some(block) = CssPropertyValueSet::parse_value(css, value) else {
            continue;
        };
        declarations.extend(block.declarations(false).cloned());
        // As serialized (`rgb(1, 2, 3)` for `rgb(1,2,3)`), as
        // `getKeyframes()` gives it.
        let serialized = block.property_value(css);
        written.push((
            css.to_owned(),
            if serialized.is_empty() {
                value.clone()
            } else {
                serialized
            },
        ));
    }
    Ok(DeclaredKeyframe {
        offset,
        easing,
        easing_text,
        composite,
        block: Arc::new(CssPropertyValueSet::from_declarations(declarations)),
        values: written,
    })
}

fn is_composite(composite: &str) -> bool {
    matches!(composite, "auto" | "replace" | "add" | "accumulate")
}

/// One keyframe object of an iterable's.
fn keyframe_of(object: KeyframeLike) -> Result<DeclaredKeyframe, &'static str> {
    let values: Vec<(String, String)> = object
        .properties
        .iter()
        .map(|(name, value)| match value {
            OneOrList::One(value) => Ok((name.clone(), value.clone())),
            OneOrList::List(_) => Err("a keyframe's value can't be a list"),
        })
        .collect::<Result<_, _>>()?;
    declared(
        one(&object.offset)?.flatten(),
        one(&object.easing)?,
        one(&object.composite)?,
        &values,
    )
}

/// A property-indexed keyframe's keyframes (web-animations 1): each
/// property's values spaced from 0 to 1 (one value alone at 1), merged by
/// offset; then the `offset`, `easing` and `composite` lists given to
/// them in order (the last two repeated if shorter).
fn property_indexed(object: KeyframeLike) -> Result<Vec<DeclaredKeyframe>, &'static str> {
    let mut by_offset: Vec<(f64, Vec<(String, String)>)> = Vec::new();
    for (name, values) in &object.properties {
        let values = match values {
            OneOrList::One(value) => vec![value.clone()],
            OneOrList::List(values) => values.clone(),
        };
        let n = values.len();
        for (i, value) in values.into_iter().enumerate() {
            #[expect(
                clippy::cast_precision_loss,
                reason = "a list's index and length: far below 2^52"
            )]
            let offset = if n == 1 {
                1.0
            } else {
                // `n` > 1 here.
                i as f64 / n.saturating_sub(1) as f64
            };
            match by_offset.iter_mut().find(|(at, _)| *at == offset) {
                Some((_, entries)) => entries.push((name.clone(), value)),
                None => by_offset.push((offset, vec![(name.clone(), value)])),
            }
        }
    }
    by_offset.sort_by(|a, b| a.0.total_cmp(&b.0));
    let offsets = list(&object.offset);
    let easings = list(&object.easing);
    let composites = list(&object.composite);
    // Each easing and composite given is checked, those no keyframe takes
    // too.
    for easing in &easings {
        css::animation::parse_easing_text(easing).ok_or("not an easing function")?;
    }
    if !composites.iter().all(|c| is_composite(c)) {
        return Err("not a composite operation");
    }
    by_offset
        .iter()
        .enumerate()
        .map(|(i, (_, values))| {
            let offset = offsets.get(i).copied().flatten();
            // Round the lists given; none for an empty one.
            let easing = i.checked_rem(easings.len()).map(|j| easings[j].clone());
            let composite = i
                .checked_rem(composites.len())
                .map(|j| composites[j].clone());
            declared(offset, easing, composite, values)
        })
        .collect()
}
