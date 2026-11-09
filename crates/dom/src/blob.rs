//! File API's `Blob` (Blink's `core/fileapi/blob.cc`): bytes that never
//! change and a type; of no realm (no window keeps it). A `File` is a blob
//! with a name and a modification time too (its `FileInfo`), the same GC
//! type: `Blob`'s members take either, and its interface is the one its
//! data says.

use std::ops::Range;
use std::sync::Arc;

use elise::{External, Gc, GcObject, JsEdge};

use crate::{Cx, sites};

/// What a blob read resolves its promise with (`Cx::queue_blob_read`):
/// `text()`'s string, or `arrayBuffer()`'s bytes (`bytes()`'s, a
/// `Uint8Array` of them if `view`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlobRead {
    Text(crate::DomString),
    Bytes { bytes: Vec<u8>, view: bool },
}

impl<'gc> Cx<'_, 'gc> {
    /// A blob read of `window`'s realm (File API's "read the blob" of
    /// `text()`, `arrayBuffer()`, `bytes()`): its promise, of the number
    /// this gives, resolved with `read` in a task of `window`'s (Blink's
    /// `FileReaderLoader` answers in a task), after the microtasks queued
    /// before it.
    pub fn queue_blob_read(&self, window: Gc<'gc, crate::Window>, read: BlobRead) -> u64 {
        let promise = self.new_promise_number();
        let cell = window.into_ref().blob_reads.get();
        let mut reads = cell.take();
        reads.push((promise, read));
        cell.set(reads);
        let document = self.window_document(window);
        self.push_element_task(
            window,
            document,
            crate::names::NONE,
            crate::window::BLOB_READ,
            promise,
        );
        promise
    }

    /// The blob read whose promise is numbered `promise`, taken (its task
    /// runs).
    pub fn take_blob_read(&self, window: Gc<'gc, crate::Window>, promise: u64) -> Option<BlobRead> {
        let cell = window.into_ref().blob_reads.get();
        let mut reads = cell.take();
        let index = reads.iter().position(|(number, _)| *number == promise);
        let read = index.map(|index| reads.remove(index).1);
        cell.set(reads);
        read
    }
}

/// A `Blob`: its bytes and type.
#[derive(GcObject)]
pub struct Blob {
    data: External<BlobData>,
    wrapper: JsEdge,
}

/// A blob's bytes, a range of ones shared with the blob it was sliced from
/// (and its slices), and its type. Shared by an `Arc`: more than one blob
/// owns them, and a fetch's body, or a worker's message, takes them to
/// another thread (Blink's `BlobDataHandle`, which is thread safe for it).
#[derive(Debug, Clone)]
pub struct BlobData {
    bytes: Arc<[u8]>,
    range: Range<usize>,
    type_: String,
    /// A `File`'s.
    file: Option<FileInfo>,
}

/// What a `File` has beside its bytes and type: its name and its last
/// modification, in milliseconds since the Unix epoch.
#[derive(Debug, Clone, PartialEq)]
pub struct FileInfo {
    pub name: String,
    pub last_modified: i64,
}

impl BlobData {
    /// Of all of `bytes`, of type `type_` (normalized already).
    pub fn new(bytes: Vec<u8>, type_: String) -> Self {
        let range = 0..bytes.len();
        Self {
            bytes: bytes.into(),
            range,
            type_,
            file: None,
        }
    }

    /// Of all of `bytes`, shared (a fetch's body's), of type `type_`
    /// (normalized already).
    pub fn from_shared(bytes: Arc<[u8]>, type_: String) -> Self {
        let range = 0..bytes.len();
        Self {
            bytes,
            range,
            type_,
            file: None,
        }
    }

    /// A `File`'s: these bytes and type, with `file`'s name and time.
    pub fn with_file(self, file: FileInfo) -> Self {
        Self {
            file: Some(file),
            ..self
        }
    }

    /// A `File`'s name and time; none for a blob that isn't one.
    pub fn file(&self) -> Option<&FileInfo> {
        self.file.as_ref()
    }

    /// Its bytes, as a buffer of their own to share (the whole buffer, if
    /// they are all of it): what goes to the browser with a blob URL.
    pub fn shared_bytes(&self) -> Arc<[u8]> {
        if self.range == (0..self.bytes.len()) {
            self.bytes.clone()
        } else {
            self.bytes().into()
        }
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes[self.range.clone()]
    }

    pub fn type_(&self) -> &str {
        &self.type_
    }

    /// The bytes from `start` to `end` of this one's (clamped to them, none
    /// if `end` is before `start`), shared; of type `type_`.
    pub fn slice(&self, start: usize, end: usize, type_: String) -> Self {
        let len = self.range.len();
        let (start, end) = (start.min(len), end.min(len));
        let end = end.max(start);
        Self {
            bytes: self.bytes.clone(),
            range: self.range.start.saturating_add(start)..self.range.start.saturating_add(end),
            type_,
            // A slice of a `File` is a `Blob`.
            file: None,
        }
    }
}

impl<'gc> Cx<'_, 'gc> {
    /// A new `Blob` of `data`.
    pub fn create_blob(&self, data: BlobData) -> Gc<'gc, Blob> {
        self.gc.alloc_in(
            &sites::OTHER,
            BlobInit {
                data: External::new(data),
                wrapper: (),
            },
        )
    }

    pub fn blob_data(&self, blob: Gc<'gc, Blob>) -> &'gc BlobData {
        blob.into_ref().data.get()
    }

    pub(crate) fn blob_wrapper_edge(&self, blob: Gc<'gc, Blob>) -> &'gc JsEdge {
        &blob.into_ref().wrapper
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slice's range is within its blob's, which a slice of it is
    /// within in turn.
    #[test]
    fn slices_share_their_bytes() {
        let data = BlobData::new(b"abcdef".to_vec(), String::new());
        let slice = data.slice(1, 5, "x".into());
        assert_eq!(slice.bytes(), b"bcde");
        assert!(Arc::ptr_eq(&data.bytes, &slice.bytes));
        assert_eq!(slice.slice(2, 10, String::new()).bytes(), b"de");
        assert_eq!(slice.slice(3, 1, String::new()).bytes(), b"");
    }
}
