//! The browser's theme: the colors and sizes its tab strip, toolbar and
//! address field paint with, as `ui`'s tokens (Chromium's `ColorId`s and
//! layout constants). One built-in theme, the light one.

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
    }
}

pub struct BrowserTheme {
    colors: TokenTable<ColorToken, Color>,
    sizes: TokenTable<SizeToken, f32>,
}

impl BrowserTheme {
    /// The light theme, after Chrome's.
    pub fn light() -> Self {
        Self {
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
            }),
            sizes: TokenTable::new(|token| match token {
                SizeToken::ToolbarHeight => 40.0,
                SizeToken::ToolbarPadding => 6.0,
                SizeToken::ToolbarButton | SizeToken::NewTabButton => 28.0,
                SizeToken::ToolbarGap | SizeToken::TabGap => 4.0,
                SizeToken::FieldHeight => 30.0,
                SizeToken::FieldPadding | SizeToken::TabTitlePadding => 12.0,
                SizeToken::FieldFontSize => 14.0,
                SizeToken::TabStripHeight => 34.0,
                SizeToken::TabStripPadding | SizeToken::TabRadius | SizeToken::TabCloseMargin => {
                    8.0
                }
                SizeToken::TabTop => 6.0,
                SizeToken::TabMinWidth => 56.0,
                SizeToken::TabMaxWidth => 240.0,
                SizeToken::TabCloseMinWidth => 100.0,
                SizeToken::TabFontSize => 12.0,
                SizeToken::TabClose => 16.0,
                SizeToken::TabFade => 24.0,
            }),
        }
    }

    pub fn color(&self, token: ColorToken) -> Color {
        *self.colors.get(token)
    }

    pub fn size(&self, token: SizeToken) -> f32 {
        *self.sizes.get(token)
    }
}
