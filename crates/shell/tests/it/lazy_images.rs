//! `loading=lazy` and `sizes="auto"` in a whole browser: a lazy `<img>` loads once its document's lazy load
//! observer finds it near a scrollport (Chrome 152's distances, measured:
//! 1250px up and down, 625px sideways, of the viewport and of each scroll
//! container), without delaying the load event; a scroll brings one in;
//! `sizes="auto"` is the width it is laid out at, and selects again as it
//! changes.

use std::collections::HashMap;
use std::time::Duration;

use net::test_server::{Route, TestServer};
use protocol::input::{InputEvent, Modifiers, ScrollDelta};
use shell::headless;

/// A server of `index` and PNGs of 1×1 at the `small` paths, 16×16 at the
/// `large` ones.
fn server(index: &str, small: &[&str], large: &[&str]) -> TestServer {
    let wpt =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../third_party/wpt/images");
    let one = std::fs::read(wpt.join("green-1x1.png")).expect("WPT's image");
    let sixteen = std::fs::read(wpt.join("green-16x16.png")).expect("WPT's image");
    let mut routes = HashMap::from([("/index.html".to_owned(), Route::ok("text/html", index))]);
    for path in small {
        routes.insert((*path).to_owned(), Route::ok("image/png", one.clone()));
    }
    for path in large {
        routes.insert((*path).to_owned(), Route::ok("image/png", sixteen.clone()));
    }
    TestServer::start(routes).expect("the test server starts")
}

/// What the page reports, laid out in a 400×300 viewport after `input`.
fn report(server: &TestServer, input: &[InputEvent]) -> String {
    headless::run_testharness_after_input(
        &server.url("/index.html"),
        gfx::Size::new(400, 300),
        input,
        Duration::from_secs(30),
    )
    .expect("the page reports")
}

/// Which lazy images load in a 400×300 viewport: those up to 1250px below
/// it and 625px beside it; in an `overflow: auto` container (200×100, no
/// scrollbars drawn), those up to as far from its scrollport; none of an
/// `overflow: clip` box's that it clips (it isn't a scroll container).
/// The window's load event doesn't wait for those that don't.
#[test]
fn which_lazy_images_load() {
    let ids = [
        "in", "y1", "y2", "x1", "x2", "sy1", "sy2", "sx1", "sx2", "clip",
    ];
    let small: Vec<String> = ids.iter().map(|id| format!("/{id}.png")).collect();
    let small: Vec<&str> = small.iter().map(String::as_str).collect();
    let server = server(
        r#"<!DOCTYPE html>
<style>
html, body { margin: 0 } body { height: 5000px; width: 5000px }
img { position: absolute; width: 10px; height: 10px }
.box { position: absolute; left: 0; width: 200px; height: 100px }
.box > div { position: relative; width: 3000px; height: 3000px }
</style>
<img loading="lazy" id="in" src="in.png" style="top: 0; left: 100px">
<img loading="lazy" id="y1" src="y1.png" style="top: 1545px; left: 0">
<img loading="lazy" id="y2" src="y2.png" style="top: 1555px; left: 0">
<img loading="lazy" id="x1" src="x1.png" style="top: 0; left: 1020px">
<img loading="lazy" id="x2" src="x2.png" style="top: 0; left: 1030px">
<div class="box" style="top: 50px; overflow: auto"><div>
  <img loading="lazy" id="sy1" src="sy1.png" style="top: 1345px; left: 0">
  <img loading="lazy" id="sy2" src="sy2.png" style="top: 1355px; left: 0">
  <img loading="lazy" id="sx1" src="sx1.png" style="top: 0; left: 820px">
  <img loading="lazy" id="sx2" src="sx2.png" style="top: 0; left: 830px">
</div></div>
<div class="box" style="top: 200px; overflow: clip"><div>
  <img loading="lazy" id="clip" src="clip.png" style="top: 150px; left: 0">
</div></div>
<script>
onload = () => setTimeout(() => {
  testRunner.reportHarnessResults([...document.querySelectorAll('img')]
    .map(i => `${i.id}:${i.complete && i.naturalWidth > 0 ? 1 : 0}`).join(' '));
}, 500);
</script>"#,
        &small,
        &[],
    );
    assert_eq!(
        report(&server, &[]),
        "in:1 y1:1 y2:0 x1:1 x2:0 sy1:1 sy2:0 sx1:1 sx2:0 clip:0"
    );
}

/// A lazy image too far below the viewport loads once the page scrolls
/// near it (the compositor's scroll, a wheel's).
#[test]
fn a_scroll_brings_a_lazy_image_in() {
    let server = server(
        r#"<!DOCTYPE html>
<style>html, body { margin: 0 } body { height: 5000px }</style>
<img loading="lazy" id="far" src="far.png" style="position: absolute; top: 2000px; width: 10px; height: 10px">
<script>
const far = document.getElementById('far');
let loaded = false;
far.onload = () => { loaded = true; testRunner.reportHarnessResults(`loaded ${scrollY}`); };
onload = () => setTimeout(() => { if (!loaded) testRunner.reportHarnessResults('not loaded'); }, 3000);
</script>"#,
        &["/far.png"],
        &[],
    );
    let wheel = InputEvent::MouseWheel {
        x: 10.0,
        y: 10.0,
        delta: ScrollDelta::Pixels { x: 0.0, y: 500.0 },
        buttons: 0,
        modifiers: Modifiers::NONE,
    };
    assert_eq!(report(&server, &[wheel]), "loaded 500");
}

/// `sizes="auto"` of a lazy image is its width as laid out (10px: the
/// 100w candidate, a density of 10); laid out wider, it selects again
/// (500px: the 800w one, 1.6), no script asking.
#[test]
fn sizes_auto_follows_the_width() {
    let server = server(
        r#"<!DOCTYPE html>
<img loading="lazy" sizes="auto" id="img" style="width: 10px; height: 10px"
     srcset="small.png 100w, large.png 800w">
<script>
const img = document.getElementById('img');
const log = [];
img.onload = () => {
  log.push(`${img.currentSrc.split('/').pop()} ${img.naturalWidth}`);
  if (log.length === 1) img.style.width = '500px';
  else testRunner.reportHarnessResults(log.join(' / '));
};
</script>"#,
        &["/small.png"],
        &["/large.png"],
    );
    assert_eq!(report(&server, &[]), "small.png 0 / large.png 10");
}
