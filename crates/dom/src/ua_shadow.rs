//! A form control's UA shadow tree (Blink's `TextControlElement` with its
//! `TextControlInnerEditorElement` and placeholder element,
//! `BaseButtonInputType`'s label, `SliderThumbElement`'s tree): a
//! text-like `<input>`'s or a `<textarea>`'s value is the text of an
//! inner editor `<div>`, and its `placeholder` that of a `<div>` before
//! it; a search field's and a number field's are in a decoration
//! container with their part after them (the clear button, the spin
//! button); an `<input>` button's label is a text; a range's thumb is in
//! its track in its container. A `<select>` shown as a menu list has an
//! inner element with the label of the option it shows and the labels it
//! is as wide as (Blink's `MenuListSelectType`, its `MenuListInnerElement`),
//! one shown as a list box a slot its options are laid out in
//! (`ListBoxSelectType`); an `<optgroup>` its label before a slot of its
//! options (`HTMLOptGroupElement`), an `<option>` its label before a slot
//! of its children, one of the two shown: the label where it has one
//! (`HTMLOptionElement`). The light children of a menu list (its options,
//! which its popup would show) aren't rendered: nothing slots them. Each
//! element of the tree has its shadow
//! pseudo id (its `pseudo` attribute, Blink's `ShadowPseudoId`): the
//! pseudo-element of the host it is, which style matches its rules
//! against. Script never gets the tree (a UA shadow root).
//!
//! The tree is built as Blink's is, when it is needed: a control
//! connected to a document with a window is scheduled
//! (`FormState::scheduled`), and gets its tree before its document is
//! styled again (`create_scheduled_shadow_trees`; Blink's
//! `Document::ScheduleShadowTreeCreation`); one that never is has none.
//! Once built, it follows the control at once (Blink's
//! `SetInnerEditorValue`): the value, the placeholder, a search field's
//! clear button's visibility, a range's thumb's ratio, a button's label,
//! a menu list's text and labels (an option's selectedness, label or text,
//! the list of options), an option's and an optgroup's label, as each
//! changes. A new `type` that needs another tree takes the
//! children away and makes them anew (none for a type that needs none);
//! the root itself stays, as an element has one shadow root for good.

use crate::form::{INPUT, TEXTAREA};
use crate::{
    Cx, InputKind, NodeKind, NodeRef, ShadowRootInit, ShadowRootMode, SlotAssignmentMode, names,
};

/// The shadow pseudo ids (the UA stylesheet styles them).
pub const INNER_EDITOR: &str = "-internal-inner-editor";
pub const PLACEHOLDER: &str = "-webkit-input-placeholder";
const EDITING_VIEW_PORT: &str = "-internal-editing-view-port";
const DECORATION_CONTAINER: &str = "-webkit-textfield-decoration-container";
pub const CANCEL_BUTTON: &str = "-webkit-search-cancel-button";
pub const SPIN_BUTTON: &str = "-webkit-inner-spin-button";
const SLIDER_CONTAINER: &str = "-webkit-slider-container";
const SLIDER_TRACK: &str = "-webkit-slider-runnable-track";
pub const SLIDER_THUMB: &str = "-webkit-slider-thumb";

/// The placeholder's display while it shows and while it doesn't (Blink's
/// `UpdatePlaceholderVisibility`, an important inline style: the author's
/// `::placeholder { display: inline }` doesn't change it).
const PLACEHOLDER_SHOWN: &str = "display: block !important";
const PLACEHOLDER_HIDDEN: &str = "display: none !important";

/// The clear button's style while the value is empty (Blink's
/// `SearchInputType::UpdateCancelButtonVisibility`): its room stays.
const CANCEL_HIDDEN: &str = "visibility: hidden";

/// A menu list's inner element's shadow pseudo id: the UA stylesheet gives
/// it its style and the theme its room (Blink's `MenuListInnerElement::
/// CustomStyleForLayoutObject`).
pub const MENU_LIST_INNER: &str = "-internal-menulist-inner";
/// An optgroup's label's: a row of a list box.
pub const OPTGROUP_LABEL: &str = "-internal-optgroup-label";
/// An option's label's (Blink's UA rules for `::-internal-option-label-
/// container` and `::-internal-option-slot`, as inline styles): the one of
/// it and the slot not shown is hidden.
const OPTION_LABEL: &str = "-internal-option-label-container";
const HIDDEN: &str = "display: none";

/// The tree a control has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tree {
    /// An `<input>` button's: its label.
    Button,
    /// A text field's: the inner editor, and the part of a decorated one
    /// (the clear button's or the spin button's shadow pseudo id).
    Field(Option<&'static str>),
    /// A text area's.
    Area,
    /// A range's: its container, its track and its thumb.
    Slider,
    /// A menu list's: its inner element.
    MenuList,
    /// A list box's: the slot of its options.
    ListBox,
    /// An optgroup's: its label and the slot of its options.
    OptGroup,
    /// An option's: its label and the slot of its children.
    Option,
}

impl<'gc> Cx<'_, 'gc> {
    /// The tree `element` needs, if it is a control that has one.
    fn tree_of(&self, element: NodeRef<'gc>) -> Option<Tree> {
        if self.is_textarea(element) {
            return Some(Tree::Area);
        }
        if self.is_select(element) {
            return Some(if self.select_uses_menu_list(element) {
                Tree::MenuList
            } else {
                Tree::ListBox
            });
        }
        if self.is_optgroup(element) {
            return Some(Tree::OptGroup);
        }
        if self.is_option(element) {
            return Some(Tree::Option);
        }
        Some(match self.input_kind(element)? {
            InputKind::Text
            | InputKind::Url
            | InputKind::Tel
            | InputKind::Email
            | InputKind::Password => Tree::Field(None),
            InputKind::Search => Tree::Field(Some(CANCEL_BUTTON)),
            InputKind::Number => Tree::Field(Some(SPIN_BUTTON)),
            InputKind::Submit | InputKind::Reset | InputKind::Button => Tree::Button,
            InputKind::Range => Tree::Slider,
            _ => return None,
        })
    }

    /// The tree `root`'s children make, if any.
    fn tree_in(&self, root: NodeRef<'gc>) -> Option<Tree> {
        let first = self.first_child(root)?;
        if self.kind(first) == NodeKind::Text {
            return Some(Tree::Button);
        }
        if self.is_part(first, SLIDER_CONTAINER) {
            return Some(Tree::Slider);
        }
        if self.is_part(first, MENU_LIST_INNER) {
            return Some(Tree::MenuList);
        }
        if self.is_part(first, OPTGROUP_LABEL) {
            return Some(Tree::OptGroup);
        }
        if self.is_part(first, OPTION_LABEL) {
            return Some(Tree::Option);
        }
        if self.is_slot(first) {
            return Some(Tree::ListBox);
        }
        if self.is_part(first, DECORATION_CONTAINER) {
            let part = [CANCEL_BUTTON, SPIN_BUTTON]
                .into_iter()
                .find(|part| self.child_part(first, part).is_some());
            return Some(Tree::Field(part));
        }
        let host = self.shadow_host(root)?;
        Some(if self.is_textarea(host) {
            Tree::Area
        } else {
            Tree::Field(None)
        })
    }

    /// `element`'s UA shadow root, if it has one.
    pub fn user_agent_shadow_root(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.shadow_root(element).filter(|root| {
            self.shadow_root_init(*root)
                .is_some_and(|init| init.mode == ShadowRootMode::UserAgent)
        })
    }

    /// `element`'s shadow pseudo id, if it is an element of a UA shadow
    /// tree that has one: which pseudo-element of the host it is.
    pub fn shadow_pseudo_id(&self, element: NodeRef<'gc>) -> Option<crate::DomString> {
        if self.kind(element) != NodeKind::Element {
            return None;
        }
        let root = self.containing_shadow_root(element)?;
        if self.shadow_root_init(root)?.mode != ShadowRootMode::UserAgent {
            return None;
        }
        // A `<use>`'s copy has its elements' attributes: `pseudo` is one.
        if self
            .shadow_host(root)
            .is_some_and(|host| self.is_svg_use(host))
        {
            return None;
        }
        self.attribute_value_by_id(element, names::local::PSEUDO)
            .map(|value| value.to_owned())
    }

    /// Whether `node` is the part of a UA shadow tree whose shadow pseudo
    /// id is `id`.
    pub fn is_part(&self, node: NodeRef<'gc>, id: &str) -> bool {
        self.shadow_pseudo_id(node)
            .is_some_and(|own| *own.to_str() == *id)
    }

    /// The input whose UA shadow tree's part `part` (an element whose
    /// shadow pseudo id is `id`) is, if it is one.
    fn field_of_part(&self, part: NodeRef<'gc>, id: &str) -> Option<NodeRef<'gc>> {
        if !self.is_part(part, id) {
            return None;
        }
        let root = self.containing_shadow_root(part)?;
        self.shadow_host(root)
            .filter(|host| self.input_kind(*host).is_some())
    }

    /// The number field whose spin button `part` is, if it is one.
    pub fn spin_button_field(&self, part: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.field_of_part(part, SPIN_BUTTON)
    }

    /// The search field whose clear button `part` is, if it is one.
    pub fn cancel_button_field(&self, part: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.field_of_part(part, CANCEL_BUTTON)
    }

    /// A node connected now, from the first control on (`inserted_connected`):
    /// an input, a textarea, a select, an optgroup or an option scheduled.
    /// Out of line: the insertion's walk stays small.
    #[inline(never)]
    pub(crate) fn schedule_control_tree(&self, node: NodeRef<'gc>) {
        if self.form_kind(node) & (INPUT | TEXTAREA) != 0
            || self.is_select(node)
            || self.is_option(node)
            || self.is_optgroup(node)
        {
            self.schedule_shadow_tree(node);
        }
    }

    /// `element` (a control) connected: its UA shadow tree built before
    /// its document is styled again, if it needs one and its document has
    /// a window (`create_scheduled_shadow_trees`).
    pub(crate) fn schedule_shadow_tree(&self, element: NodeRef<'gc>) {
        if self.user_agent_shadow_root(element).is_some() || self.tree_of(element).is_none() {
            return;
        }
        let id = self.node_id(element);
        let state = &self.changes.forms;
        let mut scheduled = state.scheduled.take();
        scheduled.insert(id);
        state.scheduled.set(scheduled);
    }

    /// Builds the UA shadow trees scheduled since (Blink's `Document::
    /// ProcessScheduledShadowTreeCreationsNow`): of the controls still
    /// connected to a document with a window. The host calls it before
    /// each style update.
    pub fn create_scheduled_shadow_trees(&self) {
        // In the order the controls were made (ids are given in order), as
        // they were scheduled, most often.
        let mut scheduled: Vec<crate::NodeId> =
            self.changes.forms.scheduled.take().into_iter().collect();
        scheduled.sort_unstable();
        for id in scheduled {
            let Some(element) = self.node_by_id(id) else {
                continue;
            };
            if self.is_connected(element) && self.window_of(self.document_of(element)).is_some() {
                self.ensure_shadow_tree(element);
            }
        }
    }

    /// Builds `element`'s UA shadow tree if it needs one and has none,
    /// whatever document it is in (Blink's `EnsureShadowSubtree`), and
    /// brings it up to date.
    pub fn ensure_shadow_tree(&self, element: NodeRef<'gc>) {
        let Some(tree) = self.tree_of(element) else {
            return;
        };
        let root = match self.user_agent_shadow_root(element) {
            Some(root) => root,
            None if self.shadow_root(element).is_none() => self.attach_shadow(
                element,
                ShadowRootInit {
                    mode: ShadowRootMode::UserAgent,
                    delegates_focus: false,
                    slot_assignment: SlotAssignmentMode::Named,
                    clonable: false,
                    serializable: false,
                },
            ),
            None => return,
        };
        if self.first_child(root).is_none() {
            self.build_shadow_tree(root, tree);
        }
        self.sync_shadow_tree(element);
    }

    /// `element`'s type changed: a tree it has that the new type doesn't
    /// need taken away, and made anew if it needs another; one it needs and
    /// hasn't, scheduled if it is connected.
    pub(crate) fn shadow_tree_type_changed(&self, element: NodeRef<'gc>) {
        let Some(root) = self.user_agent_shadow_root(element) else {
            if self.is_connected(element) {
                self.schedule_shadow_tree(element);
            }
            return;
        };
        let tree = self.tree_of(element);
        let made = self.tree_in(root);
        if made.is_some() && made != tree {
            while let Some(child) = self.first_child(root) {
                self.remove(child);
            }
        }
        if tree.is_some() {
            self.ensure_shadow_tree(element);
        }
    }

    /// A `<div>` of `document` whose shadow pseudo id is `id`.
    fn part(&self, document: NodeRef<'gc>, id: &str) -> NodeRef<'gc> {
        let div = self.create_element(document, crate::QualName::html("div"));
        self.set_attribute(div, "pseudo", id);
        div
    }

    /// Makes `tree`'s nodes in the empty `root` (Blink's
    /// `CreateShadowSubtree`): an inner editor with an empty text, in a
    /// text field's decoration container with its part; a button's empty
    /// label; a range's container, track and thumb.
    fn build_shadow_tree(&self, root: NodeRef<'gc>, tree: Tree) {
        let document = self.document_of(root);
        let append = |parent, child| {
            self.insert(parent, child, None);
        };
        let labeled = |id: &str| {
            let label = self.part(document, id);
            append(label, self.create_text(document, ""));
            append(root, label);
        };
        let slot = || {
            append(
                root,
                self.create_element(document, crate::QualName::html("slot")),
            )
        };
        match tree {
            Tree::Button => append(root, self.create_text(document, "")),
            Tree::MenuList => labeled(MENU_LIST_INNER),
            Tree::ListBox => slot(),
            Tree::OptGroup => {
                labeled(OPTGROUP_LABEL);
                slot();
            }
            Tree::Option => {
                // A `<span>`: the label is in the option's line.
                let label = self.create_element(document, crate::QualName::html("span"));
                self.set_attribute(label, "pseudo", OPTION_LABEL);
                append(label, self.create_text(document, ""));
                append(root, label);
                slot();
            }
            Tree::Slider => {
                let container = self.part(document, SLIDER_CONTAINER);
                let track = self.part(document, SLIDER_TRACK);
                append(track, self.part(document, SLIDER_THUMB));
                append(container, track);
                append(root, container);
            }
            Tree::Field(_) | Tree::Area => {
                let editor = self.part(document, INNER_EDITOR);
                append(editor, self.create_text(document, ""));
                if let Tree::Field(Some(part)) = tree {
                    let container = self.part(document, DECORATION_CONTAINER);
                    let view_port = self.part(document, EDITING_VIEW_PORT);
                    append(view_port, editor);
                    append(container, view_port);
                    append(container, self.part(document, part));
                    append(root, container);
                } else {
                    append(root, editor);
                }
            }
        }
    }

    /// The element the inner editor and the placeholder are in: the root,
    /// or a decorated field's editing view port.
    fn editor_parent(&self, root: NodeRef<'gc>) -> NodeRef<'gc> {
        match self.first_child(root) {
            Some(container) if self.is_part(container, DECORATION_CONTAINER) => {
                self.first_child(container).expect("the editing view port")
            }
            _ => root,
        }
    }

    /// The child of `parent` whose shadow pseudo id is `id`.
    fn child_part(&self, parent: NodeRef<'gc>, id: &str) -> Option<NodeRef<'gc>> {
        let mut child = self.first_child(parent);
        while let Some(at) = child {
            if self.is_part(at, id) {
                return Some(at);
            }
            child = self.next_sibling(at);
        }
        None
    }

    /// `element`'s inner editor, if its UA shadow tree has one: where its
    /// value is, and its caret.
    pub fn inner_editor(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let root = self.user_agent_shadow_root(element)?;
        self.child_part(self.editor_parent(root), INNER_EDITOR)
    }

    /// The text of `element`'s inner editor: its value, as the tree keeps
    /// it (UTF-16 offsets into it are offsets into the value).
    pub fn inner_editor_text(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.first_child(self.inner_editor(element)?)
    }

    /// `element`'s placeholder element, if its UA shadow tree has one.
    pub fn placeholder_element(&self, element: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let root = self.user_agent_shadow_root(element)?;
        self.child_part(self.editor_parent(root), PLACEHOLDER)
    }

    /// Something `element`'s UA shadow tree shows may have changed (its
    /// value, its `placeholder`, `min`, `max` or `step`): the tree brought
    /// up to date, if it has one.
    pub(crate) fn shadow_tree_changed(&self, element: NodeRef<'gc>) {
        let control = self.form_kind(element) & (INPUT | TEXTAREA) != 0
            || self.is_select(element)
            || self.is_option(element)
            || self.is_optgroup(element);
        if control && self.user_agent_shadow_root(element).is_some() {
            self.sync_shadow_tree(element);
        }
    }

    /// Makes `element`'s tree show what it is now: the text's data, an
    /// attribute's value set only where it changed.
    fn sync_shadow_tree(&self, element: NodeRef<'gc>) {
        let Some(root) = self.user_agent_shadow_root(element) else {
            return;
        };
        match self.tree_in(root) {
            Some(Tree::Button) => self.sync_label(element, root),
            Some(Tree::Slider) => self.sync_thumb(element, root),
            Some(tree @ (Tree::Field(_) | Tree::Area)) => self.sync_text(element, root, tree),
            Some(Tree::MenuList) => self.sync_menu_list(element, root),
            Some(Tree::OptGroup) => self.sync_optgroup_label(element, root),
            Some(Tree::Option) => self.sync_option_label(element, root),
            Some(Tree::ListBox) | None => {}
        }
    }

    /// `node`'s data made `data` if it isn't.
    fn set_data_if_changed(&self, node: NodeRef<'gc>, data: &str) {
        let same = self.data(node).is_some_and(|old| *old.to_str() == *data);
        if !same {
            self.set_data(node, data);
        }
    }

    /// `element`'s attribute `name` made `value` (or removed, none) if it
    /// isn't.
    fn set_attribute_if_changed(&self, element: NodeRef<'gc>, name: &str, value: Option<&str>) {
        let old = self
            .attribute_value_ns(element, None, name)
            .map(|old| old.to_owned());
        match value {
            Some(value) if old.as_ref().is_none_or(|old| *old.to_str() != *value) => {
                self.set_attribute(element, name, value);
            }
            None if old.is_some() => {
                self.remove_attribute(element, name);
            }
            _ => {}
        }
    }

    /// A menu list's inner element: the text it shows, and the labels it is
    /// as wide as, which layout measures.
    fn sync_menu_list(&self, select: NodeRef<'gc>, root: NodeRef<'gc>) {
        let inner = self.first_child(root).expect("the inner element");
        let text = self.first_child(inner).expect("the inner element's text");
        self.set_data_if_changed(text, &self.menu_list_text(select));
        let labels = self.menu_list_labels(select);
        let same = self
            .attribute_value_ns(inner, None, "labels")
            .is_some_and(|old| *old.to_owned().to_str() == *labels);
        if !same {
            self.set_attribute(inner, "labels", &labels);
            // Its width: the select's box, which measures them, built again
            // (Blink's `SetNeedsLayoutAndIntrinsicWidthsRecalc`).
            if self.is_connected(select) {
                self.mark_layout(select, crate::LayoutTreeFlags::NEEDS_REATTACH);
            }
        }
    }

    /// An optgroup's label: its `label` attribute.
    fn sync_optgroup_label(&self, optgroup: NodeRef<'gc>, root: NodeRef<'gc>) {
        let label = self.first_child(root).expect("the label");
        let text = self.first_child(label).expect("the label's text");
        let value = self
            .attribute_value_by_id(optgroup, names::local::LABEL)
            .map(|label| label.borrow().to_str().into_owned())
            .unwrap_or_default();
        self.set_data_if_changed(text, &value);
    }

    /// An option's `label` shown in place of its children where it has one
    /// that isn't empty (Blink's `option[label]:not([label=""])` rules).
    fn sync_option_label(&self, option: NodeRef<'gc>, root: NodeRef<'gc>) {
        let container = self.first_child(root).expect("the label");
        let text = self.first_child(container).expect("the label's text");
        let slot = self.next_sibling(container).expect("the slot");
        let label = self
            .attribute_value_by_id(option, names::local::LABEL)
            .map(|label| label.borrow().to_str().into_owned())
            .filter(|label| !label.is_empty());
        let shown = label.is_some();
        self.set_attribute_if_changed(container, "style", (!shown).then_some(HIDDEN));
        self.set_attribute_if_changed(slot, "style", shown.then_some(HIDDEN));
        self.set_data_if_changed(text, label.as_deref().unwrap_or(""));
    }

    /// A button's label: its `value`, else its type's default (Blink's
    /// `ValueOrDefaultLabel`; English, as Chrome's in `en-US`). An empty
    /// one is a zero-width space: the button keeps a line, as Chrome's (an
    /// empty `<button>` has none).
    fn sync_label(&self, element: NodeRef<'gc>, root: NodeRef<'gc>) {
        let mut label = match self.attribute_value_by_id(element, names::local::VALUE) {
            Some(value) => value.borrow().to_str().into_owned(),
            None => match self.input_kind(element) {
                Some(InputKind::Submit) => "Submit",
                Some(InputKind::Reset) => "Reset",
                _ => "",
            }
            .to_owned(),
        };
        if label.is_empty() {
            label.push('\u{200B}');
        }
        if let Some(text) = self.first_child(root) {
            self.set_data_if_changed(text, &label);
        }
    }

    /// Gives the range's thumb its ratio, from the value (Blink's
    /// `SliderThumbElement::SetPositionFromValue`).
    fn sync_thumb(&self, element: NodeRef<'gc>, root: NodeRef<'gc>) {
        let thumb = self
            .first_child(root)
            .and_then(|container| self.first_child(container))
            .and_then(|track| self.first_child(track))
            .expect("the thumb");
        let ratio = self.range_ratio(element).to_string();
        self.set_attribute_if_changed(thumb, "ratio", Some(&ratio));
    }

    /// Makes the inner editor hold `element`'s value, the placeholder
    /// (there while the control has a `placeholder` attribute) show while
    /// the value is empty, and a search field's clear button too.
    fn sync_text(&self, element: NodeRef<'gc>, root: NodeRef<'gc>, tree: Tree) {
        let value = self.text_control_value(element);
        let value = value.to_str();
        let parent = self.editor_parent(root);
        let editor = self
            .child_part(parent, INNER_EDITOR)
            .expect("the inner editor");
        let text = self.first_child(editor).expect("the inner editor's text");
        self.set_data_if_changed(text, &value);
        if let Tree::Field(Some(CANCEL_BUTTON)) = tree {
            let container = self.first_child(root).expect("the decoration container");
            if let Some(cancel) = self.child_part(container, CANCEL_BUTTON) {
                self.set_attribute_if_changed(
                    cancel,
                    "style",
                    value.is_empty().then_some(CANCEL_HIDDEN),
                );
            }
        }
        let placeholder = self
            .attribute_value_by_id(element, names::local::PLACEHOLDER)
            .map(|text| {
                let text = text.borrow().to_str().into_owned();
                // A text field's placeholder is one line (HTML's "strip
                // newlines"); a text area's line breaks are its own.
                if tree == Tree::Area {
                    text
                } else {
                    text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect()
                }
            });
        let holder = self.child_part(parent, PLACEHOLDER);
        match (placeholder, holder) {
            (None, Some(holder)) => self.remove(holder),
            (None, None) => {}
            (Some(placeholder), holder) => {
                let holder = holder.unwrap_or_else(|| {
                    let document = self.document_of(root);
                    let holder = self.part(document, PLACEHOLDER);
                    self.insert(holder, self.create_text(document, ""), None);
                    self.insert(parent, holder, Some(editor));
                    holder
                });
                let display = if value.is_empty() {
                    PLACEHOLDER_SHOWN
                } else {
                    PLACEHOLDER_HIDDEN
                };
                self.set_attribute_if_changed(holder, "style", Some(display));
                if let Some(text) = self.first_child(holder) {
                    self.set_data_if_changed(text, &placeholder);
                }
            }
        }
    }
}
