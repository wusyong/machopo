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

mod bad_message;
mod debug_urls;
mod frame_tree;
pub mod loader;
mod navigation_controller;
mod navigation_request;
mod profile;
mod render_process_host;
mod unique_name;
mod user_agent;
mod web_contents;

pub use bad_message::BadMessageReason;
pub use frame_tree::FrameTree;
pub use navigation_controller::{FrameNavigationEntry, NavigationController, NavigationEntry};
pub use net::{HostMapping, HostMappingRules, NetworkContext, NetworkContextParams};
pub use profile::{Platform, Profile, ProfileError, default_user_data_dir, user_data_dir_on};
pub use render_process_host::{RenderProcessHost, RendererId};
pub use renderer::{NEW_CONTENT_RENDERING_DELAY, ParserBudget, RendererFlags, ScriptEngineFactory};
pub use user_agent::user_agent;
pub use web_contents::{
    ContentsEvent, ContentsEventCallback, MAX_TITLE_CHARS, NavigationStarted, OpenUrlParams,
    WebContents,
};
