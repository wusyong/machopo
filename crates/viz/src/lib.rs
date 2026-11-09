//! Viz service (`//components/viz/service`).
//!
//! Receives [`CompositorFrame`](protocol::viz::CompositorFrame)s from frame
//! sinks, aggregates the surfaces into one frame
//! ([`surface_aggregator`]) and hands it to a [`DirectRenderer`], which
//! draws it to the output: with wgpu ([`WgpuRenderer`], our `SkiaRenderer`
//! and `SkiaOutputSurface`), into a window's surface or, with no window
//! (headless, `--automation-pipe`, tests), a texture of its own, on the
//! process's one device ([`gpu_context`]). There is no software
//! compositing: without a GPU, wgpu's CPU adapter draws.
//!
//! In Chromium this lives in the GPU process. For the MVP it runs on a
//! dedicated thread in the browser ("VizCompositorThread"), matching
//! Chromium's in-process Viz configuration.

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

mod begin_frame_source;
mod display;
pub mod gpu_context;
mod rounded_corner;
pub mod surface_aggregator;
mod thread;
mod wgpu_renderer;

pub use begin_frame_source::{BeginFrameClient, BeginFrameSource};
pub use display::{CopyFailure, DirectRenderer, Display};
pub use surface_aggregator::AggregatedFrame;
pub use thread::spawn_viz_compositor_thread;
pub use wgpu_renderer::{GpuSharedImages, MakeSurface, WgpuRenderer};
