//! machopo's own UI framework: what the browser's tab strip, toolbar and
//! address field are made of, and nothing of the browser itself.
//!
//! A [`ViewTree`] holds [`View`]s in an arena, named by generational ids
//! ([`ViewId`] for a view of a known type, [`AnyViewId`] for any). The tree
//! lays them out with taffy's algorithms through its low-level traits (each
//! node a flex or grid container, or a leaf a view measures), paints each
//! into a display list it keeps until the view asks to be painted again,
//! and puts them together into one layer with what changed since the last
//! frame. Events go to the view under the pointer (or the one that has
//! captured it, or the focused one for keys), then up its ancestors until
//! one handles them; views send what they mean as the app's messages.
//!
//! In the place of Chromium's `//ui/views`, but designed for machopo's own
//! goals rather than after Views: Views is one reference among others
//! (SwiftUI, Flutter, Masonry and Xilem, egui).

#![cfg_attr(
    test,
    allow(
        clippy::arithmetic_side_effects,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_possible_wrap,
        reason = "tests"
    )
)]

pub mod event;
mod id;
pub mod text;
mod theme;
mod tree;
mod view;

pub use event::{
    Clipboard, Cursor, Handled, ImeEvent, Key, KeyEvent, Modifiers, NamedKey, PointerButton,
    PointerEvent,
};
pub use id::{AnyViewId, ViewId};
pub use taffy;
pub use text::TextContexts;
pub use theme::{Token, TokenTable};
pub use tree::{Frame, FrameLayer, ViewTree};
pub use view::{App, Container, EventCtx, MeasureCtx, PaintCtx, View};

#[cfg(test)]
mod tests;
