//! What elements do with a click dispatched at or through them: the DOM's
//! activation behavior (https://dom.spec.whatwg.org/#eventtarget-activation-behavior)
//! with its legacy-pre-activation and legacy-canceled-activation behaviors,
//! which the dispatch runs (`event_target::dispatch`), and `click()`.
//!
//! A checkbox's and a radio button's (Blink's `HTMLInputElement::
//! PreDispatchEventHandler` / `PostDispatchEventHandler`, its input
//! type's `WillDispatchClick` and `DidDispatchClick`), and a label's: a
//! click on it, not on its control or other interactive content in it,
//! focuses its control and clicks it (HTML leaves it to the platform;
//! Blink's `HTMLLabelElement::DefaultEventHandler`). A button that submits
//! or resets its form owner has one: a reset button's resets it, a submit
//! button's does nothing yet. The other inputs and buttons have none: a
//! click goes on to an ancestor's.

use std::cell::Cell;

use dom::names::{self, event, local};
use dom::{
    ClickState, Event, EventFlags, InputKind, MouseEventInfo, NodeId, NodeRef, NodeRoot,
    QualifiedName, TargetRef, TargetRoot,
};
use elise::{Gc, Persistent};

use crate::typed::ScriptHost;
use crate::{ApiCx, ApiError, dispatch, focus};

thread_local! {
    /// The elements a synthetic click is being dispatched at (HTML's click
    /// in progress flag; Blink's `nodes_dispatching_simulated_clicks`): one
    /// isn't clicked again meanwhile. Few, and by id: nothing for the heap.
    /// In a `Cell`, taken and put back.
    static CLICKING: Cell<Vec<NodeId>> = const { Cell::new(Vec::new()) };
}

/// What has an activation behavior, and what its legacy-pre-activation
/// behavior kept.
pub(crate) enum Activation {
    /// A checkbox or a radio button, the kind it was clicked as, and what
    /// it was (its group's checked one rooted: script runs meanwhile).
    Input {
        kind: InputKind,
        checked: bool,
        indeterminate: bool,
        checked_radio: Option<NodeRoot>,
    },
    /// A label, and the node the event was dispatched at.
    Label { target: NodeRoot },
    /// A button that submits or resets its form owner.
    Button,
    /// An `<a>` or `<area>` with an `href`.
    Hyperlink,
}

/// The DOM's "isActivationEvent": a `MouseEvent` whose type is `click`.
pub(crate) fn is_activation_event<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> bool {
    cx.event_type(event) == event::CLICK && cx.mouse_event_data(event).is_some()
}

/// Whether `target` has an activation behavior.
pub(crate) fn has_activation_behavior<'gc>(cx: &ApiCx<'_, 'gc>, target: TargetRef<'gc>) -> bool {
    let TargetRef::Node(node) = target else {
        return false;
    };
    cx.input_has_activation_behavior(node)
        || cx.button_has_activation_behavior(node)
        || cx.qualified_name(node) == Some(QualifiedName::html(local::LABEL))
        || crate::hyperlink::is_hyperlink(cx, node)
}

/// A click's activation target (DOM's): `path`'s target if it has an
/// activation behavior, else, if the event bubbles, the first of the path
/// that has one; with the target. Out of the dispatch's way: few events
/// are clicks.
#[cold]
#[inline(never)]
pub(crate) fn activation_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    path: &[TargetRef<'gc>],
    bubbles: bool,
) -> Option<Box<(NodeRoot, NodeRoot)>> {
    let TargetRef::Node(target) = *path.first()? else {
        return None;
    };
    let found = path
        .iter()
        .take(if bubbles { path.len() } else { 1 })
        .find(|at| has_activation_behavior(cx, **at))?;
    let TargetRef::Node(element) = *found else {
        return None;
    };
    Some(Box::new((cx.root(element), cx.root(target))))
}

/// An activation target and what its legacy-pre-activation behavior kept.
pub(crate) struct Pending {
    element: NodeRoot,
    kept: Activation,
}

/// `activation_target`'s legacy-pre-activation behavior.
#[cold]
#[inline(never)]
#[expect(
    clippy::boxed_local,
    reason = "boxed by the dispatch, whose frame (on every event's path) stays small without it"
)]
pub(crate) fn pre_activation<H: ScriptHost>(
    host: &mut H,
    target: Box<(NodeRoot, NodeRoot)>,
) -> Box<Pending> {
    let (element, target) = *target;
    let kept = legacy_pre_activation(host, &element, &target);
    Box::new(Pending { element, kept })
}

/// After the dispatch of a click: the activation behavior if no listener
/// canceled it, else the legacy-canceled-activation behavior.
#[cold]
#[inline(never)]
#[expect(
    clippy::boxed_local,
    reason = "boxed by the dispatch, whose frame (on every event's path) stays small without it"
)]
pub(crate) fn post_activation<H: ScriptHost>(
    host: &mut H,
    pending: Box<Pending>,
    event: &Persistent<Event>,
    not_canceled: bool,
) {
    let Pending { element, kept } = *pending;
    if not_canceled {
        activation_behavior(host, &element, event, kept);
    } else {
        legacy_canceled_activation(host, &element, kept);
    }
}

/// The DOM's legacy-pre-activation behavior of `element`, before any
/// listener sees the click at `target`: what it kept.
fn legacy_pre_activation<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    target: &NodeRoot,
) -> Activation {
    host.mutate(|cx| {
        let node = element.get(cx.gc);
        if cx.qualified_name(node) == Some(QualifiedName::html(local::LABEL)) {
            return Activation::Label {
                target: target.clone(),
            };
        }
        if crate::hyperlink::is_hyperlink(cx, node) {
            return Activation::Hyperlink;
        }
        match (cx.input_kind(node), cx.input_will_click(node)) {
            (Some(kind), Some(state)) => Activation::Input {
                kind,
                checked: state.checked,
                indeterminate: state.indeterminate,
                checked_radio: state.checked_radio.map(|radio| cx.root(radio)),
            },
            _ => Activation::Button,
        }
    })
}

/// The DOM's legacy-canceled-activation behavior of `element`: a click
/// canceled.
fn legacy_canceled_activation<H: ScriptHost>(host: &mut H, element: &NodeRoot, kept: Activation) {
    if let Activation::Input {
        kind,
        checked,
        indeterminate,
        checked_radio,
    } = kept
    {
        host.mutate(|cx| {
            let state = ClickState {
                checked,
                indeterminate,
                checked_radio: checked_radio.map(|radio| radio.get(cx.gc)),
            };
            cx.input_click_canceled(element.get(cx.gc), state, kind);
        });
    }
}

/// The DOM's activation behavior of `element`, for a click `event` not
/// canceled.
fn activation_behavior<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    event: &Persistent<Event>,
    kept: Activation,
) {
    match kept {
        // HTML's input activation behavior of a checkbox and a radio
        // button: if it is connected, `input` and then `change`; not for a
        // radio button checked before, which didn't change (as in Blink).
        Activation::Input { kind, checked, .. } => {
            if (kind == InputKind::Radio && checked)
                || !host.read(|cx| cx.is_connected(element.get(cx.gc)))
            {
                return;
            }
            let target = TargetRoot::Node(element.clone());
            fire_input_event(host, &target, event::INPUT, true);
            fire_input_event(host, &target, event::CHANGE, false);
        }
        Activation::Label { target } => label_activation(host, element, &target, event),
        Activation::Hyperlink => crate::hyperlink::follow_hyperlink(host, element, event),
        // A reset button resets its form owner, if its document is fully
        // active (has a window); submitting comes later.
        Activation::Button => {
            let form = host.mutate(|cx| {
                let node = element.get(cx.gc);
                cx.form_owner(node)
                    .filter(|_| {
                        cx.resets_form(node) && cx.window_of(cx.document_of(node)).is_some()
                    })
                    .map(|form| cx.root(form))
            });
            if let Some(form) = form {
                crate::forms::reset_form(host, &form);
            }
        }
    }
}

/// `input` (composed) or `change`, trusted, bubbling, not cancelable, at
/// `target`.
fn fire_input_event<H: ScriptHost>(
    host: &mut H,
    target: &TargetRoot,
    event_type: dom::NameId,
    composed: bool,
) {
    let event = host.mutate(|cx| {
        let window = cx.target_window(target.get(cx.gc));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles: true,
                cancelable: false,
                composed,
                trusted: true,
            },
            window,
        );
        Persistent::new(cx.gc, event)
    });
    dispatch(host, target, &event);
}

/// HTML's interactive content (Blink's `IsInteractiveContent`): a click on
/// it in a label is its own, not the label's control's. `<audio>` and
/// `<video>` with controls, `<img usemap>` aren't yet.
fn is_interactive<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    let Some(name) = cx.qualified_name(node) else {
        return false;
    };
    if name.namespace() != names::HTML_NAMESPACE {
        return false;
    }
    match name.local() {
        local::A => cx.attribute_value_by_id(node, local::HREF).is_some(),
        local::INPUT => cx.input_kind(node) != Some(InputKind::Hidden),
        local::BUTTON
        | local::DETAILS
        | local::EMBED
        | local::IFRAME
        | local::LABEL
        | local::SELECT
        | local::TEXTAREA => true,
        _ => false,
    }
}

/// A label's activation: if the click wasn't on its labeled control, nor
/// on interactive content in the label, and the control isn't disabled,
/// the control takes the focus if it can and is clicked (a synthetic
/// click, trusted if this one was, with its modifier keys).
fn label_activation<H: ScriptHost>(
    host: &mut H,
    label: &NodeRoot,
    target: &NodeRoot,
    event: &Persistent<Event>,
) {
    let found = host.read(|cx| {
        let label = label.get(cx.gc);
        let control = cx.label_control(label)?;
        let mut at = Some(target.get(cx.gc));
        let mut in_label = true;
        while let Some(node) = at {
            if Gc::ptr_eq(node, control) {
                return None;
            }
            if Gc::ptr_eq(node, label) {
                in_label = false;
            }
            if in_label && is_interactive(cx, node) {
                return None;
            }
            at = cx.parent(node);
        }
        if cx.is_disabled_form_control(control) {
            return None;
        }
        let event = event.get(cx.gc);
        let modifiers = cx
            .mouse_event_data(event)
            .map_or(0, |data| cx.mouse_event_info(data).modifiers);
        Some((cx.root(control), cx.event_flags(event).trusted, modifiers))
    });
    let Some((control, trusted, modifiers)) = found else {
        return;
    };
    let document = host.mutate(|cx| cx.root(cx.document_of(control.get(cx.gc))));
    if host.mutate(|cx| focus::is_focusable_area(cx, control.get(cx.gc))) {
        focus::focusing_steps(host, &document, Some(&control), None);
    }
    simulated_click(host, &control, trusted, modifiers);
}

/// Whether `node` or an ancestor is a disabled form control: the user's
/// click on it isn't dispatched (HTML's "a form control that is disabled
/// must prevent any click events that are queued on the user interaction
/// task source"; a disabled button's text is the button's).
pub(crate) fn in_disabled_control<'gc>(cx: &ApiCx<'_, 'gc>, node: NodeRef<'gc>) -> bool {
    let mut at = Some(node);
    while let Some(node) = at {
        if cx.is_disabled_form_control(node) {
            return true;
        }
        at = cx.parent(node);
    }
    false
}

/// HTML's `click()`: a disabled form control does nothing; else a
/// synthetic click, not trusted.
pub(crate) fn click<H: ScriptHost>(host: &mut H, this: NodeRoot) -> Result<(), ApiError> {
    if host.read(|cx| cx.is_disabled_form_control(this.get(cx.gc))) {
        return Ok(());
    }
    simulated_click(host, &this, false, 0);
    Ok(())
}

/// HTML's "fire a synthetic pointer event" named `click` at `element`
/// (Blink's `DispatchSimulatedClick`; a `MouseEvent`, there being no
/// `PointerEvent` yet): bubbling, cancelable, composed, its view the
/// window, `detail` 1 as Blink's; nothing if one is being dispatched at
/// it already.
pub(crate) fn simulated_click<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    trusted: bool,
    modifiers: u16,
) {
    let id = host.read(|cx| cx.node_id(element.get(cx.gc)));
    let mut clicking = CLICKING.take();
    let started = !clicking.contains(&id);
    if started {
        clicking.push(id);
    }
    CLICKING.set(clicking);
    if !started {
        return;
    }
    let event = host.mutate(|cx| {
        let node = element.get(cx.gc);
        let window = cx.target_window(TargetRef::Node(node));
        let event = cx.create_event(
            event::CLICK,
            EventFlags {
                bubbles: true,
                cancelable: true,
                composed: true,
                trusted,
            },
            window,
        );
        let info = MouseEventInfo {
            detail: 1,
            modifiers,
            ..MouseEventInfo::default()
        };
        cx.set_mouse_event_data(event, window, &info, None);
        Persistent::new(cx.gc, event)
    });
    dispatch(host, &TargetRoot::Node(element.clone()), &event);
    let mut clicking = CLICKING.take();
    clicking.retain(|clicking| *clicking != id);
    CLICKING.set(clicking);
}
