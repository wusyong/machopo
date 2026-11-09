//! Mutation observers (DOM's, https://dom.spec.whatwg.org/#mutation-observers;
//! Blink's `MutationObserver`, `MutationObserverRegistration` and
//! `MutationRecord`): a node's registrations are in its rare data
//! (`NodeRareData`), a bit of the node saying it has some, so that queuing
//! a record reads nothing more of an ancestor without; the DOM says
//! whether anything was ever observed at all (`MutationState`), which the
//! changes look at first. The observers with records or transient
//! registrations are the DOM's, rooted, until their records are delivered
//! (`take_pending_observers`): the engine queues the microtask that does it
//! once an operation queued the first record (`take_microtasks`).

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, LocalExternal, Member, Persistent};

use crate::{Cx, DomString, NameId, Node, NodeId, NodeList, NodeRef, QualifiedName, names, sites};

/// A node's: mutation observers are registered on it (its rare data's
/// `observers`).
pub(crate) const HAS_OBSERVERS: u32 = 1 << 29;

/// What a registration observes: `MutationObserverInit`'s booleans.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ObserveOptions {
    pub child_list: bool,
    pub attributes: bool,
    pub character_data: bool,
    pub subtree: bool,
    pub attribute_old_value: bool,
    pub character_data_old_value: bool,
}

const CHILD_LIST: u8 = 1;
const ATTRIBUTES: u8 = 1 << 1;
const CHARACTER_DATA: u8 = 1 << 2;
const SUBTREE: u8 = 1 << 3;
const ATTRIBUTE_OLD_VALUE: u8 = 1 << 4;
const CHARACTER_DATA_OLD_VALUE: u8 = 1 << 5;

impl ObserveOptions {
    fn bits(self) -> u8 {
        [
            (self.child_list, CHILD_LIST),
            (self.attributes, ATTRIBUTES),
            (self.character_data, CHARACTER_DATA),
            (self.subtree, SUBTREE),
            (self.attribute_old_value, ATTRIBUTE_OLD_VALUE),
            (self.character_data_old_value, CHARACTER_DATA_OLD_VALUE),
        ]
        .into_iter()
        .filter(|(on, _)| *on)
        .fold(0, |bits, (_, bit)| bits | bit)
    }
}

/// A mutation observer: its callback (the engine's), its records queued,
/// the nodes it is registered on and those it has transient registrations
/// on (weakly, by id: DOM's "node list" of weak references).
#[derive(GcObject)]
pub struct MutationObserver {
    callback: JsEdge,
    records: Member<HeapVector<MutationRecord>>,
    nodes: LocalExternal<Cell<Vec<NodeId>>>,
    transient_nodes: LocalExternal<Cell<Vec<NodeId>>>,
    /// Whether it is among the DOM's pending observers.
    pending: Cell<bool>,
    wrapper: JsEdge,
}

/// A registration on a node (DOM's "registered observer"): its observer,
/// what it observes, the attributes' local names it keeps to (none: all);
/// a transient one's source, the registration on an ancestor it was made
/// from as the node was removed.
#[derive(GcObject)]
pub struct Registration {
    observer: Member<MutationObserver>,
    options: Cell<u8>,
    filter: LocalExternal<Option<Box<[NameId]>>>,
    source: Member<Registration>,
}

/// What a record is of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationKind {
    ChildList,
    Attributes,
    CharacterData,
}

impl MutationKind {
    /// Its `type`.
    pub fn name(self) -> &'static str {
        match self {
            Self::ChildList => "childList",
            Self::Attributes => "attributes",
            Self::CharacterData => "characterData",
        }
    }
}

/// A mutation record: what changed and where, made as it changed.
#[derive(GcObject)]
pub struct MutationRecord {
    kind: Cell<u8>,
    target: Member<Node>,
    added: Member<NodeList>,
    removed: Member<NodeList>,
    previous: Member<Node>,
    next: Member<Node>,
    /// An attribute's local name and namespace (`names::NONE`: none).
    name: Cell<NameId>,
    namespace: Cell<NameId>,
    old_value: LocalExternal<Option<DomString>>,
    wrapper: JsEdge,
}

/// The DOM's mutation observers' state: whether any node was ever
/// observed (what a change looks at first), HTML's "mutation observer
/// microtask queued" and whether the engine has yet to queue it, and the
/// observers with records or transient registrations (rooted: the agent's
/// "mutation observers" that a delivery goes through).
#[derive(Default)]
pub(crate) struct MutationState {
    observed: Cell<bool>,
    queued: Cell<bool>,
    /// Taken and put back (a move, no borrow).
    pending: Cell<Option<Persistent<HeapVector<MutationObserver>>>>,
    /// DOM's "signal slots": the slots to fire `slotchange` at, with the
    /// observers' delivery (`slot`'s `signal_slot_change`).
    signal_slots: Cell<Option<Persistent<HeapVector<Node>>>>,
}

/// The interested observers of a record, in order, each with the old value
/// it gets.
type Interested<'gc> = Vec<(Gc<'gc, MutationObserver>, bool)>;

impl<'gc> Cx<'_, 'gc> {
    /// A new observer; its callback the engine's to set.
    pub fn create_mutation_observer(&self) -> Gc<'gc, MutationObserver> {
        self.gc.alloc_in(
            &sites::OTHER,
            MutationObserverInit {
                callback: (),
                records: None,
                nodes: LocalExternal::new(Cell::new(Vec::new())),
                transient_nodes: LocalExternal::new(Cell::new(Vec::new())),
                pending: Cell::new(false),
                wrapper: (),
            },
        )
    }

    /// `observer`'s reference to its callback, which only the engine reads
    /// and writes.
    pub fn observer_callback_edge(&self, observer: Gc<'gc, MutationObserver>) -> &'gc JsEdge {
        &observer.into_ref().callback
    }

    pub(crate) fn observer_wrapper_edge(&self, observer: Gc<'gc, MutationObserver>) -> &'gc JsEdge {
        &observer.into_ref().wrapper
    }

    pub(crate) fn record_wrapper_edge(&self, record: Gc<'gc, MutationRecord>) -> &'gc JsEdge {
        &record.into_ref().wrapper
    }

    /// `target`'s registrations, if it has any.
    fn registrations(&self, target: NodeRef<'gc>) -> Option<Gc<'gc, HeapVector<Registration>>> {
        if target.flags.get() & HAS_OBSERVERS == 0 {
            return None;
        }
        self.node_rare(target, false)?
            .into_ref()
            .observers
            .get(self.gc)
    }

    /// `target`'s registrations, made if it has none.
    fn ensure_registrations(&self, target: NodeRef<'gc>) -> Gc<'gc, HeapVector<Registration>> {
        target.flags.set(target.flags.get() | HAS_OBSERVERS);
        let rare = self
            .node_rare(target, true)
            .expect("every node has rare data");
        let edge = &rare.into_ref().observers;
        match edge.get(self.gc) {
            Some(registrations) => registrations,
            None => {
                let registrations = HeapVector::new(self.gc, 1);
                edge.set(self.gc, Some(registrations));
                registrations
            }
        }
    }

    fn new_registration(
        &self,
        observer: Gc<'gc, MutationObserver>,
        options: u8,
        filter: Option<Box<[NameId]>>,
        source: Option<Gc<'gc, Registration>>,
    ) -> Gc<'gc, Registration> {
        self.gc.alloc_in(
            &sites::OTHER,
            RegistrationInit {
                observer: Some(observer),
                options: Cell::new(options),
                filter: LocalExternal::new(filter),
                source,
            },
        )
    }

    /// `observe(target, options)`, its options checked (`dom_api`):
    /// `observer`'s registration on `target` replaced (its transient ones
    /// gone), or a new one; `filter` the attributes' local names.
    pub fn observe(
        &self,
        observer: Gc<'gc, MutationObserver>,
        target: NodeRef<'gc>,
        options: ObserveOptions,
        filter: Option<&[&str]>,
    ) {
        self.changes.mutations.observed.set(true);
        let filter = filter.map(|names| names.iter().map(|name| names::intern(name)).collect());
        let registrations = self.ensure_registrations(target);
        let registration = self.new_registration(observer, options.bits(), filter, None);
        let existing = (0..registrations.len()).find(|&index| {
            registrations.get(self.gc, index).is_some_and(|it| {
                it.source.get(self.gc).is_none() && Gc::ptr_eq(self.observer_of(it), observer)
            })
        });
        match existing {
            Some(index) => {
                let old = registrations.get(self.gc, index).expect("found");
                self.remove_transients_from(observer, |source| Gc::ptr_eq(source, old));
                registrations.set(self.gc, index, registration);
            }
            None => {
                registrations.push(self.gc, registration);
                let id = self.node_id(target);
                let mut nodes = observer.nodes.get().take();
                nodes.push(id);
                observer.nodes.get().set(nodes);
            }
        }
    }

    fn observer_of(&self, registration: Gc<'gc, Registration>) -> Gc<'gc, MutationObserver> {
        registration
            .observer
            .get(self.gc)
            .expect("a registration's observer")
    }

    /// `disconnect()`: `observer`'s registrations gone from every node, its
    /// transient ones too, and its records.
    pub fn disconnect(&self, observer: Gc<'gc, MutationObserver>) {
        let nodes = observer.nodes.get().take();
        for id in nodes {
            if let Some(node) = self.node_by_id(id) {
                self.unregister(node, |registration| {
                    Gc::ptr_eq(self.observer_of(registration), observer)
                });
            }
        }
        self.remove_transients_from(observer, |_| true);
        if let Some(records) = observer.records.get(self.gc) {
            records.clear(self.gc);
        }
    }

    /// The registrations of `node` that `remove` says to removed.
    fn unregister(&self, node: NodeRef<'gc>, remove: impl Fn(Gc<'gc, Registration>) -> bool) {
        let Some(registrations) = self.registrations(node) else {
            return;
        };
        let mut index = 0;
        while let Some(registration) = registrations.get(self.gc, index) {
            if remove(registration) {
                registrations.remove(self.gc, index);
            } else {
                index = index.saturating_add(1);
            }
        }
        if registrations.is_empty() {
            node.flags.set(node.flags.get() & !HAS_OBSERVERS);
        }
    }

    /// `observer`'s transient registrations whose source `from` takes,
    /// gone (all of them for a delivery).
    fn remove_transients_from(
        &self,
        observer: Gc<'gc, MutationObserver>,
        from: impl Fn(Gc<'gc, Registration>) -> bool,
    ) {
        let nodes = observer.transient_nodes.get().take();
        let mut kept = Vec::new();
        for id in nodes {
            let Some(node) = self.node_by_id(id) else {
                continue;
            };
            self.unregister(node, |registration| {
                registration.source.get(self.gc).is_some_and(|source| {
                    Gc::ptr_eq(self.observer_of(registration), observer) && from(source)
                })
            });
            let still = self.registrations(node).is_some_and(|registrations| {
                (0..registrations.len()).any(|index| {
                    registrations
                        .get(self.gc, index)
                        .is_some_and(|registration| {
                            registration.source.get(self.gc).is_some()
                                && Gc::ptr_eq(self.observer_of(registration), observer)
                        })
                })
            });
            if still {
                kept.push(id);
            }
        }
        observer.transient_nodes.get().set(kept);
    }

    /// `takeRecords()`: `observer`'s records, its queue emptied.
    pub fn take_records(
        &self,
        observer: Gc<'gc, MutationObserver>,
    ) -> Vec<Gc<'gc, MutationRecord>> {
        let Some(records) = observer.records.get(self.gc) else {
            return Vec::new();
        };
        let taken = (0..records.len())
            .filter_map(|index| records.get(self.gc, index))
            .collect();
        records.clear(self.gc);
        taken
    }

    /// DOM's "notify mutation observers", its first steps: the microtask
    /// no longer queued, and the observers to deliver to, in the order they
    /// had their first record, each no longer pending.
    pub fn take_pending_observers(&self) -> Vec<Gc<'gc, MutationObserver>> {
        let state = &self.changes.mutations;
        state.queued.set(false);
        let Some(pending) = state.pending.take() else {
            return Vec::new();
        };
        let list = pending.get(self.gc);
        let observers: Vec<_> = (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect();
        list.clear(self.gc);
        state.pending.set(Some(pending));
        for observer in &observers {
            observer.pending.set(false);
        }
        observers
    }

    /// `slot` among the signal slots, once, the microtask queued (DOM's
    /// "signal a slot change").
    pub(crate) fn add_signal_slot(&self, slot: NodeRef<'gc>) {
        let state = &self.changes.mutations;
        let slots = match state.signal_slots.take() {
            Some(slots) => slots,
            None => Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
        };
        let list = slots.get(self.gc);
        if !(0..list.len()).any(|index| {
            list.get(self.gc, index)
                .is_some_and(|at| Gc::ptr_eq(at, slot))
        }) {
            list.push(self.gc, slot);
        }
        state.signal_slots.set(Some(slots));
        if !state.queued.replace(true) {
            self.microtask_to_queue(crate::Microtasks::MUTATIONS);
        }
    }

    /// DOM's "notify mutation observers", its signal slots: taken, in the
    /// order they were signaled.
    pub fn take_signal_slots(&self) -> Vec<NodeRef<'gc>> {
        let state = &self.changes.mutations;
        let Some(slots) = state.signal_slots.take() else {
            return Vec::new();
        };
        let list = slots.get(self.gc);
        let taken = (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect();
        list.clear(self.gc);
        state.signal_slots.set(Some(slots));
        taken
    }

    /// A delivery's step for `observer`: its transient registrations gone.
    pub fn remove_transient_registrations(&self, observer: Gc<'gc, MutationObserver>) {
        self.remove_transients_from(observer, |_| true);
    }

    /// `observer` among the pending ones, if it isn't.
    fn make_pending(&self, observer: Gc<'gc, MutationObserver>) {
        if observer.pending.replace(true) {
            return;
        }
        let state = &self.changes.mutations;
        let pending = match state.pending.take() {
            Some(pending) => pending,
            None => Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
        };
        pending.get(self.gc).push(self.gc, observer);
        state.pending.set(Some(pending));
    }

    /// Whether anything was ever observed in this DOM: what a change
    /// looks at before it does anything for observers.
    #[inline]
    pub(crate) fn observed(&self) -> bool {
        self.changes.mutations.observed.get()
    }

    /// The interested observers of a record of `kind` at `target`: those
    /// registered on it or, with `subtree`, on an ancestor (DOM's "queue a
    /// mutation record", its first steps).
    fn interested(
        &self,
        kind: MutationKind,
        target: NodeRef<'gc>,
        attribute: Option<QualifiedName>,
    ) -> Interested<'gc> {
        let mut interested: Interested<'gc> = Vec::new();
        let mut at = Some(target);
        while let Some(node) = at {
            if let Some(registrations) = self.registrations(node) {
                for index in 0..registrations.len() {
                    let Some(registration) = registrations.get(self.gc, index) else {
                        continue;
                    };
                    let options = registration.options.get();
                    if !Gc::ptr_eq(node, target) && options & SUBTREE == 0 {
                        continue;
                    }
                    let (wanted, old) = match kind {
                        MutationKind::Attributes => {
                            let filtered =
                                registration.filter.get().as_ref().is_some_and(|filter| {
                                    attribute.is_none_or(|name| {
                                        name.namespace() != names::NONE
                                            || !filter.contains(&name.local())
                                    })
                                });
                            (
                                options & ATTRIBUTES != 0 && !filtered,
                                options & ATTRIBUTE_OLD_VALUE != 0,
                            )
                        }
                        MutationKind::CharacterData => (
                            options & CHARACTER_DATA != 0,
                            options & CHARACTER_DATA_OLD_VALUE != 0,
                        ),
                        MutationKind::ChildList => (options & CHILD_LIST != 0, false),
                    };
                    if !wanted {
                        continue;
                    }
                    let observer = self.observer_of(registration);
                    match interested
                        .iter_mut()
                        .find(|(it, _)| Gc::ptr_eq(*it, observer))
                    {
                        Some((_, with_old)) => *with_old |= old,
                        None => interested.push((observer, old)),
                    }
                }
            }
            at = self.parent(node);
        }
        interested
    }

    /// DOM's "queue a mutation record" of `kind` at `target`, for its
    /// interested observers; the microtask queued if it isn't (the engine
    /// does: `take_mutation_microtask`).
    #[expect(clippy::too_many_arguments, reason = "DOM's record's fields")]
    fn queue_record(
        &self,
        kind: MutationKind,
        target: NodeRef<'gc>,
        attribute: Option<QualifiedName>,
        old_value: Option<&str>,
        added: &[NodeRef<'gc>],
        removed: &[NodeRef<'gc>],
        previous: Option<NodeRef<'gc>>,
        next: Option<NodeRef<'gc>>,
    ) {
        let interested = self.interested(kind, target, attribute);
        if interested.is_empty() {
            return;
        }
        let (added, removed) = (self.static_node_list(added), self.static_node_list(removed));
        let old_value = old_value.map(DomString::new);
        for (observer, with_old) in interested {
            let record = self.gc.alloc_in(
                &sites::OTHER,
                MutationRecordInit {
                    kind: Cell::new(kind as u8),
                    target: Some(target),
                    added: Some(added),
                    removed: Some(removed),
                    previous,
                    next,
                    name: Cell::new(attribute.map_or(names::NONE, QualifiedName::local)),
                    namespace: Cell::new(attribute.map_or(names::NONE, QualifiedName::namespace)),
                    old_value: LocalExternal::new(if with_old { old_value.clone() } else { None }),
                    wrapper: (),
                },
            );
            let records = match observer.records.get(self.gc) {
                Some(records) => records,
                None => {
                    let records = HeapVector::new(self.gc, 1);
                    observer.records.set(self.gc, Some(records));
                    records
                }
            };
            records.push(self.gc, record);
            self.make_pending(observer);
        }
        if !self.changes.mutations.queued.replace(true) {
            self.microtask_to_queue(crate::Microtasks::MUTATIONS);
        }
    }

    /// A `childList` record at `parent`.
    #[cold]
    #[inline(never)]
    pub(crate) fn queue_tree_record(
        &self,
        parent: NodeRef<'gc>,
        added: &[NodeRef<'gc>],
        removed: &[NodeRef<'gc>],
        previous: Option<NodeRef<'gc>>,
        next: Option<NodeRef<'gc>>,
    ) {
        if added.is_empty() && removed.is_empty() {
            return;
        }
        self.queue_record(
            MutationKind::ChildList,
            parent,
            None,
            None,
            added,
            removed,
            previous,
            next,
        );
    }

    /// An `attributes` record at `element`.
    #[cold]
    #[inline(never)]
    pub(crate) fn queue_attribute_record(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        old: Option<&str>,
    ) {
        self.queue_record(
            MutationKind::Attributes,
            element,
            Some(name),
            old,
            &[],
            &[],
            None,
            None,
        );
    }

    /// A `characterData` record at `node`.
    #[cold]
    #[inline(never)]
    pub(crate) fn queue_character_data_record(&self, node: NodeRef<'gc>, old: &str) {
        self.queue_record(
            MutationKind::CharacterData,
            node,
            None,
            Some(old),
            &[],
            &[],
            None,
            None,
        );
    }

    /// `node`, being removed from `parent`, gets a transient registration
    /// for each registration with `subtree` on `parent` and its ancestors
    /// (DOM's "remove", its observers' step).
    #[cold]
    #[inline(never)]
    pub(crate) fn add_transient_registrations(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        let mut at = Some(parent);
        while let Some(ancestor) = at {
            if let Some(registrations) = self.registrations(ancestor) {
                for index in 0..registrations.len() {
                    let Some(registration) = registrations.get(self.gc, index) else {
                        continue;
                    };
                    let options = registration.options.get();
                    if options & SUBTREE == 0 {
                        continue;
                    }
                    let observer = self.observer_of(registration);
                    let filter = registration.filter.get().clone();
                    let transient =
                        self.new_registration(observer, options, filter, Some(registration));
                    self.ensure_registrations(node).push(self.gc, transient);
                    let id = self.node_id(node);
                    let mut nodes = observer.transient_nodes.get().take();
                    if !nodes.contains(&id) {
                        nodes.push(id);
                    }
                    observer.transient_nodes.get().set(nodes);
                    self.make_pending(observer);
                }
            }
            at = self.parent(ancestor);
        }
    }

    pub fn record_kind(&self, record: Gc<'gc, MutationRecord>) -> MutationKind {
        match record.kind.get() {
            0 => MutationKind::ChildList,
            1 => MutationKind::Attributes,
            _ => MutationKind::CharacterData,
        }
    }

    pub fn record_target(&self, record: Gc<'gc, MutationRecord>) -> NodeRef<'gc> {
        record.target.get(self.gc).expect("a record's target")
    }

    pub fn record_added(&self, record: Gc<'gc, MutationRecord>) -> Gc<'gc, NodeList> {
        record.added.get(self.gc).expect("a record's added nodes")
    }

    pub fn record_removed(&self, record: Gc<'gc, MutationRecord>) -> Gc<'gc, NodeList> {
        record
            .removed
            .get(self.gc)
            .expect("a record's removed nodes")
    }

    pub fn record_previous_sibling(&self, record: Gc<'gc, MutationRecord>) -> Option<NodeRef<'gc>> {
        record.previous.get(self.gc)
    }

    pub fn record_next_sibling(&self, record: Gc<'gc, MutationRecord>) -> Option<NodeRef<'gc>> {
        record.next.get(self.gc)
    }

    /// An attribute's local name and namespace: none for other records
    /// (and for the null namespace).
    pub fn record_attribute(
        &self,
        record: Gc<'gc, MutationRecord>,
    ) -> (Option<NameId>, Option<NameId>) {
        let some = |id| (id != names::NONE).then_some(id);
        (some(record.name.get()), some(record.namespace.get()))
    }

    pub fn record_old_value(&self, record: Gc<'gc, MutationRecord>) -> Option<DomString> {
        record.old_value.get().clone()
    }
}
