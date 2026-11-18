//! A tab's frames as the browser knows them (Chromium's `FrameTree` and
//! its `FrameTreeNode`s): the main frame, and each child frame its
//! renderer told of, with its parent and children, by token. The renderer
//! makes a child frame on its own (script reads its window next) and tells
//! of it after (`RendererHostMsg::DidCreateChildFrame`); a frame goes only
//! as the renderer tells (`DidDetachFrame`, the frames in it with it), or
//! with the renderer itself. What the renderer tells is checked: a message
//! the tree can't take is a bad message (`BadMessageReason`). Each frame
//! has its navigation on its way, if one is (`NavigationRequest`, which
//! goes with it), and what the browser knows of its document.

use std::collections::HashMap;

use protocol::FrameToken;

use crate::bad_message::BadMessageReason;
use crate::navigation_request::NavigationRequest;

/// A frame of the tree: its parent (none for the main frame) and its
/// child frames, in the order they were made; its document, and its
/// navigation.
struct FrameTreeNode {
    parent: Option<FrameToken>,
    children: Vec<FrameToken>,
    /// Its document's URL, as the renderer last committed one the browser
    /// loaded (a failed one's for an error page); none before (its initial
    /// `about:blank`). The `about:` documents a child frame's renderer
    /// commits itself aren't told.
    url: Option<String>,
    /// Its document may open local files: it is one (not an error page), or
    /// has its origin from one (Blink's `SecurityOrigin::
    /// CanLoadLocalResources`): a child frame's initial `about:blank` its
    /// parent's.
    local: bool,
    /// The navigation the browser sent the renderer to commit last, and
    /// whether its document is local: what it reports committing.
    committing: Option<(u64, bool)>,
    navigation: Option<NavigationRequest>,
}

impl FrameTreeNode {
    fn new(parent: Option<FrameToken>, local: bool) -> Self {
        Self {
            parent,
            children: Vec::new(),
            url: None,
            local,
            committing: None,
            navigation: None,
        }
    }
}

/// The tree of a tab's frames, the main frame its root.
pub struct FrameTree {
    root: FrameToken,
    nodes: HashMap<FrameToken, FrameTreeNode>,
}

impl FrameTree {
    /// A tree of the main frame alone, whose token is `root`.
    pub fn new(root: FrameToken) -> Self {
        let mut tree = Self {
            root,
            nodes: HashMap::new(),
        };
        tree.reset();
        tree
    }

    /// The main frame's token.
    pub fn root(&self) -> FrameToken {
        self.root
    }

    /// Whether `frame` is one of the tree's.
    pub fn contains(&self, frame: FrameToken) -> bool {
        self.nodes.contains_key(&frame)
    }

    /// `frame`'s parent: none for the main frame, or a frame not in the
    /// tree.
    pub fn parent(&self, frame: FrameToken) -> Option<FrameToken> {
        self.nodes.get(&frame)?.parent
    }

    /// `frame`'s child frames, in the order they were made.
    pub fn children(&self, frame: FrameToken) -> &[FrameToken] {
        self.nodes
            .get(&frame)
            .map_or(&[], |node| node.children.as_slice())
    }

    /// `frame`'s document's URL, as the browser knows it: none for a frame
    /// not in the tree, or before one the browser loaded commits.
    pub fn url(&self, frame: FrameToken) -> Option<&str> {
        self.nodes.get(&frame)?.url.as_deref()
    }

    /// Whether `frame`'s document may open local files (a navigation it
    /// asks for to a `file:` URL): it is one, or has its origin from one.
    pub fn may_open_local_files(&self, frame: FrameToken) -> bool {
        self.nodes.get(&frame).is_some_and(|node| node.local)
    }

    /// `frame`'s navigation on its way.
    pub(crate) fn navigation(&self, frame: FrameToken) -> Option<&NavigationRequest> {
        self.nodes.get(&frame)?.navigation.as_ref()
    }

    /// `frame`'s navigation on its way, if it is `navigation_id`.
    pub(crate) fn navigation_mut(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
    ) -> Option<&mut NavigationRequest> {
        self.nodes
            .get_mut(&frame)?
            .navigation
            .as_mut()
            .filter(|navigation| navigation.id == navigation_id)
    }

    /// `frame`'s navigation is `navigation` now: the one before, which it
    /// replaces, returned (its load stops as it goes). Nothing for a frame
    /// not in the tree (it went), `navigation` returned.
    pub(crate) fn set_navigation(
        &mut self,
        frame: FrameToken,
        navigation: NavigationRequest,
    ) -> Result<Option<NavigationRequest>, NavigationRequest> {
        match self.nodes.get_mut(&frame) {
            Some(node) => Ok(node.navigation.replace(navigation)),
            None => Err(navigation),
        }
    }

    /// `frame`'s navigation taken out, whichever it is: another takes its
    /// place.
    pub(crate) fn take_any_navigation(&mut self, frame: FrameToken) -> Option<NavigationRequest> {
        self.nodes.get_mut(&frame)?.navigation.take()
    }

    /// `frame`'s navigation taken out, if it is `navigation_id`: it is
    /// over.
    pub(crate) fn take_navigation(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
    ) -> Option<NavigationRequest> {
        self.nodes
            .get_mut(&frame)?
            .navigation
            .take_if(|navigation| navigation.id == navigation_id)
    }

    /// The browser sends `frame` navigation `navigation_id`'s document to
    /// commit, a local one if `local`: as the renderer reports it
    /// committed (`did_commit`), it is the frame's.
    pub(crate) fn committing(&mut self, frame: FrameToken, navigation_id: u64, local: bool) {
        if let Some(node) = self.nodes.get_mut(&frame) {
            node.committing = Some((navigation_id, local));
        }
    }

    /// `frame` shows navigation `navigation_id`'s document, at `url`
    /// (`DidCommitNavigation`). A frame the tree doesn't have is a bad
    /// message. One the browser didn't send isn't local.
    pub(crate) fn did_commit(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        url: &str,
    ) -> Result<(), BadMessageReason> {
        let Some(node) = self.nodes.get_mut(&frame) else {
            return Err(BadMessageReason::CommitOfUnknownFrame);
        };
        node.local = node
            .committing
            .take_if(|(id, _)| *id == navigation_id)
            .is_some_and(|(_, local)| local);
        node.url = Some(url.to_owned());
        Ok(())
    }

    /// How many frames there are, the main frame's included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Never: the main frame is always there.
    pub fn is_empty(&self) -> bool {
        false
    }

    /// `frame` was made, the last child of `parent`
    /// (`FrameTreeNode::AddChild`). A parent the tree doesn't have, or a
    /// token one of its frames has already, is a bad message.
    pub fn add_child(
        &mut self,
        parent: FrameToken,
        frame: FrameToken,
    ) -> Result<(), BadMessageReason> {
        if self.nodes.contains_key(&frame) {
            return Err(BadMessageReason::FrameTokenInUse);
        }
        let Some(node) = self.nodes.get_mut(&parent) else {
            return Err(BadMessageReason::FrameParentUnknown);
        };
        node.children.push(frame);
        // Its initial `about:blank` has its parent's origin.
        let local = node.local;
        self.nodes
            .insert(frame, FrameTreeNode::new(Some(parent), local));
        Ok(())
    }

    /// `frame` has gone, and the frames in it with it
    /// (`RenderFrameHostImpl::Detach`), their navigations stopped. A frame
    /// the tree doesn't have, or the main frame (Chromium's
    /// `RFH_DETACH_MAIN_FRAME`), is a bad message.
    pub fn remove(&mut self, frame: FrameToken) -> Result<(), BadMessageReason> {
        let Some(node) = self.nodes.get(&frame) else {
            return Err(BadMessageReason::DetachUnknownFrame);
        };
        let Some(parent) = node.parent else {
            return Err(BadMessageReason::DetachMainFrame);
        };
        if let Some(parent) = self.nodes.get_mut(&parent) {
            parent.children.retain(|child| *child != frame);
        }
        let mut gone = vec![frame];
        while let Some(at) = gone.pop() {
            if let Some(node) = self.nodes.remove(&at) {
                gone.extend(node.children);
            }
        }
        Ok(())
    }

    /// The main frame alone again, showing nothing: its renderer has gone,
    /// or another took its place (`RenderFrameHostImpl::ResetChildren` as
    /// the process is gone). Its navigation goes on: the next renderer
    /// commits it.
    pub fn reset(&mut self) {
        let navigation = self
            .nodes
            .get_mut(&self.root)
            .and_then(|root| root.navigation.take());
        self.nodes.clear();
        let mut root = FrameTreeNode::new(None, false);
        root.navigation = navigation;
        self.nodes.insert(self.root, root);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(n: u8) -> FrameToken {
        FrameToken([n; 16])
    }

    /// Child frames hang from the frame they were made in, in the order
    /// they were made; a frame removed takes the frames in it with it.
    #[test]
    fn frames_come_and_go_with_the_frames_in_them() {
        let mut tree = FrameTree::new(token(0));
        assert_eq!(tree.root(), token(0));
        assert_eq!(tree.len(), 1);
        tree.add_child(token(0), token(1)).unwrap();
        tree.add_child(token(0), token(2)).unwrap();
        tree.add_child(token(1), token(3)).unwrap();
        tree.add_child(token(3), token(4)).unwrap();
        assert_eq!(tree.children(token(0)), [token(1), token(2)]);
        assert_eq!(tree.children(token(1)), [token(3)]);
        assert_eq!(tree.parent(token(4)), Some(token(3)));
        assert_eq!(tree.parent(token(0)), None);
        assert_eq!(tree.len(), 5);
        tree.remove(token(1)).unwrap();
        assert_eq!(tree.children(token(0)), [token(2)]);
        for gone in [token(1), token(3), token(4)] {
            assert!(!tree.contains(gone), "{gone:?}");
        }
        assert_eq!(tree.len(), 2);
        // Gone, its token may be made again.
        tree.add_child(token(2), token(1)).unwrap();
        assert_eq!(tree.parent(token(1)), Some(token(2)));
        tree.reset();
        assert_eq!(tree.len(), 1);
        assert!(tree.children(token(0)).is_empty());
        assert!(!tree.contains(token(2)));
    }

    /// A frame's document is what the renderer reports committing; it may
    /// open local files if the browser sent it a local one to commit. A
    /// new child frame's initial `about:blank` may if its parent may.
    #[test]
    fn a_frames_document_is_what_it_committed() {
        let mut tree = FrameTree::new(token(0));
        assert_eq!(tree.url(token(0)), None);
        assert!(!tree.may_open_local_files(token(0)));
        tree.committing(token(0), 1, true);
        tree.did_commit(token(0), 1, "file:///a.html").unwrap();
        assert_eq!(tree.url(token(0)), Some("file:///a.html"));
        assert!(tree.may_open_local_files(token(0)));
        tree.add_child(token(0), token(1)).unwrap();
        assert!(tree.may_open_local_files(token(1)), "its parent's");
        assert_eq!(tree.url(token(1)), None);
        // A commit the browser didn't send (another number) isn't local.
        tree.committing(token(1), 2, true);
        tree.did_commit(token(1), 3, "file:///b.html").unwrap();
        assert!(!tree.may_open_local_files(token(1)));
        tree.committing(token(0), 4, false);
        tree.did_commit(token(0), 4, "https://a.test/").unwrap();
        assert!(!tree.may_open_local_files(token(0)));
        tree.add_child(token(0), token(2)).unwrap();
        assert!(!tree.may_open_local_files(token(2)), "its parent's");
        assert_eq!(
            tree.did_commit(token(9), 5, "https://a.test/"),
            Err(BadMessageReason::CommitOfUnknownFrame)
        );
        tree.reset();
        assert_eq!(tree.url(token(0)), None);
    }

    /// What the tree can't take: a parent it doesn't have, a token in use
    /// (the main frame's too), a frame it doesn't have gone, the main
    /// frame gone. The tree stays as it was.
    #[test]
    fn what_the_tree_cant_take_is_a_bad_message() {
        let mut tree = FrameTree::new(token(0));
        tree.add_child(token(0), token(1)).unwrap();
        assert_eq!(
            tree.add_child(token(9), token(2)),
            Err(BadMessageReason::FrameParentUnknown)
        );
        assert_eq!(
            tree.add_child(token(0), token(1)),
            Err(BadMessageReason::FrameTokenInUse)
        );
        assert_eq!(
            tree.add_child(token(1), token(0)),
            Err(BadMessageReason::FrameTokenInUse)
        );
        assert_eq!(
            tree.remove(token(9)),
            Err(BadMessageReason::DetachUnknownFrame)
        );
        assert_eq!(
            tree.remove(token(0)),
            Err(BadMessageReason::DetachMainFrame)
        );
        assert_eq!(tree.len(), 2);
        assert_eq!(tree.children(token(0)), [token(1)]);
        assert!(tree.children(token(1)).is_empty());
    }
}
