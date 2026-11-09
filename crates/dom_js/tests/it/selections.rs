//! `Selection` from script: `getSelection()`, the document's one
//! selection; each method's checks, the new range it makes and the
//! direction it sets (Chrome's where it differs from the specification);
//! `addRange` and `removeRange` by reference; the selection's range live,
//! script setting it setting the selection; `containsNode`,
//! `deleteFromDocument`, `toString`; a selection in a shadow tree;
//! `selectionchange` at the document and at text controls.

use crate::env::Env;

/// A page whose script has `root`: `<div>` with `<p id=a>one</p>`,
/// `<p id=b><i id=c></i></p>`; `sel`, the selection; `at(range)`, its
/// boundary points as "node offset node offset" (an element by its id,
/// else its `nodeName`); `state()`, the selection's anchor and focus the
/// same way, its direction and type; `error(f)`, the name of what `f`
/// throws; and `log`.
fn page() -> Env {
    let mut env = Env::window("https://example.test/a/page.html");
    env.eval(
        "var html = document.appendChild(document.createElement('html')); \
         html.appendChild(document.createElement('body')); \
         globalThis.log = []; \
         globalThis.root = document.createElement('div'); \
         root.innerHTML = '<p id=a>one</p><p id=b><i id=c></i></p>'; \
         document.body.append(root); \
         globalThis.name = n => n.id || n.nodeName; \
         globalThis.at = r => `${name(r.startContainer)} ${r.startOffset} ${name(r.endContainer)} ${r.endOffset}`; \
         globalThis.error = f => { try { f(); return 'none'; } catch (e) { return e.name; } }; \
         globalThis.sel = document.getSelection(); \
         globalThis.state = () => sel.rangeCount \
             ? `${name(sel.anchorNode)} ${sel.anchorOffset} ${name(sel.focusNode)} ${sel.focusOffset} ${sel.direction} ${sel.type}` \
             : `empty ${sel.direction} ${sel.type}`; 0",
    );
    env
}

/// Runs `source`, then gives `log` joined with ` / ` (and empties it).
fn run(env: &mut Env, source: &str) -> String {
    env.eval(&format!(
        "{{ {source} }} var got = log.join(' / '); log = []; got"
    ))
}

/// `getSelection()` is the document's, the window's the same object; it
/// starts empty. A document without a window has none.
#[test]
fn a_document_has_one_selection() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "log.push(sel instanceof Selection, sel === document.getSelection(), sel === window.getSelection(), state(), sel.rangeCount, sel.isCollapsed, sel.anchorNode, sel.anchorOffset, sel.focusNode, sel.focusOffset, JSON.stringify(sel.toString()), error(() => sel.getRangeAt(0)), document.implementation.createHTMLDocument('').getSelection() === null, error(() => new Selection()));",
        ),
        "true / true / true / empty none None / 0 / true /  / 0 /  / 0 / \"\" / \
         IndexSizeError / true / TypeError"
    );
}

/// `collapse`, `extend`, `setBaseAndExtent`, `selectAllChildren`,
/// `collapseToStart` and `collapseToEnd` each make a new range;
/// `extend` and `setBaseAndExtent` keep which end is the anchor, as their
/// direction says. A collapsed selection has none (Chrome's).
#[test]
fn methods_set_a_new_range_and_a_direction() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "const a = root.querySelector('#a'), b = root.querySelector('#b'); const t = a.firstChild; sel.collapse(t, 1); const first = sel.getRangeAt(0); log.push(state(), sel.isCollapsed, at(first)); sel.extend(b, 1); log.push(state(), sel.getRangeAt(0) !== first, at(sel.getRangeAt(0))); sel.extend(t, 0); log.push(state(), at(sel.getRangeAt(0))); sel.setBaseAndExtent(b, 1, t, 2); log.push(state(), at(sel.getRangeAt(0))); sel.collapseToEnd(); log.push(state()); sel.setBaseAndExtent(b, 1, t, 2); sel.collapseToStart(); log.push(state()); sel.selectAllChildren(b); log.push(state()); sel.setPosition(b, 0); log.push(state(), sel.empty === sel.removeAllRanges); sel.setBaseAndExtent(t, 1, t, 1); log.push(state()); sel.collapse(null); log.push(state()); sel.collapse(t, 2); sel.empty(); log.push(state());",
        ),
        "#text 1 #text 1 none Caret / true / #text 1 #text 1 / \
         #text 1 b 1 forward Range / true / #text 1 b 1 / \
         #text 1 #text 0 backward Range / #text 0 #text 1 / \
         b 1 #text 2 backward Range / #text 2 b 1 / \
         b 1 b 1 none Caret / #text 2 #text 2 none Caret / \
         b 0 b 1 forward Range / b 0 b 0 none Caret / false / \
         #text 1 #text 1 none Caret / empty none None / empty none None"
    );
}

/// The checks, in WPT's order: a node outside the document changes
/// nothing (before `extend`'s empty selection, an `InvalidStateError`); a
/// doctype is an `InvalidNodeTypeError`, an offset past the node an
/// `IndexSizeError`; `collapseToStart` of an empty selection an
/// `InvalidStateError`; `getRangeAt` past its range an `IndexSizeError`.
#[test]
fn methods_check_their_points() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "const a = root.querySelector('#a'), t = a.firstChild; const doctype = document.implementation.createDocumentType('html', '', ''); document.insertBefore(doctype, document.documentElement); const detached = document.createElement('p'); log.push(error(() => sel.extend(t, 0)), error(() => sel.extend(detached, 0)), error(() => sel.collapseToStart()), error(() => sel.collapseToEnd())); sel.collapse(t, 1); log.push(error(() => sel.collapse(doctype, 0)), error(() => sel.collapse(t, 4)), error(() => sel.extend(doctype, 0)), error(() => sel.extend(t, 4)), error(() => sel.setBaseAndExtent(t, 0, t, 4)), error(() => sel.setBaseAndExtent(detached, 4, t, 0)), error(() => sel.selectAllChildren(doctype)), error(() => sel.getRangeAt(1)), state()); sel.collapse(detached, 0); sel.extend(detached, 0); sel.setBaseAndExtent(t, 0, detached, 0); sel.selectAllChildren(detached); log.push(state(), error(() => sel.collapse(detached, 1)));",
        ),
        "InvalidStateError / none / InvalidStateError / InvalidStateError / \
         InvalidNodeTypeError / IndexSizeError / InvalidNodeTypeError / IndexSizeError / \
         IndexSizeError / IndexSizeError / InvalidNodeTypeError / IndexSizeError / \
         #text 1 #text 1 none Caret / #text 1 #text 1 none Caret / IndexSizeError"
    );
}

/// `addRange` takes the range itself, forward, only into an empty
/// selection and only from its document (one from elsewhere isn't its,
/// moved in after); `removeRange` takes its own
/// range away, any other is a `NotFoundError`.
#[test]
fn ranges_are_added_by_reference() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "const a = root.querySelector('#a'), b = root.querySelector('#b'); const r = document.createRange(); r.setStart(a.firstChild, 0); r.setEnd(b, 1); sel.addRange(r); log.push(sel.getRangeAt(0) === r, state()); const other = document.createRange(); sel.addRange(other); log.push(sel.getRangeAt(0) === r, error(() => sel.removeRange(other))); sel.removeRange(r); log.push(state()); const detached = document.createRange(); detached.selectNodeContents(document.createElement('p')); sel.addRange(detached); log.push(state()); detached.selectNodeContents(a); log.push(state()); sel.setBaseAndExtent(b, 1, a, 0); sel.removeAllRanges(); sel.addRange(r); log.push(state());",
        ),
        "true / #text 0 b 1 forward Range / true / NotFoundError / empty none None / \
         empty none None / empty none None / #text 0 b 1 forward Range"
    );
}

/// Script setting the selection's range sets the selection, forward
/// (Chrome's); the tree's changes move it as any live range. Set out of
/// the document, the selection is empty, and stays so when the range
/// comes back; a range the selection let go is script's alone.
#[test]
fn the_selections_range_is_live() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "const a = root.querySelector('#a'), b = root.querySelector('#b'); const t = a.firstChild; sel.setBaseAndExtent(b, 1, t, 1); const r = sel.getRangeAt(0); r.setStart(t, 0); log.push(state()); t.insertData(0, 'xx'); log.push(state()); a.remove(); log.push(state()); root.prepend(a); r.setStart(document.createElement('p'), 0); log.push(state(), at(r)); r.setStart(t, 0); log.push(state()); sel.collapse(t, 1); const kept = sel.getRangeAt(0); sel.collapse(t, 2); kept.setStart(t, 0); log.push(state());",
        ),
        "#text 0 b 1 forward Range / #text 0 b 1 forward Range / DIV 0 b 1 forward Range / \
         empty none None / P 0 P 0 / empty none None / #text 2 #text 2 none Caret"
    );
}

/// `containsNode` as Chrome's: a node from before it to after it in its
/// parent, partly with `allowPartialContainment`, and a text node partly
/// in it always; `deleteFromDocument` deletes the range's contents;
/// `toString` is the range's text.
#[test]
fn contents_of_the_selection() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "const a = root.querySelector('#a'), b = root.querySelector('#b'); const t = a.firstChild; log.push(sel.containsNode(a)); sel.selectAllChildren(root); log.push(sel.containsNode(a), sel.containsNode(root), sel.containsNode(root, true), sel.containsNode(document.createElement('p'), true), sel.containsNode(document.createElement('p').appendChild(document.createElement('i')), true), sel.containsNode(document), JSON.stringify(sel.toString())); sel.collapse(t, 2); log.push(sel.containsNode(t), sel.containsNode(a), sel.containsNode(a, true), sel.containsNode(b, true), error(() => sel.containsNode(null))); sel.setBaseAndExtent(t, 1, b, 0); log.push(JSON.stringify(sel.toString())); sel.deleteFromDocument(); log.push(root.innerHTML, state()); sel.removeAllRanges(); sel.deleteFromDocument(); log.push(root.innerHTML);",
        ),
        "false / true / false / true / false / false / false / \"one\" / true / false / true / false / \
         TypeError / \"ne\" / <p id=\"a\">o</p><p id=\"b\"><i id=\"c\"></i></p> / \
         DIV 1 DIV 1 none Caret / <p id=\"a\">o</p><p id=\"b\"><i id=\"c\"></i></p>"
    );
}

/// A shadow tree of the document is in it: the selection goes there, and
/// its anchor is the node in the shadow tree (not the host). Its host
/// removed, the selection's range stays in the shadow tree, out of the
/// document: script sees the selection empty.
#[test]
fn a_selection_in_a_shadow_tree() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "const host = root.querySelector('#b'); const shadow = host.attachShadow({ mode: 'open' }); shadow.innerHTML = '<b id=s>in</b>'; const s = shadow.getElementById('s'); sel.collapse(s.firstChild, 1); log.push(state(), sel.anchorNode === s.firstChild); sel.extend(root, 0); log.push(state()); sel.collapse(s.firstChild, 1); const range = sel.getRangeAt(0); host.remove(); log.push(state(), sel.rangeCount, sel.anchorNode, sel.containsNode(s, true), error(() => sel.getRangeAt(0)), range.startContainer === s.firstChild); sel.addRange(document.createRange()); log.push(sel.rangeCount);",
        ),
        "#text 1 #text 1 none Caret / true / DIV 0 DIV 0 none Caret / empty none None / 0 /  / \
         false / IndexSizeError / true / 1"
    );
}

/// A change script can see schedules one `selectionchange` at the
/// document, a task: not before it runs, once however many changes come
/// first, again after; not for a change to the same selection. It
/// doesn't bubble. A character data change moving the selection's range
/// schedules one; an insertion doesn't (Chrome's).
#[test]
fn selectionchange_is_a_task_at_the_document() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "var t = root.querySelector('#a').firstChild; \
             document.addEventListener('selectionchange', (e) => log.push(e.target === document, e.bubbles, e.cancelable, e.isTrusted)); \
             window.addEventListener('selectionchange', () => log.push('window')); \
             sel.collapse(t, 1); sel.collapse(t, 2); log.push('sync');",
        ),
        "sync"
    );
    assert_eq!(env.run_element_tasks(), 1);
    assert_eq!(run(&mut env, ""), "true / false / false / true");
    // The same again: nothing; another change: one more.
    run(&mut env, "sel.collapse(t, 2);");
    assert_eq!(env.run_element_tasks(), 0);
    run(&mut env, "sel.extend(t, 0);");
    assert_eq!(env.run_element_tasks(), 1);
    run(&mut env, "log = [];");
    // Its data moving it: one; inserting before it: none.
    run(&mut env, "t.insertData(0, 'xx');");
    assert_eq!(env.run_element_tasks(), 1);
    run(&mut env, "root.prepend(document.createElement('hr'));");
    assert_eq!(env.run_element_tasks(), 0);
    // Another text's data: none; splitting or joining its text: one.
    run(
        &mut env,
        "root.querySelector('#b').append('far'); root.querySelector('#b').lastChild.insertData(0, 'x');",
    );
    assert_eq!(env.run_element_tasks(), 0);
    run(&mut env, "sel.collapse(t, 3);");
    env.run_element_tasks();
    run(&mut env, "t.splitText(1);");
    assert_eq!(env.run_element_tasks(), 1);
    run(&mut env, "t.parentNode.normalize();");
    assert_eq!(env.run_element_tasks(), 1);
    run(&mut env, "sel.collapse(t, 2);");
    env.run_element_tasks();
    run(&mut env, "log = [];");
    // Script setting its range: one.
    run(&mut env, "sel.getRangeAt(0).setStart(t, 1);");
    assert_eq!(env.run_element_tasks(), 1);
    assert_eq!(
        run(&mut env, "log.push(state());"),
        "true / false / false / true / #text 1 #text 2 forward Range"
    );
}

/// A text control's selection changing schedules one `selectionchange`
/// at it, which bubbles (Chrome's, HTML's): `setSelectionRange`,
/// `select()`, `setRangeText`, a value set moving the cursor; not a
/// selection set as it was. The document's selection gets none.
#[test]
fn selectionchange_at_a_text_control() {
    let mut env = page();
    run(
        &mut env,
        "var input = document.body.appendChild(document.createElement('input')); input.value = 'hello'; \
         var area = document.body.appendChild(document.createElement('textarea')); area.value = 'abc'; \
         document.addEventListener('selectionchange', (e) => log.push(e.target.nodeName, e.bubbles));",
    );
    env.run_element_tasks();
    run(&mut env, "log = [];");
    // Each change's `select` too.
    run(
        &mut env,
        "input.setSelectionRange(1, 3); input.setSelectionRange(2, 3);",
    );
    assert_eq!(env.run_element_tasks(), 3);
    assert_eq!(run(&mut env, ""), "INPUT / true");
    run(&mut env, "input.setSelectionRange(2, 3);");
    assert_eq!(env.run_element_tasks(), 0);
    run(
        &mut env,
        "area.select(); input.setRangeText('X', 0, 1, 'select');",
    );
    env.run_element_tasks();
    assert_eq!(run(&mut env, ""), "TEXTAREA / true / INPUT / true");
    run(&mut env, "input.value = 'changed'; area.value = 'abc';");
    env.run_element_tasks();
    assert_eq!(run(&mut env, ""), "INPUT / true");
    // A new value of the same length: the cursor at its end already.
    run(&mut env, "input.value = 'CHANGED';");
    assert_eq!(env.run_element_tasks(), 0);
}

/// `selectAllChildren` of a text: its children, none (not its data).
#[test]
fn all_of_a_texts_children_are_none() {
    let mut env = page();
    assert_eq!(
        run(
            &mut env,
            "var t = root.querySelector('#a').firstChild; sel.selectAllChildren(t); log.push(state());"
        ),
        "#text 0 #text 0 none Caret"
    );
}
