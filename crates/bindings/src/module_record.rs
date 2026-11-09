//! ES modules on V8: Blink's `ModuleRecord`, with the V8 half of its
//! `ModuleScript` (the record, parse error and error to rethrow) and of its
//! `ModuleRecordResolverImpl` (which module an import links to).
//!
//! Fetching the graph and resolving specifiers is the embedder's (HTML's
//! module map; Blink's `core/script`): it compiles each module it fetches,
//! tells which module each of its requests resolved to, then links and
//! evaluates the graph. Modules are numbered per realm.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::script_state::ScriptLocation;

/// A realm's modules: each one's record, parse error and error to rethrow,
/// held strongly, so they live as long as this does. Its owner drops it
/// with the realm (HTML's module map never forgets a module either).
///
/// What V8's callbacks need to find a module (`resolve_module`,
/// `initialize_import_meta`) is in the context's slot too, without a
/// strong reference: a `v8::Global` in a slot is a root, which would keep
/// the context alive through the module, and the slot with it, for good.
pub struct ModuleRecords {
    modules: Vec<Strong>,
    linkage: Rc<RefCell<Linkage>>,
}

struct Strong {
    /// `None` if it didn't compile.
    record: Option<v8::Global<v8::Module>>,
    parse_error: Option<v8::Global<v8::Value>>,
    error_to_rethrow: Option<v8::Global<v8::Value>>,
}

/// The context's slot: what the callbacks look up, by the record.
#[derive(Default)]
struct Linkage {
    modules: Vec<Linked>,
    /// The modules that compiled, by their record's identity hash, which
    /// isn't unique.
    by_hash: HashMap<i32, Vec<usize>>,
    /// HTML's "resolve a module specifier" against a base URL, the
    /// embedder's (`import.meta.resolve`); none: `import.meta` has no
    /// `resolve`.
    resolve: Option<ResolveSpecifier>,
}

/// What resolves a module specifier against a base URL: the URL, or the
/// `TypeError`'s message.
pub type ResolveSpecifier = fn(base_url: &str, specifier: &str) -> Result<String, String>;

struct Linked {
    record: Option<v8::Weak<v8::Module>>,
    /// Its base URL, `import.meta.url`.
    url: String,
    /// Its requests, as `compile` gave them.
    requests: Vec<Request>,
    /// The module each request resolved to, once the embedder says.
    resolved: Vec<usize>,
}

impl Linkage {
    /// The module whose record `record` is.
    fn find(
        &self,
        scope: &v8::PinScope<'_, '_, ()>,
        record: v8::Local<'_, v8::Module>,
    ) -> Option<usize> {
        let candidates = self.by_hash.get(&record.get_identity_hash().get())?;
        candidates.iter().copied().find(|&module| {
            self.modules[module]
                .record
                .as_ref()
                .and_then(|weak| weak.to_local(scope))
                == Some(record)
        })
    }
}

/// A module's request (ECMAScript's ModuleRequest Record), as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub specifier: String,
    /// Keys and values, in source order.
    pub attributes: Vec<(String, String)>,
}

/// What [`ModuleRecords::compile`] made.
#[derive(Debug)]
pub struct Compiled {
    /// The module's number in its realm.
    pub module: usize,
    /// Its requests, in source order, each once; `None` if it doesn't
    /// parse.
    pub requests: Option<Vec<Request>>,
}

impl ModuleRecords {
    /// A realm's (empty) modules, for the context `scope` is in.
    pub fn new(scope: &mut v8::PinScope<'_, '_>) -> Self {
        Self::with_resolve(scope, None)
    }

    /// [`new`](Self::new), with `import.meta.resolve()` resolving by
    /// `resolve` against the module's base URL.
    pub fn with_resolve(
        scope: &mut v8::PinScope<'_, '_>,
        resolve: Option<ResolveSpecifier>,
    ) -> Self {
        let linkage = Rc::new(RefCell::new(Linkage {
            resolve,
            ..Linkage::default()
        }));
        let previous = scope.get_current_context().set_slot(linkage.clone());
        assert!(previous.is_none(), "a context has one realm's modules");
        Self {
            modules: Vec::new(),
            linkage,
        }
    }

    /// Compiles `source`, from `location`, as a module (`ModuleRecord::
    /// Compile`) whose base URL, its `import.meta.url`, is `base_url`. A
    /// syntax error becomes its parse error.
    pub fn compile(
        &mut self,
        scope: &mut v8::PinScope<'_, '_>,
        source: &str,
        location: ScriptLocation<'_>,
        base_url: &str,
    ) -> Compiled {
        self.compile_with(scope, source, location, base_url, None)
    }

    /// [`compile`](Self::compile), with the credentials mode of its fetch
    /// options too ([`crate::script_options`]): what an `import()` in it
    /// fetches with.
    pub fn compile_with(
        &mut self,
        scope: &mut v8::PinScope<'_, '_>,
        source: &str,
        location: ScriptLocation<'_>,
        base_url: &str,
        credentials: Option<&str>,
    ) -> Compiled {
        let _span = trace_event::trace_event!("ModuleRecords::compile", url = location.url);
        v8::tc_scope!(let tc, scope);
        let record = v8::String::new(tc, source).and_then(|code| {
            let name = v8::String::new(tc, location.url).unwrap_or_else(|| v8::String::empty(tc));
            let line_offset = i32::try_from(location.line.saturating_sub(1)).unwrap_or(i32::MAX);
            // An `import()` in it resolves against its base URL.
            let options = crate::script_options(tc, base_url, credentials);
            let origin = v8::ScriptOrigin::new(
                tc,
                name.into(),
                line_offset,
                0,
                // Shared cross-origin: machopo doesn't mute any script's errors
                // (Blink's `SanitizeScriptErrors::kDoNotSanitize`), so DevTools
                // gets the exception itself.
                true,
                0,
                None,
                false,
                false,
                true,
                options,
            );
            let mut source = v8::script_compiler::Source::new(code, Some(&origin));
            v8::script_compiler::compile_module(tc, &mut source)
        });
        let module = self.modules.len();
        let Some(record) = record else {
            // A source too large for a V8 string throws nothing: a
            // `RangeError`, as V8 throws for one built in script.
            let error = match tc.exception() {
                Some(exception) => exception,
                None => {
                    let message = v8::String::new(tc, "source too large").expect("a short string");
                    v8::Exception::range_error(tc, message)
                }
            };
            self.modules.push(Strong {
                record: None,
                parse_error: Some(v8::Global::new(tc, error)),
                error_to_rethrow: None,
            });
            self.linkage.borrow_mut().modules.push(Linked {
                record: None,
                url: base_url.to_owned(),
                requests: Vec::new(),
                resolved: Vec::new(),
            });
            return Compiled {
                module,
                requests: None,
            };
        };
        let requests = requests(tc, record);
        self.modules.push(Strong {
            record: Some(v8::Global::new(tc, record)),
            parse_error: None,
            error_to_rethrow: None,
        });
        let weak = v8::Weak::new(tc, record);
        let mut linkage = self.linkage.borrow_mut();
        linkage.modules.push(Linked {
            record: Some(weak),
            url: base_url.to_owned(),
            requests: requests.clone(),
            resolved: Vec::new(),
        });
        linkage
            .by_hash
            .entry(record.get_identity_hash().get())
            .or_default()
            .push(module);
        Compiled {
            module,
            requests: Some(requests),
        }
    }

    /// Sets `module`'s parse error: one of its requests is invalid.
    ///
    /// # Panics
    ///
    /// If there is no such module.
    pub fn set_parse_error(
        &mut self,
        scope: &mut v8::PinScope<'_, '_>,
        module: usize,
        error: v8::Local<'_, v8::Value>,
    ) {
        self.modules[module].parse_error = Some(v8::Global::new(scope, error));
    }

    /// The modules `module`'s requests resolved to, in the order
    /// [`compile`](Self::compile) gave them: what linking links them to.
    ///
    /// # Panics
    ///
    /// If there is no such module, it didn't compile, or `resolved` isn't
    /// one for each request.
    pub fn set_requests(&mut self, module: usize, resolved: Vec<usize>) {
        assert!(
            self.modules[module].record.is_some(),
            "a module that compiled"
        );
        assert!(
            resolved.iter().all(|&child| child < self.modules.len()),
            "modules of this realm"
        );
        let mut linkage = self.linkage.borrow_mut();
        let linked = &mut linkage.modules[module];
        assert_eq!(
            linked.requests.len(),
            resolved.len(),
            "one module per request"
        );
        linked.resolved = resolved;
    }

    /// HTML "fetch the descendants of and link a module script", once the
    /// graph under `root` is fetched: `parse_error`'s parse error (the
    /// graph's first) becomes `root`'s error to rethrow; without one, the
    /// graph is linked (`ModuleRecord::Instantiate`), and the exception
    /// linking throws, if any, becomes it.
    ///
    /// # Panics
    ///
    /// If `root` didn't compile, or `parse_error` has no parse error.
    pub fn link(
        &mut self,
        scope: &mut v8::PinScope<'_, '_>,
        root: usize,
        parse_error: Option<usize>,
    ) {
        let _span = trace_event::trace_event!("ModuleRecords::link", root);
        if let Some(module) = parse_error {
            let error = self.modules[module]
                .parse_error
                .clone()
                .expect("a module with a parse error");
            self.modules[root].error_to_rethrow = Some(error);
            return;
        }
        let record = self.modules[root]
            .record
            .as_ref()
            .expect("a graph without a parse error compiled");
        v8::tc_scope!(let tc, scope);
        let record = v8::Local::new(tc, record);
        if record.instantiate_module(tc, resolve_module).is_none() {
            let error = match tc.exception() {
                Some(exception) => exception,
                // Terminated: there is nothing to rethrow, but evaluating
                // an unlinked graph would crash.
                None => v8::undefined(tc).into(),
            };
            self.modules[root].error_to_rethrow = Some(v8::Global::new(tc, error));
        }
    }

    /// What an `import()` of the graph `root`, fetched, takes (HTML's
    /// "ContinueDynamicImport"): `parse_error`'s parse error (the graph's
    /// first) to reject with; else the graph linked now, by this import,
    /// so an exception linking it is a new one for each import (unlike a
    /// script's error to rethrow, which every script of the graph shares);
    /// else its record to evaluate.
    ///
    /// # Panics
    ///
    /// If `root` didn't compile, or `parse_error` has no parse error.
    pub fn dynamic_import_evaluation(
        &mut self,
        scope: &mut v8::PinScope<'_, '_>,
        root: usize,
        parse_error: Option<usize>,
    ) -> Evaluation {
        if let Some(module) = parse_error {
            let error = self.modules[module]
                .parse_error
                .clone()
                .expect("a module with a parse error");
            return Evaluation::Rethrow(error);
        }
        let record = self.modules[root]
            .record
            .clone()
            .expect("a graph without a parse error compiled");
        v8::tc_scope!(let tc, scope);
        let local = v8::Local::new(tc, &record);
        if local.instantiate_module(tc, resolve_module).is_none() {
            let error = match tc.exception() {
                Some(exception) => exception,
                None => v8::undefined(tc).into(),
            };
            return Evaluation::Rethrow(v8::Global::new(tc, error));
        }
        Evaluation::Run(record)
    }

    /// What evaluating the graph `root`, linked by [`link`](Self::link),
    /// takes: its record, or its error to rethrow. Owned, so the caller
    /// needn't hold these modules while the graph runs (and maybe calls
    /// back into the embedder).
    ///
    /// # Panics
    ///
    /// If `root` didn't compile and has no error to rethrow.
    pub fn evaluation(&self, root: usize) -> Evaluation {
        let module = &self.modules[root];
        match (&module.error_to_rethrow, &module.record) {
            (Some(error), _) => Evaluation::Rethrow(error.clone()),
            (None, Some(record)) => Evaluation::Run(record.clone()),
            (None, None) => panic!("module {root} wasn't linked"),
        }
    }
}

/// What [`ModuleRecords::evaluation`] gives, for [`Evaluation::evaluate`].
pub enum Evaluation {
    Run(v8::Global<v8::Module>),
    Rethrow(v8::Global<v8::Value>),
}

impl Evaluation {
    /// The first half of HTML "run a module script" (`ModuleRecord::
    /// Evaluate`): the promise of the graph's evaluation, which rejects
    /// with what it threw, or with its error to rethrow; `None` if script
    /// was terminated. Microtasks wait for the event loop's checkpoint
    /// (the explicit policy), as after a classic script.
    ///
    /// # Panics
    ///
    /// If the graph hasn't been linked.
    pub fn evaluate<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> Option<v8::Local<'s, v8::Promise>> {
        let _span = trace_event::trace_event!("Evaluation::evaluate");
        let _entry = crate::trace::enter_script();
        match self {
            Self::Rethrow(error) => {
                let error = v8::Local::new(scope, error);
                let resolver = v8::PromiseResolver::new(scope)?;
                resolver.reject(scope, error)?;
                Some(resolver.get_promise(scope))
            }
            Self::Run(record) => {
                let record = v8::Local::new(scope, record);
                assert!(
                    !matches!(
                        record.get_status(),
                        v8::ModuleStatus::Uninstantiated | v8::ModuleStatus::Instantiating
                    ),
                    "a linked graph"
                );
                let completion = record.evaluate(scope)?;
                v8::Local::<v8::Promise>::try_from(completion).ok()
            }
        }
    }
}

/// HTML's `HostLoadImportedModule` for an `import()`, its first half (Blink's
/// `DynamicModuleResolver::ResolveDynamically`): a promise for it, kept in
/// the current context's table of promises under a number
/// ([`crate::create_promise`]), for the embedder to fetch the graph and
/// then [finish](finish_dynamic_import) or [reject](reject_dynamic_import)
/// it. `None` if the context has no modules ([`ModuleRecords::new`]) or no
/// table of promises, or no promise could be made (an exception is then
/// pending).
pub fn begin_dynamic_import<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Option<(u64, v8::Local<'s, v8::Promise>)> {
    scope.get_current_context().get_slot::<RefCell<Linkage>>()?;
    let (import, _) = crate::promise_resolvers::create_promise(scope)?;
    let promise = crate::promise_resolvers::hand_out(scope, import)?;
    Some((import, promise))
}

/// The promise of `import()` number `import` (a number value), taken out of
/// the current context's table: a handler's, run in the realm that made
/// it.
fn take_import<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    import: v8::Local<'_, v8::Value>,
) -> Option<v8::Local<'s, v8::PromiseResolver>> {
    // Ours, made from a `u64`; were it anything else, no import has it.
    let import = base::numerics::saturated_cast(import.number_value(scope)?);
    crate::promise_resolvers::take_resolver(scope, import)
}

/// The end of an `import()` ([`begin_dynamic_import`]) of the current
/// context whose graph was fetched and linked (HTML's
/// "ContinueDynamicImport"), given what evaluating it takes
/// ([`ModuleRecords::evaluation`], so the modules needn't be held while it
/// runs): evaluates it, and once that settles, resolves the import's
/// promise with the module's namespace, or rejects it with what
/// evaluating rejected with (the error to rethrow, too). Nothing if the
/// import isn't waiting (its realm is going).
pub fn finish_dynamic_import(
    scope: &mut v8::PinScope<'_, '_>,
    import: u64,
    evaluation: &Evaluation,
) {
    let _span = trace_event::trace_event!("finish_dynamic_import", import);
    if !crate::promise_resolvers::is_waiting(scope, import) {
        return;
    }
    let namespace = match evaluation {
        Evaluation::Run(record) => v8::Local::new(scope, record).get_module_namespace(),
        Evaluation::Rethrow(_) => v8::undefined(scope).into(),
    };
    let Some(evaluated) = evaluation.evaluate(scope) else {
        return;
    };
    // The promise stays in the table until evaluating settles: the
    // handlers find it by its number (V8 can't hand a resolver back from a
    // value).
    let number = v8::Number::new(scope, import as f64);
    let settled = v8::Array::new_with_elements(scope, &[number.into(), namespace]);
    let fulfilled = v8::Function::builder(import_fulfilled)
        .data(settled.into())
        .build(scope);
    let rejected = v8::Function::builder(import_rejected)
        .data(number.into())
        .build(scope);
    if let (Some(fulfilled), Some(rejected)) = (fulfilled, rejected) {
        // `None` only if script was terminated meanwhile.
        let _ = evaluated.then2(scope, fulfilled, rejected);
    }
}

/// Rejects the promise of the current context's `import()` number
/// `import` with `error`: its request was invalid, or its graph failed to
/// fetch. Nothing if it isn't waiting.
pub fn reject_dynamic_import(
    scope: &mut v8::PinScope<'_, '_>,
    import: u64,
    error: v8::Local<'_, v8::Value>,
) {
    let number = v8::Number::new(scope, import as f64);
    if let Some(resolver) = take_import(scope, number.into()) {
        resolver.reject(scope, error);
    }
}

/// An `import()`'s graph evaluated: its promise resolves with the namespace
/// (the function's data: the import's number, then the namespace).
fn import_fulfilled(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let Ok(settled) = v8::Local::<v8::Array>::try_from(args.data()) else {
        return;
    };
    let (Some(import), Some(namespace)) =
        (settled.get_index(scope, 0), settled.get_index(scope, 1))
    else {
        return;
    };
    if let Some(resolver) = take_import(scope, import) {
        resolver.resolve(scope, namespace);
    }
}

/// An `import()`'s graph threw or had an error to rethrow: its promise
/// rejects with the reason (the function's data: the import's number).
fn import_rejected(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    if let Some(resolver) = take_import(scope, args.data()) {
        resolver.reject(scope, args.get(0));
    }
}

/// `record`'s requests, in source order.
fn requests(scope: &mut v8::PinScope<'_, '_>, record: v8::Local<'_, v8::Module>) -> Vec<Request> {
    let array = record.get_module_requests();
    let mut requests = Vec::with_capacity(array.length());
    for index in 0..array.length() {
        let Some(request) = array
            .get(scope, index)
            .and_then(|data| v8::Local::<v8::ModuleRequest>::try_from(data).ok())
        else {
            continue;
        };
        requests.push(Request {
            specifier: request.get_specifier().to_rust_string_lossy(scope),
            attributes: attributes(scope, request.get_import_attributes()),
        });
    }
    requests
}

/// A request's import attributes, from V8's `[key, value, offset, ...]`.
fn attributes(
    scope: &mut v8::PinScope<'_, '_>,
    array: v8::Local<'_, v8::FixedArray>,
) -> Vec<(String, String)> {
    let string = |scope: &mut v8::PinScope<'_, '_>, index| {
        array
            .get(scope, index)
            .and_then(|data| v8::Local::<v8::String>::try_from(data).ok())
            .map(|s| s.to_rust_string_lossy(scope))
            .unwrap_or_default()
    };
    let keys = (0..array.length()).step_by(3);
    let values = (1..array.length()).step_by(3);
    keys.zip(values)
        .map(|(key, value)| (string(scope, key), string(scope, value)))
        .collect()
}

/// V8's `ResolveModuleCallback` (Blink's `ModuleRecord::ResolveModuleCallback`
/// and `ModuleRecordResolverImpl::Resolve`): the module `referrer`'s
/// request of `specifier` with `import_attributes` resolved to, from the
/// context's [`Linkage`].
fn resolve_module<'s>(
    context: v8::Local<'s, v8::Context>,
    specifier: v8::Local<'s, v8::String>,
    import_attributes: v8::Local<'s, v8::FixedArray>,
    referrer: v8::Local<'s, v8::Module>,
) -> Option<v8::Local<'s, v8::Module>> {
    // SAFETY: V8 calls this on the isolate's thread, inside
    // `Module::instantiate_module` in `context` (which `ModuleRecords::link`
    // called with a scope of it), so a callback scope may be opened on it.
    v8::callback_scope!(unsafe scope, context);
    let resolved = context.get_slot::<RefCell<Linkage>>().and_then(|linkage| {
        let linkage = linkage.borrow();
        let request = Request {
            specifier: specifier.to_rust_string_lossy(scope),
            attributes: attributes(scope, import_attributes),
        };
        let referrer = &linkage.modules[linkage.find(scope, referrer)?];
        let index = referrer.requests.iter().position(|r| *r == request)?;
        let &child = referrer.resolved.get(index)?;
        linkage.modules[child].record.as_ref()?.to_local(scope)
    });
    if resolved.is_none() {
        // The embedder links a graph only once each request is resolved to
        // a module that compiled.
        let message = format!(
            "module \"{}\" wasn't resolved",
            specifier.to_rust_string_lossy(scope)
        );
        let message = v8::String::new(scope, &message).unwrap_or_else(|| v8::String::empty(scope));
        let error = v8::Exception::error(scope, message);
        scope.throw_exception(error);
    }
    resolved
}

/// V8's `HostInitializeImportMetaObjectCallback` (Blink's
/// `HostGetImportMetaProperties`): `import.meta.url` is the module's URL.
pub(crate) unsafe extern "C" fn initialize_import_meta(
    context: v8::Local<v8::Context>,
    module: v8::Local<v8::Module>,
    meta: v8::Local<v8::Object>,
) {
    // SAFETY: V8 calls this on the isolate's thread, while script of
    // `context` runs (its first `import.meta`), so a callback scope may be
    // opened on it.
    v8::callback_scope!(unsafe scope, context);
    let Some(linkage) = context.get_slot::<RefCell<Linkage>>() else {
        return;
    };
    let (url, resolves) = {
        let linkage = linkage.borrow();
        (
            linkage
                .find(scope, module)
                .map(|module| linkage.modules[module].url.clone()),
            linkage.resolve.is_some(),
        )
    };
    let Some(url) = url else {
        return;
    };
    let key = v8::String::new(scope, "url").expect("a short string");
    let Some(url) = v8::String::new(scope, &url) else {
        return;
    };
    meta.create_data_property(scope, key.into(), url.into());
    // HTML's `import.meta.resolve(specifier)`, against the module's base
    // URL (Blink's `ImportMetaResolve`): a function of the module's.
    if resolves
        && let Some(resolve) = v8::Function::builder(import_meta_resolve)
            .data(url.into())
            .length(1)
            .build(scope)
    {
        let key = v8::String::new(scope, "resolve").expect("a short string");
        meta.create_data_property(scope, key.into(), resolve.into());
    }
}

/// `import.meta.resolve(specifier)` (its data the module's base URL): the
/// URL the specifier resolves to, or a `TypeError`.
fn import_meta_resolve(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    mut rv: v8::ReturnValue,
) {
    let Some(specifier) = args.get(0).to_string(scope) else {
        return;
    };
    let specifier = specifier.to_rust_string_lossy(scope);
    let base_url = args.data().to_rust_string_lossy(scope);
    let resolve = scope
        .get_current_context()
        .get_slot::<RefCell<Linkage>>()
        .and_then(|linkage| linkage.borrow().resolve);
    let Some(resolve) = resolve else {
        return;
    };
    match resolve(&base_url, &specifier) {
        Ok(url) => {
            if let Some(url) = v8::String::new(scope, &url) {
                rv.set(url.into());
            }
        }
        Err(message) => {
            let message =
                v8::String::new(scope, &message).unwrap_or_else(|| v8::String::empty(scope));
            let error = v8::Exception::type_error(scope, message);
            scope.throw_exception(error);
        }
    }
}
