//! A frame's navigation on its way (Chromium's `NavigationRequest`): its
//! load, and how its document commits once it is in. Its frame's node holds
//! it (`FrameTreeNode::navigation_request_`), so it goes with the frame.

use net::LoadHandle;
use protocol::renderer::RedirectHop;

use crate::render_process_host::Committer;

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
