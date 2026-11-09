//! `blink::InputHandlerProxy`: on the compositor thread, what each input
//! event from the browser is for. The compositor scrolls the viewport and
//! the containers that are layers itself (cc's `ThreadedInputHandler`, here
//! `LayerTreeHostImpl`), so a wheel scrolls even while the main thread is
//! busy; what needs the main thread goes there, keys included (their
//! `keydown` may cancel the scroll, as in Chrome).

use gfx::PointF;
use protocol::input::InputEvent;

use crate::LayerTreeHostImpl;
use crate::property_trees::ScrollHitTest;
use crate::scroll_utils::wheel_scroll_delta;

/// `InputHandlerProxy::EventDisposition`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EventDisposition {
    /// Scrolled here: acknowledged with the next frame drawn.
    DidHandle,
    /// Nothing to do: acknowledged at once.
    DropEvent,
    /// For the main thread; a wheel's `wheel` event `cancelable` there
    /// (in the wheel event handler region).
    DidNotHandle { cancelable: bool },
}

/// Handles `event` if the compositor can, scrolling.
pub(crate) fn handle_input_event(
    host_impl: &mut LayerTreeHostImpl,
    event: &InputEvent,
) -> EventDisposition {
    let (from, delta) = match *event {
        // Hit testing, cursors and clicks are the main thread's; so are
        // keys, which scroll only once their `keydown` wasn't canceled.
        InputEvent::MouseMove { .. }
        | InputEvent::MouseDown { .. }
        | InputEvent::MouseUp { .. }
        | InputEvent::Key(_) => {
            return EventDisposition::DidNotHandle { cancelable: true };
        }
        // The innermost container that is a layer under the pointer, and
        // those around it, then the viewport, which only the drawn document
        // has. Where the region says the compositor can't tell, the main
        // thread picks the scroller and scrolls it; it also hit tests the
        // same point, so a document it no longer shows drops it.
        InputEvent::MouseWheel { x, y, delta, .. } => {
            if host_impl.scrollable_viewport().is_none() {
                return EventDisposition::DropEvent;
            }
            match host_impl.scroll_hit_test(PointF::new(x, y)) {
                ScrollHitTest::MainThread { blocking } => {
                    return EventDisposition::DidNotHandle {
                        cancelable: blocking,
                    };
                }
                ScrollHitTest::Scroll(from) => (from, wheel_scroll_delta(delta)),
            }
        }
    };
    // Nothing moving (at the end, `overflow: hidden`, too short to scroll)
    // is nothing for the main thread either: it has no wheel events yet.
    if host_impl.scroll_from(from, delta) {
        EventDisposition::DidHandle
    } else {
        EventDisposition::DropEvent
    }
}
