//! `Node` (https://dom.spec.whatwg.org/#interface-node; Blink's `Node`).

use dom::{DomString, NodeKind, NodeList, NodeRef, names};
use elise::Gc;

use crate::typed::DomStr;
use crate::{ApiCx, ApiError, is_element};

pub(crate) fn node_type<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(u32::from(cx.node_type(this)))
}

pub(crate) fn node_name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(cx.node_name_id(this)))
}

/// `isConnected`: whether its shadow-including root is a document: the
/// flag the tree's changes keep, through hosts' shadow trees.
pub(crate) fn is_connected<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<bool, ApiError> {
    Ok(cx.is_connected(this))
}

pub(crate) fn owner_document<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.owner_document(this))
}

pub(crate) fn parent_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.parent(this))
}

pub(crate) fn parent_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.parent(this).filter(|parent| is_element(cx, *parent)))
}

pub(crate) fn has_child_nodes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<bool, ApiError> {
    Ok(cx.first_child(this).is_some())
}

pub(crate) fn first_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.first_child(this))
}

pub(crate) fn last_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.last_child(this))
}

pub(crate) fn previous_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.previous_sibling(this))
}

pub(crate) fn next_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.next_sibling(this))
}

pub(crate) fn text_content<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomString>, ApiError> {
    Ok(cx.text_content(this))
}

/// `textContent`'s setter: null is the empty string.
pub(crate) fn set_text_content<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<&str>,
) -> Result<(), ApiError> {
    cx.set_text_content(this, value.unwrap_or(""));
    Ok(())
}

/// `contains`: whether `other` is an inclusive descendant (null: false).
pub(crate) fn contains<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    other: Option<NodeRef<'gc>>,
) -> Result<bool, ApiError> {
    let mut at = other;
    while let Some(node) = at {
        if Gc::ptr_eq(node, this) {
            return Ok(true);
        }
        at = cx.parent(node);
    }
    Ok(false)
}

pub(crate) fn insert_before<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    node: NodeRef<'gc>,
    child: Option<NodeRef<'gc>>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.pre_insert(this, node, child)?)
}

pub(crate) fn append_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    node: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.append_child(this, node)?)
}

pub(crate) fn remove_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    child: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.pre_remove(this, child)?)
}

/// `childNodes`: its children, live, the same list each time.
pub(crate) fn child_nodes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, NodeList>, ApiError> {
    Ok(cx.child_nodes(this))
}

/// `nodeValue`: character data's data (`data` has none for the others);
/// null for the others (no `Attr` nodes here).
pub(crate) fn node_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomString>, ApiError> {
    if cx.kind(this) == NodeKind::Attr {
        return Ok(Some(cx.attr_value_string(this)));
    }
    Ok(cx.data(this))
}

/// Setting `nodeValue` sets character data's data or an `Attr`'s value
/// (null is the empty string); the others ignore it.
pub(crate) fn set_node_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<&str>,
) -> Result<(), ApiError> {
    if matches!(
        cx.kind(this),
        NodeKind::Text | NodeKind::Comment | NodeKind::ProcessingInstruction
    ) {
        cx.set_data(this, value.unwrap_or(""));
    } else if cx.kind(this) == NodeKind::Attr {
        cx.set_attr_value(this, value.unwrap_or(""));
    }
    Ok(())
}

/// `replaceChild(node, child)`: `node` in `child`'s place; `child`.
pub(crate) fn replace_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    node: NodeRef<'gc>,
    child: NodeRef<'gc>,
) -> Result<NodeRef<'gc>, ApiError> {
    Ok(cx.replace_child(this, node, child)?)
}

pub(crate) fn normalize<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<(), ApiError> {
    cx.normalize(this);
    Ok(())
}

/// `isEqualNode(otherNode)`: null is no node equal.
pub(crate) fn is_equal_node<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    other: Option<NodeRef<'gc>>,
) -> Result<bool, ApiError> {
    Ok(other.is_some_and(|other| cx.is_equal_node(this, other)))
}

pub(crate) fn is_same_node<'gc>(
    _: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    other: Option<NodeRef<'gc>>,
) -> Result<bool, ApiError> {
    Ok(other.is_some_and(|other| Gc::ptr_eq(this, other)))
}

/// `compareDocumentPosition(other)`: WebIDL's `unsigned short`.
pub(crate) fn compare_document_position<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    other: NodeRef<'gc>,
) -> Result<u16, ApiError> {
    Ok(cx.compare_document_position(this, other))
}

/// The element a namespace lookup starts from: itself for an element, its
/// document element for a document, none for a doctype or a fragment, its
/// element for an `Attr`, its parent element for the others.
fn lookup_start<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    match cx.kind(node) {
        NodeKind::Element => Some(node),
        NodeKind::Document => {
            let mut child = cx.first_child(node);
            while let Some(at) = child {
                if is_element(cx, at) {
                    return Some(at);
                }
                child = cx.next_sibling(at);
            }
            None
        }
        NodeKind::DocumentType | NodeKind::DocumentFragment => None,
        // An `Attr`'s: its element's.
        NodeKind::Attr => cx.attr_owner(node),
        _ => cx.parent(node).filter(|parent| is_element(cx, *parent)),
    }
}

/// DOM's "locate a namespace" from `element` for `prefix` (none: the
/// default namespace): bound by an element's own name or an `xmlns`
/// declaration, its or an ancestor's.
fn locate_namespace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    prefix: Option<&str>,
) -> Option<DomString> {
    match prefix {
        Some("xml") => return Some(names::string(names::XML_NAMESPACE)),
        Some("xmlns") => return Some(names::string(names::XMLNS_NAMESPACE)),
        _ => {}
    }
    // A prefix no name has: none bound to it by a name.
    let prefix_id = match prefix {
        Some(prefix) => names::find(prefix),
        None => Some(names::NONE),
    };
    let mut at = Some(element);
    while let Some(element) = at {
        let name = cx.element_name(element).expect("an element's name");
        if name.namespace != names::NONE && Some(name.prefix) == prefix_id {
            return Some(names::string(name.namespace));
        }
        for index in 0..cx.attribute_count(element) {
            let attribute = cx
                .attribute_at(element, index)
                .expect("an attribute below the count");
            if attribute.name.namespace != names::XMLNS_NAMESPACE {
                continue;
            }
            let declares = match prefix {
                Some(_) => {
                    attribute.name.prefix == names::find("xmlns").unwrap_or(names::NONE)
                        && Some(attribute.name.local) == prefix_id
                }
                None => {
                    attribute.name.prefix == names::NONE
                        && names::get(attribute.name.local) == "xmlns"
                }
            };
            if declares {
                return (!attribute.value.to_str().is_empty()).then_some(attribute.value);
            }
        }
        at = cx.parent(element).filter(|parent| is_element(cx, *parent));
    }
    None
}

/// DOM's "locate a namespace prefix" from `element` for `namespace`: an
/// element's own prefix, or an `xmlns:` declaration's local name, its or
/// an ancestor's.
fn locate_prefix<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    namespace: &str,
) -> Option<DomString> {
    let mut at = Some(element);
    while let Some(element) = at {
        let name = cx.element_name(element).expect("an element's name");
        if name.prefix != names::NONE && names::get(name.namespace) == namespace {
            return Some(names::string(name.prefix));
        }
        for index in 0..cx.attribute_count(element) {
            let attribute = cx
                .attribute_at(element, index)
                .expect("an attribute below the count");
            if attribute.name.namespace == names::XMLNS_NAMESPACE
                && names::get(attribute.name.prefix) == "xmlns"
                && *attribute.value.to_str() == *namespace
            {
                return Some(names::string(attribute.name.local));
            }
        }
        at = cx.parent(element).filter(|parent| is_element(cx, *parent));
    }
    None
}

/// `lookupPrefix(namespace)`: null for a null or empty namespace.
pub(crate) fn lookup_prefix<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
) -> Result<Option<DomString>, ApiError> {
    let Some(namespace) = namespace.filter(|namespace| !namespace.is_empty()) else {
        return Ok(None);
    };
    Ok(lookup_start(cx, this).and_then(|element| locate_prefix(cx, element, namespace)))
}

/// `lookupNamespaceURI(prefix)`: the empty prefix is none.
pub(crate) fn lookup_namespace_uri<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    prefix: Option<&str>,
) -> Result<Option<DomString>, ApiError> {
    let prefix = prefix.filter(|prefix| !prefix.is_empty());
    Ok(lookup_start(cx, this).and_then(|element| locate_namespace(cx, element, prefix)))
}

/// `isDefaultNamespace(namespace)`: whether `namespace` (the empty one is
/// null) is the one of no prefix here.
pub(crate) fn is_default_namespace<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    namespace: Option<&str>,
) -> Result<bool, ApiError> {
    let namespace = namespace.filter(|namespace| !namespace.is_empty());
    let default = lookup_start(cx, this).and_then(|element| locate_namespace(cx, element, None));
    Ok(default
        .as_ref()
        .map(|default| default.to_str().into_owned())
        .as_deref()
        == namespace)
}

/// `baseURI`: its document's base URL, or `about:blank` for one with
/// none.
pub(crate) fn base_uri<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<crate::typed::DomStr<'gc>, ApiError> {
    let document = if cx.kind(this) == dom::NodeKind::Document {
        this
    } else {
        cx.document_of(this)
    };
    Ok(crate::typed::DomStr::fresh(
        cx.document_base_url(document)
            .as_ref()
            .map_or_else(|| "about:blank".to_owned(), |url| url.as_str().to_owned()),
    ))
}
