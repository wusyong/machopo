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
mod navigations;
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

/// An engine as a host's thread runs it: its DOM's tasks' tokens go into a
/// scheduler of its own (`dom::TaskTokens`), and each due one is run by its
/// number (`ScriptEngine::run_dom_task`), as the renderer's main thread
/// runs them. The engine's own calls through it, as one.
pub(crate) struct TestEngine {
    engine: dom_js::V8Engine,
    tokens: dom::TaskTokens,
}

impl TestEngine {
    pub(crate) fn new(settings: dom_api::EngineSettings) -> Self {
        use dom_api::ScriptEngine;
        let mut engine = dom_js::V8Engine::new(settings);
        let tokens = dom::TaskTokens::new();
        engine.dom_mut().set_task_waker(tokens.waker());
        Self { engine, tokens }
    }

    /// Runs the tasks whose tokens are due now, each by its number, in the
    /// scheduler's order (those they queue wait for the next call, as the
    /// host's thread goes on to its other tasks): whether one ran.
    pub(crate) fn run_due_tasks(&mut self) -> bool {
        use dom_api::ScriptEngine;
        let mut ran = false;
        for number in self.tokens.take_due() {
            ran |= self.engine.run_dom_task(number);
        }
        ran
    }

    /// `run_due_tasks`, again until none ran.
    pub(crate) fn run_all_due_tasks(&mut self) {
        while self.run_due_tasks() {}
    }
}

/// The engine's calls a test makes with the engine's own frame as an
/// argument (`engine.run_classic_script(engine.top_frame(), ..)`): through
/// `DerefMut` alone the call's borrow of the engine comes before its
/// arguments'.
#[allow(dead_code, reason = "each test file uses some")]
impl TestEngine {
    pub(crate) fn run_classic_script(
        &mut self,
        frame: dom_api::FrameId,
        script: &dom_api::ClassicScript<'_>,
    ) {
        dom_api::ScriptEngine::run_classic_script(&mut self.engine, frame, script);
    }

    pub(crate) fn create_window(
        &mut self,
        frame: dom_api::FrameId,
        url: &str,
        time_origin: std::time::Instant,
    ) -> Option<dom::NodeRoot> {
        dom_api::ScriptEngine::create_window(&mut self.engine, frame, url, time_origin)
    }

    pub(crate) fn document_ready(
        &mut self,
        frame: dom_api::FrameId,
        readiness: dom_api::DocumentReadiness,
    ) {
        dom_api::ScriptEngine::document_ready(&mut self.engine, frame, readiness);
    }

    pub(crate) fn webdriver_element_property(
        &mut self,
        frame: dom_api::FrameId,
        element: dom::NodeId,
        name: &str,
    ) -> Result<dom_api::WebDriverValue, dom_api::WebDriverScriptError> {
        dom_api::ScriptEngine::webdriver_element_property(&mut self.engine, frame, element, name)
    }

    pub(crate) fn has_resize_observations(&mut self, frame: dom_api::FrameId) -> bool {
        dom_api::ScriptEngine::has_resize_observations(&mut self.engine, frame)
    }

    pub(crate) fn unload_document(
        &mut self,
        frame: dom_api::FrameId,
    ) -> Option<(std::time::Instant, std::time::Instant)> {
        dom_api::ScriptEngine::unload_document(&mut self.engine, frame)
    }

    pub(crate) fn execute_webdriver_script(
        &mut self,
        frame: dom_api::FrameId,
        id: u64,
        body: &str,
        args: &[dom_api::WebDriverValue],
        is_async: bool,
    ) -> Result<(), dom_api::WebDriverScriptError> {
        dom_api::ScriptEngine::execute_webdriver_script(
            &mut self.engine,
            frame,
            id,
            body,
            args,
            is_async,
        )
    }

    pub(crate) fn child_frames(&mut self, frame: dom_api::FrameId) -> Vec<dom_api::FrameId> {
        dom_api::ScriptEngine::child_frames(&mut self.engine, frame)
    }
}

impl std::ops::Deref for TestEngine {
    type Target = dom_js::V8Engine;

    fn deref(&self) -> &dom_js::V8Engine {
        &self.engine
    }
}

impl std::ops::DerefMut for TestEngine {
    fn deref_mut(&mut self) -> &mut dom_js::V8Engine {
        &mut self.engine
    }
}
