//! The DOM's JS front end (V8) (Blink's `bindings/core/v8`): the declaration table's interfaces as V8
//! templates (`templates`), each member's call through V8's front end
//! (`front`), `DOMException` (`exception`), the DOM's strings to and from
//! V8 (`strings`), and the isolate's DOM, whose heap V8's major GCs mark
//! (`state`); and `machopoDom` (`next`), a document of it in a realm of
//! the tests'. No `unsafe`: that is `bindings`' (`set_isolate_state_with_embedder_heap`,
//! `wrap`).

#![forbid(unsafe_code)]
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

mod cross_origin;
mod custom_elements;
mod devtools;
mod devtools_page;
mod engine;
mod exception;
mod frames;
mod front;
mod handlers;
mod info_front;
mod keyframes;
mod listeners;
mod machopo_dom;
mod modules;
mod named;
mod observable_array;
mod pairs;
mod serialize;
mod state;
pub mod strings;
mod templates;
mod webdriver;
mod window;
mod window_properties;

pub use engine::V8Engine;

/// A Perfetto trace of the process: V8's events (GC, compiling, parsing)
/// and the embedder's spans together, as Chrome's trace has them. Started
/// before V8 is.
pub use bindings::trace;

pub use machopo_dom::{
    cached_strings, dom_mut, heap_stats, install_machopo_dom, names_made, string_conversions,
    uncaught_exceptions, wrappers_ahead, wrappers_made, wrappers_wasted,
};
pub use strings::{ArgString, StringCache, arg_string, new_string, string_arg, throw_type_error};
pub use templates::{install, overload_interfaces_untold};
pub use window::{
    create_window_realm, next_timer, run_element_tasks, run_posted_messages, run_timers,
};
