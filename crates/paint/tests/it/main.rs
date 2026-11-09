//! `paint`'s integration tests, one binary: a change in `paint` or below links this
//! once, not once per file, and a new test file joins it (a module here).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod artifact;
mod child_frames;
mod geometry;
mod hit_test;
mod intersection;
mod layerize;
mod page;
mod paint_controller;
mod raster_invalidation;
mod svg;
mod tables;
mod theme;

/// Viz's texture limit, `protocol::viz::MAX_TEXTURE_SIZE`: the biggest
/// canvas that is a layer of its own.
pub const MAX_TEXTURE_SIZE: u32 = 8192;
