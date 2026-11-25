//! What the renderer has of each frame (Chromium's `RenderFrameImpl`, which
//! `g_frame_map` has one of a frame): one table, the top-level frame's and
//! the child frames' alike, keyed by the engine's numbers for them. The
//! engine numbers frames as it makes them, the top-level one first, so the
//! table goes in the order they were made. Which frames there are, their
//! parents, iframes and tokens are the engine's
//! (`RenderThread::child_parent`, `child_token`); the renderer follows them
//! (`follow_child_frames`).

use std::collections::{BTreeMap, VecDeque};

use dom::NodeRoot;
use dom_api::FrameId;
use url::Url;

use super::documents::FrameDocument;

/// The renderer's frames.
#[derive(Default)]
pub(super) struct RenderFrames {
    frames: BTreeMap<FrameId, RenderFrame>,
    /// The top-level frame, once a document committed in it.
    top: Option<FrameId>,
    /// The browser's token for the top-level frame (`DevToolsMainFrameToken`):
    /// its frame's, whatever document or engine it has, so it outlasts
    /// them (a child frame's is the engine's).
    top_token: Option<protocol::FrameToken>,
    /// The child frames' `srcdoc` and `about:` documents, which need no
    /// fetch but commit in a task of their own all the same, as a
    /// response's would (Chrome's goes through a navigation request too):
    /// each one's frame, URL and markup, in the order queued across the
    /// frames (`RenderThread::run_child_commit`).
    commits: VecDeque<(FrameId, Url, String)>,
}

/// What the renderer has of one frame.
pub(super) struct RenderFrame {
    /// The document loading or loaded in it: a child frame's from as it is
    /// made (its first `about:blank`), the top-level frame's from its first
    /// commit.
    document: Option<FrameDocument>,
    /// The navigation it asked the browser for (`BeginNavigation`), until
    /// the browser commits or drops it: one a frame, the latest.
    navigation: Option<NavigationRequest>,
    /// When its iframe fires `load` for a navigation within its document
    /// that another origin asked for (`owner_load_later`), on the clock.
    owner_load_at: Option<f64>,
}

/// A navigation the renderer asked the browser for: its number for it
/// (`BeginNavigation`'s id), whether its iframe's referrer policy was
/// `no-referrer` (what its navigation timing reads of its response's
/// `Timing-Allow-Origin`; never for the top-level frame), and, for one to
/// `about:blank`, the initiator's document, which its document has its
/// origin from (`about_blank_origin_source`).
pub(super) struct NavigationRequest {
    pub(super) begin_id: u64,
    pub(super) no_referrer: bool,
    pub(super) origin_source: Option<NodeRoot>,
}

impl RenderFrame {
    fn with(document: Option<FrameDocument>) -> Self {
        Self {
            document,
            navigation: None,
            owner_load_at: None,
        }
    }
}

impl RenderFrames {
    /// The top-level frame, if a document committed in it.
    pub(super) fn top(&self) -> Option<FrameId> {
        self.top
    }

    /// The top-level frame's document.
    pub(super) fn top_document(&self) -> Option<&FrameDocument> {
        self.document(self.top?)
    }

    pub(super) fn top_document_mut(&mut self) -> Option<&mut FrameDocument> {
        let top = self.top?;
        self.document_mut(top)
    }

    /// `frame`'s document, if the renderer has the frame.
    pub(super) fn document(&self, frame: FrameId) -> Option<&FrameDocument> {
        self.frames.get(&frame)?.document.as_ref()
    }

    pub(super) fn document_mut(&mut self, frame: FrameId) -> Option<&mut FrameDocument> {
        self.frames.get_mut(&frame)?.document.as_mut()
    }

    /// Whether the renderer has `frame`.
    pub(super) fn contains(&self, frame: FrameId) -> bool {
        self.frames.contains_key(&frame)
    }

    /// The frames' documents: the top-level one's, then the children's in
    /// the order their frames were made.
    pub(super) fn documents(&self) -> impl Iterator<Item = &FrameDocument> {
        self.top_document()
            .into_iter()
            .chain(self.child_documents())
    }

    /// The child frames' documents, in the order their frames were made.
    pub(super) fn child_documents(&self) -> impl Iterator<Item = &FrameDocument> {
        let top = self.top;
        self.frames
            .iter()
            .filter(move |(frame, _)| Some(**frame) != top)
            .filter_map(|(_, next)| next.document.as_ref())
    }

    /// The frames, the top-level one first.
    pub(super) fn ids(&self) -> Vec<FrameId> {
        self.top
            .into_iter()
            .chain(self.children())
            .collect()
    }

    /// The child frames, in the order they were made.
    pub(super) fn children(&self) -> impl Iterator<Item = FrameId> + '_ {
        self.frames
            .keys()
            .copied()
            .filter(move |frame| Some(*frame) != self.top)
    }

    /// `document` committed in the top-level frame `top`: the frames of the
    /// document before go with it, and their commits queued. The
    /// navigation it asked for stays, if this commit wasn't it (the
    /// browser drops it).
    pub(super) fn commit_top(&mut self, top: FrameId, document: FrameDocument) {
        let navigation = self
            .frames
            .remove(&top)
            .and_then(|before| before.navigation);
        self.frames.clear();
        self.commits.clear();
        self.frames.insert(
            top,
            RenderFrame {
                navigation,
                ..RenderFrame::with(Some(document))
            },
        );
        self.top = Some(top);
    }

    /// `document` is child frame `frame`'s: its first, or one committed in
    /// it in place of the one before.
    pub(super) fn set_child_document(&mut self, frame: FrameId, document: FrameDocument) {
        debug_assert!(
            Some(frame) != self.top,
            "the top-level frame's commits by `commit_top`"
        );
        self.frames
            .entry(frame)
            .or_insert_with(|| RenderFrame::with(None))
            .document = Some(document);
    }

    /// The frames `gone` (a frame and those in it) gone, and what the
    /// renderer had of them.
    pub(super) fn remove(&mut self, gone: &[FrameId]) {
        self.frames.retain(|frame, _| !gone.contains(frame));
        self.commits.retain(|(frame, _, _)| !gone.contains(frame));
    }

    /// Every frame gone (the engine replaced, its DOM with it); the
    /// browser's token for the top-level frame stays.
    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.top = None;
        self.commits.clear();
    }

    /// The browser's token for the top-level frame.
    pub(super) fn top_token(&self) -> Option<protocol::FrameToken> {
        self.top_token
    }

    pub(super) fn set_top_token(&mut self, token: protocol::FrameToken) {
        self.top_token = Some(token);
    }

    /// `frame`'s navigation `request` went to the browser: the one it asked
    /// for before, and a commit queued, no longer commit (one navigation a
    /// frame).
    pub(super) fn navigation_sent(&mut self, frame: FrameId, request: NavigationRequest) {
        self.navigating(frame);
        if let Some(next) = self.frames.get_mut(&frame) {
            next.navigation = Some(request);
        }
    }

    /// The navigation `frame` asked the browser for and still waits for.
    pub(super) fn navigation(&self, frame: FrameId) -> Option<&NavigationRequest> {
        self.frames.get(&frame)?.navigation.as_ref()
    }

    /// `frame`'s navigation `begin_id`, taken as the browser commits or
    /// drops it: none if it asked for another since.
    pub(super) fn take_navigation(
        &mut self,
        frame: FrameId,
        begin_id: u64,
    ) -> Option<NavigationRequest> {
        self.frames
            .get_mut(&frame)?
            .navigation
            .take_if(|asked| asked.begin_id == begin_id)
    }

    /// `frame`'s navigation, whichever it asked for, taken: one the browser
    /// started commits in its place.
    pub(super) fn take_any_navigation(&mut self, frame: FrameId) -> Option<NavigationRequest> {
        self.frames.get_mut(&frame)?.navigation.take()
    }

    /// Child frame `frame`'s document of `markup` at `url` queued to commit
    /// (`commits`): its navigation before no longer commits.
    pub(super) fn queue_commit(&mut self, frame: FrameId, url: Url, markup: String) {
        self.navigating(frame);
        self.commits.push_back((frame, url, markup));
    }

    /// The first of the commits queued, taken.
    pub(super) fn next_commit(&mut self) -> Option<(FrameId, Url, String)> {
        self.commits.pop_front()
    }

    /// Whether `frame` has a navigation on its way: the browser's, or a
    /// commit queued.
    pub(super) fn is_navigating(&self, frame: FrameId) -> bool {
        self.navigation(frame).is_some() || self.commits.iter().any(|(id, _, _)| *id == frame)
    }

    /// `frame` navigates anew: what it asked for before no longer commits.
    fn navigating(&mut self, frame: FrameId) {
        if let Some(next) = self.frames.get_mut(&frame) {
            next.navigation = None;
        }
        self.commits.retain(|(id, _, _)| *id != frame);
    }

    /// `frame`'s iframe fires `load` at `due`, on the clock (a later one in
    /// place of one due before).
    pub(super) fn owner_load_at(&mut self, frame: FrameId, due: f64) {
        if let Some(next) = self.frames.get_mut(&frame) {
            next.owner_load_at = Some(due);
        }
    }

    /// When the first iframe's `load` is due.
    pub(super) fn owner_loads_due(&self) -> Option<f64> {
        self.frames
            .values()
            .filter_map(|next| next.owner_load_at)
            .reduce(f64::min)
    }

    /// The frames whose iframes' `load`s are due by `now`, taken.
    pub(super) fn take_due_owner_loads(&mut self, now: f64) -> Vec<FrameId> {
        self.frames
            .iter_mut()
            .filter_map(|(frame, next)| {
                next.owner_load_at
                    .take_if(|due| *due <= now)
                    .map(|_| *frame)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use dom::{Dom, NodeRoot};

    use super::*;

    const TOP: FrameId = FrameId(0);

    fn documents(dom: &mut Dom, count: usize) -> Vec<NodeRoot> {
        dom.mutate(|cx| {
            (0..count)
                .map(|_| cx.root(cx.create_document(true)))
                .collect()
        })
    }

    fn of(frames: &RenderFrames, dom: &Dom) -> Vec<dom::NodeId> {
        dom.read(|cx| {
            frames
                .documents()
                .map(|next| cx.node_id(next.node().get(cx.gc)))
                .collect()
        })
    }

    fn id(dom: &Dom, document: &NodeRoot) -> dom::NodeId {
        dom.read(|cx| cx.node_id(document.get(cx.gc)))
    }

    /// The top-level frame's document first, then the children's in the
    /// order their frames were made; the children are those not the
    /// top-level one.
    #[test]
    fn the_top_level_frame_goes_first() {
        let mut dom = Dom::new();
        let docs = documents(&mut dom, 3);
        let mut frames = RenderFrames::default();
        frames.set_child_document(FrameId(2), FrameDocument::for_test(FrameId(2), docs[2].clone()));
        frames.set_child_document(FrameId(1), FrameDocument::for_test(FrameId(1), docs[1].clone()));
        assert_eq!(frames.top(), None);
        assert_eq!(frames.ids(), [FrameId(1), FrameId(2)]);
        // A commit in the top-level frame drops those of the document before.
        frames.commit_top(TOP, FrameDocument::for_test(TOP, docs[0].clone()));
        assert_eq!(frames.ids(), [TOP]);
        frames.set_child_document(FrameId(2), FrameDocument::for_test(FrameId(2), docs[2].clone()));
        frames.set_child_document(FrameId(1), FrameDocument::for_test(FrameId(1), docs[1].clone()));
        assert_eq!(frames.top(), Some(TOP));
        assert_eq!(frames.ids(), [TOP, FrameId(1), FrameId(2)]);
        assert_eq!(frames.children().collect::<Vec<_>>(), [FrameId(1), FrameId(2)]);
        assert_eq!(of(&frames, &dom), docs.iter().map(|doc| id(&dom, doc)).collect::<Vec<_>>());
        assert_eq!(frames.child_documents().count(), 2);
        drop(frames);
    }

    /// A child frame's document replaced, and frames gone, and every frame
    /// with the engine.
    #[test]
    fn frames_go_and_documents_are_replaced() {
        let mut dom = Dom::new();
        let docs = documents(&mut dom, 4);
        let mut frames = RenderFrames::default();
        frames.commit_top(TOP, FrameDocument::for_test(TOP, docs[0].clone()));
        frames.set_child_document(FrameId(1), FrameDocument::for_test(FrameId(1), docs[1].clone()));
        frames.set_child_document(FrameId(2), FrameDocument::for_test(FrameId(2), docs[2].clone()));
        frames.set_child_document(FrameId(1), FrameDocument::for_test(FrameId(1), docs[3].clone()));
        assert_eq!(
            frames.document(FrameId(1)).map(|next| id(&dom, next.node())),
            Some(id(&dom, &docs[3]))
        );
        frames.remove(&[FrameId(1)]);
        assert!(!frames.contains(FrameId(1)));
        assert_eq!(frames.ids(), [TOP, FrameId(2)]);
        frames.clear();
        assert_eq!(frames.top(), None);
        assert!(frames.ids().is_empty());
        assert!(frames.documents().next().is_none());
    }

    fn asked(begin_id: u64) -> NavigationRequest {
        NavigationRequest {
            begin_id,
            no_referrer: false,
            origin_source: None,
        }
    }

    fn url() -> Url {
        Url::parse("about:srcdoc").unwrap()
    }

    /// A frame's navigation: one asked for in place of the one before and
    /// of a commit queued; taken by its number only; one the browser
    /// started takes whichever; the top-level frame's stays through a
    /// commit that wasn't it, the children's commits don't.
    #[test]
    fn a_frame_navigates_one_navigation_at_a_time() {
        let mut dom = Dom::new();
        let docs = documents(&mut dom, 3);
        let mut frames = RenderFrames::default();
        frames.commit_top(TOP, FrameDocument::for_test(TOP, docs[0].clone()));
        let child = FrameId(1);
        frames.set_child_document(child, FrameDocument::for_test(child, docs[1].clone()));
        assert!(!frames.is_navigating(child));
        frames.queue_commit(child, url(), String::new());
        assert!(frames.is_navigating(child));
        frames.navigation_sent(child, asked(7));
        assert!(frames.next_commit().is_none(), "the commit queued no longer commits");
        assert!(frames.is_navigating(child));
        frames.navigation_sent(child, asked(8));
        assert!(frames.take_navigation(child, 7).is_none(), "one asked for before");
        assert_eq!(frames.take_navigation(child, 8).map(|asked| asked.begin_id), Some(8));
        assert!(!frames.is_navigating(child));
        frames.navigation_sent(child, asked(9));
        assert_eq!(frames.take_any_navigation(child).map(|asked| asked.begin_id), Some(9));
        // One asked for, then a commit queued: the commit.
        frames.navigation_sent(child, asked(10));
        frames.queue_commit(child, url(), String::new());
        assert!(frames.navigation(child).is_none());
        assert!(frames.is_navigating(child));
        // The top-level frame's, through another document's commit.
        frames.navigation_sent(TOP, asked(11));
        frames.commit_top(TOP, FrameDocument::for_test(TOP, docs[2].clone()));
        assert_eq!(frames.navigation(TOP).map(|asked| asked.begin_id), Some(11));
        assert!(frames.next_commit().is_none(), "the children's go with their document");
        assert!(!frames.contains(child));
    }

    /// The iframes' `load`s due: the earliest first, those due taken; a
    /// frame gone takes its own with it, and its commits.
    #[test]
    fn a_frame_gone_takes_its_owner_load_and_commits() {
        let mut dom = Dom::new();
        let docs = documents(&mut dom, 3);
        let mut frames = RenderFrames::default();
        frames.commit_top(TOP, FrameDocument::for_test(TOP, docs[0].clone()));
        for (frame, doc) in [(FrameId(1), &docs[1]), (FrameId(2), &docs[2])] {
            frames.set_child_document(frame, FrameDocument::for_test(frame, doc.clone()));
        }
        frames.owner_load_at(FrameId(1), 30.0);
        frames.owner_load_at(FrameId(2), 20.0);
        assert_eq!(frames.owner_loads_due(), Some(20.0));
        assert_eq!(frames.take_due_owner_loads(25.0), [FrameId(2)]);
        assert_eq!(frames.owner_loads_due(), Some(30.0));
        frames.queue_commit(FrameId(1), url(), String::new());
        frames.remove(&[FrameId(1)]);
        assert_eq!(frames.owner_loads_due(), None);
        assert!(frames.next_commit().is_none());
        // The token outlasts the frames.
        frames.set_top_token(protocol::FrameToken([1; 16]));
        frames.clear();
        assert_eq!(frames.top_token(), Some(protocol::FrameToken([1; 16])));
    }
}
