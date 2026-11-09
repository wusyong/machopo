//! Input events the browser forwards to a renderer (`blink::WebInputEvent`,
//! through `blink.mojom.WidgetInputHandler`). Coordinates are the web
//! contents' CSS px.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum InputEvent {
    /// `WebMouseEvent` of type `kMouseMove`: the pointer moved to `x`, `y`,
    /// with `buttons` (the DOM's bits) down and `modifiers` held.
    MouseMove {
        x: f32,
        y: f32,
        buttons: u16,
        modifiers: Modifiers,
    },
    /// `kMouseDown`: `button` went down with the pointer at `x`, `y`, the
    /// `click_count`th press in a row (`WebMouseEvent::click_count`, which
    /// the platform layer counts); `buttons` are those down after it.
    MouseDown {
        x: f32,
        y: f32,
        button: MouseButton,
        buttons: u16,
        modifiers: Modifiers,
        click_count: u32,
    },
    /// `kMouseUp`: as `MouseDown`, with its press's `click_count`;
    /// `buttons` are those still down.
    MouseUp {
        x: f32,
        y: f32,
        button: MouseButton,
        buttons: u16,
        modifiers: Modifiers,
        click_count: u32,
    },
    /// `WebMouseWheelEvent`, from a wheel or a touchpad. `x` / `y`: where
    /// the pointer is, in the web contents' CSS px; with `buttons` down and
    /// `modifiers` held.
    MouseWheel {
        x: f32,
        y: f32,
        delta: ScrollDelta,
        buttons: u16,
        modifiers: Modifiers,
    },
    /// A key went down or up (`WebKeyboardEvent`); the page gets it unless
    /// it is one of the browser's shortcuts.
    Key(KeyboardEvent),
}

impl InputEvent {
    /// The pointer moving to `x`, `y` with no button down.
    pub fn mouse_move(x: f32, y: f32) -> Self {
        Self::MouseMove {
            x,
            y,
            buttons: 0,
            modifiers: Modifiers::NONE,
        }
    }

    /// A wheel at `x`, `y`, no button down nor key held.
    pub fn wheel(x: f32, y: f32, delta: ScrollDelta) -> Self {
        Self::MouseWheel {
            x,
            y,
            delta,
            buttons: 0,
            modifiers: Modifiers::NONE,
        }
    }

    /// `button` going down alone, the first press of a series.
    pub fn mouse_down(x: f32, y: f32, button: MouseButton, modifiers: Modifiers) -> Self {
        Self::MouseDown {
            x,
            y,
            button,
            buttons: button.buttons_bit(),
            modifiers,
            click_count: 1,
        }
    }

    /// `button` going up with none left down, after `mouse_down`.
    pub fn mouse_up(x: f32, y: f32, button: MouseButton, modifiers: Modifiers) -> Self {
        Self::MouseUp {
            x,
            y,
            button,
            buttons: 0,
            modifiers,
            click_count: 1,
        }
    }
}

/// `blink::WebKeyboardEvent` (Chromium's `NativeWebKeyboardEvent`, which the
/// platform layer fills in the browser process): what the page's
/// `KeyboardEvent` has, worked out from the platform's key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardEvent {
    /// Down (`kRawKeyDown`) or up (`kKeyUp`).
    pub down: bool,
    /// The DOM `key`: the character it types (`"a"`, `" "`), or the key's
    /// name (`"Enter"`, `"ArrowUp"`), `"Unidentified"` for one the platform
    /// doesn't name.
    pub key: String,
    /// The DOM `code`: the physical key (`"KeyA"`, `"Space"`).
    pub code: String,
    /// `windows_key_code`: the legacy `keyCode` (65 for A, 13 for Enter).
    pub key_code: u16,
    /// What it types, if anything (a `keypress`); empty for none.
    pub text: String,
    /// `KeyboardEvent.location`: 0 standard, 1 left, 2 right, 3 numpad.
    pub location: u8,
    /// Held down and repeating.
    pub repeat: bool,
    pub modifiers: Modifiers,
}

/// `WebInputEvent::Modifiers`: the keys held, as the platform reports
/// them (Cmd is `meta`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

impl Modifiers {
    pub const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    };
}

/// `WebPointerProperties::Button`. Back and forward are the browser's
/// (they go through history), so they never reach the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseButton {
    /// The primary button, usually the left one.
    Left,
    Middle,
    Right,
}

impl MouseButton {
    /// Its bit in `MouseEvent.buttons`: 1 the primary, 2 the secondary
    /// (right), 4 the auxiliary (middle).
    pub fn buttons_bit(self) -> u16 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 4,
        }
    }
}

/// How far to scroll: positive is down and right, the way the content's
/// scroll offset grows.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ScrollDelta {
    /// CSS px (touchpads).
    Pixels { x: f32, y: f32 },
    /// Lines (wheels); the renderer decides how far a line is.
    Lines { x: f32, y: f32 },
}
