//! A realm's module scripts on V8 (Blink's `ModulatorImpl` with its
//! `ModuleRecord`s and `DynamicModuleResolver`): the module map and graph
//! fetching are `dom_api::module`'s, engine-free; the records are
//! `bindings::ModuleRecords`', with the realm's table of promises an
//! `import()` waits in; all are the realm's (`Realm`: a frame's, or the
//! worker's whose isolate it is), made at its first module script,
//! `import()` or `modulepreload`, as long as the realm lives. A frame's
//! fetch goes to the host through the DOM (`Cx::queue_module_fetch`), a
//! worker's through its host (`WorkerGlobalHost::start_module_fetch`, as
//! Blink's `WorkerModuleScriptFetcher` differs from the document's
//! fetcher alone), and comes back (`module_fetched`). A graph fetched is
//! its pending script's (`Cx::module_graph_fetched`, which runs it in a
//! task of its own: `run_module_script`), its module worker's (the worker
//! runs it then: `WorkerRealm::graph`), its `import()`'s (linked and
//! evaluated, its promise settled), or its `modulepreload`'s (`load` or
//! `error` at the link, in a task).

use std::collections::HashMap;

use bindings::{ModuleRecords, PromiseResolvers, ScriptLocation};
use dom::{LoadInitiator, NodeRoot, ScriptCors, ScriptRequest, names};
use elise::Persistent;
use dom_api::module::{
    CompiledModule, FetchedModule, Finished, GraphFor, Modulator, ModuleHost, ModuleParseError,
    ModuleRequest, ModuleScriptId,
};
use dom_api::{ModuleSource, ModuleStart};
use url::Url;

use crate::state::Scope;

/// A realm's modules: their records, its module map, its table of
/// promises, and the URL each `import()` waiting for its graph asked for
/// (by its promise's number), for the error if it fails.
pub(crate) struct RealmModules {
    records: ModuleRecords,
    map: Modulator,
    _promises: PromiseResolvers,
    imports: HashMap<u64, Url>,
}

/// Whose modules: the realm of the frame showing a document, or the
/// worker's (a worker's isolate has the one realm).
#[derive(Clone)]
pub(crate) enum Realm {
    Document(NodeRoot),
    Worker,
}

/// A worker's realm, for its modules: its context and global, its modules
/// (made at the first), and its module worker's graph once it is fetched,
/// for the worker to run (`V8WorkerEngine::module_graph_done`).
pub(crate) struct WorkerRealm {
    pub(crate) context: v8::Global<v8::Context>,
    pub(crate) global: Persistent<dom::WorkerGlobalScope>,
    modules: Option<Box<RealmModules>>,
    pub(crate) graph: Option<Finished>,
}

impl WorkerRealm {
    pub(crate) fn new(
        context: v8::Global<v8::Context>,
        global: Persistent<dom::WorkerGlobalScope>,
    ) -> Self {
        Self {
            context,
            global,
            modules: None,
            graph: None,
        }
    }
}

/// What the module map needs of the engine and the host, for the realm
/// `realm`, whose context is `context`; who asked for a graph's root
/// (`root`), for DevTools.
struct Host<'a, 'b, 's, 'i> {
    scope: &'a mut Scope<'b, 's, 'i>,
    context: &'a v8::Global<v8::Context>,
    realm: &'a Realm,
    records: &'a mut ModuleRecords,
    root: LoadInitiator,
}

impl ModuleHost for Host<'_, '_, '_, '_> {
    /// A frame's from the DOM's queue, as a module (CORS), asked for by its
    /// importer (a root, by who asked for its graph); a worker's from its
    /// host, for its owner if it is of its graph (0 for a worker with no
    /// host, which never comes).
    fn fetch(
        &mut self,
        url: &Url,
        delays_load: bool,
        credentials: ScriptCors,
        importer: Option<&Url>,
    ) -> u64 {
        let document = match self.realm {
            Realm::Document(document) => document,
            Realm::Worker => {
                let (state, _) = self.scope.state();
                return state
                    .caches
                    .worker_global_host
                    .as_deref()
                    .map_or(0, |host| host.start_module_fetch(url, credentials, delays_load));
            }
        };
        let request = ScriptRequest {
            url: url.as_str().to_owned(),
            cors: Some(credentials),
            charset: None,
        };
        let initiator = importer.map_or_else(
            || self.root.clone(),
            |importer| LoadInitiator::Import(importer.as_str().to_owned()),
        );
        let (state, _) = self.scope.state_mut();
        state
            .dom
            .mutate(|cx| cx.queue_module_fetch(document.get(cx.gc), request, initiator))
    }

    fn compile(
        &mut self,
        source: &str,
        url: &Url,
        base_url: &Url,
        line: u32,
        credentials: ScriptCors,
    ) -> CompiledModule {
        let scope = self.scope.scope();
        let context = v8::Local::new(scope, self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let location = ScriptLocation {
            url: url.as_str(),
            line,
            base_url: Some(base_url.as_str()),
        };
        let compiled = self.records.compile_with(
            scope,
            source,
            location,
            base_url.as_str(),
            Some(credentials_name(credentials)),
        );
        CompiledModule {
            id: id(compiled.module),
            requests: compiled.requests.map(|requests| {
                requests
                    .into_iter()
                    .map(|request| ModuleRequest {
                        specifier: request.specifier,
                        attributes: request.attributes,
                    })
                    .collect()
            }),
        }
    }

    /// The error HTML makes: a `SyntaxError` or a `TypeError`.
    fn set_parse_error(&mut self, module: ModuleScriptId, error: ModuleParseError) {
        let scope = self.scope.scope();
        let context = v8::Local::new(scope, self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        let value = new_error(scope, error);
        self.records.set_parse_error(scope, index(module), value);
    }

    fn set_requests(&mut self, module: ModuleScriptId, resolved: Vec<ModuleScriptId>) {
        self.records
            .set_requests(index(module), resolved.into_iter().map(index).collect());
    }

    fn link(&mut self, root: ModuleScriptId, parse_error: Option<ModuleScriptId>) {
        let scope = self.scope.scope();
        let context = v8::Local::new(scope, self.context);
        let scope = &mut v8::ContextScope::new(scope, context);
        self.records
            .link(scope, index(root), parse_error.map(index));
    }
}

/// A credentials mode as script options carry it ([`bindings::script_options`]).
fn credentials_name(credentials: ScriptCors) -> &'static str {
    match credentials {
        ScriptCors::Anonymous => "same-origin",
        ScriptCors::UseCredentials => "include",
        ScriptCors::Omit => "omit",
    }
}

/// The error a module's request gives, as a script value.
fn new_error<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    error: ModuleParseError,
) -> v8::Local<'s, v8::Value> {
    let (ModuleParseError::Syntax(message) | ModuleParseError::Type(message)) = &error;
    let message = v8::String::new(scope, message).unwrap_or_else(|| v8::String::empty(scope));
    match error {
        ModuleParseError::Syntax(_) => v8::Exception::syntax_error(scope, message),
        ModuleParseError::Type(_) => v8::Exception::type_error(scope, message),
    }
}

/// `import.meta.resolve`'s: HTML's "resolve a module specifier" against
/// the module's base URL (no import maps).
fn resolve_specifier(base_url: &str, specifier: &str) -> Result<String, String> {
    let failed = || {
        format!(
            "Failed to resolve module specifier \"{specifier}\". Relative references must start with either \"/\", \"./\", or \"../\"."
        )
    };
    let base = Url::parse(base_url).map_err(|_| failed())?;
    dom_api::module::resolve_module_specifier(&base, specifier)
        .map(String::from)
        .ok_or_else(failed)
}

/// `realm`'s context, and its modules taken out (made at the first; put
/// back by `put_back`). None if no frame shows its document, or for a
/// worker's before it is made.
fn take_modules(
    scope: &mut Scope<'_, '_, '_>,
    realm: &Realm,
) -> Option<(v8::Global<v8::Context>, Box<RealmModules>)> {
    let (context, modules) = {
        let (state, _) = scope.state_mut();
        match realm {
            Realm::Document(document) => {
                let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
                let frame = state.frames.of_document_mut(id)?;
                (frame.context.clone(), frame.modules.take())
            }
            Realm::Worker => {
                let worker = state.worker_realm.as_mut()?;
                (worker.context.clone(), worker.modules.take())
            }
        }
    };
    let modules = modules.unwrap_or_else(|| {
        let scope = scope.scope();
        let local = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, local);
        Box::new(RealmModules {
            records: ModuleRecords::with_resolve(scope, Some(resolve_specifier)),
            map: Modulator::default(),
            _promises: PromiseResolvers::new(scope),
            imports: HashMap::new(),
        })
    });
    Some((context, modules))
}

/// The realm's modules back in it (a frame's if it still shows its
/// document).
fn put_back(scope: &mut Scope<'_, '_, '_>, realm: &Realm, modules: Box<RealmModules>) {
    let (state, _) = scope.state_mut();
    match realm {
        Realm::Document(document) => {
            let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
            if let Some(frame) = state.frames.of_document_mut(id) {
                frame.modules = Some(modules);
            }
        }
        Realm::Worker => {
            if let Some(worker) = state.worker_realm.as_mut() {
                worker.modules = Some(modules);
            }
        }
    }
}

/// `f` with the module map and host of `realm`: what it finished settled
/// (`settle`; an `import()`'s in a microtask if `deferred`, as one in
/// script). No script runs while the modules are out of the realm.
pub(crate) fn with_modules(
    scope: &mut Scope<'_, '_, '_>,
    realm: &Realm,
    deferred: bool,
    root: LoadInitiator,
    f: impl FnOnce(&mut Modulator, &mut HashMap<u64, Url>, &mut dyn ModuleHost) -> Vec<Finished>,
) {
    let Some((context, mut modules)) = take_modules(scope, realm) else {
        return;
    };
    let RealmModules {
        records,
        map,
        imports,
        ..
    } = &mut *modules;
    let finished = f(
        map,
        imports,
        &mut Host {
            scope,
            context: &context,
            realm,
            records,
            root,
        },
    );
    put_back(scope, realm, modules);
    settle(scope, realm, &context, finished, deferred);
}

/// What the graphs that finished are for, told: a module script's pending
/// script (its root, linked, or with its error to rethrow; none if a
/// module failed to fetch); its module worker (kept for it to run); an
/// `import()`'s promise (`finish_import`, in a microtask if `deferred`); a
/// `modulepreload`'s link, `load` or `error` at it in a task.
fn settle(
    scope: &mut Scope<'_, '_, '_>,
    realm: &Realm,
    context: &v8::Global<v8::Context>,
    finished: Vec<Finished>,
    deferred: bool,
) {
    if finished.is_empty() {
        return;
    }
    let mut imports = Vec::new();
    {
        let (state, _) = scope.state_mut();
        let crate::state::State {
            dom, worker_realm, ..
        } = state;
        dom.mutate(|cx| {
            for done in finished {
                match done.target {
                    GraphFor::Worker => {
                        if let Some(worker) = worker_realm.as_mut() {
                            worker.graph = Some(done);
                        }
                    }
                    GraphFor::Script(key) => {
                        cx.module_graph_fetched(key, done.root.map(ModuleScriptId::index))
                    }
                    GraphFor::Preload(link) => {
                        let link = u32::try_from(link).ok().map(dom::NodeId::from_raw);
                        if let Some(link) = link.and_then(|link| cx.node_by_id(link)) {
                            let event = if done.root.is_some() {
                                names::event::LOAD
                            } else {
                                names::event::ERROR
                            };
                            cx.queue_element_task(link, event);
                        }
                    }
                    GraphFor::Import(_) => imports.push(done),
                }
            }
        });
    }
    for done in imports {
        if deferred {
            defer_import(scope, context, done);
        } else {
            finish_import(scope, realm, done);
        }
    }
    crate::state::report(scope);
}

/// HTML's "ContinueDynamicImport" for `done`, an `import()`'s graph, in
/// `realm`: rejected if a module failed to fetch (as Blink words it), else
/// linked now (each import's exception its own) and evaluated, its promise
/// settled with the namespace or what it threw.
fn finish_import(scope: &mut Scope<'_, '_, '_>, realm: &Realm, done: Finished) {
    let GraphFor::Import(import) = done.target else {
        return;
    };
    let Some((context, mut modules)) = take_modules(scope, realm) else {
        return;
    };
    let url = modules.imports.remove(&import);
    let evaluation = done.root.map(|root| {
        let scope = scope.scope();
        let local = v8::Local::new(scope, &context);
        let scope = &mut v8::ContextScope::new(scope, local);
        modules
            .records
            .dynamic_import_evaluation(scope, index(root), done.parse_error.map(index))
    });
    put_back(scope, realm, modules);
    // Its graph runs with the modules back in the realm: it may `import()`.
    let scope = scope.scope();
    let local = v8::Local::new(scope, &context);
    let scope = &mut v8::ContextScope::new(scope, local);
    match evaluation {
        Some(evaluation) => bindings::finish_dynamic_import(scope, import, &evaluation),
        None => {
            let message = format!(
                "Failed to fetch dynamically imported module: {}",
                url.as_ref().map_or("", Url::as_str)
            );
            let error = new_error(scope, ModuleParseError::Type(message));
            bindings::reject_dynamic_import(scope, import, error);
        }
    }
}

/// `finish_import` of `done` in a microtask of `context`'s: its graph was
/// in the map as `import()` ran, which must return first (HTML's
/// HostLoadImportedModule finishes at once; the evaluation is a promise
/// job's).
fn defer_import(scope: &mut Scope<'_, '_, '_>, context: &v8::Global<v8::Context>, done: Finished) {
    let GraphFor::Import(import) = done.target else {
        return;
    };
    let scope = scope.scope();
    let local = v8::Local::new(scope, context);
    let scope = &mut v8::ContextScope::new(scope, local);
    let number = |value: Option<ModuleScriptId>| -> v8::Local<'_, v8::Value> {
        match value {
            Some(module) => v8::Number::new(scope, f64::from(module.index())).into(),
            None => v8::undefined(scope).into(),
        }
    };
    let (root, parse_error) = (number(done.root), number(done.parse_error));
    let import = v8::Number::new(scope, import as f64);
    let data = v8::Array::new_with_elements(scope, &[import.into(), root, parse_error]);
    if let Some(function) = v8::Function::builder(continue_import)
        .data(data.into())
        .build(scope)
    {
        scope.enqueue_microtask(function);
    }
}

/// The microtask `defer_import` queued: its data the import's number, its
/// root's and its parse error's (undefined: none).
fn continue_import(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let Ok(data) = v8::Local::<v8::Array>::try_from(args.data()) else {
        return;
    };
    let module = |scope: &mut v8::PinScope<'_, '_>, at| {
        let value = data.get_index(scope, at)?;
        if value.is_undefined() {
            return None;
        }
        Some(ModuleScriptId::new(base::numerics::saturated_cast(
            value.number_value(scope)?,
        )))
    };
    let Some(import) = data
        .get_index(scope, 0)
        .and_then(|value| value.number_value(scope))
    else {
        return;
    };
    let done = Finished {
        target: GraphFor::Import(base::numerics::saturated_cast(import)),
        root: module(scope, 1),
        parse_error: module(scope, 2),
    };
    let current = scope.get_current_context();
    let Some(mut scope) = crate::state::scope(scope) else {
        return;
    };
    let Some(realm) = realm_of_context(&mut scope, current) else {
        return;
    };
    finish_import(&mut scope, &realm, done);
}

/// The realm `context` is: a frame's (by its document), or the worker's.
fn realm_of_context(
    scope: &mut Scope<'_, '_, '_>,
    context: v8::Local<'_, v8::Context>,
) -> Option<Realm> {
    let (frames, worker): (Vec<(v8::Global<v8::Context>, NodeRoot)>, _) = {
        let (state, _) = scope.state();
        (
            state
                .frames
                .iter()
                .map(|frame| (frame.context.clone(), frame.document.clone()))
                .collect(),
            state
                .worker_realm
                .as_ref()
                .map(|worker| worker.context.clone()),
        )
    };
    let scope = scope.scope();
    if worker.is_some_and(|worker| v8::Local::new(scope, &worker) == context) {
        return Some(Realm::Worker);
    }
    frames
        .into_iter()
        .find(|(frame, _)| v8::Local::new(scope, frame) == context)
        .map(|(_, document)| Realm::Document(document))
}

/// V8's `HostImportModuleDynamicallyCallback`: an `import()` ran (HTML's
/// `HostLoadImportedModule` for it, Blink's `V8Initializer`'s
/// `HostImportModuleDynamically`). Its promise waits in the realm's table;
/// the request resolves against its referencing script's base URL (the
/// document's or the worker's for none: an event handler's), its graph
/// fetched with that
/// script's fetch options' credentials mode (same-origin for none). An
/// invalid one rejects at once; a graph already in the map settles in a
/// microtask; others as their modules come.
pub(crate) fn import_dynamically<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    options: v8::Local<'s, v8::Data>,
    _resource_name: v8::Local<'s, v8::Value>,
    specifier: v8::Local<'s, v8::String>,
    attributes: v8::Local<'s, v8::FixedArray>,
) -> Option<v8::Local<'s, v8::Promise>> {
    let specifier = specifier.to_rust_string_lossy(scope);
    let base_url = bindings::referrer_base_url(scope, options);
    let credentials = match bindings::referrer_credentials(scope, options).as_deref() {
        Some("include") => ScriptCors::UseCredentials,
        Some("omit") => ScriptCors::Omit,
        _ => ScriptCors::Anonymous,
    };
    let attributes = import_attributes(scope, attributes);
    let current = scope.get_current_context();
    let realm = {
        let mut state_scope = crate::state::scope(scope)?;
        realm_of_context(&mut state_scope, current)
    };
    let Some(realm) = realm else {
        // A realm no frame shows.
        let resolver = v8::PromiseResolver::new(scope)?;
        let message = format!("Failed to fetch dynamically imported module: {specifier}");
        let error = new_error(scope, ModuleParseError::Type(message));
        resolver.reject(scope, error);
        return Some(resolver.get_promise(scope));
    };
    // Its realm's modules and table of promises, made at the first.
    {
        let mut state_scope = crate::state::scope(scope)?;
        let (_, modules) = take_modules(&mut state_scope, &realm)?;
        put_back(&mut state_scope, &realm, modules);
    }
    let (import, promise) = bindings::begin_dynamic_import(scope)?;
    let promise = v8::Local::new(scope, promise);
    let mut state_scope = crate::state::scope(scope)?;
    let base = base_url.or_else(|| {
        let (state, _) = state_scope.state();
        match &realm {
            Realm::Document(document) => state
                .dom
                .read(|cx| cx.document_base_url(document.get(cx.gc)).map(String::from)),
            Realm::Worker => {
                let global = &state.worker_realm.as_ref()?.global;
                Some(
                    state
                        .dom
                        .read(|cx| cx.worker_global_url(global.get(cx.gc)).to_string()),
                )
            }
        }
    });
    let resolved = base
        .as_deref()
        .and_then(|base| Url::parse(base).ok())
        .ok_or_else(|| {
            ModuleParseError::Type(format!(
                "Failed to fetch dynamically imported module: {specifier}"
            ))
        })
        .and_then(|base| dom_api::module::resolve_dynamic_request(&base, &specifier, &attributes));
    // Script asked for its graph, as it runs: its stack, if DevTools wants
    // it.
    let wanted = state_scope.state().0.dom.request_stacks_wanted();
    let root = wanted
        .then(|| crate::state::capture_stack(&mut state_scope))
        .flatten()
        .map_or(LoadInitiator::Other, LoadInitiator::Script);
    match resolved {
        Ok(url) => with_modules(
            &mut state_scope,
            &realm,
            true,
            root,
            |map, imports, host| {
                imports.insert(import, url.clone());
                map.fetch_tree(host, url, GraphFor::Import(import), credentials)
            },
        ),
        Err(error) => {
            let scope = state_scope.scope();
            let error = new_error(scope, error);
            bindings::reject_dynamic_import(scope, import, error);
        }
    }
    Some(promise)
}

/// An `import()`'s attributes, from V8's `[key, value, ...]`.
fn import_attributes(
    scope: &mut v8::PinScope<'_, '_>,
    array: v8::Local<'_, v8::FixedArray>,
) -> Vec<(String, String)> {
    let string = |scope: &mut v8::PinScope<'_, '_>, index| {
        array
            .get(scope, index)
            .and_then(|data| v8::Local::<v8::Value>::try_from(data).ok())
            .map(|value| value.to_rust_string_lossy(scope))
            .unwrap_or_default()
    };
    let keys = (0..array.length()).step_by(2);
    let values = (1..array.length()).step_by(2);
    keys.zip(values)
        .map(|(key, value)| (string(scope, key), string(scope, value)))
        .collect()
}

/// HTML's "fetch a module script graph" or "fetch an inline module script
/// graph" for `start`, of `document`'s realm.
pub(crate) fn start_graph(scope: &mut Scope<'_, '_, '_>, document: &NodeRoot, start: ModuleStart) {
    let base_url = {
        let (state, _) = scope.state();
        state
            .dom
            .read(|cx| cx.document_base_url(document.get(cx.gc)))
    };
    // Its script element's (the parser's that made it, or the script's
    // that inserted it).
    let root = {
        let (state, _) = scope.state();
        state
            .dom
            .pending_script_element(start.key)
            .map_or(LoadInitiator::Other, LoadInitiator::Element)
    };
    with_modules(scope, &Realm::Document(document.clone()), false, root, |map, _, host| {
        match start.source {
            ModuleSource::External(url) => match Url::parse(&url) {
                Ok(url) => {
                    map.fetch_tree(host, url, GraphFor::Script(start.key), start.credentials)
                }
                Err(_) => Vec::new(),
            },
            ModuleSource::Inline { source, url, line } => {
                // Its base URL the document's.
                let Some(base) = base_url else {
                    return Vec::new();
                };
                let url = Url::parse(&url).unwrap_or_else(|_| base.clone());
                map.fetch_inline_tree(
                    host,
                    &source,
                    &url,
                    &base,
                    line,
                    start.key,
                    start.credentials,
                )
            }
        }
    });
}

/// HTML's processing of a `<link rel=modulepreload>` (Blink's
/// `PreloadHelper::ModulePreloadIfNeeded`): its `href` resolved against
/// its document's base URL (none, or one that doesn't parse: nothing); its `as`
/// a destination (`script` for none), `error` if it isn't script-like
/// (machopo has only JavaScript modules: no JSON or CSS ones); else the module
/// alone fetched into the map with `crossorigin`'s credentials mode, its
/// `load` or `error` after.
pub(crate) fn preload(scope: &mut Scope<'_, '_, '_>, link: &NodeRoot) {
    let found = {
        let (state, _) = scope.state_mut();
        state.dom.mutate(|cx| {
            let element = link.get(cx.gc);
            let attr = |name| {
                cx.attribute_value_by_id(element, name)
                    .map(|value| value.borrow().to_str().into_owned())
            };
            let document = cx.document_of(element);
            let base = cx.document_base_url(document)?;
            let href = attr(names::local::HREF).filter(|href| !href.is_empty())?;
            let url = base.join(&href).ok()?;
            let destination = attr(names::local::AS)
                .and_then(|value| destination(&value))
                .unwrap_or("script");
            if !is_script_like(destination) {
                cx.queue_element_task(element, names::event::ERROR);
                return None;
            }
            let credentials = match attr(names::local::CROSSORIGIN) {
                Some(value) if value.eq_ignore_ascii_case("use-credentials") => {
                    ScriptCors::UseCredentials
                }
                _ => ScriptCors::Anonymous,
            };
            Some((
                cx.root(document),
                url,
                credentials,
                u64::from(cx.node_id(element).get()),
            ))
        })
    };
    // The link's (the parser's, or the script's that connected it).
    let root = {
        let (state, _) = scope.state();
        LoadInitiator::Element(state.dom.read(|cx| cx.node_id(link.get(cx.gc))))
    };
    let Some((document, url, credentials, link)) = found else {
        return;
    };
    with_modules(scope, &Realm::Document(document), false, root, |map, _, host| {
        map.fetch_tree(host, url, GraphFor::Preload(link), credentials)
    });
}

/// The destination a `<link>`'s `as` names (its keyword), none for a
/// value that names none (HTML's "no state").
fn destination(value: &str) -> Option<&'static str> {
    const KEYWORDS: &[&str] = &[
        "audio",
        "audioworklet",
        "document",
        "embed",
        "fetch",
        "font",
        "frame",
        "iframe",
        "image",
        "json",
        "manifest",
        "object",
        "paintworklet",
        "report",
        "script",
        "serviceworker",
        "sharedworker",
        "style",
        "text",
        "track",
        "video",
        "webidentity",
        "worker",
        "xslt",
    ];
    KEYWORDS
        .iter()
        .find(|keyword| keyword.eq_ignore_ascii_case(value))
        .copied()
}

/// Fetch's script-like destinations.
fn is_script_like(destination: &str) -> bool {
    matches!(
        destination,
        "audioworklet" | "paintworklet" | "script" | "serviceworker" | "sharedworker" | "worker"
    )
}

/// A module a realm asked for, fetched (`None`: it failed): its graphs
/// move on. Whether one of its fetches was `key`.
pub(crate) fn module_fetched(
    scope: &mut Scope<'_, '_, '_>,
    key: u64,
    response: Option<FetchedModule>,
) -> bool {
    let realm = {
        let (state, _) = scope.state();
        let fetching =
            |modules: &Option<Box<RealmModules>>| modules.as_ref().is_some_and(|modules| modules.map.is_fetching(key));
        match &state.worker_realm {
            Some(worker) if fetching(&worker.modules) => Some(Realm::Worker),
            _ => state
                .frames
                .iter()
                .find(|frame| fetching(&frame.modules))
                .map(|frame| Realm::Document(frame.document.clone())),
        }
    };
    let Some(realm) = realm else {
        return false;
    };
    // A root is fetched as its graph starts: what comes now is imported.
    with_modules(
        scope,
        &realm,
        false,
        LoadInitiator::Other,
        |map, _, host| map.fetched(host, key, response).unwrap_or_default(),
    );
    true
}

/// HTML's "run a module script" for the graph `root`, in `realm`: its
/// evaluation's promise's rejection (what it threw, or its error to
/// rethrow) reported at the global, which handles the promise (Blink's
/// `ModuleEvaluationRejectionCallback`).
pub(crate) fn run_module_script(scope: &mut Scope<'_, '_, '_>, realm: &Realm, root: u32) {
    let found = {
        let (state, _) = scope.state();
        let realm = match realm {
            Realm::Document(document) => {
                let id = state.dom.read(|cx| cx.node_id(document.get(cx.gc)));
                state
                    .frames
                    .of_document(id)
                    .map(|frame| (&frame.context, frame.modules.as_ref()))
            }
            Realm::Worker => state
                .worker_realm
                .as_ref()
                .map(|worker| (&worker.context, worker.modules.as_ref())),
        };
        realm.and_then(|(context, modules)| {
            Some((
                context.clone(),
                modules?.records.evaluation(index(ModuleScriptId::new(root))),
            ))
        })
    };
    let Some((context, evaluation)) = found else {
        return;
    };
    let scope = scope.scope();
    let context = v8::Local::new(scope, &context);
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(promise) = evaluation.evaluate(scope) else {
        return;
    };
    let Some(report) = v8::Function::new(scope, report_rejection) else {
        return;
    };
    // `None` only if script was terminated meanwhile.
    let _ = promise.catch(scope, report);
}

fn report_rejection(
    scope: &mut v8::PinScope<'_, '_>,
    args: v8::FunctionCallbackArguments,
    _: v8::ReturnValue,
) {
    let reason = args.get(0);
    let global = scope.get_current_context().global(scope);
    crate::listeners::report_value(scope, global, reason);
}

fn id(module: usize) -> ModuleScriptId {
    ModuleScriptId::new(u32::try_from(module).expect("fewer modules than that"))
}

fn index(module: ModuleScriptId) -> usize {
    usize::try_from(module.index()).expect("a module's number")
}
