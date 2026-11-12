//! The DOM's strings to and from V8: copied, a string's V8 string cached
//! by its identity and dropped once V8 collects it, a name's made once,
//! and arguments by `ToString`.

use dom::DomString;
use dom_js::{StringCache, new_string, string_arg, throw_type_error};

use crate::env::Env;

/// The references to a string.
fn references(string: &DomString) -> usize {
    string.reference_count()
}

fn full_gc(isolate: &mut v8::Isolate) {
    isolate.request_garbage_collection_for_testing_with_stack_state(
        v8::GarbageCollectionType::Full,
        v8::cppgc::EmbedderStackState::NoHeapPointers,
    );
}

/// V8 copies a string, ASCII or not: it keeps none of the DOM's (whose
/// count isn't atomic; V8 frees an external string's buffer on its GC's
/// workers).
#[test]
fn a_string_is_copied_into_v8() {
    let mut data = bindings::V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let ascii = DomString::new("class-name");
    let string = new_string(scope, &ascii).unwrap();
    assert!(!string.is_external_onebyte());
    assert_eq!(string.to_rust_string_lossy(scope), "class-name");
    assert_eq!(references(&ascii), 1, "the DOM's alone");
    let other = new_string(scope, &DomString::new("文字 é")).unwrap();
    assert!(!other.is_external_onebyte());
    assert_eq!(other.to_rust_string_lossy(scope), "文字 é");
    assert_eq!(other.length(), 4, "UTF-16 code units");
}

/// The same string (an interned value on two elements), one V8 string;
/// an equal one that isn't the same, another.
#[test]
fn a_strings_v8_string_is_cached_by_its_identity() {
    let mut data = bindings::V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let mut cache = StringCache::new();
    let value = DomString::new("item");
    let same = value.clone();
    let first = cache.get(scope, &value).unwrap();
    assert_eq!(references(&value), 3, "the two and the cache's");
    let again = cache.get(scope, &same).unwrap();
    assert!(first.strict_equals(again.into()));
    assert_eq!(cache.len(), 1);
    assert_eq!(references(&value), 3, "one entry");
    let equal = DomString::new("item");
    cache.get(scope, &equal).unwrap();
    assert_eq!(cache.len(), 2, "an equal string isn't the same");
    let non_ascii = DomString::new("é");
    let made = cache.get(scope, &non_ascii).unwrap();
    assert_eq!(made.to_rust_string_lossy(scope), "é");
    assert_eq!(cache.len(), 3);
}

/// Once V8 collects a cached string, the entry goes as the cache is next
/// used, and the string's references with it.
#[test]
fn an_entry_goes_once_v8_collects_its_string() {
    let mut data = bindings::V8PerIsolateData::new();
    let mut cache = StringCache::new();
    let value = DomString::new("transient");
    {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        cache.get(scope, &value).unwrap();
    }
    assert_eq!(references(&value), 2, "the string's and the entry's");
    for _ in 0..3 {
        full_gc(data.isolate());
    }
    // The entry's goes at the next use.
    assert_eq!(references(&value), 2);
    assert_eq!(cache.len(), 1);
    let other = DomString::new("other");
    {
        let isolate = data.isolate();
        v8::scope!(let scope, isolate);
        cache.get(scope, &other).unwrap();
        assert_eq!(cache.len(), 1, "the dead entry went");
        assert_eq!(references(&value), 1);
        // Made again on demand.
        let again = cache.get(scope, &value).unwrap();
        assert_eq!(again.to_rust_string_lossy(scope), "transient");
        assert_eq!(cache.len(), 2);
    }
}

/// `code`'s value, run in `scope`'s context.
fn run<'s>(scope: &mut v8::PinScope<'s, '_>, code: &str) -> v8::Local<'s, v8::Value> {
    let code = v8::String::new(scope, code).unwrap();
    v8::Script::compile(scope, code, None)
        .unwrap()
        .run(scope)
        .unwrap()
}

#[test]
fn arguments_are_strings_by_to_string_and_type_errors_are_thrown() {
    let mut data = bindings::V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    for (code, want) in [
        ("1.5", "1.5"),
        ("null", "null"),
        ("undefined", "undefined"),
        ("'plain'", "plain"),
        ("'é文'", "é文"),
        ("({ toString() { return 'mine'; } })", "mine"),
    ] {
        let value = run(scope, code);
        assert_eq!(string_arg(scope, value).as_deref(), Some(want), "{code}");
    }
    // `ToString` may throw: none, the exception pending.
    v8::tc_scope!(let tc, scope);
    let value = run(tc, "({ toString() { throw new Error('no'); } })");
    assert_eq!(string_arg(tc, value), None);
    assert!(tc.has_caught());
    tc.reset();
    throw_type_error(tc, "not a node");
    let exception = tc.exception().unwrap();
    assert_eq!(exception.to_rust_string_lossy(tc), "TypeError: not a node");
}

/// `peek` finds a cached string and makes none: what a binding asks in an
/// operation of the DOM.
#[test]
fn peek_makes_nothing() {
    let mut data = bindings::V8PerIsolateData::new();
    let isolate = data.isolate();
    v8::scope!(let scope, isolate);
    let mut cache = StringCache::new();
    let string = DomString::new("peeked");
    assert!(cache.peek(scope, &string).is_none());
    assert_eq!(references(&string), 1, "no V8 string made");
    assert!(cache.is_empty());
    let made = cache.get(scope, &string).unwrap();
    let peeked = cache.peek(scope, &string).unwrap();
    assert!(made == peeked);
    assert_eq!(references(&string), 2, "the string's and the entry's");
}

/// A name's V8 string (a local name, an uppercased tag name) is made the
/// first time one is returned, then returned again from the DOM's. The
/// empty string of an attribute `id` and `className` don't find is V8's
/// own: none made.
#[test]
fn a_names_v8_string_is_made_once() {
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); var e = d.createElement('p'); \
         var f = d.createElement('p');",
    );
    assert_eq!(env.names_made(), 0);
    assert_eq!(
        env.eval("[e.localName, e.localName, f.localName, e.tagName, f.tagName, e.id, f.className].join()"),
        "p,p,p,P,P,,"
    );
    assert_eq!(env.names_made(), 2, "p and P");
    // Not names: an attribute's value isn't one of them.
    assert_eq!(env.eval("e.id = 'p'; [e.id, e.localName].join()"), "p,p");
    assert_eq!(env.names_made(), 2);
}

/// A member's string argument of each kind V8 has is read whole: an ASCII
/// one in place (a literal, one made at run time), the others copied (a
/// cons string, two-byte, Latin-1, a lone surrogate, as U+FFFD); every
/// loan is over once the call returns, so V8's GCs run after it.
#[test]
fn string_arguments_of_each_kind_are_read() {
    let mut env = Env::new();
    env.eval("var d = machopoDom.createDocument(); var e = d.createElement('p');");
    let cases = [
        ("'literal'", "literal"),
        ("String.fromCharCode(105, 100)", "id"),
        ("'cons-' + 'x'.repeat(20)", "cons-xxxxxxxxxxxxxxxxxxxx"),
        ("'文字'", "文字"),
        ("'caf\\u00e9'", "café"),
        ("'a\\ud800b'", "a\u{fffd}b"),
    ];
    for (argument, text) in cases {
        let source = format!("e.setAttribute('data-x', {argument}); e.getAttribute('data-x')");
        assert_eq!(env.eval(&source), text, "{argument}");
        assert_eq!(bindings::borrowed_texts(), 0, "{argument}");
        // Its name too, and a method making a node from one.
        let source = format!("e.setAttribute({argument}, 'v'); e.hasAttribute({argument})");
        let lowercased = text.to_lowercase();
        if names_ok(&lowercased) {
            assert_eq!(env.eval(&source), "true", "{argument} as a name");
        }
        let source = format!("d.createTextNode({argument}).data");
        assert_eq!(env.eval(&source), text, "{argument} as data");
        assert_eq!(bindings::borrowed_texts(), 0, "{argument}");
    }
    env.full_gc();
    env.minor_gc();
}

/// A member's string argument that is a string already isn't converted
/// (no `ToString`, a call into V8); anything else is, once, with what
/// `ToString` gives (a number's digits, an object's `toString`, `null`
/// and `undefined` by name, a `String` object's value), and what it throws.
#[test]
fn a_string_argument_is_converted_only_if_it_isnt_a_string() {
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); var e = d.createElement('p'); \
         e.setAttribute('id', 'x'); e.setAttribute('7', 'seven'); \
         e.setAttribute('null', 'n'); e.setAttribute('undefined', 'u'); \
         var cons = 'i' + 'd'; var calls = 0;",
    );
    let converted = |env: &mut Env, source: &str| {
        let before = env.string_conversions();
        let result = env.eval(source);
        (result, env.string_conversions().wrapping_sub(before))
    };
    // Strings of each representation: none converted.
    for argument in [
        "'id'",
        "cons",
        "String.fromCharCode(105, 100)",
        "'文'",
        "''",
    ] {
        let (_, conversions) = converted(&mut env, &format!("e.getAttribute({argument})"));
        assert_eq!(conversions, 0, "{argument}");
    }
    assert_eq!(converted(&mut env, "e.getAttribute('id')"), ("x".into(), 0));
    assert_eq!(
        converted(&mut env, "e.setAttribute('a', 'b'); e.getAttribute('a')"),
        ("b".into(), 0)
    );
    // Anything else: once each.
    assert_eq!(
        converted(&mut env, "e.getAttribute(7)"),
        ("seven".into(), 1)
    );
    assert_eq!(converted(&mut env, "e.getAttribute(null)"), ("n".into(), 1));
    assert_eq!(
        converted(&mut env, "e.getAttribute(undefined)"),
        ("u".into(), 1)
    );
    assert_eq!(
        converted(&mut env, "e.getAttribute(new String('id'))"),
        ("x".into(), 1)
    );
    assert_eq!(
        converted(
            &mut env,
            "e.getAttribute({ toString() { calls++; return 'id'; } }) + calls"
        ),
        ("x1".into(), 1)
    );
    assert_eq!(
        converted(&mut env, "e.setAttribute(7, 8); e.getAttribute('7')"),
        ("8".into(), 2)
    );
    // What the conversion throws is the call's.
    assert_eq!(
        converted(
            &mut env,
            "try { e.getAttribute({ toString() { throw new RangeError('no'); } }); 'none' } \
             catch (error) { error.name + ': ' + error.message }"
        ),
        ("RangeError: no".into(), 1)
    );
    let (thrown, conversions) = converted(
        &mut env,
        "try { e.getAttribute(Symbol('s')); 'none' } catch (error) { error.name }",
    );
    assert_eq!((thrown.as_str(), conversions), ("TypeError", 1));
    // And the member isn't called.
    assert_eq!(
        env.eval(
            "try { e.setAttribute('thrown', { toString() { throw 1; } }); } catch (error) {} \
             e.hasAttribute('thrown')"
        ),
        "false"
    );
}

/// Whether `name` is a valid attribute name (`setAttribute` throws for
/// another).
fn names_ok(name: &str) -> bool {
    !name.is_empty()
        && !name
            .chars()
            .any(|c| c.is_whitespace() || "\0/=>".contains(c))
}

/// An attribute value's V8 string, made the first time it is read, is
/// returned from the cache after (from its weak handle, no local made):
/// the same value each read, on every element holding it (interned).
#[test]
fn a_cached_values_v8_string_is_returned_again() {
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); var e = d.createElement('p'); \
         var f = d.createElement('p'); e.setAttribute('title', 'cached-value'); \
         f.setAttribute('title', 'cached-value');",
    );
    assert_eq!(
        env.eval(
            "[e.getAttribute('title'), e.getAttribute('title'), f.getAttribute('title')].join()"
        ),
        "cached-value,cached-value,cached-value"
    );
    assert_eq!(
        env.eval("e.id = 'x'; [e.id, e.id, e.getAttribute('id')].join()"),
        "x,x,x"
    );
}

/// A string only the call holds (a style value serialized for this read,
/// a number's text) is given to V8 and not cached: its address is gone
/// with the call, so no later read would find its entry. One the DOM
/// holds (an attribute's value) is cached the first time it is read.
#[test]
fn a_string_only_the_call_holds_isnt_cached() {
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); var e = d.createElement('p'); \
         e.style.color = 'red'; e.setAttribute('title', 'held-by-the-dom');",
    );
    let before = env.cached_strings();
    assert_eq!(
        env.eval("var s = ''; for (var i = 0; i < 50; i++) s = e.style.color; s"),
        "red"
    );
    assert_eq!(
        env.eval("for (var i = 0; i < 50; i++) s = e.style['color']; s"),
        "red",
        "by its name, through the named getter"
    );
    assert_eq!(
        env.eval("for (var i = 0; i < 50; i++) s = e.style.cssText; s"),
        "color: red;"
    );
    assert_eq!(env.cached_strings(), before, "none of them an entry");
    assert_eq!(
        env.eval("for (var i = 0; i < 50; i++) s = e.getAttribute('title'); s"),
        "held-by-the-dom"
    );
    assert_eq!(
        env.cached_strings(),
        before + 1,
        "the attribute's value, once"
    );
}

/// A dictionary's member is read by its name's V8 string, made the first
/// time any dictionary's member of the name is read and found by the
/// name's number after: none made for a read again, of that dictionary or
/// of another with a member of the name. The same string is the name's as
/// a result (`localName`).
#[test]
fn a_dictionary_members_key_is_made_once() {
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); var e = d.createElement('p'); \
         var init = { bubbles: true, cancelable: false }; var Event = machopoDom.Event; \
         var CustomEvent = machopoDom.CustomEvent;",
    );
    let before = env.names_made();
    assert_eq!(
        env.eval("var ev = new Event('x', init); [ev.bubbles, ev.cancelable, ev.composed].join()"),
        "true,false,false"
    );
    assert_eq!(
        env.names_made(),
        before + 3,
        "bubbles, cancelable, composed"
    );
    assert_eq!(
        env.eval(
            "for (var i = 0; i < 20; i++) ev = new Event('y', { cancelable: true, composed: true }); \
             [ev.bubbles, ev.cancelable, ev.composed].join()"
        ),
        "false,true,true"
    );
    assert_eq!(env.names_made(), before + 3, "found by their numbers");
    // Another dictionary's members of the same names, and one more.
    assert_eq!(
        env.eval("ev = new CustomEvent('z', { bubbles: true, detail: 7 }); [ev.bubbles, ev.detail].join()"),
        "true,7"
    );
    assert_eq!(env.names_made(), before + 4, "detail");
    // A getter is called once a read, in the names' order.
    assert_eq!(
        env.eval(
            "var log = []; new Event('g', { get composed() { log.push('composed'); return true; }, \
             get bubbles() { log.push('bubbles'); return true; } }); log.join()"
        ),
        "bubbles,composed"
    );
    // A name that is a member's and an element's: one string, whichever
    // made it.
    assert_eq!(
        env.eval(
            "var options = { once: true }; e.addEventListener('q', () => {}, options); \
             var o = d.createElement('once'); [o.localName, o.tagName, o.localName in options].join()"
        ),
        "once,ONCE,true"
    );
}
