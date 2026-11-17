//! The browser's theme: the colors and sizes its tab strip, toolbar and
//! address field paint with, as `ui`'s tokens (Chromium's `ColorId`s and
//! layout constants). Two built-in themes, light and dark, with the same
//! sizes.

use gfx::Color;
use ui::TokenTable;

ui::tokens! {
    /// The browser UI's colors.
    pub enum ColorToken {
        /// The toolbar, and the active tab joined to it.
        ToolbarBackground,
        /// The line under the toolbar.
        ToolbarSeparator,
        /// The toolbar's buttons and the tab strip's.
        Icon,
        IconDisabled,
        /// Under a toolbar button the pointer is over.
        ButtonHover,
        /// The address field while it hasn't the keys.
        Field,
        FieldFocused,
        FocusRing,
        FieldText,
        FieldSelection,
        /// The tab strip, and the tabs not active.
        TabStrip,
        TabHover,
        /// Under a tab's close button, or the new tab button, the pointer is
        /// over.
        TabButtonHover,
        TabTitle,
        /// Between two tabs neither of them active nor hovered.
        TabSeparator,
        /// The sidebar, the vertical tabs' background.
        SidebarBackground,
        /// Under a row of the sidebar the pointer is over.
        SidebarRowHover,
        /// Under the active tab's row.
        SidebarTabActive,
        /// The line where the sidebar is dragged wider or narrower.
        SidebarEdge,
    }
}

ui::tokens! {
    /// The browser UI's sizes, CSS px.
    pub enum SizeToken {
        ToolbarHeight,
        /// Before the first toolbar button and after the address field.
        ToolbarPadding,
        ToolbarButton,
        /// Between two toolbar buttons, and once more before the field.
        ToolbarGap,
        FieldHeight,
        /// Between the field's edge and its text.
        FieldPadding,
        FieldFontSize,
        TabStripHeight,
        /// Before the first tab.
        TabStripPadding,
        /// Above the tabs.
        TabTop,
        TabMinWidth,
        TabMaxWidth,
        /// Narrower, a tab shows its close button only while active.
        TabCloseMinWidth,
        TabRadius,
        /// Between a tab's edge and its title.
        TabTitlePadding,
        TabFontSize,
        TabClose,
        /// Between a tab's close button and its right edge.
        TabCloseMargin,
        NewTabButton,
        /// Between the tabs and the new tab button.
        TabGap,
        /// How long a title too long for its tab fades out over.
        TabFade,
        /// The sidebar's width at first.
        SidebarWidth,
        /// How narrow and how wide it can be dragged (and at most half the
        /// window).
        SidebarMinWidth,
        SidebarMaxWidth,
        /// Around the sidebar's content, and between its parts.
        SidebarPadding,
        /// A row of the sidebar: a tab, or the new tab row.
        SidebarRowHeight,
        SidebarRowRadius,
        SidebarFontSize,
        /// The edge dragged to resize the sidebar, in the gap right of it;
        /// and, collapsed, the strip at the window's left edge that floats
        /// it out.
        SidebarEdgeWidth,
        /// Around the page beside the sidebar: it is a card.
        CardGap,
        CardRadius,
    }
}

/// Light or dark (Chromium's `ui::ColorProviderKey::ColorMode`): the
/// system's, unless the user picks one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorMode {
    #[default]
    Light,
    Dark,
}

pub struct BrowserTheme {
    mode: ColorMode,
    colors: TokenTable<ColorToken, Color>,
    sizes: TokenTable<SizeToken, f32>,
}

impl BrowserTheme {
    pub fn new(mode: ColorMode) -> Self {
        match mode {
            ColorMode::Light => Self::light(),
            ColorMode::Dark => Self::dark(),
        }
    }

    /// The light theme, after Chrome's before its 2023 refresh in Google's
    /// greys, a shade darker: its toolbar is white, this one's
    /// `kGoogleGrey100` (its address field's), the field a shade darker
    /// still.
    pub fn light() -> Self {
        Self {
            mode: ColorMode::Light,
            colors: TokenTable::new(|token| match token {
                ColorToken::ToolbarBackground => Color::rgb(241, 243, 244),
                ColorToken::ToolbarSeparator => Color::rgb(218, 220, 224),
                ColorToken::Icon => Color::rgb(95, 99, 104),
                ColorToken::IconDisabled => Color::rgb(189, 193, 198),
                ColorToken::ButtonHover | ColorToken::Field => Color::rgb(222, 225, 229),
                ColorToken::FieldFocused => Color::WHITE,
                ColorToken::FocusRing => Color::rgb(26, 115, 232),
                ColorToken::FieldText | ColorToken::TabTitle => Color::rgb(32, 33, 36),
                ColorToken::FieldSelection => Color::rgb(198, 218, 252),
                ColorToken::TabStrip => Color::rgb(222, 225, 230),
                ColorToken::TabHover => Color::rgb(232, 234, 237),
                ColorToken::TabButtonHover => Color::rgb(204, 207, 212),
                ColorToken::TabSeparator => Color::rgb(160, 164, 170),
                // The toolbar's, as the sidebar takes its place.
                ColorToken::SidebarBackground => Color::rgb(241, 243, 244),
                ColorToken::SidebarRowHover => Color::rgb(226, 229, 233),
                ColorToken::SidebarTabActive => Color::WHITE,
                ColorToken::SidebarEdge => Color::rgb(218, 220, 224),
            }),
            sizes: sizes(),
        }
    }

    /// The dark theme, the other side of the light one: Chrome's dark mode
    /// before its 2023 refresh (`ThemeProperties`' dark colors and the
    /// color mixers'). Where Chrome has no color of its own (a hover, a
    /// separator, the sidebar), the light theme's blend of the same two
    /// colors.
    pub fn dark() -> Self {
        Self {
            mode: ColorMode::Dark,
            colors: TokenTable::new(|token| match token {
                // `kColorToolbar`, and the active tab's.
                ColorToken::ToolbarBackground => Color::rgb(53, 54, 58),
                // `kColorSeparator`: `kGoogleGrey800`.
                ColorToken::ToolbarSeparator => Color::rgb(60, 64, 67),
                // `kChromeIconGrey` tinted by `TINT_BUTTONS`: `kGoogleGrey100`.
                ColorToken::Icon => Color::rgb(241, 243, 244),
                // The icon at `kDisabledControlAlpha` over the toolbar.
                ColorToken::IconDisabled => Color::rgb(125, 126, 129),
                // The icon at 12% over the toolbar.
                ColorToken::ButtonHover => Color::rgb(76, 77, 80),
                // `kColorOmniboxBackground`: `kGoogleGrey900`, unfocused or
                // not.
                ColorToken::Field | ColorToken::FieldFocused => Color::rgb(32, 33, 36),
                // `kColorAccent`: `kGoogleBlue300`.
                ColorToken::FocusRing => Color::rgb(138, 180, 248),
                // `kColorOmniboxText` and `kColorToolbarText`.
                ColorToken::FieldText | ColorToken::TabTitle => Color::WHITE,
                // `kColorTextSelectionBackground`: the accent at
                // `kGoogleGreyAlpha500` over the field.
                ColorToken::FieldSelection => Color::rgb(78, 96, 127),
                // `kColorFrameActive`: `kGoogleGrey900`.
                ColorToken::TabStrip => Color::rgb(32, 33, 36),
                // Half way to the active tab.
                ColorToken::TabHover => Color::rgb(42, 44, 47),
                // White at 18% over the strip: seen over the active tab too.
                ColorToken::TabButtonHover => Color::rgb(72, 73, 76),
                // White at 32% over the strip.
                ColorToken::TabSeparator => Color::rgb(103, 104, 106),
                // The toolbar's, as the sidebar takes its place.
                ColorToken::SidebarBackground => Color::rgb(53, 54, 58),
                // White at 7% over the sidebar.
                ColorToken::SidebarRowHover => Color::rgb(67, 68, 72),
                // White at 14% over the sidebar: lighter than the rows, as
                // the light theme's.
                ColorToken::SidebarTabActive => Color::rgb(81, 82, 86),
                ColorToken::SidebarEdge => Color::rgb(60, 64, 67),
            }),
            sizes: sizes(),
        }
    }

    pub fn mode(&self) -> ColorMode {
        self.mode
    }

    pub fn color(&self, token: ColorToken) -> Color {
        *self.colors.get(token)
    }

    pub fn size(&self, token: SizeToken) -> f32 {
        *self.sizes.get(token)
    }
}

/// Both themes': only the colors differ.
fn sizes() -> TokenTable<SizeToken, f32> {
    TokenTable::new(|token| match token {
        SizeToken::ToolbarHeight => 40.0,
        SizeToken::ToolbarPadding => 6.0,
        SizeToken::ToolbarButton | SizeToken::NewTabButton => 28.0,
        SizeToken::ToolbarGap | SizeToken::TabGap => 4.0,
        SizeToken::FieldHeight => 30.0,
        SizeToken::FieldPadding | SizeToken::TabTitlePadding => 12.0,
        SizeToken::FieldFontSize => 14.0,
        SizeToken::TabStripHeight => 34.0,
        SizeToken::TabStripPadding | SizeToken::TabRadius | SizeToken::TabCloseMargin => 8.0,
        SizeToken::TabTop => 6.0,
        SizeToken::TabMinWidth => 56.0,
        SizeToken::TabMaxWidth => 240.0,
        SizeToken::TabCloseMinWidth => 100.0,
        SizeToken::TabFontSize => 12.0,
        SizeToken::TabClose => 16.0,
        SizeToken::TabFade => 24.0,
        SizeToken::SidebarWidth => 260.0,
        SizeToken::SidebarMinWidth => 200.0,
        SizeToken::SidebarMaxWidth => 600.0,
        SizeToken::SidebarPadding => 8.0,
        SizeToken::SidebarRowHeight => 36.0,
        SizeToken::SidebarRowRadius => 8.0,
        SizeToken::SidebarFontSize => 13.0,
        SizeToken::SidebarEdgeWidth => 4.0,
        SizeToken::CardGap | SizeToken::CardRadius => 8.0,
    })
}
