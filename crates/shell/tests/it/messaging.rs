//! `postMessage` between a page and its child frame in a whole browser:
//! the messages come as tasks of the renderer's event loop, each way; a
//! message for another origin is dropped; `origin` and `source` are the
//! poster's, `data` a copy.

use std::collections::HashMap;

use net::test_server::{Route, TestServer};
use shell::headless;
use std::time::Duration;

#[test]
fn a_page_and_its_child_frame_talk() {
    let page = r#"<!DOCTYPE html>
<script>
const replies = [];
const sent = {n: 1};
addEventListener('message', e => {
  const child = document.getElementById('f').contentWindow;
  if (e.data === 'ready') {
    // Not the child's origin: dropped when its task runs.
    child.postMessage({n: 0}, 'https://elsewhere.test');
    child.postMessage(sent, location.origin);
    sent.n = 2;
    setTimeout(() => testRunner.reportHarnessResults([
      replies.length, replies[0].echo.n, replies[0].from === location.origin,
      replies[0].sourceWasParent, replies[0].source === child,
      replies[0].origin === location.origin,
    ].join(' / ')), 200);
    return;
  }
  replies.push({...e.data, source: e.source, origin: e.origin});
});
</script>
<iframe id="f" src="/child"></iframe>"#;
    let child = r#"<!DOCTYPE html>
<script>
onmessage = e => parent.postMessage(
  {echo: e.data, from: e.origin, sourceWasParent: e.source === parent}, '*');
parent.postMessage('ready', '*');
</script>"#;
    let server = TestServer::start(HashMap::from([
        ("/".to_string(), Route::ok("text/html", page)),
        ("/child".to_string(), Route::ok("text/html", child)),
    ]))
    .expect("test server starts");
    let report = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(report, "1 / 1 / true / true / true / true");
}

/// A channel's messages and `setTimeout(0)` run in the order they were
/// queued, one task at a time, as in Chrome 152 (measured on 2026-10-11):
/// two chains of five, each queuing the next, interleaved; whichever came
/// first, from an animation frame's callback too; and a message before the
/// next animation frame (asked for in a frame's callback, so that no frame
/// is due yet).
#[test]
fn messages_and_timers_run_in_the_order_queued() {
    let page = r#"<!DOCTYPE html>
<script>
const results = [];
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function run(setup) {
  const order = [];
  await new Promise((resolve) => setup(order, resolve));
  results.push(order.join(' '));
  await wait(50);
}
(async () => {
  // First, from the top-level script: a timer set in a timer's microtask
  // is nested in Chrome and not here (HTML runs microtasks after the task).
  await run((order, done) => {
    const { port1, port2 } = new MessageChannel();
    let m = 0, t = 0;
    port1.onmessage = () => { order.push('m' + m); if (++m < 5) port2.postMessage(null); };
    const tick = () => { order.push('t' + t); if (++t < 5) setTimeout(tick); };
    setTimeout(tick);
    port2.postMessage(null);
    setTimeout(done, 100);
  });
  await run((order, done) => {
    const { port1, port2 } = new MessageChannel();
    port1.onmessage = () => order.push('message');
    setTimeout(() => order.push('timer'));
    port2.postMessage(null);
    setTimeout(done, 30);
  });
  await run((order, done) => {
    const { port1, port2 } = new MessageChannel();
    port1.onmessage = () => order.push('message');
    port2.postMessage(null);
    setTimeout(() => order.push('timer'));
    setTimeout(done, 30);
  });
  await run((order, done) => {
    requestAnimationFrame(() => {
      const { port1, port2 } = new MessageChannel();
      port1.onmessage = () => order.push('message');
      port2.postMessage(null);
      setTimeout(() => order.push('timer'));
      setTimeout(done, 30);
    });
  });
  // From a frame's callback: a frame due already, under load, could come
  // before the message from anywhere else (HTML lets the rendering update
  // run between any two tasks; Chrome's does too once a frame is due).
  await run((order, done) => {
    requestAnimationFrame(() => {
      const { port1, port2 } = new MessageChannel();
      port1.onmessage = () => order.push('message');
      requestAnimationFrame(() => order.push('rAF'));
      port2.postMessage(null);
      setTimeout(done, 60);
    });
  });
  testRunner.reportHarnessResults(results.join(' / '));
})();
</script>"#;
    let server = TestServer::start(HashMap::from([(
        "/".to_string(),
        Route::ok("text/html", page),
    )]))
    .expect("test server starts");
    let report = headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports");
    assert_eq!(
        report,
        "t0 m0 t1 m1 t2 m2 t3 m3 t4 m4 / timer message / message timer / \
         message timer / message rAF"
    );
}
