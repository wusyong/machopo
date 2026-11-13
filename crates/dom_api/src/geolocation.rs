//! `Geolocation` (https://w3c.github.io/geolocation/, Blink's
//! `core/geolocation/geolocation.cc`): a navigator's `geolocation`. machopo has
//! no location provider and no permission prompt, so it answers every
//! request as Chrome does when the user denies the permission
//! (`PERMISSION_DENIED`, "User denied Geolocation"), the way Blink gets
//! there: the first request asks for the permission, whose answer comes
//! back a task later than the request's own (Chrome's round trip to the
//! browser, `answer_permission`), and fails every request still waiting,
//! the one-shot ones first; a `timeout` of 0 times out before it. The
//! steps are tasks of the window's (its element task queue's).

use dom::{GeoRequest, Geolocation, GeolocationPositionError, GeolocationStep, Navigator, Window};
use elise::{Gc, Persistent};

use crate::interfaces::PositionOptions;
use crate::typed::{Callback, OpFront, ScriptHost};
use crate::{ApiCx, ApiError};

/// `GeolocationPositionError`'s codes.
pub(crate) const PERMISSION_DENIED: u16 = 1;
pub(crate) const TIMEOUT: u16 = 3;

/// `navigator.geolocation`, the same each time.
pub(crate) fn navigator_geolocation<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Navigator>,
) -> Result<Gc<'gc, Geolocation>, ApiError> {
    Ok(cx.navigator_geolocation(this))
}

/// `getCurrentPosition(success, error, options)`.
pub(crate) fn get_current_position<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Geolocation>,
    success: Callback<O::Callback>,
    error: Option<Callback<O::Callback>>,
    options: &PositionOptions,
) -> Result<(), ApiError> {
    request(op, cx, this, success, error, options, false);
    Ok(())
}

/// `watchPosition(success, error, options)`: the watch's id, 0 in a
/// document that isn't fully active (Blink's, without a frame).
pub(crate) fn watch_position<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Geolocation>,
    success: Callback<O::Callback>,
    error: Option<Callback<O::Callback>>,
    options: &PositionOptions,
) -> Result<i32, ApiError> {
    Ok(request(op, cx, this, success, error, options, true))
}

/// `clearWatch(watchId)`.
pub(crate) fn clear_watch<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Geolocation>,
    watch_id: i32,
) -> Result<(), ApiError> {
    cx.geolocation_clear_watch(this, watch_id);
    Ok(())
}

/// A request (Blink's `getCurrentPosition` and `WatchPositionInternal`):
/// its watch id, 0 for a one-shot one.
fn request<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    geolocation: Gc<'gc, Geolocation>,
    success: Callback<O::Callback>,
    error: Option<Callback<O::Callback>>,
    options: &PositionOptions,
    watch: bool,
) -> i32 {
    let window = cx.geolocation_window(geolocation);
    if !crate::history::is_fully_active(cx, cx.window_document(window)) {
        return 0;
    }
    // Blink's `StartRequest`: one made while the page is hidden waits
    // until it shows.
    let visible = !cx.page_hidden(window);
    let timeout = clamp(options.timeout);
    let request = cx.add_geolocation_request(geolocation, watch, timeout, visible);
    let (success_edge, error_edge) = cx.geolocation_request_edges(request);
    op.store_callback(cx, success_edge, success);
    if let Some(error) = error {
        op.store_callback(cx, error_edge, error);
    }
    if visible {
        start(cx, geolocation, request);
    }
    cx.geolocation_request(request).1
}

/// A `[Clamp] unsigned long` of a number (WebIDL's): NaN 0, into the
/// range, the nearest, ties to even; an absent `timeout` is the largest.
fn clamp(value: Option<f64>) -> u32 {
    let Some(value) = value else {
        return u32::MAX;
    };
    if value.is_nan() {
        return 0;
    }
    let value = value.clamp(0.0, f64::from(u32::MAX)).round_ties_even();
    // In range and whole: clamped and rounded above.
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to u32's range and rounded"
    )]
    let value = value as u32;
    value
}

/// Starts `request` (Blink's `StartRequest`): its timer, then asks for the
/// permission if it isn't being asked for. The answer always comes before
/// a timer of 1 ms or more would fire, and stops it (a fatal error), so
/// only a `timeout` of 0 has a timer.
fn start<'gc>(
    cx: &ApiCx<'_, 'gc>,
    geolocation: Gc<'gc, Geolocation>,
    request: Gc<'gc, GeoRequest>,
) {
    let (id, _, timeout) = cx.geolocation_request(request);
    if timeout == 0 {
        cx.queue_geolocation_task(geolocation, GeolocationStep::Timer(id));
    }
    if cx.geolocation_ask_permission(geolocation) {
        cx.queue_geolocation_task(geolocation, GeolocationStep::Permission { answered: false });
    }
}

/// A task of `window`'s geolocation's.
pub(crate) fn run_task<H: ScriptHost>(
    host: &mut H,
    window: &Persistent<Window>,
    step: GeolocationStep,
) {
    let geolocation = host.read(|cx| {
        cx.window_geolocation(window.get(cx.gc))
            .map(|g| Persistent::new(cx.gc, g))
    });
    let Some(geolocation) = geolocation else {
        return;
    };
    match step {
        GeolocationStep::Timer(id) => timer_fired(host, &geolocation, id),
        GeolocationStep::Permission { answered: false } => {
            host.mutate(|cx| {
                cx.queue_geolocation_task(
                    geolocation.get(cx.gc),
                    GeolocationStep::Permission { answered: true },
                );
            });
        }
        GeolocationStep::Permission { answered: true } => answer_permission(host, &geolocation),
    }
}

/// Request `id`'s timer fired (Blink's `GeoNotifier::TimerFired`): a
/// `TIMEOUT`, if it still waits.
fn timer_fired<H: ScriptHost>(host: &mut H, geolocation: &Persistent<Geolocation>, id: u32) {
    let failed = host.mutate(|cx| {
        let request = cx.geolocation_timed_out(geolocation.get(cx.gc), id)?;
        let error = cx.create_geolocation_position_error(TIMEOUT);
        Some((
            Persistent::new(cx.gc, request),
            Persistent::new(cx.gc, error),
        ))
    });
    if let Some((request, error)) = failed {
        host.call_geolocation_error(&request, &error);
    }
}

/// The permission's answer, which is a denial (Blink's
/// `HandlePermissionError`): every request still waiting fails, with one
/// error. Where a real permission's answer would come in.
fn answer_permission<H: ScriptHost>(host: &mut H, geolocation: &Persistent<Geolocation>) {
    let (requests, error) = host.mutate(|cx| {
        let requests: Vec<_> = cx
            .geolocation_fail_all(geolocation.get(cx.gc))
            .into_iter()
            .map(|request| Persistent::new(cx.gc, request))
            .collect();
        let error = cx.create_geolocation_position_error(PERMISSION_DENIED);
        (requests, Persistent::new(cx.gc, error))
    });
    for request in &requests {
        host.call_geolocation_error(request, &error);
    }
}

/// The page `window`'s document is in was shown: its geolocation starts
/// the requests made while it was hidden (Blink's
/// `Geolocation::PageVisibilityChanged`).
pub(crate) fn page_shown<'gc>(cx: &ApiCx<'_, 'gc>, window: Gc<'gc, Window>) {
    let Some(geolocation) = cx.window_geolocation(window) else {
        return;
    };
    for request in cx.geolocation_start_waiting(geolocation) {
        start(cx, geolocation, request);
    }
}

/// `GeolocationPositionError.code`.
pub(crate) fn error_code<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, GeolocationPositionError>,
) -> Result<u16, ApiError> {
    Ok(cx.geolocation_position_error_code(this))
}

/// `GeolocationPositionError.message`: Blink's for its code.
pub(crate) fn error_message<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, GeolocationPositionError>,
) -> Result<crate::typed::DomStr<'gc>, ApiError> {
    let message = match cx.geolocation_position_error_code(this) {
        PERMISSION_DENIED => "User denied Geolocation",
        _ => "Timeout expired",
    };
    Ok(crate::typed::DomStr::fresh(message))
}

/// A `GeolocationPosition`'s or `GeolocationCoordinates`' member: there is
/// no such object (machopo never has a position), so the front end's check of
/// `this` stops every call before it gets here.
pub(crate) fn no_position<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, GeolocationPositionError>,
) -> Result<f64, ApiError> {
    unreachable!("machopo makes no GeolocationPosition")
}

pub(crate) fn no_optional<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, GeolocationPositionError>,
) -> Result<Option<f64>, ApiError> {
    unreachable!("machopo makes no GeolocationCoordinates")
}

pub(crate) fn no_coords<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, GeolocationPositionError>,
) -> Result<Gc<'gc, GeolocationPositionError>, ApiError> {
    unreachable!("machopo makes no GeolocationPosition")
}

pub(crate) fn no_json<'gc>(
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, GeolocationPositionError>,
) -> Result<&'gc elise::JsEdge, ApiError> {
    unreachable!("machopo makes no GeolocationPosition")
}
