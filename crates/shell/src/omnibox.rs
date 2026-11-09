//! The address bar's text field (`OmniboxViewViews`, a Views `Textfield`).
//! Editing is parley's `PlainEditor`: the caret, selection, deleting by
//! character or word, and IME composition. Its text is laid out with
//! parley too, apart from Blink's, as Views uses `gfx::RenderText`.
//!
//! Transitional, with the rest of the native toolbar (see `toolbar`).

use base::numerics::f32_from_f64;
use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{Color, PointF, RectF};
use parley::{Affinity, Cursor, FontContext, LayoutContext, PlainEditor, Selection};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use crate::ui_text;

pub const FONT_SIZE: f32 = 14.0;
const TEXT: Color = Color::rgb(32, 33, 36);
const SELECTION: Color = Color::rgb(198, 218, 252);

/// The system clipboard (`ui::Clipboard`), plain text only.
pub trait Clipboard {
    fn read(&mut self) -> Option<String>;
    fn write(&mut self, text: &str);
}

/// What a key did in the field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyResult {
    /// The field used it.
    Handled,
    /// Not for the field (Tab, shortcuts it doesn't know).
    Ignored,
    /// Enter: navigate to what was typed, as typed (the caller fixes it
    /// up into a URL).
    Navigate(String),
}

pub struct Omnibox {
    editor: PlainEditor<()>,
    fonts: FontContext,
    layouts: LayoutContext<()>,
    focused: bool,
    /// The URL shown while not editing, and what Escape goes back to.
    url: String,
    /// How far the text is scrolled left to keep the caret in view.
    scroll_x: f32,
    /// The primary button is down in the field: moving selects.
    selecting: bool,
}

impl Omnibox {
    /// `test_fonts`: Lato for every family, as the renderer's test fonts,
    /// so tests measure the same everywhere. Otherwise the system's fonts.
    pub fn new(test_fonts: bool) -> Self {
        let mut editor = PlainEditor::new(FONT_SIZE);
        editor.edit_styles().insert(ui_text::font_family());
        Self {
            editor,
            fonts: ui_text::font_context(test_fonts),
            layouts: LayoutContext::new(),
            focused: false,
            url: String::new(),
            scroll_x: 0.0,
            selecting: false,
        }
    }

    pub fn text(&self) -> String {
        self.editor.text().to_string()
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.editor.selected_text()
    }

    /// The page's URL. Shown unless the user is editing.
    pub fn set_url(&mut self, url: &str) {
        if self.url == url {
            return;
        }
        self.url = url.to_owned();
        if !self.focused {
            self.show_url();
        }
    }

    fn show_url(&mut self) {
        self.editor.set_text(&self.url);
        self.driver().move_to_text_start();
        self.scroll_x = 0.0;
    }

    /// Focus, with everything selected (Cmd+L, or a click while
    /// unfocused), so typing replaces the URL.
    pub fn focus(&mut self) {
        self.focused = true;
        self.driver().select_all();
    }

    /// Focus goes elsewhere (the page): the URL comes back.
    pub fn blur(&mut self) {
        self.focused = false;
        self.selecting = false;
        self.driver().clear_compose();
        self.show_url();
    }

    /// The primary button went down at `point`, in the text's coordinates
    /// (see [`Omnibox::paint`]). The first click focuses and selects all,
    /// as in Chrome; later ones place the caret and start selecting.
    pub fn mouse_down(&mut self, point: PointF, shift: bool) {
        if !self.focused {
            self.focus();
            return;
        }
        let x = point.x + self.scroll_x;
        if shift {
            self.driver().shift_click_extension(x, point.y);
        } else {
            self.driver().move_to_point(x, point.y);
        }
        self.selecting = true;
    }

    pub fn mouse_move(&mut self, point: PointF) {
        if self.selecting {
            let x = point.x + self.scroll_x;
            self.driver().extend_selection_to_point(x, point.y);
        }
    }

    pub fn mouse_up(&mut self) {
        self.selecting = false;
    }

    /// A key while focused (listed for users in the crate's README.md,
    /// "鍵盤和滑鼠"; keep it in step). `text` is what the key types, if
    /// anything; `mac` picks the platform's editing shortcuts.
    pub fn key(
        &mut self,
        key: &Key,
        text: Option<&str>,
        modifiers: ModifiersState,
        mac: bool,
        clipboard: &mut dyn Clipboard,
    ) -> KeyResult {
        let shift = modifiers.shift_key();
        // Cmd on macOS, Ctrl elsewhere: select all, copy, cut, paste.
        let primary = if mac {
            modifiers.super_key()
        } else {
            modifiers.control_key()
        };
        // A word at a time: Option on macOS, Ctrl elsewhere.
        let word = if mac {
            modifiers.alt_key()
        } else {
            modifiers.control_key()
        };
        // To the line's end: Cmd+arrow on macOS.
        let line = mac && modifiers.super_key();
        let mut driver = self.editor.driver(&mut self.fonts, &mut self.layouts);
        match key {
            Key::Named(NamedKey::Enter) => {
                return KeyResult::Navigate(self.editor.text().to_string());
            }
            Key::Named(NamedKey::Escape) => {
                self.editor.set_text(&self.url);
                self.driver().select_all();
            }
            Key::Named(NamedKey::Backspace) => match () {
                _ if line => {
                    driver.select_to_line_start();
                    driver.delete_selection();
                }
                _ if word => driver.backdelete_word(),
                _ => driver.backdelete(),
            },
            Key::Named(NamedKey::Delete) => match () {
                _ if line => {
                    driver.select_to_line_end();
                    driver.delete_selection();
                }
                _ if word => driver.delete_word(),
                _ => driver.delete(),
            },
            Key::Named(NamedKey::ArrowLeft) => match (shift, line, word) {
                (true, true, _) => driver.select_to_line_start(),
                (true, _, true) => driver.select_word_left(),
                (true, _, _) => driver.select_left(),
                (false, true, _) => driver.move_to_line_start(),
                (false, _, true) => driver.move_word_left(),
                (false, _, _) => driver.move_left(),
            },
            Key::Named(NamedKey::ArrowRight) => match (shift, line, word) {
                (true, true, _) => driver.select_to_line_end(),
                (true, _, true) => driver.select_word_right(),
                (true, _, _) => driver.select_right(),
                (false, true, _) => driver.move_to_line_end(),
                (false, _, true) => driver.move_word_right(),
                (false, _, _) => driver.move_right(),
            },
            Key::Named(NamedKey::Home) if shift => driver.select_to_line_start(),
            Key::Named(NamedKey::Home) => driver.move_to_line_start(),
            Key::Named(NamedKey::End) if shift => driver.select_to_line_end(),
            Key::Named(NamedKey::End) => driver.move_to_line_end(),
            // Chrome's omnibox moves through suggestions; there are none.
            Key::Named(NamedKey::ArrowUp | NamedKey::ArrowDown) => {}
            Key::Character(c) if primary => match c.to_lowercase().as_str() {
                "a" => driver.select_all(),
                "c" => {
                    if let Some(selected) = self.editor.selected_text() {
                        clipboard.write(selected);
                    }
                }
                "x" => {
                    if let Some(selected) = self.editor.selected_text() {
                        clipboard.write(selected);
                        self.driver().delete_selection();
                    }
                }
                "v" => {
                    if let Some(pasted) = clipboard.read() {
                        // One line, as Chrome's omnibox makes it.
                        let line: String = pasted
                            .chars()
                            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                            .collect();
                        self.driver().insert_or_replace_selection(line.trim());
                    }
                }
                _ => return KeyResult::Ignored,
            },
            _ => match text {
                // Ctrl or Cmd with a key types nothing.
                Some(text)
                    if !modifiers.control_key()
                        && !modifiers.super_key()
                        && !text.chars().any(char::is_control) =>
                {
                    driver.insert_or_replace_selection(text)
                }
                _ => return KeyResult::Ignored,
            },
        }
        KeyResult::Handled
    }

    /// `Ime::Preedit`: text being composed, shown underlined in place;
    /// empty when composing stops.
    pub fn preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if text.is_empty() {
            self.driver().clear_compose();
        } else {
            self.driver().set_compose(text, cursor);
        }
    }

    /// `Ime::Commit`: text the input method finished.
    pub fn commit(&mut self, text: &str) {
        let mut driver = self.driver();
        driver.clear_compose();
        driver.insert_or_replace_selection(text);
    }

    fn driver(&mut self) -> parley::PlainEditorDriver<'_, ()> {
        self.editor.driver(&mut self.fonts, &mut self.layouts)
    }

    /// Where the IME's candidate window should avoid, in the text's
    /// coordinates.
    pub fn ime_cursor_area(&mut self) -> RectF {
        self.editor
            .refresh_layout(&mut self.fonts, &mut self.layouts);
        let area = self.editor.ime_cursor_area();
        RectF::new(
            f32_from_f64(area.x0) - self.scroll_x,
            f32_from_f64(area.y0),
            f32_from_f64(area.x1 - area.x0),
            f32_from_f64(area.y1 - area.y0),
        )
    }

    /// Paints the field's text into `area` (its top left is the text's
    /// origin; the text is centred vertically and clipped to it): the
    /// selection, the glyphs, the IME's underline and the caret.
    pub fn paint(&mut self, list: &mut DisplayItemList, area: RectF) {
        self.editor
            .refresh_layout(&mut self.fonts, &mut self.layouts);
        let layout = self.editor.try_layout().expect("just laid out");
        let top = area.y + ((area.height - layout.height()) / 2.0).round();
        // Keep the caret in view.
        if self.focused {
            if let Some(caret) = self.editor.cursor_geometry(1.0) {
                let x = f32_from_f64(caret.x0);
                if x - self.scroll_x > area.width - 1.0 {
                    self.scroll_x = x - area.width + 1.0;
                } else if x < self.scroll_x {
                    self.scroll_x = x;
                }
            }
        } else {
            self.scroll_x = 0.0;
        }
        let origin = PointF::new(area.x - self.scroll_x, top);
        let at = |x0: f64, y0: f64, x1: f64, y1: f64| {
            RectF::new(
                origin.x + f32_from_f64(x0),
                origin.y + f32_from_f64(y0),
                f32_from_f64(x1 - x0),
                f32_from_f64(y1 - y0),
            )
        };
        list.push_op(PaintOp::Save);
        list.push_op(PaintOp::ClipRect {
            rect: area,
            anti_alias: false,
        });
        if self.focused {
            for (rect, _) in self.editor.selection_geometry() {
                let rect = at(rect.x0, rect.y0, rect.x1, rect.y1);
                list.push(
                    PaintOp::DrawRect {
                        rect,
                        flags: PaintFlags::fill(SELECTION),
                    },
                    Some(rect),
                );
            }
        }
        ui_text::paint_glyphs(list, layout, origin, TEXT);
        // Under the text being composed, a line.
        if let Some(compose) = self.editor.raw_compose().clone() {
            let composed = Selection::new(
                Cursor::from_byte_index(layout, compose.start, Affinity::Downstream),
                Cursor::from_byte_index(layout, compose.end, Affinity::Upstream),
            );
            for (area, _) in composed.geometry(layout) {
                let rect = at(area.x0, area.y1 - 1.0, area.x1, area.y1);
                list.push(
                    PaintOp::DrawRect {
                        rect,
                        flags: PaintFlags::fill(TEXT),
                    },
                    Some(rect),
                );
            }
        }
        if self.focused
            && let Some(caret) = self.editor.cursor_geometry(1.0)
        {
            let rect = at(caret.x0, caret.y0, caret.x1, caret.y1);
            list.push(
                PaintOp::DrawRect {
                    rect,
                    flags: PaintFlags::fill(TEXT),
                },
                Some(rect),
            );
        }
        list.push_op(PaintOp::Restore);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeClipboard(Option<String>);

    impl Clipboard for FakeClipboard {
        fn read(&mut self) -> Option<String> {
            self.0.clone()
        }
        fn write(&mut self, text: &str) {
            self.0 = Some(text.to_owned());
        }
    }

    const NONE: ModifiersState = ModifiersState::empty();

    struct Field {
        omnibox: Omnibox,
        clipboard: FakeClipboard,
        mac: bool,
    }

    impl Field {
        fn new(url: &str, mac: bool) -> Self {
            let mut omnibox = Omnibox::new(true);
            omnibox.set_url(url);
            Self {
                omnibox,
                clipboard: FakeClipboard::default(),
                mac,
            }
        }

        fn named(&mut self, key: NamedKey, modifiers: ModifiersState) -> KeyResult {
            self.omnibox.key(
                &Key::Named(key),
                None,
                modifiers,
                self.mac,
                &mut self.clipboard,
            )
        }

        fn shortcut(&mut self, c: &str, modifiers: ModifiersState) -> KeyResult {
            self.omnibox.key(
                &Key::Character(c.into()),
                Some(c),
                modifiers,
                self.mac,
                &mut self.clipboard,
            )
        }

        fn type_text(&mut self, text: &str) {
            for c in text.chars() {
                let s = c.to_string();
                let result = self.omnibox.key(
                    &Key::Character(s.as_str().into()),
                    Some(&s),
                    NONE,
                    self.mac,
                    &mut self.clipboard,
                );
                assert_eq!(result, KeyResult::Handled);
            }
        }

        fn text(&self) -> String {
            self.omnibox.text()
        }
    }

    #[test]
    fn focus_selects_the_url_and_typing_replaces_it() {
        let mut field = Field::new("https://a.test/", true);
        assert_eq!(field.text(), "https://a.test/");
        field.omnibox.focus();
        assert_eq!(field.omnibox.selected_text(), Some("https://a.test/"));
        field.type_text("b.test");
        assert_eq!(
            field.named(NamedKey::Enter, NONE),
            KeyResult::Navigate("b.test".into())
        );
        // A new URL while editing doesn't overwrite the edit…
        field.omnibox.set_url("https://c.test/");
        assert_eq!(field.text(), "b.test");
        // …Escape goes back to it, and losing focus shows it.
        field.named(NamedKey::Escape, NONE);
        assert_eq!(field.text(), "https://c.test/");
        field.type_text("x");
        field.omnibox.blur();
        assert_eq!(field.text(), "https://c.test/");
        assert!(!field.omnibox.is_focused());
    }

    #[test]
    fn caret_movement_and_deleting_by_platform() {
        // macOS: Option is a word, Cmd the whole line.
        let mut field = Field::new("", true);
        field.omnibox.focus();
        field.type_text("one two three");
        field.named(NamedKey::Backspace, ModifiersState::ALT);
        assert_eq!(field.text(), "one two ");
        field.named(NamedKey::ArrowLeft, ModifiersState::SUPER);
        field.named(NamedKey::Delete, NONE);
        assert_eq!(field.text(), "ne two ");
        field.named(NamedKey::ArrowRight, ModifiersState::SUPER);
        field.named(NamedKey::Backspace, ModifiersState::SUPER);
        assert_eq!(field.text(), "");
        // Elsewhere: Ctrl is a word.
        let mut field = Field::new("", false);
        field.omnibox.focus();
        field.type_text("one two");
        field.named(NamedKey::Backspace, ModifiersState::CONTROL);
        assert_eq!(field.text(), "one ");
        field.named(NamedKey::Home, NONE);
        field.named(NamedKey::End, ModifiersState::SHIFT);
        assert_eq!(field.omnibox.selected_text(), Some("one "));
    }

    #[test]
    fn copy_cut_and_paste() {
        let mut field = Field::new("https://a.test/path", true);
        field.omnibox.focus();
        let cmd = ModifiersState::SUPER;
        field.shortcut("c", cmd);
        assert_eq!(field.clipboard.0.as_deref(), Some("https://a.test/path"));
        // Select "path" and cut it.
        field.named(NamedKey::End, NONE);
        for _ in 0..4 {
            field.named(NamedKey::ArrowLeft, ModifiersState::SHIFT);
        }
        field.shortcut("x", cmd);
        assert_eq!(field.clipboard.0.as_deref(), Some("path"));
        assert_eq!(field.text(), "https://a.test/");
        // Pasting makes one line.
        field.clipboard.0 = Some("p\nq\r\n".into());
        field.shortcut("v", cmd);
        assert_eq!(field.text(), "https://a.test/p q");
        // Ctrl does nothing on macOS, and types nothing.
        assert_eq!(
            field.shortcut("a", ModifiersState::CONTROL),
            KeyResult::Ignored
        );
        assert_eq!(field.text(), "https://a.test/p q");
        // Elsewhere it is Ctrl.
        let mut field = Field::new("x", false);
        field.omnibox.focus();
        field.shortcut("c", ModifiersState::CONTROL);
        assert_eq!(field.clipboard.0.as_deref(), Some("x"));
    }

    #[test]
    fn ime_composition_then_commit() {
        let mut field = Field::new("", true);
        field.omnibox.focus();
        field.type_text("a");
        field.omnibox.preedit("ㄗˋ", Some((0, 0)));
        assert!(field.omnibox.editor.is_composing());
        field.omnibox.preedit("", None);
        field.omnibox.commit("字");
        assert_eq!(field.text(), "a字");
        assert!(!field.omnibox.editor.is_composing());
    }

    #[test]
    fn clicks_focus_then_place_the_caret() {
        let mut field = Field::new("abcdef", true);
        // The first click selects all.
        field.omnibox.mouse_down(PointF::new(1000.0, 5.0), false);
        assert!(field.omnibox.is_focused());
        assert_eq!(field.omnibox.selected_text(), Some("abcdef"));
        field.omnibox.mouse_up();
        // Then a click at the start puts the caret there; dragging to the
        // end selects all of it.
        field.omnibox.mouse_down(PointF::new(0.0, 5.0), false);
        assert_eq!(field.omnibox.selected_text(), None);
        field.omnibox.mouse_move(PointF::new(1000.0, 5.0));
        field.omnibox.mouse_up();
        assert_eq!(field.omnibox.selected_text(), Some("abcdef"));
        // Moving without the button changes nothing.
        field.omnibox.mouse_move(PointF::new(0.0, 5.0));
        assert_eq!(field.omnibox.selected_text(), Some("abcdef"));
    }

    #[test]
    fn paint_clips_text_and_keeps_the_caret_in_view() {
        let mut field = Field::new("", true);
        field.omnibox.focus();
        field.type_text(&"w".repeat(100));
        let mut list = DisplayItemList::new();
        let area = RectF::new(10.0, 5.0, 100.0, 20.0);
        field.omnibox.paint(&mut list, area);
        // Scrolled so the caret, at the end, is at the area's right edge.
        let caret = list
            .ops()
            .iter()
            .rev()
            .find_map(|op| match op {
                PaintOp::DrawRect { rect, .. } => Some(*rect),
                _ => None,
            })
            .unwrap();
        assert!(caret.x > 100.0 && caret.right() <= 110.0, "{caret:?}");
        assert!(matches!(list.ops()[1], PaintOp::ClipRect { rect, .. } if rect == area));
        assert!(
            list.ops()
                .iter()
                .any(|op| matches!(op, PaintOp::DrawGlyphs { .. }))
        );
    }
}
