//! Renderer (`//content/renderer`): the renderer main thread.
//!
//! Owns the event loop, the script engine and the main-thread half of the
//! compositor, and drives the rendering pipeline once per frame
//! (`BeginMainFrame`). The engine is whichever `ScriptEngineFactory` its
//! starter gives makes: the renderer only sees `dom_api::ScriptEngine`.
//!
//! A navigation's body is parsed by `html` into a document of the DOM,
//! stopping for its scripts (`render_thread::next`); each frame then runs
//! the document lifecycle (`style` -> `layout` -> `paint`, driven by
//! `FrameView`) and hands the display list to the compositor (tile,
//! raster, draw) -> `CompositorFrame` -> Viz.
//!
//! The renderer runs as a thread named `CrRendererMain`; moving it into its own
//! process should not change the message types. The thread waits on one
//! task queue (`task_queue`): the browser's messages arrive there as tasks,
//! sent through a [`RendererSender`]. The compositor's impl side runs on a
//! thread of its own (`Compositor`), which takes Viz's `BeginFrame`s and
//! the browser's input ([`CompositorSender`]), asks the main thread for
//! main frames, and passes the input on.

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

mod canvas;
mod canvas_encoder;
mod canvas_raster;
mod clock;
mod compositing;
mod document;
mod encoding;
mod frame_view;
mod idleness_detector;
mod initiator;
mod loader;
mod navigation_timing;
mod pending_user_input;
mod render_thread;
mod scroll;
mod scroll_anchor;
mod selection;
mod svg_image;
mod task_queue;
#[cfg(test)]
mod testing;

pub use compositor::{CompositorGone, CompositorSender};
pub use render_thread::{
    RendererEndpoints, RendererFlags, ScriptEngineFactory, spawn_renderer_main_thread,
};
pub use task_queue::{RendererGone, RendererReceiver, RendererSender, channel};
