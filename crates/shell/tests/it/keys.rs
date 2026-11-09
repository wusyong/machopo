//! The user's keys in a whole browser: the browser forwards them, the
//! renderer's main thread dispatches `keydown`, `keypress` and `keyup` at
//! the focused element, and a `keydown` nobody canceled does what the key
//! does: Tab moves the focus (into a child frame too), the scrolling keys
//! scroll from the focused element out, Enter follows a focused link. Each
//! page logs into its title.

use std::path::PathBuf;

use gfx::Size;
use protocol::input::{InputEvent, KeyboardEvent, Modifiers};
use shell::headless::{self, Frame};

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-keys");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

const SHIFT: Modifiers = Modifiers {
    shift: true,
    ..Modifiers::NONE
};
const CTRL: Modifiers = Modifiers {
    ctrl: true,
    ..Modifiers::NONE
};

/// A key down and up: `key`, its `code` and `keyCode`, what it types.
fn press(
    key: &str,
    code: &str,
    key_code: u16,
    text: &str,
    modifiers: Modifiers,
) -> [InputEvent; 2] {
    let event = |down| {
        InputEvent::Key(KeyboardEvent {
            down,
            key: key.to_owned(),
            code: code.to_owned(),
            key_code,
            text: if down { text.to_owned() } else { String::new() },
            location: 0,
            repeat: false,
            modifiers,
        })
    };
    [event(true), event(false)]
}

fn tab(shift: bool) -> [InputEvent; 2] {
    press(
        "Tab",
        "Tab",
        9,
        "",
        if shift { SHIFT } else { Modifiers::NONE },
    )
}

fn after(url: &str, input: &[InputEvent]) -> Frame {
    headless::render_after_input(url, Size::new(200, 200), 1.0, true, input)
        .expect("the page renders")
}

/// `keydown`, `keypress` (a key that types: Enter too, not a Ctrl
/// shortcut) and `keyup` go to the focused element (to the body with
/// none), trusted and composed, with the key's fields; a canceled
/// `keydown` has no `keypress`, and one whose listener moves the focus has
/// it at the new focused element.
#[test]
fn key_events_go_to_the_focused_element() {
    let url = page(
        "events.html",
        r#"<!DOCTYPE html><body id="body"><div id="a" tabindex="0">a</div><div id="b" tabindex="0">b</div>
<script>
const log = [];
for (const type of ['keydown', 'keypress', 'keyup'])
  document.addEventListener(type, e => {
    log.push(`${e.type}:${e.target.id}:${e.key}:${e.code}:${e.keyCode}:${e.charCode}`);
    if (!e.isTrusted || !e.composed) log.push('not-trusted-or-composed');
    if (e.type === 'keydown' && e.key === 'x') e.preventDefault();
    if (e.type === 'keydown' && e.key === 'm') document.getElementById('b').focus();
    document.title = log.join(' ');
  });
</script>"#,
    );
    let mut input = Vec::new();
    input.extend(press("a", "KeyA", 65, "a", Modifiers::NONE));
    input.extend(tab(false));
    input.extend(press("x", "KeyX", 88, "x", Modifiers::NONE));
    input.extend(press("Enter", "Enter", 13, "\r", Modifiers::NONE));
    input.extend(press("a", "KeyA", 65, "a", CTRL));
    input.extend(press("m", "KeyM", 77, "m", Modifiers::NONE));
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "keydown:body:a:KeyA:65:0 keypress:body:a:KeyA:97:97 keyup:body:a:KeyA:65:0 \
             keydown:body:Tab:Tab:9:0 keyup:a:Tab:Tab:9:0 \
             keydown:a:x:KeyX:88:0 keyup:a:x:KeyX:88:0 \
             keydown:a:Enter:Enter:13:0 keypress:a:Enter:Enter:13:13 keyup:a:Enter:Enter:13:0 \
             keydown:a:a:KeyA:65:0 keyup:a:a:KeyA:65:0 \
             keydown:a:m:KeyM:77:0 keypress:b:m:KeyM:109:109 keyup:b:m:KeyM:77:0"
        )
    );
}

/// Tab goes through the page's focusable elements in order, into a child
/// frame's and out; Shift+Tab back. With Ctrl (or Alt, or Cmd) a key is a
/// shortcut: Ctrl+Tab does nothing.
#[test]
fn tab_goes_through_the_frames() {
    let url = page(
        "tab.html",
        r##"<!DOCTYPE html>
<div id="first" tabindex="0">1</div>
<iframe srcdoc="<a id=inner href=#x>inner</a>"></iframe>
<a id="last" href="#y">last</a>
<script>
const log = [];
const note = name => () => { log.push(name); document.title = log.join(' '); };
document.getElementById('first').addEventListener('focus', note('first'));
document.getElementById('last').addEventListener('focus', note('last'));
onload = () => frames[0].document.getElementById('inner').addEventListener('focus', note('inner'));
</script>"##,
    );
    let mut input = Vec::new();
    for shift in [false, false, false, true, true] {
        input.extend(tab(shift));
    }
    input.extend(press("Tab", "Tab", 9, "", CTRL));
    let frame = after(&url, &input);
    assert_eq!(frame.title.as_deref(), Some("first inner last inner first"));
}

/// A scrolling key scrolls the focused element's scroll container (not
/// the viewport); `preventDefault` on its `keydown` stops it.
#[test]
fn keys_scroll_from_the_focused_element() {
    let html = |cancel: bool| {
        format!(
            r#"<!DOCTYPE html><style>html, body {{ margin: 0 }}</style>
<div id="box" style="height: 50px; overflow: auto">
  <div id="in" tabindex="0" style="height: 50px">in</div><div style="height: 200px"></div>
</div>
<div style="height: 2000px"></div>
<script>
onload = () => document.getElementById('in').focus();
{}
document.addEventListener('keyup', () => {{
  document.title = document.getElementById('box').scrollTop + ',' + scrollY;
}});
</script>"#,
            if cancel {
                "document.addEventListener('keydown', e => e.preventDefault());"
            } else {
                ""
            }
        )
    };
    let down = press("ArrowDown", "ArrowDown", 40, "", Modifiers::NONE);
    let frame = after(&page("scroll.html", &html(false)), &down);
    assert_eq!(frame.title.as_deref(), Some("40,0"));
    let frame = after(&page("scroll-canceled.html", &html(true)), &down);
    assert_eq!(frame.title.as_deref(), Some("0,0"));
}

/// A container that can't move that way (at its end) passes the key to
/// the one around it, before the viewport.
#[test]
fn keys_scroll_the_next_container_out() {
    let url = page(
        "nested.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 }</style>
<div id="outer" style="height: 100px; overflow: auto">
  <div id="inner" style="height: 50px; overflow: auto">
    <div style="height: 100px"></div><div id="in" tabindex="0" style="height: 20px">in</div>
  </div>
  <div style="height: 300px"></div>
</div>
<div style="height: 2000px"></div>
<script>
const byId = id => document.getElementById(id);
onload = () => {
  byId('inner').scrollTop = 1000;
  byId('in').focus({ preventScroll: true });
};
document.addEventListener('keyup', () => {
  document.title = [byId('inner').scrollTop, byId('outer').scrollTop, scrollY].join(',');
});
</script>"#,
    );
    let frame = after(
        &url,
        &press("ArrowDown", "ArrowDown", 40, "", Modifiers::NONE),
    );
    assert_eq!(frame.title.as_deref(), Some("70,40,0"));
}

/// With the focus in a child frame, a key scrolls its container there.
#[test]
fn keys_scroll_in_a_child_frame() {
    let url = page(
        "frame-scroll.html",
        r#"<!DOCTYPE html><style>html, body { margin: 0 }</style>
<iframe style="border: 0; width: 150px; height: 150px" srcdoc="<style>body { margin: 0 }</style><div id=box style='height: 50px; overflow: auto'><div id=in tabindex=0 style='height: 50px'>in</div><div style='height: 200px'></div></div>"></iframe>
<div style="height: 2000px"></div>
<script>
onload = () => {
  const child = frames[0].document;
  child.getElementById('in').focus();
  child.addEventListener('keyup', () => {
    document.title = child.getElementById('box').scrollTop + ',' + scrollY;
  });
};
</script>"#,
    );
    let frame = after(
        &url,
        &press("ArrowDown", "ArrowDown", 40, "", Modifiers::NONE),
    );
    assert_eq!(frame.title.as_deref(), Some("40,0"));
}

/// Enter on the focused link follows it; on an `<a>` without an `href`
/// (not a link), it does nothing.
#[test]
fn enter_follows_the_focused_link() {
    let enter = press("Enter", "Enter", 13, "\r", Modifiers::NONE);
    let url = page(
        "enter.html",
        r##"<!DOCTYPE html><a id="link" href="#there">go</a>
<script>onload = () => document.getElementById('link').focus();</script>"##,
    );
    let frame = after(&url, &enter);
    assert_eq!(frame.url.as_deref(), Some(format!("{url}#there").as_str()));

    let url = page(
        "enter-no-href.html",
        r#"<!DOCTYPE html><a id="a" tabindex="0">not a link</a>
<script>
const a = document.getElementById('a');
onload = () => a.focus();
a.addEventListener('click', () => document.title = 'clicked');
document.addEventListener('keyup', () => document.title ||= 'not clicked');
</script>"#,
    );
    let frame = after(&url, &enter);
    assert_eq!(frame.title.as_deref(), Some("not clicked"));
}

/// Tab reaches an SVG link and an SVG element with a `tabindex`, not one
/// that only listens for focus events (its `tabIndex` is -1) nor one in
/// `<defs>`; Enter on the SVG link follows it.
#[test]
fn tab_and_enter_reach_svg_links() {
    let url = page(
        "svg.html",
        r##"<!DOCTYPE html>
<div id="first" tabindex="0">1</div>
<svg width="100" height="50">
  <a id="link" href="#there"><rect width="10" height="10"/></a>
  <rect id="listener" x="20" width="5" height="5"/>
  <defs><rect id="defs" tabindex="0" width="5" height="5"/></defs>
  <rect id="rect" tabindex="0" x="40" width="5" height="5"/>
</svg>
<a id="last" href="#y">last</a>
<script>
const log = [];
for (const id of ['first', 'link', 'listener', 'defs', 'rect', 'last'])
  document.getElementById(id).addEventListener('focus', () => {
    log.push(id);
    document.title = log.join(' ');
  });
</script>"##,
    );
    let mut input = Vec::new();
    for shift in [false, false, false, false, true, true] {
        input.extend(tab(shift));
    }
    input.extend(press("Enter", "Enter", 13, "\r", Modifiers::NONE));
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some("first link rect last rect link")
    );
    assert_eq!(frame.url.as_deref(), Some(format!("{url}#there").as_str()));
}

/// An SVG `<a>` with only SVG 1.1's `xlink:href` is a link too: Tab
/// reaches it, Enter follows it.
#[test]
fn an_svg_link_of_xlink_href() {
    let url = page(
        "svg-xlink.html",
        r##"<!DOCTYPE html>
<svg width="100" height="50">
  <a id="link" xlink:href="#xlinked"><rect width="10" height="10"/></a>
</svg>
<script>
document.getElementById('link').addEventListener('focus', () => document.title = 'focused');
</script>"##,
    );
    let mut input = Vec::new();
    input.extend(tab(false));
    input.extend(press("Enter", "Enter", 13, "\r", Modifiers::NONE));
    let frame = after(&url, &input);
    assert_eq!(frame.title.as_deref(), Some("focused"));
    assert_eq!(
        frame.url.as_deref(),
        Some(format!("{url}#xlinked").as_str())
    );
}

/// Tab reaches an editing host without a `tabindex` (the selection going
/// to its start, and `selectionchange` following: the host has no
/// listener of its own, whose script would queue it anyway), not an
/// editable element inside it nor one with `contenteditable=false`.
#[test]
fn tab_reaches_an_editing_host() {
    let url = page(
        "editing-host.html",
        r##"<!DOCTYPE html><title></title>
<div id="first" tabindex="0">1</div>
<div id="host" contenteditable>host <b id="nested" contenteditable>n</b></div>
<div id="off" contenteditable="false">off</div>
<a id="last" href="#y">last</a>
<script>
const log = [];
for (const id of ['first', 'nested', 'off', 'last'])
  document.getElementById(id).addEventListener('focus', () => {
    log.push(id);
    document.querySelector('title').textContent = log.join(' ');
  });
document.addEventListener('selectionchange', () => {
  const sel = getSelection();
  log.push(`change:${document.activeElement.id}:${sel.anchorNode.data}:${sel.anchorOffset}`);
  document.querySelector('title').textContent = log.join(' ');
});
</script>"##,
    );
    let after = |input: &[InputEvent]| after(&url, input);
    let mut input = Vec::new();
    for _ in 0..2 {
        input.extend(tab(false));
    }
    let frame = after(&input);
    assert_eq!(frame.title.as_deref(), Some("first change:host:host :0"));
    // The next Tab and the event's task may come in either order.
    input.extend(tab(false));
    let frame = after(&input);
    let title = frame.title.unwrap_or_default();
    let focused: Vec<_> = title
        .split(' ')
        .filter(|entry| !entry.starts_with("change:") && *entry != ":0")
        .collect();
    assert_eq!(focused, ["first", "last"], "{title}");
}
