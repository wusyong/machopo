//! The stores' handles to the DOM's nodes (Blink's `DOMDataStore`, a store
//! a world): a guest holds a node as a number of its store's own, one for
//! each node it was given, the same each time, until it releases it.
//!
//! - **A store's table** (`Handles`): its slots, each a node's or free.
//!   The nodes are edges of the store's holder, an object of the DOM's heap
//!   (chunks of `CHUNK` members), not roots: what only the guest holds
//!   lives as long as the store does, and goes with it. Each slot's
//!   generation and node kind are the store's own, outside the heap, so
//!   that a handle is checked (and `this`'s interface) before an operation,
//!   with no DOM.
//! - **A handle** is a slot's index (its low `INDEX_BITS`) and its
//!   generation (the high 8, 1 to 255): a released slot's next generation
//!   tells the old handle from the new, and 0 is never one.
//! - **Finding a node's handle in a store**: the DOM's first store is its
//!   main one (Blink's main world), whose handle is in the node's engine
//!   word's low half (`Cx::engine_word`, the 8 bytes a JS document's node
//!   keeps its wrapper in); another store's handles are in a table the
//!   stores share, by the word's high half (1 or more), each entry the
//!   stores' and their handles. A main handle read is checked against the
//!   store's table, so a stale word gives none. Once the main store goes,
//!   none is main again: another store's handles are in the shared table,
//!   which looking at the word alone would miss.
//!
//! A shared entry exists while a store holds the node: releasing it, or
//! the store going, takes the store's handle out of it, so no entry
//! outlives what holds its node. The word and the entries are numbers, not
//! edges: they hold nothing alive.

use dom::{Cx, Node, NodeKind, NodeRef};
use elise::{GcObject, HeapVector, Member, Persistent};

/// The slots of a holder's chunk.
const CHUNK: usize = 256;
/// A handle's bits for its slot's index: a store holds at most 2^24 nodes.
const INDEX_BITS: u32 = 24;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
/// The engine word's halves: the main store's handle, the shared entry.
const LOW: u64 = 0xffff_ffff;

/// A chunk of a store's holder: its slots' nodes, none for a free slot.
#[derive(GcObject)]
pub(crate) struct Chunk {
    nodes: [Member<Node>; CHUNK],
}

/// A slot's generation and its node's kind (none if it is free).
#[derive(Clone, Copy)]
struct Slot {
    generation: u8,
    kind: Option<NodeKind>,
}

/// A store's table.
pub(crate) struct Handles {
    holder: Persistent<HeapVector<Chunk>>,
    slots: Vec<Slot>,
    free: Vec<u32>,
    held: usize,
}

fn handle(index: u32, generation: u8) -> u32 {
    (u32::from(generation) << INDEX_BITS) | index
}

/// A handle's slot index and generation.
fn parts(handle: u32) -> (usize, u8) {
    let generation = u8::try_from(handle >> INDEX_BITS).expect("8 bits");
    ((handle & INDEX_MASK) as usize, generation)
}

impl Handles {
    fn new<'gc>(cx: &Cx<'_, 'gc>) -> Self {
        Self {
            holder: Persistent::new(cx.gc, HeapVector::new(cx.gc, 0)),
            slots: Vec::new(),
            free: Vec::new(),
            held: 0,
        }
    }

    /// The kind of the node `handle` holds, if it is one of the store's.
    pub(crate) fn kind(&self, handle: u32) -> Option<NodeKind> {
        let (index, generation) = parts(handle);
        self.slots
            .get(index)
            .filter(|slot| slot.generation == generation)
            .and_then(|slot| slot.kind)
    }

    /// The node `handle` holds, which `kind` found one.
    pub(crate) fn node<'gc>(&self, cx: &Cx<'_, 'gc>, handle: u32) -> NodeRef<'gc> {
        debug_assert!(self.kind(handle).is_some(), "a handle checked");
        self.member(cx, parts(handle).0)
            .get(cx.gc)
            .expect("a held slot's node")
    }

    /// How many nodes the store holds.
    pub(crate) fn held(&self) -> usize {
        self.held
    }

    fn member<'gc>(&self, cx: &Cx<'_, 'gc>, index: usize) -> &'gc Member<Node> {
        let chunk = self
            .holder
            .get(cx.gc)
            .get(cx.gc, index / CHUNK)
            .expect("a slot's chunk")
            .into_ref();
        &chunk.nodes[index % CHUNK]
    }

    /// Whether `handle` holds `node`.
    fn holds<'gc>(&self, cx: &Cx<'_, 'gc>, handle: u32, node: NodeRef<'gc>) -> bool {
        self.kind(handle).is_some() && elise::Gc::ptr_eq(self.node(cx, handle), node)
    }

    /// A new handle to `node`, in a free slot or a new one.
    fn hold<'gc>(&mut self, cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> u32 {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.slots.len())
                    .ok()
                    .filter(|&index| index <= INDEX_MASK)
                    .expect("a store holds at most 2^24 nodes");
                self.slots.push(Slot {
                    generation: 1,
                    kind: None,
                });
                index
            }
        };
        let at = index as usize;
        let chunks = self.holder.get(cx.gc);
        while chunks.len() <= at / CHUNK {
            let chunk = cx.gc.alloc(ChunkInit {
                nodes: [None; CHUNK],
            });
            chunks.push(cx.gc, chunk);
        }
        self.member(cx, at).set(cx.gc, Some(node));
        let slot = &mut self.slots[at];
        slot.kind = Some(cx.kind(node));
        // Below `slots.len()`, which a `Vec` keeps below `isize::MAX`.
        self.held = self.held.saturating_add(1);
        handle(index, slot.generation)
    }

    /// Frees `handle`'s slot, which `kind` found one; its node.
    fn free<'gc>(&mut self, cx: &Cx<'_, 'gc>, handle: u32) -> NodeRef<'gc> {
        let node = self.node(cx, handle);
        let (at, _) = parts(handle);
        self.member(cx, at).set(cx.gc, None);
        let slot = &mut self.slots[at];
        slot.kind = None;
        slot.generation = if slot.generation == u8::MAX {
            1
        } else {
            slot.generation.saturating_add(1)
        };
        self.free.push(handle & INDEX_MASK);
        self.held = self.held.saturating_sub(1);
        node
    }

    /// Each node the store holds.
    fn nodes<'gc>(&self, cx: &Cx<'_, 'gc>) -> Vec<NodeRef<'gc>> {
        (0..self.slots.len())
            .filter(|&at| self.slots[at].kind.is_some())
            .map(|at| self.member(cx, at).get(cx.gc).expect("a held slot's node"))
            .collect()
    }
}

/// The entries of the stores but the main one, by the engine word's high
/// half (entry 0 none: the word's 0).
#[derive(Default)]
struct Shared {
    entries: Vec<Vec<(u32, u32)>>,
    free: Vec<u32>,
}

impl Shared {
    fn entry(word: u64) -> Option<usize> {
        let entry = word >> 32;
        (entry != 0).then(|| usize::try_from(entry).expect("a shared entry's index"))
    }

    fn add(&mut self) -> u32 {
        if self.entries.is_empty() {
            self.entries.push(Vec::new());
        }
        match self.free.pop() {
            Some(entry) => entry,
            None => {
                self.entries.push(Vec::new());
                u32::try_from(self.entries.len().saturating_sub(1)).expect("2^32 shared entries")
            }
        }
    }
}

/// Every store's handles to a DOM's nodes.
#[derive(Default)]
pub(crate) struct Tables {
    /// By store number, which isn't used again: none once it went.
    stores: Vec<Option<Handles>>,
    shared: Shared,
    main: Option<u32>,
}

impl Tables {
    /// A new store's number, its table empty: the first is the main one.
    pub(crate) fn add_store<'gc>(&mut self, cx: &Cx<'_, 'gc>) -> u32 {
        let store = u32::try_from(self.stores.len()).expect("2^32 stores");
        if self.stores.is_empty() {
            self.main = Some(store);
        }
        self.stores.push(Some(Handles::new(cx)));
        store
    }

    /// The store's table, if it is still there.
    pub(crate) fn store(&self, store: u32) -> Option<&Handles> {
        self.stores.get(store as usize).and_then(Option::as_ref)
    }

    fn store_mut(&mut self, store: u32) -> &mut Handles {
        self.stores
            .get_mut(store as usize)
            .and_then(Option::as_mut)
            .expect("a store's table")
    }

    /// The store's handle to `node`: the one it has, or a new one.
    pub(crate) fn give<'gc>(&mut self, cx: &Cx<'_, 'gc>, store: u32, node: NodeRef<'gc>) -> u32 {
        let word = cx.engine_word(node);
        let main = self.main == Some(store);
        if main {
            let handle = u32::try_from(word & LOW).expect("the low half");
            if self.store_mut(store).holds(cx, handle, node) {
                return handle;
            }
        } else if let Some(entry) = Shared::entry(word)
            && let Some(&(_, handle)) = self.shared.entries[entry]
                .iter()
                .find(|(of, _)| *of == store)
        {
            debug_assert!(self.store_mut(store).holds(cx, handle, node));
            return handle;
        }
        let handle = self.store_mut(store).hold(cx, node);
        if main {
            cx.set_engine_word(node, (word & !LOW) | u64::from(handle));
        } else {
            let entry = match Shared::entry(word) {
                Some(entry) => entry,
                None => {
                    let entry = self.shared.add();
                    cx.set_engine_word(node, (word & LOW) | (u64::from(entry) << 32));
                    entry as usize
                }
            };
            self.shared.entries[entry].push((store, handle));
        }
        handle
    }

    /// Lets the store's `handle` go, which `Handles::kind` found one.
    pub(crate) fn release<'gc>(&mut self, cx: &Cx<'_, 'gc>, store: u32, handle: u32) {
        let node = self.store_mut(store).free(cx, handle);
        let word = cx.engine_word(node);
        if self.main == Some(store) {
            debug_assert_eq!(word & LOW, u64::from(handle), "the main handle in the word");
            cx.set_engine_word(node, word & !LOW);
        } else {
            self.unshare(cx, store, node, word);
        }
    }

    /// Takes the store's handle to `node` out of its shared entry, and the
    /// entry away once no store is in it.
    fn unshare<'gc>(&mut self, cx: &Cx<'_, 'gc>, store: u32, node: NodeRef<'gc>, word: u64) {
        let entry = Shared::entry(word).expect("a shared node's entry");
        let handles = &mut self.shared.entries[entry];
        handles.retain(|&(of, _)| of != store);
        if handles.is_empty() {
            self.shared
                .free
                .push(u32::try_from(entry).expect("an entry's index"));
            cx.set_engine_word(node, word & LOW);
        }
    }

    /// Takes the store's table away, with its holder: what only it held
    /// goes at the next GC. Its handles leave the shared entries; the main
    /// store's words are left: `main` names no store after it (a store's
    /// number isn't used again), so none reads them.
    pub(crate) fn remove_store<'gc>(&mut self, cx: &Cx<'_, 'gc>, store: u32) {
        let Some(handles) = self.stores.get_mut(store as usize).and_then(Option::take) else {
            return;
        };
        if self.main == Some(store) {
            return;
        }
        for node in handles.nodes(cx) {
            let word = cx.engine_word(node);
            self.unshare(cx, store, node, word);
        }
    }

    /// The shared entries in use (the tests' count).
    pub(crate) fn shared_entries(&self) -> usize {
        self.shared
            .entries
            .len()
            .saturating_sub(1)
            .saturating_sub(self.shared.free.len())
    }
}

#[cfg(test)]
mod tests {
    use dom::{Dom, NodeRef, QualName};
    use elise::WeakPersistent;

    use super::{INDEX_BITS, Tables};

    /// A DOM with an element no root holds, and its weak root.
    fn dom_and_element() -> (Dom, WeakPersistent<dom::Node>) {
        let mut dom = Dom::new();
        let weak = dom.mutate(|cx| {
            let document = cx.create_document(true);
            WeakPersistent::new(cx.gc, cx.create_element(document, QualName::html("p")))
        });
        (dom, weak)
    }

    fn node<'gc>(cx: &dom::Cx<'_, 'gc>, weak: &WeakPersistent<dom::Node>) -> NodeRef<'gc> {
        weak.get(cx.gc).expect("the node lives")
    }

    /// The main store's handle to a node is the same each time, in the
    /// node's engine word's low half; another node's is another; released,
    /// the handle is no node's, and the slot's next handle is another
    /// number.
    #[test]
    fn the_main_stores_handle_is_the_same_each_time_until_released() {
        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let store = tables.add_store(cx);
            let p = node(cx, &weak);
            let handle = tables.give(cx, store, p);
            assert_ne!(handle, 0);
            assert_eq!(tables.give(cx, store, p), handle);
            assert_eq!(cx.engine_word(p), u64::from(handle));
            let document = cx.document_of(p);
            let other = tables.give(cx, store, document);
            assert_ne!(other, handle);
            assert_eq!(tables.store(store).unwrap().held(), 2);
            tables.release(cx, store, handle);
            assert_eq!(tables.store(store).unwrap().kind(handle), None);
            assert_eq!(cx.engine_word(p), 0);
            let again = tables.give(cx, store, p);
            assert_ne!(again, handle, "the slot's next generation");
            assert_eq!(
                again & ((1 << INDEX_BITS) - 1),
                handle & ((1 << INDEX_BITS) - 1)
            );
        });
    }

    /// Another store's handle to a node the main store holds is its own,
    /// the same each time, in an entry shared by the word's high half; a
    /// third store's in the same entry. The entry goes with the last store
    /// in it, its release or its going.
    #[test]
    fn other_stores_handles_are_in_a_shared_entry() {
        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let main = tables.add_store(cx);
            let second = tables.add_store(cx);
            let third = tables.add_store(cx);
            let p = node(cx, &weak);
            let main_handle = tables.give(cx, main, p);
            let second_handle = tables.give(cx, second, p);
            assert_eq!(tables.give(cx, second, p), second_handle);
            let word = cx.engine_word(p);
            assert_eq!(
                word & 0xffff_ffff,
                u64::from(main_handle),
                "the main handle stays"
            );
            assert_ne!(word >> 32, 0);
            assert_eq!(tables.give(cx, third, p), tables.give(cx, third, p));
            assert_eq!(cx.engine_word(p), word, "one entry for both");
            assert_eq!(tables.shared_entries(), 1);
            tables.release(cx, second, second_handle);
            assert_eq!(tables.shared_entries(), 1, "the third store's is left");
            assert_eq!(tables.store(second).unwrap().kind(second_handle), None);
            tables.remove_store(cx, third);
            assert_eq!(tables.shared_entries(), 0);
            assert_eq!(cx.engine_word(p), u64::from(main_handle));
            assert!(tables.store(third).is_none());
        });
    }

    /// Once the main store goes, none is main: a later store's handles are
    /// in the shared table, not taken from the main one's word left behind.
    #[test]
    fn no_store_is_main_after_the_main_one() {
        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let main = tables.add_store(cx);
            let p = node(cx, &weak);
            let left = tables.give(cx, main, p);
            tables.remove_store(cx, main);
            let later = tables.add_store(cx);
            let handle = tables.give(cx, later, p);
            assert_eq!(tables.give(cx, later, p), handle);
            assert_eq!(
                cx.engine_word(p) & 0xffff_ffff,
                u64::from(left),
                "the old word's left"
            );
            assert_eq!(tables.shared_entries(), 1);
        });
    }

    /// A store's handles hold their nodes past a GC; released, or with the
    /// store gone, what only they held is collected.
    #[test]
    fn handles_hold_their_nodes_until_released_or_the_store_goes() {
        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        let (store, handle) = dom.mutate(|cx| {
            let store = tables.add_store(cx);
            (store, tables.give(cx, store, node(cx, &weak)))
        });
        dom.collect();
        dom.mutate(|cx| {
            assert!(weak.get(cx.gc).is_some(), "the handle holds it");
            tables.release(cx, store, handle);
        });
        dom.collect();
        dom.mutate(|cx| assert!(weak.get(cx.gc).is_none(), "released, it is collected"));

        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let main = tables.add_store(cx);
            let second = tables.add_store(cx);
            tables.give(cx, main, node(cx, &weak));
            tables.give(cx, second, node(cx, &weak));
            tables.remove_store(cx, main);
        });
        dom.collect();
        dom.mutate(|cx| {
            assert!(weak.get(cx.gc).is_some(), "the second store holds it");
            tables.remove_store(cx, 1);
        });
        dom.collect();
        dom.mutate(|cx| assert!(weak.get(cx.gc).is_none(), "no store holds it"));
    }

    /// A slot's generation runs 1 to 255 and round: no handle is 0, and a
    /// store holds more nodes than a chunk.
    #[test]
    fn generations_wrap_past_zero_and_chunks_grow() {
        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let store = tables.add_store(cx);
            let p = node(cx, &weak);
            let mut seen = Vec::new();
            for _ in 0..300 {
                let handle = tables.give(cx, store, p);
                assert_ne!(handle, 0);
                seen.push(handle >> INDEX_BITS);
                tables.release(cx, store, handle);
            }
            assert_eq!(seen[0], 1);
            assert_eq!(seen[254], 255);
            assert_eq!(seen[255], 1, "255 is followed by 1");
            let document = cx.document_of(p);
            let texts: Vec<_> = (0..600).map(|_| cx.create_text(document, "t")).collect();
            let handles: Vec<_> = texts
                .iter()
                .map(|&text| tables.give(cx, store, text))
                .collect();
            for (&text, &handle) in texts.iter().zip(&handles) {
                assert!(elise::Gc::ptr_eq(
                    tables.store(store).unwrap().node(cx, handle),
                    text
                ));
            }
        });
    }
}
