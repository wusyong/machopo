//! The ancestor bloom filter (`selectors`' `BloomFilter`, Blink's
//! `SelectorFilter`): the names of the elements above the one being
//! styled, so that a selector whose ancestors' names aren't all among them
//! is passed over without matching. The walk pushes an element as it goes
//! down into its children and pops it as it comes back up.

use css::selector::name_hash;
use dom::{Cx, NodeRef, names};
use selectors::bloom::BloomFilter;

pub(crate) struct StyleBloom {
    filter: Box<BloomFilter>,
    /// Where each pushed element's hashes start in `hashes`, from the root
    /// down.
    pushed: Vec<usize>,
    hashes: Vec<u32>,
}

impl StyleBloom {
    pub(crate) fn new() -> Self {
        Self {
            filter: Box::new(BloomFilter::new()),
            pushed: Vec::new(),
            hashes: Vec::new(),
        }
    }

    pub(crate) fn filter(&self) -> &BloomFilter {
        &self.filter
    }

    /// `element`'s names in: the walk goes into its children.
    pub(crate) fn push<'gc>(&mut self, cx: &Cx<'_, 'gc>, element: NodeRef<'gc>) {
        self.pushed.push(self.hashes.len());
        each_hash(cx, element, |hash| {
            self.filter.insert_hash(hash);
            self.hashes.push(hash);
        });
    }

    /// The last pushed element's names out: the walk is back from its
    /// children.
    pub(crate) fn pop(&mut self) {
        let start = self.pushed.pop().expect("something pushed");
        for hash in self.hashes.drain(start..) {
            self.filter.remove_hash(hash);
        }
    }
}

/// Each name of `element` a selector's ancestor may name (its local name,
/// its namespace, its id, its classes), by its `name_hash`, as `css`
/// hashes a selector's. The namespace too: the UA stylesheet's every
/// compound names HTML's (its default namespace), which `selectors` puts
/// in the ancestors' hashes.
pub(crate) fn each_hash<'gc>(cx: &Cx<'_, 'gc>, element: NodeRef<'gc>, mut f: impl FnMut(u32)) {
    if let Some(name) = cx.qualified_name(element) {
        f(name_hash(&names::get(name.local())));
        f(name_hash(&names::get(name.namespace())));
    }
    if let Some(id) = cx.attribute_value_by_id(element, names::ID) {
        f(name_hash(&id.borrow().to_str()));
    }
    for class in cx.class_names(element) {
        f(class.hash);
    }
}
