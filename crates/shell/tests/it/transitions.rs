//! CSS transitions in a whole browser: a style change starts one, script
//! reads its value through `getComputedStyle`, the rendering update ticks
//! it at each frame and fires its events at the element (a
//! pseudo-element's at its element). Each page reports what it saw
//! through `testRunner` (`headless::run_testharness`). Time is real here:
//! the values read are pinned with negative delays and long durations,
//! and the events are of short transitions.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-transitions");
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

/// The value halfway through, as `getComputedStyle` reads it right after
/// the change (the transition starts at the timeline's time), and later:
/// a long one has hardly moved after two frames, a short one has after
/// 200 ms.
#[test]
fn script_reads_the_value() {
    let url = page(
        "value.html",
        r#"<!DOCTYPE html>
<style>
  #t, #u { width: 0px; transition: width 100s linear -50s }
  #u { transition: width 1s linear -0.5s }
  .wide { width: 100px !important }
</style>
<div id="t"></div>
<div id="u"></div>
<script>
const t = document.getElementById('t'), u = document.getElementById('u');
getComputedStyle(t).width;
getComputedStyle(u).width;
t.className = u.className = 'wide';
const now = getComputedStyle(t).width;
requestAnimationFrame(() => requestAnimationFrame(() => {
  const later = parseFloat(getComputedStyle(t).width);
  setTimeout(() => requestAnimationFrame(() => {
    const moved = parseFloat(getComputedStyle(u).width);
    testRunner.reportHarnessResults(`${now} ${later >= 50 && later < 51} ${moved > 55}`);
  }), 200);
}));
</script>"#,
    );
    assert_eq!(report(&url), "50px true true");
}

/// The page's first transition, started a while after it loaded (as a
/// click on `about:demo`'s bar does: the class changes, and the next frame
/// styles it): it starts at the frame that shows it, not at the
/// timeline's time when the page had no animations (0, its start, which
/// ended it at once).
#[test]
fn a_first_transition_later_starts_then() {
    let url = page(
        "later.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 1s linear }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
getComputedStyle(t).width;
setTimeout(() => {
  t.className = 'wide';
  requestAnimationFrame(() => requestAnimationFrame(() => {
    const later = parseFloat(getComputedStyle(t).width);
    testRunner.reportHarnessResults(`${later > 0 && later < 50}`);
  }));
}, 500);
</script>"#,
    );
    assert_eq!(report(&url), "true");
}

/// The events of a frame go in the order they were scheduled, then by
/// the transitions' order (the style change makes them in tree order):
/// `transitionrun` at the frame where they are pending, then two that
/// start and end within the next frame, the shorter's end first.
#[test]
fn a_frames_events_go_by_when() {
    let url = page(
        "order.html",
        r#"<!DOCTYPE html>
<style>
  #a, #b { width: 0px; transition: width 50ms linear }
  #a { transition-duration: 100ms }
  #a.wide, #b.wide { width: 100px }
</style>
<div id="b"></div>
<div id="a"></div>
<script>
const log = [];
for (const type of ['transitionrun', 'transitionstart', 'transitionend']) {
  document.body.addEventListener(type, e => {
    log.push(`${e.type.slice(10)} ${e.target.id}`);
    if (log.length === 6) testRunner.reportHarnessResults(log.join(' / '));
  });
}
const a = document.getElementById('a'), b = document.getElementById('b');
getComputedStyle(a).width;
getComputedStyle(b).width;
a.className = 'wide';
b.className = 'wide';
// The first frame starts them; both are done by the next.
requestAnimationFrame(() => {
  const start = performance.now();
  while (performance.now() - start < 150) {}
});
</script>"#,
    );
    assert_eq!(
        report(&url),
        "run b / run a / start b / start a / end b / end a"
    );
}

/// A frame where only a transition moves is rendered: what the rendering
/// update does (an `IntersectionObserver`'s) sees the box come in partway,
/// before the transition ends. It comes in a fifteenth of the way: frames
/// slowed down by the tests running side by side still see it before the
/// end.
#[test]
fn a_frame_shows_the_values() {
    let url = page(
        "rendered.html",
        r#"<!DOCTYPE html>
<style>
  #root { width: 280px; height: 20px; overflow: hidden }
  #t { width: 20px; height: 20px; margin-left: 300px; transition: margin-left 600ms linear }
  #t.in { margin-left: 0px }
</style>
<div id="root"><div id="t"></div></div>
<script>
const t = document.getElementById('t');
const log = [];
new IntersectionObserver(entries => {
  for (const e of entries) {
    if (e.isIntersecting) log.push('in');
  }
}, { root: document.getElementById('root') }).observe(t);
t.addEventListener('transitionend', () => {
  log.push('end');
  testRunner.reportHarnessResults(log.join(' '));
});
requestAnimationFrame(() => requestAnimationFrame(() => {
  getComputedStyle(t).marginLeft;
  t.className = 'in';
}));
</script>"#,
    );
    assert_eq!(report(&url), "in end");
}

/// Its events, in order, at the element and bubbling up: run and start at
/// the first frame, end when it is done; the elapsed time is from the
/// start of the active interval (a negative delay's part already gone).
#[test]
fn events_in_order() {
    let url = page(
        "events.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 100ms linear -40ms }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
for (const type of ['transitionrun', 'transitionstart', 'transitionend', 'transitioncancel']) {
  document.body.addEventListener(type, e => {
    log.push(`${e.type} ${e.propertyName} ${e.elapsedTime.toFixed(2)} ${e.target.id} ` +
             `${e.pseudoElement === ''} ${e instanceof TransitionEvent} ${e.isTrusted}`);
    if (e.type === 'transitionend') testRunner.reportHarnessResults(log.join(' / '));
  });
}
getComputedStyle(t).width;
t.className = 'wide';
</script>"#,
    );
    assert_eq!(
        report(&url),
        "transitionrun width 0.04 t true true true / \
         transitionstart width 0.04 t true true true / \
         transitionend width 0.10 t true true true"
    );
}

/// Hidden partway through: `transitioncancel`, and nothing after.
#[test]
fn canceled() {
    let url = page(
        "cancel.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 100s linear }
  #t.wide { width: 100px }
  #t.gone { display: none }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
t.addEventListener('transitionstart', () => {
  t.className = 'wide gone';
});
t.addEventListener('transitioncancel', e => {
  log.push(e.type);
  requestAnimationFrame(() => requestAnimationFrame(() => {
    testRunner.reportHarnessResults(`${log.join(' ')} ${getComputedStyle(t).width}`);
  }));
});
t.addEventListener('transitionend', e => log.push(e.type));
getComputedStyle(t).width;
t.className = 'wide';
</script>"#,
    );
    assert_eq!(report(&url), "transitioncancel 100px");
}

/// Removed partway through: canceled, at the element out of the document,
/// and nothing after (css-transitions 1). It starts halfway and has a
/// second to go: a slow frame (the tests run side by side) doesn't take it
/// past its end before it is removed, and the report waits past that end,
/// where a `transitionend` would show had it not been canceled.
#[test]
fn a_removed_element_cancels() {
    let url = page(
        "removed.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 2s linear -1s }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
for (const type of ['transitionstart', 'transitionend', 'transitioncancel']) {
  t.addEventListener(type, e => log.push(e.type));
}
t.addEventListener('transitionstart', () => {
  t.remove();
  setTimeout(() => requestAnimationFrame(() => requestAnimationFrame(() => {
    testRunner.reportHarnessResults(log.join(' '));
  })), 1200);
});
getComputedStyle(t).width;
t.className = 'wide';
</script>"#,
    );
    assert_eq!(report(&url), "transitionstart transitioncancel");
}

/// Moved to another parent partway through: canceled, the new style at
/// once; a later change transitions again.
#[test]
fn a_moved_element_cancels() {
    let url = page(
        "moved.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 100s linear -50s }
  #t.wide { width: 100px }
  #t.narrow { width: 20px }
</style>
<div id="p"></div>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
t.addEventListener('transitionrun', () => {
  log.push('run');
  if (log.length > 1) return;
  document.getElementById('p').append(t);
});
t.addEventListener('transitioncancel', () => {
  log.push(`cancel ${getComputedStyle(t).width}`);
  t.className = 'narrow';
  // From 100px to 20px, halfway.
  log.push(getComputedStyle(t).width);
  testRunner.reportHarnessResults(log.join(' / '));
});
getComputedStyle(t).width;
t.className = 'wide';
</script>"#,
    );
    assert_eq!(report(&url), "run / cancel 100px / 60px");
}

/// Done, the element's style is its end value at once (the last frame's
/// was short of it).
#[test]
fn done_is_at_the_end() {
    let url = page(
        "ended.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 200ms linear }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
// Read at every frame: the style is the frame's value.
function frame() {
  getComputedStyle(t).width;
  requestAnimationFrame(frame);
}
t.addEventListener('transitionend', () => {
  testRunner.reportHarnessResults(getComputedStyle(t).width);
});
getComputedStyle(t).width;
t.className = 'wide';
requestAnimationFrame(frame);
</script>"#,
    );
    assert_eq!(report(&url), "100px");
}

/// The style a new transition starts from is the old one's value now, not
/// the last frame's: one that just ended is at its end.
#[test]
fn a_new_transition_starts_from_the_end_of_one_done() {
    let url = page(
        "after-end.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 30ms linear }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
t.addEventListener('transitionend', () => {
  t.style.transition = 'width 100s linear';
  t.className = '';
  testRunner.reportHarnessResults(getComputedStyle(t).width);
}, { once: true });
getComputedStyle(t).width;
t.className = 'wide';
</script>"#,
    );
    assert_eq!(report(&url), "100px");
}

/// A pseudo-element's transition fires at its element, saying which.
#[test]
fn a_pseudo_elements_events() {
    let url = page(
        "pseudo.html",
        r#"<!DOCTYPE html>
<style>
  #t::before { content: 'x'; opacity: 1; transition: opacity 50ms }
  #t.faded::before { opacity: 0 }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
t.addEventListener('transitionend', e => {
  testRunner.reportHarnessResults(`${e.target.id} ${e.pseudoElement} ${e.propertyName}`);
});
getComputedStyle(t, '::before').opacity;
t.className = 'faded';
</script>"#,
    );
    assert_eq!(report(&url), "t ::before opacity");
}
