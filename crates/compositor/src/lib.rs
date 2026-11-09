//! Chromium-style compositor (`//cc`).
//!
//! The main thread owns a [`LayerTreeHost`] (the "main-side" tree that Blink
//! updates). A *commit* copies it into the impl side, [`LayerTreeHostImpl`],
//! which tiles and rasters each layer and finally *draws*: it emits a
//! [`protocol::viz::CompositorFrame`] of tile quads and submits it through a
//! [`LayerTreeFrameSink`] to Viz.
//!
//! How the two halves talk is the host's proxy, as in Chromium (`cc::Proxy`),
//! but a type parameter. [`SingleThreadProxy`] runs the impl side on the same
//! thread, synchronously: the browser's UI draws with it. [`ProxyMain`] hands
//! it to a compositor thread (the renderer's), which waits on one
//! queue ([`channel`]): Viz's `BeginFrame`s, the browser's input, and the
//! main thread's commits. What it can't handle itself goes back to the main
//! thread as a [`ToMainThread`].
//!
//! Scrolling's math is here too ([`ScrollNode`], [`key_scroll_delta`],
//! [`wheel_scroll_delta`]): the compositor scrolls the viewport and the
//! containers that are layers with it, and the renderer's main thread the
//! wheels passed on to it.
//! The impl side builds each commit into a pending tree, rasters it and
//! activates it.

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

#[cfg(test)]
mod fake_gpu;
mod frame_sink;
mod input_handler;
mod layer;
mod layer_tree_host;
mod layer_tree_host_impl;
pub mod mojo_embedder;
mod property_trees;
mod proxy_impl;
mod proxy_main;
mod scroll_node;
mod scroll_utils;
mod single_thread_proxy;
mod swap_promise;
mod synced_scroll_offset;

pub use frame_sink::LayerTreeFrameSink;
pub use layer::{Layer, LayerId, LayerProperties, PictureLayer, SurfaceLayer, TextureLayer};
pub use layer_tree_host::{DocumentId, LayerTreeHost, LayerTreeSettings};
pub use layer_tree_host_impl::LayerTreeHostImpl;
pub use property_trees::{
    AttributedFrame, AttributionRect, AttributionSpace, ClipNode, ClipNodeId, ElementId,
    FrameAttributionRegion, PropertyTrees, ScrollNodeId, ScrollOffsets, ScrollTreeNode,
    StickyPositionConstraint, StickyRegions, TransformNode, TransformNodeId,
};
pub use proxy_impl::{CompositorScrollDelta, InputAttribution, InputDispatch, ToMainThread};
pub use proxy_main::{CompositorGone, CompositorReceiver, CompositorSender, ProxyMain, channel};
pub use scroll_node::ScrollNode;
pub use scroll_utils::{Key as ScrollKey, key_scroll_delta, wheel_scroll_delta};
pub use single_thread_proxy::SingleThreadProxy;
pub use swap_promise::SwapPromise;
