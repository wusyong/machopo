//! The page's key events from the platform's (Chromium's `ui::KeyEvent`
//! turned into a `NativeWebKeyboardEvent` in the browser process): the DOM
//! `key` and `code`, the legacy `keyCode`, the text it types and the
//! modifiers, worked out here so the renderer never sees the platform's
//! keys.

use protocol::input::{KeyboardEvent, Modifiers};
use winit::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey};

/// A key going down or up, as the platform (winit) reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    /// What the key means on the keyboard's layout, with the modifiers.
    pub logical: Key,
    /// Which key it is on the keyboard.
    pub physical: PhysicalKey,
    pub location: KeyLocation,
    /// What it types, if anything.
    pub text: Option<String>,
    pub pressed: bool,
    pub repeat: bool,
}

impl KeyInput {
    /// A key that went down, as the scenarios and the tests press it: its
    /// physical key from what it means (`code_for`).
    pub fn press(logical: Key, text: Option<String>) -> Self {
        let physical = PhysicalKey::Code(code_for(&logical));
        Self {
            logical,
            physical,
            location: KeyLocation::Standard,
            text,
            pressed: true,
            repeat: false,
        }
    }

    /// The same key going up.
    pub fn release(&self) -> Self {
        Self {
            text: None,
            pressed: false,
            repeat: false,
            ..self.clone()
        }
    }
}

/// The page's event for `input`, with `modifiers` held.
pub fn keyboard_event(input: &KeyInput, modifiers: ModifiersState) -> KeyboardEvent {
    let key = dom_key(&input.logical);
    let code = dom_code(&input.physical);
    let key_code = key_code(&key, &code);
    KeyboardEvent {
        down: input.pressed,
        key,
        code,
        key_code,
        text: input
            .text
            .clone()
            .filter(|_| input.pressed)
            .unwrap_or_default(),
        location: match input.location {
            KeyLocation::Standard => 0,
            KeyLocation::Left => 1,
            KeyLocation::Right => 2,
            KeyLocation::Numpad => 3,
        },
        repeat: input.repeat,
        modifiers: Modifiers {
            shift: modifiers.shift_key(),
            ctrl: modifiers.control_key(),
            alt: modifiers.alt_key(),
            meta: modifiers.super_key(),
        },
    }
}

/// The DOM `key`: a character, or a named key's name (winit's names are
/// the DOM's, but for the space bar, a character, and Cmd and the Windows
/// key, which the DOM calls `Meta`).
fn dom_key(key: &Key) -> String {
    match key {
        Key::Character(text) => text.to_string(),
        Key::Named(NamedKey::Space) => " ".to_owned(),
        Key::Named(NamedKey::Super) => "Meta".to_owned(),
        Key::Named(named) => format!("{named:?}"),
        Key::Dead(_) => "Dead".to_owned(),
        Key::Unidentified(_) => "Unidentified".to_owned(),
    }
}

/// The DOM `code`: winit's names are the UI Events' `code` values, but for
/// the Cmd and Windows keys (`MetaLeft`, `MetaRight` in the DOM).
fn dom_code(key: &PhysicalKey) -> String {
    match key {
        PhysicalKey::Code(KeyCode::SuperLeft) => "MetaLeft".to_owned(),
        PhysicalKey::Code(KeyCode::SuperRight) => "MetaRight".to_owned(),
        PhysicalKey::Code(code) => format!("{code:?}"),
        PhysicalKey::Unidentified(_) => String::new(),
    }
}

/// The Windows virtual key code that is `keyCode` (Chromium's
/// `ui::KeyboardCode`): a numpad digit's own (`VK_NUMPAD0` on), else from
/// what a letter or digit types, so it follows the layout (Chromium's
/// `KeyboardCodeFromCharCode`), else from the named key, else from the
/// physical key (`ui::KeyboardCodeFromDomCode`).
fn key_code(key: &str, code: &str) -> u16 {
    // With Num Lock off the key is a named one (`Home`, `End`...).
    if let Some(digit) = code
        .strip_prefix("Numpad")
        .and_then(|d| d.parse::<u16>().ok())
        .filter(|_| key.len() == 1 && key.as_bytes()[0].is_ascii_digit())
    {
        // Only `Numpad0` to `Numpad9` come: saturating changes nothing.
        return digit.saturating_add(96);
    }
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphabetic() {
            return c.to_ascii_uppercase() as u16;
        }
        if c.is_ascii_digit() || c == ' ' {
            return c as u16;
        }
    }
    let named = match key {
        "Backspace" => 8,
        "Tab" => 9,
        "Enter" => 13,
        "Shift" => 16,
        "Control" => 17,
        "Alt" => 18,
        "Pause" => 19,
        "CapsLock" => 20,
        "Escape" => 27,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Insert" => 45,
        "Delete" => 46,
        "Meta" => 91,
        "ContextMenu" => 93,
        "NumLock" => 144,
        "ScrollLock" => 145,
        _ => 0,
    };
    if named != 0 {
        return named;
    }
    if let Some(n) = key
        .strip_prefix('F')
        .and_then(|n| n.parse::<u16>().ok())
        .filter(|n| (1..=24).contains(n))
    {
        #[expect(clippy::arithmetic_side_effects, reason = "n <= 24")]
        let code = 111 + n;
        return code;
    }
    match code {
        _ if code.starts_with("Key") && code.len() == 4 => code.as_bytes()[3] as u16,
        _ if code.starts_with("Digit") && code.len() == 6 => code.as_bytes()[5] as u16,
        "NumpadMultiply" => 106,
        "NumpadAdd" => 107,
        "NumpadSubtract" => 109,
        "NumpadDecimal" => 110,
        "NumpadDivide" => 111,
        "Semicolon" => 186,
        "Equal" => 187,
        "Comma" => 188,
        "Minus" => 189,
        "Period" => 190,
        "Slash" => 191,
        "Backquote" => 192,
        "BracketLeft" => 219,
        "Backslash" => 220,
        "BracketRight" => 221,
        "Quote" => 222,
        _ => 0,
    }
}

/// The physical key that types `key` on a US layout: for the keys the
/// scenarios and the tests press (a platform window reports its own).
pub fn code_for(key: &Key) -> KeyCode {
    let named = |named: &NamedKey| match named {
        NamedKey::Enter => KeyCode::Enter,
        NamedKey::Tab => KeyCode::Tab,
        NamedKey::Space => KeyCode::Space,
        NamedKey::Backspace => KeyCode::Backspace,
        NamedKey::Delete => KeyCode::Delete,
        NamedKey::Escape => KeyCode::Escape,
        NamedKey::ArrowUp => KeyCode::ArrowUp,
        NamedKey::ArrowDown => KeyCode::ArrowDown,
        NamedKey::ArrowLeft => KeyCode::ArrowLeft,
        NamedKey::ArrowRight => KeyCode::ArrowRight,
        NamedKey::PageUp => KeyCode::PageUp,
        NamedKey::PageDown => KeyCode::PageDown,
        NamedKey::Home => KeyCode::Home,
        NamedKey::End => KeyCode::End,
        NamedKey::F5 => KeyCode::F5,
        NamedKey::F6 => KeyCode::F6,
        _ => KeyCode::Fn,
    };
    match key {
        Key::Named(key) => named(key),
        Key::Character(text) => match text.chars().next().map(|c| c.to_ascii_lowercase()) {
            Some(c @ 'a'..='z') => [
                KeyCode::KeyA,
                KeyCode::KeyB,
                KeyCode::KeyC,
                KeyCode::KeyD,
                KeyCode::KeyE,
                KeyCode::KeyF,
                KeyCode::KeyG,
                KeyCode::KeyH,
                KeyCode::KeyI,
                KeyCode::KeyJ,
                KeyCode::KeyK,
                KeyCode::KeyL,
                KeyCode::KeyM,
                KeyCode::KeyN,
                KeyCode::KeyO,
                KeyCode::KeyP,
                KeyCode::KeyQ,
                KeyCode::KeyR,
                KeyCode::KeyS,
                KeyCode::KeyT,
                KeyCode::KeyU,
                KeyCode::KeyV,
                KeyCode::KeyW,
                KeyCode::KeyX,
                KeyCode::KeyY,
                KeyCode::KeyZ,
            ][offset(c, 'a')],
            Some(c @ '0'..='9') => [
                KeyCode::Digit0,
                KeyCode::Digit1,
                KeyCode::Digit2,
                KeyCode::Digit3,
                KeyCode::Digit4,
                KeyCode::Digit5,
                KeyCode::Digit6,
                KeyCode::Digit7,
                KeyCode::Digit8,
                KeyCode::Digit9,
            ][offset(c, '0')],
            Some('.') => KeyCode::Period,
            Some(',') => KeyCode::Comma,
            Some('/') => KeyCode::Slash,
            Some('-') => KeyCode::Minus,
            Some('=' | '+') => KeyCode::Equal,
            Some(':' | ';') => KeyCode::Semicolon,
            _ => KeyCode::Fn,
        },
        _ => KeyCode::Fn,
    }
}

/// How far `c` is from `from`: `c` is one of a range starting at `from`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "c is matched in a range from `from`, so it is at or after it"
)]
fn offset(c: char, from: char) -> usize {
    (u32::from(c) - u32::from(from)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(logical: Key, physical: KeyCode, text: Option<&str>) -> KeyboardEvent {
        let input = KeyInput {
            logical,
            physical: PhysicalKey::Code(physical),
            location: KeyLocation::Standard,
            text: text.map(str::to_owned),
            pressed: true,
            repeat: false,
        };
        keyboard_event(&input, ModifiersState::SHIFT)
    }

    /// `key`, `code`, `keyCode` and the text, for the kinds of key: a
    /// letter (its `keyCode` from what it types), the space bar, named
    /// keys, Cmd, a punctuation key, a numpad digit, a function key.
    #[test]
    fn the_dom_names_and_codes() {
        let a = event(Key::Character("A".into()), KeyCode::KeyA, Some("A"));
        assert_eq!(
            (&*a.key, &*a.code, a.key_code, &*a.text),
            ("A", "KeyA", 65, "A")
        );
        assert!(a.modifiers.shift && a.down);
        // AZERTY's A key types `q`: `keyCode` follows what it types.
        let q = event(Key::Character("q".into()), KeyCode::KeyA, Some("q"));
        assert_eq!((&*q.key, &*q.code, q.key_code), ("q", "KeyA", 81));
        let space = event(Key::Named(NamedKey::Space), KeyCode::Space, Some(" "));
        assert_eq!(
            (&*space.key, &*space.code, space.key_code),
            (" ", "Space", 32)
        );
        for (named, code, key, key_code) in [
            (NamedKey::Enter, KeyCode::Enter, "Enter", 13),
            (NamedKey::Tab, KeyCode::Tab, "Tab", 9),
            (NamedKey::ArrowDown, KeyCode::ArrowDown, "ArrowDown", 40),
            (NamedKey::Escape, KeyCode::Escape, "Escape", 27),
            (NamedKey::Super, KeyCode::SuperLeft, "Meta", 91),
            (NamedKey::F5, KeyCode::F5, "F5", 116),
        ] {
            let e = event(Key::Named(named), code, None);
            assert_eq!((&*e.key, e.key_code), (key, key_code), "{key}");
        }
        let meta = event(Key::Named(NamedKey::Super), KeyCode::SuperLeft, None);
        assert_eq!(meta.code, "MetaLeft");
        let comma = event(Key::Character(",".into()), KeyCode::Comma, Some(","));
        assert_eq!((&*comma.code, comma.key_code), ("Comma", 188));
        let numpad = event(Key::Character("7".into()), KeyCode::Numpad7, Some("7"));
        assert_eq!((&*numpad.code, numpad.key_code), ("Numpad7", 103));
        // Without Num Lock, the key it means.
        let home = event(Key::Named(NamedKey::Home), KeyCode::Numpad7, None);
        assert_eq!((&*home.key, home.key_code), ("Home", 36));
    }

    /// A key going up types nothing, even with text from the platform.
    #[test]
    fn a_release_types_nothing() {
        let press = KeyInput::press(Key::Character("x".into()), Some("x".into()));
        assert_eq!(press.physical, PhysicalKey::Code(KeyCode::KeyX));
        assert_eq!(press.release().text, None);
        let release = KeyInput {
            pressed: false,
            ..press
        };
        let up = keyboard_event(&release, ModifiersState::empty());
        assert!(!up.down);
        assert_eq!((&*up.key, &*up.code, &*up.text), ("x", "KeyX", ""));
    }
}
