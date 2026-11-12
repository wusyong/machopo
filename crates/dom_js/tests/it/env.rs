//! A realm with the DOM's interface objects and `machopoDom`, and what
//! the tests ask of it.

use bindings::V8PerIsolateData;

/// Whether this run's V8 starts GCs at its own times (one of the stress
/// flags in `MACHOPO_V8_FLAGS` that `test.py`'s matrix and fuzzer set: a
/// marking almost all the time, compaction at every GC, GCs at allocation
/// counts): a scavenge then often runs while a marking does and keeps the
/// wrappers it reached, unmodified or not, and a test of the scavenge
/// dropping one has nothing to test.
pub fn v8_gcs_at_its_own_times() -> bool {
    std::env::var("MACHOPO_V8_FLAGS").is_ok_and(|flags| {
        [
            "--stress-incremental-marking",
            "--stress-compaction",
            "--gc-interval",
            "--random-gc-interval",
        ]
        .iter()
        .any(|flag| {
            flags
                .split_whitespace()
                .any(|given| given.starts_with(flag))
        })
    })
}

/// Whether this run's V8 marks incrementally (`MACHOPO_V8_FLAGS` may turn it
/// off, as `test.py`'s fuzzer does): without it V8 starts no marking as the
/// DOM's heap grows, only an atomic GC at its heap's limit.
pub fn v8_marks_incrementally() -> bool {
    !std::env::var("MACHOPO_V8_FLAGS").is_ok_and(|flags| {
        flags
            .split_whitespace()
            .any(|flag| flag == "--no-incremental-marking")
    })
}

/// Whether this run gives the DOM's heap a nursery (`MACHOPO_GC_OPTIONS`'s
/// `nursery=`, as `test.py`'s matrix and fuzzer set it).
pub fn dom_heap_has_a_nursery() -> bool {
    std::env::var("MACHOPO_GC_OPTIONS").is_ok_and(|options| {
        options.split(',').any(|option| {
            option
                .trim()
                .strip_prefix("nursery=")
                .is_some_and(|bytes| bytes != "0")
        })
    })
}

pub struct Env {
    pub data: V8PerIsolateData,
    context: v8::Global<v8::Context>,
}

impl Env {
    pub fn new() -> Self {
        let mut data = V8PerIsolateData::new();
        let context = {
            let isolate = data.isolate();
            v8::scope!(let scope, isolate);
            let context = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, context);
            let global = context.global(scope);
            dom_js::install(scope, global);
            dom_js::install_machopo_dom(scope, global);
            v8::Global::new(scope, context)
        };
        Self { data, context }
    }

    /// A realm whose global is a window of the DOM, of a document at
    /// `url` (`dom_js::create_window_realm`).
    pub fn window(url: &str) -> Self {
        let mut data = V8PerIsolateData::new();
        let context = {
            let isolate = data.isolate();
            v8::scope!(let scope, isolate);
            // Any context of the isolate to make its templates in.
            let bootstrap = v8::Context::new(scope, Default::default());
            let scope = &mut v8::ContextScope::new(scope, bootstrap);
            let context = dom_js::create_window_realm(scope, url, None);
            v8::Global::new(scope, context)
        };
        Self { data, context }
    }

    /// Another realm of the same isolate whose global is a new window, of a
    /// document at `url`: what `eval` runs in from now on.
    pub fn add_window(&mut self, url: &str) {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let current = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, current);
        let context = dom_js::create_window_realm(scope, url, None);
        self.context = v8::Global::new(scope, context);
    }

    /// `source`'s completion value as a string, or its exception's
    /// `name: message`, `Exception: ` first.
    pub fn eval(&mut self, source: &str) -> String {
        // From no URL, shared cross-origin as a page's own script: what it
        // throws isn't muted.
        self.eval_at(source, "")
    }

    /// `source` run as a script at `url` (what an error's `filename` is),
    /// as `eval` runs it.
    pub fn eval_at(&mut self, source: &str, url: &str) -> String {
        // Script entered, as the engine's entries count it.
        let _entry = bindings::trace::enter_script();
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        v8::tc_scope!(let scope, scope);
        let source = v8::String::new(scope, source).unwrap();
        let name = v8::String::new(scope, url).unwrap();
        let origin = v8::ScriptOrigin::new(
            scope,
            name.into(),
            0,
            0,
            true,
            0,
            None,
            false,
            false,
            false,
            None,
        );
        let result =
            v8::Script::compile(scope, source, Some(&origin)).and_then(|script| script.run(scope));
        match result {
            Some(value) => value.to_rust_string_lossy(scope),
            None => {
                let exception = scope.exception().expect("an exception");
                format!("Exception: {}", exception.to_rust_string_lossy(scope))
            }
        }
    }

    /// The window's timers due by `now` (its clock's now if none) run:
    /// how many (`dom_js::run_timers`).
    pub fn run_timers(&mut self, now: Option<f64>) -> usize {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        dom_js::run_timers(scope, context, now)
    }

    /// The window's element tasks run: how many
    /// (`dom_js::run_element_tasks`).
    pub fn run_element_tasks(&mut self) -> usize {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        dom_js::run_element_tasks(scope, context)
    }

    /// The window's messages delivered: how many
    /// (`dom_js::run_posted_messages`).
    pub fn run_posted_messages(&mut self) -> usize {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        dom_js::run_posted_messages(scope, context)
    }

    /// A microtask checkpoint (what a task's end does).
    pub fn checkpoint(&mut self) {
        self.data.isolate().perform_microtask_checkpoint();
    }

    /// When the window's next timer is due (`dom_js::next_timer`).
    pub fn next_timer(&mut self) -> Option<f64> {
        let isolate = self.data.isolate();
        v8::scope!(let scope, isolate);
        let context = v8::Local::new(scope, &self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        dom_js::next_timer(scope, context)
    }

    /// A full GC that doesn't scan the stack (a stale word there may keep
    /// a wrapper a GC longer).
    pub fn full_gc(&mut self) {
        self.data
            .isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Full,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }

    pub fn minor_gc(&mut self) {
        self.data
            .isolate()
            .request_garbage_collection_for_testing_with_stack_state(
                v8::GarbageCollectionType::Minor,
                v8::cppgc::EmbedderStackState::NoHeapPointers,
            );
    }

    pub fn wrappers_made(&mut self) -> u64 {
        dom_js::wrappers_made(self.data.isolate())
    }

    pub fn wrappers_ahead(&mut self) -> u64 {
        dom_js::wrappers_ahead(self.data.isolate())
    }

    pub fn wrappers_wasted(&mut self) -> u64 {
        dom_js::wrappers_wasted(self.data.isolate())
    }

    pub fn string_conversions(&mut self) -> u64 {
        dom_js::string_conversions(self.data.isolate())
    }

    pub fn cached_strings(&mut self) -> usize {
        dom_js::cached_strings(self.data.isolate())
    }

    pub fn names_made(&mut self) -> u64 {
        dom_js::names_made(self.data.isolate())
    }

    pub fn uncaught_exceptions(&mut self) -> u64 {
        dom_js::uncaught_exceptions(self.data.isolate())
    }

    /// The isolate's DOM, as the host reaches it (an image's fetch).
    pub fn dom(&mut self) -> &mut dom::Dom {
        dom_js::dom_mut(self.data.isolate()).expect("the isolate's DOM")
    }

    pub fn used_bytes(&mut self) -> usize {
        dom_js::heap_stats(self.data.isolate()).map_or(0, |stats| stats.used_bytes)
    }
}
