//! HTML's form pseudo-classes (https://html.spec.whatwg.org/#pseudo-classes)
//! as selector matching asks them of an element (Blink's `IsChecked`,
//! `ShouldAppearIndeterminate`, `IsRequiredFormControl`,
//! `MatchesReadWritePseudoClass`, `IsPlaceholderVisible`,
//! `MatchesDefaultPseudoClass`; `:disabled` is `is_disabled_form_control`),
//! and their changes noted for the style phase where no attribute's change
//! says them (Blink's `PseudoStateChanged`): `AttributeChanges`' form bits.
//!
//! A radio button's `:indeterminate` is its group's (none checked): a
//! radio button's checkedness, group or tree changing notes it on every
//! radio button of the tree, if the document's style has the pseudo-class
//! at all. A form's default button (its first submit button) is kept in its
//! data, found again as a submit button comes, goes or changes its type
//! (Blink's `HTMLFormElement::default_button_`).

use elise::Gc;

use crate::form::DISABLABLE;
use crate::{AttributeChanges, Cx, InputKind, NodeRef, QualifiedName, names};

impl<'gc> Cx<'_, 'gc> {
    /// `:checked`: a checkbox or a radio button that is checked, an
    /// `<option>` that is selected.
    pub fn matches_checked(&self, element: NodeRef<'gc>) -> bool {
        match self.input_kind(element) {
            Some(InputKind::Checkbox | InputKind::Radio) => self.checkedness(element),
            Some(_) => false,
            None => self.is_option(element) && self.option_selectedness(element),
        }
    }

    /// `:disabled`: a form control that a `disabled` attribute disables
    /// (and a fieldset) that is disabled, an option or an optgroup that is
    /// (`select`).
    pub fn matches_disabled(&self, element: NodeRef<'gc>) -> bool {
        match self.option_matches_disabled(element) {
            Some(disabled) => disabled,
            None => self.is_disabled_form_control(element),
        }
    }

    /// `:indeterminate`: a checkbox whose `indeterminate` is set, a radio
    /// button none of whose group is checked, a `<progress>` with no
    /// `value`.
    pub fn matches_indeterminate(&self, element: NodeRef<'gc>) -> bool {
        match self.input_kind(element) {
            Some(InputKind::Checkbox) => self.indeterminate(element),
            Some(InputKind::Radio) => {
                !self.checkedness(element) && !self.radio_group_has_checked(element)
            }
            Some(_) => false,
            None => {
                self.qualified_name(element) == Some(QualifiedName::html(names::local::PROGRESS))
                    && self
                        .attribute_value_by_id(element, names::local::VALUE)
                        .is_none()
            }
        }
    }

    /// `:enabled`: a form control that a `disabled` attribute disables
    /// (and a fieldset), an option or an optgroup, that isn't disabled.
    pub fn matches_enabled(&self, element: NodeRef<'gc>) -> bool {
        match self.option_matches_disabled(element) {
            Some(disabled) => !disabled,
            None => {
                self.form_kind(element) & DISABLABLE != 0 && !self.is_disabled_form_control(element)
            }
        }
    }

    /// Whether `element` is `:required` (`Some(true)`) or `:optional`
    /// (`Some(false)`): an input of a type `required` applies to, a
    /// `<select>`, a `<textarea>`; none for another.
    pub fn matches_required(&self, element: NodeRef<'gc>) -> Option<bool> {
        let applies = match self.input_kind(element) {
            Some(kind) => {
                takes_readonly(kind)
                    || matches!(
                        kind,
                        InputKind::Checkbox | InputKind::Radio | InputKind::File
                    )
            }
            None => {
                self.qualified_name(element) == Some(QualifiedName::html(names::local::SELECT))
                    || self.qualified_name(element)
                        == Some(QualifiedName::html(names::local::TEXTAREA))
            }
        };
        applies.then(|| {
            self.attribute_value_by_id(element, names::local::REQUIRED)
                .is_some()
        })
    }

    /// `:read-write`: an input of a type `readonly` applies to, or a
    /// `<textarea>`, that isn't read-only nor disabled; any other element
    /// that is editable or an editing host (`:read-only` is any other
    /// element).
    pub fn matches_read_write(&self, element: NodeRef<'gc>) -> bool {
        let applies = match self.input_kind(element) {
            Some(kind) => takes_readonly(kind),
            None if self.qualified_name(element)
                == Some(QualifiedName::html(names::local::TEXTAREA)) =>
            {
                true
            }
            None => return self.is_editable(element),
        };
        applies
            && self
                .attribute_value_by_id(element, names::local::READONLY)
                .is_none()
            && !self.is_disabled_form_control(element)
    }

    /// `:placeholder-shown`: an input of a type `placeholder` applies to,
    /// or a textarea, with one, whose value is empty.
    pub fn matches_placeholder_shown(&self, element: NodeRef<'gc>) -> bool {
        if self.is_textarea(element) {
            return self
                .attribute_value_by_id(element, names::local::PLACEHOLDER)
                .is_some()
                && self.textarea_value(element).is_empty();
        }
        matches!(
            self.input_kind(element),
            Some(
                InputKind::Text
                    | InputKind::Search
                    | InputKind::Url
                    | InputKind::Tel
                    | InputKind::Email
                    | InputKind::Password
                    | InputKind::Number
            )
        ) && self
            .attribute_value_by_id(element, names::local::PLACEHOLDER)
            .is_some()
            && self.input_value(element).to_str().is_empty()
    }

    /// `:default`: a checkbox or a radio button with a `checked` attribute,
    /// an option with a `selected` attribute; a form's default button.
    pub fn matches_default(&self, element: NodeRef<'gc>) -> bool {
        match self.input_kind(element) {
            Some(InputKind::Checkbox | InputKind::Radio) => self
                .attribute_value_by_id(element, names::local::CHECKED)
                .is_some(),
            None if self.is_option(element) => self
                .attribute_value_by_id(element, names::local::SELECTED)
                .is_some(),
            _ => self
                .form_owner(element)
                .and_then(|form| self.default_button(form))
                .is_some_and(|button| Gc::ptr_eq(button, element)),
        }
    }

    /// Whether `element` is a submit button: an input whose type is
    /// `submit` or `image`, a `<button>` whose type is `submit`.
    pub(crate) fn is_submit_button(&self, element: NodeRef<'gc>) -> bool {
        match self.input_kind(element) {
            Some(kind) => matches!(kind, InputKind::Submit | InputKind::Image),
            None => {
                self.qualified_name(element) == Some(QualifiedName::html(names::local::BUTTON))
                    && self.button_type(element) == "submit"
            }
        }
    }

    /// `form`'s default button found again (a submit button of it came,
    /// went or changed its type): the first of its elements that is a
    /// submit button. If it is another, both are noted.
    pub(crate) fn default_button_changed(&self, form: NodeRef<'gc>) {
        let elements = self.form_listed_elements(form);
        let first = (0..elements.len())
            .filter_map(|index| elements.get(self.gc, index))
            .find(|element| self.is_submit_button(*element));
        let old = self.default_button(form);
        let same = match (old, first) {
            (Some(old), Some(first)) => Gc::ptr_eq(old, first),
            (old, first) => old.is_none() && first.is_none(),
        };
        if same {
            return;
        }
        self.set_default_button(form, first);
        for button in [old, first].into_iter().flatten() {
            self.state_changed(button, AttributeChanges::DEFAULT);
        }
    }

    /// Whether the style of `element`'s document has a pseudo-class of
    /// `state` (an `AttributeChanges` bit): if not, its changes needn't be
    /// looked for.
    pub(crate) fn style_uses_state(&self, element: NodeRef<'gc>, state: u16) -> bool {
        self.style_names(element)
            .is_some_and(|names| names.everything || names.states & state != 0)
    }

    /// A radio button of `node`'s tree changed its checkedness, group or
    /// tree: each radio button of the tree's `:indeterminate` noted.
    pub(crate) fn radio_groups_changed(&self, node: NodeRef<'gc>) {
        if !self.is_connected(node) || !self.style_uses_state(node, AttributeChanges::INDETERMINATE)
        {
            return;
        }
        let root = self.root_node(node, false);
        let mut at = Some(root);
        while let Some(at_node) = at {
            if self.input_kind(at_node) == Some(InputKind::Radio) {
                self.state_changed(at_node, AttributeChanges::INDETERMINATE);
            }
            at = self.following_within(at_node, root);
        }
    }
}

/// Whether `readonly` applies to an input of `kind` (and `required`, with
/// a checkbox's, a radio button's and a file input's).
fn takes_readonly(kind: InputKind) -> bool {
    matches!(
        kind,
        InputKind::Text
            | InputKind::Search
            | InputKind::Url
            | InputKind::Tel
            | InputKind::Email
            | InputKind::Password
            | InputKind::Date
            | InputKind::Month
            | InputKind::Week
            | InputKind::Time
            | InputKind::DatetimeLocal
            | InputKind::Number
    )
}
