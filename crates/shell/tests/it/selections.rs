//! The selection in a whole browser: `selectionchange` as a task of its
//! own, once until it runs, for what changes what script sees of the
//! selection (character data moving it too, insertions and removals not);
//! an editing host taking the focus, and the selection with it; `select()`
//! and the Selection API giving the focus (Chrome 152's). Each page logs
//! into its title, and lets the capture go (`reftest-wait`) once done.

use std::path::PathBuf;

use gfx::Size;
use shell::headless;

/// A page with `html` in this test's scratch directory; its `file:` URL.
fn page(name: &str, html: &str) -> String {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("shell-selections");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, html).unwrap();
    net::file_url(&path).expect("an absolute path")
}

fn title(url: &str) -> String {
    let frame = headless::render(url, Size::new(300, 300), 1.0, true).expect("the page renders");
    frame.title.unwrap_or_default()
}

/// `selectionchange` fires at the document in a task after the change,
/// not bubbling, once for changes in a row; setting the selection as it
/// was doesn't schedule it, nor does an insertion or a removal moving it;
/// changing the character data it is in, splitting and normalizing do,
/// as script setting its range does (Chrome 152's).
#[test]
fn selectionchange_is_a_task() {
    let url = page(
        "selectionchange.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<div id="a">hello <b>world</b></div>
<script>
const log = [];
let count = 0;
document.addEventListener('selectionchange', e =>
  count++ || log.push(`event:${e.target === document}:${e.bubbles}:${e.cancelable}:${e.isTrusted}`));
window.addEventListener('selectionchange', () => log.push('bubbled'));
const sel = getSelection(), a = document.getElementById('a'), t = a.firstChild;
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
async function step(name, change) {
  const before = count;
  change();
  const now = count;
  await tick();
  log.push(`${name}:${now - before}${count - before}`);
}
(async () => {
  await step('collapse', () => sel.collapse(t, 1));
  await step('twice', () => { sel.collapse(t, 2); sel.collapse(t, 3); });
  await step('same', () => sel.collapse(t, 3));
  await step('range', () => sel.getRangeAt(0).setEnd(t, 4));
  await step('insert', () => t.insertData(0, 'xx'));
  await step('after', () => t.appendData('zz'));
  await step('split', () => t.splitText(1));
  await step('normalize', () => a.normalize());
  sel.collapse(a, 1);
  await tick();
  const other = document.createRange(), world = a.lastChild.firstChild;
  other.setStart(world, 3);
  await step('other range', () => world.insertData(0, 'q'));
  await step('child', () => a.insertBefore(document.createElement('i'), a.firstChild));
  await step('removal', () => a.firstChild.remove());
  await step('empty', () => sel.removeAllRanges());
  await step('empty again', () => sel.empty());
  document.title = log.join(' ');
  document.documentElement.className = '';
})();
</script>"#,
    );
    assert_eq!(
        title(&url),
        "event:true:false:false:true collapse:01 twice:01 same:00 range:01 insert:01 after:00 \
         split:01 normalize:01 other range:00 child:00 removal:00 empty:01 empty again:00"
    );
}

/// An editing host takes the focus without a `tabindex` (its `tabIndex`
/// still -1); one inside another, or `contenteditable=false`, doesn't.
/// Taking it, the selection goes to its first text before its `focus`
/// listeners run (and `selectionchange` follows), unless the selection is
/// already in it; another element taking it leaves the selection.
#[test]
fn an_editing_host_takes_the_focus() {
    let url = page(
        "editing-host.html",
        r#"<!DOCTYPE html><html class="reftest-wait"><title></title>
<div id="out">out</div>
<div id="e" contenteditable><p>edit <b id="inner" contenteditable>me</b></p></div>
<div id="off" contenteditable="false">off</div>
<div id="plain" tabindex="0">plain</div>
<script>
const log = [];
const byId = id => document.getElementById(id);
const sel = getSelection(), e = byId('e');
const where = () => sel.rangeCount
  ? `${sel.anchorNode.nodeName}:${sel.anchorNode.textContent}:${sel.anchorOffset}` : 'none';
let changes = 0;
document.addEventListener('selectionchange', () => changes++);
e.addEventListener('focus', () => log.push('focus:' + where()));
requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(() => {
  log.push('tabIndex:' + e.tabIndex);
  sel.collapse(byId('out').firstChild, 1);
  e.focus();
  log.push('active:' + document.activeElement.id, 'after:' + where());
  setTimeout(() => {
    log.push('changes:' + changes);
    e.blur();
    sel.collapse(e.querySelector('b').firstChild, 1);
    e.focus();
    log.push('inside:' + where());
    byId('inner').focus();
    log.push('inner:' + document.activeElement.id);
    e.blur();
    byId('off').focus();
    log.push('off:' + document.activeElement.tagName);
    sel.collapse(byId('out').firstChild, 2);
    byId('plain').focus();
    log.push('plain:' + document.activeElement.id + ':' + where());
    document.querySelector('title').textContent = log.join(' ');
    document.documentElement.className = '';
  }, 0);
}, 0)));
</script>"#,
    );
    assert_eq!(
        title(&url),
        "tabIndex:-1 focus:#text:edit :0 active:e after:#text:edit :0 changes:1 \
         focus:#text:me:1 inside:#text:me:1 inner:e off:BODY \
         plain:plain:#text:out:2"
    );
}

/// `select()` selects the value and gives the control the focus, at once
/// (its `focus` before `select()` returns, keeping the selection), unless
/// it has it: any `<input>` that can take the focus (readonly, a number, a
/// checkbox), not a disabled or unrendered one; `setSelectionRange()`
/// doesn't (Chrome 152's).
#[test]
fn select_gives_the_focus() {
    let url = page(
        "select-focus.html",
        r#"<!DOCTYPE html><html class="reftest-wait">
<input id="i" value="abc"><input id="dis" disabled value="abc"><input id="ro" readonly value="abc"><input id="num" type="number" value="12"><textarea id="t">abc</textarea><input id="none" style="display: none" value="abc"><input id="box" type="checkbox"><button id="btn">b</button>
<script>
const $ = id => document.getElementById(id), log = [];
for (const id of ['i', 't', 'ro', 'num']) $(id).addEventListener('focus', () => log.push('focus:' + id));
$('i').addEventListener('select', () => log.push('select:i'));
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
async function step(name, change) {
  $('btn').focus();
  log.length = 0;
  change();
  const now = `${document.activeElement.id}[${log.join(',')}]`;
  await tick();
  return `${name}=${now}`;
}
(async () => {
  const out = [];
  out.push(await step('i', () => $('i').select()));
  out.push(await step('again', () => { $('i').select(); log.length = 0; $('i').select(); }));
  out.push(await step('range', () => { $('i').setSelectionRange(1, 2); $('i').select(); log.push($('i').selectionStart + '-' + $('i').selectionEnd); }));
  for (const id of ['dis', 'ro', 'num', 't', 'none', 'box']) out.push(await step(id, () => $(id).select()));
  out.push(await step('setSelectionRange', () => $('i').setSelectionRange(0, 1)));
  document.title = out.join(' ');
  document.documentElement.className = '';
})();
</script>
"#,
    );
    assert_eq!(
        title(&url),
        "i=i[focus:i] again=i[] range=i[focus:i,0-3] dis=btn[] ro=ro[focus:ro] \
         num=num[focus:num] t=t[focus:t] none=btn[] box=box[] setSelectionRange=btn[]"
    );
}

/// A Selection API method that puts the selection in an editing host gives
/// it the focus, at once: its anchor's host (the innermost, a nested one
/// inside something not editable), or its focus's if it is backward; none
/// for a selection set as it was, in something not editable inside a host, nor through a
/// `Range` method on the selection's range, nor when it leaves a host or
/// empties. The selection stays as it was set (Chrome 152's).
#[test]
fn the_selection_api_focuses_an_editing_host() {
    let url = page(
        "selection-focus.html",
        r#"<!DOCTYPE html><html class="reftest-wait"><title></title>
<p id="p0">before</p><div id="e" contenteditable>abc <b id="eb">bold</b></div><div id="e2" contenteditable>second</div><p id="p">xyz</p><button id="btn">b</button><div id="f" contenteditable><span contenteditable="false" id="off">off <i id="deep" contenteditable>deep</i></span> tail</div><div tabindex="0"><span id="inner" contenteditable>inner</span></div>
<script>
const $ = id => document.getElementById(id), sel = getSelection(), log = [];
for (const id of ['e', 'e2', 'deep', 'inner']) $(id).addEventListener('focus', () => log.push('focus:' + id));
const tick = () => new Promise(resolve => setTimeout(resolve, 0));
const where = () => sel.anchorNode.parentNode.id + ':' + sel.anchorOffset + '>' + sel.focusNode.parentNode.id + ':' + sel.focusOffset;
async function step(name, change, focused = 'btn') {
  $(focused).focus();
  log.length = 0;
  change();
  const now = `${document.activeElement.id || document.activeElement.tagName}[${log.join(',')}]`;
  await tick();
  return `${name}=${now}`;
}
(async () => {
  const out = [];
  out.push(await step('into', () => sel.selectAllChildren($('eb'))));
  out.push(await step('collapse', () => sel.collapse($('e').firstChild, 1)));
  out.push(await step('anchor', () => sel.setBaseAndExtent($('e').firstChild, 0, $('e2').firstChild, 1)));
  out.push(await step('anchor back', () => sel.setBaseAndExtent($('e2').firstChild, 1, $('e').firstChild, 0)));
  out.push(await step('focus back', () => { sel.setBaseAndExtent($('p').firstChild, 1, $('e').firstChild, 1); log.push(where()); }));
  out.push(await step('extend', () => { sel.collapse($('p0').firstChild, 0); sel.extend($('e').firstChild, 1); }));
  out.push(await step('extend in', () => { sel.collapse($('e').firstChild, 0); $('btn').focus(); log.length = 0; sel.extend($('e').firstChild, 2); }));
  out.push(await step('unchanged', () => { sel.collapse($('e').firstChild, 1); $('btn').focus(); log.length = 0; sel.collapse($('e').firstChild, 1); }));
  out.push(await step('over', () => sel.setBaseAndExtent($('p0').firstChild, 0, $('p').firstChild, 1)));
  out.push(await step('off', () => sel.selectAllChildren($('off'))));
  out.push(await step('deep', () => sel.selectAllChildren($('deep'))));
  out.push(await step('nested', () => sel.selectAllChildren($('inner'))));
  out.push(await step('host', () => sel.collapse($('e2'), 0)));
  out.push(await step('addRange', () => { sel.removeAllRanges(); const r = document.createRange(); r.selectNodeContents($('e')); sel.addRange(r); }));
  out.push(await step('toStart', () => { sel.setBaseAndExtent($('e').firstChild, 0, $('p').firstChild, 1); $('btn').focus(); log.length = 0; sel.collapseToStart(); }));
  out.push(await step('range', () => { sel.selectAllChildren($('p')); sel.getRangeAt(0).selectNodeContents($('e')); }));
  out.push(await step('out', () => sel.selectAllChildren($('p')), 'e'));
  out.push(await step('empty', () => sel.removeAllRanges(), 'e'));
  out.push(await step('same', () => sel.selectAllChildren($('e')), 'e'));
  document.querySelector('title').textContent = out.join(' ');
  document.documentElement.className = '';
})();
</script>
"#,
    );
    assert_eq!(
        title(&url),
        "into=e[focus:e] collapse=e[focus:e] anchor=e[focus:e] anchor back=e2[focus:e2] \
         focus back=e[focus:e,p:1>e:1] extend=btn[] extend in=e[focus:e] unchanged=btn[] over=btn[] off=btn[] deep=deep[focus:deep] \
         nested=inner[focus:inner] host=e2[focus:e2] addRange=e[focus:e] toStart=e[focus:e] \
         range=btn[] out=e[] empty=e[] same=e[]"
    );
}

/// Giving the focus from the selection or `select()` doesn't scroll, nor
/// does it reach an editing host that can't take the focus (not rendered)
/// or one in a frame without the focus (Chrome 152's).
#[test]
fn what_the_selection_doesnt_focus() {
    let url = page(
        "selection-no-focus.html",
        r#"<!DOCTYPE html><html class="reftest-wait"><title></title>
<button id="btn">b</button><div id="hid" contenteditable style="display: none">hidden</div>
<div tabindex="-1" id="outer"><div id="cut" contenteditable style="visibility: hidden">cut</div></div>
<div id="box" style="height: 30px; overflow: auto"><div style="height: 100px"></div><input id="deep" value="abc"></div>
<iframe id="f" srcdoc="<div id=e contenteditable>frame</div>"></iframe>
<script>
const $ = id => document.getElementById(id), sel = getSelection();
onload = () => setTimeout(() => {
  const out = [];
  $('btn').focus();
  sel.selectAllChildren($('hid'));
  out.push('hidden=' + (document.activeElement.id || document.activeElement.tagName));
  $('btn').focus();
  sel.selectAllChildren($('cut'));
  out.push('ancestor=' + (document.activeElement.id || document.activeElement.tagName));
  $('btn').focus();
  $('deep').select();
  out.push('scroll=' + document.activeElement.id + ':' + $('box').scrollTop);
  $('btn').focus();
  const doc = $('f').contentDocument;
  doc.getSelection().selectAllChildren(doc.getElementById('e'));
  out.push('frame=' + (document.activeElement.id || document.activeElement.tagName) + ':' + (doc.activeElement.id || doc.activeElement.tagName));
  document.querySelector('title').textContent = out.join(' ');
  document.documentElement.className = '';
}, 0);
</script>
"#,
    );
    assert_eq!(
        title(&url),
        "hidden=btn ancestor=btn scroll=deep:0 frame=btn:BODY"
    );
}
