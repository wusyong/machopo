//! Finding elements from script (`getElementById`, `querySelector(All)`'s
//! static `NodeList` and its iterator, `matches`, `closest`), an element's
//! `classList` (its tokens, iterator and `[PutForwards]`), and `dataset`'s
//! named properties.

use crate::env::Env;

const TREE: &str = "var html = document.createElement('html'); document.appendChild(html); \
     var div = document.createElement('div'); div.id = 'a'; div.className = 'x'; html.appendChild(div); \
     var p1 = document.createElement('p'); p1.className = 'x y'; div.appendChild(p1); \
     var p2 = document.createElement('p'); p2.id = 'a'; div.appendChild(p2);";

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (source, expected) in cases {
        assert_eq!(env.eval(source), *expected, "{source}");
    }
}

#[test]
fn queries_from_script() {
    let mut env = Env::window("about:blank");
    env.eval(TREE);
    check(
        &mut env,
        &[
            ("document.getElementById('a') === div", "true"),
            ("document.getElementById('nope')", "null"),
            ("document.querySelector('.x') === div", "true"),
            ("div.querySelector('.x') === p1", "true"),
            ("div.querySelector(':scope > #a') === p2", "true"),
            (
                "p2.closest('.x') === div && p1.closest('.y') === p1",
                "true",
            ),
            (
                "[p1.matches('div > .x'), p1.webkitMatchesSelector('#a')].join()",
                "true,false",
            ),
            (
                "try { document.querySelector('p['); } catch (e) { [e instanceof DOMException, e.name, e.code].join() }",
                "true,SyntaxError,12",
            ),
            (
                "var all = document.querySelectorAll('p, div'); [all instanceof NodeList, all.length, all[0] === div, all.item(2) === p2, all[3]].join()",
                "true,3,true,true,",
            ),
            ("Object.prototype.toString.call(all)", "[object NodeList]"),
            (
                "[...all].length + ',' + Array.from(all.keys()).join(' ')",
                "3,0 1 2",
            ),
            (
                "var n = []; all.forEach((e, i) => n.push(e.localName + i)); n.join()",
                "div0,p1,p2",
            ),
            (
                "NodeList.prototype.forEach === Array.prototype.forEach",
                "true",
            ),
            ("[...all.entries()][1][1] === p1", "true"),
            // Static.
            ("div.removeChild(p1); all.length", "3"),
            (
                "document.querySelectorAll('p') === document.querySelectorAll('p')",
                "false",
            ),
        ],
    );
}

#[test]
fn class_list_from_script() {
    let mut env = Env::window("about:blank");
    env.eval("var e = document.createElement('div'); e.className = 'a b a';");
    check(
        &mut env,
        &[
            (
                "var list = e.classList; list instanceof DOMTokenList && e.classList === list",
                "true",
            ),
            (
                "[list.length, list[0], list[1], list[2], list.item(9)].join()",
                "2,a,b,,",
            ),
            ("[...list].join(' ') + '|' + String(list)", "a b|a b a"),
            ("list.add('c', 'd'); e.className", "a b c d"),
            ("list.remove('a', 'zz'); e.className", "b c d"),
            (
                "[list.toggle('b'), list.toggle('q', true), list.toggle('q', false), e.className].join()",
                "false,true,false,c d",
            ),
            ("[list.replace('c', 'z'), e.className].join()", "true,z d"),
            (
                "[list.contains('z'), list.contains('c')].join()",
                "true,false",
            ),
            (
                "try { list.add('ok', ''); } catch (x) { x.name + ' ' + e.className }",
                "SyntaxError z d",
            ),
            (
                "try { list.toggle('a b'); } catch (x) { x.name }",
                "InvalidCharacterError",
            ),
            (
                "try { list.supports('a'); } catch (x) { x instanceof TypeError }",
                "true",
            ),
            (
                "e.classList = 'm n'; [e.className, list.length].join()",
                "m n,2",
            ),
            ("list.value = 'v'; e.className", "v"),
            (
                "DOMTokenList.prototype.add.length + ',' + DOMTokenList.prototype.toggle.length",
                "0,1",
            ),
        ],
    );
}

#[test]
fn dataset_from_script() {
    let mut env = Env::window("about:blank");
    env.eval("var e = document.createElement('div'); e.setAttribute('data-foo-bar', '1'); e.setAttribute('data-to-string', 't'); e.id = 'i';");
    check(
        &mut env,
        &[
            (
                "var d = e.dataset; d instanceof DOMStringMap && e.dataset === d",
                "true",
            ),
            ("[d.fooBar, d['foo-bar'], d.nothing].join()", "1,,"),
            ("'fooBar' in d && !('nothing' in d)", "true"),
            ("Object.keys(d).join()", "fooBar,toString"),
            // `[LegacyOverrideBuiltIns]`: a name shadows the prototype's.
            ("d.toString", "t"),
            ("d.newOne = 'v'; e.getAttribute('data-new-one')", "v"),
            (
                "d[9] = 'nine'; [e.getAttribute('data-9'), d[9]].join()",
                "nine,nine",
            ),
            (
                "try { d['a-b'] = 'x'; } catch (x) { x.name }",
                "SyntaxError",
            ),
            (
                "delete d.fooBar; [e.hasAttribute('data-foo-bar'), 'fooBar' in d].join()",
                "false,false",
            ),
            (
                "Object.defineProperty(d, 'viaDefine', { value: 'w' }); e.getAttribute('data-via-define')",
                "w",
            ),
            (
                "try { Object.defineProperty(d, 'g', { get() { return 1; } }); 'no' } catch (x) { x instanceof TypeError }",
                "true",
            ),
            ("var s = Symbol(); d[s] = 3; d[s]", "3"),
            (
                "var p = Object.getOwnPropertyDescriptor(d, 'newOne'); [p.value, p.writable, p.enumerable, p.configurable].join()",
                "v,true,true,true",
            ),
            (
                "[Object.getOwnPropertyDescriptor(d, 9).value, Object.getOwnPropertyDescriptor(d, 'none')].join()",
                "nine,",
            ),
            (
                "delete d[9]; [e.hasAttribute('data-9'), 9 in d].join()",
                "false,false",
            ),
            (
                "JSON.stringify(d)",
                "{\"toString\":\"t\",\"newOne\":\"v\",\"viaDefine\":\"w\"}",
            ),
        ],
    );
}
