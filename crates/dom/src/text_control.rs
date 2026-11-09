//! HTML's text controls (https://html.spec.whatwg.org/#textFieldSelection):
//! a `<textarea>`'s value, and the selection of a textarea and of the
//! inputs whose type has one (Blink's `TextControlElement` and
//! `HTMLTextAreaElement`). A textarea's raw value is its child text
//! content until it is dirty: then its own, kept in its state (Blink's
//! `value_` and `is_dirty_`); its API value has its newlines normalized.
//! The selection is state only (nothing draws a text control's text yet),
//! in UTF-16 code units as script's, and read within the value as it is
//! then (Blink clamps it as the value changes; the same but for a value
//! that shrinks, then grows back before the selection is read).
//!
//! A textarea's children changing (`TEXTAREA_CHILDREN` in its `scope`)
//! changes its value while it isn't dirty: `:placeholder-shown` noted.

use std::cell::Cell;

use elise::{Gc, GcObject, LocalExternal};

use crate::form::TEXTAREA;
use crate::{AttributeChanges, Cx, DomString, InputKind, NodeKind, NodeRef, sites};

/// `Node::scope`'s: the node is a textarea, whose children's changes may
/// change its value.
pub(crate) const TEXTAREA_CHILDREN: u8 = 1 << 7;

/// A text control's selection direction (HTML's; Blink's
/// `TextFieldSelectionDirection`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionDirection {
    #[default]
    None,
    Forward,
    Backward,
}

impl SelectionDirection {
    /// `forward` or `backward`; anything else is none.
    pub fn from_keyword(keyword: &str) -> Self {
        match keyword {
            "forward" => Self::Forward,
            "backward" => Self::Backward,
            _ => Self::None,
        }
    }

    pub fn keyword(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Forward => "forward",
            Self::Backward => "backward",
        }
    }

    fn from_raw(raw: u8) -> Self {
        match raw {
            1 => Self::Forward,
            2 => Self::Backward,
            _ => Self::None,
        }
    }

    fn raw(self) -> u8 {
        match self {
            Self::None => 0,
            Self::Forward => 1,
            Self::Backward => 2,
        }
    }
}

/// A text control's selection: from `start` to `end`, UTF-16 code units
/// into its value (`start <= end`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextSelection {
    pub start: u32,
    pub end: u32,
    pub direction: SelectionDirection,
}

/// `setRangeText()`'s `selectionMode` (HTML's `SelectionMode`): where the
/// selection is after the text is replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Select,
    Start,
    End,
    Preserve,
}

impl SelectionMode {
    /// The WebIDL enum's value of `keyword`; none for another.
    pub fn from_keyword(keyword: &str) -> Option<Self> {
        match keyword {
            "select" => Some(Self::Select),
            "start" => Some(Self::Start),
            "end" => Some(Self::End),
            "preserve" => Some(Self::Preserve),
            _ => None,
        }
    }
}

/// A textarea's state.
#[derive(GcObject)]
pub struct TextArea {
    /// Its raw value once dirty (HTML's dirty value flag set), as it was
    /// set; none while it is its child text content. In a `Cell`, taken and
    /// put back.
    value: LocalExternal<Cell<Option<DomString>>>,
    pub(crate) selection_start: Cell<u32>,
    pub(crate) selection_end: Cell<u32>,
    pub(crate) selection_direction: Cell<u8>,
    /// Whether `selectionchange` is scheduled at it (`selection`).
    pub(crate) selectionchange_scheduled: Cell<bool>,
    /// Its API value before the user's first change since the last commit
    /// (`note_edit`): what a commit compares with.
    before_edit: LocalExternal<Cell<Option<DomString>>>,
}

/// Where a text control's selection is kept: its textarea's state, or its
/// input's type's object.
struct SelectionCells<'a> {
    start: &'a Cell<u32>,
    end: &'a Cell<u32>,
    direction: &'a Cell<u8>,
    selectionchange_scheduled: &'a Cell<bool>,
}

/// HTML's newline normalization: CR LF and CR become LF.
fn normalize_newlines(value: &str) -> String {
    value.replace("\r\n", "\n").replace('\r', "\n")
}

/// The UTF-16 length of `value`, as a selection's offsets are.
fn utf16_len(value: &DomString) -> u32 {
    u32::try_from(value.utf16_len()).unwrap_or(u32::MAX)
}

impl<'gc> Cx<'_, 'gc> {
    /// A textarea was made (`form_element_made`): its state, empty.
    pub(crate) fn textarea_made(&self, element: NodeRef<'gc>) {
        let textarea = self.gc.alloc_in(
            &sites::OTHER,
            TextAreaInit {
                value: LocalExternal::new(Cell::new(None)),
                selection_start: Cell::new(0),
                selection_end: Cell::new(0),
                selection_direction: Cell::new(0),
                selectionchange_scheduled: Cell::new(false),
                before_edit: LocalExternal::new(Cell::new(None)),
            },
        );
        self.association_or_new(element)
            .textarea
            .set(self.gc, Some(textarea));
        element.scope.set(element.scope.get() | TEXTAREA_CHILDREN);
    }

    /// `element`'s state, if it is an HTML `<textarea>`.
    fn textarea(&self, element: NodeRef<'gc>) -> Option<Gc<'gc, TextArea>> {
        if !self.is_textarea(element) {
            return None;
        }
        self.association(element)?.textarea.get(self.gc)
    }

    /// Whether `element` is an HTML `<textarea>`.
    pub fn is_textarea(&self, element: NodeRef<'gc>) -> bool {
        self.form_kind(element) & TEXTAREA != 0
    }

    /// Whether the selection API (`selectionStart` and its kin,
    /// `setSelectionRange()`, `setRangeText()`) applies to `element`: a
    /// textarea, or an input of type text, search, url, tel or password.
    pub fn selection_applies(&self, element: NodeRef<'gc>) -> bool {
        self.is_textarea(element)
            || matches!(
                self.input_kind(element),
                Some(
                    InputKind::Text
                        | InputKind::Search
                        | InputKind::Url
                        | InputKind::Tel
                        | InputKind::Password
                )
            )
    }

    /// Whether `select()` applies: where the selection API does, and the
    /// input types whose text a user selects too (email, number, the date
    /// and time types, color, file), which have no selection of their own
    /// here.
    pub fn select_applies(&self, element: NodeRef<'gc>) -> bool {
        self.selection_applies(element)
            || matches!(
                self.input_kind(element),
                Some(
                    InputKind::Email
                        | InputKind::Number
                        | InputKind::Date
                        | InputKind::Month
                        | InputKind::Week
                        | InputKind::Time
                        | InputKind::DatetimeLocal
                        | InputKind::Color
                        | InputKind::File
                )
            )
    }

    /// The cells of `element`'s selection, if the selection API applies.
    fn selection_cells(&self, element: NodeRef<'gc>) -> Option<SelectionCells<'gc>> {
        if !self.selection_applies(element) {
            return None;
        }
        if let Some(textarea) = self.textarea(element) {
            let textarea = textarea.into_ref();
            return Some(SelectionCells {
                start: &textarea.selection_start,
                end: &textarea.selection_end,
                direction: &textarea.selection_direction,
                selectionchange_scheduled: &textarea.selectionchange_scheduled,
            });
        }
        let input = self.association(element)?.input.get(self.gc)?.into_ref();
        Some(SelectionCells {
            start: &input.selection_start,
            end: &input.selection_end,
            direction: &input.selection_direction,
            selectionchange_scheduled: &input.selectionchange_scheduled,
        })
    }

    /// The value a text control's selection is of (HTML's relevant value):
    /// an input's value, a textarea's API value.
    pub fn text_control_value(&self, element: NodeRef<'gc>) -> DomString {
        if self.is_textarea(element) {
            self.textarea_value(element)
        } else {
            self.input_value(element)
        }
    }

    /// `node`'s child text content (DOM's): its text children's data
    /// joined.
    pub fn child_text_content(&self, node: NodeRef<'gc>) -> DomString {
        let mut joined = String::new();
        let mut child = self.first_child(node);
        while let Some(at) = child {
            if self.kind(at) == NodeKind::Text
                && let Some(data) = self.text_data(at)
            {
                joined.push_str(&data.borrow().to_str());
            }
            child = self.next_sibling(at);
        }
        DomString::from(joined)
    }

    /// A textarea's raw value: its own once dirty, else its child text
    /// content.
    fn textarea_raw_value(&self, element: NodeRef<'gc>) -> DomString {
        let kept = self.textarea(element).and_then(|textarea| {
            let cell = textarea.into_ref().value.get();
            let kept = cell.take();
            cell.set(kept.clone());
            kept
        });
        kept.unwrap_or_else(|| self.child_text_content(element))
    }

    /// A textarea's `value`: its API value, its raw value with its newlines
    /// normalized.
    pub fn textarea_value(&self, element: NodeRef<'gc>) -> DomString {
        let raw = self.textarea_raw_value(element);
        if !raw.to_str().contains('\r') {
            return raw;
        }
        DomString::from(normalize_newlines(&raw.to_str()))
    }

    /// Setting a textarea's `value`: its raw value, dirty now; the cursor
    /// to the end if the API value changed.
    pub fn set_textarea_value(&self, element: NodeRef<'gc>, value: &str) {
        let Some(textarea) = self.textarea(element) else {
            return;
        };
        let old = self.textarea_value(element);
        textarea
            .into_ref()
            .value
            .get()
            .set(Some(DomString::new(value)));
        self.state_changed(element, AttributeChanges::VALUE);
        self.shadow_tree_changed(element);
        if *normalize_newlines(value) != *old.to_str() {
            self.cursor_to_end(element);
        }
    }

    /// HTML's reset algorithm of a textarea: not dirty, so its raw value is
    /// its child text content again. The selection stays (HTML has no step
    /// for it), within the value as it is read.
    pub(crate) fn textarea_reset(&self, element: NodeRef<'gc>) {
        if let Some(textarea) = self.textarea(element) {
            textarea.into_ref().value.get().set(None);
            self.state_changed(element, AttributeChanges::VALUE);
            self.shadow_tree_changed(element);
        }
    }

    /// HTML's cloning steps of a textarea: the copy has `original`'s raw
    /// value and dirty value flag.
    pub(crate) fn textarea_cloned(&self, original: NodeRef<'gc>, copy: NodeRef<'gc>) {
        let (Some(from), Some(to)) = (self.textarea(original), self.textarea(copy)) else {
            return;
        };
        let value = from.into_ref().value.get().take();
        from.into_ref().value.get().set(value.clone());
        to.into_ref().value.get().set(value);
    }

    /// A textarea's children changed (`children_changed`, or a text
    /// child's data): its value is them while it isn't dirty.
    pub(crate) fn textarea_children_changed(&self, element: NodeRef<'gc>) {
        let Some(textarea) = self.textarea(element) else {
            return;
        };
        let cell = textarea.into_ref().value.get();
        let kept = cell.take();
        let dirty = kept.is_some();
        cell.set(kept);
        if !dirty {
            self.state_changed(element, AttributeChanges::VALUE);
            self.shadow_tree_changed(element);
        }
    }

    /// Whether `selectionchange` is scheduled at text control `element`;
    /// none where the selection API doesn't apply.
    pub(crate) fn text_control_selectionchange_cell(
        &self,
        element: NodeRef<'gc>,
    ) -> Option<&'gc Cell<bool>> {
        Some(self.selection_cells(element)?.selectionchange_scheduled)
    }

    /// A text control's selection, within its value as it is now; none
    /// where the selection API doesn't apply.
    pub fn text_selection(&self, element: NodeRef<'gc>) -> Option<TextSelection> {
        let cells = self.selection_cells(element)?;
        let length = utf16_len(&self.text_control_value(element));
        Some(TextSelection {
            start: cells.start.get().min(length),
            end: cells.end.get().min(length),
            direction: SelectionDirection::from_raw(cells.direction.get()),
        })
    }

    /// HTML's "set the selection range": `start` and `end` within the
    /// value, an empty selection at `end` if `start` is after it. Whether
    /// it changed (the caller queues `select`); nothing where the selection
    /// API doesn't apply.
    pub fn set_selection_range(
        &self,
        element: NodeRef<'gc>,
        start: u32,
        end: u32,
        direction: SelectionDirection,
    ) -> bool {
        let Some(old) = self.text_selection(element) else {
            return false;
        };
        let cells = self
            .selection_cells(element)
            .expect("the selection API applies");
        let length = utf16_len(&self.text_control_value(element));
        let end = end.min(length);
        let new = TextSelection {
            start: start.min(end),
            end,
            direction,
        };
        cells.start.set(new.start);
        cells.end.set(new.end);
        cells.direction.set(new.direction.raw());
        // What the frame draws of it (the caret, the selected text), and
        // `selectionchange` at it (HTML's, Chrome's).
        if new != old {
            self.tree_changed();
            self.schedule_selectionchange(element);
        }
        new != old
    }

    /// The text entry cursor at the end of the value, nothing selected,
    /// the direction none: where a value set by script that changed it
    /// leaves it.
    pub(crate) fn cursor_to_end(&self, element: NodeRef<'gc>) {
        let length = utf16_len(&self.text_control_value(element));
        self.move_cursor(element, length);
    }

    /// The text entry cursor at `at`, the direction none; `selectionchange`
    /// at it if that moved it (Chrome's `SetSelectionRange` of a value
    /// set).
    fn move_cursor(&self, element: NodeRef<'gc>, at: u32) {
        if let Some(cells) = self.selection_cells(element) {
            let moved = (cells.start.get(), cells.end.get()) != (at, at);
            cells.start.set(at);
            cells.end.set(at);
            cells.direction.set(SelectionDirection::None.raw());
            if moved {
                self.schedule_selectionchange(element);
            }
        }
    }

    /// The text entry cursor at the start, the direction none: an input
    /// whose new type has a selection and whose old one didn't (HTML's type
    /// change steps).
    pub(crate) fn cursor_to_start(&self, element: NodeRef<'gc>) {
        self.move_cursor(element, 0);
    }

    /// HTML's `setRangeText()` once its arguments are checked (the
    /// selection API applies, `start` isn't after `end`): `replacement` in
    /// place of the code units from `start` to `end` (the selection's if
    /// `range` is none), the value dirty now; the selection after it as
    /// `mode` says. Whether the selection changed (the caller queues
    /// `select`). A UTF-16 offset that splits a character leaves a
    /// replacement character (machopo's strings hold no lone surrogate).
    pub fn set_range_text(
        &self,
        element: NodeRef<'gc>,
        replacement: &str,
        range: Option<(u32, u32)>,
        mode: SelectionMode,
    ) -> bool {
        let Some(current) = self.text_selection(element) else {
            return false;
        };
        let value = self.text_control_value(element);
        let mut units: Vec<u16> = value.to_str().encode_utf16().collect();
        let length = u32::try_from(units.len()).unwrap_or(u32::MAX);
        let (start, end) = range.unwrap_or((current.start, current.end));
        let (start, end) = (start.min(length), end.min(length));
        let inserted: Vec<u16> = replacement.encode_utf16().collect();
        let inserted_length = u32::try_from(inserted.len()).unwrap_or(u32::MAX);
        units.splice(
            usize::try_from(start).unwrap_or(usize::MAX)
                ..usize::try_from(end).unwrap_or(usize::MAX),
            inserted,
        );
        let new_end = start.saturating_add(inserted_length);
        let (selection_start, selection_end) = match mode {
            SelectionMode::Select => (start, new_end),
            SelectionMode::Start => (start, start),
            SelectionMode::End => (new_end, new_end),
            SelectionMode::Preserve => {
                // A position after the replaced text moves with it, one
                // within it goes to its edge.
                let shift = |position: u32, within: u32| {
                    if position > end {
                        position
                            .saturating_sub(end.saturating_sub(start))
                            .saturating_add(inserted_length)
                    } else if position > start {
                        within
                    } else {
                        position
                    }
                };
                (shift(current.start, start), shift(current.end, new_end))
            }
        };
        let value = String::from_utf16_lossy(&units);
        if let Some(textarea) = self.textarea(element) {
            textarea
                .into_ref()
                .value
                .get()
                .set(Some(DomString::from(value)));
            self.state_changed(element, AttributeChanges::VALUE);
            self.shadow_tree_changed(element);
        } else {
            // A text input's value mode is the element's value.
            self.set_input_value_keeping_selection(element, &value);
        }
        self.set_selection_range(
            element,
            selection_start,
            selection_end,
            SelectionDirection::None,
        )
    }

    /// Where `element`'s value before the user's change is kept, if it is
    /// a text control.
    fn before_edit_cell(&self, element: NodeRef<'gc>) -> Option<&'gc Cell<Option<DomString>>> {
        if let Some(textarea) = self.textarea(element) {
            return Some(textarea.into_ref().before_edit.get());
        }
        let input = self.association(element)?.input.get(self.gc)?;
        Some(input.into_ref().before_edit.get())
    }

    /// The user replaced the code units `start..end` (UTF-16 offsets) of
    /// `element`'s value with `text` (an edit, its events the caller's):
    /// the value dirty, the cursor after the text, the value before the
    /// user's first change since the last commit kept (Blink's
    /// `TextFieldInputType::SubtreeHasChanged`, `SetValueBeforeFirstUserEdit`).
    pub fn replace_by_user(&self, element: NodeRef<'gc>, start: u32, end: u32, text: &str) {
        let value = self.text_control_value(element);
        let mut units: Vec<u16> = value.to_str().encode_utf16().collect();
        let length = u32::try_from(units.len()).unwrap_or(u32::MAX);
        let (start, end) = (start.min(length), end.min(length).max(start.min(length)));
        let inserted: Vec<u16> = text.encode_utf16().collect();
        let at = start.saturating_add(u32::try_from(inserted.len()).unwrap_or(u32::MAX));
        units.splice(
            usize::try_from(start).unwrap_or(usize::MAX)
                ..usize::try_from(end).unwrap_or(usize::MAX),
            inserted,
        );
        if let Some(cell) = self.before_edit_cell(element) {
            let kept = cell.take();
            cell.set(Some(kept.unwrap_or(value)));
        }
        let new = String::from_utf16_lossy(&units);
        if let Some(textarea) = self.textarea(element) {
            textarea
                .into_ref()
                .value
                .get()
                .set(Some(DomString::from(new)));
            self.state_changed(element, AttributeChanges::VALUE);
            self.shadow_tree_changed(element);
        } else {
            self.set_input_value_keeping_selection(element, &new);
        }
        self.set_selection_range(element, at, at, SelectionDirection::None);
    }

    /// The user changed `element`'s value from `before` (a step of a
    /// number's spin button or keys): `before` kept as the value before
    /// the user's first change since the last commit, unless one is.
    pub fn note_edit(&self, element: NodeRef<'gc>, before: DomString) {
        if let Some(cell) = self.before_edit_cell(element) {
            let kept = cell.take();
            cell.set(Some(kept.unwrap_or(before)));
        }
    }

    /// HTML's committing of the user's change of `element`'s value (as it
    /// loses the focus, or an input's Enter): whether the value differs
    /// from what it was before the user's first change since the last
    /// commit (the caller fires `change`). None to commit: false.
    pub fn commit_edit(&self, element: NodeRef<'gc>) -> bool {
        let Some(before) = self.before_edit_cell(element).and_then(Cell::take) else {
            return false;
        };
        *before.to_str() != *self.text_control_value(element).to_str()
    }

    /// No change of the user's left to commit (WebDriver's clear sets the
    /// value as script does, and fires `change` itself).
    pub fn forget_edit(&self, element: NodeRef<'gc>) {
        if let Some(cell) = self.before_edit_cell(element) {
            cell.set(None);
        }
    }
}
