//! `selectors::Element` on the DOM's elements, for the selectors `css`
//! parses: names compare as numbers (a selector's numbered as its rule
//! index was made, `css::IdentAtom`; an element's as it holds them), ids
//! as text.
//!
//! No link counts as visited. The walk passes over selectors by the
//! ancestor bloom filter (`bloom`) before it matches them here. The new
//! DOM has no shadow trees, forms, focus or hover yet: the pseudo-classes
//! of those match nothing.

use css::selector::{CssIdent, CssString, Impl, PseudoClass, PseudoElement};
use dom::{Cx, NameId, NodeKind, NodeRef, QualifiedName, names};
use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::BloomFilter;
use selectors::matching::{ElementSelectorFlags, MatchingContext};
use selectors::{Element, OpaqueElement};

/// `selectors::Element` for an element of the DOM, in an operation,
/// with the thread's serial (what its selectors' numbers are of).
#[derive(Clone, Copy)]
pub(crate) struct StyleElement<'a, 'gc> {
    pub(crate) cx: &'a Cx<'a, 'gc>,
    pub(crate) node: NodeRef<'gc>,
    pub(crate) serial: u32,
    /// Matched as its host's pseudo-element (an element of a UA shadow
    /// tree, by its shadow pseudo id): what is before the pseudo-element
    /// matches the host.
    pub(crate) shadow_pseudo: bool,
    /// Matched as its own `::selection` (a highlight's originating element
    /// is the element itself): the `::selection` selectors, and what is
    /// before the pseudo-element matches it.
    pub(crate) selection: bool,
}

impl std::fmt::Debug for StyleElement<'_, '_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("StyleElement")
            .field(&self.cx.element_name(self.node))
            .finish()
    }
}

impl<'a, 'gc> StyleElement<'a, 'gc> {
    fn with(&self, node: NodeRef<'gc>) -> Self {
        Self {
            node,
            shadow_pseudo: false,
            selection: false,
            ..*self
        }
    }

    pub(crate) fn name(&self) -> QualifiedName {
        self.cx.qualified_name(self.node).expect("an element")
    }

    /// `ident`'s number on this thread: the one its rule index gave it, or
    /// (a selector no index of this thread numbered) the table's, if it
    /// has one (none: no element has the name).
    fn number(&self, ident: &CssIdent) -> Option<NameId> {
        ident.1.get(self.serial).or_else(|| names::find(&ident.0))
    }

    fn element(&self, node: Option<NodeRef<'gc>>) -> Option<Self> {
        node.filter(|node| self.cx.kind(*node) == NodeKind::Element)
            .map(|node| self.with(node))
    }

    /// `node` and its siblings in one direction, elements only.
    fn first_element(
        &self,
        mut node: Option<NodeRef<'gc>>,
        next: impl Fn(NodeRef<'gc>) -> Option<NodeRef<'gc>>,
    ) -> Option<Self> {
        while let Some(at) = node {
            if self.cx.kind(at) == NodeKind::Element {
                return Some(self.with(at));
            }
            node = next(at);
        }
        None
    }

    /// Whether it has the attribute `local` in the null namespace.
    fn has_attr(&self, local: NameId) -> bool {
        (0..self.cx.attribute_count(self.node)).any(|at| {
            self.cx
                .attribute_name_at(self.node, at)
                .is_some_and(|name| name.is(names::NONE, local))
        })
    }

    fn is_html(&self, local: NameId) -> bool {
        self.name() == QualifiedName::html(local)
    }

    /// Its `id` attribute's value, where it is.
    pub(crate) fn id(&self) -> Option<dom::StrRef<'gc>> {
        self.cx.attribute_value_by_id(self.node, names::ID)
    }

    /// Its language (HTML's): its own or its nearest ancestor's `lang`
    /// attribute, a shadow tree's host's among them; else its document's
    /// content language (`Content-Language`, or a `<meta>`'s); empty if
    /// none.
    fn language(&self) -> String {
        let lang = names::local::LANG;
        let mut at = Some(self.node);
        while let Some(node) = at {
            if self.cx.kind(node) == NodeKind::Element
                && let Some(value) = self.cx.attribute_value_by_id(node, lang)
            {
                return value.borrow().to_str().into_owned();
            }
            at = self.cx.parent(node).or_else(|| self.cx.shadow_host(node));
        }
        self.cx
            .content_language(self.cx.document_of(self.node))
            .unwrap_or_default()
    }
}

impl Element for StyleElement<'_, '_> {
    type Impl = Impl;

    fn opaque(&self) -> OpaqueElement {
        OpaqueElement::new(self.node.into_ref())
    }

    fn parent_element(&self) -> Option<Self> {
        self.element(self.cx.parent(self.node))
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        self.cx
            .parent(self.node)
            .is_some_and(|parent| self.cx.is_shadow_root(parent))
    }

    /// The host of the shadow tree it is in, if it is in one.
    fn containing_shadow_host(&self) -> Option<Self> {
        let root = self.cx.containing_shadow_root(self.node)?;
        self.element(self.cx.shadow_host(root))
    }

    fn is_pseudo_element(&self) -> bool {
        self.shadow_pseudo
    }

    /// As its host's pseudo-element, the host.
    fn pseudo_element_originating_element(&self) -> Option<Self> {
        // Its own `::selection`'s originating element is itself.
        if self.selection {
            return Some(Self {
                selection: false,
                ..*self
            });
        }
        if self.shadow_pseudo {
            return self.containing_shadow_host();
        }
        self.parent_element()
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.first_element(self.cx.previous_sibling(self.node), |node| {
            self.cx.previous_sibling(node)
        })
    }

    fn next_sibling_element(&self) -> Option<Self> {
        self.first_element(self.cx.next_sibling(self.node), |node| {
            self.cx.next_sibling(node)
        })
    }

    fn first_element_child(&self) -> Option<Self> {
        self.first_element(self.cx.first_child(self.node), |node| {
            self.cx.next_sibling(node)
        })
    }

    fn is_html_element_in_html_document(&self) -> bool {
        self.name().namespace() == names::HTML_NAMESPACE
            && self.cx.is_html_document(self.cx.document_of(self.node))
    }

    fn has_local_name(&self, local_name: &CssIdent) -> bool {
        self.number(local_name) == Some(self.name().local())
    }

    fn has_namespace(&self, ns: &CssIdent) -> bool {
        self.number(ns) == Some(self.name().namespace())
    }

    fn is_same_type(&self, other: &Self) -> bool {
        let (a, b) = (self.name().ids(), other.name().ids());
        a.local == b.local && a.namespace == b.namespace
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&CssIdent>,
        local_name: &CssIdent,
        operation: &AttrSelectorOperation<&CssString>,
    ) -> bool {
        let Some(local) = self.number(local_name) else {
            return false;
        };
        let namespace = match ns {
            NamespaceConstraint::Any => None,
            NamespaceConstraint::Specific(url) => match self.number(url) {
                Some(namespace) => Some(namespace),
                None => return false,
            },
        };
        (0..self.cx.attribute_count(self.node)).any(|at| {
            let Some(name) = self.cx.attribute_name_at(self.node, at) else {
                return false;
            };
            let name = name.ids();
            name.local == local
                && namespace.is_none_or(|namespace| name.namespace == namespace)
                && self
                    .cx
                    .attribute_value_at(self.node, at)
                    .is_some_and(|value| operation.eval_str(&value.borrow().to_str()))
        })
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &PseudoClass,
        _: &mut MatchingContext<'_, Impl>,
    ) -> bool {
        // A state looked at: its change may change what matches here (the
        // engine before kept these; Blink's pseudo-class invalidation sets
        // invalidate any element whose state changed).
        let state = crate::invalidation::pseudo_class_state(pc);
        if state != 0 {
            self.cx.add_state_dependencies(self.node, state);
        }
        // DevTools' (`CSS.forcePseudoState`): a flag's read while none is
        // forced, after the dependency is kept, so that its change
        // restyles what it matched.
        if let Some(forced) = self.cx.forced_pseudo_class(self.node, pc) {
            return forced;
        }
        match pc {
            PseudoClass::Link | PseudoClass::AnyLink => self.is_link(),
            // The form's (`:read-write` an editable element's too).
            PseudoClass::Checked => self.cx.matches_checked(self.node),
            PseudoClass::Indeterminate => self.cx.matches_indeterminate(self.node),
            PseudoClass::Disabled => self.cx.matches_disabled(self.node),
            PseudoClass::Enabled => self.cx.matches_enabled(self.node),
            PseudoClass::Required => self.cx.matches_required(self.node) == Some(true),
            PseudoClass::Optional => self.cx.matches_required(self.node) == Some(false),
            PseudoClass::ReadWrite => self.cx.matches_read_write(self.node),
            PseudoClass::ReadOnly => !self.cx.matches_read_write(self.node),
            PseudoClass::PlaceholderShown => self.cx.matches_placeholder_shown(self.node),
            PseudoClass::Default => self.cx.matches_default(self.node),
            PseudoClass::InternalListBox => {
                self.is_html(names::local::SELECT) && !self.cx.select_uses_menu_list(self.node)
            }
            PseudoClass::Open
                if self.is_html(names::local::DETAILS) || self.is_html(names::local::DIALOG) =>
            {
                self.has_attr(names::local::OPEN)
            }
            PseudoClass::Open => self.cx.select_popup_open(self.node),
            PseudoClass::InternalMultiSelectFocus => self.cx.matches_multi_select_focus(self.node),
            PseudoClass::Lang(ranges) => {
                let language = self.language();
                ranges.iter().any(|range| range.matches(&language))
            }
            PseudoClass::Hover => self.cx.is_hovered(self.node),
            PseudoClass::Active => self.cx.is_active(self.node),
            PseudoClass::Focus => self.cx.is_focused(self.node),
            PseudoClass::FocusWithin => self.cx.has_focus_within(self.node),
            PseudoClass::FocusVisible => self.cx.is_focus_visible(self.node),
            PseudoClass::Defined => self.cx.is_defined_element(self.node),
            PseudoClass::Target => self.cx.is_target(self.node),
            // The page doesn't have the system's focus (Chrome's, on any
            // element; a document with no window has none).
            PseudoClass::WindowInactive => !self
                .cx
                .window_of(self.cx.document_of(self.node))
                .is_some_and(|window| self.cx.page_focused(window)),
            // The rest are states the DOM doesn't keep yet: the
            // validity ones, `:visited` (never).
            _ => false,
        }
    }

    /// The pseudo-element its shadow pseudo id is, matched as one.
    fn match_pseudo_element(
        &self,
        pseudo: &PseudoElement,
        _: &mut MatchingContext<'_, Impl>,
    ) -> bool {
        if self.selection {
            return *pseudo == PseudoElement::Selection;
        }
        self.shadow_pseudo
            && pseudo
                .shadow_pseudo_id()
                .is_some_and(|id| self.cx.is_part(self.node, id))
    }

    /// What matching found it, or its parent, depends on: kept on the
    /// nodes (`Cx::add_selector_flags`), for the style phase to tell which
    /// children a change among them styles again.
    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        let bits = |flags: ElementSelectorFlags| {
            u16::try_from(flags.bits()).expect("the flags kept fit in 16 bits")
        };
        let own = flags.for_self();
        if !own.is_empty() {
            self.cx.add_selector_flags(self.node, bits(own));
        }
        let parent = flags.for_parent();
        if !parent.is_empty()
            && let Some(node) = self.cx.parent(self.node)
        {
            self.cx.add_selector_flags(node, bits(parent));
        }
    }

    /// An `<a>` or `<area>` with an `href` (HTML's `:link` / `:any-link`).
    fn is_link(&self) -> bool {
        (self.is_html(names::local::A) || self.is_html(names::local::AREA))
            && self.has_attr(names::local::HREF)
    }

    fn is_html_slot_element(&self) -> bool {
        self.cx.is_slot(self.node)
    }

    /// The slot it is assigned to, as last worked out (style works stale
    /// assignments out first).
    fn assigned_slot(&self) -> Option<Self> {
        self.element(self.cx.assigned_slot(self.node))
    }

    fn has_id(&self, id: &CssIdent, case_sensitivity: CaseSensitivity) -> bool {
        self.id().is_some_and(|value| {
            case_sensitivity.eq(value.borrow().to_str().as_bytes(), id.0.as_bytes())
        })
    }

    fn has_class(&self, name: &CssIdent, case_sensitivity: CaseSensitivity) -> bool {
        let classes = self.cx.class_names(self.node);
        match case_sensitivity {
            CaseSensitivity::CaseSensitive => self
                .number(name)
                .is_some_and(|number| classes.iter().any(|class| class.name == number)),
            CaseSensitivity::AsciiCaseInsensitive => classes
                .iter()
                .any(|class| names::get(class.name).eq_ignore_ascii_case(&name.0)),
        }
    }

    fn has_custom_state(&self, _: &CssIdent) -> bool {
        false
    }

    fn imported_part(&self, _: &CssIdent) -> Option<CssIdent> {
        None
    }

    fn is_part(&self, _: &CssIdent) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        let mut child = self.cx.first_child(self.node);
        while let Some(node) = child {
            match self.cx.kind(node) {
                NodeKind::Element => return false,
                NodeKind::Text
                    if self
                        .cx
                        .text_data(node)
                        .is_some_and(|data| !data.borrow().to_str().is_empty()) =>
                {
                    return false;
                }
                _ => {}
            }
            child = self.cx.next_sibling(node);
        }
        true
    }

    fn is_root(&self) -> bool {
        self.cx
            .parent(self.node)
            .is_some_and(|parent| self.cx.kind(parent) == NodeKind::Document)
    }

    /// Its names for `:has()`'s filter of a subtree, as the ancestor bloom
    /// filter has them.
    fn add_element_unique_hashes(&self, filter: &mut BloomFilter) -> bool {
        crate::bloom::each_hash(self.cx, self.node, |hash| filter.insert_hash(hash));
        true
    }
}
