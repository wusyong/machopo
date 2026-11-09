//! The form controls in a whole browser: the user's click checks a checkbox
//! (with `input` and `change`), through its `<label>` too, and isn't
//! dispatched at a disabled one; Tab reaches the controls, Space clicks a
//! checkbox or a button without scrolling, Enter a button; a reset button
//! resets its form; the form pseudo-classes follow the user's clicks. The
//! controls aren't drawn yet: the pages give them a size, so that the
//! pointer can hit them. Each page logs into its title.

use std::path::PathBuf;

use gfx::Size;
use protocol::input::{InputEvent, KeyboardEvent, Modifiers, MouseButton};
use shell::headless::{self, Frame};

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-forms");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

fn after(url: &str, input: &[InputEvent]) -> Frame {
    headless::render_after_input(url, Size::new(200, 200), 1.0, true, input)
        .expect("the page renders")
}

/// The primary button down and up at `x`, `y`.
fn click(x: f32, y: f32) -> [InputEvent; 2] {
    let button = MouseButton::Left;
    [
        InputEvent::MouseDown {
            x,
            y,
            button,
            buttons: button.buttons_bit(),
            modifiers: Modifiers::NONE,
            click_count: 1,
        },
        InputEvent::MouseUp {
            x,
            y,
            button,
            buttons: 0,
            modifiers: Modifiers::NONE,
            click_count: 1,
        },
    ]
}

/// A key down and up: `key`, its `code` and `keyCode`, what it types.
fn press(key: &str, code: &str, key_code: u16, text: &str) -> [InputEvent; 2] {
    let event = |down| {
        InputEvent::Key(KeyboardEvent {
            down,
            key: key.to_owned(),
            code: code.to_owned(),
            key_code,
            text: if down { text.to_owned() } else { String::new() },
            location: 0,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
    };
    [event(true), event(false)]
}

const STYLE: &str = "<style>html, body { margin: 0 } input, button, label { display: block; \
                     margin: 0; padding: 0; border: 0; width: 100px; height: 20px }</style>";

/// A click on a checkbox checks it, its trusted `input` and `change` after
/// the `click`, and takes the focus; one on its label focuses and checks
/// it too; one on a disabled checkbox, or in a disabled button, has its
/// `mousedown` but no `click`. A text input in a link leaves the click to
/// the link.
#[test]
fn the_users_clicks_check_boxes() {
    let url = page(
        "clicks.html",
        &format!(
            r##"<!DOCTYPE html>{STYLE}
<input type="checkbox" id="a">
<label id="l" for="b">label</label>
<input type="checkbox" id="d" disabled>
<button id="db" disabled><span id="in">in</span></button>
<input type="checkbox" id="b">
<a href="#followed"><input id="t" type="text"></a>
<script>
const log = [];
const note = text => {{ log.push(text); document.title = log.join(' '); }};
for (const id of ['a', 'b', 'd', 'db'])
  for (const type of ['mousedown', 'click', 'input', 'change', 'focus'])
    document.getElementById(id).addEventListener(type, e =>
      note(`${{type}}:${{id}}:${{document.getElementById(id).checked}}:${{e.isTrusted}}`));
onhashchange = () => note(location.hash);
</script>"##
        ),
    );
    let mut input = Vec::new();
    input.extend(click(10.0, 10.0));
    input.extend(click(10.0, 30.0));
    input.extend(click(10.0, 50.0));
    input.extend(click(10.0, 70.0));
    input.extend(click(10.0, 110.0));
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "mousedown:a:false:true focus:a:false:true click:a:true:true input:a:true:true \
             change:a:true:true focus:b:false:true click:b:true:true input:b:true:true \
             change:b:true:true mousedown:d:false:true mousedown:db:undefined:true #followed"
        )
    );
}

/// Tab goes to the controls; Space checks the focused checkbox as it goes
/// up, without scrolling (its `keypress` is the checkbox's); Space and
/// Enter click the focused button, or the button the focused element is
/// in (not a disabled one); Enter doesn't click a checkbox.
#[test]
fn keys_click_the_focused_control() {
    let url = page(
        "keys.html",
        &format!(
            r#"<!DOCTYPE html>{STYLE}
<input type="checkbox" id="c"><button id="b">b</button><input type="checkbox" id="e">
<button id="p"><span id="s" tabindex="0">s</span></button>
<button id="dp" disabled><span id="ds" tabindex="0">ds</span></button>
<div style="height: 2000px"></div>
<script>
const log = [];
const note = text => {{ log.push(text); document.title = log.join(' ') + ' ' + scrollY; }};
for (const id of ['c', 'e'])
  document.getElementById(id).addEventListener('change', e => note(`change:${{id}}:${{e.target.checked}}`));
document.getElementById('b').addEventListener('click', e => note(`click:${{e.isTrusted}}`));
for (const id of ['p', 'dp'])
  document.getElementById(id).addEventListener('click', e => note(`click:${{id}}`));
document.addEventListener('focusin', e => note(`focus:${{e.target.id}}`));
</script>"#
        ),
    );
    let mut input = Vec::new();
    input.extend(press("Tab", "Tab", 9, ""));
    input.extend(press(" ", "Space", 32, " "));
    input.extend(press("Tab", "Tab", 9, ""));
    input.extend(press(" ", "Space", 32, " "));
    input.extend(press("Enter", "Enter", 13, "\r"));
    input.extend(press("Tab", "Tab", 9, ""));
    input.extend(press("Enter", "Enter", 13, "\r"));
    // The button, the element in it, and the one in a disabled button.
    for _ in 0..3 {
        input.extend(press("Tab", "Tab", 9, ""));
        input.extend(press(" ", "Space", 32, " "));
    }
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some(
            "focus:c change:c:true focus:b click:true click:true focus:e focus:p click:p \
             focus:s click:p focus:ds 0"
        )
    );
}

/// A reset button resets its form, by the user's click or by Space, and
/// keeps the click from a link around it; a button that neither submits
/// nor resets (no form, or `type=button`) leaves the click to the link.
#[test]
fn reset_buttons_and_links() {
    let url = page(
        "reset.html",
        &format!(
            r##"<!DOCTYPE html>{STYLE}
<form id="f"><input id="t"><a href="#in-form"><button type="reset" id="r">r</button></a></form>
<a href="#no-form"><button id="n">n</button></a>
<a href="#plain"><button id="p" type="button" form="f">p</button></a>
<button id="k" type="reset" form="f">k</button>
<script>
const log = [];
const note = text => {{ log.push(text); document.title = log.join(' '); }};
const t = document.getElementById('t');
document.getElementById('f').addEventListener('reset', e => note(`reset:${{t.value}}`));
for (const id of ['r', 'n'])
  document.getElementById(id).addEventListener('mousedown', () => {{
    note(`down:${{t.value}}`);
    t.value = 'x';
  }});
onhashchange = () => note(location.hash);
t.value = 'x';
document.getElementById('k').focus();
</script>"##
        ),
    );
    let mut input = Vec::new();
    input.extend(press(" ", "Space", 32, " "));
    input.extend(click(10.0, 30.0));
    input.extend(click(10.0, 50.0));
    input.extend(click(10.0, 70.0));
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some("reset:x down: reset:x down: #no-form #plain")
    );
}

/// The form pseudo-classes are up to date for the user's next click: a
/// click on a label checks the box, `:checked` makes an element below it
/// tall, and the click after hits it.
#[test]
fn pseudo_classes_for_the_next_click() {
    let url = page(
        "pseudo-classes.html",
        &format!(
            r#"<!DOCTYPE html>{STYLE}
<style>#big {{ height: 0 }} input:checked ~ #big {{ height: 100px }}</style>
<label id="l" for="box">label</label><input type="checkbox" id="box"><div id="big"></div>
<script>
document.getElementById('big').addEventListener('click', () => document.title = 'big');
</script>"#
        ),
    );
    let mut input = Vec::new();
    input.extend(click(10.0, 10.0));
    input.extend(click(10.0, 60.0));
    let frame = after(&url, &input);
    assert_eq!(frame.title.as_deref(), Some("big"));
}

/// `select` comes in a task of its own after the script that changed the
/// selection (HTML's element task), trusted and bubbling; one per change,
/// none for a selection set again as it was.
#[test]
fn select_events_come_in_a_task() {
    let url = page(
        "select.html",
        r#"<!DOCTYPE html><input id="i" value="abcdef"><textarea id="t">xyz</textarea>
<script>
const log = [];
document.addEventListener('select', e => {
  log.push(`select:${e.target.id}:${e.bubbles}:${e.isTrusted}`);
  document.title = log.join(' ');
});
const input = document.getElementById('i');
input.setSelectionRange(1, 3);
input.setSelectionRange(1, 3);
document.getElementById('t').select();
log.push('script done');
document.title = log.join(' ');
</script>"#,
    );
    let frame = after(&url, &[]);
    assert_eq!(
        frame.title.as_deref(),
        Some("script done select:i:true:true select:t:true:true")
    );
}

/// A number field's spin button takes the mouse as it is pressed: moved
/// out and released far from it, the `mousemove` and the `mouseup` still
/// go to it (the field, as the page sees it), which commits the change;
/// the next click elsewhere goes where it is again.
#[test]
fn a_spin_button_keeps_the_mouse_while_pressed() {
    let url = page(
        "spin.html",
        &format!(
            r#"<!DOCTYPE html>{STYLE}
<input type="number" id="f"><div id="far" style="height: 100px"></div>
<script>
const log = [];
const note = text => {{ log.push(text); document.title = log.join(' '); }};
const field = document.getElementById('f');
for (const type of ['input', 'change'])
  field.addEventListener(type, () => note(`${{type}}:${{field.value}}`));
document.addEventListener('mouseup', e => note(`mouseup:${{e.target.id}}`));
document.addEventListener('mousemove', e => {{ if (e.buttons) note(`mousemove:${{e.target.id}}`); }});
</script>"#
        ),
    );
    let button = MouseButton::Left;
    let mut input = vec![
        // The spin button is at the field's right end, and takes the
        // pointer once the field is hovered; its upper half.
        InputEvent::MouseMove {
            x: 93.0,
            y: 4.0,
            buttons: 0,
            modifiers: Modifiers::NONE,
        },
        InputEvent::MouseDown {
            x: 93.0,
            y: 4.0,
            button,
            buttons: button.buttons_bit(),
            modifiers: Modifiers::NONE,
            click_count: 1,
        },
        InputEvent::MouseMove {
            x: 150.0,
            y: 80.0,
            buttons: button.buttons_bit(),
            modifiers: Modifiers::NONE,
        },
        InputEvent::MouseUp {
            x: 150.0,
            y: 80.0,
            button,
            buttons: 0,
            modifiers: Modifiers::NONE,
            click_count: 1,
        },
    ];
    input.extend(click(10.0, 60.0));
    let frame = after(&url, &input);
    assert_eq!(
        frame.title.as_deref(),
        Some("input:1 mousemove:f mouseup:f change:1 mouseup:far")
    );
}

/// A control script inserts has its UA shadow tree by the time script asks
/// for its style, though the tree waits for style: its placeholder's (the
/// UA's grey; a control without one gets what it inherits).
#[test]
fn a_controls_style_from_script_takes_its_ua_shadow_tree() {
    let url = page(
        "style-ua-tree.html",
        "<!DOCTYPE html><body><script>
         const field = document.createElement('input');
         field.placeholder = 'Hint';
         document.body.append(field);
         document.title = getComputedStyle(field, '::placeholder').color;
         </script>",
    );
    let frame = after(&url, &[]);
    assert_eq!(frame.title.as_deref(), Some("rgb(117, 117, 117)"));
}

/// A frame shows a control's value with no script or input before it: its
/// UA shadow tree is built for the frame's style.
#[test]
fn a_frame_shows_a_controls_value() {
    let url = page(
        "frame-ua-tree.html",
        "<!DOCTYPE html><style>html, body { margin: 0 }
         input { font: 40px sans-serif; color: black; background: white; border: none;
                 padding: 0; width: 180px; height: 60px; outline: none }</style>
         <input value='MMMM'>",
    );
    let frame = after(&url, &[]);
    let dark = (0..60).any(|y| {
        (0..180).any(|x| {
            let i = ((y * frame.size.width + x) * 4) as usize;
            frame.pixels[i] < 100
        })
    });
    assert!(dark, "the value's text is drawn");
}

/// A frame shows a control's form pseudo-classes in its native look: a
/// checked check box filled with the accent color.
#[test]
fn a_frame_shows_a_check_box_checked() {
    let url = page(
        "checked-box.html",
        "<!DOCTYPE html><style>html, body { margin: 0 } input { margin: 0 }</style>
         <input type=checkbox checked>",
    );
    let frame = after(&url, &[]);
    // Inside its box (on the line's baseline, 3px down), off the tick.
    let i = ((5 * frame.size.width + 3) * 4) as usize;
    assert_eq!(&frame.pixels[i..i + 4], [0, 117, 255, 255]);
}
