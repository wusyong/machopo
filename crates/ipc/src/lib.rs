//! The process boundary (Chromium's `ipc/` and `mojo/`): what machopo's
//! processes share, and what goes between them. The clock every process
//! reads, [`TimeTicks`]; data pipes ([`data_pipe`]: a response's body from
//! the network service to where it is used, across a channel too); the
//! channels' core ([`core`]: what a channel does with the bytes and
//! handles a platform reads and writes, and with the memory two processes
//! share); a message's value to and from its bytes ([`codec`]); one end of
//! a channel whole, and a process's IO thread ([`endpoint`]); bytes that
//! cross in shared memory uncopied ([`shared_buffer`]); and each operating
//! system's part ([`platform`]), with one for two ends within a process
//! ([`local`]).
//!
//! `unsafe` is allowed here (and in `dom` and `bindings`): the Windows API
//! the clock reads is FFI, and shared memory is read and written through
//! pointers. Each use says why it is sound; the crate's README lists them.

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
#![allow(
    unsafe_code,
    reason = "the Windows API the clock reads; copies in and out of shared memory"
)]

pub mod codec;
pub mod core;
pub mod data_pipe;
pub mod endpoint;
pub mod file;
pub mod local;
pub mod platform;
pub mod shared_buffer;
mod time_ticks;

pub use time_ticks::TimeTicks;
