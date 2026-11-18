//! Dedicated workers' members: `Worker` in the realm that makes one (HTML's
//! "new Worker", Blink's `DedicatedWorker`), and a worker's global's own
//! (`DedicatedWorkerGlobalScope`'s `name`, `postMessage()`, `close()`;
//! `WorkerGlobalScope`'s `importScripts()`).
//!
//! The worker runs elsewhere (a thread of its own, its own DOM and engine):
//! the members ask the engine's host at once (`WorkerHost` on the side that
//! made it, `WorkerGlobalHost` in the worker), as Blink's `DedicatedWorker`
//! posts a task to the worker's thread and calls `WorkerThread::Terminate`
//! there and then; what comes back is the host's to deliver.

use dom::{EventTargetImpl, GlobalRef, SerializedMessage, TargetRef, WorkerGlobalScope};
use elise::Persistent;

use elise::Gc;
use url::Url;

use crate::css_style_sheet::dom_error;
use crate::interfaces::WorkerOptions;
use crate::message::MessageParam;
use crate::typed::{DomStr, OpFront, ScriptHost};
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
    /// A module worker's (`type: "module"`): the credentials mode its
    /// script graph is fetched with (`WorkerOptions`' `credentials`, by
    /// CORS); none: a classic worker's.
    pub module: Option<dom::ScriptCors>,
}

/// What starts the dedicated workers a realm makes and carries what goes to
/// them (Blink's `DedicatedWorkerMessagingProxy`): the engine's host's,
/// called in a member's operation (its methods `&self`: what they do goes
/// to another thread).
pub trait WorkerHost {
    /// A new dedicated worker of `start`: the number it is known by from
    /// now on. Messages posted to it before it runs wait for it.
    fn start_worker(&self, start: WorkerStart) -> u64;

    /// `message` posted to the worker `id` (`Worker.postMessage`).
    fn post_to_worker(&self, id: u64, message: SerializedMessage);

    /// The worker `id` terminated (`Worker.terminate()`): its script
    /// stopped at once, nothing more of it delivered.
    fn terminate_worker(&self, id: u64);

    /// What wakes the realm's thread for its message ports' messages from
    /// workers' threads (`dom::Dom::set_port_waker`); none if no port of
    /// its goes to another thread.
    fn port_waker(&self) -> Option<dom::PortWaker> {
        None
    }
}

/// What carries what a dedicated worker's global sends to the realm that
/// made it (Blink's `DedicatedWorkerObjectProxy`): the worker's engine's
/// host's.
pub trait WorkerGlobalHost {
    /// `message` posted to the worker's `Worker`
    /// (`DedicatedWorkerGlobalScope.postMessage`).
    fn post_to_owner(&self, message: SerializedMessage);

    /// The worker closed itself (`close()`): it runs no more tasks.
    fn close(&self);

    /// What the worker's script threw, its global's `onerror` didn't
    /// cancel (HTML's "report an exception" of a worker's): for its
    /// `Worker`'s `error` event.
    fn report_error(&self, error: WorkerError);

    /// `importScripts()`'s script at `url`, fetched now (HTML's "fetch a
    /// classic worker-imported script": `no-cors`, credentials
    /// same-origin), the worker waiting for it (its other tasks after);
    /// none if it couldn't be had (a network error, a status that isn't
    /// ok, a MIME type no script is).
    fn fetch_script(&self, url: &Url) -> Option<ImportedScript>;

    /// A `fetch()`'s request, sent now (its document none: the worker's);
    /// its response comes back as a task of the worker's
    /// (`WorkerEngine::fetch_response`).
    fn start_fetch(&self, request: &dom::FetchRequest);

    /// The `fetch()` numbered `id` aborted: its request cancelled, if it is
    /// still on its way. No response comes.
    fn cancel_fetch(&self, id: u64);

    /// A module the worker's realm asks for: fetched by CORS with
    /// `credentials`, for its owner's origin if `outside` (one of its
    /// module worker's graph: HTML's outside settings), else its own (an
    /// `import()`'s), its request's number given; its response comes back
    /// as a task of the worker's (`WorkerEngine::module_fetched`).
    fn start_module_fetch(&self, url: &Url, credentials: dom::ScriptCors, outside: bool) -> u64;

    /// `url` stands for `data` (`URL.createObjectURL` in the worker), for
    /// its loads and its page's, until revoked or the worker ends.
    fn register_blob_url(&self, url: &str, data: &dom::BlobData);

    /// `url` revoked (`URL.revokeObjectURL` in the worker), if the worker
    /// registered it: no load after it gets it.
    fn revoke_blob_url(&self, url: &str);

    /// What wakes the worker's thread for its message ports' messages from
    /// other threads (`dom::Dom::set_port_waker`); none if no port of its
    /// goes to another thread.
    fn port_waker(&self) -> Option<dom::PortWaker> {
        None
    }
}

/// A script `importScripts()` fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedScript {
    /// Its text, decoded.
    pub source: String,
    /// Its URL after redirects.
    pub url: String,
    /// Whether its errors are muted (another origin's: tainted opaque).
    pub muted: bool,
}

/// An uncaught exception of a worker, as it goes to its `Worker` (an
/// `ErrorEvent`'s parts; `error` is null on that side).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerError {
    pub message: String,
    pub filename: String,
    pub lineno: u32,
    pub colno: u32,
}

/// What the worker of `worker` threw and its global didn't cancel (HTML's
/// "run a worker"'s error reporting): a task of `worker`'s realm, queued
/// after what the worker posted before it, fires it (`fire_worker_error`).
/// Nothing for one terminated.
pub fn worker_error<H: ScriptHost>(
    host: &mut H,
    worker: &Persistent<EventTargetImpl>,
    error: &WorkerError,
) {
    let info = dom::ErrorInfo {
        message: dom::DomString::new(&error.message),
        filename: dom::DomString::new(&error.filename),
        lineno: error.lineno,
        colno: error.colno,
    };
    host.mutate(|cx| cx.queue_worker_error(worker.get(cx.gc), info));
}

/// A worker's error's task: an `ErrorEvent` of `info` fired at `worker`,
/// cancelable, its `error` null; not canceled, reported at its realm's
/// global as if thrown there (no value). Nothing for one terminated since.
pub(crate) fn fire_worker_error<H: ScriptHost>(
    host: &mut H,
    worker: &Persistent<EventTargetImpl>,
    info: &dom::ErrorInfo,
) {
    let event = host.mutate(|cx| {
        let worker = worker.get(cx.gc);
        if cx.worker_terminated(worker) {
            return None;
        }
        let context = cx.target_context(TargetRef::EventTarget(worker));
        let event = cx.create_error_event(info, context);
        Some(Persistent::new(cx.gc, event))
    });
    let Some(event) = event else {
        return;
    };
    let target = host.read(|cx| cx.root_target(TargetRef::EventTarget(worker.get(cx.gc))));
    if !crate::dispatch(host, &target, &event) {
        return;
    }
    // A window's, or a worker's global for a worker it made.
    let global = host.read(|cx| {
        let target = TargetRef::EventTarget(worker.get(cx.gc));
        let global = match cx
            .target_context(target)
            .and_then(|context| cx.context_worker_global(context))
        {
            Some(global) => GlobalRef::WorkerGlobal(global),
            None => GlobalRef::Window(cx.target_window(target)?),
        };
        Some(cx.root_global(global))
    });
    let Some(global) = global else {
        return;
    };
    let reported = crate::report_exception(host, &global, info, None);
    // A worker's global, not canceled: on up to its own `Worker`.
    if reported != crate::Reported::Canceled && matches!(global, dom::GlobalRoot::WorkerGlobal(_)) {
        host.report_to_worker_owner(info);
    }
}

/// `new Worker(scriptURL, options)`: the URL parsed against the current
/// realm's API base URL (a `SyntaxError` if it doesn't parse), a new
/// `Worker` of the realm, its worker started by the host (a
/// `NotSupportedError` if the engine was given none), classic or a module
/// worker (`type: "module"`, its graph fetched with `credentials`); a
/// `type` or `credentials` of none of their values is WebIDL's `TypeError`
/// of an enum's value.
pub(crate) fn construct<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    script_url: &str,
    options: &WorkerOptions,
) -> Result<Gc<'gc, EventTargetImpl>, ApiError> {
    let context = op
        .current_context(cx)
        .ok_or_else(|| dom_error(ExceptionName::NotSupported, "Workers can't be made here."))?;
    // In the dictionary's order (WebIDL converts its members by name).
    let credentials = match options.credentials.as_str() {
        "same-origin" => dom::ScriptCors::Anonymous,
        "include" => dom::ScriptCors::UseCredentials,
        "omit" => dom::ScriptCors::Omit,
        other => {
            return Err(ApiError::Type(format!(
                "Failed to construct 'Worker': The provided value '{other}' is not a valid enum value of type RequestCredentials."
            )));
        }
    };
    let module = match options.r#type.as_str() {
        "classic" => None,
        "module" => Some(credentials),
        other => {
            return Err(ApiError::Type(format!(
                "Failed to construct 'Worker': The provided value '{other}' is not a valid enum value of type WorkerType."
            )));
        }
    };
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
        module,
    };
    let host = op
        .worker_host()
        .ok_or_else(|| dom_error(ExceptionName::NotSupported, "Workers can't be made here."))?;
    let id = host.start_worker(start);
    Ok(cx.create_worker_object(context, id))
}

/// `Worker.postMessage(message, transfer)`: the message serialized (by the
/// front end, in the current realm), the ports it transfers shipped with
/// it, given to the host for the worker; lost once it was terminated.
pub(crate) fn post_message<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, EventTargetImpl>,
    message: MessageParam<'_, 'gc>,
) -> Result<(), ApiError> {
    let Some(payload) = message.ship(cx) else {
        return Ok(());
    };
    if cx.worker_terminated(this) {
        return Ok(());
    }
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

/// `importScripts(...urls)`: a `TypeError` in a module worker (HTML's);
/// else each URL parsed against the worker's API base URL first (a
/// `SyntaxError` if one doesn't parse, nothing fetched), then
/// each fetched and run in turn (`ScriptHost::import_script`), what one
/// throws, or that it couldn't be had, ending it there.
pub(crate) fn import_scripts<H: ScriptHost>(
    host: &mut H,
    this: Persistent<WorkerGlobalScope>,
    urls: Vec<String>,
) -> Result<(), ApiError> {
    let (base, module) = host.read(|cx| {
        let global = this.get(cx.gc);
        let context = cx.global_context(GlobalRef::WorkerGlobal(global));
        (cx.context_base_url(context), cx.worker_global_is_module(global))
    });
    // HTML: a module worker's global imports no classic scripts.
    if module {
        return Err(ApiError::Type(
            "Failed to execute 'importScripts' on 'WorkerGlobalScope': Module scripts don't support importScripts()."
                .to_owned(),
        ));
    }
    let parsed = urls
        .iter()
        .map(|url| {
            Url::options().base_url(base.as_ref()).parse(url).map_err(|_| {
                dom_error(
                    ExceptionName::Syntax,
                    format!(
                        "Failed to execute 'importScripts' on 'WorkerGlobalScope': The URL '{url}' is invalid."
                    ),
                )
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    for url in &parsed {
        host.import_script(url).map_err(|error| match error {
            ImportError::Thrown => ApiError::Thrown,
            // Blink's message.
            ImportError::Failed => dom_error(
                ExceptionName::Network,
                format!(
                    "Failed to execute 'importScripts' on 'WorkerGlobalScope': The script at '{url}' failed to load."
                ),
            ),
        })?;
    }
    Ok(())
}

/// Why a script `importScripts()` asked for didn't run through
/// (`ScriptHost::import_script`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportError {
    /// It couldn't be had, or it threw and its errors are muted: a
    /// `NetworkError`.
    Failed,
    /// It threw, and the engine holds the exception still, for the member
    /// to throw on (`ApiError::Thrown`).
    Thrown,
}

/// `location`: its `WorkerLocation`, the same each time.
pub(crate) fn location<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
) -> Result<Gc<'gc, dom::Location>, ApiError> {
    Ok(cx.worker_location(this))
}

/// `navigator`: its `WorkerNavigator`, the same each time.
pub(crate) fn navigator<'gc>(
    cx: &ApiCx<'_, 'gc>,
    this: Gc<'gc, WorkerGlobalScope>,
) -> Result<Gc<'gc, dom::Navigator>, ApiError> {
    Ok(cx.worker_navigator(this))
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
/// message, the ports it transfers shipped with it, given to the host for
/// the worker's `Worker`.
pub(crate) fn global_post_message<'gc, O: OpFront>(
    op: &mut O,
    cx: &ApiCx<'_, 'gc>,
    _: Gc<'gc, WorkerGlobalScope>,
    message: MessageParam<'_, 'gc>,
) -> Result<(), ApiError> {
    let Some(payload) = message.ship(cx) else {
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
