//! The user's mouse in a whole browser: the renderer's main thread
//! dispatches `mousedown`, `mouseup`, `click`, `dblclick`, `auxclick` and
//! `contextmenu` where the pointer is, `mousemove` once a frame; a press
//! moves the focus, and one in a child frame keeps the mouse's events
//! there until the button goes up; the boundary events, `:hover` and
//! `:active` follow the mouse. Each page logs into its title, or reports
//! through `testRunner`.

use std::path::PathBuf;
use std::time::Duration;

use gfx::Size;
use protocol::input::{InputEvent, KeyboardEvent, Modifiers, MouseButton};
use shell::headless::{self, Frame};

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-mouse");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

fn after(url: &str, input: &[InputEvent]) -> Frame {
    headless::render_after_input(url, Size::new(200, 200), 1.0, true, input)
        .expect("the page renders")
}

/// `button` down and up at `x`, `y`, the `count`th click of a series.
fn click(x: f32, y: f32, button: MouseButton, count: u32) -> [InputEvent; 2] {
    [
        InputEvent::MouseDown {
            x,
            y,
            button,
            buttons: button.buttons_bit(),
            modifiers: Modifiers::NONE,
            click_count: count,
        },
        InputEvent::MouseUp {
            x,
            y,
            button,
            buttons: 0,
            modifiers: Modifiers::NONE,
            click_count: count,
        },
    ]
}

/// The pointer moving to `x`, `y` with `buttons` down.
fn move_to(x: f32, y: f32, buttons: u16) -> InputEvent {
    InputEvent::MouseMove {
        x,
        y,
        buttons,
        modifiers: Modifiers::NONE,
    }
}

/// The events of a triple click, a double click of the middle button and
/// a click of the secondary one, in order, at what is under the pointer:
/// trusted, composed, `view` the window, with `button`, `buttons` and
/// `detail` (the click count); the primary button's second click is a
/// `dblclick` too (not the third, nor the middle button's). The secondary
/// button has `contextmenu` as it goes down (Windows: up). Canceling
/// `mousedown` doesn't stop the `click`.
#[test]
fn mouse_events_go_in_order() {
    let url = page(
        "order.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 } #a { height: 50px }</style><div id="a">a</div>
<script>
const log = [];
for (const type of ['mousedown', 'mouseup', 'click', 'dblclick', 'auxclick', 'contextmenu'])
  document.addEventListener(type, e => {
    log.push(`${e.type}:${e.target.id}:${e.button}:${e.buttons}:${e.detail}`);
    if (!e.isTrusted || !e.composed || e.view !== window) log.push('not-trusted-composed-or-viewed');
    if (e.type === 'mousedown') e.preventDefault();
    document.title = log.join(' ');
  });
</script>"#,
    );
    let mut input = Vec::new();
    input.extend(click(10.0, 10.0, MouseButton::Left, 1));
    input.extend(click(10.0, 10.0, MouseButton::Left, 2));
    input.extend(click(10.0, 10.0, MouseButton::Left, 3));
    input.extend(click(10.0, 10.0, MouseButton::Middle, 1));
    input.extend(click(10.0, 10.0, MouseButton::Middle, 2));
    input.extend(click(10.0, 10.0, MouseButton::Right, 1));
    let frame = after(&url, &input);
    let secondary = if cfg!(windows) {
        "mousedown:a:2:2:1 mouseup:a:2:0:1 auxclick:a:2:0:1 contextmenu:a:2:0:0"
    } else {
        "mousedown:a:2:2:1 contextmenu:a:2:2:0 mouseup:a:2:0:1 auxclick:a:2:0:1"
    };
    assert_eq!(
        frame.title.as_deref(),
        Some(
            format!(
                "mousedown:a:0:1:1 mouseup:a:0:0:1 click:a:0:0:1 \
                 mousedown:a:0:1:2 mouseup:a:0:0:2 click:a:0:0:2 dblclick:a:0:0:2 \
                 mousedown:a:0:1:3 mouseup:a:0:0:3 click:a:0:0:3 \
                 mousedown:a:1:4:1 mouseup:a:1:0:1 auxclick:a:1:0:1 \
                 mousedown:a:1:4:2 mouseup:a:1:0:2 auxclick:a:1:0:2 {secondary}"
            )
            .as_str()
        )
    );
}

/// A press moves the focus to the nearest element around what it hit that
/// can take it, without a ring; one where none can gives it to the
/// document (the body is the active element); a canceled `mousedown`
/// moves nothing. After a key, script's focus has a ring again.
#[test]
fn a_press_moves_the_focus() {
    let url = page(
        "focus.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 } div { height: 20px }</style>
<div id="f" tabindex="0"><span id="inner">inner</span></div>
<div id="plain">plain</div>
<div id="g" tabindex="0">g</div>
<script>
const log = [];
const note = text => { log.push(text); document.title = log.join(' '); };
for (const id of ['f', 'g']) {
  const element = document.getElementById(id);
  element.addEventListener('focus', () => note(`focus:${id}:${element.matches(':focus-visible')}`));
  element.addEventListener('blur', () => note(`blur:${id}`));
}
document.getElementById('plain').addEventListener('click', () => note(`active:${document.activeElement.tagName}`));
document.getElementById('g').addEventListener('mousedown', e => e.preventDefault());
document.addEventListener('keyup', () => document.getElementById('g').focus());
</script>"#,
    );
    let mut input = Vec::new();
    input.extend(click(10.0, 10.0, MouseButton::Left, 1));
    input.extend(click(10.0, 30.0, MouseButton::Left, 1));
    input.extend(click(10.0, 50.0, MouseButton::Left, 1));
    input.extend(click(10.0, 10.0, MouseButton::Left, 1));
    for down in [true, false] {
        input.push(InputEvent::Key(KeyboardEvent {
            down,
            key: "a".to_owned(),
            code: "KeyA".to_owned(),
            key_code: 65,
            text: if down { "a".to_owned() } else { String::new() },
            location: 0,
            repeat: false,
            modifiers: Modifiers::NONE,
        }));
    }
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some("focus:f:false blur:f active:BODY focus:f:false blur:f focus:g:true")
    );
}

/// The pointer's moves wait for the next frame, the last one standing for
/// those before it (here they come while a `mousedown` listener keeps the
/// main thread busy): one `mousemove`, with where it ended up, how far it
/// moved since the press and the buttons down, before the `mouseup`.
#[test]
fn moves_wait_for_the_frame() {
    let url = page(
        "moves.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0; height: 100% }</style>
<script>
const log = [];
const note = text => { log.push(text); document.title = log.join(' '); };
document.addEventListener('mousedown', () => {
  note('mousedown');
  const end = performance.now() + 100;
  while (performance.now() < end) {}
});
document.addEventListener('mousemove', e => note(`mousemove:${e.clientX}:${e.movementX}:${e.buttons}`));
document.addEventListener('mouseup', () => note('mouseup'));
</script>"#,
    );
    let [down, up] = click(10.0, 10.0, MouseButton::Left, 1);
    let input = [
        down,
        move_to(20.0, 10.0, 1),
        move_to(30.0, 10.0, 1),
        move_to(40.0, 10.0, 1),
        up,
    ];
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some("mousedown mousemove:40:30:1 mouseup")
    );
}

/// A press in a child frame keeps the mouse's events there until the
/// button goes up, outside it too (at its root element, in its viewport's
/// coordinates); then they go where the pointer is again.
#[test]
fn a_press_in_a_frame_keeps_the_mouse_there() {
    let url = page(
        "capture.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0; height: 100% } iframe { border: 0; width: 100px; height: 100px }</style>
<iframe srcdoc="<style>html, body { margin: 0 } div { height: 50px }</style><div>in</div><script>
for (const type of ['mousedown', 'mousemove', 'mouseup', 'click'])
  document.addEventListener(type, e => parent.note(`${e.type}:${e.target.tagName}:${e.clientX}`));
</script>"></iframe>
<script>
const log = [];
function note(text) { log.push(text); document.title = log.join(' '); }
for (const type of ['mousedown', 'mousemove', 'mouseup', 'click'])
  document.addEventListener(type, e => note(`top:${e.type}:${e.target.tagName}`));
</script>"#,
    );
    let [down, _] = click(10.0, 10.0, MouseButton::Left, 1);
    let [_, up] = click(150.0, 150.0, MouseButton::Left, 1);
    let input = [down, move_to(150.0, 150.0, 1), up, move_to(160.0, 160.0, 0)];
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "mousedown:DIV:10 mousemove:HTML:150 mouseup:HTML:150 click:HTML:150 \
             top:mousemove:BODY"
        )
    );
}

/// With no focused element, a key scrolls from what the mouse last went
/// down on: the scroll container around it, not the viewport.
#[test]
fn a_key_scrolls_from_where_the_mouse_went_down() {
    let url = page(
        "press-scroll.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 }</style>
<div id="box" style="height: 50px; overflow: auto"><div style="height: 300px">in</div></div>
<div style="height: 2000px"></div>
<script>
document.addEventListener('keyup', () => {
  document.title = document.getElementById('box').scrollTop + ',' + scrollY;
});
</script>"#,
    );
    let mut input = Vec::new();
    input.extend(click(10.0, 10.0, MouseButton::Left, 1));
    for down in [true, false] {
        input.push(InputEvent::Key(KeyboardEvent {
            down,
            key: "ArrowDown".to_owned(),
            code: "ArrowDown".to_owned(),
            key_code: 40,
            text: String::new(),
            location: 0,
            repeat: false,
            modifiers: Modifiers::NONE,
        }));
    }
    let frame = after(&url, &input);
    assert_eq!(frame.title.as_deref(), Some("40,0"));
}

/// What the mouse went down on counts only in the focused frame: once the
/// focus has gone into a child frame (and its element lost it), a key
/// scrolls from that frame out, not from the container pressed in the
/// page around it.
#[test]
fn the_pressed_element_counts_in_its_frame_only() {
    let url = page(
        "press-frame.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 } iframe { border: 0; height: 50px }</style>
<div id="box" style="height: 50px; overflow: auto"><div style="height: 300px">in</div></div>
<iframe srcdoc="<div id=x tabindex=0>x</div>"></iframe>
<div style="height: 2000px"></div>
<script>
const box = document.getElementById('box');
box.addEventListener('click', () => {
  const x = frames[0].document.getElementById('x');
  x.focus();
  x.blur();
});
document.addEventListener('keyup', () => {
  document.title = box.scrollTop + ',' + scrollY;
});
onload = () => frames[0].document.addEventListener('keyup', () => {
  document.title = box.scrollTop + ',' + scrollY;
});
</script>"#,
    );
    let mut input = Vec::new();
    input.extend(click(10.0, 10.0, MouseButton::Left, 1));
    for down in [true, false] {
        input.push(InputEvent::Key(KeyboardEvent {
            down,
            key: "ArrowDown".to_owned(),
            code: "ArrowDown".to_owned(),
            key_code: 40,
            text: String::new(),
            location: 0,
            repeat: false,
            modifiers: Modifiers::NONE,
        }));
    }
    let frame = after(&url, &input);
    assert_eq!(frame.title.as_deref(), Some("0,40"));
}

/// The pointer moving to `x`, `y`, and a release there with no button
/// down: the release dispatches the move, which waits for the frame
/// otherwise, and is at the same place, so nothing changes under it.
fn move_and_settle(x: f32, y: f32) -> [InputEvent; 2] {
    let [_, up] = click(x, y, MouseButton::Left, 0);
    [move_to(x, y, 0), up]
}

/// The page logs each boundary event (`over`, `out`, `enter`, `leave`) as
/// `type:target:relatedTarget`, through a capturing listener on the window
/// (`mouseenter` and `mouseleave` don't bubble), and at each `mousemove`
/// what is `:hover`.
const LOG_BOUNDARIES: &str = r#"<script>
const log = [];
const nameOf = n => n ? (n.id || n.nodeName) : 'null';
for (const type of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave'])
  window.addEventListener(type, e => {
    log.push(`${e.type.slice(5)}:${nameOf(e.target)}:${nameOf(e.relatedTarget)}`);
    if (!e.isTrusted || e.bubbles !== (type === 'mouseover' || type === 'mouseout')) log.push('bad');
    document.title = log.join(' ');
  }, true);
window.addEventListener('mousemove', () => {
  log.push('hover:' + Array.from(document.querySelectorAll(':hover'), nameOf).join(','));
  document.title = log.join(' ');
});
</script>"#;

/// Moving over elements: `mouseout` at what the mouse left, `mouseleave`
/// at it and its ancestors up to the one shared with what it entered,
/// `mouseover` at that, `mouseenter` at its ancestors from the shared one
/// down; first into the page, from nowhere, every ancestor up to the
/// document. `:hover` is the element under the mouse and its ancestors
/// (Blink's `BoundaryEventDispatcher`).
#[test]
fn boundary_events_follow_the_mouse() {
    let url = page(
        "boundaries.html",
        &format!(
            r#"<!DOCTYPE html><style>html, body {{ margin: 0 }} div {{ height: 50px }}
#a {{ height: 100px }}</style><div id="a"><div id="a1"></div></div><div id="b"></div>{LOG_BOUNDARIES}"#
        ),
    );
    let input: Vec<InputEvent> = [(10.0, 25.0), (10.0, 75.0), (10.0, 125.0)]
        .into_iter()
        .flat_map(|(x, y)| move_and_settle(x, y))
        .collect();
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "over:a1:null enter:#document:null enter:HTML:null enter:BODY:null \
             enter:a:null enter:a1:null hover:HTML,BODY,a,a1 \
             out:a1:a leave:a1:a over:a:a1 hover:HTML,BODY,a \
             out:a:b leave:a:b over:b:a enter:b:a hover:HTML,BODY,b"
        )
    );
}

/// A press makes the element and its ancestors `:active` before
/// `mousedown`; the release takes it off before `mouseup`. While it is
/// down, `:hover` keeps to them. The middle button's press too.
#[test]
fn a_press_is_active_until_the_release() {
    let url = page(
        "active.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 } div { height: 50px }</style>
<div id="a"></div><div id="b"></div>
<script>
const log = [];
const ids = s => Array.from(document.querySelectorAll(s), n => n.id || n.nodeName).join(',');
for (const type of ['mousedown', 'mousemove', 'mouseup'])
  document.addEventListener(type, () => {
    log.push(`${type}:${ids(':active')}:${ids(':hover')}`);
    document.title = log.join(' ');
  });
</script>"#,
    );
    let [down, _] = click(10.0, 10.0, MouseButton::Left, 1);
    let [_, up] = click(10.0, 60.0, MouseButton::Left, 1);
    let [middle_down, middle_up] = click(10.0, 60.0, MouseButton::Middle, 1);
    let frame = after(
        &url,
        &[down, move_to(10.0, 60.0, 1), up, middle_down, middle_up],
    );
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "mousedown:HTML,BODY,a:HTML,BODY,a mousemove:HTML,BODY,a:HTML,BODY \
             mouseup::HTML,BODY,b \
             mousedown:HTML,BODY,b:HTML,BODY,b mouseup::HTML,BODY,b"
        )
    );
}

/// What the mouse was over taken out of the document (by its `mouseover`
/// listener): its `mouseenter` goes to it out of the document, where the
/// window's listener doesn't hear it; the mouse is in its parent now,
/// which gets the `mousemove`, and no `mouseout` at the removed one after,
/// but a `mouseover` at the parent (Blink tracks the node's removal so).
#[test]
fn what_the_mouse_was_over_taken_out() {
    let url = page(
        "removed.html",
        &format!(
            r#"<!DOCTYPE html><style>html, body {{ margin: 0 }} div {{ height: 50px }}
#a {{ height: 100px }}</style><div id="a"><div id="a1"></div></div>{LOG_BOUNDARIES}
<script>document.getElementById('a1').addEventListener('mouseover', e => e.target.remove());</script>"#
        ),
    );
    let input: Vec<InputEvent> = [(10.0, 25.0), (10.0, 30.0)]
        .into_iter()
        .flat_map(|(x, y)| move_and_settle(x, y))
        .collect();
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "over:a1:null enter:#document:null enter:HTML:null enter:BODY:null \
             enter:a:null hover:HTML,BODY,a over:a:a hover:HTML,BODY,a"
        )
    );
}

/// A layout moves what is under the still mouse: the rendering update
/// finds it again, after the frame, with its boundary events and no
/// `mousemove` (Blink's `RecomputeMouseHoverState`).
#[test]
fn a_layout_moves_what_is_under_the_mouse() {
    let url = page(
        "relayout.html",
        &format!(
            r#"<!DOCTYPE html><style>html, body {{ margin: 0 }} div {{ height: 50px }}</style>
<div id="a"></div><div id="b"></div>{LOG_BOUNDARIES}
<script>document.getElementById('b').addEventListener('mouseover', () => {{
  document.getElementById('a').style.height = '100px';
}});
document.getElementById('a').addEventListener('mouseenter', () => {{
  setTimeout(() => testRunner.reportHarnessResults(log.join(' ')), 0);
}});</script>"#
        ),
    );
    // Only the move, which goes at the frame: a release after it would
    // find what is under the mouse again itself.
    let report = headless::run_testharness_after_input(
        &url,
        Size::new(200, 200),
        &[move_to(10.0, 75.0, 0)],
        Duration::from_secs(10),
    )
    .expect("the page reports");
    assert_eq!(
        report,
        "over:b:null enter:#document:null enter:HTML:null enter:BODY:null \
         enter:b:null hover:HTML,BODY,b \
         out:b:a leave:b:a over:a:b enter:a:b"
    );
}

/// Into a child frame and out: the iframe is what the mouse is over in
/// the frame around it, and the child frame has boundary events of its
/// own, with no `relatedTarget` across documents. The frames the mouse
/// leaves go first, then the ones it is over, the main frame first; one
/// left has nothing `:hover`.
#[test]
fn boundary_events_across_frames() {
    let url = page(
        "frames.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 } iframe { display: block; border: none; width: 100px; height: 50px }
div { height: 50px }</style>
<iframe id="f" srcdoc="<style>html, body { margin: 0; height: 100% }</style><div id=c style='height: 100%'></div><script>
for (const t of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave'])
  addEventListener(t, e => parent.note('in-' + t.slice(5), e), true);
</script>"></iframe><div id="b"></div>
<script>
const log = [];
const nameOf = n => n ? (n.id || n.nodeName) : 'null';
window.note = (type, e) => {
  log.push(`${type}:${nameOf(e.target)}:${nameOf(e.relatedTarget)}`);
  document.title = log.join(' ');
};
for (const t of ['mouseover', 'mouseout', 'mouseenter', 'mouseleave'])
  addEventListener(t, e => note(t.slice(5), e), true);
addEventListener('mousemove', () => {
  const inner = document.getElementById('f').contentDocument;
  log.push('in-hover:' + Array.from(inner.querySelectorAll(':hover'), nameOf).join(','));
  document.title = log.join(' ');
});
</script>"#,
    );
    let input: Vec<InputEvent> = [(10.0, 25.0), (10.0, 75.0)]
        .into_iter()
        .flat_map(|(x, y)| move_and_settle(x, y))
        .collect();
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "over:f:null enter:#document:null enter:HTML:null enter:BODY:null enter:f:null \
             in-over:c:null in-enter:#document:null in-enter:HTML:null in-enter:BODY:null \
             in-enter:c:null \
             in-out:c:null in-leave:c:null in-leave:BODY:null in-leave:HTML:null \
             in-leave:#document:null \
             out:f:b leave:f:b over:b:f enter:b:f in-hover:"
        )
    );
}
