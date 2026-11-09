//! Script in a whole browser: a `javascript:` link runs in its document, a
//! script that fails to load is reported with `DidFinishLoad`, `load`
//! listeners run before it, and a click is an event script handles.
//! (What scripts do to the page is in the reftests, `tests/reftests/script`.)

use std::path::PathBuf;

use gfx::Size;
use protocol::input::{InputEvent, Modifiers, MouseButton};
use shell::headless::{self, Frame};

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-script");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// The pixel at `(x, y)`, as RGBA.
fn pixel(frame: &Frame, x: usize, y: usize) -> [u8; 4] {
    let i = (y * frame.size.width as usize + x) * 4;
    frame.pixels[i..i + 4].try_into().unwrap()
}

const GREEN: [u8; 4] = [0, 128, 0, 255];

#[test]
fn a_javascript_link_runs_in_its_document() {
    let url = page(
        "javascript-link.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } a { display: block; width: 100px; height: 100px }</style>
<a id="box" style="background-color: red"
   href="javascript:document.getElementById('box').setAttribute('style', 'background-color:%20green')"></a>"#,
    );
    let button = MouseButton::Left;
    let (x, y) = (50.0, 50.0);
    let input = [
        InputEvent::mouse_down(x, y, button, Modifiers::NONE),
        InputEvent::mouse_up(x, y, button, Modifiers::NONE),
    ];
    let frame = headless::render_after_input(&url, Size::new(200, 200), 1.0, true, &input)
        .expect("the page renders");
    assert_eq!(pixel(&frame, 50, 50), GREEN, "the link's script didn't run");
    assert_eq!(
        frame.url.as_deref(),
        Some(url.as_str()),
        "a javascript: link doesn't navigate"
    );
}

/// `load` fires before `DidFinishLoad`, so the frame it reports shows what
/// the listener did.
#[test]
fn the_load_event_comes_before_did_finish_load() {
    let url = page(
        "onload.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 100px; height: 100px; background-color: red }</style>
<body onload="document.querySelector('div').setAttribute('style', 'background-color: green')">
<div></div>"#,
    );
    let frame = headless::render(&url, Size::new(200, 200), 1.0, true).expect("the page renders");
    assert_eq!(pixel(&frame, 50, 50), GREEN, "the load listener didn't run");
}

/// A click goes to the element under the pointer, and its handler sees a
/// trusted `MouseEvent` with the pointer's place.
#[test]
fn a_click_is_an_event() {
    let url = page(
        "onclick.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 100px; height: 100px; background-color: red }</style>
<div onclick="if (event.isTrusted && event instanceof MouseEvent && event.clientX === 50 && event.clientY === 60)
                this.setAttribute('style', 'background-color: green')"></div>"#,
    );
    let button = MouseButton::Left;
    let (x, y) = (50.0, 60.0);
    let input = [
        InputEvent::mouse_down(x, y, button, Modifiers::NONE),
        InputEvent::mouse_up(x, y, button, Modifiers::NONE),
    ];
    let frame = headless::render_after_input(&url, Size::new(200, 200), 1.0, true, &input)
        .expect("the page renders");
    assert_eq!(pixel(&frame, 50, 50), GREEN, "the click handler didn't run");
}

/// A node moved by the first click to a document without a window gets no
/// second click, which comes before the frame without the `<div>`: input
/// is hit tested against the lifecycle brought up to date first,
/// so the click goes to the root element, not to the
/// moved `<div>` (whose listener would run, outside this document).
#[test]
fn a_click_skips_a_node_moved_to_another_document() {
    let url = page(
        "moved-click.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } html { background-color: red } div { width: 100px; height: 100px }</style>
<div></div>
<script>
const div = document.querySelector('div');
const inert = document.implementation.createHTMLDocument('');
let onDiv = 0;
div.addEventListener('click', () => { onDiv++; inert.body.appendChild(div); });
document.addEventListener('click', event => {
  if (onDiv === 1 && event.target === document.documentElement)
    document.documentElement.setAttribute('style', 'background-color: green');
});
</script>"#,
    );
    let button = MouseButton::Left;
    let (x, y) = (50.0, 60.0);
    let click = [
        InputEvent::mouse_down(x, y, button, Modifiers::NONE),
        InputEvent::mouse_up(x, y, button, Modifiers::NONE),
    ];
    let frame = headless::render_after_input(
        &url,
        Size::new(200, 200),
        1.0,
        true,
        &[click.clone(), click].concat(),
    )
    .expect("the page renders");
    assert_eq!(pixel(&frame, 50, 50), GREEN, "the second click went astray");
}

/// The navigation sets the document's URL, content type and encoding
/// from the response.
#[test]
fn a_document_has_its_url_type_and_encoding() {
    // Written twice: the page checks its own URL.
    let url = page("document-info.html", "");
    let expected = format!("{url},text/html,windows-1252");
    page(
        "document-info.html",
        &format!(
            r#"<!DOCTYPE html><meta charset="windows-1252">
<style>html, body {{ margin: 0 }} div {{ width: 100px; height: 100px; background-color: red }}</style>
<div></div>
<script>
if ([document.URL, document.contentType, document.characterSet].join() === {expected:?})
  document.querySelector('div').setAttribute('style', 'background-color: green');
</script>"#
        ),
    );
    let frame = headless::render(&url, Size::new(200, 200), 1.0, true).expect("the page renders");
    assert_eq!(pixel(&frame, 50, 50), GREEN, "not {expected}");
}

#[test]
fn a_script_that_fails_to_load_is_a_failed_subresource() {
    let url = page(
        "missing-script.html",
        r#"<!DOCTYPE html><script src="does-not-exist.js"></script><p>after"#,
    );
    let frame = headless::render(&url, Size::new(200, 200), 1.0, true).expect("the page renders");
    assert_eq!(
        frame.failed_subresources.len(),
        1,
        "{:?}",
        frame.failed_subresources
    );
    assert!(
        frame.failed_subresources[0].contains("does-not-exist.js"),
        "{:?}",
        frame.failed_subresources
    );
}

/// A test that never removes `reftest-wait`, animating all the while, ends
/// in an error after a bounded number of frames instead of running forever.
#[test]
fn an_endless_reftest_wait_is_an_error() {
    let url = page(
        "endless-reftest-wait.html",
        r#"<!DOCTYPE html><html class="reftest-wait"><script>
            function step() { requestAnimationFrame(step); }
            requestAnimationFrame(step);
        </script>"#,
    );
    let Err(err) = headless::render(&url, Size::new(100, 100), 1.0, true) else {
        panic!("an endless reftest-wait rendered");
    };
    assert!(err.contains("reftest-wait"), "{err}");
}

/// `performance.now()` is the frame's live clock: it moves while script
/// runs (the timers' time only moves between tasks). A frozen clock would
/// leave the box red after the bounded loop gives up.
#[test]
fn performance_now_moves_while_script_runs() {
    let url = page(
        "performance-now.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 100px; height: 100px; background-color: red }</style>
<div></div>
<script>
  const start = performance.now();
  let now = start;
  for (let i = 0; i < 1e9 && now - start < 20; i++) now = performance.now();
  if (now - start >= 20) document.querySelector('div').setAttribute('style', 'background-color: green');
</script>"#,
    );
    let frame = headless::render(&url, Size::new(200, 200), 1.0, true).expect("the page renders");
    assert_eq!(
        pixel(&frame, 50, 50),
        GREEN,
        "performance.now() didn't move"
    );
}

/// `headless::run_testharness`: what the page reports through `testRunner`
/// (headless runs web tests), as it is; an error for a page that never
/// reports, once the deadline passes.
#[test]
fn testharness_results_come_back_or_time_out() {
    let reports = page(
        "testharness-reports.html",
        "<script>setTimeout(() => testRunner.reportHarnessResults('the results'), 0);</script>",
    );
    assert_eq!(
        headless::run_testharness(
            &reports,
            std::time::Duration::from_secs(30),
            Default::default()
        )
        .as_deref(),
        Ok("the results")
    );
    let silent = page("testharness-silent.html", "<p>no harness");
    let started = std::time::Instant::now();
    let error = headless::run_testharness(
        &silent,
        std::time::Duration::from_secs(2),
        Default::default(),
    )
    .expect_err("no results");
    assert!(error.contains("deadline"), "{error}");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "waited past the deadline"
    );
}

/// A timer's task ends before its microtasks run, so a timer set in one of
/// them isn't nested in it (HTML's timer initialization steps; WPT's
/// `timer-nesting-not-inherited-in-microtask.html`, which measures the
/// delay). Here by order, not time: in the sixth timer of a chain
/// (nesting level 5) a direct `setTimeout(.., 0)`, nested past five
/// levels, is clamped to 4 ms, and one set right after it in a microtask
/// for 1 ms comes first unless it is clamped too.
#[test]
fn a_timer_set_in_a_microtask_is_not_nested() {
    let url = page(
        "timer-nesting-microtask.html",
        r#"<script>
const log = [];
function nest(level) {
  if (level < 6) return setTimeout(() => nest(level + 1), 0);
  setTimeout(() => log.push("nested"), 0);
  queueMicrotask(() => setTimeout(() => log.push("microtask"), 1));
  setTimeout(() => testRunner.reportHarnessResults(log.join()), 50);
}
setTimeout(() => nest(1), 0);
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("microtask,nested")
    );
}

/// A timer counts from when it is set, not from the start of the task
/// that sets it (Blink's `DOMTimer` reads the clock then): one set for
/// 30 ms after 50 ms of work runs 30 ms later (31 in Chrome 152). A task
/// the DOM queues counts on the same clock: a `postMessage` after a
/// `setTimeout(.., 0)` runs after it, as in Chrome.
#[test]
fn a_timer_counts_from_when_it_is_set() {
    let url = page(
        "timer-counts-from-set.html",
        r#"<script>
setTimeout(() => {
  const end = performance.now() + 50;
  while (performance.now() < end);
  const set = performance.now();
  const order = [];
  const { port1, port2 } = new MessageChannel();
  port1.onmessage = () => order.push("message");
  setTimeout(() => order.push("timer"), 0);
  port2.postMessage(0);
  setTimeout(() => testRunner.reportHarnessResults(
    `${performance.now() - set >= 30} ${order.join()}`), 30);
}, 0);
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("true timer,message")
    );
}

/// What a timer runs, as Chrome 152 runs the same page: a function with
/// the window as `this` and the arguments after the timeout; a string (or
/// what converts to one) as a classic script without a URL, its errors
/// the document's, its `import()` working, run again by an interval; an
/// exception in another frame's function reported in that frame.
#[test]
fn timer_handlers_run_as_chromes() {
    let url = page(
        "timer-handlers.html",
        r#"<iframe srcdoc="<script>function thrower() { throw new Error('in the child') }</script>"></iframe>
<script>
// What finishes on its own time is sorted.
const log = [], later = [];
onerror = (m, f, l, c) => { log.push(['error', m, f === location.href, l, c].join(' ')); return true; };
setTimeout(function () { 'use strict'; log.push(['function', this === window, arguments.length, [...arguments].join()].join(' ')); }, 0, 1, undefined, 'b');
setTimeout('log.push(["string", this === window, typeof arguments].join(" "))', 0, 1);
setTimeout({ toString() { return 'log.push("toString")'; } });
setTimeout('\n  throw new Error("thrown")');
setTimeout('}');
setTimeout('log.push("currentScript " + document.currentScript)');
setTimeout('log.push(new Error().stack.split("\\n")[1].trim())');
setTimeout('import("data:text/javascript,export default 1").then(m => later.push("import " + m.default))');
var n = 0;
var id = setInterval('if (++n == 3) { clearInterval(id); later.push("interval " + n); }', 1);
onload = () => {
  frames[0].onerror = (m) => { later.push('child error ' + m); return true; };
  setTimeout(frames[0].thrower);
  setTimeout(() => testRunner.reportHarnessResults(log.concat(later.sort()).join('\n')), 100);
};
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("function true 3 1,,b
string true undefined
toString
error Uncaught Error: thrown true 2 3
error Uncaught SyntaxError: Unexpected token '}' true 1 1
currentScript null
at <anonymous>:1:10
child error Uncaught Error: in the child
import 1
interval 3")
    );
}

/// An idle callback's `IdleDeadline`, as Chrome 152 gives it: made in the
/// callback's realm (another frame's function gets that frame's), and
/// counting down on the window's clock, in a frame made well after its
/// parent (so its time origin is not the renderer's start).
#[test]
fn an_idle_deadline_is_the_callbacks() {
    let url = page(
        "idle-deadline-realm.html",
        r#"<script>
window.log = [];
onload = () => setTimeout(() => {
  const f = document.createElement('iframe');
  f.srcdoc = '<script>function own(d) { parent.log.push(["child", d instanceof IdleDeadline, d instanceof parent.IdleDeadline, d.timeRemaining() > 0 && d.timeRemaining() <= 50].join(" ")); }<\/script>';
  f.onload = () => {
    const w = f.contentWindow;
    // The parent's function, given to the child's window.
    w.requestIdleCallback(function (d) {
      log.push(['parent', d instanceof IdleDeadline, d instanceof w.IdleDeadline, d.timeRemaining() > 0 && d.timeRemaining() <= 50].join(' '));
    });
    // The child's own function.
    w.requestIdleCallback(w.own);
    setTimeout(() => testRunner.reportHarnessResults(log.sort().join('\n')), 500);
  };
  document.body.append(f);
}, 300);
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("child true false true\nparent true false true")
    );
}

/// An idle period that started with nothing to draw ends at the next frame
/// once an animation frame callback waits: a callback queued before it
/// started, run after the first one asked for a frame, has at most a
/// frame's time (WPT's `deadline-max-rAF`).
#[test]
fn an_idle_period_ends_at_the_next_frame_once_frames_are_drawn() {
    let url = page(
        "idle-period-frame.html",
        r#"<script>
onload = () => setTimeout(() => {
  const log = [];
  requestIdleCallback(d => {
    log.push(d.timeRemaining() > 17);
    const loop = () => requestAnimationFrame(loop);
    loop();
  });
  requestIdleCallback(d => {
    log.push(d.timeRemaining() <= 1000 / 60);
    testRunner.reportHarnessResults(log.join(' '));
  });
}, 100);
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("true true")
    );
}

/// The documents run their animation frame callbacks in tree order, each
/// taking its own when its turn comes: the parent's before the child's,
/// though the child asked first; one a parent's callback requests in its
/// child frame runs in the same frame, before a timer the parent's
/// callback set, while one it requests of its own document waits for the
/// next frame (HTML's "update the rendering").
#[test]
fn a_child_frames_callback_runs_in_the_frame_its_parent_asked_in() {
    let url = page(
        "animation-frames-in-tree-order.html",
        r#"<iframe srcdoc="child"></iframe>
<script>
const log = [];
onload = () => {
  frames[0].requestAnimationFrame(() => log.push("child first"));
  requestAnimationFrame(parent);
};
const parent = () => {
  log.push("parent");
  frames[0].requestAnimationFrame(() => log.push("child"));
  requestAnimationFrame(() => {
    log.push("parent again");
    testRunner.reportHarnessResults(log.join());
  });
  setTimeout(() => log.push("timeout"), 0);
};
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("parent,child first,child,timeout,parent again")
    );
}

/// A page that stops animating with `reftest-wait` still on (here, its
/// `requestAnimationFrame` callback throws, as WPT's do on what machopo lacks)
/// asks for no more frames, so the frames' limit never counts down: the
/// time limit ends the wait, and says why.
#[test]
fn reftest_wait_that_never_goes_runs_out_of_time() {
    let url = page(
        "reftest-wait-stuck.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<script>
requestAnimationFrame(() => { notDefined.scrollTop; });
</script>"#,
    );
    let started = std::time::Instant::now();
    let error = headless::render_with_reftest_wait_timeout(
        &url,
        Size::new(200, 200),
        std::time::Duration::from_secs(1),
    )
    .err()
    .expect("reftest-wait never goes");
    assert_eq!(error, "reftest-wait was still on after 1 s");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "waited past the time limit"
    );
}

/// Child frames take their turns in the tree order of their iframes, not
/// in the order they were made (WPT's `child-document-raf-order`).
#[test]
fn child_frames_go_in_the_tree_order_of_their_iframes() {
    let url = page(
        "animation-frames-iframe-order.html",
        r#"<script>
const log = [];
const make = name => {
  const iframe = document.createElement("iframe");
  iframe.srcdoc = name;
  return iframe;
};
const second = make("second");
document.documentElement.prepend(second);
const first = make("first");
document.documentElement.prepend(first);
let waiting = 2;
for (const iframe of [first, second])
  iframe.onload = () => {
    if (--waiting) return;
    second.contentWindow.requestAnimationFrame(() => log.push("second"));
    first.contentWindow.requestAnimationFrame(() => log.push("first"));
    requestAnimationFrame(() => log.push("parent"));
    setTimeout(() => setTimeout(() => testRunner.reportHarnessResults(log.join()), 100), 0);
  };
</script>"#,
    );
    assert_eq!(
        headless::run_testharness(&url, std::time::Duration::from_secs(30), Default::default())
            .as_deref(),
        Ok("parent,first,second")
    );
}

/// An animation frame callback of a document no frame shows any more (its
/// iframe removed, the document discarded) is dropped, not kept asking for
/// frames: with `reftest-wait` on, the time limit ends the wait.
#[test]
fn a_removed_frames_callbacks_ask_for_no_frames() {
    let url = page(
        "removed-frame-callback.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<iframe srcdoc="child"></iframe>
<script>
onload = () => {
  frames[0].requestAnimationFrame(() => {});
  document.querySelector("iframe").remove();
};
</script>"#,
    );
    let error = headless::render_with_reftest_wait_timeout(
        &url,
        Size::new(200, 200),
        std::time::Duration::from_secs(1),
    )
    .err()
    .expect("reftest-wait never goes");
    assert_eq!(error, "reftest-wait was still on after 1 s");
}

/// A page asking for no frames may still remove `reftest-wait` from a
/// timer: the wait goes on until the time limit, not just while frames do.
#[test]
fn reftest_wait_removed_by_a_timer_is_waited_for() {
    let url = page(
        "reftest-wait-timer.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<style>html, body { margin: 0 } div { width: 100px; height: 100px; background-color: red }</style>
<div></div>
<script>
setTimeout(() => {
  document.querySelector('div').setAttribute('style', 'background-color: green');
  document.documentElement.classList.remove('reftest-wait');
}, 300);
</script>"#,
    );
    let frame = headless::render_with_reftest_wait_timeout(
        &url,
        Size::new(200, 200),
        std::time::Duration::from_secs(5),
    )
    .expect("the timer removes reftest-wait");
    assert_eq!(
        pixel(&frame, 50, 50),
        GREEN,
        "captured before the timer ran"
    );
}
