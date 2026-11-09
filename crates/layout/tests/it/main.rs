//! `layout`'s integration tests, one binary: a change in `layout` or below links this
//! once, not once per file, and a new test file joins it (a module here).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod block;
mod calc;
mod caret;
mod contain;
mod flex;
mod float;
mod font_features;
mod font_selection;
mod form_controls;
mod generated;
mod grid;
mod inline;
mod inline_block;
mod line_clamp;
mod lists;
mod page;
mod parley_inline_box;
mod parley_line_breaks;
mod parley_shaped_text;
mod parley_space;
mod parley_words;
mod position;
mod scroll_anchor;
mod shadow;
mod shaping;
mod sticky;
mod svg;
mod tables;
mod text_overflow;
mod text_rects;
mod update;
