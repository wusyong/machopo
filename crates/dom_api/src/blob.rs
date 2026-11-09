//! File API's `Blob` (https://w3c.github.io/FileAPI/#blob-section; Blink's
//! `core/fileapi/blob.cc`): made of parts, sliced and read. Its bytes are
//! `dom`'s `BlobData`, shared by its slices. They are in memory and
//! never change, so a read's promise is resolved at once (Blink's reads
//! go through a `FileReaderLoader` and resolve in a later task: what
//! script sees is the same, a reaction in a later microtask).

use dom::{Blob, BlobData, BlobRead, DomString};
use elise::Gc;

use crate::interfaces::BlobPropertyBag;
use crate::typed::{BlobPart, OpFront, PromiseOf};
use crate::{ApiCx, ApiError, DomStr};

/// File API's normalization of a type (`new Blob()`'s, `slice()`'s): a
/// character outside U+0020 to U+007E makes it empty; otherwise it is
/// lowercased.
pub(crate) fn normalize_type(type_: &str) -> String {
    if type_.chars().all(|c| ('\u{20}'..='\u{7e}').contains(&c)) {
        type_.to_ascii_lowercase()
    } else {
        String::new()
    }
}

/// File API's "convert line endings to native": each `\r\n`, `\r` and `\n`
/// becomes the platform's line ending (`\r\n` on Windows, `\n` elsewhere,
/// as Blink's `NormalizeLineEndingsToNative`).
fn to_native_line_endings(text: &str, out: &mut Vec<u8>) {
    let native: &[u8] = if cfg!(windows) { b"\r\n" } else { b"\n" };
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.extend_from_slice(native);
            }
            '\n' => out.extend_from_slice(native),
            c => out.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes()),
        }
    }
}

/// `new Blob(blobParts, options)`: File API's "process blob parts", each
/// part's bytes in order (a string UTF-8, its line endings native first if
/// `endings` says; a buffer's as the front end copied them; a blob's), of
/// the options' type, normalized. An `endings` that isn't an
/// `EndingType`'s a `TypeError`.
pub(crate) fn construct<'gc>(
    cx: &ApiCx<'_, 'gc>,
    parts: Vec<BlobPart<'_, 'gc>>,
    options: &BlobPropertyBag,
) -> Result<Gc<'gc, Blob>, ApiError> {
    let native = match options.endings.as_str() {
        "transparent" => false,
        "native" => true,
        other => {
            return Err(ApiError::type_error(format!(
                "Failed to construct 'Blob': The provided value '{other}' is not a valid enum value of type EndingType."
            )));
        }
    };
    let mut bytes = Vec::new();
    for part in parts {
        match part {
            BlobPart::String(text) if native => to_native_line_endings(text, &mut bytes),
            BlobPart::String(text) => bytes.extend_from_slice(text.as_bytes()),
            BlobPart::Bytes(part) => bytes.extend_from_slice(part),
            BlobPart::Blob(blob) => bytes.extend_from_slice(cx.blob_data(blob).bytes()),
        }
    }
    Ok(cx.create_blob(BlobData::new(bytes, normalize_type(&options.r#type))))
}

/// `size`: a `double` holds it exactly (no blob is past 2^53 bytes).
pub(crate) fn size<'gc>(cx: &ApiCx<'_, 'gc>, this: Gc<'gc, Blob>) -> Result<f64, ApiError> {
    #[expect(
        clippy::cast_precision_loss,
        reason = "a blob's bytes, in memory, are far fewer than 2^53"
    )]
    Ok(cx.blob_data(this).bytes().len() as f64)
}

pub(crate) fn blob_type<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(DomString::from(cx.blob_data(this).type_())))
}

/// File API's relative position of a `slice()` argument in a blob of
/// `size` bytes: from the end if negative, clamped to the blob.
fn relative(position: i64, size: usize) -> usize {
    let size = i64::try_from(size).unwrap_or(i64::MAX);
    let position = if position < 0 {
        size.saturating_add(position).max(0)
    } else {
        position.min(size)
    };
    usize::try_from(position).unwrap_or(0)
}

/// `slice(start, end, contentType)`: a new blob of the bytes from `start`
/// to `end` (each from the end if negative), sharing them; of
/// `contentType`, normalized (none: empty).
pub(crate) fn slice<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
    start: Option<i64>,
    end: Option<i64>,
    content_type: Option<&str>,
) -> Result<Gc<'gc, Blob>, ApiError> {
    let data = cx.blob_data(this);
    let size = data.bytes().len();
    let start = start.map_or(0, |start| relative(start, size));
    let end = end.map_or(size, |end| relative(end, size));
    let type_ = content_type.map_or_else(String::new, normalize_type);
    Ok(cx.create_blob(data.slice(start, end, type_)))
}

/// `text()`: its bytes, UTF-8 decoded (a BOM dropped, what isn't UTF-8
/// U+FFFD: Encoding's "UTF-8 decode"), the promise resolved in a task
/// (`read`).
pub(crate) fn text<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
) -> Result<PromiseOf<'gc>, ApiError> {
    let bytes = cx.blob_data(this).bytes();
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let text = DomString::from(String::from_utf8_lossy(bytes).as_ref());
    read(op, cx, BlobRead::Text(text))
}

/// `arrayBuffer()`: a copy of its bytes, in a task.
pub(crate) fn array_buffer<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
) -> Result<PromiseOf<'gc>, ApiError> {
    let bytes = cx.blob_data(this).bytes().to_vec();
    read(op, cx, BlobRead::Bytes { bytes, view: false })
}

/// `bytes()`: a copy of its bytes, in a `Uint8Array`, in a task.
pub(crate) fn bytes<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Blob>,
) -> Result<PromiseOf<'gc>, ApiError> {
    let bytes = cx.blob_data(this).bytes().to_vec();
    read(op, cx, BlobRead::Bytes { bytes, view: true })
}

/// File API's "read the blob" (the bytes are all here: no streams): a new
/// promise resolved with `read` in a task of the window of the blob's
/// relevant realm (its wrapper's: `#[realm]`; `Cx::queue_blob_read`), so
/// that the microtasks queued before it run first, as in Chrome.
fn read<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    read: BlobRead,
) -> Result<PromiseOf<'gc>, ApiError> {
    let window = op
        .current_window(cx)
        .ok_or_else(|| ApiError::type_error("reading a blob needs a window"))?;
    Ok(PromiseOf::Pending(cx.queue_blob_read(window, read)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn types_are_normalized() {
        assert_eq!(
            normalize_type("Text/HTML;Charset=UTF-8"),
            "text/html;charset=utf-8"
        );
        assert_eq!(normalize_type("a\u{7f}"), "");
        assert_eq!(normalize_type("caf\u{e9}"), "");
        assert_eq!(normalize_type("\t"), "");
    }

    #[test]
    fn line_endings_become_native() {
        let native = if cfg!(windows) { "\r\n" } else { "\n" };
        let mut out = Vec::new();
        to_native_line_endings("a\r\nb\rc\nd\r\u{e9}", &mut out);
        assert_eq!(
            out,
            format!("a{native}b{native}c{native}d{native}\u{e9}").into_bytes()
        );
    }

    #[test]
    fn positions_count_from_the_end_if_negative() {
        assert_eq!(relative(2, 10), 2);
        assert_eq!(relative(12, 10), 10);
        assert_eq!(relative(-3, 10), 7);
        assert_eq!(relative(-30, 10), 0);
        assert_eq!(relative(i64::MIN, 10), 0);
    }
}
