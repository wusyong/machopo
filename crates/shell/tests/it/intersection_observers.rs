//! `IntersectionObserver` in a whole browser: the rendering update
//! computes each observation's intersection after the lifecycle, and a task
//! of the observer's window delivers the entries. Each page reports what
//! its callbacks saw through `testRunner` (`headless::run_testharness`):
//! the first entries, what a change brings (and what doesn't), the order
//! within a frame, the roots and margins, a child frame's targets and
//! observers, a cross-origin frame, a callback's exception, and a scroll
//! by the wheel.

use std::path::PathBuf;
use std::time::Duration;

use protocol::input::{InputEvent, Modifiers, ScrollDelta};
use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-intersection-observers");
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

/// What each page's script has: `log`, `el(id)`, `rect(r)` (left, top,
/// width, height) and `entry(e)` (the target's id, its rects, whether it
/// intersects and its ratio), and `frames(n, f)`, `f` after `n` animation
/// frames and a task (entries come in a task after the frame).
const HELPERS: &str = r#"<script>
const log = [];
const el = id => document.getElementById(id);
const rect = r => r ? `${r.left},${r.top},${r.width},${r.height}` : 'null';
const entry = e => `${e.target.id} ${rect(e.boundingClientRect)} ` +
  `${rect(e.intersectionRect)} ${e.isIntersecting} ${e.intersectionRatio}`;
const frames = (n, f) => requestAnimationFrame(() =>
  n > 1 ? frames(n - 1, f) : setTimeout(f, 0));
const done = () => testRunner.reportHarnessResults(log.join(' / '));
</script>"#;

/// The first computation gives every target an entry, whatever it finds:
/// in the viewport, partly, edge to edge (an empty intersection), below
/// it, or no box (whose root bounds are zero too, as in Chrome); in the
/// order observed, in one callback, after the frame (its animation frame
/// callbacks have run), with the observer as `this` and the second
/// argument; the root bounds the viewport; one time for all,
/// `performance.now()`'s then.
#[test]
fn the_first_entries() {
    let url = page(
        "first.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>html, body {{ margin: 0 }} div {{ width: 100px; height: 100px }}</style>
<div id="in"></div>
<div id="part" style="position: absolute; top: 550px"></div>
<div id="edge" style="position: absolute; top: 600px; left: 200px"></div>
<div id="below" style="position: absolute; top: 700px"></div>
<div id="none" style="display: none"></div>
<script>
const before = performance.now();
const observer = new IntersectionObserver(function (entries, second) {{
  log.push(`callback this ${{this === observer}} ${{second === observer}} ${{entries.length}}`);
  log.push(`time ${{entries[0].time >= before && entries[0].time <= performance.now()}} ` +
           `${{entries.every(e => e.time === entries[0].time)}}`);
  log.push(`root ${{rect(entries[0].rootBounds)}}`);
  for (const e of entries) log.push(entry(e) + (e.target.id == 'none' ? ' ' + rect(e.rootBounds) : ''));
}});
for (const id of ['in', 'part', 'edge', 'below', 'none']) observer.observe(el(id));
log.push(`records ${{observer.takeRecords().length}}`);
requestAnimationFrame(() => log.push('frame'));
frames(2, done);
</script>"#
        ),
    );
    assert_eq!(
        report(&url),
        "records 0 / frame / callback this true true 5 / time true true / root 0,0,800,600 / \
         in 0,0,100,100 0,0,100,100 true 1 / \
         part 0,550,100,100 0,550,100,50 true 0.5 / \
         edge 200,600,100,100 200,600,100,0 true 0 / \
         below 0,700,100,100 0,0,0,0 false 0 / \
         none 0,0,0,0 0,0,0,0 false 0 0,0,0,0"
    );
}

/// After the first: an entry only when the threshold index or whether it
/// intersects changed, at the next frame; scrolling and moving count. A
/// ratio below the first threshold doesn't intersect (as Chrome; WPT's
/// `isIntersecting-threshold.html`), though its intersection rect and
/// ratio are given. Unobserved and disconnected, nothing more.
#[test]
fn what_a_change_brings() {
    let url = page(
        "change.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>html, body {{ margin: 0 }} body {{ height: 3000px }}
div {{ width: 100px; height: 100px; position: absolute }}</style>
<div id="a" style="top: 575px"></div>
<div id="b" style="top: 0"></div>
<script>
const observer = new IntersectionObserver(entries => {{
  log.push(entries.map(entry).join(', '));
}}, {{threshold: [0.5, 1]}});
observer.observe(el('a'));
const other = new IntersectionObserver(entries => {{
  log.push('other ' + entries.map(entry).join(', '));
}});
other.observe(el('b'));
const steps = [
  () => {{ window.scrollTo(0, 25); log.push('scrolled 25'); }},
  () => {{ window.scrollTo(0, 60); log.push('scrolled 60'); }},
  () => {{ el('a').style.top = '500px'; log.push('moved'); }},
  () => {{
    window.scrollTo(0, 0);
    el('a').style.top = '1000px';
    el('b').style.top = '1000px';
  }},
  () => {{
    observer.unobserve(el('a'));
    other.disconnect();
    el('a').style.top = '0';
    el('b').style.top = '0';
  }},
];
const next = () => {{
  const step = steps.shift();
  if (!step) return done();
  step();
  frames(2, next);
}};
frames(2, next);
</script>"#
        ),
    );
    assert_eq!(
        report(&url),
        "a 0,575,100,100 0,575,100,25 false 0.25 / other b 0,0,100,100 0,0,100,100 true 1 / \
         scrolled 25 / a 0,550,100,100 0,550,100,50 true 0.5 / \
         scrolled 60 / \
         moved / a 0,440,100,100 0,440,100,100 true 1 / \
         a 0,1000,100,100 0,0,0,0 false 0 / other b 0,1000,100,100 0,0,0,0 false 0"
    );
}

/// `observe()` asks for a frame: observing in an idle callback, with
/// nothing else to draw, gets the first entry.
#[test]
fn observing_asks_for_a_frame() {
    let url = page(
        "idle.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<div id="t" style="width: 10px; height: 10px"></div>
<script>
onload = () => requestIdleCallback(() => requestIdleCallback(() => {{
  new IntersectionObserver(entries => {{
    log.push(entries.map(e => `${{e.target.id}} ${{e.isIntersecting}}`).join());
    done();
  }}).observe(el('t'));
}}));
</script>"#
        ),
    );
    assert_eq!(report(&url), "t true");
}

/// Within a frame: the animation frame callbacks, then the resize
/// observers, then (in a task after the frame) the intersection observers
/// (WPT's `resize-observer/ordering.html`).
#[test]
fn the_order_in_a_frame() {
    let url = page(
        "order.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<div id="t" style="width: 10px; height: 10px"></div>
<script>
new IntersectionObserver(() => log.push('intersection')).observe(el('t'));
new ResizeObserver(() => log.push('resize')).observe(el('t'));
requestAnimationFrame(() => log.push('frame'));
frames(2, done);
</script>"#
        ),
    );
    assert_eq!(report(&url), "frame / resize / intersection");
}

/// An element as the root: its padding box, a scroll container in it
/// clipping, the root margin (percentages of the root) and the scroll
/// margin; a target not in it doesn't intersect it and has no rects (the
/// root bounds zero too, as in Chrome). The
/// document as the root: its viewport.
#[test]
fn an_element_root_and_the_margins() {
    let url = page(
        "root.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>html, body {{ margin: 0 }}</style>
<div id="root" style="width: 200px; height: 100px; border: 10px solid; overflow: hidden">
  <div id="scroller" style="height: 80px; overflow: hidden">
    <div style="height: 100px"></div>
    <div id="t" style="width: 50px; height: 50px"></div>
  </div>
</div>
<div id="outside" style="width: 10px; height: 10px"></div>
<script>
const observe = (options, ids) => new Promise(resolve => {{
  const observer = new IntersectionObserver(entries => {{
    log.push(entries.map(e => `${{entry(e)}} root ${{rect(e.rootBounds)}}`).join(', '));
    observer.disconnect();
    resolve();
  }}, {{root: el('root'), ...options}});
  for (const id of ids) observer.observe(el(id));
}});
(async () => {{
  await observe({{}}, ['t', 'outside']);
  await observe({{rootMargin: '10%'}}, ['t']);
  await observe({{scrollMargin: '30px'}}, ['t']);
  await observe({{root: document}}, ['outside']);
  done();
}})();
</script>"#
        ),
    );
    assert_eq!(
        report(&url),
        "t 10,110,50,50 0,0,0,0 false 0 root 10,10,200,100, \
         outside 0,0,0,0 0,0,0,0 false 0 root 0,0,0,0 / \
         t 10,110,50,50 0,0,0,0 false 0 root -10,0,240,120 / \
         t 10,110,50,50 10,110,50,0 true 0 root 10,10,200,100 / \
         outside 0,120,10,10 0,120,10,10 true 1 root 0,0,800,600"
    );
}

/// A target in a child frame, the implicit root: cut to the iframe's
/// content box, its rects in its own viewport, the root bounds the top
/// viewport's; observed from the page and from the child frame (whose
/// observer is called in its own realm); the child frame scrolled.
#[test]
fn a_child_frame() {
    let url = page(
        "child.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>html, body {{ margin: 0 }}</style>
<div style="height: 100px"></div>
<iframe id="f" style="width: 200px; height: 100px; border: 0; display: block"
  srcdoc="<style>body {{ margin: 0 }}</style><div style='height: 80px'></div>
          <div id=t style='width: 50px; height: 50px'></div><div style='height: 500px'></div>"></iframe>
<script>
// What the child frame's callback reports through (`const`s aren't the
// window's properties).
window.fromChild = entries => log.push(
  'child ' + (entries[0] instanceof el('f').contentWindow.IntersectionObserverEntry) + ' ' +
  entry(entries[0]));
onload = () => {{
  const child = el('f').contentWindow;
  const t = el('f').contentDocument.getElementById('t');
  const thresholds = {{threshold: [0, 0.5, 1]}};
  const page = new IntersectionObserver(entries => {{
    log.push('page ' + entries.map(e => `${{entry(e)}} root ${{rect(e.rootBounds)}}`).join());
  }}, thresholds);
  page.observe(t);
  child.eval(`new IntersectionObserver(entries => parent.fromChild(entries),
                                       {{threshold: [0, 0.5, 1]}})
                .observe(document.getElementById('t'))`);
  frames(2, () => {{
    child.scrollTo(0, 40);
    frames(2, done);
  }});
}};
</script>"#
        ),
    );
    assert_eq!(
        report(&url),
        "page t 0,80,50,50 0,80,50,20 true 0.4 root 0,0,800,600 / \
         child true t 0,80,50,50 0,80,50,20 true 0.4 / \
         page t 0,40,50,50 0,40,50,50 true 1 root 0,0,800,600 / \
         child true t 0,40,50,50 0,40,50,50 true 1"
    );
}

/// A callback's exception is reported (at its realm's window); the next
/// observer is called back anyway.
#[test]
fn an_exception() {
    let url = page(
        "exception.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<div id="t" style="width: 10px; height: 10px"></div>
<script>
onerror = message => {{ log.push('error ' + message); return true; }};
new IntersectionObserver(() => {{ throw new Error('thrown'); }}).observe(el('t'));
new IntersectionObserver(() => log.push('second')).observe(el('t'));
frames(2, done);
</script>"#
        ),
    );
    assert_eq!(report(&url), "error Uncaught Error: thrown / second");
}

/// A target in a document no frame shows, with the implicit root, gets no
/// entry until it is in one (as Chrome; WPT's
/// `target-in-detached-document.html`); then its first.
#[test]
fn a_target_in_no_frame() {
    let url = page(
        "detached.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>html, body {{ margin: 0 }}</style>
<script>
const other = document.implementation.createHTMLDocument('');
const t = other.createElement('div');
t.id = 't';
t.style.cssText = 'width: 10px; height: 10px';
other.body.appendChild(t);
new IntersectionObserver(entries => log.push(entries.map(entry).join())).observe(t);
frames(2, () => {{
  log.push('adopted');
  document.body.appendChild(t);
  frames(2, done);
}});
</script>"#
        ),
    );
    assert_eq!(report(&url), "adopted / t 0,0,10,10 0,0,10,10 true 1");
}

/// An observer's `delay`: an observation is computed again no sooner than
/// that after the last time, whatever changed in between; and once the
/// time is up, a frame comes to compute it, though nothing else asks for
/// one (Blink's `ScheduleDelayedIntersection`). The target moves out of
/// view right after the first entry, and nothing changes after.
#[test]
fn a_delay_holds_the_next_computation_back() {
    let url = page(
        "delay.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<div id="t" style="position: absolute; top: 0; width: 10px; height: 10px"></div>
<script>
const times = [];
new IntersectionObserver(entries => {{
  for (const e of entries) times.push([e.time, e.isIntersecting]);
  if (times.length == 1) el('t').style.top = '2000px';
  if (times.length == 2) {{
    const gap = times[1][0] - times[0][0];
    log.push(times.map(t => t[1]).join(), gap >= 99, gap < 1000);
    done();
  }}
}}, {{delay: 100}}).observe(el('t'));
</script>"#
        ),
    );
    assert_eq!(report(&url), "true,false / true / true");
}

/// An observer that tracks visibility: a target is visible unless what it
/// is painted with distorts it (an opacity below 1 around it, a rotation),
/// or it doesn't intersect; one that tracks nothing says `false`.
#[test]
fn what_tracking_visibility_finds() {
    let url = page(
        "visible.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>body {{ margin: 0 }} div div {{ width: 20px; height: 20px }}</style>
<div><div id="plain"></div></div>
<div style="opacity: 0.5"><div id="faded"></div></div>
<div style="transform: rotate(10deg)"><div id="turned"></div></div>
<div style="position: absolute; top: 5000px"><div id="below"></div></div>
<script>
const seen = {{}};
const report = () => {{
  log.push(['plain', 'faded', 'turned', 'below', 'untracked'].map(id => `${{id}} ${{seen[id]}}`).join(', '));
  done();
}};
const tracking = new IntersectionObserver(entries => {{
  for (const e of entries) seen[e.target.id] = e.isVisible;
  if (seen.untracked !== undefined) report();
}}, {{trackVisibility: true, delay: 100}});
for (const id of ['plain', 'faded', 'turned', 'below']) tracking.observe(el(id));
new IntersectionObserver(entries => {{
  seen.untracked = entries[0].isVisible;
  if (seen.plain !== undefined) report();
}}).observe(el('plain'));
</script>"#
        ),
    );
    assert_eq!(
        report(&url),
        "plain true, faded false, turned false, below false, untracked false"
    );
}

/// What covers a target makes it not visible: a box painted after it over
/// it, `pointer-events: none` too; and once a shadow that only paint
/// changes (no layout) reaches over a visible one, well after the last
/// computation, it no longer is: the paint alone computes again.
#[test]
fn what_covers_a_tracked_target() {
    let url = page(
        "covered.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>body {{ margin: 0 }} .t, .o {{ position: absolute; width: 20px; height: 20px }}</style>
<div class="t" id="covered" style="left: 0; top: 0"></div>
<div class="o" style="left: 10px; top: 10px"></div>
<div class="t" id="through" style="left: 100px; top: 0"></div>
<div class="o" style="left: 110px; top: 10px; pointer-events: none"></div>
<div class="t" id="shadowed" style="left: 200px; top: 0"></div>
<div class="o" id="caster" style="left: 250px; top: 0"></div>
<script>
const seen = [];
const observer = new IntersectionObserver(entries => {{
  for (const e of entries) seen.push(`${{e.target.id}} ${{e.isVisible}}`);
  // Past the delay, so that nothing waits to be computed again.
  if (seen.length == 3) setTimeout(() => el('caster').style.boxShadow = '-40px 0 0 red', 200);
  if (seen.length == 4) {{ log.push(seen.join(', ')); done(); }}
}}, {{trackVisibility: true, delay: 100}});
for (const id of ['covered', 'through', 'shadowed']) observer.observe(el(id));
</script>"#
        ),
    );
    assert_eq!(
        report(&url),
        "covered false, through false, shadowed true, shadowed false"
    );
}

/// What is in a tracked target covers nothing: an iframe's child frame,
/// what the target's shadow tree has (both in it in the flat tree and
/// through the frames, as Blink's hit test stops at the target).
#[test]
fn what_is_in_a_tracked_target_doesnt_cover_it() {
    let url = page(
        "in-target.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>body {{ margin: 0 }}</style>
<iframe id="framed" style="border: 0; width: 100px; height: 50px; display: block"
  srcdoc="<body style='margin: 0; background: blue'><p>a frame</p>"></iframe>
<div id="host" style="width: 100px; height: 50px"></div>
<script>
el('host').attachShadow({{mode: 'open'}}).innerHTML =
  '<div style="height: 50px; background: blue">shadow</div>';
onload = () => {{
  const seen = [];
  const observer = new IntersectionObserver(entries => {{
    for (const e of entries) seen.push(`${{e.target.id}} ${{e.isVisible}}`);
    if (seen.length == 2) {{ log.push(seen.join(', ')); done(); }}
  }}, {{trackVisibility: true, delay: 100}});
  for (const id of ['framed', 'host']) observer.observe(el(id));
}};
</script>"#
        ),
    );
    assert_eq!(report(&url), "framed true, host true");
}

/// A scroll the compositor did (the wheel's; the DOM's script can't
/// scroll yet) is computed again at the next frame: the target below the
/// viewport comes in.
#[test]
fn a_scroll_brings_an_entry() {
    let url = page(
        "scroll.html",
        &format!(
            r#"<!DOCTYPE html>{HELPERS}
<style>html, body {{ margin: 0 }} body {{ height: 3000px }}</style>
<div id="t" style="position: absolute; top: 500px; width: 50px; height: 50px"></div>
<script>
new IntersectionObserver(entries => {{
  log.push(entries.map(entry).join());
  if (entries[0].isIntersecting) done();
}}).observe(el('t'));
</script>"#
        ),
    );
    let wheel = InputEvent::MouseWheel {
        x: 10.0,
        y: 10.0,
        delta: ScrollDelta::Pixels { x: 0.0, y: 300.0 },
        buttons: 0,
        modifiers: Modifiers::NONE,
    };
    let report = headless::run_testharness_after_input(
        &url,
        gfx::Size::new(200, 300),
        &[wheel],
        Duration::from_secs(30),
    )
    .expect("the page reports");
    assert_eq!(
        report,
        "t 0,500,50,50 0,0,0,0 false 0 / t 0,200,50,50 0,200,50,50 true 1"
    );
}
