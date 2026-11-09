//! The platform's CSS system colors, which the browser reads and hands
//! every renderer it starts (Chromium's `ui::ColorProvider`, whose
//! native color mixers read the system's).
//!
//! On Windows, the light mode's: the system colors `GetSysColor` gives,
//! which are kept in the registry (`HKEY_CURRENT_USER\Control Panel\Colors`),
//! mapped as Chromium's `MapNativeColorsToCssSystemColors` does. Elsewhere
//! (macOS isn't read yet; Linux's are Chromium's own) Blink's defaults.
//!
//! And how the caret blinks (Chromium's `ui::NativeTheme::
//! caret_blink_interval`): on Windows the system's (`GetCaretBlinkTime`,
//! which `Control Panel\Desktop`'s `CursorBlinkRate` keeps), elsewhere
//! Chromium's default (Chrome without GTK on Linux; macOS's NSUserDefaults
//! aren't read yet).

use std::time::Duration;

use gfx::SystemColors;

/// How long the caret stays shown, and then hidden, when nothing says
/// (Chromium's `kDefaultCaretBlinkInterval`).
pub const DEFAULT_CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

/// The system colors now, read once (a renderer started later reads the
/// setting then: Chromium tells running ones of a change, machopo doesn't).
pub fn platform() -> SystemColors {
    #[cfg(windows)]
    {
        if let Some(colors) = windows::read() {
            return colors;
        }
    }
    SystemColors::DEFAULT
}

/// What a window's renderers get: with the test fonts (the scenarios), a
/// web test's colors, the same on every machine; else the platform's.
pub fn for_window(test_fonts: bool) -> SystemColors {
    if test_fonts {
        SystemColors::DEFAULT
    } else {
        platform()
    }
}

/// How long the caret stays shown, and then hidden, as it blinks, read
/// now; `None` if it doesn't.
pub fn caret_blink_interval() -> Option<Duration> {
    #[cfg(windows)]
    {
        if let Some(rate) = windows::cursor_blink_rate() {
            return blink_interval(&rate);
        }
    }
    Some(DEFAULT_CARET_BLINK_INTERVAL)
}

/// What a window's renderers get: with the test fonts (the scenarios) or
/// for automation, no blinking (a web test's: each frame the same); else
/// the platform's.
pub fn caret_blink_interval_for_window(test_fonts: bool, automation: bool) -> Option<Duration> {
    if test_fonts || automation {
        None
    } else {
        caret_blink_interval()
    }
}

/// A `CursorBlinkRate` value: milliseconds, or `-1` (`GetCaretBlinkTime`'s
/// `INFINITE`) for none; Chromium's default for what `GetCaretBlinkTime`
/// fails on (0, or no number).
#[cfg_attr(not(windows), allow(dead_code))]
fn blink_interval(value: &str) -> Option<Duration> {
    match value.trim().parse::<i64>() {
        Ok(-1) => None,
        Ok(ms) if ms > 0 => Some(Duration::from_millis(ms.unsigned_abs())),
        _ => Some(DEFAULT_CARET_BLINK_INTERVAL),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_test_fonts_come_with_a_web_tests_colors() {
        assert_eq!(for_window(true), SystemColors::DEFAULT);
        assert_eq!(for_window(false), platform());
    }

    #[test]
    fn a_blink_rate_is_one_phase_and_minus_one_none() {
        assert_eq!(blink_interval("530"), Some(Duration::from_millis(530)));
        assert_eq!(blink_interval("-1"), None);
        assert_eq!(blink_interval("0"), Some(DEFAULT_CARET_BLINK_INTERVAL));
        assert_eq!(blink_interval("fast"), Some(DEFAULT_CARET_BLINK_INTERVAL));
        assert_eq!(caret_blink_interval_for_window(true, false), None);
        assert_eq!(caret_blink_interval_for_window(false, true), None);
    }
}

#[cfg(windows)]
mod windows {
    use gfx::{Color, SystemColors};

    /// The colors from the registry, Blink's defaults for what isn't
    /// there; `None` if the key isn't.
    pub(super) fn read() -> Option<SystemColors> {
        let key = windows_registry::CURRENT_USER
            .open(r"Control Panel\Colors")
            .ok()?;
        Some(from_values(|name| key.get_string(name).ok()))
    }

    /// `Control Panel\Desktop`'s `CursorBlinkRate`, if it is there.
    pub(super) fn cursor_blink_rate() -> Option<String> {
        let key = windows_registry::CURRENT_USER
            .open(r"Control Panel\Desktop")
            .ok()?;
        key.get_string("CursorBlinkRate").ok()
    }

    /// The colors from the values `value` gives by name (`"0 120 212"`).
    pub(super) fn from_values(value: impl Fn(&str) -> Option<String>) -> SystemColors {
        let defaults = SystemColors::DEFAULT;
        let color =
            |name: &str, default: Color| value(name).and_then(|v| parse(&v)).unwrap_or(default);
        let button_text = color("ButtonText", defaults.button_text);
        let window = color("Window", defaults.canvas);
        let window_text = color("WindowText", defaults.canvas_text);
        let hotlight = color("HotTrackingColor", defaults.link_text);
        SystemColors {
            button_face: color("ButtonFace", defaults.button_face),
            button_text,
            // Blink's provider gives `ButtonBorder` the button's text color.
            button_border: button_text,
            canvas: window,
            canvas_text: window_text,
            field: window,
            field_text: window_text,
            gray_text: color("GrayText", defaults.gray_text),
            highlight: color("Hilight", defaults.highlight),
            highlight_text: color("HilightText", defaults.highlight_text),
            inactive_highlight: defaults.inactive_highlight,
            inactive_highlight_text: defaults.inactive_highlight_text,
            link_text: hotlight,
            visited_text: hotlight,
            active_text: hotlight,
            // Not the provider's: Chrome's own.
            selected_item: defaults.selected_item,
            selected_item_text: defaults.selected_item_text,
        }
    }

    /// `"r g b"`, as the registry keeps a system color.
    fn parse(value: &str) -> Option<Color> {
        let mut channels = value.split_whitespace().map(|c| c.parse::<u8>().ok());
        let (r, g, b) = (channels.next()??, channels.next()??, channels.next()??);
        channels.next().is_none().then_some(Color::rgb(r, g, b))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_registry_values_map_as_chromium_maps_them() {
            // Windows 11's, as `Control Panel\Colors` has them.
            let values = [
                ("ButtonFace", "240 240 240"),
                ("ButtonText", "1 2 3"),
                ("GrayText", "109 109 109"),
                ("Hilight", "0 120 212"),
                ("HilightText", "255 255 254"),
                ("HotTrackingColor", "0 102 204"),
                ("Window", "255 255 253"),
                ("WindowText", "4 5 6"),
            ];
            let colors = from_values(|name| {
                values
                    .iter()
                    .find(|(n, _)| *n == name)
                    .map(|(_, v)| v.to_string())
            });
            assert_eq!(colors.button_face, Color::rgb(240, 240, 240));
            assert_eq!(colors.button_text, Color::rgb(1, 2, 3));
            assert_eq!(colors.button_border, Color::rgb(1, 2, 3));
            assert_eq!(colors.gray_text, Color::rgb(109, 109, 109));
            assert_eq!(colors.highlight, Color::rgb(0, 120, 212));
            assert_eq!(colors.highlight_text, Color::rgb(255, 255, 254));
            for link in [colors.link_text, colors.visited_text, colors.active_text] {
                assert_eq!(link, Color::rgb(0, 102, 204));
            }
            assert_eq!(
                (colors.canvas, colors.field),
                (Color::rgb(255, 255, 253), Color::rgb(255, 255, 253))
            );
            assert_eq!(
                (colors.canvas_text, colors.field_text),
                (Color::rgb(4, 5, 6), Color::rgb(4, 5, 6))
            );
            assert_eq!(colors.selected_item, SystemColors::DEFAULT.selected_item);
        }

        #[test]
        fn a_value_missing_or_malformed_is_the_default() {
            let colors = from_values(|name| match name {
                "ButtonFace" => Some("240 240".into()),
                "GrayText" => Some("1 2 3 4".into()),
                "Hilight" => Some("0 300 0".into()),
                _ => None,
            });
            let defaults = SystemColors::DEFAULT;
            assert_eq!(colors.button_face, defaults.button_face);
            assert_eq!(colors.gray_text, defaults.gray_text);
            assert_eq!(colors.highlight, defaults.highlight);
            assert_eq!(colors.link_text, defaults.link_text);
        }
    }
}
