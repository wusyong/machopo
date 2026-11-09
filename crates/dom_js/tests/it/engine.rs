//! The DOM's engine as the renderer drives it (`V8Engine`, a
//! `dom_api::ScriptEngine`): a window's realm and document, classic
//! scripts run in it (what they throw reported at the window), microtask
//! checkpoints, `testRunner` with web tests, the time origin given, its
//! timers and element tasks.

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

#[test]
fn a_windows_scripts_run_in_its_realm() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    // No window yet: nothing runs.
    run(&mut engine, "throw 1");
    assert_eq!(engine.take_harness_results(), None);
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    // The document is the one the realm's `document` gives, empty for the
    // parser to fill.
    let children = engine
        .dom_mut()
        .read(|cx| cx.first_child(document.get(cx.gc)).is_none());
    assert!(children);
    run(
        &mut engine,
        "var log = [location.href, document.nodeName]; \
         Promise.resolve().then(() => log.push('micro')); \
         window.addEventListener('error', e => log.push(e.message + ' ' + e.lineno));",
    );
    // A syntax error is reported too, at its line.
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source: "\n\n(",
            url: "https://example.test/page.html",
            base_url: None,
            line: 10,
            muted: false,
            use_credentials: false,
        },
    );
    run(&mut engine, "testRunner.reportHarnessResults(log.join())");
    assert_eq!(
        engine.take_harness_results().as_deref(),
        Some(
            "https://example.test/page.html,#document,Uncaught SyntaxError: Unexpected end of input 12"
        ),
        "no microtask ran yet"
    );
    assert_eq!(engine.take_harness_results(), None, "taken");
    engine.perform_microtask_checkpoint();
    run(
        &mut engine,
        "testRunner.reportHarnessResults(log[log.length - 1])",
    );
    assert_eq!(engine.take_harness_results().as_deref(), Some("micro"));
    // A new window replaces the old: a new realm.
    engine
        .create_window(
            engine.top_frame(),
            "https://example.test/two.html",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "testRunner.reportHarnessResults(typeof log + ' ' + location.pathname)",
    );
    assert_eq!(
        engine.take_harness_results().as_deref(),
        Some("undefined /two.html")
    );
}

/// An event's time counts from the time origin given (a navigation's
/// start, before the window was made); without web tests there is no
/// `testRunner`.
#[test]
fn time_counts_from_the_origin_given() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(30));
    engine
        .create_window(engine.top_frame(), "about:blank", started)
        .expect("a window");
    run(
        &mut engine,
        "testRunner.reportHarnessResults(String(new Event('x').timeStamp >= 25))",
    );
    assert_eq!(engine.take_harness_results().as_deref(), Some("true"));
    // And one after the DOM was made: not counted from the DOM's start, 30
    // ms before its origin, but at most the time since the origin (however
    // long the GCs of a stressed run take meanwhile; its clock is
    // coarsened, hence the millisecond).
    let mut later = V8Engine::new(EngineSettings { web_tests: true });
    std::thread::sleep(Duration::from_millis(30));
    let origin = Instant::now();
    later
        .create_window(later.top_frame(), "about:blank", origin)
        .expect("a window");
    run(
        &mut later,
        "testRunner.reportHarnessResults(String(new Event('x').timeStamp))",
    );
    let since = origin.elapsed().as_secs_f64() * 1e3;
    let stamp: f64 = later
        .take_harness_results()
        .expect("the event's time")
        .parse()
        .expect("a number");
    assert!(
        stamp <= since + 1.0,
        "{stamp} ms, {since} ms since the origin"
    );
    let mut plain = V8Engine::new(EngineSettings::default());
    let document = plain
        .create_window(plain.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    run(
        &mut plain,
        "document.appendChild(document.createElement(typeof testRunner))",
    );
    let name = plain.dom_mut().read(|cx| {
        let element = cx
            .first_child(document.get(cx.gc))
            .expect("the script's element");
        cx.node_name(element).to_string()
    });
    assert_eq!(name, "UNDEFINED");
}

fn report(engine: &mut V8Engine, expression: &str) -> String {
    run(
        engine,
        &format!("testRunner.reportHarnessResults(String({expression}))"),
    );
    engine.take_harness_results().unwrap_or_default()
}

/// A promise rejected with no handler at a checkpoint's end fires an
/// `unhandledrejection` at the window (a `PromiseRejectionEvent`: its
/// promise and reason, trusted, cancelable); one given a handler before
/// doesn't; a listener canceling it keeps it from the log.
#[test]
fn unhandled_rejections_fire_at_the_window() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    run(
        &mut engine,
        "var log = []; var cancel = false; \
         window.addEventListener('unhandledrejection', e => { \
           log.push([e instanceof PromiseRejectionEvent, e.reason, e.promise === rejected, e.isTrusted, e.cancelable].join()); \
           if (cancel) e.preventDefault(); }); \
         var rejected = Promise.reject('why'); \
         Promise.reject('handled').catch(() => {});",
    );
    engine.perform_microtask_checkpoint();
    assert_eq!(
        report(&mut engine, "log.join('|')"),
        "true,why,true,true,true"
    );
    run(
        &mut engine,
        "cancel = true; log = []; rejected = Promise.reject(new Error('e'));",
    );
    engine.perform_microtask_checkpoint();
    assert_eq!(
        report(&mut engine, "log.join('|')"),
        "true,Error: e,true,true,true"
    );
}

/// `console`'s messages go to the sink given, with where they came from.
#[test]
fn console_messages_go_to_the_sink() {
    use std::cell::RefCell;
    use std::rc::Rc;
    let mut engine = V8Engine::new(EngineSettings::default());
    let messages = Rc::new(RefCell::new(Vec::new()));
    let sink = messages.clone();
    engine.set_console_sink(Box::new(move |message| sink.borrow_mut().push(message)));
    engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source: "\nconsole.log('a', 1, {});\nconsole.error('b');",
            url: "https://example.test/c.js",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
    let messages: Vec<_> = messages
        .borrow()
        .iter()
        .map(|message| (message.text.clone(), message.url.clone(), message.line))
        .collect();
    assert_eq!(
        messages,
        [
            (
                "a 1 [object Object]".to_owned(),
                "https://example.test/c.js".to_owned(),
                2
            ),
            ("b".to_owned(), "https://example.test/c.js".to_owned(), 3),
        ]
    );
}

/// The document's loading through the engine: `readyState`, then the
/// events of each step with a checkpoint after them.
#[test]
fn the_documents_loading_through_the_engine() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    // Loading, as the parser's.
    engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    run(
        &mut engine,
        "var log = [document.readyState]; \
         document.addEventListener('readystatechange', () => log.push(document.readyState)); \
         document.addEventListener('DOMContentLoaded', () => Promise.resolve().then(() => log.push('micro'))); \
         window.addEventListener('load', e => log.push('load ' + (e.target === document)));",
    );
    engine.document_ready(engine.top_frame(), dom_api::DocumentReadiness::Interactive);
    engine.document_ready(
        engine.top_frame(),
        dom_api::DocumentReadiness::ContentLoaded,
    );
    assert_eq!(
        report(&mut engine, "log.join()"),
        "loading,interactive,micro"
    );
    engine.document_ready(engine.top_frame(), dom_api::DocumentReadiness::Loaded);
    assert_eq!(
        report(&mut engine, "log.join()"),
        "loading,interactive,micro,complete,load true"
    );
}

/// Timers through the engine: how long until the next, run once due.
#[test]
fn timers_through_the_engine() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    assert_eq!(engine.until_next_timer(), None);
    assert!(!engine.run_timers());
    run(
        &mut engine,
        "var log = []; setTimeout(() => log.push('ran'), 10000); setTimeout('log.push(\"string\")', 0);",
    );
    let wait = engine.until_next_timer().unwrap();
    assert!(wait < Duration::from_millis(50), "{wait:?}");
    std::thread::sleep(wait);
    assert!(engine.run_timers());
    assert_eq!(report(&mut engine, "log.join()"), "string");
    let wait = engine.until_next_timer().unwrap();
    assert!(wait > Duration::from_secs(5), "{wait:?}");
    let _ = engine.run_platform_tasks();
}

/// Element tasks through the engine: due at once when queued, each run with
/// a microtask checkpoint after it.
#[test]
fn element_tasks_through_the_engine() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    assert!(!engine.run_element_tasks());
    run(
        &mut engine,
        "var log = []; var i = document.appendChild(document.createElement('input')); i.value = 'abc'; \
         i.addEventListener('select', () => { log.push('select'); queueMicrotask(() => log.push('microtask')); }); \
         i.setSelectionRange(0, 1); i.setSelectionRange(0, 2); setTimeout(() => {}, 10000);",
    );
    assert_eq!(engine.until_next_timer(), Some(Duration::ZERO));
    assert!(engine.run_element_tasks());
    assert_eq!(
        report(&mut engine, "log.join()"),
        "select,microtask,select,microtask"
    );
    let wait = engine.until_next_timer().unwrap();
    assert!(wait > Duration::from_secs(5), "{wait:?}");
    assert!(!engine.run_element_tasks());
    let _ = engine.run_platform_tasks();
}

/// What changes the DOM out of script (the parser) is delivered to its
/// observers at the next microtask checkpoint, the engine queuing the
/// microtask first.
#[test]
fn the_parsers_changes_reach_observers() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    let document = engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    run(
        &mut engine,
        "var log = []; new MutationObserver(records => log.push(records.length)).observe(document, { childList: true });",
    );
    engine.dom_mut().mutate(|cx| {
        let document = document.get(cx.gc);
        let html = cx.create_element(document, dom::QualName::html("html"));
        cx.insert(document, html, None);
    });
    engine.perform_microtask_checkpoint();
    assert_eq!(report(&mut engine, "log.join()"), "1");
}

/// What the parser connects of a defined name's (no `[CEReactions]`
/// member's scope) goes in the backup element queue: upgraded at the
/// checkpoint's microtask, not before.
#[test]
fn the_parsers_defined_elements_upgrade_at_a_checkpoint() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    let document = engine
        .create_window(engine.top_frame(), "about:blank", Instant::now())
        .expect("a window");
    run(
        &mut engine,
        "var made = 0; class P extends HTMLElement { constructor() { super(); made++; } }; \
         customElements.define('p-a', P);",
    );
    engine.dom_mut().mutate(|cx| {
        let document = document.get(cx.gc);
        let html = cx.create_element(document, dom::QualName::html("html"));
        cx.insert(document, html, None);
        for _ in 0..2 {
            let element = cx.create_element(document, dom::QualName::html("p-a"));
            cx.insert(html, element, None);
        }
    });
    assert_eq!(report(&mut engine, "made"), "0");
    engine.perform_microtask_checkpoint();
    assert_eq!(
        report(
            &mut engine,
            "[made, document.documentElement.firstChild instanceof P].join()"
        ),
        "2,true"
    );
}

/// The document's parser constructs an element whose name a script
/// before it defined at once (HTML's "create an element for a token" that
/// will execute script): the microtasks run first, the constructor sees no
/// attributes, no children and none of the markup after it; then the
/// attributes' `attributeChangedCallback`, then `connectedCallback` as it
/// is inserted (an element queue around the insertion), before the markup
/// after it is parsed. A constructor that throws is reported, its element
/// failed.
#[test]
fn the_parser_constructs_defined_elements_at_once() {
    use html::parser::{Parser, ParserState};
    let mut engine = Box::new(V8Engine::new(EngineSettings { web_tests: true }));
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    let mut parser = Parser::<V8Engine>::new(engine.dom_mut(), &document);
    parser.push(
        "<!DOCTYPE html><script>var log = []; Promise.resolve().then(() => log.push('micro')); \
         class P extends HTMLElement { static observedAttributes = ['a']; \
           constructor() { super(); log.push('made ' + this.hasAttributes() + ' ' + (this.firstChild === null) + ' ' + \
             (document.getElementById('after') === null)); } \
           attributeChangedCallback(n, o, v) { log.push(n + '=' + v); } \
           connectedCallback() { log.push('connected ' + (this.parentNode !== null) + ' ' + \
             (document.getElementById('after') === null)); } }; \
         customElements.define('p-a', P); \
         customElements.define('p-t', class extends HTMLElement { constructor() { super(); throw new Error('t'); } }); \
         var errors = 0; window.addEventListener('error', e => { errors++; e.preventDefault(); });</script>\
         <p-a id=x a=1><i></i></p-a><p id=after></p><p-t id=t></p-t>",
    );
    let (state, back) = parser.resume(engine);
    engine = back;
    let ParserState::Script { element, line } = state else {
        panic!("stops at the script");
    };
    let source = engine.dom_mut().read(|cx| {
        cx.text_content(element.root().get(cx.gc))
            .unwrap()
            .to_string()
    });
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source: &source,
            url: "https://example.test/page.html",
            base_url: None,
            line: u32::try_from(line).unwrap(),
            muted: false,
            use_credentials: false,
        },
    );
    let (state, back) = parser.resume(engine);
    engine = back;
    assert!(matches!(state, ParserState::NeedsInput));
    engine = parser.finish(engine);
    assert_eq!(
        report(&mut engine, "log.join('|')"),
        "micro|made false true true|a=1|connected true true"
    );
    assert_eq!(
        report(
            &mut engine,
            "[document.getElementById('x') instanceof P, document.getElementById('x').firstChild.nodeName, \
                             errors, document.getElementById('t').constructor === HTMLUnknownElement].join()"
        ),
        "true,I,1,true"
    );
    drop(element);
    drop(document);
}

/// A declarative shadow root the parser attached: `attachShadow` of the
/// same mode takes it over (emptied, the same root, not declarative: a
/// second call throws), another mode throws.
#[test]
fn attach_shadow_takes_over_a_declarative_shadow_root() {
    use html::parser::Parser;
    let mut engine = Box::new(V8Engine::new(EngineSettings { web_tests: true }));
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    let mut parser = Parser::<V8Engine>::new(engine.dom_mut(), &document);
    parser.push(
        "<!DOCTYPE html><div id=h><template shadowrootmode=open><span>in</span></template>light</div>\
         <div id=c><template shadowrootmode=closed></template></div>",
    );
    let (_, back) = parser.resume(engine);
    engine = parser.finish(back);
    assert_eq!(
        report(
            &mut engine,
            "(() => { try { var h = document.getElementById('h'); var root = h.shadowRoot; \
             var before = [root.mode, root.firstChild.nodeName, h.firstChild.nodeValue].join(); \
             var again = h.attachShadow({ mode: 'open' }); \
             var e; try { h.attachShadow({ mode: 'open' }); } catch (x) { e = x.name; } \
             var c; try { document.getElementById('c').attachShadow({ mode: 'open' }); } catch (x) { c = x.name; } \
             return [before, again === root, root.childNodes.length, e, c].join('|'); } catch (x) { return 'threw ' + x; } })()"
        ),
        "open,SPAN,light|true|0|NotSupportedError|NotSupportedError"
    );
    drop(document);
}

/// A frame's layout that holds the document (a root, as the renderer's
/// does) goes with the engine before its DOM's heap: the engine drops
/// with no root left over (a debug build's heap panics on one).
#[test]
fn a_layout_holding_the_document_goes_before_the_heap() {
    /// The document's root, held only to go with the engine.
    struct Holding(
        #[expect(dead_code, reason = "held, not read")] dom::NodeRoot,
        dom::NodeId,
    );
    impl dom_api::FrameLayout for Holding {
        fn shows(&mut self, document: dom::NodeId) -> bool {
            document == self.1
        }
        fn update_style_and_layout(&mut self, _: &dom::Cx<'_, '_>) {}
        fn element_box(&self, _: dom::NodeId) -> Option<gfx::ElementBox> {
            None
        }
        fn client_rects(&self, _: dom::NodeId) -> Vec<gfx::RectF> {
            Vec::new()
        }
        fn viewport(&self) -> gfx::Viewport {
            gfx::Viewport {
                size: gfx::SizeF::new(0.0, 0.0),
                device_scale_factor: 1.0,
                scroll: gfx::PointF::new(0.0, 0.0),
                content_size: gfx::SizeF::new(0.0, 0.0),
            }
        }
        fn window_screen_rect(&self) -> Option<gfx::RectF> {
            None
        }
        fn computed_style<'gc>(
            &self,
            _: &dom::Cx<'_, 'gc>,
            _: dom::NodeRef<'gc>,
        ) -> Option<std::sync::Arc<computed_style::ComputedStyle>> {
            None
        }
        fn element_at(&mut self, _: gfx::PointF) -> Option<dom::NodeId> {
            None
        }
        fn elements_at(&mut self, _: gfx::PointF) -> Vec<dom::NodeId> {
            Vec::new()
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
    }
    let mut engine = V8Engine::new(EngineSettings { web_tests: false });
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/page.html",
            Instant::now(),
        )
        .expect("a window");
    let id = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
    engine.set_layout(Box::new(Holding(document, id)));
    drop(engine);
}
