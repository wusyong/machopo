//! Style sheets (https://drafts.csswg.org/cssom/#the-cssstylesheet-interface):
//! constructed ones (`new CSSStyleSheet()`, `replace`, `replaceSync`) and
//! those of `<style>`s and `<link>`s (`LinkStyle.sheet`), their `cssRules`,
//! `insertRule`, `deleteRule` and `media`; a document's and a shadow root's
//! `styleSheets` and `adoptedStyleSheets` (Blink's `CSSStyleSheet`,
//! `StyleSheetList`, `TreeScope::SetAdoptedStyleSheets`). `parentStyleSheet`
//! and `ownerRule` are `null`, as there are no `@import` rules; `replace`
//! parses at once, as Blink's does, so it fails only on an element's
//! sheet. A `<link>`'s sheet from another origin without CORS isn't
//! origin-clean: its rules can't be read or changed (`SecurityError`).

use std::sync::Arc;

use css::{InsertRuleError, StyleSheetContents};
use dom::{
    CssRule, CssRuleList, CssStyleSheet, DomString, MediaList, MediaOwner, NodeRef, RuleParent,
    Rules, StyleSheetList,
};
use elise::Gc;

use crate::exception::{DomException, ExceptionName};
use crate::interfaces::CSSStyleSheetInit;
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// A `DOMException` `name` with `message`.
pub(crate) fn dom_error(name: ExceptionName, message: impl Into<String>) -> ApiError {
    ApiError::Dom(DomException {
        name,
        message: message.into(),
    })
}

/// What the `url()`s of a sheet of `document` resolve against: `base`
/// (none: the document's URL), fetched for the document.
fn url_context<'gc>(
    cx: &ApiCx<'_, 'gc>,
    document: NodeRef<'gc>,
    base: Option<url::Url>,
) -> Option<css::UrlContext> {
    let url = cx
        .document_url(document)
        .and_then(|url| url::Url::parse(&url.to_str()).ok());
    let base = base.or_else(|| cx.document_base_url(document))?;
    Some(match url {
        Some(url) => css::UrlContext::new(base, url.as_str()),
        None => css::UrlContext::of_sheet(base),
    })
}

/// `new CSSStyleSheet(options)`: an empty sheet of the current realm's
/// document (its constructor document), with the options' `media` and
/// `disabled`, whose `url()`s resolve against `baseURL` (parsed against
/// the document's URL; that URL itself by default).
pub(crate) fn construct<'gc, O: crate::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    options: &CSSStyleSheetInit,
) -> Result<Gc<'gc, CssStyleSheet>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::dom(ExceptionName::NotAllowed))?;
    let document = cx.window_document(window);
    let base = match &options.baseURL {
        None => None,
        Some(text) => {
            let url = cx.document_base_url(document);
            let parsed = match &url {
                Some(url) => url.join(text),
                None => url::Url::parse(text),
            };
            Some(parsed.map_err(|_| {
                dom_error(
                    ExceptionName::NotAllowed,
                    format!("CSSStyleSheet: baseURL \"{text}\" isn't a URL"),
                )
            })?)
        }
    };
    let context = url_context(cx, document, base);
    Ok(cx.create_style_sheet(
        document,
        Arc::new(StyleSheetContents::parse("", context)),
        &options.media,
        options.disabled,
    ))
}

/// `type`: always `text/css`.
pub(crate) fn type_<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CssStyleSheet>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::fresh("text/css"))
}

/// `href`: a `<link>`'s sheet's URL as asked for; `null` for any other.
pub(crate) fn href<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
) -> Result<Option<DomString>, ApiError> {
    Ok(cx.style_sheet_href(this).map(DomString::from))
}

/// `ownerNode`: the element whose sheet it is, while it is; `null` for a
/// constructed sheet.
pub(crate) fn owner_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.style_sheet_owner(this))
}

/// `parentStyleSheet`: `null`, as there are no `@import` rules.
pub(crate) fn parent_style_sheet<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CssStyleSheet>,
) -> Result<Option<Gc<'gc, CssStyleSheet>>, ApiError> {
    Ok(None)
}

/// `ownerRule`: `null`, as there are no `@import` rules.
pub(crate) fn owner_rule<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, CssStyleSheet>,
) -> Result<Option<Gc<'gc, CssRule>>, ApiError> {
    Ok(None)
}

/// `title`: its element's `title` (`null` without one, or for a
/// constructed sheet).
pub(crate) fn title<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
) -> Result<Option<DomString>, ApiError> {
    Ok(cx.style_sheet_title(this).map(DomString::from))
}

/// `media` (`[SameObject]`: the sheet keeps the one list).
pub(crate) fn media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
) -> Result<Gc<'gc, MediaList>, ApiError> {
    Ok(cx.media_list(MediaOwner::Sheet(this)))
}

/// `media = value` (`[PutForwards=mediaText]`).
pub(crate) fn set_media<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    value: &str,
) -> Result<(), ApiError> {
    crate::cssom::set_media_text(cx, cx.media_list(MediaOwner::Sheet(this)), value)
}

pub(crate) fn disabled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
) -> Result<bool, ApiError> {
    Ok(cx.style_sheet_disabled(this))
}

pub(crate) fn set_disabled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    value: bool,
) -> Result<(), ApiError> {
    cx.set_style_sheet_disabled(this, value);
    Ok(())
}

/// `SecurityError` for `what` (`"Cannot access rules"`, Chrome's) on a
/// sheet that isn't origin-clean (CSSOM's).
fn check_origin_clean<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    what: &str,
) -> Result<(), ApiError> {
    if cx.style_sheet_origin_clean(this) {
        return Ok(());
    }
    Err(dom_error(ExceptionName::Security, what))
}

/// `cssRules` (`[SameObject]`), and the legacy `rules`: not of a sheet
/// that isn't origin-clean.
pub(crate) fn css_rules<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
) -> Result<Gc<'gc, CssRuleList>, ApiError> {
    check_origin_clean(cx, this, "Cannot access rules")?;
    Ok(cx.css_rule_list(RuleParent::Sheet(this)))
}

/// What `insertRule` throws for `error`, `len` the rules there are.
pub(crate) fn insert_rule_error(
    error: InsertRuleError,
    rule: &str,
    index: u32,
    len: usize,
) -> ApiError {
    match error {
        InsertRuleError::IndexSize => dom_error(
            ExceptionName::IndexSize,
            format!("insertRule: index {index} is past the {len} rules"),
        ),
        InsertRuleError::Syntax => dom_error(
            ExceptionName::Syntax,
            format!("insertRule: \"{rule}\" isn't a rule machopo parses"),
        ),
    }
}

/// What `deleteRule` throws: no rule at `index` of `len`.
pub(crate) fn delete_rule_error(index: u32, len: usize) -> ApiError {
    dom_error(
        ExceptionName::IndexSize,
        format!("deleteRule: no rule at index {index} of {len}"),
    )
}

fn index_of(index: u32) -> usize {
    usize::try_from(index).unwrap_or(usize::MAX)
}

/// `insertRule(rule, index)` (CSSOM's "insert a CSS rule"): the index
/// first (`IndexSizeError`), then exactly one rule machopo parses
/// (`SyntaxError`: an at-rule but `@media`, `@supports` and
/// `@keyframes`, `@import` included, is one). Returns the index.
pub(crate) fn insert_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    rule: &str,
    index: u32,
) -> Result<u32, ApiError> {
    check_origin_clean(cx, this, "Cannot access StyleSheet to insertRule")?;
    let contents = cx.style_sheet_contents(this);
    let inserted = contents
        .insert_rule(rule, index_of(index))
        .map_err(|error| insert_rule_error(error, rule, index, contents.rules().len()))?;
    cx.css_rule_inserted(
        RuleParent::Sheet(this),
        Rules::Sheet(inserted),
        index_of(index),
    );
    Ok(index)
}

/// `deleteRule(index)` (CSSOM's "remove a CSS rule").
pub(crate) fn delete_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    index: u32,
) -> Result<(), ApiError> {
    check_origin_clean(cx, this, "Cannot access StyleSheet to deleteRule")?;
    let contents = cx.style_sheet_contents(this);
    let deleted = contents
        .delete_rule(index_of(index))
        .ok_or_else(|| delete_rule_error(index, contents.rules().len()))?;
    cx.css_rule_deleted(
        RuleParent::Sheet(this),
        Rules::Sheet(deleted),
        index_of(index),
    );
    Ok(())
}

/// `addRule(selector, style, index)`, legacy: `insertRule` of
/// `selector { style }` (`selector { }` without one) at `index`, the end
/// by default. Returns -1, as CSSOM says.
pub(crate) fn add_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    selector: Option<&str>,
    style: Option<&str>,
    index: Option<u32>,
) -> Result<i32, ApiError> {
    check_origin_clean(cx, this, "Cannot access StyleSheet to insertRule")?;
    let style = style.unwrap_or("undefined");
    let mut rule = format!("{} {{ ", selector.unwrap_or("undefined"));
    if !style.is_empty() {
        rule.push_str(style);
        rule.push(' ');
    }
    rule.push('}');
    let index = index.unwrap_or_else(|| {
        u32::try_from(cx.rule_count(RuleParent::Sheet(this))).unwrap_or(u32::MAX)
    });
    insert_rule(cx, this, &rule, index)?;
    Ok(-1)
}

/// `removeRule(index)`, legacy: `deleteRule(index)`.
pub(crate) fn remove_rule<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    index: Option<u32>,
) -> Result<(), ApiError> {
    delete_rule(cx, this, index.unwrap_or(0))
}

/// `NotAllowedError` for `method` (`replace`, `replaceSync`) on an
/// element's sheet: only a constructed one may be.
fn check_constructed<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    method: &str,
) -> Result<(), ApiError> {
    if cx.style_sheet_is_constructed(this) {
        return Ok(());
    }
    Err(dom_error(
        ExceptionName::NotAllowed,
        format!("Can't call {method} on non-constructed CSSStyleSheets."),
    ))
}

/// `replaceSync(text)`: the sheet's rules are `text`'s, against its base
/// URL (CSSOM's "synchronously replace the rules": an `@import` is left
/// out, as machopo leaves out every at-rule but `@media`, `@supports` and
/// `@keyframes`). Only a constructed sheet's (`NotAllowedError`).
pub(crate) fn replace_sync<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    text: &str,
) -> Result<(), ApiError> {
    check_constructed(cx, this, "replaceSync")?;
    let context = cx.style_sheet_contents(this).url_context().cloned();
    cx.replace_style_sheet_contents(this, Arc::new(StyleSheetContents::parse(text, context)));
    Ok(())
}

/// `replace(text)`: as `replaceSync`, the promise resolved with the sheet
/// (the spec parses in parallel; Blink, and machopo, at once).
pub(crate) fn replace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, CssStyleSheet>,
    text: &str,
) -> Result<Gc<'gc, CssStyleSheet>, ApiError> {
    check_constructed(cx, this, "replace")?;
    replace_sync(cx, this, text)?;
    Ok(this)
}

/// `LinkStyle.sheet` (`<style>`'s, `<link>`'s, SVG's `<style>`'s): its
/// sheet, the same object while it is its sheet; `null` while it has
/// none (not connected, not CSS's `type`, a `<link>` that isn't a loaded
/// stylesheet).
pub(crate) fn sheet<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<Gc<'gc, CssStyleSheet>>, ApiError> {
    Ok(cx.owner_sheet(this))
}

/// `HTMLStyleElement.disabled`: its sheet's disabled flag, `false` while
/// it has no sheet (Blink's `HTMLStyleElement::disabled`).
pub(crate) fn style_disabled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(cx
        .owner_sheet(this)
        .is_some_and(|sheet| cx.style_sheet_disabled(sheet)))
}

/// Sets it, if the element has a sheet; else does nothing.
pub(crate) fn set_style_disabled<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    disabled: bool,
) -> Result<(), ApiError> {
    if let Some(sheet) = cx.owner_sheet(this) {
        cx.set_style_sheet_disabled(sheet, disabled);
    }
    Ok(())
}

/// `DocumentOrShadowRoot.styleSheets` (`[SameObject]`).
pub(crate) fn style_sheets<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, StyleSheetList>, ApiError> {
    Ok(cx.style_sheet_list(this))
}

/// `StyleSheetList.length`: the elements of its tree scope with a sheet.
pub(crate) fn style_sheet_list_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, StyleSheetList>,
) -> Result<u32, ApiError> {
    let len = cx.owner_sheets_in(cx.style_sheet_list_scope(this)).len();
    Ok(u32::try_from(len).unwrap_or(u32::MAX))
}

/// `item(index)` (and `styleSheets[index]`): the sheet of the element at
/// `index`; `null` past the end.
pub(crate) fn style_sheet_list_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, StyleSheetList>,
    index: u32,
) -> Result<Option<Gc<'gc, CssStyleSheet>>, ApiError> {
    Ok(cx
        .owner_sheets_in(cx.style_sheet_list_scope(this))
        .get(index_of(index))
        .copied())
}

/// `adoptedStyleSheets`' backing list: the sheets `this` (a document or a
/// shadow root) adopted, in order.
pub(crate) fn adopted<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Vec<Gc<'gc, CssStyleSheet>>, ApiError> {
    Ok(cx.adopted_style_sheets(this))
}

/// `adoptedStyleSheets`' set algorithm (CSSOM's: an element's sheet, or
/// one constructed for another document, is a `NotAllowedError`), then
/// `sheet` at `index` (the one there replaced; the delete algorithm is
/// CSSOM's nothing).
pub(crate) fn set_adopted<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    index: u32,
    sheet: Gc<'gc, CssStyleSheet>,
) -> Result<(), ApiError> {
    if !cx.style_sheet_is_constructed(sheet) {
        return Err(dom_error(
            ExceptionName::NotAllowed,
            "Can't adopt non-constructed stylesheets.",
        ));
    }
    if !elise::Gc::ptr_eq(cx.style_sheet_document(sheet), cx.document_of(this)) {
        return Err(ApiError::dom(ExceptionName::NotAllowed));
    }
    let index = usize::try_from(index).expect("an index no greater than the list's length");
    cx.set_adopted_style_sheet(this, index, sheet);
    Ok(())
}

/// `adoptedStyleSheets`' last sheet taken off (its delete algorithm:
/// nothing).
pub(crate) fn delete_adopted<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<(), ApiError> {
    cx.remove_last_adopted_style_sheet(this);
    Ok(())
}
