//! `ResizeObserver` in a whole browser: the rendering update gathers and
//! broadcasts after the animation frame callbacks. Each page reports what
//! its callbacks saw through `testRunner` (`headless::run_testharness`):
//! the first notification of each kind of target and its sizes, what a
//! change notifies (and what doesn't), the order within a frame, the depth
//! loop and its error and the flat tree's depths, a child frame's
//! observers, a callback's exception, and entries read as their observer
//! is called.

use std::path::PathBuf;
use std::time::Duration;

use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-resize-observers");
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

/// The first notification: every target once, whatever its size (a box,
/// `display: none`, an inline element, 0×0, an SVG element's bounding box),
/// in the order observed, with the observer as `this` and the second
/// argument; the sizes of each box, the content rect at the padding, each
/// size list the same objects every time.
#[test]
fn the_first_notification() {
    let url = page(
        "first.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 }</style>
<div id="box" style="width: 100px; height: 50px; padding: 1px 2px 3px 4px;
                     border: solid; border-width: 5px 6px 7px 8px"></div>
<div id="none" style="display: none"></div>
<span id="inline">text</span>
<div id="empty" style="width: 0; height: 0"></div>
<svg width="100" height="100"><rect id="rect" x="5" y="6" width="30" height="40"/></svg>
<script>
const size = s => `${s.inlineSize}x${s.blockSize}`;
const rect = r => `${r.x},${r.y},${r.width},${r.height}`;
const ids = ['box', 'none', 'inline', 'empty', 'rect'];
const log = [];
const observer = new ResizeObserver(function (entries, second) {
  log.push(`this ${this === observer} ${second === observer}`);
  for (const e of entries) {
    log.push(`${e.target.id} ${rect(e.contentRect)} ` +
             `content ${size(e.contentBoxSize[0])} border ${size(e.borderBoxSize[0])} ` +
             `device ${size(e.devicePixelContentBoxSize[0])} ` +
             `same ${e.contentBoxSize === e.contentBoxSize && e.contentRect === e.contentRect} ` +
             `frozen ${Object.isFrozen(e.borderBoxSize)}`);
  }
});
for (const id of ids) observer.observe(document.getElementById(id));
requestAnimationFrame(() => requestAnimationFrame(() => {
  testRunner.reportHarnessResults(log.join(' / '));
}));
</script>"#,
    );
    assert_eq!(
        report(&url),
        "this true true / \
         box 4,1,100,50 content 100x50 border 120x66 device 100x50 same true frozen true / \
         none 0,0,0,0 content 0x0 border 0x0 device 0x0 same true frozen true / \
         inline 0,0,0,0 content 0x0 border 0x0 device 0x0 same true frozen true / \
         empty 0,0,0,0 content 0x0 border 0x0 device 0x0 same true frozen true / \
         rect 0,0,30,40 content 30x40 border 30x40 device 30x40 same true frozen true"
    );
}

/// After the first: only an observation whose observed box changed size
/// is notified, at the next frame, not while script runs. A transform, a
/// move or the other box's change don't count; observing again with
/// another box reports anew, with the same box does nothing (as in Chrome:
/// `b` reports only its own change); an unobserved or disconnected one is
/// never notified again.
#[test]
fn what_a_change_notifies() {
    let url = page(
        "change.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } div { width: 10px; height: 10px }</style>
<div id="a"></div><div id="b"></div><div id="c"></div><div id="d"></div>
<script>
const log = [];
const el = id => document.getElementById(id);
const observer = new ResizeObserver(entries => {
  log.push(entries.map(e => `${e.target.id}:${e.contentRect.width}`).join(','));
});
observer.observe(el('a'));
observer.observe(el('b'), {box: 'border-box'});
observer.observe(el('c'));
const steps = [
  () => {
    el('a').style.width = '20px';
    el('a').style.transform = 'scale(2)';
    el('b').style.padding = '1px';
    el('c').style.marginLeft = '5px';
    log.push('changed');
  },
  () => {
    el('b').style.padding = '0';
    el('b').style.borderLeft = '1px solid';
    observer.observe(el('b'), {box: 'border-box'});
    observer.observe(el('c'), {box: 'border-box'});
    observer.unobserve(el('a'));
    el('a').style.width = '30px';
  },
  () => {
    observer.disconnect();
    el('b').style.width = '50px';
  },
];
let frame = 0;
const next = () => requestAnimationFrame(() => {
  if (frame < steps.length) {
    steps[frame++]();
    next();
  } else {
    testRunner.reportHarnessResults(log.join(' / '));
  }
});
requestAnimationFrame(next);
</script>"#,
    );
    assert_eq!(
        report(&url),
        "a:10,b:10,c:10 / changed / a:20,b:10 / b:10,c:10"
    );
}

/// Observing asks for a frame, though nothing else changed: an idle
/// callback, which doesn't ask for one itself, observes once the page is
/// still (the second of two: the timer's task asked for a frame, which the
/// first may come before), and the observer is called back.
#[test]
fn observing_asks_for_a_frame() {
    let url = page(
        "asks.html",
        r#"<!DOCTYPE html>
<div id="target" style="width: 3px; height: 4px"></div>
<script>
onload = () => setTimeout(() => requestIdleCallback(() => requestIdleCallback(() => {
  new ResizeObserver(entries => {
    testRunner.reportHarnessResults(`${entries[0].contentRect.width}x${entries[0].contentRect.height}`);
  }).observe(document.getElementById('target'));
})), 50);
</script>"#,
    );
    assert_eq!(report(&url), "3x4");
}

/// In a frame: the animation frame callbacks, then the resize observers,
/// then the focus fixup (a focused button just disabled is still focused
/// in the callback, not after the frame), then the next task.
#[test]
fn the_order_in_a_frame() {
    let url = page(
        "order.html",
        r#"<!DOCTYPE html>
<div id="target"></div>
<button id="button">button</button>
<script>
const log = [];
const focused = () => document.activeElement.localName;
onload = () => requestAnimationFrame(() => setTimeout(() => {
  const button = document.getElementById('button');
  button.focus();
  button.disabled = true;
  new ResizeObserver(() => log.push(`resize ${focused()}`))
    .observe(document.getElementById('target'));
  requestAnimationFrame(() => {
    log.push(`raf ${focused()}`);
    setTimeout(() => {
      log.push(`task ${focused()}`);
      testRunner.reportHarnessResults(log.join(' / '));
    });
  });
}));
</script>"#,
    );
    assert_eq!(report(&url), "raf button / resize button / task body");
}

/// The depth loop: what a callback newly observes deeper than what was
/// broadcast is called back in the same frame; an element it resizes no
/// deeper than that (shallower, or as deep) waits for the next frame, and
/// the loop error is reported (an `ErrorEvent`, as an exception would be).
#[test]
fn the_depth_loop_and_its_error() {
    let url = page(
        "depth.html",
        r#"<!DOCTYPE html>
<div id="outer"><div id="inner" style="width: 10px"></div><div id="sibling" style="width: 10px"></div></div>
<script>
const log = [];
const el = id => document.getElementById(id);
let frames = 0;
const count = () => { frames++; requestAnimationFrame(count); };
requestAnimationFrame(count);
onerror = (message, source) => log.push(`${frames} error ${message} ${source === ''}`);
let round = 0;
const observer = new ResizeObserver(entries => {
  log.push(`${frames} ` + entries.map(e => e.target.id).join(','));
  round++;
  if (round === 1) {
    observer.observe(el('inner'));
    observer.observe(el('sibling'));
  } else if (round === 2) {
    el('outer').style.width = '50px';
    el('sibling').style.width = '20px';
  } else {
    testRunner.reportHarnessResults(log.join(' / '));
  }
});
observer.observe(el('outer'));
</script>"#,
    );
    let report = report(&url);
    let parts: Vec<(u32, &str)> = report
        .split(" / ")
        .map(|part| {
            let (frame, rest) = part.split_once(' ').unwrap();
            (frame.parse().unwrap(), rest)
        })
        .collect();
    let first = parts[0].0;
    assert_eq!(
        parts,
        [
            (first, "outer"),
            (first, "inner,sibling"),
            (
                first,
                "error ResizeObserver loop completed with undelivered notifications. true"
            ),
            (first + 1, "outer,sibling"),
        ],
        "{report}"
    );
}

/// What a broadcast of several depths at once lets through in the same
/// frame: its shallowest depth, so a deeper target it resized is called
/// back again in that frame, as laid out anew.
#[test]
fn the_shallowest_depth_broadcast() {
    let url = page(
        "shallowest.html",
        r#"<!DOCTYPE html>
<div id="outer"><div id="c" style="width: 10px"></div></div>
<script>
const log = [];
let frames = 0;
const count = () => { frames++; requestAnimationFrame(count); };
requestAnimationFrame(count);
onerror = message => log.push(`${frames} error`);
let round = 0;
const observer = new ResizeObserver(entries => {
  log.push(`${frames} ` + entries.map(e => `${e.target.id}:${e.contentRect.width}`).join(','));
  if (++round === 1) {
    document.getElementById('c').style.width = '20px';
  } else {
    setTimeout(() => testRunner.reportHarnessResults(log.join(' / ')), 100);
  }
});
observer.observe(document.getElementById('outer'));
observer.observe(document.getElementById('c'));
</script>"#,
    );
    let report = report(&url);
    let parts: Vec<(u32, &str)> = report
        .split(" / ")
        .map(|part| {
            let (frame, rest) = part.split_once(' ').unwrap();
            (frame.parse().unwrap(), rest)
        })
        .collect();
    let first = parts[0].0;
    assert_eq!(
        parts,
        [(first, "outer:784,c:10"), (first, "c:20")],
        "{report}"
    );
}

/// The device-pixel content box snaps to device pixels from where the box
/// starts (its border box's paint offset), as Chrome does: a 10.4px-wide
/// box at 0.3px is 11 pixels; moved there by a callback, it is the next
/// round's, in the same frame (each round is pre-painted anew).
#[test]
fn the_device_pixel_content_box() {
    let url = page(
        "device.html",
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } #c { width: 10.4px; height: 10px }</style>
<div id="outer"><div id="c"></div></div>
<div id="at" style="margin-left: 0.3px; width: 10.4px; height: 10.4px; padding-top: 0.3px"></div>
<script>
const log = [];
const el = id => document.getElementById(id);
const device = e => `${e.target.id}:${e.devicePixelContentBoxSize[0].inlineSize}x` +
                    `${e.devicePixelContentBoxSize[0].blockSize}`;
let frames = 0;
const count = () => { frames++; requestAnimationFrame(count); };
requestAnimationFrame(count);
const observer = new ResizeObserver(entries => {
  log.push(`${frames} ` + entries.map(device).join(','));
});
observer.observe(el('c'), {box: 'device-pixel-content-box'});
observer.observe(el('at'));
setTimeout(() => {
  // `outer` is broadcast first in this frame; its callback moves `c`,
  // whose snapped size changes, and `c` is deeper.
  new ResizeObserver(entries => {
    log.push(`${frames} outer`);
    el('c').style.marginLeft = '0.3px';
  }).observe(el('outer'));
  setTimeout(() => testRunner.reportHarnessResults(log.join(' / ')), 200);
}, 100);
</script>"#,
    );
    let report = report(&url);
    let parts: Vec<(u32, &str)> = report
        .split(" / ")
        .map(|part| {
            let (frame, rest) = part.split_once(' ').unwrap();
            (frame.parse().unwrap(), rest)
        })
        .collect();
    assert_eq!(parts.len(), 3, "{report}");
    assert_eq!(parts[0].1, "c:10x10,at:11x10", "{report}");
    assert_eq!(parts[1].1, "outer", "{report}");
    assert_eq!(parts[2], (parts[1].0, "c:11x10"), "{report}");
}

/// A child frame's observers are called back in its realm (their entries
/// are its), whoever's functions they call; a callback's exception is
/// reported in the callback's realm, and the next observer called back.
#[test]
fn a_child_frame_and_an_exception() {
    let url = page(
        "child.html",
        r#"<!DOCTYPE html>
<iframe srcdoc="<div id=x style='width: 7.25px; height: 3px; margin-left: 0.3px'></div>"></iframe>
<script>
onload = () => {
  const log = [];
  const child = frames[0];
  onerror = message => log.push(`error ${message}`);
  child.onerror = message => log.push(`child error ${message}`);
  new child.ResizeObserver(() => { throw new child.Error('boom'); })
    .observe(child.document.getElementById('x'));
  new child.ResizeObserver(function (entries) {
    log.push(`${this instanceof child.ResizeObserver} ` +
             `${entries[0] instanceof child.ResizeObserverEntry} ` +
             `${entries[0].contentRect.width}x${entries[0].contentRect.height} ` +
             `${entries[0].devicePixelContentBoxSize[0].inlineSize}`);
    testRunner.reportHarnessResults(log.join(' / '));
  }).observe(child.document.getElementById('x'));
};
</script>"#,
    );
    // 7.25px wide from 8.3px in the child frame (its body's margin, and
    // 0.3px): device pixels 8 to 16, 8 of them (7 if it weren't snapped
    // from where it starts).
    assert_eq!(
        report(&url),
        "error Uncaught Error: boom / true true 7.25x3 8"
    );
}

/// A callback's exception is reported, and the next observer called
/// back. (The DOM's message has no `Uncaught ` first yet, Chrome's
/// has: only the error's own is compared.)
#[test]
fn an_exception() {
    let url = page(
        "exception.html",
        r#"<!DOCTYPE html>
<div id="x" style="width: 7px; height: 3px"></div>
<script>
const log = [];
onerror = message => log.push(`error ${message.split(': ').pop()}`);
new ResizeObserver(() => { throw new Error('boom'); }).observe(document.getElementById('x'));
new ResizeObserver(function (entries) {
  log.push(`${this instanceof ResizeObserver} ${entries[0] instanceof ResizeObserverEntry} ` +
           `${entries[0].contentRect.width}x${entries[0].contentRect.height}`);
  testRunner.reportHarnessResults(log.join(' / '));
}).observe(document.getElementById('x'));
</script>"#,
    );
    assert_eq!(report(&url), "error boom / true true 7x3");
}

/// The depth is the flat tree's (Resize Observer's, Chrome's): an element
/// assigned to a slot is deeper than its host's shadow tree's elements, so
/// a callback resizing it after they were broadcast reaches it in the same
/// frame (in the shadow-including tree it is as deep as they are, and would
/// wait for the next, with the loop error).
#[test]
fn the_depth_is_the_flat_trees() {
    let url = page(
        "flat.html",
        r#"<!DOCTYPE html>
<div id="host"><div id="slotted" style="width: 10px"></div></div>
<script>
const el = id => document.getElementById(id);
const shadow = el('host').attachShadow({mode: 'open'});
shadow.innerHTML = '<div id="inner" style="width: 30px"><div><slot></slot></div></div>';
const log = [];
let frames = 0;
const count = () => { frames++; requestAnimationFrame(count); };
requestAnimationFrame(count);
onerror = () => log.push(`${frames} error`);
let round = 0;
const observer = new ResizeObserver(entries => {
  log.push(`${frames} ` + entries.map(e => `${e.target.id}:${e.contentRect.width}`).join(','));
  if (++round === 1) {
    el('slotted').style.width = '20px';
  } else {
    setTimeout(() => testRunner.reportHarnessResults(log.join(' / ')), 100);
  }
});
observer.observe(shadow.getElementById('inner'));
observer.observe(el('slotted'));
</script>"#,
    );
    let report = report(&url);
    let parts: Vec<(u32, &str)> = report
        .split(" / ")
        .map(|part| {
            let (frame, rest) = part.split_once(' ').unwrap();
            (frame.parse().unwrap(), rest)
        })
        .collect();
    let first = parts[0].0;
    assert_eq!(
        parts,
        [(first, "inner:30,slotted:10"), (first, "slotted:20")],
        "{report}"
    );
}

/// An observer's entries are read from the layout as it is called (as
/// Chrome's `ResizeObserver::DeliverObservations`): a callback before it
/// that resized the target and laid the document out (reading
/// `offsetWidth`) changes what the next observer of the same target sees,
/// and that size is what it reports; the first observer's own observation,
/// as deep as what was broadcast, waits for the next frame.
#[test]
fn entries_are_read_as_their_observer_is_called() {
    let url = page(
        "read.html",
        r#"<!DOCTYPE html>
<div id="a" style="width: 10px"></div><div id="b" style="width: 10px"></div>
<script>
const el = id => document.getElementById(id);
const log = [];
onerror = () => log.push('error');
new ResizeObserver(entries => {
  log.push(`first ${entries[0].target.id}:${entries[0].contentRect.width}`);
  el('a').style.width = '40px';
  el('a').offsetWidth;
}).observe(el('a'));
const second = new ResizeObserver(entries => {
  log.push(`second ${entries.map(e => `${e.target.id}:${e.contentRect.width}`).join(',')}`);
});
second.observe(el('a'));
second.observe(el('b'));
onload = () => setTimeout(() => testRunner.reportHarnessResults(log.join(' / ')), 300);
</script>"#,
    );
    assert_eq!(
        report(&url),
        "first a:10 / second a:40,b:10 / error / first a:40"
    );
}
