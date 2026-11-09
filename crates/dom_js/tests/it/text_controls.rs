//! The text controls from script: a textarea's `value` (its child text
//! content until dirty, its newlines normalized), `defaultValue`, `type`,
//! `textLength`, its reset and its copy's; the selection API of a textarea
//! and the text inputs (`selectionStart`, `selectionEnd`,
//! `selectionDirection`, `setSelectionRange()`, `setRangeText()`,
//! `select()`), the cursor a changed value leaves, a type change's; the
//! `select` events the selection's changes queue.

use crate::env::Env;

/// A window whose document has an `<html>` and a `<body>`, and an
/// `error(f)` giving the name of what `f` throws.
fn window() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); \
         function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } \
         function input(type) { const i = document.createElement('input'); i.type = type; return i; } \
         function sel(e) { return [e.selectionStart, e.selectionEnd, e.selectionDirection].join(); } 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn a_textareas_value() {
    let mut env = window();
    check(
        &mut env,
        &[
            // Its child text content (its text children's only) until set.
            (
                "var t = document.createElement('textarea'); t.append('a', document.createElement('b'), 'c'); \
                 t.lastChild.before(document.createComment('x')); t.children[0].append('no'); [t.value, t.defaultValue].join()",
                "ac,ac",
            ),
            ("t.firstChild.data = 'z'; t.value", "zc"),
            ("[t.type, t.textLength].join()", "textarea,2"),
            // Newlines normalized, as read; its length is the API value's.
            (
                "t.defaultValue = 'a\\r\\nb\\rc'; [JSON.stringify(t.value), t.textLength, t.childNodes.length].join()",
                "\"a\\nb\\nc\",5,1",
            ),
            ("t.textContent = ''; JSON.stringify(t.value)", "\"\""),
            // Set: dirty, its children no longer it.
            (
                "t.value = 'set'; t.defaultValue = 'other'; [t.value, t.defaultValue].join()",
                "set,other",
            ),
            ("t.value = null; JSON.stringify(t.value)", "\"\""),
            ("t.value = '\\u{1F600}'; t.textLength", "2"),
            // A copy has the value, dirty.
            (
                "t.value = 'v'; var c = t.cloneNode(); c.defaultValue = 'w'; c.value",
                "v",
            ),
            (
                "var c2 = document.createElement('textarea').cloneNode(); c2.defaultValue = 'w'; c2.value",
                "w",
            ),
            // A form's reset: its child text content again.
            (
                "var form = document.body.appendChild(document.createElement('form')); form.append(t); form.reset(); [t.value, t.defaultValue].join()",
                "other,other",
            ),
            ("t.defaultValue = 'follows'; t.value", "follows"),
        ],
    );
}

#[test]
fn the_selection_api() {
    let mut env = window();
    check(
        &mut env,
        &[
            // At the start, none.
            (
                "var t = input('text'); t.setAttribute('value', 'hello'); sel(t)",
                "0,0,none",
            ),
            // Within the value; an empty one at the end if the start is
            // after it.
            (
                "t.setSelectionRange(1, 3, 'backward'); sel(t)",
                "1,3,backward",
            ),
            ("t.setSelectionRange(4, 2); sel(t)", "2,2,none"),
            (
                "t.setSelectionRange(1, 100, 'forward'); sel(t)",
                "1,5,forward",
            ),
            // Set within the value as it was: a longer one later doesn't
            // widen it.
            (
                "t.setAttribute('value', 'hello world'); sel(t)",
                "1,5,forward",
            ),
            ("t.setAttribute('value', 'hello'); sel(t)", "1,5,forward"),
            ("t.setSelectionRange(1, 2, 'sideways'); sel(t)", "1,2,none"),
            // The start's setter moves the end, the end's the start; null
            // is 0.
            (
                "t.setSelectionRange(1, 2, 'backward'); t.selectionStart = 4; sel(t)",
                "4,4,backward",
            ),
            ("t.selectionEnd = 2; sel(t)", "2,2,backward"),
            (
                "t.selectionEnd = 4; t.selectionStart = null; sel(t)",
                "0,4,backward",
            ),
            ("t.selectionDirection = 'forward'; sel(t)", "0,4,forward"),
            ("t.selectionDirection = 'x'; sel(t)", "0,4,none"),
            // Read within the value as it is.
            ("t.setAttribute('value', 'he'); sel(t)", "0,2,none"),
            ("t.setAttribute('value', 'hello'); sel(t)", "0,4,none"),
            (
                "t.setSelectionRange(3, 4); t.setAttribute('value', 'he'); sel(t)",
                "2,2,none",
            ),
            // The other types have none.
            (
                "var n = input('number'); [n.selectionStart, n.selectionEnd, n.selectionDirection].join('/')",
                "//",
            ),
            (
                "[error(() => n.selectionStart = 1), error(() => n.selectionEnd = 1), error(() => n.selectionDirection = 'forward'), \
                 error(() => n.setSelectionRange(0, 1)), error(() => n.setRangeText('x'))].join()",
                "InvalidStateError,InvalidStateError,InvalidStateError,InvalidStateError,InvalidStateError",
            ),
            (
                "['search', 'url', 'tel', 'password'].map(type => input(type).selectionStart).join()",
                "0,0,0,0",
            ),
            (
                "var ta = document.createElement('textarea'); ta.defaultValue = 'ab'; ta.setSelectionRange(1, 2); sel(ta)",
                "1,2,none",
            ),
        ],
    );
}

#[test]
fn a_value_set_leaves_the_cursor_at_its_end_if_it_changed() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = input('text'); t.value = 'hello'; sel(t)",
                "5,5,none",
            ),
            (
                "t.setSelectionRange(1, 3, 'backward'); t.value = 'hello'; sel(t)",
                "1,3,backward",
            ),
            // The same once sanitized.
            ("t.value = 'hel\\nlo'; sel(t)", "1,3,backward"),
            ("t.value = 'hello2'; sel(t)", "6,6,none"),
            // A textarea's API value.
            (
                "var a = document.createElement('textarea'); a.value = 'hell\\no'; a.setSelectionRange(1, 3, 'backward'); a.value = 'hell\\r\\no'; sel(a)",
                "1,3,backward",
            ),
            ("a.value = 'x'; sel(a)", "1,1,none"),
            // A type with a selection after one without: the cursor at the
            // start.
            (
                "var s = input('text'); s.value = 'abc'; s.type = 'number'; s.type = 'text'; sel(s)",
                "0,0,none",
            ),
            (
                "s.setSelectionRange(1, 2); s.type = 'password'; sel(s)",
                "1,2,none",
            ),
        ],
    );
}

#[test]
fn set_range_text() {
    let mut env = window();
    check(
        &mut env,
        &[
            // The selection's text by default, the selection kept.
            (
                "var t = input('text'); t.value = 'abcdef'; t.setSelectionRange(1, 3); t.setRangeText('XYZ'); [t.value, sel(t)].join(' ')",
                "aXYZdef 1,4,none",
            ),
            // Each mode.
            (
                "t.value = 'abcdef'; t.setRangeText('12', 1, 3, 'select'); [t.value, sel(t)].join(' ')",
                "a12def 1,3,none",
            ),
            (
                "t.value = 'abcdef'; t.setRangeText('12', 1, 3, 'start'); [t.value, sel(t)].join(' ')",
                "a12def 1,1,none",
            ),
            (
                "t.value = 'abcdef'; t.setRangeText('12', 1, 3, 'end'); [t.value, sel(t)].join(' ')",
                "a12def 3,3,none",
            ),
            // Preserve: after the text moves with it, within it goes to its
            // edges, before it stays.
            (
                "t.value = 'abcdef'; t.setSelectionRange(4, 6); t.setRangeText('1', 1, 3); [t.value, sel(t)].join(' ')",
                "a1def 3,5,none",
            ),
            (
                "t.value = 'abcdef'; t.setSelectionRange(2, 3); t.setRangeText('1234', 1, 3); [t.value, sel(t)].join(' ')",
                "a1234def 1,5,none",
            ),
            (
                "t.value = 'abcdef'; t.setSelectionRange(0, 1); t.setRangeText('1', 1, 3); [t.value, sel(t)].join(' ')",
                "a1def 0,1,none",
            ),
            // Past the end is the end; dirty.
            (
                "t.value = 'ab'; t.setRangeText('Z', 5, 9, 'select'); [t.value, sel(t)].join(' ')",
                "abZ 2,3,none",
            ),
            ("t.setAttribute('value', 'other'); t.value", "abZ"),
            // Sanitized as an input's value is.
            ("t.setRangeText('\\n!', 0, 0); t.value", "!abZ"),
            // Errors: the start after the end, two arguments, a mode not
            // the enum's.
            (
                "[error(() => t.setRangeText('x', 2, 1)), error(() => t.setRangeText('x', 1)), error(() => t.setRangeText('x', 0, 0, 'all'))].join()",
                "IndexSizeError,TypeError,TypeError",
            ),
            (
                "var a = document.createElement('textarea'); a.defaultValue = 'line'; a.setRangeText('\\r\\n', 2, 2); [JSON.stringify(a.value), a.defaultValue].join()",
                "\"li\\nne\",line",
            ),
            // A code unit within a character: a replacement character.
            (
                "a.value = '\\u{1F600}'; a.setRangeText('x', 1, 1); a.value.codePointAt(0).toString(16)",
                "fffd",
            ),
        ],
    );
}

#[test]
fn a_selection_that_changes_queues_select() {
    let mut env = window();
    env.eval(
        "var t = document.body.appendChild(input('text')); t.value = 'hello'; var log = []; \
         document.body.addEventListener('select', e => log.push([e.target.type, e.isTrusted, e.bubbles, e.cancelable].join('/'))); 0",
    );
    // Queued, not fired as the selection is set; once each change.
    check(
        &mut env,
        &[(
            "t.setSelectionRange(1, 2); t.setSelectionRange(1, 3); log.length",
            "0",
        )],
    );
    // And one `selectionchange` (`selections`), scheduled once until it
    // runs.
    assert_eq!(env.run_element_tasks(), 3);
    check(
        &mut env,
        &[
            ("log.join()", "text/true/true/false,text/true/true/false"),
            // None for a selection as it was, nor a value set (the cursor
            // moves: its `selectionchange` only).
            (
                "log = []; t.setSelectionRange(1, 3); t.selectionDirection = 'none'; t.value = 'other'; 0",
                "0",
            ),
        ],
    );
    assert_eq!(env.run_element_tasks(), 1);
    env.eval(
        "t.setRangeText('x', 0, 1, 'select'); t.selectionEnd = 3; t.selectionStart = 2; t.selectionDirection = 'backward'; \
         var n = document.body.appendChild(input('number')); n.select(); var c = document.body.appendChild(input('checkbox')); c.select(); 0",
    );
    // Five `select`s, and `t`'s `selectionchange` (a number input and a
    // checkbox have no selection).
    assert_eq!(env.run_element_tasks(), 6);
    // A listener's own: run after the one it ran in. `select()`'s
    // `selectionchange` was queued before its `select` and ran first, so
    // the listener's change schedules another.
    env.eval("log = []; t.addEventListener('select', () => t.setSelectionRange(0, 1), { once: true }); t.select(); 0");
    assert_eq!(env.run_element_tasks(), 4);
    check(
        &mut env,
        &[("log.join()", "text/true/true/false,text/true/true/false")],
    );
    // An element out of a window's document: none.
    env.eval(
        "var other = document.implementation.createHTMLDocument(''); var o = other.body.appendChild(other.createElement('input')); \
         o.value = 'abc'; o.setSelectionRange(0, 2); 0",
    );
    assert_eq!(env.run_element_tasks(), 0);
}

/// (Its focus: the tests' window has no layout, so no control is a
/// focusable area: `dom_api`'s focus tests.)
#[test]
fn select_selects_all() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = document.body.appendChild(input('text')); t.value = 'hello'; t.select(); sel(t)",
                "0,5,none",
            ),
            (
                "var a = document.body.appendChild(document.createElement('textarea')); a.defaultValue = 'ab'; a.select(); sel(a)",
                "0,2,none",
            ),
            (
                "var d = document.body.appendChild(input('text')); d.disabled = true; d.value = 'xy'; d.select(); sel(d)",
                "0,2,none",
            ),
        ],
    );
}
