//! `navigator.scheduling.isInputPending()` in a whole browser: input the
//! compositor handed to the main thread while a script keeps it busy is
//! pending until the main thread takes it, a mouse move only with
//! `includeContinuous`; a key is the focused frame's; a press is the frame's
//! under it, which the page sees only if it is of its origin (the engine
//! asks the renderer's counts through its input monitor).

use std::path::PathBuf;
use std::time::Duration;

use gfx::Size;
use protocol::input::{InputEvent, KeyboardEvent, Modifiers, MouseButton};
use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-scheduling");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// A page with `body` whose script, at the first key, keeps the main
/// thread busy until input is pending, asking `isInputPending()` with
/// `includeContinuous` until it is and then without it; then reports both,
/// whether anything is pending once the main thread took the input (the
/// last move, until a frame dispatches it), and 100 ms later, once that
/// frame came.
fn busy(body: &str) -> String {
    BUSY.replace("BODY", body)
}

const BUSY: &str = r#"<!DOCTYPE html>
<body style="margin: 0; height: 200px">BODY
<script>
let started = false;
addEventListener('keydown', () => {
  if (started) return;
  started = true;
  const s = navigator.scheduling, start = performance.now();
  let continuous = false;
  while (!continuous && performance.now() - start < 5000) {
    continuous = s.isInputPending({ includeContinuous: true });
  }
  const discrete = s.isInputPending();
  // The next task runs once the main thread took the input, before the
  // frame that dispatches the last move; then 100 ms more, for that frame.
  setTimeout(() => {
    const taken = s.isInputPending({ includeContinuous: true });
    setTimeout(() => testRunner.reportHarnessResults(
      `discrete ${discrete}, continuous ${continuous}, ${s === navigator.scheduling}, ` +
      `taken ${taken}, then ${s.isInputPending({ includeContinuous: true })}`), 100);
  }, 0);
});
</script>"#;

fn key(key: &str, code: &str, key_code: u16) -> InputEvent {
    InputEvent::Key(KeyboardEvent {
        down: true,
        key: key.into(),
        code: code.into(),
        key_code,
        text: key.into(),
        location: 0,
        repeat: false,
        modifiers: Modifiers::NONE,
    })
}

/// `input` sent while the page is busy: a key starts it, and a move on the
/// page after `input` is what it waits for. The compositor hands them to
/// the main thread in order, counting each as it does, and the main thread
/// takes none while busy: once the move is pending, so is all of `input`
/// that the page may see, whenever it came.
fn run_page(name: &str, html: &str, input: &[InputEvent]) -> String {
    let mut events = vec![key("s", "KeyS", 83)];
    events.extend_from_slice(input);
    events.push(InputEvent::mouse_move(190.0, 190.0));
    headless::run_testharness_after_input(
        &page(name, html),
        Size::new(200, 200),
        &events,
        Duration::from_secs(30),
    )
    .expect("the page reports")
}

fn run(name: &str, input: &[InputEvent]) -> String {
    run_page(name, &busy(""), input)
}

/// [`run_page`] without what the page says once the main thread took the
/// input (`taken`), which asks the renderer's counts through its engine's
/// input monitor: the DOM asks for frames as it handles the
/// input, and the frame that dispatches the last move may come before the
/// timer that asks.
fn run_page_without_taken(name: &str, html: &str, input: &[InputEvent]) -> String {
    let mut events = vec![key("s", "KeyS", 83)];
    events.extend_from_slice(input);
    events.push(InputEvent::mouse_move(190.0, 190.0));
    headless::run_testharness_after_input(
        &page(name, html),
        Size::new(200, 200),
        &events,
        Duration::from_secs(30),
    )
    .expect("the page reports")
    .split(", ")
    .filter(|part| !part.starts_with("taken "))
    .collect::<Vec<_>>()
    .join(", ")
}

/// A press on the page is pending for it, with or without the continuous
/// ones.
#[test]
fn a_press_is_pending() {
    let press = InputEvent::mouse_down(50.0, 50.0, MouseButton::Left, Modifiers::NONE);
    assert_eq!(
        run("press.html", &[press]),
        "discrete true, continuous true, true, taken true, then false"
    );
}

/// A move is a continuous event: pending only for `includeContinuous`.
#[test]
fn a_move_is_continuous() {
    assert_eq!(
        run("move.html", &[InputEvent::mouse_move(50.0, 50.0)]),
        "discrete false, continuous true, true, taken true, then false"
    );
}

/// A key is the focused frame's: the page's, which has the focus.
#[test]
fn a_key_is_the_focused_frames() {
    let key = key("a", "KeyA", 65);
    assert_eq!(
        run("key.html", &[key]),
        "discrete true, continuous true, true, taken true, then false"
    );
}

/// The DOM's: a press on the page, a key, and a press on a child frame
/// of its origin are pending for it, a move only with the continuous ones;
/// a press on another origin's frame isn't, nor one on the page's own box
/// over that frame, where cc can't tell whose it is; one beside the frames
/// is the page's.
#[test]
fn input_is_pending() {
    let press = |x, y| InputEvent::mouse_down(x, y, MouseButton::Left, Modifiers::NONE);
    assert_eq!(
        run_page_without_taken("next-press.html", &busy(""), &[press(50.0, 50.0)]),
        "discrete true, continuous true, true, then false"
    );
    assert_eq!(
        run_page_without_taken(
            "next-move.html",
            &busy(""),
            &[InputEvent::mouse_move(50.0, 50.0)]
        ),
        "discrete false, continuous true, true, then false"
    );
    assert_eq!(
        run_page_without_taken("next-key.html", &busy(""), &[key("a", "KeyA", 65)]),
        "discrete true, continuous true, true, then false"
    );
    let frames = "<iframe srcdoc='<p>same' style='position: absolute; left: 0; top: 0; width: 100px; height: 50px; border: 0'></iframe>\
        <iframe src='data:text/html,other' style='position: absolute; left: 0; top: 100px; width: 100px; height: 50px; border: 0'></iframe>\
        <div style='position: absolute; left: 80px; top: 100px; width: 20px; height: 20px'></div>";
    let html = busy(frames);
    assert_eq!(
        run_page_without_taken("next-same-origin.html", &html, &[press(50.0, 25.0)]),
        "discrete true, continuous true, true, then false"
    );
    assert_eq!(
        run_page_without_taken("next-other-origin.html", &html, &[press(50.0, 125.0)]),
        "discrete false, continuous true, true, then false"
    );
    assert_eq!(
        run_page_without_taken("next-over-other-origin.html", &html, &[press(90.0, 110.0)]),
        "discrete false, continuous true, true, then false"
    );
    assert_eq!(
        run_page_without_taken("next-beside.html", &html, &[press(150.0, 125.0)]),
        "discrete true, continuous true, true, then false"
    );
}

/// A child frame in a container that is a layer is found where the
/// container is: the page sees a press on its own origin's frame in it
/// (so the compositor didn't give up there) and not one on another
/// origin's (so it wasn't the page's), and sees one below the container.
#[test]
fn a_press_in_a_container_is_the_frames_under_it() {
    let container = |src: &str| {
        busy(&format!(
            "<div style='height: 100px'></div>\
             <div style='overflow: scroll; will-change: transform; width: 150px; height: 60px'>\
             <iframe {src} style='display: block; width: 100px; height: 50px; border: 0'></iframe>\
             <div style='height: 300px'></div></div>"
        ))
    };
    let press = |x, y| InputEvent::mouse_down(x, y, MouseButton::Left, Modifiers::NONE);
    assert_eq!(
        run_page_without_taken(
            "in-container-same.html",
            &container("srcdoc='same'"),
            &[press(50.0, 125.0)]
        ),
        "discrete true, continuous true, true, then false"
    );
    let other = container("src='data:text/html,other'");
    assert_eq!(
        run_page_without_taken("in-container-other.html", &other, &[press(50.0, 125.0)]),
        "discrete false, continuous true, true, then false"
    );
    assert_eq!(
        run_page_without_taken("below-container.html", &other, &[press(50.0, 175.0)]),
        "discrete true, continuous true, true, then false"
    );
}
