//! A click's activation from script: an input's `type` and its
//! checkedness (`checked` following the attribute until dirty,
//! `indeterminate`, kept by a copy); `click()` and a dispatched
//! `MouseEvent` click toggling a checkbox before the listeners, `input`
//! and `change` after, a canceled click undoing it; the first of the path
//! with an activation behavior activated (a form's buttons have one); radio button groups
//! (by name, form owner and tree; checking one unchecks the others, as
//! does a name, a type, an owner or a connection); a label clicking its
//! control; disabled controls (their attribute, a fieldset's but in its
//! first legend), what a fieldset ancestor says kept until the tree or a
//! fieldset changes.

use crate::env::Env;

/// A window whose document has an `<html>` and a `<body>`, a `log` and a
/// `listen(element, types)` logging `type:isTrusted` of each event.
fn window_with_log() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); \
         var log = []; \
         function listen(element, types) { \
           for (const type of types) element.addEventListener(type, e => log.push(e.type + ':' + e.isTrusted)); } \
         function take() { const taken = log.join(); log = []; return taken; } 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn an_inputs_type_and_checkedness() {
    let mut env = window_with_log();
    env.eval("var i = document.createElement('input'); 0");
    check(
        &mut env,
        &[
            // `type`: the keyword, `text` for none or another value.
            ("i.type", "text"),
            ("i.setAttribute('type', 'CheckBox'); i.type", "checkbox"),
            (
                "i.type = 'datetime-local'; [i.type, i.getAttribute('type')].join()",
                "datetime-local,datetime-local",
            ),
            (
                "i.type = 'bogus'; [i.type, i.getAttribute('type')].join()",
                "text,bogus",
            ),
            (
                "i.removeAttribute('type'); i.type = 'checkbox'; i.type",
                "checkbox",
            ),
            // The checkedness follows `checked` until set.
            ("[i.checked, i.defaultChecked].join()", "false,false"),
            ("i.defaultChecked = true; i.checked", "true"),
            ("i.removeAttribute('checked'); i.checked", "false"),
            (
                "i.checked = true; i.removeAttribute('checked'); i.checked",
                "true",
            ),
            (
                "i.checked = false; i.setAttribute('checked', ''); i.checked",
                "false",
            ),
            // `indeterminate` is its own.
            (
                "i.indeterminate = true; [i.indeterminate, i.hasAttribute('indeterminate')].join()",
                "true,false",
            ),
            // A copy has them, dirty too.
            (
                "var c = i.cloneNode(); c.removeAttribute('checked'); [c.checked, c.indeterminate, c.type].join()",
                "false,true,checkbox",
            ),
            (
                "var p = document.createElement('input'); p.type = 'checkbox'; p.checked = true; \
                 var q = p.cloneNode(); q.setAttribute('checked', ''); q.removeAttribute('checked'); q.checked",
                "true",
            ),
            // The parser's: the attribute's.
            (
                "document.body.innerHTML = '<input type=checkbox checked><input type=radio>'; \
                 [document.body.firstChild.checked, document.body.lastChild.type].join()",
                "true,radio",
            ),
            // Not an input's.
            ("document.createElement('button').checked", "undefined"),
        ],
    );
}

#[test]
fn clicking_a_checkbox() {
    let mut env = window_with_log();
    env.eval(
        "var c = document.body.appendChild(document.createElement('input')); c.type = 'checkbox'; \
         listen(c, ['click', 'input', 'change']); \
         var seen = []; c.addEventListener('click', () => seen.push(c.checked)); \
         c.addEventListener('input', e => seen.push([e.bubbles, e.cancelable, e.composed].join('/'))); \
         c.addEventListener('change', e => seen.push([e.bubbles, e.cancelable, e.composed].join('/'))); 0",
    );
    check(
        &mut env,
        &[
            // Checked before the listeners; `input` and `change` after,
            // trusted though the click isn't.
            (
                "c.click(); [c.checked, take()].join()",
                "true,click:false,input:true,change:true",
            ),
            ("seen.join(' ')", "true true/false/true true/false/false"),
            // Unchecked, its indeterminate gone.
            (
                "c.indeterminate = true; c.click(); [c.checked, c.indeterminate].join()",
                "false,false",
            ),
            // A canceled click is undone after the listeners, indeterminate
            // as it was (set by a listener too); no `input` or `change`.
            (
                "seen = []; var cancel = e => { e.preventDefault(); c.indeterminate = true; }; \
                 c.addEventListener('click', cancel); take(); c.click(); \
                 [c.checked, c.indeterminate, seen, take()].join()",
                "false,false,true,click:false",
            ),
            ("c.removeEventListener('click', cancel); 0", "0"),
            // A `MouseEvent` dispatched is a click; an `Event` named so,
            // or another type, isn't.
            (
                "c.dispatchEvent(new MouseEvent('click', {bubbles: true})); [c.checked, take()].join()",
                "true,click:false,input:true,change:true",
            ),
            (
                "c.dispatchEvent(new Event('click')); [c.checked, take()].join()",
                "true,click:false",
            ),
            (
                "c.dispatchEvent(new MouseEvent('mouseup')); c.checked",
                "true",
            ),
            // Out of a document: toggled, but no `input` or `change`.
            (
                "c.remove(); c.click(); [c.checked, take()].join()",
                "false,click:false",
            ),
            // A click in a listener of its own click does nothing.
            (
                "document.body.append(c); var again = () => c.click(); c.addEventListener('click', again); \
                 take(); c.click(); c.removeEventListener('click', again); [c.checked, take()].join()",
                "true,click:false,input:true,change:true",
            ),
            // Another type: no activation.
            (
                "c.type = 'text'; c.click(); [c.checked, take()].join()",
                "true,click:false",
            ),
        ],
    );
}

#[test]
fn a_click_activates_the_first_ancestor_that_has_an_activation_behavior() {
    let mut env = window_with_log();
    env.eval(
        "document.body.innerHTML = '<div><input type=checkbox id=c><span id=s>x</span></div>'; \
         var c = document.getElementById('c'), s = document.getElementById('s'); 0",
    );
    check(
        &mut env,
        &[
            // The span has none, nor its ancestors.
            ("s.click(); c.checked", "false"),
            // In the checkbox: a span's bubbling click is the checkbox's.
            ("c.append(s); s.click(); c.checked", "true"),
            (
                "s.dispatchEvent(new MouseEvent('click')); c.checked",
                "true",
            ),
            // A form's submit or reset button has its own (doing nothing
            // yet); a `type=button` one, one with no form, has none.
            (
                "var f = c.appendChild(document.createElement('form')); \
                 f.innerHTML = '<button id=b>b</button><input type=submit id=i>'; \
                 var b = document.getElementById('b'), i = document.getElementById('i'); \
                 b.click(); i.click(); b.setAttribute('type', 'Reset'); b.click(); i.type = 'reset'; i.click(); c.checked",
                "true",
            ),
            ("i.type = 'image'; i.click(); c.checked", "true"),
            (
                "b.setAttribute('type', 'BUTTON'); b.click(); c.checked",
                "false",
            ),
            ("i.type = 'button'; i.click(); c.checked", "true"),
            (
                "i.type = 'submit'; c.append(i); i.click(); b.removeAttribute('type'); c.append(b); b.click(); c.checked",
                "true",
            ),
        ],
    );
}

#[test]
fn radio_button_groups() {
    let mut env = window_with_log();
    env.eval(
        "document.body.innerHTML = '<form id=f><input type=radio name=a id=a1 checked><input type=radio name=a id=a2>\
           <input type=radio name=b id=b1 checked><input type=radio id=n1 checked><input type=radio id=n2 checked></form>\
           <input type=radio name=a id=o1 checked><input type=radio name=a id=o2>'; \
         function $(id) { return document.getElementById(id); } \
         function checked() { return Array.from(document.querySelectorAll('input'), i => i.checked ? 1 : 0).join(''); } 0",
    );
    check(
        &mut env,
        &[
            // A group is a name and an owner: the form's `a`, `b`, the
            // nameless ones each their own, the outside `a`.
            ("checked()", "1011110"),
            ("$('a2').checked = true; checked()", "0111110"),
            ("$('o2').click(); checked()", "0111101"),
            // A click on a checked one: no `input` or `change`.
            (
                "listen($('a2'), ['click', 'input', 'change']); $('a2').click(); take()",
                "click:false",
            ),
            // A canceled click: the one checked before checked again.
            (
                "$('a1').addEventListener('click', e => e.preventDefault()); $('a1').click(); checked()",
                "0111101",
            ),
            // A canceled click on a checked one: still checked.
            (
                "$('a2').addEventListener('click', e => e.preventDefault(), {once: true}); $('a2').click(); checked()",
                "0111101",
            ),
            // The one checked before isn't checked again if it left the
            // group meanwhile.
            (
                "var leave = () => { $('a2').name = 'gone'; }; $('a1').addEventListener('click', leave, {once: true}); \
                 $('a1').click(); var after = checked(); $('a2').name = 'a'; after",
                "0011101",
            ),
            ("$('a2').checked = true; checked()", "0111101"),
            // A listener that makes it another type, then cancels: the one
            // checked before is checked again all the same (as in Chrome),
            // unless it left the group too.
            (
                "var morph = e => { $('a1').type = 'date'; e.preventDefault(); }; $('a1').addEventListener('click', morph, {once: true}); \
                 $('a1').click(); var after = [$('a1').checked, $('a2').checked].join(); $('a1').type = 'radio'; after",
                "false,true",
            ),
            (
                "var both = e => { $('a1').type = 'date'; $('a2').name = 'gone'; e.preventDefault(); }; \
                 $('a1').addEventListener('click', both, {once: true}); $('a1').click(); \
                 var after = [$('a1').checked, $('a2').checked].join(); $('a1').type = 'radio'; $('a2').name = 'a'; $('a2').checked = true; after",
                "false,false",
            ),
            ("checked()", "0111101"),
            // An unchecked one coming into a group: none unchecked.
            (
                "var u = document.createElement('input'); u.type = 'radio'; u.name = 'a'; $('f').append(u); \
                 var after = checked(); u.remove(); after",
                "01111001",
            ),
            // A name, then a type, an owner change: the new group's
            // unchecked.
            ("$('b1').name = 'a'; checked()", "0011101"),
            (
                "$('a1').type = 'text'; $('a1').checked = true; $('a1').type = 'radio'; checked()",
                "1001101",
            ),
            ("$('o2').setAttribute('form', 'f'); checked()", "0001101"),
            // An empty name is none: no group.
            (
                "var e = document.createElement('div'); document.body.append(e); \
                 e.innerHTML = '<input type=radio name=\"\" checked><input type=radio name=\"\" checked>'; \
                 var after = [e.firstChild.checked, e.lastChild.checked].join(); e.remove(); after",
                "true,true",
            ),
            // Out of the document, its own tree's group.
            (
                "var away = $('o2'); away.remove(); $('o1').checked = true; [away.checked, $('o1').checked].join()",
                "true,true",
            ),
            // Connected again: the others unchecked.
            (
                "away.removeAttribute('form'); document.body.append(away); [away.checked, $('o1').checked].join()",
                "true,false",
            ),
            // Into a tree out of the document: none unchecked.
            (
                "var t = document.createElement('div'); t.innerHTML = '<input type=radio name=q checked>'; \
                 var r = document.createElement('input'); r.type = 'radio'; r.name = 'q'; r.checked = true; t.append(r); \
                 [t.firstChild.checked, r.checked].join()",
                "true,true",
            ),
            // The parser's: the last checked of a group stays.
            (
                "var d = document.createElement('div'); document.body.append(d); \
                 d.innerHTML = '<input type=radio name=p checked><input type=radio name=p checked>'; \
                 [d.firstChild.checked, d.lastChild.checked].join()",
                "false,true",
            ),
        ],
    );
}

#[test]
fn a_label_clicks_its_control() {
    let mut env = window_with_log();
    env.eval(
        "document.body.innerHTML = '<label id=l><span id=s>text</span><input type=checkbox id=c><button id=b>b</button></label>\
           <label id=f for=c>for</label>'; \
         function $(id) { return document.getElementById(id); } \
         var c = $('c'); listen(c, ['click', 'input', 'change']); 0",
    );
    check(
        &mut env,
        &[
            // A click on the label (in it): its control's too, after.
            (
                "$('s').click(); [c.checked, take()].join()",
                "true,click:false,input:true,change:true",
            ),
            (
                "$('f').click(); [c.checked, take()].join()",
                "false,click:false,input:true,change:true",
            ),
            // On the control itself: once.
            (
                "c.click(); [c.checked, take()].join()",
                "true,click:false,input:true,change:true",
            ),
            // On interactive content in it: not the control's.
            ("$('b').click(); [c.checked, take()].join()", "true,"),
            // A trusted-looking one isn't trusted: the control's click is
            // as the label's.
            (
                "$('l').dispatchEvent(new MouseEvent('click')); [c.checked, take()].join()",
                "false,click:false,input:true,change:true",
            ),
            // A canceled one: nothing.
            (
                "$('l').addEventListener('click', e => e.preventDefault(), {once: true}); $('l').click(); [c.checked, take()].join()",
                "false,",
            ),
            // Its control's click has `detail` 1, and the label's modifier
            // keys.
            (
                "var keys = []; c.addEventListener('click', e => keys.push(e.detail, e.shiftKey), {once: true}); \
                 $('l').dispatchEvent(new MouseEvent('click', {shiftKey: true})); take(); keys.join()",
                "1,true",
            ),
            // A disabled control: nothing.
            (
                "c.disabled = true; $('l').click(); [c.checked, take()].join()",
                "true,",
            ),
            // A control with no activation behavior: its own click once.
            (
                "document.body.innerHTML = '<label id=tl><input id=t></label>'; listen($('t'), ['click']); \
                 $('t').dispatchEvent(new MouseEvent('click', {bubbles: true})); take()",
                "click:false",
            ),
            // A control that isn't interactive content, clicked in its label:
            // its click once.
            (
                "document.body.innerHTML = '<label><output id=o></output></label>'; listen($('o'), ['click']); \
                 $('o').dispatchEvent(new MouseEvent('click', {bubbles: true})); take()",
                "click:false",
            ),
            // Interactive content around the label isn't in it.
            (
                "document.body.innerHTML = '<button><label for=c2><span id=s2>x</span></label></button><input type=checkbox id=c2>'; \
                 $('s2').click(); $('c2').checked",
                "true",
            ),
        ],
    );
}

#[test]
fn disabled_controls_arent_clicked() {
    let mut env = window_with_log();
    env.eval(
        "document.body.innerHTML = '<fieldset id=fs><span></span><legend id=g1><input type=checkbox id=in_legend></legend>\
           <legend id=g2><input type=checkbox id=second></legend><div><input type=checkbox id=deep></div>\
           <fieldset id=inner><input type=checkbox id=nested></fieldset></fieldset><input type=checkbox id=d disabled>'; \
         function $(id) { return document.getElementById(id); } \
         var inputs = ['in_legend', 'second', 'deep', 'nested', 'd'].map($); \
         function clicked() { return inputs.map(i => { \
           const before = i.checked; i.click(); const changed = i.checked != before; i.checked = before; \
           return changed ? 1 : 0; }).join(''); } 0",
    );
    check(
        &mut env,
        &[
            // Its own `disabled`.
            ("clicked()", "11110"),
            // A fieldset's: not in its first legend.
            ("$('fs').disabled = true; clicked()", "10000"),
            // (The removed legend's input is in no fieldset.)
            ("var g1 = $('g1'); g1.remove(); clicked()", "11000"),
            ("$('fs').prepend(g1); clicked()", "10000"),
            (
                "$('inner').disabled = true; $('fs').disabled = false; clicked()",
                "11100",
            ),
            ("$('inner').append($('deep')); clicked()", "11000"),
            ("$('d').disabled = false; clicked()", "11001"),
            // Out of the fieldset: not its (what was found before kept no
            // longer).
            (
                "$('inner').disabled = false; $('fs').disabled = true; clicked()",
                "10001",
            ),
            ("var deep = $('deep'); deep.remove(); clicked()", "10101"),
            // Into one, never in a tree before: its.
            (
                "var fresh = document.createElement('input'); fresh.type = 'checkbox'; fresh.click(); \
                 $('fs').append(fresh); fresh.click(); fresh.checked",
                "true",
            ),
            // With a form too.
            (
                "var f = document.body.appendChild(document.createElement('form')); \
                 f.innerHTML = '<fieldset disabled><input type=checkbox id=fx></fieldset>'; var fx = $('fx'); \
                 fx.click(); var before = fx.checked; fx.remove(); fx.click(); [before, fx.checked].join()",
                "false,true",
            ),
            // `disabled` on another element: nothing.
            (
                "var dv = document.body.appendChild(document.createElement('div')); dv.setAttribute('disabled', ''); \
              listen(dv, ['click']); dv.click(); take()",
                "click:false",
            ),
        ],
    );
}

#[test]
fn a_fieldsets_disabling_is_kept_until_it_may_change() {
    let mut env = window_with_log();
    env.eval(
        "document.body.innerHTML = '<fieldset id=fs><div><input type=checkbox id=c></div></fieldset>'; \
         var c = document.getElementById('c'), fs = document.getElementById('fs'); 0",
    );
    let walks = dom::disabled_walks();
    env.eval("c.click(); c.click(); 0");
    assert_eq!(dom::disabled_walks(), walks + 1, "found once, kept");
    env.eval("document.body.setAttribute('title', 'x'); c.setAttribute('name', 'n'); c.click(); 0");
    assert_eq!(dom::disabled_walks(), walks + 1, "other changes keep it");
    env.eval("fs.disabled = true; c.click(); c.click(); 0");
    assert_eq!(dom::disabled_walks(), walks + 2, "the fieldset's change");
    env.eval("document.body.append(c); c.click(); 0");
    assert_eq!(dom::disabled_walks(), walks + 3, "moved");
    assert_eq!(
        env.eval("c.checked"),
        "false",
        "two clicks, one, none disabled, one"
    );
}
