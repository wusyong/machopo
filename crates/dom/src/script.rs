//! A script element's triggers to be prepared (HTML's "prepare the script
//! element", the engine's to run: `dom_api`'s `script`), recorded as
//! the operation goes (`ElementChange::Script`, in the iframes' list:
//! Blink's `ScriptLoader` prepares it as the insertion is over,
//! `DidNotifySubtreeInsertionsToDocument`): one that isn't parser-inserted
//! or already started becomes connected (DOM's post-connection steps),
//! has a node inserted while connected (HTML's children changed steps, as
//! Blink's `ScriptLoader::ChildrenChanged`: an insertion only), or gets a
//! `src` while connected (its attribute change steps). Nothing is looked
//! at before such a script element is made (`form::SCRIPTS_SEEN`): the
//! parser's scripts are parser-inserted, or already started.
//!
//! And the external scripts that wait for no parser (HTML's "set of
//! scripts that will execute as soon as possible" and "list of scripts
//! that will execute in order as soon as possible", Blink's
//! `ScriptRunner`, a document's): each one's element has its pending
//! script (Blink's `PendingScript`), in the heap; the in-order ones are in
//! their document's list too. The host fetches them (`take_script_fetches`)
//! and hands each back (`script_fetched`); one whose turn it is then runs
//! in an element task of its own (`ElementTaskKind::Script`, Blink's
//! `ScriptRunner` posts a task). Each delays its document's load event
//! until it ran.
//!
//! And a `<link rel=modulepreload>`'s triggers (`ElementChange::
//! ModulePreload`): connected, or its attributes changed while connected,
//! from the first such link on (`form::PRELOADS_SEEN`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use elise::{External, Gc, GcObject, HeapVector, Member};

use crate::frame::ElementChange;
use crate::{
    ALREADY_STARTED, Cx, Dom, NON_BLOCKING, Node, NodeId, NodeRef, PARSER_INSERTED, QualifiedName,
    form, names, sites,
};

/// What fetching a classic script asks (HTML's "fetch a classic script"):
/// its URL, its CORS settings (none: `no-cors`), and the encoding its
/// `charset` attribute names, the fallback for a response that says none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptRequest {
    pub url: String,
    pub cors: Option<ScriptCors>,
    pub charset: Option<String>,
}

/// A `crossorigin` attribute's credentials mode (HTML's CORS settings
/// attribute: `use-credentials`, ASCII case-insensitively, or anonymous,
/// any other value).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptCors {
    Anonymous,
    UseCredentials,
}

/// A script the host is to fetch (`Dom::take_script_fetches`): its number,
/// to hand it back by (`Cx::script_fetched`; a module's, the engine's
/// module map's), the request, the document it was prepared in, whether it
/// is a module (fetched by CORS, its credentials mode same-origin without a
/// `crossorigin`; a JavaScript MIME type its response's), and who asked
/// for it (a classic one's element; a module's, the module map's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptFetch {
    pub key: u64,
    pub request: ScriptRequest,
    pub document: NodeId,
    pub module: bool,
    pub initiator: crate::LoadInitiator,
}

/// A fetched classic script: its source, decoded; the URL the response
/// came from (after redirects: its URL and base URL); and whether its
/// errors are muted (another origin's, fetched `no-cors`: HTML's "muted
/// errors").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedScript {
    pub source: String,
    pub url: String,
    pub muted: bool,
}

/// What a pending script's fetch gave: a classic script, or a module
/// script's graph, by its root's number in the engine's realm (linked, or
/// with an error to rethrow).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptResult {
    Classic(LoadedScript),
    Module(u32),
}

/// A script whose turn it is (`Cx::take_ready_script`): the document it
/// was prepared in, what it fetched (none: it failed, `error` is due), and
/// whether it is external (a `load` after it runs).
pub struct ReadyScript<'gc> {
    pub document: NodeRef<'gc>,
    pub loaded: Option<ScriptResult>,
    pub external: bool,
}

/// A script that waits for its fetch (Blink's `PendingScript`), its
/// element's: an external classic one that waits for no parser, or any
/// module script (whose graph is fetched, the parser's deferred one's
/// too); its preparation-time document (whose load it delays), its
/// number, and what the fetch gave once it did.
#[derive(GcObject)]
pub struct PendingScript {
    document: Member<Node>,
    key: Cell<u64>,
    /// Fetched (or failed): its turn may come.
    fetched: Cell<bool>,
    /// In order: its document's list holds it too.
    in_order: Cell<bool>,
    /// The parser's to run once fetched (a deferred module script): no
    /// task of its own.
    parser: Cell<bool>,
    /// From `src`: `load` after it runs.
    external: Cell<bool>,
    /// Taken as it runs (a move, no borrow); none for a failed fetch.
    loaded: External<Cell<Option<Box<ScriptResult>>>>,
}

/// The fetches the host is yet to start, and the elements of those it
/// started, by number. A `RefCell`: reached through `&Cx` within one step,
/// on a script's path only.
#[derive(Default)]
pub(crate) struct ScriptFetches {
    keys: Cell<u64>,
    fetches: RefCell<Vec<ScriptFetch>>,
    fetching: RefCell<HashMap<u64, NodeId>>,
}

impl Dom {
    /// The scripts the DOM's documents wait for, taken: the host fetches
    /// each and hands it back (`Cx::script_fetched`).
    pub fn take_script_fetches(&self) -> Vec<ScriptFetch> {
        std::mem::take(&mut self.changes.scripts.fetches.borrow_mut())
    }
}

impl ScriptFetches {
    /// The element of the pending script numbered `key`, while it waits.
    pub(crate) fn element_of(&self, key: u64) -> Option<NodeId> {
        self.fetching.borrow().get(&key).copied()
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `node` is an HTML `<script>`.
    pub fn is_html_script(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node) == Some(QualifiedName::html(names::local::SCRIPT))
    }

    /// A script element made that the parser didn't make: non-blocking,
    /// and it may run as it is inserted.
    #[cold]
    #[inline(never)]
    pub(crate) fn script_made(&self, element: NodeRef<'gc>) {
        element.flags.set(element.flags.get() | NON_BLOCKING);
        self.see(form::SCRIPTS_SEEN);
    }

    /// HTML's cloning steps of a script element: the copy is already
    /// started if `original` is.
    pub(crate) fn script_cloned(&self, original: NodeRef<'gc>, copy: NodeRef<'gc>) {
        if self.flag(original, ALREADY_STARTED) && self.is_html_script(original) {
            copy.flags.set(copy.flags.get() | ALREADY_STARTED);
        }
    }

    /// Whether `node` is a script element that is to be prepared as script
    /// changes it: neither parser-inserted nor already started.
    fn may_prepare(&self, node: NodeRef<'gc>) -> bool {
        node.flags.get() & (PARSER_INSERTED | ALREADY_STARTED) == 0 && self.is_html_script(node)
    }

    /// `node` recorded if it is a script element that may run (script
    /// asked for its load).
    fn record_script(&self, node: NodeRef<'gc>) {
        if self.may_prepare(node) {
            self.record_element_change(ElementChange::Script(self.node_id(node)));
            self.element_asked_for_load(node, false);
        }
    }

    /// `node` connected (`frame::connection_steps`, in tree order): a
    /// script element that may run recorded.
    pub(crate) fn script_connected(&self, node: NodeRef<'gc>) {
        self.record_script(node);
    }

    /// A node was inserted into `parent`, connected: a script element that
    /// may run recorded (before what was inserted: it comes first in tree
    /// order). Out of line: the insertion stays small.
    #[inline(never)]
    pub(crate) fn script_children_changed(&self, parent: NodeRef<'gc>) {
        if self.has_seen(form::SCRIPTS_SEEN) {
            self.record_script(parent);
        }
    }

    /// `element`'s attribute `local` was set or removed (`present`; from
    /// `form::control_attribute_changed`, as `src` and `async` are its
    /// names): a script element's `src` set while connected records it;
    /// its `async` added makes it not non-blocking (HTML's attribute
    /// change steps, Blink's `ScriptLoader::HandleAsyncAttribute`).
    pub(crate) fn script_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        local: names::NameId,
        present: bool,
    ) {
        if !present || !self.is_html_script(element) {
            return;
        }
        match local {
            names::local::ASYNC => element.flags.set(element.flags.get() & !NON_BLOCKING),
            names::local::SRC if self.is_connected(element) => self.record_script(element),
            _ => {}
        }
    }

    /// `element`'s external script queued to be fetched and to run as soon
    /// as it can, or in order (`in_order`) with its document's others
    /// (HTML's "prepare the script element" for one script inserted, or
    /// the parser's `async` one): its pending script, in its document's
    /// list if in order, delaying its document's load event; the fetch for
    /// the host.
    pub fn queue_script(&self, element: NodeRef<'gc>, request: ScriptRequest, in_order: bool) {
        let key = self.pend_script(element, in_order, false, true);
        let document = self.document_of(element);
        let state = &self.changes.scripts;
        let id = self.node_id(element);
        state.fetching.borrow_mut().insert(key, id);
        state.fetches.borrow_mut().push(ScriptFetch {
            key,
            request,
            document: self.node_id(document),
            module: false,
            initiator: crate::LoadInitiator::Element(id),
        });
    }

    /// `element`'s module script queued, its graph for the engine to fetch
    /// (`module_graph_fetched` once it did): to run as soon as it can, in
    /// order, or (`parser`) when the parser says, as a deferred one;
    /// `external`, from `src`. Its number, the graph's.
    pub fn queue_module_script(
        &self,
        element: NodeRef<'gc>,
        in_order: bool,
        parser: bool,
        external: bool,
    ) -> u64 {
        let key = self.pend_script(element, in_order, parser, external);
        self.changes
            .scripts
            .fetching
            .borrow_mut()
            .insert(key, self.node_id(element));
        key
    }

    /// A module the engine's module map asks for (HTML's "fetch a single
    /// module script"), of a graph for `document`, asked for by
    /// `initiator`: the fetch for the host, handed back to the engine. Its
    /// number.
    pub fn queue_module_fetch(
        &self,
        document: NodeRef<'gc>,
        request: ScriptRequest,
        initiator: crate::LoadInitiator,
    ) -> u64 {
        let state = &self.changes.scripts;
        let key = state.keys.get().wrapping_add(1);
        state.keys.set(key);
        state.fetches.borrow_mut().push(ScriptFetch {
            key,
            request,
            document: self.node_id(document),
            module: true,
            initiator,
        });
        key
    }

    /// `element`'s pending script made, its number given: in its
    /// document's list if `in_order`, delaying its document's load event.
    fn pend_script(
        &self,
        element: NodeRef<'gc>,
        in_order: bool,
        parser: bool,
        external: bool,
    ) -> u64 {
        let state = &self.changes.scripts;
        let key = state.keys.get().wrapping_add(1);
        state.keys.set(key);
        let document = self.document_of(element);
        let pending = self.gc.alloc_in(
            &sites::OTHER,
            PendingScriptInit {
                document: Some(document),
                key: Cell::new(key),
                fetched: Cell::new(false),
                in_order: Cell::new(in_order),
                parser: Cell::new(parser),
                external: Cell::new(external),
                loaded: External::new(Cell::new(None)),
            },
        );
        self.element_data(element)
            .script
            .set(self.gc, Some(pending));
        if in_order {
            let data = self.ensure_document_data(document);
            let list = match data.in_order_scripts.get(self.gc) {
                Some(list) => list,
                None => {
                    let list = HeapVector::new(self.gc, 1);
                    data.in_order_scripts.set(self.gc, Some(list));
                    list
                }
            };
            list.push(self.gc, element);
        }
        self.change_load_delay(document, true);
        key
    }

    /// The fetch `key` numbers is over: `loaded` its script, none if it
    /// failed. Its element's turn may come: an element task queued for it
    /// (`ElementTaskKind::Script`), or, in order, for each of its
    /// document's list from the first while they have been fetched.
    pub fn script_fetched(&self, key: u64, loaded: Option<LoadedScript>) {
        self.pending_fetched(key, loaded.map(ScriptResult::Classic));
    }

    /// The module graph `key` numbers is fetched (`queue_module_script`):
    /// its root's number (none if a module of it failed to fetch), as
    /// `script_fetched`; the parser's deferred one's turn is the parser's.
    pub fn module_graph_fetched(&self, key: u64, root: Option<u32>) {
        self.pending_fetched(key, root.map(ScriptResult::Module));
    }

    fn pending_fetched(&self, key: u64, loaded: Option<ScriptResult>) {
        let Some(element) = self
            .changes
            .scripts
            .fetching
            .borrow_mut()
            .remove(&key)
            .and_then(|id| self.node_by_id(id))
        else {
            return;
        };
        let Some(pending) = self
            .pending_script(element)
            .filter(|pending| pending.key.get() == key)
        else {
            return;
        };
        pending.loaded.set(loaded.map(Box::new));
        pending.fetched.set(true);
        let document = pending
            .document
            .get(self.gc)
            .expect("a pending script's document");
        if pending.parser.get() {
            return;
        }
        if !pending.in_order.get() {
            self.queue_script_task(element, document);
            return;
        }
        let Some(list) = self
            .document_data(document)
            .and_then(|data| data.in_order_scripts.get(self.gc))
        else {
            return;
        };
        while let Some(first) = list.get(self.gc, 0)
            && self
                .pending_script(first)
                .is_none_or(|pending| pending.fetched.get())
        {
            list.remove(self.gc, 0);
            self.queue_script_task(first, document);
        }
    }

    fn pending_script(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, PendingScript>> {
        self.existing_element_data(element)?.script.get(self.gc)
    }

    /// Whether `element`'s pending script was fetched (or failed): the
    /// parser's deferred module script's turn may come.
    pub fn pending_script_fetched(&self, element: NodeRef<'gc>) -> bool {
        self.pending_script(element)
            .is_some_and(|pending| pending.fetched.get())
    }

    /// `element`'s pending script, its turn come, taken (`ElementTaskKind::
    /// Script`): what it fetched, and its preparation-time document, whose
    /// load it no longer delays. None if it has none.
    pub fn take_ready_script(&self, element: NodeRef<'gc>) -> Option<ReadyScript<'gc>> {
        let data = self.existing_element_data(element)?;
        let pending = data.script.get(self.gc)?;
        data.script.set(self.gc, None);
        let document = pending
            .document
            .get(self.gc)
            .expect("a pending script's document");
        self.change_load_delay(document, false);
        Some(ReadyScript {
            document,
            loaded: pending.loaded.take().map(|loaded| *loaded),
            external: pending.external.get(),
        })
    }

    /// Whether `element` is an HTML `<link>` whose `rel` has the
    /// `modulepreload` keyword (ASCII case-insensitively).
    pub fn is_module_preload(&self, element: NodeRef<'gc>) -> bool {
        self.qualified_name(element) == Some(QualifiedName::html(names::local::LINK))
            && self
                .attribute_value_by_id(element, names::local::REL)
                .is_some_and(|rel| {
                    rel.borrow()
                        .to_str()
                        .split(|c: char| c.is_ascii_whitespace())
                        .any(|keyword| keyword.eq_ignore_ascii_case("modulepreload"))
                })
    }

    /// `element`'s attribute `local` changed (`form::control_attribute_changed`,
    /// as `rel` is a name of its): a `<link>` that is a modulepreload one
    /// now is seen, so that what is connected is looked at.
    pub(crate) fn link_rel_changed(&self, element: NodeRef<'gc>, local: names::NameId) {
        if local == names::local::REL && self.is_module_preload(element) {
            self.see(form::PRELOADS_SEEN);
        }
    }

    /// `node` connected (`frame::connection_steps`): a modulepreload link
    /// recorded, but in a shadow tree (as Blink ignores one there).
    pub(crate) fn link_connected(&self, node: NodeRef<'gc>) {
        if self.has_seen(form::PRELOADS_SEEN)
            && self.is_module_preload(node)
            && self.containing_shadow_root(node).is_none()
        {
            self.record_element_change(ElementChange::ModulePreload(self.node_id(node)));
        }
    }

    /// A connected element's attribute `name` changed from `old` to `new`
    /// (`frame::frame_owner_attribute_changed`): a modulepreload link's
    /// `rel`, `href`, `as` or `crossorigin` to another value recorded (the
    /// same value isn't processed again: whatwg/html#11400, as Blink's
    /// `HTMLLinkElementAttributeValueChanges`).
    pub(crate) fn link_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        let relevant = [
            names::local::REL,
            names::local::HREF,
            names::local::AS,
            names::local::CROSSORIGIN,
        ]
        .into_iter()
        .any(|local| name == QualifiedName::plain(local));
        if relevant && old != new && self.has_seen(form::PRELOADS_SEEN) {
            self.link_connected(element);
        }
    }
}
