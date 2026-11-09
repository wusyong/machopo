//! `Location`'s members and their navigations, `History` (its state, the
//! session history the host gives, its traversals), a navigation to a
//! fragment (its URL, `:target`, `popstate` as the call ends, `hashchange`
//! in a task), the host's traversal and restored state, and the
//! `PopStateEvent` and `HashChangeEvent` interfaces: in an engine's
//! top-level frame, whose navigations are the frame requests it gives.

use std::time::Instant;

use dom::{FrameNavigation, SessionHistory};
use dom_api::{ClassicScript, DocumentReadiness, EngineSettings, FrameRequest, ScriptEngine};
use dom_js::V8Engine;

pub(crate) struct Page {
    pub(crate) engine: V8Engine,
}

impl Page {
    pub(crate) fn new(url: &str) -> Self {
        let mut engine = V8Engine::new(EngineSettings { web_tests: true });
        let document = engine
            .create_window(engine.top_frame(), url, Instant::now())
            .expect("a window");
        drop(document);
        let mut page = Self { engine };
        page.eval("document.appendChild(document.createElement('html')).appendChild(document.createElement('body')); 0");
        page
    }

    /// `source`'s completion value as a string, or the name of what it
    /// throws, `!` first.
    pub(crate) fn eval(&mut self, source: &str) -> String {
        let source = format!(
            "testRunner.reportHarnessResults((() => {{ try {{ return String((0, eval)({source:?})); }} \
             catch (e) {{ return '!' + e.name; }} }})())"
        );
        self.engine.run_classic_script(
            self.engine.top_frame(),
            &ClassicScript {
                source: &source,
                url: "https://example.test/",
                base_url: None,
                line: 1,
                muted: false,
                use_credentials: false,
            },
        );
        self.engine.perform_microtask_checkpoint();
        self.engine.take_harness_results().expect("reported")
    }

    /// The navigations the engine asked for since.
    pub(crate) fn navigations(&mut self) -> Vec<FrameNavigation> {
        self.engine
            .take_frame_requests()
            .into_iter()
            .filter_map(|request| match request {
                FrameRequest::Navigation { navigation, .. } => Some(navigation),
                _ => None,
            })
            .collect()
    }

    /// The document's `load` fired: completely loaded.
    pub(crate) fn load(&mut self) {
        let top = self.engine.top_frame();
        self.engine.document_ready(top, DocumentReadiness::Loaded);
    }
}

pub(crate) fn to(url: &str, replace: bool) -> FrameNavigation {
    FrameNavigation::To {
        url: url.to_owned(),
        replace,
    }
}

#[test]
fn location_members_and_their_navigations() {
    let mut page = Page::new("https://user@example.test:8080/a/b.html?q=1#f");
    assert_eq!(
        page.eval(
            "[location.href, location.origin, location.protocol, location.host, location.hostname, location.port, \
             location.pathname, location.search, location.hash, String(location)].join(' ')"
        ),
        "https://user@example.test:8080/a/b.html?q=1#f https://example.test:8080 https: example.test:8080 \
         example.test 8080 /a/b.html ?q=1 #f https://user@example.test:8080/a/b.html?q=1#f"
    );
    // Before the document is completely loaded each replaces the entry.
    for (source, url) in [
        (
            "location.search = 'r=2'",
            "https://user@example.test:8080/a/b.html?r=2#f",
        ),
        (
            "location.pathname = '/c'",
            "https://user@example.test:8080/c?q=1#f",
        ),
        (
            "location.port = '9'",
            "https://user@example.test:9/a/b.html?q=1#f",
        ),
        (
            "location.hostname = 'other.test'",
            "https://user@other.test:8080/a/b.html?q=1#f",
        ),
        (
            "location.host = 'other.test:7'",
            "https://user@other.test:7/a/b.html?q=1#f",
        ),
        (
            "location.protocol = 'http'",
            "http://user@example.test:8080/a/b.html?q=1#f",
        ),
        ("location.assign('/d')", "https://user@example.test:8080/d"),
    ] {
        page.eval(source);
        assert_eq!(page.navigations(), [to(url, true)], "{source}");
    }
    assert_eq!(page.eval("location.protocol = 'x y'"), "!SyntaxError");
    // Not HTTP(S): nothing.
    page.eval("location.protocol = 'ftp'");
    assert_eq!(page.navigations(), []);
    page.eval("location.reload()");
    assert_eq!(page.navigations(), [FrameNavigation::Reload]);
    page.load();
    page.eval("location.href = 'e.html'");
    assert_eq!(
        page.navigations(),
        [to("https://user@example.test:8080/a/e.html", false)]
    );
    page.eval("location.replace('e.html')");
    assert_eq!(
        page.navigations(),
        [to("https://user@example.test:8080/a/e.html", true)]
    );
    assert_eq!(page.eval("location.href = 'http://[x'"), "!SyntaxError");
}

/// A navigation to a fragment: the URL and `:target` at once, `popstate`
/// (its state `null`) as the setter returns, `hashchange` in a task; the
/// same fragment again navigates nowhere.
#[test]
fn a_fragment_navigates_within_the_document() {
    let mut page = Page::new("https://example.test/a.html#one");
    page.load();
    page.eval("history.replaceState({ s: 1 }, ''); 0");
    page.navigations();
    page.eval(
        "var log = []; document.body.innerHTML = '<p id=two></p><a name=three></a>'; \
         addEventListener('popstate', (e) => log.push('popstate ' + e.state + ' ' + e.isTrusted + ' ' + e.bubbles)); \
         addEventListener('hashchange', (e) => log.push('hashchange ' + e.oldURL + ' ' + e.newURL + ' ' + (e instanceof HashChangeEvent))); 0",
    );
    assert_eq!(
        page.eval(
            "location.hash = 'two'; log.push(location.hash, document.querySelector(':target').id, history.state); log.join()"
        ),
        "popstate null true false,#two,two,",
        "the new entry's state is null"
    );
    assert_eq!(
        page.navigations(),
        [FrameNavigation::SameDocument {
            url: "https://example.test/a.html#two".to_owned(),
            replace: false,
            state: None,
            scroll: true,
            pop_state: true,
        }]
    );
    assert_eq!(page.eval("history.length"), "2", "a new entry");
    assert!(page.engine.run_element_tasks());
    assert_eq!(
        page.eval("log.slice(4).join()"),
        "hashchange https://example.test/a.html#one https://example.test/a.html#two true"
    );
    // An `<a>`'s name; percent-decoded; the same fragment: nothing.
    assert_eq!(
        page.eval("location.hash = 'three'; document.querySelector(':target').tagName"),
        "A"
    );
    page.eval("location.hash = '#three'");
    page.navigations();
    page.eval("location.hash = 'three'");
    assert_eq!(page.navigations(), []);
    // The URL again: within the document, replacing the entry, no
    // `hashchange`.
    while page.engine.run_element_tasks() {}
    page.eval("location.href = location.href");
    assert!(matches!(
        &page.navigations()[..],
        [FrameNavigation::SameDocument { replace: true, .. }]
    ));
    assert!(!page.engine.run_element_tasks());
    assert_eq!(
        page.eval("location.hash = 'nothing'; String(document.querySelector(':target'))"),
        "null"
    );
}

#[test]
fn history_members() {
    let mut page = Page::new("https://example.test/a/b.html");
    page.load();
    page.engine.set_session_history(SessionHistory {
        index: 2,
        length: 5,
    });
    assert_eq!(
        page.eval("[history.length, history.state, history.scrollRestoration].join()"),
        "5,,auto"
    );
    // `pushState`: the URL and state at once; a new entry, those forward
    // of it gone; `history.state` a copy, the same each time.
    assert_eq!(
        page.eval(
            "var o = { n: [1] }; history.pushState(o, '', 'c.html?x'); \
             [location.href, history.state.n[0], history.state === history.state, history.state !== o, history.length].join()"
        ),
        "https://example.test/a/c.html?x,1,true,true,4"
    );
    let [
        FrameNavigation::SameDocument {
            url,
            replace,
            state,
            scroll,
            pop_state,
        },
    ] = &page.navigations()[..]
    else {
        panic!("one navigation within the document");
    };
    assert_eq!(
        (url.as_str(), *replace, state.is_some(), *scroll, *pop_state),
        ("https://example.test/a/c.html?x", false, true, false, false)
    );
    assert_eq!(
        page.eval("var s = history.state; history.replaceState(2, '', ''); [history.state, s !== history.state, location.href, history.length].join()"),
        "2,true,https://example.test/a/c.html?x,4"
    );
    assert!(matches!(
        &page.navigations()[..],
        [FrameNavigation::SameDocument { replace: true, .. }]
    ));
    for (source, error) in [
        (
            "history.pushState(null, '', 'https://other.test/')",
            "!SecurityError",
        ),
        ("history.pushState(null, '', 'http://[x')", "!SecurityError"),
        ("history.pushState(() => 1, '')", "!DataCloneError"),
        ("history.pushState(document, '')", "!DataCloneError"),
    ] {
        assert_eq!(page.eval(source), error, "{source}");
    }
    assert_eq!(page.navigations(), []);
    // Traversals within range as the host gave it; `go(0)` reloads.
    page.engine.set_session_history(SessionHistory {
        index: 1,
        length: 3,
    });
    page.eval("history.go(-1); history.go(-2); history.forward(); history.back(); history.go(2); history.go(); history.go(0)");
    assert_eq!(
        page.navigations(),
        [
            FrameNavigation::GoToOffset(-1),
            FrameNavigation::GoToOffset(1),
            FrameNavigation::GoToOffset(-1),
            FrameNavigation::Reload,
            FrameNavigation::Reload,
        ]
    );
    assert_eq!(page.eval("history.scrollRestoration = 'manual'; history.scrollRestoration = 'nope'; history.scrollRestoration"), "manual");
    assert_eq!(
        page.navigations(),
        [FrameNavigation::ScrollRestoration { manual: true }]
    );
}

/// The host's traversal: the URL, the entry's state (`popstate`'s, the
/// same object as `history.state`) and mode, `:target`, `hashchange` if
/// the fragment changed; a new document's state restored at its commit.
#[test]
fn the_hosts_traversal_and_restored_state() {
    let mut page = Page::new("https://example.test/a.html");
    page.load();
    page.eval("history.pushState({ n: 7 }, '', '#x'); 0");
    let Some(FrameNavigation::SameDocument {
        state: Some(state), ..
    }) = page.navigations().pop()
    else {
        panic!("a state");
    };
    page.eval(
        "var log = []; document.body.innerHTML = '<p id=y></p>'; history.replaceState(null, ''); \
         addEventListener('popstate', (e) => log.push(e.state.n, e.state === history.state, location.hash, \
           document.querySelector(':target').id, history.scrollRestoration)); \
         addEventListener('hashchange', () => log.push('hashchange')); 0",
    );
    let top = page.engine.top_frame();
    assert!(page.engine.traverse_within_document(
        top,
        "https://example.test/a.html#y",
        Some(state.clone()),
        true
    ));
    assert_eq!(page.eval("log.join()"), "7,true,#y,y,manual");
    assert!(page.engine.run_element_tasks());
    assert_eq!(page.eval("log.slice(5).join()"), "hashchange");
    // At a new document's commit.
    let mut other = Page::new("https://example.test/b.html");
    let top = other.engine.top_frame();
    other.engine.restore_history(top, Some(state), true);
    assert_eq!(
        other.eval("[history.state.n, history.scrollRestoration].join()"),
        "7,manual"
    );
}

#[test]
fn pop_state_and_hash_change_events() {
    let mut page = Page::new("https://example.test/");
    assert_eq!(
        page.eval(
            "var o = {}; var p = new PopStateEvent('popstate', { state: o, hasUAVisualTransition: true, bubbles: true }); \
             [p instanceof PopStateEvent, p.state === o, p.hasUAVisualTransition, p.bubbles, new PopStateEvent('x').state].join()"
        ),
        "true,true,true,true,"
    );
    assert_eq!(
        page.eval(
            "var h = new HashChangeEvent('hashchange', { oldURL: 'a', newURL: 'b' }); \
             [h instanceof HashChangeEvent, h.oldURL, h.newURL, new HashChangeEvent('x').oldURL === ''].join()"
        ),
        "true,a,b,true"
    );
    assert_eq!(page.eval("PopStateEvent('x')"), "!TypeError");
}
