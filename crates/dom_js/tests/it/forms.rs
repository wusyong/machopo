//! HTML's forms from script: an HTML element's interface by its name (the
//! form controls' own, `HTMLElement` for a name HTML defines with none and
//! for a custom element's, `HTMLUnknownElement` for any other), and the
//! controls' reflected attributes by HTML's rules (strings, booleans,
//! non-negative and positive numbers, with their defaults and errors); a
//! listed element's form owner (its nearest ancestor form, the one its
//! `form` attribute names, the parser's), reset as the tree and the ids
//! change; a form's `elements` in tree order, kept until an owner changes,
//! `length`, `form[i]`, and the elements by name (`form[name]` and its past
//! names, `elements.namedItem`, a `RadioNodeList` of several); a
//! fieldset's `elements` and a legend's `form`; a label's labeled control
//! (by `for`, or its first labelable descendant) and `form`, and a
//! labelable element's `labels`, live.

use crate::env::Env;

/// A window whose document has an `<html>` and a `<body>`.
fn window_with_body() -> Env {
    let mut env = Env::window("about:blank");
    env.eval("document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); 0");
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn an_html_elements_interface_is_its_names() {
    let mut env = Env::window("about:blank");
    check(
        &mut env,
        &[
            (
                "['form', 'input', 'button', 'select', 'option', 'optgroup', 'textarea', 'label', 'fieldset', 'legend', \
                  'output', 'datalist', 'progress', 'meter'] \
                 .map(name => document.createElement(name).constructor.name).join()",
                "HTMLFormElement,HTMLInputElement,HTMLButtonElement,HTMLSelectElement,HTMLOptionElement,\
                 HTMLOptGroupElement,HTMLTextAreaElement,HTMLLabelElement,HTMLFieldSetElement,HTMLLegendElement,\
                 HTMLOutputElement,HTMLDataListElement,HTMLProgressElement,HTMLMeterElement",
            ),
            // HTML's names with no interface of their own (obsolete ones
            // too), a custom element's: `HTMLElement`; others unknown.
            (
                "['section', 'b', 'acronym', 'x-y', 'foo', 'applet', 'blink', 'circle', 'Section'] \
                 .map(name => document.createElement(name).constructor.name).join()",
                "HTMLElement,HTMLElement,HTMLElement,HTMLElement,HTMLUnknownElement,HTMLUnknownElement,\
                 HTMLUnknownElement,HTMLUnknownElement,HTMLElement",
            ),
            (
                "var u = document.createElement('foo'); \
                 [u instanceof HTMLElement, Object.getPrototypeOf(HTMLUnknownElement.prototype) === HTMLElement.prototype].join()",
                "true,true",
            ),
            // The parser's alike; another namespace's not an HTML element.
            (
                "var d = document.createElement('div'); d.innerHTML = '<input><foo></foo><svg><input/></svg>'; \
                 [d.firstChild.constructor.name, d.childNodes[1].constructor.name, \
                  d.lastChild.firstChild instanceof HTMLElement].join()",
                "HTMLInputElement,HTMLUnknownElement,false",
            ),
        ],
    );
}

#[test]
fn form_controls_reflect_their_attributes() {
    let mut env = Env::window("about:blank");
    env.eval(
        "function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } \
         var i = document.createElement('input'), t = document.createElement('textarea'), \
         s = document.createElement('select'); 0",
    );
    check(
        &mut env,
        &[
            // Strings and booleans.
            (
                "[JSON.stringify(i.name), i.disabled, i.required].join()",
                "\"\",false,false",
            ),
            (
                "i.name = 'n'; i.disabled = true; i.placeholder = 'p'; \
                 [i.getAttribute('name'), i.getAttribute('disabled'), i.getAttribute('placeholder')].join()",
                "n,,p",
            ),
            ("i.disabled = false; i.hasAttribute('disabled')", "false"),
            // The `default*` ones and `htmlFor` by their attributes' names.
            (
                "i.defaultChecked = true; i.defaultValue = 'v'; var o = document.createElement('option'); o.defaultSelected = true; \
                 var l = document.createElement('label'); l.htmlFor = 'x'; \
                 [i.hasAttribute('checked'), i.getAttribute('value'), o.hasAttribute('selected'), l.getAttribute('for')].join()",
                "true,v,true,x",
            ),
            // `long`s limited to non-negative: -1 by default and for what
            // isn't one; setting a negative one an `IndexSizeError`.
            ("[i.maxLength, t.minLength].join()", "-1,-1"),
            ("i.setAttribute('maxlength', ' 12abc'); i.maxLength", "12"),
            ("i.setAttribute('maxlength', '-3'); i.maxLength", "-1"),
            (
                "[error(() => i.maxLength = -1), i.getAttribute('maxlength')].join()",
                "IndexSizeError,-3",
            ),
            ("i.maxLength = 7; i.getAttribute('maxlength')", "7"),
            // `unsigned long`s: past 2147483647 the default.
            (
                "[s.size, (s.size = 3, s.getAttribute('size')), (s.size = 2147483648, s.getAttribute('size'))].join()",
                "0,3,0",
            ),
            // Limited to positive: 0 counts as missing; setting 0 an error,
            // or with fallback the default.
            (
                "[i.size, (i.setAttribute('size', '0'), i.size), error(() => i.size = 0)].join()",
                "20,20,IndexSizeError",
            ),
            (
                "[t.cols, t.rows, (t.cols = 0, t.getAttribute('cols')), (t.rows = 5, t.rows)].join()",
                "20,2,20,5",
            ),
        ],
    );
}

#[test]
fn a_listed_elements_owner_is_its_nearest_ancestor_form() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.createElement('form'), g = document.createElement('form'), \
         d = document.createElement('div'), i = document.createElement('input'); 0",
    );
    check(
        &mut env,
        &[
            // Out of a document too.
            ("d.appendChild(i); f.appendChild(d); i.form === f", "true"),
            // Each listed element has one.
            (
                "['button', 'fieldset', 'output', 'select', 'textarea'] \
                 .map(name => d.appendChild(document.createElement(name)).form === f).join()",
                "true,true,true,true,true",
            ),
            ("document.body.appendChild(f); i.form === f", "true"),
            // The nearest: a form in a form (only script makes one).
            ("g.appendChild(i); f.appendChild(g); i.form === g", "true"),
            ("f.appendChild(i); i.form === f", "true"),
            // Removed with its form: it stays; without: none.
            ("f.remove(); i.form === f", "true"),
            ("i.remove(); i.form", "null"),
            // A form's subtree moved: the owner is the new ancestor's.
            (
                "g.remove(); g.appendChild(i); d.remove(); d.appendChild(g); i.form === g",
                "true",
            ),
            (
                "g.remove(); f.appendChild(d); d.appendChild(i); i.form === f",
                "true",
            ),
            // Not through a shadow root to its host.
            (
                "var host = f.appendChild(document.createElement('div')); \
                 var root = host.attachShadow({mode: 'open'}); root.innerHTML = '<input>'; \
                 root.firstChild.form",
                "null",
            ),
        ],
    );
}

#[test]
fn a_form_attribute_names_the_owner() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.body.appendChild(document.createElement('form')); \
         var g = document.createElement('form'); g.id = 'g'; \
         var i = f.appendChild(document.createElement('input')); 0",
    );
    check(
        &mut env,
        &[
            // A form not in the document yet: none (not the ancestor).
            ("i.setAttribute('form', 'g'); i.form", "null"),
            ("document.body.appendChild(g); i.form === g", "true"),
            // The id changed or gone: none; back, and on another form.
            ("g.id = 'h'; i.form", "null"),
            ("g.id = 'g'; i.form === g", "true"),
            ("g.remove(); i.form", "null"),
            // The first element with the id in tree order, if a form.
            (
                "document.body.appendChild(g); f.id = 'g'; i.form === f",
                "true",
            ),
            ("document.body.insertBefore(g, f); i.form === g", "true"),
            (
                "var p = document.createElement('p'); p.id = 'g'; document.body.prepend(p); i.form",
                "null",
            ),
            ("p.remove(); i.form === g", "true"),
            // Out of the document the attribute doesn't count: the
            // ancestor's.
            ("f.remove(); i.form === f", "true"),
            ("document.body.appendChild(f); i.form === g", "true"),
            // Not in a tree that isn't a document's, though a form there has
            // the id.
            (
                "var lone = document.createElement('div'); lone.innerHTML = '<form id=q></form><input form=q>'; lone.lastChild.form",
                "null",
            ),
            // Its value changed, and removed: the ancestor's again.
            ("i.setAttribute('form', 'nothing'); i.form", "null"),
            ("i.removeAttribute('form'); i.form === f", "true"),
            ("g.id = 'x'; i.form === f", "true"),
            // In a shadow tree: the shadow root's ids.
            (
                "var host = document.body.appendChild(document.createElement('div')); \
                 var root = host.attachShadow({mode: 'open'}); \
                 root.innerHTML = '<form id=s></form><input form=s><input form=g>'; \
                 [root.children[1].form === root.firstChild, root.children[2].form].join()",
                "true,",
            ),
        ],
    );
}

#[test]
fn a_forms_elements_are_its_listed_elements_in_tree_order() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.body.appendChild(document.createElement('form')); f.id = 'f'; \
         f.innerHTML = '<input id=a><div><select id=b></select><input type=image id=c><p></p></div>\
         <fieldset id=d><textarea id=e></textarea></fieldset><object id=o></object><label>x</label>'; \
         var ids = list => Array.prototype.map.call(list, e => e.id).join(); 0",
    );
    check(
        &mut env,
        &[
            // Image buttons aren't among them; listed elements are.
            ("ids(f.elements)", "a,b,d,e,o"),
            (
                "[f.length, f[1].id, f[5], f.elements[4].id, f.elements.item(0).id].join()",
                "5,b,,o,a",
            ),
            ("f.elements === f.elements", "true"),
            (
                "f.elements instanceof HTMLFormControlsCollection && f.elements instanceof HTMLCollection",
                "true",
            ),
            // `form[i]` is the indexed getter alone, and a form iterates.
            (
                "['indexedGetter' in f, '' in HTMLFormElement.prototype, Array.from(f).length].join()",
                "false,false,5",
            ),
            // Live: as owners change.
            (
                "var x = document.createElement('button'); x.id = 'x'; f.querySelector('p').appendChild(x); ids(f.elements)",
                "a,b,x,d,e,o",
            ),
            (
                "f.querySelector('div').remove(); ids(f.elements)",
                "a,d,e,o",
            ),
            // One outside, by its `form` attribute, in tree order.
            (
                "var y = document.createElement('input'); y.id = 'y'; y.setAttribute('form', 'f'); \
                 document.body.prepend(y); ids(f.elements)",
                "y,a,d,e,o",
            ),
            ("document.body.appendChild(y); ids(f.elements)", "a,d,e,o,y"),
            // An image button's `type` changed: in.
            (
                "var c = f.appendChild(document.createElement('input')); c.setAttribute('type', 'IMAGE'); c.id = 'c'; \
                 ids(f.elements)",
                "a,d,e,o,y",
            ),
            (
                "c.setAttribute('type', 'text'); ids(f.elements)",
                "a,d,e,o,c,y",
            ),
        ],
    );
}

#[test]
fn a_form_keeps_its_elements_until_an_owner_changes() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.body.appendChild(document.createElement('form')); \
         f.innerHTML = '<input><input><input>'; var e = f.elements; e.length; 0",
    );
    let walks = dom::form_walks();
    env.eval("for (var n = 0; n < 10; n++) { e.length; e[2]; f[1]; f.length; } 0");
    assert_eq!(dom::form_walks(), walks, "kept: no walk");
    // A change elsewhere, or of an attribute: still kept.
    env.eval("document.body.appendChild(document.createElement('div')); f.firstChild.name = 'n'; e.length");
    assert_eq!(dom::form_walks(), walks, "no owner changed");
    env.eval("f.appendChild(document.createElement('input')); e.length; e.length");
    assert_eq!(
        dom::form_walks(),
        walks + 1,
        "an owner changed: found again once"
    );
}

#[test]
fn a_forms_elements_by_name() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.body.appendChild(document.createElement('form')); \
         f.innerHTML = '<input name=a id=one><input id=a><input name=b><input type=image name=c>\
         <input name=tagName><img name=i>'; 0",
    );
    check(
        &mut env,
        &[
            // One: the element; several, by `id` or `name`: a live list.
            (
                "f.elements.namedItem('b') === f.children[2] && f.b === f.children[2] && f.elements.b === f.b",
                "true",
            ),
            (
                "var list = f.elements.namedItem('a'); [list.constructor.name, list instanceof NodeList, list.length].join()",
                "RadioNodeList,true,2",
            ),
            ("f.a.length + ',' + f.elements.a[1].id", "2,a"),
            (
                "f.children[1].id = 'z'; [list.length, f.a === f.children[0]].join()",
                "1,true",
            ),
            (
                "[f.elements.namedItem('c'), f.elements.namedItem(''), f.elements.namedItem('none'), f.none, f.i].join()",
                ",,,,",
            ),
            // `[LegacyOverrideBuiltIns]`: over the prototype's.
            ("f.tagName === f.children[4]", "true"),
            // The past names map: a name found one element by finds it
            // still, while it is the form's.
            ("var b = f.b; b.name = 'bb'; f.b === b", "true"),
            // Another found by it replaces it, and takes the entry when it
            // goes.
            (
                "var other = document.createElement('input'); other.name = 'b'; f.appendChild(other); f.b === other",
                "true",
            ),
            ("other.remove(); f.b", "undefined"),
            (
                "[f.bb === b, (b.name = 'b3', f.bb === b)].join()",
                "true,true",
            ),
            // Its element no longer the form's: gone, not back with it.
            ("b.remove(); f.bb", "undefined"),
            ("f.appendChild(b); f.bb", "undefined"),
        ],
    );
}

#[test]
fn the_parsers_form_element_pointer_associates() {
    let mut env = window_with_body();
    check(
        &mut env,
        &[
            // The form closed with the `<div>`, its pointer still set: the
            // `<input>` after is its, out of it.
            (
                "var d = Document.parseHTMLUnsafe('<div><form id=f></div><input id=i><p><select id=s></select>'); \
                 var f = d.getElementById('f'), i = d.getElementById('i'); \
                 [i.form === f, d.getElementById('s').form === f, f.contains(i), f.elements.length].join()",
                "true,true,false,2",
            ),
            // Removed out of its owner's tree: the ancestor's (none), its
            // flag unset (into a form, its).
            ("d.body.appendChild(i); i.form", "null"),
            (
                "var g = d.body.appendChild(d.createElement('form')); g.appendChild(i); i.form === g",
                "true",
            ),
            ("d.body.appendChild(i); i.form", "null"),
            ("f.elements.length", "1"),
            // The form removed: those outside it lose it.
            ("f.remove(); d.getElementById('s').form", "null"),
            // Its parser inserted flag stays set as it is inserted with its
            // owner (HTML's: only a reset unsets it).
            (
                "var t3 = Document.parseHTMLUnsafe('<table><form id=f><tr><td><input id=i></table><div id=two></div>'); \
                 var f3 = t3.getElementById('f'), i3 = t3.getElementById('i'), two = t3.getElementById('two'); \
                 var was = i3.form === f3 && !f3.contains(i3); two.appendChild(f3.parentNode); \
                 [was, i3.form === f3, (two.appendChild(i3), i3.form)].join()",
                "true,true,",
            ),
            // A fragment's parse has no pointer.
            (
                "var t = document.createElement('div'); t.innerHTML = '<div><form></div><input>'; t.lastChild.form",
                "null",
            ),
        ],
    );
}

#[test]
fn a_documents_forms() {
    let mut env = window_with_body();
    check(
        &mut env,
        &[
            (
                "document.body.innerHTML = '<form id=a></form><div><form name=b></form></div>'; \
                 var forms = document.forms; [forms.length, forms[1].name, forms === document.forms].join()",
                "2,b,true",
            ),
            // Live, HTML's `form`s alone; by name or id (`HTMLCollection`'s
            // named getter, which any collection has).
            (
                "document.body.appendChild(document.createElementNS('http://www.w3.org/2000/svg', 'form')); \
                 document.body.appendChild(document.createElement('form')); \
                 [forms.length, forms.a === forms[0], forms.b === forms[1], forms.c, forms.namedItem('b') === forms.b].join()",
                "3,true,true,,true",
            ),
            (
                "[document.body.children.a === forms.a, 'a' in forms, 'length' in forms].join()",
                "true,true,true",
            ),
        ],
    );
}

#[test]
fn a_labels_control_is_for_or_its_first_labelable_descendant() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.body.appendChild(document.createElement('form')); \
         f.innerHTML = '<label id=l><span><input type=hidden id=h><b><textarea id=t></textarea></b></span>\
           <input id=i></label><input id=x><div id=d></div><meter id=m></meter>'; \
         var l = document.getElementById('l'); 0",
    );
    check(
        &mut env,
        &[
            // No `for`: the first labelable descendant (a hidden input
            // isn't).
            ("l.control.id", "t"),
            ("l.form === f", "true"),
            // `for`: the element of that id if labelable, whatever is in it.
            ("l.htmlFor = 'x'; l.control.id", "x"),
            ("l.htmlFor = 'd'; l.control", "null"),
            ("l.htmlFor = 'nothing'; l.control", "null"),
            ("l.htmlFor = 'm'; [l.control.id, l.form].join()", "m,"),
            ("l.htmlFor = 'h'; l.control", "null"),
            (
                "document.getElementById('h').setAttribute('type', 'text'); l.control.id",
                "h",
            ),
            // The first element of the id, not a labelable one after it.
            (
                "var dup = document.createElement('div'); dup.id = 'x'; f.prepend(dup); l.htmlFor = 'x'; l.control",
                "null",
            ),
            ("dup.remove(); l.control.id", "x"),
            // (The input before is no longer hidden.)
            ("l.removeAttribute('for'); l.control.id", "h"),
            // Out of a document: its own tree's.
            (
                "var o = document.createElement('div'); \
                 o.innerHTML = '<label for=y></label><input id=y><label for=x></label>'; \
                 [o.firstChild.control === o.childNodes[1], o.lastChild.control].join()",
                "true,",
            ),
            (
                "var e = document.createElement('label'); [e.control, e.form].join()",
                ",",
            ),
        ],
    );
}

#[test]
fn a_labelable_elements_labels() {
    let mut env = window_with_body();
    env.eval(
        "document.body.innerHTML = '<label id=a for=i></label><label id=b><input id=i></label><label id=c></label>\
           <button id=bu></button><meter id=me></meter><label for=me id=ml></label>'; \
         var i = document.getElementById('i'), ls = i.labels; 0",
    );
    check(
        &mut env,
        &[
            // In tree order, the same each time.
            (
                "[ls instanceof NodeList, ls === i.labels, ls.length, ls[0].id, ls.item(1).id, ls[2]].join()",
                "true,true,2,a,b,",
            ),
            // Live.
            (
                "document.getElementById('c').htmlFor = 'i'; [ls.length, ls[2].id].join()",
                "3,c",
            ),
            (
                "document.getElementById('a').remove(); [ls.length, ls[0].id].join()",
                "2,b",
            ),
            ("document.getElementById('c').htmlFor = ''; ls.length", "1"),
            // A hidden input has none; the same list again after.
            ("i.setAttribute('type', 'HIDDEN'); i.labels", "null"),
            (
                "i.setAttribute('type', 'text'); [i.labels === ls, ls.length].join()",
                "true,1",
            ),
            // The other labelable elements', not other elements'.
            ("document.getElementById('bu').labels.length", "0"),
            (
                "var ml = document.getElementById('me').labels; [ml.length, ml[0].id].join()",
                "1,ml",
            ),
            (
                "[document.createElement('div').labels, document.createElement('option').labels].join()",
                ",",
            ),
            // A label in another tree isn't its.
            (
                "var o = document.createElement('label'); o.htmlFor = 'i'; ls.length",
                "1",
            ),
            // Out of a document: its own tree's.
            (
                "var d = document.createElement('div'); d.innerHTML = '<label for=z></label><input id=z>'; \
                 d.lastChild.labels.length",
                "1",
            ),
        ],
    );
}

#[test]
fn a_fieldsets_elements_and_a_legends_form() {
    let mut env = window_with_body();
    env.eval(
        "var f = document.body.appendChild(document.createElement('form')); f.id = 'f'; \
         f.innerHTML = '<fieldset id=s><legend id=g></legend><label><input id=a></label>\
           <fieldset id=n><output id=o></output></fieldset><input type=image id=im><select id=se><option></option></select>\
           <object id=ob></object><div><textarea id=t></textarea></div></fieldset><legend id=out></legend>'; \
         var s = document.getElementById('s'), es = s.elements; 0",
    );
    check(
        &mut env,
        &[
            // Its listed descendants in tree order (an image button too),
            // the same each time; not itself.
            (
                "[es instanceof HTMLCollection, es instanceof HTMLFormControlsCollection, es === s.elements].join()",
                "true,false,true",
            ),
            ("Array.from(es, e => e.id).join()", "a,n,o,im,se,ob,t"),
            (
                "[es.length, es[1].id, es.namedItem('t').id].join()",
                "7,n,t",
            ),
            ("s.type", "fieldset"),
            // Live.
            (
                "s.appendChild(document.createElement('button')); s.querySelector('div').remove(); es.length",
                "7",
            ),
            ("es[6].localName", "button"),
            // A legend's form: its fieldset's owner; none out of a fieldset.
            ("document.getElementById('g').form === f", "true"),
            ("document.getElementById('out').form", "null"),
            // (A parent with an owner, not a fieldset.)
            (
                "var lg = document.getElementById('o').appendChild(document.createElement('legend')); \
                 [document.getElementById('o').form === f, lg.form].join()",
                "true,",
            ),
            (
                "var o = document.createElement('form'); o.id = 'other'; document.body.append(o); \
              s.setAttribute('form', 'other'); document.getElementById('g').form === o",
                "true",
            ),
            ("document.createElement('legend').form", "null"),
        ],
    );
}

/// The parser's form owners (its form element pointer's, set as each
/// control is made, before it is in its form's tree): once inserted, each
/// is among its form's `elements`, and a form's default button is its
/// first submit button, found again as one comes or changes its type (an
/// image button is one, though not among `elements`).
#[test]
fn parser_associated_controls_and_the_default_button() {
    let mut page = crate::history::Page::new("https://example.test/");
    page.load();
    assert_eq!(
        page.eval(
            "document.body.innerHTML = '<form><button id=a type=button></button><button id=b></button></form>' + \
               '<form><input type=submit id=c><input type=submit id=d></form><form><input type=image id=e></form>' + \
               '<button id=f></button>'; \
             var ids = () => [...document.querySelectorAll(':default')].map(e => e.id).join(); \
             var before = ids(); document.getElementById('a').type = 'submit'; \
             [before, ids(), [...document.forms].map(f => f.elements.length).join()].join(' | ')"
        ),
        "b,c,e | a,c,e | 2,2,0"
    );
}
