//! A sheet's rules' objects (https://drafts.csswg.org/cssom/#the-cssrule-interface,
//! css-conditional's `CSSMediaRule` and `CSSSupportsRule`, css-animations'
//! `CSSKeyframesRule` and `CSSKeyframeRule`), their `cssRules`
//! (`CSSRuleList`) and the media lists (`MediaList`): Blink's `CSSRule` and
//! its subclasses, `CSSRuleList` and `MediaList`. `css` parses and edits
//! the rules; `dom` keeps them and their objects in step.

use std::sync::Arc;

use css::CssRule as Rule;
use css::media::MediaQueryList;
use dom::{
    CssRule, CssRuleList, CssStyleDeclaration, CssStyleSheet, DomString, MediaList, MediaOwner,
    RuleKind, RuleParent, Rules,
};
use elise::Gc;

use crate::css_style_sheet::{delete_rule_error, dom_error, insert_rule_error};
use crate::exception::ExceptionName;
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

fn index_of(index: u32) -> usize {
    usize::try_from(index).unwrap_or(usize::MAX)
}

fn count(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}

/// The `CSSRuleList`'s `length`: its parent's rules.
pub(crate) fn rule_list_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRuleList>,
) -> Result<u32, ApiError> {
    Ok(count(cx.rule_count(cx.rule_list_parent(this))))
}

/// `item(index)` (and `cssRules[index]`): the object of the parent's rule
/// at `index`, the same each time; `null` past the end.
pub(crate) fn rule_list_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRuleList>,
    index: u32,
) -> Result<Option<Gc<'gc, CssRule>>, ApiError> {
    Ok(cx.rule_at(cx.rule_list_parent(this), index_of(index)))
}

/// `CSSRule.cssText`: the rule serialized (`.a { color: red; }`, a group
/// rule over lines).
pub(crate) fn css_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(cx.css_rule(this).css_text())))
}

/// Setting `CSSRule.cssText` does nothing (CSSOM).
pub(crate) fn set_css_text<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CssRule>,
    _: &str,
) -> Result<(), ApiError> {
    Ok(())
}

/// `parentRule`: the group rule it is in; `null` in a sheet, or once
/// taken out.
pub(crate) fn parent_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<Option<Gc<'gc, CssRule>>, ApiError> {
    Ok(cx.css_rule_parent(this))
}

/// `parentStyleSheet`: the sheet it is in, through its group rules;
/// `null` once it, or one of them, was taken out.
pub(crate) fn parent_style_sheet<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<Option<Gc<'gc, CssStyleSheet>>, ApiError> {
    Ok(cx.css_rule_sheet(this))
}

/// `type`: `STYLE_RULE`, `MEDIA_RULE`, `SUPPORTS_RULE`, `KEYFRAMES_RULE`
/// or `KEYFRAME_RULE`.
pub(crate) fn rule_type<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, CssRule>) -> Result<u32, ApiError> {
    Ok(match cx.css_rule_kind(this) {
        RuleKind::Style => 1,
        RuleKind::Media => 4,
        RuleKind::Keyframes => 7,
        RuleKind::Keyframe => 8,
        RuleKind::Supports => 12,
    })
}

/// The style rule `this` is: the front end calls a style rule's members
/// only on a style rule's object.
fn style_rule<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, CssRule>) -> Arc<css::StyleRule> {
    cx.css_rule(this)
        .as_style()
        .expect("a style rule's member on a style rule")
        .clone()
}

pub(crate) fn selector_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(
        style_rule(cx, this).selector_text(),
    )))
}

/// Setting `selectorText`: the rule takes the selectors if they parse,
/// and nothing changes if they don't (CSSOM).
pub(crate) fn set_selector_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    value: &str,
) -> Result<(), ApiError> {
    if let Some(changed) = style_rule(cx, this).with_selector_text(value) {
        cx.set_css_rule(this, Rule::Style(Arc::new(changed)));
    }
    Ok(())
}

/// A style rule's or a keyframe's `style` (`[SameObject]`).
pub(crate) fn style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<Gc<'gc, CssStyleDeclaration>, ApiError> {
    Ok(cx.css_rule_style(this))
}

/// `style = value` (`[PutForwards=cssText]`).
pub(crate) fn set_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    value: &str,
) -> Result<(), ApiError> {
    crate::css_style::set_css_text(cx, cx.css_rule_style(this), value)
}

/// A group rule's (and a `@keyframes`') `cssRules` (`[SameObject]`).
pub(crate) fn css_rules<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<Gc<'gc, CssRuleList>, ApiError> {
    Ok(cx.css_rule_list(RuleParent::Rule(this)))
}

/// A group rule's `insertRule(rule, index)`: as the sheet's, in the group
/// rule.
pub(crate) fn insert_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    rule: &str,
    index: u32,
) -> Result<u32, ApiError> {
    let group = cx.css_rule(this);
    let len = group.child_rules().map_or(0, <[Rule]>::len);
    let inserted = group
        .insert_rule(rule, index_of(index))
        .map_err(|error| insert_rule_error(error, rule, index, len))?;
    cx.css_rule_inserted(
        RuleParent::Rule(this),
        Rules::Rule(inserted),
        index_of(index),
    );
    Ok(index)
}

/// A group rule's `deleteRule(index)`.
pub(crate) fn delete_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    index: u32,
) -> Result<(), ApiError> {
    let group = cx.css_rule(this);
    let len = group.child_rules().map_or(0, <[Rule]>::len);
    let deleted = group
        .delete_rule(index_of(index))
        .ok_or_else(|| delete_rule_error(index, len))?;
    cx.css_rule_deleted(
        RuleParent::Rule(this),
        Rules::Rule(deleted),
        index_of(index),
    );
    Ok(())
}

/// `conditionText`: a media rule's media serialized, a supports rule's
/// condition as written.
pub(crate) fn condition_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<DomStr<'gc>, ApiError> {
    let text = cx
        .css_rule(this)
        .condition_text()
        .expect("a condition rule");
    Ok(DomStr::Owned(DomString::from(text)))
}

/// A media rule's `media` (`[SameObject]`).
pub(crate) fn rule_media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<Gc<'gc, MediaList>, ApiError> {
    Ok(cx.media_list(MediaOwner::Rule(this)))
}

/// `media = value` (`[PutForwards=mediaText]`).
pub(crate) fn set_rule_media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    value: &str,
) -> Result<(), ApiError> {
    set_media_text(cx, cx.media_list(MediaOwner::Rule(this)), value)
}

/// `CSSKeyframesRule.name`: as written, a string's without its quotes.
pub(crate) fn keyframes_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<DomStr<'gc>, ApiError> {
    let Rule::Keyframes(rule) = cx.css_rule(this) else {
        unreachable!("a keyframes rule's member on a keyframes rule");
    };
    Ok(DomStr::Owned(DomString::from(&*rule.name)))
}

/// Setting `CSSKeyframesRule.name`: any string (`none` and the CSS-wide
/// keywords too, written as strings then).
pub(crate) fn set_keyframes_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    value: &str,
) -> Result<(), ApiError> {
    let renamed = cx.css_rule(this).with_keyframes_name(value);
    cx.set_css_rule(this, renamed);
    Ok(())
}

/// `CSSKeyframesRule.length`: its keyframes.
pub(crate) fn keyframes_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<u32, ApiError> {
    Ok(count(cx.rule_count(RuleParent::Rule(this))))
}

/// `keyframes[index]`: the keyframe's object at `index`, as `cssRules`
/// gives it.
pub(crate) fn keyframes_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    index: u32,
) -> Result<Option<Gc<'gc, CssRule>>, ApiError> {
    Ok(cx.rule_at(RuleParent::Rule(this), index_of(index)))
}

/// `appendRule(rule)`: `rule` as the last keyframe; nothing if it isn't
/// exactly one.
pub(crate) fn append_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    rule: &str,
) -> Result<(), ApiError> {
    let keyframes = cx.css_rule(this);
    let len = keyframes.child_rules().map_or(0, <[Rule]>::len);
    if let Some(appended) = keyframes.append_keyframe(rule) {
        cx.css_rule_inserted(RuleParent::Rule(this), Rules::Rule(appended), len);
    }
    Ok(())
}

/// `CSSKeyframesRule.deleteRule(select)`: the last keyframe whose
/// selectors are `select`, if any.
pub(crate) fn keyframes_delete_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    select: &str,
) -> Result<(), ApiError> {
    let keyframes = cx.css_rule(this);
    let Some(index) = keyframes.find_keyframe(select) else {
        return Ok(());
    };
    let deleted = keyframes
        .delete_rule(index)
        .expect("the keyframe found is there");
    cx.css_rule_deleted(RuleParent::Rule(this), Rules::Rule(deleted), index);
    Ok(())
}

/// `findRule(select)`: the object of the last keyframe whose selectors are
/// `select`; `null` if none is.
pub(crate) fn find_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    select: &str,
) -> Result<Option<Gc<'gc, CssRule>>, ApiError> {
    let Some(index) = cx.css_rule(this).find_keyframe(select) else {
        return Ok(None);
    };
    Ok(cx.rule_at(RuleParent::Rule(this), index))
}

/// `CSSKeyframeRule.keyText`: `0%`, `25%, 75%`.
pub(crate) fn key_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
) -> Result<DomStr<'gc>, ApiError> {
    let Rule::Keyframe(rule) = cx.css_rule(this) else {
        unreachable!("a keyframe rule's member on a keyframe rule");
    };
    Ok(DomStr::Owned(DomString::from(rule.key_text())))
}

/// Setting `keyText`: the selectors parsed; a `SyntaxError` (and nothing
/// changes) if they don't.
pub(crate) fn set_key_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssRule>,
    value: &str,
) -> Result<(), ApiError> {
    let changed = cx
        .css_rule(this)
        .with_key_text(value)
        .ok_or_else(|| dom_error(ExceptionName::Syntax, "not a keyframe selector"))?;
    cx.set_css_rule(this, changed);
    Ok(())
}

/// `list`'s queries now: its sheet's, or its media rule's.
fn media_of<'gc>(cx: &ApiCx<'_, 'gc>, list: Gc<'gc, MediaList>) -> Arc<MediaQueryList> {
    match cx.media_list_owner(list) {
        MediaOwner::Sheet(sheet) => cx.style_sheet_media(sheet).1.clone(),
        MediaOwner::Rule(rule) => match cx.css_rule(rule) {
            Rule::Media(media) => media.media.clone(),
            _ => unreachable!("a media list is a media rule's"),
        },
    }
}

/// Puts `media` in as the list's owner's: a sheet's media (written as
/// they serialize), or a new media rule.
fn set_media<'gc>(cx: &ApiCx<'_, 'gc>, list: Gc<'gc, MediaList>, media: MediaQueryList) {
    match cx.media_list_owner(list) {
        MediaOwner::Sheet(sheet) => {
            cx.set_style_sheet_media(sheet, &media.to_css(), Arc::new(media))
        }
        MediaOwner::Rule(rule) => {
            let changed = cx.css_rule(rule).with_media(media);
            cx.set_css_rule(rule, changed);
        }
    }
}

/// `mediaText`: the queries serialized.
pub(crate) fn media_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MediaList>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(media_of(cx, this).to_css())))
}

/// Setting `mediaText`: the queries `value` parses into (none for the
/// empty string).
pub(crate) fn set_media_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MediaList>,
    value: &str,
) -> Result<(), ApiError> {
    set_media(cx, this, MediaQueryList::parse(value));
    Ok(())
}

pub(crate) fn media_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MediaList>,
) -> Result<u32, ApiError> {
    Ok(count(media_of(cx, this).queries().count()))
}

/// `item(index)`: the query at `index`, serialized; `null` past the end.
pub(crate) fn media_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MediaList>,
    index: u32,
) -> Result<Option<DomString>, ApiError> {
    Ok(media_of(cx, this)
        .queries()
        .nth(index_of(index))
        .map(DomString::from))
}

/// `appendMedium(medium)`: adds the query, unless it doesn't parse or is
/// in the list already.
pub(crate) fn append_medium<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MediaList>,
    medium: &str,
) -> Result<(), ApiError> {
    if let Some(media) = media_of(cx, this).with_medium(medium) {
        set_media(cx, this, media);
    }
    Ok(())
}

/// `deleteMedium(medium)`: takes the query out; `NotFoundError` if it
/// isn't in the list.
pub(crate) fn delete_medium<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, MediaList>,
    medium: &str,
) -> Result<(), ApiError> {
    let media = media_of(cx, this).without_medium(medium).ok_or_else(|| {
        dom_error(
            ExceptionName::NotFound,
            format!("deleteMedium: \"{medium}\" isn't in the list"),
        )
    })?;
    set_media(cx, this, media);
    Ok(())
}
