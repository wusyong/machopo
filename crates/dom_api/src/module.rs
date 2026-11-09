//! A realm's module system, the engine-free half (Blink's `Modulator`,
//! `ModuleMap` and `ModuleTreeLinker`, which a `ScriptState` has): fetches
//! each module graph a module script needs, once per URL, resolving
//! specifiers as HTML says; the engine compiles, links and runs (reached
//! through a [`ModuleHost`]), its host fetches.
//!
//! One per realm (HTML's module map is per realm), the engine's to keep.
//! Everything here is numbers and URLs: the modules themselves are the
//! engine's.

use std::collections::{HashMap, HashSet, VecDeque};

use base::SequenceNumber;

use dom::ScriptCors as CredentialsMode;
use url::Url;

/// A module of a realm, by the engine's number for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ModuleScriptId(u32);

impl ModuleScriptId {
    /// For engines: the module's number.
    pub fn new(index: u32) -> Self {
        Self(index)
    }

    /// For engines: the number back.
    pub fn index(self) -> u32 {
        self.0
    }
}

/// What compiling a module gave ([`ModuleHost::compile`]): its number, and
/// what its `import`s and `export … from`s request, in source order, each
/// once (ECMAScript's [[RequestedModules]]); `None` if it doesn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompiledModule {
    pub id: ModuleScriptId,
    pub requests: Option<Vec<ModuleRequest>>,
}

/// One of a module's requests (ECMAScript's ModuleRequest Record): what to
/// import, as written, with its import attributes (keys and values, in
/// source order).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleRequest {
    pub specifier: String,
    pub attributes: Vec<(String, String)>,
}

/// The parse error HTML gives a module whose request it can't take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModuleParseError {
    /// A `SyntaxError`: an import attribute the host doesn't support.
    Syntax(String),
    /// A `TypeError`: a specifier that doesn't resolve, a module type machopo
    /// hasn't.
    Type(String),
}

/// What the module system needs done outside it: fetching, and the
/// engine's part (in the document's realm).
pub trait ModuleHost {
    /// Starts fetching the module at `url`, by CORS with `credentials`;
    /// the request's id, whose response comes back through
    /// [`Modulator::fetched`]. `delays_load`: whether the document's
    /// `load` waits for it (a script's graph's modules; an `import()`'s
    /// don't, as in Blink). `importer`: the module whose `import` it is,
    /// `None` for a graph's root.
    fn fetch(
        &mut self,
        url: &Url,
        delays_load: bool,
        credentials: CredentialsMode,
        importer: Option<&Url>,
    ) -> u64;
    /// Compiles a module (its URL, base URL, first line), whose fetch
    /// options are `credentials` (what an `import()` in it fetches with).
    fn compile(
        &mut self,
        source: &str,
        url: &Url,
        base_url: &Url,
        line: u32,
        credentials: CredentialsMode,
    ) -> CompiledModule;
    /// `ScriptEngine::set_module_parse_error`.
    fn set_parse_error(&mut self, module: ModuleScriptId, error: ModuleParseError);
    /// `ScriptEngine::set_module_requests`.
    fn set_requests(&mut self, module: ModuleScriptId, resolved: Vec<ModuleScriptId>);
    /// `ScriptEngine::link_module`.
    fn link(&mut self, root: ModuleScriptId, parse_error: Option<ModuleScriptId>);
}

/// A module's response, fetched and checked (an ok status, a JavaScript
/// MIME type), decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedModule {
    /// The response's URL (after redirects): the module's base URL.
    pub url: Url,
    pub source: String,
}

/// Who a graph is fetched for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphFor {
    /// A module script, by its pending script's number (`Cx::
    /// queue_module_script`).
    Script(u64),
    /// An `import()`, by its number in the realm.
    Import(u64),
    /// A `<link rel=modulepreload>`, by its number in the frame: HTML's
    /// "fetch a single module script", the module alone, as Blink's
    /// `ModulePreloadIfNeeded` (`Modulator::FetchSingle`); its descendants
    /// are fetched when a graph needs them.
    Preload(u64),
}

/// A graph whose fetch has finished, for who asked for it: its root,
/// linked (or with an error to rethrow), or `None` if a module of it
/// failed to fetch (the script's `error` event, the import's `TypeError`).
/// A preload's is its module alone, neither linked nor checked for
/// errors: one with a parse error has still been fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Finished {
    pub target: GraphFor,
    pub root: Option<ModuleScriptId>,
    /// The graph's first parse error, if any: what a script's graph has
    /// as its error to rethrow, and an `import()` rejects with.
    pub parse_error: Option<ModuleScriptId>,
}

/// A module script (HTML's, without the record the engine keeps).
#[derive(Debug)]
struct ModuleScript {
    /// Where it is: its imports' importer.
    url: Url,
    /// The URL each request resolved to, in the order of the engine's
    /// requests; `None` if it has a parse error (whose descendants aren't
    /// fetched).
    children: Option<Vec<Url>>,
    /// The engine knows what its requests resolved to.
    requests_set: bool,
}

/// An entry of the module map (Blink's `ModuleMap::Entry`). One that
/// failed to fetch is removed (HTML's "fetch a single module script", as
/// Blink's `ModuleMapDoNotCacheFailedFetch`): those waiting for it fail,
/// a later request fetches it again.
#[derive(Debug)]
enum Entry {
    /// The linkers waiting for it.
    Fetching(Vec<u64>),
    Fetched(ModuleScriptId),
}

/// One graph being fetched (Blink's `ModuleTreeLinker`), for one script
/// or `import()`.
#[derive(Debug)]
struct Linker {
    target: GraphFor,
    /// The credentials mode of its fetches (HTML's fetch options, which
    /// the descendants take from the root's).
    credentials: CredentialsMode,
    /// `None` until the root is fetched.
    root: Option<ModuleScriptId>,
    /// The URLs it has asked for (HTML's visited set), so a cycle, or a
    /// module two others import, is fetched once.
    visited: HashSet<Url>,
    /// Modules it asked for that haven't arrived.
    incomplete: usize,
}

/// The module system of a document.
#[derive(Debug, Default)]
pub struct Modulator {
    /// HTML's module map, by request URL (the only module type is
    /// JavaScript's).
    map: HashMap<Url, Entry>,
    modules: HashMap<ModuleScriptId, ModuleScript>,
    linkers: HashMap<u64, Linker>,
    linker_ids: SequenceNumber<u64>,
    /// The URL each module request in flight is for, and its fetch
    /// options (the first linker's: the module's).
    fetches: HashMap<u64, (Url, CredentialsMode)>,
    /// Modules a linker waits for that have arrived (none: failed), to
    /// hand it in order (not from inside another linker's step).
    arrived: VecDeque<(u64, Option<ModuleScriptId>)>,
}

impl Modulator {
    /// HTML "fetch a module script graph" for an external module script,
    /// or an `import()`'s: `target` is told (in what this or a later call
    /// returns) once the graph under `url` has been fetched. A preload is
    /// told once the module at `url` has. Its modules are fetched by CORS
    /// with `credentials`.
    pub fn fetch_tree(
        &mut self,
        host: &mut dyn ModuleHost,
        url: Url,
        target: GraphFor,
        credentials: CredentialsMode,
    ) -> Vec<Finished> {
        let linker = self.new_linker(target, credentials);
        let entry = self.linkers.get_mut(&linker).expect("just made");
        entry.visited.insert(url.clone());
        entry.incomplete = 1;
        self.fetch_single(host, url, linker, None);
        self.run(host)
    }

    /// HTML "fetch an inline module script graph": `source`, in the
    /// document at `url` (from its line `line`), whose base URL is
    /// `base_url`; its descendants are fetched by CORS with `credentials`.
    #[expect(clippy::too_many_arguments, reason = "HTML's own: each is the graph's")]
    pub fn fetch_inline_tree(
        &mut self,
        host: &mut dyn ModuleHost,
        source: &str,
        url: &Url,
        base_url: &Url,
        line: u32,
        script: u64,
        credentials: CredentialsMode,
    ) -> Vec<Finished> {
        let module = self.create(host, source, url, base_url, line, credentials);
        let linker = self.new_linker(GraphFor::Script(script), credentials);
        self.linkers.get_mut(&linker).expect("just made").root = Some(module);
        self.fetch_descendants(host, linker, module);
        let done = self.check_done(host, linker);
        let mut finished: Vec<Finished> = done.into_iter().collect();
        finished.extend(self.run(host));
        finished
    }

    /// The module `request_id` asked for has arrived (`None` if it failed
    /// to fetch: it leaves the map): it goes in the map, and every graph
    /// waiting for it moves on. Returns false if it isn't one of its
    /// fetches.
    pub fn fetched(
        &mut self,
        host: &mut dyn ModuleHost,
        request_id: u64,
        response: Option<FetchedModule>,
    ) -> Option<Vec<Finished>> {
        let (url, credentials) = self.fetches.remove(&request_id)?;
        let module = response.map(|module| {
            self.create(
                host,
                &module.source,
                &module.url,
                &module.url,
                1,
                credentials,
            )
        });
        let entry = match module {
            Some(module) => std::mem::replace(
                self.map.get_mut(&url).expect("fetching"),
                Entry::Fetched(module),
            ),
            None => self.map.remove(&url).expect("fetching"),
        };
        let Entry::Fetching(waiting) = entry else {
            panic!("fetched twice: {url}");
        };
        self.arrived
            .extend(waiting.into_iter().map(|linker| (linker, module)));
        Some(self.run(host))
    }

    /// Whether `request_id` is one of its fetches on its way.
    pub fn is_fetching(&self, request_id: u64) -> bool {
        self.fetches.contains_key(&request_id)
    }

    fn new_linker(&mut self, target: GraphFor, credentials: CredentialsMode) -> u64 {
        let id = self.linker_ids.get_next();
        self.linkers.insert(
            id,
            Linker {
                target,
                credentials,
                root: None,
                visited: HashSet::new(),
                incomplete: 0,
            },
        );
        id
    }

    /// HTML "fetch a single module script", for `linker`: from the map if
    /// it is there (or on its way), else fetched.
    fn fetch_single(
        &mut self,
        host: &mut dyn ModuleHost,
        url: Url,
        linker: u64,
        importer: Option<&Url>,
    ) {
        match self.map.get_mut(&url) {
            Some(Entry::Fetching(waiting)) => waiting.push(linker),
            Some(Entry::Fetched(module)) => self.arrived.push_back((linker, Some(*module))),
            None => {
                let entry = &self.linkers[&linker];
                let delays_load = matches!(entry.target, GraphFor::Script(_));
                let request_id = host.fetch(&url, delays_load, entry.credentials, importer);
                self.fetches
                    .insert(request_id, (url.clone(), entry.credentials));
                self.map.insert(url, Entry::Fetching(vec![linker]));
            }
        }
    }

    /// HTML "create a JavaScript module script": compiled, with each
    /// request resolved, or a parse error.
    fn create(
        &mut self,
        host: &mut dyn ModuleHost,
        source: &str,
        url: &Url,
        base_url: &Url,
        line: u32,
        credentials: CredentialsMode,
    ) -> ModuleScriptId {
        let compiled = host.compile(source, url, base_url, line, credentials);
        let children = compiled.requests.and_then(|requests| {
            let resolved = requests
                .iter()
                .map(|request| resolve_request(base_url, &request.specifier, &request.attributes))
                .collect::<Result<Vec<_>, _>>();
            match resolved {
                Ok(children) => Some(children),
                Err(error) => {
                    host.set_parse_error(compiled.id, error);
                    None
                }
            }
        });
        self.modules.insert(
            compiled.id,
            ModuleScript {
                url: url.clone(),
                children,
                requests_set: false,
            },
        );
        compiled.id
    }

    /// Hands the modules that have arrived to the linkers waiting for
    /// them, until none is left; the graphs that finished.
    fn run(&mut self, host: &mut dyn ModuleHost) -> Vec<Finished> {
        let mut finished = Vec::new();
        while let Some((linker, module)) = self.arrived.pop_front() {
            let Some(entry) = self.linkers.get_mut(&linker) else {
                // It failed already.
                continue;
            };
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "a module that arrives is one the linker asked for and counted"
            )]
            {
                entry.incomplete -= 1;
            }
            let Some(module) = module else {
                // HTML: the graph's result is null.
                let target = self.linkers.remove(&linker).expect("found").target;
                finished.push(Finished {
                    target,
                    root: None,
                    parse_error: None,
                });
                continue;
            };
            if let target @ GraphFor::Preload(_) = entry.target {
                // Its event doesn't wait for the descendants (HTML's
                // "modulepreload").
                self.linkers.remove(&linker);
                finished.push(Finished {
                    target,
                    root: Some(module),
                    parse_error: None,
                });
                continue;
            }
            entry.root.get_or_insert(module);
            self.fetch_descendants(host, linker, module);
            if let Some(done) = self.check_done(host, linker) {
                finished.push(done);
            }
        }
        finished
    }

    /// HTML "fetch the descendants of a module script", for `linker`: the
    /// ones it hasn't asked for yet.
    fn fetch_descendants(
        &mut self,
        host: &mut dyn ModuleHost,
        linker: u64,
        module: ModuleScriptId,
    ) {
        let Some(children) = self.modules[&module].children.clone() else {
            return;
        };
        let importer = self.modules[&module].url.clone();
        for child in children {
            let entry = self.linkers.get_mut(&linker).expect("a linker");
            if entry.visited.insert(child.clone()) {
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "one per module of the graph, each in memory: fewer than usize::MAX"
                )]
                {
                    entry.incomplete += 1;
                }
                self.fetch_single(host, child, linker, Some(&importer));
            }
        }
    }

    /// Once nothing `linker` asked for is on its way (and none failed):
    /// HTML's "find the first parse error", then, for a script, the link
    /// (Blink's `ModuleTreeLinker::Instantiate`); an `import()` links the
    /// graph itself when the engine finishes it ("ContinueDynamicImport"),
    /// so each import that fails to link gets an exception of its own.
    fn check_done(&mut self, host: &mut dyn ModuleHost, linker: u64) -> Option<Finished> {
        if self.linkers.get(&linker)?.incomplete > 0 {
            return None;
        }
        let Linker { target, root, .. } = self.linkers.remove(&linker).expect("found");
        let root = root.expect("a graph that has arrived has its root");
        let graph = self.graph(root);
        let parse_error = graph
            .iter()
            .copied()
            .find(|module| self.modules[module].children.is_none());
        if parse_error.is_none() {
            for &module in &graph {
                self.set_requests(host, module);
            }
        }
        if let GraphFor::Script(_) = target {
            host.link(root, parse_error);
        }
        Some(Finished {
            target,
            root: Some(root),
            parse_error,
        })
    }

    /// The modules of the graph under `root`, in HTML's "find the first
    /// parse error" order: depth first, each request in order, each module
    /// once; a module with a parse error has no descendants.
    fn graph(&self, root: ModuleScriptId) -> Vec<ModuleScriptId> {
        let mut order = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = vec![root];
        while let Some(module) = stack.pop() {
            // Marked when visited, not when pushed: the order of a
            // recursive walk.
            if !seen.insert(module) {
                continue;
            }
            order.push(module);
            let children = self.modules[&module].children.iter().flatten();
            // Pushed in reverse, so the first request comes out first.
            for child in children.rev() {
                let Some(Entry::Fetched(child)) = self.map.get(child) else {
                    unreachable!("a graph that has arrived has every module");
                };
                stack.push(*child);
            }
        }
        order
    }

    /// Tells the engine what `module`'s requests resolved to, once.
    fn set_requests(&mut self, host: &mut dyn ModuleHost, module: ModuleScriptId) {
        let script = &self.modules[&module];
        if script.requests_set {
            return;
        }
        let children = script.children.as_ref().expect("no parse error");
        let resolved = children
            .iter()
            .map(|child| match self.map.get(child) {
                Some(Entry::Fetched(child)) => *child,
                _ => unreachable!("a graph that has arrived has every module"),
            })
            .collect();
        host.set_requests(module, resolved);
        self.modules.get_mut(&module).expect("found").requests_set = true;
    }
}

/// The URL a module's request is for, or the parse error it gives: HTML
/// "create a JavaScript module script"'s checks of each request, and
/// "resolve a module specifier" (without import maps).
pub fn resolve_request(
    base_url: &Url,
    specifier: &str,
    attributes: &[(String, String)],
) -> Result<Url, ModuleParseError> {
    if let Some((key, _)) = attributes.iter().find(|(key, _)| key != "type") {
        return Err(ModuleParseError::Syntax(format!(
            "Import attribute \"{key}\" is not supported"
        )));
    }
    let url = resolve_module_specifier(base_url, specifier).ok_or_else(|| {
        ModuleParseError::Type(format!(
            "Failed to resolve module specifier \"{specifier}\". Relative references must start with either \"/\", \"./\", or \"../\"."
        ))
    })?;
    // Only JavaScript modules: `type` asks for another (JSON, CSS), which
    // HTML would fetch as such. (`javascript` itself isn't a type to ask
    // for.)
    if let Some((_, module_type)) = attributes.first() {
        return Err(ModuleParseError::Type(format!(
            "Module type \"{module_type}\" is not supported"
        )));
    }
    Ok(url)
}

/// [`resolve_request`] for an `import()`: an attribute other than `type`
/// is a `TypeError` there (ECMAScript's `EvaluateImportCall`), not the
/// `SyntaxError` of a static import's.
pub fn resolve_dynamic_request(
    base_url: &Url,
    specifier: &str,
    attributes: &[(String, String)],
) -> Result<Url, ModuleParseError> {
    resolve_request(base_url, specifier, attributes).map_err(|error| match error {
        ModuleParseError::Syntax(message) => ModuleParseError::Type(message),
        error => error,
    })
}

/// HTML "resolve a URL-like module specifier": a relative one starts with
/// `/`, `./` or `../`; anything else must be an absolute URL (a bare
/// specifier, `lodash`, needs an import map).
pub fn resolve_module_specifier(base_url: &Url, specifier: &str) -> Option<Url> {
    if ["/", "./", "../"].iter().any(|p| specifier.starts_with(p)) {
        return base_url.join(specifier).ok();
    }
    Url::parse(specifier).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAME: CredentialsMode = CredentialsMode::Anonymous;

    /// A host whose "engine" reads a module's requests off its source: one
    /// specifier per line, `!` for a syntax error; and records the rest.
    #[derive(Default)]
    struct Host {
        fetched: Vec<Url>,
        /// Each fetch's importer.
        importers: Vec<Option<Url>>,
        /// Whether each fetch delays the document's `load`.
        delays_load: Vec<bool>,
        /// Each fetch's credentials mode.
        credentials: Vec<CredentialsMode>,
        compiled: Vec<(Url, Url, u32)>,
        /// Each compiled module's fetch options.
        compiled_credentials: Vec<CredentialsMode>,
        parse_errors: Vec<(u32, ModuleParseError)>,
        requests: Vec<(u32, Vec<u32>)>,
        links: Vec<(u32, Option<u32>)>,
        next_module: u32,
    }

    impl ModuleHost for Host {
        fn fetch(
            &mut self,
            url: &Url,
            delays_load: bool,
            credentials: CredentialsMode,
            importer: Option<&Url>,
        ) -> u64 {
            self.importers.push(importer.cloned());
            self.fetched.push(url.clone());
            self.delays_load.push(delays_load);
            self.credentials.push(credentials);
            100 + self.fetched.len() as u64
        }

        fn compile(
            &mut self,
            source: &str,
            url: &Url,
            base_url: &Url,
            line: u32,
            credentials: CredentialsMode,
        ) -> CompiledModule {
            self.compiled.push((url.clone(), base_url.clone(), line));
            self.compiled_credentials.push(credentials);
            let id = ModuleScriptId::new(self.next_module);
            self.next_module += 1;
            let requests = (!source.contains('!')).then(|| {
                source
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .map(|line| {
                        let mut parts = line.split_whitespace();
                        let specifier = parts.next().unwrap().to_owned();
                        let attributes = parts
                            .map(|kv| {
                                let (k, v) = kv.split_once('=').unwrap();
                                (k.to_owned(), v.to_owned())
                            })
                            .collect();
                        ModuleRequest {
                            specifier,
                            attributes,
                        }
                    })
                    .collect()
            });
            CompiledModule { id, requests }
        }

        fn set_parse_error(&mut self, module: ModuleScriptId, error: ModuleParseError) {
            self.parse_errors.push((module.index(), error));
        }

        fn set_requests(&mut self, module: ModuleScriptId, resolved: Vec<ModuleScriptId>) {
            self.requests
                .push((module.index(), resolved.iter().map(|m| m.index()).collect()));
        }

        fn link(&mut self, root: ModuleScriptId, parse_error: Option<ModuleScriptId>) {
            self.links
                .push((root.index(), parse_error.map(ModuleScriptId::index)));
        }
    }

    fn url(path: &str) -> Url {
        Url::parse("https://a.test/dir/")
            .unwrap()
            .join(path)
            .unwrap()
    }

    /// Answers the fetch of `path` with `source` (`None`: it failed).
    fn answer(
        modulator: &mut Modulator,
        host: &mut Host,
        path: &str,
        source: Option<&str>,
    ) -> Vec<Finished> {
        let index = host
            .fetched
            .iter()
            .position(|u| *u == url(path))
            .unwrap_or_else(|| panic!("{path} wasn't fetched"));
        let response = source.map(|source| FetchedModule {
            url: url(path),
            source: source.into(),
        });
        modulator
            .fetched(host, 101 + index as u64, response)
            .expect("one of its fetches")
    }

    fn done(script: u64, root: u32) -> Finished {
        Finished {
            target: GraphFor::Script(script),
            root: Some(ModuleScriptId::new(root)),
            parse_error: None,
        }
    }

    #[test]
    fn a_graph_finishes_once_every_module_has_arrived() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        assert_eq!(
            modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(1), SAME),
            []
        );
        assert_eq!(host.fetched, [url("main.mjs")]);
        assert_eq!(
            answer(
                &mut modulator,
                &mut host,
                "main.mjs",
                Some("./a.mjs\n../b.mjs\n./a.mjs")
            ),
            []
        );
        assert_eq!(host.fetched, [url("main.mjs"), url("a.mjs"), url("/b.mjs")]);
        // The root has no importer; the others, the module importing them.
        assert_eq!(
            host.importers,
            [None, Some(url("main.mjs")), Some(url("main.mjs"))]
        );
        assert_eq!(answer(&mut modulator, &mut host, "/b.mjs", Some("")), []);
        assert_eq!(
            answer(&mut modulator, &mut host, "a.mjs", Some("/b.mjs")),
            [done(1, 0)]
        );
        // main is 0, b is 1, a is 2; every module's requests, then the link.
        assert_eq!(
            host.requests,
            [(0, vec![2, 1, 2]), (2, vec![1]), (1, vec![])]
        );
        assert_eq!(host.links, [(0, None)]);
        assert!(host.parse_errors.is_empty());
    }

    #[test]
    fn modules_are_fetched_once_and_shared() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("a.mjs"), GraphFor::Script(1), SAME);
        // A second graph of the same root while it is on its way.
        assert_eq!(
            modulator.fetch_tree(&mut host, url("a.mjs"), GraphFor::Script(2), SAME),
            []
        );
        assert_eq!(host.fetched.len(), 1);
        assert_eq!(
            answer(&mut modulator, &mut host, "a.mjs", Some("./shared.mjs")),
            []
        );
        let finished = answer(&mut modulator, &mut host, "shared.mjs", Some(""));
        assert_eq!(finished, [done(1, 0), done(2, 0)]);
        // A third, once everything is in the map: done at once, fetching
        // nothing; requests are set once.
        assert_eq!(
            modulator.fetch_tree(&mut host, url("a.mjs"), GraphFor::Script(3), SAME),
            [done(3, 0)]
        );
        assert_eq!(host.fetched, [url("a.mjs"), url("shared.mjs")]);
        assert_eq!(host.requests, [(0, vec![1]), (1, vec![])]);
        assert_eq!(host.links, [(0, None), (0, None), (0, None)]);
    }

    #[test]
    fn a_cycle_is_fetched_once() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("a.mjs"), GraphFor::Script(1), SAME);
        answer(&mut modulator, &mut host, "a.mjs", Some("./b.mjs"));
        let finished = answer(&mut modulator, &mut host, "b.mjs", Some("./a.mjs"));
        assert_eq!(finished, [done(1, 0)]);
        assert_eq!(host.fetched, [url("a.mjs"), url("b.mjs")]);
        assert_eq!(host.requests, [(0, vec![1]), (1, vec![0])]);
    }

    /// A module that fails to fetch fails the graph (no link), at once,
    /// even with others still on their way, and every graph waiting for it
    /// (fetched once); the map forgets the failure: a later graph fetches
    /// it again.
    #[test]
    fn a_failed_fetch_fails_the_graph() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(1), SAME);
        answer(
            &mut modulator,
            &mut host,
            "main.mjs",
            Some("./gone.mjs\n./slow.mjs"),
        );
        assert_eq!(
            modulator.fetch_tree(&mut host, url("gone.mjs"), GraphFor::Script(2), SAME),
            []
        );
        let failed = |script| Finished {
            target: GraphFor::Script(script),
            root: None,
            parse_error: None,
        };
        assert_eq!(
            answer(&mut modulator, &mut host, "gone.mjs", None),
            [failed(1), failed(2)]
        );
        assert_eq!(answer(&mut modulator, &mut host, "slow.mjs", Some("")), []);
        assert!(host.links.is_empty());
        assert_eq!(host.fetched.len(), 3);
        // Another graph with the failed module fetches it again.
        assert_eq!(
            modulator.fetch_tree(&mut host, url("gone.mjs"), GraphFor::Script(3), SAME),
            []
        );
        assert_eq!(host.fetched.len(), 4);
        let again = modulator
            .fetched(
                &mut host,
                104,
                Some(FetchedModule {
                    url: url("gone.mjs"),
                    source: String::new(),
                }),
            )
            .expect("one of its fetches");
        assert_eq!(again, [done(3, 2)]);
    }

    /// A module's fetch options are its fetch's: the graph's that asked for
    /// it first.
    #[test]
    fn a_module_has_its_fetchs_credentials() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        let include = CredentialsMode::UseCredentials;
        modulator.fetch_tree(&mut host, url("a.mjs"), GraphFor::Script(1), include);
        modulator.fetch_tree(&mut host, url("a.mjs"), GraphFor::Script(2), SAME);
        answer(&mut modulator, &mut host, "a.mjs", Some("./b.mjs"));
        answer(&mut modulator, &mut host, "b.mjs", Some(""));
        assert_eq!(host.compiled_credentials, [include, include]);
    }

    /// The first parse error, depth first in request order, is the
    /// graph's; a module with one has no descendants fetched; nothing's
    /// requests are set.
    #[test]
    fn the_first_parse_error_is_the_graphs() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(1), SAME);
        answer(
            &mut modulator,
            &mut host,
            "main.mjs",
            Some("./a.mjs\n./b.mjs"),
        );
        answer(&mut modulator, &mut host, "a.mjs", Some("./c.mjs"));
        answer(&mut modulator, &mut host, "b.mjs", Some("! ./never.mjs"));
        // main 0, a 1, b 2, c 3: c comes before b, depth first.
        let finished = answer(&mut modulator, &mut host, "c.mjs", Some("lodash"));
        assert_eq!(
            finished,
            [Finished {
                parse_error: Some(ModuleScriptId::new(3)),
                ..done(1, 0)
            }]
        );
        assert_eq!(host.links, [(0, Some(3))]);
        assert!(!host.fetched.contains(&url("never.mjs")));
        assert!(host.requests.is_empty());
        assert!(matches!(
            &host.parse_errors[..],
            [(3, ModuleParseError::Type(_))]
        ));
    }

    /// Depth first: a module a later sibling imports too comes in the
    /// subtree that reaches it first, whichever arrived first.
    #[test]
    fn the_first_parse_error_is_depth_first() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(1), SAME);
        answer(
            &mut modulator,
            &mut host,
            "main.mjs",
            Some(
                "./a.mjs
./c.mjs",
            ),
        );
        answer(
            &mut modulator,
            &mut host,
            "a.mjs",
            Some(
                "./c.mjs
./d.mjs",
            ),
        );
        // main 0, a 1, d 2, c 3; the walk goes main, a, c, d.
        answer(&mut modulator, &mut host, "d.mjs", Some("!"));
        let finished = answer(&mut modulator, &mut host, "c.mjs", Some("!"));
        assert_eq!(
            finished,
            [Finished {
                parse_error: Some(ModuleScriptId::new(3)),
                ..done(1, 0)
            }]
        );
        assert_eq!(host.links, [(0, Some(3))]);
    }

    #[test]
    fn requests_are_checked_and_resolved_as_html_says() {
        let base = url("m.mjs");
        let resolve = |specifier: &str, attributes: &[(&str, &str)]| {
            let attributes: Vec<(String, String)> = attributes
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            resolve_request(&base, specifier, &attributes)
        };
        assert_eq!(resolve("./x.mjs", &[]), Ok(url("x.mjs")));
        assert_eq!(resolve("../x.mjs", &[]), Ok(url("/x.mjs")));
        assert_eq!(resolve("/x.mjs", &[]), Ok(url("/x.mjs")));
        assert_eq!(
            resolve("https://b.test/x.mjs", &[]),
            Ok(Url::parse("https://b.test/x.mjs").unwrap())
        );
        for bare in ["x.mjs", "lodash", ".x.mjs", ""] {
            assert!(
                matches!(resolve(bare, &[]), Err(ModuleParseError::Type(_))),
                "{bare:?}"
            );
        }
        assert!(matches!(
            resolve("./x.json", &[("type", "json")]),
            Err(ModuleParseError::Type(_))
        ));
        // An unknown key is checked first, even for a bare specifier.
        assert!(matches!(
            resolve("lodash", &[("type", "json"), ("x", "y")]),
            Err(ModuleParseError::Syntax(_))
        ));
        // An `import()`'s is a `TypeError`.
        let attributes = [("x".to_owned(), "y".to_owned())];
        assert!(matches!(
            resolve_dynamic_request(&base, "./x.mjs", &attributes),
            Err(ModuleParseError::Type(_))
        ));
        assert_eq!(
            resolve_dynamic_request(&base, "./x.mjs", &[]),
            Ok(url("x.mjs"))
        );
    }

    /// An inline module script: compiled with the document's URL and line
    /// and its base URL; done at once without imports.
    #[test]
    fn an_inline_tree() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        let document = url("page.html");
        let base = Url::parse("https://b.test/base/").unwrap();
        assert_eq!(
            modulator.fetch_inline_tree(&mut host, "", &document, &base, 7, 1, SAME),
            [done(1, 0)]
        );
        assert_eq!(host.compiled, [(document.clone(), base.clone(), 7)]);
        // Its imports resolve against its base URL.
        assert_eq!(
            modulator.fetch_inline_tree(&mut host, "./x.mjs", &document, &base, 9, 2, SAME),
            []
        );
        assert_eq!(
            host.fetched,
            [Url::parse("https://b.test/base/x.mjs").unwrap()]
        );
        let response = FetchedModule {
            url: Url::parse("https://b.test/base/x.mjs").unwrap(),
            source: String::new(),
        };
        assert_eq!(
            modulator.fetched(&mut host, 101, Some(response)),
            Some(vec![done(2, 1)])
        );
        assert_eq!(modulator.fetched(&mut host, 101, None), None, "not twice");
    }

    /// A graph's modules are all fetched with its root's credentials mode
    /// (HTML's fetch options, which descendants take from it), an inline
    /// root's too.
    #[test]
    fn descendants_take_the_roots_credentials() {
        let include = CredentialsMode::UseCredentials;
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(1), include);
        answer(&mut modulator, &mut host, "main.mjs", Some("./a.mjs"));
        let document = url("page.html");
        modulator.fetch_inline_tree(&mut host, "./b.mjs", &document, &document, 1, 2, include);
        modulator.fetch_tree(&mut host, url("c.mjs"), GraphFor::Script(3), SAME);
        assert_eq!(
            host.fetched,
            [url("main.mjs"), url("a.mjs"), url("b.mjs"), url("c.mjs")]
        );
        assert_eq!(host.credentials, [include, include, include, SAME]);
    }

    /// A redirected module's base URL is where it ended up; the map keeps
    /// it under the URL asked for.
    #[test]
    fn a_redirected_module_resolves_against_its_response_url() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("old/m.mjs"), GraphFor::Script(1), SAME);
        let response = FetchedModule {
            url: url("new/m.mjs"),
            source: "./dep.mjs".into(),
        };
        modulator.fetched(&mut host, 101, Some(response));
        assert_eq!(host.fetched, [url("old/m.mjs"), url("new/dep.mjs")]);
        assert_eq!(host.compiled[0].1, url("new/m.mjs"));
        modulator.fetch_tree(&mut host, url("old/m.mjs"), GraphFor::Script(2), SAME);
        assert_eq!(host.fetched.len(), 2, "the map has it by the URL asked for");
    }

    /// An `import()`'s graph: done for the import, sharing the map with
    /// scripts' graphs; its fetches don't delay the document's `load`, a
    /// script's do.
    #[test]
    fn an_import_shares_the_map_and_delays_no_load() {
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(1), SAME);
        assert_eq!(
            modulator.fetch_tree(&mut host, url("lazy.mjs"), GraphFor::Import(7), SAME),
            []
        );
        assert_eq!(host.delays_load, [true, false]);
        answer(&mut modulator, &mut host, "main.mjs", Some(""));
        assert_eq!(
            answer(&mut modulator, &mut host, "lazy.mjs", Some("./main.mjs")),
            [Finished {
                target: GraphFor::Import(7),
                root: Some(ModuleScriptId::new(1)),
                parse_error: None,
            }]
        );
        assert_eq!(host.fetched.len(), 2, "main.mjs from the map");
        // Only the script's graph is linked here: the engine links the
        // import's as it finishes the import. Its requests are set all the
        // same.
        assert_eq!(host.links, [(0, None)]);
        assert_eq!(host.requests, [(0, vec![]), (1, vec![0])]);
        assert_eq!(
            modulator.fetch_tree(&mut host, url("gone.mjs"), GraphFor::Import(8), SAME),
            []
        );
        assert_eq!(
            answer(&mut modulator, &mut host, "gone.mjs", None),
            [Finished {
                target: GraphFor::Import(8),
                root: None,
                parse_error: None,
            }]
        );
    }

    /// A preload: done once its module alone has arrived (its imports
    /// aren't fetched, nor is it linked or its requests set), even with a
    /// parse error; failed if the fetch did; at once from the map, even
    /// while another fetch of it is on its way. Its fetch doesn't delay
    /// `load`. A script's graph then takes the module from the map and
    /// fetches what it imports.
    #[test]
    fn a_preload_fetches_the_module_alone() {
        let preloaded = |preload, root: Option<u32>| Finished {
            target: GraphFor::Preload(preload),
            root: root.map(ModuleScriptId::new),
            parse_error: None,
        };
        let mut modulator = Modulator::default();
        let mut host = Host::default();
        assert_eq!(
            modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Preload(1), SAME),
            []
        );
        assert_eq!(host.delays_load, [false]);
        assert_eq!(
            answer(&mut modulator, &mut host, "main.mjs", Some("./a.mjs")),
            [preloaded(1, Some(0))]
        );
        assert_eq!(host.fetched, [url("main.mjs")], "not its import");
        assert_eq!(host.links, []);
        assert_eq!(host.requests, []);

        // From the map, at once.
        assert_eq!(
            modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Preload(2), SAME),
            [preloaded(2, Some(0))]
        );
        // A syntax error is fetched all the same; a failed fetch isn't.
        modulator.fetch_tree(&mut host, url("bad.mjs"), GraphFor::Preload(3), SAME);
        assert_eq!(
            answer(&mut modulator, &mut host, "bad.mjs", Some("!")),
            [preloaded(3, Some(1))]
        );
        modulator.fetch_tree(&mut host, url("gone.mjs"), GraphFor::Preload(4), SAME);
        assert_eq!(
            answer(&mut modulator, &mut host, "gone.mjs", None),
            [preloaded(4, None)]
        );

        // Waiting on an import's fetch of the same URL, done with it.
        modulator.fetch_tree(&mut host, url("lazy.mjs"), GraphFor::Import(5), SAME);
        assert_eq!(
            modulator.fetch_tree(&mut host, url("lazy.mjs"), GraphFor::Preload(6), SAME),
            []
        );
        assert_eq!(
            answer(&mut modulator, &mut host, "lazy.mjs", Some("")),
            [
                Finished {
                    target: GraphFor::Import(5),
                    root: Some(ModuleScriptId::new(2)),
                    parse_error: None,
                },
                preloaded(6, Some(2)),
            ]
        );

        // The script's graph: main.mjs from the map, a.mjs fetched now.
        let fetches = host.fetched.len();
        assert_eq!(
            modulator.fetch_tree(&mut host, url("main.mjs"), GraphFor::Script(7), SAME),
            []
        );
        assert_eq!(host.fetched[fetches..], [url("a.mjs")]);
        assert_eq!(
            answer(&mut modulator, &mut host, "a.mjs", Some("")),
            [done(7, 0)]
        );
        assert_eq!(host.links, [(0, None)]);
    }
}
