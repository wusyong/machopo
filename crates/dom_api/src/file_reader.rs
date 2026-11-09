//! File API's `FileReader` (https://w3c.github.io/FileAPI/#APIASynch;
//! Blink's `core/fileapi/file_reader.cc` and `file_reader_loader.cc`), its
//! "read operation" over a blob's bytes, which are already in memory (they
//! come in one chunk): each event a task of its own, of the reader's
//! window's element task queue (`dom`'s `ElementTaskKind::FileRead`):
//! `loadstart`, `progress` (if there are bytes), then `load` and `loadend`
//! in one; a task of a read the reader has left (aborted, or begun anew)
//! does nothing. The events are `ProgressEvent`s whose `total` is the
//! blob's size (but an abort's), as Chrome's. And `File`'s members (File
//! API's), and `ProgressEvent`'s (XHR's).

use base64::Engine as _;
use dom::{
    Blob, BlobData, DomString, Event, EventFlags, EventTargetImpl, FileInfo, FileReaderResult,
    FileReaderState, ReadAs, ReadStep, TargetRoot, names,
};
use elise::{Gc, Persistent};
use encoding_rs::Encoding;

use crate::interfaces::{FilePropertyBag, ProgressEventInit};
use crate::typed::{BlobPart, OpFront, ScriptHost};
use crate::{ApiCx, ApiError, DomException, DomStr, ExceptionName, dispatch};

/// Blink's message for a read while one goes on.
const BUSY: &str = "The object is already busy reading Blobs.";

/// `new FileReader()`: one of the current realm's window's.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let window = op.current_window(cx).ok_or_else(|| {
        ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: "no realm to make the FileReader in".to_owned(),
        })
    })?;
    Ok(cx.create_file_reader(window))
}

/// `readyState`.
pub(crate) fn ready_state<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<u16, ApiError> {
    Ok(match cx.file_reader_state(this) {
        FileReaderState::Empty => 0,
        FileReaderState::Loading => 1,
        FileReaderState::Done => 2,
    })
}

/// `result`: `null` until a read is done; an `ArrayBuffer` the same one
/// for the read (`ReaderResult`).
pub(crate) fn result<'gc>(
    _: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    Ok(this)
}

/// `error`: always `null`, as reading bytes in memory doesn't fail.
pub(crate) fn error<'gc>(_: &ApiCx<'_, 'gc>, _: Gc<'gc, EventTargetImpl>) -> Result<(), ApiError> {
    Ok(())
}

pub(crate) fn read_as_array_buffer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    blob: Gc<'gc, Blob>,
) -> Result<(), ApiError> {
    start(cx, this, blob, ReadAs::ArrayBuffer, "readAsArrayBuffer")
}

pub(crate) fn read_as_binary_string<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    blob: Gc<'gc, Blob>,
) -> Result<(), ApiError> {
    start(cx, this, blob, ReadAs::BinaryString, "readAsBinaryString")
}

pub(crate) fn read_as_data_url<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    blob: Gc<'gc, Blob>,
) -> Result<(), ApiError> {
    start(cx, this, blob, ReadAs::DataUrl, "readAsDataURL")
}

pub(crate) fn read_as_text<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    blob: Gc<'gc, Blob>,
    encoding: Option<&str>,
) -> Result<(), ApiError> {
    start(
        cx,
        this,
        blob,
        ReadAs::Text(encoding.map(str::to_owned)),
        "readAsText",
    )
}

/// File API's "read operation" starting: a read while one goes on is an
/// `InvalidStateError`; otherwise the reader is `LOADING` and the first
/// task queued, with the blob's bytes as they are.
fn start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    blob: Gc<'gc, Blob>,
    how: ReadAs,
    method: &str,
) -> Result<(), ApiError> {
    if cx.file_reader_state(this) == FileReaderState::Loading {
        return Err(ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: format!("Failed to execute '{method}' on 'FileReader': {BUSY}"),
        }));
    }
    cx.start_file_reader_read(this, cx.blob_data(blob).clone(), how);
    Ok(())
}

/// A read's task (`ElementTaskKind::FileRead`): nothing if the reader left
/// the read; otherwise its event (`loadstart`, `progress`), and the next
/// task; at the end the result, `load`, and `loadend` unless a `load`
/// handler began another read.
pub(crate) fn run_read_task<H: ScriptHost>(
    host: &mut H,
    reader: &Persistent<EventTargetImpl>,
    read: u64,
    step: ReadStep,
) {
    let Some((data, how)) = host.read(|cx| {
        let reader = reader.get(cx.gc);
        cx.file_reader_reading(reader, read)
            .then(|| cx.file_reader_read(reader))
            .flatten()
    }) else {
        return;
    };
    let size = data.bytes().len();
    match step {
        ReadStep::LoadStart | ReadStep::Progress => {
            let (event_type, loaded, next) = match step {
                ReadStep::LoadStart if size > 0 => (names::event::LOADSTART, 0, ReadStep::Progress),
                ReadStep::LoadStart => (names::event::LOADSTART, 0, ReadStep::Load),
                _ => (names::event::PROGRESS, size, ReadStep::Load),
            };
            fire(host, reader, event_type, Some((loaded, size)));
            host.mutate(|cx| {
                let reader = reader.get(cx.gc);
                if cx.file_reader_reading(reader, read) {
                    cx.queue_file_read_step(reader, read, next);
                }
            });
        }
        ReadStep::Load => {
            let result = package(&data, &how);
            if !host.mutate(|cx| cx.finish_file_reader_read(reader.get(cx.gc), read, result)) {
                return;
            }
            fire(host, reader, names::event::LOAD, Some((size, size)));
            if host.read(|cx| cx.file_reader_state(reader.get(cx.gc))) != FileReaderState::Loading {
                fire(host, reader, names::event::LOADEND, Some((size, size)));
            }
        }
    }
}

/// `abort()`: of a read going on, the reader is `DONE` with a `null`
/// `result`, its tasks do nothing, and `abort` fires, then `loadend`
/// unless an `abort` handler began another read. Otherwise nothing (Chrome
/// keeps a done read's `result`, which File API would make `null`).
pub(crate) fn abort<H: ScriptHost>(
    host: &mut H,
    this: Persistent<EventTargetImpl>,
) -> Result<(), ApiError> {
    if !host.mutate(|cx| cx.abort_file_reader(this.get(cx.gc))) {
        return Ok(());
    }
    fire(host, &this, names::event::ABORT, None);
    if host.read(|cx| cx.file_reader_state(this.get(cx.gc))) != FileReaderState::Loading {
        fire(host, &this, names::event::LOADEND, None);
    }
    Ok(())
}

/// A trusted `ProgressEvent` of `event_type` at the reader, that doesn't
/// bubble and can't be canceled, with how many of how many bytes were
/// read; for an abort, none of an unknown length (Chrome 152's).
fn fire<H: ScriptHost>(
    host: &mut H,
    reader: &Persistent<EventTargetImpl>,
    event_type: names::NameId,
    read: Option<(usize, usize)>,
) {
    let event = host.mutate(|cx| {
        let target = dom::TargetRef::EventTarget(reader.get(cx.gc));
        let event = cx.create_event(
            event_type,
            EventFlags {
                bubbles: false,
                cancelable: false,
                composed: false,
                trusted: true,
            },
            cx.target_window(target),
        );
        let (loaded, total) = read.unwrap_or_default();
        #[expect(
            clippy::cast_precision_loss,
            reason = "a double is what script reads, as Blink's are; a blob in memory is far below 2^53 bytes"
        )]
        cx.set_progress_event_data(event, read.is_some(), loaded as f64, total as f64);
        Persistent::new(cx.gc, event)
    });
    dispatch(host, &TargetRoot::EventTarget(reader.clone()), &event);
}

/// File API's "package data": the read's result.
fn package(data: &BlobData, how: &ReadAs) -> FileReaderResult {
    let bytes = data.bytes();
    match how {
        ReadAs::ArrayBuffer => FileReaderResult::Bytes(data.shared_bytes()),
        ReadAs::BinaryString => {
            FileReaderResult::Text(bytes.iter().map(|&b| char::from(b)).collect())
        }
        ReadAs::DataUrl => {
            let type_ = if data.type_().is_empty() {
                "application/octet-stream"
            } else {
                data.type_()
            };
            let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
            FileReaderResult::Text(format!("data:{type_};base64,{encoded}"))
        }
        ReadAs::Text(label) => {
            FileReaderResult::Text(decode(bytes, label.as_deref(), data.type_()))
        }
    }
}

/// File API's "read as text"'s decoding: Encoding's "decode" (a BOM
/// first) with the encoding the label names, else the one the blob's
/// type's `charset` names, else UTF-8. As Chrome's, a label with white
/// space around it names none (Encoding's "get an encoding" would strip
/// it).
fn decode(bytes: &[u8], label: Option<&str>, type_: &str) -> String {
    let named = |label: &str| Encoding::for_label(label.as_bytes());
    let encoding = label
        .filter(|label| label.trim_matches(|c: char| c.is_ascii_whitespace()) == *label)
        .and_then(named)
        .or_else(|| charset(type_).and_then(named))
        .unwrap_or(encoding_rs::UTF_8);
    let (text, _, _) = encoding.decode(bytes);
    text.into_owned()
}

/// The `charset` parameter of a MIME type (lowercase, as a blob's type
/// is), if it has one.
fn charset(type_: &str) -> Option<&str> {
    type_.split(';').skip(1).find_map(|parameter| {
        let (name, value) = parameter.split_once('=')?;
        (name.trim() == "charset").then(|| value.trim().trim_matches('"'))
    })
}

/// `new File(fileBits, fileName, options)`: a blob of the bits (as
/// `new Blob()`'s), with the name and the modification time (the options',
/// or now), as Blink keeps it (`as_blink_time`).
pub(crate) fn construct_file<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    bits: Vec<BlobPart<'_, 'gc>>,
    name: &str,
    options: &FilePropertyBag,
) -> Result<Gc<'gc, Blob>, ApiError> {
    let blob = crate::blob::construct(
        cx,
        bits,
        &crate::interfaces::BlobPropertyBag {
            r#type: options.r#type.clone(),
            endings: options.endings.clone(),
        },
    )?;
    let last_modified = match options.lastModified {
        Some(time) => to_int64(time),
        // `Date.now()`'s clock: the system's.
        None => {
            let _ = op;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0.0, |since| since.as_secs_f64() * 1000.0);
            base::numerics::clamp_floor(now)
        }
    };
    let data = cx.blob_data(blob).clone().with_file(FileInfo {
        name: name.to_owned(),
        last_modified: as_blink_time(last_modified),
    });
    Ok(cx.create_blob(data))
}

/// WebIDL's conversion of a number to a `long long` (`ToInt64`): toward
/// zero, modulo 2^64; 0 for NaN and the infinities.
fn to_int64(number: f64) -> i64 {
    if !number.is_finite() {
        return 0;
    }
    let modulo = number.trunc().rem_euclid(18_446_744_073_709_551_616.0);
    // In 0 to 2^64, which a u64 holds (saturating at the top, the one
    // value a double that close to 2^64 can't tell apart); the two's
    // complement is the signed value.
    let unsigned: u64 = base::numerics::saturated_cast(modulo);
    i64::from_ne_bytes(unsigned.to_ne_bytes())
}

/// Milliseconds since the Unix epoch as Blink keeps them (`base::Time`,
/// microseconds since 1601, saturating), read back: what `lastModified`
/// gives (1e20 reads 9211727563254776, as in Chrome 152).
fn as_blink_time(millis: i64) -> i64 {
    /// From 1601 to 1970, in microseconds (`base::Time`'s epoch delta).
    const EPOCH_DELTA_MICROS: i64 = 11_644_473_600_000_000;
    let since_1601 = millis
        .saturating_mul(1000)
        .saturating_add(EPOCH_DELTA_MICROS);
    since_1601.saturating_sub(EPOCH_DELTA_MICROS) / 1000
}

fn file_info<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Blob>) -> Result<&'gc FileInfo, ApiError> {
    cx.blob_data(this)
        .file()
        .ok_or_else(|| ApiError::type_error("Illegal invocation"))
}

/// A `File`'s `name`.
pub(crate) fn name<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Blob>) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::new(&file_info(cx, this)?.name)))
}

/// A `File`'s `lastModified`.
pub(crate) fn last_modified<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
) -> Result<f64, ApiError> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a double is what script reads, as Blink's is"
    )]
    Ok(file_info(cx, this)?.last_modified as f64)
}

/// A `File`'s `webkitRelativePath`: empty, as no directory upload makes one.
pub(crate) fn webkit_relative_path<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
) -> Result<DomStr<'gc>, ApiError> {
    file_info(cx, this)?;
    Ok(DomStr::Owned(DomString::new("")))
}

/// `new ProgressEvent(type, eventInitDict)`.
pub(crate) fn construct_progress_event<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    event_type: &str,
    init: &ProgressEventInit,
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
    cx.set_progress_event_data(event, init.lengthComputable, init.loaded, init.total);
    Ok(event)
}

fn progress<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> (bool, f64, f64) {
    cx.progress_event_data(this).unwrap_or_default()
}

pub(crate) fn length_computable<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Event>,
) -> Result<bool, ApiError> {
    Ok(progress(cx, this).0)
}

pub(crate) fn loaded<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(progress(cx, this).1)
}

pub(crate) fn total<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Event>) -> Result<f64, ApiError> {
    Ok(progress(cx, this).2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_types_charset() {
        assert_eq!(
            charset("text/plain;charset=windows-1252"),
            Some("windows-1252")
        );
        assert_eq!(
            charset("text/plain; foo=bar; charset=\"utf-16\""),
            Some("utf-16")
        );
        assert_eq!(charset("text/plain"), None);
        assert_eq!(charset("charset=utf-8"), None);
    }

    #[test]
    fn what_decodes() {
        // The BOM first, then the label, then the type's charset, then UTF-8.
        assert_eq!(decode(b"\xef\xbb\xbfA", Some("windows-1252"), ""), "A");
        assert_eq!(
            decode(b"\xe9A", Some("windows-1252"), "text/plain;charset=utf-8"),
            "\u{e9}A"
        );
        assert_eq!(
            decode(b"\xe9A", None, "text/plain;charset=windows-1252"),
            "\u{e9}A"
        );
        assert_eq!(
            decode(b"\xe9A", Some("nope"), "text/plain;charset=windows-1252"),
            "\u{e9}A"
        );
        assert_eq!(decode(b"\xe9A", None, ""), "\u{fffd}A");
        assert_eq!(decode(b"A\0B\0", Some("UTF-16LE"), ""), "AB");
        assert_eq!(decode(b"A\0B\0", Some(" UTF-16LE "), ""), "A\0B\0");
    }

    #[test]
    fn file_times() {
        assert_eq!(to_int64(f64::NAN), 0);
        assert_eq!(to_int64(-1.5), -1);
        assert_eq!(as_blink_time(1_000), 1_000);
        assert_eq!(as_blink_time(i64::MAX), 9_211_727_563_254_775);
    }
}
