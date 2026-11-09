//! The user's wheel in a whole browser: over what a blocking `wheel`
//! listener is on (the wheel event handler region) the compositor waits for
//! the main thread, whose `wheel` event may cancel the scrolling; elsewhere
//! it scrolls, and the listeners get an event that can't. A `mousewheel`
//! listener gets the wheel where there is no `wheel` one. Each page logs
//! into its `<title>`'s text.

use std::path::PathBuf;

use gfx::Size;
use protocol::input::{InputEvent, Modifiers, ScrollDelta};
use shell::headless::{self, Frame};

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-wheel");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// The page at `url` after `input`.
fn after(url: &str, input: &[InputEvent]) -> Frame {
    headless::render_after_input(url, Size::new(200, 200), 1.0, true, input)
        .expect("the page renders")
}

/// The pixel at `x`, `y` (physical px), RGBA.
fn pixel(frame: &Frame, x: u32, y: u32) -> [u8; 4] {
    let at = usize::try_from((y * frame.size.width + x) * 4).unwrap();
    frame.pixels[at..at + 4].try_into().unwrap()
}

/// A wheel at `x`, `y` scrolling `dy` CSS px down, with `modifiers` held.
fn wheel(x: f32, y: f32, dy: f32, modifiers: Modifiers) -> InputEvent {
    InputEvent::MouseWheel {
        x,
        y,
        delta: ScrollDelta::Pixels { x: 0.0, y: dy },
        buttons: 0,
        modifiers,
    }
}

/// Over the element with a blocking listener the wheel's `wheel` event is
/// cancelable, and canceled the page doesn't scroll; elsewhere the
/// compositor scrolls first, and the event (to the document's listener,
/// passive by default, which can't cancel it) isn't cancelable. The event
/// is trusted, with the scroll's delta in pixels, the legacy `wheelDelta`
/// and the keys held.
#[test]
fn a_wheel_waits_for_a_blocking_listener_only_over_its_element() {
    let url = page(
        "cancel.html",
        r#"<!DOCTYPE html><title></title><style>html, body { margin: 0 }</style>
<div id="map" style="height: 100px">map</div>
<div style="height: 2000px"></div>
<script>
const log = [];
const note = text => { log.push(text); document.querySelector('title').textContent = log.join(' '); };
document.getElementById('map').addEventListener('wheel', e => {
  note(`map:${e.cancelable}:${e.deltaY}:${e.deltaMode}:${e.wheelDelta}:${e.ctrlKey}:${e.isTrusted}`);
  e.preventDefault();
});
document.addEventListener('wheel', e => {
  e.preventDefault();
  note(`document:${e.cancelable}:${e.defaultPrevented}:${scrollY}`);
});
</script>"#,
    );
    let ctrl = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let input = [
        wheel(10.0, 50.0, 40.0, ctrl),
        wheel(10.0, 150.0, 40.0, Modifiers::NONE),
    ];
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some("map:true:40:0:-40:true:true document:true:true:0 document:false:false:40")
    );
}

/// A blocking listener on the window covers the whole document, where no
/// box is too (below the document's few px): every wheel waits for it.
#[test]
fn a_blocking_listener_on_the_window_covers_the_document() {
    let url = page(
        "window.html",
        r#"<!DOCTYPE html><title></title><style>html, body { margin: 0 }</style>
<div style="height: 10px"></div>
<script>
addEventListener('wheel', e => {
  e.preventDefault();
  document.querySelector('title').textContent = `${e.cancelable}:${scrollY}`;
}, { passive: false });
</script>"#,
    );
    let frame = after(&url, &[wheel(10.0, 150.0, 40.0, Modifiers::NONE)]);
    assert_eq!(frame.title.as_deref(), Some("true:0"));
}

/// A target with no `wheel` listener has its `mousewheel` ones called with
/// the user's wheel, as `mousewheel` (Chrome's legacy type); one with a
/// `wheel` listener only has that called. Script's `wheel` reaches no
/// `mousewheel` listener. `wheelDelta` is `wheelDeltaY`, or `wheelDeltaX`
/// where that is 0, the other way from the deltas.
#[test]
fn mousewheel_listeners_get_the_wheel_where_there_is_no_wheel_listener() {
    let url = page(
        "mousewheel.html",
        r#"<!DOCTYPE html><title></title><style>html, body { margin: 0 } div { height: 100px }</style>
<div id="a">a</div><div id="b">b</div>
<div style="height: 2000px"></div>
<script>
const log = [];
const note = text => { log.push(text); document.querySelector('title').textContent = log.join(' '); };
const a = document.getElementById('a');
const b = document.getElementById('b');
a.addEventListener('mousewheel', e => note(`a:${e.type}:${e.wheelDelta}:${e.deltaY}`));
b.addEventListener('mousewheel', e => note(`b:${e.type}`));
b.addEventListener('wheel', e => note(`b:${e.type}`));
document.addEventListener('mousewheel', e => note(`document:${e.type}`));
a.dispatchEvent(new WheelEvent('wheel', { bubbles: true }));
const sideways = new WheelEvent('wheel', { deltaX: 7 });
note(`sideways:${sideways.wheelDeltaX}:${sideways.wheelDeltaY}:${sideways.wheelDelta}`);
</script>"#,
    );
    let input = [
        wheel(10.0, 50.0, 40.0, Modifiers::NONE),
        wheel(10.0, 150.0, 40.0, Modifiers::NONE),
    ];
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "sideways:-7:0:-7 a:mousewheel:-40:40 document:mousewheel b:wheel document:mousewheel"
        )
    );
}

/// A wheel the main thread scrolls only because the compositor can't pick
/// the container (one that isn't a layer) waits for it, but no listener may
/// cancel it: its `wheel` isn't cancelable. The main thread scrolls it.
#[test]
fn a_wheel_the_main_thread_scrolls_for_no_listener_is_not_cancelable() {
    let url = page(
        "main-thread.html",
        r#"<!DOCTYPE html><title></title><style>html, body { margin: 0 }</style>
<div id="box" style="height: 100px; overflow: auto; opacity: 0.9"><div style="height: 30px; background: green"></div><div style="height: 300px"></div></div>
<div style="height: 2000px"></div>
<script>
document.addEventListener('wheel', e => { document.querySelector('title').textContent = `${e.cancelable}`; });
</script>"#,
    );
    let still = after(&url, &[]);
    assert_ne!(pixel(&still, 10, 10), [255; 4], "the green box shows");
    let frame = after(&url, &[wheel(10.0, 50.0, 40.0, Modifiers::NONE)]);
    assert_eq!(frame.title.as_deref(), Some("false"));
    assert_eq!(pixel(&frame, 10, 10), [255; 4], "the container scrolled");
}
