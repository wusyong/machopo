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
use protocol::sandbox::SandboxFlags;
use site::{Site, SiteForCookies, StorageKey};

use crate::bad_message::BadMessageReason;
use crate::navigation_controller::HistoryPlace;
use crate::navigation_request::{DocumentLoad, NavigationRequest};
use crate::render_process_host::LoaderParams;

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
    /// What its document's storage is kept by: its origin, the main
    /// frame's site, and whether a frame of another site is between them
    /// (`RenderFrameHostImpl::storage_key_`, worked out as it commits).
    storage_key: StorageKey,
    /// Its document may open local files: it is one (not an error page), or
    /// has its origin from one (Blink's `SecurityOrigin::
    /// CanLoadLocalResources`), as its origin. (A local file's origin is
    /// opaque, which doesn't say it is one.)
    local: bool,
    /// Its iframe's `srcdoc`, as the renderer last told
    /// (`DidChangeSrcDoc`): what its `about:srcdoc` documents are.
    srcdoc: String,
    /// What its next document is sandboxed by (Chromium's
    /// `pending_frame_policy_`): its iframe's `sandbox` flags as the
    /// renderer last told, with its parent's document's.
    pending_sandbox: SandboxFlags,
    /// What its document is sandboxed by, as it last committed one (its
    /// initial `about:blank`: `pending_sandbox` as it was made).
    sandbox: SandboxFlags,
    /// The navigations the browser sent the renderer to commit and it
    /// hasn't reported committing, oldest first (Chromium's
    /// `navigation_requests_`): a commit reported is one of them.
    committing: Vec<Committing>,
    navigation: Option<NavigationRequest>,
    /// The load of the document last sent to commit, while its body still
    /// comes: it stops as another commits or the frame goes.
    document_load: Option<DocumentLoad>,
}

/// A navigation sent to commit: its document's URL and origin.
#[derive(Debug)]
struct Committing {
    navigation_id: u64,
    url: String,
    origin: CommittedOrigin,
    sandbox: SandboxFlags,
}

/// The frame isn't in the tree: it went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameGone;

/// A document's origin, and whether it may open local files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CommittedOrigin {
    pub(crate) origin: url::Origin,
    pub(crate) local: bool,
}

/// `origin` as a request's initiator says it: `null` for an opaque one.
pub(crate) fn serialize_origin(origin: &url::Origin) -> String {
    match origin {
        origin @ url::Origin::Tuple(..) => origin.ascii_serialization(),
        url::Origin::Opaque(_) => "null".to_owned(),
    }
}

impl CommittedOrigin {
    /// One of no other document's origin, that opens no local files (an
    /// error page's).
    pub(crate) fn opaque() -> Self {
        Self {
            // `about:blank`'s: a new opaque one.
            origin: url::Origin::new_opaque(),
            local: false,
        }
    }
}

impl FrameTreeNode {
    fn new(
        parent: Option<FrameToken>,
        unique_name: String,
        origin: CommittedOrigin,
        storage_key: StorageKey,
    ) -> Self {
        Self {
            parent,
            children: Vec::new(),
            unique_name,
            url: None,
            committed_real_load: false,
            history_place: None,
            origin: origin.origin,
            storage_key,
            local: origin.local,
            srcdoc: String::new(),
            pending_sandbox: SandboxFlags::NONE,
            sandbox: SandboxFlags::NONE,
            committing: Vec::new(),
            navigation: None,
            document_load: None,
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

    /// Every frame's token, in no order.
    pub(crate) fn frames(&self) -> Vec<FrameToken> {
        self.nodes.keys().copied().collect()
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

    /// What `frame`'s document's storage is kept by.
    pub fn storage_key(&self, frame: FrameToken) -> Option<&StorageKey> {
        self.nodes.get(&frame).map(|node| &node.storage_key)
    }

    /// The key of a document of `origin` in `frame`, in the frames it is in
    /// as they are (`RenderFrameHostImpl::CalculateStorageKey`): the
    /// frames above it keep their documents while it has one, as a frame
    /// whose document goes takes the frames in it with it.
    fn storage_key_in(&self, frame: Option<FrameToken>, origin: &url::Origin) -> StorageKey {
        let ancestors = std::iter::successors(frame, |at| self.nodes.get(at)?.parent)
            .filter_map(|at| self.nodes.get(&at).map(|node| &node.origin));
        StorageKey::of_frame(origin, ancestors)
    }

    /// The params of the loader for a document of `origin` that `frame`
    /// commits, its subresources' (Chromium's `URLLoaderFactoryParams`:
    /// `request_initiator_origin_lock`, and the `IsolationInfo` of the
    /// frame): the site it is first party to, by the frames it is in as
    /// they are now, the top-level frame's site, and its own.
    pub(crate) fn loader_params(&self, frame: FrameToken, origin: &url::Origin) -> LoaderParams {
        let ancestors: Vec<&url::Origin> =
            self.ancestors(frame).map(|(_, origin)| origin).collect();
        let top = ancestors.last().copied().unwrap_or(origin);
        let site_for_cookies = SiteForCookies::of_frame(origin, ancestors.iter().copied());
        let top_frame_site = Site::of(top);
        let frame_site = Site::of(origin);
        LoaderParams {
            origin: origin.clone(),
            trusted: protocol::network::TrustedParams {
                site_for_cookies: site_for_cookies.site().map(Site::serialize),
                top_frame_site: (!top_frame_site.is_opaque()).then(|| top_frame_site.serialize()),
                frame_site: (!frame_site.is_opaque()).then(|| frame_site.serialize()),
                initiator: Some(serialize_origin(origin)),
                is_main_frame_navigation: false,
            },
        }
    }

    /// The trusted params of `frame`'s navigation to `url`, which the frame
    /// `initiator` asked for (none: the browser's own, the address typed):
    /// a main frame's is first party to `url`'s site, its top-level site
    /// (as each hop's, which the network service works out); a child
    /// frame's, to its parent's site for cookies, in the frames as they
    /// are now (Chromium's `NavigationRequest` computes its
    /// `IsolationInfo` from the parent's).
    pub(crate) fn navigation_params(
        &self,
        frame: FrameToken,
        url: &str,
        initiator: Option<FrameToken>,
    ) -> protocol::network::TrustedParams {
        let initiator = initiator
            .and_then(|initiator| self.nodes.get(&initiator))
            .map(|node| serialize_origin(&node.origin));
        let parent = self.nodes.get(&frame).and_then(|node| node.parent);
        let Some(parent) =
            parent.and_then(|parent| self.nodes.get(&parent).map(|node| (parent, node)))
        else {
            let site = url::Url::parse(url).map_or_else(
                |_| {
                    Site::of(
                        // `about:blank`'s: a new opaque one.
                        &url::Origin::new_opaque(),
                    )
                },
                |url| Site::of_url(&url),
            );
            let site_for_cookies = SiteForCookies::of_site(site.clone());
            return protocol::network::TrustedParams {
                site_for_cookies: site_for_cookies.site().map(Site::serialize),
                top_frame_site: (!site.is_opaque()).then(|| site.serialize()),
                // Its URL's at each hop, as the network service works out.
                frame_site: None,
                initiator,
                is_main_frame_navigation: true,
            };
        };
        let (parent, parent_node) = parent;
        let mut params = self.loader_params(parent, &parent_node.origin).trusted;
        params.initiator = initiator;
        // Not the parent's: the frame's, its URL's at each hop.
        params.frame_site = None;
        params
    }

    /// `frame`'s ancestors and the origins of their documents, its
    /// parent's first, the top-level frame's last (none for the main
    /// frame): those a response in it would be shown in, as they last
    /// committed (Chromium's `GetLastCommittedOrigin` of each).
    pub(crate) fn ancestor_origins(&self, frame: FrameToken) -> Vec<(FrameToken, url::Origin)> {
        self.ancestors(frame)
            .map(|(at, origin)| (at, origin.clone()))
            .collect()
    }

    /// `frame`'s ancestors and their documents' origins, its parent's
    /// first.
    fn ancestors(&self, frame: FrameToken) -> impl Iterator<Item = (FrameToken, &url::Origin)> {
        let parent = self.nodes.get(&frame).and_then(|node| node.parent);
        std::iter::successors(parent, |at| self.nodes.get(at)?.parent)
            .filter_map(|at| self.nodes.get(&at).map(|node| (at, &node.origin)))
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

    /// `frame`'s iframe's `sandbox` flags are `sandbox` now
    /// (`DidChangeFramePolicy`; Chromium's `SetPendingFramePolicy`): with
    /// its parent's document's, what its next document is sandboxed by. A
    /// frame the tree doesn't have, or the main frame (which has no
    /// iframe), is a bad message.
    pub(crate) fn set_frame_policy(
        &mut self,
        frame: FrameToken,
        sandbox: SandboxFlags,
    ) -> Result<(), BadMessageReason> {
        let parent = self
            .nodes
            .get(&frame)
            .and_then(|node| node.parent)
            .and_then(|parent| self.nodes.get(&parent))
            .ok_or(BadMessageReason::FramePolicyOfUnknownFrame)?
            .sandbox;
        let node = self
            .nodes
            .get_mut(&frame)
            .ok_or(BadMessageReason::FramePolicyOfUnknownFrame)?;
        node.pending_sandbox = sandbox | parent;
        Ok(())
    }

    /// What `frame`'s next document is sandboxed by (its pending flags);
    /// none for a frame the tree doesn't have.
    pub(crate) fn pending_sandbox(&self, frame: FrameToken) -> SandboxFlags {
        self.nodes
            .get(&frame)
            .map_or(SandboxFlags::NONE, |node| node.pending_sandbox)
    }

    /// What `frame`'s document is sandboxed by, as it last committed; none
    /// for a frame the tree doesn't have.
    pub fn sandbox(&self, frame: FrameToken) -> SandboxFlags {
        self.nodes
            .get(&frame)
            .map_or(SandboxFlags::NONE, |node| node.sandbox)
    }

    /// Whether `frame` is `ancestor` or below it.
    pub(crate) fn is_within(&self, frame: FrameToken, ancestor: FrameToken) -> bool {
        std::iter::successors(Some(frame), |at| self.nodes.get(at)?.parent).any(|at| at == ancestor)
    }

    /// What the main frame's next documents are sandboxed by: a new tab's,
    /// its opener's flags (Chromium's `CreateNewWindow` gives the new main
    /// frame's pending frame policy the opener's), for each of its
    /// documents from now on.
    pub(crate) fn set_main_frame_sandbox(&mut self, sandbox: SandboxFlags) {
        let root = self.root;
        if let Some(node) = self.nodes.get_mut(&root) {
            node.pending_sandbox = sandbox;
        }
    }

    /// `frame`'s navigation on its way.
    pub(crate) fn navigation(&self, frame: FrameToken) -> Option<&NavigationRequest> {
        self.nodes.get(&frame)?.navigation.as_ref()
    }

    /// The URL of `frame`'s navigation `navigation_id` while it is on its
    /// way, or sent to commit and not yet reported committed: none once it
    /// has committed, or stopped, or another took its place, or the frame
    /// went.
    pub(crate) fn uncommitted_url(&self, frame: FrameToken, navigation_id: u64) -> Option<&str> {
        let node = self.nodes.get(&frame)?;
        if let Some(navigation) = node.navigation.as_ref().filter(|n| n.id == navigation_id) {
            return Some(&navigation.url);
        }
        node.committing
            .iter()
            .find(|sent| sent.navigation_id == navigation_id)
            .map(|sent| sent.url.as_str())
    }

    /// The frame whose navigation on its way is the renderer's `begin_id`
    /// (`BeginNavigation`'s), and the browser's number for it.
    pub(crate) fn navigation_of_begin_id(&self, begin_id: u64) -> Option<(FrameToken, u64)> {
        self.nodes.iter().find_map(|(frame, node)| {
            node.navigation
                .as_ref()
                .filter(|navigation| navigation.begin_id == Some(begin_id))
                .map(|navigation| (*frame, navigation.id))
        })
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
    /// commit, at `url` (a failed one's, for an error page), of `origin`,
    /// sandboxed by `sandbox`: as the renderer reports it committed
    /// (`did_commit`), it is the frame's.
    pub(crate) fn committing(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        url: &str,
        origin: CommittedOrigin,
        sandbox: SandboxFlags,
    ) {
        if let Some(node) = self.nodes.get_mut(&frame) {
            node.committing.push(Committing {
                navigation_id,
                url: url.to_owned(),
                origin,
                sandbox,
            });
        }
    }

    /// `load` is that of the document `frame` was last sent to commit,
    /// whose body still comes; it goes if the frame has.
    pub(crate) fn set_document_load(&mut self, frame: FrameToken, load: DocumentLoad) {
        if let Some(node) = self.nodes.get_mut(&frame) {
            node.document_load = Some(load);
        }
    }

    /// `frame`'s document's load taken out, whichever it is: another
    /// document commits.
    pub(crate) fn take_any_document_load(&mut self, frame: FrameToken) -> Option<DocumentLoad> {
        self.nodes.get_mut(&frame)?.document_load.take()
    }

    /// `frame`'s document's load taken out, if it is navigation
    /// `navigation_id`'s: it ended.
    pub(crate) fn take_document_load(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
    ) -> Option<DocumentLoad> {
        self.nodes
            .get_mut(&frame)?
            .document_load
            .take_if(|load| load.navigation_id() == navigation_id)
    }

    /// `frame` shows navigation `navigation_id`'s document, at `url`
    /// (`DidCommitNavigation`): one the browser sent it to commit, at that
    /// URL (the renderer drops those sent before as it commits a later
    /// one, so they go), or a child frame's own `javascript:` URL's
    /// (`RENDERER_COMMIT`), of an opaque origin. Anything else, or a frame
    /// the tree doesn't have, is a bad message (Chromium's
    /// `RFH_NO_MATCHING_NAVIGATION_REQUEST_ON_COMMIT`; the URL as the
    /// process lock it has isn't here).
    pub(crate) fn did_commit(
        &mut self,
        frame: FrameToken,
        navigation_id: u64,
        url: &str,
    ) -> Result<(), BadMessageReason> {
        let Some(node) = self.nodes.get_mut(&frame) else {
            return Err(BadMessageReason::CommitOfUnknownFrame);
        };
        let origin = if navigation_id == protocol::renderer::RENDERER_COMMIT {
            let javascript = url
                .split_once(':')
                .is_some_and(|(scheme, _)| scheme.eq_ignore_ascii_case("javascript"));
            if node.parent.is_none() || !javascript {
                return Err(BadMessageReason::CommitOfNoNavigation);
            }
            // A `javascript:` URL's document is sandboxed as the one it
            // replaces.
            (CommittedOrigin::opaque(), node.sandbox)
        } else {
            let Some(at) = node
                .committing
                .iter()
                .position(|sent| sent.navigation_id == navigation_id)
            else {
                return Err(BadMessageReason::CommitOfNoNavigation);
            };
            if node.committing.get(at).is_some_and(|sent| sent.url != url) {
                return Err(BadMessageReason::CommitOfAnotherUrl);
            }
            // The one at `at`, and those sent before it (what the renderer
            // will commit no more).
            let Some(sent) = node.committing.drain(..=at).next_back() else {
                return Err(BadMessageReason::CommitOfNoNavigation);
            };
            (sent.origin, sent.sandbox)
        };
        let (origin, sandbox) = origin;
        let parent = node.parent;
        let storage_key = self.storage_key_in(parent, &origin.origin);
        let Some(node) = self.nodes.get_mut(&frame) else {
            return Err(BadMessageReason::CommitOfUnknownFrame);
        };
        node.origin = origin.origin;
        node.storage_key = storage_key;
        node.local = origin.local;
        node.sandbox = sandbox;
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
        sandbox: SandboxFlags,
    ) -> Result<(), BadMessageReason> {
        if self.nodes.contains_key(&frame) {
            return Err(BadMessageReason::FrameTokenInUse);
        }
        if !self.nodes.contains_key(&parent) {
            return Err(BadMessageReason::FrameParentUnknown);
        }
        let unique_name = crate::unique_name::for_new_child(self, parent, name, created_by_script);
        let Some(node) = self.nodes.get_mut(&parent) else {
            return Err(BadMessageReason::FrameParentUnknown);
        };
        node.children.push(frame);
        // Its initial `about:blank` has its parent's origin.
        let origin = CommittedOrigin {
            origin: node.origin.clone(),
            local: node.local,
        };
        // Sandboxed by its iframe's flags and its parent's, its initial
        // `about:blank` too (whose origin is then an opaque one).
        let sandbox = sandbox | node.sandbox;
        let origin = if sandbox.contains(SandboxFlags::ORIGIN) {
            CommittedOrigin::opaque()
        } else {
            origin
        };
        let storage_key = self.storage_key_in(Some(parent), &origin.origin);
        let mut child = FrameTreeNode::new(Some(parent), unique_name, origin, storage_key);
        child.pending_sandbox = sandbox;
        child.sandbox = sandbox;
        self.nodes.insert(frame, child);
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
    /// the process is gone). What was sent to commit goes; its navigation
    /// on its way, if the caller left it, goes on, to commit in the next
    /// renderer.
    pub fn reset(&mut self) {
        let navigation = self
            .nodes
            .get_mut(&self.root)
            .and_then(|root| root.navigation.take());
        self.nodes.clear();
        let origin = CommittedOrigin::opaque();
        let storage_key = StorageKey::first_party(&origin.origin);
        let mut root = FrameTreeNode::new(None, String::new(), origin, storage_key);
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

    /// `frame` commits navigation `id`'s document at `url`, of `url`'s
    /// origin.
    fn commit_at(tree: &mut FrameTree, frame: FrameToken, id: u64, url: &str) {
        let origin = CommittedOrigin {
            origin: url::Url::parse(url).unwrap().origin(),
            local: false,
        };
        tree.committing(
            frame,
            id,
            url,
            origin,
            protocol::sandbox::SandboxFlags::NONE,
        );
        tree.did_commit(frame, id, url).unwrap();
    }

    /// A child frame's sandbox flags are its iframe's with its parent's
    /// document's, as it is made (its initial `about:blank` of an opaque
    /// origin with the origin flag) and as its iframe's change, for its
    /// next document; the parent's count as its document committed them.
    /// The main frame's are no iframe's: changing them is a bad message.
    #[test]
    fn a_frames_sandbox_flags_are_its_iframes_and_its_parents() {
        use protocol::sandbox::{SandboxFlags, parse};
        let mut tree = FrameTree::new(token(0));
        commit_at(&mut tree, token(0), 1, "https://a.example/");
        let scripts = parse("allow-same-origin").flags;
        tree.add_child(token(0), token(1), "", false, SandboxFlags::NONE)
            .unwrap();
        assert_eq!(tree.sandbox(token(1)), SandboxFlags::NONE);
        assert!(tree.origin(token(1)).unwrap().is_tuple());
        let all = parse("").flags;
        tree.add_child(token(0), token(2), "", false, all).unwrap();
        assert!(!tree.origin(token(2)).unwrap().is_tuple());
        // Its own document's, as it committed; then its child's.
        tree.committing(
            token(1),
            2,
            "https://a.example/child",
            CommittedOrigin {
                origin: url::Url::parse("https://a.example/").unwrap().origin(),
                local: false,
            },
            scripts,
        );
        tree.did_commit(token(1), 2, "https://a.example/child")
            .unwrap();
        tree.add_child(token(1), token(3), "", false, SandboxFlags::NONE)
            .unwrap();
        assert_eq!(tree.sandbox(token(3)), scripts);
        let forms = parse("allow-same-origin allow-scripts").flags;
        tree.set_frame_policy(token(3), forms).unwrap();
        assert_eq!(tree.pending_sandbox(token(3)), forms | scripts);
        assert_eq!(tree.sandbox(token(3)), scripts, "the next document's");
        tree.set_frame_policy(token(1), SandboxFlags::NONE).unwrap();
        assert_eq!(tree.pending_sandbox(token(1)), SandboxFlags::NONE);
        assert_eq!(
            tree.set_frame_policy(token(0), all),
            Err(BadMessageReason::FramePolicyOfUnknownFrame)
        );
        assert_eq!(
            tree.set_frame_policy(token(9), all),
            Err(BadMessageReason::FramePolicyOfUnknownFrame)
        );
    }

    /// Each frame's storage key is worked out as its document commits, and
    /// as it is made (its initial `about:blank`'s, of its parent's origin):
    /// of the main frame's site, and a third party's below a frame of
    /// another site. The main frame alone again has an opaque one.
    #[test]
    fn each_frames_storage_key_is_of_the_frames_it_is_in() {
        let mut tree = FrameTree::new(token(0));
        assert!(tree.storage_key(token(0)).unwrap().is_third_party());
        commit_at(&mut tree, token(0), 1, "https://a.example/");
        let main = tree.storage_key(token(0)).unwrap().clone();
        assert_eq!(
            main,
            StorageKey::first_party(&url::Url::parse("https://a.example/").unwrap().origin())
        );
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        assert_eq!(tree.storage_key(token(1)), Some(&main));
        commit_at(&mut tree, token(1), 2, "https://b.example/");
        let child = tree.storage_key(token(1)).unwrap();
        assert!(child.is_third_party());
        assert_eq!(child.top_level_site(), main.top_level_site());
        // `a.example` again, in `b.example`: a third party's.
        tree.add_child(
            token(1),
            token(2),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        commit_at(&mut tree, token(2), 3, "https://www.a.example/");
        let grandchild = tree.storage_key(token(2)).unwrap();
        assert!(grandchild.is_third_party());
        assert_eq!(grandchild.top_level_site(), main.top_level_site());
        // Of the main frame's site, in it directly: first party.
        tree.add_child(
            token(0),
            token(3),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        commit_at(&mut tree, token(3), 4, "https://www.a.example/");
        assert!(!tree.storage_key(token(3)).unwrap().is_third_party());
        assert_eq!(tree.storage_key(token(9)), None);
        tree.reset();
        assert!(tree.storage_key(token(0)).unwrap().is_third_party());
    }

    /// A loader's params: the document's origin, the site it is first
    /// party to (none below a frame of another site), the top-level frame's
    /// site, by the frames it is in as it commits, and its own site.
    #[test]
    fn a_loaders_params_are_of_the_frames_its_document_is_in() {
        let origin = |url: &str| url::Url::parse(url).unwrap().origin();
        let sites = |params: &LoaderParams| {
            (
                params.trusted.site_for_cookies.clone(),
                params.trusted.top_frame_site.clone(),
            )
        };
        let a = || Some("https://a.example".to_owned());
        let mut tree = FrameTree::new(token(0));
        commit_at(&mut tree, token(0), 1, "https://a.example/");
        let main = tree.loader_params(token(0), &origin("https://www.a.example/"));
        assert_eq!(main.origin, origin("https://www.a.example/"));
        assert_eq!(sites(&main), (a(), a()));
        // Its requests' initiator is its document's origin, none a
        // navigation's.
        assert_eq!(
            main.trusted.initiator.as_deref(),
            Some("https://www.a.example")
        );
        assert!(!main.trusted.is_main_frame_navigation);
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        assert_eq!(
            sites(&tree.loader_params(token(1), &origin("https://sub.a.example/"))),
            (a(), a())
        );
        let third = tree.loader_params(token(1), &origin("https://b.example/"));
        assert_eq!(sites(&third), (None, a()));
        // Its own site, not the top-level frame's.
        assert_eq!(
            third.trusted.frame_site.as_deref(),
            Some("https://b.example")
        );
        assert_eq!(main.trusted.frame_site, a());
        // `a.example` in `b.example` in `a.example`: no site's first party.
        commit_at(&mut tree, token(1), 2, "https://b.example/");
        tree.add_child(
            token(1),
            token(2),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        let below = tree.loader_params(token(2), &origin("https://a.example/"));
        assert_eq!(below.trusted.site_for_cookies, None);
        // An opaque top-level frame: no site; its initiator `null`.
        let opaque = tree.loader_params(token(0), &url::Url::parse("data:,").unwrap().origin());
        assert_eq!(sites(&opaque), (None, None));
        assert_eq!(opaque.trusted.frame_site, None);
        assert_eq!(opaque.trusted.initiator.as_deref(), Some("null"));
    }

    /// A navigation's trusted params: a main frame's, first party to its
    /// URL's site, which is its top-level site; a child frame's, its
    /// parent's; the initiator the frame that asked for it, none for the
    /// browser's own; no frame site (its URL's at each hop).
    #[test]
    fn a_navigations_params_are_of_its_frame_and_initiator() {
        let mut tree = FrameTree::new(token(0));
        commit_at(&mut tree, token(0), 1, "https://a.example/");
        let main = tree.navigation_params(token(0), "https://www.b.example/x", None);
        assert_eq!(main.site_for_cookies.as_deref(), Some("https://b.example"));
        assert_eq!(main.top_frame_site.as_deref(), Some("https://b.example"));
        assert_eq!(main.initiator, None);
        assert!(main.is_main_frame_navigation);
        assert_eq!(main.frame_site, None);
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        commit_at(&mut tree, token(1), 2, "https://c.example/");
        let asked = tree.navigation_params(token(0), "https://b.example/", Some(token(1)));
        assert_eq!(asked.initiator.as_deref(), Some("https://c.example"));
        let child = tree.navigation_params(token(1), "https://d.example/", Some(token(0)));
        assert_eq!(child.site_for_cookies.as_deref(), Some("https://a.example"));
        assert_eq!(child.top_frame_site.as_deref(), Some("https://a.example"));
        assert_eq!(child.initiator.as_deref(), Some("https://a.example"));
        assert!(!child.is_main_frame_navigation);
        // Not its parent's.
        assert_eq!(child.frame_site, None);
    }

    /// Child frames hang from the frame they were made in, in the order
    /// they were made; a frame removed takes the frames in it with it.
    #[test]
    fn frames_come_and_go_with_the_frames_in_them() {
        let mut tree = FrameTree::new(token(0));
        assert_eq!(tree.root(), token(0));
        assert_eq!(tree.len(), 1);
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        tree.add_child(
            token(0),
            token(2),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        tree.add_child(
            token(1),
            token(3),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        tree.add_child(
            token(3),
            token(4),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
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
        tree.add_child(
            token(2),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
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
        tree.committing(
            token(0),
            1,
            "file:///a.html",
            local,
            protocol::sandbox::SandboxFlags::NONE,
        );
        tree.did_commit(token(0), 1, "file:///a.html").unwrap();
        assert_eq!(tree.url(token(0)), Some("file:///a.html"));
        assert!(tree.may_open_local_files(token(0)));
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        assert!(tree.may_open_local_files(token(1)), "its parent's");
        assert_eq!(tree.url(token(1)), None);
        let parents = tree.committed_origin(token(0)).unwrap();
        tree.committing(
            token(1),
            2,
            "file:///b.html",
            parents,
            protocol::sandbox::SandboxFlags::NONE,
        );
        tree.did_commit(token(1), 2, "file:///b.html").unwrap();
        assert!(tree.may_open_local_files(token(1)), "as sent");
        let web = url::Url::parse("https://a.test/").unwrap().origin();
        let committed = CommittedOrigin {
            origin: web.clone(),
            local: false,
        };
        tree.committing(
            token(0),
            4,
            "https://a.test/",
            committed,
            protocol::sandbox::SandboxFlags::NONE,
        );
        tree.did_commit(token(0), 4, "https://a.test/").unwrap();
        assert!(!tree.may_open_local_files(token(0)));
        assert_eq!(tree.origin(token(0)), Some(&web));
        assert_ne!(tree.origin(token(1)), Some(&web), "a local file's");
        tree.add_child(
            token(0),
            token(2),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
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
        commit(&mut tree, token(2), 6, "about:blank#x");
        assert!(!tree.has_committed_real_load(token(2)));
        commit(&mut tree, token(2), 7, "about:srcdoc");
        assert!(tree.has_committed_real_load(token(2)));
        commit(&mut tree, token(2), 8, "about:blank");
        assert!(tree.has_committed_real_load(token(2)));
        tree.reset();
        assert_eq!(tree.url(token(0)), None);
    }

    /// A commit reported is one the browser sent the frame, at the URL it
    /// sent (those sent before it go: the renderer dropped them), or a
    /// child frame's own `javascript:` URL's, of an opaque origin;
    /// anything else is a bad message.
    #[test]
    fn a_commit_is_one_the_browser_sent() {
        let mut tree = FrameTree::new(token(0));
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        let web = |url: &str| CommittedOrigin {
            origin: url::Url::parse(url).unwrap().origin(),
            local: false,
        };
        tree.committing(
            token(0),
            1,
            "https://a.test/",
            web("https://a.test/"),
            protocol::sandbox::SandboxFlags::NONE,
        );
        tree.committing(
            token(0),
            2,
            "https://b.test/",
            web("https://b.test/"),
            protocol::sandbox::SandboxFlags::NONE,
        );
        assert_eq!(
            tree.did_commit(token(0), 3, "https://a.test/"),
            Err(BadMessageReason::CommitOfNoNavigation)
        );
        assert_eq!(
            tree.did_commit(token(1), 1, "https://a.test/"),
            Err(BadMessageReason::CommitOfNoNavigation),
            "another frame's"
        );
        assert_eq!(
            tree.did_commit(token(0), 2, "https://a.test/"),
            Err(BadMessageReason::CommitOfAnotherUrl)
        );
        tree.did_commit(token(0), 2, "https://b.test/").unwrap();
        assert_eq!(tree.url(token(0)), Some("https://b.test/"));
        assert!(
            tree.origin(token(0))
                .is_some_and(|origin| origin.is_tuple())
        );
        for (id, url) in [(1, "https://a.test/"), (2, "https://b.test/")] {
            assert_eq!(
                tree.did_commit(token(0), id, url),
                Err(BadMessageReason::CommitOfNoNavigation),
                "{id}: gone"
            );
        }
        // The renderer's own: a child frame's `javascript:` URL alone.
        let renderer = protocol::renderer::RENDERER_COMMIT;
        for (frame, url) in [(token(0), "javascript:1"), (token(1), "https://a.test/")] {
            assert_eq!(
                tree.did_commit(frame, renderer, url),
                Err(BadMessageReason::CommitOfNoNavigation),
                "{url}"
            );
        }
        tree.did_commit(token(1), renderer, "JavaScript:1").unwrap();
        assert!(
            tree.origin(token(1))
                .is_some_and(|origin| !origin.is_tuple())
        );
    }

    /// The browser sent `frame` navigation `id` to commit at `url`, of an
    /// opaque origin, and the renderer committed it.
    fn commit(tree: &mut FrameTree, frame: FrameToken, id: u64, url: &str) {
        tree.committing(
            frame,
            id,
            url,
            CommittedOrigin::opaque(),
            protocol::sandbox::SandboxFlags::NONE,
        );
        tree.did_commit(frame, id, url).unwrap();
    }

    /// A frame's document within itself takes what HTML lets it: any path
    /// and query of a web page's origin, a local file's fragment and
    /// query, another URL's fragment alone (an initial `about:blank`'s as
    /// `about:blank`'s); anything else, and a frame not there, is a bad
    /// message, which leaves its URL as it was.
    #[test]
    fn a_document_navigates_within_itself_as_html_lets_it() {
        let mut tree = FrameTree::new(token(0));
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
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
        commit(&mut tree, token(0), 1, "https://a.test/p?q#f");
        commit(&mut tree, token(1), 2, "file:///d/f.html");
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
        commit(&mut tree, token(1), 3, "data:text/html,x");
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
        tree.add_child(
            token(0),
            token(1),
            "",
            false,
            protocol::sandbox::SandboxFlags::NONE,
        )
        .unwrap();
        assert_eq!(
            tree.add_child(
                token(9),
                token(2),
                "",
                false,
                protocol::sandbox::SandboxFlags::NONE
            ),
            Err(BadMessageReason::FrameParentUnknown)
        );
        assert_eq!(
            tree.add_child(
                token(0),
                token(1),
                "",
                false,
                protocol::sandbox::SandboxFlags::NONE
            ),
            Err(BadMessageReason::FrameTokenInUse)
        );
        assert_eq!(
            tree.add_child(
                token(1),
                token(0),
                "",
                false,
                protocol::sandbox::SandboxFlags::NONE
            ),
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
