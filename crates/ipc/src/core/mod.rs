//! The channels' core (Chromium's `mojo/core`, without its
//! `mojo/core/platform`): what a channel does with the bytes and handles
//! its platform's layer reads and writes. It makes no system call and
//! knows no platform: bytes are bytes, a handle is whatever the platform
//! says one is, so the whole of it runs the same on all three, and under
//! Miri and a fuzzer.

pub mod channel;
pub mod frame;
mod sync;

#[cfg(test)]
mod fake;
