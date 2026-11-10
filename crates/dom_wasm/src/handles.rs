//! The stores' handles to the DOM's objects (Blink's `DOMDataStore`, a
//! store a world): a guest holds an object (a node, an event, a window...)
//! as a number of its store's own, one for each object it was given, the
//! same each time, until it releases it.
//!
//! - **A store's table** (`Handles`): its slots, each an object's or free.
//!   The objects are edges of the store's holder, an object of the DOM's
//!   heap (chunks of `CHUNK` members), not roots: what only the guest holds
//!   lives as long as the store does, and goes with it. Each slot's
//!   generation and its object's interface (the one its wrapper would be
//!   of: an element's by its name) are the store's own, outside the heap,
//!   so that a handle is checked (and `this`'s interface) before an
//!   operation, with no DOM.
//! - **A handle** is a slot's index (its low `INDEX_BITS`) and its
//!   generation (the high 8, 1 to 255): a released slot's next generation
//!   tells the old handle from the new, and 0 is never one.
//! - **Finding an object's handle in a store**: the DOM's first store is
//!   its main one (Blink's main world), whose handle is in the object's
//!   engine word's low half (`Cx::object_engine_word`, the 8 bytes a JS
//!   document's object keeps its wrapper in); another store's handles are
//!   in a table the stores share, by the word's high half (1 or more),
//!   each entry the stores' and their handles. Once the main store goes,
//!   none is main again: another store's handles are in the shared table,
//!   which looking at the word alone would miss.
//! - **A word may be stale**: a release has the slot, not the object's
//!   type, so it leaves the word as it was. What a word names is checked
//!   before it is believed: a main handle against the store's table (the
//!   slot holds this object), a shared entry against one of its own
//!   handles (which holds this object: an entry is given to another object
//!   once no store is in it). A slot keeps its entry, so that releasing it
//!   finds the entry without the object.
//!
//! A shared entry exists while a store holds the object: releasing it, or
//! the store going, takes the store's handle out of it. The word and the
//! entries are numbers, not edges: they hold nothing alive.

use dom::{Cx, NodeRef, ObjectRef};
use dom_api::ApiCx;
use elise::{Erased, Gc, GcObject, HeapVector, Member, Persistent};

/// The slots of a holder's chunk.
const CHUNK: usize = 256;
/// A handle's bits for its slot's index: a store holds at most 2^24 objects.
const INDEX_BITS: u32 = 24;
const INDEX_MASK: u32 = (1 << INDEX_BITS) - 1;
/// The engine word's halves: the main store's handle, the shared entry.
const LOW: u64 = 0xffff_ffff;

/// A chunk of a store's holder: its slots' objects, none for a free slot.
#[derive(GcObject)]
pub(crate) struct Chunk {
    objects: [Member<Erased>; CHUNK],
}

/// A slot's generation, its object's interface (none if it is free: an
/// index of `dom_api::INTERFACES`, which a `u16` holds), and the shared
/// entry its handle is in (0: none, the main store's).
#[derive(Clone, Copy)]
struct Slot {
    generation: u8,
    interface: Option<u16>,
    entry: u32,
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

    /// The interface of the object `handle` holds, if it is one of the
    /// store's.
    pub(crate) fn interface(&self, handle: u32) -> Option<usize> {
        let (index, generation) = parts(handle);
        self.slots
            .get(index)
            .filter(|slot| slot.generation == generation)
            .and_then(|slot| slot.interface)
            .map(usize::from)
    }

    /// The object `handle` holds, which `interface` found one.
    pub(crate) fn object<'gc>(&self, cx: &Cx<'_, 'gc>, handle: u32) -> Gc<'gc, Erased> {
        debug_assert!(self.interface(handle).is_some(), "a handle checked");
        self.member(cx, parts(handle).0)
            .get(cx.gc)
            .expect("a held slot's object")
    }

    /// The node `handle` holds, which `interface` found a node's.
    pub(crate) fn node<'gc>(&self, cx: &Cx<'_, 'gc>, handle: u32) -> NodeRef<'gc> {
        self.object(cx, handle)
            .downcast()
            .expect("a handle checked to be a node's")
    }

    /// How many objects the store holds.
    pub(crate) fn held(&self) -> usize {
        self.held
    }

    fn member<'gc>(&self, cx: &Cx<'_, 'gc>, index: usize) -> &'gc Member<Erased> {
        let chunk = self
            .holder
            .get(cx.gc)
            .get(cx.gc, index / CHUNK)
            .expect("a slot's chunk")
            .into_ref();
        &chunk.objects[index % CHUNK]
    }

    /// Whether `handle` holds `object`.
    fn holds<'gc>(&self, cx: &Cx<'_, 'gc>, handle: u32, object: Gc<'gc, Erased>) -> bool {
        self.interface(handle).is_some() && Gc::ptr_eq(self.object(cx, handle), object)
    }

    /// A new handle to `object`, of `interface`, in the shared `entry` (0:
    /// none), in a free slot or a new one.
    fn hold<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        object: Gc<'gc, Erased>,
        interface: usize,
        entry: u32,
    ) -> u32 {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                let index = u32::try_from(self.slots.len())
                    .ok()
                    .filter(|&index| index <= INDEX_MASK)
                    .expect("a store holds at most 2^24 objects");
                self.slots.push(Slot {
                    generation: 1,
                    interface: None,
                    entry: 0,
                });
                index
            }
        };
        let at = index as usize;
        let chunks = self.holder.get(cx.gc);
        while chunks.len() <= at / CHUNK {
            let chunk = cx.gc.alloc(ChunkInit {
                objects: [None; CHUNK],
            });
            chunks.push(cx.gc, chunk);
        }
        self.member(cx, at).set(cx.gc, Some(object));
        let slot = &mut self.slots[at];
        slot.interface = Some(u16::try_from(interface).expect("the table's interfaces fit"));
        slot.entry = entry;
        // Below `slots.len()`, which a `Vec` keeps below `isize::MAX`.
        self.held = self.held.saturating_add(1);
        handle(index, slot.generation)
    }

    /// Frees `handle`'s slot, which `interface` found one; the shared
    /// entry its handle was in.
    fn free<'gc>(&mut self, cx: &Cx<'_, 'gc>, handle: u32) -> u32 {
        let (at, _) = parts(handle);
        self.member(cx, at).set(cx.gc, None);
        let slot = &mut self.slots[at];
        slot.interface = None;
        slot.generation = if slot.generation == u8::MAX {
            1
        } else {
            slot.generation.saturating_add(1)
        };
        self.free.push(handle & INDEX_MASK);
        self.held = self.held.saturating_sub(1);
        std::mem::take(&mut slot.entry)
    }

    /// The shared entry of each handle the store holds.
    fn entries(&self) -> Vec<u32> {
        self.slots
            .iter()
            .filter(|slot| slot.interface.is_some())
            .map(|slot| slot.entry)
            .collect()
    }
}

/// The entries of the stores but the main one, by the engine word's high
/// half (entry 0 none: the word's 0). An entry in use has a store in it.
#[derive(Default)]
struct Shared {
    entries: Vec<Vec<(u32, u32)>>,
    free: Vec<u32>,
}

impl Shared {
    fn entry(word: u64) -> u32 {
        u32::try_from(word >> 32).expect("the high half")
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

    /// Takes `store`'s handle out of `entry`, and the entry away once no
    /// store is in it (it is another object's from then).
    fn leave(&mut self, entry: u32, store: u32) {
        let handles = &mut self.entries[entry as usize];
        handles.retain(|&(of, _)| of != store);
        if handles.is_empty() {
            self.free.push(entry);
        }
    }
}

/// Every store's handles to a DOM's objects.
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

    /// Whether `entry` is `object`'s: in use, and one of its handles holds
    /// the object (a stale word names an entry given to another since).
    fn entry_is_of<'gc>(&self, cx: &Cx<'_, 'gc>, entry: u32, object: Gc<'gc, Erased>) -> bool {
        self.shared
            .entries
            .get(entry as usize)
            .and_then(|handles| handles.first())
            .is_some_and(|&(store, handle)| {
                self.store(store)
                    .is_some_and(|handles| handles.holds(cx, handle, object))
            })
    }

    /// The store's handle to `object`: the one it has, or a new one.
    pub(crate) fn give<'gc>(
        &mut self,
        cx: &Cx<'_, 'gc>,
        store: u32,
        object: ObjectRef<'gc>,
    ) -> u32 {
        let erased = object.erase();
        let word = cx.object_engine_word(object);
        let main = self.main == Some(store);
        let mut entry = 0;
        if main {
            let handle = u32::try_from(word & LOW).expect("the low half");
            if self.store_mut(store).holds(cx, handle, erased) {
                return handle;
            }
        } else {
            entry = Shared::entry(word);
            if entry != 0 && self.entry_is_of(cx, entry, erased) {
                if let Some(&(_, handle)) = self.shared.entries[entry as usize]
                    .iter()
                    .find(|(of, _)| *of == store)
                {
                    debug_assert!(self.store_mut(store).holds(cx, handle, erased));
                    return handle;
                }
            } else {
                entry = self.shared.add();
                cx.set_object_engine_word(object, (word & LOW) | (u64::from(entry) << 32));
            }
        }
        let interface = dom_api::typed::interface_of(&ApiCx::new(cx), object);
        let handle = self.store_mut(store).hold(cx, erased, interface, entry);
        if main {
            cx.set_object_engine_word(object, (word & !LOW) | u64::from(handle));
        } else {
            self.shared.entries[entry as usize].push((store, handle));
        }
        handle
    }

    /// Lets the store's `handle` go, which `Handles::interface` found one.
    /// The object's word is left: nothing believes it unchecked.
    pub(crate) fn release<'gc>(&mut self, cx: &Cx<'_, 'gc>, store: u32, handle: u32) {
        let entry = self.store_mut(store).free(cx, handle);
        if entry != 0 {
            self.shared.leave(entry, store);
        }
    }

    /// Takes the store's table away, with its holder: what only it held
    /// goes at the next GC. Its handles leave the shared entries; the main
    /// store's words are left: `main` names no store after it (a store's
    /// number isn't used again), so none reads them.
    pub(crate) fn remove_store(&mut self, store: u32) {
        let Some(handles) = self.stores.get_mut(store as usize).and_then(Option::take) else {
            return;
        };
        for entry in handles.entries() {
            if entry != 0 {
                self.shared.leave(entry, store);
            }
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
            let handle = tables.give(cx, store, p.into());
            assert_ne!(handle, 0);
            assert_eq!(tables.give(cx, store, p.into()), handle);
            assert_eq!(cx.engine_word(p), u64::from(handle));
            let document = cx.document_of(p);
            let other = tables.give(cx, store, document.into());
            assert_ne!(other, handle);
            assert_eq!(tables.store(store).unwrap().held(), 2);
            tables.release(cx, store, handle);
            assert_eq!(tables.store(store).unwrap().interface(handle), None);
            // The word is left, and not believed: the slot's generation
            // isn't the handle's any more.
            assert_eq!(cx.engine_word(p), u64::from(handle));
            let again = tables.give(cx, store, p.into());
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
            let main_handle = tables.give(cx, main, p.into());
            let second_handle = tables.give(cx, second, p.into());
            assert_eq!(tables.give(cx, second, p.into()), second_handle);
            let word = cx.engine_word(p);
            assert_eq!(
                word & 0xffff_ffff,
                u64::from(main_handle),
                "the main handle stays"
            );
            assert_ne!(word >> 32, 0);
            assert_eq!(
                tables.give(cx, third, p.into()),
                tables.give(cx, third, p.into())
            );
            assert_eq!(cx.engine_word(p), word, "one entry for both");
            assert_eq!(tables.shared_entries(), 1);
            tables.release(cx, second, second_handle);
            assert_eq!(tables.shared_entries(), 1, "the third store's is left");
            assert_eq!(tables.store(second).unwrap().interface(second_handle), None);
            tables.remove_store(third);
            assert_eq!(tables.shared_entries(), 0);
            // The word still names the entry, which is no one's now: a
            // store given the node again gets a new one.
            assert_eq!(cx.engine_word(p), word);
            assert!(tables.store(third).is_none());
            let again = tables.give(cx, second, p.into());
            assert_eq!(tables.give(cx, second, p.into()), again);
            assert_eq!(tables.shared_entries(), 1);
            assert_eq!(cx.engine_word(p) & 0xffff_ffff, u64::from(main_handle));
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
            let left = tables.give(cx, main, p.into());
            tables.remove_store(main);
            let later = tables.add_store(cx);
            let handle = tables.give(cx, later, p.into());
            assert_eq!(tables.give(cx, later, p.into()), handle);
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
            (store, tables.give(cx, store, node(cx, &weak).into()))
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
            tables.give(cx, main, node(cx, &weak).into());
            tables.give(cx, second, node(cx, &weak).into());
            tables.remove_store(main);
        });
        dom.collect();
        dom.mutate(|cx| {
            assert!(weak.get(cx.gc).is_some(), "the second store holds it");
            tables.remove_store(1);
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
                let handle = tables.give(cx, store, p.into());
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
                .map(|&text| tables.give(cx, store, text.into()))
                .collect();
            for (&text, &handle) in texts.iter().zip(&handles) {
                assert!(elise::Gc::ptr_eq(
                    tables.store(store).unwrap().node(cx, handle),
                    text
                ));
            }
        });
    }

    /// A handle is to any object, a window as a node: the same each time
    /// in the main store and in another, each with the interface its
    /// wrapper would be of (an element's by its name).
    #[test]
    fn a_handle_is_to_any_object_with_its_interface() {
        let mut dom = Dom::new();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let interface = |name| dom_api::declare::interface_index(name);
            let document = cx.create_document(true);
            let window = cx.create_window(document);
            let input = cx.create_element(document, QualName::html("input"));
            let p = cx.create_element(document, QualName::html("p"));
            let main = tables.add_store(cx);
            let second = tables.add_store(cx);
            for store in [main, second] {
                let of = |tables: &Tables, handle| tables.store(store).unwrap().interface(handle);
                let handle = tables.give(cx, store, window.into());
                assert_eq!(tables.give(cx, store, window.into()), handle);
                assert_eq!(of(&tables, handle), Some(interface("Window")));
                assert!(elise::Gc::ptr_eq(
                    tables.store(store).unwrap().object(cx, handle),
                    window.erase()
                ));
                let handle = tables.give(cx, store, input.into());
                assert_eq!(of(&tables, handle), Some(interface("HTMLInputElement")));
                let handle = tables.give(cx, store, p.into());
                assert_eq!(of(&tables, handle), Some(interface("HTMLParagraphElement")));
                let handle = tables.give(cx, store, document.into());
                assert_eq!(of(&tables, handle), Some(interface("Document")));
            }
            assert_eq!(tables.store(main).unwrap().held(), 4);
            assert_eq!(tables.shared_entries(), 4);
        });
    }

    /// A shared entry let go is given to another object; the word of the
    /// object it was is left naming it, and isn't believed: each object
    /// keeps its own handle in each store.
    #[test]
    fn a_stale_word_names_no_entry_of_the_object() {
        let (mut dom, weak) = dom_and_element();
        let mut tables = Tables::default();
        dom.mutate(|cx| {
            let _main = tables.add_store(cx);
            let second = tables.add_store(cx);
            let third = tables.add_store(cx);
            let p = node(cx, &weak);
            let document = cx.document_of(p);
            // `p`'s entry, let go.
            let handle = tables.give(cx, second, p.into());
            let word = cx.engine_word(p);
            tables.release(cx, second, handle);
            assert_eq!(tables.shared_entries(), 0);
            // The document takes the entry `p`'s word still names.
            let of_document = tables.give(cx, third, document.into());
            assert_eq!(
                cx.engine_word(document) >> 32,
                word >> 32,
                "the entry, reused"
            );
            // `p` again, in the store the document's entry is of and in
            // another: an entry of its own, the document's untouched.
            let in_third = tables.give(cx, third, p.into());
            assert_ne!(in_third, of_document);
            assert_ne!(cx.engine_word(p) >> 32, word >> 32);
            let in_second = tables.give(cx, second, p.into());
            assert_eq!(tables.give(cx, second, p.into()), in_second);
            assert_eq!(tables.give(cx, third, p.into()), in_third);
            assert_eq!(tables.give(cx, third, document.into()), of_document);
            assert_eq!(tables.shared_entries(), 2);
            assert!(elise::Gc::ptr_eq(
                tables.store(third).unwrap().node(cx, in_third),
                p
            ));
        });
    }
}
