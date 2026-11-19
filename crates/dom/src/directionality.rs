//! HTML's directionality of an element (https://html.spec.whatwg.org/
//! #the-directionality): its `dir` attribute, or for `auto` (and a `bdi`
//! with none) the first strong character of its text, or its parent
//! element's (a shadow root's host's at the top of a shadow tree, and for a
//! slot in one); ltr at the top. Cached on each element, as Blink's
//! `CachedDirectionality` is, and kept up as the tree changes (Blink's
//! `Element::UpdateDirectionalityAndDescendant`,
//! `HTMLElement::AdjustDirectionalityIfNeededAfterChildrenChanged`):
//! `:dir()` reads it, and `dir=auto`'s `direction`, and a canvas's
//! `direction = "inherit"`. An element whose directionality changes is
//! styled again.
//!
//! Until an element is given a `dir` or a `<bdi>` is made
//! (`form::DIRS_SEEN`), every element is `ltr` and nothing is kept up: a
//! page without either pays a look at a word as nodes come, go and change.

use computed_style::Direction;
use icu_properties::CodePointMapData;
use icu_properties::props::BidiClass;

use elise::Gc;

use crate::form::DIRS_SEEN;
use crate::{Cx, NodeKind, NodeRef, names};

/// In `Node::more`: its cached directionality is `rtl`.
const RTL: u8 = 1 << 1;
/// In `Node::more`: it or an ancestor is `auto` (`dir=auto`, or a `<bdi>`
/// without a valid `dir`; Blink's `kSelfOrAncestorHasDirAutoAttribute`), so
/// text changing below it may change an `auto` element's directionality.
/// Changes below others are skipped.
const AUTO_SCOPE: u8 = 1 << 2;

/// The states of HTML's `dir` attribute but the undefined one.
enum DirState {
    Ltr,
    Rtl,
    Auto,
}

impl<'gc> Cx<'_, 'gc> {
    /// `element`'s directionality, as cached.
    pub fn directionality(&self, element: NodeRef<'gc>) -> Direction {
        if element.more.get() & RTL != 0 {
            Direction::Rtl
        } else {
            Direction::Ltr
        }
    }

    /// Whether `element` is `auto`: its `dir` is, or it is a `<bdi>` with
    /// no valid `dir` (Blink's `HasDirectionAuto`), so its directionality
    /// is its text's.
    pub fn is_dir_auto(&self, element: NodeRef<'gc>) -> bool {
        match self.dir_state(element) {
            Some(DirState::Auto) => true,
            Some(_) => false,
            None => self.is_html_named(element, names::local::BDI),
        }
    }

    /// A `<bdi>` was made: from now on directionality is kept up; it is
    /// `auto` from the start.
    pub(crate) fn bdi_made(&self, element: NodeRef<'gc>) {
        self.see(DIRS_SEEN);
        element.more.set(element.more.get() | AUTO_SCOPE);
    }

    /// `node` was put in `parent`'s children: its directionality and its
    /// subtree's worked out against its new parent, and its text the
    /// nearest `auto` element's above.
    #[inline]
    pub(crate) fn directionality_inserted(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if self.has_seen(DIRS_SEEN) {
            self.directionality_moved(parent, node);
        }
    }

    /// `node` was taken out of `parent`'s children: its directionality
    /// worked out without a parent, and its text gone from the nearest
    /// `auto` element's above.
    #[inline]
    pub(crate) fn directionality_removed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if self.has_seen(DIRS_SEEN) {
            self.directionality_moved(parent, node);
        }
    }

    #[cold]
    fn directionality_moved(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        let element = self.kind(node) == NodeKind::Element;
        if element {
            self.update_directionality(node, true);
        }
        if !(element && self.skipped_by_auto(node)) {
            self.directionality_text_changed(parent);
        }
    }

    /// A text node's data changed: the nearest `auto` element's above may
    /// have new text.
    #[inline]
    pub(crate) fn directionality_data_changed(&self, node: NodeRef<'gc>) {
        if self.has_seen(DIRS_SEEN)
            && let Some(parent) = self.parent(node)
        {
            self.directionality_text_changed(parent);
        }
    }

    /// `element`'s `dir` changed (`new` its value now): its directionality
    /// and its subtree's worked out again, and the nearest `auto`
    /// element's above, which leaves out an element with a `dir`.
    pub(crate) fn dir_changed(&self, element: NodeRef<'gc>, new: Option<&str>) {
        if new.is_some() {
            self.see(DIRS_SEEN);
        }
        if !self.has_seen(DIRS_SEEN) {
            return;
        }
        // Its subtree and its siblings are styled again for the
        // attribute, whatever changes below (`dir` is read below too).
        self.update_directionality(element, false);
        if let Some(parent) = self.parent(element) {
            self.directionality_text_changed(parent);
        }
    }

    /// An `<input>`'s or `<textarea>`'s value changed: an `auto` one's
    /// directionality is its value's.
    #[inline]
    pub(crate) fn directionality_value_changed(&self, element: NodeRef<'gc>) {
        if self.has_seen(DIRS_SEEN) && self.is_dir_auto(element) {
            self.update_directionality(element, true);
        }
    }

    /// An `<input>`'s type changed: a telephone field is `ltr`, and an
    /// `auto` one's value may count or not.
    #[inline]
    pub(crate) fn directionality_type_changed(&self, element: NodeRef<'gc>) {
        if self.has_seen(DIRS_SEEN) {
            self.update_directionality(element, true);
        }
    }

    /// The text below `at` changed (a node came, went or changed there, or
    /// an element there got or lost a `dir`): the nearest `auto` element
    /// whose text it is part of, `at` or above, worked out again; none
    /// past one that `auto` leaves out.
    fn directionality_text_changed(&self, mut at: NodeRef<'gc>) {
        loop {
            if self.kind(at) != NodeKind::Element || at.more.get() & AUTO_SCOPE == 0 {
                return;
            }
            if self.is_dir_auto(at) {
                self.update_directionality(at, true);
                return;
            }
            if self.skipped_by_auto(at) {
                return;
            }
            match self.parent(at) {
                Some(parent) => at = parent,
                None => return,
            }
        }
    }

    /// `root`'s directionality and `AUTO_SCOPE` worked out again, and its
    /// descendants', through shadow trees, as far as they change: below
    /// an element whose own didn't, they are as they were. If `restyle`,
    /// one whose directionality changed has its `:dir()` state changed
    /// (Blink's `PseudoStateChanged(kPseudoDir)`, which restyles what its
    /// rules reach: siblings too), and an `auto` one is styled again for
    /// its `direction`.
    fn update_directionality(&self, root: NodeRef<'gc>, restyle: bool) {
        let mut stack = vec![root];
        while let Some(element) = stack.pop() {
            let (direction, auto_scope) = self.worked_out_directionality(element);
            let old = element.more.get();
            let mut new = old & !(RTL | AUTO_SCOPE);
            if direction == Direction::Rtl {
                new |= RTL;
            }
            if auto_scope {
                new |= AUTO_SCOPE;
            }
            if new == old {
                continue;
            }
            element.more.set(new);
            if restyle && (old ^ new) & RTL != 0 {
                self.state_changed(element, crate::AttributeChanges::DIRECTIONALITY);
                if self.is_dir_auto(element) && self.is_connected(element) {
                    self.set_needs_style_recalc(element);
                }
            }
            let shadow = self.shadow_root(element);
            // A host's slots take its, whatever their parents' (which may
            // not have changed: they stop the walk).
            if let Some(root) = shadow {
                stack.extend(self.slots_of(root));
            }
            for parent in std::iter::once(element).chain(shadow) {
                let mut child = self.first_child(parent);
                while let Some(node) = child {
                    if self.kind(node) == NodeKind::Element {
                        stack.push(node);
                    }
                    child = self.next_sibling(node);
                }
            }
        }
    }

    /// What `element`'s directionality and `AUTO_SCOPE` are, its parent's
    /// cached: its `dir`'s, or its text's if `auto`, or else what it
    /// inherits (a telephone field `ltr`, a slot in a shadow tree its host's, an element at the top
    /// of one the host's, any other its parent element's; `ltr` with
    /// none). `AUTO_SCOPE` is its own and its parent element's (in its
    /// tree: an `auto` element's text is its tree's).
    fn worked_out_directionality(&self, element: NodeRef<'gc>) -> (Direction, bool) {
        let parent = self.parent(element);
        let parent_element = parent.filter(|&parent| self.kind(parent) == NodeKind::Element);
        let auto = self.is_dir_auto(element);
        let auto_scope =
            auto || parent_element.is_some_and(|parent| parent.more.get() & AUTO_SCOPE != 0);
        let direction = match self.dir_state(element) {
            Some(DirState::Ltr) => Direction::Ltr,
            Some(DirState::Rtl) => Direction::Rtl,
            _ if auto => self.auto_directionality(element).unwrap_or(Direction::Ltr),
            // A telephone field without a valid `dir` (HTML's).
            _ if self.input_kind(element) == Some(crate::input::InputKind::Tel) => Direction::Ltr,
            _ => {
                let host = |root| self.shadow_host(root);
                let from = if self.is_html_named(element, names::local::SLOT)
                    && let Some(root) = self.containing_shadow_root(element)
                {
                    host(root)
                } else if let Some(root) = parent.filter(|&parent| self.is_shadow_root(parent)) {
                    host(root)
                } else {
                    parent_element
                };
                from.map_or(Direction::Ltr, |from| self.directionality(from))
            }
        };
        (direction, auto_scope)
    }

    /// An HTML element's `dir`, if it is one of its keywords.
    fn dir_state(&self, element: NodeRef<'gc>) -> Option<DirState> {
        if self.qualified_name(element)?.namespace() != names::HTML_NAMESPACE {
            return None;
        }
        let dir = self.attribute_value_by_id(element, names::local::DIR)?;
        let dir = dir.borrow().to_str().to_ascii_lowercase();
        match dir.as_str() {
            "ltr" => Some(DirState::Ltr),
            "rtl" => Some(DirState::Rtl),
            "auto" => Some(DirState::Auto),
            _ => None,
        }
    }

    /// The way the first strong character of `element`'s text goes (HTML's
    /// "contained text auto directionality"), leaving out what `bdi`,
    /// `script`, `style` and `textarea` elements hold, and elements with
    /// their own `dir`; `None` if there is none.
    fn auto_directionality(&self, element: NodeRef<'gc>) -> Option<Direction> {
        let bidi = CodePointMapData::<BidiClass>::new();
        let strong = |text: &str| {
            text.chars().find_map(|c| match bidi.get(c) {
                BidiClass::LeftToRight => Some(Direction::Ltr),
                BidiClass::RightToLeft | BidiClass::ArabicLetter => Some(Direction::Rtl),
                _ => None,
            })
        };
        // HTML's auto-directionality form-associated elements: their
        // value's first strong character, `ltr` for a value without one.
        if let Some(value) = self.auto_directionality_value(element) {
            let value = value.to_str();
            return strong(&value).or((!value.is_empty()).then_some(Direction::Ltr));
        }
        let skipped = |node: NodeRef<'gc>| self.skipped_by_auto(node);
        let mut next = self.first_child(element);
        while let Some(node) = next {
            if let Some(text) = self
                .text_data(node)
                .filter(|_| self.kind(node) == NodeKind::Text)
            {
                let strong = strong(&text.borrow().to_str());
                if strong.is_some() {
                    return strong;
                }
            }
            // In tree order: down unless skipped, else across, else across
            // from the nearest ancestor below `element` that has a next one.
            next = self.first_child(node).filter(|_| !skipped(node));
            let mut at = node;
            while next.is_none() {
                next = self.next_sibling(at);
                if next.is_none() {
                    at = self
                        .parent(at)
                        .filter(|&parent| !Gc::ptr_eq(parent, element))?;
                }
            }
        }
        None
    }

    /// Whether an `auto` element's text leaves out what is in `node`, an
    /// element: a `bdi`, `script`, `style` or `textarea`, or one with its
    /// own `dir`.
    fn skipped_by_auto(&self, node: NodeRef<'gc>) -> bool {
        [
            names::local::BDI,
            names::local::SCRIPT,
            names::local::STYLE,
            names::local::TEXTAREA,
        ]
        .into_iter()
        .any(|local| self.is_html_named(node, local))
            || self.dir_state(node).is_some()
    }

    /// The value of `element` if it is an auto-directionality
    /// form-associated element (HTML's): a `<textarea>`, or an `<input>`
    /// of a text type or a button's.
    fn auto_directionality_value(&self, element: NodeRef<'gc>) -> Option<crate::DomString> {
        use crate::input::InputKind::*;
        if self.is_html_named(element, names::local::TEXTAREA) {
            return Some(self.textarea_value(element));
        }
        match self.input_kind(element)? {
            Hidden | Text | Search | Tel | Url | Email | Password | Submit | Reset | Button => {
                Some(self.input_value(element))
            }
            _ => None,
        }
    }
}
