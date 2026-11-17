//! Script stopped from another thread (`V8Engine::terminator`, a worker's
//! `terminate()`): it stops wherever it runs, in a callback the DOM calls
//! (a listener, an event handler, a timer, a microtask, an observer, a node
//! filter, a custom element's constructor, a string conversion) as much as
//! at the top; no callback reports it as an exception or lets the script
//! that called it run on; the engine, its realm and its DOM go after it.

use std::sync::mpsc;
use std::time::{Duration, Instant};

use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

fn run(engine: &mut V8Engine, source: &str) {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source,
            url: "https://example.test/page.html",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
}

/// Runs `source` (then the tasks it queued, `then_tasks`) on an engine of
/// a thread of its own, stops it from this one `after` it started, and
/// waits for that thread to end: it must, without a panic; no error is
/// reported (none reaches `onerror`), and nothing after what called script
/// back ran (`after` set, in the script or a `catch` of it).
fn stops(name: &str, source: &str, then_tasks: bool, after: Duration) {
    let (terminators, terminator) = mpsc::channel();
    let (ran, done) = mpsc::channel();
    let source = source.to_owned();
    let thread = std::thread::Builder::new()
        .name("Terminated".into())
        .spawn(move || {
            let mut engine = V8Engine::new(EngineSettings { web_tests: true });
            engine
                .create_window(engine.top_frame(), "https://example.test/", Instant::now())
                .expect("a window");
            terminators.send(engine.terminator()).unwrap();
            // One script: a stop asked for before any runs stops it.
            run(
                &mut engine,
                &format!(
                    "globalThis.reported = 0; onerror = (message) => {{ reported++; globalThis.why = String(message); }};\n\
                     document.append(document.createElement('html')); document.documentElement.append(document.createElement('body'));\n{source}"
                ),
            );
            if then_tasks {
                engine.run_due_tasks();
            }
            // Stopped: what runs next runs, as a worker's loop would not
            // let it but a window's would (stopped before its first line,
            // the script above set nothing).
            run(
                &mut engine,
                "testRunner.reportHarnessResults((globalThis.reported ? reported + ': ' + why : '0') + (globalThis.after ? ', and ran on' : ''))",
            );
            ran.send(engine.take_harness_results()).unwrap();
        })
        .unwrap();
    let terminator = terminator.recv().unwrap();
    std::thread::sleep(after);
    terminator.terminate();
    let reported = done
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|_| panic!("{name}: the script didn't stop"));
    thread
        .join()
        .unwrap_or_else(|_| panic!("{name}: its thread panicked"));
    assert_eq!(reported.as_deref(), Some("0"), "{name}: errors reported");
}

/// Each way the DOM calls script back: the callback runs forever, and the
/// script that led to it runs forever after it, which it would if the
/// callback's caller let the script on (as it does after an exception);
/// it sets `after` first, which it does if any of it runs at all (V8
/// stops a stop asked for again only at its next check, a loop's jump
/// back: not before `after` is set).
const CALLED_BACK: &[(&str, &str, bool)] = &[
    (
        "a listener",
        "const t = new EventTarget(); t.addEventListener('x', () => { for (;;); });
         t.dispatchEvent(new Event('x')); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a listener object's handleEvent",
        "const t = new EventTarget(); t.addEventListener('x', { handleEvent() { for (;;); } });
         t.dispatchEvent(new Event('x')); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "an event handler",
        "const c = new AbortController(); c.signal.onabort = () => { for (;;); };
         c.abort(); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a node's listener, in a dispatch through the tree",
        "const d = document.createElement('div'); document.body.append(d);
         document.body.addEventListener('click', () => { for (;;); });
         d.click(); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a node filter",
        "const w = document.createTreeWalker(document, NodeFilter.SHOW_ALL, () => { for (;;); });
         try { w.nextNode(); } catch { globalThis.after = 1; } globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a node filter object's acceptNode",
        "const w = document.createTreeWalker(document, NodeFilter.SHOW_ALL, { acceptNode() { for (;;); } });
         try { w.nextNode(); } catch { globalThis.after = 1; } globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a custom element's constructor",
        "customElements.define('x-forever', class extends HTMLElement { constructor() { super(); for (;;); } });
         document.createElement('x-forever'); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a string conversion",
        "document.createElement({ toString() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a getter structured clone gets",
        "structuredClone({ get a() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a getter a message's clone gets",
        "new MessageChannel().port1.postMessage({ get a() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a dictionary's getter",
        "new Event('x', { get bubbles() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "an options dictionary's getter",
        "document.body.addEventListener('x', () => {}, { get once() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a request's init's getter",
        "new Request('https://example.test/x', { get method() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a sequence's iterator",
        "new Blob({ *[Symbol.iterator]() { for (;;); } }); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a custom element's observed attributes",
        "customElements.define('x-observes', class extends HTMLElement {
           static get observedAttributes() { for (;;); } attributeChangedCallback() {} });
         globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a listener object's handleEvent getter",
        "const t = new EventTarget(); t.addEventListener('x', { get handleEvent() { for (;;); } });
         t.dispatchEvent(new Event('x')); globalThis.after = 1; for (;;);",
        false,
    ),
    (
        "a timer's function",
        "setTimeout(() => { for (;;); }, 0);",
        true,
    ),
    (
        "a timer's string",
        "setTimeout('for (;;);', 0);",
        true,
    ),
    (
        "a listener a timer's function dispatches to",
        "const t = new EventTarget(); t.addEventListener('x', () => { for (;;); });
         setTimeout(() => { t.dispatchEvent(new Event('x')); for (;;); }, 0);",
        true,
    ),
    (
        "a microtask",
        "queueMicrotask(() => { for (;;); });",
        true,
    ),
    (
        "a mutation observer's callback",
        "new MutationObserver(() => { for (;;); }).observe(document.body, { childList: true });
         document.body.append('x');",
        true,
    ),
];

/// The DOM about to call script back again in the same call from script
/// (the next listener, the next element to upgrade): stopped in the first
/// callback, it calls none after it, as script calling one would clear the
/// stop (V8 keeps it only while script is on the stack below).
const CALLED_BACK_AGAIN: &[(&str, &str)] = &[
    (
        "the next listener",
        "const t = new EventTarget(); t.addEventListener('x', () => { for (;;); });
         t.addEventListener('x', () => { for (;;); }); t.dispatchEvent(new Event('x')); globalThis.after = 1; for (;;);",
    ),
    (
        "an event handler after a listener",
        "const c = new AbortController(); c.signal.addEventListener('abort', () => { for (;;); });
         c.signal.onabort = () => { for (;;); }; c.abort(); globalThis.after = 1; for (;;);",
    ),
    (
        "the next event handler, compiled from its attribute",
        "const d = document.createElement('div'); document.body.append(d);
         d.setAttribute('onclick', 'for (;;);'); document.body.setAttribute('onclick', 'for (;;);');
         d.click(); globalThis.after = 1; for (;;);",
    ),
    (
        "the next element to upgrade",
        "document.body.append(document.createElement('x-up'), document.createElement('x-up'));
         customElements.define('x-up', class extends HTMLElement { constructor() { super(); for (;;); } });
         globalThis.after = 1; for (;;);",
    ),
    (
        "the next lifecycle callback",
        "customElements.define('x-in', class extends HTMLElement { connectedCallback() { for (;;); } });
         document.body.append(document.createElement('x-in'), document.createElement('x-in')); globalThis.after = 1; for (;;);",
    ),
    (
        "the next script element",
        "const a = document.createElement('script'); a.textContent = 'for (;;);';
         const b = document.createElement('script'); b.textContent = 'for (;;);';
         document.body.append(a, b); globalThis.after = 1; for (;;);",
    ),
    (
        // Its event's wrapper made first (a new event, the DOM's): making it
        // clears the stop, which only asking for it again keeps.
        "a dependent signal's listener",
        "const c = new AbortController(); const s = AbortSignal.any([c.signal]);
         c.signal.onabort = () => { for (;;); }; s.onabort = () => { for (;;); };
         c.abort(); globalThis.after = 1; for (;;);",
    ),
    (
        "a promise settled after a listener",
        "const c = new AbortController(); const s = AbortSignal.any([c.signal]);
         c.signal.onabort = () => { for (;;); };
         fetch('https://example.test/x', { signal: s }).catch(() => {}); c.abort(); globalThis.after = 1; for (;;);",
    ),
];

#[test]
fn script_called_back_stops_with_what_called_it() {
    for &(name, source, then_tasks) in CALLED_BACK {
        stops(name, source, then_tasks, Duration::from_millis(30));
    }
    for &(name, source) in CALLED_BACK_AGAIN {
        stops(name, source, false, Duration::from_millis(30));
    }
}

/// Stopped at once, before it got far, and at points through busy script
/// that keeps calling the DOM: no panic in the bindings, wherever V8 gives
/// up.
#[test]
fn script_stops_at_any_point_of_its_calls_to_the_dom() {
    const BUSY: &[&str] = &[
        "for (;;) { const e = new Event('x'); e.type; e.timeStamp; }",
        "for (;;) { const d = document.createElement('div'); d.id = 'a' + Math.random(); \
         d.append(document.createTextNode(d.id)); document.body.append(d); d.remove(); }",
        "const t = new EventTarget(); t.addEventListener('x', e => { new CustomEvent('y', { detail: e }); }); \
         for (;;) t.dispatchEvent(new Event('x'));",
        "for (;;) { try { document.createElement('1'); } catch (e) { e.name; } }",
        "for (;;) { const c = new AbortController(); c.signal.onabort = () => {}; c.abort('why'); \
         AbortSignal.any([c.signal]); }",
        "for (;;) { new URL('a?b=' + Math.random(), 'https://example.test/').searchParams.get('b'); \
         new Request('/x'); new Response('y'); }",
        "for (;;) { const ch = new MessageChannel(); ch.port1.postMessage({ a: [1, 'b'] }); ch.port2.start(); }",
        "for (;;) { document.body.innerHTML = '<p>a<b>c</b></p>'.repeat(10); document.querySelectorAll('b').length; }",
        // Stopped in the middle of parsing a body's JSON, as often as not.
        "const big = '[' + '0,'.repeat(2e6) + '0]'; for (;;) new Response(big).json();",
    ];
    for (index, source) in BUSY.iter().enumerate() {
        for after_us in [0, 200, 5_000] {
            stops(
                &format!("busy script {index} after {after_us} µs"),
                source,
                false,
                Duration::from_micros(after_us),
            );
        }
    }
}
