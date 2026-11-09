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
pub mod devtools_network;
pub mod header_list;
pub mod input;
pub mod network;
pub mod renderer;
pub mod viz;
pub mod webdriver;

use serde::{Deserialize, Serialize};

/// Identifies one tab's renderer-side frame tree. Stand-in for the
/// `(process_id, routing_id)` pair Chromium uses before `FrameToken`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TabId(pub u32);
