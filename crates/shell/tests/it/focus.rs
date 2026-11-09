//! The focus in a whole browser: what can take it once laid out,
//! `autofocus` and the focus fixup at the rendering update, the page's
//! system focus (headless has it), and a child frame's element taking it;
//! an editing host's caret on focus.
//! Each page logs into its title, and lets the capture go (`reftest-wait`)
//! once done.

use std::path::PathBuf;
use std::time::Duration;

use gfx::Size;
use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-focus");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

/// What the page at `url` reports (800×600).
fn report(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

/// What the page at `url` reports on the DOM (`--next-dom`).
fn report_next(url: &str) -> String {
    headless::run_testharness(url, Duration::from_secs(30), Default::default())
        .expect("the page reports")
}

fn title(url: &str) -> String {
    let frame = headless::render(url, Size::new(300, 300), 1.0, true).expect("the page renders");
    frame.title.unwrap_or_default()
}

/// `autofocus` takes the focus at the first rendering update; `focus()`
/// skips an element with no box (`display: none`); an element that loses
/// its box keeps the focus until the next rendering update's fixup, which
/// gives it `blur`. The page has the system's focus (as Chrome's headless
/// mode's).
#[test]
fn the_focus_follows_the_layout() {
    let url = page(
        "layout.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<style>#hidden { display: none }</style>
<div id="a" tabindex="0">a</div><div id="hidden" tabindex="0">h</div>
<div id="auto" tabindex="0" autofocus>auto</div>
<script>
const log = [];
const byId = id => document.getElementById(id);
for (const id of ['a', 'auto'])
  for (const type of ['focus', 'blur'])
    byId(id).addEventListener(type, () => log.push(type + ':' + id));
requestAnimationFrame(() => requestAnimationFrame(() => {
  log.push('active:' + document.activeElement.id, 'has:' + document.hasFocus());
  byId('hidden').focus();
  log.push('after-hidden:' + document.activeElement.id);
  byId('a').focus();
  log.push('active:' + document.activeElement.id);
  byId('a').style.display = 'none';
  log.push('still:' + document.activeElement.id);
  requestAnimationFrame(() => requestAnimationFrame(() => {
    log.push('fixed:' + document.activeElement.tagName);
    document.title = log.join(' ');
    document.documentElement.className = '';
  }));
}));
</script>"#,
    );
    assert_eq!(
        title(&url),
        "focus:auto active:auto has:true after-hidden:auto blur:auto focus:a active:a \
         still:a blur:a fixed:BODY"
    );
}

/// The DOM's `autofocus`: the first candidate that can take the
/// focus takes it at the first rendering update (one without a box is
/// skipped), once a page: one inserted after doesn't.
#[test]
fn autofocus() {
    let url = page(
        "autofocus-next.html",
        r#"<!DOCTYPE html>
<div id="hidden" tabindex="0" autofocus style="display: none">h</div>
<div id="auto" tabindex="0" autofocus>auto</div>
<div id="second" tabindex="0" autofocus>second</div>
<script>
const log = ['before:' + document.activeElement.tagName];
document.getElementById('auto').addEventListener('focus', () => log.push('focus:auto'));
requestAnimationFrame(() => requestAnimationFrame(() => {
  log.push('active:' + document.activeElement.id);
  document.activeElement.blur();
  const late = document.createElement('div');
  late.id = 'late';
  late.tabIndex = 0;
  late.autofocus = true;
  document.body.appendChild(late);
  requestAnimationFrame(() => requestAnimationFrame(() => {
    log.push('after:' + document.activeElement.tagName);
    testRunner.reportHarnessResults(log.join(' '));
  }));
}));
</script>"#,
    );
    assert_eq!(
        report_next(&url),
        "before:BODY focus:auto active:auto after:BODY"
    );
    // Something has the focus first: autofocus is over.
    let url = page(
        "autofocus-focused-next.html",
        r#"<!DOCTYPE html>
<div id="a" tabindex="0">a</div>
<div id="auto" tabindex="0" autofocus>auto</div>
<script>
document.getElementById('a').focus();
requestAnimationFrame(() => requestAnimationFrame(() => {
  testRunner.reportHarnessResults('active:' + document.activeElement.id);
}));
</script>"#,
    );
    assert_eq!(report_next(&url), "active:a");
}

/// An element of a child frame's taking the focus makes its frame the
/// focused one: the parent's focused element gets `blur`, the parent's
/// window `blur` and the child's `focus`; the parent's `activeElement` is
/// the `<iframe>`, and both documents have the focus.
#[test]
fn a_child_frames_element_takes_the_focus() {
    let url = page(
        "frames.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<div id="p" tabindex="0">p</div>
<iframe id="frame" srcdoc="<div id=c tabindex=0>c</div>"></iframe>
<script>
const log = [];
const p = document.getElementById('p');
p.addEventListener('blur', () => log.push('blur:p'));
window.addEventListener('blur', () => log.push('blur:window'));
onload = () => {
  p.focus();
  const child = document.getElementById('frame').contentWindow;
  const c = child.document.getElementById('c');
  child.addEventListener('focus', () => log.push('focus:child-window'));
  c.addEventListener('focus', () => log.push('focus:c'));
  c.focus();
  log.push(
    'active:' + document.activeElement.id,
    'child-active:' + child.document.activeElement.id,
    'has:' + document.hasFocus(),
    'child-has:' + child.document.hasFocus());
  document.title = log.join(' ');
  document.documentElement.className = '';
};
</script>"#,
    );
    assert_eq!(
        title(&url),
        "blur:p blur:window focus:child-window focus:c active:frame child-active:c \
         has:true child-has:true"
    );
}

/// `focus()` scrolls what takes the focus into view as Chrome 152 does
/// (Blink's `ScrollAlignment::CenterIfNeeded`): not at all if it is all in
/// view or covers the view, to the nearer edge if part of it is, centered
/// if none is, on each axis, and in a scroll container too; nothing with
/// `preventScroll`. An editing host or a text control by its caret: at
/// the start (of its first text with a box, not white space that
/// collapsed away), or where its selection was, not its box's edge. Chrome's
/// positions are whole px; machopo's centered ones may not be, so rounded;
/// one by a caret's edge may be 2px off (the caret's font's metrics,
/// which Chrome rounds). The DOM's the same.
#[test]
fn focusing_scrolls_into_view_if_needed() {
    let url = page(
        "scroll.html",
        r#"<!DOCTYPE html>
<style>
html { scrollbar-width: none }
body { margin: 0; width: 3000px; height: 11000px; font: 16px/20px monospace }
.a { position: absolute; width: 100px; height: 40px; margin: 0; border: 0; padding: 0; font: inherit }
#scroller { position: absolute; top: 3000px; left: 0; width: 300px; height: 200px; overflow: auto; scrollbar-width: none }
</style>
<button id="below" class="a" style="top: 2000px; left: 10px"></button>
<button id="partBelow" class="a" style="top: 580px; left: 10px"></button>
<button id="partAbove" class="a" style="top: 300px; left: 10px"></button>
<button id="right" class="a" style="top: 100px; left: 1500px"></button>
<button id="tall" class="a" style="top: 1200px; left: 200px; height: 900px"></button>
<button id="covering" class="a" style="top: 100px; left: 400px; height: 900px"></button>
<div id="scroller"><div style="height: 2000px"></div><button id="inScroller" class="a" style="top: 1000px; left: 0"></button></div>
<div id="editor" class="a" contenteditable style="top: 4000px; left: 0; width: 300px; height: 400px">first<br><br><br><br><br><br><br><br><br><br>end</div>
<div id="tallEditor" class="a" contenteditable style="top: 5000px; left: 0; width: 300px; height: 3000px">abc</div>
<div id="spaced" class="a" contenteditable style="top: 2600px; left: 0; width: 300px; height: auto">
  <div>first</div>
</div>
<textarea id="textarea" class="a" style="top: 8200px; left: 0; width: 300px; height: 400px">abc</textarea>
<input id="input" class="a" style="top: 8700px; left: 0; width: 300px" value="abc">
<textarea id="framed" class="a" style="top: 9800px; left: 0; width: 300px; height: 400px; border: 30px solid; padding: 50px">abc</textarea>
<textarea id="readonly" class="a" readonly style="top: 10500px; left: 0; width: 300px; height: 400px">abc</textarea>
<div id="anonymous" class="a" contenteditable style="top: 6000px; left: 400px; width: 300px; height: auto; border: 30px solid; padding: 50px">text<div>block</div></div>
<div id="caretless" class="a" contenteditable style="top: 6600px; left: 400px; width: 300px; height: 300px"><span></span></div>
<button id="other" class="a" style="top: 0; left: 0"></button>
<script>
function run() {
  const log = [];
  const byId = id => document.getElementById(id);
  const at = () => `${Math.round(scrollX)},${Math.round(scrollY)}`;
  // Focuses `id` from the scroll position `x`, `y` (another element had
  // the focus), and logs where the page then is; within 2px of `nearY`,
  // if given, that (by a caret's edge, where its font's metrics, which
  // Chrome rounds, decide).
  const focus = (name, id, x, y, options, nearY) => {
    byId('other').focus({ preventScroll: true });
    byId('scroller').scrollTop = 0;
    scrollTo(x, y);
    byId(id).focus(options);
    const near = nearY !== undefined && Math.abs(scrollY - nearY) <= 2;
    log.push(`${name} ${near ? `${Math.round(scrollX)},~${nearY}` : at()}`);
  };
  focus('below', 'below', 0, 0);
  focus('partBelow', 'partBelow', 0, 0);
  focus('partAbove', 'partAbove', 0, 320);
  focus('in view', 'partAbove', 0, 0);
  focus('right', 'right', 0, 0);
  focus('tall', 'tall', 0, 0);
  focus('covering', 'covering', 0, 500);
  focus('preventScroll', 'below', 0, 0, { preventScroll: true });
  focus('inScroller', 'inScroller', 0, 0);
  log.push(`scroller ${Math.round(byId('scroller').scrollTop)}`);
  // An editing host: its caret, at its start (the selection was elsewhere).
  getSelection().removeAllRanges();
  focus('editor', 'editor', 0, 0);
  focus('editor caret in view', 'editor', 0, 3700);
  // The selection it had again: its caret at the end.
  const editor = byId('editor');
  getSelection().collapse(editor.lastChild, 3);
  focus('editor at end', 'editor', 0, 0);
  getSelection().removeAllRanges();
  focus('tallEditor partly', 'tallEditor', 0, 5008, undefined, 4999);
  getSelection().removeAllRanges();
  focus('tallEditor below', 'tallEditor', 0, 8500);
  // Its caret at its first text with a box: not the collapsed white
  // space before its first child.
  getSelection().removeAllRanges();
  focus('spaced', 'spaced', 0, 0);
  log.push(`spaced at ${JSON.stringify(getSelection().anchorNode.data)} ${getSelection().anchorOffset}`);
  // A text control: its caret.
  focus('textarea', 'textarea', 0, 0, undefined, 7911);
  focus('textarea caret in view', 'textarea', 0, 7800);
  focus('input', 'input', 0, 0);
  // Its caret past its borders and padding.
  focus('framed', 'framed', 0, 0, undefined, 9590);
  // A read-only one draws no caret: its box.
  focus('readonly', 'readonly', 0, 0);
  // In an anonymous block, around the text before a block child, past
  // the host's borders and padding.
  getSelection().removeAllRanges();
  focus('anonymous', 'anonymous', 0, 0, undefined, 5790);
  // No caret (nothing a caret is drawn at): its box.
  getSelection().removeAllRanges();
  focus('caretless', 'caretless', 0, 0);
  return log.join(' / ');
}
onload = () => testRunner.reportHarnessResults(run());
</script>"#,
    );
    let expected = [
        "below 0,1720",
        "partBelow 0,20",
        "partAbove 0,300",
        "in view 0,0",
        "right 1150,0",
        "tall 0,1350",
        "covering 0,400",
        "preventScroll 0,0",
        "inScroller 0,2800",
        "scroller 920",
        "editor 0,3710",
        "editor caret in view 0,3700",
        "editor at end 0,3910",
        "tallEditor partly 0,~4999",
        "tallEditor below 0,4710",
        "spaced 0,2310",
        "spaced at \"first\" 0",
        "textarea 0,~7911",
        "textarea caret in view 0,7800",
        "input 0,8420",
        "framed 0,~9590",
        "readonly 0,10400",
        "anonymous 0,~5790",
        "caretless 0,6450",
    ];
    assert_eq!(report(&url), expected.join(" / "));
    assert_eq!(report_next(&url), expected.join(" / "), "the DOM");
}

/// An editing host taking the focus with the selection elsewhere puts its
/// caret where Chrome 152 does (Blink's `FirstPositionInOrBeforeNode`,
/// made canonical): before the first character laid out (not white
/// space that collapsed; a line break `white-space` keeps is, alone in its
/// text too), before a
/// `<br>` or an element a caret can't be in (an `<input>`), passing empty
/// elements and comments; at the start of the parent of an element that
/// isn't editable if that comes first, or of the host if nothing does.
#[test]
fn an_editing_hosts_caret_on_focus() {
    let url = page(
        "initial.html",
        r#"<!DOCTYPE html>
<body>
<script>
function run() {
  const cases = [
    ['text', 'abc'],
    ['leading space', '<p> abc</p>'],
    ['white space before child', '\n  <div>first</div>\n'],
    ['empty spans', '<span></span><span> </span><span>abc</span>'],
    ['pre newline', '<p style="white-space: pre">\nabc</p>'],
    ['pre newline alone', '<p style="white-space: pre">\n</p><p>abc</p>'],
    ['hidden pre newline', '<p style="white-space: pre; visibility: hidden">\n</p><p>abc</p>'],
    ['br', '<p></p><p><br></p>'],
    ['input', '<span></span><input><span>abc</span>'],
    ['not editable', '<span contenteditable="false">abc</span>def'],
    ['comment', '<p><!-- c --></p><p>abc</p>'],
    ['not editable in a block', '<div><span></span><span contenteditable="false">abc</span>def</div>'],
    ['nothing', '<span></span>'],
  ];
  const log = [];
  for (const [name, html] of cases) {
    const host = document.createElement('div');
    host.contentEditable = 'true';
    host.innerHTML = html;
    document.body.append(host);
    getSelection().removeAllRanges();
    host.focus();
    const s = getSelection();
    const node = s.anchorNode;
    const where = node === host ? 'host'
      : node.nodeType === Node.TEXT_NODE ? JSON.stringify(node.data)
      : node.localName;
    log.push(`${name}: ${where} ${s.anchorOffset} ${s.isCollapsed}`);
    host.blur();
    host.remove();
  }
  return log.join(' / ');
}
onload = () => testRunner.reportHarnessResults(run());
</script>"#,
    );
    let expected = [
        r#"text: "abc" 0 true"#,
        r#"leading space: " abc" 1 true"#,
        r#"white space before child: "first" 0 true"#,
        r#"empty spans: "abc" 0 true"#,
        r#"pre newline: "\nabc" 0 true"#,
        r#"pre newline alone: "\n" 0 true"#,
        // As a hidden text of characters: its block, with height.
        "hidden pre newline: p 0 true",
        "br: p 0 true",
        "input: host 1 true",
        "not editable: host 0 true",
        r#"comment: "abc" 0 true"#,
        "not editable in a block: div 0 true",
        "nothing: host 0 true",
    ];
    assert_eq!(report(&url), expected.join(" / "));
    assert_eq!(report_next(&url), expected.join(" / "), "the DOM");
}

/// On the DOM, as Blink's candidates have it beyond the cases above
/// (`IsVisuallyEquivalentCandidate`, read from Blink rather than measured
/// against Chrome): the start of a box with height and nothing laid out in
/// it, before a table (not in its cells), past text that isn't visible,
/// a wrapped text's first line; the selection already in the host stays.
#[test]
fn an_editing_hosts_caret_on_focus_by_blinks_candidates() {
    let url = page(
        "initial-candidates.html",
        r#"<!DOCTYPE html>
<body>
<script>
function run() {
  const cases = [
    ['empty block with height', '<div id=x style="height: 10px"></div>abc'],
    ['empty block', '<div></div>abc'],
    ['table', '<span></span><table><tr><td>abc</td></tr></table>'],
    ['hidden text', '<span style="visibility: hidden">abc</span>def'],
    ['wrapped', '<p style="width: 0">abc def</p>'],
    ['br after an empty span', '<p id=x><span></span><br></p>'],
  ];
  const log = [];
  for (const [name, html] of cases) {
    const host = document.createElement('div');
    host.contentEditable = 'true';
    host.innerHTML = html;
    document.body.append(host);
    getSelection().removeAllRanges();
    host.focus();
    const s = getSelection();
    const node = s.anchorNode;
    const where = node === host ? 'host'
      : node.nodeType === Node.TEXT_NODE ? JSON.stringify(node.data)
      : node.id || node.localName;
    log.push(`${name}: ${where} ${s.anchorOffset}`);
    host.blur();
    host.remove();
  }
  const host = document.createElement('div');
  host.contentEditable = 'true';
  host.innerHTML = 'abc';
  document.body.append(host);
  // In it by the range's members, which give no focus.
  getSelection().collapse(document.body, 0);
  getSelection().getRangeAt(0).setStart(host.firstChild, 2);
  log.push(`before: ${document.activeElement === host}`);
  host.focus();
  log.push(`kept: ${getSelection().anchorOffset}`);
  return log.join(' / ');
}
onload = () => testRunner.reportHarnessResults(run());
</script>"#,
    );
    let expected = [
        "empty block with height: x 0",
        r#"empty block: "abc" 0"#,
        "table: host 1",
        r#"hidden text: "def" 0"#,
        r#"wrapped: "abc def" 0"#,
        "br after an empty span: x 1",
        "before: false",
        "kept: 2",
    ];
    assert_eq!(report_next(&url), expected.join(" / "));
}
