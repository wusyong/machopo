//! DOM's traversal (https://dom.spec.whatwg.org/#traversal; Blink's
//! `NodeIteratorBase`, `NodeIterator`, `TreeWalker`): what each keeps, a
//! root, `whatToShow`, its filter (a JS edge the engine reads; whether it
//! has one, for the DOM) and the "active" flag that stops the filter from
//! using it; a `NodeIterator` also its reference and "pointer before
//! reference", and while its traversal's filter runs the position being
//! filtered; a `TreeWalker` its current node. The walking, and calling the
//! filter between operations, is `dom_api`'s.
//!
//! A document keeps its node iterators weakly (`DocumentData::
//! node_iterators`, Blink's `Document::node_iterators_`): removing a node
//! runs the pre-removing steps of each one whose root is in the node's
//! document, so that its positions stay in the tree. Adopting an
//! iterator's root moves the iterator to the new document's list (Blink's
//! `Document::MoveNodeIteratorsToNewDocument`).

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member, Mutation, Persistent, WeakMember};

use crate::{Cx, Node, NodeRef, sites};

/// `NodeIterator::flags`' and `TreeWalker::flags`'.
const ACTIVE: u8 = 1 << 0;
const HAS_FILTER: u8 = 1 << 1;
const POINTER_BEFORE_REFERENCE: u8 = 1 << 2;
const POINTER_BEFORE_CANDIDATE: u8 = 1 << 3;

/// `createNodeIterator(root, whatToShow, filter)`.
#[derive(GcObject)]
pub struct NodeIterator {
    root: Member<Node>,
    reference: Member<Node>,
    /// The position its traversal is filtering, while the filter runs:
    /// the pre-removing steps move it as they move the reference.
    candidate: Member<Node>,
    /// `ACTIVE`, `HAS_FILTER`, `POINTER_BEFORE_REFERENCE`,
    /// `POINTER_BEFORE_CANDIDATE`.
    flags: Cell<u8>,
    what_to_show: Cell<u32>,
    filter: JsEdge,
    wrapper: JsEdge,
}

/// A node iterator held weakly, on its root's document's list.
#[derive(GcObject)]
pub struct NodeIteratorRef {
    iterator: WeakMember<NodeIterator>,
}

/// `createTreeWalker(root, whatToShow, filter)`.
#[derive(GcObject)]
pub struct TreeWalker {
    root: Member<Node>,
    current: Member<Node>,
    /// `ACTIVE`, `HAS_FILTER`.
    flags: Cell<u8>,
    what_to_show: Cell<u32>,
    filter: JsEdge,
    wrapper: JsEdge,
}

/// A `NodeIterator` or a `TreeWalker`: what both have.
#[derive(Clone, Copy)]
pub enum Traversal<'gc> {
    NodeIterator(Gc<'gc, NodeIterator>),
    TreeWalker(Gc<'gc, TreeWalker>),
}

/// A `Traversal` kept past an operation (a member that calls the filter
/// between its operations).
pub enum TraversalRoot {
    NodeIterator(Persistent<NodeIterator>),
    TreeWalker(Persistent<TreeWalker>),
}

impl TraversalRoot {
    pub fn get<'gc>(&self, cx: &Mutation<'gc>) -> Traversal<'gc> {
        match self {
            Self::NodeIterator(root) => Traversal::NodeIterator(root.get(cx)),
            Self::TreeWalker(root) => Traversal::TreeWalker(root.get(cx)),
        }
    }
}

impl<'gc> Traversal<'gc> {
    fn flags(self) -> &'gc Cell<u8> {
        match self {
            Self::NodeIterator(iterator) => &iterator.into_ref().flags,
            Self::TreeWalker(walker) => &walker.into_ref().flags,
        }
    }
}

fn flag(flags: &Cell<u8>, flag: u8, on: bool) {
    let bits = flags.get();
    flags.set(if on { bits | flag } else { bits & !flag });
}

impl<'gc> Cx<'_, 'gc> {
    /// A new node iterator over `root`'s subtree, its reference `root` and
    /// its pointer before it, on the list of `root`'s document. Its filter,
    /// if `has_filter`, is the engine's to set (`traversal_filter_edge`).
    pub fn create_node_iterator(
        &self,
        root: NodeRef<'gc>,
        what_to_show: u32,
        has_filter: bool,
    ) -> Gc<'gc, NodeIterator> {
        let filter_bit = if has_filter { HAS_FILTER } else { 0 };
        let iterator = self.gc.alloc_in(
            &sites::OTHER,
            NodeIteratorInit {
                root: Some(root),
                reference: Some(root),
                candidate: None,
                flags: Cell::new(POINTER_BEFORE_REFERENCE | filter_bit),
                what_to_show: Cell::new(what_to_show),
                filter: (),
                wrapper: (),
            },
        );
        self.add_node_iterator(self.document_of(root), iterator);
        iterator
    }

    /// A new tree walker over `root`'s subtree, its current node `root`.
    pub fn create_tree_walker(
        &self,
        root: NodeRef<'gc>,
        what_to_show: u32,
        has_filter: bool,
    ) -> Gc<'gc, TreeWalker> {
        let filter_bit = if has_filter { HAS_FILTER } else { 0 };
        self.gc.alloc_in(
            &sites::OTHER,
            TreeWalkerInit {
                root: Some(root),
                current: Some(root),
                flags: Cell::new(filter_bit),
                what_to_show: Cell::new(what_to_show),
                filter: (),
                wrapper: (),
            },
        )
    }

    pub fn traversal_root(&self, traversal: Traversal<'gc>) -> NodeRef<'gc> {
        match traversal {
            Traversal::NodeIterator(iterator) => iterator.root.get(self.gc),
            Traversal::TreeWalker(walker) => walker.root.get(self.gc),
        }
        .expect("a traversal's root")
    }

    pub fn traversal_what_to_show(&self, traversal: Traversal<'gc>) -> u32 {
        match traversal {
            Traversal::NodeIterator(iterator) => iterator.what_to_show.get(),
            Traversal::TreeWalker(walker) => walker.what_to_show.get(),
        }
    }

    /// Whether it was made with a filter.
    pub fn traversal_has_filter(&self, traversal: Traversal<'gc>) -> bool {
        traversal.flags().get() & HAS_FILTER != 0
    }

    /// Its "active" flag: its filter is running.
    pub fn traversal_active(&self, traversal: Traversal<'gc>) -> bool {
        traversal.flags().get() & ACTIVE != 0
    }

    pub fn set_traversal_active(&self, traversal: Traversal<'gc>, active: bool) {
        flag(traversal.flags(), ACTIVE, active);
    }

    /// Its reference to its filter, which only the engine reads and writes.
    pub fn traversal_filter_edge(&self, traversal: Traversal<'gc>) -> &'gc JsEdge {
        match traversal {
            Traversal::NodeIterator(iterator) => &iterator.into_ref().filter,
            Traversal::TreeWalker(walker) => &walker.into_ref().filter,
        }
    }

    pub(crate) fn node_iterator_wrapper_edge(
        &self,
        iterator: Gc<'gc, NodeIterator>,
    ) -> &'gc JsEdge {
        &iterator.into_ref().wrapper
    }

    pub(crate) fn tree_walker_wrapper_edge(&self, walker: Gc<'gc, TreeWalker>) -> &'gc JsEdge {
        &walker.into_ref().wrapper
    }

    /// Its reference and whether its pointer is before it.
    pub fn node_iterator_position(&self, iterator: Gc<'gc, NodeIterator>) -> (NodeRef<'gc>, bool) {
        (
            iterator
                .reference
                .get(self.gc)
                .expect("an iterator's reference"),
            iterator.flags.get() & POINTER_BEFORE_REFERENCE != 0,
        )
    }

    pub fn set_node_iterator_position(
        &self,
        iterator: Gc<'gc, NodeIterator>,
        (node, before): (NodeRef<'gc>, bool),
    ) {
        iterator.reference.set(self.gc, Some(node));
        flag(&iterator.flags, POINTER_BEFORE_REFERENCE, before);
    }

    /// The position its traversal is filtering, if its filter runs.
    pub fn node_iterator_candidate(
        &self,
        iterator: Gc<'gc, NodeIterator>,
    ) -> Option<(NodeRef<'gc>, bool)> {
        let node = iterator.candidate.get(self.gc)?;
        Some((node, iterator.flags.get() & POINTER_BEFORE_CANDIDATE != 0))
    }

    pub fn set_node_iterator_candidate(
        &self,
        iterator: Gc<'gc, NodeIterator>,
        candidate: Option<(NodeRef<'gc>, bool)>,
    ) {
        iterator
            .candidate
            .set(self.gc, candidate.map(|(node, _)| node));
        flag(
            &iterator.flags,
            POINTER_BEFORE_CANDIDATE,
            candidate.is_some_and(|(_, before)| before),
        );
    }

    pub fn tree_walker_current(&self, walker: Gc<'gc, TreeWalker>) -> NodeRef<'gc> {
        walker
            .current
            .get(self.gc)
            .expect("a walker's current node")
    }

    pub fn set_tree_walker_current(&self, walker: Gc<'gc, TreeWalker>, node: NodeRef<'gc>) {
        walker.current.set(self.gc, Some(node));
    }

    fn add_node_iterator(&self, document: NodeRef<'gc>, iterator: Gc<'gc, NodeIterator>) {
        self.see(crate::form::LIVE_SEEN);
        let data = self.ensure_document_data(document);
        let list = match data.node_iterators.get(self.gc) {
            Some(list) => list,
            None => {
                let list = HeapVector::new(self.gc, 1);
                data.node_iterators.set(self.gc, Some(list));
                list
            }
        };
        crate::intersection::prune(self, list, |entry: Gc<'gc, NodeIteratorRef>| {
            entry.iterator.get(self.gc).is_some()
        });
        let entry = self.gc.alloc_in(
            &sites::OTHER,
            NodeIteratorRefInit {
                iterator: Some(iterator),
            },
        );
        list.push(self.gc, entry);
    }

    fn node_iterators_of(&self, document: NodeRef<'gc>) -> Vec<Gc<'gc, NodeIterator>> {
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.node_iterators.get(self.gc))
        else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index)?.iterator.get(self.gc))
            .collect()
    }

    /// DOM's "NodeIterator pre-removing steps" of `document`'s iterators,
    /// `node` to be removed: for the reference, and the position being
    /// filtered.
    pub(crate) fn node_iterators_removing(&self, document: NodeRef<'gc>, node: NodeRef<'gc>) {
        for iterator in self.node_iterators_of(document) {
            let root = iterator.root.get(self.gc).expect("an iterator's root");
            self.move_off(
                root,
                node,
                &iterator.reference,
                &iterator.flags,
                POINTER_BEFORE_REFERENCE,
            );
            self.move_off(
                root,
                node,
                &iterator.candidate,
                &iterator.flags,
                POINTER_BEFORE_CANDIDATE,
            );
        }
    }

    /// The pre-removing steps for one position of an iterator over `root`
    /// (the node it is at, none for none; whether the pointer is before it,
    /// `before` of `flags`).
    fn move_off(
        &self,
        root: NodeRef<'gc>,
        node: NodeRef<'gc>,
        position: &Member<Node>,
        flags: &Cell<u8>,
        before: u8,
    ) {
        let Some(at) = position.get(self.gc) else {
            return;
        };
        // 1. Only when the node holds the position, and not the root
        // (Blink's `NodeIterator::NodeWillBeRemoved` checks the same): the
        // position is in the root's subtree, so the node is below the root.
        if !self.is_inclusive_ancestor(node, at) || self.is_inclusive_ancestor(node, root) {
            return;
        }
        // 2. The pointer before it: the first node after `node`'s subtree
        // that is in the root's, if there is one.
        if flags.get() & before != 0 {
            if let Some(next) = self.following_past(node, root) {
                position.set(self.gc, Some(next));
                return;
            }
            flag(flags, before, false);
        }
        // 3. Its parent, if it has no previous sibling; else the last
        // inclusive descendant of that sibling.
        let Some(mut last) = self.previous_sibling(node) else {
            position.set(self.gc, self.parent(node));
            return;
        };
        while let Some(child) = self.last_child(last) {
            last = child;
        }
        position.set(self.gc, Some(last));
    }

    /// The first node after `node`'s subtree in tree order, in `root`'s.
    fn following_past(&self, node: NodeRef<'gc>, root: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut at = node;
        loop {
            if Gc::ptr_eq(at, root) {
                return None;
            }
            if let Some(next) = self.next_sibling(at) {
                return Some(next);
            }
            at = self.parent(at)?;
        }
    }

    /// `old`'s node iterators whose root is now in `document`, moved to its
    /// list.
    pub(crate) fn move_node_iterators(&self, old: NodeRef<'gc>, document: NodeRef<'gc>) {
        let Some(list) = self
            .document_data(old)
            .and_then(|data| data.node_iterators.get(self.gc))
        else {
            return;
        };
        let mut index = 0;
        while let Some(entry) = list.get(self.gc, index) {
            let moved = entry.iterator.get(self.gc).filter(|iterator| {
                let root = iterator.root.get(self.gc).expect("an iterator's root");
                Gc::ptr_eq(self.document_of(root), document)
            });
            match moved {
                Some(iterator) => {
                    list.remove(self.gc, index);
                    self.add_node_iterator(document, iterator);
                }
                None => index = index.saturating_add(1),
            }
        }
    }
}
