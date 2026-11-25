//! A window's timers and a worker's global's
//! (https://html.spec.whatwg.org/#timers, its execution context's), and a
//! window's reporting of exceptions
//! (https://html.spec.whatwg.org/#report-an-exception). A timer is the
//! DOM's (`dom::Timer`, on its window), its callback and arguments
//! the engine's; the host runs each as its token comes (`run_task`) out
//! of any operation, calling each through `ScriptHost::call_timer`
//! (Blink's `DOMTimer::Fired`). What a listener or a timer throws is
//! reported at its window (`report_exception`): an `ErrorEvent`, logged by
//! the engine if no listener cancels it.

use dom::{Dom, ErrorInfo, ExecutionContext, GlobalRef, GlobalRoot, Window, WorkerGlobalScope};
use elise::{Gc, Persistent};

use crate::typed::{Callback, OpFront, ScriptHost};
use crate::{ApiCx, ApiError, dispatch};

/// `setTimeout`: a timer of the window, its id.
pub(crate) fn set_timeout<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    handler: Callback<O::Callback>,
    timeout: i32,
    arguments: Option<O::Rest>,
) -> Result<i32, ApiError> {
    Ok(add(
        op,
        cx,
        cx.window_context(this),
        (handler, arguments),
        timeout,
        false,
    ))
}

/// `setInterval`: a timer of the window that repeats, its id.
pub(crate) fn set_interval<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    handler: Callback<O::Callback>,
    timeout: i32,
    arguments: Option<O::Rest>,
) -> Result<i32, ApiError> {
    Ok(add(
        op,
        cx,
        cx.window_context(this),
        (handler, arguments),
        timeout,
        true,
    ))
}

/// A worker's global's `setTimeout`: a timer of its execution context.
pub(crate) fn worker_set_timeout<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
    handler: Callback<O::Callback>,
    timeout: i32,
    arguments: Option<O::Rest>,
) -> Result<i32, ApiError> {
    let context = cx.worker_global_context(this);
    Ok(add(op, cx, context, (handler, arguments), timeout, false))
}

/// A worker's global's `setInterval`.
pub(crate) fn worker_set_interval<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
    handler: Callback<O::Callback>,
    timeout: i32,
    arguments: Option<O::Rest>,
) -> Result<i32, ApiError> {
    let context = cx.worker_global_context(this);
    Ok(add(op, cx, context, (handler, arguments), timeout, true))
}

fn add<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    context: Gc<'gc, ExecutionContext>,
    (handler, arguments): (Callback<O::Callback>, Option<O::Rest>),
    timeout: i32,
    repeat: bool,
) -> i32 {
    let timer = cx.add_timer(context, timeout.into(), repeat);
    op.store_callback(cx, cx.timer_callback_edge(timer), handler);
    if let Some(arguments) = arguments {
        op.store_rest(cx, cx.timer_arguments_edge(timer), arguments);
    }
    cx.timer_state(timer).id
}

/// `clearTimeout` and `clearInterval` (either clears either): the
/// window's timer of the id removed, if it has one.
pub(crate) fn clear_timer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, Window>,
    id: i32,
) -> Result<(), ApiError> {
    cx.remove_timer(cx.window_context(this), id);
    Ok(())
}

/// A worker's global's `clearTimeout` and `clearInterval`.
pub(crate) fn worker_clear_timer<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
    id: i32,
) -> Result<(), ApiError> {
    cx.remove_timer(cx.worker_global_context(this), id);
    Ok(())
}

/// Runs `global`'s task numbered `number` (a token of the host's
/// scheduler stands for it, `Cx::task_numbered`), of the queue `queue`
/// names, due as the scheduler took it, `now` milliseconds since its time
/// origin (an interval due again an interval after it), with its microtask
/// checkpoint. Whether it ran (not if it is no more).
///
/// # Panics
///
/// If `now` isn't finite.
pub fn run_task<H: ScriptHost>(
    host: &mut H,
    global: &GlobalRoot,
    queue: dom::TaskQueue,
    now: f64,
    number: u64,
) -> bool {
    assert!(now.is_finite(), "tasks run by a finite time");
    match queue {
        dom::TaskQueue::Timer => run_script_timer(host, global, now, number),
        dom::TaskQueue::NativeTimer => run_native_timer(host, global, now, number),
        // A window's alone (`Cx::task_numbered` finds none of another's).
        dom::TaskQueue::IdleTimeout => match global {
            GlobalRoot::Window(window) => crate::idle::run_due_timeout(host, window, now, number),
            GlobalRoot::WorkerGlobal(_) => false,
        },
        dom::TaskQueue::Element => crate::text_control::run_element_task(host, global, number),
        dom::TaskQueue::Message => crate::message::run_posted_message(host, global, number),
    }
}

/// `global`'s UA timer numbered `number`, run as a task (with its
/// microtask checkpoint). Whether it ran.
fn run_native_timer<H: ScriptHost>(
    host: &mut H,
    global: &GlobalRoot,
    now: f64,
    number: u64,
) -> bool {
    let native = host.mutate(|cx| {
        let context = cx.global_context(global.get(cx.gc));
        let dom::DueNativeTimer {
            element,
            action,
            data,
            target,
        } = cx.take_native_timer_numbered(context, number)?;
        Some((
            element.map(|element| cx.root(element)),
            action,
            data,
            target.map(|target| Persistent::new(cx.gc, target)),
        ))
    });
    if let Some((element, action, data, target)) = native {
        match (action, target, element) {
            (dom::NativeAction::AbortTimeout, Some(signal), _) => {
                crate::abort::timeout_fired(host, &signal);
            }
            (_, _, Some(element)) => {
                crate::controls::run_native_timer(host, &element, action, data, now);
            }
            // An element's, of a worker: none is.
            (_, _, None) => {}
        }
        host.perform_microtask_checkpoint();
        return true;
    }
    false
}

/// `global`'s script timer numbered `number`, run (HTML's timer task: its
/// nesting level the global's while its callback runs, back to 0 before
/// the host's microtask checkpoint, `ScriptHost::call_timer`); an interval
/// due again from `now`. Whether it ran.
fn run_script_timer<H: ScriptHost>(
    host: &mut H,
    global: &GlobalRoot,
    now: f64,
    number: u64,
) -> bool {
    let timer = host.mutate(|cx| {
        let context = cx.global_context(global.get(cx.gc));
        let (timer, nesting) = cx.take_timer_numbered(context, number, now)?;
        cx.set_timer_nesting(context, nesting);
        Some(Persistent::new(cx.gc, timer))
    });
    let Some(timer) = timer else {
        return false;
    };
    // Chrome's trace's span of a timer's task, its microtasks too.
    let _span = trace_event::trace_event!("TimerFire");
    host.call_timer(global, &timer);
    // Its nesting level goes before its microtasks run: a timer one of
    // them sets isn't nested (HTML's timer task, Blink's `DOMTimer::Fired`
    // resetting it as the callback returns).
    host.read(|cx| cx.set_timer_nesting(cx.global_context(global.get(cx.gc)), 0));
    host.perform_microtask_checkpoint();
    true
}

/// How long until `global` next has a timer due, by its clock (now with an
/// element task or a message queued); none with no timer.
pub fn until_next_due<'gc>(
    cx: &dom::Cx<'_, 'gc>,
    global: GlobalRef<'gc>,
) -> Option<std::time::Duration> {
    let context = cx.global_context(global);
    // An element task or a message queued is due now.
    let wait = if cx.has_element_task(context) || cx.has_posted_message(context) {
        0.0
    } else {
        cx.next_due(context)? - cx.now_in(Some(context))
    };
    Some(std::time::Duration::from_secs_f64(wait.max(0.0) / 1000.0))
}

/// When `global`'s next timer is due (milliseconds since its time
/// origin), for the host's clock; none with no timer.
pub fn next_due(dom: &Dom, global: &GlobalRoot) -> Option<f64> {
    dom.read(|cx| cx.next_due(cx.global_context(global.get(cx.gc))))
}

/// What became of an exception reported (`report_exception`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reported {
    /// A listener of the window's `error` event canceled it: not logged.
    Canceled,
    /// No listener canceled it: the engine logs it.
    NotCanceled,
    /// Thrown while the window reported another (by a listener of its
    /// `error` event): not reported again, but logged.
    WhileReporting,
}

/// HTML's "report an exception" at `global`: an `ErrorEvent` of `info`
/// fired at it, cancelable, its error `error`, the thrown value (the
/// host's, stored in the event's edge; none for an error the user agent
/// reports, Resize Observer's loop error); one thrown while the global
/// reports another isn't (HTML's "in error reporting mode").
pub fn report_exception<H: ScriptHost>(
    host: &mut H,
    global: &GlobalRoot,
    info: &ErrorInfo,
    error: Option<H::Value>,
) -> Reported {
    let mut error = error;
    let event = host.mutate_storing(|cx, store| {
        let context = cx.global_context(global.get(cx.gc));
        if cx.reporting(context) {
            return None;
        }
        cx.set_reporting(context, true);
        let event = cx.create_error_event(info, Some(context));
        let data = cx.error_event_data(event).expect("an error event's data");
        if let Some(error) = error.take() {
            store(cx.error_edge(data), error);
        }
        Some(Persistent::new(cx.gc, event))
    });
    let Some(event) = event else {
        return Reported::WhileReporting;
    };
    let target = host.read(|cx| cx.root_target(global.get(cx.gc).target()));
    let not_canceled = dispatch(host, &target, &event);
    host.read(|cx| cx.set_reporting(cx.global_context(global.get(cx.gc)), false));
    if not_canceled {
        Reported::NotCanceled
    } else {
        Reported::Canceled
    }
}
