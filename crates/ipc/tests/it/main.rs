//! `ipc`'s integration tests, one binary, as every crate's: a new test file
//! joins it (a module here) instead of being a binary of its own.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod codec;
mod data_pipe;
mod endpoint;
mod endpoint_fuzz;
mod frame_fuzz;
#[cfg(target_os = "macos")]
mod mach_channel;
mod pipe_fuzz;
// Wherever there is a platform's channel: macOS, for now.
#[cfg(target_os = "macos")]
mod process_channel;
mod region_fuzz;
#[cfg(target_os = "macos")]
mod rendezvous;
mod time_ticks;
