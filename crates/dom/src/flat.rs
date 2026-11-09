//! The flat tree (DOM's "flat tree", Blink's `FlatTreeTraversal`): what
//! style, layout and their dirty bits walk. A shadow host's children are
//! its shadow root's children; a slot's are its assigned nodes, or its own
//! children (its fallback content) if it has none; a host's child is
//! reached only through its slot (none: not in the flat tree). Worked out
//! as it is walked from the slots' assigned lists, not kept as a tree of
//! its own (Blink's too); the lists are made fresh before
//! style (`recalc_slot_assignments`), and a stale one is found by name.

use elise::{Gc, HeapVector};

use crate::shadow::{SHADOW_HOST, SHADOW_ROOT};
use crate::{Cx, Node, NodeRef};

/// A node's children in the flat tree, in order (`Cx::flat_tree_children`).
pub struct FlatChildren<'a, 'b, 'gc> {
    cx: &'a Cx<'b, 'gc>,
    at: FlatAt<'gc>,
}

enum FlatAt<'gc> {
    /// Siblings from this one on (a node's own children, a slot's
    /// fallback, a shadow root's children).
    Siblings(Option<NodeRef<'gc>>),
    /// A slot's assigned nodes from this index on.
    Assigned(Gc<'gc, HeapVector<Node>>, usize),
    /// A slot of a stale assignment: its host's children from this one on
    /// that find it.
    Found(NodeRef<'gc>, Option<NodeRef<'gc>>),
}

impl<'gc> Iterator for FlatChildren<'_, '_, 'gc> {
    type Item = NodeRef<'gc>;

    fn next(&mut self) -> Option<NodeRef<'gc>> {
        let cx = self.cx;
        match &mut self.at {
            FlatAt::Siblings(at) => {
                let node = (*at)?;
                *at = cx.next_sibling(node);
                Some(node)
            }
            FlatAt::Assigned(list, index) => {
                let node = list.get(cx.gc, *index)?;
                *index = index.saturating_add(1);
                Some(node)
            }
            FlatAt::Found(slot, at) => {
                while let Some(node) = *at {
                    *at = cx.next_sibling(node);
                    if cx
                        .assigned_slot(node)
                        .is_some_and(|of| Gc::ptr_eq(of, *slot))
                    {
                        return Some(node);
                    }
                }
                None
            }
        }
    }
}

impl<'b, 'gc> Cx<'b, 'gc> {
    /// `node`'s parent in the flat tree, for its dirty bits (Blink's
    /// `GetStyleRecalcParent`): a host's child's slot (none if it has
    /// none), a shadow root's child's host, a shadow root's host, else its
    /// parent. One read of the parent's `scope` for most.
    #[inline]
    pub fn flat_tree_parent(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        match self.parent(node) {
            Some(parent) if parent.scope.get() & (SHADOW_HOST | SHADOW_ROOT) == 0 => Some(parent),
            Some(parent) => self.flat_tree_parent_through(node, parent),
            None if node.scope.get() & SHADOW_ROOT != 0 => self.shadow_host(node),
            None => None,
        }
    }

    #[cold]
    #[inline(never)]
    fn flat_tree_parent_through(
        &self,
        node: NodeRef<'gc>,
        parent: NodeRef<'gc>,
    ) -> Option<NodeRef<'gc>> {
        if parent.scope.get() & SHADOW_HOST != 0 {
            return self.assigned_slot(node);
        }
        self.shadow_host(parent)
    }

    /// `node`'s children in the flat tree.
    pub fn flat_tree_children<'a>(&'a self, node: NodeRef<'gc>) -> FlatChildren<'a, 'b, 'gc> {
        let at = if node.scope.get() & SHADOW_HOST != 0 {
            let root = self.shadow_root(node).expect("a host's shadow root");
            FlatAt::Siblings(self.first_child(root))
        } else if self.is_slot(node) {
            self.slot_flat_children(node)
        } else {
            FlatAt::Siblings(self.first_child(node))
        };
        FlatChildren { cx: self, at }
    }

    /// A slot's: its assigned nodes as worked out, found by name if the
    /// assignment is stale; its own children if it has none.
    #[cold]
    fn slot_flat_children(&self, slot: NodeRef<'gc>) -> FlatAt<'gc> {
        let root = self
            .containing_shadow_root(slot)
            .expect("a slot is in a shadow tree");
        let host = self.shadow_host(root).expect("a shadow root's host");
        let stale = self
            .shadow_data(root)
            .is_some_and(|data| data.slot_state.get() & crate::slot::ASSIGNMENT_STALE != 0);
        if stale {
            let mut found = FlatChildren {
                cx: self,
                at: FlatAt::Found(slot, self.first_child(host)),
            };
            return match found.next() {
                Some(first) => FlatAt::Found(slot, Some(first)),
                None => FlatAt::Siblings(self.first_child(slot)),
            };
        }
        match self.assigned_vector(slot).filter(|list| !list.is_empty()) {
            Some(list) => FlatAt::Assigned(list, 0),
            None => FlatAt::Siblings(self.first_child(slot)),
        }
    }
}
