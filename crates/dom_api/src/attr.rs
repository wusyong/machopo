//! `Attr` and `NamedNodeMap` (https://dom.spec.whatwg.org/#interface-attr,
//! #interface-namednodemap), and the members that give or take them
//! (`Element`'s `attributes`, `getAttributeNode`, `setAttributeNode`,
//! `removeAttributeNode`; `Document`'s `createAttribute`): an `Attr` is the
//! DOM's (`dom::attr`), made as asked for.

use dom::{NamedNodeMap, NodeRef, QualName, StrRef, names};
use elise::Gc;

use crate::exception::ExceptionName;
use crate::names_check;
use crate::typed::DomStr;
use crate::{ApiCx, ApiError, lowercase};

pub(crate) fn namespace_uri<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    let name = cx.attr_name(this);
    Ok((name.namespace != names::NONE).then_some(DomStr::Name(name.namespace)))
}

pub(crate) fn prefix<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    let name = cx.attr_name(this);
    Ok((name.prefix != names::NONE).then_some(DomStr::Name(name.prefix)))
}

pub(crate) fn local_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(cx.attr_name(this).local))
}

/// `name`: its qualified name (`nodeName`'s).
pub(crate) fn name<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(cx.node_name_id(this)))
}

pub(crate) fn value<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<StrRef<'gc>, ApiError> {
    Ok(cx.attr_value(this))
}

/// Setting `value` (DOM's "set an existing attribute value"): its
/// element's attribute changes if it has one.
pub(crate) fn set_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attr_value(this, value);
    Ok(())
}

pub(crate) fn owner_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.attr_owner(this))
}

/// `specified`: always true (historical).
pub(crate) fn specified<'gc>(_: &ApiCx<'_, 'gc>, _: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(true)
}

/// A qualified name lowercased for an HTML element in an HTML document
/// (DOM's "get an attribute by name").
fn qualified<'a, 'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    name: &'a str,
) -> std::borrow::Cow<'a, str> {
    if cx.is_html_element_in_html_document(element) {
        lowercase(name)
    } else {
        name.into()
    }
}

/// `attributes`: the same map each time.
pub(crate) fn attributes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, NamedNodeMap>, ApiError> {
    Ok(cx.attribute_map(this))
}

pub(crate) fn get_attribute_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    qualified_name: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.attr_node(this, &qualified(cx, this, qualified_name)))
}

pub(crate) fn get_attribute_node_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.attr_node_ns(this, namespace, local_name))
}

/// `setAttributeNode(attr)`, `setAttributeNodeNS(attr)`: the attribute of
/// its namespace and local name replaced (that one given back), else it
/// is added; an `InUseAttributeError` for another element's.
pub(crate) fn set_attribute_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    attr: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.set_attribute_node(this, attr)?)
}

/// `removeAttributeNode(attr)`: a `NotFoundError` for one not its.
pub(crate) fn remove_attribute_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    attr: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.remove_attribute_node(this, attr)?)
}

/// `createAttribute(localName)`: the name checked, lowercased in an HTML
/// document; no namespace.
pub(crate) fn create_attribute<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    local_name: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    if !names_check::is_valid_attribute_local_name(local_name) {
        return Err(ApiError::dom(ExceptionName::InvalidCharacter));
    }
    let local = if cx.is_html_document(this) {
        lowercase(local_name)
    } else {
        local_name.into()
    };
    Ok(cx.create_attr(this, QualName::local(&local), ""))
}

/// `createAttributeNS(namespace, qualifiedName)`: DOM's "validate and
/// extract" as an attribute's.
pub(crate) fn create_attribute_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
    qualified_name: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    let name = names_check::validate_and_extract(
        namespace,
        qualified_name,
        names_check::NameContext::Attribute,
    )?;
    Ok(cx.create_attr(this, name, ""))
}

// --- NamedNodeMap ------------------------------------------------------------

/// `length`: how many attributes its element has (WebIDL `unsigned long`).
pub(crate) fn map_length<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
) -> Result<u32, ApiError> {
    let element = cx.attribute_map_element(this);
    Ok(u32::try_from(cx.attribute_count(element)).unwrap_or(u32::MAX))
}

/// `item(index)` and `map[index]`: the `Attr` of the attribute there.
pub(crate) fn map_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
    index: u32,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let element = cx.attribute_map_element(this);
    Ok(usize::try_from(index)
        .ok()
        .and_then(|index| cx.attr_node_at(element, index)))
}

/// `getNamedItem(qualifiedName)` and `map[name]`.
pub(crate) fn get_named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
    qualified_name: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    get_attribute_node(cx, cx.attribute_map_element(this), qualified_name)
}

pub(crate) fn get_named_item_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    get_attribute_node_ns(cx, cx.attribute_map_element(this), namespace, local_name)
}

/// `setNamedItem(attr)`, `setNamedItemNS(attr)`: `setAttributeNode`'s.
pub(crate) fn set_named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
    attr: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    set_attribute_node(cx, cx.attribute_map_element(this), attr)
}

/// `removeNamedItem(qualifiedName)`: its `Attr`, taken off; a
/// `NotFoundError` for a name it hasn't.
pub(crate) fn remove_named_item<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
    qualified_name: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    let element = cx.attribute_map_element(this);
    let attr = get_attribute_node(cx, element, qualified_name)?
        .ok_or_else(|| ApiError::dom(ExceptionName::NotFound))?;
    remove_attribute_node(cx, element, attr)
}

pub(crate) fn remove_named_item_ns<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, NamedNodeMap>,
    namespace: Option<&str>,
    local_name: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    let element = cx.attribute_map_element(this);
    let attr = get_attribute_node_ns(cx, element, namespace, local_name)?
        .ok_or_else(|| ApiError::dom(ExceptionName::NotFound))?;
    remove_attribute_node(cx, element, attr)
}

// --- DOMParser ---------------------------------------------------------------

/// `new DOMParser()`: of the current realm's window.
pub(crate) fn construct_dom_parser<'gc, O: crate::typed::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, dom::DomParser>, ApiError> {
    let window = op.current_window(cx);
    Ok(cx.create_dom_parser(window))
}

/// `parseFromString(string, type)` (HTML's): `text/html` parsed into a new
/// HTML document, scripting disabled and declarative shadow roots left
/// templates, at the URL of its window's document. XML's
/// types (`text/xml`, `application/xml`, `application/xhtml+xml`,
/// `image/svg+xml`) are a `NotSupportedError` (no XML parser for it yet);
/// any other a `TypeError` (WebIDL's enum).
pub(crate) fn parse_from_string<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, dom::DomParser>,
    string: &str,
    mime_type: &str,
) -> Result<NodeRef<'gc>, ApiError> {
    match mime_type {
        "text/html" => {}
        "text/xml" | "application/xml" | "application/xhtml+xml" | "image/svg+xml" => {
            return Err(ApiError::Dom(crate::DomException {
                name: ExceptionName::NotSupported,
                message: format!(
                    "parseFromString: {mime_type} needs an XML parser, which there isn't"
                ),
            }));
        }
        _ => {
            return Err(ApiError::type_error(format!(
                "The provided value '{mime_type}' is not a valid enum value of type DOMParserSupportedType."
            )));
        }
    }
    let url = cx
        .dom_parser_window(this)
        .and_then(|window| cx.document_url(cx.window_document(window)));
    let document = html::parser::parse_document_safe_in(cx, string);
    cx.set_document_relevant_window(document, cx.dom_parser_window(this));
    if let Some(url) = url {
        cx.set_document_url(document, &url.to_str());
    }
    Ok(document)
}
