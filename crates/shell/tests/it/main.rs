//! `shell`'s integration tests, one binary (each file was one): a change
//! below `shell` links this once, not once per file. `scenarios` has its
//! own `main`, so it is a binary of its own.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod automation_pipe;
mod browser_view;
mod canvas;
mod css_animations;
mod devtools;
mod devtools_css;
mod devtools_dom;
mod devtools_initiators;
mod devtools_lifecycle;
mod devtools_log;
mod devtools_network;
mod devtools_overlay;
mod documents;
mod fetch;
mod file_reader;
mod focus;
mod forms;
mod geometry;
mod history;
mod images;
mod inserted_scripts;
mod intersection_observers;
mod keys;
mod lazy_images;
mod limits;
mod messaging;
mod modules;
mod mouse;
mod navigation_timing;
mod navigator;
mod network;
mod performance;
mod remote_debugging;
mod resize_observers;
mod scheduling;
mod script;
mod scroll_anchoring;
mod selection_painting;
mod selections;
mod sites;
mod style_sheets;
mod trace;
mod transitions;
mod web_animations;
mod web_tests;
mod webdriver;
mod wheel;
mod windowless;
