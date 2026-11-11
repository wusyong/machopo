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
        ("d.createElement('section').tagName", "SECTION"),
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
        ("d.createElement('section').tagName", "SECTION"),
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
    for source in [
        "d.createElement('div')",
        "d.createTextNode('text')",
        "n.addEventListener('x', () => {})",
        "n.dispatchEvent(new Event('x'))",
    ] {
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
