//! Stylesheets in a whole browser: the sheets a document adopts style it,
//! after its own, and every change to them (what it adopts, a sheet's
//! rules or `disabled`) shows in the next style and layout that script
//! reads; and what `url()`s resolve against. Sheets that come and go after
//! load (an icon's `<style>` at a time) style what their rules match. A
//! `<style>`'s sheet edited by script (a CSS-in-JS library's way) styles
//! the page; `<link>`s' sheets from this origin and others, by CORS or
//! not, and what script may read of them. Each page reports what it read
//! through `testRunner` (`headless::run_testharness`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-style-sheets");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// What the page at `url` reports.
fn report(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

/// Each step reads the box's width and its color: the document's own sheet
/// gives 10px; an adopted sheet's rule of the same specificity wins; a
/// sheet that isn't adopted, is disabled, or only a shadow root adopts
/// doesn't count.
#[test]
fn adopted_sheets_style_the_document() {
    let url = page(
        "adopted.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } #t { width: 10px; height: 5px; color: rgb(1, 1, 1) }</style>
<div id="t"></div><div id="other"></div>
<script>
onload = () => {
  const log = [];
  const t = document.getElementById('t');
  const read = () => log.push(`${t.getBoundingClientRect().width} ${getComputedStyle(t).color}`);
  const sheet = new CSSStyleSheet();
  sheet.replaceSync('#t { width: 20px }');
  read();
  document.adoptedStyleSheets = [sheet];
  read();
  sheet.insertRule('#t { color: rgb(2, 2, 2) }');
  read();
  sheet.cssRules[1].style.width = '30px';
  read();
  sheet.cssRules[1].selectorText = '#nothing';
  read();
  sheet.replaceSync('#t { width: 25px }');
  read();
  sheet.disabled = true;
  read();
  sheet.disabled = false;
  const later = new CSSStyleSheet();
  later.replaceSync('#t { width: 40px }');
  document.adoptedStyleSheets.push(later);
  read();
  document.adoptedStyleSheets = [later, sheet];
  read();
  document.adoptedStyleSheets.length = 0;
  read();
  document.getElementById('other').attachShadow({ mode: 'open' }).adoptedStyleSheets = [later];
  read();
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "10 rgb(1, 1, 1) / 20 rgb(1, 1, 1) / 20 rgb(2, 2, 2) / 30 rgb(2, 2, 2) / \
         10 rgb(2, 2, 2) / 25 rgb(1, 1, 1) / 10 rgb(1, 1, 1) / 40 rgb(1, 1, 1) / \
         25 rgb(1, 1, 1) / 10 rgb(1, 1, 1) / 10 rgb(1, 1, 1)"
    );
}

/// A child frame's sheets whose changes its parent's style sees first: a
/// shadow tree's `<style>` and adopted sheet, and a sheet the frame's
/// document adopts changed by script, each style the frame's document,
/// though the parent is styled in between (each document takes its own).
#[test]
fn a_child_frames_changed_sheets_style_it() {
    let url = page(
        "child-frame-sheets.html",
        r#"<!DOCTYPE html>
<iframe id="f" srcdoc="<div id=host></div><div id=t></div>"></iframe>
<script>
onload = () => {
  const log = [];
  const doc = f.contentDocument, view = f.contentWindow;
  const width = element => { document.body.offsetWidth; return view.getComputedStyle(element).width; };
  const root = doc.getElementById('host').attachShadow({ mode: 'open' });
  root.innerHTML = '<style>p { width: 10px }</style><p></p><i></i>';
  log.push(width(root.querySelector('p')));
  const sheet = new view.CSSStyleSheet();
  sheet.replaceSync('i { width: 20px } #t { width: 30px }');
  root.adoptedStyleSheets = [sheet];
  doc.adoptedStyleSheets = [sheet];
  log.push(width(root.querySelector('i')), width(doc.getElementById('t')));
  sheet.replaceSync('i { width: 21px } #t { width: 31px }');
  log.push(width(root.querySelector('i')), width(doc.getElementById('t')));
  testRunner.reportHarnessResults(log.join(' '));
};
</script>"#,
    );
    assert_eq!(report(&url), "10px 20px 30px 21px 31px");
}

/// A `url()` resolves as it is parsed, as in Chrome: a `style` attribute's
/// against the document's base URL of then (a `<base>` inserted later
/// doesn't change it; `element.style` set after does, and the same text set
/// again changes nothing), a constructed sheet's rules against the base URL
/// of when it was made, `animate()`'s keyframes against the document's,
/// and an `about:blank` or `srcdoc` frame's against its parent's.
#[test]
fn urls_resolve_as_they_are_parsed() {
    let url = page(
        "urls.html",
        r#"<!DOCTYPE html>
<div id="a" style="background-image: url(a.png)"></div><div id="c"></div><div id="d"></div><div id="e"></div>
<script>
onload = () => {
  const log = [];
  const dir = new URL('.', location).href;
  const bg = (element, view = window) => view.getComputedStyle(element).backgroundImage.replace(dir, '');
  const [a, c, d, e] = ['a', 'c', 'd', 'e'].map(id => document.getElementById(id));
  const base = document.createElement('base');
  base.setAttribute('href', 'sub/');
  document.head.append(base);
  log.push(bg(a));
  a.style.backgroundImage = 'url(b.png)';
  log.push(bg(a));
  const records = [];
  new MutationObserver(found => records.push(...found)).observe(a, { attributes: true });
  a.style.setProperty('background-image', 'url(b.png)');
  c.style.cssText = 'background-image: url(t.png)';
  log.push(bg(c));
  c.removeAttribute('style');
  const sheet = new CSSStyleSheet();
  base.setAttribute('href', 'other/');
  sheet.replaceSync('#c { background-image: url(c.png) }');
  document.adoptedStyleSheets = [sheet];
  log.push(bg(c));
  sheet.insertRule('#d { background-image: url(d.png) }');
  log.push(bg(d));
  sheet.cssRules[1].style.backgroundImage = 'url(f.png)';
  log.push(bg(c));
  e.animate([{ backgroundImage: 'url(k.png)' }, { backgroundImage: 'url(k.png)' }], 100000);
  log.push(bg(e));
  const blank = document.createElement('iframe');
  document.body.append(blank);
  blank.contentDocument.body.innerHTML = '<div id="i" style="background-image: url(i.png)"></div>';
  const frame = document.createElement('iframe');
  frame.srcdoc = '<div id="s" style="background-image: url(s.png)"></div>';
  frame.onload = () => {
    log.push(bg(blank.contentDocument.getElementById('i'), blank.contentWindow));
    const s = frame.contentDocument.getElementById('s');
    log.push(bg(s, frame.contentWindow));
    log.push(records.length);
    testRunner.reportHarnessResults(log.join(' / '));
  };
  document.body.append(frame);
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        r#"url("a.png") / url("sub/b.png") / url("sub/t.png") / url("sub/c.png") / url("sub/d.png") / url("sub/f.png") / url("other/k.png") / url("other/i.png") / url("other/s.png") / 0"#
    );
}

/// Sheets that come after load, as a framework's icons bring theirs (an
/// inline SVG with a `<style>` of one rule each), and go: what their rules
/// match takes them, by a class, a tag and a subtree, and nothing else
/// changes.
#[test]
fn sheets_that_come_and_go_style_what_they_match() {
    let url = page(
        "come-and-go.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } .box { width: 10px; height: 5px }</style>
<div id="list"><div class="box" id="a"></div><p id="p"><i id="i"></i></p></div>
<script>
onload = () => {
  const log = [];
  const list = document.getElementById('list');
  const fill = (id) => getComputedStyle(document.getElementById(id)).fill;
  const width = (id) => getComputedStyle(document.getElementById(id)).width;
  const color = (id) => getComputedStyle(document.getElementById(id)).color;
  for (const [n, color] of [[1, 'rgb(1, 2, 3)'], [2, 'rgb(4, 5, 6)']]) {
    list.insertAdjacentHTML('beforeend',
        `<svg><style>.icon${n} { fill: ${color} }</style><path id="path${n}" class="icon${n}"/></svg>`);
    log.push(fill(`path${n}`));
  }
  const wide = document.createElement('style');
  wide.textContent = '#list > p * { color: rgb(7, 8, 9) } .box { width: 30px }';
  document.head.append(wide);
  log.push(width('a'), color('i'), color('p'), fill('path1'));
  wide.remove();
  log.push(width('a'), color('i'));
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "rgb(1, 2, 3) / rgb(4, 5, 6) / 30px / rgb(7, 8, 9) / rgb(0, 0, 0) / rgb(1, 2, 3) / 10px / rgb(0, 0, 0)"
    );
}

/// A `<style>`'s sheet edited through `sheet` styles the page, as a
/// CSS-in-JS library's production build does (emotion's
/// `insertRule(rule, sheet.cssRules.length)`): rules inserted, a rule's
/// `style`, the sheet's `media` and `disabled`. The element's text changing
/// makes a new sheet, and the edits are gone.
#[test]
fn a_style_elements_sheet_edited_by_script_styles_the_page() {
    let url = page(
        "css-in-js.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 10px; height: 5px }</style>
<div id="t" class="css-1"></div>
<script>
onload = () => {
  const log = [];
  const t = document.getElementById('t');
  const read = () => log.push(`${t.getBoundingClientRect().width} ${getComputedStyle(t).color}`);
  const style = document.head.appendChild(document.createElement('style'));
  const sheet = style.sheet;
  sheet.insertRule('.css-1 { width: 20px }', sheet.cssRules.length);
  read();
  sheet.insertRule('.css-2 { color: rgb(1, 2, 3) }', sheet.cssRules.length);
  t.className = 'css-1 css-2';
  read();
  sheet.cssRules[0].style.width = '30px';
  read();
  sheet.media.mediaText = 'print';
  read();
  style.media = 'screen';
  read();
  sheet.disabled = true;
  read();
  style.disabled = false;
  log.push(String(sheet === style.sheet), String(document.styleSheets[1] === sheet));
  style.textContent = '.css-1 { width: 40px }';
  read();
  log.push(String(sheet.ownerNode), String(style.sheet === sheet), String(style.sheet.cssRules.length));
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "20 rgb(0, 0, 0) / 20 rgb(1, 2, 3) / 30 rgb(1, 2, 3) / 10 rgb(0, 0, 0) / \
         30 rgb(1, 2, 3) / 10 rgb(0, 0, 0) / true / true / 40 rgb(0, 0, 0) / null / false / 1"
    );
}

fn server(routes: &[(&str, Route)]) -> TestServer {
    TestServer::start(
        routes
            .iter()
            .map(|(path, route)| (path.to_string(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("test server starts")
}

fn css(body: &str) -> Route {
    Route::ok("text/css", body)
}

/// `<link>`s' sheets (Chrome 152's): one from another origin without CORS
/// applies, but script can't read its rules (`SecurityError`); with
/// `crossorigin`, one the origin allows by CORS is readable, one it
/// doesn't fails (no rules). A sheet that failed (a 404) is there, with no
/// rules. `href` is the URL asked for, before a redirect. An alternate one
/// loads, is listed, but doesn't apply.
#[test]
fn link_sheets_from_this_origin_and_others() {
    let other = server(&[
        ("/closed.css", css("#p2 { color: rgb(2, 2, 2) }")),
        (
            "/open.css",
            css("#p3 { color: rgb(3, 3, 3) }").with_header("Access-Control-Allow-Origin", "*"),
        ),
        ("/closed-cors.css", css("#p4 { color: rgb(4, 4, 4) }")),
    ]);
    let page = format!(
        r#"<!DOCTYPE html>
<link id="same" rel="stylesheet" href="/a.css">
<link id="cross" rel="stylesheet" href="{closed}">
<link id="cors" rel="stylesheet" crossorigin href="{open}">
<link id="failed-cors" rel="stylesheet" crossorigin href="{closed_cors}">
<link id="missing" rel="stylesheet" href="/missing.css">
<link id="redirect" rel="stylesheet" href="/redirect.css">
<link id="alternate" rel="alternate stylesheet" href="/alternate.css">
<p id="p1"></p><p id="p2"></p><p id="p3"></p><p id="p4"></p><p id="p5"></p><p id="p6"></p>
<script>
onload = () => {{
  const rules = sheet => {{
    try {{ return sheet.cssRules.length; }} catch (e) {{ return e.name; }}
  }};
  const log = [...document.styleSheets].map(
      sheet => `${{sheet.ownerNode.id}} ${{rules(sheet)}} ${{new URL(sheet.href).pathname}}`);
  log.push([1, 2, 3, 4, 5, 6].map(n => getComputedStyle(document.getElementById('p' + n)).color).join());
  testRunner.reportHarnessResults(log.join(' / '));
}};
</script>"#,
        closed = other.url("/closed.css"),
        open = other.url("/open.css"),
        closed_cors = other.url("/closed-cors.css"),
    );
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        ("/a.css", css("#p1 { color: rgb(1, 1, 1) }")),
        ("/redirect.css", Route::redirect(302, "/target.css")),
        ("/target.css", css("#p6 { color: rgb(6, 6, 6) }")),
        ("/alternate.css", css("#p5 { color: rgb(5, 5, 5) }")),
    ]);
    assert_eq!(
        report(&server.url("/")),
        "same 1 /a.css / cross SecurityError /closed.css / cors 1 /open.css / \
         failed-cors SecurityError /closed-cors.css / missing 0 /missing.css / \
         redirect 1 /redirect.css / alternate 1 /alternate.css / \
         rgb(1, 1, 1),rgb(2, 2, 2),rgb(3, 3, 3),rgb(0, 0, 0),rgb(0, 0, 0),rgb(6, 6, 6)"
    );
}

/// The parser's scripts wait for a `<link>` stylesheet it inserted before
/// them, not for an alternate one, which blocks nothing (Chrome's loads at
/// the lowest priority).
#[test]
fn an_alternate_sheet_keeps_no_script_waiting() {
    let page = r#"<!DOCTYPE html>
<link rel="alternate stylesheet" href="/alternate.css">
<script>window.atAlternate = document.styleSheets.length;</script>
<link rel="stylesheet" href="/sheet.css">
<script>window.atSheet = document.styleSheets.length;</script>
<script>
onload = () => testRunner.reportHarnessResults(`${atAlternate} ${atSheet >= 1}`);
</script>"#;
    let server = server(&[
        ("/", Route::ok("text/html", page)),
        (
            "/alternate.css",
            css("p {}").with_delay(Duration::from_secs(3)),
        ),
        (
            "/sheet.css",
            css("p {}").with_delay(Duration::from_millis(500)),
        ),
    ]);
    assert_eq!(report(&server.url("/")), "0 true");
}
