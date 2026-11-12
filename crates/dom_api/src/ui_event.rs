//! UI Events' `UIEvent`, `MouseEvent`, `WheelEvent`, `KeyboardEvent` and
//! `InputEvent` (https://w3c.github.io/uievents/), with CSSOM View's
//! coordinates (`pageX`, `offsetX`) and Pointer Lock's `movementX`: their
//! constructors, their legacy `init*Event`s and members (Blink's `UIEvent`,
//! `MouseEvent`, `WheelEvent`, `KeyboardEvent`, `InputEvent`). Their data
//! is `dom`'s (`UiEventData`, `MouseEventData`, `WheelEventData`,
//! `KeyboardEventData`, `InputEventData`).

use base::numerics::saturated_cast;
use dom::{
    DomString, Event, EventFlags, InputEventInfo, KeyboardEventInfo, MouseEventInfo, NodeKind,
    NodeRef, TargetRef, WheelEventInfo, Window, modifiers, names,
};
use elise::Gc;
use gfx::{ElementBox, PointF};

use crate::interfaces::{
    InputEventInit, KeyboardEventInit, MouseEventInit, UIEventInit, WheelEventInit,
};
use crate::typed::{DomStr, NullableStr, OpFront, RelatedTarget, ViewObject};
use crate::{ApiCx, ApiError};

/// `EventModifierInit`'s modifier keys of `init` (a dictionary that
/// inherits it) as `modifiers`' bits.
macro_rules! modifier_bits {
    ($init:expr) => {{
        let init = $init;
        [
            (init.ctrlKey, modifiers::CTRL),
            (init.shiftKey, modifiers::SHIFT),
            (init.altKey, modifiers::ALT),
            (init.metaKey, modifiers::META),
            (init.modifierAltGraph, modifiers::ALT_GRAPH),
            (init.modifierCapsLock, modifiers::CAPS_LOCK),
            (init.modifierFn, modifiers::FN),
            (init.modifierFnLock, modifiers::FN_LOCK),
            (init.modifierHyper, modifiers::HYPER),
            (init.modifierNumLock, modifiers::NUM_LOCK),
            (init.modifierScrollLock, modifiers::SCROLL_LOCK),
            (init.modifierSuper, modifiers::SUPER),
            (init.modifierSymbol, modifiers::SYMBOL),
            (init.modifierSymbolLock, modifiers::SYMBOL_LOCK),
        ]
        .into_iter()
        .filter(|&(down, _)| down)
        .fold(0, |bits, (_, bit)| bits | bit)
    }};
}

/// `new UIEvent(type, eventInitDict)`, its init's `view` a window or
/// null.
pub(crate) fn construct_ui_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &UIEventInit<O::Node>,
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
    cx.set_ui_event_data(event, init_view(op, cx, &init.view), init.detail);
    Ok(event)
}

/// An init dictionary's `view`: the window given, if any.
pub(crate) fn init_view<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    view: &Option<ViewObject<O::Node>>,
) -> Option<Gc<'gc, Window>> {
    view.as_ref()
        .map(|ViewObject(window)| op.object::<Window>(cx, window))
}

/// An init dictionary's `relatedTarget`: the event target given, if any.
pub(crate) fn related_target_of<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    related: &Option<RelatedTarget<O::Node>>,
) -> Option<TargetRef<'gc>> {
    related
        .as_ref()
        .map(|RelatedTarget(target)| op.target(cx, target))
}

/// `MouseEventInit`'s members of `init` (a dictionary that inherits it) as
/// what a mouse event says.
macro_rules! mouse_info {
    ($init:expr) => {{
        let init = $init;
        MouseEventInfo {
            detail: init.detail,
            client_x: init.clientX,
            client_y: init.clientY,
            screen_x: init.screenX,
            screen_y: init.screenY,
            movement_x: init.movementX,
            movement_y: init.movementY,
            button: init.button,
            buttons: init.buttons,
            modifiers: modifier_bits!(init),
        }
    }};
}

/// `new MouseEvent(type, eventInitDict)`, its init's `view` a window or
/// null and its `relatedTarget` an event target or null.
pub(crate) fn construct_mouse_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &MouseEventInit<O::Node>,
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
    let related = related_target_of(op, cx, &init.relatedTarget);
    cx.set_mouse_event_data(
        event,
        init_view(op, cx, &init.view),
        &mouse_info!(init),
        related,
    );
    Ok(event)
}

/// `new WheelEvent(type, eventInitDict)`, `view` and `relatedTarget` as
/// a mouse event's.
pub(crate) fn construct_wheel_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &WheelEventInit<O::Node>,
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
    let wheel = WheelEventInfo {
        delta_x: init.deltaX,
        delta_y: init.deltaY,
        delta_z: init.deltaZ,
        delta_mode: init.deltaMode,
    };
    let related = related_target_of(op, cx, &init.relatedTarget);
    cx.set_wheel_event_data(
        event,
        init_view(op, cx, &init.view),
        &mouse_info!(init),
        related,
        &wheel,
    );
    Ok(event)
}

/// `new InputEvent(type, eventInitDict)`, its init's `view` a window or
/// null.
pub(crate) fn construct_input_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &InputEventInit<O::Node>,
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
    let info = InputEventInfo {
        detail: init.detail,
        data: init.data.as_deref().map(DomString::new),
        input_type: DomString::new(&init.inputType),
        is_composing: init.isComposing,
    };
    cx.set_input_event_data(event, init_view(op, cx, &init.view), &info);
    Ok(event)
}

/// The four modifier keys the legacy `init*Event`s take, as `modifiers`'
/// bits: only they are down after one (Blink's `InitModifiers`).
fn legacy_modifier_bits(ctrl_key: bool, alt_key: bool, shift_key: bool, meta_key: bool) -> u16 {
    [
        (ctrl_key, modifiers::CTRL),
        (alt_key, modifiers::ALT),
        (shift_key, modifiers::SHIFT),
        (meta_key, modifiers::META),
    ]
    .into_iter()
    .filter(|&(down, _)| down)
    .fold(0, |bits, (_, bit)| bits | bit)
}

/// `initUIEvent(type, bubbles, cancelable, view, detail)`: nothing while
/// it is dispatched; else DOM's "initialize", its view and detail.
pub(crate) fn init_ui_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    view: Option<Gc<'gc, Window>>,
    detail: i32,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    cx.set_ui_event_view_and_detail(this, view, detail);
    Ok(())
}

/// WebIDL's `ToInt16` of a `long`: its low 16 bits.
fn low_i16(value: i32) -> i16 {
    let [low, high, ..] = value.to_le_bytes();
    i16::from_le_bytes([low, high])
}

/// `initMouseEvent(...)`: nothing while it is dispatched; else DOM's
/// "initialize", and what it gives (a wheel event stays one, its deltas
/// kept; its `buttons` and movement are kept too, as Blink's).
#[expect(clippy::too_many_arguments, reason = "the legacy method's")]
pub(crate) fn init_mouse_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    view: Option<Gc<'gc, Window>>,
    detail: i32,
    screen_x: i32,
    screen_y: i32,
    client_x: i32,
    client_y: i32,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
    meta_key: bool,
    button: i32,
    related_target: Option<TargetRef<'gc>>,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    let info = MouseEventInfo {
        detail,
        screen_x: screen_x.into(),
        screen_y: screen_y.into(),
        client_x: client_x.into(),
        client_y: client_y.into(),
        modifiers: legacy_modifier_bits(ctrl_key, alt_key, shift_key, meta_key),
        button: low_i16(button),
        ..info(cx, this)
    };
    cx.set_mouse_event_data(this, view, &info, related_target);
    Ok(())
}

/// `initKeyboardEvent(...)`: nothing while it is dispatched; else DOM's
/// "initialize", and what it gives, its `detail` 0 (Blink's); its code
/// and legacy codes are kept.
#[expect(clippy::too_many_arguments, reason = "the legacy method's")]
pub(crate) fn init_keyboard_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    view: Option<Gc<'gc, Window>>,
    key: &str,
    location: u32,
    ctrl_key: bool,
    alt_key: bool,
    shift_key: bool,
    meta_key: bool,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    let info = KeyboardEventInfo {
        detail: 0,
        key: DomString::new(key),
        location,
        modifiers: legacy_modifier_bits(ctrl_key, alt_key, shift_key, meta_key),
        ..keyboard(cx, this)
    };
    cx.set_keyboard_event_data(this, view, &info);
    Ok(())
}

/// `new KeyboardEvent(type, eventInitDict)`, its init's `view` a window
/// or null.
pub(crate) fn construct_keyboard_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &KeyboardEventInit<O::Node>,
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
    let info = KeyboardEventInfo {
        detail: init.detail,
        key: DomString::new(&init.key),
        code: DomString::new(&init.code),
        location: init.location,
        repeat: init.repeat,
        is_composing: init.isComposing,
        modifiers: modifier_bits!(init),
        char_code: init.charCode,
        key_code: init.keyCode,
    };
    cx.set_keyboard_event_data(event, init_view(op, cx, &init.view), &info);
    Ok(event)
}

fn keyboard<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> KeyboardEventInfo {
    cx.keyboard_event_data(this)
        .map(|data| cx.keyboard_event_info(data))
        .expect("`this` is a keyboard event")
}

pub(crate) fn key<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<DomString, ApiError> {
    Ok(keyboard(cx, this).key)
}

pub(crate) fn code<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<DomString, ApiError> {
    Ok(keyboard(cx, this).code)
}

pub(crate) fn location<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    Ok(keyboard(cx, this).location)
}

pub(crate) fn repeat<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(keyboard(cx, this).repeat)
}

pub(crate) fn is_composing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(keyboard(cx, this).is_composing)
}

/// `charCode`, legacy: the character a `keypress` types, 0 for the others.
pub(crate) fn char_code<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    Ok(keyboard(cx, this).char_code)
}

/// `keyCode`, legacy: the platform's key code (Windows'), a `keypress`'s
/// character.
pub(crate) fn key_code<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    Ok(keyboard(cx, this).key_code)
}

pub(crate) fn view<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<Gc<'gc, Window>>, ApiError> {
    Ok(cx.ui_event_view_and_detail(this).0)
}

pub(crate) fn detail<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<i32, ApiError> {
    Ok(cx.ui_event_view_and_detail(this).1)
}

/// `which`, legacy: a mouse event's `button` plus one (Blink's
/// `MouseEvent::which`: `unsigned long`, so a button of -2 or less wraps
/// round), a keyboard event's `keyCode`, 0 for another UI event.
pub(crate) fn which<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    if let Some(data) = cx.keyboard_event_data(this) {
        return Ok(cx.keyboard_event_info(data).key_code);
    }
    Ok(cx.mouse_event_data(this).map_or(0, |data| {
        i32::from(cx.mouse_event_info(data).button)
            .wrapping_add(1)
            .cast_unsigned()
    }))
}

/// A mouse or a keyboard event's modifier keys (`modifiers`' bits).
fn modifiers_of<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> u16 {
    match cx.keyboard_event_data(this) {
        Some(data) => cx.keyboard_event_info(data).modifiers,
        None => info(cx, this).modifiers,
    }
}

fn info<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> MouseEventInfo {
    cx.mouse_event_data(this)
        .map(|data| cx.mouse_event_info(data))
        .expect("`this` is a mouse event")
}

pub(crate) fn screen_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).screen_x)
}

pub(crate) fn screen_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).screen_y)
}

/// `clientX` (and `x`).
pub(crate) fn client_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).client_x)
}

pub(crate) fn client_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).client_y)
}

pub(crate) fn movement_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).movement_x)
}

pub(crate) fn movement_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).movement_y)
}

pub(crate) fn ctrl_key<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(modifiers_of(cx, this) & modifiers::CTRL != 0)
}

pub(crate) fn shift_key<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(modifiers_of(cx, this) & modifiers::SHIFT != 0)
}

pub(crate) fn alt_key<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(modifiers_of(cx, this) & modifiers::ALT != 0)
}

pub(crate) fn meta_key<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<bool, ApiError> {
    Ok(modifiers_of(cx, this) & modifiers::META != 0)
}

/// `button`: WebIDL's `short`, given as a `long`.
pub(crate) fn button<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<i32, ApiError> {
    Ok(info(cx, this).button.into())
}

/// `buttons`: WebIDL's `unsigned short`.
pub(crate) fn buttons<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u16, ApiError> {
    Ok(info(cx, this).buttons)
}

pub(crate) fn related_target<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<TargetRef<'gc>>, ApiError> {
    Ok(cx
        .mouse_event_data(this)
        .and_then(|data| cx.mouse_related_target(data))
        .map(|related| retargeted_related(cx, this, related)))
}

/// A mouse or focus event's related target as its current target sees it
/// (DOM's dispatch sets each entry's: retargeted against it, as the path
/// was made if a shadow tree is in it); as it is out of a dispatch.
pub(crate) fn retargeted_related<'gc>(
    cx: &ApiCx<'_, 'gc>,
    event: Gc<'gc, Event>,
    related: TargetRef<'gc>,
) -> TargetRef<'gc> {
    if let Some(entry) = cx.event_shadow_current(event) {
        return entry.related.unwrap_or(related);
    }
    match (related, cx.event_current_target(event)) {
        (TargetRef::Node(related), Some(TargetRef::Node(current))) => {
            TargetRef::Node(cx.retarget(related, Some(current)))
        }
        (TargetRef::Node(related), Some(TargetRef::Window(_) | TargetRef::EventTarget(_))) => {
            TargetRef::Node(cx.retarget(related, None))
        }
        (related, _) => related,
    }
}

fn wheel<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> WheelEventInfo {
    cx.wheel_event_data(this)
        .map(|data| cx.wheel_event_info(data))
        .expect("`this` is a wheel event")
}

pub(crate) fn delta_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(wheel(cx, this).delta_x)
}

pub(crate) fn delta_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(wheel(cx, this).delta_y)
}

pub(crate) fn delta_z<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(wheel(cx, this).delta_z)
}

pub(crate) fn delta_mode<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<u32, ApiError> {
    Ok(wheel(cx, this).delta_mode)
}

/// Chrome's `wheelDeltaX` (`mousewheel`'s): the other way from `deltaX`,
/// whole, as Blink's constructor makes it from the init's `deltaX`. Blink's
/// from the platform's event is 120 a wheel's notch; machopo's notch is three
/// 40 px lines, so the same.
pub(crate) fn wheel_delta_x<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<i32, ApiError> {
    Ok(saturated_cast(-wheel(cx, this).delta_x))
}

pub(crate) fn wheel_delta_y<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<i32, ApiError> {
    Ok(saturated_cast(-wheel(cx, this).delta_y))
}

/// Chrome's `wheelDelta`: `wheelDeltaY`, or `wheelDeltaX` where that is 0.
pub(crate) fn wheel_delta<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<i32, ApiError> {
    let y = wheel_delta_y(cx, this)?;
    if y != 0 {
        return Ok(y);
    }
    wheel_delta_x(cx, this)
}

fn input<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> InputEventInfo {
    cx.input_event_data(this)
        .map(|data| cx.input_event_info(data))
        .expect("`this` is an input event")
}

pub(crate) fn input_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<NullableStr<'gc>, ApiError> {
    Ok(NullableStr(input(cx, this).data.map(DomStr::Owned)))
}

pub(crate) fn input_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(input(cx, this).input_type))
}

pub(crate) fn input_is_composing<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(input(cx, this).is_composing)
}

/// UI Events' modifier key names, `getModifierState`'s (Blink's
/// `kModifierKeys`), and their bits.
const MODIFIER_NAMES: [(&str, u16); 14] = [
    ("Control", modifiers::CTRL),
    ("Shift", modifiers::SHIFT),
    ("Alt", modifiers::ALT),
    ("Meta", modifiers::META),
    ("AltGraph", modifiers::ALT_GRAPH),
    ("CapsLock", modifiers::CAPS_LOCK),
    ("Fn", modifiers::FN),
    ("FnLock", modifiers::FN_LOCK),
    ("Hyper", modifiers::HYPER),
    ("NumLock", modifiers::NUM_LOCK),
    ("ScrollLock", modifiers::SCROLL_LOCK),
    ("Super", modifiers::SUPER),
    ("Symbol", modifiers::SYMBOL),
    ("SymbolLock", modifiers::SYMBOL_LOCK),
];

/// `getModifierState(keyArg)`: whether the key of that name (case
/// sensitive) was down; false for a name that isn't a modifier's.
pub(crate) fn get_modifier_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    key: &str,
) -> Result<bool, ApiError> {
    let down = modifiers_of(cx, this);
    Ok(MODIFIER_NAMES
        .iter()
        .any(|&(name, bit)| name == key && down & bit != 0))
}

/// How far the event's view is scrolled, as last laid out: no layout
/// first, as Blink's `MouseEvent::InitCoordinates` reads the layout
/// viewport's offset (a page reading every member of each mouse event, as
/// React does, would lay out each time).
fn view_scroll<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> PointF {
    let Some(view) = cx.ui_event_view_and_detail(this).0 else {
        return PointF::new(0.0, 0.0);
    };
    cx.peek_layout(cx.window_document(view), |layout| layout.viewport().scroll)
        .unwrap_or(PointF::new(0.0, 0.0))
}

/// `pageX`: `clientX` from the document's origin.
pub(crate) fn page_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).client_x + f64::from(view_scroll(cx, this).x))
}

pub(crate) fn page_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(info(cx, this).client_y + f64::from(view_scroll(cx, this).y))
}

/// Where the padding edge of the event's target starts in its viewport,
/// laid out first, if its target is an element with a box; an inline
/// element's is its first box's border edge (it has no border widths of
/// its own to read).
fn padding_origin<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Option<PointF> {
    let Some(TargetRef::Node(target)) = cx.event_target(this) else {
        return None;
    };
    if cx.kind(target) != NodeKind::Element {
        return None;
    }
    cx.with_layout(target, |layout| {
        let ElementBox {
            border_box, block, ..
        } = layout.element_box(cx.node_id(target))?;
        let scroll = layout.viewport().scroll;
        let border = block.map_or([0.0; 4], |block| block.border);
        Some(PointF::new(
            border_box.x + border[3] - scroll.x,
            border_box.y + border[0] - scroll.y,
        ))
    })
    .flatten()
}

/// `offsetX`: `clientX` from the target's padding edge; `pageX` for a
/// target that has none.
pub(crate) fn offset_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    match padding_origin(cx, this) {
        Some(origin) => Ok(info(cx, this).client_x - f64::from(origin.x)),
        None => page_x(cx, this),
    }
}

pub(crate) fn offset_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    match padding_origin(cx, this) {
        Some(origin) => Ok(info(cx, this).client_y - f64::from(origin.y)),
        None => page_y(cx, this),
    }
}

/// `fromElement`, which no spec has (Blink's `MouseEvent::fromElement`):
/// the node the pointer leaves, a `mouseout`'s or `mouseleave`'s target and
/// any other event's `relatedTarget`; `null` if that isn't a node.
pub(crate) fn from_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(legacy_element(cx, this, !leaves(cx, this)))
}

/// `toElement`, Blink's `MouseEvent::toElement`: the node the pointer
/// enters, the other one of `fromElement`'s two.
pub(crate) fn to_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<Option<NodeRef<'gc>>, ApiError> {
    Ok(legacy_element(cx, this, leaves(cx, this)))
}

/// Whether the event is the pointer leaving its target, which Blink's two
/// legacy members tell from its type alone.
fn leaves<'gc>(cx: &ApiCx<'_, 'gc>, event: Gc<'gc, Event>) -> bool {
    matches!(
        cx.event_type(event),
        names::event::MOUSEOUT | names::event::MOUSELEAVE
    )
}

/// The event's `relatedTarget` (as `relatedTarget` reads it) if `related`,
/// else its target, if a node.
fn legacy_element<'gc>(
    cx: &ApiCx<'_, 'gc>,
    event: Gc<'gc, Event>,
    related: bool,
) -> Option<NodeRef<'gc>> {
    let target = if related {
        related_target(cx, event).ok().flatten()
    } else {
        cx.event_target(event)
    };
    match target? {
        TargetRef::Node(node) => Some(node),
        TargetRef::Window(_) | TargetRef::EventTarget(_) => None,
    }
}

/// `layerX`, which no spec has (Blink's `MouseEvent::layerX`, which it
/// calls poorly defined): from the origin of the border box of the box
/// its target is in that is a layer, read now, as `offsetX` is; `pageX`
/// if that is the root's, the document's (or it has no target with a
/// box). A WebIDL `long`: floored, and clamped to one.
pub(crate) fn layer_x<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<i32, ApiError> {
    let x = match layer_origin(cx, this) {
        Some(origin) => info(cx, this).client_x - f64::from(origin.x),
        None => page_x(cx, this)?,
    };
    Ok(to_long(x))
}

pub(crate) fn layer_y<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<i32, ApiError> {
    let y = match layer_origin(cx, this) {
        Some(origin) => info(cx, this).client_y - f64::from(origin.y),
        None => page_y(cx, this)?,
    };
    Ok(to_long(y))
}

/// A layer coordinate as a `long`: floored, after snapping to Blink's
/// layout unit (1/64 px). machopo's layout is in `f32`, so a box at 0.6px
/// whose child is at 5.6px puts a point 10px into the child at
/// 14.9999998px from the box, where Blink's units give 15.
fn to_long(coordinate: f64) -> i32 {
    saturated_cast((coordinate * 64.0).round().div_euclid(64.0))
}

/// Where the layer the event's target is in starts in its viewport, laid
/// out first (Blink's `MouseEvent::ComputeRelativePosition`'s layer): from
/// the nearest element (a text node's parent), the nearest positioned
/// element or stacking context with a box (not a `display: none` or
/// `display: contents` one), its bounding client rect's origin; none past
/// the root element (the root's layer, page coordinates).
fn layer_origin<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Option<PointF> {
    let Some(TargetRef::Node(target)) = cx.event_target(this) else {
        return None;
    };
    cx.with_layout(target, |layout| {
        let mut node = target;
        while cx.kind(node) != NodeKind::Element {
            node = cx.flat_tree_parent(node)?;
        }
        loop {
            // An element without a box has none to be a layer. The root
            // element is one too if it is positioned or a stacking
            // context.
            if layout
                .computed_style(cx, node)
                .is_some_and(|style| style.is_positioned() || style.is_stacking_context())
                && layout.element_box(cx.node_id(node)).is_some()
            {
                let rect = crate::geometry::bounding_rect(&crate::geometry::client_rects_in(
                    cx, layout, node,
                ));
                return Some(PointF::new(rect.x, rect.y));
            }
            node = cx
                .flat_tree_parent(node)
                .filter(|&parent| cx.kind(parent) == NodeKind::Element)?;
        }
    })
    .flatten()
}

/// A `TextEvent`'s `data`.
pub(crate) fn text_data<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<DomStr<'gc>, ApiError> {
    let data = cx.text_event_data(this).expect("`this` is a text event");
    Ok(DomStr::Owned(cx.text_event_text(data)))
}

/// `initTextEvent(...)`: nothing while it is dispatched; else DOM's
/// "initialize", its view and data (`"undefined"` when missing, Blink's
/// default).
pub(crate) fn init_text_event<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
    event_type: &str,
    bubbles: bool,
    cancelable: bool,
    view: Option<Gc<'gc, Window>>,
    data: Option<&str>,
) -> Result<(), ApiError> {
    if cx.event_state(this).dispatching {
        return Ok(());
    }
    cx.init_event(this, names::intern(event_type), bubbles, cancelable);
    cx.set_ui_event_view_and_detail(this, view, 0);
    let text = cx.text_event_data(this).expect("`this` is a text event");
    cx.set_text_event_text(text, data.unwrap_or("undefined"));
    Ok(())
}
