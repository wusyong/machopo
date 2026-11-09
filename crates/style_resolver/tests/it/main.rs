//! `style_resolver`'s integration tests, one binary: a change in `style_resolver` or below links this
//! once, not once per file, and a new test file joins it (a module here).
//! What reads no document: declarations and sheets as parsed and written
//! back, easing functions, animation types.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod cssom;
mod fonts;
mod interpolation;
mod keyframes;
mod stylesheet;
mod svg;
mod tables;
mod transitions;
