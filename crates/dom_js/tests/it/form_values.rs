//! An input's value from script: by its value mode (its value, dirty once
//! set; the `value` attribute; `on`; a file input's empty string), the
//! value sanitization of the text types, `url`, `email`, `number`,
//! `range` and `color` (as read: the attributes it reads may change), the
//! type change steps, a copy's; a form's `reset()` and its `reset` event,
//! a reset button; a button's `type`; `RadioNodeList`'s `value`; a
//! number's and a range's `valueAsNumber`, `stepUp()` and `stepDown()`;
//! the URL and enumerated reflections of a form and its submitters,
//! `autocomplete`, an input's `width`, `height` and `src`; a form's
//! `relList` and an output's `htmlFor`.

use crate::env::Env;

/// A window whose document has an `<html>` and a `<body>`, and an
/// `error(f)` giving the name of what `f` throws.
fn window() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); \
         function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } \
         function input(type) { const i = document.createElement('input'); i.type = type; return i; } 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn a_value_by_its_value_mode() {
    let mut env = window();
    check(
        &mut env,
        &[
            // The element's value: the attribute's until set.
            (
                "var t = input('text'); t.setAttribute('value', 'a'); t.value",
                "a",
            ),
            ("t.defaultValue = 'b'; t.value", "b"),
            (
                "t.value = 'c'; t.setAttribute('value', 'd'); [t.value, t.defaultValue].join()",
                "c,d",
            ),
            ("t.value = null; JSON.stringify(t.value)", "\"\""),
            // The attribute's: set sets it.
            (
                "var h = input('hidden'); h.value = 'x'; [h.value, h.getAttribute('value')].join()",
                "x,x",
            ),
            ("var s = input('submit'); JSON.stringify(s.value)", "\"\""),
            // `on` without one.
            ("var c = input('checkbox'); c.value", "on"),
            (
                "c.value = 'v'; [c.value, c.getAttribute('value')].join()",
                "v,v",
            ),
            // A file input's: empty, set only to the empty string.
            (
                "var f = input('file'); f.setAttribute('value', 'x'); [JSON.stringify(f.value), error(() => f.value = 'y'), error(() => f.value = '')].join()",
                "\"\",InvalidStateError,no",
            ),
            // Not an input's.
            ("document.createElement('button').value", ""),
        ],
    );
}

#[test]
fn a_value_is_sanitized_by_its_type() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "var t = input('text'); t.value = 'a\\nb\\rc'; t.value",
                "abc",
            ),
            (
                "var p = input('password'); p.setAttribute('value', 'x\\ny'); [p.value, p.getAttribute('value')].join()",
                "xy,x\ny",
            ),
            (
                "var u = input('url'); u.value = ' \\thttp://a/\\n '; JSON.stringify(u.value)",
                "\"http://a/\"",
            ),
            (
                "var e = input('email'); e.value = ' a@b , c@d '; JSON.stringify(e.value)",
                "\"a@b , c@d\"",
            ),
            ("e.multiple = true; JSON.stringify(e.value)", "\"a@b,c@d\""),
            ("var n = input('number'); n.value = '1e3'; n.value", "1e3"),
            (
                "[' 1', '+1', '1.', 'abc', '-0.5'].map(v => { n.value = v; return JSON.stringify(n.value); }).join()",
                "\"\",\"\",\"\",\"\",\"-0.5\"",
            ),
            // A range: halfway by default, within min and max, aligned to
            // the step (as the attributes are when read).
            ("var r = input('range'); r.value", "50"),
            ("r.value = '150'; r.value", "100"),
            ("r.value = '7.3'; r.value", "7"),
            // (Set sanitized: a later step has nothing to round.)
            (
                "r.step = '0.5'; var before = r.value; r.value = '7.3'; [before, r.value].join()",
                "7,7.5",
            ),
            (
                "r.setAttribute('step', 'any'); r.value = '7.3'; r.value",
                "7.3",
            ),
            // A step not above 0 is the default; a result rounded to the
            // step's places; a value within them as written.
            ("r.step = '0'; r.value = '7.3'; r.value", "7"),
            ("r.step = '0.1'; r.value = '0.3'; r.value", "0.3"),
            ("r.value = '0.25'; r.value", "0.3"),
            ("r.step = '1'; r.value = '5e1'; r.value", "5e1"),
            ("r.min = '60'; r.value", "60"),
            (
                "r.max = '20'; r.removeAttribute('value'); r.value = 'x'; r.value",
                "60",
            ),
            (
                "var co = input('color'); co.value = '#ABCDEF'; co.value",
                "#abcdef",
            ),
            ("co.value = 'red'; co.value", "#000000"),
            // The date and time types' stay as they are.
            (
                "var d = input('date'); d.value = 'not a date'; d.value",
                "not a date",
            ),
        ],
    );
}

#[test]
fn the_type_change_steps() {
    let mut env = window();
    check(
        &mut env,
        &[
            // From the element's value to the attribute's: it goes there.
            (
                "var t = input('text'); t.value = 'kept'; t.type = 'hidden'; [t.value, t.getAttribute('value')].join()",
                "kept,kept",
            ),
            // Back: the attribute's, not dirty.
            (
                "t.type = 'text'; t.setAttribute('value', 'new'); t.value",
                "new",
            ),
            // An empty value doesn't.
            (
                "t.value = ''; t.type = 'checkbox'; [t.value, t.getAttribute('value')].join()",
                "new,new",
            ),
            // A file input's isn't read; leaving it, the attribute's.
            (
                "var f = input('text'); f.value = 'x'; f.type = 'file'; f.type = 'text'; JSON.stringify(f.value)",
                "\"\"",
            ),
            // A copy has the value, dirty.
            (
                "var o = input('text'); o.value = 'v'; var c = o.cloneNode(); c.setAttribute('value', 'w'); c.value",
                "v",
            ),
        ],
    );
}

#[test]
fn a_forms_reset() {
    let mut env = window();
    env.eval(
        "var form = document.body.appendChild(document.createElement('form')); \
         form.innerHTML = '<input id=t value=a><input type=checkbox id=c checked><input type=radio name=r id=r1 checked>\
           <input type=radio name=r id=r2><input type=reset id=rb><button type=reset id=bb>b</button><button id=sb>s</button>'; \
         function $(id) { return document.getElementById(id); } \
         var log = []; form.addEventListener('reset', e => log.push([e.isTrusted, e.bubbles, e.cancelable].join('/'))); \
         function change() { $('t').value = 'b'; $('c').checked = false; $('r2').checked = true; } \
         function state() { return [$('t').value, $('c').checked, $('r1').checked, $('r2').checked].join(); } 0",
    );
    check(
        &mut env,
        &[
            ("change(); state()", "b,false,false,true"),
            // `reset`, then each control as its attributes say, not dirty.
            (
                "form.reset(); [state(), log].join(' ')",
                "a,true,true,false true/true/true",
            ),
            (
                "$('t').setAttribute('value', 'z'); $('c').removeAttribute('checked'); [$('t').value, $('c').checked].join()",
                "z,false",
            ),
            // Canceled: nothing.
            (
                "log = []; change(); var cancel = e => e.preventDefault(); form.addEventListener('reset', cancel); \
                 form.reset(); form.removeEventListener('reset', cancel); [state(), log].join(' ')",
                "b,false,false,true true/true/true",
            ),
            // `reset()` while resetting: nothing.
            (
                "log = []; var again = () => form.reset(); form.addEventListener('reset', again); form.reset(); \
                 form.removeEventListener('reset', again); log.length",
                "1",
            ),
            // Unlocked after.
            ("log = []; form.reset(); log.length", "1"),
            // A reset button's click, an `<input>`'s and a `<button>`'s.
            (
                "log = []; change(); $('rb').click(); [state(), log.length].join()",
                "z,false,true,false,1",
            ),
            (
                "log = []; change(); $('bb').click(); [state(), log.length].join()",
                "z,false,true,false,1",
            ),
            // A submit button's: no reset.
            (
                "log = []; change(); $('sb').click(); [state(), log.length].join()",
                "b,false,false,true,0",
            ),
            // With no form, or in a document with no window: none.
            (
                "log = []; var lone = $('rb'); document.body.append(lone); lone.click(); log.length",
                "0",
            ),
            (
                "var other = document.implementation.createHTMLDocument(''); var of = other.body.appendChild(other.createElement('form')); \
                 var resets = 0; of.addEventListener('reset', () => resets++); \
                 of.innerHTML = '<input type=reset>'; of.firstChild.click(); var byClick = resets; of.reset(); [byClick, resets].join()",
                "0,1",
            ),
        ],
    );
}

#[test]
fn a_buttons_type() {
    let mut env = window();
    check(
        &mut env,
        &[
            ("var b = document.createElement('button'); b.type", "submit"),
            ("b.setAttribute('type', 'RESET'); b.type", "reset"),
            (
                "b.type = 'button'; [b.type, b.getAttribute('type')].join()",
                "button,button",
            ),
            (
                "b.type = 'bogus'; [b.type, b.getAttribute('type')].join()",
                "submit,bogus",
            ),
        ],
    );
}

#[test]
fn a_radio_node_lists_value() {
    let mut env = window();
    env.eval(
        "var form = document.body.appendChild(document.createElement('form')); \
         form.innerHTML = '<input type=radio name=r value=a><input type=radio name=r><input type=radio name=r value=b>\
           <input type=checkbox name=r value=c checked><input type=radio name=r value=a>'; \
         var list = form.elements.r, radios = form.querySelectorAll('input'); 0",
    );
    check(
        &mut env,
        &[
            // The checked radio button's, else empty; not a checkbox's.
            (
                "[list instanceof RadioNodeList, JSON.stringify(list.value)].join()",
                "true,\"\"",
            ),
            ("radios[2].checked = true; list.value", "b"),
            ("radios[1].checked = true; list.value", "on"),
            // Setting: the first whose `value` it is checked; `on`, one
            // with none.
            (
                "list.value = 'a'; [radios[0].checked, radios[4].checked, list.value].join()",
                "true,false,a",
            ),
            ("list.value = 'on'; radios[1].checked", "true"),
            (
                "radios[3].checked = false; list.value = 'c'; [radios[3].checked, list.value].join()",
                "false,on",
            ),
            ("list.value = 'none'; list.value", "on"),
        ],
    );
}

#[test]
fn value_as_number_and_the_steps() {
    let mut env = window();
    check(
        &mut env,
        &[
            (
                "n = input('number'); [n.valueAsNumber, (n.value = '2.5', n.valueAsNumber)].join()",
                "NaN,2.5",
            ),
            ("n.valueAsNumber = 7; n.value", "7"),
            ("n.valueAsNumber = NaN; '[' + n.value + ']'", "[]"),
            ("error(() => n.valueAsNumber = Infinity)", "TypeError"),
            ("t = input('text'); t.value = '3'; t.valueAsNumber", "NaN"),
            (
                "error(() => t.valueAsNumber = 1) + ' ' + error(() => t.stepUp())",
                "InvalidStateError InvalidStateError",
            ),
            // From none, 0; `n` steps, aligned to the step base.
            ("n.value = ''; n.stepUp(); n.value", "1"),
            (
                "n.stepUp(3); n.stepDown(); n.stepUp(undefined); n.value",
                "4",
            ),
            (
                "n.step = '2'; n.min = '1'; n.value = '4'; n.stepUp(); n.value",
                "5",
            ),
            ("n.value = '5'; n.stepDown(2); n.value", "1"),
            // Kept within the minimum and the maximum.
            ("n.max = '6'; n.value = '5'; n.stepUp(5); n.value", "5"),
            (
                "n.step = 'any'; error(() => n.stepUp())",
                "InvalidStateError",
            ),
            // A range's.
            (
                "r = input('range'); [r.valueAsNumber, (r.stepUp(10), r.value)].join()",
                "50,60",
            ),
            ("r.valueAsNumber = NaN; r.value", "50"),
            ("r.stepDown(100); r.value", "0"),
        ],
    );
}

#[test]
fn a_forms_and_its_submitters_reflections() {
    let mut env = Env::window("https://example.com/dir/page.html#top");
    env.eval("function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } 0");
    check(
        &mut env,
        &[
            (
                "f = document.createElement('form'); [f.action, f.method, f.enctype, f.encoding, f.autocomplete].join()",
                "https://example.com/dir/page.html#top,get,application/x-www-form-urlencoded,application/x-www-form-urlencoded,on",
            ),
            (
                "f.action = 'go?q=1'; f.method = 'POST'; f.enctype = 'TEXT/plain'; f.autocomplete = 'nope'; \
              [f.action, f.getAttribute('action'), f.method, f.encoding, f.autocomplete].join()",
                "https://example.com/dir/go?q=1,go?q=1,post,text/plain,on",
            ),
            // An empty action is the document's URL, its fragment too.
            (
                "f.method = 'put'; f.enctype = ''; f.action = ''; [f.method, f.enctype, f.action].join()",
                "get,application/x-www-form-urlencoded,https://example.com/dir/page.html#top",
            ),
            // A submitter's have no default for none.
            (
                "b = document.createElement('button'); [b.formAction, '[' + b.formMethod + ']', '[' + b.formEnctype + ']'].join()",
                "https://example.com/dir/page.html#top,[],[]",
            ),
            (
                "b.formMethod = 'Dialog'; b.formEnctype = 'x'; b.formAction = '/top'; [b.formMethod, b.formEnctype, b.formAction].join()",
                "dialog,application/x-www-form-urlencoded,https://example.com/top",
            ),
            (
                "i = document.createElement('input'); [i.formMethod, i.src, i.width, i.height, i.autocomplete].join('|')",
                "||0|0|",
            ),
            (
                "i.src = 'a.png'; i.width = 5; i.autocomplete = 'off'; [i.src, i.width, i.getAttribute('autocomplete')].join()",
                "https://example.com/dir/a.png,5,off",
            ),
            (
                "['select', 'textarea'].map(n => (e = document.createElement(n), e.autocomplete = 'email', e.autocomplete)).join()",
                "email,email",
            ),
            // A form's `relList`, an output's `htmlFor`.
            (
                "rl = f.relList; [rl === f.relList, rl.length, rl.value].join()",
                "true,0,",
            ),
            (
                "f.rel = 'noopener  next noopener'; [rl.length, rl[1], rl.contains('next')].join()",
                "2,next,true",
            ),
            ("rl.add('prev'); rl.remove('noopener'); f.rel", "next prev"),
            (
                "rl.toggle('next'); rl.replace('prev', 'up'); f.getAttribute('rel')",
                "up",
            ),
            (
                "f.relList = 'a b'; f.rel + ' ' + error(() => rl.supports('a'))",
                "a b TypeError",
            ),
            (
                "o = document.createElement('output'); o.htmlFor.add('x', 'y'); o.getAttribute('for') + ' ' + o.htmlFor.value",
                "x y x y",
            ),
            (
                "o.htmlFor = 'z'; o.htmlFor.length + ' ' + (o.htmlFor === o.htmlFor)",
                "1 true",
            ),
            // `classList` is still the class table's.
            (
                "o.className = 'p q'; o.classList.length + ' ' + o.classList.contains('q')",
                "2 true",
            ),
        ],
    );
}
