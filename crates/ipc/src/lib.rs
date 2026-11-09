//! The process boundary (Chromium's `ipc/` and `mojo/`): what machopo's
//! processes share. For now the clock every process reads, [`TimeTicks`];
//! the channels and shared memory come once the renderer is a process of
//! its own.
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

mod time_ticks;

pub use time_ticks::TimeTicks;
