//! The form controls' own default actions besides typing (`editing`): a
//! number's and a range's keys (Blink's `TextFieldInputType::
//! HandleKeydownEventForSpinButton`, `RangeInputType::HandleKeydownEvent`),
//! a search field's Escape and its clear button (`SearchInputType`,
//! `SearchFieldCancelButtonElement`), a number's spin button
//! (`SpinButtonElement`), and the keys that click a check box, a radio
//! button or a button (Blink's `HTMLInputElement` and `HTMLButtonElement`
//! default event handlers: Space, and a button's Enter). They are the
//! user's input's (`EventHandler`'s), done once its event wasn't canceled:
//! script's untrusted events do none of it, nor can script reach the UA
//! shadow trees' parts.
//!
//! A spin button's press steps the field by the half it went down in, then
//! again after 250 ms and every 50 ms while it is held and the pointer is
//! in that half (Windows' and Linux's; a Mac's steps by the half the
//! pointer is in now): the repeat is a native timer of the field's window
//! (`dom`'s `add_native_timer`), the press its `SpinState`.

use dom::names::{event, local};
use dom::{
    EventFlags, InputKind, NativeAction, NodeRef, NodeRoot, RangeKey, SpinState, TargetRef,
    TargetRoot,
};
use elise::Persistent;
use gfx::PointF;

use crate::typed::ScriptHost;
use crate::{ApiCx, dispatch};

/// How long a spin button is held before it steps again, and how often
/// after that (Blink's `ScrollbarTheme::InitialAutoscrollTimerDelay` and
/// `AutoscrollTimerDelay`).
const SPIN_FIRST_REPEAT_MS: f64 = 250.0;
const SPIN_REPEAT_MS: f64 = 50.0;

/// Whether the field's parts and keys do anything (Blink's
/// `IsDisabledOrReadOnly`).
fn mutable<'gc>(cx: &ApiCx<'_, 'gc>, field: NodeRef<'gc>) -> bool {
    crate::editing::is_mutable(cx, field)
}

fn is_focused<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    cx.focused_element(cx.document_of(element))
        .is_some_and(|focused| elise::Gc::ptr_eq(focused, element))
}

/// A trusted plain event of `event_type` at `element`, bubbling, not
/// cancelable, composed if `composed`.
fn fire<H: ScriptHost>(host: &mut H, element: &NodeRoot, event_type: dom::NameId, composed: bool) {
    let event = host.mutate(|cx| {
        let window = cx.target_window(TargetRef::Node(element.get(cx.gc)));
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
    dispatch(host, &TargetRoot::Node(element.clone()), &event);
}

/// The user's change of `field`'s value to `value` (Blink's
/// `SetValueForUser` with `kDispatchChangeEvent`): set as script sets it;
/// if that changed it, `input` while it has the focus (its `change` waits
/// for the commit, the value before kept to compare with), or `change` at
/// once.
fn set_value_for_user<H: ScriptHost>(host: &mut H, field: &NodeRoot, value: &str) {
    let focused = host.mutate(|cx| {
        let node = field.get(cx.gc);
        let before = cx.input_value(node);
        let _ = cx.set_input_value(node, value);
        if *cx.input_value(node).to_str() == *before.to_str() {
            return None;
        }
        let focused = is_focused(cx, node);
        if focused {
            cx.note_edit(node, before);
        }
        Some(focused)
    });
    match focused {
        Some(true) => crate::editing::fire_plain_input(host, field),
        Some(false) => crate::editing::fire_change(host, field),
        None => {}
    }
}

/// Empties a search field for the user and fires `search` (Blink's
/// `SetValueForUser("")` and `OnSearch`).
fn clear_and_search<H: ScriptHost>(host: &mut H, field: &NodeRoot) {
    set_value_for_user(host, field, "");
    fire(host, field, event::SEARCH, false);
}

/// What `key` does on its `keydown` (nobody canceled it) in the focused
/// `element`, if it is a form control that takes it: a number's ↑ and ↓
/// step it unless it is read-only, then commit its change; a range's ↑
/// and → go a step up, ↓ and ← down, PageUp and PageDown a tenth of the
/// range, Home and End to the ends, `input` and `change` if the value
/// changed; a search field's Escape, while it has a value, empties it and
/// fires `search`. Whether it took the key.
pub(crate) fn keydown<H: ScriptHost>(host: &mut H, element: &NodeRoot, key: &str) -> bool {
    let (kind, mutable, disabled) = host.read(|cx| {
        let node = element.get(cx.gc);
        (
            cx.input_kind(node),
            mutable(cx, node),
            cx.is_disabled_form_control(node),
        )
    });
    match kind {
        Some(InputKind::Number) if mutable => {
            let n = match key {
                "ArrowUp" => 1,
                "ArrowDown" => -1,
                _ => return false,
            };
            step_for_user(host, element, n);
            crate::editing::commit(host, element);
            true
        }
        Some(InputKind::Range) if !disabled => {
            let key = match key {
                "ArrowUp" | "ArrowRight" => RangeKey::Up,
                "ArrowDown" | "ArrowLeft" => RangeKey::Down,
                "PageUp" => RangeKey::PageUp,
                "PageDown" => RangeKey::PageDown,
                "Home" => RangeKey::Home,
                "End" => RangeKey::End,
                _ => return false,
            };
            let changed = host.mutate(|cx| {
                let node = element.get(cx.gc);
                let value = cx.range_key_value(node, key);
                if let Some(value) = &value {
                    let _ = cx.set_input_value(node, value);
                }
                value.is_some()
            });
            if changed {
                crate::editing::fire_plain_input(host, element);
                crate::editing::fire_change(host, element);
            }
            true
        }
        Some(InputKind::Search) if mutable && key == "Escape" => {
            let empty = host.read(|cx| cx.input_value(element.get(cx.gc)).is_empty());
            if empty {
                return false;
            }
            clear_and_search(host, element);
            true
        }
        _ => false,
    }
}

/// The user's step of a number, `n` steps up or down, set for the user.
fn step_for_user<H: ScriptHost>(host: &mut H, field: &NodeRoot, n: i32) {
    if let Some(value) = host.read(|cx| cx.number_step_for_user(field.get(cx.gc), n)) {
        set_value_for_user(host, field, &value);
    }
}

/// Whether a key clicks `element`: Space a check box, a radio button and a
/// button (an `<input>` of the button types, a `<button>`), Enter a button
/// (Blink's `BaseCheckableInputType`, `BaseButtonInputType` and
/// `HTMLButtonElement` key handling; a check box's Enter is implicit
/// submission, which there isn't yet).
fn clicked_by<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>, enter: bool) -> bool {
    let button = cx.qualified_name(element) == Some(dom::QualifiedName::html(local::BUTTON))
        || matches!(
            cx.input_kind(element),
            Some(InputKind::Submit | InputKind::Reset | InputKind::Button | InputKind::Image)
        );
    let checkable = matches!(
        cx.input_kind(element),
        Some(InputKind::Checkbox | InputKind::Radio)
    );
    button || (checkable && !enter)
}

/// What a key at `target` clicks (`clicked_by`): `target` or the nearest
/// element around it in the flat tree that the key clicks, as Blink's
/// `HTMLButtonElement::DefaultEventHandler` takes a key bubbling up from
/// an element in the button; none if that is a disabled form control.
pub(crate) fn key_clicked<'gc>(
    cx: &ApiCx<'_, 'gc>,
    target: NodeRef<'gc>,
    enter: bool,
) -> Option<NodeRef<'gc>> {
    std::iter::successors(Some(target), |&node| cx.flat_tree_parent(node))
        .find(|&node| clicked_by(cx, node, enter))
        .filter(|&clicked| !cx.is_disabled_form_control(clicked))
}

/// The key's simulated click at `element` (Blink's `DispatchSimulatedClick`
/// with the key's event: trusted).
pub(crate) fn key_click<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    crate::activation::simulated_click(host, element, true, 0);
}

/// A press of the primary button on `target` (its `mousedown` not
/// canceled), at `point`: on a number's spin button, the press begins
/// (Blink's `SpinButtonElement::DefaultEventHandler`): the field steps by
/// the half it went down in and repeats (a native timer). The field, if it
/// did, for the press's moves and release.
pub(crate) fn mouse_down<H: ScriptHost>(
    host: &mut H,
    target: &NodeRoot,
    point: PointF,
) -> Option<(NodeRoot, NodeRoot)> {
    let (field, up) = host.mutate(|cx| {
        let part = target.get(cx.gc);
        let field = cx
            .spin_button_field(part)
            .filter(|field| mutable(cx, *field))?;
        let up = upper_half(cx, part, point);
        let mut state = cx.spin_state(field);
        state.press = state.press.wrapping_add(1);
        state.pressed_up = Some(up);
        state.up = Some(up);
        cx.set_spin_state(field, state);
        cx.add_native_timer(
            field,
            NativeAction::SpinRepeat,
            state.press,
            SPIN_FIRST_REPEAT_MS,
            None,
        );
        Some((cx.root(field), up))
    })?;
    step_for_user(host, &field, if up { 1 } else { -1 });
    Some((field, target.clone()))
}

/// Whether `point` is in `part`'s upper half (Blink's
/// `CalculateUpDownStateByMouseLocation`, horizontal writing modes).
fn upper_half<'gc>(cx: &ApiCx<'_, 'gc>, part: NodeRef<'gc>, point: PointF) -> bool {
    let rect = crate::geometry::bounding_client_rect(cx, part);
    point.y < rect.y + rect.height / 2.0
}

/// The pointer moved to `point` while `field`'s spin button `part` is
/// pressed: the half it is in now.
pub(crate) fn spin_moved<H: ScriptHost>(
    host: &mut H,
    field: &NodeRoot,
    part: &NodeRoot,
    point: PointF,
) {
    host.mutate(|cx| {
        let field = field.get(cx.gc);
        let mut state = cx.spin_state(field);
        if state.pressed_up.is_some() {
            state.up = Some(upper_half(cx, part.get(cx.gc), point));
            cx.set_spin_state(field, state);
        }
    });
}

/// The primary button went up while `field`'s spin button was pressed:
/// the press ends, and the change is committed (`change` if the value
/// differs from before the user's first change).
pub(crate) fn spin_released<H: ScriptHost>(host: &mut H, field: &NodeRoot) {
    release(host, field);
    crate::editing::commit(host, field);
}

/// Ends `field`'s press, if it is pressed (Blink's `ReleaseCapture`): no
/// more steps.
fn release<H: ScriptHost>(host: &mut H, field: &NodeRoot) {
    host.mutate(|cx| {
        let field = field.get(cx.gc);
        let state = cx.spin_state(field);
        if state.pressed_up.is_some() {
            cx.set_spin_state(
                field,
                SpinState {
                    press: state.press.wrapping_add(1),
                    pressed_up: None,
                    up: None,
                },
            );
        }
    });
}

/// `element` loses the focus: a number's press ends, its change left to
/// the commit the focus moving away makes.
pub(crate) fn did_blur<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    if host.read(|cx| cx.input_kind(element.get(cx.gc)) == Some(InputKind::Number)) {
        release(host, element);
    }
}

/// A `click` (the primary button's, not canceled) on `target`: a search
/// field's clear button empties the field and fires `search` (Blink's
/// `SearchFieldCancelButtonElement`, which looks at `click`).
pub(crate) fn click<H: ScriptHost>(host: &mut H, target: &NodeRoot) {
    let field = host.read(|cx| {
        cx.cancel_button_field(target.get(cx.gc))
            .filter(|field| mutable(cx, *field))
            .map(|field| cx.root(field))
    });
    if let Some(field) = field {
        clear_and_search(host, &field);
    }
}

/// A native timer of `element`'s is due (`dom`'s `add_native_timer`),
/// run by the host's clock at `now`: its action.
pub fn run_native_timer<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    action: NativeAction,
    data: u64,
    now: f64,
) {
    match action {
        NativeAction::SpinRepeat => repeat(host, element, data, now),
        // An abort signal's, which has no element (`timer::run_next_due`).
        NativeAction::AbortTimeout => {}
    }
}

/// The repeated step of `field`'s press `press`, if it is still being
/// pressed: a step by the half the press began in, if the pointer is in it
/// (in either on a Mac); then the next in 50 ms. A press of a field that
/// left its document, or that became disabled or read-only, ends.
fn repeat<H: ScriptHost>(host: &mut H, field: &NodeRoot, press: u64, now: f64) {
    let (state, live) = host.read(|cx| {
        let node = field.get(cx.gc);
        (
            cx.spin_state(node),
            cx.is_connected(node) && mutable(cx, node),
        )
    });
    let Some(pressed_up) = state.pressed_up.filter(|_| state.press == press) else {
        return;
    };
    if !live {
        release(host, field);
        return;
    }
    let up = if cfg!(target_os = "macos") {
        state.up
    } else {
        state.up.filter(|up| *up == pressed_up)
    };
    if let Some(up) = up {
        step_for_user(host, field, if up { 1 } else { -1 });
    }
    host.mutate(|cx| {
        let node = field.get(cx.gc);
        if cx.spin_state(node).press == press {
            cx.add_native_timer(
                node,
                NativeAction::SpinRepeat,
                press,
                SPIN_REPEAT_MS,
                Some(now),
            );
        }
    });
}
