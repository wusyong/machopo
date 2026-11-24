//! The renderer's event loop in a whole browser: the DOM's tasks take
//! their turn among the scheduler's, whatever else keeps coming, in the
//! order Chrome gives them.

use std::collections::HashMap;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use shell::headless;

/// What the page at `/` of a server with `routes` reports.
fn report(routes: &[(&str, Route)]) -> String {
    let server = TestServer::start(
        routes
            .iter()
            .map(|(path, route)| ((*path).to_owned(), route.clone()))
            .collect::<HashMap<_, _>>(),
    )
    .expect("the test server starts");
    headless::run_testharness(
        &server.url("/"),
        Duration::from_secs(30),
        Default::default(),
    )
    .expect("the page reports")
}

/// A timer due while the responses to a chain of `fetch()`es keep coming
/// runs before the chain ends (Blink's timer is a task of the frame's
/// scheduler, picked among the networking ones by when it was queued):
/// each response's handler asks for the next and keeps the thread busy
/// until that one is in, so a task always waits as the one before ends.
#[test]
fn a_timer_runs_while_responses_keep_coming() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
let responses = 0;
let timer = -1;
const next = () => fetch('/r?' + responses, { cache: 'no-store' }).then(() => {
  responses++;
  if (responses < 40) { next(); busy(20); return; }
  testRunner.reportHarnessResults(timer >= 0 && timer < 40 ? 'the timer ran as they came' : `the timer ran after ${timer}`);
});
// Once the page has loaded, and none of its own tasks wait.
fetch('/start', { cache: 'no-store' }).then(() => {
  setTimeout(() => { timer = responses; }, 0);
  next();
  busy(20);
});
</script>"#;
    let report = report(&[
        ("/", Route::ok("text/html", page)),
        ("/r", Route::ok("text/plain", "r")),
        (
            "/start",
            Route::ok("text/plain", "start").with_delay(Duration::from_millis(300)),
        ),
    ]);
    assert_eq!(report, "the timer ran as they came");
}

/// A timer comes to its turn as it is due, as Chrome's delayed task gets
/// its place in the order as it is ready: a message posted after the
/// timer was set but before it was due, in a task that ran past when it
/// was, runs first.
#[test]
fn a_timer_takes_its_turn_as_it_is_due() {
    let page = r#"<!DOCTYPE html><script>
const log = [];
const done = () => { if (log.length == 2) testRunner.reportHarnessResults(log.join()); };
onmessage = () => { log.push('message'); done(); };
onload = () => setTimeout(() => {
  setTimeout(() => { log.push('timer'); done(); }, 30);
  const until = performance.now() + 60;
  while (performance.now() < until);
  postMessage('', '*');
}, 0);
</script>"#;
    assert_eq!(
        report(&[("/", Route::ok("text/html", page))]),
        "message,timer"
    );
}

/// Messages to a port that was moved (transferred, with both its ends)
/// before it was enabled come in the order they were posted, those posted
/// before the move and those after: each one a task in its turn.
#[test]
fn a_moved_ports_messages_come_in_order() {
    let page = r#"<!DOCTYPE html><script>
const log = [];
setTimeout(() => testRunner.reportHarnessResults(log.join()), 500);
const channel1 = new MessageChannel();
const channel2 = new MessageChannel();
channel1.port1.postMessage(1);
channel2.port1.postMessage('transfer', [channel1.port1]);
channel2.port1.postMessage('transfer', [channel1.port2]);
let sender;
channel2.port2.onmessage = evt => {
  if (!sender) { sender = evt.ports[0]; return; }
  sender.postMessage(2);
  evt.ports[0].onmessage = e => log.push(e.data);
  sender.postMessage(3);
};
</script>"#;
    assert_eq!(report(&[("/", Route::ok("text/html", page))]), "1,2,3");
}

/// What a worker posted before it closed itself comes to its `Worker`
/// (only the page's `terminate()` drops what waits).
#[test]
fn what_a_worker_posted_before_it_closed_comes() {
    let page = r#"<!DOCTYPE html><script>
const code = 'postMessage(1); close(); postMessage(2);';
const worker = new Worker(URL.createObjectURL(new Blob([code], { type: 'text/javascript' })));
const log = [];
worker.onmessage = e => log.push(e.data);
setTimeout(() => testRunner.reportHarnessResults(log.join()), 500);
</script>"#;
    assert_eq!(report(&[("/", Route::ok("text/html", page))]), "1,2");
}

/// What reports `log` once it has `count` entries, joined.
const REPORT: &str = "const log = [];
const note = (text, count) => { log.push(text); if (log.length == count) testRunner.reportHarnessResults(log.join()); };";

/// A page with nothing else to do wakes for its timer: the loop waits
/// for the scheduler's next task, the timer's as it comes due.
#[test]
fn an_idle_page_wakes_for_its_timer() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
onload = () => setTimeout(() => note('ran', 1), 100);
</script>"
    );
    assert_eq!(
        report(&[("/", Route::ok("text/html", page.as_str()))]),
        "ran"
    );
}

/// An interval comes again, each time set again as it runs.
#[test]
fn an_interval_comes_again() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
let n = 0;
const id = setInterval(() => {{ n++; if (n == 3) clearInterval(id); note(n, 3); }}, 20);
</script>"
    );
    assert_eq!(
        report(&[("/", Route::ok("text/html", page.as_str()))]),
        "1,2,3"
    );
}

/// The UA's own timers are the scheduler's too: `AbortSignal.timeout()`
/// aborts as its time comes.
#[test]
fn a_ua_timer_comes() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
AbortSignal.timeout(50).onabort = e => note(e.target.reason.name, 1);
</script>"
    );
    assert_eq!(
        report(&[("/", Route::ok("text/html", page.as_str()))]),
        "TimeoutError"
    );
}

/// An idle callback whose page never goes idle runs as its timeout comes,
/// while the responses of a chain of `fetch()`es keep the thread busy.
#[test]
fn an_idle_callbacks_timeout_comes_while_the_page_is_busy() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
const busy = ms => {{ const until = performance.now() + ms; while (performance.now() < until); }};
let responses = 0;
const next = () => fetch('/r?' + responses, {{ cache: 'no-store' }}).then(() => {{
  responses++;
  if (responses < 40) {{ next(); busy(20); }}
}});
fetch('/start', {{ cache: 'no-store' }}).then(() => {{
  requestIdleCallback(deadline => note(String(deadline.didTimeout), 1), {{ timeout: 50 }});
  next();
  busy(20);
}});
</script>"
    );
    let report = report(&[
        ("/", Route::ok("text/html", page.as_str())),
        ("/r", Route::ok("text/plain", "r")),
        (
            "/start",
            Route::ok("text/plain", "start").with_delay(Duration::from_millis(300)),
        ),
    ]);
    assert_eq!(report, "true");
}

/// A message posted to a port before it is enabled comes once it is, in
/// a later task.
#[test]
fn a_ports_message_waits_for_it_to_be_enabled() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
const channel = new MessageChannel();
channel.port1.postMessage(1);
setTimeout(() => {{ channel.port2.onmessage = e => note(e.data, 1); }}, 50);
</script>"
    );
    assert_eq!(report(&[("/", Route::ok("text/html", page.as_str()))]), "1");
}

/// What a worker threw as it closed itself (in a task of its, after
/// `close()`) comes to its `Worker`.
#[test]
fn what_a_worker_threw_before_it_closed_comes() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
const code = 'onmessage = () => {{ close(); throw new Error(\"boom\"); }};';
const worker = new Worker(URL.createObjectURL(new Blob([code], {{ type: 'text/javascript' }})));
worker.onerror = e => {{ e.preventDefault(); note('error', 1); }};
worker.postMessage('close');
</script>"
    );
    assert_eq!(
        report(&[("/", Route::ok("text/html", page.as_str()))]),
        "error"
    );
}
