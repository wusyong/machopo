//! A frame's navigation on its way (Chromium's `NavigationRequest`): its
//! load, and how its document commits once it is in. Its frame's node holds
//! it (`FrameTreeNode::navigation_request_`), so it goes with the frame.

use bytes::Bytes;
use ipc::data_pipe::Consumer;
use net::LoadHandle;
use protocol::network::{CompletionStatus, NetError, NetErrorCode, ResponseHead};
use protocol::renderer::RedirectHop;

use crate::render_process_host::{Committer, LoadCompleter};

/// A navigation loading: what the browser's navigation `id` is, the
/// renderer's number for it if the renderer asked for it (`begin_id`),
/// and its load.
pub(crate) struct NavigationRequest {
    pub(crate) id: u64,
    pub(crate) begin_id: Option<u64>,
    pub(crate) url: String,
    /// The redirects it followed, for Navigation Timing.
    pub(crate) redirects: Vec<RedirectHop>,
    pub(crate) committer: Committer,
    pub(crate) load: Loading,
    /// Its response, once its head has come, and the start of its body
    /// read so far: it commits with the head and the body's first bytes.
    pub(crate) response: Option<NavigationResponse>,
    /// How its load ended, if it has before it committed.
    pub(crate) end: Option<CompletionStatus>,
}

/// A navigation's response before it commits: the head, the body's pipe,
/// and what has been read of it (Chromium reads the start this way only to
/// sniff, in `MimeSniffingThrottle`).
pub(crate) struct NavigationResponse {
    pub(crate) head: ResponseHead,
    pub(crate) body: Consumer,
    /// The body's first bytes, in the pieces they came in.
    pub(crate) start: Vec<Bytes>,
    pub(crate) start_length: usize,
    /// The pipe is over: `start` is the whole body.
    pub(crate) done: bool,
}

/// The load of a document committed before it ended: kept until it does,
/// when the renderer hears how (Chromium hands the renderer the loader
/// itself, `URLLoaderClientEndpoints`), or until another document commits
/// in the frame, or the frame goes, which stops it: the renderer hears it
/// was aborted then, as it would of a load it cancelled.
pub(crate) struct DocumentLoad {
    navigation_id: u64,
    /// Held for what it does as it goes: the load stops.
    _load: Loading,
    completer: Option<LoadCompleter>,
}

impl DocumentLoad {
    pub(crate) fn new(navigation_id: u64, load: Loading, completer: LoadCompleter) -> Self {
        Self {
            navigation_id,
            _load: load,
            completer: Some(completer),
        }
    }

    pub(crate) fn navigation_id(&self) -> u64 {
        self.navigation_id
    }

    /// It ended as `status`: the renderer hears.
    pub(crate) fn complete(mut self, status: CompletionStatus) {
        if let Some(completer) = self.completer.take() {
            completer.complete(status);
        }
    }
}

impl Drop for DocumentLoad {
    fn drop(&mut self) {
        // Stopped before it ended (its load cancelled as `load` goes).
        if let Some(completer) = self.completer.take() {
            completer.complete(CompletionStatus {
                error: Some(NetError::new(
                    NetErrorCode::Aborted,
                    "the document's load stopped",
                )),
                encoded_body_length: 0,
                completion_time: ipc::TimeTicks::now(),
            });
        }
    }
}

/// A navigation's load: none for a `blob:` URL's, answered at once. It
/// stops as the navigation goes (replaced, its frame gone): left at a
/// redirect, it would wait for good (Chromium's `NavigationURLLoader` goes
/// with its `NavigationRequest`).
#[derive(Default)]
pub(crate) struct Loading(Option<LoadHandle>);

impl Loading {
    pub(crate) fn set(&mut self, load: LoadHandle) {
        if let Some(before) = self.0.replace(load) {
            before.cancel();
        }
    }

    /// Lets it go on past the redirect it waits at.
    pub(crate) fn follow_redirect(&self) {
        if let Some(load) = &self.0 {
            load.follow_redirect();
        }
    }
}

impl Drop for Loading {
    fn drop(&mut self) {
        // One over already isn't called back again: nothing happens.
        if let Some(load) = self.0.take() {
            load.cancel();
        }
    }
}
