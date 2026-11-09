//! A live collection of elements (DOM's `HTMLCollection`, Blink's
//! `HTMLCollection` of `TagCollection`, `ClassCollection` and
//! `HTMLCollection::kNodeChildren`): its root and what it matches
//! (`getElementsByTagName`'s qualified name, `getElementsByClassName`'s
//! classes, or the root's element children, `children`), its elements
//! found by walking the root's descendants (children) in tree order,
//! from the last one found while no change that could change them was made
//! (Blink's `CollectionIndexCache`, cleared by kind as its
//! `NodeListInvalidationType` says). A form's `elements` (HTML's
//! `HTMLFormControlsCollection`, Blink's) is the form's list of them
//! (`form`), which the form keeps; a fieldset's, its listed descendants.
//! A select's `options` and `selectedOptions` are its list of options
//! (`select`), found at each ask; a datalist's, its option descendants.

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, JsEdge, Member, WeakMember};

use crate::{
    ClassList, ClassName, Cx, NameId, Node, NodeKind, NodeRef, QualifiedName, QuirksMode, names,
    sites,
};

/// `HtmlCollection::kind`.
const BY_TAG: u8 = 0;
const BY_CLASS: u8 = 1;
const CHILDREN: u8 = 2;
/// A form's `elements`, its root the form.
pub(crate) const FORM_ELEMENTS: u8 = 3;
/// A document's `forms`: its HTML `form` elements.
pub(crate) const FORMS: u8 = 4;
/// A fieldset's `elements`: its listed descendants.
pub(crate) const FIELDSET_ELEMENTS: u8 = 5;
/// A select's `options` (`HTMLOptionsCollection`): its list of options.
const SELECT_OPTIONS: u8 = 6;
/// A select's `selectedOptions`: those of its list of options selected.
const SELECTED_OPTIONS: u8 = 7;
/// A datalist's `options`: its option descendants.
const DATALIST_OPTIONS: u8 = 8;
/// `getElementsByTagNameNS`: `prefix` the namespace (`STAR`: any), `local`
/// the local name (`all`: any).
const BY_TAG_NS: u8 = 9;
/// A document's `links`: its `a` and `area` elements with an `href`.
const LINKS: u8 = 10;
/// A document's `anchors`: its `a` elements with a `name`.
const ANCHORS: u8 = 11;
/// `getElementsByName`'s (a `NodeList`'s): the HTML elements whose `name`
/// is `local`'s.
const BY_NAME: u8 = 12;
/// A table's `rows`: its `thead`s' rows, then its own and its `tbody`s',
/// then its `tfoot`s'.
const TABLE_ROWS: u8 = 13;
/// A table's `tBodies`: its `tbody` children.
const TABLE_BODIES: u8 = 14;
/// A table section's `rows`: its `tr` children.
const SECTION_ROWS: u8 = 15;
/// A row's `cells`: its `td` and `th` children.
const ROW_CELLS: u8 = 16;
/// A map's `areas`: its `area` descendants.
const MAP_AREAS: u8 = 17;
/// A document's `images`, `embeds` (and `plugins`) and `scripts`: its
/// `img`, `embed` and `script` elements.
const IMAGES: u8 = 18;
const EMBEDS: u8 = 19;
const SCRIPTS: u8 = 20;
/// A window's named objects under a name (`local`'s), for its named
/// access when there are several: the elements with that id, and the HTML
/// `embed`, `form`, `img` and `object` elements with that `name`.
const WINDOW_NAMED: u8 = 21;
/// A document's `all` (`HTMLAllCollection`): every element.
const ALL: u8 = 22;
/// `document.all`'s elements under a name (`local`'s), when there are
/// several (HTML's "all"-named elements): the elements with that id, and
/// the HTML elements of `ALL_NAMED_ELEMENTS` with that `name`.
const ALL_NAMED: u8 = 23;

/// What kind of collection the same each time `kept_collection` makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeptCollection {
    TableRows,
    TableBodies,
    SectionRows,
    RowCells,
    MapAreas,
    Images,
    Embeds,
    Scripts,
    /// A document's `all`.
    All,
}

/// A collection: its root and its kind; by tag, the qualified name it
/// matches as name numbers, as given and lowercased (an HTML element in an
/// HTML document matches the lowercased one); by class, the classes an
/// element must all have; each interned when it was made so that an
/// element made after with the name has the same numbers.
#[derive(GcObject)]
pub struct HtmlCollection {
    root: Member<Node>,
    kind: Cell<u8>,
    /// `*`: every element.
    all: Cell<bool>,
    prefix: Cell<NameId>,
    local: Cell<NameId>,
    lower_prefix: Cell<NameId>,
    lower_local: Cell<NameId>,
    /// By class: its own object (a list has a finalizer: the others, most,
    /// have none).
    classes: Member<ClassList>,
    /// Blink's `CollectionIndexCache`, of a kind found by walking the tree
    /// (`walks_tree`): the element last found and its index, and the
    /// length once counted (`UNKNOWN` before), while the changes its kind
    /// is cleared by number `version` (`Cx::collection_version`: any
    /// insertion or removal, and the attributes the kind matches by), so
    /// that going through it in order, or back, is a step an element, not
    /// a walk from the first each time.
    cached: Member<Node>,
    cached_index: Cell<u32>,
    length: Cell<u32>,
    version: Cell<u64>,
    wrapper: JsEdge,
}

/// A root's collections by qualified name (`getElementsByTagName`'s and
/// `getElementsByTagNameNS`'s; Blink's `NodeListsNodeData`, which keeps
/// them weakly by type and name, so that the same call gives the same
/// object, as DOM lets it): a few, held weakly, each found again for its
/// kind and name numbers while script holds it; a new one takes a slot
/// whose collection died, else the oldest's.
#[derive(GcObject)]
pub struct NamedCollections {
    slots: [WeakMember<HtmlCollection>; NAMED_SLOTS],
    /// The slot the next one takes when none is free.
    next: Cell<u8>,
}

/// How many collections by name a root keeps: a page asks a root for few
/// names (Blink keeps any number; a slot each bounds what a page asking
/// for many keeps alive, which is nothing: they are weak).
const NAMED_SLOTS: usize = 8;

/// `HtmlCollection::length` not counted yet.
const UNKNOWN: u32 = u32::MAX;

/// The counts of the DOM's changes that clear collections' index caches,
/// by what they can change in one (Blink's `NodeListInvalidationType`):
/// any insertion or removal clears every collection's; an attribute's
/// change only those of the kinds matching by it (a tag's collection none:
/// changing classes while going through one keeps its place). Blink also
/// clears only the collections rooted at an ancestor of the change, which
/// a walk up from each change finds; counts are no walk, and a document's
/// collections, most, have every change below them anyway.
#[derive(Default)]
pub(crate) struct CollectionVersions {
    children: Cell<u64>,
    classes: Cell<u64>,
    ids: Cell<u64>,
    names: Cell<u64>,
    hrefs: Cell<u64>,
}

/// One more change counted in `count`.
fn count_change(count: &Cell<u64>) {
    count.set(count.get().checked_add(1).expect("fewer than 2^64 changes"));
}

thread_local! {
    /// How many nodes collections' walks in tree order looked at
    /// (`Cx::collection_next`), on this thread. Counted in debug builds
    /// only.
    static COLLECTION_STEPS: Cell<u64> = const { Cell::new(0) };
}

/// How many nodes, on this thread, the walks of collections in tree order
/// looked at: what tells a walk through a collection in order takes a step
/// an element, not one from the first each time. `None` in release builds.
/// For tests.
pub fn collection_steps_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| COLLECTION_STEPS.get())
}

/// A walk of a collection of a kind that `walks_tree` (`Cx::tree_walk`).
struct TreeWalk<'gc> {
    collection: &'gc HtmlCollection,
    root: NodeRef<'gc>,
    html: bool,
    quirks: bool,
    children: bool,
}

/// Whether a collection of `kind` is found by walking its root's
/// descendants (or children) in tree order, matching each
/// (`collection_matches`): the kinds its index cache keeps a place in.
/// The others walk lists of their own (a form's, a select's, a table's).
fn walks_tree(kind: u8) -> bool {
    !matches!(
        kind,
        FORM_ELEMENTS | SELECT_OPTIONS | SELECTED_OPTIONS | TABLE_ROWS
    )
}

impl<'gc> Cx<'_, 'gc> {
    /// A node was inserted or removed: every collection's index cache is
    /// out of date.
    pub(crate) fn collections_children_changed(&self) {
        count_change(&self.changes.collections.children);
    }

    /// An attribute `name` changed: the index caches of the kinds of
    /// collections matching by it are out of date. Any namespace's
    /// counts: a few more caches cleared, no compare of namespaces.
    pub(crate) fn collections_attribute_changed(&self, name: QualifiedName) {
        let versions = &self.changes.collections;
        match name.local() {
            names::CLASS => count_change(&versions.classes),
            names::ID => count_change(&versions.ids),
            names::local::NAME => count_change(&versions.names),
            names::local::HREF => count_change(&versions.hrefs),
            _ => {}
        }
    }

    /// What a class matches changed (a document's quirks mode).
    pub(crate) fn collections_classes_changed(&self) {
        count_change(&self.changes.collections.classes);
    }

    /// The number of the changes a collection of `kind`'s index cache is
    /// cleared by, so far: their counts summed, each only grows, so the
    /// sum changes with any of them.
    fn collection_version(&self, kind: u8) -> u64 {
        let versions = &self.changes.collections;
        let attributes = match kind {
            BY_CLASS => versions.classes.get(),
            ANCHORS | BY_NAME => versions.names.get(),
            LINKS => versions.hrefs.get(),
            WINDOW_NAMED | ALL_NAMED => versions
                .ids
                .get()
                .checked_add(versions.names.get())
                .expect("fewer than 2^64 changes"),
            // By its children's names, or theirs and kinds' fixed with
            // them (a form's, a table's, a select's lists): insertions and
            // removals only.
            _ => 0,
        };
        versions
            .children
            .get()
            .checked_add(attributes)
            .expect("fewer than 2^64 changes")
    }

    /// DOM's "list of elements with qualified name `qualified`" for `root`
    /// (`getElementsByTagName`): every element for `*`; else, in an HTML
    /// document, an HTML element whose qualified name is `qualified`
    /// lowercased or another element whose qualified name is `qualified`;
    /// in another document, an element whose qualified name is
    /// `qualified`.
    pub fn elements_by_tag_name(
        &self,
        root: NodeRef<'gc>,
        qualified: &str,
    ) -> Gc<'gc, HtmlCollection> {
        let (prefix, local) = self.split_qualified(qualified);
        if let Some(collection) = self.named_collection(root, BY_TAG, prefix, local) {
            return collection;
        }
        let lower = qualified.to_ascii_lowercase();
        let (lower_prefix, lower_local) = self.split_qualified(&lower);
        let collection = self.gc.alloc_in(
            &sites::OTHER,
            HtmlCollectionInit {
                root: Some(root),
                kind: Cell::new(BY_TAG),
                all: Cell::new(qualified == "*"),
                prefix: Cell::new(prefix),
                local: Cell::new(local),
                lower_prefix: Cell::new(lower_prefix),
                lower_local: Cell::new(lower_local),
                classes: None,
                cached: None,
                cached_index: Cell::new(0),
                length: Cell::new(UNKNOWN),
                version: Cell::new(0),
                wrapper: (),
            },
        );
        self.keep_named_collection(root, collection);
        collection
    }

    /// `root`'s collection of `kind` by the names `prefix` and `local`
    /// that script still holds, if there is one (`NamedCollections`).
    fn named_collection(
        &self,
        root: NodeRef<'gc>,
        kind: u8,
        prefix: NameId,
        local: NameId,
    ) -> Option<Gc<'gc, HtmlCollection>> {
        let named = self
            .node_rare(root, false)?
            .named_collections
            .get(self.gc)?;
        named.slots.iter().find_map(|slot| {
            slot.get(self.gc).filter(|collection| {
                collection.kind.get() == kind
                    && collection.prefix.get() == prefix
                    && collection.local.get() == local
            })
        })
    }

    /// `collection`, new, one of `root`'s by name: kept weakly, in a free
    /// slot or the oldest's.
    fn keep_named_collection(&self, root: NodeRef<'gc>, collection: Gc<'gc, HtmlCollection>) {
        let rare = self.node_rare(root, true).expect("any node has rare data");
        let named = match rare.named_collections.get(self.gc) {
            Some(named) => named,
            None => {
                let named = self.gc.alloc_in(
                    &sites::OTHER,
                    NamedCollectionsInit {
                        slots: [None; NAMED_SLOTS],
                        next: Cell::new(0),
                    },
                );
                rare.named_collections.set(self.gc, Some(named));
                named
            }
        };
        let slot = match named
            .slots
            .iter()
            .position(|slot| slot.get(self.gc).is_none())
        {
            Some(free) => free,
            None => {
                let next = usize::from(named.next.get());
                // Below `NAMED_SLOTS`, which fits a `u8`.
                named
                    .next
                    .set(u8::try_from((next + 1) % NAMED_SLOTS).unwrap_or(0));
                next
            }
        };
        named.slots[slot].set(self.gc, Some(collection));
    }

    /// DOM's "list of elements with class names `class_names`" for `root`
    /// (`getElementsByClassName`): the elements with every one of the
    /// classes (ASCII whitespace separated; ASCII case-insensitively in a
    /// quirks mode document); none for no classes.
    pub fn elements_by_class_name(
        &self,
        root: NodeRef<'gc>,
        class_names: &str,
    ) -> Gc<'gc, HtmlCollection> {
        let mut classes: Vec<ClassName> = Vec::new();
        for token in class_names.split_ascii_whitespace() {
            let name = names::intern(token);
            if !classes.iter().any(|class| class.name == name) {
                classes.push(ClassName {
                    name,
                    lower: names::intern(&token.to_ascii_lowercase()),
                    hash: 0,
                });
            }
        }
        let list = self.new_class_list(classes.into_boxed_slice());
        self.new_collection(root, BY_CLASS, Some(list))
    }

    /// DOM's "list of elements with namespace `namespace` and local name
    /// `local`" for `root` (`getElementsByTagNameNS`): `*` for either is
    /// any; the empty namespace is none.
    pub fn elements_by_tag_name_ns(
        &self,
        root: NodeRef<'gc>,
        namespace: Option<&str>,
        local: &str,
    ) -> Gc<'gc, HtmlCollection> {
        let namespace = match namespace {
            None | Some("") => names::NONE,
            Some(namespace) => names::intern(namespace),
        };
        let local_name = names::intern(local);
        if let Some(collection) = self.named_collection(root, BY_TAG_NS, namespace, local_name) {
            return collection;
        }
        let collection = self.new_collection(root, BY_TAG_NS, None);
        collection.prefix.set(namespace);
        collection.local.set(local_name);
        collection.all.set(local == "*");
        self.keep_named_collection(root, collection);
        collection
    }

    /// `document`'s `links` or (`anchors`) its `anchors`: a new collection
    /// each time (as `forms`' is kept: `[SameObject]` isn't asked of these).
    pub fn document_links(&self, document: NodeRef<'gc>, anchors: bool) -> Gc<'gc, HtmlCollection> {
        self.new_collection(document, if anchors { ANCHORS } else { LINKS }, None)
    }

    /// `getElementsByName(name)`'s elements: a collection the list walks.
    pub(crate) fn elements_by_name(
        &self,
        document: NodeRef<'gc>,
        name: &str,
    ) -> Gc<'gc, HtmlCollection> {
        let collection = self.new_collection(document, BY_NAME, None);
        collection.local.set(names::intern(name));
        collection
    }

    /// `root`'s collection of `kind`, the same each time.
    pub fn kept_collection(
        &self,
        root: NodeRef<'gc>,
        kind: KeptCollection,
    ) -> Gc<'gc, HtmlCollection> {
        let kind = match kind {
            KeptCollection::TableRows => TABLE_ROWS,
            KeptCollection::TableBodies => TABLE_BODIES,
            KeptCollection::SectionRows => SECTION_ROWS,
            KeptCollection::RowCells => ROW_CELLS,
            KeptCollection::MapAreas => MAP_AREAS,
            KeptCollection::Images => IMAGES,
            KeptCollection::Embeds => EMBEDS,
            KeptCollection::Scripts => SCRIPTS,
            KeptCollection::All => ALL,
        };
        let rare = self.node_rare(root, true).expect("any node has rare data");
        let kept = match rare.collections.get(self.gc) {
            Some(kept) => kept,
            None => {
                let kept = HeapVector::new(self.gc, 1);
                rare.collections.set(self.gc, Some(kept));
                kept
            }
        };
        for index in 0..kept.len() {
            if let Some(collection) = kept.get(self.gc, index)
                && collection.kind.get() == kind
            {
                return collection;
            }
        }
        let collection = self.new_collection(root, kind, None);
        kept.push(self.gc, collection);
        collection
    }

    /// `document`'s window's named objects under `name` (`window_named`'s
    /// several), the same each time: kept with the document's other
    /// collections, as long as it lives (Blink keeps them while script
    /// holds them; a page asks for few names).
    pub fn window_named_collection(
        &self,
        document: NodeRef<'gc>,
        name: &str,
    ) -> Gc<'gc, HtmlCollection> {
        let local = names::intern(name);
        let rare = self
            .node_rare(document, true)
            .expect("any node has rare data");
        let kept = match rare.collections.get(self.gc) {
            Some(kept) => kept,
            None => {
                let kept = HeapVector::new(self.gc, 1);
                rare.collections.set(self.gc, Some(kept));
                kept
            }
        };
        for index in 0..kept.len() {
            if let Some(collection) = kept.get(self.gc, index)
                && collection.kind.get() == WINDOW_NAMED
                && collection.local.get() == local
            {
                return collection;
            }
        }
        let collection = self.new_collection(document, WINDOW_NAMED, None);
        collection.local.set(local);
        kept.push(self.gc, collection);
        collection
    }

    /// `document.all`'s elements under `name` (its `namedItem`'s several),
    /// the same each time: kept with the document's other collections, as
    /// the window's named objects are.
    pub fn all_named_collection(
        &self,
        document: NodeRef<'gc>,
        name: &str,
    ) -> Gc<'gc, HtmlCollection> {
        let local = names::intern(name);
        let rare = self
            .node_rare(document, true)
            .expect("any node has rare data");
        let kept = match rare.collections.get(self.gc) {
            Some(kept) => kept,
            None => {
                let kept = HeapVector::new(self.gc, 1);
                rare.collections.set(self.gc, Some(kept));
                kept
            }
        };
        for index in 0..kept.len() {
            if let Some(collection) = kept.get(self.gc, index)
                && collection.kind.get() == ALL_NAMED
                && collection.local.get() == local
            {
                return collection;
            }
        }
        let collection = self.new_collection(document, ALL_NAMED, None);
        collection.local.set(local);
        kept.push(self.gc, collection);
        collection
    }

    /// Whether `collection` is a document's `all` (`HTMLAllCollection`).
    pub fn is_all_collection(&self, collection: Gc<'gc, HtmlCollection>) -> bool {
        collection.kind.get() == ALL
    }

    /// `node`'s `children`: its element children, the same each time.
    pub fn children_collection(&self, node: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let rare = self.node_rare(node, true).expect("any node has rare data");
        if let Some(collection) = rare.children.get(self.gc) {
            return collection;
        }
        let collection = self.new_collection(node, CHILDREN, None);
        rare.children.set(self.gc, Some(collection));
        collection
    }

    pub(crate) fn new_collection(
        &self,
        root: NodeRef<'gc>,
        kind: u8,
        classes: Option<Gc<'gc, ClassList>>,
    ) -> Gc<'gc, HtmlCollection> {
        self.gc.alloc_in(
            &sites::OTHER,
            HtmlCollectionInit {
                root: Some(root),
                kind: Cell::new(kind),
                all: Cell::new(false),
                prefix: Cell::new(names::NONE),
                local: Cell::new(names::NONE),
                lower_prefix: Cell::new(names::NONE),
                lower_local: Cell::new(names::NONE),
                classes,
                cached: None,
                cached_index: Cell::new(0),
                length: Cell::new(UNKNOWN),
                version: Cell::new(0),
                wrapper: (),
            },
        )
    }

    /// A qualified name's prefix (none without a colon) and local name,
    /// interned.
    fn split_qualified(&self, qualified: &str) -> (NameId, NameId) {
        match qualified.split_once(':') {
            Some((prefix, local)) => (names::intern(prefix), names::intern(local)),
            None => (names::NONE, names::intern(qualified)),
        }
    }

    /// Whether `element` is one of `collection`'s (`html`: its root's
    /// document is an HTML one; `quirks`: in quirks mode).
    #[inline]
    fn collection_matches(
        &self,
        collection: &HtmlCollection,
        element: NodeRef<'gc>,
        html: bool,
        quirks: bool,
    ) -> bool {
        match collection.kind.get() {
            BY_TAG => {}
            CHILDREN => return true,
            FORMS => return element.qualified_name() == QualifiedName::html(names::local::FORM),
            FIELDSET_ELEMENTS => return self.is_listed(element),
            DATALIST_OPTIONS => return self.is_option(element),
            BY_TAG_NS => return self.tag_ns_matches(collection, element),
            LINKS | ANCHORS | BY_NAME => return self.link_matches(collection, element),
            TABLE_BODIES..=SCRIPTS => return self.kept_matches(collection, element),
            WINDOW_NAMED => return self.is_window_named(element, collection.local.get()),
            ALL => return true,
            ALL_NAMED => return self.is_all_named(element, collection.local.get()),
            _ => return self.class_matches(collection, element, quirks),
        }
        if collection.all.get() {
            return true;
        }
        let name = element.qualified_name().ids();
        let (prefix, local) = if html && name.namespace == names::HTML_NAMESPACE {
            (collection.lower_prefix.get(), collection.lower_local.get())
        } else {
            (collection.prefix.get(), collection.local.get())
        };
        name.local == local && name.prefix == prefix
    }

    /// Whether `element` is of a by-namespace `collection`'s namespace and
    /// local name.
    #[inline(never)]
    fn tag_ns_matches(&self, collection: &HtmlCollection, element: NodeRef<'gc>) -> bool {
        let name = element.qualified_name().ids();
        let namespace = collection.prefix.get();
        (namespace == names::intern("*") || name.namespace == namespace)
            && (collection.all.get() || name.local == collection.local.get())
    }

    /// Whether `element` is one of a document's `links` (an HTML `a` or
    /// `area` with an `href`), `anchors` (an `a` with a `name`) or those
    /// `getElementsByName` finds (an HTML element whose `name` is the
    /// collection's).
    #[inline(never)]
    fn link_matches(&self, collection: &HtmlCollection, element: NodeRef<'gc>) -> bool {
        let name = element.qualified_name().ids();
        if name.namespace != names::HTML_NAMESPACE {
            return false;
        }
        match collection.kind.get() {
            LINKS => {
                (name.local == names::local::A || name.local == names::local::AREA)
                    && self
                        .attribute_value_by_id(element, names::local::HREF)
                        .is_some()
            }
            ANCHORS => {
                name.local == names::local::A
                    && self
                        .attribute_value_by_id(element, names::local::NAME)
                        .is_some()
            }
            _ => self
                .attribute_value_by_id(element, names::local::NAME)
                .is_some_and(|value| {
                    *value.borrow().to_str() == *names::get(collection.local.get())
                }),
        }
    }

    /// Whether `element` is one of a kept `collection`'s (its HTML
    /// elements of a local name or two; a table's rows aren't walked so).
    #[inline(never)]
    fn kept_matches(&self, collection: &HtmlCollection, element: NodeRef<'gc>) -> bool {
        let name = element.qualified_name().ids();
        if name.namespace != names::HTML_NAMESPACE {
            return false;
        }
        let local = name.local;
        match collection.kind.get() {
            TABLE_BODIES => local == names::local::TBODY,
            SECTION_ROWS => local == names::local::TR,
            ROW_CELLS => local == names::local::TD || local == names::local::TH,
            MAP_AREAS => local == names::local::AREA,
            IMAGES => local == names::local::IMG,
            EMBEDS => local == names::local::EMBED,
            _ => local == names::local::SCRIPT,
        }
    }

    /// A table's rows, in `rows`' order, each given to `found` until it says
    /// stop.
    fn for_each_table_row(&self, table: NodeRef<'gc>, mut found: impl FnMut(NodeRef<'gc>) -> bool) {
        let is =
            |node: NodeRef<'gc>, local: NameId| node.qualified_name() == QualifiedName::html(local);
        let children = |parent: NodeRef<'gc>| {
            std::iter::successors(self.first_child(parent), |&at| self.next_sibling(at))
        };
        let rows_of =
            |section: NodeRef<'gc>| children(section).filter(move |&row| is(row, names::local::TR));
        for section in children(table).filter(|&child| is(child, names::local::THEAD)) {
            for row in rows_of(section) {
                if !found(row) {
                    return;
                }
            }
        }
        for child in children(table) {
            if is(child, names::local::TR) {
                if !found(child) {
                    return;
                }
            } else if is(child, names::local::TBODY) {
                for row in rows_of(child) {
                    if !found(row) {
                        return;
                    }
                }
            }
        }
        for section in children(table).filter(|&child| is(child, names::local::TFOOT)) {
            for row in rows_of(section) {
                if !found(row) {
                    return;
                }
            }
        }
    }

    /// Whether `element` has every one of a by-class `collection`'s
    /// classes (out of the by-tag walk's way).
    #[inline(never)]
    fn class_matches(
        &self,
        collection: &HtmlCollection,
        element: NodeRef<'gc>,
        quirks: bool,
    ) -> bool {
        let wanted = collection
            .classes
            .get(self.gc)
            .map_or(&[][..], |list| self.class_list_names(list));
        if wanted.is_empty() {
            return false;
        }
        let has = self.class_names(element);
        wanted.iter().all(|want| {
            has.iter().any(|class| {
                if quirks {
                    class.lower == want.lower
                } else {
                    class.name == want.name
                }
            })
        })
    }

    /// `collection`'s elements in tree order, each given to `found` until
    /// it says stop (`false`).
    pub(crate) fn collection_walk(
        &self,
        collection: Gc<'gc, HtmlCollection>,
        mut found: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        let root = collection.root.get(self.gc).expect("a collection's root");
        match collection.kind.get() {
            FORM_ELEMENTS => {
                self.for_each_form_control(root, found);
                return;
            }
            SELECT_OPTIONS => {
                self.for_each_option(root, found);
                return;
            }
            SELECTED_OPTIONS => {
                self.for_each_option(root, |option| {
                    !self.option_selectedness(option) || found(option)
                });
                return;
            }
            TABLE_ROWS => {
                self.for_each_table_row(root, found);
                return;
            }
            _ => {}
        }
        let walk = self.tree_walk(collection);
        let mut at = self.collection_next(&walk, None);
        while let Some(element) = at {
            if !found(element) {
                return;
            }
            at = self.collection_next(&walk, Some(element));
        }
    }

    /// How a collection of a kind that `walks_tree` walks its root's
    /// subtree: its root, its document's mode, whether only the root's
    /// children.
    fn tree_walk(&self, collection: Gc<'gc, HtmlCollection>) -> TreeWalk<'gc> {
        let root = collection.root.get(self.gc).expect("a collection's root");
        let document = self.document_of(root);
        let collection = collection.into_ref();
        TreeWalk {
            collection,
            root,
            html: self.is_html_document(document),
            quirks: self.quirks_mode(document) == QuirksMode::Quirks,
            children: matches!(
                collection.kind.get(),
                CHILDREN | TABLE_BODIES | SECTION_ROWS | ROW_CELLS
            ),
        }
    }

    /// The collection's first element in tree order after `after`, or its
    /// first if none.
    fn collection_next(
        &self,
        walk: &TreeWalk<'gc>,
        after: Option<NodeRef<'gc>>,
    ) -> Option<NodeRef<'gc>> {
        let step = |node| {
            if walk.children {
                self.next_sibling(node)
            } else {
                self.following_within(node, walk.root)
            }
        };
        let mut at = match after {
            Some(node) => step(node),
            None => self.first_child(walk.root),
        };
        while let Some(node) = at {
            if cfg!(debug_assertions) {
                COLLECTION_STEPS.set(COLLECTION_STEPS.get().wrapping_add(1));
            }
            if self.kind(node) == NodeKind::Element
                && self.collection_matches(walk.collection, node, walk.html, walk.quirks)
            {
                return Some(node);
            }
            at = step(node);
        }
        None
    }

    /// The collection's last element in tree order before `before`, or its
    /// last if none.
    fn collection_previous(
        &self,
        walk: &TreeWalk<'gc>,
        before: Option<NodeRef<'gc>>,
    ) -> Option<NodeRef<'gc>> {
        let last_within = |mut node: NodeRef<'gc>| {
            while let Some(last) = self.last_child(node) {
                node = last;
            }
            node
        };
        let step = |node: NodeRef<'gc>| {
            if walk.children {
                return self.previous_sibling(node);
            }
            match self.previous_sibling(node) {
                Some(previous) => Some(last_within(previous)),
                None => self
                    .parent(node)
                    .filter(|&parent| !Gc::ptr_eq(parent, walk.root)),
            }
        };
        let mut at = match before {
            Some(node) => step(node),
            None if walk.children => self.last_child(walk.root),
            None => self.last_child(walk.root).map(last_within),
        };
        while let Some(node) = at {
            if cfg!(debug_assertions) {
                COLLECTION_STEPS.set(COLLECTION_STEPS.get().wrapping_add(1));
            }
            if self.kind(node) == NodeKind::Element
                && self.collection_matches(walk.collection, node, walk.html, walk.quirks)
            {
                return Some(node);
            }
            at = step(node);
        }
        None
    }

    /// `collection`'s index cache, cleared if a change that could change
    /// its elements was made since it was kept.
    fn fresh_index_cache(&self, collection: &HtmlCollection) {
        let version = self.collection_version(collection.kind.get());
        if collection.version.get() != version {
            collection.version.set(version);
            collection.cached.set(self.gc, None);
            collection.cached_index.set(0);
            collection.length.set(UNKNOWN);
        }
    }

    /// `collection`'s root: the node it was made for.
    pub fn collection_root(&self, collection: Gc<'gc, HtmlCollection>) -> NodeRef<'gc> {
        collection.root.get(self.gc).expect("a collection's root")
    }

    /// Whether `collection` is a form's `elements`
    /// (`HTMLFormControlsCollection`).
    pub fn is_form_controls_collection(&self, collection: Gc<'gc, HtmlCollection>) -> bool {
        collection.kind.get() == FORM_ELEMENTS
    }

    /// Whether `collection` is a select's `options` (`HTMLOptionsCollection`).
    pub fn is_options_collection(&self, collection: Gc<'gc, HtmlCollection>) -> bool {
        collection.kind.get() == SELECT_OPTIONS
    }

    /// A select's `options`, the same each time.
    pub fn select_options(&self, select: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let association = self.association_or_new(select);
        if let Some(collection) = association.collection.get(self.gc) {
            return collection;
        }
        let collection = self.new_collection(select, SELECT_OPTIONS, None);
        association.collection.set(self.gc, Some(collection));
        collection
    }

    /// A select's `selectedOptions`, the same each time.
    pub fn selected_options(&self, select: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let association = self.association_or_new(select);
        if let Some(collection) = association.selected_options.get(self.gc) {
            return collection;
        }
        let collection = self.new_collection(select, SELECTED_OPTIONS, None);
        association.selected_options.set(self.gc, Some(collection));
        collection
    }

    /// A datalist's `options`, the same each time.
    pub fn datalist_options(&self, datalist: NodeRef<'gc>) -> Gc<'gc, HtmlCollection> {
        let association = self.association_or_new(datalist);
        if let Some(collection) = association.collection.get(self.gc) {
            return collection;
        }
        let collection = self.new_collection(datalist, DATALIST_OPTIONS, None);
        association.collection.set(self.gc, Some(collection));
        collection
    }

    /// The node after `node` in tree order, within `root`'s descendants.
    pub fn following_within(&self, node: NodeRef<'gc>, root: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if let Some(child) = self.first_child(node) {
            return Some(child);
        }
        let mut at = node;
        loop {
            if Gc::ptr_eq(at, root) {
                return None;
            }
            if let Some(sibling) = self.next_sibling(at) {
                return Some(sibling);
            }
            at = self.parent(at)?;
        }
    }

    /// `length`: how many elements `collection` has now; counted once
    /// while the trees don't change, from the last one found.
    pub fn collection_length(&self, collection: Gc<'gc, HtmlCollection>) -> u32 {
        if !walks_tree(collection.kind.get()) {
            let mut count = 0u32;
            self.collection_walk(collection, |_| {
                count = count.saturating_add(1);
                true
            });
            return count;
        }
        let walk = self.tree_walk(collection);
        let cache = walk.collection;
        self.fresh_index_cache(cache);
        if cache.length.get() != UNKNOWN {
            return cache.length.get();
        }
        // A collection's elements are in the heap: fewer than u32::MAX.
        let (mut at, mut count) = match cache.cached.get(self.gc) {
            Some(cached) => (
                self.collection_next(&walk, Some(cached)),
                cache.cached_index.get().saturating_add(1),
            ),
            None => (self.collection_next(&walk, None), 0),
        };
        while let Some(element) = at {
            count = count.saturating_add(1);
            at = self.collection_next(&walk, Some(element));
        }
        cache.length.set(count);
        count
    }

    /// `item(index)`: `collection`'s element at `index` now, if it has one:
    /// from the last one found, the first or (its length counted) the
    /// last, whichever is nearest (Blink's `CollectionIndexCache::NodeAt`),
    /// so that going back through it is a step an element too.
    pub fn collection_item(
        &self,
        collection: Gc<'gc, HtmlCollection>,
        index: u32,
    ) -> Option<NodeRef<'gc>> {
        if !walks_tree(collection.kind.get()) {
            let mut left = index;
            let mut item = None;
            self.collection_walk(collection, |element| match left.checked_sub(1) {
                Some(rest) => {
                    left = rest;
                    true
                }
                None => {
                    item = Some(element);
                    false
                }
            });
            return item;
        }
        let walk = self.tree_walk(collection);
        let cache = walk.collection;
        self.fresh_index_cache(cache);
        let length = cache.length.get();
        if length != UNKNOWN && index >= length {
            return None;
        }
        let cached_index = cache.cached_index.get();
        let start = match cache.cached.get(self.gc) {
            Some(cached) if index >= cached_index || index > cached_index / 2 => {
                Some((cached, cached_index))
            }
            // The length counted, `index` below it: a last element.
            _ if length != UNKNOWN && index > length / 2 => self
                .collection_previous(&walk, None)
                .map(|last| (last, length.saturating_sub(1))),
            _ => self.collection_next(&walk, None).map(|first| (first, 0)),
        };
        let Some((mut at, mut at_index)) = start else {
            cache.length.set(0);
            return None;
        };
        while at_index > index {
            at = self
                .collection_previous(&walk, Some(at))
                .expect("an element before the cached one's index");
            at_index = at_index.saturating_sub(1);
        }
        while at_index < index {
            match self.collection_next(&walk, Some(at)) {
                Some(next) => at = next,
                None => {
                    cache.length.set(at_index.saturating_add(1));
                    return None;
                }
            }
            at_index = at_index.saturating_add(1);
        }
        cache.cached.set(self.gc, Some(at));
        cache.cached_index.set(index);
        Some(at)
    }

    /// `namedItem(key)`: `collection`'s first element whose `id` is `key`,
    /// or that is an HTML element whose `name` attribute is; none for an
    /// empty key.
    pub fn collection_named_item(
        &self,
        collection: Gc<'gc, HtmlCollection>,
        key: &str,
    ) -> Option<NodeRef<'gc>> {
        if key.is_empty() {
            return None;
        }
        let mut item = None;
        self.collection_walk(collection, |element| {
            let id = self.attribute_value_by_id(element, names::ID);
            let by_name = element.qualified_name().namespace() == names::HTML_NAMESPACE
                && self
                    .attribute_value_by_id(element, names::NAME)
                    .is_some_and(|value| *value.borrow() == key);
            if id.is_some_and(|value| *value.borrow() == key) || by_name {
                item = Some(element);
                return false;
            }
            true
        });
        item
    }

    /// `collection`'s reference to its wrapper, which only the JS engine
    /// reads and writes.
    pub(crate) fn collection_wrapper_edge(
        &self,
        collection: Gc<'gc, HtmlCollection>,
    ) -> &'gc JsEdge {
        &collection.into_ref().wrapper
    }
}
