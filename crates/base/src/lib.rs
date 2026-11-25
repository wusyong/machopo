//! What every crate builds on, with no dependencies of its own (Chromium's
//! `base/`): integer conversions that say what overflow does
//! ([`numerics`]), sequence numbers that panic rather than wrap
//! ([`SequenceNumber`], [`AtomicSequenceNumber`]), and a thread's task
//! queues ([`sequence_manager`]), the renderer's and each worker's. The rules for integer
//! arithmetic are in this crate's README.

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

pub mod census;
pub mod numerics;
mod sequence;
pub mod sequence_manager;

pub use sequence::{AtomicSequenceNumber, SequenceNumber};
