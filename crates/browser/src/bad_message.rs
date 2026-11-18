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
}
