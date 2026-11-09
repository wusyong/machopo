//! `IntersectionObserver` from script: the interfaces, the constructor and
//! its options, the attributes, `observe`'s, `unobserve`'s and
//! `disconnect`'s arguments, `takeRecords()` and the entry's constructor;
//! the entries the rendering update queues delivered to the callback.
//! What the rendering update makes of the intersections is
//! `dom_api`'s tests'; the renderer computes them (`shell`'s tests
//! drive it).

use std::time::Instant;

use dom_api::{ApiCx, ClassicScript, ComputedIntersection, EngineSettings, ScriptEngine};
use dom_js::V8Engine;
use gfx::RectF;

use crate::env::Env;

/// A window with `<p id=d>` in its body; `log`, `el(id)`, a new element
/// with that id, and `error(f)`, the name of what `f` throws.
fn window() -> Env {
    let mut env = Env::window("https://example.test/");
    env.eval(
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));
         var log = [];
         var el = id => { const e = document.createElement('p'); e.setAttribute('id', id); return e; };
         var d = el('d');
         document.body.appendChild(d);
         var error = f => {
             try { f(); return 'no error'; } catch (e) {
                 return e instanceof DOMException ? e.name : e.constructor.name;
             }
         }; 0",
    );
    env
}

/// `log`, joined with ` / `.
fn log(env: &mut Env) -> String {
    env.eval("log.join(' / ')")
}

/// The two interfaces: both construct, with `new`; their prototypes'
/// members, v2's included (the DOM's templates put the operations
/// before the attributes; Chrome the attributes first).
#[test]
fn the_interfaces() {
    let mut env = window();
    env.eval(
        "log.push(typeof IntersectionObserver, typeof IntersectionObserverEntry);
         log.push(error(() => IntersectionObserver(() => {})));
         log.push(error(() => new IntersectionObserver()));
         log.push(error(() => new IntersectionObserver({})));
         log.push(error(() => new IntersectionObserverEntry()));
         var observer = new IntersectionObserver(() => {});
         log.push(observer instanceof IntersectionObserver, String(observer));
         log.push(Object.getOwnPropertyNames(IntersectionObserver.prototype).join('|'));
         log.push(Object.getOwnPropertyNames(IntersectionObserverEntry.prototype).join('|'));",
    );
    assert_eq!(
        log(&mut env),
        "function / function / TypeError / TypeError / TypeError / TypeError / true / \
         [object IntersectionObserver] / \
         root|rootMargin|scrollMargin|thresholds|delay|trackVisibility|\
         disconnect|observe|takeRecords|unobserve|constructor / \
         time|rootBounds|boundingClientRect|intersectionRect|isIntersecting|isVisible|\
         intersectionRatio|target|constructor"
    );
}

/// The options as the attributes read them back (WPT's
/// `observer-attributes.html`): the root or `null`, the margins as four
/// whole px or percents (absolute units in px), the thresholds sorted, the
/// same array each time and frozen; `delay` in whole milliseconds toward
/// zero, saturating at the end of `i64`, negative ones kept, and
/// `trackVisibility` (Chrome 152's).
#[test]
fn the_options_read_back() {
    let mut env = window();
    env.eval(
        "var o = new IntersectionObserver(() => {});
         log.push(o.root, o.rootMargin, o.scrollMargin, o.thresholds.join());
         o = new IntersectionObserver(() => {}, {rootMargin: ' ', threshold: []});
         log.push(o.rootMargin, o.thresholds.join());
         o = new IntersectionObserver(() => {}, {
             root: d, rootMargin: '10% 20px', scrollMargin: '1px 2px 3px',
             threshold: [1, 0.25, 0, 0.5]});
         log.push(o.root === d, o.rootMargin, o.scrollMargin, o.thresholds.join());
         log.push(o.thresholds === o.thresholds, Object.isFrozen(o.thresholds));
         o = new IntersectionObserver(() => {}, {root: document, threshold: 0.5,
                                                 rootMargin: '10cm 1.9px'});
         log.push(o.root === document, o.thresholds.join(), o.rootMargin);
         o = new IntersectionObserver(() => {});
         log.push(o.delay, o.trackVisibility);
         for (const delay of [-5, 1.5, -1.5, 1e20, '30'])
             log.push(new IntersectionObserver(() => {}, {delay}).delay);
         o = new IntersectionObserver(() => {}, {trackVisibility: 1, delay: 100});
         log.push(o.delay, o.trackVisibility);",
    );
    assert_eq!(
        log(&mut env),
        " / 0px 0px 0px 0px / 0px 0px 0px 0px / 0 / 0px 0px 0px 0px / 0 / true / \
         10% 20px 10% 20px / 1px 2px 3px 2px / 0,0.25,0.5,1 / true / true / true / 0.5 / \
         377px 1px 377px 1px / 0 / false / -5 / 1 / -1 / 9223372036854776000 / 30 / \
         100 / true"
    );
}

/// What the constructor refuses (WPT's `observer-exceptions.html`): a
/// margin that isn't px or percents is a `SyntaxError`; a threshold
/// outside 0 to 1 a `RangeError`, not a number a `TypeError`; a root that
/// isn't an element or a document a `TypeError`. `trackVisibility` with a
/// `delay` under 100 ms is a `NotSupportedError` with Chrome's message,
/// checked before the margins and thresholds; a `delay` that isn't a
/// number a `TypeError`.
#[test]
fn the_constructors_errors() {
    let mut env = window();
    env.eval(
        "var make = options => error(() => new IntersectionObserver(() => {}, options));
         for (const rootMargin of ['1', '2em', 'auto', 'calc(1px + 2px)', '1px !important',
                                   '1px 1px 1px 1px 1px'])
             log.push(make({rootMargin}));
         log.push(make({scrollMargin: '5'}));
         log.push(make({threshold: [1.1]}), make({threshold: -0.1}),
                  make({threshold: ['foo']}), make({threshold: NaN}));
         log.push(make({root: document.createTextNode('t')}), make({root: 'd'}));
         log.push(make({threshold: [0, 1], rootMargin: '-5px 10%'}));
         log.push(make({trackVisibility: true}), make({trackVisibility: true, delay: 99.9}),
                  make({trackVisibility: true, delay: -1}),
                  make({trackVisibility: true, rootMargin: 'x'}),
                  make({trackVisibility: true, delay: 100, rootMargin: 'x'}),
                  make({trackVisibility: true, delay: 100, threshold: 2}),
                  make({delay: 99}), make({delay: NaN}));
         try { new IntersectionObserver(() => {}, {trackVisibility: true}); }
         catch (e) { log.push(e.message); }",
    );
    assert_eq!(
        log(&mut env),
        "SyntaxError / SyntaxError / SyntaxError / SyntaxError / SyntaxError / SyntaxError / \
         SyntaxError / RangeError / RangeError / TypeError / TypeError / TypeError / \
         TypeError / no error / NotSupportedError / NotSupportedError / NotSupportedError / \
         NotSupportedError / SyntaxError / RangeError / no error / TypeError / \
         Failed to construct 'IntersectionObserver': To enable the 'trackVisibility' option, \
         you must also use a 'delay' option with a value of at least 100. Visibility is more \
         expensive to compute than the basic intersection; enabling this option may \
         negatively affect your page's performance. Please make sure you *really* need \
         visibility tracking before enabling the 'trackVisibility' option."
    );
}

/// `observe`, `unobserve` and `takeRecords()` take an element; anything
/// else is a `TypeError`. `takeRecords()` with nothing queued: an empty
/// array.
#[test]
fn the_methods_arguments() {
    let mut env = window();
    env.eval(
        "var o = new IntersectionObserver(() => {});
         o.observe(d); o.observe(d); o.unobserve(el('never')); o.disconnect(); o.disconnect();
         log.push(error(() => o.observe('foo')), error(() => o.observe(document)),
                  error(() => o.unobserve(null)));
         var records = o.takeRecords();
         log.push(Array.isArray(records), records.length);",
    );
    assert_eq!(
        log(&mut env),
        "TypeError / TypeError / TypeError / true / 0"
    );
}

/// The entry's constructor makes its rects from the init's (doubles kept),
/// the root bounds `null` when given so; every member required, the
/// target an element.
#[test]
fn the_entrys_constructor() {
    let mut env = window();
    env.eval(
        "var init = {
             time: 12.5,
             rootBounds: {x: 0, y: 0, width: 800, height: 600},
             boundingClientRect: {x: 1, y: 2, width: 30, height: 40},
             intersectionRect: {x: 1.25, y: 2, width: 30, height: 20.1},
             isIntersecting: true,
             isVisible: true,
             intersectionRatio: 0.5,
             target: d,
         };
         var entry = new IntersectionObserverEntry(init);
         var r = rect => [rect.x, rect.y, rect.width, rect.height].join();
         log.push(entry.time, r(entry.rootBounds), r(entry.boundingClientRect),
                  r(entry.intersectionRect), entry.isIntersecting, entry.isVisible,
                  entry.intersectionRatio, entry.target === d,
                  entry.boundingClientRect instanceof DOMRectReadOnly,
                  entry.boundingClientRect === entry.boundingClientRect,
                  entry.rootBounds instanceof DOMRect || entry.intersectionRect instanceof DOMRect);
         log.push(new IntersectionObserverEntry({...init, rootBounds: null}).rootBounds);
         log.push(r(new IntersectionObserverEntry({...init, intersectionRect: {}}).intersectionRect));
         for (const name of ['target', 'isVisible', 'time', 'rootBounds']) {
             const {[name]: _, ...without} = init;
             log.push(error(() => new IntersectionObserverEntry(without)));
         }
         log.push(error(() => new IntersectionObserverEntry({...init, target: document})),
                  error(() => new IntersectionObserverEntry({...init, time: NaN})),
                  error(() => new IntersectionObserverEntry({...init, boundingClientRect: 1})));",
    );
    assert_eq!(
        log(&mut env),
        "12.5 / 0,0,800,600 / 1,2,30,40 / 1.25,2,30,20.1 / true / true / 0.5 / true / \
         true / true / false /  / 0,0,0,0 / TypeError / TypeError / TypeError / TypeError / \
         TypeError / TypeError / TypeError"
    );
}

/// The window's task, as the renderer runs it
/// (`ScriptEngine::notify_intersection_observers`), calls each observer with
/// entries back, in the order each got its first: its entries (a new
/// array), `this` and the second argument the observer; one whose entries
/// `takeRecords()` took isn't called. An exception is reported, and the
/// next observer called.
#[test]
fn the_task_calls_the_observers_back() {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    let document = engine
        .create_window(engine.top_frame(), "https://example.test/", Instant::now())
        .expect("a window");
    let run = |engine: &mut V8Engine, source: &str| {
        engine.run_classic_script(
            engine.top_frame(),
            &ClassicScript {
                source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
    };
    run(
        &mut engine,
        "var log = [];
         addEventListener('error', e => log.push('error ' + e.message.split(': ').pop()));
         document.appendChild(document.createElement('html'));
         var d = document.documentElement;
         var first = new IntersectionObserver(function (entries, second) {
             log.push(`first ${this === first} ${second === first} ${Array.isArray(entries)} ` +
                      entries.map(e => `${e.target === d} ${e.isIntersecting} ${e.time}`).join());
             throw new Error('thrown');
         });
         var taken = new IntersectionObserver(() => log.push('taken called'));
         var last = new IntersectionObserver(entries => log.push('last ' + entries.length));
         last.observe(d); first.observe(d); taken.observe(d);",
    );
    let queue = engine.dom_mut().mutate(|cx| {
        let document = document.get(cx.gc);
        let window = cx.window_of(document).expect("the document's window");
        let target = cx.first_child(document).expect("the root");
        let rect = Some(RectF::new(0.0, 0.0, 10.0, 10.0));
        // In the order the observers were made: `first` waits for the
        // task first.
        let computed: Vec<ComputedIntersection<'_>> = cx
            .window_intersection_observers(window)
            .into_iter()
            .map(|observer| ComputedIntersection {
                observer,
                target,
                target_rect: rect,
                intersection_rect: rect,
                root_bounds: rect,
                is_visible: false,
            })
            .collect();
        dom_api::update_intersections(&ApiCx::new(cx), window, &computed, 2.5)
    });
    assert!(queue);
    run(
        &mut engine,
        "log.push('taken ' + taken.takeRecords().length)",
    );
    engine.notify_intersection_observers();
    run(
        &mut engine,
        "testRunner.reportHarnessResults(log.join(' / '))",
    );
    assert_eq!(
        engine.take_harness_results().as_deref(),
        Some("taken 1 / first true true true true true 2.5 / error thrown / last 1")
    );
}
