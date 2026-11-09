//! The input events the compositor thread handed to the main thread that
//! it hasn't started to handle, by what they are for (Blink's
//! `PendingUserInput::Monitor`): what `navigator.scheduling.
//! isInputPending()` reads. The compositor thread's posting counts one in
//! (`MainThreadSchedulerImpl::WillPostInputEventToMainThread`); the main
//! thread counts it out as it takes it to handle
//! (`WillHandleInputEventOnMainThread`), a mouse move when it is
//! dispatched (or replaced by a newer one: machopo keeps only the last for the
//! next frame).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

use compositor::{DocumentId, ElementId, InputAttribution};
use protocol::input::InputEvent;

/// What a waiting event is for, as `isInputPending()` tells them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum PendingTarget {
    /// The frame with the focus.
    FocusedFrame,
    /// The document the compositor drew, which is the main frame's.
    DrawnDocument(DocumentId),
    /// A child frame's document, the element being its document node.
    Document(ElementId),
}

impl PendingTarget {
    /// What an event of `attribution` aimed at the drawn `document` is
    /// for; `None` if it can't be told, which counts for no one (Blink's
    /// `Monitor::OnEnqueue` ignores `kUnknown`).
    pub(crate) fn of(attribution: InputAttribution, document: Option<DocumentId>) -> Option<Self> {
        match attribution {
            InputAttribution::Unknown => None,
            InputAttribution::FocusedFrame => Some(Self::FocusedFrame),
            InputAttribution::TargetedFrame(Some(element)) => Some(Self::Document(element)),
            InputAttribution::TargetedFrame(None) => document.map(Self::DrawnDocument),
        }
    }
}

/// One counted event: its target, and whether it is continuous (Blink's
/// `PendingUserInput::IsContinuousEventType`: a mouse move or a wheel).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Pending {
    pub(crate) target: PendingTarget,
    pub(crate) continuous: bool,
}

impl Pending {
    pub(crate) fn of(
        event: &InputEvent,
        attribution: InputAttribution,
        document: Option<DocumentId>,
    ) -> Option<Self> {
        let continuous = matches!(
            event,
            InputEvent::MouseMove { .. } | InputEvent::MouseWheel { .. }
        );
        Some(Self {
            target: PendingTarget::of(attribution, document)?,
            continuous,
        })
    }
}

/// The counts, shared by the compositor thread and the main thread (Blink
/// keeps them under `any_thread_lock_`).
#[derive(Debug, Clone, Default)]
pub(crate) struct PendingUserInput(Arc<Mutex<HashMap<Pending, u32>>>);

impl PendingUserInput {
    /// An event handed to the main thread.
    pub(crate) fn enqueue(&self, pending: Option<Pending>) {
        let Some(pending) = pending else {
            return;
        };
        let mut counts = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let count = counts.entry(pending).or_insert(0);
        // One per event on its way to the main thread: far fewer than
        // u32::MAX wait at once.
        *count = count.saturating_add(1);
    }

    /// An event the main thread takes to handle.
    pub(crate) fn dequeue(&self, pending: Option<Pending>) {
        let Some(pending) = pending else {
            return;
        };
        let mut counts = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(count) = counts.get_mut(&pending) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                counts.remove(&pending);
            }
        }
    }

    /// What the waiting events are for (Blink's `Monitor::Info`), the
    /// continuous ones only if `include_continuous`.
    pub(crate) fn targets(&self, include_continuous: bool) -> Vec<PendingTarget> {
        let counts = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        let targets: std::collections::HashSet<_> = counts
            .keys()
            .filter(|p| include_continuous || !p.continuous)
            .map(|p| p.target)
            .collect();
        targets.into_iter().collect()
    }
}

/// What the DOM's `isInputPending()` reads (`dom_api::InputMonitor`):
/// the counts, and the document the compositor draws (its id and node),
/// whose events aimed at no child frame are for it; a child frame's
/// document's node is its element id.
pub(crate) struct PendingUserInputMonitor {
    counts: PendingUserInput,
    drawn: (DocumentId, dom::NodeId),
}

impl PendingUserInputMonitor {
    pub(crate) fn new(counts: PendingUserInput, drawn: DocumentId, document: dom::NodeId) -> Self {
        Self {
            counts,
            drawn: (drawn, document),
        }
    }
}

impl dom_api::InputMonitor for PendingUserInputMonitor {
    fn pending_input(&self, include_continuous: bool) -> Vec<dom_api::PendingInput> {
        use dom_api::PendingInput;
        self.counts
            .targets(include_continuous)
            .into_iter()
            .filter_map(|target| match target {
                PendingTarget::FocusedFrame => Some(PendingInput::FocusedFrame),
                // One aimed at the document drawn before this one counts
                // for no one.
                PendingTarget::DrawnDocument(id) => {
                    (id == self.drawn.0).then_some(PendingInput::Document(self.drawn.1))
                }
                PendingTarget::Document(element) => u32::try_from(element.0)
                    .ok()
                    .map(|raw| PendingInput::Document(dom::NodeId::from_raw(raw))),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn moved() -> InputEvent {
        InputEvent::mouse_move(1.0, 2.0)
    }

    fn pressed() -> InputEvent {
        InputEvent::mouse_down(
            1.0,
            2.0,
            protocol::input::MouseButton::Left,
            protocol::input::Modifiers::NONE,
        )
    }

    /// Each event counts until taken; a move only with the continuous ones;
    /// one that can't be told counts for no one; a drawn document's event
    /// before any document is drawn neither.
    #[test]
    fn events_count_until_taken() {
        let input = PendingUserInput::default();
        let drawn = Some(DocumentId(7));
        let press = Pending::of(&pressed(), InputAttribution::TargetedFrame(None), drawn);
        let moving = Pending::of(&moved(), InputAttribution::TargetedFrame(None), drawn);
        input.enqueue(press);
        input.enqueue(press);
        input.enqueue(moving);
        input.enqueue(Pending::of(&pressed(), InputAttribution::Unknown, drawn));
        input.enqueue(Pending::of(
            &pressed(),
            InputAttribution::TargetedFrame(None),
            None,
        ));
        assert_eq!(
            input.targets(false),
            [PendingTarget::DrawnDocument(DocumentId(7))]
        );
        assert_eq!(input.targets(true).len(), 1, "one target, counted once");
        input.dequeue(press);
        assert_eq!(input.targets(false).len(), 1, "one press still waits");
        input.dequeue(press);
        assert!(input.targets(false).is_empty());
        assert_eq!(input.targets(true).len(), 1, "the move waits");
        input.dequeue(moving);
        input.dequeue(moving);
        assert!(input.targets(true).is_empty(), "never below none");
    }

    /// Keys are the focused frame's; a child frame's events its document's.
    #[test]
    fn targets() {
        let key = PendingTarget::of(InputAttribution::FocusedFrame, Some(DocumentId(1)));
        assert_eq!(key, Some(PendingTarget::FocusedFrame));
        let child = PendingTarget::of(
            InputAttribution::TargetedFrame(Some(ElementId(9))),
            Some(DocumentId(1)),
        );
        assert_eq!(child, Some(PendingTarget::Document(ElementId(9))));
    }
}
