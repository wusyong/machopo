//! `Element`'s names and attributes (https://dom.spec.whatwg.org/#interface-element).
//! An HTML element in an HTML document lowercases the qualified names it
//! is given (DOM's "get an attribute by name" and the others).

use dom::{NameId, NodeRef, QualName, StrRef, names};

use crate::exception::ExceptionName;
use crate::names_check::{self, NameContext};
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// The name of an element, the table's types promising one.
fn name<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> dom::NameIds {
    cx.element_name(this).expect("an element's name")
}

pub(crate) fn namespace_uri<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    let namespace = name(cx, this).namespace;
    Ok((namespace != names::NONE).then_some(DomStr::Name(namespace)))
}

pub(crate) fn prefix<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    let prefix = name(cx, this).prefix;
    Ok((prefix != names::NONE).then_some(DomStr::Name(prefix)))
}

pub(crate) fn local_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(name(cx, this).local))
}

/// `tagName`: its qualified name, uppercased as `nodeName` does.
pub(crate) fn tag_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(cx.node_name_id(this)))
}

/// A reflected attribute in no namespace (`id`, `className`), by its
/// name's number: its value where it is, or the table's empty string (one
/// string a DOM, not one a call).
pub(crate) fn reflected<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    local: NameId,
) -> DomStr<'gc> {
    cx.attribute_value_by_id(this, local)
        .map_or(DomStr::Name(names::NONE), DomStr::from)
}

pub(crate) fn id<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(reflected(cx, this, names::ID))
}

pub(crate) fn set_id<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("id"), value);
    Ok(())
}

pub(crate) fn class_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(reflected(cx, this, names::CLASS))
}

pub(crate) fn set_class_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("class"), value);
    Ok(())
}

pub(crate) fn has_attributes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(cx.attribute_count(this) > 0)
}

/// `qualified`, lowercased for an HTML element in an HTML document. A name
/// with no uppercase letter (nearly every one) is itself either way: the
/// element and its document are looked at only for one with some (Blink's
/// `Element::WeakLowercaseIfNecessary` keeps a lowercase name's atom).
fn qualified<'a, 'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified: &'a str,
) -> std::borrow::Cow<'a, str> {
    if qualified.bytes().any(|b| b.is_ascii_uppercase())
        && cx.is_html_element_in_html_document(this)
    {
        qualified.to_ascii_lowercase().into()
    } else {
        qualified.into()
    }
}

pub(crate) fn get_attribute<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified_name: &str,
) -> Result<Option<StrRef<'gc>>, ApiError> {
    Ok(cx.attribute_value(this, &qualified(cx, this, qualified_name)))
}

/// `getAttribute(name)`'s value, kept (WebDriver's Get Element
/// Attribute).
pub(crate) fn attribute_named<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    name: &str,
) -> Option<dom::DomString> {
    cx.attribute_value(element, &qualified(cx, element, name))
        .map(|value| value.to_owned())
}

pub(crate) fn get_attribute_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<Option<StrRef<'gc>>, ApiError> {
    Ok(cx.attribute_value_ns(this, namespace, local_name))
}

/// `setAttribute`: the name checked, then lowercased for an HTML element
/// in an HTML document.
pub(crate) fn set_attribute<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified_name: &str,
    value: &str,
) -> Result<(), ApiError> {
    if !names_check::is_valid_attribute_local_name(qualified_name) {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    cx.set_attribute(this, &qualified(cx, this, qualified_name), value);
    Ok(())
}

/// `setAttributeNS`: "validate and extract", then DOM's "set an attribute
/// value".
pub(crate) fn set_attribute_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    qualified_name: &str,
    value: &str,
) -> Result<(), ApiError> {
    let name =
        names_check::validate_and_extract(namespace, qualified_name, NameContext::Attribute)?;
    cx.set_attribute_ns(this, name, value);
    Ok(())
}

pub(crate) fn remove_attribute<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified_name: &str,
) -> Result<(), ApiError> {
    cx.remove_attribute(this, &qualified(cx, this, qualified_name));
    Ok(())
}

pub(crate) fn remove_attribute_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<(), ApiError> {
    cx.remove_attribute_ns(this, namespace, local_name);
    Ok(())
}

pub(crate) fn has_attribute<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified_name: &str,
) -> Result<bool, ApiError> {
    // Where the value is, not a clone of it (two atomic counts).
    Ok(cx
        .attribute_value(this, &qualified(cx, this, qualified_name))
        .is_some())
}

pub(crate) fn has_attribute_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<bool, ApiError> {
    Ok(cx.attribute_value_ns(this, namespace, local_name).is_some())
}

/// An attribute's qualified name as text: `prefix:local`, or its local name.
fn qualified_text(name: dom::NameIds) -> String {
    let local = dom::names::get(name.local);
    if name.prefix == dom::names::NONE {
        local.to_string()
    } else {
        format!("{}:{local}", dom::names::get(name.prefix))
    }
}

/// `getAttributeNames()`: its attributes' qualified names, in order.
pub(crate) fn get_attribute_names<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Vec<crate::typed::NameOrText>, ApiError> {
    use crate::typed::NameOrText;
    Ok((0..cx.attribute_count(this))
        .filter_map(|index| cx.attribute_at(this, index))
        .map(|attribute| {
            // Without a prefix (most), the qualified name is the local
            // name, one of the table's: no text made.
            if attribute.name.prefix == dom::names::NONE {
                NameOrText::Name(attribute.name.local)
            } else {
                NameOrText::Text(qualified_text(attribute.name))
            }
        })
        .collect())
}

/// `toggleAttribute(qualifiedName, force)`: the name checked (lowercased
/// for an HTML element in an HTML document); without `force`, the
/// attribute removed if it is there, else added empty; with it, made there
/// (`true`) or not. Whether it is there after.
pub(crate) fn toggle_attribute<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified_name: &str,
    force: Option<bool>,
) -> Result<bool, ApiError> {
    if !names_check::is_valid_attribute_local_name(qualified_name) {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    let name = qualified(cx, this, qualified_name);
    let present = cx.attribute_value(this, &name).is_some();
    match (present, force) {
        (false, None | Some(true)) => {
            cx.set_attribute(this, &name, "");
            Ok(true)
        }
        (false, Some(false)) => Ok(false),
        (true, None | Some(false)) => {
            cx.remove_attribute(this, &name);
            Ok(false)
        }
        (true, Some(true)) => Ok(true),
    }
}

/// `getElementsByTagNameNS(namespace, localName)` of a document or an
/// element.
pub(crate) fn get_elements_by_tag_name_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<elise::Gc<'gc, dom::HtmlCollection>, ApiError> {
    Ok(cx.elements_by_tag_name_ns(this, namespace, local_name))
}

/// DOM's "insert adjacent": `node` put `where` the element is
/// (`beforebegin`, `afterbegin`, `beforeend`, `afterend`, ASCII
/// case-insensitively; else a `SyntaxError`); none where it has no parent
/// to go in.
fn insert_adjacent<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    position: &str,
    node: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let (parent, child) = match position.to_ascii_lowercase().as_str() {
        "beforebegin" => match cx.parent(this) {
            Some(parent) => (parent, Some(this)),
            None => return Ok(None),
        },
        "afterbegin" => (this, cx.first_child(this)),
        "beforeend" => (this, None),
        "afterend" => match cx.parent(this) {
            Some(parent) => (parent, cx.next_sibling(this)),
            None => return Ok(None),
        },
        _ => {
            return Err(ApiError::Dom(crate::DomException {
                name: ExceptionName::Syntax,
                message: format!("'{position}' is not a valid position."),
            }));
        }
    };
    Ok(Some(cx.pre_insert(parent, node, child)?))
}

/// `insertAdjacentElement(where, element)`: the element, if it went in.
pub(crate) fn insert_adjacent_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    position: &str,
    element: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    insert_adjacent(cx, this, position, element)
}

/// `insertAdjacentText(where, data)`: a new text of the element's
/// document with `data`, put there.
pub(crate) fn insert_adjacent_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    position: &str,
    data: &str,
) -> Result<(), ApiError> {
    let text = cx.create_text(cx.document_of(this), data);
    insert_adjacent(cx, this, position, text).map(|_| ())
}
