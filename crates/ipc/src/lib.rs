//! The process boundary (Chromium's `ipc/` and `mojo/`): what machopo's
//! processes share. The clock every process reads, [`TimeTicks`], and data
//! pipes ([`data_pipe`]: a response's body from the network service to
//! where it is used), for now within one process; and the channels' core
//! ([`core`]: what a channel does with the bytes and handles a platform
//! reads and writes), with no platform under it yet.
//!
//! `unsafe` is allowed here (and in `dom` and `bindings`): the Windows API
//! the clock reads is FFI. Each use says why it is sound; the crate's
//! README lists them.

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
#![allow(unsafe_code, reason = "the Windows API the clock reads")]

pub mod core;
pub mod data_pipe;
mod time_ticks;

pub use time_ticks::TimeTicks;
