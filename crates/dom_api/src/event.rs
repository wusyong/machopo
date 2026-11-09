//! `Event`'s members (https://dom.spec.whatwg.org/#interface-event): its
//! constructor, what it says of itself, and what a listener does to its
//! dispatch (stopping it, canceling it).

use dom::{Event, EventFlags, TargetRef, names};
use elise::Gc;

use crate::interfaces::{CustomEventInit, ErrorEventInit, EventInit};
use crate::typed::{AnyArg, DomStr};
use crate::{ApiCx, ApiError};

/// HR-Time's "coarsen time" (https://w3c.github.io/hr-time/#dfn-coarsen-time)
/// for a context that isn't cross-origin isolated, which machopo has none of:
/// down to a multiple of 100 µs, without jitter (Blink's `TimeClamper`,
/// without its jitter).
pub fn coarsen(ms: f64) -> f64 {
    const STEPS_PER_MS: f64 = 10.0;
    (ms * STEPS_PER_MS).floor() / STEPS_PER_MS
}

/// `new Event(type, eventInitDict)`: initialized, not trusted, of the
/// current realm (its time since that window's time origin).
pub(crate) fn construct_event<'gc, O: crate::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &EventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    Ok(cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_window(cx),
    ))
}

pub(crate) fn event_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(cx.event_type(this)))
}

pub(crate) fn bubbles<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(cx.event_flags(this).bubbles)
}

pub(crate) fn cancelable<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(cx.event_flags(this).cancelable)
}

pub(crate) fn composed<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(cx.event_flags(this).composed)
}

pub(crate) fn is_trusted<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(cx.event_flags(this).trusted)
}

/// `eventPhase`: `NONE` out of a dispatch.
pub(crate) fn event_phase<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u16, ApiError> {
    Ok(cx.event_phase(this))
}

/// `target` (and `srcElement`): where it was dispatched last.
pub(crate) fn target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<TargetRef<'gc>>, ApiError> {
    Ok(cx.event_target(this))
}

/// `currentTarget`: whose listeners are being called; none out of a
/// dispatch.
pub(crate) fn current_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<TargetRef<'gc>>, ApiError> {
    Ok(cx.event_current_target(this))
}

/// `composedPath()`: the dispatch's path as the current target sees it
/// (DOM's algorithm), its target first: not into the closed shadow trees
/// the current target isn't in; empty out of a dispatch. An entry is a
/// "slot in a closed tree" if it is a slot of a closed shadow tree the
/// entry before it is assigned to (reached through its assignment, not
/// as its parent: the path doesn't keep DOM's flags, what the tree is
/// now tells).
pub(crate) fn composed_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Vec<TargetRef<'gc>>, ApiError> {
    let path = cx.event_path(this);
    // An entry that is a closed shadow root: what is inside it is hidden
    // from the entries outside.
    let closed_root = |target: TargetRef<'gc>| {
        matches!(target, TargetRef::Node(node) if cx
            .shadow_root_init(node)
            .is_some_and(|init| init.mode != dom::ShadowRootMode::Open))
    };
    let Some(current) = cx.event_current_target(this) else {
        return Ok(Vec::new());
    };
    if !path.iter().any(|&target| closed_root(target)) {
        return Ok(path);
    }
    let closed_slot = |index: usize| {
        let (Some(TargetRef::Node(slot)), Some(TargetRef::Node(before))) = (
            path.get(index),
            index.checked_sub(1).and_then(|before| path.get(before)),
        ) else {
            return false;
        };
        cx.is_slot(*slot)
            && !cx
                .parent(*before)
                .is_some_and(|parent| Gc::ptr_eq(parent, *slot))
            && cx
                .containing_shadow_root(*slot)
                .and_then(|root| cx.shadow_root_init(root))
                .is_some_and(|init| init.mode != dom::ShadowRootMode::Open)
    };
    // The current target's entry (a target is on a path once). DOM counts
    // how many closed trees deep it is from the path's end, but only
    // levels relative to its own are compared: it is level 0 here.
    let current_index = path
        .iter()
        .position(|target| target.ptr_eq(current))
        .unwrap_or(0);
    let current_level = 0_i32;
    let mut composed = std::collections::VecDeque::new();
    composed.push_back(current);
    // Toward the target: none deeper than the current target, nor in a
    // closed tree a slot before it leads into.
    let (mut level, mut max_level) = (current_level, current_level);
    for index in (0..current_index).rev() {
        if closed_root(path[index]) {
            level = level.saturating_add(1);
        }
        if level <= max_level {
            composed.push_front(path[index]);
        }
        if closed_slot(index) {
            level = level.saturating_sub(1);
            max_level = max_level.min(level);
        }
    }
    // Away from it: none past a closed tree it leaves, nor in a closed
    // tree a slot leads into.
    let (mut level, mut max_level) = (current_level, current_level);
    for (index, &node) in path
        .iter()
        .enumerate()
        .skip(current_index.saturating_add(1))
    {
        if closed_slot(index) {
            level = level.saturating_add(1);
        }
        if level <= max_level {
            composed.push_back(node);
        }
        if closed_root(node) {
            level = level.saturating_sub(1);
            max_level = max_level.min(level);
        }
    }
    Ok(composed.into())
}

fn update<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>, f: impl FnOnce(&mut dom::EventState)) {
    let mut state = cx.event_state(event);
    f(&mut state);
    cx.set_event_state(event, state);
}

pub(crate) fn stop_propagation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<(), ApiError> {
    update(cx, this, |state| state.stop_propagation = true);
    Ok(())
}

pub(crate) fn stop_immediate_propagation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<(), ApiError> {
    update(cx, this, |state| {
        state.stop_propagation = true;
        state.stop_immediate_propagation = true;
    });
    Ok(())
}

/// `cancelBubble`: the stop propagation flag; setting it `true` sets the
/// flag, `false` does nothing.
pub(crate) fn cancel_bubble<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(cx.event_state(this).stop_propagation)
}

pub(crate) fn set_cancel_bubble<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    value: bool,
) -> Result<(), ApiError> {
    if value {
        update(cx, this, |state| state.stop_propagation = true);
    }
    Ok(())
}

/// DOM's "set the canceled flag": only of a cancelable event, not in a
/// passive listener (an event handler's return value sets it too).
pub fn cancel<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) {
    let cancelable = cx.event_flags(event).cancelable;
    update(cx, event, |state| {
        if cancelable && !state.in_passive_listener {
            state.canceled = true;
        }
    });
}

pub(crate) fn prevent_default<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<(), ApiError> {
    cancel(cx, this);
    Ok(())
}

pub(crate) fn default_prevented<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(cx.event_state(this).canceled)
}

/// `returnValue`: whether it isn't canceled; setting it `false` cancels
/// it, `true` does nothing.
pub(crate) fn return_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(!cx.event_state(this).canceled)
}

pub(crate) fn set_return_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    value: bool,
) -> Result<(), ApiError> {
    if !value {
        cancel(cx, this);
    }
    Ok(())
}

/// `initEvent`: nothing while it is dispatched; else DOM's "initialize".
pub(crate) fn init_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
) -> Result<(), ApiError> {
    if !cx.event_state(this).dispatching {
        cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    }
    Ok(())
}

/// `new CustomEvent(type, eventInitDict)`: an event, as `new Event`'s,
/// with the init's `detail` (null without one).
pub(crate) fn construct_custom_event<'gc, O: crate::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &CustomEventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_event(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        op.current_window(cx),
    );
    cx.set_custom_event_data(event);
    if let Some(detail) = init.detail {
        let data = custom_data(cx, event);
        op.store_any(cx, cx.detail_edge(data), detail);
    }
    Ok(event)
}

/// `new ErrorEvent(type, eventInitDict)` (HTML's): its message, filename,
/// line, column and error as given.
pub(crate) fn construct_error_event<'gc, O: crate::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &ErrorEventInit,
) -> Result<Gc<'gc, Event>, ApiError> {
    let event = cx.create_error_event_of(
        names::intern(event_type),
        EventFlags {
            bubbles: init.bubbles,
            cancelable: init.cancelable,
            composed: init.composed,
            trusted: false,
        },
        &dom::ErrorInfo {
            message: dom::DomString::from(init.message.as_str()),
            filename: dom::DomString::from(init.filename.as_str()),
            lineno: init.lineno,
            colno: init.colno,
        },
        op.current_window(cx),
    );
    if let Some(error) = init.error {
        op.store_any(cx, cx.error_edge(error_data(cx, event)), error);
    }
    Ok(event)
}

/// A `CustomEvent`'s data, which its `this` has (the front end's checks).
fn custom_data<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> Gc<'gc, dom::CustomEventData> {
    cx.custom_event_data(event).expect("a custom event's data")
}

/// `CustomEvent`'s `detail`: null by default.
pub(crate) fn detail<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx.detail_edge(custom_data(cx, this)))
}

/// `initCustomEvent(type, bubbles, cancelable, detail)`: nothing while it
/// is dispatched; else DOM's "initialize", and the `detail`.
pub(crate) fn init_custom_event<'gc, O: crate::OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    detail: AnyArg,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    op.store_any(cx, cx.detail_edge(custom_data(cx, this)), detail);
    Ok(())
}

/// `timeStamp`: when it was made, relative to the DOM's start, coarsened.
pub(crate) fn time_stamp<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(coarsen(cx.event_time_stamp(this)))
}

/// An `ErrorEvent`'s data, which its `this` has (the front end's checks).
fn error_data<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> Gc<'gc, dom::ErrorEventData> {
    cx.error_event_data(event).expect("an error event's data")
}

/// `ErrorEvent`'s `message`, `filename`, `lineno`, `colno` and `error`
/// (https://html.spec.whatwg.org/#the-errorevent-interface).
pub(crate) fn message<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.error_info(error_data(cx, this)).message))
}

pub(crate) fn filename<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.error_info(error_data(cx, this)).filename))
}

pub(crate) fn lineno<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    Ok(cx.error_info(error_data(cx, this)).lineno)
}

pub(crate) fn colno<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    Ok(cx.error_info(error_data(cx, this)).colno)
}

pub(crate) fn error<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    Ok(cx.error_edge(error_data(cx, this)))
}

/// `PromiseRejectionEvent`'s `promise` and `reason`
/// (https://html.spec.whatwg.org/#the-promiserejectionevent-interface).
pub(crate) fn promise<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    let data = cx.promise_rejection_data(this).expect("a rejection's data");
    Ok(cx.promise_edge(data))
}

pub(crate) fn reason<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    let data = cx.promise_rejection_data(this).expect("a rejection's data");
    Ok(cx.reason_edge(data))
}
