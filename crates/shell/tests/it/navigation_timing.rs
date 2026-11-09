//! Navigation Timing in a whole browser: the document's navigation entry
//! from the browser's load of it, its times in order, through a redirect
//! of its own origin (counted), its sizes; a reload's `type`; the
//! observers get it once `load` has ended; a child frame's redirects of
//! another origin and its reload; the `unload` of the document before.

use std::time::Duration;

use net::test_server::Route;
use shell::headless;

use crate::automation_pipe::server;

/// Reports, once `load` has ended, whether each of the entry's times is
/// at or after the one before (0 for none allowed where it may be), and
/// the rest of it.
const REPORT: &str = r#"<!DOCTYPE html>
<p>a page</p>
<script>
const seen = [];
new PerformanceObserver(list => seen.push(list.getEntries()[0].entryType))
  .observe({ type: 'navigation' });
addEventListener('load', () => setTimeout(() => {
  const e = performance.getEntriesByType('navigation')[0];
  const order = ['redirectStart', 'redirectEnd', 'fetchStart', 'domainLookupStart',
    'domainLookupEnd', 'connectStart', 'connectEnd', 'requestStart', 'responseStart',
    'responseEnd', 'domInteractive', 'domContentLoadedEventStart', 'domContentLoadedEventEnd',
    'domComplete', 'loadEventStart', 'loadEventEnd'];
  const out = order.filter((k, i) => i > 0 && !(e[k] >= e[order[i - 1]])).map(k => 'early ' + k);
  const t = performance.timing;
  out.push(e.redirectCount, e.redirectStart > 0, e.type, e.nextHopProtocol, e.responseStatus,
           e.contentType, e.encodedBodySize === e.decodedBodySize,
           e.transferSize - e.encodedBodySize, e.duration === e.loadEventEnd,
           e.name.endsWith('/page'), Math.abs(t.navigationStart - performance.timeOrigin) < 1,
           t.loadEventEnd >= t.domComplete, seen.join(), performance.navigation.redirectCount);
  testRunner.reportHarnessResults(out.join(' '));
}, 50));
</script>"#;

#[test]
fn the_entry_of_a_redirected_load() {
    let server = server(&[
        ("/start", Route::redirect(302, "/page")),
        ("/page", Route::ok("text/html", REPORT)),
    ]);
    let results = headless::run_testharness(
        &server.url("/start"),
        Duration::from_secs(30),
        Default::default(),
    );
    assert_eq!(
        results.as_deref(),
        Ok("1 true navigate http/1.1 200 text/html true 300 true true true true navigation 1")
    );
}

/// A child frame's entry is of its own load, the redirects of its
/// navigation counted.
#[test]
fn a_child_frames_redirected_load() {
    let top = r#"<!DOCTYPE html>
<iframe src="/start"></iframe>
<script>
onload = () => setTimeout(() => {
  const child = document.querySelector('iframe').contentWindow;
  const e = child.performance.getEntriesByType('navigation')[0];
  testRunner.reportHarnessResults(`${e.name.endsWith('/child')} ${e.redirectCount} ${e.redirectStart <= e.redirectEnd} ${e.redirectEnd <= e.fetchStart}`);
}, 0);
</script>"#;
    let server = server(&[
        ("/top", Route::ok("text/html", top)),
        ("/start", Route::redirect(302, "/child")),
        ("/child", Route::ok("text/html", "<p>child</p>")),
    ]);
    let results = headless::run_testharness(
        &server.url("/top"),
        Duration::from_secs(30),
        Default::default(),
    );
    assert_eq!(results.as_deref(), Ok("true 1 true true"));
}

/// A reload is `reload`, in the entry and in `performance.navigation`.
#[test]
fn a_reload() {
    let page = r#"<!DOCTYPE html>
<script>
addEventListener('load', () => setTimeout(() => {
  if (performance.navigation.type === 0) {
    location.reload();
    return;
  }
  const e = performance.getEntriesByType('navigation')[0];
  testRunner.reportHarnessResults(`${e.type} ${performance.navigation.type} ${e.redirectCount}`);
}, 0));
</script>"#;
    let server = server(&[("/page", Route::ok("text/html", page))]);
    let results = headless::run_testharness(
        &server.url("/page"),
        Duration::from_secs(30),
        Default::default(),
    );
    assert_eq!(results.as_deref(), Ok("reload 1 0"));
}

/// A child frame's redirect of another origin counts if it opts in to the
/// document's (`Timing-Allow-Origin`), unless its iframe's referrer policy
/// is `no-referrer` (its request's origin `null`, which only `*` opts in
/// to); its reload is `reload`, a load of its URL even with a fragment.
#[test]
fn a_child_frames_redirect_of_another_origin_and_its_reload() {
    let child = r#"<!DOCTYPE html>
<script>
const e = performance.getEntriesByType('navigation')[0];
parent.postMessage(`${e.type} ${e.redirectCount}`, '*');
if (e.type === 'navigate' && location.hash === '#reloads') location.reload();
</script>"#;
    let destination = server(&[("/child", Route::ok("text/html", child))]);
    let child_url = destination.url("/child");
    let origin = child_url.trim_end_matches("/child").to_owned();
    let top = r#"<!DOCTYPE html>
<script>
const seen = [];
addEventListener('message', (m) => {
  seen.push(m.data);
  if (seen.length === 5) testRunner.reportHarnessResults(seen.sort().join(' / '));
});
for (const [policy, hop] of [['', '/hop'], ['no-referrer', '/hop'], ['no-referrer', '/hop-any'],
                             ['', '/hop#reloads']]) {
  const frame = document.createElement('iframe');
  frame.referrerPolicy = policy;
  frame.src = hop;
  document.body.append(frame);
}
</script>"#;
    let server = server(&[
        ("/top", Route::ok("text/html", format!("<body>{top}"))),
        (
            "/hop",
            Route::redirect(302, &child_url).with_header("Timing-Allow-Origin", &origin),
        ),
        (
            "/hop-any",
            Route::redirect(302, &child_url).with_header("Timing-Allow-Origin", "*"),
        ),
    ]);
    let results = headless::run_testharness(
        &server.url("/top"),
        Duration::from_secs(30),
        Default::default(),
    );
    assert_eq!(
        results.as_deref(),
        Ok("navigate 0 / navigate 1 / navigate 1 / navigate 1 / reload 0"),
    );
}

/// A frame's document is unloaded as the frame navigates on (`pagehide`,
/// then `unload`, then its frames', as in Chrome), its `unload`'s time
/// the next document's if they are of one origin, with no redirect of
/// another between; a document of another origin before it shows none.
#[test]
fn the_unload_of_the_document_before() {
    let first = r#"<!DOCTYPE html>
<iframe src="/inner"></iframe>
<script>
addEventListener('pagehide', () => parent.log.push('pagehide'));
addEventListener('unload', () => parent.log.push('unload'));
onload = () => setTimeout(() => location.href = location.hash === '#away' ? parent.elsewhere : '/next', 0);
</script>"#;
    let inner = r#"<!DOCTYPE html>
<script>addEventListener('unload', () => parent.parent.log.push('inner unload'));</script>"#;
    let next = r#"<!DOCTYPE html>
<script>
const e = performance.getEntriesByType('navigation')[0];
const t = performance.timing;
parent.postMessage([e.unloadEventStart > 0, e.unloadEventEnd >= e.unloadEventStart,
  e.unloadEventEnd <= e.responseEnd || e.unloadEventEnd > 0, t.unloadEventStart > 0].join(), '*');
</script>"#;
    let other = server(&[("/next", Route::ok("text/html", next))]);
    let top = format!(
        r#"<!DOCTYPE html>
<script>
var log = [], seen = [];
var elsewhere = '{}';
// One frame after the other: the logs apart.
const open = (src) => {{
  const frame = document.createElement('iframe');
  frame.src = src;
  document.documentElement.append(frame);
}};
addEventListener('message', (m) => {{
  seen.push(m.data + ' ' + log.join(' '));
  log = [];
  if (seen.length === 2) testRunner.reportHarnessResults(seen.join(' / '));
  else open('/first#away');
}});
open('/first');
</script>"#,
        other.url("/next")
    );
    let server = server(&[
        ("/top", Route::ok("text/html", top)),
        ("/first", Route::ok("text/html", first)),
        ("/inner", Route::ok("text/html", inner)),
        ("/next", Route::ok("text/html", next)),
    ]);
    let results = headless::run_testharness(
        &server.url("/top"),
        Duration::from_secs(30),
        Default::default(),
    );
    assert_eq!(
        results.as_deref(),
        Ok("true,true,true,true pagehide unload inner unload / \
            false,true,true,false pagehide unload inner unload")
    );
}

/// The top-level document is unloaded as the page navigates on, its
/// `unload`'s time the next document's (of its origin).
#[test]
fn the_unload_of_the_top_level_document_before() {
    let first = r#"<!DOCTYPE html>
<script>onload = () => setTimeout(() => location.href = '/next', 0);</script>"#;
    let next = r#"<!DOCTYPE html>
<script>
const e = performance.getEntriesByType('navigation')[0];
testRunner.reportHarnessResults(`${e.unloadEventStart > 0} ${e.unloadEventEnd >= e.unloadEventStart}`);
</script>"#;
    let server = server(&[
        ("/first", Route::ok("text/html", first)),
        ("/next", Route::ok("text/html", next)),
    ]);
    let results = headless::run_testharness(
        &server.url("/first"),
        Duration::from_secs(30),
        Default::default(),
    );
    assert_eq!(results.as_deref(), Ok("true true"));
}
