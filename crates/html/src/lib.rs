//! HTML parser (`blink/renderer/core/html/parser`).
//!
//! html5ever does the tokenizing and tree construction; [`parser`] holds its
//! `TreeSink` over the DOM (`dom`), a document's parser, which stops
//! at every `</script>` so the caller can run the script, HTML's fragment
//! parsing algorithm and fragment serializing (`innerHTML` and the like),
//! and XML's parse (SVG files, as images) with xml5ever into the same sink.
//!
//! [`srcset`] parses `srcset` and `sizes`, for the DOM's images.

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

mod fast_path;
mod fast_path_build;
pub mod parser;
mod serialize;
pub mod srcset;
mod xml;

pub use fast_path::{decodes_for_testing, fast_parses_for_testing};
