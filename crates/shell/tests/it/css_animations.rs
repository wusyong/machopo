//! CSS animations in a whole browser: an element's `animation` runs from
//! the frame that shows it, the rendering update ticks it at each frame
//! (asking for the next while it runs) and fires its events at the
//! element (a pseudo-element's at its element). Each page reports what it
//! saw through `testRunner` (`headless::run_testharness`). Time is real
//! here, and a frame may come late (the tests run side by side): the
//! animations are long enough, and start far enough in, that one slow
//! frame doesn't skip what a test waits for.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-css-animations");
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

/// An animation moves with nothing but the frames it asks for: a while
/// after it started it is partway, and it ends.
#[test]
fn an_animation_runs_by_itself() {
    let url = page(
        "runs.html",
        r#"<!DOCTYPE html>
<style>
  @keyframes grow { from { width: 0px } to { width: 100px } }
  #t { animation: grow 10s linear }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
setTimeout(() => {
  const partway = parseFloat(getComputedStyle(t).width);
  testRunner.reportHarnessResults(`${partway > 0 && partway < 100}`);
}, 300);
</script>"#,
    );
    assert_eq!(report(&url), "true");
}

/// `animationstart`, `animationiteration` for each new iteration, then
/// `animationend`: `AnimationEvent`s that bubble, are trusted, with the
/// animation's name and the elapsed time of where each is (it starts
/// halfway into its first iteration).
#[test]
fn its_events() {
    let url = page(
        "events.html",
        r#"<!DOCTYPE html>
<style>
  @keyframes k { to { width: 100px } }
  #t { animation: k 1s linear 2 -0.5s }
</style>
<div id="t"></div>
<script>
const log = [];
for (const type of ['animationstart', 'animationiteration', 'animationend']) {
  document.body.addEventListener(type, e => {
    log.push(`${e.type.slice(9)} ${e.animationName} ${e.elapsedTime} ${e.target.id}`);
    if (e.type === 'animationend') {
      log.push(`${e instanceof AnimationEvent} ${e.isTrusted} ${e.bubbles} ${e.cancelable}`);
      testRunner.reportHarnessResults(log.join(' / '));
    }
  });
}
</script>"#,
    );
    assert_eq!(
        report(&url),
        "start k 0.5 t / iteration k 1 t / end k 2 t / true true true true"
    );
}

/// A paused animation started all the same (it is where it holds), and is
/// canceled when its name goes, or its element: `animationcancel` at the
/// next frame, not during the change.
#[test]
fn canceled_ones_say_so() {
    let url = page(
        "cancel.html",
        r#"<!DOCTYPE html>
<style>
  @keyframes k { to { width: 100px } }
  #a, #b { animation: k 100s paused }
</style>
<div id="a"></div>
<div id="b"></div>
<script>
const log = [];
const a = document.getElementById('a'), b = document.getElementById('b');
let started = 0;
document.body.addEventListener('animationstart', e => {
  log.push(`start ${e.target.id}`);
  if (++started < 2) return;
  a.style.animationName = 'none';
  getComputedStyle(a).width;
  b.remove();
  log.push('changed');
});
let canceled = 0;
const cancel = e => {
  log.push(`cancel ${e.target.id} ${e.elapsedTime}`);
  if (++canceled === 2) testRunner.reportHarnessResults(log.join(' / '));
};
a.addEventListener('animationcancel', cancel);
b.addEventListener('animationcancel', cancel);
</script>"#,
    );
    assert_eq!(
        report(&url),
        "start a / start b / changed / cancel a 0 / cancel b 0"
    );
}

/// A pseudo-element's events fire at its element, with its name.
#[test]
fn a_pseudo_elements_events() {
    let url = page(
        "pseudo.html",
        r#"<!DOCTYPE html>
<style>
  @keyframes k { to { width: 100px } }
  #t::after { content: ''; display: block; animation: k 50ms }
</style>
<div id="t"></div>
<script>
document.body.addEventListener('animationend', e => {
  testRunner.reportHarnessResults(`${e.target.id} ${e.pseudoElement}`);
});
</script>"#,
    );
    assert_eq!(report(&url), "t ::after");
}
