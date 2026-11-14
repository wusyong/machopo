//! What every crate builds on, with no dependencies of its own (Chromium's
//! `base/`): integer conversions that say what overflow does
//! ([`numerics`]), and sequence numbers that panic rather than wrap
//! ([`SequenceNumber`], [`AtomicSequenceNumber`]). The rules for integer
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

pub use sequence::{AtomicSequenceNumber, SequenceNumber};
