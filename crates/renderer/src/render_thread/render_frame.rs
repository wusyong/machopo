//! What the renderer has of each frame (Chromium's `RenderFrameImpl`, which
//! `g_frame_map` has one of a frame): one table, the top-level frame's and
//! the child frames' alike, keyed by the engine's numbers for them. The
//! engine numbers frames as it makes them, the top-level one first, so the
//! table goes in the order they were made. Which frames there are, their
//! parents, iframes and tokens are the engine's
//! (`RenderThread::child_parent`, `child_token`); the renderer follows them
//! (`follow_child_frames`).

use std::collections::BTreeMap;

use dom_api::FrameId;

use super::documents::FrameDocument;

/// The renderer's frames.
#[derive(Default)]
pub(super) struct RenderFrames {
    frames: BTreeMap<FrameId, RenderFrame>,
    /// The top-level frame, once a document committed in it.
    top: Option<FrameId>,
}

/// What the renderer has of one frame.
pub(super) struct RenderFrame {
    /// The document loading or loaded in it: a child frame's from as it is
    /// made (its first `about:blank`), the top-level frame's from its first
    /// commit.
    document: Option<FrameDocument>,
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
    /// document before go with it.
    pub(super) fn commit_top(&mut self, top: FrameId, document: FrameDocument) {
        self.frames.clear();
        self.frames.insert(
            top,
            RenderFrame {
                document: Some(document),
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
            .or_insert(RenderFrame { document: None })
            .document = Some(document);
    }

    /// The frames `gone` (a frame and those in it) gone, and what the
    /// renderer had of them.
    pub(super) fn remove(&mut self, gone: &[FrameId]) {
        self.frames.retain(|frame, _| !gone.contains(frame));
    }

    /// Every frame gone (the engine replaced, its DOM with it).
    pub(super) fn clear(&mut self) {
        self.frames.clear();
        self.top = None;
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
}
