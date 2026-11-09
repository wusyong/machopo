//! File API's `FileReader` (Blink's `core/fileapi/file_reader.cc`): an
//! event target with a read's state, the blob it reads (whose bytes are
//! already in memory) and its result; each step of the read a task of its
//! window's (`ElementTaskKind::FileRead`, in the window's element task
//! queue, its target the reader). And `ProgressEvent`'s data.

use std::cell::Cell;

use elise::{Gc, GcObject, JsEdge, LocalExternal, Member};

use crate::{BlobData, Cx, Event, EventTargetImpl, Window, sites};

/// A reader's `readyState`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileReaderState {
    Empty,
    Loading,
    Done,
}

/// What a read makes of the bytes (File API's "package data"'s type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadAs {
    ArrayBuffer,
    BinaryString,
    DataUrl,
    /// Decoded, by the encoding the label names, if one does.
    Text(Option<String>),
}

/// Where a read is: the task to come.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadStep {
    LoadStart,
    Progress,
    Load,
}

/// A read's result.
#[derive(Debug, Clone, Default)]
pub enum FileReaderResult {
    #[default]
    None,
    Text(String),
    Bytes(std::sync::Arc<[u8]>),
}

/// A read going on: its blob, and what it makes of it.
type Reading = Option<(BlobData, ReadAs)>;

#[derive(GcObject)]
pub struct FileReaderData {
    /// The window whose element task queue its reads' tasks go to.
    window: Member<Window>,
    state: Cell<u8>,
    /// Its read's number: a task of an earlier one (aborted, or begun anew)
    /// does nothing.
    read: Cell<u64>,
    /// The read's blob and what it makes of it; taken and put back.
    reading: LocalExternal<Cell<Reading>>,
    /// The last read's result; taken and put back.
    result: LocalExternal<Cell<FileReaderResult>>,
    /// An `ArrayBuffer` result's buffer, the engine's (the same one each
    /// time for a read, as Blink's loader keeps it), `fresh` once it is the
    /// result's.
    buffer: JsEdge,
    fresh: Cell<bool>,
}

#[derive(GcObject)]
pub struct ProgressEventData {
    length_computable: Cell<bool>,
    loaded: Cell<f64>,
    total: Cell<f64>,
}

const EMPTY: u8 = 0;
const LOADING: u8 = 1;
const DONE: u8 = 2;

impl<'gc> Cx<'_, 'gc> {
    /// A new `FileReader` of `window`'s.
    pub fn create_file_reader(&self, window: Gc<'gc, Window>) -> Gc<'gc, EventTargetImpl> {
        let target = self.create_event_target();
        let data = self.gc.alloc_in(
            &sites::OTHER,
            FileReaderDataInit {
                window: Some(window),
                state: Cell::new(EMPTY),
                read: Cell::new(0),
                reading: LocalExternal::new(Cell::new(None)),
                result: LocalExternal::new(Cell::new(FileReaderResult::None)),
                buffer: (),
                fresh: Cell::new(false),
            },
        );
        target.file_reader.set(self.gc, Some(data));
        target
    }

    pub fn is_file_reader(&self, target: Gc<'gc, EventTargetImpl>) -> bool {
        target.file_reader.get(self.gc).is_some()
    }

    /// The window a `FileReader` was made in; none for another target.
    pub fn file_reader_window(&self, target: Gc<'gc, EventTargetImpl>) -> Option<Gc<'gc, Window>> {
        target.file_reader.get(self.gc)?.window.get(self.gc)
    }

    fn reader(&self, reader: Gc<'gc, EventTargetImpl>) -> Gc<'gc, FileReaderData> {
        reader.file_reader.get(self.gc).expect("a file reader")
    }

    pub fn file_reader_state(&self, reader: Gc<'gc, EventTargetImpl>) -> FileReaderState {
        match self.reader(reader).state.get() {
            EMPTY => FileReaderState::Empty,
            LOADING => FileReaderState::Loading,
            _ => FileReaderState::Done,
        }
    }

    /// A read of `data` starts: `LOADING`, the result `null`, its first
    /// task queued. Its number.
    pub fn start_file_reader_read(
        &self,
        reader: Gc<'gc, EventTargetImpl>,
        data: BlobData,
        how: ReadAs,
    ) -> u64 {
        let state = self.reader(reader);
        let read = state.read.get().wrapping_add(1);
        state.read.set(read);
        state.state.set(LOADING);
        state.reading.set(Some((data, how)));
        state.result.set(FileReaderResult::None);
        state.fresh.set(false);
        self.queue_file_read_step(reader, read, ReadStep::LoadStart);
        read
    }

    /// Queues `read`'s `step`, a task of the reader's window's.
    pub fn queue_file_read_step(
        &self,
        reader: Gc<'gc, EventTargetImpl>,
        read: u64,
        step: ReadStep,
    ) {
        let Some(window) = self.reader(reader).window.get(self.gc) else {
            return;
        };
        let step = match step {
            ReadStep::LoadStart => 0,
            ReadStep::Progress => 1,
            ReadStep::Load => 2,
        };
        self.push_target_task(
            window,
            reader,
            crate::window::FILE_READ,
            read.wrapping_shl(2) | step,
        );
    }

    /// Whether `read` is the reader's, going on.
    pub fn file_reader_reading(&self, reader: Gc<'gc, EventTargetImpl>, read: u64) -> bool {
        let state = self.reader(reader);
        state.state.get() == LOADING && state.read.get() == read
    }

    /// The blob the reader reads and what it makes of it.
    pub fn file_reader_read(&self, reader: Gc<'gc, EventTargetImpl>) -> Option<(BlobData, ReadAs)> {
        let cell = &*self.reader(reader).into_ref().reading;
        let reading = cell.take();
        cell.set(reading.clone());
        reading
    }

    /// `read` done with `result`, if it still goes on: `DONE`. Whether it
    /// did.
    pub fn finish_file_reader_read(
        &self,
        reader: Gc<'gc, EventTargetImpl>,
        read: u64,
        result: FileReaderResult,
    ) -> bool {
        if !self.file_reader_reading(reader, read) {
            return false;
        }
        let state = self.reader(reader);
        state.state.set(DONE);
        state.reading.set(None);
        state.result.set(result);
        state.fresh.set(false);
        true
    }

    /// `abort()`'s: a read going on stops (`DONE`, a `null` result; its
    /// tasks do nothing, as it isn't `LOADING`, and a read begun after has
    /// a number of its own). Whether one did.
    pub fn abort_file_reader(&self, reader: Gc<'gc, EventTargetImpl>) -> bool {
        let state = self.reader(reader);
        if state.state.get() != LOADING {
            return false;
        }
        state.state.set(DONE);
        state.reading.set(None);
        state.result.set(FileReaderResult::None);
        state.fresh.set(false);
        true
    }

    /// The reader's result.
    pub fn file_reader_result(&self, reader: Gc<'gc, EventTargetImpl>) -> FileReaderResult {
        let cell = &*self.reader(reader).into_ref().result;
        let result = cell.take();
        cell.set(result.clone());
        result
    }

    /// An `ArrayBuffer` result's buffer (the engine's), and whether it is
    /// the result's yet.
    pub fn file_reader_buffer_edge(&self, reader: Gc<'gc, EventTargetImpl>) -> (&'gc JsEdge, bool) {
        let state = self.reader(reader);
        (&state.into_ref().buffer, state.fresh.get())
    }

    /// The buffer the engine stored is the result's.
    pub fn set_file_reader_buffer_fresh(&self, reader: Gc<'gc, EventTargetImpl>) {
        self.reader(reader).fresh.set(true);
    }

    /// Makes `event` a `ProgressEvent`.
    pub fn set_progress_event_data(
        &self,
        event: Gc<'gc, Event>,
        length_computable: bool,
        loaded: f64,
        total: f64,
    ) {
        let data = self.gc.alloc_in(
            &sites::OTHER,
            ProgressEventDataInit {
                length_computable: Cell::new(length_computable),
                loaded: Cell::new(loaded),
                total: Cell::new(total),
            },
        );
        event.data.set(self.gc, Some(data.erase()));
    }

    /// `event`'s `lengthComputable`, `loaded` and `total`, if it is a
    /// `ProgressEvent`.
    pub fn progress_event_data(&self, event: Gc<'gc, Event>) -> Option<(bool, f64, f64)> {
        let data = event.data.get(self.gc)?.downcast::<ProgressEventData>()?;
        Some((
            data.length_computable.get(),
            data.loaded.get(),
            data.total.get(),
        ))
    }
}
