//! Shadow trees (DOM's, Blink's `ShadowRoot` and `TreeScope`): a shadow
//! root is a document fragment whose data is a `ShadowRootData` (its host
//! and settings), its host's element data pointing back at it. A node in a
//! shadow tree has its shadow root as its owner (its tree scope's root,
//! Blink's `tree_scope_`) and `IN_SHADOW_TREE` set; its node document is
//! its shadow root's owner. The tree scope of a node is so one read; a
//! subtree going into or out of a shadow tree has its owners moved
//! (`set_shadow_scope`, Blink's `TreeScopeAdopter`).

use std::cell::Cell;

use elise::{Gc, GcObject, HeapVector, Member};

use crate::{Cx, Node, NodeKind, NodeRareData, NodeRef, names, sites, style};

thread_local! {
    /// How many times, on this thread, a node was retargeted against
    /// another (`Cx::retarget`). Counted in debug builds only.
    static RETARGETS: Cell<u64> = const { Cell::new(0) };
}

/// How many times, on this thread, a node was retargeted against another:
/// what tells a dispatch's path through shadow trees retargets once a
/// tree scope it goes through, not once a node. `None` in release builds.
/// For tests.
pub fn retargets_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| RETARGETS.get())
}

/// `Node::scope`: a node in a shadow tree (its owner is its shadow root);
/// a shadow root; an element hosting one.
pub(crate) const IN_SHADOW_TREE: u8 = 1 << 0;
pub(crate) const SHADOW_ROOT: u8 = 1 << 1;
pub(crate) const SHADOW_HOST: u8 = 1 << 2;

/// A shadow root's data: its host, its settings (`ShadowRootInit`, as
/// bits) and its listeners and observers.
#[derive(GcObject)]
pub struct ShadowRootData {
    host: Member<Node>,
    settings: Cell<u8>,
    pub(crate) rare: Member<NodeRareData>,
    /// Its slots in tree order (`slot`), and which of it and the
    /// assignment are out of date (`slot::SLOTS_STALE`,
    /// `ASSIGNMENT_STALE`).
    pub(crate) slots: Member<HeapVector<Node>>,
    pub(crate) slot_state: Cell<u8>,
    /// It is in the DOM's list of shadow roots whose sheets may have
    /// changed (`take_sheet_changed_roots`).
    pub(crate) sheets_listed: Cell<bool>,
    /// Its adopted sheets (`adoptedStyleSheets`), once it has had some.
    pub(crate) adopted: Member<HeapVector<crate::style_sheet::CssStyleSheet>>,
    /// Its `styleSheets`, once script asked for it.
    pub(crate) style_sheets: Member<crate::StyleSheetList>,
    /// What matching selectors found its children depend on of it as their
    /// parent (`Cx::selector_flags`), only ever added to.
    pub(crate) selector_flags: Cell<u16>,
}

/// `ShadowRootData::settings`: the mode in the low bits.
const MODE_MASK: u8 = 0b11;
const DELEGATES_FOCUS: u8 = 1 << 2;
const MANUAL_SLOTS: u8 = 1 << 3;
const CLONABLE: u8 = 1 << 4;
const SERIALIZABLE: u8 = 1 << 5;
/// DOM's "declarative": the parser attached it (`<template
/// shadowrootmode>`), and no `attachShadow` has taken it over since.
const DECLARATIVE: u8 = 1 << 6;

/// The HTML elements DOM's "attach a shadow root" takes besides a valid
/// custom element name's.
const SHADOW_HOSTS: [crate::NameId; 18] = [
    names::local::ARTICLE,
    names::local::ASIDE,
    names::local::BLOCKQUOTE,
    names::local::BODY,
    names::local::DIV,
    names::local::FOOTER,
    names::local::H1,
    names::local::H2,
    names::local::H3,
    names::local::H4,
    names::local::H5,
    names::local::H6,
    names::local::HEADER,
    names::local::MAIN,
    names::local::NAV,
    names::local::P,
    names::local::SECTION,
    names::local::SPAN,
];

/// Why DOM's "attach a shadow root" can't attach one to an element, before
/// it looks at the one it may have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowHostError {
    /// Not an HTML element of the list, nor one with a valid custom
    /// element name.
    NotAHost,
    /// Its name's custom element definition's `disabledFeatures` has
    /// `shadow`.
    Disabled,
}

/// A shadow root's mode (DOM's; `UserAgent` is Blink's, for the form
/// controls' trees: closed to script).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowRootMode {
    Open,
    Closed,
    UserAgent,
}

/// How a shadow root's slots get their nodes (DOM's "slot assignment").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotAssignmentMode {
    Named,
    Manual,
}

/// What a shadow root is made with (`attachShadow`'s `ShadowRootInit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowRootInit {
    pub mode: ShadowRootMode,
    pub delegates_focus: bool,
    pub slot_assignment: SlotAssignmentMode,
    pub clonable: bool,
    pub serializable: bool,
}

impl ShadowRootInit {
    fn bits(self) -> u8 {
        let mode = match self.mode {
            ShadowRootMode::Open => 0,
            ShadowRootMode::Closed => 1,
            ShadowRootMode::UserAgent => 2,
        };
        [
            (self.delegates_focus, DELEGATES_FOCUS),
            (
                self.slot_assignment == SlotAssignmentMode::Manual,
                MANUAL_SLOTS,
            ),
            (self.clonable, CLONABLE),
            (self.serializable, SERIALIZABLE),
        ]
        .into_iter()
        .filter(|&(on, _)| on)
        .fold(mode, |bits, (_, bit)| bits | bit)
    }

    fn from_bits(bits: u8) -> Self {
        Self {
            mode: match bits & MODE_MASK {
                0 => ShadowRootMode::Open,
                1 => ShadowRootMode::Closed,
                _ => ShadowRootMode::UserAgent,
            },
            delegates_focus: bits & DELEGATES_FOCUS != 0,
            slot_assignment: if bits & MANUAL_SLOTS != 0 {
                SlotAssignmentMode::Manual
            } else {
                SlotAssignmentMode::Named
            },
            clonable: bits & CLONABLE != 0,
            serializable: bits & SERIALIZABLE != 0,
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// A new shadow root of `host`, an element with none (DOM's "attach a
    /// shadow root", its checks the caller's): in `host`'s document,
    /// connected if `host` is.
    pub fn attach_shadow(&self, host: NodeRef<'gc>, init: ShadowRootInit) -> NodeRef<'gc> {
        debug_assert!(self.kind(host) == NodeKind::Element && self.shadow_root(host).is_none());
        let root = self.create_document_fragment(self.document_of(host));
        let data = self.gc.alloc_in(
            &sites::OTHER,
            ShadowRootDataInit {
                host: Some(host),
                settings: Cell::new(init.bits()),
                rare: None,
                slots: None,
                slot_state: Cell::new(crate::slot::SLOTS_STALE | crate::slot::ASSIGNMENT_STALE),
                sheets_listed: Cell::new(false),
                adopted: None,
                style_sheets: None,
                selector_flags: Cell::new(0),
            },
        );
        root.data.set(self.gc, Some(data.erase()));
        root.scope.set(root.scope.get() | SHADOW_ROOT);
        host.scope.set(host.scope.get() | SHADOW_HOST);
        self.assignment_stale(root);
        self.element_data(host).shadow_root.set(self.gc, Some(root));
        if self.is_connected(host) {
            self.set_flag(root, style::CONNECTED, true);
        }
        self.shadow_attached(host);
        root
    }

    /// DOM's "attach a shadow root"'s checks of `element` itself (an HTML
    /// element of the list or with a valid custom element name, whose
    /// definition, looked up by its name, doesn't disable shadows): the
    /// shadow root it may have is the caller's to look at.
    pub fn shadow_host_check(&self, element: NodeRef<'gc>) -> Result<(), ShadowHostError> {
        let name = self
            .element_name(element)
            .ok_or(ShadowHostError::NotAHost)?;
        if name.namespace != names::HTML_NAMESPACE {
            return Err(ShadowHostError::NotAHost);
        }
        let custom = !SHADOW_HOSTS.contains(&name.local)
            && crate::custom_element::is_valid_custom_element_name(&names::get(name.local));
        if !custom && !SHADOW_HOSTS.contains(&name.local) {
            return Err(ShadowHostError::NotAHost);
        }
        if custom
            && self
                .custom_element_definition_for(element)
                .is_some_and(|definition| self.definition_disabled(definition).1)
        {
            return Err(ShadowHostError::Disabled);
        }
        Ok(())
    }

    /// Whether `root` is declarative (DOM's: the parser attached it and no
    /// `attachShadow` has taken it over).
    pub fn is_declarative_shadow_root(&self, root: NodeRef<'gc>) -> bool {
        self.shadow_data(root)
            .is_some_and(|data| data.settings.get() & DECLARATIVE != 0)
    }

    pub fn set_shadow_root_declarative(&self, root: NodeRef<'gc>, declarative: bool) {
        if let Some(data) = self.shadow_data(root) {
            let settings = data.settings.get() & !DECLARATIVE;
            data.settings.set(if declarative {
                settings | DECLARATIVE
            } else {
                settings
            });
        }
    }

    /// `element`'s shadow root, if it hosts one (whatever its mode).
    pub fn shadow_root(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if element.scope.get() & SHADOW_HOST == 0 {
            return None;
        }
        self.existing_element_data(element)?
            .into_ref()
            .shadow_root
            .get(self.gc)
    }

    pub(crate) fn shadow_data(&self, node: NodeRef<'gc>) -> Option<Gc<'gc, ShadowRootData>> {
        if node.scope.get() & SHADOW_ROOT == 0 {
            return None;
        }
        node.data.get(self.gc)?.downcast::<ShadowRootData>()
    }

    pub(crate) fn shadow_rare(&self, root: NodeRef<'gc>) -> Option<&'gc Member<NodeRareData>> {
        Some(&self.shadow_data(root)?.into_ref().rare)
    }

    /// Whether `node` hosts a shadow root.
    #[inline]
    pub fn is_shadow_host(&self, node: NodeRef<'gc>) -> bool {
        node.scope.get() & SHADOW_HOST != 0
    }

    /// Whether `node` is a shadow root.
    #[inline]
    pub fn is_shadow_root(&self, node: NodeRef<'gc>) -> bool {
        node.scope.get() & SHADOW_ROOT != 0
    }

    /// A shadow root's host; none for another node.
    pub fn shadow_host(&self, root: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.shadow_data(root)?.host.get(self.gc)
    }

    /// A shadow root's settings; none for another node.
    pub fn shadow_root_init(&self, root: NodeRef<'gc>) -> Option<ShadowRootInit> {
        Some(ShadowRootInit::from_bits(
            self.shadow_data(root)?.settings.get(),
        ))
    }

    /// The shadow root `node` is in, if it is in a shadow tree (a shadow
    /// root itself is in none): its owner, one read.
    #[inline]
    pub fn containing_shadow_root(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if node.scope.get() & IN_SHADOW_TREE == 0 {
            return None;
        }
        node.owner.get(self.gc)
    }

    /// The shadow root whose tree `node` is in or is, if any.
    #[inline]
    pub(crate) fn shadow_scope(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        if node.scope.get() & SHADOW_ROOT != 0 {
            return Some(node);
        }
        self.containing_shadow_root(node)
    }

    /// `node`'s parent, or for a shadow root its host (DOM's
    /// "shadow-including" walks).
    pub fn parent_or_shadow_host(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.parent(node).or_else(|| self.shadow_host(node))
    }

    /// `root` and the nodes of its tree (not of the shadow trees in it)
    /// moved into `scope`'s tree (a shadow root's), or out of any shadow
    /// tree, into their document's: their owners and `IN_SHADOW_TREE`
    /// (Blink's `TreeScopeAdopter`). Nothing to do for a subtree already
    /// there.
    #[inline]
    pub(crate) fn set_shadow_scope(&self, root: NodeRef<'gc>, scope: Option<NodeRef<'gc>>) {
        // Into the light tree, from it: nothing (most insertions and
        // removals), inline.
        if scope.is_none() && root.scope.get() & IN_SHADOW_TREE == 0 {
            return;
        }
        self.move_shadow_scope(root, scope);
    }

    #[inline(never)]
    fn move_shadow_scope(&self, root: NodeRef<'gc>, scope: Option<NodeRef<'gc>>) {
        let current = self.containing_shadow_root(root);
        let same = match (current, scope) {
            (Some(a), Some(b)) => Gc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        let owner = match scope {
            Some(scope) => scope,
            None => self.document_of(root),
        };
        self.for_each_inclusive_descendant(root, |at| {
            at.owner.set(self.gc, Some(owner));
            let bits = at.scope.get();
            at.scope.set(if scope.is_some() {
                bits | IN_SHADOW_TREE
            } else {
                bits & !IN_SHADOW_TREE
            });
        });
    }

    /// Whether `node` is `root` (a shadow root) or of its shadow-including
    /// descendants: up the tree scopes from `node`'s, a read each (not
    /// the tree's depth).
    pub fn is_in_shadow_tree_of(&self, root: NodeRef<'gc>, node: NodeRef<'gc>) -> bool {
        let mut scope = self.shadow_scope(node);
        while let Some(at) = scope {
            if Gc::ptr_eq(at, root) {
                return true;
            }
            scope = self
                .shadow_host(at)
                .and_then(|host| self.shadow_scope(host));
        }
        false
    }

    /// DOM's "retarget" `a` against `b` (none: not a node, a window): out
    /// of the shadow trees `b` isn't in, on to their hosts. `a` itself when
    /// it is in no shadow tree, a read.
    pub fn retarget(&self, a: NodeRef<'gc>, b: Option<NodeRef<'gc>>) -> NodeRef<'gc> {
        if cfg!(debug_assertions) {
            RETARGETS.set(RETARGETS.get().wrapping_add(1));
        }
        let mut a = a;
        while let Some(root) = self.shadow_scope(a) {
            if b.is_some_and(|b| self.is_in_shadow_tree_of(root, b)) {
                return a;
            }
            a = self.shadow_host(root).expect("a shadow root has its host");
        }
        a
    }

    /// DOM's "closed-shadow-hidden": whether `a` is in a closed shadow tree
    /// (or one in one) that `b` isn't in. A user agent's tree is closed.
    pub fn is_closed_shadow_hidden(&self, a: NodeRef<'gc>, b: NodeRef<'gc>) -> bool {
        let mut scope = self.shadow_scope(a);
        while let Some(root) = scope {
            if self.is_in_shadow_tree_of(root, b) {
                return false;
            }
            if self
                .shadow_root_init(root)
                .is_some_and(|init| init.mode != ShadowRootMode::Open)
            {
                return true;
            }
            scope = self
                .shadow_host(root)
                .and_then(|host| self.shadow_scope(host));
        }
        false
    }

    /// `getRootNode()`: `node`'s root (DOM's), or with `composed` its
    /// shadow-including root. A read for a node in a shadow tree or a
    /// connected one; else up to the top of its tree.
    pub fn root_node(&self, node: NodeRef<'gc>, composed: bool) -> NodeRef<'gc> {
        let mut node = node;
        if composed {
            while let Some(root) = self.shadow_scope(node) {
                node = self.shadow_host(root).expect("a shadow root has its host");
            }
        } else if let Some(root) = self.shadow_scope(node) {
            return root;
        }
        if self.is_connected(node) {
            return self.document_of(node);
        }
        while let Some(parent) = self.parent(node) {
            node = parent;
        }
        node
    }

    /// `f` of `root` and its shadow-including descendants (DOM's): those of
    /// the shadow trees of the hosts in it too, each tree in order; with
    /// whether the node is in a shadow tree (or is a shadow root), known a
    /// tree at a time (the nodes of one tree share their tree scope). Every
    /// insertion and removal walks this way: the light tree as before, a
    /// bit more read of each node, its hosts' trees out of line.
    #[inline]
    pub(crate) fn for_each_shadow_including_inclusive_descendant(
        &self,
        root: NodeRef<'gc>,
        mut f: impl FnMut(NodeRef<'gc>, bool),
    ) {
        let in_shadow = self.shadow_scope(root).is_some();
        self.for_each_inclusive_descendant(root, |at| {
            f(at, in_shadow);
            if at.scope.get() & SHADOW_HOST != 0 {
                self.for_each_in_shadow_trees(at, &mut f);
            }
        });
    }

    /// `f` of the nodes of `host`'s shadow tree and of those in it.
    #[cold]
    #[inline(never)]
    fn for_each_in_shadow_trees(&self, host: NodeRef<'gc>, f: &mut dyn FnMut(NodeRef<'gc>, bool)) {
        let Some(root) = self.shadow_root(host) else {
            return;
        };
        self.for_each_inclusive_descendant(root, |at| {
            f(at, true);
            if at.scope.get() & SHADOW_HOST != 0 {
                self.for_each_in_shadow_trees(at, f);
            }
        });
    }
}
