//! HTML's animation frames
//! (https://html.spec.whatwg.org/#animation-frames): `requestAnimationFrame`
//! and `cancelAnimationFrame` on a window, and "run the animation frame
//! callbacks", which the host runs each frame before it updates the
//! rendering (Blink's `ScriptedAnimationController`, which
//! `PageAnimator::ServiceScriptedAnimations` calls). The callbacks are
//! `dom`'s, on the window.

use dom::Window;
use elise::{Gc, Persistent};

use crate::typed::{Callback, OpFront, ScriptHost};
use crate::{ApiCx, ApiError};

/// `requestAnimationFrame(callback)`: a new callback of the window, its
/// handle.
pub(crate) fn request_animation_frame<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    callback: Callback<O::Callback>,
) -> Result<i32, ApiError> {
    let frame = cx.add_animation_frame(this);
    op.store_callback(cx, cx.animation_frame_callback_edge(frame), callback);
    Ok(cx.animation_frame_handle(frame))
}

/// `cancelAnimationFrame(handle)`: the window's callback of the handle
/// taken out, if it has it; one running now isn't stopped.
pub(crate) fn cancel_animation_frame<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    handle: i32,
) -> Result<(), ApiError> {
    cx.take_animation_frame(this, handle);
    Ok(())
}

/// HTML's "run the animation frame callbacks" of `window` at `now`
/// (milliseconds since its time origin; coarsened as `performance.now()`
/// is): the callbacks requested so far, in order, each taken out as it
/// runs, unless an earlier one canceled it; one they request waits for the
/// next frame. Each runs out of any operation and reports what it throws,
/// then a microtask checkpoint.
pub fn run_animation_frame_callbacks<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    now: f64,
) {
    let time = crate::event::coarsen(now);
    let handles = host.read(|cx| cx.animation_frame_handles(window.get(cx.gc)));
    for handle in handles {
        let Some(frame) = host.mutate(|cx| {
            cx.take_animation_frame(window.get(cx.gc), handle)
                .map(|frame| Persistent::new(cx.gc, frame))
        }) else {
            continue;
        };
        // Chrome's trace's span of a callback, its microtasks too.
        let _span = trace_event::trace_event!("FireAnimationFrame");
        host.call_animation_frame(window, &frame, time);
        host.perform_microtask_checkpoint();
    }
}
