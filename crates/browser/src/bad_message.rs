//! What a renderer sent that the browser can't take (Chromium's
//! `content/browser/bad_message.h`): the browser ends that renderer
//! (`WebContents::received_bad_message`), as Chromium's
//! `bad_message::ReceivedBadMessage` kills its process. While the renderer
//! is a thread of the browser's process, a bad message is machopo's own
//! bug; ending the renderer shows it at once, and nothing goes on wrong.

/// Why a renderer's message was bad (Chromium's `BadMessageReason`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BadMessageReason {
    /// A child frame made in a frame the tab doesn't have.
    FrameParentUnknown,
    /// A child frame made with the token of a frame the tab has.
    FrameTokenInUse,
    /// A frame the tab doesn't have gone.
    DetachUnknownFrame,
    /// The main frame gone, as a child frame goes (Chromium's
    /// `RFH_DETACH_MAIN_FRAME`).
    DetachMainFrame,
    /// A navigation of a frame the tab doesn't have.
    NavigationOfUnknownFrame,
    /// A navigation a frame the tab doesn't have asked for.
    NavigationByUnknownFrame,
    /// A commit in a frame the tab doesn't have.
    CommitOfUnknownFrame,
    /// A navigation within the document of a frame the tab doesn't have.
    SameDocumentOfUnknownFrame,
    /// A navigation within a document to a URL it can't have without
    /// loading another (HTML's "can have its URL rewritten"; Chromium's
    /// `cross_origin_same_document`).
    SameDocumentToAnotherUrl,
    /// A `srcdoc` of a frame the tab doesn't have as a child frame
    /// (Chromium's `RFH_OWNER_PROPERTY`).
    SrcdocOfUnknownFrame,
    /// A scroll restoration mode (`UpdateState`) of a frame the tab
    /// doesn't have (Chromium's comes from the frame's own host).
    StateOfUnknownFrame,
    /// A commit of a navigation the browser didn't send the frame to commit
    /// (or has heard of), or the renderer's own of something but a child
    /// frame's `javascript:` URL (Chromium's
    /// `RFH_NO_MATCHING_NAVIGATION_REQUEST_ON_COMMIT`).
    CommitOfNoNavigation,
    /// A commit at another URL than the browser sent (Chromium's
    /// `RFH_ERROR_PAGE_URL_MISMATCH`, and its process lock's check).
    CommitOfAnotherUrl,
    /// A request for a document (`RequestDestination::Document` or
    /// `Iframe`) as a subresource: the browser loads every frame's
    /// documents (network service's "frame destination requires kNavigate
    /// mode").
    DocumentRequest,
    /// A request through a loader the browser didn't give the renderer
    /// (Chromium's factory's pipe is the browser's to make).
    UnknownLoader,
    /// A worker's request through another loader than the worker's.
    WorkerLoaderMismatch,
    /// A request whose initiator is another origin than its loader's
    /// (Chromium's `INCORRECT_LOCK`, `request_initiator_origin_lock`'s
    /// check).
    InitiatorMismatch,
    /// A request with the params only the browser gives one (Chromium's
    /// `TRUSTED_PARAMS_FROM_UNTRUSTED_FACTORY`).
    TrustedParamsFromRenderer,
    /// Script's cookies through the loader of a document of an opaque
    /// origin, which has none (`RestrictedCookieManager`'s "Access is
    /// denied in this context").
    CookiesOfOpaqueOrigin,
    /// Script's cookies of a URL of another origin than the loader's
    /// (`RestrictedCookieManager`'s "Incorrect url origin").
    CookieUrlMismatch,
    /// What the channel to a renderer process itself refused: bytes that
    /// are no message, a message that is none of its kind, a handle or a
    /// pipe a renderer may not send, more queued than may wait (Mojo's
    /// `ReportBadMessage` from its own validation). The channel is over
    /// by then.
    Channel,
}
