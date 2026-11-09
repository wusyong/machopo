//! A document's elements by id (Blink's `TreeScope`'s
//! `DocumentOrderedMap`): for each id an element of the document's tree
//! has, how many have it and the first in tree order if known. Kept as
//! elements with an id come, go and change it, in a connected tree only:
//! `getElementById` finds one with no walk, unless several have the id
//! (then the first is found by a walk and kept). Keyed by the id's string
//! in the thread's values table (an attribute's value is the table's), by
//! its address: an entry lives while an element holds the string. The
//! elements are by their ids (`Cx::node_id`, weak): the table holds none
//! of them.
//!
//! Two more tables the same way, for HTML's named access on the `Window`
//! object (Blink's `Document`'s `window_named_item_map` and `TreeScope`'s
//! frame names): the HTML `embed`, `form`, `img` and `object` elements by
//! their `name`, and the `iframe`s by theirs. Kept once such an element
//! has had a `name` (`form::NAMES_SEEN`): a page without one does no work
//! for them.

use std::cell::Cell;

use rustc_hash::FxHashMap;

use crate::{Cx, NodeId, NodeKind, NodeRef, QualifiedName, names};

thread_local! {
    /// Walks looking for an id's first element, for the tests.
    static ID_WALKS: Cell<u64> = const { Cell::new(0) };
    /// Elements added to and removed from the documents' tables, for the
    /// tests. Counted in debug builds only.
    static TABLE_CHANGES: Cell<u64> = const { Cell::new(0) };
}

/// How many times, on this thread, an element was added to or removed
/// from a document's table of ids or names: what tells an id set again to
/// its value changes none. `None` in release builds. For tests.
pub fn id_table_changes_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| TABLE_CHANGES.get())
}

fn note_table_change() {
    if cfg!(debug_assertions) {
        TABLE_CHANGES.set(TABLE_CHANGES.get().wrapping_add(1));
    }
}

/// How many walks the thread made looking for an id's first element
/// (tests: the table finds most with none).
#[doc(hidden)]
pub fn id_walks() -> u64 {
    ID_WALKS.get()
}

#[derive(Default)]
pub(crate) struct IdTable {
    /// By `Table`.
    tables: [FxHashMap<usize, IdEntry>; 3],
}

/// Which of a document's tables.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Table {
    /// Its elements by id.
    Ids = 0,
    /// Its HTML `embed`, `form`, `img` and `object` elements by `name`.
    Names = 1,
    /// Its `iframe`s by `name`.
    Frames = 2,
}

/// What a window has under a name (`Cx::window_named`).
#[derive(Clone, Copy, Debug)]
pub enum WindowNamed<'gc> {
    None,
    /// The first `iframe` in tree order with the name that has a content
    /// document.
    Frame(NodeRef<'gc>),
    /// The one element with the name as its id or (a named element's) its
    /// `name`.
    One(NodeRef<'gc>),
    /// Several elements: `Cx::window_named_collection`'s.
    Several,
}

struct IdEntry {
    /// The first in tree order of those with the id, if known.
    first: Option<NodeId>,
    count: u32,
}

/// Where a document keeps its table: taken out for a change and put
/// back.
pub(crate) type IdTableCell = Cell<Option<Box<IdTable>>>;

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s id: its `id` attribute's value's address in the values
    /// table, if it has one and it isn't empty.
    fn id_key(&self, element: NodeRef<'gc>) -> Option<usize> {
        self.value_key(element, names::ID)
    }

    /// `element`'s attribute `local`'s value's address in the values
    /// table, if it has one and it isn't empty.
    fn value_key(&self, element: NodeRef<'gc>, local: crate::NameId) -> Option<usize> {
        let value = self.attribute_value_by_id(element, local)?;
        let value = value.borrow();
        (!value.to_str().is_empty()).then(|| value.addr())
    }

    /// Which table `element`'s `name` is kept in, if any: an HTML `embed`,
    /// `form`, `img` or `object`'s, or an `iframe`'s.
    pub(crate) fn name_table(&self, element: NodeRef<'gc>) -> Option<Table> {
        use names::local;
        let name = element.qualified_name();
        if name == QualifiedName::html(local::IFRAME) {
            return Some(Table::Frames);
        }
        (name == QualifiedName::html(local::EMBED)
            || name == QualifiedName::html(local::FORM)
            || name == QualifiedName::html(local::IMG)
            || name == QualifiedName::html(local::OBJECT))
        .then_some(Table::Names)
    }

    fn key_in(&self, element: NodeRef<'gc>, table: Table) -> Option<usize> {
        match table {
            Table::Ids => self.id_key(element),
            _ if self.name_table(element) == Some(table) => {
                self.value_key(element, names::local::NAME)
            }
            _ => None,
        }
    }

    fn with_ids<R>(&self, document: NodeRef<'gc>, f: impl FnOnce(&mut IdTable) -> R) -> Option<R> {
        let cell = self.document_ids(document)?;
        let mut table = cell.take().unwrap_or_default();
        let result = f(&mut table);
        cell.set(Some(table));
        Some(result)
    }

    /// `element`, under `key`, came into `document`'s `table`.
    fn key_added(&self, document: NodeRef<'gc>, element: NodeRef<'gc>, table: Table, key: usize) {
        note_table_change();
        let id = self.node_id(element);
        // A document's data is made as it needs some: here, its first id.
        self.ensure_document_data(document);
        self.with_ids(document, |tables| {
            let entry = tables.tables[table as usize].entry(key).or_insert(IdEntry {
                first: None,
                count: 0,
            });
            // Another with the key: which is first is found when asked.
            entry.first = (entry.count == 0).then_some(id);
            entry.count = entry.count.saturating_add(1);
        });
    }

    /// `element`, under `key`, left `document`'s `table`.
    fn key_removed(&self, document: NodeRef<'gc>, element: NodeRef<'gc>, table: Table, key: usize) {
        note_table_change();
        let id = self.node_id(element);
        self.with_ids(document, |tables| {
            let entries = &mut tables.tables[table as usize];
            let Some(entry) = entries.get_mut(&key) else {
                return;
            };
            entry.count = entry.count.saturating_sub(1);
            if entry.count == 0 {
                entries.remove(&key);
            } else if entry.first == Some(id) {
                entry.first = None;
            }
        });
    }

    /// `element` came into `document`'s tree (or got an id there).
    pub(crate) fn id_added(&self, document: NodeRef<'gc>, element: NodeRef<'gc>) {
        if let Some(key) = self.id_key(element) {
            self.key_added(document, element, Table::Ids, key);
        }
    }

    /// `element`, whose id's key is `key`, left `document`'s tree (or its
    /// id changed from it).
    pub(crate) fn id_removed(&self, document: NodeRef<'gc>, element: NodeRef<'gc>, key: usize) {
        self.key_removed(document, element, Table::Ids, key);
    }

    /// `element`'s id's key, for `id_removed` once it has gone or changed.
    pub(crate) fn id_key_of(&self, element: NodeRef<'gc>) -> Option<usize> {
        self.id_key(element)
    }

    /// `element` came into `document`'s tree: under its `name` if it is
    /// one whose name is kept (`name_table`).
    pub(crate) fn name_added(&self, document: NodeRef<'gc>, element: NodeRef<'gc>) {
        if let Some(table) = self.name_table(element)
            && let Some(key) = self.value_key(element, names::local::NAME)
        {
            self.key_added(document, element, table, key);
        }
    }

    /// `element` leaves `document`'s tree (its attributes still its).
    pub(crate) fn name_removed(&self, document: NodeRef<'gc>, element: NodeRef<'gc>) {
        if let Some(table) = self.name_table(element)
            && let Some(key) = self.value_key(element, names::local::NAME)
        {
            self.key_removed(document, element, table, key);
        }
    }

    /// `element`'s `name` changed from `old` in `document`'s tree. The old
    /// value is still the values table's (the caller holds it).
    pub(crate) fn connected_name_changed(
        &self,
        document: NodeRef<'gc>,
        element: NodeRef<'gc>,
        table: Table,
        old: Option<&str>,
    ) {
        if let Some(old) = old.filter(|old| !old.is_empty())
            && let Some(old) = crate::find_value(old)
        {
            self.key_removed(document, element, table, old.addr());
        }
        if let Some(key) = self.value_key(element, names::local::NAME) {
            self.key_added(document, element, table, key);
        }
    }

    /// The first element in tree order in `root`'s subtree whose id is
    /// `id` (DOM's `getElementById`): a document's from its table, another
    /// root's by a walk.
    pub fn element_by_id(&self, root: NodeRef<'gc>, id: &str) -> Option<NodeRef<'gc>> {
        if id.is_empty() {
            return None;
        }
        // No element has an id no value in the table is.
        let key = crate::find_value(id)?.addr();
        if self.kind(root) != NodeKind::Document || !self.is_connected(root) {
            return self.first_with(root, Table::Ids, key);
        }
        self.first_in_table(root, Table::Ids, key)
    }

    /// The first element in tree order under `key` in `document`'s
    /// `table`: the one known, else found by a walk and kept.
    fn first_in_table(
        &self,
        document: NodeRef<'gc>,
        table: Table,
        key: usize,
    ) -> Option<NodeRef<'gc>> {
        let known = self.with_ids(document, |tables| {
            tables.tables[table as usize]
                .get(&key)
                .map(|entry| entry.first)
        })??;
        if let Some(first) = known
            && let Some(element) = self.node_by_id(first)
        {
            return Some(element);
        }
        let found = self.first_with(document, table, key)?;
        let first = self.node_id(found);
        self.with_ids(document, |tables| {
            if let Some(entry) = tables.tables[table as usize].get_mut(&key) {
                entry.first = Some(first);
            }
        });
        Some(found)
    }

    /// How many of `document`'s elements are under `key` in `table`.
    fn count_in_table(&self, document: NodeRef<'gc>, table: Table, key: usize) -> u32 {
        self.with_ids(document, |tables| {
            tables.tables[table as usize]
                .get(&key)
                .map_or(0, |entry| entry.count)
        })
        .unwrap_or(0)
    }

    fn first_with(&self, root: NodeRef<'gc>, table: Table, key: usize) -> Option<NodeRef<'gc>> {
        ID_WALKS.set(ID_WALKS.get().wrapping_add(1));
        let mut found = None;
        self.for_each_inclusive_descendant(root, |node| {
            if found.is_none()
                && self.kind(node) == NodeKind::Element
                && self.key_in(node, table) == Some(key)
            {
                found = Some(node);
            }
        });
        found
    }

    /// What `document`'s window has under `name` (HTML's named access on
    /// the `Window` object, as Chrome finds it: Blink's `WindowProperties::
    /// AnonymousNamedGetter`), from its tables: the first `iframe` in tree
    /// order with that `name` that has a content document; else the
    /// element with that id (any element's) or `name` (an HTML `embed`,
    /// `form`, `img` or `object`'s), if only one; else whether there are
    /// several. Not in shadow trees; not the empty name.
    pub fn window_named(&self, document: NodeRef<'gc>, name: &str) -> WindowNamed<'gc> {
        if name.is_empty() || self.kind(document) != NodeKind::Document {
            return WindowNamed::None;
        }
        // No element has an id or a name no value in the table is: most
        // globals script looks up and doesn't find end here.
        let Some(value) = crate::find_value(name) else {
            return WindowNamed::None;
        };
        let key = value.addr();
        if let Some(frame) = self.named_frame(document, key) {
            return WindowNamed::Frame(frame);
        }
        let ids = self.count_in_table(document, Table::Ids, key);
        let named = self.count_in_table(document, Table::Names, key);
        let first = |table| self.first_in_table(document, table, key);
        let one =
            |element: Option<NodeRef<'gc>>| element.map_or(WindowNamed::None, WindowNamed::One);
        match (ids, named) {
            (0, 0) => WindowNamed::None,
            (1, 0) => one(first(Table::Ids)),
            (0, 1) => one(first(Table::Names)),
            (1, 1) => match (first(Table::Ids), first(Table::Names)) {
                (Some(a), Some(b)) if elise::Gc::ptr_eq(a, b) => WindowNamed::One(a),
                _ => WindowNamed::Several,
            },
            _ => WindowNamed::Several,
        }
    }

    /// The first `iframe` in tree order in `document` named by `key` that
    /// has a content document.
    fn named_frame(&self, document: NodeRef<'gc>, key: usize) -> Option<NodeRef<'gc>> {
        let known = self.with_ids(document, |tables| {
            tables.tables[Table::Frames as usize]
                .get(&key)
                .map(|entry| entry.first)
        })??;
        // One with the name: it, if it has a document.
        if let Some(first) = known {
            return self
                .node_by_id(first)
                .filter(|iframe| self.content_document(*iframe).is_some());
        }
        // Several: not kept, as which has a document changes unseen.
        let mut found = None;
        self.for_each_inclusive_descendant(document, |node| {
            if found.is_none()
                && self.kind(node) == NodeKind::Element
                && self.key_in(node, Table::Frames) == Some(key)
                && self.content_document(node).is_some()
            {
                found = Some(node);
            }
        });
        found
    }

    /// Whether `element` is one of the window's named objects under the
    /// name `name` names (`window_named_collection`'s).
    pub(crate) fn is_window_named(&self, element: NodeRef<'gc>, name: crate::NameId) -> bool {
        let name = &*names::get(name);
        let is = |local| {
            self.attribute_value_by_id(element, local)
                .is_some_and(|value| *value.borrow().to_str() == *name)
        };
        is(names::ID) || self.name_table(element) == Some(Table::Names) && is(names::local::NAME)
    }

    /// Whether `element` is one of `document.all`'s under `name` (HTML's
    /// "all"-named elements): its `id` is `name`, or it is an HTML `a`,
    /// `button`, `embed`, `form`, `frame`, `frameset`, `iframe`, `img`,
    /// `input`, `map`, `meta`, `object`, `select` or `textarea` whose `name`
    /// is. The empty string names the elements whose `id` or `name` is
    /// empty, as in Chrome (HTML gives none).
    pub(crate) fn is_all_named(&self, element: NodeRef<'gc>, name: crate::NameId) -> bool {
        use names::local;
        const ALL_NAMED_ELEMENTS: [crate::NameId; 14] = [
            local::A,
            local::BUTTON,
            local::EMBED,
            local::FORM,
            local::FRAME,
            local::FRAMESET,
            local::IFRAME,
            local::IMG,
            local::INPUT,
            local::MAP,
            local::META,
            local::OBJECT,
            local::SELECT,
            local::TEXTAREA,
        ];
        let name = &*names::get(name);
        let is = |local| {
            self.attribute_value_by_id(element, local)
                .is_some_and(|value| *value.borrow().to_str() == *name)
        };
        if is(names::ID) {
            return true;
        }
        let qualified = element.qualified_name().ids();
        qualified.namespace == names::HTML_NAMESPACE
            && ALL_NAMED_ELEMENTS.contains(&qualified.local)
            && is(local::NAME)
    }

    /// How many entries `document`'s id table has (tests).
    #[doc(hidden)]
    pub fn id_table_len(&self, document: NodeRef<'gc>) -> usize {
        self.with_ids(document, |tables| tables.tables[Table::Ids as usize].len())
            .unwrap_or(0)
    }

    /// How many entries `document`'s tables of names have (tests).
    #[doc(hidden)]
    pub fn name_table_len(&self, document: NodeRef<'gc>) -> usize {
        self.with_ids(document, |tables| {
            tables.tables[Table::Names as usize]
                .len()
                .saturating_add(tables.tables[Table::Frames as usize].len())
        })
        .unwrap_or(0)
    }
}
