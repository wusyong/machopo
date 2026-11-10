//! What changed in the DOM's trees, for DevTools' `DOM` agent (Blink's
//! `probe::DidInsertDOMNode`, `WillRemoveDOMNode`, `DidModifyDOMAttr`,
//! `CharacterDataModified`, `DidInvalidateStyleAttr`, `DidPushShadowRoot`
//! and `PseudoElementCreated` and `PseudoElementDestroyed`, which
//! `InspectorDOMAgent` hears): while the host
//! wants them (`Dom::set_dom_changes_wanted`), each change's kind and
//! nodes, in order, until it takes them (`Dom::take_dom_changes`).
//!
//! Blink's agent is told as each change happens, and builds its events
//! then. The host here takes the changes after the operations that made
//! them, in a batch, and builds the events from the DOM as it is then: the
//! DOM doesn't call its host in an operation, nor is a node read for
//! DevTools while its tree is half changed. With no one wanting them, each
//! change reads one flag more.
//!
//! And the pseudo-classes DevTools forces on elements (`CSS.forcePseudoState`,
//! Blink's `probe::ForcePseudoState`, which `InspectorCSSAgent` answers):
//! matching asks for an element's (`Cx::forced_pseudo_class`), one flag read
//! while none is forced anywhere.

use std::cell::{Cell, RefCell};

use css::selector::PseudoClass;

use crate::{AttributeChanges, Cx, Dom, NodeId, NodeKind, NodeRef, QualifiedName};

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
    /// The style phase worked out `element`'s pseudo-elements' styles
    /// otherwise: one may have come or gone.
    PseudoElements { element: NodeId },
}

/// Whether the host wants the changes, and those since it last took them.
/// `RefCell`: reached through `&Cx`, its borrow never held past a push.
#[derive(Default)]
pub(crate) struct DomChanges {
    wanted: Cell<bool>,
    changes: RefCell<Vec<DomChange>>,
}

/// The pseudo-classes forced on an element, a bit each: the names Chrome
/// takes (Blink's `InspectorCSSAgent::ComputePseudoClassMask`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ForcedPseudoClasses(u32);

/// Those of both: every session's together.
impl std::ops::BitOr for ForcedPseudoClasses {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl ForcedPseudoClasses {
    /// The names, by their bits' places.
    const NAMES: [&'static str; 24] = [
        "active",
        "hover",
        "focus",
        "focus-within",
        "focus-visible",
        "target",
        // Taken, never matched: no link matches `:visited`.
        "visited",
        "enabled",
        "disabled",
        "checked",
        "indeterminate",
        "placeholder-shown",
        "required",
        "optional",
        "read-only",
        "read-write",
        "open",
        // Taken, nothing to match: no rule has them (selectors with them
        // don't parse).
        "valid",
        "invalid",
        "user-valid",
        "user-invalid",
        "in-range",
        "out-of-range",
        "autofill",
    ];

    /// Those of `names` it takes, the others (`link`, `default`...)
    /// passed over as Chrome does.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        Self(names.into_iter().fold(0, |bits, name| {
            Self::NAMES
                .iter()
                .position(|known| *known == name)
                .map_or(bits, |place| bits | 1 << place)
        }))
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn has(self, name: &str) -> bool {
        self.0 & Self::from_names([name]).0 != 0
    }

    /// Whether `:focus` or `:focus-within` is of them: what makes the
    /// element's ancestors match `:focus-within`.
    fn has_focus(self) -> bool {
        self.has("focus") || self.has("focus-within")
    }

    /// The name of `class` and of the one it can't match with (`:enabled`
    /// and `:disabled`...), if it is one forced.
    fn names_of(class: &PseudoClass) -> Option<(&'static str, Option<&'static str>)> {
        Some(match class {
            PseudoClass::Active => ("active", None),
            PseudoClass::Hover => ("hover", None),
            PseudoClass::Focus => ("focus", None),
            PseudoClass::FocusWithin => ("focus-within", None),
            PseudoClass::FocusVisible => ("focus-visible", None),
            PseudoClass::Target => ("target", None),
            PseudoClass::Enabled => ("enabled", Some("disabled")),
            PseudoClass::Disabled => ("disabled", Some("enabled")),
            PseudoClass::Checked => ("checked", None),
            PseudoClass::Indeterminate => ("indeterminate", None),
            PseudoClass::PlaceholderShown => ("placeholder-shown", None),
            PseudoClass::Required => ("required", Some("optional")),
            PseudoClass::Optional => ("optional", Some("required")),
            PseudoClass::ReadOnly => ("read-only", Some("read-write")),
            PseudoClass::ReadWrite => ("read-write", Some("read-only")),
            PseudoClass::Open => ("open", None),
            _ => return None,
        })
    }

    /// The states (`AttributeChanges` bits) of its pseudo-classes, whose
    /// rules match otherwise once they are forced or not.
    fn states(self) -> u16 {
        const STATES: [(&str, u16); 16] = [
            ("active", AttributeChanges::ACTIVE),
            ("hover", AttributeChanges::HOVER),
            ("focus", AttributeChanges::FOCUS),
            ("focus-within", AttributeChanges::FOCUS_WITHIN),
            ("focus-visible", AttributeChanges::FOCUS_VISIBLE),
            ("target", AttributeChanges::TARGET),
            ("enabled", AttributeChanges::DISABLED),
            ("disabled", AttributeChanges::DISABLED),
            ("checked", AttributeChanges::CHECKED),
            ("indeterminate", AttributeChanges::INDETERMINATE),
            ("placeholder-shown", AttributeChanges::VALUE),
            ("required", AttributeChanges::REQUIRED),
            ("optional", AttributeChanges::REQUIRED),
            ("read-only", AttributeChanges::DISABLED),
            ("read-write", AttributeChanges::DISABLED),
            ("open", AttributeChanges::OPEN),
        ];
        STATES
            .iter()
            .filter(|(name, _)| self.has(name))
            .fold(0, |states, (_, state)| states | state)
    }
}

/// The elements DevTools forces pseudo-classes on, by their ids, and
/// whether there are any: what matching reads first. `RefCell`: reached
/// through `&Cx`, its borrow never held past a look or a change.
#[derive(Default)]
pub(crate) struct ForcedStates {
    any: Cell<bool>,
    elements: RefCell<Vec<(NodeId, ForcedPseudoClasses)>>,
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
    pub(crate) fn note_pseudo_elements(&self, element: NodeRef<'gc>) {
        self.note_dom_change(DomChange::PseudoElements {
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

    /// Whether DevTools forces pseudo-classes on an element: the one read
    /// matching and a removal have while it doesn't.
    #[inline]
    pub(crate) fn has_forced_pseudo_classes(&self) -> bool {
        self.changes.forced.any.get()
    }

    /// What `class` matches on `element` as DevTools forces it: true if it
    /// is forced, false if the one it can't match with is (`:enabled` once
    /// `disabled` is), none to match it as the DOM is. `:focus-within`
    /// also on an ancestor of an element it or `:focus` is forced on (not
    /// on one `:focus` is: Chrome's).
    #[inline]
    pub fn forced_pseudo_class(&self, element: NodeRef<'gc>, class: &PseudoClass) -> Option<bool> {
        if !self.has_forced_pseudo_classes() {
            return None;
        }
        self.forced_match(element, class)
    }

    #[cold]
    fn forced_match(&self, element: NodeRef<'gc>, class: &PseudoClass) -> Option<bool> {
        let (name, opposite) = ForcedPseudoClasses::names_of(class)?;
        let forced = self.forced_pseudo_classes(element);
        if forced.has(name) {
            return Some(true);
        }
        if opposite.is_some_and(|opposite| forced.has(opposite)) {
            return Some(false);
        }
        if *class == PseudoClass::FocusWithin && self.has_forced_focus_below(element) {
            return Some(true);
        }
        None
    }

    /// Whether an element forced to have the focus, or one in it, is below
    /// `element` in the flat tree (where the focus chain goes,
    /// `user_action`).
    fn has_forced_focus_below(&self, element: NodeRef<'gc>) -> bool {
        let focused: Vec<NodeId> = self
            .changes
            .forced
            .elements
            .borrow()
            .iter()
            .filter(|(_, forced)| forced.has_focus())
            .map(|&(id, _)| id)
            .collect();
        focused
            .into_iter()
            .filter_map(|id| self.node_by_id(id))
            .any(|focused| {
                std::iter::successors(self.flat_tree_parent(focused), |&node| {
                    self.flat_tree_parent(node)
                })
                .any(|node| elise::Gc::ptr_eq(node, element))
            })
    }

    /// The pseudo-classes DevTools forces on `element`.
    pub fn forced_pseudo_classes(&self, element: NodeRef<'gc>) -> ForcedPseudoClasses {
        // One with no id is none of them (`node_id` would give it one).
        let id = element.id.get();
        if id == 0 {
            return ForcedPseudoClasses::default();
        }
        let id = NodeId::from_raw(id);
        self.changes
            .forced
            .elements
            .borrow()
            .iter()
            .find(|&&(at, _)| at == id)
            .map_or_else(ForcedPseudoClasses::default, |&(_, forced)| forced)
    }

    /// DevTools forces `classes` on `element` (an element's; none: it
    /// forces none), in place of those before: the rules of those that
    /// changed match again (`state_changed`), and with the focus (or one in
    /// it) forced or not, its flat tree ancestors' `:focus-within`'s.
    pub fn set_forced_pseudo_classes(&self, element: NodeRef<'gc>, classes: ForcedPseudoClasses) {
        let old = self.forced_pseudo_classes(element);
        if old == classes || self.kind(element) != NodeKind::Element {
            return;
        }
        let id = self.node_id(element);
        {
            let mut elements = self.changes.forced.elements.borrow_mut();
            elements.retain(|&(at, _)| at != id);
            if !classes.is_empty() {
                elements.push((id, classes));
            }
            self.changes.forced.any.set(!elements.is_empty());
        }
        let changed = ForcedPseudoClasses(old.0 ^ classes.0);
        let states = changed.states();
        if states != 0 {
            self.state_changed(element, states);
        }
        if changed.has_focus() {
            self.forced_focus_moved(element);
        }
    }

    /// The forced focus went from or came to `element`: its ancestors'
    /// `:focus-within` may match otherwise.
    fn forced_focus_moved(&self, element: NodeRef<'gc>) {
        let ancestors: Vec<NodeRef<'gc>> =
            std::iter::successors(self.flat_tree_parent(element), |&node| {
                self.flat_tree_parent(node)
            })
            .take_while(|&node| self.kind(node) == NodeKind::Element)
            .collect();
        for ancestor in ancestors {
            self.state_changed(ancestor, AttributeChanges::FOCUS_WITHIN);
        }
    }

    /// `element` left its document's tree: what was forced on it goes, as
    /// Chrome's does (Blink's `InspectorCSSAgent::DidRemoveDOMNode`).
    /// Whether the focus (or one in it) was forced on it: the chain above
    /// loses its `:focus-within` (`forced_focus_left`).
    #[cold]
    pub(crate) fn forced_pseudo_classes_removed(&self, element: NodeRef<'gc>) -> bool {
        let forced = self.forced_pseudo_classes(element);
        if forced.is_empty() {
            return false;
        }
        let id = NodeId::from_raw(element.id.get());
        let mut elements = self.changes.forced.elements.borrow_mut();
        elements.retain(|&(at, _)| at != id);
        self.changes.forced.any.set(!elements.is_empty());
        forced.has_focus()
    }

    /// An element forced to have the focus was taken out of `parent`: its
    /// chain's `:focus-within` may match otherwise.
    #[cold]
    pub(crate) fn forced_focus_left(&self, parent: NodeRef<'gc>) {
        if self.kind(parent) == NodeKind::Element {
            self.state_changed(parent, AttributeChanges::FOCUS_WITHIN);
            self.forced_focus_moved(parent);
        }
    }
}
