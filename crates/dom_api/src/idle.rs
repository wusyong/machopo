//! Idle callbacks (https://w3c.github.io/requestidlecallback/, Blink's
//! `ScriptedIdleTaskController` and `core/scheduler/idle_deadline.cc`):
//! `requestIdleCallback` and `cancelIdleCallback` on a window, and what
//! runs them. When an idle period starts and ends is the host's (Blink's
//! scheduler's `IdleHelper`): it starts one in each window
//! (`Cx::start_idle_period`) and runs the callbacks it may run one at a
//! time (`run_idle_callback`), each a task of its own; a callback whose
//! timeout is due runs as a timer does (`timer::run_next_due`), without
//! one. The callbacks are `dom`'s, on the window.

use dom::{IdleDeadline, Window};
use elise::{Gc, Persistent};

use crate::interfaces::IdleRequestOptions;
use crate::typed::{CallbackArg, OpFront, ScriptHost};
use crate::{ApiCx, ApiError};

/// `requestIdleCallback(callback, options)`: a new callback of the window,
/// run in the next idle period that starts, or once its `timeout` (if more
/// than 0) has passed; its handle.
pub(crate) fn request_idle_callback<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    callback: CallbackArg,
    options: &IdleRequestOptions,
) -> Result<u32, ApiError> {
    let idle = cx.add_idle_callback(this, options.timeout);
    op.store_callback(cx, cx.idle_callback_edge(idle), callback);
    Ok(cx.idle_callback_handle(idle))
}

/// `cancelIdleCallback(handle)`: the window's callback of the handle taken
/// out, if it has it.
pub(crate) fn cancel_idle_callback<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    handle: u32,
) -> Result<(), ApiError> {
    cx.cancel_idle_callback(this, handle);
    Ok(())
}

/// `timeRemaining()`: down to the deadline on the window's clock, on the
/// 100 µs grid `performance.now()` is coarsened to, never more than what is
/// left (the deadline of a period while frames are drawn, the next frame's,
/// isn't on the grid: rounding could give past it, 16.7 for 16.67); none
/// once the timeout ran the callback (its deadline the time it ran).
pub(crate) fn time_remaining<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IdleDeadline>,
) -> Result<f64, ApiError> {
    let (window, deadline, _) = cx.idle_deadline(this);
    Ok(on_grid(deadline - cx.now_since(Some(window))))
}

/// `remaining` milliseconds on the 100 µs grid, never more (none below 0).
fn on_grid(remaining: f64) -> f64 {
    const STEPS_PER_MS: f64 = 10.0;
    if remaining > 0.0 {
        (remaining * STEPS_PER_MS).floor() / STEPS_PER_MS
    } else {
        0.0
    }
}

/// `didTimeout`.
pub(crate) fn did_timeout<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, IdleDeadline>,
) -> Result<bool, ApiError> {
    Ok(cx.idle_deadline(this).2)
}

/// HTML's "invoke idle callbacks" for `window`, one of them: the first it
/// may run in the idle period running, which ends at `deadline`
/// (milliseconds since its time origin), called with its `IdleDeadline`,
/// what it throws reported, then a microtask checkpoint. Whether one ran.
pub fn run_idle_callback<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    deadline: f64,
) -> bool {
    let taken = host.mutate(|cx| {
        let window = window.get(cx.gc);
        let callback = cx.take_runnable_idle_callback(window)?;
        let deadline = cx.create_idle_deadline(window, crate::event::coarsen(deadline), false);
        Some((
            Persistent::new(cx.gc, callback),
            Persistent::new(cx.gc, deadline),
        ))
    });
    let Some((callback, deadline)) = taken else {
        return false;
    };
    host.call_idle_callback(&callback, &deadline);
    host.perform_microtask_checkpoint();
    true
}

/// HTML's "invoke idle callback timeout algorithm" for `window`: the
/// callback whose timeout is due by `now` first, if any, called without an
/// idle period (`didTimeout` true).
pub(crate) fn run_due_timeout<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    now: f64,
) -> bool {
    let taken = host.mutate(|cx| {
        let window = window.get(cx.gc);
        let callback = cx.take_due_idle_timeout(window, now)?;
        let deadline = cx.create_idle_deadline(window, crate::event::coarsen(now), true);
        Some((
            Persistent::new(cx.gc, callback),
            Persistent::new(cx.gc, deadline),
        ))
    });
    let Some((callback, deadline)) = taken else {
        return false;
    };
    host.call_idle_callback(&callback, &deadline);
    host.perform_microtask_checkpoint();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_time_remaining_is_never_more_than_left() {
        assert_eq!(on_grid(1000.0 / 60.0), 16.6);
        assert_eq!(on_grid(16.69), 16.6);
        assert_eq!(on_grid(50.0), 50.0);
        assert_eq!(on_grid(0.05), 0.0);
        assert_eq!(on_grid(-1.0), 0.0);
    }
}
