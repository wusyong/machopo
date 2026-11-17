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
        let options = bindings::script_options(tc, &script.url, Some("same-origin"));
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
        self.state().caches.worker_global_host = Some(host);
        self.state()
            .dom
            .set_agent(&params.user_agent, params.webdriver);
        let web_tests = self.settings.web_tests;
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let bootstrap = v8::Local::new(scope, &self.bootstrap);
        let scope = &mut v8::ContextScope::new(scope, bootstrap);
        let (context, global) = create_worker_realm(scope, params, web_tests);
        self.context = Some(v8::Global::new(scope, context));
        self.global = Some(global);
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
                script.base_url.unwrap_or(script.url),
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
                    crate::window::run_task(scope, context, task.queue, now)
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
}
