//! The plain members' calls (`dom_api::TypedMember::PLAIN`), through
//! V8's callback info alone: no scope made for a call that returns at
//! once, one for what allocates in V8, throws or runs script, with the
//! same results.

use crate::env::Env;

/// `source`'s value, and how many scopes the info callbacks made
/// evaluating it.
fn scopes(env: &mut Env, source: &str) -> (String, u64) {
    let before = bindings::info_scopes_for_testing().expect("a debug build");
    let value = env.eval(source);
    let made = bindings::info_scopes_for_testing().expect("a debug build");
    (value, made.wrapping_sub(before))
}

fn env() -> Env {
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); var n = d.createElement('body'); \
         d.appendChild(n); var p = d.createElement('p'); n.appendChild(p); \
         n.setAttribute('id', 'x'); n.id; n.tagName; n.getAttribute('id'); \
         n.firstChild; p.parentNode; var Event = machopoDom.Event;",
    );
    env
}

/// A plain member whose arguments lend their text or are nodes, and
/// whose result V8 has already (a Smi, a boolean, null, a cached string,
/// a made name, a wrapped node), makes no scope.
#[test]
fn a_plain_call_that_returns_at_once_makes_no_scope() {
    let mut env = env();
    let calls = [
        ("n.nodeType", "1"),
        ("n.firstChild === p", "true"),
        ("p.firstChild", "null"),
        ("p.parentNode === n", "true"),
        ("n.tagName", "BODY"),
        ("n.id", "x"),
        ("n.getAttribute('id')", "x"),
        ("n.getAttribute('none')", "null"),
        ("n.hasAttribute('id')", "true"),
        ("n.hasAttribute('none')", "false"),
        ("n.contains(p)", "true"),
        ("n.contains(null)", "false"),
        ("n.setAttribute('a', 'b')", "undefined"),
        ("n.getAttribute('a')", "b"),
        ("n.id = 'x'", "x"),
        ("n.appendChild(p) === p", "true"),
        ("n.removeAttribute('a')", "undefined"),
        ("n.hasAttribute('a')", "false"),
    ];
    // Once first: a call that changes the DOM may tell V8 how much the
    // DOM's heap grew (a scope), the first that grows it past a buffer.
    // What each gives is held: V8's scavenge drops a string or a wrapper
    // that nothing holds, and the next call would make it again.
    env.eval("var held = [];");
    for (source, _) in calls {
        env.eval(&format!("held.push({source})"));
    }
    for (source, value) in calls {
        assert_eq!(scopes(&mut env, source), (value.into(), 0), "{source}");
    }
}

/// What allocates in V8, throws or runs script makes a scope, and gives
/// what the whole front end would: an argument converted or copied, a
/// result V8 has no copy of yet, an error.
#[test]
fn a_plain_call_that_allocates_or_throws_makes_a_scope() {
    let mut env = env();
    for (source, value) in [
        // An argument that isn't a string, or doesn't lend its text.
        ("n.setAttribute('7', 'seven'); n.getAttribute(7)", "seven"),
        (
            "n.setAttribute('data-x', '\\u6587'); n.getAttribute('data-x')",
            "文",
        ),
        ("n.getAttribute('data-' + 'z'.repeat(20))", "null"),
        ("n.getAttribute({ toString() { return 'id'; } })", "x"),
        // A string V8 has no copy of; a name's, not made yet.
        (
            "n.setAttribute('data-y', 'y'); (globalThis.y = n.getAttribute('data-y'))",
            "y",
        ),
        (
            "(globalThis.section = d.createElement('section')).tagName",
            "SECTION",
        ),
        // A node with no wrapper.
        // (Held, as the string above: V8's scavenge drops a string or a
        // wrapper that nothing holds, and the next read makes it again.)
        (
            "n.innerHTML = '<b></b>'; (globalThis.b = n.firstChild).tagName",
            "B",
        ),
        // Numbers that are no Smi: not an integer, past a Smi's range.
        (
            "var i = d.createElement('input'); i.type = 'number'; i.value = '1.5'; \
             i.valueAsNumber",
            "1.5",
        ),
        (
            "var ta = d.createElement('textarea'); ta.setAttribute('cols', '2000000000'); \
             ta.cols",
            "2000000000",
        ),
        // Errors: too few arguments, the DOM's, an argument's type, what
        // a conversion threw.
        (
            "try { n.getAttribute(); 'none' } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "try { n.setAttribute('a b', 'c'); 'none' } catch (e) { e.name }",
            "InvalidCharacterError",
        ),
        (
            "try { n.appendChild(n); 'none' } catch (e) { e.name }",
            "HierarchyRequestError",
        ),
        (
            "try { n.appendChild(1); 'none' } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "try { n.appendChild(null); 'none' } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "try { n.getAttribute({ toString() { throw new RangeError('no'); } }); 'none' } \
             catch (e) { e.name }",
            "RangeError",
        ),
    ] {
        let (found, made) = scopes(&mut env, source);
        assert_eq!(found, value, "{source}");
        assert!(made >= 1, "{source}: a scope");
    }
    // Made once, V8 has them: none again.
    for (source, value) in [
        ("n.getAttribute('data-y')", "y"),
        ("n.firstChild.tagName", "B"),
        // A name's string is kept by its number, not as any string's.
        ("section.tagName", "SECTION"),
    ] {
        assert_eq!(scopes(&mut env, source), (value.into(), 0), "{source}");
    }
    // And after V8 collected what it had, made again.
    env.full_gc();
    assert_eq!(
        env.eval("n.getAttribute('data-y') + n.firstChild.tagName"),
        "yB"
    );
}

/// A failed call's error is the call's, not a result it left before it
/// failed; a setter's value is the assignment's.
#[test]
fn a_plain_calls_error_and_a_setters_value_are_the_calls() {
    let mut env = env();
    assert_eq!(
        env.eval("try { n.insertBefore(p, d) } catch (e) { e.name }"),
        "NotFoundError"
    );
    assert_eq!(
        env.eval("(n.textContent = 'text') + n.textContent"),
        "texttext"
    );
    assert_eq!(env.eval("n.firstChild.nodeType"), "3");
    // Null for a nullable string is none; for a
    // `[LegacyNullToEmptyString]` one, the empty string.
    assert_eq!(env.eval("n.textContent = null; n.childNodes.length"), "0");
    assert_eq!(
        env.eval("n.innerHTML = '<i></i>'; n.innerHTML = null; n.innerHTML"),
        ""
    );
    assert_eq!(env.eval("n.textContent = undefined; n.textContent"), "");
    assert_eq!(
        env.eval("n.innerHTML = undefined; n.innerHTML"),
        "undefined"
    );
}

/// A plain member's receiver is checked against its function's signature
/// before its callback (which reads it as one of its interface's): another
/// object is V8's `TypeError`.
#[test]
fn a_plain_members_receiver_is_checked_by_its_signature() {
    let mut env = env();
    for source in [
        "n.__lookupGetter__('nodeType').call({})",
        "n.getAttribute.call({}, 'id')",
        "n.getAttribute.call(new Event('x'), 'id')",
        "n.appendChild.call(null, p)",
    ] {
        assert_eq!(
            env.eval(&format!(
                "try {{ {source}; 'none' }} catch (e) {{ e.name }}"
            )),
            "TypeError",
            "{source}"
        );
    }
}

/// A member that isn't plain is called as before: no info callback, so
/// none of their scopes.
#[test]
fn a_member_that_isnt_plain_makes_none_of_these_scopes() {
    let mut env = env();
    for source in ["new Event('y')", "n.dispatchEvent(new Event('x'))"] {
        let (_, made) = scopes(&mut env, source);
        assert_eq!(made, 0, "{source}");
    }
}

/// Number and boolean arguments that are numbers and booleans, and
/// objects that aren't nodes (as arguments, and as results that have
/// wrappers), need no scope either.
#[test]
fn numbers_booleans_and_objects_need_no_scope() {
    let mut env = env();
    env.eval(
        "var t = d.createTextNode('abcdef'); n.appendChild(t); var list = n.childNodes; \
         var classes = n.classList; var r = d.createRange(); var r2 = d.createRange();",
    );
    // Each with whether it returns at once (a new string doesn't).
    let calls = [
        ("t.substringData(1, 3)", "bcd", false),
        ("list.item(0) === p", "true", true),
        ("list.item(7)", "null", true),
        ("n.childNodes === list", "true", true),
        ("n.classList === classes", "true", true),
        ("n.toggleAttribute('hidden', true)", "true", true),
        ("n.toggleAttribute('hidden', false)", "false", true),
        ("n.toggleAttribute('hidden')", "true", true),
        ("n.toggleAttribute('hidden')", "false", true),
        ("r.compareBoundaryPoints(0, r2)", "0", true),
        ("t.compareDocumentPosition(p)", "2", true),
        ("n.cloneNode(false) !== n", "true", false),
    ];
    env.eval("var held = [];");
    for (source, _, _) in calls {
        env.eval(&format!("held.push({source})"));
    }
    for (source, value, at_once) in calls {
        let (found, made) = scopes(&mut env, source);
        assert_eq!(found, value, "{source}");
        if at_once {
            assert_eq!(made, 0, "{source}");
        }
    }
}

/// An argument that isn't a number or a boolean is converted by the whole
/// front end (`ToNumber`, which may run script; `ToBoolean`), with what
/// WebIDL gives: a negative `unsigned long` wraps, a `double` that isn't
/// finite is a `TypeError` where it must be, an object of another type is
/// one too; an object with no wrapper has one made.
#[test]
fn numbers_booleans_and_objects_converted_or_made_make_a_scope() {
    let mut env = env();
    env.eval("var t = d.createTextNode('abcdef'); n.appendChild(t); var r = d.createRange();");
    for (source, value) in [
        ("t.substringData('1', { valueOf() { return 3; } })", "bcd"),
        ("t.substringData(1.9, 3.2)", "bcd"),
        ("t.substringData(4294967297, 2)", "bc"),
        ("n.toggleAttribute('hidden', 1)", "true"),
        ("n.toggleAttribute('hidden', '')", "false"),
        ("n.toggleAttribute('hidden', {})", "true"),
        ("n.toggleAttribute('hidden', null)", "false"),
        (
            "try { r.compareBoundaryPoints(0, n); 'none' } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "try { r.compareBoundaryPoints(0, null); 'none' } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "try { r.setStart(n, { valueOf() { throw new RangeError('no'); } }); 'none' } catch (e) { e.name }",
            "RangeError",
        ),
        ("d.createElement('div').childNodes.length", "0"),
        ("d.createElement('div').classList.length", "0"),
    ] {
        let (found, made) = scopes(&mut env, source);
        assert_eq!(found, value, "{source}");
        assert!(made >= 1, "{source}: a scope");
    }
    // Numbers as they are: a small integer, another number; one that
    // isn't finite where it must be is a `TypeError`. Strings to the
    // call's last argument.
    assert_eq!(
        env.eval(
            "var i = d.createElement('input'); i.type = 'number'; i.valueAsNumber = 7; \
             var seven = i.value; i.valueAsNumber = 1.5; seven + ' ' + i.value"
        ),
        "7 1.5"
    );
    assert_eq!(
        env.eval("try { d.createElement('meter').value = Infinity; 'none' } catch (e) { e.name }"),
        "TypeError"
    );
    assert_eq!(env.eval("n.classList.add('a', 'b'); n.className"), "a b");
    // A small negative integer wraps with no conversion; a number that is
    // no small integer is read as it is.
    assert_eq!(
        env.eval("try { t.substringData(-1, 1) } catch (e) { e.name }"),
        "IndexSizeError"
    );
    assert_eq!(scopes(&mut env, "n.scrollTop = 1e3; 0").1, 0);
}

/// A member that always makes a node has its wrapper made ahead of the
/// operation and made the node's in it: `createElement` (of the interface
/// its name has), the other `create*`, `splitText`, and `cloneNode` (of
/// the interface `this` is of, whatever the node).
#[test]
fn a_new_nodes_wrapper_is_made_ahead() {
    let mut env = Env::window("https://example.test/");
    env.eval(
        "var d = document; var n = d.createElement('body'); var t = d.createTextNode('abcdef'); \
         var f = d.createDocumentFragment(); var p = n.appendChild(d.createElement('p')); \
         var held = []; 0",
    );
    for (source, value) in [
        ("d.createElement('div')", "[object HTMLDivElement]"),
        ("d.createElement('x-y')", "[object HTMLElement]"),
        ("d.createElement('nonsuch')", "[object HTMLUnknownElement]"),
        ("d.createTextNode('t')", "[object Text]"),
        ("d.createComment('c')", "[object Comment]"),
        ("d.createDocumentFragment()", "[object DocumentFragment]"),
        ("t.splitText(3)", "[object Text]"),
        ("n.cloneNode(false)", "[object HTMLBodyElement]"),
        ("n.cloneNode(true)", "[object HTMLBodyElement]"),
        ("p.cloneNode()", "[object HTMLParagraphElement]"),
        ("t.cloneNode(true)", "[object Text]"),
        ("f.cloneNode(true)", "[object DocumentFragment]"),
        // (A window's document is an `HTMLDocument`; its copy is a
        // `Document`, as it was before its wrapper was made ahead.)
        ("d.cloneNode(false)", "[object Document]"),
    ] {
        let (ahead, wasted) = (env.wrappers_ahead(), env.wrappers_wasted());
        let (found, made) = scopes(
            &mut env,
            &format!("held.push({source}); Object.prototype.toString.call(held.at(-1))"),
        );
        assert_eq!(found, value, "{source}");
        assert_eq!(env.wrappers_ahead(), ahead + 1, "{source}: made ahead");
        assert_eq!(env.wrappers_wasted(), wasted, "{source}: none wasted");
        // The wrapper's scope (and one more when the call is the one
        // that tells V8 the DOM's heap grew).
        assert!((1..=2).contains(&made), "{source}: {made} scopes");
    }
    // The copy is the node's own wrapper from then on, and a copy's
    // children have none until they are read.
    assert_eq!(
        env.eval(
            "var copy = n.cloneNode(true); [copy !== n, copy.firstChild.tagName, \
             copy.firstChild === copy.firstChild, copy.parentNode].join()"
        ),
        "true,P,true,"
    );
}

/// A wrapper made ahead that isn't of the node's interface after all is
/// wasted, and the node's made after the operation: an uppercase name
/// (lowercased in the operation), an XML document's element; a custom
/// element is constructed after it.
#[test]
fn a_wrapper_made_ahead_of_another_interface_is_made_again() {
    let mut env = Env::window("https://example.test/");
    env.eval(
        "var d = document; var x = d.implementation.createDocument(null, 'r'); \
         customElements.define('c-e', class extends HTMLElement { constructor() { super(); this.made = 1; } }); 0",
    );
    for (source, value, wastes) in [
        ("d.createElement('DIV')", "[object HTMLDivElement]", 1),
        ("x.createElement('div')", "[object Element]", 1),
        (
            "d.createElementNS('http://www.w3.org/1999/xhtml', 'div')",
            "[object HTMLDivElement]",
            1,
        ),
        ("d.createElement('c-e')", "[object HTMLElement]", 0),
    ] {
        let (ahead, wasted) = (env.wrappers_ahead(), env.wrappers_wasted());
        assert_eq!(
            env.eval(&format!("Object.prototype.toString.call({source})")),
            value,
            "{source}"
        );
        assert_eq!(
            env.wrappers_ahead(),
            ahead,
            "{source}: not the one made ahead"
        );
        assert_eq!(env.wrappers_wasted(), wasted + wastes, "{source}");
    }
    assert_eq!(env.eval("d.createElement('c-e').made"), "1");
}

/// Nodes and strings, as many as the call gives (`append`'s), are taken
/// as they are: a node its wrapper, a string lent; what is neither is
/// converted (`ToString`, which may run script, in the arguments' order)
/// by the whole front end. So is `add`'s element or number.
#[test]
fn nodes_or_strings_are_taken_as_they_are() {
    let mut env = env();
    env.eval(
        "var a = d.createElement('a'), b = d.createElement('b'), c = d.createElement('c'); \
         var box = d.createElement('div'); \
         var text = () => Array.from(box.childNodes, (n) => n.nodeName + (n.data ?? '')).join(',');",
    );
    let calls = [
        ("box.append(a, b)", "A,B"),
        ("box.append()", "A,B"),
        ("box.prepend(c)", "C,A,B"),
        ("a.after(c)", "A,C,B"),
        ("b.before(a)", "C,A,B"),
        ("c.replaceWith(b, c)", "B,C,A"),
        ("box.replaceChildren(a)", "A"),
        ("box.replaceChildren()", ""),
    ];
    // Without a scope, each; then again for what it does (reading the
    // children between two leaves the next call the DOM's heap's growth
    // to report, with a scope).
    for (source, _) in calls {
        let (found, made) = scopes(&mut env, source);
        assert_eq!(found, "undefined", "{source}");
        assert_eq!(made, 0, "{source}");
    }
    assert_eq!(env.eval("text()"), "");
    for (source, children) in calls {
        env.eval(source);
        assert_eq!(env.eval("text()"), children, "{source}");
    }
    // A string makes a text node (no wrapper: nothing of V8's); what is
    // neither is converted, in order.
    assert_eq!(
        env.eval("box.append('x', a, 'y'); text()"),
        "#textx,A,#texty"
    );
    assert_eq!(
        env.eval(
            "var order = []; \
             box.replaceChildren({ toString() { order.push(1); return 'one'; } }, b, 7, null, \
                 { toString() { order.push(2); return 'two'; } }); \
             text() + ' ' + order"
        ),
        "#textone,B,#text7,#textnull,#texttwo 1,2"
    );
    assert_eq!(
        env.eval(
            "try { box.append(a, { toString() { throw new RangeError('no'); } }); 'none' } \
             catch (e) { e.name + ' ' + text() }"
        ),
        "RangeError #textone,B,#text7,#textnull,#texttwo"
    );
    // `add`'s `before`: an element, a number (a small integer as it is,
    // another converted), none.
    env.eval(
        "var s = d.createElement('select'); \
         var o = (v) => { var o = d.createElement('option'); o.value = v; return o; }; \
         var first = o('1'); var values = () => Array.from(s.options, (o) => o.value).join(',');",
    );
    for (source, values) in [
        ("s.add(first)", "1"),
        ("s.add(o('2'), null)", "1,2"),
        ("s.add(o('0'), first)", "0,1,2"),
        ("s.add(o('x'), 1)", "0,x,1,2"),
        ("s.add(o('y'), '1')", "0,y,x,1,2"),
        ("s.add(o('z'), 1.5)", "0,z,y,x,1,2"),
    ] {
        assert_eq!(env.eval(source), "undefined", "{source}");
        assert_eq!(env.eval("values()"), values, "{source}");
    }
    env.eval("var last = o('v'); s.id;");
    for source in [
        "s.add(last, 2)",
        "s.add(last, first)",
        "s.add(last)",
        "s.add(last, null)",
    ] {
        assert_eq!(
            scopes(&mut env, source),
            ("undefined".to_owned(), 0),
            "{source}"
        );
    }
    let (_, made) = scopes(&mut env, "s.add(last, 1.5)");
    assert!(
        made >= 1,
        "a number that isn't a small integer is converted"
    );
}

/// An indexed getter's interceptor (`list[i]`) takes the path with the
/// callback's info alone: its index read as the number it is, an item
/// that has a wrapper given with no scope. No item (past the end: null,
/// or an empty string) isn't intercepted, so the property is undefined
/// (V8 looks on the prototype).
#[test]
fn an_indexed_getter_is_given_the_info_alone() {
    let mut env = env();
    env.eval(
        "var t = d.createTextNode('x'); n.appendChild(t); n.className = 'a b'; \
         var list = n.childNodes, kids = n.children, classes = n.classList, attrs = n.attributes; \
         var held = [list[0], list[1], kids[0], attrs[0], attrs[1], classes[0], classes[1]];",
    );
    for (source, value) in [
        ("list[0] === p", "true"),
        ("list[1] === t", "true"),
        ("list[2]", "undefined"),
        ("list[4294967294]", "undefined"),
        ("kids[0] === p", "true"),
        ("kids[1]", "undefined"),
        ("attrs[0] === held[3]", "true"),
        ("attrs[5]", "undefined"),
        ("classes[0]", "a"),
        ("classes[1]", "b"),
        ("classes[2]", "undefined"),
    ] {
        let (found, made) = scopes(&mut env, source);
        assert_eq!(found, value, "{source}");
        assert_eq!(made, 0, "{source}");
    }
    // What the interceptor's other callbacks tell is as before: the
    // indices there are, in order; none past the end; read-only.
    for (source, value) in [
        ("list[0] === list.item(0)", "true"),
        // An empty string is no item (a style's `item` past its end).
        ("n.style[0]", "undefined"),
        ("n.style.item(0)", ""),
        ("n.style.color = 'red'; n.style[0]", "color"),
        ("0 in list", "true"),
        ("2 in list", "false"),
        ("Object.keys(list).join()", "0,1"),
        ("[...list].length", "2"),
        (
            "Array.prototype.map.call(kids, (k) => k.localName).join()",
            "p",
        ),
        ("list[0] = 7; list[0] === p", "true"),
        ("Object.getOwnPropertyDescriptor(list, 0).writable", "false"),
        ("list['0'] === p", "true"),
        ("list[-1]", "undefined"),
        ("list[0.5]", "undefined"),
        // An item with no wrapper yet has one made (a scope), the same
        // one after.
        ("d.createElement('ul').childNodes[0]", "undefined"),
        (
            "var ul = d.createElement('ul'); ul.innerHTML = '<li></li>'; \
             var li = ul.childNodes[0]; li === ul.childNodes[0] && li.localName",
            "li",
        ),
    ] {
        assert_eq!(env.eval(source), value, "{source}");
    }
    env.eval("var fresh = d.createElement('ol'); fresh.innerHTML = '<li></li>'; var freshList = fresh.childNodes;");
    let (found, made) = scopes(&mut env, "freshList[0].localName");
    assert_eq!(found, "li");
    assert!(made >= 1, "a wrapper made for the item");
}

/// A constructor's new object is the wrapper of what it constructs: one
/// wrapper made, counted as any other, and none made again as the object
/// is given back.
#[test]
fn a_constructed_objects_wrapper_is_counted() {
    let mut env = env();
    let before = env.wrappers_made();
    assert_eq!(env.eval("var e = new Event('x'); e.type"), "x");
    assert_eq!(env.wrappers_made() - before, 1);
    assert_eq!(env.eval("n.dispatchEvent(e); e.target === n"), "true");
    assert_eq!(env.wrappers_made() - before, 1);
}

/// What a plain member's call may reach (`dom_api::PLAIN_FRONT_METHODS`,
/// `PLAIN_OP_METHODS`: what its types and its function are checked
/// against) is what the front end given the callback's info alone has:
/// each is a method of its own there, not the trait's default (which, of
/// a method a front end must have to be given such a member, panics).
#[test]
fn the_plain_methods_are_the_info_front_ends_own() {
    let source = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/src/info_front.rs"))
        .expect("the front end's source");
    let (front, op) = source
        .split_once("OpFront for InfoOp")
        .expect("the operation's side after the call's");
    let has = |text: &str, name: &str| {
        text.contains(&format!("fn {name}(")) || text.contains(&format!("fn {name}<"))
    };
    // The defaults that do nothing, say there is none, or only call the
    // others (`finish_handler`).
    let missing: Vec<&str> = dom_api::PLAIN_FRONT_METHODS
        .iter()
        .copied()
        .filter(|name| !["finish", "finish_handler"].contains(name) && !has(front, name))
        .chain(
            dom_api::PLAIN_OP_METHODS
                .iter()
                .copied()
                .filter(|name| *name != "current_window" && !has(op, name)),
        )
        .collect();
    assert!(missing.is_empty(), "not the front end's own: {missing:?}");
}
