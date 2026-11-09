//! The DOM from JS: nodes' wrappers of the table's interfaces, one a
//! node, its members as WebIDL shapes them, arguments converted and
//! checked, wrappers V8's scavenge drops made again, and the tree alive
//! while script holds any node of it.

use dom_api::{INTERFACES, MemberKind};

use crate::env::Env;

/// A document and an element and a text of it, in `d`, `e` and `t`.
const SETUP: &str = "var d = machopoDom.createDocument(); var e = d.createElement('div'); \
                     var t = d.createTextNode('hi');";

#[test]
fn a_nodes_wrapper_is_of_its_interfaces() {
    let mut env = Env::new();
    env.eval(SETUP);
    let checks = [
        "d instanceof Document",
        "d instanceof Node",
        "e instanceof Element",
        "e instanceof Node",
        "!(e instanceof Document)",
        "t instanceof Text",
        "t instanceof CharacterData",
        "Object.getPrototypeOf(Element.prototype) === Node.prototype",
        "Object.getPrototypeOf(Text.prototype) === CharacterData.prototype",
        "Object.getPrototypeOf(Document) === Node",
        "e.nodeType === Node.ELEMENT_NODE",
        "e.ELEMENT_NODE === 1",
        "Node.DOCUMENT_NODE === 9",
        // An HTML element's interface is by its name: `div`'s its own.
        "Object.prototype.toString.call(e) === '[object HTMLDivElement]'",
        "Object.getPrototypeOf(HTMLDivElement.prototype) === HTMLElement.prototype",
        "Object.getPrototypeOf(HTMLElement.prototype) === Element.prototype",
    ];
    for check in checks {
        assert_eq!(env.eval(check), "true", "{check}");
    }
    assert_eq!(
        env.eval("[d.nodeName, e.tagName, t.data, t.length].join()"),
        "#document,DIV,hi,2"
    );
}

/// The same node, the same wrapper, its expandos with it.
#[test]
fn a_node_has_one_wrapper() {
    let mut env = Env::new();
    env.eval(SETUP);
    let made = env.wrappers_made();
    assert_eq!(
        env.eval(
            "d.appendChild(e); e.appendChild(t); e.x = 1; \
             [d.documentElement === e, e.parentNode === d, d.firstChild === d.firstChild, \
              t.parentElement === e, d.documentElement.x].join()"
        ),
        "true,true,true,true,1"
    );
    assert_eq!(env.wrappers_made(), made, "each had its wrapper");
}

/// Each member of the table, from JS: an attribute an accessor on the
/// prototype (on a window itself for `Window`'s, WebIDL's `[Global]`; a
/// setter only if it isn't read-only), a method a function of
/// its required parameters' length (an overload's, the fewest of its
/// forms') (a static one on the interface object; a namespace's on its
/// object, which has no prototype), a constant on the interface object and
/// the prototype, a constructor the interface object itself (its length
/// its required parameters'); interface objects aren't the global's
/// enumerable properties, their `prototype` is read-only, and those
/// without a constructor throw on `new`.
#[test]
fn the_tables_members_are_webidl_shaped() {
    let mut env = Env::window("https://example.test/");
    for interface in INTERFACES {
        let name = interface.name;
        // An observable array attribute has a setter of its own (WebIDL's),
        // and so does a `[Replaceable]` one.
        let has_setter = |member: &str| {
            interface.members.iter().any(|m| {
                m.name == member
                    && (m.kind == MemberKind::Setter
                        || (m.kind == MemberKind::Getter
                            && (m.replaceable
                                || matches!(m.returns, dom_api::TypeDesc::ObservableArray(_)))))
            })
        };
        // An overload's `length`: the fewest arguments a form needs.
        let length = |member: &dom_api::Member| {
            interface
                .members
                .iter()
                .filter(|other| {
                    other.kind == member.kind
                        && other.name == member.name
                        && other.is_static == member.is_static
                })
                .map(|other| other.required())
                .min()
                .unwrap_or(0)
        };
        for member in interface.members {
            // A `[Global]` interface's members are the global's own.
            let holder = if interface.global {
                "globalThis".to_owned()
            } else {
                format!("{name}.prototype")
            };
            let property = format!(
                "Object.getOwnPropertyDescriptor({holder}, '{}')",
                member.name
            );
            if member.named.is_some()
                || member.observable.is_some()
                || member.pairs
                || member.name.is_empty()
            {
                // Anonymous: the interceptors' (`queries.rs`; an indexed
                // getter with no name, `forms.rs`), an observable array's
                // operations (`style_sheets.rs`), a pair iterator's pairs
                // (`urls.rs`), on no prototype.
                let check = format!("{property} === undefined");
                assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
                continue;
            }
            if member.kind == MemberKind::LegacyFactory {
                // A function of the global (not enumerable), its prototype
                // the interface's, read-only; not on the interface object.
                let check = format!(
                    "typeof {0} === 'function' && {0}.length === {1} && {0}.prototype === {name}.prototype && \
                     !Object.getOwnPropertyDescriptor({0}, 'prototype').writable && \
                     !Object.keys(globalThis).includes('{0}') && {name}.{0} === undefined",
                    member.name,
                    member.required()
                );
                assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
                continue;
            }
            if interface.namespace {
                // A namespace's: on its object, which has no prototype,
                // enumerable (WebIDL's operations); an overload's `length`
                // its fewest.
                let check = format!(
                    "{name}.prototype === undefined && typeof {name}.{0} === 'function' && \
                     {name}.{0}.length === {1} && Object.getOwnPropertyDescriptor({name}, '{0}').enumerable",
                    member.name,
                    length(member)
                );
                assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
                continue;
            }
            if member.is_static && member.kind == MemberKind::Getter {
                // A static attribute: an accessor of the interface object,
                // read-only, not the prototype's.
                let check = format!(
                    "Object.getOwnPropertyDescriptor({name}.prototype, '{0}') === undefined && \
                     typeof Object.getOwnPropertyDescriptor({name}, '{0}').get === 'function' && \
                     Object.getOwnPropertyDescriptor({name}, '{0}').set === undefined",
                    member.name
                );
                assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
                continue;
            }
            if member.is_static {
                // On the interface object; the prototype has the name only
                // for a regular operation of it (`Response.json()` and
                // `response.json()`).
                let regular = interface
                    .members
                    .iter()
                    .any(|other| other.name == member.name && !other.is_static);
                let check = format!(
                    "({regular} || Object.getOwnPropertyDescriptor({name}.prototype, '{0}') === undefined) && \
                     typeof {name}.{0} === 'function' && {name}.{0}.length === {1} && \
                     (!{regular} || {name}.{0} !== {name}.prototype.{0})",
                    member.name,
                    member.required()
                );
                assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
                continue;
            }
            if member.unforgeable {
                // On each object (`window.rs` checks a window's), not the
                // prototype.
                let check = format!(
                    "Object.getOwnPropertyDescriptor({name}.prototype, '{}') === undefined",
                    member.name
                );
                assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
                continue;
            }
            let check = match member.kind {
                MemberKind::Getter => format!(
                    "typeof {property}.get === 'function' && typeof {property}.set === '{}' && {property}.enumerable",
                    if has_setter(member.name) {
                        "function"
                    } else {
                        "undefined"
                    }
                ),
                MemberKind::Setter => continue,
                MemberKind::Method => format!(
                    "typeof {property}.value === 'function' && {property}.value.length === {} && {property}.writable",
                    length(member)
                ),
                MemberKind::Constant(value) => format!(
                    "{name}.{0} === {value} && {name}.prototype.{0} === {value} && \
                     !Object.getOwnPropertyDescriptor({name}, '{0}').writable",
                    member.name
                ),
                MemberKind::Constructor => format!(
                    "typeof {name} === 'function' && {name}.length === {}",
                    length(member)
                ),
                MemberKind::IndexedSetter | MemberKind::LegacyFactory => {
                    unreachable!("anonymous, and a legacy factory's above")
                }
            };
            assert_eq!(env.eval(&check), "true", "{name}.{}: {check}", member.name);
        }
        assert_eq!(
            env.eval(&format!("Object.keys(globalThis).includes('{name}')")),
            "false",
            "{name} isn't enumerable"
        );
        // A namespace object: no `prototype`, not callable.
        if interface.namespace {
            let check = format!(
                "{name}.prototype === undefined && typeof {name} === 'object' && \
                 Object.getPrototypeOf({name}) === Object.prototype"
            );
            assert_eq!(env.eval(&check), "true", "{name}");
            continue;
        }
        // WebIDL's interface object's `prototype`: not writable, enumerable
        // or configurable.
        let check = format!(
            "(d => !d.writable && !d.enumerable && !d.configurable)(\
             Object.getOwnPropertyDescriptor({name}, 'prototype'))"
        );
        assert_eq!(env.eval(&check), "true", "{name}.prototype");
        if !interface
            .members
            .iter()
            .any(|member| member.kind == MemberKind::Constructor)
        {
            assert_eq!(
                env.eval(&format!("new {name}()")),
                "Exception: TypeError: Illegal constructor"
            );
        }
    }
    assert_eq!(
        env.eval("Object.keys(globalThis).includes('machopoDom')"),
        "false"
    );
}

/// V8 checks the receiver against the member's interface.
#[test]
fn the_receiver_is_checked() {
    let mut env = Env::new();
    env.eval(SETUP);
    for call in [
        "Object.getOwnPropertyDescriptor(Node.prototype, 'nodeType').get.call({})",
        "Element.prototype.getAttribute.call(d, 'x')",
        "Node.prototype.appendChild.call(Node.prototype, e)",
    ] {
        assert!(
            env.eval(call)
                .starts_with("Exception: TypeError: Illegal invocation"),
            "{call}"
        );
    }
}

/// A plain object is no node, whatever the field after its header holds:
/// one whose first property reads as the handle of each of the wrappers'
/// table's first 2^20 entries (a Smi `i << 5` is `i << 6` tagged, the
/// handle of entry `i`), among which the DOM's wrappers are (8192 weren't
/// enough), is a `TypeError` as a node argument.
#[test]
fn an_object_whose_field_looks_like_a_handle_is_no_node() {
    let mut env = Env::new();
    env.eval(SETUP);
    let refused = env.eval(
        "var refused = 0; \
         for (var i = 0; i < 1048576; i++) { \
             try { e.appendChild({ a: i << 5 }); } \
             catch (error) { if (error instanceof TypeError) refused++; } \
         } \
         refused",
    );
    assert_eq!(refused, "1048576");
}

#[test]
fn arguments_are_converted_and_checked() {
    let mut env = Env::new();
    env.eval(SETUP);
    let cases = [
        // WebIDL's `ToString`.
        ("e.setAttribute('a', 5); e.getAttribute('a')", "5"),
        (
            "e.setAttribute('b', { toString() { return 'x'; } }); e.getAttribute('b')",
            "x",
        ),
        ("e.getAttribute('none')", "null"),
        // A `DOMString?`: null is none.
        (
            "e.setAttributeNS(null, 'c', 'v'); e.getAttributeNS(null, 'c')",
            "v",
        ),
        // `[LegacyNullToEmptyString]`, but undefined spells itself.
        ("t.data = null; JSON.stringify(t.data)", "\"\""),
        ("t.data = undefined; t.data", "undefined"),
        ("e.contains(null)", "false"),
        ("e.contains(e)", "true"),
        ("e.textContent = 'text'; e.textContent", "text"),
        ("JSON.stringify(d.textContent)", "null"),
        ("t.data = '文字 é'; [t.data, t.length].join()", "文字 é,4"),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
    let thrown = [
        (
            "e.setAttribute('a')",
            "TypeError: Failed to execute 'setAttribute': 2 arguments required, but only 1 present.",
        ),
        (
            "d.appendChild('x')",
            "TypeError: parameter 1 is not of type 'Node'.",
        ),
        (
            "d.appendChild(null)",
            "TypeError: parameter 1 is not of type 'Node'",
        ),
        (
            "e.setAttribute('d', { toString() { throw new Error('thrown'); } })",
            "Error: thrown",
        ),
    ];
    for (source, expected) in thrown {
        assert_eq!(
            env.eval(source),
            format!("Exception: {expected}"),
            "{source}"
        );
    }
    assert_eq!(
        env.eval("e.hasAttribute('d')"),
        "false",
        "the call stopped where it threw"
    );
}

/// A wrapper script never changed and nothing holds, V8's scavenge drops,
/// and the node's reference to it goes: the node gets a new one (made
/// after the operation that found it) when script next asks. One with an
/// expando stays.
#[test]
fn a_dropped_wrapper_is_made_again() {
    if crate::env::v8_gcs_at_its_own_times() {
        eprintln!(
            "skipped: V8 GCs at its own times, and its scavenge keeps what a marking reached"
        );
        return;
    }
    let mut env = Env::new();
    env.eval(
        "var p = machopoDom.createDocument().createElement('p'); \
         p.appendChild(p.ownerDocument.createElement('span')); \
         p.appendChild(p.ownerDocument.createElement('b')).x = 1;",
    );
    for _ in 0..3 {
        env.minor_gc();
    }
    let made = env.wrappers_made();
    assert_eq!(env.eval("p.firstChild.tagName"), "SPAN");
    assert_eq!(
        env.wrappers_made(),
        made + 1,
        "the span's dropped, made again"
    );
    assert_eq!(env.eval("p.firstChild === p.firstChild"), "true");
    assert_eq!(env.eval("p.lastChild.x"), "1");
    assert_eq!(
        env.wrappers_made(),
        made + 1,
        "the b's kept: it has an expando"
    );
}

/// Many dropped at once: V8 resets most of the nodes' references to them on
/// its threads in parallel (`TryResetRoot`, a block of handles each), the
/// rest on the isolate's; every node gets a new wrapper when script next
/// asks, none a stale one.
#[test]
fn many_dropped_wrappers_are_made_again() {
    if crate::env::v8_gcs_at_its_own_times() {
        eprintln!(
            "skipped: V8 GCs at its own times, and its scavenge keeps what a marking reached"
        );
        return;
    }
    const NODES: u64 = 50_000;
    let mut env = Env::new();
    env.eval(&format!(
        "var p = machopoDom.createDocument().createElement('p'); \
         for (let i = 0; i < {NODES}; i++) p.appendChild(p.ownerDocument.createElement('i'));"
    ));
    for _ in 0..3 {
        env.minor_gc();
    }
    let made = env.wrappers_made();
    assert_eq!(
        env.eval("let n = 0; for (let c = p.firstChild; c; c = c.nextSibling) if (c.tagName === 'I') n++; n"),
        NODES.to_string()
    );
    assert_eq!(
        env.wrappers_made(),
        made + NODES,
        "each child's dropped, made again"
    );
}

/// The tree lives while script holds any node of it (the node its wrapper
/// holds, and the rest from it), and goes once it holds none.
#[test]
fn a_tree_lives_while_script_holds_a_node() {
    let mut env = Env::new();
    let empty = env.used_bytes();
    env.eval(
        "var kept = (() => { const d = machopoDom.createDocument(); const r = d.createElement('r'); \
         d.appendChild(r); for (let i = 0; i < 2000; i++) { const c = d.createElement('c'); \
         c.setAttribute('i', String(i)); r.appendChild(c); } return r.lastChild; })();",
    );
    let full = env.used_bytes();
    for _ in 0..2 {
        env.full_gc();
    }
    assert_eq!(
        env.eval(
            "[kept.getAttribute('i'), kept.parentNode.firstChild.getAttribute('i'), \
             kept.ownerDocument.documentElement === kept.parentNode, kept.parentNode.childElementCount].join()"
        ),
        "1999,0,true,2000"
    );
    env.eval("kept = undefined;");
    for _ in 0..2 {
        env.full_gc();
    }
    assert!(
        env.used_bytes() < full - (full - empty) / 2,
        "collected: {} bytes, {full} with the tree",
        env.used_bytes()
    );
}

/// After a major GC, V8 counts the DOM's live bytes, not what it used with
/// the dead its sweep has yet to free: its next limit follows what lives,
/// so garbage made on the DOM's heap alone keeps starting GCs at a steady
/// pace. Counting the dead, each limit was four times the last (61 MB,
/// 242 MB, past 659 MB): two GCs in 160 MB of garbage.
#[test]
fn v8s_limits_follow_the_doms_live_bytes() {
    if !crate::env::v8_marks_incrementally() {
        eprintln!("skipped: V8 doesn't mark incrementally, so the DOM's growth starts none");
        return;
    }
    let mut env = Env::new();
    env.eval("var d = machopoDom.createDocument();");
    let gcs = dom_js::heap_stats(env.data.isolate()).unwrap().gcs;
    // Some 160 MB of garbage, each round some 0.5 MB.
    for _ in 0..320 {
        env.eval(
            "for (let i = 0; i < 2000; i++) { const p = d.createElement('p'); \
             p.setAttribute('i', String(i)); p.appendChild(d.createTextNode('garbage')); }",
        );
    }
    let collected = dom_js::heap_stats(env.data.isolate()).unwrap().gcs - gcs;
    assert!(
        collected >= 4,
        "{collected} major GCs in some 160 MB of garbage"
    );
}

/// The DOM's heap under V8 has no nursery: its objects with wrappers are
/// old from their allocation (V8's reference to them never moves), most
/// of the rest hangs on them, and the few short-lived ones left (an
/// event's data and path) are collected with the old generation, as
/// Blink's are.
#[test]
fn the_doms_heap_has_no_nursery() {
    if crate::env::dom_heap_has_a_nursery() {
        eprintln!("skipped: MACHOPO_GC_OPTIONS gives the DOM's heap a nursery");
        return;
    }
    let mut env = Env::new();
    env.eval(
        "var d = machopoDom.createDocument(); \
         for (let i = 0; i < 200; i++) { const p = d.createElement('p'); \
         p.setAttribute('i', String(i)); p.appendChild(d.createTextNode('t')); \
         p.addEventListener('e', () => {}); p.dispatchEvent(new Event('e')); }",
    );
    let stats = dom_js::heap_stats(env.data.isolate()).unwrap();
    assert_eq!(stats.nursery_bytes, 0);
    assert_eq!(stats.young_bytes, 0);
    assert!(stats.used_bytes > 0);
}

/// The DOM's growth is V8's to know (as cppgc's allocation): garbage made
/// on the DOM's heap alone (V8's old generation hardly grows: the
/// wrappers die young) starts V8's major GCs once it passes V8's limit
/// (some 60 MB at first), which collect it.
#[test]
fn the_doms_growth_starts_v8s_gcs() {
    let mut env = Env::new();
    env.eval("var d = machopoDom.createDocument();");
    let gcs = dom_js::heap_stats(env.data.isolate()).unwrap().gcs;
    let mut peak = 0;
    let mut collected = false;
    // At most some 150 MB of garbage, each round 0.5 MB.
    for _ in 0..300 {
        env.eval(
            "for (let i = 0; i < 2000; i++) { const p = d.createElement('p'); \
             p.setAttribute('i', String(i)); p.appendChild(d.createTextNode('garbage')); }",
        );
        let used = env.used_bytes();
        peak = peak.max(used);
        let stats = dom_js::heap_stats(env.data.isolate()).unwrap();
        if stats.gcs > gcs && used < peak / 2 {
            collected = true;
            break;
        }
    }
    assert!(
        collected,
        "V8 collected the DOM's garbage: {peak} bytes at most, none collected"
    );
}

/// `createElement` makes the wrapper of the interface its name has ahead
/// (none wasted for a lowercase name in an HTML document, whatever its
/// interface), and the right one otherwise: an uppercase name (lowercased
/// in the operation), an XML document's element, a custom element's name.
#[test]
fn create_element_foresees_its_wrapper() {
    let mut env = Env::window("https://example.test/");
    env.eval("var d = document; d.appendChild(d.createElement('html')); 0");
    let wasted = env.wrappers_wasted();
    assert_eq!(
        env.eval(
            "[d.createElement('div'), d.createElement('td'), d.createElement('section'), d.createElement('x-y'), \
              d.createElement('nonsuch')].map((e) => Object.prototype.toString.call(e)).join()"
        ),
        "[object HTMLDivElement],[object HTMLTableCellElement],[object HTMLElement],[object HTMLElement],[object HTMLUnknownElement]"
    );
    assert_eq!(env.wrappers_wasted(), wasted, "each foreseen");
    assert_eq!(
        env.eval(
            "var x = d.implementation.createDocument(null, null); \
             [d.createElement('DIV'), x.createElement('div')].map((e) => Object.prototype.toString.call(e)).join()"
        ),
        "[object HTMLDivElement],[object Element]"
    );
    assert_eq!(
        env.wrappers_wasted(),
        wasted.saturating_add(2),
        "an uppercase name and an XML document's: made again"
    );
    assert_eq!(
        env.eval("customElements.define('x-z', class extends HTMLElement {}); d.createElement('x-z').constructor.name"),
        ""
    );
}

/// An attribute's accessors are named as WebIDL names them: `get x`, `set
/// x` (a static one's too).
#[test]
fn accessors_are_named() {
    let mut page = crate::history::Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var d = n => Object.getOwnPropertyDescriptor(n, 'href') || Object.getOwnPropertyDescriptor(n, 'id');
             [d(URL.prototype).get.name, d(URL.prototype).set.name, d(Element.prototype).get.name,
              Object.getOwnPropertyDescriptor(PerformanceObserver, 'supportedEntryTypes').get.name,
              Object.getOwnPropertyDescriptor(Document.prototype, 'URL').get.name].join()"
        ),
        "get href,set href,get id,get supportedEntryTypes,get URL"
    );
}

/// Members and interfaces as Chrome has them: the legacy
/// `WebKitMutationObserver`, interfaces of which no object is made (machopo has
/// no server timing or back-forward cache), a document's own event handlers
/// and an SVG element's `autofocus`.
#[test]
fn legacy_and_objectless_members() {
    let mut env = Env::window("https://example.test/");
    let cases = [
        (
            "[WebKitMutationObserver === MutationObserver, Object.keys(window).includes('WebKitMutationObserver')].join()",
            "true,false",
        ),
        (
            "var error = f => { try { f(); return 'none'; } catch (e) { return e.name; } }; \
             [typeof PerformanceServerTiming, typeof NotRestoredReasons, \
              error(() => new PerformanceServerTiming()), error(() => new NotRestoredReasons())].join()",
            "function,function,TypeError,TypeError",
        ),
        (
            "var got = []; \
             [document.onvisibilitychange, document.onprerenderingchange].forEach((h) => got.push(String(h))); \
             document.onvisibilitychange = (e) => got.push(e.type); \
             document.onprerenderingchange = (e) => got.push(e.type); \
             document.dispatchEvent(new Event('visibilitychange')); \
             document.dispatchEvent(new Event('prerenderingchange')); got.join()",
            "null,null,visibilitychange,prerenderingchange",
        ),
        (
            "var svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg'); \
             var before = svg.autofocus; svg.autofocus = true; \
             [before, svg.autofocus, svg.getAttribute('autofocus')].join()",
            "false,true,",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// An object's wrapper is made in its relevant realm, whichever realm's
/// function made it: a node's, its document's window's (one of no window,
/// the window whose script made the document); a style sheet's, its
/// rules' and their lists', its document's (a constructed one's
/// constructor document).
#[test]
fn wrappers_are_made_in_their_relevant_realm() {
    let mut page = crate::history::Page::new("https://example.test/");
    page.load();
    page.eval(
        "var w = document.body.appendChild(document.createElement('iframe')).contentWindow; 0",
    );
    assert_eq!(
        page.eval(
            "[w.Document.prototype.createTextNode.call(document, 'x') instanceof Text,
              w.Document.prototype.createComment.call(document, 'x') instanceof Comment,
              w.Document.prototype.createDocumentFragment.call(document) instanceof DocumentFragment,
              w.document.createTextNode('y') instanceof w.Text,
              document.createElement('p') instanceof HTMLParagraphElement].join()"
        ),
        "true,true,true,true,true"
    );
    // A document of no window: its maker's realm, its nodes too.
    assert_eq!(
        page.eval(
            "var made = w.DOMImplementation.prototype.createHTMLDocument.call(document.implementation, 't');
             var theirs = w.document.implementation.createHTMLDocument('u');
             [made instanceof Document, made.body instanceof HTMLBodyElement,
              theirs instanceof w.Document, theirs.createElement('p') instanceof w.HTMLParagraphElement,
              new w.Document().createElement('div') instanceof w.Element,
              w.Document.parseHTMLUnsafe('<p>').body.firstChild instanceof w.HTMLParagraphElement].join()"
        ),
        "true,true,true,true,true,true"
    );
    assert_eq!(
        page.eval(
            "var t = w.document.createElement('template'); t.innerHTML = '<p>';
             [w.DOMImplementation.prototype.createDocument.call(document.implementation, null, 'r') instanceof XMLDocument,
              DOMParser.prototype.parseFromString.call(new w.DOMParser(), '<p>', 'text/html') instanceof w.Document,
              Object.getOwnPropertyDescriptor(HTMLTemplateElement.prototype, 'content').get.call(t) instanceof w.DocumentFragment,
              Node.prototype.cloneNode.call(w.document.implementation.createHTMLDocument('')) instanceof w.Document].join()"
        ),
        "true,true,true,true"
    );
    assert_eq!(
        page.eval(
            "var style = w.document.createElement('style'); w.document.documentElement.append(style);
             var sheet = style.sheet; style.remove(); sheet.insertRule('.a {}');
             var built = new w.CSSStyleSheet(); built.insertRule('.b {}');
             [sheet.cssRules[0] instanceof w.CSSStyleRule, sheet.cssRules instanceof w.CSSRuleList,
              built.cssRules[0].style instanceof w.CSSStyleDeclaration].join()"
        ),
        "true,true,true"
    );
    // Web Animations': their target's document's (through this realm's
    // getters too).
    assert_eq!(
        page.eval(
            "var target = w.document.body.appendChild(w.document.createElement('div'));
             var animation = Element.prototype.animate.call(target, null);
             var get = name => Object.getOwnPropertyDescriptor(Animation.prototype, name).get.call(animation);
             [animation instanceof w.Animation, get('effect') instanceof w.KeyframeEffect,
              get('timeline') instanceof w.DocumentTimeline].join()"
        ),
        "true,true,true"
    );
}

/// A member's call unwraps its receiver as V8 checked it against the
/// function's signature (`bindings::Receiver`: its map isn't read again),
/// an overloaded method's forms too, an indexed getter's its holder; the
/// receiver is still the member's own object, whichever of an
/// `EventTarget`'s types it is, and one of another interface's never gets
/// to the call. A legacy caller (`document.all(…)`, no signature) unwraps
/// its receiver whole. Counted in debug builds.
#[test]
fn members_unwrap_their_receivers_as_v8_checked_them() {
    let Some(before) = bindings::receiver_unwraps_for_testing() else {
        return;
    };
    let mut env = Env::window("https://example.test/");
    let unwraps = || bindings::receiver_unwraps_for_testing().unwrap();
    env.eval(
        "var d = document.createElement('div'); d.appendChild(document.createElement('b')); 0",
    );
    let start = unwraps();
    assert!(start > before, "making the elements unwrapped `document`");
    assert_eq!(
        env.eval("var t = 0; for (let i = 0; i < 10; i++) t += d.nodeType; t"),
        "10"
    );
    assert!(unwraps() >= start + 10, "each getter's call");
    // The getters around it count too: the index alone.
    env.eval("var nodes = d.childNodes; 0");
    let start = unwraps();
    env.eval("var child = nodes[0]; 0");
    assert!(unwraps() > start, "the indexed getter's holder");
    assert_eq!(env.eval("child.tagName"), "B");
    // An overloaded method's forms.
    let start = unwraps();
    env.eval("d.scrollTo(0, 0); d.scrollTo({ top: 0 }); 0");
    assert!(unwraps() >= start + 2, "each form's call");
    // A window's cross-origin member (its own function, with the
    // interface's signature).
    let start = unwraps();
    assert_eq!(env.eval("window.closed"), "false");
    assert!(unwraps() > start, "the cross-origin getter's call");
    // Each of an event target's types, the window's too.
    assert_eq!(
        env.eval(
            "var log = []; for (const t of [d, window, new EventTarget()]) { \
             t.addEventListener('x', () => log.push(t === window ? 'window' : t.constructor.name)); \
             t.dispatchEvent(new Event('x')); } log.join()"
        ),
        "HTMLDivElement,window,EventTarget"
    );
    // Another interface's object: V8's signature check throws before the
    // call.
    let start = unwraps();
    assert_eq!(
        env.eval(
            "try { Object.getOwnPropertyDescriptor(Node.prototype, 'nodeType').get.call(window); 'called' } \
             catch (e) { e.constructor.name }"
        ),
        "TypeError"
    );
    assert_eq!(unwraps(), start, "no call");
    // No signature: checked whole.
    env.eval("document.appendChild(document.createElement('html')); var all = document.all; 0");
    let start = unwraps();
    env.eval("var first = all(0); 0");
    assert_eq!(unwraps(), start, "the legacy caller's receiver");
    assert_eq!(env.eval("first.localName"), "html");
}
