//! Web Animations' script API in a whole browser: a CSS transition's or
//! animation's object, its promises settled and its `finish` and `cancel`
//! events fired by the rendering update, with the CSS events at the
//! element, frames for an animation that left its element as script
//! plays it, and script's own (`animate()`, `new Animation()`, and the
//! replaced ones removed). Each page reports what it saw through `testRunner`
//! (`headless::run_testharness`). Time is real here: the animations are
//! short, or pinned with seeks.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-web-animations");
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

/// A transition's `ready` resolves at the frame that starts it, at its
/// start time; at its end, `finished` resolves (web-animations 1's update
/// of animations: the microtask checkpoint after the timelines), then the
/// events go: `transitionend` at the element, `finish` at the animation.
#[test]
fn ready_then_the_end() {
    let url = page(
        "end.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 300ms linear }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
getComputedStyle(t).width;
t.className = 'wide';
const a = t.getAnimations()[0];
log.push(a.pending);
a.ready.then(() => log.push('ready', !a.pending, a.startTime <= document.timeline.currentTime));
t.addEventListener('transitionend', () => log.push('transitionend'));
a.finished.then(() => log.push('finished'));
a.onfinish = e => {
  log.push('finish', e.currentTime);
  requestAnimationFrame(() => testRunner.reportHarnessResults(log.join(' ')));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "true ready true true finished transitionend finish 300"
    );
}

/// `cancel()`: `finished` rejects at once; the next update fires
/// `transitioncancel` at the element and `cancel` at the animation; the
/// value is the base one again.
#[test]
fn canceled_by_script() {
    let url = page(
        "cancel.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 100s linear -50s }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
getComputedStyle(t).width;
t.className = 'wide';
const a = t.getAnimations()[0];
log.push(getComputedStyle(t).width);
a.finished.catch(e => log.push(e.name));
t.addEventListener('transitioncancel', e => log.push('transitioncancel', e.elapsedTime));
a.oncancel = e => {
  log.push('cancel', e.currentTime);
  requestAnimationFrame(() => testRunner.reportHarnessResults(log.join(' ')));
};
requestAnimationFrame(() => {
  a.cancel();
  log.push(getComputedStyle(t).width, a.playState);
});
</script>"#,
    );
    let got = report(&url);
    let (start, rest) = got
        .split_once(" transitioncancel ")
        .expect("transitioncancel");
    assert_eq!(start, "50px 100px idle AbortError");
    let (elapsed, cancel) = rest.split_once(' ').unwrap();
    // Canceled after about 50 s of its active time.
    assert!(
        (50.0..51.0).contains(&elapsed.parse::<f64>().unwrap()),
        "{got}"
    );
    // `null`, joined.
    assert_eq!(cancel, "cancel ");
}

/// A transition canceled, then played by script: it left its element,
/// but frames keep coming while it runs, and it finishes.
#[test]
fn a_canceled_transition_played_again_finishes() {
    let url = page(
        "replay.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 200ms linear }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
getComputedStyle(t).width;
t.className = 'wide';
const a = t.getAnimations()[0];
requestAnimationFrame(() => requestAnimationFrame(() => {
  a.cancel();
  requestAnimationFrame(() => {
    a.play();
    a.finished.then(() => testRunner.reportHarnessResults(
      `${a.playState} ${a.currentTime} ${t.getAnimations().length}`));
  });
}));
</script>"#,
    );
    assert_eq!(report(&url), "finished 200 0");
}

/// Events scheduled for the same time go by their targets' places: an
/// element, its `::before`, its `::after`, then its child.
#[test]
fn events_of_one_time_go_in_tree_order() {
    let url = page(
        "order.html",
        r#"<!DOCTYPE html>
<style>
  @keyframes grow { to { width: 10px } }
  #p, #c, #p::before, #p::after { animation: grow 100s }
  #p::before, #p::after { content: '' }
</style>
<div id="p"><div id="c"></div></div>
<script>
const p = document.getElementById('p');
const seen = [];
p.addEventListener('animationstart', e => {
  seen.push(`${e.target.id}${e.pseudoElement}`);
  if (seen.length === 4) testRunner.reportHarnessResults(seen.join(' '));
});
</script>"#,
    );
    assert_eq!(report(&url), "p p::before p::after c");
}

/// A transition of an element no longer rendered, played by script: it
/// is no element's any more, but frames keep coming while it runs, and it
/// finishes.
#[test]
fn one_of_an_element_not_rendered_finishes() {
    let url = page(
        "hidden.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; transition: width 200ms linear }
  #t.wide { width: 100px }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
getComputedStyle(t).width;
t.className = 'wide';
const a = t.getAnimations()[0];
requestAnimationFrame(() => requestAnimationFrame(() => {
  t.style.display = 'none';
  getComputedStyle(t).width;
  requestAnimationFrame(() => {
    a.play();
    a.finished.then(() => testRunner.reportHarnessResults(`${a.playState} ${a.currentTime}`));
  });
}));
</script>"#,
    );
    assert_eq!(report(&url), "finished 200");
}

/// A CSS animation that script paused and sought holds that value, which
/// the frames show, whatever `animation-play-state` says after.
#[test]
fn a_paused_css_animation_holds_its_value() {
    let url = page(
        "paused.html",
        r#"<!DOCTYPE html>
<style>
  #t { width: 0px; animation: grow 10s linear }
  @keyframes grow { to { width: 100px } }
</style>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const a = t.getAnimations()[0];
a.pause();
a.currentTime = 2500;
t.style.animationPlayState = 'running';
a.ready.then(() => requestAnimationFrame(() => requestAnimationFrame(() => {
  testRunner.reportHarnessResults(
    `${a.animationName} ${a.playState} ${a.currentTime} ${getComputedStyle(t).width}`);
})));
</script>"#,
    );
    assert_eq!(report(&url), "grow paused 2500 25px");
}

/// `animate()`: script's animation starts at a frame (`ready`), its value
/// follows the frames' time, and at its end `finished` resolves and
/// `finish` fires; without a fill, the base value is back.
#[test]
fn animate_runs_to_its_end() {
    let url = page(
        "animate.html",
        r#"<!DOCTYPE html>
<div id="t" style="width: 0px"></div>
<script>
const t = document.getElementById('t');
const log = [];
const a = t.animate({ width: ['100px', '200px'] }, { duration: 300, id: 'grow' });
log.push(a.id, a.playState, a.pending);
a.ready.then(() => {
  const width = parseFloat(getComputedStyle(t).width);
  log.push('ready', width >= 100 && width < 200);
});
a.finished.then(() => log.push('finished'));
a.onfinish = e => {
  log.push('finish', e.currentTime, getComputedStyle(t).width, t.getAnimations().length);
  testRunner.reportHarnessResults(log.join(' '));
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "grow running true ready true finished finish 300 0px 0"
    );
}

/// `new Animation(new KeyframeEffect(...))` with `fill: forwards`: played,
/// it holds its last keyframe after its end; its effect moved to another
/// element takes the value with it.
#[test]
fn a_constructed_animation_fills_forwards() {
    let url = page(
        "constructed.html",
        r#"<!DOCTYPE html>
<div id="t" style="width: 0px"></div>
<div id="u" style="width: 0px"></div>
<script>
const t = document.getElementById('t');
const u = document.getElementById('u');
const effect = new KeyframeEffect(t, [{ width: '10px' }, { width: '50px' }],
  { duration: 100, fill: 'forwards' });
const a = new Animation(effect, document.timeline);
a.play();
a.finished.then(() => requestAnimationFrame(() => {
  const before = getComputedStyle(t).width;
  effect.target = u;
  testRunner.reportHarnessResults(
    `${a.playState} ${before} ${getComputedStyle(t).width} ${getComputedStyle(u).width} ` +
    `${u.getAnimations()[0] === a}`);
}));
</script>"#,
    );
    assert_eq!(report(&url), "finished 50px 0px 50px true");
}

/// Replaced animations, with real frames: the one another covers is
/// removed at the update where both have finished (the `finish` events
/// first, then its `remove`), its value gone from the next frame; then
/// `commitStyles()` keeps its value.
#[test]
fn a_replaced_animation_is_removed() {
    let url = page(
        "replaced.html",
        r#"<!DOCTYPE html>
<div id="t"></div>
<script>
const t = document.getElementById('t');
const log = [];
const f = { duration: 50, fill: 'forwards' };
const a = t.animate({ opacity: [0, 0.2] }, f);
const b = t.animate({ opacity: [0, 0.4] }, f);
a.onfinish = () => log.push('finish a');
b.onfinish = () => log.push('finish b');
a.onremove = e => {
  log.push('remove a', a.replaceState, e.currentTime);
  requestAnimationFrame(() => {
    log.push('frame', getComputedStyle(t).opacity);
    a.commitStyles();
    b.cancel();
    log.push(t.style.opacity, getComputedStyle(t).opacity);
    testRunner.reportHarnessResults(log.join(' '));
  });
};
</script>"#,
    );
    assert_eq!(
        report(&url),
        "finish a finish b remove a removed 50 frame 0.4 0.2 0.2"
    );
}
