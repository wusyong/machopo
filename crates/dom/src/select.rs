//! HTML's `<select>` and `<option>` (https://html.spec.whatwg.org/#the-select-element):
//! an option's selectedness and dirtiness, a select's list of options, the
//! selectedness setting algorithm, and the steps that run it as an option
//! comes or goes and an attribute changes (Blink's `HTMLOptionElement`
//! and `HTMLSelectElement::OptionInserted`, `OptionRemoved`). The steps run
//! as each change happens, as Blink's: the parser's options, one at a
//! time, each look for a selected one (the first, usually: the walk stops
//! there) and only an option inserted selected walks them all.
//!
//! An option keeps its selectedness and dirtiness in two of its flag
//! word's bits (Blink's `is_selected_` and `is_dirty_`): those a script's
//! and a MathML `annotation-xml`'s take, as an element is one of the
//! three at most. A select keeps nothing: its list of options is found at
//! each ask (Blink keeps `list_items_` until its
//! descendants change).
//!
//! Until a select is made, inserting and removing a subtree looks at no
//! option (`SELECTS_SEEN`).

use std::cell::Cell;

use elise::{Gc, GcObject, LocalExternal};

use crate::form::SELECTS_SEEN;
use crate::{AttributeChanges, Cx, DomString, NameId, NodeKind, NodeRef, QualifiedName, names};

/// `Node::flags`' of an option (HTML's selectedness and dirtiness).
pub(crate) const OPTION_SELECTED: u32 = 1 << 8;
pub(crate) const OPTION_DIRTY: u32 = 1 << 9;

/// An `<output>`'s state: its default value override (HTML's), once its
/// `value` was set (Blink's `default_value_`).
#[derive(GcObject)]
pub struct Output {
    default_value: LocalExternal<Cell<Option<DomString>>>,
}

/// What a select asks of its popup (`take_popup_requests`): the host
/// shows it (Blink's `ExternalPopupMenu::Show`), or hides it with no
/// result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupRequest {
    Show(gfx::NodeId),
    Hide(gfx::NodeId),
}

/// A list box's state between the user's events (`ListBoxState`), made at
/// the first.
#[derive(GcObject)]
pub struct ListBox {
    state: LocalExternal<Cell<ListBoxState>>,
}

/// What a list box keeps between the user's events (Blink's
/// `ListBoxSelectType`'s members; `dom_api`'s list box works it): the
/// range's anchor and end (options of its, by id), the options'
/// selectedness when the anchor was set (to go back to) and when the user
/// began to change it (to compare with: empty while not), whether the
/// range selects or unselects, and whether the keys moved the end without
/// selecting (Ctrl).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ListBoxState {
    pub anchor: Option<gfx::NodeId>,
    pub end: Option<gfx::NodeId>,
    pub anchor_selection: Vec<bool>,
    pub change_selection: Vec<bool>,
    pub range_selects: bool,
    pub non_contiguous: bool,
}

/// Where the walk of a select's descendants for its options is (HTML's
/// "option element nearest ancestor select"): a select, a datalist, an
/// `<hr>` or an option hides the options below it; an optgroup in an
/// optgroup too.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Below {
    Select,
    Optgroup,
}

impl<'gc> Cx<'_, 'gc> {
    pub(crate) fn is_html_named(&self, node: NodeRef<'gc>, local: NameId) -> bool {
        node.qualified_name() == QualifiedName::html(local)
    }

    pub fn is_option(&self, node: NodeRef<'gc>) -> bool {
        self.is_html_named(node, names::local::OPTION)
    }

    pub fn is_select(&self, node: NodeRef<'gc>) -> bool {
        self.is_html_named(node, names::local::SELECT)
    }

    pub fn is_optgroup(&self, node: NodeRef<'gc>) -> bool {
        self.is_html_named(node, names::local::OPTGROUP)
    }

    /// Whether `node` hides the options below it from a select (HTML's
    /// nearest ancestor select stops at it).
    fn hides_options(&self, node: NodeRef<'gc>) -> bool {
        [
            names::local::DATALIST,
            names::local::HR,
            names::local::OPTION,
        ]
        .into_iter()
        .any(|local| self.is_html_named(node, local))
    }

    /// An option's selectedness.
    pub fn option_selectedness(&self, option: NodeRef<'gc>) -> bool {
        self.flag(option, OPTION_SELECTED)
    }

    /// An option's dirtiness: script or the user set its selectedness, and
    /// its `selected` attribute no longer does.
    pub fn option_dirtiness(&self, option: NodeRef<'gc>) -> bool {
        self.flag(option, OPTION_DIRTY)
    }

    /// Sets an option's selectedness: `:checked` noted if it changed.
    fn set_selectedness(&self, option: NodeRef<'gc>, selected: bool) {
        if self.option_selectedness(option) == selected {
            return;
        }
        self.set_flag(option, OPTION_SELECTED, selected);
        self.state_changed(option, AttributeChanges::CHECKED);
        self.option_shown_changed(option);
    }

    /// The select whose list of options an option whose parent is `parent`
    /// is in (HTML's "option element nearest ancestor select"; with
    /// customizable selects, an option in a `<div>` in the select is one of
    /// its options too, as Chrome's).
    pub fn nearest_select(&self, parent: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut optgroup = false;
        let mut at = Some(parent);
        while let Some(node) = at {
            if self.kind(node) == NodeKind::Element {
                if self.hides_options(node) {
                    return None;
                }
                if self.is_optgroup(node) {
                    if optgroup {
                        return None;
                    }
                    optgroup = true;
                }
                if self.is_select(node) {
                    return Some(node);
                }
            }
            at = self.parent(node);
        }
        None
    }

    /// The select whose list of options `option` is in.
    pub fn option_select(&self, option: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.nearest_select(self.parent(option)?)
    }

    /// `select`'s list of options in tree order, each given to `found`
    /// until it says stop (`false`); whether it went through them all.
    pub fn for_each_option(
        &self,
        select: NodeRef<'gc>,
        mut found: impl FnMut(NodeRef<'gc>) -> bool,
    ) -> bool {
        self.options_below(select, Below::Select, &mut found)
    }

    fn options_below(
        &self,
        node: NodeRef<'gc>,
        below: Below,
        found: &mut impl FnMut(NodeRef<'gc>) -> bool,
    ) -> bool {
        let mut child = self.first_child(node);
        while let Some(at) = child {
            child = self.next_sibling(at);
            if self.kind(at) != NodeKind::Element {
                continue;
            }
            if self.is_option(at) {
                if !found(at) {
                    return false;
                }
                continue;
            }
            if self.is_select(at) || self.hides_options(at) {
                continue;
            }
            let inner = if self.is_optgroup(at) {
                if below == Below::Optgroup {
                    continue;
                }
                Below::Optgroup
            } else {
                below
            };
            if !self.options_below(at, inner, found) {
                return false;
            }
        }
        true
    }

    /// `select`'s list of options.
    pub fn list_of_options(&self, select: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let mut options = Vec::new();
        self.for_each_option(select, |option| {
            options.push(option);
            true
        });
        options
    }

    /// Whether `select` has the `multiple` attribute.
    pub fn select_is_multiple(&self, select: NodeRef<'gc>) -> bool {
        self.attribute_value_by_id(select, names::local::MULTIPLE)
            .is_some()
    }

    /// A select's display size: its `size` attribute's positive number,
    /// else 1 (Blink's, which takes 0 for the default too). HTML's default
    /// is 4 with `multiple`, where the algorithm doesn't ask.
    pub fn select_display_size(&self, select: NodeRef<'gc>) -> u32 {
        self.attribute_value_by_id(select, names::local::SIZE)
            .and_then(|size| crate::numbers::parse_integer(&size.borrow().to_str()))
            .filter(|size| *size > 0)
            .map_or(1, i32::unsigned_abs)
    }

    /// Whether a select is shown as a menu list, collapsed to the option it
    /// shows (Blink's `HTMLSelectElement::UsesMenuList`): without
    /// `multiple`, one of a display size of 1; with it, only one whose
    /// `size` is 1.
    pub fn select_uses_menu_list(&self, select: NodeRef<'gc>) -> bool {
        if self.select_is_multiple(select) {
            self.attribute_value_by_id(select, names::local::SIZE)
                .and_then(|size| crate::numbers::parse_integer(&size.borrow().to_str()))
                == Some(1)
        } else {
            self.select_display_size(select) == 1
        }
    }

    /// Whether an option is disabled: by its `disabled` attribute, or its
    /// nearest `<optgroup>` ancestor's in its select (HTML's parent
    /// optgroup, with customizable selects).
    pub fn option_disabled(&self, option: NodeRef<'gc>) -> bool {
        if self
            .attribute_value_by_id(option, names::local::DISABLED)
            .is_some()
        {
            return true;
        }
        let mut at = self.parent(option);
        while let Some(node) = at {
            if self.is_select(node) {
                return false;
            }
            if self.is_optgroup(node) {
                return self
                    .attribute_value_by_id(node, names::local::DISABLED)
                    .is_some();
            }
            at = self.parent(node);
        }
        false
    }

    /// HTML's selectedness setting algorithm (Blink's `HTMLSelectElement::
    /// ResetToDefaultSelection`): a select without `multiple` keeps one
    /// option selected, the last; with none, a display size of 1 selects
    /// its first option that isn't disabled.
    pub(crate) fn selectedness_setting(&self, select: NodeRef<'gc>) {
        if self.select_is_multiple(select) {
            return;
        }
        let options = self.list_of_options(select);
        let mut selected = options
            .iter()
            .filter(|option| self.option_selectedness(**option));
        match selected.next_back() {
            Some(last) => {
                for option in &options {
                    if !Gc::ptr_eq(*option, *last) {
                        self.set_selectedness(*option, false);
                    }
                }
            }
            None => self.select_first_if_none(select),
        }
    }

    /// The algorithm's case where none is selected, for a select whose
    /// options are known to have at most one selected (none is selected
    /// more as an option comes or goes, or an option's or an optgroup's
    /// `disabled` changes): a display size of 1 without `multiple` selects
    /// the first that isn't disabled. The walk stops at a selected one.
    fn select_first_if_none(&self, select: NodeRef<'gc>) {
        if self.select_is_multiple(select) || self.select_display_size(select) != 1 {
            return;
        }
        let mut first = None;
        let none = self.for_each_option(select, |option| {
            if self.option_selectedness(option) {
                return false;
            }
            if first.is_none() && !self.option_disabled(option) {
                first = Some(option);
            }
            true
        });
        if none && let Some(first) = first {
            self.set_selectedness(first, true);
        }
    }

    /// HTML's rule of a select without `multiple`: an option of its list
    /// whose selectedness is set to true (or that is inserted so)
    /// unselects the others.
    fn unselect_others(&self, select: NodeRef<'gc>, option: NodeRef<'gc>) {
        if self.select_is_multiple(select) {
            return;
        }
        self.for_each_option(select, |other| {
            if !Gc::ptr_eq(other, option) {
                self.set_selectedness(other, false);
            }
            true
        });
    }

    /// Setting an option's `selected` (script's, or the user's): its
    /// selectedness, dirty; a select without `multiple` keeps it alone, and
    /// has one selected if it is unselected (HTML's "ask for a reset").
    pub fn set_option_selected(&self, option: NodeRef<'gc>, selected: bool) {
        self.set_selectedness(option, selected);
        self.set_flag(option, OPTION_DIRTY, true);
        if let Some(select) = self.option_select(option) {
            if selected {
                self.unselect_others(select, option);
            } else {
                self.select_first_if_none(select);
            }
        }
    }

    /// Sets an option's selectedness alone, its dirtiness as it was and no
    /// other option changed (a list box's user, Blink's `SetSelectedState`).
    pub fn set_option_selectedness(&self, option: NodeRef<'gc>, selected: bool) {
        self.set_selectedness(option, selected);
    }

    /// Sets an option's selectedness alone and makes it dirty (a list box's
    /// user's choice of it).
    pub fn set_option_selected_dirty(&self, option: NodeRef<'gc>, selected: bool) {
        self.set_selectedness(option, selected);
        self.set_flag(option, OPTION_DIRTY, true);
    }

    /// `new Option(...)`'s selectedness: its `selected` argument, whatever
    /// its `selected` attribute, not dirty.
    pub fn set_option_constructed(&self, option: NodeRef<'gc>, selected: bool) {
        self.set_selectedness(option, selected);
        self.set_flag(option, OPTION_DIRTY, false);
    }

    /// Setting a select's `selectedIndex` or `value`: every option
    /// unselected, and `chosen` (if any) selected and dirty. No algorithm:
    /// none chosen leaves none selected.
    pub fn select_only(&self, select: NodeRef<'gc>, chosen: Option<NodeRef<'gc>>) {
        self.for_each_option(select, |option| {
            self.set_selectedness(option, false);
            true
        });
        if let Some(chosen) = chosen {
            self.set_selectedness(chosen, true);
            self.set_flag(chosen, OPTION_DIRTY, true);
        }
    }

    /// The first selected option of `select`'s list, if any.
    pub fn first_selected_option(&self, select: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let mut first = None;
        self.for_each_option(select, |option| {
            if self.option_selectedness(option) {
                first = Some(option);
                return false;
            }
            true
        });
        first
    }

    /// HTML's reset algorithm of a select (Blink's `HTMLSelectElement::
    /// ResetImpl`): each option of its list selected as its `selected`
    /// attribute says, not dirty, and the selectedness setting algorithm
    /// run.
    pub(crate) fn select_reset(&self, select: NodeRef<'gc>) {
        if !self.is_select(select) {
            return;
        }
        self.for_each_option(select, |option| {
            self.set_selectedness(
                option,
                self.attribute_value_by_id(option, names::local::SELECTED)
                    .is_some(),
            );
            self.set_flag(option, OPTION_DIRTY, false);
            true
        });
        self.selectedness_setting(select);
    }

    /// A select was made: from now on, subtrees inserted and removed are
    /// looked at for options.
    pub(crate) fn select_made(&self) {
        self.see(SELECTS_SEEN);
        // Its options and optgroups have UA shadow trees too.
        self.see(crate::form::CONTROLS_SEEN);
    }

    /// An option, as it is made, is selected if made with a `selected`
    /// attribute: its attribute change steps (`option_attribute_changed`)
    /// run as the attribute is set.
    ///
    /// `node` was inserted into `parent` (`inserted_noting`): HTML's
    /// insertion steps of the options among its inclusive descendants that
    /// are in a select outside it (Blink's `HTMLSelectElement::
    /// OptionInserted`). One inserted selected unselects the others of a
    /// select without `multiple`, so of several the last stays selected;
    /// then the select has one selected if it has none. (An option whose
    /// select came with it changes nothing.)
    pub(crate) fn options_inserted(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if !self.has_seen(SELECTS_SEEN) || !self.has_option(node) {
            return;
        }
        let Some(select) = self.nearest_select(parent) else {
            return;
        };
        // Each one inserted selected unselects the others, the ones after
        // it too: the last of them stays selected.
        let mut last = None;
        self.for_each_inclusive_descendant(node, |at| {
            if self.is_option(at)
                && self.option_selectedness(at)
                && self
                    .option_select(at)
                    .is_some_and(|of| Gc::ptr_eq(of, select))
            {
                last = Some(at);
            }
        });
        if let Some(last) = last {
            self.unselect_others(select, last);
        }
        if !self.changes.forms.replacing.get() {
            self.select_first_if_none(select);
            // Its labels.
            self.shadow_tree_changed(select);
        }
    }

    /// `node` was removed from `parent` (`remove_noting`): HTML's removing
    /// steps of the options among its inclusive descendants that were in a
    /// select outside it (Blink's `HTMLSelectElement::OptionRemoved`): the
    /// select, left with none selected, selects one.
    pub(crate) fn options_removed(&self, parent: NodeRef<'gc>, node: NodeRef<'gc>) {
        if !self.has_seen(SELECTS_SEEN)
            || self.changes.forms.replacing.get()
            || !self.has_option(node)
        {
            return;
        }
        if let Some(select) = self.nearest_select(parent) {
            self.select_first_if_none(select);
            self.shadow_tree_changed(select);
        }
    }

    /// A "replace all" of `parent`'s children is to begin: a select's
    /// options' steps wait for its end (`options_replaced`), as HTML runs
    /// the algorithm in a select's children changed steps, once for the
    /// whole of it (whatwg/html#11825, as Blink: a select emptied and
    /// filled again keeps the option it had selected). Whether they wait.
    pub(crate) fn options_replacing(&self, parent: NodeRef<'gc>) -> bool {
        if !self.has_seen(SELECTS_SEEN) || self.nearest_select(parent).is_none() {
            return false;
        }
        self.changes.forms.replacing.set(true);
        true
    }

    /// The "replace all" of `parent`'s children is over: the select has one
    /// selected if it has none.
    pub(crate) fn options_replaced(&self, parent: NodeRef<'gc>) {
        self.changes.forms.replacing.set(false);
        if let Some(select) = self.nearest_select(parent) {
            self.select_first_if_none(select);
            self.shadow_tree_changed(select);
        }
    }

    /// Whether `node` is an option or has one among its descendants.
    fn has_option(&self, node: NodeRef<'gc>) -> bool {
        if self.is_option(node) {
            return true;
        }
        let mut at = self.first_child(node);
        while let Some(descendant) = at {
            if self.is_option(descendant) {
                return true;
            }
            at = self.following_within(descendant, node);
        }
        false
    }

    /// An option's `selected`, a select's `multiple` or `size`, an
    /// option's, an optgroup's or a select's `disabled` changed
    /// (`control_attribute_changed`): HTML's attribute change steps. An
    /// option's `selected`, while it isn't dirty, is its selectedness; the
    /// select's options are set again; an optgroup's and a select's
    /// options are disabled with it (`:disabled`).
    pub(crate) fn select_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        local: NameId,
        present: bool,
    ) {
        if self.is_option(element) {
            match local {
                names::local::SELECTED => {
                    if !self.option_dirtiness(element) {
                        self.set_selectedness(element, present);
                    }
                    if let Some(select) = self.option_select(element) {
                        if self.option_selectedness(element) {
                            self.unselect_others(select, element);
                        }
                        self.select_first_if_none(select);
                    }
                }
                names::local::DISABLED => {
                    if let Some(select) = self.option_select(element) {
                        self.select_first_if_none(select);
                    }
                }
                names::local::LABEL => {
                    self.shadow_tree_changed(element);
                    self.option_shown_changed(element);
                }
                _ => {}
            }
        } else if self.is_optgroup(element) {
            if local == names::local::LABEL {
                self.shadow_tree_changed(element);
            }
            if local == names::local::DISABLED {
                self.for_each_inclusive_descendant(element, |at| {
                    if self.is_option(at) {
                        self.state_changed(at, AttributeChanges::DISABLED);
                    }
                });
                if let Some(select) = self.nearest_select(element) {
                    self.select_first_if_none(select);
                }
            }
        } else if self.is_select(element) {
            match local {
                names::local::MULTIPLE | names::local::SIZE => {
                    self.selectedness_setting(element);
                    // A menu list and a list box have different trees.
                    self.shadow_tree_type_changed(element);
                }
                names::local::DISABLED => self.select_options_disabled_changed(element),
                _ => {}
            }
        }
    }

    /// Whether `select` disabling its options and optgroups (`:disabled`)
    /// may have changed: each of them noted.
    pub(crate) fn select_options_disabled_changed(&self, select: NodeRef<'gc>) {
        self.for_each_inclusive_descendant(select, |at| {
            if self.is_option(at) || self.is_optgroup(at) {
                self.state_changed(at, AttributeChanges::DISABLED);
            }
        });
    }

    /// `:disabled` of an option (disabled itself, or by its parent
    /// optgroup) or an optgroup (by its attribute): also in a disabled
    /// select (HTML's rule since whatwg/html#12205, 2026; only for the
    /// pseudo-classes, not the selectedness setting algorithm). None for
    /// another element.
    pub(crate) fn option_matches_disabled(&self, element: NodeRef<'gc>) -> Option<bool> {
        let disabled = if self.is_option(element) {
            self.option_disabled(element)
        } else if self.is_optgroup(element) {
            self.attribute_value_by_id(element, names::local::DISABLED)
                .is_some()
        } else {
            return None;
        };
        Some(
            disabled
                || self
                    .parent(element)
                    .and_then(|parent| self.nearest_select(parent))
                    .is_some_and(|select| self.is_disabled_form_control(select)),
        )
    }

    /// Whether `select`'s popup is open (`:open`).
    pub fn select_popup_open(&self, select: NodeRef<'gc>) -> bool {
        self.is_select(select)
            && self
                .association(select)
                .is_some_and(|association| association.popup_open.get())
    }

    /// Sets whether `select`'s popup is open: `:open` noted if it changed.
    pub fn set_select_popup_open(&self, select: NodeRef<'gc>, open: bool) {
        if self.select_popup_open(select) == open {
            return;
        }
        self.association_or_new(select).popup_open.set(open);
        self.state_changed(select, AttributeChanges::OPEN);
    }

    /// Asks the host to show or hide `select`'s popup, once no script runs.
    pub fn request_popup(&self, select: NodeRef<'gc>, show: bool) {
        let id = self.node_id(select);
        let state = &self.changes.forms.popup_requests;
        let mut requests = state.take();
        requests.push(if show {
            PopupRequest::Show(id)
        } else {
            PopupRequest::Hide(id)
        });
        state.set(requests);
    }

    /// What the selects asked of their popups since last taken.
    pub fn take_popup_requests(&self) -> Vec<PopupRequest> {
        self.changes.forms.popup_requests.take()
    }

    /// `select`'s list box state (none yet: the default), its anchor and end
    /// forgotten if they are no longer its options (Blink's `OptionRemoved`
    /// clears them).
    pub fn list_box_state(&self, select: NodeRef<'gc>) -> ListBoxState {
        let Some(list_box) = self
            .association(select)
            .and_then(|association| association.list_box.get(self.gc))
        else {
            return ListBoxState::default();
        };
        let cell = list_box.into_ref().state.get();
        let mut state = cell.take();
        cell.set(state.clone());
        let ours = |id: Option<gfx::NodeId>| {
            id.filter(|id| {
                self.node_by_id(*id)
                    .and_then(|option| self.option_select(option))
                    .is_some_and(|of| Gc::ptr_eq(of, select))
            })
        };
        state.anchor = ours(state.anchor);
        state.end = ours(state.end);
        state
    }

    /// Sets `select`'s list box state: the option of
    /// `:-internal-multi-select-focus` noted if it is another.
    pub fn set_list_box_state(&self, select: NodeRef<'gc>, state: ListBoxState) {
        let focus = |state: &ListBoxState| state.end.filter(|_| state.non_contiguous);
        let old = self.list_box_state(select);
        if focus(&old) != focus(&state) {
            for option in [focus(&old), focus(&state)].into_iter().flatten() {
                if let Some(option) = self.node_by_id(option) {
                    self.state_changed(option, AttributeChanges::MULTI_SELECT_FOCUS);
                }
            }
        }
        let association = self.association_or_new(select);
        let list_box = match association.list_box.get(self.gc) {
            Some(list_box) => list_box,
            None => {
                let list_box = self.gc.alloc_in(
                    &crate::sites::OTHER,
                    ListBoxInit {
                        state: LocalExternal::new(Cell::new(ListBoxState::default())),
                    },
                );
                association.list_box.set(self.gc, Some(list_box));
                list_box
            }
        };
        list_box.into_ref().state.get().set(state);
    }

    /// `:-internal-multi-select-focus`: `option` is the end the keys moved a
    /// `multiple` list box to without selecting it (Blink's
    /// `UpdateMultiSelectFocus`), not a disabled one.
    pub fn matches_multi_select_focus(&self, option: NodeRef<'gc>) -> bool {
        if !self.is_option(option) || self.option_disabled(option) {
            return false;
        }
        let Some(select) = self.option_select(option) else {
            return false;
        };
        if !self.select_is_multiple(select) || self.select_uses_menu_list(select) {
            return false;
        }
        let state = self.list_box_state(select);
        state.non_contiguous
            && state.end.is_some_and(|end| {
                self.node_by_id(end)
                    .is_some_and(|end| Gc::ptr_eq(end, option))
            })
    }

    /// HTML's cloning steps of an option: its selectedness and dirtiness.
    pub(crate) fn option_cloned(&self, original: NodeRef<'gc>, copy: NodeRef<'gc>) {
        if self.is_option(original) {
            self.set_flag(copy, OPTION_SELECTED, self.option_selectedness(original));
            self.set_flag(copy, OPTION_DIRTY, self.option_dirtiness(original));
        }
    }

    /// An option's `text`: its descendant text but a script element's (an
    /// HTML or SVG one), with ASCII whitespace stripped and collapsed
    /// (Blink's `HTMLOptionElement::text`).
    pub fn option_text(&self, option: NodeRef<'gc>) -> String {
        let mut text = String::new();
        let mut at = self.first_child(option);
        while let Some(node) = at {
            match self.kind(node) {
                NodeKind::Text => {
                    if let Some(data) = self.data(node) {
                        text.push_str(&data.to_str());
                    }
                }
                NodeKind::Element => {
                    let name = node.qualified_name().ids();
                    let script = name.local == names::local::SCRIPT
                        && (name.namespace == names::HTML_NAMESPACE
                            || name.namespace == names::SVG_NAMESPACE);
                    if script {
                        at = self.next_skipping_children(node, option);
                        continue;
                    }
                }
                _ => {}
            }
            at = self.following_within(node, option);
        }
        strip_and_collapse(&text)
    }

    /// The node after `node` in tree order, but its descendants, within
    /// `root`'s.
    fn next_skipping_children(
        &self,
        node: NodeRef<'gc>,
        root: NodeRef<'gc>,
    ) -> Option<NodeRef<'gc>> {
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

    /// The label an option is shown with (Blink's `HTMLOptionElement::
    /// DisplayLabel`): its `label` attribute, as it is, unless that is
    /// empty; else its `text`.
    pub fn option_display_label(&self, option: NodeRef<'gc>) -> String {
        match self.attribute_value_by_id(option, names::local::LABEL) {
            Some(label) if !label.borrow().to_str().is_empty() => {
                label.borrow().to_str().to_string()
            }
            _ => self.option_text(option),
        }
    }

    /// The text a menu list shows (Blink's `MenuListSelectType::
    /// UpdateTextStyleAndContent`): its selected option's label, ASCII
    /// whitespace stripped (not collapsed); with `multiple`, unless one
    /// exactly is selected, how many are (`MultipleOptionsSelectedText`;
    /// English, as Chrome's in `en-US`).
    pub(crate) fn menu_list_text(&self, select: NodeRef<'gc>) -> String {
        let mut selected = Vec::new();
        self.for_each_option(select, |option| {
            if self.option_selectedness(option) {
                selected.push(option);
            }
            true
        });
        match selected.as_slice() {
            [option] => self
                .option_display_label(*option)
                .trim_matches([' ', '\t', '\n', '\x0c', '\r'])
                .to_owned(),
            _ if self.select_is_multiple(select) => format!("{} selected", selected.len()),
            _ => String::new(),
        }
    }

    /// What a menu list is as wide as (Blink's `LayoutBox::
    /// MenuListIntrinsicInlineSize` measures them): each option's label as
    /// it is (a line break a space), indented four spaces in an
    /// `<optgroup>`, and with `multiple` the text that says how many are
    /// selected; one a line.
    pub(crate) fn menu_list_labels(&self, select: NodeRef<'gc>) -> String {
        let mut labels = Vec::new();
        self.for_each_option(select, |option| {
            let label = self.option_display_label(option).replace('\n', " ");
            let grouped = self
                .parent(option)
                .is_some_and(|parent| self.is_optgroup(parent));
            labels.push(if grouped {
                format!("    {label}")
            } else {
                label
            });
            true
        });
        if self.select_is_multiple(select) {
            labels.push(format!("{} selected", labels.len()));
        }
        labels.join("\n")
    }

    /// What an option's select's menu list shows may have changed (its
    /// selectedness, its label, its text): the select's tree brought up to
    /// date.
    pub(crate) fn option_shown_changed(&self, option: NodeRef<'gc>) {
        if let Some(select) = self.option_select(option) {
            self.shadow_tree_changed(select);
        }
    }

    /// Text in `parent`'s subtree changed (a text's data, or children that
    /// have text came or went): the nearest option ancestor's, if any,
    /// shown again. Nothing until a select is made.
    pub(crate) fn option_text_changed(&self, parent: NodeRef<'gc>) {
        if !self.has_seen(SELECTS_SEEN) {
            return;
        }
        let mut at = Some(parent);
        while let Some(node) = at {
            if self.is_option(node) {
                self.option_shown_changed(node);
                return;
            }
            if self.is_select(node) {
                return;
            }
            at = self.parent(node);
        }
    }

    /// Whether `node` is a text or has one among its descendants.
    pub(crate) fn has_text(&self, node: NodeRef<'gc>) -> bool {
        if self.kind(node) == NodeKind::Text {
            return true;
        }
        let mut at = self.first_child(node);
        while let Some(descendant) = at {
            if self.kind(descendant) == NodeKind::Text {
                return true;
            }
            at = self.following_within(descendant, node);
        }
        false
    }

    /// An option's `label`: its `label` attribute where it has one, else
    /// its `text`.
    pub fn option_label(&self, option: NodeRef<'gc>) -> String {
        match self.attribute_value_by_id(option, names::local::LABEL) {
            Some(label) => label.borrow().to_str().to_string(),
            None => self.option_text(option),
        }
    }

    /// An option's `value`: its `value` attribute where it has one, else
    /// its `text`.
    pub fn option_value(&self, option: NodeRef<'gc>) -> String {
        match self.attribute_value_by_id(option, names::local::VALUE) {
            Some(value) => value.borrow().to_str().to_string(),
            None => self.option_text(option),
        }
    }

    /// An option's `index`: where it is in its select's list of options; 0
    /// out of one.
    pub fn option_index(&self, option: NodeRef<'gc>) -> u32 {
        let Some(select) = self.option_select(option) else {
            return 0;
        };
        let mut index = 0u32;
        self.for_each_option(select, |other| {
            if Gc::ptr_eq(other, option) {
                return false;
            }
            index = index.saturating_add(1);
            true
        });
        index
    }

    /// An output was made: its state.
    pub(crate) fn output_made(&self, element: NodeRef<'gc>) {
        let output = self.gc.alloc_in(
            &crate::sites::OTHER,
            OutputInit {
                default_value: LocalExternal::new(Cell::new(None)),
            },
        );
        self.association_or_new(element)
            .output
            .set(self.gc, Some(output));
    }

    fn output(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, Output>> {
        self.association(element)?.output.get(self.gc)
    }

    /// An `<output>`'s default value override, if its `value` was set.
    pub fn output_default_value_override(&self, element: NodeRef<'gc>) -> Option<DomString> {
        let cell = self.output(element)?.into_ref().default_value.get();
        let value = cell.take();
        cell.set(value.clone());
        value
    }

    pub fn set_output_default_value_override(
        &self,
        element: NodeRef<'gc>,
        value: Option<DomString>,
    ) {
        if let Some(output) = self.output(element) {
            output.into_ref().default_value.get().set(value);
        }
    }

    /// HTML's reset algorithm of an `<output>`: its children its default
    /// value's text, and no override.
    pub(crate) fn output_reset(&self, element: NodeRef<'gc>) {
        if self.output(element).is_none() {
            return;
        }
        let default = self
            .output_default_value_override(element)
            .unwrap_or_else(|| {
                self.text_content(element)
                    .unwrap_or_else(|| DomString::new(""))
            });
        self.set_text_content(element, &default.to_str());
        self.set_output_default_value_override(element, None);
    }
}

/// HTML's "strip and collapse ASCII whitespace".
fn strip_and_collapse(text: &str) -> String {
    text.split([' ', '\t', '\n', '\x0c', '\r'])
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}
