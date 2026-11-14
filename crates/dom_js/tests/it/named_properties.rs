//! WebIDL's named properties through their interceptors: what a name's
//! query, descriptor, delete and define do, the supported names as the
//! object's keys, an array index as a name, and an interface's indexed
//! setter. A query asks its getter only whether there is a value: nothing
//! of V8's is made for it.

use crate::env::Env;

fn page() -> Env {
    let mut env = Env::window("about:blank");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); \
         document.body.innerHTML = \"<div id=d data-a='1' data-b=''></div><form><input name=x><input name=y></form>\" + \
             '<select><option>1<option>2</select>'; \
         var d = document.getElementById('d'), data = d.dataset, form = document.forms[0], select = document.querySelector('select'); \
         function error(f) { try { f(); return 'no'; } catch (e) { return e.name; } } \
         function describe(o, name) { var p = Object.getOwnPropertyDescriptor(o, name); \
           return p ? [p.value, p.writable, p.enumerable, p.configurable].join() : 'none'; } 0",
    );
    env
}

fn check(env: &mut Env, cases: &[(&str, &str)]) {
    for (script, expected) in cases {
        assert_eq!(env.eval(script), *expected, "{script}");
    }
}

/// A name the getter gives is a property: configurable, writable with a
/// setter, enumerable with the names listed; an empty string is a value;
/// a name it gives none for is no property.
#[test]
fn a_query_tells_what_the_getter_gives() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "['a' in data, 'b' in data, 'c' in data].join()",
                "true,true,false",
            ),
            (
                "[describe(data, 'a'), describe(data, 'b'), describe(data, 'c')].join(' ')",
                "1,true,true,true ,true,true,true none",
            ),
            ("Object.prototype.hasOwnProperty.call(data, 'a')", "true"),
            // A getter alone, whose names aren't listed: read-only, not
            // enumerable; what it gives an object.
            ("['x' in form, 'z' in form].join()", "true,false"),
            (
                "var p = Object.getOwnPropertyDescriptor(form, 'x'); \
                 [p.value === form.elements[0], p.writable, p.enumerable, p.configurable].join()",
                "true,false,false,true",
            ),
            // A name of the prototype's isn't the interceptor's.
            (
                "['toString' in data, describe(data, 'toString')].join()",
                "true,none",
            ),
        ],
    );
}

/// A query makes nothing of V8's: no scope, no string, no wrapper for
/// what the getter gives.
#[test]
fn a_query_makes_nothing_of_v8s() {
    let mut env = page();
    env.eval("var fresh = document.createElement('form'); fresh.innerHTML = '<input name=n>'; 0");
    for source in [
        "'a' in data",
        "'c' in data",
        "Reflect.has(data, 'b')",
        // An element script hasn't seen: no wrapper is made to tell.
        "'n' in fresh",
    ] {
        let before = crate::env::slow_paths();
        env.eval(source);
        assert_eq!(crate::env::slow_paths(), before, "{source}");
    }
    assert_eq!(env.eval("'n' in fresh"), "true");
}

/// The supported names are the object's keys, as they are then.
#[test]
fn the_supported_names_are_the_keys() {
    let mut env = page();
    check(
        &mut env,
        &[
            ("Object.keys(data).join()", "a,b"),
            ("data.cD = '3'; Object.keys(data).join()", "a,b,cD"),
            (
                "d.removeAttribute('data-a'); Object.keys(data).join()",
                "b,cD",
            ),
            (
                "var seen = []; for (var k in data) seen.push(k); seen.join()",
                "b,cD",
            ),
        ],
    );
}

/// `delete` of a named property goes through the deleter; of another
/// name, it is an ordinary property's.
#[test]
fn delete_goes_through_the_deleter() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "[delete data.a, 'a' in data, d.hasAttribute('data-a')].join()",
                "true,false,false",
            ),
            ("[delete data.never, d.attributes.length].join()", "true,2"),
            (
                "data.own = 1; delete data.own; d.hasAttribute('data-own')",
                "false",
            ),
            // No deleter: the property stays. (The delete itself says
            // true: with no deleter, the interceptor isn't asked.)
            ("[delete form.x, 'x' in form].join()", "true,true"),
        ],
    );
}

/// A property defined with a value goes through the setter; with a getter
/// or a setter it is refused; on an interface with no setter it is an
/// ordinary property's.
#[test]
fn define_goes_through_the_setter() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "Object.defineProperty(data, 'e', { value: 5 }); [data.e, d.getAttribute('data-e')].join()",
                "5,5",
            ),
            (
                "Object.defineProperty(data, 'f', {}); d.getAttribute('data-f')",
                "undefined",
            ),
            (
                "Reflect.defineProperty(data, 'g', { get() { return 1; } })",
                "false",
            ),
            (
                "error(() => Object.defineProperty(data, 'g', { set(v) {} }))",
                "TypeError",
            ),
            ("d.hasAttribute('data-g')", "false"),
            // What the setter throws is the define's.
            (
                "error(() => Object.defineProperty(data, 'a-b', { value: 1 }))",
                "SyntaxError",
            ),
        ],
    );
}

/// An array index is a name like any other where the interface has no
/// indexed properties of its own: its digits.
#[test]
fn an_index_is_a_name() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "data[9] = 'nine'; [d.getAttribute('data-9'), data[9], data['9']].join()",
                "nine,nine,nine",
            ),
            (
                "[9 in data, 8 in data, describe(data, 9)].join(' ')",
                "true false nine,true,true,true",
            ),
            // (In the names' order: the index isn't put first.)
            ("Object.keys(data).join()", "a,b,9"),
            (
                "Object.defineProperty(data, 7, { value: 'seven' }); d.getAttribute('data-7')",
                "seven",
            ),
            ("Reflect.defineProperty(data, 6, { get() {} })", "false"),
            (
                "[delete data[9], 9 in data, d.hasAttribute('data-9'), delete data[1234]].join()",
                "true,false,false,true",
            ),
            (
                "data[4294967294] = 'big'; d.getAttribute('data-4294967294')",
                "big",
            ),
        ],
    );
}

/// A setter that doesn't take the name leaves it an ordinary property's;
/// what a setter throws is the assignment's.
#[test]
fn a_setter_may_decline_or_throw() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "d.style.notAProperty = 1; [d.style.notAProperty, d.getAttribute('style')].join()",
                "1,",
            ),
            (
                "d.style.color = 'red'; d.getAttribute('style')",
                "color: red;",
            ),
            ("error(() => { data['a-b'] = 1; })", "SyntaxError"),
        ],
    );
}

/// An interface's indexed setter takes every index, with the index and
/// the value assigned.
#[test]
fn an_indexed_setter_takes_the_index_and_the_value() {
    let mut env = page();
    check(
        &mut env,
        &[
            (
                "select[1] = new Option('two'); [select.length, select[1].text].join()",
                "2,two",
            ),
            (
                "select[3] = new Option('four'); [select.length, select[3].text, select[2].text].join()",
                "4,four,",
            ),
            (
                "select[0] = null; [select.length, select[0].text].join()",
                "3,two",
            ),
            ("error(() => { select[0] = {}; })", "TypeError"),
            (
                "select.options[0] = new Option('first'); select[0].text",
                "first",
            ),
        ],
    );
}
