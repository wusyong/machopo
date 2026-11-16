//! What views get from the user: the pointer, keys and the input method,
//! in `ui`'s own types (Chromium's `//ui/events`). Whatever drives the tree
//! turns its platform's events into these, so `ui` depends on no windowing
//! library.

use gfx::PointF;

/// The modifier keys held. `meta` is Cmd on macOS, the Windows key
/// elsewhere.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Primary,
    Middle,
    Secondary,
}

/// What the pointer did, where: a view gets it at its own coordinates,
/// CSS px from its top left.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Move {
        position: PointF,
        modifiers: Modifiers,
    },
    /// The `click_count`th press of a series (double click is 2).
    Down {
        position: PointF,
        button: PointerButton,
        click_count: u32,
        modifiers: Modifiers,
    },
    Up {
        position: PointF,
        button: PointerButton,
        click_count: u32,
        modifiers: Modifiers,
    },
}

impl PointerEvent {
    pub fn position(&self) -> PointF {
        match *self {
            PointerEvent::Move { position, .. }
            | PointerEvent::Down { position, .. }
            | PointerEvent::Up { position, .. } => position,
        }
    }

    pub fn modifiers(&self) -> Modifiers {
        match *self {
            PointerEvent::Move { modifiers, .. }
            | PointerEvent::Down { modifiers, .. }
            | PointerEvent::Up { modifiers, .. } => modifiers,
        }
    }

    /// The same event at `position`.
    pub(crate) fn at(mut self, at: PointF) -> Self {
        match &mut self {
            PointerEvent::Move { position, .. }
            | PointerEvent::Down { position, .. }
            | PointerEvent::Up { position, .. } => *position = at,
        }
        self
    }
}

/// What a key means on the keyboard's layout, with the modifiers (the
/// DOM's `key`): a named key, or the characters it types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    Named(NamedKey),
    Character(String),
    /// A key the views have no name for.
    Unidentified,
}

/// The keys that don't type characters, as far as views use them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedKey {
    Enter,
    Escape,
    Tab,
    Backspace,
    Delete,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEvent {
    pub key: Key,
    /// What it types, if anything.
    pub text: Option<String>,
    pub modifiers: Modifiers,
    /// Down, or up.
    pub pressed: bool,
    /// Held down: one of the presses it repeats.
    pub repeat: bool,
}

/// The input method's: text being composed, or finished.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImeEvent {
    /// Text being composed, shown in place; empty when composing stops.
    /// `cursor` is the byte range selected in it, if any.
    Preedit {
        text: String,
        cursor: Option<(usize, usize)>,
    },
    /// Text the input method finished.
    Commit(String),
}

/// What the pointer should look like over a view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Cursor {
    #[default]
    Default,
    /// Over text that can be selected or edited: the I-beam.
    Text,
}

/// Whether a view used an event. One that didn't goes on to its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handled {
    Yes,
    No,
}

impl Handled {
    pub fn is_handled(self) -> bool {
        self == Handled::Yes
    }
}

/// The system clipboard (`ui::Clipboard`), plain text only. Whatever drives
/// the tree has the platform's and lends it.
pub trait Clipboard {
    fn read(&mut self) -> Option<String>;
    fn write(&mut self, text: &str);
}
