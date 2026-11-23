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
mod indexed_query;
mod info_front;
mod intersection_observers;
mod links;
mod markup;
mod media_queries;
mod messages;
mod mutation_observer;
mod named_properties;
mod navigator;
mod navigator_apis;
mod node_tree;
mod performance;
mod queries;
mod ranges;
mod resize_observers;
mod script_members;
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
mod termination;
mod text_controls;
mod timers;
mod ui_events;
mod urls;
mod web_animations;
mod webdriver;
mod window;
mod window_named;
mod worker;
mod wrappers;

// Forced GCs: `--expose-gc` lets a test ask V8 for one.
::bindings::initialize_v8_before_main!("--expose-gc");

/// The engine's child frames, parents before their children, each parent's
/// in the order it made them: as the host asks for them
/// (`ScriptEngine::child_frames`).
pub(crate) fn child_frames(engine: &mut dom_js::V8Engine) -> Vec<dom_api::FrameId> {
    use dom_api::ScriptEngine;
    let mut frames = Vec::new();
    let mut parents = std::collections::VecDeque::from([engine.top_frame()]);
    while let Some(parent) = parents.pop_front() {
        for child in engine.child_frames(parent) {
            frames.push(child);
            parents.push_back(child);
        }
    }
    frames
}
