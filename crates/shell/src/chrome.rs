//! The browser's own UI on `ui`'s tree, in one of two layouts: the tab
//! strip above the toolbar (back, forward, reload and the address field),
//! or a sidebar down the window's left side with the same buttons and field
//! and a row for each tab, after Zen's. The UI is described from a snapshot
//! of the browser's state ([`UiState`]) each time that changes, and the tree
//! keeps the views that are still the same. The tree covers the window: the
//! web views are where its [`WebViewSlot`]s are laid out.

use base::numerics::{clamp_round, saturated_cast};
use gfx::{PointF, Rect, SizeF};
use protocol::TabId;
use ui::taffy::prelude::{fr, length, line, minmax, percent};
use ui::taffy::{AlignItems, Display, FlexDirection, Overflow, Point, Size, Style};
use ui::{
    Clipboard, Component, CreateCtx, Element, EventCtx, ScrollView, TextContexts, View, ViewId,
    ViewTree,
};

use crate::browser_view::Command;
use crate::omnibox::{Omnibox, OmniboxProps};
use crate::sidebar::{
    NewTabRow, SidebarBackdrop, SidebarEdge, SidebarEdgeProps, SidebarPanel, SidebarPanelProps,
    SidebarTab, SidebarTabProps,
};
use crate::tab_strip::{TabStrip, TabStripProps};
use crate::theme::{BrowserTheme, ColorMode, SizeToken};
use crate::toolbar::{self, Button, NavButton, NavButtonProps, ToolbarRow};

/// What the browser UI's views share: they send [`UiMessage`]s and paint
/// with a [`BrowserTheme`].
pub struct BrowserUi;

impl ui::App for BrowserUi {
    type Message = UiMessage;
    type Theme = BrowserTheme;
}

/// What a view of the browser UI asks of the browser view.
#[derive(Debug, Clone, PartialEq)]
pub enum UiMessage {
    /// What a shortcut would do too.
    Command(Command),
    /// Close the tab at this index (its close button, or the middle
    /// button on it, in the tab strip).
    CloseTab(usize),
    /// Show this tab (its row in the sidebar).
    SelectTabById(TabId),
    /// Close this tab (its row's close button, or the middle button on it).
    CloseTabById(TabId),
    /// Enter in the address field: go to what was typed, as typed.
    Navigate(String),
    /// The sidebar's edge dragged: the width it would be, CSS px.
    ResizeSidebar(f32),
}

/// Which way the UI is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLayout {
    /// The tab strip above the toolbar, above the page.
    #[default]
    Horizontal,
    /// A sidebar down the left, the page right of it.
    Sidebar,
}

/// What the UI shows of the browser: all a description of it reads.
#[derive(Debug, Clone, PartialEq)]
pub struct UiState {
    pub layout: UiLayout,
    /// The tabs, in order, their titles, and the active one's index.
    pub tabs: Vec<TabId>,
    pub titles: Vec<String>,
    pub active: usize,
    /// The web contents shown, the active tab's first; a layout may show
    /// only some of them.
    pub shown: Vec<TabId>,
    /// The active tab's URL.
    pub url: String,
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// Editing keys as on macOS.
    pub mac: bool,
    /// The window's size, CSS px, and its scale.
    pub width: f32,
    pub height: f32,
    pub scale: f32,
    /// How high the tab strip is, CSS px: the toolbar starts there, a whole
    /// physical px, which the strip's own height in CSS px needn't be at
    /// every scale.
    pub strip_height: f32,
    /// How wide the sidebar is, CSS px.
    pub sidebar_width: f32,
    /// The sidebar collapsed: the page takes the window, and the sidebar
    /// is shown only floated over it.
    pub sidebar_collapsed: bool,
    /// Collapsed, floated over the page.
    pub sidebar_floating: bool,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            layout: UiLayout::Horizontal,
            tabs: Vec::new(),
            titles: Vec::new(),
            active: 0,
            shown: Vec::new(),
            url: String::new(),
            can_go_back: false,
            can_go_forward: false,
            mac: cfg!(target_os = "macos"),
            width: 0.0,
            height: 0.0,
            scale: 1.0,
            strip_height: crate::tab_strip::HEIGHT,
            sidebar_width: BrowserTheme::light().size(SizeToken::SidebarWidth),
            sidebar_collapsed: false,
            sidebar_floating: false,
        }
    }
}

/// The browser UI as it is now: the tab strip, then the toolbar's row of
/// back, forward and reload and the address field, as wide as the window.
pub fn horizontal(state: &UiState, theme: &BrowserTheme) -> Element<BrowserUi> {
    let button_size = theme.size(SizeToken::ToolbarButton);
    let button = |button: Button, name, enabled| {
        Element::new::<NavButton>(NavButtonProps { button, enabled })
            .name(name)
            .style(Style {
                flex_shrink: 0.0,
                size: Size {
                    width: length(button_size),
                    height: length(button_size),
                },
                ..Style::default()
            })
    };
    Element::container()
        .style(Style {
            flex_direction: FlexDirection::Column,
            ..Style::default()
        })
        .child(
            Element::new::<TabStrip>(TabStripProps {
                titles: state.titles.clone(),
                active: state.active,
            })
            .name("tab-strip")
            .style(Style {
                flex_shrink: 0.0,
                size: Size {
                    width: percent(1.0),
                    height: length(state.strip_height),
                },
                ..Style::default()
            }),
        )
        .child(
            Element::new::<ToolbarRow>(())
                .name("toolbar")
                .style(toolbar_style(theme))
                .child(button(Button::Back, "back", state.can_go_back))
                .child(button(Button::Forward, "forward", state.can_go_forward))
                .child(button(Button::Reload, "reload", true))
                .child(
                    Element::new::<Omnibox>(OmniboxProps {
                        url: state.url.clone(),
                        mac: state.mac,
                    })
                    .name("omnibox")
                    .style(field_style(theme)),
                ),
        )
        .children(first_web_view(state, None))
}

/// The UI laid out down a sidebar on the left: back, forward and reload,
/// the address field, a new tab row and a row for each tab; its edge, which
/// drags; the page right of it.
pub fn sidebar(state: &UiState, theme: &BrowserTheme) -> Element<BrowserUi> {
    let padding = theme.size(SizeToken::SidebarPadding);
    let button_size = theme.size(SizeToken::ToolbarButton);
    let fixed = |width, height| Style {
        flex_shrink: 0.0,
        size: Size { width, height },
        ..Style::default()
    };
    let button = |button: Button, name, enabled| {
        Element::new::<NavButton>(NavButtonProps { button, enabled })
            .name(name)
            .style(fixed(length(button_size), length(button_size)))
    };
    let tabs = state
        .tabs
        .iter()
        .zip(&state.titles)
        .enumerate()
        .map(|(index, (&tab, title))| {
            Element::new::<SidebarTab>(SidebarTabProps {
                tab,
                title: title.clone(),
                active: index == state.active,
            })
            .key(u64::from(tab.0))
            .style(fixed(
                ui::taffy::Dimension::auto(),
                ui::taffy::Dimension::auto(),
            ))
        });
    let gap = theme.size(SizeToken::CardGap);
    let collapsed = state.sidebar_collapsed;
    // The same view expanded, hidden and floated: what is typed in its
    // field stays.
    let panel = Element::new::<SidebarPanel>(SidebarPanelProps {
        floating: collapsed,
    })
    .name("sidebar")
    .style(Style {
        display: if collapsed && !state.sidebar_floating {
            Display::None
        } else {
            Display::Flex
        },
        grid_row: line(1),
        grid_column: line(1),
        justify_self: Some(AlignItems::START),
        flex_direction: FlexDirection::Column,
        padding: ui::taffy::Rect {
            left: length(padding),
            right: length(padding),
            top: length(padding),
            bottom: length(padding),
        },
        gap: Size {
            width: length(0.0),
            height: length(padding),
        },
        size: Size {
            width: length(state.sidebar_width),
            height: percent(1.0),
        },
        ..Style::default()
    })
    .child(
        Element::container()
            .style(Style {
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::CENTER,
                gap: Size {
                    width: length(theme.size(SizeToken::ToolbarGap)),
                    height: length(0.0),
                },
                ..Style::default()
            })
            .child(button(Button::Back, "back", state.can_go_back))
            .child(button(Button::Forward, "forward", state.can_go_forward))
            .child(button(Button::Reload, "reload", true)),
    )
    .child(
        Element::new::<Omnibox>(OmniboxProps {
            url: state.url.clone(),
            mac: state.mac,
        })
        .name("omnibox")
        .style(fixed(
            ui::taffy::Dimension::auto(),
            length(theme.size(SizeToken::FieldHeight)),
        )),
    )
    .child(Element::new::<NewTabRow>(()).name("new-tab").style(fixed(
        ui::taffy::Dimension::auto(),
        ui::taffy::Dimension::auto(),
    )))
    // The rows scroll under the buttons, the field and New
    // Tab, which stay.
    .child(
        Element::new::<ScrollView>(())
            .name("tabs")
            .style(Style {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                // As tall as the sidebar leaves it, not its rows.
                overflow: Point {
                    x: Overflow::Visible,
                    y: Overflow::Scroll,
                },
                gap: Size {
                    width: length(0.0),
                    height: length(2.0),
                },
                ..Style::default()
            })
            .children(tabs),
    );
    // Under the panel: where it is (nothing when collapsed), the gap right
    // of it with the edge that drags it, and the page, a card in the gaps.
    let place = if collapsed {
        vec![Element::container().style(fixed(length(gap), percent(1.0)))]
    } else {
        vec![
            Element::container().style(fixed(length(state.sidebar_width), percent(1.0))),
            Element::new::<SidebarEdge>(SidebarEdgeProps {
                width: state.sidebar_width,
            })
            .name("sidebar-edge")
            .style(fixed(length(gap), percent(1.0))),
        ]
    };
    let page = Element::new::<SidebarBackdrop>(())
        .style(Style {
            grid_row: line(1),
            grid_column: line(1),
            flex_direction: FlexDirection::Row,
            ..Style::default()
        })
        .children(place)
        .children(first_web_view(
            state,
            Some(Card {
                gap,
                radius: theme.size(SizeToken::CardRadius),
            }),
        ));
    // One cell, the page's row and the panel over it: the window's size,
    // whatever is in it (`minmax(0, 1fr)`, not `1fr`, whose minimum is
    // its content's).
    Element::container()
        .style(Style {
            display: Display::Grid,
            grid_template_columns: vec![minmax(length(0.0), fr(1.0))],
            grid_template_rows: vec![minmax(length(0.0), fr(1.0))],
            ..Style::default()
        })
        .child(page)
        .child(panel)
}

/// The UI as `state` says it is laid out.
pub fn describe(state: &UiState, theme: &BrowserTheme) -> Element<BrowserUi> {
    match state.layout {
        UiLayout::Horizontal => horizontal(state, theme),
        UiLayout::Sidebar => sidebar(state, theme),
    }
}

/// A web view as a card: its gaps to what is around it, and its corners.
#[derive(Debug, Clone, Copy)]
struct Card {
    gap: f32,
    radius: f32,
}

/// The active tab's web view, taking the rest of its parent: the layouts
/// show one web contents (another, like a docked DevTools', needs a layout
/// of its own). As a card, with gaps above, right and below it (the one
/// left of it is its parent's).
fn first_web_view(state: &UiState, card: Option<Card>) -> Option<Element<BrowserUi>> {
    let &tab = state.shown.first()?;
    let gap = card.map_or(0.0, |card| card.gap);
    Some(
        Element::new::<WebViewSlot>(WebViewSlotProps {
            tab,
            radius: card.map_or(0.0, |card| card.radius),
        })
        .key(u64::from(tab.0))
        .style(Style {
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_basis: length(0.0),
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            margin: ui::taffy::Rect {
                left: length(0.0),
                right: length(gap),
                top: length(gap),
                bottom: length(gap),
            },
            ..Style::default()
        }),
    )
}

/// Which web contents a web view slot shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WebViewSlotProps {
    pub tab: TabId,
    /// Its corners' radius, CSS px: 0 square.
    pub radius: f32,
}

/// Where a web view is (Chromium's `views::WebView`): it draws nothing and
/// isn't hit itself; the browser view puts the web contents' surface where
/// it is laid out, and sends it the input there.
pub struct WebViewSlot {
    tab: TabId,
    radius: f32,
}

impl WebViewSlot {
    pub fn tab(&self) -> TabId {
        self.tab
    }

    /// Its corners' radius, CSS px.
    pub fn radius(&self) -> f32 {
        self.radius
    }
}

impl Component<BrowserUi> for WebViewSlot {
    type Props = WebViewSlotProps;

    fn create(props: &WebViewSlotProps, _: &mut CreateCtx<'_, BrowserUi>) -> Self {
        Self {
            tab: props.tab,
            radius: props.radius,
        }
    }

    fn set_props(&mut self, props: &WebViewSlotProps, _: &mut EventCtx<'_, BrowserUi>) {
        self.tab = props.tab;
        self.radius = props.radius;
    }
}

impl View<BrowserUi> for WebViewSlot {
    fn hit_test(&self, _: PointF) -> bool {
        false
    }
}

/// A layout of the browser's own: the UI for a state, instead of the one
/// its `layout` names (tests that show two web views lay them out so).
pub type CustomLayout = Box<dyn Fn(&UiState, &BrowserTheme) -> Element<BrowserUi>>;

/// The tree and the views the browser view talks to.
pub struct Chrome {
    pub tree: ViewTree<BrowserUi>,
    /// The horizontal layout's.
    pub tab_strip: Option<ViewId<TabStrip>>,
    pub toolbar: Option<ViewId<ToolbarRow>>,
    /// Back, forward and reload, in `Button::ALL`'s order: every layout
    /// has them, and the address field.
    pub buttons: [ViewId<NavButton>; 3],
    pub omnibox: ViewId<Omnibox>,
    /// The sidebar's list of tabs, which scrolls.
    pub tab_list: Option<ViewId<ScrollView>>,
    /// What the UI was last described from.
    state: UiState,
    custom: Option<CustomLayout>,
}

impl Chrome {
    /// `test_fonts`: Lato for every family, as the renderer's test fonts.
    pub fn new(test_fonts: bool, clipboard: Box<dyn Clipboard>) -> Self {
        let mut tree = ViewTree::new(BrowserTheme::light(), TextContexts::new(test_fonts));
        tree.set_clipboard(clipboard);
        let state = UiState::default();
        tree.describe(describe(&state, tree.theme()));
        let mut chrome = Self {
            tab_strip: None,
            toolbar: None,
            tab_list: None,
            buttons: [
                found(&tree, "back"),
                found(&tree, "forward"),
                found(&tree, "reload"),
            ],
            omnibox: found(&tree, "omnibox"),
            tree,
            state,
            custom: None,
        };
        chrome.find_views();
        chrome
    }

    /// The views the browser view talks to, in the layout described last.
    fn find_views(&mut self) {
        let tree = &self.tree;
        self.tab_strip = tree.find("tab-strip");
        self.toolbar = tree.find("toolbar");
        self.tab_list = tree.find("tabs");
        self.buttons = [
            found(tree, "back"),
            found(tree, "forward"),
            found(tree, "reload"),
        ];
        self.omnibox = found(tree, "omnibox");
    }

    /// Paints the UI light or dark from now on: all of it again, if that's
    /// a change. The sizes are the same, so nothing moves.
    pub fn set_color_mode(&mut self, mode: ColorMode) {
        if self.tree.theme().mode() != mode {
            self.tree.set_theme(BrowserTheme::new(mode));
        }
    }

    pub fn color_mode(&self) -> ColorMode {
        self.tree.theme().mode()
    }

    /// Lays the UI out with `layout` from now on, instead of as the state
    /// says (`None`: as it says).
    pub fn set_custom_layout(&mut self, layout: Option<CustomLayout>) {
        self.custom = layout;
        let state = self.state.clone();
        self.describe(&state);
    }

    /// Describes the UI again from `state`, if it changed: only the views
    /// whose props changed are touched.
    pub fn update(&mut self, state: UiState) {
        if state == self.state {
            return;
        }
        let (list, active) = (
            self.tab_list,
            self.state.tabs.get(self.state.active).copied(),
        );
        self.describe(&state);
        self.state = state;
        // Another tab shown, or the list new (the sidebar laid out anew):
        // its row is scrolled to, if it is out of sight.
        if self.tab_list != list || self.state.tabs.get(self.state.active).copied() != active {
            self.reveal_active_tab();
        }
    }

    /// Scrolls the sidebar's list as little as shows the active tab's row
    /// (Chromium's `ScrollRectToVisible`).
    fn reveal_active_tab(&mut self) {
        let Some(list) = self.tab_list else {
            return;
        };
        // Where the row is now: the list laid out with it.
        self.tree.layout();
        let Some(row) = self.tree.children(list).get(self.state.active).copied() else {
            return;
        };
        if let Some(rect) = self.tree.bounds(row) {
            self.tree
                .update(list, |list, ctx| list.scroll_rect_to_visible(rect, ctx));
        }
    }

    fn describe(&mut self, state: &UiState) {
        self.tree.set_size(SizeF::new(state.width, state.height));
        let element = match &self.custom {
            Some(layout) => layout(state, self.tree.theme()),
            None => describe(state, self.tree.theme()),
        };
        self.tree.describe(element);
        self.find_views();
    }

    pub fn state(&self) -> &UiState {
        &self.state
    }

    /// The web views the UI lays out, in paint order: the web contents
    /// each shows, and where, physical px of the window at `scale`, whole
    /// pixels (a page between two would be blurred; what is drawn, the size
    /// a renderer is told and where clicks land all agree).
    pub fn web_views(&mut self, scale: f32) -> Vec<(TabId, Rect)> {
        self.tree.layout();
        let tree = &self.tree;
        tree.find_all::<WebViewSlot>()
            .into_iter()
            .filter_map(|id| {
                let tab = tree.get(id)?.tab();
                let at = tree.absolute_bounds(id)?;
                let x0: i32 = clamp_round(at.x * scale);
                let y0: i32 = clamp_round(at.y * scale);
                let x1: i32 = clamp_round(at.right() * scale);
                let y1: i32 = clamp_round(at.bottom() * scale);
                Some((
                    tab,
                    Rect::new(
                        x0,
                        y0,
                        saturated_cast(x1.saturating_sub(x0)),
                        saturated_cast(y1.saturating_sub(y0)),
                    ),
                ))
            })
            .collect()
    }

    /// The corners' radius of the web view showing `tab`, CSS px.
    pub fn web_view_radius(&self, tab: TabId) -> f32 {
        let tree = &self.tree;
        tree.find_all::<WebViewSlot>()
            .into_iter()
            .filter_map(|id| tree.get(id))
            .find(|slot| slot.tab() == tab)
            .map_or(0.0, WebViewSlot::radius)
    }

    /// The sidebar, while it floats over the page (collapsed and shown).
    pub fn floating_panel(&self) -> Option<ViewId<SidebarPanel>> {
        let panel = self.tree.find::<SidebarPanel>("sidebar")?;
        let floating = self.tree.get(panel)?.is_floating();
        let shown = self
            .tree
            .style(panel)
            .is_some_and(|style| style.display != Display::None);
        (floating && shown).then_some(panel)
    }

    pub fn omnibox(&self) -> &Omnibox {
        self.tree.get(self.omnibox).expect("the toolbar keeps it")
    }

    /// The tab strip, in the horizontal layout.
    pub fn tab_strip(&self) -> Option<&TabStrip> {
        self.tree.get(self.tab_strip?)
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
}

/// The view the description names `name`: a horizontal and a vertical UI
/// name theirs alike, and keep them while they last.
fn found<T: ui::View<BrowserUi>>(tree: &ViewTree<BrowserUi>, name: &str) -> ViewId<T> {
    tree.find(name).expect("the description names it")
}

/// The toolbar's row: as wide as the window, `toolbar::HEIGHT` high, the
/// buttons then the field across it, centered down it.
fn toolbar_style(theme: &BrowserTheme) -> Style {
    let padding = theme.size(SizeToken::ToolbarPadding);
    Style {
        flex_direction: FlexDirection::Row,
        flex_shrink: 0.0,
        align_items: AlignItems::CENTER,
        padding: ui::taffy::Rect {
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

/// The address field: what the buttons leave of the row, the gap before
/// it two of the buttons'.
fn field_style(theme: &BrowserTheme) -> Style {
    Style {
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
        margin: ui::taffy::Rect {
            left: length(theme.size(SizeToken::ToolbarGap)),
            right: length(0.0),
            top: length(0.0),
            bottom: length(0.0),
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

    /// The UI's state, 800 CSS px wide.
    fn state() -> UiState {
        UiState {
            width: 800.0,
            height: 600.0,
            ..UiState::default()
        }
    }

    /// The UI for `state()`, laid out.
    fn chrome() -> Chrome {
        let mut chrome = Chrome::new(true, Box::new(NoClipboard));
        chrome.update(state());
        chrome.tree.layout();
        chrome
    }

    fn set_history(chrome: &mut Chrome, can_go_back: bool, can_go_forward: bool) {
        chrome.update(UiState {
            can_go_back,
            can_go_forward,
            ..state()
        });
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
            rect(&chrome, chrome.toolbar.unwrap()),
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
            rect(&chrome, chrome.tab_strip.unwrap()),
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
        set_history(&mut chrome, true, false);
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
        set_history(&mut chrome, false, true);
        assert!(lit(&mut chrome));
        chrome.tree.pointer_left();
        assert!(!lit(&mut chrome));
        assert_eq!(chrome.tree.stale_views(), []);
    }

    #[test]
    fn a_click_in_the_field_focuses_it_with_its_ring() {
        let mut chrome = chrome();
        chrome.update(UiState {
            url: "https://a.test/".into(),
            ..state()
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
    fn a_new_color_mode_paints_everything_and_the_same_one_nothing() {
        let mut chrome = chrome();
        chrome.tree.paint();
        chrome.set_color_mode(ColorMode::Light);
        assert!(chrome.tree.paint().is_none());
        chrome.set_color_mode(ColorMode::Dark);
        assert_eq!(chrome.color_mode(), ColorMode::Dark);
        let toolbar = BrowserTheme::dark().color(crate::theme::ColorToken::ToolbarBackground);
        let frame = chrome.tree.paint().unwrap();
        let ops = frame.layers[0].display_list.ops();
        assert!(ops.iter().any(|op| matches!(
            op,
            compositor_paint::PaintOp::DrawRect { flags, .. } if flags.color == toolbar
        )));
        // Painted all again: as if dark from the start.
        let mut fresh = Chrome::new(true, Box::new(NoClipboard));
        fresh.set_color_mode(ColorMode::Dark);
        fresh.update(state());
        let fresh = fresh.tree.paint().unwrap();
        assert_eq!(ops, fresh.layers[0].display_list.ops());
        chrome.set_color_mode(ColorMode::Dark);
        assert!(chrome.tree.paint().is_none());
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
        assert_eq!(chrome.tree.captured(), Some(chrome.toolbar.unwrap().any()));
        // The page it was on has history now: the up isn't a press.
        set_history(&mut chrome, true, false);
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
        assert_eq!(chrome.tree.captured(), Some(chrome.toolbar.unwrap().any()));
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
        chrome.update(UiState {
            titles: vec!["a".into(), "b".into()],
            ..state()
        });
        chrome.tree.paint();
        let tab = center(chrome.tab_strip().unwrap().tab_rect(1));
        let close = center(chrome.tab_strip().unwrap().close_rect(0));
        let new_tab = center(chrome.tab_strip().unwrap().new_tab_rect());
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

#[cfg(test)]
mod description_tests {
    use super::*;

    struct NoClipboard;

    impl Clipboard for NoClipboard {
        fn read(&mut self) -> Option<String> {
            None
        }
        fn write(&mut self, _: &str) {}
    }

    #[test]
    fn the_horizontal_ui_is_the_strip_then_the_toolbar_from_the_state() {
        let mut chrome = Chrome::new(true, Box::new(NoClipboard));
        let state = UiState {
            titles: vec!["one".into(), "two".into()],
            active: 1,
            url: "https://a.test/".into(),
            can_go_back: true,
            can_go_forward: false,
            mac: true,
            width: 640.0,
            height: 480.0,
            strip_height: 34.0,
            ..UiState::default()
        };
        chrome.update(state.clone());
        let root = chrome.tree.root();
        assert_eq!(
            chrome.tree.children(root),
            [
                chrome.tab_strip.unwrap().any(),
                chrome.toolbar.unwrap().any()
            ]
        );
        assert_eq!(chrome.tab_strip().unwrap().titles(), ["one", "two"]);
        assert_eq!(chrome.omnibox().text(), "https://a.test/");
        assert_eq!(chrome.history(), (true, false));
        assert_eq!(chrome.tree.size(), SizeF::new(640.0, 480.0));
        chrome.tree.paint();
        // The same views, told what changed.
        let views = (chrome.tab_strip, chrome.omnibox, chrome.buttons);
        chrome.update(UiState {
            titles: vec!["one".into()],
            active: 0,
            url: "https://b.test/".into(),
            can_go_back: false,
            ..state
        });
        assert_eq!(views, (chrome.tab_strip, chrome.omnibox, chrome.buttons));
        assert!(chrome.tree.contains(chrome.tab_strip.unwrap()));
        assert_eq!(chrome.tab_strip().unwrap().titles(), ["one"]);
        assert_eq!(chrome.omnibox().text(), "https://b.test/");
        assert_eq!(chrome.history(), (false, false));
        // Checked: each view changed asked to be painted again.
        chrome.tree.paint();
    }
}

#[cfg(test)]
mod made_from_props_tests {
    use super::*;

    #[test]
    fn buttons_made_from_a_state_with_history_are_enabled() {
        // A UI described for the first time with history: the buttons are
        // made from the props, not told after.
        let mut tree = ViewTree::<BrowserUi>::new(BrowserTheme::light(), TextContexts::new(true));
        let state = UiState {
            can_go_back: true,
            can_go_forward: true,
            width: 400.0,
            height: 300.0,
            ..UiState::default()
        };
        tree.describe(horizontal(&state, tree.theme()));
        for name in ["back", "forward", "reload"] {
            let button: ViewId<NavButton> = found(&tree, name);
            assert!(tree.get(button).unwrap().is_enabled(), "{name}");
        }
    }
}

#[cfg(test)]
mod web_view_tests {
    use super::*;
    use crate::browser_view_layout::tab_strip_height;

    struct NoClipboard;

    impl Clipboard for NoClipboard {
        fn read(&mut self) -> Option<String> {
            None
        }
        fn write(&mut self, _: &str) {}
    }

    /// Where the tab's web view is in a window `width` × `height` physical
    /// px at `scale`, laid out `layout`, as the browser view lays it out.
    fn web_view(layout: UiLayout, width: u32, height: u32, scale: f32) -> Rect {
        web_view_with(layout, width, height, scale, 260.0)
    }

    fn web_view_with(
        layout: UiLayout,
        width: u32,
        height: u32,
        scale: f32,
        sidebar_width: f32,
    ) -> Rect {
        let mut chrome = Chrome::new(true, Box::new(NoClipboard));
        chrome.update(UiState {
            layout,
            tabs: vec![TabId(0)],
            titles: vec!["a".into()],
            shown: vec![TabId(0)],
            width: width as f32 / scale,
            height: height as f32 / scale,
            scale,
            strip_height: tab_strip_height(scale) as f32 / scale,
            sidebar_width,
            ..UiState::default()
        });
        let views = chrome.web_views(scale);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].0, TabId(0));
        views[0].1
    }

    #[test]
    fn the_page_is_under_the_toolbar_on_a_whole_pixel() {
        let horizontal = |w, h, scale| web_view(UiLayout::Horizontal, w, h, scale);
        // 34 + 40 CSS px at 1x; 51 + 60 px at 1.5x.
        assert_eq!(horizontal(400, 300, 1.0), Rect::new(0, 74, 400, 226));
        assert_eq!(horizontal(600, 450, 1.5), Rect::new(0, 111, 600, 339));
        // At 1.33x, 45.22px and 53.2px: the page starts on a whole pixel.
        assert_eq!(horizontal(400, 300, 1.33), Rect::new(0, 98, 400, 202));
        // Shorter than the tab strip and the toolbar: nothing left of it.
        assert!(horizontal(400, 50, 1.0).is_empty());
    }

    #[test]
    fn the_page_is_a_card_right_of_the_sidebar() {
        // 260 + the gap of 8 CSS px in, 8 from the other edges.
        assert_eq!(
            web_view(UiLayout::Sidebar, 800, 600, 1.0),
            Rect::new(268, 8, 524, 584)
        );
        assert_eq!(
            web_view(UiLayout::Sidebar, 1200, 900, 1.5),
            Rect::new(402, 12, 786, 876)
        );
        // Dragged to 261: the page from 403.5px, rounded half away from 0.
        assert_eq!(
            web_view_with(UiLayout::Sidebar, 1200, 900, 1.5, 261.0),
            Rect::new(404, 12, 784, 876)
        );
    }
}

#[cfg(test)]
mod sidebar_tests {
    use gfx::RectF;
    use ui::{Modifiers, PointerButton, PointerEvent};

    use super::*;

    struct NoClipboard;

    impl Clipboard for NoClipboard {
        fn read(&mut self) -> Option<String> {
            None
        }
        fn write(&mut self, _: &str) {}
    }

    /// Three tabs, the second active, in the sidebar of an 800 × 600 window.
    fn state() -> UiState {
        UiState {
            layout: UiLayout::Sidebar,
            tabs: vec![TabId(1), TabId(2), TabId(3)],
            titles: vec!["one".into(), "two".into(), "three".into()],
            active: 1,
            shown: vec![TabId(2)],
            width: 800.0,
            height: 600.0,
            ..UiState::default()
        }
    }

    fn chrome(state: UiState) -> Chrome {
        let mut chrome = Chrome::new(true, Box::new(NoClipboard));
        chrome.update(state);
        chrome.tree.layout();
        chrome.tree.paint();
        chrome
    }

    fn rows(chrome: &Chrome) -> Vec<ViewId<SidebarTab>> {
        chrome.tree.find_all()
    }

    fn rect(chrome: &Chrome, id: impl Into<ui::AnyViewId>) -> RectF {
        chrome.tree.absolute_bounds(id).unwrap()
    }

    fn center(rect: RectF) -> PointF {
        PointF::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
    }

    fn press(chrome: &mut Chrome, at: PointF, button: PointerButton, down: bool) {
        let modifiers = Modifiers::NONE;
        chrome.tree.pointer(if down {
            PointerEvent::Down {
                position: at,
                button,
                click_count: 1,
                modifiers,
            }
        } else {
            PointerEvent::Up {
                position: at,
                button,
                click_count: 1,
                modifiers,
            }
        });
    }

    fn click(chrome: &mut Chrome, at: PointF, button: PointerButton) {
        press(chrome, at, button, true);
        press(chrome, at, button, false);
    }

    /// A row's close button's center: 10 from its end.
    fn close_of(row: RectF) -> PointF {
        PointF::new(row.right() - 10.0 - 8.0, row.y + row.height / 2.0)
    }

    #[test]
    fn the_sidebar_is_the_buttons_the_field_new_tab_and_a_row_a_tab() {
        let chrome = chrome(state());
        assert!(chrome.tab_strip.is_none() && chrome.toolbar.is_none());
        let sidebar = chrome.tree.find::<SidebarPanel>("sidebar").unwrap();
        assert_eq!(rect(&chrome, sidebar), RectF::new(0.0, 0.0, 260.0, 600.0));
        // 8 in: the buttons, then the field under them, then new tab, then
        // the rows, 8 apart.
        assert_eq!(
            rect(&chrome, chrome.buttons[0]),
            RectF::new(8.0, 8.0, 28.0, 28.0)
        );
        assert_eq!(
            rect(&chrome, chrome.omnibox),
            RectF::new(8.0, 44.0, 244.0, 30.0)
        );
        let new_tab = chrome.tree.find::<NewTabRow>("new-tab").unwrap();
        assert_eq!(rect(&chrome, new_tab), RectF::new(8.0, 82.0, 244.0, 36.0));
        let rows = rows(&chrome);
        assert_eq!(rows.len(), 3);
        assert_eq!(rect(&chrome, rows[0]), RectF::new(8.0, 126.0, 244.0, 36.0));
        assert_eq!(rect(&chrome, rows[1]).y, 126.0 + 38.0);
        // The gap with the edge in it, then the page. The middle 4 of the
        // gap's 8 drag; its sides are only the gap.
        let edge = chrome.tree.find::<SidebarEdge>("sidebar-edge").unwrap();
        assert_eq!(rect(&chrome, edge), RectF::new(260.0, 0.0, 8.0, 600.0));
        assert_eq!(
            chrome.tree.cursor(center(rect(&chrome, edge))),
            ui::Cursor::ResizeHorizontal
        );
        for x in [261.0, 266.5] {
            let hit = chrome.tree.hit_test(PointF::new(x, 300.0));
            assert_ne!(hit.map(|(id, _)| id), Some(edge.any()), "{x}");
        }
    }

    #[test]
    fn a_tab_s_row_stays_its_own_as_tabs_open_close_and_move() {
        let mut chrome = chrome(state());
        let before = rows(&chrome);
        // The first closed, a fourth at the end, the last two swapped.
        chrome.update(UiState {
            tabs: vec![TabId(3), TabId(2), TabId(4)],
            titles: vec!["three".into(), "two".into(), "four".into()],
            active: 1,
            ..state()
        });
        let after = rows(&chrome);
        assert_eq!(after[..2], [before[2], before[1]]);
        assert!(!chrome.tree.contains(before[0]));
        assert!(!before.contains(&after[2]));
        chrome.tree.paint();
    }

    #[test]
    fn rows_select_close_and_open_tabs() {
        let mut chrome = chrome(state());
        let rows = rows(&chrome);
        let first = rect(&chrome, rows[0]);
        let active = rect(&chrome, rows[1]);
        // A press on a row shows its tab at once.
        press(&mut chrome, center(first), PointerButton::Primary, true);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::SelectTabById(TabId(1))]
        );
        press(&mut chrome, center(first), PointerButton::Primary, false);
        assert_eq!(chrome.tree.take_messages(), []);
        // The active row's close button, down and up on it.
        click(&mut chrome, close_of(active), PointerButton::Primary);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::CloseTabById(TabId(2))]
        );
        // Down on it, up off it: nothing.
        press(&mut chrome, close_of(active), PointerButton::Primary, true);
        press(&mut chrome, center(first), PointerButton::Primary, false);
        assert_eq!(chrome.tree.take_messages(), []);
        // The middle button closes the row it clicks.
        click(&mut chrome, center(first), PointerButton::Middle);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::CloseTabById(TabId(1))]
        );
        // The new tab row: down and up on it, not off it.
        let new_tab = chrome.tree.find::<NewTabRow>("new-tab").unwrap();
        let at = center(rect(&chrome, new_tab));
        press(&mut chrome, at, PointerButton::Primary, true);
        press(&mut chrome, center(first), PointerButton::Primary, false);
        assert_eq!(chrome.tree.take_messages(), []);
        click(&mut chrome, at, PointerButton::Primary);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::Command(Command::NewTab)]
        );
        chrome.tree.paint();
    }

    #[test]
    fn a_row_not_active_has_its_close_button_under_the_pointer() {
        let mut chrome = chrome(state());
        let first = rect(&chrome, rows(&chrome)[0]);
        // The pointer over the row, it has a close button (a press comes
        // with the pointer over it).
        click(&mut chrome, close_of(first), PointerButton::Primary);
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::CloseTabById(TabId(1))]
        );
        // Hovered and left, painted each time.
        chrome.tree.pointer(PointerEvent::Move {
            position: center(first),
            modifiers: Modifiers::NONE,
        });
        assert!(chrome.tree.paint().is_some());
        chrome.tree.pointer_left();
        assert!(chrome.tree.paint().is_some());
    }

    #[test]
    fn dragging_the_edge_asks_for_the_width_under_the_pointer() {
        let mut chrome = chrome(state());
        let edge = chrome.tree.find::<SidebarEdge>("sidebar-edge").unwrap();
        let grab = PointF::new(264.0, 300.0);
        assert_eq!(rect(&chrome, edge).x, 260.0);
        press(&mut chrome, grab, PointerButton::Primary, true);
        chrome.tree.pointer(PointerEvent::Move {
            position: PointF::new(304.0, 300.0),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::ResizeSidebar(300.0)]
        );
        // The browser takes it: the edge is there now, and a further move
        // goes on from it.
        chrome.update(UiState {
            sidebar_width: 300.0,
            ..state()
        });
        chrome.tree.layout();
        assert_eq!(rect(&chrome, edge).x, 300.0);
        chrome.tree.pointer(PointerEvent::Move {
            position: PointF::new(294.0, 300.0),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::ResizeSidebar(290.0)]
        );
        press(
            &mut chrome,
            PointF::new(294.0, 300.0),
            PointerButton::Primary,
            false,
        );
        chrome.tree.pointer(PointerEvent::Move {
            position: PointF::new(400.0, 300.0),
            modifiers: Modifiers::NONE,
        });
        assert_eq!(chrome.tree.take_messages(), [], "let go");
    }

    #[test]
    fn switching_layouts_keeps_the_page_where_the_layout_puts_it() {
        let mut chrome = chrome(UiState {
            layout: UiLayout::Horizontal,
            ..state()
        });
        assert!(chrome.tab_strip.is_some());
        let horizontal = chrome.web_views(1.0);
        chrome.update(state());
        assert!(chrome.tab_strip.is_none());
        let sidebar = chrome.web_views(1.0);
        assert_eq!(horizontal, [(TabId(2), Rect::new(0, 74, 800, 526))]);
        assert_eq!(sidebar, [(TabId(2), Rect::new(268, 8, 524, 584))]);
        chrome.tree.paint();
    }

    /// `count` tabs, the one at `active` shown, in the sidebar of an
    /// 800 × 600 window: the rows (36 high, 2 apart) from 126, the list
    /// down to 592, 466 high.
    fn many(count: u32, active: usize) -> UiState {
        let tabs: Vec<TabId> = (1..=count).map(TabId).collect();
        UiState {
            titles: tabs.iter().map(|tab| format!("tab {}", tab.0)).collect(),
            shown: vec![tabs[active]],
            tabs,
            active,
            ..state()
        }
    }

    fn scrolled(chrome: &Chrome) -> f32 {
        let list = chrome.tab_list.unwrap();
        chrome.tree.get(list).unwrap().offset().y
    }

    fn wheel(chrome: &mut Chrome, at: PointF, dy: f32) {
        chrome.tree.pointer(PointerEvent::Wheel {
            position: at,
            delta: PointF::new(0.0, dy),
            modifiers: Modifiers::NONE,
        });
    }

    #[test]
    fn the_rows_scroll_in_a_layer_of_their_own_under_what_stays() {
        let mut chrome = chrome(many(20, 0));
        let list = chrome.tab_list.unwrap();
        assert_eq!(rect(&chrome, list), RectF::new(8.0, 126.0, 244.0, 466.0));
        let field = rect(&chrome, chrome.omnibox);
        wheel(&mut chrome, PointF::new(100.0, 300.0), 100.0);
        assert_eq!(scrolled(&chrome), 100.0);
        let frame = chrome.tree.paint().unwrap();
        let [_, rows] = &frame.layers[..] else {
            panic!("the tree's layer and the rows'")
        };
        assert_eq!(rows.owner, Some(list.any()));
        // 20 rows of 36, 2 apart; moved up by the scroll, cut to the list.
        assert_eq!(rows.size, SizeF::new(244.0, 758.0));
        assert_eq!(rows.origin, PointF::new(8.0, 26.0));
        assert_eq!(rows.clip, Some(RectF::new(8.0, 126.0, 244.0, 466.0)));
        // The field and the buttons stay.
        assert_eq!(rect(&chrome, chrome.omnibox), field);
        // No further than the last row: 758 in 466.
        wheel(&mut chrome, PointF::new(100.0, 300.0), 1000.0);
        assert_eq!(scrolled(&chrome), 292.0);
        // Over the field the wheel scrolls nothing.
        wheel(&mut chrome, center(field), -50.0);
        assert_eq!(scrolled(&chrome), 292.0);
    }

    #[test]
    fn a_row_scrolled_to_is_the_one_clicked() {
        let mut chrome = chrome(many(20, 0));
        wheel(&mut chrome, PointF::new(100.0, 300.0), 100.0);
        chrome.tree.take_messages();
        // 126 + 18 down the window, 100 + 18 down the list: the fourth row
        // (114 to 150).
        click(
            &mut chrome,
            PointF::new(100.0, 126.0 + 18.0),
            PointerButton::Primary,
        );
        assert_eq!(
            chrome.tree.take_messages(),
            [UiMessage::SelectTabById(TabId(4))]
        );
    }

    #[test]
    fn the_active_tab_s_row_is_scrolled_to_when_another_is_shown() {
        // The last shown: its row's bottom at the list's.
        let mut chrome = chrome(many(20, 19));
        assert_eq!(scrolled(&chrome), 292.0);
        // The first: its top at the top.
        chrome.update(many(20, 0));
        assert_eq!(scrolled(&chrome), 0.0);
        // The eleventh (380 to 416) shows already: nothing moves.
        chrome.update(many(20, 10));
        assert_eq!(scrolled(&chrome), 0.0);
        // A new title isn't another tab: the list stays where it was put.
        wheel(&mut chrome, PointF::new(100.0, 300.0), 40.0);
        let mut renamed = many(20, 10);
        renamed.titles[3] = "renamed".into();
        chrome.update(renamed);
        assert_eq!(scrolled(&chrome), 40.0);
        // Back in the sidebar after the horizontal layout: a new list,
        // scrolled to the tab.
        chrome.update(UiState {
            layout: UiLayout::Horizontal,
            ..many(20, 19)
        });
        assert!(chrome.tab_list.is_none());
        chrome.update(many(20, 19));
        assert_eq!(scrolled(&chrome), 292.0);
    }

    #[test]
    fn closing_tabs_keeps_the_list_within_its_rows() {
        let mut chrome = chrome(many(20, 19));
        assert_eq!(scrolled(&chrome), 292.0);
        // The first ten closed, the same tab shown (so not scrolled to):
        // 378 of rows in 466, nothing to scroll.
        let mut fewer = many(20, 19);
        fewer.tabs.drain(..10);
        fewer.titles.drain(..10);
        fewer.active = 9;
        chrome.update(fewer);
        chrome.tree.layout();
        assert_eq!(scrolled(&chrome), 0.0);
    }

    #[test]
    fn collapsed_the_page_takes_the_window_and_the_sidebar_floats_over_it() {
        let collapsed = UiState {
            sidebar_collapsed: true,
            ..state()
        };
        let mut chrome = chrome(collapsed.clone());
        // Hidden: the page a card in the whole window.
        assert_eq!(chrome.floating_panel(), None);
        assert_eq!(
            chrome.web_views(1.0),
            [(TabId(2), Rect::new(8, 8, 784, 584))]
        );
        assert_eq!(chrome.web_view_radius(TabId(2)), 8.0);
        assert_eq!(chrome.tree.hit_test(PointF::new(100.0, 144.0)), None);
        // Floating: over the page, which stays; a layer of its own, the
        // list's in it.
        let field = chrome.omnibox;
        chrome.update(UiState {
            sidebar_floating: true,
            ..collapsed.clone()
        });
        let panel = chrome.floating_panel().unwrap();
        assert_eq!(
            chrome.web_views(1.0),
            [(TabId(2), Rect::new(8, 8, 784, 584))]
        );
        assert_eq!(rect(&chrome, panel), RectF::new(0.0, 0.0, 260.0, 600.0));
        let row = rows(&chrome)[0];
        assert_eq!(
            chrome
                .tree
                .hit_test(PointF::new(100.0, 144.0))
                .map(|(id, _)| id),
            Some(row.any())
        );
        let frame = chrome.tree.paint().unwrap();
        let owners: Vec<_> = frame.layers.iter().map(|layer| layer.owner).collect();
        let list = chrome.tab_list.unwrap();
        assert_eq!(owners, [None, Some(panel.any()), Some(list.any())]);
        assert!(chrome.tree.is_within(list, panel));
        // The same field, hidden, floated and expanded.
        assert_eq!(chrome.omnibox, field);
        chrome.update(collapsed);
        assert_eq!(chrome.floating_panel(), None);
        chrome.update(state());
        assert_eq!(chrome.omnibox, field);
        assert_eq!(chrome.floating_panel(), None);
        assert_eq!(
            chrome.tree.paint().unwrap().layers.len(),
            2,
            "the tree's and the list's"
        );
    }
}
