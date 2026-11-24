//! A committed document's body as it comes through its pipe (Blink's
//! `NavigationBodyLoader`, which the document's loader owns): read in tasks
//! the pipe's watcher posts, decoded a piece at a time, and handed to the
//! document's parser, whether it parses or waits for a script.

use bytes::Bytes;
use ipc::data_pipe::{Consumer, Read};
use protocol::FrameToken;
use protocol::network::CompletionStatus;
use protocol::renderer::{HistoryEntryParams, NavigationLoad, FrameLoadType};

use crate::document::MarkupFilter;
use crate::encoding::DocumentDecoder;

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
/// ended: its pipe, its decoder, and what came so far.
pub(super) struct DocumentBody {
    /// The rest of it, until the pipe is over.
    pipe: Option<Consumer>,
    decoder: DocumentDecoder,
    markup: MarkupFilter,
    /// Its load as far as its head, with its end once in.
    loaded: Option<Box<NavigationLoad>>,
    /// The URL it was loaded from, as the log names it.
    url: String,
    /// Its bytes so far, in the pieces they came in (what DevTools'
    /// `Network.getResponseBody` gives once it is over), and their text
    /// (the document's source, `Page.getResourceContent`'s).
    bytes: Vec<Bytes>,
    text: String,
}

/// What reading a document's body's pipe came to.
pub(super) enum BodyRead {
    /// That many bytes, and the markup they make.
    Data(usize, String),
    /// Nothing for now: its watcher is to be told when there is more.
    Waiting,
    /// The pipe is over: the markup the decoder still held.
    Over(String),
}

/// A document's body once it has all come and its load has ended.
pub(super) struct FinishedBody {
    pub(super) loaded: Option<Box<NavigationLoad>>,
    pub(super) url: String,
    pub(super) bytes: Bytes,
    pub(super) text: String,
}

impl DocumentBody {
    /// The body of a document of `mime_type` whose transport said
    /// `charset`, decoded as `body`'s start says, and that start, none of
    /// it taken yet.
    pub(super) fn new(
        mime_type: &str,
        charset: Option<&str>,
        body: NavigationBody,
    ) -> (Self, Bytes) {
        let NavigationBody {
            start,
            pipe,
            loaded,
            url,
        } = body;
        let decoder = DocumentDecoder::new(mime_type, charset, &start, pipe.is_some());
        let body = Self {
            pipe,
            decoder,
            markup: MarkupFilter::new(mime_type),
            loaded,
            url,
            bytes: Vec::new(),
            text: String::new(),
        };
        (body, start)
    }

    /// What its text is read in.
    pub(super) fn encoding(&self) -> &'static encoding_rs::Encoding {
        self.decoder.encoding()
    }

    /// The markup of its next piece, `bytes` (`last`: the end of it), kept
    /// as it came.
    pub(super) fn take(&mut self, bytes: Bytes, last: bool) -> String {
        let text = self.decoder.decode(&bytes, last);
        let markup = self.markup.push(&text, last);
        self.text.push_str(&text);
        if !bytes.is_empty() {
            self.bytes.push(bytes);
        }
        markup
    }

    /// Takes the next piece its pipe has, at most `max` bytes.
    pub(super) fn read(&mut self, max: usize) -> BodyRead {
        let Some(pipe) = &mut self.pipe else {
            return BodyRead::Over(String::new());
        };
        match pipe.read(max) {
            Read::Data(data) => {
                let length = data.len();
                BodyRead::Data(length, self.take(data, false))
            }
            Read::Empty => BodyRead::Waiting,
            Read::Done => {
                self.pipe = None;
                BodyRead::Over(self.take(Bytes::new(), true))
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

    /// Its text so far.
    pub(super) fn text(&self) -> &str {
        &self.text
    }

    pub(super) fn finish(self) -> FinishedBody {
        FinishedBody {
            loaded: self.loaded,
            url: self.url,
            bytes: concat(self.bytes),
            text: self.text,
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

/// A document's markup its parser hasn't been given yet, in the pieces it
/// came in: given a slice at a time, so the parser can stop between them
/// (Blink's parser takes its input a token at a time).
#[derive(Default)]
pub(super) struct ParserInput {
    pieces: std::collections::VecDeque<String>,
    /// How much of the first piece was given.
    at: usize,
}

impl ParserInput {
    pub(super) fn push(&mut self, markup: String) {
        if !markup.is_empty() {
            self.pieces.push_back(markup);
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// `give` the next slice: at most `max` bytes of whole characters (one
    /// character, if it is longer). False if there is none.
    pub(super) fn give(&mut self, max: usize, give: impl FnOnce(&str)) -> bool {
        let Some(first) = self.pieces.front() else {
            return false;
        };
        let mut end = self.at.saturating_add(max).min(first.len());
        while !first.is_char_boundary(end) {
            end = end.saturating_sub(1);
        }
        if end <= self.at {
            end = first[self.at..]
                .chars()
                .next()
                .map_or(first.len(), |c| self.at.saturating_add(c.len_utf8()));
        }
        give(&first[self.at..end]);
        if end == first.len() {
            self.pieces.pop_front();
            self.at = 0;
        } else {
            self.at = end;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Given in slices of whole characters, the pieces' text as it was.
    #[test]
    fn input_is_given_in_slices_of_whole_characters() {
        let mut input = ParserInput::default();
        input.push("ab中".to_owned());
        input.push(String::new());
        input.push("cdef".to_owned());
        let mut slices = Vec::new();
        while input.give(3, |slice| slices.push(slice.to_owned())) {}
        assert_eq!(slices, ["ab", "中", "cde", "f"]);
        assert!(input.is_empty());
        // Narrower than a character: the character.
        input.push("中".to_owned());
        assert!(input.give(1, |slice| assert_eq!(slice, "中")));
        assert!(!input.give(1, |_| unreachable!()));
    }
}
