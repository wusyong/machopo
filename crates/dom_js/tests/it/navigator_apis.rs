//! The navigator's objects of other specs: `geolocation` (every request
//! denied a task after the permission is asked for, the one-shot ones
//! first; a `timeout` of 0 timing out first; a hidden page's waiting until
//! it shows), `wakeLock` (granted a task after it is asked for, released by
//! `release()` and as the page is hidden, rejected while hidden),
//! `connection` (fixed values), and `scheduling.isInputPending()` (what
//! the host's monitor says, as this document may know of it).

use std::time::Instant;

use dom::NodeId;
use dom_api::{EngineSettings, InputMonitor, PendingInput, ScriptEngine};

use crate::history::Page;

fn run_tasks(page: &mut Page) {
    while page.engine.run_element_tasks() {}
}

#[test]
fn the_objects_and_the_connection() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var n = navigator; [n.geolocation === n.geolocation, n.geolocation instanceof Geolocation, n.wakeLock === n.wakeLock, n.wakeLock instanceof WakeLock, \
             n.scheduling === n.scheduling, n.scheduling instanceof Scheduling, n.connection === n.connection, n.connection instanceof NetworkInformation, \
             n.connection instanceof EventTarget, n.connection.effectiveType, n.connection.rtt, n.connection.downlink, n.connection.saveData, \
             n.scheduling.isInputPending(), GeolocationPositionError.TIMEOUT, typeof GeolocationPosition, typeof GeolocationCoordinates].join()"
        ),
        "true,true,true,true,true,true,true,true,true,4g,0,10,false,false,3,function,function"
    );
    assert_eq!(
        page.eval(
            "var r = []; n.connection.onchange = () => {}; r.push(typeof n.connection.onchange); \
             try { Object.getOwnPropertyDescriptor(GeolocationCoordinates.prototype, 'latitude').get.call(n.geolocation); } catch (e) { r.push(e.name); } r.join()"
        ),
        "function,TypeError"
    );
}

#[test]
fn geolocation_denies_every_request() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var log = []; var g = navigator.geolocation; var fail = name => e => log.push([name, e.code, e.message, e instanceof GeolocationPositionError].join()); \
             g.getCurrentPosition(() => log.push('success'), fail('a'), { timeout: 0 }); \
             var w = g.watchPosition(() => {}, fail('w')); g.getCurrentPosition(() => {}, fail('b')); \
             g.getCurrentPosition(() => {}, fail('clamped'), { timeout: -100, maximumAge: -1 }); \
             g.getCurrentPosition(() => {}, fail('infinite'), { timeout: Infinity }); \
             var w2 = g.watchPosition(() => {}, fail('w2')); g.clearWatch(w2); g.getCurrentPosition(() => {}, null); \
             var r = [w, w2, log.length]; for (var t of [() => g.getCurrentPosition(), () => g.getCurrentPosition(() => {}, 5)]) { try { t(); } catch (e) { r.push(e.name); } } r.join()"
        ),
        "1,2,0,TypeError,TypeError"
    );
    run_tasks(&mut page);
    assert_eq!(
        page.eval("log.join(' ; ')"),
        "a,3,Timeout expired,true ; clamped,3,Timeout expired,true ; b,1,User denied Geolocation,true ; infinite,1,User denied Geolocation,true ; \
         w,1,User denied Geolocation,true"
    );
    // Asked again after the answer; a hidden page's waits until it shows.
    page.eval("log = []; g.getCurrentPosition(() => {}, fail('again')); 0");
    run_tasks(&mut page);
    page.engine.set_page_visible(false);
    page.eval("g.getCurrentPosition(() => {}, fail('hidden')); 0");
    run_tasks(&mut page);
    assert_eq!(
        page.eval("log.join(' ; ')"),
        "again,1,User denied Geolocation,true"
    );
    page.engine.set_page_visible(true);
    run_tasks(&mut page);
    assert_eq!(
        page.eval("log.join(' ; ')"),
        "again,1,User denied Geolocation,true ; hidden,1,User denied Geolocation,true"
    );
}

#[test]
fn wake_locks_are_granted_and_released() {
    let mut page = Page::new("https://example.test/");
    page.eval(
        "var r = []; var s; navigator.wakeLock.request().then(x => { s = x; r.push([x instanceof WakeLockSentinel, x instanceof EventTarget, x.released, x.type].join()); \
         x.onrelease = e => r.push(['released', e.type, e.isTrusted, x.released].join()); }); \
         navigator.wakeLock.request('system').catch(e => r.push(e.name)); 0",
    );
    assert_eq!(
        page.eval("r.join(' ; ')"),
        "TypeError",
        "not granted yet: a task"
    );
    run_tasks(&mut page);
    assert_eq!(
        page.eval("r.join(' ; ')"),
        "TypeError ; true,true,false,screen"
    );
    page.eval("s.release().then(v => r.push('resolved ' + v)); s.release(); 0");
    assert_eq!(
        page.eval("r.slice(2).join(' ; ')"),
        "released,release,true,true ; resolved undefined",
        "once"
    );
    // Hidden: the active locks are released; a request is rejected, and so
    // is one granted after.
    page.eval(
        "r = []; navigator.wakeLock.request().then(x => { x.onrelease = () => r.push('hidden released'); }); \
         navigator.wakeLock.request().catch(e => r.push('meanwhile ' + e.name)); 0",
    );
    run_tasks(&mut page);
    assert_eq!(page.eval("r.join()"), "");
    page.eval("navigator.wakeLock.request().catch(e => r.push('late ' + e.name)); 0");
    page.engine.set_page_visible(false);
    page.eval("navigator.wakeLock.request().catch(e => r.push(e.name + ' ' + e.message)); 0");
    run_tasks(&mut page);
    assert_eq!(
        page.eval("r.join(' ; ')"),
        "hidden released ; NotAllowedError The requesting page is not visible ; late NotAllowedError"
    );
}

/// What the test's host says waits: one pointer event aimed at `target`,
/// a continuous one; and a key, unless none.
struct Monitor {
    target: NodeId,
    key: bool,
}

impl InputMonitor for Monitor {
    fn pending_input(&self, include_continuous: bool) -> Vec<PendingInput> {
        let mut pending = Vec::new();
        if include_continuous {
            pending.push(PendingInput::Document(self.target));
        }
        if self.key {
            pending.push(PendingInput::FocusedFrame);
        }
        pending
    }
}

#[test]
fn is_input_pending_asks_the_hosts_monitor() {
    let mut engine = dom_js::V8Engine::new(EngineSettings { web_tests: true });
    let document = engine
        .create_window(engine.top_frame(), "https://example.test/", Instant::now())
        .expect("a window");
    let id = engine.dom_mut().read(|cx| cx.node_id(document.get(cx.gc)));
    let mut page = Page { engine };
    let check = "var s = navigator.scheduling; [s.isInputPending(), s.isInputPending({ includeContinuous: true })].join()";
    assert_eq!(page.eval(check), "false,false", "no monitor");
    page.engine.set_input_monitor(Box::new(Monitor {
        target: id,
        key: false,
    }));
    assert_eq!(page.eval(check), "false,true", "a move only if asked for");
    page.engine.set_input_monitor(Box::new(Monitor {
        target: id,
        key: true,
    }));
    assert_eq!(
        page.eval(check),
        "true,true",
        "a key: the frame with the focus is this one"
    );
    page.eval("document.appendChild(document.createElement('html')); 0");
    let html = page
        .engine
        .dom_mut()
        .read(|cx| cx.node_id(cx.first_child(document.get(cx.gc)).expect("its element")));
    page.engine.set_input_monitor(Box::new(Monitor {
        target: html,
        key: false,
    }));
    assert_eq!(
        page.eval(check),
        "false,false",
        "an element's id is no document's"
    );
    // A document of the same origin with no window (a parser's): no one's.
    let top = page.engine.top_frame();
    page.engine
        .execute_webdriver_script(
            top,
            1,
            "return (window.parsed = new DOMParser().parseFromString('<p>', 'text/html')).documentElement",
            &[],
            false,
        )
        .expect("started");
    let Some((_, Ok(dom_api::WebDriverValue::Element(element)))) =
        page.engine.take_webdriver_results().pop()
    else {
        panic!("the parsed document's element");
    };
    let parsed = page.engine.dom_mut().read(|cx| {
        let element = cx.node_by_id(element).expect("alive");
        cx.node_id(cx.document_of(element))
    });
    assert_eq!(page.eval("parsed.URL === document.URL"), "true");
    page.engine.set_input_monitor(Box::new(Monitor {
        target: parsed,
        key: false,
    }));
    assert_eq!(
        page.eval(check),
        "false,false",
        "a windowless document is no one's"
    );
    // Before its heap.
    drop(document);
}
