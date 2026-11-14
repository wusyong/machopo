//! WebIDL's supported property indices: what `index in object`, a
//! property's descriptor and an object's keys tell of each interface with
//! an indexed getter. They are the indices below its `length`, read alone:
//! no item is made to tell, and no scope.

use crate::env::Env;

/// A window with one of each kind of indexed object reachable from it.
fn page() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('head')); \
         document.documentElement.appendChild(document.createElement('body')); 0",
    );
    env.eval(
        "const style = document.createElement('style'); \
         style.media = 'screen, print'; \
         style.textContent = 'p { color: red } i { top: 0 } @keyframes k { from { top: 0 } to { top: 1px } }'; \
         document.head.appendChild(style); \
         document.body.innerHTML = \"<p id=a class='x y z' title=t style='color: red; top: 0'>one</p><p>two</p>\" + \
             '<form><input><input><button></button></form>' + \
             '<select><option>1<option>2<optgroup><option>3</optgroup></select>'; 0",
    );
    // What is wrong with `o`'s indexed properties, after its length.
    env.eval(
        "check = (o, writable) => { \
           const n = o.length, wrong = []; \
           for (let i = 0; i < n; i++) { \
             if (!(i in o)) wrong.push('not in: ' + i); \
             if (!Object.prototype.hasOwnProperty.call(o, i)) wrong.push('not own: ' + i); \
             const v = o[i]; \
             if (v === undefined || v === null || v === '') wrong.push('no item: ' + i); \
             const d = Object.getOwnPropertyDescriptor(o, i); \
             if (!d || d.value !== v || d.writable !== writable || !d.enumerable || !d.configurable) \
               wrong.push('descriptor: ' + i); \
           } \
           for (const past of [n, n + 1, 4294967294]) { \
             if (past in o) wrong.push('in: ' + past); \
             if (o[past] !== undefined) wrong.push('item: ' + past); \
             if (Object.getOwnPropertyDescriptor(o, past)) wrong.push('descriptor: ' + past); \
           } \
           const keys = Object.keys(o).filter(k => /^[0-9]+$/.test(k)).join(); \
           if (keys !== [...Array(n).keys()].join()) wrong.push('keys: ' + keys); \
           return n + ' ' + wrong.join('; '); \
         }; 0",
    );
    env
}

/// Every interface with an indexed getter: each index below the length is
/// a property with an item, none at or past it is, and the keys are those
/// indices; writable where the interface has an indexed setter.
#[test]
fn the_indices_below_the_length_are_the_properties() {
    let mut env = page();
    for (object, writable, length) in [
        ("document.querySelectorAll('p')", false, 2),
        ("document.body.childNodes", false, 4),
        ("document.body.children", false, 4),
        ("document.getElementsByTagName('p')", false, 2),
        ("document.all", false, 15),
        ("document.getElementById('a').classList", false, 3),
        ("document.getElementById('a').attributes", false, 4),
        ("document.getElementById('a').style", false, 2),
        ("getComputedStyle(document.getElementById('a'))", false, -1),
        ("document.styleSheets", false, 1),
        ("document.styleSheets[0].cssRules", false, 3),
        ("document.styleSheets[0].media", false, 2),
        ("document.styleSheets[0].cssRules[2]", false, 2),
        ("document.forms[0]", false, 3),
        ("document.forms[0].elements", false, 3),
        ("document.querySelector('select')", true, 3),
        ("document.querySelector('select').options", true, 3),
        ("document.querySelector('select').selectedOptions", false, 1),
        ("document.getElementById('a').getClientRects()", false, -1),
        ("window", false, 0),
    ] {
        let found = env.eval(&format!("check({object}, {writable})"));
        let (count, wrong) = found.split_once(' ').expect("a count and what is wrong");
        assert_eq!(wrong, "", "{object}");
        // A length this test doesn't fix (the computed style's properties,
        // the rectangles of a page with no layout) is any.
        if length >= 0 {
            assert_eq!(count, length.to_string(), "{object}: its length");
        }
    }
}

/// The properties follow the length as it changes.
#[test]
fn an_index_is_a_property_while_it_is_below_the_length() {
    let mut env = page();
    for (source, value) in [
        ("list = document.body.childNodes; 3 in list", "true"),
        ("4 in list", "false"),
        (
            "document.body.appendChild(document.createElement('i')); 4 in list",
            "true",
        ),
        ("Object.keys(list).join()", "0,1,2,3,4"),
        ("document.body.lastChild.remove(); 4 in list", "false"),
        (
            "classes = document.getElementById('a').classList; 2 in classes",
            "true",
        ),
        ("classes.remove('z'); 2 in classes", "false"),
        ("Object.keys(classes).join()", "0,1"),
        // A setter's interface: the property past the end isn't one until
        // it is set.
        (
            "select = document.querySelector('select'); 3 in select",
            "false",
        ),
        ("select[3] = new Option('4'); 3 in select", "true"),
        (
            "Object.getOwnPropertyDescriptor(select, 3).writable",
            "true",
        ),
    ] {
        assert_eq!(env.eval(source), value, "{source}");
    }
}

/// Asking whether an index is there makes no scope: V8's callback reads
/// the length and answers. Listing the keys makes one, for their array.
#[test]
fn a_query_makes_no_scope_and_the_keys_one() {
    let mut env = page();
    env.eval("list = document.body.childNodes; nodes = [...list]; 0");
    for source in [
        "0 in list",
        "9 in list",
        "Object.prototype.hasOwnProperty.call(list, 1)",
        "Reflect.has(list, 2)",
    ] {
        let before = crate::env::slow_paths();
        env.eval(source);
        assert_eq!(crate::env::slow_paths(), before, "{source}");
    }
    let before = crate::env::info_scopes();
    assert_eq!(env.eval("Object.keys(list).length"), "4");
    assert_eq!(crate::env::info_scopes() - before, 1, "the keys' array");
}
