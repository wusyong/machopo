//! HTML's `<input>` (https://html.spec.whatwg.org/#the-input-element): its
//! type, its value and checkedness, and the radio button groups. Each input has
//! its type's object from its making (Blink's `HTMLInputElement::
//! input_type_`, an `InputType`): the state its `type` attribute is in,
//! changed as the attribute is (HTML's type change steps), and the state
//! the element keeps (Blink's `is_checked_`, `dirty_checkedness_`,
//! `is_indeterminate_`, `non_attribute_value_`). The value is kept only
//! once dirty: until then it is the `value` attribute's, sanitized as it
//! is read (as the attribute's changes would set it), so that most inputs
//! keep none; a kept one is sanitized again as it is read, the attributes
//! the sanitization reads (`min`, `max`, `step`, `multiple`) having maybe
//! changed since. What each type does is a `match` on the kind, as
//! Blink's `InputType` subclasses override (`CheckboxInputType`,
//! `RadioInputType`).
//!
//! A radio button's group is found when a checked one may uncheck the
//! others (Blink keeps `RadioButtonGroupScope`'s groups by name): its form
//! owner's elements, or its tree's radio buttons with no owner.

use std::cell::Cell;

use elise::{Gc, GcObject, LocalExternal};

use crate::form::{INPUT, RADIOS_SEEN};
use crate::numbers::{is_ascii_whitespace, is_valid_float, parse_float};
use crate::{AttributeChanges, Cx, DomString, NodeRef, names, sites};

thread_local! {
    /// How many times, on this thread, an input's type changed (the type
    /// change steps past the same type). Counted in debug builds only.
    static INPUT_TYPE_CHANGES: Cell<u64> = const { Cell::new(0) };
}

/// How many times, on this thread, an input's type changed: what tells an
/// input the parser makes is made of its type, not changed to it. `None` in
/// release builds. For tests.
pub fn input_type_changes_for_testing() -> Option<u64> {
    cfg!(debug_assertions).then(|| INPUT_TYPE_CHANGES.get())
}

/// HTML's value modes of an input's type: what `value` is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueMode {
    /// The element's value.
    Value,
    /// The `value` attribute, or the empty string.
    Default,
    /// The `value` attribute, or `on`.
    DefaultOn,
    /// The selected files' names (machopo has no files: the empty string).
    Filename,
}

/// The state an input's `type` attribute is in (HTML's keywords; `Text`
/// for none, or a value that is none of them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum InputKind {
    Text,
    Hidden,
    Search,
    Tel,
    Url,
    Email,
    Password,
    Date,
    Month,
    Week,
    Time,
    DatetimeLocal,
    Number,
    Range,
    Color,
    Checkbox,
    Radio,
    File,
    Submit,
    Image,
    Reset,
    Button,
}

impl InputKind {
    /// Each kind's keyword, by the kind's number.
    const KEYWORDS: [&'static str; 22] = [
        "text",
        "hidden",
        "search",
        "tel",
        "url",
        "email",
        "password",
        "date",
        "month",
        "week",
        "time",
        "datetime-local",
        "number",
        "range",
        "color",
        "checkbox",
        "radio",
        "file",
        "submit",
        "image",
        "reset",
        "button",
    ];

    const ALL: [Self; 22] = [
        Self::Text,
        Self::Hidden,
        Self::Search,
        Self::Tel,
        Self::Url,
        Self::Email,
        Self::Password,
        Self::Date,
        Self::Month,
        Self::Week,
        Self::Time,
        Self::DatetimeLocal,
        Self::Number,
        Self::Range,
        Self::Color,
        Self::Checkbox,
        Self::Radio,
        Self::File,
        Self::Submit,
        Self::Image,
        Self::Reset,
        Self::Button,
    ];

    /// The kind a `type` attribute's value is (ASCII case-insensitively);
    /// `Text` for none.
    pub fn from_attribute(value: Option<&str>) -> Self {
        value
            .and_then(|value| {
                Self::KEYWORDS
                    .iter()
                    .position(|keyword| keyword.eq_ignore_ascii_case(value))
            })
            .map_or(Self::Text, |index| Self::ALL[index])
    }

    /// The kind's keyword (`type`'s value).
    pub fn keyword(self) -> &'static str {
        Self::KEYWORDS[self as usize]
    }

    fn from_raw(raw: u8) -> Self {
        Self::ALL[usize::from(raw)]
    }

    /// The kind's value mode.
    pub fn value_mode(self) -> ValueMode {
        match self {
            Self::Hidden | Self::Submit | Self::Image | Self::Reset | Self::Button => {
                ValueMode::Default
            }
            Self::Checkbox | Self::Radio => ValueMode::DefaultOn,
            Self::File => ValueMode::Filename,
            _ => ValueMode::Value,
        }
    }
}

/// An input's type and state.
#[derive(GcObject)]
pub struct InputType {
    /// `InputKind`'s number.
    kind: Cell<u8>,
    /// HTML's checkedness and dirty checkedness flag.
    checked: Cell<bool>,
    dirty_checked: Cell<bool>,
    indeterminate: Cell<bool>,
    /// The value once dirty (HTML's dirty value flag set), as it was set;
    /// none while it is the `value` attribute's. In a `Cell`, taken and put
    /// back.
    value: LocalExternal<Cell<Option<DomString>>>,
    /// Its selection (`text_control`'s), if its type has one: Blink's
    /// `TextControlElement::cached_selection_start_` and its kin.
    pub(crate) selection_start: Cell<u32>,
    pub(crate) selection_end: Cell<u32>,
    pub(crate) selection_direction: Cell<u8>,
    /// Whether `selectionchange` is scheduled at it (`selection`).
    pub(crate) selectionchange_scheduled: Cell<bool>,
    /// Its value before the user's first change since the last commit
    /// (`text_control`'s `note_edit`): what a commit compares with.
    pub(crate) before_edit: LocalExternal<Cell<Option<DomString>>>,
    /// A number's spin button's press (`SpinState`'s: its number, the half
    /// it began in, the half the pointer is in now).
    spin_press: Cell<u64>,
    spin_halves: Cell<[u8; 2]>,
}

/// A number's spin button's press, as the user holds it (Blink's
/// `SpinButtonElement`'s `up_down_state_` and `press_starting_state_`):
/// the press's number (each press, and its release, another, so that a
/// repeat of an old press does nothing), the half it began in (up: true;
/// none while not pressed) and the half the pointer is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SpinState {
    pub press: u64,
    pub pressed_up: Option<bool>,
    pub up: Option<bool>,
}

fn half(raw: u8) -> Option<bool> {
    match raw {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn half_raw(up: Option<bool>) -> u8 {
    match up {
        Some(true) => 1,
        Some(false) => 2,
        None => 0,
    }
}

/// What a checkbox's or a radio button's legacy-pre-activation behavior
/// keeps for its legacy-canceled-activation behavior and its activation
/// behavior (Blink's `ClickHandlingState`).
#[derive(Clone, Copy)]
pub struct ClickState<'gc> {
    pub checked: bool,
    pub indeterminate: bool,
    /// A radio button's: the one of its group that was checked.
    pub checked_radio: Option<NodeRef<'gc>>,
}

impl<'gc> Cx<'_, 'gc> {
    /// An input was made (`form_element_made`): its type's object, `Text`
    /// with none of its attributes yet (the parser's and a copy's are set
    /// after, as script's are).
    pub(crate) fn input_made(&self, element: NodeRef<'gc>) {
        let input = self.gc.alloc_beside(
            element,
            &sites::OTHER,
            InputTypeInit {
                kind: Cell::new(InputKind::Text as u8),
                checked: Cell::new(false),
                dirty_checked: Cell::new(false),
                indeterminate: Cell::new(false),
                value: LocalExternal::new(Cell::new(None)),
                selection_start: Cell::new(0),
                selection_end: Cell::new(0),
                selection_direction: Cell::new(0),
                selectionchange_scheduled: Cell::new(false),
                before_edit: LocalExternal::new(Cell::new(None)),
                spin_press: Cell::new(0),
                spin_halves: Cell::new([0, 0]),
            },
        );
        self.association_or_new(element)
            .input
            .set(self.gc, Some(input));
    }

    /// `element`'s type's object, if it is an HTML `<input>`.
    fn input(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, InputType>> {
        if self.form_kind(element) & INPUT == 0 {
            return None;
        }
        self.association(element)?.input.get(self.gc)
    }

    /// `element`'s type, if it is an HTML `<input>`.
    pub fn input_kind(&self, element: NodeRef<'gc>) -> Option<InputKind> {
        Some(InputKind::from_raw(self.input(element)?.kind.get()))
    }

    fn is_radio(&self, element: NodeRef<'gc>) -> bool {
        self.input_kind(element) == Some(InputKind::Radio)
    }

    /// An input's attribute whose name is a `CONTROL_ATTRIBUTE` changed
    /// (`control_attribute_changed`): HTML's attribute change steps.
    pub(crate) fn input_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        local: crate::NameId,
        present: bool,
    ) {
        let Some(input) = self.input(element) else {
            return;
        };
        match local {
            names::local::TYPE => self.input_type_changed(element, input),
            // While not dirty, the checkedness is the attribute's.
            names::local::CHECKED if !input.dirty_checked.get() => {
                self.set_checkedness(element, present)
            }
            // A checked radio button with another name unchecks its new
            // group; groups changed.
            names::local::NAME if self.is_radio(element) => {
                self.uncheck_radio_group(element);
                self.radio_groups_changed(element);
            }
            // What its UA shadow tree shows: its value, its placeholder,
            // what its value's sanitization reads.
            names::local::VALUE
            | names::local::PLACEHOLDER
            | names::local::MIN
            | names::local::MAX
            | names::local::STEP
            | names::local::MULTIPLE => self.shadow_tree_changed(element),
            _ => {}
        }
    }

    /// A new input, with none of its attributes yet, made of the type
    /// `value` (its `type` attribute's, about to be set: Blink's
    /// `InitializeTypeInParsing`), as the type change steps from `Text`
    /// would leave it: with no value, no checkedness, no form owner and no
    /// shadow tree there is nothing else of them to do. The attribute's
    /// change then finds the type the same.
    pub(crate) fn input_made_of_type(&self, element: NodeRef<'gc>, value: &str) {
        let Some(input) = self.input(element) else {
            return;
        };
        let kind = InputKind::from_attribute(Some(value));
        input.kind.set(kind as u8);
        if kind == InputKind::Radio {
            self.see(RADIOS_SEEN);
        }
    }

    /// HTML's type change steps (Blink's `HTMLInputElement::UpdateType`):
    /// the kind the attribute now says; a checked radio button unchecks
    /// its group; a type with a selection after one without has the cursor
    /// at the start.
    fn input_type_changed(&self, element: NodeRef<'gc>, input: Gc<'gc, InputType>) {
        let kind = match self.attribute_value_by_id(element, names::local::TYPE) {
            Some(value) => InputKind::from_attribute(Some(&value.borrow().to_str())),
            None => InputKind::Text,
        };
        if kind as u8 == input.kind.get() {
            return;
        }
        if cfg!(debug_assertions) {
            INPUT_TYPE_CHANGES.set(INPUT_TYPE_CHANGES.get().wrapping_add(1));
        }
        let (old, new) = (
            InputKind::from_raw(input.kind.get()).value_mode(),
            kind.value_mode(),
        );
        // From the element's value to the attribute's: a value not empty
        // goes into the attribute (its value read by the old type).
        let value = (old == ValueMode::Value
            && matches!(new, ValueMode::Default | ValueMode::DefaultOn))
        .then(|| self.input_value(element))
        .filter(|value| !value.to_str().is_empty());
        let was = InputKind::from_raw(input.kind.get());
        let was_selectable = self.selection_applies(element);
        input.kind.set(kind as u8);
        if was == InputKind::Radio || kind == InputKind::Radio {
            self.radio_groups_changed(element);
        }
        if let Some(form) = self.form_owner(element)
            && (matches!(was, InputKind::Submit | InputKind::Image)
                || self.is_submit_button(element))
        {
            self.default_button_changed(form);
        }
        // To the element's value from another mode: the attribute's, not
        // dirty; a file input's isn't read, and is the attribute's again as
        // it leaves.
        if old != new {
            input.value.get().set(None);
        }
        if let Some(value) = value {
            self.set_attribute(element, "value", &value.to_str());
        }
        if !was_selectable && self.selection_applies(element) {
            self.cursor_to_start(element);
        }
        if kind == InputKind::Radio {
            self.see(RADIOS_SEEN);
            self.uncheck_radio_group(element);
        }
        self.shadow_tree_type_changed(element);
    }

    /// A number's spin button's press (default for another element).
    pub fn spin_state(&self, element: NodeRef<'gc>) -> SpinState {
        self.input(element)
            .map_or_else(SpinState::default, |input| {
                let [pressed_up, up] = input.spin_halves.get();
                SpinState {
                    press: input.spin_press.get(),
                    pressed_up: half(pressed_up),
                    up: half(up),
                }
            })
    }

    pub fn set_spin_state(&self, element: NodeRef<'gc>, state: SpinState) {
        if let Some(input) = self.input(element) {
            input.spin_press.set(state.press);
            input
                .spin_halves
                .set([half_raw(state.pressed_up), half_raw(state.up)]);
        }
    }

    /// An input's checkedness; false for another element.
    pub fn checkedness(&self, element: NodeRef<'gc>) -> bool {
        self.input(element).is_some_and(|input| input.checked.get())
    }

    /// An input's checkedness made `checked`; a radio button checked
    /// unchecks the others of its group.
    fn set_checkedness(&self, element: NodeRef<'gc>, checked: bool) {
        let Some(input) = self.input(element) else {
            return;
        };
        self.set_checked_bit(element, input, checked);
        if checked {
            self.uncheck_radio_group(element);
        }
    }

    /// `input`'s checkedness made `checked`: if it changed, `:checked`
    /// noted, and a radio button's groups'.
    fn set_checked_bit(&self, element: NodeRef<'gc>, input: Gc<'gc, InputType>, checked: bool) {
        if input.checked.get() == checked {
            return;
        }
        input.checked.set(checked);
        self.state_changed(element, AttributeChanges::CHECKED);
        if InputKind::from_raw(input.kind.get()) == InputKind::Radio {
            self.radio_groups_changed(element);
        }
    }

    /// `input`'s indeterminate made `indeterminate`, noted if it changed.
    fn set_indeterminate_bit(
        &self,
        element: NodeRef<'gc>,
        input: Gc<'gc, InputType>,
        indeterminate: bool,
    ) {
        if input.indeterminate.get() != indeterminate {
            input.indeterminate.set(indeterminate);
            self.state_changed(element, AttributeChanges::INDETERMINATE);
        }
    }

    /// Whether one of `element`'s radio button group (it aside) is checked.
    pub(crate) fn radio_group_has_checked(&self, element: NodeRef<'gc>) -> bool {
        let mut found = false;
        self.for_each_in_radio_group(element, |other| {
            found = self.checkedness(other);
            !found
        });
        found
    }

    /// `checked`'s setter: the checkedness, now dirty.
    pub fn set_checked(&self, element: NodeRef<'gc>, checked: bool) {
        let Some(input) = self.input(element) else {
            return;
        };
        input.dirty_checked.set(true);
        self.set_checkedness(element, checked);
    }

    /// `indeterminate`.
    pub fn indeterminate(&self, element: NodeRef<'gc>) -> bool {
        self.input(element)
            .is_some_and(|input| input.indeterminate.get())
    }

    pub fn set_indeterminate(&self, element: NodeRef<'gc>, indeterminate: bool) {
        if let Some(input) = self.input(element) {
            self.set_indeterminate_bit(element, input, indeterminate);
        }
    }

    /// HTML's cloning steps of an input: the copy has `original`'s value,
    /// checkedness, their dirty flags and indeterminate.
    pub(crate) fn input_cloned(&self, original: NodeRef<'gc>, copy: NodeRef<'gc>) {
        let (Some(from), Some(to)) = (self.input(original), self.input(copy)) else {
            return;
        };
        to.checked.set(from.checked.get());
        to.dirty_checked.set(from.dirty_checked.get());
        to.indeterminate.set(from.indeterminate.get());
        let value = from.into_ref().value.get().take();
        from.into_ref().value.get().set(value.clone());
        to.into_ref().value.get().set(value);
    }

    /// `value` of an input, by its value mode: its value (sanitized), the
    /// `value` attribute (or `on`), or for a file input the empty string.
    pub fn input_value(&self, element: NodeRef<'gc>) -> DomString {
        let Some(input) = self.input(element) else {
            return DomString::new("");
        };
        let kind = InputKind::from_raw(input.kind.get());
        let attribute = || {
            self.attribute_value_by_id(element, names::local::VALUE)
                .map(|value| value.to_owned())
        };
        match kind.value_mode() {
            ValueMode::Value => {
                let cell = input.into_ref().value.get();
                let kept = cell.take();
                cell.set(kept.clone());
                let value = kept
                    .or_else(attribute)
                    .unwrap_or_else(|| DomString::new(""));
                let sanitized = self.sanitize(element, kind, &value.to_str());
                if *sanitized == *value.to_str() {
                    value
                } else {
                    DomString::new(&sanitized)
                }
            }
            ValueMode::Default => attribute().unwrap_or_else(|| DomString::new("")),
            ValueMode::DefaultOn => attribute().unwrap_or_else(|| DomString::new("on")),
            ValueMode::Filename => DomString::new(""),
        }
    }

    /// Setting `value`, by the value mode: the element's value, sanitized,
    /// dirty now, the cursor to its end if that changed it; or the `value`
    /// attribute; a file input's only to the empty string, which empties
    /// its files (`Err` for another).
    #[expect(
        clippy::result_unit_err,
        reason = "one error: a file input's value set to a string; the API makes it its `InvalidStateError`"
    )]
    pub fn set_input_value(&self, element: NodeRef<'gc>, value: &str) -> Result<(), ()> {
        let Some(input) = self.input(element) else {
            return Ok(());
        };
        let kind = InputKind::from_raw(input.kind.get());
        match kind.value_mode() {
            ValueMode::Value => {
                let old = self.input_value(element);
                if self.set_input_value_keeping_selection(element, value) != *old.to_str() {
                    self.cursor_to_end(element);
                }
            }
            ValueMode::Default | ValueMode::DefaultOn => {
                self.set_attribute(element, "value", value)
            }
            ValueMode::Filename if value.is_empty() => {}
            ValueMode::Filename => return Err(()),
        }
        Ok(())
    }

    /// An input's value (its type's value mode the element's value) set to
    /// `value` sanitized, dirty now, the selection as it is: what the value
    /// is now.
    pub(crate) fn set_input_value_keeping_selection(
        &self,
        element: NodeRef<'gc>,
        value: &str,
    ) -> String {
        let Some(input) = self.input(element) else {
            return String::new();
        };
        let sanitized = self.sanitize(element, InputKind::from_raw(input.kind.get()), value);
        input
            .into_ref()
            .value
            .get()
            .set(Some(DomString::new(&sanitized)));
        self.state_changed(element, AttributeChanges::VALUE);
        self.shadow_tree_changed(element);
        sanitized
    }

    /// HTML's value sanitization algorithm of `kind` for `element`'s
    /// `value`: the text types', `url`'s, `email`'s, `number`'s, `range`'s
    /// and `color`'s; the date and time types' values stay as they are.
    fn sanitize(&self, element: NodeRef<'gc>, kind: InputKind, value: &str) -> String {
        let strip_newlines = |value: &str| {
            value
                .chars()
                .filter(|c| !matches!(c, '\n' | '\r'))
                .collect::<String>()
        };
        match kind {
            InputKind::Text | InputKind::Search | InputKind::Tel | InputKind::Password => {
                strip_newlines(value)
            }
            InputKind::Url => strip_newlines(value)
                .trim_matches(is_ascii_whitespace)
                .to_owned(),
            InputKind::Email
                if self
                    .attribute_value_by_id(element, names::local::MULTIPLE)
                    .is_some() =>
            {
                value
                    .split(',')
                    .map(|address| address.trim_matches(is_ascii_whitespace))
                    .collect::<Vec<_>>()
                    .join(",")
            }
            InputKind::Email => strip_newlines(value)
                .trim_matches(is_ascii_whitespace)
                .to_owned(),
            // A valid floating-point number, which a double holds.
            InputKind::Number if !is_valid_float(value) || parse_float(value).is_none() => {
                String::new()
            }
            InputKind::Range => self.sanitize_range(element, value),
            // A valid simple color, lowercased; else black.
            InputKind::Color => {
                let simple = value.len() == 7
                    && value.starts_with('#')
                    && value.bytes().skip(1).all(|byte| byte.is_ascii_hexdigit());
                if simple {
                    value.to_ascii_lowercase()
                } else {
                    "#000000".to_owned()
                }
            }
            _ => value.to_owned(),
        }
    }

    /// HTML's reset algorithm of an input (Blink's `HTMLInputElement::
    /// ResetImpl`): not dirty, its value and checkedness the attributes'
    /// (a radio button checked so unchecks its group).
    pub(crate) fn input_reset(&self, element: NodeRef<'gc>) {
        let Some(input) = self.input(element) else {
            return;
        };
        input.into_ref().value.get().set(None);
        self.state_changed(element, AttributeChanges::VALUE);
        self.shadow_tree_changed(element);
        input.dirty_checked.set(false);
        self.set_checkedness(
            element,
            self.attribute_value_by_id(element, names::local::CHECKED)
                .is_some(),
        );
    }

    /// Whether `element` has an activation behavior of its type's: a
    /// checkbox's and a radio button's; a submit, image or reset button's
    /// with a form owner (Blink's: the other inputs' leave a click to an
    /// ancestor's, a link's).
    pub fn input_has_activation_behavior(&self, element: NodeRef<'gc>) -> bool {
        match self.input_kind(element) {
            Some(InputKind::Checkbox | InputKind::Radio) => true,
            Some(InputKind::Submit | InputKind::Image | InputKind::Reset) => {
                self.form_owner(element).is_some()
            }
            _ => false,
        }
    }

    /// Whether `element` is a `<button>` that submits or resets its form
    /// owner, which its activation behavior does (its `type` a submit
    /// button's, `reset`'s; not `button`'s; Blink's
    /// `HTMLFormControlElement` with no form has none).
    pub fn button_has_activation_behavior(&self, element: NodeRef<'gc>) -> bool {
        self.qualified_name(element) == Some(crate::QualifiedName::html(names::local::BUTTON))
            && self.form_owner(element).is_some()
            && self.button_type(element) != "button"
    }

    /// A `<button>`'s type (`type`'s keyword: `submit`, `reset` or
    /// `button`; a submit button's for none or another value).
    pub fn button_type(&self, element: NodeRef<'gc>) -> &'static str {
        let Some(value) = self.attribute_value_by_id(element, names::local::TYPE) else {
            return "submit";
        };
        let value = value.to_owned();
        let value = value.to_str();
        ["reset", "button"]
            .into_iter()
            .find(|keyword| keyword.eq_ignore_ascii_case(&value))
            .unwrap_or("submit")
    }

    /// Whether `element`'s activation behavior resets its form owner: a
    /// reset button's, an `<input>`'s or a `<button>`'s.
    pub fn resets_form(&self, element: NodeRef<'gc>) -> bool {
        match self.input_kind(element) {
            Some(kind) => kind == InputKind::Reset,
            None => {
                self.qualified_name(element)
                    == Some(crate::QualifiedName::html(names::local::BUTTON))
                    && self.button_type(element) == "reset"
            }
        }
    }

    /// HTML's legacy-pre-activation behavior of an input (Blink's
    /// `WillDispatchClick`): a checkbox's checkedness flips and its
    /// indeterminate goes; a radio button is checked, unchecking its group.
    /// Dirty, as `checked` makes it. What was, for after.
    pub fn input_will_click(&self, element: NodeRef<'gc>) -> Option<ClickState<'gc>> {
        let input = self.input(element)?;
        let state = ClickState {
            checked: input.checked.get(),
            indeterminate: input.indeterminate.get(),
            checked_radio: None,
        };
        match InputKind::from_raw(input.kind.get()) {
            InputKind::Checkbox => {
                self.set_indeterminate_bit(element, input, false);
                self.set_checked(element, !state.checked);
                Some(state)
            }
            InputKind::Radio => {
                let mut checked_radio = None;
                self.for_each_in_radio_group(element, |other| {
                    if self.checkedness(other) {
                        checked_radio = Some(other);
                        return false;
                    }
                    true
                });
                self.set_checked(element, true);
                Some(ClickState {
                    checked_radio,
                    ..state
                })
            }
            _ => None,
        }
    }

    /// HTML's legacy-canceled-activation behavior of an input (Blink's
    /// `DidDispatchClick` of a canceled click): a checkbox's checkedness
    /// and indeterminate as they were; a radio button unchecked, and the
    /// one of its group that was checked checked again, if it still is in
    /// the group. By the kind it was clicked as (`was`): the element may be
    /// another now.
    pub fn input_click_canceled(
        &self,
        element: NodeRef<'gc>,
        state: ClickState<'gc>,
        was: InputKind,
    ) {
        let Some(input) = self.input(element) else {
            return;
        };
        match was {
            InputKind::Checkbox => {
                self.set_checked_bit(element, input, state.checked);
                self.set_indeterminate_bit(element, input, state.indeterminate);
            }
            InputKind::Radio if !state.checked => {
                self.set_checked_bit(element, input, false);
                let Some(previous) = state.checked_radio else {
                    return;
                };
                // A listener may have made it another type: the one
                // checked before is checked again if it is still a radio
                // button of the name and form owner this had (Blink's
                // `RadioInputType::DidDispatchClick`).
                let regroup = if self.is_radio(element) {
                    self.in_radio_group_of(element, previous)
                } else {
                    let name = |node| {
                        self.attribute_value_by_id(node, names::NAME)
                            .map(|name| name.to_owned())
                    };
                    self.is_radio(previous)
                        && match (self.form_owner(previous), self.form_owner(element)) {
                            (Some(a), Some(b)) => Gc::ptr_eq(a, b),
                            (a, b) => a.is_none() && b.is_none(),
                        }
                        && name(previous) == name(element)
                };
                if regroup {
                    self.set_checkedness(previous, true);
                }
            }
            _ => {}
        }
    }

    /// Whether `a` and `b` are radio buttons of one group: the same form
    /// owner (or none), the same `name`, not empty, in one tree.
    fn in_radio_group_of(&self, a: NodeRef<'gc>, b: NodeRef<'gc>) -> bool {
        let mut found = false;
        self.for_each_in_radio_group(a, |other| {
            found = Gc::ptr_eq(other, b);
            !found
        });
        found
    }

    /// The other radio buttons of `element`'s group, in tree order, each
    /// given to `f` until it says stop; none if it is no radio button or
    /// has no name.
    fn for_each_in_radio_group(
        &self,
        element: NodeRef<'gc>,
        mut f: impl FnMut(NodeRef<'gc>) -> bool,
    ) {
        if !self.is_radio(element) {
            return;
        }
        let Some(name) = self
            .attribute_value_by_id(element, names::NAME)
            .map(|name| name.to_owned())
        else {
            return;
        };
        if name.to_str().is_empty() {
            return;
        }
        let owner = self.form_owner(element);
        let mut in_group = |other: NodeRef<'gc>| {
            if Gc::ptr_eq(other, element)
                || !self.is_radio(other)
                || self
                    .attribute_value_by_id(other, names::NAME)
                    .is_none_or(|other| *other.borrow() != name)
            {
                return true;
            }
            f(other)
        };
        // An owner's elements are in its tree: they are the group's.
        if let Some(form) = owner {
            let elements = self.form_listed_elements(form);
            for index in 0..elements.len() {
                if let Some(other) = elements.get(self.gc, index)
                    && !in_group(other)
                {
                    return;
                }
            }
            return;
        }
        let root = self.root_node(element, false);
        let mut at = Some(root);
        while let Some(node) = at {
            if self.form_owner(node).is_none() && !in_group(node) {
                return;
            }
            at = self.following_within(node, root);
        }
    }

    /// HTML's rule of a checked radio button: the others of its group are
    /// unchecked (their dirty flags as they were). Nothing for another
    /// element, or one unchecked.
    pub(crate) fn uncheck_radio_group(&self, element: NodeRef<'gc>) {
        if !self.is_radio(element) || !self.checkedness(element) {
            return;
        }
        let mut others = Vec::new();
        self.for_each_in_radio_group(element, |other| {
            if self.checkedness(other) {
                others.push(other);
            }
            true
        });
        for other in others {
            if let Some(input) = self.input(other) {
                self.set_checked_bit(other, input, false);
            }
        }
    }
}
