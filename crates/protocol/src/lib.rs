//! Messages exchanged between Browser, Renderer and Viz.
//!
//! This crate plays the role of Chromium's `.mojom` files: it only defines
//! data. Each module corresponds to one mojom interface family. Everything is
//! `serde`-serialisable so that, once the renderer is its own process, the
//! same messages can go over IPC instead of the in-process
//! `crossbeam-channel` channels without touching the payloads.
//!
//! | Module | Chromium counterpart |
//! | --- | --- |
//! | [`control_channel`] | the pipe a `mojo::core::Channel` is, and its `mojo::Remote`s |
//! | [`automation`] | the DevTools protocol (`browser_protocol.pdl`), between the browser and what drives it |
//! | [`renderer`] | `content/common/{widget,frame}.mojom`, `blink.mojom.Widget` |
//! | [`viz`] | `services/viz/public/mojom/compositing/*.mojom` |
//! | [`network`] | `services/network/public/mojom` |
//! | [`input`] | `blink.mojom.WidgetInputHandler`, `blink::WebInputEvent` |
//! | [`webdriver`] | (chromedriver's injected JS) WebDriver's work in the renderer |

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

pub mod automation;
mod big_buffer;
pub mod control_channel;
pub mod devtools_network;
pub mod header_list;
pub mod input;
pub mod network;
pub mod renderer;
pub mod viz;
pub mod webdriver;

pub use big_buffer::BigBuffer;
pub use ipc::shared_buffer::{SharedBuffer, Writer as SharedBufferWriter};
use serde::{Deserialize, Serialize};

/// Identifies one tab's renderer-side frame tree. Stand-in for the
/// `(process_id, routing_id)` pair Chromium uses before `FrameToken`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabId(pub u32);

/// A frame's id outside its renderer (Chromium's `blink::LocalFrameToken`,
/// a `base::UnguessableToken`): 128 random bits, a frame's own as it
/// navigates. The browser draws the main frame's, the renderer a child
/// frame's as it makes it; the browser's frame tree knows a tab's frames
/// by them. DevTools' and WebDriver's id of a frame is its token written
/// as 32 upper-case hex digits (`Display`), as Chrome's `frameId` is.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FrameToken(pub [u8; 16]);

impl FrameToken {
    /// The token `text` writes (`Display`'s 32 hex digits, of either
    /// case), if it is one.
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.as_bytes();
        if digits.len() != 32 {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (byte, pair) in bytes.iter_mut().zip(digits.as_chunks::<2>().0) {
            let pair = std::str::from_utf8(pair).ok()?;
            // `from_str_radix` takes a sign, which isn't a digit.
            if !pair.bytes().all(|digit| digit.is_ascii_hexdigit()) {
                return None;
            }
            *byte = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl std::fmt::Display for FrameToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02X}"))
    }
}

impl std::fmt::Debug for FrameToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FrameToken({self})")
    }
}
