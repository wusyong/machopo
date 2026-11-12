//! HTML's event handlers' IDL attributes (`el.onclick`): one getter and one
//! setter for every type, the type a constant of each member (the table's
//! `GlobalEventHandlers` and `WindowEventHandlers`, `dom::names`'
//! lists). A handler is a listener of its own on its target (`dom`'s
//! `set_compiled_handler`, `set_uncompiled_handler`): set, it keeps its
//! place among the target's listeners; null, it is taken out. A content
//! attribute's (`<button onclick>`) is set by `dom` as the attribute
//! changes, uncompiled; its engine compiles it the first time it is needed
//! (`ScriptHost::compile_event_handler`), after its getter's operation
//! (`EventHandler`'s result) or as it is invoked.
//! `<body>`'s and `<frameset>`'s window-reflecting ones are their window's.

use dom::{NameId, NodeRef, TargetRef, Window, names};
use elise::Gc;

use crate::typed::{Callback, HandlerValue, OpFront};
use crate::{ApiCx, ApiError};

/// Where `element`'s handler of `event_type` is: its own, or for a
/// `<body>`'s or `<frameset>`'s window-reflecting one or one of its
/// `WindowEventHandlers`, its document's window (none without one).
fn element_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    element: NodeRef<'gc>,
    event_type: NameId,
) -> Option<TargetRef<'gc>> {
    let reflects = (names::WINDOW_REFLECTING.contains(&event_type)
        || names::WINDOW_EVENT_HANDLERS
            .iter()
            .any(|&(_, of)| of == event_type))
        && cx.element_name(element).is_some_and(|name| {
            name.namespace == names::HTML_NAMESPACE
                && (name.local == names::local::BODY || name.local == names::local::FRAMESET)
        });
    if reflects {
        return cx.window_of(cx.document_of(element)).map(TargetRef::Window);
    }
    Some(TargetRef::Node(element))
}

/// HTML's "getting the current value of the event handler": its
/// listener, none if null; an uncompiled one is compiled after the
/// operation (`HandlerValue::Uncompiled`).
#[inline(always)]
fn current_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    target: TargetRef<'gc>,
    event_type: NameId,
) -> HandlerValue<'gc> {
    match cx.event_handler(target, event_type) {
        Some(listener) if cx.handler_uncompiled(listener) => {
            HandlerValue::Uncompiled(target, event_type)
        }
        Some(listener) => HandlerValue::Listener(listener),
        None => HandlerValue::None,
    }
}

/// HTML's setter of an event handler IDL attribute: an object makes it
/// that callback (its listener kept where it is, or added last); null
/// deactivates it.
fn set_handler<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    target: TargetRef<'gc>,
    event_type: NameId,
    value: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    match value {
        Some(callback) => {
            let listener = cx.set_compiled_handler(target, event_type);
            op.store_callback(cx, cx.callback_edge(listener), callback);
        }
        None => cx.remove_event_handler(target, event_type),
    }
    Ok(())
}

/// An HTML element's handler of `T` (its window's for `<body>`'s
/// window-reflecting ones).
pub(crate) fn element_handler<'gc, const T: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<HandlerValue<'gc>, ApiError> {
    Ok(match element_target(cx, this, T) {
        Some(target) => current_value(cx, target, T),
        None => HandlerValue::None,
    })
}

pub(crate) fn set_element_handler<'gc, O: OpFront, const T: NameId>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    match element_target(cx, this, T) {
        Some(target) => set_handler(op, cx, target, T, value),
        None => Ok(()),
    }
}

/// A document's handler of `T`.
pub(crate) fn document_handler<'gc, const T: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<HandlerValue<'gc>, ApiError> {
    Ok(current_value(cx, TargetRef::Node(this), T))
}

pub(crate) fn set_document_handler<'gc, O: OpFront, const T: NameId>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    set_handler(op, cx, TargetRef::Node(this), T, value)
}

/// A window's handler of `T`.
pub(crate) fn port_handler<'gc, const T: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, dom::EventTargetImpl>,
) -> Result<HandlerValue<'gc>, ApiError> {
    Ok(current_value(cx, TargetRef::EventTarget(this), T))
}

pub(crate) fn set_port_handler<'gc, O: OpFront, const T: NameId>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, dom::EventTargetImpl>,
    value: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    set_handler(op, cx, TargetRef::EventTarget(this), T, value)
}

/// A port's `onmessage` set: its messages come from now on too (HTML's:
/// as `start()`).
pub(crate) fn set_port_message_handler<'gc, O: OpFront, const T: NameId>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, dom::EventTargetImpl>,
    value: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    set_handler(op, cx, TargetRef::EventTarget(this), T, value)?;
    cx.start_message_port(this);
    Ok(())
}

pub(crate) fn window_handler<'gc, const T: NameId>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
) -> Result<HandlerValue<'gc>, ApiError> {
    Ok(current_value(cx, TargetRef::Window(this), T))
}

pub(crate) fn set_window_handler<'gc, O: OpFront, const T: NameId>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    value: Option<Callback<O::Callback>>,
) -> Result<(), ApiError> {
    set_handler(op, cx, TargetRef::Window(this), T, value)
}
