//! DOM's algorithms over trees besides inserting and removing
//! (https://dom.spec.whatwg.org/#interface-node; Blink's `Node`):
//! `normalize`, `isEqualNode` and `compareDocumentPosition`.

use elise::Gc;

use crate::{Cx, NodeKind, NodeRef};

/// `compareDocumentPosition`'s bits.
pub const DOCUMENT_POSITION_DISCONNECTED: u16 = 0x01;
pub const DOCUMENT_POSITION_PRECEDING: u16 = 0x02;
pub const DOCUMENT_POSITION_FOLLOWING: u16 = 0x04;
pub const DOCUMENT_POSITION_CONTAINS: u16 = 0x08;
pub const DOCUMENT_POSITION_CONTAINED_BY: u16 = 0x10;
pub const DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC: u16 = 0x20;

impl<'gc> Cx<'_, 'gc> {
    /// `normalize()`: in `node`'s descendants, an empty text goes, and a
    /// run of adjacent texts becomes its first with their data appended (a
    /// character data record for it, a tree record for each that goes);
    /// the live ranges in the run move into the first.
    pub fn normalize(&self, node: NodeRef<'gc>) {
        let mut texts = Vec::new();
        self.for_each_inclusive_descendant(node, |at| {
            if !Gc::ptr_eq(at, node) && self.is_exclusive_text(at) {
                texts.push(at);
            }
        });
        for text in texts {
            // Joined into one before it.
            if self.parent(text).is_none() {
                continue;
            }
            let own = self.data(text).expect("a text's data");
            if own.to_str().is_empty() {
                self.remove(text);
                continue;
            }
            let mut run = Vec::new();
            let mut rest = String::new();
            let mut at = self.next_sibling(text);
            while let Some(next) = at.filter(|next| self.is_exclusive_text(*next)) {
                rest.push_str(&self.data(next).expect("a text's data").to_str());
                run.push(next);
                at = self.next_sibling(next);
            }
            if run.is_empty() {
                continue;
            }
            let length = own.utf16_len();
            self.replace_data(text, length, 0, &rest);
            if self.has_seen_live() {
                let mut length = u32::try_from(length).unwrap_or(u32::MAX);
                for &next in &run {
                    self.live_ranges_merged(text, next, length);
                    length = length.saturating_add(self.node_length(next));
                }
            }
            for next in run {
                self.remove(next);
            }
        }
    }

    /// DOM's "equals" (`isEqualNode`): the same kind and what it has of
    /// its own (an element's name and its attributes, any order, by
    /// namespace, local name and value; a doctype's ids; character data's
    /// data), and equal children in order.
    pub fn is_equal_node(&self, a: NodeRef<'gc>, b: NodeRef<'gc>) -> bool {
        if self.kind(a) != self.kind(b) {
            return false;
        }
        let same = match self.kind(a) {
            NodeKind::DocumentType => self.doctype(a) == self.doctype(b),
            NodeKind::Element => {
                let count = self.attribute_count(a);
                self.element_name(a) == self.element_name(b)
                    && count == self.attribute_count(b)
                    && (0..count).all(|index| {
                        let ours = self
                            .attribute_at(a, index)
                            .expect("an attribute below the count");
                        (0..count).any(|other| {
                            self.attribute_at(b, other).is_some_and(|theirs| {
                                theirs.name.namespace == ours.name.namespace
                                    && theirs.name.local == ours.name.local
                                    && theirs.value == ours.value
                            })
                        })
                    })
            }
            NodeKind::ProcessingInstruction => {
                self.target(a) == self.target(b) && self.data(a) == self.data(b)
            }
            NodeKind::Text | NodeKind::Comment => {
                self.node_type(a) == self.node_type(b) && self.data(a) == self.data(b)
            }
            NodeKind::Attr => {
                let (ours, theirs) = (self.attr_name(a), self.attr_name(b));
                ours.namespace == theirs.namespace
                    && ours.local == theirs.local
                    && self.attr_value_string(a) == self.attr_value_string(b)
            }
            NodeKind::Document | NodeKind::DocumentFragment => true,
        };
        if !same {
            return false;
        }
        let (mut x, mut y) = (self.first_child(a), self.first_child(b));
        loop {
            match (x, y) {
                (None, None) => return true,
                (Some(p), Some(q)) if self.is_equal_node(p, q) => {
                    x = self.next_sibling(p);
                    y = self.next_sibling(q);
                }
                _ => return false,
            }
        }
    }

    /// `compareDocumentPosition(other)` of `node`: where `other` is from it
    /// (no attributes here: the DOM has no `Attr` nodes). Nodes of
    /// different trees are disconnected, in the order of their ids (the
    /// spec leaves it to the implementation, but it must be the same each
    /// time; an id is given once and never changes).
    pub fn compare_document_position(&self, node: NodeRef<'gc>, other: NodeRef<'gc>) -> u16 {
        if Gc::ptr_eq(node, other) {
            return 0;
        }
        // Each one's inclusive ancestors, the root first.
        let chain = |at: NodeRef<'gc>| {
            let mut chain = vec![at];
            let mut up = self.parent(at);
            while let Some(parent) = up {
                chain.push(parent);
                up = self.parent(parent);
            }
            chain.reverse();
            chain
        };
        let (theirs, ours) = (chain(other), chain(node));
        if !Gc::ptr_eq(theirs[0], ours[0]) {
            let order = if self.node_id(other) < self.node_id(node) {
                DOCUMENT_POSITION_PRECEDING
            } else {
                DOCUMENT_POSITION_FOLLOWING
            };
            return DOCUMENT_POSITION_DISCONNECTED
                | DOCUMENT_POSITION_IMPLEMENTATION_SPECIFIC
                | order;
        }
        let common = theirs
            .iter()
            .zip(&ours)
            .take_while(|(a, b)| Gc::ptr_eq(**a, **b))
            .count();
        if common == theirs.len() {
            return DOCUMENT_POSITION_CONTAINS | DOCUMENT_POSITION_PRECEDING;
        }
        if common == ours.len() {
            return DOCUMENT_POSITION_CONTAINED_BY | DOCUMENT_POSITION_FOLLOWING;
        }
        // Their ancestors among the common one's children: which is first.
        let (their_child, our_child) = (theirs[common], ours[common]);
        let mut at = self.next_sibling(their_child);
        while let Some(sibling) = at {
            if Gc::ptr_eq(sibling, our_child) {
                return DOCUMENT_POSITION_PRECEDING;
            }
            at = self.next_sibling(sibling);
        }
        DOCUMENT_POSITION_FOLLOWING
    }
}
