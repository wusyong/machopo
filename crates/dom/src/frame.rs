//! An `<iframe>`'s content document (HTML's content navigable's active
//! document; Blink's `HTMLFrameOwnerElement::ContentDocument` and
//! `Document::LocalOwner`): kept on both sides, the iframe's data and the
//! document's. What the engine does as an iframe comes or goes, or its
//! `src` or `srcdoc` changes (DOM's post-connection steps, HTML's removing
//! and attribute change steps), and as a script element is to be prepared
//! (`script`), it does once the operation is over: the DOM records each
//! change here, in order (tree order for a subtree connected: a script
//! runs after the iframes before it got their browsing contexts, before
//! those after it), and asks for the engine's attention
//! (`Microtasks::elements`, taken after each call, as Blink does them as
//! the operation ends: `DidNotifySubtreeInsertionsToDocument`). Nothing is
//! recorded before the first `<iframe>`, or script element that may run,
//! is made.

use std::cell::Cell;

use elise::Gc;

use crate::{Cx, Microtasks, NodeId, NodeKind, NodeRef, QualifiedName, names};

/// What happened to an element that has steps for the engine to run once
/// the operation is over (`Cx::take_element_changes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementChange {
    Frame(FrameOwnerChange),
    /// A script element that isn't parser-inserted or already started
    /// became connected, had a node inserted while connected, or got a
    /// `src` while connected: it is to be prepared (HTML's "prepare the
    /// script element", which looks again).
    Script(NodeId),
    /// A `<link rel=modulepreload>` became connected (not in a shadow
    /// tree), or its `rel`, `href`, `as` or `crossorigin` changed while
    /// connected (to another value): it is to be processed (HTML's, Blink's
    /// `HTMLLinkElement::Process`).
    ModulePreload(NodeId),
    /// An element with an `autofocus` attribute became connected (HTML's
    /// "autofocus" steps are to run for it).
    Autofocus(NodeId),
}

/// What happened to an `<iframe>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOwnerChange {
    /// Connected: it gets a child browsing context, if its document is
    /// shown in one.
    Inserted(NodeId),
    /// Disconnected: its browsing context goes, and its document.
    Removed(NodeId),
    /// Its `src` or `srcdoc` changed, connected.
    SrcChanged(NodeId),
    SrcdocChanged(NodeId),
}

#[derive(Default)]
pub(crate) struct FrameState {
    changes: Cell<Vec<ElementChange>>,
    navigations: Cell<Vec<NavigationRequest>>,
    blob_urls: Cell<Vec<BlobUrlRequest>>,
    /// `fetch()`s to start and to cancel, in order (`fetch`).
    pub(crate) fetches: Cell<Vec<crate::FetchHostRequest>>,
}

/// A blob URL script registered or revoked (`URL.createObjectURL`,
/// `revokeObjectURL`), for the host, which keeps the URLs (Chrome's
/// `BlobURLStore`, not the DOM).
#[derive(Debug, Clone)]
pub enum BlobUrlRequest {
    /// `url` stands for `data` now, for as long as `document`'s realm (the
    /// one that made it) lives.
    Register {
        document: NodeId,
        url: String,
        data: crate::BlobData,
    },
    /// `url` stands for nothing now.
    Revoke { url: String },
}

/// A navigation script asked for (`location`, `history`): of the frame
/// showing `document`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigationRequest {
    pub document: NodeId,
    pub navigation: FrameNavigation,
}

/// What a frame's navigation is, for its host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameNavigation {
    /// To another document at `url` (resolved; HTML's navigate): the host
    /// loads it (the top-level frame's, the browser), its entry taking the
    /// current one's place if `replace`.
    To { url: String, replace: bool },
    /// `location.reload()`, `history.go(0)`.
    Reload,
    /// Within the document, which happened (HTML's navigate to a fragment,
    /// the URL and history update steps of `pushState` and
    /// `replaceState`): its URL is `url` now, its new entry (or the
    /// current one, if `replace`) has `state` (serialized; none: `null`);
    /// `scroll`: it scrolls to the fragment (`pushState` doesn't);
    /// `pop_state`: `popstate` fires at its window now (a fragment's).
    SameDocument {
        url: String,
        replace: bool,
        state: Option<Vec<u8>>,
        scroll: bool,
        pop_state: bool,
    },
    /// A hyperlink followed to `url` in a new tab (the browser's), in the
    /// background or not.
    NewTab { url: String, background: bool },
    /// `history.go(delta)` (and `back()`, `forward()`): the session
    /// history traverses (the browser's).
    GoToOffset(i32),
    /// `history.scrollRestoration` changed: `manual` or not.
    ScrollRestoration { manual: bool },
}

impl<'gc> Cx<'_, 'gc> {
    /// Whether `node` is an HTML `<iframe>`.
    pub fn is_iframe(&self, node: NodeRef<'gc>) -> bool {
        self.qualified_name(node) == Some(QualifiedName::html(names::local::IFRAME))
    }

    /// An `<iframe>` was made.
    pub(crate) fn iframe_made(&self) {
        self.see(crate::form::IFRAMES_SEEN);
    }

    /// Whether an `<iframe>` was ever made: before then, nothing is looked
    /// at as a subtree is disconnected or an attribute changes.
    #[inline]
    pub(crate) fn has_seen_iframes(&self) -> bool {
        self.has_seen(crate::form::IFRAMES_SEEN)
    }

    /// Whether an `<iframe>` or a `<base>` was ever made: before then,
    /// nothing is looked at as a subtree is disconnected.
    #[inline]
    pub(crate) fn has_seen_removal_steps(&self) -> bool {
        self.has_seen(crate::form::IFRAMES_SEEN | crate::form::BASES_SEEN)
    }

    /// Whether an `<iframe>`, or a script element that may run as it is
    /// inserted, was ever made: before then, nothing is looked at as a
    /// subtree is connected (one test of the word for both).
    #[inline]
    pub(crate) fn has_seen_connection_steps(&self) -> bool {
        self.has_seen(
            crate::form::IFRAMES_SEEN
                | crate::form::SCRIPTS_SEEN
                | crate::form::PRELOADS_SEEN
                | crate::form::BASES_SEEN
                | crate::form::AUTOFOCUS_SEEN
                | crate::form::PRAGMAS_SEEN,
        )
    }

    pub(crate) fn record_element_change(&self, change: ElementChange) {
        let state = &self.changes.frames;
        let mut changes = state.changes.take();
        changes.push(change);
        state.changes.set(changes);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | Microtasks::ELEMENTS);
    }

    /// `node` connected, as its subtree is walked in tree order: an
    /// `<iframe>`, or a script element to prepare, recorded. Out of line:
    /// the walk stays small.
    #[inline(never)]
    pub(crate) fn connection_steps(&self, node: NodeRef<'gc>) {
        // Any namespace's (HTML's `HTMLOrSVGElement`'s).
        if self.has_seen(crate::form::AUTOFOCUS_SEEN)
            && self.kind(node) == NodeKind::Element
            && self
                .attribute_value_by_id(node, names::local::AUTOFOCUS)
                .is_some()
        {
            self.record_element_change(ElementChange::Autofocus(self.node_id(node)));
        }
        if self.is_iframe(node) {
            self.record_element_change(ElementChange::Frame(FrameOwnerChange::Inserted(
                self.node_id(node),
            )));
            // Script that inserted it made its frame and asked for its
            // document.
            self.element_asked_for_load(node, true);
        } else {
            self.script_connected(node);
            self.link_connected(node);
            self.base_changed(node);
            if self.has_seen(crate::form::PRAGMAS_SEEN) {
                self.pragma_connected(node);
            }
        }
    }

    /// `node` about to be disconnected, as its subtree is walked: an
    /// `<iframe>` recorded. Out of line: the walk stays small.
    #[inline(never)]
    pub(crate) fn frame_owner_removed(&self, node: NodeRef<'gc>) {
        if self.is_iframe(node) {
            self.record_element_change(ElementChange::Frame(FrameOwnerChange::Removed(
                self.node_id(node),
            )));
        } else {
            self.base_changed(node);
        }
    }

    /// `element`'s attribute `name` changed from `old` to `new`, and it is
    /// connected: an `<iframe>`'s `src` or `srcdoc` recorded; a
    /// modulepreload link's (`script`).
    #[inline(never)]
    pub(crate) fn frame_owner_attribute_changed(
        &self,
        element: NodeRef<'gc>,
        name: QualifiedName,
        old: Option<&str>,
        new: Option<&str>,
    ) {
        if !self.is_iframe(element) {
            if name == QualifiedName::plain(names::local::HREF) {
                self.base_changed(element);
            }
            if self.has_seen(crate::form::PRAGMAS_SEEN)
                && (name == QualifiedName::plain(names::local::HTTP_EQUIV)
                    || name == QualifiedName::plain(names::local::CONTENT))
            {
                self.pragma_connected(element);
            }
            self.link_attribute_changed(element, name, old, new);
            return;
        }
        let id = self.node_id(element);
        if name == QualifiedName::plain(names::local::SRC) {
            self.record_element_change(ElementChange::Frame(FrameOwnerChange::SrcChanged(id)));
            self.element_asked_for_load(element, false);
        } else if name == QualifiedName::plain(names::local::SRCDOC) {
            self.record_element_change(ElementChange::Frame(FrameOwnerChange::SrcdocChanged(id)));
        }
    }

    /// Whether something happened to an `<iframe>` or a script element
    /// that its steps haven't taken yet.
    pub fn has_element_changes(&self) -> bool {
        let state = &self.changes.frames;
        let changes = state.changes.take();
        let any = !changes.is_empty();
        state.changes.set(changes);
        any
    }

    /// The frame showing `document` is to navigate to `url` (HTML's
    /// Location-object navigate): recorded for the engine, which looks
    /// once the operation is over (`Microtasks::frames`).
    pub fn request_navigation(&self, document: NodeRef<'gc>, navigation: FrameNavigation) {
        let state = &self.changes.frames;
        let mut navigations = state.navigations.take();
        navigations.push(NavigationRequest {
            document: self.node_id(document),
            navigation,
        });
        state.navigations.set(navigations);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | Microtasks::ELEMENTS);
    }

    /// A blob URL registered or revoked: the host told after the call.
    pub fn request_blob_url(&self, request: BlobUrlRequest) {
        let state = &self.changes.frames;
        let mut requests = state.blob_urls.take();
        requests.push(request);
        state.blob_urls.set(requests);
        self.changes
            .microtasks
            .set(self.changes.microtasks.get() | Microtasks::ELEMENTS);
    }

    /// The blob URLs registered and revoked since the last time, in order,
    /// taken.
    pub fn take_blob_url_requests(&self) -> Vec<BlobUrlRequest> {
        self.changes.frames.blob_urls.take()
    }

    /// The navigations asked for since the last time, in order, taken.
    pub fn take_navigation_requests(&self) -> Vec<NavigationRequest> {
        self.changes.frames.navigations.take()
    }

    /// What happened to the `<iframe>`s and script elements since the last
    /// time, in order, taken.
    pub fn take_element_changes(&self) -> Vec<ElementChange> {
        self.changes.frames.changes.take()
    }

    /// `owner`'s content document (an `<iframe>`'s), if it has a browsing
    /// context.
    pub fn content_document(&self, owner: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.existing_element_data(owner)?
            .into_ref()
            .content_document
            .get(self.gc)
    }

    /// The `<iframe>` whose content document `document` is, if it is one.
    pub fn frame_owner(&self, document: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        self.document_data(document)?
            .into_ref()
            .frame_owner
            .get(self.gc)
    }

    /// `owner`'s content document is `document` now (none: its browsing
    /// context went); the one before is no longer its. One its iframe
    /// navigated from is discarded, with its iframes' (HTML's "unload a
    /// document" destroys its child navigables: machopo keeps no document in a
    /// back/forward cache), so that it is no longer fully active.
    pub fn set_content_document(&self, owner: NodeRef<'gc>, document: Option<NodeRef<'gc>>) {
        debug_assert_eq!(self.kind(owner), NodeKind::Element);
        let data = self.element_data(owner);
        if let Some(old) = data.content_document.get(self.gc) {
            self.keep_about_base_url(old);
            if let Some(old_data) = self.document_data(old) {
                old_data.frame_owner.set(self.gc, None);
            }
            if let Some(document) = document
                && !Gc::ptr_eq(old, document)
            {
                self.discard_document_tree(old);
            }
        }
        let new = document.filter(|&document| {
            data.content_document
                .get(self.gc)
                .is_none_or(|old| !Gc::ptr_eq(old, document))
        });
        data.content_document.set(self.gc, document);
        if let Some(document) = document {
            self.ensure_document_data(document)
                .frame_owner
                .set(self.gc, Some(owner));
        }
        // A new `about:blank` takes its creator's base URL as it is now.
        if let Some(document) = new {
            self.copy_about_base_url(owner, document);
        }
    }

    /// HTML's "destroy a child navigable" for `owner`, as it is removed:
    /// its content document discarded, and those of the iframes in it, down
    /// (shadow trees too), each no longer its iframe's. Its content
    /// document, if it had one.
    pub fn discard_content_document(&self, owner: NodeRef<'gc>) -> Option<NodeRef<'gc>> {
        let document = self.content_document(owner)?;
        self.set_content_document(owner, None);
        self.discard_document_tree(document);
        Some(document)
    }

    /// The content documents of the iframes in `document`, in
    /// shadow-including tree order (HTML's order of the documents a
    /// rendering update goes through).
    pub fn child_content_documents(&self, document: NodeRef<'gc>) -> Vec<NodeRef<'gc>> {
        let mut documents = Vec::new();
        if !self.has_seen_iframes() {
            return documents;
        }
        self.for_each_shadow_including_inclusive_descendant(document, |node, _| {
            documents.extend(self.content_document(node));
        });
        documents
    }

    /// The top-level document of `document`'s frame tree: up its iframes'
    /// documents.
    pub fn top_document(&self, document: NodeRef<'gc>) -> NodeRef<'gc> {
        let mut at = document;
        while let Some(owner) = self.frame_owner(at) {
            at = self.document_of(owner);
        }
        at
    }

    /// The document of the focused frame of `top`'s frame tree (Blink's
    /// `FocusController::FocusedFrame`): the one set last if a frame of the
    /// tree still shows it, else `top`.
    pub fn focused_frame(&self, top: NodeRef<'gc>) -> NodeRef<'gc> {
        self.document_data(top)
            .and_then(|data| data.into_ref().focused_frame.get(self.gc))
            // One discarded, or navigated from, is no iframe's any more.
            .filter(|&document| Gc::ptr_eq(self.top_document(document), top))
            .unwrap_or(top)
    }

    /// The focused frame of `top`'s frame tree shows `document` now.
    pub fn set_focused_frame(&self, top: NodeRef<'gc>, document: NodeRef<'gc>) {
        self.ensure_document_data(top)
            .focused_frame
            .set(self.gc, (!Gc::ptr_eq(document, top)).then_some(document));
    }

    /// `element` is one of `top`'s autofocus candidates, last, unless it
    /// is one already.
    pub fn add_autofocus_candidate(&self, top: NodeRef<'gc>, element: NodeRef<'gc>) {
        let id = self.node_id(element);
        let data = self.ensure_document_data(top);
        let mut candidates = data.autofocus_candidates.take();
        if !candidates.contains(&id) {
            candidates.push(id);
        }
        data.autofocus_candidates.set(candidates);
    }

    /// `top`'s autofocus candidates, taken.
    pub fn take_autofocus_candidates(&self, top: NodeRef<'gc>) -> Vec<NodeId> {
        self.document_data(top)
            .map(|data| data.into_ref().autofocus_candidates.take())
            .unwrap_or_default()
    }

    /// Whether `top` has autofocus candidates and hasn't processed its
    /// autofocus.
    pub fn has_autofocus_candidates(&self, top: NodeRef<'gc>) -> bool {
        self.document_data(top).is_some_and(|data| {
            let data = data.into_ref();
            let candidates = data.autofocus_candidates.take();
            let has = !candidates.is_empty();
            data.autofocus_candidates.set(candidates);
            has && !data.autofocus_processed.get()
        })
    }

    /// HTML's autofocus processed flag of `top`.
    pub fn autofocus_processed(&self, top: NodeRef<'gc>) -> bool {
        self.document_data(top)
            .is_some_and(|data| data.into_ref().autofocus_processed.get())
    }

    /// `top`'s autofocus has run: its candidates go.
    pub fn set_autofocus_processed(&self, top: NodeRef<'gc>) {
        let data = self.ensure_document_data(top);
        data.autofocus_processed.set(true);
        data.autofocus_candidates.set(Vec::new());
    }

    /// `document` discarded, and the content documents of the iframes in
    /// it, down (shadow trees too), each no longer its iframe's.
    fn discard_document_tree(&self, document: NodeRef<'gc>) {
        self.changes.discarded_documents.set(true);
        let mut documents = vec![document];
        while let Some(at) = documents.pop() {
            self.ensure_document_data(at).discarded.set(true);
            self.release_document_images(at);
            self.for_each_shadow_including_inclusive_descendant(at, |node, _| {
                if let Some(inner) = self.content_document(node) {
                    self.set_content_document(node, None);
                    documents.push(inner);
                }
            });
        }
    }

    /// Whether a document was ever discarded (`is_discarded`).
    #[inline]
    pub fn has_discarded_documents(&self) -> bool {
        self.changes.discarded_documents.get()
    }

    /// Whether `document` was discarded (`discard_content_document`, or its
    /// iframe navigated from it): it
    /// has no browsing context, and makes none for its iframes.
    pub fn is_discarded(&self, document: NodeRef<'gc>) -> bool {
        self.document_data(document)
            .is_some_and(|data| data.into_ref().discarded.get())
    }
}
