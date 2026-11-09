//! ES modules on V8, without the DOM: compiling, linking a graph from the
//! requests the embedder resolved, evaluating it, and its errors.

use std::collections::HashMap;

use bindings::{ModuleRecords, ScriptLocation, ScriptState, V8PerIsolateData};

/// A realm with modules, and the URLs of the ones compiled.
struct Modules {
    records: ModuleRecords,
    urls: HashMap<String, usize>,
    state: ScriptState,
    data: V8PerIsolateData,
}

impl Modules {
    fn new() -> Self {
        let mut data = V8PerIsolateData::new();
        let state = ScriptState::new(&mut data);
        let records = {
            let isolate = data.isolate();
            v8::scope_with_context!(let scope, isolate, state.context());
            ModuleRecords::new(scope)
        };
        Self {
            records,
            urls: HashMap::new(),
            state,
            data,
        }
    }

    /// Compiles `source` as the module at `url`; its requests, or `None`.
    fn compile(&mut self, url: &str, source: &str) -> Option<Vec<String>> {
        let isolate = self.data.isolate();
        v8::scope_with_context!(let scope, isolate, self.state.context());
        let compiled = self.records.compile(
            scope,
            source,
            ScriptLocation {
                url,
                line: 1,
                base_url: None,
            },
            url,
        );
        self.urls.insert(url.to_owned(), compiled.module);
        compiled
            .requests
            .map(|requests| requests.into_iter().map(|r| r.specifier).collect())
    }

    /// Compiles each `(url, source)`, and resolves every request to the
    /// module whose URL is the specifier, as an embedder would after
    /// fetching them all.
    fn graph(&mut self, modules: &[(&str, &str)]) {
        let mut requests = Vec::new();
        for (url, source) in modules {
            requests.push((*url, self.compile(url, source)));
        }
        for (url, requests) in requests {
            if let Some(requests) = requests {
                let resolved = requests
                    .iter()
                    .map(|specifier| self.urls[specifier])
                    .collect();
                self.records.set_requests(self.urls[url], resolved);
            }
        }
    }

    fn link(&mut self, root: &str, parse_error: Option<&str>) {
        let isolate = self.data.isolate();
        v8::scope_with_context!(let scope, isolate, self.state.context());
        let parse_error = parse_error.map(|url| self.urls[url]);
        self.records.link(scope, self.urls[root], parse_error);
    }

    /// Evaluates the graph `root` and runs the microtasks; how its
    /// promise settled: `fulfilled`, or the rejection's reason.
    fn run(&mut self, root: &str) -> String {
        let evaluation = self.records.evaluation(self.urls[root]);
        let promise = {
            let isolate = self.data.isolate();
            v8::scope_with_context!(let scope, isolate, self.state.context());
            let promise = evaluation.evaluate(scope).expect("not terminated");
            v8::Global::new(scope, promise)
        };
        self.data.perform_microtask_checkpoint();
        let isolate = self.data.isolate();
        v8::scope_with_context!(let scope, isolate, self.state.context());
        let promise = v8::Local::new(scope, &promise);
        match promise.state() {
            v8::PromiseState::Fulfilled => "fulfilled".into(),
            v8::PromiseState::Rejected => promise.result(scope).to_rust_string_lossy(scope),
            v8::PromiseState::Pending => "pending".into(),
        }
    }

    fn eval(&mut self, source: &str) -> String {
        self.state
            .evaluate_classic_script(&mut self.data, source)
            .unwrap_or_else(|e| panic!("{source}: {e}"))
    }
}

#[test]
fn requests_come_once_in_source_order_with_their_attributes() {
    let mut modules = Modules::new();
    let isolate = modules.data.isolate();
    v8::scope_with_context!(let scope, isolate, modules.state.context());
    let compiled = modules.records.compile(
        scope,
        "import './a.mjs'; export { x } from './b.mjs';
         import j from './c.json' with { type: 'json' }; import './a.mjs';",
        ScriptLocation {
            url: "https://a.test/m.mjs",
            line: 1,
            base_url: None,
        },
        "https://a.test/m.mjs",
    );
    let requests = compiled.requests.expect("it parses");
    let written: Vec<(&str, Vec<(String, String)>)> = requests
        .iter()
        .map(|r| (r.specifier.as_str(), r.attributes.clone()))
        .collect();
    assert_eq!(
        written,
        [
            ("./a.mjs", vec![]),
            ("./b.mjs", vec![]),
            ("./c.json", vec![("type".into(), "json".into())]),
        ]
    );
}

#[test]
fn a_graph_links_and_runs_each_module_once() {
    let mut modules = Modules::new();
    modules.graph(&[
        (
            "main",
            "import { a } from 'a'; import { b } from 'b';
             globalThis.log = (globalThis.log || '') + `main(${a},${b});`;",
        ),
        (
            "a",
            "import { shared } from 'shared';
             export const a = shared + 1; globalThis.log = (globalThis.log || '') + 'a;';",
        ),
        (
            "b",
            "import { shared } from 'shared';
             export const b = shared + 2; globalThis.log = (globalThis.log || '') + 'b;';",
        ),
        (
            "shared",
            "export const shared = 10; globalThis.log = (globalThis.log || '') + 'shared;';",
        ),
    ]);
    modules.link("main", None);
    assert_eq!(modules.run("main"), "fulfilled");
    assert_eq!(modules.eval("log"), "shared;a;b;main(11,12);");
    // A second graph of shared modules doesn't run them again.
    modules.graph(&[(
        "second",
        "import { a } from 'a'; globalThis.log += `second(${a});`;",
    )]);
    modules.link("second", None);
    assert_eq!(modules.run("second"), "fulfilled");
    assert_eq!(modules.eval("log"), "shared;a;b;main(11,12);second(11);");
}

#[test]
fn cycles_link_and_see_uninitialized_bindings() {
    let mut modules = Modules::new();
    modules.graph(&[
        (
            "even",
            "import { odd } from 'odd';
             export function even(n) { return n === 0 ? true : odd(n - 1); }
             globalThis.result = even(9);",
        ),
        (
            "odd",
            "import { even } from 'even';
             export function odd(n) { return n === 0 ? false : even(n - 1); }
             let early; try { even; early = 'reached'; } catch (e) { early = e.name; }
             globalThis.early = early;",
        ),
    ]);
    modules.link("even", None);
    assert_eq!(modules.run("even"), "fulfilled");
    // `even` is a function declaration, so already initialized while `odd`
    // runs first; a `let` export would be in its TDZ.
    assert_eq!(modules.eval("`${result},${early}`"), "false,reached");

    let mut modules = Modules::new();
    modules.graph(&[
        ("x", "import { y } from 'y'; export let x = 1;"),
        (
            "y",
            "import { x } from 'x'; export let y = 2;
             try { x; globalThis.seen = 'value'; } catch (e) { globalThis.seen = e.name; }",
        ),
    ]);
    modules.link("x", None);
    assert_eq!(modules.run("x"), "fulfilled");
    assert_eq!(modules.eval("seen"), "ReferenceError");
}

#[test]
fn errors_reject_the_evaluation() {
    // A missing export fails linking: the error to rethrow.
    let mut modules = Modules::new();
    modules.graph(&[
        ("main", "import { nope } from 'lib'; globalThis.ran = true;"),
        ("lib", "export const yes = 1;"),
    ]);
    modules.link("main", None);
    let reason = modules.run("main");
    assert!(reason.starts_with("SyntaxError"), "{reason}");
    assert!(reason.contains("nope"), "{reason}");
    assert_eq!(modules.eval("typeof ran"), "undefined");

    // A parse error anywhere in the graph is rethrown, and nothing runs.
    let mut modules = Modules::new();
    modules.graph(&[
        ("main", "import 'bad'; globalThis.ran = true;"),
        ("bad", "export const = ;"),
    ]);
    assert_eq!(modules.compile("bad2", "let = ;"), None);
    modules.link("main", Some("bad"));
    let reason = modules.run("main");
    assert!(reason.starts_with("SyntaxError"), "{reason}");
    assert_eq!(modules.eval("typeof ran"), "undefined");

    // An exception, and a top-level `await` that rejects.
    let mut modules = Modules::new();
    modules.graph(&[("throws", "throw new TypeError('boom');")]);
    modules.link("throws", None);
    assert_eq!(modules.run("throws"), "TypeError: boom");
    modules.graph(&[(
        "awaits",
        "globalThis.before = true; await Promise.reject(new RangeError('later'));",
    )]);
    modules.link("awaits", None);
    assert_eq!(modules.run("awaits"), "RangeError: later");
    assert_eq!(modules.eval("before"), "true");
}

#[test]
fn a_set_parse_error_is_rethrown() {
    let mut modules = Modules::new();
    modules.graph(&[("main", "globalThis.ran = true;")]);
    {
        let isolate = modules.data.isolate();
        v8::scope_with_context!(let scope, isolate, modules.state.context());
        let message = v8::String::new(scope, "unresolved").unwrap();
        let error = v8::Exception::type_error(scope, message);
        modules
            .records
            .set_parse_error(scope, modules.urls["main"], error);
    }
    modules.link("main", Some("main"));
    assert_eq!(modules.run("main"), "TypeError: unresolved");
    assert_eq!(modules.eval("typeof ran"), "undefined");
}

#[test]
fn import_meta_url_is_the_modules_url() {
    let mut modules = Modules::new();
    modules.graph(&[
        (
            "https://a.test/dir/main.mjs",
            "import { url } from 'https://a.test/lib.mjs';
             globalThis.urls = [import.meta.url, url, import.meta === import.meta];",
        ),
        (
            "https://a.test/lib.mjs",
            "export const url = import.meta.url;",
        ),
    ]);
    modules.link("https://a.test/dir/main.mjs", None);
    assert_eq!(modules.run("https://a.test/dir/main.mjs"), "fulfilled");
    assert_eq!(
        modules.eval("urls.join()"),
        "https://a.test/dir/main.mjs,https://a.test/lib.mjs,true"
    );
}

/// A realm's modules hold its context until they go: dropping both lets V8
/// collect the context, and what only a module's environment held.
#[test]
fn modules_go_with_their_realm() {
    let mut modules = Modules::new();
    modules.graph(&[(
        "main",
        "const held = { big: new Array(1000).fill(1) }; export function get() { return held; }",
    )]);
    modules.link("main", None);
    assert_eq!(modules.run("main"), "fulfilled");
    let context = v8::Weak::new(modules.data.isolate(), modules.state.context());
    let Modules {
        records,
        state,
        mut data,
        ..
    } = modules;
    // The context alone doesn't hold the modules, nor they it.
    drop(state);
    data.collect_garbage_for_testing();
    {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        assert!(
            context.to_local(scope).is_some(),
            "the modules hold their context"
        );
    }
    drop(records);
    for _ in 0..2 {
        data.collect_garbage_for_testing();
    }
    let collected = {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        context.to_local(scope).is_none()
    };
    drop(context);
    assert!(collected, "the context");
}

/// An inline module's `import.meta.url` is its base URL (the document's
/// base URL), not the URL its errors name (the document's).
#[test]
fn import_meta_url_is_the_base_url() {
    let mut modules = Modules::new();
    let module = {
        let isolate = modules.data.isolate();
        v8::scope_with_context!(let scope, isolate, modules.state.context());
        let location = ScriptLocation {
            url: "https://a.test/page.html",
            line: 3,
            base_url: None,
        };
        let compiled = modules.records.compile(
            scope,
            "globalThis.meta = import.meta.url; throw new Error('x');",
            location,
            "https://a.test/base/",
        );
        compiled.module
    };
    modules.urls.insert("inline".into(), module);
    modules.records.set_requests(module, Vec::new());
    modules.link("inline", None);
    assert_eq!(modules.run("inline"), "Error: x");
    assert_eq!(modules.eval("meta"), "https://a.test/base/");
}
