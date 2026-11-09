//! `performance`: its clock (`now()`, `timeOrigin`), User Timing's marks
//! and measures (their times, `detail`, the buffer and its order, the
//! errors), `PerformanceObserver` (its task, its modes, `buffered`,
//! `takeRecords()`, `disconnect()`, `supportedEntryTypes`, the console's
//! warnings of what it ignores), and Navigation
//! Timing (the navigation entry of the timing the host gave, the
//! document's times marked as its events fire, `performance.timing` and
//! `performance.navigation`).

use dom::{NavigationTiming, NavigationType};
use dom_api::{DocumentReadiness, ScriptEngine};

use crate::history::Page;

#[test]
fn the_clock_and_user_timing() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var r = [performance === window.performance, performance instanceof EventTarget, Object.prototype.toString.call(performance), \
             performance.now() >= 0, Math.abs(performance.timeOrigin + performance.now() - Date.now()) < 1000]; \
             var t = performance.now() * 10; r.push(Math.abs(t - Math.round(t)) < 1e-6); r.join()"
        ),
        "true,true,[object Performance],true,true,true"
    );
    assert_eq!(
        page.eval(
            "var a = performance.mark('a', { startTime: 5, detail: { x: [1] } }); var b = performance.mark('b', { startTime: 2 }); \
             var r = [a instanceof PerformanceMark, a.entryType, a.name, a.startTime, a.duration, JSON.stringify(a.detail), a.detail === a.detail, String(b.detail)]; \
             r.push(performance.getEntries().map(e => e.name).join('|'), performance.getEntriesByType('mark').length, performance.getEntriesByName('a')[0] === a); \
             var m = performance.measure('m', 'b', 'a'); r.push(m instanceof PerformanceMeasure, m.startTime, m.duration); \
             r.push(performance.measure('o', { start: 1, duration: 3 }).duration, performance.measure('p', { end: 'a', duration: 1 }).startTime, \
             performance.measure('q', { start: 'b', end: 4, detail: 7 }).detail, performance.measure('s').startTime); \
             var c = new PerformanceMark('c'); r.push(performance.getEntriesByName('c').length, c.startTime > 0); \
             performance.clearMarks('a'); r.push(performance.getEntriesByType('mark').map(e => e.name).join('|')); performance.clearMeasures(); \
             r.push(performance.getEntriesByType('measure').length); r.join(' ')"
        ),
        "true mark a 5 0 {\"x\":[1]} true null b|a 2 true true 2 3 3 4 7 0 0 true b 0"
    );
    assert_eq!(
        page.eval(
            "var r = []; var tries = [() => performance.mark('fetchStart'), () => performance.mark('x', { startTime: -1 }), \
             () => performance.measure('x', 'nope'), () => performance.measure('x', { start: 1 }, 'b'), () => performance.measure('x', {}), \
             () => performance.measure('x', { detail: 1 }), () => performance.measure('x', { start: 1, end: 2, duration: 1 }), \
             () => performance.measure('x', 'domInteractive'), () => performance.mark('x', { detail: () => {} })]; \
             for (var t of tries) { try { t(); r.push('ok'); } catch (e) { r.push(e.name); } } \
             r.push(performance.measure('n', 'navigationStart').startTime); \
             performance.mark('dup', { startTime: 1 }); performance.mark('dup', { startTime: 9 }); r.push(performance.measure('x', 'dup').startTime); r.join()"
        ),
        "SyntaxError,TypeError,SyntaxError,TypeError,ok,TypeError,TypeError,InvalidAccessError,DataCloneError,0,9"
    );
}

/// `timeOrigin` is the window's time origin's Unix time: a window made
/// after its DOM has a later one.
#[test]
fn the_time_origin_is_the_windows() {
    use std::time::{Duration, Instant};

    use dom_api::{ClassicScript, EngineSettings};
    let mut engine = dom_js::V8Engine::new(EngineSettings { web_tests: true });
    std::thread::sleep(Duration::from_millis(60));
    let top = engine.top_frame();
    engine
        .create_window(top, "https://example.test/", Instant::now())
        .expect("a window");
    engine.run_classic_script(top, &ClassicScript {
        source: "testRunner.reportHarnessResults(String(Math.abs(performance.timeOrigin + performance.now() - Date.now()) < 30))",
        url: "https://example.test/",
        base_url: None,
        line: 1,
        muted: false,
        use_credentials: false,
    });
    assert_eq!(engine.take_harness_results().as_deref(), Some("true"));
}

/// A navigation that started before the engine (a renderer's first): the
/// window's time origin is before the DOM was made, and its clock counts
/// from there.
#[test]
fn a_time_origin_before_the_engine() {
    use std::time::{Duration, Instant};

    use dom_api::{ClassicScript, EngineSettings};
    let started = Instant::now()
        .checked_sub(Duration::from_millis(500))
        .expect("a monotonic clock past half a second");
    let mut engine = dom_js::V8Engine::new(EngineSettings { web_tests: true });
    let top = engine.top_frame();
    drop(
        engine
            .create_window(top, "https://example.test/", started)
            .expect("a window"),
    );
    engine.run_classic_script(top, &ClassicScript {
        source: "testRunner.reportHarnessResults(String(performance.now() >= 500 && Math.abs(performance.timeOrigin + performance.now() - Date.now()) < 30))",
        url: "https://example.test/",
        base_url: None,
        line: 1,
        muted: false,
        use_credentials: false,
    });
    assert_eq!(engine.take_harness_results().as_deref(), Some("true"));
}

#[test]
fn observers_are_called_in_a_task() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var calls = []; var o = new PerformanceObserver((list, observer, options) => calls.push([list.getEntries().map(e => e.name).join('|'), \
         list.getEntriesByType('mark').length, list.getEntriesByName('y').length, observer === o, JSON.stringify(options)].join(' '))); \
         performance.mark('early'); o.observe({ type: 'mark', buffered: true }); performance.mark('y', { startTime: 0 }); 0",
    );
    assert_eq!(page.eval("calls.length"), "0", "not yet: a task");
    while page.engine.run_element_tasks() {}
    page.eval("performance.mark('z'); performance.measure('w'); 0");
    while page.engine.run_element_tasks() {}
    assert_eq!(
        page.eval("calls.join(' ; ')"),
        "y|early 2 1 true {\"droppedEntriesCount\":0} ; z 1 0 true {}"
    );
    assert_eq!(
        page.eval(
            "var r = []; try { o.observe({ entryTypes: ['mark'] }); } catch (e) { r.push(e.name); } \
             try { o.observe({}); } catch (e) { r.push(e.name); } try { o.observe({ type: 'mark', entryTypes: ['mark'] }); } catch (e) { r.push(e.name); } \
             performance.mark('t'); r.push(o.takeRecords().map(e => e.name).join(), o.takeRecords().length); \
             var p = new PerformanceObserver(() => r.push('p called')); p.observe({ entryTypes: ['measure', 'nope'] }); try { p.observe({ type: 'mark' }); } catch (e) { r.push(e.name); } \
             o.disconnect(); performance.mark('u'); \
             var q = new PerformanceObserver(() => {}); q.observe({ entryTypes: ['mark'] }); q.observe({ entryTypes: ['measure'] }); performance.mark('v'); r.push(q.takeRecords().length); \
             var q2 = new PerformanceObserver(() => {}); q2.observe({ type: 'mark' }); q2.disconnect(); q2.observe({ type: 'measure' }); performance.mark('v2'); r.push(q2.takeRecords().length); \
             r.push(JSON.stringify(PerformanceObserver.supportedEntryTypes), Object.getOwnPropertyDescriptor(PerformanceObserver.prototype, 'supportedEntryTypes')); r.join()"
        ),
        "InvalidModificationError,TypeError,TypeError,t,0,InvalidModificationError,0,0,[\"mark\",\"measure\",\"navigation\"],"
    );
    // A frozen array, the same each time.
    assert_eq!(
        page.eval(
            "var types = PerformanceObserver.supportedEntryTypes; \
             [types === PerformanceObserver.supportedEntryTypes, Object.isFrozen(types)].join()"
        ),
        "true,true"
    );
    while page.engine.run_element_tasks() {}
    assert_eq!(
        page.eval("calls.length"),
        "2",
        "disconnected; `p` observes measures only"
    );
}

/// An entry type machopo doesn't make, and `buffered` with `entryTypes`, are
/// warned of in the page's console, with Chrome 152's text.
#[test]
fn what_observe_ignores_is_warned_of() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "new PerformanceObserver(() => {}).observe({ type: 'nope' }); \
         new PerformanceObserver(() => {}).observe({ entryTypes: ['mark', 'x'], buffered: true }); \
         new PerformanceObserver(() => {}).observe({ type: 'mark', buffered: true }); 0",
    );
    assert_eq!(
        page.engine.dom_mut().take_console_warnings(),
        [
            "The entry type 'nope' does not exist or isn't supported.",
            "The entry type 'x' does not exist or isn't supported.",
            "The PerformanceObserver does not support buffered flag with the entryTypes argument.",
        ]
    );
    assert_eq!(
        page.engine.dom_mut().take_console_warnings(),
        Vec::<String>::new()
    );
}

#[test]
fn the_navigation_entry_and_the_legacy_timing() {
    let mut page = Page::new("https://example.test/doc");
    let top = page.engine.top_frame();
    page.engine.set_navigation_timing(
        top,
        NavigationTiming {
            navigation_type: NavigationType::Reload,
            fetch_start: Some(1.0),
            request_start: Some(2.0),
            response_start: Some(3.0),
            response_end: Some(4.0),
            next_hop_protocol: "h2".into(),
            encoded_body_size: 10,
            decoded_body_size: 20,
            transferred: true,
            response_status: 200,
            content_type: "text/html".into(),
            dom_loading: Some(4.5),
            confidence_high: true,
            ..NavigationTiming::default()
        },
    );
    page.eval("var log = []; var o = new PerformanceObserver(list => log.push(list.getEntries()[0].entryType)); o.observe({ type: 'navigation' }); 0");
    page.engine
        .document_ready(top, DocumentReadiness::Interactive);
    page.engine
        .document_ready(top, DocumentReadiness::ContentLoaded);
    assert_eq!(
        page.eval(
            "var n = performance.getEntriesByType('navigation')[0]; [n instanceof PerformanceNavigationTiming, n instanceof PerformanceResourceTiming, \
             n.name, n.entryType, n.startTime, n.type, n.fetchStart, n.requestStart, n.responseStart, n.responseEnd, n.nextHopProtocol, n.transferSize, \
             n.encodedBodySize, n.decodedBodySize, n.responseStatus, n.contentType, n.initiatorType, n.domInteractive > 0, n.domContentLoadedEventEnd >= n.domContentLoadedEventStart, \
             n.loadEventEnd, n.duration, n.confidence.value, n.confidence !== n.confidence, String(n.notRestoredReasons), JSON.stringify(n.serverTiming), n.redirectCount].join(' ')"
        ),
        "true true https://example.test/doc navigation 0 reload 1 2 3 4 h2 310 10 20 200 text/html navigation true true 0 0 high true null [] 0"
    );
    page.eval("var inLoad; addEventListener('load', () => { inLoad = performance.now(); }); 0");
    page.engine.document_ready(top, DocumentReadiness::Loaded);
    while page.engine.run_element_tasks() {}
    assert_eq!(
        page.eval(
            "var t = performance.timing; [log.join(), n.loadEventEnd >= inLoad && inLoad >= n.loadEventStart && n.loadEventStart >= n.domComplete && n.domComplete > 0, n.duration === n.loadEventEnd, \
             t === performance.timing, t.navigationStart === Math.floor(performance.timeOrigin), t.fetchStart - t.navigationStart, t.unloadEventStart, t.redirectStart, \
             t.loadEventEnd >= t.navigationStart, performance.navigation.type, performance.navigation.redirectCount, PerformanceNavigation.TYPE_RELOAD, \
             performance.measure('d', 'fetchStart', 'responseEnd').duration].join(' ')"
        ),
        "navigation true true true true 1 0 0 true 1 0 1 3"
    );
}
