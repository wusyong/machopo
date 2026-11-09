//! Module scripts in a whole browser, over HTTP from a local test server:
//! the graph is fetched (each module once, against the response's URL),
//! runs after parsing in document order with the deferred classic
//! scripts, before `DOMContentLoaded`; a module needs an ok response
//! served as JavaScript; script elements fire `load` and `error`; errors
//! in a module are reported at the window. Modules (and classic scripts
//! with `crossorigin`) from another origin go by CORS; a `file:` page's
//! are blocked.
//!
//! Each page paints a green square at (10, 10) if what it checked holds,
//! red otherwise; it writes what it saw into the square's `title`, which
//! the assertion's message can't show, so the check is spelled out in the
//! page. A page that checks after `load` (an `import()` doesn't delay it)
//! puts `reftest-wait` on the root first: the frame is taken once the
//! check removes it.

use std::collections::HashMap;

use gfx::Size;
use net::test_server::{Route, TestServer};
use shell::headless::{self, Frame};

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn render(url: &str) -> Frame {
    headless::render(url, Size::new(100, 100), 1.0, true).expect("the page renders")
}

/// Premultiplied RGBA at `(x, y)`.
fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * frame.size.width + x) * 4) as usize;
    frame.pixels[i..i + 4].try_into().unwrap()
}

const GREEN: [u8; 4] = [0, 128, 0, 255];

/// The start of each page: `log`, and `check(seen, expected)`, which
/// paints the square.
const HEAD: &str = r#"<!DOCTYPE html>
<style>html, body { margin: 0 } #result { width: 20px; height: 20px }</style>
<script>
window.log = [];
// A URL without the server's origin (no `location` yet).
window.path = url => url.slice(document.URL.length - 1);
window.check = (seen, expected) => {
  const result = document.createElement('div');
  result.id = 'result';
  result.title = seen;
  result.setAttribute('style', 'background-color: ' + (seen === expected ? 'green' : 'red'));
  document.body.prepend(result);
  // The page is captured once it goes, for checks after `load`.
  document.documentElement.classList.remove('reftest-wait');
};
</script>
"#;

fn js(body: &str) -> Route {
    Route::ok("text/javascript", body)
}

/// Modules run after parsing, in document order with the deferred classic
/// scripts and with inline modules, each once, before `DOMContentLoaded`;
/// a module shared by two graphs is fetched once.
#[test]
fn modules_run_after_parsing_in_document_order() {
    let page = format!(
        r#"{HEAD}
<script defer src="/defer1.js"></script>
<script type="module" src="/main.mjs"></script>
<script type="module">import "./shared.mjs"; log.push("inline");</script>
<script>
log.push("parser");
document.addEventListener("DOMContentLoaded", () => log.push("dcl"));
document.addEventListener("DOMContentLoaded", () =>
  check(log.join(), "parser,defer1,shared,a,main,inline,defer2,dcl"));
</script>
<script defer src="/defer2.js"></script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/defer1.js", js("log.push('defer1')")),
        ("/defer2.js", js("log.push('defer2')")),
        (
            "/main.mjs",
            js("import './lib/a.mjs'; import { shared } from './shared.mjs'; log.push('main');"),
        ),
        // Resolves against its own URL.
        ("/lib/a.mjs", js("import '../shared.mjs'; log.push('a');")),
        (
            "/shared.mjs",
            js("export const shared = 1; log.push('shared');"),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let shared = server
        .requests()
        .iter()
        .filter(|r| r.target == "/shared.mjs")
        .count();
    assert_eq!(shared, 1, "a module is fetched once");
}

/// A module needs an ok response with a JavaScript MIME type, and so does
/// every module of its graph; one that fails fires `error` at its script
/// element and runs nothing. A classic script that fails, an empty `src`
/// and one that doesn't resolve fire `error` too; scripts from a `src`
/// that ran fire `load`, inline ones nothing.
#[test]
fn scripts_fire_load_and_error() {
    let page = format!(
        r#"{HEAD}
<script>
for (const type of ["load", "error"])
  document.addEventListener(type, e => {{
    if (e.target.localName === "script") log.push(type + ":" + e.target.id);
  }}, true);
window.addEventListener("load", () => check(log.sort().join() + "|" + window.decoded,
  ["error:404", "error:bad-src", "error:empty", "error:missing-dep", "error:plain",
   "inline ran", "load:classic", "load:ok", "load:wrong-charset"].join() + "|1"));
</script>
<script id="ok" type="module" src="/ok.mjs"></script>
<script id="wrong-charset" type="module" src="/latin1.mjs"></script>
<script id="plain" type="module" src="/plain.mjs"></script>
<script id="missing-dep" type="module" src="/missing-dep.mjs"></script>
<script id="inline" type="module">log.push("inline ran")</script>
<script id="classic" src="/classic.js"></script>
<script id="404" src="/404.js"></script>
<script id="empty" src=""></script>
<script id="bad-src" src="https://[/"></script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/ok.mjs",
            Route::ok("application/javascript; charset=utf-8", ""),
        ),
        // A module is UTF-8, whatever the response says.
        (
            "/latin1.mjs",
            Route::ok(
                "text/javascript; charset=iso-8859-1",
                "window.decoded = 'é'.length;",
            ),
        ),
        (
            "/plain.mjs",
            Route::ok("text/plain", "log.push('plain ran')"),
        ),
        (
            "/missing-dep.mjs",
            js("import './404.mjs'; log.push('missing-dep ran')"),
        ),
        ("/classic.js", js("")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// The `error` HTML queues at a script whose `src` is empty or doesn't
/// parse comes before the window's `load`, which is queued after it (the
/// same task source), however soon the document is done: nothing else
/// delays it here.
#[test]
fn a_scripts_error_comes_before_the_windows_load() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
document.addEventListener("error", e => log.push("error:" + e.target.id), true);
window.addEventListener("load", () => {{
  log.push("load");
  check(log.join(), "error:empty,error:bad-src,load");
}});
</script>
<script id="empty" src=""></script>
<script id="bad-src" src="https://[/"></script>
<body>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// The window's `load` fires once: its task is queued once, however often
/// the loop finds the document loaded since.
#[test]
fn the_windows_load_fires_once() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
let loads = 0;
window.addEventListener("load", () => {{
  loads += 1;
  if (loads === 1) setTimeout(() => check(String(loads), "1"), 300);
}});
</script>
<body>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A module that doesn't parse, imports what isn't there, imports a bare
/// specifier or throws: reported at the window (its script element fires
/// `load`, it fetched fine), and the others go on. The bare specifier's
/// `TypeError` is the renderer's, made outside script: no file.
#[test]
fn errors_in_modules_are_reported_at_the_window() {
    let page = format!(
        r#"{HEAD}
<script>
window.addEventListener("error", e =>
  log.push(e.error.name + ":" + (e.filename.split("/").pop() || "-")));
window.addEventListener("load", () => check(log.join(),
  "SyntaxError:syntax.mjs,SyntaxError:missing-export.mjs,TypeError:-,Error:throws.mjs,ran"));
</script>
<script type="module" src="/syntax.mjs"></script>
<script type="module" src="/missing-export.mjs"></script>
<script type="module">import "lodash";</script>
<script type="module" src="/throws.mjs"></script>
<script type="module">log.push("ran")</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/syntax.mjs", js("export const = ;")),
        (
            "/missing-export.mjs",
            js("import { nope } from './lib.mjs';"),
        ),
        ("/lib.mjs", js("export const yes = 1;")),
        ("/throws.mjs", js("throw new Error('boom');")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A browser that runs modules doesn't run `nomodule` classic scripts.
#[test]
fn nomodule_scripts_do_not_run() {
    let page = format!(
        r#"{HEAD}
<script nomodule>log.push("inline nomodule")</script>
<script nomodule src="/fallback.js"></script>
<script type="module">log.push("module")</script>
<script>window.addEventListener("load", () => check(log.join(), "module"));</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/fallback.js", js("log.push('fallback')")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A redirected module's URL, for `import.meta.url` and its imports, is
/// where it ended up; an inline module's is the document's base URL.
#[test]
fn a_modules_url_is_its_response_url() {
    let page = format!(
        r#"{HEAD}
<base href="/base/">
<script type="module" src="/old.mjs"></script>
<script type="module">log.push(path(import.meta.url));</script>
<script>
window.addEventListener("load", () => check(log.join(), "/new/real.mjs,/new/dep.mjs,/base/"));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/old.mjs", Route::redirect(302, "/new/real.mjs")),
        (
            "/new/real.mjs",
            js("import { url } from './dep.mjs'; log.push(path(import.meta.url), url);"),
        ),
        (
            "/new/dep.mjs",
            js("export const url = path(import.meta.url);"),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A redirected classic script's URL, for its `import()` and its errors,
/// is where it ended up too.
#[test]
fn a_classic_scripts_url_is_its_response_url() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
window.addEventListener("error", e => {{ log.push(path(e.filename)); e.preventDefault(); }});
</script>
<script src="/old.js"></script>
<script>
window.addEventListener("load", () => window.imported.then(where =>
  check(log.concat(where).join(), "/new/real.js,/new/x.mjs")));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/old.js", Route::redirect(302, "/new/real.js")),
        (
            "/new/real.js",
            js("window.imported = import('./x.mjs').then(ns => ns.where); throw new Error('x');"),
        ),
        ("/new/x.mjs", js("export const where = '/new/x.mjs';")),
        ("/x.mjs", js("export const where = '/x.mjs';")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A script that failed fires `error` in its turn: a deferred one after
/// the deferred scripts before it, before those after it; a module graph
/// too. A script element script removed still gets its event.
#[test]
fn script_events_come_in_the_scripts_turn() {
    let page = format!(
        r#"{HEAD}
<script>
document.addEventListener("error", e => log.push("error:" + e.target.id), true);
</script>
<script defer src="/a.js"></script>
<script id="missing" defer src="/missing.js"></script>
<script id="graph" type="module" src="/graph.mjs"></script>
<script defer src="/b.js"></script>
<script id="removed" defer src="/c.js"></script>
<script>
const removed = document.getElementById("removed");
removed.addEventListener("load", () => log.push("load:removed"));
removed.remove();
window.addEventListener("load", () =>
  check(log.join(), "a,error:missing,error:graph,b,c,load:removed"));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        // Slow, so the failures arrive first.
        (
            "/a.js",
            js("log.push('a')").with_delay(std::time::Duration::from_millis(200)),
        ),
        ("/graph.mjs", js("import './missing.mjs';")),
        ("/b.js", js("log.push('b')")),
        ("/c.js", js("log.push('c')")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// `import()`: from a classic script, against its
/// URL; from a module, against the module's; from an inline script and an
/// event handler, against the document's base URL. Each resolves with the
/// namespace, and a module already in the map isn't fetched again.
#[test]
fn import_resolves_against_the_running_script() {
    let page = format!(
        r#"{HEAD}
<base href="/base/">
<script>document.documentElement.classList.add("reftest-wait");</script>
<script src="/scripts/classic.js"></script>
<script type="module" src="/mods/main.mjs"></script>
<script>
const inline = import("./x.mjs").then(ns => ns.where);
const handler = new Promise(resolve => {{
  document.documentElement.setAttribute("onclick", "window.fromHandler = import('./x.mjs')");
  document.documentElement.dispatchEvent(new MouseEvent("click"));
  resolve(window.fromHandler.then(ns => ns.where));
}});
window.addEventListener("load", () => Promise.all([
  window.fromClassic, window.fromModule, inline, handler,
]).then(found => check(found.join(), "/scripts/x.mjs,/mods/x.mjs,/base/x.mjs,/base/x.mjs")));
</script>
<body>"#
    );
    let x = |dir: &str| js(&format!("export const where = '{dir}x.mjs';"));
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/scripts/classic.js",
            js("window.fromClassic = import('./x.mjs').then(ns => ns.where);"),
        ),
        (
            "/mods/main.mjs",
            js("window.fromModule = import('./x.mjs').then(ns => ns.where);"),
        ),
        ("/scripts/x.mjs", x("/scripts/")),
        ("/mods/x.mjs", x("/mods/")),
        ("/base/x.mjs", x("/base/")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let base = server
        .requests()
        .iter()
        .filter(|r| r.target == "/base/x.mjs")
        .count();
    assert_eq!(base, 1, "a module is fetched once");
}

/// A specifier that doesn't resolve rejects before script goes on; a graph
/// that fails to fetch rejects with a `TypeError` naming the URL; a module
/// that throws rejects with what it threw, and isn't reported at the
/// window.
#[test]
fn import_rejects_with_the_error() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
window.addEventListener("error", e => log.push("error event"));
import("lodash").catch(e => log.push("sync " + e.name));
Promise.resolve().then(() => log.push("microtask"));
const results = [
  import("./missing.mjs").catch(e => e.name + ": " + path(e.message.split(": ")[1])),
  import("./throws.mjs").catch(e => "threw " + e.message),
  import("./bad-dep.mjs").catch(e => e.name),
];
window.addEventListener("load", () => Promise.all(results).then(found =>
  check(log.concat(found).join(),
        "sync TypeError,microtask,TypeError: /missing.mjs,threw boom,TypeError")));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/throws.mjs", js("throw new Error('boom');")),
        ("/bad-dep.mjs", js("import './plain.mjs';")),
        ("/plain.mjs", Route::ok("text/plain", "")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// An `import()` doesn't delay the document's `load` (as in Blink): with
/// an image still loading when the import's module is asked for, `load`
/// waits for the image only, and the slower module comes after it.
#[test]
fn import_does_not_delay_the_load_event() {
    let page = format!(
        r#"{HEAD}
<img src="/image.png">
<script>
document.documentElement.classList.add("reftest-wait");
import("./slow.mjs").then(() => {{ log.push("imported"); check(log.join(), "load,imported"); }});
window.addEventListener("load", () => log.push("load"));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/image.png",
            Route::new(404, "").with_delay(std::time::Duration::from_millis(200)),
        ),
        (
            "/slow.mjs",
            js("export {};").with_delay(std::time::Duration::from_millis(600)),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// An iframe's script imports too: its frame's requests are taken with the
/// main frame's, and its module map is its own (the module runs again
/// there, against the srcdoc's base URL, its parent's).
#[test]
fn an_iframe_imports_in_its_own_realm() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
window.runs = 0;
</script>
<script type="module">import "./counted.mjs";</script>
<iframe srcdoc="<script>parent.fromFrame = import('./counted.mjs').then(ns => ns.where);</script>"></iframe>
<script>
window.addEventListener("load", () => window.fromFrame.then(where =>
  check(where + "," + runs, "/counted.mjs,2")));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/counted.mjs",
            js("parent.runs++; export const where = parent.path(import.meta.url);"),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// Two `import()`s of a graph that fails to link, waiting together, each
/// reject with an exception of their own (each links the graph itself);
/// of one with a parse error, with the one error.
#[test]
fn imports_waiting_together_get_their_own_link_errors() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
const errors = [
  import("./bad-link.mjs"), import("./bad-link.mjs"),
  import("./bad-parse.mjs"), import("./bad-parse.mjs"),
].map(promise => promise.catch(e => e));
Promise.all(errors).then(([a, b, c, d]) =>
  check([a.name, a === b, c.name, c === d].join(), "SyntaxError,false,SyntaxError,true"));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/bad-link.mjs", js("import { nope } from './lib.mjs';")),
        ("/lib.mjs", js("export const yes = 1;")),
        ("/bad-parse.mjs", js("export const = ;")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// `<link rel=modulepreload>` (any case, among other tokens) fetches its
/// module alone, not what it imports, and fires `load` once it has (a
/// syntax error is fetched all the same), `error` if the fetch fails or
/// the response isn't JavaScript's. `as` names the destination: a
/// script-like one or none is a module, another (`image`) or a module
/// type machopo hasn't (`json`) is an `error` without a fetch. One without an
/// `href` gets nothing.
#[test]
fn modulepreload_fetches_the_module_alone() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
const expected = [
  "main:load", "gone:error", "text:error", "syntax:load", "image:error",
  "bogus:load", "worker:load", "json:error",
];
const seen = [];
for (const type of ["load", "error"])
  document.addEventListener(type, e => {{
    if (e.target.localName !== "link") return;
    seen.push(e.target.id + ":" + type);
    if (seen.length === expected.length)
      check(seen.sort().join(), expected.sort().join());
  }}, true);
</script>
<link id="main" rel="modulepreload" href="/main.mjs">
<link id="gone" rel="x ModulePreload" href="/gone.mjs">
<link id="text" rel="modulepreload" href="/text.mjs">
<link id="syntax" rel="modulepreload" href="/syntax.mjs">
<link id="image" rel="modulepreload" href="/main.mjs?image" as="image">
<link id="bogus" rel="modulepreload" href="/main.mjs?bogus" as="bogus">
<link id="worker" rel="modulepreload" href="/main.mjs?worker" as="WORKER">
<link id="json" rel="modulepreload" href="/main.mjs?json" as="json">
<link id="none" rel="modulepreload">
<link id="empty" rel="modulepreload" href="">
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/main.mjs", js("import './a.mjs';")),
        ("/a.mjs", js("")),
        ("/gone.mjs", Route::new(404, "")),
        ("/text.mjs", Route::ok("text/plain", "export {};")),
        ("/syntax.mjs", js("export const = ;")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let mut targets: Vec<_> = server
        .requests()
        .into_iter()
        .map(|r| r.target)
        .filter(|t| t != "/")
        .collect();
    targets.sort();
    assert_eq!(
        targets,
        [
            "/gone.mjs",
            "/main.mjs",
            "/main.mjs?bogus",
            "/main.mjs?worker",
            "/syntax.mjs",
            "/text.mjs"
        ]
    );
}

/// What a `<link>` loads starts as it is inserted or changed: one given
/// its `href` after (once, though it changed again), one whose `rel`
/// becomes `stylesheet`, a module preload given its `href` after, one in a
/// shadow tree (as in Chrome).
#[test]
fn a_link_loads_as_it_is_inserted_or_changed() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
const add = (rel, href) => {{
  const link = document.createElement("link");
  link.rel = rel;
  if (href) link.href = href;
  document.head.append(link);
  return link;
}};
const later = add("stylesheet");
const alternate = add("alternate stylesheet", "/alternate.css");
const preload = add("modulepreload");
preload.onload = () => setTimeout(() => check("done", "done"), 50);
const host = document.createElement("div");
document.documentElement.append(host);
host.attachShadow({{mode: "open"}}).innerHTML = '<link rel="stylesheet" href="/shadow.css">';
setTimeout(() => {{
  later.href = "/later.css";
  later.setAttribute("media", "all");
  alternate.rel = "stylesheet";
  preload.href = "/late.mjs";
}});
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/late.mjs", js("")),
        ("/later.css", Route::ok("text/css", "")),
        ("/alternate.css", Route::ok("text/css", "")),
        ("/shadow.css", Route::ok("text/css", "")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let mut targets: Vec<_> = server
        .requests()
        .into_iter()
        .map(|r| r.target)
        .filter(|t| t != "/")
        .collect();
    targets.sort();
    assert_eq!(
        targets,
        ["/alternate.css", "/late.mjs", "/later.css", "/shadow.css"]
    );
}

/// A preload doesn't delay the document's `load` (as in Blink): with an
/// image still loading when the module is asked for, `load` waits for the
/// image only, and the slower module's `load` comes after it.
#[test]
fn modulepreload_does_not_delay_the_load_event() {
    let page = format!(
        r#"{HEAD}
<script>document.documentElement.classList.add("reftest-wait");</script>
<img src="/image.png">
<link rel="modulepreload" href="/slow.mjs"
  onload="log.push('preload'); check(log.join(), 'load,preload')">
<script>window.addEventListener("load", () => log.push("load"));</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/image.png",
            Route::new(404, "").with_delay(std::time::Duration::from_millis(200)),
        ),
        (
            "/slow.mjs",
            js("export {};").with_delay(std::time::Duration::from_millis(600)),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// What a preload fetched is in the module map: an `import()` of it later
/// fetches only what it imports, and runs it once. A `<link>` script
/// inserts is processed too; one for a module the map has fires `load` in
/// a task of its own, after the script that inserted it (a timer's here:
/// no response comes after it).
#[test]
fn modulepreload_fills_the_module_map() {
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
window.runs = 0;
function preload(href) {{
  const link = document.createElement("link");
  link.relList = "modulepreload";
  link.href = href;
  return new Promise((resolve, reject) => {{
    link.onload = resolve;
    link.onerror = reject;
    document.head.append(link);
  }});
}}
preload("/main.mjs")
  .then(() => import("/main.mjs"))
  .then(ns => new Promise(resolve => setTimeout(() => {{
    log.push(ns.value);
    const again = preload("/main.mjs").then(() => log.push("again"));
    log.push("appended");
    resolve(again);
  }})))
  .then(() => check(log.join() + "," + runs, "a,appended,again,1"));
</script>
<body>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/main.mjs",
            js("import { a } from './a.mjs'; runs++; export const value = a;"),
        ),
        ("/a.mjs", js("export const a = 'a';")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let count = |path: &str| {
        server
            .requests()
            .iter()
            .filter(|r| r.target == path)
            .count()
    };
    assert_eq!((count("/main.mjs"), count("/a.mjs")), (1, 1));
}

/// Subresources by CORS from another origin: a module script (and what it
/// imports), a `modulepreload` and an `import()` need the other origin to
/// allow this one, as does a classic script with `crossorigin`; one
/// without it goes as `no-cors`. `crossorigin=use-credentials` (an inline
/// module's for its imports too) asks with credentials, which `*` doesn't
/// allow. Each CORS request has this origin's `Origin`.
#[test]
fn subresources_from_another_origin_go_by_cors() {
    let open = |route: Route| route.with_header("Access-Control-Allow-Origin", "*");
    let other = server(&[
        (
            "/open.mjs",
            open(js("import './open-child.mjs'; log.push('open');")),
        ),
        ("/open-child.mjs", open(js("export {};"))),
        // Slow, so that `deep.mjs`'s import of it finds it on its way:
        // the map keeps no module that failed to fetch, so one asked for
        // after the failure would be fetched again.
        (
            "/closed.mjs",
            js("log.push('closed');").with_delay(std::time::Duration::from_millis(300)),
        ),
        ("/deep.mjs", open(js("import './closed.mjs';"))),
        (
            "/open.js",
            open(js("log.push('classic-' + document.currentScript.id);")),
        ),
        (
            "/closed.js",
            js("log.push('classic-' + document.currentScript.id);"),
        ),
    ]);
    let b = |path: &str| other.url(path);
    // An inline module has no `load` to wait for: it says it ran, so the
    // check waits for it too (it may run after the last event came).
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
const expected = [
  "open:load", "closed:error", "deep:error", "creds:error", "inline-creds:error",
  "classic-cors:load", "classic-closed:error", "classic-nocors:load", "classic-creds:error",
  "preload-open:load", "preload-closed:error", "preload-creds:error",
  "import-open:ok", "import-closed:TypeError", "inline-anon:ran",
];
const seen = [];
const saw = what => {{
  seen.push(what);
  if (seen.length === expected.length)
    check(seen.sort().join() + "|" + log.sort().join(),
          expected.sort().join() + "|classic-classic-cors,classic-classic-nocors,inline-anon,open");
}};
for (const type of ["load", "error"])
  document.addEventListener(type, e => {{
    if (["script", "link"].includes(e.target.localName)) saw(e.target.id + ":" + type);
  }}, true);
for (const [name, url] of [["import-open", "{import_open}"], ["import-closed", "{import_closed}"]])
  import(url).then(() => saw(name + ":ok"), e => saw(name + ":" + e.name));
</script>
<script id="open" type="module" src="{open_mjs}"></script>
<script id="closed" type="module" src="{closed_mjs}"></script>
<script id="deep" type="module" src="{deep_mjs}"></script>
<script id="creds" type="module" src="{creds_mjs}" crossorigin="use-credentials"></script>
<script id="inline-creds" type="module" crossorigin="use-credentials">import "{inline_creds}";</script>
<script id="inline-anon" type="module" crossorigin="anonymous">import "{inline_anon}"; log.push("inline-anon"); saw("inline-anon:ran");</script>
<script id="classic-cors" src="{open_js}" crossorigin></script>
<script id="classic-closed" src="{closed_js}" crossorigin></script>
<script id="classic-nocors" src="{nocors_js}"></script>
<script id="classic-creds" src="{creds_js}" crossorigin="use-credentials"></script>
<link id="preload-open" rel="modulepreload" href="{preload_open}">
<link id="preload-closed" rel="modulepreload" href="{preload_closed}">
<link id="preload-creds" rel="modulepreload" href="{preload_creds}" crossorigin="use-credentials">
<body>"#,
        import_open = b("/open-child.mjs?import"),
        import_closed = b("/closed.mjs?import"),
        open_mjs = b("/open.mjs"),
        closed_mjs = b("/closed.mjs"),
        deep_mjs = b("/deep.mjs"),
        creds_mjs = b("/open-child.mjs?creds"),
        inline_creds = b("/open-child.mjs?inline-creds"),
        inline_anon = b("/open-child.mjs?inline-anon"),
        open_js = b("/open.js"),
        closed_js = b("/closed.js"),
        nocors_js = b("/closed.js?nocors"),
        creds_js = b("/open.js?creds"),
        preload_open = b("/open-child.mjs?preload"),
        preload_closed = b("/closed.mjs?preload"),
        preload_creds = b("/open-child.mjs?preload-creds"),
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let origin = server.url("").trim_end_matches('/').to_owned();
    let mut asked: Vec<(String, Option<String>)> = other
        .requests()
        .into_iter()
        .map(|r| (r.target.clone(), r.header("origin").map(str::to_owned)))
        .collect();
    asked.sort();
    let cors = |target: &str| (target.to_owned(), Some(origin.clone()));
    assert_eq!(
        asked,
        [
            cors("/closed.js"),
            ("/closed.js?nocors".to_owned(), None),
            // `deep.mjs` imports the one `closed` asked for: in the map.
            cors("/closed.mjs"),
            cors("/closed.mjs?import"),
            cors("/closed.mjs?preload"),
            cors("/deep.mjs"),
            cors("/open-child.mjs"),
            cors("/open-child.mjs?creds"),
            cors("/open-child.mjs?import"),
            cors("/open-child.mjs?inline-anon"),
            cors("/open-child.mjs?inline-creds"),
            cors("/open-child.mjs?preload"),
            cors("/open-child.mjs?preload-creds"),
            cors("/open.js"),
            cors("/open.js?creds"),
            cors("/open.mjs"),
        ]
    );
}

/// A `file:` page's modules are blocked, as in Chrome: its origin is
/// opaque, and CORS is only for HTTP(S). A `data:` module still runs, and
/// a classic script from a file still loads.
#[test]
fn a_file_pages_modules_are_blocked() {
    let dir = std::env::temp_dir().join(format!("machopo-file-modules-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("m.mjs"), "log.push('module');").unwrap();
    std::fs::write(dir.join("c.js"), "log.push('classic');").unwrap();
    let page = format!(
        r#"{HEAD}
<script>
document.documentElement.classList.add("reftest-wait");
const expected = ["classic:load", "data:load", "module:error", "preload:error"];
const seen = [];
for (const type of ["load", "error"])
  document.addEventListener(type, e => {{
    if (!["script", "link"].includes(e.target.localName)) return;
    seen.push(e.target.id + ":" + type);
    if (seen.length === expected.length)
      check(seen.sort().join() + "|" + log.sort().join(), expected.join() + "|classic,data");
  }}, true);
</script>
<script id="module" type="module" src="m.mjs"></script>
<script id="data" type="module" src="data:text/javascript,log.push('data')"></script>
<link id="preload" rel="modulepreload" href="m.mjs?preload">
<script id="classic" src="c.js"></script>
<body>"#
    );
    let page_path = dir.join("page.html");
    std::fs::write(&page_path, page).unwrap();
    let frame = render(net::file_url(&page_path).unwrap().as_str());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}
