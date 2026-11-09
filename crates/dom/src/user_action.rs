//! A document's user action states (Blink's `Document::hover_element_`,
//! `active_element_`, `focused_element_` and `UserActionElementSet`): the
//! element the pointer is over, the one a button went down on and the one
//! with the focus, each with its ancestors a chain that `:hover`,
//! `:active` and `:focus-within` match (`:focus` and `:focus-visible` the
//! focused element alone, while the page has the system's focus). Each element of a chain has
//! its bit (`HOVERED`, `ACTIVE`), so that matching reads one bit; moving a
//! chain changes the bits, and notes the change for the style phase, only
//! where the old and the new chains differ.

use elise::Gc;

use crate::style::{
    ACTIVE, FOCUS_BITS, FOCUS_CHAIN, FOCUS_VISIBLE, FOCUS_WITHIN, FOCUSED, HOVERED,
};
use crate::{AttributeChanges, Cx, NodeKind, NodeRef};

impl<'gc> Cx<'_, 'gc> {
    /// `document`'s hover element (the bottom of its `:hover` chain).
    pub fn hover_element(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.document_data(document)?.hover.get(self.gc)
    }

    /// `document`'s active element: the bottom of its `:active` chain (not
    /// `document.activeElement`, which is the focused one).
    pub fn active_element(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.document_data(document)?.active.get(self.gc)
    }

    /// Whether `element` matches `:hover`.
    pub fn is_hovered(&self, element: NodeRef<'gc>) -> bool {
        element.flags.get() & HOVERED != 0
    }

    /// Whether `element` matches `:active`.
    pub fn is_active(&self, element: NodeRef<'gc>) -> bool {
        element.flags.get() & ACTIVE != 0
    }

    /// `document`'s hover element is `element` (one in its tree), or none.
    pub fn set_hover_element(&self, document: NodeRef<'gc>, element: Option<NodeRef<'gc>>) {
        let Some(data) = self.user_action_data(document, element.is_some()) else {
            return;
        };
        let old = data.hover.get(self.gc);
        data.hover.set(self.gc, element);
        self.move_chain(old, element, HOVERED, AttributeChanges::HOVER);
    }

    /// `document`'s active element is `element` (one in its tree), or none.
    pub fn set_active_element(&self, document: NodeRef<'gc>, element: Option<NodeRef<'gc>>) {
        let Some(data) = self.user_action_data(document, element.is_some()) else {
            return;
        };
        let old = data.active.get(self.gc);
        data.active.set(self.gc, element);
        self.move_chain(old, element, ACTIVE, AttributeChanges::ACTIVE);
    }

    /// `document`'s focused element (none: its viewport has the focus).
    pub fn focused_element(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.document_data(document)?.focused.get(self.gc)
    }

    /// Whether `element` matches `:focus`, `:focus-within`, `:focus-visible`.
    pub fn is_focused(&self, element: NodeRef<'gc>) -> bool {
        element.flags.get() & FOCUSED != 0
    }

    pub fn has_focus_within(&self, element: NodeRef<'gc>) -> bool {
        element.flags.get() & FOCUS_WITHIN != 0
    }

    pub fn is_focus_visible(&self, element: NodeRef<'gc>) -> bool {
        element.flags.get() & FOCUS_VISIBLE != 0
    }

    /// `document`'s focused element is `element` (one in its tree), or none
    /// (its viewport), showing a ring if `visible`; what matches as its
    /// window's page's focus says (`set_page_focused`).
    pub fn set_focused_element(
        &self,
        document: NodeRef<'gc>,
        element: Option<NodeRef<'gc>>,
        visible: bool,
    ) {
        let Some(data) = self.user_action_data(document, element.is_some()) else {
            return;
        };
        let old = data.focused.get(self.gc);
        data.focused.set(self.gc, element);
        let matching = data
            .window
            .get(self.gc)
            .is_some_and(|window| self.page_focused(window));
        self.move_focus_chain(old, element, matching, visible);
    }

    /// `window`'s page took or lost the system's focus: its document's
    /// focus states follow (no events: the API's).
    pub fn set_page_focused(&self, window: Gc<'gc, crate::Window>, focused: bool) {
        // The same as it was: the focused element's states stay as they
        // are (shown or not).
        if self.page_focused(window) == focused {
            return;
        }
        self.set_window_page_focused(window, focused);
        let document = self.window_document(window);
        if let Some(element) = self.focused_element(document) {
            let visible = element.flags.get() & FOCUS_VISIBLE != 0 || self.focus_visible(window);
            self.move_focus_chain(Some(element), Some(element), focused, visible);
        }
    }

    /// The focus chain from `old` to `new`: each element of either gets the
    /// bits it should have now (the chain's; while `matching`,
    /// `:focus-within`'s, and the new one's `:focus` and, if `visible`,
    /// `:focus-visible`), and those whose matching bits changed are noted.
    fn move_focus_chain(
        &self,
        old: Option<NodeRef<'gc>>,
        new: Option<NodeRef<'gc>>,
        matching: bool,
        visible: bool,
    ) {
        let old_chain = self.element_chain(old);
        let new_chain = self.element_chain(new);
        let elements = old_chain
            .iter()
            .chain(new_chain.iter())
            .enumerate()
            .filter(|&(index, element)| {
                // Once each: the old chain's not in the new one, then the new one's.
                index >= old_chain.len()
                    || !new_chain.iter().any(|&other| Gc::ptr_eq(other, *element))
            })
            .map(|(_, &element)| element);
        for element in elements {
            let in_new = new_chain.iter().any(|&other| Gc::ptr_eq(other, element));
            let is_new = new.is_some_and(|new| Gc::ptr_eq(new, element));
            let mut bits = 0;
            if in_new {
                bits |= FOCUS_CHAIN;
                if matching {
                    bits |= FOCUS_WITHIN;
                    if is_new {
                        bits |= FOCUSED;
                        if visible {
                            bits |= FOCUS_VISIBLE;
                        }
                    }
                }
            }
            let flags = element.flags.get();
            let changed = (flags & FOCUS_BITS) ^ bits;
            if changed == 0 {
                continue;
            }
            element.flags.set((flags & !FOCUS_BITS) | bits);
            let mut states = 0;
            if changed & FOCUSED != 0 {
                states |= AttributeChanges::FOCUS;
            }
            if changed & FOCUS_WITHIN != 0 {
                states |= AttributeChanges::FOCUS_WITHIN;
            }
            if changed & FOCUS_VISIBLE != 0 {
                states |= AttributeChanges::FOCUS_VISIBLE;
            }
            if states != 0 {
                self.state_changed(element, states);
            }
        }
    }

    /// `document`'s data, made if `make` (a document of no window has none
    /// till it needs it); none for a document with none to clear.
    fn user_action_data(
        &self,
        document: NodeRef<'gc>,
        make: bool,
    ) -> Option<Gc<'gc, crate::DocumentData>> {
        if make {
            Some(self.ensure_document_data(document))
        } else {
            self.document_data(document)
        }
    }

    /// The chain of `bit` (`state` for the style phase) from `old` to
    /// `new`: the elements of the old one not in the new one lose it, those
    /// of the new one not in the old one get it.
    fn move_chain(
        &self,
        old: Option<NodeRef<'gc>>,
        new: Option<NodeRef<'gc>>,
        bit: u32,
        state: u16,
    ) {
        let old = self.element_chain(old);
        let new = self.element_chain(new);
        // What both chains end with: the ancestors that keep the state.
        let shared = old
            .iter()
            .rev()
            .zip(new.iter().rev())
            .take_while(|&(&a, &b)| Gc::ptr_eq(a, b))
            .count();
        for &element in &old[..old.len().saturating_sub(shared)] {
            element.flags.set(element.flags.get() & !bit);
            self.state_changed(element, state);
        }
        for &element in &new[..new.len().saturating_sub(shared)] {
            element.flags.set(element.flags.get() | bit);
            self.state_changed(element, state);
        }
    }

    /// `element` and its ancestors that are elements, `element` first: in
    /// the flat tree (a host's `:hover` with what is in its shadow tree, a
    /// slot's with what is assigned to it), as Blink's chains are.
    fn element_chain(&self, element: Option<NodeRef<'gc>>) -> Vec<NodeRef<'gc>> {
        std::iter::successors(element, |&node| self.flat_tree_parent(node))
            .take_while(|&node| self.kind(node) == NodeKind::Element)
            .collect()
    }

    /// The bottom of a chain of `states` (`HOVERED`, `ACTIVE`) went with a
    /// subtree taken out of `parent`, whose bits are cleared: the chain
    /// ends at `parent` now, if it is an element (Blink's
    /// `Document::HoveredElementDetached`, which moves it to the parent
    /// until the mouse is looked at again).
    pub(crate) fn user_action_element_removed(&self, parent: NodeRef<'gc>, states: u32) {
        let Some(data) = self.document_data(self.document_of(parent)) else {
            return;
        };
        // The focused element went: the viewport has the focus, with no
        // events (HTML's removing steps, Blink's
        // `Document::RemoveFocusedElementOfSubtree`); the chain left above
        // loses its bits.
        if states & FOCUS_CHAIN != 0 {
            data.focused.set(self.gc, None);
            let above: Vec<NodeRef<'gc>> = self.element_chain(Some(parent));
            for element in above {
                let flags = element.flags.get();
                if flags & FOCUS_BITS == 0 {
                    break;
                }
                element.flags.set(flags & !FOCUS_BITS);
                if flags & FOCUS_WITHIN != 0 {
                    self.state_changed(element, AttributeChanges::FOCUS_WITHIN);
                }
            }
        }
        let parent = (self.kind(parent) == NodeKind::Element).then_some(parent);
        if states & HOVERED != 0 {
            data.hover.set(self.gc, parent);
        }
        if states & ACTIVE != 0 {
            data.active.set(self.gc, parent);
        }
    }
}
