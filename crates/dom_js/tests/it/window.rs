//! A realm whose global is a window of the DOM: the window is the
//! global (its global proxy), its unforgeable members on it, its document
//! and location, events' paths through it and their times since its time
//! origin.

use crate::env::Env;

#[test]
fn the_window_is_the_global() {
    let mut env = Env::window("https://example.test/dir/page.html?q#h");
    let cases = [
        (
            "window === globalThis && self === window && frames === window",
            "true",
        ),
        ("parent === window && top === window", "true"),
        ("this === window", "true"),
        ("var top_level = 1; window.top_level", "1"),
        (
            "window instanceof Window && window instanceof EventTarget",
            "true",
        ),
        ("Object.getPrototypeOf(window) === Window.prototype", "true"),
        ("Object.prototype.toString.call(window)", "[object Window]"),
        (
            "document instanceof Document && window.document === document",
            "true",
        ),
        ("document.nodeName", "#document"),
        // The document is the DOM's, its interface objects the realm's.
        ("document.createElement('p') instanceof Element", "true"),
        ("new Event('x') instanceof Event", "true"),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
}

/// `[LegacyUnforgeable]`: on the window itself, not its prototype, not
/// configurable; the others on the window itself too (WebIDL's `[Global]`),
/// configurable, so a script's `var` of the name doesn't replace them.
#[test]
fn unforgeable_members_are_the_windows_own() {
    let mut env = Env::window("about:blank");
    for name in ["window", "document", "location", "top"] {
        let source = format!(
            "var d = Object.getOwnPropertyDescriptor(window, '{name}'); \
             [typeof d.get, d.configurable, '{name}' in Window.prototype].join()"
        );
        assert_eq!(env.eval(&source), "function,false,false", "{name}");
        assert_eq!(
            env.eval(&format!("delete window.{name}; typeof window.{name}")),
            "object",
            "{name} stays"
        );
    }
    for name in ["self", "frames", "parent", "closed", "setTimeout"] {
        assert_eq!(
            env.eval(&format!(
                "var d = Object.getOwnPropertyDescriptor(window, '{name}'); \
                 [typeof (d.get || d.value), d.configurable, '{name}' in Window.prototype].join()"
            )),
            "function,true,false",
            "{name}"
        );
    }
    // A script's `var` of an accessor's name declares nothing new: the
    // name stays the window's (as `var closed` in Chrome), unless it is
    // `[Replaceable]` (`self`), which the assignment then replaces.
    assert_eq!(
        env.eval("var closed = 1; var self = 2; [closed, self].join()"),
        "false,2"
    );
}

/// `location`: the same object, its `href`, `pathname` and `toString` its
/// document's URL's, each its own, not configurable.
#[test]
fn location_is_the_documents_url() {
    let mut env = Env::window("https://example.test/dir/page.html?q#h");
    let cases = [
        (
            "location === window.location && location instanceof Location",
            "true",
        ),
        ("location.href", "https://example.test/dir/page.html?q#h"),
        ("location.pathname", "/dir/page.html"),
        ("String(location)", "https://example.test/dir/page.html?q#h"),
        (
            "['href', 'pathname', 'toString'].map(n => Object.getOwnPropertyDescriptor(location, n).configurable).join()",
            "false,false,false",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(env.eval(source), expected, "{source}");
    }
    let mut blank = Env::window("about:blank");
    assert_eq!(
        blank.eval("[location.href, location.pathname].join()"),
        "about:blank,blank"
    );
    let mut none = Env::window("");
    assert_eq!(none.eval("location.href"), "about:blank");
}

/// A document's parent in an event's path is its window, but for `load`.
#[test]
fn events_go_from_the_document_to_the_window() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "var log = []; var html = document.createElement('html'); document.appendChild(html); \
             window.addEventListener('x', e => log.push('window ' + e.eventPhase + ' ' + (e.currentTarget === window)), true); \
             window.addEventListener('x', e => log.push('window bubble')); \
             document.addEventListener('x', () => log.push('document')); \
             html.dispatchEvent(new Event('x', {bubbles: true})); \
             var path; html.addEventListener('y', e => path = e.composedPath().length); \
             html.dispatchEvent(new Event('y')); \
             window.addEventListener('load', () => log.push('window load')); \
             html.dispatchEvent(new Event('load', {bubbles: true})); \
             [log.join(), path].join('|')"
        ),
        "window 1 true,document,window bubble|3"
    );
    assert_eq!(
        env.eval("var t; window.addEventListener('z', e => t = e.target === window); window.dispatchEvent(new Event('z')); t"),
        "true"
    );
}

/// An event made in the realm counts its time from the window's time
/// origin, which is the window's making: earlier than an event made after
/// a wait, and small.
#[test]
fn an_events_time_counts_from_its_windows_time_origin() {
    let mut env = Env::window("about:blank");
    assert_eq!(
        env.eval(
            "var a = new Event('x').timeStamp; var until = Date.now() + 3; while (Date.now() < until); \
             var b = new Event('x').timeStamp; [a >= 0, a < 1000, b - a >= 1.9].join()"
        ),
        "true,true,true"
    );
    // A window made later in the same isolate (one DOM): its events count
    // from it, not from the DOM's start 20 ms before, so at most the time
    // since it was made (however long the GCs of a stressed run take
    // meanwhile; the clock is coarsened, hence the millisecond).
    std::thread::sleep(std::time::Duration::from_millis(20));
    let made = std::time::Instant::now();
    env.add_window("about:blank");
    let stamp: f64 = env
        .eval("new Event('x').timeStamp")
        .parse()
        .expect("a number");
    let since = made.elapsed().as_secs_f64() * 1e3;
    assert!(
        stamp <= since + 1.0,
        "{stamp} ms, {since} ms since the window"
    );
}
