//! Iframes from script: a child browsing context made as one is
//! connected (its realm, a window of an initial `about:blank`, whose `load`
//! fires at once), `contentDocument`, `contentWindow`, the window's
//! `parent`, `top`, `frames`, `length`, `frameElement`, `window[i]`; what
//! the host is asked (`ScriptEngine::take_frame_requests`): a frame made, to
//! navigate to `src` or `srcdoc`, gone; and a frame's new document
//! (`ScriptEngine::create_window`), its iframe's content document.

use std::time::Instant;

use dom_api::{ChildNavigation, ClassicScript, EngineSettings, FrameRequest, ScriptEngine};
use dom_js::V8Engine;

/// A window of a document at `https://example.test/a/` with
/// `<html><body>`; `log`.
fn engine() -> V8Engine {
    let mut engine = V8Engine::new(EngineSettings { web_tests: true });
    engine
        .create_window(
            engine.top_frame(),
            "https://example.test/a/",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "var log = [];
         document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));",
    );
    engine
}

fn run(engine: &mut V8Engine, source: &str) {
    engine.run_classic_script(
        engine.top_frame(),
        &ClassicScript {
            source,
            url: "https://example.test/a/",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
}

fn log(engine: &mut V8Engine) -> String {
    run(
        engine,
        "testRunner.reportHarnessResults(log.join(' / ')); log = [];",
    );
    engine.take_harness_results().expect("reported")
}

#[test]
fn an_iframe_connected_gets_a_browsing_context() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.createElement('iframe');
         log.push(String(f), String(f.contentWindow), String(f.contentDocument), frames.length);
         f.onload = () => log.push('load', f.contentDocument.readyState);
         document.body.appendChild(f);
         var w = f.contentWindow, d = f.contentDocument;
         log.push('appended', w !== window, w.parent === window, w.top === window, window.parent === window,
                  window.top === window, w.frameElement === f, String(window.frameElement), frames.length,
                  frames[0] === w, String(frames[1]), d === w.document, d.documentElement.outerHTML,
                  d.compatMode, String(w.document.defaultView === w),
                  Object.keys(window).filter((key) => /^\\d/.test(key)).join('|'), 0 in window, 1 in window);",
    );
    assert_eq!(
        log(&mut engine),
        "[object HTMLIFrameElement] / null / null / 0 / load / complete / appended / true / true / true / \
         true / true / true / null / 1 / true / undefined / true / <html><head></head><body></body></html> / \
         BackCompat / true / 0 / true / false"
    );
    let requests = engine.take_frame_requests();
    assert!(
        matches!(requests.as_slice(), [FrameRequest::Created { parent, .. }] if *parent == engine.top_frame()),
        "{requests:?}"
    );
    // A child of the child, then the first removed: both gone.
    run(
        &mut engine,
        "var g = d.createElement('iframe'); d.body.appendChild(g);
         log.push(w.length, g.contentWindow.parent === w, g.contentWindow.top === window);
         f.remove();
         log.push(String(f.contentWindow), String(f.contentDocument), frames.length, d.defaultView === w);",
    );
    assert_eq!(log(&mut engine), "1 / true / true / null / null / 0 / true");
    let requests = engine.take_frame_requests();
    let FrameRequest::Created { frame: inner, .. } = &requests[0] else {
        panic!("{requests:?}");
    };
    assert_ne!(*inner, engine.top_frame());
    assert!(
        matches!(requests[1..], [FrameRequest::Removed { .. }]),
        "the outer one: {requests:?}"
    );
    assert!(
        engine
            .create_window(*inner, "https://example.test/a/c.html", Instant::now())
            .is_none(),
        "gone with its parent"
    );
    // A custom element's `connectedCallback`, inserted with an iframe:
    // after the iframe's steps (HTML's post-connection steps come before
    // the reactions), its window there.
    run(
        &mut engine,
        "customElements.define('x-a', class extends HTMLElement {
             connectedCallback() { log.push(String(this.previousSibling.contentWindow === null)); }
         });
         var fragment = document.createDocumentFragment();
         fragment.append(document.createElement('iframe'), document.createElement('x-a'));
         document.body.append(fragment);",
    );
    assert_eq!(log(&mut engine), "false");
    // One removed before its steps ran (by the `load` of one inserted with
    // it): none.
    run(
        &mut engine,
        "var a = document.createElement('iframe'), b = document.createElement('iframe');
         a.onload = () => { log.push('a'); b.remove(); };
         b.onload = () => log.push('b');
         var fragment = document.createDocumentFragment();
         fragment.append(a, b);
         document.body.append(fragment);
         log.push(String(b.contentWindow));",
    );
    assert_eq!(log(&mut engine), "a / null");
}

#[test]
fn src_and_srcdoc_navigate_the_child() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.createElement('iframe');
         f.src = 'b.html';
         f.onload = () => log.push('load');
         document.body.appendChild(f);
         log.push(f.contentDocument.URL, f.src);",
    );
    // No `load` yet: the host loads `b.html`.
    assert_eq!(
        log(&mut engine),
        "about:blank / https://example.test/a/b.html"
    );
    let requests = engine.take_frame_requests();
    let [
        FrameRequest::Created { frame, .. },
        FrameRequest::Navigate {
            frame: navigated,
            navigation,
        },
    ] = requests.as_slice()
    else {
        panic!("{requests:?}");
    };
    assert_eq!(frame, navigated);
    assert_eq!(
        *navigation,
        ChildNavigation::Src("https://example.test/a/b.html".into())
    );
    let frame = *frame;
    // `srcdoc` wins over `src` (a `src` set with one does nothing); its
    // removal navigates to `src`; `src` to about:blank after the first, to
    // a new one.
    run(
        &mut engine,
        "f.srcdoc = '<p>hi'; f.src = 'c.html'; f.removeAttribute('srcdoc'); f.src = 'about:blank';",
    );
    let navigations: Vec<_> = engine
        .take_frame_requests()
        .into_iter()
        .map(|request| match request {
            FrameRequest::Navigate { navigation, .. } => navigation,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(
        navigations,
        [
            ChildNavigation::Srcdoc("<p>hi".into()),
            ChildNavigation::Src("https://example.test/a/c.html".into()),
            ChildNavigation::AboutBlank,
        ]
    );
    // The host's document for it: the iframe's content document now, its
    // window the same proxy (`WindowProxy`).
    run(&mut engine, "var before = f.contentWindow;");
    let document = engine
        .create_window(frame, "https://example.test/a/b.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(f.contentDocument.URL, f.contentWindow === before, f.contentWindow.parent === window,
                  f.contentDocument.readyState);",
    );
    assert_eq!(
        log(&mut engine),
        "https://example.test/a/b.html / true / true / loading"
    );
    engine.dom_mut().read(|cx| {
        let document = document.get(cx.gc);
        assert!(cx.frame_owner(document).is_some());
    });
    // A frame that went makes no window.
    run(&mut engine, "f.remove();");
    engine.take_frame_requests();
    assert!(
        engine
            .create_window(frame, "https://example.test/a/d.html", Instant::now())
            .is_none()
    );
}

/// An iframe removed discards its document and those below it: their
/// windows' `parent` and `top` null, their iframes' `contentWindow` too,
/// and no frame made for an iframe put in one (not even the iframe
/// itself, put in its own child document). One in a shadow tree gets a
/// frame; one in a document with no window, none; one moved in its
/// document, a new one.
#[test]
fn a_removed_iframes_documents_are_discarded() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow, d = f.contentDocument;
         var g = d.body.appendChild(d.createElement('iframe'));
         var gw = g.contentWindow;
         document.body.removeChild(f);
         log.push(String(f.contentWindow), String(w.parent), String(w.top), String(g.contentWindow),
                  String(gw.parent), String(gw.top), w.length, String(w.frameElement));
         var late = d.body.appendChild(d.createElement('iframe'));
         log.push(String(late.contentWindow));",
    );
    assert_eq!(
        log(&mut engine),
        "null / null / null / null / null / null / 0 / null / null"
    );
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var d = f.contentDocument;
         d.body.appendChild(f);
         log.push(String(f.contentWindow), d.body.firstChild === f, frames.length);
         var host = document.body.appendChild(document.createElement('div'));
         var s = host.attachShadow({mode: 'open'}).appendChild(document.createElement('iframe'));
         log.push(s.contentWindow.parent === window, frames.length);
         var other = document.implementation.createHTMLDocument('');
         log.push(String(other.body.appendChild(other.createElement('iframe')).contentWindow));
         var m = document.body.appendChild(document.createElement('iframe'));
         var before = m.contentWindow;
         document.body.insertBefore(m, document.body.firstChild);
         log.push(m.contentWindow !== before, String(before.parent), m.contentWindow.parent === window);",
    );
    assert_eq!(
        log(&mut engine),
        "null / true / 0 / true / 0 / null / true / null / true"
    );
}

/// A document its iframe navigated from (to another origin's: a new
/// window) is no longer fully active: its window's wake lock refuses a
/// request; the iframes in it are discarded.
#[test]
fn a_document_navigated_from_is_no_longer_fully_active() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var lock = f.contentWindow.navigator.wakeLock, d = f.contentDocument;
         var inner = d.body.appendChild(d.createElement('iframe'));
         log.push(inner.contentWindow !== null);
         f.src = 'https://other.test/';",
    );
    assert_eq!(log(&mut engine), "true");
    let requests = engine.take_frame_requests();
    let FrameRequest::Created { frame, .. } = &requests[0] else {
        panic!("{requests:?}");
    };
    let frame = *frame;
    drop(
        engine
            .create_window(frame, "https://other.test/", Instant::now())
            .expect("its frame"),
    );
    run(
        &mut engine,
        "lock.request().then(() => log.push('granted'), e => log.push(e.name));
         log.push(String(inner.contentWindow));",
    );
    engine.perform_microtask_checkpoint();
    while engine.run_element_tasks() {}
    assert_eq!(log(&mut engine), "null / NotAllowedError");
}

/// A child's realm reaches its parent's as their documents' origins are
/// the same (an initial `about:blank` its parent's); one of another
/// origin gets a `SecurityError` (`cross_origin`).
#[test]
fn realms_reach_each_other_by_origin() {
    let mut engine = engine();
    run(
        &mut engine,
        "var same = document.body.appendChild(document.createElement('iframe'));
         var other = document.body.appendChild(document.createElement('iframe'));
         var srcdoc = document.body.appendChild(document.createElement('iframe'));
         log.push(same.contentWindow.document === same.contentDocument);",
    );
    assert_eq!(log(&mut engine), "true");
    let requests = engine.take_frame_requests();
    let frames: Vec<_> = requests
        .iter()
        .filter_map(|request| match request {
            FrameRequest::Created { frame, .. } => Some(*frame),
            _ => None,
        })
        .collect();
    let [same, other, srcdoc] = frames[..] else {
        panic!("{requests:?}");
    };
    let FrameRequest::Created { owner, .. } = &requests[0] else {
        unreachable!("the first is made");
    };
    let documents = [
        engine.create_window(same, "https://example.test/b.html", Instant::now()),
        engine.create_window(other, "https://other.test/", Instant::now()),
        engine.create_window(srcdoc, "about:srcdoc", Instant::now()),
    ]
    .map(|document| document.expect("its frame"));
    // Their documents' origins (`same_origin`): an `about:srcdoc`'s its
    // parent's.
    let origins = engine.dom_mut().read(|cx| {
        let top = cx.document_of(owner.get(cx.gc));
        documents
            .each_ref()
            .map(|document| dom_api::same_origin(cx, document.get(cx.gc), top))
    });
    assert_eq!(origins, [true, false, true]);
    run(
        &mut engine,
        "var error = f => { try { f(); return 'no error'; } catch (e) { return e.constructor.name; } };
         log.push(error(() => same.contentWindow.document.URL),
                  error(() => other.contentWindow.document), error(() => srcdoc.contentWindow.document.URL));",
    );
    assert_eq!(log(&mut engine), "no error / DOMException / no error");
    // A new top-level document: the frames of the one before gone.
    engine
        .create_window(
            engine.top_frame(),
            "https://example.test/c/",
            Instant::now(),
        )
        .expect("a window");
    assert!(
        engine
            .create_window(same, "https://example.test/d.html", Instant::now())
            .is_none()
    );
}

/// An iframe the parser inserts gets its browsing context as it is
/// inserted, before the parser goes on (not at the next call).
#[test]
fn the_parsers_iframe_gets_one_as_it_is_inserted() {
    let mut engine = Box::new(V8Engine::new(EngineSettings { web_tests: true }));
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/a/",
            Instant::now(),
        )
        .expect("a window");
    let mut parser = html::parser::Parser::<V8Engine>::new(engine.dom_mut(), &document);
    parser.push("<!DOCTYPE html><body><iframe id=f src=b.html></iframe><p>");
    let (_, back) = parser.resume(engine);
    // Into the same binding: it drops after `document`'s root.
    engine = parser.finish(back);
    let requests = engine.take_frame_requests();
    assert!(
        matches!(
            requests.as_slice(),
            [FrameRequest::Created { .. }, FrameRequest::Navigate { navigation: ChildNavigation::Src(src), .. }]
                if src == "https://example.test/a/b.html"
        ),
        "{requests:?}"
    );
}

/// The parser moving an iframe and a custom element in one insertion (the
/// adoption agency's furthest block): the iframe's steps before the
/// element's `connectedCallback`, as a member's call does them.
#[test]
fn the_parsers_move_runs_the_iframes_steps_first() {
    let mut engine = Box::new(V8Engine::new(EngineSettings { web_tests: true }));
    let document = engine
        .create_window(
            engine.top_frame(),
            "https://example.test/a/",
            Instant::now(),
        )
        .expect("a window");
    run(
        &mut engine,
        "var log = [];
         customElements.define('x-a', class extends HTMLElement {
             connectedCallback() { log.push(String(this.previousSibling.contentWindow === null)); }
         });",
    );
    let mut parser = html::parser::Parser::<V8Engine>::new(engine.dom_mut(), &document);
    parser.push("<!DOCTYPE html><body><b><p><iframe></iframe><x-a></x-a></b>");
    let (_, back) = parser.resume(engine);
    // Into the same binding: it drops after `document`'s root.
    engine = parser.finish(back);
    run(&mut engine, "log.push(document.body.innerHTML);");
    assert_eq!(
        log(&mut engine),
        "false / false / false / <b></b><p><b><iframe></iframe><x-a></x-a></b></p>"
    );
}

/// The windows' timers run together by when they are due (a child's due
/// first runs first, in its realm), the host's wait the earliest of them;
/// a removed iframe's window's never run.
#[test]
fn the_frames_timers_run_by_when_they_are_due() {
    let mut engine = engine();
    assert!(engine.until_next_timer().is_none());
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var g = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow;
         setTimeout(() => log.push('parent'), 30);
         w.setTimeout(function () { log.push('child ' + (this === w)); }, 0);
         g.contentWindow.setTimeout(() => log.push('removed'), 0);
         g.remove();",
    );
    assert_eq!(
        engine.until_next_timer(),
        Some(std::time::Duration::ZERO),
        "the child's"
    );
    std::thread::sleep(std::time::Duration::from_millis(40));
    assert!(engine.run_timers());
    assert_eq!(log(&mut engine), "child true / parent");
    assert!(!engine.run_timers());
    // Only a child's: its wait, and it doesn't run before.
    run(&mut engine, "w.setTimeout(() => log.push('later'), 10000);");
    assert!(
        engine
            .until_next_timer()
            .is_some_and(|wait| wait > std::time::Duration::from_secs(5))
    );
    assert!(!engine.run_timers());
    // An interval due at once: the turn ends, its runs due by the window's
    // now as it began (the first five 0 ms apart, then 4 ms), however long
    // each takes (each 5 ms here; it stops itself after 50).
    run(
        &mut engine,
        "var ticks = 0;
         var tick = w.setInterval(() => {
             log.push('tick');
             const end = Date.now() + 5;
             while (Date.now() < end) {}
             if (++ticks == 50) w.clearInterval(tick);
         }, 0);",
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert!(engine.run_timers());
    run(&mut engine, "w.clearInterval(tick);");
    let ticks = log(&mut engine);
    assert!(
        ticks.starts_with("tick") && ticks.split(" / ").count() < 10,
        "{ticks}"
    );
}

/// The rendering update's animation frame callbacks in tree order: the
/// parent's, then its iframes' as they are in its tree (not as they were
/// made); one asked for only by a child asks the host for a frame.
#[test]
fn animation_frames_run_in_tree_order() {
    let mut engine = engine();
    run(
        &mut engine,
        "var a = document.body.appendChild(document.createElement('iframe'));
         var b = document.body.insertBefore(document.createElement('iframe'), a);
         a.contentWindow.requestAnimationFrame(() => log.push('a'));",
    );
    assert!(engine.has_animation_frames(), "a child's");
    assert_eq!(engine.frames_in_tree_order().len(), 3);
    run(
        &mut engine,
        "b.contentWindow.requestAnimationFrame(() => log.push('b'));
         requestAnimationFrame(() => log.push('parent'));",
    );
    engine.run_animation_frames(Instant::now());
    assert_eq!(log(&mut engine), "parent / b / a");
    assert!(!engine.has_animation_frames());
}

/// The frame tree's walks for its iframes on this thread (debug builds).
fn walks() -> u64 {
    dom_api::frame_tree_walks_for_testing().expect("a debug build")
}

/// A rendering update finds its documents once: each of its steps goes
/// through them (in tree order) without walking the frame tree's
/// documents for their iframes again; after it, they are walked again.
#[test]
fn a_rendering_update_walks_the_frame_tree_once() {
    let mut engine = engine();
    run(
        &mut engine,
        "var a = document.body.appendChild(document.createElement('iframe'));
         var b = document.body.insertBefore(document.createElement('iframe'), a);
         a.contentWindow.requestAnimationFrame(() => log.push('a'));
         b.contentWindow.requestAnimationFrame(() => log.push('b'));",
    );
    let before = walks();
    engine.begin_rendering_update();
    let frames = engine.frames_in_tree_order();
    assert_eq!(frames.len(), 3);
    assert_eq!(engine.frames_in_tree_order(), frames);
    assert_eq!(engine.documents_in_tree_order().len(), 3);
    engine.run_animation_frames(Instant::now());
    engine.run_focus_fixup();
    assert_eq!(walks(), before + 1, "once for the whole update");
    assert_eq!(log(&mut engine), "b / a", "in tree order");
    engine.end_rendering_update();
    engine.frames_in_tree_order();
    assert_eq!(walks(), before + 2, "walked again after it");
}

/// What the host's loop asks at each turn (whether animations want a
/// frame) walks no frame tree, and still sees a child frame's.
#[test]
fn the_hosts_loop_walks_no_frame_tree() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));",
    );
    let before = walks();
    assert!(!engine.animations_want_frames());
    run(
        &mut engine,
        "f.contentDocument.body.animate([{opacity: 0}, {opacity: 1}], 100000);",
    );
    assert!(engine.animations_want_frames(), "the child's animation");
    assert_eq!(walks(), before);
}

/// A frame that goes during a rendering update is left out of the rest of
/// it; one that comes is the next update's (HTML's `docs`, made as the
/// update begins).
#[test]
fn a_rendering_update_keeps_its_documents() {
    let mut engine = engine();
    run(
        &mut engine,
        "var a = document.body.appendChild(document.createElement('iframe'));
         var b = document.body.appendChild(document.createElement('iframe'));",
    );
    engine.begin_rendering_update();
    assert_eq!(engine.frames_in_tree_order().len(), 3);
    run(
        &mut engine,
        "a.remove();
         document.body.appendChild(document.createElement('iframe'));",
    );
    assert_eq!(
        engine.frames_in_tree_order().len(),
        2,
        "a's frame gone, the new one not yet"
    );
    assert_eq!(engine.documents_in_tree_order().len(), 2);
    engine.end_rendering_update();
    assert_eq!(engine.frames_in_tree_order().len(), 3, "the new one's");
}

/// The intersection observers' task of each window in tree order: a
/// child's observer is called back with its entries.
#[test]
fn a_childs_intersection_observers_are_called_back() {
    use dom_api::{ApiCx, ComputedIntersection};
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var d = f.contentDocument;
         new f.contentWindow.IntersectionObserver(entries => log.push('child ' + entries.length))
             .observe(d.body);",
    );
    let requests = engine.take_frame_requests();
    let Some(FrameRequest::Created { document, .. }) = requests.first() else {
        panic!("{requests:?}");
    };
    let queue = engine.dom_mut().mutate(|cx| {
        let document = document.get(cx.gc);
        let child = cx.window_of(document).expect("the child's window");
        let body = cx.last_child(cx.first_child(document).unwrap()).unwrap();
        let rect = Some(gfx::RectF::new(0.0, 0.0, 10.0, 10.0));
        let computed: Vec<ComputedIntersection<'_>> = cx
            .window_intersection_observers(child)
            .into_iter()
            .map(|observer| ComputedIntersection {
                observer,
                target: body,
                target_rect: rect,
                intersection_rect: rect,
                root_bounds: None,
                is_visible: false,
            })
            .collect();
        dom_api::update_intersections(&ApiCx::new(cx), child, &computed, 1.0)
    });
    assert!(queue);
    engine.notify_intersection_observers();
    assert_eq!(log(&mut engine), "child 1");
}

/// A child's element tasks (a text control's `select`) run with the
/// page's.
#[test]
fn a_childs_element_tasks_run() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var d = f.contentDocument;
         var t = d.body.appendChild(d.createElement('input'));
         t.value = 'hello';
         t.addEventListener('select', () => log.push('select'));
         t.setSelectionRange(1, 2);",
    );
    assert!(engine.run_element_tasks());
    assert_eq!(log(&mut engine), "select");
    assert!(!engine.run_element_tasks());
}

/// An event handler attribute of a child frame's element is compiled in
/// that frame's realm (HTML's: the element's relevant global last in its
/// scope): its `document` and `window` are the child frame's.
#[test]
fn a_childs_event_handlers_are_in_its_realm() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var d = f.contentDocument, w = f.contentWindow;
         w.marker = 'child';
         var b = d.body.appendChild(d.createElement('button'));
         b.setAttribute('onclick', 'parent.log.push(String(document === parent.d), String(window.marker))');
         b.click();",
    );
    assert_eq!(log(&mut engine), "true / child");
}

/// HTML's `WindowProxy`: a frame's window stays the same object as it
/// navigates, the window behind it the new document's (what was put on the
/// one before gone), its own `window` the same one; a child frame's
/// initial `about:blank` navigated to a document of its origin keeps its
/// window (what is on it, its listeners), one of another origin doesn't.
#[test]
fn a_frames_window_proxy_stays() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var g = document.body.appendChild(document.createElement('iframe'));
         var h = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow, wg = g.contentWindow, wh = h.contentWindow;
         var initial = f.contentDocument;
         w.marker = 'kept';
         wh.marker = 'kept';
         w.addEventListener('load', () => log.push('load on the window kept'));
         wg.marker = 'kept';",
    );
    let frames: Vec<_> = engine
        .take_frame_requests()
        .into_iter()
        .filter_map(|request| match request {
            FrameRequest::Created { frame, .. } => Some(frame),
            _ => None,
        })
        .collect();
    let [f, g, h] = frames[..] else {
        panic!("{frames:?}");
    };
    // The same origin: the initial `about:blank`'s window kept.
    engine
        .create_window(f, "https://example.test/a/b.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(w === f.contentWindow, w.marker, w.document === f.contentDocument, w.document.URL,
                  String(initial.defaultView));",
    );
    assert_eq!(
        log(&mut engine),
        "true / kept / true / https://example.test/a/b.html / null"
    );
    engine.document_ready(f, dom_api::DocumentReadiness::Loaded);
    assert_eq!(log(&mut engine), "load on the window kept");
    // Again: a new window behind the same proxy.
    engine
        .create_window(f, "https://example.test/a/c.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(w === f.contentWindow, String(w.marker), w.document.URL, w === frames[0]);",
    );
    assert_eq!(
        log(&mut engine),
        "true / undefined / https://example.test/a/c.html / true"
    );
    engine.run_classic_script(
        f,
        &ClassicScript {
            source: "parent.log.push(window === parent.w, self === parent.f.contentWindow)",
            url: "https://example.test/a/c.html",
            base_url: None,
            line: 1,
            muted: false,
            use_credentials: false,
        },
    );
    assert_eq!(log(&mut engine), "true / true");
    // Another origin: a new window, the same proxy.
    engine
        .create_window(g, "https://other.test/", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(wg === g.contentWindow, wg === frames[1]);
         try { log.push(String(wg.marker)); } catch (e) { log.push(e.constructor.name); }",
    );
    assert_eq!(log(&mut engine), "true / true / DOMException");
    // `about:srcdoc`: its parent's origin, the window kept.
    engine
        .create_window(h, "about:srcdoc", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(wh === h.contentWindow, wh.marker, wh.document.URL);",
    );
    assert_eq!(log(&mut engine), "true / kept / about:srcdoc");
}

/// A child's initial window has the page's system focus, as a window the
/// host makes does: kept as it navigates to its origin, it gets the focus
/// events as the focused frame changes.
#[test]
fn a_childs_initial_window_has_the_page_focus() {
    let mut engine = engine();
    engine.set_page_focused(true);
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow;
         w.addEventListener('focus', () => log.push('focus'));
         w.addEventListener('blur', () => log.push('blur'));",
    );
    let frame = engine
        .take_frame_requests()
        .into_iter()
        .find_map(|request| match request {
            FrameRequest::Created { frame, .. } => Some(frame),
            _ => None,
        })
        .expect("made");
    engine
        .create_window(frame, "https://example.test/a/b.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(w === f.contentWindow); w.focus(); focus();",
    );
    assert_eq!(log(&mut engine), "true / focus / blur");
}

/// The page hidden and shown (the browser's `WasHidden`, `WasShown`):
/// every document of the frame tree takes the state, then
/// `visibilitychange` fires at each in tree order (nothing if it was
/// already so); a window made after takes it too.
#[test]
fn the_pages_visibility_reaches_every_frame() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow;
         document.addEventListener('visibilitychange', e => log.push('top ' + document.visibilityState + ' ' + e.bubbles));
         w.document.addEventListener('visibilitychange', () => log.push('child ' + w.document.hidden));",
    );
    engine.take_frame_requests();
    engine.set_page_visible(false);
    assert_eq!(log(&mut engine), "top hidden true / child true");
    engine.set_page_visible(false);
    assert_eq!(log(&mut engine), "");
    run(
        &mut engine,
        "var g = document.body.appendChild(document.createElement('iframe'));
         log.push(g.contentDocument.visibilityState);",
    );
    assert_eq!(log(&mut engine), "hidden");
    engine.set_page_visible(true);
    assert_eq!(log(&mut engine), "top visible true / child false");
}

/// A traversal's filter of a realm that can no longer run script (its
/// iframe's, removed) isn't called: an error.
#[test]
fn a_filter_of_a_removed_frames_realm_isnt_called() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         f.contentWindow.eval('var called = false; var it = document.createNodeIterator(document, -1, () => { called = true; return 1 })');
         var w = f.contentWindow, it = w.it;
         log.push(it.nextNode() === f.contentDocument, w.called);
         w.called = false; f.remove();
         try { it.nextNode(); log.push('no error') } catch (e) { log.push(e.name) }
         log.push(w.called);",
    );
    assert_eq!(log(&mut engine), "true / true / TypeError / false");
}

/// A node's new wrapper is made in its document's realm, wherever script
/// reaches it first (HTML's relevant realm, as Chrome's):
/// a child document, its nodes a walker of the parent's finds or a filter of
/// the parent's is given, a collection, an element's style, attributes,
/// class list and dataset; the parent's own
/// nodes, and a document no frame shows, in the current realm.
#[test]
fn new_wrappers_are_made_in_their_documents_realm() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow, d = f.contentDocument;
         d.body.appendChild(d.createElement('p'));
         var walker = document.createTreeWalker(d, NodeFilter.SHOW_ELEMENT);
         walker.nextNode();
         var filtered;
         document.createTreeWalker(d, NodeFilter.SHOW_ELEMENT, { acceptNode(node) { filtered = node; return 1; } }).nextNode();
         var child = (node, name) => Object.getOwnPropertyDescriptor(Node.prototype, name).get.call(node);
         var p = child(child(child(d, 'firstChild'), 'lastChild'), 'lastChild');
         log.push(d instanceof w.Document, !(d instanceof Document), walker.currentNode instanceof w.Node,
                  !(walker.currentNode instanceof Node), filtered instanceof w.Element,
                  p instanceof w.HTMLParagraphElement,
                  Document.prototype.getElementsByTagName.call(d, 'p') instanceof w.HTMLCollection,
                  Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'style').get.call(p) instanceof w.CSSStyleProperties,
                  Object.getOwnPropertyDescriptor(Element.prototype, 'attributes').get.call(p) instanceof w.NamedNodeMap,
                  Object.getOwnPropertyDescriptor(Element.prototype, 'classList').get.call(p) instanceof w.DOMTokenList,
                  Object.getOwnPropertyDescriptor(HTMLElement.prototype, 'dataset').get.call(p) instanceof w.DOMStringMap,
                  document.body.firstChild instanceof Node,
                  document.implementation.createHTMLDocument('').body instanceof HTMLBodyElement);",
    );
    assert_eq!(
        log(&mut engine),
        "true / true / true / true / true / true / true / true / true / true / true / true / true"
    );
}

/// A frame that navigates on: its document before is unloaded first
/// (`pagehide`, then `unload`, its own frames' after; an initial
/// `about:blank` isn't), and what its realm's script finds by name stays
/// that document's, though its window proxy is the next realm's.
#[test]
fn a_navigated_frames_realm_and_unload() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));",
    );
    let requests = engine.take_frame_requests();
    let Some(FrameRequest::Created { frame, .. }) = requests.first() else {
        panic!("{requests:?}");
    };
    let frame = *frame;
    assert!(
        engine.unload_document(frame).is_none(),
        "the initial about:blank"
    );
    engine
        .create_window(frame, "https://example.test/a/b.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "var w = f.contentWindow, d = f.contentDocument;
         var body = d.appendChild(d.createElement('html')).appendChild(d.createElement('body'));
         body.appendChild(d.createElement('div')).id = 'abc';
         var g = body.appendChild(d.createElement('iframe'));
         var find = new w.Function('return abc');
         w.addEventListener('pagehide', () => log.push('pagehide'));
         w.addEventListener('unload', (e) => log.push(e.type, e.isTrusted, e.cancelable));
         g.contentWindow.addEventListener('unload', () => log.push('inner unload'));
         body.appendChild(d.createElement('img')).src = 'x.png';",
    );
    engine.take_frame_requests();
    // Its image is let go with it.
    let url = "https://example.test/a/x.png";
    engine.perform_microtask_checkpoint();
    let fetches = engine.dom_mut().take_image_fetches();
    assert!(fetches.iter().any(|fetch| fetch.url == url), "{fetches:?}");
    engine.dom_mut().mutate(|cx| {
        let image = dom::Image::Raster(gfx::ImageData::new(vec![0; 4]), gfx::Size::new(1, 1));
        cx.image_fetched(url, Some(image), true);
        assert!(cx.cached_image(url).is_some());
    });
    let (start, end) = engine.unload_document(frame).expect("a document before");
    assert!(engine.dom_mut().read(|cx| cx.cached_image(url).is_none()));
    assert!(start <= end);
    assert_eq!(
        log(&mut engine),
        "pagehide / unload / true / false",
        "the inner frame's initial document isn't unloaded"
    );
    engine
        .create_window(frame, "https://example.test/a/c.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "var e = f.contentDocument; e.appendChild(e.createElement('span')).id = 'abc';
         log.push(find().localName, f.contentWindow.abc.localName, f.contentWindow === w);",
    );
    assert_eq!(log(&mut engine), "div / span / true");
}

/// A removed iframe's document is discarded: the images it used leave the
/// list of available images with it.
#[test]
fn a_removed_frames_images_go() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));",
    );
    let requests = engine.take_frame_requests();
    let Some(FrameRequest::Created { frame, .. }) = requests.first() else {
        panic!("{requests:?}");
    };
    engine
        .create_window(*frame, "https://example.test/a/b.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "var d = f.contentDocument;
         d.appendChild(d.createElement('html')).appendChild(d.createElement('body'))
           .appendChild(d.createElement('img')).src = 'x.png';",
    );
    engine.perform_microtask_checkpoint();
    let url = "https://example.test/a/x.png";
    assert!(
        engine
            .dom_mut()
            .take_image_fetches()
            .iter()
            .any(|fetch| fetch.url == url)
    );
    engine.dom_mut().mutate(|cx| {
        let image = dom::Image::Raster(gfx::ImageData::new(vec![0; 4]), gfx::Size::new(1, 1));
        cx.image_fetched(url, Some(image), true);
    });
    assert!(engine.dom_mut().read(|cx| cx.cached_image(url).is_some()));
    run(&mut engine, "f.remove();");
    assert!(engine.dom_mut().read(|cx| cx.cached_image(url).is_none()));
}

/// No listener of a target whose relevant global's document isn't fully
/// active is called (its iframe removed): a node of it, its window, an
/// `EventTarget` its script made, even one removed in a listener; the
/// dispatch goes on all the same. A document of no window made by a fully
/// active one's script is.
#[test]
fn listeners_of_a_removed_frames_targets_arent_called() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow, d = f.contentDocument;
         var made = new w.EventTarget(), node = d.createElement('p'), other = document.createElement('p');
         var inert = d.implementation.createHTMLDocument('').body;
         var heard = [];
         for (const [name, target] of [['made', made], ['node', node], ['window', w], ['inert', inert]])
           target.addEventListener('x', () => heard.push(name));
         other.addEventListener('x', () => { heard.push('other'); f.remove(); });
         made.dispatchEvent(new Event('x')); other.dispatchEvent(new Event('x'));
         var e = new Event('x', { cancelable: true });
         made.addEventListener('x', (x) => x.preventDefault());
         log.push(heard.join(), made.dispatchEvent(e), e.target === made, e.eventPhase);
         heard = [];
         for (const target of [made, node, w, inert]) target.dispatchEvent(new Event('x'));
         log.push(heard.join() || 'none');
         var mine = document.implementation.createHTMLDocument('').body;
         mine.addEventListener('x', () => log.push('mine')); mine.dispatchEvent(new Event('x'));
         var inner = document.createElement('p'), outer = document.body.appendChild(document.createElement('div'));
         var g = outer.appendChild(document.createElement('iframe')), gd = g.contentDocument;
         var target = gd.body.appendChild(gd.createElement('span'));
         target.addEventListener('x', () => { heard.push('first'); g.remove(); });
         target.addEventListener('x', () => heard.push('second'));
         heard = []; target.dispatchEvent(new Event('x')); log.push(heard.join());",
    );
    assert_eq!(
        log(&mut engine),
        "made,other / true / true / 0 / none / mine / first"
    );
}

/// The global's `document` and `window` are read from the global's cached
/// accessors (V8's, Blink's `[CachedAccessor]`): a loop's reads call no
/// getter, the getter called itself still runs; and they are the window's
/// own, the document a new one once the window's initial `about:blank` is
/// replaced by a document of its origin (the window reused), in code V8
/// optimized for the one before too.
#[test]
fn the_globals_document_is_cached_and_follows_the_window() {
    let mut engine = engine();
    let gets = dom_api::window_document_gets_for_testing().expect("a debug build");
    run(
        &mut engine,
        "var d; for (let i = 0; i < 1000; i++) d = document;
         log.push(d === document, window === self, window.window === window);",
    );
    assert_eq!(dom_api::window_document_gets_for_testing().unwrap(), gets);
    run(
        &mut engine,
        "var get = Object.getOwnPropertyDescriptor(window, 'document').get;
         log.push(get.call(window) === document, get.prototype === undefined);
         try { new get(); } catch (e) { log.push(e instanceof TypeError); }",
    );
    assert_eq!(
        dom_api::window_document_gets_for_testing().unwrap() - gets,
        1
    );
    assert_eq!(log(&mut engine), "true / true / true / true / true / true");
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow, before = w.document;
         var read = new w.Function('let d; for (let i = 0; i < 20000; i++) d = document; return d');
         var mine = new w.Function('return window');
         for (let i = 0; i < 5; i++) read();
         log.push(read() === before, mine() === w);",
    );
    let requests = engine.take_frame_requests();
    let Some(FrameRequest::Created { frame, .. }) = requests.first() else {
        panic!("{requests:?}");
    };
    let frame = *frame;
    engine
        .create_window(frame, "https://example.test/a/next.html", Instant::now())
        .expect("its frame");
    run(
        &mut engine,
        "log.push(w === f.contentWindow, w.document !== before, w.document === f.contentDocument,
                  read() === w.document, w.document.URL, mine() === w);",
    );
    assert_eq!(
        log(&mut engine),
        "true / true / true / true / true / true / https://example.test/a/next.html / true"
    );
}

/// A blob's read is a task of the blob's relevant realm's window (File
/// API's "read the blob"), not of the realm whose `text` was called: one
/// of a removed iframe's blob never settles, though the top window's
/// method read it; the top window's blob read with the iframe's method
/// settles.
#[test]
fn a_blob_read_is_a_task_of_the_blobs_realm() {
    let mut engine = engine();
    run(
        &mut engine,
        "var f = document.body.appendChild(document.createElement('iframe'));
         var w = f.contentWindow;
         var theirs = new w.Blob(['theirs']), ours = new Blob(['ours']);
         var read = w.Blob.prototype.text;
         Blob.prototype.text.call(theirs).then(text => log.push(text));
         read.call(ours).then(text => log.push(text));
         document.body.removeChild(f);",
    );
    engine.perform_microtask_checkpoint();
    engine.run_due_tasks();
    engine.perform_microtask_checkpoint();
    assert_eq!(log(&mut engine), "ours");
}
