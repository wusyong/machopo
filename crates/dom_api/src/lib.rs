//! The DOM's engine-independent API on the DOM (`dom`), beside
//! `dom_api` until it replaces it: the spec's algorithms with their
//! arguments' checks (names' validity, an HTML document's lowercasing),
//! and the declaration table (`declare!`, `interfaces`) each engine binds
//! from, one typed path a member (`typed`). Blink's DOM interfaces' C++
//! (`Node::appendChild`), not the bindings generated for them.
//!
//! A call is one operation of the DOM: a member's function gets the
//! operation (`ApiCx`), `this` and its arguments as objects of it
//! (`NodeRef<'gc>`, or another interface's, `typed::ObjectType`), and
//! roots nothing. No member here calls script; those
//! that do will root what crosses and end the operation first, as script
//! can't run in one.

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

use std::borrow::Cow;
use std::ops::Deref;

use dom::{Cx, NodeKind, NodeRef, names};

// `declare!`'s expansion names the crate's paths as `::dom_api`, here
// as in an engine's crate.
extern crate self as dom_api;

pub mod declare;
mod engine;
mod exception;
pub mod interfaces;
mod names_check;
pub mod typed;

mod activation;
mod animation;
mod animation_frame;
mod animations;
mod attr;
mod blob;
mod canvas;
mod character_data;
mod child_node;
mod collection;
mod computed_style;
mod controls;
mod css_namespace;
mod css_style;
mod css_style_sheet;
mod cssom;
mod custom_elements;
/// The document parser's host, which `ScriptEngine` is.
pub use ::html::parser::ParserHost;
pub use custom_elements::{
    LifecycleCallback, created as check_created_custom_element,
    failed_element as failed_custom_element, invoke_left_ce_reactions,
    process_backup_element_queue, upgradable as check_custom_element_upgradable,
};
mod abort;
mod body;
mod crypto;
mod dataset;
mod document;
mod document_type;
mod dom_implementation;
mod editing;
mod effect;
mod element;
mod event;
mod event_target;
mod fetch;
mod file_reader;
mod focus;
mod forms;
mod frame;
mod geolocation;
mod geometry;
mod handlers;
mod headers;
mod history;
mod html;
mod hyperlink;
mod idle;
mod image;
mod input;
mod intersection_observer;
mod layout;
mod lifecycle;
mod list_box;
mod location;
mod markup;
mod media_query_list;
mod menu_list;
mod message;
pub mod module;
mod mutation_observer;
mod navigator;
mod node;
mod parent_node;
mod performance;
mod query;
mod range;
mod reflect;
mod request;
mod resize_observer;
mod response;
mod script;
mod scroll;
mod select;
mod selection;
mod shadow_root;
mod slot;
mod svg;
mod svg_geometry;
mod table;
mod text_control;
mod timer;
mod token_list;
mod traversal;
mod ui_event;
mod url;
mod wake_lock;
mod webdriver;
mod window;

/// What `EventHandler` says the pointer looks like: `cursor`'s keyword,
/// `auto` resolved.
pub use ::computed_style::CursorKeyword;
pub use computed_style::{computed_style_for_devtools, computed_values_for_devtools};
pub use animation::{notice_steps, settle_animation_notices, settle_started_animations};
pub use animation_frame::run_animation_frame_callbacks;
pub use animations::{AnimationsUpdate, update_animations};
pub use declare::{
    Interface, InterfaceDesc, Member, MemberKind, Named, Param, TypeDesc, overload_interfaces,
    resolve_overload,
};
pub use document::{document_title, set_page_visible};
pub use dom_api_macros::declare;
pub use engine::{
    ClassicScript, ConsoleMessage, ConsoleSink, DevToolsFrameIds, DevToolsInterrupter,
    DevToolsObjects, DevToolsPage, DevToolsQueue, DevToolsQueued, DevToolsSend, EngineSettings,
    LoadSink, ScriptEngine,
};
pub use event::{cancel as cancel_event, coarsen};
pub use event_target::{dispatch, fire_event};
pub use exception::{ApiError, DomException, ExceptionName, LEGACY_CODES, legacy_code};
pub use fetch::{FetchedResponse, did_receive_response};
pub use focus::{flush_autofocus_candidates, focus_fixup, set_initial_focus, set_page_focused};
pub use frame::{
    ChildNavigation, FrameId, FrameRequest, child_windows, documents_in_tree_order,
    frame_tree_walks_for_testing, named_child_windows, origin, same_origin, update_elements,
};
pub use history::{
    can_have_url_rewritten, indicated_element, restore_history, traverse_within_document,
    update_target_element,
};
pub use idle::run_idle_callback;
pub use image::{react_to_environment_changes, run_image_updates};
pub use input::{
    EventHandler, KeyInput, KeyResult, MouseAction, MouseButton, MouseInput, WheelInput,
};
pub use interfaces::{CALLBACK_INTERFACES, INTERFACES};
pub use intersection_observer::{
    ComputedIntersection, notify_intersection_observers, update_intersections,
};
pub use layout::{FrameLayout, Hit, LayoutCell};
pub use lifecycle::{
    DocumentReadiness, document_ready, report_unhandled_rejection, unload_document,
};
pub use media_query_list::report_media_query_changes;
pub use menu_list::{
    PopupItem, PopupItemKind, PopupResult, can_keep_popup, popup_items, popup_menu_closed,
};
pub use message::run_posted_messages;
pub use mutation_observer::deliver_mutations;
pub use navigator::{InputMonitor, PendingInput};
pub use performance::RANDOMIZED_TRIGGER_RATE;
pub use resize_observer::{
    ResizeObserverRound, gather_and_broadcast, has_resize_observations, report_loop_error,
};
pub use script::{
    ModuleSource, ModuleStart, Prepared, is_javascript_mime_type, prepare_script, run_script_task,
    uses_credentials,
};
pub use text_control::run_element_tasks;
pub use timer::{Reported, next_due, report_exception, run_due, run_next_due, run_task};
pub use typed::{
    Any, AnyArg, AnyOrNull, ArgType, CallbackArg, ConstructorInfo, Created, DictMember, DictOrBool,
    Dictionary, DomStr, EmptyStringDefault, ErrorEventObject, Front, FrontHost, FrontTypes,
    HtmlConstructed, NewElement, NewObject, NodeObject, NullableStr, ObjectType, OpFront,
    PromiseRejectionEventObject, Rest, RestArg, RetType, ScriptHost, Sequence, StringPairsArg,
    Strings, TargetObject, TimerHandler, TransferArg, TypedMember, TypedVisitor,
    UnrestrictedDouble, accepts_node, html_element_interface, interface_of, node_interface,
    node_interface_of, realm_document, visit_typed,
};
pub use webdriver::{
    ElementLocator, Probed, WebDriverActionError, WebDriverAnswer, WebDriverQuery,
    WebDriverQueryError, WebDriverScriptError, WebDriverValue, answer as webdriver_answer,
    clear as webdriver_clear, send_keys as webdriver_send_keys,
};
pub use window::{
    NamedProperty, named_property, named_property_collection, window_document_gets_for_testing,
};

/// A member's call: the DOM's operation (`dom::Cx`, which it derefs
/// to), and what the engine gives the API: its frame's layout, if it has
/// one (`layout::FrameLayout`).
pub struct ApiCx<'a, 'gc> {
    dom: &'a Cx<'a, 'gc>,
    layout: Option<&'a dyn layout::LendLayout>,
}

impl<'a, 'gc> ApiCx<'a, 'gc> {
    /// An operation with no layout (a document of no frame's).
    pub fn new(dom: &'a Cx<'a, 'gc>) -> Self {
        Self { dom, layout: None }
    }

    /// An operation that reaches the frame's layout in `layout`.
    pub fn with_layout_cell(dom: &'a Cx<'a, 'gc>, layout: &'a LayoutCell) -> Self {
        Self {
            dom,
            layout: Some(layout),
        }
    }

    /// `f` with the layout of `node`'s document's frame, its style and
    /// layout brought up to date first; none if it has no frame (another
    /// document, or no layout at all) or `node` isn't in its tree.
    pub(crate) fn with_layout<R>(
        &self,
        node: NodeRef<'gc>,
        f: impl FnOnce(&mut dyn FrameLayout) -> R,
    ) -> Option<R> {
        self.lend_layout(node, |layout| {
            layout.update_style_and_layout(self.dom);
            f(layout)
        })
    }

    /// `f` with the layout of `node`'s document's frame, its style brought
    /// up to date first and nothing laid out (`FrameLayout::update_style`);
    /// none as `with_layout`'s.
    pub(crate) fn with_style<R>(
        &self,
        node: NodeRef<'gc>,
        f: impl FnOnce(&mut dyn FrameLayout) -> R,
    ) -> Option<R> {
        self.lend_layout(node, |layout| {
            layout.update_style(self.dom);
            f(layout)
        })
    }

    /// The size of `node`'s document's frame, for `innerWidth` and
    /// `innerHeight` (`FrameLayout::viewport_size`); none as `with_layout`'s.
    pub(crate) fn viewport_size(&self, node: NodeRef<'gc>) -> Option<gfx::SizeF> {
        self.lend_layout(node, |layout| layout.viewport_size(self.dom))
    }

    /// `f` with the layout of `node`'s document's frame as it is, nothing
    /// brought up to date: for what no layout changes (the window's size
    /// and scale).
    pub(crate) fn peek_layout<R>(
        &self,
        node: NodeRef<'gc>,
        f: impl FnOnce(&dyn FrameLayout) -> R,
    ) -> Option<R> {
        self.lend_layout(node, |layout| f(layout))
    }

    /// `f` with the frame's layout as it is, whatever document's the node
    /// it is for: what draws a canvas that isn't shown (the renderer finds
    /// it by its id). None without a frame.
    pub(crate) fn lend_frame_layout<R>(
        &self,
        f: impl FnOnce(&mut dyn FrameLayout) -> R,
    ) -> Option<R> {
        let mut f = Some(f);
        let mut result = None;
        self.layout?.lend(&mut |layout| {
            result = f.take().map(|f| f(layout));
        });
        result
    }

    pub(crate) fn lend_layout<R>(
        &self,
        node: NodeRef<'gc>,
        f: impl FnOnce(&mut dyn FrameLayout) -> R,
    ) -> Option<R> {
        let document = self.document_of(node);
        self.lend_frame_layout(|layout| {
            let shown = self.is_connected(node) && layout.shows(self.node_id(document));
            shown.then(|| f(layout))
        })
        .flatten()
    }

    /// Whether `element` is an HTML element in an HTML document: whose
    /// names the API lowercases (DOM's "get an attribute by name" and the
    /// others).
    fn is_html_element_in_html_document(&self, element: NodeRef<'gc>) -> bool {
        self.element_name(element)
            .is_some_and(|name| name.namespace == names::HTML_NAMESPACE)
            && self.is_html_document(self.document_of(element))
    }
}

impl<'a, 'gc> Deref for ApiCx<'a, 'gc> {
    type Target = Cx<'a, 'gc>;

    fn deref(&self) -> &Cx<'a, 'gc> {
        self.dom
    }
}

/// `name` in ASCII lowercase: itself when it is already (no copy).
fn lowercase(name: &str) -> Cow<'_, str> {
    if name.bytes().any(|b| b.is_ascii_uppercase()) {
        name.to_ascii_lowercase().into()
    } else {
        name.into()
    }
}

/// Whether `node` is an element.
fn is_element<'gc>(cx: &Cx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    cx.kind(node) == NodeKind::Element
}
