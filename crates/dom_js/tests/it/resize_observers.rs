//! `ResizeObserver` from script: the interfaces, the constructor, and
//! `observe`'s, `unobserve`'s and `disconnect`'s arguments and what they
//! leave in the DOM; a round of the rendering update's gather and
//! broadcast as the renderer runs it (`ScriptEngine::
//! gather_and_broadcast_resizes`, no layout: every size 0), and the entry's
//! members. The renderer runs the loop (`shell`'s tests drive it).

use std::time::Instant;

use dom::ResizeObserverBox;
use dom_api::{ClassicScript, EngineSettings, ScriptEngine};
use dom_js::V8Engine;

/// A window whose document has `<html><body><p id=d>`; `log`, `el(id)`, a
/// new element with that id, and `error(f)`, the name of what `f` throws.
struct Page {
    document: dom::NodeRoot,
    engine: V8Engine,
}

impl Page {
    fn new() -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let document = engine
            .create_window(engine.top_frame(), "https://example.test/", Instant::now())
            .expect("a window");
        let mut page = Self { document, engine };
        page.run(
            "var log = [];
             document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
             var el = id => { const e = document.createElement('p'); e.setAttribute('id', id); return e; };
             var d = el('d');
             document.body.appendChild(d);
             var error = f => {
                 try { f(); return 'no error'; } catch (e) {
                     return e instanceof DOMException ? e.name : e.constructor.name;
                 }
             };",
        );
        page
    }

    fn run(&mut self, source: &str) {
        self.engine.run_classic_script(
            self.engine.top_frame(),
            &ClassicScript {
                source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    }

    /// `log`, joined with ` / `.
    fn log(&mut self) -> String {
        self.run("testRunner.reportHarnessResults(log.join(' / '))");
        self.engine.take_harness_results().expect("reported")
    }

    /// The ids of the targets of each of the window's observers, and their
    /// boxes, in order.
    fn observations(&mut self) -> Vec<Vec<(String, ResizeObserverBox)>> {
        let document = &self.document;
        self.engine.dom_mut().read(|cx| {
            let window = cx
                .window_of(document.get(cx.gc))
                .expect("the document's window");
            cx.window_resize_observers(window)
                .into_iter()
                .map(|observer| {
                    cx.resize_observations(observer)
                        .into_iter()
                        .map(|o| {
                            let id = cx
                                .get_attribute(o.target, "id")
                                .map(|id| id.to_string())
                                .unwrap_or_default();
                            (id, o.observed_box)
                        })
                        .collect()
                })
                .collect()
        })
    }
}

/// The three interfaces: only `ResizeObserver` constructs, with `new` and
/// a function; the others are what the rendering update makes.
#[test]
fn the_interfaces() {
    let mut page = Page::new();
    page.run(
        "log.push(typeof ResizeObserver, typeof ResizeObserverEntry, typeof ResizeObserverSize);
         log.push(error(() => ResizeObserver(() => {})));
         log.push(error(() => new ResizeObserver()));
         log.push(error(() => new ResizeObserver({})));
         log.push(error(() => new ResizeObserverEntry()));
         log.push(error(() => new ResizeObserverSize()));
         var observer = new ResizeObserver(() => {});
         log.push(observer instanceof ResizeObserver,
                  Object.getPrototypeOf(observer) === ResizeObserver.prototype,
                  String(observer));
         log.push(['observe', 'unobserve', 'disconnect']
             .every(name => typeof ResizeObserver.prototype[name] === 'function'));
         log.push(['target', 'contentRect', 'borderBoxSize', 'contentBoxSize',
                   'devicePixelContentBoxSize']
             .every(name => name in ResizeObserverEntry.prototype));
         log.push(['inlineSize', 'blockSize'].every(name => name in ResizeObserverSize.prototype));",
    );
    assert_eq!(
        page.log(),
        "function / function / function / TypeError / TypeError / TypeError / TypeError / \
         TypeError / true / true / [object ResizeObserver] / true / true / true"
    );
}

/// `observe` takes an element and a box (`content-box` when not given);
/// observing again with the same box does nothing, with another replaces,
/// last (as Chrome does); `unobserve` and `disconnect` take observations
/// away. Each observer is one of the window's, in the order they were
/// made.
#[test]
fn observe_unobserve_and_disconnect() {
    let mut page = Page::new();
    page.run(
        "var first = new ResizeObserver(() => {});
         var second = new ResizeObserver(() => {});
         var a = el('a');
         first.observe(d);
         first.observe(a, {box: 'border-box'});
         second.observe(d, {box: 'device-pixel-content-box'});
         first.observe(d, {});
         first.observe(d, {box: 'border-box'});",
    );
    assert_eq!(
        page.observations(),
        [
            vec![
                ("a".to_owned(), ResizeObserverBox::BorderBox),
                ("d".to_owned(), ResizeObserverBox::BorderBox),
            ],
            vec![("d".to_owned(), ResizeObserverBox::DevicePixelContentBox)],
        ]
    );
    page.run("first.unobserve(d); first.unobserve(el('never')); second.disconnect(); second.disconnect();");
    assert_eq!(
        page.observations(),
        [vec![("a".to_owned(), ResizeObserverBox::BorderBox)], vec![]]
    );
}

/// What `observe` and `unobserve` refuse: anything but an element, and a
/// box that isn't one of the three; nothing is observed then.
#[test]
fn the_arguments_errors() {
    let mut page = Page::new();
    page.run(
        "var observer = new ResizeObserver(() => {});
         log.push(error(() => observer.observe()));
         log.push(error(() => observer.observe(null)));
         log.push(error(() => observer.observe(document)));
         log.push(error(() => observer.observe(document.createTextNode('t'))));
         log.push(error(() => observer.observe(d, {box: 'padding-box'})));
         log.push(error(() => observer.observe(d, 'content-box')));
         log.push(error(() => observer.unobserve(document)));
         log.push(error(() => observer.observe(document.createElementNS(
             'http://www.w3.org/2000/svg', 'rect'))));",
    );
    assert_eq!(
        page.log(),
        "TypeError / TypeError / TypeError / TypeError / TypeError / TypeError / TypeError / \
         no error"
    );
    assert_eq!(
        page.observations(),
        [vec![(String::new(), ResizeObserverBox::ContentBox)]]
    );
}

/// A round of the rendering update calls each observer back with its
/// entries (a new array), `this` and the second argument the observer, a
/// microtask checkpoint after each callback; an entry's target, its rect
/// (the same object each time), its size lists (frozen, the same arrays
/// each time, of the same objects). With no layout every size is 0; the
/// depth broadcast is the target's.
#[test]
fn a_round_calls_the_observers_back() {
    let mut page = Page::new();
    page.run("var idle = new ResizeObserver(() => {});");
    assert!(
        !page.engine.has_resize_observations(page.engine.top_frame()),
        "none observing"
    );
    page.run(
        "var first = new ResizeObserver(function (entries, second) {
             log.push(`first ${this === first} ${second === first} ${Array.isArray(entries)} ${entries.length}`);
             const e = entries[0];
             log.push(e instanceof ResizeObserverEntry, e.target === d, e.contentRect === e.contentRect,
                      e.contentRect instanceof DOMRectReadOnly, e.contentRect instanceof DOMRect,
                      e.borderBoxSize === e.borderBoxSize, Object.isFrozen(e.contentBoxSize),
                      e.contentBoxSize !== e.borderBoxSize,
                      e.devicePixelContentBoxSize[0] === e.devicePixelContentBoxSize[0],
                      e.contentBoxSize[0] instanceof ResizeObserverSize,
                      `${e.contentBoxSize[0].inlineSize}x${e.contentBoxSize[0].blockSize}`,
                      `${e.contentRect.width}x${e.contentRect.height}`);
             Promise.resolve().then(() => log.push('microtask'));
         });
         first.observe(d);
         var second = new ResizeObserver(() => log.push('second'));
         second.observe(d);",
    );
    assert!(page.engine.has_resize_observations(page.engine.top_frame()));
    let round = page
        .engine
        .gather_and_broadcast_resizes(page.engine.top_frame(), 0);
    assert_eq!(round.broadcast, Some(3), "html, body, d");
    assert!(!round.skipped);
    assert_eq!(
        page.log(),
        "first true true true 1 / true / true / true / true / false / true / true / true / true / true / \
         0x0 / 0x0 / microtask / second"
    );
    // Reported: nothing the next round.
    let round = page
        .engine
        .gather_and_broadcast_resizes(page.engine.top_frame(), 0);
    assert_eq!(round, dom_api::ResizeObserverRound::default());
}
