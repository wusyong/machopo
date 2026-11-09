//! `AbortController` and `AbortSignal` (https://dom.spec.whatwg.org/#aborting-ongoing-activities,
//! Blink's `core/dom/abort_controller.cc`, `abort_signal.cc`), over
//! `dom`'s. DOM's "signal abort": the reason set (the engine's value:
//! script's, or a `DOMException` made then, in an operation the front end
//! keeps it for, or by the host for a timeout's), the signal and its
//! dependents aborted (`dom`'s `signal_abort`), then each one's abort
//! steps, out of the operation: its algorithms (listeners removed), then
//! `abort` fired at it.

use dom::{AbortController, EventTargetImpl, NativeAction, TargetRoot, Window};
use elise::{Gc, Persistent};

use crate::typed::{AnyArg, OpFront, Root, ScriptHost, ThenScript};
use crate::{ApiCx, ApiError, DomException, ExceptionName};

fn current_window<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    what: &str,
) -> Result<Gc<'gc, Window>, ApiError> {
    op.current_window(cx).ok_or_else(|| {
        ApiError::Dom(DomException {
            name: ExceptionName::InvalidState,
            message: format!("no realm to make the {what} in"),
        })
    })
}

/// `new AbortController()`: with a new signal, of the current realm.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
) -> Result<Gc<'gc, AbortController>, ApiError> {
    let window = current_window(op, cx, "AbortController")?;
    Ok(cx.create_abort_controller(window))
}

/// `AbortController.signal`.
pub(crate) fn signal<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, AbortController>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    Ok(cx.abort_controller_signal(this))
}

/// A signal aborted in an operation, whose abort steps are left to do out
/// of it.
pub(crate) struct AbortSteps<'gc>(Gc<'gc, EventTargetImpl>);

pub(crate) struct AbortStepsRoot(Persistent<EventTargetImpl>);

impl<'gc> Root<'gc> for AbortSteps<'gc> {
    type Rooted = AbortStepsRoot;

    fn root(self, cx: &ApiCx<'_, 'gc>) -> AbortStepsRoot {
        AbortStepsRoot(Persistent::new(cx.gc, self.0))
    }
}

impl ThenScript for AbortStepsRoot {
    fn run<H: ScriptHost>(self, host: &mut H) {
        signal_abort(host, &self.0);
    }
}

/// `AbortController.abort(reason)`: DOM's "signal abort", with `reason`
/// (a new `AbortError` if none was given: the front end made it). Nothing
/// if the signal aborted already.
pub(crate) fn abort<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, AbortController>,
    reason: AnyArg,
) -> Result<Option<AbortSteps<'gc>>, ApiError> {
    let signal = cx.abort_controller_signal(this);
    if cx.abort_signal_aborted(signal) {
        return Ok(None);
    }
    op.store_any(cx, cx.abort_reason_edge(signal), reason);
    Ok(Some(AbortSteps(signal)))
}

/// DOM's "signal abort" of `signal`, its reason set: it and its dependent
/// signals aborted, then each one's abort steps, it first: its abort
/// algorithms, then `abort` (trusted, neither bubbling nor cancelable).
pub(crate) fn signal_abort<H: ScriptHost>(host: &mut H, signal: &Persistent<EventTargetImpl>) {
    let aborted: Vec<Persistent<EventTargetImpl>> = host.mutate(|cx| {
        cx.signal_abort(signal.get(cx.gc))
            .into_iter()
            .map(|signal| Persistent::new(cx.gc, signal))
            .collect()
    });
    for signal in aborted {
        let fetches: Vec<Persistent<dom::FetchRecord>> = host.mutate(|cx| {
            cx.run_abort_algorithms(signal.get(cx.gc))
                .into_iter()
                .map(|fetch| Persistent::new(cx.gc, fetch))
                .collect()
        });
        for fetch in &fetches {
            crate::fetch::abort_fetch(host, fetch, &signal);
        }
        crate::event_target::fire_event(
            host,
            &TargetRoot::EventTarget(signal),
            dom::names::event::ABORT,
            false,
            false,
        );
    }
}

/// `AbortSignal.abort(reason)`: a new signal of the current realm, aborted
/// already (no event: it has no listeners yet).
pub(crate) fn abort_signal<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    reason: AnyArg,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let window = current_window(op, cx, "AbortSignal")?;
    let signal = cx.create_abort_signal(window);
    op.store_any(cx, cx.abort_reason_edge(signal), reason);
    cx.signal_abort(signal);
    Ok(signal)
}

/// `AbortSignal.timeout(milliseconds)`: a new signal of the current realm,
/// which a native timer of its window aborts with a `TimeoutError` then
/// (`timeout_fired`).
pub(crate) fn timeout<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    milliseconds: f64,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let window = current_window(op, cx, "AbortSignal")?;
    let signal = cx.create_abort_signal(window);
    cx.add_target_timer(window, signal, NativeAction::AbortTimeout, milliseconds);
    Ok(signal)
}

/// `AbortSignal.timeout()`'s timer is due: its signal aborts with a new
/// `TimeoutError` of its realm. Nothing else aborts it before (it has no
/// controller; `any()` aborts no source).
pub(crate) fn timeout_fired<H: ScriptHost>(host: &mut H, signal: &Persistent<EventTargetImpl>) {
    host.set_abort_reason(signal, ExceptionName::Timeout, "signal timed out");
    signal_abort(host, signal);
}

/// `AbortSignal.any(signals)`: DOM's "create a dependent abort signal", of
/// the current realm.
pub(crate) fn any<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    signals: Vec<Gc<'gc, EventTargetImpl>>,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let window = current_window(op, cx, "AbortSignal")?;
    Ok(cx.create_dependent_abort_signal(window, &signals))
}

/// `AbortSignal.aborted`.
pub(crate) fn aborted<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<bool, ApiError> {
    Ok(cx.abort_signal_aborted(this))
}

/// `AbortSignal.reason`: undefined until it aborts.
pub(crate) fn reason<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<Option<&'gc elise::JsEdge>, ApiError> {
    Ok(cx.abort_reason(this))
}

/// `AbortSignal.throwIfAborted()`: throws its reason, once aborted.
pub(crate) fn throw_if_aborted<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    match cx.abort_reason(this) {
        None => Ok(()),
        Some(reason) => Err(op.rethrow(cx, reason)),
    }
}
