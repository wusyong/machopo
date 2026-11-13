//! `DOMTokenList` (https://dom.spec.whatwg.org/#interface-domtokenlist):
//! one of an element's attributes as an ordered set of tokens: `classList`
//! (`class`), a form's `relList` (`rel`), an output's `htmlFor` (`for`).
//! `class`'s set is the element's classes as style has them (each once, in
//! order, interned: `contains` compares numbers); another's is its
//! attribute's value parsed at each ask (few pages use them). A change
//! writes the attribute (the "update steps").

use dom::{DomTokenList, NodeRef, QualName, names};
use elise::Gc;

use crate::exception::{DomException, ExceptionName};
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// `element.classList`: the same list each time (`[SameObject]`).
pub(crate) fn class_list<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomTokenList>, ApiError> {
    Ok(cx.class_list(this))
}

/// `classList = value` (`[PutForwards=value]`).
pub(crate) fn set_class_list<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set_value(cx, cx.class_list(this), value)
}

/// A form's `relList`: the same list each time.
pub(crate) fn rel_list<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomTokenList>, ApiError> {
    Ok(cx.attribute_token_list(this, names::local::REL))
}

pub(crate) fn set_rel_list<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set_value(cx, cx.attribute_token_list(this, names::local::REL), value)
}

/// An output's `htmlFor`: the same list each time.
pub(crate) fn html_for<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomTokenList>, ApiError> {
    Ok(cx.attribute_token_list(this, names::local::FOR))
}

pub(crate) fn set_html_for<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    set_value(cx, cx.attribute_token_list(this, names::local::FOR), value)
}

fn element<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomTokenList>) -> NodeRef<'gc> {
    cx.token_list_element(this)
}

fn is_class<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomTokenList>) -> bool {
    cx.token_list_attribute(this) == names::CLASS
}

/// Another attribute's tokens: its value split on ASCII whitespace, each
/// once (DOM's ordered set parser).
fn parsed<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomTokenList>) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    if let Some(value) = cx.attribute_value_by_id(element(cx, this), cx.token_list_attribute(this))
    {
        for token in value
            .borrow()
            .to_str()
            .split(|c: char| c.is_ascii_whitespace())
        {
            if !token.is_empty() && !tokens.iter().any(|have| have == token) {
                tokens.push(token.to_owned());
            }
        }
    }
    tokens
}

pub(crate) fn length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
) -> Result<u32, ApiError> {
    let tokens = if is_class(cx, this) {
        cx.class_names(element(cx, this)).len()
    } else {
        parsed(cx, this).len()
    };
    Ok(u32::try_from(tokens)
        .expect("an attribute's tokens are fewer than its UTF-16 length, a u32"))
}

/// `item(index)`, and the indexed getter: null past the last.
pub(crate) fn item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    index: u32,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    let index = usize::try_from(index).ok();
    if !is_class(cx, this) {
        let tokens = parsed(cx, this);
        return Ok(index
            .and_then(|index| tokens.get(index))
            .map(|token| DomStr::fresh(token.to_string())));
    }
    let classes = cx.class_names(element(cx, this));
    Ok(index
        .and_then(|index| classes.get(index))
        .map(|class| DomStr::Name(class.name)))
}

pub(crate) fn contains<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    token: &str,
) -> Result<bool, ApiError> {
    if !is_class(cx, this) {
        return Ok(parsed(cx, this).iter().any(|have| have == token));
    }
    // The classes first: made now if stale, they intern their names. A
    // name the table hasn't then is no class's.
    let classes = cx.class_names(element(cx, this));
    Ok(names::find(token).is_some_and(|name| classes.iter().any(|class| class.name == name)))
}

/// A token the methods take: an empty one is a `SyntaxError`, one with
/// ASCII whitespace an `InvalidCharacterError`.
fn check(token: &str) -> Result<(), ApiError> {
    check_empty(token)?;
    if token.bytes().any(|byte| byte.is_ascii_whitespace()) {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::InvalidCharacter,
            message: format!("the token '{token}' has whitespace"),
        }));
    }
    Ok(())
}

fn check_empty(token: &str) -> Result<(), ApiError> {
    if token.is_empty() {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::Syntax,
            message: "the token is empty".to_owned(),
        }));
    }
    Ok(())
}

/// The set's tokens, as text.
fn tokens<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomTokenList>) -> Vec<String> {
    if !is_class(cx, this) {
        return parsed(cx, this);
    }
    cx.class_names(element(cx, this))
        .iter()
        .map(|class| names::get(class.name).to_string())
        .collect()
}

/// DOM's "update steps": the attribute set to the set's serialization,
/// unless it is missing and the set empty.
fn update<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, DomTokenList>, tokens: &[String]) {
    let (element, attribute) = (element(cx, this), cx.token_list_attribute(this));
    if tokens.is_empty() && cx.attribute_value_by_id(element, attribute).is_none() {
        return;
    }
    cx.set_attribute_ns(
        element,
        QualName::local(&names::get(attribute)),
        &tokens.join(" "),
    );
}

pub(crate) fn add<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    added: Vec<&str>,
) -> Result<(), ApiError> {
    added.iter().try_for_each(|token| check(token))?;
    let mut tokens = tokens(cx, this);
    for token in added {
        if !tokens.iter().any(|have| have == token) {
            tokens.push(token.to_owned());
        }
    }
    update(cx, this, &tokens);
    Ok(())
}

pub(crate) fn remove<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    removed: Vec<&str>,
) -> Result<(), ApiError> {
    removed.iter().try_for_each(|token| check(token))?;
    let mut tokens = tokens(cx, this);
    tokens.retain(|have| !removed.contains(&have.as_str()));
    update(cx, this, &tokens);
    Ok(())
}

/// `toggle(token, force)`: whether the token is there after.
pub(crate) fn toggle<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    token: &str,
    force: Option<bool>,
) -> Result<bool, ApiError> {
    check(token)?;
    let mut tokens = tokens(cx, this);
    match tokens.iter().position(|have| have == token) {
        Some(index) if force != Some(true) => {
            tokens.remove(index);
            update(cx, this, &tokens);
            Ok(false)
        }
        Some(_) => Ok(true),
        None if force != Some(false) => {
            tokens.push(token.to_owned());
            update(cx, this, &tokens);
            Ok(true)
        }
        None => Ok(false),
    }
}

/// `replace(token, newToken)`: whether `token` was there. The first of the
/// two becomes `new_token`, the other goes (an ordered set's "replace").
pub(crate) fn replace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    token: &str,
    new_token: &str,
) -> Result<bool, ApiError> {
    // Both for emptiness first, then both for whitespace.
    check_empty(token)?;
    check_empty(new_token)?;
    check(token)?;
    check(new_token)?;
    let mut tokens = tokens(cx, this);
    let Some(index) = tokens.iter().position(|have| have == token) else {
        return Ok(false);
    };
    tokens[index] = new_token.to_owned();
    // The set had `token` once: the first `new_token` stays, the other goes.
    let mut kept = false;
    tokens.retain(|have| have != new_token || !std::mem::replace(&mut kept, true));
    update(cx, this, &tokens);
    Ok(true)
}

/// `supports(token)`: whether the attribute's supported tokens have it,
/// ASCII case-insensitively; `class`, a form's `rel` and an output's
/// `for` have none, a `TypeError`.
pub(crate) fn supports<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    token: &str,
) -> Result<bool, ApiError> {
    let attribute = cx.token_list_attribute(this);
    let Some(tokens) = supported_tokens(cx, element(cx, this), attribute) else {
        return Err(ApiError::type_error(format!(
            "the '{}' attribute has no supported tokens",
            names::get(attribute)
        )));
    };
    Ok(tokens
        .iter()
        .any(|supported| supported.eq_ignore_ascii_case(token)))
}

/// `attribute`'s supported tokens on `element`: a `<link>`'s `rel`, those
/// it does something for; an `<a>`'s and an `<area>`'s, those a
/// navigation follows (Blink's `HTMLAnchorElement::relList` supports the
/// same three); none for any other.
fn supported_tokens<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    attribute: dom::NameId,
) -> Option<&'static [&'static str]> {
    if attribute != names::local::REL {
        return None;
    }
    let name = cx.qualified_name(element)?;
    if name == dom::QualifiedName::html(names::local::LINK) {
        Some(&["modulepreload", "stylesheet"])
    } else if name == dom::QualifiedName::html(names::local::A)
        || name == dom::QualifiedName::html(names::local::AREA)
    {
        Some(&["noopener", "noreferrer", "opener"])
    } else {
        None
    }
}

/// `value`, and the stringifier: the attribute's value, or empty.
pub(crate) fn value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(crate::element::reflected(
        cx,
        element(cx, this),
        cx.token_list_attribute(this),
    ))
}

pub(crate) fn set_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomTokenList>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(
        element(cx, this),
        QualName::local(&names::get(cx.token_list_attribute(this))),
        value,
    );
    Ok(())
}
