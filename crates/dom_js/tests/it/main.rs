//! `dom_js`'s integration tests, one binary.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod abort;
mod activation;
mod bench;
mod blobs;
mod canvas;
mod collections;
mod cross_origin;
mod css_style;
mod custom_elements;
mod devtools;
mod document_all;
mod documents;
mod dom_core;
mod editing;
mod engine;
mod env;
mod event_interfaces;
mod events;
mod exceptions;
mod fetch;
mod files;
mod form_values;
mod forms;
mod frames;
mod geometry;
mod handlers;
mod headers;
mod history;
mod html_elements;
mod idle;
mod images;
mod intersection_observers;
mod links;
mod markup;
mod media_queries;
mod messages;
mod mutation_observer;
mod navigator;
mod navigator_apis;
mod node_tree;
mod performance;
mod queries;
mod ranges;
mod resize_observers;
mod scripts;
mod scrolling;
mod select;
mod selections;
mod shadow_dom;
mod strings;
mod style_sheets;
mod svg;
mod svg_geometry;
mod tasks;
mod text_controls;
mod timers;
mod ui_events;
mod urls;
mod web_animations;
mod webdriver;
mod window;
mod window_named;
mod wrappers;

// Forced GCs: `--expose-gc` lets a test ask V8 for one.
::bindings::initialize_v8_before_main!("--expose-gc");
