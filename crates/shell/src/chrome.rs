//! The browser's own UI on `ui`'s tree: the tab strip above the toolbar
//! (back, forward, reload and the address field), the types its views
//! share, and the messages they send the browser view. The tree covers
//! the area above the web views; the web views are the browser view's.

use gfx::SizeF;
use ui::taffy::prelude::{length, percent};
use ui::taffy::{AlignItems, FlexDirection, Rect, Size, Style};
use ui::{Clipboard, TextContexts, ViewId, ViewTree};

use crate::browser_view::Command;
use crate::omnibox::Omnibox;
use crate::tab_strip::TabStrip;
use crate::theme::{BrowserTheme, SizeToken};
use crate::toolbar::{self, Button, NavButton, ToolbarRow};

/// What the browser UI's views share: they send [`UiMessage`]s and paint
/// with a [`BrowserTheme`].
pub struct BrowserUi;

impl ui::App for BrowserUi {
    type Message = UiMessage;
    type Theme = BrowserTheme;
}

/// What a view of the browser UI asks of the browser view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UiMessage {
    /// What a shortcut would do too.
    Command(Command),
    /// Close the tab at this index (its close button, or the middle
    /// button on it).
    CloseTab(usize),
    /// Enter in the address field: go to what was typed, as typed.
    Navigate(String),
}

/// The tree and the views the browser view talks to.
pub struct Chrome {
    pub tree: ViewTree<BrowserUi>,
    pub tab_strip: ViewId<TabStrip>,
    pub toolbar: ViewId<ToolbarRow>,
    /// Back, forward and reload, in `Button::ALL`'s order.
    pub buttons: [ViewId<NavButton>; 3],
    pub omnibox: ViewId<Omnibox>,
    /// How high the tab strip is laid out, CSS px.
    strip_height: f32,
}

impl Chrome {
    /// `test_fonts`: Lato for every family, as the renderer's test fonts.
    pub fn new(test_fonts: bool, clipboard: Box<dyn Clipboard>) -> Self {
        let theme = BrowserTheme::light();
        let tab_strip = TabStrip::new(&theme);
        let omnibox = Omnibox::new(theme.size(SizeToken::FieldFontSize));
        let toolbar_style = toolbar_style(&theme);
        let button_size = theme.size(SizeToken::ToolbarButton);
        let field_style = Style {
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_basis: length(0.0),
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            size: Size {
                width: ui::taffy::Dimension::auto(),
                height: length(theme.size(SizeToken::FieldHeight)),
            },
            // The gap before it is two of the buttons'.
            margin: Rect {
                left: length(theme.size(SizeToken::ToolbarGap)),
                right: length(0.0),
                top: length(0.0),
                bottom: length(0.0),
            },
            ..Style::default()
        };
        let mut tree = ViewTree::new(theme, TextContexts::new(test_fonts));
        tree.set_clipboard(clipboard);
        let root = tree.root();
        tree.set_style(
            root,
            Style {
                flex_direction: FlexDirection::Column,
                ..Style::default()
            },
        );
        let tab_strip = tree.insert(tab_strip);
        tree.append_child(root, tab_strip);
        let toolbar = tree.insert(ToolbarRow);
        tree.append_child(root, toolbar);
        tree.set_style(toolbar, toolbar_style);
        let buttons = Button::ALL.map(|button| {
            let id = tree.insert(NavButton::new(button));
            tree.append_child(toolbar, id);
            tree.set_style(
                id,
                Style {
                    flex_shrink: 0.0,
                    size: Size {
                        width: length(button_size),
                        height: length(button_size),
                    },
                    ..Style::default()
                },
            );
            id
        });
        let omnibox = tree.insert(omnibox);
        tree.append_child(toolbar, omnibox);
        tree.set_style(omnibox, field_style);
        let mut chrome = Self {
            tree,
            tab_strip,
            toolbar,
            buttons,
            omnibox,
            strip_height: -1.0,
        };
        chrome.set_size(0.0, 0.0);
        chrome
    }

    /// The UI `width` CSS px wide, its tab strip `strip_height` high (the
    /// toolbar starts there: a whole physical px, which the strip's own
    /// height in CSS px needn't be at every scale), the toolbar under it.
    pub fn set_size(&mut self, width: f32, strip_height: f32) {
        self.tree
            .set_size(SizeF::new(width, strip_height + toolbar::HEIGHT));
        if strip_height != self.strip_height {
            self.strip_height = strip_height;
            self.tree.set_style(
                self.tab_strip,
                Style {
                    flex_shrink: 0.0,
                    size: Size {
                        width: percent(1.0),
                        height: length(strip_height),
                    },
                    ..Style::default()
                },
            );
        }
    }

    pub fn omnibox(&self) -> &Omnibox {
        self.tree.get(self.omnibox).expect("the toolbar keeps it")
    }

    pub fn tab_strip(&self) -> &TabStrip {
        self.tree.get(self.tab_strip).expect("the UI keeps it")
    }

    /// Whether the address field has the keys.
    pub fn omnibox_focused(&self) -> bool {
        self.tree.focused() == Some(self.omnibox.any())
    }

    /// Gives the address field the keys, everything in it selected (again,
    /// what was typed kept, if it has them already: Cmd+L while editing).
    pub fn focus_omnibox(&mut self) {
        if self.omnibox_focused() {
            self.tree.update(self.omnibox, |omnibox, ctx| {
                omnibox.focus(ctx.text());
                ctx.request_paint();
            });
        } else {
            self.tree.set_focus(Some(self.omnibox.any()));
        }
    }

    /// The address field gives the keys up, if it has them: the URL comes
    /// back.
    pub fn blur_omnibox(&mut self) {
        if self.omnibox_focused() {
            self.tree.set_focus(None);
        }
    }

    /// Whether back and forward are enabled.
    pub fn history(&self) -> (bool, bool) {
        let enabled = |id| {
            self.tree
                .get(id)
                .is_some_and(|button: &NavButton| button.is_enabled())
        };
        (enabled(self.buttons[0]), enabled(self.buttons[1]))
    }

    pub fn set_history(&mut self, can_go_back: bool, can_go_forward: bool) {
        for (id, enabled) in [
            (self.buttons[0], can_go_back),
            (self.buttons[1], can_go_forward),
        ] {
            self.tree
                .update(id, |button, ctx| button.set_enabled(enabled, ctx));
        }
    }
}

/// The toolbar's row: as wide as the window, `toolbar::HEIGHT` high, the
/// buttons then the field across it, centered down it.
fn toolbar_style(theme: &BrowserTheme) -> Style {
    let padding = theme.size(SizeToken::ToolbarPadding);
    Style {
        flex_direction: FlexDirection::Row,
        flex_shrink: 0.0,
        align_items: AlignItems::CENTER,
        padding: Rect {
            left: length(padding),
            right: length(padding),
            top: length(0.0),
            bottom: length(0.0),
        },
        gap: Size {
            width: length(theme.size(SizeToken::ToolbarGap)),
            height: length(0.0),
        },
        size: Size {
            width: percent(1.0),
            height: length(toolbar::HEIGHT),
        },
        ..Style::default()
    }
}

#[cfg(test)]
mod tests {
    use gfx::{PointF, RectF};
    use ui::{Modifiers, PointerButton, PointerEvent};

    use super::*;

    struct NoClipboard;

    impl Clipboard for NoClipboard {
        fn read(&mut self) -> Option<String> {
            None
        }
        fn write(&mut self, _: &str) {}
    }

    /// The UI 800 CSS px wide, laid out.
    fn chrome() -> Chrome {
        let mut chrome = Chrome::new(true, Box::new(NoClipboard));
        chrome.set_size(800.0, crate::tab_strip::HEIGHT);
        chrome.tree.layout();
        chrome
    }

    fn rect(chrome: &Chrome, id: impl Into<ui::AnyViewId>) -> RectF {
        chrome.tree.absolute_bounds(id).unwrap()
    }

    fn center(rect: RectF) -> PointF {
        PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    fn click(chrome: &mut Chrome, at: PointF, button: PointerButton) {
        let modifiers = Modifiers::NONE;
        chrome.tree.pointer(PointerEvent::Down {
            position: at,
            button,
            click_count: 1,
            modifiers,
        });
        chrome.tree.pointer(PointerEvent::Up {
            position: at,
            button,
            click_count: 1,
            modifiers,
        });
    }

    #[test]
    fn the_toolbar_lays_its_buttons_then_the_field_across_it() {
        let chrome = chrome();
        let top = crate::tab_strip::HEIGHT;
        assert_eq!(
            rect(&chrome, chrome.toolbar),
            RectF::new(0.0, top, 800.0, toolbar::HEIGHT)
        );
        // 6 in, 28 square, 4 apart, centered down the toolbar.
        for (i, x) in [6.0, 38.0, 70.0].into_iter().enumerate() {
            assert_eq!(
                rect(&chrome, chrome.buttons[i]),
                RectF::new(x, top + 6.0, 28.0, 28.0)
            );
        }
        // Then the field, 4 more after the gap, to 6 from the end.
        assert_eq!(
            rect(&chrome, chrome.omnibox),
            RectF::new(106.0, top + 5.0, 800.0 - 106.0 - 6.0, 30.0)
        );
        // The strip above.
        assert_eq!(
            rect(&chrome, chrome.tab_strip),
            RectF::new(0.0, 0.0, 800.0, top)
        );
    }

    #[test]
    fn a_button_is_pressed_by_down_and_up_on_it_while_enabled() {
        let mut chrome = chrome();
        let back = center(rect(&chrome, chrome.buttons[0]));
        let reload = center(rect(&chrome, chrome.buttons[2]));
        // Back is disabled with nothing to go back to.
        click(&mut chrome, back, PointerButton::Primary);
        assert_eq!(chrome.tree.take_messages(), []);
        chrome.set_history(true, false);
        assert_eq!(chrome.history(), (true, false));
        click(&mut chrome, back, PointerButton::Primary);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::Command(Command::Back)]
        );
        // Down on one, up on another: nothing.
        chrome.tree.pointer(PointerEvent::Down {
            position: back,
            button: PointerButton::Primary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        chrome.tree.pointer(PointerEvent::Up {
            position: reload,
            button: PointerButton::Primary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(chrome.tree.take_messages(), []);
        // Another button does nothing.
        click(&mut chrome, reload, PointerButton::Middle);
        assert_eq!(chrome.tree.take_messages(), []);
        click(&mut chrome, reload, PointerButton::Primary);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::Command(Command::Reload)]
        );
    }

    #[test]
    fn only_an_enabled_button_lights_up_under_the_pointer() {
        let mut chrome = chrome();
        chrome.tree.paint();
        let forward = center(rect(&chrome, chrome.buttons[1]));
        let hover = chrome
            .tree
            .theme()
            .color(crate::theme::ColorToken::ButtonHover);
        let lit = |chrome: &mut Chrome| {
            let frame = chrome.tree.paint();
            frame.is_some_and(|frame| {
                // A button's circle, in its own coordinates (the field is
                // that color too while it hasn't the keys).
                frame.layers[0].display_list.ops().iter().any(|op| {
                    matches!(
                        op,
                        compositor_paint::PaintOp::DrawRRect { rrect, flags }
                            if flags.color == hover && rrect.rect == RectF::new(0.0, 0.0, 28.0, 28.0)
                    )
                })
            })
        };
        chrome.tree.pointer(PointerEvent::Move {
            position: forward,
            modifiers: Modifiers::NONE,
        });
        assert!(!lit(&mut chrome));
        // Enabled under the pointer, it lights up.
        chrome.set_history(false, true);
        assert!(lit(&mut chrome));
        chrome.tree.pointer_left();
        assert!(!lit(&mut chrome));
        assert_eq!(chrome.tree.stale_views(), []);
    }

    #[test]
    fn a_click_in_the_field_focuses_it_with_its_ring() {
        let mut chrome = chrome();
        chrome.tree.update(chrome.omnibox, |omnibox, ctx| {
            omnibox.set_url(ctx.text(), "https://a.test/")
        });
        let field = center(rect(&chrome, chrome.omnibox));
        click(&mut chrome, field, PointerButton::Primary);
        assert!(chrome.omnibox_focused());
        assert_eq!(chrome.omnibox().selected_text(), Some("https://a.test/"));
        let ring = chrome
            .tree
            .theme()
            .color(crate::theme::ColorToken::FocusRing);
        let frame = chrome.tree.paint().unwrap();
        assert!(frame.layers[0].display_list.ops().iter().any(|op| matches!(
            op,
            compositor_paint::PaintOp::DrawRRect { flags, .. } if flags.color == ring
        )));
        // Cmd+L again selects all, what was typed kept.
        chrome.tree.update(chrome.omnibox, |omnibox, ctx| {
            omnibox.mouse_down(ctx.text(), PointF::new(0.0, 5.0), false);
            omnibox.mouse_up();
            ctx.request_paint();
        });
        assert_eq!(chrome.omnibox().selected_text(), None);
        chrome.focus_omnibox();
        assert_eq!(chrome.omnibox().selected_text(), Some("https://a.test/"));
        chrome.blur_omnibox();
        assert!(!chrome.omnibox_focused());
        chrome.tree.paint();
        assert_eq!(chrome.tree.stale_views(), []);
    }

    #[test]
    fn a_press_on_a_disabled_button_is_the_toolbar_s_even_once_it_is_enabled() {
        let mut chrome = chrome();
        let back = center(rect(&chrome, chrome.buttons[0]));
        chrome.tree.pointer(PointerEvent::Down {
            position: back,
            button: PointerButton::Primary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(chrome.tree.captured(), Some(chrome.toolbar.any()));
        // The page it was on has history now: the up isn't a press.
        chrome.set_history(true, false);
        chrome.tree.pointer(PointerEvent::Up {
            position: back,
            button: PointerButton::Primary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(chrome.tree.take_messages(), []);
    }

    #[test]
    fn a_press_on_the_toolbar_s_background_keeps_the_pointer() {
        let mut chrome = chrome();
        // Between reload and the field.
        let at = PointF::new(100.0, crate::tab_strip::HEIGHT + 20.0);
        chrome.tree.pointer(PointerEvent::Down {
            position: at,
            button: PointerButton::Primary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(chrome.tree.captured(), Some(chrome.toolbar.any()));
        // Not with another button.
        chrome.tree.pointer(PointerEvent::Up {
            position: at,
            button: PointerButton::Primary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        chrome.tree.pointer(PointerEvent::Down {
            position: at,
            button: PointerButton::Secondary,
            click_count: 1,
            modifiers: Modifiers::NONE,
        });
        assert_eq!(chrome.tree.captured(), None);
    }

    #[test]
    fn the_tab_strip_sends_what_its_clicks_ask() {
        let mut chrome = chrome();
        chrome.tree.update(chrome.tab_strip, |strip, ctx| {
            strip.set_tabs(vec!["a".into(), "b".into()], 0);
            ctx.request_paint();
        });
        chrome.tree.paint();
        let tab = center(chrome.tab_strip().tab_rect(1));
        let close = center(chrome.tab_strip().close_rect(0));
        let new_tab = center(chrome.tab_strip().new_tab_rect());
        click(&mut chrome, tab, PointerButton::Primary);
        click(&mut chrome, close, PointerButton::Primary);
        click(&mut chrome, tab, PointerButton::Middle);
        click(&mut chrome, new_tab, PointerButton::Primary);
        assert_eq!(
            chrome.tree.take_messages(),
            [
                UiMessage::Command(Command::SelectTab(1)),
                UiMessage::CloseTab(0),
                UiMessage::CloseTab(1),
                UiMessage::Command(Command::NewTab),
            ]
        );
        // Hovered, painted again; left, again.
        chrome.tree.pointer(PointerEvent::Move {
            position: tab,
            modifiers: Modifiers::NONE,
        });
        assert!(chrome.tree.paint().is_some());
        chrome.tree.pointer_left();
        assert!(chrome.tree.paint().is_some());
        assert_eq!(chrome.tree.stale_views(), []);
    }
}
