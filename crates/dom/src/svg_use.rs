//! SVG's `<use>` (Blink's `SVGUseElement`): the element it references,
//! copied into its UA shadow tree (closed: script never gets it), drawn,
//! styled and hit as nodes of their own. A copy matches the rules of the
//! tree of the `<use>` it is in (Blink's `ScopedStyleResolverFor` takes its
//! corresponding element's) as itself: its tree ends at the copy's root, so
//! `defs rect` doesn't match a copy of a rect in `<defs>`; it inherits from
//! the `<use>`. A copy keeps the SVG elements and texts (those it doesn't
//! draw too, as Blink's), and a `<use>` in a copy has a copy of its own,
//! made with it.
//!
//! Each `<use>` connected outside a copy is kept with what its copy was
//! made from: the ids it and the `<use>`s in its copy looked up, and the
//! elements copied. Its copy is made again before style (Blink's
//! `SVGUseElement::BuildPendingResource`, its `IdTargetObserver`s and
//! `InvalidateInstances`) when it was connected, its `href` changed, one of
//! those ids finds another element now, or what it copied changed. The
//! host calls `update_use_trees` before each style update.

use std::cell::Cell;

use elise::Gc;
use rustc_hash::FxHashSet;

use crate::names;
use crate::{
    Cx, NodeId, NodeKind, NodeRef, QualifiedName, ShadowRootInit, ShadowRootMode,
    SlotAssignmentMode,
};

/// At most this many elements copied for one `<use>` outside a copy, with
/// those of the `<use>`s in its copy, or it draws nothing: nested `<use>`s
/// that each reference the one before twice over grow it exponentially.
/// Chrome copies them all; machopo stops a page from growing it without end.
pub const MAX_COPIES: usize = 10_000;

/// Whether the `<use>`s were ever made, and what each was copied from.
#[derive(Default)]
pub(crate) struct UseState {
    /// A `<use>` was made: what is connected, changed and removed is
    /// looked at.
    seen: Cell<bool>,
    /// Taken out for a change and put back.
    trees: Cell<Option<Box<UseTrees>>>,
}

#[derive(Default)]
struct UseTrees {
    uses: Vec<UseTree>,
    /// A `<use>` may have to be copied again, or forgotten: one came,
    /// left, or is dirty.
    stale: bool,
    /// An id came, went or changed: each lookup made again.
    ids_changed: bool,
    /// The elements the `<use>`s copied: what a change is looked up in.
    targets: FxHashSet<NodeId>,
}

/// A `<use>` outside a copy, and what its copy was made from.
struct UseTree {
    element: NodeId,
    /// The ids it and the `<use>`s in its copy looked up, with the element
    /// each found.
    lookups: Vec<(String, Option<NodeId>)>,
    /// The elements copied: its target, and the targets of the `<use>`s in
    /// its copy.
    targets: Vec<NodeId>,
    /// To be copied again.
    dirty: bool,
}

/// What a copy is being made with: the tree its ids are in, and what it
/// has found and copied so far.
struct Copying<'gc> {
    scope: NodeRef<'gc>,
    lookups: Vec<(String, Option<NodeId>)>,
    targets: Vec<NodeId>,
    copies: usize,
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `node` is an SVG `<use>`.
    pub fn is_svg_use(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node) == Some(QualifiedName::svg(names::local::USE))
    }

    fn is_svg_element(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node)
            .is_some_and(|name| name.namespace() == names::SVG_NAMESPACE)
    }

    /// The `<use>` whose copy `node` is in, if it is in one.
    pub fn use_tree_host(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let root = self.containing_shadow_root(node)?;
        let host = self.shadow_host(root)?;
        (self.shadow_root_init(root)?.mode == ShadowRootMode::UserAgent && self.is_svg_use(host))
            .then_some(host)
    }

    /// The `<use>` outside any copy that `node`'s copy is in, through the
    /// `<use>`s in copies: whose tree's rules it matches.
    pub fn outermost_use(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut host = self.use_tree_host(node)?;
        while let Some(outer) = self.use_tree_host(host) {
            host = outer;
        }
        Some(host)
    }

    /// The root of `user`'s copy (a `<use>`'s), if it has one.
    pub fn use_copy(&self, user: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let root = self.user_agent_shadow_root(user)?;
        self.first_child(root)
    }

    /// A `<use>` was made.
    pub(crate) fn svg_use_made(&self) {
        self.changes.svg_uses.seen.set(true);
    }

    /// Whether a `<use>` was ever made: before then, nothing is looked at.
    #[inline]
    pub(crate) fn has_seen_uses(&self) -> bool {
        self.changes.svg_uses.seen.get()
    }

    fn with_uses<R>(&self, f: impl FnOnce(&mut UseTrees) -> R) -> R {
        let cell = &self.changes.svg_uses.trees;
        let mut trees = cell.take().unwrap_or_default();
        let result = f(&mut trees);
        cell.set(Some(trees));
        result
    }

    /// `node` connected (or about to be disconnected), as its subtree is
    /// walked: a `<use>` outside a copy is kept (one that left is
    /// forgotten when the copies are next made); an id may find another
    /// element now. Out of line: the insertion's walk stays small.
    #[inline(never)]
    pub(crate) fn svg_use_node_moved(&self, node: NodeRef<'gc>, connected: bool) {
        let is_use = self.is_svg_use(node);
        let has_id = node.flags.get() & crate::style::HAS_ID != 0;
        if !(is_use || has_id) || self.use_tree_host(node).is_some() {
            return;
        }
        let id = self.node_id(node);
        self.with_uses(|trees| {
            if is_use {
                trees.stale = true;
                // One kept (it left and came back since): its ids may be
                // another tree's now.
                match trees.uses.iter_mut().find(|tree| tree.element == id) {
                    Some(tree) => tree.dirty |= connected,
                    None if connected => trees.uses.push(UseTree {
                        element: id,
                        lookups: Vec::new(),
                        targets: Vec::new(),
                        dirty: true,
                    }),
                    None => {}
                }
            }
            if has_id {
                trees.ids_changed = true;
            }
        });
    }

    /// `element`'s attribute `name` changed, and it is connected: a
    /// `<use>`'s reference is copied again, an id may find another element
    /// now, and what `element` is in may be copied.
    #[inline(never)]
    pub(crate) fn svg_use_attribute_changed(&self, element: NodeRef<'gc>, name: QualifiedName) {
        if self.use_tree_host(element).is_some() {
            return;
        }
        let id = name == QualifiedName::plain(names::ID);
        let href = name.local() == names::local::HREF
            && (name.namespace() == names::NONE || name.namespace() == names::XLINK_NAMESPACE);
        if id || (href && self.is_svg_use(element)) {
            let element = self.node_id(element);
            self.with_uses(|trees| {
                trees.ids_changed |= id;
                if let Some(tree) = trees.uses.iter_mut().find(|tree| tree.element == element) {
                    tree.dirty = true;
                    trees.stale = true;
                }
            });
        }
        self.svg_use_subtree_changed(element);
    }

    /// Something in `node`'s subtree changed (its attributes, its children,
    /// a text's data): the `<use>`s that copied it or what it is in are
    /// copied again.
    #[inline(never)]
    pub(crate) fn svg_use_subtree_changed(&self, node: NodeRef<'gc>) {
        let cell = &self.changes.svg_uses.trees;
        let Some(mut trees) = cell.take() else {
            return;
        };
        if !trees.targets.is_empty() {
            let mut at = Some(node);
            while let Some(ancestor) = at {
                let id = self.node_id(ancestor);
                if trees.targets.contains(&id) {
                    for tree in &mut trees.uses {
                        if tree.targets.contains(&id) {
                            tree.dirty = true;
                            trees.stale = true;
                        }
                    }
                }
                at = self.parent(ancestor);
            }
        }
        cell.set(Some(trees));
    }

    /// Makes again the copies of the `<use>`s that may draw otherwise, and
    /// forgets those no longer connected (their copies taken away). The
    /// host calls it before each style update.
    pub fn update_use_trees(&self) {
        let cell = &self.changes.svg_uses.trees;
        let Some(mut trees) = cell.take() else {
            return;
        };
        if !(trees.stale || trees.ids_changed) {
            cell.set(Some(trees));
            return;
        }
        let ids_changed = std::mem::take(&mut trees.ids_changed);
        trees.stale = false;
        let mut uses = std::mem::take(&mut trees.uses);
        // Put back while copies are made: what that inserts is looked at as
        // anything else is (and is in a copy: nothing is noted).
        cell.set(Some(trees));
        uses.retain_mut(|tree| {
            let Some(element) = self
                .node_by_id(tree.element)
                .filter(|element| self.is_connected(*element))
            else {
                if let Some(element) = self.node_by_id(tree.element) {
                    self.clear_use_copy(element);
                }
                return false;
            };
            if ids_changed && !tree.dirty {
                let scope = self.tree_scope_root(element);
                tree.dirty = tree.lookups.iter().any(|(id, found)| {
                    self.element_by_id(scope, id)
                        .map(|found| self.node_id(found))
                        != *found
                });
            }
            if tree.dirty {
                let (lookups, targets) = self.copy_use(element);
                *tree = UseTree {
                    element: tree.element,
                    lookups,
                    targets,
                    dirty: false,
                };
            }
            true
        });
        let mut trees = cell.take().unwrap_or_default();
        // What changed while copying was in copies.
        trees.stale = false;
        trees.ids_changed = false;
        trees.targets = uses
            .iter()
            .flat_map(|tree| tree.targets.iter().copied())
            .collect();
        trees.uses = uses;
        cell.set(Some(trees));
    }

    /// The root of the tree `node` is in: its shadow root's, or its
    /// document.
    fn tree_scope_root(&self, node: NodeRef<'gc>) -> NodeRef<'gc> {
        self.shadow_scope(node)
            .unwrap_or_else(|| self.document_of(node))
    }

    /// `user`'s copy taken away.
    fn clear_use_copy(&self, user: NodeRef<'gc>) {
        if let Some(root) = self.user_agent_shadow_root(user) {
            while let Some(child) = self.first_child(root) {
                self.remove(child);
            }
        }
    }

    /// Makes `user`'s copy again (Blink's `SVGUseElement::BuildShadowTree`):
    /// what it found by id, and the elements it copied. One of more than
    /// `MAX_COPIES` elements is taken away again.
    fn copy_use(&self, user: NodeRef<'gc>) -> (Vec<(String, Option<NodeId>)>, Vec<NodeId>) {
        self.clear_use_copy(user);
        let mut copying = Copying {
            scope: self.tree_scope_root(user),
            lookups: Vec::new(),
            targets: Vec::new(),
            copies: 0,
        };
        if !self.copy_into(user, &mut copying) {
            self.clear_use_copy(user);
        }
        (copying.lookups, copying.targets)
    }

    /// Copies what `user` references into its UA shadow root (made if it
    /// has none), and what the `<use>`s in the copy reference into theirs:
    /// nothing for a reference to no element, to one a copy doesn't keep
    /// (outside SVG), or a cycle. False once more than `MAX_COPIES` elements were copied.
    fn copy_into(&self, user: NodeRef<'gc>, copying: &mut Copying<'gc>) -> bool {
        let Some(id) = self.use_reference(user) else {
            return true;
        };
        let target = self.element_by_id(copying.scope, &id);
        copying
            .lookups
            .push((id, target.map(|target| self.node_id(target))));
        let Some(target) = target.filter(|target| self.is_copied(*target)) else {
            return true;
        };
        if self.has_cycle(user, target) {
            return true;
        }
        copying.targets.push(self.node_id(target));
        let document = self.document_of(user);
        let Some(copy) = self.copy_subtree(target, document, &mut copying.copies) else {
            return false;
        };
        let root = match self.user_agent_shadow_root(user) {
            Some(root) => root,
            None => self.attach_shadow(
                user,
                ShadowRootInit {
                    mode: ShadowRootMode::UserAgent,
                    delegates_focus: false,
                    slot_assignment: SlotAssignmentMode::Named,
                    clonable: false,
                    serializable: false,
                },
            ),
        };
        self.insert(root, copy, None);
        // The `<use>`s in the copy, in tree order, the root too.
        let mut nested = Vec::new();
        self.for_each_inclusive_descendant(copy, |node| {
            if self.is_svg_use(node) {
                nested.push(node);
            }
        });
        nested
            .into_iter()
            .all(|nested| self.copy_into(nested, copying))
    }

    /// Whether `element` is one a copy keeps: an SVG element, those it
    /// doesn't draw too (Blink's copies keep them, for selectors: `defs +
    /// rect`), but a `<script>`, which a copy never runs.
    fn is_copied(&self, element: NodeRef<'gc>) -> bool {
        self.qualified_name(element).is_some_and(|name| {
            name.namespace() == names::SVG_NAMESPACE && name.local() != names::local::SCRIPT
        })
    }

    /// A copy of `node` in `document` and of what of its subtree a copy
    /// keeps (its SVG elements, texts), its elements counted into
    /// `copies`: none once that is more than `MAX_COPIES`.
    fn copy_subtree(
        &self,
        node: NodeRef<'gc>,
        document: NodeRef<'gc>,
        copies: &mut usize,
    ) -> Option<NodeRef<'gc>> {
        if self.kind(node) == NodeKind::Element {
            // Only compared with the limit: saturating doesn't change that.
            *copies = copies.saturating_add(1);
            if *copies > MAX_COPIES {
                return None;
            }
        }
        let copy = self.clone_node(node, Some(document), false);
        let mut child = self.first_child(node);
        while let Some(at) = child {
            let kept = match self.kind(at) {
                NodeKind::Element => self.is_copied(at),
                NodeKind::Text => true,
                _ => false,
            };
            if kept {
                let child_copy = self.copy_subtree(at, document, copies)?;
                self.insert(copy, child_copy, None);
            }
            child = self.next_sibling(at);
        }
        Some(copy)
    }

    /// The id `user` (a `<use>`) references: its `href`, or SVG 1.1's
    /// `xlink:href` without one, as a fragment of its document (`#id`); an
    /// external reference (`icons.svg#check`) needs an XML parser machopo
    /// hasn't.
    fn use_reference(&self, user: NodeRef<'gc>) -> Option<String> {
        let href = self
            .attribute_value_by_id(user, names::local::HREF)
            .or_else(|| {
                self.attribute_value_ns(user, Some("http://www.w3.org/1999/xlink"), "href")
            })?;
        css::svg::fragment_id(&href.borrow().to_str())
    }

    /// Whether `user` copying `target` is a cycle (Blink's
    /// `SVGUseElement::HasCycleUseReferencing`): `target` is `user`, or has
    /// the id of an SVG element `user` is in, through the copies it is in
    /// and the `<use>`s whose copies they are (copies have their elements'
    /// ids), up to the first that isn't one.
    fn has_cycle(&self, user: NodeRef<'gc>, target: NodeRef<'gc>) -> bool {
        if Gc::ptr_eq(user, target) {
            return true;
        }
        let Some(id) = self.attribute_value_by_id(target, names::ID) else {
            return false;
        };
        let id = id.borrow().to_str().into_owned();
        let mut at = self.parent_or_shadow_host_element(user);
        while let Some(ancestor) = at.filter(|at| self.is_svg_element(*at)) {
            if self
                .attribute_value_by_id(ancestor, names::ID)
                .is_some_and(|own| *own.borrow().to_str() == *id)
            {
                return true;
            }
            at = self.parent_or_shadow_host_element(ancestor);
        }
        false
    }

    /// `node`'s parent element, or for a child of a shadow root its host
    /// (Blink's `ParentOrShadowHostElement`).
    fn parent_or_shadow_host_element(&self, node: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let parent = self.parent(node)?;
        if self.is_shadow_root(parent) {
            return self.shadow_host(parent);
        }
        (self.kind(parent) == NodeKind::Element).then_some(parent)
    }
}
