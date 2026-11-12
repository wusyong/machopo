//! `EventTarget`'s members (https://dom.spec.whatwg.org/#interface-eventtarget):
//! its constructor, its listeners, and the DOM's dispatch
//! (https://dom.spec.whatwg.org/#dispatching-events), which calls script:
//! it runs out of any operation, opening its own between the listeners it
//! calls (`ScriptHost::call_listener`), with the event alone rooted (its
//! path and the listeners being called are on it).

use dom::{
    Event, EventFlags, EventTargetImpl, ListenerFlags, NameId, NodeRef, TargetRef, TargetRoot,
    names,
};
use elise::{Gc, Persistent};

use crate::interfaces::{AddEventListenerOptions, EventListenerOptions};
use crate::typed::{Callback, DictOrBool, OpFront, ScriptHost};
use crate::{ApiCx, ApiError, ExceptionName, activation};

/// `eventPhase`'s values.
const CAPTURING_PHASE: u16 = 1;
const AT_TARGET: u16 = 2;
const BUBBLING_PHASE: u16 = 3;

/// `new EventTarget()`: a target that is nothing else, of the current
/// realm's window.
pub(crate) fn construct_event_target<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let target = cx.create_event_target();
    cx.set_event_target_window(target, op.current_window(cx));
    Ok(target)
}

/// `addEventListener`: a listener of the callback, unless the target has
/// one of the same type, callback and capture already; nothing for a null
/// callback.
pub(crate) fn add_event_listener<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: TargetRef<'gc>,
    event_type: &str,
    callback: Option<Callback<O::Callback>>,
    options: &DictOrBool<AddEventListenerOptions<O::Node>>,
) -> Result<(), ApiError> {
    let Some(callback) = callback else {
        return Ok(());
    };
    let (capture, once, passive, signal) = match options {
        DictOrBool::Dict(options) => (
            options.capture,
            options.once,
            options.passive,
            options.signal.as_ref(),
        ),
        DictOrBool::Bool(capture) => (*capture, false, None, None),
    };
    // DOM's: an `AbortSignal`'s (checked here, as WebIDL's conversion
    // would); one aborted already adds nothing.
    let signal = match signal {
        Some(signal) => {
            let signal = op.object::<EventTargetImpl>(cx, &signal.0);
            if !cx.is_abort_signal(signal) {
                return Err(ApiError::type_error(
                    "Failed to read the 'signal' property from 'AddEventListenerOptions': Failed to convert value to 'AbortSignal'.",
                ));
            }
            if cx.abort_signal_aborted(signal) {
                return Ok(());
            }
            Some(signal)
        }
        None => None,
    };
    let event_type = names::intern(event_type);
    let same = cx.find_listener(this, event_type, capture, |edge| {
        op.same_callback(cx, edge, callback)
    });
    if same.is_none() {
        let flags = ListenerFlags {
            capture,
            once,
            passive: passive.unwrap_or_else(|| cx.default_passive(this, event_type)),
        };
        let listener = cx.add_listener(this, event_type, flags);
        op.store_callback(cx, cx.callback_edge(listener), callback);
        if let Some(signal) = signal {
            cx.add_listener_abort_algorithm(signal, this, listener);
        }
    }
    Ok(())
}

/// `removeEventListener`: the listener of the type, callback and capture
/// removed, if the target has it.
pub(crate) fn remove_event_listener<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: TargetRef<'gc>,
    event_type: &str,
    callback: Option<Callback<O::Callback>>,
    options: &DictOrBool<EventListenerOptions>,
) -> Result<(), ApiError> {
    let Some(callback) = callback else {
        return Ok(());
    };
    let capture = match options {
        DictOrBool::Dict(options) => options.capture,
        DictOrBool::Bool(capture) => *capture,
    };
    // A type no name has is one no listener has.
    let Some(event_type) = names::find(event_type) else {
        return Ok(());
    };
    if let Some(listener) = cx.find_listener(this, event_type, capture, |edge| {
        op.same_callback(cx, edge, callback)
    }) {
        cx.remove_listener(this, listener);
    }
    Ok(())
}

/// `dispatchEvent`: an event being dispatched is an `InvalidStateError`;
/// else it is no longer trusted, and dispatched. Whether no listener
/// canceled it.
pub(crate) fn dispatch_event<H: ScriptHost>(
    host: &mut H,
    this: TargetRoot,
    event: Persistent<Event>,
) -> Result<bool, ApiError> {
    // DOM's: one being dispatched, or one `createEvent` made and nothing
    // initialized, is an `InvalidStateError`.
    let dispatchable = host.read(|cx| {
        let event = event.get(cx.gc);
        let dispatchable = !cx.event_state(event).dispatching && cx.event_initialized(event);
        if dispatchable {
            cx.set_event_trusted(event, false);
        }
        dispatchable
    });
    if !dispatchable {
        return Err(ApiError::dom(ExceptionName::InvalidState));
    }
    Ok(dispatch(host, &this, &event))
}

/// An event the user agent fires (DOM's "fire an event", trusted) at
/// `target`, of `event_type` (`names::event`'s): whether no listener
/// canceled it.
pub fn fire_event<H: ScriptHost>(
    host: &mut H,
    target: &TargetRoot,
    event_type: NameId,
    bubbles: bool,
    cancelable: bool,
) -> bool {
    let event = host.mutate(|cx| {
        let window = cx.target_window(target.get(cx.gc));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles,
                cancelable,
                composed: false,
                trusted: true,
            },
            window,
        );
        Persistent::new(cx.gc, event)
    });
    dispatch(host, target, &event)
}

/// The DOM's dispatch of `event` at `target` (its "event path" without
/// shadow trees or a window yet: the target and its ancestors): the
/// capture pass from the root, the bubble pass back, then the event's
/// dispatch state reset. A click's activation target (the target if it
/// has an activation behavior, else, if it bubbles, the first of the path
/// that has one) has its legacy-pre-activation behavior before, and its
/// activation behavior after if no listener canceled it, else its
/// legacy-canceled-activation behavior. Whether no listener canceled it.
pub fn dispatch<H: ScriptHost>(
    host: &mut H,
    target: &TargetRoot,
    event: &Persistent<Event>,
) -> bool {
    // Chrome's trace's span of a dispatch (Blink's `EventDispatcher`).
    let _span = trace_event::trace_event!("EventDispatch");
    let (len, bubbles, legacy, shadow, activation) = host.mutate(|cx| {
        let (event, target) = (event.get(cx.gc), target.get(cx.gc));
        let mut state = cx.event_state(event);
        state.dispatching = true;
        cx.set_event_state(event, state);
        cx.set_event_target(event, Some(target));
        let related = cx.event_related_target(event);
        // A target or related target in a shadow tree: DOM's path through
        // the hosts, each retargeted to; else the target and its
        // ancestors, as most are. A slotted target's path goes into a
        // shadow tree through its slot and out through the host, but the
        // plain path is DOM's for it: a target outside shadow trees is
        // every entry's own (retargeted against anything), no host is at
        // the target, its shadow roots aren't the first's root.
        let mut shadow = in_shadow_tree(cx, target)
            || related.is_some_and(|related| in_shadow_tree(cx, related));
        let path = if shadow {
            shadow_path(cx, event, target, related)
        } else {
            let mut path = vec![target];
            let mut at = target;
            while let Some(parent) = parent_of(cx, event, at, None) {
                // Through a slot, into its shadow tree: its entries are
                // noted as such (`shadow_entries`).
                shadow |= in_shadow_tree(cx, parent);
                path.push(parent);
                at = parent;
            }
            path
        };
        cx.set_event_path(event, &path);
        if shadow {
            shadow_entries(cx, event, &path, related);
        }
        let bubbles = cx.event_flags(event).bubbles;
        let activation = activation::is_activation_event(cx, event)
            .then(|| activation::activation_target(cx, &path, bubbles))
            .flatten();
        (
            path.len(),
            bubbles,
            legacy_type_of(cx, event),
            shadow,
            activation,
        )
    });
    let activation = activation.map(|target| activation::pre_activation(host, target));
    if shadow {
        // At each host as at the target; the others, bubbling, only if it
        // bubbles.
        invoke_pass::<_, true>(host, event, Pass::Capture, len, legacy, true);
        invoke_pass::<_, true>(host, event, Pass::Bubble, len, legacy, bubbles);
    } else {
        invoke_pass::<_, false>(host, event, Pass::Capture, len, legacy, true);
        invoke_pass::<_, false>(
            host,
            event,
            Pass::Bubble,
            if bubbles { len } else { 1 },
            legacy,
            true,
        );
    }
    let not_canceled = host.read(|cx| {
        let event = event.get(cx.gc);
        if shadow {
            shadow_dispatched(cx, event, len);
        }
        cx.set_event_phase(event, 0);
        cx.set_event_current_target(event, None);
        cx.clear_event_path(event);
        let mut state = cx.event_state(event);
        let canceled = state.canceled;
        state.dispatching = false;
        state.stop_propagation = false;
        state.stop_immediate_propagation = false;
        cx.set_event_state(event, state);
        !canceled
    });
    if let Some(activation) = activation {
        activation::post_activation(host, activation, event, not_canceled);
    }
    not_canceled
}

/// `dispatch` at `at` with `target` as the event's target (HTML's legacy
/// target override: the window's `load`, its target the document): `at`
/// alone is the path. Not on `dispatch`'s path, which stays as it is.
pub(crate) fn dispatch_with_target<H: ScriptHost>(
    host: &mut H,
    at: &TargetRoot,
    target: &TargetRoot,
    event: &Persistent<Event>,
) -> bool {
    let legacy = host.mutate(|cx| {
        let event = event.get(cx.gc);
        let mut state = cx.event_state(event);
        state.dispatching = true;
        cx.set_event_state(event, state);
        cx.set_event_target(event, Some(target.get(cx.gc)));
        cx.set_event_path(event, &[at.get(cx.gc)]);
        legacy_type_of(cx, event)
    });
    invoke_pass::<_, false>(host, event, Pass::Capture, 1, legacy, true);
    invoke_pass::<_, false>(host, event, Pass::Bubble, 1, legacy, true);
    host.read(|cx| {
        let event = event.get(cx.gc);
        cx.set_event_phase(event, 0);
        cx.set_event_current_target(event, None);
        cx.clear_event_path(event);
        let mut state = cx.event_state(event);
        let canceled = state.canceled;
        state.dispatching = false;
        state.stop_propagation = false;
        state.stop_immediate_propagation = false;
        cx.set_event_state(event, state);
        !canceled
    })
}

/// DOM's "get the parent" of a target for `event`: a node's assigned slot
/// if it has one, else its parent; a shadow root's host, unless the event
/// isn't composed and it is
/// `first_root`, the root of the path's first target; a document's window
/// (HTML's document's "get the parent", none for a `load` event, which a
/// window gets on its own: its type compared only there); none for
/// another target.
fn parent_of<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    event: Gc<'gc, Event>,
    target: TargetRef<'gc>,
    first_root: Option<NodeRef<'gc>>,
) -> Option<TargetRef<'gc>> {
    match target {
        TargetRef::Node(node) => match cx.parent(node) {
            Some(parent) if cx.is_shadow_host(parent) => {
                Some(TargetRef::Node(cx.assigned_slot(node).unwrap_or(parent)))
            }
            Some(parent) => Some(TargetRef::Node(parent)),
            None if cx.kind(node) == dom::NodeKind::Document => cx
                .window_of(node)
                .filter(|_| cx.event_type(event) != names::event::LOAD)
                .map(TargetRef::Window),
            None if cx.is_shadow_root(node) => {
                let first = first_root.is_some_and(|first| Gc::ptr_eq(first, node));
                if first && !cx.event_flags(event).composed {
                    return None;
                }
                cx.shadow_host(node).map(TargetRef::Node)
            }
            None => None,
        },
        TargetRef::Window(_) | TargetRef::EventTarget(_) => None,
    }
}

/// What each entry of `path`, a dispatch's with an entry in a shadow tree,
/// sees, worked out now, before any listener (DOM's dispatch's structs,
/// made as the path is): the target and the related target retargeted
/// against it, and whether it is in a shadow tree; and whether the
/// targets the last entry sees are cleared at the end (DOM's "clear
/// targets": the event mustn't give one in a shadow tree away).
#[cold]
#[inline(never)]
fn shadow_entries<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    event: Gc<'gc, Event>,
    path: &[TargetRef<'gc>],
    related: Option<TargetRef<'gc>>,
) {
    let mut targets = path.first().map(|target| Retargeter::new(*target));
    let mut relateds = related.map(Retargeter::new);
    let entries: Vec<dom::ShadowEntry<'gc>> = path
        .iter()
        .map(|at| dom::ShadowEntry {
            target: targets
                .as_mut()
                .map_or(*at, |targets| targets.against(cx, *at)),
            related: relateds.as_mut().map(|relateds| relateds.against(cx, *at)),
            in_shadow: in_shadow_tree(cx, *at),
        })
        .collect();
    let clear = entries.last().is_some_and(|last| {
        in_shadow_tree(cx, last.target)
            || last
                .related
                .is_some_and(|related| in_shadow_tree(cx, related))
    });
    cx.set_event_shadow_path(event, &entries, clear);
}

/// Whether `target` is a node in a shadow tree, or a shadow root.
fn in_shadow_tree<'gc>(cx: &dom::Cx<'_, 'gc>, target: TargetRef<'gc>) -> bool {
    matches!(target, TargetRef::Node(node) if cx.is_shadow_root(node) || cx.containing_shadow_root(node).is_some())
}

/// The tree scope DOM's retarget reads its `b` through: the shadow root a
/// node is in or is (none: a document's tree), or no node's.
#[derive(Clone, Copy)]
enum ScopeKey<'gc> {
    Node(Option<NodeRef<'gc>>),
    NotNode,
}

impl<'gc> ScopeKey<'gc> {
    fn of(cx: &dom::Cx<'_, 'gc>, target: TargetRef<'gc>) -> Self {
        match target {
            TargetRef::Node(node) if cx.is_shadow_root(node) => Self::Node(Some(node)),
            TargetRef::Node(node) => Self::Node(cx.containing_shadow_root(node)),
            TargetRef::Window(_) | TargetRef::EventTarget(_) => Self::NotNode,
        }
    }

    fn same(self, other: Self) -> bool {
        match (self, other) {
            (Self::Node(a), Self::Node(b)) => match (a, b) {
                (Some(a), Some(b)) => Gc::ptr_eq(a, b),
                (None, None) => true,
                _ => false,
            },
            (Self::NotNode, Self::NotNode) => true,
            _ => false,
        }
    }
}

/// `retarget_target` of one `a` against each of a run of targets, the
/// last scope's result kept: retarget reads `b` only through its tree
/// scope (`ScopeKey`), so the `b`s of one scope, as a path's runs are, give
/// the same, worked out once a run, not a walk of the shadow trees each
/// (Blink's `EventPath::BuildRelatedNodeMap` finds the related node once a
/// tree scope).
struct Retargeter<'gc> {
    a: TargetRef<'gc>,
    last: Option<(ScopeKey<'gc>, TargetRef<'gc>)>,
}

impl<'gc> Retargeter<'gc> {
    fn new(a: TargetRef<'gc>) -> Self {
        Self { a, last: None }
    }

    fn against(&mut self, cx: &dom::Cx<'_, 'gc>, b: TargetRef<'gc>) -> TargetRef<'gc> {
        let key = ScopeKey::of(cx, b);
        if let Some((last, result)) = self.last
            && last.same(key)
        {
            return result;
        }
        let result = retarget_target(cx, self.a, b);
        self.last = Some((key, result));
        result
    }
}

/// DOM's "retarget" of an event target `a` against `b`: only a node is
/// retargeted, against a node (a window is none's).
fn retarget_target<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    a: TargetRef<'gc>,
    b: TargetRef<'gc>,
) -> TargetRef<'gc> {
    match a {
        TargetRef::Node(a) => {
            let b = match b {
                TargetRef::Node(b) => Some(b),
                TargetRef::Window(_) | TargetRef::EventTarget(_) => None,
            };
            TargetRef::Node(cx.retarget(a, b))
        }
        other => other,
    }
}

/// DOM's dispatch's path of `event` at `target` with its related target
/// `related`, a shadow tree in it (steps 5 to 5.10): none if
/// `target` is its retargeted related target and not the related target
/// itself (a `mouseover` within one host); else from the target out, a
/// shadow root on to its host (as `parent_of` says), stopping where the
/// retargeted related target is the parent, out of the target's tree (the
/// targets each entry sees are `shadow_entries`', worked out before any
/// listener).
/// A slottable goes on to its slot, into the shadow tree, whose root's
/// host is in the target's tree: the target stays.
fn shadow_path<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    event: Gc<'gc, Event>,
    target: TargetRef<'gc>,
    related: Option<TargetRef<'gc>>,
) -> Vec<TargetRef<'gc>> {
    if let Some(related) = related
        && retarget_target(cx, related, target).ptr_eq(target)
        && !related.ptr_eq(target)
    {
        return Vec::new();
    }
    let first_root = match target {
        TargetRef::Node(node) => Some(cx.root_node(node, false)),
        TargetRef::Window(_) | TargetRef::EventTarget(_) => None,
    };
    let mut path = vec![target];
    // DOM's "target": the outermost host the path went through.
    let mut current = target;
    let mut at = target;
    let mut relateds = related.map(Retargeter::new);
    // Whether the last parent's tree scope is in the tree of the target so
    // far: it reads the parent only through its scope, which a path keeps
    // for a run of parents (`Retargeter`).
    let mut last_in: Option<(ScopeKey<'gc>, bool)> = None;
    while let Some(parent) = parent_of(cx, event, at, first_root) {
        // Whether the parent is in the tree of the target so far (a
        // window is the document's): it sees that target, not itself.
        let key = ScopeKey::of(cx, parent);
        let in_targets_tree = match last_in {
            Some((last, in_tree)) if last.same(key) => in_tree,
            _ => {
                let in_tree = match (current, parent) {
                    (TargetRef::Node(current), TargetRef::Node(parent)) => {
                        match cx
                            .containing_shadow_root(current)
                            .or_else(|| cx.is_shadow_root(current).then_some(current))
                        {
                            Some(root) => cx.is_in_shadow_tree_of(root, parent),
                            None => true,
                        }
                    }
                    _ => true,
                };
                last_in = Some((key, in_tree));
                in_tree
            }
        };
        if !in_targets_tree {
            if relateds
                .as_mut()
                .is_some_and(|relateds| relateds.against(cx, parent).ptr_eq(parent))
            {
                break;
            }
            current = parent;
            // The target so far is another: its tree another.
            last_in = None;
        }
        path.push(parent);
        at = parent;
    }
    path
}

/// The end of a dispatch with a shadow tree in its path of `len`: its
/// target and related target as the path's last entry saw them (what DOM
/// leaves of its last invoke), or none if they are cleared
/// (`shadow_entries`).
fn shadow_dispatched<'gc>(cx: &dom::Cx<'_, 'gc>, event: Gc<'gc, Event>, len: usize) {
    let last = len
        .checked_sub(1)
        .and_then(|last| cx.event_shadow_entry(event, last));
    let clear = cx.event_shadow_clears(event);
    cx.clear_event_shadow_path(event);
    let Some(last) = last else {
        cx.set_event_target(event, None);
        return;
    };
    if clear {
        cx.set_event_target(event, None);
        cx.set_event_related_target(event, None);
    } else {
        cx.set_event_target(event, Some(last.target));
        if last.related.is_some() {
            cx.set_event_related_target(event, last.related);
        }
    }
}

/// A pass of the dispatch over the path: capturing, from the root down
/// to the target, or bubbling, from the target up.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pass {
    Capture,
    Bubble,
}

/// DOM's "invoke" of each of the path's first `count` targets for one
/// pass, in its order (the target's in either at `AT_TARGET`): the
/// listeners of each from a snapshot taken as it is reached ("inner
/// invoke"), one removed since skipped, a `once` one removed before its
/// call, a passive one called with `preventDefault` doing nothing. The
/// targets with no listeners for it are passed in the same operation, up
/// to the next that has some (most of a path has none): an operation
/// ends only to call script, as Blink's `EventDispatcher` asks each node
/// `HasEventListeners` in place.
///
/// With a shadow tree in the path (`SHADOW`, a version of its own: the
/// others' path stays as it was), each entry sees its own target, a host
/// the path went through is at the target, and the others than at the
/// target are invoked if `others` (it bubbles, or it is the capture pass).
fn invoke_pass<H: ScriptHost, const SHADOW: bool>(
    host: &mut H,
    event: &Persistent<Event>,
    pass: Pass,
    count: usize,
    legacy: Option<NameId>,
    others: bool,
) {
    let mut at = 0;
    while at < count {
        let listeners = host.mutate(|cx| {
            let event = event.get(cx.gc);
            while at < count {
                if cx.event_state(event).stop_propagation {
                    return None;
                }
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "`at < count`, the loop's condition: `count - 1 - at` is at least 0, `at + 1` at most `count`"
                )]
                let index = match pass {
                    Pass::Capture => count - 1 - at,
                    Pass::Bubble => at,
                };
                #[expect(clippy::arithmetic_side_effects, reason = "`at < count`, a `usize`")]
                {
                    at += 1;
                }
                let target = cx
                    .event_path_at(event, index)
                    .expect("an index of the path");
                let at_target = if SHADOW {
                    let shown = cx
                        .event_shadow_entry(event, index)
                        .expect("an entry of the path")
                        .target;
                    let at_target = index == 0 || shown.ptr_eq(target);
                    if !at_target && !others {
                        continue;
                    }
                    cx.set_event_target(event, Some(shown));
                    cx.set_event_shadow_current(event, index);
                    at_target
                } else {
                    index == 0
                };
                let phase = match (at_target, pass) {
                    (true, _) => AT_TARGET,
                    (false, Pass::Capture) => CAPTURING_PHASE,
                    (false, Pass::Bubble) => BUBBLING_PHASE,
                };
                cx.set_event_phase(event, phase);
                cx.set_event_current_target(event, Some(target));
                // Allocates the snapshot's list the first time.
                let listeners = cx.snapshot_listeners(event, target);
                if listeners > 0 {
                    return Some((listeners, None));
                }
                // DOM's legacy event types: a trusted event's listeners of
                // its legacy type (`legacy`, found once for the dispatch)
                // where there are none of its own, called with that type.
                if let Some(legacy_type) = legacy {
                    let listeners = cx.snapshot_legacy_listeners(event, target, legacy_type);
                    if listeners > 0 {
                        let original = cx.event_type(event);
                        cx.set_event_type(event, legacy_type);
                        return Some((listeners, Some(original)));
                    }
                }
            }
            None
        });
        match listeners {
            Some((listeners, original)) => {
                invoke_listeners(host, event, listeners, pass == Pass::Capture);
                if let Some(original) = original {
                    host.read(|cx| cx.set_event_type(event.get(cx.gc), original));
                }
            }
            None => return,
        }
    }
}

/// DOM's legacy event type of `event` (with Blink's: `wheel`'s
/// `mousewheel`, which Chrome still fires), if it is trusted and has one.
fn legacy_type_of<'gc>(cx: &dom::Cx<'_, 'gc>, event: Gc<'gc, Event>) -> Option<NameId> {
    use names::event;
    if !cx.event_flags(event).trusted {
        return None;
    }
    Some(match cx.event_type(event) {
        event::ANIMATIONEND => event::WEBKIT_ANIMATION_END,
        event::ANIMATIONITERATION => event::WEBKIT_ANIMATION_ITERATION,
        event::ANIMATIONSTART => event::WEBKIT_ANIMATION_START,
        event::TRANSITIONEND => event::WEBKIT_TRANSITION_END,
        event::WHEEL => event::MOUSEWHEEL,
        _ => return None,
    })
}

/// The event handler at `at` of `event`'s snapshot compiled, at its
/// current target: out of line, as few are uncompiled.
#[inline(never)]
fn compile_handler<H: ScriptHost>(host: &mut H, event: &Persistent<Event>, at: usize) {
    let (target, event_type) = host.read(|cx| {
        let event = event.get(cx.gc);
        let listener = cx.snapshot_at(event, at).expect("an index of the snapshot");
        let target = cx.event_current_target(event).expect("invoked at a target");
        (cx.root_target(target), cx.listener_type(listener))
    });
    host.compile_event_handler(&target, event_type);
}

/// Whether the document of `event`'s current target's relevant global is
/// fully active (DOM's "inner invoke" calls no listener of a target whose
/// isn't: an iframe removed, or navigated from; Blink's, of an
/// `ExecutionContext` destroyed): a node's document's (one of no window,
/// the window whose script made it), a window's, an event target's
/// window. One of no window known is. Asked only once a document was
/// discarded.
#[cold]
#[inline(never)]
fn current_target_fully_active<'gc>(cx: &dom::Cx<'_, 'gc>, event: Gc<'gc, Event>) -> bool {
    let target = cx.event_current_target(event).expect("invoked at a target");
    let window = match target {
        TargetRef::Node(node) => cx.document_relevant_window(cx.document_of(node)),
        TargetRef::Window(_) | TargetRef::EventTarget(_) => cx.target_window(target),
    };
    window.is_none_or(|window| {
        let document = cx.window_document(window);
        cx.window_of(document).is_some() && !cx.is_discarded(document)
    })
}

/// `invoke`'s listeners (`count` of the snapshot) for one pass: out of
/// line, so that a target with none (most of a path) pays for its
/// operation alone, not for the registers the calls of script take.
#[inline(never)]
fn invoke_listeners<H: ScriptHost>(
    host: &mut H,
    event: &Persistent<Event>,
    count: usize,
    capture: bool,
) {
    for at in 0..count {
        // `Some` to call it, `Some(true)` an event handler's to compile
        // first (HTML's "getting the current value").
        // Its relevant global's document gone: none of its listeners are
        // called.
        let mut gone = false;
        let call = host.read(|cx| {
            let event = event.get(cx.gc);
            if cx.has_discarded_documents() && !current_target_fully_active(cx, event) {
                gone = true;
                return None;
            }
            let listener = cx.snapshot_at(event, at).expect("an index of the snapshot");
            let flags = cx.listener_flags(listener);
            if cx.listener_removed(listener) || flags.capture != capture {
                return None;
            }
            if flags.once {
                let target = cx.event_current_target(event).expect("invoked at a target");
                cx.remove_listener(target, listener);
            }
            if flags.passive {
                let mut state = cx.event_state(event);
                state.in_passive_listener = true;
                cx.set_event_state(event, state);
            }
            Some(cx.listener_is_handler(listener) && cx.handler_uncompiled(listener))
        });
        if gone {
            return;
        }
        let Some(compile) = call else {
            continue;
        };
        if compile {
            compile_handler(host, event, at);
        }
        host.call_listener(event, at);
        let stop = host.read(|cx| {
            let event = event.get(cx.gc);
            let mut state = cx.event_state(event);
            state.in_passive_listener = false;
            cx.set_event_state(event, state);
            state.stop_immediate_propagation
        });
        if stop {
            return;
        }
    }
}
