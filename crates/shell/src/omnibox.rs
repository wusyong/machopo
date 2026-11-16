//! The address bar's text field (`OmniboxViewViews`, a Views `Textfield`),
//! a view of the toolbar. Editing is parley's `PlainEditor`: the caret,
//! selection, deleting by character or word, and IME composition. Its text
//! is laid out with parley too, apart from Blink's, as Views uses
//! `gfx::RenderText`, with the fonts the tree lends.

use base::numerics::f32_from_f64;
use compositor_paint::{DisplayItemList, PaintFlags, PaintOp};
use gfx::{PointF, RRectF, RectF, SizeF};
use parley::{Affinity, Cursor as TextCursor, PlainEditor, Selection};
use ui::taffy::{AvailableSpace, Size};
use ui::{
    Cursor, EventCtx, Handled, ImeEvent, Key, KeyEvent, MeasureCtx, Modifiers, NamedKey, PaintCtx,
    PointerButton, PointerEvent, TextContexts, View,
};

use crate::chrome::{BrowserUi, UiMessage};
use crate::theme::{BrowserTheme, ColorToken, SizeToken};

pub use ui::Clipboard;

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
    focused: bool,
    /// The URL shown while not editing, and what Escape goes back to.
    url: String,
    /// How far the text is scrolled left to keep the caret in view.
    scroll_x: f32,
    /// The primary button is down in the field: moving selects.
    selecting: bool,
    /// Editing keys as on macOS, or not.
    mac: bool,
}

impl Omnibox {
    /// Its text `font_size` CSS px.
    pub fn new(font_size: f32) -> Self {
        let mut editor = PlainEditor::new(font_size);
        editor.edit_styles().insert(ui::text::font_family());
        Self {
            editor,
            focused: false,
            url: String::new(),
            scroll_x: 0.0,
            selecting: false,
            mac: cfg!(target_os = "macos"),
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

    /// Editing keys as on macOS (`mac`) or as elsewhere.
    pub fn set_mac(&mut self, mac: bool) {
        self.mac = mac;
    }

    /// The page's URL. Shown unless the user is editing. Whether the field
    /// shows anything differently.
    pub fn set_url(&mut self, text: &mut TextContexts, url: &str) -> bool {
        if self.url == url {
            return false;
        }
        self.url = url.to_owned();
        if self.focused {
            return false;
        }
        self.show_url(text);
        true
    }

    fn show_url(&mut self, text: &mut TextContexts) {
        self.editor.set_text(&self.url);
        edit(&mut self.editor, text).move_to_text_start();
        self.scroll_x = 0.0;
    }

    /// Focus, with everything selected (Cmd+L, or a click while
    /// unfocused), so typing replaces the URL.
    pub fn focus(&mut self, text: &mut TextContexts) {
        self.focused = true;
        edit(&mut self.editor, text).select_all();
    }

    /// Focus goes elsewhere (the page): the URL comes back.
    pub fn blur(&mut self, text: &mut TextContexts) {
        self.focused = false;
        self.selecting = false;
        edit(&mut self.editor, text).clear_compose();
        self.show_url(text);
    }

    /// The primary button went down at `point`, in the text's coordinates
    /// (see [`Omnibox::paint_text`]), the field already focused: it places
    /// the caret and starts selecting.
    pub fn mouse_down(&mut self, text: &mut TextContexts, point: PointF, shift: bool) {
        let x = point.x + self.scroll_x;
        let mut driver = edit(&mut self.editor, text);
        if shift {
            driver.shift_click_extension(x, point.y);
        } else {
            driver.move_to_point(x, point.y);
        }
        self.selecting = true;
    }

    pub fn mouse_move(&mut self, text: &mut TextContexts, point: PointF) {
        if self.selecting {
            let x = point.x + self.scroll_x;
            edit(&mut self.editor, text).extend_selection_to_point(x, point.y);
        }
    }

    pub fn mouse_up(&mut self) {
        self.selecting = false;
    }

    /// A key while focused (listed for users in the crate's README.md,
    /// "鍵盤和滑鼠"; keep it in step). `typed` is what the key types, if
    /// anything.
    pub fn key(
        &mut self,
        text: &mut TextContexts,
        key: &Key,
        typed: Option<&str>,
        modifiers: Modifiers,
        clipboard: Option<&mut (dyn Clipboard + 'static)>,
    ) -> KeyResult {
        let mac = self.mac;
        let shift = modifiers.shift;
        // Cmd on macOS, Ctrl elsewhere: select all, copy, cut, paste.
        let primary = if mac { modifiers.meta } else { modifiers.ctrl };
        // A word at a time: Option on macOS, Ctrl elsewhere.
        let word = if mac { modifiers.alt } else { modifiers.ctrl };
        // To the line's end: Cmd+arrow on macOS.
        let line = mac && modifiers.meta;
        let mut driver = edit(&mut self.editor, text);
        match key {
            Key::Named(NamedKey::Enter) => {
                return KeyResult::Navigate(self.editor.text().to_string());
            }
            Key::Named(NamedKey::Escape) => {
                self.editor.set_text(&self.url);
                edit(&mut self.editor, text).select_all();
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
                    if let (Some(selected), Some(clipboard)) =
                        (self.editor.selected_text(), clipboard)
                    {
                        clipboard.write(selected);
                    }
                }
                "x" => {
                    if let (Some(selected), Some(clipboard)) =
                        (self.editor.selected_text(), clipboard)
                    {
                        clipboard.write(selected);
                        edit(&mut self.editor, text).delete_selection();
                    }
                }
                "v" => {
                    if let Some(pasted) = clipboard.and_then(|clipboard| clipboard.read()) {
                        // One line, as Chrome's omnibox makes it.
                        let line: String = pasted
                            .chars()
                            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
                            .collect();
                        edit(&mut self.editor, text).insert_or_replace_selection(line.trim());
                    }
                }
                _ => return KeyResult::Ignored,
            },
            _ => match typed {
                // Ctrl or Cmd with a key types nothing.
                Some(typed)
                    if !modifiers.ctrl
                        && !modifiers.meta
                        && !typed.chars().any(char::is_control) =>
                {
                    driver.insert_or_replace_selection(typed)
                }
                _ => return KeyResult::Ignored,
            },
        }
        KeyResult::Handled
    }

    /// `Ime::Preedit`: text being composed, shown underlined in place;
    /// empty when composing stops.
    pub fn preedit(
        &mut self,
        text: &mut TextContexts,
        composed: &str,
        cursor: Option<(usize, usize)>,
    ) {
        let mut driver = edit(&mut self.editor, text);
        if composed.is_empty() {
            driver.clear_compose();
        } else {
            driver.set_compose(composed, cursor);
        }
    }

    /// `Ime::Commit`: text the input method finished.
    pub fn commit(&mut self, text: &mut TextContexts, committed: &str) {
        let mut driver = edit(&mut self.editor, text);
        driver.clear_compose();
        driver.insert_or_replace_selection(committed);
    }

    /// Where the IME's candidate window should avoid, in the text's
    /// coordinates.
    pub fn ime_cursor_area(&mut self, text: &mut TextContexts) -> RectF {
        self.editor
            .refresh_layout(&mut text.fonts, &mut text.layouts);
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
    pub fn paint_text(
        &mut self,
        text: &mut TextContexts,
        theme: &BrowserTheme,
        list: &mut DisplayItemList,
        area: RectF,
    ) {
        let color = theme.color(ColorToken::FieldText);
        self.editor
            .refresh_layout(&mut text.fonts, &mut text.layouts);
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
                        flags: PaintFlags::fill(theme.color(ColorToken::FieldSelection)),
                    },
                    Some(rect),
                );
            }
        }
        ui::text::paint_glyphs(list, layout, origin, color);
        // Under the text being composed, a line.
        if let Some(compose) = self.editor.raw_compose().clone() {
            let composed = Selection::new(
                TextCursor::from_byte_index(layout, compose.start, Affinity::Downstream),
                TextCursor::from_byte_index(layout, compose.end, Affinity::Upstream),
            );
            for (area, _) in composed.geometry(layout) {
                let rect = at(area.x0, area.y1 - 1.0, area.x1, area.y1);
                list.push(
                    PaintOp::DrawRect {
                        rect,
                        flags: PaintFlags::fill(color),
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
                    flags: PaintFlags::fill(color),
                },
                Some(rect),
            );
        }
        list.push_op(PaintOp::Restore);
    }
}

fn edit<'a>(
    editor: &'a mut PlainEditor<()>,
    text: &'a mut TextContexts,
) -> parley::PlainEditorDriver<'a, ()> {
    editor.driver(&mut text.fonts, &mut text.layouts)
}

/// Where the field's text goes within the field: the field without its
/// padding.
fn text_area(theme: &BrowserTheme, size: SizeF) -> RectF {
    let padding = theme.size(SizeToken::FieldPadding);
    RectF::new(
        padding,
        0.0,
        (size.width - 2.0 * padding).max(0.0),
        size.height,
    )
}

impl View<BrowserUi> for Omnibox {
    fn measure(
        &mut self,
        ctx: &mut MeasureCtx<'_, BrowserUi>,
        _: Size<Option<f32>>,
        _: Size<AvailableSpace>,
    ) -> SizeF {
        // As wide as the toolbar leaves it (its style grows it).
        SizeF::new(0.0, ctx.theme().size(SizeToken::FieldHeight))
    }

    /// The field, rounded, with a focus ring while it has the keys, and
    /// its text.
    fn paint(&mut self, ctx: &mut PaintCtx<'_, BrowserUi>, list: &mut DisplayItemList) {
        let field = RectF::from_size(ctx.size());
        let theme = ctx.theme();
        let (ring_color, focused_color, field_color) = (
            theme.color(ColorToken::FocusRing),
            theme.color(ColorToken::FieldFocused),
            theme.color(ColorToken::Field),
        );
        let area = text_area(theme, field.size());
        if self.focused {
            let ring = field.inset(-1.0);
            list.push(
                PaintOp::DrawRRect {
                    rrect: RRectF::uniform(ring, ring.height / 2.0),
                    flags: PaintFlags::fill(ring_color),
                },
                Some(ring),
            );
        }
        list.push(
            PaintOp::DrawRRect {
                rrect: RRectF::uniform(field, field.height / 2.0),
                flags: PaintFlags::fill(if self.focused {
                    focused_color
                } else {
                    field_color
                }),
            },
            Some(field),
        );
        let (shared_text, theme) = ctx.text_and_theme();
        self.paint_text(shared_text, theme, list, area);
    }

    fn on_pointer(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &PointerEvent) -> Handled {
        let area = text_area(ctx.theme(), ctx.size());
        let p = event.position();
        let in_text = PointF::new(p.x - area.x, p.y - area.y);
        match *event {
            PointerEvent::Down {
                button: PointerButton::Primary,
                modifiers,
                ..
            } => {
                // The first click focuses and selects all, as in Chrome;
                // later ones place the caret and start selecting.
                if self.focused {
                    self.mouse_down(ctx.text(), in_text, modifiers.shift);
                    ctx.request_paint();
                } else {
                    ctx.request_focus();
                }
                Handled::Yes
            }
            PointerEvent::Move { .. } if self.selecting => {
                self.mouse_move(ctx.text(), in_text);
                ctx.request_paint();
                Handled::Yes
            }
            PointerEvent::Up {
                button: PointerButton::Primary,
                ..
            } => {
                self.mouse_up();
                Handled::Yes
            }
            _ => Handled::No,
        }
    }

    fn on_focus(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, focused: bool) {
        if focused {
            self.focus(ctx.text());
        } else {
            self.blur(ctx.text());
        }
        ctx.request_paint();
    }

    fn on_key(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &KeyEvent) -> Handled {
        if !event.pressed {
            return Handled::No;
        }
        let (text, clipboard) = ctx.text_and_clipboard();
        match self.key(
            text,
            &event.key,
            event.text.as_deref(),
            event.modifiers,
            clipboard,
        ) {
            KeyResult::Navigate(typed) => ctx.submit(UiMessage::Navigate(typed)),
            KeyResult::Handled => ctx.request_paint(),
            KeyResult::Ignored => return Handled::No,
        }
        Handled::Yes
    }

    fn on_ime(&mut self, ctx: &mut EventCtx<'_, BrowserUi>, event: &ImeEvent) -> Handled {
        match event {
            ImeEvent::Preedit { text, cursor } => self.preedit(ctx.text(), text, *cursor),
            ImeEvent::Commit(text) => self.commit(ctx.text(), text),
        }
        ctx.request_paint();
        Handled::Yes
    }

    fn cursor(&self, _: PointF) -> Cursor {
        Cursor::Text
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

    const NONE: Modifiers = Modifiers::NONE;
    const SHIFT: Modifiers = Modifiers {
        shift: true,
        ..Modifiers::NONE
    };
    const CTRL: Modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    const ALT: Modifiers = Modifiers {
        alt: true,
        ..Modifiers::NONE
    };
    const META: Modifiers = Modifiers {
        meta: true,
        ..Modifiers::NONE
    };

    struct Field {
        omnibox: Omnibox,
        text: TextContexts,
        clipboard: FakeClipboard,
    }

    impl Field {
        fn new(url: &str, mac: bool) -> Self {
            let mut text = TextContexts::new(true);
            let mut omnibox = Omnibox::new(14.0);
            omnibox.set_mac(mac);
            omnibox.set_url(&mut text, url);
            Self {
                omnibox,
                text,
                clipboard: FakeClipboard::default(),
            }
        }

        fn key(&mut self, key: Key, typed: Option<&str>, modifiers: Modifiers) -> KeyResult {
            self.omnibox.key(
                &mut self.text,
                &key,
                typed,
                modifiers,
                Some(&mut self.clipboard),
            )
        }

        fn named(&mut self, key: NamedKey, modifiers: Modifiers) -> KeyResult {
            self.key(Key::Named(key), None, modifiers)
        }

        fn shortcut(&mut self, c: &str, modifiers: Modifiers) -> KeyResult {
            self.key(Key::Character(c.into()), Some(c), modifiers)
        }

        fn type_text(&mut self, text: &str) {
            for c in text.chars() {
                let s = c.to_string();
                let result = self.key(Key::Character(s.clone()), Some(&s), NONE);
                assert_eq!(result, KeyResult::Handled);
            }
        }

        fn focus(&mut self) {
            self.omnibox.focus(&mut self.text);
        }

        fn text(&self) -> String {
            self.omnibox.text()
        }
    }

    #[test]
    fn focus_selects_the_url_and_typing_replaces_it() {
        let mut field = Field::new("https://a.test/", true);
        assert_eq!(field.text(), "https://a.test/");
        field.focus();
        assert_eq!(field.omnibox.selected_text(), Some("https://a.test/"));
        field.type_text("b.test");
        assert_eq!(
            field.named(NamedKey::Enter, NONE),
            KeyResult::Navigate("b.test".into())
        );
        // A new URL while editing doesn't overwrite the edit…
        field.omnibox.set_url(&mut field.text, "https://c.test/");
        assert_eq!(field.text(), "b.test");
        // …Escape goes back to it, and losing focus shows it.
        field.named(NamedKey::Escape, NONE);
        assert_eq!(field.text(), "https://c.test/");
        field.type_text("x");
        field.omnibox.blur(&mut field.text);
        assert_eq!(field.text(), "https://c.test/");
        assert!(!field.omnibox.is_focused());
    }

    #[test]
    fn caret_movement_and_deleting_by_platform() {
        // macOS: Option is a word, Cmd the whole line.
        let mut field = Field::new("", true);
        field.focus();
        field.type_text("one two three");
        field.named(NamedKey::Backspace, ALT);
        assert_eq!(field.text(), "one two ");
        field.named(NamedKey::ArrowLeft, META);
        field.named(NamedKey::Delete, NONE);
        assert_eq!(field.text(), "ne two ");
        field.named(NamedKey::ArrowRight, META);
        field.named(NamedKey::Backspace, META);
        assert_eq!(field.text(), "");
        // Elsewhere: Ctrl is a word.
        let mut field = Field::new("", false);
        field.focus();
        field.type_text("one two");
        field.named(NamedKey::Backspace, CTRL);
        assert_eq!(field.text(), "one ");
        field.named(NamedKey::Home, NONE);
        field.named(NamedKey::End, SHIFT);
        assert_eq!(field.omnibox.selected_text(), Some("one "));
    }

    #[test]
    fn copy_cut_and_paste() {
        let mut field = Field::new("https://a.test/path", true);
        field.focus();
        let cmd = META;
        field.shortcut("c", cmd);
        assert_eq!(field.clipboard.0.as_deref(), Some("https://a.test/path"));
        // Select "path" and cut it.
        field.named(NamedKey::End, NONE);
        for _ in 0..4 {
            field.named(NamedKey::ArrowLeft, SHIFT);
        }
        field.shortcut("x", cmd);
        assert_eq!(field.clipboard.0.as_deref(), Some("path"));
        assert_eq!(field.text(), "https://a.test/");
        // Pasting makes one line.
        field.clipboard.0 = Some("p\nq\r\n".into());
        field.shortcut("v", cmd);
        assert_eq!(field.text(), "https://a.test/p q");
        // Ctrl does nothing on macOS, and types nothing.
        assert_eq!(field.shortcut("a", CTRL), KeyResult::Ignored);
        assert_eq!(field.text(), "https://a.test/p q");
        // Elsewhere it is Ctrl.
        let mut field = Field::new("x", false);
        field.focus();
        field.shortcut("c", CTRL);
        assert_eq!(field.clipboard.0.as_deref(), Some("x"));
    }

    #[test]
    fn ime_composition_then_commit() {
        let mut field = Field::new("", true);
        field.focus();
        field.type_text("a");
        field.omnibox.preedit(&mut field.text, "ㄗˋ", Some((0, 0)));
        assert!(field.omnibox.editor.is_composing());
        field.omnibox.preedit(&mut field.text, "", None);
        field.omnibox.commit(&mut field.text, "字");
        assert_eq!(field.text(), "a字");
        assert!(!field.omnibox.editor.is_composing());
    }

    #[test]
    fn clicks_focus_then_place_the_caret() {
        // On the tree: the first click focuses it, which selects all.
        let mut tree =
            ui::ViewTree::<BrowserUi>::new(BrowserTheme::light(), TextContexts::new(true));
        tree.set_size(SizeF::new(200.0, 30.0));
        tree.set_style(
            tree.root(),
            ui::taffy::Style {
                align_items: ui::taffy::AlignItems::STRETCH,
                ..ui::taffy::Style::default()
            },
        );
        let id = tree.insert(Omnibox::new(14.0));
        tree.append_child(tree.root(), id);
        tree.set_style(
            id,
            ui::taffy::Style {
                flex_grow: 1.0,
                ..ui::taffy::Style::default()
            },
        );
        tree.update(id, |omnibox, ctx| omnibox.set_url(ctx.text(), "abcdef"));
        tree.layout();
        let at = |x: f32, down: bool| {
            let position = PointF::new(x, 5.0);
            if down {
                PointerEvent::Down {
                    position,
                    button: PointerButton::Primary,
                    click_count: 1,
                    modifiers: NONE,
                }
            } else {
                PointerEvent::Up {
                    position,
                    button: PointerButton::Primary,
                    click_count: 1,
                    modifiers: NONE,
                }
            }
        };
        let moved = |x: f32| PointerEvent::Move {
            position: PointF::new(x, 5.0),
            modifiers: NONE,
        };
        tree.pointer(at(190.0, true));
        assert!(tree.get(id).unwrap().is_focused());
        assert_eq!(tree.focused(), Some(id.any()));
        assert_eq!(tree.get(id).unwrap().selected_text(), Some("abcdef"));
        tree.pointer(at(190.0, false));
        // Then a click at the start puts the caret there; dragging to the
        // end selects all of it.
        tree.pointer(at(0.0, true));
        assert_eq!(tree.get(id).unwrap().selected_text(), None);
        tree.pointer(moved(190.0));
        tree.pointer(at(190.0, false));
        assert_eq!(tree.get(id).unwrap().selected_text(), Some("abcdef"));
        // Moving without the button changes nothing.
        tree.pointer(moved(0.0));
        assert_eq!(tree.get(id).unwrap().selected_text(), Some("abcdef"));
        // Losing the keys brings the URL back, unselected.
        tree.set_focus(None);
        assert!(!tree.get(id).unwrap().is_focused());
        assert_eq!(tree.get(id).unwrap().selected_text(), None);
    }

    #[test]
    fn paint_clips_text_and_keeps_the_caret_in_view() {
        let mut field = Field::new("", true);
        field.focus();
        field.type_text(&"w".repeat(100));
        let mut list = DisplayItemList::new();
        let area = RectF::new(10.0, 5.0, 100.0, 20.0);
        field
            .omnibox
            .paint_text(&mut field.text, &BrowserTheme::light(), &mut list, area);
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
