//! Dedicated workers' members: `Worker` in the realm that makes one (HTML's
//! "new Worker", Blink's `DedicatedWorker`), and a worker's global's own
//! (`DedicatedWorkerGlobalScope`'s `name`, `postMessage()`, `close()`).
//!
//! The worker runs elsewhere (a thread of its own, its own DOM and engine):
//! the members ask the engine's host at once (`WorkerHost` on the side that
//! made it, `WorkerGlobalHost` in the worker), as Blink's `DedicatedWorker`
//! posts a task to the worker's thread and calls `WorkerThread::Terminate`
//! there and then; what comes back is the host's to deliver.

use dom::{EventTargetImpl, SerializedMessage, WorkerGlobalScope};
use elise::Gc;
use url::Url;

use crate::css_style_sheet::dom_error;
use crate::interfaces::WorkerOptions;
use crate::message::MessageParam;
use crate::typed::{DomStr, OpFront};
use crate::{ApiCx, ApiError, ExceptionName};

/// A dedicated worker to start (HTML's "run a worker"'s inputs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerStart {
    /// Its script's URL, parsed against the API base URL of the realm that
    /// made it.
    pub url: Url,
    /// Its name (`WorkerOptions`' `name`).
    pub name: String,
    /// The origin of the realm that made it: what its script is fetched
    /// for (same-origin), and its own but for a `data:` URL's.
    pub owner_origin: Option<url::Origin>,
}

/// What starts the dedicated workers a realm makes and carries what goes to
/// them (Blink's `DedicatedWorkerMessagingProxy`): the engine's host's.
pub trait WorkerHost {
    /// A new dedicated worker of `start`: the number it is known by from
    /// now on. Messages posted to it before it runs wait for it.
    fn start_worker(&mut self, start: WorkerStart) -> u64;

    /// `message` posted to the worker `id` (`Worker.postMessage`).
    fn post_to_worker(&mut self, id: u64, message: SerializedMessage);

    /// The worker `id` terminated (`Worker.terminate()`): its script
    /// stopped at once, nothing more of it delivered.
    fn terminate_worker(&mut self, id: u64);
}

/// What carries what a dedicated worker's global sends to the realm that
/// made it (Blink's `DedicatedWorkerObjectProxy`): the worker's engine's
/// host's.
pub trait WorkerGlobalHost {
    /// `message` posted to the worker's `Worker`
    /// (`DedicatedWorkerGlobalScope.postMessage`).
    fn post_to_owner(&mut self, message: SerializedMessage);

    /// The worker closed itself (`close()`): it runs no more tasks.
    fn close(&mut self);
}

/// `new Worker(scriptURL, options)`: the URL parsed against the current
/// realm's API base URL (a `SyntaxError` if it doesn't parse), a new
/// `Worker` of the realm, its worker started by the host (a
/// `NotSupportedError` if the engine was given none). `type: "module"` is
/// a `NotSupportedError`: only classic workers run; a `type` of neither is
/// WebIDL's `TypeError` of an enum's value.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    script_url: &str,
    options: &WorkerOptions,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let context = op
        .current_context(cx)
        .ok_or_else(|| dom_error(ExceptionName::NotSupported, "Workers can't be made here."))?;
    match options.r#type.as_str() {
        "classic" => {}
        "module" => {
            return Err(dom_error(
                ExceptionName::NotSupported,
                "Module workers aren't supported.",
            ));
        }
        other => {
            return Err(ApiError::Type(format!(
                "Failed to construct 'Worker': The provided value '{other}' is not a valid enum value of type WorkerType."
            )));
        }
    }
    let base = cx.context_base_url(context);
    let url = Url::options()
        .base_url(base.as_ref())
        .parse(script_url)
        .map_err(|_| {
            dom_error(
                ExceptionName::Syntax,
                format!("Failed to construct 'Worker': The URL '{script_url}' is invalid."),
            )
        })?;
    let start = WorkerStart {
        url,
        name: options.name.clone(),
        owner_origin: cx.context_origin(context),
    };
    let host = op
        .worker_host()
        .ok_or_else(|| dom_error(ExceptionName::NotSupported, "Workers can't be made here."))?;
    let id = host.start_worker(start);
    Ok(cx.create_worker_object(context, id))
}

/// `Worker.postMessage(message, transfer)`: the message serialized (by the
/// front end, in the current realm) given to the host for the worker;
/// nothing once it was terminated. Message ports can't go to a worker yet
/// (they would be another thread's): a `DataCloneError`.
pub(crate) fn post_message<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    message: MessageParam<'_, 'gc>,
) -> Result<(), ApiError> {
    if !message.ports.is_empty() {
        return Err(dom_error(
            ExceptionName::DataClone,
            "A MessagePort can't be transferred to a worker.",
        ));
    }
    if cx.worker_terminated(this) {
        return Ok(());
    }
    let Some(payload) = message.payload.take() else {
        return Ok(());
    };
    if let Some(host) = op.worker_host() {
        host.post_to_worker(cx.worker_id(this), payload);
    }
    Ok(())
}

/// `Worker.terminate()`: its worker terminated by the host, once.
pub(crate) fn terminate<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
) -> Result<(), ApiError> {
    if cx.worker_terminated(this) {
        return Ok(());
    }
    cx.terminate_worker_object(this);
    if let Some(host) = op.worker_host() {
        host.terminate_worker(cx.worker_id(this));
    }
    Ok(())
}

/// `self` of a worker's global: itself.
pub(crate) fn global_self<'gc>(
    _: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
) -> Result<Gc<'gc, WorkerGlobalScope>, ApiError> {
    Ok(this)
}

/// `DedicatedWorkerGlobalScope.name`.
pub(crate) fn name<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
) -> Result<DomStr<'gc>, ApiError> {
    Ok(DomStr::Owned(cx.worker_global_name(this).clone()))
}

/// `DedicatedWorkerGlobalScope.postMessage(message, transfer)`: the
/// message given to the host for the worker's `Worker`.
pub(crate) fn global_post_message<'gc, O: OpFront>(
    op: &mut O,
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, WorkerGlobalScope>,
    message: MessageParam<'_, 'gc>,
) -> Result<(), ApiError> {
    if !message.ports.is_empty() {
        return Err(dom_error(
            ExceptionName::DataClone,
            "A MessagePort can't be transferred out of a worker.",
        ));
    }
    let Some(payload) = message.payload.take() else {
        return Ok(());
    };
    if let Some(host) = op.worker_global_host() {
        host.post_to_owner(payload);
    }
    Ok(())
}

/// `DedicatedWorkerGlobalScope.close()`: the worker runs no more tasks
/// (HTML's closing flag, which its host keeps).
pub(crate) fn close<'gc, O: OpFront>(
    op: &mut O,
    _: &ApiCx<'_, 'gc>,
    _: Gc<'gc, WorkerGlobalScope>,
) -> Result<(), ApiError> {
    if let Some(host) = op.worker_global_host() {
        host.close();
    }
    Ok(())
}
