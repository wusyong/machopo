//! The user's mouse input on a document (Blink's `core/input`:
//! `EventHandler`, `MouseEventManager`, `BoundaryEventDispatcher`): what
//! the pointer is over (the frame's hit test, laid out and pre-painted
//! first), the document's hover and active elements, the boundary events
//! as the pointer goes from one element to another, and the mouse events
//! at what it is over; a click at what the press and the release have in
//! common, unless it is on or in a disabled form control. Each event is
//! dispatched out of any operation, with a microtask checkpoint after it,
//! as HTML's "clean up after running script" does once the stack is
//! empty. No text selection, no pointer capture yet: there is no editing.

use computed_style::{CursorKeyword, UserSelect};
use dom::names::event;
use dom::{
    DomString, Event, EventFlags, KeyboardEventInfo, MouseEventInfo, NameId, NodeRef, NodeRoot,
    TargetRef, TargetRoot, WheelEventInfo,
};
use elise::{Gc, Persistent};
use gfx::PointF;

use crate::typed::ScriptHost;
use crate::{ApiCx, dispatch};

/// A mouse button, as DOM's `button` numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    /// The primary one (the left one): 0.
    Primary,
    /// The auxiliary one (the middle one, the wheel): 1.
    Auxiliary,
    /// The secondary one (the right one): 2.
    Secondary,
}

impl MouseButton {
    fn index(self) -> usize {
        match self {
            Self::Primary => 0,
            Self::Auxiliary => 1,
            Self::Secondary => 2,
        }
    }

    fn dom(self) -> i16 {
        match self {
            Self::Primary => 0,
            Self::Auxiliary => 1,
            Self::Secondary => 2,
        }
    }
}

/// What the mouse did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    /// The pointer moved.
    Move,
    /// `button` went down, the `click_count`th press in a row (the
    /// platform counts them).
    Down {
        button: MouseButton,
        click_count: i32,
    },
    /// `button` went up, its press's `click_count`.
    Up {
        button: MouseButton,
        click_count: i32,
    },
}

/// One input of the mouse: what it did, where the pointer is (CSS px of
/// the viewport), the buttons down after it (DOM's `buttons` bits) and the
/// modifier keys held (`dom::modifiers`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MouseInput {
    pub action: MouseAction,
    pub point: PointF,
    pub buttons: u16,
    pub modifiers: u16,
}

/// A wheel turned over the frame (Blink's `WebMouseWheelEvent`): where
/// (CSS px of the viewport), how far it scrolls the page (CSS px:
/// `deltaMode` is pixels, as Chrome's), the buttons and modifier keys held
/// (`dom::modifiers`), and whether its `wheel` event can cancel the
/// scrolling (the compositor waits for it: a blocking listener is there).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WheelInput {
    pub point: PointF,
    pub delta: PointF,
    pub buttons: u16,
    pub modifiers: u16,
    pub cancelable: bool,
}

/// A key going down or up: the DOM's `key` and `code`, the platform's
/// legacy key code (Windows'), what it types (none: empty), its
/// `location`, whether it repeats, the modifier keys held
/// (`dom::modifiers`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyInput {
    pub down: bool,
    pub key: String,
    pub code: String,
    pub key_code: u32,
    pub text: String,
    pub location: u32,
    pub repeat: bool,
    pub modifiers: u16,
}

impl KeyInput {
    /// Whether it types something, so that it gets a `keypress`: text with
    /// no control character (Enter's carriage return aside), and not a
    /// shortcut (Ctrl without Alt, which on Windows is AltGr; Cmd).
    fn types_text(&self) -> bool {
        let printable = self.text == "\r" || !self.text.chars().any(char::is_control);
        let ctrl = self.modifiers & dom::modifiers::CTRL != 0;
        let alt = self.modifiers & dom::modifiers::ALT != 0;
        let meta = self.modifiers & dom::modifiers::META != 0;
        !self.text.is_empty() && printable && !(ctrl && !alt) && !meta
    }
}

/// What a key left for the host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyResult {
    Done,
    /// Tab went past the page's last (or, `reverse`, first) element that
    /// can take the focus: the browser takes it (`TakeFocus`).
    TakeFocus {
        reverse: bool,
    },
}

/// Whether the secondary button opens the context menu as it goes down,
/// or (Windows) as it goes up, as Blink's `WebFrameWidgetImpl` has it.
const CONTEXT_MENU_ON_PRESS: bool = !cfg!(windows);

/// The page's input state between inputs (Blink's `EventHandler` of each
/// `LocalFrame`): the host's engine keeps one for its frame tree. Each
/// frame's mouse state is its own, by its document; an input over an
/// iframe goes on to its child frame's (Blink's
/// `PassMousePressEventToSubframe`). Nodes are held as roots (few, between
/// inputs).
#[derive(Default)]
pub struct EventHandler {
    /// Each frame's, by its active document; one whose document no frame
    /// shows any more is let go of at the next input.
    frames: Vec<(NodeRoot, FrameMouse)>,
    /// The element Space went down on, which its release clicks (Blink's
    /// `HTMLInputElement` keeps it `:active` meanwhile).
    space: Option<NodeRoot>,
    /// The pointer's last input over the page, as a move: where it is (the
    /// top-level frame's viewport's), the buttons down and the modifier
    /// keys held (a key changes those): what is under it is found again
    /// from there after a layout (`recompute_under_mouse`).
    last_mouse: Option<MouseInput>,
}

/// A frame's mouse state between inputs (Blink's `MouseEventManager`).
#[derive(Default)]
struct FrameMouse {
    /// What the pointer was over and its ancestors, the deepest first, as
    /// when it got there (Blink's `element_under_mouse_`, which tracks
    /// removals): one taken out of the document since is found by the
    /// nearest of them still in it. An iframe whose child frame the pointer
    /// is in.
    under_mouse: Vec<NodeRoot>,
    /// Where each button went down, by `MouseButton::index`.
    pressed: [Option<NodeRoot>; 3],
    /// What a button last went down on, kept after the release: where a
    /// key scrolls from with no focused element (Blink's
    /// `mouse_press_node_`).
    press_node: Option<NodeRoot>,
    /// Where the pointer was last: what `movementX` counts from.
    last_point: Option<PointF>,
    /// A number field and its spin button the primary button is holding
    /// (`controls`: its moves and release go to the press).
    spin: Option<(NodeRoot, NodeRoot)>,
    /// The child frame's document a button went down in, which the moves
    /// and the releases go to until none is down (Blink's
    /// `capturing_mouse_events_frame_`).
    subframe: Option<NodeRoot>,
}

impl EventHandler {
    /// `input` on the frame tree of `document`, the top-level frame's
    /// (Blink's `EventHandler::HandleMouseMoveEvent`,
    /// `HandleMousePressEvent`, `HandleMouseReleaseEvent`): from the
    /// top-level frame down, each frame whose iframe the pointer is over
    /// (or whose child frame a button went down in) has it over the iframe
    /// (`:hover`, `:active`, its boundary events) and passes it on, its
    /// point in the child frame's viewport; the frame it ends in has its
    /// events. A frame the pointer left has it over nothing. The cursor
    /// for what the pointer is over, as it was before the events
    /// (`select_cursor`; Blink's `HandleMouseMoveOrLeaveEvent` sets it
    /// from the hit test before it dispatches).
    pub fn handle_mouse<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
    ) -> CursorKeyword {
        self.last_mouse = Some(MouseInput {
            action: MouseAction::Move,
            ..*input
        });
        self.mouse(host, document, input, true)
    }

    /// What the pointer is over, found again where it last was after a
    /// layout, a scroll or a style change that may have moved what is
    /// under it (Blink's `MouseEventManager::RecomputeMouseHoverState`):
    /// the hover states and the boundary events, no `mousemove`; and the
    /// cursor for it (Blink's `EventHandler::ScheduleCursorUpdate`). None
    /// before the pointer was over the page.
    pub fn recompute_under_mouse<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
    ) -> Option<CursorKeyword> {
        let input = self.last_mouse?;
        Some(self.mouse(host, document, &input, false))
    }

    /// `handle_mouse`, its `mousemove` only if `moves` (not when what is
    /// under a pointer that stays is found again).
    fn mouse<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
        moves: bool,
    ) -> CursorKeyword {
        self.forget_gone_frames(host, document);
        // The topmost element at the point in the frame tree, and whether
        // it is a run of its text there.
        let dragging = input.buttons != 0 || matches!(input.action, MouseAction::Up { .. });
        let (deep, over_text) = mouse_target(host, document, input.point, dragging);
        let cursor = select_cursor(host, deep.as_ref(), over_text);
        // The frames it goes through, decided first (Blink picks the
        // subframe before the events): each with its point, what it is over
        // there and the child frame it goes on to.
        let mut steps = Vec::new();
        let mut at = document.clone();
        // Whether `at`'s parent passed it on captured.
        let mut captured_here = false;
        loop {
            let point = point_in_frame(host, document, &at, input.point).unwrap_or(input.point);
            let index = self.frame_index(host, &at);
            let captured = self.frames[index]
                .1
                .subframe
                .clone()
                .filter(|_| !matches!(input.action, MouseAction::Down { .. }));
            let mut target = deep.as_ref().and_then(|deep| node_in(host, &at, deep));
            // Captured, the pointer over none of the frame's elements: its
            // root (Blink's hit test of a point outside the frame).
            if target.is_none() && captured_here {
                target = host.read(|cx| {
                    crate::geometry::document_element(cx, at.get(cx.gc)).map(|root| cx.root(root))
                });
            }
            captured_here = captured.is_some();
            let child = match captured {
                Some(child) => host.read(|cx| {
                    cx.frame_owner(child.get(cx.gc))
                        .map(|iframe| (cx.root(iframe), child.clone()))
                }),
                None => target.as_ref().and_then(|target| {
                    host.read(|cx| {
                        let child = cx.content_document(target.get(cx.gc))?;
                        // The pointer is in the child frame, not on the
                        // iframe's border: what it is over isn't the iframe.
                        let deep = deep.as_ref()?.get(cx.gc);
                        (!Gc::ptr_eq(deep, target.get(cx.gc)))
                            .then(|| (target.clone(), cx.root(child)))
                    })
                }),
            };
            let next = child.as_ref().map(|(_, child)| child.clone());
            steps.push((index, MouseInput { point, ..*input }, target, child));
            match next {
                Some(child) => at = child,
                None => break,
            }
        }
        // The frames it left first, deepest first: over nothing in them now
        // (Blink's `HandleMouseLeaveEvent` of the subframe it was in).
        for index in (0..self.frames.len()).rev() {
            if !steps.iter().any(|(at, ..)| *at == index)
                && !self.frames[index].1.under_mouse.is_empty()
            {
                let left = self.frames[index].0.clone();
                let point =
                    point_in_frame(host, document, &left, input.point).unwrap_or(input.point);
                let info = event_info(&MouseInput { point, ..*input }, 0, 0);
                self.frames[index]
                    .1
                    .update_under_mouse(host, &left, None, &info);
            }
        }
        for (index, input, target, child) in steps {
            let (document, frame) = &mut self.frames[index];
            let document = document.clone();
            match child {
                Some((iframe, child)) => {
                    frame.pass_to_subframe(host, &document, &input, &iframe, &child)
                }
                None => frame.handle_mouse(host, &document, &input, target, moves),
            }
        }
        cursor
    }

    /// The index of `document`'s frame's state, made the first time.
    fn frame_index<H: ScriptHost>(&mut self, host: &mut H, document: &NodeRoot) -> usize {
        let found = host.read(|cx| {
            let document = document.get(cx.gc);
            self.frames
                .iter()
                .position(|(at, _)| Gc::ptr_eq(at.get(cx.gc), document))
        });
        found.unwrap_or_else(|| {
            self.frames.push((document.clone(), FrameMouse::default()));
            self.frames.len().saturating_sub(1)
        })
    }

    /// The frames' states whose documents no frame of `top`'s tree shows
    /// any more (navigated, their iframes removed), let go of.
    fn forget_gone_frames<H: ScriptHost>(&mut self, host: &mut H, top: &NodeRoot) {
        if self.frames.len() < 2 {
            return;
        }
        let shown: Vec<bool> = host.read(|cx| {
            let documents = crate::documents_in_tree_order(cx, top.get(cx.gc));
            self.frames
                .iter()
                .map(|(document, _)| {
                    documents
                        .iter()
                        .any(|&at| Gc::ptr_eq(at, document.get(cx.gc)))
                })
                .collect()
        });
        let mut shown = shown.into_iter();
        self.frames.retain(|_| shown.next().unwrap_or(false));
    }

    /// `input`'s `wheel` event at what is under it in the frame tree of
    /// `document`, its point the frame's it is in (Blink's
    /// `MouseWheelEventManager::HandleWheelEvent`: no capture of the
    /// mouse; trusted, bubbling, composed), none when the DOM has no
    /// `wheel` or `mousewheel` listener; then a microtask checkpoint.
    /// Whether nothing canceled it: the host scrolls then.
    pub fn handle_wheel<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &WheelInput,
    ) -> bool {
        if !host.read(|cx| cx.may_have_wheel_listeners()) {
            return true;
        }
        let (Some(target), _) = mouse_target(host, document, input.point, false) else {
            return true;
        };
        // Its point in the viewport of the frame it is in.
        let frame = host.read(|cx| cx.root(cx.document_of(target.get(cx.gc))));
        let input = &WheelInput {
            point: point_in_frame(host, document, &frame, input.point).unwrap_or(input.point),
            ..*input
        };
        let event = host.mutate(|cx| {
            let node = target.get(cx.gc);
            let window = cx.target_window(TargetRef::Node(node));
            let event = cx.create_event(
                event::WHEEL,
                EventFlags {
                    bubbles: true,
                    cancelable: input.cancelable,
                    composed: true,
                    trusted: true,
                },
                window,
            );
            let mouse = MouseEventInfo {
                detail: 0,
                client_x: f64::from(input.point.x),
                client_y: f64::from(input.point.y),
                screen_x: f64::from(input.point.x),
                screen_y: f64::from(input.point.y),
                movement_x: 0.0,
                movement_y: 0.0,
                button: 0,
                buttons: input.buttons,
                modifiers: input.modifiers,
            };
            let wheel = WheelEventInfo {
                delta_x: f64::from(input.delta.x),
                delta_y: f64::from(input.delta.y),
                delta_z: 0.0,
                delta_mode: 0,
            };
            cx.set_wheel_event_data(event, window, &mouse, None, &wheel);
            Persistent::new(cx.gc, event)
        });
        let not_canceled = dispatch(host, &TargetRoot::Node(target), &event);
        host.perform_microtask_checkpoint();
        not_canceled
    }

    /// A key went down or up (Blink's `KeyboardEventManager::KeyEvent`):
    /// `keydown` or `keyup` at the document's focused element, else its
    /// body, else its root; after a `keydown` nobody canceled that types
    /// something, `keypress` there, its `charCode` and `keyCode` the
    /// character. A key down shows focus rings from now on. Tab (Shift+Tab
    /// backwards) nobody canceled moves the focus (Blink's
    /// `DefaultKeyboardEventHandler`); past the ends the browser takes it
    /// back. In a text control (`editing`), a `keydown` of Backspace,
    /// Delete, an arrow, Home or End nobody canceled edits or moves, a
    /// `keypress` nobody canceled types. Enter on the focused link clicks
    /// it; Space and Enter click the button the target is (or is in). A
    /// scrolling key nobody took scrolls from the focused element out
    /// (`keyboard_scroll`).
    pub fn handle_key<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &KeyInput,
    ) -> KeyResult {
        // The keys held: what the boundary events have if a layout moves
        // what is under the pointer before it moves again.
        if let Some(last) = &mut self.last_mouse {
            last.modifiers = input.modifiers;
        }
        // The focused frame's (Blink's `FocusController::FocusedFrame`).
        let top = document;
        let document = &host.read(|cx| cx.root(cx.focused_frame(cx.top_document(top.get(cx.gc)))));
        if input.down {
            crate::focus::key_shows_rings(host, document);
        }
        let event_type = if input.down {
            event::KEYDOWN
        } else {
            event::KEYUP
        };
        let info = KeyboardEventInfo {
            detail: 0,
            key: DomString::new(&input.key),
            code: DomString::new(&input.code),
            location: input.location,
            repeat: input.repeat,
            is_composing: false,
            modifiers: input.modifiers,
            char_code: 0,
            key_code: input.key_code,
        };
        let Some(target) = key_target(host, document) else {
            return KeyResult::Done;
        };
        // HTML's activation-triggering input event: a `keydown` but Esc's.
        if input.down && input.key != "Escape" {
            notify_activation(host, &target);
        }
        let not_canceled = fire_key(host, &target, event_type, &info);
        let shortcut = input.modifiers
            & (dom::modifiers::CTRL | dom::modifiers::ALT | dom::modifiers::META)
            != 0;
        if input.down && not_canceled && input.key == "Tab" && !shortcut {
            let reverse = input.modifiers & dom::modifiers::SHIFT != 0;
            if !crate::focus::sequential_navigation(host, top, reverse) {
                return KeyResult::TakeFocus { reverse };
            }
            return KeyResult::Done;
        }
        // A select's: Alt+↓ and Alt+↑, Ctrl's too.
        if input.down
            && not_canceled
            && (crate::menu_list::keydown(host, &target, &input.key, input.modifiers)
                || crate::list_box::keydown(host, &target, &input.key, input.modifiers))
        {
            return KeyResult::Done;
        }
        // Enter on the focused link clicks it: a trusted simulated click
        // with the key's modifiers, so Ctrl+Enter opens a tab behind
        // (Blink's `HTMLAnchorElement::DefaultEventHandler`); the key is
        // the link's then, with no `keypress`.
        if input.down
            && not_canceled
            && input.key == "Enter"
            && host.read(|cx| {
                let target = target.get(cx.gc);
                cx.focused_element(cx.document_of(target))
                    .is_some_and(|focused| Gc::ptr_eq(focused, target))
                    && crate::hyperlink::is_hyperlink(cx, target)
            })
        {
            crate::activation::simulated_click(host, &target, true, input.modifiers);
            return KeyResult::Done;
        }
        if input.down && not_canceled && !shortcut {
            if let Some(key) = crate::editing::EditKey::of(&input.key) {
                let extend = input.modifiers & dom::modifiers::SHIFT != 0;
                if crate::editing::edit_key(host, &target, key, extend) {
                    return KeyResult::Done;
                }
            }
            if crate::controls::keydown(host, &target, &input.key) {
                return KeyResult::Done;
            }
            if input.key == " " {
                self.space = host.read(|cx| {
                    crate::controls::key_clicked(cx, target.get(cx.gc), false)
                        .map(|clicked| cx.root(clicked))
                });
            }
        }
        // Space's release clicks what it went down on (Blink's
        // `HTMLInputElement::HandleKeyupEvent`).
        if !input.down && input.key == " " {
            let space = self.space.take();
            if not_canceled
                && let Some(space) = space
                && host.read(|cx| {
                    crate::controls::key_clicked(cx, target.get(cx.gc), false)
                        .is_some_and(|clicked| elise::Gc::ptr_eq(space.get(cx.gc), clicked))
                })
            {
                crate::controls::key_click(host, &space);
            }
        }
        // A control the Space went down on takes it (Blink's buttons and
        // check boxes mark its `keypress` handled): it doesn't scroll.
        let mut scrolls =
            input.down && not_canceled && !shortcut && !(input.key == " " && self.space.is_some());
        if input.down && not_canceled && input.types_text() {
            // At the target as it is now: a listener may have taken it out.
            let Some(target) = key_target(host, document) else {
                return KeyResult::Done;
            };
            let char_code = input.text.chars().next().map_or(0, u32::from);
            let info = KeyboardEventInfo {
                char_code,
                key_code: char_code,
                ..info
            };
            // Its `keypress` canceled or handled (what it typed, a
            // button's Enter) keeps it from scrolling too, as in Blink,
            // whose Space scrolls on the `keypress`.
            let handled = !fire_key(host, &target, event::KEYPRESS, &info)
                || crate::menu_list::keypress(host, &target, &input.text)
                || crate::list_box::keypress(host, &target, &input.text)
                || crate::editing::type_text(host, &target, &input.text)
                || (input.text == "\r" && {
                    let button = host.read(|cx| {
                        crate::controls::key_clicked(cx, target.get(cx.gc), true)
                            .map(|clicked| cx.root(clicked))
                    });
                    // A button's Enter clicks it.
                    if let Some(button) = &button {
                        crate::controls::key_click(host, button);
                    }
                    button.is_some()
                });
            scrolls &= !handled;
        }
        if scrolls {
            self.keyboard_scroll(host, top, document, input);
        }
        KeyResult::Done
    }

    /// A scrolling key nobody took (Blink's `KeyboardEventManager::
    /// DefaultKeyboardEventHandler`, `ScrollManager::LogicalScroll`): from
    /// the focused element of `document` (the focused frame's), else what
    /// the mouse last went down on there, else the document, out through
    /// the frames around it (`FrameLayout::keyboard_scroll`, the frame
    /// tree's layout of `top`). Not a scrolling key: nothing moves.
    fn keyboard_scroll<H: ScriptHost>(
        &mut self,
        host: &mut H,
        top: &NodeRoot,
        document: &NodeRoot,
        input: &KeyInput,
    ) {
        let index = self.frame_index(host, document);
        let pressed = self.frames[index].1.press_node.clone();
        let shift = input.modifiers & dom::modifiers::SHIFT != 0;
        host.mutate(|cx| {
            let mut document = document.get(cx.gc);
            let mut start = cx.focused_element(document).or_else(|| {
                pressed
                    .as_ref()
                    .map(|pressed| pressed.get(cx.gc))
                    .filter(|&pressed| {
                        cx.is_connected(pressed) && Gc::ptr_eq(cx.document_of(pressed), document)
                    })
            });
            // Each node out to the top-level document, each document too:
            // its viewport's scroll node is its own.
            let mut chain = Vec::new();
            loop {
                let from = start.take().unwrap_or(document);
                chain.extend(
                    std::iter::successors(Some(from), |&node| cx.flat_tree_parent(node))
                        .map(|node| cx.node_id(node)),
                );
                if chain.last() != Some(&cx.node_id(document)) {
                    chain.push(cx.node_id(document));
                }
                let Some(owner) = cx.frame_owner(document) else {
                    break;
                };
                document = cx.document_of(owner);
                start = Some(owner);
            }
            cx.with_layout(cx.top_document(top.get(cx.gc)), |layout| {
                layout.keyboard_scroll(&chain, &input.key, shift)
            })
        });
    }
}

impl FrameMouse {
    /// `input` on `document` (its point the frame's viewport's), over
    /// `target`, the frame's events.
    fn handle_mouse<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
        target: Option<NodeRoot>,
        moves: bool,
    ) {
        // A spin button the primary button holds has the mouse: its moves
        // and release go to it, wherever the pointer is (Blink's
        // `SpinButtonElement` captures the mouse events), and the page
        // sees them at its field.
        let target = match (&self.spin, input.action) {
            (Some((_, part)), MouseAction::Move | MouseAction::Up { .. }) => Some(part.clone()),
            _ => target,
        };
        match input.action {
            MouseAction::Move => self.mouse_move(host, document, input, target, moves),
            MouseAction::Down {
                button,
                click_count,
            } => self.mouse_down(host, document, input, button, click_count, target),
            MouseAction::Up {
                button,
                click_count,
            } => self.mouse_up(host, document, input, button, click_count, target),
        }
        self.last_point = Some(input.point);
    }

    /// `input` over `iframe` in `document`, going on to its child frame
    /// (`child`, its document): the iframe is what the pointer is over
    /// here (`:hover`, the boundary events) and, while a button is down,
    /// `:active`; a button going down there captures the child frame until
    /// none is down. No mouse event of its own here (Blink's subframe gets
    /// it).
    fn pass_to_subframe<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
        iframe: &NodeRoot,
        child: &NodeRoot,
    ) {
        match input.action {
            MouseAction::Down { .. } => {
                host.mutate(|cx| {
                    cx.set_active_element(document.get(cx.gc), Some(iframe.get(cx.gc)))
                });
                self.subframe = Some(child.clone());
            }
            MouseAction::Up { .. } if input.buttons == 0 => {
                host.mutate(|cx| cx.set_active_element(document.get(cx.gc), None));
                self.subframe = None;
            }
            _ => {}
        }
        self.update_under_mouse(host, document, Some(iframe), &event_info(input, 0, 0));
        self.last_point = Some(input.point);
    }

    fn mouse_move<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
        target: Option<NodeRoot>,
        moves: bool,
    ) {
        let last = self.last_point.unwrap_or(input.point);
        let info = MouseEventInfo {
            movement_x: f64::from(input.point.x - last.x),
            movement_y: f64::from(input.point.y - last.y),
            ..event_info(input, 0, 0)
        };
        self.update_under_mouse(host, document, target.as_ref(), &event_info(input, 0, 0));
        if !moves {
            return;
        }
        if let Some(target) = self.still_under_mouse(host, document, target)
            && fire_mouse(host, &target, event::MOUSEMOVE, &info, None)
            && input.buttons & 1 != 0
        {
            // A list box's drag (the primary button held).
            crate::list_box::mouse_moved(host, &target);
        }
        if let Some((field, part)) = &self.spin {
            crate::controls::spin_moved(host, field, part, input.point);
        }
    }

    /// A button went down: what it went down on, and the elements around
    /// it, are `:active` (Blink's `HitTestRequest::kActive`), and
    /// `mousedown` goes there; the secondary button then opens the context
    /// menu where the platform does it on the press.
    fn mouse_down<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
        button: MouseButton,
        click_count: i32,
        target: Option<NodeRoot>,
    ) {
        if let Some(target) = &target {
            host.mutate(|cx| cx.set_active_element(document.get(cx.gc), Some(target.get(cx.gc))));
        }
        self.update_under_mouse(host, document, target.as_ref(), &event_info(input, 0, 0));
        let Some(target) = target else {
            return;
        };
        self.pressed[button.index()] = Some(target.clone());
        self.press_node = Some(target.clone());
        // HTML's activation-triggering input event.
        notify_activation(host, &target);
        if fire_mouse(
            host,
            &target,
            event::MOUSEDOWN,
            &event_info(input, button.dom(), click_count),
            None,
        ) {
            crate::focus::mouse_focus(host, document, &target);
            if button == MouseButton::Primary
                && !crate::menu_list::mouse_down(host, &target)
                && !crate::list_box::mouse_down(host, &target, input.modifiers)
            {
                self.spin = crate::controls::mouse_down(host, &target, input.point);
            }
        }
        if button == MouseButton::Secondary && CONTEXT_MENU_ON_PRESS {
            context_menu(host, &target, input);
        }
    }

    /// A button went up: with none down, nothing is `:active` and `:hover`
    /// follows the pointer again; `mouseup` goes where it is, then `click`
    /// (the primary button) or `auxclick` at what that and where the
    /// button went down have in common (UI Events; Blink's
    /// `MouseEventManager::GetClickTarget`), and `dblclick` after the
    /// second click of a series; the secondary button opens the context
    /// menu where the platform does it on the release.
    fn mouse_up<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        input: &MouseInput,
        button: MouseButton,
        click_count: i32,
        released: Option<NodeRoot>,
    ) {
        if input.buttons == 0 {
            host.mutate(|cx| cx.set_active_element(document.get(cx.gc), None));
        }
        self.update_under_mouse(host, document, released.as_ref(), &event_info(input, 0, 0));
        let pressed = self.pressed[button.index()].take();
        let click = match (&pressed, &released) {
            (Some(pressed), Some(released)) => host.mutate(|cx| {
                common_ancestor(cx, pressed.get(cx.gc), released.get(cx.gc))
                    .map(|node| cx.root(node))
            }),
            _ => None,
        };
        let info = event_info(input, button.dom(), click_count);
        if let Some(released) = &released
            && fire_mouse(host, released, event::MOUSEUP, &info, None)
            && button == MouseButton::Primary
        {
            crate::list_box::mouse_up(host, released);
        }
        if button == MouseButton::Primary
            && let Some((field, _)) = self.spin.take()
        {
            crate::controls::spin_released(host, &field);
        }
        let click = click.filter(|click| {
            !host.read(|cx| crate::activation::in_disabled_control(cx, click.get(cx.gc)))
        });
        if let Some(click) = &click {
            let primary = button == MouseButton::Primary;
            let (not_canceled, event) = dispatch_mouse(
                host,
                click,
                if primary {
                    event::CLICK
                } else {
                    event::AUXCLICK
                },
                &info,
                None,
            );
            if not_canceled && primary {
                crate::controls::click(host, click);
            }
            // The middle button's `auxclick` nobody canceled follows the
            // link it is in, in a tab behind (Blink's
            // `HTMLAnchorElement::DefaultEventHandler` takes it as a link
            // click).
            if not_canceled
                && button == MouseButton::Auxiliary
                && let Some(link) = host.read(|cx| {
                    std::iter::successors(Some(click.get(cx.gc)), |&node| cx.flat_tree_parent(node))
                        .find(|&node| crate::hyperlink::is_hyperlink(cx, node))
                        .map(|link| cx.root(link))
                })
            {
                crate::hyperlink::follow_hyperlink(host, &link, &event);
            }
            if primary && click_count == 2 {
                fire_mouse(host, click, event::DBLCLICK, &info, None);
            }
        }
        if button == MouseButton::Secondary
            && !CONTEXT_MENU_ON_PRESS
            && let Some(released) = &released
        {
            context_menu(host, released, input);
        }
    }

    /// The pointer is over `target` (none: nothing of the document): the
    /// hover element, and the boundary events where what it is over
    /// changed (Blink's `MouseEventManager::SetElementUnderMouse` with
    /// `Document::UpdateHoverActiveState`). While a button is down,
    /// `:hover` keeps to the active chain: an element is hovered only if
    /// it, or a descendant, is active.
    fn update_under_mouse<H: ScriptHost>(
        &mut self,
        host: &mut H,
        document: &NodeRoot,
        target: Option<&NodeRoot>,
        info: &MouseEventInfo,
    ) {
        // Recorded before the events, whose listeners may take the nodes
        // out.
        let chain = host.mutate(|cx| {
            let document = document.get(cx.gc);
            let element = target.map(|target| target.get(cx.gc));
            let hover = match (element, cx.active_element(document)) {
                (Some(element), Some(active)) => common_ancestor(cx, element, active),
                (element, _) => element,
            };
            cx.set_hover_element(document, hover);
            element.map_or_else(Vec::new, |element| {
                std::iter::successors(Some(element), |&node| cx.flat_tree_parent(node))
                    .map(|node| cx.root(node))
                    .collect()
            })
        });
        let old = std::mem::replace(&mut self.under_mouse, chain);
        let exited = exited(host, document, &old);
        send_boundary_events(host, exited, target, info);
    }

    /// Where `target`'s event goes once the boundary events are done: a
    /// listener may have taken it out of its document, and then the
    /// pointer is in the nearest of its old ancestors still there.
    fn still_under_mouse<H: ScriptHost>(
        &self,
        host: &mut H,
        document: &NodeRoot,
        target: Option<NodeRoot>,
    ) -> Option<NodeRoot> {
        let target = target?;
        match exited(host, document, &self.under_mouse) {
            Some((still, true)) => Some(still),
            _ => Some(target),
        }
    }
}

/// Where a key's events go (Blink's `EventTargetNodeForDocument`): the
/// focused element, else the body, else the root element, else none.
fn key_target<H: ScriptHost>(host: &mut H, document: &NodeRoot) -> Option<NodeRoot> {
    host.mutate(|cx| {
        let document = document.get(cx.gc);
        cx.focused_element(document)
            .or_else(|| crate::geometry::body(cx, document))
            .or_else(|| crate::geometry::document_element(cx, document))
            .map(|node| cx.root(node))
    })
}

/// A trusted keyboard event of `event_type` (bubbling, cancelable,
/// composed) dispatched at `target`, its view the target's window, then a
/// microtask checkpoint; whether it wasn't canceled.
fn fire_key<H: ScriptHost>(
    host: &mut H,
    target: &NodeRoot,
    event_type: NameId,
    info: &KeyboardEventInfo,
) -> bool {
    let event = host.mutate(|cx| {
        let window = cx.target_window(TargetRef::Node(target.get(cx.gc)));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles: true,
                cancelable: true,
                composed: true,
                trusted: true,
            },
            window,
        );
        cx.set_keyboard_event_data(event, window, info);
        Persistent::new(cx.gc, event)
    });
    let not_canceled = dispatch(host, &TargetRoot::Node(target.clone()), &event);
    host.perform_microtask_checkpoint();
    not_canceled
}

/// The node of `document` the pointer over `deep` is over: `deep` if it is
/// `document`'s, else the iframe in `document` whose frame (or one below)
/// it is in; none if it is in no frame below.
fn node_in<H: ScriptHost>(host: &mut H, document: &NodeRoot, deep: &NodeRoot) -> Option<NodeRoot> {
    host.read(|cx| {
        let document = document.get(cx.gc);
        let mut node = deep.get(cx.gc);
        // A frame tree is fewer than u16::MAX deep.
        for _ in 0..u16::MAX {
            if Gc::ptr_eq(cx.document_of(node), document) {
                return Some(cx.root(node));
            }
            node = cx.frame_owner(cx.document_of(node))?;
        }
        None
    })
}

/// `point` (the top-level frame's viewport's, `top` its document) in the
/// viewport of the frame showing `document` (`FrameLayout::point_in_frame`);
/// none without a frame.
fn point_in_frame<H: ScriptHost>(
    host: &mut H,
    top: &NodeRoot,
    document: &NodeRoot,
    point: PointF,
) -> Option<PointF> {
    host.mutate(|cx| {
        let (top, document) = (top.get(cx.gc), document.get(cx.gc));
        if Gc::ptr_eq(top, document) {
            return Some(point);
        }
        let document = cx.node_id(document);
        cx.with_layout(top, |layout| layout.point_in_frame(document, point))
            .flatten()
    })
}

/// What the pointer is over at `point`: the frame's hit test through the
/// child frames (`geometry::deepest_hit_at`), and whether that is a run
/// of its text; none without a frame, or outside the viewport, unless
/// `dragging`: a button that went down in the frame is down (or going up),
/// and the frame has the pointer (the browser sends it the moves and the
/// release outside it), which go to what is laid out there.
fn mouse_target<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    point: PointF,
    dragging: bool,
) -> (Option<NodeRoot>, bool) {
    host.mutate(|cx| {
        let (element, text) =
            crate::geometry::deepest_hit_at(cx, document.get(cx.gc), point, !dragging);
        (element.map(|node| cx.root(node)), text)
    })
}

/// The cursor for the pointer over `over` (over a run of its text if
/// `text`), Blink's `EventHandler::SelectCursor`: its `cursor`'s keyword,
/// the one after its images (machopo loads none); `auto` the hand over a link
/// or in one (`UseHandCursor`), the I-beam over text that can start a
/// selection (`ShouldShowIBeamForNode`: its `user-select` isn't `none`),
/// else the arrow. The arrow over nothing or without a style.
fn select_cursor<H: ScriptHost>(
    host: &mut H,
    over: Option<&NodeRoot>,
    text: bool,
) -> CursorKeyword {
    let Some(over) = over else {
        return CursorKeyword::Default;
    };
    host.read(|cx| {
        let element = over.get(cx.gc);
        // Styled as the hit test laid it out.
        let Some(style) = cx
            .peek_layout(element, |layout| layout.computed_style(cx, element))
            .flatten()
        else {
            return CursorKeyword::Default;
        };
        match style.rare_inherited.cursor.keyword {
            CursorKeyword::Auto => {
                if std::iter::successors(Some(element), |&node| cx.flat_tree_parent(node))
                    .any(|node| crate::hyperlink::is_hyperlink(cx, node))
                {
                    CursorKeyword::Pointer
                } else if text && style.rare_inherited.user_select != UserSelect::None {
                    CursorKeyword::Text
                } else {
                    CursorKeyword::Default
                }
            }
            keyword => keyword,
        }
    })
}

/// A mouse event's data: `input`'s buttons, modifiers and point, `button`
/// and `detail`. The screen's coordinates are the viewport's (no window
/// position yet).
/// HTML's activation notification steps for user input at `target`, before
/// its event is dispatched: its document's window activated, and those of
/// the frames around it (`frame::notify_activation`).
fn notify_activation<H: ScriptHost>(host: &mut H, target: &NodeRoot) {
    host.mutate(|cx| crate::frame::notify_activation(cx, cx.document_of(target.get(cx.gc))));
}

/// A trusted mouse event of `event_type` at `target` with no position or
/// button (a select's legacy `mouseup` and `click` after its popup's pick).
pub(crate) fn fire_plain_mouse<H: ScriptHost>(host: &mut H, target: &NodeRoot, event_type: NameId) {
    fire_mouse(host, target, event_type, &MouseEventInfo::default(), None);
}

fn event_info(input: &MouseInput, button: i16, detail: i32) -> MouseEventInfo {
    MouseEventInfo {
        detail,
        client_x: f64::from(input.point.x),
        client_y: f64::from(input.point.y),
        screen_x: f64::from(input.point.x),
        screen_y: f64::from(input.point.y),
        movement_x: 0.0,
        movement_y: 0.0,
        button,
        buttons: input.buttons,
        modifiers: input.modifiers,
    }
}

/// The nearest node around both `a` and `b` (inclusive) in the flat tree,
/// if any (Blink's `FlatTreeTraversal::CommonAncestor`).
fn common_ancestor<'gc>(
    cx: &ApiCx<'_, 'gc>,
    a: NodeRef<'gc>,
    b: NodeRef<'gc>,
) -> Option<NodeRef<'gc>> {
    let around_a: Vec<NodeRef<'gc>> =
        std::iter::successors(Some(a), |&node| cx.flat_tree_parent(node)).collect();
    std::iter::successors(Some(b), |&node| cx.flat_tree_parent(node))
        .find(|&node| around_a.iter().any(|&other| Gc::ptr_eq(other, node)))
}

/// What the pointer leaves of the chain `old` was over: the deepest of
/// its nodes still in `document`, and whether that isn't the one it was
/// over (it was taken out: Blink sends no `mouseout` then).
fn exited<H: ScriptHost>(
    host: &mut H,
    document: &NodeRoot,
    old: &[NodeRoot],
) -> Option<(NodeRoot, bool)> {
    host.read(|cx| {
        let document = document.get(cx.gc);
        old.iter().enumerate().find_map(|(index, node)| {
            let at = node.get(cx.gc);
            (cx.is_connected(at) && Gc::ptr_eq(cx.document_of(at), document))
                .then(|| (node.clone(), index > 0))
        })
    })
}

/// The boundary events of the pointer leaving `exited` (and whether that
/// stands in for a node taken out) for `entered` (Blink's
/// `BoundaryEventDispatcher::SendBoundaryEvents`): `mouseout` at what it
/// left, `mouseleave` at it and each of its ancestors up to (not
/// including) the one they share with what it entered, child first,
/// `mouseover` at what it entered, then `mouseenter` at those of its
/// ancestors, parent first. Each related target is the other end.
fn send_boundary_events<H: ScriptHost>(
    host: &mut H,
    exited: Option<(NodeRoot, bool)>,
    entered: Option<&NodeRoot>,
    info: &MouseEventInfo,
) {
    let (exited, removed) = match exited {
        Some((node, removed)) => (Some(node), removed),
        None => (None, false),
    };
    let same = host.read(|cx| match (&exited, entered) {
        (Some(exited), Some(entered)) => Gc::ptr_eq(exited.get(cx.gc), entered.get(cx.gc)),
        (None, None) => true,
        _ => false,
    });
    if same && !removed {
        return;
    }
    let (exited_ancestors, entered_ancestors) = host.mutate(|cx| {
        let chain = |node: Option<&NodeRoot>| -> Vec<NodeRoot> {
            node.map_or_else(Vec::new, |node| {
                // The flat tree's, as Blink's boundary events' ancestors.
                std::iter::successors(Some(node.get(cx.gc)), |&at| cx.flat_tree_parent(at))
                    .map(|at| cx.root(at))
                    .collect()
            })
        };
        (chain(exited.as_ref()), chain(entered))
    });
    // What both chains end with: the ancestors the pointer stays in.
    let shared = host.read(|cx| {
        exited_ancestors
            .iter()
            .rev()
            .zip(entered_ancestors.iter().rev())
            .take_while(|(a, b)| Gc::ptr_eq(a.get(cx.gc), b.get(cx.gc)))
            .count()
    });
    let left = exited_ancestors.len().saturating_sub(shared);
    let came = entered_ancestors.len().saturating_sub(shared);
    if let Some(exited) = exited.as_ref().filter(|_| !removed) {
        fire_mouse(host, exited, event::MOUSEOUT, info, entered);
    }
    for node in &exited_ancestors[..left] {
        fire_mouse(host, node, event::MOUSELEAVE, info, entered);
    }
    if let Some(entered) = entered {
        fire_mouse(host, entered, event::MOUSEOVER, info, exited.as_ref());
    }
    for node in entered_ancestors[..came].iter().rev() {
        fire_mouse(host, node, event::MOUSEENTER, info, exited.as_ref());
    }
}

/// `contextmenu` at `target`, what the pointer is over (Blink's
/// `EventHandler::SendContextMenuEvent`); there is no menu for it to open.
fn context_menu<H: ScriptHost>(host: &mut H, target: &NodeRoot, input: &MouseInput) {
    fire_mouse(
        host,
        target,
        event::CONTEXTMENU,
        &event_info(input, MouseButton::Secondary.dom(), 0),
        None,
    );
}

/// A trusted mouse event of `event_type` dispatched at `target`, its view
/// the target's window (UI Events' flags: `mouseenter` and `mouseleave`
/// neither bubble, are canceled nor are composed), then a microtask
/// checkpoint; whether it wasn't canceled.
fn fire_mouse<H: ScriptHost>(
    host: &mut H,
    target: &NodeRoot,
    event_type: NameId,
    info: &MouseEventInfo,
    related: Option<&NodeRoot>,
) -> bool {
    dispatch_mouse(host, target, event_type, info, related).0
}

/// `fire_mouse`, with the event too, for a default action after it.
fn dispatch_mouse<H: ScriptHost>(
    host: &mut H,
    target: &NodeRoot,
    event_type: NameId,
    info: &MouseEventInfo,
    related: Option<&NodeRoot>,
) -> (bool, Persistent<Event>) {
    let boundary_only = matches!(event_type, event::MOUSEENTER | event::MOUSELEAVE);
    let event = host.mutate(|cx| {
        let node = target.get(cx.gc);
        let window = cx.target_window(TargetRef::Node(node));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles: !boundary_only,
                cancelable: !boundary_only,
                composed: !boundary_only,
                trusted: true,
            },
            window,
        );
        let related = related.map(|related| TargetRef::Node(related.get(cx.gc)));
        cx.set_mouse_event_data(event, window, info, related);
        Persistent::new(cx.gc, event)
    });
    let not_canceled = dispatch(host, &TargetRoot::Node(target.clone()), &event);
    host.perform_microtask_checkpoint();
    (not_canceled, event)
}
