//! `protocol`'s integration tests, one binary: a change in `protocol` or below links this
//! once, not once per file, and a new test file joins it (a module here).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod automation;
mod boundary;
mod network;
