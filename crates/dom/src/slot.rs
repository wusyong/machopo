//! Slots (DOM's "slottables" and "slots"; Blink's `SlotAssignment`): which
//! of a host's children each of its shadow tree's slots gets. A shadow
//! root keeps its slots in tree order and two stale bits (its slots, the
//! assignment); a slot (an HTML `<slot>` in a shadow tree) its assigned
//! nodes, in a `SlotData`, and for the manual mode the nodes `assign()`
//! gave it; a slottable (an element or a text, a host's child) its slot in
//! its rare data, and its manual one. A change of the host's children, of
//! a slot or of a `slot` or `name` attribute marks the shadow root's
//! assignment stale, worked out again as it is next needed
//! (`recalc_slot_assignment`, in an operation that allocates).
//!
//! DOM's "signal a slot change" queues `slotchange` at the slots whose
//! assigned nodes change, with the mutation observers' microtask: a host's
//! child coming, going or renamed signals the slot its name finds (and the
//! one it had), as Blink's `SlotAssignment::DidAddSlotInternal` and the
//! others do, without working the whole assignment out; a slot coming,
//! going or renamed (rarer) works its shadow root's out at once and
//! signals the slots whose nodes changed.

use elise::{Gc, GcObject, HeapVector, Member};

use crate::shadow::{IN_SHADOW_TREE, SHADOW_HOST, SHADOW_ROOT};
use crate::{Cx, Node, NodeKind, NodeRef, QualifiedName, SlotAssignmentMode, names, sites};

/// The attributes that name slottables and slots.
pub(crate) const SLOT_ATTRIBUTE: QualifiedName = QualifiedName::plain(names::local::SLOT);
pub(crate) const NAME_ATTRIBUTE: QualifiedName = QualifiedName::plain(names::NAME);

/// `Node::scope`'s bits under which a tree change can change slots.
pub(crate) const SLOT_SCOPES: u8 = IN_SHADOW_TREE | SHADOW_ROOT | SHADOW_HOST;

/// A slot's: its assigned nodes (DOM's "assigned nodes"), as last worked
/// out, and the nodes `assign()` gave it (its "manually assigned nodes").
#[derive(GcObject)]
pub struct SlotData {
    assigned: Member<HeapVector<Node>>,
    manual: Member<HeapVector<Node>>,
}

/// `ShadowRootData::slot_state`: its slots' list, its assignment, out of
/// date.
pub(crate) const SLOTS_STALE: u8 = 1 << 0;
pub(crate) const ASSIGNMENT_STALE: u8 = 1 << 1;
/// It is in the DOM's list of shadow roots whose assignment is worked out
/// before style (`recalc_slot_assignments`).
const LISTED: u8 = 1 << 2;

impl<'gc> Cx<'_, 'gc> {
    /// Whether `node` is a slot: an HTML `<slot>` in a shadow tree.
    pub fn is_slot(&self, node: NodeRef<'gc>) -> bool {
        self.containing_shadow_root(node).is_some() && self.is_slot_element(node)
    }

    fn is_slot_element(&self, node: NodeRef<'gc>) -> bool {
        self.element_name(node).is_some_and(|name| {
            name.namespace == names::HTML_NAMESPACE && name.local == names::local::SLOT
        })
    }

    /// A slottable's name for named slots: its `slot` attribute's value,
    /// empty for a text.
    fn slottable_name(&self, slottable: NodeRef<'gc>) -> crate::DomString {
        match self.attribute_value_by_id(slottable, names::local::SLOT) {
            Some(value) => value.to_owned(),
            None => crate::DomString::new(""),
        }
    }

    /// A slot's name: its `name` attribute's value, empty without one.
    fn slot_name(&self, slot: NodeRef<'gc>) -> crate::DomString {
        match self.attribute_value_by_id(slot, names::NAME) {
            Some(value) => value.to_owned(),
            None => crate::DomString::new(""),
        }
    }

    fn slot_data(&self, slot: NodeRef<'gc>, make: bool) -> Option<Gc<'gc, SlotData>> {
        let data = if make {
            self.element_data(slot)
        } else {
            self.existing_element_data(slot)?
        };
        let edge = &data.into_ref().slot;
        if let Some(slot_data) = edge.get(self.gc) {
            return Some(slot_data);
        }
        if !make {
            return None;
        }
        let slot_data = self.gc.alloc_in(
            &sites::OTHER,
            SlotDataInit {
                assigned: None,
                manual: None,
            },
        );
        edge.set(self.gc, Some(slot_data));
        Some(slot_data)
    }

    /// `root`'s slots in tree order, its list made again if stale.
    fn slots_of(&self, root: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let data = self.shadow_data(root).expect("a shadow root's data");
        let data = data.into_ref();
        if data.slot_state.get() & SLOTS_STALE != 0 || data.slots.get(self.gc).is_none() {
            let list = HeapVector::new(self.gc, 0);
            self.for_each_inclusive_descendant(root, |at| {
                if self.is_slot(at) {
                    list.push(self.gc, at);
                }
            });
            data.slots.set(self.gc, Some(list));
            data.slot_state.set(data.slot_state.get() & !SLOTS_STALE);
        }
        let list = data.slots.get(self.gc).expect("just made");
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect()
    }

    /// DOM's "find a slot" for `slottable` in `root`, `root`'s slots
    /// given (none: found by walking, which allocates nothing).
    fn find_slot_in(
        &self,
        root: NodeRef<'gc>,
        slottable: NodeRef<'gc>,
        slots: Option<&[NodeRef<'gc>]>,
    ) -> Option<NodeRef<'gc>> {
        let init = self.shadow_root_init(root)?;
        if init.slot_assignment == SlotAssignmentMode::Manual {
            let slot = self.node_rare(slottable, false)?.manual_slot.get(self.gc)?;
            return self
                .containing_shadow_root(slot)
                .filter(|&scope| Gc::ptr_eq(scope, root) && self.is_slot_element(slot))
                .map(|_| slot);
        }
        self.find_slot_named(root, &self.slottable_name(slottable).to_str(), slots)
    }

    /// The first of `root`'s slots named `name` (named mode).
    fn find_slot_named(
        &self,
        root: NodeRef<'gc>,
        name: &str,
        slots: Option<&[NodeRef<'gc>]>,
    ) -> Option<NodeRef<'gc>> {
        let matches = |slot: NodeRef<'gc>| *self.slot_name(slot).to_str() == *name;
        match slots {
            Some(slots) => slots.iter().copied().find(|&slot| matches(slot)),
            None => {
                let mut found = None;
                self.for_each_inclusive_descendant(root, |at| {
                    if found.is_none() && self.is_slot(at) && matches(at) {
                        found = Some(at);
                    }
                });
                found
            }
        }
    }

    /// Whether `node` can be slotted: an element or a text.
    fn is_slottable(&self, node: NodeRef<'gc>) -> bool {
        matches!(self.kind(node), NodeKind::Element | NodeKind::Text)
    }

    /// `slottable`'s assigned slot (DOM's): its host's shadow root's
    /// assignment's, read as worked out, or found if that is stale (an
    /// attribute's getter's: nothing allocated).
    pub fn assigned_slot(&self, slottable: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let host = self.parent(slottable)?;
        if host.scope.get() & SHADOW_HOST == 0 || !self.is_slottable(slottable) {
            return None;
        }
        let root = self.shadow_root(host)?;
        let data = self.shadow_data(root)?.into_ref();
        if data.slot_state.get() & ASSIGNMENT_STALE != 0 {
            return self.find_slot_in(root, slottable, None);
        }
        self.node_rare(slottable, false)?.assigned_slot.get(self.gc)
    }

    /// `slot`'s assigned nodes, `root`'s assignment worked out first if
    /// stale.
    pub fn assigned_nodes(&self, slot: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        if !self.is_slot(slot) {
            return Vec::new();
        }
        let root = self
            .containing_shadow_root(slot)
            .expect("a slot is in a shadow tree");
        self.recalc_slot_assignment(root);
        self.assigned_list(slot)
    }

    /// DOM's "find flattened slottables" of `slot`: its assigned nodes, or
    /// with none its slottable children (its fallback content), each slot
    /// among them replaced by its own.
    pub fn flattened_assigned_nodes(&self, slot: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let mut out = Vec::new();
        if self.is_slot(slot) {
            self.flatten_into(slot, &mut out);
        }
        out
    }

    fn flatten_into(&self, slot: NodeRef<'gc>, out: &mut Vec<NodeRef<'gc>>) {
        let mut nodes = self.assigned_nodes(slot);
        if nodes.is_empty() {
            nodes = self
                .children(slot)
                .into_iter()
                .filter(|&at| self.is_slottable(at))
                .collect();
        }
        for node in nodes {
            if self.is_slot(node) {
                self.flatten_into(node, out);
            } else {
                out.push(node);
            }
        }
    }

    fn assigned_list(&self, slot: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let Some(list) = self
            .slot_data(slot, false)
            .and_then(|data| data.assigned.get(self.gc))
        else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect()
    }

    /// `root`'s assignment worked out again if stale (Blink's
    /// `SlotAssignment::RecalcAssignment`): each slot's assigned nodes, in
    /// the host's children's order (named) or its manually assigned
    /// nodes' (manual), and each slottable's slot.
    pub fn recalc_slot_assignment(&self, root: NodeRef<'gc>) {
        let Some(data) = self.shadow_data(root) else {
            return;
        };
        if data.slot_state.get() & ASSIGNMENT_STALE == 0 {
            return;
        }
        self.assign_slottables(root);
        data.slot_state
            .set(data.slot_state.get() & !ASSIGNMENT_STALE);
    }

    /// The assignment of `root` worked out: every slot's list emptied and
    /// filled, and the slot of each child of the host.
    fn assign_slottables(&self, root: NodeRef<'gc>) {
        let host = self.shadow_host(root).expect("a shadow root's host");
        let slots = self.slots_of(root);
        let mut before = Vec::new();
        let mut child = self.first_child(host);
        while let Some(at) = child {
            let rare = self.node_rare(at, false);
            before.push((at, rare.and_then(|rare| rare.assigned_slot.get(self.gc))));
            if let Some(rare) = rare {
                rare.assigned_slot.set(self.gc, None);
            }
            child = self.next_sibling(at);
        }
        for (slot, nodes) in slots.iter().zip(self.assignment(root, &slots, None)) {
            for &node in &nodes {
                self.node_rare(node, true)
                    .expect("a slottable has rare data")
                    .assigned_slot
                    .set(self.gc, Some(*slot));
            }
            self.set_assigned(*slot, &nodes);
        }
        // An element that moved to another slot, or out of its slot (style
        // styles one assigned to none as the host's child), inherits from
        // elsewhere now: styled again, with what is below it. The slots'
        // own styles don't change.
        for (node, old) in before {
            let new = self.assigned_slot(node);
            let moved = match (old, new) {
                (Some(old), Some(new)) => !Gc::ptr_eq(old, new),
                (None, None) => false,
                _ => true,
            };
            if moved && self.kind(node) == NodeKind::Element {
                self.set_needs_style_recalc(node);
            }
        }
    }

    /// DOM's "find slottables" of each of `slots` (`root`'s, or what they
    /// were), its host's children now: in the host's children's order
    /// (named: the first of the slots of a child's name) or the order
    /// `assign()` gave them (manual). `renamed` a slot with the name it
    /// had (what the assignment was before it was renamed).
    fn assignment(
        &self,
        root: NodeRef<'gc>,
        slots: &[NodeRef<'gc>],
        renamed: Option<(NodeRef<'gc>, &str)>,
    ) -> Vec<Vec<NodeRef<'gc>>> {
        let host = self.shadow_host(root).expect("a shadow root's host");
        let manual = self
            .shadow_root_init(root)
            .is_some_and(|init| init.slot_assignment == SlotAssignmentMode::Manual);
        if manual {
            return slots
                .iter()
                .map(|&slot| {
                    self.manually_assigned(slot)
                        .into_iter()
                        .filter(|&node| {
                            self.parent(node)
                                .is_some_and(|parent| Gc::ptr_eq(parent, host))
                        })
                        .collect()
                })
                .collect();
        }
        let names: Vec<crate::DomString> = slots
            .iter()
            .map(|&slot| match renamed {
                Some((renamed, name)) if Gc::ptr_eq(renamed, slot) => crate::DomString::new(name),
                _ => self.slot_name(slot),
            })
            .collect();
        let mut lists = vec![Vec::new(); slots.len()];
        let mut child = self.first_child(host);
        while let Some(at) = child {
            if self.is_slottable(at) {
                let name = self.slottable_name(at);
                if let Some(index) = names
                    .iter()
                    .position(|slot| *slot.to_str() == *name.to_str())
                {
                    lists[index].push(at);
                }
            }
            child = self.next_sibling(at);
        }
        lists
    }

    fn set_assigned(&self, slot: NodeRef<'gc>, nodes: &[NodeRef<'gc>]) {
        if nodes.is_empty() {
            if let Some(list) = self
                .slot_data(slot, false)
                .and_then(|data| data.assigned.get(self.gc))
            {
                list.clear(self.gc);
            }
            return;
        }
        let data = self.slot_data(slot, true).expect("made");
        let list = match data.assigned.get(self.gc) {
            Some(list) => {
                list.clear(self.gc);
                list
            }
            None => {
                let list = HeapVector::new(self.gc, nodes.len());
                data.assigned.set(self.gc, Some(list));
                list
            }
        };
        for &node in nodes {
            list.push(self.gc, node);
        }
    }

    /// The nodes `assign()` last gave `slot`.
    pub fn manually_assigned(&self, slot: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let Some(list) = self
            .slot_data(slot, false)
            .and_then(|data| data.manual.get(self.gc))
        else {
            return Vec::new();
        };
        (0..list.len())
            .filter_map(|index| list.get(self.gc, index))
            .collect()
    }

    /// `assign(...nodes)` (HTML's): each node's manual slot is `slot` from
    /// now on (no longer another's), `slot`'s manually assigned nodes
    /// `nodes` (once each, in order); the slots whose assigned nodes that
    /// changes signaled.
    pub fn assign(&self, slot: NodeRef<'gc>, nodes: &[NodeRef<'gc>]) {
        let mut unique: Vec<NodeRef<'gc>> = Vec::with_capacity(nodes.len());
        for &node in nodes {
            if !unique.iter().any(|&at| Gc::ptr_eq(at, node)) {
                unique.push(node);
            }
        }
        // The shadow trees whose slots may change: `slot`'s and those of
        // the slots `nodes` leave; what their slots had, before.
        let mut roots: Vec<NodeRef<'gc>> = Vec::new();
        let previous = unique
            .iter()
            .filter_map(|&node| self.node_rare(node, false)?.manual_slot.get(self.gc));
        for at in std::iter::once(slot).chain(previous) {
            if let Some(root) = self.containing_shadow_root(at)
                && !roots.iter().any(|&known| Gc::ptr_eq(known, root))
            {
                roots.push(root);
            }
        }
        let before: Vec<_> = roots
            .iter()
            .map(|&root| {
                let slots = self.slots_of(root);
                let had = self.assignment(root, &slots, None);
                (slots, had)
            })
            .collect();
        for &old in &self.manually_assigned(slot) {
            if let Some(rare) = self.node_rare(old, false) {
                rare.manual_slot.set(self.gc, None);
            }
        }
        let list = HeapVector::new(self.gc, unique.len());
        for &node in &unique {
            let rare = self
                .node_rare(node, true)
                .expect("a slottable has rare data");
            if let Some(previous) = rare.manual_slot.get(self.gc)
                && !Gc::ptr_eq(previous, slot)
            {
                self.remove_manual(previous, node);
            }
            rare.manual_slot.set(self.gc, Some(slot));
            list.push(self.gc, node);
        }
        self.slot_data(slot, true)
            .expect("made")
            .manual
            .set(self.gc, Some(list));
        for (root, (slots, had)) in roots.into_iter().zip(before) {
            let data = self.shadow_data(root).expect("a shadow root's data");
            data.slot_state
                .set(data.slot_state.get() | ASSIGNMENT_STALE);
            self.recalc_slot_assignment(root);
            for (&slot, had) in slots.iter().zip(&had) {
                if !same_nodes(had, &self.assigned_list(slot)) {
                    self.signal_slot_change(slot);
                }
            }
        }
    }

    fn remove_manual(&self, slot: NodeRef<'gc>, node: NodeRef<'gc>) {
        let Some(list) = self
            .slot_data(slot, false)
            .and_then(|data| data.manual.get(self.gc))
        else {
            return;
        };
        if let Some(index) = (0..list.len()).find(|&index| {
            list.get(self.gc, index)
                .is_some_and(|at| Gc::ptr_eq(at, node))
        }) {
            list.remove(self.gc, index);
        }
    }

    /// DOM's "signal a slot change": `slot` among the signal slots (once),
    /// the mutation observers' microtask queued.
    pub(crate) fn signal_slot_change(&self, slot: NodeRef<'gc>) {
        self.add_signal_slot(slot);
        self.flat_tree_children_changed(slot);
    }

    /// `root`'s slots changed (one came, went or was `renamed` from a
    /// name): its assignment worked out at once, the slots whose assigned
    /// nodes changed signaled (DOM's "assign slottables for a tree"),
    /// those no longer its with nodes too. What each had is worked out
    /// again from the slots' list as it was (kept up to date by each such
    /// change: the slots' list is only made again here) and the host's
    /// children now (a change of them in the same operation isn't one of
    /// a slot's), not from the assigned lists, which may be stale.
    pub(crate) fn slots_changed(&self, root: NodeRef<'gc>, renamed: Option<(NodeRef<'gc>, &str)>) {
        let Some(data) = self.shadow_data(root) else {
            return;
        };
        let old_slots: Vec<NodeRef<'gc>> = match data.slots.get(self.gc) {
            Some(list) if data.slot_state.get() & SLOTS_STALE == 0 => (0..list.len())
                .filter_map(|index| list.get(self.gc, index))
                .collect(),
            _ => Vec::new(),
        };
        let before = self.assignment(root, &old_slots, renamed);
        data.slot_state
            .set(data.slot_state.get() | SLOTS_STALE | ASSIGNMENT_STALE);
        self.recalc_slot_assignment(root);
        let slots = self.slots_of(root);
        for &slot in &slots {
            let had = old_slots
                .iter()
                .position(|&old| Gc::ptr_eq(old, slot))
                .map_or(&[][..], |index| before[index].as_slice());
            if !same_nodes(had, &self.assigned_list(slot)) {
                self.signal_slot_change(slot);
            }
        }
        // A slot that left with nodes: none now.
        for (&slot, had) in old_slots.iter().zip(&before) {
            if !slots.iter().any(|&now| Gc::ptr_eq(now, slot)) {
                self.set_assigned(slot, &[]);
                if !had.is_empty() {
                    self.signal_slot_change(slot);
                }
            }
        }
    }

    /// A child of `host` (a slottable) came (`inserted`) or went: the slot
    /// its name finds signaled (DOM's "assign a slot" on insertion, named
    /// mode only; "assign slottables" for its assigned slot on removal);
    /// the assignment stale (worked out when next needed).
    fn slottable_changed(&self, host: NodeRef<'gc>, slottable: NodeRef<'gc>, inserted: bool) {
        let Some(root) = self.shadow_root(host) else {
            return;
        };
        if !self.is_slottable(slottable) {
            return;
        }
        let manual = self
            .shadow_root_init(root)
            .is_some_and(|init| init.slot_assignment == SlotAssignmentMode::Manual);
        let slots = self.slots_of(root);
        if let Some(slot) = self.find_slot_in(root, slottable, Some(&slots)) {
            if inserted && manual {
                // DOM signals none, but the flat tree changed.
                self.flat_tree_children_changed(slot);
            } else {
                self.signal_slot_change(slot);
            }
        }
        self.assignment_stale(root);
    }

    /// `node` put in `parent`'s children, `parent` a host or in a shadow
    /// tree (`SLOT_SCOPES`): DOM's insert's slot steps.
    #[cold]
    pub(crate) fn slots_inserted(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if parent.scope.get() & SHADOW_HOST != 0 {
            self.slottable_changed(parent, node, true);
        }
        self.slot_tree_changed(parent, node);
    }

    /// `node` taken from `parent`'s children, `parent` as for
    /// `slots_inserted`: DOM's remove's slot steps.
    #[cold]
    pub(crate) fn slots_removed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if parent.scope.get() & SHADOW_HOST != 0 {
            self.slottable_changed(parent, node, false);
        }
        self.slot_tree_changed(parent, node);
    }

    /// Insert's and remove's steps for a shadow tree: a slot's fallback
    /// content changing signals it (while it has no assigned nodes); a
    /// slot coming or going works the slots out again.
    fn slot_tree_changed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        let Some(root) = self.shadow_scope(parent) else {
            return;
        };
        if self.is_slot(parent) && self.assigned_nodes(parent).is_empty() {
            self.signal_slot_change(parent);
        }
        let mut has_slot = false;
        self.for_each_inclusive_descendant(node, |at| has_slot |= self.is_slot_element(at));
        if has_slot {
            self.slots_changed(root, None);
        }
    }

    /// `element`'s attribute `name` (`SLOT_ATTRIBUTE` or `NAME_ATTRIBUTE`)
    /// changed from `old` to `new`: a host's child's `slot` signals the
    /// slots its old and new names find (DOM's attribute change steps for
    /// slottables), a slot's `name` works its shadow root's slots out
    /// again (for slots).
    #[cold]
    pub(crate) fn slot_name_changed(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        if old.unwrap_or("") == new.unwrap_or("") {
            return;
        }
        if name == NAME_ATTRIBUTE {
            if self.is_slot(element) {
                let root = self
                    .containing_shadow_root(element)
                    .expect("a slot is in a shadow tree");
                self.slots_changed(root, Some((element, old.unwrap_or(""))));
            }
            return;
        }
        let Some(host) = self
            .parent(element)
            .filter(|host| host.scope.get() & SHADOW_HOST != 0)
        else {
            return;
        };
        let root = self.shadow_root(host).expect("a host's shadow root");
        if self
            .shadow_root_init(root)
            .is_some_and(|init| init.slot_assignment == SlotAssignmentMode::Manual)
        {
            return;
        }
        let slots = self.slots_of(root);
        for name in [old, new] {
            if let Some(slot) = self.find_slot_named(root, name.unwrap_or(""), Some(&slots)) {
                self.signal_slot_change(slot);
            }
        }
        self.assignment_stale(root);
    }

    /// `root`'s assignment out of date: worked out again when next asked
    /// for, before style at the latest (it goes in the list once).
    pub(crate) fn assignment_stale(&self, root: NodeRef<'gc>) {
        let data = self.shadow_data(root).expect("a shadow root's data");
        let state = data.slot_state.get();
        data.slot_state.set(state | ASSIGNMENT_STALE | LISTED);
        if state & LISTED == 0 {
            let roots = &self.changes.stale_slot_roots;
            let list = match roots.take() {
                Some(list) => list,
                None => elise::Persistent::new(self.gc, HeapVector::new(self.gc, 1)),
            };
            list.get(self.gc).push(self.gc, root);
            roots.set(Some(list));
        }
    }

    /// Every stale assignment worked out (Blink's
    /// `SlotAssignmentEngine::RecalcSlotAssignments`): what style and
    /// layout read of the flat tree, before them.
    pub fn recalc_slot_assignments(&self) {
        let Some(list) = self.changes.stale_slot_roots.take() else {
            return;
        };
        let roots = list.get(self.gc);
        let taken: Vec<NodeRef<'gc>> = (0..roots.len())
            .filter_map(|index| roots.get(self.gc, index))
            .collect();
        roots.clear(self.gc);
        self.changes.stale_slot_roots.set(Some(list));
        for root in taken {
            if let Some(data) = self.shadow_data(root) {
                data.slot_state.set(data.slot_state.get() & !LISTED);
            }
            self.recalc_slot_assignment(root);
        }
    }

    /// `slot`'s assigned nodes' list, as last worked out.
    pub(crate) fn assigned_vector(&self, slot: NodeRef<'gc>) -> Option<Gc<'gc, HeapVector<Node>>> {
        self.slot_data(slot, false)?.assigned.get(self.gc)
    }
}

fn same_nodes<'gc>(a: &[NodeRef<'gc>], b: &[NodeRef<'gc>]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| Gc::ptr_eq(*x, *y))
}
