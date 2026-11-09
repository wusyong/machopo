//! `cc::SwapPromise`: something to do once a commit's content is drawn.

/// Called once with the token of the first frame that shows the commit it
/// came with, or with `None` if no frame could be drawn (an empty viewport,
/// or a hidden tab for an input ack). A commit that a newer one overtakes
/// before it is drawn hands its promises on: the frame that shows the
/// newer commit shows it too.
///
/// Chromium's has `WillSwap` / `DidSwap` / `DidNotSwap(reason)`; this keeps
/// the one answer the renderer needs, the frame (`DidFinishLoad`, input
/// acks). Dropped without a call if the compositor shuts down first.
pub struct SwapPromise {
    on_swap: Box<dyn FnOnce(Option<u64>) + Send>,
    input_ack: bool,
}

impl SwapPromise {
    pub fn new(on_swap: impl FnOnce(Option<u64>) + Send + 'static) -> Self {
        Self {
            on_swap: Box::new(on_swap),
            input_ack: false,
        }
    }

    /// An input event's ack. Chromium acks input once it is handled,
    /// whatever is drawn; this waits for the frame that shows what the
    /// input changed, so what is asked of the page next sees it. A hidden
    /// tab draws no frame: hiding it, or anything it handles while hidden,
    /// answers this at once with `None`. Other promises wait for the frame
    /// drawn once the tab is shown again, as Chromium's do (it breaks them
    /// only for a main frame that started before the tab was hidden).
    pub fn input_ack(on_swap: impl FnOnce(Option<u64>) + Send + 'static) -> Self {
        Self {
            input_ack: true,
            ..Self::new(on_swap)
        }
    }

    pub(crate) fn is_input_ack(&self) -> bool {
        self.input_ack
    }

    pub(crate) fn fulfill(self, frame_token: Option<u64>) {
        (self.on_swap)(frame_token)
    }

    /// `DidNotSwap`: no frame will show it.
    pub fn did_not_swap(self) {
        self.fulfill(None)
    }
}
