//! A committed document's body as it comes through its pipe (Blink's
//! `NavigationBodyLoader`, which the document's loader owns): read in tasks
//! the pipe's watcher posts, and handed to the engine's loading of the
//! document (`dom_api::DocumentLoader`), which decodes and parses it.

use bytes::Bytes;
use ipc::data_pipe::{Consumer, Read};
use protocol::FrameToken;
use protocol::network::CompletionStatus;
use protocol::renderer::{FrameLoadType, HistoryEntryParams, NavigationLoad};

/// What `CommitNavigation` brought, but the body.
pub(super) struct NavigationCommit {
    pub(super) frame: FrameToken,
    /// The loader the browser gave the document.
    pub(super) loader: u64,
    pub(super) navigation_id: u64,
    pub(super) begin_id: Option<u64>,
    pub(super) url: String,
    pub(super) error_page: bool,
    pub(super) mime_type: String,
    pub(super) charset: Option<String>,
    pub(super) content_language: Option<String>,
    pub(super) history: HistoryEntryParams,
    pub(super) navigation_start: ipc::TimeTicks,
    /// Its load as far as its head, with its end once it has one.
    pub(super) loaded: Option<Box<NavigationLoad>>,
    pub(super) navigation_type: FrameLoadType,
    /// The main frame's page before may stay shown till it is drawn.
    pub(super) paint_holding: bool,
}

/// A document's body as a commit brings it: its start (all of it, for one
/// with no pipe), the pipe the rest comes through, its load (none for a
/// document not loaded: an `about:` one, an error page) and the URL it was
/// loaded from.
pub(super) struct NavigationBody {
    pub(super) start: Bytes,
    pub(super) pipe: Option<Consumer>,
    pub(super) loaded: Option<Box<NavigationLoad>>,
    pub(super) url: String,
}

/// The most of a document's body one task reads (Chromium's
/// `kMaxNumConsumedBytesInTask`): the rest is read in the next.
pub(super) const BYTES_PER_TASK: usize = 1024 * 1024;

/// A committed document's body until it has all come and its load has
/// ended: its pipe, and what came so far.
pub(super) struct DocumentBody {
    /// The rest of it, until the pipe is over.
    pipe: Option<Consumer>,
    /// Its load as far as its head, with its end once in.
    loaded: Option<Box<NavigationLoad>>,
    /// The URL it was loaded from, as the log names it.
    url: String,
    /// Its bytes so far, in the pieces they came in (what DevTools'
    /// `Network.getResponseBody` gives once it is over).
    bytes: Vec<Bytes>,
}

/// What reading a document's body's pipe came to.
pub(super) enum BodyRead {
    /// These bytes.
    Data(Bytes),
    /// Nothing for now: its watcher is to be told when there is more.
    Waiting,
    /// The pipe is over now: the end of the body.
    Over,
    /// The pipe was over before (or there was none): its end was told.
    Ended,
}

/// A document's body once it has all come and its load has ended.
pub(super) struct FinishedBody {
    pub(super) loaded: Option<Box<NavigationLoad>>,
    pub(super) url: String,
    pub(super) bytes: Bytes,
}

impl DocumentBody {
    /// The body `body` brings, and its start, none of it taken yet.
    pub(super) fn new(body: NavigationBody) -> (Self, Bytes) {
        let NavigationBody {
            start,
            pipe,
            loaded,
            url,
        } = body;
        let body = Self {
            pipe,
            loaded,
            url,
            bytes: Vec::new(),
        };
        (body, start)
    }

    /// Its next piece, `bytes`, kept as it came.
    pub(super) fn take(&mut self, bytes: &Bytes) {
        if !bytes.is_empty() {
            self.bytes.push(bytes.clone());
        }
    }

    /// Takes the next piece its pipe has, at most `max` bytes.
    pub(super) fn read(&mut self, max: usize) -> BodyRead {
        let Some(pipe) = &mut self.pipe else {
            return BodyRead::Ended;
        };
        match pipe.read(max) {
            Read::Data(data) => {
                self.take(&data);
                BodyRead::Data(data)
            }
            Read::Empty => BodyRead::Waiting,
            Read::Done => {
                self.pipe = None;
                BodyRead::Over
            }
        }
    }

    /// Its pipe, to watch, while it has one.
    pub(super) fn pipe_mut(&mut self) -> Option<&mut Consumer> {
        self.pipe.as_mut()
    }

    /// Its load, if it was loaded.
    pub(super) fn loaded(&self) -> Option<&NavigationLoad> {
        self.loaded.as_deref()
    }

    /// Its load ended as `status`.
    pub(super) fn completed(&mut self, status: CompletionStatus) {
        if let Some(loaded) = &mut self.loaded {
            loaded.end = Some(status);
        }
    }

    /// It has all come, and its load has ended if it had one.
    pub(super) fn is_over(&self) -> bool {
        self.pipe.is_none()
            && self
                .loaded
                .as_ref()
                .is_none_or(|loaded| loaded.end.is_some())
    }

    pub(super) fn finish(self) -> FinishedBody {
        FinishedBody {
            loaded: self.loaded,
            url: self.url,
            bytes: concat(self.bytes),
        }
    }
}

/// `pieces` as one, copying only if there is more than one.
fn concat(mut pieces: Vec<Bytes>) -> Bytes {
    match pieces.len() {
        0 => Bytes::new(),
        1 => pieces.pop().unwrap_or_default(),
        _ => Bytes::from(pieces.concat()),
    }
}
