//! `BrowserView`: what a browser window shows, the browser's own UI (the
//! tab strip and the toolbar, a tree of `ui`'s views, `chrome`) and the web
//! views, and where its input goes. Its tabs are a
//! `TabStripModel`; the first web view shows the active one. A
//! `BrowserViewLayout` gives each its bounds; drawing, the renderers' sizes
//! and hit testing all follow them. The browser draws through its own
//! compositor (Chromium's `ui::Compositor`, a `cc::LayerTreeHost`): the UI's
//! tree is the root layer, and each web view's surface is
//! a `SurfaceLayer`, which Viz fills with that renderer's frames. A new
//! frame from a renderer needs nothing from the browser.
//!
//! Kept apart from winit (`window` translates its events), so tests can
//! drive a window's worth of browser with plain calls.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use base::SequenceNumber;
use browser::{ContentsEvent, OpenUrlParams, WebContents};
use compositor::mojo_embedder::ChannelLayerTreeFrameSink;
use compositor::{
    ClipNode, ClipNodeId, LayerId, LayerProperties, LayerTreeHost, LayerTreeSettings, PictureLayer,
    PropertyTrees, SingleThreadProxy, SurfaceLayer, TransformNodeId,
};
use crossbeam_channel::Sender;
use gfx::{Color, PointF, Rect, RectF, Size, SizeF};
use protocol::TabId;
use protocol::input::{InputEvent, Modifiers, MouseButton, ScrollDelta};
use protocol::renderer::{CursorType, PopupMenuResult, RendererHostMsg, WindowOpenDisposition};
use protocol::viz::{
    CompositorFrameSinkSender, ContextProvider, FrameSinkId, LocalSurfaceId, VizMsg,
};
use std::sync::Arc;
use winit::keyboard::{Key, ModifiersState, NamedKey, PhysicalKey};

use crate::browser_view_layout::tab_strip_height;
use crate::chrome::{Chrome, CustomLayout, UiLayout, UiMessage, UiState};
use crate::keyboard::{self, KeyInput};
use crate::omnibox::Clipboard;
use crate::omnibox::Omnibox;
use crate::popup_menu::{KeyOutcome, PopupMenu};
use crate::sad_tab::{self, SadTab};
use crate::tab_strip::TabStrip;
use crate::tab_strip_model::TabStripModel;
use crate::theme::{BrowserTheme, ColorMode, SizeToken};
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
    /// Reload past the HTTP cache (Chrome's `IDC_RELOAD_BYPASSING_CACHE`).
    ReloadBypassingCache,
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
    /// Lay the UI out down a sidebar, or across the top again.
    ToggleSidebar,
    /// Collapse the sidebar, or expand it again (in the sidebar layout).
    ToggleSidebarCollapsed,
}

/// The shortcut `key` is, with `modifiers` held, on macOS (`mac`) or
/// elsewhere:
/// - back / forward: Cmd+[ / Cmd+] and Cmd+← / Cmd+→ on macOS (the arrows
///   only outside the address field, where they move the caret), Alt+← /
///   Alt+→ elsewhere;
/// - reload: Cmd+R / Ctrl+R, and F5; past the HTTP cache, with Shift
///   (Shift+Cmd+R / Ctrl+Shift+R, Shift+F5), and Ctrl+F5;
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
        // Shift+F5 and Ctrl+F5 bypass the cache, as in Chrome.
        Key::Named(NamedKey::F5) if shift || ctrl => Some(Command::ReloadBypassingCache),
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
            "r" if primary && shift => Some(Command::ReloadBypassingCache),
            "r" if primary => Some(Command::Reload),
            // Safari's to show or hide its sidebar; before Cmd+L.
            "l" if primary && shift => Some(Command::ToggleSidebarCollapsed),
            "l" if primary => Some(Command::FocusLocation),
            "d" if modifiers.alt_key() && !mac => Some(Command::FocusLocation),
            "t" if primary => Some(Command::NewTab),
            // Shift with the comma types `<` on most layouts.
            "," | "<" if primary && shift => Some(Command::ToggleSidebar),
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
    /// The browser's own UI, the tree's.
    Chrome,
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
    /// Its corners' radius, CSS px: a card's.
    radius: f32,
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
    /// Which way the UI is laid out.
    ui_layout: UiLayout,
    /// The sidebar's width, CSS px, as last dragged.
    sidebar_width: f32,
    /// The sidebar collapsed (Cmd+Shift+L), and floating over the page now.
    sidebar_collapsed: bool,
    sidebar_floating: bool,
    /// `ui::Compositor`.
    compositor: LayerTreeHost<SingleThreadProxy>,
    /// The tab strip and the toolbar (Chromium's `TopContainerView`), one
    /// layer, painted where it changed.
    chrome: Chrome,
    /// The UI's layers as last painted (the tree's, then one for each view
    /// that scrolls its children): given the compositor again while only
    /// other layers change, so that they keep their tiles.
    chrome_layers: Vec<ui::FrameLayer>,
    /// The compositor's layer for each view that scrolls, while it is
    /// there: a layer keeps its tiles from one frame to the next.
    chrome_layer_ids: HashMap<ui::AnyViewId, LayerId>,
    /// Their ids: after the browser's own layers' (the UI's 1, the web
    /// views' from 2, the dropdown's 50, sad tabs' from 100).
    next_chrome_layer_id: SequenceNumber<u32>,
    /// What the UI's last frame changed, as given its layer (`None`: all
    /// of it), for tests.
    chrome_invalidation: Option<Arc<[RectF]>>,
    /// What a web view whose renderer has gone shows.
    sad_tab: SadTab,
    /// Where sad tabs were last drawn, physical px, and their corners.
    sad_drawn: Vec<(Rect, f32)>,
    /// Physical px.
    size: Size,
    scale: f32,
    /// Where the window is on the screen, CSS px.
    origin: PointF,
    local_surface_id: LocalSurfaceId,
    /// CSS px of the window.
    pointer: PointF,
    /// Whether the pointer is in the window: it moved there and hasn't
    /// left (unknown at first, as if not).
    pointer_in_window: bool,
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
}

impl BrowserView {
    /// `viz` takes the browser's frames, for `BROWSER_FRAME_SINK`.
    /// `test_fonts` as for the renderer: the toolbar's text too.
    /// With `gpu` (Viz has one), the toolbar's tiles go to it as shared
    /// images; with `None` they go to Viz as pixels, which it uploads.
    pub fn new(
        web_contents: WebContents,
        viz: Sender<VizMsg>,
        gpu: Option<Arc<dyn ContextProvider>>,
        test_fonts: bool,
        clipboard: Box<dyn Clipboard>,
    ) -> Self {
        // The toolbar's tiles go to the GPU as a renderer's do: by
        // messages to Viz, which tells this client when it has done with
        // a tile's pixels.
        let shared_images = gpu.is_some().then(|| {
            let sink = CompositorFrameSinkSender::new(BROWSER_FRAME_SINK, viz.clone());
            let images = compositor::SharedImages::new(move |msg| {
                let _ = sink.send(msg);
            });
            let returned = Arc::downgrade(&images);
            let client = protocol::viz::BeginFrameObserver::new(|_| {}).with_pixels_returned(
                move |mailbox| {
                    if let Some(images) = returned.upgrade() {
                        images.pixels_returned(mailbox);
                    }
                },
            );
            let _ = viz.send(VizMsg::RegisterFrameSinkId(BROWSER_FRAME_SINK, client));
            images
        });
        let frame_sink =
            ChannelLayerTreeFrameSink::new(CompositorFrameSinkSender::new(BROWSER_FRAME_SINK, viz));
        let tab = web_contents.tab();
        Self {
            tabs: TabStripModel::new(web_contents),
            others: Vec::new(),
            factory: None,
            // Where the UI lays them out, once the window has a size.
            web_views: Vec::new(),
            cursors: HashMap::new(),
            openers: HashMap::new(),
            next_tab: 0,
            close_requested: false,
            ui_layout: UiLayout::Horizontal,
            sidebar_width: BrowserTheme::light().size(SizeToken::SidebarWidth),
            sidebar_collapsed: false,
            sidebar_floating: false,
            compositor: LayerTreeHost::new_single_threaded(
                LayerTreeSettings {
                    shared_images,
                    ..LayerTreeSettings::default()
                },
                Box::new(frame_sink),
            ),
            chrome: Chrome::new(test_fonts, clipboard),
            chrome_layers: Vec::new(),
            chrome_layer_ids: HashMap::new(),
            next_chrome_layer_id: SequenceNumber::starting_at(1000),
            chrome_invalidation: None,
            sad_tab: SadTab::new(),
            sad_drawn: Vec::new(),
            size: Size::new(0, 0),
            scale: 1.0,
            origin: PointF::new(0.0, 0.0),
            local_surface_id: LocalSurfaceId::default(),
            pointer: PointF::new(0.0, 0.0),
            pointer_in_window: false,
            modifiers: ModifiersState::empty(),
            captured: None,
            focused: tab,
            window_focused: true,
            keys_down_in_page: HashSet::new(),
            buttons: 0,
            click_counts: [1; 3],
            mac: cfg!(target_os = "macos"),
            cwd: std::env::current_dir().unwrap_or_default(),
            popup: None,
            popup_dirty: false,
            pressed_in_popup: false,
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
        self.sync_ui();
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
        self.sync_ui();
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
        self.chrome.focus_omnibox();
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
        self.focused = tab;
        self.chrome.blur_omnibox();
        // The web view the UI lays out shows it now: the one shown before
        // is let go of (and its capture).
        self.relayout();
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
        let shown = self.tabs.active().tab();
        let bounds = self
            .web_view(shown)
            .map(|view| view.bounds)
            .unwrap_or_default();
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
        self.sync_ui();
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
        self.sync_ui();
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
        self.sync_ui();
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

    /// The address field.
    pub fn omnibox(&self) -> &Omnibox {
        self.chrome.omnibox()
    }

    /// The address field's box, CSS px of the window.
    pub fn omnibox_rect(&self) -> RectF {
        self.chrome_rect(self.chrome.omnibox.any())
    }

    /// Where a view of the UI is, CSS px of the window.
    fn chrome_rect(&self, id: ui::AnyViewId) -> RectF {
        self.chrome.tree.absolute_bounds(id).unwrap_or_default()
    }

    /// Whether back and forward are enabled.
    pub fn history(&self) -> (bool, bool) {
        self.chrome.history()
    }

    /// What the UI's last frame changed, CSS px of the UI (`None`: all of
    /// it): only that is rastered again.
    pub fn chrome_invalidation_for_testing(&self) -> Option<&[RectF]> {
        self.chrome_invalidation.as_deref()
    }

    /// The tab strip, in the horizontal layout.
    pub fn tab_strip(&self) -> &TabStrip {
        self.chrome
            .tab_strip()
            .expect("the horizontal layout has a tab strip")
    }

    /// Lays the window out with `layout` from now on, instead of in the
    /// horizontal or sidebar layout (`None`: in those again).
    pub fn set_layout(&mut self, layout: Option<CustomLayout>) {
        self.chrome.set_custom_layout(layout);
        self.relayout();
        self.update();
    }

    /// Lays the UI out `layout` from now on.
    pub fn set_ui_layout(&mut self, layout: UiLayout) {
        self.ui_layout = layout;
        self.relayout();
        self.update();
    }

    pub fn ui_layout(&self) -> UiLayout {
        self.ui_layout
    }

    /// Paints the UI light or dark from now on (the pages are as they were).
    pub fn set_color_mode(&mut self, mode: ColorMode) {
        self.chrome.set_color_mode(mode);
        self.update();
    }

    pub fn color_mode(&self) -> ColorMode {
        self.chrome.color_mode()
    }

    /// Shows `web_contents` too, in a web view of its own after the others.
    /// Its frames go to Viz like the tab's: the caller has Viz send it
    /// `BeginFrame`s.
    pub fn add_web_view(&mut self, web_contents: WebContents) {
        self.others.push(web_contents);
        self.relayout();
        self.update();
    }

    /// Stops showing `tab`, which isn't a tab but another web view's: its
    /// web contents, for the caller to shut down. The pointer's capture
    /// goes with it, and the focus goes back to the active tab.
    pub fn remove_web_view(&mut self, tab: TabId) -> Option<WebContents> {
        let index = self.others.iter().position(|c| c.tab() == tab)?;
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
        self.chrome.blur_omnibox();
        self.sync_ui();
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

    /// Lays the UI out again, puts the web views where it says, and tells
    /// each renderer its size (`RenderWidgetHostViewAura::SetBounds`). A new
    /// scale alone needs no new frame of the UI: its layer's tiles are
    /// rastered again at it.
    fn relayout(&mut self) {
        self.sync_ui();
        self.place_web_views();
        self.send_screen_rects();
    }

    /// The web views where the UI lays them out now, each renderer told its
    /// size (`WebContents::resize` skips the same one). A web view no more
    /// shown lets go of the pointer's capture.
    fn place_web_views(&mut self) {
        let placed = self.chrome.web_views(self.scale);
        let scale = self.scale;
        self.web_views = placed
            .into_iter()
            .map(|(tab, bounds)| {
                if !bounds.is_empty()
                    && let Some(contents) = self.contents_mut(tab)
                {
                    contents.resize(bounds.size(), scale);
                }
                WebView {
                    contents: tab,
                    bounds,
                    radius: self.chrome.web_view_radius(tab),
                }
            })
            .collect();
        if let Some(Target::WebView(tab)) = self.captured
            && self.web_view(tab).is_none()
        {
            self.captured = None;
        }
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
    /// the web views from the top one down, then the UI (aura's
    /// `WindowTargeter`).
    fn hit_test(&self, point: PointF) -> Option<(Target, PointF)> {
        // The sidebar floated over the page is over it.
        if self
            .floating_sidebar()
            .is_some_and(|sidebar| sidebar.contains(point))
        {
            return Some((Target::Chrome, self.in_target(Target::Chrome, point)?));
        }
        let at = PointF::new(point.x * self.scale, point.y * self.scale);
        let target = self
            .web_views
            .iter()
            .rev()
            .find(|view| view.bounds.to_f32().contains(at))
            .map(|view| Target::WebView(view.contents))
            .or_else(|| {
                self.window_bounds()
                    .to_f32()
                    .contains(at)
                    .then_some(Target::Chrome)
            })?;
        Some((target, self.in_target(target, point)?))
    }

    /// The window, physical px: the UI's tree covers it, the web views
    /// over it.
    fn window_bounds(&self) -> Rect {
        Rect::new(0, 0, self.size.width, self.size.height)
    }

    /// The bounds of `target`, if it's still there.
    fn bounds_of(&self, target: Target) -> Option<Rect> {
        match target {
            Target::Chrome => Some(self.window_bounds()),
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
            (self.window_focused && !self.chrome.omnibox_focused()).then_some(self.focused);
        for contents in self.tabs.iter_mut().chain(self.others.iter_mut()) {
            contents.set_focus(Some(contents.tab()) == focused);
        }
    }

    /// The pointer moved to `point`, CSS px of the window.
    pub fn pointer_moved(&mut self, point: PointF) {
        self.pointer = point;
        self.pointer_in_window = true;
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
        if !matches!(target, Some((Target::Chrome, _))) {
            self.chrome.tree.pointer_left();
        }
        match target {
            Some((Target::Chrome, p)) => {
                let modifiers = self.ui_modifiers();
                self.chrome.tree.pointer(ui::PointerEvent::Move {
                    position: p,
                    modifiers,
                });
                self.handle_ui_messages();
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
            None => {}
            Some((Target::Chrome, position)) => {
                let (button, modifiers) = (ui_button(button), self.ui_modifiers());
                self.chrome.tree.pointer(if pressed {
                    ui::PointerEvent::Down {
                        position,
                        button,
                        click_count,
                        modifiers,
                    }
                } else {
                    ui::PointerEvent::Up {
                        position,
                        button,
                        click_count,
                        modifiers,
                    }
                });
                self.handle_ui_messages();
            }
            Some((Target::WebView(tab), p)) => {
                // A press in a web view gives it the focus, from the
                // address field too.
                if pressed {
                    self.focused = tab;
                    self.chrome.blur_omnibox();
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
                            self.sync_ui();
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

    /// The sidebar `width` CSS px wide, as far as it may be in this window
    /// (`sidebar_width_in_window`).
    fn resize_sidebar(&mut self, width: f32) {
        let width = self.sidebar_width_in_window(width);
        if width != self.sidebar_width {
            self.sidebar_width = width;
            self.relayout();
        }
    }

    /// `width` as wide as the sidebar may be in this window: not narrower
    /// than the theme's least, not wider than its most nor half the window
    /// (in a window too narrow for both, half the window).
    fn sidebar_width_in_window(&self, width: f32) -> f32 {
        let theme = self.chrome.tree.theme();
        let least = theme.size(SizeToken::SidebarMinWidth);
        let most = theme
            .size(SizeToken::SidebarMaxWidth)
            .min(self.size.width as f32 / self.scale / 2.0);
        width.max(least).min(most)
    }

    /// The keys held, as the UI's events have them.
    fn ui_modifiers(&self) -> ui::Modifiers {
        let m = self.modifiers;
        ui::Modifiers {
            shift: m.shift_key(),
            ctrl: m.control_key(),
            alt: m.alt_key(),
            meta: m.super_key(),
        }
    }

    /// Does what the UI's views asked, in order.
    fn handle_ui_messages(&mut self) {
        for message in self.chrome.tree.take_messages() {
            match message {
                UiMessage::Command(command) => self.run(command),
                UiMessage::CloseTab(index) => {
                    self.close_tab(index);
                    self.sync_ui();
                }
                UiMessage::SelectTabById(tab) => {
                    if let Some(index) = self.tabs.position(|c| c.tab() == tab) {
                        self.run(Command::SelectTab(index));
                    }
                }
                UiMessage::CloseTabById(tab) => {
                    if let Some(index) = self.tabs.position(|c| c.tab() == tab) {
                        self.close_tab(index);
                        self.sync_ui();
                    }
                }
                UiMessage::ResizeSidebar(width) => self.resize_sidebar(width),
                UiMessage::Navigate(typed) => {
                    if let Some(url) = fixup_url(&typed, &self.cwd) {
                        self.chrome.blur_omnibox();
                        self.navigate(&url);
                    }
                }
            }
        }
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
        // Over the UI, to what scrolls there.
        if let Some((Target::Chrome, p)) = self.hit_test(self.pointer) {
            let modifiers = self.ui_modifiers();
            self.chrome.tree.pointer(ui::PointerEvent::Wheel {
                position: p,
                delta: compositor::wheel_scroll_delta(delta),
                modifiers,
            });
            self.handle_ui_messages();
            self.update();
            return;
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
        let in_field = self.chrome.omnibox_focused();
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
                self.chrome.blur_omnibox();
                self.update();
                if let Some(contents) = self.contents_mut(self.focused).filter(|c| !c.is_crashed())
                {
                    contents.set_initial_focus(m.shift_key());
                }
                return;
            }
            // Every key goes to the field while it has them, used or not.
            let event = keyboard::ui_key_event(input, self.modifiers);
            self.chrome.tree.key(&event);
            // Escape in the field of the floating sidebar puts it away too,
            // as Enter does.
            if self.sidebar_floating
                && event.pressed
                && event.key == ui::Key::Named(ui::NamedKey::Escape)
            {
                self.chrome.blur_omnibox();
            }
            self.handle_ui_messages();
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
        if tab == self.focused {
            self.chrome.focus_omnibox();
        }
    }

    /// `Ime::Preedit`, for the address field.
    pub fn ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if self.chrome.omnibox_focused() {
            self.chrome.tree.ime(&ui::ImeEvent::Preedit {
                text: text.to_owned(),
                cursor,
            });
            self.update();
        }
    }

    /// `Ime::Commit`, for the address field.
    pub fn ime_commit(&mut self, text: &str) {
        if self.chrome.omnibox_focused() {
            self.chrome.tree.ime(&ui::ImeEvent::Commit(text.to_owned()));
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
            Command::ReloadBypassingCache => self.tabs.active_mut().reload_bypassing_cache(),
            Command::FocusLocation => self.chrome.focus_omnibox(),
            Command::NewTab => self.new_tab(),
            Command::CloseTab => self.close_tab(self.tabs.active_index()),
            Command::NextTab => self.activate_tab(self.tabs.next_index()),
            Command::PreviousTab => self.activate_tab(self.tabs.previous_index()),
            Command::SelectTab(index) => self.activate_tab(index),
            Command::SelectLastTab => self.activate_tab(self.tabs.last_index()),
            Command::ToggleSidebar => {
                self.ui_layout = match self.ui_layout {
                    UiLayout::Horizontal => UiLayout::Sidebar,
                    UiLayout::Sidebar => UiLayout::Horizontal,
                };
                self.relayout();
            }
            // Nothing to collapse across the top.
            Command::ToggleSidebarCollapsed if self.ui_layout == UiLayout::Sidebar => {
                self.sidebar_collapsed = !self.sidebar_collapsed;
                self.relayout();
            }
            Command::ToggleSidebarCollapsed => {}
        }
        self.sync_ui();
        self.update();
    }

    /// An event of `tab`'s, on the UI thread: handed to the tab, and the
    /// renderer's message returned, if it is one that counts (its renderer
    /// the tab's, not one it ended or replaced).
    pub fn on_contents_event(
        &mut self,
        tab: TabId,
        event: ContentsEvent,
    ) -> Option<RendererHostMsg> {
        match event {
            ContentsEvent::Renderer { renderer, msg } => {
                // A closed tab's renderer's go on (its `RendererGone` ends
                // what waited for the tab): the tab has nothing left to be
                // misled.
                if self
                    .contents(tab)
                    .is_some_and(|contents| !contents.is_current_renderer(renderer))
                {
                    return None;
                }
                self.on_renderer_message(tab, &msg);
                Some(msg)
            }
            ContentsEvent::BadMessage { renderer, reason } => {
                if let Some(contents) = self.contents_mut(tab)
                    && contents.is_current_renderer(renderer)
                {
                    contents.received_bad_message(reason);
                }
                None
            }
            // For DevTools.
            ContentsEvent::NavigationStarted(_) => None,
            // A navigation's.
            event => {
                if let Some(contents) = self.contents_mut(tab) {
                    contents.on_navigation_event(event);
                }
                None
            }
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
                        self.chrome.tree.text(),
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
        self.sync_ui();
        self.update();
    }

    /// What the UI shows of the browser now: every tab's title, the active
    /// tab's URL and history, the window's width.
    fn ui_state(&self) -> UiState {
        let controller = self.tabs.active().controller();
        UiState {
            titles: self.tabs.iter().map(tab_title).collect(),
            active: self.tabs.active_index(),
            url: self.tabs.active().url().unwrap_or_default().to_owned(),
            can_go_back: controller.can_go_back(),
            can_go_forward: controller.can_go_forward(),
            mac: self.mac,
            width: self.size.width as f32 / self.scale,
            height: self.size.height as f32 / self.scale,
            scale: self.scale,
            strip_height: tab_strip_height(self.scale) as f32 / self.scale,
            // As dragged, or the theme's at first, within this window: a
            // window made wider again gives it back.
            sidebar_width: self.sidebar_width_in_window(self.sidebar_width),
            sidebar_collapsed: self.sidebar_collapsed,
            sidebar_floating: self.sidebar_floats(),
            layout: self.ui_layout,
            tabs: self.tabs.iter().map(WebContents::tab).collect(),
            shown: std::iter::once(self.tabs.active().tab())
                .chain(self.others.iter().map(WebContents::tab))
                .collect(),
        }
    }

    /// The UI follows the browser: described again if what it shows
    /// changed.
    fn sync_ui(&mut self) {
        let state = self.ui_state();
        self.sidebar_floating = state.sidebar_floating;
        self.chrome.update(state);
    }

    /// Whether the collapsed sidebar floats over the page now (Zen's
    /// compact mode): the pointer at the window's left edge brings it, and
    /// over it keeps it; the address field having the keys (Cmd+L) keeps
    /// it, and so does a press in it until the button comes up.
    fn sidebar_floats(&self) -> bool {
        if !(self.sidebar_collapsed && self.ui_layout == UiLayout::Sidebar) {
            return false;
        }
        let p = self.pointer;
        let in_window = self.pointer_in_window;
        let edge = self.chrome.tree.theme().size(SizeToken::SidebarEdgeWidth);
        let over = self.sidebar_floating && p.x < self.sidebar_width_in_window(self.sidebar_width);
        (in_window && (p.x < edge || over))
            || self.chrome.omnibox_focused()
            || (self.sidebar_floating && self.chrome.tree.captured().is_some())
    }

    /// The pointer left the window: nothing in it is under the pointer,
    /// and the floating sidebar goes (unless its field has the keys).
    pub fn pointer_left_window(&mut self) {
        self.pointer_in_window = false;
        self.chrome.tree.pointer_left();
        self.update();
    }

    /// The floating sidebar's bounds, CSS px of the window, while it
    /// floats.
    fn floating_sidebar(&self) -> Option<RectF> {
        let panel = self.chrome.floating_panel()?;
        self.chrome.tree.absolute_bounds(panel)
    }

    /// What the pointer should look like where it is: over a page, what
    /// its renderer last asked for (`RenderWidgetHostViewAura::
    /// UpdateCursor`); over the address field, the I-beam.
    pub fn cursor(&self) -> CursorType {
        match self.pointer_target() {
            Some((Target::Chrome, p)) => match self.chrome.tree.cursor(p) {
                ui::Cursor::Text => CursorType::IBeam,
                ui::Cursor::ResizeHorizontal => CursorType::EastWestResize,
                ui::Cursor::Default => CursorType::Pointer,
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
        if !self.chrome.omnibox_focused() {
            return None;
        }
        // In the field's text: the field's padding in.
        let field = self.omnibox_rect();
        let padding = self.chrome.tree.theme().size(SizeToken::FieldPadding);
        let chrome = &mut self.chrome;
        let area = chrome.tree.update(chrome.omnibox, |omnibox, ctx| {
            omnibox.ime_cursor_area(ctx.text())
        })?;
        let top = field.y + (field.height - area.height) / 2.0;
        Some(RectF::new(
            field.x + padding + area.x,
            top,
            area.width,
            area.height,
        ))
    }

    /// Draws the browser's frame if anything in it changed: the tab strip
    /// or the toolbar, the window's size, or a web view's surface.
    fn update(&mut self) {
        self.sync_ui();
        self.place_web_views();
        self.sync_focus();
        if self.size.is_empty() {
            return;
        }
        let scale = self.scale;
        let css = move |rect: Rect| rect.to_f32().scale(1.0 / scale);
        // The web views whose renderer has gone show their sad tab instead
        // (Chromium's `SadTabView`, over the contents).
        let sad: Vec<(Rect, f32)> = self
            .web_views
            .iter()
            .filter(|view| !view.bounds.is_empty() && self.crashed(view.contents))
            .map(|view| (view.bounds, view.radius))
            .collect();
        let popup_dirty = std::mem::take(&mut self.popup_dirty);
        // The UI's tree, painted where it changed (`None`: nothing did).
        let chrome_changed = match self.chrome.tree.paint() {
            Some(frame) => {
                self.chrome_invalidation = frame.layers[0].invalidation.clone();
                self.chrome_layers = frame.layers;
                true
            }
            None => false,
        };
        if chrome_changed || sad != self.sad_drawn || popup_dirty {
            let mut trees = PropertyTrees::default();
            let layers_now = &self.chrome_layers;
            self.chrome_layer_ids
                .retain(|owner, _| layers_now.iter().any(|layer| layer.owner == Some(*owner)));
            let mut layers = Vec::new();
            let floating = self.chrome.floating_panel();
            for layer in &self.chrome_layers {
                // Each list is read again only when it is a new one: the
                // same one keeps its tiles.
                let (display_list, invalidation) =
                    (layer.display_list.clone(), layer.invalidation.clone());
                match layer.owner {
                    // The window: the web views are drawn over it where the
                    // UI lays them out, and where no op draws its tiles
                    // aren't rastered.
                    None => layers.push(PictureLayer {
                        id: LayerId(1),
                        bounds: layer.size,
                        display_list,
                        contents_opaque: true,
                        background_color: Color::WHITE,
                        invalidation,
                        properties: LayerProperties::default(),
                    }),
                    // A view's scrolled children (`views::ScrollView`'s
                    // contents layer), moved by the offset and cut to the
                    // view; or a view painted to a layer of its own.
                    Some(owner) => {
                        let next = &mut self.next_chrome_layer_id;
                        let id = *self
                            .chrome_layer_ids
                            .entry(owner)
                            .or_insert_with(|| LayerId(next.get_next()));
                        let clip = ClipNodeId(trees.clip.len());
                        trees.clip.push(ClipNode {
                            parent: Some(ClipNodeId(0)),
                            transform: TransformNodeId(0),
                            rect: layer.clip,
                            radii: Default::default(),
                        });
                        layers.push(PictureLayer {
                            id,
                            bounds: layer.size,
                            display_list,
                            contents_opaque: false,
                            background_color: Color::TRANSPARENT,
                            invalidation,
                            properties: LayerProperties {
                                origin: layer.origin,
                                clip,
                                // The sidebar floated over the page, and
                                // what scrolls in it.
                                over_surfaces: floating
                                    .is_some_and(|panel| self.chrome.tree.is_within(owner, panel)),
                                ..Default::default()
                            },
                        });
                    }
                }
            }
            for (&(bounds, radius), id) in sad.iter().zip(100..) {
                let at = css(bounds);
                // A card's corners, as its web view's.
                let clip = ClipNodeId(trees.clip.len());
                trees.clip.push(ClipNode {
                    parent: Some(ClipNodeId(0)),
                    transform: TransformNodeId(0),
                    rect: Some(at),
                    radii: [SizeF::new(radius, radius); 4],
                });
                layers.push(PictureLayer {
                    id: LayerId(id),
                    bounds: at.size(),
                    display_list: Arc::new(self.sad_tab.paint(self.chrome.tree.text(), at.size())),
                    contents_opaque: true,
                    background_color: sad_tab::BACKGROUND,
                    invalidation: None,
                    properties: LayerProperties {
                        origin: PointF::new(at.x, at.y),
                        clip,
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
                    display_list: Arc::new(popup.paint(self.chrome.tree.text())),
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
            self.compositor.set_layers(layers, trees);
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
                    corner_radius: view.radius,
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

    /// machopo is ending: every tab's renderer is asked to exit, then
    /// waited for until one deadline, [`browser::shutdown::LIMIT`] after
    /// the asking; those that haven't exited by then are killed. Hung
    /// renderers take that long however many they are.
    pub fn shutdown(&mut self) {
        let asked = std::time::Instant::now();
        for contents in self.tabs.iter_mut().chain(&mut self.others) {
            contents.ask_to_shut_down();
        }
        let deadline = asked + browser::shutdown::LIMIT;
        for contents in self.tabs.iter_mut().chain(&mut self.others) {
            let _ = contents.wait_for_shutdown(asked, deadline);
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

/// A mouse button as the UI's events have it.
fn ui_button(button: MouseButton) -> ui::PointerButton {
    match button {
        MouseButton::Left => ui::PointerButton::Primary,
        MouseButton::Middle => ui::PointerButton::Middle,
        MouseButton::Right => ui::PointerButton::Secondary,
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
        assert_eq!(
            on(&char("R"), cmd | ModifiersState::SHIFT, true),
            Some(Command::ReloadBypassingCache)
        );
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
        assert_eq!(
            on(&char("R"), ctrl | ModifiersState::SHIFT, false),
            Some(Command::ReloadBypassingCache)
        );
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
        for bypass in [ModifiersState::SHIFT, ctrl] {
            assert_eq!(
                on(&Key::Named(NamedKey::F5), bypass, false),
                Some(Command::ReloadBypassingCache)
            );
        }
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
