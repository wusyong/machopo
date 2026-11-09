//! `style`'s integration tests, one binary: documents parsed into
//! `dom`, styled, their elements' computed styles read with the token
//! (the harness: `page`).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod adopted;
mod animation_properties;
mod animations;
mod calc;
mod cascade;
mod computed;
mod css_animations;
mod fonts;
mod form_controls;
mod form_states;
mod hints;
mod interpolation;
mod invalidation;
mod keyframes;
mod legacy_hints;
mod logical;
mod matched_properties;
mod matching;
mod media;
mod page;
mod parts;
mod query;
mod recalc;
mod shadow;
mod sheets;
mod style_groups;
mod svg;
mod svg_use;
mod tables;
mod transitions;
mod variables;

pub use page::*;
