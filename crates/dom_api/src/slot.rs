//! Slots' members (https://html.spec.whatwg.org/#the-slot-element):
//! `HTMLSlotElement`'s, and `Element`'s and `Text`'s `assignedSlot` and
//! `Element`'s `slot` (Blink's `HTMLSlotElement`, `Slottable`). The
//! assignment is `dom`'s (`assigned_nodes`, `assign`).

use dom::{NodeKind, NodeRef, QualName, ShadowRootMode, names};

use crate::element::reflected;
use crate::interfaces::AssignedNodesOptions;
use crate::typed::DomStr;
use crate::{ApiCx, ApiError};

/// `slot`: the `slot` attribute reflected.
pub(crate) fn slot<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<DomStr<'gc>, ApiError> {
    Ok(reflected(cx, this, names::local::SLOT))
}

pub(crate) fn set_slot<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_attribute_ns(this, QualName::local("slot"), value);
    Ok(())
}

/// `assignedSlot`: DOM's "find a slot" with the open flag, its slot only
/// in an open shadow tree.
pub(crate) fn assigned_slot<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(cx.assigned_slot(this).filter(|&slot| {
        cx.containing_shadow_root(slot)
            .and_then(|root| cx.shadow_root_init(root))
            .is_some_and(|init| init.mode == ShadowRootMode::Open)
    }))
}

fn nodes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &AssignedNodesOptions,
) -> Vec<NodeRef<'gc>> {
    if options.flatten {
        cx.flattened_assigned_nodes(this)
    } else {
        cx.assigned_nodes(this)
    }
}

/// `assignedNodes(options)`: its assigned nodes, or with `flatten` DOM's
/// flattened ones (its fallback content's if none, through the slots).
pub(crate) fn assigned_nodes<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &AssignedNodesOptions,
) -> Result<Vec<NodeRef<'gc>>, ApiError> {
    Ok(nodes(cx, this, options))
}

/// `assignedElements(options)`: `assignedNodes`'s elements.
pub(crate) fn assigned_elements<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    options: &AssignedNodesOptions,
) -> Result<Vec<NodeRef<'gc>>, ApiError> {
    let mut nodes = nodes(cx, this, options);
    nodes.retain(|&node| cx.kind(node) == NodeKind::Element);
    Ok(nodes)
}

/// `assign(...nodes)`: its manually assigned nodes.
pub(crate) fn assign<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    nodes: Vec<NodeRef<'gc>>,
) -> Result<(), ApiError> {
    cx.assign(this, &nodes);
    Ok(())
}
