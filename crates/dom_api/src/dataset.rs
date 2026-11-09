//! `DOMStringMap` (https://html.spec.whatwg.org/#domstringmap): an
//! element's `dataset`, its `data-*` attributes by name (Blink's
//! `DatasetDOMStringMap`), as named properties: read from the attributes
//! at each ask; setting and deleting change them, as `setAttribute` does.

use dom::{DomString, DomStringMap, NodeRef, QualName, StrRef, names};
use elise::Gc;

use crate::exception::{DomException, ExceptionName};
use crate::{ApiCx, ApiError, names_check};

/// `element.dataset`: the same map each time (`[SameObject]`).
pub(crate) fn dataset<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, DomStringMap>, ApiError> {
    Ok(cx.dataset(this))
}

/// HTML's "list of name-value pairs", each given to `found` until it says
/// stop (`false`): each attribute in the null namespace whose local name is
/// `data-` and then no ASCII upper alpha, in order, its name with `data-`
/// removed and each `-` before an ASCII lower alpha removed, that letter
/// uppercased (`data-foo-bar` is `fooBar`).
fn pairs<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    mut found: impl FnMut(String, StrRef<'gc>) -> bool,
) {
    for index in 0..cx.attribute_count(element) {
        let name = cx
            .attribute_name_at(element, index)
            .expect("an attribute below the count");
        if name.namespace() != names::NONE {
            continue;
        }
        let local = names::get(name.local());
        let Some(rest) = local.strip_prefix("data-") else {
            continue;
        };
        if rest.bytes().any(|byte| byte.is_ascii_uppercase()) {
            continue;
        }
        let value = cx
            .attribute_value_at(element, index)
            .expect("an attribute below the count");
        if !found(camel_cased(rest), value) {
            return;
        }
    }
}

fn camel_cased(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut chars = name.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(next) if c == '-' && next.is_ascii_lowercase() => {
                out.push(next.to_ascii_uppercase());
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// Whether `name` has a `-` before an ASCII lower alpha, which no pair's
/// name can have (`fooBar`'s attribute is `data-foo-bar`, so `foo-bar`
/// names nothing).
fn has_dash_before_lower(name: &str) -> bool {
    name.as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'-' && pair[1].is_ascii_lowercase())
}

/// The attribute's local name for `name`: `data-`, then `name` with each
/// ASCII upper alpha lowercased after a `-` (`fooBar` is `data-foo-bar`).
fn attribute_name(name: &str) -> String {
    let mut out = String::from("data-");
    for c in name.chars() {
        if c.is_ascii_uppercase() {
            out.push('-');
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// The supported property names: the pairs' names, in order.
pub(crate) fn names<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomStringMap>,
) -> Result<Vec<DomString>, ApiError> {
    let mut names = Vec::new();
    pairs(cx, cx.dataset_element(this), |name, _| {
        names.push(DomString::new(&name));
        true
    });
    Ok(names)
}

/// The named getter: the value of the first pair named `name`, or none
/// (not a named property).
pub(crate) fn get<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomStringMap>,
    name: &str,
) -> Result<Option<StrRef<'gc>>, ApiError> {
    let mut value = None;
    pairs(cx, cx.dataset_element(this), |pair, found| {
        if pair != name {
            return true;
        }
        value = Some(found);
        false
    });
    Ok(value)
}

/// The named setter: `name` with a `-` before an ASCII lower alpha is a
/// `SyntaxError`; its attribute name not a valid one an
/// `InvalidCharacterError`; else that attribute in the null namespace
/// gets `value`.
pub(crate) fn set<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomStringMap>,
    name: &str,
    value: &str,
) -> Result<(), ApiError> {
    if has_dash_before_lower(name) {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::Syntax,
            message: format!("'{name}' has a '-' followed by a lowercase letter"),
        }));
    }
    let local = attribute_name(name);
    if !names_check::is_valid_attribute_local_name(&local) {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    cx.set_attribute_ns(cx.dataset_element(this), QualName::local(&local), value);
    Ok(())
}

/// The named deleter: removes the attribute `name` is (nothing for a name
/// with a `-` before an ASCII lower alpha, which no attribute is).
pub(crate) fn delete<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, DomStringMap>,
    name: &str,
) -> Result<(), ApiError> {
    if !has_dash_before_lower(name) {
        cx.remove_attribute_ns(cx.dataset_element(this), None, &attribute_name(name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_convert_both_ways() {
        assert_eq!(camel_cased("foo-bar"), "fooBar");
        assert_eq!(camel_cased("foo--bar"), "foo-Bar");
        assert_eq!(camel_cased("-foo"), "Foo");
        assert_eq!(camel_cased("foo-"), "foo-");
        assert_eq!(camel_cased("foo-1"), "foo-1");
        assert_eq!(camel_cased("é-é"), "é-é");
        assert_eq!(attribute_name("fooBar"), "data-foo-bar");
        assert_eq!(attribute_name("Foo"), "data--foo");
        assert_eq!(attribute_name("foo-1"), "data-foo-1");
        assert!(has_dash_before_lower("foo-bar"));
        assert!(!has_dash_before_lower("foo-Bar"));
        assert!(!has_dash_before_lower("foo-"));
    }
}
