//! A node's subtree's bloom filter (Blink's
//! `Element::attribute_or_class_bloom_`): two bits of 32 for each
//! attribute's name and each class of the node and its descendants, added
//! as they come (an attribute set, a node inserted) and never taken off,
//! so that a node's bits hold its descendants'. A bit too many only leaves
//! a selector query less to skip; one missing would skip a match. The
//! `style` attribute, written back lazily from its declarations, may not
//! be in it yet: a query doesn't look for it (Blink's bloom filter leaves
//! out the attributes it synchronizes lazily too).
//! `style`'s queries skip a subtree whose bits miss what a selector needs.

use std::hash::BuildHasher;

use crate::{Cx, NameId, NodeRef, QualifiedName, names};

/// A hash's two bits (Blink's `FilterForHash`, k = 2): from its top ten
/// bits, two five-bit halves.
fn bits(hash: u32) -> u32 {
    1u32.wrapping_shl(hash.wrapping_shr(27)) | 1u32.wrapping_shl(hash.wrapping_shr(22) & 31)
}

/// An attribute name's bits, by its local name's number (the numbers are
/// dense: spread by a Fibonacci hash).
pub fn attribute_bloom(local: NameId) -> u32 {
    bits(local.wrapping_mul(0x9E37_79B9))
}

/// A class's bits, by its text: an element's classes needn't be numbered
/// for it as they are set.
pub fn class_bloom(class: &str) -> u32 {
    let hash = rustc_hash::FxBuildHasher.hash_one(class);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the hash's halves folded into 32 bits"
    )]
    let folded = (hash ^ hash.wrapping_shr(32)) as u32;
    bits(folded)
}

impl<'gc> Cx<'_, 'gc> {
    /// The bits of `node`'s and its descendants' attribute names and
    /// classes (`attribute_bloom`, `class_bloom`), and maybe more.
    pub fn subtree_bloom(&self, node: NodeRef<'gc>) -> u32 {
        node.bloom.get()
    }

    /// `bits` added to `node`'s and its ancestors', up to the first that
    /// has them all (whose ancestors have them too).
    pub(crate) fn add_to_bloom(&self, node: NodeRef<'gc>, bits: u32) {
        let mut at = Some(node);
        while let Some(node) = at {
            let had = node.bloom.get();
            if had & bits == bits {
                return;
            }
            node.bloom.set(had | bits);
            at = node.parent.get(self.gc);
        }
    }

    /// `element`'s attribute `name` set to `value`: its name's bits, and a
    /// class attribute's classes'.
    pub(crate) fn attribute_bloomed(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        value: &str,
    ) {
        let mut bits = attribute_bloom(name.local());
        if name == QualifiedName::plain(names::CLASS) {
            for class in value.split_ascii_whitespace() {
                bits |= class_bloom(class);
            }
        }
        self.add_to_bloom(element, bits);
    }
}
