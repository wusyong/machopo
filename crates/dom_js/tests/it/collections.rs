//! `getElementsByTagName`'s live `HTMLCollection` from script (its indexed
//! getter, `@@iterator`), HTML elements' interfaces by name, and the real
//! `testharness.js` running in a window of the DOM.

use crate::env::Env;

const TREE: &str = "var html = document.createElement('html'); document.appendChild(html); \
     var head = document.createElement('head'); html.appendChild(head); \
     var m1 = document.createElement('meta'); m1.name = 'timeout'; m1.content = 'long'; head.appendChild(m1); \
     var body = document.createElement('body'); html.appendChild(body); \
     var m2 = document.createElement('meta'); m2.id = 'two'; body.appendChild(m2);";

#[test]
fn a_collection_from_script() {
    let mut env = Env::window("about:blank");
    env.eval(TREE);
    let cases = [
        (
            "var metas = document.getElementsByTagName('meta'); metas instanceof HTMLCollection",
            "true",
        ),
        (
            "Object.prototype.toString.call(metas)",
            "[object HTMLCollection]",
        ),
        (
            "[metas.length, metas[0] === m1, metas[1] === m2, metas[2], metas.item(1) === m2].join()",
            "2,true,true,,true",
        ),
        (
            "[0 in metas, 2 in metas, metas.namedItem('two') === m2, metas.namedItem('timeout') === m1].join()",
            "true,false,true,true",
        ),
        (
            "[...metas].length + ',' + Array.from(metas).every(m => m.localName === 'meta')",
            "2,true",
        ),
        ("var n = 0; for (var m of metas) n++; n", "2"),
        ("metas[0].content", "long"),
        // The same object while held, as Chrome's (DOM lets it).
        ("document.getElementsByTagName('meta') === metas", "true"),
        ("body.getElementsByTagName('meta')[0] === m2", "true"),
        ("document.getElementsByTagName('*').length", "5"),
        // Live.
        (
            "body.appendChild(document.createElement('meta')); metas.length",
            "3",
        ),
        (
            "head.removeChild(m1); [metas.length, metas[0] === m2].join()",
            "2,true",
        ),
        // An index past the last is no property: the prototype's.
        ("HTMLCollection.prototype[5] = 'p'; metas[5]", "p"),
        (
            "typeof HTMLCollection.prototype[Symbol.iterator]",
            "function",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// An HTML element's wrapper is of its name's interface (`meta` its own,
/// others `HTMLElement`), another namespace's `Element`; a member of an
/// element interface checks its receiver.
/// An indexed getter's properties as script sees them: listed by
/// `Object.keys` (each index below `length`, WebIDL's supported property
/// indices), `in` only those, enumerable and read-only (writable with an
/// indexed setter: `select[0] = option`).
#[test]
fn indexed_properties_are_listed_and_described() {
    let mut env = Env::window("about:blank");
    env.eval(TREE);
    let cases = [
        (
            "var metas = document.getElementsByTagName('meta'); \
             [Object.keys(metas).join('|'), 0 in metas, 2 in metas, Object.getOwnPropertyNames(metas).join('|')].join()",
            "0|1,true,false,0|1",
        ),
        (
            "var d = Object.getOwnPropertyDescriptor(metas, 1); \
             [d.value === m2, d.writable, d.enumerable, d.configurable, \
              Object.getOwnPropertyDescriptor(metas, 2)].join()",
            "true,false,true,true,",
        ),
        (
            "m2.className = 'a b'; [Object.keys(m2.classList).join('|'), Object.keys(m2.attributes).join('|'), \
             Object.keys(body.childNodes).join('|'), Object.keys(document.createElement('p').children).join('|')].join()",
            "0|1,0|1,0,",
        ),
        (
            "m2.style.color = 'red'; m2.style.width = '1px'; Object.keys(m2.style).slice(0, 2).join('|')",
            "0|1",
        ),
        (
            "var select = body.appendChild(document.createElement('select')); \
             select.append(document.createElement('option'), document.createElement('option')); \
             var o = Object.getOwnPropertyDescriptor(select, 0); \
             [Object.keys(select).join('|'), o.writable, o.enumerable].join()",
            "0|1,true,true",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

#[test]
fn elements_are_of_their_names_interfaces() {
    let mut env = Env::window("about:blank");
    let cases = [
        (
            "var meta = document.createElement('meta'); meta instanceof HTMLMetaElement && meta instanceof HTMLElement",
            "true",
        ),
        (
            "Object.getPrototypeOf(meta) === HTMLMetaElement.prototype",
            "true",
        ),
        (
            "var div = document.createElement('div'); [div instanceof HTMLElement, div instanceof HTMLMetaElement].join()",
            "true,false",
        ),
        (
            "var svg = document.createElementNS('http://www.w3.org/2000/svg', 'meta'); [svg instanceof Element, svg instanceof HTMLElement].join()",
            "true,false",
        ),
        (
            "meta.name = 'n'; [meta.name, meta.getAttribute('name')].join()",
            "n,n",
        ),
        (
            "try { Object.getOwnPropertyDescriptor(HTMLMetaElement.prototype, 'name').get.call(div); } catch (e) { e.name }",
            "TypeError",
        ),
        (
            "document.createElement('META') instanceof HTMLMetaElement",
            "true",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// The real `testharness.js` (WPT's, as a test page loads it) runs in a
/// window of the DOM: a passing test, a failing one and an async one
/// with a timer, their results through a completion callback. The harness
/// waits for the window's `load`, which the test dispatches (no page load
/// yet).
#[test]
fn testharness_runs() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../third_party/wpt/resources/testharness.js"
    );
    let harness = std::fs::read_to_string(path).expect("WPT's testharness.js");
    let mut env = Env::window("http://web-platform.test/smoke.html");
    let loaded = env.eval_at(
        &harness,
        "http://web-platform.test/resources/testharness.js",
    );
    assert!(!loaded.starts_with("Exception"), "{loaded}");
    let started = env.eval(
        "var results; \
         setup({explicit_done: true, output: false}); \
         add_completion_callback((tests, status) => { \
           results = tests.map(t => t.name + ':' + t.status).join() + '|' + status.status; }); \
         test(() => assert_equals(document.createElement('p').localName, 'p'), 'passes'); \
         test(() => assert_true(false), 'fails'); \
         async_test(t => { setTimeout(t.step_func_done(() => assert_equals(1, 1)), 0); }, 'async'); \
         window.dispatchEvent(new Event('load')); done(); 'started'",
    );
    assert_eq!(started, "started");
    // By the window's clock: the harness's own timeout isn't due.
    for _ in 0..100 {
        env.checkpoint();
        if env.eval("typeof results") == "string" {
            break;
        }
        env.run_timers(None);
    }
    // testharness's statuses: PASS 0, FAIL 1; the harness's OK 0.
    assert_eq!(env.eval("results"), "passes:0,fails:1,async:0|0");
}
