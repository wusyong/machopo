//! Script elements that script inserts (HTML's "prepare the script
//! element"): an inline classic one runs as the call that connected it
//! ends, before the call's custom element reactions, in tree order with
//! the iframes' steps; on a node inserted into it, or a `src` set, while
//! connected; never twice, nor the parser's, nor in a document no frame
//! shows; in its document's realm, `currentScript` it meanwhile, what it
//! throws reported there. An external one queued for the host to fetch,
//! run as soon as it is fetched or in order, each in a task of its own,
//! its `load` or `error` after; delaying the load event till then. And
//! `HTMLScriptElement`'s members.

use std::time::Instant;

use dom::{LoadedScript, ScriptCors, ScriptFetch, ScriptRequest};
use dom_api::module::FetchedModule;
use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// A window of a document at `https://example.test/a/` with
/// `<html><body>`; `log`, and `script(text)` a new script element.
fn engine() -> V8Engine {
    engine_and_document().0
}

/// `engine`'s, and its document (to bind after the engine: it drops
/// first).
fn engine_and_document() -> (V8Engine, dom::NodeRoot) {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/a/",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "var log = [];
         document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
         var script = (text, d = document) => { const s = d.createElement('script'); s.text = text; return s; };",
    );
    (engine, document)
}

fn run(engine: &mut V8Engine, source: &str) {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source,
            url: "https://example.test/a/",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
}

fn log(engine: &mut V8Engine) -> String {
    run(
        engine,
        "testRunner.reportHarnessResults(log.join(' / ')); log = [];",
    );
    engine.take_harness_results().expect("reported")
}

/// Inserted, an inline script runs before the call returns; in tree
/// order with others inserted with it, each prepared as its turn comes
/// (one an earlier one removed doesn't run); once only.
#[test]
fn an_inserted_inline_script_runs_as_the_call_ends() {
    let mut engine = engine();
    run(
        &mut engine,
        "const a = script('log.push(\"a\")');
         log.push('before');
         document.body.appendChild(a);
         log.push('after');
         document.body.appendChild(a);
         const f = document.createDocumentFragment();
         f.append(script('log.push(\"b\"); document.getElementById(\"c\").remove()'),
                  script('log.push(\"x\")'));
         f.lastChild.id = 'c';
         f.append(script('log.push(\"d\")'));
         document.body.append(f);
         const g = document.createDocumentFragment();
         g.append(script(`log.push(\"e\"); document.getElementById(\"filled\").text = 'log.push(\"f\")'`));
         g.append(document.createElement('script'));
         g.lastChild.id = 'filled';
         document.body.append(g);",
    );
    // `f` ran as `e` filled it, not again in its turn.
    assert_eq!(log(&mut engine), "before / a / after / b / d / e / f");
}

/// Not connected, empty, or of a type that doesn't run: nothing yet. A
/// node inserted into it while connected, its `type` changed and
/// reinserted, or a `src` set prepare it again; prepared and run, it is
/// started for good.
#[test]
fn what_prepares_a_script_again() {
    let mut engine = engine();
    run(
        &mut engine,
        "const empty = document.body.appendChild(document.createElement('script'));
         const outside = document.createElement('div').appendChild(script('log.push(\"never\")'));
         log.push('empty');
         empty.appendChild(document.createTextNode('log.push(\"filled\")'));
         empty.appendChild(document.createTextNode('log.push(\"twice\")'));
         const plain = script('log.push(\"plain\")');
         plain.type = 'text/plain';
         document.body.appendChild(plain);
         plain.type = '';
         document.body.appendChild(plain);
         const sourced = document.createElement('script');
         document.body.appendChild(sourced);
         sourced.appendChild(document.createComment('c'));
         log.push('comment');
         const text = sourced.appendChild(document.createTextNode(''));
         log.push('empty text');
         text.data = 'log.push(\"data\")';
         log.push('data changed');
         text.splitText(0);
         log.push('split');",
    );
    // `splitText` inserts a text into it: prepared, and run.
    assert_eq!(
        log(&mut engine),
        "empty / filled / plain / comment / empty text / data changed / data / split"
    );
}

/// The parser's scripts, and those `innerHTML` makes, never run as script
/// inserts them; `createContextualFragment`'s do; a clone of a started
/// one doesn't, of one never prepared does.
#[test]
fn the_parsers_scripts_run_only_as_it_says() {
    let mut engine = engine();
    run(
        &mut engine,
        "const div = document.body.appendChild(document.createElement('div'));
         div.innerHTML = '<script>log.push(\"inner\")<\\/script>';
         document.body.appendChild(div.firstChild);
         const range = document.createRange();
         range.selectNodeContents(document.body);
         document.body.appendChild(range.createContextualFragment('<script>log.push(\"contextual\")<\\/script>'));
         const ran = script('log.push(\"ran\")');
         document.body.appendChild(ran);
         document.body.appendChild(ran.cloneNode(true));
         const unprepared = script('log.push(\"clone\")');
         document.body.appendChild(unprepared.cloneNode(true));",
    );
    assert_eq!(log(&mut engine), "contextual / ran / clone");
}

/// `currentScript` while it runs (none in a shadow tree), back after; in
/// the realm of its document's frame, what it throws reported at that
/// frame's window, not thrown at the call.
#[test]
fn it_runs_in_its_documents_realm() {
    let mut engine = engine();
    run(
        &mut engine,
        "const s = script('log.push(document.currentScript === s)');
         s.id = 's';
         document.body.appendChild(s);
         log.push(document.currentScript === null);
         const host = document.body.appendChild(document.createElement('div'));
         host.attachShadow({mode: 'open'}).appendChild(script('log.push(String(document.currentScript))'));
         window.onerror = message => log.push('top: ' + message);
         const frame = document.body.appendChild(document.createElement('iframe'));
         frame.contentWindow.onerror = message => log.push('child: ' + message);
         frame.contentWindow.mark = 'child';
         window.mark = 'top';
         const d = frame.contentDocument;
         d.body.appendChild(script('parent.log.push(mark); throw new Error(\"x\")', d));
         log.push('returned');",
    );
    assert_eq!(
        log(&mut engine),
        "true / true / null / child / child: Uncaught Error: x / returned"
    );
}

/// A script and an iframe connected together: the script after the
/// iframes before it got their browsing contexts, before those after it;
/// before a custom element's `connectedCallback` the same call left.
#[test]
fn it_runs_in_tree_order_with_the_iframes_steps() {
    let mut engine = engine();
    run(
        &mut engine,
        "customElements.define('x-a', class extends HTMLElement {
             connectedCallback() { log.push('connected'); }
         });
         const f = document.createDocumentFragment();
         const before = f.appendChild(document.createElement('iframe'));
         f.appendChild(script('log.push(String(before.contentWindow !== null), String(after.contentWindow === null))'));
         const after = f.appendChild(document.createElement('iframe'));
         f.appendChild(document.createElement('x-a'));
         document.body.appendChild(f);",
    );
    assert_eq!(log(&mut engine), "true / true / connected");
}

/// In a document no frame shows: started, and never run (moved into one
/// later neither).
#[test]
fn not_in_a_document_no_frame_shows() {
    let mut engine = engine();
    run(
        &mut engine,
        "const d = document.implementation.createHTMLDocument('');
         const s = d.body.appendChild(script('log.push(\"never\")', d));
         document.body.appendChild(s);
         log.push('done');",
    );
    assert_eq!(log(&mut engine), "done");
}

/// The types that run: a JavaScript MIME type (ASCII case-insensitively,
/// stripped of whitespace, no parameters), an empty one, `language`
/// without `type` (`text/` and it); not one of whitespace, nor a module
/// yet. `nomodule` doesn't, nor `for` and `event` other than `window`'s
/// `onload` (both needed, stripped, ASCII case-insensitively).
#[test]
fn which_inserted_scripts_run() {
    let mut engine = engine();
    run(
        &mut engine,
        "const cases = [
             ['', {type: ''}], ['upper', {type: ' TEXT/JavaScript '}], ['ecma', {type: 'application/ecmascript'}],
             ['x', {type: 'text/x-javascript'}], ['js15', {type: 'text/javascript1.5'}],
             ['lang', {language: 'javascript'}], ['empty lang', {language: ''}],
             ['type wins', {type: 'text/javascript', language: 'vbscript'}],
             ['params', {type: 'text/javascript; charset=utf-8'}], ['space', {type: ' '}],
             ['module', {type: 'module'}], ['plain', {type: 'text/plain'}], ['vb', {language: 'vbscript'}],
             ['js2', {type: 'text/javascript2.0'}],
             ['nomodule', {nomodule: ''}],
             ['onload', {for: ' Window ', event: 'onload()'}], ['ONLOAD', {for: 'window', event: 'ONLOAD'}],
             ['for alone', {for: 'document'}], ['event alone', {event: 'onclick'}],
             ['onclick', {for: 'window', event: 'onclick'}], ['document', {for: 'document', event: 'onload'}],
         ];
         for (const [name, attributes] of cases) {
             const s = script('log.push(' + JSON.stringify(name) + ')');
             for (const [attribute, value] of Object.entries(attributes)) s.setAttribute(attribute, value);
             document.body.appendChild(s);
         }",
    );
    assert_eq!(
        log(&mut engine),
        " / upper / ecma / x / js15 / lang / empty lang / type wins / onload / ONLOAD / for alone / event alone"
    );
}

/// An empty `src` (or one of whitespace), or one that doesn't parse,
/// queues `error` at the element; one that parses is queued to fetch, and
/// its text doesn't run.
#[test]
fn an_inserted_scripts_src() {
    let mut engine = engine();
    run(
        &mut engine,
        r#"for (const src of ['', ' ', 'https://[', 'b.js']) {
             const s = script('log.push("text")');
             s.src = src;
             s.onerror = () => log.push('error ' + JSON.stringify(s.getAttribute('src')));
             s.onload = () => log.push('load');
             document.body.appendChild(s);
         }
         log.push('inserted');"#,
    );
    assert!(engine.run_element_tasks());
    assert_eq!(
        log(&mut engine),
        r#"inserted / error "" / error " " / error "https://[""#
    );
    assert_eq!(fetches(&mut engine).len(), 1);
}

/// The fetches the DOM queued, taken.
fn fetches(engine: &mut V8Engine) -> Vec<ScriptFetch> {
    engine.dom_mut().take_script_fetches()
}

/// `fetch`'s script handed back: `source` from `url`, or (none) failed.
fn fetched(engine: &mut V8Engine, fetch: &ScriptFetch, source: Option<&str>, muted: bool) {
    let loaded = source.map(|source| LoadedScript {
        source: source.to_owned(),
        url: fetch.request.url.clone(),
        muted,
    });
    engine
        .dom_mut()
        .mutate(|cx| cx.script_fetched(fetch.key, loaded));
}

/// Whether `document`'s load event waits.
fn load_delayed(engine: &mut V8Engine, document: &dom::NodeRoot) -> bool {
    engine
        .dom_mut()
        .read(|cx| cx.load_event_delayed(document.get(cx.gc)))
}

/// What a fetch asks: its URL resolved, `crossorigin`'s credentials mode,
/// the `charset` attribute; the document it was prepared in.
#[test]
fn an_external_scripts_fetch() {
    let mut engine = engine();
    run(
        &mut engine,
        "for (const [src, crossorigin, charset] of [['a.js'], ['/b.js', '', 'shift_jis'], ['c.js', 'USE-credentials'],
                                                 ['d.js', 'bogus']]) {
             const s = document.createElement('script');
             s.src = src;
             if (crossorigin !== undefined) s.setAttribute('crossorigin', crossorigin);
             if (charset) s.charset = charset;
             document.body.appendChild(s);
         }",
    );
    let requests: Vec<ScriptRequest> = fetches(&mut engine)
        .into_iter()
        .map(|fetch| fetch.request)
        .collect();
    let request = |url: &str, cors, charset: Option<&str>| ScriptRequest {
        url: url.to_owned(),
        cors,
        charset: charset.map(str::to_owned),
    };
    assert_eq!(
        requests,
        [
            request("https://example.test/a/a.js", None, None),
            request(
                "https://example.test/b.js",
                Some(ScriptCors::Anonymous),
                Some("shift_jis")
            ),
            request(
                "https://example.test/a/c.js",
                Some(ScriptCors::UseCredentials),
                None
            ),
            request(
                "https://example.test/a/d.js",
                Some(ScriptCors::Anonymous),
                None
            ),
        ]
    );
}

/// As soon as it is fetched (`async` left as it was, or true), each in a
/// task of its own (its `load`, a microtask checkpoint after); with
/// `async = false`, in the order they
/// were inserted, once those before it were fetched; one that failed
/// fires `error`, its turn come. `currentScript` while it runs. The load
/// event waits for them all.
#[test]
fn external_scripts_run_as_soon_as_they_can_or_in_order() {
    let (mut engine, document) = engine_and_document();
    run(
        &mut engine,
        "for (const [name, async] of [['a'], ['b', true], ['c', false], ['d', false], ['e', false]]) {
             const s = document.createElement('script');
             s.src = name + '.js';
             s.id = name;
             if (async !== undefined) s.async = async;
             s.onload = () => { log.push('load ' + name); Promise.resolve().then(() => log.push('micro ' + name)); };
             s.onerror = () => log.push('error ' + name);
             document.body.appendChild(s);
         }",
    );
    let fetches = fetches(&mut engine);
    assert_eq!(fetches.len(), 5);
    assert!(load_delayed(&mut engine, &document));
    let source = |name: &str| format!("log.push('{name} ' + document.currentScript.id)");
    // `b` before `a`; `d` waits for `c`; `c` failed.
    fetched(&mut engine, &fetches[1], Some(&source("ran b")), false);
    fetched(&mut engine, &fetches[3], Some(&source("ran d")), false);
    assert!(engine.run_element_tasks());
    fetched(&mut engine, &fetches[2], None, false);
    fetched(&mut engine, &fetches[0], Some(&source("ran a")), false);
    assert!(engine.run_element_tasks());
    assert!(load_delayed(&mut engine, &document), "e not yet");
    fetched(&mut engine, &fetches[4], Some(&source("ran e")), false);
    assert!(engine.run_element_tasks());
    assert!(!load_delayed(&mut engine, &document));
    assert_eq!(
        log(&mut engine),
        "ran b b / load b / micro b / error c / ran d d / load d / micro d / ran a a / load a / micro a / \
         ran e e / load e / micro e"
    );
}

/// One whose element moved to another document before its turn runs
/// nothing and fires nothing, and no longer delays the load event; one
/// removed from its document runs all the same. A fetch handed back
/// twice, or one never asked for, does nothing.
#[test]
fn a_moved_external_script_doesnt_run() {
    let (mut engine, document) = engine_and_document();
    run(
        &mut engine,
        "var moved = document.createElement('script'), removed = document.createElement('script');
         moved.src = 'moved.js';
         removed.src = 'removed.js';
         for (const s of [moved, removed]) {
             s.onload = () => log.push('load ' + s.src);
             document.body.appendChild(s);
         }
         document.implementation.createHTMLDocument('').body.appendChild(moved);
         removed.remove();",
    );
    let fetches = fetches(&mut engine);
    fetched(
        &mut engine,
        &fetches[0],
        Some("log.push('moved ran')"),
        false,
    );
    fetched(
        &mut engine,
        &fetches[1],
        Some("log.push('removed ran')"),
        false,
    );
    fetched(&mut engine, &fetches[1], Some("log.push('again')"), false);
    engine
        .dom_mut()
        .mutate(|cx| cx.script_fetched(u64::MAX, None));
    assert!(engine.run_element_tasks());
    assert!(!load_delayed(&mut engine, &document));
    assert_eq!(
        log(&mut engine),
        "removed ran / load https://example.test/a/removed.js"
    );
}

/// Muted (another origin's, fetched `no-cors`), what it throws is
/// `Script error.`, from nowhere, its error null; even in a callback it
/// made. Not muted, all of it.
#[test]
fn a_muted_scripts_errors() {
    let mut engine = engine();
    run(
        &mut engine,
        "window.onerror = (message, filename, line, column, error) =>
             log.push([message, filename, line, column, error === null ? 'null' : String(error)].join(' '));
         for (const name of ['muted', 'shared']) {
             const s = document.createElement('script');
             s.src = name + '.js';
             document.body.appendChild(s);
         }",
    );
    let fetches = fetches(&mut engine);
    let throws =
        "\nsetTimeout(() => { throw new Error('later'); }, 0);\nthrow new TypeError('now');";
    fetched(&mut engine, &fetches[0], Some(throws), true);
    fetched(&mut engine, &fetches[1], Some(throws), false);
    assert!(engine.run_element_tasks());
    engine.run_timers();
    assert_eq!(
        log(&mut engine),
        "Script error.  0 0 null / \
         Uncaught TypeError: now https://example.test/a/shared.js 3 1 TypeError: now / \
         Script error.  0 0 null / \
         Uncaught Error: later https://example.test/a/shared.js 2 20 Error: later"
    );
}

/// `HTMLScriptElement`'s members: `async` true for one script made until
/// set (or its attribute added), false for the parser's; `text` its child
/// text content; the rest reflect; `supports` (classic and module
/// scripts, by case).
#[test]
fn html_script_elements_members() {
    let mut engine = engine();
    run(
        &mut engine,
        "const s = document.createElement('script');
         log.push(s instanceof HTMLScriptElement, s.async);
         s.async = false;
         log.push(s.async, s.hasAttribute('async'));
         s.async = true;
         log.push(s.async, s.getAttribute('async'));
         const t = document.createElement('script');
         t.setAttribute('async', '');
         t.removeAttribute('async');
         log.push(t.async);
         const div = document.createElement('div');
         div.innerHTML = '<script>a<\\/script>';
         log.push(div.firstChild.async);
         s.append('x', document.createElement('b'), 'y');
         s.lastChild.previousSibling.textContent = 'not';
         log.push(s.text);
         s.text = 'z';
         log.push(s.childNodes.length, s.text);
         s.src = 'b.js';
         s.htmlFor = 'window';
         s.noModule = true;
         s.crossOrigin = 'USE-CREDENTIALS';
         log.push(s.src, s.getAttribute('for'), s.noModule, s.crossOrigin, s.defer, s.type, s.charset, s.event);
         log.push(HTMLScriptElement.supports('classic'), HTMLScriptElement.supports('Classic'),
                  HTMLScriptElement.supports('module'));",
    );
    assert_eq!(
        log(&mut engine),
        "true / true / false / false / true /  / false / false / xy / 1 / z / \
         https://example.test/a/b.js / window / true / use-credentials / false /  /  /  / \
         true / false / true"
    );
}

/// A module the module map asked for, handed back: `source` from `fetch`'s
/// URL, or (none) failed.
fn module_fetched(engine: &mut V8Engine, fetch: &ScriptFetch, source: Option<&str>) {
    let response = source.map(|source| FetchedModule {
        url: url::Url::parse(&fetch.request.url).unwrap(),
        source: source.to_owned(),
    });
    assert!(
        engine.module_fetched(fetch.key, response),
        "{}",
        fetch.request.url
    );
}

/// An inline module script runs as soon as its graph is in (none to
/// fetch: its own task), after the script that inserted it; `currentScript`
/// null in it, `import.meta.url` its document's URL, what it throws
/// reported at the window; no `load` (inline).
#[test]
fn an_inline_module_script_runs_in_a_task() {
    let mut engine = engine();
    run(
        &mut engine,
        "window.onerror = message => log.push('error ' + message);
         const s = document.createElement('script');
         s.type = 'module';
         s.text = 'log.push(String(document.currentScript), import.meta.url); throw new Error(\"boom\")';
         s.onload = () => log.push('load');
         document.body.appendChild(s);
         log.push('inserted');",
    );
    assert!(fetches(&mut engine).is_empty());
    assert!(engine.run_element_tasks());
    assert_eq!(
        log(&mut engine),
        "inserted / null / https://example.test/a/ / error Uncaught Error: boom"
    );
}

/// An external module's graph: the root, then what it imports (resolved
/// against its URL), each once, by CORS (same-origin, or `crossorigin`'s
/// credentials), the modules fetched; it runs once all are in, then its
/// `load`. A graph with a module that failed to fetch fires `error`; one
/// with a parse error runs nothing and reports it as its evaluation's
/// promise rejects (a microtask, in the checkpoint before its `load`).
/// The load event waits for them.
#[test]
fn an_external_module_scripts_graph() {
    let (mut engine, document) = engine_and_document();
    run(
        &mut engine,
        "window.onerror = message => log.push('error ' + message);
         for (const [src, crossorigin] of [['main.mjs'], ['gone.mjs', 'use-credentials'], ['broken.mjs']]) {
             const s = document.createElement('script');
             s.type = 'module';
             s.src = src;
             if (crossorigin) s.crossOrigin = crossorigin;
             s.onload = () => log.push('load ' + src);
             s.onerror = () => log.push('error event ' + src);
             document.body.appendChild(s);
         }",
    );
    let roots = fetches(&mut engine);
    let requests: Vec<(&str, Option<ScriptCors>, bool)> = roots
        .iter()
        .map(|fetch| (fetch.request.url.as_str(), fetch.request.cors, fetch.module))
        .collect();
    assert_eq!(
        requests,
        [
            (
                "https://example.test/a/main.mjs",
                Some(ScriptCors::Anonymous),
                true
            ),
            (
                "https://example.test/a/gone.mjs",
                Some(ScriptCors::UseCredentials),
                true
            ),
            (
                "https://example.test/a/broken.mjs",
                Some(ScriptCors::Anonymous),
                true
            ),
        ]
    );
    assert!(load_delayed(&mut engine, &document));
    module_fetched(
        &mut engine,
        &roots[0],
        Some("import { a } from './dep.mjs'; import '/b/dep2.mjs'; log.push('main ' + a);"),
    );
    module_fetched(&mut engine, &roots[1], None);
    module_fetched(
        &mut engine,
        &roots[2],
        Some("import 'missing'; log.push('never');"),
    );
    let deps = fetches(&mut engine);
    assert_eq!(
        deps.iter()
            .map(|fetch| fetch.request.url.as_str())
            .collect::<Vec<_>>(),
        [
            "https://example.test/a/dep.mjs",
            "https://example.test/b/dep2.mjs"
        ]
    );
    module_fetched(
        &mut engine,
        &deps[0],
        Some("export const a = 'a'; log.push('dep');"),
    );
    assert!(engine.run_element_tasks());
    assert!(load_delayed(&mut engine, &document), "main waits for dep2");
    module_fetched(&mut engine, &deps[1], Some("log.push('dep2');"));
    assert!(engine.run_element_tasks());
    assert!(!load_delayed(&mut engine, &document));
    assert_eq!(
        log(&mut engine),
        "error event gone.mjs / \
         error Uncaught TypeError: Failed to resolve module specifier \"missing\". Relative references must start with either \"/\", \"./\", or \"../\". / \
         load broken.mjs / dep / dep2 / main a / load main.mjs"
    );
}

/// `nomodule`, `for` and `event` are a classic script's: a module script
/// with them runs. Its type ASCII case-insensitively, stripped.
#[test]
fn a_module_script_ignores_classic_attributes() {
    let mut engine = engine();
    run(
        &mut engine,
        "for (const [name, attributes] of [['nomodule', {nomodule: ''}], ['for', {for: 'document', event: 'onclick'}],
                                           ['case', {type: ' Module '}]]) {
             const s = document.createElement('script');
             s.type = 'module';
             s.text = 'log.push(\"' + name + '\")';
             for (const [attribute, value] of Object.entries(attributes)) s.setAttribute(attribute, value);
             document.body.appendChild(s);
         }",
    );
    assert!(engine.run_element_tasks());
    assert_eq!(log(&mut engine), "nomodule / for / case");
}

/// With `async = false`, module scripts run in the order they were
/// inserted, an inline one too.
#[test]
fn in_order_module_scripts() {
    let mut engine = engine();
    run(
        &mut engine,
        "for (const [name, inline] of [['a'], ['b', true], ['c']]) {
             const s = document.createElement('script');
             s.type = 'module';
             s.async = false;
             if (inline) s.text = 'log.push(\"b\")'; else s.src = name + '.mjs';
             document.body.appendChild(s);
         }",
    );
    let roots = fetches(&mut engine);
    module_fetched(&mut engine, &roots[1], Some("log.push('c')"));
    engine.run_element_tasks();
    module_fetched(&mut engine, &roots[0], Some("log.push('a')"));
    assert!(engine.run_element_tasks());
    assert_eq!(log(&mut engine), "a / b / c");
}

/// `import()` from a classic script: the graph fetched (the specifier
/// resolved against the script's base URL, by CORS with its fetch options'
/// credentials: `crossorigin=use-credentials`'s include, else
/// same-origin), its promise settled with the namespace; one already in
/// the map in a microtask, after the script that asked; one that failed
/// rejects, and is fetched again the next time; one that doesn't resolve
/// rejects at once. An event handler's resolves against the document's
/// URL. None delays the load event.
#[test]
fn dynamic_imports() {
    let (mut engine, document) = engine_and_document();
    run(
        &mut engine,
        "window.settled = p => p.then(m => log.push('ok ' + m.x), e => log.push(e.name + ': ' + e.message));
         settled(import('./m.mjs'));
         settled(import('gone'));
         const s = script('settled(import(\"../c.mjs\"))');
         s.setAttribute('crossorigin', 'use-credentials');
         document.body.appendChild(s);",
    );
    let first = fetches(&mut engine);
    let requests: Vec<(&str, Option<ScriptCors>)> = first
        .iter()
        .map(|fetch| (fetch.request.url.as_str(), fetch.request.cors))
        .collect();
    assert_eq!(
        requests,
        [
            ("https://example.test/a/m.mjs", Some(ScriptCors::Anonymous)),
            (
                "https://example.test/c.mjs",
                Some(ScriptCors::UseCredentials)
            ),
        ]
    );
    assert!(!load_delayed(&mut engine, &document));
    module_fetched(&mut engine, &first[0], Some("export const x = 'm';"));
    module_fetched(&mut engine, &first[1], None);
    run(
        &mut engine,
        "settled(import('./m.mjs')); log.push('after');
         settled(import('../c.mjs'));",
    );
    engine.perform_microtask_checkpoint();
    let again = fetches(&mut engine);
    assert_eq!(
        again
            .iter()
            .map(|fetch| fetch.request.url.as_str())
            .collect::<Vec<_>>(),
        ["https://example.test/c.mjs"]
    );
    module_fetched(&mut engine, &again[0], Some("export const x = 'c';"));
    // In the map, not yet evaluated (a preload's): evaluated after the
    // script that imports it goes on, not inside the call.
    run(
        &mut engine,
        "const l = document.createElement('link');
         l.setAttribute('rel', 'modulepreload');
         l.setAttribute('href', 'u.mjs');
         document.body.appendChild(l);",
    );
    let preload = fetches(&mut engine);
    module_fetched(
        &mut engine,
        &preload[0],
        Some("log.push('u'); export const x = 'u';"),
    );
    run(
        &mut engine,
        "settled(import('./u.mjs')); log.push('before u');",
    );
    engine.perform_microtask_checkpoint();
    run(
        &mut engine,
        "document.body.setAttribute('onclick', 'settled(import(\"./m.mjs\"))');
         document.body.click();",
    );
    engine.perform_microtask_checkpoint();
    assert_eq!(
        log(&mut engine),
        "TypeError: Failed to resolve module specifier \"gone\". Relative references must start with either \"/\", \"./\", or \"../\". / \
         ok m / TypeError: Failed to fetch dynamically imported module: https://example.test/c.mjs / \
         after / ok m / ok c / before u / u / ok u / ok m"
    );
}

/// A module's `import()`: against the module's URL, with its graph's
/// credentials; what its graph's evaluation throws rejects it; a parse
/// error rejects it. `import.meta.resolve`: the URL a specifier resolves
/// to against the module's URL, a `TypeError` for one that doesn't.
#[test]
fn a_modules_dynamic_imports_and_import_meta_resolve() {
    let mut engine = engine();
    run(
        &mut engine,
        "const s = document.createElement('script');
         s.type = 'module';
         s.src = '/b/main.mjs';
         s.crossOrigin = 'use-credentials';
         document.body.appendChild(s);",
    );
    let main = fetches(&mut engine);
    module_fetched(
        &mut engine,
        &main[0],
        Some(
            "log.push(import.meta.resolve('./x.mjs'), import.meta.resolve('/y.mjs'));
             try { import.meta.resolve('bare'); } catch (e) { log.push(e.name); }
             import('./throws.mjs').catch(e => log.push('throws: ' + e.message));
             import('./broken.mjs').catch(e => log.push('broken: ' + e.name));",
        ),
    );
    assert!(engine.run_element_tasks());
    let imports = fetches(&mut engine);
    let requests: Vec<(&str, Option<ScriptCors>)> = imports
        .iter()
        .map(|fetch| (fetch.request.url.as_str(), fetch.request.cors))
        .collect();
    assert_eq!(
        requests,
        [
            (
                "https://example.test/b/throws.mjs",
                Some(ScriptCors::UseCredentials)
            ),
            (
                "https://example.test/b/broken.mjs",
                Some(ScriptCors::UseCredentials)
            ),
        ]
    );
    module_fetched(&mut engine, &imports[0], Some("throw new Error('boom');"));
    module_fetched(&mut engine, &imports[1], Some("export {"));
    assert_eq!(
        log(&mut engine),
        "https://example.test/b/x.mjs / https://example.test/y.mjs / TypeError / throws: boom / broken: SyntaxError"
    );
}

/// `<link rel=modulepreload>` (one of `rel`'s keywords, ASCII
/// case-insensitively) connected: its module alone fetched (by CORS,
/// `crossorigin`'s credentials), `load` once it came, `error` if it
/// failed; again as its `href` changes (not for the same value); `as` not
/// script-like: `error`, nothing fetched; one in a shadow tree, or out of
/// the document, is nothing. The module is in the map for a script after.
#[test]
fn module_preloads() {
    let mut engine = engine();
    run(
        &mut engine,
        "window.link = (href, attributes = {}) => {
             const l = document.createElement('link');
             l.setAttribute('rel', attributes.rel ?? 'modulepreload');
             l.setAttribute('href', href);
             for (const [name, value] of Object.entries(attributes)) l.setAttribute(name, value);
             l.onload = () => log.push('load ' + l.getAttribute('href'));
             l.onerror = () => log.push('error ' + l.getAttribute('href'));
             return l;
         };
         document.body.appendChild(link('p.mjs', {crossorigin: 'use-credentials'}));
         document.body.appendChild(link('gone.mjs', {rel: 'preload ModulePreload'}));
         document.body.appendChild(link('s.css', {as: 'style'}));
         document.createElement('div').appendChild(link('out.mjs'));
         document.body.appendChild(document.createElement('div')).attachShadow({mode: 'open'}).appendChild(link('shadow.mjs'));
         window.changing = document.body.appendChild(link('first.mjs'));
         changing.setAttribute('href', 'second.mjs');
         changing.setAttribute('href', 'second.mjs');",
    );
    let preloads = fetches(&mut engine);
    let requests: Vec<(&str, Option<ScriptCors>, bool)> = preloads
        .iter()
        .map(|fetch| (fetch.request.url.as_str(), fetch.request.cors, fetch.module))
        .collect();
    assert_eq!(
        requests,
        [
            (
                "https://example.test/a/p.mjs",
                Some(ScriptCors::UseCredentials),
                true
            ),
            (
                "https://example.test/a/gone.mjs",
                Some(ScriptCors::Anonymous),
                true
            ),
            (
                "https://example.test/a/first.mjs",
                Some(ScriptCors::Anonymous),
                true
            ),
            (
                "https://example.test/a/second.mjs",
                Some(ScriptCors::Anonymous),
                true
            ),
        ]
    );
    module_fetched(
        &mut engine,
        &preloads[0],
        Some("import './dep.mjs'; log.push('p ran');"),
    );
    module_fetched(&mut engine, &preloads[1], None);
    module_fetched(&mut engine, &preloads[3], Some(""));
    assert!(engine.run_element_tasks());
    // Its import isn't fetched for the preload; a script of it is.
    assert!(fetches(&mut engine).is_empty());
    run(
        &mut engine,
        "const s = document.createElement('script'); s.type = 'module'; s.src = 'p.mjs'; document.body.appendChild(s);",
    );
    let dep = fetches(&mut engine);
    assert_eq!(
        dep.iter()
            .map(|fetch| fetch.request.url.as_str())
            .collect::<Vec<_>>(),
        ["https://example.test/a/dep.mjs"]
    );
    module_fetched(&mut engine, &dep[0], Some(""));
    engine.run_element_tasks();
    assert_eq!(
        log(&mut engine),
        "error s.css / load p.mjs / error gone.mjs / load second.mjs / p ran"
    );
}
