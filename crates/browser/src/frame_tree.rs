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
use crate::navigation_controller::HistoryPlace;
use crate::navigation_request::NavigationRequest;

/// A frame of the tree: its parent (none for the main frame) and its
/// child frames, in the order they were made; its document, and its
/// navigation.
struct FrameTreeNode {
    parent: Option<FrameToken>,
    children: Vec<FrameToken>,
    /// Its place in the tab's session history (`unique_name`): empty for
    /// the main frame.
    unique_name: String,
    /// Its document's URL, as the renderer last committed one the browser
    /// loaded (a failed one's for an error page); none before (its initial
    /// `about:blank`). The `about:` documents a child frame's renderer
    /// commits itself aren't told.
    url: Option<String>,
    /// It committed a document not at `about:blank` (Chromium's
    /// `has_committed_real_load`): till then its next document takes its
    /// place in the session history, as one replacing its initial
    /// `about:blank` does.
    committed_real_load: bool,
    /// Where its document is in the session history, as it last committed
    /// one (Chromium's `nav_entry_id`, `last_committed_frame_entry`): none
    /// before, or for one with no place there.
    history_place: Option<HistoryPlace>,
    /// Its document's origin: its URL's, or the one it has from another
    /// document (an `about:blank`'s initiator's, an `about:srcdoc`'s
    /// parent's, a child frame's initial `about:blank`'s parent's); an
    /// error page's is opaque.
    origin: url::Origin,
    /// Its document may open local files: it is one (not an error page), or
    /// has its origin from one (Blink's `SecurityOrigin::
    /// CanLoadLocalResources`), as its origin. (A local file's origin is
    /// opaque, which doesn't say it is one.)
    local: bool,
    /// Its iframe's `srcdoc`, as the renderer last told
    /// (`DidChangeSrcDoc`): what its `about:srcdoc` documents are.
    srcdoc: String,
    /// The navigation the browser sent the renderer to commit last, and
    /// what its document is: what the renderer reports committing.
    committing: Option<(u64, CommittedOrigin)>,
    navigation: Option<NavigationRequest>,
}

/// The frame isn't in the tree: it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameGone;

/// A document's origin, and whether it may open local files.
#[derive(Debug, Clone)]
pub(crate) struct CommittedOrigin {
    pub(crate) origin: url::Origin,
    pub(crate) local: bool,
}

impl CommittedOrigin {
    /// One of no other document's origin, that opens no local files (an
    /// error page's).
    pub(crate) fn opaque() -> Self {
        Self {
            origin: url::Url::parse("about:blank")
                .expect("about:blank")
                .origin(),
            local: false,
        }
    }
}

impl FrameTreeNode {
    fn new(parent: Option<FrameToken>, unique_name: String, origin: CommittedOrigin) -> Self {
        Self {
            parent,
            children: Vec::new(),
            unique_name,
            url: None,
            committed_real_load: false,
            history_place: None,
            origin: origin.origin,
            local: origin.local,
            srcdoc: String::new(),
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

    /// Where `frame`'s document is in the session history, as it last
    /// committed one.
    pub(crate) fn history_place(&self, frame: FrameToken) -> Option<HistoryPlace> {
        self.nodes.get(&frame)?.history_place
    }

    /// `frame`'s document, just committed, is at `place` in the session
    /// history.
    pub(crate) fn set_history_place(&mut self, frame: FrameToken, place: Option<HistoryPlace>) {
        if let Some(node) = self.nodes.get_mut(&frame) {
            node.history_place = place;
        }
    }

    /// Whether `frame` committed a document not at `about:blank`: a frame
    /// not in the tree hasn't.
    pub fn has_committed_real_load(&self, frame: FrameToken) -> bool {
        self.nodes
            .get(&frame)
            .is_some_and(|node| node.committed_real_load)
    }

    /// Whether `frame`'s document may open local files (a navigation it
    /// asks for to a `file:` URL): it is one, or has its origin from one.
    pub fn may_open_local_files(&self, frame: FrameToken) -> bool {
        self.nodes.get(&frame).is_some_and(|node| node.local)
    }

    /// `frame`'s place in the tab's session history, which a frame of a
    /// later document at the same place has too (Chromium's unique name):
    /// empty for the main frame.
    pub fn unique_name(&self, frame: FrameToken) -> Option<&str> {
        self.nodes.get(&frame).map(|node| node.unique_name.as_str())
    }

    /// Every frame's unique name.
    pub(crate) fn unique_names(&self) -> impl Iterator<Item = &str> {
        self.nodes.values().map(|node| node.unique_name.as_str())
    }

    /// `frame`'s document's origin, as the browser knows it.
    pub fn origin(&self, frame: FrameToken) -> Option<&url::Origin> {
        self.nodes.get(&frame).map(|node| &node.origin)
    }

    /// `frame`'s document's origin and whether it may open local files,
    /// for a document that has them from it.
    pub(crate) fn committed_origin(&self, frame: FrameToken) -> Option<CommittedOrigin> {
        let node = self.nodes.get(&frame)?;
        Some(CommittedOrigin {
            origin: node.origin.clone(),
            local: node.local,
        })
    }

    /// `frame`'s iframe's `srcdoc`, as the renderer last told.
    pub fn srcdoc(&self, frame: FrameToken) -> Option<&str> {
        self.nodes.get(&frame).map(|node| node.srcdoc.as_str())
    }

    /// `frame`'s iframe's `srcdoc` is `srcdoc` now (`DidChangeSrcDoc`). A
    /// frame the tree doesn't have, or the main frame (which has no
    /// iframe), is a bad message.
    pub(crate) fn set_srcdoc(
        &mut self,
        frame: FrameToken,
        srcdoc: &str,
    ) -> Result<(), BadMessageReason> {
        match self.nodes.get_mut(&frame) {
            Some(node) if node.parent.is_some() => {
                srcdoc.clone_into(&mut node.srcdoc);
                Ok(())
            }
            _ => Err(BadMessageReason::SrcdocOfUnknownFrame),
        }
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
    /// replaces, returned (its load stops as it goes). An error for a frame
    /// not in the tree (it went), `navigation` going with it.
    pub(crate) fn set_navigation(
        &mut self,
        frame: FrameToken,
        navigation: NavigationRequest,
    ) -> Result<Option<NavigationRequest>, FrameGone> {
        match self.nodes.get_mut(&frame) {
            Some(node) => Ok(node.navigation.replace(navigation)),
            None => Err(FrameGone),
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
    /// commit, of `origin`: as the renderer reports it committed
    /// (`did_commit`), it is the frame's.
    pub(crate) fn committing(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        origin: CommittedOrigin,
    ) {
        if let Some(node) = self.nodes.get_mut(&frame) {
            node.committing = Some((navigation_id, origin));
        }
    }

    /// `frame` shows navigation `navigation_id`'s document, at `url`
    /// (`DidCommitNavigation`). A frame the tree doesn't have is a bad
    /// message. One the browser didn't send is of an opaque origin and
    /// isn't local.
    pub(crate) fn did_commit(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        url: &str,
    ) -> Result<(), BadMessageReason> {
        let Some(node) = self.nodes.get_mut(&frame) else {
            return Err(BadMessageReason::CommitOfUnknownFrame);
        };
        let origin = node
            .committing
            .take_if(|(id, _)| *id == navigation_id)
            .map_or_else(CommittedOrigin::opaque, |(_, origin)| origin);
        node.origin = origin.origin;
        node.local = origin.local;
        node.url = Some(url.to_owned());
        node.committed_real_load |= crate::loader::about_path(url) != Some("blank");
        Ok(())
    }

    /// `frame`'s document navigated within itself to `url`
    /// (`DidCommitSameDocumentNavigation`): its URL now. A frame the tree
    /// doesn't have, or a URL its document can't have without loading
    /// another (`can_have_url_rewritten`, against the URL the tree has;
    /// before any, its initial `about:blank`), is a bad message.
    pub(crate) fn did_commit_same_document(
        &mut self,
        frame: FrameToken,
        url: &str,
    ) -> Result<(), BadMessageReason> {
        let Some(node) = self.nodes.get_mut(&frame) else {
            return Err(BadMessageReason::SameDocumentOfUnknownFrame);
        };
        let current = node.url.as_deref().unwrap_or("about:blank");
        let rewritable = match (url::Url::parse(current), url::Url::parse(url)) {
            (Ok(current), Ok(target)) => can_have_url_rewritten(&current, &target),
            _ => false,
        };
        if !rewritable {
            return Err(BadMessageReason::SameDocumentToAnotherUrl);
        }
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
    /// (`FrameTreeNode::AddChild`), named `name`, by script running if
    /// `created_by_script`: its unique name comes of them. A parent the
    /// tree doesn't have, or a token one of its frames has already, is a
    /// bad message.
    pub fn add_child(
        &mut self,
        parent: FrameToken,
        frame: FrameToken,
        name: &str,
        created_by_script: bool,
    ) -> Result<(), BadMessageReason> {
        if self.nodes.contains_key(&frame) {
            return Err(BadMessageReason::FrameTokenInUse);
        }
        if !self.nodes.contains_key(&parent) {
            return Err(BadMessageReason::FrameParentUnknown);
        }
        let unique_name =
            crate::unique_name::for_new_child(self, parent, name, created_by_script);
        let Some(node) = self.nodes.get_mut(&parent) else {
            return Err(BadMessageReason::FrameParentUnknown);
        };
        node.children.push(frame);
        // Its initial `about:blank` has its parent's origin.
        let origin = CommittedOrigin {
            origin: node.origin.clone(),
            local: node.local,
        };
        self.nodes
            .insert(frame, FrameTreeNode::new(Some(parent), unique_name, origin));
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
        let mut root = FrameTreeNode::new(None, String::new(), CommittedOrigin::opaque());
        root.navigation = navigation;
        self.nodes.insert(self.root, root);
    }
}

/// HTML's "can have its URL rewritten": a document at `document` may take
/// `target` as its URL without loading another (the renderer checks the
/// same, Blink's `CanChangeToUrlForHistoryApi`; the browser checks what it
/// is told): the same scheme, credentials, host and port; a web page's any
/// path and query; a local file's its path; any other's only its fragment.
pub(crate) fn can_have_url_rewritten(document: &url::Url, target: &url::Url) -> bool {
    if document.scheme() != target.scheme()
        || document.username() != target.username()
        || document.password() != target.password()
        || document.host() != target.host()
        || document.port() != target.port()
    {
        return false;
    }
    if matches!(target.scheme(), "http" | "https") {
        return true;
    }
    if target.scheme() == "file" {
        return document.path() == target.path();
    }
    document.path() == target.path() && document.query() == target.query()
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
        tree.add_child(token(0), token(1), "", false).unwrap();
        tree.add_child(token(0), token(2), "", false).unwrap();
        tree.add_child(token(1), token(3), "", false).unwrap();
        tree.add_child(token(3), token(4), "", false).unwrap();
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
        tree.add_child(token(2), token(1), "", false).unwrap();
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
        let local = CommittedOrigin {
            origin: url::Url::parse("file:///a.html").unwrap().origin(),
            local: true,
        };
        tree.committing(token(0), 1, local);
        tree.did_commit(token(0), 1, "file:///a.html").unwrap();
        assert_eq!(tree.url(token(0)), Some("file:///a.html"));
        assert!(tree.may_open_local_files(token(0)));
        tree.add_child(token(0), token(1), "", false).unwrap();
        assert!(tree.may_open_local_files(token(1)), "its parent's");
        assert_eq!(tree.url(token(1)), None);
        // A commit the browser didn't send (another number) isn't local.
        tree.committing(token(1), 2, tree.committed_origin(token(0)).unwrap());
        tree.did_commit(token(1), 3, "file:///b.html").unwrap();
        assert!(!tree.may_open_local_files(token(1)));
        let web = url::Url::parse("https://a.test/").unwrap().origin();
        let committed = CommittedOrigin {
            origin: web.clone(),
            local: false,
        };
        tree.committing(token(0), 4, committed);
        tree.did_commit(token(0), 4, "https://a.test/").unwrap();
        assert!(!tree.may_open_local_files(token(0)));
        assert_eq!(tree.origin(token(0)), Some(&web));
        assert_ne!(tree.origin(token(1)), Some(&web), "an opaque one");
        tree.add_child(token(0), token(2), "", false).unwrap();
        assert!(!tree.may_open_local_files(token(2)), "its parent's");
        assert_eq!(tree.origin(token(2)), Some(&web), "its parent's");
        // Its iframe's `srcdoc`, a child frame's alone.
        assert_eq!(tree.srcdoc(token(2)), Some(""));
        tree.set_srcdoc(token(2), "<p>x").unwrap();
        assert_eq!(tree.srcdoc(token(2)), Some("<p>x"));
        for frame in [token(0), token(9)] {
            assert_eq!(
                tree.set_srcdoc(frame, "y"),
                Err(BadMessageReason::SrcdocOfUnknownFrame)
            );
        }
        assert_eq!(
            tree.did_commit(token(9), 5, "https://a.test/"),
            Err(BadMessageReason::CommitOfUnknownFrame)
        );
        // Its first document not at `about:blank`, whatever comes after.
        assert!(!tree.has_committed_real_load(token(2)));
        tree.did_commit(token(2), 6, "about:blank#x").unwrap();
        assert!(!tree.has_committed_real_load(token(2)));
        tree.did_commit(token(2), 7, "about:srcdoc").unwrap();
        assert!(tree.has_committed_real_load(token(2)));
        tree.did_commit(token(2), 8, "about:blank").unwrap();
        assert!(tree.has_committed_real_load(token(2)));
        tree.reset();
        assert_eq!(tree.url(token(0)), None);
    }

    /// A frame's document within itself takes what HTML lets it: any path
    /// and query of a web page's origin, a local file's fragment and
    /// query, another URL's fragment alone (an initial `about:blank`'s as
    /// `about:blank`'s); anything else, and a frame not there, is a bad
    /// message, which leaves its URL as it was.
    #[test]
    fn a_document_navigates_within_itself_as_html_lets_it() {
        let mut tree = FrameTree::new(token(0));
        tree.add_child(token(0), token(1), "", false).unwrap();
        let cases = [
            (token(1), "about:blank#x", true),
            (token(1), "about:blank?q", false),
            (token(0), "about:blank", true),
        ];
        for (frame, url, ok) in cases {
            assert_eq!(
                tree.did_commit_same_document(frame, url).is_ok(),
                ok,
                "{url}"
            );
        }
        tree.did_commit(token(0), 1, "https://a.test/p?q#f")
            .unwrap();
        tree.did_commit(token(1), 2, "file:///d/f.html").unwrap();
        let cases = [
            (token(0), "https://a.test/other?r", true),
            (token(0), "https://b.test/other", false),
            (token(0), "http://a.test/other", false),
            (token(0), "https://u@a.test/other", false),
            (token(0), "https://a.test:8443/other", false),
            (token(1), "file:///d/f.html?x#y", true),
            (token(1), "file:///d/g.html", false),
            (token(9), "https://a.test/", false),
        ];
        for (frame, url, ok) in cases {
            assert_eq!(
                tree.did_commit_same_document(frame, url).is_ok(),
                ok,
                "{url}"
            );
        }
        assert_eq!(tree.url(token(0)), Some("https://a.test/other?r"));
        assert_eq!(tree.url(token(1)), Some("file:///d/f.html?x#y"));
        assert_eq!(
            tree.did_commit_same_document(token(9), "https://a.test/"),
            Err(BadMessageReason::SameDocumentOfUnknownFrame)
        );
        assert_eq!(
            tree.did_commit_same_document(token(0), "https://b.test/"),
            Err(BadMessageReason::SameDocumentToAnotherUrl)
        );
        tree.did_commit(token(1), 3, "data:text/html,x").unwrap();
        assert!(
            tree.did_commit_same_document(token(1), "data:text/html,x#f")
                .is_ok()
        );
        assert!(
            tree.did_commit_same_document(token(1), "data:text/html,y")
                .is_err()
        );
    }

    /// What the tree can't take: a parent it doesn't have, a token in use
    /// (the main frame's too), a frame it doesn't have gone, the main
    /// frame gone. The tree stays as it was.
    #[test]
    fn what_the_tree_cant_take_is_a_bad_message() {
        let mut tree = FrameTree::new(token(0));
        tree.add_child(token(0), token(1), "", false).unwrap();
        assert_eq!(
            tree.add_child(token(9), token(2), "", false),
            Err(BadMessageReason::FrameParentUnknown)
        );
        assert_eq!(
            tree.add_child(token(0), token(1), "", false),
            Err(BadMessageReason::FrameTokenInUse)
        );
        assert_eq!(
            tree.add_child(token(1), token(0), "", false),
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
