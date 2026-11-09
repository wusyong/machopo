//! Typing in a text control: the user's keys in a `<textarea>` or a text
//! `<input>` (those the selection API applies to), at its selection
//! (Blink's `Editor` and `TypingCommand` on a `TextControlElement`'s inner
//! editor, the small part of them that a value with a selection needs).
//! The edit changes the control's value and selection (`dom`'s
//! `replace_by_user`), which its UA shadow tree follows. There is no input
//! method, undo or clipboard yet.
//!
//! A key that types text, its `keypress` not canceled, goes in through
//! [`type_text`]: `beforeinput` (cancelable), `textInput` (cancelable, the
//! legacy `TextEvent`), the value changed and the cursor after the text,
//! `input`. Backspace, Delete and the keys that move the cursor are the
//! `keydown`'s ([`edit_key`]). Deleting and moving go by grapheme cluster,
//! as Blink's (whose Backspace takes some scripts' clusters apart, which
//! machopo doesn't). A change the user made is committed, firing `change`, as
//! the control loses the focus or, for an `<input>`, on Enter
//! ([`commit`]).

use dom::names::{event, local};
use dom::{
    EventFlags, InputEventInfo, NameId, NodeRef, NodeRoot, SelectionDirection, TargetRef,
    TargetRoot, TextSelection,
};
use elise::Persistent;
use icu_segmenter::GraphemeClusterSegmenter;

use crate::typed::ScriptHost;
use crate::{ApiCx, dispatch};

/// A key that edits or moves in a text control on its `keydown`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKey {
    Backspace,
    Delete,
    Left,
    Right,
    Home,
    End,
}

impl EditKey {
    /// The key the DOM's `key` names, if it is one.
    pub fn of(key: &str) -> Option<Self> {
        Some(match key {
            "Backspace" => Self::Backspace,
            "Delete" => Self::Delete,
            "ArrowLeft" => Self::Left,
            "ArrowRight" => Self::Right,
            "Home" => Self::Home,
            "End" => Self::End,
            _ => return None,
        })
    }
}

/// Whether `element` is a text control the user types in: those the
/// selection API applies to.
pub(crate) fn is_text_field<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    cx.selection_applies(element)
}

/// Whether the user may change its value: not read-only, not disabled.
pub(crate) fn is_mutable<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> bool {
    cx.attribute_value_by_id(element, local::READONLY).is_none()
        && !cx.is_disabled_form_control(element)
}

fn utf16_len(text: &str) -> u32 {
    u32::try_from(text.encode_utf16().count()).unwrap_or(u32::MAX)
}

/// The UTF-16 offset of byte `byte` of `value`.
fn unit_at(value: &str, byte: usize) -> u32 {
    utf16_len(&value[..byte])
}

/// The byte offset of the UTF-16 offset `unit` in `value` (its end past
/// it).
fn byte_at(value: &str, unit: u32) -> usize {
    let mut units = 0u32;
    for (byte, c) in value.char_indices() {
        if units >= unit {
            return byte;
        }
        // A char is one or two UTF-16 units: no overflow below a JS
        // string's length.
        units = units.saturating_add(u32::try_from(c.len_utf16()).unwrap_or(2));
    }
    value.len()
}

/// The grapheme cluster boundaries of `value`, as UTF-16 offsets.
fn boundaries(value: &str) -> Vec<u32> {
    GraphemeClusterSegmenter::new()
        .segment_str(value)
        .map(|byte| unit_at(value, byte))
        .collect()
}

/// The boundary before `at` (0 at the start).
fn previous_boundary(value: &str, at: u32) -> u32 {
    boundaries(value)
        .into_iter()
        .filter(|b| *b < at)
        .max()
        .unwrap_or(0)
}

/// The boundary after `at` (the end at the end).
fn next_boundary(value: &str, at: u32) -> u32 {
    boundaries(value)
        .into_iter()
        .find(|b| *b > at)
        .unwrap_or_else(|| utf16_len(value))
}

/// Where the line `at` is on starts: after the newline before it. Lines
/// are the value's: machopo doesn't know where a textarea wraps.
fn line_start(value: &str, at: u32) -> u32 {
    let byte = byte_at(value, at);
    value[..byte]
        .rfind('\n')
        .map_or(0, |newline| unit_at(value, newline.saturating_add(1)))
}

/// Where it ends: at the newline after it, or the value's end.
fn line_end(value: &str, at: u32) -> u32 {
    let byte = byte_at(value, at);
    value[byte..].find('\n').map_or_else(
        || utf16_len(value),
        |newline| unit_at(value, byte.saturating_add(newline)),
    )
}

/// The selection's anchor and focus: the focus is the end that moves.
fn anchor_and_focus(selection: TextSelection) -> (u32, u32) {
    match selection.direction {
        SelectionDirection::Backward => (selection.end, selection.start),
        _ => (selection.start, selection.end),
    }
}

/// The selection from `anchor` to `focus`: backward if the focus is
/// before, forward if after, none if they are one.
fn selection_between(anchor: u32, focus: u32) -> TextSelection {
    let direction = match focus.cmp(&anchor) {
        std::cmp::Ordering::Less => SelectionDirection::Backward,
        std::cmp::Ordering::Greater => SelectionDirection::Forward,
        std::cmp::Ordering::Equal => SelectionDirection::None,
    };
    TextSelection {
        start: anchor.min(focus),
        end: anchor.max(focus),
        direction,
    }
}

/// HTML's `maxlength` as a valid non-negative integer, if it has one.
fn max_length<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>) -> Option<u32> {
    let value = cx.attribute_value_by_id(element, local::MAXLENGTH)?;
    let value = value.borrow().to_str().into_owned();
    value
        .trim_matches(|c: char| c.is_ascii_whitespace())
        .parse::<u32>()
        .ok()
}

/// `text` cut to what fits in `room` UTF-16 code units, whole grapheme
/// clusters only.
fn fitting(text: &str, room: u32) -> &str {
    let mut end = 0;
    for byte in GraphemeClusterSegmenter::new().segment_str(text) {
        if utf16_len(&text[..byte]) > room {
            break;
        }
        end = byte;
    }
    &text[..end]
}

/// A trusted UI event of `event_type` at `element` (bubbling, composed,
/// its view the element's window), made by `data` (an input event's or a
/// text event's), cancelable if `cancelable`: whether nobody canceled it.
fn fire_ui<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    event_type: NameId,
    cancelable: bool,
    data: impl for<'gc> FnOnce(
        &ApiCx<'_, 'gc>,
        elise::Gc<'gc, dom::Event>,
        Option<elise::Gc<'gc, dom::Window>>,
    ),
) -> bool {
    let event = host.mutate(|cx| {
        let window = cx.target_window(TargetRef::Node(element.get(cx.gc)));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles: true,
                cancelable,
                composed: true,
                trusted: true,
            },
            window,
        );
        data(cx, event, window);
        Persistent::new(cx.gc, event)
    });
    dispatch(host, &TargetRoot::Node(element.clone()), &event)
}

/// A trusted `beforeinput` (cancelable) or `input` (not), an `InputEvent`
/// with `input_type` and `data`: whether nobody canceled it.
fn fire_input_event<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    event_type: NameId,
    input_type: &str,
    data: Option<&str>,
) -> bool {
    let info = InputEventInfo {
        detail: 0,
        data: data.map(dom::DomString::new),
        input_type: dom::DomString::new(input_type),
        is_composing: false,
    };
    fire_ui(
        host,
        element,
        event_type,
        event_type == event::BEFOREINPUT,
        |cx, event, window| {
            cx.set_input_event_data(event, window, &info);
        },
    )
}

/// A trusted `textInput` (the legacy `TextEvent`, cancelable) with `data`:
/// whether nobody canceled it.
fn fire_text_input<H: ScriptHost>(host: &mut H, element: &NodeRoot, data: &str) -> bool {
    fire_ui(
        host,
        element,
        event::TEXT_INPUT,
        true,
        |cx, event, window| {
            cx.set_text_event_data(event, window, data);
        },
    )
}

/// A trusted `change` at `element`: bubbling, not cancelable, not
/// composed.
pub(crate) fn fire_change<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    fire_plain(host, element, event::CHANGE, false);
}

/// A trusted plain `input` at `element` (HTML's "fire an event named
/// input", as WebDriver's clear does): bubbling, composed, not cancelable.
pub(crate) fn fire_plain_input<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    fire_plain(host, element, event::INPUT, true);
}

fn fire_plain<H: ScriptHost>(host: &mut H, element: &NodeRoot, event_type: NameId, composed: bool) {
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

/// Whether the element is still one to edit, and connected, after
/// script ran.
fn still_editable<H: ScriptHost>(host: &mut H, element: &NodeRoot) -> bool {
    host.read(|cx| {
        let node = element.get(cx.gc);
        is_text_field(cx, node) && is_mutable(cx, node) && cx.is_connected(node)
    })
}

/// An edit of the control's selection, or of `range` (UTF-16 offsets)
/// if given, to `text`, as the user's: `beforeinput` with `input_type`
/// and `data`, then (with `text_input`) `textInput` with it, each may
/// cancel it; then the change, and `input`.
fn edit<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    range: Option<(u32, u32)>,
    text: &str,
    input_type: &str,
    data: Option<&str>,
    text_input: Option<&str>,
) {
    if !fire_input_event(host, element, event::BEFOREINPUT, input_type, data)
        || !still_editable(host, element)
    {
        return;
    }
    if let Some(typed) = text_input
        && (!fire_text_input(host, element, typed) || !still_editable(host, element))
    {
        return;
    }
    host.mutate(|cx| {
        let node = element.get(cx.gc);
        let (start, end) = range.unwrap_or_else(|| {
            let current = cx.text_selection(node).unwrap_or_default();
            (current.start, current.end)
        });
        cx.replace_by_user(node, start, end, text);
    });
    fire_input_event(host, element, event::INPUT, input_type, data);
}

/// What the user typed, `text`, in `element` (the key's `keypress` wasn't
/// canceled; Blink's `EventHandler::HandleTextInputEvent`): in a text
/// control, it takes the selection's place, as much of it as `maxlength`
/// leaves room for; Enter (`"\r"`) is a line break in a `<textarea>`, and
/// commits an `<input>`'s change (there is no implicit submission yet).
/// Whether `element` is a text control, which takes the key: it does
/// nothing else (a Space doesn't scroll).
pub fn type_text<H: ScriptHost>(host: &mut H, element: &NodeRoot, text: &str) -> bool {
    let (field, textarea, mutable, room) = host.read(|cx| {
        let node = element.get(cx.gc);
        let room = max_length(cx, node).map(|max| {
            let current = cx.text_selection(node).unwrap_or_default();
            let length = u32::try_from(cx.text_control_value(node).utf16_len()).unwrap_or(u32::MAX);
            // The selection is within the value: start <= end <= length.
            let unselected = length.saturating_sub(current.end.saturating_sub(current.start));
            max.saturating_sub(unselected)
        });
        (
            is_text_field(cx, node),
            cx.is_textarea(node),
            is_mutable(cx, node),
            room,
        )
    });
    if !field {
        return false;
    }
    if text == "\r" && !textarea {
        commit(host, element);
        return true;
    }
    if !mutable {
        return true;
    }
    if text == "\r" {
        edit(
            host,
            element,
            None,
            "\n",
            "insertLineBreak",
            None,
            Some("\n"),
        );
        return true;
    }
    let text = match room {
        Some(room) => fitting(text, room),
        None => text,
    };
    if !text.is_empty() {
        edit(
            host,
            element,
            None,
            text,
            "insertText",
            Some(text),
            Some(text),
        );
    }
    true
}

/// The user's selection: a new one that isn't empty queues `select`, as
/// the user selecting text does.
fn select_by_user<'gc>(cx: &ApiCx<'_, 'gc>, element: NodeRef<'gc>, new: TextSelection) {
    if cx.set_selection_range(element, new.start, new.end, new.direction) && new.start != new.end {
        cx.queue_element_task(element, event::SELECT);
    }
}

/// What `key` does in `element` on its `keydown` (nobody canceled it):
/// in a text control, Backspace and Delete delete the selection, or the
/// grapheme cluster before or after the cursor; the arrows, Home and End
/// move the cursor (Home and End to the line's ends), or with `extend`
/// (Shift) the selection's focus. Whether `element` is a text control,
/// which takes the key: it does nothing else (an arrow doesn't scroll).
pub fn edit_key<H: ScriptHost>(
    host: &mut H,
    element: &NodeRoot,
    key: EditKey,
    extend: bool,
) -> bool {
    let Some((value, current, mutable)) = host.read(|cx| {
        let node = element.get(cx.gc);
        is_text_field(cx, node).then(|| {
            (
                cx.text_control_value(node).to_str().into_owned(),
                cx.text_selection(node).unwrap_or_default(),
                is_mutable(cx, node),
            )
        })
    }) else {
        return false;
    };
    let (anchor, focus) = anchor_and_focus(current);
    let collapsed = current.start == current.end;
    match key {
        EditKey::Backspace | EditKey::Delete => {
            if !mutable {
                return true;
            }
            let backward = key == EditKey::Backspace;
            let (start, end) = match key {
                _ if !collapsed => (current.start, current.end),
                EditKey::Backspace => (previous_boundary(&value, current.start), current.start),
                _ => (current.start, next_boundary(&value, current.start)),
            };
            let input_type = if backward {
                "deleteContentBackward"
            } else {
                "deleteContentForward"
            };
            if start != end {
                edit(
                    host,
                    element,
                    Some((start, end)),
                    "",
                    input_type,
                    None,
                    None,
                );
            }
        }
        EditKey::Left | EditKey::Right | EditKey::Home | EditKey::End => {
            let target = match key {
                EditKey::Left if collapsed || extend => previous_boundary(&value, focus),
                EditKey::Left => current.start,
                EditKey::Right if collapsed || extend => next_boundary(&value, focus),
                EditKey::Right => current.end,
                EditKey::Home => line_start(&value, focus),
                _ => line_end(&value, focus),
            };
            let new = if extend {
                selection_between(anchor, target)
            } else {
                selection_between(target, target)
            };
            host.mutate(|cx| select_by_user(cx, element.get(cx.gc), new));
        }
    }
    true
}

/// Commits the user's change of `element`'s value (HTML's "committing"
/// a text control's value, as it loses the focus or on Enter): `change`
/// if the value differs from what it was before the user's first change
/// since the last commit.
pub fn commit<H: ScriptHost>(host: &mut H, element: &NodeRoot) {
    if host.mutate(|cx| cx.commit_edit(element.get(cx.gc))) {
        fire_change(host, element);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundaries_are_grapheme_clusters_in_utf16() {
        // e + combining acute, an emoji with a skin tone (two surrogate
        // pairs), x.
        let value = "e\u{301}\u{1F44D}\u{1F3FD}x";
        assert_eq!(boundaries(value), [0, 2, 6, 7]);
        assert_eq!(previous_boundary(value, 6), 2);
        assert_eq!(previous_boundary(value, 0), 0);
        assert_eq!(next_boundary(value, 2), 6);
        assert_eq!(next_boundary(value, 7), 7);
        assert_eq!(byte_at(value, 2), 3);
        assert_eq!(fitting(value, 5), "e\u{301}");
        assert_eq!(fitting(value, 6), "e\u{301}\u{1F44D}\u{1F3FD}");
    }

    #[test]
    fn lines_end_at_newlines() {
        let value = "ab\ncd\n";
        assert_eq!((line_start(value, 4), line_end(value, 4)), (3, 5));
        assert_eq!((line_start(value, 2), line_end(value, 2)), (0, 2));
        assert_eq!((line_start(value, 6), line_end(value, 6)), (6, 6));
    }
}
