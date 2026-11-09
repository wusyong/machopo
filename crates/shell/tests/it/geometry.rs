//! CSSOM View's geometry in a whole browser: script reads the
//! renderer's layout, brought up to date as it asks. Each page reports what
//! it read through `testRunner` (`headless::run_testharness`): elements'
//! rects as the viewport shows them, the viewport, a document scrolled to
//! its fragment, child frames (their own viewports, the parent laid out
//! first, one just inserted, one without a box, their media queries and
//! first style), a document whose stylesheet is still loading, elements
//! in a shadow tree; and SVG elements' geometry, as Chrome 152 gives it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use gfx::Size;
use net::test_server::{Route, TestServer};
use protocol::input::{InputEvent, Modifiers, MouseButton};
use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-geometry");
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

/// Rects in viewport coordinates, from the layout as script left it
/// (`display: none`, a detached element, a style just changed); the
/// window's viewport (headless: 800×600 at 1x).
#[test]
fn elements_and_the_viewport() {
    let url = page(
        "elements.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div id="box" style="margin-left: 10px; width: 100px; height: 50px"></div>
<div id="moved" style="width: 10px; height: 10px; transform: translate(5px, 6px)"></div>
<div style="width: 50px"><span id="span">aaaa bbbb cccc</span></div>
<div id="none" style="display: none"></div>
<script>
onload = () => {
  const log = [];
  const rect = r => `${r.x},${r.y},${r.width},${r.height}`;
  const box = document.getElementById('box');
  log.push(rect(box.getBoundingClientRect()), box.getClientRects().length);
  log.push(rect(document.getElementById('moved').getBoundingClientRect()));
  // An inline box on each line; the bounding rect is around them all.
  const span = document.getElementById('span');
  const lines = [...span.getClientRects()];
  const b = span.getBoundingClientRect();
  log.push(lines.length > 1,
           b.left === Math.min(...lines.map(r => r.left)),
           b.bottom === Math.max(...lines.map(r => r.bottom)));
  const none = document.getElementById('none');
  log.push(none.getClientRects().length, rect(none.getBoundingClientRect()));
  log.push(rect(document.createElement('div').getBoundingClientRect()));
  // Changed by script: laid out again as script asks.
  box.style.width = '200px';
  log.push(box.getBoundingClientRect().width);
  log.push(innerWidth, innerHeight, devicePixelRatio, scrollX, scrollY);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "10,0,100,50 / 1 / 5,56,10,10 / true / true / true / 0 / 0,0,0,0 / 0,0,0,0 / 200 / \
         800 / 600 / 1 / 0 / 0"
    );
}

/// A document scrolled to its `#fragment`: the rects are the viewport's,
/// less the scroll offset.
#[test]
fn a_scrolled_document() {
    let url = page(
        "scrolled.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div style="height: 1000px"></div>
<div id="target" style="height: 2000px"></div>
<script>
onload = () => {
  const target = document.getElementById('target').getBoundingClientRect();
  testRunner.reportHarnessResults([scrollY, pageYOffset, target.top].join(' / '));
};
</script>"#,
    );
    assert_eq!(report(&format!("{url}#target")), "1000 / 1000 / 0");
}

/// A sticky box is where it stuck as its scrollport scrolls, for its
/// rects and `offsetTop`, at once as script scrolls; its insets resolve
/// against its scrollport, not its parent, and an `auto` one stays `auto`;
/// what is in it, an SVG element's geometry too, moves with it. A table
/// cell doesn't stick, but its insets are still resolved.
#[test]
fn sticky_boxes() {
    let url = page(
        "sticky.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div id="scroller" style="overflow: auto; width: 300px; height: 200px">
  <div style="height: 100px"></div>
  <div style="height: 400px">
    <div id="s" style="position: sticky; top: 10%; height: 20px"></div>
  </div>
  <div style="height: 1000px"></div>
</div>
<div id="nav" style="position: sticky; top: 5px; height: 30px"><svg width="10" height="10" style="display: block"><rect id="r" y="2" width="10" height="5"/></svg></div>
<table><tr><td id="td" style="position: sticky; top: 10px">x</td></tr></table>
<div style="height: 3000px"></div>
<script>
onload = () => {
  const log = [];
  const s = document.getElementById('s');
  log.push(getComputedStyle(document.getElementById('td')).top);
  const nav = document.getElementById('nav');
  log.push(getComputedStyle(s).top, getComputedStyle(s).bottom);
  log.push(s.getBoundingClientRect().top, s.offsetTop);
  document.getElementById('scroller').scrollTop = 150;
  log.push(s.getBoundingClientRect().top, s.offsetTop);
  scrollTo(0, 500);
  log.push(nav.getBoundingClientRect().top, nav.offsetTop, s.getBoundingClientRect().top);
  log.push(document.getElementById('r').getBoundingClientRect().top);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    // Scrolled 150 in its container: 20px (10% of 200) below its top, so
    // 170 down its content, 70 below where it was laid out. The page
    // scrolled 500: the navigation 5px below the top, 505 down the page,
    // and the container gone up past the top with what is in it; an SVG
    // element in the navigation with it.
    assert_eq!(
        report(&url),
        "10px / 20px / auto / 100 / 100 / 20 / 170 / 5 / 505 / -480 / 7"
    );
}

/// A child frame's elements are in its own viewport, its iframe's content
/// box; the parent is laid out first (its iframe resized by script); an
/// iframe script just inserted has a laid-out document at once; one with
/// no box has a 0×0 viewport, and its parent's device pixels.
#[test]
fn child_frames() {
    let url = page(
        "frames.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } iframe { border: 0; width: 300px; height: 200px; display: block }</style>
<iframe id="f" srcdoc="<div id=d style='height: 30px'></div>"></iframe>
<script>
onload = () => {
  const log = [];
  const f = document.getElementById('f');
  const d = f.contentDocument.getElementById('d');
  const r = d.getBoundingClientRect();
  log.push(f.contentWindow.innerWidth, f.contentWindow.innerHeight, r.x, r.y, r.width);
  f.style.width = '400px';
  log.push(f.contentWindow.innerWidth, d.getBoundingClientRect().width);
  const inserted = document.createElement('iframe');
  inserted.style.width = '250px';
  document.body.append(inserted);
  log.push(inserted.contentWindow.innerWidth,
           inserted.contentDocument.body.getBoundingClientRect().width);
  const hidden = document.createElement('iframe');
  hidden.style.display = 'none';
  document.body.append(hidden);
  log.push(hidden.contentWindow.innerWidth, hidden.contentWindow.innerHeight,
           hidden.contentDocument.body.getBoundingClientRect().width,
           hidden.contentWindow.devicePixelRatio);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "300 / 200 / 8 / 8 / 284 / 400 / 384 / 250 / 234 / 0 / 0 / 0 / 1"
    );
}

/// `innerWidth` and `innerHeight` style nothing of their window's
/// document, whose layout doesn't decide them: a child frame's lays out
/// only the frames around it, a main frame's nothing once it has been laid
/// out, and itself before (Blink's `LocalDOMWindow::GetViewportSize`);
/// `scrollY` styles it. Seen through transitions: an element hidden around
/// a read that styled it has no before-change style when it shows again
/// with another opacity, so no transition starts. As Chrome 152 gives it.
#[test]
fn reading_the_size_styles_nothing() {
    let url = page(
        "viewport-size-style.html",
        r#"<!DOCTYPE html>
<style>.t { transition: opacity 100s linear }</style>
<body>
<script>
function transitions(win, read) {
  const e = win.document.createElement('div');
  e.className = 't';
  win.document.body.append(e);
  win.getComputedStyle(e).opacity;
  e.style.display = 'none';
  read(win);
  e.style.display = '';
  e.style.opacity = '0';
  win.getComputedStyle(e).opacity;
  return e.getAnimations().length;
}
// The main frame before its first layout (before the iframe, whose
// insertion lays it out in machopo), then after the one that read did.
const log = [transitions(window, w => w.innerWidth), transitions(window, w => w.innerWidth)];
</script>
<iframe id="f" srcdoc="<style>.t { transition: opacity 100s linear }</style>"></iframe>
<script>
onload = () => {
  const f = document.getElementById('f').contentWindow;
  log.push(
    transitions(window, w => w.innerHeight),
    transitions(f, w => w.innerWidth),
    transitions(f, w => w.innerHeight),
    transitions(window, w => w.scrollY),
    transitions(f, w => w.scrollY),
  );
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(report(&url), "0 / 1 / 1 / 1 / 1 / 0 / 0");
}

/// The top-level frame's media queries go by the window's size before its
/// first layout: a style read by a script the parser runs first has it
/// (WPT's `adoptedstylesheets-cascade-order`).
#[test]
fn media_queries_have_the_windows_size_before_the_first_layout() {
    let url = page(
        "media-before-layout.html",
        r#"<!DOCTYPE html>
<style>@media (min-width: 1px) { html { color: rgb(0, 128, 0) } }</style>
<script>
const color = getComputedStyle(document.documentElement).color;
onload = () => testRunner.reportHarnessResults(color);
</script>"#,
    );
    assert_eq!(report(&url), "rgb(0, 128, 0)");
}

/// A child frame's media queries go by its iframe's size: asked before
/// the frame is first laid out (an iframe script inserted loads before a
/// rendering update), and then on each change of its iframe's width,
/// which a `change` event reports at the next rendering update even when
/// script read `matches` in between (the frame laid out for it keeps its
/// size as last laid out until then; the parent's layout alone would
/// give the new one, and the change would go unreported).
#[test]
fn a_child_frames_media_queries_follow_its_iframe() {
    let url = page(
        "frame-media.html",
        r#"<!DOCTYPE html>
<body>
<script>
const f = document.createElement('iframe');
f.style.width = '300px';
f.srcdoc = '<p>x';
document.body.append(f);
f.onload = () => {
  const mql = f.contentWindow.matchMedia('(max-width: 150px)');
  const log = [mql.matches];
  mql.onchange = () => {
    log.push(mql.matches);
    if (log.length < 3) {
      f.style.width = '300px';
      mql.matches;
      return;
    }
    testRunner.reportHarnessResults(log.join(' '));
  };
  f.style.width = '100px';
  mql.matches;
};
</script>"#,
    );
    assert_eq!(report(&url), "false true false");
}

/// A child frame's first style, before its first layout (as its loads
/// start, for its background images), already sees its iframe's size in
/// its media queries: nothing transitions from a style of a 0×0 frame.
#[test]
fn a_child_frames_first_style_sees_its_iframes_size() {
    let url = page(
        "frame-first-style.html",
        r#"<!DOCTYPE html>
<style>iframe { border: 0; width: 300px; height: 200px }</style>
<iframe id="f" srcdoc="<style>
#t { color: rgb(255, 0, 0); transition: color 100s linear }
@media (min-width: 200px) { #t { color: rgb(0, 128, 0) } }
</style><div id=t>x</div>"></iframe>
<script>
onload = () => {
  const w = document.getElementById('f').contentWindow;
  testRunner.reportHarnessResults(
      w.getComputedStyle(w.document.getElementById('t')).color);
};
</script>"#,
    );
    assert_eq!(report(&url), "rgb(0, 128, 0)");
}

/// A document whose stylesheet is still loading (render-blocked) is laid
/// out for script all the same, with the styles it has. (One script asked
/// for: one the parser's `<link>` loads would hold the parser's scripts
/// back.)
#[test]
fn a_render_blocked_document() {
    let page = r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div id="d" style="width: 20px; height: 20px"></div>
<script>
const link = document.createElement('link');
link.rel = 'stylesheet';
link.href = '/slow.css';
document.head.append(link);
setTimeout(() => testRunner.reportHarnessResults(
    String(document.getElementById('d').getBoundingClientRect().width)), 0);
</script>"#;
    let server = TestServer::start(HashMap::from([
        ("/".to_string(), Route::ok("text/html", page)),
        (
            "/slow.css".to_string(),
            Route::ok("text/css", "#d { width: 40px !important }")
                .with_delay(Duration::from_secs(2)),
        ),
    ]))
    .expect("test server starts");
    assert_eq!(report(&server.url("/")), "20");
}

/// Every way the renderer runs script lends it the layout: a classic
/// script, a module script, a module `import()` evaluates, a timer, an
/// animation frame, a `load` listener, an inline and an external classic
/// script, an iframe's `load` handler, a mutation observer told of what the
/// parser inserted, and the microtasks after each, all read the viewport. (A click and a `javascript:` URL: below.)
#[test]
fn every_way_script_runs_reads_the_layout() {
    page("external.js", "read('external');");
    let url = page(
        "every-way.html",
        r#"<!DOCTYPE html>
<script>
const log = [];
// What `innerWidth` reads, from `way` and from a microtask after it.
window.read = way => {
  log.push(`${way} ${innerWidth}`);
  queueMicrotask(() => log.push(`${way}-microtask ${innerWidth}`));
};
read('classic');
// What the parser inserts, before a later script and after the last one.
const observed = new Set();
new MutationObserver(() => observed.add(`observer ${innerWidth}`))
    .observe(document, { childList: true, subtree: true });
setTimeout(() => read('timer'), 0);
requestAnimationFrame(() => read('frame'));
addEventListener('load', () => {
  read('load');
  // A `data:` URL: a `file:` page's modules are blocked.
  import("data:text/javascript,window.read('import')").then(() => {
    log.push(`imported ${innerWidth}`);
    setTimeout(() => {
      log.push(...observed);
      testRunner.reportHarnessResults(log.sort().join(' / '));
    }, 100);
  });
});
</script>
<p>inserted before a script</p>
<script src="external.js"></script>
<iframe onload="read('iframe-load')"></iframe>
<script type="module">read('module');</script>
<p>inserted after the last script</p>"#,
    );
    let ways = [
        "classic",
        "external",
        "frame",
        "iframe-load",
        "import",
        "load",
        "module",
        "timer",
    ];
    let mut expected: Vec<String> = ways
        .iter()
        .flat_map(|way| [format!("{way} 800"), format!("{way}-microtask 800")])
        .chain(["imported 800".to_string(), "observer 800".to_string()])
        .collect();
    expected.sort();
    assert_eq!(report(&url), expected.join(" / "));
}

/// A click the user makes, and the `javascript:` URL of the link clicked,
/// read the viewport too (the window is 200 CSS px wide here): each turns
/// its box green if so.
#[test]
fn a_click_and_a_javascript_url_read_the_layout() {
    let url = page(
        "click.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } a, div { display: block; width: 100px; height: 100px }</style>
<a id="link" style="background-color: red"
   href="javascript:document.getElementById('link').style.backgroundColor=(innerWidth==200?'green':'red')"></a>
<div id="listened" style="background-color: red"></div>
<script>
document.getElementById('link').addEventListener('click', () => {
  document.getElementById('listened').style.backgroundColor = innerWidth == 200 ? 'green' : 'red';
});
</script>"#,
    );
    let (x, y, button, modifiers) = (50.0, 50.0, MouseButton::Left, Modifiers::NONE);
    let input = [
        InputEvent::mouse_down(x, y, button, modifiers),
        InputEvent::mouse_up(x, y, button, modifiers),
    ];
    let frame = headless::render_after_input(&url, Size::new(200, 200), 1.0, true, &input)
        .expect("the page renders");
    let pixel = |x: usize, y: usize| {
        let i = (y * frame.size.width as usize + x) * 4;
        <[u8; 4]>::try_from(&frame.pixels[i..i + 4]).unwrap()
    };
    const GREEN: [u8; 4] = [0, 128, 0, 255];
    assert_eq!(pixel(50, 50), GREEN, "the javascript: URL");
    assert_eq!(pixel(50, 150), GREEN, "the listener");
}

/// `offset*`, `client*` and `scroll*` from the renderer's layout: a scroll
/// container's (its `offsetParent` the body, its scrollable overflow), the
/// scrolling element's (the viewport and the document's size), and a
/// child frame's, in its own document.
#[test]
fn box_members() {
    let url = page(
        "boxes.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  #s { overflow: scroll; width: 100px; height: 50px; border: 2px solid; margin-left: 10px }
  #c { width: 300px; height: 200px }
  iframe { border: 0; width: 300px; height: 200px; display: block }
</style>
<div id="s"><div id="c"></div></div>
<iframe id="f" srcdoc="<div id=d style='margin-left: 5px; height: 10px'></div>"></iframe>
<div style="height: 2000px"></div>
<script>
onload = () => {
  const log = [];
  const s = document.getElementById('s');
  log.push(s.offsetParent.tagName, s.offsetLeft, s.offsetTop, s.offsetWidth, s.offsetHeight,
           s.clientTop, s.clientWidth, s.scrollWidth, s.scrollHeight, s.scrollTop);
  const root = document.documentElement;
  log.push(document.scrollingElement === root, root.clientWidth, root.clientHeight,
           root.scrollWidth, root.scrollHeight);
  const child = document.getElementById('f').contentDocument;
  const d = child.getElementById('d');
  log.push(d.offsetLeft, d.offsetTop, d.offsetWidth, child.documentElement.clientWidth,
           child.documentElement.clientHeight);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "BODY / 10 / 0 / 104 / 54 / 2 / 100 / 300 / 200 / 0 / true / 800 / 600 / 800 / 2254 / \
         13 / 8 / 279 / 300 / 200"
    );
}

/// Elements in a shadow tree have boxes and styles; the host's box holds
/// its shadow tree, with its children in the slot. `elementFromPoint` of
/// the document gives the host for what is in its shadow tree, the shadow
/// root's its own element (text, its parent element: the host for its text
/// child in a slot, or for a shadow root's own text); a slot (`display:
/// contents`) has no box. A child's `offsetParent` is in the flat tree,
/// past a shadow tree it isn't in, open or closed (Chrome's retargeting).
/// A range holding a `display: contents`
/// element whole has its children's boxes.
#[test]
fn elements_in_shadow_trees_have_boxes() {
    let url = page(
        "shadow.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div id="host" style="width: 100px"><span id="light" style="display: block; height: 10px">light</span></div>
<div id="open"><i id="in-open">x</i></div><div id="closed"><i id="in-closed">x</i></div>
<div id="slotted" style="height: 20px">slotted text</div><div id="own" style="height: 20px"></div>
<div id="before" style="height: 10px"></div><div id="contents" style="display: contents"><div
  style="height: 30px"></div></div><div id="after" style="height: 10px"></div>
<script>
onload = () => {
  const log = [];
  const host = document.getElementById('host');
  const root = host.attachShadow({ mode: 'open' });
  root.innerHTML = '<div id="inner" style="width: 50px; height: 20px; color: rgb(1, 2, 3)">shadow</div><slot id="slot"></slot>';
  const inner = root.getElementById('inner');
  const r = inner.getBoundingClientRect();
  log.push(inner.getClientRects().length, `${r.x},${r.y},${r.width},${r.height}`,
           inner.offsetWidth, inner.offsetHeight, inner.offsetParent === document.body);
  const style = getComputedStyle(inner);
  log.push(style.color, style.width, style.display);
  const light = document.getElementById('light');
  log.push(light.getBoundingClientRect().y, host.offsetHeight);
  const slot = root.getElementById('slot');
  log.push(slot.getClientRects().length, getComputedStyle(slot).display);
  log.push(document.elementFromPoint(5, 5).id, root.elementFromPoint(5, 5).id,
           document.elementFromPoint(5, 25).id, root.elementFromPoint(5, 25).id);
  log.push(document.elementsFromPoint(5, 5).map(e => e.id || e.localName).join(','));
  const markup = '<div style="position: relative"><slot></slot></div>';
  document.getElementById('open').attachShadow({ mode: 'open' }).innerHTML = markup;
  document.getElementById('closed').attachShadow({ mode: 'closed' }).innerHTML = markup;
  log.push(document.getElementById('in-open').offsetParent.localName,
           document.getElementById('in-closed').offsetParent.localName);
  log.push(root.elementsFromPoint(5, 5).map(e => e.id || e.localName).join(','));
  const slotted = document.getElementById('slotted');
  const slottedRoot = slotted.attachShadow({ mode: 'open' });
  slottedRoot.innerHTML = '<slot></slot>';
  const own = document.getElementById('own');
  const ownRoot = own.attachShadow({ mode: 'open' });
  ownRoot.innerHTML = 'own text';
  const at = e => [e.getBoundingClientRect().x + 5, e.getBoundingClientRect().y + 5];
  log.push(slottedRoot.elementFromPoint(...at(slotted)).id, ownRoot.elementFromPoint(...at(own)).id,
           document.elementFromPoint(...at(own)).id);
  const range = document.createRange();
  range.selectNode(document.getElementById('contents'));
  log.push(range.getBoundingClientRect().height);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "1 / 0,0,50,20 / 50 / 20 / true / rgb(1, 2, 3) / 50px / block / 20 / 30 / \
         0 / contents / host / inner / light / light / host,body,html / body / body / \
         inner,host,body,html / slotted / own / own / 30"
    );
}

/// A thousand elements one in another, and a thousand shadow trees: laid
/// out (the renderer's thread has the stack for it), the innermost has its
/// box.
#[test]
fn a_thousand_levels_deep() {
    let url = page(
        "deep.html",
        r#"<!DOCTYPE html>
<div id="elements"></div><div id="shadows"></div>
<script>
onload = () => {
  let parent = document.getElementById('elements');
  for (let i = 0; i < 1000; i++) {
    const div = document.createElement('div');
    parent.appendChild(div);
    parent = div;
  }
  parent.textContent = 'deep';
  let container = document.getElementById('shadows');
  for (let i = 0; i < 1000; i++) {
    const host = document.createElement('div');
    container.appendChild(host);
    container = host.attachShadow({ mode: 'open' });
  }
  const inner = document.createElement('div');
  inner.textContent = 'deep';
  container.appendChild(inner);
  testRunner.reportHarnessResults(`${parent.offsetHeight > 0} ${inner.offsetHeight > 0}`);
};
</script>"#,
    );
    assert_eq!(report(&url), "true true");
}

/// Script scrolls through the renderer: a container's `scrollTop` past its
/// end is clamped, `overflow: hidden` scrolls by script, the window by
/// `scrollTo` and `scrollBy` (and the scrolling element), a child frame's
/// window its own viewport only; the rects move, `offsetTop` doesn't; the
/// promise resolves.
#[test]
fn scrolling() {
    let url = page(
        "scrolling.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  .s { width: 100px; height: 50px; overflow: auto }
  .s div { height: 250px }
  iframe { border: 0; width: 300px; height: 100px; display: block }
</style>
<div id="s" class="s"><div id="in"></div></div>
<div id="h" class="s" style="overflow: hidden"><div></div></div>
<iframe id="f" srcdoc="<div style='height: 1000px'></div>"></iframe>
<div style="height: 2000px"></div>
<script>
onload = async () => {
  const log = [];
  const s = document.getElementById('s');
  s.scrollTop = 1000;
  log.push(s.scrollTop, document.getElementById('in').getBoundingClientRect().top,
           document.getElementById('in').offsetTop);
  const h = document.getElementById('h');
  h.scrollTo(0, 30);
  log.push(h.scrollTop);
  await window.scrollTo(0, 300);
  log.push('resolved', scrollY, s.getBoundingClientRect().top);
  window.scrollBy({ top: -100 });
  log.push(scrollY, document.documentElement.scrollTop);
  document.documentElement.scrollTop = 1e9;
  log.push(scrollY);
  const child = document.getElementById('f').contentWindow;
  child.scrollTo(0, 50);
  log.push(child.scrollY, scrollY);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    // The page is 50 + 50 + 100 + 2000 = 2200 high in 600: 1600 at most.
    assert_eq!(
        report(&url),
        "200 / -200 / 0 / 30 / resolved / 300 / -300 / 200 / 200 / 1600 / 50 / 1600"
    );
}

/// `scroll` events (HTML's scroll steps): in the next frame, before its
/// animation frame callbacks; once per target however often it moved; none
/// for a scroll to where it is. The document's (the viewport's) bubbles to
/// the window, where `<body onscroll>` is; a container's doesn't bubble. A
/// child frame's is in its own document. (`<body onscroll>` was set as the
/// page was parsed: before the window's other listener.)
#[test]
fn scroll_events() {
    let url = page(
        "scroll-events.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  #s { width: 100px; height: 50px; overflow: auto }
  #s div { height: 250px }
  iframe { border: 0; width: 300px; height: 100px; display: block }
</style>
<body onscroll="log.push('body-attribute ' + event.target.nodeName)">
<div id="s"><div></div></div>
<iframe id="f" srcdoc="<div style='height: 1000px'></div>"></iframe>
<div style="height: 2000px"></div>
<script>
const log = [];
onload = () => {
  const s = document.getElementById('s');
  s.addEventListener('scroll', e => log.push(`s ${e.bubbles} ${e.cancelable}`));
  s.onscroll = () => log.push('s onscroll');
  log.push(typeof onscroll, document.body.onscroll === onscroll);
  document.body.addEventListener('scroll', () => log.push('body'));
  document.addEventListener('scroll', e => log.push(`document ${e.bubbles}`));
  addEventListener('scroll', e => log.push('window ' + e.target.nodeName));
  const child = document.getElementById('f').contentWindow;
  child.addEventListener('scroll', e => log.push('child ' + (e.target === child.document)));
  s.scrollTop = 10;
  s.scrollTop = 20;
  scrollTo(0, 100);
  child.scrollTo(0, 5);
  // Where they are already: no event.
  s.scrollTo(0, 20);
  requestAnimationFrame(() => {
    log.push('frame');
    s.scrollTop = 20;
    requestAnimationFrame(() => requestAnimationFrame(() => {
      testRunner.reportHarnessResults(log.join(' / '));
    }));
  });
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "function / true / s false false / s onscroll / document true / \
         body-attribute #document / window #document / child true / frame"
    );
}

/// A `scroll` listener runs in the scroll steps, inside a main frame: an
/// iframe it inserts loads all the same (its navigation starts after the
/// scroll steps, as after animation frame callbacks).
#[test]
fn a_scroll_listener_may_insert_an_iframe() {
    let url = page(
        "scroll-inserts.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div style="height: 2000px"></div>
<script>
onload = () => {
  addEventListener('scroll', () => {
    const iframe = document.createElement('iframe');
    iframe.srcdoc = 'inserted';
    iframe.onload = () => testRunner.reportHarnessResults(
        iframe.contentDocument.body.textContent);
    document.body.append(iframe);
  }, { once: true });
  scrollTo(0, 100);
};
</script>"#,
    );
    assert_eq!(report(&url), "inserted");
}

/// A scroll offset layout clamps (the content shrank) moved: its `scroll`
/// event comes in the next frame, which the renderer asks for itself, as
/// nothing else would here (the shrinking reads no geometry).
#[test]
fn layout_clamping_fires_a_scroll_event() {
    let url = page(
        "scroll-clamped.html",
        r#"<!DOCTYPE html>
<style>
  #s { width: 100px; height: 50px; overflow: auto }
  #c { height: 250px }
</style>
<div id="s"><div id="c"></div></div>
<script>
onload = () => {
  const s = document.getElementById('s');
  s.scrollTop = 200;
  requestAnimationFrame(() => requestAnimationFrame(() => {
    s.addEventListener('scroll', () => testRunner.reportHarnessResults(`scroll ${s.scrollTop}`));
    setTimeout(() => { document.getElementById('c').style.height = '100px'; }, 0);
  }));
};
</script>"#,
    );
    assert_eq!(report(&url), "scroll 50");
}

/// The window scrolled to (0, 0) goes there before its content, just
/// grown above where it was scrolled to, is laid out: at the top after
/// that layout too (no scroll anchoring moves it back), the content where
/// the layout puts it, its `scroll` event in the next frame.
#[test]
fn scrolling_the_window_to_its_top_before_a_layout() {
    let url = page(
        "scroll-to-top.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div id="above"></div>
<div id="target" style="height: 50px"></div>
<div style="height: 3000px"></div>
<script>
onload = () => {
  const log = [];
  scrollTo(0, 1000);
  log.push(scrollY);
  requestAnimationFrame(() => requestAnimationFrame(() => {
    addEventListener('scroll', () => log.push('scroll ' + scrollY));
    document.getElementById('above').style.height = '500px';
    scrollTo({ left: 0, top: 0 });
    log.push(scrollY, document.getElementById('target').getBoundingClientRect().top);
    requestAnimationFrame(() => requestAnimationFrame(() => {
      log.push(scrollY);
      testRunner.reportHarnessResults(log.join(' / '));
    }));
  }));
};
</script>"#,
    );
    assert_eq!(report(&url), "1000 / 0 / 500 / scroll 0 / 0");
}

/// `elementFromPoint` and `elementsFromPoint` hit test the renderer's
/// frame, laid out and pre-painted as script left it: a box just moved by
/// a transform covers the one under it; a scrolled page's point is the
/// viewport's; what is in a child frame is its iframe, and the child
/// document's own hit test is in its viewport (its parent's box over it
/// not counted, its own scroll offset added); below the content is the root element; outside the
/// viewport is nothing. (`#cover` is moved over the iframe.)
#[test]
fn hit_testing() {
    let url = page(
        "hit-testing.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  div { height: 50px }
  iframe { border: 0; width: 300px; height: 100px; display: block }
  #cover { width: 50px; transform: translateY(-100px) }
</style>
<div id="under"></div>
<div id="over"></div>
<iframe id="f" srcdoc="<style>body { margin: 0 }</style><p id=p style='margin: 0; height: 100px'>p</p><p id=q style='margin: 0; height: 100px'>q</p>"></iframe>
<div id="cover"></div>
<div style="height: 2000px" id="long"></div>
<script>
onload = () => {
  const log = [];
  const id = e => e ? e.id || e.tagName : 'null';
  const ids = list => list.map(id).join(',');
  log.push(id(document.elementFromPoint(5, 5)));
  document.getElementById('over').style.transform = 'translateY(-40px)';
  log.push(id(document.elementFromPoint(5, 15)), ids(document.elementsFromPoint(5, 15)));
  const f = document.getElementById('f');
  log.push(id(document.elementFromPoint(100, 150)), ids(document.elementsFromPoint(100, 150)));
  const child = f.contentDocument;
  log.push(id(child.elementFromPoint(5, 5)), ids(child.elementsFromPoint(5, 5)));
  log.push(id(document.elementFromPoint(5, 105)));
  log.push(id(child.elementFromPoint(5, 150)), id(child.elementFromPoint(500, 5)));
  log.push(id(document.elementFromPoint(-1, 5)), id(document.elementFromPoint(5, 600)),
           document.elementsFromPoint(800, 5).length);
  document.getElementById('long').style.height = '0';
  log.push(id(document.elementFromPoint(5, 500)), ids(document.elementsFromPoint(5, 500)));
  document.getElementById('long').style.height = '2000px';
  window.scrollTo(0, 100);
  log.push(id(document.elementFromPoint(100, 5)));
  f.contentWindow.scrollTo(0, 60);
  log.push(id(child.elementFromPoint(5, 50)));
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "under / over / over,under,BODY,HTML / f / f,BODY,HTML / p / p,BODY,HTML / cover / \
         null / null / null / null / 0 / HTML / HTML / f / q"
    );
}

/// A child document's own hit test where its parent clips its iframe:
/// its frame's, what the parent shows of it aside (Blink hit tests the
/// child frame's layout view).
#[test]
fn a_clipped_child_frame_hit_tests_its_own_document() {
    let url = page(
        "clipped-frame.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  #clip { height: 50px; overflow: hidden }
  iframe { border: 0; width: 100px; height: 100px; display: block }
</style>
<div id="clip"><iframe id="f" srcdoc="<style>body { margin: 0 }</style><p id=p style='margin: 0; height: 100px'>p</p>"></iframe></div>
<script>
onload = () => {
  const child = document.getElementById('f').contentDocument;
  const id = e => e ? e.id || e.tagName : 'null';
  testRunner.reportHarnessResults([
    child.elementsFromPoint(5, 75).map(id).join(','),
    id(child.elementFromPoint(5, 75)),
  ].join(' / '));
};
</script>"#,
    );
    assert_eq!(report(&url), "p,BODY,HTML / p");
}

/// `scrollIntoView` through the renderer: a container's target centered
/// (the viewport can't scroll up to center the container); sideways, the
/// nearest edge; an inline element on three lines, its end by its last
/// line's; a child frame's element, its frame's viewport first, then the
/// page's to where the iframe's content box shows it (not the iframe's
/// border box); with `container: "nearest"`, the child frame's viewport
/// only; `scroll` events in both documents; an element with no box scrolls nothing. The promise
/// resolves.
#[test]
fn scroll_into_view() {
    let url = page(
        "into-view.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  .s { width: 100px; height: 50px; overflow: auto }
  iframe { border: 0; width: 300px; height: 100px; display: block }
</style>
<div id="box" class="s"><div style="height: 200px"></div><div id="t" style="height: 10px"></div><div style="height: 100px"></div></div>
<div id="h" class="s" style="height: 20px"><div style="width: 400px; height: 10px"><div id="ht" style="margin-left: 250px; width: 20px; height: 10px"></div></div></div>
<div id="lines" class="s" style="width: 60px; height: 30px"><div style="height: 100px"></div><span id="span">aaaa bbbb cccc dddd</span><div style="height: 100px"></div></div>
<div style="height: 900px"></div>
<iframe id="f" style="border: 5px solid; padding: 3px" srcdoc="<style>body { margin: 0 }</style><div style='height: 500px'></div><div id=target style='width: 10px; height: 10px'></div><div style='height: 1000px'></div>"></iframe>
<div style="height: 2000px"></div>
<div id="none" style="display: none"></div>
<script>
onload = async () => {
  const log = [];
  const frame = () => new Promise(r => requestAnimationFrame(r));
  const box = document.getElementById('box');
  await document.getElementById('t').scrollIntoView({ block: 'center' });
  log.push('resolved', box.scrollTop, scrollY);
  const h = document.getElementById('h');
  document.getElementById('ht').scrollIntoView({ block: 'nearest' });
  log.push(h.scrollLeft, h.scrollTop);
  const lines = document.getElementById('lines');
  const span = document.getElementById('span');
  span.scrollIntoView({ block: 'end' });
  const last = [...span.getClientRects()].pop();
  log.push(span.getClientRects().length > 1,
           Math.abs(last.bottom - (lines.getBoundingClientRect().top + lines.clientHeight)) < 0.01);
  const f = document.getElementById('f');
  const child = f.contentWindow;
  const target = f.contentDocument.getElementById('target');
  await frame();
  const events = [];
  addEventListener('scroll', () => events.push('page'));
  child.addEventListener('scroll', () => events.push('child'));
  target.scrollIntoView();
  log.push(child.scrollY, scrollY, target.getBoundingClientRect().top,
           f.getBoundingClientRect().top);
  await new Promise(resolve => child.addEventListener('scroll', resolve, { once: true }));
  log.push(events.sort().join());
  scrollTo(0, 0);
  child.scrollTo(0, 0);
  target.scrollIntoView({ block: 'end', container: 'nearest' });
  log.push(child.scrollY, scrollY);
  document.getElementById('none').scrollIntoView();
  log.push(child.scrollY, scrollY);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    // The page: 50 + 20 + 30 + 900 above the iframe, at 1000; its content
    // box at 1008.
    assert_eq!(
        report(&url),
        "resolved / 180 / 0 / 170 / 0 / true / true / 500 / 1008 / 0 / -8 / child,page / 410 / \
         0 / 410 / 0"
    );
}

/// Absolute positioning in a whole browser: the box
/// is laid out against its containing block's padding box however deep it
/// sits, `offsetParent` is that containing block, and `getComputedStyle`
/// resolves the insets to where layout put it (`auto` on a static box).
#[test]
fn absolute_boxes_and_their_insets() {
    let url = page(
        "absolute.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  #cb { position: relative; margin: 10px; border: 2px solid; padding: 5px;
        width: 300px; height: 200px }
  #deep { height: 20px }
  #abs { position: absolute; top: 3px; left: 20%; width: 50px; height: 60px }
  #rel { position: relative; top: 4px; left: 5px; height: 10px }
  /* Over-constrained: `bottom` is ignored, so it keeps its own value. */
  #over { position: absolute; top: 10px; bottom: 20px; height: 40px; width: 10px }
  #flow { height: 10px }
</style>
<div id="cb"><div id="deep"><div id="abs"></div><div id="over"></div></div><div id="rel"></div><div id="flow"></div></div>
<script>
onload = () => {
  const log = [];
  const at = id => document.getElementById(id);
  const abs = at('abs');
  log.push(abs.offsetParent.id, abs.offsetLeft, abs.offsetTop, abs.offsetWidth);
  const s = getComputedStyle(abs);
  log.push(s.position, s.top, s.left, s.right, s.bottom);
  const rel = getComputedStyle(at('rel'));
  log.push(rel.top, rel.left, at('rel').offsetParent.id);
  const flow = getComputedStyle(at('flow'));
  log.push(flow.position, flow.top, flow.left);
  const over = getComputedStyle(at('over'));
  log.push(over.top, over.bottom, at('over').offsetTop);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "cb / 62 / 3 / 50 / absolute / 3px / 62px / 198px / 147px / 4px / 5px / cb / \
         static / auto / auto / 10px / 20px / 10"
    );
}

/// A fixed box in a whole browser: it is laid out
/// against the viewport however deep it sits, and CSSOM View gives it no
/// `offsetParent` (its `offset*` are the document origin's) even inside a
/// positioned ancestor.
#[test]
fn fixed_boxes_have_no_offset_parent() {
    let url = page(
        "fixed.html",
        r#"<!DOCTYPE html>
<style>
  html, body { margin: 0 }
  #cb { position: relative; margin: 10px; border: 2px solid; padding: 5px;
        width: 300px; height: 200px }
  #pinned { position: fixed; top: 7px; right: 0; width: 40px; height: 20px }
</style>
<div id="cb"><div id="deep"><div id="pinned"></div></div></div>
<script>
onload = () => {
  const pinned = document.getElementById('pinned');
  const s = getComputedStyle(pinned);
  testRunner.reportHarnessResults([
    String(pinned.offsetParent), pinned.offsetLeft, pinned.offsetTop,
    s.position, s.top, s.left, s.right,
  ].join(' / '));
};
</script>"#,
    );
    // The viewport is 800 wide: `right: 0` puts it at 760.
    assert_eq!(report(&url), "null / 760 / 7 / fixed / 7px / 760px / 0px");
}

/// `url()` in a `style` attribute resolves against the document's base
/// URL: a `<base>` added later changes what it resolves to once the style
/// is computed again.
#[test]
fn background_urls_resolve_against_the_base_url() {
    let url = page(
        "base-url.html",
        r#"<!DOCTYPE html>
<div id="d" style="background-image: url(img.png), linear-gradient(red, blue)"></div>
<script>
onload = () => {
  const d = document.getElementById('d');
  const log = [getComputedStyle(d).backgroundImage.replace(/^url\("file:\/\/.*\/(img.png)"\)/, 'url($1)')];
  const base = document.createElement('base');
  base.setAttribute('href', 'http://example.test/dir/');
  document.head.appendChild(base);
  d.style.backgroundImage = 'url(img.png)';
  log.push(getComputedStyle(d).backgroundImage);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "url(img.png), linear-gradient(rgb(255, 0, 0), rgb(0, 0, 255)) / \
         url(\"http://example.test/dir/img.png\")"
    );
}

/// SVG elements' geometry, as Chrome 152 gives it: `getBBox()` in the
/// element's user space (a `<use>`'s copy placed at its `x` and `y`; what
/// is in `<defs>`, an unused `<symbol>` and a `<g>` with `display: none`
/// has one, a `display: none` element hasn't), `getCTM()` up to the
/// nearest viewport, `getScreenCTM()` through the `<svg>`'s box and the
/// CSS transform around it, one client rect for what is rendered (none in
/// a hidden container, which isn't hit either); and which take the focus:
/// a link, a `tabindex`, an SVG graphics element's focus listener (with
/// `tabIndex` -1; not an HTML element's), not `visibility: hidden`.
#[test]
fn svg_elements_geometry() {
    let url = page(
        "svg-elements.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<div style="height:30px"></div>
<div style="margin-left:20px;transform:translate(5px,7px)">
<svg id=s width=200 height=100 viewBox="0 0 100 50" style="border:3px solid;padding:4px">
 <g id=g transform="translate(10,5)">
  <rect id=r x=1 y=2 width=10 height=20 transform="scale(2)" stroke=black stroke-width=4 />
  <circle id=c cx=50 cy=20 r=5 />
 </g>
 <svg id=n x=60 y=10 width=30 height=20 viewBox="0 0 10 10"><rect id=nr width=5 height=5 /></svg>
 <g id=empty></g>
 <defs><rect id=dr x=3 y=4 width=5 height=6 transform="translate(1,1)" /></defs>
 <symbol id=sym><rect id=sr width=7 height=8 /></symbol>
 <use id=u href="#dr" x=20 y=20 />
 <rect id=dn style="display:none" width=10 height=10 />
 <g style="display:none"><rect id=indn width=10 height=10 /></g>
 <a id=a href="#x"><rect id=ar width=5 height=5 /></a>
 <rect id=ti tabindex=0 width=5 height=5 />
 <rect id=fl width=5 height=5 />
 <circle id=vis cx=5 cy=5 r=3 style="visibility:hidden" />
 <line id=ln x1=0 y1=0 x2=10 y2=0 stroke=black stroke-width=2 />
</svg></div>
<svg id=hid style="display:none"><rect id=hr width=4 height=4 /></svg>
<script>
onload = () => {
  document.getElementById("fl").addEventListener("focus", () => {});
  const o = [];
  const n = v => +v.toFixed(3);
  const m = x => `[${[x.a, x.b, x.c, x.d, x.e, x.f].map(n)}]`;
  const r = x => `(${[x.x, x.y, x.width, x.height].map(n)})`;
  const free = document.createElementNS("http://www.w3.org/2000/svg", "rect");
  free.id = "free";
  const ids = "s g r c n nr empty dr sym sr u dn indn a ar ti fl vis ln hid hr";
  for (const e of [...ids.split(" ").map(i => document.getElementById(i)), free]) {
    e.focus();
    o.push(`${e.id}: bbox=${r(e.getBBox())} ctm=${m(e.getCTM())} sctm=${m(e.getScreenCTM())} ` +
      `bcr=${r(e.getBoundingClientRect())} rects=${e.getClientRects().length} ` +
      `focus=${document.activeElement === e} tabIndex=${e.tabIndex}`);
  }
  // Only an SVG graphics element takes the focus for its listener; what is
  // in <defs> isn't hit.
  const div = document.createElement("div");
  document.body.append(div);
  div.addEventListener("focus", () => {});
  div.focus();
  o.push(`div: focus=${document.activeElement === div} hit=${document.elementFromPoint(41, 55).id}`);
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        "s: bbox=(0,0,75,49) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(25,37,214,114) rects=1 focus=false tabIndex=-1",
        "g: bbox=(2,4,53,40) ctm=[2,0,0,2,20,10] sctm=[2,0,0,2,52,54] bcr=(56,62,106,80) rects=1 focus=false tabIndex=-1",
        "r: bbox=(1,2,10,20) ctm=[4,0,0,4,20,10] sctm=[4,0,0,4,52,54] bcr=(56,62,40,80) rects=1 focus=false tabIndex=-1",
        "c: bbox=(45,15,10,10) ctm=[2,0,0,2,20,10] sctm=[2,0,0,2,52,54] bcr=(142,84,20,20) rects=1 focus=false tabIndex=-1",
        "n: bbox=(0,0,5,5) ctm=[4,0,0,4,130,20] sctm=[4,0,0,4,162,64] bcr=(162,64,20,20) rects=1 focus=false tabIndex=-1",
        "nr: bbox=(0,0,5,5) ctm=[2,0,0,2,65,10] sctm=[4,0,0,4,162,64] bcr=(162,64,20,20) rects=1 focus=false tabIndex=-1",
        "empty: bbox=(0,0,0,0) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(32,44,0,0) rects=1 focus=false tabIndex=-1",
        "dr: bbox=(3,4,5,6) ctm=[2,0,0,2,2,2] sctm=[2,0,0,2,34,46] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "sym: bbox=(0,0,7,8) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "sr: bbox=(0,0,7,8) ctm=[1,0,0,1,0,0] sctm=[2,0,0,2,32,44] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "u: bbox=(24,25,5,6) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(80,94,10,12) rects=1 focus=false tabIndex=-1",
        "dn: bbox=(0,0,0,0) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "indn: bbox=(0,0,10,10) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "a: bbox=(0,0,5,5) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(32,44,10,10) rects=1 focus=true tabIndex=0",
        "ar: bbox=(0,0,5,5) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(32,44,10,10) rects=1 focus=false tabIndex=-1",
        "ti: bbox=(0,0,5,5) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(32,44,10,10) rects=1 focus=true tabIndex=0",
        "fl: bbox=(0,0,5,5) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(32,44,10,10) rects=1 focus=true tabIndex=-1",
        "vis: bbox=(2,2,6,6) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(36,48,12,12) rects=1 focus=false tabIndex=-1",
        "ln: bbox=(0,0,10,0) ctm=[2,0,0,2,0,0] sctm=[2,0,0,2,32,44] bcr=(32,44,20,0) rects=1 focus=false tabIndex=-1",
        "hid: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "hr: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "free: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,0,0) rects=0 focus=false tabIndex=-1",
        "div: focus=false hit=s",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// The screen matrix takes the scroll containers' and the viewport's
/// offsets off, and a rotation around the `<svg>` in; a child frame's is
/// its own viewport's; a CSS `transform` wins over the attribute, and a
/// `display: none` element's own counts for nothing; an SVG element out
/// of an `<svg>` has no box. An SVG `<a>` without a link has `tabIndex` 0
/// but can't take the focus. A new matrix each call, of Chrome's own
/// interfaces.
#[test]
fn svg_geometry_scrolled_and_in_frames() {
    let url = page(
        "svg-scrolled.html",
        r##"<!DOCTYPE html><style>body{margin:0;height:3000px}</style>
<div style="height:100px"></div>
<div id=sc style="overflow:scroll;height:80px;width:300px"><div style="height:50px"></div>
<svg id=s width=100 height=100>
 <rect id=r x=1 y=2 width=10 height=20 style="transform:matrix(0,1,-1,0,3,4)" transform="scale(5)" />
 <rect id=dnt style="display:none" transform="translate(7,8)" width=1 height=1 />
 <a id=a2><rect width=1 height=1 /></a>
</svg><div style="height:500px"></div></div>
<div style="transform:matrix(0,1,-1,0,0,0);margin-left:300px;width:100px;height:100px">
<svg id=rs width=100 height=100 style="display:block"><rect id=rr x=1 y=2 width=10 height=20 /></svg></div>
<iframe id=f width=300 height=200 style="border:5px solid;margin-left:40px" srcdoc="<style>body{margin:10px}</style><div style='height:20px'></div><svg id=s width=50 height=50><rect id=r width=5 height=5 x=2 y=3 /></svg>"></iframe>
<script>
onload = () => {
  const o = [];
  const n = v => +v.toFixed(3);
  const m = x => `[${[x.a, x.b, x.c, x.d, x.e, x.f].map(n)}]`;
  const r = x => `(${[x.x, x.y, x.width, x.height].map(n)})`;
  document.getElementById("sc").scrollTop = 30;
  window.scrollTo(0, 40);
  const line = (label, e) => o.push(`${label}: bbox=${r(e.getBBox())} ctm=${m(e.getCTM())} ` +
    `sctm=${m(e.getScreenCTM())} bcr=${r(e.getBoundingClientRect())}`);
  for (const id of ["s", "r", "dnt", "rs", "rr"]) line(id, document.getElementById(id));
  const a2 = document.getElementById("a2");
  a2.focus();
  o.push(`a2: focus=${document.activeElement === a2} tabIndex=${a2.tabIndex}`);
  const inner = document.getElementById("f").contentDocument;
  for (const id of ["s", "r"]) line("iframe " + id, inner.getElementById(id));
  const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
  c.setAttribute("r", 5);
  document.body.appendChild(c);
  line("no <svg>", c);
  const e = document.getElementById("r");
  o.push(`${e.getCTM() instanceof SVGMatrix} ${e.getCTM() === e.getCTM()} ${e.getBBox() instanceof SVGRect}`);
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        "s: bbox=(-19,0,20,15) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,80] bcr=(0,80,100,100)",
        "r: bbox=(1,2,10,20) ctm=[0,1,-1,0,3,4] sctm=[0,1,-1,0,3,84] bcr=(-19,85,20,10)",
        "dnt: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,80] bcr=(0,0,0,0)",
        "rs: bbox=(1,2,10,20) ctm=[1,0,0,1,0,0] sctm=[0,1,-1,0,400,140] bcr=(300,140,100,100)",
        "rr: bbox=(1,2,10,20) ctm=[1,0,0,1,0,0] sctm=[0,1,-1,0,400,140] bcr=(378,141,20,10)",
        "a2: focus=false tabIndex=0",
        "iframe s: bbox=(2,3,5,5) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,10,30] bcr=(10,30,50,50)",
        "iframe r: bbox=(2,3,5,5) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,10,30] bcr=(12,33,5,5)",
        "no <svg>: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,0,0)",
        "true false true",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// Bounding boxes of shapes with no area (their place, as Blink keeps
/// it), and which count in a container's (not those, nor an empty path,
/// nor a hidden container; a line with no length does); a `<switch>`'s
/// child it doesn't draw has none; a nested `<svg>`'s and a `<symbol>`'s
/// `transform`; and a change laid out as script asks.
#[test]
fn svg_geometry_of_empty_shapes_and_containers() {
    let url = page(
        "svg-empty.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<svg id=s width=200 height=200>
 <rect id=z x=5 y=6 width=0 height=10 />
 <g id=gz><rect x=5 y=6 width=0 height=10 /><line x1=1 y1=1 x2=1 y2=1 /></g>
 <g id=gl><line x1=50 y1=60 x2=70 y2=60 /><rect x=80 y=80 width=1 height=1 /></g>
 <path id=p d="M10 10 C 50 -20 90 40 100 10" />
 <path id=pe d="" />
 <circle id=c0 cx=5 cy=6 r=0 />
 <ellipse id=e0 cx=5 cy=6 rx=0 ry=3 />
 <switch id=sw><rect id=sw1 width=3 height=3 systemLanguage="xx" /><rect id=sw2 x=7 width=4 height=4 /></switch>
 <svg id=n x=10 y=10 width=50 height=50 transform="translate(3,4)"><rect id=nr width=5 height=5 /></svg>
 <g id=gt transform="translate(10,10)"><rect id=gr width=5 height=5 transform="translate(1,1)" /></g>
 <symbol id=sy transform="translate(5,5)"><rect id=syr width=1 height=1 /></symbol>
 <defs id=dt transform="translate(5,5)"><rect id=dtr width=1 height=1 /><rect x=190 y=190 width=1 height=1 /></defs>
</svg>
<script>
onload = () => {
  const o = [];
  const n = v => +v.toFixed(3);
  const m = x => `[${[x.a, x.b, x.c, x.d, x.e, x.f].map(n)}]`;
  const r = x => `(${[x.x, x.y, x.width, x.height].map(n)})`;
  for (const id of "s z gz gl p pe c0 e0 sw sw1 sw2 n nr gt gr sy syr dt dtr".split(" ")) {
    const e = document.getElementById(id);
    o.push(`${id}: bbox=${r(e.getBBox())} ctm=${m(e.getCTM())} sctm=${m(e.getScreenCTM())} ` +
      `bcr=${r(e.getBoundingClientRect())} rects=${e.getClientRects().length}`);
  }
  const gr = document.getElementById("gr");
  gr.setAttribute("transform", "translate(2,2)");
  o.push(`changed: ctm=${m(gr.getCTM())} bbox=${r(document.getElementById("gt").getBBox())}`);
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        "s: bbox=(1,0,99,81) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,200,200) rects=1",
        "z: bbox=(5,6,0,10) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(5,6,0,10) rects=1",
        "gz: bbox=(1,1,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(1,1,0,0) rects=1",
        "gl: bbox=(50,60,31,21) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(50,60,31,21) rects=1",
        "p: bbox=(10,1.34,90,17.321) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(10,1.34,90,17.321) rects=1",
        "pe: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,0,0) rects=1",
        "c0: bbox=(5,6,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(5,6,0,0) rects=1",
        "e0: bbox=(5,3,0,6) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(5,3,0,6) rects=1",
        "sw: bbox=(7,0,4,4) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(7,0,4,4) rects=1",
        "sw1: bbox=(0,0,0,0) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(0,0,0,0) rects=0",
        "sw2: bbox=(7,0,4,4) ctm=[1,0,0,1,0,0] sctm=[1,0,0,1,0,0] bcr=(7,0,4,4) rects=1",
        "n: bbox=(0,0,5,5) ctm=[1,0,0,1,13,14] sctm=[1,0,0,1,13,14] bcr=(13,14,5,5) rects=1",
        "nr: bbox=(0,0,5,5) ctm=[1,0,0,1,13,14] sctm=[1,0,0,1,13,14] bcr=(13,14,5,5) rects=1",
        "gt: bbox=(1,1,5,5) ctm=[1,0,0,1,10,10] sctm=[1,0,0,1,10,10] bcr=(11,11,5,5) rects=1",
        "gr: bbox=(0,0,5,5) ctm=[1,0,0,1,11,11] sctm=[1,0,0,1,11,11] bcr=(11,11,5,5) rects=1",
        "sy: bbox=(0,0,1,1) ctm=[1,0,0,1,5,5] sctm=[1,0,0,1,5,5] bcr=(0,0,0,0) rects=0",
        "syr: bbox=(0,0,1,1) ctm=[1,0,0,1,5,5] sctm=[1,0,0,1,5,5] bcr=(0,0,0,0) rects=0",
        "dt: bbox=(0,0,191,191) ctm=[1,0,0,1,5,5] sctm=[1,0,0,1,5,5] bcr=(0,0,0,0) rects=0",
        "dtr: bbox=(0,0,1,1) ctm=[1,0,0,1,5,5] sctm=[1,0,0,1,5,5] bcr=(0,0,0,0) rects=0",
        "changed: ctm=[1,0,0,1,12,12] bbox=(2,2,5,5)",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// What is in a `<text>`: a `<tspan>`'s bounding box is its characters',
/// those of what is in it too, and its matrices the `<text>`'s; one with
/// `display: none` has neither box nor rect. A `<text>` with no
/// characters has no bounding box, but a client rect at its origin. (The
/// boxes themselves depend on the font: compared with each other.)
#[test]
fn svg_text_geometry() {
    let url = page(
        "svg-text.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<svg width=300 height=200>
 <text id=t x=10 y=100 font-size=20 transform="translate(5,6)">A<tspan id=o>B<tspan id=i>CD</tspan></tspan><tspan id=dn style="display:none">E</tspan></text>
 <text id=te x=10 y=100 transform="translate(7,8)"></text>
</svg>
<script>
onload = () => {
  const o = [];
  const n = v => +v.toFixed(3);
  const m = x => `[${[x.a, x.b, x.c, x.d, x.e, x.f].map(n)}]`;
  const r = x => `(${[x.x, x.y, x.width, x.height].map(n)})`;
  const byId = id => document.getElementById(id);
  const [t, ou, i] = ["t", "o", "i"].map(id => byId(id).getBBox());
  o.push(`o in t: ${ou.x > t.x && n(ou.x + ou.width) === n(t.x + t.width) && ou.y === t.y && ou.height === t.height}`);
  o.push(`i ends o: ${i.x > ou.x && n(i.x + i.width) === n(ou.x + ou.width) && i.y === ou.y}`);
  const b = byId("o").getBoundingClientRect();
  o.push(`o's rect: ${n(b.x) === n(ou.x + 5) && n(b.y) === n(ou.y + 6) && n(b.width) === n(ou.width)}`);
  for (const id of ["o", "dn", "te"]) {
    const e = byId(id);
    o.push(`${id}: ctm=${m(e.getCTM())} sctm=${m(e.getScreenCTM())} rects=${e.getClientRects().length}`);
  }
  o.push(`te: bbox=${r(byId("te").getBBox())} bcr=${r(byId("te").getBoundingClientRect())} dn: bbox=${r(byId("dn").getBBox())}`);
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        "o in t: true",
        "i ends o: true",
        "o's rect: true",
        "o: ctm=[1,0,0,1,5,6] sctm=[1,0,0,1,5,6] rects=1",
        "dn: ctm=[1,0,0,1,5,6] sctm=[1,0,0,1,5,6] rects=0",
        "te: ctm=[1,0,0,1,7,8] sctm=[1,0,0,1,7,8] rects=1",
        "te: bbox=(0,0,0,0) bcr=(7,8,0,0) dn: bbox=(0,0,0,0)",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// `SVGGeometryElement`'s methods, as Chrome 152 gives them: a shape's
/// length measured as Skia measures it (a circle shorter than 2πr), a
/// `<path>`'s along its path data as Blink does (`pathLength` aside, a
/// segment after a close from the subpath's start), its CSS `d` first;
/// points along them, the length clamped; one not rendered from its style
/// (percentages of nothing), a detached `<path>` from its `d`, a detached
/// `<rect>` with none; an empty path has no point. A point in the fill or
/// the stroke, as laid out (in `<defs>` too), the point a dictionary or an
/// `SVGPoint`.
#[test]
fn svg_shapes_lengths_and_points() {
    let url = page(
        "svg-lengths.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<svg width=300 height=200>
 <path id=p d="M0 0 L100 0 L100 50" pathLength=10 />
 <path id=pd style="display:none" d="M0 0 L30 40" />
 <defs><circle id=cd r=10 /></defs>
 <rect id=r x=10 y=10 width=50 height=20 stroke-width=10 />
 <rect id=rp x=10% y=10 width=50% height=20 />
 <rect id=rn style="display:none" width=10% height=10 />
 <svg style="display:none"><rect id=rh width=10% height=10 /></svg>
 <rect id=z x=5 y=5 width=0 height=10 />
 <circle id=c cx=50 cy=50 r=10 />
 <ellipse id=e cx=50 cy=50 rx=30 ry=10 />
 <rect id=rr width=40 height=20 rx=5 />
 <line id=l x1=0 y1=0 x2=3 y2=4 />
 <polygon id=pg points="0,0 10,0 10,10" />
 <path id=pe d="" />
 <path id=pm d="M5 5" />
 <path id=pa d="M60 50 A10 10 0 1 1 40 50 A10 10 0 1 1 60 50" />
 <path id=pq d="M0 0 Q50 100 100 0" />
 <path id=pz d="M0 0 L10 0 Z M20 0 L20 10" />
 <path id=pr d="M0 0 L10 0 Z L0 10" />
 <path id=css style="d: path('M0 0 L0 30')" d="M0 0 L10 0" />
 <path id=sq d="M10 80 L30 80" stroke-width=4 stroke-linecap=square />
 <path id=da d="M10 90 L50 90" stroke-width=2 stroke-dasharray="5 5" />
</svg>
<script>
onload = () => {
  const o = [];
  const n = v => +v.toFixed(3);
  const pt = p => `(${n(p.x)},${n(p.y)})`;
  const tryit = f => { try { return f(); } catch (e) { return "!" + e.name; } };
  const byId = id => document.getElementById(id);
  for (const id of "p pd cd r rp rn rh z c e rr l pg pe pm pa pq pz pr css".split(" ")) {
    const e = byId(id);
    const total = tryit(() => e.getTotalLength());
    const at = f => tryit(() => pt(e.getPointAtLength(typeof total === "number" ? total * f : 1)));
    o.push(`${id}: len=${typeof total === "number" ? n(total) : total} at=${[0.1, 0.5, 0.77].map(at)} ` +
      `ends=${tryit(() => pt(e.getPointAtLength(-5)))} ${tryit(() => pt(e.getPointAtLength(1e6)))}`);
  }
  const r = byId("r");
  const svgp = document.querySelector("svg").createSVGPoint();
  svgp.x = 20; svgp.y = 20;
  o.push(`fill: ${r.isPointInFill({x: 20, y: 20})} ${r.isPointInFill({x: 5, y: 20})} ${r.isPointInFill({x: 10, y: 10})} ` +
    `${r.isPointInFill(svgp)} ${r.isPointInFill()} ${r.isPointInFill({x: NaN, y: 20})}`);
  o.push(`stroke: ${r.isPointInStroke({x: 7, y: 20})} ${r.isPointInStroke({x: 20, y: 20})} ${r.isPointInStroke({x: 4, y: 20})}`);
  o.push(`others: ${byId("cd").isPointInFill({x: 1, y: 1})} ${byId("rn").isPointInFill({x: 1, y: 1})} ` +
    `${byId("c").isPointInStroke({x: 60, y: 50})} ${byId("pg").isPointInFill({x: 9, y: 1})} ${byId("pg").isPointInFill({x: 1, y: 9})}`);
  o.push(`stroke styles: ${byId("sq").isPointInStroke({x: 9, y: 80})} ${byId("da").isPointInStroke({x: 12, y: 90})} ` +
    `${byId("da").isPointInStroke({x: 17, y: 90})}`);
  const det = document.createElementNS("http://www.w3.org/2000/svg", "path");
  det.setAttribute("d", "M0 0 L20 0");
  const detr = document.createElementNS("http://www.w3.org/2000/svg", "rect");
  detr.setAttribute("width", "10");
  // Relative commands, from where the last one ended (a close goes back
  // to its subpath's start).
  const rel = document.createElementNS("http://www.w3.org/2000/svg", "path");
  rel.setAttribute("d", "m 10 10 l 20 0 z m 5 5 l 0 10");
  o.push(`detached: ${tryit(() => det.getTotalLength())} ${tryit(() => pt(det.getPointAtLength(5)))} ` +
    `${tryit(() => detr.getTotalLength())} ${detr.isPointInFill({x: 1, y: 1})} ` +
    `${n(rel.getTotalLength())} ${pt(rel.getPointAtLength(45))}`);
  o.push(`types: ${r.getPointAtLength(1) instanceof SVGPoint} ${tryit(() => r.getPointAtLength(NaN))} ${SVGGeometryElement.prototype.isPointInFill.length}`);
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        "p: len=150 at=(15,0),(75,0),(100,15.5) ends=(0,0) (100,50)",
        "pd: len=50 at=(3,4),(15,20),(23.1,30.8) ends=(0,0) (30,40)",
        "cd: len=62.427 at=(8.102,5.862),(-10,0),(1.203,-9.927) ends=(10,0) (10,0)",
        "r: len=140 at=(24,10),(60,30),(22.2,30) ends=(10,10) (10,10)",
        "rp: len=340 at=(64,10),(180,30),(88.2,30) ends=(30,10) (30,10)",
        "rn: len=20 at=(0,2),(0,10),(0,4.6) ends=(0,0) (0,0)",
        "rh: len=20 at=(0,2),(0,10),(0,4.6) ends=(0,0) (0,0)",
        "z: len=20 at=(5,7),(5,15),(5,9.6) ends=(5,5) (5,5)",
        "c: len=62.427 at=(58.102,55.862),(40,50),(51.203,40.073) ends=(60,50) (60,50)",
        "e: len=133.376 at=(69.937,57.472),(20,50),(52.598,40.038) ends=(80,50) (80,50)",
        "rr: len=110.615 at=(16.061,0),(35,20),(5.134,20) ends=(5,0) (5,0)",
        "l: len=5 at=(0.3,0.4),(1.5,2),(2.31,3.08) ends=(0,0) (3,4)",
        "pg: len=34.142 at=(3.414,0),(10,7.071),(5.553,5.553) ends=(0,0) (0,0)",
        "pe: len=0 at=!InvalidStateError,!InvalidStateError,!InvalidStateError ends=!InvalidStateError !InvalidStateError",
        "pm: len=0 at=(5,5),(5,5),(5,5) ends=(5,5) (5,5)",
        "pa: len=62.841 at=(58.091,55.878),(40,50),(51.254,40.078) ends=(60,50) (60,50)",
        "pq: len=147.894 at=(7.002,13.024),(50,50),(82.377,29.034) ends=(0,0) (100,0)",
        "pz: len=30 at=(3,0),(5,0),(20,3.1) ends=(0,0) (20,10)",
        "pr: len=30 at=(3,0),(5,0),(0,3.1) ends=(0,0) (0,10)",
        "css: len=30 at=(0,3),(0,15),(0,23.1) ends=(0,0) (0,30)",
        "fill: true false true true false false",
        "stroke: true false false",
        "others: true false true true false",
        "stroke styles: true true false",
        "detached: 20 (5,0) !InvalidStateError false 50 (15,20)",
        "types: true !TypeError 0",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// `SVGTextContentElement`'s methods: character numbers are UTF-16 code
/// units of the text, white space collapsed, each of a cluster's (an
/// emoji's, a combining mark's) with its cell; a `<tspan>`'s from its own
/// first; lengths, positions, extents and turns from the cells; out of
/// range, an `IndexSizeError`; a `<text>` not rendered has no characters.
/// (Font-dependent values are compared with each other.)
#[test]
fn svg_text_methods() {
    let url = page(
        "svg-text-methods.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<svg width=300 height=200>
 <text id=t x=10 y=100 font-size=20>Ab<tspan id=ts>c&#x1F600;d</tspan></text>
 <text id=te x=10 y=150></text>
 <text id=tr x=10 y=150 rotate="10 20">xyz</text>
 <text id=tc x=0 y=180>  e&#x301;x  </text>
 <text id=tn style="display:none" x=10 y=10>abc</text>
</svg>
<script>
onload = () => {
  const o = [];
  const n = v => +v.toFixed(3);
  const tryit = f => { try { return f(); } catch (e) { return "!" + e.name; } };
  const byId = id => document.getElementById(id);
  const t = byId("t"), ts = byId("ts");
  o.push(`chars: ${t.getNumberOfChars()} ${ts.getNumberOfChars()} ${byId("te").getNumberOfChars()} ${byId("tc").getNumberOfChars()} ${byId("tn").getNumberOfChars()}`);
  const sub = (e, a, b) => n(e.getSubStringLength(a, b));
  const ext = (e, i) => e.getExtentOfChar(i);
  const start = (e, i) => e.getStartPositionOfChar(i);
  const end = (e, i) => e.getEndPositionOfChar(i);
  const total = n(t.getComputedTextLength());
  o.push(`length is the sum: ${total === sub(t, 0, 100)} ${total === sub(t, 0, -1)} tspan: ${n(ts.getComputedTextLength()) === sub(t, 2, 4)}`);
  o.push(`the emoji's units: ${sub(t, 3, 1) === sub(t, 4, 1)} ${sub(t, 3, 2) === sub(t, 3, 1)} ` +
    `${start(t, 3).x === start(t, 4).x} ${end(t, 3).x === end(t, 4).x} ${n(ext(t, 3).width) === sub(t, 3, 1)}`);
  o.push(`in order: ${start(t, 0).x === 10} ${start(t, 0).y === 100} ${end(t, 0).y === 100 && end(t, 0).x > 10} ` +
    `${n(start(ts, 0).x) === n(start(t, 2).x)} ${n(ext(t, 0).width) === sub(t, 0, 1)} ${ext(t, 0).y < 100 && ext(t, 0).y + ext(t, 0).height > 100}`);
  const tr = byId("tr");
  const e1 = ext(tr, 1), w1 = tr.getSubStringLength(1, 1);
  o.push(`rotated: ${[0, 1, 2].map(i => tr.getRotationOfChar(i))} ${t.getRotationOfChar(0)} ` +
    `${n(end(tr, 0).x - start(tr, 0).x) === n(tr.getSubStringLength(0, 1) * Math.cos(Math.PI / 18))} ${e1.width > w1 * Math.cos(Math.PI / 9) - 0.01}`);
  const tc = byId("tc");
  o.push(`a cluster: ${start(tc, 0).x === start(tc, 1).x} ${sub(tc, 0, 2) === sub(tc, 0, 1)} ${start(tc, 0).x === 0} ${start(tc, 2).x > start(tc, 0).x}`);
  const at = (x, y) => t.getCharNumAtPosition({x, y});
  const mid = i => ({x: start(t, i).x + 1, y: 95});
  o.push(`at: ${at(mid(0).x, 95)} ${at(mid(1).x, 95)} ${at(mid(4).x, 95)} ${at(mid(5).x, 95)} ${at(0, 0)} ${t.getCharNumAtPosition()} ` +
    `${ts.getCharNumAtPosition(mid(5))} ${ts.getCharNumAtPosition(mid(0))}`);
  o.push(`out of range: ${tryit(() => start(t, 6))} ${tryit(() => end(t, -1))} ${tryit(() => t.getRotationOfChar(9))} ` +
    `${tryit(() => ext(byId("tn"), 0))} ${tryit(() => sub(t, 6, 1))} ${tryit(() => sub(t, 5, 0))}`);
  o.push(`select: ${tryit(() => t.selectSubString(0, 2))} ${tryit(() => t.selectSubString(6, 1))}`);
  o.push(`types: ${start(t, 0) instanceof SVGPoint} ${ext(t, 0) instanceof SVGRect} ${SVGTextContentElement.prototype.getCharNumAtPosition.length}`);
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        "chars: 6 4 0 3 0",
        "length is the sum: true true tspan: true",
        "the emoji's units: true true true true true",
        "in order: true true true true true true",
        "rotated: 10,20,20 0 true true",
        "a cluster: true true true true",
        "at: 0 1 3 5 -1 -1 3 -1",
        "out of range: !IndexSizeError !IndexSizeError !IndexSizeError !IndexSizeError !IndexSizeError 0",
        "select: undefined !IndexSizeError",
        "types: true true 0",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// A range's boxes from the renderer's layout (Ahem, every glyph a 1em
/// square): its text's part on each line, as tall as the font; a
/// collapsed range one of no width; the viewport's scroll taken off, but
/// not for text in a box fixed to the viewport; an element the range
/// holds whole, its border box and its text's.
#[test]
fn a_ranges_boxes() {
    let url = page(
        "range.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } p { font: 20px/1 Ahem; margin: 0; width: 100px }</style>
<p id="p">XX XX XX</p>
<p id="fixed" style="position: fixed; top: 100px; left: 0">XX</p>
<div style="height: 2000px"></div>
<script>
onload = () => {
  const log = [];
  const rect = r => `${r.x},${r.y},${r.width},${r.height}`;
  const p = document.getElementById('p');
  const r = document.createRange();
  r.setStart(p.firstChild, 1);
  r.setEnd(p.firstChild, 8);
  log.push([...r.getClientRects()].map(rect).join(' '), rect(r.getBoundingClientRect()));
  r.collapse(false);
  r.setStart(p.firstChild, 2);
  r.collapse(true);
  log.push([...r.getClientRects()].map(rect).join(' '));
  scrollTo(0, 10);
  r.selectNode(p);
  log.push([...r.getClientRects()].map(rect).join(' '));
  r.selectNodeContents(document.getElementById('fixed').firstChild);
  log.push([...r.getClientRects()].map(rect).join(' '));
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "20,0,80,20 0,20,40,20 / 0,0,100,40 / 40,0,0,20 / \
         0,-10,100,40 0,-10,100,20 0,10,40,20 / 0,100,40,20"
    );
}

/// A table's parts have boxes: the table's border box holds its caption,
/// a row group and a row span its columns (not the border spacing around
/// them), cells are hit, and a cell's and a row's `offsetParent` is the
/// table.
#[test]
fn tables_have_boxes() {
    let url = page(
        "tables.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } td { padding: 0; width: 50px; height: 20px }</style>
<table id="t" border="1" cellspacing="3"><caption id="cap" style="height: 10px"></caption>
<tbody id="g"><tr id="r"><td id="a"></td><td id="b"></td></tr></tbody></table>
<script>
onload = () => {
  const log = [];
  const rect = id => {
    const r = document.getElementById(id).getBoundingClientRect();
    return `${r.x},${r.y},${r.width},${r.height}`;
  };
  log.push(...['t', 'cap', 'g', 'r', 'a', 'b'].map(rect));
  const a = document.getElementById('a');
  log.push(a.offsetParent.id, a.offsetLeft, a.offsetWidth, a.offsetHeight,
           document.getElementById('r').offsetParent.id);
  log.push(document.elementFromPoint(70, 20).id, document.elementFromPoint(50, 5).id);
  testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "0,0,115,40 / 0,0,115,10 / 4,14,107,22 / 4,14,107,22 / 4,14,52,22 / 59,14,52,22 / \
         t / 3 / 52 / 22 / t / b / cap"
    );
}

/// `transform-origin` in the reference box (`transform-box`): an SVG
/// shape turned about its object bounding box's center, about its stroke
/// bounding box's corner (a rect's corners reach half the stroke's width
/// past it, a path's as far as a miter), and by default about its
/// viewport's origin;
/// `getComputedStyle` resolves a box's origin to px in its reference box
/// and its `transform` to the matrix without the origin, as Chrome does;
/// a box with none has the computed value.
#[test]
fn transform_origins_in_their_reference_boxes() {
    let url = page(
        "transform-origins.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<svg id=s width=200 height=100 style="display:block">
 <rect id=fill x=10 y=20 width=40 height=20 style="transform-box:fill-box;transform-origin:center;transform:rotate(90deg)" />
 <rect id=stroke x=10 y=20 width=40 height=20 stroke=black stroke-width=4
   style="transform-box:stroke-box;transform-origin:0 0;transform:rotate(90deg)" />
 <rect id=view x=10 y=20 width=40 height=20 style="transform:rotate(90deg)" />
 <path id=path d="M10 20h40v20h-40z" stroke=black stroke-width=4
   style="transform-box:stroke-box;transform-origin:0 0;transform:rotate(90deg)" />
</svg>
<div id=d style="width:100px;height:40px;padding:10px;border:3px solid;transform:rotate(90deg)"></div>
<div id=c style="width:100px;height:40px;padding:10px;transform-box:content-box;transform-origin:right 10%"></div>
<div id=n style="display:none;transform-origin:left"></div>
<script>
onload = () => {
  const o = [];
  const n = v => +v.toFixed(3);
  const m = x => `[${[x.a, x.b, x.c, x.d, x.e, x.f].map(n)}]`;
  const r = x => `(${[x.x, x.y, x.width, x.height].map(n)})`;
  for (const id of ["fill", "stroke", "view", "path"]) {
    const e = document.getElementById(id);
    o.push(`${id}: ctm=${m(e.getCTM())} bcr=${r(e.getBoundingClientRect())}`);
  }
  for (const id of ["d", "c", "n"]) {
    const s = getComputedStyle(document.getElementById(id));
    o.push(`${id}: ${s.transformOrigin} | ${s.transform} | ${s.transformBox}`);
  }
  testRunner.reportHarnessResults(o.join("\n"));
};
</script>"##,
    );
    let expected = [
        // About (30, 30): (x, y) to (60 - y, x).
        "fill: ctm=[0,1,-1,0,60,0] bcr=(20,10,20,40)",
        // About the stroke box's corner (8, 18): (x, y) to (26 - y, x + 10).
        "stroke: ctm=[0,1,-1,0,26,10] bcr=(-14,20,20,40)",
        "view: ctm=[0,1,-1,0,0,0] bcr=(-40,10,20,40)",
        // A path's corners can reach as far as a miter (`stroke-
        // miterlimit` 4 times half the width): about (2, 12).
        "path: ctm=[0,1,-1,0,14,10] bcr=(-26,20,20,40)",
        "d: 63px 33px | matrix(0, 1, -1, 0, 0, 0) | view-box",
        "c: 100px 4px | none | content-box",
        "n: 0% 50% | none | view-box",
    ];
    assert_eq!(report(&url), expected.join("\n"));
}

/// `overflow: clip` isn't a scroll container: script can't scroll it, it
/// scrolls nothing into view (its parent does), and no offset is kept for
/// it to have once it becomes one. An `<img>` clips (the UA stylesheet),
/// and says so.
#[test]
fn overflow_clip_scrolls_nothing() {
    let url = page(
        "overflow-clip.html",
        r##"<!DOCTYPE html><style>body{margin:0}</style>
<div id=c style="overflow:clip;height:50px"><div style="height:200px"></div><span id=t>t</span></div>
<div style="height:2000px"></div>
<div id=h style="overflow:hidden;height:50px"><div style="height:200px"></div></div>
<img id=i>
<script>
onload = () => {
  const c = document.getElementById("c"), h = document.getElementById("h");
  c.scrollTop = 30;
  h.scrollTop = 30;
  c.scrollTo(0, 40);
  const before = c.scrollTop;
  document.getElementById("t").scrollIntoView();
  const into = c.scrollTop;
  const overflow = getComputedStyle(c).overflow;
  // Nothing was kept for it either: a scroll container now, it is at 0.
  c.style.overflow = "hidden";
  testRunner.reportHarnessResults([
    `clip: ${before} ${into} ${scrollY > 0} ${overflow} ${c.scrollTop}`,
    `hidden: ${h.scrollTop}`,
    `img: ${getComputedStyle(document.getElementById("i")).overflow}`,
  ].join("\n"));
};
</script>"##,
    );
    // Scrolled into view, the span is past the clip: the document scrolls
    // down to it, the box doesn't.
    let expected = ["clip: 0 0 true clip 0", "hidden: 30", "img: clip"];
    assert_eq!(report(&url), expected.join("\n"));
}

/// `MouseEvent`'s `layerX` and `layerY`, as Chrome 152 gives them: from
/// the border box of the nearest box around the target (or its nearest
/// ancestor with one), the root element's too, that is positioned or a
/// stacking context (a
/// transform, `opacity`, `contain: paint`, a flex item's `z-index`; not a
/// clip, a scroller, a `z-index` that doesn't apply or a `display:
/// contents` element), in the viewport; else `pageX` and `pageY`. Read
/// when asked, floored and clamped to a `long`; the shadow host's for an
/// event retargeted to it. (A box in a fixed one isn't scrolled here:
/// its rect is off by the scroll.) The DOM gives the same.
#[test]
fn mouse_event_layer_coordinates() {
    let url = page(
        "layer.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } body { height: 3000px } .t { width: 40px; height: 40px; margin: 5px; border: 3px solid; padding: 2px; box-sizing: border-box }</style>
<div style="height: 30px"></div>
<div id="rel" style="position: relative; left: 20px; top: 10px; margin-left: 30px; border: 4px solid; padding: 6px; width: 300px"><div id="inRel" class="t"></div><div id="none" style="display: none"></div></div>
<div style="position: absolute; left: 400px; top: 50px; border: 2px solid; width: 300px; height: 200px"><div style="position: relative; left: 10px; top: 20px; margin: 7px; border: 1px solid; height: 100px"><div id="inNested" class="t"></div></div></div>
<div style="transform: translate(15px, 25px); margin-left: 50px; width: 300px"><div id="inTransform" class="t"></div></div>
<div style="opacity: 0.5; margin-left: 60px; width: 300px"><div id="inOpacity" class="t"></div></div>
<div style="overflow: hidden; margin-left: 70px; width: 300px; border: 5px solid"><div id="inHidden" class="t"></div></div>
<div id="scroller" style="overflow: scroll; margin-left: 80px; width: 300px; height: 100px"><div style="height: 50px"></div><div id="inScroller" class="t"></div><div style="height: 300px"></div></div>
<div style="margin-left: 90px; width: 300px"><div id="inPlain" class="t"></div></div>
<div style="position: static; z-index: 5; margin-left: 10px"><div id="inStatic" class="t"></div></div>
<div style="contain: paint; margin-left: 10px"><div id="inContained" class="t"></div></div>
<div style="display: flex"><div style="z-index: 1; margin-left: 13px"><div id="inFlexItem" class="t"></div></div></div>
<div style="position: relative; left: 40px"><div style="display: contents; position: relative"><div id="inContents" class="t"></div></div></div>
<div style="position: relative; left: 0.6px; width: 50px"><div id="inFraction" class="t"></div></div>
<div id="host" style="position: relative; left: 30px"></div>
<div style="position: fixed; left: 500px; top: 400px; width: 100px; height: 100px"><div id="inFixed" class="t"></div></div>
<script>
function run() {
  const log = [];
  document.getElementById('scroller').scrollTop = 20;
  const shadow = document.getElementById('host').attachShadow({ mode: 'open' });
  shadow.innerHTML = '<div id="inShadow" style="position: relative; left: 5px; height: 20px"></div>';
  const click = (target, x, y, init) =>
    new MouseEvent('click', { clientX: x, clientY: y, bubbles: true, composed: true, view: window, ...init });
  const at = (target, x, y, listening = target) => {
    let got;
    listening.addEventListener('click', e => got = `${e.layerX},${e.layerY}`, { once: true });
    target.dispatchEvent(click(target, x, y));
    return got;
  };
  // 10px into each target's box.
  for (const id of ['inRel', 'rel', 'inNested', 'inTransform', 'inOpacity', 'inHidden', 'inScroller',
                    'inPlain', 'inStatic', 'inContained', 'inFlexItem', 'inContents', 'inFraction', 'inFixed']) {
    const target = document.getElementById(id);
    const r = target.getBoundingClientRect();
    log.push(`${id} ${at(target, r.left + 10, r.top + 10)}`);
  }
  const rel = document.getElementById('rel');
  // A node without a box: its nearest ancestor's layer; a text node's;
  // one in no document, page coordinates.
  log.push(`none ${at(document.getElementById('none'), 100, 60)}`);
  const text = rel.appendChild(document.createTextNode('x'));
  log.push(`text ${at(text, 100, 60)}`);
  // (Its line's height is its font's: gone again, for what follows.)
  text.remove();
  log.push(`detached ${at(document.createElement('div'), 100, 60)}`);
  // Retargeted to the shadow host: the host's layer.
  const host = document.getElementById('host');
  log.push(`shadow ${at(shadow.getElementById('inShadow'), 50, host.getBoundingClientRect().top + 5, host)}`);
  // Never dispatched: pageX; floored, clamped to a long.
  const lone = click(null, 10.7, -3.2);
  log.push(`lone ${lone.layerX},${lone.layerY}`);
  const far = click(null, 1e12, -1e12);
  log.push(`far ${far.layerX},${far.layerY}`);
  // Read after the layer moved: from where it is now.
  const inRel = document.getElementById('inRel');
  const r = inRel.getBoundingClientRect();
  let later;
  inRel.addEventListener('click', e => later = e, { once: true });
  inRel.dispatchEvent(click(inRel, r.left + 10, r.top + 10));
  rel.style.left = '120px';
  log.push(`later ${later.layerX},${later.layerY}`);
  // The page scrolled: still the layer's box, in the viewport.
  rel.style.left = '20px';
  scrollTo(0, 15);
  const s = inRel.getBoundingClientRect();
  log.push(`scrolled ${at(inRel, s.left + 10, s.top + 10)}`);
  // A positioned root element is a layer too.
  scrollTo(0, 0);
  document.documentElement.style.cssText = 'position: relative; left: 40px; top: 30px';
  const plain = document.getElementById('inPlain');
  const p = plain.getBoundingClientRect();
  log.push(`positioned root ${at(plain, p.left + 10, p.top + 10)}`);
  return log.join(' / ');
}
onload = () => testRunner.reportHarnessResults(run());
</script>"#,
    );
    let expected = [
        "inRel 25,25",
        "rel 10,10",
        "inNested 16,16",
        "inTransform 15,10",
        "inOpacity 15,10",
        "inHidden 90,215",
        "inScroller 95,300",
        "inPlain 105,370",
        "inStatic 25,415",
        "inContained 15,15",
        "inFlexItem 15,15",
        "inContents 15,10",
        "inFraction 15,10",
        "inFixed 15,15",
        "none 50,20",
        "text 50,20",
        "detached 100,60",
        "shadow 20,5",
        "lone 10,-4",
        "far 2147483647,-2147483648",
        "later -75,25",
        "scrolled 25,25",
        "positioned root 105,370",
    ];
    assert_eq!(report(&url), expected.join(" / "));
    assert_eq!(report(&url), expected.join(" / "), "the DOM");
}
