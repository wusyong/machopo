//! `BrowserView`: what a browser window shows, the tab strip, the toolbar
//! and the web views, and where its input goes. Its tabs are a
//! `TabStripModel`; the first web view shows the active one. A
//! `BrowserViewLayout` gives each its bounds; drawing, the renderers' sizes
//! and hit testing all follow them. The browser draws through its own
//! compositor (Chromium's `ui::Compositor`, a `cc::LayerTreeHost`): the tab
//! strip and the toolbar are the root layer, and each web view's surface is
//! a `SurfaceLayer`, which Viz fills with that renderer's frames. A new
//! frame from a renderer needs nothing from the browser.
//!
//! Kept apart from winit (`window` translates its events), so tests can
//! drive a window's worth of browser with plain calls.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use browser::{ContentsEvent, OpenUrlParams, WebContents};
use compositor::mojo_embedder::ChannelLayerTreeFrameSink;
use compositor::{
    LayerId, LayerProperties, LayerTreeHost, LayerTreeSettings, PictureLayer, PropertyTrees,
    SingleThreadProxy, SurfaceLayer,
};
use compositor_paint::DisplayItemList;
use crossbeam_channel::Sender;
use gfx::{Color, PointF, Rect, RectF, Size, SizeF};
use protocol::TabId;
use protocol::input::{InputEvent, Modifiers, MouseButton, ScrollDelta};
use protocol::network::RedirectInfo;
use protocol::renderer::{CursorType, PopupMenuResult, RendererHostMsg, WindowOpenDisposition};
use protocol::viz::{
    CompositorFrameSinkSender, FrameSinkId, LocalSurfaceId, SharedImageInterface, VizMsg,
};
use std::sync::Arc;
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

use crate::browser_view_layout::{BrowserViewLayout, DefaultLayout};
use crate::keyboard::{self, KeyInput};
use crate::omnibox::{Clipboard, KeyResult};
use crate::popup_menu::{KeyOutcome, PopupMenu};
use crate::sad_tab::{self, SadTab};
use crate::tab_strip::{self, TabStrip};
use crate::tab_strip_model::TabStripModel;
use crate::toolbar::{self, Button, Part, Toolbar};
use crate::url_fixup::fixup_url;

/// The browser's own frame sink: the root of a window's `Display`.
pub const BROWSER_FRAME_SINK: FrameSinkId = FrameSinkId {
    client_id: 0,
    sink_id: 1,
};

/// The CSS keyword for `cursor` (`all-scroll` looks as `move` does).
pub fn cursor_name(cursor: CursorType) -> &'static str {
    use CursorType as C;
    match cursor {
        C::Pointer => "default",
        C::Cross => "crosshair",
        C::Hand => "pointer",
        C::IBeam => "text",
        C::Wait => "wait",
        C::Help => "help",
        C::EastResize => "e-resize",
        C::NorthResize => "n-resize",
        C::NorthEastResize => "ne-resize",
        C::NorthWestResize => "nw-resize",
        C::SouthResize => "s-resize",
        C::SouthEastResize => "se-resize",
        C::SouthWestResize => "sw-resize",
        C::WestResize => "w-resize",
        C::NorthSouthResize => "ns-resize",
        C::EastWestResize => "ew-resize",
        C::NorthEastSouthWestResize => "nesw-resize",
        C::NorthWestSouthEastResize => "nwse-resize",
        C::ColumnResize => "col-resize",
        C::RowResize => "row-resize",
        C::Move => "move",
        C::VerticalText => "vertical-text",
        C::Cell => "cell",
        C::ContextMenu => "context-menu",
        C::Alias => "alias",
        C::Progress => "progress",
        C::NoDrop => "no-drop",
        C::Copy => "copy",
        C::None => "none",
        C::NotAllowed => "not-allowed",
        C::ZoomIn => "zoom-in",
        C::ZoomOut => "zoom-out",
        C::Grab => "grab",
        C::Grabbing => "grabbing",
    }
}

/// What a browser shortcut or button does (Chrome's commands, in
/// `chrome/browser/ui`): these never reach the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Back,
    Forward,
    Reload,
    /// Focus the address field, everything selected.
    FocusLocation,
    /// A new tab after the others, shown.
    NewTab,
    /// Close the active tab; the last one closes the window.
    CloseTab,
    /// Show the tab after the active one, the first after the last.
    NextTab,
    /// Show the tab before the active one, the last before the first.
    PreviousTab,
    /// Show the tab at this index, if there is one.
    SelectTab(usize),
    SelectLastTab,
}

/// The shortcut `key` is, with `modifiers` held, on macOS (`mac`) or
/// elsewhere:
/// - back / forward: Cmd+[ / Cmd+] and Cmd+← / Cmd+→ on macOS (the arrows
///   only outside the address field, where they move the caret), Alt+← /
///   Alt+→ elsewhere;
/// - reload: Cmd+R / Ctrl+R, and F5;
/// - the address field: Cmd+L / Ctrl+L, Alt+D, F6;
/// - tabs: Cmd+T / Ctrl+T opens one, Cmd+W / Ctrl+W closes one; Ctrl+Tab
///   and Ctrl+Shift+Tab show the next and the previous, as do
///   Cmd+Option+→ / ← and Cmd+Shift+] / [ on macOS, Ctrl+PageDown /
///   PageUp elsewhere; Cmd+1 to 8 / Ctrl+1 to 8 show that tab, 9 the last.
///
/// The crate's README.md lists them for users ("鍵盤和滑鼠"); keep it
/// in step.
pub fn command_for(
    key: &Key,
    modifiers: ModifiersState,
    mac: bool,
    in_field: bool,
) -> Option<Command> {
    let primary = if mac {
        modifiers.super_key()
    } else {
        modifiers.control_key()
    };
    let history = if mac {
        modifiers.super_key() && !in_field
    } else {
        modifiers.alt_key()
    };
    let ctrl = modifiers.control_key();
    let shift = modifiers.shift_key();
    let mac_tabs = mac && modifiers.super_key() && modifiers.alt_key();
    match key {
        Key::Named(NamedKey::F5) => Some(Command::Reload),
        Key::Named(NamedKey::F6) => Some(Command::FocusLocation),
        Key::Named(NamedKey::Tab) if ctrl && shift => Some(Command::PreviousTab),
        Key::Named(NamedKey::Tab) if ctrl => Some(Command::NextTab),
        Key::Named(NamedKey::PageUp) if ctrl && !mac => Some(Command::PreviousTab),
        Key::Named(NamedKey::PageDown) if ctrl && !mac => Some(Command::NextTab),
        Key::Named(NamedKey::ArrowLeft) if mac_tabs => Some(Command::PreviousTab),
        Key::Named(NamedKey::ArrowRight) if mac_tabs => Some(Command::NextTab),
        Key::Named(NamedKey::ArrowLeft) if history => Some(Command::Back),
        Key::Named(NamedKey::ArrowRight) if history => Some(Command::Forward),
        // winit's logical key ignores Ctrl, so this is the letter. Shift
        // makes [ and ] the braces on most layouts.
        Key::Character(c) => match c.to_lowercase().as_str() {
            "r" if primary => Some(Command::Reload),
            "l" if primary => Some(Command::FocusLocation),
            "d" if modifiers.alt_key() && !mac => Some(Command::FocusLocation),
            "t" if primary => Some(Command::NewTab),
            "w" if primary => Some(Command::CloseTab),
            "{" if primary && mac => Some(Command::PreviousTab),
            "}" if primary && mac => Some(Command::NextTab),
            "[" if primary && mac && shift => Some(Command::PreviousTab),
            "]" if primary && mac && shift => Some(Command::NextTab),
            "[" if primary && mac => Some(Command::Back),
            "]" if primary && mac => Some(Command::Forward),
            "9" if primary => Some(Command::SelectLastTab),
            digit @ ("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8") if primary => {
                // 1 to 8: the first to the eighth.
                Some(Command::SelectTab(
                    digit.parse::<usize>().ok()?.checked_sub(1)?,
                ))
            }
            _ => None,
        },
        _ => None,
    }
}

/// What a point in the window is over.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    TabStrip,
    Toolbar,
    /// The web view showing this tab.
    WebView(TabId),
}

/// `views::WebView`, with where its `RenderWidgetHostViewAura` is: shows
/// one of the view's web contents. Its bounds are the one place the page's
/// position comes from: the size its renderer is told, where its surface
/// is drawn, and the point its input is at.
struct WebView {
    contents: TabId,
    /// Physical px of the window.
    bounds: Rect,
}

/// Makes the web contents of new tabs and takes back closed ones
/// (Chromium's `Browser`, which makes tabs with the window's profile; the
/// window's makes their frame sinks known to Viz, as
/// `HostFrameSinkManager` does, and forgets them).
pub trait TabFactory {
    fn create(&mut self, tab: TabId) -> Result<WebContents, String>;
    /// Shuts `contents` down: its tab was closed.
    fn destroy(&mut self, contents: WebContents);
}

pub struct BrowserView {
    tabs: TabStripModel<WebContents>,
    /// What the web views after the first show (as Chromium's docked
    /// DevTools are): not tabs.
    others: Vec<WebContents>,
    /// Without one, no new tab opens.
    factory: Option<Box<dyn TabFactory>>,
    /// The first shows the active tab.
    web_views: Vec<WebView>,
    /// The cursor each renderer last asked for (each
    /// `RenderWidgetHostView`'s).
    cursors: HashMap<TabId, CursorType>,
    /// The tab each tab opened from a link was opened from, while both are
    /// open (`TabStripModel`'s openers).
    openers: HashMap<TabId, TabId>,
    /// No tab id below it is given again.
    next_tab: u32,
    /// The last tab was closed: the window closes, and its tab with it.
    close_requested: bool,
    layout: Box<dyn BrowserViewLayout>,
    /// Physical px of the window.
    toolbar_bounds: Rect,
    /// Physical px of the window.
    tab_strip_bounds: Rect,
    /// `ui::Compositor`.
    compositor: LayerTreeHost<SingleThreadProxy>,
    tab_strip: TabStrip,
    toolbar: Toolbar,
    /// What a web view whose renderer has gone shows.
    sad_tab: SadTab,
    /// Where sad tabs were last drawn, physical px.
    sad_drawn: Vec<Rect>,
    /// The tab strip or the toolbar shows differently: they are painted
    /// again (Chromium's `TopContainerView`, one layer).
    chrome_dirty: bool,
    /// Physical px.
    size: Size,
    scale: f32,
    /// Where the window is on the screen, CSS px.
    origin: PointF,
    local_surface_id: LocalSurfaceId,
    /// CSS px of the window.
    pointer: PointF,
    modifiers: ModifiersState,
    /// Where the primary button went down: it gets the moves and the up.
    captured: Option<Target>,
    /// The web view keys go to while the address field doesn't have them:
    /// the last one a button went down in (aura's `FocusClient`).
    focused: TabId,
    /// The physical keys that went down in the page and are still down:
    /// their key-ups go there too.
    keys_down_in_page: HashSet<PhysicalKey>,
    /// The mouse buttons down (the DOM's `buttons` bits), and the click
    /// count of each one's last press (the primary, the middle and the
    /// secondary), which its release has too.
    buttons: u16,
    click_counts: [u32; 3],
    /// The window is the active one (`Widget::IsActive`); a view with no
    /// platform window (headless, the scenarios') counts as one.
    window_focused: bool,
    clipboard: Box<dyn Clipboard>,
    mac: bool,
    /// What relative paths typed in the address field are from.
    cwd: PathBuf,
    /// A select's dropdown, over everything (`popup_menu`): it has the
    /// mouse over it and every key while it is open.
    popup: Option<PopupMenu>,
    /// The dropdown shows differently: the layers are made again.
    popup_dirty: bool,
    /// The primary button went down in the dropdown: its up is the
    /// dropdown's too.
    pressed_in_popup: bool,
    test_fonts: bool,
}

impl BrowserView {
    /// `viz` takes the browser's frames, for `BROWSER_FRAME_SINK`.
    /// `test_fonts` as for the renderer: the toolbar's text too.
    /// `gpu` is where the toolbar's tiles go on the GPU Viz draws with;
    /// with `None` they go to Viz as pixels, which it uploads.
    pub fn new(
        web_contents: WebContents,
        viz: Sender<VizMsg>,
        gpu: Option<Arc<dyn SharedImageInterface>>,
        test_fonts: bool,
        clipboard: Box<dyn Clipboard>,
    ) -> Self {
        let frame_sink =
            ChannelLayerTreeFrameSink::new(CompositorFrameSinkSender::new(BROWSER_FRAME_SINK, viz));
        let tab = web_contents.tab();
        Self {
            tabs: TabStripModel::new(web_contents),
            others: Vec::new(),
            factory: None,
            web_views: vec![WebView::new(tab)],
            cursors: HashMap::new(),
            openers: HashMap::new(),
            next_tab: 0,
            close_requested: false,
            layout: Box::new(DefaultLayout),
            toolbar_bounds: Rect::default(),
            tab_strip_bounds: Rect::default(),
            compositor: LayerTreeHost::new_single_threaded(
                LayerTreeSettings {
                    shared_image_interface: gpu,
                    ..LayerTreeSettings::default()
                },
                Box::new(frame_sink),
            ),
            tab_strip: TabStrip::new(test_fonts),
            toolbar: Toolbar::new(test_fonts),
            sad_tab: SadTab::new(test_fonts),
            sad_drawn: Vec::new(),
            chrome_dirty: true,
            size: Size::new(0, 0),
            scale: 1.0,
            origin: PointF::new(0.0, 0.0),
            local_surface_id: LocalSurfaceId::default(),
            pointer: PointF::new(0.0, 0.0),
            modifiers: ModifiersState::empty(),
            captured: None,
            focused: tab,
            window_focused: true,
            keys_down_in_page: HashSet::new(),
            buttons: 0,
            click_counts: [1; 3],
            clipboard,
            mac: cfg!(target_os = "macos"),
            cwd: std::env::current_dir().unwrap_or_default(),
            popup: None,
            popup_dirty: false,
            pressed_in_popup: false,
            test_fonts,
        }
    }

    /// The dropdown shown, if any.
    pub fn popup_menu(&self) -> Option<&PopupMenu> {
        self.popup.as_ref()
    }

    /// Closes the dropdown, if one is shown, and tells its renderer how
    /// (Chromium's `PopupMenuHelper`).
    fn close_popup(&mut self, result: PopupMenuResult) {
        let Some(popup) = self.popup.take() else {
            return;
        };
        self.popup_dirty = true;
        self.pressed_in_popup = false;
        if let Some(contents) = self.contents_mut(popup.tab()) {
            contents.popup_menu_closed(result);
        }
    }

    /// Editing and shortcut keys as on macOS or not; the platform's by
    /// default.
    pub fn set_mac(&mut self, mac: bool) {
        self.mac = mac;
    }

    /// Where new tabs come from.
    pub fn set_tab_factory(&mut self, factory: Box<dyn TabFactory>) {
        self.factory = Some(factory);
    }

    /// The active tab's.
    pub fn web_contents(&self) -> &WebContents {
        self.tabs.active()
    }

    /// The web contents of `tab`, a tab's or another web view's.
    pub fn contents(&self, tab: TabId) -> Option<&WebContents> {
        self.tabs
            .iter()
            .chain(&self.others)
            .find(|contents| contents.tab() == tab)
    }

    pub fn contents_mut(&mut self, tab: TabId) -> Option<&mut WebContents> {
        self.tabs
            .iter_mut()
            .chain(&mut self.others)
            .find(|contents| contents.tab() == tab)
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.count()
    }

    /// The tabs' web contents, in the tab strip's order.
    pub fn tabs(&self) -> impl Iterator<Item = &WebContents> {
        self.tabs.iter()
    }

    /// Does `act` to the web contents of `tab` (any tab, not only the
    /// active one), then shows what changed, as the view's own commands
    /// do; `None` if there is no such tab.
    pub fn update_contents<R>(
        &mut self,
        tab: TabId,
        act: impl FnOnce(&mut WebContents) -> R,
    ) -> Option<R> {
        let result = act(self.contents_mut(tab)?);
        self.sync_toolbar();
        // A debug URL may have changed what the tab shows (`about:crash`).
        self.update();
        Some(result)
    }

    pub fn active_tab_index(&self) -> usize {
        self.tabs.active_index()
    }

    /// The last tab was closed: the window should close (Chromium closes
    /// the window, which then closes its last tab).
    pub fn close_requested(&self) -> bool {
        self.close_requested
    }

    /// A tab id no web contents of the view has, nor had: a closed tab's
    /// renderer may still have messages on their way, which mustn't reach
    /// a new tab (ids only grow, as Chromium's).
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "one id per tab opened in the window: fewer than u32::MAX"
    )]
    fn next_tab_id(&mut self) -> TabId {
        let next = self.tabs.iter().chain(&self.others).map(|c| c.tab().0 + 1);
        let tab = TabId(next.max().unwrap_or(0).max(self.next_tab));
        self.next_tab = tab.0 + 1;
        tab
    }

    /// `chrome::NewTab`: `about:blank` after the other tabs, shown, the
    /// address field focused (Chrome opens its new tab page).
    fn new_tab(&mut self) {
        let tab = self.next_tab_id();
        let Some(factory) = &mut self.factory else {
            return;
        };
        let mut contents = match factory.create(tab) {
            Ok(contents) => contents,
            Err(e) => {
                tracing::warn!("couldn't open a tab: {e}");
                return;
            }
        };
        contents.navigate("about:blank");
        let index = self.tabs.append(contents);
        self.activate_tab(index);
        self.toolbar.omnibox.focus();
        self.chrome_dirty = true;
    }

    /// `TabStripModel::ActivateTabAt`: shows the tab at `index`, if there
    /// is one, and hides the one shown before.
    fn activate_tab(&mut self, index: usize) {
        let before = self.tabs.active_index();
        if !self.tabs.activate(index) {
            return;
        }
        if let Some(hidden) = self.tabs.iter_mut().nth(before) {
            hidden.was_hidden();
        }
        self.show_active_tab();
    }

    /// `BrowserView::OnActiveTabChanged`: the first web view shows the
    /// active tab, which has the keys; the address field follows it.
    fn show_active_tab(&mut self) {
        self.close_popup(PopupMenuResult::Canceled { by_press: false });
        let tab = self.tabs.active().tab();
        self.tabs.active_mut().was_shown();
        if let Some(Target::WebView(captured)) = self.captured
            && captured == self.web_views[0].contents
        {
            self.captured = None;
        }
        self.web_views[0].contents = tab;
        self.focused = tab;
        if self.toolbar.omnibox.is_focused() {
            self.toolbar.omnibox.blur();
        }
        self.chrome_dirty = true;
        self.relayout();
        self.sync_toolbar();
        self.update();
    }

    /// `Browser::OpenURLFromTab`: a link `opener` followed to a new tab.
    /// It goes right of the opener, after the tabs opened from it before
    /// (`TabStripModel::AddWebContents`); in front, or behind, hidden and
    /// as big as the tab shown.
    fn open_from(&mut self, opener: TabId, params: OpenUrlParams) {
        let Some(opener_index) = self.tabs.position(|c| c.tab() == opener) else {
            return;
        };
        let tab = self.next_tab_id();
        let Some(factory) = &mut self.factory else {
            return;
        };
        let mut contents = match factory.create(tab) {
            Ok(contents) => contents,
            Err(e) => {
                tracing::warn!("couldn't open a tab: {e}");
                return;
            }
        };
        // A tab opened behind is hidden before it shows anything
        // (Chromium's `WebContents::CreateParams::initially_hidden`): its
        // first document starts hidden.
        let foreground = params.disposition == WindowOpenDisposition::NewForegroundTab;
        if !foreground {
            contents.was_hidden();
        }
        contents.navigate(&params.url);
        // After the opener and the tabs it opened before: each index one
        // of a tab, or one past the last.
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "index is a tab's at most, below a Vec's length"
        )]
        let index = {
            let mut index = opener_index + 1;
            while self
                .tabs
                .iter()
                .nth(index)
                .is_some_and(|c| self.openers.get(&c.tab()) == Some(&opener))
            {
                index += 1;
            }
            index
        };
        self.openers.insert(tab, opener);
        if foreground {
            let index = self.tabs.insert(index, contents);
            self.activate_tab(index);
        } else {
            self.insert_hidden(index, contents);
        }
    }

    /// Puts `contents`, hidden, in the tab strip at `index` behind the tab
    /// shown, as big as the tab shown.
    fn insert_hidden(&mut self, index: usize, mut contents: WebContents) {
        let bounds = self.web_views[0].bounds;
        if !bounds.is_empty() {
            contents.resize(bounds.size(), self.scale);
        }
        if let Some(rect) = self.window_screen_rect() {
            contents.set_window_screen_rect(rect);
        }
        self.tabs.insert(index, contents);
    }

    /// A new tab after the others showing `url`, for what drives the
    /// browser (DevTools' `Target.createTarget`, which Chromium's
    /// `TargetHandler` opens with `chrome::AddTabAt`): shown, or behind the
    /// tab shown if `background`. `about:blank` is the initial entry, which
    /// the first navigation takes the place of, as on a new tab. `None` if
    /// no tab can be made.
    pub fn open_tab(&mut self, url: &str, background: bool) -> Option<TabId> {
        let tab = self.next_tab_id();
        let factory = self.factory.as_mut()?;
        let mut contents = match factory.create(tab) {
            Ok(contents) => contents,
            Err(e) => {
                tracing::warn!("couldn't open a tab: {e}");
                return None;
            }
        };
        // Hidden before it shows anything, as `open_from`'s.
        if background {
            contents.was_hidden();
        }
        if url == "about:blank" {
            contents.navigate_initial(url);
        } else {
            contents.navigate(url);
        }
        if background {
            self.insert_hidden(self.tabs.count(), contents);
        } else {
            let index = self.tabs.append(contents);
            self.activate_tab(index);
        }
        self.sync_toolbar();
        self.update();
        Some(tab)
    }

    /// Closes `tab` as its close button does (`Target.closeTarget`): the
    /// last one closes the window. Whether there was such a tab.
    pub fn close(&mut self, tab: TabId) -> bool {
        let Some(index) = self.tabs.position(|contents| contents.tab() == tab) else {
            return false;
        };
        self.close_tab(index);
        self.sync_toolbar();
        self.update();
        true
    }

    /// Shows `tab` as clicking it in the tab strip does
    /// (`Target.activateTarget`). Whether there is such a tab.
    pub fn activate(&mut self, tab: TabId) -> bool {
        let Some(index) = self.tabs.position(|contents| contents.tab() == tab) else {
            return false;
        };
        self.activate_tab(index);
        self.sync_toolbar();
        self.update();
        true
    }

    /// `TabStripModel::CloseWebContentsAt`: the last tab closes the window
    /// instead.
    fn close_tab(&mut self, index: usize) {
        if self.tabs.count() == 1 {
            self.close_requested = true;
            return;
        }
        let was_active = index == self.tabs.active_index();
        let Some(contents) = self.tabs.close(index) else {
            return;
        };
        let tab = contents.tab();
        self.cursors.remove(&tab);
        // Back to the tab it was opened from, if it still is
        // (`DetermineNewSelectedIndex`); the tabs it opened forget it.
        let opener = self.openers.remove(&tab);
        self.openers.retain(|_, from| *from != tab);
        if was_active
            && let Some(opener) = opener
            && let Some(index) = self.tabs.position(|c| c.tab() == opener)
        {
            self.tabs.activate(index);
        }
        if self.captured == Some(Target::WebView(tab)) {
            self.captured = None;
        }
        match &mut self.factory {
            Some(factory) => factory.destroy(contents),
            None => {
                let mut contents = contents;
                contents.shutdown();
            }
        }
        if was_active {
            self.show_active_tab();
        }
    }

    /// Whether `tab`'s renderer has gone: its web view shows the sad tab,
    /// and no input goes to it.
    fn crashed(&self, tab: TabId) -> bool {
        self.contents(tab).is_some_and(WebContents::is_crashed)
    }

    fn web_view(&self, tab: TabId) -> Option<&WebView> {
        self.web_views.iter().find(|view| view.contents == tab)
    }

    pub fn toolbar(&self) -> &Toolbar {
        &self.toolbar
    }

    pub fn tab_strip(&self) -> &TabStrip {
        &self.tab_strip
    }

    /// Lays the window out with `layout` from now on.
    pub fn set_layout(&mut self, layout: Box<dyn BrowserViewLayout>) {
        self.layout = layout;
        self.relayout();
        self.update();
    }

    /// Shows `web_contents` too, in a web view of its own after the others.
    /// Its frames go to Viz like the tab's: the caller has Viz send it
    /// `BeginFrame`s.
    pub fn add_web_view(&mut self, web_contents: WebContents) {
        self.web_views.push(WebView::new(web_contents.tab()));
        self.others.push(web_contents);
        self.relayout();
        self.update();
    }

    /// Stops showing `tab`, which isn't a tab but another web view's: its
    /// web contents, for the caller to shut down. The pointer's capture
    /// goes with it, and the focus goes back to the active tab.
    pub fn remove_web_view(&mut self, tab: TabId) -> Option<WebContents> {
        let index = self.others.iter().position(|c| c.tab() == tab)?;
        self.web_views.retain(|view| view.contents != tab);
        let contents = self.others.remove(index);
        self.cursors.remove(&tab);
        if self.captured == Some(Target::WebView(tab)) {
            self.captured = None;
        }
        if self.focused == tab {
            self.focused = self.tabs.active().tab();
        }
        self.relayout();
        self.update();
        Some(contents)
    }

    /// In the active tab, as if typed in the address field: the field
    /// lets go of the keys and shows where the tab goes.
    pub fn navigate(&mut self, url: &str) {
        self.tabs.active_mut().navigate(url);
        if self.toolbar.omnibox.is_focused() {
            self.toolbar.omnibox.blur();
            self.chrome_dirty = true;
        }
        self.sync_toolbar();
        // A debug URL may have changed what the tab shows (`about:crash`).
        self.update();
    }

    /// The window is `size` physical px at `scale`.
    pub fn resize(&mut self, size: Size, scale: f32) {
        if (size, scale) == (self.size, self.scale) {
            return;
        }
        self.size = size;
        self.scale = scale;
        self.close_popup(PopupMenuResult::Canceled { by_press: false });
        self.relayout();
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "one per resize: 2^32 of them at 60 a second take two years of resizing"
        )]
        {
            self.local_surface_id.parent_sequence_number += 1;
        }
        self.compositor
            .set_viewport_rect_and_scale(size, scale, self.local_surface_id);
        self.update();
    }

    /// Puts the tab strip, the toolbar and the web views where the layout
    /// says, and tells
    /// each renderer its size (`RenderWidgetHostViewAura::SetBounds`).
    fn relayout(&mut self) {
        let layout = self
            .layout
            .layout(self.size, self.scale, self.web_views.len());
        assert_eq!(layout.web_views.len(), self.web_views.len());
        // Painted again even at the same bounds: the scale may be new.
        self.toolbar_bounds = layout.toolbar;
        self.tab_strip_bounds = layout.tab_strip;
        self.chrome_dirty = true;
        self.toolbar
            .set_width(layout.toolbar.width as f32 / self.scale);
        self.tab_strip
            .set_width(layout.tab_strip.width as f32 / self.scale);
        for (view, bounds) in self.web_views.iter_mut().zip(layout.web_views) {
            view.bounds = bounds;
            if bounds.is_empty() {
                continue;
            }
            if let Some(contents) = self
                .tabs
                .iter_mut()
                .chain(&mut self.others)
                .find(|c| c.tab() == view.contents)
            {
                contents.resize(bounds.size(), self.scale);
            }
        }
        self.send_screen_rects();
    }

    /// The window moved (the platform's, or as what drives the browser
    /// asked): where it is on the screen now, CSS px.
    pub fn move_to(&mut self, origin: PointF) {
        self.origin = origin;
        self.send_screen_rects();
    }

    /// Where the window is on the screen, CSS px.
    pub fn origin(&self) -> PointF {
        self.origin
    }

    /// The window's rect on the screen, CSS px, once it has a size.
    fn window_screen_rect(&self) -> Option<RectF> {
        (!self.size.is_empty()).then(|| {
            RectF::new(
                self.origin.x,
                self.origin.y,
                self.size.width as f32 / self.scale,
                self.size.height as f32 / self.scale,
            )
        })
    }

    /// Tells every web contents' renderer where the window is and how big
    /// (`RenderWidgetHostViewAura::OnBoundsChanged` sending screen rects).
    fn send_screen_rects(&mut self) {
        let Some(rect) = self.window_screen_rect() else {
            return;
        };
        for contents in self.tabs.iter_mut().chain(&mut self.others) {
            contents.set_window_screen_rect(rect);
        }
    }

    /// What `point` (CSS px of the window) is over, and where it is in it:
    /// the web views from the top one down, then the tab strip and the
    /// toolbar (aura's `WindowTargeter`).
    fn hit_test(&self, point: PointF) -> Option<(Target, PointF)> {
        let at = PointF::new(point.x * self.scale, point.y * self.scale);
        let target = self
            .web_views
            .iter()
            .rev()
            .find(|view| view.bounds.to_f32().contains(at))
            .map(|view| Target::WebView(view.contents))
            .or_else(|| {
                [
                    (self.tab_strip_bounds, Target::TabStrip),
                    (self.toolbar_bounds, Target::Toolbar),
                ]
                .into_iter()
                .find(|(bounds, _)| bounds.to_f32().contains(at))
                .map(|(_, target)| target)
            })?;
        Some((target, self.in_target(target, point)?))
    }

    /// The bounds of `target`, if it's still there.
    fn bounds_of(&self, target: Target) -> Option<Rect> {
        match target {
            Target::TabStrip => Some(self.tab_strip_bounds),
            Target::Toolbar => Some(self.toolbar_bounds),
            Target::WebView(tab) => self.web_view(tab).map(|view| view.bounds),
        }
    }

    /// `point`, CSS px of the window, in `target`'s CSS px (aura's
    /// `ConvertLocationToTarget`): inside it or not, as for a captured one.
    fn in_target(&self, target: Target, point: PointF) -> Option<PointF> {
        let bounds = self.bounds_of(target)?;
        Some(PointF::new(
            (point.x * self.scale - bounds.x as f32) / self.scale,
            (point.y * self.scale - bounds.y as f32) / self.scale,
        ))
    }

    /// Where the pointer's events go: what has captured it, or else what
    /// it is over.
    fn pointer_target(&self) -> Option<(Target, PointF)> {
        match self.captured {
            Some(target) => Some((target, self.in_target(target, self.pointer)?)),
            None => self.hit_test(self.pointer),
        }
    }

    /// Where `tab`'s web view is, CSS px of the window.
    pub fn web_view_bounds(&self, tab: TabId) -> Option<RectF> {
        Some(self.web_view(tab)?.bounds.to_f32().scale(1.0 / self.scale))
    }

    /// `rect` in `tab`'s CSS px, in the window's: where its web view shows
    /// it (the other way from `in_target`).
    pub fn web_view_to_window(&self, tab: TabId, rect: RectF) -> Option<RectF> {
        let bounds = self.web_view_bounds(tab)?;
        Some(rect.offset(bounds.x, bounds.y))
    }

    /// Device pixels per CSS px.
    /// The window's size, physical px.
    pub fn size(&self) -> Size {
        self.size
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    pub fn set_modifiers(&mut self, modifiers: ModifiersState) {
        self.modifiers = modifiers;
    }

    /// The window became the active one, or stopped being it.
    pub fn set_window_focused(&mut self, focused: bool) {
        self.window_focused = focused;
        if !focused {
            self.close_popup(PopupMenuResult::Canceled { by_press: false });
        }
        self.update();
    }

    /// Which page has the system's focus (Chromium's
    /// `RenderWidgetHostImpl::SetFocus`, which `WebContentsImpl` calls as
    /// views take and lose the focus): the web view that has the keys, if
    /// the window is active and the address field doesn't have them.
    fn sync_focus(&mut self) {
        let focused =
            (self.window_focused && !self.toolbar.omnibox.is_focused()).then_some(self.focused);
        for contents in self.tabs.iter_mut().chain(self.others.iter_mut()) {
            contents.set_focus(Some(contents.tab()) == focused);
        }
    }

    /// The pointer moved to `point`, CSS px of the window.
    pub fn pointer_moved(&mut self, point: PointF) {
        self.pointer = point;
        if let Some(popup) = &mut self.popup
            && (popup.contains(point) || self.pressed_in_popup)
        {
            if popup.mouse_move(point) {
                self.popup_dirty = true;
            }
            self.update();
            return;
        }
        let target = self.pointer_target();
        if !matches!(target, Some((Target::Toolbar, _))) && self.toolbar.mouse_leave() {
            self.chrome_dirty = true;
        }
        if !matches!(target, Some((Target::TabStrip, _))) && self.tab_strip.mouse_leave() {
            self.chrome_dirty = true;
        }
        match target {
            Some((Target::TabStrip, p)) => {
                if self.tab_strip.mouse_move(p) {
                    self.chrome_dirty = true;
                }
            }
            Some((Target::Toolbar, p)) => {
                if self.toolbar.mouse_move(p) {
                    self.chrome_dirty = true;
                }
            }
            Some((Target::WebView(tab), p)) => {
                let (buttons, modifiers) = (self.buttons, self.page_modifiers());
                if let Some(contents) = self.contents_mut(tab).filter(|c| !c.is_crashed()) {
                    contents.forward_input_event(InputEvent::MouseMove {
                        x: p.x,
                        y: p.y,
                        buttons,
                        modifiers,
                    });
                }
            }
            None => {}
        }
        self.update();
    }

    /// A mouse button went down or up where the pointer is; a press the
    /// first of its series.
    pub fn mouse_input(&mut self, button: MouseButton, pressed: bool) {
        self.mouse_input_with_count(button, pressed, 1);
    }

    /// A mouse button went down or up where the pointer is. A press is the
    /// `click_count`th of its series (`mouse::ClickCounter`); a release
    /// has its press's.
    pub fn mouse_input_with_count(&mut self, button: MouseButton, pressed: bool, click_count: u32) {
        let index = match button {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        };
        let click_count = if pressed {
            self.buttons |= button.buttons_bit();
            self.click_counts[index] = click_count;
            click_count
        } else {
            self.buttons &= !button.buttons_bit();
            self.click_counts[index]
        };
        // The dropdown takes the primary button over it, and its up after a
        // press in it; an up over it after a press elsewhere (on the select
        // that opened it) picks too. A press anywhere else closes it: on
        // the page by the renderer, which the press goes on to, and on the
        // browser's own parts here.
        if button == MouseButton::Left
            && let Some((popup_tab, inside)) = self
                .popup
                .as_ref()
                .map(|popup| (popup.tab(), popup.contains(self.pointer)))
        {
            let point = self.pointer;
            if pressed && inside {
                self.pressed_in_popup = true;
                if self
                    .popup
                    .as_mut()
                    .is_some_and(|popup| popup.mouse_move(point))
                {
                    self.popup_dirty = true;
                }
                self.update();
                return;
            }
            if !pressed && (self.pressed_in_popup || inside) {
                let in_popup = std::mem::take(&mut self.pressed_in_popup);
                if let Some(result) = self.popup.as_mut().and_then(|popup| popup.mouse_up(point)) {
                    self.close_popup(result);
                }
                if in_popup {
                    self.update();
                    return;
                }
            } else if pressed
                && !matches!(self.hit_test(point), Some((Target::WebView(tab), _)) if tab == popup_tab)
            {
                self.close_popup(PopupMenuResult::Canceled { by_press: true });
            }
        }
        let hit = match (pressed, self.captured) {
            (false, Some(_)) => self.pointer_target(),
            _ => self.hit_test(self.pointer),
        };
        if button == MouseButton::Left {
            self.captured = if pressed {
                hit.map(|(target, _)| target)
            } else {
                None
            };
        }
        match hit {
            Some((Target::Toolbar, p)) if button == MouseButton::Left => {
                if pressed {
                    self.toolbar.mouse_down(p, self.modifiers.shift_key());
                } else if let Some(pressed) = self.toolbar.mouse_up(p) {
                    self.run(match pressed {
                        Button::Back => Command::Back,
                        Button::Forward => Command::Forward,
                        Button::Reload => Command::Reload,
                    });
                }
                self.chrome_dirty = true;
            }
            Some((Target::Toolbar, _)) | None => {}
            Some((Target::TabStrip, p)) => {
                let action = if pressed {
                    self.tab_strip.mouse_down(p, button)
                } else {
                    self.tab_strip.mouse_up(p, button)
                };
                match action {
                    Some(tab_strip::Action::Activate(index)) => self.run(Command::SelectTab(index)),
                    Some(tab_strip::Action::Close(index)) => {
                        self.close_tab(index);
                        self.sync_toolbar();
                    }
                    Some(tab_strip::Action::NewTab) => self.run(Command::NewTab),
                    None => {}
                }
                self.chrome_dirty = true;
            }
            Some((Target::WebView(tab), p)) => {
                // A press in a web view gives it the focus, from the
                // address field too.
                if pressed {
                    self.focused = tab;
                    if self.toolbar.omnibox.is_focused() {
                        self.toolbar.omnibox.blur();
                        self.chrome_dirty = true;
                    }
                }
                if self.crashed(tab) {
                    // Only its sad tab's button: the renderer has gone.
                    let size = self
                        .web_view(tab)
                        .map(|view| view.bounds.to_f32().scale(1.0 / self.scale).size())
                        .unwrap_or_default();
                    if button == MouseButton::Left {
                        if pressed {
                            self.sad_tab.mouse_down(p, size);
                        } else if self.sad_tab.mouse_up(p, size) {
                            if let Some(contents) = self.contents_mut(tab) {
                                contents.reload();
                            }
                            self.sync_toolbar();
                        }
                    }
                    self.update();
                    return;
                }
                let (x, y) = (p.x, p.y);
                let modifiers = self.page_modifiers();
                let buttons = self.buttons;
                if let Some(contents) = self.contents_mut(tab) {
                    contents.forward_input_event(if pressed {
                        InputEvent::MouseDown {
                            x,
                            y,
                            button,
                            buttons,
                            modifiers,
                            click_count,
                        }
                    } else {
                        InputEvent::MouseUp {
                            x,
                            y,
                            button,
                            buttons,
                            modifiers,
                            click_count,
                        }
                    });
                }
            }
        }
        self.update();
    }

    /// The keys held, as the page's events have them.
    fn page_modifiers(&self) -> Modifiers {
        let m = self.modifiers;
        Modifiers {
            shift: m.shift_key(),
            ctrl: m.control_key(),
            alt: m.alt_key(),
            meta: m.super_key(),
        }
    }

    /// A wheel or touchpad scroll where the pointer is: to what it is over,
    /// whatever has captured it.
    pub fn wheel(&mut self, delta: ScrollDelta) {
        // Over the dropdown, it scrolls its rows; anywhere else it closes
        // it, and goes on.
        if let Some(popup) = &mut self.popup {
            if popup.contains(self.pointer) {
                let dy = match delta {
                    ScrollDelta::Pixels { y, .. } => y,
                    ScrollDelta::Lines { y, .. } => y * popup.row_height_px(),
                };
                if popup.wheel(dy) {
                    self.popup_dirty = true;
                    self.update();
                }
                return;
            }
            self.close_popup(PopupMenuResult::Canceled { by_press: false });
            self.update();
        }
        let (buttons, modifiers) = (self.buttons, self.page_modifiers());
        if let Some((Target::WebView(tab), p)) = self.hit_test(self.pointer)
            && let Some(contents) = self.contents_mut(tab).filter(|c| !c.is_crashed())
        {
            contents.forward_input_event(InputEvent::MouseWheel {
                x: p.x,
                y: p.y,
                delta,
                buttons,
                modifiers,
            });
        }
    }

    /// A key went down or up. The browser's shortcuts and the address
    /// field (while it has the keys) take what they want; any other key
    /// goes to the web view that has the keys (its key-up too, if its
    /// key-down went there). Tab in the address field moves into the page:
    /// to its first focusable element (its last with Shift+Tab). The keys
    /// are listed for users in the crate's README.md ("鍵盤和滑鼠"); keep
    /// it in step.
    pub fn key(&mut self, input: &KeyInput) {
        // Every key that goes down goes to the dropdown while it is open
        // (one that went down in the page still goes up there).
        if input.pressed
            && let Some(popup) = &mut self.popup
        {
            let event = keyboard::keyboard_event(input, self.modifiers);
            match popup.key(&event.key, self.modifiers.alt_key()) {
                KeyOutcome::Open => self.popup_dirty = true,
                KeyOutcome::Closed(result) => self.close_popup(result),
            }
            self.update();
            return;
        }
        if !input.pressed {
            let event = keyboard::keyboard_event(input, self.modifiers);
            if self.keys_down_in_page.remove(&input.physical)
                && let Some(contents) = self.contents_mut(self.focused).filter(|c| !c.is_crashed())
            {
                contents.forward_input_event(InputEvent::Key(event));
            }
            return;
        }
        let key = &input.logical;
        let in_field = self.toolbar.omnibox.is_focused();
        if let Some(command) = command_for(key, self.modifiers, self.mac, in_field) {
            self.run(command);
            return;
        }
        if in_field {
            let m = self.modifiers;
            if *key == Key::Named(NamedKey::Tab)
                && !(m.super_key() || m.control_key() || m.alt_key())
            {
                // `BrowserView` hands the focus to the web contents, which
                // gives it to the page's first (or last) element
                // (`WebContentsImpl::FocusThroughTabTraversal`).
                self.toolbar.omnibox.blur();
                self.chrome_dirty = true;
                self.update();
                if let Some(contents) = self.contents_mut(self.focused).filter(|c| !c.is_crashed())
                {
                    contents.set_initial_focus(m.shift_key());
                }
                return;
            }
            let result = self.toolbar.omnibox.key(
                key,
                input.text.as_deref(),
                self.modifiers,
                self.mac,
                self.clipboard.as_mut(),
            );
            match result {
                KeyResult::Navigate(typed) => {
                    if let Some(url) = fixup_url(&typed, &self.cwd) {
                        self.toolbar.omnibox.blur();
                        self.navigate(&url);
                    }
                }
                KeyResult::Handled | KeyResult::Ignored => {}
            }
            self.chrome_dirty = true;
            self.update();
            return;
        }
        let event = keyboard::keyboard_event(input, self.modifiers);
        let Some(contents) = self.contents_mut(self.focused).filter(|c| !c.is_crashed()) else {
            return;
        };
        contents.forward_input_event(InputEvent::Key(event));
        self.keys_down_in_page.insert(input.physical);
    }

    /// The page gave the focus back (`RendererHostMsg::TakeFocus`, Tab
    /// past its last or first focusable element): the address field takes
    /// it, as Chrome's toolbar does.
    fn take_focus(&mut self, tab: TabId) {
        if tab == self.focused && !self.toolbar.omnibox.is_focused() {
            self.toolbar.omnibox.focus();
            self.chrome_dirty = true;
        }
    }

    /// `Ime::Preedit`, for the address field.
    pub fn ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if self.toolbar.omnibox.is_focused() {
            self.toolbar.omnibox.preedit(text, cursor);
            self.chrome_dirty = true;
            self.update();
        }
    }

    /// `Ime::Commit`, for the address field.
    pub fn ime_commit(&mut self, text: &str) {
        if self.toolbar.omnibox.is_focused() {
            self.toolbar.omnibox.commit(text);
            self.chrome_dirty = true;
            self.update();
        }
    }

    pub fn run(&mut self, command: Command) {
        match command {
            Command::Back => {
                self.tabs.active_mut().go_back();
            }
            Command::Forward => {
                self.tabs.active_mut().go_forward();
            }
            Command::Reload => self.tabs.active_mut().reload(),
            Command::FocusLocation => {
                self.toolbar.omnibox.focus();
                self.chrome_dirty = true;
            }
            Command::NewTab => self.new_tab(),
            Command::CloseTab => self.close_tab(self.tabs.active_index()),
            Command::NextTab => self.activate_tab(self.tabs.next_index()),
            Command::PreviousTab => self.activate_tab(self.tabs.previous_index()),
            Command::SelectTab(index) => self.activate_tab(index),
            Command::SelectLastTab => self.activate_tab(self.tabs.last_index()),
        }
        self.sync_toolbar();
        self.update();
    }

    /// An event of `tab`'s, on the UI thread: handed to the tab, and the
    /// renderer's message returned, if it is one.
    pub fn on_contents_event(
        &mut self,
        tab: TabId,
        event: ContentsEvent,
    ) -> Option<RendererHostMsg> {
        match event {
            ContentsEvent::Renderer(msg) => {
                self.on_renderer_message(tab, &msg);
                Some(msg)
            }
            ContentsEvent::NavigationRedirected {
                navigation_id,
                redirect,
            } => {
                self.on_navigation_redirected(tab, navigation_id, &redirect);
                None
            }
            ContentsEvent::NavigationCompleted {
                navigation_id,
                result,
            } => {
                if let Some(contents) = self.contents_mut(tab) {
                    contents.on_navigation_completed(navigation_id, result);
                }
                None
            }
        }
    }

    /// `tab`'s navigation `navigation_id` reached a redirect, on the UI
    /// thread.
    pub fn on_navigation_redirected(
        &mut self,
        tab: TabId,
        navigation_id: u64,
        redirect: &RedirectInfo,
    ) {
        if let Some(contents) = self.contents_mut(tab) {
            contents.on_navigation_redirected(navigation_id, redirect);
        }
    }

    /// A message from `tab`'s renderer, on the UI thread. A tab whose
    /// renderer has gone is closed (Chrome shows its sad tab instead).
    pub fn on_renderer_message(&mut self, tab: TabId, msg: &RendererHostMsg) {
        let Some(contents) = self.contents_mut(tab) else {
            return;
        };
        contents.on_renderer_message(msg);
        let opened = contents.take_opened();
        for params in opened {
            self.open_from(tab, params);
        }
        match msg {
            RendererHostMsg::SetCursor(cursor) => {
                self.cursors.insert(tab, *cursor);
            }
            RendererHostMsg::TakeFocus { .. } => self.take_focus(tab),
            // Only the tab shown has one, over its web view.
            RendererHostMsg::ShowPopupMenu(menu) => {
                let anchor = self.web_view_to_window(tab, menu.bounds);
                if let Some(anchor) = anchor.filter(|_| tab == self.tabs.active().tab()) {
                    let window = SizeF::new(
                        self.size.width as f32 / self.scale,
                        self.size.height as f32 / self.scale,
                    );
                    self.popup = Some(PopupMenu::new(
                        tab,
                        menu.clone(),
                        anchor,
                        window,
                        self.test_fonts,
                    ));
                    self.pressed_in_popup = false;
                    self.popup_dirty = true;
                } else if let Some(contents) = self.contents_mut(tab) {
                    contents.popup_menu_closed(PopupMenuResult::Canceled { by_press: false });
                }
            }
            RendererHostMsg::HidePopupMenu => {
                if self.popup.as_ref().is_some_and(|p| p.tab() == tab) {
                    self.popup = None;
                    self.pressed_in_popup = false;
                    self.popup_dirty = true;
                }
            }
            // The tab stays, with its sad tab, until a navigation launches
            // another renderer.
            RendererHostMsg::RendererGone => {
                self.cursors.remove(&tab);
                if self.captured == Some(Target::WebView(tab)) {
                    self.captured = None;
                }
            }
            _ => {}
        }
        self.sync_toolbar();
        self.update();
    }

    /// The URL and the history buttons follow the active tab; the tab strip
    /// shows every tab's title.
    fn sync_toolbar(&mut self) {
        let titles = self.tabs.iter().map(tab_title).collect();
        if self.tab_strip.set_tabs(titles, self.tabs.active_index()) {
            self.chrome_dirty = true;
        }
        let before = (self.toolbar.omnibox.text(), self.toolbar.history());
        let url = self.tabs.active().url().unwrap_or_default().to_owned();
        self.toolbar.omnibox.set_url(&url);
        let controller = self.tabs.active().controller();
        self.toolbar
            .set_history(controller.can_go_back(), controller.can_go_forward());
        if before != (self.toolbar.omnibox.text(), self.toolbar.history()) {
            self.chrome_dirty = true;
        }
    }

    /// What the pointer should look like where it is: over a page, what
    /// its renderer last asked for (`RenderWidgetHostViewAura::
    /// UpdateCursor`); over the address field, the I-beam.
    pub fn cursor(&self) -> CursorType {
        match self.pointer_target() {
            Some((Target::TabStrip, _)) => CursorType::Pointer,
            Some((Target::Toolbar, p)) => match self.toolbar.part_at(p) {
                Part::Field => CursorType::IBeam,
                _ => CursorType::Pointer,
            },
            Some((Target::WebView(tab), _)) => self
                .cursors
                .get(&tab)
                .copied()
                .unwrap_or(CursorType::Pointer),
            None => CursorType::Pointer,
        }
    }

    /// The window's title: the active tab's.
    pub fn title(&self) -> String {
        tab_title(self.tabs.active())
    }

    /// While the address field has focus, where the IME's candidate window
    /// should avoid (CSS px of the window); `None` turns the IME off, so
    /// keys go to the page as they are.
    pub fn ime_area(&mut self) -> Option<RectF> {
        if !self.toolbar.omnibox.is_focused() {
            return None;
        }
        let origin = self.toolbar_bounds.to_f32().scale(1.0 / self.scale);
        Some(self.toolbar.ime_cursor_area().offset(origin.x, origin.y))
    }

    /// Draws the browser's frame if anything in it changed: the tab strip
    /// or the toolbar, the window's size, or a web view's surface.
    fn update(&mut self) {
        self.sync_focus();
        if self.size.is_empty() {
            return;
        }
        let scale = self.scale;
        let css = move |rect: Rect| rect.to_f32().scale(1.0 / scale);
        // The web views whose renderer has gone show their sad tab instead
        // (Chromium's `SadTabView`, over the contents).
        let sad: Vec<Rect> = self
            .web_views
            .iter()
            .filter(|view| !view.bounds.is_empty() && self.crashed(view.contents))
            .map(|view| view.bounds)
            .collect();
        let popup_dirty = std::mem::take(&mut self.popup_dirty);
        if std::mem::take(&mut self.chrome_dirty) || sad != self.sad_drawn || popup_dirty {
            // One layer from the window's top left: each painted where the
            // layout put it.
            let (strip, toolbar) = (css(self.tab_strip_bounds), css(self.toolbar_bounds));
            let mut list = DisplayItemList::new();
            list.push_list_at(&self.tab_strip.paint(), strip.x, strip.y);
            list.push_list_at(&self.toolbar.paint(), toolbar.x, toolbar.y);
            list.finalize();
            let mut layers = vec![PictureLayer {
                id: LayerId(1),
                bounds: SizeF::new(
                    strip.right().max(toolbar.right()),
                    (strip.y + tab_strip::HEIGHT).max(toolbar.y + toolbar::HEIGHT),
                ),
                display_list: Arc::new(list),
                contents_opaque: true,
                background_color: Color::WHITE,
                // Small: rastered whole when it changes.
                invalidation: None,
                properties: LayerProperties::default(),
            }];
            for (bounds, id) in sad.iter().zip(100..) {
                let at = css(*bounds);
                layers.push(PictureLayer {
                    id: LayerId(id),
                    bounds: at.size(),
                    display_list: Arc::new(self.sad_tab.paint(at.size())),
                    contents_opaque: true,
                    background_color: sad_tab::BACKGROUND,
                    invalidation: None,
                    properties: LayerProperties {
                        origin: PointF::new(at.x, at.y),
                        ..Default::default()
                    },
                });
            }
            // The dropdown, over the web views too.
            if let Some(popup) = &mut self.popup {
                let at = popup.rect();
                layers.push(PictureLayer {
                    id: LayerId(50),
                    bounds: at.size(),
                    display_list: Arc::new(popup.paint()),
                    contents_opaque: true,
                    background_color: Color::WHITE,
                    invalidation: None,
                    properties: LayerProperties {
                        origin: PointF::new(at.x, at.y),
                        over_surfaces: true,
                        ..Default::default()
                    },
                });
            }
            self.compositor.set_layers(layers, PropertyTrees::default());
            self.sad_drawn = sad;
        }
        // In the web views' order: a later one is drawn over an earlier
        // one, as it is hit first. The compositor rounds them back to the
        // bounds' whole pixels.
        let surfaces = self
            .web_views
            .iter()
            .zip(2..)
            .filter(|(view, _)| !view.bounds.is_empty() && !self.crashed(view.contents))
            .filter_map(|(view, id)| {
                let bounds = css(view.bounds);
                Some(SurfaceLayer {
                    id: LayerId(id),
                    position: PointF::new(bounds.x, bounds.y),
                    bounds: bounds.size(),
                    surface_id: self.contents(view.contents)?.surface_id(),
                    background_color: Color::WHITE,
                })
            })
            .collect();
        self.compositor.set_surface_layers(surfaces);
        if self.compositor.needs_commit() {
            self.compositor.composite();
        }
    }

    /// The GPU was lost and Viz has a new one: the toolbar's tiles go on
    /// it again. (The renderers hear of it from their own hosts.)
    pub fn did_lose_context(&mut self) {
        self.compositor.did_lose_layer_tree_frame_sink();
        if !self.size.is_empty() {
            self.compositor.composite();
        }
    }

    pub fn shutdown(&mut self) {
        for contents in self.tabs.iter_mut().chain(&mut self.others) {
            contents.shutdown();
        }
    }
}

/// What a tab is called: its page's title, or its URL while it has none.
fn tab_title(contents: &WebContents) -> String {
    match contents.title() {
        "" => contents.url().unwrap_or("machopo").to_owned(),
        title => title.to_owned(),
    }
}

impl WebView {
    fn new(contents: TabId) -> Self {
        Self {
            contents,
            bounds: Rect::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_per_platform() {
        let cmd = ModifiersState::SUPER;
        let (ctrl, alt) = (ModifiersState::CONTROL, ModifiersState::ALT);
        let char = |c: &str| Key::Character(c.into());
        let left = Key::Named(NamedKey::ArrowLeft);
        let right = Key::Named(NamedKey::ArrowRight);
        let on = |key: &Key, modifiers, mac| command_for(key, modifiers, mac, false);
        // macOS.
        assert_eq!(on(&char("["), cmd, true), Some(Command::Back));
        assert_eq!(on(&char("]"), cmd, true), Some(Command::Forward));
        assert_eq!(on(&left, cmd, true), Some(Command::Back));
        assert_eq!(on(&right, cmd, true), Some(Command::Forward));
        assert_eq!(on(&char("R"), cmd, true), Some(Command::Reload));
        assert_eq!(on(&char("l"), cmd, true), Some(Command::FocusLocation));
        assert_eq!(on(&left, alt, true), None);
        assert_eq!(on(&char("r"), ctrl, true), None);
        assert_eq!(on(&char("d"), alt, true), None);
        // In the address field, Cmd+arrows move the caret; Cmd+[ still
        // goes back.
        assert_eq!(command_for(&left, cmd, true, true), None);
        assert_eq!(
            command_for(&char("["), cmd, true, true),
            Some(Command::Back)
        );
        // Elsewhere.
        assert_eq!(on(&left, alt, false), Some(Command::Back));
        assert_eq!(on(&right, alt, false), Some(Command::Forward));
        assert_eq!(command_for(&left, alt, false, true), Some(Command::Back));
        assert_eq!(on(&char("r"), ctrl, false), Some(Command::Reload));
        assert_eq!(on(&char("l"), ctrl, false), Some(Command::FocusLocation));
        assert_eq!(on(&char("d"), alt, false), Some(Command::FocusLocation));
        assert_eq!(on(&char("["), ctrl, false), None);
        assert_eq!(on(&left, cmd, false), None);
        // F5 and F6 anywhere; plain keys aren't shortcuts.
        let none = ModifiersState::empty();
        assert_eq!(
            on(&Key::Named(NamedKey::F5), none, false),
            Some(Command::Reload)
        );
        assert_eq!(
            on(&Key::Named(NamedKey::F6), none, true),
            Some(Command::FocusLocation)
        );
        assert_eq!(on(&left, none, true), None);
        assert_eq!(on(&char("r"), none, false), None);
    }

    #[test]
    fn tab_shortcuts_per_platform() {
        let cmd = ModifiersState::SUPER;
        let ctrl = ModifiersState::CONTROL;
        let shift = ModifiersState::SHIFT;
        let char = |c: &str| Key::Character(c.into());
        let named = Key::Named;
        let on = |key: &Key, modifiers, mac| command_for(key, modifiers, mac, false);
        for (mac, primary) in [(true, cmd), (false, ctrl)] {
            assert_eq!(on(&char("t"), primary, mac), Some(Command::NewTab));
            assert_eq!(on(&char("W"), primary, mac), Some(Command::CloseTab));
            assert_eq!(on(&char("1"), primary, mac), Some(Command::SelectTab(0)));
            assert_eq!(on(&char("8"), primary, mac), Some(Command::SelectTab(7)));
            assert_eq!(on(&char("9"), primary, mac), Some(Command::SelectLastTab));
            assert_eq!(on(&char("0"), primary, mac), None);
            assert_eq!(on(&char("t"), ModifiersState::empty(), mac), None);
            // Ctrl+Tab on both, in the address field too.
            let tab = named(NamedKey::Tab);
            assert_eq!(on(&tab, ctrl, mac), Some(Command::NextTab));
            assert_eq!(on(&tab, ctrl | shift, mac), Some(Command::PreviousTab));
            assert_eq!(command_for(&tab, ctrl, mac, true), Some(Command::NextTab));
            assert_eq!(on(&tab, ModifiersState::empty(), mac), None);
        }
        assert_eq!(on(&char("t"), ctrl, true), None, "Ctrl isn't macOS's");
        assert_eq!(on(&char("t"), cmd, false), None);
        // macOS: Cmd+Option+arrows, Cmd+Shift+[ ]; Cmd+[ stays back.
        let opt = cmd | ModifiersState::ALT;
        let left = named(NamedKey::ArrowLeft);
        let right = named(NamedKey::ArrowRight);
        assert_eq!(on(&right, opt, true), Some(Command::NextTab));
        assert_eq!(on(&left, opt, true), Some(Command::PreviousTab));
        assert_eq!(on(&char("}"), cmd | shift, true), Some(Command::NextTab));
        assert_eq!(
            on(&char("{"), cmd | shift, true),
            Some(Command::PreviousTab)
        );
        assert_eq!(on(&char("]"), cmd | shift, true), Some(Command::NextTab));
        assert_eq!(
            on(&char("["), cmd | shift, true),
            Some(Command::PreviousTab)
        );
        assert_eq!(on(&char("["), cmd, true), Some(Command::Back));
        // Elsewhere: Ctrl+PageDown / PageUp.
        let down = named(NamedKey::PageDown);
        let up = named(NamedKey::PageUp);
        assert_eq!(on(&down, ctrl, false), Some(Command::NextTab));
        assert_eq!(on(&up, ctrl, false), Some(Command::PreviousTab));
        assert_eq!(on(&down, ctrl, true), None);
        assert_eq!(on(&down, ModifiersState::empty(), false), None);
    }
}
