//! Script elements script inserts, in a whole browser, over HTTP from a
//! local test server: prepared as the call that inserts them ends, they
//! load and run as soon as they can, or in the order they were inserted
//! with `async = false`; they fire `load` and `error`; one moved to another
//! document before its turn does nothing; those markup makes never run.
//! And `document.currentScript`.
//!
//! Each page paints a green square at (10, 10) if what it checked holds,
//! red otherwise; it writes what it saw into the square's `title`, which
//! the assertion's message can't show, so the check is spelled out in the
//! page. A page that checks after `load` puts `reftest-wait` on the root
//! first: the frame is taken once the check removes it.

use std::collections::HashMap;
use std::time::Duration;

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

/// The start of each page: `log`, `check(seen, expected)`, which paints
/// the square, and `insert(src, setup)`, which inserts a script element
/// from `src` (after `setup(script)`) and resolves with its event's type.
const HEAD: &str = r#"<!DOCTYPE html>
<html class="reftest-wait">
<style>html, body { margin: 0 } #result { width: 20px; height: 20px }</style>
<script>
window.log = [];
window.check = (seen, expected) => {
  const result = document.createElement('div');
  result.id = 'result';
  result.title = seen;
  result.setAttribute('style', 'background-color: ' + (seen === expected ? 'green' : 'red'));
  document.body.prepend(result);
  document.documentElement.classList.remove('reftest-wait');
};
window.insert = (src, setup) => new Promise(resolve => {
  const script = document.createElement('script');
  script.id = src;
  if (setup) setup(script);
  if (src !== null) script.src = src;
  script.onload = () => resolve(src + ':load');
  script.onerror = () => resolve(src + ':error');
  document.head.append(script);
});
</script>
"#;

fn js(body: &str) -> Route {
    Route::ok("text/javascript", body)
}

fn slow(route: Route) -> Route {
    route.with_delay(Duration::from_millis(300))
}

/// Inserted scripts run as soon as they have loaded, whatever their order
/// (`a.js` loads slower than `b.js`); with `async = false`, in the order
/// they were inserted (`c.js`, slower, before `d.js`). Each fires `load`,
/// or `error` if it doesn't load (404, an empty `src`); a module runs too,
/// as soon as it can. `currentScript` is a classic script's element while
/// it runs, `null` in a module.
#[test]
fn inserted_scripts_run_as_soon_as_they_can_or_in_order() {
    let page = format!(
        r#"{HEAD}<body>
<script>
const inOrder = s => {{ s.async = false; }};
Promise.all([
  insert('/a.js'), insert('/b.js'),
  insert('/c.js', inOrder), insert('/d.js', inOrder),
  insert('/m.mjs', s => {{ s.type = 'module'; }}),
  insert('/gone.js'), insert('', null),
]).then(events => {{
  const before = (x, y) => log.indexOf(x) >= 0 && log.indexOf(x) < log.indexOf(y);
  check([log.length, before('b', 'a'), before('c', 'd'), log.includes('m'), events.join()].join(),
    '5,true,true,true,' +
    '/a.js:load,/b.js:load,/c.js:load,/d.js:load,/m.mjs:load,/gone.js:error,:error');
}});
</script>"#
    );
    let current = "document.currentScript === document.getElementById(document.currentScript.id)";
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/a.js", slow(js(&format!("if ({current}) log.push('a');")))),
        ("/b.js", js(&format!("if ({current}) log.push('b');"))),
        ("/c.js", slow(js(&format!("if ({current}) log.push('c');")))),
        ("/d.js", js(&format!("if ({current}) log.push('d');"))),
        (
            "/m.mjs",
            js("if (document.currentScript === null) log.push('m');"),
        ),
        ("/gone.js", Route::new(404, "")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A script inserted before `load` delays it: the slow one runs first.
#[test]
fn inserted_scripts_delay_the_load_event() {
    let page = format!(
        r#"{HEAD}<body>
<script>
insert('/slow.js');
window.addEventListener('load', () => check(log.join(), 'slow'));
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/slow.js", slow(js("log.push('slow');"))),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// Scripts that never run: one markup makes (`innerHTML`), a clone of one
/// that ran (already started), one moved to another document before its
/// turn (no `load` either), one of a type that doesn't run. The ones
/// after them do run, so their turn has come.
#[test]
fn some_inserted_scripts_never_run() {
    let page = format!(
        r#"{HEAD}<body>
<script id="parsed">log.push('parsed');</script>
<script>
const div = document.createElement('div');
div.innerHTML = '<script src="/never.js"><\/script>';
document.body.append(div);
document.body.append(document.getElementById('parsed').cloneNode(true));
const moved = document.createElement('script');
moved.src = '/moved.js';
moved.onload = moved.onerror = () => log.push('moved event');
document.head.append(moved);
document.implementation.createHTMLDocument('').body.append(moved);
insert('/plain.txt', s => {{ s.type = 'text/plain'; }});
insert('/last.js', s => {{ s.async = false; }}).then(() =>
  setTimeout(() => check(log.join(), 'parsed,last'), 400));
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/never.js", js("log.push('never');")),
        ("/moved.js", slow(js("log.push('moved');"))),
        ("/plain.txt", js("log.push('plain');")),
        ("/last.js", js("log.push('last');")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
    let requested = |path: &str| server.requests().iter().any(|r| r.target == path);
    assert!(!requested("/never.js"), "markup's scripts aren't fetched");
    assert!(
        !requested("/plain.txt"),
        "nor scripts of a type that doesn't run"
    );
}

/// A range's `createContextualFragment` makes scripts that run once
/// inserted (inline at once, nested ones too, from `src` once loaded),
/// unlike `innerHTML`'s.
#[test]
fn contextual_fragments_scripts_run() {
    let page = format!(
        r#"{HEAD}<body>
<script>
const range = document.createRange();
range.selectNodeContents(document.body);
const fragment = range.createContextualFragment(
  '<script>log.push("inline");<\/script><div><script>log.push("nested");<\/script></div>' +
  '<script src="/external.js"><\/script>');
log.push('parsed');
document.body.append(fragment);
const div = document.createElement('div');
div.innerHTML = '<script>log.push("innerHTML");<\/script>';
document.body.append(div);
setTimeout(() => check(log.join(), 'parsed,inline,nested,external'), 400);
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/external.js", js("log.push('external');")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A shadow tree is connected with its host: a script inserted into a
/// connected one runs (inline at once, from `src` once loaded), and so does
/// one in a host's shadow tree as the host is inserted; one in a shadow
/// tree of a host out of the document doesn't, nor does one in a template's
/// content in a shadow tree.
#[test]
fn scripts_in_shadow_trees_run() {
    let page = format!(
        r#"{HEAD}<body>
<script>
const host = document.body.appendChild(document.createElement('div'));
const root = host.attachShadow({{ mode: 'open' }});
const inline = document.createElement('script');
inline.textContent = "log.push('inline')";
root.append(inline);
const later = document.createElement('div');
const closed = later.attachShadow({{ mode: 'closed' }});
const loaded = document.createElement('script');
loaded.src = '/loaded.js';
loaded.onload = () => setTimeout(() => check(log.join(), 'inline,out,loaded'), 100);
closed.append(loaded);
const away = document.createElement('div');
const unconnected = document.createElement('script');
unconnected.textContent = "log.push('never')";
away.attachShadow({{ mode: 'open' }}).append(unconnected);
const template = document.createElement('template');
root.append(template);
const inert = document.createElement('script');
inert.textContent = "log.push('inert')";
template.content.append(inert);
log.push('out');
document.body.append(later);
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/loaded.js", js("log.push('loaded');")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A script inserted into an iframe's document runs in the iframe's
/// realm, from its document's frame: a `srcdoc` one's, and the initial
/// `about:blank` of one without `src`. `currentScript` is per document:
/// the parent's script isn't running when they do.
#[test]
fn a_script_inserted_into_an_iframe_runs_there() {
    let page = format!(
        r#"{HEAD}<body>
<iframe srcdoc="<p>child</p>"></iframe>
<iframe></iframe>
<script>
window.addEventListener('load', () => {{
  const runs = [...document.querySelectorAll('iframe')].map((iframe, i) => {{
    const child = iframe.contentDocument;
    const script = child.createElement('script');
    script.src = '/framed.js?' + i;
    const ran = new Promise(resolve => {{ script.onload = resolve; }});
    child.body.append(script);
    return ran;
  }});
  Promise.all(runs).then(() =>
    check(log.join() + ',' + (document.currentScript === null), 'child,child,true'));
}});
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/framed.js",
            js("parent.log.push(window === parent ? 'parent' : 'child');"),
        ),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// An inline module script inserts has nothing to load, so nothing comes
/// back that would run it: it runs at the top of the event loop, after
/// the timer's callback that inserted it.
#[test]
fn an_inserted_inline_module_runs() {
    let page = format!(
        r#"{HEAD}<body>
<script>
window.addEventListener('load', () => setTimeout(() => {{
  const module = document.createElement('script');
  module.type = 'module';
  module.textContent = "log.push('module'); check(log.join(), 'inserted,module');";
  document.body.append(module);
  log.push('inserted');
}}));
</script>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// `currentScript` for the parser's scripts: an inline classic one's own
/// element, an external one's (gone once it is done: `null` in its `load`
/// listener); `null` in a timer callback and in an inline module.
#[test]
fn current_script_for_the_parsers_scripts() {
    let page = format!(
        r#"{HEAD}<body>
<script id="first">log.push(document.currentScript.id);</script>
<script id="external" src="/external.js"
  onload="log.push(String(document.currentScript))"></script>
<script id="second">
log.push(document.currentScript.id);
setTimeout(() => log.push(String(document.currentScript)));
</script>
<script type="module">log.push(String(document.currentScript));</script>
<script>
window.addEventListener('load', () => setTimeout(() =>
  check(log.join(), 'first,external,null,second,null,null')));
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/external.js", js("log.push(document.currentScript.id);")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A script inserted into an iframe's document, the iframe removed in the
/// same task: its frame goes with what it was to fetch (the element's
/// root with it, before the DOM), and nothing runs.
#[test]
fn a_removed_frame_drops_its_inserted_scripts() {
    let page = format!(
        r#"{HEAD}<body>
<iframe></iframe>
<script>
window.addEventListener('load', () => {{
  const iframe = document.querySelector('iframe');
  const script = iframe.contentDocument.createElement('script');
  script.src = '/gone.js';
  iframe.contentDocument.body.append(script);
  iframe.remove();
  setTimeout(() => check(log.join(), ''), 300);
}});
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/gone.js", js("parent.log.push('gone');")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// An inline classic script script inserts runs before the call that
/// inserted it returns, `currentScript` its element; one inserted empty
/// runs when text is appended to it. Its microtasks wait for the script
/// that inserted it (no checkpoint in between); what it throws goes to
/// `window.onerror`, not to the script that inserted it.
#[test]
fn an_inserted_inline_script_runs_at_once() {
    let page = format!(
        r#"{HEAD}<body>
<script>
window.onerror = message => {{ log.push('onerror'); return true; }};
const inline = document.createElement('script');
inline.id = 'inline';
inline.textContent = "log.push('ran ' + document.currentScript.id); " +
  "queueMicrotask(() => log.push('microtask'));";
document.body.append(inline);
log.push('appended');
const empty = document.createElement('script');
document.body.append(empty);
log.push('empty');
empty.append("log.push('late')");
const thrower = document.createElement('script');
thrower.textContent = "throw new Error('x')";
document.body.append(thrower);
log.push('after throw');
queueMicrotask(() => check(log.join(),
  'ran inline,appended,empty,late,onerror,after throw,microtask'));
</script>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// An inline script inserted into an iframe's document runs in the
/// iframe's realm; its `import()` resolves against the document's base
/// URL as it is then: an `about:srcdoc` one's parent's (its fallback base
/// URL), and a `<base>` script inserted just before counts. What one
/// throws is reported at the iframe's window.
#[test]
fn an_inserted_inline_script_runs_in_its_documents_realm() {
    let page = format!(
        r#"{HEAD}<body>
<iframe srcdoc="<p>child</p>"></iframe>
<script>
window.addEventListener('load', () => {{
  const child = document.querySelector('iframe').contentDocument;
  const script = child.createElement('script');
  script.textContent = "parent.log.push(window === parent ? 'parent' : 'child'); " +
    "import('./c.mjs').then(ns => parent.log.push(ns.where)); " +
    "window.onerror = () => {{ parent.log.push('child onerror'); return true; }};";
  // This realm's `append`: the call runs in this realm, the scripts in
  // the iframe's all the same.
  Element.prototype.append.call(child.body, script);
  window.onerror = () => {{ log.push('parent onerror'); return true; }};
  const thrower = child.createElement('script');
  thrower.textContent = "throw new Error('in the child')";
  Element.prototype.append.call(child.body, thrower);
  const base = document.createElement('base');
  base.setAttribute('href', '/sub/');
  document.head.append(base);
  const importer = document.createElement('script');
  importer.textContent = "import('./m.mjs').then(ns => {{ log.push(ns.where); " +
    "setTimeout(() => check(log.sort().join(), '/c.mjs,/sub/m.mjs,child,child onerror'), 100); }});";
  document.body.append(importer);
}});
</script>"#
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/sub/m.mjs", js("export const where = '/sub/m.mjs';")),
        ("/c.mjs", js("export const where = '/c.mjs';")),
    ]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// In one insertion, iframes get their browsing contexts and inline
/// scripts run in tree order (DOM's post-connection steps): a script
/// before an iframe sees no `contentWindow`, one after it sees it.
#[test]
fn iframes_and_inline_scripts_take_their_turns_in_tree_order() {
    let page = format!(
        r#"{HEAD}<body>
<script>
const div = document.createElement('div');
const before = document.createElement('script');
before.textContent = "log.push(String(div.querySelector('iframe').contentWindow !== null));";
const after = before.cloneNode(true);
div.append(before, document.createElement('iframe'), after);
document.body.append(div);
check(log.join(), 'false,true');
</script>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}

/// A form owner is reset as the tree changes (HTML's insertion steps),
/// before the scripts the same insertion connected run: one inserted
/// before a button in a div sees the button's form. And the parser
/// associates a control with its form element pointer, which needn't be
/// an ancestor (`<div><form></div><input>`; the `</form>` clears the
/// pointer, which would have the next `<form>` ignored).
#[test]
fn inserted_scripts_see_the_form_owners() {
    let page = format!(
        r#"{HEAD}<body>
<div><form id=f></div><input id=i></form>
<form id=g></form>
<script>
window.buttonForm = 'not run';
const script = document.createElement('script');
script.textContent = 'buttonForm = button.form';
window.button = document.createElement('button');
const div = document.createElement('div');
div.append(script, button);
const g = document.getElementById('g');
g.append(div);
const i = document.getElementById('i');
check([buttonForm === g, i.form === document.getElementById('f')].join(), 'true,true');
</script>"#
    );
    let server = server(&[("/", Route::ok("text/html", page))]);
    let frame = render(&server.url("/"));
    assert_eq!(pixel(&frame, 10, 10), GREEN);
}
