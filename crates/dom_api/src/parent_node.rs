//! The `ParentNode` mixin's members of the tree
//! (https://dom.spec.whatwg.org/#interface-parentnode), in `Document`,
//! `DocumentFragment` and `Element`.

use dom::{HtmlCollection, NodeRef};
use elise::Gc;

use crate::typed::NodeOrString;
use crate::{ApiCx, ApiError, is_element};

pub(crate) fn first_element_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let mut child = cx.first_child(this);
    while let Some(node) = child {
        if is_element(cx, node) {
            return Ok(Some(node));
        }
        child = cx.next_sibling(node);
    }
    Ok(None)
}

pub(crate) fn last_element_child<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let mut child = cx.last_child(this);
    while let Some(node) = child {
        if is_element(cx, node) {
            return Ok(Some(node));
        }
        child = cx.previous_sibling(node);
    }
    Ok(None)
}

/// `childElementCount`: at most `u32::MAX` (WebIDL's `unsigned long`).
pub(crate) fn child_element_count<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<u32, ApiError> {
    let mut count = 0u32;
    let mut child = cx.first_child(this);
    while let Some(node) = child {
        if is_element(cx, node) {
            count = count.saturating_add(1);
        }
        child = cx.next_sibling(node);
    }
    Ok(count)
}

/// `children`: its element children, live, the same each time.
pub(crate) fn children<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Gc<'gc, HtmlCollection>, ApiError> {
    Ok(cx.children_collection(this))
}

/// DOM's "convert nodes into a node" of `this`'s node document: a string
/// a new text; one node itself, several in a new fragment, in order.
pub(crate) fn convert<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: &[NodeOrString<'_, 'gc>],
) -> Result<NodeRef<'gc>, ApiError> {
    let document = cx.document_of(this);
    let node = |item: &NodeOrString<'_, 'gc>| match *item {
        NodeOrString::Node(node) => node,
        NodeOrString::String(text) => cx.create_text(document, text),
    };
    if let [only] = nodes {
        return Ok(node(only));
    }
    let fragment = cx.create_document_fragment(document);
    for item in nodes {
        cx.pre_insert(fragment, node(item), None)?;
    }
    Ok(fragment)
}

/// `append(...nodes)`: after its last child.
pub(crate) fn append<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeOrString<'_, 'gc>>,
) -> Result<(), ApiError> {
    let node = convert(cx, this, &nodes)?;
    cx.pre_insert(this, node, None)?;
    Ok(())
}

/// `prepend(...nodes)`: before its first child.
pub(crate) fn prepend<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeOrString<'_, 'gc>>,
) -> Result<(), ApiError> {
    let node = convert(cx, this, &nodes)?;
    cx.pre_insert(this, node, cx.first_child(this))?;
    Ok(())
}

/// `replaceChildren(...nodes)`: every child replaced by `nodes`, checked
/// before any goes.
pub(crate) fn replace_children<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeOrString<'_, 'gc>>,
) -> Result<(), ApiError> {
    let node = convert(cx, this, &nodes)?;
    cx.replace_children(this, node)?;
    Ok(())
}
