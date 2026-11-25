//! A dedicated worker's engine (`dom_api::WorkerEngine`, Blink's
//! `WorkerOrWorkletScriptController`): an isolate of its own, on the
//! worker's thread (Blink's `WorkerBackingThread` makes one for each), with
//! its own DOM; its one realm's global a worker's global, made from
//! `DedicatedWorkerGlobalScope`'s template, the interfaces a dedicated
//! worker has on it. The templates, the members and their host are the
//! window's engine's (`state`, `templates`, `listeners`): only the realm and
//! what runs its tasks are the worker's.

use std::time::Duration;

use bindings::V8PerIsolateData;
use dom_api::{ClassicScript, ConsoleMessage, ConsoleSink, EngineSettings, WorkerEngine};
use elise::Persistent;

use crate::front::wrap_new_object;
use crate::listeners::{ReportAt, report_caught};
use crate::state::{self, State};
use crate::templates::{self, GlobalKind};

pub struct V8WorkerEngine {
    settings: EngineSettings,
    /// A context of no global, to make the realm in (as a window's engine's).
    bootstrap: v8::Global<v8::Context>,
    /// Its realm and its global, once made (`create_global`).
    context: Option<v8::Global<v8::Context>>,
    global: Option<Persistent<dom::WorkerGlobalScope>>,
    /// The isolate, and in its state the DOM's heap: last, as fields drop
    /// in order (the roots and `Global`s above go first).
    data: V8PerIsolateData,
}

impl V8WorkerEngine {
    pub fn new(settings: EngineSettings) -> Self {
        let mut data = V8PerIsolateData::new();
        state::install(data.isolate(), GlobalKind::DedicatedWorker);
        let bootstrap = {
            let isolate = data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Context::new(scope, Default::default());
            v8::Global::new(scope, context)
        };
        Self {
            settings,
            bootstrap,
            context: None,
            global: None,
            data,
        }
    }

    pub fn boxed(settings: EngineSettings) -> Box<dyn WorkerEngine> {
        Box::new(Self::new(settings))
    }

    fn state(&mut self) -> &mut State {
        bindings::isolate_state_mut::<State>(self.data.isolate()).expect("the engine's DOM")
    }

    /// `f` in its realm; none before it is made.
    fn in_realm<R>(&mut self, f: impl FnOnce(&mut v8::PinScope<'_, '_>) -> R) -> Option<R> {
        let context = self.context.clone()?;
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, context);
        Some(f(scope))
    }

    /// The `Worker` of its realm for the worker it made numbered `id`, if
    /// it is still there.
    fn worker_object(&mut self, id: u64) -> Option<Persistent<dom::EventTargetImpl>> {
        let global = self.global.clone()?;
        self.state().dom.read(|cx| {
            let worker = cx.context_worker(cx.worker_global_context(global.get(cx.gc)), id)?;
            Some(Persistent::new(cx.gc, worker))
        })
    }

    /// Its module worker's graph, if it was fetched since the last ask:
    /// run if it can be (HTML's "run a worker": a module script with no
    /// error to rethrow, its evaluation's rejection reported at the
    /// global; a microtask checkpoint after). Whether it ran.
    fn module_graph_done(&mut self) -> Option<bool> {
        let done = self.state().worker_realm.as_mut()?.graph.take()?;
        let (Some(root), None) = (done.root, done.parse_error) else {
            return Some(false);
        };
        self.in_realm(|scope| {
            if let Some(mut scope) = state::scope(scope) {
                let _entry = bindings::trace::enter_script();
                crate::modules::run_module_script(
                    &mut scope,
                    &crate::modules::Realm::Worker,
                    root.index(),
                );
                state::report(&mut scope);
            }
        });
        self.perform_microtask_checkpoint();
        Some(true)
    }

    /// A microtask checkpoint (HTML's "clean up after running script"),
    /// the mutation observers' delivery queued first.
    fn perform_microtask_checkpoint(&mut self) {
        self.in_realm(|scope| {
            if let Some(mut scope) = state::scope(scope) {
                state::report(&mut scope);
            }
            let _span = trace_event::trace_event!("RunMicrotasks");
            let _entry = bindings::trace::enter_script();
            scope.perform_microtask_checkpoint();
        });
    }
}

/// What the worker of `worker` (a `Worker` of the realm `scope` is in)
/// threw: an `error` event at it, reported at the realm's global if not
/// canceled (`dom_api::worker_error`). A page's and a worker's engine's
/// alike.
pub(crate) fn worker_error_in(
    scope: &mut v8::PinScope<'_, '_>,
    worker: &Persistent<dom::EventTargetImpl>,
    error: &dom_api::WorkerError,
) {
    if let Some(mut scope) = state::scope(scope) {
        dom_api::worker_error(
            &mut crate::listeners::Host { scope: &mut scope },
            worker,
            error,
        );
        state::report(&mut scope);
    }
}

/// The script of the worker of `worker` (a `Worker` of the realm `scope`
/// is in) couldn't be had: a plain `error` event at it, which gets nothing
/// more of it.
pub(crate) fn worker_script_failed_in(
    scope: &mut v8::PinScope<'_, '_>,
    worker: &Persistent<dom::EventTargetImpl>,
) {
    if let Some(mut scope) = state::scope(scope) {
        let target = dom::TargetRoot::EventTarget(worker.clone());
        dom_api::fire_event(
            &mut crate::listeners::Host { scope: &mut scope },
            &target,
            dom::names::event::ERROR,
            false,
            false,
        );
        let (state, _) = scope.state_mut();
        state
            .dom
            .mutate(|cx| cx.terminate_worker_object(worker.get(cx.gc)));
        state::report(&mut scope);
    }
}

/// A new realm of the isolate whose global is a new worker's global of
/// `url`, `origin` and `name` (HTML's "set up a worker environment settings
/// object"): its context made from `DedicatedWorkerGlobalScope`'s template
/// (its members and `WorkerGlobalScope`'s the global's own, WebIDL's
/// `[Global]`), the interfaces a dedicated worker has on it, and
/// `queueMicrotask`; `testRunner` too with web tests.
fn create_worker_realm<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    params: dom_api::WorkerGlobalParams,
    web_tests: bool,
) -> (
    v8::Local<'s, v8::Context>,
    Persistent<dom::WorkerGlobalScope>,
) {
    state::install(scope, GlobalKind::DedicatedWorker);
    let index = dom_api::INTERFACES
        .iter()
        .position(|interface| interface.name == "DedicatedWorkerGlobalScope")
        .expect("the table's DedicatedWorkerGlobalScope");
    let template = {
        let mut scope = state::scope(scope).expect("installed");
        templates::template(&mut scope, index)
    };
    let global_template = template.instance_template(scope);
    let context = v8::Context::new(
        scope,
        v8::ContextOptions {
            global_template: Some(global_template),
            ..Default::default()
        },
    );
    let scope = &mut v8::ContextScope::new(scope, context);
    let proxy = context.global(scope);
    let global = {
        let mut scope = state::scope(scope).expect("installed");
        let (state, mut no_gc) = scope.state_mut();
        state.dom.mutate(|cx| {
            let global = cx.create_worker_global(
                params.url,
                params.origin,
                dom::DomString::new(&params.name),
            );
            cx.set_context_time_origin(cx.worker_global_context(global), params.time_origin);
            if params.module {
                cx.set_worker_global_module(global);
            }
            wrap_new_object(&mut no_gc, cx, proxy, dom::ObjectRef::WorkerGlobal(global));
            Persistent::new(cx.gc, global)
        })
    };
    templates::install(scope, proxy, GlobalKind::DedicatedWorker);
    crate::window::install_queue_microtask(scope, proxy);
    if web_tests {
        crate::engine::install_test_runner(scope, proxy);
    }
    state::report(&mut state::scope(scope).expect("installed"));
    (context, global)
}

/// `importScripts()`'s script at `url` (`ScriptHost::import_script`):
/// fetched by the worker's host, run in the current realm, what it throws
/// thrown on as the member returns (V8 holds it till then); muted, it
/// fails (the member's `NetworkError`), what it threw dropped.
pub(crate) fn import_script(
    scope: &mut state::Scope<'_, '_, '_>,
    url: &url::Url,
) -> Result<(), dom_api::ImportError> {
    use dom_api::ImportError;
    let fetched = {
        let (state, _) = scope.state();
        state
            .caches
            .worker_global_host
            .as_deref()
            .and_then(|host| host.fetch_script(url))
    };
    let Some(script) = fetched else {
        return Err(ImportError::Failed);
    };
    crate::listeners::run_script(scope, |scope| {
        let scope = scope.scope();
        v8::tc_scope!(let tc, scope);
        let (Some(source), Some(name)) = (
            v8::String::new(tc, &script.source),
            v8::String::new(tc, &script.url),
        ) else {
            return Err(ImportError::Failed);
        };
        // Its base URL, which an `import()` in it takes: muted, `about:blank`
        // (HTML's "create a classic script").
        let base_url = if script.muted {
            "about:blank"
        } else {
            script.url.as_str()
        };
        let options = bindings::script_options(tc, base_url, Some("same-origin"));
        let origin = v8::ScriptOrigin::new(
            tc,
            name.into(),
            0,
            0,
            !script.muted,
            0,
            None,
            false,
            false,
            false,
            options,
        );
        let span = trace_event::trace_event!("EvaluateScript");
        let ran = v8::Script::compile(tc, source, Some(&origin)).and_then(|script| script.run(tc));
        drop(span);
        if ran.is_some() {
            return Ok(());
        }
        // Stopped: V8 throws the stop on as the `TryCatch` ends.
        if tc.has_terminated() {
            bindings::stop_again(tc);
            return Err(ImportError::Thrown);
        }
        if script.muted {
            return Err(ImportError::Failed);
        }
        // Thrown on as the `TryCatch` ends, for the member's caller.
        tc.rethrow();
        Err(ImportError::Thrown)
    })
}

impl WorkerEngine for V8WorkerEngine {
    fn create_global(
        &mut self,
        params: dom_api::WorkerGlobalParams,
        host: Box<dyn dom_api::WorkerGlobalHost>,
    ) {
        if let Some(waker) = host.port_waker() {
            self.state().dom.set_port_waker(waker);
        }
        self.state().caches.worker_global_host = Some(host);
        self.state()
            .dom
            .set_agent(&params.user_agent, params.webdriver);
        let web_tests = self.settings.web_tests;
        let (context, global) = {
            let isolate = self.data.isolate();
            v8::scope!(let scope, isolate);
            let bootstrap = v8::Local::new(scope, &self.bootstrap);
            let scope = &mut v8::ContextScope::new(scope, bootstrap);
            let (context, global) = create_worker_realm(scope, params, web_tests);
            (v8::Global::new(scope, context), global)
        };
        self.context = Some(context.clone());
        self.global = Some(global.clone());
        self.state().worker_realm = Some(crate::modules::WorkerRealm::new(context, global));
    }

    fn run_classic_script(&mut self, script: &ClassicScript<'_>) -> bool {
        let parsed = self.in_realm(|scope| {
            let global = scope.get_current_context().global(scope);
            v8::tc_scope!(let scope, scope);
            let (Some(source), Some(name)) = (
                v8::String::new(scope, script.source),
                v8::String::new(scope, script.url),
            ) else {
                return false;
            };
            let line_offset = i32::try_from(script.line.saturating_sub(1)).unwrap_or(i32::MAX);
            let options = bindings::script_options(
                scope,
                crate::frames::import_base_url(script),
                Some(crate::frames::credentials_of(script)),
            );
            let origin = v8::ScriptOrigin::new(
                scope,
                name.into(),
                line_offset,
                0,
                !script.muted,
                0,
                None,
                false,
                false,
                false,
                options,
            );
            // Script entered until what it threw is reported, as a frame's
            // classic script is.
            let _entry = bindings::trace::enter_script();
            let span = trace_event::trace_event!("EvaluateScript");
            // Not parsed: its error is dropped, not reported (HTML's "run a
            // worker", as Blink's `WorkerGlobalScope::EvaluateClassicScript`
            // tells its `Worker` it failed).
            let Some(compiled) = v8::Script::compile(scope, source, Some(&origin)) else {
                return false;
            };
            let ran = compiled.run(scope);
            drop(span);
            if ran.is_none() {
                report_caught(scope, ReportAt::Global(global));
            }
            true
        });
        self.perform_microtask_checkpoint();
        parsed.unwrap_or(false)
    }

    fn post_message(&mut self, message: dom::SerializedMessage) {
        let Some(global) = self.global.clone() else {
            return;
        };
        self.state()
            .dom
            .mutate(|cx| cx.post_worker_global_message(global.get(cx.gc), message));
    }

    fn fetch_response(&mut self, id: u64, response: Result<dom_api::FetchedResponse, ()>) {
        let Some(global) = self.global.clone() else {
            return;
        };
        let global = dom::GlobalRoot::WorkerGlobal(global);
        self.in_realm(|scope| {
            let Some(mut scope) = state::scope(scope) else {
                return;
            };
            let mut host = crate::listeners::Host { scope: &mut scope };
            dom_api::did_receive_response(&mut host, &global, id, response);
            state::report(&mut scope);
        });
        self.perform_microtask_checkpoint();
    }

    fn run_platform_tasks(&mut self) -> bool {
        // V8's own tasks may run script (a finalization registry's cleanup).
        let _entry = bindings::trace::enter_script();
        let ran = self.data.pump_platform_tasks();
        if ran {
            self.perform_microtask_checkpoint();
        }
        ran
    }

    fn set_platform_task_waker(
        &mut self,
        waker: std::sync::Arc<dyn Fn(Option<std::time::Instant>) + Send + Sync>,
    ) {
        self.data.set_platform_task_waker(waker);
    }

    fn until_next_task(&mut self) -> Option<Duration> {
        let global = self.global.clone()?;
        self.state()
            .dom
            .read(|cx| dom_api::until_next_due(cx, dom::GlobalRef::WorkerGlobal(global.get(cx.gc))))
    }

    fn run_due_tasks(&mut self) -> bool {
        let Some(global) = self.global.clone() else {
            return false;
        };
        self.state().dom.read(|cx| cx.begin_task_pass());
        let mut ran = false;
        loop {
            // The clock read again before each, as a window's engine does.
            let next = self.state().dom.read(|cx| {
                let context = cx.worker_global_context(global.get(cx.gc));
                let now = cx.dom_now();
                cx.next_task(context, now)
                    .map(|task| (task, cx.now_in(Some(context))))
            });
            let Some((task, now)) = next else {
                break;
            };
            let one = self
                .in_realm(|scope| {
                    let context = scope.get_current_context();
                    crate::window::run_task(scope, context, task.queue, now, None)
                })
                .unwrap_or(false);
            if !one {
                break;
            }
            ran = true;
        }
        self.state().dom.read(|cx| {
            cx.end_task_pass(&[cx.worker_global_context(global.get(cx.gc))]);
        });
        ran
    }

    fn set_task_waker(&mut self, waker: dom::TaskWaker) {
        self.state().dom.set_task_waker(waker);
    }

    fn run_dom_task(&mut self, number: u64) -> bool {
        let Some(global) = self.global.clone() else {
            return false;
        };
        // The one task the token stands for, wherever it is in its queue,
        // taken by its number: the scheduler said it is due.
        let found = self.state().dom.read(|cx| {
            let context = cx.worker_global_context(global.get(cx.gc));
            let queue = cx.task_numbered(context, number)?;
            Some((queue, cx.now_in(Some(context))))
        });
        let Some((queue, now)) = found else {
            return false;
        };
        self.in_realm(|scope| {
            let context = scope.get_current_context();
            crate::window::run_task(scope, context, queue, now, Some(number))
        })
        .unwrap_or(false)
    }

    fn port_messages_arrived(&mut self) {
        let Some(global) = self.global.clone() else {
            return;
        };
        self.state().dom.read(|cx| {
            cx.port_messages_arrived(cx.worker_global_context(global.get(cx.gc)));
        });
    }

    fn terminator(&mut self) -> Box<dyn Fn() + Send> {
        let handle = self.data.isolate().thread_safe_handle();
        Box::new(move || {
            handle.terminate_execution();
        })
    }

    fn set_console_sink(&mut self, sink: ConsoleSink) {
        self.data.set_console_sink(Box::new(move |message| {
            sink(ConsoleMessage {
                text: message.text,
                url: message.url,
                line: message.line,
                uncaught: message.uncaught,
            });
        }));
    }

    fn take_harness_results(&mut self) -> Option<String> {
        self.state().harness_results.take()
    }

    fn set_worker_host(&mut self, host: Box<dyn dom_api::WorkerHost>) {
        if let Some(waker) = host.port_waker() {
            self.state().dom.set_port_waker(waker);
        }
        self.state().caches.worker_host = Some(host);
    }

    fn worker_message(&mut self, id: u64, message: dom::SerializedMessage) {
        let Some(worker) = self.worker_object(id) else {
            return;
        };
        self.state()
            .dom
            .mutate(|cx| cx.post_worker_object_message(worker.get(cx.gc), message));
    }

    fn worker_error(&mut self, id: u64, error: &dom_api::WorkerError) {
        let Some(worker) = self.worker_object(id) else {
            return;
        };
        self.in_realm(|scope| worker_error_in(scope, &worker, error));
    }

    fn worker_script_failed(&mut self, id: u64) {
        let Some(worker) = self.worker_object(id) else {
            return;
        };
        self.in_realm(|scope| worker_script_failed_in(scope, &worker));
    }

    fn worker_closed(&mut self, id: u64) {
        if let Some(worker) = self.worker_object(id) {
            self.state()
                .dom
                .mutate(|cx| cx.close_worker_object(worker.get(cx.gc)));
        }
    }

    fn run_module_graph(
        &mut self,
        url: &url::Url,
        root: dom_api::module::FetchedModule,
        credentials: dom::ScriptCors,
    ) -> Option<bool> {
        self.in_realm(|scope| {
            let Some(mut scope) = state::scope(scope) else {
                return;
            };
            crate::modules::with_modules(
                &mut scope,
                &crate::modules::Realm::Worker,
                false,
                dom::LoadInitiator::Other,
                |map, _, host| map.fetch_worker_tree(host, url.clone(), root, credentials),
            );
            state::report(&mut scope);
        })?;
        self.module_graph_done()
    }

    fn module_fetched(
        &mut self,
        id: u64,
        response: Option<dom_api::module::FetchedModule>,
    ) -> Option<bool> {
        let found = self
            .in_realm(|scope| {
                let mut scope = state::scope(scope)?;
                let found = crate::modules::module_fetched(&mut scope, id, response);
                state::report(&mut scope);
                Some(found)
            })
            .flatten()
            .unwrap_or(false);
        // A task's end: an `import()` it settled reacts now.
        if found {
            self.perform_microtask_checkpoint();
        }
        self.module_graph_done()
    }
}
