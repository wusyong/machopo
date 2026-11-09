//! Animations in the style phase, as the style engine (`style`)
//! drives them (Blink's `CSSAnimations` as the style resolver drives it):
//! what one target's style change event does to its animations, which it
//! keeps in an [`AnimatedStyle`], which the DOM holds. Each style change event
//! of an element (or one of its pseudo-elements) starts, updates and
//! cancels its CSS animations (each keyframe cascaded on it, at the
//! animation origin), then starts, replaces and cancels its transitions,
//! and the style it gets is the cascade's with their values at the
//! document timeline's time. A frame that only moves them on applies them
//! to the style the cascade last made (Blink's base computed style),
//! unless they animate an inherited property or `font-size`
//! ([`needs_cascade`]): the element's descendants or its `em`s depend on
//! those, and are styled again.

use std::cell::RefCell;
use std::sync::Arc;

use animation::{
    AnimatedStyle, Animation, AnimationEvent, ElementAnimations, Keyframe, KeyframeEffect,
    KeyframeSource, KeyframesInput,
};
use computed_style::{AnimationName, ComputedStyle, Display, EasingFunction, PseudoId};
use css::properties::LonghandId;
use css::{CssPropertyValueSet, KeyframesRule};

use crate::cascade::{
    Block, BlockSource, Blocks, CascadePriority, Environment, MatchedPropertiesCache, Position,
    cascade_blocks,
};
use crate::theme;

/// How the cascade styles a target again, for its animations.
pub trait Restyle {
    /// With `font-size` this (an animation's): its `em`s follow.
    fn with_font_size(&self, font_size: f32) -> ComputedStyle;
    /// With `block`, a keyframe's declarations, at the animation origin:
    /// over its normal declarations, under its `!important` ones.
    fn with_keyframe(&self, block: &CssPropertyValueSet) -> ComputedStyle;
    /// The style it inherits from.
    fn parent(&self) -> &ComputedStyle;
    fn root_font_size(&self) -> f32;
    /// The viewport's width and height, in px.
    fn viewport(&self) -> (f32, f32);
}

/// The `@keyframes` an element's `animation-name` names.
pub type FindKeyframes<'a> = dyn Fn(&str) -> Option<Arc<KeyframesRule>> + 'a;

/// An element's or a pseudo-element's declarations, and what they are
/// cascaded with: what its animations cascade again.
pub struct Recascade<'a, 'b> {
    pub blocks: &'b [Block<'a>],
    pub parent: &'b ComputedStyle,
    pub layout_parent: &'b ComputedStyle,
    pub is_root: bool,
    pub root_font_size: f32,
    pub env: Environment,
    /// The form control the element is, if any (a pseudo-element is none).
    pub control: Option<theme::Control>,
    pub applies: fn(LonghandId) -> bool,
    /// `parent`, if shared (an element's: a pseudo-element's is being
    /// made).
    pub shared_parent: Option<&'b Arc<ComputedStyle>>,
    /// The engine's matched properties cache.
    pub cache: Option<&'b RefCell<MatchedPropertiesCache>>,
    /// The element has a blocking `wheel` listener of its own (a
    /// pseudo-element inherits its element's): what it adds to what it
    /// inherits of `inside_blocking_wheel_event_handler`.
    pub blocking_wheel: bool,
    /// The pseudo-element it is of its parent, if it is one.
    pub pseudo: Option<PseudoId>,
}

impl Recascade<'_, '_> {
    /// The style its blocks give, with `font_size` (an animation's) if
    /// given, and `keyframe`'s declarations at the animation origin.
    pub fn cascade(
        &self,
        font_size: Option<f32>,
        keyframe: Option<&CssPropertyValueSet>,
    ) -> Arc<ComputedStyle> {
        let mut blocks: Blocks<'_> = self.blocks.iter().cloned().collect();
        if let Some(block) = keyframe {
            blocks.push((CascadePriority::ANIMATION, block, false, BlockSource::Own));
        }
        cascade_blocks(
            blocks,
            self.parent,
            self.layout_parent,
            Position {
                is_root: self.is_root,
                in_flex_or_grid: self.layout_parent.display.blockifies_children(),
                in_webkit_box: self.layout_parent.display.is_webkit_box(),
                control: self.control,
                blocking_wheel: self.blocking_wheel,
                pseudo: self.pseudo,
            },
            self.root_font_size,
            self.env,
            self.applies,
            font_size,
            self.shared_parent,
            self.cache,
        )
    }
}

impl Restyle for Recascade<'_, '_> {
    // Neither is cached: their styles are their own.
    fn with_font_size(&self, font_size: f32) -> ComputedStyle {
        Arc::unwrap_or_clone(self.cascade(Some(font_size), None))
    }

    fn with_keyframe(&self, block: &CssPropertyValueSet) -> ComputedStyle {
        Arc::unwrap_or_clone(self.cascade(None, Some(block)))
    }

    fn parent(&self) -> &ComputedStyle {
        self.parent
    }

    fn root_font_size(&self) -> f32 {
        self.root_font_size
    }

    fn viewport(&self) -> (f32, f32) {
        (self.env.viewport.width, self.env.viewport.height)
    }
}

/// A target's style change event: its style as of the last one (`old`,
/// with its animations), the cascade's now (`base`), and how to cascade
/// it again.
pub struct StyleChange<'a> {
    pub old: Option<&'a Arc<ComputedStyle>>,
    pub base: Arc<ComputedStyle>,
    /// Neither it nor an ancestor is `display: none` (a pseudo-element
    /// has a box): else its animations are canceled.
    pub rendered: bool,
    /// It or an ancestor was removed from its tree since it was styled and
    /// is back: `old` is from before, which a transition doesn't start
    /// from (css-transitions 1), and its CSS animations and transitions
    /// stop.
    pub moved: bool,
    /// Cascades it again, with an animation's `font-size` (the `em`s
    /// follow) or a keyframe.
    pub restyle: &'a dyn Restyle,
    /// Finds the keyframes of its `animation-name`.
    pub find: &'a FindKeyframes<'a>,
}

/// Where a style change event's animations go: the document timeline's
/// time, the last animation's number (their order), and what it sent and
/// what left its target (canceled, replaced, done), which script may hold.
pub struct Timeline<'a> {
    pub now: f64,
    pub order: &'a mut u64,
    pub sent: &'a mut Vec<AnimationEvent>,
    pub removed: &'a mut Vec<Animation>,
}

/// Whether a style change event of a target with no animations (`slot` is
/// empty) could start one: most elements' can't, and the DOM need look no
/// further.
pub fn may_start(style: &ComputedStyle) -> bool {
    may_animate(style) || may_transition(style)
}

/// `change`'s target's style change event, its animations in `slot` (none
/// if it has none, and none once it has none left): its style with their
/// values.
pub fn style_change(
    slot: &mut Option<AnimatedStyle>,
    change: StyleChange<'_>,
    timeline: &mut Timeline<'_>,
) -> Arc<ComputedStyle> {
    let StyleChange {
        old,
        base,
        rendered,
        moved,
        restyle,
        find,
    } = change;
    let now = timeline.now;
    if slot.is_none() && !may_start(&base) {
        return base;
    }
    if !rendered {
        if let Some(mut gone) = slot.take() {
            gone.animations.cancel_all(now, timeline.sent);
            timeline.removed.extend(gone.animations.take_removed());
        }
        return base;
    }
    // A style from before it was removed (and inserted again) is no
    // before-change style: its transitions and CSS animations stop;
    // script's stay on it.
    if moved && let Some(entry) = slot.as_mut() {
        entry.animations.cancel_markup(now, timeline.sent);
        timeline.removed.extend(entry.animations.take_removed());
    }
    // The before-change style has its animations' values now (the style
    // stored may be an earlier frame's, or have one that ended since, or
    // keyframes script set since, computed here first): applied again to
    // the base style they were last applied to.
    if let Some(entry) = slot.as_mut() {
        let last = entry.base.clone();
        for animation in entry.animations.animations_mut() {
            if animation.effect.needs_computing(&last) {
                animation.effect = script_effect(&last, &animation.effect, restyle);
            }
        }
    }
    let resampled = slot
        .as_ref()
        .map(|entry| Arc::new(entry.animations.apply(now, &entry.base)));
    let old = if moved {
        None
    } else {
        resampled.as_ref().or(old)
    };
    let was_rendered = old.is_some_and(|old| old.display != Display::None);
    if slot.is_none() && !may_animate(&base) && !(was_rendered && may_transition(&base)) {
        return base;
    }
    let entry = slot.get_or_insert_with(|| AnimatedStyle {
        animations: Default::default(),
        base: base.clone(),
    });
    // Its CSS animations first: transitions start from and to styles with
    // their values.
    entry.animations.css_animations.style_change(
        &base,
        now,
        timeline.order,
        &mut |i, name, previous| effect(&base, i, name, previous, restyle, find),
        timeline.sent,
    );
    // Script's keyframes on it, computed for this style.
    for animation in entry.animations.animations_mut() {
        // Computed again if anything they depend on changed (the parent's
        // style, the root's font size, the viewport too).
        if animation.effect.declared.is_some() {
            animation.effect = script_effect(&base, &animation.effect, restyle);
        }
    }
    // The after-change style has the CSS animations' and script's values
    // too (as the before-change one does): a property they animate on
    // both sides doesn't transition.
    let after = if entry.animations.css_animations.is_empty() && entry.animations.others.is_empty()
    {
        base.clone()
    } else {
        Arc::new(entry.animations.apply_css_animations(now, &base))
    };
    if let Some(old) = old.filter(|_| was_rendered) {
        entry
            .animations
            .style_change(old, &after, now, timeline.order, timeline.sent);
    }
    entry.base = base.clone();
    // Script's apply too when they don't run: paused, or filling.
    let mut style = if entry.animations.is_running()
        || !entry.animations.css_animations.is_empty()
        || !entry.animations.others.is_empty()
    {
        let mut style = entry.animations.apply(now, &base);
        if entry.animations.animates(LonghandId::FontSize) && style.font.size != base.font.size {
            style = entry
                .animations
                .apply(now, &restyle.with_font_size(style.font.size));
        }
        Arc::new(style)
    } else {
        base.clone()
    };
    // A new viewport styles it again, for its keyframes' `vw`s.
    if entry.animations.css_animations.uses_viewport() && !style.has_viewport_units {
        Arc::make_mut(&mut style).has_viewport_units = true;
    }
    // Those style canceled stay on it: script may still play them.
    entry.animations.keep_canceled();
    if entry.animations.is_empty() {
        *slot = None;
    }
    style
}

/// Whether an element's animations need the cascade, not only its base
/// style: its own (`own`) animate an inherited property (its
/// descendants, pseudo-elements too, inherit the values) or `font-size`
/// (its `em`s follow), or script's keyframes on it or on a pseudo-element
/// of it (`all`, each target's animations and base style, its own among
/// them) aren't computed for its style yet.
pub fn needs_cascade<'a>(
    own: Option<&ElementAnimations>,
    all: impl IntoIterator<Item = (&'a ElementAnimations, &'a Arc<ComputedStyle>)>,
) -> bool {
    let inherited = own.is_some_and(|animations| {
        animations
            .properties()
            .any(|id| id == LonghandId::FontSize || id.is_inherited())
    });
    inherited
        || all.into_iter().any(|(animations, base)| {
            animations
                .animations()
                .any(|animation| animation.effect.needs_computing(base))
        })
}

/// Whether an element (`own`, its animations) has any (CSS animations,
/// transitions or script's): their keyframes may take its parent's values
/// explicitly (`margin-left: inherit`), and they are kept across its
/// restyles, so its parent's other values changing styles it again (with
/// `ComputedStyle::explicit_inheritance`).
pub fn animated(own: Option<&ElementAnimations>) -> bool {
    own.is_some_and(|animations| animations.animations().next().is_some())
}

/// The keyframes of the `@keyframes` the `i`th name of `base`'s
/// `animation-name` (`name`) finds, cascaded on the target (`restyle`):
/// `previous`, the animation's keyframes so far, if they were computed
/// from the same rule, base style, parent style, root font size and
/// viewport; `None` without such a rule.
fn effect(
    base: &Arc<ComputedStyle>,
    i: usize,
    name: &str,
    previous: Option<&Arc<KeyframeEffect>>,
    restyle: &dyn Restyle,
    find: &FindKeyframes<'_>,
) -> Option<Arc<KeyframeEffect>> {
    let rule = find(name)?;
    let easing = base.rare.animations.timing(i).timing_function.clone();
    Some(computed_effect(
        base,
        KeyframesInput::Rule(rule),
        &easing,
        previous,
        restyle,
    ))
}

/// Script's keyframes (`previous` has them) cascaded on the target, as a
/// CSS animation's are: `previous` itself if they were computed for the
/// same styles.
pub fn script_effect(
    base: &Arc<ComputedStyle>,
    previous: &Arc<KeyframeEffect>,
    restyle: &dyn Restyle,
) -> Arc<KeyframeEffect> {
    let declared = previous
        .declared
        .clone()
        .expect("only script's effects are computed so");
    computed_effect(
        base,
        KeyframesInput::Declared(declared),
        &EasingFunction::Linear,
        Some(previous),
        restyle,
    )
}

/// The keyframes of `input` cascaded on the target (`restyle`), each one's
/// easing `easing` unless it gives its own: `previous` if it was computed
/// from the same input, base style, parent style, root font size and
/// viewport.
fn computed_effect(
    base: &Arc<ComputedStyle>,
    input: KeyframesInput,
    easing: &EasingFunction,
    previous: Option<&Arc<KeyframeEffect>>,
    restyle: &dyn Restyle,
) -> Arc<KeyframeEffect> {
    let (root_font_size, viewport) = (restyle.root_font_size(), restyle.viewport());
    if let Some(previous) = previous
        && previous.source.as_ref().is_some_and(|source| {
            source.is(&input, base, restyle.parent(), root_font_size, viewport)
        })
    {
        return previous.clone();
    }
    let mut keyframes = Vec::new();
    match &input {
        KeyframesInput::Rule(rule) => {
            for keyframe in rule.keyframes() {
                let style = Arc::new(restyle.with_keyframe(&keyframe.block));
                let properties: Vec<LonghandId> =
                    keyframe.block.declarations(false).map(|d| d.id).collect();
                let easing = properties
                    .contains(&LonghandId::AnimationTimingFunction)
                    .then(|| style.rare.animations.timing_function[0].clone());
                for offset in &keyframe.offsets {
                    keyframes.push(Keyframe {
                        offset: f64::from(*offset),
                        style: style.clone(),
                        properties: properties.clone(),
                        easing: easing.clone(),
                    });
                }
            }
        }
        // Each keyframe's own easing; where none is written, linear.
        KeyframesInput::Declared(declared) => {
            for (keyframe, offset) in declared.keyframes.iter().zip(declared.computed_offsets()) {
                let style = Arc::new(restyle.with_keyframe(&keyframe.block));
                keyframes.push(Keyframe {
                    offset,
                    style,
                    properties: keyframe.block.declarations(false).map(|d| d.id).collect(),
                    easing: Some(keyframe.easing.clone()),
                });
            }
        }
    }
    let source = KeyframeSource {
        input,
        base: base.clone(),
        parent: Arc::new(restyle.parent().clone()),
        root_font_size,
        viewport,
    };
    Arc::new(KeyframeEffect::new(source, &keyframes, easing))
}

/// Whether a style's `animation-name` names something.
fn may_animate(style: &ComputedStyle) -> bool {
    style
        .rare
        .animations
        .name
        .iter()
        .any(|name| matches!(name, AnimationName::Name(_)))
}

/// Whether a style's `transition` could start one: some entry with time
/// (a duration or a delay above 0) for some property.
fn may_transition(style: &ComputedStyle) -> bool {
    let transitions = &style.transitions;
    !matches!(
        &*transitions.property,
        [computed_style::TransitionProperty::None]
    ) && (transitions.duration.iter().any(|d| *d > 0.0)
        || transitions.delay.iter().any(|d| *d > 0.0))
}
