//! The bodies of the documents the browser sent to commit, as they come
//! through their pipes (Blink's `NavigationBodyLoader`): a document commits
//! once its body is all in and its load has ended, and is parsed whole.

use bytes::Bytes;
use ipc::data_pipe::{Consumer, Read};
use protocol::FrameToken;
use protocol::network::CompletionStatus;
use protocol::renderer::{HistoryEntryParams, NavigationLoad, NavigationTimingType};

/// What `CommitNavigation` brought, but the body.
pub(super) struct NavigationCommit {
    pub(super) frame: FrameToken,
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
    pub(super) navigation_type: NavigationTimingType,
}

/// A commit whose body is still coming, or whose load hasn't ended.
pub(super) struct BodyLoader {
    commit: NavigationCommit,
    /// The body so far, in the pieces it came in.
    body: Vec<Bytes>,
    /// The rest of it, until the pipe is over.
    pipe: Option<Consumer>,
}

impl BodyLoader {
    pub(super) fn new(commit: NavigationCommit, body: Bytes, pipe: Option<Consumer>) -> Self {
        Self {
            commit,
            body: vec![body],
            pipe,
        }
    }

    pub(super) fn frame(&self) -> FrameToken {
        self.commit.frame
    }

    pub(super) fn navigation_id(&self) -> u64 {
        self.commit.navigation_id
    }

    /// Takes what its pipe has: until it is empty, which asks for its
    /// watcher to be called once it has more, or over.
    pub(super) fn read(&mut self) {
        let Some(pipe) = &mut self.pipe else {
            return;
        };
        loop {
            match pipe.read(usize::MAX) {
                Read::Data(data) => self.body.push(data),
                Read::Empty => return,
                Read::Done => {
                    self.pipe = None;
                    return;
                }
            }
        }
    }

    /// Its pipe, to watch, while it has one.
    pub(super) fn pipe_mut(&mut self) -> Option<&mut Consumer> {
        self.pipe.as_mut()
    }

    /// Its load ended as `status`.
    pub(super) fn completed(&mut self, status: CompletionStatus) {
        if let Some(loaded) = &mut self.commit.loaded {
            loaded.end = Some(status);
        }
    }

    /// Its body is all in, and its load has ended if it had one: it may
    /// commit.
    pub(super) fn is_ready(&self) -> bool {
        self.pipe.is_none()
            && self
                .commit
                .loaded
                .as_ref()
                .is_none_or(|loaded| loaded.end.is_some())
    }

    /// The commit and its whole body.
    pub(super) fn finish(self) -> (NavigationCommit, Bytes) {
        (self.commit, concat(self.body))
    }
}

/// `pieces` as one, copying only if there is more than one.
fn concat(mut pieces: Vec<Bytes>) -> Bytes {
    pieces.retain(|piece| !piece.is_empty());
    match pieces.len() {
        0 => Bytes::new(),
        1 => pieces.pop().unwrap_or_default(),
        _ => Bytes::from(pieces.concat()),
    }
}

/// The commits waiting for their bodies, at most one a frame: the latest
/// the browser sent it.
#[derive(Default)]
pub(super) struct BodyLoaders(Vec<BodyLoader>);

impl BodyLoaders {
    /// `loader` waits for its body, in place of one its frame had, which
    /// the browser sent before (that commit goes, and its pipe with it:
    /// the browser sends a frame no commit of a navigation it replaced).
    pub(super) fn wait(&mut self, loader: BodyLoader) {
        let frame = loader.frame();
        self.0.retain(|other| other.frame() != frame);
        self.0.push(loader);
    }

    /// A commit the browser sent `frame` after navigation `navigation_id`'s
    /// commits now: the frame waits for no earlier one.
    pub(super) fn forget_before(&mut self, frame: FrameToken, navigation_id: u64) {
        self.0
            .retain(|loader| loader.frame() != frame || loader.navigation_id() > navigation_id);
    }

    /// The commit of navigation `navigation_id` in `frame` waiting for its
    /// body, if it is.
    pub(super) fn get_mut(&mut self, frame: FrameToken, navigation_id: u64) -> Option<&mut BodyLoader> {
        self.0
            .iter_mut()
            .find(|loader| loader.frame() == frame && loader.navigation_id() == navigation_id)
    }

    /// The commit of navigation `navigation_id` in `frame`, taken out if it
    /// is ready.
    pub(super) fn take_ready(&mut self, frame: FrameToken, navigation_id: u64) -> Option<BodyLoader> {
        let at = self.0.iter().position(|loader| {
            loader.frame() == frame && loader.navigation_id() == navigation_id && loader.is_ready()
        })?;
        Some(self.0.swap_remove(at))
    }
}
