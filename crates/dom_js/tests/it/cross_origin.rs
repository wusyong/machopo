//! HTML's cross-origin objects: a child window and its `Location` of
//! another origin reached from its parent (and the parent's from it):
//! HTML's CrossOriginProperties, their functions the accessing realm's,
//! the rest a `SecurityError`; a child's `location` navigating it.

use std::time::Instant;

use dom::FrameNavigation;
use dom_api::{
    ChildNavigation, ClassicScript, EngineSettings, FrameId, FrameRequest, ScriptEngine,
};
use dom_js::V8Engine;

/// A window of a document at `https://example.test/a/` with
/// `<html><body>` and `iframes` iframes (`f0`, `f1`...), each child at
/// its URL of `urls`; `log`, `error(f)`. The children's frames.
fn engine(urls: &[&str]) -> (V8Engine, Vec<FrameId>) {
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
         var error = f => {
             try { f(); return 'no error'; } catch (e) { return e.name || e.constructor.name; }
         };
         document.appendChild(document.createElement('html')).appendChild(document.createElement('body'));",
    );
    for index in 0..urls.len() {
        run(
            &mut engine,
            &format!("var f{index} = document.body.appendChild(document.createElement('iframe'));"),
        );
    }
    let frames: Vec<_> = engine
        .take_frame_requests()
        .into_iter()
        .filter_map(|request| match request {
            FrameRequest::Created { frame, .. } => Some(frame),
            _ => None,
        })
        .collect();
    for (&frame, url) in frames.iter().zip(urls) {
        engine
            .create_window(frame, url, Instant::now())
            .expect("its frame");
    }
    (engine, frames)
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

fn run_in(engine: &mut V8Engine, frame: FrameId, url: &str, source: &str) {
    engine.run_classic_script(
        frame,
        &ClassicScript {
            source,
            url,
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

/// The navigations to another document the engine asked for since.
fn navigations(engine: &mut V8Engine) -> Vec<(FrameId, String)> {
    engine
        .take_frame_requests()
        .into_iter()
        .filter_map(|request| match request {
            FrameRequest::Navigate {
                frame,
                navigation: ChildNavigation::Src(url),
            }
            | FrameRequest::Navigation {
                frame,
                navigation: FrameNavigation::To { url, .. },
            } => Some((frame, url)),
            _ => None,
        })
        .collect()
}

/// A window of another origin: its cross-origin attributes and methods
/// (`then` and the symbols undefined), each function the parent's realm's
/// and the same one each time, not the child's own; anything else, a
/// write, a delete or a define a `SecurityError`; its own keys HTML's.
#[test]
fn a_window_of_another_origin_shows_its_cross_origin_properties() {
    let (mut engine, _) = engine(&["https://other.test/"]);
    run(
        &mut engine,
        "var w = f0.contentWindow;
         log.push(w.window === w, w.self === w, w.frames === w, w.top === window, w.parent === window,
                  w.length, w.closed, String(w.opener), String(w.then), String(w[Symbol.toStringTag]));",
    );
    assert_eq!(
        log(&mut engine),
        "true / true / true / true / true / 0 / false / null / undefined / undefined"
    );
    run(
        &mut engine,
        "log.push(typeof w.postMessage, w.postMessage === w.postMessage, w.postMessage !== postMessage,
                  Object.getPrototypeOf(w.postMessage) === Function.prototype, w.postMessage.length,
                  w.close === w.close, w.focus !== w.blur);",
    );
    assert_eq!(
        log(&mut engine),
        "function / true / true / true / 1 / true / true"
    );
    run(
        &mut engine,
        "log.push(error(() => w.document), error(() => w.name), error(() => { w.foo = 1; }),
                  error(() => { w.close = 1; }), error(() => { delete w.close; }),
                  error(() => Object.defineProperty(w, 'x', {value: 1})), error(() => 'document' in w),
                  error(() => w[0]), error(() => Object.setPrototypeOf(w, null)));",
    );
    assert_eq!(
        log(&mut engine),
        "SecurityError / SecurityError / SecurityError / SecurityError / SecurityError / \
         SecurityError / SecurityError / SecurityError / SecurityError"
    );
    // [[GetOwnProperty]]: an attribute's accessors, a method's value, of
    // the parent's realm; [[OwnPropertyKeys]].
    run(
        &mut engine,
        "var d = Object.getOwnPropertyDescriptor(w, 'location');
         var m = Object.getOwnPropertyDescriptor(w, 'postMessage');
         var t = Object.getOwnPropertyDescriptor(w, 'then');
         log.push(typeof d.get, typeof d.set, d.enumerable, d.configurable,
                  Object.getPrototypeOf(d.get) === Function.prototype,
                  m.value === w.postMessage, m.writable, m.enumerable, m.configurable,
                  String(t.value), t.configurable, 'close' in w, String(Object.getPrototypeOf(w)),
                  Object.getOwnPropertyNames(w).join(), Reflect.ownKeys(w).length, Object.keys(w).length);",
    );
    assert_eq!(
        log(&mut engine),
        "function / function / false / true / true / true / false / false / true / undefined / true / \
         true / null / window,self,location,close,closed,focus,blur,frames,length,top,opener,parent,\
         postMessage,then / 17 / 0"
    );
    // The functions' names.
    run(
        &mut engine,
        "log.push(d.get.name, d.set.name, w.postMessage.name, Object.getOwnPropertyDescriptor(w, 'top').get.name);",
    );
    assert_eq!(
        log(&mut engine),
        "get location / set location / postMessage / get top"
    );
    // A getter on another object: refused, as the parent's own members
    // are on the child's window, but those of the cross-origin properties
    // (its own `location` the child's, the same object). Its getter runs
    // in the child's realm: the location's first wrapper made there, so
    // its `href` refused still.
    run(
        &mut engine,
        "log.push(error(() => Object.getOwnPropertyDescriptor(window, 'location').get.call(w).href),
                  error(() => Object.getOwnPropertyDescriptor(w, 'top').get.call({})),
                  error(() => Object.getOwnPropertyDescriptor(window, 'innerWidth').get.call(w)),
                  error(() => window.setTimeout.call(w, () => {})),
                  error(() => queueMicrotask.call(w, () => {})),
                  error(() => window.postMessage.call(w, 1, '*')),
                  Object.getOwnPropertyDescriptor(window, 'location').get.call(w) === w.location);",
    );
    assert_eq!(
        log(&mut engine),
        "SecurityError / TypeError / SecurityError / SecurityError / SecurityError / no error / true"
    );
}

/// A cross-origin window's location: the same object each time; `href`
/// only set, `replace` called, anything else a `SecurityError`.
#[test]
fn a_location_of_another_origin_shows_href_and_replace() {
    let (mut engine, _) = engine(&["https://other.test/"]);
    run(
        &mut engine,
        "var w = f0.contentWindow, l = w.location;
         var d = Object.getOwnPropertyDescriptor(l, 'href');
         log.push(l === w.location, typeof l.replace, l.replace === l.replace, String(d.get), typeof d.set,
                  Object.getOwnPropertyNames(l).join(), String(l.then));
         log.push(error(() => l.href), error(() => l.pathname), error(() => l.assign('x')),
                  error(() => String(l)), error(() => { l.pathname = 'x'; }));",
    );
    assert_eq!(
        log(&mut engine),
        "true / function / true / undefined / function / href,replace,then / undefined / \
         SecurityError / SecurityError / SecurityError / SecurityError / SecurityError"
    );
}

/// The child of another origin's view: its parent's window cross-origin
/// too, a message to it delivered; its own `frameElement` none, its
/// iframe's `contentDocument` none.
#[test]
fn a_child_of_another_origin_reaches_its_parent_cross_origin() {
    let (mut engine, frames) = engine(&["https://other.test/", "https://example.test/b.html"]);
    run(
        &mut engine,
        "addEventListener('message', e => log.push(e.data, e.origin));",
    );
    run_in(
        &mut engine,
        frames[0],
        "https://other.test/",
        "var e = f => { try { f(); return 'no error'; } catch (e) { return e.name; } };
         parent.postMessage([e(() => parent.document), parent === top, String(frameElement),
                             e(() => parent.f0)].join(), '*');",
    );
    engine.run_posted_messages();
    assert_eq!(
        log(&mut engine),
        "SecurityError,true,null,SecurityError / https://other.test"
    );
    run(
        &mut engine,
        "log.push(String(f0.contentDocument), f1.contentDocument === f1.contentWindow.document,
                  f1.contentWindow.frameElement === f1);",
    );
    assert_eq!(log(&mut engine), "null / true / true");
    // To it: `/` is the caller's origin, not the child's.
    run_in(
        &mut engine,
        frames[0],
        "https://other.test/",
        "addEventListener('message', e => parent.postMessage('got ' + e.data, '*'));",
    );
    run(
        &mut engine,
        "f0.contentWindow.postMessage('slash', '/'); f0.contentWindow.postMessage('any', '*');",
    );
    engine.run_posted_messages();
    engine.run_posted_messages();
    assert_eq!(log(&mut engine), "got any / https://other.test");
}

/// A cross-origin window's child windows by index, enumerable, and by
/// their iframes' names, not among its keys; past them a `SecurityError`.
#[test]
fn a_cross_origin_windows_children_are_reached_by_index() {
    let (mut engine, frames) = engine(&["https://other.test/"]);
    run_in(
        &mut engine,
        frames[0],
        "https://other.test/",
        "document.appendChild(document.createElement('html')).appendChild(document.createElement('iframe')).name = 'kid';",
    );
    run(
        &mut engine,
        "var w = f0.contentWindow;
         var d = Object.getOwnPropertyDescriptor(w, 0);
         log.push(w.length, w[0].parent === w, d.value === w[0], d.enumerable, d.writable,
                  Object.keys(w).join(), error(() => w[1]), error(() => { w[0] = 1; }));",
    );
    assert_eq!(
        log(&mut engine),
        "1 / true / true / true / false / 0 / SecurityError / SecurityError"
    );
    run(
        &mut engine,
        "var n = Object.getOwnPropertyDescriptor(w, 'kid');
         log.push(w.kid === w[0], n.value === w[0], n.enumerable, n.writable,
                  Object.getOwnPropertyNames(w).includes('kid'), error(() => w.other));",
    );
    assert_eq!(
        log(&mut engine),
        "true / true / false / false / false / SecurityError"
    );
    // An iframe's empty name isn't one.
    run_in(
        &mut engine,
        frames[0],
        "https://other.test/",
        "document.documentElement.appendChild(document.createElement('iframe')).name = '';",
    );
    run(&mut engine, "log.push(w.length, error(() => w['']));");
    assert_eq!(log(&mut engine), "2 / SecurityError");
}

/// A child's `location` navigates it: `location`'s setter, `href`'s,
/// `assign()`, `replace()`, `document.location`'s setter, the URL resolved
/// against the caller's document (the entry realm's), from its parent
/// across origins too; a URL that doesn't parse a `SyntaxError`; the
/// top-level window's too (its host's to do: the browser's).
#[test]
fn a_childs_location_navigates_it() {
    let (mut engine, frames) = engine(&["https://example.test/b/c.html", "https://other.test/x/"]);
    let [same, other] = frames[..] else {
        unreachable!("two");
    };
    run(
        &mut engine,
        "f0.contentWindow.location = 'd.html';
         f0.contentWindow.location.href = '/e.html';
         f0.contentWindow.location.assign('f.html');
         f0.contentWindow.location.replace('g.html');
         f1.contentWindow.location = 'h.html';
         f1.contentWindow.location.href = 'https://other.test/i';
         f1.contentWindow.location.replace('j.html');
         log.push(error(() => { f0.contentWindow.location.href = 'http://[x'; }));",
    );
    assert_eq!(log(&mut engine), "SyntaxError");
    assert_eq!(
        navigations(&mut engine),
        [
            (same, "https://example.test/a/d.html".to_owned()),
            (same, "https://example.test/e.html".to_owned()),
            (same, "https://example.test/a/f.html".to_owned()),
            (same, "https://example.test/a/g.html".to_owned()),
            (other, "https://example.test/a/h.html".to_owned()),
            (other, "https://other.test/i".to_owned()),
            (other, "https://example.test/a/j.html".to_owned()),
        ]
    );
    // `document.location`: its window's, forwarded to; none without one
    // (setting it a `TypeError`).
    run(
        &mut engine,
        "var lone = document.implementation.createHTMLDocument('');
         log.push(document.location === location, f0.contentDocument.location === f0.contentWindow.location,
                  String(lone.location), error(() => { lone.location = 'x'; }));
         f0.contentDocument.location = 'm.html';",
    );
    assert_eq!(log(&mut engine), "true / true / null / TypeError");
    assert_eq!(
        navigations(&mut engine),
        [(same, "https://example.test/a/m.html".to_owned())]
    );
    // From the child: against its own document.
    run_in(
        &mut engine,
        other,
        "https://other.test/x/",
        "location.href = 'k.html';",
    );
    assert_eq!(
        navigations(&mut engine),
        [(other, "https://other.test/x/k.html".to_owned())]
    );
    // A child's entries aren't the session history's (its host's).
    run(
        &mut engine,
        "f0.contentWindow.history.pushState(null, ''); log.push(history.length);",
    );
    assert_eq!(log(&mut engine), "1");
    engine.take_frame_requests();
    run(&mut engine, "location.href = 'l.html';");
    let top = engine.top_frame();
    assert_eq!(
        navigations(&mut engine),
        [(top, "https://example.test/a/l.html".to_owned())]
    );
}

/// The window's `close()` and `blur()` do nothing, `opener` is none,
/// `closed` whether its iframe is gone; a child's `focus()` makes it the
/// focused frame (its iframe the parent's active element).
#[test]
fn a_windows_close_focus_and_blur() {
    let (mut engine, _) = engine(&["https://example.test/b.html", "https://other.test/"]);
    run(
        &mut engine,
        "var w = f0.contentWindow, o = f1.contentWindow;
         close(); blur(); w.blur();
         log.push(closed, String(opener), w.closed, document.activeElement === document.body);
         w.focus();
         log.push(document.activeElement === f0);
         o.focus();
         log.push(document.activeElement === f1);
         focus();
         log.push(document.activeElement === document.body);
         f0.remove(); f1.remove();
         log.push(w.closed, o.closed);",
    );
    assert_eq!(
        log(&mut engine),
        "false / null / false / true / true / true / true / true / true"
    );
}

/// A cross-origin property's getter is called in its receiver's realm,
/// whichever realm's script reads it: the object it gives, when that has
/// no wrapper yet, is made there (a child window's `location`, first read
/// by its parent, is the child's realm's `Location`).
#[test]
fn a_cross_origin_getters_object_is_its_receivers_realms() {
    let (mut engine, _) = engine(&["https://example.test/b.html"]);
    run(
        &mut engine,
        "var w = f0.contentWindow; var l = w.location;
         log.push(Object.getPrototypeOf(l) === w.Location.prototype,
                  Object.getPrototypeOf(l) === Location.prototype,
                  l instanceof w.Location, l === w.location, l.href);",
    );
    assert_eq!(
        log(&mut engine),
        "true / false / true / true / https://example.test/b.html"
    );
}
