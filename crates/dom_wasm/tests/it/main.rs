//! `dom_wasm`'s integration tests, one binary: the WIT is the table's
//! (`wit`), a C guest calls the DOM through it (`guest`), the guests'
//! wasm GC objects share the DOM's heap (`shared`), and the guest's WASI:
//! its waits and its output (`wasi`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod guest;
mod shared;
mod wasi;
mod wit;
