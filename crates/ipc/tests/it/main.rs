//! `ipc`'s integration tests, one binary, as every crate's: a new test file
//! joins it (a module here) instead of being a binary of its own.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod time_ticks;
