//! Browser-side content layer (`//content/browser`).
//!
//! [`WebContents`] is one tab. It owns a [`RenderProcessHost`] (for now a
//! renderer *thread*), allocates `LocalSurfaceId`s when the tab's geometry
//! changes, and forwards navigations. Messages coming back from the renderer
//! arrive on an IO thread and are handed to the embedder's callback, which
//! posts them to the UI thread — the same hop Chromium makes.

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

mod debug_urls;
pub mod loader;
mod navigation_controller;
mod render_process_host;
mod user_agent;
mod web_contents;

pub use navigation_controller::{NavigationController, NavigationEntry};
pub use net::{HostMapping, HostMappingRules, NetworkContext, NetworkContextParams};
pub use render_process_host::RenderProcessHost;
pub use renderer::{RendererFlags, ScriptEngineFactory};
pub use user_agent::user_agent;
pub use web_contents::{
    ContentsEvent, ContentsEventCallback, MAX_TITLE_CHARS, OpenUrlParams, WebContents,
};
