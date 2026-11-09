//! What a document of the DOM draws as selected (Blink's
//! `LayoutSelection`, which `FrameSelection` commits before paint): its
//! `Selection`'s range's text, by text node and part of the data
//! (`paint::TextSelection`), and the `<br>`s the range has, in flat tree
//! order (shadow trees' text too). A text the range goes on past, to
//! something laid out, is selected to `u32::MAX` (the end of a line it
//! ends draws as selected); paint clips that to each run. A focused text
//! control draws its own selection instead (`FrameView`). Where the
//! selection is collapsed in the editing host that has the focus, its
//! caret (`caret`).

use std::collections::HashMap;

use dom::{Cx, FlatChildren, LocalToken, NodeId, NodeKind, NodeRef, names};
use paint::TextSelection;

/// Adds what `document`'s selection draws as selected to `out`, in the
/// active colors if `active`; `is_rendered` says which nodes have a box.
pub(crate) fn add_document<'gc>(
    cx: &Cx<'_, 'gc>,
    document: NodeRef<'gc>,
    active: bool,
    is_rendered: &dyn Fn(NodeId) -> bool,
    out: &mut HashMap<NodeId, TextSelection>,
) {
    let Some(range) = cx.document_selection_range(document) else {
        return;
    };
    let ((start, start_offset), (end, end_offset)) = (cx.range_start(range), cx.range_end(range));
    if NodeRef::ptr_eq(start, end) && start_offset == end_offset {
        return;
    }
    let mut selected = Vec::new();
    for_each_selected(
        cx,
        (start, start_offset),
        (end, end_offset),
        |node, range| {
            selected.push((node, range));
        },
    );
    // The last node laid out is where the selection ends (Blink's `kEnd`
    // state): a text before it goes on past its data, it doesn't.
    if let Some((node, range)) = selected
        .iter_mut()
        .rev()
        .find(|(node, _)| is_rendered(cx.node_id(*node)))
        && range.end == u32::MAX
    {
        range.end = cx.node_length(*node);
    }
    out.extend(selected.into_iter().filter_map(|(node, range)| {
        let selected = cx.kind(node) == NodeKind::Text || is_br(cx, node);
        selected.then(|| (cx.node_id(node), TextSelection { range, active }))
    }));
}

fn is_br<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    cx.element_name(node).is_some_and(|name| {
        name.namespace == names::HTML_NAMESPACE && name.local == names::local::BR
    })
}

/// Each node from boundary point `start` to `end` in flat tree order (as
/// Blink's `LayoutSelection` goes through the layout tree: a host's shadow
/// tree is in it, its children where slots put them), with the part of a
/// text's data in the range: from the start's offset in the start's node
/// (else 0) to the end's in the end's (else `u32::MAX`, past the data);
/// 0..1 for another.
fn for_each_selected<'gc>(
    cx: &Cx<'_, 'gc>,
    (start, start_offset): (NodeRef<'gc>, u32),
    (end, end_offset): (NodeRef<'gc>, u32),
    mut each: impl FnMut(NodeRef<'gc>, std::ops::Range<u32>),
) {
    let is_data = |node: NodeRef<'gc>| cx.kind(node).is_character_data();
    let child_at = |node: NodeRef<'gc>, offset: u32| {
        let offset = usize::try_from(offset).expect("a `u32` fits a `usize`");
        std::iter::successors(cx.first_child(node), |child| cx.next_sibling(*child)).nth(offset)
    };
    // The first node at or after the start, and the node the end is just
    // before (none when the end is in character data, which is the last).
    let first = if is_data(start) {
        Some(start)
    } else {
        child_at(start, start_offset).or_else(|| after_subtree(cx, start))
    };
    let stop = if is_data(end) {
        None
    } else {
        child_at(end, end_offset).or_else(|| after_subtree(cx, end))
    };
    let Some(first) = first else {
        return;
    };
    flat_tree_from(cx, first, |node| {
        if stop.is_some_and(|stop| NodeRef::ptr_eq(stop, node)) {
            return false;
        }
        let from = if NodeRef::ptr_eq(node, start) {
            start_offset
        } else {
            0
        };
        if NodeRef::ptr_eq(node, end) && is_data(end) {
            each(node, from..end_offset);
            return false;
        }
        if cx.kind(node) == NodeKind::Text {
            each(node, from..u32::MAX);
        } else {
            each(node, 0..1);
        }
        true
    });
}

/// Each node in flat tree order from `first` on (its subtree, then what
/// comes after it), until `visit` says to stop.
fn flat_tree_from<'a, 'b, 'gc>(
    cx: &'a Cx<'b, 'gc>,
    first: NodeRef<'gc>,
    mut visit: impl FnMut(NodeRef<'gc>) -> bool,
) {
    // The children left to go through of each of its flat tree ancestors,
    // outermost first, then of each node gone into.
    let mut ancestors = Vec::new();
    let mut child = first;
    while let Some(parent) = cx.flat_tree_parent(child) {
        ancestors.push((parent, child));
        child = parent;
    }
    let mut rest: Vec<FlatChildren<'a, 'b, 'gc>> = ancestors
        .into_iter()
        .rev()
        .map(|(parent, child)| {
            let mut children = cx.flat_tree_children(parent);
            for sibling in children.by_ref() {
                if NodeRef::ptr_eq(sibling, child) {
                    break;
                }
            }
            children
        })
        .collect();
    let mut next = Some(first);
    while let Some(node) = next {
        if !visit(node) {
            return;
        }
        rest.push(cx.flat_tree_children(node));
        next = loop {
            let Some(children) = rest.last_mut() else {
                return;
            };
            if let Some(child) = children.next() {
                break Some(child);
            }
            rest.pop();
        };
    }
}

/// The node after `node`'s subtree in tree order.
fn after_subtree<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
    let mut at = Some(node);
    while let Some(node) = at {
        if let Some(next) = cx.next_sibling(node) {
            return Some(next);
        }
        at = cx.parent(node);
    }
    None
}

/// Where `document`'s caret is (Blink's `FrameCaret`), if it shows one:
/// the collapsed selection's, in the editable content of its focused
/// element (an editing host or in one), where the user could type;
/// `box_tree` says which box it is in, and `text` the font of an empty
/// element's.
pub(crate) fn caret<'gc>(
    cx: &Cx<'_, 'gc>,
    token: &LocalToken,
    document: NodeRef<'gc>,
    box_tree: &layout::BoxTree,
    text: &mut layout::TextContext,
) -> Option<paint::Caret> {
    let focused = cx.focused_element(document)?;
    let range = cx.document_selection_range(document)?;
    let ((container, offset), (end, end_offset)) = (cx.range_start(range), cx.range_end(range));
    if !NodeRef::ptr_eq(container, end) || offset != end_offset {
        return None;
    }
    let is_text = cx.kind(container) == NodeKind::Text;
    let element = if is_text {
        cx.parent(container)?
    } else {
        container
    };
    let inside = std::iter::successors(Some(element), |n| cx.parent(*n))
        .any(|n| NodeRef::ptr_eq(n, focused));
    if !inside || !cx.is_editable(element) {
        return None;
    }
    if is_text {
        return in_text(cx, token, container, offset, box_tree, text);
    }
    // Between children: the text after it, or else before it, or a
    // `<br>` it is just before, or one it is just after at the end (which
    // ends no line of its own: its line's); in an element with nothing,
    // where its line would be.
    let child_at = |index: u32| {
        let index = usize::try_from(index).ok()?;
        std::iter::successors(cx.first_child(container), |child| cx.next_sibling(*child)).nth(index)
    };
    let after = child_at(offset);
    let before = offset.checked_sub(1).and_then(child_at);
    if let Some(node) = after.filter(|n| cx.kind(*n) == NodeKind::Text) {
        return in_text(cx, token, node, 0, box_tree, text);
    }
    if let Some(node) = before.filter(|n| cx.kind(*n) == NodeKind::Text) {
        return in_text(cx, token, node, cx.node_length(node), box_tree, text);
    }
    if let Some(br) = after
        .filter(|n| is_br(cx, *n))
        .or(before.filter(|n| after.is_none() && is_br(cx, *n)))
    {
        let id = cx.node_id(br);
        return Some(paint::Caret {
            at: paint::CaretPosition::Break(id),
            block: box_tree.block_of(id)?,
        });
    }
    if cx.first_child(container).is_none() {
        return empty(cx, token, container, text);
    }
    None
}

/// The caret at `offset` in text node `node`; with no text laid out (an
/// empty one), where its parent's empty line would be.
fn in_text<'gc>(
    cx: &Cx<'_, 'gc>,
    token: &LocalToken,
    node: NodeRef<'gc>,
    offset: u32,
    box_tree: &layout::BoxTree,
    text: &mut layout::TextContext,
) -> Option<paint::Caret> {
    let id = cx.node_id(node);
    match box_tree.block_of(id) {
        Some(block) => Some(paint::Caret {
            at: paint::CaretPosition::Text { text: id, offset },
            block,
        }),
        None if cx.node_length(node) == 0 => empty(cx, token, cx.parent(node)?, text),
        None => None,
    }
}

/// The caret in empty editable element `element`, in its first font.
fn empty<'gc>(
    cx: &Cx<'_, 'gc>,
    token: &LocalToken,
    element: NodeRef<'gc>,
    text: &mut layout::TextContext,
) -> Option<paint::Caret> {
    let style = cx.computed_style(element, token)?;
    let (ascent, descent, line_height) = layout::empty_caret_metrics(text, style);
    let id = cx.node_id(element);
    Some(paint::Caret {
        at: paint::CaretPosition::Empty {
            element: id,
            ascent,
            descent,
            line_height,
        },
        block: id,
    })
}
