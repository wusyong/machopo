//! `performance` in a whole browser: a
//! document's timeline starts when its navigation did, so a page's script
//! sees the network's time already; `timeOrigin` is that start's Unix
//! time; a child frame's starts with its own navigation, an initial
//! `about:blank`'s as it is made, and its `performance` is of its own
//! realm; an animation frame's time and an event's `timeStamp` are on the
//! same timeline as `performance.now()`, never past it, in a child frame
//! (whose origin is late) too, where the frame's time isn't before the
//! callback was asked for. A mark is on its window's timeline, in its
//! window's buffer only (the child's has only its navigation entry).

use std::collections::HashMap;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use shell::headless;

#[test]
fn timelines_start_when_navigations_do() {
    let page = r#"<!DOCTYPE html>
<script>
const start = performance.now();
const mark = performance.mark('start');
const skew = Math.abs(performance.timeOrigin + performance.now() - Date.now());
onload = () => {
  const child = document.getElementById('f').contentWindow;
  // Nothing has read this one's `performance` yet: this window's getter
  // makes it, in its own realm all the same.
  const blank = document.body.appendChild(document.createElement('iframe')).contentWindow;
  const get = Object.getOwnPropertyDescriptor(window, 'performance').get;
  const blankRealm = get.call(blank) instanceof blank.Performance;
  // The documents run their callbacks in tree order: the child's after
  // this one, so the report waits for a task.
  requestAnimationFrame(time => {
    const now = performance.now();
    const stamp = new Event('x').timeStamp;
    setTimeout(() => testRunner.reportHarnessResults([
      start >= 300, skew < 100,
      child.startedAt >= 200,
      child.performance.timeOrigin >= performance.timeOrigin + 300,
      child.performance !== performance,
      blankRealm, blank.performance.timeOrigin >= performance.timeOrigin + 300,
      // Its frame began after it was asked for, before now.
      child.rafAsked <= child.rafTime && child.rafTime <= child.rafNow,
      time <= now, now - time < 1000,
      stamp <= performance.now(), now - stamp < 1000,
      mark.startTime >= start, performance.getEntriesByName('start')[0] === mark,
      child.performance.getEntries().map(e => e.entryType).join(),
    ].join(' / ')), 0);
  });
};
</script>
<iframe id="f" src="/child"></iframe>"#;
    let server = TestServer::start(HashMap::from([
        (
            "/".to_string(),
            Route::ok("text/html", page).with_delay(Duration::from_millis(300)),
        ),
        (
            "/child".to_string(),
            Route::ok(
                "text/html",
                "<script>window.startedAt = performance.now();
                 window.rafAsked = performance.now();
                 requestAnimationFrame(t => { window.rafTime = t; window.rafNow = performance.now(); });
                 </script>",
            )
            .with_delay(Duration::from_millis(200)),
        ),
    ]))
    .expect("test server starts");
    let report = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(
        report,
        "true / true / true / true / true / true / true / true / true / true / true / true / \
         true / true / navigation"
    );
}
