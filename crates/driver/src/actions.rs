//! Perform Actions and Release Actions (chromedriver's `ExecutePerformActions`
//! and `ExecuteReleaseActions`, and its `key_converter` for the keys).
//!
//! The input state (the input sources by id, what each has pressed and
//! where a pointer is, and the input cancel list) is each top-level
//! window's, as the spec has it, unlike chromedriver's one per session:
//! the session keeps one per window handle and drops it with the window.
//!
//! The actions are first checked and put into ticks ([`process`]), which
//! creates the sources they name; then each tick's are dispatched in
//! turn ([`perform`]) as machopo's `Input.dispatchKeyEvent` and
//! `Input.dispatchMouseEvent` to the window's tab, each answered once the
//! renderer has handled it, and the tick waits out its longest duration.
//! A pointer move with a duration moves at once and then waits, as
//! chromedriver's does (no moves in between). machopo has a mouse only: `pen`
//! and `touch` pointers, and the mouse's back and forward buttons, are
//! `unsupported operation`.

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use base::numerics::f32_from_f64;
use icu_segmenter::GraphemeClusterSegmenter;
use protocol::automation::{self, DispatchKeyEvent, DispatchMouseEvent};
use webdriver::actions::{
    ActionSequence, ActionsType, GeneralAction, KeyAction, KeyActionItem, NullActionItem,
    PointerAction, PointerActionItem, PointerOrigin, PointerType, WheelAction, WheelActionItem,
};
use webdriver::error::ErrorStatus;

use crate::error::{Failure, Result};

/// Two presses of a button make a double click if they are this close in
/// time and place (chromedriver's, after the platforms').
const DOUBLE_CLICK_TIME: Duration = Duration::from_millis(500);
const DOUBLE_CLICK_RANGE: f64 = 4.0;

/// The spec's integers stop at JavaScript's maximum safe integer, which
/// the `webdriver` crate doesn't check.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// A top-level window's input state.
#[derive(Debug, Default)]
pub struct InputState {
    /// The input sources by id, in the order they were made.
    sources: Vec<(String, Source)>,
    /// What undoes each key and button pressed, in the order they were.
    cancel_list: Vec<(String, Action)>,
    /// Element Send Keys run with it, for their sources' ids.
    typed: u64,
}

#[derive(Debug, PartialEq)]
enum Source {
    Null,
    Key(KeyState),
    Pointer(PointerState),
    Wheel,
}

#[derive(Debug, Default, PartialEq)]
struct KeyState {
    /// The keys down, by their normalised key value.
    pressed: BTreeSet<String>,
    alt: bool,
    ctrl: bool,
    meta: bool,
    shift: bool,
}

#[derive(Debug, Default, PartialEq)]
struct PointerState {
    /// The buttons down, by WebDriver's number.
    pressed: BTreeSet<u64>,
    /// Where the pointer is, as the spec keeps it: in the viewport of the
    /// browsing context of the move that put it there, which the next
    /// `pointer` origin starts from.
    x: f64,
    y: f64,
    /// Where it is in the top-level viewport, where presses go.
    at: (f64, f64),
    /// The last press in this Perform Actions: its button, when (in the
    /// actions' own time, see `perform`), where in the top-level viewport,
    /// and its click count.
    last_press: Option<(u64, Duration, f64, f64, u32)>,
}

/// A checked action.
#[derive(Debug, Clone, PartialEq)]
enum Action {
    Pause,
    KeyDown(String),
    KeyUp(String),
    PointerDown(u64),
    PointerUp(u64),
    PointerMove {
        origin: PointerOrigin,
        x: f64,
        y: f64,
    },
    /// Nothing for a mouse to cancel.
    PointerCancel,
    Scroll {
        origin: PointerOrigin,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
    },
}

/// The actions of a tick, in their sequences' order, and how long it
/// lasts, in ms.
#[derive(Debug, PartialEq)]
pub struct Tick {
    actions: Vec<(String, Action)>,
    duration: u64,
}

/// What the actions are dispatched with: the window's tab, and the clock.
pub trait Input {
    fn key(&mut self, event: DispatchKeyEvent) -> Result<()>;
    fn mouse(&mut self, event: DispatchMouseEvent) -> Result<()>;
    /// The element's in-view center point, in the CSS px of the current
    /// browsing context's viewport (outside it if the element is).
    fn element_point(&mut self, element: &str) -> Result<(f64, f64)>;
    /// The current browsing context's viewport.
    fn viewport(&mut self) -> Result<Viewport>;
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

/// A browsing context's viewport, CSS px: where it is in the top-level
/// one, where the events go, and its size. The actions' coordinates are
/// in it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::new(ErrorStatus::InvalidArgument, message)
}

fn unsupported(message: impl Into<String>) -> Failure {
    Failure::new(ErrorStatus::UnsupportedOperation, message)
}

/// The spec's "extract an action sequence": the actions checked and put
/// into ticks, the sources they name made in `state` (as the spec makes
/// them, even if a later sequence is wrong).
pub fn process(sequences: Vec<ActionSequence>, state: &mut InputState) -> Result<Vec<Tick>> {
    let mut by_source = Vec::new();
    for sequence in sequences {
        let id = sequence.id;
        if by_source.iter().any(|(known, _)| *known == id) {
            return Err(invalid(format!("two action sequences for {id:?}")));
        }
        let actions = match sequence.actions {
            ActionsType::Null { actions } => {
                state.source(&id, Source::Null)?;
                actions
                    .into_iter()
                    .map(|NullActionItem::General(general)| general_action(general))
                    .collect::<Result<Vec<_>>>()?
            }
            ActionsType::Key { actions } => {
                state.source(&id, Source::Key(KeyState::default()))?;
                actions
                    .into_iter()
                    .map(|item| match item {
                        KeyActionItem::General(general) => general_action(general),
                        KeyActionItem::Key(KeyAction::Down(down)) => {
                            Ok((Action::KeyDown(down.value), None))
                        }
                        KeyActionItem::Key(KeyAction::Up(up)) => {
                            Ok((Action::KeyUp(up.value), None))
                        }
                    })
                    .collect::<Result<Vec<_>>>()?
            }
            ActionsType::Pointer {
                parameters,
                actions,
            } => {
                if parameters.pointer_type != PointerType::Mouse {
                    return Err(unsupported(format!(
                        "machopo has a mouse only, not a {:?} pointer",
                        parameters.pointer_type
                    )));
                }
                state.source(&id, Source::Pointer(PointerState::default()))?;
                actions
                    .into_iter()
                    .map(pointer_action)
                    .collect::<Result<Vec<_>>>()?
            }
            ActionsType::Wheel { actions } => {
                state.source(&id, Source::Wheel)?;
                actions
                    .into_iter()
                    .map(wheel_action)
                    .collect::<Result<Vec<_>>>()?
            }
        };
        by_source.push((id, actions));
    }
    let ticks = by_source
        .iter()
        .map(|(_, actions)| actions.len())
        .max()
        .unwrap_or(0);
    Ok((0..ticks)
        .map(|i| {
            let mut tick = Tick {
                actions: Vec::new(),
                duration: 0,
            };
            for (id, actions) in &by_source {
                if let Some((action, duration)) = actions.get(i) {
                    tick.actions.push((id.clone(), action.clone()));
                    tick.duration = tick.duration.max(duration.unwrap_or(0));
                }
            }
            tick
        })
        .collect())
}

fn general_action(GeneralAction::Pause(pause): GeneralAction) -> Result<(Action, Option<u64>)> {
    Ok((Action::Pause, duration(pause.duration)?))
}

/// An action's duration, if given: `invalid argument` past the maximum
/// safe integer (else a tick would wait for ages).
fn duration(duration: Option<u64>) -> Result<Option<u64>> {
    safe(duration, "duration")?;
    Ok(duration)
}

fn safe(value: Option<u64>, name: &str) -> Result<()> {
    match value {
        Some(value) if value > MAX_SAFE_INTEGER => Err(invalid(format!(
            "{name} {value} is past the maximum safe integer"
        ))),
        _ => Ok(()),
    }
}

fn pointer_action(item: PointerActionItem) -> Result<(Action, Option<u64>)> {
    let button = |button: u64| match button {
        0..=2 => Ok(button),
        3 | 4 => Err(unsupported(format!(
            "machopo's mouse has no button {button}"
        ))),
        _ => Err(invalid(format!("no mouse button {button}"))),
    };
    let size = |width: Option<u64>, height: Option<u64>| {
        safe(width, "width")?;
        safe(height, "height")
    };
    Ok(match item {
        PointerActionItem::General(general) => general_action(general)?,
        PointerActionItem::Pointer(PointerAction::Down(down)) => {
            size(down.width, down.height)?;
            (Action::PointerDown(button(down.button)?), None)
        }
        PointerActionItem::Pointer(PointerAction::Up(up)) => {
            size(up.width, up.height)?;
            (Action::PointerUp(button(up.button)?), None)
        }
        PointerActionItem::Pointer(PointerAction::Move(motion)) => {
            size(motion.width, motion.height)?;
            (
                Action::PointerMove {
                    origin: motion.origin,
                    x: motion.x,
                    y: motion.y,
                },
                duration(motion.duration)?,
            )
        }
        PointerActionItem::Pointer(PointerAction::Cancel) => (Action::PointerCancel, None),
    })
}

fn wheel_action(item: WheelActionItem) -> Result<(Action, Option<u64>)> {
    Ok(match item {
        WheelActionItem::General(general) => general_action(general)?,
        WheelActionItem::Wheel(WheelAction::Scroll(scroll)) => {
            if scroll.origin == PointerOrigin::Pointer {
                return Err(invalid("a scroll's origin is the viewport or an element"));
            }
            let given = |value: Option<i64>, name: &str| match value {
                None => Err(invalid(format!("a scroll needs its {name}"))),
                Some(value) => {
                    safe(Some(value.unsigned_abs()), name)?;
                    Ok(value as f64)
                }
            };
            (
                Action::Scroll {
                    x: given(scroll.x, "x")?,
                    y: given(scroll.y, "y")?,
                    delta_x: given(scroll.deltaX, "deltaX")?,
                    delta_y: given(scroll.deltaY, "deltaY")?,
                    origin: scroll.origin,
                },
                duration(scroll.duration)?,
            )
        }
    })
}

impl InputState {
    /// The spec's "get or create an input source": `invalid argument` if
    /// `id` is a source of another type (another pointer type, for a
    /// pointer).
    fn source(&mut self, id: &str, new: Source) -> Result<()> {
        match self.sources.iter().find(|(known, _)| known == id) {
            None => {
                self.sources.push((id.to_owned(), new));
                Ok(())
            }
            Some((_, source)) if std::mem::discriminant(source) == std::mem::discriminant(&new) => {
                Ok(())
            }
            Some(_) => Err(invalid(format!(
                "{id:?} is an input source of another type"
            ))),
        }
    }

    fn get_mut(&mut self, id: &str) -> &mut Source {
        &mut self
            .sources
            .iter_mut()
            .find(|(known, _)| known == id)
            .expect("made when processed")
            .1
    }

    /// The modifier keys held by any key source, as CDP's bits.
    fn modifiers(&self) -> u8 {
        let mut bits = 0;
        for (_, source) in &self.sources {
            if let Source::Key(keys) = source {
                for (held, bit) in [
                    (keys.alt, automation::ALT),
                    (keys.ctrl, automation::CTRL),
                    (keys.meta, automation::META),
                    (keys.shift, automation::SHIFT),
                ] {
                    if held {
                        bits |= bit;
                    }
                }
            }
        }
        bits
    }

    /// The DOM's bits of the buttons down on any pointer.
    fn buttons(&self) -> u16 {
        let mut bits = 0;
        for (_, source) in &self.sources {
            if let Source::Pointer(pointer) = source {
                for button in &pointer.pressed {
                    bits |= buttons_bit(*button);
                }
            }
        }
        bits
    }
}

/// The spec's "dispatch actions": each tick's actions in turn, each tick
/// lasting its duration at least.
/// A double click is two presses in one Perform Actions: the spec doesn't
/// say yet (w3c/webdriver#1772), and WPT's tentative test has the clicks
/// of two apart as two single ones.
///
/// How far apart two presses are is in the actions' own time, the
/// durations of the ticks between them, not the clock's: each event waits
/// for the renderer's ack, so a busy renderer would stretch two presses
/// in a row past the double click time. chromedriver reads the clock, but
/// doesn't wait for the events (but the last), so its presses are as far
/// apart as its ticks' sleeps too.
pub fn perform(ticks: &[Tick], state: &mut InputState, input: &mut dyn Input) -> Result<()> {
    for (_, source) in &mut state.sources {
        if let Source::Pointer(pointer) = source {
            pointer.last_press = None;
        }
    }
    let mut viewport = None;
    let mut at = Duration::ZERO;
    for tick in ticks {
        let start = input.now();
        dispatch_tick(&tick.actions, at, state, input, &mut viewport)?;
        let lasts = Duration::from_millis(tick.duration);
        // The actions' own clock: each tick's duration is at most
        // JavaScript's largest safe integer of ms, so only ticks past any
        // wait could saturate it.
        at = at.saturating_add(lasts);
        let past = input.now().saturating_duration_since(start);
        if let Some(left) = lasts.checked_sub(past).filter(|left| !left.is_zero()) {
            input.sleep(left);
        }
    }
    Ok(())
}

/// Release Actions: what the input cancel list undoes, last first; the
/// state is then the caller's to drop.
pub fn release(state: &mut InputState, input: &mut dyn Input) -> Result<()> {
    let undo: Vec<_> = state.cancel_list.iter().rev().cloned().collect();
    // Only releases: no press for the time to matter to.
    dispatch_tick(&undo, Duration::ZERO, state, input, &mut None)
}

/// Element Send Keys' keys (WebDriver's "dispatch actions for a string"),
/// from a key input source of their own in the window's state, which the
/// keys of other sources held count with, removed after: `text` by
/// grapheme cluster; `\u{E000}` (NULL) lets go of the modifiers it
/// pressed; a modifier key goes down until then; a character goes down
/// and up, Shift pressed first for one the US keyboard types with Shift
/// (and let go of for one it doesn't). A cluster of more than one code
/// point is typed as one key: machopo has no composition events, which
/// WebDriver would send. What is still pressed at the end is let go of.
pub fn send_keys(text: &str, state: &mut InputState, input: &mut dyn Input) -> Result<()> {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "one per Element Send Keys of the session: a u64 lasts"
    )]
    {
        state.typed += 1;
    }
    let id = format!("Element Send Keys {}", state.typed);
    state
        .sources
        .push((id.clone(), Source::Key(KeyState::default())));
    let mut held = Vec::new();
    let typed = type_string(&id, text, &mut held, state, input);
    let released = release_keys(&id, &mut held, state, input);
    state.sources.retain(|(known, _)| *known != id);
    typed.and(released)
}

/// `text`'s keys from the source `id`, the modifiers it presses kept in
/// `held` (WebDriver's undo actions), Shift too while it is pressed for a
/// character.
fn type_string(
    id: &str,
    text: &str,
    held: &mut Vec<String>,
    state: &mut InputState,
    input: &mut dyn Input,
) -> Result<()> {
    let boundaries: Vec<usize> = GraphemeClusterSegmenter::new().segment_str(text).collect();
    for cluster in boundaries.windows(2).map(|pair| &text[pair[0]..pair[1]]) {
        if cluster == "\u{E000}" {
            release_keys(id, held, state, input)?;
            continue;
        }
        if is_modifier_key(cluster) {
            key_down(id, cluster, state, input)?;
            if !held.iter().any(|key| key == cluster) {
                held.push(cluster.to_owned());
            }
            continue;
        }
        if let Some(c) = single(cluster) {
            let Source::Key(keys) = state.get_mut(id) else {
                unreachable!("a key source");
            };
            let shift = "\u{E008}";
            if is_shifted(c) && !keys.shift {
                key_down(id, shift, state, input)?;
                held.push(shift.to_owned());
            } else if !is_shifted(c) && keys.shift {
                key_up(id, shift, state, input)?;
                held.retain(|key| key != shift);
            }
        }
        key_down(id, cluster, state, input)?;
        key_up(id, cluster, state, input)?;
    }
    Ok(())
}

/// Lets go of the keys in `held`, in the order of their code points (as
/// WebDriver sorts its undo actions).
fn release_keys(
    id: &str,
    held: &mut Vec<String>,
    state: &mut InputState,
    input: &mut dyn Input,
) -> Result<()> {
    held.sort();
    for key in std::mem::take(held) {
        key_up(id, &key, state, input)?;
    }
    Ok(())
}

/// Shift, Control, Alt and Meta, either side's.
fn is_modifier_key(raw: &str) -> bool {
    matches!(
        single(raw),
        Some('\u{E008}' | '\u{E009}' | '\u{E00A}' | '\u{E03D}' | '\u{E050}'..='\u{E053}')
    )
}

/// WebDriver's shifted character: the US keyboard types it with Shift
/// (the second column of the spec's table of keys and codes).
fn is_shifted(c: char) -> bool {
    c.is_ascii_uppercase() || "~|{}<)!@#$%^&*(+>_\":?".contains(c)
}

/// `at`: when the tick starts, in the actions' own time.
fn dispatch_tick(
    actions: &[(String, Action)],
    at: Duration,
    state: &mut InputState,
    input: &mut dyn Input,
    viewport: &mut Option<Viewport>,
) -> Result<()> {
    for (id, action) in actions {
        match action {
            Action::Pause | Action::PointerCancel => {}
            Action::KeyDown(raw) => {
                state
                    .cancel_list
                    .push((id.clone(), Action::KeyUp(raw.clone())));
                key_down(id, raw, state, input)?;
            }
            Action::KeyUp(raw) => key_up(id, raw, state, input)?,
            Action::PointerDown(button) => {
                state
                    .cancel_list
                    .push((id.clone(), Action::PointerUp(*button)));
                pointer_down(id, *button, at, state, input)?;
            }
            Action::PointerUp(button) => pointer_up(id, *button, state, input)?,
            Action::PointerMove { origin, x, y } => {
                let Source::Pointer(pointer) = state.get_mut(id) else {
                    unreachable!("a pointer's action");
                };
                let from = (pointer.x, pointer.y);
                let ((x, y), at) = target(origin, *x, *y, from, input, viewport)?;
                let Source::Pointer(pointer) = state.get_mut(id) else {
                    unreachable!("a pointer's action");
                };
                (pointer.x, pointer.y, pointer.at) = (x, y, at);
                let (x, y) = at;
                input.mouse(DispatchMouseEvent {
                    kind: "mouseMoved".into(),
                    x: f32_from_f64(x),
                    y: f32_from_f64(y),
                    buttons: state.buttons(),
                    modifiers: state.modifiers(),
                    ..DispatchMouseEvent::default()
                })?;
            }
            Action::Scroll {
                origin,
                x,
                y,
                delta_x,
                delta_y,
            } => {
                let (_, (x, y)) = target(origin, *x, *y, (0.0, 0.0), input, viewport)?;
                input.mouse(DispatchMouseEvent {
                    kind: "mouseWheel".into(),
                    x: f32_from_f64(x),
                    y: f32_from_f64(y),
                    buttons: state.buttons(),
                    modifiers: state.modifiers(),
                    delta_x: f32_from_f64(*delta_x),
                    delta_y: f32_from_f64(*delta_y),
                    ..DispatchMouseEvent::default()
                })?;
            }
        }
    }
    Ok(())
}

/// Where a move or a scroll goes, in the current browsing context's
/// viewport and in the top-level one: the offset from its origin (the
/// viewport's corner, the pointer's place `from`, or an element's in-view
/// center point); `move target out of bounds` outside the viewport.
fn target(
    origin: &PointerOrigin,
    x: f64,
    y: f64,
    from: (f64, f64),
    input: &mut dyn Input,
    viewport: &mut Option<Viewport>,
) -> Result<((f64, f64), (f64, f64))> {
    let port = match *viewport {
        Some(port) => port,
        None => *viewport.insert(input.viewport()?),
    };
    let (left, top) = match origin {
        PointerOrigin::Viewport => (0.0, 0.0),
        PointerOrigin::Pointer => from,
        PointerOrigin::Element(element) => input.element_point(&element.0)?,
    };
    let (x, y) = (left + x, top + y);
    if !(0.0..=port.width).contains(&x) || !(0.0..=port.height).contains(&y) {
        return Err(Failure::new(
            ErrorStatus::MoveTargetOutOfBounds,
            format!(
                "({x}, {y}) is outside the viewport ({} by {})",
                port.width, port.height
            ),
        ));
    }
    Ok(((x, y), (port.x + x, port.y + y)))
}

fn key_down(id: &str, raw: &str, state: &mut InputState, input: &mut dyn Input) -> Result<()> {
    let key = normalised_key(raw);
    let Source::Key(keys) = state.get_mut(id) else {
        unreachable!("a key source's action");
    };
    let repeat = !keys.pressed.insert(key.to_owned());
    match key {
        "Alt" => keys.alt = true,
        "Control" => keys.ctrl = true,
        "Meta" => keys.meta = true,
        "Shift" => keys.shift = true,
        _ => {}
    }
    let text = key_text(raw, key);
    input.key(DispatchKeyEvent {
        kind: if text.is_empty() {
            "rawKeyDown"
        } else {
            "keyDown"
        }
        .into(),
        modifiers: state.modifiers(),
        text,
        key: key.to_owned(),
        code: key_code(raw).to_owned(),
        windows_virtual_key_code: legacy_key_code(raw, key),
        location: key_location(raw),
        auto_repeat: repeat,
    })
}

fn key_up(id: &str, raw: &str, state: &mut InputState, input: &mut dyn Input) -> Result<()> {
    let key = normalised_key(raw);
    let Source::Key(keys) = state.get_mut(id) else {
        unreachable!("a key source's action");
    };
    if !keys.pressed.remove(key) {
        return Ok(());
    }
    match key {
        "Alt" => keys.alt = false,
        "Control" => keys.ctrl = false,
        "Meta" => keys.meta = false,
        "Shift" => keys.shift = false,
        _ => {}
    }
    input.key(DispatchKeyEvent {
        kind: "keyUp".into(),
        modifiers: state.modifiers(),
        key: key.to_owned(),
        code: key_code(raw).to_owned(),
        windows_virtual_key_code: legacy_key_code(raw, key),
        location: key_location(raw),
        ..DispatchKeyEvent::default()
    })
}

fn pointer_down(
    id: &str,
    button: u64,
    now: Duration,
    state: &mut InputState,
    input: &mut dyn Input,
) -> Result<()> {
    let Source::Pointer(pointer) = state.get_mut(id) else {
        unreachable!("a pointer's action");
    };
    if !pointer.pressed.insert(button) {
        return Ok(());
    }
    let (x, y) = pointer.at;
    let click_count = match pointer.last_press {
        Some((last, at, last_x, last_y, count))
            if last == button
                && now.saturating_sub(at) < DOUBLE_CLICK_TIME
                && (x - last_x).abs() <= DOUBLE_CLICK_RANGE
                && (y - last_y).abs() <= DOUBLE_CLICK_RANGE =>
        {
            // One per press of this Perform Actions, its request's actions:
            // far fewer than u32::MAX, where it would stay.
            count.saturating_add(1)
        }
        _ => 1,
    };
    pointer.last_press = Some((button, now, x, y, click_count));
    input.mouse(DispatchMouseEvent {
        kind: "mousePressed".into(),
        x: f32_from_f64(x),
        y: f32_from_f64(y),
        button: Some(button_name(button).into()),
        buttons: state.buttons(),
        click_count,
        modifiers: state.modifiers(),
        ..DispatchMouseEvent::default()
    })
}

fn pointer_up(id: &str, button: u64, state: &mut InputState, input: &mut dyn Input) -> Result<()> {
    let Source::Pointer(pointer) = state.get_mut(id) else {
        unreachable!("a pointer's action");
    };
    if !pointer.pressed.remove(&button) {
        return Ok(());
    }
    let (x, y) = pointer.at;
    // Its press's.
    let click_count = pointer
        .last_press
        .filter(|(last, ..)| *last == button)
        .map_or(1, |(.., count)| count);
    input.mouse(DispatchMouseEvent {
        kind: "mouseReleased".into(),
        x: f32_from_f64(x),
        y: f32_from_f64(y),
        button: Some(button_name(button).into()),
        buttons: state.buttons(),
        click_count,
        modifiers: state.modifiers(),
        ..DispatchMouseEvent::default()
    })
}

/// WebDriver's button numbers (as the DOM's `button`) as CDP's names.
fn button_name(button: u64) -> &'static str {
    match button {
        0 => "left",
        1 => "middle",
        _ => "right",
    }
}

/// The DOM's `buttons` bit of WebDriver's button: the middle and right
/// ones' are swapped.
fn buttons_bit(button: u64) -> u16 {
    match button {
        0 => 1,
        1 => 4,
        _ => 2,
    }
}

/// The spec's "normalised key value": a WebDriver key's DOM `key`, the
/// raw key itself if it isn't one.
fn normalised_key(raw: &str) -> &str {
    let Some(c) = single(raw) else {
        return raw;
    };
    match c {
        '\u{E000}' => "Unidentified",
        '\u{E001}' => "Cancel",
        '\u{E002}' => "Help",
        '\u{E003}' => "Backspace",
        '\u{E004}' => "Tab",
        '\u{E005}' => "Clear",
        '\u{E006}' | '\u{E007}' => "Enter",
        '\u{E008}' | '\u{E050}' => "Shift",
        '\u{E009}' | '\u{E051}' => "Control",
        '\u{E00A}' | '\u{E052}' => "Alt",
        '\u{E00B}' => "Pause",
        '\u{E00C}' => "Escape",
        '\u{E00D}' => " ",
        '\u{E00E}' | '\u{E054}' => "PageUp",
        '\u{E00F}' | '\u{E055}' => "PageDown",
        '\u{E010}' | '\u{E056}' => "End",
        '\u{E011}' | '\u{E057}' => "Home",
        '\u{E012}' | '\u{E058}' => "ArrowLeft",
        '\u{E013}' | '\u{E059}' => "ArrowUp",
        '\u{E014}' | '\u{E05A}' => "ArrowRight",
        '\u{E015}' | '\u{E05B}' => "ArrowDown",
        '\u{E016}' | '\u{E05C}' => "Insert",
        '\u{E017}' | '\u{E05D}' => "Delete",
        '\u{E018}' => ";",
        '\u{E019}' => "=",
        '\u{E01A}' => "0",
        '\u{E01B}' => "1",
        '\u{E01C}' => "2",
        '\u{E01D}' => "3",
        '\u{E01E}' => "4",
        '\u{E01F}' => "5",
        '\u{E020}' => "6",
        '\u{E021}' => "7",
        '\u{E022}' => "8",
        '\u{E023}' => "9",
        '\u{E024}' => "*",
        '\u{E025}' => "+",
        '\u{E026}' => ",",
        '\u{E027}' => "-",
        '\u{E028}' => ".",
        '\u{E029}' => "/",
        '\u{E031}' => "F1",
        '\u{E032}' => "F2",
        '\u{E033}' => "F3",
        '\u{E034}' => "F4",
        '\u{E035}' => "F5",
        '\u{E036}' => "F6",
        '\u{E037}' => "F7",
        '\u{E038}' => "F8",
        '\u{E039}' => "F9",
        '\u{E03A}' => "F10",
        '\u{E03B}' => "F11",
        '\u{E03C}' => "F12",
        '\u{E03D}' | '\u{E053}' => "Meta",
        '\u{E040}' => "ZenkakuHankaku",
        _ => raw,
    }
}

/// The spec's "code for key": the US keyboard's key a WebDriver key is
/// (shifted or not), `""` if none.
fn key_code(raw: &str) -> &'static str {
    let Some(c) = single(raw) else {
        return "";
    };
    match c {
        '`' | '~' => "Backquote",
        '\\' | '|' => "Backslash",
        '\u{E003}' => "Backspace",
        '[' | '{' => "BracketLeft",
        ']' | '}' => "BracketRight",
        ',' | '<' => "Comma",
        '0' | ')' => "Digit0",
        '1' | '!' => "Digit1",
        '2' | '@' => "Digit2",
        '3' | '#' => "Digit3",
        '4' | '$' => "Digit4",
        '5' | '%' => "Digit5",
        '6' | '^' => "Digit6",
        '7' | '&' => "Digit7",
        '8' | '*' => "Digit8",
        '9' | '(' => "Digit9",
        '=' | '+' => "Equal",
        'a' | 'A' => "KeyA",
        'b' | 'B' => "KeyB",
        'c' | 'C' => "KeyC",
        'd' | 'D' => "KeyD",
        'e' | 'E' => "KeyE",
        'f' | 'F' => "KeyF",
        'g' | 'G' => "KeyG",
        'h' | 'H' => "KeyH",
        'i' | 'I' => "KeyI",
        'j' | 'J' => "KeyJ",
        'k' | 'K' => "KeyK",
        'l' | 'L' => "KeyL",
        'm' | 'M' => "KeyM",
        'n' | 'N' => "KeyN",
        'o' | 'O' => "KeyO",
        'p' | 'P' => "KeyP",
        'q' | 'Q' => "KeyQ",
        'r' | 'R' => "KeyR",
        's' | 'S' => "KeyS",
        't' | 'T' => "KeyT",
        'u' | 'U' => "KeyU",
        'v' | 'V' => "KeyV",
        'w' | 'W' => "KeyW",
        'x' | 'X' => "KeyX",
        'y' | 'Y' => "KeyY",
        'z' | 'Z' => "KeyZ",
        '-' | '_' => "Minus",
        '.' | '>' => "Period",
        '\'' | '"' => "Quote",
        ';' | ':' => "Semicolon",
        '/' | '?' => "Slash",
        '\u{E00A}' => "AltLeft",
        '\u{E052}' => "AltRight",
        '\u{E009}' => "ControlLeft",
        '\u{E051}' => "ControlRight",
        '\u{E006}' => "Enter",
        '\u{E00B}' => "Pause",
        '\u{E03D}' => "MetaLeft",
        '\u{E053}' => "MetaRight",
        '\u{E008}' => "ShiftLeft",
        '\u{E050}' => "ShiftRight",
        ' ' | '\u{E00D}' => "Space",
        '\u{E004}' => "Tab",
        '\u{E017}' => "Delete",
        '\u{E010}' => "End",
        '\u{E002}' => "Help",
        '\u{E011}' => "Home",
        '\u{E016}' => "Insert",
        '\u{E00F}' => "PageDown",
        '\u{E00E}' => "PageUp",
        '\u{E015}' => "ArrowDown",
        '\u{E012}' => "ArrowLeft",
        '\u{E014}' => "ArrowRight",
        '\u{E013}' => "ArrowUp",
        '\u{E00C}' => "Escape",
        '\u{E031}' => "F1",
        '\u{E032}' => "F2",
        '\u{E033}' => "F3",
        '\u{E034}' => "F4",
        '\u{E035}' => "F5",
        '\u{E036}' => "F6",
        '\u{E037}' => "F7",
        '\u{E038}' => "F8",
        '\u{E039}' => "F9",
        '\u{E03A}' => "F10",
        '\u{E03B}' => "F11",
        '\u{E03C}' => "F12",
        '\u{E019}' => "NumpadEqual",
        '\u{E01A}' | '\u{E05C}' => "Numpad0",
        '\u{E01B}' | '\u{E056}' => "Numpad1",
        '\u{E01C}' | '\u{E05B}' => "Numpad2",
        '\u{E01D}' | '\u{E055}' => "Numpad3",
        '\u{E01E}' | '\u{E058}' => "Numpad4",
        '\u{E01F}' => "Numpad5",
        '\u{E020}' | '\u{E05A}' => "Numpad6",
        '\u{E021}' | '\u{E057}' => "Numpad7",
        '\u{E022}' | '\u{E059}' => "Numpad8",
        '\u{E023}' | '\u{E054}' => "Numpad9",
        '\u{E025}' => "NumpadAdd",
        '\u{E026}' => "NumpadComma",
        '\u{E028}' | '\u{E05D}' => "NumpadDecimal",
        '\u{E029}' => "NumpadDivide",
        '\u{E007}' => "NumpadEnter",
        '\u{E024}' => "NumpadMultiply",
        '\u{E027}' => "NumpadSubtract",
        _ => "",
    }
}

/// The spec's "key location": 1 left, 2 right, 3 numpad, 0 otherwise.
fn key_location(raw: &str) -> u8 {
    match single(raw) {
        Some('\u{E007}' | '\u{E008}' | '\u{E009}' | '\u{E00A}' | '\u{E03D}') => 1,
        Some('\u{E050}'..='\u{E053}') => 2,
        Some('\u{E019}'..='\u{E029}' | '\u{E054}'..='\u{E05D}') => 3,
        _ => 0,
    }
}

/// The legacy `keyCode` (Windows' virtual key code, which Chromium's
/// `windowsVirtualKeyCode` is): the spec has none. A named key's, a
/// numpad key's (by what it does: with Num Lock off for the right-hand
/// ones), else that of the US keyboard's key the character is on; 0 for
/// a character on none.
fn legacy_key_code(raw: &str, key: &str) -> u16 {
    let named = match key {
        "Cancel" => 3,
        "Backspace" => 8,
        "Tab" => 9,
        "Clear" => 12,
        "Enter" => 13,
        "Shift" => 16,
        "Control" => 17,
        "Alt" => 18,
        "Pause" => 19,
        "Escape" => 27,
        "PageUp" => 33,
        "PageDown" => 34,
        "End" => 35,
        "Home" => 36,
        "ArrowLeft" => 37,
        "ArrowUp" => 38,
        "ArrowRight" => 39,
        "ArrowDown" => 40,
        "Insert" => 45,
        "Delete" => 46,
        "Help" => 47,
        "F1" => 112,
        "F2" => 113,
        "F3" => 114,
        "F4" => 115,
        "F5" => 116,
        "F6" => 117,
        "F7" => 118,
        "F8" => 119,
        "F9" => 120,
        "F10" => 121,
        "F11" => 122,
        "F12" => 123,
        "Meta" if key_location(raw) == 2 => 92,
        "Meta" => 91,
        _ => 0,
    };
    if named != 0 {
        return named;
    }
    let code = key_code(raw);
    if key_location(raw) == 3 {
        return match key {
            "*" => 106,
            "+" => 107,
            "," => 108,
            "-" => 109,
            "." => 110,
            "/" => 111,
            "=" => 187,
            // A numpad digit's `key` is one digit: the sum is 96 to 105.
            digit => digit
                .parse::<u16>()
                .ok()
                .and_then(|n| n.checked_add(96))
                .unwrap_or(0),
        };
    }
    if let Some(letter) = code.strip_prefix("Key") {
        return u16::from(letter.as_bytes()[0]);
    }
    if let Some(digit) = code.strip_prefix("Digit") {
        return u16::from(digit.as_bytes()[0]);
    }
    match code {
        "Space" => 32,
        "Semicolon" => 186,
        "Equal" => 187,
        "Comma" => 188,
        "Minus" => 189,
        "Period" => 190,
        "Slash" => 191,
        "Backquote" => 192,
        "BracketLeft" => 219,
        "Backslash" => 220,
        "BracketRight" => 221,
        "Quote" => 222,
        _ => 0,
    }
}

/// What a key down types: Enter's carriage return; the character of a
/// WebDriver key that is one (`" "`, the numpad's), not a named key's; a
/// key that isn't a WebDriver key, unless it is a control character or
/// one of the code points WebDriver keeps for its keys. The renderer
/// leaves out what a shortcut's would type.
fn key_text(raw: &str, key: &str) -> String {
    if key == "Enter" {
        return "\r".into();
    }
    if key != raw {
        return match single(key) {
            Some(_) => key.to_owned(),
            None => String::new(),
        };
    }
    let reserved = single(raw).is_some_and(|c| ('\u{E000}'..='\u{E05D}').contains(&c));
    if reserved || raw.chars().any(char::is_control) {
        return String::new();
    }
    raw.to_owned()
}

/// The one character `raw` is, if it is one.
fn single(raw: &str) -> Option<char> {
    let mut chars = raw.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use webdriver::command::ActionsParameters;

    /// A window on a clock the test moves, recording what is dispatched.
    struct Recorder {
        events: Vec<String>,
        now: Instant,
        slept: Duration,
        /// How long the renderer takes to answer each mouse event.
        mouse_takes: Duration,
        viewport: Viewport,
    }

    impl Recorder {
        fn new() -> Self {
            Self {
                events: Vec::new(),
                now: Instant::now(),
                slept: Duration::ZERO,
                mouse_takes: Duration::ZERO,
                viewport: Viewport {
                    x: 0.0,
                    y: 0.0,
                    width: 800.0,
                    height: 600.0,
                },
            }
        }

        fn take(&mut self) -> Vec<String> {
            std::mem::take(&mut self.events)
        }
    }

    impl Input for Recorder {
        fn key(&mut self, e: DispatchKeyEvent) -> Result<()> {
            self.events.push(format!(
                "{} {:?} {} {} {} {:?} m{}{}",
                e.kind,
                e.key,
                e.code,
                e.windows_virtual_key_code,
                e.location,
                e.text,
                e.modifiers,
                if e.auto_repeat { " repeat" } else { "" }
            ));
            Ok(())
        }

        fn mouse(&mut self, e: DispatchMouseEvent) -> Result<()> {
            self.events.push(format!(
                "{} {},{} {} b{} c{} m{} d{},{}",
                e.kind,
                e.x,
                e.y,
                e.button.as_deref().unwrap_or("-"),
                e.buttons,
                e.click_count,
                e.modifiers,
                e.delta_x,
                e.delta_y
            ));
            self.now += self.mouse_takes;
            Ok(())
        }

        fn element_point(&mut self, element: &str) -> Result<(f64, f64)> {
            self.events.push(format!("point of {element}"));
            Ok((40.0, 30.0))
        }

        fn viewport(&mut self) -> Result<Viewport> {
            self.events.push("viewport".into());
            Ok(self.viewport)
        }

        fn now(&self) -> Instant {
            self.now
        }

        fn sleep(&mut self, duration: Duration) {
            self.now += duration;
            self.slept += duration;
        }
    }

    fn sequences(actions: Value) -> Vec<ActionSequence> {
        serde_json::from_value::<ActionsParameters>(json!({ "actions": actions }))
            .expect("the crate takes them")
            .actions
    }

    fn run(actions: Value, state: &mut InputState, input: &mut Recorder) -> Result<()> {
        let ticks = process(sequences(actions), state)?;
        perform(&ticks, state, input)
    }

    fn keys(actions: Value) -> Value {
        json!([{ "type": "key", "id": "k", "actions": actions }])
    }

    fn mouse(actions: Value) -> Value {
        json!([{ "type": "pointer", "id": "m", "actions": actions }])
    }

    fn error_of(result: Result<()>) -> String {
        match result {
            Ok(()) => "none".into(),
            Err(failure) => failure.error.into_owned(),
        }
    }

    /// The spec's key tables, and machopo's `keyCode` and text.
    #[test]
    fn keys_are_the_spec_tables() {
        let row = |raw: &str| {
            let key = normalised_key(raw);
            (
                key.to_owned(),
                key_code(raw),
                key_location(raw),
                legacy_key_code(raw, key),
                key_text(raw, key),
            )
        };
        let r = |key: &str, code: &'static str, location, key_code, text: &str| {
            (key.to_owned(), code, location, key_code, text.to_owned())
        };
        assert_eq!(row("a"), r("a", "KeyA", 0, 65, "a"));
        assert_eq!(row("Q"), r("Q", "KeyQ", 0, 81, "Q"));
        assert_eq!(row("@"), r("@", "Digit2", 0, 50, "@"));
        assert_eq!(row("\""), r("\"", "Quote", 0, 222, "\""));
        assert_eq!(row("à"), r("à", "", 0, 0, "à"));
        assert_eq!(row(" "), r(" ", "Space", 0, 32, " "));
        assert_eq!(row("\u{E00D}"), r(" ", "Space", 0, 32, " "));
        assert_eq!(row("\u{E006}"), r("Enter", "Enter", 0, 13, "\r"));
        assert_eq!(row("\u{E007}"), r("Enter", "NumpadEnter", 1, 13, "\r"));
        assert_eq!(row("\u{E004}"), r("Tab", "Tab", 0, 9, ""));
        assert_eq!(row("\u{E00C}"), r("Escape", "Escape", 0, 27, ""));
        assert_eq!(row("\u{E008}"), r("Shift", "ShiftLeft", 1, 16, ""));
        assert_eq!(row("\u{E050}"), r("Shift", "ShiftRight", 2, 16, ""));
        assert_eq!(row("\u{E03D}"), r("Meta", "MetaLeft", 1, 91, ""));
        assert_eq!(row("\u{E053}"), r("Meta", "MetaRight", 2, 92, ""));
        assert_eq!(row("\u{E01A}"), r("0", "Numpad0", 3, 96, "0"));
        assert_eq!(row("\u{E024}"), r("*", "NumpadMultiply", 3, 106, "*"));
        assert_eq!(row("\u{E019}"), r("=", "NumpadEqual", 3, 187, "="));
        assert_eq!(row("\u{E018}"), r(";", "", 0, 0, ";"));
        assert_eq!(row("\u{E05C}"), r("Insert", "Numpad0", 3, 45, ""));
        assert_eq!(row("\u{E05B}"), r("ArrowDown", "Numpad2", 3, 40, ""));
        assert_eq!(row("\u{E031}"), r("F1", "F1", 0, 112, ""));
        assert_eq!(row("\u{E000}"), r("Unidentified", "", 0, 0, ""));
        // A code point WebDriver keeps but gives no key: no text.
        assert_eq!(row("\u{E030}"), r("\u{E030}", "", 0, 0, ""));
        // A grapheme of more than one code point is its own key.
        assert_eq!(row("e\u{301}"), r("e\u{301}", "", 0, 0, "e\u{301}"));
    }

    /// A key down types what it is; held modifiers go with what follows,
    /// on keys and on the mouse; a key pressed again repeats; a key up of
    /// one not down does nothing.
    #[test]
    fn modifiers_are_held_and_keys_repeat() {
        let (mut state, mut input) = (InputState::default(), Recorder::new());
        run(
            json!([
                { "type": "key", "id": "k", "actions": [
                    { "type": "keyDown", "value": "\u{E008}" },
                    { "type": "keyDown", "value": "a" },
                    { "type": "keyDown", "value": "a" },
                    { "type": "keyUp", "value": "a" },
                    { "type": "keyUp", "value": "b" },
                    { "type": "pause" },
                    { "type": "keyUp", "value": "\u{E008}" },
                ]},
                { "type": "pointer", "id": "m", "actions": [
                    { "type": "pause" },
                    { "type": "pause" },
                    { "type": "pause" },
                    { "type": "pause" },
                    { "type": "pause" },
                    { "type": "pointerMove", "x": 1, "y": 2 },
                ]},
            ]),
            &mut state,
            &mut input,
        )
        .unwrap();
        assert_eq!(
            input.take(),
            [
                r#"rawKeyDown "Shift" ShiftLeft 16 1 "" m8"#,
                r#"keyDown "a" KeyA 65 0 "a" m8"#,
                r#"keyDown "a" KeyA 65 0 "a" m8 repeat"#,
                r#"keyUp "a" KeyA 65 0 "" m8"#,
                "viewport",
                "mouseMoved 1,2 - b0 c0 m8 d0,0",
                r#"keyUp "Shift" ShiftLeft 16 1 "" m0"#,
            ]
        );
        // Held across Perform Actions, until let go of.
        let control = keys(json!([{ "type": "keyDown", "value": "\u{E009}" }]));
        run(control, &mut state, &mut input).unwrap();
        let alt = keys(json!([{ "type": "keyDown", "value": "\u{E052}" }]));
        run(alt, &mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                r#"rawKeyDown "Control" ControlLeft 17 1 "" m2"#,
                r#"rawKeyDown "Alt" AltRight 18 2 "" m3"#,
            ]
        );
        // Release Actions: the last pressed first.
        release(&mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                r#"keyUp "Alt" AltRight 18 2 "" m2"#,
                r#"keyUp "Control" ControlLeft 17 1 "" m0"#,
            ]
        );
    }

    /// Presses within 500 ms and 4 px of the last, of the same button, count
    /// up; in another Perform Actions they start again. Buttons down show
    /// in `buttons`, the DOM's bits.
    #[test]
    fn presses_in_a_row_count_up() {
        let (mut state, mut input) = (InputState::default(), Recorder::new());
        let click = [
            json!({ "type": "pointerDown", "button": 0 }),
            json!({ "type": "pointerUp", "button": 0 }),
        ];
        let mut actions = vec![json!({ "type": "pointerMove", "x": 10, "y": 10 })];
        actions.extend(click.clone());
        actions.push(json!({ "type": "pointerMove", "x": 4, "y": -4, "origin": "pointer" }));
        actions.extend(click.clone());
        actions.push(json!({ "type": "pause", "duration": 499 }));
        actions.extend(click.clone());
        // Too far now.
        actions.push(json!({ "type": "pointerMove", "x": 5, "y": 0, "origin": "pointer" }));
        actions.extend(click.clone());
        // Too late.
        actions.push(json!({ "type": "pause", "duration": 500 }));
        actions.extend(click.clone());
        // Other buttons.
        actions.push(json!({ "type": "pointerDown", "button": 2 }));
        actions.push(json!({ "type": "pointerDown", "button": 1 }));
        actions.push(json!({ "type": "pointerUp", "button": 2 }));
        actions.push(json!({ "type": "pointerUp", "button": 1 }));
        run(mouse(Value::Array(actions)), &mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                "viewport",
                "mouseMoved 10,10 - b0 c0 m0 d0,0",
                "mousePressed 10,10 left b1 c1 m0 d0,0",
                "mouseReleased 10,10 left b0 c1 m0 d0,0",
                "mouseMoved 14,6 - b0 c0 m0 d0,0",
                "mousePressed 14,6 left b1 c2 m0 d0,0",
                "mouseReleased 14,6 left b0 c2 m0 d0,0",
                "mousePressed 14,6 left b1 c3 m0 d0,0",
                "mouseReleased 14,6 left b0 c3 m0 d0,0",
                "mouseMoved 19,6 - b0 c0 m0 d0,0",
                "mousePressed 19,6 left b1 c1 m0 d0,0",
                "mouseReleased 19,6 left b0 c1 m0 d0,0",
                "mousePressed 19,6 left b1 c1 m0 d0,0",
                "mouseReleased 19,6 left b0 c1 m0 d0,0",
                "mousePressed 19,6 right b2 c1 m0 d0,0",
                "mousePressed 19,6 middle b6 c1 m0 d0,0",
                "mouseReleased 19,6 right b4 c1 m0 d0,0",
                "mouseReleased 19,6 middle b0 c1 m0 d0,0",
            ]
        );
        run(mouse(Value::from(click.to_vec())), &mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                "mousePressed 19,6 left b1 c1 m0 d0,0",
                "mouseReleased 19,6 left b0 c1 m0 d0,0",
            ]
        );
        // A press of one down already: nothing.
        let down = || mouse(json!([{ "type": "pointerDown", "button": 0 }]));
        run(down(), &mut state, &mut input).unwrap();
        run(down(), &mut state, &mut input).unwrap();
        assert_eq!(input.take(), ["mousePressed 19,6 left b1 c1 m0 d0,0"]);
        release(&mut state, &mut input).unwrap();
        assert_eq!(input.take(), ["mouseReleased 19,6 left b0 c1 m0 d0,0"]);
    }

    /// How far apart presses are is in the actions' own time: two in a row
    /// are a double click however long the renderer takes to answer each
    /// event, and a pause of the double click time makes them two single
    /// ones.
    #[test]
    fn presses_count_up_in_the_actions_time() {
        let (mut state, mut input) = (InputState::default(), Recorder::new());
        input.mouse_takes = Duration::from_millis(400);
        let down = json!({ "type": "pointerDown", "button": 0 });
        let up = json!({ "type": "pointerUp", "button": 0 });
        let pause = json!({ "type": "pause", "duration": 500 });
        run(
            mouse(json!([down, up, down, up, pause, down, up])),
            &mut state,
            &mut input,
        )
        .unwrap();
        assert_eq!(
            input.take(),
            [
                "mousePressed 0,0 left b1 c1 m0 d0,0",
                "mouseReleased 0,0 left b0 c1 m0 d0,0",
                "mousePressed 0,0 left b1 c2 m0 d0,0",
                "mouseReleased 0,0 left b0 c2 m0 d0,0",
                "mousePressed 0,0 left b1 c1 m0 d0,0",
                "mouseReleased 0,0 left b0 c1 m0 d0,0",
            ]
        );
    }

    /// Moves and scrolls go to their origin plus the offset, in the current
    /// browsing context's viewport, and the events to where that is in the
    /// top-level one; outside it, `move target out of bounds`. The pointer
    /// keeps its place in the viewport.
    #[test]
    fn moves_are_from_their_origin_in_the_viewport() {
        let (mut state, mut input) = (InputState::default(), Recorder::new());
        input.viewport = Viewport {
            x: 100.0,
            y: 50.0,
            width: 300.0,
            height: 150.0,
        };
        let element = json!({ "element-6066-11e4-a52e-4f735466cecf": "e" });
        run(
            json!([
                { "type": "pointer", "id": "m", "actions": [
                    { "type": "pointerMove", "x": 5, "y": 6 },
                    { "type": "pointerMove", "x": 1, "y": 2, "origin": "pointer" },
                    { "type": "pointerMove", "x": -3, "y": 4, "origin": element },
                    { "type": "pointerDown", "button": 0 },
                ]},
                { "type": "wheel", "id": "w", "actions": [
                    { "type": "pause" },
                    { "type": "pause" },
                    { "type": "pause" },
                    { "type": "scroll", "x": 1, "y": 1, "deltaX": -7, "deltaY": 70,
                      "origin": element },
                ]},
            ]),
            &mut state,
            &mut input,
        )
        .unwrap();
        assert_eq!(
            input.take(),
            [
                "viewport",
                "mouseMoved 105,56 - b0 c0 m0 d0,0",
                "mouseMoved 106,58 - b0 c0 m0 d0,0",
                "point of e",
                "mouseMoved 137,84 - b0 c0 m0 d0,0",
                "mousePressed 137,84 left b1 c1 m0 d0,0",
                "point of e",
                "mouseWheel 141,81 - b1 c0 m0 d-7,70",
            ]
        );
        // The edges are in; past them, out, and the pointer stays.
        for (x, y, out) in [
            (300, 150, false),
            (0, 0, false),
            (301, 0, true),
            (0, -1, true),
        ] {
            let moved = run(
                mouse(json!([{ "type": "pointerMove", "x": x, "y": y }])),
                &mut state,
                &mut input,
            );
            let expected = if out {
                "move target out of bounds"
            } else {
                "none"
            };
            assert_eq!(error_of(moved), expected, "{x}, {y}");
        }
        let past = mouse(json!([
            { "type": "pointerMove", "x": 0, "y": -1, "origin": "pointer" },
        ]));
        assert_eq!(
            error_of(run(past, &mut state, &mut input)),
            "move target out of bounds"
        );
        let wheel = json!([{ "type": "wheel", "id": "w", "actions": [
            { "type": "scroll", "x": 0, "y": 151, "deltaX": 0, "deltaY": 1 },
        ]}]);
        assert_eq!(
            error_of(run(wheel, &mut state, &mut input)),
            "move target out of bounds"
        );
    }

    /// A tick lasts its longest duration (a move moves at once); a source's
    /// type and pointer type stay; machopo's mouse has three buttons and no pen
    /// or touch; durations stop at the maximum safe integer; a scroll
    /// needs its coordinates and deltas, and no `pointer` origin.
    #[test]
    fn ticks_wait_and_wrong_actions_are_refused() {
        let (mut state, mut input) = (InputState::default(), Recorder::new());
        run(
            json!([
                { "type": "none", "id": "n", "actions": [
                    { "type": "pause", "duration": 30 },
                    { "type": "pause", "duration": 5 },
                ]},
                { "type": "pointer", "id": "m", "actions": [
                    { "type": "pointerMove", "x": 0, "y": 0, "duration": 70 },
                ]},
            ]),
            &mut state,
            &mut input,
        )
        .unwrap();
        assert_eq!(input.slept, Duration::from_millis(75));
        assert_eq!(input.take(), ["viewport", "mouseMoved 0,0 - b0 c0 m0 d0,0"]);

        let mut refused = |actions: Value| error_of(run(actions, &mut state, &mut input));
        assert_eq!(refused(keys(json!([]))), "none");
        let not_a_key = json!([{ "type": "pointer", "id": "k", "actions": [] }]);
        assert_eq!(refused(not_a_key), "invalid argument");
        let pen = json!([{ "type": "pointer", "id": "m", "actions": [],
                           "parameters": { "pointerType": "pen" } }]);
        assert_eq!(refused(pen), "unsupported operation");
        let touch = json!([{ "type": "pointer", "id": "p", "actions": [],
                             "parameters": { "pointerType": "touch" } }]);
        assert_eq!(refused(touch), "unsupported operation");
        for (button, error) in [
            (2, "none"),
            (3, "unsupported operation"),
            (4, "unsupported operation"),
            (5, "invalid argument"),
        ] {
            let up = mouse(json!([{ "type": "pointerUp", "button": button }]));
            assert_eq!(refused(up), error, "button {button}");
        }
        let max = MAX_SAFE_INTEGER;
        for (duration, error) in [(max, "none"), (max + 1, "invalid argument")] {
            let pause = json!([{ "type": "none", "id": "n", "actions": [
                { "type": "pause", "duration": duration },
            ]}]);
            let ticks = process(sequences(pause), &mut InputState::default());
            assert_eq!(error_of(ticks.map(drop)), error);
        }
        let too_wide = mouse(json!([{ "type": "pointerDown", "button": 0, "width": max + 1 }]));
        assert_eq!(refused(too_wide), "invalid argument");
        let far = json!([{ "type": "wheel", "id": "w", "actions": [
            { "type": "scroll", "x": 0, "y": 0, "deltaX": 0, "deltaY": max + 1 },
        ]}]);
        assert_eq!(refused(far), "invalid argument");
        let missing = json!([{ "type": "wheel", "id": "w", "actions": [
            { "type": "scroll", "x": 0, "y": 0, "deltaX": 0 },
        ]}]);
        assert_eq!(refused(missing), "invalid argument");
        let from_pointer = json!([{ "type": "wheel", "id": "w", "actions": [
            { "type": "scroll", "x": 0, "y": 0, "deltaX": 0, "deltaY": 0, "origin": "pointer" },
        ]}]);
        assert_eq!(refused(from_pointer), "invalid argument");
        let twice = json!([
            { "type": "key", "id": "k", "actions": [] },
            { "type": "key", "id": "k", "actions": [] },
        ]);
        assert_eq!(refused(twice), "invalid argument");
        // Nothing was dispatched for any of them.
        assert_eq!(input.take(), Vec::<String>::new());
    }

    /// Element Send Keys' keys: each character down and up, Shift pressed
    /// for one the US keyboard types with it and let go of for one it
    /// doesn't; a modifier held until NULL or the end, the right one's
    /// side kept; a cluster of more than one code point one key. Keys
    /// Perform Actions holds count; the source is gone after.
    #[test]
    fn send_keys_types_each_character() {
        let (mut state, mut input) = (InputState::default(), Recorder::new());
        send_keys("aB!c", &mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                r#"keyDown "a" KeyA 65 0 "a" m0"#,
                r#"keyUp "a" KeyA 65 0 "" m0"#,
                r#"rawKeyDown "Shift" ShiftLeft 16 1 "" m8"#,
                r#"keyDown "B" KeyB 66 0 "B" m8"#,
                r#"keyUp "B" KeyB 66 0 "" m8"#,
                r#"keyDown "!" Digit1 49 0 "!" m8"#,
                r#"keyUp "!" Digit1 49 0 "" m8"#,
                r#"keyUp "Shift" ShiftLeft 16 1 "" m0"#,
                r#"keyDown "c" KeyC 67 0 "c" m0"#,
                r#"keyUp "c" KeyC 67 0 "" m0"#,
            ]
        );
        send_keys("\u{E051}x\u{E000}y\u{E00A}e\u{301}", &mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                r#"rawKeyDown "Control" ControlRight 17 2 "" m2"#,
                r#"keyDown "x" KeyX 88 0 "x" m2"#,
                r#"keyUp "x" KeyX 88 0 "" m2"#,
                r#"keyUp "Control" ControlRight 17 2 "" m0"#,
                r#"keyDown "y" KeyY 89 0 "y" m0"#,
                r#"keyUp "y" KeyY 89 0 "" m0"#,
                r#"rawKeyDown "Alt" AltLeft 18 1 "" m1"#,
                r#"keyDown "e\u{301}"  0 0 "e\u{301}" m1"#,
                r#"keyUp "e\u{301}"  0 0 "" m1"#,
                r#"keyUp "Alt" AltLeft 18 1 "" m0"#,
            ]
        );
        // Shift held by Perform Actions counts.
        let shift = json!([{ "type": "key", "id": "k", "actions": [
            { "type": "keyDown", "value": "\u{E008}" },
        ]}]);
        run(shift, &mut state, &mut input).unwrap();
        input.take();
        send_keys("q", &mut state, &mut input).unwrap();
        assert_eq!(
            input.take(),
            [
                r#"keyDown "q" KeyQ 81 0 "q" m8"#,
                r#"keyUp "q" KeyQ 81 0 "" m8"#
            ]
        );
        assert_eq!(state.sources.len(), 1, "only Perform Actions' source");
        assert!(state.cancel_list.iter().all(|(id, _)| id == "k"));
    }
}
