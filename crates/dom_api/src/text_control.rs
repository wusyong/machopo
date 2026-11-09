//! HTML's text controls (https://html.spec.whatwg.org/#textFieldSelection):
//! a textarea's `value`, `defaultValue`, `type` and `textLength`, and the
//! selection API of a textarea and the text inputs (`dom`'s
//! `text_control`). A selection that changes queues `select` as an element
//! task (Chrome fires it later too), which the host runs
//! (`run_element_tasks`).

use dom::{
    ElementTaskKind, NodeRef, NodeRoot, SelectionDirection, SelectionMode, TargetRoot, Window,
    names,
};
use elise::Persistent;

use crate::interfaces::FocusOptions;
use crate::typed::ScriptHost;
use crate::{ApiCx, ApiError, DomStr, ExceptionName, fire_event};

fn not_applicable() -> ApiError {
    ApiError::dom(ExceptionName::InvalidState)
}

/// The selection range set, `select` queued if it changed.
fn set_selection_range<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    start: u32,
    end: u32,
    direction: SelectionDirection,
) {
    if cx.set_selection_range(this, start, end, direction) {
        cx.queue_element_task(this, names::event::SELECT);
    }
}

// --- <textarea> -------------------------------------------------------------------------

/// A textarea's `value`: its API value.
pub(crate) fn textarea_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.textarea_value(this)))
}

pub(crate) fn set_textarea_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_textarea_value(this, value);
    Ok(())
}

/// A textarea's `defaultValue`: its child text content.
pub(crate) fn textarea_default_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.child_text_content(this)))
}

/// Setting a textarea's `defaultValue`: DOM's string replace all, as
/// `textContent`'s.
pub(crate) fn set_textarea_default_value<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    value: &str,
) -> Result<(), ApiError> {
    cx.set_text_content(this, value);
    Ok(())
}

/// A textarea's `type`.
pub(crate) fn textarea_type<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: NodeRef<'gc>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Name(names::intern("textarea")))
}

/// A textarea's `textLength`: its API value's length in UTF-16 code units.
pub(crate) fn text_length<'gc>(cx: &ApiCx<'_, 'gc>, this: NodeRef<'gc>) -> Result<u32, ApiError> {
    Ok(u32::try_from(cx.textarea_value(this).utf16_len()).unwrap_or(u32::MAX))
}

// --- The selection API ----------------------------------------------------------------

/// `selectionStart`: null where the selection API doesn't apply.
pub(crate) fn selection_start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<u32>, ApiError> {
    Ok(cx.text_selection(this).map(|selection| selection.start))
}

/// Setting `selectionStart` (null is 0): the end moves to it if it is
/// before.
pub(crate) fn set_selection_start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    start: Option<u32>,
) -> Result<(), ApiError> {
    let current = cx.text_selection(this).ok_or_else(not_applicable)?;
    let start = start.unwrap_or(0);
    set_selection_range(cx, this, start, current.end.max(start), current.direction);
    Ok(())
}

pub(crate) fn selection_end<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<u32>, ApiError> {
    Ok(cx.text_selection(this).map(|selection| selection.end))
}

/// Setting `selectionEnd` (null is 0): the start moves to it if it is
/// after.
pub(crate) fn set_selection_end<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    end: Option<u32>,
) -> Result<(), ApiError> {
    let current = cx.text_selection(this).ok_or_else(not_applicable)?;
    set_selection_range(cx, this, current.start, end.unwrap_or(0), current.direction);
    Ok(())
}

pub(crate) fn selection_direction<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
) -> Result<Option<DomStr<'gc>>, ApiError> {
    Ok(cx
        .text_selection(this)
        .map(|selection| DomStr::Name(names::intern(selection.direction.keyword()))))
}

/// Setting `selectionDirection`: `forward` or `backward`, anything else
/// none.
pub(crate) fn set_selection_direction<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    direction: Option<&str>,
) -> Result<(), ApiError> {
    let current = cx.text_selection(this).ok_or_else(not_applicable)?;
    let direction = SelectionDirection::from_keyword(direction.unwrap_or(""));
    set_selection_range(cx, this, current.start, current.end, direction);
    Ok(())
}

/// `setSelectionRange(start, end, direction)`.
pub(crate) fn set_selection_range_method<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    start: u32,
    end: u32,
    direction: Option<&str>,
) -> Result<(), ApiError> {
    if !cx.selection_applies(this) {
        return Err(not_applicable());
    }
    set_selection_range(
        cx,
        this,
        start,
        end,
        SelectionDirection::from_keyword(direction.unwrap_or("")),
    );
    Ok(())
}

/// `select()`: all of the value selected, then the control takes the
/// focus if it can, whatever its type (Blink's `TextControlElement::
/// select`, which every `<input>` has: Chrome focuses a checkbox or a
/// read-only one too, not a disabled one). An input whose type `select()`
/// applies to but the selection API doesn't (a number) has no selection
/// here: it gets its `select` event only.
pub(crate) fn select<H: ScriptHost>(host: &mut H, this: NodeRoot) -> Result<(), ApiError> {
    host.mutate(|cx| {
        let element = this.get(cx.gc);
        if cx.selection_applies(element) {
            set_selection_range(cx, element, 0, u32::MAX, SelectionDirection::None);
        } else if cx.select_applies(element) {
            cx.queue_element_task(element, names::event::SELECT);
        }
    });
    crate::focus::focus(
        host,
        this,
        FocusOptions {
            focusVisible: None,
            preventScroll: true,
        },
    )
}

/// `setRangeText(replacement, start, end, selectionMode)`: `replacement`
/// in place of the code units from `start` to `end` (the selection's
/// without them), the value now dirty; the selection after it as
/// `selectionMode` says. `start` without `end` is WebIDL's `TypeError` (no
/// overload takes two arguments).
pub(crate) fn set_range_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: NodeRef<'gc>,
    replacement: &str,
    start: Option<u32>,
    end: Option<u32>,
    mode: Option<&str>,
) -> Result<(), ApiError> {
    let range = match (start, end) {
        (None, None) => None,
        (Some(start), Some(end)) => Some((start, end)),
        _ => {
            return Err(ApiError::type_error(
                "Failed to execute 'setRangeText': 2 arguments, not 1, 3 or 4.",
            ));
        }
    };
    let mode = match mode {
        None => SelectionMode::Preserve,
        Some(keyword) => SelectionMode::from_keyword(keyword).ok_or_else(|| {
            ApiError::type_error(format!(
                "The provided value '{keyword}' is not a valid enum value of type SelectionMode."
            ))
        })?,
    };
    if !cx.selection_applies(this) {
        return Err(not_applicable());
    }
    if range.is_some_and(|(start, end)| start > end) {
        return Err(ApiError::dom(ExceptionName::IndexSize));
    }
    if cx.set_range_text(this, replacement, range, mode) {
        cx.queue_element_task(this, names::event::SELECT);
    }
    Ok(())
}

// --- Element tasks ----------------------------------------------------------------------

/// Runs `window`'s element tasks (`queue_element_task`), each a task of its
/// own with a microtask checkpoint after it, those their listeners queue
/// too: `select` fired at its element, trusted, bubbling; `selectionchange`
/// at a document or a text control (bubbling at the control); an `<img>`'s
/// `load` or `error` and `decode()` promises (`image`); a `<canvas>`'s
/// `toBlob` callback (`canvas`); and the tasks of no element's (a
/// `FileReader`'s read, the performance timeline's, geolocation's and the
/// wake lock's permissions); the window's `load`. How many ran.
pub fn run_element_tasks<H: ScriptHost>(host: &mut H, window: &Persistent<Window>) -> usize {
    let mut ran = 0usize;
    while run_element_task(host, window) {
        ran = ran.saturating_add(1);
    }
    ran
}

/// Runs `window`'s first element task (`run_element_tasks`'s one), if the
/// DOM's pass lets it run. Whether one ran.
pub fn run_element_task<H: ScriptHost>(host: &mut H, window: &Persistent<Window>) -> bool {
    {
        let task = host.mutate(|cx| {
            let (element, kind, target) = cx.take_element_task(window.get(cx.gc))?;
            Some((
                cx.root(element),
                kind,
                target.map(|target| Persistent::new(cx.gc, target)),
            ))
        });
        let Some((element, kind, target)) = task else {
            return false;
        };
        match kind {
            ElementTaskKind::Fire(event_type) => {
                // `selectionchange` bubbles at a text control, not at a
                // document; the next change schedules another.
                let bubbles = match event_type {
                    names::event::SELECT => true,
                    names::event::SELECTIONCHANGE => host.mutate(|cx| {
                        let target = element.get(cx.gc);
                        cx.selectionchange_fired(target);
                        cx.kind(target) != dom::NodeKind::Document
                    }),
                    _ => false,
                };
                fire_event(host, &TargetRoot::Node(element), event_type, bubbles, false);
            }
            ElementTaskKind::Image(action) => crate::image::run_image_task(host, &element, action),
            ElementTaskKind::CanvasBlob(id) => crate::canvas::run_blob_task(host, id),
            ElementTaskKind::Script => crate::script::run_script_task(host, &element),
            ElementTaskKind::HashChange(old_url, new_url) => {
                crate::history::run_hash_change(host, &element, old_url, new_url);
            }
            ElementTaskKind::PerformanceTimeline => {
                if let Some(performance) = &target {
                    crate::performance::notify(host, performance);
                }
            }
            ElementTaskKind::FileRead(read, step) => {
                if let Some(reader) = &target {
                    crate::file_reader::run_read_task(host, reader, read, step);
                }
            }
            ElementTaskKind::Geolocation(step) => crate::geolocation::run_task(host, window, step),
            ElementTaskKind::WakeLockPermission { promise, answered } => {
                crate::wake_lock::run_task(host, window, promise, answered);
            }
            // HTML's "the end": complete, the window's `load` (each event
            // with its checkpoint), then the host hears of it
            // (`Dom::take_loaded_documents`): its iframe's `load` next.
            ElementTaskKind::BlobRead(promise) => {
                let read = host.read(|cx| cx.take_blob_read(window.get(cx.gc), promise));
                if let Some(read) = read {
                    host.resolve_promise_with_read(promise, read);
                }
            }
            ElementTaskKind::WindowLoad => {
                crate::lifecycle::document_ready(host, &element, crate::DocumentReadiness::Loaded);
                host.mutate(|cx| cx.note_document_loaded(element.get(cx.gc)));
            }
        }
        host.perform_microtask_checkpoint();
        true
    }
}
