//! HTML's `<select>` from script: the options' selectedness as the parser,
//! insertions, removals and attributes change it (the selectedness setting
//! algorithm: one selected, the last; the first that isn't disabled with
//! none), a select emptied and filled again in one change keeping its
//! selected option; the list of options (in optgroups, and in other
//! elements but a datalist, an `<hr>`, an option and an optgroup in an
//! optgroup); `options` (`HTMLOptionsCollection`), `length`, the indexed
//! getter and setter, `add`, `remove`, `selectedIndex`, `value`,
//! `selectedOptions`; an option's `text`, `label`, `value`, `index`,
//! `form`, `selected` and `new Option`; the pseudo-classes of an option;
//! a datalist's `options`, a progress's, a meter's and an output's values.

use crate::env::Env;

/// A window whose body is `html`.
fn page(html: &str) -> Env {
    let mut env = Env::window("about:blank");
    env.eval("document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); 0");
    env.eval(&format!("document.body.innerHTML = {html:?}; 0"));
    // A select's options' selectedness, as `1` and `0`.
    env.eval("sel = s => [...s.options].map(o => o.selected ? 1 : 0).join(''); 0");
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

#[test]
fn the_parsers_options_one_selected_the_last() {
    let mut env = page(
        "<select id=a><option>1<option>2<option>3</select>\
         <select id=b><option>1<option selected>2<option selected>3<option>4</select>\
         <select id=c size=2><option>1<option>2</select>\
         <select id=d multiple><option selected>1<option>2<option selected>3</select>\
         <select id=e><option disabled>1<option>2</select>",
    );
    check(
        &mut env,
        &[
            // None: the first.
            ("s = document.getElementById('a'); sel(s)", "100"),
            // Of several, the last.
            ("s = document.getElementById('b'); sel(s)", "0010"),
            // A list box selects none.
            ("s = document.getElementById('c'); sel(s)", "00"),
            // With `multiple`, as they say.
            ("s = document.getElementById('d'); sel(s)", "101"),
            // The first that isn't disabled.
            ("s = document.getElementById('e'); sel(s)", "01"),
        ],
    );
}

#[test]
fn insertions_and_removals_set_them_again() {
    let mut env = page("<select id=s><option>1<option>2</select>");
    check(
        &mut env,
        &[
            ("s = document.getElementById('s'); sel(s)", "10"),
            // One inserted selected unselects the others (`new Option`'s
            // selectedness is its last argument, not its `selected`
            // attribute).
            (
                "o = new Option('3', '3', true); s.appendChild(o); sel(s)",
                "100",
            ),
            (
                "o.remove(); o = new Option('3', '3', false, true); s.appendChild(o); sel(s)",
                "001",
            ),
            // Of a subtree with two, the last.
            (
                "g = document.createElement('optgroup'); \
                 for (let i = 0; i < 2; i++) { const o = document.createElement('option'); o.defaultSelected = true; g.appendChild(o); } \
                 s.appendChild(g); sel(s)",
                "00001",
            ),
            // The selected one removed: the first (the one the optgroup
            // unselected stays so).
            ("g.remove(); sel(s)", "100"),
            (
                "s.options[0].selected = false; s.options[2].selected = true; o.remove(); sel(s)",
                "10",
            ),
            // An unselected one removed: nothing.
            ("s.options[1].remove(); sel(s)", "1"),
            // Emptied and filled again in one change: it keeps its own.
            (
                "s.innerHTML = '<option>a<option selected>b<option>c'; const kept = [...s.options]; \
                 s.innerHTML = ''; kept.forEach(o => s.appendChild(o)); s.value",
                "b",
            ),
            // `textContent` too.
            (
                "s.textContent = ''; kept.forEach(o => s.appendChild(o)); s.value",
                "b",
            ),
            // A clone keeps its selectedness.
            (
                "s.options[1].cloneNode().selected + ' ' + s.options[0].cloneNode().selected",
                "true false",
            ),
            // One selected by script, with no attribute.
            (
                "s.options[0].selected = true; s.options[0].cloneNode().selected",
                "true",
            ),
        ],
    );
}

#[test]
fn attributes_and_script_set_them() {
    let mut env = page("<select id=s><option>1<option>2<option>3</select>");
    check(
        &mut env,
        &[
            (
                "s = document.getElementById('s'); o = s.options; sel(s)",
                "100",
            ),
            // A `selected` attribute, while it isn't dirty.
            ("o[2].setAttribute('selected', ''); sel(s)", "001"),
            ("o[2].removeAttribute('selected'); sel(s)", "100"),
            // Dirty: the attribute no longer does.
            ("o[1].selected = true; sel(s)", "010"),
            (
                "o[1].removeAttribute('selected'); o[1].setAttribute('selected', ''); o[2].defaultSelected = true; sel(s)",
                "001",
            ),
            ("o[2].selected = false; sel(s)", "100"),
            // `selectedIndex` and `value`: none for -1 or no such value.
            (
                "s.selectedIndex = 2; s.selectedIndex + ' ' + s.value",
                "2 3",
            ),
            (
                "s.selectedIndex = -1; s.selectedIndex + ' [' + s.value + ']'",
                "-1 []",
            ),
            ("s.value = '2'; s.selectedIndex", "1"),
            ("s.value = 'none'; s.selectedIndex", "-1"),
            // `multiple` removed, or the size changed: the algorithm.
            (
                "s.multiple = true; o[0].selected = o[2].selected = true; s.multiple = false; sel(s)",
                "001",
            ),
            (
                "s.size = 3; s.selectedIndex = -1; s.size = 1; sel(s)",
                "100",
            ),
            // With `multiple`, none selected stays so.
            (
                "m = document.createElement('select'); m.multiple = true; m.append(new Option('a'), new Option('b')); \
                 m.options[0].remove(); m.selectedIndex",
                "-1",
            ),
            // `selectedIndex`'s option is dirty: its attribute no longer
            // does.
            (
                "s.selectedIndex = 1; o[1].setAttribute('selected', ''); o[1].removeAttribute('selected'); s.selectedIndex",
                "1",
            ),
            // A disabled option and optgroup: one that isn't is selected.
            ("s.selectedIndex = -1; o[0].disabled = true; sel(s)", "010"),
            (
                "s.innerHTML = '<optgroup disabled><option>a</optgroup><option disabled>b'; s.selectedIndex + ' ' + s.options[0].selected",
                "-1 false",
            ),
            ("s.firstChild.disabled = false; s.selectedIndex", "0"),
            (
                "s.type + ' ' + (s.multiple = true, s.type)",
                "select-one select-multiple",
            ),
        ],
    );
}

#[test]
fn a_forms_reset_resets_its_selects() {
    let mut env =
        page("<form id=f><select id=s><option>1<option selected>2<option>3</select></form>");
    check(
        &mut env,
        &[
            (
                "s = document.getElementById('s'); s.selectedIndex = 2; s.options[0].selected = true; s.value",
                "1",
            ),
            ("document.getElementById('f').reset(); s.value", "2"),
            // Not dirty again: the attributes do.
            ("s.options[0].setAttribute('selected', ''); s.value", "1"),
            // `selectedIndex`'s option dirty: its attribute no longer does.
            (
                "s.selectedIndex = 2; s.options[2].setAttribute('selected', ''); s.options[2].removeAttribute('selected'); s.selectedIndex",
                "2",
            ),
        ],
    );
}

#[test]
fn the_list_of_options() {
    // The parser closes an optgroup in an optgroup, an option in an
    // option and a select in a select: script makes them.
    let mut env =
        page("<select id=s><option id=a>a<optgroup id=g><option id=b>b</optgroup></select>");
    env.eval(
        "const make = (name, id, parent) => { const e = document.createElement(name); if (id) e.id = id; parent.appendChild(e); return e; }; \
         s = document.getElementById('s'); \
         make('option', 'x', make('optgroup', '', document.getElementById('g'))); \
         const div = make('div', '', s); make('option', 'c', div); make('option', 'y', make('datalist', '', div)); \
         make('option', 'h', make('hr', '', div)); make('option', 'z', make('select', '', div)); \
         make('option', 'w', make('option', 'd', s)); 0",
    );
    check(
        &mut env,
        &[
            (
                "s = document.getElementById('s'); [...s.options].map(o => o.id).join()",
                "a,b,c,d",
            ),
            (
                "s.length + ' ' + s.options.length + ' ' + s[2].id + ' ' + s.item(3).id + ' ' + s[9]",
                "4 4 c d undefined",
            ),
            (
                "s.namedItem('b').id + ' ' + s.options.namedItem('c').id + ' ' + s.options.d.id",
                "b c d",
            ),
            (
                "s.options === s.options && s.options instanceof HTMLOptionsCollection && s.options instanceof HTMLCollection",
                "true",
            ),
            (
                "document.getElementById('c').index + ' ' + document.getElementById('y').index + ' ' + document.getElementById('x').index",
                "2 0 0",
            ),
            // Live: `selectedOptions` too.
            (
                "so = s.selectedOptions; [so.length, so[0].id, so === s.selectedOptions].join()",
                "1,a,true",
            ),
            (
                "s.multiple = true; s.options[3].selected = true; [...so].map(o => o.id).join()",
                "a,d",
            ),
            (
                "dl = document.querySelector('datalist'); dl.appendChild(document.createElement('span')); [...dl.options].map(o => o.id).join() + ' ' + (dl.options === dl.options)",
                "y true",
            ),
        ],
    );
}

#[test]
fn length_the_indexed_setter_add_and_remove() {
    let mut env = page("<select id=s><option>1<optgroup><option>2</optgroup></select>");
    check(
        &mut env,
        &[
            (
                "s = document.getElementById('s'); s.length = 4; s.length + ' ' + s.children.length + ' ' + s.lastChild.outerHTML",
                "4 4 <option></option>",
            ),
            (
                "s.options.length = 1; s.length + ' ' + s.innerHTML",
                "1 <option>1</option><optgroup></optgroup>",
            ),
            // Past Blink's limit: nothing.
            ("s.length = 100001; s.length", "1"),
            // The indexed setter: blank ones up to it, or the one there
            // replaced, null removing it.
            (
                "s[2] = new Option('x'); [...s.options].map(o => o.text).join()",
                "1,,x",
            ),
            (
                "s.options[0] = new Option('y'); [...s.options].map(o => o.text).join()",
                "y,,x",
            ),
            ("s[1] = null; [...s.options].map(o => o.text).join()", "y,x"),
            (
                "try { s[0] = document.createElement('div'); 'no' } catch (e) { e.name }",
                "TypeError",
            ),
            ("try { s[0] = 5; 'no' } catch (e) { e.name }", "TypeError"),
            // `add`: at the end, before an element, before the option at an
            // index (in its optgroup).
            (
                "s.add(new Option('z')); s.innerHTML = '<option>1<optgroup><option>2</optgroup>'; s.add(new Option('a'), 1); s.options[1].parentNode.localName",
                "optgroup",
            ),
            (
                "s.add(new Option('b'), s.options[0]); s.add(new Option('c'), null); s.add(new Option('d'), 99); [...s.options].map(o => o.text).join()",
                "b,1,a,2,c,d",
            ),
            (
                "g = document.createElement('optgroup'); s.options.add(g, 0); s.firstChild === g",
                "true",
            ),
            (
                "try { s.add(new Option('e'), document.createElement('option')); 'no' } catch (e) { e.name }",
                "NotFoundError",
            ),
            (
                "try { s.add(new Option('e'), document.body); 'no' } catch (e) { e.name }",
                "NotFoundError",
            ),
            (
                "try { s.add(document.createElement('div')); 'no' } catch (e) { e.name }",
                "TypeError",
            ),
            (
                "try { s.add(s.parentNode); 'no' } catch (e) { e.name }",
                "TypeError",
            ),
            // Before itself: nothing, not even a mutation.
            (
                "o = s.options[0]; mo = new MutationObserver(() => {}); mo.observe(s, { childList: true, subtree: true }); \
                 s.add(o, o); [s.options[0] === o, mo.takeRecords().length].join()",
                "true,0",
            ),
            // `remove(index)`, and `remove()` the select itself.
            (
                "s.remove(0); s.options.remove(0); [...s.options].map(o => o.text).join()",
                "a,2,c,d",
            ),
            ("s.remove(9); s.remove(-1); s.length", "4"),
            // An argument given, undefined: the option at 0.
            (
                "s.remove(undefined); [...s.options].map(o => o.text).join()",
                "2,c,d",
            ),
            (
                "s.options.selectedIndex = 2; s.value + ' ' + s.options.selectedIndex",
                "d 2",
            ),
            ("s.remove(); document.getElementById('s')", "null"),
        ],
    );
}

#[test]
fn an_options_members() {
    let mut env = page(
        "<form id=f><select id=s><optgroup><option id=o label=L value=V>  a <b>b</b><script>x</script>\n c </option></optgroup></select></form>\
         <option id=alone>z</option>",
    );
    check(
        &mut env,
        &[
            (
                "o = document.getElementById('o'); [o.text, o.label, o.value, o.index].join('|')",
                "a b c|L|V|0",
            ),
            (
                "o.removeAttribute('label'); o.removeAttribute('value'); [o.label, o.value].join('|')",
                "a b c|a b c",
            ),
            (
                "o.label = ''; o.label === '' && o.getAttribute('label') === ''",
                "true",
            ),
            ("o.text = 'new'; o.innerHTML", "new"),
            (
                "o.form.id + ' ' + document.getElementById('alone').form + ' ' + document.getElementById('alone').index",
                "f null 0",
            ),
            // `new Option`.
            (
                "n = new Option(); [n.text, n.hasAttribute('value'), n.defaultSelected, n.selected, n.childNodes.length].join()",
                ",false,false,false,0",
            ),
            (
                "n = new Option('t', 'v', true, false); [n.text, n.value, n.defaultSelected, n.selected].join()",
                "t,v,true,false",
            ),
            (
                "n = new Option('t', undefined, false, true); [n.hasAttribute('value'), n.selected].join()",
                "false,true",
            ),
            ("new Option('t', null).value", "null"),
            (
                "n instanceof HTMLOptionElement && Object.getPrototypeOf(n) === HTMLOptionElement.prototype && Option.prototype === HTMLOptionElement.prototype",
                "true",
            ),
            ("Option.name + ' ' + Option.length", "Option 0"),
            ("try { Option(); 'no' } catch (e) { e.name }", "TypeError"),
        ],
    );
}

#[test]
fn an_options_pseudo_classes() {
    let mut env = page(
        "<fieldset id=fs><select id=s><option id=a>a<option id=b selected>b\
         <optgroup id=g><option id=c>c</optgroup></select></fieldset>",
    );
    let matches = |ids: &str, selector: &str| {
        format!(
            "['{ids}'].join().split(',').map(id => document.getElementById(id).matches('{selector}') ? 1 : 0).join('')"
        )
    };
    check(
        &mut env,
        &[
            (matches("a,b,c", ":checked").as_str(), "010"),
            (matches("a,b,c", ":default").as_str(), "010"),
            (matches("a,b,c,g", ":enabled").as_str(), "1111"),
            ("document.getElementById('g').disabled = true; 0", "0"),
            (matches("a,b,c,g", ":disabled").as_str(), "0011"),
            (
                "document.getElementById('g').disabled = false; document.getElementById('s').disabled = true; 0",
                "0",
            ),
            (matches("a,b,c,g", ":disabled").as_str(), "1111"),
            (
                "document.getElementById('s').disabled = false; document.getElementById('fs').disabled = true; 0",
                "0",
            ),
            (matches("a,b,c,g", ":disabled").as_str(), "1111"),
        ],
    );
}

#[test]
fn progress_meter_and_output() {
    let mut env = page(
        "<form id=f><progress id=p></progress><meter id=m value=5 min=2 max=4 low=1 high=9></meter><output id=o>out</output></form>",
    );
    check(
        &mut env,
        &[
            (
                "p = document.getElementById('p'); [p.value, p.max, p.position].join()",
                "0,1,-1",
            ),
            (
                "p.value = 3; p.max = 4; [p.value, p.position, p.getAttribute('value')].join()",
                "3,0.75,3",
            ),
            ("p.max = -1; p.value = 9; [p.max, p.value].join()", "4,4"),
            (
                "p.setAttribute('max', 'x'); p.setAttribute('value', '-2'); [p.max, p.value, p.position].join()",
                "1,0,0",
            ),
            (
                "try { p.value = NaN; 'no' } catch (e) { e.name }",
                "TypeError",
            ),
            // A meter's are each clamped.
            (
                "m = document.getElementById('m'); [m.min, m.max, m.value, m.low, m.high, m.optimum].join()",
                "2,4,4,2,4,3",
            ),
            (
                "m.max = 1; m.optimum = 0.5; [m.max, m.value, m.optimum, m.getAttribute('optimum')].join()",
                "2,2,2,0.5",
            ),
            // An output's value and default value.
            (
                "o = document.getElementById('o'); [o.type, o.value, o.defaultValue].join()",
                "output,out,out",
            ),
            ("o.defaultValue = 'def'; o.value", "def"),
            (
                "o.value = 'v'; [o.value, o.defaultValue, o.textContent].join()",
                "v,def,v",
            ),
            (
                "o.defaultValue = 'd2'; [o.value, o.defaultValue].join()",
                "v,d2",
            ),
            (
                "document.getElementById('f').reset(); [o.value, o.defaultValue].join()",
                "d2,d2",
            ),
            ("o.defaultValue = 'd3'; o.value", "d3"),
        ],
    );
}
