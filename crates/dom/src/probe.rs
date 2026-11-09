//! What changed in the DOM's trees, for DevTools' `DOM` agent (Blink's
//! `probe::DidInsertDOMNode`, `WillRemoveDOMNode`, `DidModifyDOMAttr`,
//! `CharacterDataModified`, `DidInvalidateStyleAttr` and
//! `DidPushShadowRoot`, which `InspectorDOMAgent` hears): while the host
//! wants them (`Dom::set_dom_changes_wanted`), each change's kind and
//! nodes, in order, until it takes them (`Dom::take_dom_changes`).
//!
//! Blink's agent is told as each change happens, and builds its events
//! then. The host here takes the changes after the operations that made
//! them, in a batch, and builds the events from the DOM as it is then: the
//! DOM doesn't call its host in an operation, nor is a node read for
//! DevTools while its tree is half changed. With no one wanting them, each
//! change reads one flag more.

use std::cell::{Cell, RefCell};

use crate::{Cx, Dom, NodeId, NodeRef, QualifiedName};

/// A change of a tree, by its nodes' ids (`Cx::node_id`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomChange {
    /// `node` was put in `parent`'s children, after `previous` (first if
    /// none).
    Inserted {
        parent: NodeId,
        node: NodeId,
        previous: Option<NodeId>,
    },
    /// `node` was taken out of `parent`'s children.
    Removed { parent: NodeId, node: NodeId },
    /// `element`'s attribute `name` was set or removed.
    Attribute {
        element: NodeId,
        name: QualifiedName,
    },
    /// A character data node's data changed.
    CharacterData { node: NodeId },
    /// `element`'s inline style changed through CSSOM, its `style`
    /// attribute to be written from it when next read.
    InlineStyle { element: NodeId },
    /// `host` got the shadow root `root`.
    ShadowAttached { host: NodeId, root: NodeId },
}

/// Whether the host wants the changes, and those since it last took them.
/// `RefCell`: reached through `&Cx`, its borrow never held past a push.
#[derive(Default)]
pub(crate) struct DomChanges {
    wanted: Cell<bool>,
    changes: RefCell<Vec<DomChange>>,
}

impl Dom {
    /// Whether the host wants the trees' changes (DevTools' `DOM` is on in
    /// a session): the DOM notes them only while it does, and drops those
    /// noted once it doesn't.
    pub fn set_dom_changes_wanted(&self, wanted: bool) {
        let changes = &self.changes.dom_changes;
        changes.wanted.set(wanted);
        if !wanted {
            changes.changes.borrow_mut().clear();
        }
    }

    /// The trees' changes since the host last took them, in order.
    pub fn take_dom_changes(&self) -> Vec<DomChange> {
        std::mem::take(&mut *self.changes.dom_changes.changes.borrow_mut())
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether the trees' changes are noted: the one read each change has
    /// when the host doesn't want them.
    #[inline]
    pub(crate) fn dom_changes_wanted(&self) -> bool {
        self.changes.dom_changes.wanted.get()
    }

    #[cold]
    fn note_dom_change(&self, change: DomChange) {
        self.changes.dom_changes.changes.borrow_mut().push(change);
    }

    #[cold]
    pub(crate) fn note_inserted(
        &self,
        parent: NodeRef<'gc>,
        node: NodeRef<'gc>,
        previous: Option<NodeRef<'gc>>,
    ) {
        self.note_dom_change(DomChange::Inserted {
            parent: self.node_id(parent),
            node: self.node_id(node),
            previous: previous.map(|previous| self.node_id(previous)),
        });
    }

    #[cold]
    pub(crate) fn note_removed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        self.note_dom_change(DomChange::Removed {
            parent: self.node_id(parent),
            node: self.node_id(node),
        });
    }

    #[cold]
    pub(crate) fn note_attribute(&self, element: NodeRef<'gc>, name: QualifiedName) {
        self.note_dom_change(DomChange::Attribute {
            element: self.node_id(element),
            name,
        });
    }

    #[cold]
    pub(crate) fn note_character_data(&self, node: NodeRef<'gc>) {
        self.note_dom_change(DomChange::CharacterData {
            node: self.node_id(node),
        });
    }

    #[cold]
    pub(crate) fn note_inline_style(&self, element: NodeRef<'gc>) {
        self.note_dom_change(DomChange::InlineStyle {
            element: self.node_id(element),
        });
    }

    #[cold]
    pub(crate) fn note_shadow_attached(&self, host: NodeRef<'gc>) {
        if let Some(root) = self.shadow_root(host) {
            self.note_dom_change(DomChange::ShadowAttached {
                host: self.node_id(host),
                root: self.node_id(root),
            });
        }
    }
}
