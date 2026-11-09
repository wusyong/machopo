//! `CSSStyleDeclaration` (https://drafts.csswg.org/cssom/#the-cssstyledeclaration-interface)
//! over an element's `style` attribute: `element.style` (Blink's
//! `InlineCSSStyleDeclaration` and `AbstractPropertySetCSSStyleDeclaration`);
//! over a style rule's or keyframe's declarations, its `style`
//! (`StyleRuleCSSStyleDeclaration`), which an edit puts into a new rule;
//! and over an element's computed style (`computed_style.rs`), read only.
//! The element's declarations are the one copy (`dom`'s
//! `inline_style`): a member that changes them takes them out, changes
//! them in place unless the style phase still holds them (copy on write),
//! and puts them back, the attribute serialized from them when it is next
//! read.

use std::sync::Arc;

use css::CssPropertyValueSet;
use dom::{CssStyleDeclaration, DeclarationOwner, DomString, NodeRef};
use elise::Gc;

use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// `element.style`: the same declaration each time (`[SameObject]`).
pub(crate) fn style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, CssStyleDeclaration>, ApiError> {
    Ok(cx.style_declaration(this))
}

/// `element.style = value` (`[PutForwards=cssText]`).
pub(crate) fn set_style<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set_css_text(cx, cx.style_declaration(this), value)
}

/// The declarations of `rule` (a style rule's, a keyframe's) as they are.
fn rule_block<'gc>(cx: &ApiCx<'_, 'gc>, rule: Gc<'gc, dom::CssRule>) -> CssPropertyValueSet {
    match cx.css_rule(rule) {
        css::CssRule::Keyframe(keyframe) => keyframe.block.clone(),
        css::CssRule::Style(style) => style.style().clone(),
        _ => unreachable!("a style rule's or a keyframe's declarations"),
    }
}

/// `rule` with `block` as its declarations, put in.
fn set_rule_block<'gc>(
    cx: &ApiCx<'_, 'gc>,
    rule: Gc<'gc, dom::CssRule>,
    block: CssPropertyValueSet,
) {
    let now = cx.css_rule(rule);
    let changed = match &now {
        css::CssRule::Keyframe(_) => now.with_keyframe_block(block),
        css::CssRule::Style(style) => css::CssRule::Style(Arc::new(style.with_style(block))),
        _ => unreachable!("a style rule's or a keyframe's declarations"),
    };
    cx.set_css_rule(rule, changed);
}

/// The computed style `this` shows, if it is a computed declaration's:
/// its element and which of its styles.
fn computed<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
) -> Option<(NodeRef<'gc>, dom::StyleTarget)> {
    match cx.style_declaration_owner(this) {
        DeclarationOwner::Computed(element, target) => Some((element, target)),
        _ => None,
    }
}

/// `NoModificationAllowedError` for a computed declaration: what each
/// member that changes the declarations checks first.
fn writable<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, CssStyleDeclaration>) -> Result<(), ApiError> {
    match computed(cx, this) {
        Some(_) => Err(crate::computed_style::read_only()),
        None => Ok(()),
    }
}

/// The declarations as they are (an element's none: empty; a computed
/// declaration has none of its own: its `cssText` and priorities are
/// empty, as Chrome's).
fn read<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    f: impl FnOnce(&CssPropertyValueSet) -> R,
) -> R {
    match cx.style_declaration_owner(this) {
        DeclarationOwner::Element(element) => match cx.inline_style(element) {
            Some(block) => f(&block),
            None => f(&CssPropertyValueSet::default()),
        },
        DeclarationOwner::Rule(rule) => f(&rule_block(cx, rule)),
        DeclarationOwner::Computed(..) => f(&CssPropertyValueSet::default()),
    }
}

/// The declarations changed by `f`, put back if it says they changed (a
/// rule's into a new rule).
fn change<'gc, R>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    f: impl FnOnce(&mut CssPropertyValueSet) -> (R, bool),
) -> R {
    let element = match cx.style_declaration_owner(this) {
        DeclarationOwner::Element(element) => element,
        DeclarationOwner::Rule(rule) => {
            let mut block = rule_block(cx, rule);
            let (result, changed) = f(&mut block);
            if changed {
                set_rule_block(cx, rule, block);
            }
            return result;
        }
        DeclarationOwner::Computed(..) => {
            unreachable!("checked: a computed declaration doesn't change")
        }
    };
    let mut block = cx.take_inline_style(element);
    let (result, changed) = f(Arc::make_mut(&mut block));
    if changed {
        cx.set_inline_style(element, block);
    } else {
        // As they were: put back, the attribute as it is (not read here:
        // with the declarations out, it would be written from none).
        cx.restore_inline_style(element, block);
    }
    result
}

/// What the `url()`s set resolve against: an element's document's URL as
/// it is now (Chrome parses them with its base URL); a rule's sheet's
/// (none for a rule out of its sheet).
fn base_url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
) -> Option<css::UrlContext> {
    match cx.style_declaration_owner(this) {
        DeclarationOwner::Element(element) => {
            let document = cx.document_of(element);
            let href = cx.document_url(document)?;
            let base = cx.document_base_url(document)?;
            Some(css::UrlContext::new(base, &href.to_str()))
        }
        DeclarationOwner::Rule(rule) => {
            let sheet = cx.css_rule_sheet(rule)?;
            cx.style_sheet_contents(sheet).url_context().cloned()
        }
        DeclarationOwner::Computed(..) => None,
    }
}

/// `parentRule`: a rule's declarations' rule; `null` for an element's
/// `style`.
pub(crate) fn parent_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
) -> Result<Option<Gc<'gc, dom::CssRule>>, ApiError> {
    Ok(match cx.style_declaration_owner(this) {
        DeclarationOwner::Element(_) | DeclarationOwner::Computed(..) => None,
        DeclarationOwner::Rule(rule) => Some(rule),
    })
}

pub(crate) fn css_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(read(
        cx,
        this,
        CssPropertyValueSet::css_text,
    ))))
}

/// Replaces the declarations with those `value` parses into, even if they
/// are the same (a mutation record each time, as in Chrome).
pub(crate) fn set_css_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    value: &str,
) -> Result<(), ApiError> {
    writable(cx, this)?;
    let mut block = CssPropertyValueSet::parse(value);
    // A value with no `url()` needs no base URL (an element's is its
    // document's URL parsed).
    if block.has_unresolved_urls()
        && let Some(base) = base_url(cx, this)
    {
        block.resolve_urls(&base);
    }
    match cx.style_declaration_owner(this) {
        DeclarationOwner::Element(element) => cx.set_inline_style(element, Arc::new(block)),
        DeclarationOwner::Rule(rule) => set_rule_block(cx, rule, block),
        DeclarationOwner::Computed(..) => {
            unreachable!("checked: a computed declaration doesn't change")
        }
    }
    Ok(())
}

pub(crate) fn length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
) -> Result<u32, ApiError> {
    if let Some((element, target)) = computed(cx, this) {
        let names = crate::computed_style::property_names(cx, element, target);
        return Ok(u32::try_from(names.len()).unwrap_or(u32::MAX));
    }
    Ok(read(cx, this, |block| {
        u32::try_from(block.len()).unwrap_or(u32::MAX)
    }))
}

/// `item(index)` (and `style[index]`): the `index`th declaration's
/// property name, the empty string past the end.
pub(crate) fn item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    index: u32,
) -> Result<DomStr<'gc>, ApiError> {
    let name = if let Some((element, target)) = computed(cx, this) {
        usize::try_from(index)
            .ok()
            .and_then(|index| {
                crate::computed_style::property_names(cx, element, target)
                    .into_iter()
                    .nth(index)
            })
            .map(DomString::from)
    } else {
        read(cx, this, |block| {
            usize::try_from(index)
                .ok()
                .and_then(|index| block.item(index))
                .map(DomString::from)
        })
    };
    // The empty string is the names table's.
    Ok(name.map_or(DomStr::Name(dom::names::NONE), DomStr::Owned))
}

pub(crate) fn get_property_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    property: &str,
) -> Result<DomStr<'gc>, ApiError> {
    // A computed declaration's: the property's resolved value.
    if let Some((element, target)) = computed(cx, this) {
        let value = crate::computed_style::property_value(cx, element, target, property);
        return Ok(DomStr::Owned(DomString::from(value)));
    }
    Ok(DomStr::Owned(DomString::from(read(cx, this, |block| {
        block.property_value(property)
    }))))
}

pub(crate) fn get_property_priority<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    property: &str,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(read(cx, this, |block| {
        block.property_priority(property)
    }))))
}

/// Changes the declarations only if they change: not for a property machopo
/// doesn't have, a value it can't parse, or the value and priority it
/// already has.
pub(crate) fn set_property<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    property: &str,
    value: &str,
    priority: Option<&str>,
) -> Result<(), ApiError> {
    writable(cx, this)?;
    change(cx, this, |block| {
        (
            (),
            block.set_property_resolved(property, value, priority.unwrap_or(""), || {
                base_url(cx, this)
            }),
        )
    });
    Ok(())
}

/// The property's value before; the declarations change only if it was
/// set.
pub(crate) fn remove_property<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    property: &str,
) -> Result<DomStr<'gc>, ApiError> {
    writable(cx, this)?;
    let value = change(cx, this, |block| block.remove_property(property));
    Ok(DomStr::Owned(DomString::from(value)))
}

pub(crate) fn css_float<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
) -> Result<DomStr<'gc>, ApiError> {
    get_property_value(cx, this, "float")
}

pub(crate) fn set_css_float<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    value: &str,
) -> Result<(), ApiError> {
    set_property(cx, this, "float", value, None)
}

/// `style.backgroundColor`, `style['background-color']` (CSSOM's camel-
/// cased and dashed attributes; Blink's `AnonymousNamedGetter`): the
/// property's value; none for a name that is no property machopo has, which
/// the front end leaves to the object's ordinary properties.
pub(crate) fn named_property<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    name: &str,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    let Some(property) = css::css_property_for_attribute(name) else {
        return Ok(None);
    };
    get_property_value(cx, this, property).map(Some)
}

/// The camel-cased name of every property machopo supports (Blink's
/// `NamedPropertyEnumerator`): the object's own keys.
pub(crate) fn supported_property_names<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CssStyleDeclaration>,
) -> Result<Vec<DomString>, ApiError> {
    Ok(css::css_property_attributes()
        .into_iter()
        .map(DomString::from)
        .collect())
}

/// `style.backgroundColor = value`: `setProperty(property, value)`.
/// Whether the name is a property's (Blink's `AnonymousNamedSetter`): any
/// other is an ordinary property of the object, as in Chrome.
pub(crate) fn set_named_property<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleDeclaration>,
    name: &str,
    value: &str,
) -> Result<bool, ApiError> {
    let Some(property) = css::css_property_for_attribute(name) else {
        return Ok(false);
    };
    set_property(cx, this, property, value, None)?;
    Ok(true)
}
