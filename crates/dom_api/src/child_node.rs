//! The `ChildNode` mixin (https://dom.spec.whatwg.org/#interface-childnode)
//! of doctypes, elements and character data: `before`, `after`,
//! `replaceWith`, `remove`; and `NonDocumentTypeChildNode`'s element
//! siblings.

use dom::NodeRef;
use elise::Gc;

use crate::parent_node::convert;
use crate::typed::NodeOrString;
use crate::{ApiCx, ApiError, is_element};

/// Whether `node` is one of `nodes`' nodes.
fn listed<'gc>(nodes: &[NodeOrString<'_, 'gc>], node: NodeRef<'gc>) -> bool {
    nodes
        .iter()
        .any(|item| matches!(item, NodeOrString::Node(at) if Gc::ptr_eq(*at, node)))
}

/// DOM's "viable previous sibling" (`previous`) or "viable next sibling"
/// of `this`: the first sibling that way not among `nodes`.
fn viable_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: &[NodeOrString<'_, 'gc>],
    previous: bool,
) -> Option<NodeRef<'gc>> {
    let step = |at| {
        if previous {
            cx.previous_sibling(at)
        } else {
            cx.next_sibling(at)
        }
    };
    let mut at = step(this);
    while let Some(node) = at {
        if !listed(nodes, node) {
            return Some(node);
        }
        at = step(node);
    }
    None
}

/// `before(...nodes)`: in its parent, before it (and before those of
/// `nodes` right before it); nothing without a parent.
pub(crate) fn before<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeOrString<'_, 'gc>>,
) -> Result<(), ApiError> {
    let Some(parent) = cx.parent(this) else {
        return Ok(());
    };
    let viable = viable_sibling(cx, this, &nodes, true);
    let node = convert(cx, this, &nodes)?;
    let reference = match viable {
        None => cx.first_child(parent),
        Some(viable) => cx.next_sibling(viable),
    };
    cx.pre_insert(parent, node, reference)?;
    Ok(())
}

/// `after(...nodes)`: in its parent, after it (and after those of `nodes`
/// right after it).
pub(crate) fn after<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeOrString<'_, 'gc>>,
) -> Result<(), ApiError> {
    let Some(parent) = cx.parent(this) else {
        return Ok(());
    };
    let viable = viable_sibling(cx, this, &nodes, false);
    let node = convert(cx, this, &nodes)?;
    cx.pre_insert(parent, node, viable)?;
    Ok(())
}

/// `replaceWith(...nodes)`: `nodes` in its place (where it would have
/// been, if converting them took it out of its parent).
pub(crate) fn replace_with<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeOrString<'_, 'gc>>,
) -> Result<(), ApiError> {
    let Some(parent) = cx.parent(this) else {
        return Ok(());
    };
    let viable = viable_sibling(cx, this, &nodes, false);
    let node = convert(cx, this, &nodes)?;
    if cx.parent(this).is_some_and(|of| Gc::ptr_eq(of, parent)) {
        cx.replace_child(parent, node, this)?;
    } else {
        cx.pre_insert(parent, node, viable)?;
    }
    Ok(())
}

/// `remove()`: out of its parent, if it has one.
pub(crate) fn remove<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<(), ApiError> {
    cx.remove(this);
    Ok(())
}

pub(crate) fn previous_element_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let mut at = cx.previous_sibling(this);
    while let Some(node) = at {
        if is_element(cx, node) {
            return Ok(Some(node));
        }
        at = cx.previous_sibling(node);
    }
    Ok(None)
}

pub(crate) fn next_element_sibling<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    let mut at = cx.next_sibling(this);
    while let Some(node) = at {
        if is_element(cx, node) {
            return Ok(Some(node));
        }
        at = cx.next_sibling(node);
    }
    Ok(None)
}
