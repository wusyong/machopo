//! `dom_api`'s integration tests, one binary. Members are called as an
//! engine calls them: through their typed paths (`visit_typed`), with a
//! front end of the tests' (`TestFront`) whose arguments are a list of
//! values and whose result is kept.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    reason = "tests"
)]

mod animation_frame;
mod character_data;
mod collection;
mod computed_style;
mod controls;
mod css_style;
mod dataset;
mod dispatch;
mod document;
mod editing;
mod element;
mod event;
mod focus;
mod geometry;
mod images;
mod input;
mod intersection;
mod kinds;
mod markup;
mod mutation_observer;
mod node;
mod query;
mod ranges;
mod resize;
mod scroll;
mod shadow;
mod table;
mod timer;
mod token_list;
mod webdriver;
mod wheel;
mod window;

use dom_api::{ApiCx, ApiError, Front, FrontTypes, MemberKind, OpFront, TypedMember, TypedVisitor};
use std::cell::RefCell;
use std::rc::Rc;

use dom::{Dom, DomString, NodeRef, NodeRoot, ObjectRef, ObjectRoot};
use elise::HeapStats;

/// A DOM as the tests' front end holds it: borrowed for each operation of
/// a call (`Front::read` shared, `Front::mutate` whole: a `RefCell`, so
/// that one overlapping the other panics), and for the tests' own; and the
/// tests' listeners: what each callback (a number, in its listener's
/// engine word) does when called, and the calls made.
pub struct TestDom(RefCell<Dom>, pub Listeners);

impl Drop for TestDom {
    /// The roots the listeners hold go before the heap (the tests' own
    /// may share them, and keep theirs past it no more than the DOM).
    fn drop(&mut self) {
        self.1.current_window.borrow_mut().take();
        self.1.action_node.borrow_mut().take();
    }
}

/// The mutation observers' calls: the callback's number, the records'
/// types.
pub type MutationLog = Rc<RefCell<Vec<(u64, Vec<String>)>>>;

/// The intersection observers' calls: the callback's number, each entry
/// as `id time isIntersecting ratio isVisible rootBounds
/// boundingClientRect intersectionRect` (rects `x,y,width,height`).
pub type IntersectionLog = Rc<RefCell<Vec<(u64, Vec<String>)>>>;

/// The tests' callbacks: each a number, which a call logs with the event's
/// phase and current target, then does its action.
#[derive(Clone, Default)]
pub struct Listeners {
    pub actions: Rc<RefCell<std::collections::HashMap<u64, Action>>>,
    pub log: Rc<RefCell<Vec<Call>>>,
    /// What `Front::current_window` gives: the realm's window, if any.
    pub current_window: Rc<RefCell<Option<elise::Persistent<dom::Window>>>>,
    /// The timers' calls (`ScriptHost::call_timer`).
    pub timer_log: Rc<RefCell<Vec<TimerCall>>>,
    /// What each `ReportAgain` action's report became.
    pub reported: Rc<RefCell<Vec<dom_api::Reported>>>,
    /// The mutation observers' calls: the callback's number, the records'
    /// types.
    pub mutation_log: MutationLog,
    /// How many microtask checkpoints the host was asked for
    /// (`ScriptHost::perform_microtask_checkpoint`).
    pub checkpoints: Rc<std::cell::Cell<usize>>,
    /// The node `Action::MoveFocus` and `Action::DetachNode` act on.
    pub action_node: Rc<RefCell<Option<Rc<NodeRoot>>>>,
    /// The animation frame callbacks called: their callback numbers and
    /// the time each was given.
    pub frame_log: Rc<RefCell<Vec<(u64, f64)>>>,
    /// The intersection observers called back: the callback's number,
    /// each entry's values.
    pub intersection_log: IntersectionLog,
    /// The resize observers called back: the callback's number, each
    /// entry as `id contentRect border content device` (sizes
    /// `inline x block`).
    pub resize_log: IntersectionLog,
    /// The keyboard events' data, as each was dispatched.
    pub keys: Rc<RefCell<Vec<dom::KeyboardEventInfo>>>,
    /// The frame's layout the operations reach, if a test gave one.
    pub layout: Rc<dom_api::LayoutCell>,
    /// The operations the front opened (`Front::read`, `Front::mutate`).
    pub operations: Rc<std::cell::Cell<usize>>,
}

/// One call of a timer's callback: its number, its arguments' (their
/// count, in the tests' engine word), and the window's nesting level as it
/// ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimerCall {
    pub callback: u64,
    pub arguments: u64,
    pub nesting: u8,
}

/// What a callback does to the event when called.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Nothing,
    StopPropagation,
    StopImmediatePropagation,
    /// Removes the listener of this callback number (of the same type and
    /// capture) from the current target.
    Remove(u64),
    /// Cancels the event (`preventDefault`).
    Cancel,
    /// Reports an exception at the current target, a window, again (as a
    /// listener of an `error` event that throws).
    ReportAgain,
    /// Takes the current target, a node, out of its parent.
    Detach,
    /// Gives the focus to `Listeners::action_node` (as its `focus()` does,
    /// with no events).
    MoveFocus,
    /// Takes `Listeners::action_node` out of its parent.
    DetachNode,
    /// Adds a bubbling listener of this callback number, for the event's
    /// type, to `Listeners::action_node`.
    ListenAtNode(u64),
}

/// One call of a callback: its number, the event's phase, and its current
/// target's node name (`#target` for a plain target); whether it ran in a
/// passive listener.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    pub callback: u64,
    pub phase: u16,
    pub target: String,
    /// The event's target's name (`target`, which a legacy target override
    /// makes another than the current target's).
    pub event_target: String,
    /// The event's type.
    pub event_type: String,
    pub passive: bool,
    pub trusted: bool,
    pub cancelable: bool,
    /// An error event's error (`report_exception`'s): the engine word of
    /// its edge (0 for none, or another event).
    pub error: u64,
    /// The event's time (milliseconds since its relevant global's time
    /// origin), at full precision.
    pub time: f64,
    /// A mouse event's data and its related target's name (empty for
    /// none).
    pub mouse: Option<dom::MouseEventInfo>,
    pub related: String,
    /// A wheel event's deltas, and the interface its wrapper is of
    /// (`interface_of`).
    pub wheel: Option<dom::WheelEventInfo>,
    pub interface: &'static str,
    /// An input event's data, and a text event's text.
    pub input: Option<dom::InputEventInfo>,
    pub text: Option<String>,
}

impl Default for TestDom {
    fn default() -> Self {
        Self::new()
    }
}

impl TestDom {
    pub fn new() -> Self {
        Self(RefCell::new(Dom::new()), Listeners::default())
    }

    pub fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&dom::Cx<'_, 'gc>) -> R) -> R {
        self.0.get_mut().mutate(f)
    }

    pub fn read<R>(&self, f: impl for<'gc> FnOnce(&dom::Cx<'_, 'gc>) -> R) -> R {
        self.0.borrow().read(f)
    }

    /// The DOM, for what takes it whole.
    pub fn dom(&self) -> std::cell::Ref<'_, Dom> {
        self.0.borrow()
    }

    pub fn stats(&self) -> HeapStats {
        self.0.borrow().stats()
    }
}

/// An argument, as a script would pass it.
#[derive(Clone)]
pub enum Arg {
    Str(&'static str),
    Null,
    Undefined,
    Node(std::rc::Rc<NodeRoot>),
    /// A dictionary's members; `None` a member whose getter throws.
    Dict(Vec<(&'static str, Option<bool>)>),
    Bool(bool),
    /// A callback, by its number.
    Callback(u64),
    /// An object besides a node.
    Object(Rc<ObjectRoot>),
    /// A number.
    Number(f64),
}

/// What a call returned.
#[derive(Debug)]
pub enum Ret {
    Undefined,
    Null,
    Bool(bool),
    U32(u32),
    F64(f64),
    Str(DomString),
    Node(NodeRoot),
    /// A constructor's new object, or an object result.
    Object(ObjectRoot),
    Objects(Vec<ObjectRoot>),
    /// Any value: the tests' engine word of its edge (0 for none).
    Any(u64),
    /// Named properties' names.
    Strings(Vec<DomString>),
}

impl Ret {
    pub fn string(&self) -> &DomString {
        match self {
            Self::Str(string) => string,
            other => panic!("not a string: {other:?}"),
        }
    }

    pub fn node(&self) -> &NodeRoot {
        match self {
            Self::Node(node) => node,
            other => panic!("not a node: {other:?}"),
        }
    }
}

/// The tests' front end: the DOM, `this`, the arguments, and what the
/// call gave. Its own side in an operation (`OpFront`).
struct TestFront<'d> {
    dom: &'d RefCell<Dom>,
    this: This,
    args: Vec<Arg>,
    ret: Option<Ret>,
    thrown: Option<ApiError>,
    hooks: Hooks,
    listeners: Listeners,
    /// The callbacks taken, by `CallbackArg`.
    callbacks: Vec<u64>,
    /// The rest arguments taken: their count.
    rest: u64,
}

/// A node or object argument.
#[derive(Clone)]
pub enum Taken {
    Node(Rc<NodeRoot>),
    Object(Rc<ObjectRoot>),
}

/// What a call asked of its front end beyond its arguments and result.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Hooks {
    /// The interfaces of the wrappers made ahead (`prepare_new_node`).
    pub prepared: Vec<usize>,
    /// Results given as a new node's (`ret_new_node`).
    pub new_nodes: usize,
    /// Whether the call finished (`finish`, after its operation).
    pub finished: bool,
    /// The references to an ASCII string result's buffer as it was given
    /// (a string the DOM holds, borrowed or cloned).
    pub string_refs: Option<usize>,
    /// The dictionary members read, in order.
    pub dict_reads: Vec<&'static str>,
}

/// A call's `this`: a node, another object, or none (a constructor's).
#[derive(Clone)]
pub enum This {
    Node(std::rc::Rc<NodeRoot>),
    Object(std::rc::Rc<ObjectRoot>),
    None,
}

impl<'d> TestFront<'d> {
    fn new(dom: &'d TestDom, this: This, args: &[Arg]) -> Self {
        Self {
            dom: &dom.0,
            this,
            args: args.to_vec(),
            ret: None,
            thrown: None,
            hooks: Hooks::default(),
            listeners: dom.1.clone(),
            callbacks: Vec::new(),
            rest: 0,
        }
    }

    fn throw_type_error(&mut self, message: &str) {
        self.throw(ApiError::type_error(message));
    }
}

impl FrontTypes for TestFront<'_> {
    type Str = String;
    type Node = Taken;
    type Dict = Vec<(&'static str, Option<bool>)>;
    type PendingString = std::convert::Infallible;
    type PendingNode = std::convert::Infallible;
    type PendingNumber = std::convert::Infallible;
    type PendingObject = std::convert::Infallible;
}

impl<'d> Front for TestFront<'d> {
    type Op<'o>
        = TestFront<'d>
    where
        Self: 'o;

    fn read<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        self.listeners
            .operations
            .set(self.listeners.operations.get() + 1);
        let (dom, layout) = (self.dom, self.listeners.layout.clone());
        dom.borrow()
            .read(|cx| f(&ApiCx::with_layout_cell(cx, &layout), self))
    }

    fn mutate<'o, R>(
        &'o mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut Self::Op<'o>) -> R,
    ) -> R {
        self.listeners
            .operations
            .set(self.listeners.operations.get() + 1);
        let (dom, layout) = (self.dom, self.listeners.layout.clone());
        dom.borrow_mut()
            .mutate(|cx| f(&ApiCx::with_layout_cell(cx, &layout), self))
    }

    fn has_args(&mut self, name: &str, required: usize) -> bool {
        if self.args.len() < required {
            self.throw_type_error(&format!("{name}: {required} arguments required"));
            return false;
        }
        true
    }

    fn string_arg(&mut self, index: usize) -> Option<String> {
        // WebIDL's `ToString`: null and undefined spell themselves.
        Some(
            match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
                Arg::Str(text) => text.to_owned(),
                Arg::Null => "null".to_owned(),
                Arg::Undefined => "undefined".to_owned(),
                Arg::Node(_) => "[object Node]".to_owned(),
                Arg::Dict(_) | Arg::Object(_) | Arg::Callback(_) => "[object Object]".to_owned(),
                Arg::Bool(value) => value.to_string(),
                Arg::Number(value) => value.to_string(),
            },
        )
    }

    fn arg_count(&mut self) -> usize {
        self.args.len()
    }

    fn nullable_string_arg(&mut self, index: usize) -> Option<Option<String>> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Null | Arg::Undefined => Some(None),
            _ => self.string_arg(index).map(Some),
        }
    }

    fn legacy_null_string_arg(&mut self, index: usize) -> Option<Option<String>> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Null => Some(None),
            _ => self.string_arg(index).map(Some),
        }
    }

    fn node_arg(&mut self, index: usize) -> Option<Option<Self::Node>> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Node(node) => Some(Some(Taken::Node(node))),
            Arg::Null | Arg::Undefined => Some(None),
            _ => {
                self.throw_type_error("not a node");
                None
            }
        }
    }

    fn dict_arg(&mut self, index: usize, name: &str) -> Option<Option<Self::Dict>> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Dict(members) => Some(Some(members)),
            Arg::Null | Arg::Undefined => Some(None),
            _ => {
                self.throw_type_error(&format!("not a {name}"));
                None
            }
        }
    }

    fn dict_bool(&mut self, dict: &Self::Dict, member: &str) -> Option<Option<bool>> {
        let entry = dict.iter().find(|(name, _)| *name == member);
        if let Some((name, _)) = entry {
            self.hooks.dict_reads.push(name);
        }
        match entry {
            None => Some(None),
            Some((_, Some(value))) => Some(Some(*value)),
            Some((_, None)) => {
                self.throw_type_error(&format!("{member}'s getter threw"));
                None
            }
        }
    }

    /// The tests' dictionaries have booleans alone: a string sequence is
    /// always missing.
    fn dict_strings(&mut self, _: &Self::Dict, _: &str) -> Option<Option<Vec<String>>> {
        Some(None)
    }

    /// Nor an object (`AddEventListenerOptions`' `signal`).
    fn dict_object<T: elise::HeapType>(
        &mut self,
        _: &Self::Dict,
        _: &str,
        _: &str,
    ) -> Option<Option<dom_api::typed::KeptObject>> {
        Some(None)
    }

    fn bool_arg(&mut self, index: usize) -> bool {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Bool(value) => value,
            Arg::Null | Arg::Undefined => false,
            Arg::Str(text) => !text.is_empty(),
            _ => true,
        }
    }

    fn is_undefined(&mut self, index: usize) -> bool {
        matches!(
            self.args.get(index).cloned().unwrap_or(Arg::Undefined),
            Arg::Undefined
        )
    }

    fn is_null_or_undefined(&mut self, index: usize) -> bool {
        matches!(
            self.args.get(index).cloned().unwrap_or(Arg::Undefined),
            Arg::Null | Arg::Undefined
        )
    }

    fn is_dictionary_like(&mut self, index: usize) -> bool {
        !matches!(
            self.args.get(index).cloned().unwrap_or(Arg::Undefined),
            Arg::Bool(_) | Arg::Str(_)
        )
    }

    fn callback_arg(&mut self, index: usize, name: &str) -> Option<Option<dom_api::CallbackArg>> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Callback(number) => {
                self.callbacks.push(number);
                Some(Some(dom_api::CallbackArg(self.callbacks.len() - 1)))
            }
            Arg::Null | Arg::Undefined => Some(None),
            _ => {
                self.throw_type_error(&format!("not a {name}"));
                None
            }
        }
    }

    /// A callback by its number; anything else a `TypeError`.
    fn function_arg(&mut self, index: usize, name: &str) -> Option<dom_api::CallbackArg> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Callback(number) => {
                self.callbacks.push(number);
                Some(dom_api::CallbackArg(self.callbacks.len() - 1))
            }
            _ => {
                self.throw_type_error(&format!("not a {name}"));
                None
            }
        }
    }

    /// A callback by its number; anything else a string handler, number
    /// `STRING_HANDLER`.
    fn timer_handler_arg(&mut self, index: usize) -> Option<dom_api::CallbackArg> {
        let number = match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Callback(number) => number,
            _ => STRING_HANDLER,
        };
        self.callbacks.push(number);
        Some(dom_api::CallbackArg(self.callbacks.len() - 1))
    }

    /// `ToUint32` of a number (wrapping, NaN 0), undefined 0.
    fn u32_arg(&mut self, index: usize) -> Option<u32> {
        Some(
            match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
                Arg::Number(value) if value.is_finite() => {
                    value.trunc().rem_euclid(4_294_967_296.0) as u32
                }
                Arg::Bool(value) => value.into(),
                _ => 0,
            },
        )
    }

    /// `ToNumber` of a number, a boolean or undefined (NaN); with
    /// `finite`, a NaN or an infinity a `TypeError`.
    fn f64_arg(&mut self, index: usize, finite: bool) -> Option<f64> {
        let value = match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Number(value) => value,
            Arg::Bool(value) => f64::from(u8::from(value)),
            _ => f64::NAN,
        };
        if finite && !value.is_finite() {
            self.throw_type_error("The provided double value is non-finite.");
            return None;
        }
        Some(value)
    }

    /// `ToInt32` of a number (wrapping, NaN 0), undefined 0.
    fn i32_arg(&mut self, index: usize) -> Option<i32> {
        Some(
            match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
                Arg::Number(value) if value.is_finite() => {
                    value.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
                }
                Arg::Bool(value) => value.into(),
                _ => 0,
            },
        )
    }

    /// Kept as their count.
    fn rest_arg(&mut self, index: usize) -> Option<dom_api::RestArg> {
        let count = self.args.len().saturating_sub(index);
        (count > 0).then(|| {
            self.rest = count as u64;
            dom_api::RestArg
        })
    }

    fn object_arg<T: elise::HeapType>(
        &mut self,
        index: usize,
        interface: &str,
    ) -> Option<Option<Taken>> {
        match self.args.get(index).cloned().unwrap_or(Arg::Undefined) {
            Arg::Object(object) => Some(Some(Taken::Object(object))),
            Arg::Null | Arg::Undefined => Some(None),
            _ => {
                self.throw_type_error(&format!("not a {interface}"));
                None
            }
        }
    }

    /// Logs the call, then does the callback's action.
    fn call_listener(&mut self, event: &elise::Persistent<dom::Event>, index: usize) {
        let listeners = self.listeners.clone();
        let dom = self.dom;
        let action = dom.borrow().read(|cx| {
            let event = event.get(cx.gc);
            let listener = cx
                .snapshot_at(event, index)
                .expect("a listener of the snapshot");
            let callback = cx.callback_edge(listener).embedder_word(cx.gc);
            let target = cx.event_current_target(event).expect("a current target");
            listeners.log.borrow_mut().push(Call {
                callback,
                phase: cx.event_phase(event),
                target: target_name(cx, target),
                event_target: cx
                    .event_target(event)
                    .map_or_else(String::new, |target| target_name(cx, target)),
                event_type: dom::names::get(cx.event_type(event)).to_string(),
                passive: cx.event_state(event).in_passive_listener,
                trusted: cx.event_flags(event).trusted,
                cancelable: cx.event_flags(event).cancelable,
                error: cx
                    .error_event_data(event)
                    .map_or(0, |data| cx.error_edge(data).embedder_word(cx.gc)),
                time: cx.event_time_stamp(event),
                mouse: cx
                    .mouse_event_data(event)
                    .map(|data| cx.mouse_event_info(data)),
                related: cx
                    .mouse_event_data(event)
                    .and_then(|data| cx.mouse_related_target(data))
                    .or_else(|| {
                        cx.focus_event_data(event)
                            .and_then(|data| cx.focus_related_target(data))
                    })
                    .map_or_else(String::new, |target| target_name(cx, target)),
                wheel: cx
                    .wheel_event_data(event)
                    .map(|data| cx.wheel_event_info(data)),
                input: cx
                    .input_event_data(event)
                    .map(|data| cx.input_event_info(data)),
                text: cx
                    .text_event_data(event)
                    .map(|data| cx.text_event_text(data).to_str().into_owned()),
                interface: dom_api::INTERFACES
                    [dom_api::interface_of(&ApiCx::new(cx), ObjectRef::Event(event))]
                .name,
            });
            if let Some(data) = cx.keyboard_event_data(event) {
                listeners
                    .keys
                    .borrow_mut()
                    .push(cx.keyboard_event_info(data));
            }
            let action = listeners
                .actions
                .borrow()
                .get(&callback)
                .copied()
                .unwrap_or(Action::Nothing);
            let mut state = cx.event_state(event);
            match action {
                Action::Nothing
                | Action::ReportAgain
                | Action::Detach
                | Action::MoveFocus
                | Action::DetachNode => {}
                Action::ListenAtNode(other) => {
                    let node = listeners
                        .action_node
                        .borrow()
                        .clone()
                        .expect("a node to act on");
                    let added = cx.add_listener(
                        dom::TargetRef::Node(node.get(cx.gc)),
                        cx.event_type(event),
                        dom::ListenerFlags::default(),
                    );
                    cx.callback_edge(added).set_embedder_word(cx.gc, other);
                }
                Action::Cancel => state.canceled = true,
                Action::StopPropagation => state.stop_propagation = true,
                Action::StopImmediatePropagation => {
                    state.stop_propagation = true;
                    state.stop_immediate_propagation = true;
                }
                Action::Remove(other) => {
                    let flags = cx.listener_flags(listener);
                    let found =
                        cx.find_listener(target, cx.event_type(event), flags.capture, |edge| {
                            edge.embedder_word(cx.gc) == other
                        });
                    if let Some(found) = found {
                        cx.remove_listener(target, found);
                    }
                }
            }
            cx.set_event_state(event, state);
            action
        });
        if matches!(action, Action::MoveFocus | Action::DetachNode) {
            let node = listeners
                .action_node
                .borrow()
                .clone()
                .expect("a node to act on");
            dom.borrow_mut().mutate(|cx| {
                let node = node.get(cx.gc);
                if action == Action::MoveFocus {
                    cx.set_focused_element(cx.document_of(node), Some(node), true);
                } else {
                    cx.remove(node);
                }
            });
        }
        if action == Action::Detach {
            dom.borrow_mut().mutate(|cx| {
                if let Some(dom::TargetRef::Node(node)) = cx.event_current_target(event.get(cx.gc))
                {
                    cx.remove(node);
                }
            });
        }
        if action == Action::ReportAgain {
            let window = dom.borrow().read(|cx| {
                let target = cx
                    .event_current_target(event.get(cx.gc))
                    .expect("a current target");
                let dom::TargetRef::Window(window) = target else {
                    panic!("reported again at a window");
                };
                elise::Persistent::new(cx.gc, window)
            });
            let reported =
                dom_api::report_exception(&mut TestHost(self), &window, &error_info(), Some(99));
            listeners.reported.borrow_mut().push(reported);
        }
    }

    fn construct(&mut self, interface: usize, required: usize) -> bool {
        self.has_args(dom_api::INTERFACES[interface].name, required)
    }

    fn prepare_new_node(&mut self, interface: usize) {
        assert!(
            self.thrown.is_none(),
            "made ahead once the arguments are taken"
        );
        self.hooks.prepared.push(interface);
    }

    fn finish(&mut self) {
        assert!(
            self.dom.try_borrow_mut().is_ok(),
            "finished after the operation"
        );
        self.hooks.finished = true;
    }

    fn throw(&mut self, error: ApiError) {
        assert!(self.thrown.is_none(), "thrown twice");
        self.thrown = Some(error);
    }
}

impl OpFront for TestFront<'_> {
    fn str_param<'a>(&mut self, string: &'a String) -> &'a str {
        string
    }

    fn store_rest(&mut self, cx: &ApiCx<'_, '_>, edge: &elise::JsEdge, _: dom_api::RestArg) {
        edge.set_embedder_word(cx.gc, self.rest);
    }

    fn ret_any<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, edge: &'gc elise::JsEdge) {
        self.ret = Some(Ret::Any(edge.embedder_word(cx.gc)));
    }

    fn same_callback(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &elise::JsEdge,
        callback: dom_api::CallbackArg,
    ) -> bool {
        edge.embedder_word(cx.gc) == self.callbacks[callback.0]
    }

    fn store_callback(
        &mut self,
        cx: &ApiCx<'_, '_>,
        edge: &elise::JsEdge,
        callback: dom_api::CallbackArg,
    ) {
        edge.set_embedder_word(cx.gc, self.callbacks[callback.0]);
    }

    fn object<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: &Taken,
    ) -> elise::Gc<'gc, T> {
        let Taken::Object(object) = object else {
            panic!("a node for an object");
        };
        erase(object.get(cx.gc))
            .downcast()
            .expect("an object of the type asked for")
    }

    fn ret_object<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        object: ObjectRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        self.ret = Some(Ret::Object(cx.root_object(object)));
        None
    }

    fn ret_objects<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, objects: &[ObjectRef<'gc>]) {
        self.ret = Some(Ret::Objects(
            objects
                .iter()
                .map(|object| cx.root_object(*object))
                .collect(),
        ));
    }

    fn this_as<'gc, T: elise::HeapType>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
    ) -> Option<elise::Gc<'gc, T>> {
        let erased = match &self.this {
            This::Node(node) => node.get(cx.gc).erase(),
            This::Object(object) => erase(object.get(cx.gc)),
            This::None => panic!("a call with no `this` asked for it"),
        };
        erased.downcast()
    }

    fn current_window<'gc>(&mut self, cx: &ApiCx<'_, 'gc>) -> Option<elise::Gc<'gc, dom::Window>> {
        self.listeners
            .current_window
            .borrow()
            .as_ref()
            .map(|window| window.get(cx.gc))
    }

    fn construct_object<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, object: ObjectRef<'gc>) {
        self.ret = Some(Ret::Object(cx.root_object(object)));
    }

    fn node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: &Self::Node) -> NodeRef<'gc> {
        match node {
            Taken::Node(node) => node.get(cx.gc),
            Taken::Object(_) => panic!("an object for a node"),
        }
    }

    fn ret_undefined(&mut self) {
        self.ret = Some(Ret::Undefined);
    }

    /// A promise resolved with undefined: as undefined here (the tests'
    /// engine has no promises).
    fn ret_resolved_undefined(&mut self) {
        self.ret = Some(Ret::Undefined);
    }

    fn ret_null(&mut self) {
        self.ret = Some(Ret::Null);
    }

    fn ret_bool(&mut self, value: bool) {
        self.ret = Some(Ret::Bool(value));
    }

    fn ret_u32(&mut self, value: u32) -> Option<std::convert::Infallible> {
        self.ret = Some(Ret::U32(value));
        None
    }

    fn ret_f64(&mut self, value: f64) -> Option<std::convert::Infallible> {
        self.ret = Some(Ret::F64(value));
        None
    }

    fn ret_string(&mut self, value: &DomString) -> Option<std::convert::Infallible> {
        self.hooks.string_refs = Some(value.reference_count());
        self.ret = Some(Ret::Str(value.clone()));
        None
    }

    fn ret_node<'gc>(
        &mut self,
        cx: &ApiCx<'_, 'gc>,
        node: NodeRef<'gc>,
    ) -> Option<std::convert::Infallible> {
        self.ret = Some(Ret::Node(cx.root(node)));
        None
    }

    fn ret_strings(&mut self, strings: Vec<DomString>) {
        self.ret = Some(Ret::Strings(strings));
    }

    fn ret_new_node<'gc>(&mut self, cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) {
        self.hooks.new_nodes += 1;
        let None = self.ret_node(cx, node);
    }
}

/// `fire_event` of `event_type` at `target`, the tests' front end its
/// script host: whether no listener canceled it.
pub fn fire(dom: &mut TestDom, target: &dom::TargetRoot, event_type: &str) -> bool {
    let mut front = TestFront::new(dom, This::None, &[]);
    dom_api::fire_event(
        &mut dom_api::FrontHost(&mut front),
        target,
        dom::names::intern(event_type),
        false,
        true,
    )
}

/// The tests' front end as the script host out of any member's call: its
/// listeners, and timers, whose calls it logs; the values it stores are
/// engine words.
pub struct TestHost<'a, 'd>(&'a mut TestFront<'d>);

impl dom_api::ScriptHost for TestHost<'_, '_> {
    type Value = u64;

    fn read<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        self.0.read(|cx, _| f(cx))
    }

    fn mutate<R>(&mut self, f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>) -> R) -> R {
        self.0.mutate(|cx, _| f(cx))
    }

    fn mutate_storing<R>(
        &mut self,
        f: impl for<'gc> FnOnce(&ApiCx<'_, 'gc>, &mut dyn FnMut(&elise::JsEdge, u64)) -> R,
    ) -> R {
        self.0
            .mutate(|cx, _| f(cx, &mut |edge, word| edge.set_embedder_word(cx.gc, word)))
    }

    fn call_listener(&mut self, event: &elise::Persistent<dom::Event>, index: usize) {
        self.0.call_listener(event, index);
    }

    fn perform_microtask_checkpoint(&mut self) {
        let checkpoints = &self.0.listeners.checkpoints;
        checkpoints.set(checkpoints.get() + 1);
    }

    fn call_mutation_callback(
        &mut self,
        observer: &elise::Persistent<dom::MutationObserver>,
        records: &[elise::Persistent<dom::MutationRecord>],
    ) {
        let call = self.read(|cx| {
            let callback = cx
                .observer_callback_edge(observer.get(cx.gc))
                .embedder_word(cx.gc);
            let types = records
                .iter()
                .map(|record| cx.record_kind(record.get(cx.gc)).name().to_owned())
                .collect();
            (callback, types)
        });
        self.0.listeners.mutation_log.borrow_mut().push(call);
    }

    fn call_timer(
        &mut self,
        window: &elise::Persistent<dom::Window>,
        timer: &elise::Persistent<dom::Timer>,
    ) {
        let call = self.read(|cx| {
            let timer = timer.get(cx.gc);
            TimerCall {
                callback: cx.timer_callback_edge(timer).embedder_word(cx.gc),
                arguments: cx.timer_arguments_edge(timer).embedder_word(cx.gc),
                nesting: cx.timer_nesting(window.get(cx.gc)),
            }
        });
        let callback = call.callback;
        self.0.listeners.timer_log.borrow_mut().push(call);
        // A timer's callback of number 100 or more makes a timer of that
        // number less 100 (`setTimeout(f, 0)` in a timer), with the
        // window's nesting level.
        if callback >= 100 {
            self.mutate(|cx| {
                let timer = cx.add_timer(window.get(cx.gc), 0.0, false);
                cx.timer_callback_edge(timer)
                    .set_embedder_word(cx.gc, callback - 100);
            });
        }
    }

    fn call_intersection_callback(
        &mut self,
        observer: &elise::Persistent<dom::IntersectionObserver>,
        entries: &[elise::Persistent<dom::IntersectionObserverEntry>],
    ) {
        let call = self.read(|cx| {
            let callback = cx
                .intersection_callback_edge(observer.get(cx.gc))
                .embedder_word(cx.gc);
            let entries = entries
                .iter()
                .map(|entry| {
                    let entry = entry.get(cx.gc);
                    let target = cx.intersection_entry_target(entry);
                    let id = cx
                        .get_attribute(target, "id")
                        .map(|id| id.to_string())
                        .unwrap_or_default();
                    let (time, intersecting, ratio, visible) = cx.intersection_entry_values(entry);
                    let rect = |rect| {
                        let [x, y, width, height] = cx.rect(rect);
                        format!("{x},{y},{width},{height}")
                    };
                    let root = cx
                        .intersection_entry_root_bounds(entry)
                        .map_or_else(|| "null".to_owned(), rect);
                    format!(
                        "{id} {time} {intersecting} {ratio} {visible} {root} {} {}",
                        rect(cx.intersection_entry_bounding_client_rect(entry)),
                        rect(cx.intersection_entry_intersection_rect(entry)),
                    )
                })
                .collect();
            (callback, entries)
        });
        self.0.listeners.intersection_log.borrow_mut().push(call);
    }

    /// A resize observer's callback of number 100 or more sets the
    /// `style` of its first entry's target to `width: 77px` (a change a
    /// callback makes).
    fn call_resize_callback(
        &mut self,
        observer: &elise::Persistent<dom::ResizeObserver>,
        entries: &[elise::Persistent<dom::ResizeObserverEntry>],
    ) {
        let call = self.read(|cx| {
            let callback = cx
                .resize_callback_edge(observer.get(cx.gc))
                .embedder_word(cx.gc);
            let entries = entries
                .iter()
                .map(|entry| {
                    let entry = entry.get(cx.gc);
                    let target = cx.resize_entry_target(entry);
                    let id = cx
                        .get_attribute(target, "id")
                        .map(|id| id.to_string())
                        .unwrap_or_default();
                    let [x, y, width, height] = cx.rect(cx.resize_entry_content_rect(entry));
                    let size = |b| {
                        let size = cx.resize_size(cx.resize_entry_size(entry, b));
                        format!("{}x{}", size.inline, size.block)
                    };
                    format!(
                        "{id} {x},{y},{width},{height} {} {} {}",
                        size(dom::ResizeObserverBox::BorderBox),
                        size(dom::ResizeObserverBox::ContentBox),
                        size(dom::ResizeObserverBox::DevicePixelContentBox),
                    )
                })
                .collect();
            (callback, entries)
        });
        let first = entries
            .first()
            .map(|entry| self.read(|cx| cx.node_id(cx.resize_entry_target(entry.get(cx.gc)))));
        let call_number = call.0;
        self.0.listeners.resize_log.borrow_mut().push(call);
        if call_number >= 100
            && let Some(target) = first
        {
            crate::geometry::fake_in(&self.0.listeners.layout, |layout| {
                if let Some(element_box) = layout.boxes.get_mut(&target) {
                    element_box.border_box.width = 77.0;
                }
            });
        }
    }

    /// An animation frame callback of number 100 or more requests one of
    /// that number less 100; 200 or more cancels the handle of that number
    /// less 200.
    fn call_animation_frame(
        &mut self,
        window: &elise::Persistent<dom::Window>,
        frame: &elise::Persistent<dom::AnimationFrame>,
        time: f64,
    ) {
        let callback = self.read(|cx| {
            cx.animation_frame_callback_edge(frame.get(cx.gc))
                .embedder_word(cx.gc)
        });
        self.0
            .listeners
            .frame_log
            .borrow_mut()
            .push((callback, time));
        self.mutate(|cx| {
            let window = window.get(cx.gc);
            if callback >= 200 {
                let handle = i32::try_from(callback - 200).unwrap();
                cx.take_animation_frame(window, handle);
            } else if callback >= 100 {
                let frame = cx.add_animation_frame(window);
                cx.animation_frame_callback_edge(frame)
                    .set_embedder_word(cx.gc, callback - 100);
            }
        });
    }
}

/// A host of the tests' front end, with no call.
pub fn with_host<R>(dom: &TestDom, f: impl FnOnce(&mut TestHost<'_, '_>) -> R) -> R {
    let mut front = TestFront::new(dom, This::None, &[]);
    f(&mut TestHost(&mut front))
}

/// A target's name in the tests' log: a node's name, `#window`, `#target`.
fn target_name<'gc>(cx: &dom::Cx<'_, 'gc>, target: dom::TargetRef<'gc>) -> String {
    match target {
        dom::TargetRef::Node(node) => cx.node_name(node).to_string(),
        dom::TargetRef::Window(_) => "#window".to_owned(),
        dom::TargetRef::EventTarget(_) => "#target".to_owned(),
    }
}

/// The callback number a timer's string handler is kept as.
pub const STRING_HANDLER: u64 = 77;

/// The error the tests report.
pub fn error_info() -> dom::ErrorInfo {
    dom::ErrorInfo {
        message: "Error: boom".into(),
        filename: "https://example.test/a.js".into(),
        lineno: 3,
        colno: 7,
    }
}

/// An object as an edge to any type.
pub fn erase(object: ObjectRef<'_>) -> elise::Gc<'_, elise::Erased> {
    match object {
        ObjectRef::Node(node) => node.erase(),
        ObjectRef::Event(event) => event.erase(),
        ObjectRef::EventTarget(target) => target.erase(),
        ObjectRef::Window(window) => window.erase(),
        ObjectRef::Location(location) => location.erase(),
        ObjectRef::History(history) => history.erase(),
        ObjectRef::Navigator(navigator) => navigator.erase(),
        ObjectRef::UserActivation(activation) => activation.erase(),
        ObjectRef::Url(url) => url.erase(),
        ObjectRef::UrlSearchParams(params) => params.erase(),
        ObjectRef::Crypto(crypto) => crypto.erase(),
        ObjectRef::IdleDeadline(deadline) => deadline.erase(),
        ObjectRef::AbortController(controller) => controller.erase(),
        ObjectRef::Headers(headers) => headers.erase(),
        ObjectRef::Request(request) => request.erase(),
        ObjectRef::Response(response) => response.erase(),
        ObjectRef::Geolocation(geolocation) => geolocation.erase(),
        ObjectRef::GeolocationPositionError(error) => error.erase(),
        ObjectRef::WakeLock(wake_lock) => wake_lock.erase(),
        ObjectRef::Scheduling(scheduling) => scheduling.erase(),
        ObjectRef::PerformanceEntry(entry) => entry.erase(),
        ObjectRef::PerformanceObserver(observer) => observer.erase(),
        ObjectRef::PerformanceObserverEntryList(list) => list.erase(),
        ObjectRef::PerformanceTiming(timing) => timing.erase(),
        ObjectRef::HtmlCollection(collection) => collection.erase(),
        ObjectRef::NodeList(list) => list.erase(),
        ObjectRef::DomTokenList(list) => list.erase(),
        ObjectRef::DomStringMap(map) => map.erase(),
        ObjectRef::CssStyleDeclaration(declaration) => declaration.erase(),
        ObjectRef::MutationObserver(observer) => observer.erase(),
        ObjectRef::MutationRecord(record) => record.erase(),
        ObjectRef::DomRect(rect) => rect.erase(),
        ObjectRef::DomRectList(list) => list.erase(),
        ObjectRef::DomImplementation(implementation) => implementation.erase(),
        ObjectRef::CssRule(rule) => rule.erase(),
        ObjectRef::CssRuleList(list) => list.erase(),
        ObjectRef::MediaList(list) => list.erase(),
        ObjectRef::KeyframeEffect(effect) => effect.erase(),
        ObjectRef::DocumentTimeline(timeline) => timeline.erase(),
        ObjectRef::StyleSheetList(list) => list.erase(),
        ObjectRef::CssStyleSheet(sheet) => sheet.erase(),
        ObjectRef::CustomElementRegistry(registry) => registry.erase(),
        ObjectRef::IntersectionObserver(observer) => observer.erase(),
        ObjectRef::IntersectionObserverEntry(entry) => entry.erase(),
        ObjectRef::ResizeObserver(observer) => observer.erase(),
        ObjectRef::ResizeObserverEntry(entry) => entry.erase(),
        ObjectRef::ResizeObserverSize(size) => size.erase(),
        ObjectRef::CanvasRenderingContext2d(context) => context.erase(),
        ObjectRef::CanvasGradient(gradient) => gradient.erase(),
        ObjectRef::CanvasPattern(pattern) => pattern.erase(),
        ObjectRef::ImageData(image_data) => image_data.erase(),
        ObjectRef::Blob(blob) => blob.erase(),
        ObjectRef::Path2d(path) => path.erase(),
        ObjectRef::TextMetrics(metrics) => metrics.erase(),
        ObjectRef::MessageChannel(channel) => channel.erase(),
        ObjectRef::SvgAnimatedString(animated) => animated.erase(),
        ObjectRef::SvgMatrix(matrix) => matrix.erase(),
        ObjectRef::SvgPoint(point) => point.erase(),
        ObjectRef::SvgRect(rect) => rect.erase(),
        ObjectRef::NamedNodeMap(map) => map.erase(),
        ObjectRef::DomParser(parser) => parser.erase(),
        ObjectRef::Range(range) => range.erase(),
        ObjectRef::NodeIterator(iterator) => iterator.erase(),
        ObjectRef::TreeWalker(walker) => walker.erase(),
        ObjectRef::Selection(selection) => selection.erase(),
    }
}

/// Finds a member's typed path and calls it.
struct Caller<'a, 'd> {
    interface: &'a str,
    name: &'a str,
    kind: MemberKind,
    front: &'a mut TestFront<'d>,
    found: bool,
}

impl TypedVisitor for Caller<'_, '_> {
    fn any<M: TypedMember>(&mut self) {
        // An overload's form: the one taking as many arguments as given
        // (WebIDL's overload resolution, by the count first).
        let desc = &dom_api::INTERFACES[M::INTERFACE].members[M::MEMBER];
        let given = self.front.args.len();
        let forms = dom_api::INTERFACES[M::INTERFACE]
            .members
            .iter()
            .filter(|member| member.name == M::NAME && member.kind == M::KIND)
            .count();
        let this_form = forms < 2 || (desc.required() <= given && given <= desc.params.len());
        if dom_api::INTERFACES[M::INTERFACE].name == self.interface
            && M::NAME == self.name
            && M::KIND == self.kind
            && this_form
        {
            assert!(!self.found, "one typed path a member");
            self.found = true;
            M::call(self.front);
        }
    }
}

/// `INTERFACES[interface]`'s member `name` of `kind`, called on `this` with
/// `args`: what it returned, or threw.
pub fn call(
    dom: &mut TestDom,
    this: &std::rc::Rc<NodeRoot>,
    interface: &str,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> Result<Ret, ApiError> {
    call_with_hooks(dom, this, interface, name, kind, args).0
}

/// `call`, and what it asked of its front end beyond that.
pub fn call_with_hooks(
    dom: &mut TestDom,
    this: &std::rc::Rc<NodeRoot>,
    interface: &str,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> (Result<Ret, ApiError>, Hooks) {
    call_on(dom, This::Node(this.clone()), interface, name, kind, args)
}

/// `call_with_hooks` with `this` of any kind (none for a constructor).
pub fn call_on(
    dom: &mut TestDom,
    this: This,
    interface: &str,
    name: &str,
    kind: MemberKind,
    args: &[Arg],
) -> (Result<Ret, ApiError>, Hooks) {
    let mut front = TestFront::new(dom, this, args);
    let mut caller = Caller {
        interface,
        name,
        kind,
        front: &mut front,
        found: false,
    };
    dom_api::visit_typed(&mut caller);
    assert!(
        caller.found,
        "{interface}.{name} ({kind:?}) has a typed path"
    );
    let result = match (front.ret, front.thrown) {
        (Some(ret), None) => Ok(ret),
        // A setter gives nothing back (V8's setters return nothing).
        (None, None) if kind == MemberKind::Setter => Ok(Ret::Undefined),
        (None, Some(error)) => Err(error),
        (ret, thrown) => panic!("returned {ret:?} and threw {thrown:?}"),
    };
    assert_eq!(
        front.hooks.finished,
        result.is_ok(),
        "finished unless it threw"
    );
    (result, front.hooks)
}

/// A getter's value.
pub fn get(dom: &mut TestDom, this: &std::rc::Rc<NodeRoot>, interface: &str, name: &str) -> Ret {
    call(dom, this, interface, name, MemberKind::Getter, &[]).unwrap()
}

/// A setter's call.
pub fn set(
    dom: &mut TestDom,
    this: &std::rc::Rc<NodeRoot>,
    interface: &str,
    name: &str,
    value: Arg,
) -> Result<(), ApiError> {
    call(dom, this, interface, name, MemberKind::Setter, &[value]).map(|ret| {
        assert!(matches!(ret, Ret::Undefined), "a setter returns nothing");
    })
}

/// A method's call.
pub fn method(
    dom: &mut TestDom,
    this: &std::rc::Rc<NodeRoot>,
    interface: &str,
    name: &str,
    args: &[Arg],
) -> Result<Ret, ApiError> {
    call(dom, this, interface, name, MemberKind::Method, args)
}

/// A new document's root (`html`: an HTML document, else an XML one).
pub fn document(dom: &mut TestDom, html: bool) -> std::rc::Rc<NodeRoot> {
    std::rc::Rc::new(dom.mutate(|cx| cx.root(cx.create_document(html))))
}

/// A root as an argument.
pub fn node(root: &NodeRoot, dom: &mut TestDom) -> Arg {
    Arg::Node(std::rc::Rc::new(dom.mutate(|cx| cx.root(root.get(cx.gc)))))
}

/// A result's node, rooted, to call members on.
pub fn rc(ret: Ret) -> std::rc::Rc<NodeRoot> {
    match ret {
        Ret::Node(node) => std::rc::Rc::new(node),
        other => panic!("not a node: {other:?}"),
    }
}

/// The `DOMException` name of `error`.
pub fn exception(error: ApiError) -> &'static str {
    match error {
        ApiError::Dom(exception) => exception.name.name(),
        ApiError::Type(message) => panic!("a TypeError: {message}"),
        ApiError::Range(message) => panic!("a RangeError: {message}"),
    }
}
