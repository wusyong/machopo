//! UI Events' events' data (Blink's `UIEvent`, `MouseEvent`, `WheelEvent`,
//! `KeyboardEvent`, `InputEvent` and `TextEvent`): a `UIEvent`'s view and
//! detail; a `MouseEvent`'s besides, its coordinates, buttons, modifier
//! keys and related target; a `WheelEvent`'s, its mouse event's and its
//! deltas; a `KeyboardEvent`'s, its key, code, location, modifier keys and
//! legacy codes; an `InputEvent`'s, what it inserts and how; a
//! `TextEvent`'s, the text (the legacy `textInput`'s). An event's subclass
//! data (`Event::data`): plain events stay as small as they are.

use std::cell::Cell;

use elise::{Erased, Gc, GcObject, LocalExternal, Member};

use crate::event::Event;
use crate::{Cx, DomString, TargetRef, Window, sites};

/// The modifier keys an event says were down (UI Events' `ctrlKey` and the
/// others, `getModifierState`'s names), a bit each.
pub mod modifiers {
    pub const CTRL: u16 = 1 << 0;
    pub const SHIFT: u16 = 1 << 1;
    pub const ALT: u16 = 1 << 2;
    pub const META: u16 = 1 << 3;
    pub const ALT_GRAPH: u16 = 1 << 4;
    pub const CAPS_LOCK: u16 = 1 << 5;
    pub const FN: u16 = 1 << 6;
    pub const FN_LOCK: u16 = 1 << 7;
    pub const HYPER: u16 = 1 << 8;
    pub const NUM_LOCK: u16 = 1 << 9;
    pub const SCROLL_LOCK: u16 = 1 << 10;
    pub const SUPER: u16 = 1 << 11;
    pub const SYMBOL: u16 = 1 << 12;
    pub const SYMBOL_LOCK: u16 = 1 << 13;
}

/// A `UIEvent`'s data that isn't a mouse event's.
#[derive(GcObject)]
pub struct UiEventData {
    view: Member<Window>,
    detail: Cell<i32>,
}

/// A `MouseEvent`'s data, its `UIEvent`'s too.
#[derive(GcObject)]
pub struct MouseEventData {
    view: Member<Window>,
    detail: Cell<i32>,
    /// `clientX`, `clientY`, `screenX`, `screenY`, `movementX`,
    /// `movementY`, CSS px.
    coordinates: Cell<[f64; 6]>,
    button: Cell<i16>,
    buttons: Cell<u16>,
    /// `modifiers`' bits.
    modifiers: Cell<u16>,
    /// A node or another target (`TargetRef`), as an event's target is.
    related_target: Member<Erased>,
}

/// A `WheelEvent`'s data: its `MouseEvent`'s, an object of its own that a
/// mouse event's members read as any mouse event's (the GC's objects have
/// no inheritance), and how far the wheel turned.
#[derive(GcObject)]
pub struct WheelEventData {
    mouse: Member<MouseEventData>,
    /// `deltaX`, `deltaY`, `deltaZ`.
    deltas: Cell<[f64; 3]>,
    delta_mode: Cell<u32>,
}

/// An `InputEvent`'s data, its `UIEvent`'s too.
#[derive(GcObject)]
pub struct InputEventData {
    view: Member<Window>,
    detail: Cell<i32>,
    data: LocalExternal<Option<DomString>>,
    input_type: LocalExternal<DomString>,
    is_composing: Cell<bool>,
}

/// A `TextEvent`'s data (the legacy `textInput`'s), its `UIEvent`'s too.
#[derive(GcObject)]
pub struct TextEventData {
    view: Member<Window>,
    detail: Cell<i32>,
    data: LocalExternal<Cell<Option<DomString>>>,
}

/// A `KeyboardEvent`'s data, its `UIEvent`'s too.
#[derive(GcObject)]
pub struct KeyboardEventData {
    view: Member<Window>,
    detail: Cell<i32>,
    key: LocalExternal<DomString>,
    code: LocalExternal<DomString>,
    /// `location`, `charCode`, `keyCode`.
    numbers: Cell<[u32; 3]>,
    /// `modifiers`' bits.
    modifiers: Cell<u16>,
    /// `repeat`, `isComposing`.
    flags: Cell<[bool; 2]>,
}

/// A `FocusEvent`'s data, its `UIEvent`'s too: its related target (the
/// element losing or taking the focus).
#[derive(GcObject)]
pub struct FocusEventData {
    view: Member<Window>,
    detail: Cell<i32>,
    related_target: Member<Erased>,
}

/// What a keyboard event says, besides its type's flags and its view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardEventInfo {
    pub detail: i32,
    pub key: DomString,
    pub code: DomString,
    /// `DOM_KEY_LOCATION_STANDARD` (0), `_LEFT`, `_RIGHT`, `_NUMPAD`.
    pub location: u32,
    pub repeat: bool,
    pub is_composing: bool,
    /// `modifiers`' bits.
    pub modifiers: u16,
    /// The legacy `charCode` and `keyCode`.
    pub char_code: u32,
    pub key_code: u32,
}

/// What a wheel event says besides its mouse event's.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WheelEventInfo {
    pub delta_x: f64,
    pub delta_y: f64,
    pub delta_z: f64,
    /// `DOM_DELTA_PIXEL` (0), `_LINE`, `_PAGE`.
    pub delta_mode: u32,
}

/// What an input event says, besides its type's flags and its view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputEventInfo {
    pub detail: i32,
    pub data: Option<DomString>,
    pub input_type: DomString,
    pub is_composing: bool,
}

/// What a mouse event says, besides its type's flags, its view and its
/// related target.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MouseEventInfo {
    pub detail: i32,
    pub client_x: f64,
    pub client_y: f64,
    pub screen_x: f64,
    pub screen_y: f64,
    pub movement_x: f64,
    pub movement_y: f64,
    /// DOM's `button`: 0 the primary button, 1 the middle one, 2 the
    /// secondary one.
    pub button: i16,
    /// The buttons down, a bit each (1 the primary, 2 the secondary, 4 the
    /// middle one).
    pub buttons: u16,
    /// `modifiers`' bits.
    pub modifiers: u16,
}

impl<'gc> Cx<'_, 'gc> {
    /// Makes `event` a `UIEvent` of `view` and `detail`.
    pub fn set_ui_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        detail: i32,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            UiEventDataInit {
                view,
                detail: Cell::new(detail),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// Makes `event` a `MouseEvent` of `view`, `info` and `related_target`;
    /// a `WheelEvent` stays one, of them (`initMouseEvent`).
    pub fn set_mouse_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        info: &MouseEventInfo,
        related_target: Option<TargetRef<'gc>>,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            MouseEventDataInit {
                view,
                detail: Cell::new(info.detail),
                coordinates: Cell::new([
                    info.client_x,
                    info.client_y,
                    info.screen_x,
                    info.screen_y,
                    info.movement_x,
                    info.movement_y,
                ]),
                button: Cell::new(info.button),
                buttons: Cell::new(info.buttons),
                modifiers: Cell::new(info.modifiers),
                related_target: related_target.map(TargetRef::erase),
            },
        );
        match self.wheel_event_data(event) {
            Some(wheel) => wheel.mouse.set(self.gc, Some(data)),
            None => event.data.set(self.gc, Some(data.erase())),
        }
    }

    /// Makes `event` a `WheelEvent` of `view`, `mouse`, `related_target`
    /// and `wheel`.
    pub fn set_wheel_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        mouse: &MouseEventInfo,
        related_target: Option<TargetRef<'gc>>,
        wheel: &WheelEventInfo,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            WheelEventDataInit {
                mouse: None,
                deltas: Cell::new([wheel.delta_x, wheel.delta_y, wheel.delta_z]),
                delta_mode: Cell::new(wheel.delta_mode),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
        self.set_mouse_event_data(event, view, mouse, related_target);
    }

    /// Makes `event` an `InputEvent` of `view` and `info`.
    pub fn set_input_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        info: &InputEventInfo,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            InputEventDataInit {
                view,
                detail: Cell::new(info.detail),
                data: LocalExternal::new(info.data.clone()),
                input_type: LocalExternal::new(info.input_type.clone()),
                is_composing: Cell::new(info.is_composing),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `WheelEvent` data, if it is one.
    pub fn wheel_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, WheelEventData>> {
        event.data.get(self.gc)?.downcast::<WheelEventData>()
    }

    /// What a wheel event says besides its mouse event's.
    pub fn wheel_event_info(&self, data: Gc<'gc, WheelEventData>) -> WheelEventInfo {
        let [delta_x, delta_y, delta_z] = data.deltas.get();
        WheelEventInfo {
            delta_x,
            delta_y,
            delta_z,
            delta_mode: data.delta_mode.get(),
        }
    }

    /// Makes `event` a `TextEvent` of `view`, its data `data`.
    pub fn set_text_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        data: &str,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            TextEventDataInit {
                view,
                detail: Cell::new(0),
                data: LocalExternal::new(Cell::new(Some(DomString::new(data)))),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `TextEvent` data, if it is one.
    pub fn text_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, TextEventData>> {
        event.data.get(self.gc)?.downcast::<TextEventData>()
    }

    /// A text event's `data`.
    pub fn text_event_text(&self, data: Gc<'gc, TextEventData>) -> DomString {
        let cell = data.into_ref().data.get();
        let text = cell.take();
        cell.set(text.clone());
        text.unwrap_or_else(|| DomString::new(""))
    }

    /// A text event's `data` set (`initTextEvent`).
    pub fn set_text_event_text(&self, data: Gc<'gc, TextEventData>, text: &str) {
        data.into_ref().data.get().set(Some(DomString::new(text)));
    }

    /// `event`'s `InputEvent` data, if it is one.
    pub fn input_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, InputEventData>> {
        event.data.get(self.gc)?.downcast::<InputEventData>()
    }

    /// What an input event says.
    pub fn input_event_info(&self, data: Gc<'gc, InputEventData>) -> InputEventInfo {
        let is_composing = data.is_composing.get();
        let data = data.into_ref();
        InputEventInfo {
            detail: data.detail.get(),
            data: data.data.get().clone(),
            input_type: data.input_type.get().clone(),
            is_composing,
        }
    }

    /// Makes `event` a `KeyboardEvent` of `view` and `info`.
    pub fn set_keyboard_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        info: &KeyboardEventInfo,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            KeyboardEventDataInit {
                view,
                detail: Cell::new(info.detail),
                key: LocalExternal::new(info.key.clone()),
                code: LocalExternal::new(info.code.clone()),
                numbers: Cell::new([info.location, info.char_code, info.key_code]),
                modifiers: Cell::new(info.modifiers),
                flags: Cell::new([info.repeat, info.is_composing]),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// Makes `event` a `FocusEvent` of `view`, `detail` and
    /// `related_target`.
    pub fn set_focus_event_data(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        detail: i32,
        related_target: Option<TargetRef<'gc>>,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            FocusEventDataInit {
                view,
                detail: Cell::new(detail),
                related_target: related_target.map(TargetRef::erase),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `FocusEvent` data, if it is one.
    pub fn focus_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, FocusEventData>> {
        event.data.get(self.gc)?.downcast::<FocusEventData>()
    }

    pub fn focus_related_target(&self, data: Gc<'gc, FocusEventData>) -> Option<TargetRef<'gc>> {
        data.related_target.get(self.gc).map(TargetRef::from_erased)
    }

    /// `event`'s `KeyboardEvent` data, if it is one.
    pub fn keyboard_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, KeyboardEventData>> {
        event.data.get(self.gc)?.downcast::<KeyboardEventData>()
    }

    /// What a keyboard event says.
    pub fn keyboard_event_info(&self, data: Gc<'gc, KeyboardEventData>) -> KeyboardEventInfo {
        let [location, char_code, key_code] = data.numbers.get();
        let [repeat, is_composing] = data.flags.get();
        let data = data.into_ref();
        KeyboardEventInfo {
            detail: data.detail.get(),
            key: data.key.get().clone(),
            code: data.code.get().clone(),
            location,
            repeat,
            is_composing,
            modifiers: data.modifiers.get(),
            char_code,
            key_code,
        }
    }

    /// `event`'s `MouseEvent` data, if it is one (a wheel event's too).
    pub fn mouse_event_data(&self, event: Gc<'gc, Event>) -> Option<Gc<'gc, MouseEventData>> {
        let data = event.data.get(self.gc)?;
        data.downcast::<MouseEventData>().or_else(|| {
            data.downcast::<WheelEventData>()
                .and_then(|wheel| wheel.mouse.get(self.gc))
        })
    }

    /// Whether `event` is a `UIEvent` (a mouse event too).
    pub fn is_ui_event(&self, event: Gc<'gc, Event>) -> bool {
        event.data.get(self.gc).is_some_and(|data| {
            data.downcast::<UiEventData>().is_some()
                || data.downcast::<MouseEventData>().is_some()
                || data.downcast::<WheelEventData>().is_some()
                || data.downcast::<KeyboardEventData>().is_some()
                || data.downcast::<FocusEventData>().is_some()
                || data.downcast::<InputEventData>().is_some()
                || data.downcast::<TextEventData>().is_some()
        })
    }

    /// A `UIEvent`'s `view` and `detail` (none and 0 for another event).
    pub fn ui_event_view_and_detail(
        &self,
        event: Gc<'gc, Event>,
    ) -> (Option<Gc<'gc, Window>>, i32) {
        let Some(data) = event.data.get(self.gc) else {
            return (None, 0);
        };
        if let Some(ui) = data.downcast::<UiEventData>() {
            return (ui.view.get(self.gc), ui.detail.get());
        }
        if let Some(mouse) = self.mouse_event_data(event) {
            return (mouse.view.get(self.gc), mouse.detail.get());
        }
        if let Some(key) = data.downcast::<KeyboardEventData>() {
            return (key.view.get(self.gc), key.detail.get());
        }
        if let Some(input) = data.downcast::<InputEventData>() {
            return (input.view.get(self.gc), input.detail.get());
        }
        if let Some(text) = data.downcast::<TextEventData>() {
            return (text.view.get(self.gc), text.detail.get());
        }
        match data.downcast::<FocusEventData>() {
            Some(focus) => (focus.view.get(self.gc), focus.detail.get()),
            None => (None, 0),
        }
    }

    /// Sets a `UIEvent`'s `view` and `detail` (`initUIEvent`), of whichever
    /// UI event it is; nothing for another event.
    pub fn set_ui_event_view_and_detail(
        &self,
        event: Gc<'gc, Event>,
        view: Option<Gc<'gc, Window>>,
        detail: i32,
    ) {
        let Some(data) = event.data.get(self.gc) else {
            return;
        };
        let (view_field, detail_field) = if let Some(ui) = data.downcast::<UiEventData>() {
            let ui = ui.into_ref();
            (&ui.view, &ui.detail)
        } else if let Some(mouse) = self.mouse_event_data(event) {
            let mouse = mouse.into_ref();
            (&mouse.view, &mouse.detail)
        } else if let Some(key) = data.downcast::<KeyboardEventData>() {
            let key = key.into_ref();
            (&key.view, &key.detail)
        } else if let Some(input) = data.downcast::<InputEventData>() {
            let input = input.into_ref();
            (&input.view, &input.detail)
        } else if let Some(text) = data.downcast::<TextEventData>() {
            let text = text.into_ref();
            (&text.view, &text.detail)
        } else if let Some(focus) = data.downcast::<FocusEventData>() {
            let focus = focus.into_ref();
            (&focus.view, &focus.detail)
        } else {
            return;
        };
        view_field.set(self.gc, view);
        detail_field.set(detail);
    }

    /// What a mouse event says.
    pub fn mouse_event_info(&self, data: Gc<'gc, MouseEventData>) -> MouseEventInfo {
        let [
            client_x,
            client_y,
            screen_x,
            screen_y,
            movement_x,
            movement_y,
        ] = data.coordinates.get();
        MouseEventInfo {
            detail: data.detail.get(),
            client_x,
            client_y,
            screen_x,
            screen_y,
            movement_x,
            movement_y,
            button: data.button.get(),
            buttons: data.buttons.get(),
            modifiers: data.modifiers.get(),
        }
    }

    /// A mouse event's `relatedTarget`, as it was given (its getter
    /// retargets it against the current target).
    pub fn mouse_related_target(&self, data: Gc<'gc, MouseEventData>) -> Option<TargetRef<'gc>> {
        data.related_target.get(self.gc).map(TargetRef::from_erased)
    }

    /// `event`'s related target as it was given, a mouse or a focus event's
    /// (DOM's "relatedTarget"); none for another event: inline, as every
    /// dispatch asks (a plain event has no data to look at).
    #[inline]
    pub fn event_related_target(&self, event: Gc<'gc, Event>) -> Option<TargetRef<'gc>> {
        event.data.get(self.gc)?;
        self.subclass_related_target(event)
    }

    fn subclass_related_target(&self, event: Gc<'gc, Event>) -> Option<TargetRef<'gc>> {
        if let Some(data) = self.mouse_event_data(event) {
            return self.mouse_related_target(data);
        }
        self.focus_event_data(event)
            .and_then(|data| self.focus_related_target(data))
    }

    /// Sets a mouse or a focus event's related target (DOM's dispatch, at
    /// its end: what its path's last entry saw, or none for one in a
    /// shadow tree); nothing for another event.
    pub fn set_event_related_target(&self, event: Gc<'gc, Event>, related: Option<TargetRef<'gc>>) {
        let related = related.map(TargetRef::erase);
        if let Some(data) = self.mouse_event_data(event) {
            data.related_target.set(self.gc, related);
        } else if let Some(data) = self.focus_event_data(event) {
            data.related_target.set(self.gc, related);
        }
    }
}
