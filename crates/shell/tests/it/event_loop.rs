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

/// A port shipped as it is enabled, a message waiting for it, comes
/// elsewhere (here, to the same page) with that message, then one posted
/// as it came, before its new port is enabled: both come once it is, in
/// order.
#[test]
fn a_shipped_ports_messages_wait_for_its_new_port() {
    let page = format!(
        "<!DOCTYPE html><script>{REPORT}
const {{ port1, port2 }} = new MessageChannel();
port2.onmessage = () => note('old port', 2);
port1.postMessage('a');
onmessage = e => {{
  const port = e.ports[0];
  port1.postMessage('b');
  setTimeout(() => {{ port.onmessage = m => note(m.data, 2); }}, 50);
}};
postMessage('', '*', [port2]);
setTimeout(() => testRunner.reportHarnessResults(log.join() || 'none'), 1000);
</script>"
    );
    assert_eq!(
        report(&[("/", Route::ok("text/html", page.as_str()))]),
        "a,b"
    );
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

/// A parser that gave way parses on while the responses to a chain of
/// `fetch()`es keep coming (Blink's parser pumps again in a task of its
/// own, `SchedulePumpTokenizer`): a child frame's long document, asked
/// for as the chain starts, is parsed to its end before the chain ends.
#[test]
fn a_parser_parses_on_while_responses_keep_coming() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
var responses = 0;
var parsed = -1;
const next = () => fetch('/r?' + responses, { cache: 'no-store' }).then(() => {
  responses++;
  if (responses < 150) { next(); busy(20); return; }
  testRunner.reportHarnessResults(parsed >= 0 && parsed < 150 ? 'parsed as they came' : `parsed after ${parsed}`);
});
fetch('/start', { cache: 'no-store' }).then(() => {
  const frame = document.createElement('iframe');
  frame.src = '/long';
  document.body.append(frame);
  next();
  busy(20);
});
</script>"#;
    let long = format!(
        "<!DOCTYPE html>{}<script>parent.parsed = parent.responses</script>",
        "<div><span>x</span></div>".repeat(60_000)
    );
    let report = report(&[
        ("/", Route::ok("text/html", page)),
        ("/long", Route::ok("text/html", long.as_str())),
        ("/r", Route::ok("text/plain", "r")),
        (
            "/start",
            Route::ok("text/plain", "start").with_delay(Duration::from_millis(300)),
        ),
    ]);
    assert_eq!(report, "parsed as they came");
}

/// The engine's platform tasks are the scheduler's: V8's foreground task
/// that cleans up after a GC (a `FinalizationRegistry`'s callback) runs as
/// it is posted, in a task of its own, with nothing else to wake the page
/// (`gc()`, V8's `--expose-gc`); a worker's in its own loop.
#[test]
fn a_finalization_registrys_callback_runs_after_a_gc() {
    let server = crate::automation_pipe::server(&[(
        "/",
        Route::ok(
            "text/html",
            r#"<!DOCTYPE html><body><script>
const registry = new FinalizationRegistry(held => {
  const p = document.createElement('p');
  p.id = held;
  document.body.append(p);
  // Again, once this one's task ran: the platform's next task wakes it too.
  if (held == 'cleaned') {
    (() => registry.register({}, 'cleaned-again'))();
    setTimeout(() => gc(), 50);
  }
});
(() => registry.register({}, 'cleaned'))();
setTimeout(() => gc(), 50);
// A worker's too: what its engine posts runs in its loop.
const code = `const registry = new FinalizationRegistry(() => postMessage('cleaned-in-worker'));
(() => registry.register({}, 0))();
setTimeout(() => gc(), 50);`;
const worker = new Worker(URL.createObjectURL(new Blob([code], { type: 'text/javascript' })));
worker.onmessage = e => {
  const p = document.createElement('p');
  p.id = e.data;
  document.body.append(p);
};
</script>"#,
        ),
    )]);
    let flags = std::ffi::OsStr::new("--expose-gc");
    let mut browser =
        crate::automation_pipe::Browser::start_with_env(&[("MACHOPO_V8_FLAGS", flags)]);
    let session = browser.attach();
    browser.navigate(&session, &server.url("/"));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    for cleaned in ["#cleaned", "#cleaned-again", "#cleaned-in-worker"] {
        while browser.query(cleaned)["element"].is_null() {
            assert!(std::time::Instant::now() < deadline, "no {cleaned}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

/// A canvas's `toBlob()` comes back while the responses to a chain of
/// `fetch()`es keep coming: its encoding goes to the encoder as the task
/// that asked ends, not once the tasks stop.
#[test]
fn a_canvas_blob_comes_while_responses_keep_coming() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
let responses = 0;
let encoded = -1;
const next = () => fetch('/r?' + responses, { cache: 'no-store' }).then(() => {
  responses++;
  if (responses < 40) { next(); busy(20); return; }
  testRunner.reportHarnessResults(encoded >= 0 && encoded < 40 ? 'encoded as they came' : `encoded after ${encoded}`);
});
fetch('/start', { cache: 'no-store' }).then(() => {
  const canvas = document.createElement('canvas');
  canvas.toBlob(() => { encoded = responses; });
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
    assert_eq!(report, "encoded as they came");
}

/// A timer of no timeout set as the thread has run past an earlier one's
/// time runs, and before it, as in Chrome (each timer is a task of its
/// own, the earlier one in the scheduler's queue as it is taken: HTML
/// orders only a timer of a timeout no longer after one set before it);
/// the others after them run too.
#[test]
fn a_timer_set_after_an_earlier_ones_time_runs() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
const ran = [];
const end = setTimeout(() => testRunner.reportHarnessResults(ran.join(',') || 'none'), 2000);
onload = () => {
  setTimeout(() => ran.push('a'), 10);
  busy(20);
  setTimeout(() => ran.push('b'), 0);
};
</script>"#;
    let report = report(&[("/", Route::ok("text/html", page))]);
    assert_eq!(report, "b,a");
}

/// The same of the UA's timers (`AbortSignal.timeout()`'s): each runs as
/// its own token comes, the one set as the thread had run past the other's
/// time first, as script timers do.
#[test]
fn a_ua_timer_set_after_an_earlier_ones_time_runs() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
const ran = [];
setTimeout(() => testRunner.reportHarnessResults(ran.join(',') || 'none'), 2000);
onload = () => {
  AbortSignal.timeout(10).onabort = () => ran.push('abort a');
  busy(20);
  AbortSignal.timeout(0).onabort = () => ran.push('abort b');
};
</script>"#;
    let report = report(&[("/", Route::ok("text/html", page))]);
    assert_eq!(report, "abort b,abort a");
}

/// Idle callbacks' timeouts run by their tokens, the earliest due first,
/// whatever order they were asked for in: the page busy, only their
/// timeouts run them.
#[test]
fn idle_timeouts_run_earliest_due_first() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
const ran = [];
setTimeout(() => testRunner.reportHarnessResults(ran.join(',') || 'none'), 2000);
onload = () => {
  requestIdleCallback(() => ran.push('a'), { timeout: 60 });
  requestIdleCallback(() => ran.push('b'), { timeout: 20 });
  // Busy past both timeouts, a task at a time.
  for (let i = 0; i < 10; i++) setTimeout(() => busy(15), 0);
};
</script>"#;
    let report = report(&[("/", Route::ok("text/html", page))]);
    assert_eq!(report, "b,a");
}

/// An animation frame callback asked for runs while the responses to a
/// chain of `fetch()`es keep coming: the compositor hears a main frame is
/// wanted as the task that asked ends, though another always waits
/// (Blink's `SetNeedsAnimate` as it is asked for).
#[test]
fn an_animation_frame_comes_while_responses_keep_coming() {
    let page = r#"<!DOCTYPE html><script>
const busy = ms => { const until = performance.now() + ms; while (performance.now() < until); };
let responses = 0;
let frame = -1;
const next = () => fetch('/r?' + responses, { cache: 'no-store' }).then(() => {
  responses++;
  if (responses < 40) { next(); busy(20); return; }
  testRunner.reportHarnessResults(frame >= 0 && frame < 40 ? 'the frame came as they came' : `the frame came after ${frame}`);
});
fetch('/start', { cache: 'no-store' }).then(() => {
  requestAnimationFrame(() => { frame = responses; });
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
    assert_eq!(report, "the frame came as they came");
}
