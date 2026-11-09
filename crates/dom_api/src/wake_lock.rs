//! `WakeLock` and `WakeLockSentinel` (https://w3c.github.io/screen-wake-lock/,
//! Blink's `modules/wake_lock`): a navigator's `wakeLock`. machopo asks the
//! operating system for nothing: a lock is granted (as Chrome's desktop
//! grants the permission) and released as Chrome's is, on `release()` or
//! when the page is hidden, but the screen may still dim. The permission's
//! answer comes back a task later than the request (Chrome's round trip to
//! the browser), as geolocation's does; both are tasks of the window's (its
//! element task queue's).

use dom::{EventTargetImpl, Navigator, ObjectRef, WakeLock, Window};
use elise::{Gc, Persistent};

use crate::typed::{DomStr, ScriptHost};
use crate::{ApiCx, ApiError, DomException, ExceptionName};

/// Blink's message for a hidden page's request.
const NOT_VISIBLE: &str = "The requesting page is not visible";

fn not_allowed(message: &str) -> ApiError {
    ApiError::Dom(DomException {
        name: ExceptionName::NotAllowed,
        message: message.to_owned(),
    })
}

/// `navigator.wakeLock`, the same each time.
pub(crate) fn navigator_wake_lock<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<Gc<'gc, WakeLock>, ApiError> {
    Ok(cx.navigator_wake_lock(this))
}

/// `request(type)` (Blink's `WakeLock::request`): only `screen` (Chrome's
/// `system` is behind a flag, a `TypeError` without it, as a value no enum
/// has); rejected at once in a document no browsing context shows, or a
/// hidden page; else granted, a task after the permission's request. (machopo
/// has no permissions policy: Chrome's `screen-wake-lock` one rejects a
/// cross-origin iframe's without `allow`.)
pub(crate) fn request<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WakeLock>,
    lock_type: Option<&str>,
) -> Result<u64, ApiError> {
    if let Some(other) = lock_type.filter(|&lock_type| lock_type != "screen") {
        return Err(ApiError::type_error(format!(
            "Failed to execute 'request' on 'WakeLock': The provided value '{other}' is not a valid enum value of type WakeLockType."
        )));
    }
    let window = cx.wake_lock_window(this);
    if !crate::history::is_fully_active(cx, cx.window_document(window)) {
        return Err(not_allowed(
            "The document has no associated browsing context",
        ));
    }
    if cx.page_hidden(window) {
        return Err(not_allowed(NOT_VISIBLE));
    }
    let promise = cx.new_promise_number();
    cx.queue_wake_lock_task(this, promise, false);
    Ok(promise)
}

/// The permission of `window`'s wake lock for the request whose promise
/// `promise` names: on its way (the way there queues the answer), or
/// `answered`, which grants it (Blink's `DidReceivePermissionResponse`):
/// the lock, unless the page was hidden meanwhile. Where a real
/// permission's answer would come in.
pub(crate) fn run_task<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    promise: u64,
    answered: bool,
) {
    let granted = host.mutate(|cx| {
        let window = window.get(cx.gc);
        let wake_lock = cx.window_wake_lock(window)?;
        if !answered {
            cx.queue_wake_lock_task(wake_lock, promise, true);
            return None;
        }
        Some(
            (!cx.page_hidden(window))
                .then(|| cx.root_object(ObjectRef::EventTarget(cx.acquire_wake_lock(wake_lock)))),
        )
    });
    match granted {
        None => {}
        Some(Some(sentinel)) => host.resolve_promise_with(promise, &sentinel),
        Some(None) => host.settle_promises(&[promise], Some(not_allowed(NOT_VISIBLE))),
    }
}

/// `WakeLockSentinel.released`.
pub(crate) fn released<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<bool, ApiError> {
    Ok(cx.wake_lock_sentinel_released(this))
}

/// `WakeLockSentinel.type`: machopo has only `screen` locks.
pub(crate) fn sentinel_type<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, EventTargetImpl>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(dom::DomString::new("screen")))
}

/// `release()`: releases the lock if it is active, then a promise resolved
/// with undefined, a new one each time.
pub(crate) fn release<H: ScriptHost>(
    host: &mut H,
    this: Persistent<EventTargetImpl>,
) -> Result<(), ApiError> {
    release_lock(host, &this);
    Ok(())
}

/// The screen wake lock's "release a wake lock" (Blink's
/// `WakeLockSentinel::DoRelease`): `release` fires at it, there and then.
fn release_lock<H: ScriptHost>(host: &mut H, sentinel: &Persistent<EventTargetImpl>) {
    if !host.mutate(|cx| cx.release_wake_lock(sentinel.get(cx.gc))) {
        return;
    }
    crate::event_target::fire_event(
        host,
        &dom::TargetRoot::EventTarget(sentinel.clone()),
        dom::names::event::RELEASE,
        false,
        false,
    );
}

/// The page `window`'s document is in was hidden, before its
/// `visibilitychange`: the window's locks are released (Blink's
/// `WakeLock::PageVisibilityChanged`), in the order they were granted.
pub(crate) fn page_hidden<H: ScriptHost>(host: &mut H, window: &Persistent<Window>) {
    let sentinels = host.read(|cx| {
        let Some(wake_lock) = cx.window_wake_lock(window.get(cx.gc)) else {
            return Vec::new();
        };
        cx.wake_lock_sentinels(wake_lock)
            .into_iter()
            .map(|sentinel| Persistent::new(cx.gc, sentinel))
            .collect()
    });
    for sentinel in &sentinels {
        release_lock(host, sentinel);
    }
}
